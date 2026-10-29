//! A coalesced, bounded in-memory path index. Rendering only shares ready row
//! identities; Unicode normalization and matching happen on one background lane.
use crate::{FileChange, GitTurtle, SerialExecutor, WorkspaceMode};
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
/// The filter worker's answer: the index it prepared and the rows matching.
type Answer =
    std::result::Result<Result<(Arc<Index>, Arc<[usize]>)>, futures::channel::oneshot::Canceled>;
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
        // Working Changes owns a separate status-path filter. Its one-file
        // preview snapshot must not be hidden by a retained History query.
        let query = if self.mode == WorkspaceMode::Working {
            String::new()
        } else {
            self.file_filter.read(cx).value().to_string()
        };
        let ticket = self.file_paths.cancellation.fetch_add(1, Ordering::Relaxed) + 1;
        self.file_paths.task = None;
        self.file_paths.error = None;
        self.file_paths.pending = false;
        if query.is_empty() {
            self.file_paths.visible = (0..self.files.len()).collect::<Vec<_>>().into();
            cx.notify();
            return;
        }
        if query.len() > MAX_QUERY_BYTES || self.files.len() > MAX_PATHS {
            self.file_paths.visible = Arc::from([]);
            self.file_paths.error = Some("Path filtering supports 100,000 changed files and queries up to 4,096 bytes. Clear the filter to browse every file.".into());
            cx.notify();
            return;
        }
        if self.files.is_empty() {
            self.file_paths.visible = Arc::from([]);
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
                self.file_paths.visible = Arc::from([]);
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
        // The rows the last answer left stay drawn until this query's answer
        // lands, so typing never shows an empty list or a false "No
        // matches" in between; a fresh snapshot has none to keep and shows
        // the pending state.
        self.file_paths.pending = true;
        self.file_paths.task = Some(cx.spawn(async move |this, cx| {
            let result = response.await;
            let _ = this.update(cx, |this, cx| {
                this.land_file_filter(ticket, identity, result, cx)
            });
        }));
        cx.notify();
    }

    /// Takes the filter's answer for the query `ticket` numbered, asked of
    /// the snapshot `identity` names. An answer to an older query, or to a
    /// snapshot since replaced, is dropped, keeping the rows drawn.
    fn land_file_filter(
        &mut self,
        ticket: u64,
        identity: Option<(usize, usize)>,
        result: Answer,
        cx: &mut Context<Self>,
    ) {
        if self.file_paths.cancellation.load(Ordering::Relaxed) != ticket
            || self.file_paths.identity != identity
            || identity != Some((self.files.as_ptr() as usize, self.files.len()))
            || self.mode == WorkspaceMode::Working
        {
            return;
        }
        self.file_paths.pending = false;
        self.file_paths.task = None;
        match result {
            Ok(Ok((index, visible))) => {
                self.file_paths.index = Some(index);
                self.file_paths.raw = None;
                if self
                    .selected_file
                    .is_some_and(|selected| !visible.contains(&selected))
                {
                    self.invalidate_read();
                    self.clear_preview();
                    self.selected_file = None;
                }
                self.file_paths.visible = visible;
            }
            Ok(Err(error)) => {
                self.file_paths.visible = Arc::from([]);
                self.file_paths.error = Some(error.to_string());
            }
            Err(_) => {
                self.file_paths.visible = Arc::from([]);
                self.file_paths.error =
                    Some("Path filtering stopped. Edit or clear the query to retry.".into());
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{Index, MAX_QUERY_BYTES, checkpoint, matching, prepare_index};
    use gpui_kit as gpui;
    use std::sync::atomic::AtomicU64;

    #[gpui::test]
    async fn working_preview_survives_retained_history_filter_and_late_reply(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::{
            AppPage, Content, FileChange, GitTurtle, Output, Preferences, WorkspaceMode, activity,
            recovery_drafts, repository_tabs,
        };
        use gitturtle_core::ChangeStatus;
        use gpui::{AppContext, Entity, component::Root};
        use std::{cell::RefCell, rc::Rc, sync::Arc, sync::mpsc, time::Duration};

        fn file(path: &str) -> FileChange {
            FileChange {
                old_oid: None,
                new_oid: None,
                old_mode: "000000".into(),
                new_mode: "100644".into(),
                status: ChangeStatus::Added,
                old_path: None,
                new_path: Some(path.into()),
            }
        }

        cx.executor().allow_parking();
        crate::views::tests::init_app(cx);
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let observed = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                GitTurtle::new(
                    None,
                    Preferences::default(),
                    repository_tabs::Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                )
            });
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = observed.borrow_mut().take().unwrap();
        let (release, blocked) = cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                let (release, wait) = mpsc::channel();
                let blocked = app.file_paths.executor().submit_read(move || {
                    wait.recv()?;
                    Ok(())
                });
                app.files = vec![file("external.glb"), file("other.glb")];
                app.selected_file = Some(0);
                app.file_filter.update(cx, |input, cx| {
                    input.set_value("external", window, cx);
                });
                app.refresh_file_filter(cx);
                assert!(app.file_paths.pending);
                app.retained_history_files =
                    Some((std::mem::take(&mut app.files), app.selected_file.take()));
                app.mode = WorkspaceMode::Working;
                app.page = AppPage::Repository;
                for path in ["working-added.GLB", "assembly.glb"] {
                    app.receive(
                        Output::WorkingPreview(
                            file(path),
                            Arc::new(Content::Notice("Captured working comparison".into())),
                            Duration::ZERO,
                        ),
                        window,
                        cx,
                    );
                    assert!(!app.file_paths.pending);
                    assert_eq!(app.filtered_file_indices(cx).as_ref(), &[0]);
                    assert_eq!(app.selected_file, Some(0));
                }
                (release, blocked)
            })
        });
        release.send(()).unwrap();
        blocked.await.unwrap().unwrap();
        let drained = cx.update(|_, cx| {
            app.update(cx, |app, _| {
                app.file_paths.executor().submit_read(|| Ok(()))
            })
        });
        drained.await.unwrap().unwrap();
        cx.executor().run_until_parked();
        let history_filter = cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                assert_eq!(app.selected_file, Some(0));
                assert!(app.content.is_some());
                assert_eq!(app.files[0].path(), std::path::Path::new("assembly.glb"));
                assert_eq!(app.file_filter.read(cx).value().as_ref(), "external");
                app.back_to_history(window, cx);
                assert!(app.mode == WorkspaceMode::History);
                app.file_paths.task.take().expect("History query reapplied")
            })
        });
        history_filter.await;
        cx.executor().run_until_parked();
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                assert_eq!(app.filtered_file_indices(cx).as_ref(), &[0]);
                assert_eq!(app.selected_file, Some(0));
                assert_eq!(app.files[0].path(), std::path::Path::new("external.glb"));
                assert_eq!(app.file_filter.read(cx).value().as_ref(), "external");
                app._display_preferences_task = None;
            });
        });
    }

    /// Typing a filter keeps the rows the last answer left until the
    /// worker answers the query typed: with the worker held, every frame
    /// draws the same rows, never an empty list or "No matches", and an
    /// answer to an older query that lands meanwhile is dropped. The
    /// newest query's answer then replaces the rows.
    #[gpui::test]
    async fn typing_a_filter_keeps_its_rows_until_the_answer_lands(cx: &mut gpui::TestAppContext) {
        use crate::views::tests::{draw, history_window_from};
        use gpui::VisualTestContext;
        use std::sync::{Arc, atomic::Ordering, mpsc};

        const PATHS: [&str; 4] = [
            "src/app/views.rs",
            "src/app/main.rs",
            "docs/guide.md",
            "README.md",
        ];
        let mut script = String::from(
            "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> 1700000000 +0000\ndata 7\nfiles 4\n",
        );
        for path in PATHS {
            script.push_str(&format!(
                "M 100644 inline {path}\ndata {}\n{path}\n",
                path.len()
            ));
        }
        script.push_str("done\n");
        let (_fixture, app, cx) = history_window_from(cx, &script).await;
        // The rows the last frame drew, by their files' indices.
        let drawn = |cx: &mut VisualTestContext| -> Vec<usize> {
            let selected = app.read_with(cx, |app, _| app.selected_file);
            const ROWS: [&str; 4] = [
                "changed-file-0",
                "changed-file-1",
                "changed-file-2",
                "changed-file-3",
            ];
            (0..PATHS.len())
                .filter(|index| {
                    cx.debug_bounds(ROWS[*index]).is_some()
                        || (selected == Some(*index)
                            && cx.debug_bounds("selected-changed-file").is_some())
                })
                .collect()
        };
        assert_eq!(drawn(cx), vec![0, 1, 2, 3], "every changed file");
        let (release, wait) = mpsc::channel::<()>();
        let held = app.update(cx, |app, _| {
            app.file_paths.executor().submit_read(move || {
                wait.recv()?;
                Ok(())
            })
        });
        cx.update(|window, cx| {
            let filter = app.read(cx).file_filter.clone();
            filter.update(cx, |input, cx| input.focus(window, cx));
        });
        draw(cx);
        // Within the executor's queue of eight jobs.
        for typed in ["a", "p", "p", "/", "m"] {
            cx.simulate_input(typed);
            for _ in 0..2 {
                draw(cx);
                assert!(
                    app.read_with(cx, |app, _| app.file_paths.pending),
                    "{typed:?} typed into {:?}",
                    app.read_with(cx, |app, cx| app.file_filter.read(cx).value().to_string())
                );
                assert_eq!(
                    drawn(cx),
                    vec![0, 1, 2, 3],
                    "the rows stay while {typed:?} pends"
                );
                assert!(
                    cx.debug_bounds("files-no-match").is_none(),
                    "no false No matches"
                );
            }
        }
        // An older query's answer that lands now is dropped.
        app.update(cx, |app, cx| {
            let current = app.file_paths.cancellation.load(Ordering::Relaxed);
            let identity = app.file_paths.identity;
            app.land_file_filter(
                current - 1,
                identity,
                Ok(Ok((Arc::new(Vec::new()), Arc::from([0usize])))),
                cx,
            );
            assert!(app.file_paths.pending, "the newest query still pends");
        });
        draw(cx);
        assert_eq!(drawn(cx), vec![0, 1, 2, 3], "a stale answer is dropped");
        release.send(()).unwrap();
        held.await.unwrap().unwrap();
        let drained = app.update(cx, |app, _| {
            app.file_paths.executor().submit_read(|| Ok(()))
        });
        drained.await.unwrap().unwrap();
        cx.executor().run_until_parked();
        draw(cx);
        let main = app.read_with(cx, |app, _| {
            app.files
                .iter()
                .position(|file| file.path() == std::path::Path::new("src/app/main.rs"))
                .unwrap()
        });
        assert!(!app.read_with(cx, |app, _| app.file_paths.pending));
        assert_eq!(drawn(cx), vec![main], "the newest query's answer");
        assert_eq!(
            app.read_with(cx, |app, cx| app.file_filter.read(cx).value().to_string()),
            "app/m"
        );
    }

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
