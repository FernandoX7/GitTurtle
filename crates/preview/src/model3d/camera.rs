//! Retained, immutable geometry and an orthographic camera. Camera edits are
//! constant work; callers render changed frames on their cancellable worker.
use super::appearance::{AlphaMode, SceneAppearance, linear_to_srgb, srgb_to_linear};
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelUnits {
    /// Source coordinates are preserved; physical comparison is not established.
    Unknown,
    Millimeters,
}

impl ModelUnits {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "source units (physical units unknown)",
            Self::Millimeters => "mm",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelBounds {
    pub minimum: [f64; 3],
    pub maximum: [f64; 3],
}

impl ModelBounds {
    pub fn union(self, other: Self) -> Self {
        Self {
            minimum: std::array::from_fn(|i| self.minimum[i].min(other.minimum[i])),
            maximum: std::array::from_fn(|i| self.maximum[i].max(other.maximum[i])),
        }
    }

    pub fn center(self) -> [f64; 3] {
        std::array::from_fn(|i| (self.minimum[i] + self.maximum[i]) * 0.5)
    }

    pub fn extent(self) -> [f64; 3] {
        subtract(self.maximum, self.minimum)
    }
}

#[derive(Debug)]
pub struct ModelScene {
    triangles: Box<[Triangle]>,
    pub bounds: ModelBounds,
    pub units: ModelUnits,
    format: String,
    appearance: Option<Arc<SceneAppearance>>,
    animation_source: Option<Arc<glb::GlbAnimation>>,
    reverse_winding: Option<Box<[bool]>>,
}

impl ModelScene {
    pub(super) fn new(mesh: Vec<Triangle>, units: ModelUnits, format: &str) -> Result<Self> {
        let (minimum, maximum) = bounds(&mesh)?;
        Ok(Self {
            triangles: mesh.into_boxed_slice(),
            bounds: ModelBounds { minimum, maximum },
            units,
            format: format.into(),
            appearance: None,
            animation_source: None,
            reverse_winding: None,
        })
    }

    /// Accounting includes retained triangle coordinates, not only input bytes.
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of_val(self.triangles.as_ref())
            + self.format.capacity()
            + self
                .appearance
                .as_ref()
                .map_or(0, |value| value.retained_bytes())
            + self
                .animation_source
                .as_ref()
                .map_or(0, |value| value.retained_bytes())
            + self
                .reverse_winding
                .as_ref()
                .map_or(0, |value| std::mem::size_of_val(value.as_ref()))
            + std::mem::size_of::<Self>()
    }

    pub(super) fn set_appearance(&mut self, appearance: Arc<SceneAppearance>) {
        self.appearance = Some(appearance);
    }

    pub(super) fn set_animation_source(&mut self, source: Arc<glb::GlbAnimation>) {
        self.animation_source = Some(source);
    }

    pub(super) fn set_reverse_winding(&mut self, reverse: Vec<bool>) {
        self.reverse_winding = Some(reverse.into_boxed_slice());
    }

    /// The supported shading contract for the native details/status surface.
    pub fn appearance_label(&self) -> &'static str {
        let Some(appearance) = &self.appearance else {
            return "Geometry only";
        };
        let unlit = appearance.materials.iter().any(|m| m.unlit);
        let lit = appearance.materials.iter().any(|m| !m.unlit);
        match (unlit, lit) {
            (true, true) => "Unlit and base-color inspection",
            (true, false) => "Unlit appearance",
            _ => "Base-color inspection",
        }
    }

    pub fn animation_clips(&self) -> &[glb::ModelAnimationClip] {
        self.animation_source
            .as_ref()
            .map_or(&[], |source| source.clips())
    }

    /// Evaluate on a cancellable worker; the caller retains its camera unchanged.
    pub fn evaluate_animation(
        &self,
        clip: usize,
        time_seconds: f64,
        check: &impl Fn() -> Result<()>,
    ) -> Result<Self> {
        let source = self
            .animation_source
            .as_ref()
            .context("This model has no animation clips")?;
        let frame = source.evaluate(clip, time_seconds, check)?;
        let mut minimum = [f64::INFINITY; 3];
        let mut maximum = [f64::NEG_INFINITY; 3];
        for (index, triangle) in frame.triangles.iter().enumerate() {
            if index.is_multiple_of(256) {
                check()?;
            }
            for point in triangle {
                for axis in 0..3 {
                    minimum[axis] = minimum[axis].min(point[axis]);
                    maximum[axis] = maximum[axis].max(point[axis]);
                }
            }
        }
        ensure!(
            minimum.iter().chain(&maximum).all(|v| v.is_finite()),
            "Animation produced no finite geometry"
        );
        // A valid animated scale may collapse all triangles. Keep the captured
        // topology and camera; rasterization then correctly produces background.
        Ok(Self {
            triangles: frame.triangles.into_boxed_slice(),
            bounds: ModelBounds { minimum, maximum },
            units: self.units,
            format: self.format.clone(),
            appearance: self.appearance.clone(),
            animation_source: self.animation_source.clone(),
            reverse_winding: Some(frame.reverse_winding.into_boxed_slice()),
        })
    }

    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    pub fn triangles(&self) -> &[[[f64; 3]; 3]] {
        &self.triangles
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelStandardView {
    Isometric,
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
}

impl ModelStandardView {
    pub const ALL: [Self; 7] = [
        Self::Isometric,
        Self::Front,
        Self::Back,
        Self::Left,
        Self::Right,
        Self::Top,
        Self::Bottom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Isometric => "Isometric",
            Self::Front => "Front",
            Self::Back => "Back",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Top => "Top",
            Self::Bottom => "Bottom",
        }
    }
}

/// Use the *same* camera for both revisions after fitting their union bounds.
/// Fitting each side separately would conceal changes in position and size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelCamera {
    pub target: [f64; 3],
    /// Radians around the source Z axis.
    pub yaw: f64,
    /// Radians above the source XY plane.
    pub pitch: f64,
    /// Source-unit width and height of the square orthographic viewport.
    pub span: f64,
}

impl ModelCamera {
    pub fn fit(bounds: ModelBounds) -> Self {
        let extent = bounds.extent();
        let mut camera = Self {
            target: bounds.center(),
            yaw: 0.,
            pitch: 0.,
            span: (dot(extent, extent).sqrt() * 1.12).clamp(1e-9, 1e15),
        };
        camera.set_view(ModelStandardView::Isometric);
        camera
    }

    /// Fit without changing the selected orientation.
    pub fn fit_bounds(&mut self, bounds: ModelBounds) {
        let fitted = Self::fit(bounds);
        self.target = fitted.target;
        self.span = fitted.span;
    }

    pub fn set_view(&mut self, view: ModelStandardView) {
        use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};
        (self.yaw, self.pitch) = match view {
            ModelStandardView::Isometric => (-FRAC_PI_4, (0.8f64 / 2f64.sqrt()).atan()),
            ModelStandardView::Front => (-FRAC_PI_2, 0.),
            ModelStandardView::Back => (FRAC_PI_2, 0.),
            ModelStandardView::Left => (PI, 0.),
            ModelStandardView::Right => (0., 0.),
            ModelStandardView::Top => (-FRAC_PI_2, FRAC_PI_2),
            ModelStandardView::Bottom => (-FRAC_PI_2, -FRAC_PI_2),
        };
    }

    pub fn orbit(&mut self, yaw_delta: f64, pitch_delta: f64) {
        if yaw_delta.is_finite() && pitch_delta.is_finite() {
            self.yaw = (self.yaw + yaw_delta).rem_euclid(std::f64::consts::TAU);
            self.pitch = (self.pitch + pitch_delta)
                .clamp(-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2);
        }
    }

    /// Drag fractions of the viewport: positive X moves geometry right, positive
    /// Y moves it down. No geometry work or renderer allocation occurs here.
    pub fn pan(&mut self, x_fraction: f64, y_fraction: f64) {
        if !x_fraction.is_finite() || !y_fraction.is_finite() {
            return;
        }
        let (right, up, _) = self.basis();
        self.target = std::array::from_fn(|i| {
            (self.target[i] + self.span * (-right[i] * x_fraction + up[i] * y_fraction))
                .clamp(-1e15, 1e15)
        });
    }

    /// A factor greater than one magnifies; invalid input leaves the camera alone.
    pub fn zoom(&mut self, factor: f64) {
        if factor.is_finite() && factor > 0. {
            self.span = (self.span / factor).clamp(1e-9, 1e15);
        }
    }

    fn basis(self) -> (Point, Point, Point) {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let forward = [cy * cp, sy * cp, sp];
        let right = [-sy, cy, 0.];
        let up = cross(forward, right);
        (right, up, forward)
    }

    /// Unit-length source axes projected into screen X/right, Y/down, and depth.
    /// Labels are available to native text and accessibility alongside colors.
    pub fn orientation_axes(self) -> [OrientationAxis; 3] {
        let (right, up, forward) = self.basis();
        std::array::from_fn(|axis| OrientationAxis {
            label: ["X", "Y", "Z"][axis],
            direction: [right[axis], -up[axis], forward[axis]],
            color: [[240, 122, 128], [123, 213, 139], [124, 181, 255]][axis],
        })
    }

    /// Normalized screen coordinates, useful for overlays and measurements.
    pub fn project(self, point: [f64; 3]) -> [f64; 3] {
        let (right, up, forward) = self.basis();
        let relative = subtract(point, self.target);
        [
            0.5 + dot(relative, right) / self.span,
            0.5 - dot(relative, up) / self.span,
            dot(relative, forward),
        ]
    }

    fn validate(self) -> Result<()> {
        ensure!(
            self.target.iter().all(|v| v.is_finite() && v.abs() <= 1e15)
                && self.yaw.is_finite()
                && self.pitch.is_finite()
                && self.span.is_finite()
                && (1e-9..=1e15).contains(&self.span),
            "Invalid 3D camera coordinates"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrientationAxis {
    pub label: &'static str,
    pub direction: [f64; 3],
    pub color: [u8; 3],
}

/// Render one requested frame. Use a smaller edge during interaction if needed;
/// the caller owns replacement/cancellation and should request nothing when idle.
/// Wireframe exposes all triangulation edges, including hidden/internal edges.
pub fn render_model(
    scene: &ModelScene,
    camera: &ModelCamera,
    edge: u32,
    wireframe: bool,
    check: impl Fn() -> Result<()>,
) -> Result<ImagePreview> {
    check()?;
    camera.validate()?;
    ensure!(
        (64..=MODEL_VIEW_EDGE).contains(&edge),
        "3D output edge must be between 64 and 720 pixels"
    );
    let size = edge as usize;
    let mut rgba = [28, 34, 42, 255].repeat(size * size);
    let mut depth = if wireframe {
        Vec::new()
    } else {
        vec![f64::NEG_INFINITY; size * size]
    };
    let appearance = scene.appearance.as_deref();
    let has_blend = !wireframe
        && appearance.is_some_and(|a| a.materials.iter().any(|m| m.alpha == AlphaMode::Blend));
    let mut heads = if has_blend {
        vec![u32::MAX; size * size]
    } else {
        Vec::new()
    };
    let mut fragments = Vec::<Fragment>::new();
    // Keep opaque RGB in linear light until transparent compositing has finished.
    let mut linear = if has_blend {
        vec![[srgb_to_linear(28), srgb_to_linear(34), srgb_to_linear(42)]; size * size]
    } else {
        Vec::new()
    };
    let (right, up, forward) = camera.basis();
    let project = |point| {
        let relative = subtract(point, camera.target);
        [
            edge as f64 * (0.5 + dot(relative, right) / camera.span),
            edge as f64 * (0.5 - dot(relative, up) / camera.span),
            dot(relative, forward),
        ]
    };
    let mut work = 0u64;
    let light_direction = normalize([-0.4, 0.6, 1.]);
    for (triangle_index, triangle) in scene.triangles().iter().enumerate() {
        check()?;
        let p = triangle.map(project);
        if wireframe {
            for i in 0..3 {
                line(p[i], p[(i + 1) % 3], size, &mut rgba, &mut work)?;
            }
            continue;
        }
        let signed_area = edge_value(p[0], p[1], p[2]);
        if signed_area.abs() < 1e-8 {
            continue;
        }
        let face = appearance.and_then(|a| a.triangles.get(triangle_index));
        let material = face
            .zip(appearance)
            .map(|(face, appearance)| &appearance.materials[face.material]);
        if let (Some(face), Some(material)) = (face, material) {
            let reverse = scene
                .reverse_winding
                .as_ref()
                .and_then(|v| v.get(triangle_index))
                .copied()
                .unwrap_or(face.reverse_winding);
            // Screen Y points down, so front-facing CCW glTF has negative area.
            if !material.double_sided && ((signed_area > 0.) != reverse) {
                continue;
            }
        }
        let normal = normalize(cross(
            subtract(triangle[1], triangle[0]),
            subtract(triangle[2], triangle[0]),
        ));
        let normal = [dot(normal, right), dot(normal, up), dot(normal, forward)];
        let light = (0.28 + 0.72 * dot(normal, light_direction).abs()) as f32;
        let neutral = [
            (70. + 66. * light) as u8,
            (130. + 70. * light) as u8,
            (149. + 74. * light) as u8,
            255,
        ];
        let lod = match (appearance, face, material.and_then(|m| m.texture)) {
            (Some(a), Some(f), Some(t)) => a.textures[t].lod(f.uv, p, signed_area),
            _ => 0.,
        };
        let min_x = p.iter().map(|v| v[0]).fold(f64::INFINITY, f64::min);
        let max_x = p.iter().map(|v| v[0]).fold(f64::NEG_INFINITY, f64::max);
        let min_y = p.iter().map(|v| v[1]).fold(f64::INFINITY, f64::min);
        let max_y = p.iter().map(|v| v[1]).fold(f64::NEG_INFINITY, f64::max);
        if max_x < 0. || max_y < 0. || min_x >= edge as f64 || min_y >= edge as f64 {
            continue;
        }
        let limit = edge as f64 - 1.;
        let (xmin, xmax) = (
            min_x.floor().clamp(0., limit) as usize,
            max_x.ceil().clamp(0., limit) as usize,
        );
        let (ymin, ymax) = (
            min_y.floor().clamp(0., limit) as usize,
            max_y.ceil().clamp(0., limit) as usize,
        );
        work += ((xmax - xmin + 1) * (ymax - ymin + 1)) as u64;
        ensure!(
            work <= MAX_RASTER_SAMPLES,
            "Model overlap exceeds the bounded raster preview budget"
        );
        for y in ymin..=ymax {
            if y % 16 == 0 {
                check()?;
            }
            for x in xmin..=xmax {
                let sample = [x as f64 + 0.5, y as f64 + 0.5, 0.];
                let raw = [
                    edge_value(p[1], p[2], sample),
                    edge_value(p[2], p[0], sample),
                    edge_value(p[0], p[1], sample),
                ];
                // A half-open top-left rule ensures shared triangle edges never
                // double-composite a transparent surface.
                if !(0..3)
                    .all(|i| covered_edge(raw[i], p[(i + 1) % 3], p[(i + 2) % 3], signed_area))
                {
                    continue;
                }
                let barycentric = raw.map(|value| value / signed_area);
                let z = (0..3).map(|i| barycentric[i] * p[i][2]).sum::<f64>();
                let pixel = y * size + x;
                if z <= depth[pixel] {
                    continue;
                }
                if let (Some(appearance), Some(face), Some(material)) = (appearance, face, material)
                {
                    let mut color = appearance.color(face, barycentric, lod, light);
                    match material.alpha {
                        AlphaMode::Mask(cutoff) if color[3] < cutoff => continue,
                        AlphaMode::Blend => {
                            color[3] = color[3].clamp(0., 1.);
                            if color[3] <= 0. {
                                continue;
                            }
                            ensure!(
                                fragments.len() < MAX_TRANSPARENT_FRAGMENTS,
                                "Transparent model exceeds the 2-million-fragment preview limit; use wireframe to inspect geometry"
                            );
                            if fragments.len() == fragments.capacity() {
                                let additional =
                                    (MAX_TRANSPARENT_FRAGMENTS - fragments.len()).min(16_384);
                                fragments.reserve_exact(additional);
                            }
                            let next = heads[pixel];
                            heads[pixel] = fragments.len() as u32;
                            fragments.push(Fragment {
                                depth: z,
                                color,
                                next,
                            });
                            continue;
                        }
                        _ => {}
                    }
                    depth[pixel] = z;
                    if has_blend {
                        linear[pixel].copy_from_slice(&color[..3]);
                    } else {
                        for c in 0..3 {
                            rgba[pixel * 4 + c] = linear_to_srgb(color[c]);
                        }
                    }
                } else {
                    depth[pixel] = z;
                    rgba[pixel * 4..pixel * 4 + 4].copy_from_slice(&neutral);
                    if has_blend {
                        linear[pixel] = [
                            srgb_to_linear(neutral[0]),
                            srgb_to_linear(neutral[1]),
                            srgb_to_linear(neutral[2]),
                        ];
                    }
                }
            }
        }
    }
    if has_blend {
        let mut sorted = [0u32; MAX_TRANSPARENT_LAYERS];
        for pixel in 0..size * size {
            if pixel.is_multiple_of(size * 16) {
                check()?;
            }
            let mut length = 0;
            let mut index = heads[pixel];
            while index != u32::MAX {
                let fragment = &fragments[index as usize];
                if fragment.depth > depth[pixel] {
                    ensure!(
                        length < MAX_TRANSPARENT_LAYERS,
                        "Transparent overlap exceeds the 32-layer-per-pixel preview limit; use wireframe to inspect geometry"
                    );
                    sorted[length] = index;
                    length += 1;
                }
                index = fragment.next;
            }
            sorted[..length].sort_unstable_by(|a, b| {
                fragments[*a as usize]
                    .depth
                    .total_cmp(&fragments[*b as usize].depth)
                    .then(a.cmp(b))
            });
            for &index in &sorted[..length] {
                let source = fragments[index as usize].color;
                for (c, value) in linear[pixel].iter_mut().enumerate() {
                    *value = source[c] * source[3] + *value * (1. - source[3]);
                }
            }
            for c in 0..3 {
                rgba[pixel * 4 + c] = linear_to_srgb(linear[pixel][c]);
            }
        }
    }
    check()?;
    Ok(ImagePreview {
        width: edge,
        height: edge,
        original_width: edge,
        original_height: edge,
        rgba,
        format: format!(
            "{} {}",
            scene.format,
            if wireframe { "wireframe" } else { "mesh" }
        ),
    })
}

const MAX_TRANSPARENT_FRAGMENTS: usize = 2_000_000;
const MAX_TRANSPARENT_LAYERS: usize = 32;
struct Fragment {
    depth: f64,
    color: [f32; 4],
    next: u32,
}
fn covered_edge(value: f64, mut a: Point, mut b: Point, area: f64) -> bool {
    let value = if area < 0. {
        std::mem::swap(&mut a, &mut b);
        -value
    } else {
        value
    };
    value > 0. || (value == 0. && (b[1] < a[1] || (b[1] == a[1] && b[0] > a[0])))
}

fn line(a: Point, b: Point, edge: usize, rgba: &mut [u8], work: &mut u64) -> Result<()> {
    // Clip before raster work: offscreen coordinates may be very large at zoom.
    let delta = subtract(b, a);
    let mut from: f64 = 0.;
    let mut to: f64 = 1.;
    for axis in 0..2 {
        if delta[axis].abs() < 1e-30 {
            if a[axis] < 0. || a[axis] > edge as f64 - 1. {
                return Ok(());
            }
        } else {
            let first = -a[axis] / delta[axis];
            let last = (edge as f64 - 1. - a[axis]) / delta[axis];
            from = from.max(first.min(last));
            to = to.min(first.max(last));
        }
    }
    if from > to {
        return Ok(());
    }
    let first = [a[0] + delta[0] * from, a[1] + delta[1] * from];
    let last = [a[0] + delta[0] * to, a[1] + delta[1] * to];
    let steps = ((last[0] - first[0])
        .abs()
        .max((last[1] - first[1]).abs())
        .ceil() as usize)
        .min(edge * 2)
        .max(1);
    *work += steps as u64 + 1;
    ensure!(
        *work <= MAX_RASTER_SAMPLES,
        "Model wireframe exceeds the bounded raster preview budget"
    );
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let x = (first[0] + (last[0] - first[0]) * t)
            .round()
            .clamp(0., edge as f64 - 1.) as usize;
        let y = (first[1] + (last[1] - first[1]) * t)
            .round()
            .clamp(0., edge as f64 - 1.) as usize;
        rgba[(y * edge + x) * 4..(y * edge + x) * 4 + 4].copy_from_slice(&[150, 214, 233, 255]);
    }
    Ok(())
}
