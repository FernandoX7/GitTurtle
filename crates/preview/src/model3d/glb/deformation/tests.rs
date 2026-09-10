use super::*;
use crate::model3d::glb::tests::{decoded, expect_error, pack, triangle};
use serde_json::{Value, json};
use std::cell::Cell;

fn accessor(
    document: &mut Value,
    bin: &mut Vec<u8>,
    kind: &str,
    components: usize,
    values: &[f32],
) -> usize {
    let start = bin.len().next_multiple_of(4);
    bin.resize(start, 0);
    for value in values {
        bin.extend(value.to_le_bytes());
    }
    document["buffers"][0]["byteLength"] = bin.len().into();
    let views = document["bufferViews"].as_array_mut().unwrap();
    let index = views.len();
    views.push(json!({"buffer":0,"byteOffset":start,"byteLength":values.len()*4}));
    let accessors = document["accessors"].as_array_mut().unwrap();
    let output = accessors.len();
    let mut value = json!({"bufferView":index,"componentType":5126,"count":values.len()/components,"type":kind});
    if kind == "SCALAR" {
        value["min"] = json!([values.iter().copied().fold(f32::INFINITY, f32::min)]);
        value["max"] = json!([values.iter().copied().fold(f32::NEG_INFINITY, f32::max)]);
    }
    accessors.push(value);
    output
}
fn translation_animation(interpolation: &str, times: &[f32], values: &[f32]) -> (Value, Vec<u8>) {
    let (mut document, mut bin) = triangle();
    let input = accessor(&mut document, &mut bin, "SCALAR", 1, times);
    let output = accessor(&mut document, &mut bin, "VEC3", 3, values);
    document["animations"] = json!([{"name":"Move","channels":[{"sampler":0,"target":{"node":0,"path":"translation"}}],"samplers":[{"input":input,"output":output,"interpolation":interpolation}]}]);
    (document, bin)
}

#[test]
fn static_morph_node_weights_override_mesh_and_apply_before_node_transform() {
    let (mut document, mut bin) = triangle();
    let delta = accessor(
        &mut document,
        &mut bin,
        "VEC3",
        3,
        &[1., 2., 3., 1., 2., 3., 1., 2., 3.],
    );
    document["meshes"][0]["primitives"][0]["targets"] = json!([{"POSITION":delta}]);
    document["meshes"][0]["weights"] = json!([0.25]);
    document["nodes"] = json!([{"mesh":0,"translation":[4,0,0],"weights":[0.5]},{"mesh":0}]);
    document["scenes"][0]["nodes"] = json!([0, 1]);
    let scene = decoded(&document, &bin).scene;
    assert_eq!(scene.triangles()[0][0], [4500., -1500., 1000.]);
    assert_eq!(scene.triangles()[1][0], [250., -750., 500.]);
    assert!(scene.animation_clips().is_empty());
}

#[test]
fn static_public_skin_preserves_joint_hierarchy_inverse_bind_and_ignores_mesh_transform() {
    let bytes = include_bytes!("../../../../tests/fixtures/models/glb/workflow/skin-animated.glb");
    let decoded = crate::model3d::decode_geometry(bytes, "skin.glb", || Ok(())).unwrap();
    let (json, _) = container(bytes, &|| Ok(())).unwrap();
    let document: Value = serde_json::from_slice(json).unwrap();
    assert_ne!(
        document["nodes"][0].get("translation"),
        Some(&json!([0, 0, 0]))
    );
    assert!(!decoded.scene.triangles().is_empty());
    // Exact independently derived expectations are checked in the reference
    // workflow; this local regression asserts pose is independent of mesh TRS.
    let (json, bin) = container(bytes, &|| Ok(())).unwrap();
    let mut document: Value = serde_json::from_slice(json).unwrap();
    for node in document["nodes"].as_array_mut().unwrap() {
        if node.get("skin").is_some() {
            node["translation"] = json!([999, 888, 777]);
            node["scale"] = json!([2, 3, 4]);
        }
    }
    let changed =
        crate::model3d::decode_geometry(&pack(&document, bin.unwrap()), "skin.glb", || Ok(()))
            .unwrap();
    assert_eq!(decoded.scene.triangles(), changed.scene.triangles());
}

#[test]
fn step_linear_and_cubic_clamp_and_preserve_default_pose() {
    for (interpolation, values, expected) in [
        ("LINEAR", vec![0., 0., 0., 4., 0., 0.], 1000.),
        ("STEP", vec![0., 0., 0., 4., 0., 0.], 0.),
        // Keys at 0 and 2. Outgoing tangent 2 m/s; endpoint values zero.
        (
            "CUBICSPLINE",
            vec![
                0., 0., 0., 0., 0., 0., 2., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.,
            ],
            562.5,
        ),
    ] {
        let (document, bin) = translation_animation(interpolation, &[0., 2.], &values);
        let scene = decoded(&document, &bin).scene;
        assert_eq!(scene.animation_clips()[0].duration_seconds, 2.);
        assert_eq!(scene.triangles()[0][0], [0.; 3]);
        let frame = scene.evaluate_animation(0, 0.5, &|| Ok(())).unwrap();
        assert!(
            (frame.triangles()[0][0][0] - expected).abs() < 1e-8,
            "{interpolation}: {:?}",
            frame.triangles()
        );
        assert_eq!(
            scene
                .evaluate_animation(0, -5., &|| Ok(()))
                .unwrap()
                .triangles()[0][0],
            [0.; 3]
        );
        assert_eq!(
            scene
                .evaluate_animation(0, 99., &|| Ok(()))
                .unwrap()
                .triangles(),
            scene
                .evaluate_animation(0, 2., &|| Ok(()))
                .unwrap()
                .triangles()
        );
    }
}

#[test]
fn shortest_path_quaternion_linear_avoids_long_rotation() {
    let (mut document, mut bin) = triangle();
    let input = accessor(&mut document, &mut bin, "SCALAR", 1, &[0., 1.]);
    let angle = 170f32.to_radians() / 2.;
    let output = accessor(
        &mut document,
        &mut bin,
        "VEC4",
        4,
        &[
            0.,
            0.,
            angle.sin(),
            angle.cos(),
            0.,
            0.,
            -angle.sin(),
            angle.cos(),
        ],
    );
    document["animations"] = json!([{"channels":[{"sampler":0,"target":{"node":0,"path":"rotation"}}],"samplers":[{"input":input,"output":output}]}]);
    let scene = decoded(&document, &bin).scene;
    let frame = scene.evaluate_animation(0, 0.5, &|| Ok(())).unwrap();
    assert!((frame.triangles()[0][1][0] + 2000.).abs() < 1e-5);
    assert!(frame.triangles()[0][1][2].abs() < 1e-5);
}

#[test]
fn malformed_animation_preserves_default_and_valid_other_clip() {
    let (mut document, bin) = translation_animation("LINEAR", &[0., 1.], &[0., 0., 0., 1., 0., 0.]);
    let valid = document["animations"][0].clone();
    document["animations"][0]["channels"][0]["target"]["path"] = json!("unsupported");
    document["animations"].as_array_mut().unwrap().push(valid);
    let geometry = decoded(&document, &bin);
    assert_eq!(geometry.scene.triangle_count(), 1);
    assert_eq!(geometry.scene.animation_clips().len(), 1);
    assert!(
        geometry
            .details
            .iter()
            .any(|v| v.contains("clip 1 unavailable"))
    );
    assert_eq!(
        geometry
            .scene
            .evaluate_animation(0, 1., &|| Ok(()))
            .unwrap()
            .triangles()[0][0][0],
        1000.
    );
}

#[test]
fn animation_limits_values_counts_and_cancellation_are_explicit() {
    for times in [[0., 0.], [1., 0.], [-1., 1.], [0., 90_000.]] {
        let (document, bin) = translation_animation("LINEAR", &times, &[0., 0., 0., 1., 0., 0.]);
        let geometry = decoded(&document, &bin);
        assert!(geometry.scene.animation_clips().is_empty());
        assert!(geometry.details.iter().any(|v| v.contains("timestamps")));
    }
    let (mut document, bin) = translation_animation("LINEAR", &[0., 1.], &[0., 0., 0., 1., 0., 0.]);
    document["animations"][0]["name"] = json!("é".repeat(1000));
    let scene = decoded(&document, &bin).scene;
    assert_eq!(scene.animation_clips()[0].name.len(), 256);
    assert!(scene.evaluate_animation(0, f64::NAN, &|| Ok(())).is_err());
    assert!(scene.evaluate_animation(5, 0., &|| Ok(())).is_err());
    let calls = Cell::new(0);
    assert!(
        scene
            .evaluate_animation(0, 0.5, &|| {
                calls.set(calls.get() + 1);
                ensure!(calls.get() < 4, "cancel frame");
                Ok(())
            })
            .unwrap_err()
            .to_string()
            .contains("cancel frame")
    );
    let clip = document["animations"][0].clone();
    document["animations"] = json!(vec![clip; 65]);
    let geometry = decoded(&document, &bin);
    assert!(geometry.scene.animation_clips().is_empty());
    assert!(geometry.details.iter().any(|v| v.contains("64-clip")));
}

#[test]
fn malformed_morph_and_joint_limits_are_explicit() {
    let (mut document, bin) = triangle();
    document["meshes"][0]["primitives"][0]["targets"] = json!(vec![json!({"POSITION":0}); 9]);
    expect_error(&document, &bin, "8 morph-target");
    let bytes = include_bytes!("../../../../tests/fixtures/models/glb/workflow/skin-animated.glb");
    let (json, bin) = container(bytes, &|| Ok(())).unwrap();
    let mut document: Value = serde_json::from_slice(json).unwrap();
    document["skins"][0]["joints"] = json!(vec![0; 257]);
    expect_error(&document, bin.unwrap(), "256 joints");
}

#[test]
fn malformed_animation_accessor_layout_preserves_static_geometry() {
    for (field, value) in [
        ("count", json!(99)),
        ("byteOffset", json!(usize::MAX)),
        ("bufferView", json!(99)),
        ("componentType", json!(123)),
    ] {
        let (mut document, bin) =
            translation_animation("LINEAR", &[0., 1.], &[0., 0., 0., 1., 0., 0.]);
        document["accessors"][2][field] = value;
        let geometry = decoded(&document, &bin);
        assert_eq!(geometry.scene.triangle_count(), 1);
        assert!(geometry.scene.animation_clips().is_empty());
        assert!(
            geometry
                .details
                .iter()
                .any(|v| v.contains("clip 1 unavailable"))
        );
    }
}

#[test]
fn animation_zero_scale_can_collapse_pose_without_invalidating_scene() {
    let (mut document, bin) = translation_animation("LINEAR", &[0., 1.], &[1., 1., 1., 0., 0., 0.]);
    document["animations"][0]["channels"][0]["target"]["path"] = json!("scale");
    let scene = decoded(&document, &bin).scene;
    let camera = crate::model3d::ModelCamera::fit(scene.bounds);
    let collapsed = scene.evaluate_animation(0, 1., &|| Ok(())).unwrap();
    assert_eq!(collapsed.triangles(), &[[[0.; 3]; 3]]);
    assert_eq!(collapsed.bounds.minimum, collapsed.bounds.maximum);
    assert!(crate::model3d::render_model(&collapsed, &camera, 64, false, || Ok(())).is_ok());
}

#[test]
fn checked_in_rigged_and_morph_fixtures_are_supported() {
    for bytes in [
        include_bytes!("../../../../tests/fixtures/models/glb/RiggedSimple.glb").as_slice(),
        include_bytes!("../../../../tests/fixtures/models/glb/workflow/morph-animation.glb")
            .as_slice(),
    ] {
        let scene = crate::model3d::decode_geometry(bytes, "fixture.glb", || Ok(()))
            .unwrap()
            .scene;
        assert!(scene.triangle_count() > 0);
        for index in 0..scene.animation_clips().len() {
            assert!(scene.evaluate_animation(index, 0.5, &|| Ok(())).is_ok());
        }
    }
}

#[test]
fn eight_joint_influences_and_morphs_follow_transform_order() {
    let (mut document, mut bin) = triangle();
    let delta = accessor(
        &mut document,
        &mut bin,
        "VEC3",
        3,
        &[1., 0., 0., 1., 0., 0., 1., 0., 0.],
    );
    document["meshes"][0]["weights"] = json!([2.]);
    document["meshes"][0]["primitives"][0]["targets"] = json!([{"POSITION":delta}]);
    for set in 0..2 {
        let start = bin.len().next_multiple_of(4);
        bin.resize(start, 0);
        for _ in 0..3 {
            for joint in set * 4..set * 4 + 4 {
                bin.extend((joint as u16).to_le_bytes());
            }
        }
        let views = document["bufferViews"].as_array_mut().unwrap();
        let view = views.len();
        views.push(json!({"buffer":0,"byteOffset":start,"byteLength":24}));
        let accessors = document["accessors"].as_array_mut().unwrap();
        let joints = accessors.len();
        accessors.push(json!({"bufferView":view,"componentType":5123,"type":"VEC4","count":3}));
        let weights = accessor(&mut document, &mut bin, "VEC4", 4, &[0.125; 12]);
        document["meshes"][0]["primitives"][0]["attributes"][format!("JOINTS_{set}")] =
            json!(joints);
        document["meshes"][0]["primitives"][0]["attributes"][format!("WEIGHTS_{set}")] =
            json!(weights);
    }
    let mut nodes = vec![json!({"translation":[1,0,0],"children":[1,2,3,4,5,6,7,8]})];
    let q = 0.5f64.sqrt();
    for index in 0..8 {
        nodes.push(json!({"translation":[index,0,0],"rotation":[0,0,q,q]}));
    }
    nodes.push(json!({"mesh":0,"skin":0,"translation":[100,0,0]}));
    document["nodes"] = json!(nodes);
    document["skins"] = json!([{"joints":[1,2,3,4,5,6,7,8],"skeleton":0}]);
    document["scenes"][0]["nodes"] = json!([0, 9]);
    let scene = decoded(&document, &bin).scene;
    for (actual, expected) in scene.triangles()[0][0].iter().zip([4500., 0., 2000.]) {
        assert!((actual - expected).abs() < 1e-8);
    }
    for (actual, expected) in scene.triangles()[0][1].iter().zip([4500., 0., 4000.]) {
        assert!((actual - expected).abs() < 1e-8);
    }
    document["meshes"][0]["primitives"][0]["attributes"]["JOINTS_2"] = json!(1);
    expect_error(&document, &bin, "eight influences");
}

#[test]
fn deformation_decode_and_frame_work_limits_are_independent_of_triangle_count() {
    for (vertices, targets, instances, message) in [
        (200_000, 8, 1, "4,000,000 deformation"),
        (150_000, 4, 11, "8,000,000 per-frame"),
    ] {
        let (mut document, mut bin) = triangle();
        document["accessors"][0]
            .as_object_mut()
            .unwrap()
            .remove("bufferView");
        document["accessors"][0]["count"] = json!(vertices);
        crate::model3d::glb::tests::add_indices(&mut document, &mut bin, 5121, &[0, 1, 2]);
        document["meshes"][0]["primitives"][0]["targets"] =
            json!(vec![json!({"POSITION":0}); targets]);
        document["nodes"] = json!(vec![json!({"mesh":0}); instances]);
        document["scenes"][0]["nodes"] = json!((0..instances).collect::<Vec<_>>());
        expect_error(&document, &bin, message);
    }
}
