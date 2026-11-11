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
