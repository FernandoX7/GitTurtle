//! Retained linear-light appearance. Texture resources contain only decoded
//! captured BIN bytes; samplers never resolve paths or network resources.
use anyhow::{Result, ensure};
use std::sync::Arc;

pub(super) const MAX_TEXTURE_BYTES: usize = 96 * 1024 * 1024;
pub(super) const MAX_IMAGE_PIXELS: usize = 4 * 1024 * 1024;
pub(super) const MAX_ATTRIBUTE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum AlphaMode {
    Opaque,
    Mask(f32),
    Blend,
}
#[derive(Clone, Debug)]
pub(super) struct Material {
    pub factor: [f32; 4],
    pub texture: Option<usize>,
    pub alpha: AlphaMode,
    pub double_sided: bool,
    pub unlit: bool,
}
impl Default for Material {
    fn default() -> Self {
        Self {
            factor: [1.; 4],
            texture: None,
            alpha: AlphaMode::Opaque,
            double_sided: false,
            unlit: false,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub(super) struct TriangleAppearance {
    pub material: usize,
    pub colors: [[f32; 4]; 3],
    pub uv: [[f32; 2]; 3],
    pub reverse_winding: bool,
}
#[derive(Debug)]
pub(super) struct SceneAppearance {
    pub triangles: Box<[TriangleAppearance]>,
    pub materials: Box<[Material]>,
    pub textures: Box<[Texture]>,
    retained_bytes: usize,
}
impl SceneAppearance {
    pub fn new(
        triangles: Box<[TriangleAppearance]>,
        materials: Box<[Material]>,
        textures: Box<[Texture]>,
    ) -> Self {
        let mut seen = std::collections::HashSet::new();
        let retained_bytes = std::mem::size_of::<Self>()
            + std::mem::size_of_val(triangles.as_ref())
            + std::mem::size_of_val(materials.as_ref())
            + std::mem::size_of_val(textures.as_ref())
            + textures
                .iter()
                .filter(|t| seen.insert(Arc::as_ptr(&t.image)))
                .map(|t| t.image.retained_bytes())
                .sum::<usize>();
        Self {
            triangles,
            materials,
            textures,
            retained_bytes,
        }
    }
    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
    pub fn color(
        &self,
        triangle: &TriangleAppearance,
        barycentric: [f64; 3],
        lod: f32,
        light: f32,
    ) -> [f32; 4] {
        let material = &self.materials[triangle.material];
        let mut color = std::array::from_fn(|c| {
            material.factor[c]
                * (0..3)
                    .map(|v| triangle.colors[v][c] * barycentric[v] as f32)
                    .sum::<f32>()
        });
        if let Some(index) = material.texture {
            let uv = std::array::from_fn(|c| {
                (0..3)
                    .map(|v| triangle.uv[v][c] * barycentric[v] as f32)
                    .sum()
            });
            let sampled = self.textures[index].sample(uv, lod);
            for c in 0..4 {
                color[c] *= sampled[c];
            }
        }
        if !material.unlit {
            for channel in &mut color[..3] {
                *channel *= light;
            }
        }
        color
    }
}
#[derive(Clone, Copy, Debug)]
pub(super) enum Wrap {
    Repeat,
    Mirror,
    Clamp,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Sampler {
    pub wrap: [Wrap; 2],
    pub mag_linear: bool,
    /// OpenGL/glTF filter enum; all six core minification modes are supported.
    pub min_filter: u32,
}
impl Default for Sampler {
    fn default() -> Self {
        Self {
            wrap: [Wrap::Repeat; 2],
            mag_linear: true,
            min_filter: 9987,
        }
    }
}
#[derive(Debug)]
pub(super) struct Texture {
    pub image: Arc<TextureImage>,
    pub sampler: Sampler,
}
#[derive(Debug)]
pub(super) struct TextureImage {
    pub levels: Box<[MipLevel]>,
}
#[derive(Debug)]
pub(super) struct MipLevel {
    pub width: usize,
    pub height: usize,
    pub pixels: Box<[[f32; 4]]>,
}
impl TextureImage {
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + std::mem::size_of_val(self.levels.as_ref())
            + self
                .levels
                .iter()
                .map(|l| std::mem::size_of_val(l.pixels.as_ref()))
                .sum::<usize>()
    }
    pub fn allocation_bytes(mut width: usize, mut height: usize) -> usize {
        let mut bytes = std::mem::size_of::<Self>();
        loop {
            bytes += width * height * 16 + std::mem::size_of::<MipLevel>();
            if width == 1 && height == 1 {
                break;
            }
            width = (width / 2).max(1);
            height = (height / 2).max(1);
        }
        bytes
    }
    pub fn from_rgba(
        width: usize,
        height: usize,
        rgba: &[u8],
        check: &impl Fn() -> Result<()>,
    ) -> Result<Self> {
        ensure!(
            width > 0
                && height > 0
                && width <= 4096
                && height <= 4096
                && width * height <= MAX_IMAGE_PIXELS,
            "GLB embedded image exceeds the 4-megapixel or 4096-edge limit"
        );
        ensure!(
            rgba.len() == width * height * 4,
            "Invalid GLB decoded image length"
        );
        let mut pixels = Vec::with_capacity(width * height);
        for (i, value) in rgba.as_chunks::<4>().0.iter().enumerate() {
            if i.is_multiple_of(4096) {
                check()?;
            }
            pixels.push([
                srgb_to_linear(value[0]),
                srgb_to_linear(value[1]),
                srgb_to_linear(value[2]),
                value[3] as f32 / 255.,
            ]);
        }
        let mut levels = vec![MipLevel {
            width,
            height,
            pixels: pixels.into_boxed_slice(),
        }];
        while levels.last().is_some_and(|l| l.width > 1 || l.height > 1) {
            check()?;
            let source = levels.last().unwrap();
            let (width, height) = ((source.width / 2).max(1), (source.height / 2).max(1));
            let mut pixels = Vec::with_capacity(width * height);
            for y in 0..height {
                if y.is_multiple_of(16) {
                    check()?;
                }
                for x in 0..width {
                    // Area integration keeps odd-sized and non-power-of-two mip
                    // levels unbiased; source/target ratios remain at most three.
                    let (x0, x1) = (
                        x as f64 * source.width as f64 / width as f64,
                        (x + 1) as f64 * source.width as f64 / width as f64,
                    );
                    let (y0, y1) = (
                        y as f64 * source.height as f64 / height as f64,
                        (y + 1) as f64 * source.height as f64 / height as f64,
                    );
                    let mut value = [0.; 4];
                    for sy in y0.floor() as usize..y1.ceil() as usize {
                        for sx in x0.floor() as usize..x1.ceil() as usize {
                            let weight = ((x1.min((sx + 1) as f64) - x0.max(sx as f64))
                                * (y1.min((sy + 1) as f64) - y0.max(sy as f64))
                                / ((x1 - x0) * (y1 - y0)))
                                as f32;
                            for (c, v) in value.iter_mut().enumerate() {
                                *v += source.pixels[sy.min(source.height - 1) * source.width
                                    + sx.min(source.width - 1)][c]
                                    * weight;
                            }
                        }
                    }
                    pixels.push(value);
                }
            }
            levels.push(MipLevel {
                width,
                height,
                pixels: pixels.into_boxed_slice(),
            });
        }
        Ok(Self {
            levels: levels.into_boxed_slice(),
        })
    }
}
impl Texture {
    pub fn lod(&self, uv: [[f32; 2]; 3], screen: [[f64; 3]; 3], area: f64) -> f32 {
        let source = &self.image.levels[0];
        let derivative = |channel: usize, axis: usize| {
            let a = (uv[1][channel] - uv[0][channel]) as f64;
            let b = (uv[2][channel] - uv[0][channel]) as f64;
            if axis == 0 {
                (a * (screen[2][1] - screen[0][1]) - b * (screen[1][1] - screen[0][1])) / area
            } else {
                (b * (screen[1][0] - screen[0][0]) - a * (screen[2][0] - screen[0][0])) / area
            }
        };
        let dx =
            (derivative(0, 0) * source.width as f64).hypot(derivative(1, 0) * source.height as f64);
        let dy =
            (derivative(0, 1) * source.width as f64).hypot(derivative(1, 1) * source.height as f64);
        dx.max(dy).max(1e-30).log2() as f32
    }
    pub fn sample(&self, uv: [f32; 2], lod: f32) -> [f32; 4] {
        let filter = self.sampler.min_filter;
        if lod <= 0. {
            return self.level_sample(0, uv, self.sampler.mag_linear);
        }
        if filter == 9728 || filter == 9729 {
            return self.level_sample(0, uv, filter == 9729);
        }
        let lod = lod.clamp(0., (self.image.levels.len() - 1) as f32);
        let linear = filter == 9985 || filter == 9987;
        if filter == 9984 || filter == 9985 {
            return self.level_sample(lod.round() as usize, uv, linear);
        }
        let lower = lod.floor() as usize;
        let upper = (lower + 1).min(self.image.levels.len() - 1);
        mix(
            self.level_sample(lower, uv, linear),
            self.level_sample(upper, uv, linear),
            lod.fract(),
        )
    }
    fn level_sample(&self, level: usize, uv: [f32; 2], linear: bool) -> [f32; 4] {
        let level = &self.image.levels[level];
        let position = [
            uv[0] as f64 * level.width as f64,
            uv[1] as f64 * level.height as f64,
        ];
        let texel = |x: i64, y: i64| {
            level.pixels[wrap_index(y, level.height, self.sampler.wrap[1]) * level.width
                + wrap_index(x, level.width, self.sampler.wrap[0])]
        };
        if !linear {
            return texel(position[0].floor() as i64, position[1].floor() as i64);
        }
        let (x, y) = (position[0] - 0.5, position[1] - 0.5);
        let (ix, iy) = (x.floor() as i64, y.floor() as i64);
        mix(
            mix(texel(ix, iy), texel(ix + 1, iy), (x - x.floor()) as f32),
            mix(
                texel(ix, iy + 1),
                texel(ix + 1, iy + 1),
                (x - x.floor()) as f32,
            ),
            (y - y.floor()) as f32,
        )
    }
}
fn wrap_index(index: i64, size: usize, wrap: Wrap) -> usize {
    let size = size as i64;
    match wrap {
        Wrap::Clamp => index.clamp(0, size - 1) as usize,
        Wrap::Repeat => index.rem_euclid(size) as usize,
        Wrap::Mirror => {
            let p = index.rem_euclid(size * 2);
            (if p < size { p } else { size * 2 - p - 1 }) as usize
        }
    }
}
fn mix(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    std::array::from_fn(|i| a[i] * (1. - t) + b[i] * t)
}
pub(super) fn srgb_to_linear(byte: u8) -> f32 {
    let v = byte as f32 / 255.;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
pub(super) fn linear_to_srgb(v: f32) -> u8 {
    let v = v.clamp(0., 1.);
    let s = if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1. / 2.4) - 0.055
    };
    (s * 255.).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model3d::{
        ModelCamera, ModelScene, ModelStandardView, ModelUnits, decode_geometry, render_model,
    };

    fn texture() -> Texture {
        Texture {
            image: Arc::new(
                TextureImage::from_rgba(2, 1, &[255, 0, 0, 255, 0, 255, 0, 0], &|| Ok(())).unwrap(),
            ),
            sampler: Sampler {
                mag_linear: false,
                min_filter: 9728,
                ..Sampler::default()
            },
        }
    }
    #[test]
    fn sampler_wrap_filter_and_linear_mips_match_expected_samples() {
        let mut texture = texture();
        assert_eq!(texture.sample([1.25, 0.5], 0.), [1., 0., 0., 1.]);
        assert_eq!(texture.sample([-0.25, 0.5], 0.), [0., 1., 0., 0.]);
        texture.sampler.wrap[0] = Wrap::Mirror;
        assert_eq!(texture.sample([1.25, 0.5], 0.), [0., 1., 0., 0.]);
        assert_eq!(texture.sample([-0.25, 0.5], 0.), [1., 0., 0., 1.]);
        texture.sampler.wrap[0] = Wrap::Clamp;
        assert_eq!(texture.sample([100., 0.5], 0.), [0., 1., 0., 0.]);
        texture.sampler.mag_linear = true;
        assert_eq!(texture.sample([0.5, 0.5], 0.), [0.5, 0.5, 0., 0.5]);
        for min_filter in 9984..=9987 {
            texture.sampler.min_filter = min_filter;
            assert_eq!(texture.sample([0.2, 0.5], 10.), [0.5, 0.5, 0., 0.5]);
        }
        assert_eq!(linear_to_srgb(0.5), 188);
        for value in 0..=255 {
            assert_eq!(linear_to_srgb(srgb_to_linear(value)), value);
        }
        let odd = TextureImage::from_rgba(
            3,
            1,
            &[255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255],
            &|| Ok(()),
        )
        .unwrap();
        assert!((odd.levels[1].pixels[0][0] - 1. / 3.).abs() < 1e-6);
    }

    fn quad(y: f64) -> Vec<[[f64; 3]; 3]> {
        vec![
            [[-1., y, -1.], [1., y, -1.], [1., y, 1.]],
            [[-1., y, -1.], [1., y, 1.], [-1., y, 1.]],
        ]
    }
    fn scene(layers: &[(f64, [f32; 4], AlphaMode)]) -> ModelScene {
        let mut geometry = Vec::new();
        let mut faces = Vec::new();
        let mut materials = Vec::new();
        for &(y, factor, alpha) in layers {
            let material = materials.len();
            materials.push(Material {
                factor,
                alpha,
                unlit: true,
                ..Material::default()
            });
            geometry.extend(quad(y));
            faces.extend(
                [TriangleAppearance {
                    material,
                    colors: [[1.; 4]; 3],
                    uv: [[0.; 2]; 3],
                    reverse_winding: false,
                }; 2],
            );
        }
        let mut scene = ModelScene::new(geometry, ModelUnits::Unknown, "test").unwrap();
        scene.set_appearance(Arc::new(SceneAppearance::new(
            faces.into_boxed_slice(),
            materials.into_boxed_slice(),
            Box::new([]),
        )));
        scene
    }
    fn camera() -> ModelCamera {
        let mut camera = ModelCamera {
            target: [0.; 3],
            span: 2.,
            yaw: 0.,
            pitch: 0.,
        };
        camera.set_view(ModelStandardView::Front);
        camera
    }
    fn pixel(image: &crate::ImagePreview, x: usize, y: usize) -> [u8; 4] {
        image.rgba[(y * image.width as usize + x) * 4..][..4]
            .try_into()
            .unwrap()
    }

    #[test]
    fn transparent_surfaces_composite_in_linear_depth_order_without_shared_edge_seams() {
        let layers = [
            (0., [1., 0., 0., 0.5], AlphaMode::Blend),
            (0.5, [0., 1., 0., 0.5], AlphaMode::Blend),
            (1., [0., 0., 1., 0.], AlphaMode::Opaque),
        ];
        let first = render_model(&scene(&layers), &camera(), 64, false, || Ok(())).unwrap();
        let mut reversed = layers;
        reversed.reverse();
        let second = render_model(&scene(&reversed), &camera(), 64, false, || Ok(())).unwrap();
        assert_eq!(first.rgba, second.rgba);
        let expected = [
            linear_to_srgb(0.5),
            linear_to_srgb(0.25),
            linear_to_srgb(0.25),
            255,
        ];
        assert_eq!(pixel(&first, 32, 32), expected);
        assert_eq!(pixel(&first, 31, 32), expected);
        assert_eq!(pixel(&first, 33, 32), expected);
    }

    #[test]
    fn intersecting_transparent_triangles_sort_at_each_pixel() {
        let mut model = ModelScene::new(
            vec![
                [[-1., -1., -1.], [1., 1., -1.], [0., 0., 1.]],
                [[-1., 0., -1.], [1., 0., -1.], [0., 0., 1.]],
            ],
            ModelUnits::Unknown,
            "crossing",
        )
        .unwrap();
        model.set_appearance(Arc::new(SceneAppearance::new(
            vec![
                TriangleAppearance {
                    material: 0,
                    colors: [[1.; 4]; 3],
                    uv: [[0.; 2]; 3],
                    reverse_winding: false,
                },
                TriangleAppearance {
                    material: 1,
                    colors: [[1.; 4]; 3],
                    uv: [[0.; 2]; 3],
                    reverse_winding: false,
                },
            ]
            .into_boxed_slice(),
            vec![
                Material {
                    factor: [1., 0., 0., 0.5],
                    alpha: AlphaMode::Blend,
                    unlit: true,
                    ..Material::default()
                },
                Material {
                    factor: [0., 1., 0., 0.5],
                    alpha: AlphaMode::Blend,
                    unlit: true,
                    ..Material::default()
                },
            ]
            .into_boxed_slice(),
            Box::new([]),
        )));
        let output = render_model(&model, &camera(), 64, false, || Ok(())).unwrap();
        let left = pixel(&output, 16, 48);
        let right = pixel(&output, 48, 48);
        assert!(left[0] > left[1], "{left:?}");
        assert!(right[1] > right[0], "{right:?}");
    }

    #[test]
    fn alpha_masks_ignore_rejected_depth_and_opaque_ignores_alpha() {
        let output = render_model(
            &scene(&[
                (0., [1., 0., 0., 0.49], AlphaMode::Mask(0.5)),
                (1., [0., 1., 0., 0.], AlphaMode::Opaque),
            ]),
            &camera(),
            64,
            false,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(pixel(&output, 32, 32), [0, 255, 0, 255]);
        let output = render_model(
            &scene(&[(0., [1., 0., 0., 0.5], AlphaMode::Mask(0.5))]),
            &camera(),
            64,
            false,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(pixel(&output, 32, 32), [255, 0, 0, 255]);
    }

    #[test]
    fn culling_double_sided_and_mirrored_winding_are_independent() {
        let mut model = scene(&[(0., [1., 0., 0., 1.], AlphaMode::Opaque)]);
        let mut reverse = camera();
        reverse.set_view(ModelStandardView::Back);
        let invisible = render_model(&model, &reverse, 64, false, || Ok(())).unwrap();
        assert_eq!(pixel(&invisible, 32, 32), [28, 34, 42, 255]);
        model.set_reverse_winding(vec![true; 2]);
        let visible = render_model(&model, &reverse, 64, false, || Ok(())).unwrap();
        assert_eq!(pixel(&visible, 32, 32), [255, 0, 0, 255]);
        model.set_appearance(Arc::new(SceneAppearance::new(
            vec![
                TriangleAppearance {
                    material: 0,
                    colors: [[1.; 4]; 3],
                    uv: [[0.; 2]; 3],
                    reverse_winding: false
                };
                2
            ]
            .into_boxed_slice(),
            vec![Material {
                factor: [1., 0., 0., 1.],
                double_sided: true,
                unlit: true,
                ..Material::default()
            }]
            .into_boxed_slice(),
            Box::new([]),
        )));
        let visible = render_model(&model, &camera(), 64, false, || Ok(())).unwrap();
        assert_eq!(pixel(&visible, 32, 32), [255, 0, 0, 255]);
    }

    #[test]
    fn transparency_and_texture_work_are_bounded_and_cancellable() {
        let layers: Vec<_> = (0..33)
            .map(|i| (i as f64 / 100., [1., 1., 1., 0.1], AlphaMode::Blend))
            .collect();
        let error = render_model(&scene(&layers), &camera(), 64, false, || Ok(())).unwrap_err();
        assert!(error.to_string().contains("32-layer"), "{error}");
        let checks = std::cell::Cell::new(0);
        let error = TextureImage::from_rgba(128, 128, &vec![255; 128 * 128 * 4], &|| {
            checks.set(checks.get() + 1);
            ensure!(checks.get() < 3, "cancelled");
            Ok(())
        })
        .unwrap_err();
        assert_eq!(error.to_string(), "cancelled");
        assert!(
            TextureImage::from_rgba(4096, 4096, &[], &|| Ok(()))
                .unwrap_err()
                .to_string()
                .contains("4-megapixel")
        );
    }

    #[test]
    fn material_and_texture_only_public_revisions_change_pixels_without_changing_geometry() {
        for (before, after) in [
            (
                include_bytes!("../../tests/fixtures/models/glb/workflow/material-before.glb")
                    .as_slice(),
                include_bytes!("../../tests/fixtures/models/glb/workflow/material-after.glb")
                    .as_slice(),
            ),
            (
                include_bytes!("../../tests/fixtures/models/glb/workflow/texture-before.glb")
                    .as_slice(),
                include_bytes!("../../tests/fixtures/models/glb/workflow/texture-after.glb")
                    .as_slice(),
            ),
        ] {
            let before = decode_geometry(before, "before.glb", || Ok(())).unwrap();
            let after = decode_geometry(after, "after.glb", || Ok(())).unwrap();
            assert_eq!(before.scene.triangles(), after.scene.triangles());
            assert_eq!(before.scene.bounds, after.scene.bounds);
            let mut camera = ModelCamera::fit(before.scene.bounds);
            camera.set_view(ModelStandardView::Front);
            let before = render_model(&before.scene, &camera, 64, false, || Ok(())).unwrap();
            let after = render_model(&after.scene, &camera, 64, false, || Ok(())).unwrap();
            assert_ne!(before.rgba, after.rgba);
        }
    }

    #[test]
    fn malformed_or_external_textures_preserve_geometry_with_disclosure() {
        for bytes in [
            include_bytes!("../../tests/fixtures/models/glb/workflow/malformed-texture.glb")
                .as_slice(),
            include_bytes!("../../tests/fixtures/models/glb/workflow/external-texture.glb")
                .as_slice(),
        ] {
            let geometry = decode_geometry(bytes, "appearance.glb", || Ok(())).unwrap();
            assert_eq!(geometry.scene.triangle_count(), 2);
            assert!(
                geometry
                    .details
                    .iter()
                    .any(|s| s.contains("texture omitted")),
                "{:?}",
                geometry.details
            );
        }
    }
}
