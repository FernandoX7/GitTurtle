//! Real-binary Linux seccomp integration tests for confined 3D native decoders
//! (`ufbx` for `.obj`/`.fbx` and batched `meshopt` for `EXT_meshopt_compression` GLB).
#![cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

use gitturtle::{
    mesh_helper::{self, Client, Failure},
    pdf_helper::{DEADLINE, HELPER_FLAG, HelperCommand, Runner, Slots},
};
use gitturtle_preview::pdf_helper::{MeshErrorKind, PROTOCOL_VERSION};

fn real_client() -> Client<'static> {
    let slots: &'static Slots = Box::leak(Box::new(Slots::new(2)));
    let version = PROTOCOL_VERSION.to_string();
    let parent = std::process::id().to_string();
    let command = HelperCommand::new(
        env!("CARGO_BIN_EXE_gitturtle"),
        [HELPER_FLAG, &version, &parent],
    );
    Client::with(Runner::with(command, DEADLINE, slots))
}

#[test]
fn real_seccomp_helper_decodes_obj_via_ufbx() {
    let obj = b"v 0 0 0\nv 2 0 0\nv 0 3 0\nv 0 0 4\nf 1 2 3\nf 1 2 4\nf 2 3 4\nf 3 1 4\n";
    let client = real_client();
    let geometry = mesh_helper::decode_geometry_with(&client, obj, "tetra.obj", &|| Ok(()))
        .expect("confined ufbx helper decodes valid OBJ under seccomp");
    assert_eq!(geometry.format, "OBJ");
    assert_eq!(geometry.scene.triangle_count(), 4);
}

#[test]
fn real_seccomp_helper_decodes_meshopt_glb_in_one_batch() {
    let glb = include_bytes!("../../preview/tests/fixtures/models/glb/meshopt-arch-before.glb");
    let expected = gitturtle_preview::model3d::decode_geometry(glb, "arch.glb", || Ok(()))
        .expect("reference decode");
    let client = real_client();
    let confined = mesh_helper::decode_geometry_with(&client, glb, "arch.glb", &|| Ok(()))
        .expect("confined meshopt helper decodes EXT_meshopt_compression GLB under seccomp");
    assert_eq!(confined.format, "GLB");
    assert_eq!(
        confined.scene.triangle_count(),
        expected.scene.triangle_count()
    );
    assert_eq!(confined.scene.bounds, expected.scene.bounds);
}

#[test]
fn real_seccomp_helper_rejects_corrupt_fbx_without_sigsys() {
    let client = real_client();
    let result = client.decode_ufbx(
        b"Kaydara FBX Binary  \x00\x1a\x00corrupt",
        false,
        &|| Ok(()),
    );
    assert!(
        matches!(result, Err(Failure::Document(MeshErrorKind::DecodeFailed))),
        "expected clean DecodeFailed under seccomp, got {result:?}"
    );
}

#[test]
fn real_seccomp_helper_rejects_corrupt_meshopt_stream_without_sigsys() {
    use gitturtle_preview::model3d::{MeshoptFilter, MeshoptMode, MeshoptViewJob};
    let client = real_client();
    let bad_job = MeshoptViewJob {
        view_index: 0,
        mode: MeshoptMode::Attributes,
        filter: MeshoptFilter::None,
        count: 4,
        stride: 12,
        output_len: 48,
        compressed: vec![0xa0, 0xff, 0xff, 0xff],
    };
    let result = client.decode_meshopt(&[bad_job], &|| Ok(()));
    assert!(
        matches!(result, Err(Failure::Document(MeshErrorKind::DecodeFailed))),
        "expected clean DecodeFailed under seccomp, got {result:?}"
    );
}

#[test]
fn real_seccomp_helper_respects_cancellation_without_hanging() {
    let obj = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
    let client = real_client();
    let result = client.decode_ufbx(obj, true, &|| anyhow::bail!("cancelled by caller"));
    assert!(matches!(result, Err(Failure::Cancelled(_))));
}
