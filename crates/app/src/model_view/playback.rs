//! Shared seconds, independent clip choices, and demand-driven 30 Hz sampling.
use super::*;
use std::time::Instant;

const FPS: f64 = 30.;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Pose {
    pub clip: Option<usize>,
    pub seconds: f64,
}

#[derive(Debug, Default)]
pub(super) struct Playback {
    pub pose: Pose,
    offset: f64,
    started: Option<Instant>,
    duration: f64,
    clip_duration: f64,
    sampled_seconds: f64,
}
impl Playback {
    pub fn playing(&self) -> bool {
        self.started.is_some()
    }
    fn position(&self, now: Instant) -> f64 {
        match self.started {
            Some(start) if self.duration > 0. => {
                (self.offset + now.saturating_duration_since(start).as_secs_f64()) % self.duration
            }
            _ => self.offset,
        }
    }
    pub fn tick(&mut self, now: Instant) {
        if self.playing() {
            // Never cancel an in-flight frame to chase the clock. The caller
            // samples only after the prior request completes and skips time.
            let seconds = (self.position(now) * FPS).floor() / FPS;
            self.sampled_seconds = seconds;
            self.pose.seconds = seconds.min(self.clip_duration);
        }
    }
    pub fn pause(&mut self) {
        if self.playing() {
            // Hold the visible/requested sample on lifecycle pauses.
            self.offset = self.sampled_seconds;
        }
        self.started = None;
    }
    fn seek(&mut self, seconds: f64, duration: f64, clip_duration: f64) {
        self.started = None;
        self.duration = duration;
        self.clip_duration = clip_duration;
        self.offset = if seconds.is_finite() {
            seconds.clamp(0., duration)
        } else {
            0.
        };
        self.pose.seconds = self.offset.min(clip_duration);
        self.sampled_seconds = self.offset;
    }
}

fn duration(documents: &[Arc<Document>]) -> f64 {
    documents
        .iter()
        .map(|document| {
            let state = document.state.lock().unwrap_or_else(|e| e.into_inner());
            state
                .playback
                .pose
                .clip
                .and_then(|clip| document.scene.animation_clips().get(clip))
                .map_or(0., |clip| clip.duration_seconds)
        })
        .fold(0., f64::max)
}
fn position(documents: &[Arc<Document>]) -> f64 {
    // Every side has the same offset and start instant. Static and shorter sides
    // still retain the comparison clock, independently of their clamped pose.
    documents.first().map_or(0., |document| {
        document
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .playback
            .position(Instant::now())
    })
}
fn seek(documents: &[Arc<Document>], seconds: f64) {
    let duration = duration(documents);
    for document in documents {
        let mut state = document.state.lock().unwrap_or_else(|e| e.into_inner());
        let clip_duration = state
            .playback
            .pose
            .clip
            .and_then(|clip| document.scene.animation_clips().get(clip))
            .map_or(0., |clip| clip.duration_seconds);
        state.playback.seek(seconds, duration, clip_duration);
        document.invalidate(&mut state);
    }
}
/// Freeze one comparison clock, even if one side has already reached its final
/// pose while its partner is still preparing a more demanding frame.
pub(super) fn pause_documents(documents: &[Arc<Document>]) {
    let seconds = position(documents);
    seek(documents, seconds);
    for document in documents {
        document.pause();
    }
}
pub(super) fn stop_on_error(documents: &[Arc<Document>]) {
    let errors: Vec<_> = documents
        .iter()
        .map(|document| {
            document
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .error
                .clone()
        })
        .collect();
    pause_documents(documents);
    for (document, error) in documents.iter().zip(errors) {
        document
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .error = error;
    }
}
fn toggle(documents: &[Arc<Document>]) {
    let playing = documents.iter().any(|document| {
        document
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .playback
            .playing()
    });
    let seconds = position(documents);
    if !playing && duration(documents) == 0. {
        for document in documents {
            let mut state = document.state.lock().unwrap_or_else(|e| e.into_inner());
            state.playback.pose.clip = document
                .scene
                .animation_clips()
                .iter()
                .position(|clip| clip.duration_seconds > 0.);
        }
    }
    seek(documents, seconds);
    let now = Instant::now();
    for document in documents {
        let mut state = document.state.lock().unwrap_or_else(|e| e.into_inner());
        if !playing && state.playback.duration > 0. {
            state.playback.started = Some(now);
        }
    }
}
fn choose_clip(document: &Arc<Document>, partner: Option<&Arc<Document>>, clip: Option<usize>) {
    {
        let mut state = document.state.lock().unwrap_or_else(|e| e.into_inner());
        state.playback.pose.clip =
            clip.filter(|&index| index < document.scene.animation_clips().len());
    }
    let mut documents = vec![document.clone()];
    documents.extend(partner.cloned());
    seek(&documents, 0.);
}

pub(super) fn clip_control<T: 'static>(
    index: usize,
    label: &'static str,
    document: &Arc<Document>,
    partner: Option<Arc<Document>>,
    cx: &mut Context<T>,
) -> Option<AnyElement> {
    let clips = document.scene.animation_clips();
    if clips.is_empty() {
        return None;
    }
    let (selected, sampled) = {
        let state = document.state.lock().unwrap_or_else(|e| e.into_inner());
        (
            state.playback.pose.clip,
            state.frame_pose.unwrap_or_default(),
        )
    };
    let text = selected
        .and_then(|index| clips.get(index))
        .map_or_else(|| "Default pose".to_owned(), |clip| clip.name.clone());
    let label_text = format!("{label}: Animation clip, {text}");
    let target = document.clone();
    let owner = cx.entity().downgrade();
    let selected_duration = selected
        .and_then(|index| clips.get(index))
        .map(|clip| clip.duration_seconds);
    Some(
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(
                button(("model-clip", index), text, "", false)
                    .dropdown_caret(true)
                    .accessibility_label(label_text)
                    .dropdown_menu(move |mut menu, _, _| {
                        menu = menu.label(format!("{label} animation"));
                        for (clip, name) in std::iter::once((None, "Default pose".to_owned()))
                            .chain(target.scene.animation_clips().iter().enumerate().map(
                                |(index, clip)| {
                                    (
                                        Some(index),
                                        format!("{} · {:.2} s", clip.name, clip.duration_seconds),
                                    )
                                },
                            ))
                        {
                            let target = target.clone();
                            let partner = partner.clone();
                            let owner = owner.clone();
                            menu = menu.item(
                                PopupMenuItem::new(name).checked(selected == clip).on_click(
                                    move |_, window, cx| {
                                        choose_clip(&target, partner.as_ref(), clip);
                                        window.refresh();
                                        let _ = owner.update(cx, |_, cx| cx.notify());
                                    },
                                ),
                            );
                        }
                        menu
                    }),
            )
            .children(selected_duration.map(|duration| {
                div()
                    .id(("model-pose-time", index))
                    .role(Role::Label)
                    .aria_label(format!(
                        "{label} displayed pose: {:.2} of {:.2} seconds",
                        sampled.seconds, duration
                    ))
                    .text_size(appearance::ui_text(10.))
                    .text_color(rgb(palette(cx).muted))
                    .child(format!("Pose {:.2} / {:.2} s", sampled.seconds, duration))
            }))
            .into_any_element(),
    )
}

pub(super) fn controls<T: 'static>(
    documents: &[Arc<Document>],
    cx: &mut Context<T>,
) -> Option<AnyElement> {
    if !documents
        .iter()
        .any(|document| !document.scene.animation_clips().is_empty())
    {
        return None;
    }
    let colors = palette(cx);
    let total = duration(documents);
    let seconds = position(documents);
    let playing = documents.iter().any(|document| {
        document
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .playback
            .playing()
    });
    let play = documents.to_vec();
    let playable = documents.iter().any(|document| {
        document
            .scene
            .animation_clips()
            .iter()
            .any(|clip| clip.duration_seconds > 0.)
    });
    let first = documents.to_vec();
    let back = documents.to_vec();
    let next = documents.to_vec();
    let mut row = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .child(
            button(
                "model-play",
                if playing { "Pause" } else { "Play" },
                "",
                playing,
            )
            .toggled(playing)
            .disabled(cx.reduce_motion() || !playable)
            .accessibility_label(if playing {
                "Pause model animations"
            } else {
                "Play model animations"
            })
            .on_click(cx.listener(move |_, _, window, cx| {
                if !cx.reduce_motion() {
                    toggle(&play);
                    window.refresh();
                    cx.notify();
                }
            })),
        )
        .child(
            button("model-first", "Start", "", false)
                .accessibility_label("Seek model animations to start")
                .on_click(cx.listener(move |_, _, window, cx| {
                    seek(&first, 0.);
                    window.refresh();
                    cx.notify();
                })),
        )
        .child(
            button("model-previous-pose", "−1/30 s", "", false)
                .disabled(total == 0.)
                .accessibility_label("Previous model animation sample")
                .on_click(cx.listener(move |_, _, window, cx| {
                    seek(&back, position(&back) - 1. / FPS);
                    window.refresh();
                    cx.notify();
                })),
        )
        .child(
            button("model-next-pose", "+1/30 s", "", false)
                .disabled(total == 0.)
                .accessibility_label("Next model animation sample")
                .on_click(cx.listener(move |_, _, window, cx| {
                    seek(&next, position(&next) + 1. / FPS);
                    window.refresh();
                    cx.notify();
                })),
        )
        .child(
            div()
                .id("model-comparison-time")
                .role(Role::Label)
                .aria_label(format!(
                    "Comparison time {:.2} of {:.2} seconds",
                    seconds, total
                ))
                .text_size(appearance::ui_text(11.))
                .child(format!("{seconds:.2} / {total:.2} s")),
        );
    if total > 0. {
        row = row.child(scrubber(documents, seconds, total, cx));
    }
    Some(div().flex().flex_col().gap_1().px_3().py_1().border_b_1().border_color(rgb(colors.border)).child(row)
        .child(div().text_size(appearance::ui_text(10.)).text_color(rgb(colors.muted)).child(if cx.reduce_motion() { "Reduce Motion is enabled. Choose a clip and scrub or step through poses." } else { "Shared seconds · shorter clips hold their last pose · longest clip loops. Clip changes and scrubbing pause both sides." }))
        .into_any_element())
}

fn scrub_at(documents: &[Arc<Document>], point: Point<Pixels>) {
    let Some(first) = documents.first() else {
        return;
    };
    let bounds = first
        .state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .timeline_bounds;
    let fraction =
        (f32::from(point.x - bounds.left()) / f32::from(bounds.size.width).max(1.)).clamp(0., 1.);
    seek(documents, f64::from(fraction) * duration(documents));
}
fn scrubber<T: 'static>(
    documents: &[Arc<Document>],
    seconds: f64,
    total: f64,
    cx: &mut Context<T>,
) -> AnyElement {
    let colors = palette(cx);
    let first = documents[0].clone();
    let focus = first
        .state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .timeline_focus
        .get_or_insert_with(|| cx.focus_handle())
        .clone();
    let focus_down = focus.clone();
    let down = documents.to_vec();
    let moved = documents.to_vec();
    let up = documents.to_vec();
    let outside = documents.to_vec();
    let keyed = documents.to_vec();
    let increment = documents.to_vec();
    let decrement = documents.to_vec();
    let fraction = (seconds / total).clamp(0., 1.) as f32;
    div().id("model-timeline").min_w(px(120.)).w(px(210.)).h(px(22.)).relative().cursor(CursorStyle::PointingHand)
        .track_focus(&focus).tab_stop(true).role(Role::Slider).aria_label("Model animation time in seconds").aria_description("Left and right seek one thirtieth second; Home and End seek to the beginning and end. Seeking pauses playback.")
        .aria_numeric_value(seconds).aria_min_numeric_value(0.).aria_max_numeric_value(total).aria_numeric_value_step(1. / FPS)
        .focus_visible(|style| style.border_1().border_color(rgb(colors.accent)))
        .on_a11y_action(gpui::AccessibleAction::Increment, move |_, window, cx| { seek(&increment, position(&increment) + 1. / FPS); window.refresh(); cx.refresh_windows(); })
        .on_a11y_action(gpui::AccessibleAction::Decrement, move |_, window, cx| { seek(&decrement, position(&decrement) - 1. / FPS); window.refresh(); cx.refresh_windows(); })
        .on_mouse_down(MouseButton::Left, cx.listener(move |_, event: &MouseDownEvent, window, cx| { focus_down.focus(window, cx); for document in &down { document.state.lock().unwrap_or_else(|e| e.into_inner()).scrubbing = true; } scrub_at(&down, event.position); window.refresh(); cx.stop_propagation(); cx.notify(); }))
        .on_mouse_move(cx.listener(move |_, event: &MouseMoveEvent, window, cx| { if event.pressed_button == Some(MouseButton::Left) && moved[0].state.lock().unwrap_or_else(|e| e.into_inner()).scrubbing { scrub_at(&moved, event.position); window.refresh(); cx.stop_propagation(); cx.notify(); } }))
        .on_mouse_up(MouseButton::Left, cx.listener(move |_, _, window, cx| { end_scrub(&up); window.refresh(); cx.notify(); }))
        .on_mouse_up_out(MouseButton::Left, cx.listener(move |_, _, window, cx| { end_scrub(&outside); window.refresh(); cx.notify(); }))
        .on_key_down(cx.listener(move |_, event: &KeyDownEvent, window, cx| { if event.keystroke.modifiers.platform || event.keystroke.modifiers.control || event.keystroke.modifiers.alt { return; } let target = match event.keystroke.key.as_str() { "left" | "down" => position(&keyed) - 1. / FPS, "right" | "up" => position(&keyed) + 1. / FPS, "home" => 0., "end" => duration(&keyed), _ => return }; seek(&keyed, target); window.refresh(); cx.stop_propagation(); cx.notify(); }))
        .on_prepaint(move |bounds, _, _| first.state.lock().unwrap_or_else(|e| e.into_inner()).timeline_bounds = bounds)
        .child(div().absolute().left_0().right_0().top(px(9.)).h(px(4.)).rounded_sm().bg(rgb(colors.border)))
        .child(div().absolute().left_0().top(px(9.)).w(relative(fraction)).h(px(4.)).rounded_sm().bg(rgb(colors.accent)))
        .child(div().absolute().left(relative(fraction)).ml(px(-5.)).top(px(6.)).size(px(10.)).rounded_full().bg(rgb(colors.accent)))
        .into_any_element()
}
fn end_scrub(documents: &[Arc<Document>]) {
    for document in documents {
        let mut state = document.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.scrubbing {
            state.scrubbing = false;
            document.invalidate(&mut state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use std::time::Duration;
    fn animated() -> Arc<Document> {
        Arc::new(Document::new(
            model3d::decode_geometry(
                include_bytes!(
                    "../../../preview/tests/fixtures/models/glb/workflow/morph-animation.glb"
                ),
                "morph.glb",
                || Ok(()),
            )
            .unwrap()
            .scene,
        ))
    }
    #[test]
    fn lifecycle_and_failure_pause_share_one_time_when_partner_frame_is_pending() {
        let pair = [animated(), animated()];
        choose_clip(&pair[0], Some(&pair[1]), Some(2));
        choose_clip(&pair[1], Some(&pair[0]), Some(0));
        toggle(&pair);
        let start = Instant::now() - Duration::from_millis(1500);
        for (index, document) in pair.iter().enumerate() {
            let mut state = document.state.lock().unwrap();
            state.playback.started = Some(start);
            state
                .playback
                .tick(start + Duration::from_millis(if index == 0 { 1500 } else { 1200 }));
            state.pending = index == 1;
        }
        pause_documents(&pair);
        let a = pair[0].state.lock().unwrap();
        let b = pair[1].state.lock().unwrap();
        assert_eq!(a.playback.offset, b.playback.offset);
        assert_eq!(a.playback.pose.seconds, 1.);
        assert_eq!(b.playback.pose.seconds, b.playback.offset);
        assert!(!a.playback.playing() && !b.playback.playing() && !b.pending);
        drop((a, b));
        pair[1].state.lock().unwrap().error = Some("Raster budget exceeded".into());
        stop_on_error(&pair);
        assert_eq!(
            pair[1].state.lock().unwrap().error.as_deref(),
            Some("Raster budget exceeded")
        );
    }
    #[test]
    fn clip_choices_scrubbing_and_hidden_pause_preserve_cameras_and_invalidate_results() {
        let before = animated();
        let after = animated();
        let pair = [before.clone(), after.clone()];
        before.change(Some(&after), Action::Orbit(0.4, 0.2));
        let camera = before.bookmark();
        assert_eq!(before.state.lock().unwrap().playback.pose.clip, None);
        choose_clip(&before, Some(&after), Some(2));
        choose_clip(&after, Some(&before), Some(0));
        assert_eq!(duration(&pair), 2.);
        seek(&pair, 1.5);
        assert_eq!(before.state.lock().unwrap().playback.pose.seconds, 1.);
        assert_eq!(after.state.lock().unwrap().playback.pose.seconds, 1.5);
        let generation = before.generation.load(Ordering::Acquire);
        before.state.lock().unwrap().pending = true;
        toggle(&pair);
        assert!(before.state.lock().unwrap().playback.playing());
        assert!(!before.state.lock().unwrap().pending);
        assert_ne!(before.generation.load(Ordering::Acquire), generation);
        for document in &pair {
            document.pause();
        }
        assert!(!before.state.lock().unwrap().playback.playing());
        assert_eq!(before.bookmark().target, camera.target);
        assert_eq!(before.bookmark().yaw, camera.yaw);
        assert_eq!(before.bookmark().span, camera.span);
        choose_clip(&after, Some(&before), None);
        assert_eq!(after.state.lock().unwrap().playback.pose, Pose::default());
        assert_eq!(before.state.lock().unwrap().playback.pose.clip, Some(2));
        assert_eq!(position(&pair), 0.);
    }
    #[test]
    fn shared_clock_holds_shorter_pose_and_loops_at_longer_duration() {
        let now = Instant::now();
        let mut short = Playback::default();
        let mut long = Playback::default();
        for (clock, duration) in [(&mut short, 1.), (&mut long, 3.)] {
            clock.pose.clip = Some(0);
            clock.seek(0., 3., duration);
            clock.started = Some(now);
        }
        short.tick(now + Duration::from_millis(2200));
        long.tick(now + Duration::from_millis(2200));
        assert_eq!(short.pose.seconds, 1.);
        assert!((long.pose.seconds - 2.2).abs() < 1e-9);
        short.tick(now + Duration::from_millis(3250));
        long.tick(now + Duration::from_millis(3250));
        assert_eq!(short.pose.seconds, long.pose.seconds);
        assert!((long.pose.seconds - 7. / 30.).abs() < 1e-9);
    }
    #[test]
    fn pause_freezes_sample_and_scrub_can_inspect_exact_final_pose() {
        let mut clock = Playback::default();
        let now = Instant::now();
        clock.seek(0., 3., 3.);
        clock.started = Some(now);
        clock.tick(now + Duration::from_millis(75));
        assert_eq!(clock.pose.seconds, 2. / 30.);
        clock.pause();
        clock.tick(now + Duration::from_secs(100));
        assert_eq!(clock.pose.seconds, 2. / 30.);
        assert!(!clock.playing());
        clock.seek(3., 3., 3.);
        assert_eq!(clock.pose.seconds, 3.);
        clock.seek(f64::NAN, 3., 3.);
        assert_eq!(clock.pose.seconds, 0.);
    }
}
