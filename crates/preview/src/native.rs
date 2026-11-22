//! macOS supplied-byte ImageIO, static CoreGraphics PDF rendering and PDFKit text.
//!
//! Every retained native reference stays on this worker stack. The public model
//! contains only owned pixels/text; no UI-thread decoder or native PDF state.
use super::*;
use objc2_core_foundation::{
    CFBoolean, CFData, CFDictionary, CFNumber, CFRetained, CFType, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGDataProvider, CGImage, CGPDFBox,
    CGPDFDocument, CGPDFPage,
};
use std::ptr::NonNull;

mod pdf_text;
pub(super) use pdf_text::decode_pdf_page_text;

fn rect(width: u32, height: u32) -> CGRect {
    CGRect {
        origin: CGPoint { x: 0., y: 0. },
        size: CGSize {
            width: f64::from(width),
            height: f64::from(height),
        },
    }
}

#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    fn CGImageSourceCreateWithData(
        data: &CFData,
        options: &CFDictionary<CFType, CFType>,
    ) -> Option<NonNull<CFType>>;
    fn CGImageSourceCopyPropertiesAtIndex(
        source: &CFType,
        index: usize,
        options: &CFDictionary<CFType, CFType>,
    ) -> Option<NonNull<CFDictionary<CFType, CFType>>>;
    fn CGImageSourceCreateThumbnailAtIndex(
        source: &CFType,
        index: usize,
        options: &CFDictionary<CFType, CFType>,
    ) -> Option<NonNull<CGImage>>;
    #[cfg(test)]
    fn CGImageSourceCreateImageAtIndex(
        source: &CFType,
        index: usize,
        options: &CFDictionary<CFType, CFType>,
    ) -> Option<NonNull<CGImage>>;
    static kCGImageSourceShouldCache: &'static CFType;
    static kCGImageSourceCreateThumbnailFromImageAlways: &'static CFType;
    static kCGImageSourceCreateThumbnailWithTransform: &'static CFType;
    static kCGImageSourceThumbnailMaxPixelSize: &'static CFType;
    static kCGImagePropertyPixelWidth: &'static CFType;
    static kCGImagePropertyPixelHeight: &'static CFType;
    static kCGImagePropertyOrientation: &'static CFType;
}

fn number(dictionary: &CFDictionary<CFType, CFType>, key: &CFType) -> Option<i64> {
    dictionary
        .get(key)
        .and_then(|value| value.downcast::<CFNumber>().ok())
        .and_then(|number| number.as_i64())
}

fn canvas(width: u32, height: u32, rgba: &mut [u8]) -> Result<CFRetained<CGContext>> {
    ensure!(
        rgba.len() == width as usize * height as usize * 4,
        "Invalid native canvas buffer"
    );
    let space = CGColorSpace::new_device_rgb().context("Cannot create RGB color space")?;
    // SAFETY: the output slice has exact `width * height * 4` byte length and
    // callers drop the returned `CGContext` before reading or dropping `rgba`.
    // Bitmap info 0x4001 is byte-order-32-big | premultiplied-alpha-last (RGBA8).
    unsafe {
        CGBitmapContextCreate(
            rgba.as_mut_ptr().cast(),
            width as usize,
            height as usize,
            8,
            width as usize * 4,
            Some(&space),
            0x4001,
        )
    }
    .context("Cannot allocate native drawing context")
}

pub(super) fn decode_image(bytes: &[u8], format: &str, max_edge: u32) -> Result<ImagePreview> {
    // Some ImageIO versions recognize JPEG 2000 but omit pixel dimensions from
    // their property dictionary. Read and bound its mandatory header before
    // native decoding; the codec still owns color interpretation and pixels.
    let header_dimensions = jpeg2000::dimensions(bytes)
        .with_context(|| format!("Invalid {format} source dimensions"))?;
    let data = CFData::from_bytes(bytes);
    let cf_false: &CFType = CFBoolean::new(false).as_ref();
    let cf_true: &CFType = CFBoolean::new(true).as_ref();
    // SAFETY: ImageIO framework statics have static lifetime; every non-null
    // Create/Copy return transfers ownership to `CFRetained::from_raw`.
    let (source, properties, metadata_width, metadata_height, orientation) = unsafe {
        let options =
            CFDictionary::<CFType, CFType>::from_slices(&[kCGImageSourceShouldCache], &[cf_false]);
        let source = CGImageSourceCreateWithData(&data, &options)
            .map(|raw| CFRetained::from_raw(raw))
            .context("This macOS ImageIO installation cannot read this image format")?;
        let properties = CGImageSourceCopyPropertiesAtIndex(&source, 0, &options)
            .map(|raw| CFRetained::from_raw(raw))
            .context("Cannot read ImageIO dimensions; corrupt or unsupported codec")?;
        let metadata_width =
            number(&properties, kCGImagePropertyPixelWidth).and_then(|v| u32::try_from(v).ok());
        let metadata_height =
            number(&properties, kCGImagePropertyPixelHeight).and_then(|v| u32::try_from(v).ok());
        let orientation = number(&properties, kCGImagePropertyOrientation).unwrap_or(1);
        (
            source,
            properties,
            metadata_width,
            metadata_height,
            orientation,
        )
    };
    drop(properties);
    let (width, height) = if let Some(dimensions) = header_dimensions {
        dimensions.image_dimensions(metadata_width, metadata_height)?
    } else {
        (
            metadata_width.with_context(|| format!("Missing {format} image width"))?,
            metadata_height.with_context(|| format!("Missing {format} image height"))?,
        )
    };
    check_dimensions(width, height)?;
    let (original_width, original_height) = if (5..=8).contains(&orientation) {
        (height, width)
    } else {
        (width, height)
    };
    let edge_number = CFNumber::new_i64(i64::from(max_edge.min(width.max(height))));
    let edge_ref: &CFType = edge_number.as_ref();
    // SAFETY: `source` and `options` are live retained CF objects; a non-null
    // `CGImage` returned from CreateThumbnail transfers ownership to `CFRetained`.
    let image = unsafe {
        let options = CFDictionary::<CFType, CFType>::from_slices(
            &[
                kCGImageSourceShouldCache,
                kCGImageSourceCreateThumbnailFromImageAlways,
                kCGImageSourceCreateThumbnailWithTransform,
                kCGImageSourceThumbnailMaxPixelSize,
            ],
            &[cf_false, cf_true, cf_true, edge_ref],
        );
        CGImageSourceCreateThumbnailAtIndex(&source, 0, &options)
            .map(|raw| CFRetained::from_raw(raw))
            .with_context(|| {
                format!("The installed macOS codec cannot decode this {format} image")
            })?
    };
    let width = u32::try_from(CGImage::width(Some(&image)))?;
    let height = u32::try_from(CGImage::height(Some(&image)))?;
    ensure!(
        width <= max_edge && height <= max_edge,
        "Native decoder exceeded the requested output limit"
    );
    check_dimensions(width, height)?;
    let mut rgba = vec![0; width as usize * height as usize * 4];
    let context = canvas(width, height, &mut rgba)?;
    CGContext::draw_image(Some(&context), rect(width, height), Some(&image));
    drop(context);
    unpremultiply(&mut rgba);
    Ok(ImagePreview {
        width,
        height,
        original_width,
        original_height,
        rgba,
        format: format!("{format} · first image · macOS codec"),
    })
}

/// Independently probe direct ImageIO decoding of the fixed 64 × 64 raw J2K
/// fixture. This test-only helper cannot receive arbitrary/full-sized images.
#[cfg(test)]
pub(super) fn raw_jpeg2000_fixture_is_supported() -> Result<bool> {
    const FIXTURE: &[u8; 548] = include_bytes!("../tests/fixtures/half-red-blue.j2k");
    let data = CFData::from_bytes(FIXTURE);
    let cf_false: &CFType = CFBoolean::new(false).as_ref();
    // SAFETY: only the compiled, known-small fixture reaches this direct decode;
    // every Create return transfers ownership to `CFRetained::from_raw`.
    let image = unsafe {
        let options =
            CFDictionary::<CFType, CFType>::from_slices(&[kCGImageSourceShouldCache], &[cf_false]);
        let source = CGImageSourceCreateWithData(&data, &options)
            .map(|raw| CFRetained::from_raw(raw))
            .context("Cannot create the independent raw JPEG 2000 capability source")?;
        let Some(raw_image) = CGImageSourceCreateImageAtIndex(&source, 0, &options) else {
            return Ok(false);
        };
        CFRetained::from_raw(raw_image)
    };
    ensure!(
        CGImage::width(Some(&image)) == 64 && CGImage::height(Some(&image)) == 64,
        "The raw JPEG 2000 capability fixture must decode to 64 × 64 pixels"
    );
    Ok(true)
}

pub(super) fn decode_pdf_page(
    bytes: &[u8],
    page_index: usize,
    check: impl Fn() -> Result<()>,
) -> Result<DocumentPreview> {
    let data = CFData::from_bytes(bytes);
    let provider =
        CGDataProvider::with_cf_data(Some(&data)).context("Cannot create PDF byte provider")?;
    let document = CGPDFDocument::with_provider(Some(&provider))
        .context("Cannot read PDF; corrupt or unsupported document")?;
    ensure!(
        !CGPDFDocument::is_encrypted(Some(&document))
            || CGPDFDocument::is_unlocked(Some(&document)),
        "PDF is password protected; use explicit external inspection to unlock it"
    );
    let page_count = CGPDFDocument::number_of_pages(Some(&document));
    ensure!(page_count > 0, "PDF contains no pages");
    ensure!(
        page_count <= MAX_PDF_PAGES,
        "PDF page count exceeds the 100,000-page navigation bound"
    );
    ensure!(
        page_index < page_count,
        "PDF page {} is outside this document's {}",
        page_index.saturating_add(1),
        crate::metadata::counted(page_count, "page", "pages")
    );
    let mut pages = Vec::new();
    {
        let index = page_index + 1;
        check()?;
        let page = CGPDFDocument::page(Some(&document), index)
            .with_context(|| format!("PDF page {index} is unavailable"))?;
        // Crop box, falling back to media box in CoreGraphics.
        let bounds = CGPDFPage::box_rect(Some(&page), CGPDFBox::CropBox);
        ensure!(
            bounds.size.width.is_finite()
                && bounds.size.height.is_finite()
                && bounds.size.width > 0.
                && bounds.size.height > 0.,
            "Invalid PDF page dimensions"
        );
        let (mut original_width, mut original_height) = (
            bounds.size.width.ceil() as u32,
            bounds.size.height.ceil() as u32,
        );
        check_dimensions(original_width, original_height)?;
        if CGPDFPage::rotation_angle(Some(&page)).rem_euclid(180) == 90 {
            std::mem::swap(&mut original_width, &mut original_height);
        }
        let (width, height) = scaled_dimensions(original_width, original_height, PDF_PREVIEW_EDGE);
        let mut rgba = vec![255; width as usize * height as usize * 4];
        let context = canvas(width, height, &mut rgba)?;
        CGContext::set_rgb_fill_color(Some(&context), 1., 1., 1., 1.);
        CGContext::fill_rect(Some(&context), rect(width, height));
        let transform = CGPDFPage::drawing_transform(
            Some(&page),
            CGPDFBox::CropBox,
            rect(width, height),
            0,
            true,
        );
        CGContext::concat_ctm(Some(&context), transform);
        CGContext::draw_pdf_page(Some(&context), Some(&page));
        drop(context);
        check()?;
        pages.push(ImagePreview {
            width,
            height,
            original_width,
            original_height,
            rgba,
            format: format!("PDF page {index}"),
        });
    }
    Ok(DocumentPreview { pages, page_count })
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn pdf(pages: usize) -> Vec<u8> {
        let mut output = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0usize];
        let mut objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            format!(
                "<< /Type /Pages /Count {pages} /Kids [{}] >>",
                (0..pages)
                    .map(|i| format!("{} 0 R", 3 + i * 2))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
        ];
        for i in 0..pages {
            objects.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 80] /Contents {} 0 R >>",
                4 + i * 2
            ));
            let stream = "1 0 0 rg 0 40 120 40 re f";
            objects.push(format!(
                "<< /Length {} >>\nstream\n{stream}\nendstream",
                stream.len()
            ));
        }
        for (i, object) in objects.iter().enumerate() {
            offsets.push(output.len());
            output.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
        }
        let xref = output.len();
        output.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes(),
        );
        for offset in &offsets[1..] {
            output.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        output.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                offsets.len()
            )
            .as_bytes(),
        );
        output
    }
    #[test]
    fn pdf_renders_supplied_pages_with_bounds_and_orientation() {
        let document = super::decode_pdf_page(&pdf(10), 9, || Ok(())).unwrap();
        assert_eq!(document.page_count, 10);
        assert_eq!(document.pages.len(), 1);
        assert_eq!(document.pages[0].format, "PDF page 10");
        let page = &document.pages[0];
        assert_eq!((page.width, page.height), (120, 80));
        assert_eq!(&page.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&page.rgba[(79 * 120 * 4)..][..4], &[255, 255, 255, 255]);
    }
    #[test]
    fn pdf_rejects_corruption_and_honors_cancellation() {
        assert!(super::decode_pdf_page(b"%PDF-1.7\ncorrupt", 0, || Ok(())).is_err());
        assert!(
            super::decode_pdf_page(&pdf(2), 0, || bail!("cancelled"))
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
        assert!(super::decode_pdf_page(&pdf(2), 2, || Ok(())).is_err());
    }
    #[test]
    fn pdf_caps_output_and_rejects_unreasonable_page_geometry() {
        let source = String::from_utf8(pdf(1))
            .unwrap()
            .replace("120 80]", "1200 800]");
        let preview = crate::decode_pdf(source.as_bytes(), || Ok(())).unwrap();
        assert_eq!(
            (preview.pages[0].width, preview.pages[0].height),
            (1000, 667)
        );
        let huge = String::from_utf8(pdf(1))
            .unwrap()
            .replace("120 80]", "32000 32000]");
        assert!(
            crate::decode_pdf(huge.as_bytes(), || Ok(()))
                .unwrap_err()
                .to_string()
                .contains("pixel limit")
        );
        assert!(
            crate::decode_pdf(&vec![0; MAX_INPUT_BYTES + 1], || Ok(()))
                .unwrap_err()
                .to_string()
                .contains("input limit")
        );
    }
    #[test]
    fn imageio_preserves_pixel_orientation_and_alpha() {
        let pixels =
            image::RgbaImage::from_raw(1, 2, vec![255, 0, 0, 255, 0, 0, 255, 128]).unwrap();
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(pixels)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        let image = super::decode_image(&bytes.into_inner(), "test", 10).unwrap();
        assert_eq!((image.width, image.height), (1, 2));
        assert_eq!(&image.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&image.rgba[4..], &[0, 0, 255, 128]);
    }
    #[test]
    fn imageio_decodes_bounded_avif_and_heic_by_content() {
        for bytes in [
            include_bytes!("../tests/fixtures/half-red-blue.heic").as_slice(),
            include_bytes!("../tests/fixtures/half-red-blue.avif").as_slice(),
        ] {
            let preview = crate::decode_image(bytes, "misleading.txt", 16).unwrap();
            assert_eq!((preview.original_width, preview.original_height), (64, 64));
            assert_eq!((preview.width, preview.height), (16, 16));
            assert!(preview.rgba[0] > 180 && preview.rgba[2] < 80);
            let last = preview.rgba.len() - 4;
            assert!(preview.rgba[last] < 80 && preview.rgba[last + 2] > 180);
            assert!(crate::decode_image(&bytes[..20], "image.heic", 16).is_err());
        }
    }
}
