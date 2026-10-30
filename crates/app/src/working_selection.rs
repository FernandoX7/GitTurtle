//! Stable, visible file selection; row indices are never write targets.
use crate::*;

/// Whether two repository paths name one file: every byte the same, so
/// `sub` and `sub/`, which `Path` holds equal, as a deleted `sub` and an
/// untracked repository at `sub/` are, name two.
pub(crate) fn same_path(one: &Path, other: &Path) -> bool {
    one.as_os_str() == other.as_os_str()
}
use gitturtle_core::{ChangeArea, RepositoryStatus, StatusEntry, WriteCommand};
use gpui_kit::prelude::FluentBuilder;
use std::{collections::BTreeSet, path::Path};

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
    /// The selected paths in `visible` order, each once: a file selected in
    /// both Staged and Unstaged is one path.
    fn paths<'a>(&self, visible: &'a [Key]) -> Vec<&'a Path> {
        let mut seen = BTreeSet::new();
        visible
            .iter()
            .filter(|key| self.selected.contains(*key))
            .map(|key| key.path.as_path())
            .filter(|path| seen.insert(*path))
            .collect()
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

/// What the Working rows being prepared are for, which a row's menu, its
/// items refused meanwhile, states (`GitTurtle::working_rows_wait`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Preparation {
    /// The rows of a newer status, as a read of it lands.
    #[default]
    Status,
    /// The rows the filter's query or the Directories grouping shows.
    Filter,
}

/// Status snapshots and the prepared index are shared; query changes do not
/// copy or normalize every path on the UI thread.
#[derive(Default)]
pub(super) struct FilterState {
    cancellation: Arc<std::sync::atomic::AtomicU64>,
    index: Option<Arc<path_filter::Index>>,
    task: Option<Task<()>>,
    pub pending: bool,
    /// What the rows prepared while `pending` are for, as the latest
    /// preparation was asked.
    pub preparation: Preparation,
    pub error: Option<String>,
    pub staged_count: usize,
    pub visible_count: usize,
    pub conflicted: bool,
    /// While the rows of a newer status are prepared, the status the shown
    /// rows index and the cursor among them as it was: the list keeps
    /// drawing them, with the counts above, until the new rows land.
    retained: Option<Shown>,
}
/// The status shown rows index and the cursor among them.
pub(super) type Shown = (Arc<RepositoryStatus>, Option<(usize, ChangeArea)>);
/// The shown rows' counts: the staged files, the filter's matches and
/// whether any file is conflicted.
pub(super) type Counts = (usize, usize, bool);
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
        self.retained = None;
    }

    /// A status with other entries replaces the one the index and the shown
    /// rows were built from. Where rows are shown (`shown`: the status they
    /// index and the cursor among them), they and their counts stay until
    /// the new status's rows land, so the list neither empties nor moves
    /// while the filter prepares them (DESIGN.md, "Motion"); with none shown
    /// this is `reset`.
    pub fn supersede(&mut self, shown: Option<Shown>) {
        let Some(shown) = shown else {
            self.reset();
            return;
        };
        self.cancellation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.index = None;
        self.task = None;
        self.pending = false;
        self.error = None;
        // Rows kept from an earlier status still index that one.
        self.retained.get_or_insert(shown);
    }

    /// Whether the shown rows index an earlier status than the window's,
    /// while that status's rows are prepared.
    pub fn retains_rows(&self) -> bool {
        self.retained.is_some()
    }

    /// The shown rows' counts, which a retained tab keeps with its rows.
    pub fn counts(&self) -> Counts {
        (self.staged_count, self.visible_count, self.conflicted)
    }

    /// Shows a returning tab's counts with its rows until the filter's land.
    pub fn restore_counts(&mut self, (staged, visible, conflicted): Counts) {
        self.staged_count = staged;
        self.visible_count = visible;
        self.conflicted = conflicted;
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
    /// The status the shown rows index: while a newer status's rows are
    /// prepared, the earlier one they were built from.
    pub(super) fn shown_working_status(&self) -> Option<&RepositoryStatus> {
        match &self.working_paths.retained {
            Some((status, _)) => Some(status),
            None => self.work_status.as_deref(),
        }
    }

    /// The cursor among the shown rows, in their status's indices.
    pub(super) fn shown_working_cursor(&self) -> Option<(usize, ChangeArea)> {
        match &self.working_paths.retained {
            Some((_, cursor)) => *cursor,
            None => self.working_selected,
        }
    }

    /// The rows the cursor, keys, selection, copying and menus act on. While
    /// the filter prepares new rows there are none: the rows still shown are
    /// the previous result's, drawn so that the list holds its place, and a
    /// key or a click on them must not act on what is being replaced.
    pub(super) fn actionable_working_rows(&self) -> &[workspace::WorkingRow] {
        if self.working_paths.pending {
            &[]
        } else {
            &self.working_rows
        }
    }

    /// The element ID of the shown rows' row of the `index`th file in
    /// `area`, made once per row of the status they show
    /// (`workspace::WorkingRowIds`).
    pub(super) fn working_row_key(&self, index: usize, area: ChangeArea) -> ElementId {
        let status = match &self.working_paths.retained {
            Some((status, _)) => status,
            None => self.work_status.as_ref().expect("the status the rows show"),
        };
        self.working_row_ids.borrow_mut().get(status, index, area)
    }

    /// Where `path`'s row in `area` stands among the actionable rows, with
    /// the file as it now stands there. A row menu acts on its file by path
    /// and finds the row's place when it acts, since rows coming or going
    /// before it move that place.
    fn working_row_entry(&self, path: &Path, area: ChangeArea) -> Option<(usize, &StatusEntry)> {
        let status = self.work_status.as_ref()?;
        self.actionable_working_rows()
            .iter()
            .enumerate()
            .find_map(|(position, row)| match *row {
                workspace::WorkingRow::File(index, at) if at == area => status
                    .entries
                    .get(index)
                    .filter(|entry| same_path(&entry.path, path))
                    .map(|entry| (position, entry)),
                _ => None,
            })
    }

    /// Where `path`'s row in `area` stands among the actionable rows.
    pub(super) fn working_row_position(&self, path: &Path, area: ChangeArea) -> Option<usize> {
        self.working_row_entry(path, area)
            .map(|(position, _)| position)
    }

    /// Keeps the file list's keyboard menu, while it is a Working Changes
    /// row's (`file_menu_row`), with that row as installed rows move it:
    /// drawn beside the row where it now stands, and built again, its
    /// keyboard cursor on its first action, where the file's status changed,
    /// so its items act on what the row now shows; or, where the row went,
    /// as a refresh after its file was committed, staged away, reverted or
    /// deleted outside the app takes it, closed, focus returning to the
    /// list. Kept by the row's place, the menu stayed there, beside
    /// whichever row moved into it, and its items still acted on the file
    /// that left; kept as it opened, it offered to delete an untracked file
    /// that had since been added, which the delete refused.
    ///
    /// A write's start or end runs it too (`KeyboardMenu::followed_by`),
    /// which builds the menu again from the row's file as it then stands:
    /// built again from the file as the menu opened, it brought back Delete
    /// untracked file… for a file a refresh had found added. While rows are
    /// being prepared there is none to act on, and the rows landing run it;
    /// a write that starts meanwhile builds the menu again at once, from the
    /// file as it last found it, every item refused, where it once kept
    /// offering what that write refused until the rows landed, and a menu
    /// built so is built again as the reason it states changes
    /// (`working_rows_wait`), as that write ends or a newer status lands
    /// while the filter's rows are prepared, which `followed_by` runs it for
    /// too: the rows the filter then prepares are that status's, and the
    /// menu once kept saying "Filtering…" until they landed.
    pub(super) fn follow_working_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(followed) = &self.file_menu_row else {
            return;
        };
        if self.working_paths.pending {
            let waiting = self.working_rows_wait();
            if (!followed.busy && self.write_unsettled())
                || (followed.waiting.is_some() && followed.waiting != waiting)
            {
                let items = self.working_menu_items(cx);
                if let (Some(items), Some(menu)) = (items, &mut self.file_menu) {
                    menu.rebuild(items, window, cx);
                }
            }
            return;
        }
        let target = &followed.target;
        let Some((row, entry)) = self.working_row_entry(&target.entry.path, target.area()) else {
            self.dismiss_file_menu(window, cx);
            return;
        };
        let changed = (*entry != target.entry).then(|| entry.clone());
        let rebuild = changed.is_some()
            || followed.busy != self.write_unsettled()
            || followed.waiting.is_some();
        if let (Some(entry), Some(followed)) = (changed, self.file_menu_row.as_mut()) {
            followed.target.entry = entry;
        }
        let items = if rebuild {
            self.working_menu_items(cx)
        } else {
            None
        };
        let Some(menu) = &mut self.file_menu else {
            return;
        };
        menu.follow_row(row);
        if let Some(items) = items {
            menu.rebuild(items, window, cx);
        }
    }

    /// No rows to show: the status is gone, or the filter failed and its
    /// error replaces the list.
    fn clear_working_rows(&mut self) {
        self.working_rows.clear();
        self.working_paths.retained = None;
    }

    /// The filter is preparing the first rows, with no earlier result to
    /// show meanwhile: only then does "Filtering working paths…" show.
    pub(super) fn working_filter_waiting(&self) -> bool {
        self.working_paths.pending && self.working_rows.is_empty()
    }

    /// Whether the shown rows are a path filter's that matched nothing: the
    /// status they index has entries and none of them is listed. It follows
    /// the shown rows rather than the field, so while a new query's rows are
    /// prepared the list keeps showing what it showed.
    pub(super) fn working_no_match(&self) -> bool {
        self.working_paths.error.is_none()
            && !self.working_rows.is_empty()
            && self
                .shown_working_status()
                .is_some_and(|status| !status.entries.is_empty())
            && !self
                .working_rows
                .iter()
                .any(|row| matches!(row, workspace::WorkingRow::File(..)))
    }

    /// Empties the path filter, as the no-match state's Clear filter does,
    /// and moves focus to the list, which shows every file again.
    pub(super) fn clear_working_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.working_filter
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.filter_working_paths(window, cx);
        window.focus(&self.file_focus, cx);
        cx.notify();
    }

    fn visible_working_keys(&self) -> Vec<Key> {
        self.actionable_working_rows()
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
    /// What Copy Selected List Item takes from Working Changes: the selected
    /// files' repository paths, one per line, else the highlighted row's.
    pub(super) fn working_copy_paths(&self) -> Option<String> {
        let visible = self.visible_working_keys();
        let paths = self.working_selection.paths(&visible);
        if !paths.is_empty() {
            return Some(
                paths
                    .iter()
                    .map(|path| path.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
        let (index, _) = self.working_selected?;
        let entry = self.work_status.as_ref()?.entries.get(index)?;
        Some(entry.path.to_string_lossy().into_owned())
    }
    /// Whether the shown row of `index` in `area` is among the selected
    /// files; `index` is in the shown rows' status.
    pub(super) fn working_is_selected(&self, index: usize, area: ChangeArea) -> bool {
        self.shown_working_status()
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
            .actionable_working_rows()
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
            .actionable_working_rows()
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
        // The row at the top of the list as drawn, kept from an earlier
        // status while a newer one's rows are prepared.
        let (path, area) = match self.working_rows.get(top)? {
            workspace::WorkingRow::Heading(area, _) | workspace::WorkingRow::Directory(_, area) => {
                (None, *area)
            }
            workspace::WorkingRow::File(index, area) => (
                Some(
                    self.shown_working_status()?
                        .entries
                        .get(*index)?
                        .path
                        .clone(),
                ),
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
        self.schedule_working_filter(None, false, Preparation::Filter, window, cx);
    }

    /// Prepares the rows the filter shows of the window's status, for
    /// `preparation`, off the UI thread.
    pub(super) fn schedule_working_filter(
        &mut self,
        anchor: Option<ScrollAnchor>,
        center_selected: bool,
        preparation: Preparation,
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
        // The shown rows stay, drawn where they are, until the new ones land:
        // a list that empties for the frames the filter takes moves every
        // row and the controls beside it, and a row that a Stage moves would
        // vanish before it travels (DESIGN.md, "Motion").
        self.interrupt_working_travel();
        let departure = self.working_departure();
        let Some(status) = self.work_status.clone() else {
            self.clear_working_rows();
            self.follow_working_menu(window, cx);
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
            let landing = departure
                .and_then(|(path, from)| crate::motion::landing(&status, &rows, &path, from));
            let staged = status
                .entries
                .iter()
                .filter(|entry| entry.staged.is_some())
                .count();
            let conflicted = status.entries.iter().any(|entry| entry.conflicted);
            path_filter::checkpoint(&cancellation, ticket)?;
            Ok((index, rows, staged, conflicted, matches.len(), landing))
        });
        self.working_paths.pending = true;
        self.working_paths.preparation = preparation;
        self.working_paths.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.working_paths.cancellation.load(Ordering::Relaxed) != ticket { return; }
                this.working_paths.pending = false;
                this.working_paths.task = None;
                match result {
                    Ok(Ok((index, rows, staged, conflicted, visible_count, landing))) => {
                        this.working_paths.index = index;
                        this.working_paths.staged_count = staged;
                        this.working_paths.visible_count = visible_count;
                        this.working_paths.conflicted = conflicted;
                        this.working_rows = rows;
                        this.working_paths.retained = None;
                        this.reconcile_working_selection();
                        let selected = this.working_selected.and_then(|(i, a)| this.working_rows.iter().position(|r| matches!(r, workspace::WorkingRow::File(index, area) if *index == i && *area == a)));
                        if this.working_selected.is_some() && selected.is_none() {
                            this.working_selected = None;
                            if this.mode == WorkspaceMode::Working {
                                this.invalidate_read(); this.clear_preview(); this.files.clear(); this.file_paths.reset(); this.selected_file = None;
                                // The stopped read puts a running quiet read
                                // back to wait; nothing else here starts it.
                                this.try_automatic_refresh(window, cx);
                            }
                        }
                        if let Some(anchor) = anchor {
                            if let Some(next_top) = this.working_rows.iter().position(|row| match row {
                                workspace::WorkingRow::Heading(area, _) | workspace::WorkingRow::Directory(_, area) => anchor.path.is_none() && *area == anchor.area,
                                workspace::WorkingRow::File(index, area) => *area == anchor.area && this.work_status.as_ref().and_then(|status| status.entries.get(*index)).is_some_and(|entry| anchor.path.as_deref().is_some_and(|path| same_path(&entry.path, path))),
                            }) {
                                let height = f32::from(window.pixel_snap(px(this.settings.density.file_row_height())));
                                this.working_scroll.0.borrow().base_handle.set_offset(anchor.reanchored_offset(next_top, height));
                            }
                        } else if center_selected && let Some(position) = selected {
                            this.working_scroll.scroll_to_item(position, ScrollStrategy::Center);
                        }
                        if center_selected { this.reveal_confirming_heading(selected, window, cx); }
                        this.rows_moved_under_pointer(window, cx);
                        this.land_working_row(landing, cx);
                    }
                    Ok(Err(error)) => { this.working_paths.error = Some(error.to_string()); this.clear_working_rows(); this.working_selection.clear(); this.land_working_row(None, cx); }
                    Err(_) => { this.working_paths.error = Some("Path filtering stopped. Edit or clear the query to retry.".into()); this.clear_working_rows(); this.working_selection.clear(); this.land_working_row(None, cx); }
                }
                this.follow_working_menu(window, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }
    pub(super) fn apply_selected_files(
        &mut self,
        staged: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
            let Some(entry) = status
                .entries
                .iter()
                .find(|entry| same_path(&entry.path, &key.path))
            else {
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
    /// The selected staged and unstaged entries, which Unstage and Stage
    /// count.
    pub(super) fn selection_counts(&self) -> (usize, usize) {
        let staged = self
            .working_selection
            .selected
            .iter()
            .filter(|key| key.staged)
            .count();
        (staged, self.working_selection.selected.len() - staged)
    }

    /// The filter and the selection's controls. Stage and Unstage stay
    /// together, wrapping below Directories as a pair; `compact`, where the
    /// pair does not fit a row of its own labelled (`WorkingForms`), the
    /// three keep only their icons on one row, with their accessible names
    /// and tooltips that lead with those names.
    pub(super) fn render_working_selection(
        &self,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (staged, unstaged) = self.selection_counts();
        // A write in flight or rows being prepared leave Stage and Unstage
        // inert at once (`apply_selected_files`), dimmed once the spell
        // looks busy.
        let spell = self.operation_busy.is_some() || self.working_paths.pending;
        let unavailable = (spell && self.busy_shown()) || self.working_paths.error.is_some();
        let refused = spell || self.working_paths.error.is_some();
        let grouped = self.working_selection.grouped;
        let (stage, unstage) = (format!("Stage {unstaged}"), format!("Unstage {staged}"));
        let directories = if compact {
            crate::views::compact_control_name(
                button("group-working", "", "folder", grouped),
                "group-working",
                "Directories",
            )
            .tooltip("Directories · Group the files by folder")
        } else {
            button("group-working", "Directories", "", grouped)
        };
        let (stage_button, unstage_button) = if compact {
            (
                crate::views::compact_control_label(
                    button("stage-selection", "", "plus", false),
                    "stage-selection",
                    stage.clone(),
                )
                .tooltip(format!("{stage} · Stage the selected files")),
                crate::views::compact_control_label(
                    button("unstage-selection", "", "minus", false),
                    "unstage-selection",
                    unstage.clone(),
                )
                .tooltip(format!("{unstage} · Unstage the selected files")),
            )
        } else {
            (
                button("stage-selection", stage, "", false),
                button("unstage-selection", unstage, "", false),
            )
        };
        div()
            .px_3()
            .py_2()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .debug_selector(|| "working-filter".into())
                    .relative()
                    .child(
                        Input::new(&self.working_filter)
                            .aria_label(crate::workspace::FILTER_NAME)
                            .text_size(crate::appearance::ui_text(12.)),
                    )
                    // Painted after the field, it fits its placeholder.
                    .child(self.fit_filter_placeholder()),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .child(
                        directories
                            .debug_selector(|| "group-working".into())
                            .toggled(grouped)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.working_selection.grouped = !this.working_selection.grouped;
                                this.filter_working_paths(window, cx);
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(
                                stage_button
                                    .debug_selector(|| "stage-selection".into())
                                    .map(|stage| {
                                        crate::workspace::refusing(
                                            stage,
                                            || "stage-selection".into(),
                                            unstaged == 0 || refused,
                                            unstaged == 0 || unavailable,
                                        )
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.apply_selected_files(false, window, cx)
                                    })),
                            )
                            .child(
                                unstage_button
                                    .debug_selector(|| "unstage-selection".into())
                                    .map(|unstage| {
                                        crate::workspace::refusing(
                                            unstage,
                                            || "unstage-selection".into(),
                                            staged == 0 || refused,
                                            staged == 0 || unavailable,
                                        )
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.apply_selected_files(true, window, cx)
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| "working-selection-count".into())
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(palette(cx).muted))
                    .child(format!(
                        "{} selected · filtering clears selection",
                        staged + unstaged
                    )),
            )
            // Only before the first rows: later the shown rows stay until
            // the new ones land, and the line would move them down.
            .when(self.working_filter_waiting(), |el| {
                el.child(
                    native_accessibility::text(
                        "working-filter-pending",
                        "Filtering working paths…",
                    )
                    .text_size(crate::appearance::ui_text(11.)),
                )
            })
            // A filter that matches nothing says so inside the list
            // (`working_no_match`), so nothing here moves the list down.
            .when_some(self.working_paths.error.clone(), |el, error| {
                el.child(
                    native_accessibility::text("working-filter-error", error)
                        .text_size(crate::appearance::ui_text(11.))
                        .text_color(rgb(palette(cx).warning)),
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

    /// A status with other entries cancels the index built from the earlier
    /// one, as a reset does, but rows on screen keep their counts and the
    /// status they index, the first one kept while later statuses arrive,
    /// until a reset; with no rows shown it is a reset.
    #[test]
    fn a_new_status_keeps_the_shown_rows_with_their_status_and_counts() {
        use gitturtle_core::{ChangeArea, RepositoryStatus};
        use std::sync::{Arc, atomic::Ordering};
        let status = |branch: &str| {
            Arc::new(RepositoryStatus {
                branch: Some(branch.into()),
                ..Default::default()
            })
        };
        let mut state = super::FilterState {
            index: Some(Arc::new(vec![["old path".into(), String::new()]])),
            staged_count: 2,
            visible_count: 3,
            conflicted: true,
            ..Default::default()
        };
        let ticket = state.cancellation.load(Ordering::Relaxed);
        let cursor = Some((1, ChangeArea::Unstaged));
        state.supersede(Some((status("shown"), cursor)));
        assert!(crate::path_filter::checkpoint(&state.cancellation, ticket).is_err());
        assert!(state.index.is_none());
        assert_eq!(state.counts(), (2, 3, true));
        assert!(state.retains_rows());
        state.supersede(Some((status("later"), None)));
        let (kept, kept_cursor) = state.retained.clone().unwrap();
        assert_eq!(kept.branch.as_deref(), Some("shown"));
        assert_eq!(kept_cursor, cursor);
        state.reset();
        assert!(!state.retains_rows());
        assert_eq!(state.counts(), (0, 0, false));
        state.restore_counts((4, 5, false));
        state.supersede(None);
        assert_eq!(state.counts(), (0, 0, false));
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
    fn copied_paths_follow_the_list_once_each() {
        let key = |path: &str, area| Key::new(path.into(), area);
        let visible = [
            key("b", ChangeArea::Staged),
            key("a", ChangeArea::Unstaged),
            key("b", ChangeArea::Unstaged),
            key("c", ChangeArea::Unstaged),
        ];
        let mut selection = Selection::default();
        assert!(selection.paths(&visible).is_empty());
        for index in [3, 2, 0] {
            selection.choose(visible[index].clone(), &visible, true, false);
        }
        let paths: Vec<_> = selection.paths(&visible);
        assert_eq!(
            paths,
            [std::path::Path::new("b"), std::path::Path::new("c")]
        );
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
