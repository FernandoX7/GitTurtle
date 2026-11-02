//! Linux PDF pages and page text with hayro, reached only through
//! [`crate::pdf_helper::serve`] inside GitTurtle's confined helper process.
//!
//! hayro has no memory budget or cancellation, and some hostile inputs abort,
//! overflow the stack or never finish; only the helper's resource limits and
//! the client's deadline stop those. Rust panics do unwind, and each phase
//! turns one into [`ErrorKind::RendererFailed`]. The page keeps the macOS
//! contract: crop box, rotation, a white background, at most 1,000 pixels on
//! the longer edge, and no annotations or actions.
//!
//! Page text is cut at 256 KiB on a character boundary and flagged truncated.
//! A page whose text is only whitespace has absent text that is never
//! truncated, even when the whitespace runs past the cap.
//!
//! Known limits, for the helper and the release measurement:
//! - hayro builds the whole page tree while opening the document, before the
//!   100,000-page bound applies; only the helper's resource limits bound a file
//!   of millions of inline pages.
//! - A crop box entirely outside the media box renders as a blank A4 page
//!   (596 × 842, from hayro's `base_dimensions`); compare with macOS.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use hayro::hayro_interpret::font::{Glyph, GlyphRun};
use hayro::hayro_interpret::hayro_cmap::BfString;
use hayro::hayro_interpret::util::{RectExt, TransformExt};
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps,
    InterpreterCache, InterpreterSettings, SoftMask, interpret_page,
};
use hayro::hayro_syntax::page::Page;
use hayro::hayro_syntax::{DecryptionError, LoadPdfError, Pdf};
use hayro::kurbo::{Affine, BezPath, Point};
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::peniko::ImageAlphaType;
use hayro::vello_cpu::{Pixmap, RasterizerSettings, RenderContext, Resources, TargetInit};
use hayro::{RenderCache, RenderSettings, render_into};

use crate::pdf_helper::{ErrorKind, PageReply, PhaseTimings};
use crate::{
    ImagePreview, MAX_INPUT_BYTES, MAX_PDF_PAGE_TEXT_BYTES, MAX_PDF_PAGES, MAX_SOURCE_EDGE,
    PDF_PREVIEW_EDGE, PdfPageText, check_dimensions, scaled_dimensions,
};

/// A word gap wider than this share of the font size separates two words.
const WORD_GAP: f64 = 0.2;
/// A baseline that moves more than this share of the font size starts a line.
const LINE_GAP: f64 = 0.5;

pub(crate) fn prepare(pdf: Vec<u8>, page: usize) -> Result<PageReply, ErrorKind> {
    prepare_with(pdf, page, extract_text)
}

fn prepare_with(
    pdf: Vec<u8>,
    page_index: usize,
    text: impl FnOnce(&Page<'_>) -> PdfPageText,
) -> Result<PageReply, ErrorKind> {
    if pdf.len() > MAX_INPUT_BYTES {
        return Err(ErrorKind::InputTooLarge);
    }
    if !pdf.starts_with(b"%PDF-") {
        return Err(ErrorKind::NotPdf);
    }
    if page_index >= MAX_PDF_PAGES {
        return Err(ErrorKind::PageBound);
    }
    let started = Instant::now();
    // An empty user password opens; hayro tries it and nothing else.
    let document = guarded(|| Pdf::new(pdf))?.map_err(|error| match error {
        LoadPdfError::Decryption(DecryptionError::PasswordProtected) => ErrorKind::Locked,
        LoadPdfError::Decryption(_) | LoadPdfError::Invalid => ErrorKind::Corrupt,
    })?;
    let pages = document.pages();
    let page_count = pages.len();
    if page_count == 0 {
        return Err(ErrorKind::NoPages);
    }
    if page_count > MAX_PDF_PAGES {
        return Err(ErrorKind::TooManyPages);
    }
    let page = pages
        .get(page_index)
        .ok_or(ErrorKind::PageOutOfRange { page_count })?;
    // The crop box within the media box, with the page's rotation applied.
    let (page_width, page_height) = page.render_dimensions();
    if !(page_width.is_finite() && page_height.is_finite() && page_width > 0. && page_height > 0.) {
        return Err(ErrorKind::InvalidPageSize);
    }
    let (original_width, original_height) = (page_width.ceil() as u32, page_height.ceil() as u32);
    if check_dimensions(original_width, original_height).is_err() {
        return Err(if original_width.max(original_height) > MAX_SOURCE_EDGE {
            ErrorKind::PageEdgeLimit
        } else {
            ErrorKind::PagePixelLimit
        });
    }
    let (width, height) = scaled_dimensions(original_width, original_height, PDF_PREVIEW_EDGE);
    let parsed = Instant::now();
    let rgba = guarded(|| render(page, (width, height), (page_width, page_height)))?;
    if rgba.len() != width as usize * height as usize * 4 {
        return Err(ErrorKind::RendererFailed);
    }
    let rendered = Instant::now();
    let text = guarded(|| text(page));
    Ok(PageReply {
        page: page_index,
        page_count,
        image: ImagePreview {
            width,
            height,
            original_width,
            original_height,
            rgba,
            format: format!("PDF page {}", page_index + 1),
        },
        text,
        timings: PhaseTimings {
            parse_us: micros(parsed - started),
            render_us: micros(rendered - parsed),
            text_us: micros(rendered.elapsed()),
        },
    })
}

/// Runs one hayro phase, turning a panic into an error kind.
fn guarded<T>(phase: impl FnOnce() -> T) -> Result<T, ErrorKind> {
    catch_unwind(AssertUnwindSafe(phase)).map_err(|_| ErrorKind::RendererFailed)
}

fn settings() -> InterpreterSettings {
    // Embedded standard fonts and CMaps; annotations are not drawn, as with
    // CoreGraphics' page drawing on macOS, and no action is ever run.
    InterpreterSettings {
        render_annotations: false,
        ..InterpreterSettings::default()
    }
}

/// Straight RGBA of exactly `size`, the page scaled to fill it on white.
fn render(page: &Page<'_>, size: (u32, u32), page_size: (f32, f32)) -> Vec<u8> {
    // Both edges are at most `PDF_PREVIEW_EDGE`.
    let (width, height) = (size.0 as u16, size.1 as u16);
    let mut context = RenderContext::new(width, height);
    let transform = Affine::scale_non_uniform(
        f64::from(width) / f64::from(page_size.0),
        f64::from(height) / f64::from(page_size.1),
    ) * page.initial_transform(true).to_kurbo();
    render_into(
        page,
        &RenderCache::new(),
        &settings(),
        &RenderSettings::default(),
        &mut context,
        transform,
    );
    context.flush();
    let mut pixmap = Pixmap::new(width, height);
    context.render_with(
        &mut pixmap,
        &mut Resources::default(),
        RasterizerSettings {
            target_init: TargetInit::Clear(WHITE),
            ..RasterizerSettings::default()
        },
    );
    pixmap.take_rgba8(ImageAlphaType::Alpha)
}

/// The page's text in content order, at most 256 KiB of UTF-8. Lines and
/// word spaces come from glyph positions, so reading order may differ from
/// the visual layout, as on macOS; no OCR is performed.
fn extract_text(page: &Page<'_>) -> PdfPageText {
    let cache = InterpreterCache::new();
    let mut context = Context::new(
        Affine::IDENTITY,
        page.intersected_crop_box().to_kurbo(),
        &cache,
        page.xref(),
        settings(),
    );
    let mut collector = TextCollector::default();
    interpret_page(page, &mut context, &mut collector);
    collector.finish()
}

#[derive(Default)]
struct TextCollector {
    text: String,
    truncated: bool,
    previous: Option<Placement>,
    /// Glyph count and first and last placement of the previous run.
    previous_run: Option<(usize, Affine, Affine)>,
}

#[derive(Clone, Copy)]
struct Placement {
    origin: Point,
    end: Option<Point>,
    size: f64,
}

impl TextCollector {
    fn separate(&mut self, origin: Point, size: f64) {
        let Some(previous) = self.previous else {
            return;
        };
        let size = size.max(previous.size);
        if !size.is_finite() || size <= 0. {
            return;
        }
        if (origin.y - previous.origin.y).abs() > size * LINE_GAP {
            self.push_separator('\n');
        } else if let Some(end) = previous.end {
            let gap = origin.x - end.x;
            if gap > size * WORD_GAP || gap < -size {
                self.push_separator(' ');
            }
        }
    }

    fn push_separator(&mut self, separator: char) {
        match self.text.chars().next_back() {
            None | Some('\n') => {}
            Some(last) if last.is_whitespace() && separator == ' ' => {}
            Some(last) if last.is_whitespace() => {
                self.text.pop();
                self.push(separator);
            }
            Some(_) => self.push(separator),
        }
    }

    fn push(&mut self, character: char) {
        if self.truncated || (character.is_control() && !matches!(character, '\n' | '\t')) {
            return;
        }
        if self.text.len() + character.len_utf8() > MAX_PDF_PAGE_TEXT_BYTES {
            self.truncated = true;
            return;
        }
        self.text.push(character);
    }

    /// Text that is only whitespace is absent, never truncated, even past the cap.
    fn finish(mut self) -> PdfPageText {
        self.text.shrink_to_fit();
        let text = (!self.text.trim().is_empty()).then_some(self.text);
        PdfPageText {
            truncated: self.truncated && text.is_some(),
            text,
        }
    }
}

impl<'a> Device<'a> for TextCollector {
    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, mode: &DrawMode) {
        let glyphs = run.glyphs();
        let (Some(first), Some(last)) = (glyphs.first(), glyphs.last()) else {
            return;
        };
        let identity = (
            glyphs.len(),
            props.transform * first.transform(),
            props.transform * last.transform(),
        );
        // Fill-and-stroke text arrives twice: once filled, then stroked.
        let repeated = matches!(mode, DrawMode::Stroke(_)) && self.previous_run == Some(identity);
        self.previous_run = Some(identity);
        if repeated {
            return;
        }
        for glyph in glyphs {
            if self.truncated {
                return;
            }
            let Some(unicode) = glyph.as_unicode() else {
                continue;
            };
            let transform = props.transform * glyph.transform();
            let origin = transform * Point::ORIGIN;
            // Glyph space has 1,000 units to the font size.
            let size = (transform * Point::new(0., 1000.) - origin).hypot();
            let advance = match &**glyph {
                Glyph::Outline(outline) => outline.advance_width(),
                Glyph::Type3(_) => None,
            };
            let end = advance
                .filter(|advance| advance.is_finite())
                .map(|advance| transform * Point::new(f64::from(advance), 0.));
            self.separate(origin, size);
            match unicode {
                BfString::Char(character) => self.push(character),
                BfString::String(string) => string.chars().for_each(|c| self.push(c)),
            }
            self.previous = Some(Placement { origin, end, size });
        }
    }

    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

fn micros(duration: Duration) -> u32 {
    u32::try_from(duration.as_micros()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_pdf::{Page, assemble, document, pdf, text_pdf};

    const RED: [u8; 4] = [255, 0, 0, 255];
    const PAPER: [u8; 4] = [255, 255, 255, 255];

    fn pixel(reply: &PageReply, x: u32, y: u32) -> [u8; 4] {
        let image = &reply.image;
        let offset = (y as usize * image.width as usize + x as usize) * 4;
        image.rgba[offset..offset + 4].try_into().unwrap()
    }

    fn page(page: Page) -> PageReply {
        prepare(document(&[page], None), 0).unwrap()
    }

    #[test]
    fn renders_only_the_selected_page_with_its_count_and_pixels() {
        let reply = prepare(pdf(10), 9).unwrap();
        assert_eq!((reply.page, reply.page_count), (9, 10));
        let image = &reply.image;
        assert_eq!(image.format, "PDF page 10");
        assert_eq!((image.width, image.height), (120, 80));
        assert_eq!((image.original_width, image.original_height), (120, 80));
        assert_eq!(image.rgba.len(), 120 * 80 * 4);
        assert_eq!(pixel(&reply, 0, 0), RED);
        assert_eq!(pixel(&reply, 119, 39), RED);
        assert_eq!(pixel(&reply, 0, 40), PAPER);
        assert_eq!(pixel(&reply, 119, 79), PAPER);
        assert_eq!(
            reply.text,
            Ok(PdfPageText {
                text: None,
                truncated: false
            })
        );
    }

    #[test]
    fn paints_an_empty_page_white() {
        let reply = page(Page::new(""));
        assert!(reply.image.rgba.iter().all(|&channel| channel == 255));
    }

    #[test]
    fn uses_the_crop_box_and_the_page_rotation() {
        let cropped = page(Page {
            media_box: [0, 0, 200, 100],
            crop_box: Some([50, 0, 150, 100]),
            ..Page::new("1 0 0 rg 0 0 100 100 re f")
        });
        assert_eq!((cropped.image.width, cropped.image.height), (100, 100));
        assert_eq!(pixel(&cropped, 10, 50), RED);
        assert_eq!(pixel(&cropped, 90, 50), PAPER);

        // The top half is red; /Rotate turns the page clockwise.
        let rotated = |rotate| {
            page(Page {
                rotate,
                ..Page::new("1 0 0 rg 0 40 120 40 re f")
            })
        };
        let quarter = rotated(90);
        assert_eq!((quarter.image.width, quarter.image.height), (80, 120));
        assert_eq!(
            (quarter.image.original_width, quarter.image.original_height),
            (80, 120)
        );
        assert_eq!(pixel(&quarter, 79, 60), RED);
        assert_eq!(pixel(&quarter, 0, 60), PAPER);
        let half = rotated(180);
        assert_eq!((half.image.width, half.image.height), (120, 80));
        assert_eq!(pixel(&half, 60, 79), RED);
        assert_eq!(pixel(&half, 60, 0), PAPER);
        let three_quarters = rotated(270);
        assert_eq!(pixel(&three_quarters, 0, 60), RED);
        assert_eq!(pixel(&three_quarters, 79, 60), PAPER);
    }

    #[test]
    fn fits_pages_within_the_preview_edge_and_refuses_unreasonable_ones() {
        let sized = |media_box, rotate| {
            prepare(
                document(
                    &[Page {
                        media_box,
                        rotate,
                        ..Page::new("1 0 0 rg 0 0 100000 100000 re f")
                    }],
                    None,
                ),
                0,
            )
        };
        let wide = sized([0, 0, 1200, 800], 0).unwrap();
        assert_eq!((wide.image.width, wide.image.height), (1000, 667));
        assert_eq!(
            (wide.image.original_width, wide.image.original_height),
            (1200, 800)
        );
        assert_eq!(pixel(&wide, 999, 666), RED);
        let tall = sized([0, 0, 1200, 800], 90).unwrap();
        assert_eq!((tall.image.width, tall.image.height), (667, 1000));
        assert_eq!(
            sized([0, 0, 32000, 32000], 0).unwrap_err(),
            ErrorKind::PagePixelLimit
        );
        assert_eq!(
            sized([0, 0, 40000, 10], 0).unwrap_err(),
            ErrorKind::PageEdgeLimit
        );
    }

    #[test]
    fn leaves_annotations_undrawn() {
        let appearance = "1 0 0 rg 0 0 120 80 re f";
        let reply = prepare(
            assemble(
                &[
                    b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
                    b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
                    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 80] /Annots [4 0 R] /Contents 6 0 R >>".to_vec(),
                    b"<< /Type /Annot /Subtype /Square /Rect [0 0 120 80] /AP << /N 5 0 R >> >>".to_vec(),
                    format!(
                        "<< /Type /XObject /Subtype /Form /BBox [0 0 120 80] /Length {} >>\nstream\n{appearance}\nendstream",
                        appearance.len()
                    )
                    .into_bytes(),
                    b"<< /Length 0 >>\nstream\n\nendstream".to_vec(),
                ],
                "",
            ),
            0,
        )
        .unwrap();
        assert_eq!(pixel(&reply, 60, 40), PAPER);
    }

    #[test]
    fn opens_an_empty_user_password_and_refuses_a_real_one() {
        let red = Page::new("1 0 0 rg 0 40 120 40 re f");
        let reply = prepare(document(std::slice::from_ref(&red), Some(b"")), 0).unwrap();
        assert_eq!(pixel(&reply, 0, 0), RED);
        assert_eq!(pixel(&reply, 0, 79), PAPER);
        assert_eq!(
            prepare(document(&[red], Some(b"secret")), 0).unwrap_err(),
            ErrorKind::Locked
        );
    }

    #[test]
    fn refuses_invalid_documents_and_pages() {
        let refused = |bytes: Vec<u8>, page| prepare(bytes, page).unwrap_err();
        assert_eq!(
            refused(b"%PDF-1.7\ncorrupt".to_vec(), 0),
            ErrorKind::Corrupt
        );
        assert_eq!(refused(b"not a pdf".to_vec(), 0), ErrorKind::NotPdf);
        let mut large = pdf(1);
        large.resize(MAX_INPUT_BYTES + 1, b' ');
        assert_eq!(refused(large, 0), ErrorKind::InputTooLarge);
        assert_eq!(refused(pdf(2), MAX_PDF_PAGES), ErrorKind::PageBound);
        assert_eq!(
            refused(pdf(2), 2),
            ErrorKind::PageOutOfRange { page_count: 2 }
        );
        assert_eq!(refused(document(&[], None), 0), ErrorKind::NoPages);
        // Inline page dictionaries keep 100,001 pages small.
        let kids = "<< /Type /Page >> ".repeat(MAX_PDF_PAGES + 1);
        let many = assemble(
            &[
                b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
                format!("<< /Type /Pages /MediaBox [0 0 10 10] /Kids [{kids}] >>").into_bytes(),
            ],
            "",
        );
        assert_eq!(refused(many, 0), ErrorKind::TooManyPages);
    }

    #[test]
    fn extracts_the_selected_page_text_or_reports_its_absence() {
        let source = text_pdf(&["Captured first page", "Distinct second page", ""]);
        let text = |bytes: &[u8], page| prepare(bytes.to_vec(), page).unwrap().text.unwrap();
        assert_eq!(
            text(&source, 1),
            PdfPageText {
                text: Some("Distinct second page".into()),
                truncated: false
            }
        );
        assert_eq!(text(&source, 2).text, None);
        assert_eq!(text(&pdf(1), 0).text, None);
        // Lines from baselines, words from gaps, and fill-and-stroke text once.
        let layout = document(
            &[Page::new(
                "BT /F1 12 Tf 10 60 Td (Line one) Tj 0 -14 Td [(Hello) -500 (World)] TJ 2 Tr 0 -14 Td (Once) Tj ET",
            )],
            None,
        );
        assert_eq!(
            text(&layout, 0).text.as_deref(),
            Some("Line one\nHello World\nOnce")
        );
    }

    /// The text of a page that shows `shown` (WinAnsi bytes) on one line.
    fn shown(shown: &[u8]) -> PdfPageText {
        let mut content = b"BT /F1 12 Tf 0 40 Td (".to_vec();
        content.extend_from_slice(shown);
        content.extend_from_slice(b") Tj ET");
        page(Page::new(content)).text.unwrap()
    }

    #[test]
    fn cuts_page_text_at_a_character_boundary_past_the_cap() {
        // One ASCII byte, then two-byte "é" (WinAnsi 0xE9) past the 256 KiB cap.
        let mut bytes = vec![b'a'];
        bytes.extend(std::iter::repeat_n(0xe9, MAX_PDF_PAGE_TEXT_BYTES / 2 + 1));
        let text = shown(&bytes);
        assert!(text.truncated);
        let text = text.text.unwrap();
        assert_eq!(text.len(), MAX_PDF_PAGE_TEXT_BYTES - 1);
        assert!(text.starts_with("aé") && text.ends_with('é'));
    }

    #[test]
    fn ascii_text_fills_the_cap_exactly() {
        let full = shown(&vec![b'a'; MAX_PDF_PAGE_TEXT_BYTES]);
        assert!(!full.truncated);
        assert_eq!(full.text.unwrap().len(), MAX_PDF_PAGE_TEXT_BYTES);
        let over = shown(&vec![b'a'; MAX_PDF_PAGE_TEXT_BYTES + 1]);
        assert!(over.truncated);
        assert_eq!(over.text.unwrap().len(), MAX_PDF_PAGE_TEXT_BYTES);
    }

    #[test]
    fn whitespace_past_the_cap_is_absent_not_truncated() {
        assert_eq!(
            shown(&vec![b' '; MAX_PDF_PAGE_TEXT_BYTES + 1]),
            PdfPageText {
                text: None,
                truncated: false
            }
        );
    }

    #[test]
    fn turns_a_panicking_phase_into_an_error_kind() {
        assert_eq!(
            guarded(|| -> u8 { panic!("injected renderer panic") }),
            Err(ErrorKind::RendererFailed)
        );
        // A text panic keeps the rendered page.
        let reply = prepare_with(pdf(1), 0, |_| panic!("injected text panic")).unwrap();
        assert_eq!(pixel(&reply, 0, 0), RED);
        assert_eq!(reply.text, Err(ErrorKind::RendererFailed));
    }
}
