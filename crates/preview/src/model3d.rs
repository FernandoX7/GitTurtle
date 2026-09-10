//! Supplied-byte mesh previews. All parsing, deformation and rasterization stays on
//! the preview worker; only owned geometry and RGBA views leave this module.

use crate::{ImagePreview, MAX_INPUT_BYTES};
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::HashMap,
    io::{Cursor, Read},
    path::Path,
    sync::Arc,
};

mod appearance;
mod camera;
mod glb;
mod step;
pub use camera::{
    ModelBounds, ModelCamera, ModelScene, ModelStandardView, ModelUnits, OrientationAxis,
    render_model,
};
pub use glb::ModelAnimationClip;

pub const MODEL_VIEW_EDGE: u32 = 720;
pub const MAX_MODEL_TRIANGLES: usize = 100_000;
const MAX_VERTICES: usize = 300_000;
const MAX_OBJECTS: usize = 4096;
const MAX_FACE_VERTICES: usize = 1024;
const MAX_MODEL_XML: usize = 16 * 1024 * 1024;
const MAX_RASTER_SAMPLES: u64 = 64_000_000;
const CORE_3MF: &str = "http://schemas.microsoft.com/3dmanufacturing/core/2015/02";
type Point = [f64; 3];
type Triangle = [Point; 3];
type Transform = [f64; 12];
const IDENTITY: Transform = [1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0.];

#[derive(Debug)]
pub struct ModelView {
    pub caption: String,
    pub image: ImagePreview,
}

#[derive(Debug)]
pub struct ModelPreview {
    pub format: String,
    pub details: Vec<String>,
    pub views: Vec<ModelView>,
    pub scene: Arc<ModelScene>,
}

#[derive(Debug)]
pub struct ModelGeometry {
    pub format: String,
    pub details: Vec<String>,
    pub scene: Arc<ModelScene>,
}

pub fn is_model_path(path: &Path) -> bool {
    path.extension().and_then(|v| v.to_str()).is_some_and(|v| {
        matches!(
            v.to_ascii_lowercase().as_str(),
            "stl" | "obj" | "fbx" | "3mf" | "step" | "stp" | "glb"
        )
    })
}

/// File names are hints only, never paths to open. External geometry, materials,
/// textures, caches, and scripts are not resolved.
pub fn decode_geometry(
    bytes: &[u8],
    name: &str,
    check: impl Fn() -> Result<()>,
) -> Result<ModelGeometry> {
    check()?;
    ensure!(
        bytes.len() <= MAX_INPUT_BYTES,
        "3D source exceeds the 32 MiB preview limit"
    );
    let ext = Path::new(name)
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut glb_resources = None;
    let (format, mesh, units, mut details) = match ext.as_str() {
        "stl" => ("STL", stl(bytes, &check)?, ModelUnits::Unknown, vec!["Untextured mesh; STL does not define units".into()]),
        "obj" => ("OBJ", fbx_or_obj(bytes, true, &check)?, ModelUnits::Unknown, vec!["Untextured mesh; OBJ does not define units; material libraries and textures are not loaded".into()]),
        "fbx" => ("FBX", fbx_or_obj(bytes, false, &check)?, ModelUnits::Millimeters, vec!["Static mesh at its default pose, Z-up axes; declared FBX units converted to millimeters (FBX defaults to centimeters when unspecified); materials, textures, animation, and external caches are not loaded".into()]),
        "3mf" => {
            let (mut mesh, unit) = three_mf(bytes, &check)?;
            let factor = match unit.as_str() { "micron" => 0.001, "centimeter" => 10., "inch" => 25.4, "foot" => 304.8, "meter" => 1000., _ => 1. };
            for triangle in &mut mesh { check()?; for point in triangle { *point = valid_point(point.map(|v| v * factor))?; } }
            ("3MF", mesh, ModelUnits::Millimeters, vec![format!("Core build geometry · source units: {unit}, displayed in millimeters; materials and textures are not rendered")])
        }
        "step" | "stp" => {
            let cad = step::decode(bytes, &check)?;
            ("STEP", cad.mesh, cad.units, cad.details)
        }
        "glb" => {
            let model = glb::decode(bytes, &check)?;
            glb_resources = Some((model.appearance, model.animation_source, model.reverse_winding));
            ("GLB", model.mesh, ModelUnits::Millimeters, model.details)
        }
        _ => bail!("Unsupported 3D model format"),
    };
    ensure!(
        !mesh.is_empty(),
        "The model has no renderable triangle geometry"
    );
    ensure!(
        mesh.iter().any(|triangle| {
            let normal = cross(
                subtract(triangle[1], triangle[0]),
                subtract(triangle[2], triangle[0]),
            );
            dot(normal, normal) > 1e-30
        }),
        "The model contains only degenerate triangles"
    );
    let (minimum, maximum) = bounds(&mesh)?;
    let mut scene = ModelScene::new(mesh, units, format)?;
    if let Some((appearance, animation, reverse)) = glb_resources {
        scene.set_appearance(appearance);
        if let Some(animation) = animation {
            scene.set_animation_source(animation);
        }
        scene.set_reverse_winding(reverse);
    }
    let scene = Arc::new(scene);
    details.push(format!(
        "{} triangles · extent {:.4} × {:.4} × {:.4}",
        scene.triangle_count(),
        maximum[0] - minimum[0],
        maximum[1] - minimum[1],
        maximum[2] - minimum[2]
    ));
    Ok(ModelGeometry {
        format: format.into(),
        details,
        scene,
    })
}

/// Legacy fixed views for secondary static surfaces. Interactive consumers use
/// `decode_geometry` and render only the requested camera with `render_model`.
pub fn decode_model(
    bytes: &[u8],
    name: &str,
    check: impl Fn() -> Result<()>,
) -> Result<ModelPreview> {
    let ModelGeometry {
        format,
        details,
        scene,
    } = decode_geometry(bytes, name, &check)?;
    let mut work = 0;
    let mut views = Vec::with_capacity(4);
    for (view, eye, up) in [
        (ModelStandardView::Isometric, [1., -1., 0.8], [0., 0., 1.]),
        (ModelStandardView::Front, [0., -1., 0.], [0., 0., 1.]),
        (ModelStandardView::Right, [1., 0., 0.], [0., 0., 1.]),
        (ModelStandardView::Top, [0., 0., 1.], [0., 1., 0.]),
    ] {
        check()?;
        views.push(ModelView {
            caption: view.label().into(),
            image: if format == "GLB" {
                let mut camera = ModelCamera::fit(scene.bounds);
                camera.set_view(view);
                render_model(&scene, &camera, MODEL_VIEW_EDGE, false, &check)?
            } else {
                rasterize(scene.triangles(), eye, up, &format, &mut work, &check)?
            },
        });
    }
    Ok(ModelPreview {
        format,
        details,
        views,
        scene,
    })
}

fn valid_point(point: Point) -> Result<Point> {
    ensure!(
        point.iter().all(|v| v.is_finite() && v.abs() <= 1.0e12),
        "Model contains non-finite or unreasonable coordinates"
    );
    Ok(point)
}

fn push_triangle(mesh: &mut Vec<Triangle>, triangle: Triangle) -> Result<()> {
    ensure!(
        mesh.len() < MAX_MODEL_TRIANGLES,
        "Model exceeds the 100,000 triangle preview limit"
    );
    for point in triangle {
        valid_point(point)?;
    }
    mesh.push(triangle);
    Ok(())
}

fn stl(bytes: &[u8], check: &impl Fn() -> Result<()>) -> Result<Vec<Triangle>> {
    if bytes.len() >= 84 {
        let declared = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        if 84usize.checked_add(declared.saturating_mul(50)) == Some(bytes.len()) {
            ensure!(
                declared <= MAX_MODEL_TRIANGLES,
                "STL exceeds the 100,000 triangle preview limit"
            );
        }
    }
    let mut input = Cursor::new(bytes);
    let reader = stl_io::create_stl_reader(&mut input).context("Invalid STL")?;
    let mut mesh = Vec::new();
    for triangle in reader {
        check()?;
        let triangle = triangle.context("Invalid STL triangle")?;
        push_triangle(
            &mut mesh,
            triangle
                .vertices
                .map(|v| [v[0] as f64, v[1] as f64, v[2] as f64]),
        )?;
    }
    Ok(mesh)
}

fn fbx_or_obj(bytes: &[u8], obj: bool, check: &impl Fn() -> Result<()>) -> Result<Vec<Triangle>> {
    let progress = |_: &ufbx::Progress| {
        if check().is_ok() {
            ufbx::ProgressResult::Continue
        } else {
            ufbx::ProgressResult::Cancel
        }
    };
    let deny_file = |_: &str, _: &ufbx::OpenFileInfo| None;
    let allocator = || ufbx::AllocatorOpts {
        memory_limit: 96 * 1024 * 1024,
        allocation_limit: 1_000_000,
        ..Default::default()
    };
    let opts = ufbx::LoadOpts {
        temp_allocator: allocator(),
        result_allocator: allocator(),
        ignore_animation: true,
        ignore_embedded: true,
        load_external_files: false,
        ignore_missing_external_files: true,
        evaluate_caches: false,
        skip_skin_vertices: true,
        skip_mesh_parts: true,
        force_single_thread_ascii_parsing: true,
        index_error_handling: ufbx::IndexErrorHandling::AbortLoading,
        target_axes: if obj {
            Default::default()
        } else {
            ufbx::axes_right_handed_z_up()
        },
        target_unit_meters: if obj { 0. } else { 0.001 },
        open_main_file_with_default: false,
        open_file_cb: ufbx::OpenFileCb::Ref(&deny_file),
        progress_cb: ufbx::ProgressCb::Ref(&progress),
        progress_interval_hint: 64 * 1024,
        node_depth_limit: 64,
        file_format: if obj {
            ufbx::FileFormat::Obj
        } else {
            ufbx::FileFormat::Fbx
        },
        ..Default::default()
    };
    let result = ufbx::load_memory(bytes, opts);
    check()?;
    let scene = result.map_err(|error| anyhow::anyhow!("Could not decode model: {error:?}"))?;
    ensure!(
        scene.nodes.len() <= MAX_OBJECTS,
        "Model exceeds the 4,096 node preview limit"
    );
    let mut mesh = Vec::new();
    let mut vertices = 0usize;
    let mut face_indices = Vec::new();
    for node in &scene.nodes {
        check()?;
        let Some(source) = &node.mesh else { continue };
        vertices = vertices.saturating_add(source.num_vertices);
        ensure!(
            vertices <= MAX_VERTICES,
            "Model exceeds the 300,000 vertex preview limit"
        );
        ensure!(
            source.num_faces <= MAX_MODEL_TRIANGLES && source.num_indices <= MAX_VERTICES * 3,
            "Model exceeds the face/index preview limit"
        );
        ensure!(
            source.num_triangles <= MAX_MODEL_TRIANGLES.saturating_sub(mesh.len()),
            "Model exceeds the 100,000 triangle preview limit"
        );
        for face in &source.faces {
            check()?;
            ensure!(
                face.num_indices as usize <= MAX_FACE_VERTICES,
                "A model polygon exceeds the 1,024 vertex preview limit"
            );
            if face.num_indices < 3 {
                continue;
            }
            face_indices.resize((face.num_indices as usize - 2) * 3, 0);
            let count = source.triangulate_face(&mut face_indices, *face) as usize;
            ensure!(
                count * 3 <= face_indices.len(),
                "Invalid model triangulation"
            );
            for indices in face_indices[..count * 3].as_chunks::<3>().0 {
                let mut triangle = [[0.; 3]; 3];
                for (target, index) in triangle.iter_mut().zip(indices) {
                    ensure!(
                        (*index as usize) < source.vertex_position.indices.len(),
                        "Invalid model vertex index"
                    );
                    let value = ufbx::transform_position(
                        &node.geometry_to_world,
                        source.vertex_position[*index as usize],
                    );
                    *target = [value.x, value.y, value.z];
                }
                push_triangle(&mut mesh, triangle)?;
            }
        }
    }
    Ok(mesh)
}

fn archive_preflight(bytes: &[u8]) -> Result<()> {
    let end = bytes.len().saturating_sub(22);
    let start = bytes.len().saturating_sub(65_557);
    let eocd = (start..=end)
        .rev()
        .find(|&offset| bytes.get(offset..offset + 4) == Some(b"PK\x05\x06"));
    let offset = eocd.context("3MF has no ZIP directory")?;
    let fields = bytes
        .get(offset..offset + 22)
        .context("Truncated ZIP directory")?;
    ensure!(
        offset < 20 || bytes.get(offset - 20..offset - 16) != Some(b"PK\x06\x07"),
        "ZIP64 3MF packages are not supported by the bounded preview"
    );
    let word = |i| u16::from_le_bytes([fields[i], fields[i + 1]]);
    let dword = |i| u32::from_le_bytes(fields[i..i + 4].try_into().unwrap());
    ensure!(
        word(4) == 0 && word(6) == 0 && word(8) == word(10),
        "Multi-disk 3MF packages are not supported"
    );
    ensure!(
        word(10) <= 2048 && dword(12) <= 2 * 1024 * 1024 && dword(16) != u32::MAX,
        "3MF directory exceeds preview limits or uses ZIP64"
    );
    ensure!(
        offset + 22 + word(20) as usize == bytes.len(),
        "Invalid ZIP directory length"
    );
    ensure!(
        (dword(16) as u64) + (dword(12) as u64) <= offset as u64,
        "Invalid 3MF central directory range"
    );
    Ok(())
}

fn read_part(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
    cap: usize,
    check: &impl Fn() -> Result<()>,
) -> Result<String> {
    check()?;
    let mut part = archive
        .by_name(name)
        .with_context(|| format!("Missing 3MF part {name}"))?;
    ensure!(
        !part.is_dir() && part.size() <= cap as u64,
        "3MF part exceeds its decompressed preview limit"
    );
    ensure!(
        matches!(
            part.compression(),
            zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
        ),
        "Unsupported 3MF compression method"
    );
    let mut data = Vec::new();
    let mut chunk = [0; 16 * 1024];
    loop {
        check()?;
        let count = part
            .read(&mut chunk)
            .context("Could not decompress 3MF part")?;
        if count == 0 {
            break;
        }
        ensure!(
            data.len() + count <= cap,
            "3MF part exceeds its decompressed preview limit"
        );
        data.extend_from_slice(&chunk[..count]);
    }
    String::from_utf8(data).context("3MF XML must be UTF-8")
}

fn xml(text: &str) -> Result<roxmltree::Document<'_>> {
    roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: 650_000,
            ..Default::default()
        },
    )
    .context("Invalid or over-limit 3MF XML")
}

fn child<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    tag: &str,
) -> Result<roxmltree::Node<'a, 'input>> {
    node.children()
        .find(|v| v.has_tag_name((CORE_3MF, tag)))
        .with_context(|| format!("3MF is missing {tag}"))
}

fn number(node: roxmltree::Node<'_, '_>, name: &str) -> Result<f64> {
    node.attribute(name)
        .with_context(|| format!("3MF is missing {name}"))?
        .parse()
        .context("Invalid 3MF coordinate")
}

fn index(node: roxmltree::Node<'_, '_>, name: &str) -> Result<usize> {
    node.attribute(name)
        .with_context(|| format!("3MF is missing {name}"))?
        .parse()
        .context("Invalid 3MF index")
}

fn transform(node: roxmltree::Node<'_, '_>) -> Result<Transform> {
    let Some(text) = node.attribute("transform") else {
        return Ok(IDENTITY);
    };
    let mut result = [0.; 12];
    let mut values = text.split_ascii_whitespace();
    for value in &mut result {
        *value = values
            .next()
            .context("Incomplete 3MF transform")?
            .parse::<f64>()
            .context("Invalid 3MF transform")?;
    }
    ensure!(
        values.next().is_none() && result.iter().all(|v| v.is_finite() && v.abs() <= 1.0e12),
        "Invalid 3MF transform"
    );
    Ok(result)
}

fn apply(matrix: Transform, point: Point) -> Result<Point> {
    valid_point(std::array::from_fn(|i| {
        point[0] * matrix[i] + point[1] * matrix[3 + i] + point[2] * matrix[6 + i] + matrix[9 + i]
    }))
}

enum Object {
    Mesh(Vec<Triangle>),
    Components(Vec<(usize, Transform)>),
}

fn three_mf(bytes: &[u8], check: &impl Fn() -> Result<()>) -> Result<(Vec<Triangle>, String)> {
    archive_preflight(bytes)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).context("Invalid 3MF package")?;
    ensure!(archive.len() <= 2048, "3MF contains too many parts");
    let relations = read_part(&mut archive, "_rels/.rels", 64 * 1024, check)?;
    let relations = xml(&relations)?;
    let starts: Vec<_> = relations
        .descendants()
        .filter(|v| {
            v.has_tag_name((
                "http://schemas.openxmlformats.org/package/2006/relationships",
                "Relationship",
            )) && v.attribute("Type").is_some_and(|t| t.ends_with("/3dmodel"))
        })
        .collect();
    ensure!(
        starts.len() == 1,
        "3MF must have one root model relationship"
    );
    ensure!(
        starts[0].attribute("TargetMode") != Some("External"),
        "External 3MF models are not loaded"
    );
    let part_name = starts[0]
        .attribute("Target")
        .context("3MF model relationship has no target")?
        .trim_start_matches('/');
    ensure!(
        !part_name.contains(['\\', ':', '%', '?', '#'])
            && part_name
                .split('/')
                .all(|p| !p.is_empty() && p != "." && p != ".."),
        "Unsupported 3MF model part reference"
    );
    let source = read_part(&mut archive, part_name, MAX_MODEL_XML, check)?;
    let document = xml(&source)?;
    let model = document.root_element();
    ensure!(
        model.tag_name().name() == "model" && model.tag_name().namespace() == Some(CORE_3MF),
        "Unsupported 3MF model namespace"
    );
    ensure!(
        model
            .attribute("requiredextensions")
            .is_none_or(|v| v.trim().is_empty()),
        "This 3MF requires geometry extensions that are not rendered"
    );
    let unit = model.attribute("unit").unwrap_or("millimeter");
    ensure!(
        matches!(
            unit,
            "micron" | "millimeter" | "centimeter" | "inch" | "foot" | "meter"
        ),
        "Unknown 3MF model units"
    );
    let resources = child(model, "resources")?;
    let mut objects = HashMap::new();
    let mut total_vertices = 0usize;
    let mut total_triangles = 0usize;
    for object in resources
        .children()
        .filter(|v| v.has_tag_name((CORE_3MF, "object")))
    {
        check()?;
        ensure!(
            objects.len() < MAX_OBJECTS,
            "3MF exceeds the 4,096 object limit"
        );
        let id = index(object, "id")?;
        ensure!(!objects.contains_key(&id), "Duplicate 3MF object ID");
        let value = if let Ok(source_mesh) = child(object, "mesh") {
            let mut vertices = Vec::new();
            for vertex in child(source_mesh, "vertices")?
                .children()
                .filter(|v| v.has_tag_name((CORE_3MF, "vertex")))
            {
                check()?;
                total_vertices += 1;
                ensure!(
                    total_vertices <= MAX_VERTICES,
                    "3MF exceeds the 300,000 vertex preview limit"
                );
                vertices.push(valid_point([
                    number(vertex, "x")?,
                    number(vertex, "y")?,
                    number(vertex, "z")?,
                ])?);
            }
            let mut triangles = Vec::new();
            for triangle in child(source_mesh, "triangles")?
                .children()
                .filter(|v| v.has_tag_name((CORE_3MF, "triangle")))
            {
                check()?;
                total_triangles += 1;
                ensure!(
                    total_triangles <= MAX_MODEL_TRIANGLES,
                    "3MF exceeds the 100,000 triangle preview limit"
                );
                let ids = [
                    index(triangle, "v1")?,
                    index(triangle, "v2")?,
                    index(triangle, "v3")?,
                ];
                ensure!(
                    ids.iter().all(|&id| id < vertices.len()),
                    "Invalid 3MF triangle vertex index"
                );
                triangles.push(ids.map(|id| vertices[id]));
            }
            Object::Mesh(triangles)
        } else {
            let mut components = Vec::new();
            for component in child(object, "components")?
                .children()
                .filter(|v| v.has_tag_name((CORE_3MF, "component")))
            {
                ensure!(
                    components.len() < MAX_OBJECTS,
                    "3MF has too many components"
                );
                ensure!(
                    !component.attributes().any(|v| v.name() == "path"),
                    "Cross-part 3MF components are not rendered"
                );
                components.push((index(component, "objectid")?, transform(component)?));
            }
            Object::Components(components)
        };
        objects.insert(id, value);
    }
    let mut mesh = Vec::new();
    let mut instances = 0;
    for item in child(model, "build")?
        .children()
        .filter(|v| v.has_tag_name((CORE_3MF, "item")))
    {
        check()?;
        ensure!(
            !item.attributes().any(|v| v.name() == "path"),
            "Cross-part 3MF build items are not rendered"
        );
        expand(
            index(item, "objectid")?,
            &objects,
            &mut vec![transform(item)?],
            &mut Vec::new(),
            &mut instances,
            &mut mesh,
            check,
        )?;
    }
    Ok((mesh, unit.into()))
}

fn expand(
    id: usize,
    objects: &HashMap<usize, Object>,
    transforms: &mut Vec<Transform>,
    stack: &mut Vec<usize>,
    instances: &mut usize,
    output: &mut Vec<Triangle>,
    check: &impl Fn() -> Result<()>,
) -> Result<()> {
    check()?;
    ensure!(
        stack.len() < 32 && !stack.contains(&id),
        "3MF component cycle or excessive nesting"
    );
    *instances += 1;
    ensure!(
        *instances <= MAX_OBJECTS,
        "3MF exceeds the 4,096 instance limit"
    );
    stack.push(id);
    match objects
        .get(&id)
        .context("3MF references a missing object")?
    {
        Object::Mesh(mesh) => {
            for triangle in mesh {
                check()?;
                let mut triangle = *triangle;
                for matrix in transforms.iter().rev() {
                    for point in &mut triangle {
                        *point = apply(*matrix, *point)?;
                    }
                }
                push_triangle(output, triangle)?;
            }
        }
        Object::Components(components) => {
            for (id, matrix) in components {
                transforms.push(*matrix);
                expand(*id, objects, transforms, stack, instances, output, check)?;
                transforms.pop();
            }
        }
    }
    stack.pop();
    Ok(())
}

fn bounds(mesh: &[Triangle]) -> Result<(Point, Point)> {
    let mut minimum = [f64::INFINITY; 3];
    let mut maximum = [f64::NEG_INFINITY; 3];
    for point in mesh.iter().flatten() {
        for i in 0..3 {
            minimum[i] = minimum[i].min(point[i]);
            maximum[i] = maximum[i].max(point[i]);
        }
    }
    ensure!(
        (0..3).any(|i| maximum[i] - minimum[i] > 1e-12),
        "Model geometry has no visible extent"
    );
    Ok((minimum, maximum))
}

fn subtract(a: Point, b: Point) -> Point {
    std::array::from_fn(|i| a[i] - b[i])
}
fn dot(a: Point, b: Point) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn cross(a: Point, b: Point) -> Point {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn normalize(a: Point) -> Point {
    let length = dot(a, a).sqrt();
    a.map(|v| v / length.max(1e-30))
}

fn rasterize(
    mesh: &[Triangle],
    eye: Point,
    up: Point,
    format: &str,
    work: &mut u64,
    check: &impl Fn() -> Result<()>,
) -> Result<ImagePreview> {
    let forward = normalize(eye);
    let right = normalize(cross(up, forward));
    let up = cross(forward, right);
    let project = |point| [dot(point, right), dot(point, up), dot(point, forward)];
    let mut projected = Vec::with_capacity(mesh.len());
    for triangle in mesh {
        check()?;
        projected.push(triangle.map(project));
    }
    let (min, max) = bounds(&projected)?;
    let edge = MODEL_VIEW_EDGE as usize;
    let scale = (MODEL_VIEW_EDGE as f64 - 64.) / (max[0] - min[0]).max(max[1] - min[1]).max(1e-12);
    let center = [(min[0] + max[0]) / 2., (min[1] + max[1]) / 2.];
    let mut rgba = vec![0; edge * edge * 4];
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel.copy_from_slice(&[28, 34, 42, 255]);
    }
    let mut depth = vec![f64::NEG_INFINITY; edge * edge];
    for triangle in projected {
        check()?;
        let normal = normalize(cross(
            subtract(triangle[1], triangle[0]),
            subtract(triangle[2], triangle[0]),
        ));
        let light = 0.28 + 0.72 * dot(normal, normalize([-0.4, 0.6, 1.])).abs();
        let color = [
            ((70. + 66. * light) as u8),
            ((130. + 70. * light) as u8),
            ((149. + 74. * light) as u8),
            255,
        ];
        let p = triangle.map(|v| {
            [
                (v[0] - center[0]) * scale + edge as f64 / 2.,
                edge as f64 / 2. - (v[1] - center[1]) * scale,
                v[2],
            ]
        });
        let signed_area = edge_value(p[0], p[1], p[2]);
        if signed_area.abs() < 1e-8 {
            continue;
        }
        let xmin = p
            .iter()
            .map(|v| v[0])
            .fold(f64::INFINITY, f64::min)
            .floor()
            .clamp(0., edge as f64 - 1.) as usize;
        let xmax = p
            .iter()
            .map(|v| v[0])
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .clamp(0., edge as f64 - 1.) as usize;
        let ymin = p
            .iter()
            .map(|v| v[1])
            .fold(f64::INFINITY, f64::min)
            .floor()
            .clamp(0., edge as f64 - 1.) as usize;
        let ymax = p
            .iter()
            .map(|v| v[1])
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .clamp(0., edge as f64 - 1.) as usize;
        *work += ((xmax - xmin + 1) * (ymax - ymin + 1)) as u64;
        ensure!(
            *work <= MAX_RASTER_SAMPLES,
            "Model overlap exceeds the bounded raster preview budget"
        );
        for y in ymin..=ymax {
            if y % 32 == 0 {
                check()?;
            }
            for x in xmin..=xmax {
                let sample = [x as f64 + 0.5, y as f64 + 0.5, 0.];
                let a = edge_value(p[1], p[2], sample) / signed_area;
                let b = edge_value(p[2], p[0], sample) / signed_area;
                let c = 1. - a - b;
                if a < -1e-8 || b < -1e-8 || c < -1e-8 {
                    continue;
                }
                let z = a * p[0][2] + b * p[1][2] + c * p[2][2];
                let pixel = y * edge + x;
                if z > depth[pixel] {
                    depth[pixel] = z;
                    rgba[pixel * 4..pixel * 4 + 4].copy_from_slice(&color);
                }
            }
        }
    }
    Ok(ImagePreview {
        width: MODEL_VIEW_EDGE,
        height: MODEL_VIEW_EDGE,
        original_width: MODEL_VIEW_EDGE,
        original_height: MODEL_VIEW_EDGE,
        rgba,
        format: format!("{format} mesh"),
    })
}

fn edge_value(a: Point, b: Point, point: Point) -> f64 {
    (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0])
}

#[cfg(test)]
mod tests;
