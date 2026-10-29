//! The width walk: History, Compare (text and image), Working Changes and
//! the GitHub panel on the offline fixture, laid out at every width from 461
//! to 1,480 px at 13 and 18 pt in a short and a tall window, fail on cut or
//! orphaned text, on a repository tab's name cut while the tab strip's
//! Menu or Workspaces keeps its label, on the Git action bar's repository
//! edge left out, its line short of its text or a control running into the
//! next (DESIGN.md, "One responsive order"), on the selected commit's
//! Actions wrapped off the row of the message toolbar's copy controls, on
//! the navigation's mode toggles wrapped onto a second row or its keyboard
//! hint wrapped inside a phrase, and on a field's placeholder wider than
//! its field. The fixture has no remote, so every page's bar shows that
//! edge; History is walked again on a detached HEAD, on a branch with no
//! upstream, with the navigation at its 180 px minimum, with a search that
//! found nothing, whose line and Clear search must lie wholly inside the
//! list, and with a write's outcome fresh in the status line, which must be
//! drawn whole beside the keyboard hints; History and Working Changes are
//! walked again with the local watch unable to start, whose status bar
//! warning, Refresh and Details… stay whole while the hints give way, and
//! History with both, where the outcome yields only to the warning. The
//! window is resized from width to width as a person or a tiling window
//! manager resizes it, so the inspector is drawn at the size its panel
//! group rescales it to, not only its saved width; History and Working
//! Changes are walked again with the inspector saved at its widest, 480 px,
//! wider than every narrower window allows it, so it gives way to the
//! content column's minimum (DESIGN.md, "One responsive order").
//!
//! The test text system paints no glyphs, so the walk marks what it checks
//! with fills a test can find among the painted quads, each with the content
//! mask its ancestors clip it to. Every text run takes a marked background
//! from the window's root text style, which GPUI paints exactly behind the
//! run's glyphs, ellipsis included: a run that crosses the side of its clip
//! is cut mid-glyph, where a truncated one ends inside it. Every shared
//! control at rest takes a marked fill (`appearance::mark_controls`): one
//! that crosses the side of its clip is cut too. Lists scroll, so a row the
//! bottom of its list cuts is not; the walk checks sides. Code lines scroll
//! sideways in their editor, which is content: the walk checks the chrome
//! around them.
//!
//! The widths step 40 px from 461 to 1,480, and wherever what the window
//! shows changes between two steps (a control drawn as its icon, a label
//! left out, the rail for navigation), the walk bisects to the breakpoint and
//! checks the width on either side of it as well.

use crate::views::shaped_width;
use crate::views::tests::{bounds_of, fixture_git, settle, styled_history_window};
use crate::*;
use core::prelude::v1::test;
use gpui_kit::{ScaledPixels, Styled as _};
use std::collections::BTreeSet;

/// Behind every text run: a hue nothing in the app paints, almost clear.
pub(crate) fn text_mark() -> Hsla {
    hsla(0.123, 0.987, 0.456, 0.004)
}

/// Every shared control's fill at rest.
fn control_mark() -> Hsla {
    hsla(0.789, 0.987, 0.456, 0.004)
}

/// What the walk fails on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Rule {
    /// A text run crosses the side of its clip.
    CutText,
    /// A shared control crosses the side of its clip.
    CutControl,
    /// Compare's path is cut short of the file's whole path.
    PathCut,
    /// A count or caption is drawn without the control it belongs to.
    Orphan,
    /// A repository tab's name is cut while the strip's Menu or Workspaces
    /// keeps its label: actions become icons before content is cut.
    TabNameCut,
    /// The Git action bar's repository edge is not drawn, or its line is
    /// drawn short of its whole text, as "No r…", or a line no wider than
    /// nothing beside a detached HEAD's picker, was at 18 pt in a 461 × 490
    /// half tile: a line is whole or gives way to its glyph.
    StateCut,
    /// A control in the Git action bar runs into the one after it on its
    /// row, as a detached HEAD's "Detached HEAD" picker ran under Fetch.
    Overlap,
    /// The selected commit's Actions is drawn below Hash's row, the last
    /// row of the message toolbar's copy controls, where the toolbar lays
    /// it out (`commit_message::Toolbar`): the toolbar was fitted to another
    /// width than the inspector is drawn at.
    Wrapped,
    /// The navigation's mode toggles are drawn on more than one row, as
    /// "Worktrees" under Local and Remote at 18 pt in a 220 px navigation
    /// and at 13 pt in a 180 px one: actions become their glyphs before
    /// they wrap.
    ModesWrapped,
    /// The navigation's keyboard hint is drawn on more lines than it has,
    /// wrapped inside a phrase, as in a 180 px navigation: it is whole on
    /// its lines or gives way whole.
    HintWrapped,
    /// A field's placeholder is wider than the room the field draws its
    /// text in, so the field's edge cuts it mid-glyph, as the navigation
    /// filter's "Filter branches & worktre" at 18 pt: a placeholder fits in
    /// whole words or ends in an ellipsis. The kit draws a field's text
    /// without the marked background, so the cut-text rule cannot see it.
    PlaceholderCut,
    /// A History search's no-match line or Clear search is not drawn wholly
    /// inside the list and the window, as at 461 × 490, where the search's
    /// labelled controls left the list 2 px and Clear search ended below the
    /// window.
    StateOutside,
    /// A write's fresh outcome in the status line is drawn short of its
    /// whole text beside the keyboard hints, as "Up to date with ori…" at
    /// 18 pt in a 461 × 490 half tile: the outcome comes first, and the
    /// hints keep their keys alone, or leave, while it lasts.
    OutcomeCut,
}

/// The pages the walk lays out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    History,
    /// History with a search that found nothing.
    HistorySearch,
    CompareText,
    CompareImage,
    Working,
    GitHub,
}

/// Where a finding lies: the first of these whose bounds hold the middle of
/// what its clip leaves visible. Compare's controls, then each page's
/// panes from the most to the least specific.
const REGIONS: &[&str] = &[
    "image-system-preview-0",
    "image-system-preview-1",
    "system-preview-0",
    "system-preview-1",
    "preview-file-path",
    "compare-footer",
    "compare-toolbar",
    "code-editor",
    "preview-content",
    "history-scope-toolbar",
    "history-search-controls",
    "history-list-edge",
    "history-column-0",
    "history-column-1",
    "history-column-2",
    "history-column-3",
    "history-column-4",
    "history-column-5",
    "changed-files-header",
    "commit-message-viewport",
    "commit-inspector-files",
    "working-header",
    "working-toolbar",
    "working-pane",
    "commit-composer",
    "github-fixture-mode",
    "github-title",
    "github-panel",
    "repository-tab-strip",
    "repository-header",
    "git-actions",
    "repository-rail",
    "local-refresh-warning",
    "status-hints",
    "history-sidebar",
    "history-pane",
    "files-pane",
];

/// A count or caption, then the controls one of which it needs beside it.
const PAIRS: &[(&str, &[&str])] = &[
    ("changes-count-badge", &["changes-tab"]),
    ("changed-files-count", &["commit-inspector-files"]),
    ("history-scope-count", &["history-scope-toolbar"]),
    ("staged-heading-count", &["unstage-all"]),
    ("unstaged-heading-count", &["stage-all"]),
    ("commit-title-counter", &["commit-title"]),
    ("commit-staged-count", &["commit-staged"]),
    ("working-header-count", &["working-header"]),
];

/// A finding the walk expects until the named follow-up fixes it. The walk
/// fails on a finding no entry names, and on an entry nothing matches, so a
/// fix removes its entry.
struct Known {
    page: Page,
    sizes: &'static [u8],
    rule: Rule,
    region: &'static str,
    /// The follow-up that owns it.
    follow_up: &'static str,
}

const KNOWN: &[Known] = &[];

/// One thing the walk fails on, where and at what size it was found.
#[derive(Clone, Debug)]
struct Finding {
    rule: Rule,
    region: &'static str,
    width: f32,
    height: f32,
    detail: String,
}

/// One frame, as `views::tests::draw` draws three: the walk lays each
/// width out in two, the second after its measured widths settle.
pub(crate) fn frame(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.simulate_next_frame(cx);
        window.draw(cx).clear(cx);
    });
    cx.executor().run_until_parked();
}

/// The keys of the compact forms the last frame named: a control drawn as
/// its icon, a label left out.
fn compact_forms() -> BTreeSet<&'static str> {
    let mut forms: BTreeSet<&'static str> =
        crate::views::COMPACT_NAMES.with(|names| names.borrow().keys().copied().collect());
    crate::views::COMPACT_LABELS.with(|names| forms.extend(names.borrow().keys().copied()));
    forms
}

/// Selectors whose presence tells a page's layouts apart.
const LAYOUT_SELECTORS: &[&str] = &[
    "repository-rail",
    "history-sidebar",
    "changed-files-label",
    "changed-files-count",
    "history-scope-name",
    "history-scope-count",
    "history-search-summary",
    "history-search-controls",
    "text-mode-menu",
    "working-toolbar",
    "working-header-label",
    "commit-description",
    "open-blame",
];

/// What the window shows at a width, as far as its breakpoints go: where it
/// changes between two widths, a breakpoint lies between them.
type Layout = (BTreeSet<&'static str>, Vec<bool>);

/// The walk's state for one page at one text size.
struct Walk<'a> {
    page: Page,
    text: u8,
    app: &'a Entity<GitTurtle>,
    /// The path Compare shows, which stays whole.
    path: Option<&'static str>,
    findings: Vec<Finding>,
    layouts: usize,
}

impl Walk<'_> {
    /// Lays the window out `width` × `height`, records what it finds there and
    /// returns the layout.
    fn lay_out(&mut self, width: f32, height: f32, cx: &mut VisualTestContext) -> Layout {
        self.layouts += 1;
        cx.simulate_resize(size(px(width), px(height)));
        frame(cx);
        // Only what the settled layout names: a frame laid out at the
        // previous width can have named another form.
        crate::views::COMPACT_NAMES.with(|names| names.borrow_mut().clear());
        crate::views::COMPACT_LABELS.with(|names| names.borrow_mut().clear());
        self.app.update(cx, |_, cx| cx.notify());
        frame(cx);
        let layout = (
            compact_forms(),
            LAYOUT_SELECTORS
                .iter()
                .map(|selector| cx.debug_bounds(selector).is_some())
                .collect(),
        );
        self.inspect(width, height, cx);
        layout
    }

    fn found(&mut self, rule: Rule, region: &'static str, at: (f32, f32), detail: String) {
        self.findings.push(Finding {
            rule,
            region,
            width: at.0,
            height: at.1,
            detail,
        });
    }

    /// Records the last frame's cut texts and controls, Compare's cut path
    /// and orphaned counts.
    fn inspect(&mut self, width: f32, height: f32, cx: &mut VisualTestContext) {
        let (text, control) = (
            gpui_kit::Background::from(text_mark()),
            gpui_kit::Background::from(control_mark()),
        );
        let (scale, viewport, quads) = cx.update(|window, _| {
            (
                window.scale_factor(),
                window.viewport_size(),
                window.painted_quads(),
            )
        });
        // Over the GitHub panel, only its own regions name a finding: the
        // page it covers keeps its regions' bounds beneath it.
        let regions: Vec<(&'static str, Bounds<Pixels>)> = REGIONS
            .iter()
            .filter(|region| self.page != Page::GitHub || region.starts_with("github-"))
            .filter_map(|region| cx.debug_bounds(region).map(|bounds| (*region, bounds)))
            .collect();
        let logical = |value: ScaledPixels| px(value.0 / scale);
        let slack = ScaledPixels(0.5);
        // The GitHub panel is a dialog: what it covers is drawn before the
        // last fill that spans the window, its overlay, and is walked on its
        // own page.
        let overlay = (self.page == Page::GitHub)
            .then(|| {
                quads
                    .iter()
                    .filter(|quad| {
                        logical(quad.bounds.size.width) >= viewport.width - px(1.)
                            && logical(quad.bounds.size.height) >= viewport.height - px(1.)
                    })
                    .map(|quad| quad.order)
                    .max()
            })
            .flatten();
        for quad in quads {
            let rule = if quad.background == text {
                Rule::CutText
            } else if quad.background == control {
                Rule::CutControl
            } else {
                continue;
            };
            if overlay.is_some_and(|overlay| quad.order <= overlay) {
                continue;
            }
            let (drawn, clip) = (quad.bounds, quad.content_mask.bounds);
            if drawn.left() >= clip.left() - slack && drawn.right() <= clip.right() + slack {
                continue;
            }
            let shown = drawn.intersect(&clip);
            let middle = point(
                logical(shown.left() + shown.size.width / 2.),
                logical(shown.top() + shown.size.height / 2.),
            );
            let region = regions
                .iter()
                .find(|(_, bounds)| bounds.contains(&middle))
                .map_or("window", |(region, _)| region);
            // Code scrolls sideways in its editor.
            if region == "code-editor" {
                continue;
            }
            let detail = format!(
                "{:.1}..{:.1} beside a clip of {:.1}..{:.1}, {:.1} px down",
                f32::from(logical(drawn.left())),
                f32::from(logical(drawn.right())),
                f32::from(logical(clip.left())),
                f32::from(logical(clip.right())),
                f32::from(logical(drawn.top())),
            );
            self.found(rule, region, (width, height), detail);
        }
        if let Some(path) = self.path {
            let need = cx.update(|window, _| {
                shaped_width(path, appearance::ui_text(12.), FontWeight::MEDIUM, window).ceil()
            });
            let shown = cx
                .debug_bounds("preview-file-path")
                .map_or(px(0.), |bounds| bounds.size.width);
            if shown < need - px(0.5) {
                let detail = format!("{shown:?} for {path:?}, which takes {need:?}");
                self.found(Rule::PathCut, "preview-file-path", (width, height), detail);
            }
        }
        for (caption, controls) in PAIRS {
            if cx.debug_bounds(caption).is_some()
                && !controls
                    .iter()
                    .any(|control| cx.debug_bounds(control).is_some())
            {
                let detail = format!("{caption} without {controls:?}");
                self.found(Rule::Orphan, caption, (width, height), detail);
            }
        }
        if let (Some(hash), Some(actions)) = (
            cx.debug_bounds("copy-commit"),
            cx.debug_bounds("commit-recovery-menu"),
        ) && (actions.top() - hash.top()).abs() > px(0.5)
        {
            let inspector = cx.debug_bounds("commit-inspector");
            let detail = format!("{actions:?} below Hash {hash:?} in {inspector:?}");
            self.found(
                Rule::Wrapped,
                "commit-message-toolbar",
                (width, height),
                detail,
            );
        }
        self.inspect_tabs(width, height, cx);
        self.inspect_navigation(width, height, cx);
        self.inspect_placeholders(width, height, cx);
        self.inspect_edge(width, height, cx);
        self.inspect_outcome(width, height, cx);
        if self.page == Page::HistorySearch {
            self.inspect_search_state(width, height, cx);
        }
    }

    /// Records a fresh outcome in the status line drawn short of its whole
    /// text. A truncated line is whole inside its clip, so the cut-text rule
    /// cannot see it: its box is narrower than the text it says.
    fn inspect_outcome(&mut self, width: f32, height: f32, cx: &mut VisualTestContext) {
        let Some(outcome) = self
            .app
            .read_with(cx, |app, _| app.fresh_outcome().map(str::to_owned))
        else {
            return;
        };
        let need = cx.update(|window, _| {
            let weight = window.text_style().font_weight;
            shaped_width(&outcome, appearance::ui_text(10.), weight, window).ceil()
        });
        let drawn = cx.debug_bounds("status-line").map(|line| line.size.width);
        let yielded = cx.debug_bounds("local-refresh-warning").is_some()
            && cx.debug_bounds("status-hints").is_none()
            && crate::views::COMPACT_NAMES
                .with(|names| names.borrow().contains_key("local-refresh-details"));
        if drawn.is_none_or(|drawn| drawn < need - px(0.5)) && !yielded {
            let detail = format!("{outcome:?} in {drawn:?} of its {need:?}");
            self.found(Rule::OutcomeCut, "status-line", (width, height), detail);
        }
    }

    /// Records the no-match line or Clear search of History's search not
    /// drawn, or drawn past the list's visible part or the window: lists
    /// scroll, but a state stands in the list's place and does not.
    fn inspect_search_state(&mut self, width: f32, height: f32, cx: &mut VisualTestContext) {
        let viewport = cx.update(|window, _| window.viewport_size());
        let Some(list) = cx.debug_bounds("history-empty-state") else {
            let detail = "no state in History's list".into();
            self.found(Rule::StateOutside, "history-pane", (width, height), detail);
            return;
        };
        let slack = px(0.5);
        for part in ["history-no-match-line", "history-clear-search"] {
            let inside = cx.debug_bounds(part).is_some_and(|drawn| {
                drawn.left() >= list.left().max(px(0.)) - slack
                    && drawn.right() <= list.right().min(viewport.width) + slack
                    && drawn.top() >= list.top().max(px(0.)) - slack
                    && drawn.bottom() <= list.bottom().min(viewport.height) + slack
            });
            if !inside {
                let detail = format!("{part} {:?} in {list:?}", cx.debug_bounds(part));
                self.found(Rule::StateOutside, "history-pane", (width, height), detail);
            }
        }
    }

    /// Records the repository edge left out of the Git action bar or its
    /// line drawn short of its whole text, and any of the bar's controls
    /// running into the next on its row. A truncated line is whole inside
    /// its clip, so the cut-text rule cannot see it: its box is narrower
    /// than the text it says.
    fn inspect_edge(&mut self, width: f32, height: f32, cx: &mut VisualTestContext) {
        let at = (width, height);
        if self
            .app
            .read_with(cx, |app, _| app.repository_edge().is_some())
        {
            let drawn = cx.debug_bounds("branch-edge-line");
            let shown = crate::empty_state::shown("branch-edge");
            let need = shown.as_ref().map(|shown| {
                cx.update(|window, _| match shown.strip_prefix("glyph:") {
                    Some(_) => appearance::ui_size(crate::empty_state::GLYPH),
                    None => shaped_width(
                        shown,
                        appearance::ui_text(12.),
                        window.text_style().font_weight,
                        window,
                    )
                    .ceil(),
                })
            });
            match (drawn, need) {
                (Some(drawn), Some(need)) if drawn.size.width < need - px(0.5) => {
                    let detail = format!("{shown:?} in {:?} of its {need:?}", drawn.size.width);
                    self.found(Rule::StateCut, "git-actions", at, detail);
                }
                (Some(_), Some(_)) => {}
                _ => self.found(Rule::StateCut, "git-actions", at, "not drawn".into()),
            }
        }
        let row: Vec<(&str, Bounds<Pixels>)> = [
            "current-branch",
            "branch-edge",
            "fetch",
            "pull",
            "push",
            "edge-create-branch",
            "edge-add-remote",
            "git-actions-toggle",
        ]
        .into_iter()
        .filter_map(|control| cx.debug_bounds(control).map(|bounds| (control, bounds)))
        .collect();
        for pair in row.windows(2) {
            let [(first, a), (second, b)] = pair else {
                continue;
            };
            let same_row = a.top() < b.bottom() && b.top() < a.bottom();
            if same_row && a.right() > b.left() + px(0.5) {
                let detail = format!("{first} {a:?} runs into {second} {b:?}");
                self.found(Rule::Overlap, "git-actions", at, detail);
            }
        }
    }

    /// Records the fixture's repository tab cut short of its whole label
    /// while Menu or Workspaces, drawn, keeps its label (one named in
    /// `views::COMPACT_NAMES` is drawn as its icon). A truncated label is
    /// whole inside its clip, so the cut-text rule cannot see it: the tab
    /// is narrower than its insets and its label's unrounded width.
    fn inspect_tabs(&mut self, width: f32, height: f32, cx: &mut VisualTestContext) {
        let icons = crate::views::COMPACT_NAMES.with(|names| names.borrow().clone());
        let labelled: Vec<&str> = ["main-menu", "repository-workspaces"]
            .into_iter()
            .filter(|control| cx.debug_bounds(control).is_some() && !icons.contains_key(control))
            .collect();
        let Some(tab) = cx.debug_bounds("repository-tab-0") else {
            return;
        };
        if labelled.is_empty() {
            return;
        }
        let (label, whole) = cx.update(|window, cx| {
            let app = self.app.read(cx);
            let (label, _) = app.tab_label(&app.repository_tabs.tabs[0]);
            let text = shaped_width(
                &label,
                appearance::ui_text(12.),
                window.text_style().font_weight,
                window,
            );
            let insets = window.pixel_snap(appearance::ui_size(crate::LABEL_INSET)) * 2.;
            (label, (insets + text).min(appearance::ui_size(220.)))
        });
        if tab.size.width < whole - px(0.5) {
            let detail = format!(
                "{label:?} in {:?} of its {whole:?} beside labelled {labelled:?}",
                tab.size.width
            );
            self.found(
                Rule::TabNameCut,
                "repository-tab-strip",
                (width, height),
                detail,
            );
        }
    }

    /// Records the navigation's mode toggles drawn on more than one row and
    /// its keyboard hint drawn on more lines than it has: each line of text
    /// paints its own marked background.
    fn inspect_navigation(&mut self, width: f32, height: f32, cx: &mut VisualTestContext) {
        let at = (width, height);
        if cx.debug_bounds("history-sidebar").is_none() {
            return;
        }
        let modes: Vec<(&str, Bounds<Pixels>)> =
            ["nav-mode-local", "nav-mode-remote", "nav-mode-worktrees"]
                .into_iter()
                .filter_map(|mode| cx.debug_bounds(mode).map(|bounds| (mode, bounds)))
                .collect();
        if let Some((first, row)) = modes.first().copied() {
            for (mode, bounds) in &modes[1..] {
                if (bounds.top() - row.top()).abs() > px(0.5) {
                    let detail = format!("{mode} {bounds:?} off {first}'s row {row:?}");
                    self.found(Rule::ModesWrapped, "history-sidebar", at, detail);
                }
            }
        }
        let Some(hint) = cx.debug_bounds("navigation-hint") else {
            return;
        };
        let (scale, quads) = cx.update(|window, _| (window.scale_factor(), window.painted_quads()));
        let text = gpui_kit::Background::from(text_mark());
        let lines: BTreeSet<i32> = quads
            .iter()
            .filter(|quad| quad.background == text)
            .map(|quad| {
                let top = px(quad.bounds.top().0 / scale);
                let left = px(quad.bounds.left().0 / scale);
                (top, left)
            })
            .filter(|(top, left)| {
                *top >= hint.top()
                    && *top < hint.bottom()
                    && *left < hint.right()
                    && *left >= hint.left()
            })
            .map(|(top, _)| f32::from(top).round() as i32)
            .collect();
        let whole = crate::views::nav_hint_lines().len();
        if lines.len() > whole {
            let detail = format!("{} lines for its {whole} in {hint:?}", lines.len());
            self.found(Rule::HintWrapped, "history-sidebar", at, detail);
        }
    }

    /// Records a placeholder wider than the room its field draws text in:
    /// the navigation filter's where the navigation is drawn, and History's
    /// search on History.
    fn inspect_placeholders(&mut self, width: f32, height: f32, cx: &mut VisualTestContext) {
        let (navigation, search) = self
            .app
            .read_with(cx, |app, _| (app.nav_search.clone(), app.search.clone()));
        let mut fields = Vec::new();
        if cx.debug_bounds("history-sidebar").is_some() {
            fields.push(("navigation-filter", navigation, 11.));
        }
        if self.page == Page::History {
            fields.push(("history-search", search, 12.));
        }
        for (region, field, text) in fields {
            let (room, shown) = field.read_with(cx, |field, _| {
                (
                    field.text_bounds(),
                    field.presentation().placeholder().clone(),
                )
            });
            let Some(room) = room else {
                continue;
            };
            let need = cx.update(|window, _| {
                shaped_width(
                    &shown,
                    appearance::ui_text(text),
                    window.text_style().font_weight,
                    window,
                )
            });
            if need > room.size.width + px(0.5) {
                let detail = format!("{shown:?} takes {need:?} in {:?}", room.size.width);
                self.found(Rule::PlaceholderCut, region, (width, height), detail);
            }
        }
    }

    /// Walks every width at `height`, bisecting to each breakpoint.
    fn walk(&mut self, height: f32, cx: &mut VisualTestContext) {
        let scaled = |width: f32| f32::from(appearance::ui_size(width)).round();
        // Below a scaled 800 px the header keeps only its icons and History
        // shows the rail; the walk finds the measured breakpoints itself.
        let mut fixed: Vec<f32> = [scaled(800.) - 1., scaled(800.), scaled(800.) + 1.]
            .into_iter()
            .filter(|width| (461. ..=1480.).contains(width))
            .collect();
        let mut widths: Vec<f32> = (0..26).map(|step| 461. + 40. * step as f32).collect();
        widths.push(1480.);
        let mut previous: Option<(f32, Layout)> = None;
        for width in widths {
            let layout = self.lay_out(width, height, cx);
            if let Some((before, earlier)) = previous.take()
                && earlier != layout
            {
                let (mut low, mut high) = (before, width);
                while high - low > 1. {
                    let middle = ((low + high) / 2.).floor();
                    if self.lay_out(middle, height, cx) == earlier {
                        low = middle;
                    } else {
                        high = middle;
                    }
                }
                fixed.extend([low - 1., high + 1.]);
            }
            previous = Some((width, layout));
        }
        fixed.sort_by(f32::total_cmp);
        fixed.dedup();
        for width in fixed {
            self.lay_out(width, height, cx);
        }
    }

    /// Fails on every finding no [`KNOWN`] entry names, then on every entry
    /// for this page and size that nothing matched.
    fn verdict(self) {
        let known: Vec<&Known> = KNOWN
            .iter()
            .filter(|known| known.page == self.page && known.sizes.contains(&self.text))
            .collect();
        let mut matched = vec![false; known.len()];
        let mut unknown: std::collections::BTreeMap<(Rule, &str), Vec<Finding>> =
            Default::default();
        for finding in self.findings {
            match known
                .iter()
                .position(|known| known.rule == finding.rule && known.region == finding.region)
            {
                Some(index) => matched[index] = true,
                None => unknown
                    .entry((finding.rule, finding.region))
                    .or_default()
                    .push(finding),
            }
        }
        let mut report = String::new();
        for ((rule, region), findings) in &unknown {
            let mut widths: Vec<String> = findings
                .iter()
                .map(|finding| format!("{} × {}", finding.width, finding.height))
                .collect();
            widths.dedup();
            report.push_str(&format!(
                "\n  {rule:?} in {region} at {} widths ({}): {}",
                widths.len(),
                widths.join(", "),
                findings[0].detail
            ));
        }
        for (known, matched) in known.iter().zip(matched) {
            if !matched {
                report.push_str(&format!(
                    "\n  no {:?} in {} is left: remove its entry ({})",
                    known.rule, known.region, known.follow_up
                ));
            }
        }
        assert!(
            report.is_empty(),
            "{:?} at {} pt, {} layouts:{report}",
            self.page,
            self.text,
            self.layouts
        );
    }
}

/// A PNG `width` × `height` of one colour.
fn png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(width, height, image::Rgba(color))
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    bytes
}

/// A `git fast-import` stream of 12 commits on `main`: each rewrites
/// `README.md`, the 11th adds `assets/turtle.png` and the newest changes it.
fn fixture_script() -> Vec<u8> {
    let mut input = Vec::new();
    let file = |input: &mut Vec<u8>, path: &str, data: &[u8]| {
        input
            .extend_from_slice(format!("M 100644 inline {path}\ndata {}\n", data.len()).as_bytes());
        input.extend_from_slice(data);
        input.push(b'\n');
    };
    for commit in 0..12 {
        let message = format!("Describe the turtle's walk, step {commit}");
        input.extend_from_slice(
            format!(
                "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> {} +0000\ndata {}\n{message}\n",
                1_700_000_000 + commit,
                message.len()
            )
            .as_bytes(),
        );
        let readme: String = (0..=commit % 7)
            .map(|line| format!("Line {line} of draft {commit}\n"))
            .collect();
        file(&mut input, "README.md", readme.as_bytes());
        if commit == 10 {
            file(
                &mut input,
                "assets/turtle.png",
                &png(48, 32, [40, 160, 120, 255]),
            );
        } else if commit == 11 {
            file(
                &mut input,
                "assets/turtle.png",
                &png(64, 40, [200, 120, 40, 255]),
            );
        }
    }
    input.extend_from_slice(b"done\n");
    input
}

/// Walks `page` at `text` pt in a window on the fixture, its every text run
/// and shared control marked. The fixture has no remote, so the Git action
/// bar says so on every page.
async fn walk(page: Page, text: u8, cx: &mut TestAppContext) {
    walk_at_edge(page, text, &[], cx).await;
}

/// [`walk`] on History with the navigation dragged to its 180 px minimum.
async fn walk_narrowest_navigation(text: u8, cx: &mut TestAppContext) {
    walk_with(
        Page::History,
        text,
        &[],
        Saved::NarrowestNavigation,
        None,
        false,
        cx,
    )
    .await;
}

/// [`walk`] with the repository's local watch unable to start, as when the
/// host's inotify instances are exhausted: the status bar's warning, its
/// Refresh and Details… stay whole beside the hints, which give way whole.
async fn walk_with_watch_warning(page: Page, text: u8, cx: &mut TestAppContext) {
    walk_with(page, text, &[], Saved::Defaults, None, true, cx).await;
}

/// [`walk`] with the inspector dragged to its widest, 480 px, and that
/// width saved.
async fn walk_widest_inspector(page: Page, text: u8, cx: &mut TestAppContext) {
    walk_with(page, text, &[], Saved::WidestInspector, None, false, cx).await;
}

/// The widths a walk starts from, as a drag saved them.
#[derive(Clone, Copy, PartialEq)]
enum Saved {
    /// Each panel's default width.
    Defaults,
    /// The navigation at its 180 px minimum.
    NarrowestNavigation,
    /// The inspector at its 480 px maximum.
    WidestInspector,
}

/// [`walk`], once the Git commands `edge` have moved the fixture to another
/// repository edge, as a detached HEAD or a branch with no upstream.
async fn walk_at_edge(page: Page, text: u8, edge: &[&[&str]], cx: &mut TestAppContext) {
    walk_with(page, text, edge, Saved::Defaults, None, false, cx).await;
}

/// The Fetch outcome the outcome walks keep fresh in the status line.
const FETCH_OUTCOME: &str = "Up to date with origin · nothing new";

/// [`walk`] on History with a Fetch's outcome fresh in the status line
/// for the whole walk, beside the local watch's warning where `unwatched`:
/// the test clock stands still, so the outcome never ends.
async fn walk_with_outcome(text: u8, unwatched: bool, cx: &mut TestAppContext) {
    walk_with(
        Page::History,
        text,
        &[],
        Saved::Defaults,
        Some(FETCH_OUTCOME),
        unwatched,
        cx,
    )
    .await;
}

/// [`walk_at_edge`], from the widths `saved`, with `outcome` recorded as a
/// write's fresh outcome, and the status bar's local watch warning where
/// `unwatched`.
async fn walk_with(
    page: Page,
    text: u8,
    edge: &[&[&str]],
    saved: Saved,
    outcome: Option<&str>,
    unwatched: bool,
    cx: &mut TestAppContext,
) {
    let (_fixture, app, cx, path) = open(page, cx).await;
    if let Some(outcome) = outcome {
        app.update(cx, |app, cx| {
            let root = app.repository.as_ref().unwrap().path().to_owned();
            app.record_outcome(&root, outcome.to_owned(), cx);
        });
        frame(cx);
    }
    if saved == Saved::NarrowestNavigation {
        cx.simulate_resize(size(px(1480.), px(980.)));
        crate::views::tests::draw(cx);
        let panels = app.read_with(cx, |app, _| app.history_panels.clone());
        cx.update(|window, cx| {
            panels.update(cx, |panels, cx| {
                panels.resize_panel(0, px(180.), window, cx)
            })
        });
        crate::views::tests::draw(cx);
        let drawn = cx
            .debug_bounds("history-sidebar")
            .map(|bounds| bounds.size.width);
        assert_eq!(drawn, Some(px(180.)), "the navigation at its minimum");
    }
    if saved == Saved::WidestInspector {
        cx.simulate_resize(size(px(1480.), px(980.)));
        crate::views::tests::draw(cx);
        let panels = app.read_with(cx, |app, _| app.content_panels.clone());
        cx.update(|window, cx| {
            panels.update(cx, |panels, cx| {
                panels.resize_panel(1, px(480.), window, cx)
            });
            // As the drag's end saves it.
            app.update(cx, |app, cx| {
                app.settings.inspector_width = f32::from(panels.read(cx).sizes()[1]);
                app.save_preferences(window, cx);
            });
        });
        crate::views::tests::draw(cx);
        let (drawn, kept) = app.read_with(cx, |app, cx| {
            (
                app.content_panels.read(cx).sizes().get(1).copied(),
                app.settings.inspector_width,
            )
        });
        assert_eq!(drawn, Some(px(480.)), "the inspector at its widest");
        assert_eq!(kept, 480., "its width saved");
    }
    if !edge.is_empty() {
        let root = app.read_with(cx, |app, _| {
            app.repository.as_ref().unwrap().path().to_owned()
        });
        for args in edge {
            fixture_git(&root, args);
        }
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        settle(&app, cx).await;
    }
    if unwatched {
        unwatch(&app, cx);
    }
    cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
    let mut walk = Walk {
        page,
        text,
        app: &app,
        path,
        findings: Vec::new(),
        layouts: 0,
    };
    for height in [490., 980.] {
        walk.walk(height, cx);
    }
    if unwatched && outcome.is_some() && text == 18 {
        // In a 461 × 490 half tile the warning leaves the outcome less than
        // its text: every hint and Details…'s label have given way to it.
        cx.simulate_resize(size(px(461.), px(490.)));
        frame(cx);
        crate::views::COMPACT_NAMES.with(|names| names.borrow_mut().clear());
        app.update(cx, |_, cx| cx.notify());
        frame(cx);
        let details_icon = crate::views::COMPACT_NAMES
            .with(|names| names.borrow().contains_key("local-refresh-details"));
        let shown = (
            cx.debug_bounds("retry-local-refresh").is_some(),
            cx.debug_bounds("status-hints").is_some(),
            details_icon,
        );
        assert_eq!(
            shown,
            (true, false, true),
            "(Refresh, hints, Details… as its icon)"
        );
    }
    if unwatched {
        // Where the bar has the room, every hint is drawn beside the warning.
        cx.simulate_resize(size(px(1480.), px(980.)));
        frame(cx);
        let (hints, whole) = cx.update(|window, cx| {
            let hints = app.update(cx, |app, cx| app.key_hints(window, cx));
            let hints = hints.expect("the page names its keys").whole();
            let whole = shaped_width(
                &hints,
                appearance::ui_text(10.),
                window.text_style().font_weight,
                window,
            );
            (hints, whole)
        });
        let drawn = cx
            .debug_bounds("status-hints")
            .map(|bounds| bounds.size.width);
        assert!(
            drawn.is_some_and(|drawn| drawn >= whole - px(0.5)),
            "{hints:?}, {whole:?} wide, drawn {drawn:?} beside the warning at 1,480 px"
        );
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    if let Some(outcome) = outcome {
        let fresh = app.read_with(cx, |app, _| app.fresh_outcome().map(str::to_owned));
        assert_eq!(fresh.as_deref(), Some(outcome), "fresh for the whole walk");
    }
    walk.verdict();
}

/// Subscribes `app`'s repository again through a watcher that cannot start,
/// as one cannot once the host's inotify instances are exhausted, and waits
/// for the status bar to say that some changes need Refresh.
pub(crate) fn unwatch(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.watch_through(local_refresh::Backend::Unavailable, window, cx)
        })
    });
    // The subscription is made on the operation thread, which
    // `run_until_parked` cannot wait for: once it drains, the subscription
    // has answered, and its reply lands as the next frame parks.
    app.read_with(cx, |app, _| app.operations.drain());
    frame(cx);
    assert!(
        cx.debug_bounds("local-refresh-warning").is_some(),
        "the status bar says that some changes need Refresh"
    );
}

/// A window on the fixture in Midnight, its every text run and shared
/// control marked, showing `page` once its reads settle, and the path
/// Compare shows there. The layout grid (`layout_grid`) lays its pages out
/// from here too. Keep the returned directory until the test ends.
pub(crate) async fn open(
    page: Page,
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<GitTurtle>,
    &mut VisualTestContext,
    Option<&'static str>,
) {
    let (fixture, app, cx) =
        styled_history_window(cx, &fixture_script(), |root| root.text_bg(text_mark())).await;
    cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
    crate::appearance::mark_controls(control_mark());
    // The repository's own identity, so the commit action's label, and the
    // breakpoints it moves, do not depend on the caller's Git config.
    let root = app.read_with(cx, |app, _| {
        app.repository.as_ref().unwrap().path().to_owned()
    });
    fixture_git(&root, &["config", "user.name", "Walk Fixture"]);
    fixture_git(&root, &["config", "user.email", "walk@example.invalid"]);
    let open = |cx: &mut VisualTestContext, path: &str| {
        let index = app.read_with(cx, |app, _| {
            app.files
                .iter()
                .position(|file| file.path() == std::path::Path::new(path))
                .unwrap_or_else(|| panic!("{path} among the commit's files"))
        });
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(index, window, cx)));
    };
    let path = match page {
        Page::History => None,
        Page::HistorySearch => {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.search.update(cx, |input, cx| {
                        input.set_value("no such commit", window, cx)
                    });
                    app.history_query_changed(window, cx);
                })
            });
            // Past its debounce, once the reader has answered.
            crate::history_search::search_lands(&app, cx);
            frame(cx);
            assert!(
                app.read_with(cx, |app, _| app.history_search_no_match().is_some()),
                "the search finds nothing"
            );
            None
        }
        Page::CompareText => {
            open(cx, "README.md");
            Some("README.md")
        }
        Page::CompareImage => {
            open(cx, "assets/turtle.png");
            Some("assets/turtle.png")
        }
        Page::Working => {
            std::fs::write(root.join("alpha.txt"), "alpha\n").unwrap();
            std::fs::write(root.join("beta.txt"), "beta\n").unwrap();
            fixture_git(&root, &["add", "alpha.txt", "beta.txt"]);
            std::fs::write(root.join("README.md"), "edited\n").unwrap();
            cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
            None
        }
        Page::GitHub => {
            cx.update(|window, cx| crate::github_view::open_offline_fixture(&app, window, cx));
            let (operations, preferences) = app.read_with(cx, |app, _| {
                (
                    app.operations.submit_read(|| Ok(())),
                    app.preferences_writer.submit_read(|| Ok(())),
                )
            });
            operations.await.unwrap().unwrap();
            preferences.await.unwrap().unwrap();
            None
        }
    };
    settle(&app, cx).await;
    // The status strip names how long the last read took, so the width its
    // line takes, and where GPUI's ellipsis lets a truncated line end up to
    // a pixel past its box, changed from run to run; the walk reads it with
    // that figure pinned.
    app.update(cx, |app, cx| {
        app.status = pinned_timing(&app.status);
        cx.notify();
    });
    frame(cx);
    (fixture, app, cx, path)
}

/// `status` with each measured duration ("12.9 ms") read as 10.0 ms.
fn pinned_timing(status: &str) -> String {
    let words: Vec<&str> = status.split(' ').collect();
    words
        .iter()
        .enumerate()
        .map(|(index, word)| {
            let measured = words
                .get(index + 1)
                .is_some_and(|unit| unit.starts_with("ms"))
                && word.parse::<f32>().is_ok();
            if measured { "10.0" } else { word }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[gpui::test]
async fn history_keeps_its_text_whole_at_13_pt(cx: &mut TestAppContext) {
    walk(Page::History, 13, cx).await;
}

#[gpui::test]
async fn history_keeps_its_text_whole_at_18_pt(cx: &mut TestAppContext) {
    walk(Page::History, 18, cx).await;
}

/// A search with no match: its controls take their compact forms as the
/// column narrows, and its line and Clear search stay inside the list.
#[gpui::test]
async fn a_search_with_no_match_keeps_its_state_whole_at_13_pt(cx: &mut TestAppContext) {
    walk(Page::HistorySearch, 13, cx).await;
}

#[gpui::test]
async fn a_search_with_no_match_keeps_its_state_whole_at_18_pt(cx: &mut TestAppContext) {
    walk(Page::HistorySearch, 18, cx).await;
}

/// A write's fresh outcome stays whole in the status line: the keyboard
/// hints beside it keep their keys alone, or leave, while it lasts.
#[gpui::test]
async fn a_fresh_outcome_keeps_the_status_line_whole_at_13_pt(cx: &mut TestAppContext) {
    walk_with_outcome(13, false, cx).await;
}

#[gpui::test]
async fn a_fresh_outcome_keeps_the_status_line_whole_at_18_pt(cx: &mut TestAppContext) {
    walk_with_outcome(18, false, cx).await;
}

/// A fresh outcome beside the local watch's warning: the warning and its
/// Refresh stay whole, then the outcome, for which the hints give way and
/// Details… becomes its icon. In a 461 × 490 half tile at 18 pt the
/// warning leaves the outcome less than its whole text, so there it ends in
/// an ellipsis inside the status line, with every hint gone and Details…
/// as its icon.
#[gpui::test]
async fn a_fresh_outcome_beside_a_watch_warning_yields_only_to_it_at_18_pt(
    cx: &mut TestAppContext,
) {
    walk_with_outcome(18, true, cx).await;
}

#[gpui::test]
async fn the_narrowest_navigation_keeps_its_text_whole_at_13_pt(cx: &mut TestAppContext) {
    walk_narrowest_navigation(13, cx).await;
}

#[gpui::test]
async fn the_narrowest_navigation_keeps_its_text_whole_at_18_pt(cx: &mut TestAppContext) {
    walk_narrowest_navigation(18, cx).await;
}

#[gpui::test]
async fn compare_text_keeps_its_text_whole_at_13_pt(cx: &mut TestAppContext) {
    walk(Page::CompareText, 13, cx).await;
}

#[gpui::test]
async fn compare_text_keeps_its_text_whole_at_18_pt(cx: &mut TestAppContext) {
    walk(Page::CompareText, 18, cx).await;
}

#[gpui::test]
async fn compare_image_keeps_its_text_whole_at_13_pt(cx: &mut TestAppContext) {
    walk(Page::CompareImage, 13, cx).await;
}

#[gpui::test]
async fn compare_image_keeps_its_text_whole_at_18_pt(cx: &mut TestAppContext) {
    walk(Page::CompareImage, 18, cx).await;
}

#[gpui::test]
async fn working_changes_keep_their_text_whole_at_13_pt(cx: &mut TestAppContext) {
    walk(Page::Working, 13, cx).await;
}

#[gpui::test]
async fn working_changes_keep_their_text_whole_at_18_pt(cx: &mut TestAppContext) {
    walk(Page::Working, 18, cx).await;
}

#[gpui::test]
async fn history_gives_way_from_the_widest_saved_inspector_at_13_pt(cx: &mut TestAppContext) {
    walk_widest_inspector(Page::History, 13, cx).await;
}

#[gpui::test]
async fn history_gives_way_from_the_widest_saved_inspector_at_18_pt(cx: &mut TestAppContext) {
    walk_widest_inspector(Page::History, 18, cx).await;
}

#[gpui::test]
async fn working_changes_give_way_from_the_widest_saved_inspector_at_13_pt(
    cx: &mut TestAppContext,
) {
    walk_widest_inspector(Page::Working, 13, cx).await;
}

#[gpui::test]
async fn working_changes_give_way_from_the_widest_saved_inspector_at_18_pt(
    cx: &mut TestAppContext,
) {
    walk_widest_inspector(Page::Working, 18, cx).await;
}

/// Detaching HEAD, as `git checkout --detach` does.
const DETACHED: &[&[&str]] = &[&["checkout", "--detach"]];
/// A remote the branch does not track.
const NO_UPSTREAM: &[&[&str]] = &[&["remote", "add", "origin", "../origin.git"]];

#[gpui::test]
async fn a_detached_head_keeps_the_bar_whole_at_13_pt(cx: &mut TestAppContext) {
    walk_at_edge(Page::History, 13, DETACHED, cx).await;
}

#[gpui::test]
async fn a_detached_head_keeps_the_bar_whole_at_18_pt(cx: &mut TestAppContext) {
    walk_at_edge(Page::History, 18, DETACHED, cx).await;
}

#[gpui::test]
async fn no_upstream_keeps_the_bar_whole_at_13_pt(cx: &mut TestAppContext) {
    walk_at_edge(Page::History, 13, NO_UPSTREAM, cx).await;
}

#[gpui::test]
async fn no_upstream_keeps_the_bar_whole_at_18_pt(cx: &mut TestAppContext) {
    walk_at_edge(Page::History, 18, NO_UPSTREAM, cx).await;
}

/// With the local watch unable to start, the status bar's warning, Refresh
/// and Details… stay whole and its hints give way item by item: the warning
/// pushed them past the window's edge, cut mid-glyph at every width up to
/// 821 px at 18 pt, whenever concurrent test suites exhausted the host's
/// inotify instances.
#[gpui::test]
async fn a_watch_warning_keeps_the_status_bar_whole_on_history_at_13_pt(cx: &mut TestAppContext) {
    walk_with_watch_warning(Page::History, 13, cx).await;
}

#[gpui::test]
async fn a_watch_warning_keeps_the_status_bar_whole_on_history_at_18_pt(cx: &mut TestAppContext) {
    walk_with_watch_warning(Page::History, 18, cx).await;
}

#[gpui::test]
async fn a_watch_warning_keeps_the_status_bar_whole_on_working_changes_at_13_pt(
    cx: &mut TestAppContext,
) {
    walk_with_watch_warning(Page::Working, 13, cx).await;
}

#[gpui::test]
async fn a_watch_warning_keeps_the_status_bar_whole_on_working_changes_at_18_pt(
    cx: &mut TestAppContext,
) {
    walk_with_watch_warning(Page::Working, 18, cx).await;
}

#[gpui::test]
async fn the_github_panel_keeps_its_text_whole_at_13_pt(cx: &mut TestAppContext) {
    walk(Page::GitHub, 13, cx).await;
}

#[gpui::test]
async fn the_github_panel_keeps_its_text_whole_at_18_pt(cx: &mut TestAppContext) {
    walk(Page::GitHub, 18, cx).await;
}

/// A Working row's name fits its room whole or ends in an ellipsis inside
/// it: at 18 pt in a 515 × 980 window "turtle.png", 99.7 px in the unit
/// tests' text system, was drawn whole in its 99 px box and cut by half a
/// pixel, since GPUI ends a line in an ellipsis only once its width, rounded
/// down, exceeds the room.
#[gpui::test]
async fn a_working_row_ends_a_name_it_cannot_hold_inside_its_room(cx: &mut TestAppContext) {
    let (_fixture, app, cx, _) = open(Page::Working, cx).await;
    cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
    cx.simulate_resize(size(px(515.), px(980.)));
    frame(cx);
    app.update(cx, |_, cx| cx.notify());
    frame(cx);
    let row = app.read_with(cx, |app, _| {
        let entries = &app.work_status.as_ref().unwrap().entries;
        app.working_rows
            .iter()
            .position(|row| {
                matches!(row, crate::workspace::WorkingRow::File(index, _)
                    if entries[*index].path == std::path::Path::new("assets/turtle.png"))
            })
            .expect("assets/turtle.png among the Working rows")
    });
    let selector: &'static str = format!("working-name-{row}").leak();
    let name = cx.debug_bounds(selector).expect("the row's name");
    let whole = cx.update(|window, _| {
        shaped_width(
            "turtle.png",
            appearance::ui_text(12.),
            FontWeight::MEDIUM,
            window,
        )
    });
    assert!(
        whole > name.size.width && whole < name.size.width + px(1.),
        "less than a pixel too wide, as the rounding misses: {whole:?} in {name:?}"
    );
    let (scale, quads) = cx.update(|window, _| (window.scale_factor(), window.painted_quads()));
    let logical = |value: ScaledPixels| px(value.0 / scale);
    let text = gpui_kit::Background::from(text_mark());
    let runs: Vec<(Pixels, Pixels)> = quads
        .iter()
        .filter(|quad| quad.background == text)
        .filter(|quad| {
            (logical(quad.bounds.left()) - name.left()).abs() < px(0.5)
                && logical(quad.bounds.top()) >= name.top()
                && logical(quad.bounds.top()) < name.bottom()
        })
        .map(|quad| {
            (
                logical(quad.bounds.right()),
                logical(quad.content_mask.bounds.right()),
            )
        })
        .collect();
    let [(right, clip)] = runs.as_slice() else {
        panic!("one run for the name, not {runs:?}");
    };
    assert!(
        *right <= *clip && *right <= name.right(),
        "the name ends at {right:?} beside a clip at {clip:?}, its box at {:?}",
        name.right()
    );
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// The dialogs the narrow walk opens: every one with a width of its own.
#[derive(Clone, Copy, Debug)]
enum Dialog {
    Reflog,
    Rebase,
    RewriteReview,
    CommandPalette,
    QuickOpen,
    KeyboardShortcuts,
    Tags,
    Stashes,
    ThemeEditor,
    Worktrees,
    /// The worktree manager while a read runs.
    WorktreesReading,
    RevisionComparison,
    Activity,
    RecoveryDrafts,
    Profiles,
    RepositoryLibrary,
}

impl Dialog {
    const ALL: [Dialog; 16] = [
        Dialog::Reflog,
        Dialog::Rebase,
        Dialog::RewriteReview,
        Dialog::CommandPalette,
        Dialog::QuickOpen,
        Dialog::KeyboardShortcuts,
        Dialog::Tags,
        Dialog::Stashes,
        Dialog::ThemeEditor,
        Dialog::Worktrees,
        Dialog::WorktreesReading,
        Dialog::RevisionComparison,
        Dialog::Activity,
        Dialog::RecoveryDrafts,
        Dialog::Profiles,
        Dialog::RepositoryLibrary,
    ];

    /// The room it keeps from either side of a window narrower than it:
    /// `appearance::DIALOG_MARGIN`, or for the theme editor the 16 px its
    /// own width rule leaves (DESIGN.md, "Custom theme editor").
    fn margin(self) -> Pixels {
        match self {
            Dialog::ThemeEditor => px(16.),
            _ => appearance::DIALOG_MARGIN,
        }
    }

    /// A list it shows on the fixture, and that list's first row, which the
    /// list shows whole however the content around it wraps: repository
    /// content keeps its room first.
    fn first_row(self) -> Option<(&'static str, &'static str)> {
        match self {
            Dialog::Reflog => Some(("reflog-list", "reflog-entry-0")),
            Dialog::KeyboardShortcuts => Some(("shortcut-help-results", "shortcut-help-row-1")),
            Dialog::Worktrees => Some(("managed-worktree-list", "managed-worktree-row-0")),
            _ => None,
        }
    }

    /// A list that gives up its height to the dialog's height cap, its
    /// height where the dialog has the room, and the body's last line and
    /// the footer's control after it: the list, not the body, gives way, so
    /// the last line ends above the control rather than below the body's
    /// fold.
    fn yielding_list(self) -> Option<(&'static str, Pixels, String, String)> {
        use crate::shortcuts::DialogKeys;
        match self {
            Dialog::KeyboardShortcuts => Some((
                "shortcut-help-results",
                crate::shortcuts::list_height(),
                format!(
                    "dialog-help-escape: {}",
                    DialogKeys::Shortcuts.escape_help()
                ),
                format!("dialog-dismiss: {}", DialogKeys::Shortcuts.dismiss()),
            )),
            _ => None,
        }
    }

    /// Opens it from History once its reads land, as the focus sweep does.
    async fn open(self, app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        use crate::focus_ring::sweep::open_dialog;
        let name = format!("{self:?}");
        match self {
            Dialog::Reflog => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_reflog_browser(window, cx)
                })
                .await
            }
            Dialog::Rebase => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_interactive_rebase(None, window, cx)
                })
                .await
            }
            Dialog::RewriteReview => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_rewrite_review(window, cx)
                })
                .await
            }
            Dialog::CommandPalette => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_command_palette(window, cx)
                })
                .await
            }
            Dialog::QuickOpen => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_quick_file(window, cx)
                })
                .await
            }
            Dialog::KeyboardShortcuts => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.shortcut_help(window, cx)
                })
                .await
            }
            Dialog::Tags => {
                open_dialog(app, cx, &name, |app, window, cx| app.open_tags(window, cx)).await
            }
            Dialog::Stashes => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_stashes(window, cx)
                })
                .await
            }
            Dialog::ThemeEditor => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_theme_editor(None, window, cx)
                })
                .await
            }
            Dialog::Worktrees => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_worktree_manager(window, cx)
                })
                .await;
                crate::worktrees::settle_manager(app, cx).await;
            }
            Dialog::WorktreesReading => {
                Box::pin(Dialog::Worktrees.open(app, cx)).await;
                crate::worktrees::show_reading(app, cx);
            }
            Dialog::RevisionComparison => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_revision_comparison(window, cx)
                })
                .await
            }
            Dialog::Activity => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_activity(window, cx)
                })
                .await
            }
            Dialog::RecoveryDrafts => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_recovery_drafts(window, cx)
                })
                .await
            }
            Dialog::Profiles => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_profiles(window, cx)
                })
                .await
            }
            Dialog::RepositoryLibrary => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_repository_library(window, cx)
                })
                .await
            }
        }
    }
}

/// The kit's dialog padding, inside which a dialog's text lies.
const DIALOG_PADDING: Pixels = px(16.);

/// What the last frame drew wrong in the open dialog of a `width`-wide
/// window: the dialog closer to a side than `margin`, its foot closer to
/// the status bar's top edge than `margin` (`appearance::dialog_max_height`),
/// and every text run or shared control above the window's overlay that
/// crosses a side of its clip, as [`Walk::inspect`] finds them on a page.
fn dialog_findings(cx: &mut VisualTestContext, width: f32, margin: Pixels) -> Vec<String> {
    let (text, control) = (
        gpui_kit::Background::from(text_mark()),
        gpui_kit::Background::from(control_mark()),
    );
    let (scale, viewport, status_bar, mut quads) = cx.update(|window, _| {
        (
            window.scale_factor(),
            window.viewport_size(),
            theme_editor::status_bar_top(window),
            window.painted_quads(),
        )
    });
    quads.sort_by_key(|quad| quad.order);
    let logical = |value: ScaledPixels| px(value.0 / scale);
    // The dialog draws above the last fill that spans the window, its
    // overlay, and its own fill is the first above it.
    let Some(overlay) = quads
        .iter()
        .filter(|quad| {
            logical(quad.bounds.size.width) >= viewport.width - px(1.)
                && logical(quad.bounds.size.height) >= viewport.height - px(1.)
        })
        .map(|quad| quad.order)
        .max()
    else {
        return vec!["no overlay drawn".into()];
    };
    quads.retain(|quad| quad.order > overlay);
    let mut findings = Vec::new();
    match quads
        .iter()
        .find(|quad| quad.background != text && quad.background != control)
    {
        Some(dialog) => {
            let (left, right, bottom) = (
                logical(dialog.bounds.left()),
                logical(dialog.bounds.right()),
                logical(dialog.bounds.bottom()),
            );
            let slack = px(0.5);
            if left < margin - slack || right > px(width) - margin + slack {
                findings.push(format!(
                    "the dialog spans {:.1}..{:.1} px, closer than {:.0} px to a side",
                    f32::from(left),
                    f32::from(right),
                    f32::from(margin)
                ));
            }
            if bottom > status_bar - margin + slack {
                findings.push(format!(
                    "the dialog ends {:.1} px down, closer than {:.0} px to the status bar at {:.1}",
                    f32::from(bottom),
                    f32::from(margin),
                    f32::from(status_bar)
                ));
            }
        }
        None => findings.push("no dialog drawn".into()),
    }
    // Text inside the panel's padding, where a row that cannot hold its
    // controls, such as a footer, pushes them.
    if let Some(dialog) = quads
        .iter()
        .find(|quad| quad.background != text && quad.background != control)
        .map(|quad| quad.bounds)
    {
        let inset = ScaledPixels(f32::from(DIALOG_PADDING) * scale);
        let slack = ScaledPixels(scale);
        for quad in quads.iter().filter(|quad| quad.background == text) {
            if quad.bounds.left() < dialog.left() + inset - slack
                || quad.bounds.right() > dialog.right() - inset + slack
            {
                findings.push(format!(
                    "text at {:.1}..{:.1} runs into the padding of the dialog at {:.1}..{:.1}, {:.1} px down",
                    f32::from(logical(quad.bounds.left())),
                    f32::from(logical(quad.bounds.right())),
                    f32::from(logical(dialog.left())),
                    f32::from(logical(dialog.right())),
                    f32::from(logical(quad.bounds.top())),
                ));
            }
        }
    }
    let slack = ScaledPixels(0.5);
    for quad in &quads {
        let kind = if quad.background == text {
            "text"
        } else if quad.background == control {
            "a control"
        } else {
            continue;
        };
        let (drawn, clip) = (quad.bounds, quad.content_mask.bounds);
        if drawn.left() >= clip.left() - slack && drawn.right() <= clip.right() + slack {
            continue;
        }
        findings.push(format!(
            "{kind} at {:.1}..{:.1} crosses a clip of {:.1}..{:.1}, {:.1} px down",
            f32::from(logical(drawn.left())),
            f32::from(logical(drawn.right())),
            f32::from(logical(clip.left())),
            f32::from(logical(clip.right())),
            f32::from(logical(drawn.top())),
        ));
    }
    findings
}

/// Opens every dialog with a width of its own in a 461 × 490 half tile at
/// `text` pt: each lies inside the window with its margin on either side,
/// rather than centred at its width with its left side past the window's,
/// ends that margin above the status bar, where Keyboard Shortcuts once
/// ended 14 px over the bar at 13 pt and on its top rule at 18 pt, and its
/// content gives way in the responsive order with nothing cut by its clip
/// (DESIGN.md, "One responsive order").
async fn narrow_dialogs(text: u8, cx: &mut TestAppContext) {
    let (_fixture, app, cx, _) = open(Page::History, cx).await;
    cx.update(|_, cx| {
        theme_editor::init(cx);
        native_accessibility::bind_keys(cx);
        shortcuts::bind_keys(cx);
        // Dialogs open at rest, so the frame read is where they stay.
        cx.set_reduce_motion(true);
    });
    let (width, height) = (461., 490.);
    cx.simulate_resize(size(px(width), px(height)));
    let mut report = String::new();
    for dialog in Dialog::ALL {
        // Closing the theme editor applies the saved settings again, the
        // interface size among them.
        cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
        frame(cx);
        dialog.open(&app, cx).await;
        frame(cx);
        app.update(cx, |_, cx| cx.notify());
        frame(cx);
        let mut findings = dialog_findings(cx, width, dialog.margin());
        if let Some((list, row)) = dialog.first_row() {
            let slack = px(0.5);
            match (cx.debug_bounds(list), cx.debug_bounds(row)) {
                (Some(shown), Some(drawn))
                    if drawn.top() >= shown.top() - slack
                        && drawn.bottom() <= shown.bottom() + slack => {}
                (shown, drawn) => findings.push(format!(
                    "{list} at {shown:?} does not show {row} at {drawn:?} whole"
                )),
            }
        }
        if let Some((list, full, last, control)) = dialog.yielding_list() {
            match (
                cx.debug_bounds(list),
                bounds_of(cx, last),
                bounds_of(cx, control),
            ) {
                (Some(list), Some(last), Some(control)) => {
                    if list.size.height >= full - px(0.5) {
                        findings.push(format!(
                            "{list:?} keeps its full {full:?} rather than giving it up"
                        ));
                    }
                    if last.bottom() > control.top() {
                        findings.push(format!(
                            "the body's last line at {last:?} runs below the fold, under {control:?}"
                        ));
                    }
                }
                drawn => findings.push(format!("the list, last line and control at {drawn:?}")),
            }
        }
        let mut seen = BTreeSet::new();
        for finding in findings {
            if seen.insert(finding.clone()) {
                report.push_str(&format!("\n  {dialog:?}: {finding}"));
            }
        }
        cx.update(|window, cx| window.close_all_dialogs(cx));
        frame(cx);
        settle(&app, cx).await;
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    assert!(
        report.is_empty(),
        "at {text} pt in {width} × {height}:{report}"
    );
}

#[gpui::test]
async fn dialogs_keep_inside_a_narrow_window_at_13_pt(cx: &mut TestAppContext) {
    narrow_dialogs(13, cx).await;
}

#[gpui::test]
async fn dialogs_keep_inside_a_narrow_window_at_18_pt(cx: &mut TestAppContext) {
    narrow_dialogs(18, cx).await;
}
