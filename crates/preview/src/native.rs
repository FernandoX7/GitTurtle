//! macOS supplied-byte ImageIO and static CoreGraphics PDF rendering.
//!
//! Every retained native reference stays on this worker stack. The public model
//! contains only owned RGBA bytes; no UI-thread decoder or native PDF state.
use super::*;
use std::{ffi::c_void, ptr};

type Ref = *const c_void;
#[repr(C)]
#[derive(Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Size {
    width: f64,
    height: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Rect {
    origin: Point,
    size: Size,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Transform {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    tx: f64,
    ty: f64,
}
fn rect(width: u32, height: u32) -> Rect {
    Rect {
        origin: Point { x: 0., y: 0. },
        size: Size {
            width: f64::from(width),
            height: f64::from(height),
        },
    }
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDataCreate(allocator: Ref, bytes: *const u8, length: isize) -> Ref;
    fn CFRelease(value: Ref);
    fn CFDictionaryCreate(
        allocator: Ref,
        keys: *const Ref,
        values: *const Ref,
        count: isize,
        key_callbacks: Ref,
        value_callbacks: Ref,
    ) -> Ref;
    fn CFDictionaryGetValue(dictionary: Ref, key: Ref) -> Ref;
    fn CFNumberCreate(allocator: Ref, kind: isize, value: *const c_void) -> Ref;
    fn CFNumberGetValue(number: Ref, kind: isize, value: *mut c_void) -> bool;
    fn CFGetTypeID(value: Ref) -> usize;
    fn CFNumberGetTypeID() -> usize;
    static kCFBooleanTrue: Ref;
    static kCFBooleanFalse: Ref;
}
#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    fn CGImageSourceCreateWithData(data: Ref, options: Ref) -> Ref;
    fn CGImageSourceCopyPropertiesAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
    fn CGImageSourceCreateThumbnailAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
    static kCGImageSourceShouldCache: Ref;
    static kCGImageSourceCreateThumbnailFromImageAlways: Ref;
    static kCGImageSourceCreateThumbnailWithTransform: Ref;
    static kCGImageSourceThumbnailMaxPixelSize: Ref;
    static kCGImagePropertyPixelWidth: Ref;
    static kCGImagePropertyPixelHeight: Ref;
    static kCGImagePropertyOrientation: Ref;
}
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGDataProviderCreateWithCFData(data: Ref) -> Ref;
    fn CGDataProviderRelease(provider: Ref);
    fn CGPDFDocumentCreateWithProvider(provider: Ref) -> Ref;
    fn CGPDFDocumentRelease(document: Ref);
    fn CGPDFDocumentIsEncrypted(document: Ref) -> bool;
    fn CGPDFDocumentIsUnlocked(document: Ref) -> bool;
    fn CGPDFDocumentGetNumberOfPages(document: Ref) -> usize;
    fn CGPDFDocumentGetPage(document: Ref, index: usize) -> Ref;
    fn CGPDFPageGetBoxRect(page: Ref, box_type: i32) -> Rect;
    fn CGPDFPageGetRotationAngle(page: Ref) -> i32;
    fn CGPDFPageGetDrawingTransform(
        page: Ref,
        box_type: i32,
        rect: Rect,
        rotate: i32,
        preserve: bool,
    ) -> Transform;
    fn CGColorSpaceCreateDeviceRGB() -> Ref;
    fn CGColorSpaceRelease(space: Ref);
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits: usize,
        row: usize,
        space: Ref,
        info: u32,
    ) -> Ref;
    fn CGContextRelease(context: Ref);
    fn CGContextSetRGBFillColor(context: Ref, r: f64, g: f64, b: f64, a: f64);
    fn CGContextFillRect(context: Ref, rect: Rect);
    fn CGContextConcatCTM(context: Ref, transform: Transform);
    fn CGContextDrawPDFPage(context: Ref, page: Ref);
    fn CGContextDrawImage(context: Ref, rect: Rect, image: Ref);
    fn CGImageGetWidth(image: Ref) -> usize;
    fn CGImageGetHeight(image: Ref) -> usize;
    fn CGImageRelease(image: Ref);
}

struct Owned {
    raw: Ref,
    release: unsafe extern "C" fn(Ref),
}
impl Owned {
    fn new(raw: Ref, release: unsafe extern "C" fn(Ref), message: &str) -> Result<Self> {
        ensure!(!raw.is_null(), "{message}");
        Ok(Self { raw, release })
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: each create/copy reference is released exactly once, after dependent uses.
        unsafe { (self.release)(self.raw) }
    }
}

fn data(bytes: &[u8]) -> Result<Owned> {
    // SAFETY: CFDataCreate copies this bounded slice and accepts a null default allocator.
    unsafe {
        Owned::new(
            CFDataCreate(ptr::null(), bytes.as_ptr(), bytes.len() as isize),
            CFRelease,
            "Cannot copy native preview input",
        )
    }
}
fn dictionary(keys: &[Ref], values: &[Ref]) -> Result<Owned> {
    assert_eq!(keys.len(), values.len());
    // SAFETY: arrays contain live CF references. Null callbacks make this a non-owning
    // dictionary; all referenced keys/values outlive it in the calling stack frame.
    unsafe {
        Owned::new(
            CFDictionaryCreate(
                ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                keys.len() as isize,
                ptr::null(),
                ptr::null(),
            ),
            CFRelease,
            "Cannot create native preview options",
        )
    }
}
fn number(dictionary: Ref, key: Ref) -> Option<i64> {
    // SAFETY: dictionary and key are live CF objects; validate the value's type
    // before asking CFNumber to write a correctly sized signed 64-bit value.
    unsafe {
        let value = CFDictionaryGetValue(dictionary, key);
        if value.is_null() || CFGetTypeID(value) != CFNumberGetTypeID() {
            return None;
        }
        let mut result = 0i64;
        CFNumberGetValue(value, 4, (&mut result as *mut i64).cast()).then_some(result)
    }
}
fn canvas(width: u32, height: u32, rgba: &mut [u8]) -> Result<Owned> {
    ensure!(
        rgba.len() == width as usize * height as usize * 4,
        "Invalid native canvas buffer"
    );
    // SAFETY: the output vector has exact dimensions and outlives the context.
    // Bitmap info is byte-order-32-big | premultiplied-alpha-last (RGBA8).
    unsafe {
        let space = Owned::new(
            CGColorSpaceCreateDeviceRGB(),
            CGColorSpaceRelease,
            "Cannot create RGB color space",
        )?;
        Owned::new(
            CGBitmapContextCreate(
                rgba.as_mut_ptr().cast(),
                width as usize,
                height as usize,
                8,
                width as usize * 4,
                space.raw,
                0x4001,
            ),
            CGContextRelease,
            "Cannot allocate native drawing context",
        )
    }
}

pub(super) fn decode_image(bytes: &[u8], format: &str, max_edge: u32) -> Result<ImagePreview> {
    let data = data(bytes)?;
    // SAFETY: framework option constants have static lifetime. The CFData and
    // non-owning option dictionary stay live while the source is inspected.
    unsafe {
        let options = dictionary(&[kCGImageSourceShouldCache], &[kCFBooleanFalse])?;
        let source = Owned::new(
            CGImageSourceCreateWithData(data.raw, options.raw),
            CFRelease,
            "This macOS ImageIO installation cannot read the AVIF/HEIF data",
        )?;
        let properties = Owned::new(
            CGImageSourceCopyPropertiesAtIndex(source.raw, 0, options.raw),
            CFRelease,
            "Cannot read ImageIO dimensions; corrupt or unsupported codec",
        )?;
        let width = number(properties.raw, kCGImagePropertyPixelWidth)
            .and_then(|v| u32::try_from(v).ok())
            .context("Missing image width")?;
        let height = number(properties.raw, kCGImagePropertyPixelHeight)
            .and_then(|v| u32::try_from(v).ok())
            .context("Missing image height")?;
        check_dimensions(width, height)?;
        let orientation = number(properties.raw, kCGImagePropertyOrientation).unwrap_or(1);
        let (original_width, original_height) = if (5..=8).contains(&orientation) {
            (height, width)
        } else {
            (width, height)
        };
        let edge = i64::from(max_edge.min(width.max(height)));
        let edge_number = Owned::new(
            CFNumberCreate(ptr::null(), 4, (&edge as *const i64).cast()),
            CFRelease,
            "Cannot create image size option",
        )?;
        let options = dictionary(
            &[
                kCGImageSourceShouldCache,
                kCGImageSourceCreateThumbnailFromImageAlways,
                kCGImageSourceCreateThumbnailWithTransform,
                kCGImageSourceThumbnailMaxPixelSize,
            ],
            &[
                kCFBooleanFalse,
                kCFBooleanTrue,
                kCFBooleanTrue,
                edge_number.raw,
            ],
        )?;
        let image = Owned::new(
            CGImageSourceCreateThumbnailAtIndex(source.raw, 0, options.raw),
            CGImageRelease,
            "The installed macOS codec cannot decode this AVIF/HEIF image",
        )?;
        let width = u32::try_from(CGImageGetWidth(image.raw))?;
        let height = u32::try_from(CGImageGetHeight(image.raw))?;
        ensure!(
            width <= max_edge && height <= max_edge,
            "Native decoder exceeded the requested output limit"
        );
        check_dimensions(width, height)?;
        let mut rgba = vec![0; width as usize * height as usize * 4];
        let context = canvas(width, height, &mut rgba)?;
        CGContextDrawImage(context.raw, rect(width, height), image.raw);
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
}

pub(super) fn decode_pdf(bytes: &[u8], check: impl Fn() -> Result<()>) -> Result<DocumentPreview> {
    let data = data(bytes)?;
    // SAFETY: provider/document creation uses only owned CFData. Borrowed pages
    // never escape this document or thread. All dimensions are checked before
    // allocation. CGContext draws page graphics, without PDF actions or scripts.
    unsafe {
        let provider = Owned::new(
            CGDataProviderCreateWithCFData(data.raw),
            CGDataProviderRelease,
            "Cannot create PDF byte provider",
        )?;
        let document = Owned::new(
            CGPDFDocumentCreateWithProvider(provider.raw),
            CGPDFDocumentRelease,
            "Cannot read PDF; corrupt or unsupported document",
        )?;
        ensure!(
            !CGPDFDocumentIsEncrypted(document.raw) || CGPDFDocumentIsUnlocked(document.raw),
            "PDF is password protected; use explicit external inspection to unlock it"
        );
        let page_count = CGPDFDocumentGetNumberOfPages(document.raw);
        ensure!(page_count > 0, "PDF contains no pages");
        let mut pages = Vec::new();
        for index in 1..=page_count.min(MAX_PDF_PAGES) {
            check()?;
            let page = CGPDFDocumentGetPage(document.raw, index);
            ensure!(!page.is_null(), "PDF page {index} is unavailable");
            let bounds = CGPDFPageGetBoxRect(page, 1); // crop box, falling back to media box in CoreGraphics
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
            if CGPDFPageGetRotationAngle(page).rem_euclid(180) == 90 {
                std::mem::swap(&mut original_width, &mut original_height);
            }
            let (width, height) =
                scaled_dimensions(original_width, original_height, PDF_PREVIEW_EDGE);
            let mut rgba = vec![255; width as usize * height as usize * 4];
            let context = canvas(width, height, &mut rgba)?;
            CGContextSetRGBFillColor(context.raw, 1., 1., 1., 1.);
            CGContextFillRect(context.raw, rect(width, height));
            CGContextConcatCTM(
                context.raw,
                CGPDFPageGetDrawingTransform(page, 1, rect(width, height), 0, true),
            );
            CGContextDrawPDFPage(context.raw, page);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pdf(pages: usize) -> Vec<u8> {
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
        let document = super::decode_pdf(&pdf(10), || Ok(())).unwrap();
        assert_eq!(document.page_count, 10);
        assert_eq!(document.pages.len(), MAX_PDF_PAGES);
        let page = &document.pages[0];
        assert_eq!((page.width, page.height), (120, 80));
        assert_eq!(&page.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&page.rgba[(79 * 120 * 4)..][..4], &[255, 255, 255, 255]);
    }
    #[test]
    fn pdf_rejects_corruption_and_honors_cancellation() {
        assert!(super::decode_pdf(b"%PDF-1.7\ncorrupt", || Ok(())).is_err());
        assert!(
            super::decode_pdf(&pdf(2), || bail!("cancelled"))
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
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
