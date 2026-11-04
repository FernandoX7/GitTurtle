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
    // hayro-jpeg2000 computes the size it decodes from SIZ and the levels it
    // chose to skip. It must be the size GitTurtle computes for the full image
    // or for a level it may skip, so the budget below holds for what is
    // decoded. `total_bytes()` and `DynamicImage::from_decoder`'s buffer both
    // derive from this size, so no later check of either can disagree.
    let size = image.dimensions();
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
    image_helper::check_rgba_budget(size.0, size.1)?;
    timings.parse_us = image_helper::elapsed_us(start);

    let start = Instant::now();
    let decoded = DynamicImage::from_decoder(image).map_err(|_| Jpeg2000Unreadable)?;
    timings.decode_us = image_helper::elapsed_us(start);

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

    fn issue_1549() -> Vec<u8> {
        let bytes: Vec<u8> = (0..ISSUE_1549.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&ISSUE_1549[at..at + 2], 16).unwrap())
            .collect();
        assert_eq!(bytes.len(), 97);
        bytes
    }

    /// A JP2 box of `kind` around `payload`, with an explicit length.
    fn jp2_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut bytes = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        bytes.extend_from_slice(kind);
        bytes.extend_from_slice(payload);
        bytes
    }

    /// The whole top-level box of `kind` (with an explicit length) in `bytes`.
    fn whole_box(bytes: &[u8], kind: &[u8; 4]) -> Vec<u8> {
        let start = position(bytes, kind) - 4;
        let length = u32::from_be_bytes(bytes[start..start + 4].try_into().unwrap());
        bytes[start..start + length as usize].to_vec()
    }

    /// The RGB fixture's signature and `ftyp` boxes, a `jp2h` of `children`,
    /// then the `jp2c` box that ends `codestream_of`.
    fn jp2(children: &[&[u8]], codestream_of: &[u8]) -> Vec<u8> {
        let mut bytes = fixture("jpeg2000-rgb.jp2")[..32].to_vec();
        bytes.extend(jp2_box(b"jp2h", &children.concat()));
        bytes.extend_from_slice(&codestream_of[position(codestream_of, b"jp2c") - 4..]);
        bytes
    }

    /// A `cmap` entry: SIZ component, mapping type and palette column.
    fn entry(component: u16, kind: u8, column: u8) -> [u8; 4] {
        let [high, low] = component.to_be_bytes();
        [high, low, kind, column]
    }

    fn cmap(entries: &[[u8; 4]]) -> Vec<u8> {
        jp2_box(b"cmap", &entries.concat())
    }

    /// A `cdef` box (I.5.3.6) of channel, type and association triples
    /// (type 1 is opacity; association 0 is the whole image).
    fn cdef(channels: &[[u16; 3]]) -> Vec<u8> {
        let mut definitions = (channels.len() as u16).to_be_bytes().to_vec();
        for value in channels.as_flattened() {
            definitions.extend_from_slice(&value.to_be_bytes());
        }
        jp2_box(b"cdef", &definitions)
    }

    /// Red, green, blue and opacity, in component order.
    const RGBA_CHANNELS: [[u16; 3]; 4] = [[0, 0, 1], [1, 0, 2], [2, 0, 3], [3, 1, 0]];

    /// The palette fixture (one 8-bit index component; four entries in three
    /// 8-bit columns; sRGB) with `mapping` for its `cmap` box.
    fn palette_with(mapping: &[u8]) -> Vec<u8> {
        let palette = fixture("jpeg2000-palette.jp2");
        let [ihdr, colr, pclr] = [b"ihdr", b"colr", b"pclr"].map(|kind| whole_box(&palette, kind));
        jp2(&[&ihdr, &colr, &pclr, mapping], &palette)
    }

    /// hayro-jpeg2000 makes one full-size plane per `cmap` entry and one
    /// output channel each; the preflight refuses a mapping it cannot account
    /// for, before anything is decoded.
    #[test]
    fn palette_component_mappings_are_refused_before_decoding() {
        use ImageErrorKind::{Jpeg2000TooManyComponents, Jpeg2000Unreadable};
        // The fixture's own mapping, rebuilt, decodes as before.
        let columns = [entry(0, 1, 0), entry(0, 1, 1), entry(0, 1, 2)];
        let reply = jpeg2000(&palette_with(&cmap(&columns)), 1600).unwrap();
        assert_quadrants(&reply.image, QUADRANTS, 2, "rebuilt palette");
        let mut wrong = Vec::new();
        for (case, mapping, kind) in [
            (
                "16 entries",
                cmap(&[entry(0, 1, 0); 16]),
                Jpeg2000TooManyComponents,
            ),
            (
                "5 entries",
                cmap(&[entry(0, 1, 0); 5]),
                Jpeg2000TooManyComponents,
            ),
            (
                "component 1 of 1",
                cmap(&[entry(0, 1, 0), entry(1, 1, 1), entry(0, 1, 2)]),
                Jpeg2000Unreadable,
            ),
            (
                "mapping type 2",
                cmap(&[entry(0, 1, 0), entry(0, 2, 1), entry(0, 1, 2)]),
                Jpeg2000Unreadable,
            ),
            (
                "column 3 of 3",
                cmap(&[entry(0, 1, 0), entry(0, 1, 1), entry(0, 1, 3)]),
                Jpeg2000Unreadable,
            ),
            (
                "a partial entry",
                jp2_box(b"cmap", &[0, 0, 1]),
                Jpeg2000Unreadable,
            ),
        ] {
            let bytes = palette_with(&mapping);
            // Refused by the preflight itself, and so through `serve`.
            let found = (
                jpeg2000::preflight(&bytes).err(),
                jpeg2000(&bytes, 100).err(),
            );
            if found != (Some(kind), Some(kind)) {
                wrong.push((case, found));
            }
        }
        assert!(wrong.is_empty(), "{wrong:?}");
    }

    /// hayro-jpeg2000 0.4.1's `pclr::parse` fails on a signed palette column
    /// (descriptor bit 0x80) and, not strict, drops the palette: the alpha
    /// fixture's four components with no `colr` then decode as CMYK, which
    /// the palette's budget (four component planes and one mapped) does not
    /// cover. The preflight refuses the column.
    #[test]
    fn a_signed_palette_column_is_refused() {
        use hayro_jpeg2000::ColorSpace;
        let alpha = fixture("jpeg2000-alpha.jp2");
        let ihdr = whole_box(&alpha, b"ihdr");
        let mapping = cmap(&[entry(0, 1, 0)]);
        // Two entries in one 8-bit column, unsigned (0x07) or signed (0x87).
        let with = |descriptor: u8| {
            let pclr = jp2_box(b"pclr", &[0, 2, 1, descriptor, 0, 1]);
            jp2(&[&ihdr, &pclr, &mapping], &alpha)
        };
        let (unsigned, signed) = (with(0x07), with(0x87));
        let settings = DecodeSettings {
            resolve_palette_indices: true,
            strict: false,
            target_resolution: None,
        };
        let image = Image::new(&unsigned, &settings).unwrap();
        assert!(matches!(image.color_space(), ColorSpace::Gray));
        let image = Image::new(&signed, &settings).unwrap();
        assert!(matches!(image.color_space(), ColorSpace::CMYK));
        let plane = 64 * 48 * 4;
        assert_eq!(
            jpeg2000::preflight(&unsigned).unwrap().decode_bytes(0),
            plane * (4 + 1) + plane
        );
        assert_eq!(
            jpeg2000::preflight(&signed),
            Err(ImageErrorKind::Jpeg2000Unreadable)
        );
        assert_eq!(
            jpeg2000(&signed, 1600).unwrap_err(),
            ImageErrorKind::Jpeg2000Unreadable
        );
    }

    /// The budget counts every codestream plane and one per `cmap` entry,
    /// alpha included.
    #[test]
    fn palette_budget_counts_each_component_and_mapping_entry() {
        // The alpha fixture's four 8-bit components: the first indexes the
        // palette fixture's three columns and the fourth is used directly,
        // as opacity.
        let alpha = fixture("jpeg2000-alpha.jp2");
        let palette = fixture("jpeg2000-palette.jp2");
        let mapping = cmap(&[
            entry(0, 1, 0),
            entry(0, 1, 1),
            entry(0, 1, 2),
            entry(3, 0, 0),
        ]);
        let bytes = jp2(
            &[
                &whole_box(&alpha, b"ihdr"),
                &whole_box(&alpha, b"colr"),
                &whole_box(&palette, b"pclr"),
                &mapping,
                &cdef(&RGBA_CHANNELS),
            ],
            &alpha,
        );
        // 64 × 48 f32 samples in four component and four mapped planes (as
        // many as the `cdef` copy of the four output planes holds), and the
        // one 64 × 48 tile-component.
        assert_eq!(
            jpeg2000::preflight(&bytes).unwrap().decode_bytes(0),
            64 * 48 * 4 * (4 + 4) + 64 * 48 * 4
        );
        // Every first-component value indexes the last palette entry; the
        // opacity is the fixture's.
        let reply = jpeg2000(&bytes, 1600).unwrap();
        let [opaque, half] = [255, 128].map(|alpha| [240, 240, 240, alpha]);
        assert_quadrants(
            &reply.image,
            [opaque, half, opaque, half],
            2,
            "palette and alpha",
        );
    }

    /// With a `cdef` box, hayro-jpeg2000 0.4.1 clones every output plane to
    /// reorder them (`lib.rs` 217-234), so the budget counts twice the output
    /// planes when that is more than the palette step holds.
    #[test]
    fn a_channel_definition_counts_its_copy_of_the_output_planes() {
        let alpha = fixture("jpeg2000-alpha.jp2");
        let [ihdr, colr] = [b"ihdr", b"colr"].map(|kind| whole_box(&alpha, kind));
        let defined = jp2(&[&ihdr, &colr, &cdef(&RGBA_CHANNELS)], &alpha);
        // 64 × 48 f32 samples in the four component planes, then in their
        // four copies, and the one 64 × 48 tile-component.
        let plane = 64 * 48 * 4;
        assert_eq!(
            jpeg2000::preflight(&alpha).unwrap().decode_bytes(0),
            plane * 4 + plane
        );
        assert_eq!(
            jpeg2000::preflight(&defined).unwrap().decode_bytes(0),
            plane * 8 + plane
        );
        // The same opacity either way.
        let [red, green, blue, white] = QUADRANTS;
        let half = |[r, g, b, _]: [u8; 4]| [r, g, b, 128];
        for (case, bytes) in [("inferred", &alpha), ("defined", &defined)] {
            let reply = jpeg2000(bytes, 1600).unwrap_or_else(|kind| panic!("{case}: {kind:?}"));
            assert_quadrants(&reply.image, [red, half(green), blue, half(white)], 2, case);
        }
        // With a palette the output planes are the `cmap` entries: one index
        // component and three mapped planes, then three copies.
        let palette = fixture("jpeg2000-palette.jp2");
        let [ihdr, colr, pclr, mapping] =
            [b"ihdr", b"colr", b"pclr", b"cmap"].map(|kind| whole_box(&palette, kind));
        let channels = cdef(&[[0, 0, 1], [1, 0, 2], [2, 0, 3]]);
        for (case, children, planes) in [
            ("palette", vec![&ihdr, &colr, &pclr, &mapping], 1 + 3),
            (
                "palette and cdef",
                vec![&ihdr, &colr, &pclr, &mapping, &channels],
                3 * 2,
            ),
        ] {
            let children: Vec<&[u8]> = children.into_iter().map(Vec::as_slice).collect();
            let bytes = jp2(&children, &palette);
            assert_eq!(
                jpeg2000::preflight(&bytes).unwrap().decode_bytes(0),
                plane * planes + plane,
                "{case}"
            );
            assert_quadrants(&jpeg2000(&bytes, 1600).unwrap().image, QUADRANTS, 2, case);
        }
        // hayro-jpeg2000 keeps the last of two; the preflight refuses a second.
        let twice = jp2(
            &[&ihdr, &colr, &pclr, &mapping, &channels, &channels],
            &palette,
        );
        assert_eq!(
            jpeg2000(&twice, 1600).unwrap_err(),
            ImageErrorKind::Jpeg2000Unreadable
        );
    }

    /// An RGBA JP2 that cannot skip resolution levels (one component sampled
    /// 2 × 2) fits the budget in its four component planes but not with the
    /// `cdef` copy of them. One that can skip levels is budgeted at the level
    /// it decodes, copies included.
    #[test]
    fn a_channel_definition_over_the_budget_is_refused_before_decoding() {
        let alpha = fixture("jpeg2000-alpha.jp2");
        // 4,096 × 4,000 in the fixture's 64 × 48 tiles (5,376 of them), with
        // the last component sampled `sampling` × `sampling`.
        let shape = |sampling: u8, channels: Option<&[[u16; 3]]>| {
            let mut ihdr = whole_box(&alpha, b"ihdr");
            set(&mut ihdr, 8, &4_000u32.to_be_bytes());
            set(&mut ihdr, 12, &4_096u32.to_be_bytes());
            let colr = whole_box(&alpha, b"colr");
            let definition = channels.map(cdef).unwrap_or_default();
            let mut bytes = jp2(&[&ihdr, &colr, &definition], &alpha);
            let marker = position(&bytes, b"jp2c") + 4;
            set(&mut bytes, marker + 8, &4_096u32.to_be_bytes());
            set(&mut bytes, marker + 12, &4_000u32.to_be_bytes());
            set(&mut bytes, marker + 42 + 3 * 3 + 1, &[sampling, sampling]);
            bytes
        };
        let (plane, tile) = (4_096 * 4_000 * 4, 64 * 48 * 4);
        let without = jpeg2000::preflight(&shape(2, None)).unwrap();
        assert!(!without.may_reduce());
        assert_eq!(without.decode_bytes(0), plane * 4 + tile);
        assert!(without.decode_bytes(0) <= MAX_DECODER_BYTES);
        let over = shape(2, Some(&RGBA_CHANNELS));
        assert_eq!(
            jpeg2000::preflight(&over).unwrap().decode_bytes(0),
            plane * 8 + tile
        );
        assert_eq!(
            jpeg2000(&over, 1600).unwrap_err(),
            ImageErrorKind::DecodedTooLarge
        );
        // With 1 × 1 sampling one level is skipped at a 1,600-pixel edge, and
        // the copies are of its 2,048 × 2,000 planes.
        let reducible = jpeg2000::preflight(&shape(1, Some(&RGBA_CHANNELS))).unwrap();
        assert!(reducible.may_reduce());
        assert!(reducible.decode_bytes(0) > MAX_DECODER_BYTES);
        assert_eq!(
            reducible.decode_bytes(1),
            2_048 * 2_000 * 4 * 8 + 32 * 24 * 4
        );
    }

    /// hayro-jpeg2000 0.4.1 converts a colour profile while the first
    /// decode's planes and their 8-bit copy are alive, and decodes a second
    /// time when moxcms refuses the profile: three output planes take
    /// 3 + 1 + max(3, 2 × 3 + 1) = 11 planes and two tile-components.
    #[test]
    fn a_colour_profile_counts_a_second_decode() {
        let icc = fixture("jpeg2000-display-p3-icc.jp2");
        let rgb = fixture("jpeg2000-rgb.jp2");
        let plane = 64 * 48 * 4;
        let budget = |bytes: &[u8]| jpeg2000::preflight(bytes).unwrap().decode_bytes(0);
        assert_eq!(budget(&rgb), plane * 3 + plane);
        assert_eq!(budget(&icc), plane * 11 + 2 * plane);
        // hayro reads only the first `colr`: CIELab (14) and ROMM-RGB (21)
        // convert through built-in profiles, sRGB (16) does not.
        let ihdr = whole_box(&rgb, b"ihdr");
        let [profile, srgb] = [&icc, &rgb].map(|bytes| whole_box(bytes, b"colr"));
        let enumerated =
            |space: u32| jp2_box(b"colr", &[&[1, 0, 0][..], &space.to_be_bytes()].concat());
        for (case, colours, planes, contexts) in [
            ("CIELab", vec![enumerated(14)], 11, 2),
            ("ROMM-RGB", vec![enumerated(21)], 11, 2),
            (
                "a profile after sRGB",
                vec![srgb.clone(), profile.clone()],
                3,
                1,
            ),
            (
                "sRGB after a profile",
                vec![profile.clone(), srgb.clone()],
                11,
                2,
            ),
        ] {
            let mut children = vec![ihdr.as_slice()];
            children.extend(colours.iter().map(Vec::as_slice));
            assert_eq!(
                budget(&jp2(&children, &rgb)),
                plane * planes + contexts * plane,
                "{case}"
            );
        }

        // 4,096 × 2,048 with the last component sampled 2 × 2, so no level
        // is skipped: three planes fit the budget, eleven do not.
        let shape = |colour: &[u8]| {
            let mut ihdr = whole_box(&icc, b"ihdr");
            set(&mut ihdr, 8, &2_048u32.to_be_bytes());
            set(&mut ihdr, 12, &4_096u32.to_be_bytes());
            let mut bytes = jp2(&[&ihdr, colour], &icc);
            let marker = position(&bytes, b"jp2c") + 4;
            set(&mut bytes, marker + 8, &4_096u32.to_be_bytes());
            set(&mut bytes, marker + 12, &2_048u32.to_be_bytes());
            set(&mut bytes, marker + 42 + 3 * 2 + 1, &[2, 2]);
            bytes
        };
        let (plane, tile) = (4_096 * 2_048 * 4, 64 * 48 * 4);
        let without = jpeg2000::preflight(&shape(&srgb)).unwrap();
        assert!(!without.may_reduce());
        assert_eq!(without.decode_bytes(0), plane * 3 + tile);
        assert!(without.decode_bytes(0) <= MAX_DECODER_BYTES);
        let over = shape(&profile);
        assert_eq!(budget(&over), plane * 11 + 2 * tile);
        assert_eq!(
            jpeg2000(&over, 1600).unwrap_err(),
            ImageErrorKind::DecodedTooLarge
        );
    }

    /// Without a colour profile, hayro-jpeg2000 0.4.1 converts CMYK to RGB
    /// after the decode (`integration.rs` 227-232) while the decode's context
    /// keeps its output planes: four planes, their 8-bit copy and moxcms's
    /// 3-byte result take 4 + 1 + 1 = 6 planes. It reads four components as
    /// CMYK under the CMYK (12) and YCCK (13) spaces, with no `colr` or one of
    /// another method, and under greyscale (17), which it repairs; under sRGB
    /// the fourth is alpha. Without a palette it repairs three components
    /// under CMYK to RGB.
    #[test]
    fn cmyk_without_a_profile_counts_its_copy_and_result() {
        let alpha = fixture("jpeg2000-alpha.jp2");
        let ihdr = whole_box(&alpha, b"ihdr");
        let srgb = whole_box(&alpha, b"colr");
        let enumerated =
            |space: u32| jp2_box(b"colr", &[&[1, 0, 0][..], &space.to_be_bytes()].concat());
        let budget = |bytes: &[u8]| jpeg2000::preflight(bytes).unwrap().decode_bytes(0);
        let plane = 64 * 48 * 4;
        let mut wrong = Vec::new();
        for (case, colour, planes, cmyk) in [
            ("sRGB", srgb.clone(), 4, false),
            ("CMYK", enumerated(12), 6, true),
            ("YCCK", enumerated(13), 6, true),
            ("greyscale", enumerated(17), 6, true),
            ("no colr", Vec::new(), 6, true),
            ("method 3", jp2_box(b"colr", &[3, 0, 0]), 6, true),
        ] {
            let bytes = jp2(&[&ihdr, &colour], &alpha);
            // hayro decodes the fourth component as K into opaque RGB, or as
            // the fixture's half-transparent alpha.
            let image = jpeg2000(&bytes, 1600)
                .unwrap_or_else(|kind| panic!("{case}: {kind:?}"))
                .image;
            let opaque = image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == 255);
            // The planes counted, then the tile-component.
            let found = budget(&bytes);
            if (found, opaque) != (plane * planes + plane, cmyk) {
                wrong.push((case, found / plane - 1, opaque));
            }
        }
        // Three components under CMYK, without a palette, are RGB to hayro:
        // the RGB fixture's quadrants in its three planes.
        let rgb = fixture("jpeg2000-rgb.jp2");
        let three = jp2(&[&whole_box(&rgb, b"ihdr"), &enumerated(12)], &rgb);
        assert_quadrants(
            &jpeg2000(&three, 1600).unwrap().image,
            QUADRANTS,
            2,
            "3 × CMYK",
        );
        if budget(&three) != plane * 3 + plane {
            wrong.push((
                "three components under CMYK",
                budget(&three) / plane - 1,
                true,
            ));
        }
        // With a palette the output planes are the `cmap` entries, and CMYK
        // (12) is read whatever the columns: one index and three mapped
        // planes, then 3 + 1 + 1 to convert. A `cdef` copies the three output
        // planes (2 × 3), and when its last channel by index is opacity,
        // which leaves alpha beside CMYK there, the split takes 2 × 3 + 1.
        let palette = fixture("jpeg2000-palette.jp2");
        let [ihdr, pclr, mapping] =
            [b"ihdr", b"pclr", b"cmap"].map(|kind| whole_box(&palette, kind));
        let cmyk = enumerated(12);
        let colours = cdef(&[[0, 0, 1], [1, 0, 2], [2, 0, 3]]);
        let opacity_first = cdef(&[[0, 1, 0], [1, 0, 1], [2, 0, 2]]);
        let opacity_last = cdef(&[[2, 1, 0], [0, 0, 1], [1, 0, 2]]);
        for (case, children, planes) in [
            ("palette", vec![&ihdr, &cmyk, &pclr, &mapping], 5),
            (
                "palette and colour cdef",
                vec![&ihdr, &cmyk, &pclr, &mapping, &colours],
                6,
            ),
            (
                "palette and cdef, opacity first",
                vec![&ihdr, &cmyk, &pclr, &mapping, &opacity_first],
                6,
            ),
            (
                "palette and cdef, opacity last by index",
                vec![&ihdr, &cmyk, &pclr, &mapping, &opacity_last],
                7,
            ),
        ] {
            let children: Vec<&[u8]> = children.into_iter().map(Vec::as_slice).collect();
            let found = budget(&jp2(&children, &palette));
            if found != plane * planes + plane {
                wrong.push((case, found / plane - 1, false));
            }
        }
        assert!(wrong.is_empty(), "{wrong:?}");

        // 4,096 pixels wide with the last component sampled 2 × 2, so no
        // level is skipped. At 3,000 rows four planes fit the budget and six
        // do not; at 2,700 six fit.
        let shape = |height: u32, colour: &[u8]| {
            let mut ihdr = whole_box(&alpha, b"ihdr");
            set(&mut ihdr, 8, &height.to_be_bytes());
            set(&mut ihdr, 12, &4_096u32.to_be_bytes());
            let mut bytes = jp2(&[&ihdr, colour], &alpha);
            let marker = position(&bytes, b"jp2c") + 4;
            set(&mut bytes, marker + 8, &4_096u32.to_be_bytes());
            set(&mut bytes, marker + 12, &height.to_be_bytes());
            set(&mut bytes, marker + 42 + 3 * 3 + 1, &[2, 2]);
            bytes
        };
        let tile = 64 * 48 * 4;
        let plane = |height: u64| 4_096 * height * 4;
        let accepted = jpeg2000::preflight(&shape(3_000, &srgb)).unwrap();
        assert!(!accepted.may_reduce());
        assert_eq!(accepted.decode_bytes(0), plane(3_000) * 4 + tile);
        assert!(accepted.decode_bytes(0) <= MAX_DECODER_BYTES);
        for (case, colour) in [("CMYK", enumerated(12)), ("no colr", Vec::new())] {
            let over = shape(3_000, &colour);
            assert_eq!(budget(&over), plane(3_000) * 6 + tile, "{case}");
            assert_eq!(
                jpeg2000(&over, 1600).unwrap_err(),
                ImageErrorKind::DecodedTooLarge,
                "{case}"
            );
            let under = budget(&shape(2_700, &colour));
            assert_eq!(under, plane(2_700) * 6 + tile, "{case}");
            assert!(under <= MAX_DECODER_BYTES, "{case}");
        }
        // Three components under CMYK at 4,096 × 4,000, the last sampled
        // 2 × 2: their three planes fit, where the five of a CMYK conversion
        // would not.
        let mut ihdr = whole_box(&rgb, b"ihdr");
        set(&mut ihdr, 8, &4_000u32.to_be_bytes());
        set(&mut ihdr, 12, &4_096u32.to_be_bytes());
        let mut three = jp2(&[&ihdr, &enumerated(12)], &rgb);
        let marker = position(&three, b"jp2c") + 4;
        set(&mut three, marker + 8, &4_096u32.to_be_bytes());
        set(&mut three, marker + 12, &4_000u32.to_be_bytes());
        set(&mut three, marker + 42 + 3 * 2 + 1, &[2, 2]);
        let three = jpeg2000::preflight(&three).unwrap();
        assert!(!three.may_reduce());
        assert_eq!(three.decode_bytes(0), plane(4_000) * 3 + tile);
        assert!(three.decode_bytes(0) <= MAX_DECODER_BYTES);
        assert!(plane(4_000) * 5 + tile > MAX_DECODER_BYTES);
    }

    /// hayro-jpeg2000 0.4.1 decodes the last `jp2c` of a file; the preflight
    /// checks the first and refuses any after it.
    #[test]
    fn a_jp2_codestream_after_the_first_is_refused() {
        use ImageErrorKind::{Jpeg2000TooManyTiles, Jpeg2000Unreadable};
        // A 300 × 300 grey grid as one tile, and as 90,000 1 × 1 tiles.
        let codestream = |tile: u32| {
            let mut bytes = issue_1549();
            for offset in [8, 12] {
                set(&mut bytes, offset, &300u32.to_be_bytes());
            }
            for offset in [24, 28] {
                set(&mut bytes, offset, &tile.to_be_bytes());
            }
            bytes
        };
        let (one_tile, many_tiles) = (codestream(300), codestream(1));
        assert_eq!(
            jpeg2000(&many_tiles, 100).unwrap_err(),
            Jpeg2000TooManyTiles
        );
        let rgb = fixture("jpeg2000-rgb.jp2");
        // HEIGHT, WIDTH, one component of 8 bits, compression type 7.
        let mut ihdr = [300u32.to_be_bytes(), 300u32.to_be_bytes()].concat();
        ihdr.extend_from_slice(&[0, 1, 7, 7, 0, 0]);
        // The signature and `ftyp` boxes, then a header and the one-tile grid.
        let mut one_tile_jp2 = rgb[..32].to_vec();
        one_tile_jp2.extend(jp2_box(b"jp2h", &jp2_box(b"ihdr", &ihdr)));
        one_tile_jp2.extend(jp2_box(b"jp2c", &one_tile));
        let reply = jpeg2000(&one_tile_jp2, 100).unwrap();
        assert_eq!(
            (reply.image.original_width, reply.image.original_height),
            (300, 300)
        );
        let mut appended = one_tile_jp2;
        appended.extend(jp2_box(b"jp2c", &many_tiles));
        assert_eq!(jpeg2000(&appended, 100).unwrap_err(), Jpeg2000Unreadable);
    }

    /// hayro-jpeg2000, not strict, stops reading top-level boxes at bytes
    /// that frame none, and so does the preflight: padding after the
    /// codestream decodes, and the same bytes before it leave no codestream.
    #[test]
    fn bytes_that_frame_no_box_end_the_walk() {
        let rgb = fixture("jpeg2000-rgb.jp2");
        // The codestream's explicit length ends the file.
        let jp2c = position(&rgb, b"jp2c") - 4;
        assert_eq!(whole_box(&rgb, b"jp2c").len(), rgb.len() - jp2c);
        // Fewer bytes than a box header, and a header longer than the file.
        let mut tails: Vec<Vec<u8>> = (1..8).map(|length| vec![0; length]).collect();
        tails.push(b"\xff\xff\xff\xf0uuid".to_vec());
        for tail in &tails {
            let padded = [&rgb[..], tail].concat();
            let reply =
                jpeg2000(&padded, 1600).unwrap_or_else(|kind| panic!("after {tail:?}: {kind:?}"));
            assert_quadrants(&reply.image, QUADRANTS, 2, &format!("after {tail:?}"));
            let before = [&rgb[..jp2c], tail, &rgb[jp2c..]].concat();
            assert_eq!(
                jpeg2000(&before, 1600).unwrap_err(),
                ImageErrorKind::Jpeg2000Unreadable,
                "before {tail:?}"
            );
        }
    }

    /// hayro-jpeg2000 0.4.1 decodes with the last `jp2h` of a file; the
    /// preflight accepts exactly one, before the codestream.
    #[test]
    fn a_second_jp2_header_is_refused_before_or_after_the_codestream() {
        // The palette fixture's header with a 32-bit column (#1649), which
        // alone ends in `Jpeg2000TooDeep`, after or before the RGB fixture's
        // codestream (whose explicit length ends the file).
        let rgb = fixture("jpeg2000-rgb.jp2");
        let palette = fixture("jpeg2000-palette.jp2");
        let mut deep_header = whole_box(&palette, b"jp2h");
        let pclr = position(&deep_header, b"pclr") + 4;
        deep_header[pclr + 3] = 31;
        let mut appended_header = rgb.clone();
        appended_header.extend_from_slice(&deep_header);
        let jp2c = position(&rgb, b"jp2c") - 4;
        assert_eq!(whole_box(&rgb, b"jp2c").len(), rgb.len() - jp2c);
        let second_header = [&rgb[..jp2c], &deep_header[..], &rgb[jp2c..]].concat();
        for (case, bytes) in [
            ("after the jp2c", appended_header),
            ("before the jp2c", second_header),
        ] {
            assert_eq!(
                jpeg2000(&bytes, 100).unwrap_err(),
                ImageErrorKind::Jpeg2000Unreadable,
                "{case}"
            );
        }
    }

    /// A real `hayro-jpeg2000` 0.4.1 panic past every preflight bound,
    /// requirement 12's (kept for A3): a one-column 7-bit palette (no `colr`,
    /// so grey, one output byte per pixel) whose `cmap` maps column 0 twice.
    /// `interleave_and_convert` takes its slow path for the 7-bit planes and
    /// writes both into the grey buffer until an `unwrap()` meets its end
    /// (`lib.rs:482`); `serve` turns the panic into `DecoderFailed`. With the
    /// fixture's 8-bit columns and sRGB `colr` the same mapping decodes.
    #[test]
    fn a_hayro_panic_past_the_preflight_ends_as_decoder_failed() {
        let palette = fixture("jpeg2000-palette.jp2");
        // Four entries in one 7-bit column.
        let one_column = jp2_box(b"pclr", &[0, 4, 1, 6, 0, 40, 80, 127]);
        let bytes = jp2(
            &[
                &whole_box(&palette, b"ihdr"),
                &one_column,
                &cmap(&[entry(0, 1, 0); 2]),
            ],
            &palette,
        );
        // 64 × 48 f32 samples in one component and two mapped planes, and
        // the one 64 × 48 tile-component: well inside the budget.
        assert_eq!(
            jpeg2000::preflight(&bytes).unwrap().decode_bytes(0),
            64 * 48 * 4 * (1 + 2) + 64 * 48 * 4
        );
        assert_eq!(
            jpeg2000(&bytes, 1600).unwrap_err(),
            ImageErrorKind::DecoderFailed
        );
    }

    #[test]
    fn hostile_jpeg2000_ends_in_its_error_kind_before_decoding() {
        use ImageErrorKind::*;
        let issue_1549 = issue_1549();
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
