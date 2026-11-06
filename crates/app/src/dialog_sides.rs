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

/// A dialog the fixture opens from History: the ones with a ghost Button at
/// their text edge that it can open (Git profiles, the profile form,
/// Activity, Compare local revisions' choices, the rewritten-series review,
/// About's Copy bug diagnostics and Add remote's Fetch rules…), the command
/// palette, and the dialogs that size themselves from the padding (the
/// GitHub panel's, whose Files fit reads it, and the theme editor's). Not
/// here: Download an LFS preview and Worktree created, whose openers the
/// fixture cannot reach (a working file's LFS pointer, a created worktree's
/// plan). The signing site is no dialog of its own: it is the profile form's
/// Use current repository signing settings, walked with the form.
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
            Site::RevisionChoices => {
                open_dialog(app, cx, &name, |app, window, cx| {
                    app.open_compare_form(window, cx)
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
        }
    }
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
/// right edge; and Tab draws every ring of each dialog with a ghost
/// Button at its text edge, and the palette's, inside the panel's edge.
/// Without the app's room installed, or with the room less the ring's 3 px,
/// the text started 16 px inside at 18 pt, where the 14 px pull and the 3 px
/// ring take 17. The ghost Buttons stand on that edge today, their frames
/// not yet pulled before it, so their rings lie 13 and 14 px inside it.
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
                // The sweep's own findings, such as a ring a scrolling body
                // cuts inside the dialog, belong to the focus sweeps; this
                // walk asks only that each ring lie inside the dialog.
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
