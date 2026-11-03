//! JPEG 2000 images for the confined Linux helper, decoded by
//! `hayro-jpeg2000` through its `image` integration after
//! [`crate::jpeg2000::preflight`]. Only [`crate::image_helper`] calls this.
//!
//! The entry is `Image::new` with GitTurtle's own settings followed by its
//! `ImageDecoder` reads; the crate's reader-based decoder and its process-wide
//! `image` hook stay unused (a source test in `avif.rs` holds that). 0.4.1 has no
//! memory budget (#1549) and panics on some inputs (#1409, #1649): the preflight
//! refuses the known cases before the tile vector is allocated, the helper turns
//! a panic into an error kind, and its data limit and deadline bound the rest.
#![forbid(unsafe_code)]

use std::time::Instant;

use hayro_jpeg2000::{DecodeSettings, Image};
use image::{DynamicImage, ImageDecoder};

use crate::image_helper::{self, Decoded, ImageErrorKind, ImageFlags, ImageTimings};
use crate::{MAX_DECODER_BYTES, jpeg2000, scaled_dimensions};

pub(crate) fn decode(
    bytes: &[u8],
    edge: u32,
    timings: &mut ImageTimings,
) -> Result<Decoded, ImageErrorKind> {
    use ImageErrorKind::{DecodedTooLarge, Jpeg2000Unreadable};
    let start = Instant::now();
    let preflight = jpeg2000::preflight(bytes)?;
    let original = preflight.original();
    let target = scaled_dimensions(original.0, original.1, edge);
    // Reduced resolution only where 0.4.1 decodes it correctly (#1409).
    let reduce = preflight.may_reduce() && target != original;
    let settings = DecodeSettings {
        resolve_palette_indices: true,
        strict: false,
        target_resolution: reduce.then_some(target),
    };
    let image = Image::new(bytes, &settings).map_err(|_| Jpeg2000Unreadable)?;
    let size = image.dimensions();
    // The skipped levels hayro-jpeg2000 chose: its size must be one GitTurtle
    // computes from SIZ for a level it may skip.
    let deepest = if reduce {
        (original.0 / target.0)
            .min(original.1 / target.1)
            .checked_ilog2()
            .unwrap_or(0)
    } else {
        0
    };
    let level = (0..=deepest)
        .find(|level| preflight.at_level(*level) == size)
        .ok_or(Jpeg2000Unreadable)?;
    if preflight.decode_bytes(level) > MAX_DECODER_BYTES {
        return Err(DecodedTooLarge);
    }
    let expected =
        u64::from(size.0) * u64::from(size.1) * u64::from(image.color_type().bytes_per_pixel());
    if image.total_bytes() != expected {
        return Err(Jpeg2000Unreadable);
    }
    image_helper::check_rgba_budget(size.0, size.1)?;
    timings.parse_us = image_helper::elapsed_us(start);

    let start = Instant::now();
    let decoded = DynamicImage::from_decoder(image).map_err(|_| Jpeg2000Unreadable)?;
    timings.decode_us = image_helper::elapsed_us(start);
    // Some reduced or subsampled images decode one column too wide (#1405).
    if (decoded.width(), decoded.height()) != size {
        return Err(Jpeg2000Unreadable);
    }

    let start = Instant::now();
    // A reduced level scales to the full size's output like any other.
    let decoded = image_helper::finish(decoded.into_rgba8(), original, edge, ImageFlags::default());
    timings.convert_us = image_helper::elapsed_us(start);
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::image_helper::tests::{assert_near, decoded, pixel};

    const QUADRANTS: [[u8; 4]; 4] = [
        [220, 30, 30, 255],
        [30, 200, 60, 255],
        [30, 50, 220, 255],
        [240, 240, 240, 255],
    ];

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(name),
        )
        .unwrap()
    }

    fn jpeg2000(bytes: &[u8], edge: u32) -> Result<image_helper::ImageReply, ImageErrorKind> {
        decoded(crate::confined_image_format(bytes).unwrap(), edge, bytes)
    }

    fn assert_quadrants(
        image: &crate::ImagePreview,
        colours: [[u8; 4]; 4],
        tolerance: u8,
        case: &str,
    ) {
        let (width, height) = (image.width, image.height);
        for (index, (x, y)) in [(1, 1), (3, 1), (1, 3), (3, 3)].into_iter().enumerate() {
            let at = pixel(image, width * x / 4, height * y / 4);
            assert_near(
                at,
                colours[index],
                tolerance,
                &format!("{case} quadrant {index}"),
            );
        }
    }

    #[test]
    fn decodes_jpeg2000_colour_grey_alpha_profiles_and_palettes() {
        for name in [
            "jpeg2000-rgb.jp2",
            "jpeg2000-rgb.j2k",
            "jpeg2000-palette.jp2",
        ] {
            let reply =
                jpeg2000(&fixture(name), 1600).unwrap_or_else(|kind| panic!("{name}: {kind:?}"));
            assert_eq!((reply.image.width, reply.image.height), (64, 48), "{name}");
            assert_eq!(reply.flags, ImageFlags::default());
            assert_quadrants(&reply.image, QUADRANTS, 2, name);
        }
        let raw = jpeg2000(&fixture("jpeg2000-rgb.j2k"), 1600).unwrap();
        assert_eq!(raw.image.format, "JPEG 2000 / J2K codestream · first image");
        // 16-bit grey to 8 bits.
        let grey = jpeg2000(&fixture("jpeg2000-grey16.jp2"), 1600).unwrap();
        for (x, value) in [(0, 0), (32, 130), (63, 255)] {
            assert_near(
                pixel(&grey.image, x, 10),
                [value, value, value, 255],
                2,
                "grey",
            );
        }
        let half = |[r, g, b, _]: [u8; 4]| [r, g, b, 128];
        let alpha = jpeg2000(&fixture("jpeg2000-alpha.jp2"), 1600).unwrap();
        let [red, green, blue, white] = QUADRANTS;
        assert_quadrants(
            &alpha.image,
            [red, half(green), blue, half(white)],
            2,
            "alpha",
        );
        // The Display P3 ICC profile is converted to sRGB (the script prints
        // the expected values for its 2.2 gamma).
        let icc = jpeg2000(&fixture("jpeg2000-display-p3-icc.jp2"), 1600).unwrap();
        assert_quadrants(
            &icc.image,
            [
                [218, 67, 47, 255],
                [0, 185, 77, 255],
                [62, 90, 209, 255],
                [129, 129, 129, 255],
            ],
            6,
            "display p3",
        );
    }

    #[test]
    fn decodes_subsampled_jpeg2000_at_full_and_large_images_at_reduced_resolution() {
        // Subsampled chroma decodes at full resolution, then scales.
        let bytes = fixture("jpeg2000-subsampled.jp2");
        assert!(!jpeg2000::preflight(&bytes).unwrap().may_reduce());
        let reply = jpeg2000(&bytes, 32).unwrap();
        assert_eq!(
            (reply.image.original_width, reply.image.original_height),
            (64, 48)
        );
        assert_eq!((reply.image.width, reply.image.height), (32, 24));
        assert_quadrants(&reply.image, QUADRANTS, 16, "subsampled");

        let bytes = fixture("jpeg2000-large.jp2");
        let preflight = jpeg2000::preflight(&bytes).unwrap();
        assert!(preflight.may_reduce());
        assert_eq!(preflight.original(), (1024, 768));
        assert_eq!(preflight.at_level(3), (128, 96));
        let settings = DecodeSettings {
            resolve_palette_indices: true,
            strict: false,
            target_resolution: Some((128, 96)),
        };
        // Three levels skipped: an eighth of the samples per side.
        assert_eq!(
            Image::new(&bytes, &settings).unwrap().dimensions(),
            (128, 96)
        );
        let reply = jpeg2000(&bytes, 128).unwrap();
        assert_eq!(
            (reply.image.original_width, reply.image.original_height),
            (1024, 768)
        );
        assert_eq!((reply.image.width, reply.image.height), (128, 96));
        for (x, y) in [(16, 12), (64, 48), (112, 84)] {
            let expected = [(x * 255 / 127) as u8, (y * 255 / 95) as u8, 128, 255];
            assert_near(pixel(&reply.image, x, y), expected, 10, "gradient");
        }
    }

    fn set(bytes: &mut [u8], offset: usize, value: &[u8]) {
        bytes[offset..offset + value.len()].copy_from_slice(value);
    }

    fn position(bytes: &[u8], kind: &[u8; 4]) -> usize {
        bytes.windows(4).position(|found| found == kind).unwrap()
    }

    /// https://github.com/LaurenzV/hayro/issues/1549: a 97-byte codestream
    /// declaring 100000 × 100000 that asked hayro-jpeg2000 for about 95 GB.
    const ISSUE_1549: &str = "ff4fff5100290000000186a0000186a00000000000000000000186a0000186a000000000000000000001070101ff52000c00000001000004040001ff5c00044040ff90000a0000000000000001ff9300000000000000000000000000000000ffd9";

    #[test]
    fn hostile_jpeg2000_ends_in_its_error_kind_before_decoding() {
        use ImageErrorKind::*;
        let issue_1549: Vec<u8> = (0..ISSUE_1549.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&ISSUE_1549[at..at + 2], 16).unwrap())
            .collect();
        assert_eq!(issue_1549.len(), 97);
        // https://github.com/LaurenzV/hayro/pull/1558 (comment of 2026-10-22):
        // 16383 × 16383 with 1 × 1 tiles passes hayro's sample budget and still
        // asks for 25.8 GB of tiles; GitTurtle's pixel limit refuses it first.
        let mut one_pixel_tiles = issue_1549.clone();
        for offset in [8, 12] {
            set(&mut one_pixel_tiles, offset, &16_383u32.to_be_bytes());
        }
        for offset in [24, 28] {
            set(&mut one_pixel_tiles, offset, &1u32.to_be_bytes());
        }
        // The same tiles within the pixel limit: refused by the tile count.
        let mut many_tiles = one_pixel_tiles.clone();
        for offset in [8, 12] {
            set(&mut many_tiles, offset, &5_000u32.to_be_bytes());
        }
        // 1 × 1 tiles: 21845 × 3 is exactly the 16-bit tile index's 65,535;
        // 32768 × 2 is one over.
        let tiles = |width: u32, height: u32| {
            let mut bytes = one_pixel_tiles.clone();
            set(&mut bytes, 8, &width.to_be_bytes());
            set(&mut bytes, 12, &height.to_be_bytes());
            bytes
        };
        assert!(jpeg2000::preflight(&tiles(21_845, 3)).is_ok());
        let over_count = tiles(32_768, 2);
        let rgb = fixture("jpeg2000-rgb.j2k");
        let mut five_components = rgb.clone();
        set(&mut five_components, 40, &5u16.to_be_bytes());
        let mut seventeen_bits = rgb.clone();
        seventeen_bits[42] = 16;
        let mut origin = rgb.clone();
        set(&mut origin, 16, &64u32.to_be_bytes());
        let jp2 = fixture("jpeg2000-rgb.jp2");
        let mut jpx = jp2.clone();
        set(&mut jpx, 20, b"jpx ");
        set(&mut jpx, 28, b"jpx ");
        let mut jpx_compatible = jpx.clone();
        set(&mut jpx_compatible, 28, b"jp2 ");
        let mut header_disagrees = jp2.clone();
        set(
            &mut header_disagrees,
            position(&jp2, b"ihdr") + 4,
            &47u32.to_be_bytes(),
        );
        // https://github.com/LaurenzV/hayro/issues/1649: a palette column of
        // 32 bits, and an empty component mapping.
        let palette = fixture("jpeg2000-palette.jp2");
        let pclr = position(&palette, b"pclr") + 4;
        let mut deep_palette = palette.clone();
        deep_palette[pclr + 3] = 31;
        let mut no_entries = palette.clone();
        set(&mut no_entries, pclr, &0u16.to_be_bytes());
        let cmap = position(&palette, b"cmap") - 4;
        let mut empty_mapping = palette[..cmap].to_vec();
        empty_mapping.extend_from_slice(b"\0\0\0\x08cmap");
        empty_mapping.extend_from_slice(&palette[cmap + 20..]);
        let jp2h = position(&empty_mapping, b"jp2h") - 4;
        let length = u32::from_be_bytes(empty_mapping[jp2h..jp2h + 4].try_into().unwrap()) - 12;
        set(&mut empty_mapping, jp2h, &length.to_be_bytes());
        for (case, bytes, kind) in [
            ("#1549", issue_1549, EdgeLimit),
            ("#1558 1 × 1 tiles", one_pixel_tiles, PixelLimit),
            ("25,000,000 tiles", many_tiles, Jpeg2000TooManyTiles),
            ("65,536 tiles", over_count, Jpeg2000TooManyTiles),
            (
                "five components",
                five_components,
                Jpeg2000TooManyComponents,
            ),
            ("17-bit samples", seventeen_bits, Jpeg2000TooDeep),
            ("origin at the width", origin, Jpeg2000Unreadable),
            ("JPX not JP2-compatible", jpx, JpxNotJp2Compatible),
            (
                "ihdr and SIZ disagree",
                header_disagrees,
                Jpeg2000Unreadable,
            ),
            ("#1649 32-bit palette column", deep_palette, Jpeg2000TooDeep),
            ("#1649 empty cmap", empty_mapping, Jpeg2000Unreadable),
            ("palette without entries", no_entries, Jpeg2000Unreadable),
        ] {
            assert_eq!(jpeg2000(&bytes, 100).unwrap_err(), kind, "{case}");
        }
        // JPX that lists JP2 compatibility decodes.
        assert!(jpeg2000(&jpx_compatible, 100).is_ok());
    }
}
