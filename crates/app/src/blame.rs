//! Transient attribution with retained comparison state and replaceable reads.
use crate::appearance::CodeFont;
use crate::focus_reveal::{FocusReveal, Trigger};
use crate::*;
use gitturtle_core::{Blame, BlameTarget, LineHistory};
use gpui_kit::prelude::FluentBuilder;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    Attribution,
    History(usize),
}

/// What scrolls each of Blame's Tab stops into view, whole, as its focus
/// comes from the keyboard (DESIGN.md: whole focus). A pane shorter than its
/// parts' least heights scrolls as a whole, and the footer scrolls its own
/// rows within it, the line history's among them, so a footer control is
/// revealed in the footer first and then in the pane, where the footer has
/// just scrolled it (`FocusReveal::control_within`). Kept with the
/// attribution, so Back to an earlier one finds its pane where it was left.
#[derive(Clone)]
struct Reveals {
    panel: FocusReveal,
    footer: FocusReveal,
    /// The line history's capped list, where the footer has its full height
    /// and the list scrolls by itself.
    history: FocusReveal,
}

impl Default for Reveals {
    fn default() -> Self {
        let keyboard = || FocusReveal::new(Trigger::Keyboard);
        Self {
            panel: keyboard(),
            footer: keyboard(),
            history: keyboard(),
        }
    }
}

impl Reveals {
    /// Whether focus rests on a control this frame's form of the pane does
    /// not draw, its handle kept only because it holds focus: a line-history
    /// hash in the form the pane has just left, each form tracking its own
    /// handle, or the compact source line, a Button only while compact.
    fn focus_removed(&self, compact: bool, footer_full: bool, window: &Window) -> bool {
        let footer_hash = matches!(
            self.footer.focused(window),
            Some(ElementId::NamedInteger(name, _)) if name.as_ref() == "line-history-hash"
        );
        let hash = if footer_full {
            footer_hash
        } else {
            self.history.focused(window).is_some()
        };
        hash || (!compact
            && self.panel.focused(window) == Some(ElementId::from("blame-source-scope")))
    }

    /// `button` among the pane's own rows, tracked as `id`, revealed whole
    /// with its ring's `room` in the pane's scroll.
    fn track_panel(&self, id: &'static str, button: Button, room: Pixels, cx: &mut App) -> Button {
        let focus = self.panel.focus(id, cx);
        let panel = self.panel.control(focus.clone(), room);
        gpui_kit::base::ElementExt::on_prepaint(
            button.track_focus(&focus),
            move |bounds, window, cx| panel(vec![bounds], window, cx),
        )
    }

    /// `button`, a line-history commit's hash in the capped list, tracked as
    /// `id`, revealed whole in the list and in the footer and pane around it.
    fn track_list(
        &self,
        id: impl Into<ElementId>,
        button: Button,
        room: Pixels,
        cx: &mut App,
    ) -> Button {
        let focus = self.history.focus(id, cx);
        let history = self.history.control(focus.clone(), room);
        let footer = self
            .footer
            .control_within(focus.clone(), room, &self.history);
        let panel = self.panel.control_within(focus.clone(), room, &self.footer);
        gpui_kit::base::ElementExt::on_prepaint(
            button.track_focus(&focus),
            move |bounds, window, cx| {
                history(vec![bounds], window, cx);
                footer(vec![bounds], window, cx);
                panel(vec![bounds], window, cx);
            },
        )
    }

    /// `button`, a footer control tracked as `id`, revealed whole with its
    /// ring's `room` in the footer's scroll and in the pane's, where the
    /// footer has just scrolled it.
    fn track(
        &self,
        id: impl Into<ElementId>,
        button: Button,
        room: Pixels,
        cx: &mut App,
    ) -> Button {
        let focus = self.footer.focus(id, cx);
        let footer = self.footer.control(focus.clone(), room);
        let panel = self.panel.control_within(focus.clone(), room, &self.footer);
        gpui_kit::base::ElementExt::on_prepaint(
            button.track_focus(&focus),
            move |bounds, window, cx| {
                footer(vec![bounds], window, cx);
                panel(vec![bounds], window, cx);
            },
        )
    }
}

#[derive(Default)]
pub(super) struct State {
    visible: bool,
    focus: Option<FocusHandle>,
    /// Whether a compact header's source line shows the lineage note it
    /// gave up beneath it, the press the keyboard reaches it by.
    note_open: bool,
    reveals: Reveals,
    previous: Option<Box<State>>,
    repository: Option<PathBuf>,
    target: Option<BlameTarget>,
    data: Option<Blame>,
    history: Option<LineHistory>,
    selected: Option<usize>,
    pending: Option<Pending>,
    error: Option<String>,
    task: Option<Task<()>>,
    scroll: UniformListScrollHandle,
    return_context: Option<(WorkspaceMode, Pane, bool, Option<FocusHandle>)>,
}

impl State {
    pub(super) fn rescale_lists(&self, scales: settings::ListScales) {
        settings::rescale_list_scroll(&self.scroll, scales.history);
        if let Some(previous) = &self.previous {
            previous.rescale_lists(scales);
        }
    }
    pub(super) fn pause_for_tab(&mut self) {
        self.task = None;
        if self.pending.take().is_some() {
            self.error = Some("Attribution paused on tab switch; Reload to continue.".into());
        }
        if let Some(previous) = &mut self.previous {
            previous.pause_for_tab();
        }
    }
    pub(super) fn retained_bytes(&self) -> usize {
        self.previous
            .as_ref()
            .map_or(0, |previous| previous.retained_bytes())
            + self.history.as_ref().map_or(0, |history| {
                history.commits.iter().map(Commit::history_bytes).sum()
            })
            + self.data.as_ref().map_or(0, |data| {
                data.lines
                    .iter()
                    .map(|line| {
                        std::mem::size_of_val(line)
                            + line.text.capacity()
                            + line.attribution.as_ref().map_or(0, |attribution| {
                                std::mem::size_of_val(attribution.as_ref())
                                    + attribution.oid.capacity()
                                    + attribution.author.capacity()
                                    + attribution.subject.capacity()
                                    + attribution.path.as_os_str().len()
                            })
                    })
                    .sum()
            })
    }
    pub(super) fn is_visible(&self) -> bool {
        self.visible
    }
    /// How far the footer's content reaches past its view.
    #[cfg(test)]
    pub(crate) fn footer_overflow(&self) -> Pixels {
        self.reveals.footer.scroll().max_offset().y
    }
    /// How far the footer is scrolled.
    #[cfg(test)]
    pub(crate) fn footer_offset(&self) -> Pixels {
        self.reveals.footer.scroll().offset().y
    }
    /// The capped line-history list's reveal.
    #[cfg(test)]
    pub(crate) fn history_reveal(&self) -> FocusReveal {
        self.reveals.history.clone()
    }
    /// Points the open attribution at `path`, as a working-tree one reads.
    #[cfg(test)]
    pub(crate) fn name_target(&mut self, path: &str) {
        self.target = Some(BlameTarget::Working { path: path.into() });
    }
    /// Marks the open attribution as read from a shallow repository.
    #[cfg(test)]
    pub(crate) fn mark_shallow(&mut self) {
        if let Some(data) = self.data.as_mut() {
            data.shallow = true;
        }
    }
    /// How far the whole pane's content reaches past its view.
    #[cfg(test)]
    pub(crate) fn panel_overflow(&self) -> Pixels {
        self.reveals.panel.scroll().max_offset().y
    }
    /// The footer's reveal.
    #[cfg(test)]
    pub(crate) fn footer_reveal(&self) -> FocusReveal {
        self.reveals.footer.clone()
    }
    /// The pane's own reveal.
    #[cfg(test)]
    pub(crate) fn panel_reveal(&self) -> FocusReveal {
        self.reveals.panel.clone()
    }
    /// How many commits the shown line history lists.
    #[cfg(test)]
    pub(crate) fn history_rows(&self) -> usize {
        self.history
            .as_ref()
            .map_or(0, |history| history.commits.len())
    }
    /// The attribution lines' focus handle, while a target is open.
    #[cfg(test)]
    pub(crate) fn focus(&self) -> Option<FocusHandle> {
        self.focus.clone()
    }
    /// Whether the shown attribution lines draw `ring`, the window's keyboard
    /// ring, on their edge this frame.
    pub(super) fn ring_shows(&self, ring: &crate::focus_ring::FocusRing) -> bool {
        self.visible && self.focus.as_ref().is_some_and(|focus| ring.shows(focus))
    }
    pub(super) fn owns_focus(&self, window: &Window) -> bool {
        self.visible
            && self
                .focus
                .as_ref()
                .is_some_and(|focus| focus.is_focused(window))
    }
    pub(super) fn hide(&mut self) {
        self.visible = false;
    }
    pub(super) fn set_visible(&mut self, visible: bool) {
        self.visible = visible && self.target.is_some();
    }
    fn depth(&self) -> usize {
        usize::from(self.target.is_some())
            + self
                .previous
                .as_ref()
                .map_or(0, |previous| previous.depth())
    }
    fn accepts(&self, repository: &Path, target: &BlameTarget, pending: Pending) -> bool {
        self.repository.as_deref() == Some(repository)
            && self.target.as_ref() == Some(target)
            && self.pending == Some(pending)
    }
}

impl GitTurtle {
    pub(super) fn open_blame(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != AppPage::Repository
            || self.operation_busy.is_some()
            || self.blame.is_visible()
        {
            return;
        }
        if self.blame.depth() >= 4 {
            self.error = Some(
                "Four attribution contexts are already open. Use Back before opening another."
                    .into(),
            );
            cx.notify();
            return;
        }
        let Some(repo) = &self.repository else {
            return;
        };
        let target = if self.mode == WorkspaceMode::Working {
            let Some(entry) = self
                .work_status
                .as_ref()
                .and_then(|status| status.entries.get(index))
            else {
                return;
            };
            BlameTarget::Working {
                path: entry.path.clone(),
            }
        } else if let Some(target) = self.file_history_blame_target() {
            target
        } else if self.revision_inspection.is_active() {
            let Some(target) = self.inspection_blame_target(index) else {
                self.error = Some("This file is absent on the selected comparison side. Choose its existing side for attribution.".into());
                cx.notify();
                return;
            };
            target
        } else {
            let (Some(file), Some(commit)) = (
                self.files.get(index),
                self.selected_commit.and_then(|i| self.commits.get(i)),
            ) else {
                return;
            };
            BlameTarget::Committed {
                oid: commit.oid.clone(),
                path: file.path().to_owned(),
            }
        };
        let previous = self
            .blame
            .target
            .is_some()
            .then(|| Box::new(std::mem::take(&mut self.blame)));
        self.blame = State {
            visible: true,
            // The lines are one Tab stop: GPUI ignores the `tab_stop` of the
            // div that tracks this handle, so the handle carries it.
            focus: Some(cx.focus_handle().tab_stop(true)),
            previous,
            repository: Some(repo.path().to_owned()),
            target: Some(target),
            return_context: Some((
                self.mode,
                self.pane,
                self.sidebar,
                self.inspection_opener(window, cx),
            )),
            ..Default::default()
        };
        if self.mode == WorkspaceMode::History {
            self.mode = WorkspaceMode::Compare;
            self.sidebar = false;
        }
        self.pane = Pane::Files;
        if let Some(focus) = &self.blame.focus {
            window.focus(focus, cx);
        }
        self.load_blame(window, cx);
    }

    pub(super) fn close_blame(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.blame.is_visible() {
            return false;
        }
        self.invalidate_read();
        if let Some((mode, pane, sidebar, focus)) = self.blame.return_context.take() {
            self.mode = mode;
            self.pane = pane;
            self.sidebar = sidebar;
            // Opened with nothing focused, or with the window's root focused
            // by a click on an empty spot (`saved_focus`), attribution
            // returns focus to the shown mode's own list and names its pane:
            // given back, the root showed no focus and Down moved nothing.
            let focus = self.saved_focus(focus);
            if self.page == AppPage::Repository {
                let focus = focus.unwrap_or_else(|| self.take_mode_list());
                self.give_inspection_focus_back(focus, window, cx);
            } else if self.page_return_focus.is_some() && self.page_return_focus == self.blame.focus
            {
                // Closed behind Settings or Projects, as a status read
                // landing there closes it, focus stays on that page, and
                // Back returns it where the attribution would have, or to
                // the list where it had nothing to return to. Returned to
                // the attribution's own focus, which is no longer drawn,
                // keys went nowhere.
                self.page_return_focus = focus;
            }
        }
        self.blame = self
            .blame
            .previous
            .take()
            .map(|previous| *previous)
            .unwrap_or_default();
        if self.mode == WorkspaceMode::Compare && self.content.is_none() {
            self.read_selected_preview(window, cx);
        }
        // Blame holds quiet reads off while it is shown, as File history
        // does; they start once this handler has finished. A quiet read it
        // had held, a write's reload among them, which menus wait for once
        // it can run (`write_unsettled`), otherwise waited for the next local
        // change or another read's end.
        let owner = cx.entity().downgrade();
        window.on_next_frame(move |window, cx| {
            let _ = owner.update(cx, |this, cx| this.try_automatic_refresh(window, cx));
        });
        cx.notify();
        true
    }

    fn load_blame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(repo), Some(target)) = (self.repository.clone(), self.blame.target.clone())
        else {
            return;
        };
        if self.blame.repository.as_deref() != Some(repo.path()) {
            return;
        }
        // The comparison stays behind the attribution for its return, so a
        // review being prepared for it ends here with its read.
        self.release_text_review(cx);
        self.invalidate_read();
        let generation = self.generation;
        let repository = repo.path().to_owned();
        self.blame.pending = Some(Pending::Attribution);
        self.blame.error = None;
        self.blame.history = None;
        let response = self.worker.submit(Job::Blame {
            repo,
            target: target.clone(),
        });
        self.blame.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Attribution was interrupted. Reload to try again."
                ))
            });
            let _ =
                this.update_in(cx, |this, _, cx| {
                    if this.generation != generation
                        || this.path.as_ref() != Some(&repository)
                        || !this
                            .blame
                            .accepts(&repository, &target, Pending::Attribution)
                    {
                        return;
                    }
                    this.blame.pending = None;
                    this.blame.task = None;
                    match result {
                        Ok(Output::Blame(data)) if data.target == target => {
                            this.blame.selected = if data.lines.is_empty() {
                                None
                            } else {
                                Some(this.blame.selected.unwrap_or(0).min(data.lines.len() - 1))
                            };
                            this.blame.data = Some(data);
                        }
                        Ok(_) => this.blame.error = Some(
                            "The attribution result did not match this file. Reload to try again."
                                .into(),
                        ),
                        Err(error) => this.blame.error = Some(format!("{error:#}")),
                    }
                    cx.notify();
                });
        }));
        cx.notify();
    }

    fn select_blame_line(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if !self.blame.is_visible()
            || self
                .blame
                .data
                .as_ref()
                .is_none_or(|data| index >= data.lines.len())
        {
            return;
        }
        if matches!(self.blame.pending, Some(Pending::History(_))) {
            self.invalidate_read();
            self.blame.pending = None;
            self.blame.task = None;
        }
        self.blame.selected = Some(index);
        self.blame.history = None;
        self.blame.error = None;
        self.blame
            .scroll
            .scroll_to_item(index, ScrollStrategy::Center);
        if let Some(focus) = &self.blame.focus {
            window.focus(focus, cx);
        }
        cx.notify();
    }

    pub(super) fn move_blame_line(
        &mut self,
        direction: i32,
        edge: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.page != AppPage::Repository || !self.blame.owns_focus(window) {
            return false;
        }
        let count = self.blame.data.as_ref().map_or(0, |data| data.lines.len());
        if count > 0 {
            let index = if edge {
                if direction < 0 { 0 } else { count - 1 }
            } else {
                (self.blame.selected.unwrap_or(0) as i32 + direction).clamp(0, count as i32 - 1)
                    as usize
            };
            self.select_blame_line(index, window, cx);
        }
        true
    }

    /// Copies the selected line's text, confirmed on the Copy line button
    /// when `button` asked, or only announced for the list's Copy.
    pub(super) fn copy_blame_line(
        &self,
        button: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.blame.is_visible() {
            return false;
        }
        if let Some(line) = self
            .blame
            .selected
            .and_then(|index| self.blame.data.as_ref()?.lines.get(index))
        {
            let mark = button.then(|| self.copy_line_mark());
            copy_feedback::copy(window, cx, mark, "line", line.text.clone());
        }
        true
    }

    /// The Copy line button's mark: the attributed file, at its revision or
    /// in the working tree, and the selected line.
    fn copy_line_mark(&self) -> copy_feedback::Mark {
        let file = self.blame.target.as_ref().map(|target| match target {
            BlameTarget::Committed { oid, path } => (Some(oid), path),
            BlameTarget::Working { path } => (None, path),
        });
        copy_feedback::Mark::new("copy-blame-line", (file, self.blame.selected))
    }

    pub(super) fn open_blame_commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(attribution) = self.blame.selected.and_then(|index| {
            self.blame
                .data
                .as_ref()?
                .lines
                .get(index)?
                .attribution
                .clone()
        }) else {
            return;
        };
        self.open_file_history_at(
            attribution.oid.clone(),
            attribution.path.clone(),
            window,
            cx,
        );
    }

    pub(super) fn load_line_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(repo), Some(target), Some(index)) = (
            self.repository.clone(),
            self.blame.target.clone(),
            self.blame.selected,
        ) else {
            return;
        };
        let Some(line) = self
            .blame
            .data
            .as_ref()
            .and_then(|data| data.lines.get(index))
        else {
            return;
        };
        let Some(attribution) = line.attribution.clone() else {
            return;
        };
        let line_number = line.original_line;
        let oid = attribution.oid.clone();
        let path = attribution.path.clone();
        self.invalidate_read();
        let generation = self.generation;
        let repository = repo.path().to_owned();
        self.blame.pending = Some(Pending::History(index));
        self.blame.error = None;
        self.blame.history = None;
        let response = self.worker.submit(Job::LineHistory {
            repo,
            oid: oid.clone(),
            path: path.clone(),
            line: line_number,
        });
        self.blame.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Line history was interrupted. Select History of line to retry."
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                if this.generation != generation
                    || this.path.as_ref() != Some(&repository)
                    || this.blame.selected != Some(index)
                    || !this
                        .blame
                        .accepts(&repository, &target, Pending::History(index))
                {
                    return;
                }
                this.blame.pending = None;
                this.blame.task = None;
                match result {
                    Ok(Output::LineHistory(history))
                        if history.anchor == oid
                            && history.path == path
                            && history.line == line_number =>
                    {
                        // Said once, politely: its rows may land past the
                        // footer's view (DESIGN.md, Blame).
                        let count = history.commits.len();
                        let changes = if count == 1 { "change" } else { "changes" };
                        copy_feedback::announce(
                            window,
                            cx,
                            format!("Line history ready · {count} line {changes}"),
                        );
                        this.blame.history = Some(history)
                    }
                    Ok(_) => {
                        copy_feedback::announce(window, cx, "Line history could not be read");
                        this.blame.error = Some(
                            "The returned line history did not match this selection. Retry the read."
                                .into(),
                        )
                    }
                    Err(error) => {
                        copy_feedback::announce(window, cx, "Line history could not be read");
                        this.blame.error = Some(format!("{error:#}"))
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Blame in a pane `pane` big, in the frame it is drawn.
    pub(super) fn render_blame(
        &self,
        pane: Size<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(focus) = &self.blame.focus else {
            return empty(
                "Attribution unavailable",
                "Close this view and reopen attribution.",
            );
        };
        let p = palette(cx);
        let count = self.blame.data.as_ref().map_or(0, |data| data.lines.len());
        let selected = self
            .blame
            .selected
            .and_then(|index| self.blame.data.as_ref()?.lines.get(index));
        let attribution = selected.and_then(|line| line.attribution.as_ref());
        let path = self
            .blame
            .target
            .as_ref()
            .map_or(String::new(), |target| target.path().display().to_string());
        let busy = self.blame.pending.is_some();
        let working = matches!(self.blame.target, Some(BlameTarget::Working { .. }));
        let source = if working {
            "Working file · raw snapshot relative to HEAD".into()
        } else {
            self.blame
                .target
                .as_ref()
                .and_then(|target| match target {
                    BlameTarget::Committed { oid, .. } => {
                        Some(format!("Committed revision {}", short_oid(oid)))
                    }
                    _ => None,
                })
                .unwrap_or_default()
        };
        let lineage = if working {
            "Staged and unstaged edits are uncommitted. Detected renames retain HEAD attribution; copies and moved lines are not traced."
        } else {
            "Follows whole-file renames across merge parents. Copies and moved lines across files are not traced."
        };
        let summary = attribution.map_or_else(
            || {
                if selected.is_some() {
                    "Uncommitted line".into()
                } else {
                    "Select a line to inspect its origin".into()
                }
            },
            |attribution| attribution.subject.clone(),
        );
        let detail = attribution.map_or_else(
            || if selected.is_some() {
                "This line is uncommitted, so Compare commit and History of line are unavailable. Arrow keys select another line; Copy line preserves its exact source text.".into()
            } else {
                "Arrow keys select a line. Copy line preserves its exact source text.".into()
            },
            |attribution| format!("{} · {} · {}:{}", attribution.author, full_date(attribution.timestamp), attribution.path.display(), selected.map_or(0, |line| line.original_line)),
        );
        let action_reason = if busy {
            Some("unavailable while a read is in progress")
        } else if attribution.is_none() {
            Some(if selected.is_some() {
                "unavailable for an uncommitted line"
            } else {
                "unavailable until a committed line is selected"
            })
        } else {
            None
        };
        let action_label = |name: &str| {
            action_reason.map_or_else(|| name.to_owned(), |reason| format!("{name}, {reason}"))
        };
        let empty_title = if busy {
            "Reading attribution…"
        } else {
            "No source lines"
        };
        let empty_detail =
            "Choose a regular UTF-8 text file. Attribution is limited to 2 MiB and 100,000 lines.";
        // The lines keep three rows wherever the window can hold them with
        // the header; the footer gives up its height first, down to one
        // control with its ring's room, and scrolls within itself (DESIGN.md,
        // Blame). At 18 pt in a 461 × 490 window it left the lines 2 px.
        let least_lines = px(self.settings.density.history_row_height()) * 3.;
        let room = crate::appearance::button_ring_room(cx);
        // One control with its ring whole, below the footer's divider, which
        // stands beside its scrolling rows (`tags::ScrollCue::divider`): the
        // footer's least and its full height hold the divider and the rows.
        let least_footer = crate::appearance::ui_size(28.) + room * 2. + px(3.);
        let divider = crate::tags::ScrollCue::divider_height(window);
        // A pane that cannot hold the header at its usual height, three rows
        // and the footer's least: the header draws compact and gives up the
        // lineage note's line, which the source line's tooltip and name say,
        // and which the keyboard reaches on it. Decided from the room needed
        // and before layout (`ui_size(500)` was 692 px at 18 pt, so a 1,280
        // × 690 tile lost the note with room to spare).
        // A shallow repository's boundary line stands with it, so its known
        // height counts toward every threshold below: uncounted, a pane just
        // above the footer's kept the full footer with its first action cut.
        // The transient error banner is left out; it scrolls.
        let header = full_blame_header(&source, pane.width, window)
            + self
                .blame
                .data
                .as_ref()
                .filter(|data| data.shallow)
                .map_or(Pixels::ZERO, |_| shallow_line_height(pane.width, window));
        let compact = pane.height < header + least_lines + least_footer;
        // The footer keeps its full form only where that form's first row of
        // actions shows whole with its ring: decided with the header's, panes
        // of about 322 to 361 px at 18 pt showed a summary and a detail and
        // no action in view. Decided from the pane alone, the summary and
        // detail clamped to the lines it allows them, so arrowing through
        // lines with different subjects never changes the footer's form.
        let footer_compact =
            pane.height < header + least_lines + full_footer_first_action(room, window);
        // Where the pane holds the header, the lines' least and the footer's
        // full height, scaling with the text as the list's cap does, and is
        // wide enough for one row of actions, the line history keeps its
        // capped list and the summary and actions above it stay where they
        // are (DESIGN.md, Blame: at 13 pt nothing moves); only a shorter or
        // narrower pane merges its rows into the footer's own scroll. In
        // pixels, the cap nested inside a footer that scrolled at 18 pt and
        // wherever the actions wrapped. Where the text above the list wraps
        // past that height, the list gives way inside the footer.
        let limit_note = self.blame.history.as_ref().map(|history| {
            if history.truncated {
                TRUNCATED_LIMIT_NOTE
            } else {
                LIMIT_NOTE
            }
        });
        let footer_full = pane.height
            >= header + least_lines + crate::appearance::ui_size(FOOTER_FULL)
            && pane.width
                >= one_row_of_actions(
                    attribution.map(|line| short_oid(&line.oid)).as_deref(),
                    window,
                );
        // In the tight footer, History of line's own label says the read is
        // under way, as the Inspector's pressed control does: the rows land
        // below the wrapped actions, out of view, and nothing scrolls away
        // from what was pressed. In the full footer the row below says it.
        // A resize that takes the control holding focus away hands focus to
        // the lines, the region's own stop, so nothing is left focused that
        // no frame draws.
        if self
            .blame
            .reveals
            .focus_removed(compact, footer_full, window)
        {
            let lines = focus.clone();
            window.defer(cx, move |window, cx| lines.focus(window, cx));
        }
        let tracing = matches!(self.blame.pending, Some(Pending::History(_)));
        let line_history_label = if tracing && !footer_full {
            "Tracing…"
        } else {
            "History of line"
        };
        #[cfg(test)]
        LINE_HISTORY_LABEL_DRAWN.set(Some(line_history_label));
        // Each region reveals the control focus moves onto in its own rows,
        // inner before outer, so all show it whole in the frame that first
        // draws it.
        let reveals = &self.blame.reveals;
        reveals.history.reveal(window, cx);
        reveals.footer.reveal(window, cx);
        reveals.panel.reveal(window, cx);
        // A pane shorter than its parts' least heights scrolls as a whole,
        // the footer having given up its height first.
        // Where the whole pane scrolls, its gutter leaves the footer's clip
        // short of the widest action's ring: at 18 pt in a 461 × 420 window
        // History of line's ring ran 0.5 px past the footer. The footer takes
        // back the ring's room from that gutter, which keeps it for a ring.
        let cue = crate::tags::ScrollCue::revealing(&reveals.panel, Pixels::ZERO);
        // Decided from the pane, not the window, as Compare's toolbar decides
        // (a dragged inspector narrows the pane), and before layout, so no
        // frame draws the other form.
        let reload_labelled = pane.width >= crate::appearance::ui_size(RELOAD_LABEL_WIDTH);
        // The heading's room: the pane less the row's padding, Back, Reload,
        // their gaps and, where the pane scrolls, its gutter.
        let heading_room = {
            let rem = window.rem_size();
            let back = labelled_button_width("Back", &BACK_LABEL, window);
            let reload = if reload_labelled {
                labelled_button_width("Reload", &RELOAD_LABEL, window)
            } else {
                window.pixel_snap(crate::appearance::ui_size(28.))
            };
            let gutter = if cue.kept() {
                crate::tags::ScrollCue::gutter()
            } else {
                Pixels::ZERO
            };
            pane.width - gutter - rem * 0.75 * 2. - back - reload - rem * 0.5 * 2.
        };
        let footer_cue = crate::tags::ScrollCue::revealing(&reveals.footer, room);
        cue.list(div().on_children_prepainted(cue.observe(reveals.panel.observe())).id("blame-panel").debug_selector(|| "blame-panel".into()).overflow_y_scroll().track_scroll(reveals.panel.scroll()).size_full().flex().flex_col().min_w_0().bg(rgb(p.canvas))
            .child(div().flex_shrink_0().map(|row| if compact { row.px_3().py_1() } else { row.p_3() }).flex().items_center().gap_2().border_b_1().border_color(rgb(p.border))
                .child({
                    let back =button("close-blame", "Back", "arrow-left", false).accessibility_label("Back from attribution · restore previous view").on_click(cx.listener(|this, _, window, cx| { this.close_blame(window, cx); }));
                    reveals.track_panel("close-blame", back, room, cx)
                })
                .child({
                    // The file name stays in view in a narrow pane: the path
                    // gives way from its middle (`views::path_tail`), as
                    // Compare's does, and the name and tooltip keep it whole.
                    // Where the room after "Blame · " cannot hold even the
                    // name's least form, the prefix gives way: `truncate` cut
                    // what was left to "Blame · …" at 18 pt in a 461 px window.
                    const PREFIX: &str = "Blame · ";
                    let whole = format!("{PREFIX}{path}");
                    let width = |text: &str| crate::views::shaped_width(text, crate::appearance::ui_text(12.), FontWeight::SEMIBOLD, window).ceil();
                    let after_prefix = (heading_room - width(PREFIX)).max(Pixels::ZERO);
                    let fitted = crate::views::path_tail(&path, after_prefix, width);
                    let shown = if width(&fitted) <= after_prefix { format!("{PREFIX}{fitted}") } else { crate::views::path_tail(&path, heading_room.max(Pixels::ZERO), width) };
                    #[cfg(test)]
                    HEADING_DRAWN.with_borrow_mut(|drawn| *drawn = Some(shown.clone()));
                    let tooltip = whole.clone();
                    crate::native_accessibility::static_text(div().id("blame-heading").debug_selector(|| "blame-heading".into()).flex_1().min_w_0().truncate().text_size(crate::appearance::ui_text(12.)).font_weight(FontWeight::SEMIBOLD).tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx)).child(shown), whole)
                })
                .child({
                    // In a narrow window, Reload keeps its icon alone, its
                    // name and tooltip whole: labelled, it ran past the pane's
                    // edge at 18 pt in a 461 px window.
                    let reload_label = if reload_labelled { "Reload" } else { "" };
                    // Icon alone, its tooltip leads with the name its label
                    // would say (DESIGN.md, the responsive order).
                    let reload_tooltip = if reload_label.is_empty() { RELOAD_TOOLTIP_NAMED } else { RELOAD_TOOLTIP };
                    let reload_name = if busy { "Reload attribution, unavailable while a read is in progress" } else { "Reload attribution" };
                    #[cfg(test)]
                    RELOAD_TOOLTIP_DRAWN.set(Some((reload_tooltip, reload_name)));
                    let reload = button("reload-blame", reload_label, "refresh", false).debug_selector(|| "reload-blame".into()).accessibility_label(reload_name).disabled(busy).tooltip(reload_tooltip).on_click(cx.listener(|this, _, window, cx| this.load_blame(window, cx)));
                    reveals.track_panel("reload-blame", reload, room, cx)
                }))
            .child(div().flex_shrink_0().px_3().map(|block| if compact { block.py_1() } else { block.py_2() }).flex().flex_col().gap_1().border_b_1().border_color(rgb(p.border))
                .map(|block| if compact {
                    // The line the keyboard reaches: a press shows the lineage
                    // note beneath it, the note this form gave up.
                    let scope = self.blame_source_scope(&source, lineage, window, cx);
                    block.child(reveals.track_panel("blame-source-scope", scope, room, cx))
                } else {
                    block.child(crate::native_accessibility::static_text(div().id("blame-source-scope").debug_selector(|| "blame-source-scope".into()).text_size(crate::appearance::ui_text(11.)).child(source.clone()).children(crate::appearance::text_probe(|| SOURCE_SCOPE_TEXT.into())), source.clone()))
                })
                .when(compact && self.blame.note_open, |block| block.child(crate::native_accessibility::static_text(div().id("blame-source-note").debug_selector(|| "blame-source-note".into()).text_size(crate::appearance::ui_text(10.)).text_color(rgb(p.muted)).child(lineage), lineage)))
                // One line, its whole text in its tooltip and accessible
                // name, wherever it stands at all: in a short window it gives
                // up its line, and the source's tooltip and name say it, as
                // wrapped in the pane at 18 pt in a 461 × 490 window it took
                // the room three source rows need.
                .when(!compact, |block| block.child(crate::native_accessibility::static_text(div().id("blame-lineage-limit").debug_selector(|| "blame-lineage-limit".into()).min_w_0().truncate().text_size(crate::appearance::ui_text(10.)).text_color(rgb(p.muted)).tooltip(move |window, cx| Tooltip::new(lineage).build(window, cx)).child(lineage), lineage))))
            .children(self.blame.data.as_ref().filter(|data| data.shallow).map(|_| static_text("blame-shallow-boundary", SHALLOW_LINE).flex_shrink_0().px_3().py_2().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.warning))))
            .children(self.blame.error.as_ref().map(|error| static_text("blame-error", format!("Attribution unavailable: {error}")).flex_shrink_0().max_h(px(100.)).overflow_y_scroll().p_3().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.warning))))
            .child(crate::native_accessibility::clips_children(div().on_children_prepainted(reveals.panel.control(focus.clone(), room)).id("blame-lines-region").role(Role::ListBox).aria_label("Source lines and commit attribution").tab_stop(true).key_context("GitTurtleList").on_action(cx.listener(|this, _: &gpui_kit::component::input::Copy, window, cx| { this.copy_blame_line(false, window, cx); })).track_focus(focus).flex_1().min_h(least_lines).min_w_0().relative().debug_selector(|| "blame-lines-region".into())
                .child(if count == 0 { div().id("blame-empty-state").size_full().child(empty(empty_title, empty_detail)).into_any_element() } else {
                    uniform_list("blame-lines", count, cx.processor(|this, range: std::ops::Range<usize>, _, cx| range.map(|index| this.render_blame_row(index, cx)).collect::<Vec<_>>())).size_full().track_scroll(&self.blame.scroll).into_any_element()
                })
                // Its keyboard ring on its own edge, over the lines, so none of
                // them moves.
                .children(self.focus_ring.edge(focus, rgb(p.accent)))))
            // The footer's divider stands above it, beside the list rather
            // than on it, so it takes nothing from what the footer scrolls,
            // its fades or its track (`tags::ScrollCue::divider`), and spans
            // what the footer spans.
            .child(crate::tags::ScrollCue::divider(rgb(p.border)).when(cue.kept(), |line| line.mr(-room)))
            .child(footer_cue.list(div().on_children_prepainted(footer_cue.observe(reveals.footer.observe())).id("blame-footer").debug_selector(|| "blame-footer".into()).min_h_0().overflow_y_scroll().track_scroll(reveals.footer.scroll()).map(|footer| if footer_compact { footer.px_3().py_1() } else { footer.p_3() }).flex().flex_col().gap_2()
                // Compact, one line of summary, its full text with the
                // detail in its tooltip and name, so the first action starts
                // in view at 18 pt in a 461 × 490 window.
                .child({
                    let line = static_text("blame-selected-summary", summary.clone()).flex_shrink_0().text_size(crate::appearance::ui_text(12.)).font_weight(FontWeight::MEDIUM);
                    if footer_compact {
                        let whole = format!("{summary}\n{detail}");
                        line.min_w_0().truncate().aria_label(format!("{summary}. {detail}")).tooltip(move |window, cx| Tooltip::new(whole.clone()).build(window, cx)).into_any_element()
                    } else {
                        let whole = summary.clone();
                        line.line_clamp(FOOTER_TEXT_LINES).text_ellipsis().tooltip(move |window, cx| Tooltip::new(whole.clone()).build(window, cx)).into_any_element()
                    }
                })
                .when(!footer_compact, |footer| { let whole = detail.clone(); footer.child(static_text("blame-selected-details", detail).flex_shrink_0().line_clamp(FOOTER_TEXT_LINES).text_ellipsis().text_size(crate::appearance::ui_text(10.)).text_color(rgb(p.muted)).tooltip(move |window, cx| Tooltip::new(whole.clone()).build(window, cx))) })
                .child(div().flex().flex_shrink_0().flex_wrap().items_center().gap_2()
                    .child({
                        let compare = button("blame-compare-commit", "Compare commit", "", false).max_w_full().flex_shrink(1.).debug_selector(|| "blame-compare-commit".into()).accessibility_label(action_label("Compare commit")).disabled(attribution.is_none() || busy).tooltip("Open the line's originating commit; Back restores this attribution and selected line").on_click(cx.listener(|this, _, window, cx| this.open_blame_commit(window, cx)));
                        reveals.track("blame-compare-commit", compare, room, cx)
                    })
                    .child({
                        // "Tracing…" reads in the bounds "History of line"
                        // gives the control, starting at its inset after its
                        // icon, so the row of actions does not move (DESIGN.md,
                        // as Copied holds a control's bounds): the two stacked
                        // in one cell, which a pane narrower than the control
                        // can shrink to an ellipsis, as a minimum width could
                        // not.
                        let label_cell = |text: &'static str| div().row_start(1).col_start(1).min_w_0().whitespace_nowrap().text_ellipsis().line_height(relative(1.)).when(text != line_history_label, |cell| cell.invisible()).child(text);
                        let line_history = crate::button_frame("blame-line-history", line_history_label, "clock", false).icon(control_icon("clock")).max_w_full().flex_shrink(1.).child(div().debug_selector(|| "blame-line-history-label".into()).grid().grid_cols(1).flex_grow(1.).min_w_0().child(label_cell("History of line")).child(label_cell("Tracing…"))).debug_selector(|| "blame-line-history".into()).accessibility_label(action_label("History of line")).disabled(attribution.is_none() || busy).on_click(cx.listener(|this, _, window, cx| this.load_line_history(window, cx)));
                        reveals.track("blame-line-history", line_history, room, cx)
                    })
                    .child({
                        let copy = copy_feedback::copy_button(self.copy_line_mark(), "Copy line", "", cx).max_w_full().flex_shrink(1.).debug_selector(|| "blame-copy-line".into()).accessibility_label(if selected.is_some() { "Copy line" } else { "Copy line, unavailable until a line is selected" }).disabled(selected.is_none()).on_click(cx.listener(|this, _, window, cx| { this.copy_blame_line(true, window, cx); }));
                        reveals.track("blame-copy-line", copy, room, cx)
                    })
                    .children(attribution.map(|attribution| {
                        let hash = commit_hash_button(copy_feedback::Mark::new("copy-blame-commit", &attribution.oid), &attribution.oid, ORIGINATING_HASH, cx).max_w_full().flex_shrink(1.);
                        reveals.track("blame-copy-commit", hash, room, cx)
                    })))
                .children((tracing && footer_full).then(|| static_text("blame-line-history-loading", "Tracing the selected line…").flex_shrink_0().text_size(crate::appearance::ui_text(11.))))
                .children(self.blame.history.as_ref().map(|history| div().flex().flex_col().map(|rows| if footer_full { rows.min_h_0() } else { rows.flex_shrink_0() }).gap_1()
                    .child(static_text("blame-line-history-limit", limit_note.unwrap_or_default()).flex_shrink_0().text_size(crate::appearance::ui_text(10.)).text_color(rgb(p.muted)))
                    // Its capped list where the footer has its full height,
                    // which ends each frame of its own reveal, else its rows
                    // scroll with the footer, which holds them whole and
                    // reveals each hash as it takes focus.
                    .child(crate::native_accessibility::clips_children(div().when(footer_full, |list| list.on_children_prepainted(reveals.history.observe())).id("line-history-results").debug_selector(|| "line-history-results".into()).flex().flex_col().map(|list| if footer_full { list.min_h_0() } else { list.flex_shrink_0() }).when(footer_full, |list| list.max_h(crate::appearance::ui_size(LIST_CAP)).overflow_y_scroll().track_scroll(reveals.history.scroll())).children(history.commits.iter().enumerate().map(|(index, commit)| {
                        let hash = commit_hash_button(copy_feedback::Mark::new(("copy-line-history-commit", index), &commit.oid), &commit.oid, LINE_HISTORY_HASH, cx);
                        div().py_1().flex().items_center().gap_2().child(if footer_full { reveals.track_list(("line-history-hash", index), hash, room, cx) } else { reveals.track(("line-history-hash", index), hash, room, cx) }).child(static_text(("blame-line-history-subject", index), commit.subject.clone()).flex_1().truncate().text_size(crate::appearance::ui_text(11.))).child(static_text(("blame-line-history-author", index), commit.author.clone()).text_size(crate::appearance::ui_text(10.)).text_color(rgb(p.muted)))
                    })))))), "blame-footer-scrollbar").flex_shrink(1.).min_h(least_footer - divider).max_h(crate::appearance::ui_size(FOOTER_FULL) - divider).when(cue.kept(), |footer| footer.mr(-room))), "blame-panel-scrollbar").size_full().min_w_0().into_any_element()
    }

    /// The compact header's source line, the note its form gave up (`lineage`)
    /// in its name and tooltip, and a press, the keyboard's way to it, showing
    /// it beneath. A ghost Button standing at the text edge (DESIGN.md, one
    /// left edge): its text starts where the full form's does, its frame and
    /// hover fill spreading into the padding before it by the Button's own
    /// inset and fitting its text, at the full form's size, an ellipsis
    /// keeping it in a narrow pane. Its column would stretch it across the
    /// pane, the kit centring its label there.
    pub(super) fn blame_source_scope(
        &self,
        source: &str,
        lineage: &str,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Button {
        let pull = window.pixel_snap(window.rem_size() * 0.25);
        let tooltip = SharedString::from(format!("{source}\n{lineage}"));
        let name = SharedString::from(if self.blame.note_open {
            format!("{source}. Hide the lineage note")
        } else {
            format!("{source}. Show the lineage note: {lineage}")
        });
        #[cfg(test)]
        {
            SOURCE_SCOPE_TOOLTIP.with_borrow_mut(|drawn| *drawn = Some(tooltip.clone()));
            SOURCE_SCOPE_NAME.with_borrow_mut(|drawn| *drawn = Some(name.clone()));
        }
        Button::new("blame-source-scope")
            .debug_selector(|| "blame-source-scope".into())
            .ghost()
            .xsmall()
            .compact()
            .self_start()
            .max_w_full()
            .ml(-pull)
            .text_size(crate::appearance::ui_text(11.))
            .child(
                div()
                    .debug_selector(|| SOURCE_SCOPE_TEXT.into())
                    .min_w_0()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .line_height(relative(1.))
                    .child(source.to_owned())
                    .children(crate::appearance::text_probe(|| SOURCE_SCOPE_TEXT.into())),
            )
            .accessibility_label(name)
            .aria_expanded(self.blame.note_open)
            .tooltip(tooltip)
            .on_click(cx.listener(|this, _, _, cx| {
                this.blame.note_open = !this.blame.note_open;
                cx.notify();
            }))
    }

    fn render_blame_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(line) = self
            .blame
            .data
            .as_ref()
            .and_then(|data| data.lines.get(index))
        else {
            return div().into_any_element();
        };
        let p = palette(cx);
        let selected = self.blame.selected == Some(index);
        let label = line.attribution.as_ref().map_or_else(
            || "Uncommitted".into(),
            |a| format!("{} · {}", short_oid(&a.oid), a.author),
        );
        let source = line.text.trim_end_matches(['\r', '\n']);
        let full = display_excerpt(source, 1024);
        let tooltip = if full != source {
            format!(
                "{full}\nLong line shortened for display. Copy line preserves the complete source."
            )
        } else {
            full.clone()
        };
        div()
            .id(("blame-line", index))
            .role(Role::ListBoxOption)
            .aria_selected(selected)
            .when(selected, |row| row.aria_active_descendant())
            .aria_label(format!("Line {} · {} · {}", index + 1, label, full))
            .h(px(self.settings.density.history_row_height()))
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .border_l_2()
            .border_color(rgb(if selected { p.accent } else { p.canvas }))
            .bg(rgb(if selected { p.selected } else { p.canvas }))
            .hover(|style| style.bg(rgb(p.row_hover(selected))))
            .cursor_pointer()
            .child(
                div()
                    .w(px(44.))
                    .flex_shrink_0()
                    .text_right()
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(p.muted))
                    .child((index + 1).to_string()),
            )
            .child(
                div()
                    .w(px(180.))
                    .flex_shrink_0()
                    .truncate()
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(if line.attribution.is_some() {
                        p.muted
                    } else {
                        p.modified
                    }))
                    .child(label),
            )
            .child(
                blame_source(index, cx)
                    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                    .child(full),
            )
            .on_click(
                cx.listener(move |this, _, window, cx| this.select_blame_line(index, window, cx)),
            )
            .into_any_element()
    }
}

/// Blame's header at its usual height in a pane `pane` wide: Back and
/// Reload's row, the source line as it wraps across the pane and the lineage
/// note beneath it, each line at the toolkit's line height for its text size
/// and the padding in rems, as it is laid out. In literal pixels and one
/// source line, the header counted 29 px short of what a 243.5 px pane drew
/// at 18 pt, a band of window heights that scrolled the whole pane.
fn full_blame_header(source: &str, pane: Pixels, window: &Window) -> Pixels {
    use crate::appearance::{ui_size, ui_text};
    let rem = window.rem_size();
    let line = |size: f32| (ui_text(size) * 1.618).ceil();
    let weight = window.text_style().font_weight;
    let source_lines = wrapped_lines(source, ui_text(11.), weight, pane - rem * 1.5, window);
    let row = ui_size(28.) + rem * 0.75 * 2. + px(1.);
    let block = rem * 0.5 * 2. + line(11.) * source_lines as f32 + rem * 0.25 + line(10.) + px(1.);
    row + block
}

/// What a shallow repository's Blame says under its header.
const SHALLOW_LINE: &str =
    "Shallow repository · attribution and line history stop at the locally available boundary.";

/// That line's height in a pane `pane` wide: its text as it wraps across the
/// pane, at the toolkit's line height for its size, and its padding.
fn shallow_line_height(pane: Pixels, window: &Window) -> Pixels {
    use crate::appearance::ui_text;
    let rem = window.rem_size();
    let weight = window.text_style().font_weight;
    let lines = wrapped_lines(SHALLOW_LINE, ui_text(11.), weight, pane - rem * 1.5, window);
    rem + (ui_text(11.) * 1.618).ceil() * lines as f32
}

/// The lines the full footer's summary and detail each take at most.
const FOOTER_TEXT_LINES: usize = 2;

/// The full footer's height down to its first row of actions and that row's
/// ring: its top border and padding, [`FOOTER_TEXT_LINES`] of summary and of
/// detail, each with the gap below it, and the row.
fn full_footer_first_action(room: Pixels, window: &Window) -> Pixels {
    use crate::appearance::{ui_size, ui_text};
    let rem = window.rem_size();
    let lines = |size: f32| (ui_text(size) * 1.618).ceil() * FOOTER_TEXT_LINES as f32;
    px(1.) + rem * 0.75 + lines(12.) + rem * 0.5 + lines(10.) + rem * 0.5 + ui_size(28.) + room
}

/// The line history's limit note, under its cap and at it.
const LIMIT_NOTE: &str =
    "Line changes · first-parent lineage; copies and other merge parents are excluded.";
const TRUNCATED_LIMIT_NOTE: &str = "Latest 100 line changes · limit reached. First-parent lineage; copies and other merge parents are excluded.";

/// The lines `text` takes wrapped in `room`, as its text element shapes it.
fn wrapped_lines(
    text: &str,
    size: Pixels,
    weight: FontWeight,
    room: Pixels,
    window: &Window,
) -> usize {
    let mut style = window.text_style();
    style.font_weight = weight;
    window
        .text_system()
        .shape_text(
            SharedString::from(text.to_owned()),
            size,
            &[style.to_run(text.len())],
            Some(room.max(px(1.))),
            None,
        )
        .map_or(1, |lines| {
            lines
                .iter()
                .map(|line| 1 + line.wrap_boundaries().len())
                .sum::<usize>()
                .max(1)
        })
}

/// A Blame line's source text, drawn in the code font.
fn blame_source(index: usize, cx: &App) -> Stateful<Div> {
    div()
        .id(("blame-source", index))
        .flex_1()
        .min_w_0()
        .truncate()
        .code_font(cx)
        .text_size(crate::appearance::ui_text(11.))
}

use crate::native_accessibility::text as static_text;

/// Cap text shaping for unusually long source lines, at the longest prefix
/// of whole grapheme clusters within `limit` characters
/// (`text_cut::within_characters`). Exact source stays in the bounded result
/// and is used by Copy, independently of the displayed excerpt.
fn display_excerpt(source: &str, limit: usize) -> String {
    match crate::text_cut::within_characters(source, limit) {
        Some(end) => format!("{}…", &source[..end]),
        None => source.to_owned(),
    }
}

/// The footer's full height at 13 pt, and the line history's capped list's,
/// scaling with the interface text as the rows in them do.
const FOOTER_FULL: f32 = 260.;
const LIST_CAP: f32 = 130.;

thread_local! {
    /// "History of line"'s width, which "Tracing…" keeps.
    static HISTORY_OF_LINE: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static COMPARE_COMMIT: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static COPY_LINE: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static COPIED: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static SHORT_HASH: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
}

/// How wide the footer's actions are on one row, measured as they are drawn:
/// its padding, Compare commit, History of line and Copy line, and the
/// originating commit's `hash`, as wide as its text or "Copied", where the
/// selected line has one, with the gaps between them.
fn one_row_of_actions(hash: Option<&str>, window: &Window) -> Pixels {
    let compare = placed_button_width(
        label_text_width("Compare commit", &COMPARE_COMMIT, window),
        false,
        window,
    );
    let history = labelled_button_width("History of line", &HISTORY_OF_LINE, window);
    let copy = labelled_button_width("Copy line", &COPY_LINE, window);
    let hash = hash.map(|hash| {
        labelled_button_width(hash, &SHORT_HASH, window)
            .max(labelled_button_width("Copied", &COPIED, window))
    });
    let controls = 3. + f32::from(hash.is_some() as u8);
    let rem = window.rem_size();
    window.pixel_snap(rem * 0.75) * 2.
        + compare
        + history
        + copy
        + hash.unwrap_or(Pixels::ZERO)
        + rem * 0.5 * (controls - 1.)
}

/// What the source line's text is drawn and probed under, in either form.
const SOURCE_SCOPE_TEXT: &str = "blame-source-scope-text";

/// The narrowest pane, at 13 pt, that holds Back, the heading's least room and
/// Reload with its label beside them.
const RELOAD_LABEL_WIDTH: f32 = 260.;

#[cfg(test)]
thread_local! {
    /// The label History of line was drawn with last.
    static LINE_HISTORY_LABEL_DRAWN: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

/// The label History of line was drawn with last.
#[cfg(test)]
pub(crate) fn line_history_label_drawn() -> Option<&'static str> {
    LINE_HISTORY_LABEL_DRAWN.get()
}

thread_local! {
    static BACK_LABEL: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static RELOAD_LABEL: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
thread_local! {
    /// The heading Blame drew last.
    static HEADING_DRAWN: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The heading Blame drew last.
#[cfg(test)]
pub(crate) fn heading_drawn() -> Option<String> {
    HEADING_DRAWN.with_borrow(Clone::clone)
}

/// Reload's tooltip where its label says its name.
const RELOAD_TOOLTIP: &str = "Read a fresh snapshot of this file";
/// Reload's tooltip while it is its icon alone, leading with its name.
const RELOAD_TOOLTIP_NAMED: &str = "Reload · Read a fresh snapshot of this file";

#[cfg(test)]
thread_local! {
    /// The tooltip and accessible name Reload was drawn with last.
    static RELOAD_TOOLTIP_DRAWN: std::cell::Cell<Option<(&'static str, &'static str)>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
thread_local! {
    /// The name the compact source line was drawn with last.
    static SOURCE_SCOPE_NAME: std::cell::RefCell<Option<SharedString>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
thread_local! {
    /// The tooltip the compact source line was drawn with last.
    static SOURCE_SCOPE_TOOLTIP: std::cell::RefCell<Option<SharedString>> = const { std::cell::RefCell::new(None) };
}

/// The tooltip the compact source line was drawn with last.
#[cfg(test)]
pub(crate) fn source_scope_tooltip() -> Option<SharedString> {
    SOURCE_SCOPE_TOOLTIP.with_borrow(Clone::clone)
}

/// The name the compact source line was drawn with last.
#[cfg(test)]
pub(crate) fn source_scope_name() -> Option<SharedString> {
    SOURCE_SCOPE_NAME.with_borrow(Clone::clone)
}

/// The tooltip and accessible name Reload was drawn with last.
#[cfg(test)]
pub(crate) fn reload_tooltip_drawn() -> Option<(&'static str, &'static str)> {
    RELOAD_TOOLTIP_DRAWN.get()
}

/// Blame's originating commit hash button: its name, then its tooltip.
const ORIGINATING_HASH: (&str, &str) = (
    "Copy originating commit hash",
    "Copy originating commit hash",
);
/// A line-history commit's hash button: its name, then its tooltip.
const LINE_HISTORY_HASH: (&str, &str) = (
    "Copy line-history commit hash",
    "Copy this line-history commit hash",
);

/// A copy of `oid`, a commit Blame shows, drawn as its short ID after the hash glyph and named
/// by its action, `text`'s name, with `text`'s tooltip: named by the short ID alone, it told a
/// screen reader nothing of what it did, where the inspector's Hash is "Copy full commit hash".
fn commit_hash_button(
    mark: copy_feedback::Mark,
    oid: &str,
    text: (&'static str, &'static str),
    cx: &App,
) -> Button {
    let (name, tooltip) = text;
    let (copy_mark, oid) = (mark.clone(), oid.to_owned());
    copy_feedback::copy_hash_button(mark, short_oid(&oid), tooltip, cx)
        .accessibility_label(name)
        .on_click(move |_, window, cx| {
            copy_feedback::copy(
                window,
                cx,
                Some(copy_mark.clone()),
                "commit hash",
                oid.clone(),
            )
        })
}

/// Returns once the attribution `open_blame` started has landed and been
/// drawn. It is read on the app's reader, a thread `run_until_parked` cannot
/// wait for, so the test waits for that thread, which holds the test clock,
/// and then parks to handle the reply.
#[cfg(test)]
pub(crate) fn lands(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    app.read_with(cx, |app, _| app.worker.wait_idle());
    crate::views::tests::draw(cx);
    assert!(
        app.read_with(cx, |app, _| app.blame.pending.is_none()
            && app.blame.task.is_none()),
        "the attribution lands"
    );
}

#[cfg(test)]
mod tests {
    use super::{Pending, State};
    use gitturtle_core::BlameTarget;
    use gpui_kit::{IntoElement as _, ParentElement as _};
    use std::path::Path;

    /// A long line's excerpt ends between whole grapheme clusters: a letter
    /// with its combining marks, a ZWJ emoji sequence or a flag at the cut is
    /// left out whole rather than split, the excerpt still within its count.
    #[test]
    fn a_long_lines_excerpt_keeps_whole_grapheme_clusters() {
        use crate::text_cut::tests::{CLUSTERS, assert_whole_prefix};
        for (case, cluster) in CLUSTERS {
            let source = format!("{}{cluster}tail", "a".repeat(9));
            let excerpt = super::display_excerpt(&source, 10);
            let kept = excerpt.strip_suffix('…').expect("an excerpt");
            assert_whole_prefix(&source, kept, case);
            assert_eq!(kept, "a".repeat(9), "{case}");
        }
    }

    /// Blame's two commit hash buttons draw the short ID and are named by what they copy, as
    /// the inspector's Hash is: they were named by the short ID alone.
    #[gpui_kit::test]
    fn commit_hash_buttons_are_named_by_their_action(cx: &mut gpui_kit::TestAppContext) {
        use gpui::Element as _;
        let (_app, cx) = crate::copy_feedback::tests::history(cx, Vec::new());
        let oid = "0123456789abcdef0123456789abcdef01234567";
        for (text, name) in [
            (super::ORIGINATING_HASH, "Copy originating commit hash"),
            (super::LINE_HISTORY_HASH, "Copy line-history commit hash"),
        ] {
            let node = crate::views::tests::view_node(cx, move |window, cx| {
                let mark = crate::copy_feedback::Mark::new("copy-blame-commit", oid);
                let button = super::commit_hash_button(mark, oid, text, cx).base(window, cx);
                let element = gpui::RenderOnce::render(button, window, cx).into_element();
                let mut node = gpui::accesskit::Node::new(element.a11y_role().expect("a role"));
                element.write_a11y_info(&mut node);
                node
            });
            assert_eq!(node.label(), Some(name), "{text:?}");
        }
    }

    #[gpui_kit::test]
    fn blame_source_uses_the_code_font(cx: &mut gpui_kit::TestAppContext) {
        let font = crate::shaped_code_font(cx, |probe, cx| {
            super::blame_source(0, cx).child(probe).into_any_element()
        });
        assert_eq!(font.family, "Desktop Mono");
        assert_eq!(font.features.is_calt_enabled(), Some(false));
        assert!(font.features.tag_value_list().contains(&("liga".into(), 0)));
    }
    #[test]
    fn scaling_preserves_active_and_nested_attribution_rows() {
        use crate::settings::ListScales;
        use gpui_kit::{point, px};
        let state = State {
            selected: Some(50),
            previous: Some(Box::new(State {
                selected: Some(75),
                ..Default::default()
            })),
            ..Default::default()
        };
        state
            .scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(-3400.)));
        let previous = state.previous.as_ref().unwrap();
        previous
            .scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(-6800.)));
        state.rescale_lists(ListScales {
            history: 42. / 34.,
            files: 1.,
            navigation: 1.,
            lineage: 1.,
        });
        assert_eq!(state.scroll.0.borrow().base_handle.offset().y, px(-4200.));
        assert_eq!(
            previous.scroll.0.borrow().base_handle.offset().y,
            px(-8400.)
        );
        assert_eq!(state.selected, Some(50));
        assert_eq!(previous.selected, Some(75));
    }

    #[test]
    fn stale_target_repository_and_line_history_selection_are_rejected() {
        let target = BlameTarget::Working {
            path: "source".into(),
        };
        let state = State {
            repository: Some("/fixture".into()),
            target: Some(target.clone()),
            pending: Some(Pending::History(3)),
            ..Default::default()
        };
        assert!(state.accepts(Path::new("/fixture"), &target, Pending::History(3)));
        assert!(!state.accepts(Path::new("/other"), &target, Pending::History(3)));
        assert!(!state.accepts(Path::new("/fixture"), &target, Pending::History(4)));
        assert!(!state.accepts(
            Path::new("/fixture"),
            &BlameTarget::Working {
                path: "different".into()
            },
            Pending::History(3)
        ));
    }

    /// Attribution opened over a Quick Open working-tree source, whose read
    /// it cancels, and closed, reads the source again as it was opened, by
    /// its tracked read and no comparison read: the file's working content
    /// shows. Read again as a comparison, the source asked for neither
    /// side's object, since its working side has none, and showed an empty
    /// file.
    #[gpui_kit::test]
    async fn closing_attribution_over_a_source_shows_its_content(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::{Content, GitTurtle, WorkspaceMode};
        use gitturtle_core::{HistoryCancellation, PathScope};
        let (_fixture, app, cx) = crate::views::tests::history_window(cx).await;
        // The fixture's history is imported: its file is checked out and
        // changed, for a working-tree source of its own.
        let path = app.read_with(cx, |app, _| app.path.clone().unwrap());
        crate::tags::tests::git(&path, &["reset", "--quiet", "--hard"]);
        std::fs::write(path.join("notes.txt"), "working notes\n").unwrap();
        crate::views::tests::settle(&app, cx).await;
        let reads = |app: &GitTurtle| (app.worker.previews(), app.worker.tracked_previews());
        let held = app.read_with(cx, |app, _| {
            app.worker.wait_idle();
            app.worker.hold()
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                let repo = app.repository.clone().expect("a repository");
                let paths = repo
                    .search_tracked_paths(
                        &PathScope::Worktree,
                        "notes",
                        &HistoryCancellation::default(),
                    )
                    .expect("tracked paths");
                let entry = paths.entries.first().expect("the file").clone();
                app.show_tracked_file(paths.scope.clone(), entry, window, cx);
                assert_eq!(app.loading, Some("Reading tracked file…"));
                app.open_blame(0, window, cx);
                assert!(app.blame.is_visible());
            })
        });
        drop(held);
        crate::views::tests::settle(&app, cx).await;
        let (previews, tracked) = app.read_with(cx, |app, _| {
            assert!(app.content.is_none(), "the source's read was cancelled");
            reads(app)
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                assert!(app.close_blame(window, cx));
                assert_eq!(app.loading, Some("Reading tracked file…"));
            })
        });
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(reads(app), (previews, tracked + 1), "by its tracked read");
            assert!(app.mode == WorkspaceMode::Compare && app.is_quick_source());
            assert!(
                matches!(
                    app.content.as_deref(),
                    Some(Content::Text { new, .. }) if new == "working notes\n"
                ),
                "its working content shows"
            );
        });
    }
}
