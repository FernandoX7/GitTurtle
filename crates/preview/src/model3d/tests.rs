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
    // Missing FBX GlobalSettings use its centimeter default; output is mm.
    assert!((dot(delta, delta) - 15_500.).abs() < 1e-6);
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

#[test]
fn interactive_camera_preserves_revision_scale_and_translation() {
    let before = decode_geometry(OBJ, "before.obj", || Ok(())).unwrap();
    let after_source = std::str::from_utf8(OBJ)
        .unwrap()
        .replace("v 2 0 0", "v 4 0 0");
    let after = decode_geometry(after_source.as_bytes(), "after.obj", || Ok(())).unwrap();
    let bounds = before.scene.bounds.union(after.scene.bounds);
    let mut camera = ModelCamera::fit(bounds);
    camera.set_view(ModelStandardView::Front);
    let origin = camera.project([0., 0., 0.]);
    let old_tip = camera.project([2., 0., 0.]);
    let new_tip = camera.project([4., 0., 0.]);
    assert!(((new_tip[0] - origin[0]) / (old_tip[0] - origin[0]) - 2.).abs() < 1e-12);
    let translated = camera.project([12., 0., 0.]);
    assert!(translated[0] > new_tip[0]);
    let a = render_model(&before.scene, &camera, 256, false, || Ok(())).unwrap();
    let b = render_model(&after.scene, &camera, 256, false, || Ok(())).unwrap();
    assert_ne!(a.rgba, b.rgba);
    assert!(before.scene.retained_bytes() >= 4 * std::mem::size_of::<Triangle>());
}

#[test]
fn orbit_pan_zoom_fit_and_wireframe_change_requested_frame_only() {
    let geometry = decode_geometry(OBJ, "tetra.obj", || Ok(())).unwrap();
    let mut camera = ModelCamera::fit(geometry.scene.bounds);
    let initial = camera;
    let render = |camera: &ModelCamera, wire| {
        render_model(&geometry.scene, camera, 256, wire, || Ok(()))
            .unwrap()
            .rgba
    };
    let frame = render(&camera, false);
    camera.orbit(0.4, 0.2);
    assert_ne!(render(&camera, false), frame);
    camera = initial;
    let point = camera.project([0., 0., 0.]);
    camera.pan(0.1, -0.2);
    let panned = camera.project([0., 0., 0.]);
    assert!((panned[0] - point[0] - 0.1).abs() < 1e-12);
    assert!((panned[1] - point[1] + 0.2).abs() < 1e-12);
    camera.zoom(2.);
    assert_eq!(camera.span, initial.span / 2.);
    camera.fit_bounds(geometry.scene.bounds);
    assert_eq!(camera, initial);
    assert_ne!(render(&camera, true), frame);
    let copy = camera;
    camera.zoom(f64::NAN);
    camera.pan(f64::INFINITY, 0.);
    camera.orbit(f64::NAN, 0.);
    assert_eq!(camera, copy);
    for view in ModelStandardView::ALL {
        camera.set_view(view);
        let axes = camera.orientation_axes();
        for axis in axes {
            assert!((dot(axis.direction, axis.direction) - 1.).abs() < 1e-12);
        }
        assert!(
            render(&camera, false)
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| *p != [28, 34, 42, 255])
        );
    }
}

#[test]
fn known_model_units_normalize_and_frames_are_cancellable_and_bounded() {
    let source = core_model(THREE_MF_OBJECT, "<item objectid=\"1\"/>");
    let mm = decode_geometry(
        &three_mf_bytes(&source, zip::CompressionMethod::Stored),
        "mm.3mf",
        || Ok(()),
    )
    .unwrap();
    let inch_source = source.replace("unit=\"millimeter\"", "unit=\"inch\"");
    let inch = decode_geometry(
        &three_mf_bytes(&inch_source, zip::CompressionMethod::Stored),
        "inch.3mf",
        || Ok(()),
    )
    .unwrap();
    assert_eq!(mm.scene.units, ModelUnits::Millimeters);
    for (a, b) in mm
        .scene
        .triangles()
        .iter()
        .flatten()
        .zip(inch.scene.triangles().iter().flatten())
    {
        assert!((0..3).all(|i| (b[i] - a[i] * 25.4).abs() < 1e-12));
    }
    let camera = ModelCamera::fit(mm.scene.bounds);
    assert!(render_model(&mm.scene, &camera, 721, false, || Ok(())).is_err());
    assert!(
        render_model(&mm.scene, &camera, 256, false, || bail!("cancelled camera"))
            .unwrap_err()
            .to_string()
            .contains("cancelled camera")
    );
    let invalid = ModelCamera {
        span: f64::NAN,
        ..camera
    };
    assert!(render_model(&mm.scene, &invalid, 256, true, || Ok(())).is_err());
}

fn step_document(entities: &str) -> String {
    format!(
        "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));\nENDSEC;\nDATA;\n{entities}\nENDSEC;\nEND-ISO-10303-21;"
    )
}

const STEP_MM: &str = "#100=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n#101=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#100)) REPRESENTATION_CONTEXT('',''));";

#[test]
fn step_curved_csg_sphere_has_curvature_and_measured_volume() {
    let source = step_document(&format!(
        "#1=CARTESIAN_POINT('',(10.,20.,30.));\n#2=SPHERE('',2.,#1);\n#3=CSG_SOLID('',#2);\n#4=CSG_SHAPE_REPRESENTATION('',(#3),#101);\n{STEP_MM}"
    ));
    let model = decode_geometry(source.as_bytes(), "sphere.step", || Ok(())).unwrap();
    assert_eq!(model.scene.units, ModelUnits::Millimeters);
    assert_eq!(model.scene.bounds.minimum, [8., 18., 28.]);
    assert_eq!(model.scene.bounds.maximum, [12., 22., 32.]);
    assert_eq!(model.scene.triangle_count(), 3968);
    let center = [10., 20., 30.];
    for p in model.scene.triangles().iter().flatten() {
        let relative = subtract(*p, center);
        assert!((dot(relative, relative) - 4.).abs() < 1e-12);
    }
    let volume: f64 = model
        .scene
        .triangles()
        .iter()
        .map(|t| {
            dot(
                subtract(t[0], center),
                cross(subtract(t[1], center), subtract(t[2], center)),
            ) / 6.
        })
        .sum();
    let analytic = 4. / 3. * std::f64::consts::PI * 8.;
    assert!((volume - analytic).abs() / analytic < 0.006);
    assert!(model.details.iter().any(|v| v.contains("64 segments")));
}

#[test]
fn step_cylinder_torus_and_block_use_analytic_dimensions_and_placements() {
    let common = format!(
        "#1=CARTESIAN_POINT('',(10.,20.,30.));\n#2=DIRECTION('',(1.,0.,0.));\n#3=AXIS1_PLACEMENT('',#1,#2);\n#4=AXIS2_PLACEMENT_3D('',#1,$,$);\n#6=CSG_SOLID('',#5);\n#7=CSG_SHAPE_REPRESENTATION('',(#6),#101);\n{STEP_MM}"
    );
    for (primitive, minimum, maximum, count) in [
        (
            "RIGHT_CIRCULAR_CYLINDER('',#3,6.,2.)",
            [10., 18., 28.],
            [16., 22., 32.],
            256,
        ),
        ("TORUS('',#3,5.,2.)", [8., 13., 23.], [12., 27., 37.], 4096),
        (
            "BLOCK('',#4,2.,3.,4.)",
            [10., 20., 30.],
            [12., 23., 34.],
            12,
        ),
    ] {
        let source = step_document(&format!("{common}\n#5={primitive};"));
        let model = decode_geometry(source.as_bytes(), "solid.step", || Ok(())).unwrap();
        for axis in 0..3 {
            assert!((model.scene.bounds.minimum[axis] - minimum[axis]).abs() < 1e-12);
            assert!((model.scene.bounds.maximum[axis] - maximum[axis]).abs() < 1e-12);
        }
        assert_eq!(model.scene.triangle_count(), count);
        let camera = ModelCamera::fit(model.scene.bounds);
        let frame = render_model(&model.scene, &camera, 256, false, || Ok(())).unwrap();
        assert!(
            frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| *p != [28, 34, 42, 255])
        );
    }
}

fn mapped_step() -> String {
    format!(
        "{STEP_MM}\n#1=CARTESIAN_POINT('',(3.,0.,0.));\n#2=SPHERE('',1.,#1);\n#3=CSG_SOLID('',#2);\n#4=AXIS2_PLACEMENT_3D('',#1,$,$);\n#5=CSG_SHAPE_REPRESENTATION('',(#3,#4),#101);\n#6=REPRESENTATION_MAP(#4,#5);\n#7=CARTESIAN_POINT('',(10.,0.,0.));\n#8=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',$,$,#7,2.,$);\n#9=MAPPED_ITEM('',#6,#8);\n#10=SHAPE_REPRESENTATION('',(#9),#101);"
    )
}

#[test]
fn step_mapped_instances_apply_inverse_origin_scale_and_nested_placement() {
    let entities = mapped_step();
    let source = step_document(&entities);
    let model = decode_geometry(source.as_bytes(), "mapped.step", || Ok(())).unwrap();
    assert_eq!(model.scene.triangle_count(), 3968);
    assert_eq!(model.scene.bounds.minimum, [8., -2., -2.]);
    assert_eq!(model.scene.bounds.maximum, [12., 2., 2.]);
    let nested = format!(
        "{entities}\n#11=REPRESENTATION_MAP(#4,#10);\n#12=MAPPED_ITEM('',#11,#8);\n#13=SHAPE_REPRESENTATION('',(#12),#101);"
    );
    let source = step_document(&nested);
    let model = decode_geometry(source.as_bytes(), "nested.step", || Ok(())).unwrap();
    assert_eq!(model.scene.triangle_count(), 3968);
    assert_eq!(model.scene.bounds.minimum, [20., -4., -4.]);
    assert_eq!(model.scene.bounds.maximum, [28., 4., 4.]);
}

#[test]
fn step_unit_conversion_is_attached_to_context_and_mixed_units_are_refused() {
    let source =
        step_document(&mapped_step().replace("SI_UNIT(.MILLI.,.METRE.)", "SI_UNIT($,.METRE.)"));
    let model = decode_geometry(source.as_bytes(), "meters.step", || Ok(())).unwrap();
    assert_eq!(model.scene.bounds.minimum, [8000., -2000., -2000.]);
    let inch = mapped_step().replace(
        "GLOBAL_UNIT_ASSIGNED_CONTEXT((#100))",
        "GLOBAL_UNIT_ASSIGNED_CONTEXT((#103))",
    );
    let source = step_document(&format!(
        "{inch}\n#102=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#100);\n#103=(CONVERSION_BASED_UNIT('inch',#102) LENGTH_UNIT() NAMED_UNIT(*));"
    ));
    let model = decode_geometry(source.as_bytes(), "inch.step", || Ok(())).unwrap();
    assert!((model.scene.bounds.minimum[0] - 203.2).abs() < 1e-10);
    let mixed = mapped_step().replace(
        "#10=SHAPE_REPRESENTATION('',(#9),#101)",
        "#10=SHAPE_REPRESENTATION('',(#9),#105)",
    );
    let source = step_document(&format!(
        "{mixed}\n#104=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT($,.METRE.));\n#105=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#104)) REPRESENTATION_CONTEXT('',''));"
    ));
    assert!(
        decode_geometry(source.as_bytes(), "mixed.step", || Ok(()))
            .unwrap_err()
            .to_string()
            .contains("mixed")
    );
}

#[test]
fn step_mapped_cycles_geometry_expansion_and_invalid_analytic_parameters_fail() {
    let entities = mapped_step();
    for invalid in [
        entities.replace("SPHERE('',1.,#1)", "SPHERE('',-1.,#1)"),
        entities.replace("SPHERE('',1.,#1)", "SPHERE('',NaN,#1)"),
        entities.replace(
            "#8=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',$,$,#7,2.,$)",
            "#8=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',$,$,#7,0.,$)",
        ),
        entities.replace(
            "#6=REPRESENTATION_MAP(#4,#5)",
            "#6=REPRESENTATION_MAP(#4,#10)",
        ),
        entities.replace("#3=CSG_SOLID('',#2)", "#3=MANIFOLD_SOLID_BREP('',#2)"),
        entities.replace(
            "#3=CSG_SOLID('',#2)",
            "#3=CSG_SOLID('',#200)\n;#200=BOOLEAN_RESULT(.UNION.,#2,#2)",
        ),
    ] {
        let source = step_document(&invalid);
        assert!(decode_geometry(source.as_bytes(), "invalid.step", || Ok(())).is_err());
    }
    let instances = std::iter::repeat_n("#9", 26).collect::<Vec<_>>().join(",");
    let source = step_document(&entities.replace(
        "#10=SHAPE_REPRESENTATION('',(#9),#101)",
        &format!("#10=SHAPE_REPRESENTATION('',({instances}),#101)"),
    ));
    assert!(
        decode_geometry(source.as_bytes(), "large.step", || Ok(()))
            .unwrap_err()
            .to_string()
            .contains("triangle limit")
    );
    let checks = std::cell::Cell::new(0);
    let source = step_document(&entities);
    assert!(
        decode_geometry(source.as_bytes(), "cancelled.step", || {
            checks.set(checks.get() + 1);
            if checks.get() > 150 {
                bail!("cancelled tessellation");
            }
            Ok(())
        })
        .unwrap_err()
        .to_string()
        .contains("cancelled tessellation")
    );
}
