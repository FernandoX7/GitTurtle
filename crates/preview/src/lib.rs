//! Bounded image previews decoded entirely from repository blob bytes.
//!
//! Run these synchronous functions on a bounded worker pool. Source/output sizes
//! are hard bounds; codec allocation limits are best effort, not a process memory
//! sandbox or a wall-clock deadline. No repository path is opened by this crate.

use std::{
    io::Cursor,
    path::Path,
    sync::{Arc, OnceLock},
};

use anyhow::{Context, Result, bail, ensure};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use resvg::{tiny_skia, usvg};

pub mod animation;
// Linux decodes AVIF with mp4parse and dav1d, and JPEG 2000 with
// hayro-jpeg2000, only inside the same confined helper as PDF pages.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod avif;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod image_helper;
#[cfg(any(
    target_os = "macos",
    test,
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
mod jpeg2000;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod jpeg2000_hayro;
pub mod mermaid;
pub mod metadata;
pub mod model3d;
#[cfg(target_os = "macos")]
mod native;
// Linux renders PDF pages with hayro only inside GitTurtle's confined helper
// process: `pdf_helper::serve` is the single way in, so the app never runs it.
// The helper supports x86-64 and AArch64; other targets keep refusing PDF pages.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod pdf_hayro;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub mod pdf_helper;
mod svg_limits;

pub const PDF_PREVIEW_EDGE: u32 = 1000;
pub const MAX_PDF_PAGE_TEXT_BYTES: usize = 256 * 1024;
const MAX_PDF_PAGES: usize = 100_000;

/// Extracted page text, separate from raster pixels and never used as a Git patch.
/// `None` means the page has no extractable text; no OCR is performed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfPageText {
    pub text: Option<String>,
    pub truncated: bool,
}

#[derive(Debug)]
pub struct DocumentPreview {
    pub pages: Vec<ImagePreview>,
    pub page_count: usize,
}

/// Static PDF rendering from supplied bytes only. Never invokes PDF actions,
/// JavaScript, launch links, external applications, or document URL loading.
pub fn decode_pdf(bytes: &[u8], check: impl Fn() -> Result<()>) -> Result<DocumentPreview> {
    decode_pdf_page(bytes, 0, check)
}

/// Render exactly one zero-based page from the captured document. Page count
/// discovery does not rasterize preceding pages or retain a native document.
pub fn decode_pdf_page(
    bytes: &[u8],
    page: usize,
    check: impl Fn() -> Result<()>,
) -> Result<DocumentPreview> {
    check_pdf_input(bytes, page)?;
    check()?;
    #[cfg(target_os = "macos")]
    {
        native::decode_pdf_page(bytes, page, check)
    }
    #[cfg(not(target_os = "macos"))]
    {
        bail!("PDF pages are not rendered on this platform.")
    }
}

/// Extract a single zero-based page using supplied bytes and the native PDFKit
/// text layer. Run on a bounded worker. At most 256 KiB of UTF-8 is returned;
/// truncation is explicit. PDF reading order may differ from its visual layout.
/// Cancellation is checked between phases, but cannot interrupt a system call.
pub fn decode_pdf_page_text(
    bytes: &[u8],
    page: usize,
    check: impl Fn() -> Result<()>,
) -> Result<PdfPageText> {
    check_pdf_input(bytes, page)?;
    check()?;
    #[cfg(target_os = "macos")]
    {
        native::decode_pdf_page_text(bytes, page, check)
    }
    #[cfg(not(target_os = "macos"))]
    {
        bail!("PDF text extraction is not available on this platform")
    }
}

fn check_pdf_input(bytes: &[u8], page: usize) -> Result<()> {
    ensure!(
        bytes.len() <= MAX_INPUT_BYTES,
        "PDF exceeds the 32 MiB input limit"
    );
    ensure!(bytes.starts_with(b"%PDF-"), "Unrecognized PDF header");
    ensure!(
        page < MAX_PDF_PAGES,
        "PDF page exceeds the 100,000-page navigation bound"
    );
    Ok(())
}

pub const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_SOURCE_PIXELS: u64 = 32_000_000;
pub const MAX_SOURCE_EDGE: u32 = 32_768;
pub const MAX_PREVIEW_EDGE: u32 = 4_096;
pub const MAX_DECODER_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_SVG_BYTES: usize = 2 * 1024 * 1024;
const MAX_SVG_NODES: usize = 10_000;
const MAX_SVG_DEPTH: usize = 128;
const MAX_LFS_POINTER_BYTES: usize = 1_024;

/// Straight (not premultiplied) RGBA8 pixels ready for a UI image upload.
#[derive(Debug, Clone)]
pub struct ImagePreview {
    pub width: u32,
    pub height: u32,
    pub original_width: u32,
    pub original_height: u32,
    pub rgba: Vec<u8>,
    pub format: String,
}

/// Metadata stored in a Git LFS pointer. `oid` is the 64-character SHA-256 hex value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LfsPointer {
    pub oid: String,
    pub size: u64,
}

pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png"
                    | "jpg"
                    | "jpeg"
                    | "webp"
                    | "gif"
                    | "svg"
                    | "bmp"
                    | "tif"
                    | "tiff"
                    | "ico"
                    | "avif"
                    | "heic"
                    | "heif"
                    | "jp2"
                    | "j2k"
                    | "j2c"
                    | "jpc"
                    | "jpx"
                    | "jpf"
            )
        })
}

/// Recognize a v1 Git LFS pointer without running a filter or opening its object.
///
/// Unknown extension fields are allowed; duplicate required fields, invalid OIDs,
/// and oversized or non-UTF-8 input are rejected. The returned OID excludes
/// `sha256:` and is never a filesystem path.
pub fn detect_lfs_pointer(bytes: &[u8]) -> Option<LfsPointer> {
    if bytes.len() > MAX_LFS_POINTER_BYTES {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines();
    if lines.next()? != "version https://git-lfs.github.com/spec/v1" {
        return None;
    }
    let mut oid = None;
    let mut size = None;
    for line in lines {
        let (key, value) = line.split_once(' ')?;
        match key {
            "oid" => {
                let hex = value.strip_prefix("sha256:")?;
                if oid.is_some()
                    || hex.len() != 64
                    || !hex
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                {
                    return None;
                }
                oid = Some(hex.to_owned());
            }
            "size" => {
                if size.is_some() || value.is_empty() || !value.bytes().all(|c| c.is_ascii_digit())
                {
                    return None;
                }
                size = Some(value.parse::<u64>().ok()?);
            }
            "version" => return None,
            // Extension lines are part of the LFS pointer format, but are never
            // interpreted as instructions or paths by the preview pipeline.
            _ if key.starts_with("ext-") && !value.is_empty() => {}
            _ => return None,
        }
    }
    Some(LfsPointer {
        oid: oid?,
        size: size?,
    })
}

/// Decode supported rasters (first image/frame) or a static, self-contained SVG.
///
/// The filename is a format hint, never a local path to read. Raster magic takes
/// precedence over it. Images are not enlarged; `max_edge` is capped at 4096.
/// Original dimensions are expressed after applying EXIF orientation.
/// SVG linked/embedded images, DTDs, and filter effects are rejected explicitly
/// rather than returning a misleading partial preview. SVG fonts come only from
/// the system font database, loaded once when text first appears.
pub fn decode_image(bytes: &[u8], file_name: &str, max_edge: u32) -> Result<ImagePreview> {
    ensure!(max_edge > 0, "Preview size must be greater than zero");
    ensure!(
        bytes.len() <= MAX_INPUT_BYTES,
        "Image exceeds the 32 MiB input limit"
    );
    ensure!(!bytes.is_empty(), "Image is empty");
    if detect_lfs_pointer(bytes).is_some() {
        bail!("Git LFS object is unavailable in this blob; only its pointer is stored here");
    }
    let max_edge = max_edge.min(MAX_PREVIEW_EDGE);
    match image::guess_format(bytes) {
        Ok(
            format @ (ImageFormat::Png
            | ImageFormat::Jpeg
            | ImageFormat::WebP
            | ImageFormat::Gif
            | ImageFormat::Bmp
            | ImageFormat::Tiff
            | ImageFormat::Ico),
        ) => decode_raster(bytes, format, max_edge),
        _ if let Some(format) =
            metadata::iso_image_format(bytes).or_else(|| metadata::jpeg2000_format(bytes)) =>
        {
            #[cfg(target_os = "macos")]
            {
                native::decode_image(bytes, format, max_edge)
            }
            // Linux decodes AVIF and JPEG 2000 only in the confined helper
            // (`confined_image_format`), never here, and HEIF not at all.
            #[cfg(target_os = "linux")]
            if format == "HEIF / HEIC" {
                bail!("HEIF / HEIC images are not decoded on Linux")
            }
            #[cfg(not(target_os = "macos"))]
            {
                bail!("{format} rendering is not available on this platform")
            }
        }
        _ if metadata::is_svg(bytes)
            || Path::new(file_name)
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("svg")) =>
        {
            decode_svg(bytes, max_edge)
        }
        _ => bail!(
            "Unrecognized or unsupported image data; extension alone cannot identify a decoder"
        ),
    }
}

/// The format GitTurtle's confined Linux helper decodes for `bytes` (AVIF,
/// JP2 or a raw J2K codestream), by signature alone: the single decision that
/// routes an image to the helper instead of `decode_image`. HEIF and HEIC are
/// never routed. `bytes` may be a prefix of at least the first 4,096 bytes.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub fn confined_image_format(bytes: &[u8]) -> Option<pdf_helper::ConfinedFormat> {
    use pdf_helper::ConfinedFormat;
    if metadata::iso_image_format(bytes) == Some("AVIF") {
        return Some(ConfinedFormat::Avif);
    }
    match metadata::jpeg2000_format(bytes)? {
        "JPEG 2000 / JP2" => Some(ConfinedFormat::Jp2),
        _ => Some(ConfinedFormat::J2k),
    }
}

/// The system libdav1d's version string, as `dav1d_version()` reports it. The
/// only `unsafe` in the helper's image decoders: dav1d-rs does not wrap it.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn libdav1d_version() -> Option<String> {
    // SAFETY: `dav1d_version` takes no arguments and returns a pointer to a
    // NUL-terminated string in libdav1d's read-only data, which lives as long
    // as the library; it is checked for null and copied before use.
    unsafe {
        let version = dav1d_sys::dav1d_version();
        (!version.is_null())
            .then(|| {
                std::ffi::CStr::from_ptr(version)
                    .to_str()
                    .ok()
                    .map(str::to_owned)
            })
            .flatten()
    }
}

fn check_dimensions(width: u32, height: u32) -> Result<()> {
    ensure!(width > 0 && height > 0, "Image dimensions must be positive");
    ensure!(
        width <= MAX_SOURCE_EDGE && height <= MAX_SOURCE_EDGE,
        "Image dimensions exceed the 32768-pixel edge limit"
    );
    ensure!(
        u64::from(width) * u64::from(height) <= MAX_SOURCE_PIXELS,
        "Image exceeds the 32-million-pixel limit"
    );
    Ok(())
}

/// The size of a preview of a `width` × `height` image at `max_edge`: never
/// enlarged, its aspect ratio kept and each side at least one pixel.
/// `decode_image` applies it after capping `max_edge` at
/// [`MAX_PREVIEW_EDGE`], and the confined Linux helper at its own 1,600.
pub fn scaled_dimensions(width: u32, height: u32, max_edge: u32) -> (u32, u32) {
    let edge = width.max(height);
    if edge <= max_edge {
        return (width, height);
    }
    let scale = |value| {
        ((u64::from(value) * u64::from(max_edge) + u64::from(edge) / 2) / u64::from(edge)).max(1)
            as u32
    };
    (scale(width), scale(height))
}

/// The dimensions `bytes`' image decodes to, read from its header without
/// decoding a pixel, so that a caller can bound the work before it starts.
/// It is never more than the decoder produces from that header, so a
/// budget checked against it refuses nothing the decoder would fit: a
/// raster's through the header read `decode_image` itself begins with, and
/// on Linux a JPEG 2000 image's (JP2 or a raw codestream) as the confined
/// helper decodes it ([`jpeg2000`]'s bounded box and SIZ reader, never
/// hayro). `None` where nothing is read here: SVG, AVIF and HEIF; a JP2
/// file whose brand is not `jp2 `, including a JP2-compatible JPX the helper
/// decodes; JPEG 2000 on macOS, whose ImageIO may draw any size up to the
/// reference grid, and on platforms that do not decode it. `None` too where
/// the header cannot be read, and where the decoder refuses the image from
/// its header anyway (the input, source and allocation bounds), so that its
/// own refusal stays the one shown. The size is the stored one, before any
/// orientation, which can swap the sides but never changes their product or
/// a preview's. A header can lie: a caller that budgets decoded pixels
/// still checks the pixels a decode returns.
pub fn declared_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.is_empty() || bytes.len() > MAX_INPUT_BYTES {
        return None;
    }
    let (width, height) = match image::guess_format(bytes) {
        Ok(
            format @ (ImageFormat::Png
            | ImageFormat::Jpeg
            | ImageFormat::WebP
            | ImageFormat::Gif
            | ImageFormat::Bmp
            | ImageFormat::Tiff
            | ImageFormat::Ico),
        ) => {
            let decoder = raster_decoder(bytes, format).ok()?;
            if decoder.total_bytes() > MAX_DECODER_BYTES {
                return None;
            }
            decoder.dimensions()
        }
        #[cfg(all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ))]
        _ if metadata::jpeg2000_format(bytes).is_some() => {
            jpeg2000::dimensions(bytes).ok()??.declared()
        }
        _ => return None,
    };
    check_dimensions(width, height).ok()?;
    Some((width, height))
}

/// A decoder for a raster `format` that has read `bytes`' header, within the
/// source-edge and allocation limits.
fn raster_decoder(bytes: &[u8], format: ImageFormat) -> Result<impl ImageDecoder + '_> {
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_EDGE);
    limits.max_image_height = Some(MAX_SOURCE_EDGE);
    limits.max_alloc = Some(MAX_DECODER_BYTES);
    reader.limits(limits);
    reader.into_decoder().context("Cannot read image header")
}

fn decode_raster(bytes: &[u8], format: ImageFormat, max_edge: u32) -> Result<ImagePreview> {
    let mut decoder = raster_decoder(bytes, format)?;
    let (width, height) = decoder.dimensions();
    check_dimensions(width, height)?;
    ensure!(
        decoder.total_bytes() <= MAX_DECODER_BYTES,
        "Decoded image exceeds the 256 MiB limit"
    );
    let orientation = decoder
        .orientation()
        .context("Cannot read image orientation")?;
    let mut image = DynamicImage::from_decoder(decoder).context("Cannot decode image")?;
    image.apply_orientation(orientation);
    let (original_width, original_height) = (image.width(), image.height());
    let (width, height) = scaled_dimensions(original_width, original_height, max_edge);
    let mut rgba = image.into_rgba8();
    if (width, height) != (original_width, original_height) {
        // Straight-alpha interpolation mixes invisible RGB into visible edges.
        // Interpolate premultiplied samples, then return UI-friendly straight RGBA.
        premultiply(rgba.as_mut());
        rgba = image::imageops::resize(&rgba, width, height, image::imageops::FilterType::Triangle);
        unpremultiply(rgba.as_mut());
    }
    Ok(ImagePreview {
        width,
        height,
        original_width,
        original_height,
        rgba: rgba.into_raw(),
        format: match format {
            ImageFormat::Png => "PNG",
            ImageFormat::Jpeg => "JPEG",
            ImageFormat::WebP => "WebP",
            ImageFormat::Gif => "GIF",
            ImageFormat::Bmp => "BMP",
            ImageFormat::Tiff => "TIFF · first image",
            ImageFormat::Ico => "ICO · selected icon",
            _ => unreachable!("only explicitly supported raster formats reach the decoder"),
        }
        .into(),
    })
}

fn premultiply(bytes: &mut [u8]) {
    for px in bytes.as_chunks_mut::<4>().0 {
        let alpha = u16::from(px[3]);
        for channel in &mut px[..3] {
            *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
}

fn unpremultiply(bytes: &mut [u8]) {
    for px in bytes.as_chunks_mut::<4>().0 {
        let alpha = u16::from(px[3]);
        for channel in &mut px[..3] {
            *channel = (u16::from(*channel) * 255 + alpha / 2)
                .checked_div(alpha)
                .unwrap_or(0)
                .min(255) as u8;
        }
    }
}

fn decode_svg(bytes: &[u8], max_edge: u32) -> Result<ImagePreview> {
    ensure!(
        bytes.len() <= MAX_SVG_BYTES,
        "SVG exceeds the 2 MiB input limit"
    );
    let text = std::str::from_utf8(bytes).context("SVG is not UTF-8 text")?;
    // The XML parser rejects DTDs by default. Validate structural complexity
    // before usvg expands references or creates any rendering intermediates.
    let document = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: MAX_SVG_NODES as u32,
            entity_resolver: None,
        },
    )
    .context("Cannot parse SVG XML within the 10000-node limit")?;
    let mut has_text = false;
    for node in document.descendants() {
        ensure!(
            node.ancestors().count() <= MAX_SVG_DEPTH,
            "SVG nesting exceeds the 128-level limit"
        );
        if node.is_element() {
            match node.tag_name().name() {
                "image" | "feImage" => bail!("SVG image resources are disabled in this preview"),
                // Filter regions can allocate large intermediate pixmaps unrelated
                // to the viewport. Support needs its own rendering budget first.
                "filter" => bail!("SVG filter effects are not supported in bounded previews yet"),
                "text" => has_text = true,
                _ => {}
            }
            for attribute in node.attributes() {
                if attribute.name() == "href" && !attribute.value().starts_with('#') {
                    bail!("External SVG resources are disabled in this preview");
                }
            }
        }
    }
    svg_limits::check_expansion(&document)?;
    let mut options = usvg::Options {
        // Explicit resolvers: resources_dir=None alone still permits absolute
        // paths in the default usvg resolver. No resource bytes may enter here.
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    if has_text {
        static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
        options.fontdb = Arc::clone(FONTS.get_or_init(|| {
            let mut fonts = usvg::fontdb::Database::new();
            fonts.load_system_fonts();
            Arc::new(fonts)
        }));
    }
    let tree = usvg::Tree::from_xmltree(&document, &options).context("Cannot render SVG")?;
    let size = tree.size();
    let original_width = size.width().ceil() as u32;
    let original_height = size.height().ceil() as u32;
    check_dimensions(original_width, original_height)?;
    let (width, height) = scaled_dimensions(original_width, original_height, max_edge);
    let mut pixmap =
        tiny_skia::Pixmap::new(width, height).context("Cannot allocate SVG preview")?;
    // Use one scale on both axes; viewport rounding must not stretch geometry.
    let scale = (width as f32 / size.width()).min(height as f32 / size.height());
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let mut rgba = pixmap.take();
    unpremultiply(&mut rgba);
    Ok(ImagePreview {
        width,
        height,
        original_width,
        original_height,
        rgba,
        format: "SVG".into(),
    })
}

/// Small, valid PDFs generated for the Linux renderer's tests. Every page
/// shares one Helvetica font with WinAnsi encoding, so content can show text.
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub(crate) mod test_pdf {
    #[derive(Clone)]
    pub(crate) struct Page {
        pub(crate) media_box: [u32; 4],
        pub(crate) crop_box: Option<[u32; 4]>,
        pub(crate) rotate: i32,
        pub(crate) content: Vec<u8>,
    }

    impl Page {
        /// A 120 × 80 point page drawing `content`.
        pub(crate) fn new(content: impl Into<Vec<u8>>) -> Self {
            Self {
                media_box: [0, 0, 120, 80],
                crop_box: None,
                rotate: 0,
                content: content.into(),
            }
        }
    }

    /// `pages` pages of 120 × 80 points whose top half is red.
    pub(crate) fn pdf(pages: usize) -> Vec<u8> {
        document(&vec![Page::new("1 0 0 rg 0 40 120 40 re f"); pages], None)
    }

    /// One US Letter page per text, shown once at the top left.
    pub(crate) fn text_pdf(texts: &[&str]) -> Vec<u8> {
        let pages: Vec<_> = texts
            .iter()
            .map(|text| Page {
                media_box: [0, 0, 612, 792],
                ..Page::new(format!("BT /F1 12 Tf 40 740 Td ({text}) Tj ET"))
            })
            .collect();
        document(&pages, None)
    }

    /// A document with `pages`; `user_password` encrypts it with the standard
    /// security handler (revision 2, 40-bit RC4) and owner password "owner".
    pub(crate) fn document(pages: &[Page], user_password: Option<&[u8]>) -> Vec<u8> {
        let encryption = user_password.map(Encryption::new);
        let font = 3 + pages.len() * 2;
        let kids: Vec<_> = (0..pages.len())
            .map(|index| format!("{} 0 R", 3 + index * 2))
            .collect();
        let mut objects = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            format!(
                "<< /Type /Pages /Count {} /Kids [{}] >>",
                pages.len(),
                kids.join(" ")
            )
            .into_bytes(),
        ];
        let numbers = |values: [u32; 4]| values.map(|value| value.to_string()).join(" ");
        for (index, page) in pages.iter().enumerate() {
            let crop_box = page
                .crop_box
                .map(|crop_box| format!(" /CropBox [{}]", numbers(crop_box)))
                .unwrap_or_default();
            objects.push(
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [{}]{crop_box} /Rotate {} /Resources << /Font << /F1 {font} 0 R >> >> /Contents {} 0 R >>",
                    numbers(page.media_box),
                    page.rotate,
                    4 + index * 2
                )
                .into_bytes(),
            );
            let content = match &encryption {
                Some(encryption) => encryption.encrypt(4 + index * 2, &page.content),
                None => page.content.clone(),
            };
            let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
            stream.extend_from_slice(&content);
            stream.extend_from_slice(b"\nendstream");
            objects.push(stream);
        }
        objects.push(
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
                .to_vec(),
        );
        let identifier = hex(DOCUMENT_ID);
        let mut trailer = format!("/ID [<{identifier}> <{identifier}>]");
        if let Some(encryption) = &encryption {
            objects.push(
                format!(
                    "<< /Filter /Standard /V 1 /R 2 /O <{}> /U <{}> /P {PERMISSIONS} >>",
                    hex(&encryption.owner),
                    hex(&encryption.user)
                )
                .into_bytes(),
            );
            trailer.push_str(&format!(" /Encrypt {} 0 R", objects.len()));
        }
        assemble(&objects, &trailer)
    }

    /// Numbers `objects` from 1, with object 1 as the catalog, and writes an
    /// exact cross-reference table and a trailer carrying `trailer` entries.
    pub(crate) fn assemble(objects: &[Vec<u8>], trailer: &str) -> Vec<u8> {
        let mut output = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::with_capacity(objects.len());
        for (index, object) in objects.iter().enumerate() {
            offsets.push(output.len());
            output.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            output.extend_from_slice(object);
            output.extend_from_slice(b"\nendobj\n");
        }
        let xref = output.len();
        output.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            output.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        output.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R {trailer} >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        output
    }

    const DOCUMENT_ID: &[u8; 16] = b"GitTurtleFixture";
    const PERMISSIONS: i32 = -4;
    // ISO 32000-1, 7.6.3.3, Algorithm 2 step a.
    const PADDING: [u8; 32] = [
        0x28, 0xbf, 0x4e, 0x5e, 0x4e, 0x75, 0x8a, 0x41, 0x64, 0x00, 0x4e, 0x56, 0xff, 0xfa, 0x01,
        0x08, 0x2e, 0x2e, 0x00, 0xb6, 0xd0, 0x68, 0x3e, 0x80, 0x2f, 0x0c, 0xa9, 0xfe, 0x64, 0x53,
        0x69, 0x7a,
    ];

    struct Encryption {
        key: [u8; 5],
        owner: Vec<u8>,
        user: Vec<u8>,
    }

    impl Encryption {
        /// Algorithms 2, 3 and 4 of ISO 32000-1 for revision 2.
        fn new(user_password: &[u8]) -> Self {
            let owner = rc4(&md5(&padded(b"owner"))[..5], &padded(user_password));
            let mut input = padded(user_password).to_vec();
            input.extend_from_slice(&owner);
            input.extend_from_slice(&PERMISSIONS.to_le_bytes());
            input.extend_from_slice(DOCUMENT_ID);
            let mut key = [0; 5];
            key.copy_from_slice(&md5(&input)[..5]);
            let user = rc4(&key, &PADDING);
            Self { key, owner, user }
        }

        /// Algorithm 1: RC4 with the object's own key (generation 0).
        fn encrypt(&self, object: usize, data: &[u8]) -> Vec<u8> {
            let mut input = self.key.to_vec();
            input.extend_from_slice(&(object as u32).to_le_bytes()[..3]);
            input.extend_from_slice(&[0, 0]);
            rc4(&md5(&input)[..10], data)
        }
    }

    fn padded(password: &[u8]) -> [u8; 32] {
        let length = password.len().min(32);
        let mut output = PADDING;
        output[..length].copy_from_slice(&password[..length]);
        output[length..].copy_from_slice(&PADDING[..32 - length]);
        output
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
        let mut state: Vec<u8> = (0..=255).collect();
        let mut j = 0u8;
        for i in 0..256 {
            j = j.wrapping_add(state[i]).wrapping_add(key[i % key.len()]);
            state.swap(i, usize::from(j));
        }
        let (mut i, mut j) = (0u8, 0u8);
        data.iter()
            .map(|byte| {
                i = i.wrapping_add(1);
                j = j.wrapping_add(state[usize::from(i)]);
                state.swap(usize::from(i), usize::from(j));
                byte ^ state[usize::from(state[usize::from(i)].wrapping_add(state[usize::from(j)]))]
            })
            .collect()
    }

    /// RFC 1321.
    fn md5(input: &[u8]) -> [u8; 16] {
        const SHIFTS: [u32; 16] = [7, 12, 17, 22, 5, 9, 14, 20, 4, 11, 16, 23, 6, 10, 15, 21];
        let constants: Vec<u32> = (0..64u32)
            .map(|index| ((f64::from(index) + 1.).sin().abs() * 4_294_967_296.) as u32)
            .collect();
        let mut message = input.to_vec();
        message.push(0x80);
        while message.len() % 64 != 56 {
            message.push(0);
        }
        message.extend_from_slice(&(input.len() as u64 * 8).to_le_bytes());
        let mut state = [0x6745_2301u32, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];
        for block in message.as_chunks::<64>().0 {
            let words: Vec<u32> = block
                .as_chunks::<4>()
                .0
                .iter()
                .map(|word| u32::from_le_bytes(*word))
                .collect();
            let [mut a, mut b, mut c, mut d] = state;
            for index in 0..64 {
                let (mixed, word) = match index / 16 {
                    0 => ((b & c) | (!b & d), index),
                    1 => ((d & b) | (!d & c), (5 * index + 1) % 16),
                    2 => (b ^ c ^ d, (3 * index + 5) % 16),
                    _ => (c ^ (b | !d), (7 * index) % 16),
                };
                let sum = mixed
                    .wrapping_add(a)
                    .wrapping_add(constants[index])
                    .wrapping_add(words[word]);
                (a, d, c) = (d, c, b);
                b = b.wrapping_add(sum.rotate_left(SHIFTS[index / 16 * 4 + index % 4]));
            }
            for (value, add) in state.iter_mut().zip([a, b, c, d]) {
                *value = value.wrapping_add(add);
            }
        }
        let mut output = [0; 16];
        for (chunk, value) in output.as_chunks_mut::<4>().0.iter_mut().zip(state) {
            *chunk = value.to_le_bytes();
        }
        output
    }

    #[test]
    fn pdf_fixture_digests_match_their_specifications() {
        assert_eq!(hex(&md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(
            hex(&md5(b"The quick brown fox jumps over the lazy dog")),
            "9e107d9d372bb6826bd81d3542a419d6"
        );
        assert_eq!(hex(&rc4(b"Key", b"Plaintext")), "bbf316e8d940af0ad3");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Frame, Rgba, RgbaImage};

    fn encoded(image: RgbaImage, format: ImageFormat) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        let mut image = DynamicImage::ImageRgba8(image);
        if format == ImageFormat::Jpeg {
            image = DynamicImage::ImageRgb8(image.to_rgb8());
        }
        image.write_to(&mut bytes, format).unwrap();
        bytes.into_inner()
    }

    #[test]
    fn decodes_all_supported_rasters_by_magic() {
        for (format, name) in [
            (ImageFormat::Png, "PNG"),
            (ImageFormat::Jpeg, "JPEG"),
            (ImageFormat::WebP, "WebP"),
            (ImageFormat::Gif, "GIF"),
            (ImageFormat::Bmp, "BMP"),
            (ImageFormat::Tiff, "TIFF · first image"),
            (ImageFormat::Ico, "ICO · selected icon"),
        ] {
            let bytes = encoded(RgbaImage::from_pixel(3, 2, Rgba([255, 0, 0, 255])), format);
            let preview = decode_image(&bytes, "wrong.extension", 100).unwrap();
            assert_eq!((preview.width, preview.height), (3, 2));
            assert_eq!((preview.original_width, preview.original_height), (3, 2));
            assert_eq!(preview.rgba.len(), 24);
            assert_eq!(preview.format, name);
        }
    }

    #[test]
    fn preserves_alpha_without_resizing() {
        let original = RgbaImage::from_raw(2, 1, vec![255, 40, 20, 128, 1, 2, 3, 0]).unwrap();
        let bytes = encoded(original.clone(), ImageFormat::Png);
        let preview = decode_image(&bytes, "alpha.png", 20).unwrap();
        assert_eq!(preview.rgba, original.into_raw());
    }

    #[test]
    fn thumbnail_preserves_aspect_and_has_no_transparent_color_bleed() {
        let mut original = RgbaImage::from_pixel(4, 2, Rgba([0, 0, 255, 0]));
        for y in 0..2 {
            original.put_pixel(0, y, Rgba([255, 0, 0, 255]));
            original.put_pixel(1, y, Rgba([255, 0, 0, 255]));
        }
        let preview = decode_image(&encoded(original, ImageFormat::Png), "alpha.png", 2).unwrap();
        assert_eq!((preview.width, preview.height), (2, 1));
        assert_eq!((preview.original_width, preview.original_height), (4, 2));
        for pixel in preview.rgba.as_chunks::<4>().0.iter().filter(|p| p[3] > 0) {
            assert_eq!(pixel[0], 255);
            assert_eq!(pixel[2], 0);
        }
    }

    #[test]
    fn gif_returns_only_first_frame() {
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            encoder
                .encode_frame(Frame::new(RgbaImage::from_pixel(
                    2,
                    1,
                    Rgba([255, 0, 0, 255]),
                )))
                .unwrap();
            encoder
                .encode_frame(Frame::new(RgbaImage::from_pixel(
                    2,
                    1,
                    Rgba([0, 255, 0, 255]),
                )))
                .unwrap();
        }
        let preview = decode_image(&bytes, "animation.gif", 20).unwrap();
        assert_eq!(&preview.rgba[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn jpeg2000_jp2_magic_precedes_names_and_preserves_native_orientation() {
        assert_jpeg2000_preview(
            "JP2 container",
            include_bytes!("../tests/fixtures/half-red-blue.jp2"),
        );
    }

    #[test]
    fn jpeg2000_j2k_respects_native_capability_and_preserves_magic_and_pixels() {
        #[cfg(target_os = "macos")]
        if !native::raw_jpeg2000_fixture_is_supported()
            .expect("Independent raw J2K capability probe failed")
        {
            let bytes = include_bytes!("../tests/fixtures/half-red-blue.j2k");
            assert_eq!(
                metadata::jpeg2000_format(bytes),
                Some("JPEG 2000 / J2K codestream")
            );
            assert!(metadata::is_image(bytes));
            assert_eq!(
                decode_image(bytes, "misleading.txt", 32)
                    .expect_err("The unavailable raw J2K codec must remain explicit")
                    .to_string(),
                "The installed macOS codec cannot decode this JPEG 2000 / J2K codestream image"
            );
            return;
        }
        assert_jpeg2000_preview(
            "raw J2K codestream",
            include_bytes!("../tests/fixtures/half-red-blue.j2k"),
        );
    }

    fn assert_jpeg2000_preview(kind: &str, bytes: &[u8]) {
        assert!(metadata::jpeg2000_format(bytes).is_some());
        assert!(metadata::is_image(bytes));
        let result = decode_image(bytes, "misleading.txt", 32);
        #[cfg(target_os = "macos")]
        {
            let image = result.unwrap_or_else(|error| panic!("{kind}: {error:#}"));
            assert_eq!((image.original_width, image.original_height), (64, 64));
            assert_eq!((image.width, image.height), (32, 32));
            assert!(image.format.contains("JPEG 2000"));
            let top = &image.rgba[(8 * 32 + 16) * 4..][..4];
            let bottom = &image.rgba[(24 * 32 + 16) * 4..][..4];
            assert!(top[0] > 200 && top[2] < 40, "{kind}: {top:?}");
            assert!(bottom[2] > 200 && bottom[0] < 40, "{kind}: {bottom:?}");
        }
        #[cfg(not(target_os = "macos"))]
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("rendering is not available on this platform"),
            "{kind}"
        );
    }

    #[test]
    fn jpeg2000_rejects_missing_or_invalid_content() {
        assert!(is_image_path(Path::new("photo.JP2")));
        assert!(is_image_path(Path::new("photo.j2k")));
        assert!(decode_image(b"not an image", "photo.jp2", 32).is_err());
        assert!(decode_image(b"\xff\x4f\xff\x51", "broken.j2k", 32).is_err());
    }

    /// Linux decodes AVIF only in the confined helper and HEIF not at all, so
    /// `decode_image` refuses both in the app process, each with its sentence.
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_decode_image_refuses_avif_and_heif_in_process() {
        for (bytes, sentence) in [
            (
                include_bytes!("../tests/fixtures/half-red-blue.avif").as_slice(),
                "AVIF rendering is not available on this platform",
            ),
            (
                include_bytes!("../tests/fixtures/half-red-blue.heic").as_slice(),
                "HEIF / HEIC images are not decoded on Linux",
            ),
        ] {
            assert_eq!(
                decode_image(bytes, "image.png", 32)
                    .map(|image| image.format)
                    .map_err(|error| error.to_string()),
                Err(sentence.to_owned())
            );
        }
    }

    #[test]
    fn applies_jpeg_exif_orientation() {
        let bytes = encoded(
            RgbaImage::from_pixel(2, 3, Rgba([255, 0, 0, 255])),
            ImageFormat::Jpeg,
        );
        let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
        let mut oriented = bytes[..2].to_vec();
        oriented.extend_from_slice(&[0xff, 0xe1]);
        oriented.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        oriented.extend_from_slice(exif);
        oriented.extend_from_slice(&bytes[2..]);
        let preview = decode_image(&oriented, "photo.jpg", 20).unwrap();
        assert_eq!((preview.width, preview.height), (3, 2));
        assert_eq!((preview.original_width, preview.original_height), (3, 2));
    }

    #[test]
    fn rejects_input_and_requested_size_limits() {
        assert!(
            decode_image(&vec![0; MAX_INPUT_BYTES + 1], "large.png", 100)
                .unwrap_err()
                .to_string()
                .contains("input limit")
        );
        assert!(decode_image(b"", "empty.png", 100).is_err());
        assert!(
            decode_image(b"anything", "image.png", 0)
                .unwrap_err()
                .to_string()
                .contains("greater than zero")
        );
        let huge_svg = b"<svg xmlns='http://www.w3.org/2000/svg' width='8000' height='8000'/>";
        assert!(
            decode_image(huge_svg, "huge.svg", 100)
                .unwrap_err()
                .to_string()
                .contains("pixel limit")
        );
    }

    #[test]
    fn rejects_raster_pixel_limit_before_decoding_pixels() {
        // Valid PNG header from a tiny fixture, with a larger IHDR and its CRC
        // recomputed. IDAT is intentionally tiny: the header bound must be hit
        // before the decoder attempts to inflate image data.
        let mut bytes = encoded(RgbaImage::new(1, 1), ImageFormat::Png);
        bytes[16..20].copy_from_slice(&8000_u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&8000_u32.to_be_bytes());
        let mut crc = !0_u32;
        for &byte in &bytes[12..29] {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb88320 & (0_u32.wrapping_sub(crc & 1)));
            }
        }
        bytes[29..33].copy_from_slice(&(!crc).to_be_bytes());
        let error = decode_image(&bytes, "huge.png", 100).unwrap_err();
        assert!(error.to_string().contains("32-million-pixel"), "{error:#}");
    }

    #[test]
    fn svg_renders_straight_alpha_and_internal_references() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2">
            <defs><rect id="r" width="2" height="2" fill="red" fill-opacity="0.5"/></defs>
            <use href="#r"/>
        </svg>"##;
        let preview = decode_image(svg, "test.SVG", 100).unwrap();
        assert_eq!((preview.width, preview.height), (4, 2));
        assert_eq!(preview.format, "SVG");
        assert_eq!(&preview.rgba[..4], &[255, 0, 0, 128]);
        assert_eq!(preview.rgba[15], 0);
    }

    #[test]
    fn svg_cannot_open_external_or_embedded_image_resources() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("secret.png");
        let secret = encoded(
            RgbaImage::from_pixel(1, 1, Rgba([0, 255, 0, 255])),
            ImageFormat::Png,
        );
        std::fs::write(&path, &secret).unwrap();
        for href in [
            path.to_string_lossy().into_owned(),
            "https://example.com/image.png".into(),
            "../secret.png".into(),
            "data:image/png;base64,iVBORw0KGgo=".into(),
        ] {
            let svg = format!(
                "<svg xmlns='http://www.w3.org/2000/svg' width='2' height='2'><image href='{href}' width='2' height='2'/></svg>"
            );
            let error = decode_image(svg.as_bytes(), "test.svg", 100).unwrap_err();
            assert!(error.to_string().contains("resources are disabled"));
        }
        assert_eq!(std::fs::read(path).unwrap(), secret);
    }

    #[test]
    fn svg_rejects_dtd_filters_and_excessive_nesting() {
        let dtd = b"<!DOCTYPE svg [<!ENTITY a 'secret'>]><svg xmlns='http://www.w3.org/2000/svg' width='2' height='2'><text>&a;</text></svg>";
        assert!(decode_image(dtd, "test.svg", 100).is_err());
        let filters = b"<svg xmlns='http://www.w3.org/2000/svg' width='2' height='2'><filter id='blur'><feGaussianBlur stdDeviation='100000'/></filter></svg>";
        assert!(
            decode_image(filters, "test.svg", 100)
                .unwrap_err()
                .to_string()
                .contains("filter effects")
        );
        let nested = format!(
            "<svg xmlns='http://www.w3.org/2000/svg' width='2' height='2'>{}{}</svg>",
            "<g>".repeat(130),
            "</g>".repeat(130)
        );
        assert!(
            decode_image(nested.as_bytes(), "test.svg", 100)
                .unwrap_err()
                .to_string()
                .contains("nesting")
        );
        // roxmltree before 0.21.2 overflowed a 2 MiB stack while parsing about
        // 3,500 nested elements, aborting the process before the depth check.
        let deep = format!(
            "<svg xmlns='http://www.w3.org/2000/svg' width='2' height='2'>{}{}</svg>",
            "<g>".repeat(9_000),
            "</g>".repeat(9_000)
        );
        let refused = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                decode_image(deep.as_bytes(), "test.svg", 100)
                    .map(|_| ())
                    .unwrap_err()
                    .to_string()
            })
            .unwrap()
            .join()
            .unwrap();
        assert!(refused.contains("nesting"), "{refused}");
    }

    #[test]
    fn svg_rejects_compact_reference_expansion_before_rendering() {
        // Only 43 XML elements, but cloning each pair of references recursively
        // produces more than ten thousand elements before rasterization.
        let mut svg = String::from(
            "<svg xmlns='http://www.w3.org/2000/svg' width='2' height='2'><defs><rect id='n0' width='1' height='1'/>",
        );
        for index in 1..=13 {
            svg.push_str(&format!(
                "<g id='n{index}'><use href='#n{}'/><use href='#n{}'/></g>",
                index - 1,
                index - 1
            ));
        }
        svg.push_str("</defs><use href='#n13'/></svg>");
        assert!(svg.len() < 1024);
        for svg in [
            svg.clone(),
            svg.replace("'/>", " '/>"),
            svg.replace(" xmlns='http://www.w3.org/2000/svg'", ""),
        ] {
            let error = decode_image(svg.as_bytes(), "fan-out.svg", 2).unwrap_err();
            assert!(error.to_string().contains("expanded"), "{error:#}");
        }
    }

    #[test]
    fn svg_bounds_reference_payload_cycles_and_expanded_depth() {
        // A small node count can still multiply a large path's geometry.
        let svg = format!(
            "<svg xmlns='http://www.w3.org/2000/svg' width='2' height='2'><defs><path id='p' d='M0 0 {}'/></defs>{}</svg>",
            "L1 1 ".repeat(8192),
            "<use href='#p'/>".repeat(64)
        );
        assert!(svg.len() < MAX_SVG_BYTES);
        let error = decode_image(svg.as_bytes(), "payload.svg", 2).unwrap_err();
        assert!(error.to_string().contains("payload"), "{error:#}");

        let mut deep = String::from(
            "<svg xmlns='http://www.w3.org/2000/svg' width='2' height='2'><defs><rect id='n0' width='1' height='1'/>",
        );
        for index in 1..=MAX_SVG_DEPTH {
            deep.push_str(&format!("<use id='n{index}' href='#n{}'/>", index - 1));
        }
        deep.push_str("</defs></svg>");
        let error = decode_image(deep.as_bytes(), "deep.svg", 2).unwrap_err();
        assert!(error.to_string().contains("nesting"), "{error:#}");

        let cycle = b"<svg xmlns='http://www.w3.org/2000/svg' width='2' height='2'><g id='a'><use href='#b'/></g><g id='b'><use href='#a'/></g></svg>";
        let error = decode_image(cycle, "cycle.svg", 2).unwrap_err();
        assert!(error.to_string().contains("cycle"), "{error:#}");
    }

    #[test]
    fn svg_expansion_preserves_href_precedence_and_nested_reference_pixels() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="2" height="1">
            <defs><rect id="r" width="1" height="1" fill="red"/>
            <rect id="r" width="1" height="1" fill="blue"/>
            <use id="a" xlink:href="#r"/></defs>
            <use xlink:href="#unused" href="#a "/><use href="#a" x="1"/>
        </svg>"##;
        let preview = decode_image(svg, "references.svg", 2).unwrap();
        assert_eq!(preview.rgba, [255, 0, 0, 255, 255, 0, 0, 255]);
    }

    #[test]
    fn svg_bounds_marker_multiplication_before_rendering() {
        let svg = format!(
            "<svg xmlns='http://www.w3.org/2000/svg' width='2' height='2'><defs><marker id='m' markerWidth='1' markerHeight='1'>{}</marker></defs><path d='M0 0 {}' marker-mid='url(#m)'/></svg>",
            "<path d='M0 0 L1 1 L0 1 Z'/>".repeat(50),
            "L1 1 L0 0 ".repeat(128)
        );
        assert!(svg.len() < 4096);
        let error = decode_image(svg.as_bytes(), "markers.svg", 2).unwrap_err();
        assert!(error.to_string().contains("expanded"), "{error:#}");
    }

    #[test]
    fn svg_marker_preflight_refuses_css_bypasses_and_nested_cycles() {
        let document = |extra: &str, path: &str| {
            format!(
                "<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'>{extra}<defs><marker id='m' markerWidth='2' markerHeight='2'><path d='M0 0 L2 2 L0 2 Z'/></marker></defs>{path}</svg>"
            )
        };
        for (extra, path) in [
            (
                "<style>path { marker-mid: url('#m') }</style>",
                "<path d='M0 0 L1 1'/>",
            ),
            (
                "<style xmlns='urn:untrusted'>path { marker-mid: url('#m') }</style>",
                "<path d='M0 0 L1 1'/>",
            ),
            ("", "<path style='marker:url(#m)' d='M0 0 L1 1'/>"),
            (
                "",
                "<g marker-mid='url(#m)'><path marker-mid='none' style='marker-mid:inherit' d='M0 0 L1 1'/></g>",
            ),
            (
                "<style>path { m\\61rker-mid: url('#m') }</style>",
                "<path d='M0 0 L1 1'/>",
            ),
        ] {
            let error = decode_image(document(extra, path).as_bytes(), "css.svg", 10).unwrap_err();
            assert!(error.to_string().contains("CSS"), "{error:#}");
        }
        let cycle = document("", "<path d='M0 0 L2 2' marker-end='url(#m)'/>").replace(
            "<path d='M0 0 L2 2 L0 2 Z'",
            "<path marker-end='url(#m)' d='M0 0 L2 2 L0 2 Z'",
        );
        let error = decode_image(cycle.as_bytes(), "nested.svg", 10).unwrap_err();
        assert!(error.to_string().contains("cycle"), "{error:#}");
    }

    #[test]
    fn svg_preserves_inherited_marker_arrows_and_plain_styles() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
            <style>path { stroke: none }</style>
            <defs><marker id="m" markerUnits="userSpaceOnUse" markerWidth="2" markerHeight="2"><rect width="2" height="2" fill="red"/></marker></defs>
            <g marker-end="url('#m')"><path d="M0 0 L4 4" style="fill: none"/></g>
        </svg>"##;
        let preview = decode_image(svg, "arrow.svg", 10).unwrap();
        assert_eq!(&preview.rgba[(4 * 10 + 4) * 4..][..4], &[255, 0, 0, 255]);
        assert_eq!(preview.rgba[3], 0);
    }

    #[test]
    #[ignore = "bounded mutation probe; run explicitly with --ignored --nocapture"]
    fn svg_bounded_mutation_probe() {
        // Reproducible, coverage-uninstrumented byte mutation of the boundary's
        // supported/refused syntax. This is a finite panic/output-invariant
        // probe, not a proof of safety or a native-framework fuzz campaign.
        let corpus = [
            "<svg xmlns='http://www.w3.org/2000/svg' width='8' height='8'><path d='M0 0 L8 8 L0 8 Z' fill='red'/></svg>",
            "<svg width='8' height='8'><defs><rect id='r' width='2' height='2'/><use id='u' href='#r'/></defs><use href='#u '/></svg>",
            "<svg width='8' height='8'><g id='a'><use href='#b'/></g><g id='b'><use href='#a'/></g></svg>",
            "<svg width='8' height='8'><defs><marker id='m' markerWidth='2' markerHeight='2'><path d='M0 0 L2 2 L0 2 Z'/></marker></defs><path marker-mid='url(#m)' d='M0 0 L2 2 L3 3'/></svg>",
            "<svg width='8' height='8'><defs><marker id='m'><path marker-end='url(#m)' d='M0 0 L1 1'/></marker></defs><style>path{marker:url(#m)}</style><path d='M0 0 L1 1'/></svg>",
            "<!DOCTYPE svg [<!ENTITY x 'bounded'>]><svg width='8' height='8'><image href='data:bad'/></svg>",
        ];
        let started = std::time::Instant::now();
        let mut random = 0x4153_5452_4153_5647u64;
        let mut accepted = 0;
        let mut attempted = 0;
        while attempted < 16_384 && started.elapsed() < std::time::Duration::from_secs(5) {
            let mut bytes = corpus[attempted % corpus.len()].as_bytes().to_vec();
            for _ in 0..1 + attempted % 4 {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                let at = random as usize % bytes.len().max(1);
                match random % 4 {
                    0 if !bytes.is_empty() => {
                        bytes.remove(at);
                    }
                    1 => bytes.insert(at, (random >> 32) as u8),
                    2 if !bytes.is_empty() => bytes[at] ^= (random >> 32) as u8,
                    _ => bytes.truncate(at),
                }
            }
            if let Ok(preview) = decode_image(&bytes, "mutation.svg", 32) {
                accepted += 1;
                assert!(preview.width > 0 && preview.width <= 32);
                assert!(preview.height > 0 && preview.height <= 32);
                assert_eq!(
                    preview.rgba.len(),
                    preview.width as usize * preview.height as usize * 4
                );
            }
            attempted += 1;
        }
        eprintln!(
            "SVG mutation seed=4153545241535647 corpus={} attempts={attempted} accepted={accepted} refused={} elapsed_ms={:.3}",
            corpus.len(),
            attempted - accepted,
            started.elapsed().as_secs_f64() * 1000.
        );
    }

    #[test]
    fn lfs_pointer_is_metadata_and_never_decoded_as_image() {
        let oid = "a".repeat(64);
        let text =
            format!("version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize 12345\n");
        assert_eq!(
            detect_lfs_pointer(text.as_bytes()),
            Some(LfsPointer { oid, size: 12345 })
        );
        assert!(
            decode_image(text.as_bytes(), "photo.png", 100)
                .unwrap_err()
                .to_string()
                .contains("Git LFS")
        );
        assert!(detect_lfs_pointer(format!("{text}size 2\n").as_bytes()).is_none());
        assert!(detect_lfs_pointer(text.replace("sha256:", "sha256:../").as_bytes()).is_none());
        assert!(detect_lfs_pointer(text.replace("size 12345", "size -1").as_bytes()).is_none());
        assert!(detect_lfs_pointer(b"not an LFS pointer").is_none());
    }

    #[test]
    fn supported_extensions_are_case_insensitive() {
        assert!(is_image_path(Path::new("nested/photo.JPEG")));
        assert!(is_image_path(Path::new("icon.svg")));
        assert!(!is_image_path(Path::new("photo.svg.exe")));
        assert!(!is_image_path(Path::new("photo")));
    }

    /// A tiny PNG whose header declares `width` × `height`, with the
    /// header's CRC recomputed and its one-pixel image data left as it was.
    fn png_declaring(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = encoded(RgbaImage::new(1, 1), ImageFormat::Png);
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        bytes[20..24].copy_from_slice(&height.to_be_bytes());
        let mut crc = !0_u32;
        for &byte in &bytes[12..29] {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb88320 & (0_u32.wrapping_sub(crc & 1)));
            }
        }
        bytes[29..33].copy_from_slice(&(!crc).to_be_bytes());
        bytes
    }

    /// The declared size is read from the header alone, for every raster
    /// format `decode_image` decodes and for JPEG 2000 (JP2 and a raw
    /// codestream): a PNG that declares 3,000 × 2,000 over one pixel of image
    /// data reads as that size, which its decode refuses. Nothing is
    /// reported for the formats no header reader covers here, for a header
    /// cut short, and for one past the source bounds, whose decode refuses
    /// it with its own sentence.
    #[test]
    fn declared_dimensions_read_headers_without_decoding_pixels() {
        for format in [
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::WebP,
            ImageFormat::Gif,
            ImageFormat::Bmp,
            ImageFormat::Tiff,
            ImageFormat::Ico,
        ] {
            let bytes = encoded(RgbaImage::new(7, 5), format);
            assert_eq!(declared_dimensions(&bytes), Some((7, 5)), "{format:?}");
        }
        let lying = png_declaring(3000, 2000);
        assert_eq!(declared_dimensions(&lying), Some((3000, 2000)));
        assert!(decode_image(&lying, "lying.png", 100).is_err());
        // JPEG 2000 is read where the confined helper decodes it, at the
        // size it decodes to: a 4,000 × 1,000 codestream whose component
        // samples every fourth row decodes to 4,000 × 250, not the default
        // size's 4,000 × 1,000. macOS's ImageIO may draw any size up to the
        // grid, so nothing is read there.
        let helper = cfg!(all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ));
        let mut subsampled = vec![0xff, 0x4f, 0xff, 0x51, 0, 41, 0, 0];
        for value in [4000_u32, 1000, 0, 0, 4000, 1000, 0, 0] {
            subsampled.extend(value.to_be_bytes());
        }
        subsampled.extend([0, 1, 7, 1, 4]);
        assert_eq!(
            declared_dimensions(&subsampled),
            helper.then_some((4000, 250))
        );
        let fixtures: [(&[u8], (u32, u32)); 4] = [
            (
                include_bytes!("../tests/fixtures/half-red-blue.jp2"),
                (64, 64),
            ),
            (
                include_bytes!("../tests/fixtures/half-red-blue.j2k"),
                (64, 64),
            ),
            (
                include_bytes!("../tests/fixtures/jpeg2000-large.jp2"),
                (1024, 768),
            ),
            (
                include_bytes!("../tests/fixtures/jpeg2000-subsampled.jp2"),
                (64, 48),
            ),
        ];
        for (bytes, size) in fixtures {
            assert_eq!(declared_dimensions(bytes), helper.then_some(size));
        }
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2"/>"#;
        let unread: [&[u8]; 7] = [
            include_bytes!("../tests/fixtures/half-red-blue.avif"),
            include_bytes!("../tests/fixtures/half-red-blue.heic"),
            svg,
            b"",
            &lying[..20],
            &png_declaring(8000, 8000),
            &include_bytes!("../tests/fixtures/half-red-blue.j2k")[..30],
        ];
        for bytes in unread {
            assert_eq!(declared_dimensions(bytes), None);
        }
    }
}
