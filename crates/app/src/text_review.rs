//! Bounded, presentation-only review of a captured text comparison. Every option
//! is rebuilt from the original snapshot on the replaceable read worker.
use crate::{
    AppMenu, AppPage, GitTurtle, TextMode, WorkspaceMode,
    appearance::{self, palette},
    split_diff::{self, Row, SplitPresentation},
    text::PatchPresentation,
    views::compact_control_name,
    worker::{Content, Job},
};
use anyhow::{Result, ensure};
use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Pixels, Styled, Window,
    component::{
        Icon, Sizable,
        button::{Button, ButtonVariants},
        menu::PopupMenuItem,
    },
    div, percentage,
    prelude::FluentBuilder,
    rgb,
};
use std::{ops::Range, sync::Arc};

const CONTEXT_STEPS: [usize; 4] = [3, 12, 48, 192];
const MAX_REVIEW_BYTES: usize = 4 * 1024 * 1024;
const MAX_REVIEW_ROWS: usize = 100_000;
const MAX_REVIEW_HUNKS: usize = 4096;
/// The collapsed Options button's tooltip: the explanations of the options
/// it holds.
pub(crate) const OPTIONS_TOOLTIP: &str = "Hide whitespace-only changes, expand unchanged \
     context (3, 12, 48, then 192 lines) or reset to the original diff. Source tabs keep exact \
     content; partial staging requires the original diff.";
/// The caption while the review shows Git's own diff. Like the captions of
/// the review's other states, it says what the review shows, so where it
/// closes the Options menu below a separator it reads as state rather than
/// as the title of an empty group, as "Original Git diff" did.
pub(crate) const ORIGINAL_CAPTION: &str = "Showing the original Git diff";
/// The caption while the review's own read failed, as past its limits, and
/// its error replaces the diff: the content area shows no diff, original or
/// reviewed, so the caption says the review is unavailable; the body gives
/// the reason, and Reset review beside it brings the diff back. It once said
/// [`ORIGINAL_CAPTION`] beside the error (QA-101 O1). Any other error keeps
/// the caption the options give.
pub(crate) const UNAVAILABLE_CAPTION: &str = "Review unavailable";

/// The review bar's caption for `options`, or for a read whose error
/// replaces the diff (`unavailable`), and whether it reads in the colour of
/// a modified review rather than muted: it names what the content area
/// shows.
fn review_caption(options: Options, unavailable: bool) -> (&'static str, bool) {
    if unavailable {
        (UNAVAILABLE_CAPTION, true)
    } else if options.hide_whitespace {
        ("Review filter active · exact source retained", true)
    } else if options != Options::default() {
        ("Expanded context · partial staging unavailable", true)
    } else {
        (ORIGINAL_CAPTION, false)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub hide_whitespace: bool,
    pub context: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            hide_whitespace: false,
            context: 3,
        }
    }
}

#[derive(Clone, Default)]
pub struct State {
    pub options: Options,
    pending: Option<Options>,
    original: Option<Arc<Content>>,
    /// The error the review's own read failed with, which the bar names
    /// [`UNAVAILABLE_CAPTION`] while the window shows it
    /// ([`GitTurtle::review_unavailable`]).
    failed: Option<String>,
}

/// Called on the read worker. No Git command, filesystem read, mutation, or
/// network access is needed to adjust a captured comparison's presentation.
pub fn prepare(
    content: &Arc<Content>,
    options: Options,
    checkpoint: impl Fn() -> Result<()>,
) -> Result<Arc<Content>> {
    checkpoint()?;
    if options == Options::default() {
        return Ok(Arc::clone(content));
    }
    ensure!(
        CONTEXT_STEPS.contains(&options.context),
        "Choose a supported context size."
    );
    let Content::Text {
        diagrams,
        markdown,
        patch,
        old,
        new,
        presentation,
        lfs_missing,
        lfs_failed,
        ..
    } = content.as_ref()
    else {
        anyhow::bail!("Text review options are unavailable for this preview.");
    };
    ensure!(
        patch.len().max(old.len()).max(new.len()) <= MAX_REVIEW_BYTES,
        "This comparison exceeds the text review limit. Use the original diff or source tabs."
    );
    let sources = [
        old.split_inclusive('\n').collect::<Vec<_>>(),
        new.split_inclusive('\n').collect::<Vec<_>>(),
    ];
    ensure!(
        sources[0].len() + sources[1].len() <= MAX_REVIEW_ROWS,
        "This file exceeds the 100,000-line review limit. Use the original diff or source tabs."
    );
    checkpoint()?;
    let mut rows = split_diff::align_rows(&sources, presentation);
    if options.hide_whitespace {
        for (i, row) in rows.iter_mut().enumerate() {
            if i % 256 == 0 {
                checkpoint()?;
            }
            if row.changed && whitespace_only(row, &sources) {
                row.changed = false;
            }
        }
    }
    let patch = review_patch(patch, &sources, &rows, options.context, &checkpoint)?;
    checkpoint()?;
    let presentation = Arc::new(PatchPresentation::prepare(&patch));
    checkpoint()?;
    // Full source rows remain literal and aligned; suppressed changes have no
    // diff highlight. Before/After and split copy retain their original bytes.
    let split = Arc::new(SplitPresentation::from_rows(&sources, &rows));
    checkpoint()?;
    Ok(Arc::new(Content::Text {
        diagrams: diagrams.clone(),
        markdown: markdown.clone(),
        patch, old: old.clone(), new: new.clone(), presentation, split,
        partial: None,
        partial_unavailable: Some("Review options are active. Partial staging is unavailable: reset to the original diff to select exact Git lines or hunks. Whole-file actions include every change.".into()),
        lfs_missing: *lfs_missing,
        lfs_failed: *lfs_failed,
    }))
}

fn whitespace_only(row: &Row, sources: &[Vec<&str>; 2]) -> bool {
    let old = row
        .old
        .and_then(|i| sources[0].get(i))
        .copied()
        .unwrap_or("");
    let new = row
        .new
        .and_then(|i| sources[1].get(i))
        .copied()
        .unwrap_or("");
    old.chars()
        .filter(|c| !c.is_whitespace())
        .eq(new.chars().filter(|c| !c.is_whitespace()))
}

fn review_patch(
    original: &str,
    sources: &[Vec<&str>; 2],
    rows: &[Row],
    context: usize,
    checkpoint: &impl Fn() -> Result<()>,
) -> Result<String> {
    let mut groups: Vec<Range<usize>> = Vec::new();
    // A hidden insertion/deletion cannot be represented as a two-sided context
    // line. It divides review hunks instead, keeping both gutters honest.
    let mut segment = 0;
    while segment < rows.len() {
        let end = rows[segment..]
            .iter()
            .position(|row| !row.changed && (row.old.is_none() || row.new.is_none()))
            .map_or(rows.len(), |i| segment + i);
        let group_start = groups.len();
        for (i, row) in rows.iter().enumerate().take(end).skip(segment) {
            if i % 256 == 0 {
                checkpoint()?;
            }
            if !row.changed {
                continue;
            }
            let span = i.saturating_sub(context).max(segment)..(i + context + 1).min(end);
            if groups.len() > group_start && groups.last().unwrap().end >= span.start {
                groups.last_mut().unwrap().end = span.end;
            } else {
                groups.push(span);
            }
            ensure!(
                groups.len() <= MAX_REVIEW_HUNKS,
                "Too many separate review hunks. Reset the review options to inspect the original diff."
            );
        }
        segment = end + 1;
    }
    if groups.is_empty() {
        return Ok(String::new());
    }
    let mut cursors = Vec::with_capacity(rows.len() + 1);
    let mut cursor = [0, 0];
    for row in rows {
        cursors.push(cursor);
        if let Some(old) = row.old {
            cursor[0] = old + 1;
        }
        if let Some(new) = row.new {
            cursor[1] = new + 1;
        }
    }
    cursors.push(cursor);
    let header_length = original
        .split_inclusive('\n')
        .take_while(|line| !line.starts_with("@@ "))
        .map(str::len)
        .sum();
    let mut result = original[..header_length].to_owned();
    for group in groups {
        checkpoint()?;
        let counts = [
            rows[group.clone()]
                .iter()
                .filter(|r| r.old.is_some())
                .count(),
            rows[group.clone()]
                .iter()
                .filter(|r| r.new.is_some())
                .count(),
        ];
        let starts = [
            cursors[group.start][0] + usize::from(counts[0] > 0),
            cursors[group.start][1] + usize::from(counts[1] > 0),
        ];
        result.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            starts[0], counts[0], starts[1], counts[1]
        ));
        let mut i = group.start;
        while i < group.end {
            if i % 256 == 0 {
                checkpoint()?;
            }
            if rows[i].changed {
                let end = (i..group.end)
                    .find(|&j| !rows[j].changed)
                    .unwrap_or(group.end);
                for (side, prefix) in [(0, '-'), (1, '+')] {
                    for row in &rows[i..end] {
                        if let Some(number) = if side == 0 { row.old } else { row.new } {
                            push_line(&mut result, prefix, sources[side][number])?;
                        }
                    }
                }
                i = end;
            } else {
                if let Some(number) = rows[i].new {
                    push_line(&mut result, ' ', sources[1][number])?;
                }
                i += 1;
            }
        }
    }
    Ok(result)
}

fn push_line(output: &mut String, prefix: char, line: &str) -> Result<()> {
    ensure!(
        output.len() + line.len() + 35 <= MAX_REVIEW_BYTES,
        "Expanded context exceeds the 4 MiB review limit. Choose less context or use the source tabs."
    );
    output.push(prefix);
    output.push_str(line);
    if !line.ends_with('\n') {
        output.push_str("\n\\ No newline at end of file\n");
    }
    Ok(())
}

impl GitTurtle {
    pub(super) fn set_text_review(
        &mut self,
        options: Options,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.loading.is_some() || self.operation_busy.is_some() {
            return;
        }
        let Some(content) = self
            .content
            .as_ref()
            .filter(|content| matches!(content.as_ref(), Content::Text { .. }))
        else {
            return;
        };
        let original = self
            .review
            .original
            .get_or_insert_with(|| Arc::clone(content))
            .clone();
        self.review.pending = Some(options);
        self.review.failed = None;
        if let Some(view) = &self.patch_view {
            view.update(cx, |view, cx| view.suspend_partial(cx));
        }
        self.request(
            Job::ReviewText {
                content: original,
                options,
            },
            "Preparing text review…",
            window,
            cx,
        );
    }

    pub(super) fn receive_text_review(
        &mut self,
        content: Arc<Content>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(options) = self.review.pending.take() else {
            return;
        };
        self.review.options = options;
        if let Content::Text {
            patch,
            presentation,
            partial,
            split,
            ..
        } = content.as_ref()
        {
            if let Some(editor) = &self.patch_editor {
                crate::text::refresh_editor(
                    editor,
                    patch,
                    self.patch_decoration
                        .as_ref()
                        .map(|collection| (collection, presentation.as_ref())),
                    window,
                    cx,
                );
            }
            if let Some(view) = &self.patch_view {
                view.update(cx, |view, cx| {
                    view.refresh(presentation, partial.clone(), cx)
                });
            }
            if let Some(view) = &self.split_view {
                view.update(cx, |view, cx| view.refresh(Arc::clone(split), window, cx));
            }
        }
        self.content = Some(content);
        if self.page == AppPage::Repository
            && matches!(self.mode, WorkspaceMode::Compare | WorkspaceMode::Working)
        {
            self.ensure_editor(window, cx);
        }
        cx.notify();
    }

    /// Records the window's error as the failure of the review's own read,
    /// as a review past its limits fails, for the bar to name.
    pub(super) fn text_review_failed(&mut self) {
        self.review.failed = self.error.clone();
    }

    /// Whether the error the window shows is the review's own failure: the
    /// bar then says the review is unavailable. Any other error the window
    /// shows keeps the caption the options give, though it replaces the
    /// diff too; a refusal never sets it ([`Self::refuse_in_tab_strip`]).
    pub(super) fn review_unavailable(&self) -> bool {
        self.error.is_some() && self.error == self.review.failed
    }

    /// Ends a text review whose preparation will not land while the
    /// comparison it was for stays shown or kept for the return: a read
    /// that fails, as past the review's limits, or is dropped unanswered,
    /// and one abandoned as Blame opens over the comparison, as Quick Open
    /// inspects a tracked file, or as another repository tab is shown, each
    /// of which keeps the comparison for its return (file history cannot
    /// open from Working Changes, and a Compare diff has no partial
    /// actions). Its options are dropped and the shown diff's partial
    /// actions, suspended while it was prepared, act again, since that diff
    /// is the one they act on. Left suspended, they stayed refused, a hunk's
    /// Stage or Unstage disabled and a click on a line selecting nothing,
    /// until another file was chosen. Back drops the comparison instead
    /// (`clear_preview`).
    pub(super) fn release_text_review(&mut self, cx: &mut Context<Self>) {
        if self.review.pending.take().is_none() {
            return;
        }
        if let (
            Some(view),
            Some(Content::Text {
                presentation,
                partial,
                ..
            }),
        ) = (&self.patch_view, self.content.as_deref())
        {
            view.update(cx, |view, cx| {
                view.refresh(presentation, partial.clone(), cx)
            });
        }
    }

    /// Takes the partial actions of `view`, the unified diff built for the
    /// shown content, for as long as it lives, however the window's reads
    /// move on: a press acts on the diff it was made on only while the
    /// window shows that diff for the selected Working row
    /// ([`Self::shows_partial`]), and on none other. Bound instead to the
    /// window's read as the view was built, the actions had to be bound
    /// again at each read that left the diff shown, Blame's, an
    /// inspection's or a text review's, and any read that was not left
    /// Stage looking enabled while a press started nothing. A press on a
    /// diff no longer shown leaves its actions suspended
    /// (`DiffView::apply_partial`), so they read disabled; one refused
    /// because a write already runs, another tab's, gives them back with
    /// the busy look, so the next press after that write acts.
    pub(super) fn bind_text_partial(
        &mut self,
        view: &gpui_kit::Entity<crate::diff_view::DiffView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let repository = self.path.clone();
        self.partial_subscription =
            Some(
                cx.subscribe_in(view, window, move |this, view, event, window, cx| {
                    let crate::diff_view::DiffViewEvent::ApplyPartial { diff, selection } = event;
                    if this.path != repository || !this.shows_partial(view, diff) {
                        // No write runs for it, so no read is to land in
                        // the view: left waiting, it kept its stale text
                        // through the next Refresh, which landed there.
                        view.update(cx, |view, cx| view.end_write(cx));
                        return;
                    }
                    // A write already runs, another tab's, as this one's
                    // clears the diff it acts on: the press starts nothing,
                    // so its controls return at once rather than stay
                    // suspended until another file is chosen.
                    if this.operation_busy.is_some() {
                        if let Some(Content::Text {
                            presentation,
                            partial,
                            ..
                        }) = this.content.as_deref()
                        {
                            view.update(cx, |view, cx| {
                                view.refresh(presentation, partial.clone(), cx)
                            });
                        }
                        this.show_busy_look(cx);
                        return;
                    }
                    this.write(
                        gitturtle_core::WriteCommand::ApplyPartial {
                            diff: Arc::clone(diff),
                            selection: selection.clone(),
                        },
                        if diff.area == gitturtle_core::ChangeArea::Unstaged {
                            "Staging selected changes…"
                        } else {
                            "Unstaging selected changes…"
                        },
                        window,
                        cx,
                    );
                }),
            );
    }

    /// Whether `view` is the unified diff shown in Working Changes and
    /// `diff` the partial diff of the content it shows, read for the
    /// selected row's path and side: the one diff a partial action acts on.
    fn shows_partial(
        &self,
        view: &gpui_kit::Entity<crate::diff_view::DiffView>,
        diff: &Arc<gitturtle_core::PartialDiff>,
    ) -> bool {
        let shown = matches!(
            self.content.as_deref(),
            Some(Content::Text { partial: Some(partial), .. }) if Arc::ptr_eq(&partial.diff, diff)
        );
        let selected = self.working_selected.is_some_and(|(index, area)| {
            area == diff.area
                && self
                    .work_status
                    .as_ref()
                    .and_then(|status| status.entries.get(index))
                    .is_some_and(|entry| entry.path == diff.path)
        });
        self.page == AppPage::Repository
            && self.mode == WorkspaceMode::Working
            && self.patch_view.as_ref() == Some(view)
            && shown
            && selected
    }

    pub(super) fn navigate_text_change(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != AppPage::Repository || self.blame.is_visible() {
            return;
        }
        match self.text_mode {
            TextMode::Split => {
                if let Some(view) = &self.split_view {
                    view.update(cx, |view, cx| view.navigate_change(forward, cx));
                }
            }
            TextMode::Unified => {
                if let Some(view) = &self.patch_view {
                    view.update(cx, |view, cx| view.navigate_change(forward, cx));
                }
            }
            _ => {
                self.text_mode = TextMode::Unified;
                self.ensure_editor(window, cx);
                self.navigate_text_change(forward, window, cx);
            }
        }
        cx.notify();
    }

    /// `column` is the width the row spans. Narrower than the row needs at
    /// the interface size, the change buttons keep only their arrows and the
    /// other options move into an Options menu, so the row stays one line
    /// and the diff keeps its height. The caption stays beside them only
    /// where its whole text fits; narrower, it closes the Options menu
    /// instead, so the row never shows a cut caption such as "Origin…".
    pub(super) fn render_text_review(
        &self,
        column: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = palette(cx);
        let options = self.review.options;
        // A write refuses these at once, and they look it only once the
        // write looks busy (`workspace::BusyLook`); a preview still loading
        // leaves them unavailable at once.
        let busy = self.loading.is_some() || self.operation_busy.is_some();
        let looks_busy = self.loading.is_some() || self.operation_shown().is_some();
        let has_changes = matches!(self.content.as_deref(), Some(Content::Text { presentation, .. }) if !presentation.change_rows.is_empty());
        let modified = options != Options::default();
        let unmodified = !modified && self.error.is_none();
        let reset_disabled = busy || unmodified;
        let row = div()
            .debug_selector(|| "text-review-options".into())
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .px_3()
            .py_1()
            .bg(rgb(colors.panel))
            .border_b_1()
            .border_color(rgb(colors.border));
        // The caption takes a small button's text inset, which the control
        // before it gives back: beside that control nothing moves, and on a
        // line of its own the caption starts where the buttons' labels do.
        // While the review's own failure replaces the diff the content
        // area shows none (`render_preview`), so neither does the caption.
        let (caption_text, accented) = review_caption(options, self.review_unavailable());
        let caption = |text: AnyElement| {
            div()
                .debug_selector(move || format!("text-review-caption/{caption_text}"))
                .pl_2()
                .text_size(crate::appearance::ui_text(11.))
                .text_color(rgb(if accented { colors.hunk } else { colors.muted }))
                .child(text)
        };
        let previous = Button::new("previous-text-change")
            .debug_selector(|| "previous-text-change".into())
            .small()
            .ghost()
            .tooltip(format!(
                "Previous change · {}",
                crate::shortcuts::label(crate::shortcuts::ShortcutId::PreviousChange)
            ))
            .map(|button| {
                crate::workspace::refusing(
                    button,
                    || "previous-text-change".into(),
                    busy || !has_changes,
                    looks_busy || !has_changes,
                )
            })
            .on_click(cx.listener(|this, _, window, cx| {
                if this.loading.is_none() && this.operation_busy.is_none() {
                    this.navigate_text_change(false, window, cx)
                }
            }));
        let next = Button::new("next-text-change")
            .debug_selector(|| "next-text-change".into())
            .small()
            .ghost()
            .tooltip(format!(
                "Next change · {}",
                crate::shortcuts::label(crate::shortcuts::ShortcutId::NextChange)
            ))
            .map(|button| {
                crate::workspace::refusing(
                    button,
                    || "next-text-change".into(),
                    busy || !has_changes,
                    looks_busy || !has_changes,
                )
            })
            .on_click(cx.listener(|this, _, window, cx| {
                if this.loading.is_none() && this.operation_busy.is_none() {
                    this.navigate_text_change(true, window, cx)
                }
            }));
        if column < options_row_width(window) {
            // The app's arrow, turned up and down.
            let arrow = |turn: f32| {
                Icon::default()
                    .path("icons/arrow-left.svg")
                    .size(appearance::ui_size(14.))
                    .rotate(percentage(turn))
            };
            let owner = cx.entity().downgrade();
            let caption_fits = column >= collapsed_caption_width(caption_text, window);
            let menu = compact_control_name(
                Button::new("text-review-menu")
                    .debug_selector(|| "text-review-menu".into())
                    .small()
                    .ghost()
                    .label("Options")
                    .dropdown_caret(true)
                    .mr_neg_2(),
                "text-review-menu",
                "Review options",
            )
            .tooltip(OPTIONS_TOOLTIP)
            .app_menu(move |menu, _, _| {
                // Built when opened; each item acts on the review as it
                // is when chosen, as the buttons of the full row do. A
                // caption with no room in the row closes the menu, below a
                // separator, as it follows the options in the row; it is a
                // label, which the keys pass over.
                options_menu(options, busy, reset_disabled)
                    .into_iter()
                    .fold(menu, |menu, item| {
                        let owner = owner.clone();
                        menu.item(
                            PopupMenuItem::new(item.label)
                                .when_some(item.name, PopupMenuItem::accessibility_label)
                                .when_some(item.checked, PopupMenuItem::checked)
                                .disabled(item.disabled)
                                .on_click(move |_, window, cx| {
                                    let _ = owner.update(cx, |this, cx| {
                                        if this.page == AppPage::Repository
                                            && let Some(options) =
                                                (item.choose)(this.review.options)
                                        {
                                            this.set_text_review(options, window, cx);
                                        }
                                    });
                                }),
                        )
                    })
                    .when(!caption_fits, |menu| menu.separator().label(caption_text))
            });
            row.child(compact_control_name(
                previous.icon(arrow(0.25)),
                "previous-text-change",
                "Previous change",
            ))
            .child(compact_control_name(
                next.icon(arrow(0.75)),
                "next-text-change",
                "Next change",
            ))
            .child(menu)
            .when(caption_fits, |row| {
                row.child(
                    caption(
                        div()
                            .debug_selector(|| "text-review-caption".into())
                            .whitespace_nowrap()
                            .child(caption_text)
                            .into_any_element(),
                    )
                    .flex_1()
                    .min_w_0(),
                )
            })
            .into_any_element()
        } else {
            row.child(previous.label("Previous change"))
                .child(next.label("Next change"))
                .child(Button::new("hide-whitespace-changes").debug_selector(|| "hide-whitespace-changes".into()).small().ghost().label(if options.hide_whitespace { "Whitespace hidden" } else { "Hide whitespace" })
                    .toggled(options.hide_whitespace).map(|hide| crate::workspace::refusing(hide, || "hide-whitespace-changes".into(), busy, looks_busy))
                    .tooltip("Hide whitespace-only changes, including indentation and line endings. Source tabs keep exact content; partial staging requires the original diff.")
                    .on_click(cx.listener(move |this, _, window, cx| this.set_text_review(Options { hide_whitespace: !options.hide_whitespace, ..options }, window, cx))))
                .child(context_button(options, busy, looks_busy)
                    .on_click(cx.listener(move |this, _, window, cx| { if let Some(options) = more_context(options) { this.set_text_review(options, window, cx); } })))
                .child(Button::new("reset-text-review").debug_selector(|| "reset-text-review".into()).small().ghost().label("Reset review")
                    .map(|reset| crate::workspace::refusing(reset, || "reset-text-review".into(), reset_disabled, looks_busy || unmodified)).mr_neg_2()
                    .on_click(cx.listener(|this, _, window, cx| this.set_text_review(Options::default(), window, cx))))
                .child(caption(div().debug_selector(|| "text-review-caption".into()).child(caption_text).into_any_element()))
                .into_any_element()
        }
    }
}

/// One item of the collapsed review options' menu.
struct MenuItem {
    label: String,
    /// The accessible name, where it says more than the label: the full
    /// row's button's name.
    name: Option<String>,
    /// `Some` for an item that toggles, announced as a check menu item.
    checked: Option<bool>,
    disabled: bool,
    /// The options it chooses, from those current when it is chosen.
    choose: fn(Options) -> Option<Options>,
}

/// `options` with the next context step, or `None` at the last.
fn more_context(options: Options) -> Option<Options> {
    CONTEXT_STEPS
        .iter()
        .copied()
        .find(|&n| n > options.context)
        .map(|context| Options { context, ..options })
}

/// The context control's label, led by what it does and showing the context
/// each hunk has now, in the full row and the Options menu alike.
fn context_label(context: usize) -> String {
    format!(
        "More context ({})",
        crate::counted(context, "line", "lines")
    )
}

/// The context control's accessible name, its label in words, for the full
/// row's button and the Options menu's item alike.
fn context_name(context: usize) -> String {
    format!(
        "More context, {} shown",
        crate::counted(context, "line", "lines")
    )
}

/// The full row's context button. Its accessible name says what it does and
/// the context shown now, and its tooltip the step it takes. `busy` refuses
/// it, which it looks only where `looks_busy` (`workspace::refusing`).
pub(crate) fn context_button(options: Options, busy: bool, looks_busy: bool) -> Button {
    let context = options.context;
    let next = more_context(options);
    Button::new("expand-text-context")
        .debug_selector(|| "expand-text-context".into())
        .small()
        .ghost()
        .label(context_label(context))
        .accessibility_label(context_name(context))
        .map(|button| {
            crate::workspace::refusing(
                button,
                || "expand-text-context".into(),
                busy || next.is_none(),
                looks_busy || next.is_none(),
            )
        })
        .tooltip(match next {
            Some(next) => format!(
                "Show {} unchanged lines around each hunk instead of {context}, in steps of 3, \
                 12, 48, then 192. Split and source tabs contain the full bounded preview.",
                next.context
            ),
            None => format!(
                "Each hunk shows {context} unchanged lines around it, the most this review \
                 shows. Split and source tabs contain the full bounded preview."
            ),
        })
}

/// The Options menu, as the full row's Hide whitespace, More context and
/// Reset review buttons: whitespace a checked item, context showing its value,
/// named as its button is and disabled at its last step, reset disabled as its
/// button is.
fn options_menu(options: Options, busy: bool, reset_disabled: bool) -> [MenuItem; 3] {
    [
        MenuItem {
            label: "Hide whitespace".into(),
            name: None,
            checked: Some(options.hide_whitespace),
            disabled: busy,
            choose: |options| {
                Some(Options {
                    hide_whitespace: !options.hide_whitespace,
                    ..options
                })
            },
        },
        MenuItem {
            label: context_label(options.context),
            name: Some(context_name(options.context)),
            checked: None,
            disabled: busy || more_context(options).is_none(),
            choose: more_context,
        },
        MenuItem {
            label: "Reset review".into(),
            name: None,
            checked: None,
            disabled: reset_disabled,
            choose: |_| Some(Options::default()),
        },
    ]
}

/// The width the review options take on one row as they first appear, with
/// [`ORIGINAL_CAPTION`]: `px_3` on either side, six items `gap_1` apart, and
/// five small buttons with `px_2` on either side of a `text_sm` label. Fixed
/// labels keep the layout from switching when an option is chosen; a longer
/// caption wraps to a line of its own.
pub(crate) fn options_row_width(window: &Window) -> Pixels {
    let rem = window.rem_size();
    rem * (2. * 0.75 + 5. * 0.25 + 5. * 2. * 0.5)
        + crate::views::cached_width(
            &OPTIONS_ROW,
            &crate::views::LineKey::current(window),
            || {
                let label = rem * 0.875;
                let context = context_label(Options::default().context);
                crate::views::line_widths(
                    &[
                        ("Previous change", label),
                        ("Next change", label),
                        ("Hide whitespace", label),
                        (&context, label),
                        ("Reset review", label),
                        (ORIGINAL_CAPTION, appearance::ui_text(11.)),
                    ],
                    window,
                )
            },
        )
}

/// The column the collapsed review options need to show `caption` whole
/// beside them: `px_3` on either side, four items `gap_1` apart, the two
/// arrow buttons (`size_6`), the Options button (`px_2` on either side of
/// its `text_sm` label, `gap_1` and the small caret, `size_3p5`), and the
/// caption's 11 pt text. The Options button's `mr_neg_2` gives back the
/// caption's `pl_2`, so the caption's text starts one gap after the button,
/// as it does after Reset review in the full row. The fixed part is rounded
/// up to a whole pixel, as the texts are, so a caption shown is never cut.
pub(crate) fn collapsed_caption_width(caption: &str, window: &Window) -> Pixels {
    let rem = window.rem_size();
    (rem * (2. * 0.75 + 3. * 0.25 + 2. * 1.5 + 2. * 0.5 + 0.25 + 0.875)).ceil()
        + crate::views::cached_text_width(&COLLAPSED_CAPTION, caption, window, || {
            crate::views::line_widths(
                &[
                    ("Options", rem * 0.875),
                    (caption, appearance::ui_text(11.)),
                ],
                window,
            )
        })
}

thread_local! {
    /// The review options' laid-out labels, by font and size.
    pub(crate) static OPTIONS_ROW: std::cell::RefCell<Option<(crate::views::LineKey, Pixels)>> =
        const { std::cell::RefCell::new(None) };
    /// The collapsed row's Options label and the caption it shows, laid out.
    static COLLAPSED_CAPTION: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
}

/// A navigation action preserves selection/copy and horizontal scroll. Repeated
/// actions advance from the last target, avoiding deferred-layout bounce.
pub(crate) fn next_change(
    rows: &[usize],
    current: Option<usize>,
    visible: usize,
    forward: bool,
) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    if let Some(current) = current {
        Some(if forward {
            (current + 1) % rows.len()
        } else {
            (current + rows.len() - 1) % rows.len()
        })
    } else if forward {
        Some(rows.iter().position(|&row| row >= visible).unwrap_or(0))
    } else {
        Some(
            rows.iter()
                .rposition(|&row| row < visible)
                .unwrap_or(rows.len() - 1),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpui;

    /// The collapsed Options menu carries each full-row button's state and
    /// chooses what that button would.
    #[test]
    fn collapsed_options_menu_keeps_each_buttons_state() {
        let states = |menu: [MenuItem; 3]| {
            menu.map(|item| (item.label, item.name, item.checked, item.disabled))
        };
        let original = Options::default();
        assert_eq!(
            states(options_menu(original, false, true)),
            [
                ("Hide whitespace".into(), None, Some(false), false),
                (
                    "More context (3 lines)".into(),
                    Some("More context, 3 lines shown".into()),
                    None,
                    false
                ),
                ("Reset review".into(), None, None, true),
            ]
        );
        let hidden = Options {
            hide_whitespace: true,
            context: 3,
        };
        let [whitespace, context, reset] = options_menu(hidden, false, false);
        assert!(whitespace.checked == Some(true) && !reset.disabled);
        assert_eq!((whitespace.choose)(hidden), Some(original));
        assert_eq!(
            (context.choose)(hidden),
            Some(Options {
                context: 12,
                ..hidden
            })
        );
        assert_eq!((reset.choose)(hidden), Some(original));
        let widest = Options {
            hide_whitespace: false,
            context: 192,
        };
        let [_, context, _] = options_menu(widest, false, false);
        assert_eq!(
            (
                context.label.as_str(),
                context.name.as_deref(),
                context.disabled
            ),
            (
                "More context (192 lines)",
                Some("More context, 192 lines shown"),
                true
            )
        );
        assert_eq!((context.choose)(widest), None);
        assert!(
            options_menu(hidden, true, true)
                .iter()
                .all(|item| item.disabled)
        );
    }
    /// Each caption names what the content area shows, and every one but
    /// the original diff's reads in the modified review's colour: a read
    /// whose error replaces the diff says the review is unavailable, under
    /// any options.
    #[test]
    fn captions_name_what_the_content_area_shows() {
        let hidden = Options {
            hide_whitespace: true,
            context: 3,
        };
        let expanded = Options {
            hide_whitespace: false,
            context: 12,
        };
        assert_eq!(
            review_caption(Options::default(), false),
            (ORIGINAL_CAPTION, false)
        );
        assert_eq!(
            review_caption(hidden, false),
            ("Review filter active · exact source retained", true)
        );
        assert_eq!(
            review_caption(expanded, false),
            ("Expanded context · partial staging unavailable", true)
        );
        for options in [Options::default(), hidden, expanded] {
            assert_eq!(
                review_caption(options, true),
                (UNAVAILABLE_CAPTION, true),
                "{options:?}"
            );
        }
    }
    #[test]
    fn review_removes_actionable_partial_ids_and_reset_retains_the_exact_snapshot() {
        use gitturtle_core::{ChangeArea, GitRepository, TextPreview};
        use std::{fs, process::Command};
        // In this process's test root, named by its process-wide sequence.
        let fixture = crate::preferences::test_directories::TestDirectory::new("text-review");
        let git = |args: &[&str]| {
            let mut command = Command::new("git");
            command
                .arg("-C")
                .arg(fixture.path())
                .args([
                    "-c",
                    "user.name=Review Fixture",
                    "-c",
                    "user.email=review@example.invalid",
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
            output.stdout
        };
        git(&["init", "--quiet"]);
        fs::write(fixture.path().join("file.txt"), "a = 1\nold\n").unwrap();
        git(&["add", "--", "file.txt"]);
        git(&["commit", "--quiet", "-m", "baseline"]);
        fs::write(fixture.path().join("file.txt"), "a=1\nnew\n").unwrap();
        let repo = GitRepository::open(fixture.path()).unwrap();
        let status = repo.status().unwrap();
        let preview = repo
            .worktree_preview(&status.entries[0], ChangeArea::Unstaged)
            .unwrap();
        let TextPreview::Patch(patch) = preview.preview else {
            panic!()
        };
        let old = String::from_utf8(preview.old).unwrap();
        let new = String::from_utf8(preview.new).unwrap();
        let presentation = Arc::new(PatchPresentation::prepare(&patch));
        let split = Arc::new(SplitPresentation::prepare(&old, &new, &presentation));
        let diff = Arc::new(preview.partial.unwrap());
        let partial = Arc::new(
            crate::partial_view::prepare(&patch, &presentation, Arc::clone(&diff)).unwrap(),
        );
        let original = Arc::new(Content::Text {
            diagrams: None,
            markdown: None,
            patch,
            old,
            new,
            presentation,
            split,
            partial: Some(partial),
            partial_unavailable: None,
            lfs_missing: [false; 2],
            lfs_failed: [None; 2],
        });
        let before_index = fs::read(fixture.path().join(".git/index")).unwrap();
        for options in [
            Options {
                hide_whitespace: true,
                context: 3,
            },
            Options {
                hide_whitespace: false,
                context: 12,
            },
        ] {
            let reviewed = prepare(&original, options, || Ok(())).unwrap();
            assert!(matches!(
                reviewed.as_ref(),
                Content::Text { partial: None, .. }
            ));
        }
        let reset = prepare(&original, Options::default(), || Ok(())).unwrap();
        let Content::Text {
            partial: Some(partial),
            ..
        } = reset.as_ref()
        else {
            panic!()
        };
        assert!(Arc::ptr_eq(&partial.diff, &diff));
        assert_eq!(
            fs::read(fixture.path().join(".git/index")).unwrap(),
            before_index
        );
        assert_eq!(
            fs::read_to_string(fixture.path().join("file.txt")).unwrap(),
            "a=1\nnew\n"
        );
    }
    fn content(old: &str, new: &str, patch: &str) -> Arc<Content> {
        let presentation = Arc::new(PatchPresentation::prepare(patch));
        let split = Arc::new(SplitPresentation::prepare(old, new, &presentation));
        Arc::new(Content::Text {
            diagrams: None,
            markdown: None,
            old: old.into(),
            new: new.into(),
            patch: patch.into(),
            presentation,
            split,
            partial: None,
            partial_unavailable: None,
            lfs_missing: [false; 2],
            lfs_failed: [None; 2],
        })
    }
    fn patch(content: &Content) -> &str {
        let Content::Text { patch, .. } = content else {
            panic!("text expected")
        };
        patch
    }
    /// Review options redraw the comparison: what the reading found of its
    /// Git LFS objects still decides what is called missing and offered.
    #[test]
    fn review_options_keep_which_lfs_objects_are_missing() {
        let mut original = content("a\n", "b\n", "--- a/f\n+++ b/f\n@@ -1 +1 @@\n-a\n+b\n");
        let Content::Text { lfs_missing, .. } = Arc::get_mut(&mut original).unwrap() else {
            panic!("text expected")
        };
        *lfs_missing = [true, false];
        let filtered = prepare(
            &original,
            Options {
                hide_whitespace: true,
                context: 3,
            },
            || Ok(()),
        )
        .unwrap();
        let Content::Text { lfs_missing, .. } = filtered.as_ref() else {
            panic!("text expected")
        };
        assert_eq!(*lfs_missing, [true, false]);
    }

    #[test]
    fn whitespace_filter_preserves_original_snapshot_and_exact_sources() {
        let old = "a = 1\r\nold 🐢\n\nend";
        let new = "a=1\nnew 🐢\nend";
        let original = content(
            old,
            new,
            "--- a/f\n+++ b/f\n@@ -1,4 +1,3 @@\n-a = 1\r\n-old 🐢\n-\n+a=1\n+new 🐢\n end\n\\ No newline at end of file\n",
        );
        let filtered = prepare(
            &original,
            Options {
                hide_whitespace: true,
                context: 3,
            },
            || Ok(()),
        )
        .unwrap();
        let Content::Text {
            old: actual_old,
            new: actual_new,
            partial,
            partial_unavailable,
            ..
        } = filtered.as_ref()
        else {
            panic!()
        };
        assert_eq!(actual_old, old);
        assert_eq!(actual_new, new);
        assert!(partial.is_none());
        assert!(
            partial_unavailable
                .as_ref()
                .unwrap()
                .contains("Whole-file actions include every change")
        );
        assert!(!patch(&filtered).contains("-a = 1"));
        assert!(patch(&filtered).contains("-old 🐢\n+new 🐢\n"));
        assert!(patch(&original).contains("-a = 1"));
        let reset = prepare(&original, Options::default(), || Ok(())).unwrap();
        assert!(Arc::ptr_eq(&original, &reset));
    }
    #[test]
    fn blank_line_insertions_and_removals_can_be_hidden_without_false_numbers() {
        let original = content(
            "one\n\nold\n",
            "one\nnew\n \n",
            "@@ -1,3 +1,3 @@\n one\n-\n-old\n+new\n+ \n",
        );
        // Pairing is deliberately literal: a mixed replacement remains visible
        // unless its complete row is whitespace-equivalent.
        let filtered = prepare(
            &original,
            Options {
                hide_whitespace: true,
                context: 3,
            },
            || Ok(()),
        )
        .unwrap();
        assert!(patch(&filtered).contains("old"));
        let blank_only = content("one\n\n", "one\n", "@@ -1,2 +1 @@\n one\n-\n");
        let filtered = prepare(
            &blank_only,
            Options {
                hide_whitespace: true,
                context: 3,
            },
            || Ok(()),
        )
        .unwrap();
        assert!(patch(&filtered).is_empty());
    }
    #[test]
    fn expansion_exposes_bounded_surrounding_lines_and_merges_overlapping_hunks() {
        let old = (1..=600).map(|i| format!("line {i}\n")).collect::<String>();
        let new = old
            .replace("line 300\n", "changed 300\n")
            .replace("line 310\n", "changed 310\n");
        let original = content(
            &old,
            &new,
            "@@ -300 +300 @@\n-line 300\n+changed 300\n@@ -310 +310 @@\n-line 310\n+changed 310\n",
        );
        let expanded = prepare(
            &original,
            Options {
                context: 12,
                ..Options::default()
            },
            || Ok(()),
        )
        .unwrap();
        assert_eq!(patch(&expanded).matches("@@ -").count(), 1);
        assert!(patch(&expanded).contains(" line 288\n"));
        assert!(patch(&expanded).contains(" line 322\n"));
        assert!(!patch(&expanded).contains(" line 287\n"));
        assert!(!patch(&expanded).contains(" line 323\n"));
        assert!(patch(&expanded).starts_with("@@ -288,35 +288,35 @@\n"));
    }
    #[test]
    fn crlf_unicode_and_missing_newlines_survive_context_expansion() {
        let original = content(
            "héllo\r\n🐢",
            "你好\r\n🐢",
            "@@ -1,2 +1,2 @@\n-héllo\r\n+你好\r\n 🐢\n\\ No newline at end of file\n",
        );
        let expanded = prepare(
            &original,
            Options {
                context: 12,
                ..Options::default()
            },
            || Ok(()),
        )
        .unwrap();
        assert_eq!(
            patch(&expanded),
            "@@ -1,2 +1,2 @@\n-héllo\r\n+你好\r\n 🐢\n\\ No newline at end of file\n"
        );
    }
    #[test]
    fn empty_sides_keep_valid_zero_count_headers() {
        for (old, new, patch_text, expected) in [
            (
                "",
                "added",
                "@@ -0,0 +1 @@\n+added\n\\ No newline at end of file\n",
                "@@ -0,0 +1,1 @@\n+added\n\\ No newline at end of file\n",
            ),
            (
                "removed",
                "",
                "@@ -1 +0,0 @@\n-removed\n\\ No newline at end of file\n",
                "@@ -1,1 +0,0 @@\n-removed\n\\ No newline at end of file\n",
            ),
        ] {
            let original = content(old, new, patch_text);
            let expanded = prepare(
                &original,
                Options {
                    context: 12,
                    ..Options::default()
                },
                || Ok(()),
            )
            .unwrap();
            assert_eq!(patch(&expanded), expected);
        }
    }
    #[test]
    fn cancelled_and_out_of_budget_review_never_returns_partial_content() {
        let original = content("old\n", "new\n", "@@ -1 +1 @@\n-old\n+new\n");
        let visits = std::cell::Cell::new(0);
        let result = prepare(
            &original,
            Options {
                context: 12,
                ..Options::default()
            },
            || {
                visits.set(visits.get() + 1);
                ensure!(visits.get() < 3, "Cancelled");
                Ok(())
            },
        );
        assert!(result.err().unwrap().to_string().contains("Cancelled"));
        assert!(
            prepare(
                &original,
                Options {
                    context: usize::MAX,
                    ..Options::default()
                },
                || Ok(())
            )
            .is_err()
        );
        let large = content(&"\n".repeat(MAX_REVIEW_ROWS + 1), "", "");
        assert!(
            prepare(
                &large,
                Options {
                    context: 12,
                    ..Options::default()
                },
                || Ok(())
            )
            .err()
            .unwrap()
            .to_string()
            .contains("100,000")
        );
    }
    /// Working Changes showing a modified file's unified diff, whose
    /// partial actions are offered and not suspended.
    async fn working_diff(
        cx: &mut gpui::TestAppContext,
    ) -> (
        tempfile::TempDir,
        gpui::Entity<GitTurtle>,
        &mut gpui::VisualTestContext,
    ) {
        working_diff_from(cx, &crate::views::tests::history_script(40, 3)).await
    }

    /// [`working_diff`] in a repository `script` imports.
    async fn working_diff_from<'a>(
        cx: &'a mut gpui::TestAppContext,
        script: &str,
    ) -> (
        tempfile::TempDir,
        gpui::Entity<GitTurtle>,
        &'a mut gpui::VisualTestContext,
    ) {
        working_diff_edited(cx, script, &[("notes.txt", "edited\n")]).await
    }

    /// [`working_diff`] in a repository `script` imports, each of `edits`
    /// written over its tracked file, showing the first changed file.
    async fn working_diff_edited<'a>(
        cx: &'a mut gpui::TestAppContext,
        script: &str,
        edits: &[(&str, &str)],
    ) -> (
        tempfile::TempDir,
        gpui::Entity<GitTurtle>,
        &'a mut gpui::VisualTestContext,
    ) {
        use crate::views::tests::{fixture_git, history_window_from, settle};
        let (fixture, app, cx) = history_window_from(cx, script).await;
        let root = fixture.path().join("narrow");
        fixture_git(&root, &["reset", "--hard", "--quiet"]);
        for (path, text) in edits {
            std::fs::write(root.join(path), text).unwrap();
        }
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.text_mode = TextMode::Unified;
                app.select_working(0, gitturtle_core::ChangeArea::Unstaged, window, cx);
            })
        });
        settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert!(matches!(
                app.content.as_deref(),
                Some(Content::Text {
                    partial: Some(_),
                    ..
                })
            ));
        });
        assert_eq!(suspended(&app, cx), (false, false));
        (fixture, app, cx)
    }

    /// Whether the shown unified diff's partial actions are suspended, and
    /// whether a text review is pending.
    fn suspended(app: &gpui::Entity<GitTurtle>, cx: &mut gpui::VisualTestContext) -> (bool, bool) {
        app.read_with(cx, |app, cx| {
            let view = app.patch_view.as_ref().expect("the unified diff");
            (
                view.read(cx).partial_suspended(),
                app.review.pending.is_some(),
            )
        })
    }

    /// QA-101's route to a failed review: Working Changes showing a tracked
    /// file of 50,001 lines with one line changed, its diff small but its
    /// two sides past the review's 100,000-line limit, in QA's 1480 × 973
    /// window, and Hide whitespace pressed in the review bar. The review's
    /// read fails, and its error replaces the diff.
    async fn review_past_the_limit(
        cx: &mut gpui::TestAppContext,
    ) -> (
        tempfile::TempDir,
        gpui::Entity<GitTurtle>,
        &mut gpui::VisualTestContext,
    ) {
        use gpui::{Modifiers, px, size};
        let long: String = (1..=MAX_REVIEW_ROWS / 2 + 1)
            .map(|line| format!("{line}\n"))
            .collect();
        let script = crate::views::tests::history_script(40, 3).replace(
            "done\n",
            &format!(
                "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> 1700000100 \
                 +0000\ndata 10\nadd a long\nM 100644 inline long.txt\ndata {}\n{long}\ndone\n",
                long.len()
            ),
        );
        let edited = long.replacen("1\n", "one\n", 1);
        let (fixture, app, cx) =
            working_diff_edited(cx, &script, &[("long.txt", edited.as_str())]).await;
        cx.simulate_resize(size(px(1480.), px(973.)));
        crate::tags::tests::draw(cx);
        let hide = cx
            .debug_bounds("hide-whitespace-changes")
            .expect("the full row's Hide whitespace");
        cx.simulate_click(hide.center(), Modifiers::default());
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert!(
                app.error
                    .as_deref()
                    .is_some_and(|error| error.contains("100,000-line review limit")),
                "the review fails: {:?}",
                app.error
            );
        });
        let body = crate::native_accessibility::written::get("empty-state:Preview unavailable")
            .expect("the error replaces the diff");
        assert!(
            body.node
                .value()
                .is_some_and(|text| text.contains("100,000-line review limit")),
            "{:?}",
            body.node.value()
        );
        (fixture, app, cx)
    }

    /// The selector of the review bar's caption reading `caption`.
    fn caption_shown(caption: &str) -> &'static str {
        format!("text-review-caption/{caption}").leak()
    }

    /// QA-101 O1: while a failed review's error replaces the diff, the
    /// review bar says the review is unavailable, never that it shows the
    /// original Git diff, which nothing shows; Reset review brings the diff
    /// back, and with it the caption its options give.
    #[gpui::test]
    async fn a_failed_review_reads_unavailable_until_reset(cx: &mut gpui::TestAppContext) {
        use gpui::Modifiers;
        let (_fixture, app, cx) = review_past_the_limit(cx).await;
        assert!(
            cx.debug_bounds(caption_shown(UNAVAILABLE_CAPTION))
                .is_some(),
            "the bar says the review is unavailable"
        );
        assert!(
            cx.debug_bounds(caption_shown(ORIGINAL_CAPTION)).is_none(),
            "not that it shows the original Git diff"
        );
        let reset = cx.debug_bounds("reset-text-review").expect("Reset review");
        cx.simulate_click(reset.center(), Modifiers::default());
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(app.error, None, "the diff is back");
            assert_eq!(app.review.options, Options::default());
        });
        assert!(
            cx.debug_bounds(caption_shown(ORIGINAL_CAPTION)).is_some(),
            "the original diff's caption is back"
        );
        assert!(
            cx.debug_bounds(caption_shown(UNAVAILABLE_CAPTION))
                .is_none()
        );
    }

    /// A refusal never replaces the diff being read: the tab's close
    /// button, clicked while a write runs in its repository, says why
    /// beneath the tab strip, announced at once, while the diff stays drawn
    /// with the caption its options give. Set as the window's error, the
    /// refusal took the diff's place ("Preview unavailable").
    #[gpui::test]
    async fn a_refusal_over_the_diff_keeps_the_options_caption(cx: &mut gpui::TestAppContext) {
        use crate::native_accessibility::written;
        let (_fixture, app, cx) = working_diff(cx).await;
        cx.simulate_resize(gpui::size(gpui::px(1480.), gpui::px(973.)));
        app.update(cx, |app, _| {
            app.operation_busy = Some("Fetching");
            app.operation_repository = app.path.clone();
        });
        crate::tags::tests::draw(cx);
        let close = cx
            .debug_bounds("repository-tab-close-0")
            .expect("the tab's close button");
        cx.simulate_click(close.center(), gpui::Modifiers::default());
        app.update(cx, |app, _| {
            app.operation_busy = None;
            app.operation_repository = None;
        });
        written::forget();
        crate::tags::tests::draw(cx);
        app.read_with(cx, |app, _| {
            assert_eq!(app.repository_tabs.tabs.len(), 1, "the close is refused");
            assert_eq!(app.error, None, "not as the window's error");
            assert!(app.shown_text().is_some(), "the diff stays drawn");
        });
        assert!(
            cx.debug_bounds("repository-tab-refusal").is_some(),
            "the strip says why"
        );
        let announced = written::announced_from("repository-tab-refusal");
        assert!(
            announced.len() == 1 && announced[0].1 == written::Announced::ASSERTIVE,
            "announced at once: {announced:?}"
        );
        assert!(
            cx.debug_bounds(caption_shown(ORIGINAL_CAPTION)).is_some(),
            "the options' caption"
        );
        assert!(
            cx.debug_bounds(caption_shown(UNAVAILABLE_CAPTION))
                .is_none(),
            "not the review's failure"
        );
    }

    /// The failed review leaves its tab's label alone, before Reset review
    /// and after it: " · unavailable" speaks for the repository, which
    /// still reads. Marked by the failure, the tab kept the mark after Reset
    /// review brought the diff back (QA-101 O1).
    #[gpui::test]
    async fn a_failed_review_never_marks_its_tab_unavailable(cx: &mut gpui::TestAppContext) {
        use gpui::Modifiers;
        let (_fixture, app, cx) = review_past_the_limit(cx).await;
        let tab = |cx: &mut gpui::VisualTestContext| {
            app.read_with(cx, |app, _| {
                let tab = &app.repository_tabs.tabs[app.repository_tabs.active.expect("a tab")];
                (app.tab_label(tab).0, tab.error.clone())
            })
        };
        let (label, error) = tab(cx);
        assert!(
            !label.ends_with(" · unavailable") && error.is_none(),
            "after the failure: {label:?}, {error:?}"
        );
        let reset = cx.debug_bounds("reset-text-review").expect("Reset review");
        cx.simulate_click(reset.center(), Modifiers::default());
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(tab(cx), (label, None), "after Reset review");
    }

    /// The failed review's error stays in its pane: once Reset review has
    /// brought the diff back and the last tab is closed, the Projects page
    /// shows no error. Set on the hub by every failed read, it met the
    /// person there as an assertive alert naming the review's limit.
    #[gpui::test]
    async fn a_failed_review_leaves_the_projects_page_without_its_error(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::native_accessibility::written;
        use gpui::Modifiers;
        let (_fixture, app, cx) = review_past_the_limit(cx).await;
        let reset = cx.debug_bounds("reset-text-review").expect("Reset review");
        cx.simulate_click(reset.center(), Modifiers::default());
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(app.read_with(cx, |app, _| app.error.clone()), None);
        assert_eq!(
            app.read_with(cx, |app, _| app.repository_tabs.tabs.len()),
            1
        );
        written::forget();
        cx.update(|window, cx| app.update(cx, |app, cx| app.close_repository_tab(0, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| app.page == crate::AppPage::Projects));
        assert!(
            cx.debug_bounds("project-hub-action").is_some(),
            "the Projects page is drawn"
        );
        assert_eq!(
            written::announced_from("project-hub-error"),
            Vec::new(),
            "with no error alert"
        );
    }

    /// Presses the Stage button of the shown diff's first hunk where the
    /// window draws it, as a person does, and answers the write that
    /// started, if any, once it has landed: the window draws the frames it
    /// asks for, the pointer arrives at the button's centre in the frame
    /// shown, the window draws what that changes, which must leave the
    /// button where it was drawn, and the button is pressed and released
    /// there. Emitted into the diff view instead, as this helper once did,
    /// a press could not see a button drawn off its hit area, as QA-101 F1's
    /// Stage was, 8 px above its row until the pointer's arrival drew it in
    /// place. The write the press starts waits on the operation executor
    /// until the helper has read that it started: the click's own
    /// `run_until_parked` takes any reply that has arrived, and on a loaded
    /// host the frame the press draws once outlasted `git apply`, so the
    /// write had ended, its status read already running, before the helper
    /// read `operation_busy`, and a press that staged read as one that
    /// started nothing.
    async fn press_drawn_stage(
        app: &gpui::Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
    ) -> Option<&'static str> {
        use gpui::Modifiers;
        let row = app.read_with(cx, |app, _| {
            let Some(Content::Text {
                partial: Some(partial),
                ..
            }) = app.content.as_deref()
            else {
                panic!("the diff offers its partial actions");
            };
            partial
                .rows
                .iter()
                .position(|row| matches!(row, crate::partial_view::PartialRow::Hunk(_)))
                .expect("a hunk")
        });
        let selector: &'static str = format!("partial-hunk-{row}").leak();
        native_frames(cx);
        let stage = cx
            .debug_bounds(selector)
            .expect("the hunk's Stage is drawn");
        cx.simulate_mouse_move(stage.center(), None, Modifiers::default());
        native_frames(cx);
        assert_eq!(
            cx.debug_bounds(selector),
            Some(stage),
            "the pointer's arrival leaves Stage where the frame shown drew it"
        );
        let write_held = app.read_with(cx, |app, _| app.operations.hold());
        cx.simulate_click(stage.center(), Modifiers::default());
        let started = app.read_with(cx, |app, _| app.operation_busy);
        drop(write_held);
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(app, cx).await;
        started
    }

    /// Starts a review of the shown diff, holding the reader so that its
    /// preparation is still under way as `then` runs, then releases it.
    async fn while_a_review_is_prepared(
        app: &gpui::Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
        then: impl FnOnce(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>),
    ) {
        let held = app.read_with(cx, |app, _| {
            app.worker.wait_idle();
            app.worker.hold()
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.automatic.hold_quiet_reads(true);
                let options = Options {
                    hide_whitespace: true,
                    ..Options::default()
                };
                app.set_text_review(options, window, cx);
                assert_eq!(app.loading, Some("Preparing text review…"));
                then(app, window, cx);
            })
        });
        drop(held);
        crate::views::tests::settle(app, cx).await;
    }

    const STAGING: Option<&str> = Some("Staging selected changes…");

    /// A text review whose read fails (here an unsupported context size;
    /// past the review's limits in practice) gives the shown diff's partial
    /// actions back, and a press of Stage then stages, though the review
    /// moved the window's read on (`bind_text_partial`). Left suspended,
    /// they stayed refused, a hunk's Stage disabled and a line click
    /// selecting nothing, until another file was chosen; released but
    /// bound to the read before the review, Stage looked enabled and a
    /// press started nothing. Back drops the comparison instead
    /// (`late_preview_tests::back_cancels_a_preview_read_under_any_label`).
    #[gpui::test]
    async fn a_review_ended_unprepared_gives_the_partial_actions_back(
        cx: &mut gpui::TestAppContext,
    ) {
        let (_fixture, app, cx) = working_diff(cx).await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                let options = Options {
                    hide_whitespace: false,
                    context: 5,
                };
                app.set_text_review(options, window, cx);
            })
        });
        assert_eq!(
            suspended(&app, cx),
            (true, true),
            "suspended while prepared"
        );
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(
                app.error.as_deref(),
                Some("Choose a supported context size.")
            );
        });
        assert_eq!(
            suspended(&app, cx),
            (false, false),
            "a failed review gives them back"
        );
        // The error replaces the diff, so no Stage is drawn until Reset
        // review, in QA's window's full row, brings the diff back.
        cx.simulate_resize(gpui::size(gpui::px(1480.), gpui::px(973.)));
        crate::tags::tests::draw(cx);
        let reset = cx.debug_bounds("reset-text-review").expect("Reset review");
        cx.simulate_click(reset.center(), gpui::Modifiers::default());
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(
            press_drawn_stage(&app, cx).await,
            STAGING,
            "and Stage stages"
        );
    }

    /// A hunk's Stage pressed while a write runs in another tab starts
    /// nothing, and its controls return at once: once that write ends, the
    /// next press stages. Left suspended by the press the write refused,
    /// Stage stayed disabled and a line click selected nothing until
    /// another file was chosen.
    #[gpui::test]
    async fn a_partial_press_refused_by_another_write_gives_its_controls_back(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::Modifiers;
        let (fixture, app, cx) = working_diff(cx).await;
        let row = app.read_with(cx, |app, _| {
            let Some(Content::Text {
                partial: Some(partial),
                ..
            }) = app.content.as_deref()
            else {
                panic!("the diff offers its partial actions");
            };
            partial
                .rows
                .iter()
                .position(|row| matches!(row, crate::partial_view::PartialRow::Hunk(_)))
                .expect("a hunk")
        });
        let selector: &'static str = format!("partial-hunk-{row}").leak();
        app.update(cx, |app, _| {
            app.operation_busy = Some("Fetching");
            app.operation_repository = Some(fixture.path().join("another tab"));
        });
        native_frames(cx);
        let stage = cx.debug_bounds(selector).expect("the hunk's Stage");
        cx.simulate_mouse_move(stage.center(), None, Modifiers::default());
        native_frames(cx);
        cx.simulate_click(stage.center(), Modifiers::default());
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            Some("Fetching"),
            "the press starts nothing"
        );
        assert_eq!(suspended(&app, cx), (false, false), "its controls return");
        app.update(cx, |app, _| {
            app.operation_busy = None;
            app.operation_repository = None;
        });
        assert_eq!(
            press_drawn_stage(&app, cx).await,
            STAGING,
            "and once that write ends Stage stages"
        );
    }

    /// A text review whose read the worker drops unanswered, as a cancel
    /// that leaves the window's read where it was drops it, gives the shown
    /// diff's partial actions back, and a press of Stage then stages.
    #[gpui::test]
    async fn a_review_dropped_unanswered_gives_the_partial_actions_back(
        cx: &mut gpui::TestAppContext,
    ) {
        let (_fixture, app, cx) = working_diff(cx).await;
        let held = app.read_with(cx, |app, _| {
            app.worker.wait_idle();
            app.worker.hold()
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.automatic.hold_quiet_reads(true);
                let options = Options {
                    hide_whitespace: true,
                    ..Options::default()
                };
                app.set_text_review(options, window, cx);
            })
        });
        assert_eq!(
            suspended(&app, cx),
            (true, true),
            "suspended while prepared"
        );
        app.read_with(cx, |app, _| app.worker.cancel());
        drop(held);
        cx.executor().run_until_parked();
        assert_eq!(
            suspended(&app, cx),
            (false, false),
            "a dropped review gives them back"
        );
        assert_eq!(
            press_drawn_stage(&app, cx).await,
            STAGING,
            "and Stage stages"
        );
    }

    /// Blame opened over the shown diff while a review of it is prepared
    /// ends the review with its read and gives the partial actions back,
    /// and once Blame closes a press of Stage stages.
    #[gpui::test]
    async fn blame_during_a_review_gives_the_partial_actions_back(cx: &mut gpui::TestAppContext) {
        let (_fixture, app, cx) = working_diff(cx).await;
        let held = app.read_with(cx, |app, _| {
            app.worker.wait_idle();
            app.worker.hold()
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.automatic.hold_quiet_reads(true);
                let options = Options {
                    hide_whitespace: true,
                    ..Options::default()
                };
                app.set_text_review(options, window, cx);
                assert_eq!(app.loading, Some("Preparing text review…"));
                app.open_blame(0, window, cx);
                assert!(app.blame.is_visible());
            })
        });
        assert_eq!(suspended(&app, cx), (false, false), "Blame gives them back");
        drop(held);
        crate::views::tests::settle(&app, cx).await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                assert!(app.close_blame(window, cx));
                app.automatic.hold_quiet_reads(false);
            })
        });
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(
            suspended(&app, cx),
            (false, false),
            "and so does its return"
        );
        assert_eq!(
            press_drawn_stage(&app, cx).await,
            STAGING,
            "and Stage stages"
        );
    }

    /// Blame opened from Working Changes and closed again, with no review,
    /// keeps the diff's partial actions working, though closing it moves
    /// the window's read on: they act on the diff shown, not at a read
    /// (`bind_text_partial`). Bound to the read before, a press of Stage or Unstage
    /// after any attribution round trip started nothing and said nothing.
    #[gpui::test]
    async fn a_blame_round_trip_keeps_the_partial_actions_working(cx: &mut gpui::TestAppContext) {
        let (_fixture, app, cx) = working_diff(cx).await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_blame(0, window, cx);
                assert!(app.blame.is_visible());
            })
        });
        crate::views::tests::settle(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| assert!(app.close_blame(window, cx))));
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(suspended(&app, cx), (false, false));
        assert_eq!(press_drawn_stage(&app, cx).await, STAGING, "Stage stages");
    }

    /// Quick Open's inspection of a tracked file, opened while a review of
    /// the shown diff is prepared, keeps that diff for Back: the review
    /// ends with its read, and the diff Back brings back offers its
    /// partial actions, which stage. It came back with them suspended.
    #[gpui::test]
    async fn quick_open_during_a_review_gives_the_partial_actions_back(
        cx: &mut gpui::TestAppContext,
    ) {
        use gitturtle_core::{HistoryCancellation, PathScope};
        let (_fixture, app, cx) = working_diff(cx).await;
        while_a_review_is_prepared(&app, cx, |app, window, cx| {
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
            assert!(app.revision_inspection.is_active());
        })
        .await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                assert!(app.close_revision_inspection(window, cx));
                let view = app.patch_view.as_ref().expect("the diff kept for Back");
                assert!(
                    !view.read(cx).partial_suspended(),
                    "the diff comes back with its partial actions"
                );
                app.automatic.hold_quiet_reads(false);
            })
        });
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(suspended(&app, cx), (false, false));
        assert_eq!(
            press_drawn_stage(&app, cx).await,
            STAGING,
            "and Stage stages"
        );
    }

    /// QA-101 F1's route, every step through the window as a person takes
    /// it: Working Changes showing a modified file's unified diff, a
    /// passive read running on the reader (focus regained, as QA's slow
    /// `git status` ran; here the reader is held), More context chosen so
    /// that its review waits behind that read, Quick Open by its command
    /// inspecting another tracked file by its keys, then Escape back to the
    /// diff, the read still held. The hunk's Stage reads enabled, and one
    /// click on it starts the write.
    #[gpui::test]
    async fn quick_open_behind_a_held_read_keeps_the_first_press(cx: &mut gpui::TestAppContext) {
        use crate::tags::tests::draw;
        use gpui::{Modifiers, px, size};
        use gpui_kit::component::WindowExt;
        let script = crate::views::tests::history_script(40, 3).replace(
            "done\n",
            "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> 1700000100 +0000\n\
             data 11\nadd a small\nM 100644 inline small.txt\ndata 6\nsmall\n\ndone\n",
        );
        let (_fixture, app, cx) = working_diff_from(cx, &script).await;
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        // QA's window size, with the diff shown again in it, so the window
        // holds nothing measured at the size before.
        cx.simulate_resize(size(px(1480.), px(973.)));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.select_working(0, gitturtle_core::ChangeArea::Unstaged, window, cx)
            })
        });
        crate::views::tests::settle(&app, cx).await;
        let held = app.read_with(cx, |app, _| {
            app.worker.wait_idle();
            app.worker.hold()
        });
        // The passive read regaining focus asks for (`observe_window_activation`).
        cx.update(|window, cx| app.update(cx, |app, cx| app.retry_automatic_refresh(window, cx)));
        assert!(
            app.read_with(cx, |app, _| app.automatic.quiet_read_running()),
            "a passive read runs"
        );
        draw(cx);
        let more = cx
            .debug_bounds("expand-text-context")
            .expect("More context");
        cx.simulate_click(more.center(), Modifiers::default());
        draw(cx);
        assert_eq!(
            app.read_with(cx, |app, _| app.loading),
            Some("Preparing text review…"),
            "the review waits behind the passive read"
        );
        cx.dispatch_action(crate::QuickOpenFile);
        draw(cx);
        cx.simulate_input("small");
        crate::revision_inspection::settle_quick_open(cx).await;
        cx.simulate_keystrokes("enter");
        draw(cx);
        // Time passes after the dialog closed, before the next key.
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(400));
        draw(cx);
        assert!(
            app.read_with(cx, |app, _| app.revision_inspection.is_active()),
            "Quick Open inspects small.txt"
        );
        assert!(
            !cx.update(|window, cx| window.has_active_dialog(cx)),
            "Quick Open closed"
        );
        // Escape, and only the frames the window then asks for, each as a
        // platform delivers it: the frame's callbacks run, then the window
        // draws once if anything changed (`Window::on_request_frame`). So
        // Back's own repaint (`repaint_page`) is the frame Escape draws.
        cx.update(|window, cx| {
            window.dispatch_keystroke(gpui::Keystroke::parse("escape").unwrap(), cx);
            window.simulate_next_frame(cx);
        });
        native_frames(cx);
        let row = app.read_with(cx, |app, cx| {
            assert!(app.mode == WorkspaceMode::Working, "back on the diff");
            assert!(!app.revision_inspection.is_active());
            let view = app.patch_view.as_ref().expect("the diff kept for Back");
            assert!(!view.read(cx).partial_suspended(), "Stage reads enabled");
            let Some(Content::Text {
                partial: Some(partial),
                ..
            }) = app.content.as_deref()
            else {
                panic!("the diff offers its partial actions");
            };
            partial
                .rows
                .iter()
                .position(|row| matches!(row, crate::partial_view::PartialRow::Hunk(_)))
                .expect("a hunk")
        });
        // A press on the Stage button where that frame draws it, in its
        // upper part: the pointer arrives, the window draws what that
        // changes, then the button is pressed and released. That frame once
        // drew it 8 px above its row, and the pointer's arrival drew it in
        // place, from under the pointer, so the press started nothing.
        let stage = cx
            .debug_bounds(format!("partial-hunk-{row}").leak())
            .expect("the hunk's Stage");
        let press = gpui::point(stage.center().x, stage.top() + stage.size.height / 4.);
        cx.simulate_mouse_move(press, None, Modifiers::default());
        native_frames(cx);
        // The write waits until its start is read, as in `press_drawn_stage`.
        let write_held = app.read_with(cx, |app, _| app.operations.hold());
        cx.simulate_click(press, Modifiers::default());
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            STAGING,
            "the first press stages, the passive read still held"
        );
        drop(write_held);
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
    }

    /// Delivers the frames the window asks for with `on_next_frame`, each
    /// in GPUI's order: its callbacks run, then the test app draws the
    /// window as the update ends, once and only if anything changed.
    fn native_frames(cx: &mut gpui::VisualTestContext) {
        for _ in 0..8 {
            cx.run_until_parked();
            if cx.update(|window, cx| window.simulate_next_frame(cx)) == 0 {
                return;
            }
        }
        panic!("the window still asks for frames after 8");
    }

    /// Another repository tab, opened while a review of the shown diff is
    /// prepared, keeps this tab's diff warm for its return: the review ends
    /// with its read, and the diff the tab comes back with offers its
    /// partial actions, which stage. It came back with them suspended.
    #[gpui::test]
    async fn a_tab_switch_during_a_review_gives_the_partial_actions_back(
        cx: &mut gpui::TestAppContext,
    ) {
        let (fixture, app, cx) = working_diff(cx).await;
        let second = crate::views::tests::fixture_repository(fixture.path().join("second"));
        let second = second.path().to_owned();
        while_a_review_is_prepared(&app, cx, |app, window, cx| {
            app.open_repository_tab(second, window, cx);
            assert_eq!(app.repository_tabs.active, Some(1));
        })
        .await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.switch_repository_tab(0, window, cx);
                let view = app.patch_view.as_ref().expect("the warm tab's diff");
                assert!(
                    !view.read(cx).partial_suspended(),
                    "the tab comes back with its partial actions"
                );
                app.automatic.hold_quiet_reads(false);
            })
        });
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(suspended(&app, cx), (false, false));
        assert_eq!(
            press_drawn_stage(&app, cx).await,
            STAGING,
            "and Stage stages"
        );
    }

    /// Presses and releases each of `keys` in turn, as a keyboard does.
    fn press(cx: &mut gpui::VisualTestContext, keys: &str) {
        for key in keys.split(' ') {
            cx.simulate_keystrokes(key);
            let keystroke = gpui::Keystroke::parse(key).unwrap();
            cx.simulate_event(gpui::KeyUpEvent { keystroke });
        }
        crate::tags::tests::draw(cx);
    }

    /// The shown diff's text and its view.
    fn shown_diff(
        app: &gpui::Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
    ) -> (
        gpui::Entity<gpui_kit::component::input::EditorState>,
        gpui::Entity<crate::diff_view::DiffView>,
    ) {
        app.read_with(cx, |app, _| {
            (
                app.patch_editor.clone().expect("the patch's editor"),
                app.patch_view.clone().expect("the Diff view"),
            )
        })
    }

    /// The window's primary modifier with `key`, Command on a Mac and
    /// Control elsewhere.
    fn primary(key: &str) -> String {
        if cfg!(target_os = "macos") {
            format!("cmd-{key}")
        } else {
            format!("ctrl-{key}")
        }
    }

    /// Every changed line of the shown file selected from the keyboard,
    /// Select All and Space, then Tab to the selection bar's action and
    /// Enter: a real partial write stages them all, the file leaves
    /// Unstaged, and focus lands on the file list where the file was, its
    /// ring drawn, rather than on nothing. Shift+Tab from the action goes
    /// back to the text first.
    #[gpui::test]
    async fn a_keyboard_stage_of_every_line_leaves_focus_on_the_drawn_file_list(
        cx: &mut gpui::TestAppContext,
    ) {
        let (fixture, app, cx) = working_diff(cx).await;
        crate::focus_ring::sweep::prepare(&app, cx);
        let accent: gpui::Hsla =
            cx.read(|cx| gpui::rgb(crate::appearance::palette(cx).accent).into());
        let (editor, view) = shown_diff(&app, cx);
        cx.update(|window, cx| {
            let focus = view.read(cx).focus_handle(cx);
            window.focus(&focus, cx)
        });
        crate::tags::tests::draw(cx);
        press(cx, &primary("a"));
        press(cx, "space");
        let (apply, text) = cx.update(|_, cx| {
            let view = view.read(cx);
            (view.apply_focus_handle(), view.focus_handle(cx))
        });
        press(cx, "tab");
        assert!(
            cx.update(|window, _| apply.is_focused(window)),
            "Tab reaches the action"
        );
        press(cx, "shift-tab");
        assert!(
            cx.update(|window, _| text.is_focused(window)),
            "Shift+Tab goes back to the text, focus on {:?}",
            cx.update(|window, cx| window.focused(cx))
        );
        press(cx, "tab");
        assert!(cx.update(|window, _| apply.is_focused(window)));
        press(cx, "enter");
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        crate::tags::tests::draw(cx);
        let staged = crate::views::tests::fixture_git_output(
            &fixture.path().join("narrow"),
            &["diff", "--cached", "--name-only"],
        );
        assert_eq!(
            String::from_utf8_lossy(&staged.stdout).trim(),
            "notes.txt",
            "the lines were staged"
        );
        let unstaged = app.read_with(cx, |app, _| {
            app.work_status.as_ref().map(|status| {
                status
                    .entries
                    .iter()
                    .filter(|entry| entry.unstaged.is_some())
                    .count()
            })
        });
        assert_eq!(unstaged, Some(0), "the file left Unstaged");
        let list = app.read_with(cx, |app, _| app.file_focus.clone());
        assert!(
            cx.update(|window, _| list.is_focused(window)),
            "focus lands on the file list, not on {:?}",
            cx.update(|window, cx| window.focused(cx))
        );
        let rings = crate::focus_ring::painted_outlines(cx, accent);
        assert!(
            rings.iter().any(|ring| ring.whole),
            "the focused list draws its ring whole: {rings:?}"
        );
        let _ = editor;
    }

    /// Space in Find's field over the diff types a space there and selects
    /// no line, though the text it searches has per-line checkboxes.
    #[gpui::test]
    async fn space_in_find_types_a_space_and_selects_no_line(cx: &mut gpui::TestAppContext) {
        let (_fixture, app, cx) = working_diff(cx).await;
        let (editor, view) = shown_diff(&app, cx);
        cx.update(|window, cx| {
            let focus = view.read(cx).focus_handle(cx);
            window.focus(&focus, cx)
        });
        crate::tags::tests::draw(cx);
        press(cx, &primary("f"));
        assert_eq!(
            cx.update(|_, cx| crate::editor_find::query(&editor, cx)),
            Some("".into()),
            "Find opens"
        );
        press(cx, "space");
        assert_eq!(
            cx.update(|_, cx| crate::editor_find::query(&editor, cx)),
            Some(" ".into()),
            "Space types a space in Find's field"
        );
        assert!(
            view.read_with(cx, |view, _| view.selected_lines() == 0),
            "and selects no line"
        );
    }

    /// notes.txt as the partial-write tests edit it: "added one" and "added
    /// two" inserted after note 10, note 12 replaced by "twelve", and 60
    /// tail lines appended, so the diff scrolls in QA's window.
    fn landing_notes() -> String {
        let mut text = String::new();
        for line in 1..=40 {
            match line {
                11 => text.push_str("added one\nadded two\nnote 11\n"),
                12 => text.push_str("twelve\n"),
                _ => text.push_str(&format!("note {line}\n")),
            }
        }
        for line in 0..60 {
            text.push_str(&format!("tail {line}\n"));
        }
        text
    }

    /// The row of the shown patch's line `line`.
    fn patch_row(
        editor: &gpui::Entity<gpui_kit::component::input::EditorState>,
        cx: &mut gpui::VisualTestContext,
        line: &str,
    ) -> usize {
        editor.read_with(cx, |state, _| {
            state
                .value()
                .split('\n')
                .position(|shown| shown == line)
                .unwrap_or_else(|| panic!("the patch shows {line:?}"))
        })
    }

    /// The text's caret row and scroll offset.
    fn text_place(
        editor: &gpui::Entity<gpui_kit::component::input::EditorState>,
        cx: &mut gpui::VisualTestContext,
    ) -> (usize, gpui::Point<gpui::Pixels>) {
        use gpui_kit::component::input::RopeExt as _;
        editor.read_with(cx, |state, _| {
            let caret = state.text().offset_to_point(state.cursor()).row;
            (caret, state.scroll_offset())
        })
    }

    /// What a partial write's route reads in its frames.
    struct Route {
        editor: gpui::Entity<gpui_kit::component::input::EditorState>,
        view: gpui::Entity<crate::diff_view::DiffView>,
        caret: usize,
        scroll: gpui::Point<gpui::Pixels>,
    }

    impl Route {
        /// QA-118 P3.10's route in `area` of notes.txt edited to `notes`
        /// ([`landing_notes`]) at 13 pt in QA's 1480 × 973 Midnight window: the text scrolled so
        /// "+added one" is its first row, a click on that line, Space, Down,
        /// Space, Tab, then, with the operations executor held, Enter, which
        /// starts the write and leaves it held. `written` is forgotten
        /// before Enter.
        async fn press<'a>(
            cx: &'a mut gpui::TestAppContext,
            area: gitturtle_core::ChangeArea,
            notes: &str,
        ) -> (
            tempfile::TempDir,
            gpui::Entity<GitTurtle>,
            &'a mut gpui::VisualTestContext,
            Route,
            crate::operations::Held,
        ) {
            use crate::native_accessibility::written;
            use crate::views::tests::{fixture_git, settle};
            let (fixture, app, cx) = working_diff_edited(
                cx,
                &crate::views::tests::history_script(1, 40),
                &[("notes.txt", notes)],
            )
            .await;
            crate::focus_ring::sweep::prepare(&app, cx);
            if area == gitturtle_core::ChangeArea::Staged {
                fixture_git(&fixture.path().join("narrow"), &["add", "notes.txt"]);
                cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
                settle(&app, cx).await;
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| app.select_working(0, area, window, cx))
                });
                settle(&app, cx).await;
            }
            cx.simulate_resize(gpui::size(gpui::px(1480.), gpui::px(973.)));
            crate::focus_ring::sweep::interface_size(cx, 13);
            let (editor, view) = shown_diff(&app, cx);
            let first = patch_row(&editor, cx, "+added one");
            editor.update(cx, |state, cx| {
                let height = state.line_height().expect("the text is laid out");
                state.set_scroll_offset(gpui::point(gpui::px(0.), -height * first as f32), cx);
            });
            crate::tags::tests::draw(cx);
            let (origin, height) = editor.read_with(cx, |state, _| {
                (
                    state.text_bounds().expect("the text is painted").origin,
                    state.line_height().expect("the text is laid out"),
                )
            });
            cx.simulate_click(
                gpui::point(
                    origin.x + gpui::px(40.),
                    origin.y + height * (first as f32 + 0.5),
                ),
                gpui::Modifiers::default(),
            );
            crate::tags::tests::draw(cx);
            assert_eq!(text_place(&editor, cx).0, first, "the click's line");
            press(cx, "space");
            press(cx, "down");
            press(cx, "space");
            assert_eq!(view.read_with(cx, |view, _| view.selected_lines()), 2);
            let apply = view.read_with(cx, |view, _| view.apply_focus_handle());
            press(cx, "tab");
            assert!(
                cx.update(|window, _| apply.is_focused(window)),
                "Tab reaches the action"
            );
            let (caret, scroll) = text_place(&editor, cx);
            assert_eq!(caret, first + 1, "Down's line");
            let held = app.read_with(cx, |app, _| app.operations.hold());
            written::forget();
            let enter = gpui::Keystroke::parse("enter").unwrap();
            cx.simulate_keystrokes("enter");
            cx.simulate_event(gpui::KeyUpEvent { keystroke: enter });
            (
                fixture,
                app,
                cx,
                Route {
                    editor,
                    view,
                    caret,
                    scroll,
                },
                held,
            )
        }

        /// Asserts, `when` the write runs held, that the window draws the
        /// Diff view the write was made in, never "Choose a file", with its
        /// text focused and its caret's mark drawn, the caret, line
        /// selection and scroll as the press left them, the bar's action
        /// drawn and disabled, and the text's description naming no Space.
        fn assert_kept(
            &self,
            app: &gpui::Entity<GitTurtle>,
            cx: &mut gpui::VisualTestContext,
            label: &str,
            when: &str,
        ) {
            assert_eq!(
                app.read_with(cx, |app, _| app.operation_busy),
                Some(label),
                "{when}: the write runs"
            );
            assert!(
                cx.debug_bounds("empty-state").is_none(),
                "{when}: no empty state in the Diff view's place"
            );
            app.read_with(cx, |app, _| {
                assert_eq!(
                    app.patch_view.as_ref(),
                    Some(&self.view),
                    "{when}: the view"
                );
                assert_eq!(
                    app.patch_editor.as_ref(),
                    Some(&self.editor),
                    "{when}: the text"
                );
            });
            assert!(
                cx.update(|window, cx| self.view.read(cx).focus_handle(cx).is_focused(window)),
                "{when}: focus in the text, not on {:?}",
                cx.update(|window, cx| window.focused(cx))
            );
            assert!(
                cx.debug_bounds("partial-line-caret").is_some(),
                "{when}: the caret's mark is drawn"
            );
            assert_eq!(
                text_place(&self.editor, cx),
                (self.caret, self.scroll),
                "{when}: the caret and scroll"
            );
            self.view.read_with(cx, |view, _| {
                assert_eq!(view.selected_lines(), 2, "{when}: the selection");
                assert!(view.partial_suspended(), "{when}: the bar is disabled");
            });
            assert!(
                cx.debug_bounds("apply-partial-selection").is_some(),
                "{when}: the bar's action is drawn"
            );
            assert_eq!(
                crate::views::tests::input_node(cx, &self.editor).description(),
                None,
                "{when}: Space does nothing during the write"
            );
        }

        /// Asserts, once the write has landed, that the same view shows the
        /// landed text in place with focus in it, the caret on the line
        /// `caret` and the line `top` first, no selection or bar left, and
        /// one spoken result, the write's own `said`.
        fn assert_landed(
            &self,
            app: &gpui::Entity<GitTurtle>,
            cx: &mut gpui::VisualTestContext,
            caret: &str,
            top: &str,
            said: &str,
        ) {
            use crate::native_accessibility::written;
            app.read_with(cx, |app, _| {
                assert_eq!(app.operation_busy, None, "the write has ended");
                assert_eq!(app.patch_view.as_ref(), Some(&self.view), "the same view");
                assert_eq!(
                    app.patch_editor.as_ref(),
                    Some(&self.editor),
                    "the same text"
                );
            });
            assert!(
                cx.update(|window, cx| self.view.read(cx).focus_handle(cx).is_focused(window)),
                "focus stays in the text, not on {:?}",
                cx.update(|window, cx| window.focused(cx))
            );
            let height = self
                .editor
                .read_with(cx, |state, _| state.line_height().expect("laid out"));
            let (row, scroll) = text_place(&self.editor, cx);
            assert_eq!(row, patch_row(&self.editor, cx, caret), "the caret's line");
            assert_eq!(
                scroll,
                gpui::point(
                    gpui::px(0.),
                    -height * patch_row(&self.editor, cx, top) as f32
                ),
                "the first visible line"
            );
            self.view.read_with(cx, |view, _| {
                assert_eq!(
                    view.selected_lines(),
                    0,
                    "the selection left with its lines"
                );
                assert!(!view.partial_suspended(), "the actions act again");
            });
            assert!(cx.debug_bounds("apply-partial-selection").is_none());
            let spoken: Vec<_> = written::announced_from("")
                .into_iter()
                .filter_map(|(id, _)| written::get(&id)?.node.label().map(str::to_owned))
                .collect();
            assert_eq!(spoken, [said], "one spoken result");
        }
    }

    /// QA-118 P3.10: a partial Stage keeps the Diff view through its write
    /// and its landing. Through the held write the window draws the text,
    /// its caret, selection and scroll as the press left them, focus in it
    /// and the bar disabled; once landed, the staged lines show as context
    /// in the same text, the caret on its line, the first visible line
    /// where it was, focus in the text and one spoken result. Torn down
    /// for the write, the view read "Choose a file" for its whole length,
    /// focus fell to the window frame, and the landing focused the list
    /// with the caret, selection and scroll lost.
    #[gpui::test]
    async fn a_partial_stage_keeps_the_diff_view_through_its_write(cx: &mut gpui::TestAppContext) {
        let (fixture, app, cx, route, held) =
            Route::press(cx, gitturtle_core::ChangeArea::Unstaged, &landing_notes()).await;
        route.assert_kept(
            &app,
            cx,
            "Staging selected changes…",
            "the frame after Enter",
        );
        native_frames(cx);
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        native_frames(cx);
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_shown()),
            Some("Staging selected changes…"),
            "the header says the write"
        );
        route.assert_kept(
            &app,
            cx,
            "Staging selected changes…",
            "while the write is held",
        );
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        crate::tags::tests::draw(cx);
        let staged = crate::views::tests::fixture_git_output(
            &fixture.path().join("narrow"),
            &["diff", "--cached"],
        );
        let staged = String::from_utf8_lossy(&staged.stdout).into_owned();
        assert!(
            staged.contains("+added one\n+added two\n") && !staged.contains("twelve"),
            "the two lines were staged: {staged}"
        );
        route.assert_landed(
            &app,
            cx,
            " added two",
            " added one",
            "Selected changes staged",
        );
    }

    /// [`a_partial_stage_keeps_the_diff_view_through_its_write`] for
    /// Unstage in the staged area: the unstaged lines leave the staged diff,
    /// so the caret goes to the nearest changed line after its own, and the
    /// first visible line to the line after where it stood.
    #[gpui::test]
    async fn a_partial_unstage_keeps_the_diff_view_through_its_write(
        cx: &mut gpui::TestAppContext,
    ) {
        let (fixture, app, cx, route, held) =
            Route::press(cx, gitturtle_core::ChangeArea::Staged, &landing_notes()).await;
        route.assert_kept(
            &app,
            cx,
            "Unstaging selected changes…",
            "the frame after Enter",
        );
        native_frames(cx);
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        native_frames(cx);
        route.assert_kept(
            &app,
            cx,
            "Unstaging selected changes…",
            "while the write is held",
        );
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        crate::tags::tests::draw(cx);
        let staged = crate::views::tests::fixture_git_output(
            &fixture.path().join("narrow"),
            &["diff", "--cached"],
        );
        let staged = String::from_utf8_lossy(&staged.stdout).into_owned();
        assert!(
            !staged.contains("added") && staged.contains("+twelve"),
            "the two lines were unstaged: {staged}"
        );
        route.assert_landed(
            &app,
            cx,
            "-note 12",
            " note 11",
            "Selected changes unstaged",
        );
    }

    /// A partial Stage that fails, its index locked by another Git as it
    /// runs, leaves the Diff view as the press left it: the text, caret,
    /// line selection and scroll stay, focus stays in the text, the failure
    /// shows where a write's failures show, the window's error banner, and
    /// the bar acts again, so a second press can stage the same lines.
    #[gpui::test]
    async fn a_failed_partial_stage_leaves_the_diff_view_as_it_was(cx: &mut gpui::TestAppContext) {
        let (fixture, app, cx, route, held) =
            Route::press(cx, gitturtle_core::ChangeArea::Unstaged, &landing_notes()).await;
        route.assert_kept(
            &app,
            cx,
            "Staging selected changes…",
            "the frame after Enter",
        );
        let lock = fixture
            .path()
            .join("narrow")
            .join(".git")
            .join("index.lock");
        std::fs::write(&lock, "").unwrap();
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        crate::tags::tests::draw(cx);
        std::fs::remove_file(&lock).unwrap();
        let staged = crate::views::tests::fixture_git_output(
            &fixture.path().join("narrow"),
            &["diff", "--cached", "--name-only"],
        );
        assert!(staged.stdout.is_empty(), "nothing was staged");
        app.read_with(cx, |app, _| {
            assert_eq!(app.operation_busy, None, "the write has ended");
            assert!(app.operation_error.is_some(), "it failed");
            assert_eq!(app.patch_view.as_ref(), Some(&route.view), "the same view");
            assert_eq!(
                app.patch_editor.as_ref(),
                Some(&route.editor),
                "the same text"
            );
        });
        assert!(
            cx.debug_bounds("operation-error-summary").is_some(),
            "the failure shows where a write's failures show"
        );
        assert!(cx.debug_bounds("empty-state").is_none());
        assert!(
            cx.update(|window, cx| route.view.read(cx).focus_handle(cx).is_focused(window)),
            "focus stays in the text, not on {:?}",
            cx.update(|window, cx| window.focused(cx))
        );
        assert_eq!(
            text_place(&route.editor, cx),
            (route.caret, route.scroll),
            "the caret and scroll"
        );
        route.view.read_with(cx, |view, _| {
            assert_eq!(view.selected_lines(), 2, "the selection stays");
            assert!(!view.partial_suspended(), "the bar acts again");
        });
        assert!(cx.debug_bounds("apply-partial-selection").is_some());
        assert!(cx.debug_bounds("partial-line-caret").is_some());
    }

    /// The first hunk's Stage hunk (Unstage hunk in `area`) of notes.txt
    /// edited to [`landing_notes`], in QA's 1480 × 973 window at 13 pt with
    /// the text at its top and focus on the file list, pressed with the
    /// operations executor held, `locked` with the index locked so the
    /// write fails: returns the press's route, its hunk header's row and the
    /// executor's hold.
    async fn press_a_hunk(
        cx: &mut gpui::TestAppContext,
        area: gitturtle_core::ChangeArea,
        locked: bool,
    ) -> (
        tempfile::TempDir,
        gpui::Entity<GitTurtle>,
        &mut gpui::VisualTestContext,
        Route,
        usize,
        crate::operations::Held,
    ) {
        press_the_hunk(cx, area, &landing_notes(), 0, locked).await
    }

    /// [`press_a_hunk`] on the `nth` hunk, counted from 0, of notes.txt
    /// edited to `notes`.
    async fn press_the_hunk<'a>(
        cx: &'a mut gpui::TestAppContext,
        area: gitturtle_core::ChangeArea,
        notes: &str,
        nth: usize,
        locked: bool,
    ) -> (
        tempfile::TempDir,
        gpui::Entity<GitTurtle>,
        &'a mut gpui::VisualTestContext,
        Route,
        usize,
        crate::operations::Held,
    ) {
        let (fixture, app, cx, route) = hunks_shown(cx, area, notes).await;
        let hunk = hunk_rows(&app, cx)[nth];
        if locked {
            std::fs::write(
                fixture
                    .path()
                    .join("narrow")
                    .join(".git")
                    .join("index.lock"),
                "",
            )
            .unwrap();
        }
        let held = app.read_with(cx, |app, _| app.operations.hold());
        let stage = cx
            .debug_bounds(format!("partial-hunk-{hunk}").leak())
            .expect("the hunk's Stage hunk is drawn");
        cx.simulate_click(stage.center(), gpui::Modifiers::default());
        crate::tags::tests::draw(cx);
        assert!(
            app.read_with(cx, |app, _| app.operation_busy.is_some()),
            "the press starts the write"
        );
        (fixture, app, cx, route, hunk, held)
    }

    /// notes.txt edited to `notes`, its diff in `area` shown in QA's 1480 ×
    /// 973 window at 13 pt, the text at its top and focus on the file list,
    /// nothing pressed yet: the fixture, its window and the route.
    async fn hunks_shown<'a>(
        cx: &'a mut gpui::TestAppContext,
        area: gitturtle_core::ChangeArea,
        notes: &str,
    ) -> (
        tempfile::TempDir,
        gpui::Entity<GitTurtle>,
        &'a mut gpui::VisualTestContext,
        Route,
    ) {
        use crate::views::tests::{fixture_git, settle};
        let (fixture, app, cx) = working_diff_edited(
            cx,
            &crate::views::tests::history_script(1, 40),
            &[("notes.txt", notes)],
        )
        .await;
        crate::focus_ring::sweep::prepare(&app, cx);
        if area == gitturtle_core::ChangeArea::Staged {
            fixture_git(&fixture.path().join("narrow"), &["add", "notes.txt"]);
            cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
            settle(&app, cx).await;
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.select_working(0, area, window, cx))
            });
            settle(&app, cx).await;
        }
        cx.simulate_resize(gpui::size(gpui::px(1480.), gpui::px(973.)));
        crate::focus_ring::sweep::interface_size(cx, 13);
        let (editor, view) = shown_diff(&app, cx);
        editor.update(cx, |state, cx| {
            state.set_scroll_offset(gpui::point(gpui::px(0.), gpui::px(0.)), cx)
        });
        let list = app.read_with(cx, |app, _| app.file_focus.clone());
        cx.update(|window, cx| window.focus(&list, cx));
        crate::tags::tests::draw(cx);
        let (caret, scroll) = text_place(&editor, cx);
        let route = Route {
            editor,
            view,
            caret,
            scroll,
        };
        (fixture, app, cx, route)
    }

    /// The rows of the shown diff's hunk headers, each drawing its Stage
    /// hunk.
    fn hunk_rows(app: &gpui::Entity<GitTurtle>, cx: &mut gpui::VisualTestContext) -> Vec<usize> {
        use crate::partial_view::PartialRow;
        app.read_with(cx, |app, _| {
            let Some(Content::Text {
                partial: Some(partial),
                ..
            }) = app.content.as_deref()
            else {
                panic!("the diff offers its partial actions");
            };
            partial
                .rows
                .iter()
                .enumerate()
                .filter_map(|(row, kind)| matches!(kind, PartialRow::Hunk(_)).then_some(row))
                .collect()
        })
    }

    /// A Stage hunk press focuses its button, as a pressed control takes
    /// focus, here from the file list; the landing that takes its hunk away
    /// gives that focus to the text, as from the bar's action or Clear,
    /// never to the window frame. Unfocused, the press left focus on the
    /// list, and the hunk's own write moved nothing to the text.
    async fn a_hunk_press_lands_focus_in_the_text(
        cx: &mut gpui::TestAppContext,
        area: gitturtle_core::ChangeArea,
    ) {
        let (fixture, app, cx, route, _, held) = press_a_hunk(cx, area, false).await;
        let hunk = route.view.read_with(cx, |view, _| view.hunk_focus_handle());
        assert!(
            cx.update(|window, _| hunk.is_focused(window)),
            "{area:?}: the press focuses its button, not {:?}",
            cx.update(|window, cx| window.focused(cx))
        );
        land_write(&app, cx, held).await;
        let staged = staged_patch(&fixture);
        assert_eq!(
            staged.contains("+added one\n+added two\n"),
            area == gitturtle_core::ChangeArea::Unstaged,
            "{area:?}: the hunk's write landed: {staged}"
        );
        assert_eq!(
            app.read_with(cx, |app, _| app.patch_view.clone()),
            Some(route.view.clone()),
            "{area:?}: the view stays, with the tail's hunk"
        );
        assert!(
            cx.update(|window, cx| route.view.read(cx).focus_handle(cx).is_focused(window)),
            "{area:?}: focus goes to the text, not {:?}",
            cx.update(|window, cx| window.focused(cx))
        );
    }

    #[gpui::test]
    async fn a_stage_hunk_press_lands_focus_in_the_text(cx: &mut gpui::TestAppContext) {
        a_hunk_press_lands_focus_in_the_text(cx, gitturtle_core::ChangeArea::Unstaged).await;
    }

    #[gpui::test]
    async fn an_unstage_hunk_press_lands_focus_in_the_text(cx: &mut gpui::TestAppContext) {
        a_hunk_press_lands_focus_in_the_text(cx, gitturtle_core::ChangeArea::Staged).await;
    }

    /// The read of the diff after a Stage hunk's write that the reader
    /// drops takes the view away with focus on that Stage hunk, which goes
    /// to the list, as focus anywhere in the view does: it once fell to the
    /// window frame with the button.
    #[gpui::test]
    async fn a_dropped_read_after_a_stage_hunk_press_focuses_the_list(
        cx: &mut gpui::TestAppContext,
    ) {
        let (_fixture, app, cx, route, _, held) =
            press_a_hunk(cx, gitturtle_core::ChangeArea::Unstaged, false).await;
        let hunk = route.view.read_with(cx, |view, _| view.hunk_focus_handle());
        assert!(
            cx.update(|window, _| hunk.is_focused(window)),
            "the press focuses it"
        );
        let reads = land_the_status_alone(&app, cx, held);
        app.read_with(cx, |app, _| app.worker.cancel());
        drop(reads);
        crate::views::tests::settle(&app, cx).await;
        crate::tags::tests::draw(cx);
        assert!(
            app.read_with(cx, |app, _| app.patch_view.is_none()),
            "the view went with its landing"
        );
        let list = app.read_with(cx, |app, _| app.file_focus.clone());
        assert!(
            cx.update(|window, _| list.is_focused(window)),
            "focus goes to the list, not to {:?}",
            cx.update(|window, cx| window.focused(cx))
        );
    }

    /// A Return held on the Stage hunk its press focused, through that
    /// write, which fails and so keeps the view and focus there, starts no
    /// write as it comes up, as a refused press stays refused for its whole
    /// press. The next press acts, with the hunk scrolled out of view, where
    /// its button stays built for the focus it holds: unbuilt, its focus
    /// fell to the window frame and the press reached nothing.
    #[gpui::test]
    async fn a_return_held_on_a_pressed_stage_hunk_through_its_failed_write_starts_none(
        cx: &mut gpui::TestAppContext,
    ) {
        let (fixture, app, cx, route, row, held) =
            press_a_hunk(cx, gitturtle_core::ChangeArea::Unstaged, true).await;
        let hunk = route.view.read_with(cx, |view, _| view.hunk_focus_handle());
        cx.simulate_keystrokes("enter");
        repeat_key(cx, "enter", Repeats::Held, 2);
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        crate::tags::tests::draw(cx);
        std::fs::remove_file(
            fixture
                .path()
                .join("narrow")
                .join(".git")
                .join("index.lock"),
        )
        .unwrap();
        assert!(
            app.read_with(cx, |app, _| app.operation_error.is_some()),
            "the write failed"
        );
        assert!(
            cx.update(|window, _| hunk.is_focused(window)),
            "focus stays on the Stage hunk the failure keeps, not {:?}",
            cx.update(|window, cx| window.focused(cx))
        );
        repeat_key(cx, "enter", Repeats::Held, 3);
        release_key(cx, "enter");
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            None,
            "the key-up starts no write"
        );
        assert!(staged_patch(&fixture).is_empty(), "nothing staged");
        route.editor.update(cx, |state, cx| {
            let height = state.line_height().expect("the text is laid out");
            state.set_scroll_offset(gpui::point(gpui::px(0.), -height * (row + 20) as f32), cx);
        });
        crate::tags::tests::draw(cx);
        let text = route.editor.read_with(cx, |state, _| state.input_bounds());
        let stage = cx
            .debug_bounds(format!("partial-hunk-{row}").leak())
            .expect("the focused Stage hunk stays built");
        assert!(
            stage.bottom() <= text.top(),
            "its row is out of view: {stage:?} above {text:?}"
        );
        press(cx, "enter");
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        let staged = staged_patch(&fixture);
        assert!(
            staged.contains("+added one\n+added two\n") && staged.contains("+twelve"),
            "the next Return stages the hunk: {staged}"
        );
    }

    /// Once the window looks busy through a partial write, each control it
    /// refuses says why in its tooltip, "Wait for the current Git operation
    /// to finish", as DESIGN.md has a control that is unavailable say why:
    /// the bar's action and Clear, a line's checkbox and Stage hunk. Through
    /// the delay, and once the write lands, they say what they do. Dimmed,
    /// they once said only what they did.
    #[gpui::test]
    async fn a_dimmed_partial_control_says_why_in_its_tooltip(cx: &mut gpui::TestAppContext) {
        use crate::workspace::BUSY_REASON;
        let (_fixture, app, cx, route, held) =
            Route::press(cx, gitturtle_core::ChangeArea::Unstaged, &landing_notes()).await;
        let (hunk, twelve) = route_controls(&app, &route, cx);
        let keys = [
            "apply-partial-selection".to_owned(),
            "clear-partial-selection".to_owned(),
            format!("partial-hunk-{hunk}"),
            format!("partial-line-{twelve}"),
        ];
        let tooltip = |key: &str| {
            crate::views::CONTROL_TOOLTIPS.with(|tooltips| tooltips.borrow().get(key).cloned())
        };
        let tooltips = |cx: &mut gpui::VisualTestContext| {
            crate::tags::tests::draw(cx);
            keys.iter()
                .map(|key| tooltip(key).unwrap_or_else(|| panic!("{key} is drawn")))
                .collect::<Vec<_>>()
        };
        let purposes = [
            "For a replacement, select both the removed and added lines.".to_owned(),
            "Clear line selection".to_owned(),
            "Stage all 60 changed lines in this hunk".to_owned(),
            format!(
                "Select this changed line. {} selects a range.",
                crate::shortcuts::SHIFT_CLICK
            ),
        ];
        assert_eq!(tooltips(cx), purposes, "through the delay, what each does");
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        native_frames(cx);
        assert_eq!(
            tooltips(cx),
            [BUSY_REASON; 4].map(str::to_owned),
            "once the window looks busy, why each is unavailable"
        );
        land_write(&app, cx, held).await;
        let twelve = patch_row(&route.editor, cx, "+twelve");
        assert_eq!(
            tooltip(&format!("partial-line-{twelve}")),
            Some(purposes[3].clone()),
            "landed, each says what it does again"
        );
    }

    /// notes 1 to 40 with note 20 changed, hunk B, and note 35, hunk C,
    /// and, where `a`, hunk A before them: note 5 changed, and note 6 with
    /// it where `a_lines` is 2. Without A, as an edit outside the app
    /// reverting it leaves the file, B and C move up by A's rows.
    fn hunks_abc(a: bool, a_lines: usize) -> String {
        (1..=40)
            .map(|line| match line {
                5 if a => "five\n".to_owned(),
                6 if a && a_lines == 2 => "six\n".to_owned(),
                20 => "twenty\n".to_owned(),
                35 => "thirty-five\n".to_owned(),
                _ => format!("note {line}\n"),
            })
            .collect()
    }

    /// [`landing_notes`] without its first hunk, as an edit outside the app
    /// reverting that hunk leaves it: the tail's hunk heads that hunk's row.
    fn tail_notes() -> String {
        let mut text: String = (1..=40).map(|line| format!("note {line}\n")).collect();
        for line in 0..60 {
            text.push_str(&format!("tail {line}\n"));
        }
        text
    }

    /// Lets the held write run to its failure, the index locked, which
    /// keeps the view, and focus on the Stage hunk pressed, then unlocks it.
    async fn fail_held_write(
        fixture: &tempfile::TempDir,
        app: &gpui::Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
        held: crate::operations::Held,
    ) {
        land_write(app, cx, held).await;
        std::fs::remove_file(
            fixture
                .path()
                .join("narrow")
                .join(".git")
                .join("index.lock"),
        )
        .unwrap();
        assert!(
            app.read_with(cx, |app, _| app.operation_error.is_some()),
            "the write failed"
        );
    }

    /// notes.txt written to `notes` outside the app, and the quiet read the
    /// local watcher asks for landed, as it lands in place in the shown
    /// view (`replace_quiet_preview`).
    fn edit_outside(
        fixture: &tempfile::TempDir,
        app: &gpui::Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
        notes: &str,
    ) {
        std::fs::write(fixture.path().join("narrow").join("notes.txt"), notes).unwrap();
        quiet_read_lands(app, cx, false);
    }

    /// The quiet read the local watcher asks for after a change made outside
    /// the app, to the worktree, and where `git` to the index too, landed.
    fn quiet_read_lands(
        app: &gpui::Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
        git: bool,
    ) {
        // The watcher's subscription is made on the operation thread.
        app.read_with(cx, |app, _| app.operations.drain());
        cx.executor().run_until_parked();
        app.update(cx, |app, _| {
            app.automatic
                .report_local_change(crate::local_refresh::LocalChange {
                    worktree: true,
                    git,
                    ..Default::default()
                })
        });
        for _ in 0..16 {
            cx.executor().run_until_parked();
            if !app.read_with(cx, |app, _| app.automatic.quiet_read_requested()) {
                crate::tags::tests::draw(cx);
                return;
            }
            app.read_with(cx, |app, _| app.worker.wait_idle());
        }
        panic!("the quiet read never landed");
    }

    /// Lets every read the window asked for by itself land, so that none
    /// runs or waits as a test changes the repository outside the app: the
    /// watcher's setup, on the operation thread, and the rescan it asks for
    /// as it lands, which each read of the fixture's setup puts back to
    /// wait and starts again. `settle` awaits neither
    /// (`views::tests::hold_quiet_reads_for_test`), so on a slow host that
    /// rescan still runs on the reader's thread as the test goes on. A
    /// quiet read compares the index its status found with the worktree as
    /// it stands when the read reaches the file, so one that read the
    /// status before a change of several steps and the file during or after
    /// it lands as a diff the repository never held, ahead of the read the
    /// test asks for ([`quiet_read_lands`]). A test's watch reports nothing
    /// by itself: once these have landed, nothing reads until the test asks.
    fn earlier_reads_land(app: &gpui::Entity<GitTurtle>, cx: &mut gpui::VisualTestContext) {
        for _ in 0..16 {
            // The watcher's setup and its checks run on the operation thread.
            app.read_with(cx, |app, _| app.operations.drain());
            cx.executor().run_until_parked();
            if app.read_with(cx, |app, _| {
                !app.automatic.quiet_read_requested() && app.automatic.replies_landed()
            }) {
                return;
            }
            app.read_with(cx, |app, _| app.worker.wait_idle());
        }
        panic!("the window's own reads never landed");
    }

    /// Asserts that `route`'s text holds focus, `when`.
    fn assert_text_focused(route: &Route, cx: &mut gpui::VisualTestContext, when: &str) {
        assert!(
            cx.update(|window, cx| route.view.read(cx).focus_handle(cx).is_focused(window)),
            "{when}: focus goes to the text, not {:?}",
            cx.update(|window, cx| window.focused(cx))
        );
    }

    /// A Stage hunk pressed on B, of hunks A, B and C, fails, which leaves
    /// focus on its button; an edit outside the app reverts A, and its quiet
    /// read lands in the view in place, moving B and C up by A's rows, so C
    /// heads B's row. That read ends the press focus, which goes to the
    /// text, as a landing gives it: `key` then presses nothing, and nothing
    /// is staged. The pressed row kept its focus, so C's Stage hunk took it,
    /// and Return or Space staged C, a hunk never pressed (FU45-A7's
    /// security review).
    async fn a_quiet_read_moving_the_pressed_row_ends_its_focus(
        cx: &mut gpui::TestAppContext,
        key: &str,
    ) {
        let (fixture, app, cx, route, row, held) = press_the_hunk(
            cx,
            gitturtle_core::ChangeArea::Unstaged,
            &hunks_abc(true, 1),
            1,
            true,
        )
        .await;
        fail_held_write(&fixture, &app, cx, held).await;
        let hunk = route.view.read_with(cx, |view, _| view.hunk_focus_handle());
        assert!(
            cx.update(|window, _| hunk.is_focused(window)),
            "the failure keeps focus on B's Stage hunk"
        );
        edit_outside(&fixture, &app, cx, &hunks_abc(false, 1));
        assert_eq!(
            app.read_with(cx, |app, _| app.patch_view.clone()),
            Some(route.view.clone()),
            "the read lands in the view"
        );
        assert_eq!(
            (
                hunk_rows(&app, cx).contains(&row),
                patch_row(&route.editor, cx, "+thirty-five")
            ),
            (true, row + 5),
            "C's hunk heads B's row"
        );
        assert_text_focused(&route, cx, "the read moved the hunks");
        press(cx, key);
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            None,
            "{key} starts no write"
        );
        assert!(staged_patch(&fixture).is_empty(), "{key} stages nothing");
    }

    #[gpui::test]
    async fn a_quiet_read_moving_the_pressed_row_ends_its_focus_for_return(
        cx: &mut gpui::TestAppContext,
    ) {
        a_quiet_read_moving_the_pressed_row_ends_its_focus(cx, "enter").await;
    }

    #[gpui::test]
    async fn a_quiet_read_moving_the_pressed_row_ends_its_focus_for_space(
        cx: &mut gpui::TestAppContext,
    ) {
        a_quiet_read_moving_the_pressed_row_ends_its_focus(cx, "space").await;
    }

    /// As above with A two lines long, so B's row lands on one of C's
    /// context lines, which draws no Stage hunk: focus goes to the text,
    /// and Return stages nothing. It rested on a handle no frame drew.
    #[gpui::test]
    async fn a_quiet_read_moving_the_pressed_row_onto_context_focuses_the_text(
        cx: &mut gpui::TestAppContext,
    ) {
        let (fixture, app, cx, route, row, held) = press_the_hunk(
            cx,
            gitturtle_core::ChangeArea::Unstaged,
            &hunks_abc(true, 2),
            1,
            true,
        )
        .await;
        fail_held_write(&fixture, &app, cx, held).await;
        edit_outside(&fixture, &app, cx, &hunks_abc(false, 2));
        assert_eq!(
            patch_row(&route.editor, cx, "+thirty-five"),
            row + 3,
            "B's row is C's second context line"
        );
        assert_eq!(
            cx.debug_bounds(format!("partial-hunk-{row}").leak()),
            None,
            "a context line draws no Stage hunk"
        );
        assert_text_focused(&route, cx, "the read moved the hunks");
        press(cx, "enter");
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        assert!(staged_patch(&fixture).is_empty(), "Return stages nothing");
    }

    /// The pressed hunk's write fails and an edit outside the app reverts
    /// that hunk; its quiet read lands with the tail's hunk heading the
    /// pressed row. Focus goes to the text, and Return stages nothing, nor
    /// does it once that row is scrolled out of view, where no Stage hunk is
    /// built for it. The tail's Stage hunk took the focus, built out of
    /// view, and Return staged the tail there and in view.
    #[gpui::test]
    async fn a_quiet_read_reverting_the_pressed_hunk_stages_nothing_on_return(
        cx: &mut gpui::TestAppContext,
    ) {
        let (fixture, app, cx, route, row, held) =
            press_a_hunk(cx, gitturtle_core::ChangeArea::Unstaged, true).await;
        fail_held_write(&fixture, &app, cx, held).await;
        edit_outside(&fixture, &app, cx, &tail_notes());
        assert_eq!(hunk_rows(&app, cx), [row], "the tail's hunk heads the row");
        assert_text_focused(&route, cx, "the read took the pressed hunk away");
        press(cx, "enter");
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        assert!(staged_patch(&fixture).is_empty(), "Return stages nothing");
        route.editor.update(cx, |state, cx| {
            let height = state.line_height().expect("the text is laid out");
            state.set_scroll_offset(gpui::point(gpui::px(0.), -height * (row + 20) as f32), cx);
        });
        crate::tags::tests::draw(cx);
        assert_eq!(
            cx.debug_bounds(format!("partial-hunk-{row}").leak()),
            None,
            "out of view, no Stage hunk is built for the row"
        );
        press(cx, "enter");
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        assert!(
            staged_patch(&fixture).is_empty(),
            "Return out of view stages nothing"
        );
    }

    /// A Return pressed on the Stage hunk a failed write left focused, held
    /// while an edit outside the app reverts that hunk and its quiet read
    /// lands, does nothing as it comes up: its press began before the read.
    /// It came up on the tail's Stage hunk, which then held the focus, and
    /// staged the tail.
    #[gpui::test]
    async fn a_return_held_across_a_quiet_read_does_nothing_as_it_comes_up(
        cx: &mut gpui::TestAppContext,
    ) {
        let (fixture, app, cx, route, _, held) =
            press_a_hunk(cx, gitturtle_core::ChangeArea::Unstaged, true).await;
        fail_held_write(&fixture, &app, cx, held).await;
        cx.simulate_keystrokes("enter");
        edit_outside(&fixture, &app, cx, &tail_notes());
        assert_text_focused(&route, cx, "the read came while Return was down");
        release_key(cx, "enter");
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            None,
            "the key-up starts no write"
        );
        assert!(staged_patch(&fixture).is_empty(), "nothing staged");
    }

    /// notes 1 to 40 with each of hunks A (note 5), B (note 14), C (note 23)
    /// and D (note 32) that `hunks` names changed: each hunk takes nine
    /// rows, so reverting or staging one moves those after it up by nine,
    /// the next hunk's header onto its row.
    fn hunks_named(hunks: &str) -> String {
        (1..=40)
            .map(
                |line| match HUNKS.iter().find(|(_, note, _)| *note == line) {
                    Some((name, _, text)) if hunks.contains(*name) => format!("{text}\n"),
                    _ => format!("note {line}\n"),
                },
            )
            .collect()
    }

    /// [`hunks_named`]'s hunks: each one's name, the note it changes and
    /// the text it changes it to.
    const HUNKS: [(char, usize, &str); 4] = [
        ('A', 5, "five"),
        ('B', 14, "fourteen"),
        ('C', 23, "twenty-three"),
        ('D', 32, "thirty-two"),
    ];

    /// Which of [`hunks_named`]'s hunks the fixture's index holds staged,
    /// as "AC".
    fn staged_hunks(fixture: &tempfile::TempDir) -> String {
        let staged = staged_patch(fixture);
        HUNKS
            .iter()
            .filter(|(_, _, text)| staged.contains(&format!("+{text}\n")))
            .map(|(name, _, _)| *name)
            .collect()
    }

    /// notes.txt written to `notes` outside the app, and staged there too
    /// in the staged `area`, as `git add` in a terminal does, and the quiet
    /// read of it landed in the shown view ([`edit_outside`]).
    fn change_outside(
        fixture: &tempfile::TempDir,
        app: &gpui::Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
        area: gitturtle_core::ChangeArea,
        notes: &str,
    ) {
        let root = fixture.path().join("narrow");
        std::fs::write(root.join("notes.txt"), notes).unwrap();
        let staged = area == gitturtle_core::ChangeArea::Staged;
        if staged {
            crate::views::tests::fixture_git(&root, &["add", "notes.txt"]);
        }
        quiet_read_lands(app, cx, staged);
    }

    /// Where the window draws the Stage hunk (Unstage hunk) of the hunk
    /// header on `row`.
    fn drawn_stage_hunk(
        cx: &mut gpui::VisualTestContext,
        row: usize,
    ) -> gpui::Bounds<gpui::Pixels> {
        cx.debug_bounds(format!("partial-hunk-{row}").leak())
            .unwrap_or_else(|| panic!("row {row} draws its Stage hunk"))
    }

    /// The pointer's button goes down at `at`, and the window draws the
    /// frame that asks for.
    fn pointer_down(cx: &mut gpui::VisualTestContext, at: gpui::Point<gpui::Pixels>) {
        cx.simulate_mouse_down(at, gpui::MouseButton::Left, gpui::Modifiers::default());
        crate::tags::tests::draw(cx);
    }

    /// The pointer's button comes up at `at`, with the operations executor
    /// held across it so that a write it starts is still waiting as it is
    /// read ([`press_drawn_stage`]): returns that write's label, if it
    /// started one, once it has run and its reads have landed.
    async fn pointer_up(
        app: &gpui::Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
        at: gpui::Point<gpui::Pixels>,
    ) -> Option<&'static str> {
        let held = app.read_with(cx, |app, _| app.operations.hold());
        cx.simulate_mouse_up(at, gpui::MouseButton::Left, gpui::Modifiers::default());
        let started = app.read_with(cx, |app, _| app.operation_busy);
        land_write(app, cx, held).await;
        started
    }

    /// The pointer goes down on the Stage hunk (Unstage hunk in `area`) of
    /// B, of hunks A, B and C, and comes up there only after a change made
    /// outside the app left `left` of them and its quiet read landed in the
    /// view: another diff in the pressed one's place. That read ends the
    /// pointer's press, as it ends a key's, so the button coming up starts
    /// no write, whichever hunk's button the read drew under it: C's where A
    /// or B went, B's own where C went. A complete click after it acts on
    /// the hunk it presses. GPUI keeps a button's pending press by its
    /// element, which is the button's row, so the press outlived the read
    /// and its release wrote the hunk then on that row: C, which nobody
    /// pressed (FU45-A8's security re-check, F1).
    async fn a_pointer_press_ends_with_a_read_of_another_diff(
        cx: &mut gpui::TestAppContext,
        area: gitturtle_core::ChangeArea,
        left: &str,
    ) {
        let (fixture, app, cx, route) = hunks_shown(cx, area, &hunks_named("ABC")).await;
        let row = hunk_rows(&app, cx)[1];
        let stage = drawn_stage_hunk(cx, row);
        let at = stage.center();
        pointer_down(cx, at);
        change_outside(&fixture, &app, cx, area, &hunks_named(left));
        assert_eq!(
            app.read_with(cx, |app, _| app.patch_view.clone()),
            Some(route.view.clone()),
            "{area:?} {left}: the read lands in the view"
        );
        assert_eq!(
            (hunk_rows(&app, cx).len(), drawn_stage_hunk(cx, row)),
            (2, stage),
            "{area:?} {left}: a hunk's button stands under the pointer still"
        );
        let unpressed = staged_hunks(&fixture);
        assert_eq!(
            (pointer_up(&app, cx, at).await, staged_hunks(&fixture)),
            (None, unpressed.clone()),
            "{area:?} {left}: the press ended with the read, and its release writes nothing"
        );
        let list = app.read_with(cx, |app, _| app.file_focus.clone());
        assert!(
            cx.update(|window, _| list.is_focused(window)),
            "{area:?} {left}: focus stays on the list, not {:?}",
            cx.update(|window, cx| window.focused(cx))
        );
        // The hunk that heads the pressed row now, which a fresh click
        // presses: staged from the unstaged area, unstaged from the staged.
        let under = &left[1..];
        let written = if area == gitturtle_core::ChangeArea::Staged {
            unpressed.replace(under, "")
        } else {
            under.to_owned()
        };
        pointer_down(cx, at);
        assert!(
            pointer_up(&app, cx, at).await.is_some(),
            "{area:?} {left}: a fresh click starts its write"
        );
        assert_eq!(
            staged_hunks(&fixture),
            written,
            "{area:?} {left}: and writes the hunk it pressed, {under}"
        );
    }

    #[gpui::test]
    async fn a_pointer_press_on_stage_hunk_ends_as_a_read_moves_another_hunk_under_it(
        cx: &mut gpui::TestAppContext,
    ) {
        let area = gitturtle_core::ChangeArea::Unstaged;
        a_pointer_press_ends_with_a_read_of_another_diff(cx, area, "BC").await;
    }

    #[gpui::test]
    async fn a_pointer_press_on_stage_hunk_ends_as_a_read_reverts_its_hunk(
        cx: &mut gpui::TestAppContext,
    ) {
        let area = gitturtle_core::ChangeArea::Unstaged;
        a_pointer_press_ends_with_a_read_of_another_diff(cx, area, "AC").await;
    }

    #[gpui::test]
    async fn a_pointer_press_on_stage_hunk_ends_as_a_read_reverts_a_later_hunk(
        cx: &mut gpui::TestAppContext,
    ) {
        let area = gitturtle_core::ChangeArea::Unstaged;
        a_pointer_press_ends_with_a_read_of_another_diff(cx, area, "AB").await;
    }

    #[gpui::test]
    async fn a_pointer_press_on_unstage_hunk_ends_as_a_read_moves_another_hunk_under_it(
        cx: &mut gpui::TestAppContext,
    ) {
        let area = gitturtle_core::ChangeArea::Staged;
        a_pointer_press_ends_with_a_read_of_another_diff(cx, area, "BC").await;
    }

    #[gpui::test]
    async fn a_pointer_press_on_unstage_hunk_ends_as_a_read_reverts_its_hunk(
        cx: &mut gpui::TestAppContext,
    ) {
        let area = gitturtle_core::ChangeArea::Staged;
        a_pointer_press_ends_with_a_read_of_another_diff(cx, area, "AC").await;
    }

    /// The pointer goes down on B's Stage hunk, leaves the button for the
    /// text with its button held, and comes back before it comes up: the
    /// press is the one that went down, as on any button, so a read of
    /// another diff meanwhile ends it and its release writes nothing, and
    /// with no read between, as on the click after, it writes the hunk it
    /// went down on.
    #[gpui::test]
    async fn a_pointer_press_that_leaves_stage_hunk_and_comes_back_is_the_same_press(
        cx: &mut gpui::TestAppContext,
    ) {
        let area = gitturtle_core::ChangeArea::Unstaged;
        let (fixture, app, cx, _route) = hunks_shown(cx, area, &hunks_named("ABC")).await;
        let row = hunk_rows(&app, cx)[1];
        let stage = drawn_stage_hunk(cx, row);
        let (at, away) = (
            stage.center(),
            gpui::point(
                stage.right() + gpui::px(240.),
                stage.bottom() + gpui::px(60.),
            ),
        );
        let leave_and_return = |cx: &mut gpui::VisualTestContext| {
            let held = Some(gpui::MouseButton::Left);
            cx.simulate_mouse_move(away, held, gpui::Modifiers::default());
            crate::tags::tests::draw(cx);
            cx.simulate_mouse_move(at, held, gpui::Modifiers::default());
            crate::tags::tests::draw(cx);
        };
        pointer_down(cx, at);
        cx.simulate_mouse_move(
            away,
            Some(gpui::MouseButton::Left),
            gpui::Modifiers::default(),
        );
        change_outside(&fixture, &app, cx, area, &hunks_named("BC"));
        assert_eq!(
            drawn_stage_hunk(cx, row),
            stage,
            "C's button takes B's place"
        );
        cx.simulate_mouse_move(
            at,
            Some(gpui::MouseButton::Left),
            gpui::Modifiers::default(),
        );
        crate::tags::tests::draw(cx);
        assert_eq!(
            (pointer_up(&app, cx, at).await, staged_hunks(&fixture)),
            (None, String::new()),
            "back on the button after the read, the release writes nothing"
        );
        pointer_down(cx, at);
        leave_and_return(cx);
        assert!(
            pointer_up(&app, cx, at).await.is_some(),
            "a press that left and came back with no read between starts its write"
        );
        assert_eq!(staged_hunks(&fixture), "C", "of the hunk it went down on");
    }

    /// The pointer goes down on B's Stage hunk and comes up there after a
    /// change made outside the app, to a line far from every hunk, in the
    /// index and the worktree alike, and its quiet read landed: a read of
    /// the same diff, hunk for hunk, in other text. It keeps the press, as
    /// it keeps a key's, so the release stages B, the hunk pressed, alone.
    #[gpui::test]
    async fn a_pointer_press_on_stage_hunk_outlasts_a_read_of_the_same_diff(
        cx: &mut gpui::TestAppContext,
    ) {
        let area = gitturtle_core::ChangeArea::Unstaged;
        let (fixture, app, cx, route) = hunks_shown(cx, area, &hunks_named("ABC")).await;
        let row = hunk_rows(&app, cx)[1];
        let stage = drawn_stage_hunk(cx, row);
        pointer_down(cx, stage.center());
        // Earlier reads land first, so none reads this three-step change
        // part made, or the index from before it against the worktree after
        // it: another diff, which ends the press before the read asked for
        // below shows the same one again.
        earlier_reads_land(&app, cx);
        let root = fixture.path().join("narrow");
        let far = |notes: String| notes.replace("note 40\n", "forty\n");
        std::fs::write(root.join("notes.txt"), far(hunks_named(""))).unwrap();
        crate::views::tests::fixture_git(&root, &["add", "notes.txt"]);
        std::fs::write(root.join("notes.txt"), far(hunks_named("ABC"))).unwrap();
        quiet_read_lands(&app, cx, true);
        let read = app.read_with(cx, |app, _| match app.content.as_deref() {
            Some(Content::Text { new, .. }) => new.contains("forty\n"),
            _ => false,
        });
        assert_eq!(
            (
                read,
                app.read_with(cx, |app, _| app.patch_view.clone()),
                hunk_rows(&app, cx).len(),
                drawn_stage_hunk(cx, row),
                staged_hunks(&fixture)
            ),
            (true, Some(route.view.clone()), 3, stage, String::new()),
            "the read landed in the view with the same hunks on the same rows"
        );
        assert!(
            pointer_up(&app, cx, stage.center()).await.is_some(),
            "the press outlasts that read, and its release starts a write"
        );
        assert_eq!(staged_hunks(&fixture), "B", "of B alone");
    }

    /// The pointer goes down on B's Stage hunk while a write of A, pressed
    /// just before, still runs, and comes up after that write has landed:
    /// its landing changed the text, B's hunk moving up to A's row and C's
    /// button onto the pressed row, and ended the press, so the release
    /// stages nothing more. The window says the write's result in a row
    /// above the text, which draws the rows lower, so the pointer, its
    /// button still held, follows the pressed row's button down before it
    /// comes up. A click after it stages C, which it presses.
    #[gpui::test]
    async fn a_pointer_press_on_stage_hunk_ends_as_another_write_lands(
        cx: &mut gpui::TestAppContext,
    ) {
        let (fixture, app, cx, route, _, held) = press_the_hunk(
            cx,
            gitturtle_core::ChangeArea::Unstaged,
            &hunks_named("ABC"),
            0,
            false,
        )
        .await;
        let row = hunk_rows(&app, cx)[1];
        let pressed = drawn_stage_hunk(cx, row);
        pointer_down(cx, pressed.center());
        land_write(&app, cx, held).await;
        let stage = drawn_stage_hunk(cx, row);
        assert_eq!(
            (
                app.read_with(cx, |app, _| app.patch_view.clone()),
                staged_hunks(&fixture),
                patch_row(&route.editor, cx, "+twenty-three"),
                stage.origin.x,
                stage.top() > pressed.top()
            ),
            (
                Some(route.view.clone()),
                "A".to_owned(),
                row + 5,
                pressed.origin.x,
                true
            ),
            "A's write landed in the view: C's button heads the pressed row, drawn lower"
        );
        cx.simulate_mouse_move(
            stage.center(),
            Some(gpui::MouseButton::Left),
            gpui::Modifiers::default(),
        );
        crate::tags::tests::draw(cx);
        assert_eq!(
            (
                pointer_up(&app, cx, stage.center()).await,
                staged_hunks(&fixture)
            ),
            (None, "A".to_owned()),
            "the press ended with the landing, and its release writes nothing"
        );
        pointer_down(cx, stage.center());
        assert!(
            pointer_up(&app, cx, stage.center()).await.is_some(),
            "a fresh click starts its write"
        );
        assert_eq!(staged_hunks(&fixture), "AC", "of C, which it pressed");
    }

    /// The pointer goes down on the checkbox of B's removed line and comes
    /// up there after a change made outside the app reverted A and its
    /// quiet read landed: C's removed line then stands under it, with the
    /// same place among the changed lines, which names its checkbox. That
    /// read ends the press, so the release selects nothing: it selected C's
    /// line, which nobody pressed, for the bar's action to stage. A click
    /// after it selects the line it presses.
    #[gpui::test]
    async fn a_pointer_press_on_a_line_checkbox_ends_with_a_read_of_another_diff(
        cx: &mut gpui::TestAppContext,
    ) {
        let area = gitturtle_core::ChangeArea::Unstaged;
        let (fixture, app, cx, route) = hunks_shown(cx, area, &hunks_named("ABC")).await;
        let row = patch_row(&route.editor, cx, "-note 14");
        let selector: &'static str = format!("partial-line-{row}").leak();
        let checkbox = cx.debug_bounds(selector).expect("the line's checkbox");
        pointer_down(cx, checkbox.center());
        change_outside(&fixture, &app, cx, area, &hunks_named("BC"));
        assert_eq!(
            (
                patch_row(&route.editor, cx, "-note 23"),
                cx.debug_bounds(selector)
            ),
            (row, Some(checkbox)),
            "C's removed line takes the pressed line's row and checkbox"
        );
        assert_eq!(
            (
                pointer_up(&app, cx, checkbox.center()).await,
                route.view.read_with(cx, |view, _| view.selected_lines()),
                cx.debug_bounds("apply-partial-selection")
            ),
            (None, 0, None),
            "the press ended with the read, and its release selects no line"
        );
        cx.simulate_click(checkbox.center(), gpui::Modifiers::default());
        crate::tags::tests::draw(cx);
        assert_eq!(
            route.view.read_with(cx, |view, _| view.selected_lines()),
            1,
            "a fresh click selects the line it presses"
        );
    }

    /// The diff whose partial actions the window's content holds.
    fn partial_held(app: &GitTurtle) -> Option<Arc<crate::partial_view::PartialActions>> {
        match app.content.as_deref() {
            Some(Content::Text { partial, .. }) => partial.clone(),
            _ => None,
        }
    }

    /// The pointer's left button going down at `at` and coming up there,
    /// each counted `count` by the platform, as it counts the presses of a
    /// multi-click made within its double-click time and distance:
    /// `simulate_click` always counts one.
    fn click_events(at: gpui::Point<gpui::Pixels>, count: usize) -> [gpui::PlatformInput; 2] {
        let (button, modifiers) = (gpui::MouseButton::Left, gpui::Modifiers::default());
        [
            gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                button,
                position: at,
                modifiers,
                click_count: count,
                first_mouse: false,
            }),
            gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
                button,
                position: at,
                modifiers,
                click_count: count,
            }),
        ]
    }

    /// A change made outside the app leaves notes.txt as `notes`, staged
    /// there too in the staged `area` ([`change_outside`]), and `events`
    /// reach the window as its quiet read lands in the view, before the
    /// window draws the frame that shows it: the window still shows the
    /// frame drawn before the read, whose controls take them, while the
    /// view already holds the diff read. They are dispatched as the update
    /// the read lands in is flushed, on the app's first notification with
    /// the diff read, and the window draws only after them.
    fn as_the_read_lands(
        fixture: &tempfile::TempDir,
        app: &gpui::Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
        area: gitturtle_core::ChangeArea,
        notes: &str,
        events: Vec<gpui::PlatformInput>,
    ) {
        use std::{cell::Cell, rc::Rc};
        let before = app.read_with(cx, |app, _| partial_held(app));
        let window = cx.update(|window, _| window.window_handle());
        let sent = Rc::new(Cell::new(false));
        let subscription = cx.update(|_, cx| {
            let (sent, mut events) = (sent.clone(), Some(events));
            cx.observe(app, move |app, cx| {
                let read = match (&before, partial_held(app.read(cx))) {
                    (Some(before), Some(now)) => !Arc::ptr_eq(before, &now),
                    (None, None) => false,
                    _ => true,
                };
                let Some(events) = events.take_if(|_| read) else {
                    return;
                };
                window
                    .update(cx, |_, window, cx| {
                        for event in events {
                            window.dispatch_event(event, cx);
                        }
                    })
                    .expect("the window takes the events");
                sent.set(true);
            })
        });
        change_outside(fixture, app, cx, area, notes);
        drop(subscription);
        assert!(sent.get(), "the events arrived as the read landed");
    }

    /// The window shows hunks A, B and C with B's Stage hunk (Unstage hunk
    /// in `area`) under the pointer, a change made outside the app reverts
    /// A, and its quiet read lands: the view holds B and C, C's hunk on the
    /// row B's button was drawn on, while the window still shows the frame
    /// drawn before the read. Before the next frame the pointer clicks
    /// there, down and up, as a tap or an assistive technology's click
    /// does, or where `held` its button goes down there and comes up after
    /// that frame, as a hand's does. Either is a press on B's button, which
    /// belongs to a diff the view no longer shows, so nothing is written:
    /// the click applied B's lines' places among the changed lines to the
    /// diff read, where they are C's, and wrote C; the press was recorded
    /// on the row, which the next frame drew C's button on, and its release
    /// wrote C (PTR-B's security re-check, G1). A click once the frame is
    /// drawn acts on the hunk it presses.
    async fn a_press_before_the_frame_of_a_read_writes_nothing(
        cx: &mut gpui::TestAppContext,
        area: gitturtle_core::ChangeArea,
        held: bool,
    ) {
        let (fixture, app, cx, route) = hunks_shown(cx, area, &hunks_named("ABC")).await;
        let row = hunk_rows(&app, cx)[1];
        let stage = drawn_stage_hunk(cx, row);
        let at = stage.center();
        let [down, up] = click_events(at, 1);
        let events = if held { vec![down] } else { vec![down, up] };
        as_the_read_lands(&fixture, &app, cx, area, &hunks_named("BC"), events);
        let staged = area == gitturtle_core::ChangeArea::Staged;
        let unpressed = if staged { "BC" } else { "" };
        // A write the click started has run before the index is read.
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        crate::tags::tests::draw(cx);
        assert_eq!(
            staged_hunks(&fixture),
            unpressed,
            "{area:?}: a press on a frame of another diff writes nothing"
        );
        assert_eq!(
            (
                app.read_with(cx, |app, _| app.patch_view.clone()),
                hunk_rows(&app, cx).len(),
                drawn_stage_hunk(cx, row)
            ),
            (Some(route.view.clone()), 2, stage),
            "{area:?}: the read landed in the view, and the frame after it draws C's button there"
        );
        if held {
            assert_eq!(
                (pointer_up(&app, cx, at).await, staged_hunks(&fixture)),
                (None, unpressed.to_owned()),
                "{area:?}: the press went down on a frame of another diff, and its release writes nothing"
            );
        }
        pointer_down(cx, at);
        assert!(
            pointer_up(&app, cx, at).await.is_some(),
            "{area:?}: a click on the frame drawn after the read starts its write"
        );
        assert_eq!(
            staged_hunks(&fixture),
            if staged { "B" } else { "C" },
            "{area:?}: of C, which it presses"
        );
    }

    #[gpui::test]
    async fn a_click_on_stage_hunk_before_the_frame_of_a_read_stages_nothing(
        cx: &mut gpui::TestAppContext,
    ) {
        let area = gitturtle_core::ChangeArea::Unstaged;
        a_press_before_the_frame_of_a_read_writes_nothing(cx, area, false).await;
    }

    #[gpui::test]
    async fn a_press_on_stage_hunk_before_the_frame_of_a_read_stages_nothing(
        cx: &mut gpui::TestAppContext,
    ) {
        let area = gitturtle_core::ChangeArea::Unstaged;
        a_press_before_the_frame_of_a_read_writes_nothing(cx, area, true).await;
    }

    #[gpui::test]
    async fn a_click_on_unstage_hunk_before_the_frame_of_a_read_unstages_nothing(
        cx: &mut gpui::TestAppContext,
    ) {
        let area = gitturtle_core::ChangeArea::Staged;
        a_press_before_the_frame_of_a_read_writes_nothing(cx, area, false).await;
    }

    #[gpui::test]
    async fn a_press_on_unstage_hunk_before_the_frame_of_a_read_unstages_nothing(
        cx: &mut gpui::TestAppContext,
    ) {
        let area = gitturtle_core::ChangeArea::Staged;
        a_press_before_the_frame_of_a_read_writes_nothing(cx, area, true).await;
    }

    /// As above with the change outside the app also growing B by a second
    /// replaced line: the click before the frame stages no part of a hunk.
    /// B's two lines' places among the changed lines of the diff drawn are
    /// its two added lines' in the diff read, and the click staged
    /// "+fourteen" and "+fifteen" without the lines they replace.
    #[gpui::test]
    async fn a_click_on_stage_hunk_before_the_frame_of_a_read_stages_no_part_of_a_hunk(
        cx: &mut gpui::TestAppContext,
    ) {
        let area = gitturtle_core::ChangeArea::Unstaged;
        let (fixture, app, cx, route) = hunks_shown(cx, area, &hunks_named("ABC")).await;
        let row = hunk_rows(&app, cx)[1];
        let at = drawn_stage_hunk(cx, row).center();
        let grown = hunks_named("BC").replace("note 15\n", "fifteen\n");
        as_the_read_lands(&fixture, &app, cx, area, &grown, click_events(at, 1).into());
        assert_eq!(
            (
                app.read_with(cx, |app, _| app.patch_view.clone()),
                patch_row(&route.editor, cx, "+fifteen") > 0
            ),
            (Some(route.view.clone()), true),
            "the read landed in the view with B grown"
        );
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(
            staged_patch(&fixture),
            "",
            "the click, on a frame of another diff, stages nothing"
        );
    }

    /// The checkbox of B's removed line stands under the pointer, a change
    /// made outside the app reverts A, and its quiet read lands, C's
    /// removed line taking that line's row and place among the changed
    /// lines. Before the next frame the pointer clicks there, or where
    /// `held` goes down there and comes up after that frame: a press on a
    /// checkbox of a diff the view no longer shows, which selects nothing.
    /// It selected C's removed line, for the bar's action to stage. A click
    /// once the frame is drawn selects the line it presses.
    async fn a_press_on_a_line_checkbox_before_the_frame_of_a_read_selects_nothing(
        cx: &mut gpui::TestAppContext,
        held: bool,
    ) {
        let area = gitturtle_core::ChangeArea::Unstaged;
        let (fixture, app, cx, route) = hunks_shown(cx, area, &hunks_named("ABC")).await;
        let row = patch_row(&route.editor, cx, "-note 14");
        let selector: &'static str = format!("partial-line-{row}").leak();
        let checkbox = cx.debug_bounds(selector).expect("the line's checkbox");
        let at = checkbox.center();
        let [down, up] = click_events(at, 1);
        let events = if held { vec![down] } else { vec![down, up] };
        as_the_read_lands(&fixture, &app, cx, area, &hunks_named("BC"), events);
        assert_eq!(
            (
                patch_row(&route.editor, cx, "-note 23"),
                cx.debug_bounds(selector)
            ),
            (row, Some(checkbox)),
            "C's removed line takes the pressed line's row and checkbox"
        );
        if held {
            assert_eq!(pointer_up(&app, cx, at).await, None, "no write");
        }
        assert_eq!(
            (
                route.view.read_with(cx, |view, _| view.selected_lines()),
                cx.debug_bounds("apply-partial-selection")
            ),
            (0, None),
            "a press on a frame of another diff selects no line"
        );
        cx.simulate_click(at, gpui::Modifiers::default());
        crate::tags::tests::draw(cx);
        assert_eq!(
            route.view.read_with(cx, |view, _| view.selected_lines()),
            1,
            "a click on the frame drawn after the read selects the line it presses"
        );
    }

    #[gpui::test]
    async fn a_click_on_a_line_checkbox_before_the_frame_of_a_read_selects_nothing(
        cx: &mut gpui::TestAppContext,
    ) {
        a_press_on_a_line_checkbox_before_the_frame_of_a_read_selects_nothing(cx, false).await;
    }

    #[gpui::test]
    async fn a_press_held_on_a_line_checkbox_before_the_frame_of_a_read_selects_nothing(
        cx: &mut gpui::TestAppContext,
    ) {
        a_press_on_a_line_checkbox_before_the_frame_of_a_read_selects_nothing(cx, true).await;
    }

    /// The pointer's left button goes down at `at` and comes up there, each
    /// counted `count` by the platform ([`click_events`]), the window
    /// drawing between them, with the operations executor held across the
    /// release ([`pointer_up`]): returns the label of the write it started,
    /// if it started one, once that has run and its reads have landed.
    async fn counted_click(
        app: &gpui::Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
        at: gpui::Point<gpui::Pixels>,
        count: usize,
    ) -> Option<&'static str> {
        let [down, up] = click_events(at, count);
        cx.update(|window, cx| {
            window.dispatch_event(down, cx);
        });
        crate::tags::tests::draw(cx);
        let held = app.read_with(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            window.dispatch_event(up, cx);
        });
        cx.run_until_parked();
        let started = app.read_with(cx, |app, _| app.operation_busy);
        land_write(app, cx, held).await;
        started
    }

    /// A double-click on B's Stage hunk (Unstage hunk in `area`), of hunks
    /// A, B, C and D, after a click wrote A, whose result the window says
    /// in a row above the text that stays through the next write. The first
    /// press writes B, and that write and the reads after it land before
    /// the second: C's hunk then heads the pressed row, its button under
    /// the still pointer, as the second press arrives, counted 2 by the
    /// platform. Another diff took the place of the one the double-click
    /// began on, so that press writes nothing, nor does a third: it wrote
    /// C, which nobody aimed at (PTR-B's security re-check, F2). Focus
    /// stays in the text, where B's landing left it. A click counted 1, as
    /// one made after the double-click time is, writes C, the hunk its
    /// button names.
    async fn a_double_click_writes_its_hunk_once(
        cx: &mut gpui::TestAppContext,
        area: gitturtle_core::ChangeArea,
    ) {
        let (fixture, app, cx, route) = hunks_shown(cx, area, &hunks_named("ABCD")).await;
        // What the index holds staged once `written` have been pressed.
        let staged = area == gitturtle_core::ChangeArea::Staged;
        let after = |written: &str| -> String {
            "ABCD"
                .chars()
                .filter(|hunk| written.contains(*hunk) != staged)
                .collect()
        };
        let row = hunk_rows(&app, cx)[0];
        let first = drawn_stage_hunk(cx, row).center();
        assert!(
            counted_click(&app, cx, first, 1).await.is_some(),
            "{area:?}: a click on A starts its write"
        );
        let stage = drawn_stage_hunk(cx, row);
        let at = stage.center();
        assert_eq!(staged_hunks(&fixture), after("A"), "{area:?}: A is written");
        assert!(
            counted_click(&app, cx, at, 1).await.is_some(),
            "{area:?}: the double-click's first press starts B's write"
        );
        assert_eq!(
            (
                app.read_with(cx, |app, _| app.patch_view.clone()),
                staged_hunks(&fixture),
                hunk_rows(&app, cx).len(),
                drawn_stage_hunk(cx, row)
            ),
            (Some(route.view.clone()), after("AB"), 2, stage),
            "{area:?}: B's write landed in the view, C's button under the still pointer"
        );
        assert_text_focused(&route, cx, "B's hunk landed away");
        for count in [2, 3] {
            assert_eq!(
                (
                    counted_click(&app, cx, at, count).await,
                    staged_hunks(&fixture)
                ),
                (None, after("AB")),
                "{area:?}: press {count} of the double-click writes nothing"
            );
            assert_text_focused(&route, cx, "a press that does nothing");
        }
        assert!(
            counted_click(&app, cx, at, 1).await.is_some(),
            "{area:?}: a click of its own starts its write"
        );
        assert_eq!(
            staged_hunks(&fixture),
            after("ABC"),
            "{area:?}: of C, which its button names"
        );
    }

    #[gpui::test]
    async fn a_double_click_on_stage_hunk_stages_its_hunk_once(cx: &mut gpui::TestAppContext) {
        a_double_click_writes_its_hunk_once(cx, gitturtle_core::ChangeArea::Unstaged).await;
    }

    #[gpui::test]
    async fn a_double_click_on_unstage_hunk_unstages_its_hunk_once(cx: &mut gpui::TestAppContext) {
        a_double_click_writes_its_hunk_once(cx, gitturtle_core::ChangeArea::Staged).await;
    }

    /// A double-click on B's Stage hunk whose first press starts a write
    /// that fails, the index locked as it was for the click before it,
    /// whose failure the window already says in a row above the text: the
    /// failure reads the same diff back, B's button under the still pointer
    /// names B still, and the second press, counted 2, stages it, as it did
    /// before the rule. Only another diff in the first press's place ends a
    /// double-click.
    #[gpui::test]
    async fn a_double_click_whose_first_write_failed_stages_its_hunk_on_the_second_press(
        cx: &mut gpui::TestAppContext,
    ) {
        let area = gitturtle_core::ChangeArea::Unstaged;
        let (fixture, app, cx, route) = hunks_shown(cx, area, &hunks_named("ABC")).await;
        let row = hunk_rows(&app, cx)[1];
        let lock = fixture
            .path()
            .join("narrow")
            .join(".git")
            .join("index.lock");
        std::fs::write(&lock, "").unwrap();
        let first = drawn_stage_hunk(cx, row).center();
        assert!(
            counted_click(&app, cx, first, 1).await.is_some(),
            "a click on B starts a write"
        );
        let stage = drawn_stage_hunk(cx, row);
        let at = stage.center();
        assert!(
            counted_click(&app, cx, at, 1).await.is_some(),
            "the double-click's first press starts a write"
        );
        std::fs::remove_file(&lock).unwrap();
        assert_eq!(
            (
                app.read_with(cx, |app, _| app.operation_error.is_some()),
                app.read_with(cx, |app, _| app.patch_view.clone()),
                staged_hunks(&fixture),
                hunk_rows(&app, cx).len(),
                drawn_stage_hunk(cx, row)
            ),
            (true, Some(route.view.clone()), String::new(), 3, stage),
            "that write failed too: the same diff, B's button under the still pointer"
        );
        assert!(
            counted_click(&app, cx, at, 2).await.is_some(),
            "the second press starts a write"
        );
        assert_eq!(
            staged_hunks(&fixture),
            "B",
            "of B, which both presses named"
        );
    }

    /// A double-click on B's Stage hunk whose first press arrives before
    /// the frame of a read that reverted A
    /// ([`a_press_before_the_frame_of_a_read_writes_nothing`]): that press
    /// went down on B's button, of a diff the view no longer shows, and the
    /// second, counted 2, on C's, which the frame after drew there. Neither
    /// stages anything: the diff shown changed since the double-click's
    /// first press, which the frame it went down on dates, not the view.
    #[gpui::test]
    async fn a_double_click_begun_before_the_frame_of_a_read_stages_nothing(
        cx: &mut gpui::TestAppContext,
    ) {
        let area = gitturtle_core::ChangeArea::Unstaged;
        let (fixture, app, cx, _route) = hunks_shown(cx, area, &hunks_named("ABC")).await;
        let row = hunk_rows(&app, cx)[1];
        let stage = drawn_stage_hunk(cx, row);
        let at = stage.center();
        let first = click_events(at, 1).into();
        as_the_read_lands(&fixture, &app, cx, area, &hunks_named("BC"), first);
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        crate::tags::tests::draw(cx);
        assert_eq!(
            (staged_hunks(&fixture), drawn_stage_hunk(cx, row)),
            (String::new(), stage),
            "the first press stages nothing, and the frame after draws C's button there"
        );
        assert_eq!(
            (counted_click(&app, cx, at, 2).await, staged_hunks(&fixture)),
            (None, String::new()),
            "the second press stages nothing"
        );
        assert!(
            counted_click(&app, cx, at, 1).await.is_some(),
            "a click of its own starts its write"
        );
        assert_eq!(staged_hunks(&fixture), "C", "of C, which its button names");
    }

    /// A double-click on the Stage hunk of the last unstaged hunk, B, after
    /// a click staged A: the first press stages B, the file leaves Unstaged
    /// and the selection follows it to Staged, whose diff a view built for
    /// it shows, A's Unstage hunk where B's Stage hunk stood, under the
    /// still pointer. The second press, counted 2, unstages nothing: that
    /// view saw no first press of this double-click, which began on another
    /// diff. It unstaged A, which nobody aimed at. A click of its own
    /// unstages A.
    #[gpui::test]
    async fn a_double_click_on_the_last_hunks_stage_hunk_unstages_nothing(
        cx: &mut gpui::TestAppContext,
    ) {
        let area = gitturtle_core::ChangeArea::Unstaged;
        let (fixture, app, cx, route) = hunks_shown(cx, area, &hunks_named("AB")).await;
        let row = hunk_rows(&app, cx)[0];
        let first = drawn_stage_hunk(cx, row).center();
        assert!(
            counted_click(&app, cx, first, 1).await.is_some(),
            "a click on A starts its write"
        );
        let stage = drawn_stage_hunk(cx, row);
        let at = stage.center();
        assert!(
            counted_click(&app, cx, at, 1).await.is_some(),
            "the double-click's first press starts B's write"
        );
        assert_eq!(
            (
                app.read_with(cx, |app, _| app.patch_view.clone()) == Some(route.view.clone()),
                app.read_with(cx, |app, _| partial_held(app)
                    .map(|partial| partial.diff.area)),
                staged_hunks(&fixture),
                drawn_stage_hunk(cx, row)
            ),
            (
                false,
                Some(gitturtle_core::ChangeArea::Staged),
                "AB".to_owned(),
                stage
            ),
            "B's write landed: another view shows the staged diff, A's Unstage hunk under the still pointer"
        );
        assert_eq!(
            (counted_click(&app, cx, at, 2).await, staged_hunks(&fixture)),
            (None, "AB".to_owned()),
            "the second press of the double-click unstages nothing"
        );
        assert!(
            counted_click(&app, cx, at, 1).await.is_some(),
            "a click of its own starts its write"
        );
        assert_eq!(staged_hunks(&fixture), "B", "unstaging A, which it presses");
    }

    /// The Stage hunk a failed write left focused stays built as its row
    /// scrolls out of view, for the focus it holds there, and goes once
    /// focus leaves it, here for the file list. Built on, that one control
    /// out of view lasted until the next landing that changed the text.
    #[gpui::test]
    async fn a_pressed_stage_hunk_out_of_view_goes_as_focus_leaves_it(
        cx: &mut gpui::TestAppContext,
    ) {
        let (fixture, app, cx, route, row, held) =
            press_a_hunk(cx, gitturtle_core::ChangeArea::Unstaged, true).await;
        fail_held_write(&fixture, &app, cx, held).await;
        route.editor.update(cx, |state, cx| {
            let height = state.line_height().expect("the text is laid out");
            state.set_scroll_offset(gpui::point(gpui::px(0.), -height * (row + 20) as f32), cx);
        });
        crate::tags::tests::draw(cx);
        let selector: &'static str = format!("partial-hunk-{row}").leak();
        assert!(
            cx.debug_bounds(selector).is_some(),
            "built out of view while it holds focus"
        );
        let list = app.read_with(cx, |app, _| app.file_focus.clone());
        cx.update(|window, cx| window.focus(&list, cx));
        crate::tags::tests::draw(cx);
        assert_eq!(cx.debug_bounds(selector), None, "gone once focus leaves it");
    }

    /// notes.txt's unstaged diff edited to [`landing_notes`] in QA's 1480 ×
    /// 973 Midnight window at 13 pt, the text at its top: the fixture, its
    /// window and the patch's editor.
    async fn landing_diff_at_its_top(
        cx: &mut gpui::TestAppContext,
    ) -> (
        tempfile::TempDir,
        gpui::Entity<GitTurtle>,
        &mut gpui::VisualTestContext,
        gpui::Entity<gpui_kit::component::input::EditorState>,
    ) {
        let (fixture, app, cx) = working_diff_edited(
            cx,
            &crate::views::tests::history_script(1, 40),
            &[("notes.txt", &landing_notes())],
        )
        .await;
        crate::focus_ring::sweep::prepare(&app, cx);
        cx.simulate_resize(gpui::size(gpui::px(1480.), gpui::px(973.)));
        crate::focus_ring::sweep::interface_size(cx, 13);
        let (editor, _) = shown_diff(&app, cx);
        editor.update(cx, |state, cx| {
            state.set_scroll_offset(gpui::point(gpui::px(0.), gpui::px(0.)), cx)
        });
        crate::tags::tests::draw(cx);
        (fixture, app, cx, editor)
    }

    /// Starts a review of the shown diff with whitespace hidden, holding
    /// the reader so that it stays prepared, until the hold is dropped.
    fn prepare_a_review(
        app: &gpui::Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
    ) -> crate::worker::ReaderHeld {
        let held = app.read_with(cx, |app, _| {
            app.worker.wait_idle();
            app.worker.hold()
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.automatic.hold_quiet_reads(true);
                app.set_text_review(
                    Options {
                        hide_whitespace: true,
                        ..Options::default()
                    },
                    window,
                    cx,
                );
            })
        });
        crate::tags::tests::draw(cx);
        held
    }

    /// A pointer resting on Stage hunk reads what it does, then, as a text
    /// review of the diff is prepared, which dims it at once, why it is
    /// unavailable, and once the review is released what it does again,
    /// without moving: its tooltip reads the view each time it renders. The
    /// kit kept the text from the pointer's entry, so it read what the
    /// button did beside its dimmed look, and through a write the reason
    /// after the write had ended.
    #[gpui::test]
    async fn a_pointer_resting_on_stage_hunk_reads_its_tooltip_change(
        cx: &mut gpui::TestAppContext,
    ) {
        let (_fixture, app, cx, _) = landing_diff_at_its_top(cx).await;
        let row = hunk_rows(&app, cx)[0];
        let selector: &'static str = format!("partial-hunk-{row}").leak();
        let stage = cx.debug_bounds(selector).expect("Stage hunk is drawn");
        cx.simulate_mouse_move(stage.center(), None, gpui::Modifiers::default());
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(600));
        crate::tags::tests::draw(cx);
        let shows = |cx: &mut gpui::VisualTestContext, text: &str| {
            cx.debug_bounds(format!("partial-hunk-{row}/tooltip/{text}").leak())
                .is_some()
        };
        let purpose = "Stage all 4 changed lines in this hunk";
        assert!(shows(cx, purpose), "at rest, what it does");
        let held = prepare_a_review(&app, cx);
        assert_eq!(
            cx.debug_bounds(selector),
            Some(stage),
            "Stage hunk stays under the pointer"
        );
        assert!(
            shows(cx, "Wait until the text review is ready"),
            "while the review is prepared, why it is unavailable"
        );
        cx.update(|_, cx| app.update(cx, |app, cx| app.release_text_review(cx)));
        crate::tags::tests::draw(cx);
        assert!(shows(cx, purpose), "released, what it does again");
        drop(held);
        crate::views::tests::settle(&app, cx).await;
    }

    /// While a text review of the diff is prepared, which suspends the
    /// partial actions and dims them at once, Stage hunk and a line's
    /// checkbox say why in their tooltips, "Wait until the text review is
    /// ready", rather than the Git operation's reason, which would name
    /// work that is not running: they once said only what they did. Once
    /// the review is released they say what they do again.
    #[gpui::test]
    async fn partial_controls_dimmed_for_a_text_review_say_why(cx: &mut gpui::TestAppContext) {
        let (_fixture, app, cx, editor) = landing_diff_at_its_top(cx).await;
        let hunk = hunk_rows(&app, cx)[0];
        let twelve = patch_row(&editor, cx, "+twelve");
        let keys = [
            format!("partial-hunk-{hunk}"),
            format!("partial-line-{twelve}"),
        ];
        let tooltips = |cx: &mut gpui::VisualTestContext| {
            crate::tags::tests::draw(cx);
            keys.iter()
                .map(|key| {
                    crate::views::CONTROL_TOOLTIPS
                        .with(|tooltips| tooltips.borrow().get(key.as_str()).cloned())
                        .unwrap_or_else(|| panic!("{key} is drawn"))
                })
                .collect::<Vec<_>>()
        };
        let purposes = [
            "Stage all 4 changed lines in this hunk".to_owned(),
            format!(
                "Select this changed line. {} selects a range.",
                crate::shortcuts::SHIFT_CLICK
            ),
        ];
        assert_eq!(tooltips(cx), purposes, "at rest, what each does");
        let held = prepare_a_review(&app, cx);
        assert_eq!(
            tooltips(cx),
            ["Wait until the text review is ready"; 2].map(str::to_owned),
            "while the review is prepared, why each is unavailable"
        );
        cx.update(|_, cx| app.update(cx, |app, cx| app.release_text_review(cx)));
        assert_eq!(tooltips(cx), purposes, "released, what each does again");
        drop(held);
        crate::views::tests::settle(&app, cx).await;
    }

    /// How a key held on a partial control repeats: marked held, as macOS
    /// and Wayland send a held key's repeats; as fresh presses with no
    /// key-up between, as GPUI's X11 client delivers them; or as a release
    /// and a fresh press together, before the application next runs a
    /// task, as an X11 repeat whose release GPUI did not drop, or an input
    /// method's, arrives; or as a release that ends one batch of X11 events
    /// and a press that begins the next, with an idle pass between them and
    /// no frame.
    #[derive(Clone, Copy, Debug)]
    enum Repeats {
        Held,
        X11,
        Paired,
        Split,
    }

    /// Sends `times` repeats of the held `key`, as `repeats` says.
    fn repeat_key(cx: &mut gpui::VisualTestContext, key: &str, repeats: Repeats, times: usize) {
        let keystroke = gpui::Keystroke::parse(key).unwrap();
        let down = gpui::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: matches!(repeats, Repeats::Held),
            prefer_character_input: false,
        };
        for _ in 0..times {
            if let Repeats::Paired | Repeats::Split = repeats {
                let up = gpui::KeyUpEvent {
                    keystroke: keystroke.clone(),
                };
                cx.update(|window, cx| {
                    window.dispatch_event(gpui::PlatformInput::KeyUp(up), cx);
                });
                if let Repeats::Split = repeats {
                    cx.run_until_parked();
                }
                cx.update(|window, cx| {
                    window.dispatch_event(gpui::PlatformInput::KeyDown(down.clone()), cx);
                });
                cx.run_until_parked();
            } else {
                cx.simulate_event(down.clone());
            }
        }
        crate::tags::tests::draw(cx);
    }

    /// Releases the held `key`, then draws, as the window does before a
    /// person presses a key again.
    fn release_key(cx: &mut gpui::VisualTestContext, key: &str) {
        let keystroke = gpui::Keystroke::parse(key).unwrap();
        cx.simulate_event(gpui::KeyUpEvent { keystroke });
        crate::tags::tests::draw(cx);
    }

    /// Route's Stage in Unstaged held and made to fail (a held index lock),
    /// Tab to the bar's action, then `key` down on it while the write runs
    /// and held, repeating as `repeats` says, until the write has failed,
    /// which keeps the view, its selection and focus on the action: the
    /// fixture, its window and route, with the key still down.
    async fn hold_on_the_action_through_a_failure<'a>(
        cx: &'a mut gpui::TestAppContext,
        key: &str,
        repeats: Repeats,
    ) -> (
        tempfile::TempDir,
        gpui::Entity<GitTurtle>,
        &'a mut gpui::VisualTestContext,
        Route,
    ) {
        hold_on_the_action_through_a_failure_with(cx, key, repeats, None).await
    }

    /// [`hold_on_the_action_through_a_failure`], with `other`, where given,
    /// the other of Return and Space pressed and released on the action
    /// while the write runs, after which `key` repeats no more, as
    /// autorepeat follows the key pressed last.
    async fn hold_on_the_action_through_a_failure_with<'a>(
        cx: &'a mut gpui::TestAppContext,
        key: &str,
        repeats: Repeats,
        other: Option<&str>,
    ) -> (
        tempfile::TempDir,
        gpui::Entity<GitTurtle>,
        &'a mut gpui::VisualTestContext,
        Route,
    ) {
        let (fixture, app, cx, route, held) =
            Route::press(cx, gitturtle_core::ChangeArea::Unstaged, &landing_notes()).await;
        cx.update(|window, _| window.activate_window());
        crate::tags::tests::draw(cx);
        assert!(
            cx.update(|window, _| window.is_window_active()),
            "the window has the keyboard"
        );
        let action = route
            .view
            .read_with(cx, |view, _| view.apply_focus_handle());
        press(cx, "tab");
        let lock = fixture
            .path()
            .join("narrow")
            .join(".git")
            .join("index.lock");
        std::fs::write(&lock, "").unwrap();
        cx.simulate_keystrokes(key);
        repeat_key(cx, key, repeats, 2);
        if let Some(other) = other {
            press(cx, other);
        }
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        crate::tags::tests::draw(cx);
        std::fs::remove_file(&lock).unwrap();
        if other.is_none() {
            repeat_key(cx, key, repeats, 2);
        }
        assert!(
            app.read_with(cx, |app, _| app.operation_error.is_some()),
            "the write failed"
        );
        assert!(
            cx.update(|window, _| action.is_focused(window)),
            "focus stays on the action"
        );
        assert_eq!(route.view.read_with(cx, |view, _| view.selected_lines()), 2);
        (fixture, app, cx, route)
    }

    /// What the index stages once any write a key started has ended.
    async fn staged_once_settled(
        fixture: &tempfile::TempDir,
        app: &gpui::Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
    ) -> String {
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(app, cx).await;
        crate::tags::tests::draw(cx);
        staged_patch(fixture)
    }

    /// The next press of the action after a refused one: it stages the two
    /// selected lines, once.
    async fn the_next_press_stages(
        fixture: &tempfile::TempDir,
        app: &gpui::Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
        key: &str,
        at: &str,
    ) {
        assert_eq!(staged_patch(fixture), "", "{at}: nothing staged before");
        press(cx, key);
        let staged = staged_once_settled(fixture, app, cx).await;
        assert!(
            staged.contains("+added one\n+added two\n") && !staged.contains("twelve"),
            "{at}: the next {key} stages the two lines: {staged}"
        );
    }

    /// How a partial write a key is held through ends.
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Ending {
        /// The write fails (a held index lock): the view, its selection and
        /// focus on the control the key went down on stay.
        Failed,
        /// The write lands: its lines leave the area with their selection,
        /// and focus goes to the text.
        Landed,
    }

    /// Route's write held, a Stage in Unstaged that fails, or an Unstage
    /// in Staged that lands, Tab (twice for Clear, `on_clear`) to the bar's
    /// refused control, then `key` down on it and held, repeating as
    /// `repeats` says before and after the window looks busy and after the
    /// write ends as `ending` says, then released: the key-up starts no
    /// write, changes the index no more and changes no selection, as a
    /// refused press stays refused for its whole press. A key held on the
    /// action through a failed write and released after it started a second
    /// write of the lines (FU45-A3's security re-check: the failure keeps
    /// focus on the action, where GPUI clicks as the key comes up), and
    /// Space so held under X11, its repeats fresh presses, went on toggling
    /// the line the landed caret is on, in the text the landing gave focus
    /// to. After a failure, the next press acts.
    async fn hold_a_key_through_a_partial_write(
        cx: &mut gpui::TestAppContext,
        key: &str,
        repeats: Repeats,
        ending: Ending,
        on_clear: bool,
    ) {
        let area = match ending {
            Ending::Failed => gitturtle_core::ChangeArea::Unstaged,
            Ending::Landed => gitturtle_core::ChangeArea::Staged,
        };
        let (fixture, app, cx, route, held) = Route::press(cx, area, &landing_notes()).await;
        let staged_before = staged_patch(&fixture);
        let at = format!(
            "{key} held ({repeats:?}) on the {} through a {ending:?} write",
            if on_clear { "Clear" } else { "action" }
        );
        let control = route.view.read_with(cx, |view, _| {
            if on_clear {
                view.clear_focus_handle()
            } else {
                view.apply_focus_handle()
            }
        });
        press(cx, if on_clear { "tab tab" } else { "tab" });
        assert!(
            cx.update(|window, _| control.is_focused(window)),
            "{at}: Tab reaches the refused control"
        );
        let lock = fixture
            .path()
            .join("narrow")
            .join(".git")
            .join("index.lock");
        if ending == Ending::Failed {
            std::fs::write(&lock, "").unwrap();
        }
        cx.simulate_keystrokes(key);
        crate::tags::tests::draw(cx);
        repeat_key(cx, key, repeats, 2);
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        native_frames(cx);
        repeat_key(cx, key, repeats, 2);
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        crate::tags::tests::draw(cx);
        if ending == Ending::Failed {
            std::fs::remove_file(&lock).unwrap();
        }
        let staged = staged_patch(&fixture);
        let (failed, same) = app.read_with(cx, |app, _| {
            (
                app.operation_error.is_some(),
                app.patch_view.as_ref() == Some(&route.view),
            )
        });
        assert_eq!(
            failed,
            ending == Ending::Failed,
            "{at}: the write's outcome"
        );
        assert!(same, "{at}: the Diff view stays");
        let (focus, selected) = (
            cx.update(|window, _| control.is_focused(window)),
            route.view.read_with(cx, |view, _| view.selected_lines()),
        );
        match ending {
            Ending::Failed => {
                assert!(staged.is_empty(), "{at}: the failed write staged nothing");
                assert!(focus, "{at}: focus stays on the control the key is held on");
                assert_eq!(selected, 2, "{at}: the failure keeps the selection");
            }
            Ending::Landed => {
                assert!(
                    staged_before.contains("+added one\n+added two\n")
                        && !staged.contains("added one")
                        && !staged.contains("added two")
                        && staged.contains("+twelve"),
                    "{at}: the write unstaged its two lines alone: {staged}"
                );
                assert!(
                    patch_row(&route.editor, cx, "-note 12") == text_place(&route.editor, cx).0,
                    "{at}: the caret lands on the next changed line, where Space selects"
                );
                assert!(
                    cx.update(|window, cx| route.view.read(cx).focus_handle(cx).is_focused(window)),
                    "{at}: the landing gives focus to the text"
                );
                assert_eq!(selected, 0, "{at}: the lines left with their selection");
            }
        }
        // The key still held after the write ended, then released.
        repeat_key(cx, key, repeats, 3);
        release_key(cx, key);
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            None,
            "{at}: the key-up starts no write"
        );
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        crate::tags::tests::draw(cx);
        assert_eq!(staged_patch(&fixture), staged, "{at}: nothing more staged");
        assert_eq!(
            route.view.read_with(cx, |view, _| view.selected_lines()),
            selected,
            "{at}: the selection as the write left it"
        );
        if ending == Ending::Failed && !on_clear {
            // The refusal was the press's: the next press of the action acts.
            press(cx, key);
            app.read_with(cx, |app, _| app.operations.drain());
            crate::views::tests::settle(&app, cx).await;
            let staged = staged_patch(&fixture);
            assert!(
                staged.contains("+added one\n+added two\n") && !staged.contains("twelve"),
                "{at}: the next {key} stages the two lines: {staged}"
            );
        }
    }

    #[gpui::test]
    async fn a_return_held_on_the_action_through_a_failed_write_starts_none(
        cx: &mut gpui::TestAppContext,
    ) {
        hold_a_key_through_a_partial_write(cx, "enter", Repeats::Held, Ending::Failed, false).await;
    }

    #[gpui::test]
    async fn a_return_repeated_on_the_action_through_a_failed_write_starts_none(
        cx: &mut gpui::TestAppContext,
    ) {
        hold_a_key_through_a_partial_write(cx, "enter", Repeats::X11, Ending::Failed, false).await;
    }

    #[gpui::test]
    async fn a_space_held_on_the_action_through_a_failed_write_starts_none(
        cx: &mut gpui::TestAppContext,
    ) {
        hold_a_key_through_a_partial_write(cx, "space", Repeats::Held, Ending::Failed, false).await;
    }

    #[gpui::test]
    async fn a_space_repeated_on_the_action_through_a_failed_write_starts_none(
        cx: &mut gpui::TestAppContext,
    ) {
        hold_a_key_through_a_partial_write(cx, "space", Repeats::X11, Ending::Failed, false).await;
    }

    #[gpui::test]
    async fn a_return_repeated_in_pairs_on_the_action_through_a_failed_write_starts_none(
        cx: &mut gpui::TestAppContext,
    ) {
        hold_a_key_through_a_partial_write(cx, "enter", Repeats::Paired, Ending::Failed, false)
            .await;
    }

    #[gpui::test]
    async fn a_space_repeated_on_clear_through_a_failed_write_keeps_the_selection(
        cx: &mut gpui::TestAppContext,
    ) {
        hold_a_key_through_a_partial_write(cx, "space", Repeats::X11, Ending::Failed, true).await;
    }

    #[gpui::test]
    async fn a_return_held_on_the_action_through_a_landing_does_nothing_more(
        cx: &mut gpui::TestAppContext,
    ) {
        hold_a_key_through_a_partial_write(cx, "enter", Repeats::Held, Ending::Landed, false).await;
    }

    #[gpui::test]
    async fn a_return_repeated_on_the_action_through_a_landing_does_nothing_more(
        cx: &mut gpui::TestAppContext,
    ) {
        hold_a_key_through_a_partial_write(cx, "enter", Repeats::X11, Ending::Landed, false).await;
    }

    #[gpui::test]
    async fn a_space_held_on_the_action_through_a_landing_does_nothing_more(
        cx: &mut gpui::TestAppContext,
    ) {
        hold_a_key_through_a_partial_write(cx, "space", Repeats::Held, Ending::Landed, false).await;
    }

    #[gpui::test]
    async fn a_space_repeated_on_the_action_through_a_landing_selects_nothing(
        cx: &mut gpui::TestAppContext,
    ) {
        hold_a_key_through_a_partial_write(cx, "space", Repeats::X11, Ending::Landed, false).await;
    }

    /// An X11 repeat whose release ends one batch of events and whose press
    /// begins the next, the idle pass between them, continues the refused
    /// press: ended at that idle pass, the rest of the hold was a fresh
    /// press, which its key-up let write.
    #[gpui::test]
    async fn a_return_repeated_in_split_pairs_on_the_action_through_a_failed_write_starts_none(
        cx: &mut gpui::TestAppContext,
    ) {
        hold_a_key_through_a_partial_write(cx, "enter", Repeats::Split, Ending::Failed, false)
            .await;
    }

    /// Return held on the refused action through a failed write, then
    /// Space pressed with it after the failure, then each released, in
    /// either order: neither key-up starts a write, as GPUI clicks on
    /// either key's release after either's press, and the Space pressed
    /// while the refused Return was down acts no more than it. Space,
    /// pressed last, repeats until it comes up, and Return then repeats no
    /// more, as autorepeat follows the key pressed last on every platform.
    /// The next press stages the lines once.
    async fn return_and_space_together_on_the_refused_action(
        cx: &mut gpui::TestAppContext,
        repeats: Repeats,
        space_first: bool,
    ) {
        let (fixture, app, cx, _route) =
            hold_on_the_action_through_a_failure(cx, "enter", repeats).await;
        let at = format!("Return then Space ({repeats:?}), Space up first: {space_first}");
        cx.simulate_keystrokes("space");
        repeat_key(cx, "space", repeats, 2);
        let (first, second) = if space_first {
            ("space", "enter")
        } else {
            ("enter", "space")
        };
        release_key(cx, first);
        assert_eq!(
            staged_once_settled(&fixture, &app, cx).await,
            "",
            "{at}: {first} up writes nothing"
        );
        if second == "space" {
            repeat_key(cx, second, repeats, 2);
        }
        release_key(cx, second);
        assert_eq!(
            staged_once_settled(&fixture, &app, cx).await,
            "",
            "{at}: {second} up writes nothing"
        );
        the_next_press_stages(&fixture, &app, cx, "enter", &at).await;
    }

    #[gpui::test]
    async fn return_up_before_a_space_pressed_with_it_on_the_refused_action_starts_no_write(
        cx: &mut gpui::TestAppContext,
    ) {
        return_and_space_together_on_the_refused_action(cx, Repeats::X11, false).await;
    }

    #[gpui::test]
    async fn space_up_before_a_return_it_was_pressed_with_on_the_refused_action_starts_no_write(
        cx: &mut gpui::TestAppContext,
    ) {
        return_and_space_together_on_the_refused_action(cx, Repeats::Held, true).await;
    }

    /// A Space coming up on the refused action whose press the window never
    /// saw, while Return is held there through a failed write, starts no
    /// write: GPUI clicks on it after the Return's key-down.
    #[gpui::test]
    async fn a_space_up_without_its_press_on_the_refused_action_starts_no_write(
        cx: &mut gpui::TestAppContext,
    ) {
        let (fixture, app, cx, _route) =
            hold_on_the_action_through_a_failure(cx, "enter", Repeats::X11).await;
        release_key(cx, "space");
        assert_eq!(
            staged_once_settled(&fixture, &app, cx).await,
            "",
            "Space up writes nothing"
        );
        repeat_key(cx, "enter", Repeats::X11, 2);
        release_key(cx, "enter");
        assert_eq!(
            staged_once_settled(&fixture, &app, cx).await,
            "",
            "Return up writes nothing"
        );
        the_next_press_stages(&fixture, &app, cx, "enter", "after a lone Space up").await;
    }

    /// `first` held on the refused action through a failed write, the
    /// other of Return and Space pressed and released there while it is
    /// down, as the write runs (`during`) or after it failed, then pressed
    /// and released again, then `first` released: none of them starts a
    /// write, as every Return or Space pressed while a refused one is down
    /// is refused, and the refused press outlasts the other's. Ended with
    /// the other's press, it left the third press to begin afresh, which
    /// started a second write of the lines as it came up (FU45-A5's
    /// security re-check). The next press stages the lines once.
    async fn a_key_pressed_again_while_a_refused_one_is_down(
        cx: &mut gpui::TestAppContext,
        first: &str,
        repeats: Repeats,
        during: bool,
    ) {
        let other = if first == "enter" { "space" } else { "enter" };
        let at = format!("{first} held ({repeats:?}), {other} pressed during the write: {during}");
        let (fixture, app, cx, _route) =
            hold_on_the_action_through_a_failure_with(cx, first, repeats, during.then_some(other))
                .await;
        if !during {
            press(cx, other);
            assert_eq!(
                staged_once_settled(&fixture, &app, cx).await,
                "",
                "{at}: the second press writes nothing"
            );
        }
        press(cx, other);
        assert_eq!(
            staged_once_settled(&fixture, &app, cx).await,
            "",
            "{at}: the third press writes nothing"
        );
        release_key(cx, first);
        assert_eq!(
            staged_once_settled(&fixture, &app, cx).await,
            "",
            "{at}: {first} up writes nothing"
        );
        the_next_press_stages(&fixture, &app, cx, first, &at).await;
    }

    #[gpui::test]
    async fn space_pressed_twice_while_a_repeated_return_is_refused_starts_no_write(
        cx: &mut gpui::TestAppContext,
    ) {
        a_key_pressed_again_while_a_refused_one_is_down(cx, "enter", Repeats::X11, false).await;
    }

    #[gpui::test]
    async fn space_pressed_twice_while_a_held_return_is_refused_starts_no_write(
        cx: &mut gpui::TestAppContext,
    ) {
        a_key_pressed_again_while_a_refused_one_is_down(cx, "enter", Repeats::Held, false).await;
    }

    #[gpui::test]
    async fn return_pressed_twice_while_a_held_space_is_refused_starts_no_write(
        cx: &mut gpui::TestAppContext,
    ) {
        a_key_pressed_again_while_a_refused_one_is_down(cx, "space", Repeats::Held, false).await;
    }

    #[gpui::test]
    async fn return_pressed_twice_while_a_repeated_space_is_refused_starts_no_write(
        cx: &mut gpui::TestAppContext,
    ) {
        a_key_pressed_again_while_a_refused_one_is_down(cx, "space", Repeats::X11, false).await;
    }

    #[gpui::test]
    async fn space_pressed_during_the_write_and_after_while_a_held_return_is_refused_starts_no_write(
        cx: &mut gpui::TestAppContext,
    ) {
        a_key_pressed_again_while_a_refused_one_is_down(cx, "enter", Repeats::Held, true).await;
    }

    #[gpui::test]
    async fn return_pressed_during_the_write_and_after_while_a_repeated_space_is_refused_starts_no_write(
        cx: &mut gpui::TestAppContext,
    ) {
        a_key_pressed_again_while_a_refused_one_is_down(cx, "space", Repeats::X11, true).await;
    }

    /// A Return repeat marked held on the action at rest, its press gone
    /// down where the view never saw it, then its key-up: the action stages
    /// the lines, once, as nothing refuses it. A held repeat with no press
    /// under way begins one, refused only while the partial controls refuse
    /// a press, so a key held into the view acts as it comes up.
    #[gpui::test]
    async fn a_held_return_whose_press_came_elsewhere_stages_once_on_the_action(
        cx: &mut gpui::TestAppContext,
    ) {
        let (fixture, app, cx, _route) =
            hold_on_the_action_through_a_failure(cx, "enter", Repeats::X11).await;
        release_key(cx, "enter");
        assert_eq!(
            staged_once_settled(&fixture, &app, cx).await,
            "",
            "the refused Return writes nothing"
        );
        repeat_key(cx, "enter", Repeats::Held, 3);
        release_key(cx, "enter");
        let staged = staged_once_settled(&fixture, &app, cx).await;
        assert!(
            staged.contains("+added one\n+added two\n") && !staged.contains("twelve"),
            "the held Return's key-up stages the two lines: {staged}"
        );
    }

    /// Return held on the refused action through a failed write, X11's
    /// repeats arriving as key-downs alone, while the window turns inactive
    /// and active again, as another client's keyboard grab makes it under
    /// X11: the repeats after it, and the key-up, start no write. Forgotten
    /// as the window turned inactive, the next repeat began a fresh press.
    #[gpui::test]
    async fn a_return_held_while_the_window_turns_inactive_stays_refused(
        cx: &mut gpui::TestAppContext,
    ) {
        let (fixture, app, cx, _route) =
            hold_on_the_action_through_a_failure(cx, "enter", Repeats::X11).await;
        cx.deactivate_window();
        crate::tags::tests::draw(cx);
        assert!(
            !cx.update(|window, _| window.is_window_active()),
            "inactive"
        );
        cx.update(|window, _| window.activate_window());
        crate::tags::tests::draw(cx);
        assert!(
            cx.update(|window, _| window.is_window_active()),
            "active again"
        );
        repeat_key(cx, "enter", Repeats::X11, 2);
        release_key(cx, "enter");
        assert_eq!(
            staged_once_settled(&fixture, &app, cx).await,
            "",
            "the key-up writes nothing"
        );
        the_next_press_stages(&fixture, &app, cx, "enter", "after the window's return").await;
    }

    /// Return held on the refused action through a failed write, its
    /// repeats marked held, its key-up lost outside the window, which turns
    /// inactive and active again: the next Return, unmarked, is a fresh
    /// press and stages the lines, rather than repeat the refused press.
    #[gpui::test]
    async fn a_press_after_a_held_return_came_up_outside_the_window_acts(
        cx: &mut gpui::TestAppContext,
    ) {
        let (fixture, app, cx, _route) =
            hold_on_the_action_through_a_failure(cx, "enter", Repeats::Held).await;
        cx.deactivate_window();
        crate::tags::tests::draw(cx);
        cx.update(|window, _| window.activate_window());
        crate::tags::tests::draw(cx);
        the_next_press_stages(&fixture, &app, cx, "enter", "after a key-up lost").await;
    }

    /// Return held on the refused action through a failed write, X11's
    /// repeats arriving as key-downs alone, its key-up lost outside the
    /// window, which turns inactive and active again: the next press, of
    /// `next`, is swallowed, taken for that Return's repeat or as pressed
    /// while it is down, and writes nothing; the one after stages the
    /// lines, so a key-up lost costs one press at most. A Space would
    /// otherwise inherit the lost Return's refusal at each press, as a
    /// Space pressed while a refused Return is down does.
    async fn a_return_lost_outside_the_window_costs_one_press(
        cx: &mut gpui::TestAppContext,
        next: &str,
    ) {
        let (fixture, app, cx, _route) =
            hold_on_the_action_through_a_failure(cx, "enter", Repeats::X11).await;
        cx.deactivate_window();
        crate::tags::tests::draw(cx);
        cx.update(|window, _| window.activate_window());
        crate::tags::tests::draw(cx);
        press(cx, next);
        assert_eq!(
            staged_once_settled(&fixture, &app, cx).await,
            "",
            "the first {next} after the lost key-up writes nothing"
        );
        the_next_press_stages(&fixture, &app, cx, next, "after a key-up lost").await;
    }

    #[gpui::test]
    async fn a_return_after_a_repeated_return_came_up_outside_the_window_is_swallowed_once(
        cx: &mut gpui::TestAppContext,
    ) {
        a_return_lost_outside_the_window_costs_one_press(cx, "enter").await;
    }

    #[gpui::test]
    async fn a_space_after_a_repeated_return_came_up_outside_the_window_is_swallowed_once(
        cx: &mut gpui::TestAppContext,
    ) {
        a_return_lost_outside_the_window_costs_one_press(cx, "space").await;
    }

    /// Return held on the refused action through a failed write and Space
    /// pressed with it, X11's repeats arriving as key-downs alone, while the
    /// window turns inactive and active again, as a keyboard grab makes it;
    /// then a Space repeat, Space released and pressed once more while
    /// Return is still down: that press writes nothing, nor does Return's
    /// key-up, and the next press stages the lines once. The Space repeat
    /// once ended Return's press, marked as the window turned inactive, and
    /// that press, begun afresh, wrote a second time.
    #[gpui::test]
    async fn a_space_pressed_again_after_the_window_returns_while_a_refused_return_is_held_starts_no_write(
        cx: &mut gpui::TestAppContext,
    ) {
        let (fixture, app, cx, _route) =
            hold_on_the_action_through_a_failure(cx, "enter", Repeats::X11).await;
        cx.simulate_keystrokes("space");
        repeat_key(cx, "space", Repeats::X11, 2);
        cx.deactivate_window();
        crate::tags::tests::draw(cx);
        cx.update(|window, _| window.activate_window());
        crate::tags::tests::draw(cx);
        repeat_key(cx, "space", Repeats::X11, 1);
        release_key(cx, "space");
        assert_eq!(
            staged_once_settled(&fixture, &app, cx).await,
            "",
            "Space up writes nothing"
        );
        press(cx, "space");
        assert_eq!(
            staged_once_settled(&fixture, &app, cx).await,
            "",
            "Space pressed again while Return is down writes nothing"
        );
        release_key(cx, "enter");
        assert_eq!(
            staged_once_settled(&fixture, &app, cx).await,
            "",
            "Return up writes nothing"
        );
        the_next_press_stages(&fixture, &app, cx, "enter", "after both keys came up").await;
    }

    /// Return held on the refused action through a failed write, X11's
    /// repeats arriving as key-downs alone, then focus moved to the file
    /// list, where its key-up comes, and back to the action: the next
    /// Return there is a fresh press and stages the lines. Never ended, the
    /// press took it for a repeat and refused it.
    #[gpui::test]
    async fn a_press_after_a_refused_return_came_up_on_another_control_acts(
        cx: &mut gpui::TestAppContext,
    ) {
        let (fixture, app, cx, route) =
            hold_on_the_action_through_a_failure(cx, "enter", Repeats::X11).await;
        let (list, action) = (
            app.read_with(cx, |app, _| app.file_focus.clone()),
            route
                .view
                .read_with(cx, |view, _| view.apply_focus_handle()),
        );
        cx.update(|window, cx| window.focus(&list, cx));
        crate::tags::tests::draw(cx);
        release_key(cx, "enter");
        assert_eq!(
            staged_once_settled(&fixture, &app, cx).await,
            "",
            "the key-up on the list writes nothing"
        );
        cx.update(|window, cx| window.focus(&action, cx));
        crate::tags::tests::draw(cx);
        assert_eq!(
            route.view.read_with(cx, |view, _| view.selected_lines()),
            2,
            "the selection and its bar stay"
        );
        the_next_press_stages(&fixture, &app, cx, "enter", "back on the action").await;
    }

    /// Space pressed in Find's field over the diff while a partial write
    /// runs, where it types, refused, X11's repeats arriving as key-downs
    /// alone, then focus moved to the file list mid-hold, where its key-up
    /// comes, and the write failing: back on the action, the next Return
    /// stages the lines. Focus leaving Find's field, unlike the text, the
    /// action and Clear, never ended the press, and that Return inherited
    /// its refusal. (Return in the field never begins a press: Find's
    /// Enter action takes it first.)
    #[gpui::test]
    async fn a_return_after_a_refused_space_in_find_came_up_on_another_control_acts(
        cx: &mut gpui::TestAppContext,
    ) {
        let (fixture, app, cx, route, held) =
            Route::press(cx, gitturtle_core::ChangeArea::Unstaged, &landing_notes()).await;
        cx.update(|window, _| window.activate_window());
        crate::tags::tests::draw(cx);
        let lock = fixture
            .path()
            .join("narrow")
            .join(".git")
            .join("index.lock");
        std::fs::write(&lock, "").unwrap();
        let text = cx.update(|_, cx| route.view.read(cx).focus_handle(cx));
        cx.update(|window, cx| window.focus(&text, cx));
        crate::tags::tests::draw(cx);
        press(cx, &primary("f"));
        assert!(
            cx.update(|window, cx| crate::editor_find::bar_contains_focused(
                &route.editor,
                window,
                cx
            )),
            "Find's field has focus"
        );
        cx.simulate_keystrokes("space");
        repeat_key(cx, "space", Repeats::X11, 2);
        let (list, action) = (
            app.read_with(cx, |app, _| app.file_focus.clone()),
            route
                .view
                .read_with(cx, |view, _| view.apply_focus_handle()),
        );
        cx.update(|window, cx| window.focus(&list, cx));
        crate::tags::tests::draw(cx);
        release_key(cx, "space");
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        crate::tags::tests::draw(cx);
        std::fs::remove_file(&lock).unwrap();
        assert!(
            app.read_with(cx, |app, _| app.operation_error.is_some()),
            "the write failed"
        );
        cx.update(|window, cx| window.focus(&action, cx));
        crate::tags::tests::draw(cx);
        assert_eq!(
            route.view.read_with(cx, |view, _| view.selected_lines()),
            2,
            "the selection and its bar stay"
        );
        the_next_press_stages(&fixture, &app, cx, "enter", "back on the action from Find").await;
    }

    /// A partial Stage of every changed line keeps the Diff view through
    /// its write; the file then leaves Unstaged, and the selection follows
    /// it to its other group, Staged (`retained_area`), as any write that
    /// moves a file has it: that row is selected with its staged diff shown,
    /// and focus goes to the list that shows it, its ring drawn whole, never
    /// to the window frame.
    #[gpui::test]
    async fn a_partial_stage_of_every_line_follows_the_file_to_staged(
        cx: &mut gpui::TestAppContext,
    ) {
        let notes: String = (1..=40)
            .map(|line| match line {
                11 => "added one\nadded two\nnote 11\n".to_owned(),
                _ => format!("note {line}\n"),
            })
            .collect();
        let (_fixture, app, cx, route, held) =
            Route::press(cx, gitturtle_core::ChangeArea::Unstaged, &notes).await;
        route.assert_kept(
            &app,
            cx,
            "Staging selected changes…",
            "the frame after Enter",
        );
        native_frames(cx);
        route.assert_kept(
            &app,
            cx,
            "Staging selected changes…",
            "while the write is held",
        );
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        crate::tags::tests::draw(cx);
        app.read_with(cx, |app, _| {
            assert_eq!(
                app.working_selected,
                Some((0, gitturtle_core::ChangeArea::Staged)),
                "the selection follows the file to Staged"
            );
            assert!(
                matches!(
                    app.content.as_deref(),
                    Some(Content::Text { partial: Some(partial), .. })
                        if partial.diff.area == gitturtle_core::ChangeArea::Staged
                ),
                "its staged diff is shown"
            );
        });
        assert!(cx.debug_bounds("empty-state").is_none());
        let list = app.read_with(cx, |app, _| app.file_focus.clone());
        assert!(
            cx.update(|window, _| list.is_focused(window)),
            "focus goes to the list, not to {:?}",
            cx.update(|window, cx| window.focused(cx))
        );
        let accent: gpui::Hsla =
            cx.read(|cx| gpui::rgb(crate::appearance::palette(cx).accent).into());
        let rings = crate::focus_ring::painted_outlines(cx, accent);
        assert!(
            rings.iter().any(|ring| ring.whole),
            "the list draws its ring whole: {rings:?}"
        );
    }

    /// An outside change that lands as a partial write runs, a file added
    /// beside the one shown, its watcher's report waiting for the write as
    /// every quiet read does, never drops the Diff view either: through the
    /// write, its reads and the quiet read after them, every frame draws
    /// the same view with focus in its text, which ends with the staged
    /// lines landed in place and the new file listed. (A change to the shown
    /// file itself fails the write's own check, which keeps the view as
    /// [`a_failed_partial_stage_leaves_the_diff_view_as_it_was`] shows.)
    #[gpui::test]
    async fn an_outside_change_during_a_partial_write_keeps_the_diff_view(
        cx: &mut gpui::TestAppContext,
    ) {
        let notes = landing_notes();
        let (fixture, app, cx, route, held) =
            Route::press(cx, gitturtle_core::ChangeArea::Unstaged, &notes).await;
        std::fs::write(fixture.path().join("narrow").join("outside.txt"), "new\n").unwrap();
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.queue_automatic_refresh(
                    crate::local_refresh::LocalChange {
                        worktree: true,
                        ..Default::default()
                    },
                    window,
                    cx,
                )
            })
        });
        native_frames(cx);
        route.assert_kept(
            &app,
            cx,
            "Staging selected changes…",
            "as the change is reported",
        );
        drop(held);
        for step in 0..32 {
            app.read_with(cx, |app, _| {
                app.operations.drain();
                app.worker.wait_idle();
            });
            cx.executor().run_until_parked();
            crate::tags::tests::draw(cx);
            assert!(cx.debug_bounds("empty-state").is_none(), "step {step}");
            app.read_with(cx, |app, _| {
                assert_eq!(app.patch_view.as_ref(), Some(&route.view), "step {step}");
            });
            assert!(
                cx.update(|window, cx| route.view.read(cx).focus_handle(cx).is_focused(window)),
                "step {step}: focus in the text, not on {:?}",
                cx.update(|window, cx| window.focused(cx))
            );
            let settled = app.read_with(cx, |app, _| {
                app.operation_busy.is_none()
                    && app.status_task.is_none()
                    && app.task.is_none()
                    && !app.automatic.quiet_read_running()
            });
            if settled {
                break;
            }
        }
        crate::views::tests::settle(&app, cx).await;
        crate::tags::tests::draw(cx);
        let shown = route
            .editor
            .read_with(cx, |state, _| state.value().to_string());
        assert!(
            shown.contains("\n added two\n"),
            "the staged lines landed: {shown}"
        );
        app.read_with(cx, |app, _| {
            assert!(
                app.operation_error.is_none(),
                "the write staged: {:?}",
                app.operation_error
            );
            assert!(
                app.work_status.as_ref().is_some_and(|status| status
                    .entries
                    .iter()
                    .any(|entry| entry.path.ends_with("outside.txt"))),
                "the outside change is read"
            );
        });
        assert_eq!(
            text_place(&route.editor, cx).0,
            patch_row(&route.editor, cx, " added two"),
            "the caret's line"
        );
    }

    /// notes.txt's `lines` notes, each of `inserted` after its note, and
    /// `tail` lines appended.
    fn notes_with(lines: usize, inserted: &[(usize, &str)], tail: usize) -> String {
        let mut text = String::new();
        for line in 1..=lines {
            text.push_str(&format!("note {line}\n"));
            for (_, added) in inserted.iter().filter(|(after, _)| *after == line) {
                text.push_str(added);
                text.push('\n');
            }
        }
        for line in 0..tail {
            text.push_str(&format!("tail {line}\n"));
        }
        text
    }

    /// Clicks the shown patch's line `line` in its text, which moves the
    /// caret there.
    fn click_line(
        editor: &gpui::Entity<gpui_kit::component::input::EditorState>,
        cx: &mut gpui::VisualTestContext,
        line: &str,
    ) {
        let row = patch_row(editor, cx, line);
        let (origin, height) = editor.read_with(cx, |state, _| {
            (
                state.text_bounds().expect("the text is painted").origin,
                state.line_height().expect("the text is laid out"),
            )
        });
        cx.simulate_click(
            gpui::point(
                origin.x + gpui::px(40.),
                origin.y + height * (row as f32 + 0.5),
            ),
            gpui::Modifiers::default(),
        );
        crate::tags::tests::draw(cx);
        assert_eq!(text_place(editor, cx).0, row, "the click's line {line:?}");
    }

    /// Tab to the selection bar's action, then, with the operations
    /// executor held, Enter, which starts the write and leaves it held.
    fn press_action(
        app: &gpui::Entity<GitTurtle>,
        view: &gpui::Entity<crate::diff_view::DiffView>,
        cx: &mut gpui::VisualTestContext,
    ) -> crate::operations::Held {
        let apply = view.read_with(cx, |view, _| view.apply_focus_handle());
        press(cx, "tab");
        assert!(
            cx.update(|window, _| apply.is_focused(window)),
            "Tab reaches the action"
        );
        let held = app.read_with(cx, |app, _| app.operations.hold());
        crate::native_accessibility::written::forget();
        let enter = gpui::Keystroke::parse("enter").unwrap();
        cx.simulate_keystrokes("enter");
        cx.simulate_event(gpui::KeyUpEvent { keystroke: enter });
        held
    }

    /// Lets the held write run and its reads land.
    async fn land_write(
        app: &gpui::Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
        held: crate::operations::Held,
    ) {
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(app, cx).await;
        crate::tags::tests::draw(cx);
    }

    /// The staged patch of the fixture, as Git reads it.
    fn staged_patch(fixture: &tempfile::TempDir) -> String {
        let staged = crate::views::tests::fixture_git_output(
            &fixture.path().join("narrow"),
            &["diff", "--cached"],
        );
        String::from_utf8_lossy(&staged.stdout).into_owned()
    }

    /// A partial write pressed in `area` of `path`, notes.txt's `lines`
    /// notes edited as `edits` say (staged first for Staged), in QA's
    /// 1480 × 973 window at 13 pt: the text scrolled so `top` is its first
    /// row, or with `bottom` its last whole row, a click on each of
    /// `select`'s lines and Space, a click on `caret`, the text scrolled so
    /// `then` is its first row where given, Tab and, with the operations
    /// executor held, Enter.
    #[expect(clippy::too_many_arguments)]
    async fn press_lines<'a>(
        cx: &'a mut gpui::TestAppContext,
        lines: usize,
        edits: &[(&str, &str)],
        area: gitturtle_core::ChangeArea,
        path: &str,
        (top, bottom): (&str, bool),
        select: &[&str],
        caret: &str,
        then: Option<&str>,
    ) -> (
        tempfile::TempDir,
        gpui::Entity<GitTurtle>,
        &'a mut gpui::VisualTestContext,
        Route,
        crate::operations::Held,
    ) {
        use crate::views::tests::{fixture_git, settle};
        let (fixture, app, cx) =
            working_diff_edited(cx, &crate::views::tests::history_script(1, lines), edits).await;
        crate::focus_ring::sweep::prepare(&app, cx);
        if area == gitturtle_core::ChangeArea::Staged {
            fixture_git(&fixture.path().join("narrow"), &["add", "--", path]);
            cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
            settle(&app, cx).await;
        }
        let index = app.read_with(cx, |app, _| {
            let status = app.work_status.as_ref().expect("the status");
            status
                .entries
                .iter()
                .position(|entry| entry.path.ends_with(path))
                .expect("the file's row")
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| app.select_working(index, area, window, cx))
        });
        settle(&app, cx).await;
        cx.simulate_resize(gpui::size(gpui::px(1480.), gpui::px(973.)));
        crate::focus_ring::sweep::interface_size(cx, 13);
        let (editor, view) = shown_diff(&app, cx);
        let row = patch_row(&editor, cx, top);
        editor.update(cx, |state, cx| {
            let height = state.line_height().expect("the text is laid out");
            let first = if bottom {
                let shown = (state.input_bounds().size.height / height).floor() as usize;
                (row + 1).saturating_sub(shown)
            } else {
                row
            };
            state.set_scroll_offset(gpui::point(gpui::px(0.), -height * first as f32), cx);
        });
        crate::tags::tests::draw(cx);
        for line in select {
            click_line(&editor, cx, line);
            press(cx, "space");
        }
        assert_eq!(
            view.read_with(cx, |view, _| view.selected_lines()),
            select.len()
        );
        click_line(&editor, cx, caret);
        if let Some(line) = then {
            let row = patch_row(&editor, cx, line);
            editor.update(cx, |state, cx| {
                let height = state.line_height().expect("the text is laid out");
                state.set_scroll_offset(gpui::point(gpui::px(0.), -height * row as f32), cx);
            });
            crate::tags::tests::draw(cx);
        }
        let (caret, scroll) = text_place(&editor, cx);
        let held = press_action(&app, &view, cx);
        (
            fixture,
            app,
            cx,
            Route {
                editor,
                view,
                caret,
                scroll,
            },
            held,
        )
    }

    /// A staged hunk's added lines each keep their own place through an
    /// Unstage: with the caret on the fourth of five, a1 to a5, and the
    /// text scrolled to the second, unstaging a line of another hunk keeps
    /// both, and unstaging the fourth alone puts the caret on the fifth,
    /// the first visible line still the second. Numbered on neither side
    /// HEAD keeps, each landed on the run's first, a1.
    #[gpui::test]
    async fn an_unstage_keeps_each_staged_added_line_in_its_place(cx: &mut gpui::TestAppContext) {
        let notes = notes_with(
            40,
            &[
                (10, "a1"),
                (10, "a2"),
                (10, "a3"),
                (10, "a4"),
                (10, "a5"),
                (30, "b1"),
            ],
            60,
        );
        let (fixture, app, cx, route, held) = press_lines(
            cx,
            40,
            &[("notes.txt", &notes)],
            gitturtle_core::ChangeArea::Staged,
            "notes.txt",
            ("+a2", false),
            &["+b1"],
            "+a4",
            None,
        )
        .await;
        land_write(&app, cx, held).await;
        let staged = staged_patch(&fixture);
        assert!(
            !staged.contains("+b1") && staged.contains("+a4\n+a5\n"),
            "b1 was unstaged: {staged}"
        );
        route.assert_landed(&app, cx, "+a4", "+a2", "Selected changes unstaged");

        press(cx, "space");
        let held = press_action(&app, &route.view, cx);
        land_write(&app, cx, held).await;
        let staged = staged_patch(&fixture);
        assert!(staged.contains("+a3\n+a5\n"), "a4 was unstaged: {staged}");
        route.assert_landed(&app, cx, "+a5", "+a2", "Selected changes unstaged");
    }

    /// A partial write in `area` of notes.txt edited to `notes` (staged
    /// first for Staged), of the run's lines `select` with the caret on
    /// `caret` and, once they are clicked, the text scrolled so `top` is
    /// its first row: once landed, the index's patch shows `written`, the
    /// caret is on `landed.0` and `landed.1` is the first visible line.
    async fn land_in_a_run(
        cx: &mut gpui::TestAppContext,
        area: gitturtle_core::ChangeArea,
        notes: &str,
        (select, caret, top): (&[&str], &str, &str),
        written: &str,
        landed: (&str, &str),
    ) {
        let said = if area == gitturtle_core::ChangeArea::Staged {
            "Selected changes unstaged"
        } else {
            "Selected changes staged"
        };
        let (fixture, app, cx, route, held) = press_lines(
            cx,
            40,
            &[("notes.txt", notes)],
            area,
            "notes.txt",
            (select[0], false),
            select,
            caret,
            Some(top),
        )
        .await;
        land_write(&app, cx, held).await;
        let staged = staged_patch(&fixture);
        assert!(staged.contains(written), "{written:?} in {staged}");
        route.assert_landed(&app, cx, landed.0, landed.1, said);
    }

    /// notes.txt with five added lines, a1 to a5, after note 10 and b1
    /// after note 30, all staged: a1 to a5 are a run of rows numbered on
    /// neither side an Unstage leaves alone.
    fn staged_run() -> String {
        notes_with(
            40,
            &[
                (10, "a1"),
                (10, "a2"),
                (10, "a3"),
                (10, "a4"),
                (10, "a5"),
                (30, "b1"),
            ],
            60,
        )
    }

    /// notes.txt with notes 11 to 15 deleted from the worktree and b1 added
    /// after note 30: the Unstaged diff's removed lines, numbered on
    /// neither side a Stage leaves alone, make a run.
    fn removed_run() -> String {
        let notes = notes_with(40, &[(30, "b1")], 60);
        notes
            .lines()
            .filter(|line| !(11..=15).any(|note| *line == format!("note {note}")))
            .map(|line| format!("{line}\n"))
            .collect()
    }

    /// Unstaging a2 of a staged run a1 to a5, with the caret on a4 and a3
    /// the first visible line, keeps both: counted with a2, which the write
    /// took, the caret went to a5 and a4 became the first line.
    #[gpui::test]
    async fn an_unstage_above_the_caret_in_its_run_keeps_its_place(cx: &mut gpui::TestAppContext) {
        land_in_a_run(
            cx,
            gitturtle_core::ChangeArea::Staged,
            &staged_run(),
            (&["+a2"], "+a4", "+a3"),
            "+a1\n+a3\n+a4\n+a5\n",
            ("+a4", "+a3"),
        )
        .await;
    }

    /// Unstaging a1 and a2 of a staged run a1 to a5, with the caret and
    /// the first visible line on a2, puts both on a3, the nearest changed
    /// line after it: counted with a1, the caret skipped a3 for a4.
    #[gpui::test]
    async fn an_unstage_through_the_caret_in_its_run_lands_on_the_next_line(
        cx: &mut gpui::TestAppContext,
    ) {
        land_in_a_run(
            cx,
            gitturtle_core::ChangeArea::Staged,
            &staged_run(),
            (&["+a1", "+a2"], "+a2", "+a2"),
            " note 10\n+a3\n+a4\n+a5\n",
            ("+a3", "+a3"),
        )
        .await;
    }

    /// Unstaging a1 to a3 of a staged run a1 to a5, with the caret and the
    /// first visible line on a3, puts both on a4: counted with a1 and a2,
    /// the caret's place was past the run's last row, and it went to the
    /// next hunk's b1.
    #[gpui::test]
    async fn an_unstage_of_a_runs_start_lands_on_the_line_it_leaves(cx: &mut gpui::TestAppContext) {
        land_in_a_run(
            cx,
            gitturtle_core::ChangeArea::Staged,
            &staged_run(),
            (&["+a1", "+a2", "+a3"], "+a3", "+a3"),
            " note 10\n+a4\n+a5\n",
            ("+a4", "+a4"),
        )
        .await;
    }

    /// The Unstage cases' Stage: staging note 12's deletion of an unstaged
    /// run of deleted notes 11 to 15, with the caret on note 14 and note 13
    /// first, keeps both.
    #[gpui::test]
    async fn a_stage_above_the_caret_in_its_run_keeps_its_place(cx: &mut gpui::TestAppContext) {
        land_in_a_run(
            cx,
            gitturtle_core::ChangeArea::Unstaged,
            &removed_run(),
            (&["-note 12"], "-note 14", "-note 13"),
            " note 11\n-note 12\n note 13\n",
            ("-note 14", "-note 13"),
        )
        .await;
    }

    /// Staging the deletions of notes 11 and 12, with the caret and the
    /// first visible line on note 12, puts both on note 13.
    #[gpui::test]
    async fn a_stage_through_the_caret_in_its_run_lands_on_the_next_line(
        cx: &mut gpui::TestAppContext,
    ) {
        land_in_a_run(
            cx,
            gitturtle_core::ChangeArea::Unstaged,
            &removed_run(),
            (&["-note 11", "-note 12"], "-note 12", "-note 12"),
            " note 10\n-note 11\n-note 12\n note 13\n",
            ("-note 13", "-note 13"),
        )
        .await;
    }

    /// Staging the deletions of notes 11 to 13, with the caret and the
    /// first visible line on note 13, puts both on note 14, never on the
    /// next hunk's b1.
    #[gpui::test]
    async fn a_stage_of_a_runs_start_lands_on_the_line_it_leaves(cx: &mut gpui::TestAppContext) {
        land_in_a_run(
            cx,
            gitturtle_core::ChangeArea::Unstaged,
            &removed_run(),
            (
                &["-note 11", "-note 12", "-note 13"],
                "-note 13",
                "-note 13",
            ),
            " note 10\n-note 11\n-note 12\n-note 13\n note 14\n",
            ("-note 14", "-note 14"),
        )
        .await;
    }

    /// A staged new file, whose every line is added and so numbered on
    /// neither side HEAD keeps, keeps its caret and first visible line
    /// through an Unstage of a line below them: they landed at the file's
    /// top.
    #[gpui::test]
    async fn an_unstage_in_a_staged_new_file_keeps_the_caret_and_scroll(
        cx: &mut gpui::TestAppContext,
    ) {
        let fresh: String = (1..=80).map(|line| format!("fresh {line}\n")).collect();
        let notes = notes_with(40, &[(5, "edited")], 0);
        let (fixture, app, cx, route, held) = press_lines(
            cx,
            40,
            &[("notes.txt", &notes), ("zz-fresh.txt", &fresh)],
            gitturtle_core::ChangeArea::Staged,
            "zz-fresh.txt",
            ("+fresh 20", false),
            &["+fresh 40"],
            "+fresh 30",
            None,
        )
        .await;
        land_write(&app, cx, held).await;
        let staged = staged_patch(&fixture);
        assert!(
            staged.contains("+fresh 39\n+fresh 41\n"),
            "fresh 40 was unstaged: {staged}"
        );
        route.assert_landed(
            &app,
            cx,
            "+fresh 30",
            "+fresh 20",
            "Selected changes unstaged",
        );
    }

    /// The caret that a landing moves to the nearest changed line is drawn
    /// in view, revealed by the least scroll: unstaging the line at the
    /// caret, the text's last whole row, moves it to the next hunk's line,
    /// seven rows below, which the kept first visible line left out of
    /// view, where the next Space would select a line the person cannot
    /// see.
    #[gpui::test]
    async fn a_landed_caret_out_of_view_is_revealed(cx: &mut gpui::TestAppContext) {
        let mut inserted: Vec<(usize, String)> =
            (1..=60).map(|line| (10, format!("p{line}"))).collect();
        inserted.extend([
            (100, "keep".to_owned()),
            (101, "lone".to_owned()),
            (190, "far".to_owned()),
        ]);
        let inserted: Vec<(usize, &str)> = inserted
            .iter()
            .map(|(after, line)| (*after, line.as_str()))
            .collect();
        let notes = notes_with(200, &inserted, 0);
        let (fixture, app, cx, route, held) = press_lines(
            cx,
            200,
            &[("notes.txt", &notes)],
            gitturtle_core::ChangeArea::Staged,
            "notes.txt",
            ("+lone", true),
            &["+lone"],
            "+lone",
            None,
        )
        .await;
        let height = route
            .editor
            .read_with(cx, |state, _| state.line_height().expect("laid out"));
        let room = route
            .editor
            .read_with(cx, |state, _| state.input_bounds().size.height);
        let line = route.scroll.y + height * route.caret as f32;
        assert!(
            line + height <= room && line + height * 8. > room,
            "the caret's line is among the last whole rows before the write"
        );
        land_write(&app, cx, held).await;
        assert!(
            !staged_patch(&fixture).contains("+lone"),
            "lone was unstaged"
        );
        let (row, scroll) = text_place(&route.editor, cx);
        assert_eq!(
            row,
            patch_row(&route.editor, cx, "+far"),
            "the caret's line"
        );
        let text = route.editor.read_with(cx, |state, _| state.input_bounds());
        let line = scroll.y + height * row as f32;
        assert!(
            line >= gpui::px(0.) && line + height <= text.size.height,
            "the caret's line is whole in the text: {line:?} of {:?}",
            text.size.height
        );
        assert!(
            line + height * 2. > text.size.height,
            "revealed by the least scroll, its last whole row"
        );
        let mark = cx
            .debug_bounds("partial-line-caret")
            .expect("the caret's mark is drawn");
        assert!(
            mark.top() >= text.top() && mark.bottom() <= text.bottom(),
            "the caret's mark is in view: {mark:?} in {text:?}"
        );
    }

    /// What the window last built the shown diff's partial controls saying,
    /// the bar's action and Clear, the Stage hunk on `hunk` and the
    /// checkbox on `line`: whether each is announced unavailable and
    /// whether it looks it (`workspace::refusals`).
    fn partial_refusals(hunk: usize, line: usize) -> Vec<Option<(bool, bool)>> {
        [
            "apply-partial-selection".to_owned(),
            "clear-partial-selection".to_owned(),
            format!("partial-hunk-{hunk}"),
            format!("partial-line-{line}"),
        ]
        .iter()
        .map(|key| {
            crate::workspace::refusals::get(key).map(|refusal| (refusal.announced, refusal.dimmed))
        })
        .collect()
    }

    /// A partial write that lands within the window's busy-look delay dims
    /// nothing, as DESIGN.md has every write: the bar's action, Clear, the
    /// line checkboxes and Stage hunk refuse a press at once and are
    /// announced unavailable at once, but they look it only once the window
    /// looks busy, 200 ms into the write. Presses 100 ms in, a hunk's Stage,
    /// a line's checkbox, Space and the action from the keyboard, do
    /// nothing: the selection, the caret, focus and the one write stay.
    /// Every control dimmed as the write was pressed, so each quick Stage
    /// flashed them disabled.
    #[gpui::test]
    async fn a_partial_write_dims_its_controls_only_once_it_looks_busy(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::partial_view::PartialRow;
        let (fixture, app, cx, route, held) =
            Route::press(cx, gitturtle_core::ChangeArea::Unstaged, &landing_notes()).await;
        let hunk = app.read_with(cx, |app, _| {
            let Some(Content::Text {
                partial: Some(partial),
                ..
            }) = app.content.as_deref()
            else {
                panic!("the diff offers its partial actions");
            };
            (route.caret..partial.rows.len())
                .find(|row| matches!(partial.rows[*row], PartialRow::Hunk(_)))
                .expect("the tail's hunk")
        });
        let twelve = patch_row(&route.editor, cx, "+twelve");
        crate::workspace::refusals::forget();
        crate::tags::tests::draw(cx);
        assert_eq!(
            partial_refusals(hunk, twelve),
            [Some((true, false)); 4],
            "the press: refused and announced so, nothing dimmed"
        );
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(100));
        native_frames(cx);
        for selector in [
            format!("partial-hunk-{hunk}"),
            format!("partial-line-{twelve}"),
        ] {
            let control = cx
                .debug_bounds(selector.clone().leak())
                .unwrap_or_else(|| panic!("{selector} is drawn"));
            cx.simulate_click(control.center(), gpui::Modifiers::default());
            native_frames(cx);
        }
        press(cx, "space");
        press(cx, "tab");
        press(cx, "enter");
        press(cx, "shift-tab");
        native_frames(cx);
        crate::workspace::refusals::forget();
        crate::tags::tests::draw(cx);
        route.assert_kept(&app, cx, "Staging selected changes…", "100 ms in");
        assert_eq!(
            partial_refusals(hunk, twelve),
            [Some((true, false)); 4],
            "100 ms in: refused, nothing dimmed"
        );
        // The frame the window asks for as it looks busy.
        crate::workspace::refusals::forget();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(100));
        native_frames(cx);
        assert_eq!(
            partial_refusals(hunk, twelve),
            [Some((true, true)); 4],
            "200 ms in, as the window looks busy: dimmed"
        );
        land_write(&app, cx, held).await;
        let staged = staged_patch(&fixture);
        assert!(
            staged.contains("+added one\n+added two\n")
                && !staged.contains("twelve")
                && !staged.contains("tail"),
            "one write staged the two lines alone: {staged}"
        );
    }

    /// The node the line checkbox on `row` writes, as `view` builds it now:
    /// the kit's Checkbox laid out to its base element, whose node it is,
    /// as `editor_find`'s tests read Match case's.
    fn line_node(
        view: &gpui::Entity<crate::diff_view::DiffView>,
        row: usize,
        cx: &mut gpui::VisualTestContext,
    ) -> gpui::accesskit::Node {
        let view = view.clone();
        crate::views::tests::view_node(cx, move |window, cx| {
            use gpui::{Element as _, RenderOnce as _};
            fn of_type<'a, E: gpui::Element>(
                _witness: &E,
                element: &'a mut gpui::AnyElement,
            ) -> Option<&'a mut E> {
                element.downcast_mut()
            }
            let checkbox = view
                .read(cx)
                .line_checkbox_at(row)
                .expect("a changed line's checkbox");
            let mut outer = checkbox.render(window, cx).into_element();
            let (_, mut layout) = outer.request_layout(None, None, window, cx);
            let mut base = (&mut layout as &mut dyn std::any::Any)
                .downcast_mut::<Option<gpui::AnyElement>>()
                .and_then(Option::take)
                .expect("the kit's Checkbox renders a base Checkbox");
            let witness = gpui_kit::base::Checkbox::new("witness")
                .render(window, cx)
                .into_element();
            let element = of_type(&witness, &mut base).expect("the base element writes the node");
            let mut node = gpui::accesskit::Node::new(element.a11y_role().expect("a role"));
            element.write_a11y_info(&mut node);
            node
        })
    }

    /// The edge the line checkbox on `row` draws its box with, as the last
    /// frame painted it.
    fn line_edge(cx: &mut gpui::VisualTestContext, row: usize) -> gpui::Hsla {
        let cell = cx
            .debug_bounds(format!("partial-line-{row}").leak())
            .expect("the checkbox is drawn");
        cx.update(|window, _| {
            let scale = window.scale_factor();
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| quad.border_widths.left.as_f32() > 0.)
                .find(|quad| {
                    let (origin, extent) = (quad.bounds.origin, quad.bounds.size);
                    cell.contains(&gpui::point(
                        gpui::px((origin.x.as_f32() + extent.width.as_f32() / 2.) / scale),
                        gpui::px((origin.y.as_f32() + extent.height.as_f32() / 2.) / scale),
                    ))
                })
                .expect("the checkbox's box is painted")
                .border_color
        })
    }

    /// A line's checkbox refuses a press from the press of a partial write
    /// and is announced unavailable from then too, as the bar's action,
    /// Clear and Stage hunk are, while it looks available until the window
    /// looks busy, 200 ms in, as DESIGN.md has every control a write
    /// refuses: through the delay its node is disabled (on Linux neither
    /// enabled nor sensitive to AT-SPI) and its box keeps the enabled edge;
    /// then it dims, still disabled, and once the write lands it is
    /// available again. The kit's Checkbox reported no unavailable state
    /// apart from its look, so it was announced available through the
    /// delay, and even dimmed.
    #[gpui::test]
    async fn a_line_checkbox_is_announced_unavailable_before_it_dims(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui_kit::component::ActiveTheme as _;
        let (_fixture, app, cx, route, held) =
            Route::press(cx, gitturtle_core::ChangeArea::Unstaged, &landing_notes()).await;
        let (_, twelve) = route_controls(&app, &route, cx);
        let refusal = |row: usize| {
            crate::workspace::refusals::get(&format!("partial-line-{row}"))
                .map(|refusal| (refusal.announced, refusal.dimmed))
        };
        let enabled = cx.update(|_, cx| cx.theme().input);
        crate::workspace::refusals::forget();
        crate::tags::tests::draw(cx);
        assert_eq!(
            refusal(twelve),
            Some((true, false)),
            "the press: announced unavailable, not dimmed"
        );
        let node = line_node(&route.view, twelve, cx);
        assert_eq!(node.role(), gpui::accesskit::Role::CheckBox);
        assert!(node.is_disabled(), "its node is disabled through the delay");
        #[cfg(target_os = "linux")]
        {
            use crate::native_accessibility::atspi_state_tests::IgnoreCallbacks;
            use accesskit_atspi_common::{Adapter, AppContext, State, WindowBounds};
            use gpui::accesskit::{Node, NodeId, Role, Tree, TreeId, TreeUpdate};
            let mut window = Node::new(Role::Window);
            window.set_children(vec![NodeId(1)]);
            let adapter = Adapter::new(
                &AppContext::new(None),
                IgnoreCallbacks,
                TreeUpdate {
                    nodes: vec![(NodeId(0), window), (NodeId(1), node.clone())],
                    tree: Some(Tree::new(NodeId(0))),
                    tree_id: TreeId::ROOT,
                    focus: NodeId(0),
                },
                true,
                WindowBounds::default(),
                IgnoreCallbacks,
            );
            let root = adapter.platform_node(adapter.root_id());
            let checkbox = root.child_at_index(0).unwrap().unwrap();
            let state = adapter.platform_node(checkbox).state();
            assert!(
                !state.contains(State::Enabled) && !state.contains(State::Sensitive),
                "AT-SPI hears the checkbox unavailable: {state:?}"
            );
        }
        assert_eq!(
            line_edge(cx, twelve),
            enabled,
            "drawn undimmed through the delay"
        );
        crate::workspace::refusals::forget();
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        native_frames(cx);
        assert_eq!(
            refusal(twelve),
            Some((true, true)),
            "dimmed once the window looks busy"
        );
        assert!(line_node(&route.view, twelve, cx).is_disabled());
        assert_eq!(
            line_edge(cx, twelve),
            enabled.opacity(0.5),
            "drawn dimmed once the window looks busy"
        );
        land_write(&app, cx, held).await;
        let twelve = patch_row(&route.editor, cx, "+twelve");
        assert_eq!(
            refusal(twelve),
            Some((false, false)),
            "available once landed"
        );
        assert!(!line_node(&route.view, twelve, cx).is_disabled());
    }

    /// The Stage hunk on the tail's hunk and the checkbox on "+twelve" of
    /// the Route's diff, as [`partial_refusals`] reads them.
    fn route_controls(
        app: &gpui::Entity<GitTurtle>,
        route: &Route,
        cx: &mut gpui::VisualTestContext,
    ) -> (usize, usize) {
        use crate::partial_view::PartialRow;
        let hunk = app.read_with(cx, |app, _| {
            let Some(Content::Text {
                partial: Some(partial),
                ..
            }) = app.content.as_deref()
            else {
                panic!("the diff offers its partial actions");
            };
            (route.caret..partial.rows.len())
                .find(|row| matches!(partial.rows[*row], PartialRow::Hunk(_)))
                .expect("the tail's hunk")
        });
        (hunk, patch_row(&route.editor, cx, "+twelve"))
    }

    /// Lets the held write and its status read land, the reader held so
    /// the diff's read after them waits: returns the reader's hold.
    fn land_the_status_alone(
        app: &gpui::Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
        held: crate::operations::Held,
    ) -> crate::worker::ReaderHeld {
        let reads = app.read_with(cx, |app, _| {
            app.worker.wait_idle();
            app.worker.hold()
        });
        drop(held);
        for _ in 0..2 {
            app.read_with(cx, |app, _| app.operations.drain());
            cx.run_until_parked();
        }
        app.read_with(cx, |app, cx| {
            assert_eq!(app.operation_busy, None, "the write has ended");
            assert!(app.status_task.is_none(), "its status read has landed");
            assert!(app.lands_partial_write(cx), "the diff's read waits");
        });
        reads
    }

    /// Once the window has looked busy through a partial write, the
    /// write's controls stay dimmed until its diff lands: the status read
    /// after the write ends the window's busy spell while the diff's read
    /// still runs, and there the bar's action, Clear, the line checkboxes
    /// and Stage hunk, which still refused a press, came back looking
    /// available, as DESIGN.md has a busy look that has shown stay until
    /// the work it shows ends.
    #[gpui::test]
    async fn a_partial_writes_dimmed_controls_stay_dimmed_until_it_lands(
        cx: &mut gpui::TestAppContext,
    ) {
        let (_fixture, app, cx, route, held) =
            Route::press(cx, gitturtle_core::ChangeArea::Unstaged, &landing_notes()).await;
        let (hunk, twelve) = route_controls(&app, &route, cx);
        native_frames(cx);
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        native_frames(cx);
        crate::workspace::refusals::forget();
        crate::tags::tests::draw(cx);
        assert_eq!(
            partial_refusals(hunk, twelve),
            [Some((true, true)); 4],
            "dimmed as the window looks busy"
        );
        let reads = land_the_status_alone(&app, cx, held);
        native_frames(cx);
        crate::workspace::refusals::forget();
        crate::tags::tests::draw(cx);
        assert!(
            !app.read_with(cx, |app, _| app.busy_shown()),
            "the window's busy spell has ended"
        );
        assert_eq!(
            partial_refusals(hunk, twelve),
            [Some((true, true)); 4],
            "still dimmed while the diff's read runs"
        );
        drop(reads);
        crate::views::tests::settle(&app, cx).await;
        crate::tags::tests::draw(cx);
        route.assert_landed(
            &app,
            cx,
            " added two",
            " added one",
            "Selected changes staged",
        );
    }

    /// The read of the diff after a partial write that the reader drops
    /// unanswered ends the landing as a failed one does: the view goes,
    /// "Preview unavailable" says why in its place, and focus, here on the
    /// bar's Clear, which Tab reaches while the write runs, goes to the
    /// list. Dropped, the read once took the view away with nothing said,
    /// and focus on the bar fell to the window frame.
    #[gpui::test]
    async fn a_dropped_read_after_a_partial_write_says_why_and_focuses_the_list(
        cx: &mut gpui::TestAppContext,
    ) {
        let (_fixture, app, cx, route, held) =
            Route::press(cx, gitturtle_core::ChangeArea::Unstaged, &landing_notes()).await;
        press(cx, "tab tab");
        let clear = route
            .view
            .read_with(cx, |view, _| view.clear_focus_handle());
        assert!(
            cx.update(|window, _| clear.is_focused(window)),
            "Tab reaches Clear during the write"
        );
        let reads = land_the_status_alone(&app, cx, held);
        app.read_with(cx, |app, _| app.worker.cancel());
        drop(reads);
        crate::views::tests::settle(&app, cx).await;
        crate::tags::tests::draw(cx);
        app.read_with(cx, |app, cx| {
            assert!(!app.lands_partial_write(cx), "nothing waits on the view");
            assert!(app.patch_view.is_none(), "the view went with its landing");
            assert_eq!(
                app.error.as_deref(),
                Some(crate::workspace::LANDING_INTERRUPTED),
                "the dropped read is said"
            );
        });
        let body = crate::native_accessibility::written::get("empty-state:Preview unavailable")
            .expect("the failure takes the view's place");
        assert!(
            body.node.value().is_some_and(
                |text| text.contains("The diff could not be read again. Refresh to retry.")
            ),
            "and says why, under the heading that already says preview: {:?}",
            body.node.value()
        );
        let list = app.read_with(cx, |app, _| app.file_focus.clone());
        assert!(
            cx.update(|window, _| list.is_focused(window)),
            "focus goes to the list, not to {:?}",
            cx.update(|window, cx| window.focused(cx))
        );
    }

    /// The bar's action, which Tab reaches while the write runs, goes with
    /// the line selection as the write lands: focus there goes to the text,
    /// whose landed caret and first visible line the route keeps. Left on
    /// the action, focus fell to the window frame.
    #[gpui::test]
    async fn a_landing_gives_focus_on_the_bar_to_the_text(cx: &mut gpui::TestAppContext) {
        let (_fixture, app, cx, route, held) =
            Route::press(cx, gitturtle_core::ChangeArea::Unstaged, &landing_notes()).await;
        press(cx, "tab");
        let apply = route
            .view
            .read_with(cx, |view, _| view.apply_focus_handle());
        assert!(
            cx.update(|window, _| apply.is_focused(window)),
            "Tab reaches the bar's action during the write"
        );
        land_write(&app, cx, held).await;
        route.assert_landed(
            &app,
            cx,
            " added two",
            " added one",
            "Selected changes staged",
        );
    }

    /// The read of the diff after a partial write that fails, here as the
    /// file can no longer be read, ends the write's landing: the failure is
    /// said in the Diff view's place, "Preview unavailable" and why, focus
    /// goes to the list, as a read of the row has it, and nothing waits on
    /// the view. Kept waiting, the view's controls stayed refused, focus
    /// fell to the window frame as the failure took the view's place, and
    /// the next Refresh landed in the view.
    #[cfg(unix)]
    #[gpui::test]
    async fn a_failed_read_after_a_partial_write_says_why_and_focuses_the_list(
        cx: &mut gpui::TestAppContext,
    ) {
        fail_the_landing(cx, None).await;
    }

    /// [`a_failed_read_after_a_partial_write_says_why_and_focuses_the_list`]
    /// with focus on the bar's action, which Tab reaches while the write
    /// runs: focus goes to the list too. Only focus in the text went there,
    /// and focus on the bar fell to the window frame with the view.
    #[cfg(unix)]
    #[gpui::test]
    async fn a_failed_read_after_a_partial_write_takes_focus_from_the_bar_to_the_list(
        cx: &mut gpui::TestAppContext,
    ) {
        fail_the_landing(cx, Some("tab")).await;
    }

    /// QA's route, `tabs` pressed while the write is held, the write let
    /// land and the read of the diff after it failed, as the file can no
    /// longer be read: asserts the failure said in the view's place and
    /// focus on the list.
    #[cfg(unix)]
    async fn fail_the_landing(cx: &mut gpui::TestAppContext, tabs: Option<&str>) {
        use std::os::unix::fs::PermissionsExt;
        let (fixture, app, cx, route, held) =
            Route::press(cx, gitturtle_core::ChangeArea::Unstaged, &landing_notes()).await;
        if let Some(tabs) = tabs {
            press(cx, tabs);
            let apply = route
                .view
                .read_with(cx, |view, _| view.apply_focus_handle());
            assert!(
                cx.update(|window, _| apply.is_focused(window)),
                "Tab reaches the bar's action during the write"
            );
        }
        let reads = app.read_with(cx, |app, _| {
            app.worker.wait_idle();
            app.worker.hold()
        });
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        cx.run_until_parked();
        let notes = fixture.path().join("narrow").join("notes.txt");
        std::fs::set_permissions(&notes, std::fs::Permissions::from_mode(0o000)).unwrap();
        drop(reads);
        crate::views::tests::settle(&app, cx).await;
        crate::tags::tests::draw(cx);
        std::fs::set_permissions(&notes, std::fs::Permissions::from_mode(0o644)).unwrap();
        let staged = staged_patch(&fixture);
        assert!(
            staged.contains("+added one\n+added two\n"),
            "the write staged: {staged}"
        );
        let error = app.read_with(cx, |app, cx| {
            assert_eq!(app.operation_busy, None, "the write has ended");
            assert!(app.operation_error.is_none(), "{:?}", app.operation_error);
            assert!(!app.lands_partial_write(cx), "nothing waits on the view");
            assert!(app.patch_view.is_none(), "the view went with its landing");
            app.error.clone().expect("the read failed")
        });
        let body = crate::native_accessibility::written::get("empty-state:Preview unavailable")
            .expect("the failure takes the view's place");
        assert!(
            body.node.value().is_some_and(|text| text.contains(&error)),
            "and says why: {:?} for {error:?}",
            body.node.value()
        );
        let list = app.read_with(cx, |app, _| app.file_focus.clone());
        assert!(
            cx.update(|window, _| list.is_focused(window)),
            "focus goes to the list, not to {:?}",
            cx.update(|window, cx| window.focused(cx))
        );
    }

    /// A press the window refuses, as one on a diff that is no longer its
    /// own, ends the view's wait for a write's read: the window's next
    /// Refresh reads the row afresh and builds its view anew, as for any
    /// diff. Left waiting, the view kept its text through that Refresh,
    /// whose read then landed in it in place.
    #[gpui::test]
    async fn a_refused_partial_press_leaves_refresh_to_rebuild_the_view(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::partial_view::PartialRow;
        let (fixture, app, cx) = working_diff(cx).await;
        let (_, view) = shown_diff(&app, cx);
        let hunk = app.read_with(cx, |app, _| {
            let Some(Content::Text {
                partial: Some(partial),
                ..
            }) = app.content.as_deref()
            else {
                panic!("the diff offers its partial actions");
            };
            partial
                .rows
                .iter()
                .position(|row| matches!(row, PartialRow::Hunk(_)))
                .expect("a hunk")
        });
        native_frames(cx);
        let stage = cx
            .debug_bounds(format!("partial-hunk-{hunk}").leak())
            .expect("the hunk's Stage");
        // The press arrives for a repository the window no longer shows.
        let path = app.update(cx, |app, _| {
            app.path.replace(fixture.path().join("another repository"))
        });
        cx.simulate_click(stage.center(), gpui::Modifiers::default());
        app.update(cx, |app, _| app.path = path);
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            None,
            "the press starts nothing"
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert!(
                app.patch_view.as_ref().is_some_and(|shown| *shown != view),
                "Refresh builds the view anew"
            );
        });
    }

    #[test]
    fn change_navigation_wraps_and_accumulates_before_layout() {
        assert_eq!(next_change(&[], None, 0, true), None);
        let rows = [4, 20, 80];
        assert_eq!(next_change(&rows, None, 10, true), Some(1));
        assert_eq!(next_change(&rows, None, 10, false), Some(0));
        assert_eq!(next_change(&rows, Some(1), 10, true), Some(2));
        assert_eq!(next_change(&rows, Some(2), 10, true), Some(0));
        assert_eq!(next_change(&rows, Some(0), 10, false), Some(2));
    }
}
