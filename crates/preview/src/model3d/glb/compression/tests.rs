use super::super::tests::{add_indices, decoded, expect_error, triangle};
use super::*;
use crate::model3d::{ModelCamera, decode_geometry};
use serde_json::{Value, json};
use std::cell::Cell;

fn encode(bytes: &[u8], count: usize, stride: usize, mode: &str) -> Vec<u8> {
    assert_eq!(bytes.len(), count * stride);
    if mode == "ATTRIBUTES" {
        let bound = unsafe { meshopt::ffi::meshopt_encodeVertexBufferBound(count, stride) };
        let mut result = vec![0; bound];
        let length = unsafe {
            meshopt::ffi::meshopt_encodeVertexBufferLevel(
                result.as_mut_ptr(),
                result.len(),
                bytes.as_ptr().cast(),
                count,
                stride,
                2,
                0,
            )
        };
        assert!(length > 0);
        result.truncate(length);
        result
    } else {
        let indices: Vec<u32> = bytes
            .chunks_exact(stride)
            .map(|v| {
                if stride == 2 {
                    u16::from_le_bytes(v.try_into().unwrap()) as u32
                } else {
                    u32::from_le_bytes(v.try_into().unwrap())
                }
            })
            .collect();
        encode_indices(&indices, mode)
    }
}
fn encode_indices(indices: &[u32], mode: &str) -> Vec<u8> {
    if mode == "TRIANGLES" {
        meshopt::encode_index_buffer(indices, indices.iter().copied().max().unwrap() as usize + 1)
            .unwrap()
    } else {
        let bound = unsafe {
            meshopt::ffi::meshopt_encodeIndexSequenceBound(
                indices.len(),
                indices.iter().copied().max().unwrap() as usize + 1,
            )
        };
        let mut bytes = vec![0; bound];
        let length = unsafe {
            meshopt::ffi::meshopt_encodeIndexSequence(
                bytes.as_mut_ptr(),
                bytes.len(),
                indices.as_ptr(),
                indices.len(),
            )
        };
        assert!(length > 0);
        bytes.truncate(length);
        bytes
    }
}
fn compress_view(
    document: &mut Value,
    bin: &mut Vec<u8>,
    index: usize,
    count: usize,
    stride: usize,
    mode: &str,
    filter: &str,
) {
    let source = &document["bufferViews"][index];
    let start = source["byteOffset"].as_u64().unwrap_or(0) as usize;
    let length = source["byteLength"].as_u64().unwrap() as usize;
    let encoded = encode(&bin[start..start + length], count, stride, mode);
    attach(document, bin, index, count, stride, mode, filter, &encoded);
}
#[allow(clippy::too_many_arguments)]
fn attach(
    document: &mut Value,
    bin: &mut Vec<u8>,
    index: usize,
    count: usize,
    stride: usize,
    mode: &str,
    filter: &str,
    encoded: &[u8],
) {
    if document["buffers"].as_array().unwrap().len() == 1 {
        document["buffers"]
            .as_array_mut()
            .unwrap()
            .push(json!({"byteLength":bin.len(),"extensions":{EXTENSION:{"fallback":true}}}));
    }
    let start = bin.len();
    bin.extend(encoded);
    document["buffers"][0]["byteLength"] = bin.len().into();
    let source = &mut document["bufferViews"][index];
    source["buffer"] = 1.into();
    source["extensions"] = json!({EXTENSION:{"buffer":0,"byteOffset":start,"byteLength":encoded.len(),"byteStride":stride,"count":count,"mode":mode,"filter":filter}});
    for key in ["extensionsUsed", "extensionsRequired"] {
        if document.get(key).is_none() {
            document[key] = json!([]);
        }
        if !document[key]
            .as_array()
            .unwrap()
            .contains(&Value::from(EXTENSION))
        {
            document[key].as_array_mut().unwrap().push(EXTENSION.into());
        }
    }
}
fn compressed_triangle() -> (Value, Vec<u8>) {
    let (mut document, mut bin) = triangle();
    compress_view(&mut document, &mut bin, 0, 3, 12, "ATTRIBUTES", "NONE");
    (document, bin)
}

#[test]
fn meshopt_modes_preserve_exact_geometry_units_transforms_and_instances() {
    for mode in ["TRIANGLES", "INDICES"] {
        for stride in [2, 4] {
            let (mut document, mut bin) = triangle();
            add_indices(
                &mut document,
                &mut bin,
                if stride == 2 { 5123 } else { 5125 },
                &[0, 1, 2],
            );
            document["nodes"] = json!([{"mesh":0,"translation":[1,2,3],"scale":[2,3,4]}, {"mesh":0,"translation":[-2,0,1]}]);
            document["scenes"][0]["nodes"] = json!([0, 1]);
            let expected = decoded(&document, &bin);
            compress_view(&mut document, &mut bin, 0, 3, 12, "ATTRIBUTES", "NONE");
            compress_view(&mut document, &mut bin, 1, 3, stride, mode, "NONE");
            let actual = decoded(&document, &bin);
            assert_eq!(actual.scene.triangles(), expected.scene.triangles());
            assert_eq!(actual.scene.bounds, expected.scene.bounds);
            assert_eq!(actual.scene.units, expected.scene.units);
            assert_eq!(
                actual.scene.retained_bytes(),
                expected.scene.retained_bytes()
            );
        }
    }
}

#[test]
fn meshopt_quantized_interleaved_and_shared_views_preserve_positions() {
    for (kind, normalized) in [
        (5120, false),
        (5120, true),
        (5121, true),
        (5122, false),
        (5122, true),
        (5123, true),
    ] {
        let (mut document, _) = triangle();
        let width = if kind < 5122 { 1 } else { 2 };
        let stride = 12;
        let mut bin = Vec::new();
        for point in [[0, 0, 0], [64, 0, 0], [0, 32, 64]] {
            bin.extend([42, 42, 42, 42]);
            for value in point {
                if width == 1 {
                    bin.push(value as u8);
                } else {
                    bin.extend((value as u16).to_le_bytes());
                }
            }
            bin.resize(bin.len().next_multiple_of(stride), 0);
        }
        document["buffers"][0]["byteLength"] = bin.len().into();
        document["bufferViews"][0] = json!({"buffer":0,"byteLength":bin.len(),"byteStride":stride});
        document["accessors"][0]["componentType"] = kind.into();
        document["accessors"][0]["byteOffset"] = 4.into();
        document["accessors"][0]["normalized"] = normalized.into();
        document["extensionsUsed"] = json!([super::super::QUANTIZATION]);
        document["extensionsRequired"] = document["extensionsUsed"].clone();
        document["meshes"][0]["primitives"]
            .as_array_mut()
            .unwrap()
            .push(json!({"attributes":{"POSITION":0}}));
        let expected = decoded(&document, &bin);
        compress_view(&mut document, &mut bin, 0, 3, stride, "ATTRIBUTES", "NONE");
        assert_eq!(
            decoded(&document, &bin).scene.triangles(),
            expected.scene.triangles()
        );
    }
}

#[test]
fn meshopt_filters_match_explicit_khronos_values() {
    let cases = [
        (4, "OCTAHEDRAL", vec![127, 0, 127, 85], vec![127, 0, 0, 85]),
        (
            8,
            "OCTAHEDRAL",
            [0i16, 32767, 32767, 77]
                .into_iter()
                .flat_map(i16::to_le_bytes)
                .collect(),
            [0i16, 32767, 0, 77]
                .into_iter()
                .flat_map(i16::to_le_bytes)
                .collect(),
        ),
        (
            8,
            "QUATERNION",
            [0i16, 0, 0, 32767]
                .into_iter()
                .flat_map(i16::to_le_bytes)
                .collect(),
            [0i16, 0, 0, 32767]
                .into_iter()
                .flat_map(i16::to_le_bytes)
                .collect(),
        ),
        (
            12,
            "EXPONENTIAL",
            [1u32, 0x01000002, 0xffffffff]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect(),
            [1f32, 4., -0.5]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect(),
        ),
    ];
    for (stride, filter, input, expected) in cases {
        // Multiple filter blocks and an odd tail exercise SIMD tail dispatch.
        let count = FILTER_BLOCK_ELEMENTS + 3;
        let source = Compression {
            buffer: 0,
            byte_offset: 0,
            byte_length: 1,
            byte_stride: stride,
            count,
            mode: "ATTRIBUTES".into(),
            filter: Some(filter.into()),
        };
        let encoded = encode(&input.repeat(count), count, stride, "ATTRIBUTES");
        let actual = decode_view(&source, &encoded, &|| Ok(())).unwrap();
        assert_eq!(actual.bytes(), expected.repeat(count));
    }
}

#[test]
fn meshopt_exponential_positions_and_oct_quantization_use_filtered_bytes() {
    let (mut document, _) = triangle();
    let mut bin = [0u32, 0, 0, 2, 0, 0, 0, 3, 4]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    compress_view(
        &mut document,
        &mut bin,
        0,
        3,
        12,
        "ATTRIBUTES",
        "EXPONENTIAL",
    );
    let expected = decoded(&triangle().0, &triangle().1);
    assert_eq!(
        decoded(&document, &bin).scene.triangles(),
        expected.scene.triangles()
    );
    let (mut document, mut bin) = triangle();
    bin.clear();
    bin.extend([127, 0, 127, 0, 0, 127, 127, 0, 0, 0, 127, 0]);
    document["buffers"][0]["byteLength"] = 12.into();
    document["bufferViews"][0] = json!({"buffer":0,"byteLength":12,"byteStride":4});
    document["accessors"][0]["componentType"] = 5120.into();
    document["accessors"][0]["normalized"] = true.into();
    document["extensionsUsed"] = json!([super::super::QUANTIZATION]);
    document["extensionsRequired"] = document["extensionsUsed"].clone();
    compress_view(&mut document, &mut bin, 0, 3, 4, "ATTRIBUTES", "OCTAHEDRAL");
    assert_eq!(
        decoded(&document, &bin).scene.triangles(),
        &[[[1000., 0., 0.], [0., 0., 1000.], [0., -1000., 0.]]]
    );
}

#[test]
fn meshopt_sparse_indices_and_values_preserve_implicit_base() {
    let (mut document, mut bin) = triangle();
    document["accessors"][0]
        .as_object_mut()
        .unwrap()
        .remove("bufferView");
    bin.extend([0, 0, 1, 0, 2, 0]);
    document["buffers"][0]["byteLength"] = bin.len().into();
    document["bufferViews"]
        .as_array_mut()
        .unwrap()
        .push(json!({"buffer":0,"byteOffset":36,"byteLength":6}));
    document["accessors"][0]["sparse"] = json!({"count":3,"indices":{"bufferView":1,"componentType":5123},"values":{"bufferView":0}});
    let expected = decoded(&document, &bin);
    compress_view(&mut document, &mut bin, 0, 3, 12, "ATTRIBUTES", "NONE");
    compress_view(&mut document, &mut bin, 1, 3, 2, "INDICES", "NONE");
    assert_eq!(
        decoded(&document, &bin).scene.triangles(),
        expected.scene.triangles()
    );
}

#[test]
fn meshopt_fallbacks_are_descriptors_never_substitutes_for_bad_streams() {
    let (original, original_bin) = triangle();
    for uri in [
        None,
        Some("https://invalid.example/never-fetch.bin"),
        Some("file:///never-open.bin"),
        Some("data:application/octet-stream;base64,AAAA"),
    ] {
        let (mut document, mut bin) = compressed_triangle();
        if let Some(uri) = uri {
            document["buffers"][1]["uri"] = uri.into();
            document["extensionsRequired"] = json!([]);
        }
        assert_eq!(
            decoded(&document, &bin).scene.triangles(),
            decoded(&original, &original_bin).scene.triangles()
        );
        let offset = document["bufferViews"][0]["extensions"][EXTENSION]["byteOffset"]
            .as_u64()
            .unwrap() as usize;
        bin[offset] = 0;
        expect_error(&document, &bin, "bitstream header");
    }
    let (mut document, bin) = compressed_triangle();
    document["bufferViews"][0]["buffer"] = 0.into();
    document["buffers"].as_array_mut().unwrap().truncate(1);
    document["extensionsRequired"] = json!([]);
    assert_eq!(decoded(&document, &bin).scene.triangle_count(), 1);
    let (mut document, bin) = compressed_triangle();
    document["extensionsRequired"] = json!([]);
    expect_error(&document, &bin, "placeholder buffer requires");
    let (mut document, bin) = compressed_triangle();
    document["buffers"][1]["byteLength"] = 1.into();
    expect_error(&document, &bin, "range exceeds its buffer");
    let (mut document, bin) = compressed_triangle();
    document["buffers"][0]["extensions"] = json!({EXTENSION:{"fallback":true}});
    expect_error(&document, &bin, "fallback buffer must only");
    let (mut document, bin) = compressed_triangle();
    document["buffers"]
        .as_array_mut()
        .unwrap()
        .push(json!({"byteLength":bin.len(),"uri":"https://invalid.example/never-fetch.bin"}));
    document["bufferViews"][0]["extensions"][EXTENSION]["buffer"] = 2.into();
    expect_error(&document, &bin, "external resources are never loaded");
}

#[test]
fn meshopt_validates_layouts_declarations_references_and_overflow() {
    for (key, value, message) in [
        ("mode", json!("POINTS"), "compression mode"),
        ("filter", json!("COLOR"), "meshopt filter"),
        ("filter", json!("OCTAHEDRAL"), "OCTAHEDRAL filter requires"),
        ("filter", json!("QUATERNION"), "QUATERNION filter requires"),
        ("count", json!(0), "count must be positive"),
        ("count", json!(4), "output length disagrees"),
        ("count", json!(usize::MAX), "output overflow"),
        ("byteStride", json!(3), "ATTRIBUTES stride"),
        ("byteStride", json!(260), "ATTRIBUTES stride"),
        ("byteLength", json!(0), "length must be positive"),
        ("byteOffset", json!(usize::MAX), "compressed range"),
        ("buffer", json!(9), "missing compressed buffer"),
    ] {
        let (mut document, bin) = compressed_triangle();
        document["bufferViews"][0]["extensions"][EXTENSION][key] = value;
        expect_error(&document, &bin, message);
    }
    let (mut document, bin) = compressed_triangle();
    document["bufferViews"][0]["byteStride"] = 16.into();
    expect_error(&document, &bin, "stride disagrees");
    let (mut document, bin) = compressed_triangle();
    document["extensionsUsed"] = json!([]);
    document["extensionsRequired"] = json!([]);
    expect_error(&document, &bin, "extensionsUsed declaration");
    let (mut document, bin) = triangle();
    document["nodes"][0]["extensions"] = json!({EXTENSION:{}});
    expect_error(&document, &bin, "only valid on buffers or buffer views");
    for (mode, count, stride, filter) in [
        ("INDICES", 3, 1, "NONE"),
        ("TRIANGLES", 4, 2, "NONE"),
        ("INDICES", 3, 2, "EXPONENTIAL"),
    ] {
        let source = Compression {
            buffer: 0,
            byte_offset: 0,
            byte_length: 1,
            count,
            byte_stride: stride,
            mode: mode.into(),
            filter: Some(filter.into()),
        };
        assert!(source.validate_layout().is_err());
    }
}

#[test]
fn meshopt_rejects_truncation_newer_versions_and_out_of_width_indices() {
    for mode in ["ATTRIBUTES", "TRIANGLES", "INDICES"] {
        let stride = if mode == "ATTRIBUTES" { 12 } else { 2 };
        let raw = if mode == "ATTRIBUTES" {
            triangle().1
        } else {
            vec![0, 0, 1, 0, 2, 0]
        };
        let bytes = encode(&raw, 3, stride, mode);
        let source = Compression {
            buffer: 0,
            byte_offset: 0,
            byte_length: bytes.len(),
            count: 3,
            byte_stride: stride,
            mode: mode.into(),
            filter: None,
        };
        for length in 0..bytes.len() {
            assert!(
                decode_view(&source, &bytes[..length], &|| Ok(())).is_err(),
                "{mode} accepted prefix {length}"
            );
        }
        let mut wrong = bytes.clone();
        wrong[0] += 1;
        assert!(decode_view(&source, &wrong, &|| Ok(())).is_err());
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(decode_view(&source, &extra, &|| Ok(())).is_err());
    }
    for mode in ["TRIANGLES", "INDICES"] {
        let bytes = encode_indices(&[0, 1, 65536], mode);
        let source = Compression {
            buffer: 0,
            byte_offset: 0,
            byte_length: bytes.len(),
            count: 3,
            byte_stride: 2,
            mode: mode.into(),
            filter: None,
        };
        assert!(
            decode_view(&source, &bytes, &|| Ok(()))
                .err()
                .unwrap()
                .to_string()
                .contains("unsigned short")
        );
    }
}

#[test]
fn meshopt_rejects_invalid_filtered_components_and_nonfinite_positions() {
    for (stride, filter, raw) in [
        (4, "OCTAHEDRAL", vec![0, 0, 0, 0]),
        (4, "OCTAHEDRAL", vec![9, 0, 7, 0]),
        (8, "QUATERNION", vec![0; 8]),
    ] {
        let bytes = encode(&raw, 1, stride, "ATTRIBUTES");
        let source = Compression {
            buffer: 0,
            byte_offset: 0,
            byte_length: bytes.len(),
            count: 1,
            byte_stride: stride,
            mode: "ATTRIBUTES".into(),
            filter: Some(filter.into()),
        };
        assert!(decode_view(&source, &bytes, &|| Ok(())).is_err());
    }
    let (mut document, mut bin) = triangle();
    bin[..4].copy_from_slice(&f32::NAN.to_le_bytes());
    compress_view(&mut document, &mut bin, 0, 3, 12, "ATTRIBUTES", "NONE");
    expect_error(&document, &bin, "non-finite");
}

#[test]
fn meshopt_cancellation_surrounds_native_decode_and_filter_blocks() {
    let count = FILTER_BLOCK_ELEMENTS * 3;
    let raw = [1u32, 2, 3]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>()
        .repeat(count);
    let bytes = encode(&raw, count, 12, "ATTRIBUTES");
    let source = Compression {
        buffer: 0,
        byte_offset: 0,
        byte_length: bytes.len(),
        count,
        byte_stride: 12,
        mode: "ATTRIBUTES".into(),
        filter: Some("EXPONENTIAL".into()),
    };
    for stop in 1..=5 {
        let calls = Cell::new(0);
        let result = decode_view(&source, &bytes, &|| {
            calls.set(calls.get() + 1);
            ensure!(calls.get() != stop, "cancelled");
            Ok(())
        });
        assert!(result.err().unwrap().to_string().contains("cancelled"));
        assert_eq!(calls.get(), stop);
    }
}

#[test]
fn meshopt_selected_output_and_geometry_have_independent_bounds() {
    let (mut document, bin) = compressed_triangle();
    let count = 300_001;
    document["bufferViews"][0]["extensions"][EXTENSION]["count"] = count.into();
    document["bufferViews"][0]["byteLength"] = (count * 12).into();
    document["buffers"][1]["byteLength"] = (count * 12).into();
    expect_error(&document, &bin, "selected view exceeds the 300,000");
    let (mut document, bin) = compressed_triangle();
    document["bufferViews"][0]["extensions"][EXTENSION]["count"] = 300_000.into();
    document["bufferViews"][0]["extensions"][EXTENSION]["byteStride"] = 64.into();
    document["bufferViews"][0]["byteLength"] = 19_200_000.into();
    document["buffers"][1]["byteLength"] = 19_200_000.into();
    expect_error(&document, &bin, "16 MiB compressed or decompressed");
    // An unrelated valid descriptor can exceed selected-work limits; it is never
    // allocated or decoded when the selected geometry does not reference it.
    document["bufferViews"][0]["buffer"] = 0.into();
    let compressed = document["bufferViews"][0].clone();
    document["bufferViews"][0] = json!({"buffer":0,"byteLength":36});
    document["bufferViews"]
        .as_array_mut()
        .unwrap()
        .push(compressed);
    document["bufferViews"][1]["buffer"] = 1.into();
    assert_eq!(decoded(&document, &bin).scene.triangle_count(), 1);
}

#[test]
fn meshopt_public_reference_arch_preserves_changed_bounds_and_union_camera() {
    let before = include_bytes!("../../../../tests/fixtures/models/glb/meshopt-arch-before.glb");
    let after = include_bytes!("../../../../tests/fixtures/models/glb/meshopt-arch-after.glb");
    let before = decode_geometry(before, "arch.glb", || Ok(())).unwrap();
    let after = decode_geometry(after, "arch.glb", || Ok(())).unwrap();
    assert_eq!(before.scene.triangle_count(), 36);
    assert_eq!(after.scene.triangle_count(), 36);
    assert_ne!(before.scene.bounds, after.scene.bounds);
    let union = before.scene.bounds.union(after.scene.bounds);
    let camera = ModelCamera::fit(union);
    assert_eq!(camera.target, union.center());
    // The adapter releases decompressed views; retained accounting depends only
    // on the static scene, not the compressed input's byte length.
    assert!(before.scene.retained_bytes() >= 36 * 3 * 3 * 8);
}

#[test]
fn meshopt_shared_view_decodes_once_and_distinct_views_hit_cumulative_budget() {
    let (mut document, mut bin) = triangle();
    let count = 49_152;
    let stride = 256;
    bin.resize(count * stride, 0);
    document["buffers"][0]["byteLength"] = bin.len().into();
    document["bufferViews"][0]["byteLength"] = bin.len().into();
    let encoded = encode(&bin, count, stride, "ATTRIBUTES");
    attach(
        &mut document,
        &mut bin,
        0,
        count,
        stride,
        "ATTRIBUTES",
        "NONE",
        &encoded,
    );
    document["meshes"][0]["primitives"] = json!([
        {"attributes":{"POSITION":0}}, {"attributes":{"POSITION":0}}, {"attributes":{"POSITION":0}}
    ]);
    assert_eq!(decoded(&document, &bin).scene.triangle_count(), 3);
    let source = document["bufferViews"][0].clone();
    let accessor = document["accessors"][0].clone();
    for index in 1..3 {
        document["bufferViews"]
            .as_array_mut()
            .unwrap()
            .push(source.clone());
        document["accessors"]
            .as_array_mut()
            .unwrap()
            .push(accessor.clone());
        document["accessors"][index]["bufferView"] = index.into();
        document["meshes"][0]["primitives"][index]["attributes"]["POSITION"] = index.into();
    }
    expect_error(
        &document,
        &bin,
        "32 MiB cumulative compressed or decompressed",
    );
}

#[test]
fn meshopt_compressed_input_and_expanded_instances_are_independently_bounded() {
    let (mut document, mut bin) = compressed_triangle();
    let offset = document["bufferViews"][0]["extensions"][EXTENSION]["byteOffset"]
        .as_u64()
        .unwrap() as usize;
    bin.resize(offset + MAX_VIEW_BYTES + 1, 0);
    document["buffers"][0]["byteLength"] = bin.len().into();
    document["bufferViews"][0]["extensions"][EXTENSION]["byteLength"] = (MAX_VIEW_BYTES + 1).into();
    expect_error(&document, &bin, "16 MiB compressed or decompressed");
    let (mut document, bin) = compressed_triangle();
    document["meshes"][0]["primitives"] =
        Value::Array(vec![json!({"attributes":{"POSITION":0}}); 100]);
    document["nodes"] = Value::Array(vec![json!({"mesh":0}); 1001]);
    document["scenes"][0]["nodes"] = json!((0..1001).collect::<Vec<_>>());
    expect_error(&document, &bin, "100,000 expanded triangle");
}
