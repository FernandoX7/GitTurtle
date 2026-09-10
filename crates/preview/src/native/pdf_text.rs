//! Selected-page text from PDFKit's public in-memory document API.
//!
//! Native extraction can allocate internally and is not preemptible. Only the
//! selected page is requested; Rust buffers and the returned UTF-8 are bounded.
use super::*;
use std::ffi::c_char;

#[link(name = "PDFKit", kind = "framework")]
unsafe extern "C" {
    #[link_name = "OBJC_CLASS_$_PDFDocument"]
    static PDF_DOCUMENT_CLASS: c_void;
}
#[link(name = "objc")]
unsafe extern "C" {
    fn sel_registerName(name: *const c_char) -> Ref;
    fn objc_msgSend();
    fn objc_release(object: Ref);
    fn objc_autoreleasePoolPush() -> Ref;
    fn objc_autoreleasePoolPop(pool: Ref);
}
#[repr(C)]
struct Range {
    location: isize,
    length: isize,
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringGetLength(string: Ref) -> isize;
    fn CFStringGetCharacters(string: Ref, range: Range, buffer: *mut u16);
}

struct AutoreleasePool(Ref);
impl AutoreleasePool {
    fn new() -> Self {
        // SAFETY: this private pool is popped on the same worker stack after all
        // native document/page/string uses, including error and cancellation exits.
        Self(unsafe { objc_autoreleasePoolPush() })
    }
}
impl Drop for AutoreleasePool {
    fn drop(&mut self) {
        // SAFETY: the corresponding push belongs to this stack and is popped once.
        unsafe { objc_autoreleasePoolPop(self.0) }
    }
}

pub(crate) fn decode_pdf_page_text(
    bytes: &[u8],
    page_index: usize,
    check: impl Fn() -> Result<()>,
) -> Result<PdfPageText> {
    check()?;
    let _pool = AutoreleasePool::new();
    let data = data(bytes)?;
    // SAFETY: these signatures match PDFKit's public Objective-C selectors.
    // initWithData receives toll-free bridged CFData/NSData and retains its own
    // reference. No URL, action, script, annotation, or external resource API is
    // called. Owned releases the document before data and the autorelease pool.
    unsafe {
        let object: unsafe extern "C" fn(Ref, Ref) -> Ref =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let with_object: unsafe extern "C" fn(Ref, Ref, Ref) -> Ref =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let boolean: unsafe extern "C" fn(Ref, Ref) -> i8 =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let count: unsafe extern "C" fn(Ref, Ref) -> usize =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let indexed: unsafe extern "C" fn(Ref, Ref, usize) -> Ref =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let allocated = object(
            ptr::addr_of!(PDF_DOCUMENT_CLASS).cast(),
            sel_registerName(c"alloc".as_ptr()),
        );
        ensure!(!allocated.is_null(), "Cannot allocate PDF text document");
        // An Objective-C initializer consumes the allocated receiver, including
        // on failure. Retain ownership only of its returned instance.
        let document = Owned::new(
            with_object(
                allocated,
                sel_registerName(c"initWithData:".as_ptr()),
                data.raw,
            ),
            objc_release,
            "Cannot read PDF text; corrupt or unsupported document",
        )?;
        check()?;
        ensure!(
            boolean(document.raw, sel_registerName(c"isLocked".as_ptr())) == 0,
            "PDF is password protected; text extraction is unavailable"
        );
        let page_count = count(document.raw, sel_registerName(c"pageCount".as_ptr()));
        ensure!(page_count > 0, "PDF contains no pages");
        ensure!(
            page_count <= MAX_PDF_PAGES,
            "PDF page count exceeds the 100,000-page navigation bound"
        );
        // pageAtIndex: raises an Objective-C exception for an out-of-range index.
        ensure!(
            page_index < page_count,
            "PDF page {} is outside this document's {page_count} pages",
            page_index.saturating_add(1)
        );
        check()?;
        let page = indexed(
            document.raw,
            sel_registerName(c"pageAtIndex:".as_ptr()),
            page_index,
        );
        ensure!(
            !page.is_null(),
            "PDF page is unavailable for text extraction"
        );
        check()?;
        let string = object(page, sel_registerName(c"string".as_ptr()));
        check()?;
        if string.is_null() {
            return Ok(PdfPageText {
                text: None,
                truncated: false,
            });
        }
        // PDFPage.string returns a borrowed NSString, toll-free bridged to
        // CFString. It stays alive through the surrounding autorelease pool.
        bounded_text(string, &check)
    }
}

unsafe fn bounded_text(string: Ref, check: &impl Fn() -> Result<()>) -> Result<PdfPageText> {
    // SAFETY: caller provides a live NSString/CFString for this stack lifetime.
    let length = usize::try_from(unsafe { CFStringGetLength(string) })?;
    let copied = length.min(MAX_PDF_PAGE_TEXT_BYTES);
    // At most 512 KiB UTF-16 scratch, independent of the native string length.
    let mut utf16 = vec![0u16; copied];
    if copied > 0 {
        // SAFETY: the requested range is inside the validated string length and
        // the output vector has exactly enough initialized UTF-16 code units.
        unsafe {
            CFStringGetCharacters(
                string,
                Range {
                    location: 0,
                    length: copied as isize,
                },
                utf16.as_mut_ptr(),
            );
        }
    }
    let mut truncated = copied < length;
    // Do not turn half of a truncated surrogate pair into replacement text.
    if truncated
        && utf16
            .last()
            .is_some_and(|last| (0xd800..=0xdbff).contains(last))
    {
        utf16.pop();
    }
    check()?;
    let mut text = String::with_capacity(copied.saturating_mul(3).min(MAX_PDF_PAGE_TEXT_BYTES));
    for (index, character) in char::decode_utf16(utf16).enumerate() {
        if index % 4096 == 0 {
            check()?;
        }
        let character = character.unwrap_or(char::REPLACEMENT_CHARACTER);
        if text.len() + character.len_utf8() > MAX_PDF_PAGE_TEXT_BYTES {
            truncated = true;
            break;
        }
        text.push(character);
    }
    text.shrink_to_fit();
    check()?;
    Ok(PdfPageText {
        text: (!text.trim().is_empty()).then_some(text),
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCharacters(
            allocator: Ref,
            characters: *const u16,
            count: isize,
        ) -> Ref;
    }

    fn pdf(texts: &[&str]) -> Vec<u8> {
        let mut output = b"%PDF-1.4\n".to_vec();
        let font = 3 + texts.len() * 2;
        let mut objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            format!(
                "<< /Type /Pages /Count {} /Kids [{}] >>",
                texts.len(),
                (0..texts.len())
                    .map(|i| format!("{} 0 R", 3 + i * 2))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
        ];
        for (index, text) in texts.iter().enumerate() {
            objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 {font} 0 R >> >> /Contents {} 0 R >>", 4 + index * 2));
            let stream = format!("BT /F1 12 Tf 40 740 Td ({text}) Tj ET");
            objects.push(format!(
                "<< /Length {} >>\nstream\n{stream}\nendstream",
                stream.len()
            ));
        }
        objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned());
        let mut offsets = vec![0];
        for (index, object) in objects.iter().enumerate() {
            offsets.push(output.len());
            output.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
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
    fn extracts_only_the_selected_page_and_reports_absent_text() {
        let source = pdf(&["Captured first page", "Distinct second page", ""]);
        let text = crate::decode_pdf_page_text(&source, 1, || Ok(())).unwrap();
        assert_eq!(
            text.text.as_deref().map(str::trim),
            Some("Distinct second page")
        );
        assert!(!text.truncated);
        assert_eq!(
            crate::decode_pdf_page_text(&source, 2, || Ok(()))
                .unwrap()
                .text,
            None
        );
        assert_eq!(
            crate::decode_pdf_page_text(&super::super::tests::pdf(1), 0, || Ok(()))
                .unwrap()
                .text,
            None
        );
    }

    #[test]
    fn refuses_invalid_pages_input_and_cancels_between_native_phases() {
        let source = pdf(&["Text"]);
        assert!(crate::decode_pdf_page_text(&source, 1, || Ok(())).is_err());
        assert!(crate::decode_pdf_page_text(&source, MAX_PDF_PAGES, || Ok(())).is_err());
        assert!(crate::decode_pdf_page_text(b"not pdf", 0, || Ok(())).is_err());
        assert!(crate::decode_pdf_page_text(b"%PDF-1.4\ncorrupt", 0, || Ok(())).is_err());
        assert!(crate::decode_pdf_page_text(&vec![0; MAX_INPUT_BYTES + 1], 0, || Ok(())).is_err());
        for stop in 1..=6 {
            let calls = std::cell::Cell::new(0);
            let result = crate::decode_pdf_page_text(&source, 0, || {
                calls.set(calls.get() + 1);
                ensure!(calls.get() < stop, "cancelled");
                Ok(())
            });
            assert!(result.unwrap_err().to_string().contains("cancelled"));
        }
    }

    #[test]
    fn text_caps_utf8_bytes_without_splitting_characters_or_surrogate_pairs() {
        for original in [
            "a".repeat(MAX_PDF_PAGE_TEXT_BYTES + 3),
            "🦀".repeat(MAX_PDF_PAGE_TEXT_BYTES),
            "水".repeat(MAX_PDF_PAGE_TEXT_BYTES),
            format!("{}🦀tail", "a".repeat(MAX_PDF_PAGE_TEXT_BYTES - 1)),
        ] {
            let utf16: Vec<_> = original.encode_utf16().collect();
            // SAFETY: CFString copies initialized UTF-16 and Owned releases it.
            let native = unsafe {
                Owned::new(
                    CFStringCreateWithCharacters(ptr::null(), utf16.as_ptr(), utf16.len() as isize),
                    CFRelease,
                    "test string",
                )
                .unwrap()
            };
            // SAFETY: this live CFString is valid for the call's duration.
            let extracted = unsafe { bounded_text(native.raw, &|| Ok(())) }.unwrap();
            assert!(extracted.truncated);
            let text = extracted.text.unwrap();
            assert!(text.len() <= MAX_PDF_PAGE_TEXT_BYTES);
            assert!(text.len() >= MAX_PDF_PAGE_TEXT_BYTES - 3);
            assert!(original.starts_with(&text));
            assert!(!text.contains(char::REPLACEMENT_CHARACTER));
        }
    }
}
