//! Selected-page text from PDFKit's public in-memory document API.
//!
//! Native extraction can allocate internally and is not preemptible. Only the
//! selected page is requested; Rust buffers and the returned UTF-8 are bounded.
use super::*;
use objc2::msg_send;
use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::{NSData, NSRange, NSString};
use std::ptr::NonNull;

#[link(name = "PDFKit", kind = "framework")]
unsafe extern "C" {
    #[link_name = "OBJC_CLASS_$_PDFDocument"]
    static PDF_DOCUMENT_CLASS: c_void;
}

pub(crate) fn decode_pdf_page_text(
    bytes: &[u8],
    page_index: usize,
    check: impl Fn() -> Result<()>,
) -> Result<PdfPageText> {
    check()?;
    autoreleasepool(|_| {
        let data = NSData::with_bytes(bytes);
        // SAFETY: `OBJC_CLASS_$_PDFDocument` is the static Objective-C `Class`
        // object exported by `PDFKit.framework`, and these selectors match
        // PDFKit's public Objective-C API (`PDFDocument`, `PDFPage`).
        // `initWithData:` receives an in-memory `NSData` and retains its own
        // reference; no URL, action, script, annotation, or external resource
        // API is called. `Retained` releases Objective-C instances before the
        // surrounding autorelease pool exits.
        let document: Retained<AnyObject> = unsafe {
            let cls = &*(&raw const PDF_DOCUMENT_CLASS).cast::<AnyClass>();
            let allocated: Allocated<AnyObject> = msg_send![cls, alloc];
            let initialized: Option<Retained<AnyObject>> =
                msg_send![allocated, initWithData: &*data];
            initialized.context("Cannot read PDF text; corrupt or unsupported document")?
        };
        check()?;
        // SAFETY: `PDFDocument.isLocked` returns Objective-C `BOOL`.
        let locked: bool = unsafe { msg_send![&*document, isLocked] };
        ensure!(
            !locked,
            "PDF is password protected; text extraction is unavailable"
        );
        // SAFETY: `PDFDocument.pageCount` returns `NSUInteger` (`usize` on 64-bit macOS).
        let page_count: usize = unsafe { msg_send![&*document, pageCount] };
        ensure!(page_count > 0, "PDF contains no pages");
        ensure!(
            page_count <= MAX_PDF_PAGES,
            "PDF page count exceeds the 100,000-page navigation bound"
        );
        // `pageAtIndex:` raises an Objective-C exception for an out-of-range index.
        ensure!(
            page_index < page_count,
            "PDF page {} is outside this document's {}",
            page_index.saturating_add(1),
            crate::metadata::counted(page_count, "page", "pages")
        );
        check()?;
        // SAFETY: `page_index < page_count` is verified above and `PDFDocument.pageAtIndex:`
        // returns an optional `PDFPage *`.
        let page: Retained<AnyObject> = unsafe {
            let page: Option<Retained<AnyObject>> = msg_send![&*document, pageAtIndex: page_index];
            page.context("PDF page is unavailable for text extraction")?
        };
        check()?;
        // SAFETY: `PDFPage.string` returns an optional `NSString *`.
        let string: Option<Retained<NSString>> = unsafe { msg_send![&*page, string] };
        check()?;
        let Some(string) = string else {
            return Ok(PdfPageText {
                text: None,
                truncated: false,
            });
        };
        bounded_text(&string, &check)
    })
}

fn bounded_text(string: &NSString, check: &impl Fn() -> Result<()>) -> Result<PdfPageText> {
    let length = string.len_utf16();
    let copied = length.min(MAX_PDF_PAGE_TEXT_BYTES);
    // At most 512 KiB UTF-16 scratch, independent of the native string length.
    let mut utf16 = vec![0u16; copied];
    if copied > 0 {
        let buffer = NonNull::new(utf16.as_mut_ptr()).expect("non-empty Vec buffer is non-null");
        // SAFETY: `0..copied` is within `string.len_utf16()` and `utf16` has
        // exactly `copied` initialized `u16` code units.
        unsafe {
            string.getCharacters_range(buffer, NSRange::new(0, copied));
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
            objects.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 {font} 0 R >> >> /Contents {} 0 R >>",
                4 + index * 2
            ));
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
            let native = NSString::from_str(&original);
            let extracted = bounded_text(&native, &|| Ok(())).unwrap();
            assert!(extracted.truncated);
            let text = extracted.text.unwrap();
            assert!(text.len() <= MAX_PDF_PAGE_TEXT_BYTES);
            assert!(text.len() >= MAX_PDF_PAGE_TEXT_BYTES - 3);
            assert!(original.starts_with(&text));
            assert!(!text.contains(char::REPLACEMENT_CHARACTER));
        }
    }
}
