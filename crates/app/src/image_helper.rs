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
//! The worker's image sides call [`decode`]; a Markdown document's local
//! images go through [`DocumentImages`], which stops sending a document's
//! confined images to a helper once one of them timed out or stopped, or
//! once their helpers have run for one deadline in total (requirement 17).
//!
//! The module depends only on std, `anyhow`, `gitturtle-preview` and
//! `crate::pdf_helper`: `tests/image_helper.rs` compiles both as its own
//! modules.

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
use std::fmt;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
use std::time::Duration;

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
        with_client(bytes, |client| {
            decode_with(client, bytes, name, edge, check)
        })
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
    let Some(format) = gitturtle_preview::confined_image_format(bytes) else {
        return gitturtle_preview::decode_image(bytes, name, edge);
    };
    client
        .decode(bytes, format, edge, check)
        .map(|reply| reply.image)
        .map_err(worded)
}

/// What a document's later confined images say once [`DocumentImages`]
/// sends no more of them to a helper, for either of its two reasons.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub(crate) const AFTER_EARLIER_IMAGES: &str = "Earlier images in this document stopped the image decoder or used its 10 seconds, so this one was not decoded.";

/// One document's images, decoded in turn as [`decode`] decodes each. The
/// document's later confined images are not sent to a helper, and each
/// fails with `AFTER_EARLIER_IMAGES`, once a confined image ends in
/// `Failure::TimedOut` or `Failure::Stopped` (a document whose image runs
/// past the deadline or kills its helper is broken or hostile), or once the
/// document's helpers have run for one deadline in total, whatever their
/// results: each request that starts finds less than one deadline spent and
/// runs for at most one more and the tick that kills it, so a document holds
/// one of the two helper slots for about two deadlines at most, with no
/// budget to tune (requirement 17). Only a helper's own run counts, from its
/// start until it is reaped, never the wait for a slot. Other images decode
/// as before. Only a flag and a sum: nothing is scanned.
#[derive(Default)]
pub(crate) struct DocumentImages {
    /// A confined image of this document timed out or stopped its helper.
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    stopped: bool,
    /// How long this document's helpers have run in total.
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    ran: Duration,
}

impl DocumentImages {
    /// The document's next image, as [`decode`] decodes it.
    pub(crate) fn decode(
        &mut self,
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
            with_client(bytes, |client| {
                self.decode_with(client, bytes, name, edge, check)
            })
        }
        #[cfg(not(all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )))]
        {
            decode(bytes, name, edge, check)
        }
    }

    /// [`Self::decode`] through `client` for the confined formats.
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    pub(crate) fn decode_with(
        &mut self,
        client: &Client<'_>,
        bytes: &[u8],
        name: &str,
        edge: u32,
        check: &dyn Fn() -> anyhow::Result<()>,
    ) -> anyhow::Result<ImagePreview> {
        let Some(format) = gitturtle_preview::confined_image_format(bytes) else {
            return gitturtle_preview::decode_image(bytes, name, edge);
        };
        anyhow::ensure!(
            !self.stopped && self.ran < client.runner.deadline(),
            AFTER_EARLIER_IMAGES
        );
        let (result, ran) = client.decode_timed(bytes, format, edge, check);
        self.ran += ran;
        result.map(|reply| reply.image).map_err(|failure| {
            self.stopped |= matches!(failure, Failure::TimedOut | Failure::Stopped { .. });
            worded(failure)
        })
    }
}

/// `use_client` with the client that decodes `bytes`: this executable as the
/// confined helper, with the app's two helper slots. In tests, the client a
/// test named for these bytes (`named::name`), or else one that cannot
/// start, so that no test runs its own test binary as a helper.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn with_client<T>(bytes: &[u8], use_client: impl FnOnce(&Client<'_>) -> T) -> T {
    #[cfg(test)]
    {
        use_client(&named::client(bytes))
    }
    #[cfg(not(test))]
    {
        let _ = bytes;
        use_client(&Client::new())
    }
}

/// The helpers tests name for images' bytes, as `pdf_view`'s tests name the
/// PDF helper for a document's.
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub(crate) mod named {
    use std::sync::{Arc, Mutex, OnceLock, PoisonError};
    use std::time::Duration;

    use super::Client;
    use crate::pdf_helper::{DEADLINE, HelperCommand, Runner, Slots};

    /// The clients named so far, the latest last.
    static NAMED: Mutex<Vec<(Vec<u8>, Arc<Client<'static>>)>> = Mutex::new(Vec::new());

    /// From now on, `bytes` decode through `command`, stopped at `deadline`,
    /// with two helper slots of their own.
    pub(crate) fn name(bytes: &[u8], command: HelperCommand, deadline: Duration) {
        let slots: &'static Slots = Box::leak(Box::new(Slots::new(2)));
        let client = Client::with(Runner::with(command, deadline, slots));
        NAMED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((bytes.to_vec(), Arc::new(client)));
    }

    /// The client named last for `bytes`, or else one whose program does not
    /// exist, which fails at once with "The image decoder could not start."
    pub(super) fn client(bytes: &[u8]) -> Arc<Client<'static>> {
        if let Some((_, client)) = NAMED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .rev()
            .find(|(named, _)| named == bytes)
        {
            return client.clone();
        }
        static SLOTS: Slots = Slots::new(2);
        static UNSTARTABLE: OnceLock<Arc<Client<'static>>> = OnceLock::new();
        UNSTARTABLE
            .get_or_init(|| {
                Arc::new(Client::with(Runner::with(
                    HelperCommand::new("/nonexistent/gitturtle-image-helper", [""; 0]),
                    DEADLINE,
                    &SLOTS,
                )))
            })
            .clone()
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
    #[cfg_attr(
        test,
        allow(
            dead_code,
            reason = "the app's tests decode through named helpers; tests/image_helper.rs checks this one"
        )
    )]
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
    #[allow(dead_code, reason = "only tests/image_helper.rs reads it")]
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
        self.decode_timed(bytes, format, edge, check).0
    }

    /// [`Self::decode`], and how long its helper ran ([`Runner::run_timed`]):
    /// zero when it refused the request or no helper started.
    fn decode_timed(
        &self,
        bytes: &[u8],
        format: ConfinedFormat,
        edge: u32,
        check: &dyn Fn() -> anyhow::Result<()>,
    ) -> (Result<ImageReply, Failure>, Duration) {
        let edge = edge.min(MAX_HELPER_IMAGE_EDGE);
        let header = match encode_image_request(format, edge, bytes) {
            Ok(header) => header,
            Err(error) => return (Err(Failure::Refused(error)), Duration::ZERO),
        };
        let cap = max_image_reply_bytes(edge);
        let (outcome, ran) = self.runner.run_timed([&header, bytes], cap, check);
        (classify(outcome, format, edge), ran)
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
                Kind::Jpeg2000TooManyTiles => {
                    "This JPEG 2000 image declares more than 65,535 tiles"
                }
                Kind::Jpeg2000TooManyComponents => {
                    "JPEG 2000 images with more than four components are not decoded on Linux"
                }
                Kind::Jpeg2000TooDeep => {
                    "JPEG 2000 samples deeper than 16 bits are not decoded on Linux"
                }
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
#[allow(dead_code, reason = "only tests/image_helper.rs calls it")]
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
