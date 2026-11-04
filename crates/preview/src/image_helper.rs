//! The image frame of GitTurtle's confined helper protocol, and the helper's
//! side of it: AVIF ([`crate::avif`]) and JPEG 2000 ([`crate::jpeg2000_hayro`])
//! decode only here, reached through [`crate::pdf_helper::serve`].
//!
//! Request: magic `GTIq`, version (u32), format (u8: 1 AVIF, 2 JP2, 3 J2K),
//! output edge (u32, 1..=1,600), length (u32, at most 32 MiB), then the image
//! bytes. Reply: magic `GTIr`, version (u32), a status byte, then either an
//! error kind (u8) with its detail (u32, always 0), or the format echo (u8),
//! flags (u16), original and output size (u32 each), parse, decode and
//! convert times in microseconds (u32 each), then exactly width × height × 4
//! straight RGBA bytes. All integers are little-endian.
#![forbid(unsafe_code)]

use std::io::{self, Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

use anyhow::{Result, bail, ensure};
use image::RgbaImage;

use crate::pdf_helper::PROTOCOL_VERSION;
use crate::{
    ImagePreview, MAX_DECODER_BYTES, MAX_INPUT_BYTES, MAX_SOURCE_EDGE, MAX_SOURCE_PIXELS,
    confined_image_format, premultiply, scaled_dimensions, unpremultiply,
};

/// The fixed image request header [`encode_image_request`] returns.
pub const IMAGE_REQUEST_HEADER_BYTES: usize = 17;
/// The helper never returns an image larger than this on its longer edge;
/// History asks for 1,600 and Markdown for 1,000.
pub const MAX_HELPER_IMAGE_EDGE: u32 = 1_600;

pub(crate) const REQUEST_MAGIC: [u8; 4] = *b"GTIq";
const REPLY_MAGIC: [u8; 4] = *b"GTIr";
const REPLY_PREFIX_BYTES: usize = 9;
const FAILURE_BYTES: usize = 5;
const IMAGE_HEADER_BYTES: usize = 31;
/// The bound `metadata::iso_image_format` reads for `ftyp`.
const SNIFF_BYTES: usize = 4_096;
const STATUS_IMAGE: u8 = 0;
const STATUS_FAILED: u8 = 1;

/// A format the Linux helper decodes. HEIF is never one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfinedFormat {
    Avif,
    Jp2,
    J2k,
}

impl ConfinedFormat {
    fn code(self) -> u8 {
        match self {
            Self::Avif => 1,
            Self::Jp2 => 2,
            Self::J2k => 3,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Avif),
            2 => Some(Self::Jp2),
            3 => Some(Self::J2k),
            _ => None,
        }
    }

    /// The format's name as the rest of the preview names it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Avif => "AVIF",
            Self::Jp2 => "JPEG 2000 / JP2",
            Self::J2k => "JPEG 2000 / J2K codestream",
        }
    }
}

/// What the helper disclosed about an AVIF it decoded. JPEG 2000 sets none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImageFlags {
    /// The file is an animation; the reply holds its still (first) image.
    pub animation: bool,
    /// A `clap` crop was present and not applied.
    pub crop_not_applied: bool,
    /// PQ or HLG content shown without tone mapping.
    pub hdr_not_tone_mapped: bool,
    /// An embedded ICC profile was not applied.
    pub profile_not_applied: bool,
}

impl ImageFlags {
    const ANIMATION: u16 = 1;
    const CROP: u16 = 1 << 1;
    const HDR: u16 = 1 << 2;
    const PROFILE: u16 = 1 << 3;
    const KNOWN: u16 = Self::ANIMATION | Self::CROP | Self::HDR | Self::PROFILE;

    fn bits(self) -> u16 {
        [
            (self.animation, Self::ANIMATION),
            (self.crop_not_applied, Self::CROP),
            (self.hdr_not_tone_mapped, Self::HDR),
            (self.profile_not_applied, Self::PROFILE),
        ]
        .into_iter()
        .filter(|(set, _)| *set)
        .fold(0, |bits, (_, bit)| bits | bit)
    }

    /// Only known flags, and only the ones `format` can produce.
    fn from_bits(bits: u16, format: ConfinedFormat) -> Option<Self> {
        let allowed = match format {
            ConfinedFormat::Avif => Self::KNOWN,
            ConfinedFormat::Jp2 | ConfinedFormat::J2k => 0,
        };
        (bits & !allowed == 0).then_some(Self {
            animation: bits & Self::ANIMATION != 0,
            crop_not_applied: bits & Self::CROP != 0,
            hdr_not_tone_mapped: bits & Self::HDR != 0,
            profile_not_applied: bits & Self::PROFILE != 0,
        })
    }

    /// The caption for an image of `format` with these flags. Built only from
    /// the format echo and the flags, never from helper bytes.
    pub fn caption(self, format: ConfinedFormat) -> String {
        let mut caption = format.name().to_owned();
        caption.push_str(if self.animation {
            " · first frame of an animation"
        } else {
            " · first image"
        });
        for (set, words) in [
            (self.crop_not_applied, " · crop not applied"),
            (self.hdr_not_tone_mapped, " · HDR not tone-mapped"),
            (self.profile_not_applied, " · color profile not applied"),
        ] {
            if set {
                caption.push_str(words);
            }
        }
        caption
    }
}

/// Why the helper returned no image. A closed set with no detail on the wire;
/// the client words each kind itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageErrorKind {
    /// The request frame was malformed: magic, version, format or edge, or truncated.
    Request,
    /// The image exceeds the 32 MiB input limit.
    InputTooLarge,
    /// The bytes are not the requested format.
    FormatMismatch,
    /// The image declares a zero width or height.
    InvalidDimensions,
    /// The image is wider or taller than the 32,768-pixel edge limit.
    EdgeLimit,
    /// The image is larger than the 32-million-pixel limit.
    PixelLimit,
    /// Decoding would exceed the 256 MiB limit.
    DecodedTooLarge,
    /// The decoder stopped on this image (a panic inside it).
    DecoderFailed,
    /// The AVIF cannot be read: corrupt, or an unsupported structure.
    AvifUnreadable,
    /// The AVIF's primary image is a grid.
    AvifGrid,
    /// The AVIF is an animation with no still image.
    AvifNoStill,
    /// A decoded AVIF picture or its alpha disagrees with the declared size.
    AvifSizeMismatch,
    /// The system libdav1d is older than 1.4.1 or reports no readable version.
    Dav1dTooOld,
    /// The JPEG 2000 cannot be read: corrupt, or an unsupported codestream.
    Jpeg2000Unreadable,
    /// The codestream declares more than 65,535 tiles.
    Jpeg2000TooManyTiles,
    /// The codestream has more than four components, or a palette's `cmap`
    /// maps more than four (one output channel each).
    Jpeg2000TooManyComponents,
    /// A component or palette column is deeper than 16 bits.
    Jpeg2000TooDeep,
    /// A JPX file that is not JP2-compatible.
    JpxNotJp2Compatible,
}

impl ImageErrorKind {
    const ALL: [Self; 18] = [
        Self::Request,
        Self::InputTooLarge,
        Self::FormatMismatch,
        Self::InvalidDimensions,
        Self::EdgeLimit,
        Self::PixelLimit,
        Self::DecodedTooLarge,
        Self::DecoderFailed,
        Self::AvifUnreadable,
        Self::AvifGrid,
        Self::AvifNoStill,
        Self::AvifSizeMismatch,
        Self::Dav1dTooOld,
        Self::Jpeg2000Unreadable,
        Self::Jpeg2000TooManyTiles,
        Self::Jpeg2000TooManyComponents,
        Self::Jpeg2000TooDeep,
        Self::JpxNotJp2Compatible,
    ];

    fn code(self) -> u8 {
        // The wire code is the position in `ALL`, plus one.
        Self::ALL.iter().position(|kind| *kind == self).unwrap() as u8 + 1
    }

    fn from_code(code: u8) -> Option<Self> {
        Self::ALL.get(usize::from(code).checked_sub(1)?).copied()
    }

    /// Whether a helper decoding `format` can return this kind.
    fn produced_by(self, format: ConfinedFormat) -> bool {
        match self {
            Self::AvifUnreadable
            | Self::AvifGrid
            | Self::AvifNoStill
            | Self::AvifSizeMismatch
            | Self::Dav1dTooOld => format == ConfinedFormat::Avif,
            Self::Jpeg2000Unreadable
            | Self::Jpeg2000TooManyTiles
            | Self::Jpeg2000TooManyComponents
            | Self::Jpeg2000TooDeep => format != ConfinedFormat::Avif,
            Self::JpxNotJp2Compatible => format == ConfinedFormat::Jp2,
            Self::Request
            | Self::InputTooLarge
            | Self::FormatMismatch
            | Self::InvalidDimensions
            | Self::EdgeLimit
            | Self::PixelLimit
            | Self::DecodedTooLarge
            | Self::DecoderFailed => true,
        }
    }
}

/// Microseconds the helper spent in each phase, saturating at `u32::MAX`.
/// Metrics only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImageTimings {
    /// Reading the container and checking its bounds.
    pub parse_us: u32,
    /// Running the codec.
    pub decode_us: u32,
    /// Colour conversion, orientation and the resize.
    pub convert_us: u32,
}

/// One decoded image. `image.format` is [`ImageFlags::caption`].
#[derive(Debug)]
pub struct ImageReply {
    pub format: ConfinedFormat,
    pub flags: ImageFlags,
    /// At most the requested edge on its longer side.
    pub image: ImagePreview,
    pub timings: ImageTimings,
}

/// The request header for `bytes` decoded to at most `edge` pixels: write
/// it, then `bytes` unchanged. Refuses what the helper would refuse.
pub fn encode_image_request(
    format: ConfinedFormat,
    edge: u32,
    bytes: &[u8],
) -> Result<[u8; IMAGE_REQUEST_HEADER_BYTES]> {
    ensure!(
        (1..=MAX_HELPER_IMAGE_EDGE).contains(&edge),
        "Preview size must be between 1 and 1600 pixels"
    );
    ensure!(
        bytes.len() <= MAX_INPUT_BYTES,
        "Image exceeds the 32 MiB input limit"
    );
    if confined_image_format(bytes) != Some(format) {
        bail!("The image is not {}", format.name());
    }
    let mut header = [0; IMAGE_REQUEST_HEADER_BYTES];
    header[..4].copy_from_slice(&REQUEST_MAGIC);
    header[4..8].copy_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    header[8] = format.code();
    header[9..13].copy_from_slice(&edge.to_le_bytes());
    header[13..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    Ok(header)
}

/// The most bytes a valid reply to a request for `edge` can hold. A client may
/// stop reading past this many bytes.
pub fn max_image_reply_bytes(edge: u32) -> usize {
    let edge = edge.min(MAX_HELPER_IMAGE_EDGE) as usize;
    REPLY_PREFIX_BYTES + IMAGE_HEADER_BYTES + edge * edge * 4
}

/// Reads exactly one reply to a request for `format` at `edge`, then end of
/// input. The outer error means the reply is unreadable: truncated, malformed,
/// another format or an impossible kind or flag, a size other than
/// `scaled_dimensions` of its original at `edge`, or followed by more bytes.
/// Every size is checked before anything is allocated for it.
pub fn read_image_reply(
    mut input: impl Read,
    format: ConfinedFormat,
    edge: u32,
) -> io::Result<Result<ImageReply, ImageErrorKind>> {
    if !(1..=MAX_HELPER_IMAGE_EDGE).contains(&edge) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the requested edge is out of bounds",
        ));
    }
    let mut prefix = [0; REPLY_PREFIX_BYTES];
    input.read_exact(&mut prefix)?;
    if prefix[..4] != REPLY_MAGIC {
        return Err(unreadable("the reply has the wrong magic"));
    }
    if u32_at(&prefix, 4) != PROTOCOL_VERSION {
        return Err(unreadable("the reply has another protocol version"));
    }
    let reply = match prefix[8] {
        STATUS_IMAGE => Ok(read_image(&mut input, format, edge)?),
        STATUS_FAILED => {
            let mut failure = [0; FAILURE_BYTES];
            input.read_exact(&mut failure)?;
            let kind = ImageErrorKind::from_code(failure[0])
                .filter(|kind| kind.produced_by(format) && u32_at(&failure, 1) == 0)
                .ok_or_else(|| unreadable("the reply has an impossible error kind"))?;
            Err(kind)
        }
        _ => return Err(unreadable("the reply has an unknown status")),
    };
    if input.read(&mut [0])? != 0 {
        return Err(unreadable("the reply is followed by more bytes"));
    }
    Ok(reply)
}

fn read_image(input: &mut impl Read, format: ConfinedFormat, edge: u32) -> io::Result<ImageReply> {
    let mut header = [0; IMAGE_HEADER_BYTES];
    input.read_exact(&mut header)?;
    if ConfinedFormat::from_code(header[0]) != Some(format) {
        return Err(unreadable("the reply is for another format"));
    }
    let flags = ImageFlags::from_bits(u16::from_le_bytes([header[1], header[2]]), format)
        .ok_or_else(|| unreadable("the reply has an impossible flag"))?;
    let field = |index: usize| u32_at(&header, 3 + index * 4);
    let (original_width, original_height, width, height) = (field(0), field(1), field(2), field(3));
    if check_size(original_width, original_height).is_err()
        || (width, height) != scaled_dimensions(original_width, original_height, edge)
    {
        return Err(unreadable("the reply's image size is out of bounds"));
    }
    let timings = ImageTimings {
        parse_us: field(4),
        decode_us: field(5),
        convert_us: field(6),
    };
    // At most edge × edge × 4 by the checks above.
    let length = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|length| *length <= edge as usize * edge as usize * 4)
        .ok_or_else(|| unreadable("the reply's image size is out of bounds"))?;
    let mut rgba = vec![0; length];
    input.read_exact(&mut rgba)?;
    Ok(ImageReply {
        format,
        flags,
        image: ImagePreview {
            width,
            height,
            original_width,
            original_height,
            rgba,
            format: flags.caption(format),
        },
        timings,
    })
}

/// A decoded, oriented and scaled image with what the decoder disclosed.
pub(crate) struct Decoded {
    pub(crate) image: ImagePreview,
    pub(crate) flags: ImageFlags,
}

type Decode = fn(&[u8], u32, &mut ImageTimings) -> Result<Decoded, ImageErrorKind>;

/// Serves one image request whose magic [`crate::pdf_helper::serve`] already
/// read. Only I/O errors on `output` are returned.
pub(crate) fn serve(input: impl Read, output: impl Write) -> io::Result<()> {
    serve_with(input, output, |format| match format {
        ConfinedFormat::Avif => crate::avif::decode as Decode,
        ConfinedFormat::Jp2 | ConfinedFormat::J2k => crate::jpeg2000_hayro::decode,
    })
}

fn serve_with(
    mut input: impl Read,
    mut output: impl Write,
    decoder: impl FnOnce(ConfinedFormat) -> Decode,
) -> io::Result<()> {
    let reply = read_request(&mut input).and_then(|(format, edge, bytes)| {
        let decode = decoder(format);
        let mut timings = ImageTimings::default();
        // A panic anywhere in the decoders is that image's failure.
        let decoded = catch_unwind(AssertUnwindSafe(|| decode(&bytes, edge, &mut timings)))
            .unwrap_or(Err(ImageErrorKind::DecoderFailed))?;
        Ok((format, decoded, timings))
    });
    write_reply(&mut output, &reply)?;
    output.flush()
}

/// Refuses, in this order and before allocating the body: the frame header,
/// version, format, edge, length, then a prefix that is not the format.
fn read_request(input: &mut impl Read) -> Result<(ConfinedFormat, u32, Vec<u8>), ImageErrorKind> {
    let mut header = [0; IMAGE_REQUEST_HEADER_BYTES - 4];
    input
        .read_exact(&mut header)
        .map_err(|_| ImageErrorKind::Request)?;
    if u32_at(&header, 0) != PROTOCOL_VERSION {
        return Err(ImageErrorKind::Request);
    }
    let format = ConfinedFormat::from_code(header[4]).ok_or(ImageErrorKind::Request)?;
    let edge = u32_at(&header, 5);
    if !(1..=MAX_HELPER_IMAGE_EDGE).contains(&edge) {
        return Err(ImageErrorKind::Request);
    }
    let length = u32_at(&header, 9) as usize;
    if length > MAX_INPUT_BYTES {
        return Err(ImageErrorKind::InputTooLarge);
    }
    let mut prefix = [0; SNIFF_BYTES];
    let prefix = &mut prefix[..length.min(SNIFF_BYTES)];
    input
        .read_exact(prefix)
        .map_err(|_| ImageErrorKind::Request)?;
    if confined_image_format(prefix) != Some(format) {
        return Err(ImageErrorKind::FormatMismatch);
    }
    // A zeroed buffer of the validated length, filled by plain reads only.
    let mut bytes = vec![0; length];
    bytes[..prefix.len()].copy_from_slice(prefix);
    input
        .read_exact(&mut bytes[prefix.len()..])
        .map_err(|_| ImageErrorKind::Request)?;
    Ok((format, edge, bytes))
}

fn write_reply(
    output: &mut impl Write,
    reply: &Result<(ConfinedFormat, Decoded, ImageTimings), ImageErrorKind>,
) -> io::Result<()> {
    let mut header = Vec::with_capacity(REPLY_PREFIX_BYTES + IMAGE_HEADER_BYTES);
    header.extend_from_slice(&REPLY_MAGIC);
    header.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    let (format, decoded, timings) = match reply {
        Err(kind) => {
            header.push(STATUS_FAILED);
            header.push(kind.code());
            header.extend_from_slice(&0u32.to_le_bytes());
            return output.write_all(&header);
        }
        Ok(reply) => reply,
    };
    let image = &decoded.image;
    header.push(STATUS_IMAGE);
    header.push(format.code());
    header.extend_from_slice(&decoded.flags.bits().to_le_bytes());
    for value in [
        image.original_width,
        image.original_height,
        image.width,
        image.height,
        timings.parse_us,
        timings.decode_us,
        timings.convert_us,
    ] {
        header.extend_from_slice(&value.to_le_bytes());
    }
    output.write_all(&header)?;
    output.write_all(&image.rgba)
}

/// `check_dimensions` as error kinds, in the same order.
pub(crate) fn check_size(width: u32, height: u32) -> Result<(), ImageErrorKind> {
    if width == 0 || height == 0 {
        Err(ImageErrorKind::InvalidDimensions)
    } else if width > MAX_SOURCE_EDGE || height > MAX_SOURCE_EDGE {
        Err(ImageErrorKind::EdgeLimit)
    } else if u64::from(width) * u64::from(height) > MAX_SOURCE_PIXELS {
        Err(ImageErrorKind::PixelLimit)
    } else {
        Ok(())
    }
}

/// Refuses a straight RGBA buffer of `width` × `height` above the decoded limit.
pub(crate) fn check_rgba_budget(width: u32, height: u32) -> Result<(), ImageErrorKind> {
    if u64::from(width) * u64::from(height) * 4 > MAX_DECODER_BYTES {
        Err(ImageErrorKind::DecodedTooLarge)
    } else {
        Ok(())
    }
}

/// Scales straight RGBA, decoded at `original` or a reduced level of it, to
/// exactly `scaled_dimensions(original, edge)`, interpolating premultiplied
/// samples as `decode_raster` does.
pub(crate) fn finish(
    mut rgba: RgbaImage,
    (original_width, original_height): (u32, u32),
    edge: u32,
    flags: ImageFlags,
) -> Decoded {
    let (width, height) = scaled_dimensions(original_width, original_height, edge);
    if rgba.dimensions() != (width, height) {
        premultiply(rgba.as_mut());
        rgba = image::imageops::resize(&rgba, width, height, image::imageops::FilterType::Triangle);
        unpremultiply(rgba.as_mut());
    }
    Decoded {
        image: ImagePreview {
            width,
            height,
            original_width,
            original_height,
            rgba: rgba.into_raw(),
            format: String::new(),
        },
        flags,
    }
}

/// Microseconds since `start`, saturating.
pub(crate) fn elapsed_us(start: Instant) -> u32 {
    u32::try_from(start.elapsed().as_micros()).unwrap_or(u32::MAX)
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    let mut value = [0; 4];
    value.copy_from_slice(&bytes[offset..offset + 4]);
    u32::from_le_bytes(value)
}

fn unreadable(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const RGB_JP2: &[u8] = include_bytes!("../tests/fixtures/jpeg2000-rgb.jp2");

    pub(crate) fn request(format: ConfinedFormat, edge: u32, bytes: &[u8]) -> Vec<u8> {
        let mut request = encode_image_request(format, edge, bytes).unwrap().to_vec();
        request.extend_from_slice(bytes);
        request
    }

    /// Serves `bytes` through the helper's single entry, as the app will.
    pub(crate) fn decoded(
        format: ConfinedFormat,
        edge: u32,
        bytes: &[u8],
    ) -> Result<ImageReply, ImageErrorKind> {
        let mut reply = Vec::new();
        crate::pdf_helper::serve(&request(format, edge, bytes)[..], &mut reply).unwrap();
        assert!(reply.len() <= max_image_reply_bytes(edge));
        read_image_reply(&reply[..], format, edge).unwrap()
    }

    /// The RGBA pixel at `(x, y)`.
    pub(crate) fn pixel(image: &ImagePreview, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * image.width + x) * 4) as usize;
        image.rgba[at..at + 4].try_into().unwrap()
    }

    /// Each channel within `tolerance` of `expected`.
    pub(crate) fn assert_near(actual: [u8; 4], expected: [u8; 4], tolerance: u8, case: &str) {
        for (a, e) in actual.iter().zip(expected) {
            assert!(
                a.abs_diff(e) <= tolerance,
                "{case}: {actual:?} is not within {tolerance} of {expected:?}"
            );
        }
    }

    /// Yields its bytes, then fails: a reader that reaches the body fails
    /// with an error the caller cannot mistake for a bounds refusal.
    struct HeaderOnly(io::Cursor<Vec<u8>>);

    impl Read for HeaderOnly {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            match self.0.read(buffer)? {
                0 => Err(io::Error::other("read past the header")),
                read => Ok(read),
            }
        }
    }

    fn image_reply(format: ConfinedFormat, flags: ImageFlags, size: (u32, u32)) -> Vec<u8> {
        let mut reply = Vec::new();
        let decoded = Decoded {
            image: ImagePreview {
                width: size.0,
                height: size.1,
                original_width: size.0,
                original_height: size.1,
                rgba: vec![9; size.0 as usize * size.1 as usize * 4],
                format: String::new(),
            },
            flags,
        };
        let timings = ImageTimings {
            parse_us: 1,
            decode_us: 2,
            convert_us: 3,
        };
        write_reply(&mut reply, &Ok((format, decoded, timings))).unwrap();
        reply
    }

    fn failure_reply(kind: ImageErrorKind) -> Vec<u8> {
        let mut reply = Vec::new();
        write_reply(&mut reply, &Err(kind)).unwrap();
        reply
    }

    #[test]
    fn round_trips_an_image_through_the_helper() {
        let reply = decoded(ConfinedFormat::Jp2, 32, RGB_JP2).unwrap();
        assert_eq!(reply.format, ConfinedFormat::Jp2);
        assert_eq!(reply.flags, ImageFlags::default());
        assert_eq!(
            (reply.image.original_width, reply.image.original_height),
            (64, 48)
        );
        assert_eq!((reply.image.width, reply.image.height), (32, 24));
        assert_eq!(reply.image.format, "JPEG 2000 / JP2 · first image");
        assert_near(pixel(&reply.image, 8, 6), [220, 30, 30, 255], 4, "red");
    }

    #[test]
    fn the_largest_reply_fits_its_cap() {
        let reply = image_reply(ConfinedFormat::Avif, ImageFlags::default(), (1600, 1600));
        assert_eq!(reply.len(), max_image_reply_bytes(1600));
        assert_eq!(max_image_reply_bytes(1600), 9 + 31 + 10_240_000);
        assert_eq!(max_image_reply_bytes(1000), 9 + 31 + 4_000_000);
        let read = read_image_reply(&reply[..], ConfinedFormat::Avif, 1600)
            .unwrap()
            .unwrap();
        assert_eq!(read.image.rgba.len(), 10_240_000);
        assert_eq!(
            read.timings,
            ImageTimings {
                parse_us: 1,
                decode_us: 2,
                convert_us: 3
            }
        );
    }

    #[test]
    fn every_error_kind_survives_the_reply() {
        // A new kind fails to compile here until it joins `ALL`.
        for kind in ImageErrorKind::ALL {
            match kind {
                ImageErrorKind::Request
                | ImageErrorKind::InputTooLarge
                | ImageErrorKind::FormatMismatch
                | ImageErrorKind::InvalidDimensions
                | ImageErrorKind::EdgeLimit
                | ImageErrorKind::PixelLimit
                | ImageErrorKind::DecodedTooLarge
                | ImageErrorKind::DecoderFailed
                | ImageErrorKind::AvifUnreadable
                | ImageErrorKind::AvifGrid
                | ImageErrorKind::AvifNoStill
                | ImageErrorKind::AvifSizeMismatch
                | ImageErrorKind::Dav1dTooOld
                | ImageErrorKind::Jpeg2000Unreadable
                | ImageErrorKind::Jpeg2000TooManyTiles
                | ImageErrorKind::Jpeg2000TooManyComponents
                | ImageErrorKind::Jpeg2000TooDeep
                | ImageErrorKind::JpxNotJp2Compatible => {}
            }
            let reply = failure_reply(kind);
            for format in [
                ConfinedFormat::Avif,
                ConfinedFormat::Jp2,
                ConfinedFormat::J2k,
            ] {
                let read = read_image_reply(&reply[..], format, 100);
                if kind.produced_by(format) {
                    assert_eq!(read.unwrap().unwrap_err(), kind);
                } else {
                    // A kind the format cannot produce is unreadable.
                    assert_eq!(read.unwrap_err().kind(), io::ErrorKind::InvalidData);
                }
            }
        }
        assert!(ImageErrorKind::JpxNotJp2Compatible.produced_by(ConfinedFormat::Jp2));
        assert!(!ImageErrorKind::JpxNotJp2Compatible.produced_by(ConfinedFormat::J2k));
        assert!(!ImageErrorKind::AvifGrid.produced_by(ConfinedFormat::Jp2));
        assert!(!ImageErrorKind::Jpeg2000TooDeep.produced_by(ConfinedFormat::Avif));
    }

    #[test]
    fn encode_image_request_applies_the_request_bounds() {
        assert!(encode_image_request(ConfinedFormat::Jp2, 0, RGB_JP2).is_err());
        assert!(encode_image_request(ConfinedFormat::Jp2, 1601, RGB_JP2).is_err());
        assert!(encode_image_request(ConfinedFormat::J2k, 100, RGB_JP2).is_err());
        assert!(encode_image_request(ConfinedFormat::Avif, 100, RGB_JP2).is_err());
        let heif = b"\0\0\0\x18ftypheic\0\0\0\0mif1heic";
        assert!(encode_image_request(ConfinedFormat::Avif, 100, heif).is_err());
        let mut large = RGB_JP2.to_vec();
        large.resize(MAX_INPUT_BYTES + 1, 0);
        assert!(encode_image_request(ConfinedFormat::Jp2, 100, &large).is_err());
        let header = encode_image_request(ConfinedFormat::Jp2, 1600, RGB_JP2).unwrap();
        assert_eq!(&header[..4], b"GTIq");
        assert_eq!(u32_at(&header, 4), 2);
        assert_eq!(header[8], 2);
        assert_eq!(u32_at(&header, 9), 1600);
        assert_eq!(u32_at(&header, 13) as usize, RGB_JP2.len());
    }

    #[test]
    fn serve_refuses_image_requests_in_order_before_reading_their_body() {
        let valid = request(ConfinedFormat::Jp2, 100, RGB_JP2);
        let edited = |edits: &[(usize, &[u8])]| {
            let mut header = valid[..IMAGE_REQUEST_HEADER_BYTES].to_vec();
            for (offset, bytes) in edits {
                header[*offset..*offset + bytes.len()].copy_from_slice(bytes);
            }
            header
        };
        let refusal = |bytes: Vec<u8>| {
            let mut reply = Vec::new();
            crate::pdf_helper::serve(HeaderOnly(io::Cursor::new(bytes)), &mut reply).unwrap();
            read_image_reply(&reply[..], ConfinedFormat::Jp2, 100)
                .unwrap()
                .unwrap_err()
        };
        let version: &[u8] = &3u32.to_le_bytes();
        let format: &[u8] = &[4];
        let no_format: &[u8] = &[0];
        let no_edge: &[u8] = &0u32.to_le_bytes();
        let wide_edge: &[u8] = &1601u32.to_le_bytes();
        let too_long: &[u8] = &(MAX_INPUT_BYTES as u32 + 1).to_le_bytes();
        let longest: &[u8] = &u32::MAX.to_le_bytes();
        for (case, edits, kind) in [
            ("version", vec![(4, version)], ImageErrorKind::Request),
            // Each check runs before every later one.
            (
                "version before the rest",
                vec![(4, version), (8, format), (9, no_edge), (13, too_long)],
                ImageErrorKind::Request,
            ),
            ("format", vec![(8, format)], ImageErrorKind::Request),
            ("no format", vec![(8, no_format)], ImageErrorKind::Request),
            (
                "format before edge and length",
                vec![(8, format), (9, no_edge), (13, too_long)],
                ImageErrorKind::Request,
            ),
            ("no edge", vec![(9, no_edge)], ImageErrorKind::Request),
            (
                "edge over 1,600",
                vec![(9, wide_edge)],
                ImageErrorKind::Request,
            ),
            (
                "edge before length",
                vec![(9, wide_edge), (13, too_long)],
                ImageErrorKind::Request,
            ),
            (
                "length",
                vec![(13, too_long)],
                ImageErrorKind::InputTooLarge,
            ),
            (
                "largest length",
                vec![(13, longest)],
                ImageErrorKind::InputTooLarge,
            ),
        ] {
            assert_eq!(refusal(edited(&edits)), kind, "{case}");
        }
        // The prefix sniff refuses a mismatch before the body is allocated:
        // only the header and 4,096 bytes are readable here.
        let mut j2k_as_jp2 = valid[..IMAGE_REQUEST_HEADER_BYTES].to_vec();
        j2k_as_jp2[8] = ConfinedFormat::J2k.code();
        j2k_as_jp2.extend_from_slice(&RGB_JP2[..RGB_JP2.len().min(SNIFF_BYTES)]);
        let mut reply = Vec::new();
        crate::pdf_helper::serve(HeaderOnly(io::Cursor::new(j2k_as_jp2)), &mut reply).unwrap();
        assert_eq!(
            read_image_reply(&reply[..], ConfinedFormat::J2k, 100)
                .unwrap()
                .unwrap_err(),
            ImageErrorKind::FormatMismatch
        );
        let heif = b"\0\0\0\x18ftypheic\0\0\0\0mif1heic".to_vec();
        let mut heif_request = valid[..8].to_vec();
        heif_request.push(ConfinedFormat::Avif.code());
        heif_request.extend_from_slice(&100u32.to_le_bytes());
        heif_request.extend_from_slice(&(heif.len() as u32).to_le_bytes());
        heif_request.extend_from_slice(&heif);
        let mut reply = Vec::new();
        crate::pdf_helper::serve(&heif_request[..], &mut reply).unwrap();
        assert_eq!(
            read_image_reply(&reply[..], ConfinedFormat::Avif, 100)
                .unwrap()
                .unwrap_err(),
            ImageErrorKind::FormatMismatch
        );
        // Truncated headers and bodies.
        for cut in [5, 10, IMAGE_REQUEST_HEADER_BYTES + 3, valid.len() - 1] {
            let mut reply = Vec::new();
            crate::pdf_helper::serve(&valid[..cut], &mut reply).unwrap();
            assert_eq!(
                read_image_reply(&reply[..], ConfinedFormat::Jp2, 100)
                    .unwrap()
                    .unwrap_err(),
                ImageErrorKind::Request,
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn read_image_reply_refuses_impossible_frames_before_allocating() {
        let flags = ImageFlags::default();
        let valid = image_reply(ConfinedFormat::Jp2, flags, (40, 20));
        let headers = REPLY_PREFIX_BYTES + IMAGE_HEADER_BYTES;
        let at = REPLY_PREFIX_BYTES;
        let set = |edits: &[(usize, &[u8])]| {
            let mut reply = valid.clone();
            for (offset, bytes) in edits {
                reply[*offset..*offset + bytes.len()].copy_from_slice(bytes);
            }
            reply
        };
        let le = |value: u32| value.to_le_bytes();
        let (w4000, w1, w40_000, w32_000, w100, w41, w0) = (
            le(4000),
            le(1),
            le(40_000),
            le(32_000),
            le(100),
            le(41),
            le(0),
        );
        for (case, frame, edge) in [
            ("magic", set(&[(0, b"X")]), 100),
            ("version", set(&[(4, &le(1))]), 100),
            ("status", set(&[(8, &[7])]), 100),
            (
                "wrong echo",
                set(&[(at, &[ConfinedFormat::J2k.code()])]),
                100,
            ),
            ("unknown echo", set(&[(at, &[9])]), 100),
            ("an AVIF-only flag on JP2", set(&[(at + 1, &[1, 0])]), 100),
            ("an unknown flag", set(&[(at + 1, &[0, 0x80])]), 100),
            ("output larger than scaled", set(&[(at + 11, &w41)]), 100),
            // The frame is consistent with edge 100, not with this request's 30.
            ("another edge's size", valid.clone(), 30),
            ("zero original", set(&[(at + 3, &w0), (at + 11, &w0)]), 100),
            (
                "original over the edge limit",
                set(&[
                    (at + 3, &w40_000),
                    (at + 7, &w1),
                    (at + 11, &w100),
                    (at + 15, &w1),
                ]),
                100,
            ),
            (
                "original over the pixel limit",
                set(&[
                    (at + 3, &w32_000),
                    (at + 7, &w32_000),
                    (at + 11, &w100),
                    (at + 15, &w100),
                ]),
                100,
            ),
            (
                "output over the edge",
                set(&[(at + 3, &w4000), (at + 11, &w4000)]),
                100,
            ),
        ] {
            let error = read_image_reply(
                HeaderOnly(io::Cursor::new(frame[..headers].to_vec())),
                ConfinedFormat::Jp2,
                edge,
            )
            .map(|_| ())
            .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{case}: {error}");
        }
        let avif = image_reply(
            ConfinedFormat::Avif,
            ImageFlags {
                animation: true,
                crop_not_applied: true,
                hdr_not_tone_mapped: true,
                profile_not_applied: true,
            },
            (40, 20),
        );
        let read = read_image_reply(&avif[..], ConfinedFormat::Avif, 100)
            .unwrap()
            .unwrap();
        assert_eq!(
            read.image.format,
            "AVIF · first frame of an animation · crop not applied · HDR not tone-mapped · color profile not applied"
        );
        let mut trailing = valid.clone();
        trailing.push(0);
        let mut detail = failure_reply(ImageErrorKind::EdgeLimit);
        detail[REPLY_PREFIX_BYTES + 1] = 1;
        let mut unknown = failure_reply(ImageErrorKind::EdgeLimit);
        unknown[REPLY_PREFIX_BYTES] = 99;
        let mut zero = failure_reply(ImageErrorKind::EdgeLimit);
        zero[REPLY_PREFIX_BYTES] = 0;
        let mut pdf = failure_reply(ImageErrorKind::Request);
        pdf[..4].copy_from_slice(b"GTPr");
        for (case, frame) in [
            ("trailing bytes", trailing),
            ("detail on a kind", detail),
            ("unknown kind", unknown),
            ("kind zero", zero),
            ("a PDF reply", pdf),
        ] {
            let error = read_image_reply(&frame[..], ConfinedFormat::Jp2, 100)
                .map(|_| ())
                .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{case}: {error}");
        }
        for cut in [3, REPLY_PREFIX_BYTES + 2, headers + 1, valid.len() - 1] {
            let error = read_image_reply(&valid[..cut], ConfinedFormat::Jp2, 100)
                .map(|_| ())
                .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof, "cut at {cut}");
        }
        assert!(read_image_reply(&valid[..], ConfinedFormat::Jp2, 0).is_err());
        assert!(read_image_reply(&valid[..], ConfinedFormat::Jp2, 1601).is_err());
        assert!(
            read_image_reply(&valid[..], ConfinedFormat::Jp2, 100)
                .unwrap()
                .is_ok()
        );
    }

    #[test]
    fn a_decoder_panic_becomes_an_error_kind() {
        let mut reply = Vec::new();
        let panics: Decode = |_, _, _| panic!("injected decoder panic");
        serve_with(
            &request(ConfinedFormat::Jp2, 100, RGB_JP2)[4..],
            &mut reply,
            |_| panics,
        )
        .unwrap();
        assert_eq!(
            read_image_reply(&reply[..], ConfinedFormat::Jp2, 100)
                .unwrap()
                .unwrap_err(),
            ImageErrorKind::DecoderFailed
        );
    }
}
