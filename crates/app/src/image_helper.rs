//! Preview images for the app. On Linux x86-64 and AArch64, [`decode`] sends
//! the formats `gitturtle_preview::confined_image_format` names (AVIF, JP2 and
//! raw J2K codestreams) to GitTurtle's confined helper process and every other
//! image to `gitturtle_preview::decode_image`, which is the only decoder on
//! other targets. A confined format never falls back to an in-process decode,
//! whatever the helper's failure: `decode_image` refuses it there too.
//!
//! [`Client`] runs one helper per image through [`crate::pdf_helper::Runner`],
//! so images share the PDF pages' helper, deadline and two slots, with a reply
//! cap computed from the requested edge. Its [`Failure`] wording is fixed:
//! nothing the helper sends reaches it but a validated error kind. The contract
//! is requirement 14 of the
//! [architecture review](../../../docs/development/2026-11-02-linux-avif-jpeg2000-architecture.md#7-security-review-of-the-design-2026-11-02).
//!
//! The module depends only on std, `anyhow`, `gitturtle-preview` and
//! `crate::pdf_helper`: `tests/image_helper.rs` compiles both as its own
//! modules.

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
use std::fmt;

use gitturtle_preview::ImagePreview;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
use gitturtle_preview::pdf_helper::{
    ConfinedFormat, ImageErrorKind, ImageReply, MAX_HELPER_IMAGE_EDGE, encode_image_request,
    max_image_reply_bytes, read_image_reply,
};

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
use crate::pdf_helper::{Outcome, Runner};

/// `bytes`, named `name`, decoded to at most `edge` pixels on its longer side,
/// capped at 1,600 for a confined format on Linux (`Client::decode`), with
/// errors worded for a person. On Linux a confined format runs in a new
/// helper on the calling thread, which must outlive the call (the helper's
/// parent-death signal follows it); `check` runs at least every 25 ms while
/// it waits for a slot or the helper, and a failed check kills the helper and
/// returns the check's own error.
pub(crate) fn decode(
    bytes: &[u8],
    name: &str,
    edge: u32,
    check: &dyn Fn() -> anyhow::Result<()>,
) -> anyhow::Result<ImagePreview> {
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        decode_with(&Client::new(), bytes, name, edge, check)
    }
    #[cfg(not(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    {
        let _ = check;
        gitturtle_preview::decode_image(bytes, name, edge)
    }
}

/// [`decode`] through `client` for the confined formats.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub(crate) fn decode_with(
    client: &Client<'_>,
    bytes: &[u8],
    name: &str,
    edge: u32,
    check: &dyn Fn() -> anyhow::Result<()>,
) -> anyhow::Result<ImagePreview> {
    match gitturtle_preview::confined_image_format(bytes) {
        Some(format) => client
            .decode(bytes, format, edge, check)
            .map(|reply| reply.image)
            .map_err(worded),
        None => gitturtle_preview::decode_image(bytes, name, edge),
    }
}

/// `failure` in the words a person reads, which only `Failure`'s `Display`
/// gives: its `Debug` form carries error kinds and signals and never reaches
/// a view, a log or a label. A cancelled request keeps its check's own error.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn worded(failure: Failure) -> anyhow::Error {
    match failure {
        Failure::Cancelled(error) => error,
        failure => anyhow::anyhow!("{failure}"),
    }
}

/// Decodes AVIF and JPEG 2000 images in confined helper processes, one per
/// request.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub(crate) struct Client<'a> {
    runner: Runner<'a>,
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
impl Client<'static> {
    /// This executable as the helper, with the 10 s deadline and the two
    /// helper slots the app's PDF pages also use.
    pub(crate) fn new() -> Self {
        Self {
            runner: Runner::new(),
        }
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
impl<'a> Client<'a> {
    /// A client for tests, on another runner.
    #[cfg(test)]
    pub(crate) fn with(runner: Runner<'a>) -> Self {
        Self { runner }
    }

    /// The runner a client uses.
    #[cfg(test)]
    pub(crate) fn runner(&self) -> &Runner<'a> {
        &self.runner
    }

    /// `bytes`, which are `format`, decoded to at most `edge` pixels by a new
    /// helper through [`Runner::run`]. An `edge` past the helper's 1,600
    /// pixels asks for 1,600, as `decode_image` caps its own bound silently;
    /// that one edge is the request's, the reply's cap and the edge the reply
    /// is checked against. The reply may hold at most an `edge` × `edge`
    /// image, so a longer one is killed as it arrives. An edge of 0 is
    /// refused, as `decode_image` refuses it.
    pub(crate) fn decode(
        &self,
        bytes: &[u8],
        format: ConfinedFormat,
        edge: u32,
        check: &dyn Fn() -> anyhow::Result<()>,
    ) -> Result<ImageReply, Failure> {
        let edge = edge.min(MAX_HELPER_IMAGE_EDGE);
        let header = encode_image_request(format, edge, bytes).map_err(Failure::Refused)?;
        let cap = max_image_reply_bytes(edge);
        classify(self.runner.run([&header, bytes], cap, check), format, edge)
    }
}

/// Why an image request returned no image.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[derive(Debug)]
pub(crate) enum Failure {
    /// The caller's check failed; a helper that had started was killed and
    /// reaped first.
    Cancelled(anyhow::Error),
    /// The request exceeds a bound; no helper started.
    Refused(anyhow::Error),
    /// The helper reported this error for the image.
    Document(ImageErrorKind),
    /// The helper ran past the deadline, its CPU limit or its own alarm.
    TimedOut,
    /// The helper died on `signal`, or exited without a valid reply. The
    /// signal is for tests and diagnostics, never wording.
    Stopped {
        #[allow(
            dead_code,
            reason = "never worded; only tests/image_helper.rs reads which signal ended the helper"
        )]
        signal: Option<i32>,
    },
    /// The reply was malformed, for another frame or format, out of bounds,
    /// larger than the request's cap or followed by more bytes.
    Unreadable,
    /// The helper could not be started or could not confine itself.
    CouldNotStart,
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ImageErrorKind as Kind;
        const STOPPED: &str = "The image decoder stopped on this image.";
        const UNREADABLE: &str = "The image decoder returned an unreadable result.";
        let sentence = match self {
            Self::Cancelled(error) | Self::Refused(error) => return write!(formatter, "{error}"),
            Self::Document(kind) => match kind {
                Kind::InputTooLarge => "Image exceeds the 32 MiB input limit",
                Kind::InvalidDimensions => "Image dimensions must be positive",
                Kind::EdgeLimit => "Image dimensions exceed the 32768-pixel edge limit",
                Kind::PixelLimit => "Image exceeds the 32-million-pixel limit",
                Kind::DecodedTooLarge => "Decoded image exceeds the 256 MiB limit",
                Kind::DecoderFailed => STOPPED,
                Kind::AvifUnreadable => "Cannot read AVIF; corrupt or unsupported file",
                Kind::AvifGrid => "AVIF image grids are not decoded on Linux yet",
                Kind::AvifNoStill => "This AVIF animation has no still image",
                Kind::AvifSizeMismatch => "The AVIF image size disagrees with its header",
                Kind::Dav1dTooOld => "AVIF decoding needs libdav1d 1.4.1 or later",
                Kind::Jpeg2000Unreadable => {
                    "Cannot read JPEG 2000; corrupt or unsupported codestream"
                }
                Kind::Jpeg2000TooManyTiles => "JPEG 2000 declares more than 65,535 tiles",
                Kind::Jpeg2000TooManyComponents => {
                    "JPEG 2000 with more than four components is not shown"
                }
                Kind::Jpeg2000TooDeep => "JPEG 2000 samples deeper than 16 bits are not shown",
                Kind::JpxNotJp2Compatible => {
                    "This JPX file is not JP2-compatible and is not decoded on Linux"
                }
                // The client sent a well-formed request for the format it
                // sniffed, so these contradict it.
                Kind::Request | Kind::FormatMismatch => UNREADABLE,
            },
            Self::TimedOut => "The image took longer than 10 seconds to decode and was stopped.",
            Self::Stopped { .. } => STOPPED,
            Self::Unreadable => UNREADABLE,
            Self::CouldNotStart => "The image decoder could not start.",
        };
        formatter.write_str(sentence)
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
impl std::error::Error for Failure {}

/// [`classify`] for a helper that ended with `status`, so tests can cover the
/// signals that would make a fake helper dump core.
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub(crate) fn classify_exit(
    status: std::process::ExitStatus,
    reply: &[u8],
    format: ConfinedFormat,
    edge: u32,
) -> Result<ImageReply, Failure> {
    classify(Outcome::exited(status, reply), format, edge)
}

/// Success needs exit status 0 and a valid image frame for the requested
/// format and edge; signals and exit codes map as they do for PDF pages.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn classify(outcome: Outcome, format: ConfinedFormat, edge: u32) -> Result<ImageReply, Failure> {
    let reply = match outcome {
        Outcome::Replied(reply) => reply,
        Outcome::Cancelled(error) => return Err(Failure::Cancelled(error)),
        Outcome::TimedOut => return Err(Failure::TimedOut),
        Outcome::Stopped { signal } => return Err(Failure::Stopped { signal }),
        Outcome::Oversized => return Err(Failure::Unreadable),
        Outcome::CouldNotStart => return Err(Failure::CouldNotStart),
    };
    match read_image_reply(&reply[..], format, edge) {
        Ok(Ok(image)) => Ok(image),
        Ok(Err(kind)) => Err(Failure::Document(kind)),
        Err(_) => Err(Failure::Unreadable),
    }
}
