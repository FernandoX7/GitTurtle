//! Bounded container inspection. No extraction, execution, decompression or I/O.
use std::{path::Path, sync::Arc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub format: String,
    pub details: Vec<String>,
    pub source: Option<Arc<str>>,
}

pub fn iso_image_format(bytes: &[u8]) -> Option<&'static str> {
    if bytes.get(4..8)? != b"ftyp" {
        return None;
    }
    let length = u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    if !(16..=4096).contains(&length) || length > bytes.len() {
        return None;
    }
    let brands = std::iter::once(bytes.get(8..12)?).chain(
        bytes[16..length]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|brand| brand.as_slice()),
    );
    let mut heif = false;
    for brand in brands {
        if brand == b"avif" || brand == b"avis" {
            return Some("AVIF");
        }
        if matches!(
            brand,
            b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" | b"msf1"
        ) {
            heif = true;
        }
    }
    heif.then_some("HEIF / HEIC")
}

pub fn is_svg(bytes: &[u8]) -> bool {
    let prefix = &bytes[..bytes.len().min(4096)];
    std::str::from_utf8(prefix).is_ok_and(|text| {
        let text = text.trim_start_matches('\u{feff}').trim_start();
        text.starts_with("<svg") || (text.starts_with("<?xml") && text.contains("<svg"))
    })
}

pub fn is_image(bytes: &[u8]) -> bool {
    image::guess_format(bytes).is_ok()
        || iso_image_format(bytes).is_some()
        || jpeg2000_format(bytes).is_some()
        || is_svg(bytes)
}

pub fn jpeg2000_format(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x00\x00\x00\x0cjP  \r\n\x87\n") {
        Some("JPEG 2000 / JP2")
    } else if bytes.starts_with(b"\xff\x4f\xff\x51") {
        Some("JPEG 2000 / J2K codestream")
    } else {
        None
    }
}

pub fn is_literal_text(bytes: &[u8]) -> bool {
    !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok()
}

pub fn is_rich_path(path: &Path) -> bool {
    matches!(
        extension(path).as_str(),
        "pdf"
            | "wav"
            | "mp3"
            | "m4a"
            | "aac"
            | "flac"
            | "ogg"
            | "mp4"
            | "mov"
            | "webm"
            | "mkv"
            | "avi"
            | "doc"
            | "xls"
            | "ppt"
            | "docx"
            | "xlsx"
            | "pptx"
            | "odt"
            | "ods"
            | "odp"
            | "zip"
            | "tar"
            | "gz"
            | "tgz"
            | "bz2"
            | "xz"
            | "7z"
            | "rar"
            | "ttf"
            | "otf"
            | "woff"
            | "woff2"
    )
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}
fn u16le(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}
fn u32le(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}
fn u16be(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}
fn u32be(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}
fn visible(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(160)])
        .chars()
        .flat_map(|c| {
            if c.is_control() {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

/// Useful bounded metadata with explicit container/codec uncertainty. Filename
/// hints never count as content detection. Exact original bytes stay in the app.
pub fn inspect(bytes: &[u8], name: &Path) -> Metadata {
    let mut result = Metadata {
        format: "Binary / unknown encoding".into(),
        details: vec![format!("{} captured bytes", bytes.len())],
        source: None,
    };
    let ext = extension(name);
    if bytes.starts_with(b"%PDF-") {
        result.format = "PDF".into();
        result.details.push(format!(
            "PDF version {}",
            visible(&bytes[5..bytes.len().min(8)])
        ));
        result.details.push(
            "Visual page comparison; document pages cannot be used for partial staging.".into(),
        );
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        result.format = "WAV audio".into();
        wav(bytes, &mut result.details);
    } else if bytes.starts_with(b"ID3")
        || (bytes.len() >= 2
            && bytes[0] == 0xff
            && bytes[1] & 0xe0 == 0xe0
            && !bytes.starts_with(&[0xff, 0xfe]))
    {
        result.format = "MPEG audio / MP3".into();
        result
            .details
            .push("Audio playback is available through explicit external inspection.".into());
        if bytes.starts_with(b"ID3") && bytes.len() >= 10 {
            result
                .details
                .push(format!("ID3v2.{} metadata header", bytes[3]));
        }
        if let Some(tag) = bytes
            .len()
            .checked_sub(128)
            .and_then(|at| bytes.get(at..))
            .filter(|tag| tag.starts_with(b"TAG"))
        {
            for (label, at) in [("Title", 3), ("Artist", 33), ("Album", 63)] {
                let value = visible(&tag[at..at + 30])
                    .trim_matches(['\0', ' '])
                    .to_owned();
                if !value.is_empty() {
                    result.details.push(format!("{label}: {value}"));
                }
            }
        }
    } else if bytes.get(4..8) == Some(b"ftyp") || bytes.get(4..8) == Some(b"moov") {
        result.format = iso_image_format(bytes)
            .unwrap_or("ISO media / QuickTime container")
            .into();
        if let Some(brand) = bytes.get(8..12) {
            result
                .details
                .push(format!("Container brand: {}", visible(brand)));
        }
        result.details.push("The container does not establish its audio/video codecs. Use external inspection for playback.".into());
    } else if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        result.format = "WebM / Matroska container".into();
        result.details.push("EBML container detected; codec and playback support depend on the external application.".into());
    } else if bytes.starts_with(b"fLaC") {
        result.format = "FLAC audio".into();
    } else if bytes.starts_with(b"OggS") {
        result.format = "Ogg media container".into();
    } else if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        result.format = "ZIP archive / packaged document".into();
        zip(bytes, &mut result);
    } else if bytes.starts_with(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]) {
        result.format = "OLE compound document".into();
        result.details.push("Legacy Office or another OLE container. Macros and document content are not executed or extracted.".into());
    } else if bytes.starts_with(b"\x00\x01\x00\x00")
        || bytes.starts_with(b"OTTO")
        || bytes.starts_with(b"true")
    {
        result.format = if bytes.starts_with(b"OTTO") {
            "OpenType font"
        } else {
            "TrueType font"
        }
        .into();
        font(bytes, &mut result.details);
    } else if bytes.starts_with(b"wOFF") || bytes.starts_with(b"wOF2") {
        result.format = if bytes.starts_with(b"wOF2") {
            "WOFF2 web font"
        } else {
            "WOFF web font"
        }
        .into();
        if let Some(tables) = u16be(bytes, 12) {
            result.details.push(format!(
                "{tables} declared font tables; font is not installed"
            ));
        }
    } else if bytes.starts_with(b"\x1f\x8b") {
        result.format = "Gzip archive".into();
    } else if bytes.starts_with(b"7z\xbc\xaf\x27\x1c") {
        result.format = "7-Zip archive".into();
    } else if bytes.starts_with(b"Rar!\x1a\x07") {
        result.format = "RAR archive".into();
    } else if bytes.starts_with(b"BZh") {
        result.format = "Bzip2 archive".into();
    } else if bytes.starts_with(b"\xfd7zXZ\0") {
        result.format = "XZ archive".into();
    } else if bytes.get(257..262) == Some(b"ustar") {
        result.format = "Tar archive".into();
    } else if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let little = bytes.starts_with(&[0xff, 0xfe]);
        result.format = if little {
            "UTF-16 little-endian source"
        } else {
            "UTF-16 big-endian source"
        }
        .into();
        if bytes.len() <= 2 * 1024 * 1024 && bytes.len().is_multiple_of(2) {
            let units: Vec<u16> = bytes[2..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| {
                    if little {
                        u16::from_le_bytes([pair[0], pair[1]])
                    } else {
                        u16::from_be_bytes([pair[0], pair[1]])
                    }
                })
                .collect();
            match String::from_utf16(&units) {
                Ok(text) => {
                    result.details.push("Decoded source for reading/copying. Original UTF-16 bytes are retained for export; partial staging is unavailable.".into());
                    result.source = Some(text.into());
                }
                Err(_) => result.details.push(
                    "Invalid UTF-16 sequence; no replacement characters have been substituted."
                        .into(),
                ),
            }
        } else {
            result.details.push(
                "Decoded source exceeds the 2 MiB limit or has an incomplete code unit.".into(),
            );
        }
    } else if is_literal_text(bytes) {
        result.format = "UTF-8 source".into();
        if bytes.len() <= 2 * 1024 * 1024 {
            result.source = std::str::from_utf8(bytes).ok().map(Arc::from);
        }
        result
            .details
            .push("Literal UTF-8 source. The filename does not change its encoding.".into());
    } else if !ext.is_empty() {
        result.details.push(format!(
            "Filename hint: .{ext}; content signature is unrecognized or corrupt."
        ));
    }
    result
}

fn wav(bytes: &[u8], details: &mut Vec<String>) {
    let mut at = 12usize;
    let mut rate = None;
    let mut data = None;
    for _ in 0..1024 {
        let Some(header) = bytes.get(at..at.saturating_add(8)) else {
            break;
        };
        let size = u32le(header, 4).unwrap() as usize;
        let Some(end) = at
            .checked_add(8)
            .and_then(|p| p.checked_add(size))
            .filter(|end| *end <= bytes.len())
        else {
            details.push("Truncated WAV chunk; metadata may be incomplete.".into());
            break;
        };
        if &header[..4] == b"fmt " && size >= 16 {
            let data = &bytes[at + 8..end];
            let codec = u16le(data, 0).unwrap();
            details.push(format!(
                "{} channels · {} Hz · {} bits/sample · WAV codec {}",
                u16le(data, 2).unwrap(),
                u32le(data, 4).unwrap(),
                u16le(data, 14).unwrap(),
                codec
            ));
            rate = u32le(data, 8).filter(|r| *r > 0);
        } else if &header[..4] == b"data" {
            data = Some(size);
        }
        at = end.saturating_add(size % 2);
    }
    if let (Some(rate), Some(data)) = (rate, data) {
        details.push(format!(
            "Duration {:.2} seconds from declared byte rate",
            data as f64 / f64::from(rate)
        ));
    }
}

fn zip(bytes: &[u8], result: &mut Metadata) {
    let lower = bytes.len().saturating_sub(65_557);
    let Some(end) = (lower..bytes.len().saturating_sub(21))
        .rev()
        .find(|at| bytes.get(*at..at + 4) == Some(b"PK\x05\x06"))
    else {
        result.details.push(
            "ZIP directory missing or ZIP64 layout unsupported; no archive extraction attempted."
                .into(),
        );
        return;
    };
    let count = u16le(bytes, end + 10).unwrap();
    let Some(mut at) = u32le(bytes, end + 16).map(|n| n as usize) else {
        return;
    };
    result.details.push(format!(
        "{count} declared entries; listing at most 64, without extraction"
    ));
    for _ in 0..usize::from(count).min(64) {
        if bytes.get(at..at.saturating_add(4)) != Some(b"PK\x01\x02") {
            result
                .details
                .push("Truncated or unsupported archive directory.".into());
            break;
        }
        let (Some(name_len), Some(extra_len), Some(comment_len)) = (
            u16le(bytes, at + 28),
            u16le(bytes, at + 30),
            u16le(bytes, at + 32),
        ) else {
            break;
        };
        let Some(name) = bytes.get(at + 46..at + 46 + usize::from(name_len)) else {
            break;
        };
        if name.starts_with(b"word/") {
            result.format = "Word / OOXML package".into();
        } else if name.starts_with(b"xl/") {
            result.format = "Excel / OOXML package".into();
        } else if name.starts_with(b"ppt/") {
            result.format = "PowerPoint / OOXML package".into();
        } else if name == b"content.xml" && result.format == "ZIP archive / packaged document" {
            result.format = "OpenDocument / XML package".into();
        }
        result.details.push(format!(
            "{} · {} uncompressed bytes",
            visible(name),
            u32le(bytes, at + 24).unwrap_or_default()
        ));
        at = at.saturating_add(
            46 + usize::from(name_len) + usize::from(extra_len) + usize::from(comment_len),
        );
    }
    result.details.push(
        "Package contents, formulas, macros and linked resources are not executed or decompressed."
            .into(),
    );
}

fn font(bytes: &[u8], details: &mut Vec<String>) {
    let Some(count) = u16be(bytes, 4) else {
        return;
    };
    details.push(format!(
        "{count} declared font tables; font is not installed"
    ));
    for index in 0..usize::from(count).min(128) {
        let at = 12 + index * 16;
        if bytes.get(at..at + 4) != Some(b"name") {
            continue;
        }
        let (Some(offset), Some(length)) = (u32be(bytes, at + 8), u32be(bytes, at + 12)) else {
            break;
        };
        let Some(table) =
            bytes.get(offset as usize..(offset as usize).saturating_add(length as usize))
        else {
            break;
        };
        let (Some(records), Some(strings)) = (u16be(table, 2), u16be(table, 4)) else {
            break;
        };
        for index in 0..usize::from(records).min(128) {
            let at = 6 + index * 12;
            if u16be(table, at) != Some(3) || u16be(table, at + 6) != Some(4) {
                continue;
            }
            let (Some(length), Some(offset)) = (u16be(table, at + 8), u16be(table, at + 10)) else {
                break;
            };
            let start = usize::from(strings) + usize::from(offset);
            let Some(name) = table.get(start..start + usize::from(length).min(256)) else {
                break;
            };
            let name: Vec<u16> = name
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| u16::from_be_bytes([p[0], p[1]]))
                .collect();
            if let Ok(name) = String::from_utf16(&name) {
                details.push(format!("Font name: {}", visible(name.as_bytes())));
            }
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_content_without_trusting_extension() {
        assert_eq!(inspect(b"%PDF-1.7\n", Path::new("photo.jpg")).format, "PDF");
        assert_eq!(
            inspect(b"const x = 1;", Path::new("photo.png")).format,
            "UTF-8 source"
        );
        assert!(
            inspect(&[0, 1, 2], Path::new("file.docx"))
                .details
                .iter()
                .any(|d| d.contains("unrecognized"))
        );
        assert!(is_svg(b"<?xml version='1.0'?><svg/>"));
        assert!(!is_svg(b"<html><script>test</script></html>"));
    }
    #[test]
    fn checks_iso_brands_and_declared_box_bounds() {
        let bytes = b"\0\0\0\x18ftypmif1\0\0\0\0mif1avif";
        assert_eq!(iso_image_format(bytes), Some("AVIF"));
        assert_eq!(iso_image_format(&bytes[..20]), None);
        assert_eq!(iso_image_format(b"\0\0\0\x14ftypmp42\0\0\0\0mp42"), None);
    }
    #[test]
    fn decodes_utf16_without_silently_replacing_invalid_text() {
        let good = inspect(&[255, 254, b'A', 0, 0xac, 0x20], Path::new("data.txt"));
        assert_eq!(good.source.as_deref(), Some("A€"));
        assert!(
            inspect(&[255, 254, 0, 0xd8], Path::new("bad.txt"))
                .source
                .is_none()
        );
        assert!(
            inspect(&[255, 254, b'A'], Path::new("odd.txt"))
                .source
                .is_none()
        );
    }
    #[test]
    fn all_short_or_corrupt_inputs_are_bounded() {
        for magic in [
            b"RIFF".as_slice(),
            b"PK\x03\x04",
            b"OTTO",
            b"wOF2",
            b"\x1a\x45\xdf\xa3",
            b"ID3",
        ] {
            for n in 0..128 {
                let mut bytes = magic.to_vec();
                bytes.resize(n.max(bytes.len()), 255);
                let _ = inspect(&bytes, Path::new("unknown.bin"));
            }
        }
    }
    #[test]
    fn wav_reports_channels_rate_and_duration() {
        let mut bytes = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0".to_vec();
        bytes.extend_from_slice(&[1, 0, 2, 0]);
        bytes.extend_from_slice(&48000u32.to_le_bytes());
        bytes.extend_from_slice(&192000u32.to_le_bytes());
        bytes.extend_from_slice(&[4, 0, 16, 0]);
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&1920u32.to_le_bytes());
        bytes.resize(bytes.len() + 1920, 0);
        let metadata = inspect(&bytes, Path::new("sound.bin"));
        assert!(
            metadata
                .details
                .iter()
                .any(|line| line.contains("2 channels · 48000 Hz · 16 bits"))
        );
        assert!(
            metadata
                .details
                .iter()
                .any(|line| line.contains("0.01 seconds"))
        );
    }
    #[test]
    fn zip_metadata_lists_entries_without_expanding_or_following_them() {
        let name = b"word/document.xml";
        let mut bytes = b"PK\x03\x04".to_vec();
        let central = bytes.len();
        let mut entry = vec![0u8; 46];
        entry[..4].copy_from_slice(b"PK\x01\x02");
        entry[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
        entry[28..30].copy_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend(entry);
        bytes.extend(name);
        let mut end = vec![0u8; 22];
        end[..4].copy_from_slice(b"PK\x05\x06");
        end[10..12].copy_from_slice(&1u16.to_le_bytes());
        end[16..20].copy_from_slice(&(central as u32).to_le_bytes());
        bytes.extend(end);
        let result = inspect(&bytes, Path::new("wrong.zip"));
        assert_eq!(result.format, "Word / OOXML package");
        assert!(
            result
                .details
                .iter()
                .any(|d| d.contains("4294967295 uncompressed bytes"))
        );
        assert!(result.source.is_none());
        bytes.truncate(12);
        assert!(
            inspect(&bytes, Path::new("wrong.docx"))
                .details
                .iter()
                .any(|d| d.contains("directory missing"))
        );
    }
    #[test]
    fn font_metadata_reads_bounded_name_table() {
        let name: Vec<u8> = "Test Font"
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect();
        let mut bytes = vec![0u8; 28];
        bytes[..4].copy_from_slice(b"OTTO");
        bytes[4..6].copy_from_slice(&1u16.to_be_bytes());
        bytes[12..16].copy_from_slice(b"name");
        bytes[20..24].copy_from_slice(&28u32.to_be_bytes());
        bytes[24..28].copy_from_slice(&((18 + name.len()) as u32).to_be_bytes());
        let mut table = vec![0u8; 18];
        table[2..4].copy_from_slice(&1u16.to_be_bytes());
        table[4..6].copy_from_slice(&18u16.to_be_bytes());
        table[6..8].copy_from_slice(&3u16.to_be_bytes());
        table[12..14].copy_from_slice(&4u16.to_be_bytes());
        table[14..16].copy_from_slice(&(name.len() as u16).to_be_bytes());
        bytes.extend(table);
        bytes.extend(name);
        assert!(
            inspect(&bytes, Path::new("font.bin"))
                .details
                .iter()
                .any(|d| d == "Font name: Test Font")
        );
    }
}
