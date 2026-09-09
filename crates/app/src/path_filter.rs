//! A coalesced, bounded in-memory path index. Rendering only shares ready row
//! identities; Unicode normalization and matching happen on one background lane.
use crate::{FileChange, GitTurtle, SerialExecutor};
use anyhow::{Result, ensure};
use gpui_kit::{App, Context, ScrollStrategy, Task};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

pub(super) const MAX_PATHS: usize = 100_000;
pub(super) const MAX_INDEX_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MAX_QUERY_BYTES: usize = 4096;

pub(super) type Index = Vec<[String; 2]>;
#[derive(Default)]
pub(super) struct State {
    executor: Option<SerialExecutor>,
    cancellation: Arc<AtomicU64>,
    identity: Option<(usize, usize)>,
    raw: Option<Arc<Vec<FileChange>>>,
    index: Option<Arc<Index>>,
    visible: Arc<[usize]>,
    pub pending: bool,
    pub error: Option<String>,
    task: Option<Task<()>>,
}
impl State {
    pub(super) fn executor(&mut self) -> &SerialExecutor {
        self.executor
            .get_or_insert_with(|| SerialExecutor::new("gitturtle-path-filter"))
    }
    pub fn reset(&mut self) {
        self.cancellation.fetch_add(1, Ordering::Relaxed);
        self.identity = None;
        self.raw = None;
        self.index = None;
        self.visible = Arc::from([]);
        self.pending = false;
        self.error = None;
        self.task = None;
    }
}

pub(super) fn checkpoint(cancellation: &AtomicU64, ticket: u64) -> Result<()> {
    ensure!(
        cancellation.load(Ordering::Relaxed) == ticket,
        "Path filter superseded"
    );
    Ok(())
}
fn prepare_index(files: &[FileChange], cancellation: &AtomicU64, ticket: u64) -> Result<Index> {
    ensure!(
        files.len() <= MAX_PATHS,
        "Path filtering supports up to 100,000 changed files. Clear the filter to browse every file."
    );
    prepare_paths(
        files
            .iter()
            .map(|file| [file.old_path.as_deref(), file.new_path.as_deref()]),
        cancellation,
        ticket,
    )
}
pub(super) fn prepare_paths<'a>(
    paths: impl Iterator<Item = [Option<&'a std::path::Path>; 2]>,
    cancellation: &AtomicU64,
    ticket: u64,
) -> Result<Index> {
    let mut bytes = 0;
    let mut index = Vec::new();
    for (i, paths) in paths.enumerate() {
        ensure!(
            i < MAX_PATHS,
            "Path filtering supports up to 100,000 changed files. Clear the filter to browse every file."
        );
        if i % 128 == 0 {
            checkpoint(cancellation, ticket)?;
        }
        let mut entry = [String::new(), String::new()];
        for (side, path) in paths.into_iter().enumerate() {
            if let Some(path) = path {
                let display = path.to_string_lossy();
                ensure!(
                    display.len() <= 64 * 1024,
                    "A changed path exceeds the 64 KiB filter limit. Clear the filter to browse every file."
                );
                ensure!(
                    display.len() <= MAX_INDEX_BYTES - bytes,
                    "Changed paths exceed the 16 MiB filter limit. Clear the filter to browse every file."
                );
                entry[side] = display.to_lowercase();
                bytes += entry[side].len();
                ensure!(
                    bytes <= MAX_INDEX_BYTES,
                    "Changed paths exceed the 16 MiB filter limit. Clear the filter to browse every file."
                );
            }
        }
        index.push(entry);
    }
    Ok(index)
}
pub(super) fn matching(
    index: &Index,
    query: &str,
    cancellation: &AtomicU64,
    ticket: u64,
) -> Result<Arc<[usize]>> {
    ensure!(
        query.len() <= MAX_QUERY_BYTES,
        "Path filter queries must be at most 4,096 bytes."
    );
    let query = query.to_lowercase();
    let mut rows = Vec::new();
    for (i, paths) in index.iter().enumerate() {
        if i % 128 == 0 {
            checkpoint(cancellation, ticket)?;
        }
        if paths.iter().any(|path| path.contains(&query)) {
            rows.push(i);
        }
    }
    checkpoint(cancellation, ticket)?;
    Ok(rows.into())
}

impl GitTurtle {
    pub(super) fn filtered_file_indices(&self, _: &App) -> Arc<[usize]> {
        if self.file_paths.identity == Some((self.files.as_ptr() as usize, self.files.len())) {
            Arc::clone(&self.file_paths.visible)
        } else {
            Arc::from([])
        }
    }

    /// Every replacement of the owned changed-file snapshot calls this hook.
    /// The identity guard also prevents a missed clear from exposing stale rows.
    pub(super) fn refresh_file_filter(&mut self, cx: &mut Context<Self>) {
        let identity = (self.files.as_ptr() as usize, self.files.len());
        if self.file_paths.identity != Some(identity) {
            self.file_paths.reset();
            self.file_paths.identity = Some(identity);
        }
        self.schedule_file_filter(cx);
    }

    pub(super) fn change_file_filter(&mut self, cx: &mut Context<Self>) {
        self.refresh_file_filter(cx);
        self.file_scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    fn schedule_file_filter(&mut self, cx: &mut Context<Self>) {
        let query = self.file_filter.read(cx).value().to_string();
        let ticket = self.file_paths.cancellation.fetch_add(1, Ordering::Relaxed) + 1;
        self.file_paths.task = None;
        self.file_paths.error = None;
        self.file_paths.visible = Arc::from([]);
        self.file_paths.pending = false;
        if query.is_empty() {
            self.file_paths.visible = (0..self.files.len()).collect::<Vec<_>>().into();
            cx.notify();
            return;
        }
        if query.len() > MAX_QUERY_BYTES || self.files.len() > MAX_PATHS {
            self.file_paths.error = Some("Path filtering supports 100,000 changed files and queries up to 4,096 bytes. Clear the filter to browse every file.".into());
            cx.notify();
            return;
        }
        if self.files.is_empty() {
            cx.notify();
            return;
        }
        if self.file_paths.index.is_none() && self.file_paths.raw.is_none() {
            let bytes = self
                .files
                .iter()
                .flat_map(|file| [file.old_path.as_deref(), file.new_path.as_deref()])
                .flatten()
                .map(|path| path.as_os_str().len())
                .fold(0usize, usize::saturating_add);
            if bytes > MAX_INDEX_BYTES {
                self.file_paths.error = Some("Changed paths exceed the 16 MiB filter limit. Clear the filter to browse every file.".into());
                cx.notify();
                return;
            }
            self.file_paths.raw = Some(Arc::new(self.files.clone()));
        }
        let raw = self.file_paths.raw.clone();
        let index = self.file_paths.index.clone();
        let cancellation = Arc::clone(&self.file_paths.cancellation);
        let identity = self.file_paths.identity;
        let response = self.file_paths.executor().submit_read(move || {
            checkpoint(&cancellation, ticket)?;
            let index = match index {
                Some(index) => index,
                None => Arc::new(prepare_index(
                    raw.as_deref().map(Vec::as_slice).unwrap_or_default(),
                    &cancellation,
                    ticket,
                )?),
            };
            let visible = matching(&index, &query, &cancellation, ticket)?;
            Ok((index, visible))
        });
        self.file_paths.pending = true;
        self.file_paths.task = Some(cx.spawn(async move |this, cx| {
            let result = response.await;
            let _ = this.update(cx, |this, cx| {
                if this.file_paths.cancellation.load(Ordering::Relaxed) != ticket
                    || this.file_paths.identity != identity
                    || identity != Some((this.files.as_ptr() as usize, this.files.len()))
                {
                    return;
                }
                this.file_paths.pending = false;
                this.file_paths.task = None;
                match result {
                    Ok(Ok((index, visible))) => {
                        this.file_paths.index = Some(index);
                        this.file_paths.raw = None;
                        if this
                            .selected_file
                            .is_some_and(|selected| !visible.contains(&selected))
                        {
                            this.invalidate_read();
                            this.clear_preview();
                            this.selected_file = None;
                        }
                        this.file_paths.visible = visible;
                    }
                    Ok(Err(error)) => this.file_paths.error = Some(error.to_string()),
                    Err(_) => {
                        this.file_paths.error =
                            Some("Path filtering stopped. Edit or clear the query to retry.".into())
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{Index, MAX_QUERY_BYTES, checkpoint, matching, prepare_index};
    use std::sync::atomic::AtomicU64;
    #[test]
    fn matching_keeps_original_indices_and_covers_unicode_renamed_paths() {
        let index: Index = vec![
            ["src/old.rs".into(), "lib/new.rs".into()],
            ["tést/🐢.rs".into(), String::new()],
            ["README.md".to_lowercase(), String::new()],
        ];
        let cancellation = AtomicU64::new(4);
        assert_eq!(
            matching(&index, "OLD", &cancellation, 4).unwrap().as_ref(),
            &[0]
        );
        assert_eq!(
            matching(&index, "new.rs", &cancellation, 4)
                .unwrap()
                .as_ref(),
            &[0]
        );
        assert_eq!(
            matching(&index, "TÉST/🐢", &cancellation, 4)
                .unwrap()
                .as_ref(),
            &[1]
        );
        assert!(
            matching(&index, "absent", &cancellation, 4)
                .unwrap()
                .is_empty()
        );
        assert!(matching(&index, &"x".repeat(MAX_QUERY_BYTES + 1), &cancellation, 4).is_err());
    }
    #[test]
    fn superseded_work_does_not_publish_rows_from_previous_query_or_snapshot() {
        let generation = AtomicU64::new(3);
        assert!(checkpoint(&generation, 2).is_err());
        assert!(
            matching(
                &vec![["file".into(), String::new()]; 512],
                "file",
                &generation,
                2
            )
            .is_err()
        );
    }

    #[test]
    fn prepared_search_metadata_leaves_byte_safe_operation_targets_unchanged() {
        use gitturtle_core::{ChangeStatus, FileChange};
        let mut file = FileChange {
            old_oid: None,
            new_oid: None,
            old_mode: "100644".into(),
            new_mode: "100644".into(),
            status: ChangeStatus::Renamed,
            old_path: Some("old/École.rs".into()),
            new_path: Some("new/🐢.rs".into()),
        };
        let original = file.clone();
        let generation = AtomicU64::new(1);
        let index = prepare_index(std::slice::from_ref(&file), &generation, 1).unwrap();
        assert_eq!(
            matching(&index, "éCOLE", &generation, 1).unwrap().as_ref(),
            &[0]
        );
        assert_eq!(
            matching(&index, "🐢", &generation, 1).unwrap().as_ref(),
            &[0]
        );
        assert_eq!(file, original);
        file.new_path = Some("x".repeat(64 * 1024 + 1).into());
        assert!(prepare_index(&[file], &generation, 1).is_err());
    }
}
