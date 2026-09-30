//! Bounded, presentation-only review of a captured text comparison. Every option
//! is rebuilt from the original snapshot on the replaceable read worker.
use crate::{
    AppPage, GitTurtle, TextMode, WorkspaceMode,
    appearance::{self, palette},
    split_diff::{self, Row, SplitPresentation},
    text::PatchPresentation,
    views::compact_control_name,
    worker::{Content, Job},
};
use anyhow::{Result, ensure};
use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Pixels, SharedString,
    StatefulInteractiveElement, Styled, Window,
    component::{
        Disableable, Icon, Sizable,
        button::{Button, ButtonVariants},
        menu::{DropdownMenu, PopupMenuItem},
        tooltip::Tooltip,
    },
    div, percentage, rgb,
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
        self.rebind_text_partial(window, cx);
        self.content = Some(content);
        if self.page == AppPage::Repository
            && matches!(self.mode, WorkspaceMode::Compare | WorkspaceMode::Working)
        {
            self.ensure_editor(window, cx);
        }
        cx.notify();
    }

    pub(super) fn rebind_text_partial(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(view) = self.patch_view.clone() {
            let generation = self.generation;
            let path = self.path.clone();
            self.partial_subscription = Some(cx.subscribe_in(
                &view,
                window,
                move |this, _, event, window, cx| {
                    if this.generation != generation
                        || this.path != path
                        || this.mode != WorkspaceMode::Working
                    {
                        return;
                    }
                    let crate::diff_view::DiffViewEvent::ApplyPartial { diff, selection } = event;
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
                },
            ));
        }
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
    /// and the diff keeps its height.
    pub(super) fn render_text_review(
        &self,
        column: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = palette(cx);
        let options = self.review.options;
        let busy = self.loading.is_some() || self.operation_busy.is_some();
        let has_changes = matches!(self.content.as_deref(), Some(Content::Text { presentation, .. }) if !presentation.change_rows.is_empty());
        let next_context = CONTEXT_STEPS.iter().copied().find(|&n| n > options.context);
        let modified = options != Options::default();
        let reset_disabled = busy || (!modified && self.error.is_none());
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
        let caption_text = if options.hide_whitespace {
            "Review filter active · exact source retained"
        } else if modified {
            "Expanded context · partial staging unavailable"
        } else {
            "Original Git diff"
        };
        let caption = |text: AnyElement| {
            div()
                .pl_2()
                .text_size(crate::appearance::ui_text(11.))
                .text_color(rgb(if modified { colors.hunk } else { colors.muted }))
                .child(text)
        };
        let previous = Button::new("previous-text-change")
            .debug_selector(|| "previous-text-change".into())
            .small()
            .ghost()
            .tooltip("Previous change · Option/Alt-Up")
            .disabled(busy || !has_changes)
            .on_click(
                cx.listener(|this, _, window, cx| this.navigate_text_change(false, window, cx)),
            );
        let next = Button::new("next-text-change")
            .debug_selector(|| "next-text-change".into())
            .small()
            .ghost()
            .tooltip("Next change · Option/Alt-Down")
            .disabled(busy || !has_changes)
            .on_click(
                cx.listener(|this, _, window, cx| this.navigate_text_change(true, window, cx)),
            );
        if column < options_row_width(window) {
            // The app's arrow, turned up and down.
            let arrow = |turn: f32| {
                Icon::default()
                    .path("icons/arrow-left.svg")
                    .size(appearance::ui_size(14.))
                    .rotate(percentage(turn))
            };
            let owner = cx.entity().downgrade();
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
            .dropdown_menu(move |menu, _, _| {
                // Built when opened; each item acts on the review as it
                // is when chosen, as the buttons of the full row do.
                options_menu(options, busy, reset_disabled)
                    .into_iter()
                    .fold(menu, |menu, item| {
                        let owner = owner.clone();
                        menu.item(
                            PopupMenuItem::new(item.label)
                                .checked(item.checked)
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
            });
            // The caption truncates here; its tooltip keeps the whole text.
            let caption_text = SharedString::from(caption_text);
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
            .child(
                caption(
                    div()
                        .id("text-review-caption")
                        .debug_selector(|| "text-review-caption".into())
                        .truncate()
                        .tooltip({
                            let text = caption_text.clone();
                            move |window, cx| Tooltip::new(text.clone()).build(window, cx)
                        })
                        .child(caption_text)
                        .into_any_element(),
                )
                .flex_1()
                .min_w_0(),
            )
            .into_any_element()
        } else {
            row.child(previous.label("Previous change"))
                .child(next.label("Next change"))
                .child(Button::new("hide-whitespace-changes").debug_selector(|| "hide-whitespace-changes".into()).small().ghost().label(if options.hide_whitespace { "Whitespace hidden" } else { "Hide whitespace" })
                    .toggled(options.hide_whitespace).disabled(busy)
                    .tooltip("Hide whitespace-only changes, including indentation and line endings. Source tabs keep exact content; partial staging requires the original diff.")
                    .on_click(cx.listener(move |this, _, window, cx| this.set_text_review(Options { hide_whitespace: !options.hide_whitespace, ..options }, window, cx))))
                .child(Button::new("expand-text-context").small().ghost().label(format!("Context {} +", options.context))
                    .disabled(busy || next_context.is_none()).tooltip("Expand unchanged context around all hunks: 3, 12, 48, then 192 lines. Split and source tabs contain the full bounded preview.")
                    .on_click(cx.listener(move |this, _, window, cx| { if let Some(context) = next_context { this.set_text_review(Options { context, ..options }, window, cx); } })))
                .child(Button::new("reset-text-review").small().ghost().label("Reset review")
                    .disabled(reset_disabled).mr_neg_2()
                    .on_click(cx.listener(|this, _, window, cx| this.set_text_review(Options::default(), window, cx))))
                .child(caption(div().debug_selector(|| "text-review-caption".into()).child(caption_text).into_any_element()))
                .into_any_element()
        }
    }
}

/// One item of the collapsed review options' menu.
struct MenuItem {
    label: String,
    checked: bool,
    disabled: bool,
    /// The options it chooses, from those current when it is chosen.
    choose: fn(Options) -> Option<Options>,
}

/// The Options menu, as the full row's Hide whitespace, Context and Reset
/// review buttons: whitespace a checked item, context showing its value and
/// disabled at its last step, reset disabled as its button is.
fn options_menu(options: Options, busy: bool, reset_disabled: bool) -> [MenuItem; 3] {
    let next_context = |options: Options| {
        CONTEXT_STEPS
            .iter()
            .copied()
            .find(|&n| n > options.context)
            .map(|context| Options { context, ..options })
    };
    [
        MenuItem {
            label: "Hide whitespace".into(),
            checked: options.hide_whitespace,
            disabled: busy,
            choose: |options| {
                Some(Options {
                    hide_whitespace: !options.hide_whitespace,
                    ..options
                })
            },
        },
        MenuItem {
            label: format!("Context {} +", options.context),
            checked: false,
            disabled: busy || next_context(options).is_none(),
            choose: next_context,
        },
        MenuItem {
            label: "Reset review".into(),
            checked: false,
            disabled: reset_disabled,
            choose: |_| Some(Options::default()),
        },
    ]
}

/// The width the review options take on one row as they first appear, with
/// "Original Git diff": `px_3` on either side, six items `gap_1` apart, and
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
                crate::views::line_widths(
                    &[
                        ("Previous change", label),
                        ("Next change", label),
                        ("Hide whitespace", label),
                        ("Context 3 +", label),
                        ("Reset review", label),
                        ("Original Git diff", appearance::ui_text(11.)),
                    ],
                    window,
                )
            },
        )
}

thread_local! {
    /// The review options' laid-out labels, by font and size.
    pub(crate) static OPTIONS_ROW: std::cell::RefCell<Option<(crate::views::LineKey, Pixels)>> =
        const { std::cell::RefCell::new(None) };
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

    /// The collapsed Options menu carries each full-row button's state and
    /// chooses what that button would.
    #[test]
    fn collapsed_options_menu_keeps_each_buttons_state() {
        let states =
            |menu: [MenuItem; 3]| menu.map(|item| (item.label, item.checked, item.disabled));
        let original = Options::default();
        assert_eq!(
            states(options_menu(original, false, true)),
            [
                ("Hide whitespace".into(), false, false),
                ("Context 3 +".into(), false, false),
                ("Reset review".into(), false, true),
            ]
        );
        let hidden = Options {
            hide_whitespace: true,
            context: 3,
        };
        let [whitespace, context, reset] = options_menu(hidden, false, false);
        assert!(whitespace.checked && !reset.disabled);
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
            (context.label.as_str(), context.disabled),
            ("Context 192 +", true)
        );
        assert_eq!((context.choose)(widest), None);
        assert!(
            options_menu(hidden, true, true)
                .iter()
                .all(|item| item.disabled)
        );
    }
    #[test]
    fn review_removes_actionable_partial_ids_and_reset_retains_the_exact_snapshot() {
        use gitturtle_core::{ChangeArea, GitRepository, TextPreview};
        use std::{fs, process::Command};
        struct Fixture(std::path::PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let fixture = Fixture(std::env::temp_dir().join(format!(
                "gitturtle-text-review-{}-{}",
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
        fs::write(fixture.0.join("file.txt"), "a = 1\nold\n").unwrap();
        git(&["add", "--", "file.txt"]);
        git(&["commit", "--quiet", "-m", "baseline"]);
        fs::write(fixture.0.join("file.txt"), "a=1\nnew\n").unwrap();
        let repo = GitRepository::open(&fixture.0).unwrap();
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
        });
        let before_index = fs::read(fixture.0.join(".git/index")).unwrap();
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
            fs::read(fixture.0.join(".git/index")).unwrap(),
            before_index
        );
        assert_eq!(
            fs::read_to_string(fixture.0.join("file.txt")).unwrap(),
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
        })
    }
    fn patch(content: &Content) -> &str {
        let Content::Text { patch, .. } = content else {
            panic!("text expected")
        };
        patch
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
