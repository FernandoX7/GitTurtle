//! A dialog's sides keep the room a ghost Button standing at its text edge
//! takes before that edge, its pull and its focus ring (DESIGN.md, "One left
//! edge"): the kit pads them by the larger of 16 px and the room the app
//! installs (`dialog_ghost_room`, through `native_accessibility::set_overlay_room`),
//! 16 px at 13 pt and 17 px at 18 pt. These tests open the app's own dialogs
//! on the width walk's fixture and read what they draw, so they fail where
//! the app stops installing that room or the room loses the ring's 3 px,
//! which every reader of the kit's `dialog_side_padding` follows unseen.
//!
//! The walk's text mark paints a fill exactly behind every text run
//! (`width_walk`), so a dialog's text edge is where its leftmost run above
//! the backdrop starts, its title's among them.

use crate::focus_ring::sweep;
use crate::width_walk::{self, text_mark};
use crate::*;
use core::prelude::v1::test;
use gpui_kit::ScaledPixels;

/// The text sizes and windows the dialogs open in: the default and the
/// largest size, in a 461 × 490 half tile and a 1,480 × 800 window.
const SIZES: [(u8, f32, f32); 4] = [
    (13, 461., 490.),
    (18, 461., 490.),
    (13, 1480., 800.),
    (18, 1480., 800.),
];

/// The half tile at both text sizes on a display of fractional scale, 1.25,
/// where a ghost Button's pull, 12.5 device pixels at 13 pt and about 17.3 at
/// 18 pt, lies on the device pixel only as layout snaps it
/// ([`ghost_buttons_stand_their_labels_on_a_dialogs_text_edge`]; the test
/// display's own scale is 2).
const FRACTIONAL: [(u8, f32, f32, f32); 2] = [(13, 461., 490., 1.25), (18, 461., 490., 1.25)];

/// A dialog the fixture opens from History: the ones with a ghost Button at
/// their text edge (Git profiles, the profile form, Activity, Compare local
/// revisions' choices, the rewritten-series review, About's Copy bug
/// diagnostics, Add remote's Fetch rules…, Download an LFS preview,
/// Worktree created and the Reflog, the second and third from their openers
/// with a pointer and a plan the test makes, the Reflog with its newest
/// entry selected), the command palette, and the dialogs that size
/// themselves from the padding (the GitHub panel's, whose Files fit reads
/// it, and the theme editor's). The signing site is no dialog of its own: it
/// is the profile form's Use current repository signing settings, walked
/// with the form. Interactive rebase's Load commits is a ghost Button that
/// stands beside the base field rather than on the edge.
#[derive(Clone, Copy, Debug)]
enum Site {
    About,
    Profiles,
    ProfileForm,
    CommandPalette,
    Activity,
    RevisionChoices,
    RewriteReview,
    RemoteForm,
    GitHub,
    ThemeEditor,
    LfsSource,
    WorktreeCreated,
    Reflog,
    InteractiveRebase,
}

impl Site {
    const ALL: [Site; 10] = [
        Site::About,
        Site::Profiles,
        Site::ProfileForm,
        Site::CommandPalette,
        Site::Activity,
        Site::RevisionChoices,
        Site::RewriteReview,
        Site::RemoteForm,
        Site::GitHub,
        Site::ThemeEditor,
    ];

    /// The dialogs with a ghost Button at their text edge, walked by
    /// [`ghost_buttons_stand_their_labels_on_a_dialogs_text_edge`]: those in
    /// [`Self::ALL`], Download an LFS preview and Worktree created, opened
    /// from their openers with a working file's pointer and a worktree's
    /// plan the fixture reads but never creates, the Reflog, whose
    /// newest entry's details hold Copy commit ID and Review recovery
    /// branch…, and interactive rebase, whose Load commits stands beside its
    /// base field.
    const GHOSTS: [Site; 11] = [
        Site::About,
        Site::Profiles,
        Site::ProfileForm,
        Site::Activity,
        Site::RevisionChoices,
        Site::RewriteReview,
        Site::RemoteForm,
        Site::LfsSource,
        Site::WorktreeCreated,
        Site::Reflog,
        Site::InteractiveRebase,
    ];

    /// The rows of ghost Buttons standing at its text edge. Compare local
    /// revisions lists up to eight choices, the fixture's first at least;
    /// Download an LFS preview lists the repository's remotes, which the
    /// fixture has none of, before Read configured source. Review messages'
    /// row is drawn whole with no rewritten series to review, and Back to
    /// series, which shows only while one is inspected, is not drawn. The
    /// Reflog's selected entry shows Copy commit ID below its selector and
    /// IDs, and Review recovery branch… below the recovery branch's name.
    /// Interactive rebase's Load commits stands beside the base field.
    fn ghost_rows(self) -> Vec<Row> {
        let row = |selectors: &[(&'static str, Option<&'static str>)], icon: bool| Row {
            ghosts: selectors
                .iter()
                .map(|(selector, label)| ((*selector).to_owned(), *label))
                .collect(),
            icon,
            drawn: true,
            beside: false,
        };
        match self {
            Site::About => vec![row(&[("copy-bug-diagnostics", None)], true)],
            Site::Profiles => vec![row(&[("new-git-profile", None)], true)],
            Site::ProfileForm => vec![row(
                &[(
                    "profile-use-current-signer",
                    Some("Use current repository signing settings"),
                )],
                false,
            )],
            Site::Activity => vec![row(
                &[
                    ("activity-refresh", Some("Refresh current repository")),
                    ("activity-reflog", Some("Browse reflog…")),
                ],
                false,
            )],
            Site::RevisionChoices => (0..8)
                .map(|choice| Row {
                    ghosts: vec![(format!("revision-choice-{choice}"), None)],
                    icon: false,
                    drawn: choice == 0,
                    beside: false,
                })
                .collect(),
            Site::RewriteReview => vec![row(
                &[
                    ("inspect-series-messages", Some("Review messages")),
                    ("inspect-original-commit", Some("Inspect original files")),
                    ("inspect-rewritten-commit", Some("Inspect rewritten files")),
                    ("reload-series", Some("Refresh series")),
                ],
                false,
            )],
            Site::RemoteForm => vec![row(
                &[("remote-advanced-settings", Some("Fetch rules…"))],
                false,
            )],
            Site::LfsSource => vec![
                Row {
                    ghosts: (0..12)
                        .map(|remote| (format!("lfs-source-remote-{remote}"), None))
                        .collect(),
                    icon: false,
                    drawn: false,
                    beside: false,
                },
                row(&[("read-lfs-source", None)], true),
            ],
            Site::WorktreeCreated => vec![row(
                &[
                    ("open-created-worktree", Some("Open in GitTurtle")),
                    (
                        "reveal-created-worktree",
                        Some(if cfg!(target_os = "macos") {
                            "Show in Finder"
                        } else {
                            "Show in files"
                        }),
                    ),
                    ("edit-created-worktree", Some("Open in editor")),
                ],
                false,
            )],
            Site::Reflog => vec![
                row(&[("copy-reflog-oid", None)], true),
                row(
                    &[("review-reflog-recovery", Some("Review recovery branch…"))],
                    false,
                ),
            ],
            Site::InteractiveRebase => vec![Row {
                beside: true,
                ..row(&[("load-rebase-commits", Some("Load commits"))], false)
            }],
            _ => Vec::new(),
        }
    }

    /// The ghost Buttons at its text edge that Tab reaches on the fixture,
    /// each of which must draw its ring there: every one but the series
    /// review's three that wait for a selected pair, and the choices past
    /// Compare local revisions' first.
    fn tab_reaches(self) -> &'static [&'static str] {
        match self {
            Site::About => &["copy-bug-diagnostics"],
            Site::Profiles => &["new-git-profile"],
            Site::ProfileForm => &["profile-use-current-signer"],
            Site::Activity => &["activity-refresh", "activity-reflog"],
            Site::RevisionChoices => &["revision-choice-0"],
            Site::RewriteReview => &["reload-series"],
            Site::RemoteForm => &["remote-advanced-settings"],
            Site::LfsSource => &["read-lfs-source"],
            Site::WorktreeCreated => &[
                "open-created-worktree",
                "reveal-created-worktree",
                "edit-created-worktree",
            ],
            Site::Reflog => &["copy-reflog-oid", "review-reflog-recovery"],
            Site::InteractiveRebase => &["load-rebase-commits"],
            _ => &[],
        }
    }

    /// Whether Tab walks it for its rings: every dialog with a ghost Button
    /// at its text edge, and the palette. The GitHub panel's and the theme
    /// editor's many stops are walked by their own sweeps.
    fn walked(self) -> bool {
        !matches!(self, Site::GitHub | Site::ThemeEditor)
    }

    /// Whether its text starts at the padding's edge: the GitHub panel
    /// starts its text at its own inset past it (`github_view::text_inset`),
    /// where its Buttons at that edge start their labels.
    fn text_at_edge(self) -> bool {
        !matches!(self, Site::GitHub)
    }

    /// How far past its padding its text starts: the Reflog starts its text,
    /// its title's among it, at a field's text inset, where its fields' text
    /// starts (`appearance::FIELD_TEXT_INSET`), so its ghost Buttons stand
    /// their content there.
    fn text_inset(self) -> Pixels {
        match self {
            Site::Reflog => appearance::FIELD_TEXT_INSET,
            _ => px(0.),
        }
    }

    /// The control its footer ends with, at the dialog's right text edge,
    /// where it has a name to find it by.
    fn footer_end(self) -> Option<&'static str> {
        match self {
            Site::About | Site::Profiles | Site::Activity => Some("dialog-ok: Done"),
            Site::CommandPalette => Some("run-palette-command"),
            Site::GitHub => Some("github-close"),
            _ => None,
        }
    }

    async fn open(self, app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        use sweep::open_dialog;
        let name = format!("{self:?}");
        match self {
            Site::About => {
                open_dialog(app, cx, &name, |app, window, cx| app.about(window, cx)).await
            }
            Site::Profiles => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_profiles(window, cx)
                })
                .await
            }
            // Through Git profiles' New profile…, which closes it and opens
            // the form in its place. A person scrolls the dialog's body to it
            // first where it lies past the fold: the target's path wraps
            // onto as many lines as its length takes, and the fixture's in a
            // macOS temporary directory, as Git names it
            // (`/private/var/folders/…/T/…`), took three at 18 pt in the half
            // tile, where the body scrolls and the button lay wholly below
            // it, so a click at its centre missed it.
            Site::ProfileForm => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_profiles(window, cx)
                })
                .await;
                let body = cx
                    .debug_bounds("profile-manager-content")
                    .expect("Git profiles' body");
                let new = cx
                    .debug_bounds("new-git-profile")
                    .expect("Git profiles' New profile…");
                if new.bottom() > body.bottom() {
                    cx.simulate_event(ScrollWheelEvent {
                        position: body.center(),
                        delta: ScrollDelta::Pixels(point(px(0.), body.bottom() - new.bottom())),
                        modifiers: Modifiers::default(),
                        touch_phase: TouchPhase::Moved,
                    });
                    width_walk::frame(cx);
                }
                let new = cx
                    .debug_bounds("new-git-profile")
                    .expect("Git profiles' New profile…");
                assert!(
                    body.contains(&new.center()),
                    "New profile… at {new:?} lies in Git profiles' body, {body:?}"
                );
                cx.simulate_click(new.center(), Modifiers::none());
                app.read_with(cx, |app, _| app.operations.drain());
                views::tests::settle(app, cx).await;
                assert!(
                    cx.debug_bounds("profile-form").is_some(),
                    "New profile… opens the profile form"
                );
            }
            Site::CommandPalette => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_command_palette(window, cx)
                })
                .await
            }
            Site::Activity => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_activity(window, cx)
                })
                .await
            }
            // With its Before field emptied, so the read's first branches
            // and tags are its choices.
            Site::RevisionChoices => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    let form = app.open_compare_form(window, cx);
                    if let Some(form) = &form {
                        form.update(cx, |form, cx| form.show_every_choice(window, cx));
                    }
                    form
                })
                .await
            }
            Site::RewriteReview => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_series_review(window, cx)
                })
                .await
            }
            Site::RemoteForm => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_remote_form(None, window, cx)
                })
                .await
            }
            Site::GitHub => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_github(window, cx)
                })
                .await
            }
            Site::ThemeEditor => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_theme_editor(None, window, cx)
                })
                .await
            }
            // For a working file's pointer with no remote named, so the form
            // reads nothing and says to choose one.
            Site::LfsSource => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.remote_name
                        .update(cx, |input, cx| input.set_value("", window, cx));
                    let target = gitturtle_core::LfsDownloadTarget {
                        path: "assets/large.bin".into(),
                        blob_oid: None,
                        pointer: format!(
                            "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize 12\n",
                            "a".repeat(64)
                        )
                        .into_bytes(),
                    };
                    app.open_lfs_download(target, window, cx)
                })
                .await
            }
            // As a created worktree's write ends: its plan, read from the
            // fixture for a folder that does not exist, never created.
            Site::WorktreeCreated => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    let repo = app.repository.clone().expect("a repository");
                    let destination = repo
                        .path()
                        .parent()
                        .expect("the fixture's folder")
                        .join("walk-worktree");
                    let plan = repo
                        .create_worktree_plan(&destination, "walk-worktree", true, "HEAD")
                        .expect("a worktree plan");
                    let path = app.path.clone().expect("a repository path");
                    let command = gitturtle_core::WorktreeCommand::Create(plan);
                    app.finish_worktree_write(&path, &command, true, window, cx)
                })
                .await
            }
            // With its newest entry selected and that commit read, so its
            // details show Copy commit ID, the recovery branch's name and
            // Review recovery branch….
            Site::Reflog => {
                let browser = std::rc::Rc::new(std::cell::RefCell::new(None));
                let opened = browser.clone();
                open_dialog(app, cx, &name, move |app, window, cx| {
                    let browser = app.open_reflog_browser(window, cx);
                    *opened.borrow_mut() = browser.clone();
                    browser
                })
                .await;
                let browser = browser.borrow().clone().expect("the Reflog opens");
                crate::reflog::ReflogBrowser::select_newest(&browser, cx);
                views::tests::settle(app, cx).await;
            }
            Site::InteractiveRebase => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_interactive_rebase(None, window, cx)
                })
                .await
            }
        }
    }
}

/// A row of ghost Buttons at a dialog's text edge, each line's first
/// standing on it: each Button's debug selector with its label where the
/// test knows it, whether a leading icon starts their content, whether
/// the fixture draws the row's first, and whether they stand beside a field
/// on its line instead, past the edge and never on it, on one line.
struct Row {
    ghosts: Vec<(String, Option<&'static str>)>,
    icon: bool,
    drawn: bool,
    beside: bool,
}

/// `bounds` in the window's logical pixels.
fn logical(bounds: Bounds<ScaledPixels>, scale: f32) -> Bounds<Pixels> {
    Bounds::from_corners(
        point(
            px(bounds.left().as_f32() / scale),
            px(bounds.top().as_f32() / scale),
        ),
        point(
            px(bounds.right().as_f32() / scale),
            px(bounds.bottom().as_f32() / scale),
        ),
    )
}

/// What the last frame drew of the open dialog: its panel, its 1 px edge
/// included, the first fill above the last one that spans the window, its
/// backdrop, as the width walk finds it; and where its text starts, the
/// leftmost text run above the backdrop that lies in the panel and that its
/// clip leaves whole on the left.
fn drawn(cx: &mut VisualTestContext) -> Option<(Bounds<Pixels>, Option<Pixels>)> {
    let mark = gpui_kit::Background::from(text_mark());
    let (scale, viewport, mut quads) = cx.update(|window, _| {
        (
            window.scale_factor(),
            window.viewport_size(),
            window.painted_quads(),
        )
    });
    quads.sort_by_key(|quad| quad.order);
    let backdrop = quads
        .iter()
        .filter(|quad| {
            let drawn = logical(quad.bounds, scale);
            drawn.size.width >= viewport.width - px(1.)
                && drawn.size.height >= viewport.height - px(1.)
        })
        .map(|quad| quad.order)
        .max()?;
    quads.retain(|quad| quad.order > backdrop);
    let panel = quads
        .iter()
        .find(|quad| quad.background != mark)
        .map(|quad| logical(quad.bounds, scale))?;
    let slack = px(0.25);
    let edge = quads
        .iter()
        .filter(|quad| quad.background == mark)
        .filter_map(|quad| {
            let (run, clip) = (
                logical(quad.bounds, scale),
                logical(quad.content_mask.bounds, scale),
            );
            (run.left() >= clip.left() - slack
                && run.left() >= panel.left()
                && run.left() < panel.right()
                && run.center().y > panel.top()
                && run.center().y < panel.bottom())
            .then_some(run.left())
        })
        .reduce(Pixels::min);
    Some((panel, edge))
}

/// Inside `panel`'s 1 px edge.
fn inside(panel: Bounds<Pixels>) -> Bounds<Pixels> {
    Bounds::from_corners(
        panel.origin + point(px(1.), px(1.)),
        panel.bottom_right() - point(px(1.), px(1.)),
    )
}

/// Every dialog the fixture opens pads its sides by the larger of 16 px and
/// a ghost Button's pull with its focus ring's room (`dialog_ghost_room`): at
/// 13 and 18 pt, in a 461 × 490 half tile and a 1,480 × 800 window, its text
/// starts 16 px inside the panel's 1 px edge at 13 pt and 17 px at 18 pt, on
/// the device pixel (the GitHub panel's at its own inset past that), and the
/// control its footer ends with, where it has a name, ends as far inside the
/// right edge; and every Tab stop of each dialog with a ghost Button at its
/// text edge, and of the palette, draws a whole ring, which no clip cuts,
/// inside the panel's edge.
/// Without the app's room installed, or with the room less the ring's 3 px,
/// the text started 16 px inside at 18 pt, where the 14 px pull and the 3 px
/// ring take 17. The ghost Buttons pulled before that edge, their rings end
/// 3 px inside the panel's edge at 13 pt and on it at 18 pt
/// ([`ghost_buttons_stand_their_labels_on_a_dialogs_text_edge`]).
#[gpui::test]
async fn dialogs_pad_their_sides_for_a_ghost_buttons_ring(cx: &mut TestAppContext) {
    let (_fixture, app, cx, _) = width_walk::open(width_walk::Page::History, cx).await;
    sweep::prepare(&app, cx);
    let mut failures = Vec::new();
    for (interface, width, height) in SIZES {
        cx.simulate_resize(size(px(width), px(height)));
        // The setting too, which the theme editor's draft applies again as
        // it opens.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.interface_text_size = interface;
                appearance::apply_text_sizes(interface, app.settings.code_text_size, window, cx);
                cx.notify();
            })
        });
        width_walk::frame(cx);
        width_walk::frame(cx);
        let expected = px(if interface == 18 { 17. } else { 16. });
        for site in Site::ALL {
            let at = format!("{site:?} at {interface} pt in {width} × {height}");
            site.open(&app, cx).await;
            let Some((panel, edge)) = drawn(cx) else {
                failures.push(format!("{at}: no dialog panel drawn above a backdrop"));
                sweep::close_dialogs(cx, &at, interface, &mut Vec::new());
                continue;
            };
            let scale = cx.update(|window, _| window.scale_factor());
            // On the device pixel: what layout drew, in whole device pixels,
            // against the padding's.
            let device = |inset: Pixels| (f32::from(inset) * scale * 100.).round() / 100.;
            match edge {
                _ if !site.text_at_edge() => {}
                Some(edge) => {
                    let inset = edge - panel.left() - px(1.);
                    if device(inset) != device(expected) {
                        failures.push(format!(
                            "{at}: its text starts {inset:?} inside the panel's edge, not \
                             {expected:?}"
                        ));
                    }
                }
                None => failures.push(format!("{at}: no text drawn in the panel")),
            }
            if let Some(selector) = site.footer_end() {
                match cx.debug_bounds(selector) {
                    Some(end) => {
                        let inset = panel.right() - px(1.) - end.right();
                        if device(inset) != device(expected) {
                            failures.push(format!(
                                "{at}: {selector} ends {inset:?} inside the panel's right \
                                 edge, not {expected:?}"
                            ));
                        }
                    }
                    None => failures.push(format!("{at}: {selector} is not drawn")),
                }
            }
            if site.walked() {
                // The sweep's own findings count too: a stop that draws no
                // ring, as one a scrolling body never brings into view, and
                // a ring a clip cuts.
                let mut sweep_findings = Vec::new();
                let mut seen = 0;
                sweep::walk_each(
                    &app,
                    cx,
                    "tab",
                    &at,
                    interface,
                    &mut sweep_findings,
                    &mut |cx, stop, rings| {
                        let Some((panel, _)) = drawn(cx) else {
                            failures.push(format!("{stop}: no dialog panel drawn"));
                            return;
                        };
                        let inside = inside(panel);
                        let slack = px(0.25);
                        seen += rings.len();
                        for ring in rings {
                            let ring = ring.bounds;
                            if ring.left() < inside.left() - slack
                                || ring.right() > inside.right() + slack
                                || ring.top() < inside.top() - slack
                                || ring.bottom() > inside.bottom() + slack
                            {
                                failures.push(format!(
                                    "{stop}: a ring at {ring:?} leaves the inside of the \
                                     dialog's edge, {inside:?}"
                                ));
                            }
                        }
                    },
                );
                failures.extend(sweep_findings);
                if seen == 0 {
                    failures.push(format!("{at}: Tab drew no ring"));
                }
            }
            sweep::close_dialogs(cx, &at, interface, &mut failures);
            views::tests::settle(&app, cx).await;
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fill a shared control paints at rest in the ghost Buttons' walk
/// (`appearance::mark_controls`), a hue nothing in the app paints, almost
/// clear: a ghost Button transparent at rest paints it, so the walk finds
/// its frame among the painted quads with the clip that cuts it.
fn control_mark() -> Hsla {
    hsla(0.789, 0.987, 0.456, 0.004)
}

/// The clip the last frame drew a shared control's `frame` under, where it
/// drew the frame whole inside it: None for one below a scrolling body's
/// fold, or cut by its edge.
fn whole_in_view(cx: &mut VisualTestContext, frame: Bounds<Pixels>) -> Option<Bounds<Pixels>> {
    // At whatever opacity the variant paints it.
    let mark = control_mark();
    let marked = |background: gpui_kit::Background| {
        background.as_solid().is_some_and(|color| {
            (color.h - mark.h).abs() < 0.001
                && (color.s - mark.s).abs() < 0.001
                && (color.l - mark.l).abs() < 0.001
        })
    };
    let slack = px(0.25);
    cx.update(|window, _| {
        let scale = window.scale_factor();
        window.painted_quads().into_iter().find_map(|quad| {
            let (drawn, clip) = (
                logical(quad.bounds, scale),
                logical(quad.content_mask.bounds, scale),
            );
            let at_frame = (drawn.left() - frame.left()).abs() <= slack
                && (drawn.top() - frame.top()).abs() <= slack
                && (drawn.right() - frame.right()).abs() <= slack
                && (drawn.bottom() - frame.bottom()).abs() <= slack;
            let whole = drawn.left() >= clip.left() - slack
                && drawn.right() <= clip.right() + slack
                && drawn.top() >= clip.top() - slack
                && drawn.bottom() <= clip.bottom() + slack;
            (marked(quad.background) && at_frame && whole).then_some(clip)
        })
    })
}

/// The text runs the last frame drew above the open dialog's backdrop under
/// `clip`, or a clip inside it, whose centre lies in `frame`, in the
/// window's logical pixels, top first:
/// a control's label, where `clip` is the control's own, and not the text of
/// what is drawn over it, as a dialog's footer is over a control below its
/// body's fold.
fn runs_in(
    cx: &mut VisualTestContext,
    frame: Bounds<Pixels>,
    clip: Bounds<Pixels>,
) -> Vec<Bounds<Pixels>> {
    let mark = gpui_kit::Background::from(text_mark());
    let slack = px(0.25);
    let mut runs: Vec<_> = cx.update(|window, _| {
        let (scale, viewport) = (window.scale_factor(), window.viewport_size());
        let quads = window.painted_quads();
        // Above the dialog's backdrop: the page's text beneath it lies
        // under clips inside the dialog's too.
        let backdrop = quads
            .iter()
            .filter(|quad| {
                let drawn = logical(quad.bounds, scale);
                drawn.size.width >= viewport.width - px(1.)
                    && drawn.size.height >= viewport.height - px(1.)
            })
            .map(|quad| quad.order)
            .max()
            .unwrap_or(0);
        quads
            .into_iter()
            .filter(|quad| quad.background == mark && quad.order > backdrop)
            .filter_map(|quad| {
                let (run, under) = (
                    logical(quad.bounds, scale),
                    logical(quad.content_mask.bounds, scale),
                );
                // A Button clips its label to its own content too.
                let within = under.left() >= clip.left() - slack
                    && under.top() >= clip.top() - slack
                    && under.right() <= clip.right() + slack
                    && under.bottom() <= clip.bottom() + slack;
                (within && frame.contains(&run.center())).then_some(run)
            })
            .collect()
    });
    runs.sort_by(|a, b| f32::from(a.top()).total_cmp(&f32::from(b.top())));
    runs
}

/// Where `selector`'s content ends as the last frame laid it out, where it
/// drew its label on one `line`: the label's end, or, for a feedback Button,
/// the end of the state it holds back, icon included, where that lies
/// further, as a copy Button lays out Copied in the cell its label is drawn
/// in (`copy_feedback::laid_out`, which only those Buttons note), so one as
/// wide as Copied beside a shorter label is as wide as its content; measured
/// by its label alone, it read as stretched. Nothing else a Button lays out
/// past its label is measured, as a trailing icon would be: none of the
/// walked Buttons has one.
fn content_end(selector: &str, line: Bounds<Pixels>) -> Pixels {
    copy_feedback::laid_out::contents(selector)
        .into_iter()
        .map(|content| content.right())
        .fold(line.right(), Pixels::max)
}

/// What `site`'s ghost Buttons drew in the last frame, against the dialog's
/// `panel` and its text's `edge`, added to `failures`, and each that stands
/// at the edge whole in view added to `measured` with `opening`, the dialog
/// as it was opened, and read only once in each opening: the first Button of each
/// line of a row stands its frame its pull before the edge, on the device
/// pixel, and starts its label there, or with a leading icon its icon, the
/// icon's width and gap before its label; on one line its frame ends its
/// inset past its label, or past a feedback Button's held-back state
/// ([`content_end`]), as wide as its content rather than stretched across
/// the column; every line of a
/// label without an icon starts there, and together they are as wide as the
/// label, so one wider than its room wraps rather than end in an ellipsis;
/// a Button standing beside a field stays on that line, past the edge, its
/// label whole on one line and its frame as wide as its content; and no
/// Button runs past the dialog's text column on the right.
fn ghost_findings(
    cx: &mut VisualTestContext,
    site: Site,
    (opening, at): (&str, &str),
    (panel, edge): (Bounds<Pixels>, Pixels),
    measured: &mut std::collections::HashSet<String>,
    failures: &mut Vec<String>,
) {
    let (scale, pull, padding, icon, space) = cx.update(|window, cx| {
        (
            window.scale_factor(),
            crate::ghost_pull(window),
            gpui_kit::component::dialog::dialog_side_padding(window, cx),
            appearance::ui_size(16.) + appearance::ui_size(6.),
            views::line_widths(&[(" ", appearance::ui_text(12.))], window),
        )
    });
    let device = |x: Pixels| (f32::from(x) * scale * 100.).round() / 100.;
    let slack = px(0.25);
    let column = panel.right() - px(1.) - padding;
    // The edge is the text the panel draws leftmost, which lies its padding
    // and the site's text inset inside its 1 px edge, on the device pixel
    // nearest that, so a label drawn left of the edge cannot pass for it:
    // text stands on whole device pixels, and at scale 1.25 the 1 px edge is
    // 1.25 of them.
    let inset = edge - panel.left() - px(1.);
    let text = padding + site.text_inset();
    if (f32::from(inset - text) * scale).abs() >= 0.5 {
        failures.push(format!(
            "{at}: the text starts {inset:?} inside the panel's edge, not on the device \
             pixel nearest its padding and text inset, {text:?}"
        ));
    }
    for row in site.ghost_rows() {
        let drawn: Vec<_> = row
            .ghosts
            .iter()
            .filter_map(|(selector, label)| {
                cx.debug_bounds(selector.clone().leak())
                    .map(|frame| (selector, *label, frame))
            })
            .collect();
        for (index, (selector, label, frame)) in drawn.iter().enumerate() {
            if frame.right() > column + slack {
                failures.push(format!(
                    "{at}: {selector} at {frame:?} runs past the text column, which ends at \
                     {column:?}"
                ));
            }
            // Only the first Button of each line stands at the edge.
            let first = !drawn[..index]
                .iter()
                .any(|(_, _, other)| (other.top() - frame.top()).abs() < px(1.));
            if !first {
                continue;
            }
            let Some(clip) = whole_in_view(cx, *frame) else {
                continue;
            };
            let runs = runs_in(cx, *frame, clip);
            let Some(&start) = runs.first() else {
                failures.push(format!("{at}: {selector} at {frame:?} draws no label"));
                continue;
            };
            if !measured.insert(format!("{opening}: {selector}")) {
                continue;
            }
            // As wide as its content: on one line its frame ends its inset
            // past its label, or a feedback Button's held-back state, so a
            // Button stretched across its column, its content centred or
            // not, fails.
            if let [line] = runs.as_slice() {
                let end = content_end(selector, *line);
                if frame.right() > end + pull + px(1.) {
                    failures.push(format!(
                        "{at}: {selector}'s frame ends at {:?}, past its content's end \
                         {end:?} and its inset {pull:?}",
                        frame.right()
                    ));
                }
            }
            if row.beside {
                // Beside the field, past the edge: on a line of its own it
                // would stand at the row's start.
                if frame.left() <= edge + slack {
                    failures.push(format!(
                        "{at}: {selector} at {frame:?} leaves the field's line for the edge \
                         {edge:?}"
                    ));
                }
                if runs.len() != 1 {
                    failures.push(format!(
                        "{at}: {selector} wraps its label beside the field: {runs:?}"
                    ));
                }
            } else if device(frame.left() + pull) != device(edge) {
                failures.push(format!(
                    "{at}: {selector}'s frame starts at {:?}, not its pull, {pull:?}, before \
                     the text's edge {edge:?}",
                    frame.left()
                ));
            }
            if row.icon {
                let content = start.left() - icon;
                if (content - edge).abs() > px(1. / scale) {
                    failures.push(format!(
                        "{at}: {selector}'s icon starts at about {content:?}, not on the \
                         text's edge {edge:?}"
                    ));
                }
                continue;
            }
            for line in runs.iter().filter(|_| !row.beside) {
                if device(line.left()) != device(edge) {
                    failures.push(format!(
                        "{at}: {selector}'s label has a line at {:?}, not on the text's edge \
                         {edge:?}",
                        line.left()
                    ));
                }
            }
            if let Some(label) = label {
                let whole = cx.update(|window, _| {
                    views::line_widths(&[(label, appearance::ui_text(12.))], window)
                });
                let lines = runs.len() as f32;
                let drawn: Pixels = runs.iter().map(|line| line.size.width).sum();
                if drawn + space * (lines - 1.) + px(1.) * lines < whole {
                    failures.push(format!(
                        "{at}: {selector} draws {drawn:?} of its {whole:?} label on {lines} \
                         line(s) in a frame {:?} wide",
                        frame.size.width
                    ));
                }
            }
        }
    }
}

/// Every dialog with a ghost Button standing at its text edge, at 13 and
/// 18 pt in a 461 × 490 half tile and a 1,480 × 800 window, stands it by the
/// rule (DESIGN.md, "One left edge"): the first Button of each line stands
/// its label, or its leading icon, on the dialog's text edge and its frame
/// its pull before it, on the device pixel, and no Button runs past the
/// text column on the right, a label wider than its room wrapping onto lines
/// that start on the edge, as Use current repository signing settings does
/// at 18 pt in the half tile; and every Tab stop draws a whole ring, which no
/// clip cuts, inside the dialog's edge, each Button [`Site::tab_reaches`]
/// names drawing its ring at one stop: a scrolling body reveals the control
/// Tab moves onto. The half tile is read at scale 1.25 too
/// ([`FRACTIONAL`]). Each Button is read whole in view at one size at least. Download an LFS
/// preview and Worktree created open from their openers. Standing their
/// frames on the edge, the Buttons started their content 10 px past it at
/// 13 pt and 14 px at 18 pt; New profile…, the signing Button, Compare local
/// revisions' choices and Copy bug diagnostics, stretched across the
/// dialog, centred it, and the signing Button's label ended in an ellipsis
/// at 18 pt in the half tile. In the dialog's own scrolling body, which
/// reveals nothing, or with no reveal of their own, Compare local
/// revisions' choices took focus below the form's fold in the half tile,
/// and Read configured source's, Open in editor's and the series list's
/// rings were cut at the foot at 18 pt there.
#[gpui::test]
async fn ghost_buttons_stand_their_labels_on_a_dialogs_text_edge(cx: &mut TestAppContext) {
    let (_fixture, app, cx, _) = width_walk::open(width_walk::Page::History, cx).await;
    sweep::prepare(&app, cx);
    let mut failures = Vec::new();
    let mut measured = std::collections::HashSet::new();
    for (interface, width, height, scale) in SIZES
        .map(|(interface, width, height)| (interface, width, height, 2.))
        .into_iter()
        .chain(FRACTIONAL)
    {
        cx.simulate_resize(size(px(width), px(height)));
        // A resize takes the test display's scale, 2.
        cx.update(|window, _| window.set_scale_factor(scale));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.interface_text_size = interface;
                appearance::apply_text_sizes(interface, app.settings.code_text_size, window, cx);
                cx.notify();
            })
        });
        // After the palette the walk and a text size apply, which clears
        // the mark.
        appearance::mark_controls(control_mark());
        width_walk::frame(cx);
        width_walk::frame(cx);
        for site in Site::GHOSTS {
            let at = format!("{site:?} at {interface} pt in {width} × {height} at scale {scale}");
            site.open(&app, cx).await;
            let Some((panel, Some(edge))) = drawn(cx) else {
                failures.push(format!("{at}: no dialog panel with text drawn"));
                sweep::close_dialogs(cx, &at, interface, &mut Vec::new());
                continue;
            };
            ghost_findings(
                cx,
                site,
                (&at, &at),
                (panel, edge),
                &mut measured,
                &mut failures,
            );
            // Every stop draws a whole ring, as the sweep finds it, and Tab
            // reaches each of the site's Buttons it must, drawing its ring.
            let mut sweep_findings = Vec::new();
            let mut reached = std::collections::HashSet::new();
            let ring_room = cx.update(|_, cx| appearance::button_ring_room(cx));
            sweep::walk_each(
                &app,
                cx,
                "tab",
                &at,
                interface,
                &mut sweep_findings,
                &mut |cx, stop, rings| {
                    let Some((panel, edge)) = drawn(cx) else {
                        failures.push(format!("{stop}: no dialog panel drawn"));
                        return;
                    };
                    for selector in site.tab_reaches() {
                        let Some(frame) = cx.debug_bounds(selector) else {
                            continue;
                        };
                        let around = frame.dilate(ring_room);
                        if rings.iter().any(|ring| {
                            (ring.bounds.left() - around.left()).abs() <= px(0.5)
                                && (ring.bounds.top() - around.top()).abs() <= px(0.5)
                                && (ring.bounds.right() - around.right()).abs() <= px(0.5)
                                && (ring.bounds.bottom() - around.bottom()).abs() <= px(0.5)
                        }) {
                            reached.insert(*selector);
                        }
                    }
                    // Read again with each stop's focus, as a scrolling body
                    // reveals a Button below its fold.
                    if let Some(edge) = edge {
                        ghost_findings(
                            cx,
                            site,
                            (&at, stop),
                            (panel, edge),
                            &mut measured,
                            &mut failures,
                        );
                    }
                    let inside = inside(panel);
                    let slack = px(0.25);
                    for ring in rings {
                        let ring = ring.bounds;
                        if ring.left() < inside.left() - slack
                            || ring.right() > inside.right() + slack
                            || ring.top() < inside.top() - slack
                            || ring.bottom() > inside.bottom() + slack
                        {
                            failures.push(format!(
                                "{stop}: a ring at {ring:?} leaves the inside of the dialog's \
                                 edge, {inside:?}"
                            ));
                        }
                    }
                },
            );
            failures.extend(sweep_findings);
            for selector in site.tab_reaches() {
                if !reached.contains(selector) {
                    failures.push(format!("{at}: Tab never draws {selector}'s ring"));
                }
            }
            sweep::close_dialogs(cx, &at, interface, &mut failures);
            views::tests::settle(&app, cx).await;
        }
    }
    for site in Site::GHOSTS {
        for row in site.ghost_rows().into_iter().filter(|row| row.drawn) {
            let (selector, _) = &row.ghosts[0];
            let opened = format!("{site:?} at ");
            let read = format!(": {selector}");
            if !measured
                .iter()
                .any(|key| key.starts_with(&opened) && key.ends_with(&read))
            {
                failures.push(format!(
                    "{site:?}: {selector} is never drawn whole in view at the dialog's edge"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An empty view for a window that only needs the kit's root.
struct Blank;

impl Render for Blank {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// The padding the kit gives a dialog's sides lies on the device pixel at
/// every scale and text size, and is the room the app keeps there
/// (`dialog_ghost_room`) unchanged: at least the pull with the ring's room
/// and the kit's 16 px, and less than a device pixel more. Layout draws the
/// padding at exactly that width, so the GitHub panel's Files fit and
/// Keyboard Shortcuts' `help_width`, which read it back, measure what is
/// drawn. At scale 1.5 and 18 pt the 14 px pull and the 3 px ring summed to
/// 25.5 device pixels, between the two layout can draw, which the Files fit
/// subtracted unrounded; with only that sum rounded, the kit's 16 px, which
/// it keeps at 13 pt, was 25.6 device pixels at scale 1.6 and 19.2 at 1.2.
#[gpui::test]
fn the_ghost_room_lies_on_the_device_pixel(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        native_accessibility::set_overlay_room(cx);
    });
    let (_, cx) = cx.add_window_view(|window, cx| {
        let blank = cx.new(|_| Blank);
        gpui_kit::component::Root::new(blank, window, cx)
    });
    cx.update(|window, cx| appearance::ThemeChoice::Midnight.apply(Some(window), cx));
    let mut failures = Vec::new();
    for scale in [1., 1.2, 1.25, 4. / 3., 1.5, 1.6, 5. / 3., 1.75, 2.] {
        cx.update(|window, _| window.set_scale_factor(scale));
        for interface in [11, 13, 15, 18] {
            let (room, needed, padding) = cx.update(|window, cx| {
                appearance::apply_text_sizes(interface, 12, window, cx);
                (
                    crate::dialog_ghost_room(window, cx),
                    (crate::ghost_pull(window) + appearance::button_ring_room(cx)).max(px(16.)),
                    gpui_kit::component::dialog::dialog_side_padding(window, cx),
                )
            });
            let at = format!("at scale {scale} and {interface} pt");
            let device = f32::from(padding) * scale;
            if (device - device.round()).abs() > 0.001 {
                failures.push(format!("{at}: the sides take {device} device pixels"));
            }
            if padding != room {
                failures.push(format!(
                    "{at}: the sides take {padding:?} for a room of {room:?}"
                ));
            }
            if room < needed - px(0.001) || room >= needed + px(1. / scale) {
                failures.push(format!(
                    "{at}: the room is {room:?} for a pull and ring or the kit's 16 px, \
                     {needed:?}"
                ));
            }
        }
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The Reflog's ghost Buttons as its newest entry's details draw them, in
/// its narrowest window, a 461 × 490 half tile, at 13 and 18 pt, each read
/// at the Tab stop that reveals it whole: Review recovery branch… stands its
/// label on the Reflog's text edge, a field's text inset past its padding,
/// on one line, its frame its pull before that edge and as wide as its label
/// and its inset on either side (186 px at 13 pt, 258 px at 18 pt); Copy
/// commit ID stands its hash glyph there, its label after the glyph's room,
/// on one 28 px line as wide as its glyph, its label and its insets. In a
/// 260 × 490 window at 18 pt, whose column leaves Copy commit ID less room
/// than its content, it shrinks to the column and wraps its label onto
/// lines that start where its first does, drawn whole, its glyph still on
/// the edge and beside the first line, where the label reads from, and
/// pressed, it draws Copied where that line starts in the same frame; its
/// label had ended in an ellipsis in a frame cut to the column. Tab draws
/// each one's whole ring inside the dialog.
#[gpui::test]
async fn the_reflogs_ghost_buttons_stand_on_its_text_edge_as_wide_as_their_content(
    cx: &mut TestAppContext,
) {
    let (_fixture, app, cx, _) = width_walk::open(width_walk::Page::History, cx).await;
    sweep::prepare(&app, cx);
    let mut failures = Vec::new();
    for (interface, width, height) in [(13, 461., 490.), (18, 461., 490.), (18, 260., 490.)] {
        cx.simulate_resize(size(px(width), px(height)));
        sweep::interface_size(cx, interface);
        appearance::mark_controls(control_mark());
        width_walk::frame(cx);
        let at = format!("the Reflog at {interface} pt in {width} × {height}");
        Site::Reflog.open(&app, cx).await;
        let Some((panel, Some(edge))) = drawn(cx) else {
            failures.push(format!("{at}: no dialog panel with text drawn"));
            sweep::close_dialogs(cx, &at, interface, &mut Vec::new());
            continue;
        };
        let (scale, pull, padding, icon, ring_room, one_line) = cx.update(|window, cx| {
            (
                window.scale_factor(),
                crate::ghost_pull(window),
                gpui_kit::component::dialog::dialog_side_padding(window, cx),
                appearance::ui_size(16.) + appearance::ui_size(6.),
                appearance::button_ring_room(cx),
                appearance::ui_size(28.),
            )
        });
        let (review_label, copy_label, space) = cx.update(|window, _| {
            let width = |text| views::line_widths(&[(text, appearance::ui_text(12.))], window);
            (
                width("Review recovery branch…"),
                width("Copy commit ID"),
                width(" "),
            )
        });
        let column = panel.right() - px(1.) - padding;
        let device = |x: Pixels| (f32::from(x) * scale).round();
        // Each Button as the first stop that shows it whole drew it, and
        // whether Tab drew its ring whole inside the dialog's edge.
        let mut read = std::collections::HashMap::new();
        let mut ringed = std::collections::HashSet::new();
        let mut pressed = None;
        let mut sweep_findings = Vec::new();
        sweep::walk_each(
            &app,
            cx,
            "tab",
            &at,
            interface,
            &mut sweep_findings,
            &mut |cx, _, rings| {
                for selector in ["review-reflog-recovery", "copy-reflog-oid"] {
                    let Some(frame) = cx.debug_bounds(selector) else {
                        continue;
                    };
                    if !read.contains_key(selector)
                        && let Some(clip) = whole_in_view(cx, frame)
                    {
                        // Where Copy commit ID's wrapping cell stands its
                        // glyph.
                        let glyph = (selector == "copy-reflog-oid")
                            .then(|| cx.debug_bounds("copy-reflog-oid/rest/icon"))
                            .flatten();
                        read.insert(selector, (frame, runs_in(cx, frame, clip), glyph));
                    }
                    let around = frame.dilate(ring_room);
                    let inside = inside(panel);
                    if rings.iter().any(|ring| {
                        ring.whole
                            && (ring.bounds.left() - around.left()).abs() <= px(0.5)
                            && (ring.bounds.right() - around.right()).abs() <= px(0.5)
                            && (ring.bounds.top() - around.top()).abs() <= px(0.5)
                            && (ring.bounds.bottom() - around.bottom()).abs() <= px(0.5)
                            && ring.bounds.left() >= inside.left() - px(0.25)
                            && ring.bounds.right() <= inside.right() + px(0.25)
                    }) {
                        ringed.insert(selector);
                        // Wrapped, Copy commit ID pressed where Tab brought
                        // it whole into view: its frame and the rest label's
                        // place before the press, then what it draws.
                        if selector == "copy-reflog-oid" && width <= 300. && pressed.is_none() {
                            let rest = cx.debug_bounds("copy-reflog-oid/rest/text");
                            copy_feedback::tests::click(cx, "copy-reflog-oid");
                            width_walk::frame(cx);
                            pressed = Some((
                                frame,
                                rest,
                                copy_feedback::tests::shows(cx, "copy-reflog-oid"),
                                cx.debug_bounds("copy-reflog-oid"),
                                cx.debug_bounds("copy-reflog-oid/copied/text"),
                            ));
                        }
                    }
                }
            },
        );
        for selector in ["review-reflog-recovery", "copy-reflog-oid"] {
            if !ringed.contains(selector) {
                failures.push(format!("{at}: Tab never draws {selector}'s whole ring"));
            }
        }
        // Copied takes the wrapped label's place: the frame keeps its
        // bounds, and the word starts where the label's first line does.
        if width <= 300. {
            match pressed.take() {
                Some((before, Some(rest), shows, Some(after), Some(word)))
                    if shows == copy_feedback::tests::Shows::Copied
                        && after == before
                        && (word.left() - rest.left()).abs() <= px(0.25)
                        && (word.top() - rest.top()).abs() <= px(0.25) => {}
                pressed => failures.push(format!(
                    "{at}: Copy commit ID pressed draws (frame before, rest label, shows, \
                     frame after, Copied) {pressed:?}, not Copied in the label's place in \
                     the same frame"
                )),
            }
        }
        // In the half tile, Review recovery branch… stands on one line.
        match read.get("review-reflog-recovery") {
            Some((frame, runs, _)) if width > 300. => {
                if device(frame.left() + pull) != device(edge) {
                    failures.push(format!(
                        "{at}: Review recovery branch…'s frame starts at {:?}, not its pull \
                         {pull:?} before the edge {edge:?}",
                        frame.left()
                    ));
                }
                match runs.as_slice() {
                    [line] if (line.left() - edge).abs() <= px(0.5) => {}
                    runs => failures.push(format!(
                        "{at}: Review recovery branch…'s label is drawn {runs:?}, not on one \
                         line from the edge {edge:?}"
                    )),
                }
                if (frame.size.width - (review_label + pull * 2.)).abs() > px(1.) {
                    failures.push(format!(
                        "{at}: Review recovery branch… is {:?} wide, not its {review_label:?} \
                         label and its {pull:?} inset on either side",
                        frame.size.width
                    ));
                }
            }
            Some(_) => {}
            None => failures.push(format!(
                "{at}: Review recovery branch… is never drawn whole"
            )),
        }
        match read.get("copy-reflog-oid") {
            Some((frame, runs, glyph)) => {
                if device(frame.left() + pull) != device(edge) {
                    failures.push(format!(
                        "{at}: Copy commit ID's frame starts at {:?}, not its pull {pull:?} \
                         before the edge {edge:?}",
                        frame.left()
                    ));
                }
                if frame.right() > column + px(0.25) {
                    failures.push(format!(
                        "{at}: Copy commit ID at {frame:?} runs past the column, which ends at \
                         {column:?}"
                    ));
                }
                if let Some(first) = runs.first() {
                    if (first.left() - icon - edge).abs() > px(1. / scale) {
                        failures.push(format!(
                            "{at}: Copy commit ID's glyph starts at about {:?}, not on the \
                             edge {edge:?}",
                            first.left() - icon
                        ));
                    }
                    for line in runs {
                        if (line.left() - first.left()).abs() > px(0.5) {
                            failures.push(format!(
                                "{at}: Copy commit ID has a line at {:?}, not where its first \
                                 starts, {:?}",
                                line.left(),
                                first.left()
                            ));
                        }
                    }
                } else {
                    failures.push(format!("{at}: Copy commit ID draws no label"));
                }
                let drawn: Pixels = runs.iter().map(|line| line.size.width).sum();
                let lines = runs.len() as f32;
                if drawn + space * (lines - 1.) + px(1.) * lines < copy_label {
                    failures.push(format!(
                        "{at}: Copy commit ID draws {drawn:?} of its {copy_label:?} label on \
                         {lines} line(s)"
                    ));
                }
                let content = pull + icon + copy_label + pull;
                if width > 300. {
                    if runs.len() != 1
                        || (frame.size.width - content).abs() > px(1.)
                        || (frame.size.height - one_line).abs() > px(0.5)
                    {
                        failures.push(format!(
                            "{at}: Copy commit ID at {frame:?} draws {runs:?}, not one line \
                             {content:?} wide and {one_line:?} tall"
                        ));
                    }
                } else {
                    if runs.len() < 2 || frame.size.height <= one_line {
                        failures.push(format!(
                            "{at}: Copy commit ID at {frame:?} draws {runs:?} in a column \
                             narrower than its {content:?} content, not wrapped"
                        ));
                    }
                    // Its glyph stands beside the label's first line, where
                    // the label reads from, not centred on its lines.
                    let beside = |glyph: &Bounds<Pixels>, first: &Bounds<Pixels>| {
                        let middle = glyph.center().y;
                        first.top() <= middle
                            && middle <= first.bottom()
                            && (middle - first.center().y).abs() <= px(1.)
                    };
                    match (glyph, runs.first()) {
                        (Some(glyph), Some(first)) if beside(glyph, first) => {}
                        (glyph, first) => failures.push(format!(
                            "{at}: Copy commit ID's glyph at {glyph:?} does not stand beside \
                             its label's first line {first:?}"
                        )),
                    }
                }
            }
            None => failures.push(format!("{at}: Copy commit ID is never drawn whole")),
        }
        sweep::close_dialogs(cx, &at, interface, &mut failures);
        views::tests::settle(&app, cx).await;
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A test view of two copy Buttons labelled "Copy", narrower than the
/// Copied each lays out beside it in one cell: one as wide as its content,
/// one stretched across its column.
struct CopyRows;

impl Render for CopyRows {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let copy = |id: &'static str, cx: &App| {
            copy_feedback::copy_button(copy_feedback::Mark::new(id, 1), "Copy", "", cx)
        };
        div()
            .size_full()
            .bg(rgb(0x202020))
            .text_bg(text_mark())
            .flex()
            .flex_col()
            .gap_4()
            .p_4()
            .child(div().flex().child(copy("copy-as-wide-as-copied", cx)))
            .child(div().flex().flex_col().child(copy("copy-stretched", cx)))
    }
}

/// The ghost walk measures a feedback Button's held-back state, icon
/// included, beside its label ([`content_end`]): a copy Button labelled
/// "Copy", as wide as the
/// Copied it holds back, ends its inset past that content, though its label
/// alone ends more than a pixel short of it, which the walk read as
/// stretched; the same Button stretched across its column ends past its
/// content and its inset, and the walk fails it.
#[gpui::test]
fn the_ghost_walk_measures_a_buttons_widest_content(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let rows = cx.new(|_| CopyRows);
        gpui_kit::component::Root::new(rows, window, cx)
    });
    cx.update(|window, cx| appearance::ThemeChoice::Midnight.apply(Some(window), cx));
    cx.simulate_resize(size(px(600.), px(300.)));
    width_walk::frame(cx);
    width_walk::frame(cx);
    let pull = cx.update(|window, _| crate::ghost_pull(window));
    let read = |cx: &mut VisualTestContext, selector: &'static str| {
        let frame = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is drawn"));
        let runs = runs_in(cx, frame, frame);
        let [line] = runs.as_slice() else {
            panic!("{selector} draws {runs:?}");
        };
        (frame, *line, content_end(selector, *line))
    };
    let (frame, line, end) = read(cx, "copy-as-wide-as-copied");
    assert!(
        frame.right() > line.right() + pull + px(1.),
        "its label alone ends short of its frame's inset: {frame:?}, {line:?}"
    );
    assert!(
        (frame.right() - (end + pull)).abs() <= px(1.),
        "it ends its inset past its widest content: {frame:?}, {end:?}"
    );
    let (frame, _, end) = read(cx, "copy-stretched");
    assert!(
        frame.right() > end + pull + px(1.),
        "a stretched Button ends past its widest content and its inset: {frame:?}, {end:?}"
    );
}
