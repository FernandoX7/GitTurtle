//! Bounded supplied-byte GLB geometry, appearance, deformation and animation.
//! This module has no resource resolver; external resources are never loaded.

use super::{MAX_MODEL_TRIANGLES, MAX_OBJECTS, MAX_VERTICES, Point, Triangle, valid_point};
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};

mod appearance;
mod compression;
mod deformation;
use compression::{BufferExtensions, DecodedView, ViewExtensions};
pub(super) use deformation::GlbAnimation;
pub use deformation::ModelAnimationClip;
use deformation::{RetainedMesh, Skin};
use std::sync::Arc;

const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;
const MAX_JSON_TOKENS: usize = 250_000;
const MAX_JSON_DEPTH: usize = 32;
const MAX_JSON_ARRAY: usize = 16_384;
const MAX_JSON_STRING: usize = 64 * 1024;
const MAX_NODE_DEPTH: usize = 32;
const MAX_ACCESSOR_WORK: usize = 900_000;
const JSON_CHUNK: u32 = 0x4e4f534a;
const BIN_CHUNK: u32 = 0x004e4942;
const QUANTIZATION: &str = "KHR_mesh_quantization";
type Matrix = [f64; 16];
const IDENTITY: Matrix = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

pub(super) struct Decoded {
    pub mesh: Vec<Triangle>,
    pub details: Vec<String>,
    pub animation_source: Option<Arc<GlbAnimation>>,
    pub appearance: Arc<super::appearance::SceneAppearance>,
    pub reverse_winding: Vec<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    asset: Asset,
    scene: Option<usize>,
    #[serde(default)]
    scenes: Vec<Scene>,
    #[serde(default)]
    nodes: Vec<Node>,
    #[serde(default)]
    meshes: Vec<Mesh>,
    #[serde(default)]
    accessors: Vec<Accessor>,
    #[serde(default)]
    buffer_views: Vec<BufferView>,
    #[serde(default)]
    buffers: Vec<Buffer>,
    #[serde(default)]
    skins: Vec<Skin>,
    #[serde(default)]
    animations: Vec<serde_json::Value>,
    #[serde(default)]
    materials: Vec<serde_json::Value>,
    #[serde(default)]
    images: Vec<serde_json::Value>,
    #[serde(default)]
    textures: Vec<serde_json::Value>,
    #[serde(default)]
    samplers: Vec<serde_json::Value>,
    #[serde(default)]
    extensions_used: Vec<String>,
    #[serde(default)]
    extensions_required: Vec<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Asset {
    version: String,
    min_version: Option<String>,
}
#[derive(Deserialize)]
struct Scene {
    #[serde(default)]
    nodes: Vec<usize>,
}
#[derive(Clone, Debug, Deserialize)]
struct Node {
    mesh: Option<usize>,
    skin: Option<usize>,
    weights: Option<Vec<f64>>,
    #[serde(default)]
    children: Vec<usize>,
    matrix: Option<Matrix>,
    translation: Option<Point>,
    rotation: Option<[f64; 4]>,
    scale: Option<Point>,
}
#[derive(Deserialize)]
struct Mesh {
    primitives: Vec<Primitive>,
    weights: Option<Vec<f64>>,
}
#[derive(Deserialize)]
struct Primitive {
    attributes: BTreeMap<String, usize>,
    indices: Option<usize>,
    mode: Option<u32>,
    material: Option<usize>,
    #[serde(default)]
    targets: Vec<BTreeMap<String, usize>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Buffer {
    byte_length: usize,
    uri: Option<String>,
    #[serde(default)]
    extensions: BufferExtensions,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BufferView {
    buffer: usize,
    #[serde(default)]
    byte_offset: usize,
    byte_length: usize,
    byte_stride: Option<usize>,
    target: Option<u32>,
    #[serde(default)]
    extensions: ViewExtensions,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Accessor {
    buffer_view: Option<usize>,
    #[serde(default)]
    byte_offset: usize,
    component_type: u32,
    count: usize,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    normalized: bool,
    sparse: Option<Sparse>,
    min: Option<Vec<f64>>,
    max: Option<Vec<f64>>,
}
#[derive(Deserialize)]
struct Sparse {
    count: usize,
    indices: SparseIndices,
    values: SparseValues,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SparseIndices {
    buffer_view: usize,
    #[serde(default)]
    byte_offset: usize,
    component_type: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SparseValues {
    buffer_view: usize,
    #[serde(default)]
    byte_offset: usize,
}

pub(super) fn decode(bytes: &[u8], check: &impl Fn() -> Result<()>) -> Result<Decoded> {
    check()?;
    let (json, bin) = container(bytes, check)?;
    JsonPreflight::new(json, check).run()?;
    check()?;
    let document: Document = serde_json::from_slice(json).context("Invalid GLB glTF JSON")?;
    check()?;
    let transforms = validate_document(&document, bin, check)?;
    let scene = document.scene.unwrap_or(0);
    let roots = &document
        .scenes
        .get(scene)
        .context("GLB has no selectable scene; a declared scene or scene 0 is required")?
        .nodes;
    let quantized = document
        .extensions_required
        .iter()
        .any(|v| v == QUANTIZATION);
    let mut decoder = Decoder {
        document: &document,
        bin,
        check,
        quantized,
        vertices: 0,
        indices: 0,
        accessor_work: 0,
        source_triangles: 0,
        meshes: HashMap::new(),
        deformation_components: 0,
        retained_deformation_bytes: 0,
        appearance: appearance::Builder::default(),
        decoded_views: HashMap::new(),
        compressed_bytes: 0,
        decompressed_bytes: 0,
    };
    let mut details = vec![format!(
        "GLB scene {scene} ({}) · meters converted to millimeters, right-handed Z-up",
        if document.scene.is_some() {
            "declared default"
        } else {
            "first scene; no default declared"
        }
    )];
    let mut source = decoder.deformation_source(roots, transforms, &mut details)?;
    let frame = source.default_frame(check)?;
    let appearance_triangles = source.take_appearance_triangles();
    let (appearance, appearance_details) = decoder.appearance.finish(appearance_triangles);
    details.extend(appearance_details);
    details.push(format!("{} animation playback clip(s); authored default pose shown until a clip is selected. External resources are never loaded.", source.clips().len()));
    let animation_source = (!source.clips().is_empty()).then(|| Arc::new(source));
    Ok(Decoded {
        mesh: frame.triangles,
        details,
        animation_source,
        appearance,
        reverse_winding: frame.reverse_winding,
    })
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32> {
    let end = offset
        .checked_add(4)
        .context("GLB integer offset overflow")?;
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..end)
            .context("Truncated GLB integer")?
            .try_into()
            .unwrap(),
    ))
}

fn container<'a>(
    bytes: &'a [u8],
    check: &impl Fn() -> Result<()>,
) -> Result<(&'a [u8], Option<&'a [u8]>)> {
    ensure!(
        bytes.len() >= 20 && bytes.get(..4) == Some(b"glTF"),
        "Invalid or truncated GLB header"
    );
    ensure!(
        u32_at(bytes, 4)? == 2,
        "Only GLB container version 2 is supported"
    );
    ensure!(
        u32_at(bytes, 8)? as usize == bytes.len() && bytes.len().is_multiple_of(4),
        "Invalid GLB declared length or alignment"
    );
    let mut offset = 12usize;
    let mut json = None;
    let mut bin = None;
    let mut chunks = 0;
    while offset < bytes.len() {
        check()?;
        ensure!(chunks < 128, "GLB exceeds the 128 chunk preview limit");
        let length = u32_at(bytes, offset)? as usize;
        let kind = u32_at(bytes, offset + 4)?;
        let start = offset.checked_add(8).context("GLB chunk offset overflow")?;
        let end = start
            .checked_add(length)
            .context("GLB chunk length overflow")?;
        ensure!(
            length.is_multiple_of(4),
            "GLB chunk length is not four-byte aligned"
        );
        let data = bytes.get(start..end).context("Truncated GLB chunk")?;
        ensure!(
            chunks != 0 || kind == JSON_CHUNK,
            "GLB must begin with its JSON chunk"
        );
        match kind {
            JSON_CHUNK => {
                ensure!(
                    chunks == 0 && json.is_none(),
                    "Duplicate or misplaced GLB JSON chunk"
                );
                ensure!(
                    length <= MAX_JSON_BYTES,
                    "GLB JSON exceeds the 4 MiB preview limit"
                );
                json = Some(data);
            }
            BIN_CHUNK => {
                ensure!(
                    chunks == 1 && bin.is_none(),
                    "Duplicate or misplaced GLB BIN chunk"
                );
                bin = Some(data);
            }
            _ => {} // Khronos permits unknown chunks; they are never interpreted.
        }
        offset = end;
        chunks += 1;
    }
    Ok((json.context("GLB has no JSON chunk")?, bin))
}

fn supported_extension(name: &str) -> bool {
    matches!(
        name,
        QUANTIZATION
            | compression::EXTENSION
            | "KHR_materials_unlit"
            | "KHR_materials_pbrSpecularGlossiness"
            | "KHR_materials_clearcoat"
            | "KHR_materials_transmission"
            | "KHR_materials_volume"
            | "KHR_materials_ior"
            | "KHR_materials_specular"
            | "KHR_materials_sheen"
            | "KHR_materials_iridescence"
            | "KHR_materials_anisotropy"
            | "KHR_materials_emissive_strength"
            | "KHR_materials_dispersion"
            | "KHR_materials_diffuse_transmission"
            | "KHR_materials_variants"
            | "KHR_texture_transform"
            | "KHR_texture_basisu"
            | "KHR_lights_punctual"
            | "EXT_texture_webp"
            | "EXT_texture_avif"
            | "MSFT_texture_dds"
            | "EXT_texture_filter_anisotropic"
    )
}

// Structurally bounded preflight before serde builds typed vectors.
// Every object rejects duplicate decoded keys, including escaped duplicates.
struct JsonPreflight<'a, F> {
    bytes: &'a [u8],
    check: &'a F,
    offset: usize,
    tokens: usize,
    next_check: usize,
}
#[derive(Clone, Copy)]
enum JsonLocation {
    Root,
    Buffers,
    BufferViews,
    Buffer,
    BufferView,
    Other,
}
impl<'a, F: Fn() -> Result<()>> JsonPreflight<'a, F> {
    fn new(bytes: &'a [u8], check: &'a F) -> Self {
        Self {
            bytes,
            check,
            offset: 0,
            tokens: 0,
            next_check: 0,
        }
    }
    fn run(mut self) -> Result<()> {
        self.value(0, false, true, JsonLocation::Root)?;
        self.space()?;
        ensure!(
            self.offset == self.bytes.len(),
            "Trailing data after GLB JSON"
        );
        Ok(())
    }
    fn checkpoint(&mut self) -> Result<()> {
        if self.offset >= self.next_check {
            (self.check)()?;
            self.next_check = self.offset.saturating_add(4096);
        }
        Ok(())
    }
    fn space(&mut self) -> Result<()> {
        while self
            .bytes
            .get(self.offset)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.offset += 1;
            self.checkpoint()?;
        }
        Ok(())
    }
    fn expect(&mut self, value: u8) -> Result<()> {
        self.space()?;
        ensure!(
            self.bytes.get(self.offset) == Some(&value),
            "Malformed GLB JSON structure"
        );
        self.offset += 1;
        Ok(())
    }
    fn string(&mut self) -> Result<&'a [u8]> {
        self.expect(b'"')?;
        let start = self.offset - 1;
        loop {
            self.checkpoint()?;
            ensure!(
                self.offset - start <= MAX_JSON_STRING,
                "GLB JSON string exceeds the 64 KiB preview limit"
            );
            let byte = *self
                .bytes
                .get(self.offset)
                .context("Unterminated GLB JSON string")?;
            self.offset += 1;
            match byte {
                b'"' => return Ok(&self.bytes[start..self.offset]),
                b'\\' => {
                    self.offset = self
                        .offset
                        .checked_add(1)
                        .context("GLB JSON escape overflow")?;
                }
                0..=31 => bail!("Invalid control character in GLB JSON string"),
                _ => {}
            }
        }
    }
    fn value(
        &mut self,
        depth: usize,
        extension_map: bool,
        inspect_extensions: bool,
        location: JsonLocation,
    ) -> Result<()> {
        self.checkpoint()?;
        self.tokens += 1;
        ensure!(
            self.tokens <= MAX_JSON_TOKENS,
            "GLB JSON exceeds the 250,000 token preview limit"
        );
        ensure!(
            depth <= MAX_JSON_DEPTH,
            "GLB JSON exceeds the 32-level nesting preview limit"
        );
        self.space()?;
        let next = self
            .bytes
            .get(self.offset)
            .copied()
            .context("Incomplete GLB JSON")?;
        ensure!(
            !extension_map || next == b'{',
            "GLB extensions must be an object"
        );
        match next {
            b'{' => {
                self.offset += 1;
                let mut keys = HashSet::new();
                self.space()?;
                if self.bytes.get(self.offset) == Some(&b'}') {
                    self.offset += 1;
                    return Ok(());
                }
                loop {
                    self.tokens += 1;
                    ensure!(
                        self.tokens <= MAX_JSON_TOKENS && keys.len() < MAX_JSON_ARRAY,
                        "GLB JSON object exceeds the preview complexity limit"
                    );
                    let key: String = serde_json::from_slice(self.string()?)
                        .context("Invalid GLB JSON property")?;
                    if extension_map {
                        ensure!(
                            supported_extension(&key),
                            "GLB extension {key} is not supported for static geometry preview"
                        );
                        ensure!(
                            key != compression::EXTENSION
                                || matches!(
                                    location,
                                    JsonLocation::Buffer | JsonLocation::BufferView
                                ),
                            "GLB EXT_meshopt_compression is only valid on buffers or buffer views"
                        );
                    }
                    ensure!(
                        keys.insert(key.clone()),
                        "Duplicate GLB JSON property: {key}"
                    );
                    self.expect(b':')?;
                    self.value(
                        depth + 1,
                        inspect_extensions && key == "extensions",
                        inspect_extensions && key != "extras",
                        match (location, key.as_str()) {
                            (_, "extensions") => location,
                            (JsonLocation::Root, "buffers") => JsonLocation::Buffers,
                            (JsonLocation::Root, "bufferViews") => JsonLocation::BufferViews,
                            _ => JsonLocation::Other,
                        },
                    )?;
                    self.space()?;
                    match self.bytes.get(self.offset) {
                        Some(b'}') => {
                            self.offset += 1;
                            break;
                        }
                        Some(b',') => self.offset += 1,
                        _ => bail!("Malformed GLB JSON object"),
                    }
                }
            }
            b'[' => {
                self.offset += 1;
                self.space()?;
                if self.bytes.get(self.offset) == Some(&b']') {
                    self.offset += 1;
                    return Ok(());
                }
                let mut entries = 0;
                loop {
                    entries += 1;
                    ensure!(
                        entries <= MAX_JSON_ARRAY,
                        "GLB JSON array exceeds the 16,384-entry preview limit"
                    );
                    self.value(
                        depth + 1,
                        false,
                        inspect_extensions,
                        match location {
                            JsonLocation::Buffers => JsonLocation::Buffer,
                            JsonLocation::BufferViews => JsonLocation::BufferView,
                            _ => JsonLocation::Other,
                        },
                    )?;
                    self.space()?;
                    match self.bytes.get(self.offset) {
                        Some(b']') => {
                            self.offset += 1;
                            break;
                        }
                        Some(b',') => self.offset += 1,
                        _ => bail!("Malformed GLB JSON array"),
                    }
                }
            }
            b'"' => {
                self.string()?;
            }
            _ => {
                let start = self.offset;
                while self
                    .bytes
                    .get(self.offset)
                    .is_some_and(|v| !v.is_ascii_whitespace() && !matches!(v, b',' | b']' | b'}'))
                {
                    self.offset += 1;
                    self.checkpoint()?;
                }
                ensure!(
                    self.offset > start && self.offset - start <= 128,
                    "Invalid or over-limit GLB JSON literal"
                );
            }
        }
        Ok(())
    }
}

fn checked_range(
    offset: usize,
    count: usize,
    stride: usize,
    size: usize,
    length: usize,
) -> Result<()> {
    ensure!(count > 0, "GLB accessor count must be positive");
    let end = (count - 1)
        .checked_mul(stride)
        .and_then(|v| offset.checked_add(v))
        .and_then(|v| v.checked_add(size))
        .context("GLB accessor range overflow")?;
    ensure!(end <= length, "GLB accessor range exceeds its buffer view");
    Ok(())
}
fn component_size(kind: u32) -> Result<usize> {
    match kind {
        5120 | 5121 => Ok(1),
        5122 | 5123 => Ok(2),
        5125 | 5126 => Ok(4),
        _ => bail!("Unsupported GLB accessor component type {kind}"),
    }
}
fn element_size(accessor: &Accessor) -> Result<usize> {
    let component = component_size(accessor.component_type)?;
    let count = match accessor.kind.as_str() {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" => 4,
        "MAT2" => return Ok((2 * component).next_multiple_of(4) * 2),
        "MAT3" => return Ok((3 * component).next_multiple_of(4) * 3),
        "MAT4" => 16,
        _ => bail!("Unsupported GLB accessor layout {}", accessor.kind),
    };
    Ok(component * count)
}
// Matrix columns have four-byte starts, but the final column need not retain
// trailing padding. Distinguish the element step from its actual range payload.
fn element_payload(accessor: &Accessor) -> Result<usize> {
    let component = component_size(accessor.component_type)?;
    let columns = match accessor.kind.as_str() {
        "MAT2" => 2,
        "MAT3" => 3,
        _ => return element_size(accessor),
    };
    let column = columns * component;
    Ok((columns - 1) * column.next_multiple_of(4) + column)
}
fn view(document: &Document, index: usize) -> Result<&BufferView> {
    document
        .buffer_views
        .get(index)
        .context("GLB references a missing buffer view")
}
fn validate_view(document: &Document, source: &BufferView) -> Result<()> {
    let buffer = document
        .buffers
        .get(source.buffer)
        .context("GLB buffer view references a missing buffer")?;
    ensure!(
        source.byte_length > 0
            && source
                .byte_offset
                .checked_add(source.byte_length)
                .is_some_and(|v| v <= buffer.byte_length),
        "GLB buffer view range exceeds its buffer"
    );
    ensure!(
        source
            .byte_stride
            .is_none_or(|v| (4..=252).contains(&v) && v.is_multiple_of(4)),
        "Unsupported GLB byteStride; expected a multiple of four from 4 through 252"
    );
    ensure!(
        source.target.is_none_or(|v| v == 34962 || v == 34963),
        "Invalid GLB buffer view target"
    );
    Ok(())
}
fn validate_accessor(document: &Document, accessor: &Accessor) -> Result<()> {
    ensure!(
        accessor.count > 0 && accessor.count <= MAX_VERTICES,
        "GLB accessor exceeds the 300,000-element preview limit"
    );
    let component = component_size(accessor.component_type)?;
    let size = element_size(accessor)?;
    let payload = element_payload(accessor)?;
    let components = match accessor.kind.as_str() {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" | "MAT2" => 4,
        "MAT3" => 9,
        _ => 16,
    };
    for bound in [&accessor.min, &accessor.max].into_iter().flatten() {
        ensure!(
            bound.len() == components && bound.iter().all(|v| v.is_finite()),
            "Invalid GLB accessor min/max bounds"
        );
    }
    if let (Some(min), Some(max)) = (&accessor.min, &accessor.max) {
        ensure!(
            min.iter().zip(max).all(|(a, b)| a <= b),
            "GLB accessor min exceeds max"
        );
    }
    ensure!(
        !accessor.normalized || matches!(accessor.component_type, 5120..=5123),
        "Unsupported GLB normalized component type"
    );
    ensure!(
        accessor.byte_offset.is_multiple_of(component),
        "Misaligned GLB accessor byteOffset"
    );
    if let Some(index) = accessor.buffer_view {
        let source = view(document, index)?;
        validate_view(document, source)?;
        ensure!(
            source
                .byte_offset
                .checked_add(accessor.byte_offset)
                .is_some_and(|v| v.is_multiple_of(component)),
            "Misaligned GLB accessor buffer position"
        );
        let stride = source.byte_stride.unwrap_or(size);
        ensure!(
            stride >= size,
            "GLB accessor element exceeds its byteStride"
        );
        checked_range(
            accessor.byte_offset,
            accessor.count,
            stride,
            payload,
            source.byte_length,
        )?;
    } else {
        ensure!(
            accessor.byte_offset == 0,
            "GLB accessor without a buffer view cannot have a byteOffset"
        );
    }
    if let Some(sparse) = &accessor.sparse {
        ensure!(
            sparse.count > 0 && sparse.count <= accessor.count,
            "Invalid GLB sparse accessor count"
        );
        ensure!(
            matches!(sparse.indices.component_type, 5121 | 5123 | 5125),
            "Unsupported GLB sparse index component type"
        );
        for (index, offset, component, size, payload) in [
            (
                sparse.indices.buffer_view,
                sparse.indices.byte_offset,
                component_size(sparse.indices.component_type)?,
                component_size(sparse.indices.component_type)?,
                component_size(sparse.indices.component_type)?,
            ),
            (
                sparse.values.buffer_view,
                sparse.values.byte_offset,
                component,
                size,
                payload,
            ),
        ] {
            let source = view(document, index)?;
            validate_view(document, source)?;
            ensure!(
                source.byte_stride.is_none() && source.target.is_none(),
                "GLB sparse buffer views cannot declare byteStride or target"
            );
            ensure!(
                offset.is_multiple_of(component)
                    && source
                        .byte_offset
                        .checked_add(offset)
                        .is_some_and(|v| v.is_multiple_of(component)),
                "Misaligned GLB sparse accessor data"
            );
            checked_range(offset, sparse.count, size, payload, source.byte_length)?;
        }
    }
    Ok(())
}

fn validate_document(
    document: &Document,
    bin: Option<&[u8]>,
    check: &impl Fn() -> Result<()>,
) -> Result<Vec<Matrix>> {
    ensure!(
        document.asset.version == "2.0"
            && document
                .asset
                .min_version
                .as_deref()
                .is_none_or(|v| v == "2.0"),
        "Only glTF asset version 2.0 is supported"
    );
    for names in [&document.extensions_used, &document.extensions_required] {
        let mut unique = HashSet::new();
        for name in names {
            ensure!(unique.insert(name), "Duplicate GLB extension declaration");
            ensure!(
                supported_extension(name),
                "GLB extension {name} is not supported for static geometry preview"
            );
        }
    }
    ensure!(
        document
            .extensions_required
            .iter()
            .all(|v| document.extensions_used.contains(v)),
        "GLB required extensions must also be declared in extensionsUsed"
    );
    ensure!(
        document.nodes.len() <= MAX_OBJECTS
            && document.meshes.len() <= MAX_OBJECTS
            && document.scenes.len() <= MAX_OBJECTS
            && document.accessors.len() <= MAX_JSON_ARRAY
            && document.buffer_views.len() <= MAX_JSON_ARRAY,
        "GLB exceeds node, mesh, scene or accessor preview limits"
    );
    let mut primitive_count = 0usize;
    for mesh in &document.meshes {
        check()?;
        deformation::validate_mesh_weights(mesh)?;
        primitive_count += mesh.primitives.len();
        ensure!(
            primitive_count <= MAX_OBJECTS,
            "GLB exceeds the 4,096 primitive preview limit"
        );
        for primitive in &mesh.primitives {
            check()?;
            let position = *primitive
                .attributes
                .get("POSITION")
                .context("GLB primitive has no POSITION attribute")?;
            let position = document
                .accessors
                .get(position)
                .context("GLB POSITION references a missing accessor")?;
            for (name, &attribute) in &primitive.attributes {
                if name.starts_with("COLOR_") || name.starts_with("TEXCOORD_") {
                    continue;
                }
                let attribute = document
                    .accessors
                    .get(attribute)
                    .context("GLB attribute references a missing accessor")?;
                ensure!(
                    attribute.count == position.count,
                    "GLB primitive attribute counts do not match POSITION"
                );
            }
            ensure!(
                primitive
                    .indices
                    .is_none_or(|v| v < document.accessors.len()),
                "GLB indices reference a missing accessor"
            );
        }
    }
    for (index, buffer) in document.buffers.iter().enumerate() {
        check()?;
        ensure!(buffer.byte_length > 0, "GLB buffer length must be positive");
        if buffer.uri.is_none() && index == 0 {
            let bin = bin.context("GLB embedded geometry buffer has no BIN chunk")?;
            ensure!(
                buffer.byte_length <= bin.len() && bin.len() - buffer.byte_length <= 3,
                "GLB BIN length disagrees with the embedded buffer length"
            );
            ensure!(
                bin[buffer.byte_length..].iter().all(|&v| v == 0),
                "GLB BIN padding must contain zero bytes"
            );
        }
    }
    if bin.is_some() {
        ensure!(
            document.buffers.first().is_some_and(|v| v.uri.is_none()),
            "GLB BIN chunk has no matching embedded buffer 0; external or URI geometry buffers are not supported"
        );
    }
    compression::validate(document, check)?;
    // Geometry/deformation errors remain fatal. Appearance and animation-only
    // accessors are checked when selected, allowing their own disclosed fallback.
    let mut geometry_accessors = HashSet::new();
    for mesh in &document.meshes {
        for primitive in &mesh.primitives {
            geometry_accessors.extend(
                primitive
                    .attributes
                    .iter()
                    .filter(|(name, _)| {
                        !name.starts_with("COLOR_") && !name.starts_with("TEXCOORD_")
                    })
                    .map(|(_, &index)| index),
            );
            geometry_accessors.extend(primitive.indices);
            for target in &primitive.targets {
                geometry_accessors.extend(target.values().copied());
            }
        }
    }
    for skin in &document.skins {
        geometry_accessors.extend(skin.inverse_bind_matrices);
    }
    for index in geometry_accessors {
        check()?;
        validate_accessor(
            document,
            document
                .accessors
                .get(index)
                .context("GLB geometry references a missing accessor")?,
        )?;
    }
    let mut parents = vec![0u8; document.nodes.len()];
    let mut transforms = Vec::with_capacity(document.nodes.len());
    for node in &document.nodes {
        check()?;
        deformation::validate_node(document, node)?;
        ensure!(
            node.mesh.is_none_or(|v| v < document.meshes.len()),
            "GLB node references a missing mesh"
        );
        for &child in &node.children {
            let parent_count = parents
                .get_mut(child)
                .context("GLB node references a missing child")?;
            ensure!(
                *parent_count == 0,
                "GLB nodes must form disjoint trees; duplicate child or multiple parents found"
            );
            *parent_count = 1;
        }
        transforms.push(node_transform(node)?);
    }
    let mut visited = vec![false; document.nodes.len()];
    for (index, &parent_count) in parents.iter().enumerate() {
        if parent_count == 0 {
            validate_tree(document, index, 0, &mut visited, check)?;
        }
    }
    ensure!(
        visited.iter().all(|v| *v),
        "GLB node graph contains a cycle"
    );
    for scene in &document.scenes {
        let mut roots = HashSet::new();
        for &root in &scene.nodes {
            check()?;
            ensure!(roots.insert(root), "Duplicate GLB scene root");
            ensure!(
                parents.get(root) == Some(&0),
                "GLB scene references a missing or non-root node"
            );
        }
    }
    Ok(transforms)
}
fn validate_tree(
    document: &Document,
    node: usize,
    depth: usize,
    visited: &mut [bool],
    check: &impl Fn() -> Result<()>,
) -> Result<()> {
    check()?;
    ensure!(
        depth < MAX_NODE_DEPTH,
        "GLB node hierarchy exceeds the 32-level preview limit"
    );
    ensure!(
        !visited[node],
        "GLB node graph contains a cycle or repeated node"
    );
    visited[node] = true;
    for &child in &document.nodes[node].children {
        validate_tree(document, child, depth + 1, visited, check)?;
    }
    Ok(())
}

fn node_transform(node: &Node) -> Result<Matrix> {
    if let Some(matrix) = node.matrix {
        ensure!(
            node.translation.is_none() && node.rotation.is_none() && node.scale.is_none(),
            "GLB node cannot combine matrix and TRS transforms"
        );
        ensure!(
            matrix.iter().all(|v| v.is_finite() && v.abs() <= 1e12)
                && matrix[3] == 0.
                && matrix[7] == 0.
                && matrix[11] == 0.
                && matrix[15] == 1.,
            "Invalid GLB affine node matrix"
        );
        for a in 0..3 {
            for b in a + 1..3 {
                let column = |c: usize| [matrix[c * 4], matrix[c * 4 + 1], matrix[c * 4 + 2]];
                let a = column(a);
                let b = column(b);
                let scale = (super::dot(a, a) * super::dot(b, b)).sqrt();
                ensure!(
                    super::dot(a, b).abs() <= scale * 1e-5 + 1e-12,
                    "GLB node matrix contains unsupported shear; expected a decomposable TRS matrix"
                );
            }
        }
        return Ok(matrix);
    }
    let translation = valid_point(node.translation.unwrap_or([0.; 3]))?;
    let scale = valid_point(node.scale.unwrap_or([1.; 3]))?;
    let [x, y, z, w] = node.rotation.unwrap_or([0., 0., 0., 1.]);
    let length = x * x + y * y + z * z + w * w;
    ensure!(
        length.is_finite() && (length - 1.).abs() <= 1e-5,
        "GLB node rotation must be a finite unit quaternion"
    );
    Ok([
        (1. - 2. * (y * y + z * z)) * scale[0],
        2. * (x * y + z * w) * scale[0],
        2. * (x * z - y * w) * scale[0],
        0.,
        2. * (x * y - z * w) * scale[1],
        (1. - 2. * (x * x + z * z)) * scale[1],
        2. * (y * z + x * w) * scale[1],
        0.,
        2. * (x * z + y * w) * scale[2],
        2. * (y * z - x * w) * scale[2],
        (1. - 2. * (x * x + y * y)) * scale[2],
        0.,
        translation[0],
        translation[1],
        translation[2],
        1.,
    ])
}
fn compose(parent: Matrix, local: Matrix) -> Result<Matrix> {
    let result = std::array::from_fn(|index| {
        let row = index % 4;
        let column = index / 4;
        (0..4)
            .map(|k| parent[k * 4 + row] * local[column * 4 + k])
            .sum::<f64>()
    });
    ensure!(
        result.iter().all(|v| v.is_finite() && v.abs() <= 1e12),
        "GLB composed transform contains unreasonable values"
    );
    Ok(result)
}
fn transform_point(matrix: Matrix, point: Point) -> Result<Point> {
    let point: Point = std::array::from_fn(|i| {
        matrix[i] * point[0] + matrix[4 + i] * point[1] + matrix[8 + i] * point[2] + matrix[12 + i]
    });
    valid_point([1000. * point[0], -1000. * point[2], 1000. * point[1]])
}

struct Decoder<'a, F> {
    document: &'a Document,
    bin: Option<&'a [u8]>,
    check: &'a F,
    quantized: bool,
    vertices: usize,
    indices: usize,
    accessor_work: usize,
    source_triangles: usize,
    meshes: HashMap<usize, RetainedMesh>,
    deformation_components: usize,
    retained_deformation_bytes: usize,
    appearance: appearance::Builder,
    decoded_views: HashMap<usize, DecodedView>,
    compressed_bytes: usize,
    decompressed_bytes: usize,
}
impl<F: Fn() -> Result<()>> Decoder<'_, F> {
    fn data(&self, index: usize) -> Result<&[u8]> {
        if let Some(decoded) = self.decoded_views.get(&index) {
            return Ok(decoded.bytes());
        }
        let source = view(self.document, index)?;
        validate_view(self.document, source)?;
        ensure!(
            source.extensions.meshopt.is_none(),
            "GLB compressed buffer view was not prepared"
        );
        let buffer = &self.document.buffers[source.buffer];
        ensure!(
            buffer.uri.is_none() && source.buffer == 0,
            "GLB geometry references an external or URI buffer; only supplied embedded BIN geometry is supported"
        );
        let bin = self
            .bin
            .context("GLB geometry requires a missing BIN chunk")?;
        bin.get(source.byte_offset..source.byte_offset + source.byte_length)
            .context("GLB buffer view is outside the supplied BIN chunk")
    }
    fn charge(&mut self, accessor: &Accessor, positions: bool) -> Result<()> {
        let counter = if positions {
            &mut self.vertices
        } else {
            &mut self.indices
        };
        *counter = counter
            .checked_add(accessor.count)
            .context("GLB accessor work overflow")?;
        ensure!(
            *counter <= MAX_VERTICES,
            "GLB exceeds the 300,000 cumulative decoded vertex or index preview limit"
        );
        self.accessor_work = self
            .accessor_work
            .checked_add(accessor.count)
            .and_then(|v| v.checked_add(accessor.sparse.as_ref().map_or(0, |v| v.count)))
            .context("GLB accessor work overflow")?;
        ensure!(
            self.accessor_work <= MAX_ACCESSOR_WORK,
            "GLB exceeds the 900,000 accessor-element work preview limit"
        );
        Ok(())
    }
    fn sparse_indices(&self, accessor: &Accessor) -> Result<Vec<usize>> {
        let Some(sparse) = &accessor.sparse else {
            return Ok(Vec::new());
        };
        let data = self.data(sparse.indices.buffer_view)?;
        let size = component_size(sparse.indices.component_type)?;
        let mut indices = Vec::with_capacity(sparse.count);
        for i in 0..sparse.count {
            if i.is_multiple_of(256) {
                (self.check)()?;
            }
            let index = read_index(
                data,
                sparse.indices.byte_offset + i * size,
                sparse.indices.component_type,
            )?;
            ensure!(
                index < accessor.count && indices.last().is_none_or(|&last| last < index),
                "GLB sparse indices must be strictly increasing and within the accessor"
            );
            indices.push(index);
        }
        Ok(indices)
    }
    fn positions(&mut self, index: usize) -> Result<Vec<Point>> {
        self.prepare_accessor(index)?;
        let accessor = self
            .document
            .accessors
            .get(index)
            .context("GLB POSITION references a missing accessor")?;
        ensure!(
            accessor.kind == "VEC3",
            "GLB POSITION must use VEC3 accessors"
        );
        let integer = matches!(accessor.component_type, 5120..=5123);
        ensure!(
            (accessor.component_type == 5126 && !accessor.normalized)
                || (integer && self.quantized),
            "GLB POSITION supports FLOAT or BYTE/SHORT quantization declared in required KHR_mesh_quantization"
        );
        let size = element_size(accessor)?;
        if let Some(index) = accessor.buffer_view {
            let source = view(self.document, index)?;
            ensure!(
                source.target != Some(34963),
                "GLB POSITION cannot use an index buffer view"
            );
            let stride = source.byte_stride.unwrap_or(size);
            ensure!(
                accessor.byte_offset.is_multiple_of(4)
                    && (source.byte_offset + accessor.byte_offset).is_multiple_of(4)
                    && stride.is_multiple_of(4),
                "GLB vertex positions and stride must be four-byte aligned"
            );
            self.data(index)?;
        }
        if let Some(sparse) = &accessor.sparse {
            self.data(sparse.values.buffer_view)?;
        }
        self.charge(accessor, true)?;
        let sparse_indices = self.sparse_indices(accessor)?;
        let mut positions = vec![[0.; 3]; accessor.count];
        if let Some(index) = accessor.buffer_view {
            let source = view(self.document, index)?;
            let data = self.data(index)?;
            let stride = source.byte_stride.unwrap_or(size);
            for (i, point) in positions.iter_mut().enumerate() {
                if i.is_multiple_of(256) {
                    (self.check)()?;
                }
                *point = read_point(data, accessor.byte_offset + i * stride, accessor)?;
            }
        }
        if let Some(sparse) = &accessor.sparse {
            let data = self.data(sparse.values.buffer_view)?;
            for (i, &index) in sparse_indices.iter().enumerate() {
                if i.is_multiple_of(256) {
                    (self.check)()?;
                }
                positions[index] =
                    read_point(data, sparse.values.byte_offset + i * size, accessor)?;
            }
        }
        Ok(positions)
    }
    fn indices(&mut self, index: usize, vertices: usize) -> Result<Vec<usize>> {
        self.prepare_accessor(index)?;
        let accessor = self
            .document
            .accessors
            .get(index)
            .context("GLB indices reference a missing accessor")?;
        ensure!(
            accessor.kind == "SCALAR"
                && !accessor.normalized
                && matches!(accessor.component_type, 5121 | 5123 | 5125),
            "GLB indices require non-normalized unsigned SCALAR accessors"
        );
        let size = component_size(accessor.component_type)?;
        if let Some(index) = accessor.buffer_view {
            let source = view(self.document, index)?;
            ensure!(
                source.byte_stride.is_none() && source.target != Some(34962),
                "GLB index accessor must be tightly packed in an index buffer view"
            );
            self.data(index)?;
        }
        if let Some(sparse) = &accessor.sparse {
            self.data(sparse.values.buffer_view)?;
        }
        self.charge(accessor, false)?;
        let sparse_indices = self.sparse_indices(accessor)?;
        let mut indices = vec![0; accessor.count];
        if let Some(index) = accessor.buffer_view {
            let data = self.data(index)?;
            for (i, target) in indices.iter_mut().enumerate() {
                if i.is_multiple_of(256) {
                    (self.check)()?;
                }
                *target = read_index(
                    data,
                    accessor.byte_offset + i * size,
                    accessor.component_type,
                )?;
            }
        }
        if let Some(sparse) = &accessor.sparse {
            let data = self.data(sparse.values.buffer_view)?;
            for (i, &index) in sparse_indices.iter().enumerate() {
                if i.is_multiple_of(256) {
                    (self.check)()?;
                }
                indices[index] = read_index(
                    data,
                    sparse.values.byte_offset + i * size,
                    accessor.component_type,
                )?;
            }
        }
        let sentinel = match accessor.component_type {
            5121 => u8::MAX as usize,
            5123 => u16::MAX as usize,
            _ => u32::MAX as usize,
        };
        for (i, &index) in indices.iter().enumerate() {
            if i.is_multiple_of(256) {
                (self.check)()?;
            }
            ensure!(
                index < vertices && index != sentinel,
                "GLB triangle index is out of range or uses the forbidden primitive-restart value"
            );
        }
        Ok(indices)
    }
}
fn read_index(data: &[u8], offset: usize, kind: u32) -> Result<usize> {
    let size = component_size(kind)?;
    let end = offset
        .checked_add(size)
        .context("GLB index offset overflow")?;
    let data = data.get(offset..end).context("Truncated GLB index data")?;
    match kind {
        5121 => Ok(data[0] as usize),
        5123 => Ok(u16::from_le_bytes(data.try_into().unwrap()) as usize),
        5125 => Ok(u32::from_le_bytes(data.try_into().unwrap()) as usize),
        _ => bail!("Unsupported GLB index component type"),
    }
}
fn read_point(data: &[u8], offset: usize, accessor: &Accessor) -> Result<Point> {
    let size = component_size(accessor.component_type)?;
    let mut result = [0.; 3];
    for (axis, value) in result.iter_mut().enumerate() {
        let start = offset
            .checked_add(axis * size)
            .context("GLB position offset overflow")?;
        let end = start
            .checked_add(size)
            .context("GLB position offset overflow")?;
        let bytes = data
            .get(start..end)
            .context("Truncated GLB position data")?;
        let (raw, divisor, signed) = match accessor.component_type {
            5120 => (bytes[0] as i8 as f64, 127., true),
            5121 => (bytes[0] as f64, 255., false),
            5122 => (
                i16::from_le_bytes(bytes.try_into().unwrap()) as f64,
                32767.,
                true,
            ),
            5123 => (
                u16::from_le_bytes(bytes.try_into().unwrap()) as f64,
                65535.,
                false,
            ),
            5126 => (
                f32::from_le_bytes(bytes.try_into().unwrap()) as f64,
                1.,
                false,
            ),
            _ => bail!("Unsupported GLB POSITION component type"),
        };
        *value = if accessor.normalized {
            if signed {
                (raw / divisor).max(-1.)
            } else {
                raw / divisor
            }
        } else {
            raw
        };
    }
    valid_point(result)
}

#[cfg(test)]
mod tests;
