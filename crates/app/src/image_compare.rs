//! GPU composition over bounded worker-decoded images. No pixels are copied on drag.
use crate::*;
use gpui_kit::prelude::FluentBuilder;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
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
        let playing = self.image_comparison.playback.playing(key);
        let position =
            self.image_comparison
                .playback
                .position(key, duration, std::time::Instant::now());
        let mut labels = Vec::new();
        let mut truncated = false;
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
                }
            }
        }
        Some(div().flex().flex_col().gap_1().px_3().py_2().border_b_1().border_color(rgb(p.border))
            .child(div().flex().flex_wrap().items_center().gap_2()
                .child(button("gif-playback-toggle", if playing { "Pause GIF" } else { "Play GIF" }, "", playing).on_click(cx.listener(|this, _, _, cx| { let (key, duration, _) = this.gif_context(); this.image_comparison.playback.toggle(key, duration, std::time::Instant::now()); cx.notify(); })))
                .child(button("gif-first-frame", "First frame", "", false).on_click(cx.listener(|this, _, _, cx| { let (key, _, _) = this.gif_context(); this.image_comparison.playback.seek(key, 0); cx.notify(); })))
                .child(button("gif-previous-frame", "Previous frame", "", false).on_click(cx.listener(|this, _, _, cx| this.gif_step(false, cx))))
                .child(button("gif-next-frame", "Next frame", "", false).on_click(cx.listener(|this, _, _, cx| this.gif_step(true, cx))))
                .child(div().text_size(appearance::ui_text(11.)).text_color(rgb(p.muted)).child(format!("{:.2} / {:.2} s · {}", position as f64 / 1000., duration as f64 / 1000., labels.join(" · ")))))
            .child(div().text_size(appearance::ui_text(10.)).text_color(rgb(p.muted)).child(if truncated { "Playback is limited to the decoded segment (120 frames, 30 seconds, 16 million output pixels). Open captured bytes in system preview for the complete animation." } else if self.is_quick_source() { "Frame controls pause playback for inspection." } else { "Both versions share one clock; a shorter animation holds its final frame. Frame controls pause playback for inspection." }))
            .into_any_element())
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
    pub(super) fn render_image_comparison(
        &self,
        old: &worker::ImageSide,
        new: &worker::ImageSide,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = palette(cx);
        let source = self.is_quick_source();
        let state = &self.image_comparison;
        let both = self.images.iter().all(Option::is_some);
        let mut modes = div().flex().gap_1();
        for (mode, label) in [
            (Mode::SideBySide, "Side by side"),
            (Mode::Overlay, "Overlay"),
            (Mode::Wipe, "Wipe"),
        ] {
            modes = modes.child(
                button(label, label, "", state.mode == mode)
                    .toggled(state.mode == mode)
                    .tooltip(if mode == Mode::SideBySide {
                        "Compare both versions with linked zoom and pan"
                    } else {
                        "Align both versions at their top-left corner on one canvas"
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.image_comparison.mode = mode;
                        this.image_comparison.pan = [0.; 2];
                        this.end_image_drag();
                        cx.notify();
                    })),
            );
        }
        let mut zooms = div().flex().gap_1();
        for (label, zoom) in [("Fit", 0.), ("50%", 0.5), ("100%", 1.), ("200%", 2.)] {
            zooms = zooms.child(
                button(label, label, "", self.zoom == zoom)
                    .toggled(self.zoom == zoom)
                    .tooltip(format!("100% comparison scale equals {:.1}% of source pixels. Source and decoded dimensions are shown below.", self.image_geometry().source_scale * 100.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.zoom = zoom;
                        this.image_comparison.pan = [0.; 2];
                        cx.notify();
                    })),
            );
        }
        let mut adjust = div().flex().items_center().gap_1();
        if state.mode != Mode::SideBySide {
            let label = if state.mode == Mode::Wipe {
                "Before width"
            } else {
                "After opacity"
            };
            adjust = adjust.child(
                div()
                    .text_size(crate::appearance::ui_text(11.))
                    .text_color(rgb(p.muted))
                    .child(format!("{label} · {:.0}%", state.amount * 100.)),
            );
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
        if self.zoom > 0. {
            for (id, label, delta) in [
                ("pan-left", "←", [80., 0.]),
                ("pan-right", "→", [-80., 0.]),
                ("pan-up", "↑", [0., 80.]),
                ("pan-down", "↓", [0., -80.]),
            ] {
                adjust = adjust.child(
                    button(id, label, "", false)
                        .accessibility_label(id.replace('-', " "))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let pan = this.image_comparison.pan;
                            this.pan_image([pan[0] + delta[0], pan[1] + delta[1]]);
                            cx.notify();
                        })),
                );
            }
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
        let mut captured_actions = div().flex().flex_wrap().gap_2().px_3().py_1();
        for (index, side) in [old, new].into_iter().enumerate() {
            let label = if index == 0 { "Before" } else { "After" };
            if let Some(literal) = side.literal_source.clone() {
                captured_actions = captured_actions.child(button(("copy-image-source",index),format!("Copy {label} source"),"copy",false).tooltip("Copy the literal source bytes as UTF-8; rendered pixels never become a Git patch").on_click(move |_,_,cx|cx.write_to_clipboard(ClipboardItem::new_string(literal.to_string()))));
            }
            if let Some(bytes) = side.captured.clone() {
                let path = self
                    .selected_file
                    .and_then(|selected| self.files.get(selected))
                    .and_then(|file| {
                        if index == 0 {
                            file.old_path.clone()
                        } else {
                            file.new_path.clone()
                        }
                    })
                    .unwrap_or_else(|| PathBuf::from("image.bin"));
                captured_actions = captured_actions.child(button(("image-system-preview",index),format!("{label} system preview"),"external-link",false).tooltip("Open an isolated read-only copy of these captured bytes in system Quick Look").on_click(cx.listener(move |this,_,window,cx|this.open_captured_bytes(bytes.clone(),path.clone(),window,cx))));
            }
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(captured_actions)
            .children(self.gif_controls(cx))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .bg(rgb(p.panel))
                    .border_b_1()
                    .border_color(rgb(p.border))
                    .when(!source, |element| element.child(modes))
                    .child(zooms)
                    .when(!source, |element| element.child(adjust)),
            )
            .child(
                div()
                    .id("image-source-scale")
                    .role(Role::Label)
                    .aria_label(scale_notice.clone())
                    .px_3()
                    .py_1()
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(p.muted))
                    .child(scale_notice),
            )
            .child(
                div()
                    .flex()
                    .border_b_1()
                    .border_color(rgb(p.border))
                    .children(
                        [old, new]
                            .into_iter()
                            .enumerate()
                            .filter(|(index, _)| !source || *index == 1)
                            .map(|(i, side)| {
                                let name = if source {
                                    "Source"
                                } else if i == 0 {
                                    "Before"
                                } else {
                                    "After"
                                };
                                let details = side
                                    .image
                                    .as_ref()
                                    .map(|image| {
                                        format!(
                                            "{} × {} · {}{}",
                                            image.original_width,
                                            image.original_height,
                                            image.format,
                                            if image.width != image.original_width
                                                || image.height != image.original_height
                                            {
                                                format!(
                                                    " · preview {} × {}",
                                                    image.width, image.height
                                                )
                                            } else {
                                                String::new()
                                            }
                                        )
                                    })
                                    .unwrap_or_else(|| {
                                        side.message
                                            .clone()
                                            .unwrap_or_else(|| "No image on this side".into())
                                    });
                                let details = if side.image.is_some() {
                                    side.message.as_ref().map_or(details.clone(), |notice| {
                                        format!("{details} · {notice}")
                                    })
                                } else {
                                    details
                                };
                                div()
                                    .id(("image-side-description", i))
                                    .role(Role::Label)
                                    .aria_label(format!("{name}: {details}"))
                                    .flex_1()
                                    .min_w_0()
                                    .px_3()
                                    .py_2()
                                    .text_size(crate::appearance::ui_text(11.))
                                    .child(div().font_weight(FontWeight::MEDIUM).child(name))
                                    .child(div().text_color(rgb(p.muted)).child(details))
                            }),
                    ),
            )
            .child(if source {
                div()
                    .flex_1()
                    .min_h_0()
                    .child(self.image_canvas(Some(1), cx))
                    .into_any_element()
            } else if state.mode == Mode::SideBySide {
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.image_canvas(Some(0), cx))
                    .child(self.image_canvas(Some(1), cx))
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .min_h_0()
                    .child(self.image_canvas(None, cx))
                    .into_any_element()
            })
            .child(
                div()
                    .id("image-interaction-guidance")
                    .role(Role::Label)
                    .aria_label(guidance)
                    .px_3()
                    .py_1()
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(p.muted))
                    .child(guidance),
            )
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
        let playing = timeline.is_some() && self.image_comparison.playback.playing(key);
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

#[cfg(test)]
mod tests {
    use super::{Drag, State, layout, source_geometry};
    use gpui_kit::{point, px};
    #[test]
    fn thumbnail_reduction_preserves_original_resize_and_absent_sides() {
        let g = source_geometry([Some([3200, 1600, 1600, 800]), Some([1600, 800, 1600, 800])]);
        assert_eq!(g.sizes, [[1600., 800.], [800., 400.]]);
        assert_eq!(g.source_scale, 0.5);
        let absent = source_geometry([None, Some([720, 440, 720, 440])]);
        assert_eq!(absent.sizes[0], [0., 0.]);
        assert_eq!(absent.extent, [720., 440.]);
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
