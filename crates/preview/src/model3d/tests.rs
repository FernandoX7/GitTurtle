use super::*;
use std::io::Write;

const OBJ: &[u8] = include_bytes!("../../tests/fixtures/models/tetra.obj");
const FBX: &[u8] = include_bytes!("../../tests/fixtures/models/tetra.fbx");

fn three_mf_bytes(model: &str, method: zip::CompressionMethod) -> Vec<u8> {
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(method);
    output.start_file("_rels/.rels", options).unwrap();
    output.write_all(b"<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"start\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\" Target=\"/3D/model.model\"/></Relationships>").unwrap();
    output.start_file("3D/model.model", options).unwrap();
    output.write_all(model.as_bytes()).unwrap();
    output.finish().unwrap().into_inner()
}

fn core_model(objects: &str, build: &str) -> String {
    format!(
        "<model xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\" unit=\"millimeter\"><resources>{objects}</resources><build>{build}</build></model>"
    )
}

const THREE_MF_OBJECT: &str = "<object id=\"1\"><mesh><vertices><vertex x=\"0\" y=\"0\" z=\"0\"/><vertex x=\"2\" y=\"0\" z=\"0\"/><vertex x=\"0\" y=\"3\" z=\"4\"/></vertices><triangles><triangle v1=\"0\" v2=\"1\" v3=\"2\"/></triangles></mesh></object>";

const STEP: &[u8] = include_bytes!("../../tests/fixtures/models/tetra.step");

fn assert_views(preview: &ModelPreview) {
    assert_eq!(preview.views.len(), 4);
    assert_eq!(preview.views[0].caption, "Isometric");
    for view in &preview.views {
        assert_eq!((view.image.width, view.image.height), (720, 720));
        assert_eq!(view.image.rgba.len(), 720 * 720 * 4);
        assert!(
            view.image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[3] == 255)
        );
    }
    assert!(
        preview.views[0]
            .image
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| **p != [28, 34, 42, 255])
            .count()
            > 10_000
    );
    assert_ne!(preview.views[0].image.rgba, preview.views[1].image.rgba);
}

#[test]
fn supplied_obj_and_fbx_render_geometry_without_external_materials() {
    for (name, bytes) in [("model.obj", OBJ), ("model.fbx", FBX)] {
        let preview = decode_model(bytes, name, || Ok(())).unwrap();
        assert_views(&preview);
        assert!(preview.details.iter().any(|v| v.contains("4 triangles")));
    }
}

#[test]
fn fbx_node_placement_is_applied_and_invalid_obj_indices_are_refused() {
    let placed = std::str::from_utf8(FBX).unwrap().replace("Version: 232", "Version: 232\nProperties70: {\nP: \"Lcl Translation\", \"Lcl Translation\", \"\", \"A\", 5,7,9\n}");
    let base = fbx_or_obj(FBX, false, &|| Ok(())).unwrap();
    let moved = fbx_or_obj(placed.as_bytes(), false, &|| Ok(())).unwrap();
    assert_eq!(base.len(), moved.len());
    let delta = subtract(moved[0][0], base[0][0]);
    assert!((dot(delta, delta) - 155.).abs() < 1e-6);
    for (before, after) in base.iter().flatten().zip(moved.iter().flatten()) {
        let actual = subtract(*after, *before);
        assert!((0..3).all(|axis| (actual[axis] - delta[axis]).abs() < 1e-6));
    }
    assert!(
        decode_model(
            b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 999\n",
            "invalid.obj",
            || Ok(())
        )
        .is_err()
    );
}

#[test]
fn ascii_and_binary_stl_produce_same_geometry() {
    let ascii = b"solid triangle\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 2 0 0\nvertex 0 3 4\nendloop\nendfacet\nendsolid triangle\n";
    let mut binary = vec![0; 80];
    binary.extend_from_slice(&1u32.to_le_bytes());
    for value in [0f32, 0., 1., 0., 0., 0., 2., 0., 0., 0., 3., 4.] {
        binary.extend_from_slice(&value.to_le_bytes());
    }
    binary.extend_from_slice(&[0, 0]);
    let a = decode_model(ascii, "triangle.stl", || Ok(())).unwrap();
    let b = decode_model(&binary, "triangle.stl", || Ok(())).unwrap();
    assert_views(&a);
    assert_eq!(a.views[0].image.rgba, b.views[0].image.rgba);
}

#[test]
fn three_mf_build_instances_compose_transforms_and_ignore_unused_objects() {
    let source = core_model(
        &format!(
            "{THREE_MF_OBJECT}<object id=\"2\"><components><component objectid=\"1\" transform=\"2 0 0 0 1 0 0 0 1 5 0 0\"/></components></object><object id=\"3\"><components><component objectid=\"3\"/></components></object>"
        ),
        "<item objectid=\"2\" transform=\"1 0 0 0 1 0 0 0 1 0 7 0\"/>",
    );
    for method in [
        zip::CompressionMethod::Stored,
        zip::CompressionMethod::Deflated,
    ] {
        let bytes = three_mf_bytes(&source, method);
        let (mesh, units) = three_mf(&bytes, &|| Ok(())).unwrap();
        assert_eq!(units, "millimeter");
        assert_eq!(mesh, vec![[[5., 7., 0.], [9., 7., 0.], [5., 10., 4.]]]);
        assert_views(&decode_model(&bytes, "build.3mf", || Ok(())).unwrap());
    }
}

#[test]
fn model_refuses_cycles_bad_indices_nonfinite_and_geometry_amplification() {
    let cycle = core_model(
        "<object id=\"1\"><components><component objectid=\"1\"/></components></object>",
        "<item objectid=\"1\"/>",
    );
    assert!(
        decode_model(
            &three_mf_bytes(&cycle, zip::CompressionMethod::Stored),
            "cycle.3mf",
            || Ok(())
        )
        .unwrap_err()
        .to_string()
        .contains("cycle")
    );
    let invalid = core_model(
        &THREE_MF_OBJECT.replace("v3=\"2\"", "v3=\"999\""),
        "<item objectid=\"1\"/>",
    );
    assert!(
        decode_model(
            &three_mf_bytes(&invalid, zip::CompressionMethod::Stored),
            "invalid.3mf",
            || Ok(())
        )
        .is_err()
    );
    assert!(
        decode_model(
            b"v NaN 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n",
            "invalid.obj",
            || Ok(())
        )
        .is_err()
    );
    let mesh = vec![[[0., 0., 0.], [1., 0., 0.], [0., 1., 1.]]];
    let mut work = MAX_RASTER_SAMPLES;
    assert!(
        rasterize(
            &mesh,
            [1., -1., 1.],
            [0., 0., 1.],
            "test",
            &mut work,
            &|| Ok(())
        )
        .unwrap_err()
        .to_string()
        .contains("raster")
    );
    let mut too_many = vec![0; 84 + (MAX_MODEL_TRIANGLES + 1) * 50];
    too_many[80..84].copy_from_slice(&((MAX_MODEL_TRIANGLES + 1) as u32).to_le_bytes());
    assert!(
        decode_model(&too_many, "large.stl", || Ok(()))
            .unwrap_err()
            .to_string()
            .contains("triangle preview limit")
    );
}

#[test]
fn model_cancellation_and_xml_external_references_are_refused() {
    let checks = std::cell::Cell::new(0);
    let error = decode_model(OBJ, "mesh.obj", || {
        checks.set(checks.get() + 1);
        if checks.get() > 2 {
            bail!("cancelled preview")
        }
        Ok(())
    })
    .unwrap_err();
    assert!(error.to_string().contains("cancelled"));
    let source = core_model(THREE_MF_OBJECT, "<item objectid=\"1\"/>");
    let external = source.replace(
        "<model ",
        "<!DOCTYPE model [<!ENTITY external SYSTEM \"file:///not/read\">]><model ",
    );
    assert!(
        decode_model(
            &three_mf_bytes(&external, zip::CompressionMethod::Deflated),
            "external.3mf",
            || Ok(())
        )
        .is_err()
    );
    let required = source.replace("<model ", "<model requiredextensions=\"production\" ");
    assert!(
        decode_model(
            &three_mf_bytes(&required, zip::CompressionMethod::Stored),
            "extension.3mf",
            || Ok(())
        )
        .unwrap_err()
        .to_string()
        .contains("extensions")
    );
}

#[test]
fn faceted_step_renders_the_same_solid_as_obj_and_refuses_curved_or_placed_solids() {
    let step = decode_model(STEP, "tetra.step", || Ok(())).unwrap();
    let obj = decode_model(OBJ, "tetra.obj", || Ok(())).unwrap();
    assert_views(&step);
    assert_eq!(step.views[0].image.rgba, obj.views[0].image.rgba);
    let source = std::str::from_utf8(STEP).unwrap();
    for bad in [
        source.replace(
            "FACETED_BREP('Tetrahedron',#40)",
            "MANIFOLD_SOLID_BREP('Tetrahedron',#40)",
        ),
        source.replace("#41=", "#99=MAPPED_ITEM('instance',#1,#2);\n#41="),
        source.replace("FACE('',(#20))", "FACE('',(#20,#21))"),
        source.replace("(#1,#3,#2)", "(#1,#3,#999)"),
    ] {
        assert!(decode_model(bad.as_bytes(), "unsupported.stp", || Ok(())).is_err());
    }
}

#[test]
fn step_concave_face_triangulates_without_filling_the_notch() {
    let points = [
        [0., 0., 0.],
        [4., 0., 0.],
        [4., 4., 0.],
        [2., 2., 0.],
        [0., 4., 0.],
    ];
    let mut output = Vec::new();
    step::triangulate(&points, &mut output, &mut 0, &|| Ok(())).unwrap();
    assert_eq!(output.len(), 3);
    let area: f64 = output
        .iter()
        .map(|triangle| {
            dot(
                cross(
                    subtract(triangle[1], triangle[0]),
                    subtract(triangle[2], triangle[0]),
                ),
                [0., 0., 1.],
            )
            .abs()
                / 2.
        })
        .sum();
    assert_eq!(area, 12.);
    assert!(
        step::triangulate(
            &[[0., 0., 0.], [1., 0., 0.], [1., 1., 1.], [0., 1., 0.]],
            &mut Vec::new(),
            &mut 0,
            &|| Ok(())
        )
        .is_err()
    );
}
