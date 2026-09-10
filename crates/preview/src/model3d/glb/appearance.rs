//! Appearance failures are disclosed while independently valid geometry survives.
//! No resource URI is ever resolved, including data: images.
use super::super::appearance::{
    AlphaMode, MAX_ATTRIBUTE_BYTES, MAX_IMAGE_PIXELS, MAX_TEXTURE_BYTES, Material, Sampler,
    SceneAppearance, Texture, TextureImage, TriangleAppearance, Wrap,
};
use super::*;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use serde_json::Value;
use std::{io::Cursor, sync::Arc};

#[derive(Default)]
pub(super) struct Builder {
    materials: Vec<Material>,
    material_map: HashMap<Option<usize>, (usize, usize)>,
    textures: Vec<Texture>,
    texture_map: HashMap<usize, usize>,
    images: HashMap<usize, Arc<TextureImage>>,
    failed_textures: HashSet<usize>,
    failed_images: HashSet<usize>,
    texture_bytes: usize,
    attribute_bytes: usize,
    warnings: Vec<String>,
    has_unlit: bool,
    has_lit: bool,
}
impl Builder {
    fn warn(&mut self, warning: impl Into<String>) {
        let warning = warning.into();
        if self.warnings.len() < 32 && !self.warnings.contains(&warning) {
            self.warnings.push(warning);
        }
    }
    pub(super) fn finish(
        self,
        triangles: Vec<TriangleAppearance>,
    ) -> (Arc<SceneAppearance>, Vec<String>) {
        let mut details = Vec::new();
        if self.has_unlit {
            details.push("Unlit materials use linear base color × vertex color × embedded texture, with authored alpha and sidedness; no tone mapping.".into());
        }
        if self.has_lit {
            details.push("Core PBR materials are base-color inspection with simplified flat diffuse lighting: metallic/roughness, authored normals, normal/occlusion/emissive maps, lights and environment shading are not reproduced.".into());
        }
        details.extend(self.warnings);
        details.push(format!("Captured PNG/JPEG only · {} texture sampler(s), {:.2} MiB retained linear texture mipmaps · external resources are never loaded", self.textures.len(), self.texture_bytes as f64 / (1024. * 1024.)));
        (
            Arc::new(SceneAppearance::new(
                triangles.into_boxed_slice(),
                self.materials.into_boxed_slice(),
                self.textures.into_boxed_slice(),
            )),
            details,
        )
    }
    fn material(
        &mut self,
        index: Option<usize>,
        document: &Document,
        bin: Option<&[u8]>,
        check: &impl Fn() -> Result<()>,
    ) -> Result<(usize, usize)> {
        if let Some(&cached) = self.material_map.get(&index) {
            return Ok(cached);
        }
        check()?;
        let decoded = (|| -> Result<(Material, usize)> {
            let Some(index) = index else {
                return Ok((Material::default(), 0));
            };
            let source = document.materials.get(index).context("missing material")?;
            ensure!(source.is_object(), "material must be an object");
            let mut result = Material::default();
            if let Some(pbr) = source.get("pbrMetallicRoughness") {
                ensure!(pbr.is_object(), "pbrMetallicRoughness must be an object");
                if let Some(factor) = pbr.get("baseColorFactor") {
                    result.factor = vector::<4>(factor, true)?;
                }
            }
            result.alpha = match source
                .get("alphaMode")
                .map(|v| v.as_str().context("alphaMode must be a string"))
                .transpose()?
                .unwrap_or("OPAQUE")
            {
                "OPAQUE" => AlphaMode::Opaque,
                "BLEND" => AlphaMode::Blend,
                "MASK" => {
                    let cutoff = number(source, "alphaCutoff", 0.5)?;
                    ensure!(cutoff >= 0., "invalid alpha cutoff");
                    AlphaMode::Mask(cutoff)
                }
                _ => bail!("unknown alpha mode"),
            };
            result.double_sided = source
                .get("doubleSided")
                .map(|v| v.as_bool().context("doubleSided must be boolean"))
                .transpose()?
                .unwrap_or(false);
            if let Some(extensions) = source.get("extensions") {
                let extensions = extensions
                    .as_object()
                    .context("material extensions must be an object")?;
                if let Some(unlit) = extensions.get("KHR_materials_unlit") {
                    ensure!(unlit.is_object(), "invalid KHR_materials_unlit");
                    result.unlit = true;
                }
                for name in extensions.keys() {
                    if name != "KHR_materials_unlit" {
                        self.warn(format!(
                            "Appearance approximation: material extension {name} is omitted."
                        ));
                    }
                }
            }
            let mut texcoord = 0;
            if let Some(info) = source.pointer("/pbrMetallicRoughness/baseColorTexture") {
                let texture_result = (|| -> Result<usize> {
                    ensure!(info.is_object(), "baseColorTexture must be an object");
                    // Applying unchanged UVs to transformed textures would make a
                    // plausible but wrong appearance. Omit only this resource.
                    ensure!(
                        info.get("extensions")
                            .is_none_or(|v| v.as_object().is_some_and(|m| m.is_empty())),
                        "texture coordinate extensions are unsupported"
                    );
                    texcoord = optional_index(info, "texCoord")?.unwrap_or(0);
                    let texture = required_index(info, "index")?;
                    self.texture(texture, document, bin, check)
                })();
                match texture_result {
                    Ok(texture) => result.texture = Some(texture),
                    Err(error) => {
                        check()?;
                        self.warn(format!("Base-color texture omitted: {error}. Geometry, factor and valid vertex colors remain available."));
                    }
                }
            }
            Ok((result, texcoord))
        })();
        let (material, texcoord) = match decoded {
            Ok(value) => value,
            Err(error) => {
                check()?;
                self.warn(format!("Malformed appearance material omitted: {error}. Neutral double-sided geometry is shown."));
                (
                    Material {
                        double_sided: true,
                        ..Material::default()
                    },
                    0,
                )
            }
        };
        self.has_unlit |= material.unlit;
        self.has_lit |= !material.unlit;
        let result = (self.materials.len(), texcoord);
        self.materials.push(material);
        self.material_map.insert(index, result);
        Ok(result)
    }
    fn texture(
        &mut self,
        index: usize,
        document: &Document,
        bin: Option<&[u8]>,
        check: &impl Fn() -> Result<()>,
    ) -> Result<usize> {
        if let Some(&cached) = self.texture_map.get(&index) {
            return Ok(cached);
        }
        ensure!(
            !self.failed_textures.contains(&index),
            "previously invalid or unsupported embedded texture"
        );
        self.failed_textures.insert(index);
        let source = document.textures.get(index).context("missing texture")?;
        ensure!(source.is_object(), "texture must be an object");
        if source
            .get("extensions")
            .is_some_and(|v| !v.as_object().is_some_and(|m| m.is_empty()))
        {
            self.warn("Texture extensions are omitted; a supplied core PNG/JPEG fallback is used when available.");
        }
        let image_index = required_index(source, "source")?;
        let sampler = match optional_index(source, "sampler")? {
            Some(index) => parse_sampler(document.samplers.get(index).context("missing sampler")?)?,
            None => Sampler::default(),
        };
        let image = if let Some(image) = self.images.get(&image_index) {
            image.clone()
        } else {
            ensure!(
                !self.failed_images.contains(&image_index),
                "previously invalid or unsupported embedded image"
            );
            self.failed_images.insert(image_index);
            let image = document.images.get(image_index).context("missing image")?;
            ensure!(image.is_object(), "image must be an object");
            ensure!(
                image.get("uri").is_none(),
                "external and data-URI images are not loaded"
            );
            let format = match image.get("mimeType").and_then(Value::as_str) {
                Some("image/png") => ImageFormat::Png,
                Some("image/jpeg") => ImageFormat::Jpeg,
                _ => bail!("only embedded PNG/JPEG images are supported"),
            };
            let source = view(document, required_index(image, "bufferView")?)?;
            validate_view(document, source)?;
            ensure!(
                source.extensions.meshopt.is_none()
                    && source.byte_stride.is_none()
                    && source.target.is_none(),
                "image buffer view must be uncompressed and tightly packed without a GPU target"
            );
            ensure!(
                source.buffer == 0 && document.buffers.first().is_some_and(|b| b.uri.is_none()),
                "image requires supplied BIN buffer 0"
            );
            let end = source
                .byte_offset
                .checked_add(source.byte_length)
                .context("image range overflow")?;
            let bytes = bin
                .context("missing image BIN data")?
                .get(source.byte_offset..end)
                .context("image exceeds captured BIN data")?;
            ensure!(
                image::guess_format(bytes).ok() == Some(format),
                "image MIME type disagrees with encoded format"
            );
            let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(4096);
            limits.max_image_height = Some(4096);
            limits.max_alloc = Some(32 * 1024 * 1024);
            reader.limits(limits);
            check()?;
            let decoder = reader
                .into_decoder()
                .context("cannot read embedded image header")?;
            let (width, height) = decoder.dimensions();
            ensure!(
                width > 0
                    && height > 0
                    && width as u64 * height as u64 <= MAX_IMAGE_PIXELS as u64
                    && decoder.total_bytes() <= 32 * 1024 * 1024,
                "embedded image exceeds the 4-megapixel decoded image limit"
            );
            let bytes = TextureImage::allocation_bytes(width as usize, height as usize);
            ensure!(
                bytes <= MAX_TEXTURE_BYTES.saturating_sub(self.texture_bytes),
                "embedded texture mipmaps exceed the 96 MiB retained texture limit"
            );
            // glTF ignores color profiles/orientation: base-color RGB is sRGB,
            // alpha is linear, and UV zero addresses the encoded top image row.
            let pixels = DynamicImage::from_decoder(decoder)
                .context("cannot decode embedded image")?
                .into_rgba8();
            check()?;
            let image = Arc::new(TextureImage::from_rgba(
                width as usize,
                height as usize,
                pixels.as_raw(),
                check,
            )?);
            self.texture_bytes += image.retained_bytes();
            self.failed_images.remove(&image_index);
            self.images.insert(image_index, image.clone());
            image
        };
        let result = self.textures.len();
        self.textures.push(Texture { image, sampler });
        self.texture_map.insert(index, result);
        self.failed_textures.remove(&index);
        Ok(result)
    }
}
fn required_index(value: &Value, key: &str) -> Result<usize> {
    optional_index(value, key)?.with_context(|| format!("missing {key}"))
}
fn optional_index(value: &Value, key: &str) -> Result<Option<usize>> {
    value
        .get(key)
        .map(|v| {
            v.as_u64()
                .and_then(|v| usize::try_from(v).ok())
                .with_context(|| format!("invalid {key} index"))
        })
        .transpose()
}
fn number(value: &Value, key: &str, default: f32) -> Result<f32> {
    let n = value
        .get(key)
        .map(|v| v.as_f64().with_context(|| format!("invalid {key}")))
        .transpose()?
        .unwrap_or(default as f64);
    ensure!(
        n.is_finite() && n.abs() <= f32::MAX as f64,
        "unreasonable {key}"
    );
    Ok(n as f32)
}
fn vector<const N: usize>(value: &Value, unit: bool) -> Result<[f32; N]> {
    let values = value.as_array().context("invalid appearance vector")?;
    ensure!(values.len() == N, "invalid appearance vector length");
    let mut output = [0.; N];
    for (target, value) in output.iter_mut().zip(values) {
        let value = value
            .as_f64()
            .context("appearance vector contains non-number")?;
        ensure!(
            value.is_finite() && (!unit || (0. ..=1.).contains(&value)),
            "appearance factor outside [0,1]"
        );
        *target = value as f32;
    }
    Ok(output)
}
fn parse_sampler(value: &Value) -> Result<Sampler> {
    ensure!(value.is_object(), "sampler must be an object");
    let wrap = |name| -> Result<Wrap> {
        match optional_index(value, name)?.unwrap_or(10497) {
            10497 => Ok(Wrap::Repeat),
            33648 => Ok(Wrap::Mirror),
            33071 => Ok(Wrap::Clamp),
            _ => bail!("invalid texture wrap mode"),
        }
    };
    let mag = optional_index(value, "magFilter")?.unwrap_or(9729);
    let min = optional_index(value, "minFilter")?.unwrap_or(9987);
    ensure!(
        matches!(mag, 9728 | 9729) && matches!(min, 9728 | 9729 | 9984..=9987),
        "invalid texture filtering mode"
    );
    Ok(Sampler {
        wrap: [wrap("wrapS")?, wrap("wrapT")?],
        mag_linear: mag == 9729,
        min_filter: min as u32,
    })
}
impl<F: Fn() -> Result<()>> Decoder<'_, F> {
    pub(super) fn primitive_appearance(
        &mut self,
        primitive: &Primitive,
        indices: &[usize],
        vertex_count: usize,
    ) -> Result<Vec<TriangleAppearance>> {
        let (mut material, texcoord) =
            self.appearance
                .material(primitive.material, self.document, self.bin, self.check)?;
        let colors = if let Some(&index) = primitive.attributes.get("COLOR_0") {
            match self.appearance_attribute(index, vertex_count, true) {
                Ok(values) => Some(values),
                Err(error) => {
                    (self.check)()?;
                    self.appearance
                        .warn(format!("Vertex COLOR_0 omitted: {error}."));
                    None
                }
            }
        } else {
            None
        };
        let uv = if self.appearance.materials[material].texture.is_some() {
            match primitive
                .attributes
                .get(&format!("TEXCOORD_{texcoord}"))
                .copied()
                .context("material requires missing texture coordinates")
                .and_then(|index| self.appearance_attribute(index, vertex_count, false))
            {
                Ok(values) => Some(values),
                Err(error) => {
                    (self.check)()?;
                    self.appearance
                        .warn(format!("Base-color texture omitted: {error}."));
                    let mut fallback = self.appearance.materials[material].clone();
                    fallback.texture = None;
                    material = self.appearance.materials.len();
                    self.appearance.materials.push(fallback);
                    None
                }
            }
        } else {
            None
        };
        let mut output = Vec::with_capacity(indices.len() / 3);
        for (i, indices) in indices.as_chunks::<3>().0.iter().enumerate() {
            if i.is_multiple_of(256) {
                (self.check)()?;
            }
            output.push(TriangleAppearance {
                material,
                colors: indices.map(|i| colors.as_ref().map_or([1.; 4], |values| values[i])),
                uv: indices.map(|i| {
                    uv.as_ref()
                        .map_or([0.; 2], |values| [values[i][0], values[i][1]])
                }),
                reverse_winding: false,
            });
        }
        Ok(output)
    }
    fn appearance_attribute(
        &mut self,
        index: usize,
        vertex_count: usize,
        color: bool,
    ) -> Result<Vec<[f32; 4]>> {
        self.prepare_accessor(index)?;
        let accessor = self
            .document
            .accessors
            .get(index)
            .context("missing appearance accessor")?;
        ensure!(
            accessor.count == vertex_count,
            "appearance attribute count differs from POSITION"
        );
        let channels = match (color, accessor.kind.as_str()) {
            (true, "VEC3") => 3,
            (true, "VEC4") => 4,
            (false, "VEC2") => 2,
            _ => bail!("invalid appearance accessor vector type"),
        };
        ensure!(
            (accessor.component_type == 5126 && !accessor.normalized)
                || (matches!(accessor.component_type, 5121 | 5123) && accessor.normalized)
                || (!color && self.quantized && matches!(accessor.component_type, 5120..=5123)),
            "appearance accessor requires FLOAT or normalized unsigned BYTE/SHORT (additional UV integers require KHR_mesh_quantization)"
        );
        let cost = accessor
            .count
            .checked_mul(16)
            .context("appearance attribute size overflow")?;
        ensure!(
            cost <= MAX_ATTRIBUTE_BYTES.saturating_sub(self.appearance.attribute_bytes),
            "appearance attributes exceed the 16 MiB decoded work limit"
        );
        self.appearance.attribute_bytes += cost;
        let size = element_size(accessor)?;
        if let Some(index) = accessor.buffer_view {
            let source = view(self.document, index)?;
            ensure!(
                source.target != Some(34963)
                    && accessor.byte_offset.is_multiple_of(4)
                    && (source.byte_offset + accessor.byte_offset).is_multiple_of(4)
                    && source.byte_stride.unwrap_or(size).is_multiple_of(4),
                "appearance vertex attributes require four-byte alignment"
            );
        }
        let sparse = self.sparse_indices(accessor)?;
        let read = |bytes: &[u8], offset: usize| -> Result<[f32; 4]> {
            let component = component_size(accessor.component_type)?;
            let mut output = [0., 0., 0., 1.];
            for (i, target) in output[..channels].iter_mut().enumerate() {
                let start = offset
                    .checked_add(i * component)
                    .context("appearance offset overflow")?;
                let bytes = bytes
                    .get(start..start + component)
                    .context("truncated appearance component")?;
                let (raw, divisor, signed) = match accessor.component_type {
                    5120 => (bytes[0] as i8 as f32, 127., true),
                    5121 => (bytes[0] as f32, 255., false),
                    5122 => (
                        i16::from_le_bytes(bytes.try_into().unwrap()) as f32,
                        32767.,
                        true,
                    ),
                    5123 => (
                        u16::from_le_bytes(bytes.try_into().unwrap()) as f32,
                        65535.,
                        false,
                    ),
                    5126 => (f32::from_le_bytes(bytes.try_into().unwrap()), 1., false),
                    _ => bail!("unsupported appearance component"),
                };
                *target = if accessor.normalized {
                    if signed {
                        (raw / divisor).max(-1.)
                    } else {
                        raw / divisor
                    }
                } else {
                    raw
                };
                ensure!(
                    target.is_finite()
                        && target.abs() <= 1e6
                        && (!color || (0. ..=1.).contains(target)),
                    "nonfinite, unreasonable, or out-of-range appearance attribute"
                );
            }
            Ok(output)
        };
        let mut values = vec![[0., 0., 0., if channels == 3 { 1. } else { 0. }]; accessor.count];
        if let Some(index) = accessor.buffer_view {
            let source = view(self.document, index)?;
            let bytes = self.data(index)?;
            let stride = source.byte_stride.unwrap_or(size);
            for (i, value) in values.iter_mut().enumerate() {
                if i.is_multiple_of(256) {
                    (self.check)()?;
                }
                *value = read(bytes, accessor.byte_offset + i * stride)?;
            }
        }
        if let Some(sparse_source) = &accessor.sparse {
            let bytes = self.data(sparse_source.values.buffer_view)?;
            for (i, &index) in sparse.iter().enumerate() {
                if i.is_multiple_of(256) {
                    (self.check)()?;
                }
                values[index] = read(bytes, sparse_source.values.byte_offset + i * size)?;
            }
        }
        Ok(values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model3d::decode_geometry;

    fn mutate(bytes: &[u8], edit: impl FnOnce(&mut Value)) -> Vec<u8> {
        let (json, bin) = container(bytes, &|| Ok(())).unwrap();
        let mut document: Value = serde_json::from_slice(json).unwrap();
        edit(&mut document);
        let mut json = serde_json::to_vec(&document).unwrap();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let bin = bin.unwrap();
        let mut output = b"glTF".to_vec();
        output.extend_from_slice(&2u32.to_le_bytes());
        output.extend_from_slice(&((28 + json.len() + bin.len()) as u32).to_le_bytes());
        output.extend_from_slice(&(json.len() as u32).to_le_bytes());
        output.extend_from_slice(&JSON_CHUNK.to_le_bytes());
        output.extend(json);
        output.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        output.extend_from_slice(&BIN_CHUNK.to_le_bytes());
        output.extend(bin);
        output
    }
    const TEXTURE: &[u8] =
        include_bytes!("../../../tests/fixtures/models/glb/workflow/texture-before.glb");

    #[test]
    fn invalid_resource_ranges_and_uv_layouts_preserve_valid_geometry() {
        for bytes in [
            mutate(TEXTURE, |v| {
                let view = v["images"][0]["bufferView"].as_u64().unwrap() as usize;
                v["bufferViews"][view]["byteLength"] = serde_json::json!(999999999);
            }),
            mutate(TEXTURE, |v| {
                let uv = v["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_0"]
                    .as_u64()
                    .unwrap() as usize;
                v["accessors"][uv]["bufferView"] = serde_json::json!(99999);
            }),
            mutate(TEXTURE, |v| {
                let uv = v["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_0"]
                    .as_u64()
                    .unwrap() as usize;
                v["accessors"][uv]["count"] = serde_json::json!(2);
            }),
            mutate(TEXTURE, |v| {
                v["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"] =
                    serde_json::json!([-1, 1, 1, 1]);
            }),
            mutate(TEXTURE, |v| {
                v["meshes"][0]["primitives"][0]["material"] = serde_json::json!(99999);
            }),
        ] {
            let geometry = decode_geometry(&bytes, "malformed-appearance.glb", || Ok(())).unwrap();
            assert_eq!(geometry.scene.triangle_count(), 2);
            assert!(
                geometry.details.iter().any(|s| s.contains("omitted")),
                "{:?}",
                geometry.details
            );
        }
    }

    #[test]
    fn appearance_fallback_never_swallows_cancellation() {
        let calls = std::cell::Cell::new(0);
        decode_geometry(TEXTURE, "texture.glb", || {
            calls.set(calls.get() + 1);
            Ok(())
        })
        .unwrap();
        let total = calls.get();
        for stop in 1..=total {
            calls.set(0);
            let result = decode_geometry(TEXTURE, "texture.glb", || {
                calls.set(calls.get() + 1);
                ensure!(calls.get() < stop, "cancel appearance fixture");
                Ok(())
            });
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("cancel appearance fixture"),
                "checkpoint {stop} was swallowed"
            );
        }
    }
}
