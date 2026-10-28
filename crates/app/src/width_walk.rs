//! The width walk: History, Compare (text and image), Working Changes and
//! the GitHub panel on the offline fixture, laid out at every width from 461
//! to 1,480 px at 13 and 18 pt in a short and a tall window, fail on cut or
//! orphaned text (DESIGN.md, "One responsive order").
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
use crate::views::tests::{fixture_git, settle, styled_history_window};
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
}

/// The pages the walk lays out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    History,
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
    "repository-header",
    "git-actions",
    "repository-rail",
    "status-hints",
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

const KNOWN: &[Known] = &[
    // Compare cuts its path for its actions: "READ…" at 18 pt in 461 px.
    Known {
        page: Page::CompareText,
        sizes: &[18],
        rule: Rule::PathCut,
        region: "preview-file-path",
        follow_up: "Compare keeps its path whole",
    },
    Known {
        page: Page::CompareImage,
        sizes: &[13, 18],
        rule: Rule::PathCut,
        region: "preview-file-path",
        follow_up: "Compare keeps its path whole",
    },
    // An image side's caption lines run past a narrow image column.
    Known {
        page: Page::CompareImage,
        sizes: &[13, 18],
        rule: Rule::CutText,
        region: "preview-content",
        follow_up: "Compare's image captions wrap or truncate in a narrow column",
    },
    // At 18 pt in 461 × 980 a line of the selected commit's details runs
    // past the 156 px inspector: the parent Button, "P1 · ‹hash›", is 148 px
    // wide where the metadata has 128.5, so the date line wraps at its width.
    Known {
        page: Page::History,
        sizes: &[18],
        rule: Rule::CutText,
        region: "commit-message-viewport",
        follow_up: "the commit details fit an 18 pt half-tile inspector",
    },
    Known {
        page: Page::CompareText,
        sizes: &[18],
        rule: Rule::CutText,
        region: "commit-message-viewport",
        follow_up: "the commit details fit an 18 pt half-tile inspector",
    },
    Known {
        page: Page::CompareImage,
        sizes: &[18],
        rule: Rule::CutText,
        region: "commit-message-viewport",
        follow_up: "the commit details fit an 18 pt half-tile inspector",
    },
    // At 18 pt in 515 × 980 a Working row's name is 0.5 px wider than the
    // room the row fits it to.
    Known {
        page: Page::Working,
        sizes: &[18],
        rule: Rule::CutText,
        region: "working-pane",
        follow_up: "a Working row measures its name as it draws it",
    },
    // A PR's detail rows and their controls run past a narrow panel.
    Known {
        page: Page::GitHub,
        sizes: &[13, 18],
        rule: Rule::CutText,
        region: "github-panel",
        follow_up: "the GitHub panel wraps its labels inside the panel",
    },
];

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
/// and shared control marked.
async fn walk(page: Page, text: u8, cx: &mut TestAppContext) {
    let (_fixture, app, cx, path) = open(page, cx).await;
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
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    walk.verdict();
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
    (fixture, app, cx, path)
}

#[gpui::test]
async fn history_keeps_its_text_whole_at_13_pt(cx: &mut TestAppContext) {
    walk(Page::History, 13, cx).await;
}

#[gpui::test]
async fn history_keeps_its_text_whole_at_18_pt(cx: &mut TestAppContext) {
    walk(Page::History, 18, cx).await;
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
async fn the_github_panel_keeps_its_text_whole_at_13_pt(cx: &mut TestAppContext) {
    walk(Page::GitHub, 13, cx).await;
}

#[gpui::test]
async fn the_github_panel_keeps_its_text_whole_at_18_pt(cx: &mut TestAppContext) {
    walk(Page::GitHub, 18, cx).await;
}
