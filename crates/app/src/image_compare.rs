//! GPU composition over bounded worker-decoded images. No pixels are copied on drag.
use crate::*;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::prelude::FluentBuilder;

/// The comparison modes, in their group's order.
const MODES: [(Mode, &str); 3] = [
    (Mode::SideBySide, "Side by side"),
    (Mode::Overlay, "Overlay"),
    (Mode::Wipe, "Wipe"),
];
/// The zoom choices, in their group's order; 0 fits the image.
const ZOOMS: [(f32, &str); 4] = [(0., "Fit"), (0.5, "50%"), (1., "100%"), (2., "200%")];
/// The pan's directions, in their group's order: each arrow's ID, its glyph,
/// its name, which the Pan menu's item reads, and how far it moves the
/// images.
const PANS: [(&str, &str, &str, [f32; 2]); 4] = [
    ("pan-left", "←", "Pan left", [80., 0.]),
    ("pan-right", "→", "Pan right", [-80., 0.]),
    ("pan-up", "↑", "Pan up", [0., 80.]),
    ("pan-down", "↓", "Pan down", [0., -80.]),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Mode {
    #[default]
    SideBySide,
    Overlay,
    Wipe,
}
#[derive(Clone, Copy)]
enum Drag {
    Pan(Point<Pixels>, [f32; 2]),
    Wipe,
}
pub(super) struct State {
    mode: Mode,
    amount: f32,
    pan: [f32; 2],
    bounds: Bounds<Pixels>,
    drag: Option<Drag>,
    playback: gif_playback::Playback,
}
impl Clone for State {
    fn clone(&self) -> Self {
        Self {
            mode: self.mode,
            amount: self.amount,
            pan: self.pan,
            bounds: self.bounds,
            drag: None,
            playback: self.playback.clone(),
        }
    }
}

#[derive(Debug)]
struct Geometry {
    sizes: [[f32; 2]; 2],
    extent: [f32; 2],
    source_scale: f32,
}
fn source_geometry(sides: [Option<[u32; 4]>; 2]) -> Geometry {
    // Compare in one common source coordinate system. Different thumbnail reductions
    // must never conceal an image resize or make equal-size originals look different.
    let source_scale = sides.iter().flatten().fold(1.0_f32, |scale, side| {
        scale
            .min(side[2] as f32 / side[0].max(1) as f32)
            .min(side[3] as f32 / side[1].max(1) as f32)
    });
    let sizes = sides.map(|side| {
        side.map_or([0., 0.], |s| {
            [s[0] as f32 * source_scale, s[1] as f32 * source_scale]
        })
    });
    let extent = [
        sizes[0][0].max(sizes[1][0]).max(1.),
        sizes[0][1].max(sizes[1][1]).max(1.),
    ];
    Geometry {
        sizes,
        extent,
        source_scale,
    }
}
impl Default for State {
    fn default() -> Self {
        Self {
            mode: Mode::SideBySide,
            amount: 0.5,
            pan: [0.; 2],
            bounds: Bounds::default(),
            drag: None,
            playback: gif_playback::Playback::default(),
        }
    }
}

fn layout(
    viewport: [f32; 2],
    extent: [f32; 2],
    zoom: f32,
    pan: [f32; 2],
) -> (f32, [f32; 2], [f32; 2]) {
    let scale = if zoom == 0. {
        ((viewport[0] - 32.).max(1.) / extent[0].max(1.))
            .min((viewport[1] - 32.).max(1.) / extent[1].max(1.))
            .min(1.)
    } else {
        zoom
    };
    let maximum = [
        (extent[0] * scale - viewport[0]).max(0.),
        (extent[1] * scale - viewport[1]).max(0.),
    ];
    let offset = [pan[0].clamp(-maximum[0], 0.), pan[1].clamp(-maximum[1], 0.)];
    let origin = [
        ((viewport[0] - extent[0] * scale) / 2.).max(0.) + offset[0],
        ((viewport[1] - extent[1] * scale) / 2.).max(0.) + offset[1],
    ];
    (scale, origin, maximum)
}

impl GitTurtle {
    fn gif_context(
        &self,
    ) -> (
        gif_playback::SourceKey,
        u64,
        Option<Arc<gif_playback::Timeline>>,
    ) {
        let key = self
            .images
            .each_ref()
            .map(|image| image.as_ref().map(|image| image.id));
        let Some(Content::Images { old, new }) = self.content.as_deref() else {
            return (key, 0, None);
        };
        let duration = [old, new]
            .iter()
            .filter_map(|side| side.animation.as_ref())
            .map(|animation| animation.duration_ms)
            .max()
            .unwrap_or(0);
        let timeline = new
            .animation
            .as_ref()
            .filter(|animation| animation.end_ms.len() > 1)
            .or_else(|| {
                old.animation
                    .as_ref()
                    .filter(|animation| animation.end_ms.len() > 1)
            })
            .cloned();
        (key, duration, timeline)
    }
    pub(super) fn apply_motion_preferences(&mut self, cx: &App) {
        if cx.reduce_motion() {
            self.image_comparison
                .playback
                .pause(std::time::Instant::now());
        }
    }
    fn gif_step(&mut self, next: bool, cx: &mut Context<Self>) {
        let (key, duration, Some(timeline)) = self.gif_context() else {
            return;
        };
        let position =
            self.image_comparison
                .playback
                .position(key, duration, std::time::Instant::now());
        let index = timeline.frame_at(position);
        let index = if next {
            (index + 1).min(timeline.end_ms.len() - 1)
        } else {
            index.saturating_sub(1)
        };
        let position = if index == 0 {
            0
        } else {
            timeline.end_ms[index - 1]
        };
        self.image_comparison.playback.seek(key, position);
        cx.notify();
    }
    fn gif_controls(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (key, duration, timeline) = self.gif_context();
        timeline?;
        let p = palette(cx);
        let playing = !cx.reduce_motion() && self.image_comparison.playback.playing(key);
        let position =
            self.image_comparison
                .playback
                .position(key, duration, std::time::Instant::now());
        let mut labels = Vec::new();
        let mut truncated = false;
        let mut complete = None;
        if let Some(Content::Images { old, new }) = self.content.as_deref() {
            for (index, side) in [old, new].iter().enumerate() {
                if let Some(animation) = &side.animation {
                    labels.push(format!(
                        "{} {}/{}",
                        if self.is_quick_source() {
                            "Frame"
                        } else if index == 0 {
                            "Before"
                        } else {
                            "After"
                        },
                        animation.frame_at(position) + 1,
                        animation.end_ms.len()
                    ));
                    truncated |= animation.truncated;
                    if animation.truncated && side.captured.is_some() {
                        complete = complete.or(side.system_preview);
                    }
                }
            }
        }
        const SEGMENT: &str = "Playback is limited to the decoded segment (120 frames, 30 seconds, 16 million output pixels).";
        let note: SharedString = if cx.reduce_motion() {
            "Reduce Motion is enabled. Use First, Previous, and Next frame for static inspection."
                .into()
        } else if let (true, Some(offer)) = (truncated, complete) {
            // Only where the truncated side offers the action that shows it.
            format!("{SEGMENT} {}", offer.complete_animation()).into()
        } else if truncated {
            SEGMENT.into()
        } else if self.is_quick_source() {
            "Frame controls pause playback for inspection.".into()
        } else {
            "Both versions share one clock; a shorter animation holds its final frame. Frame controls pause playback for inspection.".into()
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .px_3()
                .py_2()
                .border_b_1()
                .border_color(rgb(p.border))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .child(
                            button(
                                "gif-playback-toggle",
                                if playing { "Pause GIF" } else { "Play GIF" },
                                "",
                                playing,
                            )
                            .toggled(playing)
                            .disabled(cx.reduce_motion())
                            .on_click(cx.listener(|this, _, _, cx| {
                                if cx.reduce_motion() {
                                    return;
                                }
                                let (key, duration, _) = this.gif_context();
                                this.image_comparison.playback.toggle(
                                    key,
                                    duration,
                                    std::time::Instant::now(),
                                );
                                cx.notify();
                            })),
                        )
                        .child(
                            button("gif-first-frame", "First frame", "", false).on_click(
                                cx.listener(|this, _, _, cx| {
                                    let (key, _, _) = this.gif_context();
                                    this.image_comparison.playback.seek(key, 0);
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(
                            button("gif-previous-frame", "Previous frame", "", false)
                                .on_click(cx.listener(|this, _, _, cx| this.gif_step(false, cx))),
                        )
                        .child(
                            button("gif-next-frame", "Next frame", "", false)
                                .on_click(cx.listener(|this, _, _, cx| this.gif_step(true, cx))),
                        )
                        .child(
                            div()
                                .text_size(appearance::ui_text(11.))
                                .text_color(rgb(p.muted))
                                .child(format!(
                                    "{:.2} / {:.2} s · {}",
                                    position as f64 / 1000.,
                                    duration as f64 / 1000.,
                                    labels.join(" · ")
                                )),
                        ),
                )
                .child(
                    div()
                        .text_size(appearance::ui_text(10.))
                        .text_color(rgb(p.muted))
                        .debug_selector(|| "gif-playback-note".into())
                        .child(note),
                )
                .into_any_element(),
        )
    }
    fn image_geometry(&self) -> Geometry {
        let sides = match self.content.as_deref() {
            Some(Content::Images { old, new }) => [old, new].map(|side| {
                side.image.as_ref().map(|image| {
                    [
                        image.original_width,
                        image.original_height,
                        image.width,
                        image.height,
                    ]
                })
            }),
            _ => [None, None],
        };
        source_geometry(sides)
    }
    pub(super) fn move_image_drag(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if self.page != AppPage::Repository
            || !matches!(self.content.as_deref(), Some(Content::Images { .. }))
        {
            self.end_image_drag();
            return;
        }
        let Some(drag) = self.image_comparison.drag else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.image_comparison.drag = None;
            return;
        }
        match drag {
            Drag::Wipe => {
                self.image_comparison.amount =
                    (f32::from(event.position.x - self.image_comparison.bounds.origin.x)
                        / f32::from(self.image_comparison.bounds.size.width).max(1.))
                    .clamp(0., 1.)
            }
            Drag::Pan(start, offset) => {
                let delta = event.position - start;
                self.pan_image([
                    offset[0] + f32::from(delta.x),
                    offset[1] + f32::from(delta.y),
                ]);
            }
        }
        cx.notify();
    }
    pub(super) fn end_image_drag(&mut self) {
        self.image_comparison.drag = None;
    }
    /// Shows the comparison in `mode`, from the start of its pan.
    pub(super) fn set_image_mode(&mut self, mode: Mode) {
        self.image_comparison.mode = mode;
        self.image_comparison.pan = [0.; 2];
        self.end_image_drag();
    }
    /// Shows the comparison at `zoom` (0 fits it), from the start of its pan.
    pub(super) fn set_image_zoom(&mut self, zoom: f32) {
        self.zoom = zoom;
        self.image_comparison.pan = [0.; 2];
    }
    /// Sets the overlay's opacity or the wipe's width, for a test.
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn set_image_amount(&mut self, amount: f32) {
        self.image_comparison.amount = amount;
    }
    /// How far the images are panned, for a test.
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn image_pan(&self) -> [f32; 2] {
        self.image_comparison.pan
    }
    /// Which ways the images can still move, in [`PANS`]' order, in the
    /// images' bounds as last laid out, which panning reads too.
    fn pan_room(&self) -> [bool; 4] {
        let bounds = self.image_comparison.bounds;
        let (_, _, most) = layout(
            [f32::from(bounds.size.width), f32::from(bounds.size.height)],
            self.image_geometry().extent,
            self.zoom,
            self.image_comparison.pan,
        );
        pan_room(self.image_comparison.pan, most)
    }
    /// Moves the zoomed images by `delta`, as a pan control does.
    fn pan_by(&mut self, delta: [f32; 2]) {
        let pan = self.image_comparison.pan;
        self.pan_image([pan[0] + delta[0], pan[1] + delta[1]]);
    }
    fn pan_image(&mut self, requested: [f32; 2]) {
        let bounds = self.image_comparison.bounds;
        let (_, _, max) = layout(
            [f32::from(bounds.size.width), f32::from(bounds.size.height)],
            self.image_geometry().extent,
            self.zoom,
            requested,
        );
        self.image_comparison.pan = [
            requested[0].clamp(-max[0], 0.),
            requested[1].clamp(-max[1], 0.),
        ];
    }
    /// The comparison in Compare's preview column, `width` wide in the frame
    /// it is drawn ([`GitTurtle::render_preview`] lays it out there), which
    /// its toolbar's groups fit, with Compare's `footer` at its foot, where
    /// the footer gives way before the image's minimum. What lies beneath
    /// the toolbar takes the column's width again from its own bounds.
    pub(super) fn render_image_comparison(
        &self,
        old: &worker::ImageSide,
        new: &worker::ImageSide,
        width: Pixels,
        footer: Option<crate::views::CompareFooter>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = palette(cx);
        let source = self.is_quick_source();
        let state = &self.image_comparison;
        let both = self.images.iter().all(Option::is_some);
        // Narrower than its labelled group, the toolbar's padding aside, each
        // group keeps one row as a menu that reads the current choice, as
        // the text modes do.
        let room = width - window.rem_size() * 1.5;
        let scale = format!(
            "100% comparison scale equals {:.1}% of source pixels. Source and decoded dimensions are shown below.",
            self.image_geometry().source_scale * 100.
        );
        let mode_group = group_width(&MODES.map(|(_, name)| name), &MODE_GROUP, window);
        let modes_labelled = mode_group <= room;
        let mode_label = MODES
            .iter()
            .find(|(mode, _)| *mode == state.mode)
            .map_or("Side by side", |(_, name)| *name);
        let zoom_label = ZOOMS
            .iter()
            .find(|(zoom, _)| *zoom == self.zoom)
            .map_or("Fit", |(_, name)| *name);
        // The forms the zooms and the pan take at this width: the pan never
        // takes a row of its own, and each form changes at one width.
        let adjust_line = adjust_line(window);
        let forms = ToolbarWidths {
            room,
            gap: window.rem_size() * 0.5,
            modes: (!source).then_some(mode_group),
            zooms: [
                group_width(&ZOOMS.map(|(_, name)| name), &ZOOM_GROUP, window),
                widest_menu(&ZOOMS.map(|(_, name)| name), &WIDEST_ZOOM, window),
            ],
            pan: (!source).then(|| {
                [
                    group_width(&PANS.map(|(_, arrow, _, _)| arrow), &PAN_ARROWS, window),
                    menu_width("Pan", &PAN_MENU, window),
                ]
            }),
            adjust: (!source && state.mode != Mode::SideBySide)
                .then(|| adjust_width(adjust_line, window)),
        }
        .forms(self.zoom > 0.);
        // Which ways the zoomed images can still move, in PANS' order: a pan
        // control that cannot move them is unavailable.
        let pan_room = self.pan_room();
        #[cfg(test)]
        PAN_ROOM.set(pan_room);
        let modes = if modes_labelled {
            let mut modes = div().debug_selector(|| "image-modes".into()).flex().gap_1();
            for (mode, label) in MODES {
                modes = modes.child(
                    button(label, label, "", state.mode == mode)
                        .toggled(state.mode == mode)
                        .tooltip(mode_tooltip(mode))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_image_mode(mode);
                            cx.notify();
                        })),
                );
            }
            crate::in_frame::probed(modes, "image-modes").into_any_element()
        } else {
            let current = state.mode;
            let label = mode_label;
            let owner = cx.entity().downgrade();
            crate::in_frame::probed(
                crate::views::compact_control_label(
                    button("image-mode-menu", label, "", false),
                    "image-mode-menu",
                    format!("Comparison mode: {label}"),
                ),
                "image-mode-menu",
            )
            .debug_selector(|| "image-mode-menu".into())
            // Transparent, like the text modes' menu, it gives its inset
            // back to the toolbar's padding, so its label starts where the
            // captions' names do and its focus ring stays in the column.
            .px(crate::appearance::ui_size(crate::ICON_INSET))
            .ml(-crate::appearance::ui_size(crate::ICON_INSET))
            // As wide as its widest choice, so Overlay and Wipe, and Side by
            // side, move nothing after it.
            .min_w(widest_menu(
                &MODES.map(|(_, name)| name),
                &MODE_MENU,
                window,
            ))
            .dropdown_caret(true)
            .tooltip(format!("Comparison mode · {}", mode_tooltip(current)))
            .app_menu(move |menu, _, _| {
                // Built when opened; a choice acts on the comparison as it
                // is then, as a mode button does.
                MODES.iter().fold(menu, |menu, &(mode, name)| {
                    let owner = owner.clone();
                    menu.item(PopupMenuItem::new(name).checked(mode == current).on_click(
                        move |_, _, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                this.set_image_mode(mode);
                                cx.notify();
                            });
                        },
                    ))
                })
            })
            .into_any_element()
        };
        let zooms = if forms.zooms_labelled {
            let mut zooms = div().debug_selector(|| "image-zooms".into()).flex().gap_1();
            for (zoom, label) in ZOOMS {
                zooms = zooms.child(
                    button(label, label, "", self.zoom == zoom)
                        .toggled(self.zoom == zoom)
                        .tooltip(scale.clone())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_image_zoom(zoom);
                            cx.notify();
                        })),
                );
            }
            crate::in_frame::probed(zooms, "image-zooms").into_any_element()
        } else {
            let current = self.zoom;
            let label = zoom_label;
            // Where not even the Pan menu fits beside it, the zoom menu
            // offers the pan's directions after the zooms.
            let pans = forms.pan == Some(Pan::InZoomMenu);
            let owner = cx.entity().downgrade();
            crate::in_frame::probed(
                crate::views::compact_control_label(
                    button("image-zoom-menu", label, "", false),
                    "image-zoom-menu",
                    if pans {
                        format!("Zoom and pan: {label}")
                    } else {
                        format!("Zoom: {label}")
                    },
                ),
                "image-zoom-menu",
            )
            .debug_selector(|| "image-zoom-menu".into())
            .px(crate::appearance::ui_size(crate::ICON_INSET))
            // As wide as its widest choice, so choosing a zoom moves nothing
            // after it: "100%" once took the modes' row from a "Fit" that had
            // shared it, and the captions moved down a row.
            .min_w(widest_menu(
                &ZOOMS.map(|(_, name)| name),
                &WIDEST_ZOOM,
                window,
            ))
            .dropdown_caret(true)
            .tooltip(if pans {
                format!("Zoom and pan · {scale}")
            } else {
                format!("Zoom · {scale}")
            })
            .app_menu(move |menu, _, _| {
                let menu = ZOOMS.iter().fold(menu, |menu, &(zoom, name)| {
                    let owner = owner.clone();
                    menu.item(PopupMenuItem::new(name).checked(zoom == current).on_click(
                        move |_, _, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                this.set_image_zoom(zoom);
                                cx.notify();
                            });
                        },
                    ))
                });
                if pans {
                    pan_items(menu.separator(), owner.clone(), pan_room)
                } else {
                    menu
                }
            })
            .into_any_element()
        };
        // At a zoom other than Fit, the pan follows the zooms in their group,
        // so it never takes a toolbar row of its own: the four arrows where
        // they fit beside the zooms, otherwise one Pan menu there, and where
        // not even that fits, the zoom menu's own items (above). A row of
        // arrows once put the captions 48 px lower at 18 pt in a 461 × 490
        // window, and Before's control under the preview's clip.
        let pan = match forms.pan {
            Some(Pan::Arrows) => {
                let arrows = PANS.iter().zip(&pan_room).fold(
                    div()
                        .debug_selector(|| "image-pan-arrows".into())
                        .flex()
                        .items_center()
                        .gap_1(),
                    |arrows, (&(id, arrow, name, delta), &room)| {
                        arrows.child(
                            button(id, arrow, "", false)
                                .debug_selector(move || id.into())
                                .accessibility_label(name)
                                .tooltip(name)
                                .disabled(!room)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.pan_by(delta);
                                    cx.notify();
                                })),
                        )
                    },
                );
                Some(crate::in_frame::probed(arrows, "image-pan-arrows").into_any_element())
            }
            Some(Pan::Menu) => {
                let owner = cx.entity().downgrade();
                Some(
                    crate::in_frame::probed(
                        crate::views::compact_control_label(
                            button("image-pan-menu", "Pan", "", false),
                            "image-pan-menu",
                            "Pan",
                        ),
                        "image-pan-menu",
                    )
                    .debug_selector(|| "image-pan-menu".into())
                    .px(crate::appearance::ui_size(crate::ICON_INSET))
                    .dropdown_caret(true)
                    .tooltip("Pan · Move the zoomed images left, right, up or down")
                    .app_menu(move |menu, _, _| pan_items(menu, owner.clone(), pan_room))
                    .into_any_element(),
                )
            }
            Some(Pan::InZoomMenu) | None => None,
        };
        let zoom_group = div()
            .flex()
            .items_center()
            .gap_2()
            .child(zooms)
            .children(pan);
        // The overlay's or the wipe's adjustment keeps the toolbar's trailing
        // edge, so the zooms stay beside the modes in every mode. Where its
        // labelled line does not fit the toolbar, the line keeps only its
        // value, its words in its tooltip, its accessible name and the
        // buttons' names, and the group wraps rather than run past the
        // column.
        let adjust_labelled = adjust_width(adjust_line, window) <= room;
        let mut adjust = div()
            .debug_selector(|| "image-adjust".into())
            .ml_auto()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1();
        if state.mode != Mode::SideBySide {
            let label = if state.mode == Mode::Wipe {
                "Before width"
            } else {
                "After opacity"
            };
            let value = format!("{:.0}%", state.amount * 100.);
            let whole = format!("{label} · {value}");
            // As wide as its widest line, so the amount moves nothing.
            let (shown, widest) = if adjust_labelled {
                (whole.clone(), adjust_line[0])
            } else {
                (value, adjust_line[1])
            };
            // Read as its whole line in either form: its value alone once
            // read only "50%".
            let read = whole.clone();
            adjust = adjust.child(crate::native_accessibility::static_text(
                div()
                    .id("image-adjust-line")
                    .debug_selector(|| "image-adjust-line".into())
                    .min_w(widest)
                    .text_size(crate::appearance::ui_text(11.))
                    .text_color(rgb(p.muted))
                    .when(!adjust_labelled, |line| {
                        line.tooltip(move |window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(whole.clone())
                                .build(window, cx)
                        })
                    })
                    .child(shown),
                read,
            ));
            for (id, label, delta) in [("image-less", "−", -0.1), ("image-more", "+", 0.1)] {
                adjust = adjust.child(
                    button(id, label, "", false)
                        .accessibility_label(format!(
                            "{} {} · {:.0} percent",
                            if delta < 0. { "Decrease" } else { "Increase" },
                            if state.mode == Mode::Wipe {
                                "Before width"
                            } else {
                                "After opacity"
                            },
                            state.amount * 100.
                        ))
                        .disabled(
                            !both
                                || (delta < 0. && state.amount <= 0.)
                                || (delta > 0. && state.amount >= 1.),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.image_comparison.amount =
                                (this.image_comparison.amount + delta).clamp(0., 1.);
                            cx.notify();
                        })),
                );
            }
            adjust = adjust.child(
                button("image-center", "50:50", "", false)
                    .disabled(!both)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.image_comparison.amount = 0.5;
                        cx.notify();
                    })),
            );
        }
        let animation_label = if old.animation.is_some() || new.animation.is_some() {
            "GIF frame preview"
        } else {
            "First image/frame"
        };
        let scale_notice = if source {
            format!(
                "{animation_label} · 100% = {:.1}% of source size",
                self.image_geometry().source_scale * 100.
            )
        } else {
            format!(
                "{animation_label} · 100% = {:.1}% of source size · Both versions share the same scale",
                self.image_geometry().source_scale * 100.
            )
        };
        let guidance = if source {
            "Source image · Drag or scroll to pan · Use the zoom controls for keyboard adjustment"
        } else if !both {
            "An absent or unavailable side stays empty. The available image is shown at full opacity."
        } else if state.mode == Mode::Wipe {
            "Drag the divider · Before on the left, After on the right · Tab to controls for keyboard adjustment"
        } else {
            "Linked zoom and pan · Drag or scroll to move · Equal source coordinates stay aligned"
        };
        let mut copies = Vec::new();
        let names = if source {
            ["Source"; 2]
        } else {
            ["Before", "After"]
        };
        let details = [0, 1].map(|index| side_details(index, [old, new][index]));
        // The one short line an absent side draws where its two do not fit.
        let shorts = [0, 1].map(|index| absent_short(index, [old, new][index]));
        // A side without an image says why where its image would be, in its
        // half side by side or across a source image's room; overlaid or
        // wiped, where it has no room of its own, beneath its name whatever
        // the room. Only the dimensions are a label that gives way.
        let own_room = source || state.mode == Mode::SideBySide;
        // The sides drawn: a source image shows only its own.
        let drawn = move |index: usize| !source || index == 1;
        let kinds = [old, new].map(|side| {
            if side.image.is_some() {
                Details::Dimensions
            } else if own_room {
                Details::InItsPlace
            } else {
                Details::Kept
            }
        });
        let stage_min = crate::appearance::ui_size(STAGE_MIN);
        let opener =
            crate::system_preview::Opener::new(cx.entity().downgrade(), self.preview_showing);
        // Each side's open control and what it offers, which its caption
        // draws, and why its last click opened nothing, which a band beneath
        // the captions says.
        let mut offers: [Option<(crate::system_preview::Target, crate::system_preview::Offer)>; 2] =
            [None, None];
        let mut answers: [Option<crate::system_preview::Answer>; 2] = [None, None];
        for (index, side) in [old, new].into_iter().enumerate() {
            let label = if index == 0 { "Before" } else { "After" };
            let side_path = self
                .selected_file
                .and_then(|selected| self.files.get(selected))
                .and_then(|file| {
                    if index == 0 {
                        file.old_path.as_deref()
                    } else {
                        file.new_path.as_deref()
                    }
                });
            if let Some(literal) = side.literal_source.clone() {
                // The control's id names the side; the mark adds its path.
                let mark = copy_feedback::Mark::new(("copy-image-source", index), side_path);
                let copy_mark = mark.clone();
                copies.push(copy_feedback::copy_button(mark,format!("Copy {label} source"),"Copy the literal source bytes as UTF-8; rendered pixels never become a Git patch",cx).on_click(move |_,window,cx|copy_feedback::copy(window,cx,Some(copy_mark.clone()),&format!("{label} source"),literal.to_string())));
            }
            if let (Some(bytes), Some(offer), false) =
                (&side.captured, side.system_preview, source && index == 0)
            {
                let label = names[index];
                let path = side_path.unwrap_or(std::path::Path::new("image.bin"));
                let target = opener.target(
                    ("image-system-preview", index),
                    label,
                    bytes,
                    path,
                    "image",
                    offer,
                );
                answers[index] = target.answered(cx);
                offers[index] = Some((target, offer));
            }
        }
        // The band saying why a click opened nothing, and the comparison's
        // notes: their text's size and their lines' height and padding.
        let band_size = crate::appearance::ui_text(11.);
        let band_lines = BandLines {
            line: text_line(band_size, window),
            padding: window.rem_size() * 0.5,
        };
        let note_size = crate::appearance::ui_text(10.);
        let note_line = window
            .text_style()
            .line_height
            .to_pixels(note_size.into(), window.rem_size());
        let note_pad = window.rem_size() * 0.5;
        let footer_room = footer.as_ref().map(|_| {
            window.pixel_snap(crate::appearance::ui_size(
                crate::views::COMPARE_FOOTER_HEIGHT,
            ))
        });
        // The image, built here and drawn only where the room leaves it a
        // pixel.
        let canvases: Vec<AnyElement> = if source {
            vec![self.image_canvas(Some(1), cx)]
        } else if state.mode == Mode::SideBySide {
            vec![
                self.image_canvas(Some(0), cx),
                self.image_canvas(Some(1), cx),
            ]
        } else {
            vec![self.image_canvas(None, cx)]
        };
        // What the image's area is named where it carries the words that gave
        // way: the images it draws, never one a side does not have; with
        // none, it is named as the band is.
        let pictured = [old, new].map(|side| side.image.is_some());
        let image_label = match (source, pictured) {
            (true, [_, true]) => Some("Source image"),
            (false, [true, true]) => Some("Before and After images"),
            (false, [true, false]) => Some("Before image"),
            (false, [false, true]) => Some("After image"),
            _ => None,
        };
        // What the band is named where the image is not drawn: the band and
        // the words that gave way, never an image.
        let details_label = if source {
            "Source details"
        } else {
            "Comparison details"
        };
        // Beneath the toolbar, the captions, the image and Compare's footer,
        // laid out for the room the column leaves them in the frame they are
        // drawn: what gives way is chosen from that room, never from the room
        // a frame before laid out.
        let beneath = crate::in_frame::in_frame(move |bounds, window, cx| {
            // What the column's width decides, from its width in this frame.
            let width = bounds.size.width;
            // The captions share the column equally, and their open controls
            // take one form and one place, chosen from the narrower room a
            // side's name leaves its control: labelled where the label fits
            // beside the name, otherwise its icon, and on the name's line
            // where the icon's scaled 28 px fits there, otherwise both on the
            // line beneath, so the sides' names and details stay level.
            let caption_width = width / if source { 1. } else { 2. };
            let room = offers
                .iter()
                .enumerate()
                .filter(|(_, offered)| offered.is_some())
                .map(|(index, _)| caption_room(index, caption_width, names[index], window))
                .reduce(|narrower, room| narrower.min(room));
            let inline = room.is_none_or(|room| room >= crate::appearance::ui_size(28.));
            let controls = offers.map(|offered| {
                offered.map(|(target, offer)| {
                    let form = offer.form(room.unwrap_or_default(), window);
                    (target, form)
                })
            });
            // The band shows each side's short line in that side's half,
            // whole where the image keeps its usable minimum beneath it and
            // otherwise on two lines, and always ends at a whole line.
            let band_room = caption_width - window.rem_size() * 1.5;
            let natural = answers.each_ref().map(|answer| {
                answer.as_ref().map_or(0, |answer| {
                    wrapped_lines(answer.brief(), band_size, band_room, window).min(64)
                })
            });
            let band = answers
                .iter()
                .any(Option::is_some)
                .then(|| (band_lines, natural.into_iter().max().unwrap_or(0)));
            // Whether each note needs a second line to be whole.
            let note_room = width - window.rem_size() * 1.5;
            let wraps = [
                (scale_notice.as_str(), &SCALE_LINE),
                (guidance, &GUIDANCE_LINE),
            ]
            .map(|(text, cache)| one_line_width(text, cache, note_size, window) > note_room);
            // Overlaid or wiped, a side without an image keeps why beneath its
            // name: whole beside the dimensions, and on one line ending in an
            // ellipsis where they give way, cut there where it wraps to more.
            let caption_text = caption_width - window.rem_size() * 1.5;
            let kept_wraps = [0, 1].map(|i| {
                kinds[i] == Details::Kept
                    && wrapped_lines(
                        &one_line(&details[i]),
                        crate::appearance::ui_text(11.),
                        caption_text,
                        window,
                    ) > 1
            });
            let captions = Captions {
                names,
                details: &details,
                kinds,
                source,
                controls: &controls,
                inline,
                kept_wraps,
            };
            let fit = Beneath {
                room: bounds.size.height,
                names: crate::in_frame::measured_height(
                    captions.build(false, [false; 2], &p, cx),
                    width,
                    window,
                    cx,
                ),
                dimensions: crate::in_frame::measured_height(
                    captions.build(true, [false; 2], &p, cx),
                    width,
                    window,
                    cx,
                ),
                footer: footer_room,
                notes: (note_line + note_pad) * 2.,
                band,
                stage: stage_min,
            }
            .fit();
            // The comparison's notes take two lines each where the room
            // beneath the captions, after any band, keeps the image its
            // usable minimum beneath them, one where only that does, and
            // otherwise leave the image their room, their words staying in
            // its accessible description: labels give way before content.
            let note_lines = note_lines(
                fit.below - fit.band_height,
                wraps,
                2,
                note_line,
                note_pad,
                stage_min,
            );
            // An image-less side's reason, where its image would be: whole
            // where the image's room holds it, otherwise on the whole lines
            // it holds, the last ending in an ellipsis with the whole line in
            // its tooltip. It is static text read by that whole line, so the
            // side's name leaves it out; where the room holds no line, the
            // name keeps it, in its accessible name and tooltip.
            let area =
                fit.below - fit.band_height - notes_height(wraps, note_lines, note_line, note_pad);
            let empty_lines = ((area - window.rem_size()) / band_lines.line)
                .floor()
                .clamp(0., 64.) as usize;
            let said = [0, 1].map(|i| {
                fit.image_shown && empty_lines > 0 && drawn(i) && kinds[i] == Details::InItsPlace
            });
            let empties = said.contains(&true).then(|| {
                div()
                    .debug_selector(|| "image-side-empties".into())
                    .absolute()
                    .inset_0()
                    .flex()
                    .children([0, 1].into_iter().filter(|i| drawn(*i)).map(|i| {
                        let empty = said[i].then(|| {
                            // An absent side's two lines where the room holds
                            // both whole, and otherwise its one short line,
                            // whole where it fits and only then ending in an
                            // ellipsis; the tooltip and the side's line read
                            // keep both sentences.
                            let text = details[i].clone();
                            let drawn = match shorts[i] {
                                Some(short)
                                    if wrapped_lines(&text, band_size, caption_text, window)
                                        > empty_lines =>
                                {
                                    short.to_owned()
                                }
                                _ => text.clone(),
                            };
                            let cut = drawn != text
                                || wrapped_lines(&drawn, band_size, caption_text, window)
                                    > empty_lines;
                            let whole = text.clone();
                            native_accessibility::static_text(
                                div()
                                    .id(("image-side-empty", i))
                                    .debug_selector(move || format!("image-side-empty-{i}")),
                                text,
                            )
                            .min_w_0()
                            .text_center()
                            .text_size(band_size)
                            .text_color(rgb(p.muted))
                            .line_clamp(empty_lines)
                            .text_ellipsis()
                            .when(cut, |line| {
                                line.tooltip(move |window, cx| {
                                    gpui_kit::component::tooltip::Tooltip::new(whole.clone())
                                        .build(window, cx)
                                })
                            })
                            .child(
                                div()
                                    .debug_selector({
                                        let drawn = drawn.clone();
                                        move || format!("image-side-empty-{i}: {drawn}")
                                    })
                                    .child(drawn),
                            )
                        });
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .px_3()
                            .py_2()
                            .children(empty)
                    }))
            });
            // The words of the notes and the footer that gave way, which one
            // drawn node keeps in its accessible description: the image where
            // it is drawn, otherwise the band, and otherwise the captions,
            // which every layout draws, so they always reach a drawn node.
            // The notes give way wherever a line each would leave the image
            // less than its minimum, drawn or not, and so does the footer,
            // its room going to the image, the band or, where they alone do
            // not fit above it, the captions.
            let mut left = Vec::new();
            if note_lines == 0 {
                left.extend([scale_notice.clone(), guidance.to_owned()]);
            }
            let footer = match footer {
                Some(footer) if !fit.footer => {
                    left.push(footer.line.to_string());
                    None
                }
                footer => footer.map(|footer| footer.element),
            };
            let described = !left.is_empty();
            let description = SharedString::from(left.join(". "));
            let band_keeps = described && !fit.image_shown && band.is_some();
            let captions_keep = described && !fit.image_shown && band.is_none();
            let band = band.map(|_| {
                div()
                    .debug_selector(|| "image-open-notes".into())
                    .flex()
                    .flex_shrink_0()
                    // Where the image is not drawn, the band reaches the
                    // column's foot, the footer having given way to it.
                    .when(!fit.image_shown, |band| band.flex_1())
                    // Where the captions leave less than one line and its
                    // rule, the band still stays inside the room.
                    .max_h(fit.below.max(px(0.)))
                    .overflow_hidden()
                    .children(
                        answers
                            .into_iter()
                            .enumerate()
                            .filter(|(index, _)| !source || *index == 1)
                            .map(|(i, answer)| {
                                // Both halves keep the captions' inset, so they
                                // share the column as the captions do, and only
                                // a side with a line has the band's surface and
                                // rule, which span the half whose text it
                                // carries. Where the image is not drawn the band
                                // has no rule of its own, which would double the
                                // status bar's beneath it with nothing between them.
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .px_3()
                                    .when_some(answer, |half, answer| {
                                        let clamp =
                                            (natural[i] > fit.band_lines).then_some(fit.band_lines);
                                        half.debug_selector(move || format!("image-side-note-{i}"))
                                            .bg(rgb(p.panel))
                                            .when(fit.image_shown, |half| {
                                                half.border_b_1().border_color(rgb(p.border))
                                            })
                                            .when(fit.band_padded, |half| half.py_1())
                                            .child(native_accessibility::recorded(
                                                answer.short_line(clamp, cx),
                                            ))
                                    })
                            }),
                    )
            });
            // The band keeps one ID path and one node, a group, on both sides
            // of the image's last pixel, so each line in it keeps its
            // accessibility node and the node holding it, and its tooltip: a
            // new path is a new node, and GPUI writes a node only for an
            // element with a role, so a group only where the image was not
            // drawn moved the line beneath a new node there, which a screen
            // reader can announce again. Where the image is not drawn, the
            // group is named for what is drawn and described by the words
            // that gave way.
            let band = band.map(|band| {
                if band_keeps {
                    // Only the band drawn in this frame records its name and
                    // description for tests.
                    crate::views::note_compact_label("image-open-notes", &description);
                    crate::views::note_compact_label(
                        "image-open-notes-name",
                        &SharedString::from(details_label),
                    );
                }
                native_accessibility::recorded(
                    div()
                        .id("image-open-notes")
                        .role(Role::Group)
                        .flex()
                        .flex_col()
                        .map(|wrapper| {
                            if fit.image_shown {
                                wrapper.flex_shrink_0()
                            } else {
                                wrapper.flex_1().min_h_0()
                            }
                        })
                        .when(band_keeps, |wrapper| {
                            wrapper
                                .aria_label(details_label)
                                .aria_description(description.clone())
                        })
                        .child(crate::in_frame::probed(band, "image-open-notes")),
                )
            });
            let canvas = fit.image_shown.then(|| {
                // Only the image drawn in this frame records its name and
                // description for tests.
                let name = image_label.unwrap_or(details_label);
                if described {
                    crate::views::note_compact_label("image-canvas-area", &description);
                    crate::views::note_compact_label(
                        "image-canvas-area-name",
                        &SharedString::from(name),
                    );
                }
                // The image's node holds only the images it draws. A side's
                // line saying why it has none lies over it, in its room, as
                // the node's sibling: a screen reader presents an image by
                // its name without reading what it holds, so inside it the
                // line was reached by no one.
                crate::in_frame::probed(
                    div()
                        .id("image-canvas-area")
                        .debug_selector(|| "image-canvas-area".into())
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .child(native_accessibility::recorded(
                            div()
                                .id("image-canvases")
                                .size_full()
                                .min_w_0()
                                .flex()
                                .when(described, |area| {
                                    area.role(if image_label.is_some() {
                                        Role::Image
                                    } else {
                                        Role::Group
                                    })
                                    .aria_label(name)
                                    .aria_description(description.clone())
                                })
                                .children(canvases),
                        ))
                        .children(empties),
                    "image-canvas-area",
                )
            });
            // The captions are a group in every layout, so what they hold
            // keeps the node holding it as the room changes. Where neither
            // the image nor the band is drawn, as where the room is shorter
            // than the captions, the group is named for what is drawn and
            // described by the words that gave way, which no other drawn
            // node keeps. The rows measured above are never drawn and write
            // no node.
            let captions = native_accessibility::recorded(
                captions
                    .build(fit.dimensions, said, &p, cx)
                    .id("image-captions")
                    .role(Role::Group)
                    .when(captions_keep, |row| {
                        row.aria_label(details_label)
                            .aria_description(description.clone())
                    }),
            );
            div()
                .debug_selector(|| "image-captions-and-stage".into())
                .size_full()
                .flex()
                .flex_col()
                .child(captions)
                // Beneath the captions: the band saying why a click opened
                // nothing takes its own lines there, beneath its side's
                // caption, so the captions stay put and the image, then its
                // notes, give way to it; the notes follow the image.
                .child(crate::in_frame::probed(
                    div()
                        .debug_selector(|| "image-stage-room".into())
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .children(band)
                        .children(canvas)
                        .children(comparison_note(
                            "image-source-scale",
                            scale_notice,
                            note_lines,
                            &SCALE_LINE,
                            width,
                            window,
                            &p,
                        ))
                        .children(comparison_note(
                            "image-interaction-guidance",
                            guidance.into(),
                            note_lines,
                            &GUIDANCE_LINE,
                            width,
                            window,
                            &p,
                        )),
                    "image-stage-room",
                ))
                .children(footer)
                .into_any_element()
        })
        .named("image-comparison");
        div()
            .size_full()
            .flex()
            .flex_col()
            // Only copies of literal source have a row of their own; each
            // open control sits in its side's caption.
            .when(!copies.is_empty(), |element| {
                element.child(
                    div()
                        .debug_selector(|| "image-captured-actions".into())
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .px_3()
                        .py_1()
                        .children(copies),
                )
            })
            .children(self.gif_controls(cx))
            .child(crate::in_frame::probed(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .bg(rgb(p.panel))
                    .border_b_1()
                    .border_color(rgb(p.border))
                    .when(!source, |element| element.child(modes))
                    .child(zoom_group)
                    // Only overlaid or wiped does the adjustment have controls;
                    // side by side it takes no place in the row.
                    .when(!source && state.mode != Mode::SideBySide, |element| {
                        element.child(adjust)
                    }),
                "image-toolbar",
            ))
            .child(beneath)
            .into_any_element()
    }
    fn image_canvas(&self, side: Option<usize>, cx: &mut Context<Self>) -> AnyElement {
        let colors = palette(cx);
        let geometry = self.image_geometry();
        let extent = geometry.extent;
        let zoom = self.zoom;
        let state = self.image_comparison.clone();
        let both = self.images.iter().all(Option::is_some);
        let weak = cx.weak_entity();
        let (key, duration, timeline) = self.gif_context();
        let position =
            self.image_comparison
                .playback
                .position(key, duration, std::time::Instant::now());
        let playing = !cx.reduce_motion()
            && timeline.is_some()
            && self.image_comparison.playback.playing(key);
        let mut view = div()
            .id(("image-composite", side.unwrap_or(2)))
            .relative()
            .flex_1()
            .size_full()
            .min_w_0()
            .overflow_hidden()
            .cursor(CursorStyle::OpenHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.image_comparison.drag =
                        Some(Drag::Pan(event.position, this.image_comparison.pan));
                    cx.notify();
                }),
            )
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                let delta = event.delta.pixel_delta(px(24.));
                let pan = this.image_comparison.pan;
                this.pan_image([pan[0] + f32::from(delta.x), pan[1] + f32::from(delta.y)]);
                cx.stop_propagation();
                cx.notify();
            }))
            .child(checkerboard(colors))
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let _ = weak.update(cx, |this, _| {
                            this.image_comparison.bounds = bounds;
                        });
                    },
                    move |_, _, window, _| {
                        // A frame is requested only while this canvas is painted
                        // and explicit playback is active. Hidden/paused previews
                        // have no timer or recursively scheduled callback.
                        if playing && side != Some(0) && window.is_window_active() {
                            window.request_animation_frame();
                        }
                    },
                )
                .absolute()
                .inset_0(),
            );
        for (index, image) in self.images.iter().enumerate() {
            if side.is_some_and(|side| side != index) {
                continue;
            }
            let Some(image) = image.clone() else {
                continue;
            };
            let frame_index = match self.content.as_deref() {
                Some(Content::Images { old, new }) => [old, new][index]
                    .animation
                    .as_ref()
                    .map_or(0, |timeline| timeline.frame_at(position)),
                _ => 0,
            };
            let opacity = if side.is_none() && state.mode == Mode::Overlay && index == 1 && both {
                state.amount
            } else {
                1.
            };
            let state = state.clone();
            let logical_size = geometry.sizes[index];
            let layer = canvas(
                |_, _, _| (),
                move |bounds, _, window, cx| {
                    crate::image_lifetime::track(&image, window, cx);
                    let (scale, origin, _) = layout(
                        [f32::from(bounds.size.width), f32::from(bounds.size.height)],
                        extent,
                        zoom,
                        state.pan,
                    );
                    let image_bounds = Bounds::new(
                        bounds.origin + point(px(origin[0]), px(origin[1])),
                        size(px(logical_size[0] * scale), px(logical_size[1] * scale)),
                    );
                    let mut mask = bounds;
                    if side.is_none() && state.mode == Mode::Wipe && both {
                        if index == 0 {
                            mask.size.width *= state.amount;
                        } else {
                            mask.origin.x += bounds.size.width * state.amount;
                            mask.size.width *= 1. - state.amount;
                        }
                    }
                    let _ = window.paint_image(
                        mask,
                        image_bounds,
                        Corners::default(),
                        image.clone(),
                        frame_index,
                        false,
                    );
                },
            )
            .absolute()
            .inset_0();
            view = view.child(div().absolute().inset_0().opacity(opacity).child(layer));
        }
        if side.is_none() && state.mode == Mode::Wipe && both {
            view = view.child(
                div()
                    .id("wipe-divider")
                    .absolute()
                    .left(relative(state.amount))
                    .top_0()
                    .bottom_0()
                    .w(px(12.))
                    .ml(px(-6.))
                    .cursor(CursorStyle::ResizeLeftRight)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.image_comparison.drag = Some(Drag::Wipe);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(5.))
                            .w(px(2.))
                            .h_full()
                            .bg(rgb(colors.accent)),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(relative(0.5))
                            .left(px(-8.))
                            .w(px(28.))
                            .h(crate::appearance::ui_size(40.))
                            .rounded(px(8.))
                            .bg(rgb(colors.accent))
                            .text_color(rgb(colors.accent_foreground))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child("↔"),
                    ),
            );
        }
        view.into_any_element()
    }
}

/// What side `index` says where its file is absent: that it has no image,
/// and on a second line why, which holds in any comparison, not only a
/// commit's: Before has no file where it was added, After none where it was
/// deleted. "No image on this side" alone left the cause unsaid, and once
/// the footer's "Added" gave way nothing on screen said the file was added.
pub(crate) fn absent_side(index: usize) -> &'static str {
    if index == 0 {
        "No image on this side\nThe file was added"
    } else {
        "No image on this side\nThe file was deleted"
    }
}

/// The one short line side `index` draws where its file is absent and its
/// two lines ([`absent_side`]) cannot both be drawn whole: the cause is the
/// point of the second line, so it is said in the fewest words, and only
/// this line ends in an ellipsis where even it does not fit (two lines once
/// lost the cause at 461 × 490, "No image on / this side / The file…").
/// `None` for a side with an image or one that says why it could not be
/// shown.
fn absent_short(index: usize, side: &worker::ImageSide) -> Option<&'static str> {
    (side.image.is_none() && side.message.is_none()).then_some(if index == 0 {
        "No image: file added"
    } else {
        "No image: file deleted"
    })
}

/// `details` on one line, as an absent side keeps it beneath its name in
/// Overlay and Wipe where that line holds it: its lines joined.
fn one_line(details: &str) -> String {
    details.replace('\n', " · ")
}

/// What side `index`'s caption says below its name: its dimensions and
/// format, with the preview's own where it was scaled and any notice, or why
/// it shows no image ([`absent_side`] where its file is absent).
fn side_details(index: usize, side: &worker::ImageSide) -> String {
    let details = side
        .image
        .as_ref()
        .map(|image| {
            format!(
                "{} × {} · {}{}",
                image.original_width,
                image.original_height,
                image.format,
                if image.width != image.original_width || image.height != image.original_height {
                    format!(" · preview {} × {}", image.width, image.height)
                } else {
                    String::new()
                }
            )
        })
        .unwrap_or_else(|| {
            side.message
                .clone()
                .unwrap_or_else(|| absent_side(index).into())
        });
    if side.image.is_some() {
        side.message
            .as_ref()
            .map_or(details.clone(), |notice| format!("{details} · {notice}"))
    } else {
        details
    }
}

thread_local! {
    /// Each side caption's name, laid out.
    static BEFORE_CAPTION: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static AFTER_CAPTION: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    /// The labelled mode and zoom groups' labels, laid out.
    static MODE_GROUP: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static ZOOM_GROUP: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    /// The pan's four arrows, laid out.
    static PAN_ARROWS: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    /// Each menu's label, laid out: the modes', the widest zoom's and the Pan
    /// menu's.
    static MODE_MENU: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static WIDEST_ZOOM: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static PAN_MENU: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    /// The overlay's or the wipe's adjustment: its line and its controls.
    static ADJUST_LINE: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static ADJUST_VALUE: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static ADJUST_GROUP: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    /// The scale notice and the guidance, each laid out as one line.
    static SCALE_LINE: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static GUIDANCE_LINE: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
}

/// The room side `index`'s caption leaves its open control on its first line
/// in a caption `caption` wide: the caption less its `px_3` either side and
/// `gap_2`, and its 11 pt medium `name`.
pub(crate) fn caption_room(index: usize, caption: Pixels, name: &str, window: &Window) -> Pixels {
    let cache = if index == 0 {
        &BEFORE_CAPTION
    } else {
        &AFTER_CAPTION
    };
    let text = crate::views::cached_text_width(cache, name, window, || {
        let size = crate::appearance::ui_text(11.);
        crate::views::shaped_width(name, size, FontWeight::MEDIUM, window).ceil()
    });
    caption - window.rem_size() * (0.75 * 2. + 0.5) - text
}

/// The labelled group of `names` as the toolbar draws it: each the shared
/// `button` helper's 10 px of padding either side of its 12 pt label, at
/// least its 28 px square, `gap_1` apart. Laid out again only when the
/// names, the font or the sizes change.
fn group_width(
    names: &[&str],
    cache: &'static std::thread::LocalKey<crate::views::TextWidth>,
    window: &Window,
) -> Pixels {
    crate::views::cached_text_width(cache, &names.join("\n"), window, || {
        names
            .iter()
            .map(|name| {
                (crate::views::line_widths(&[(name, crate::appearance::ui_text(12.))], window)
                    + crate::appearance::ui_size(20.))
                .max(crate::appearance::ui_size(28.))
            })
            .sum::<Pixels>()
    }) + window.rem_size() * 0.25 * names.len().saturating_sub(1) as f32
}

/// One of the toolbar's menus reading `label` with its caret: the icon's
/// inset either side, the shared `button` helper's 6 px gap and the caret, as
/// the text modes' menu is measured. Laid out again only when the label, the
/// font or the sizes change.
fn menu_width(
    label: &str,
    cache: &'static std::thread::LocalKey<crate::views::TextWidth>,
    window: &Window,
) -> Pixels {
    crate::appearance::ui_size(crate::ICON_INSET * 2. + 6.)
        + crate::views::cached_text_width(cache, label, window, || {
            crate::views::line_widths(&[(label, crate::appearance::ui_text(12.))], window)
        })
        + window.rem_size() * 0.875
}

/// A menu reading the widest of `names`, which is its width whatever it
/// reads, so choosing one never changes the toolbar's rows or forms.
fn widest_menu(
    names: &[&str],
    cache: &'static std::thread::LocalKey<crate::views::TextWidth>,
    window: &Window,
) -> Pixels {
    crate::appearance::ui_size(crate::ICON_INSET * 2. + 6.)
        + crate::views::cached_text_width(cache, &names.join("\n"), window, || {
            names
                .iter()
                .map(|name| {
                    crate::views::line_widths(&[(name, crate::appearance::ui_text(12.))], window)
                })
                .fold(px(0.), Pixels::max)
        })
        + window.rem_size() * 0.875
}

/// The overlay's or the wipe's 11 pt line at its widest, whatever the mode
/// and the amount: labelled ("After opacity · 100%"), and its value alone.
fn adjust_line(window: &Window) -> [Pixels; 2] {
    let size = crate::appearance::ui_text(11.);
    [
        crate::views::cached_text_width(&ADJUST_LINE, "labelled", window, || {
            ["After opacity · 100%", "Before width · 100%"]
                .iter()
                .map(|line| crate::views::line_widths(&[(line, size)], window))
                .fold(px(0.), Pixels::max)
        }),
        crate::views::cached_text_width(&ADJUST_VALUE, "100%", window, || {
            crate::views::line_widths(&[("100%", size)], window)
        }),
    ]
}

/// The overlay's or the wipe's adjustment as the toolbar draws it labelled,
/// at its widest `line`: the line, then − and + and 50:50, `gap_1` apart.
fn adjust_width(line: [Pixels; 2], window: &Window) -> Pixels {
    line[0] + window.rem_size() * 0.25 + group_width(&["−", "+", "50:50"], &ADJUST_GROUP, window)
}

/// Which ways the images can still move from `pan`, at most `most` from
/// their start each way, in [`PANS`]' order: left and up move them back
/// towards their start, right and down further from it.
fn pan_room(pan: [f32; 2], most: [f32; 2]) -> [bool; 4] {
    [
        pan[0] < -0.01,
        pan[0] > -most[0] + 0.01,
        pan[1] < -0.01,
        pan[1] > -most[1] + 0.01,
    ]
}

/// `menu` with the pan's four directions, each moving the images as its
/// arrow does.
fn pan_items(
    menu: gpui_kit::component::menu::PopupMenu,
    owner: WeakEntity<GitTurtle>,
    room: [bool; 4],
) -> gpui_kit::component::menu::PopupMenu {
    PANS.iter()
        .zip(room)
        .fold(menu, |menu, (&(_, _, name, delta), room)| {
            let owner = owner.clone();
            menu.item(
                PopupMenuItem::new(name)
                    .disabled(!room)
                    .on_click(move |_, _, cx| {
                        let _ = owner.update(cx, |this, cx| {
                            this.pan_by(delta);
                            cx.notify();
                        });
                    }),
            )
        })
}

/// What the image toolbar's groups take on one row in the frame being drawn,
/// from which [`ToolbarWidths::forms`] chooses the zooms' form and the pan's.
#[derive(Clone, Copy, Debug)]
struct ToolbarWidths {
    /// The toolbar's width inside its padding.
    room: Pixels,
    /// The `gap_2` between its groups, and between the zooms and the pan.
    gap: Pixels,
    /// The modes labelled, where they are drawn, whatever form they take.
    modes: Option<Pixels>,
    /// The zooms labelled, and as their menu, which is as wide as its widest
    /// choice whatever the zoom.
    zooms: [Pixels; 2],
    /// The pan's four arrows, and the one Pan menu they fold into, where the
    /// images pan (not a source image).
    pan: Option<[Pixels; 2]>,
    /// The overlay's or the wipe's adjustment labelled at its widest, where
    /// drawn, whatever form it takes.
    adjust: Option<Pixels>,
}

/// The form the pan's controls take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pan {
    /// Four arrows after the zooms.
    Arrows,
    /// One Pan menu after the zooms, offering the four directions.
    Menu,
    /// The four directions in the zoom menu, after the zooms.
    InZoomMenu,
}

/// What [`ToolbarWidths::forms`] chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ToolbarForms {
    zooms_labelled: bool,
    /// The pan's form, where the images are zoomed past Fit.
    pan: Option<Pan>,
}

impl ToolbarWidths {
    /// Whether the toolbar's groups, with its zoom group `zooms` wide, fit
    /// one row: the modes and the adjustment counted labelled, the widest
    /// they are, so each form below changes at one width.
    fn fits(&self, zooms: Pixels) -> bool {
        let groups = [self.modes, Some(zooms), self.adjust];
        let width = groups
            .iter()
            .flatten()
            .fold(px(0.), |row, &group| row + group);
        let gaps = groups.iter().flatten().count().saturating_sub(1) as f32;
        width + self.gap * gaps <= self.room
    }

    /// The zooms' form, and the pan's where the images are `panning`
    /// (zoomed past Fit). The pan follows the zooms in their group and takes
    /// a place beside them only where every group fits one row with it, so
    /// it never adds a row: four arrows beside labelled zooms, otherwise one
    /// Pan menu, and where not even that fits, the zoom menu's own items. The
    /// zooms keep their labels only where the Pan menu fits beside them so,
    /// at Fit too, so choosing a zoom never changes their form. Each form
    /// changes at one width, so a wider toolbar is never less labelled.
    ///
    /// A known choice follows: the modes, and overlaid or wiped the
    /// adjustment, are counted labelled whatever form they take, the modes
    /// even where they are drawn as their narrower menu. So below the
    /// labelled modes, the zoom menu and the Pan menu with their gaps, about
    /// 356 px of toolbar side by side at 13 pt (616.5 px overlaid or wiped,
    /// with the labelled adjustment), the pan always takes the zoom menu's
    /// items, even where the mode menu would leave the Pan menu room beside
    /// the zooms; counting the forms drawn instead would change the pan's
    /// form with the width the mode menu reads or the amount.
    ///
    /// Another: the adjustment is counted only overlaid or wiped, where it
    /// is drawn, so switching between Side by side and Overlay or Wipe can
    /// change the zooms' form, and the pan's with them, at a width where the
    /// labelled zooms fit one row beside the modes and the Pan menu but not
    /// beside the adjustment too: from 488 px of toolbar up to 724.5 px in
    /// `the_pan_folds_before_it_takes_a_row`'s widths, the zooms are
    /// labelled side by side and a menu overlaid or wiped. Counting the
    /// adjustment side by side as well would keep their form across the
    /// switch, but side by side every toolbar in that range, which wide
    /// windows give, would lose the labelled zooms it holds.
    fn forms(&self, panning: bool) -> ToolbarForms {
        let [labelled, menu] = self.zooms;
        let zooms_labelled = match self.pan {
            Some([_, pan]) => self.fits(labelled + self.gap + pan),
            None => labelled <= self.room,
        };
        let pan = self.pan.filter(|_| panning).map(|[arrows, pan]| {
            if zooms_labelled && self.fits(labelled + self.gap + arrows) {
                Pan::Arrows
            } else if zooms_labelled || self.fits(menu + self.gap + pan) {
                Pan::Menu
            } else {
                Pan::InZoomMenu
            }
        });
        ToolbarForms {
            zooms_labelled,
            pan,
        }
    }
}

#[cfg(test)]
thread_local! {
    /// Which ways the zoomed images could move as last drawn, for a test.
    pub(crate) static PAN_ROOM: std::cell::Cell<[bool; 4]> = const { std::cell::Cell::new([true; 4]) };
}

/// What `mode` shows, for its button's tooltip and the menu's.
fn mode_tooltip(mode: Mode) -> &'static str {
    if mode == Mode::SideBySide {
        "Compare both versions with linked zoom and pan"
    } else {
        "Align both versions at their top-left corner on one canvas"
    }
}

/// The image's usable minimum beneath the captions, before scaling: a fitted
/// image 64 px tall inside the 16 px margins [`layout`] keeps on each side.
/// A document's page, diagram or model keeps the same beneath its header's
/// line wherever the room holds one line above it, and otherwise gives way
/// for that one line, as the image does for its band
/// ([`crate::system_preview::header_ends`]).
pub(crate) const STAGE_MIN: f32 = 96.;

/// The side captions' row an image comparison draws beneath its toolbar, over
/// the row's rule: each side's name, then the control that opens it, as a
/// document's header has them, and its details beneath where they stay.
/// The name's line is as tall as the scaled 28 px control and the caption
/// keeps to its top, so both sides' names and details line up whether or not
/// a side offers the control. The `controls` share one form and one place,
/// on the names' line where the narrower room beside a name holds them
/// (`inline`), otherwise on the line beneath, both together, so one side's
/// control never wraps alone; there a side without one keeps an empty line
/// of the control's height in its place.
struct Captions<'a> {
    names: [&'static str; 2],
    details: &'a [String; 2],
    kinds: [Details; 2],
    source: bool,
    controls: &'a [Option<(crate::system_preview::Target, crate::system_preview::Form)>; 2],
    inline: bool,
    /// Whether each side's [`Details::Kept`] line wraps past one line.
    kept_wraps: [bool; 2],
}

impl Captions<'_> {
    /// The row, with each side's `dimensions` beneath its name where they
    /// stay, and otherwise their words in the name's accessible name and its
    /// tooltip. A side without an image draws why beneath its name only
    /// where it is [`Details::Kept`]: whole beside the dimensions, and on one
    /// line ending in an ellipsis where they give way, its words in the
    /// name's accessible name and, where that line is cut, its tooltip.
    /// Where the side's line is `said` in the image's room, which reads it
    /// as static text, the name leaves it out; otherwise the name keeps it,
    /// in its accessible name and tooltip.
    fn build(
        &self,
        dimensions: bool,
        said: [bool; 2],
        p: &crate::appearance::Palette,
        cx: &App,
    ) -> Div {
        let offered = self.controls.iter().any(Option::is_some);
        div()
            .flex()
            .border_b_1()
            .border_color(rgb(p.border))
            .children(
                [0, 1]
                    .into_iter()
                    .filter(|index| !self.source || *index == 1)
                    .map(|i| {
                        let (name, details) = (self.names[i], self.details[i].clone());
                        // Placed as a document's open control is
                        // (`Target::control`): where the comparison lies in
                        // a container that reveals its controls, the
                        // keyboard reveals it there with its ring.
                        let control = self.controls[i]
                            .as_ref()
                            .map(|(target, form)| target.control(*form, cx));
                        // Beneath the names, a side without a control keeps
                        // its line, so both sides' details stay level.
                        let keep_place = (!self.inline && offered && control.is_none())
                            .then(|| div().h(crate::appearance::ui_size(28.)).flex_shrink_0());
                        let kept = self.kinds[i] == Details::Kept;
                        let (drawn, cut) = match self.kinds[i] {
                            Details::Dimensions => (dimensions, false),
                            Details::InItsPlace => (false, false),
                            Details::Kept => (true, !dimensions && self.kept_wraps[i]),
                        };
                        let hidden = ((!drawn && !said[i]) || cut).then(|| details.clone());
                        let read = if said[i] {
                            name.to_owned()
                        } else {
                            format!("{name}: {details}")
                        };
                        div()
                            .debug_selector(move || format!("image-side-caption-{i}"))
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .px_3()
                            .py_2()
                            .text_size(crate::appearance::ui_text(11.))
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .map(|line| {
                                        if self.inline {
                                            line.flex_wrap().items_center().content_start()
                                        } else {
                                            line.flex_col().items_start()
                                        }
                                    })
                                    .child(
                                        native_accessibility::static_text(
                                            div().id(("image-side-description", i)).debug_selector(
                                                move || format!("image-side-name-{i}"),
                                            ),
                                            read,
                                        )
                                        .min_h(crate::appearance::ui_size(28.))
                                        .flex()
                                        .items_center()
                                        .font_weight(FontWeight::MEDIUM)
                                        .when_some(hidden, |name, details| {
                                            name.tooltip(move |window, cx| {
                                                gpui_kit::component::tooltip::Tooltip::new(
                                                    details.clone(),
                                                )
                                                .build(window, cx)
                                            })
                                        })
                                        .child(name),
                                    )
                                    .children(control)
                                    .children(keep_place),
                            )
                            .when(drawn, |caption| {
                                caption.child(
                                    div()
                                        .debug_selector(move || format!("image-side-details-{i}"))
                                        .min_w_0()
                                        .text_color(rgb(p.muted))
                                        .when(kept && !dimensions, |line| line.truncate())
                                        .child(if kept && !dimensions {
                                            one_line(&details)
                                        } else {
                                            details
                                        }),
                                )
                            })
                    }),
            )
    }
}

/// What the line beneath a side's name says, and where it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Details {
    /// The image's dimensions: a label, which gives way before the image.
    Dimensions,
    /// Why the side has no image, said where its image would be (its half
    /// side by side, or a source image's room), not beneath its name.
    InItsPlace,
    /// Why the side has no image, beneath its name whatever the room, where
    /// the image has no room of its own for the side (Overlay, Wipe): whole
    /// beside the dimensions, and on one line where they give way.
    Kept,
}

/// The room beneath an image comparison's toolbar in the frame being drawn,
/// and what each part there takes, from which [`Beneath::fit`] chooses what
/// gives way.
#[derive(Clone, Copy)]
struct Beneath {
    /// The room beneath the toolbar, which the captions, the band, the image,
    /// its notes and Compare's footer share.
    room: Pixels,
    /// The captions' height, their rule included, with each side's name
    /// alone, but for a reason kept beneath it on one line.
    names: Pixels,
    /// The captions' height with each side's dimensions beneath its name,
    /// and a reason kept there whole.
    dimensions: Pixels,
    /// Compare's footer's height, where the comparison draws one.
    footer: Option<Pixels>,
    /// Both notes, on one line each.
    notes: Pixels,
    /// The band saying why a click opened nothing, and the most lines either
    /// side's line wraps to, where a click's answer is shown.
    band: Option<(BandLines, usize)>,
    /// The image's usable minimum.
    stage: Pixels,
}

/// What [`Beneath::fit`] chose for one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Fit {
    /// Whether each side's dimensions stay beneath its name.
    dimensions: bool,
    /// Whether Compare's footer is drawn.
    footer: bool,
    /// The room beneath the captions, above the footer where it is drawn,
    /// which the band, the image and its notes share.
    below: Pixels,
    band_lines: usize,
    band_padded: bool,
    band_height: Pixels,
    /// Whether the image has at least a pixel beneath the band.
    image_shown: bool,
}

impl Beneath {
    /// Chooses what gives way, labels before content, in one order. A label
    /// whose words stay where it stood, in a tooltip, goes first: the notes'
    /// second lines (`note_lines`, from the room this leaves), then each
    /// side's dimensions, whose words stay in the name's tooltip and
    /// accessible name. Compare's footer, whose whole line the image's
    /// accessible description keeps, goes next, before the notes leave the
    /// image; its room goes to the image or a click's band, or to the
    /// captions where they alone do not fit above it, and its words to the
    /// drawn node among them ([`GitTurtle::render_image_comparison`]). The
    /// band saying why a click opened nothing shows every line where the
    /// image keeps its minimum beneath it and otherwise two; the dimensions
    /// do not count it, so a click's answer never moves the captions.
    fn fit(self) -> Fit {
        let band = |below: Pixels| {
            self.band.map_or((0, false, px(0.)), |(band, most)| {
                let (lines, padded) = band.fit(most, Some(below), self.stage);
                (lines, padded, band.height(lines, padded))
            })
        };
        let footer = self.footer.unwrap_or(px(0.));
        // The dimensions stay where the image keeps its minimum beneath the
        // captions with the footer drawn and the notes on a line each, and
        // where giving way would free no room, as where a side's reason
        // beneath its name is as tall as they are.
        let dimensions = self.dimensions <= self.names
            || self.room - footer - self.dimensions - self.notes >= self.stage;
        let captions = if dimensions {
            self.dimensions
        } else {
            self.names
        };
        let with_footer = self.room - footer - captions;
        let without = self.room - captions;
        // The footer gives way where the image, beneath the captions, any
        // band at its two-line cap and the notes on a line each, would keep
        // less than its minimum with the footer drawn. Its room goes to what
        // is drawn beneath the captions, the image or a click's band, whose
        // node keeps its words, and where the captions alone do not fit
        // above it, to the captions, which name the sides and hold their
        // open controls and then keep its words. It once stayed there, where
        // giving way drew no image, and ran past the preview's foot beneath
        // captions cut by the clip. A band that shows more than two lines
        // does so only above an image that keeps its minimum, so it never
        // pushes the footer out.
        let footer = self.footer.is_some() && {
            let capped = self
                .band
                .map_or(px(0.), |(band, most)| band.height(most.min(2), true));
            with_footer - capped - self.notes >= self.stage
        };
        let below = if footer { with_footer } else { without };
        let (band_lines, band_padded, band_height) = band(below);
        Fit {
            dimensions,
            footer,
            below,
            band_lines,
            band_padded,
            band_height,
            image_shown: below - band_height >= px(1.),
        }
    }
}

/// The lines of the band beneath an image comparison's captions, each `line`
/// tall, inside `padding` above and below and over its 1 px rule.
#[derive(Clone, Copy)]
struct BandLines {
    line: Pixels,
    padding: Pixels,
}

impl BandLines {
    /// The band's height on `lines` lines, `padded` or not, its rule included.
    fn height(self, lines: usize, padded: bool) -> Pixels {
        (if padded { self.padding } else { px(0.) }) + self.line * lines as f32 + px(1.)
    }

    /// How many lines the band takes for a line that wraps to `most` in the
    /// room `below` the captions, and whether it keeps its padding: every
    /// line where the image keeps `stage` beneath it, and otherwise two, so
    /// the image stays in view. Where the room holds less than that, the band
    /// gives up its padding, then whole lines, never ending inside one;
    /// before the room is laid out, every line.
    fn fit(self, most: usize, below: Option<Pixels>, stage: Pixels) -> (usize, bool) {
        let Some(below) = below else {
            return (most, true);
        };
        let mut lines = if below - self.height(most, true) >= stage {
            most
        } else {
            most.min(2)
        };
        let padded = self.height(lines, true) <= below;
        while lines > 1 && self.height(lines, padded) > below {
            lines -= 1;
        }
        (lines, padded)
    }
}

/// How many lines each of the comparison's notes takes in `room`, the height
/// the captions and any refusal's band leave the image and its notes: up to
/// `most` for a note that `wraps` (needs more than one line), the most that
/// leaves the image `stage` beneath both, and none, the notes leaving the
/// image their room, where even one line each would leave it less. Each line
/// is `line` tall and each note adds `pad`.
fn note_lines(
    room: Pixels,
    wraps: [bool; 2],
    most: usize,
    line: Pixels,
    pad: Pixels,
    stage: Pixels,
) -> usize {
    (1..=most)
        .rev()
        .find(|&lines| room - notes_height(wraps, lines, line, pad) >= stage)
        .unwrap_or(0)
}

/// The height both notes take on up to `lines` lines each, a note that
/// `wraps` taking them all and another one, each line `line` tall and each
/// note adding `pad`; none where they take no lines.
fn notes_height(wraps: [bool; 2], lines: usize, line: Pixels, pad: Pixels) -> Pixels {
    if lines == 0 {
        return px(0.);
    }
    wraps
        .iter()
        .map(|&wraps| line * if wraps { lines as f32 } else { 1. } + pad)
        .sum()
}

/// `text` laid out on one line at `size`, again only when it, the font or the
/// sizes change.
fn one_line_width(
    text: &str,
    cache: &'static std::thread::LocalKey<crate::views::TextWidth>,
    size: Pixels,
    window: &Window,
) -> Pixels {
    crate::views::cached_text_width(cache, text, window, || {
        crate::views::line_widths(&[(text, size)], window)
    })
}

/// A muted note of the comparison, `id`, in a column `width` wide, on up to
/// `lines` lines: whole where it fits them, wrapping inside its room, and
/// otherwise ending in an ellipsis at a character boundary on its last line,
/// its whole text in its tooltip as in its name. None where it takes no
/// lines, having left the image its room.
fn comparison_note(
    id: &'static str,
    text: String,
    lines: usize,
    cache: &'static std::thread::LocalKey<crate::views::TextWidth>,
    width: Pixels,
    window: &Window,
    p: &crate::appearance::Palette,
) -> Option<native_accessibility::StaticText<Stateful<Div>>> {
    if lines == 0 {
        return None;
    }
    let size = crate::appearance::ui_text(10.);
    let room = width - window.rem_size() * 1.5;
    let whole = one_line_width(&text, cache, size, window) <= room
        || (lines > 1 && wrapped_lines(&text, size, room, window) <= lines);
    let tooltip = text.clone();
    Some(
        native_accessibility::static_text(
            div().id(id).debug_selector(move || id.into()),
            text.clone(),
        )
        .flex_shrink_0()
        .px_3()
        .py_1()
        .text_size(size)
        .text_color(rgb(p.muted))
        .when(!whole, |note| {
            note.tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
            })
        })
        .child(
            div()
                .min_w_0()
                .map(|line| {
                    if lines == 1 {
                        line.truncate()
                    } else {
                        line.line_clamp(lines).text_ellipsis()
                    }
                })
                .child(text),
        ),
    )
}

/// One line of text at `size` in the inherited interface font, as its text
/// element lays it out: the style's line height snapped to a device pixel.
pub(crate) fn text_line(size: Pixels, window: &Window) -> Pixels {
    window.pixel_snap(
        window
            .text_style()
            .line_height
            .to_pixels(size.into(), window.rem_size()),
    )
}

/// The lines `text` wraps to at `size` in `room`, as its text element lays
/// it out in the inherited interface font.
pub(crate) fn wrapped_lines(text: &str, size: Pixels, room: Pixels, window: &Window) -> usize {
    let style = window.text_style();
    window
        .text_system()
        .shape_text(
            text.to_owned().into(),
            size,
            &[style.to_run(text.len())],
            Some(room),
            None,
        )
        .map_or(usize::MAX, |lines| {
            lines
                .iter()
                .map(|line| line.wrap_boundaries().len() + 1)
                .sum()
        })
}

#[cfg(test)]
mod tests {
    use super::{BandLines, Beneath, Drag, State, layout, note_lines, source_geometry};
    use gpui_kit::{point, px};

    /// As the room beneath the toolbar shrinks, labels give way in one order
    /// before the image gives up its minimum: the dimensions (their words in
    /// the name's tooltip), then Compare's footer (its line in the image's
    /// description), then the notes. Captions 40 px tall, 70 with their
    /// dimensions, a 24 px footer, notes 22 px each on one line, a 96 px
    /// minimum. Where the captions alone do not fit above the footer, it
    /// gives way to them.
    #[test]
    fn labels_beneath_an_image_give_way_in_one_order() {
        let beneath = |room: f32, band| Beneath {
            room: px(room),
            names: px(40.),
            dimensions: px(70.),
            footer: Some(px(24.)),
            notes: px(44.),
            band,
            stage: px(96.),
        };
        // Whether the notes, which fit one line, are drawn.
        let notes = |room: f32| {
            let fit = beneath(room, None).fit();
            note_lines(
                fit.below - fit.band_height,
                [false; 2],
                2,
                px(14.),
                px(8.),
                px(96.),
            ) > 0
        };
        // Everything fits: 70 + 24 + 44 + 96 = 234.
        let fit = beneath(234., None).fit();
        assert!(fit.dimensions && fit.footer && notes(234.));
        // The dimensions go first, where the notes would leave with them.
        let fit = beneath(233., None).fit();
        assert!(!fit.dimensions && fit.footer && notes(233.));
        // Then the footer, while the notes stay: 40 + 24 + 44 + 96 = 204.
        assert!(beneath(204., None).fit().footer);
        let fit = beneath(203., None).fit();
        assert!(!fit.footer && fit.below == px(163.) && notes(203.));
        // Then the notes: 40 + 44 + 96 = 180.
        assert!(notes(180.) && !notes(179.));
        // Once gone, a label stays gone as the room shrinks, down to the
        // room where even without the footer no pixel of the image is left
        // (41 px beneath 40 px of captions).
        let mut kept = (true, true, true);
        for half in (82..=600).rev() {
            let room = half as f32 / 2.;
            let fit = beneath(room, None).fit();
            let now = (fit.dimensions, fit.footer, notes(room));
            assert!(
                (now.0 <= kept.0) && (now.1 <= kept.1) && (now.2 <= kept.2),
                "{room}: {now:?} after {kept:?}"
            );
            assert!(fit.image_shown, "{room}: the image is drawn");
            kept = now;
        }
        // Where even without the footer no pixel of the image is left and no
        // band is shown, the captions alone do not fit above the footer,
        // which gives way to them; it once stayed there and ran past the
        // room's foot.
        for room in [40.5, 30., 20.] {
            let fit = beneath(room, None).fit();
            assert!(!fit.image_shown && !fit.footer, "{room}: {fit:?}");
        }
        // A band at two 14 px lines and its 8 px padding and rule, 37 px:
        // the footer gives its room to a click's answer even where that
        // leaves the image none, and to the image where it leaves a pixel.
        let band = Some((
            BandLines {
                line: px(14.),
                padding: px(8.),
            },
            2,
        ));
        let fit = beneath(77., band).fit();
        assert!(
            !fit.image_shown && !fit.footer && fit.band_lines == 2,
            "{fit:?}"
        );
        let fit = beneath(78., band).fit();
        assert!(fit.image_shown && !fit.footer, "{fit:?}");
        // A band whose line wraps to four lines counts at its two-line cap,
        // 37 px: beneath 70 px of captions and the 24 px footer, 190 px
        // keep the image its minimum with the capped band and the notes
        // (190 - 37 - 44 = 109), so the footer stays, and the band shows all
        // four lines, 65 px, above an image that keeps its minimum
        // (190 - 65 = 125). Counted whole, the band pushed the footer out
        // (190 - 65 - 44 = 81).
        let four = Some((
            BandLines {
                line: px(14.),
                padding: px(8.),
            },
            4,
        ));
        let fit = beneath(284., four).fit();
        assert!(
            fit.dimensions && fit.footer && fit.band_lines == 4 && fit.image_shown,
            "{fit:?}"
        );
        // Where even the capped band leaves the image less, it goes.
        let fit = beneath(284. - 14., four).fit();
        assert!(!fit.footer, "{fit:?}");
    }
    /// The pan never takes a toolbar row of its own, and each form changes at
    /// one width, so a wider toolbar is never less labelled: the zoom menu's
    /// own items, then a Pan menu, then labelled zooms, then the arrows
    /// beside them, each only where every group fits one row with it, the
    /// overlay's or the wipe's adjustment among them. The zooms' form is the
    /// same at Fit. Widths as at 13 pt in the test fonts: the modes 233, the
    /// zooms 191 labelled or 58.5 as their menu, the arrows 130, the Pan menu
    /// 51, the adjustment 230.
    #[test]
    fn the_pan_folds_before_it_takes_a_row() {
        use super::{Pan, ToolbarForms, ToolbarWidths};
        let widths = |room: f32, adjust: Option<f32>| ToolbarWidths {
            room: px(room),
            gap: px(6.5),
            modes: Some(px(233.)),
            zooms: [px(191.), px(58.5)],
            pan: Some([px(130.), px(51.)]),
            adjust: adjust.map(px),
        };
        let forms = |zooms_labelled, pan| ToolbarForms {
            zooms_labelled,
            pan: Some(pan),
        };
        // Side by side: the Pan menu from 355.5 px, the labelled zooms from
        // 488 and the arrows from 567.
        for (adjust, by) in [(None, 0.), (Some(230.), 236.5)] {
            let at = |room: f32| widths(room + by, adjust).forms(true);
            assert_eq!(at(355.), forms(false, Pan::InZoomMenu), "{adjust:?}");
            assert_eq!(at(355.5), forms(false, Pan::Menu), "{adjust:?}");
            assert_eq!(at(487.5), forms(false, Pan::Menu), "{adjust:?}");
            assert_eq!(at(488.), forms(true, Pan::Menu), "{adjust:?}");
            assert_eq!(at(567.), forms(true, Pan::Arrows), "{adjust:?}");
            assert_eq!(widths(1300., adjust).forms(false).pan, None);
        }
        let rank = |forms: ToolbarForms| {
            (
                forms.zooms_labelled,
                match forms.pan {
                    Some(Pan::Arrows) => 2,
                    Some(Pan::Menu) => 1,
                    _ => 0,
                },
            )
        };
        for adjust in [None, Some(230.)] {
            let mut last = (false, 0);
            for room in 150..1300 {
                let widths = widths(room as f32, adjust);
                let (fit, panned) = (widths.forms(false), widths.forms(true));
                assert_eq!(fit.zooms_labelled, panned.zooms_labelled, "{room}");
                let now = rank(panned);
                assert!(
                    now.0 >= last.0 && now.1 >= last.1,
                    "{room}: {panned:?}, less labelled than {last:?} a pixel narrower"
                );
                last = now;
                let zooms = px(if panned.zooms_labelled { 191. } else { 58.5 });
                let group = match panned.pan {
                    Some(Pan::Arrows) => zooms + px(6.5 + 130.),
                    Some(Pan::Menu) => zooms + px(6.5 + 51.),
                    _ => zooms,
                };
                assert!(
                    group == zooms || widths.fits(group),
                    "{room}: the pan {panned:?} lies beside the zooms only on one row"
                );
            }
        }
        // A source image does not pan: its zooms are labelled where they fit.
        let source = ToolbarWidths {
            modes: None,
            pan: None,
            ..widths(200., None)
        };
        assert_eq!(
            source.forms(true),
            ToolbarForms {
                zooms_labelled: true,
                pan: None
            }
        );
    }
    /// The images can move back towards their start where they have left it,
    /// and further where they have room: a pan control that cannot move them
    /// is unavailable.
    #[test]
    fn pan_room_follows_the_pan_limits() {
        assert_eq!(
            super::pan_room([0., 0.], [200., 0.]),
            [false, true, false, false]
        );
        assert_eq!(
            super::pan_room([-80., 0.], [200., 300.]),
            [true, true, false, true]
        );
        assert_eq!(
            super::pan_room([-200., -300.], [200., 300.]),
            [true, false, true, false]
        );
    }
    /// The dimensions give way only where that frees room: overlaid, a side
    /// without an image keeps why beneath its name, as tall as the other
    /// side's dimensions, and hiding them left the captions as tall and the
    /// image no more room.
    #[test]
    fn dimensions_stay_where_giving_way_frees_nothing() {
        let beneath = |names: f32| Beneath {
            room: px(150.),
            names: px(names),
            dimensions: px(70.),
            footer: Some(px(24.)),
            notes: px(44.),
            band: None,
            stage: px(96.),
        };
        assert!(beneath(70.).fit().dimensions);
        assert!(!beneath(69.).fit().dimensions);
    }
    /// A side whose file is absent says it has no image and, on a second
    /// line, why: Before's file was added, After's deleted. A side whose
    /// image could not be shown says only why.
    #[test]
    fn an_absent_side_says_why_on_a_second_line() {
        let absent = crate::worker::ImageSide {
            animation: None,
            captured: None,
            system_preview: None,
            literal_source: None,
            image: None,
            render: None,
            message: None,
            bytes: 0,
            lfs_pointer: None,
        };
        assert_eq!(
            [0, 1].map(|index| super::side_details(index, &absent)),
            [
                "No image on this side\nThe file was added",
                "No image on this side\nThe file was deleted",
            ]
        );
        assert_eq!(
            super::one_line(&super::side_details(1, &absent)),
            "No image on this side · The file was deleted"
        );
        // Where both lines cannot be drawn whole, one short line carries both.
        assert_eq!(
            [0, 1].map(|index| super::absent_short(index, &absent)),
            [Some("No image: file added"), Some("No image: file deleted")]
        );
        let unreadable = crate::worker::ImageSide {
            message: Some("Could not read the image".into()),
            ..absent
        };
        assert_eq!(
            super::side_details(0, &unreadable),
            "Could not read the image"
        );
    }
    #[test]
    fn thumbnail_reduction_preserves_original_resize_and_absent_sides() {
        let g = source_geometry([Some([3200, 1600, 1600, 800]), Some([1600, 800, 1600, 800])]);
        assert_eq!(g.sizes, [[1600., 800.], [800., 400.]]);
        assert_eq!(g.source_scale, 0.5);
        let absent = source_geometry([None, Some([720, 440, 720, 440])]);
        assert_eq!(absent.sizes[0], [0., 0.]);
        assert_eq!(absent.extent, [720., 440.]);
    }
    /// The notes take two lines where the image keeps its minimum beneath
    /// them, one where only that does, and leave where even one would cost
    /// it; a note that fits one line never takes two.
    #[test]
    fn notes_give_their_lines_to_the_image_first() {
        let lines = |room: f32, wraps| note_lines(px(room), wraps, 2, px(14.), px(8.), px(96.));
        // Two lines each: 2 × (28 + 8) = 72 beneath a 96 px image.
        assert_eq!(lines(168., [true, true]), 2);
        assert_eq!(lines(167., [true, true]), 1);
        // One line each: 2 × (14 + 8) = 44.
        assert_eq!(lines(140., [true, true]), 1);
        assert_eq!(lines(139., [true, true]), 0);
        // Only the guidance wraps: 22 + 36 = 58 for two lines.
        assert_eq!(lines(154., [false, true]), 2);
        assert_eq!(lines(153., [false, true]), 1);
        assert_eq!(
            note_lines(px(500.), [true; 2], 1, px(14.), px(8.), px(96.)),
            1
        );
    }
    #[test]
    fn retained_image_state_never_revives_an_active_gesture() {
        let state = State {
            drag: Some(Drag::Pan(point(px(10.), px(10.)), [0., 0.])),
            pan: [-10., -20.],
            ..State::default()
        };
        let retained = state.clone();
        assert!(retained.drag.is_none());
        assert_eq!(retained.pan, state.pan);
    }
    #[test]
    fn shared_image_geometry_fits_without_upscaling_and_clamps_pan() {
        assert_eq!(layout([1000., 800.], [720., 440.], 0., [0., 0.]).0, 1.);
        let (scale, origin, maximum) = layout([500., 300.], [720., 440.], 2., [-9000., 90.]);
        assert_eq!(scale, 2.);
        assert_eq!(maximum, [940., 580.]);
        assert_eq!(origin, [-940., 0.]);
        let (scale, _, _) = layout([300., 200.], [1600., 800.], 0., [0., 0.]);
        assert!((scale - 268. / 1600.).abs() < 0.001);
    }
}
