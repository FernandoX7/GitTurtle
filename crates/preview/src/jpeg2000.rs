//! JPEG 2000 source dimensions, independent of optional ImageIO properties,
//! and the confined Linux helper's preflight before `hayro-jpeg2000` decodes.
//!
//! Inspect only JP2 box framing, the header boxes and the mandatory codestream
//! SIZ marker (ITU-T T.800 Annexes A, B and I). Do not decode pixels or infer
//! color space from components.
use super::*;

const JP2_SIGNATURE: &[u8] = b"\x00\x00\x00\x0cjP  \r\n\x87\n";
const CODESTREAM_SIGNATURE: &[u8] = b"\xff\x4f\xff\x51";

#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Dimensions {
    grid: (u32, u32),
    default: (u32, u32),
}

#[cfg(any(target_os = "macos", test))]
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

#[cfg(any(target_os = "macos", test))]
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

#[cfg(any(target_os = "macos", test))]
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

#[cfg(any(target_os = "macos", test))]
fn gcd(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}

fn be_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().unwrap())
}

/// The most tiles a codestream may declare: SOT's tile index is 16 bits, and
/// `hayro-jpeg2000` allocates its tile vector from the declared count (#1558).
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
const MAX_TILES: u64 = 65_535;

/// What the confined helper's JPEG 2000 preflight established before decoding.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Preflight {
    /// `Xsiz - XOsiz` and `Ysiz - YOsiz`.
    area: (u32, u32),
    /// Every component's sampling when they all agree, else 1 × 1: the
    /// divisor `hayro-jpeg2000` applies to the area.
    shrink: (u32, u32),
    /// The first tile's size within the area, the largest a tile can be.
    tile: (u32, u32),
    /// The f32 planes alive at once: one per SIZ component, plus one per
    /// `cmap` entry when a palette is resolved (always at full resolution);
    /// with a `cdef`, at least twice the output planes, which
    /// `hayro-jpeg2000` 0.4.1 copies to reorder them; with a colour profile,
    /// [`profile_planes`] of those.
    planes: u32,
    /// The `hayro-jpeg2000` decoder contexts alive at once, each with a
    /// tile-component: two when it converts through a colour profile.
    contexts: u32,
    /// Every component samples 1 × 1.
    unit_sampling: bool,
    palette: bool,
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
impl Preflight {
    /// The image size at full resolution, as `hayro-jpeg2000` reports it.
    pub(super) fn original(&self) -> (u32, u32) {
        self.at_level(0)
    }

    /// The size after skipping `level` resolution levels.
    pub(super) fn at_level(&self, level: u32) -> (u32, u32) {
        let shrink =
            |area: u32, factor: u32| u64::from(area).div_ceil(u64::from(factor) << level) as u32;
        (
            shrink(self.area.0, self.shrink.0),
            shrink(self.area.1, self.shrink.1),
        )
    }

    /// Whether `hayro-jpeg2000` 0.4.1 may skip resolution levels: only with
    /// 1 × 1 sampling (#1409) and no palette (which it never reduces).
    pub(super) fn may_reduce(&self) -> bool {
        self.unit_sampling && !self.palette
    }

    /// f32 samples of every plane at `level` plus the largest tile-component
    /// there for each decoder context (#1558's formula; the tile count is
    /// bounded). A `cdef` copy and a profile conversion are of the planes at
    /// that level, so the same plane count holds when a level is skipped.
    pub(super) fn decode_bytes(&self, level: u32) -> u64 {
        let (width, height) = self.at_level(level);
        let tile = |size: u32| u64::from(size).div_ceil(1 << level);
        u64::from(width) * u64::from(height) * u64::from(self.planes) * 4
            + u64::from(self.contexts) * tile(self.tile.0) * tile(self.tile.1) * 4
    }
}

/// Whether `hayro-jpeg2000` 0.4.1 converts through a colour profile with this
/// `colr` payload (I.5.3.3): method 2, an ICC profile, or method 1 with the
/// CIELab (14) or ROMM-RGB (21) space, which it converts with built-in
/// profiles (`get_color_space`). It reads only the first `colr` box.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn converts_with_a_profile(colour: &[u8]) -> bool {
    match *colour {
        [2, ..] => true,
        [1, _, _, a, b, c, d, ..] => matches!(u32::from_be_bytes([a, b, c, d]), 14 | 21),
        _ => false,
    }
}

/// The f32 planes alive at once when `hayro-jpeg2000` 0.4.1 converts through
/// a colour profile, from the `peak` planes of one decode and the `output`
/// planes it leaves (`integration.rs`, `convert_inner::process`). The first
/// decode's context lives for the whole call and keeps its `output` planes
/// beside their interleaved 8-bit copy (`data_u8`: `output` bytes a pixel,
/// `ceil(output / 4)` planes). moxcms's result then takes at most 4 bytes a
/// pixel, one plane, or, when moxcms refuses the profile (malformed, or 10 MB
/// or more, which the helper's input allows), `process` runs again as grey,
/// RGB or CMYK and decodes the whole image a second time beside them: `peak`
/// planes again, or after that decode, for CMYK, its own `output` planes and
/// 8-bit buffers of at most `4 × output + 4` bytes a pixel (its own copy, the
/// CMYK and alpha split grown by doubling, and the 3-byte sRGB result), that
/// is `2 × output + 1` planes.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn profile_planes(peak: u32, output: u32) -> u32 {
    output + output.div_ceil(4) + peak.max(2 * output + 1)
}

/// Every bound the helper applies before `hayro-jpeg2000` allocates its tiles:
/// JP2 compatibility, one `jp2h` and then one `jp2c` with no other of either
/// kind, `ihdr` agreeing with SIZ, the source bounds, at most
/// 65,535 tiles by formula B-5, 1 to 4 components of at most 16 bits, and a
/// palette of 1 to 1,024 entries and 1 to 4 columns of at most 16 bits with a
/// component mapping of 1 to 4 entries that each name a component and column
/// that exist (#1649), and at most one channel definition.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub(super) fn preflight(
    bytes: &[u8],
) -> std::result::Result<Preflight, image_helper::ImageErrorKind> {
    use image_helper::ImageErrorKind::{
        Jpeg2000TooDeep, Jpeg2000TooManyComponents, Jpeg2000TooManyTiles, Jpeg2000Unreadable,
        JpxNotJp2Compatible,
    };
    // `cdef` present, and the first `colr` converting through a profile.
    let (codestream, header, palette, cdef, icc) = if bytes.starts_with(CODESTREAM_SIGNATURE) {
        (bytes, None, None, false, false)
    } else if bytes.starts_with(JP2_SIGNATURE) {
        let mut boxes = &bytes[JP2_SIGNATURE.len()..];
        let (kind, file_type) = next_box(&mut boxes).map_err(|_| Jpeg2000Unreadable)?;
        if &kind != b"ftyp" || file_type.len() < 8 || !file_type.len().is_multiple_of(4) {
            return Err(Jpeg2000Unreadable);
        }
        let compatible = &file_type[..4] == b"jp2 "
            || file_type[8..]
                .as_chunks::<4>()
                .0
                .iter()
                .any(|brand| brand == b"jp2 ");
        if !compatible {
            return Err(JpxNotJp2Compatible);
        }
        // hayro-jpeg2000 0.4.1 reads every top-level box to the end of the
        // file and keeps the last `jp2h` and the last `jp2c` (`jp2/mod.rs`
        // 56-117), so the preflight walks every box too and accepts only the
        // shape where its first and hayro's last are the same box: one `jp2h`,
        // then one `jp2c`. A `jp2h` or `jp2c` after the first `jp2c` is
        // refused (a JP2 reader renders the first codestream, I.5.4; hayro
        // would decode the last). A second `jp2h` before it is refused as well:
        // I.5.3 allows exactly one, and hayro keeps only the last one's
        // `pclr`, `cmap`, `colr` and `cdef`, so checking either alone could
        // miss what the other holds. hayro, not strict, stops at the first
        // bytes that do not frame a box (`jp2/box.rs::read` frames as
        // `next_box` does). After the first `jp2c` this walk stops there
        // too, so padding after the codestream decodes as hayro decodes it;
        // before it such bytes are refused. Either way no box hayro reads goes
        // unseen.
        let (mut header_box, mut codestream) = (None, None);
        while !boxes.is_empty() {
            let Ok((kind, payload)) = next_box(&mut boxes) else {
                if codestream.is_some() {
                    break;
                }
                return Err(Jpeg2000Unreadable);
            };
            match &kind {
                b"jp2h" if header_box.is_none() && codestream.is_none() => {
                    header_box = Some(payload);
                }
                b"jp2c" if codestream.is_none() => codestream = Some(payload),
                b"jp2h" | b"jp2c" => return Err(Jpeg2000Unreadable),
                _ => {}
            }
        }
        let (mut header, mut palette, mut mapping, mut definition) = (None, None, None, None);
        let mut colour = None;
        let mut children = header_box.ok_or(Jpeg2000Unreadable)?;
        while !children.is_empty() {
            let (kind, payload) = next_box(&mut children).map_err(|_| Jpeg2000Unreadable)?;
            let slot = match &kind {
                b"ihdr" => &mut header,
                b"pclr" => &mut palette,
                b"cmap" => &mut mapping,
                b"cdef" => &mut definition,
                // I.5.3.3 allows several; hayro reads the first.
                b"colr" => {
                    colour.get_or_insert(payload);
                    continue;
                }
                _ => continue,
            };
            if slot.replace(payload).is_some() {
                return Err(Jpeg2000Unreadable);
            }
        }
        let header: &[u8] = header.ok_or(Jpeg2000Unreadable)?;
        if header.len() != 14 {
            return Err(Jpeg2000Unreadable);
        }
        let size = (be_u32(&header[4..8]), be_u32(&header[..4]));
        let palette = palette.map(|palette| (palette, mapping));
        (
            codestream.ok_or(Jpeg2000Unreadable)?,
            Some(size),
            palette,
            definition.is_some(),
            colour.is_some_and(converts_with_a_profile),
        )
    } else {
        return Err(Jpeg2000Unreadable);
    };

    // SIZ (A.5.1): Lsiz, Rsiz, eight u32 fields, Csiz, then the components.
    if !codestream.starts_with(CODESTREAM_SIGNATURE) || codestream.len() < 42 {
        return Err(Jpeg2000Unreadable);
    }
    let components = usize::from(u16::from_be_bytes([codestream[40], codestream[41]]));
    match components {
        0 => return Err(Jpeg2000Unreadable),
        1..=4 => {}
        _ => return Err(Jpeg2000TooManyComponents),
    }
    let segment_length = usize::from(u16::from_be_bytes([codestream[4], codestream[5]]));
    let segment_end = 4 + segment_length;
    if segment_length != 38 + 3 * components || segment_end > codestream.len() {
        return Err(Jpeg2000Unreadable);
    }
    let field = |index: usize| be_u32(&codestream[8 + index * 4..12 + index * 4]);
    let (x_size, y_size, x_origin, y_origin) = (field(0), field(1), field(2), field(3));
    let (tile_width, tile_height, tile_x, tile_y) = (field(4), field(5), field(6), field(7));
    // hayro-jpeg2000 subtracts the origin unchecked (C2).
    if x_origin >= x_size || y_origin >= y_size {
        return Err(Jpeg2000Unreadable);
    }
    let area = (x_size - x_origin, y_size - y_origin);
    image_helper::check_size(area.0, area.1)?;
    if let Some((width, height)) = header {
        image_helper::check_size(width, height)?;
        if (width, height) != area {
            return Err(Jpeg2000Unreadable);
        }
    }
    // B-3 and B-4.
    if tile_width == 0
        || tile_height == 0
        || tile_x > x_origin
        || tile_y > y_origin
        || u64::from(tile_x) + u64::from(tile_width) <= u64::from(x_origin)
        || u64::from(tile_y) + u64::from(tile_height) <= u64::from(y_origin)
    {
        return Err(Jpeg2000Unreadable);
    }
    // B-5 over the reference grid, as hayro-jpeg2000 counts them.
    let tiles = u64::from(x_size - tile_x).div_ceil(u64::from(tile_width))
        * u64::from(y_size - tile_y).div_ceil(u64::from(tile_height));
    if tiles > MAX_TILES {
        return Err(Jpeg2000TooManyTiles);
    }
    let table = codestream[42..segment_end].as_chunks::<3>().0;
    let mut sampling = None;
    let mut uniform = true;
    for &[depth, x_sampling, y_sampling] in table {
        if (depth & 0x7f) + 1 > 16 {
            return Err(Jpeg2000TooDeep);
        }
        if x_sampling == 0 || y_sampling == 0 {
            return Err(Jpeg2000Unreadable);
        }
        uniform &= *sampling.get_or_insert((x_sampling, y_sampling)) == (x_sampling, y_sampling);
    }
    let (x_sampling, y_sampling) = sampling.ok_or(Jpeg2000Unreadable)?;
    // hayro-jpeg2000 resolves a palette into one plane per `cmap` entry while
    // it still holds the codestream's planes.
    let (mapped, output) = match palette {
        Some((palette, mapping)) => {
            let entries = palette_mapping(palette, mapping, components)?;
            (entries, entries)
        }
        None => (0, components as u32),
    };
    // With a `cdef` box, hayro-jpeg2000 0.4.1 then clones every output plane
    // (one per `cmap` entry with a palette, else one per component) into a
    // reordered vector before it drops the originals (`lib.rs` 217-234). The
    // box counts whether or not hayro accepts it: one it ignores only makes
    // the budget stricter.
    let peak = if cdef {
        (components as u32 + mapped).max(2 * output)
    } else {
        components as u32 + mapped
    };
    let (planes, contexts) = if icc {
        (profile_planes(peak, output), 2)
    } else {
        (peak, 1)
    };
    let preflight = Preflight {
        area,
        shrink: if uniform {
            (u32::from(x_sampling), u32::from(y_sampling))
        } else {
            (1, 1)
        },
        tile: (tile_width.min(area.0), tile_height.min(area.1)),
        planes,
        contexts,
        unit_sampling: table.iter().all(|component| component[1..] == [1, 1]),
        palette: palette.is_some(),
    };
    let (width, height) = preflight.original();
    image_helper::check_size(width, height)?;
    Ok(preflight)
}

/// A `pclr` box (I.5.3.4) with 1 to 1,024 entries and 1 to 4 columns of at
/// most 16 bits, and its `cmap` (I.5.3.5) of 1 to 4 entries. Each entry names a
/// SIZ component below `components`, a mapping type of 0 (the component itself)
/// or 1 (through a palette column), and a column below the palette's count,
/// checked for type 0 as well although hayro ignores it there. Returns the entry count: `hayro-jpeg2000` 0.4.1's
/// `resolve_palette_indices` makes one full-size f32 plane per entry while it
/// still holds the codestream's planes, and its output takes one channel per
/// entry, so more than four are too many components for an RGBA image. A
/// missing or empty `cmap` is refused (#1649).
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn palette_mapping(
    palette: &[u8],
    mapping: Option<&[u8]>,
    components: usize,
) -> std::result::Result<u32, image_helper::ImageErrorKind> {
    use image_helper::ImageErrorKind::{
        Jpeg2000TooDeep, Jpeg2000TooManyComponents, Jpeg2000Unreadable,
    };
    if palette.len() < 3 {
        return Err(Jpeg2000Unreadable);
    }
    let entries = usize::from(u16::from_be_bytes([palette[0], palette[1]]));
    let columns = usize::from(palette[2]);
    if !(1..=1024).contains(&entries) || !(1..=4).contains(&columns) || palette.len() < 3 + columns
    {
        return Err(Jpeg2000Unreadable);
    }
    let mut entry_bytes = 0;
    for &depth in &palette[3..3 + columns] {
        let bits = usize::from(depth & 0x7f) + 1;
        if bits > 16 {
            return Err(Jpeg2000TooDeep);
        }
        entry_bytes += bits.div_ceil(8);
    }
    if palette.len() < 3 + columns + entries * entry_bytes {
        return Err(Jpeg2000Unreadable);
    }
    // One output channel per `cmap` entry.
    let (channels, partial) = mapping.unwrap_or_default().as_chunks::<4>();
    if channels.is_empty() || !partial.is_empty() {
        return Err(Jpeg2000Unreadable);
    }
    if channels.len() > 4 {
        return Err(Jpeg2000TooManyComponents);
    }
    for &[high, low, kind, column] in channels {
        if usize::from(u16::from_be_bytes([high, low])) >= components
            || kind > 1
            || usize::from(column) >= columns
        {
            return Err(Jpeg2000Unreadable);
        }
    }
    Ok(channels.len() as u32)
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
