//! The message format between GitTurtle and its confined Linux helper
//! process, and the helper's side of it.
//!
//! [`serve`] is the only way into hayro, `hayro-jpeg2000`, `mp4parse` and
//! dav1d: the app process never renders a PDF page or decodes an AVIF or JPEG
//! 2000 image itself. It starts a helper per request, writes
//! [`encode_request`]'s or [`encode_image_request`]'s header followed by the
//! bytes, and reads one bounded reply with [`read_reply`] or
//! [`read_image_reply`]. [`serve`] reads the 4-byte magic and dispatches before
//! allocating anything; the image frame is described with its reader. The
//! helper confines itself before calling [`serve`]; nothing here opens a file
//! or a socket. All integers are little-endian.
//!
//! PDF request: magic, version (u32), zero-based page (u32), length (u32), then the
//! PDF bytes. Reply: magic, version (u32), a status byte, then either an error
//! kind (u8) with its detail (u32), or a page: page, page count, original and
//! output size (u32 each), text state, text error kind and truncated flag (u8
//! each), text length (u32), parse, render and text times in microseconds (u32
//! each), exactly width × height × 4 straight RGBA bytes, then the UTF-8 text.

use std::io::{self, Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};

use anyhow::Result;

use crate::{
    ImagePreview, MAX_INPUT_BYTES, MAX_PDF_PAGE_TEXT_BYTES, MAX_PDF_PAGES, MAX_PDF_SEARCH_MATCHES,
    MAX_PDF_SEARCH_PAGES, MAX_PDF_SEARCH_QUERY_BYTES, MAX_PDF_SEARCH_SNIPPET_BYTES,
    PDF_PREVIEW_EDGE, PdfPageText, PdfSearchMatch, PdfSearchReply, check_dimensions,
    check_pdf_input, check_pdf_search_input, image_helper, pdf_hayro, scaled_dimensions,
};
pub use image_helper::{
    ConfinedFormat, IMAGE_REQUEST_HEADER_BYTES, ImageErrorKind, ImageFlags, ImageReply,
    ImageTimings, MAX_HELPER_IMAGE_EDGE, encode_image_request, max_image_reply_bytes,
    read_image_reply,
};

/// Changes whenever any frame changes; both sides refuse any other value.
/// Version 2 added the image frame; version 3 adds the PDF text-search frame (`GTPS`/`GTPR`).
pub const PROTOCOL_VERSION: u32 = 3;
/// The fixed request header [`encode_request`] returns.
pub const REQUEST_HEADER_BYTES: usize = 16;
/// The fixed prefix header for a PDF text-search request (`GTPS` + version + query_len + pdf_len).
pub const SEARCH_REQUEST_HEADER_BYTES: usize = 16;
/// The largest reply [`read_reply`] accepts: a 1,000 × 1,000 page and 256 KiB
/// of text, plus the headers. A client may stop reading past this many bytes.
pub const MAX_REPLY_BYTES: usize =
    REPLY_PREFIX_BYTES + PAGE_HEADER_BYTES + MAX_PIXEL_BYTES + MAX_PDF_PAGE_TEXT_BYTES;
/// Maximum byte length of a PDF search reply (`GTPR`).
pub const MAX_SEARCH_REPLY_BYTES: usize = REPLY_PREFIX_BYTES
    + SEARCH_HEADER_BYTES
    + MAX_PDF_SEARCH_MATCHES * (12 + MAX_PDF_SEARCH_SNIPPET_BYTES);

const REQUEST_MAGIC: [u8; 4] = *b"GTPq";
const REPLY_MAGIC: [u8; 4] = *b"GTPr";
const SEARCH_REQUEST_MAGIC: [u8; 4] = *b"GTPS";
const SEARCH_REPLY_MAGIC: [u8; 4] = *b"GTPR";
const PDF_SIGNATURE: &[u8; 5] = b"%PDF-";
const REPLY_PREFIX_BYTES: usize = 9;
const FAILURE_BYTES: usize = 5;
const PAGE_HEADER_BYTES: usize = 43;
const SEARCH_HEADER_BYTES: usize = 13;
const MAX_PIXEL_BYTES: usize = PDF_PREVIEW_EDGE as usize * PDF_PREVIEW_EDGE as usize * 4;
const STATUS_PAGE: u8 = 0;
const STATUS_FAILED: u8 = 1;
const STATUS_SEARCH: u8 = 2;
const TEXT_ABSENT: u8 = 0;
const TEXT_PRESENT: u8 = 1;
const TEXT_FAILED: u8 = 2;

/// Why the helper produced no page, or no text for a rendered page. The client
/// maps each kind to its own fixed wording; the helper sends no message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The request frame was malformed: wrong magic or version, or truncated.
    Request,
    /// The PDF exceeds the 32 MiB input limit.
    InputTooLarge,
    /// The bytes do not start with `%PDF-`.
    NotPdf,
    /// The page index is beyond the 100,000-page navigation bound.
    PageBound,
    /// The document cannot be read: corrupt, or an unsupported structure or encryption.
    Corrupt,
    /// The document needs a password to open.
    Locked,
    /// The document has no pages.
    NoPages,
    /// The document has more than 100,000 pages.
    TooManyPages,
    /// The page index is outside this document.
    PageOutOfRange { page_count: usize },
    /// The page's crop box has no usable size.
    InvalidPageSize,
    /// The page is wider or taller than the 32,768-pixel edge limit.
    PageEdgeLimit,
    /// The page is larger than the 32-million-pixel limit.
    PagePixelLimit,
    /// The renderer stopped on this page (a panic inside hayro).
    RendererFailed,
}

impl ErrorKind {
    fn code(self) -> (u8, u32) {
        match self {
            Self::Request => (1, 0),
            Self::InputTooLarge => (2, 0),
            Self::NotPdf => (3, 0),
            Self::PageBound => (4, 0),
            Self::Corrupt => (5, 0),
            Self::Locked => (6, 0),
            Self::NoPages => (7, 0),
            Self::TooManyPages => (8, 0),
            Self::PageOutOfRange { page_count } => (9, wire(page_count)),
            Self::InvalidPageSize => (10, 0),
            Self::PageEdgeLimit => (11, 0),
            Self::PagePixelLimit => (12, 0),
            Self::RendererFailed => (13, 0),
        }
    }

    fn from_code(code: u8, detail: u32) -> Option<Self> {
        let kind = match code {
            1 => Self::Request,
            2 => Self::InputTooLarge,
            3 => Self::NotPdf,
            4 => Self::PageBound,
            5 => Self::Corrupt,
            6 => Self::Locked,
            7 => Self::NoPages,
            8 => Self::TooManyPages,
            9 => {
                let page_count = detail as usize;
                return (1..=MAX_PDF_PAGES)
                    .contains(&page_count)
                    .then_some(Self::PageOutOfRange { page_count });
            }
            10 => Self::InvalidPageSize,
            11 => Self::PageEdgeLimit,
            12 => Self::PagePixelLimit,
            13 => Self::RendererFailed,
            _ => return None,
        };
        (detail == 0).then_some(kind)
    }
}

/// Microseconds the helper spent in each phase, saturating at `u32::MAX`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PhaseTimings {
    /// Opening the document and checking the selected page.
    pub parse_us: u32,
    /// Rasterizing the page.
    pub render_us: u32,
    /// Extracting the page's text.
    pub text_us: u32,
}

/// One rendered page with its text. Text failure keeps the pixels.
#[derive(Debug)]
pub struct PageReply {
    /// The zero-based page that was rendered.
    pub page: usize,
    pub page_count: usize,
    /// At most 1,000 pixels on its longer edge; `format` names the page.
    pub image: ImagePreview,
    pub text: Result<PdfPageText, ErrorKind>,
    pub timings: PhaseTimings,
}

/// The request header for zero-based `page` of `pdf`: write it, then `pdf`
/// unchanged. Refuses what the helper would refuse, with the same wording as
/// the other platforms.
pub fn encode_request(page: usize, pdf: &[u8]) -> Result<[u8; REQUEST_HEADER_BYTES]> {
    check_pdf_input(pdf, page)?;
    let mut header = [0; REQUEST_HEADER_BYTES];
    header[..4].copy_from_slice(&REQUEST_MAGIC);
    header[4..8].copy_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    header[8..12].copy_from_slice(&wire(page).to_le_bytes());
    header[12..].copy_from_slice(&wire(pdf.len()).to_le_bytes());
    Ok(header)
}

/// Reads exactly one reply and then end of input. The outer error means the
/// reply is unreadable (truncated, malformed, out of bounds or followed by more
/// bytes); every size is checked against the page and text bounds before
/// anything is allocated for it.
pub fn read_reply(mut input: impl Read) -> io::Result<Result<PageReply, ErrorKind>> {
    let mut prefix = [0; REPLY_PREFIX_BYTES];
    input.read_exact(&mut prefix)?;
    if prefix[..4] != REPLY_MAGIC {
        return Err(unreadable("the reply has the wrong magic"));
    }
    if u32_at(&prefix, 4) != PROTOCOL_VERSION {
        return Err(unreadable("the reply has another protocol version"));
    }
    let reply = match prefix[8] {
        STATUS_PAGE => Ok(read_page(&mut input)?),
        STATUS_FAILED => {
            let mut failure = [0; FAILURE_BYTES];
            input.read_exact(&mut failure)?;
            Err(ErrorKind::from_code(failure[0], u32_at(&failure, 1))
                .ok_or_else(|| unreadable("the reply has an unknown error kind"))?)
        }
        _ => return Err(unreadable("the reply has an unknown status")),
    };
    if input.read(&mut [0])? != 0 {
        return Err(unreadable("the reply is followed by more bytes"));
    }
    Ok(reply)
}

/// Maximum request frame size across PDF page, PDF search, and image requests.
pub const MAX_REQUEST_BYTES: usize =
    MAX_INPUT_BYTES + SEARCH_REQUEST_HEADER_BYTES + MAX_PDF_SEARCH_QUERY_BYTES;

/// Encode the header and UTF-8 query prefix for a confined PDF text-search request.
/// Write the returned bytes followed by `pdf` unchanged.
pub fn encode_search_request(query: &str, pdf: &[u8]) -> Result<Vec<u8>> {
    check_pdf_search_input(pdf, query)?;
    let trimmed = query.trim();
    let mut prefix = Vec::with_capacity(SEARCH_REQUEST_HEADER_BYTES + trimmed.len());
    prefix.extend_from_slice(&SEARCH_REQUEST_MAGIC);
    prefix.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    prefix.extend_from_slice(&wire(trimmed.len()).to_le_bytes());
    prefix.extend_from_slice(&wire(pdf.len()).to_le_bytes());
    prefix.extend_from_slice(trimmed.as_bytes());
    Ok(prefix)
}

/// Reads exactly one confined PDF text-search reply (`GTPR`) and then EOF.
pub fn read_search_reply(mut input: impl Read) -> io::Result<Result<PdfSearchReply, ErrorKind>> {
    let mut prefix = [0; REPLY_PREFIX_BYTES];
    input.read_exact(&mut prefix)?;
    if prefix[..4] != SEARCH_REPLY_MAGIC {
        return Err(unreadable("the search reply has the wrong magic"));
    }
    if u32_at(&prefix, 4) != PROTOCOL_VERSION {
        return Err(unreadable("the search reply has another protocol version"));
    }
    let reply = match prefix[8] {
        STATUS_SEARCH => Ok(read_search_body(&mut input)?),
        STATUS_FAILED => {
            let mut failure = [0; FAILURE_BYTES];
            input.read_exact(&mut failure)?;
            Err(ErrorKind::from_code(failure[0], u32_at(&failure, 1))
                .ok_or_else(|| unreadable("the search reply has an unknown error kind"))?)
        }
        _ => return Err(unreadable("the search reply has an unknown status")),
    };
    if input.read(&mut [0])? != 0 {
        return Err(unreadable("the search reply is followed by more bytes"));
    }
    Ok(reply)
}

fn read_search_body(input: &mut impl Read) -> io::Result<PdfSearchReply> {
    let mut header = [0; SEARCH_HEADER_BYTES];
    input.read_exact(&mut header)?;
    let page_count = u32_at(&header, 0) as usize;
    let pages_scanned = u32_at(&header, 4) as usize;
    let match_entries = u32_at(&header, 8) as usize;
    let truncated = header[12];
    if !(1..=MAX_PDF_PAGES).contains(&page_count)
        || pages_scanned > page_count
        || pages_scanned > MAX_PDF_SEARCH_PAGES
        || match_entries > MAX_PDF_SEARCH_MATCHES
        || truncated > 1
    {
        return Err(unreadable("the search reply header is out of bounds"));
    }
    let mut matches = Vec::with_capacity(match_entries);
    let mut last_page: Option<usize> = None;
    for _ in 0..match_entries {
        let mut entry = [0; 12];
        input.read_exact(&mut entry)?;
        let page = u32_at(&entry, 0) as usize;
        let match_count = u32_at(&entry, 4) as usize;
        let snippet_len = u32_at(&entry, 8) as usize;
        if page >= pages_scanned
            || last_page.is_some_and(|prev| page <= prev)
            || match_count == 0
            || snippet_len > MAX_PDF_SEARCH_SNIPPET_BYTES
        {
            return Err(unreadable("a search reply match entry is out of bounds"));
        }
        last_page = Some(page);
        let mut snippet_bytes = vec![0; snippet_len];
        input.read_exact(&mut snippet_bytes)?;
        let raw_snippet = String::from_utf8(snippet_bytes)
            .map_err(|_| unreadable("a search reply snippet is not UTF-8"))?;
        let snippet: String = raw_snippet
            .chars()
            .filter(|ch| {
                !ch.is_control()
                    && !matches!(
                        *ch,
                        '\u{202A}'..='\u{202E}'
                            | '\u{2066}'..='\u{2069}'
                            | '\u{200E}'
                            | '\u{200F}'
                            | '\u{061C}'
                    )
            })
            .collect();
        matches.push(PdfSearchMatch {
            page,
            match_count,
            snippet,
        });
    }
    Ok(PdfSearchReply {
        page_count,
        pages_scanned,
        truncated: truncated == 1,
        matches,
    })
}

fn write_search_reply(
    output: &mut impl Write,
    reply: &Result<PdfSearchReply, ErrorKind>,
) -> io::Result<()> {
    let mut buf = Vec::with_capacity(REPLY_PREFIX_BYTES + SEARCH_HEADER_BYTES);
    buf.extend_from_slice(&SEARCH_REPLY_MAGIC);
    buf.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    let body = match reply {
        Err(kind) => {
            let (code, detail) = kind.code();
            buf.push(STATUS_FAILED);
            buf.push(code);
            buf.extend_from_slice(&detail.to_le_bytes());
            return output.write_all(&buf);
        }
        Ok(body) => body,
    };
    buf.push(STATUS_SEARCH);
    buf.extend_from_slice(&wire(body.page_count).to_le_bytes());
    buf.extend_from_slice(&wire(body.pages_scanned).to_le_bytes());
    buf.extend_from_slice(&wire(body.matches.len()).to_le_bytes());
    buf.push(u8::from(body.truncated));
    for found in &body.matches {
        buf.extend_from_slice(&wire(found.page).to_le_bytes());
        buf.extend_from_slice(&wire(found.match_count).to_le_bytes());
        buf.extend_from_slice(&wire(found.snippet.len()).to_le_bytes());
        buf.extend_from_slice(found.snippet.as_bytes());
    }
    output.write_all(&buf)
}

struct SubSlice<B> {
    buffer: B,
    start: usize,
    len: usize,
}

impl<B: AsRef<[u8]>> AsRef<[u8]> for SubSlice<B> {
    fn as_ref(&self) -> &[u8] {
        &self.buffer.as_ref()[self.start..self.start + self.len]
    }
}

/// The helper's whole job: read one request from `input`, render its page and
/// text with hayro or decode its image, and write one reply to `output`. The
/// magic chooses the frame before anything is allocated; an unknown magic gets
/// the PDF frame's request failure. Only I/O errors on `output` are returned;
/// every other failure is an error reply.
pub fn serve(input: impl Read, output: impl Write) -> io::Result<()> {
    serve_with(input, output, pdf_hayro::prepare)
}

/// Zero-copy [`serve`] over an owned in-memory request buffer (such as a
/// read-only `mmap` of a fully sealed `memfd`), avoiding up to 32 MiB of pipe
/// copies and heap allocations inside the confined helper.
pub fn serve_buffer<B: AsRef<[u8]> + Send + Sync + 'static>(
    buffer: B,
    mut output: impl Write,
) -> io::Result<()> {
    let slice = buffer.as_ref();
    let Some(magic) = slice.get(..4) else {
        write_reply(&mut output, &Err(ErrorKind::Request))?;
        return output.flush();
    };
    if magic == image_helper::REQUEST_MAGIC {
        return image_helper::serve_slice(&slice[4..], output);
    }
    if magic == SEARCH_REQUEST_MAGIC {
        let reply = parse_pdf_search_buffer(slice).and_then(|(query, pdf_start, length)| {
            let data = std::sync::Arc::new(SubSlice {
                buffer,
                start: pdf_start,
                len: length,
            });
            let pdf = hayro::hayro_syntax::PdfData::from(data);
            catch_unwind(AssertUnwindSafe(|| pdf_hayro::search_data(pdf, &query)))
                .unwrap_or(Err(ErrorKind::RendererFailed))
        });
        write_search_reply(&mut output, &reply)?;
        return output.flush();
    }
    if magic != REQUEST_MAGIC {
        write_reply(&mut output, &Err(ErrorKind::Request))?;
        return output.flush();
    }
    let reply = parse_pdf_buffer_header(slice).and_then(|(page, length)| {
        let data = std::sync::Arc::new(SubSlice {
            buffer,
            start: REQUEST_HEADER_BYTES,
            len: length,
        });
        let pdf = hayro::hayro_syntax::PdfData::from(data);
        catch_unwind(AssertUnwindSafe(|| pdf_hayro::prepare_data(pdf, page)))
            .unwrap_or(Err(ErrorKind::RendererFailed))
    });
    write_reply(&mut output, &reply)?;
    output.flush()
}

fn parse_pdf_buffer_header(slice: &[u8]) -> Result<(usize, usize), ErrorKind> {
    let header = slice
        .get(4..REQUEST_HEADER_BYTES)
        .ok_or(ErrorKind::Request)?;
    if u32_at(header, 0) != PROTOCOL_VERSION {
        return Err(ErrorKind::Request);
    }
    let page = u32_at(header, 4) as usize;
    let length = u32_at(header, 8) as usize;
    if length > MAX_INPUT_BYTES {
        return Err(ErrorKind::InputTooLarge);
    }
    if page >= MAX_PDF_PAGES {
        return Err(ErrorKind::PageBound);
    }
    if length < PDF_SIGNATURE.len() {
        return Err(ErrorKind::NotPdf);
    }
    let body = slice
        .get(REQUEST_HEADER_BYTES..REQUEST_HEADER_BYTES + length)
        .ok_or(ErrorKind::Request)?;
    if !body.starts_with(PDF_SIGNATURE) {
        return Err(ErrorKind::NotPdf);
    }
    Ok((page, length))
}

fn parse_pdf_search_buffer(slice: &[u8]) -> Result<(String, usize, usize), ErrorKind> {
    let header = slice
        .get(4..SEARCH_REQUEST_HEADER_BYTES)
        .ok_or(ErrorKind::Request)?;
    if u32_at(header, 0) != PROTOCOL_VERSION {
        return Err(ErrorKind::Request);
    }
    let query_len = u32_at(header, 4) as usize;
    let pdf_len = u32_at(header, 8) as usize;
    if query_len == 0 || query_len > MAX_PDF_SEARCH_QUERY_BYTES {
        return Err(ErrorKind::Request);
    }
    if pdf_len > MAX_INPUT_BYTES {
        return Err(ErrorKind::InputTooLarge);
    }
    if pdf_len < PDF_SIGNATURE.len() {
        return Err(ErrorKind::NotPdf);
    }
    let query_end = SEARCH_REQUEST_HEADER_BYTES + query_len;
    let query_bytes = slice
        .get(SEARCH_REQUEST_HEADER_BYTES..query_end)
        .ok_or(ErrorKind::Request)?;
    let query = std::str::from_utf8(query_bytes)
        .map_err(|_| ErrorKind::Request)?
        .trim();
    if query.is_empty() || query.contains('\0') {
        return Err(ErrorKind::Request);
    }
    let body = slice
        .get(query_end..query_end + pdf_len)
        .ok_or(ErrorKind::Request)?;
    if !body.starts_with(PDF_SIGNATURE) {
        return Err(ErrorKind::NotPdf);
    }
    Ok((query.to_owned(), query_end, pdf_len))
}

fn read_search_request(input: &mut impl Read) -> Result<(String, Vec<u8>), ErrorKind> {
    let mut header = [0; SEARCH_REQUEST_HEADER_BYTES - 4];
    input
        .read_exact(&mut header)
        .map_err(|_| ErrorKind::Request)?;
    if u32_at(&header, 0) != PROTOCOL_VERSION {
        return Err(ErrorKind::Request);
    }
    let query_len = u32_at(&header, 4) as usize;
    let pdf_len = u32_at(&header, 8) as usize;
    if query_len == 0 || query_len > MAX_PDF_SEARCH_QUERY_BYTES {
        return Err(ErrorKind::Request);
    }
    if pdf_len > MAX_INPUT_BYTES {
        return Err(ErrorKind::InputTooLarge);
    }
    if pdf_len < PDF_SIGNATURE.len() {
        return Err(ErrorKind::NotPdf);
    }
    let mut query_bytes = vec![0; query_len];
    input
        .read_exact(&mut query_bytes)
        .map_err(|_| ErrorKind::Request)?;
    let query = String::from_utf8(query_bytes).map_err(|_| ErrorKind::Request)?;
    let trimmed = query.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return Err(ErrorKind::Request);
    }
    let mut signature = [0; PDF_SIGNATURE.len()];
    input
        .read_exact(&mut signature)
        .map_err(|_| ErrorKind::Request)?;
    if &signature != PDF_SIGNATURE {
        return Err(ErrorKind::NotPdf);
    }
    let mut pdf = vec![0; pdf_len];
    pdf[..PDF_SIGNATURE.len()].copy_from_slice(&signature);
    input
        .read_exact(&mut pdf[PDF_SIGNATURE.len()..])
        .map_err(|_| ErrorKind::Request)?;
    Ok((trimmed.to_owned(), pdf))
}

fn serve_with(
    mut input: impl Read,
    mut output: impl Write,
    prepare: impl FnOnce(Vec<u8>, usize) -> Result<PageReply, ErrorKind>,
) -> io::Result<()> {
    let mut magic = [0; 4];
    let reply = match input.read_exact(&mut magic) {
        Ok(()) if magic == REQUEST_MAGIC => read_request(&mut input),
        Ok(()) if magic == image_helper::REQUEST_MAGIC => {
            return image_helper::serve(input, output);
        }
        Ok(()) if magic == SEARCH_REQUEST_MAGIC => {
            let reply = read_search_request(&mut input).and_then(|(query, pdf)| {
                catch_unwind(AssertUnwindSafe(|| pdf_hayro::search(pdf, &query)))
                    .unwrap_or(Err(ErrorKind::RendererFailed))
            });
            write_search_reply(&mut output, &reply)?;
            return output.flush();
        }
        _ => Err(ErrorKind::Request),
    }
    .and_then(|(page, pdf)| {
        // `prepare` guards each phase; this is the backstop for the rest.
        catch_unwind(AssertUnwindSafe(|| prepare(pdf, page)))
            .unwrap_or(Err(ErrorKind::RendererFailed))
    });
    write_reply(&mut output, &reply)?;
    output.flush()
}

/// Checks the size, page bound and `%PDF-` signature before allocating the
/// body, after [`serve`] read the magic.
fn read_request(input: &mut impl Read) -> Result<(usize, Vec<u8>), ErrorKind> {
    let mut header = [0; REQUEST_HEADER_BYTES - 4];
    input
        .read_exact(&mut header)
        .map_err(|_| ErrorKind::Request)?;
    if u32_at(&header, 0) != PROTOCOL_VERSION {
        return Err(ErrorKind::Request);
    }
    let page = u32_at(&header, 4) as usize;
    let length = u32_at(&header, 8) as usize;
    if length > MAX_INPUT_BYTES {
        return Err(ErrorKind::InputTooLarge);
    }
    if page >= MAX_PDF_PAGES {
        return Err(ErrorKind::PageBound);
    }
    if length < PDF_SIGNATURE.len() {
        return Err(ErrorKind::NotPdf);
    }
    let mut signature = [0; PDF_SIGNATURE.len()];
    input
        .read_exact(&mut signature)
        .map_err(|_| ErrorKind::Request)?;
    if &signature != PDF_SIGNATURE {
        return Err(ErrorKind::NotPdf);
    }
    // A zeroed buffer of the validated length, filled by plain reads only:
    // no end-of-input probing that could reach `statx` or `lseek`.
    let mut pdf = vec![0; length];
    pdf[..PDF_SIGNATURE.len()].copy_from_slice(&signature);
    input
        .read_exact(&mut pdf[PDF_SIGNATURE.len()..])
        .map_err(|_| ErrorKind::Request)?;
    Ok((page, pdf))
}

fn write_reply(output: &mut impl Write, reply: &Result<PageReply, ErrorKind>) -> io::Result<()> {
    let mut header = Vec::with_capacity(REPLY_PREFIX_BYTES + PAGE_HEADER_BYTES);
    header.extend_from_slice(&REPLY_MAGIC);
    header.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    let page = match reply {
        Err(kind) => {
            let (code, detail) = kind.code();
            header.push(STATUS_FAILED);
            header.push(code);
            header.extend_from_slice(&detail.to_le_bytes());
            return output.write_all(&header);
        }
        Ok(page) => page,
    };
    let (state, error, truncated, text) = match &page.text {
        Ok(PdfPageText {
            text: None,
            truncated,
        }) => (TEXT_ABSENT, 0, *truncated, ""),
        Ok(PdfPageText {
            text: Some(text),
            truncated,
        }) => (TEXT_PRESENT, 0, *truncated, text.as_str()),
        Err(kind) => (TEXT_FAILED, kind.code().0, false, ""),
    };
    let image = &page.image;
    header.push(STATUS_PAGE);
    for value in [
        wire(page.page),
        wire(page.page_count),
        image.original_width,
        image.original_height,
        image.width,
        image.height,
    ] {
        header.extend_from_slice(&value.to_le_bytes());
    }
    header.extend_from_slice(&[state, error, u8::from(truncated)]);
    for value in [
        wire(text.len()),
        page.timings.parse_us,
        page.timings.render_us,
        page.timings.text_us,
    ] {
        header.extend_from_slice(&value.to_le_bytes());
    }
    output.write_all(&header)?;
    output.write_all(&image.rgba)?;
    output.write_all(text.as_bytes())
}

fn read_page(input: &mut impl Read) -> io::Result<PageReply> {
    let mut header = [0; PAGE_HEADER_BYTES];
    input.read_exact(&mut header)?;
    let field = |index: usize| u32_at(&header, index * 4);
    let (page, page_count) = (field(0) as usize, field(1) as usize);
    let (original_width, original_height, width, height) = (field(2), field(3), field(4), field(5));
    let (state, error, truncated) = (header[24], header[25], header[26]);
    let text_length = u32_at(&header, 27) as usize;
    let timings = PhaseTimings {
        parse_us: u32_at(&header, 31),
        render_us: u32_at(&header, 35),
        text_us: u32_at(&header, 39),
    };
    if !(1..=MAX_PDF_PAGES).contains(&page_count) || page >= page_count {
        return Err(unreadable("the reply's page is outside its page count"));
    }
    if check_dimensions(original_width, original_height).is_err()
        || (width, height) != scaled_dimensions(original_width, original_height, PDF_PREVIEW_EDGE)
    {
        return Err(unreadable("the reply's page size is out of bounds"));
    }
    if truncated > 1 {
        return Err(unreadable("the reply's truncated flag is not a flag"));
    }
    // Absent text is never truncated (see `pdf_hayro`).
    let text_error = match (state, error) {
        (TEXT_ABSENT, 0) if text_length == 0 && truncated == 0 => None,
        (TEXT_PRESENT, 0) if (1..=MAX_PDF_PAGE_TEXT_BYTES).contains(&text_length) => None,
        // A renderer failure is the only way the helper loses a page's text.
        (TEXT_FAILED, code)
            if code == ErrorKind::RendererFailed.code().0 && text_length == 0 && truncated == 0 =>
        {
            Some(ErrorKind::RendererFailed)
        }
        _ => return Err(unreadable("the reply's text state is out of bounds")),
    };
    // Bounded by the checks above: at most 4,000,000 pixel bytes and 256 KiB of text.
    let mut rgba = vec![0; width as usize * height as usize * 4];
    input.read_exact(&mut rgba)?;
    let mut text = vec![0; text_length];
    input.read_exact(&mut text)?;
    let text = String::from_utf8(text).map_err(|_| unreadable("the reply's text is not UTF-8"))?;
    Ok(PageReply {
        page,
        page_count,
        image: ImagePreview {
            width,
            height,
            original_width,
            original_height,
            rgba,
            format: format!("PDF page {}", page + 1),
        },
        text: match text_error {
            Some(kind) => Err(kind),
            None => Ok(PdfPageText {
                text: (state == TEXT_PRESENT).then_some(text),
                truncated: truncated == 1,
            }),
        },
        timings,
    })
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    let mut value = [0; 4];
    value.copy_from_slice(&bytes[offset..offset + 4]);
    u32::from_le_bytes(value)
}

/// Every count and length on the wire is bounded far below `u32::MAX`; an
/// impossible larger value saturates and the reader refuses it.
fn wire(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn unreadable(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_pdf::{pdf, text_pdf};

    fn request(page: usize, pdf: &[u8]) -> Vec<u8> {
        let mut request = encode_request(page, pdf).unwrap().to_vec();
        request.extend_from_slice(pdf);
        request
    }

    fn served(request: &[u8]) -> Vec<u8> {
        let mut reply = Vec::new();
        serve(request, &mut reply).unwrap();
        reply
    }

    fn failure(reply: &[u8]) -> ErrorKind {
        read_reply(reply).unwrap().unwrap_err()
    }

    /// Yields `header`, then fails: a reader that reaches the body fails with
    /// an error the caller cannot mistake for a bounds refusal.
    struct HeaderOnly(io::Cursor<Vec<u8>>);

    impl Read for HeaderOnly {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            match self.0.read(buffer)? {
                0 => Err(io::Error::other("read past the header")),
                read => Ok(read),
            }
        }
    }

    fn header_only(bytes: &[u8]) -> HeaderOnly {
        HeaderOnly(io::Cursor::new(bytes.to_vec()))
    }

    fn page_reply(width: u32, height: u32, text: Result<PdfPageText, ErrorKind>) -> Vec<u8> {
        let mut reply = Vec::new();
        let page = PageReply {
            page: 1,
            page_count: 3,
            image: ImagePreview {
                width,
                height,
                original_width: width,
                original_height: height,
                rgba: vec![7; width as usize * height as usize * 4],
                format: String::new(),
            },
            text,
            timings: PhaseTimings {
                parse_us: 1,
                render_us: 2,
                text_us: 3,
            },
        };
        write_reply(&mut reply, &Ok(page)).unwrap();
        reply
    }

    fn present(text: &str) -> Result<PdfPageText, ErrorKind> {
        Ok(PdfPageText {
            text: Some(text.into()),
            truncated: false,
        })
    }

    #[test]
    fn round_trips_a_page_and_its_text_through_the_helper() {
        let source = text_pdf(&["First", "Second page"]);
        let reply = served(&request(1, &source));
        assert!(reply.len() <= MAX_REPLY_BYTES);
        let page = read_reply(&reply[..]).unwrap().unwrap();
        assert_eq!((page.page, page.page_count), (1, 2));
        assert_eq!(page.image.format, "PDF page 2");
        assert_eq!((page.image.width, page.image.height), (612, 792));
        assert_eq!(page.image.rgba.len(), 612 * 792 * 4);
        assert_eq!(&page.image.rgba[..4], &[255, 255, 255, 255]);
        assert_eq!(page.text, present("Second page"));
        assert_eq!(
            failure(&served(&request(2, &source))),
            ErrorKind::PageOutOfRange { page_count: 2 }
        );
    }

    #[test]
    fn the_largest_reply_fits_the_cap() {
        let text = "a".repeat(MAX_PDF_PAGE_TEXT_BYTES);
        let reply = page_reply(1000, 1000, present(&text));
        assert_eq!(reply.len(), MAX_REPLY_BYTES);
        assert_eq!(
            MAX_REPLY_BYTES,
            REPLY_PREFIX_BYTES + PAGE_HEADER_BYTES + 4_000_000 + 262_144
        );
        let page = read_reply(&reply[..]).unwrap().unwrap();
        assert_eq!(page.image.rgba.len(), 4_000_000);
        assert_eq!(page.text.unwrap().text.unwrap().len(), 262_144);
        assert_eq!(
            page.timings,
            PhaseTimings {
                parse_us: 1,
                render_us: 2,
                text_us: 3
            }
        );
    }

    #[test]
    fn every_error_kind_survives_the_reply() {
        let kinds = [
            ErrorKind::Request,
            ErrorKind::InputTooLarge,
            ErrorKind::NotPdf,
            ErrorKind::PageBound,
            ErrorKind::Corrupt,
            ErrorKind::Locked,
            ErrorKind::NoPages,
            ErrorKind::TooManyPages,
            ErrorKind::PageOutOfRange { page_count: 1 },
            ErrorKind::PageOutOfRange {
                page_count: MAX_PDF_PAGES,
            },
            ErrorKind::InvalidPageSize,
            ErrorKind::PageEdgeLimit,
            ErrorKind::PagePixelLimit,
            ErrorKind::RendererFailed,
        ];
        // A new kind fails to compile here until it joins the list above.
        for kind in kinds {
            match kind {
                ErrorKind::Request
                | ErrorKind::InputTooLarge
                | ErrorKind::NotPdf
                | ErrorKind::PageBound
                | ErrorKind::Corrupt
                | ErrorKind::Locked
                | ErrorKind::NoPages
                | ErrorKind::TooManyPages
                | ErrorKind::PageOutOfRange { .. }
                | ErrorKind::InvalidPageSize
                | ErrorKind::PageEdgeLimit
                | ErrorKind::PagePixelLimit
                | ErrorKind::RendererFailed => {}
            }
            let mut reply = Vec::new();
            write_reply(&mut reply, &Err(kind)).unwrap();
            assert_eq!(failure(&reply), kind);
        }
    }

    #[test]
    fn encode_request_applies_the_input_bounds() {
        assert!(encode_request(0, b"not a pdf").is_err());
        assert!(encode_request(MAX_PDF_PAGES, &pdf(1)).is_err());
        let mut large = pdf(1);
        large.resize(MAX_INPUT_BYTES + 1, b' ');
        assert!(encode_request(0, &large).is_err());
    }

    #[test]
    fn serve_refuses_bad_requests_before_reading_their_body() {
        let source = pdf(1);
        let valid = request(0, &source);
        let header = |edit: &dyn Fn(&mut Vec<u8>)| {
            let mut header = valid[..REQUEST_HEADER_BYTES].to_vec();
            edit(&mut header);
            header
        };
        let refusal = |reader: HeaderOnly| {
            let mut reply = Vec::new();
            serve(reader, &mut reply).unwrap();
            failure(&reply)
        };
        let magic = header(&|header| header[0] = b'X');
        assert_eq!(refusal(header_only(&magic)), ErrorKind::Request);
        let version = header(&|header| header[4] = 1);
        assert_eq!(refusal(header_only(&version)), ErrorKind::Request);
        let next_version = header(&|header| header[4] = 3);
        assert_eq!(refusal(header_only(&next_version)), ErrorKind::Request);
        let too_large = header(&|header| {
            header[12..].copy_from_slice(&(MAX_INPUT_BYTES as u32 + 1).to_le_bytes())
        });
        assert_eq!(refusal(header_only(&too_large)), ErrorKind::InputTooLarge);
        let largest = header(&|header| header[12..].copy_from_slice(&u32::MAX.to_le_bytes()));
        assert_eq!(refusal(header_only(&largest)), ErrorKind::InputTooLarge);
        let far_page =
            header(&|header| header[8..12].copy_from_slice(&(MAX_PDF_PAGES as u32).to_le_bytes()));
        assert_eq!(refusal(header_only(&far_page)), ErrorKind::PageBound);
        // The signature is checked before the rest of the body is allocated.
        let mut not_pdf = valid.clone();
        not_pdf[REQUEST_HEADER_BYTES] = b'#';
        assert_eq!(
            refusal(header_only(&not_pdf[..REQUEST_HEADER_BYTES + 5])),
            ErrorKind::NotPdf
        );
        let tiny = header(&|header| header[12..].copy_from_slice(&4u32.to_le_bytes()));
        assert_eq!(refusal(header_only(&tiny)), ErrorKind::NotPdf);
        assert_eq!(failure(&served(&valid[..10])), ErrorKind::Request);
        assert_eq!(
            failure(&served(&valid[..valid.len() - 1])),
            ErrorKind::Request
        );
    }

    #[test]
    fn read_reply_refuses_out_of_bounds_frames_before_allocating() {
        let valid = page_reply(2, 2, present("text"));
        let page = REPLY_PREFIX_BYTES;
        let set_all = |fields: &[(usize, u32)]| {
            let mut reply = valid.clone();
            for &(offset, value) in fields {
                reply[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
            reply
        };
        let set = |offset: usize, value: u32| set_all(&[(offset, value)]);
        let set_byte = |offset: usize, value: u8| {
            let mut reply = valid.clone();
            reply[offset] = value;
            reply
        };
        let headers = REPLY_PREFIX_BYTES + PAGE_HEADER_BYTES;
        for (case, frame) in [
            ("magic", set_byte(0, b'X')),
            ("version", set(4, 1)),
            ("status", set_byte(8, 9)),
            ("page past the count", set(page, 3)),
            ("no pages", set(page + 4, 0)),
            ("too many pages", set(page + 4, MAX_PDF_PAGES as u32 + 1)),
            // Output sizes consistent with each original, so only its own check refuses it.
            (
                "edge limit",
                set_all(&[(page + 8, 40_000), (page + 16, 1000), (page + 20, 1)]),
            ),
            (
                "pixel limit",
                set_all(&[
                    (page + 8, 32_000),
                    (page + 12, 32_000),
                    (page + 16, 1000),
                    (page + 20, 1000),
                ]),
            ),
            (
                "output wider than the edge",
                set_all(&[(page + 8, 1001), (page + 16, 1001)]),
            ),
            ("output not scaled from the page", set(page + 16, u32::MAX)),
            ("text state", set_byte(page + 24, 3)),
            ("text error without failure", set_byte(page + 25, 13)),
            ("truncated flag", set_byte(page + 26, 2)),
            (
                "text over the cap",
                set(page + 27, MAX_PDF_PAGE_TEXT_BYTES as u32 + 1),
            ),
            ("present without text", set(page + 27, 0)),
        ] {
            // Only the headers: reading on would fail differently.
            let error = read_reply(header_only(&frame[..headers]))
                .map(|_| ())
                .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{case}: {error}");
        }

        let absent_text = |truncated| {
            page_reply(
                2,
                2,
                Ok(PdfPageText {
                    text: None,
                    truncated,
                }),
            )
        };
        let absent = absent_text(false);
        let reply = read_reply(&absent[..]).unwrap().unwrap();
        assert_eq!(
            reply.text,
            Ok(PdfPageText {
                text: None,
                truncated: false
            })
        );
        let absent_truncated = absent_text(true);
        let mut absent_with_text = absent;
        absent_with_text[page + 27..page + 31].copy_from_slice(&1u32.to_le_bytes());
        absent_with_text.push(b'a');
        let failed = page_reply(2, 2, Err(ErrorKind::RendererFailed));
        assert_eq!(
            read_reply(&failed[..]).unwrap().unwrap().text,
            Err(ErrorKind::RendererFailed)
        );
        let mut failed_truncated = failed.clone();
        failed_truncated[page + 26] = 1;
        let mut unknown_text_error = failed.clone();
        unknown_text_error[page + 25] = 99;
        let mut other_text_error = failed;
        other_text_error[page + 25] = ErrorKind::Locked.code().0;
        let mut not_utf8 = valid.clone();
        let last = not_utf8.len() - 1;
        not_utf8[last] = 0xff;
        let mut trailing = valid.clone();
        trailing.push(0);
        let mut failure_frame = Vec::new();
        write_reply(&mut failure_frame, &Err(ErrorKind::Locked)).unwrap();
        let mut unknown_kind = failure_frame.clone();
        unknown_kind[REPLY_PREFIX_BYTES] = 99;
        let mut detail_on_plain_kind = failure_frame.clone();
        detail_on_plain_kind[REPLY_PREFIX_BYTES + 1] = 1;
        let mut out_of_range = Vec::new();
        write_reply(
            &mut out_of_range,
            &Err(ErrorKind::PageOutOfRange { page_count: 2 }),
        )
        .unwrap();
        let mut out_of_range_without_count = out_of_range.clone();
        out_of_range_without_count[REPLY_PREFIX_BYTES + 1] = 0;
        for (case, frame) in [
            ("absent and truncated", absent_truncated),
            ("absent with text", absent_with_text),
            ("failed and truncated", failed_truncated),
            ("unknown text error", unknown_text_error),
            ("text error other than a renderer failure", other_text_error),
            ("text not UTF-8", not_utf8),
            ("trailing bytes", trailing),
            ("unknown error kind", unknown_kind),
            ("detail on a plain kind", detail_on_plain_kind),
            ("out of range without a count", out_of_range_without_count),
        ] {
            let error = read_reply(&frame[..]).map(|_| ()).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{case}: {error}");
        }
        assert_eq!(failure(&failure_frame), ErrorKind::Locked);
        assert_eq!(
            failure(&out_of_range),
            ErrorKind::PageOutOfRange { page_count: 2 }
        );
        for cut in [4, REPLY_PREFIX_BYTES + 3, headers + 3, valid.len() - 1] {
            let error = read_reply(&valid[..cut]).map(|_| ()).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof, "cut at {cut}");
        }
    }

    #[test]
    fn pdf_frames_keep_their_layout_and_an_unknown_magic_is_refused() {
        // Version 2 added the image frame; version 3 added the PDF text-search frame (`GTPS`/`GTPR`).
        assert_eq!(PROTOCOL_VERSION, 3);
        let source = pdf(1);
        let header = encode_request(7, &source).unwrap();
        assert_eq!(&header[..4], b"GTPq");
        assert_eq!(u32_at(&header, 4), 3);
        assert_eq!(u32_at(&header, 8), 7);
        assert_eq!(u32_at(&header, 12) as usize, source.len());
        let reply = page_reply(2, 2, present("text"));
        assert_eq!(&reply[..4], b"GTPr");
        assert_eq!(reply.len(), REPLY_PREFIX_BYTES + PAGE_HEADER_BYTES + 16 + 4);
        let mut refused = Vec::new();
        write_reply(&mut refused, &Err(ErrorKind::Request)).unwrap();
        assert_eq!(refused.len(), REPLY_PREFIX_BYTES + FAILURE_BYTES);
        for magic in [*b"GTPr", *b"GTIr", *b"GTIQ", [0; 4]] {
            let mut request = request(0, &source);
            request[..4].copy_from_slice(&magic);
            assert_eq!(failure(&served(&request)), ErrorKind::Request, "{magic:?}");
        }
        // An image request is answered in the image frame, never the PDF one.
        let mut image =
            encode_image_request(ConfinedFormat::Jp2, 10, crate::image_helper::tests::RGB_JP2)
                .unwrap()
                .to_vec();
        image.extend_from_slice(crate::image_helper::tests::RGB_JP2);
        let reply = served(&image);
        assert_eq!(&reply[..4], b"GTIr");
        assert!(read_reply(&reply[..]).is_err());
    }

    #[test]
    fn a_panic_while_preparing_becomes_an_error_reply() {
        let mut reply = Vec::new();
        serve_with(&request(0, &pdf(1))[..], &mut reply, |_, _| {
            panic!("injected renderer panic")
        })
        .unwrap();
        assert_eq!(failure(&reply), ErrorKind::RendererFailed);
    }

    #[test]
    fn serve_buffer_round_trips_pdf_and_image_and_enforces_header_bounds() {
        let source = pdf(1);
        let mut out = Vec::new();
        serve_buffer(request(0, &source), &mut out).unwrap();
        let page = read_reply(&out[..]).unwrap().unwrap();
        assert_eq!(page.page_count, 1);
        assert_eq!(page.page, 0);

        let mut image_req =
            encode_image_request(ConfinedFormat::Jp2, 10, crate::image_helper::tests::RGB_JP2)
                .unwrap()
                .to_vec();
        image_req.extend_from_slice(crate::image_helper::tests::RGB_JP2);
        let mut image_out = Vec::new();
        serve_buffer(image_req, &mut image_out).unwrap();
        assert_eq!(&image_out[..4], b"GTIr");

        // Truncated body, wrong version, oversize length, out-of-range page, non-PDF header.
        let mut truncated = request(0, &source);
        truncated.pop();
        let mut reply = Vec::new();
        serve_buffer(truncated, &mut reply).unwrap();
        assert_eq!(failure(&reply), ErrorKind::Request);

        let mut bad_version = request(0, &source);
        bad_version[4..8].copy_from_slice(&99u32.to_le_bytes());
        reply.clear();
        serve_buffer(bad_version, &mut reply).unwrap();
        assert_eq!(failure(&reply), ErrorKind::Request);

        let mut too_large = request(0, &source);
        too_large[12..16].copy_from_slice(&((MAX_INPUT_BYTES + 1) as u32).to_le_bytes());
        reply.clear();
        serve_buffer(too_large, &mut reply).unwrap();
        assert_eq!(failure(&reply), ErrorKind::InputTooLarge);

        let mut bad_page = request(0, &source);
        bad_page[8..12].copy_from_slice(&(MAX_PDF_PAGES as u32).to_le_bytes());
        reply.clear();
        serve_buffer(bad_page, &mut reply).unwrap();
        assert_eq!(failure(&reply), ErrorKind::PageBound);

        let mut not_pdf = request(0, &source);
        not_pdf[REQUEST_HEADER_BYTES..REQUEST_HEADER_BYTES + 5].copy_from_slice(b"HELLO");
        reply.clear();
        serve_buffer(not_pdf, &mut reply).unwrap();
        assert_eq!(failure(&reply), ErrorKind::NotPdf);
    }

    #[test]
    fn search_frame_round_trips_through_stream_and_buffer_helpers() {
        let source = pdf(2);
        let mut frame = encode_search_request("page", &source).unwrap();
        frame.extend_from_slice(&source);

        let mut stream_out = Vec::new();
        serve(io::Cursor::new(&frame), &mut stream_out).unwrap();
        let stream_reply = read_search_reply(io::Cursor::new(&stream_out))
            .unwrap()
            .unwrap();
        assert_eq!(stream_reply.page_count, 2);
        assert_eq!(stream_reply.pages_scanned, 2);
        assert!(!stream_reply.truncated);

        let mut buf_out = Vec::new();
        serve_buffer(frame, &mut buf_out).unwrap();
        let buf_reply = read_search_reply(io::Cursor::new(&buf_out))
            .unwrap()
            .unwrap();
        assert_eq!(buf_reply, stream_reply);
    }
}
