//! JPEG 2000 source dimensions, independent of optional ImageIO properties.
//!
//! Inspect only JP2 box framing and the mandatory codestream SIZ marker (ITU-T
//! T.800 Annexes A/I). Do not decode pixels or infer color space from components.
use super::*;

const JP2_SIGNATURE: &[u8] = b"\x00\x00\x00\x0cjP  \r\n\x87\n";
const CODESTREAM_SIGNATURE: &[u8] = b"\xff\x4f\xff\x51";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Dimensions {
    grid: (u32, u32),
    default: (u32, u32),
}

impl Dimensions {
    pub(super) fn image_dimensions(
        self,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Result<(u32, u32)> {
        let size = (
            width.unwrap_or(self.default.0),
            height.unwrap_or(self.default.1),
        );
        // Component sampling/rendering can make the image smaller than IHDR's
        // reference grid. Existing ImageIO properties take precedence, within
        // the independently checked source bounds (T.800 I.5.3.1.1).
        ensure!(
            size.0 <= self.grid.0 && size.1 <= self.grid.1,
            "JPEG 2000 ImageIO dimensions exceed the source reference grid"
        );
        check_dimensions(size.0, size.1)?;
        Ok(size)
    }
}

pub(super) fn dimensions(bytes: &[u8]) -> Result<Option<Dimensions>> {
    if bytes.starts_with(CODESTREAM_SIGNATURE) {
        return codestream_dimensions(bytes).map(Some);
    }
    if !bytes.starts_with(JP2_SIGNATURE) {
        return Ok(None);
    }
    let mut boxes = &bytes[JP2_SIGNATURE.len()..];
    let (kind, file_type) = next_box(&mut boxes)?;
    ensure!(
        &kind == b"ftyp" && file_type.len() >= 8 && file_type.len().is_multiple_of(4),
        "Invalid JPEG 2000 file type box"
    );
    // JPX shares the signature but may use per-codestream headers and image
    // composition. Keep it on the existing ImageIO property path; its first
    // codestream is not necessarily the rendered image.
    if &file_type[..4] != b"jp2 " {
        return Ok(None);
    }
    let mut header_dimensions = None;
    while !boxes.is_empty() {
        let (kind, payload) = next_box(&mut boxes)?;
        match &kind {
            b"jp2h" => {
                let mut children = payload;
                while !children.is_empty() {
                    let (kind, payload) = next_box(&mut children)?;
                    if &kind == b"ihdr" {
                        ensure!(header_dimensions.is_none(), "Duplicate JP2 image header");
                        ensure!(payload.len() == 14, "Invalid JP2 image header length");
                        let size = (be_u32(&payload[4..8]), be_u32(&payload[..4]));
                        check_dimensions(size.0, size.1)?;
                        header_dimensions = Some(size);
                    }
                }
            }
            b"jp2c" => {
                let size = codestream_dimensions(payload)?;
                ensure!(
                    header_dimensions == Some(size.grid),
                    "JP2 image header and codestream dimensions disagree or are missing"
                );
                return Ok(Some(size));
            }
            _ => {}
        }
    }
    bail!("Missing JP2 codestream")
}

fn next_box<'a>(remaining: &mut &'a [u8]) -> Result<([u8; 4], &'a [u8])> {
    let bytes = *remaining;
    ensure!(bytes.len() >= 8, "Truncated JP2 box header");
    let kind = bytes[4..8].try_into().unwrap();
    let (length, header_length) = match be_u32(&bytes[..4]) {
        0 => (bytes.len(), 8), // A final box may extend through EOF.
        1 => {
            ensure!(bytes.len() >= 16, "Truncated extended JP2 box header");
            let length = u64::from_be_bytes(bytes[8..16].try_into().unwrap());
            (
                usize::try_from(length).context("JP2 box length overflow")?,
                16,
            )
        }
        length => (length as usize, 8),
    };
    ensure!(
        length >= header_length && length <= bytes.len(),
        "Invalid or truncated JP2 box length"
    );
    *remaining = &bytes[length..];
    Ok((kind, &bytes[header_length..length]))
}

fn codestream_dimensions(bytes: &[u8]) -> Result<Dimensions> {
    ensure!(
        bytes.starts_with(CODESTREAM_SIGNATURE) && bytes.len() >= 42,
        "Missing or truncated JPEG 2000 SIZ marker"
    );
    let segment_length = usize::from(u16::from_be_bytes(bytes[4..6].try_into().unwrap()));
    let components = usize::from(u16::from_be_bytes(bytes[40..42].try_into().unwrap()));
    ensure!(
        components > 0 && segment_length == 38 + 3 * components,
        "Invalid JPEG 2000 SIZ component table"
    );
    let segment_end = 4 + segment_length;
    ensure!(segment_end <= bytes.len(), "Truncated JPEG 2000 SIZ marker");
    let x_size = be_u32(&bytes[8..12]);
    let y_size = be_u32(&bytes[12..16]);
    let x_origin = be_u32(&bytes[16..20]);
    let y_origin = be_u32(&bytes[20..24]);
    let width = x_size
        .checked_sub(x_origin)
        .context("Invalid JPEG 2000 image origin")?;
    let height = y_size
        .checked_sub(y_origin)
        .context("Invalid JPEG 2000 image origin")?;
    check_dimensions(width, height)?;
    let tile_width = be_u32(&bytes[24..28]);
    let tile_height = be_u32(&bytes[28..32]);
    let tile_x = be_u32(&bytes[32..36]);
    let tile_y = be_u32(&bytes[36..40]);
    ensure!(
        tile_width > 0
            && tile_height > 0
            && tile_x <= x_origin
            && tile_y <= y_origin
            && u64::from(tile_x) + u64::from(tile_width) > u64::from(x_origin)
            && u64::from(tile_y) + u64::from(tile_height) > u64::from(y_origin),
        "Invalid JPEG 2000 tile geometry"
    );
    let mut sampling = 0;
    for component in bytes[42..segment_end].as_chunks::<3>().0 {
        ensure!(
            component[1] > 0 && component[2] > 0,
            "Invalid JPEG 2000 component sampling"
        );
        sampling = gcd(
            gcd(sampling, u32::from(component[1])),
            u32::from(component[2]),
        );
    }
    // T.800 I.5.3.1.1: default image samples use the GCD of all horizontal
    // and vertical component sampling factors, including nonzero origins.
    let default = (
        x_size.div_ceil(sampling) - x_origin.div_ceil(sampling),
        y_size.div_ceil(sampling) - y_origin.div_ceil(sampling),
    );
    check_dimensions(default.0, default.1)?;
    Ok(Dimensions {
        grid: (width, height),
        default,
    })
}

fn gcd(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}

fn be_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    const JP2: &[u8] = include_bytes!("../tests/fixtures/half-red-blue.jp2");
    const J2K: &[u8] = include_bytes!("../tests/fixtures/half-red-blue.j2k");

    #[test]
    fn jpeg2000_dimensions_use_codestream_and_validate_container_header() {
        assert_eq!(dimensions(JP2).unwrap().unwrap().default, (64, 64));
        assert_eq!(dimensions(J2K).unwrap().unwrap().default, (64, 64));
        assert_eq!(dimensions(b"unrelated bytes").unwrap(), None);
        let mut changed_header = JP2.to_vec();
        let ihdr = JP2.windows(4).position(|kind| kind == b"ihdr").unwrap();
        changed_header[ihdr + 4..ihdr + 8].copy_from_slice(&32u32.to_be_bytes());
        assert!(
            dimensions(&changed_header)
                .unwrap_err()
                .to_string()
                .contains("disagree")
        );

        // The fixture's last jp2c has length zero. Also accept ordinary and
        // extended box lengths without reading beyond the captured container.
        let jp2c = JP2.windows(4).position(|kind| kind == b"jp2c").unwrap() - 4;
        let mut explicit = JP2.to_vec();
        explicit[jp2c..jp2c + 4].copy_from_slice(&((JP2.len() - jp2c) as u32).to_be_bytes());
        assert_eq!(dimensions(&explicit).unwrap().unwrap().default, (64, 64));
        let mut extended = JP2[..jp2c].to_vec();
        extended.extend_from_slice(b"\0\0\0\x01jp2c");
        extended.extend_from_slice(&((J2K.len() + 16) as u64).to_be_bytes());
        extended.extend_from_slice(J2K);
        assert_eq!(dimensions(&extended).unwrap().unwrap().default, (64, 64));
        assert!(dimensions(&extended[..extended.len() - 1]).is_err());
        let mut jpx = JP2.to_vec();
        jpx[20..24].copy_from_slice(b"jpx ");
        assert_eq!(dimensions(&jpx).unwrap(), None);
    }

    #[test]
    fn jpeg2000_dimensions_reject_truncated_and_invalid_headers() {
        for end in 4..51 {
            assert!(dimensions(&J2K[..end]).is_err(), "SIZ prefix {end}");
        }
        for (offset, replacement) in [(4, vec![0, 38]), (40, vec![0, 0]), (43, vec![0])] {
            let mut changed = J2K.to_vec();
            changed[offset..offset + replacement.len()].copy_from_slice(&replacement);
            assert!(dimensions(&changed).is_err(), "SIZ field {offset}");
        }
        for box_length in [1u32, 2, 7, u32::MAX] {
            let mut changed = JP2.to_vec();
            changed[12..16].copy_from_slice(&box_length.to_be_bytes());
            assert!(dimensions(&changed).is_err(), "Box length {box_length}");
        }
    }

    #[test]
    fn jpeg2000_dimensions_enforce_source_bounds_and_reference_grid_origins() {
        for (offset, value) in [
            (8, 0),
            (8, MAX_SOURCE_EDGE + 1),
            (12, MAX_SOURCE_EDGE + 1),
            (16, 65),
            (24, 0),
            (32, 1),
        ] {
            let mut changed = J2K.to_vec();
            changed[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
            assert!(dimensions(&changed).is_err(), "SIZ field {offset}: {value}");
        }
        let mut excessive_pixels = J2K.to_vec();
        excessive_pixels[8..12].copy_from_slice(&8_000u32.to_be_bytes());
        excessive_pixels[12..16].copy_from_slice(&8_000u32.to_be_bytes());
        assert!(dimensions(&excessive_pixels).is_err());
        let mut cropped_grid = J2K.to_vec();
        cropped_grid[16..20].copy_from_slice(&16u32.to_be_bytes());
        cropped_grid[20..24].copy_from_slice(&8u32.to_be_bytes());
        assert_eq!(
            dimensions(&cropped_grid).unwrap().unwrap().default,
            (48, 56)
        );
    }

    #[test]
    fn jpeg2000_dimensions_preserve_subsampling_and_check_native_metadata() {
        let mut subsampled = J2K.to_vec();
        for component in subsampled[42..51].as_chunks_mut::<3>().0 {
            component[1] = 2;
            component[2] = 2;
        }
        subsampled[16..20].copy_from_slice(&1u32.to_be_bytes());
        subsampled[20..24].copy_from_slice(&3u32.to_be_bytes());
        let size = dimensions(&subsampled).unwrap().unwrap();
        assert_eq!(size.grid, (63, 61));
        assert_eq!(size.image_dimensions(None, None).unwrap(), (31, 30));
        assert_eq!(size.image_dimensions(Some(63), Some(61)).unwrap(), (63, 61));
        assert_eq!(size.image_dimensions(None, Some(30)).unwrap(), (31, 30));
        assert!(size.image_dimensions(Some(64), Some(61)).is_err());
        assert!(size.image_dimensions(Some(63), Some(62)).is_err());
        assert!(size.image_dimensions(Some(0), None).is_err());
    }
}
