//! Transient attribution with retained comparison state and replaceable reads.
use crate::*;
use gitturtle_core::{Blame, BlameTarget, LineHistory};
use gpui_kit::prelude::FluentBuilder;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    Attribution,
    History(usize),
}

#[derive(Default)]
pub(super) struct State {
    visible: bool,
    focus: Option<FocusHandle>,
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
    pub(super) fn is_visible(&self) -> bool {
        self.visible
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
            focus: Some(cx.focus_handle()),
            previous,
            repository: Some(repo.path().to_owned()),
            target: Some(target),
            return_context: Some((self.mode, self.pane, self.sidebar, window.focused(cx))),
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
            if let Some(focus) = focus {
                window.focus(&focus, cx);
            }
        }
        self.blame = self
            .blame
            .previous
            .take()
            .map(|previous| *previous)
            .unwrap_or_default();
        if self.mode == WorkspaceMode::Compare
            && self.content.is_none()
            && let Some(index) = self.selected_file
        {
            self.load_file(index, window, cx);
        }
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

    pub(super) fn copy_blame_line(&self, cx: &mut Context<Self>) -> bool {
        if !self.blame.is_visible() {
            return false;
        }
        if let Some(line) = self
            .blame
            .selected
            .and_then(|index| self.blame.data.as_ref()?.lines.get(index))
        {
            cx.write_to_clipboard(ClipboardItem::new_string(line.text.clone()));
        }
        true
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

    fn load_line_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
            let _ = this.update_in(cx, |this, _, cx| {
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
                        this.blame.history = Some(history)
                    }
                    Ok(_) => this.blame.error = Some(
                        "The returned line history did not match this selection. Retry the read."
                            .into(),
                    ),
                    Err(error) => this.blame.error = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn render_blame(&self, cx: &mut Context<Self>) -> AnyElement {
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
        div().size_full().flex().flex_col().min_w_0().bg(rgb(p.canvas))
            .child(div().flex_shrink_0().p_3().flex().items_center().gap_2().border_b_1().border_color(rgb(p.border))
                .child(button("close-blame", "Back", "arrow-left", false).accessibility_label("Back from attribution · restore previous view").on_click(cx.listener(|this, _, window, cx| { this.close_blame(window, cx); })))
                .child(static_text("blame-heading", format!("Blame · {path}")).flex_1().min_w_0().truncate().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD))
                .child(button("reload-blame", "Reload", "refresh", false).accessibility_label(if busy { "Reload attribution, unavailable while a read is in progress" } else { "Reload attribution" }).disabled(busy).tooltip("Read a fresh snapshot of this file").on_click(cx.listener(|this, _, window, cx| this.load_blame(window, cx)))))
            .child(div().flex_shrink_0().px_3().py_2().flex().flex_col().gap_1().border_b_1().border_color(rgb(p.border))
                .child(static_text("blame-source-scope", source).text_size(px(11.)))
                .child(static_text("blame-lineage-limit", lineage).text_size(px(10.)).text_color(rgb(p.muted))))
            .children(self.blame.data.as_ref().filter(|data| data.shallow).map(|_| static_text("blame-shallow-boundary", "Shallow repository · attribution and line history stop at the locally available boundary.").flex_shrink_0().px_3().py_2().text_size(px(11.)).text_color(rgb(p.warning))))
            .children(self.blame.error.as_ref().map(|error| static_text("blame-error", format!("Attribution unavailable: {error}")).flex_shrink_0().max_h(px(100.)).overflow_y_scroll().p_3().text_size(px(11.)).text_color(rgb(p.warning))))
            .child(div().id("blame-lines-region").role(Role::ListBox).aria_label("Source lines and commit attribution").tab_stop(true).key_context("GitTurtleList").on_action(cx.listener(|this, _: &gpui_kit::component::input::Copy, _, cx| { this.copy_blame_line(cx); })).track_focus(focus).flex_1().min_h_0().min_w_0()
                .child(if count == 0 { div().id("blame-empty-state").role(Role::Label).aria_label(format!("{empty_title}. {empty_detail}")).size_full().child(empty(empty_title, empty_detail)).into_any_element() } else {
                    uniform_list("blame-lines", count, cx.processor(|this, range: std::ops::Range<usize>, _, cx| range.map(|index| this.render_blame_row(index, cx)).collect::<Vec<_>>())).size_full().track_scroll(&self.blame.scroll).into_any_element()
                }))
            .child(div().flex_shrink_0().max_h(px(260.)).border_t_1().border_color(rgb(p.border)).p_3().flex().flex_col().gap_2()
                .child(static_text("blame-selected-summary", summary).text_size(px(12.)).font_weight(FontWeight::MEDIUM))
                .child(static_text("blame-selected-details", detail).text_size(px(10.)).text_color(rgb(p.muted)))
                .child(div().flex().items_center().gap_2()
                    .child(button("blame-compare-commit", "Compare commit", "", false).accessibility_label(action_label("Compare commit")).disabled(attribution.is_none() || busy).tooltip("Open the line's originating commit; Back restores this attribution and selected line").on_click(cx.listener(|this, _, window, cx| this.open_blame_commit(window, cx))))
                    .child(button("blame-line-history", "History of line", "clock", false).accessibility_label(action_label("History of line")).disabled(attribution.is_none() || busy).on_click(cx.listener(|this, _, window, cx| this.load_line_history(window, cx))))
                    .child(button("copy-blame-line", "Copy line", "copy", false).accessibility_label(if selected.is_some() { "Copy line" } else { "Copy line, unavailable until a line is selected" }).disabled(selected.is_none()).on_click(cx.listener(|this, _, _, cx| { this.copy_blame_line(cx); })))
                    .children(attribution.map(|attribution| { let oid = attribution.oid.clone(); button("copy-blame-commit", short_oid(&oid), "copy", false).tooltip("Copy originating commit hash").on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(oid.clone()))) })))
                .children(matches!(self.blame.pending, Some(Pending::History(_))).then(|| static_text("blame-line-history-loading", "Tracing the selected line…").text_size(px(11.))))
                .children(self.blame.history.as_ref().map(|history| div().flex().flex_col().min_h_0().gap_1()
                    .child(static_text("blame-line-history-limit", if history.truncated { "Latest 100 line changes · limit reached. First-parent lineage; copies and other merge parents are excluded." } else { "Line changes · first-parent lineage; copies and other merge parents are excluded." }).text_size(px(10.)).text_color(rgb(p.muted)))
                    .child(div().id("line-history-results").overflow_y_scroll().max_h(px(130.)).children(history.commits.iter().enumerate().map(|(index, commit)| { let oid = commit.oid.clone();
                        div().py_1().flex().items_center().gap_2().child(button(("copy-line-history-commit", index), short_oid(&oid), "copy", false).tooltip("Copy this line-history commit hash").on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(oid.clone())))).child(static_text(("blame-line-history-subject", index), commit.subject.clone()).flex_1().truncate().text_size(px(11.))).child(static_text(("blame-line-history-author", index), commit.author.clone()).text_size(px(10.)).text_color(rgb(p.muted)))
                    }))))))
            .into_any_element()
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
                    .text_size(px(10.))
                    .text_color(rgb(p.muted))
                    .child((index + 1).to_string()),
            )
            .child(
                div()
                    .w(px(180.))
                    .flex_shrink_0()
                    .truncate()
                    .text_size(px(10.))
                    .text_color(rgb(if line.attribution.is_some() {
                        p.muted
                    } else {
                        p.modified
                    }))
                    .child(label),
            )
            .child(
                div()
                    .id(("blame-source", index))
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(mono())
                    .text_size(px(11.))
                    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                    .child(full),
            )
            .on_click(
                cx.listener(move |this, _, window, cx| this.select_blame_line(index, window, cx)),
            )
            .into_any_element()
    }
}

/// Stable Label nodes expose the same meaningful copy to assistive technology.
/// Plain string children alone do not get an ID in this GPUI version.
fn static_text(id: impl Into<ElementId>, value: impl Into<SharedString>) -> Stateful<Div> {
    let value = value.into();
    div()
        .id(id)
        .role(Role::Label)
        .aria_label(value.clone())
        .child(value)
}

/// Cap text shaping for unusually long source lines. Exact source stays in the
/// bounded result and is used by Copy, independently of the displayed excerpt.
fn display_excerpt(source: &str, limit: usize) -> String {
    match source.char_indices().nth(limit) {
        Some((boundary, _)) => format!("{}…", &source[..boundary]),
        None => source.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Pending, State};
    use gitturtle_core::BlameTarget;
    use std::path::Path;
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
}
