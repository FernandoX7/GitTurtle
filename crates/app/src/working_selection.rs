//! Stable, visible file selection; row indices are never write targets.
use crate::*;
use gitturtle_core::{ChangeArea, RepositoryStatus, WriteCommand};
use gpui_kit::prelude::FluentBuilder;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Key {
    path: PathBuf,
    staged: bool,
}
impl Key {
    fn new(path: PathBuf, area: ChangeArea) -> Self {
        Self {
            path,
            staged: area == ChangeArea::Staged,
        }
    }
}
#[derive(Default)]
pub(super) struct Selection {
    selected: BTreeSet<Key>,
    anchor: Option<Key>,
    pub grouped: bool,
}
impl Selection {
    fn choose(&mut self, key: Key, visible: &[Key], toggle: bool, range: bool) {
        if range
            && let (Some(start), Some(end)) = (
                self.anchor
                    .as_ref()
                    .and_then(|anchor| visible.iter().position(|k| k == anchor)),
                visible.iter().position(|k| k == &key),
            )
        {
            if !toggle {
                self.selected.clear();
            }
            self.selected
                .extend(visible[start.min(end)..=start.max(end)].iter().cloned());
            return;
        }
        self.anchor = Some(key.clone());
        if toggle {
            if !self.selected.remove(&key) {
                self.selected.insert(key);
            }
        } else {
            self.selected.clear();
            self.selected.insert(key);
        }
    }
    pub(super) fn clear(&mut self) {
        self.selected.clear();
        self.anchor = None;
    }
    fn retain_visible(&mut self, visible: &[Key]) {
        let visible: BTreeSet<_> = visible.iter().collect();
        self.selected.retain(|key| visible.contains(key));
        if self
            .anchor
            .as_ref()
            .is_some_and(|key| !visible.contains(key))
        {
            self.anchor = None;
        }
    }
}

/// Status snapshots and the prepared index are shared; query changes do not
/// copy or normalize every path on the UI thread.
#[derive(Default)]
pub(super) struct FilterState {
    cancellation: Arc<std::sync::atomic::AtomicU64>,
    index: Option<Arc<path_filter::Index>>,
    task: Option<Task<()>>,
    pub pending: bool,
    pub error: Option<String>,
    pub staged_count: usize,
    pub visible_count: usize,
    pub conflicted: bool,
}
impl FilterState {
    pub fn reset(&mut self) {
        self.cancellation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.index = None;
        self.task = None;
        self.pending = false;
        self.error = None;
        self.staged_count = 0;
        self.visible_count = 0;
        self.conflicted = false;
    }
}

pub(super) struct ScrollAnchor {
    path: Option<PathBuf>,
    area: ChangeArea,
    offset: Point<Pixels>,
    top: usize,
    row_height: f32,
}
impl ScrollAnchor {
    fn reanchored_offset(&self, next_top: usize, height: f32) -> Point<Pixels> {
        let offset = f32::from(self.offset.y) * height / self.row_height;
        point(
            self.offset.x,
            px(automatic_refresh::reanchor_offset(
                offset, self.top, next_top, height,
            )),
        )
    }
}

fn rows(
    status: &RepositoryStatus,
    matches: &[usize],
    grouped: bool,
    cancellation: &std::sync::atomic::AtomicU64,
    ticket: u64,
) -> anyhow::Result<Vec<workspace::WorkingRow>> {
    use workspace::WorkingRow;
    let mut result = Vec::new();
    for area in [ChangeArea::Staged, ChangeArea::Unstaged] {
        let mut indices = Vec::new();
        for (position, &index) in matches.iter().enumerate() {
            if position % 128 == 0 {
                path_filter::checkpoint(cancellation, ticket)?;
            }
            let entry = &status.entries[index];
            let present = if area == ChangeArea::Staged {
                entry.staged.is_some()
            } else {
                entry.unstaged.is_some() || entry.untracked || entry.conflicted
            };
            if present {
                indices.push(index);
            }
        }
        if grouped {
            anyhow::ensure!(
                status.entries.len() <= path_filter::MAX_PATHS,
                "Directory grouping supports up to 100,000 working paths. Turn off Directories to browse every file."
            );
            indices.sort_by(|a, b| status.entries[*a].path.cmp(&status.entries[*b].path));
            path_filter::checkpoint(cancellation, ticket)?;
        }
        result.push(WorkingRow::Heading(area, indices.len()));
        let mut previous = None;
        for (position, index) in indices.into_iter().enumerate() {
            if position % 128 == 0 {
                path_filter::checkpoint(cancellation, ticket)?;
            }
            let parent = status.entries[index].path.parent();
            if grouped && previous != Some(parent) {
                result.push(WorkingRow::Directory(index, area));
                previous = Some(parent);
            }
            result.push(WorkingRow::File(index, area));
        }
    }
    Ok(result)
}

impl GitTurtle {
    fn visible_working_keys(&self) -> Vec<Key> {
        self.working_rows
            .iter()
            .filter_map(|row| match row {
                workspace::WorkingRow::File(index, area) => self
                    .work_status
                    .as_ref()?
                    .entries
                    .get(*index)
                    .map(|entry| Key::new(entry.path.clone(), *area)),
                _ => None,
            })
            .collect()
    }
    pub(super) fn reconcile_working_selection(&mut self) {
        if self.working_selection.selected.is_empty() && self.working_selection.anchor.is_none() {
            return;
        }
        let visible = self.visible_working_keys();
        self.working_selection.retain_visible(&visible);
    }
    pub(super) fn working_is_selected(&self, index: usize, area: ChangeArea) -> bool {
        self.work_status
            .as_ref()
            .and_then(|s| s.entries.get(index))
            .is_some_and(|entry| {
                self.working_selection
                    .selected
                    .contains(&Key::new(entry.path.clone(), area))
            })
    }
    pub(super) fn choose_working_selection(
        &mut self,
        index: usize,
        area: ChangeArea,
        toggle: bool,
        range: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some()
            || self.working_paths.pending
            || self.working_paths.error.is_some()
        {
            return;
        }
        let Some(entry) = self.work_status.as_ref().and_then(|s| s.entries.get(index)) else {
            return;
        };
        let key = Key::new(entry.path.clone(), area);
        let visible = self.visible_working_keys();
        self.working_selection.choose(key, &visible, toggle, range);
        self.select_working(index, area, window, cx);
    }
    pub(super) fn select_all_working(&mut self, cx: &mut Context<Self>) {
        if self.mode != WorkspaceMode::Working
            || self.operation_busy.is_some()
            || self.working_paths.pending
            || self.working_paths.error.is_some()
        {
            return;
        }
        self.working_selection.selected = self.visible_working_keys().into_iter().collect();
        cx.notify();
    }
    pub(super) fn extend_working_selection(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let files: Vec<_> = self
            .working_rows
            .iter()
            .filter_map(|row| match row {
                workspace::WorkingRow::File(i, a) => Some((*i, *a)),
                _ => None,
            })
            .collect();
        if files.is_empty() {
            return;
        }
        let current = files
            .iter()
            .position(|key| Some(*key) == self.working_selected)
            .unwrap_or(0);
        let next = if forward {
            (current + 1).min(files.len() - 1)
        } else {
            current.saturating_sub(1)
        };
        let (index, area) = files[next];
        self.choose_working_selection(index, area, false, true, window, cx);
        if let Some(position) = self
            .working_rows
            .iter()
            .position(|r| matches!(r,workspace::WorkingRow::File(i,a) if *i==index&&*a==area))
        {
            self.working_scroll
                .scroll_to_item(position, ScrollStrategy::Nearest);
        }
    }
    pub(super) fn working_scroll_anchor(&self, window: &Window) -> Option<ScrollAnchor> {
        let offset = self.working_scroll.0.borrow().base_handle.offset();
        let height = f32::from(window.pixel_snap(px(self.settings.density.file_row_height())));
        let top = ((-f32::from(offset.y)) / height).max(0.) as usize;
        let (path, area) = match self.working_rows.get(top)? {
            workspace::WorkingRow::Heading(area, _) | workspace::WorkingRow::Directory(_, area) => {
                (None, *area)
            }
            workspace::WorkingRow::File(index, area) => (
                Some(self.work_status.as_ref()?.entries.get(*index)?.path.clone()),
                *area,
            ),
        };
        Some(ScrollAnchor {
            path,
            area,
            offset,
            top,
            row_height: height,
        })
    }

    pub(super) fn filter_working_paths(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.working_selection.clear();
        self.working_scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.schedule_working_filter(None, false, window, cx);
    }

    pub(super) fn schedule_working_filter(
        &mut self,
        anchor: Option<ScrollAnchor>,
        center_selected: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use std::sync::atomic::Ordering;
        let ticket = self
            .working_paths
            .cancellation
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        self.working_paths.task = None;
        self.working_paths.error = None;
        self.working_paths.pending = false;
        self.working_rows.clear();
        let Some(status) = self.work_status.clone() else {
            cx.notify();
            return;
        };
        let query = self.working_filter.read(cx).value().to_string();
        let grouped = self.working_selection.grouped;
        let index = self.working_paths.index.clone();
        let cancellation = Arc::clone(&self.working_paths.cancellation);
        let response = self.file_paths.executor().submit_read(move || {
            path_filter::checkpoint(&cancellation, ticket)?;
            let (index, matches) = if query.is_empty() {
                (index, (0..status.entries.len()).collect::<Vec<_>>().into())
            } else {
                anyhow::ensure!(
                    query.len() <= path_filter::MAX_QUERY_BYTES,
                    "Path filter queries must be at most 4,096 bytes."
                );
                let index = match index {
                    Some(index) => index,
                    None => Arc::new(path_filter::prepare_paths(
                        status.entries.iter().map(|entry| {
                            [Some(entry.path.as_path()), entry.original_path.as_deref()]
                        }),
                        &cancellation,
                        ticket,
                    )?),
                };
                let matches = path_filter::matching(&index, &query, &cancellation, ticket)?;
                (Some(index), matches)
            };
            let rows = rows(&status, &matches, grouped, &cancellation, ticket)?;
            let staged = status
                .entries
                .iter()
                .filter(|entry| entry.staged.is_some())
                .count();
            let conflicted = status.entries.iter().any(|entry| entry.conflicted);
            path_filter::checkpoint(&cancellation, ticket)?;
            Ok((index, rows, staged, conflicted, matches.len()))
        });
        self.working_paths.pending = true;
        self.working_paths.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.working_paths.cancellation.load(Ordering::Relaxed) != ticket { return; }
                this.working_paths.pending = false;
                this.working_paths.task = None;
                match result {
                    Ok(Ok((index, rows, staged, conflicted, visible_count))) => {
                        this.working_paths.index = index;
                        this.working_paths.staged_count = staged;
                        this.working_paths.visible_count = visible_count;
                        this.working_paths.conflicted = conflicted;
                        this.working_rows = rows;
                        this.reconcile_working_selection();
                        let selected = this.working_selected.and_then(|(i, a)| this.working_rows.iter().position(|r| matches!(r, workspace::WorkingRow::File(index, area) if *index == i && *area == a)));
                        if this.working_selected.is_some() && selected.is_none() {
                            this.working_selected = None;
                            if this.mode == WorkspaceMode::Working {
                                this.invalidate_read(); this.clear_preview(); this.files.clear(); this.file_paths.reset(); this.selected_file = None;
                            }
                        }
                        if let Some(anchor) = anchor {
                            if let Some(next_top) = this.working_rows.iter().position(|row| match row {
                                workspace::WorkingRow::Heading(area, _) | workspace::WorkingRow::Directory(_, area) => anchor.path.is_none() && *area == anchor.area,
                                workspace::WorkingRow::File(index, area) => *area == anchor.area && this.work_status.as_ref().and_then(|status| status.entries.get(*index)).is_some_and(|entry| Some(&entry.path) == anchor.path.as_ref()),
                            }) {
                                let height = f32::from(window.pixel_snap(px(this.settings.density.file_row_height())));
                                this.working_scroll.0.borrow().base_handle.set_offset(anchor.reanchored_offset(next_top, height));
                            }
                        } else if center_selected && let Some(position) = selected {
                            this.working_scroll.scroll_to_item(position, ScrollStrategy::Center);
                        }
                    }
                    Ok(Err(error)) => { this.working_paths.error = Some(error.to_string()); this.working_selection.clear(); }
                    Err(_) => { this.working_paths.error = Some("Path filtering stopped. Edit or clear the query to retry.".into()); this.working_selection.clear(); }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn apply_selected_files(&mut self, staged: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation_busy.is_some()
            || self.working_paths.pending
            || self.working_paths.error.is_some()
        {
            return;
        }
        self.reconcile_working_selection();
        let Some(status) = &self.work_status else {
            return;
        };
        let mut paths = BTreeSet::new();
        let mut count = 0;
        for key in &self.working_selection.selected {
            if key.staged != staged {
                continue;
            }
            let Some(entry) = status.entries.iter().find(|entry| entry.path == key.path) else {
                return;
            };
            if entry.conflicted {
                self.operation_error = Some(
                    "Resolve and explicitly mark conflicted files before using batch staging."
                        .into(),
                );
                cx.notify();
                return;
            }
            paths.extend(entry.paths());
            count += 1;
        }
        if paths.is_empty() {
            return;
        }
        let paths: Vec<_> = paths.into_iter().collect();
        let title = format!(
            "{} {count} selected files",
            if staged { "Unstage" } else { "Stage" }
        );
        let details = format!(
            "Repository: {}\n\n{}\n\nOnly these visible selected files are included. Rename source paths are included with their destination. Other staged and working changes are preserved.",
            self.path
                .as_ref()
                .map_or(String::new(), |p| p.display().to_string()),
            paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n")
        );
        let command = if staged {
            WriteCommand::Unstage { paths }
        } else {
            WriteCommand::Stage { paths }
        };
        self.confirm_git_write(
            title,
            details,
            if staged {
                "Unstage selected files"
            } else {
                "Stage selected files"
            },
            command,
            window,
            cx,
        );
    }
    pub(super) fn render_working_selection(&self, cx: &mut Context<Self>) -> AnyElement {
        let staged = self
            .working_selection
            .selected
            .iter()
            .filter(|key| key.staged)
            .count();
        let unstaged = self.working_selection.selected.len() - staged;
        div()
            .px_3()
            .py_2()
            .flex()
            .flex_col()
            .gap_1()
            .child(Input::new(&self.working_filter).text_size(crate::appearance::ui_text(12.)))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .child(
                        button(
                            "group-working",
                            "Directories",
                            "",
                            self.working_selection.grouped,
                        )
                        .toggled(self.working_selection.grouped)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.working_selection.grouped = !this.working_selection.grouped;
                            this.filter_working_paths(window, cx);
                        })),
                    )
                    .child(
                        button("stage-selection", format!("Stage {unstaged}"), "", false)
                            .disabled(
                                unstaged == 0
                                    || self.operation_busy.is_some()
                                    || self.working_paths.pending
                                    || self.working_paths.error.is_some(),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.apply_selected_files(false, window, cx)
                            })),
                    )
                    .child(
                        button("unstage-selection", format!("Unstage {staged}"), "", false)
                            .disabled(
                                staged == 0
                                    || self.operation_busy.is_some()
                                    || self.working_paths.pending
                                    || self.working_paths.error.is_some(),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.apply_selected_files(true, window, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(palette(cx).muted))
                    .child(format!(
                        "{} selected · filtering clears selection",
                        staged + unstaged
                    )),
            )
            .when(self.working_paths.pending, |el| {
                el.child(
                    div()
                        .id("working-filter-pending")
                        .role(Role::Label)
                        .text_size(crate::appearance::ui_text(11.))
                        .child("Filtering working paths…"),
                )
            })
            .when(
                !self.working_paths.pending
                    && self.working_paths.error.is_none()
                    && self.working_paths.visible_count == 0
                    && !self.working_filter.read(cx).value().is_empty(),
                |el| {
                    el.child(
                        div()
                            .id("working-filter-empty")
                            .role(Role::Label)
                            .text_size(crate::appearance::ui_text(11.))
                            .child("No working paths match this filter"),
                    )
                },
            )
            .when_some(self.working_paths.error.clone(), |el, error| {
                el.child(
                    div()
                        .id("working-filter-error")
                        .role(Role::Label)
                        .text_size(crate::appearance::ui_text(11.))
                        .text_color(rgb(palette(cx).warning))
                        .child(error),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{Key, ScrollAnchor, Selection};
    use gitturtle_core::ChangeArea;
    use gpui_kit::{point, px};
    fn keys() -> Vec<Key> {
        ["a", "b", "c", "d"]
            .map(|p| Key::new(p.into(), ChangeArea::Unstaged))
            .to_vec()
    }
    #[test]
    fn quiet_refresh_anchor_uses_snapped_rows_across_a_pending_scale_change() {
        let anchor = ScrollAnchor {
            path: None,
            area: ChangeArea::Unstaged,
            offset: point(px(-20.), px(-25512.75)),
            top: 500,
            row_height: 51.,
        };
        // 44 * 1.15 lays out at 51 px, not 50.6. A pending filter may return
        // after scale changes again; retain row500 plus its quarter-row offset.
        assert_eq!(
            anchor.reanchored_offset(502, 51.),
            point(px(-20.), px(-25614.75))
        );
        assert_eq!(
            anchor.reanchored_offset(502, 55.),
            point(px(-20.), px(-27623.75))
        );
    }

    #[test]
    fn prepared_working_filter_keeps_renames_areas_and_literal_targets() {
        use crate::{path_filter, workspace::WorkingRow};
        use gitturtle_core::GitRepository;
        use std::{fs, path::PathBuf, process::Command, sync::atomic::AtomicU64};
        struct Fixture(PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let fixture = Fixture(std::env::temp_dir().join(format!(
                "gitturtle-working-filter-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )));
        fs::create_dir(&fixture.0).unwrap();
        let git = |args: &[&str]| {
            let mut command = Command::new("git");
            command
                .arg("-C")
                .arg(&fixture.0)
                .args([
                    "-c",
                    "user.name=Filter Fixture",
                    "-c",
                    "user.email=filter@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "core.hooksPath=/dev/null",
                ])
                .args(args)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null");
            for key in [
                "GIT_DIR",
                "GIT_WORK_TREE",
                "GIT_COMMON_DIR",
                "GIT_INDEX_FILE",
                "GIT_OBJECT_DIRECTORY",
                "GIT_ALTERNATE_OBJECT_DIRECTORIES",
                "GIT_CONFIG_COUNT",
                "GIT_CONFIG_PARAMETERS",
            ] {
                command.env_remove(key);
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "--quiet"]);
        fs::create_dir(fixture.0.join("old")).unwrap();
        fs::create_dir(fixture.0.join("new")).unwrap();
        fs::write(fixture.0.join("old/École.rs"), "one\ntwo\nthree\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "baseline"]);
        fs::rename(fixture.0.join("old/École.rs"), fixture.0.join("new/🐢.rs")).unwrap();
        git(&["add", "."]);
        fs::write(fixture.0.join("new/🐢.rs"), "one\ntwo\nthree\nextra\n").unwrap();
        fs::write(fixture.0.join("unrelated.txt"), "preserved\n").unwrap();
        let repo = GitRepository::open(&fixture.0).unwrap();
        let status = repo.status().unwrap();
        let index_before = fs::read(fixture.0.join(".git/index")).unwrap();
        let cancellation = AtomicU64::new(3);
        let index = path_filter::prepare_paths(
            status
                .entries
                .iter()
                .map(|entry| [Some(entry.path.as_path()), entry.original_path.as_deref()]),
            &cancellation,
            3,
        )
        .unwrap();
        let matched = path_filter::matching(&index, "éCOLE", &cancellation, 3).unwrap();
        assert_eq!(matched.len(), 1);
        let entry = &status.entries[matched[0]];
        assert_eq!(
            entry.paths(),
            [PathBuf::from("new/🐢.rs"), PathBuf::from("old/École.rs")]
        );
        let result = super::rows(&status, &matched, true, &cancellation, 3).unwrap();
        let files: Vec<_> = result
            .iter()
            .filter_map(|row| match row {
                WorkingRow::File(index, area) => Some((*index, *area)),
                _ => None,
            })
            .collect();
        assert_eq!(
            files,
            [
                (matched[0], ChangeArea::Staged),
                (matched[0], ChangeArea::Unstaged)
            ]
        );
        assert_eq!(
            result
                .iter()
                .filter(|row| matches!(row, WorkingRow::Directory(..)))
                .count(),
            2
        );
        assert!(super::rows(&status, &matched, true, &cancellation, 2).is_err());
        assert_eq!(
            fs::read(fixture.0.join(".git/index")).unwrap(),
            index_before
        );
        assert_eq!(
            fs::read_to_string(fixture.0.join("unrelated.txt")).unwrap(),
            "preserved\n"
        );
        assert_eq!(repo.status().unwrap(), status);
    }

    #[test]
    fn replacing_working_snapshot_cancels_pending_index_and_clears_cached_rows() {
        use std::sync::{Arc, atomic::Ordering};
        let mut state = super::FilterState {
            index: Some(Arc::new(vec![["old path".into(), String::new()]])),
            pending: true,
            visible_count: 12,
            ..Default::default()
        };
        let ticket = state.cancellation.load(Ordering::Relaxed);
        state.reset();
        assert!(crate::path_filter::checkpoint(&state.cancellation, ticket).is_err());
        assert!(state.index.is_none());
        assert!(!state.pending);
        assert_eq!(state.visible_count, 0);
    }

    #[test]
    fn range_toggle_and_filtered_refresh_never_leave_hidden_targets() {
        let keys = keys();
        let mut s = Selection::default();
        s.choose(keys[1].clone(), &keys, false, false);
        s.choose(keys[3].clone(), &keys, false, true);
        assert_eq!(s.selected, keys[1..].iter().cloned().collect());
        s.choose(keys[2].clone(), &keys, true, false);
        assert_eq!(
            s.selected,
            [keys[1].clone(), keys[3].clone()].into_iter().collect()
        );
        s.retain_visible(&keys[..2]);
        assert_eq!(s.selected, [keys[1].clone()].into_iter().collect());
        s.clear();
        assert!(s.selected.is_empty());
        assert!(s.anchor.is_none());
    }
    #[test]
    fn staged_and_unstaged_versions_are_distinct_and_removed_rows_are_dropped() {
        let u = Key::new("file".into(), ChangeArea::Unstaged);
        let s = Key::new("file".into(), ChangeArea::Staged);
        let mut selection = Selection::default();
        selection.choose(u.clone(), &[u.clone(), s.clone()], false, false);
        selection.choose(s.clone(), &[u, s.clone()], true, false);
        assert_eq!(selection.selected.len(), 2);
        selection.retain_visible(std::slice::from_ref(&s));
        assert_eq!(selection.selected, [s].into_iter().collect());
    }
}

#[cfg(test)]
#[path = "review_bench.rs"]
mod benchmarks;
