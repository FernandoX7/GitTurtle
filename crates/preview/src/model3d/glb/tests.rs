use super::*;
use crate::model3d::{ModelCamera, ModelUnits, decode_geometry, is_model_path, render_model};
use serde_json::{Value, json};
use std::{cell::Cell, path::Path};

pub(super) fn pack(document: &Value, bin: &[u8]) -> Vec<u8> {
    pack_json(&serde_json::to_vec(document).unwrap(), Some(bin))
}
fn pack_json(json: &[u8], bin: Option<&[u8]>) -> Vec<u8> {
    let mut json = json.to_vec();
    json.resize(json.len().next_multiple_of(4), b' ');
    let mut bytes = b"glTF".to_vec();
    bytes.extend(2u32.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.extend((json.len() as u32).to_le_bytes());
    bytes.extend(JSON_CHUNK.to_le_bytes());
    bytes.extend(json);
    if let Some(bin) = bin {
        let mut bin = bin.to_vec();
        bin.resize(bin.len().next_multiple_of(4), 0);
        bytes.extend((bin.len() as u32).to_le_bytes());
        bytes.extend(BIN_CHUNK.to_le_bytes());
        bytes.extend(bin);
    }
    let length = bytes.len() as u32;
    bytes[8..12].copy_from_slice(&length.to_le_bytes());
    bytes
}
pub(super) fn triangle() -> (Value, Vec<u8>) {
    let mut bytes = Vec::new();
    for coordinate in [0f32, 0., 0., 2., 0., 0., 0., 3., 4.] {
        bytes.extend(coordinate.to_le_bytes());
    }
    let document = json!({
        "asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],
        "buffers":[{"byteLength":36}],"bufferViews":[{"buffer":0,"byteLength":36}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[2,3,4]}]
    });
    (document, bytes)
}
pub(super) fn decoded(document: &Value, bin: &[u8]) -> crate::model3d::ModelGeometry {
    decode_geometry(&pack(document, bin), "captured.GLB", || Ok(())).unwrap()
}
fn error(document: &Value, bin: &[u8]) -> String {
    format!(
        "{:#}",
        decode_geometry(&pack(document, bin), "bad.glb", || Ok(())).unwrap_err()
    )
}
pub(super) fn expect_error(document: &Value, bin: &[u8], message: &str) {
    let actual = error(document, bin);
    assert!(
        actual.contains(message),
        "expected {message:?}, got {actual}"
    );
}
pub(super) fn add_indices(document: &mut Value, bin: &mut Vec<u8>, kind: u32, values: &[u32]) {
    let start = bin.len().next_multiple_of(4);
    bin.resize(start, 0);
    for &index in values {
        match kind {
            5121 => bin.push(index as u8),
            5123 => bin.extend((index as u16).to_le_bytes()),
            _ => bin.extend(index.to_le_bytes()),
        }
    }
    let length = bin.len() - start;
    document["buffers"][0]["byteLength"] = bin.len().into();
    let views = document["bufferViews"].as_array_mut().unwrap();
    let view = views.len();
    views.push(json!({"buffer":0,"byteOffset":start,"byteLength":length,"target":34963}));
    let accessors = document["accessors"].as_array_mut().unwrap();
    let accessor = accessors.len();
    accessors
        .push(json!({"bufferView":view,"componentType":kind,"type":"SCALAR","count":values.len()}));
    document["meshes"][0]["primitives"][0]["indices"] = accessor.into();
}

#[test]
fn glb_case_insensitive_detection_retained_units_orientation_and_rendering() {
    assert!(is_model_path(Path::new("example.GlB")));
    let (mut document, bin) = triangle();
    document["materials"] = json!([{ "doubleSided": true }]);
    document["meshes"][0]["primitives"][0]["material"] = json!(0);
    let geometry = decoded(&document, &bin);
    assert_eq!(geometry.format, "GLB");
    assert_eq!(geometry.scene.units, ModelUnits::Millimeters);
    assert_eq!(
        geometry.scene.triangles(),
        &[[[0., 0., 0.], [2000., 0., 0.], [0., -4000., 3000.]]]
    );
    assert_eq!(geometry.scene.bounds.minimum, [0., -4000., 0.]);
    assert_eq!(geometry.scene.bounds.maximum, [2000., 0., 3000.]);
    assert!(
        geometry
            .details
            .iter()
            .any(|v| v.contains("animation playback"))
    );
    let camera = ModelCamera::fit(geometry.scene.bounds);
    let image = render_model(&geometry.scene, &camera, 128, false, || Ok(())).unwrap();
    assert_eq!(image.rgba.len(), 128 * 128 * 4);
    assert!(
        image
            .rgba
            .as_chunks::<4>()
            .0
            .windows(2)
            .any(|v| v[0] != v[1])
    );
    let wire = render_model(&geometry.scene, &camera, 128, true, || Ok(())).unwrap();
    assert_ne!(image.rgba, wire.rgba);
}

#[test]
fn glb_indexed_integer_widths_and_nonindexed_geometry_agree() {
    let (document, bin) = triangle();
    let expected = decoded(&document, &bin);
    for kind in [5121, 5123, 5125] {
        let (mut document, mut bin) = triangle();
        add_indices(&mut document, &mut bin, kind, &[0, 1, 2]);
        assert_eq!(
            decoded(&document, &bin).scene.triangles(),
            expected.scene.triangles()
        );
    }
}

#[test]
fn glb_multiple_primitives_instances_nested_trs_and_column_major_matrix() {
    let (mut document, bin) = triangle();
    document["meshes"][0]["primitives"]
        .as_array_mut()
        .unwrap()
        .push(json!({"attributes":{"POSITION":0}}));
    let q = 0.5f64.sqrt();
    document["nodes"] = json!([
        {"translation":[1,2,3],"rotation":[0,0,q,q],"scale":[2,3,4],"children":[1]},
        {"mesh":0,"translation":[2,0,0]},
        {"mesh":0,"matrix":[1,0,0,0,0,1,0,0,0,0,1,0,5,6,7,1]}
    ]);
    document["scenes"][0]["nodes"] = json!([0, 2]);
    let scene = decoded(&document, &bin).scene;
    assert_eq!(scene.triangle_count(), 4);
    for (a, b) in scene.triangles()[0][0].iter().zip([1000., -3000., 6000.]) {
        assert!((*a - b).abs() < 1e-8);
    }
    assert_eq!(scene.triangles()[2][0], [5000., -7000., 6000.]);
}

#[test]
fn glb_authored_negative_and_zero_scale_are_preserved() {
    let (mut document, bin) = triangle();
    document["nodes"][0]["scale"] = json!([-2, 0, 3]);
    let scene = decoded(&document, &bin).scene;
    assert_eq!(scene.triangles()[0][1], [-4000., 0., 0.]);
    assert_eq!(scene.triangles()[0][2], [0., -12000., 0.]);
}

#[test]
fn glb_scene_policy_default_then_first_and_no_implicit_unplaced_meshes() {
    let (mut document, bin) = triangle();
    document["nodes"] = json!([{"mesh":0},{"mesh":0,"translation":[10,0,0]}]);
    document["scenes"] = json!([{"nodes":[0]},{"nodes":[1]}]);
    document["scene"] = json!(1);
    assert_eq!(decoded(&document, &bin).scene.bounds.minimum[0], 10000.);
    document.as_object_mut().unwrap().remove("scene");
    let first = decoded(&document, &bin);
    assert_eq!(first.scene.bounds.minimum[0], 0.);
    assert_eq!(first.scene.triangle_count(), 1);
    assert!(first.details[0].contains("first scene"));
    document["scene"] = json!(99);
    expect_error(&document, &bin, "no selectable scene");
    document.as_object_mut().unwrap().remove("scene");
    document["scenes"] = json!([]);
    expect_error(&document, &bin, "no selectable scene");
}

#[test]
fn glb_interleaved_positions_respect_accessor_offsets_and_stride() {
    let (mut document, _) = triangle();
    let mut bin = vec![0u8; 4];
    for point in [[0f32, 0., 0.], [2., 0., 0.], [0., 3., 4.]] {
        bin.extend([99, 99, 99, 99]);
        for value in point {
            bin.extend(value.to_le_bytes());
        }
        bin.extend([55, 55, 55, 55]);
    }
    document["buffers"][0]["byteLength"] = bin.len().into();
    document["bufferViews"][0] =
        json!({"buffer":0,"byteOffset":4,"byteLength":60,"byteStride":20,"target":34962});
    document["accessors"][0]["byteOffset"] = json!(4);
    let (base, bytes) = triangle();
    assert_eq!(
        decoded(&document, &bin).scene.triangles(),
        decoded(&base, &bytes).scene.triangles()
    );
}

#[test]
fn glb_quantized_position_component_types_normalization_and_alignment() {
    for (kind, normalized) in [
        (5120, false),
        (5120, true),
        (5121, false),
        (5121, true),
        (5122, false),
        (5122, true),
        (5123, false),
        (5123, true),
    ] {
        let (mut document, _) = triangle();
        let size = component_size(kind).unwrap();
        let stride = (3 * size).next_multiple_of(4);
        let mut bin = Vec::new();
        for point in [[0i32, 0, 0], [64, 0, 0], [0, 64, 64]] {
            for component in point {
                if size == 1 {
                    bin.push(component as u8);
                } else {
                    bin.extend((component as u16).to_le_bytes());
                }
            }
            bin.resize(bin.len().next_multiple_of(stride), 0);
        }
        document["buffers"][0]["byteLength"] = bin.len().into();
        document["bufferViews"][0] = json!({"buffer":0,"byteLength":bin.len(),"byteStride":stride});
        document["accessors"][0]["componentType"] = kind.into();
        document["accessors"][0]["normalized"] = normalized.into();
        document["extensionsUsed"] = json!([QUANTIZATION]);
        document["extensionsRequired"] = json!([QUANTIZATION]);
        let divisor = if normalized {
            match kind {
                5120 => 127.,
                5121 => 255.,
                5122 => 32767.,
                _ => 65535.,
            }
        } else {
            1.
        };
        assert_eq!(
            decoded(&document, &bin).scene.triangles()[0][1][0],
            64000. / divisor
        );
        document["extensionsRequired"] = json!([]);
        expect_error(&document, &bin, "required KHR_mesh_quantization");
    }
    let (mut document, mut bin) = triangle();
    document["extensionsUsed"] = json!([QUANTIZATION]);
    document["extensionsRequired"] = json!([QUANTIZATION]);
    document["accessors"][0]["componentType"] = json!(5120);
    document["accessors"][0]["normalized"] = json!(true);
    document["bufferViews"][0]["byteStride"] = json!(4);
    bin[..12].copy_from_slice(&[128, 0, 0, 0, 127, 0, 0, 0, 0, 127, 127, 0]);
    assert_eq!(decoded(&document, &bin).scene.triangles()[0][0][0], -1000.);
    document["bufferViews"][0]
        .as_object_mut()
        .unwrap()
        .remove("byteStride");
    expect_error(&document, &bin, "four-byte aligned");
}

fn sparse_positions(kind: u32, with_base: bool) -> (Value, Vec<u8>) {
    let (mut document, mut bin) = triangle();
    if !with_base {
        document["accessors"][0]
            .as_object_mut()
            .unwrap()
            .remove("bufferView");
    }
    let indices_start = bin.len();
    for index in [1u32, 2] {
        match kind {
            5121 => bin.push(index as u8),
            5123 => bin.extend((index as u16).to_le_bytes()),
            _ => bin.extend(index.to_le_bytes()),
        }
    }
    let indices_length = bin.len() - indices_start;
    bin.resize(bin.len().next_multiple_of(4), 0);
    let values_start = bin.len();
    for value in [5f32, 0., 0., 0., 6., 7.] {
        bin.extend(value.to_le_bytes());
    }
    document["buffers"][0]["byteLength"] = bin.len().into();
    document["bufferViews"].as_array_mut().unwrap().extend([
        json!({"buffer":0,"byteOffset":indices_start,"byteLength":indices_length}),
        json!({"buffer":0,"byteOffset":values_start,"byteLength":24}),
    ]);
    document["accessors"][0]["sparse"] = json!({"count":2,"indices":{"bufferView":1,"componentType":kind},"values":{"bufferView":2}});
    (document, bin)
}

#[test]
fn glb_sparse_positions_override_base_or_zero_storage_for_each_index_width() {
    for kind in [5121, 5123, 5125] {
        for with_base in [false, true] {
            let (document, bin) = sparse_positions(kind, with_base);
            assert_eq!(
                decoded(&document, &bin).scene.triangles()[0],
                [[0., 0., 0.], [5000., 0., 0.], [0., -7000., 6000.]]
            );
        }
    }
}

#[test]
fn glb_sparse_quantized_values_are_tightly_packed() {
    let (mut document, _) = triangle();
    let bin = [0u8, 1, 2, 0, 0, 0, 0, 127, 0, 0, 0, 127, 127];
    document["buffers"][0]["byteLength"] = bin.len().into();
    document["bufferViews"] =
        json!([{"buffer":0,"byteLength":3},{"buffer":0,"byteOffset":4,"byteLength":9}]);
    document["accessors"][0] = json!({"componentType":5120,"type":"VEC3","count":3,"normalized":true,"sparse":{"count":3,"indices":{"bufferView":0,"componentType":5121},"values":{"bufferView":1}}});
    document["extensionsUsed"] = json!([QUANTIZATION]);
    document["extensionsRequired"] = json!([QUANTIZATION]);
    assert_eq!(
        decoded(&document, &bin).scene.triangles()[0],
        [[0., 0., 0.], [1000., 0., 0.], [0., -1000., 1000.]]
    );
}

#[test]
fn glb_sparse_triangle_indices_apply_before_validation_and_expansion() {
    let (mut document, mut bin) = triangle();
    add_indices(&mut document, &mut bin, 5123, &[0, 0, 0]);
    bin.resize(bin.len().next_multiple_of(4), 0);
    let offset = bin.len();
    bin.extend([1, 2, 0, 0, 1, 0, 2, 0]);
    document["buffers"][0]["byteLength"] = bin.len().into();
    document["bufferViews"].as_array_mut().unwrap().extend([
        json!({"buffer":0,"byteOffset":offset,"byteLength":2}),
        json!({"buffer":0,"byteOffset":offset+4,"byteLength":4}),
    ]);
    document["accessors"][1]["sparse"] = json!({"count":2,"indices":{"bufferView":2,"componentType":5121},"values":{"bufferView":3}});
    let (base, bytes) = triangle();
    assert_eq!(
        decoded(&document, &bin).scene.triangles(),
        decoded(&base, &bytes).scene.triangles()
    );
}

#[test]
fn glb_sparse_refuses_invalid_order_counts_views_and_ranges() {
    for indices in [[2u8, 1], [1, 1], [1, 3]] {
        let (document, mut bin) = sparse_positions(5121, true);
        bin[36..38].copy_from_slice(&indices);
        expect_error(&document, &bin, "strictly increasing");
    }
    for (field, value, message) in [
        ("count", json!(4), "sparse accessor count"),
        ("count", json!(0), "sparse accessor count"),
    ] {
        let (mut document, bin) = sparse_positions(5121, false);
        document["accessors"][0]["sparse"][field] = value;
        expect_error(&document, &bin, message);
    }
    for (field, value) in [("target", json!(34962)), ("byteStride", json!(4))] {
        let (mut document, bin) = sparse_positions(5121, true);
        document["bufferViews"][1][field] = value;
        expect_error(&document, &bin, "cannot declare");
    }
    let (mut document, bin) = sparse_positions(5121, true);
    document["bufferViews"][2]["byteLength"] = json!(20);
    expect_error(&document, &bin, "accessor range");
}

#[test]
fn glb_container_checks_every_truncation_header_length_and_chunk_rule() {
    let (document, bin) = triangle();
    let valid = pack(&document, &bin);
    for end in 0..valid.len() {
        assert!(
            decode_geometry(&valid[..end], "bad.glb", || Ok(())).is_err(),
            "accepted truncation at {end}"
        );
    }
    for (offset, value, message) in [
        (0, 0, "header"),
        (4, 1, "version"),
        (8, 999, "length"),
        (12, u32::MAX, "aligned"),
        (16, BIN_CHUNK, "begin"),
    ] {
        let mut bytes = valid.clone();
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        let error = decode_geometry(&bytes, "bad.glb", || Ok(()))
            .unwrap_err()
            .to_string();
        assert!(error.contains(message), "{error}");
    }
    let mut unknown = valid.clone();
    unknown.extend(4u32.to_le_bytes());
    unknown.extend(12345u32.to_le_bytes());
    unknown.extend([0; 4]);
    let length = unknown.len() as u32;
    unknown[8..12].copy_from_slice(&length.to_le_bytes());
    assert!(decode_geometry(&unknown, "unknown.glb", || Ok(())).is_ok());
    for kind in [JSON_CHUNK, BIN_CHUNK] {
        let mut duplicate = valid.clone();
        duplicate.extend(0u32.to_le_bytes());
        duplicate.extend(kind.to_le_bytes());
        let length = duplicate.len() as u32;
        duplicate[8..12].copy_from_slice(&length.to_le_bytes());
        assert!(
            decode_geometry(&duplicate, "duplicate.glb", || Ok(()))
                .unwrap_err()
                .to_string()
                .contains("Duplicate or misplaced")
        );
    }
}

#[test]
fn glb_bin_padding_and_asset_versions_are_checked() {
    let (mut document, mut bin) = triangle();
    add_indices(&mut document, &mut bin, 5121, &[0, 1, 2]);
    assert!(decode_geometry(&pack(&document, &bin), "padding.glb", || Ok(())).is_ok());
    let mut bytes = pack(&document, &bin);
    *bytes.last_mut().unwrap() = 1;
    assert!(
        decode_geometry(&bytes, "bad.glb", || Ok(()))
            .unwrap_err()
            .to_string()
            .contains("padding")
    );
    for (field, value) in [
        ("version", "1.0"),
        ("version", "2.1"),
        ("minVersion", "2.1"),
    ] {
        let (mut document, bin) = triangle();
        document["asset"][field] = json!(value);
        expect_error(&document, &bin, "asset version");
    }
    let (mut document, bin) = triangle();
    document["buffers"][0]["byteLength"] = json!(32);
    expect_error(&document, &bin, "BIN length");
}

#[test]
fn glb_refuses_external_geometry_and_omits_appearance_resources() {
    let (mut document, bin) = triangle();
    document["buffers"]
        .as_array_mut()
        .unwrap()
        .push(json!({"byteLength":36,"uri":"https://example.invalid/model.bin"}));
    document["bufferViews"][0]["buffer"] = json!(1);
    expect_error(&document, &bin, "external or URI buffer");
    document["bufferViews"][0]["buffer"] = json!(0);
    document["images"] = json!([{"uri":"file:///nonexistent/image.png"},{"uri":"https://example.invalid/image.png"}]);
    document["extensionsUsed"] = json!(["KHR_texture_basisu", "KHR_materials_unlit"]);
    document["extensionsRequired"] = json!(["KHR_texture_basisu"]);
    document["textures"] = json!([{"extensions":{"KHR_texture_basisu":{"source":0}}}]);
    assert_eq!(decoded(&document, &bin).scene.triangle_count(), 1);
}

#[test]
fn glb_refuses_geometry_extensions_even_optional_undeclared_or_on_unselected_nodes() {
    for extension in [
        "KHR_draco_mesh_compression",
        "KHR_meshopt_compression",
        "EXT_mesh_gpu_instancing",
        "KHR_node_visibility",
        "KHR_gaussian_splatting",
        "vendor_unknown",
    ] {
        let (mut document, bin) = triangle();
        document["extensionsUsed"] = json!([extension]);
        expect_error(&document, &bin, extension);
        let (mut document, bin) = triangle();
        document["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"extensions":{extension:{}}}));
        expect_error(&document, &bin, extension);
    }
    let (mut document, bin) = triangle();
    document["extensionsRequired"] = json!([QUANTIZATION]);
    expect_error(&document, &bin, "extensionsUsed");
}

#[test]
fn glb_refuses_malformed_deformation_and_unsupported_modes_without_partial_meshes() {
    for (field, value, message) in [("skin", json!(0), "skin"), ("weights", json!([0]), "morph")] {
        let (mut document, bin) = triangle();
        document["nodes"][0][field] = value;
        expect_error(&document, &bin, message);
    }
    let (mut document, bin) = triangle();
    document["skins"] = json!([{}]);
    expect_error(&document, &bin, "Invalid GLB glTF JSON");
    let (mut document, bin) = triangle();
    document["meshes"][0]["weights"] = json!([0]);
    expect_error(&document, &bin, "morph");
    let (mut document, bin) = triangle();
    document["meshes"][0]["primitives"][0]["targets"] = json!([{"POSITION":0}]);
    assert_eq!(decoded(&document, &bin).scene.triangle_count(), 1);
    for mode in [0, 1, 2, 3, 5, 6, 7] {
        let (mut document, bin) = triangle();
        document["meshes"][0]["primitives"]
            .as_array_mut()
            .unwrap()
            .push(json!({"attributes":{"POSITION":0},"mode":mode}));
        expect_error(&document, &bin, "only TRIANGLES");
    }
}

#[test]
fn glb_refuses_bad_layouts_counts_ranges_finite_coordinates_and_indices() {
    for (field, value, message) in [
        ("type", json!("VEC2"), "min/max"),
        ("componentType", json!(5125), "POSITION supports"),
        ("normalized", json!(true), "normalized component"),
        ("byteOffset", json!(1), "Misaligned"),
        ("count", json!(4), "range"),
        ("count", json!(usize::MAX), "300,000"),
        ("min", json!("invalid"), "Invalid GLB glTF JSON"),
    ] {
        let (mut document, bin) = triangle();
        document["accessors"][0][field] = value;
        expect_error(&document, &bin, message);
    }
    for stride in [0, 3, 8, 253, usize::MAX] {
        let (mut document, bin) = triangle();
        document["bufferViews"][0]["byteStride"] = stride.into();
        expect_error(&document, &bin, "byteStride");
    }
    for value in [f32::NAN, f32::INFINITY, f32::MAX] {
        let (document, mut bin) = triangle();
        bin[0..4].copy_from_slice(&value.to_le_bytes());
        expect_error(&document, &bin, "coordinates");
    }
    for indices in [[0, 1, 3], [0, 1, 255]] {
        let (mut document, mut bin) = triangle();
        add_indices(&mut document, &mut bin, 5121, &indices);
        expect_error(&document, &bin, "triangle index");
    }
    // Restart sentinels are forbidden even when they fall inside POSITION's
    // count, so this must fail before the zero-initialized geometry is expanded.
    for (kind, vertices, sentinel) in [(5121, 256, 255), (5123, 65536, 65535)] {
        let (mut document, mut bin) = triangle();
        document["accessors"][0]
            .as_object_mut()
            .unwrap()
            .remove("bufferView");
        document["accessors"][0]["count"] = json!(vertices);
        add_indices(&mut document, &mut bin, kind, &[0, 1, sentinel]);
        expect_error(&document, &bin, "primitive-restart");
    }
    let (mut document, mut bin) = triangle();
    add_indices(&mut document, &mut bin, 5121, &[0, 1]);
    expect_error(&document, &bin, "divisible by three");
}

#[test]
fn glb_refs_include_ignored_attributes_and_unused_meshes() {
    let (mut document, bin) = triangle();
    document["meshes"][0]["primitives"][0]["attributes"]["NORMAL"] = json!(99);
    expect_error(&document, &bin, "missing accessor");
    let (mut document, bin) = triangle();
    document["meshes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"primitives":[{"attributes":{"POSITION":99}}]}));
    expect_error(&document, &bin, "missing accessor");
    let (mut document, bin) = triangle();
    document["meshes"][0]["primitives"][0]["material"] = json!(99);
    assert!(
        decoded(&document, &bin)
            .details
            .iter()
            .any(|v| v.contains("missing material"))
    );
    let (mut document, bin) = triangle();
    document["accessors"]
        .as_array_mut()
        .unwrap()
        .push(json!({"componentType":5126,"type":"VEC3","count":2}));
    document["meshes"][0]["primitives"][0]["attributes"]["NORMAL"] = json!(1);
    expect_error(&document, &bin, "counts do not match");
}

#[test]
fn glb_tree_and_transform_validation_refuses_cycles_multiple_parents_and_invalid_matrices() {
    for nodes in [
        json!([{"mesh":0,"children":[0]}]),
        json!([{"children":[1]},{"children":[0]}]),
        json!([{"children":[1,1]},{"mesh":0}]),
        json!([{"children":[2]},{"children":[2]},{"mesh":0}]),
        json!([{"children":[99]}]),
    ] {
        let (mut document, bin) = triangle();
        document["nodes"] = nodes;
        assert!(decode_geometry(&pack(&document, &bin), "bad.glb", || Ok(())).is_err());
    }
    let (mut document, bin) = triangle();
    document["scenes"][0]["nodes"] = json!([0, 0]);
    expect_error(&document, &bin, "Duplicate GLB scene root");
    let (mut document, bin) = triangle();
    document["nodes"][0]["rotation"] = json!([0, 0, 0, 2]);
    expect_error(&document, &bin, "unit quaternion");
    let (mut document, bin) = triangle();
    document["nodes"][0]["matrix"] = json!([1, 0, 0, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
    expect_error(&document, &bin, "shear");
    document["nodes"][0]["translation"] = json!([0, 0, 0]);
    expect_error(&document, &bin, "combine matrix and TRS");
}

#[test]
fn glb_json_duplicate_keys_depth_tokens_arrays_strings_and_arbitrary_extras() {
    let (mut document, bin) = triangle();
    document["extras"] = json!({"extensions":{"vendorApplicationMetadata":{"version":1}}});
    assert!(decode_geometry(&pack(&document, &bin), "extras.glb", || Ok(())).is_ok());
    let text = serde_json::to_string(&document).unwrap();
    for prefix in ["{\"asset\":{},", "{\"ass\\u0065t\":{},"] {
        let duplicate = format!("{prefix}{}", &text[1..]);
        let bytes = pack_json(duplicate.as_bytes(), Some(&bin));
        assert!(
            decode_geometry(&bytes, "dup.glb", || Ok(()))
                .unwrap_err()
                .to_string()
                .contains("Duplicate")
        );
    }
    let nested = format!("{}0{}", "[".repeat(34), "]".repeat(34));
    assert!(
        JsonPreflight::new(nested.as_bytes(), &|| Ok(()))
            .run()
            .unwrap_err()
            .to_string()
            .contains("nesting")
    );
    let array = format!("[{}]", vec!["0"; MAX_JSON_ARRAY + 1].join(","));
    assert!(
        JsonPreflight::new(array.as_bytes(), &|| Ok(()))
            .run()
            .unwrap_err()
            .to_string()
            .contains("array")
    );
    let text = format!("\"{}\"", "a".repeat(MAX_JSON_STRING + 1));
    assert!(
        JsonPreflight::new(text.as_bytes(), &|| Ok(()))
            .run()
            .unwrap_err()
            .to_string()
            .contains("string")
    );
    let blocks = vec![format!("[{}]", vec!["0"; 1000].join(",")); 251];
    let tokens = format!("[{}]", blocks.join(","));
    assert!(
        JsonPreflight::new(tokens.as_bytes(), &|| Ok(()))
            .run()
            .unwrap_err()
            .to_string()
            .contains("token")
    );
    let oversized = vec![b' '; MAX_JSON_BYTES + 4];
    assert!(
        decode_geometry(&pack_json(&oversized, None), "large.glb", || Ok(()))
            .unwrap_err()
            .to_string()
            .contains("4 MiB")
    );
}

#[test]
fn glb_ignored_matrix_accessor_accepts_missing_final_column_padding() {
    for (kind, component, size) in [("MAT2", 5121, 6), ("MAT3", 5121, 11), ("MAT3", 5123, 22)] {
        let (mut document, mut bin) = triangle();
        let offset = bin.len();
        bin.resize(offset + size, 0);
        document["buffers"][0]["byteLength"] = bin.len().into();
        document["bufferViews"]
            .as_array_mut()
            .unwrap()
            .push(json!({"buffer":0,"byteOffset":offset,"byteLength":size}));
        document["accessors"]
            .as_array_mut()
            .unwrap()
            .push(json!({"bufferView":1,"componentType":component,"count":1,"type":kind}));
        assert_eq!(decoded(&document, &bin).scene.triangle_count(), 1);
    }
}

#[test]
fn glb_node_depth_count_and_instance_triangle_amplification_are_bounded() {
    let (mut document, bin) = triangle();
    document["nodes"] = (0..33)
        .map(|index| {
            if index == 32 {
                json!({"mesh":0})
            } else {
                json!({"children":[index+1]})
            }
        })
        .collect();
    expect_error(&document, &bin, "32-level");
    document["nodes"] = (0..4097).map(|_| json!({"mesh":0})).collect();
    expect_error(&document, &bin, "preview limits");
    let (mut document, mut bin) = triangle();
    let indices: Vec<u32> = (0..75).map(|index| index % 3).collect();
    add_indices(&mut document, &mut bin, 5121, &indices);
    document["nodes"] = (0..4096).map(|_| json!({"mesh":0})).collect();
    document["scenes"][0]["nodes"] = (0..4096).collect();
    expect_error(&document, &bin, "expanded triangle");
}

#[test]
fn glb_cumulative_accessor_decode_work_is_bounded_independently_of_output() {
    let (mut document, mut bin) = triangle();
    add_indices(&mut document, &mut bin, 5121, &[0, 1, 2]);
    document["accessors"][0]
        .as_object_mut()
        .unwrap()
        .remove("bufferView");
    document["accessors"][0]["count"] = json!(300000);
    let primitive = document["meshes"][0]["primitives"][0].clone();
    document["meshes"][0]["primitives"]
        .as_array_mut()
        .unwrap()
        .push(primitive);
    expect_error(&document, &bin, "cumulative decoded");
}

#[test]
fn glb_cancellation_during_json_accessors_instances_and_rendering() {
    let (mut document, bin) = triangle();
    document["nodes"] = (0..100).map(|_| json!({"mesh":0})).collect();
    document["scenes"][0]["nodes"] = (0..100).collect();
    let bytes = pack(&document, &bin);
    let total = Cell::new(0usize);
    decode_geometry(&bytes, "count.glb", || {
        total.set(total.get() + 1);
        Ok(())
    })
    .unwrap();
    for stop in [1, 5, 20, 100, total.get() - 2] {
        let count = Cell::new(0usize);
        let result = decode_geometry(&bytes, "cancel.glb", || {
            count.set(count.get() + 1);
            ensure!(count.get() < stop, "cancelled GLB test");
            Ok(())
        });
        assert!(result.unwrap_err().to_string().contains("cancelled"));
        assert_eq!(count.get(), stop);
    }
    let (mut document, bin) = triangle();
    document["accessors"][0]
        .as_object_mut()
        .unwrap()
        .remove("bufferView");
    document["accessors"][0]["count"] = json!(299997);
    let bytes = pack(&document, &bin);
    let count = Cell::new(0usize);
    let result = decode_geometry(&bytes, "cancel.glb", || {
        count.set(count.get() + 1);
        ensure!(count.get() < 40, "cancelled GLB test");
        Ok(())
    });
    assert!(result.unwrap_err().to_string().contains("cancelled"));
}

#[test]
fn glb_permissive_real_assets_and_changed_assembly_bounds() {
    let fixtures: [(&str, &[u8], usize); 3] = [
        (
            "BoxInterleaved.glb",
            include_bytes!("../../../tests/fixtures/models/glb/BoxInterleaved.glb"),
            12,
        ),
        (
            "Avocado.glb",
            include_bytes!("../../../tests/fixtures/models/glb/Avocado.glb"),
            682,
        ),
        (
            "assembly-before.glb",
            include_bytes!("../../../tests/fixtures/models/glb/assembly-before.glb"),
            36,
        ),
    ];
    for (name, bytes, count) in fixtures {
        let scene = decode_geometry(bytes, name, || Ok(())).unwrap().scene;
        assert_eq!(scene.triangle_count(), count);
        assert!(scene.retained_bytes() >= count * 72);
    }
    let before = decode_geometry(
        include_bytes!("../../../tests/fixtures/models/glb/assembly-before.glb"),
        "before.glb",
        || Ok(()),
    )
    .unwrap()
    .scene;
    let after = decode_geometry(
        include_bytes!("../../../tests/fixtures/models/glb/assembly-after.glb"),
        "after.glb",
        || Ok(()),
    )
    .unwrap()
    .scene;
    for (actual, expected) in [
        (before.bounds.minimum, [-1200., -300., 0.]),
        (before.bounds.maximum, [1200., 300., 2400.]),
        (after.bounds.minimum, [-300., -450., 0.]),
        (after.bounds.maximum, [3300., 450., 3600.]),
    ] {
        for (a, b) in actual.iter().zip(expected) {
            assert!((*a - b).abs() < 0.001, "{actual:?}");
        }
    }
    let union = before.bounds.union(after.bounds);
    assert!(union.maximum[0] > before.bounds.maximum[0]);
    assert!(union.minimum[0] < after.bounds.minimum[0]);
}

#[test]
fn glb_checked_in_refusals_explain_unsupported_content() {
    for (name, bytes, message) in [
        (
            "external-buffer.glb",
            include_bytes!("../../../tests/fixtures/models/glb/external-buffer.glb").as_slice(),
            "external",
        ),
        (
            "compressed.glb",
            include_bytes!("../../../tests/fixtures/models/glb/compressed.glb").as_slice(),
            "compression",
        ),
        (
            "morph-target.glb",
            include_bytes!("../../../tests/fixtures/models/glb/morph-target.glb").as_slice(),
            "inconsistent morph target counts",
        ),
        (
            "skinned.glb",
            include_bytes!("../../../tests/fixtures/models/glb/skinned.glb").as_slice(),
            "requires JOINTS_0",
        ),
    ] {
        let error = decode_geometry(bytes, name, || Ok(()))
            .unwrap_err()
            .to_string();
        assert!(error.contains(message), "{name}: {error}");
    }
}
