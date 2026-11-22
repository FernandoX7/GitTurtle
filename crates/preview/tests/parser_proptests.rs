//! Property-based fuzzing tests for untrusted byte parsers in `gitturtle-preview`.
//!
//! Uses bounded deterministic case counts so CI stays fast while exercising
//! arbitrary, truncated, and header-prefixed payloads without panicking.

use std::path::Path;

use proptest::prelude::*;

fn bounded_config() -> ProptestConfig {
    ProptestConfig {
        cases: 64,
        failure_persistence: None,
        rng_seed: proptest::test_runner::RngSeed::Fixed(0x5A17_F001_0001),
        ..ProptestConfig::default()
    }
}

proptest! {
    #![proptest_config(bounded_config())]

    #[test]
    fn metadata_and_sniffers_never_panic_on_arbitrary_bytes(
        bytes in proptest::collection::vec(any::<u8>(), 0..512),
        ext in prop_oneof!["png", "wav", "zip", "ttf", "svg", "j2k", "jp2", "avif", "heic", "pdf", "bin"],
    ) {
        let _ = gitturtle_preview::metadata::iso_image_format(&bytes);
        let _ = gitturtle_preview::metadata::is_svg(&bytes);
        let _ = gitturtle_preview::metadata::is_image(&bytes);
        let _ = gitturtle_preview::metadata::jpeg2000_format(&bytes);
        let _ = gitturtle_preview::metadata::is_literal_text(&bytes);
        let _ = gitturtle_preview::metadata::container(&bytes);
        let _ = gitturtle_preview::detect_lfs_pointer(&bytes);
        let _ = gitturtle_preview::declared_dimensions(&bytes);
        let name = format!("fixture.{ext}");
        let inspected = gitturtle_preview::metadata::inspect(&bytes, Path::new(&name));
        prop_assert!(inspected.details.len() <= 128);
    }

    #[test]
    fn scaled_dimensions_always_stay_positive_and_within_edge(
        width in 1u32..=16_384,
        height in 1u32..=16_384,
        max_edge in 1u32..=4_096,
    ) {
        let (out_w, out_h) = gitturtle_preview::scaled_dimensions(width, height, max_edge);
        prop_assert!(out_w >= 1 && out_w <= max_edge);
        prop_assert!(out_h >= 1 && out_h <= max_edge);
        prop_assert!(out_w <= width && out_h <= height);
    }

    #[test]
    fn decode_image_never_panics_on_arbitrary_or_header_prefixed_bytes(
        prefix in prop_oneof![
            Just(b"".to_vec()),
            Just(b"\x89PNG\r\n\x1a\n".to_vec()),
            Just(b"\xff\xd8\xff\xe0".to_vec()),
            Just(b"GIF89a".to_vec()),
            Just(b"<svg xmlns=\"http://www.w3.org/2000/svg\">".to_vec()),
            Just(b"\x00\x00\x00\x0cjP  \r\n\x87\n".to_vec()),
            Just(b"\xff\x4f\xff\x51".to_vec()),
        ],
        tail in proptest::collection::vec(any::<u8>(), 0..256),
        ext in prop_oneof!["png", "jpg", "gif", "svg", "jp2", "j2k", "webp"],
    ) {
        let mut bytes = prefix;
        bytes.extend_from_slice(&tail);
        let name = format!("sample.{ext}");
        if let Ok(decoded) = gitturtle_preview::decode_image(&bytes, &name, 128) {
            prop_assert!(decoded.width <= 128 && decoded.height <= 128);
            prop_assert_eq!(decoded.rgba.len(), decoded.width as usize * decoded.height as usize * 4);
        }
    }

    #[test]
    fn model3d_decoders_never_panic_on_arbitrary_bytes(
        prefix in prop_oneof![
            Just(b"".to_vec()),
            Just(b"glTF\x02\x00\x00\x00".to_vec()),
            Just(b"PK\x03\x04".to_vec()),
            Just(b"ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n".to_vec()),
            Just(b"solid x\n".to_vec()),
            Just(b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n".to_vec()),
        ],
        tail in proptest::collection::vec(any::<u8>(), 0..256),
        ext in prop_oneof!["stl", "obj", "fbx", "glb", "3mf", "step"],
    ) {
        let mut bytes = prefix;
        bytes.extend_from_slice(&tail);
        let name = format!("mesh.{ext}");
        let _ = gitturtle_preview::model3d::decode_geometry(&bytes, &name, || Ok(()));
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
proptest! {
    #![proptest_config(bounded_config())]

    #[test]
    fn helper_wire_protocols_never_panic_on_arbitrary_frames(
        bytes in proptest::collection::vec(any::<u8>(), 0..512),
    ) {
        let _ = gitturtle_preview::pdf_helper::read_reply(bytes.as_slice());
        let _ = gitturtle_preview::pdf_helper::read_image_reply(
            bytes.as_slice(),
            gitturtle_preview::pdf_helper::ConfinedFormat::Avif,
            64,
        );
        let mut out = Vec::new();
        let _ = gitturtle_preview::pdf_helper::serve_buffer(bytes, &mut out);
    }
}
