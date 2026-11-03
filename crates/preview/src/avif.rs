//! AVIF images for the confined Linux helper: `mp4parse` reads the container,
//! the system libdav1d (through dav1d-rs) decodes the AV1 items, and this
//! module converts, orients and scales the result. Only
//! [`crate::image_helper`] calls this.
//!
//! Reviewed limits (security review requirements 5, 6 and 10): one decoder per
//! item on fresh settings with one thread, a frame delay of one, a frame size
//! limit of the declared `ispe` area, operating point 0 and film grain applied;
//! a bounded send/get loop; only dav1d's default picture allocator; the picture
//! size equal to `ispe` before any plane is read; and only the luma plane of a
//! monochrome picture or an alpha item. dav1d-sys 0.8.3 writes some C layouts
//! by hand (#126): the picture accessors used here read fields whose offsets
//! `libdav1d_layouts_match_the_binding` proves, and nothing parses a sequence
//! header outside libdav1d.
#![forbid(unsafe_code)]

use std::time::Instant;

use dav1d::pixel::YUVRange;
use dav1d::{Decoder, Picture, PixelLayout, PlanarImageComponent, Settings};
use image::metadata::{
    Cicp, CicpColorPrimaries, CicpMatrixCoefficients, CicpTransferCharacteristics,
    CicpVideoFullRangeFlag,
};
use image::{ConvertColorOptions, DynamicImage, RgbaImage};
use moxcms::{ColorProfile, DataColorSpace, Layout, ParsingOptions, TransformOptions};
use mp4parse::{Feature, ParseStrictness};

use crate::image_helper::{self, Decoded, ImageErrorKind, ImageFlags, ImageTimings};

/// The oldest libdav1d the helper uses: Ubuntu 24.04's, past CVE-2024-1580.
/// A policy floor on the reported version, not a patch-level check.
const MINIMUM_LIBDAV1D: (u32, u32, u32) = (1, 4, 1);
/// Send and get steps for one item before it counts as unreadable.
const MAX_DECODE_STEPS: usize = 16;
/// dav1d refuses a frame over `frame_size_limit` with `DAV1D_ERR(ERANGE)`;
/// ERANGE is 34 on Linux for both helper architectures.
const ERANGE: i32 = 34;
/// The largest ICC profile converted. RGB display and working-space profiles
/// take kilobytes and LUT-based ones hundreds; a larger `colr` is flagged
/// without being parsed (moxcms's own default allows 10 MB).
const MAX_ICC_PROFILE_BYTES: usize = 4 << 20;
/// Pixels converted from a profile per step, so the conversion needs no second
/// full-size buffer.
const PROFILE_STEP_PIXELS: usize = 1 << 14;
const ALPHA_URNS: [&[u8]; 2] = [
    b"urn:mpeg:mpegB:cicp:systems:auxiliary:alpha",
    b"urn:mpeg:hevc:2015:auxid:1",
];

pub(crate) fn decode(
    bytes: &[u8],
    edge: u32,
    timings: &mut ImageTimings,
) -> Result<Decoded, ImageErrorKind> {
    use ImageErrorKind::{AvifGrid, AvifNoStill, AvifSizeMismatch, AvifUnreadable, Dav1dTooOld};
    let start = Instant::now();
    // The floor is checked before mp4parse sees a byte.
    if !crate::libdav1d_version().is_some_and(|version| supported_libdav1d(&version)) {
        return Err(Dav1dTooOld);
    }
    let context = mp4parse::read_avif(&mut &bytes[..], ParseStrictness::Normal)
        .map_err(|_| AvifUnreadable)?;
    let features = &context.unsupported_features;
    if features.contains(Feature::Grid) {
        return Err(AvifGrid);
    }
    if features.contains(Feature::Ipro) {
        return Err(AvifUnreadable);
    }
    let mut flags = ImageFlags {
        animation: context.sequence.is_some(),
        crop_not_applied: features.contains(Feature::Clap),
        ..ImageFlags::default()
    };
    let Some(primary) = context.primary_item_coded_data() else {
        return Err(if flags.animation {
            AvifNoStill
        } else {
            AvifUnreadable
        });
    };
    // mp4parse exposes `ispe`, `nclx` and `imir` only as raw pointers, so the
    // properties GitTurtle uses are read here from the same bytes.
    let properties = Properties::parse(bytes).ok_or(AvifUnreadable)?;
    let size = properties
        .spatial_extents(properties.primary)
        .ok_or(AvifUnreadable)?;
    image_helper::check_size(size.0, size.1)?;
    image_helper::check_rgba_budget(size.0, size.1)?;
    let alpha = context.alpha_item_coded_data();
    if alpha.is_some() {
        let items = properties.alpha_items();
        if items.is_empty() {
            return Err(AvifUnreadable);
        }
        for item in items {
            if properties.spatial_extents(item) != Some(size) {
                return Err(AvifSizeMismatch);
            }
        }
    }
    timings.parse_us = image_helper::elapsed_us(start);

    let start = Instant::now();
    let picture = decode_item(primary, size)?;
    let alpha_picture = alpha.map(|alpha| decode_item(alpha, size)).transpose()?;
    timings.decode_us = image_helper::elapsed_us(start);

    let start = Instant::now();
    // The colour signal comes from `nclx`, else from the sequence header, and
    // is read before the pictures go. `convert` and `apply_alpha` consume the
    // pictures, so dav1d's planes are freed as soon as they are read and only
    // the RGBA image reaches colour conversion, orientation and scaling
    // (security review requirement 18).
    let colour = properties
        .colour(properties.primary)
        .unwrap_or_else(|| Nclx::signalled(&picture));
    let mut rgba = convert(picture, size, colour)?;
    if let Some(alpha) = alpha_picture {
        apply_alpha(&mut rgba, alpha)?;
        if context.premultiplied_alpha {
            crate::unpremultiply(rgba.as_mut());
        }
    }
    // An ICC profile describes the colour in place of the `nclx` or sequence
    // header primaries and transfer, as libavif reads it. One that cannot be
    // applied is disclosed, and those signals convert as without it.
    let profile = match context.icc_colour_information() {
        Some(Ok(icc)) => Some(apply_profile(&mut rgba, icc)),
        _ => None,
    };
    flags.profile_not_applied = profile == Some(false);
    if profile != Some(true) {
        flags.hdr_not_tone_mapped = matches!(colour.transfer, 16 | 18);
        if !flags.hdr_not_tone_mapped {
            rgba = to_srgb(rgba, colour.primaries, colour.transfer);
        }
    }
    let rgba = orient(rgba, properties.rotation(), properties.mirror());
    let original = rgba.dimensions();
    let decoded = image_helper::finish(rgba, original, edge, flags);
    timings.convert_us = image_helper::elapsed_us(start);
    Ok(decoded)
}

/// Whether libdav1d's reported version is at least the floor. Takes the
/// leading decimal `major.minor.patch` (`1.4.1-0-g…` is 1.4.1) and refuses
/// anything that does not parse.
fn supported_libdav1d(version: &str) -> bool {
    let mut parts = version.splitn(3, '.');
    let number = |part: Option<&str>, whole: bool| {
        let part = part?;
        let digits = part.len() - part.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits == 0 || digits > 9 || (whole && digits != part.len()) {
            return None;
        }
        part[..digits].parse::<u32>().ok()
    };
    let major = number(parts.next(), true);
    let minor = number(parts.next(), true);
    let patch = number(parts.next(), false);
    match (major, minor, patch) {
        (Some(major), Some(minor), Some(patch)) => (major, minor, patch) >= MINIMUM_LIBDAV1D,
        _ => false,
    }
}

/// One AV1 item on its own decoder: the reviewed settings, the data sent once,
/// then `send_pending_data` after `Again`, within a fixed number of steps. The
/// picture must have the declared size before anything reads it.
fn decode_item(data: &[u8], (width, height): (u32, u32)) -> Result<Picture, ImageErrorKind> {
    let mut settings = Settings::new();
    settings.set_n_threads(1);
    settings.set_max_frame_delay(1);
    // At most 32,000,000 by `check_size`.
    settings.set_frame_size_limit(width * height);
    settings.set_all_layers(false);
    settings.set_operating_point(0);
    settings.set_apply_grain(true);
    settings.set_output_invisible_frames(false);
    let mut decoder =
        Decoder::with_settings(&settings).map_err(|_| ImageErrorKind::DecoderFailed)?;
    let mut sent = decoder.send_data(data.to_vec(), None, None, None);
    for _ in 0..MAX_DECODE_STEPS {
        match sent {
            Ok(()) | Err(dav1d::Error::Again) => {}
            Err(error) => return Err(failure(error)),
        }
        match decoder.get_picture() {
            Ok(picture) if (picture.width(), picture.height()) == (width, height) => {
                return Ok(picture);
            }
            Ok(_) => return Err(ImageErrorKind::AvifSizeMismatch),
            // Everything was sent and no picture came out.
            Err(dav1d::Error::Again) if sent.is_ok() => break,
            Err(dav1d::Error::Again) => {}
            Err(error) => return Err(failure(error)),
        }
        sent = decoder.send_pending_data();
    }
    Err(ImageErrorKind::AvifUnreadable)
}

fn failure(error: dav1d::Error) -> ImageErrorKind {
    match error {
        // A frame header larger than `ispe`, refused before its allocation.
        dav1d::Error::UnknownError(code) if code == -ERANGE => ImageErrorKind::AvifSizeMismatch,
        dav1d::Error::NotEnoughMemory => ImageErrorKind::DecoderFailed,
        _ => ImageErrorKind::AvifUnreadable,
    }
}

/// The planes a picture's conversion reads: luma alone for a monochrome
/// picture and for every alpha item, whatever its layout (dav1d-rs #123).
fn components(layout: PixelLayout, alpha: bool) -> &'static [PlanarImageComponent] {
    use PlanarImageComponent::{U, V, Y};
    match layout {
        _ if alpha => &[Y],
        PixelLayout::I400 => &[Y],
        PixelLayout::I420 | PixelLayout::I422 | PixelLayout::I444 => &[Y, U, V],
    }
}

/// A plane checked against its stride and length, read as 8-bit or
/// little-endian 16-bit samples.
struct Samples {
    data: dav1d::Plane,
    stride: usize,
    wide: bool,
}

impl Samples {
    fn get(&self, x: usize, y: usize) -> u32 {
        let at = y * self.stride + if self.wide { x * 2 } else { x };
        if self.wide {
            u32::from(u16::from_le_bytes([self.data[at], self.data[at + 1]]))
        } else {
            u32::from(self.data[at])
        }
    }
}

/// The layout, depth and planes of `picture`, each plane long enough for
/// `stride × (rows − 1) + width × bytes per sample`.
fn planes(
    picture: &Picture,
    alpha: bool,
) -> Result<(PixelLayout, u32, Vec<Samples>), ImageErrorKind> {
    let layout = picture.pixel_layout();
    let bits = picture.bit_depth();
    if !matches!(bits, 8 | 10 | 12)
        || picture.bits_per_component() != Some(dav1d::BitsPerComponent(bits))
    {
        return Err(ImageErrorKind::AvifUnreadable);
    }
    let wide = bits > 8;
    let (width, height) = (picture.width() as usize, picture.height() as usize);
    let mut samples = Vec::new();
    for &component in components(layout, alpha) {
        let (columns, rows) = match (component, layout) {
            (PlanarImageComponent::Y, _) | (_, PixelLayout::I444) => (width, height),
            (_, PixelLayout::I422) => (width.div_ceil(2), height),
            _ => (width.div_ceil(2), height.div_ceil(2)),
        };
        let row_bytes = columns * if wide { 2 } else { 1 };
        let stride = picture.stride(component) as usize;
        let data = picture.plane(component);
        let needed = stride
            .checked_mul(rows - 1)
            .and_then(|bytes| bytes.checked_add(row_bytes));
        if stride < row_bytes || needed.is_none_or(|needed| data.len() < needed) {
            return Err(ImageErrorKind::AvifUnreadable);
        }
        samples.push(Samples { data, stride, wide });
    }
    Ok((layout, bits as u32, samples))
}

/// `colr` `nclx` values (ISO/IEC 23091-2 code points).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Nclx {
    primaries: u8,
    transfer: u8,
    matrix: u8,
    full_range: bool,
}

impl Nclx {
    /// The values the AV1 sequence header signals, for an item without `nclx`.
    fn signalled(picture: &Picture) -> Self {
        Self {
            primaries: picture.color_primaries() as u8,
            transfer: picture.transfer_characteristic() as u8,
            matrix: picture.matrix_coefficients() as u8,
            full_range: picture.color_range() == YUVRange::Full,
        }
    }
}

/// Luma and chroma weights, or the two matrices without them.
#[derive(Debug, Clone, Copy)]
enum Matrix {
    Weights(f32, f32),
    Identity,
    YCgCo,
}

fn matrix(code: u8) -> Option<Matrix> {
    Some(match code {
        0 => Matrix::Identity,
        1 => Matrix::Weights(0.2126, 0.0722),
        // Unspecified means BT.601, as libavif and ImageIO read it.
        2 | 5 | 6 => Matrix::Weights(0.299, 0.114),
        4 => Matrix::Weights(0.30, 0.11),
        7 => Matrix::Weights(0.212, 0.087),
        8 => Matrix::YCgCo,
        9 => Matrix::Weights(0.2627, 0.0593),
        _ => return None,
    })
}

/// Straight RGBA8 from a colour or monochrome picture of `size`. Chroma is
/// taken from the nearest sample; the output is downscaled anyway. Takes the
/// picture so its planes are freed on return.
fn convert(picture: Picture, size: (u32, u32), nclx: Nclx) -> Result<RgbaImage, ImageErrorKind> {
    let (layout, bits, planes) = planes(&picture, false)?;
    let matrix = matrix(nclx.matrix).ok_or(ImageErrorKind::AvifUnreadable)?;
    if matches!(matrix, Matrix::Identity) && layout != PixelLayout::I444 && planes.len() == 3 {
        return Err(ImageErrorKind::AvifUnreadable);
    }
    let range = Range::new(bits, nclx.full_range);
    let (shift_x, shift_y) = match layout {
        PixelLayout::I420 => (1, 1),
        PixelLayout::I422 => (1, 0),
        PixelLayout::I400 | PixelLayout::I444 => (0, 0),
    };
    let mut rgba = RgbaImage::new(size.0, size.1);
    for (x, y, pixel) in rgba.enumerate_pixels_mut() {
        let (x, y) = (x as usize, y as usize);
        let luma = range.luma(planes[0].get(x, y));
        let [red, green, blue] = match planes.as_slice() {
            [_, u, v] => {
                let (cx, cy) = (x >> shift_x, y >> shift_y);
                let (u, v) = (u.get(cx, cy), v.get(cx, cy));
                match matrix {
                    Matrix::Identity => [range.luma(v), luma, range.luma(u)],
                    Matrix::YCgCo => {
                        let (green, orange) = (range.chroma(u), range.chroma(v));
                        let base = luma - green;
                        [base + orange, luma + green, base - orange]
                    }
                    Matrix::Weights(kr, kb) => {
                        let (cb, cr) = (range.chroma(u), range.chroma(v));
                        let red = luma + 2.0 * (1.0 - kr) * cr;
                        let blue = luma + 2.0 * (1.0 - kb) * cb;
                        let green = (luma - kr * red - kb * blue) / (1.0 - kr - kb);
                        [red, green, blue]
                    }
                }
            }
            _ => [luma; 3],
        };
        pixel.0 = [unit(red), unit(green), unit(blue), 255];
    }
    Ok(rgba)
}

/// Sample normalisation for one depth and range.
struct Range {
    luma_offset: f32,
    luma_scale: f32,
    chroma_offset: f32,
    chroma_scale: f32,
}

impl Range {
    fn new(bits: u32, full: bool) -> Self {
        let max = ((1u32 << bits) - 1) as f32;
        let step = (1u32 << (bits - 8)) as f32;
        if full {
            Self {
                luma_offset: 0.0,
                luma_scale: max,
                chroma_offset: (1u32 << (bits - 1)) as f32,
                chroma_scale: max,
            }
        } else {
            Self {
                luma_offset: 16.0 * step,
                luma_scale: 219.0 * step,
                chroma_offset: 128.0 * step,
                chroma_scale: 224.0 * step,
            }
        }
    }

    fn luma(&self, sample: u32) -> f32 {
        (sample as f32 - self.luma_offset) / self.luma_scale
    }

    fn chroma(&self, sample: u32) -> f32 {
        (sample as f32 - self.chroma_offset) / self.chroma_scale
    }
}

fn unit(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The alpha item's luma, in its own range, into the alpha channel. The
/// picture has the colour picture's size (`decode_item`) and is freed on
/// return.
fn apply_alpha(rgba: &mut RgbaImage, picture: Picture) -> Result<(), ImageErrorKind> {
    let (_, bits, planes) = planes(&picture, true)?;
    let max = (1u32 << bits) - 1;
    let full = picture.color_range() == YUVRange::Full;
    let range = Range::new(bits, full);
    for (x, y, pixel) in rgba.enumerate_pixels_mut() {
        let sample = planes[0].get(x as usize, y as usize);
        pixel.0[3] = if full {
            ((sample * 255 + max / 2) / max) as u8
        } else {
            unit(range.luma(sample))
        };
    }
    Ok(())
}

/// Converts colour in `primaries` and `transfer` to sRGB through `image`'s
/// colour management (moxcms). sRGB, BT.709 and unspecified primaries are left
/// as they are, as is anything `image` cannot convert.
fn to_srgb(rgba: RgbaImage, primaries: u8, transfer: u8) -> RgbaImage {
    let primaries = match primaries {
        9 => CicpColorPrimaries::Rgb2020,
        11 => CicpColorPrimaries::SmpteRp431,
        12 => CicpColorPrimaries::SmpteRp432,
        _ => return rgba,
    };
    let transfer = match transfer {
        1 | 6 | 14 | 15 => CicpTransferCharacteristics::Bt709,
        8 => CicpTransferCharacteristics::Linear,
        _ => CicpTransferCharacteristics::SRgb,
    };
    let source = Cicp {
        primaries,
        transfer,
        matrix: CicpMatrixCoefficients::Identity,
        full_range: CicpVideoFullRangeFlag::FullRange,
    };
    let mut image = DynamicImage::ImageRgba8(rgba);
    let converted = image.set_color_space(source).is_ok()
        && image
            .apply_color_space(Cicp::SRGB, ConvertColorOptions::default())
            .is_ok();
    match image {
        DynamicImage::ImageRgba8(rgba) if converted => rgba,
        image => image.into_rgba8(),
    }
}

/// Converts straight RGBA from the item's ICC profile to sRGB in place with
/// moxcms, the conversion hayro-jpeg2000 gives a JP2 profile: an RGB profile,
/// RGBA8 in and out, default options. False, with `rgba` unchanged, for a
/// profile over [`MAX_ICC_PROFILE_BYTES`] (never parsed), one moxcms refuses or
/// panics on while parsing it or building the transform, or one that does not
/// describe RGB: the image then draws without it and is flagged. Running the
/// transform is not caught here. A panic there fails the whole image as
/// `DecoderFailed` through `image_helper::serve`'s `catch_unwind`, and the
/// helper serves one request per process, so no half-converted pixels leave
/// it: catching each step would leave the image half converted, and
/// converting a copy would double the RGBA's memory (security review
/// requirement 18). Converting otherwise fails only on lane sizes, which every
/// step meets, so the first step's result stands for all of them.
fn apply_profile(rgba: &mut RgbaImage, icc: &[u8]) -> bool {
    if icc.len() > MAX_ICC_PROFILE_BYTES {
        return false;
    }
    let transform = std::panic::catch_unwind(|| {
        let options = ParsingOptions {
            // moxcms refuses a profile of this many bytes or more.
            max_profile_size: MAX_ICC_PROFILE_BYTES + 1,
            ..ParsingOptions::default()
        };
        let source = ColorProfile::new_from_slice_with_options(icc, options).ok()?;
        if source.color_space != DataColorSpace::Rgb {
            return None;
        }
        source
            .create_transform_8bit(
                Layout::Rgba,
                &ColorProfile::new_srgb(),
                Layout::Rgba,
                TransformOptions::default(),
            )
            .ok()
    });
    let Ok(Some(transform)) = transform else {
        return false;
    };
    let mut converted = vec![0; PROFILE_STEP_PIXELS * 4];
    for pixels in rgba.chunks_mut(PROFILE_STEP_PIXELS * 4) {
        let converted = &mut converted[..pixels.len()];
        if transform.transform(pixels, converted).is_err() {
            return false;
        }
        pixels.copy_from_slice(converted);
    }
    true
}

/// MIAF order: `clap` (not applied), then `irot` anticlockwise quarter turns,
/// then `imir` (0 exchanges top and bottom, 1 left and right). Only a quarter
/// turn needs a second buffer, and the first goes as soon as it is copied.
fn orient(mut rgba: RgbaImage, rotation: u8, mirror: Option<u8>) -> RgbaImage {
    use image::imageops::{
        flip_horizontal_in_place, flip_vertical_in_place, rotate90, rotate180_in_place, rotate270,
    };
    match rotation {
        1 => rgba = rotate270(&rgba),
        2 => rotate180_in_place(&mut rgba),
        3 => rgba = rotate90(&rgba),
        _ => {}
    }
    match mirror {
        Some(0) => flip_vertical_in_place(&mut rgba),
        Some(_) => flip_horizontal_in_place(&mut rgba),
        None => {}
    }
    rgba
}

/// The item properties GitTurtle reads itself (ISO/IEC 23008-12 9.3): the
/// primary item, the items with an `auxl` reference to it, and the property
/// associations of those items only. Every length is checked against the
/// input; `None` means unreadable.
struct Properties<'a> {
    primary: u32,
    container: Vec<([u8; 4], &'a [u8])>,
    /// Item and 1-based `ipco` index, for the primary and `auxiliary` items.
    associations: Vec<(u32, u16)>,
    /// Items with an `auxl` reference to the primary.
    auxiliary: Vec<u32>,
}

/// The most `ipma` entries read, summed over every `ipma` box. Each entry
/// names one item, and an image GitTurtle decodes has a handful (the primary,
/// its alpha, metadata; libheif's default limit is 1,000 items). A grid's
/// tiles, which can number in the hundreds, never reach [`Properties::parse`]:
/// the grid is refused first.
const MAX_IPMA_ENTRIES: u32 = 4_096;
/// The most items with an `auxl` reference to the primary: an alpha item, a
/// depth map and the like.
const MAX_AUXILIARY_ITEMS: usize = 16;
/// The most property associations kept, for the primary and its `auxl` items
/// together. An item has a few (`av1C`, `ispe`, `pixi`, `colr`, `irot`,
/// `imir`, `auxC`); the format allows 255 per entry.
const MAX_KEPT_ASSOCIATIONS: usize = 1_024;

impl<'a> Properties<'a> {
    /// Reads `pitm`, then the items with an `auxl` reference to the primary
    /// (refused above [`MAX_AUXILIARY_ITEMS`]), then `ipma`. Each `ipma` box's
    /// entry count joins the total and is refused above [`MAX_IPMA_ENTRIES`]
    /// before any of its entries is read; an entry for any other item is
    /// skipped without being stored; and an entry whose associations would
    /// take the kept total above [`MAX_KEPT_ASSOCIATIONS`] is refused before
    /// they are stored.
    ///
    /// These bounds protect this parser's own lists. `mp4parse` has already
    /// stored every `ipma` entry by then, in fallible `TryVec`s (mp4parse
    /// 0.17.0 `src/lib.rs:3663-3698`), and the confined helper's resource
    /// limits bound that memory.
    fn parse(bytes: &'a [u8]) -> Option<Self> {
        let (_, _, meta) = full_box(find(&boxes(bytes)?, b"meta")?)?;
        let meta = boxes(meta)?;
        let (version, _, pitm) = full_box(find(&meta, b"pitm")?)?;
        let primary = Reader(pitm).id(version)?;
        let mut auxiliary = Vec::new();
        if let Some(iref) = find(&meta, b"iref") {
            let (version, _, references) = full_box(iref)?;
            for (kind, payload) in boxes(references)? {
                let mut reader = Reader(payload);
                let from = reader.id(version)?;
                for _ in 0..reader.u16()? {
                    let to = reader.id(version)?;
                    let kept = &kind == b"auxl" && to == primary && from != primary;
                    if kept && !auxiliary.contains(&from) {
                        if auxiliary.len() == MAX_AUXILIARY_ITEMS {
                            return None;
                        }
                        auxiliary.push(from);
                    }
                }
            }
        }
        let iprp = boxes(find(&meta, b"iprp")?)?;
        let container = boxes(find(&iprp, b"ipco")?)?;
        let mut entries = 0u32;
        let mut associations = Vec::new();
        for (_, ipma) in iprp.iter().filter(|(kind, _)| kind == b"ipma") {
            let (version, flags, payload) = full_box(ipma)?;
            let mut reader = Reader(payload);
            let count = reader.u32()?;
            entries = entries
                .checked_add(count)
                .filter(|entries| *entries <= MAX_IPMA_ENTRIES)?;
            let width = if flags & 1 == 1 { 2 } else { 1 };
            for _ in 0..count {
                let item = reader.id(version)?;
                let count = usize::from(reader.u8()?);
                let indices = reader.take(count * width)?;
                if item != primary && !auxiliary.contains(&item) {
                    continue;
                }
                if associations.len() + count > MAX_KEPT_ASSOCIATIONS {
                    return None;
                }
                associations.extend(indices.chunks(width).map(|index| match *index {
                    [high, low] => (item, u16::from_be_bytes([high, low]) & 0x7fff),
                    _ => (item, u16::from(index[0] & 0x7f)),
                }));
            }
        }
        Some(Self {
            primary,
            container,
            associations,
            auxiliary,
        })
    }

    fn properties(&self, item: u32) -> impl Iterator<Item = ([u8; 4], &'a [u8])> + '_ {
        self.associations
            .iter()
            .filter(move |(id, _)| *id == item)
            .filter_map(|(_, index)| {
                let index = usize::from(*index).checked_sub(1)?;
                self.container.get(index).copied()
            })
    }

    fn property(&self, item: u32, kind: &[u8; 4]) -> Option<&'a [u8]> {
        self.properties(item)
            .find(|(found, _)| found == kind)
            .map(|(_, payload)| payload)
    }

    fn spatial_extents(&self, item: u32) -> Option<(u32, u32)> {
        let (_, _, payload) = full_box(self.property(item, b"ispe")?)?;
        let mut reader = Reader(payload);
        Some((reader.u32()?, reader.u32()?))
    }

    fn colour(&self, item: u32) -> Option<Nclx> {
        self.properties(item)
            .filter(|(kind, _)| kind == b"colr")
            .find_map(|(_, payload)| {
                let mut reader = Reader(payload.strip_prefix(b"nclx")?);
                Some(Nclx {
                    primaries: u8::try_from(reader.u16()?).ok()?,
                    transfer: u8::try_from(reader.u16()?).ok()?,
                    matrix: u8::try_from(reader.u16()?).ok()?,
                    full_range: reader.u8()? & 0x80 != 0,
                })
            })
    }

    fn rotation(&self) -> u8 {
        self.property(self.primary, b"irot")
            .and_then(|payload| payload.first())
            .map_or(0, |angle| angle & 3)
    }

    fn mirror(&self) -> Option<u8> {
        Some(self.property(self.primary, b"imir")?.first()? & 1)
    }

    /// Items with an `auxl` reference to the primary and an alpha `auxC`.
    fn alpha_items(&self) -> Vec<u32> {
        self.auxiliary
            .iter()
            .copied()
            .filter(|item| {
                self.property(*item, b"auxC")
                    .and_then(full_box)
                    .is_some_and(|(_, _, urn)| {
                        let urn = urn.split(|byte| *byte == 0).next().unwrap_or(urn);
                        ALPHA_URNS.contains(&urn)
                    })
            })
            .collect()
    }
}

/// ISOBMFF boxes in `data`, each within it.
fn boxes(mut data: &[u8]) -> Option<Vec<([u8; 4], &[u8])>> {
    let mut found = Vec::new();
    while !data.is_empty() {
        let mut reader = Reader(data);
        let size = reader.u32()?;
        let kind: [u8; 4] = reader.take(4)?.try_into().ok()?;
        let (size, header) = match size {
            0 => (data.len() as u64, 8),
            1 => (
                u64::from(reader.u32()?) << 32 | u64::from(reader.u32()?),
                16,
            ),
            size => (u64::from(size), 8),
        };
        let size = usize::try_from(size).ok()?;
        if size < header || size > data.len() {
            return None;
        }
        found.push((kind, &data[header..size]));
        data = &data[size..];
    }
    Some(found)
}

fn find<'a>(boxes: &[([u8; 4], &'a [u8])], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes
        .iter()
        .find(|(found, _)| found == kind)
        .map(|(_, payload)| *payload)
}

/// A full box's version, flags and payload.
fn full_box(payload: &[u8]) -> Option<(u8, u32, &[u8])> {
    let mut reader = Reader(payload);
    let header = reader.u32()?;
    Some(((header >> 24) as u8, header & 0x00ff_ffff, reader.0))
}

/// Big-endian reads that fail rather than run past the end.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let (taken, rest) = self.0.split_at_checked(count)?;
        self.0 = rest;
        Some(taken)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }

    /// An item ID: 16 bits in version 0 boxes, 32 bits after.
    fn id(&mut self, version: u8) -> Option<u32> {
        if version == 0 {
            self.u16().map(u32::from)
        } else {
            self.u32()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::mem::{offset_of, size_of};
    use std::path::Path;
    use std::process::Command;

    use super::*;
    use crate::image_helper::tests::{assert_near, decoded, pixel};
    use crate::pdf_helper::ConfinedFormat;

    const RED: [u8; 4] = [220, 30, 30, 255];
    const GREEN: [u8; 4] = [30, 200, 60, 255];
    const BLUE: [u8; 4] = [30, 50, 220, 255];
    const WHITE: [u8; 4] = [240, 240, 240, 255];

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(name),
        )
        .unwrap()
    }

    fn avif(name: &str, edge: u32) -> Result<image_helper::ImageReply, ImageErrorKind> {
        decoded(ConfinedFormat::Avif, edge, &fixture(name))
    }

    /// The quadrant centres of a 64 × 48 image.
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
    fn decodes_avif_depths_layouts_and_alpha() {
        let half = |colour: [u8; 4]| [colour[0], colour[1], colour[2], 128];
        for (name, tolerance) in [
            ("avif-yuv420-8bit-alpha.avif", 12),
            ("avif-yuv444-10bit-premultiplied.avif", 12),
            ("avif-yuv420-12bit-alpha.avif", 12),
        ] {
            let reply = avif(name, 1600).unwrap_or_else(|kind| panic!("{name}: {kind:?}"));
            assert_eq!((reply.image.width, reply.image.height), (64, 48), "{name}");
            assert_eq!(reply.flags, ImageFlags::default(), "{name}");
            assert_eq!(reply.image.format, "AVIF · first image");
            // Opaque left half, half-transparent right half, straight colour.
            assert_quadrants(
                &reply.image,
                [RED, half(GREEN), BLUE, half(WHITE)],
                tolerance,
                name,
            );
        }
        // Scaled to the edge with the aspect kept.
        let reply = avif("avif-yuv420-8bit-alpha.avif", 32).unwrap();
        assert_eq!((reply.image.width, reply.image.height), (32, 24));
        assert_eq!(
            (reply.image.original_width, reply.image.original_height),
            (64, 48)
        );
    }

    #[test]
    fn decodes_monochrome_avif_from_luma_alone() {
        for bits in [8, 10, 12] {
            let name = format!("avif-yuv400-{bits}bit.avif");
            let reply = avif(&name, 1600).unwrap_or_else(|kind| panic!("{name}: {kind:?}"));
            assert_eq!((reply.image.width, reply.image.height), (64, 48));
            for (x, grey) in [(0, 0), (32, 130), (63, 255)] {
                let at = pixel(&reply.image, x, 20);
                assert_near(at, [grey, grey, grey, 255], 8, &format!("{name} x {x}"));
            }
        }
        // The converter asks for luma alone wherever U and V may be unset.
        use PlanarImageComponent::{U, V, Y};
        assert_eq!(components(PixelLayout::I400, false), [Y]);
        for layout in [
            PixelLayout::I400,
            PixelLayout::I420,
            PixelLayout::I422,
            PixelLayout::I444,
        ] {
            assert_eq!(components(layout, true), [Y], "{layout:?} alpha");
        }
        assert_eq!(components(PixelLayout::I420, false), [Y, U, V]);
    }

    #[test]
    fn applies_irot_then_imir_and_reports_the_oriented_size() {
        let reply = avif("avif-irot-imir.avif", 1600).unwrap();
        assert_eq!((reply.image.width, reply.image.height), (48, 64));
        assert_eq!(
            (reply.image.original_width, reply.image.original_height),
            (48, 64)
        );
        // Turned a quarter anticlockwise, then left and right exchanged.
        assert_quadrants(&reply.image, [WHITE, GREEN, BLUE, RED], 12, "oriented");
    }

    #[test]
    fn applies_film_grain_converts_display_p3_and_shows_an_animation_still() {
        let grain = avif("avif-film-grain.avif", 1600).unwrap();
        assert_quadrants(&grain.image, [RED, GREEN, BLUE, WHITE], 40, "grain");
        // Display P3 through `nclx` 12/13/6, converted to sRGB (the script
        // prints the expected values).
        let p3 = avif("avif-display-p3.avif", 1600).unwrap();
        assert_quadrants(
            &p3.image,
            [
                [216, 69, 50, 255],
                [0, 183, 78, 255],
                [64, 91, 207, 255],
                [128, 128, 128, 255],
            ],
            12,
            "display p3",
        );
        let animation = avif("avif-animation.avif", 1600).unwrap();
        assert!(animation.flags.animation);
        assert_eq!(animation.image.format, "AVIF · first frame of an animation");
        assert_quadrants(
            &animation.image,
            [RED, GREEN, BLUE, WHITE],
            12,
            "first frame",
        );
    }

    /// The Display P3 quadrants stored in both P3 fixtures.
    const P3_STORED: [[u8; 4]; 4] = [
        [200, 80, 60, 255],
        [60, 180, 90, 255],
        [70, 90, 200, 255],
        [128, 128, 128, 255],
    ];
    /// Their sRGB values under the fixtures' 2.2-gamma Display P3 profile (the
    /// script prints them; the JP2 fixture converts to the same).
    const P3_PROFILE_IN_SRGB: [[u8; 4]; 4] = [
        [218, 67, 47, 255],
        [0, 185, 77, 255],
        [62, 90, 209, 255],
        [129, 129, 129, 255],
    ];

    /// Where the ICC fixture's profile starts, after its `colr` type `prof`.
    fn profile_start(bytes: &[u8]) -> usize {
        let at = bytes.windows(4).position(|w| w == b"prof").unwrap();
        assert_eq!(&bytes[at - 4..at], b"colr", "the first `prof` is the type");
        at + 4
    }

    #[test]
    fn converts_an_icc_profile_and_flags_one_it_cannot_apply() {
        let icc = fixture("avif-display-p3-icc.avif");
        let reply = avif("avif-display-p3-icc.avif", 1600).unwrap();
        assert_eq!(reply.flags, ImageFlags::default());
        assert_eq!(reply.image.format, "AVIF · first image");
        assert_quadrants(&reply.image, P3_PROFILE_IN_SRGB, 12, "icc");

        // The profile takes precedence over the `nclx` beside it: a PQ transfer
        // flags nothing, and Display P3 primaries do not convert the pixels a
        // second time.
        for (case, nclx) in [
            ("P3 primaries and a PQ transfer", [0, 12, 0, 16]),
            ("P3 primaries", [0, 12, 0, 13]),
        ] {
            let bytes = patched(&icc, b"nclx", 0, 0, &nclx);
            let reply = decoded(ConfinedFormat::Avif, 1600, &bytes)
                .unwrap_or_else(|kind| panic!("{case}: {kind:?}"));
            assert_eq!(reply.flags, ImageFlags::default(), "{case}");
            assert_eq!(reply.image.format, "AVIF · first image", "{case}");
            assert_quadrants(&reply.image, P3_PROFILE_IN_SRGB, 12, case);
        }

        // Each profile is refused, so the stored values stay as they are (the
        // `nclx` beside it is BT.709) and the caption says so. Offsets are
        // into the ICC header, which starts right after `prof`.
        profile_start(&icc);
        for (case, at, with) in [
            ("no `acsp` signature", 36, *b"acsx"),
            // A tag table running past the end of the profile.
            ("2^32 - 1 tags", 128, u32::MAX.to_be_bytes()),
            ("a grey profile", 16, *b"GRAY"),
        ] {
            let bytes = patched(&icc, b"prof", 0, at, &with);
            let reply = decoded(ConfinedFormat::Avif, 1600, &bytes)
                .unwrap_or_else(|kind| panic!("{case}: {kind:?}"));
            assert!(reply.flags.profile_not_applied, "{case}");
            assert_eq!(
                reply.image.format, "AVIF · first image · colour profile not applied",
                "{case}"
            );
            assert_quadrants(&reply.image, P3_STORED, 12, case);
        }
    }

    #[test]
    fn profile_size_cap_refuses_before_parsing() {
        let bytes = fixture("avif-display-p3-icc.avif");
        let start = profile_start(&bytes);
        let colr = start - 12;
        let end = colr + u32::from_be_bytes(bytes[colr..colr + 4].try_into().unwrap()) as usize;
        let profile = &bytes[start..end];
        let stored = RgbaImage::from_pixel(4, 2, image::Rgba(P3_STORED[0]));
        // The same profile padded to the cap converts; one byte more does not
        // reach moxcms.
        for (length, applies) in [
            (profile.len(), true),
            (MAX_ICC_PROFILE_BYTES, true),
            (MAX_ICC_PROFILE_BYTES + 1, false),
        ] {
            let mut padded = profile.to_vec();
            padded.resize(length, 0);
            padded[..4].copy_from_slice(&(length as u32).to_be_bytes());
            let mut rgba = stored.clone();
            assert_eq!(apply_profile(&mut rgba, &padded), applies, "{length}");
            let expected = if applies {
                P3_PROFILE_IN_SRGB[0]
            } else {
                P3_STORED[0]
            };
            for pixel in rgba.pixels() {
                assert_near(pixel.0, expected, 1, &format!("{length}"));
            }
        }
    }

    #[test]
    fn orients_in_place_like_the_copying_operations() {
        use image::imageops::{flip_horizontal, flip_vertical, rotate90, rotate180, rotate270};
        let source = RgbaImage::from_fn(5, 3, |x, y| image::Rgba([x as u8, y as u8, 7, 255]));
        for rotation in 0..4 {
            for mirror in [None, Some(0), Some(1)] {
                let rotated = match rotation {
                    1 => rotate270(&source),
                    2 => rotate180(&source),
                    3 => rotate90(&source),
                    _ => source.clone(),
                };
                let expected = match mirror {
                    Some(0) => flip_vertical(&rotated),
                    Some(_) => flip_horizontal(&rotated),
                    None => rotated,
                };
                assert_eq!(
                    orient(source.clone(), rotation, mirror),
                    expected,
                    "irot {rotation}, imir {mirror:?}"
                );
            }
        }
    }

    /// Requirement 18: `convert` and `apply_alpha` take dav1d's pictures by
    /// value, so their planes are freed before colour conversion, orientation
    /// and scaling allocate, and `decode` keeps no copy of either.
    #[test]
    fn decoded_pictures_are_freed_before_the_rgba_steps() {
        let source = include_str!("avif.rs");
        let code = source.split("\n#[cfg(test)]\nmod tests").next().unwrap();
        assert!(code.contains("fn convert(picture: Picture, "));
        assert!(code.contains("fn apply_alpha(rgba: &mut RgbaImage, picture: Picture)"));
        let body = code
            .split("pub(crate) fn decode(")
            .nth(1)
            .and_then(|rest| rest.split("\n}\n").next())
            .unwrap();
        assert!(!body.contains(".clone()"));
        let consumed = body.find("apply_alpha(&mut rgba, alpha)").unwrap();
        assert!(body.find("convert(picture, ").unwrap() < consumed);
        let rest = &body[consumed..];
        assert!(!rest.contains("picture"), "{rest}");
        for step in ["to_srgb(", "orient(", "finish("] {
            assert!(rest.contains(step), "{step}");
        }
    }

    /// Replaces the `index`th `kind` box's first bytes after its header.
    fn patched(bytes: &[u8], kind: &[u8; 4], index: usize, at: usize, with: &[u8]) -> Vec<u8> {
        let mut bytes = bytes.to_vec();
        let start = bytes
            .windows(4)
            .enumerate()
            .filter(|(_, window)| window == kind)
            .nth(index)
            .unwrap()
            .0
            + 4
            + at;
        bytes[start..start + with.len()].copy_from_slice(with);
        bytes
    }

    #[test]
    fn hostile_avif_ends_in_its_error_kind_before_decoding() {
        use ImageErrorKind::*;
        let still = fixture("avif-yuv420-8bit-alpha.avif");
        let small_alpha = fixture("avif-alpha-smaller-than-image.avif");
        let ispe = |width: u32, height: u32| {
            let mut size = width.to_be_bytes().to_vec();
            size.extend_from_slice(&height.to_be_bytes());
            patched(&still, b"ispe", 0, 4, &size)
        };
        // The sequence header's first OBU claims more bytes than the item holds.
        let mut truncated = still.clone();
        let mdat = truncated.windows(4).position(|w| w == b"mdat").unwrap() + 4;
        assert_eq!(
            truncated[mdat], 0x0a,
            "the item starts with its sequence header"
        );
        truncated[mdat + 1] = 0x7f;
        for (case, bytes, kind) in [
            // 40000 × 1 passes the pixel limit but not the edge limit.
            ("ispe 40000 × 1", ispe(40_000, 1), EdgeLimit),
            ("ispe 8000 × 8000", ispe(8_000, 8_000), PixelLimit),
            ("ispe 0 × 48", ispe(0, 48), InvalidDimensions),
            (
                "missing ispe",
                patched(&still, b"ispe", 0, 0, b"free"),
                AvifUnreadable,
            ),
            // The alpha item's own 32 × 32 `ispe` disagrees with the primary's.
            (
                "32 × 32 alpha under 64 × 48",
                small_alpha.clone(),
                AvifSizeMismatch,
            ),
            // Its `ispe` claims the primary's size; the decoded picture does not.
            (
                "32 × 32 alpha declared 64 × 48",
                patched(&small_alpha, b"ispe", 1, 4, &[0, 0, 0, 64, 0, 0, 0, 48]),
                AvifSizeMismatch,
            ),
            ("truncated OBU", truncated, AvifUnreadable),
            // dav1d refuses the 16384 × 16384 frame against `frame_size_limit`.
            (
                "frame larger than ispe",
                fixture("avif-frame-larger-than-ispe.avif"),
                AvifSizeMismatch,
            ),
        ] {
            assert_eq!(
                decoded(ConfinedFormat::Avif, 100, &bytes).unwrap_err(),
                kind,
                "{case}"
            );
        }
    }

    fn bmff(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let size = u32::try_from(8 + payload.len()).unwrap();
        [&size.to_be_bytes()[..], kind, payload].concat()
    }

    fn full(kind: &[u8; 4], flags: u32, payload: &[u8]) -> Vec<u8> {
        bmff(kind, &[&flags.to_be_bytes()[..], payload].concat())
    }

    /// A version 0 `meta` whose primary item 1 may take property 1, a 64 × 48
    /// `ispe`, and an item may take property 2, an alpha `auxC`. Each slice of
    /// `ipma` is one `ipma` box of `(item, property indices)` entries, and
    /// each of `auxiliary` has an `auxl` reference to item 1.
    fn item_properties(ipma: &[&[(u16, Vec<u8>)]], auxiliary: &[u16]) -> Vec<u8> {
        let ispe = full(b"ispe", 0, &[0, 0, 0, 64, 0, 0, 0, 48]);
        let auxc = full(b"auxC", 0, ALPHA_URNS[0]);
        let mut iprp = bmff(b"ipco", &[ispe, auxc].concat());
        for entries in ipma {
            let mut payload = u32::try_from(entries.len()).unwrap().to_be_bytes().to_vec();
            for (item, indices) in *entries {
                payload.extend_from_slice(&item.to_be_bytes());
                payload.push(u8::try_from(indices.len()).unwrap());
                payload.extend_from_slice(indices);
            }
            iprp.extend(full(b"ipma", 0, &payload));
        }
        let references: Vec<u8> = auxiliary
            .iter()
            .flat_map(|item| bmff(b"auxl", &[&item.to_be_bytes()[..], &[0, 1, 0, 1]].concat()))
            .collect();
        let meta = [
            full(b"pitm", 0, &[0, 1]),
            full(b"iref", 0, &references),
            bmff(b"iprp", &iprp),
        ];
        full(b"meta", 0, &meta.concat())
    }

    #[test]
    fn ipma_keeps_only_the_primary_and_its_auxl_items_within_its_bounds() {
        let primary = (1, vec![1]);
        let alpha = (2, vec![1, 2]);
        // What a parse kept: the associations and the `auxl` items.
        let parse = |ipma: &[&[(u16, Vec<u8>)]], auxiliary: &[u16]| {
            Properties::parse(&item_properties(ipma, auxiliary))
                .map(|properties| (properties.associations, properties.auxiliary))
        };
        // As many entries as the bound: only the primary's and the alpha
        // item's associations are kept.
        let mut entries = vec![primary.clone(), alpha.clone()];
        entries.extend(
            (3..)
                .take(MAX_IPMA_ENTRIES as usize - 2)
                .map(|item| (item, vec![1, 2])),
        );
        let bytes = item_properties(&[&entries], &[2]);
        let properties = Properties::parse(&bytes).unwrap();
        assert_eq!(properties.associations, [(1, 1), (2, 1), (2, 2)]);
        assert_eq!(properties.spatial_extents(1), Some((64, 48)));
        assert_eq!(properties.alpha_items(), [2]);
        // One entry more is refused by the count alone, in one box or summed
        // over two.
        entries.push((5_000, vec![1]));
        assert_eq!(parse(&[&entries], &[2]), None);
        assert_eq!(parse(&[&entries[..2], &entries[2..]], &[2]), None);
        assert!(parse(&[&entries[..2], &entries[3..]], &[2]).is_some());

        // The kept associations: exactly the bound for the primary, over
        // entries of at most 255, then one more.
        let kept = |extra: usize| {
            let mut entries = vec![(1, vec![1; 255]); MAX_KEPT_ASSOCIATIONS / 255];
            entries.push((1, vec![1; MAX_KEPT_ASSOCIATIONS % 255 + extra]));
            parse(&[&entries], &[]).map(|(associations, _)| associations.len())
        };
        assert_eq!(kept(0), Some(MAX_KEPT_ASSOCIATIONS));
        assert_eq!(kept(1), None);

        // `auxl` items of the primary up to their bound.
        let auxiliary: Vec<u16> = (2..).take(MAX_AUXILIARY_ITEMS + 1).collect();
        let only_primary = std::slice::from_ref(&primary);
        let (_, found) = parse(&[only_primary], &auxiliary[..MAX_AUXILIARY_ITEMS]).unwrap();
        let expected: Vec<u32> = (2..).take(MAX_AUXILIARY_ITEMS).collect();
        assert_eq!(found, expected);
        assert_eq!(parse(&[only_primary], &auxiliary), None);
    }

    /// The AVIF counterpart of the real `hayro-jpeg2000` panic, kept for A3:
    /// the 8-bit quadrants without `colr`, their sequence header rewritten to
    /// the reserved primaries 0, transfer 0 and matrix 3. Every check before
    /// decoding passes and libdav1d decodes the picture; dav1d-rs 0.11.1's
    /// `color_primaries`, `transfer_characteristic` and `matrix_coefficients`
    /// each reach `unreachable!()` on them (also for primaries 3 and 13 to 21,
    /// transfer 3), so reading the signalled colour panics, and `serve` turns
    /// the panic into `DecoderFailed`.
    #[test]
    fn a_dav1d_rs_panic_past_every_check_ends_as_decoder_failed() {
        use std::panic::{AssertUnwindSafe, catch_unwind};
        let bytes = fixture("avif-reserved-colour.avif");
        let context = mp4parse::read_avif(&mut &bytes[..], ParseStrictness::Normal).unwrap();
        assert!(context.unsupported_features.is_empty());
        assert!(context.alpha_item_coded_data().is_none());
        let properties = Properties::parse(&bytes).unwrap();
        assert_eq!(
            properties.spatial_extents(properties.primary),
            Some((64, 48))
        );
        assert_eq!(properties.colour(properties.primary), None);
        let picture = decode_item(context.primary_item_coded_data().unwrap(), (64, 48)).unwrap();
        let panics =
            |read: &dyn Fn(&Picture)| catch_unwind(AssertUnwindSafe(|| read(&picture))).is_err();
        assert!(panics(&|picture| {
            let _ = picture.color_primaries();
        }));
        assert!(panics(&|picture| {
            let _ = picture.transfer_characteristic();
        }));
        assert!(panics(&|picture| {
            let _ = picture.matrix_coefficients();
        }));
        drop(picture);
        assert_eq!(
            avif("avif-reserved-colour.avif", 1600).unwrap_err(),
            ImageErrorKind::DecoderFailed
        );
    }

    /// The design's two structural refusals, on files `mp4parse` reads as
    /// such: a grid primary item, whose tiles it cannot expose, and an image
    /// sequence with no still primary item.
    #[test]
    fn refuses_a_grid_and_a_sequence_without_a_still() {
        let grid = fixture("avif-grid.avif");
        let context = mp4parse::read_avif(&mut &grid[..], ParseStrictness::Normal).unwrap();
        assert!(context.unsupported_features.contains(Feature::Grid));
        assert!(context.sequence.is_none());
        let sequence = fixture("avif-sequence-without-still.avif");
        let context = mp4parse::read_avif(&mut &sequence[..], ParseStrictness::Normal).unwrap();
        assert!(context.unsupported_features.is_empty());
        assert!(context.sequence.is_some());
        assert!(context.primary_item_coded_data().is_none());
        for (name, kind) in [
            ("avif-grid.avif", ImageErrorKind::AvifGrid),
            (
                "avif-sequence-without-still.avif",
                ImageErrorKind::AvifNoStill,
            ),
        ] {
            assert_eq!(avif(name, 1600).unwrap_err(), kind, "{name}");
        }
    }

    #[test]
    fn libdav1d_version_floor_parses_strictly() {
        for version in ["1.4.1", "1.5.4", "1.4.1-0-gabcdef", "1.10.0", "2.0.0"] {
            assert!(supported_libdav1d(version), "{version}");
        }
        for version in [
            "1.4.0", "1.3.9", "0.9.9", "", "1", "1.4", "1.4.", "x.4.1", "1.x.1", "1.4.x", "v1.5.4",
            " 1.5.4", "1.5 .4", "1.5.4.0x",
        ] {
            assert_eq!(
                supported_libdav1d(version),
                version == "1.5.4.0x",
                "{version}"
            );
        }
        let installed = crate::libdav1d_version().expect("libdav1d reports a version");
        assert!(supported_libdav1d(&installed), "{installed}");
    }

    /// Source files of this crate and its examples, for the structural checks
    /// below.
    fn sources() -> Vec<(String, String)> {
        let mut sources = Vec::new();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut directories = vec![root.join("src"), root.join("examples")];
        while let Some(directory) = directories.pop() {
            for entry in std::fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    directories.push(path);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    sources.push((path.display().to_string(), text));
                }
            }
        }
        sources
    }

    /// Whether a line names the `moxcms` crate: the whole word anywhere but in
    /// a line that is only a comment. A trailing or block comment naming it
    /// counts too, on the safe side.
    fn names_moxcms(line: &str) -> bool {
        if line.trim_start().starts_with("//") {
            return false;
        }
        let word = |c: char| c.is_alphanumeric() || c == '_';
        line.match_indices("moxcms").any(|(at, name)| {
            !line[..at].ends_with(word) && !line[at + name.len()..].starts_with(word)
        })
    }

    #[test]
    fn decoders_use_only_the_reviewed_codec_entries() {
        // Split so this file does not match itself. Anywhere in the crate:
        let anywhere = [
            ["register_", "decoding_hook"].concat(),
            ["register_format_", "detection_hook"].concat(),
            ["Jp2", "Decoder"].concat(),
            ["with_settings_", "and_allocator"].concat(),
            ["with_", "allocator"].concat(),
            ["dav1d_parse_", "sequence_header"].concat(),
        ];
        // dav1d-rs calls outside the reviewed list, in the module that uses it.
        let outside_the_list = [
            ["content_", "light"].concat(),
            ["mastering_", "display"].concat(),
            ["chroma_", "location"].concat(),
            ["allocator_", "data"].concat(),
            ["plane_data_", "ptr"].concat(),
            ["get_frame_", "delay"].concat(),
            ["seq_", "hdr"].concat(),
            ["frame_", "hdr"].concat(),
            ["Decoder::", "new"].concat(),
            ["set_strict_", "std_compliance"].concat(),
            ["set_inloop_", "filters"].concat(),
            ["set_decode_", "frame_type"].concat(),
        ];
        let sources = sources();
        for (path, text) in &sources {
            // Product code only: the layout probe below names every field.
            let code = text.split("\n#[cfg(test)]\nmod tests").next().unwrap();
            let dav1d = path.ends_with("/avif.rs");
            for needle in anywhere
                .iter()
                .chain(outside_the_list.iter().filter(|_| dav1d))
            {
                assert!(!code.contains(needle.as_str()), "{path} uses {needle}");
            }
        }
        // Every dav1d call is in that module, and so is every use of the
        // moxcms crate (an AVIF's ICC profile; `image` and hayro-jpeg2000 call
        // it themselves): no other line names it, as a path, `use moxcms as`,
        // `extern crate` or in a macro, and the manifest gives it no other name.
        for line in [
            "use moxcms as cms;",
            "extern crate moxcms;",
            "    convert!(::moxcms::ColorProfile::new_srgb());",
            "let profile = r#moxcms::ColorProfile::new_srgb(); // sRGB",
        ] {
            assert!(names_moxcms(line), "{line}");
        }
        for line in [
            "/// Converts as moxcms does.",
            "    // moxcms",
            "moxcms_profile",
        ] {
            assert!(!names_moxcms(line), "{line}");
        }
        for (path, text) in &sources {
            if !path.ends_with("/avif.rs") && !path.ends_with("/lib.rs") {
                assert!(!text.contains(["dav1d", "::"].concat().as_str()), "{path}");
            }
            if !path.ends_with("/avif.rs") {
                for (index, line) in text.lines().enumerate() {
                    assert!(!names_moxcms(line), "{path}:{} names moxcms", index + 1);
                }
            }
        }
        let manifest: String = include_str!("../Cargo.toml").split_whitespace().collect();
        assert!(!manifest.contains("package=\"moxcms\""));
        for module in ["avif.rs", "jpeg2000_hayro.rs", "image_helper.rs"] {
            let (path, text) = sources
                .iter()
                .find(|(path, _)| path.ends_with(module))
                .unwrap();
            assert!(text.contains("\n#![forbid(unsafe_code)]\n"), "{path}");
        }
        // `libdav1d_version` holds the one unsafe block of the image decoders.
        let (_, lib) = sources
            .iter()
            .find(|(path, _)| path.ends_with("/lib.rs"))
            .unwrap();
        assert_eq!(lib.matches(["unsafe", " {"].concat().as_str()).count(), 1);
    }

    /// Requirement 8: the C layouts dav1d-sys 0.8.3 writes by hand against the
    /// installed headers. Equal sizes for every struct C writes into Rust
    /// memory and equal offsets for every field read; #126 and its picture
    /// counterpart are the recorded mismatches.
    #[test]
    fn libdav1d_layouts_match_the_binding() {
        use dav1d_sys::*;
        let probe = r#"
#include <stddef.h>
#include <stdio.h>
#include <dav1d/dav1d.h>
#define S(t) printf("size %s %zu\n", #t, sizeof(t));
#define O(t, f) printf("offset %s.%s %zu\n", #t, #f, offsetof(t, f));
int main(void) {
    printf("api %d\n", DAV1D_API_VERSION_MAJOR);
    S(Dav1dSettings) S(Dav1dData) S(Dav1dDataProps) S(Dav1dUserData) S(Dav1dPicture)
    S(Dav1dPictureParameters) S(Dav1dSequenceHeader)
    O(Dav1dSettings, n_threads) O(Dav1dSettings, max_frame_delay) O(Dav1dSettings, apply_grain)
    O(Dav1dSettings, operating_point) O(Dav1dSettings, all_layers)
    O(Dav1dSettings, frame_size_limit) O(Dav1dSettings, allocator) O(Dav1dSettings, logger)
    O(Dav1dSettings, strict_std_compliance) O(Dav1dSettings, output_invisible_frames)
    O(Dav1dSettings, inloop_filters) O(Dav1dSettings, decode_frame_type) O(Dav1dSettings, reserved)
    O(Dav1dPicture, seq_hdr) O(Dav1dPicture, frame_hdr) O(Dav1dPicture, data) O(Dav1dPicture, stride)
    O(Dav1dPicture, p) O(Dav1dPicture, m) O(Dav1dPicture, content_light)
    O(Dav1dPicture, mastering_display) O(Dav1dPicture, itut_t35) O(Dav1dPicture, ref)
    O(Dav1dPicture, allocator_data) O(Dav1dPicture, frame_hdr_ref) O(Dav1dPicture, n_itut_t35)
    O(Dav1dPictureParameters, w) O(Dav1dPictureParameters, h) O(Dav1dPictureParameters, layout)
    O(Dav1dPictureParameters, bpc)
    O(Dav1dSequenceHeader, layout) O(Dav1dSequenceHeader, pri) O(Dav1dSequenceHeader, trc)
    O(Dav1dSequenceHeader, mtrx) O(Dav1dSequenceHeader, chr) O(Dav1dSequenceHeader, hbd)
    O(Dav1dSequenceHeader, color_range)
    return 0;
}
"#;
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("probe.c");
        let binary = directory.path().join("probe");
        std::fs::write(&source, probe).unwrap();
        let flags = Command::new("pkg-config")
            .args(["--cflags", "dav1d"])
            .output()
            .expect("pkg-config runs");
        assert!(flags.status.success());
        let flags = String::from_utf8(flags.stdout).unwrap();
        let compiler = std::env::var("CC").unwrap_or_else(|_| "cc".into());
        let built = Command::new(compiler)
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .args(flags.split_whitespace())
            .output()
            .expect("the C compiler runs");
        assert!(
            built.status.success(),
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let output = Command::new(&binary).output().unwrap();
        let output = String::from_utf8(output.stdout).unwrap();
        println!("{output}");
        let c: BTreeMap<String, usize> = output
            .lines()
            .map(|line| {
                let mut words = line.split_whitespace();
                let (kind, name, value) =
                    (words.next().unwrap(), words.next().unwrap(), words.next());
                match value {
                    Some(value) => (format!("{kind} {name}"), value.parse().unwrap()),
                    None => (kind.to_owned(), name.parse().unwrap()),
                }
            })
            .collect();
        // libdav1d.so.7 is API major 7.
        assert_eq!(c["api"], 7);
        let rust: BTreeMap<String, usize> = [
            ("size Dav1dSettings", size_of::<Dav1dSettings>()),
            ("size Dav1dData", size_of::<Dav1dData>()),
            ("size Dav1dDataProps", size_of::<Dav1dDataProps>()),
            ("size Dav1dUserData", size_of::<Dav1dUserData>()),
            ("size Dav1dPicture", size_of::<Dav1dPicture>()),
            (
                "size Dav1dPictureParameters",
                size_of::<Dav1dPictureParameters>(),
            ),
            (
                "offset Dav1dSettings.n_threads",
                offset_of!(Dav1dSettings, n_threads),
            ),
            (
                "offset Dav1dSettings.max_frame_delay",
                offset_of!(Dav1dSettings, max_frame_delay),
            ),
            (
                "offset Dav1dSettings.apply_grain",
                offset_of!(Dav1dSettings, apply_grain),
            ),
            (
                "offset Dav1dSettings.operating_point",
                offset_of!(Dav1dSettings, operating_point),
            ),
            (
                "offset Dav1dSettings.all_layers",
                offset_of!(Dav1dSettings, all_layers),
            ),
            (
                "offset Dav1dSettings.frame_size_limit",
                offset_of!(Dav1dSettings, frame_size_limit),
            ),
            (
                "offset Dav1dSettings.allocator",
                offset_of!(Dav1dSettings, allocator),
            ),
            (
                "offset Dav1dSettings.logger",
                offset_of!(Dav1dSettings, logger),
            ),
            (
                "offset Dav1dSettings.strict_std_compliance",
                offset_of!(Dav1dSettings, strict_std_compliance),
            ),
            (
                "offset Dav1dSettings.output_invisible_frames",
                offset_of!(Dav1dSettings, output_invisible_frames),
            ),
            (
                "offset Dav1dSettings.inloop_filters",
                offset_of!(Dav1dSettings, inloop_filters),
            ),
            (
                "offset Dav1dSettings.decode_frame_type",
                offset_of!(Dav1dSettings, decode_frame_type),
            ),
            (
                "offset Dav1dSettings.reserved",
                offset_of!(Dav1dSettings, reserved),
            ),
            (
                "offset Dav1dPicture.seq_hdr",
                offset_of!(Dav1dPicture, seq_hdr),
            ),
            (
                "offset Dav1dPicture.frame_hdr",
                offset_of!(Dav1dPicture, frame_hdr),
            ),
            ("offset Dav1dPicture.data", offset_of!(Dav1dPicture, data)),
            (
                "offset Dav1dPicture.stride",
                offset_of!(Dav1dPicture, stride),
            ),
            ("offset Dav1dPicture.p", offset_of!(Dav1dPicture, p)),
            ("offset Dav1dPicture.m", offset_of!(Dav1dPicture, m)),
            (
                "offset Dav1dPicture.content_light",
                offset_of!(Dav1dPicture, content_light),
            ),
            (
                "offset Dav1dPicture.mastering_display",
                offset_of!(Dav1dPicture, mastering_display),
            ),
            (
                "offset Dav1dPicture.itut_t35",
                offset_of!(Dav1dPicture, itut_t35),
            ),
            ("offset Dav1dPicture.ref", offset_of!(Dav1dPicture, ref_)),
            (
                "offset Dav1dPicture.allocator_data",
                offset_of!(Dav1dPicture, allocator_data),
            ),
            (
                "offset Dav1dPictureParameters.w",
                offset_of!(Dav1dPictureParameters, w),
            ),
            (
                "offset Dav1dPictureParameters.h",
                offset_of!(Dav1dPictureParameters, h),
            ),
            (
                "offset Dav1dPictureParameters.layout",
                offset_of!(Dav1dPictureParameters, layout),
            ),
            (
                "offset Dav1dPictureParameters.bpc",
                offset_of!(Dav1dPictureParameters, bpc),
            ),
            (
                "offset Dav1dSequenceHeader.layout",
                offset_of!(Dav1dSequenceHeader, layout),
            ),
            (
                "offset Dav1dSequenceHeader.pri",
                offset_of!(Dav1dSequenceHeader, pri),
            ),
            (
                "offset Dav1dSequenceHeader.trc",
                offset_of!(Dav1dSequenceHeader, trc),
            ),
            (
                "offset Dav1dSequenceHeader.mtrx",
                offset_of!(Dav1dSequenceHeader, mtrx),
            ),
            (
                "offset Dav1dSequenceHeader.chr",
                offset_of!(Dav1dSequenceHeader, chr),
            ),
            (
                "offset Dav1dSequenceHeader.hbd",
                offset_of!(Dav1dSequenceHeader, hbd),
            ),
            (
                "offset Dav1dSequenceHeader.color_range",
                offset_of!(Dav1dSequenceHeader, color_range),
            ),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();
        for (name, value) in &rust {
            assert_eq!(
                c[name], *value,
                "{name}: C {} against Rust {value}",
                c[name]
            );
        }
        // The recorded mismatches: #126 (the sequence header) and C1 (the
        // picture's reference pointers one word early, `n_itut_t35` moved).
        // If dav1d-sys fixes them, this fails so the review is updated.
        let word = size_of::<usize>();
        assert_ne!(
            c["size Dav1dSequenceHeader"],
            size_of::<Dav1dSequenceHeader>()
        );
        assert_eq!(
            c["offset Dav1dPicture.frame_hdr_ref"],
            offset_of!(Dav1dPicture, frame_hdr_ref) + word
        );
        assert_ne!(
            c["offset Dav1dPicture.n_itut_t35"],
            offset_of!(Dav1dPicture, n_itut_t34)
        );
        assert_eq!(c.len(), rust.len() + 4);
    }
}
