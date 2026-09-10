//! Native retained-geometry comparison. A single active render and one replaceable
//! pending pair bound CPU preparation; idle/hidden viewers request no frames.
use crate::*;
use futures::channel::oneshot;
use gitturtle_preview::model3d::{self, ModelBounds, ModelCamera, ModelScene, ModelStandardView};
use gpui_kit::base::ElementExt;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::prelude::FluentBuilder;
use std::sync::{
    Condvar, Mutex, OnceLock, Weak,
    atomic::{AtomicU64, Ordering},
};

const FRAME_BYTES: usize = 720 * 720 * 4;
const CANCELLED: &str = "3D render request superseded";

type RenderWork = Box<dyn FnOnce() + Send>;

/// The slot is replaced rather than enqueued. Dropping an obsolete closure also
/// releases its retained scene references and notifies its waiting UI task.
struct Lane {
    slot: Arc<(Mutex<Option<RenderWork>>, Condvar)>,
    unavailable: Option<String>,
}
impl Lane {
    fn new() -> Self {
        let slot: Arc<(Mutex<Option<RenderWork>>, Condvar)> =
            Arc::new((Mutex::new(None), Condvar::new()));
        let worker = slot.clone();
        let unavailable = std::thread::Builder::new()
            .name("gitturtle-model-frames".into())
            .spawn(move || {
                loop {
                    let work = {
                        let mut pending = worker.0.lock().unwrap_or_else(|e| e.into_inner());
                        while pending.is_none() {
                            pending = worker.1.wait(pending).unwrap_or_else(|e| e.into_inner());
                        }
                        pending.take().expect("render slot is populated")
                    };
                    work();
                }
            })
            .err()
            .map(|error| format!("Could not start the 3D render worker: {error}"));
        Self { slot, unavailable }
    }
    fn submit<T: Send + 'static>(
        &self,
        operation: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
    ) -> oneshot::Receiver<anyhow::Result<T>> {
        let (sender, response) = oneshot::channel();
        if let Some(error) = &self.unavailable {
            let _ = sender.send(Err(anyhow::anyhow!(error.clone())));
            return response;
        }
        let work: RenderWork = Box::new(move || {
            if sender.is_canceled() {
                return;
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation))
                .unwrap_or_else(|_| {
                    Err(anyhow::anyhow!("3D frame preparation ended unexpectedly"))
                });
            let _ = sender.send(result);
        });
        *self.slot.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(work);
        self.slot.1.notify_one();
        response
    }
}
fn renderer() -> &'static Lane {
    static RENDERER: OnceLock<Lane> = OnceLock::new();
    RENDERER.get_or_init(Lane::new)
}

#[derive(Clone, Copy)]
struct Drag {
    position: Point<Pixels>,
    pan: bool,
}

struct State {
    camera: ModelCamera,
    fit: ModelBounds,
    configured: bool,
    linked: bool,
    wireframe: bool,
    generation: u64,
    pending: bool,
    frame: Option<Arc<RenderImage>>,
    frame_camera: Option<ModelCamera>,
    frame_wireframe: bool,
    frame_edge: u32,
    error: Option<String>,
    drag: Option<Drag>,
    bounds: Bounds<Pixels>,
    focus: Option<FocusHandle>,
}
impl State {
    fn edge(&self) -> u32 {
        if self.drag.is_some() { 360 } else { 720 }
    }
    fn dirty(&self) -> bool {
        self.frame_camera != Some(self.camera)
            || self.frame_wireframe != self.wireframe
            || self.frame_edge != self.edge()
    }
}

pub struct Document {
    scene: Arc<ModelScene>,
    state: Mutex<State>,
    generation: Arc<AtomicU64>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Bookmark {
    pub target: [f64; 3],
    pub yaw: f64,
    pub pitch: f64,
    pub span: f64,
    pub linked: bool,
    pub wireframe: bool,
}
impl Document {
    pub fn new(scene: Arc<ModelScene>) -> Self {
        Self {
            state: Mutex::new(State {
                camera: ModelCamera::fit(scene.bounds),
                fit: scene.bounds,
                configured: false,
                linked: true,
                wireframe: false,
                generation: 0,
                pending: false,
                frame: None,
                frame_camera: None,
                frame_wireframe: false,
                frame_edge: 0,
                error: None,
                drag: None,
                bounds: Bounds::default(),
                focus: None,
            }),
            scene,
            generation: Arc::new(AtomicU64::new(0)),
        }
    }
    pub fn retained_bytes(&self) -> usize {
        self.scene.retained_bytes() + FRAME_BYTES * 2 + std::mem::size_of::<Self>()
    }
    pub fn bookmark(&self) -> Bookmark {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        Bookmark {
            target: state.camera.target,
            yaw: state.camera.yaw,
            pitch: state.camera.pitch,
            span: state.camera.span,
            linked: state.linked,
            wireframe: state.wireframe,
        }
    }
    pub fn restore(&self, bookmark: Bookmark) {
        if !bookmark
            .target
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 1e15)
            || !bookmark.yaw.is_finite()
            || !bookmark.pitch.is_finite()
            || !bookmark.span.is_finite()
            || !(1e-9..=1e15).contains(&bookmark.span)
        {
            return;
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.camera = ModelCamera {
            target: bookmark.target,
            yaw: bookmark.yaw.rem_euclid(std::f64::consts::TAU),
            pitch: bookmark
                .pitch
                .clamp(-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2),
            span: bookmark.span,
        };
        state.linked = bookmark.linked;
        state.wireframe = bookmark.wireframe;
        state.configured = true;
        state.drag = None;
        self.invalidate(&mut state);
    }
    fn invalidate(&self, state: &mut State) {
        state.generation = state.generation.wrapping_add(1);
        self.generation.store(state.generation, Ordering::Release);
        state.pending = false;
        state.error = None;
    }
    pub fn pause(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.drag = None;
        self.invalidate(&mut state);
    }
    fn configure(&self, fit: ModelBounds) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.fit = fit;
        if !state.configured {
            state.configured = true;
            state.camera = ModelCamera::fit(fit);
            self.invalidate(&mut state);
        }
    }
    fn change(&self, partner: Option<&Document>, action: Action) {
        let (camera, wireframe, linked) = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let before = (state.camera, state.wireframe);
            match action {
                Action::Orbit(x, y) => state.camera.orbit(x, y),
                Action::Pan(x, y) => state.camera.pan(x, y),
                Action::Zoom(factor) => state.camera.zoom(factor),
                Action::Fit => {
                    let fit = if state.linked {
                        state.fit
                    } else {
                        self.scene.bounds
                    };
                    state.camera.fit_bounds(fit);
                }
                Action::Reset => {
                    let fit = if state.linked {
                        state.fit
                    } else {
                        self.scene.bounds
                    };
                    state.camera = ModelCamera::fit(fit);
                }
                Action::View(view) => state.camera.set_view(view),
                Action::Wireframe => state.wireframe = !state.wireframe,
            }
            if before != (state.camera, state.wireframe) || state.error.is_some() {
                self.invalidate(&mut state);
            }
            (state.camera, state.wireframe, state.linked)
        };
        if linked && let Some(partner) = partner {
            let mut state = partner.state.lock().unwrap_or_else(|e| e.into_inner());
            if (state.camera, state.wireframe) != (camera, wireframe) {
                state.camera = camera;
                state.wireframe = wireframe;
                partner.invalidate(&mut state);
            }
        }
    }
    fn set_linked(&self, partner: &Document, linked: bool) {
        let (camera, wireframe) = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.linked = linked;
            (state.camera, state.wireframe)
        };
        let mut state = partner.state.lock().unwrap_or_else(|e| e.into_inner());
        state.linked = linked;
        if linked {
            state.camera = camera;
            state.wireframe = wireframe;
            partner.invalidate(&mut state);
        }
    }
    fn end_drag(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.drag.take().is_some() {
            self.invalidate(&mut state);
        }
    }
}

#[derive(Clone, Copy)]
enum Action {
    Orbit(f64, f64),
    Pan(f64, f64),
    Zoom(f64),
    Fit,
    Reset,
    View(ModelStandardView),
    Wireframe,
}

struct Request {
    document: Weak<Document>,
    scene: Arc<ModelScene>,
    token: Arc<AtomicU64>,
    generation: u64,
    camera: ModelCamera,
    edge: u32,
    wireframe: bool,
}

fn request_pair<T: 'static>(documents: &[Arc<Document>], window: &mut Window, cx: &mut Context<T>) {
    if !window.is_window_active() {
        for document in documents {
            document.pause();
        }
        return;
    }
    let mut requests = Vec::with_capacity(2);
    for document in documents {
        let mut state = document.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.pending || state.error.is_some() || !state.dirty() {
            continue;
        }
        state.pending = true;
        requests.push(Request {
            document: Arc::downgrade(document),
            scene: document.scene.clone(),
            token: document.generation.clone(),
            generation: state.generation,
            camera: state.camera,
            edge: state.edge(),
            wireframe: state.wireframe,
        });
    }
    if requests.is_empty() {
        return;
    }
    // Optional native evidence: request dispatch through the frame callback
    // after publication. This includes lane wait, CPU raster/pixel conversion
    // and UI delivery, but excludes input delivery and completed GPU work.
    let trace_started = std::env::var_os("GITTURTLE_TRACE")
        .is_some()
        .then(std::time::Instant::now);
    let consumers: Vec<_> = requests
        .iter()
        .map(|r| {
            (
                r.document.clone(),
                r.generation,
                r.camera,
                r.edge,
                r.wireframe,
            )
        })
        .collect();
    let response = renderer().submit(move || {
        let check = || {
            anyhow::ensure!(
                requests
                    .iter()
                    .all(|r| r.token.load(Ordering::Acquire) == r.generation
                        && r.document.strong_count() > 0),
                "{CANCELLED}"
            );
            Ok(())
        };
        let mut frames = Vec::with_capacity(requests.len());
        for request in &requests {
            check()?;
            let frame = model3d::render_model(
                &request.scene,
                &request.camera,
                request.edge,
                request.wireframe,
                check,
            );
            check()?;
            frames.push(frame.and_then(|frame| worker::render_image(&frame)));
        }
        check()?;
        Ok(frames)
    });
    cx.spawn_in(window, async move |owner, cx| {
        let result = response.await.ok();
        let shared_error = match &result {
            Some(Err(error)) if error.to_string() != CANCELLED => Some(format!("{error:#}")),
            _ => None,
        };
        let mut frames = match result {
            Some(Ok(frames)) => frames.into_iter(),
            _ => Vec::new().into_iter(),
        };
        let mut changed = false;
        let mut completed = Vec::with_capacity(2);
        for (weak, generation, camera, edge, wireframe) in consumers {
            let frame = frames.next();
            let Some(document) = weak.upgrade() else {
                continue;
            };
            let mut state = document.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.generation != generation || !state.pending {
                continue;
            }
            state.pending = false;
            changed = true;
            if let Some(Ok(frame)) = frame {
                state.frame = Some(frame);
                state.frame_camera = Some(camera);
                state.frame_edge = edge;
                state.frame_wireframe = wireframe;
                if trace_started.is_some() {
                    completed.push((weak, generation, edge));
                }
            } else if let Some(Err(error)) = frame {
                state.error = Some(format!("{error:#}"));
            } else if let Some(error) = &shared_error {
                state.error = Some(error.clone());
            }
        }
        if changed {
            let _ = owner.update_in(cx, |_, window, cx| {
                window.refresh();
                cx.notify();
                if let Some(start) = trace_started
                    && !completed.is_empty()
                {
                    window.on_next_frame(move |_, _| {
                        for (weak, generation, edge) in completed {
                            if let Some(document) = weak.upgrade() {
                                let state = document.state.lock().unwrap_or_else(|e| e.into_inner());
                                if state.generation == generation && !state.pending {
                                    eprintln!("gitturtle.model_frame_callback_ms={:.3} edge={edge} triangles={}", start.elapsed().as_secs_f64() * 1000., document.scene.triangle_count());
                                }
                            }
                        }
                    });
                }
            });
        }
    })
    .detach();
}

pub(super) fn pause(content: Option<&Content>) {
    if let Some(Content::Rich(comparison)) = content {
        for side in [&comparison.old, &comparison.new] {
            if let Some(document) = &side.model {
                document.pause();
            }
        }
    }
}

pub(super) fn render_comparison<T: 'static>(
    preview: &rich_preview::Comparison,
    quick: bool,
    owner: WeakEntity<GitTurtle>,
    cx: &mut Context<T>,
) -> AnyElement {
    let colors = palette(cx);
    let documents: Vec<_> = [&preview.old, &preview.new]
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !quick || *i == 1)
        .filter_map(|(_, side)| side.model.clone())
        .collect();
    if let Some(fit) = documents
        .iter()
        .map(|d| d.scene.bounds)
        .reduce(ModelBounds::union)
    {
        for document in &documents {
            document.configure(fit);
        }
    }
    let view = cx.entity().downgrade();
    div()
        .size_full()
        .flex()
        .flex_col()
        .min_w_0()
        .min_h_0()
        .when(!documents.is_empty(), |element| {
            element.child(
                div()
                    .px_3()
                    .py_1()
                    .text_size(appearance::ui_text(10.))
                    .text_color(rgb(colors.muted))
                    .child("Drag to orbit · Shift-drag to pan · Scroll to zoom"),
            )
        })
        .child(
            div().flex_1().min_h_0().min_w_0().flex().children(
                [
                    ("Before", &preview.old, &preview.new),
                    (
                        if quick { "Source" } else { "After" },
                        &preview.new,
                        &preview.old,
                    ),
                ]
                .into_iter()
                .enumerate()
                .filter(|(i, _)| !quick || *i == 1)
                .map(|(index, (label, side, other))| {
                    render_side(
                        index,
                        label,
                        side,
                        if quick { None } else { other.model.clone() },
                        owner.clone(),
                        cx,
                    )
                }),
            ),
        )
        .on_prepaint(move |_, window, cx| {
            let _ = view.update(cx, |_, cx| request_pair(&documents, window, cx));
        })
        .into_any_element()
}

fn selected_standard_view(camera: ModelCamera) -> Option<ModelStandardView> {
    ModelStandardView::ALL.into_iter().find(|view| {
        let mut standard = camera;
        standard.set_view(*view);
        // Bookmarks and orbit normalize yaw, while standard views use signed
        // angles. Equivalent orientations must retain the same visible label.
        let yaw_distance = (standard.yaw - camera.yaw).rem_euclid(std::f64::consts::TAU);
        yaw_distance.min(std::f64::consts::TAU - yaw_distance) < 1e-8
            && (standard.pitch - camera.pitch).abs() < 1e-8
    })
}

fn render_side<T: 'static>(
    index: usize,
    label: &'static str,
    side: &Arc<rich_preview::Side>,
    partner: Option<Arc<Document>>,
    owner: WeakEntity<GitTurtle>,
    cx: &mut Context<T>,
) -> AnyElement {
    let colors = palette(cx);
    let mut header = div()
        .flex()
        .flex_col()
        .gap_1()
        .px_2()
        .py_1()
        .border_b_1()
        .border_color(rgb(colors.border));
    let format = if side.model.is_some() {
        side.metadata.format.clone()
    } else {
        side.name
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_uppercase())
            .filter(|extension| {
                matches!(
                    extension.as_str(),
                    "GLB" | "OBJ" | "STL" | "FBX" | "3MF" | "STEP" | "STP"
                )
            })
            .unwrap_or_else(|| "3D".into())
    };
    let lfs_pointer = side
        .captured
        .as_deref()
        .is_some_and(|bytes| gitturtle_preview::detect_lfs_pointer(bytes).is_some());
    let source_label = if side.present {
        format!("{label} · {format}")
    } else {
        label.into()
    };
    let mut source_actions = div().flex().flex_wrap().items_center().gap_2().child(
        div()
            .id(("model-source-label", index))
            .role(Role::Label)
            .aria_label(source_label.clone())
            .font_weight(FontWeight::SEMIBOLD)
            .child(source_label),
    );
    if let Some(source) = side.metadata.source.clone() {
        source_actions = source_actions.child(
            button(
                ("model-source", index),
                if lfs_pointer {
                    "Copy pointer"
                } else {
                    "Copy source"
                },
                "copy",
                false,
            )
            .accessibility_label(format!(
                "Copy exact {label} {}",
                if lfs_pointer {
                    "LFS pointer"
                } else {
                    "model source"
                }
            ))
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(source.to_string()))
            }),
        );
    }
    if side.captured.is_some() && !lfs_pointer {
        let side = side.clone();
        source_actions = source_actions.child(
            button(
                ("model-external", index),
                "System preview",
                "external-link",
                false,
            )
            .accessibility_label(format!("Open captured {label} model in system preview"))
            .on_click(move |_, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.open_captured_preview(side.clone(), window, cx)
                });
            }),
        );
    }
    header = header.child(source_actions);
    let Some(document) = side.model.clone() else {
        let message = side.error.clone().unwrap_or_else(|| {
            if side.present {
                "3D preview unavailable"
            } else {
                "No file on this side"
            }
            .into()
        });
        return div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .id(("model-unavailable", index))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .role(if side.error.is_some() {
                        Role::Alert
                    } else {
                        Role::Label
                    })
                    .aria_label(bounded_accessible_text(format!("{label}: {message}")))
                    .when(side.error.is_some(), |element| {
                        element.a11y_synthetic_children(native_accessibility::assertive)
                    })
                    .p_3()
                    .when(side.error.is_some(), |element| {
                        element.child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .mb_2()
                                .child("This version can’t be displayed"),
                        )
                    })
                    .child(
                        div()
                            .text_size(appearance::ui_text(12.))
                            .text_color(rgb(colors.muted))
                            .child(message),
                    ),
            )
            .into_any_element();
    };
    let (camera, wireframe, linked, pending, error, frame, display_camera, focus) = {
        let mut state = document.state.lock().unwrap_or_else(|e| e.into_inner());
        let focus = state.focus.get_or_insert_with(|| cx.focus_handle()).clone();
        (
            state.camera,
            state.wireframe,
            state.linked,
            state.pending,
            state.error.clone(),
            state.frame.clone(),
            state.frame_camera.unwrap_or(state.camera),
            focus,
        )
    };
    let mut controls = div().flex().flex_wrap().items_center().gap_1();
    for (id, text, action) in [
        (0, "Fit", Action::Fit),
        (1, "Reset", Action::Reset),
        (2, "−", Action::Zoom(0.8)),
        (3, "+", Action::Zoom(1.25)),
        (
            4,
            if wireframe { "Solid" } else { "Edges" },
            Action::Wireframe,
        ),
    ] {
        let target = document.clone();
        let partner = partner.clone();
        controls = controls.child(
            button(
                ("model-camera", index * 5 + id),
                text,
                "",
                id == 4 && wireframe,
            )
            .when(id == 4, |button| button.toggled(wireframe))
            .accessibility_label(format!(
                "{label}: {}",
                match id {
                    2 => "Zoom out",
                    3 => "Zoom in",
                    4 => "Toggle all triangle edges",
                    _ => text,
                }
            ))
            .on_click(cx.listener(move |_, _, window, cx| {
                target.change(partner.as_deref(), action);
                window.refresh();
                cx.notify();
            })),
        );
    }
    let selected_view = selected_standard_view(camera);
    let target = document.clone();
    let view_partner = partner.clone();
    let view_owner = cx.entity().downgrade();
    let mut views = div().flex().flex_wrap().items_center().gap_1().child(
        button(
            ("model-views", index),
            selected_view.map_or("Custom view", |view| view.label()),
            "",
            false,
        )
        .dropdown_caret(true)
        .accessibility_label(format!(
            "{label}: Standard views, {}",
            selected_view.map_or("Custom", |view| view.label())
        ))
        .tooltip("Choose Isometric, Front, Back, Left, Right, Top or Bottom")
        .dropdown_menu(move |mut menu, _, _| {
            menu = menu.label(format!("{label} standard view"));
            for view in ModelStandardView::ALL {
                let target = target.clone();
                let partner = view_partner.clone();
                let owner = view_owner.clone();
                menu = menu.item(
                    PopupMenuItem::new(view.label())
                        .checked(selected_view == Some(view))
                        .on_click(move |_, window, cx| {
                            target.change(partner.as_deref(), Action::View(view));
                            window.refresh();
                            let _ = owner.update(cx, |_, cx| cx.notify());
                        }),
                );
            }
            menu
        }),
    );
    if let Some(partner) = partner.clone() {
        let target = document.clone();
        views = views.child(
            button(
                ("model-link", index),
                if linked { "Linked" } else { "Independent" },
                "",
                linked,
            )
            .toggled(linked)
            .accessibility_label(format!(
                "{label}: {}",
                if linked {
                    "Cameras linked. Switch to independent cameras"
                } else {
                    "Cameras independent. Link cameras to this view"
                }
            ))
            .on_click(cx.listener(move |_, _, window, cx| {
                target.set_linked(&partner, !linked);
                window.refresh();
                cx.notify();
            })),
        );
    }
    let summary = format!(
        "{} triangles · view span {:.4} {}",
        document.scene.triangle_count(),
        camera.span,
        document.scene.units.label()
    );
    header = header.child(
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x_3()
            .gap_y_1()
            .child(controls)
            .child(views),
    );
    let first_frame = frame.is_none();
    let mut canvas = model_canvas(
        index,
        label,
        document.clone(),
        partner,
        focus,
        frame,
        display_camera,
        error.is_some(),
        cx,
    );
    if let Some(error) = error {
        canvas = div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .id(("model-render-error", index))
                    .role(Role::Alert)
                    .a11y_synthetic_children(native_accessibility::assertive)
                    .aria_label(format!(
                        "{label}: {error}. {} Fit or Reset to try another view.",
                        if first_frame {
                            "No completed view is available."
                        } else {
                            "Previous completed view retained."
                        }
                    ))
                    .max_h(px(96.))
                    .overflow_y_scroll()
                    .p_2()
                    .text_size(appearance::ui_text(12.))
                    .text_color(rgb(colors.warning))
                    .child(format!(
                        "{error} · {} Fit or Reset to try another view.",
                        if first_frame {
                            "No completed view is available."
                        } else {
                            "Previous completed view retained."
                        }
                    )),
            )
            .child(canvas)
            .into_any_element();
    } else {
        canvas = div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .relative()
            .child(canvas)
            .child(
                div()
                    .id(("model-render-status", index))
                    .role(Role::Status)
                    .aria_label(format!(
                        "{label}: {}",
                        if first_frame {
                            "Preparing 3D view"
                        } else if pending {
                            "Updating 3D view"
                        } else {
                            "3D view ready"
                        }
                    ))
                    .a11y_synthetic_children(move |builder| {
                        builder.parent_node().set_live(gpui::accesskit::Live::Off);
                        if pending {
                            builder.parent_node().set_busy();
                        }
                    })
                    .absolute()
                    .top_1()
                    .left_2()
                    .when(pending && !first_frame, |element| {
                        element.px_2().py_1().rounded_md().bg(rgb(colors.panel))
                    })
                    .text_size(appearance::ui_text(10.))
                    .text_color(rgb(colors.muted))
                    .child(if pending && !first_frame {
                        "Updating view…"
                    } else {
                        ""
                    }),
            )
            .into_any_element();
    }
    let details = side
        .metadata
        .details
        .iter()
        .filter(|s| !s.starts_with("Filename hint:"))
        .cloned()
        .collect::<Vec<_>>()
        .join(" · ");
    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .flex_col()
        .border_r_1()
        .border_color(rgb(colors.border))
        .child(header)
        .child(canvas)
        .child(
            div()
                .id(("model-details", index))
                .role(Role::Label)
                .aria_label(bounded_accessible_text(format!(
                    "{label} model details: {details}"
                )))
                .max_h(px(48.))
                .overflow_y_scroll()
                .p_2()
                .text_size(appearance::ui_text(10.))
                .text_color(rgb(colors.muted))
                .child(
                    div()
                        .id(("model-summary", index))
                        .role(Role::Label)
                        .aria_label(format!("{label}: Geometry only · {summary}"))
                        .child(format!(
                            "Geometry only · {} triangles · {}",
                            document.scene.triangle_count(),
                            document.scene.units.label()
                        )),
                )
                .child(details),
        )
        .into_any_element()
}

fn bounded_accessible_text(mut text: String) -> String {
    const MAX_BYTES: usize = 16 * 1024;
    if text.len() > MAX_BYTES {
        let mut end = MAX_BYTES - '…'.len_utf8();
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push('…');
    }
    text
}

#[allow(clippy::too_many_arguments)]
fn model_canvas<T: 'static>(
    index: usize,
    label: &'static str,
    document: Arc<Document>,
    partner: Option<Arc<Document>>,
    focus: FocusHandle,
    frame: Option<Arc<RenderImage>>,
    camera: ModelCamera,
    failed: bool,
    cx: &mut Context<T>,
) -> AnyElement {
    let colors = palette(cx);
    let down = document.clone();
    let click_focus = focus.clone();
    let moved = document.clone();
    let move_partner = partner.clone();
    let up = document.clone();
    let outside = document.clone();
    let wheel = document.clone();
    let wheel_partner = partner.clone();
    let key = document.clone();
    let key_partner = partner;
    let measured = document.clone();
    let has_frame = frame.is_some();
    div().id(("model-canvas",index)).flex_1().min_h_0().min_w_0().relative().overflow_hidden().bg(rgb(0x1c222a)).border_1().border_color(rgb(colors.border))
        .focus_visible(|style|style.border_color(rgb(colors.accent))).tab_stop(true).track_focus(&focus).role(Role::Image)
        .aria_label(format!("{label} interactive 3D model, {} triangles",document.scene.triangle_count()))
        .aria_description(format!(
            "{} Orientation: yaw {:.1} degrees, elevation {:.1} degrees. Target X {:.4}, Y {:.4}, Z {:.4}; view span {:.4} {}. Arrows orbit; Shift and arrows pan; plus and minus zoom; F fits; zero resets; W toggles edges. The captured original is available above.",
            if has_frame { "Completed view." } else { "No completed view yet." }, camera.yaw.to_degrees(), camera.pitch.to_degrees(), camera.target[0], camera.target[1], camera.target[2], camera.span, document.scene.units.label()
        ))
        .cursor(CursorStyle::OpenHand)
        .on_mouse_down(MouseButton::Left,cx.listener(move |_,event:&MouseDownEvent,window,cx|{
            click_focus.focus(window,cx);
            let mut state=down.state.lock().unwrap_or_else(|e|e.into_inner());
            state.drag=Some(Drag{position:event.position,pan:event.modifiers.shift});
            down.invalidate(&mut state);drop(state);cx.stop_propagation();cx.notify();
        }))
        .on_mouse_move(cx.listener(move |_,event:&MouseMoveEvent,window,cx|{
            let action={
                let mut state=moved.state.lock().unwrap_or_else(|e|e.into_inner());
                let Some(drag)=state.drag else{return;};
                if event.pressed_button!=Some(MouseButton::Left){state.drag=None;return;}
                let delta=event.position-drag.position;
                let edge=f32::from(state.bounds.size.width).min(f32::from(state.bounds.size.height)).max(1.) as f64;
                state.drag=Some(Drag{position:event.position,..drag});
                let (x,y)=(f32::from(delta.x) as f64/edge,f32::from(delta.y) as f64/edge);
                if drag.pan {Action::Pan(x,y)}else{Action::Orbit(-x*std::f64::consts::TAU,y*std::f64::consts::PI)}
            };
            moved.change(move_partner.as_deref(),action);window.refresh();cx.stop_propagation();cx.notify();
        }))
        .on_mouse_up(MouseButton::Left,cx.listener(move |_,_,window,cx|{up.end_drag();window.refresh();cx.notify();}))
        .on_mouse_up_out(MouseButton::Left,cx.listener(move |_,_,window,cx|{outside.end_drag();window.refresh();cx.notify();}))
        .on_scroll_wheel(cx.listener(move |_,event:&ScrollWheelEvent,window,cx|{
            let y=f32::from(event.delta.pixel_delta(px(24.)).y) as f64;
            wheel.change(wheel_partner.as_deref(),Action::Zoom((y*0.008).clamp(-0.7,0.7).exp()));window.refresh();cx.stop_propagation();cx.notify();
        }))
        .on_key_down(cx.listener(move |_,event:&KeyDownEvent,window,cx|{
            if event.keystroke.modifiers.platform || event.keystroke.modifiers.control || event.keystroke.modifiers.alt {return;}
            let shift=event.keystroke.modifiers.shift;
            let action=match event.keystroke.key.as_str(){
                "left"=>Some(if shift{Action::Pan(-0.06,0.)}else{Action::Orbit(-0.15,0.)}),
                "right"=>Some(if shift{Action::Pan(0.06,0.)}else{Action::Orbit(0.15,0.)}),
                "up"=>Some(if shift{Action::Pan(0.,-0.06)}else{Action::Orbit(0.,0.15)}),
                "down"=>Some(if shift{Action::Pan(0.,0.06)}else{Action::Orbit(0.,-0.15)}),
                "+"|"="=>Some(Action::Zoom(1.25)),"-"=>Some(Action::Zoom(0.8)),"f"=>Some(Action::Fit),"0"=>Some(Action::Reset),"w"=>Some(Action::Wireframe),_=>None,
            };
            if let Some(action)=action {key.change(key_partner.as_deref(),action);window.refresh();cx.stop_propagation();cx.notify();}
        }))
        .on_prepaint(move |bounds,_,_|{measured.state.lock().unwrap_or_else(|e|e.into_inner()).bounds=bounds;})
        .child(if let Some(frame)=frame {gif_playback::static_image(frame)}else{div().size_full().flex().items_center().justify_center().text_color(rgb(0xb9c8d3)).child(if failed { "No 3D view available" } else { "Preparing model…" }).into_any_element()})
        .when(has_frame, |element| element.child(orientation(camera).absolute().right_2().bottom_2())).into_any_element()
}

fn orientation(camera: ModelCamera) -> Div {
    let axes = camera.orientation_axes();
    div()
        .relative()
        .w(px(88.))
        .h(px(88.))
        .rounded(px(5.))
        .bg(rgba(0x1c222a80))
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let origin = bounds.origin + point(px(44.), px(44.));
                    for axis in axes {
                        let mut path = PathBuilder::stroke(px(2.));
                        path.move_to(origin);
                        path.line_to(
                            origin
                                + point(
                                    px(axis.direction[0] as f32 * 27.),
                                    px(axis.direction[1] as f32 * 27.),
                                ),
                        );
                        if let Ok(path) = path.build() {
                            window.paint_path(
                                path,
                                rgb((axis.color[0] as u32) << 16
                                    | (axis.color[1] as u32) << 8
                                    | axis.color[2] as u32),
                            );
                        }
                    }
                },
            )
            .size_full(),
        )
        .children(axes.into_iter().map(|axis| {
            div()
                .absolute()
                .left(px(40. + axis.direction[0] as f32 * 33.))
                .top(px(35. + axis.direction[1] as f32 * 33.))
                .text_size(appearance::ui_text(11.))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb((axis.color[0] as u32) << 16
                    | (axis.color[1] as u32) << 8
                    | axis.color[2] as u32))
                .child(axis.label)
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    fn document() -> Document {
        Document::new(
            model3d::decode_geometry(b"v 0 0 0\nv 2 0 0\nv 0 3 4\nf 1 2 3\n", "model.obj", || {
                Ok(())
            })
            .unwrap()
            .scene,
        )
    }
    #[test]
    fn linked_camera_changes_share_scale_and_unlink_preserves_independent_inspection() {
        let before = document();
        let after = document();
        before.configure(before.scene.bounds.union(after.scene.bounds));
        after.configure(before.scene.bounds.union(after.scene.bounds));
        before.change(Some(&after), Action::Zoom(2.));
        assert_eq!(
            before.state.lock().unwrap().camera,
            after.state.lock().unwrap().camera
        );
        before.set_linked(&after, false);
        let old = after.state.lock().unwrap().camera;
        before.change(Some(&after), Action::Orbit(1., 0.5));
        assert_eq!(after.state.lock().unwrap().camera, old);
        before.set_linked(&after, true);
        assert_eq!(
            before.state.lock().unwrap().camera,
            after.state.lock().unwrap().camera
        );
    }

    #[test]
    fn glb_initial_camera_fits_revision_union_and_retains_changed_placement() {
        let before = Document::new(
            model3d::decode_geometry(
                include_bytes!("../../preview/tests/fixtures/models/glb/assembly-before.glb"),
                "before.glb",
                || Ok(()),
            )
            .unwrap()
            .scene,
        );
        let after = Document::new(
            model3d::decode_geometry(
                include_bytes!("../../preview/tests/fixtures/models/glb/assembly-after.glb"),
                "after.GLB",
                || Ok(()),
            )
            .unwrap()
            .scene,
        );
        let union = before.scene.bounds.union(after.scene.bounds);
        before.configure(union);
        after.configure(union);
        let camera = before.state.lock().unwrap().camera;
        assert_eq!(camera, after.state.lock().unwrap().camera);
        for (actual, expected) in camera.target.into_iter().zip([1050., 0., 1800.]) {
            assert!((actual - expected).abs() < 1e-6);
        }
        assert_ne!(camera.target, before.scene.bounds.center());
        assert_ne!(camera.target, after.scene.bounds.center());
        assert_eq!(before.scene.units, model3d::ModelUnits::Millimeters);
        before.change(Some(&after), Action::View(ModelStandardView::Front));
        before.change(Some(&after), Action::Zoom(2.));
        let bookmark = before.bookmark();
        before.pause();
        before.configure(union);
        assert_eq!(before.bookmark().target, bookmark.target);
        assert_eq!(before.bookmark().span, bookmark.span);
        before.change(Some(&after), Action::Reset);
        assert_eq!(before.state.lock().unwrap().camera, camera);
        assert_eq!(after.state.lock().unwrap().camera, camera);
        before.set_linked(&after, false);
        before.change(Some(&after), Action::Fit);
        assert_eq!(before.bookmark().target, before.scene.bounds.center());
        assert_eq!(after.state.lock().unwrap().camera, camera);
    }
    #[test]
    fn camera_edits_and_pause_invalidate_render_generation() {
        let document = document();
        let initial = document.generation.load(Ordering::Acquire);
        document.state.lock().unwrap().pending = true;
        document.change(None, Action::Orbit(0.2, 0.));
        assert_ne!(document.generation.load(Ordering::Acquire), initial);
        assert!(!document.state.lock().unwrap().pending);
        document.state.lock().unwrap().pending = true;
        document.pause();
        assert!(!document.state.lock().unwrap().pending);
        assert!(document.retained_bytes() >= document.scene.retained_bytes() + FRAME_BYTES * 2);
    }

    #[test]
    fn renderer_replaces_queued_work_and_releases_its_resources() {
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let first = renderer().submit(move || {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(1)
        });
        started_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let obsolete = Arc::new(AtomicU64::new(0));
        let weak = Arc::downgrade(&obsolete);
        let replaced = renderer().submit(move || {
            obsolete.store(1, Ordering::Release);
            Ok(2)
        });
        let latest = renderer().submit(|| Ok(3));
        assert!(
            weak.upgrade().is_none(),
            "replacing a queued render must release its retained data"
        );
        release_tx.send(()).unwrap();
        assert_eq!(futures::executor::block_on(first).unwrap().unwrap(), 1);
        assert!(futures::executor::block_on(replaced).is_err());
        assert_eq!(futures::executor::block_on(latest).unwrap().unwrap(), 3);
    }

    #[test]
    fn bookmark_restores_camera_and_modes_without_pointer_or_pending_work() {
        let source = document();
        source.change(None, Action::Orbit(0.7, 0.3));
        source.change(None, Action::Zoom(2.));
        source.change(None, Action::Wireframe);
        let saved = source.bookmark();
        let restored = document();
        restored.restore(saved.clone());
        restored.configure(ModelBounds {
            minimum: [-100.; 3],
            maximum: [100.; 3],
        });
        let state = restored.state.lock().unwrap();
        assert!((state.camera.yaw - saved.yaw.rem_euclid(std::f64::consts::TAU)).abs() < 1e-12);
        assert_eq!(state.camera.target, saved.target);
        assert_eq!(state.camera.span, saved.span);
        assert!(state.wireframe);
        assert!(!state.pending);
        assert!(state.drag.is_none());
        drop(state);
        restored.restore(Bookmark {
            span: f64::NAN,
            ..saved
        });
        assert!(restored.state.lock().unwrap().camera.span.is_finite());
    }

    #[test]
    fn standard_view_labels_survive_serialized_camera_bookmark_restoration() {
        for view in ModelStandardView::ALL {
            let source = document();
            source.change(None, Action::View(view));
            source.change(None, Action::Pan(0.2, -0.1));
            source.change(None, Action::Zoom(2.));
            let before = source.state.lock().unwrap().camera;
            assert_eq!(selected_standard_view(before), Some(view));
            let saved = serde_json::to_vec(&source.bookmark()).unwrap();
            let restored = document();
            restored.restore(serde_json::from_slice(&saved).unwrap());
            restored.configure(restored.scene.bounds);
            let after = restored.state.lock().unwrap().camera;
            assert_eq!(selected_standard_view(after), Some(view));
            assert_eq!(after.target, before.target);
            assert_eq!(after.span, before.span);
            for (actual, expected) in after
                .orientation_axes()
                .into_iter()
                .zip(before.orientation_axes())
            {
                for (actual, expected) in actual.direction.into_iter().zip(expected.direction) {
                    assert!((actual - expected).abs() < 1e-12);
                }
            }
        }
    }

    #[test]
    fn standard_view_matching_wraps_yaw_and_preserves_custom_orientations() {
        let document = document();
        for view in ModelStandardView::ALL {
            let mut camera = document.state.lock().unwrap().camera;
            camera.set_view(view);
            for turns in [-3., -1., 0., 1., 3.] {
                let mut equivalent = camera;
                equivalent.yaw += turns * std::f64::consts::TAU;
                assert_eq!(selected_standard_view(equivalent), Some(view));
                equivalent.yaw += 1e-4;
                assert_eq!(selected_standard_view(equivalent), None);
            }
            camera.pitch += 1e-4;
            assert_eq!(selected_standard_view(camera), None);
        }
        let mut camera = document.state.lock().unwrap().camera;
        camera.set_view(ModelStandardView::Right);
        camera.yaw = -1e-10;
        assert_eq!(
            selected_standard_view(camera),
            Some(ModelStandardView::Right)
        );
    }
}
