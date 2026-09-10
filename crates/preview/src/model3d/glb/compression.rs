//! EXT_meshopt_compression adaptation around meshoptimizer 0.25 (meshopt 0.6.2).
//! Only supplied BIN slices enter the decoder. Native codecs allocate no heap;
//! their bounded, whole-stream calls cannot be interrupted. Cancellation runs
//! immediately around each call and between independently filterable blocks.

use super::{Decoder, Document, MAX_VERTICES, view};
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

pub(super) const EXTENSION: &str = "EXT_meshopt_compression";
const MAX_VIEW_BYTES: usize = 16 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;
const FILTER_BLOCK_ELEMENTS: usize = 4096;

#[derive(Default, Deserialize)]
pub(super) struct BufferExtensions {
    #[serde(rename = "EXT_meshopt_compression")]
    meshopt: Option<Fallback>,
}
#[derive(Deserialize)]
struct Fallback {
    #[serde(default)]
    fallback: bool,
}
#[derive(Default, Deserialize)]
pub(super) struct ViewExtensions {
    #[serde(rename = "EXT_meshopt_compression")]
    pub meshopt: Option<Compression>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Compression {
    buffer: usize,
    #[serde(default)]
    byte_offset: usize,
    byte_length: usize,
    byte_stride: usize,
    count: usize,
    mode: String,
    filter: Option<String>,
}
impl Compression {
    fn filter(&self) -> &str {
        self.filter.as_deref().unwrap_or("NONE")
    }
    fn output_length(&self) -> Result<usize> {
        self.count
            .checked_mul(self.byte_stride)
            .context("GLB meshopt count/stride output overflow")
    }
    fn validate_layout(&self) -> Result<()> {
        ensure!(self.count > 0, "GLB meshopt count must be positive");
        ensure!(
            self.byte_length > 0,
            "GLB meshopt compressed length must be positive"
        );
        match self.mode.as_str() {
            "ATTRIBUTES" => ensure!(
                (4..=256).contains(&self.byte_stride) && self.byte_stride.is_multiple_of(4),
                "GLB meshopt ATTRIBUTES stride must be a multiple of four through 256"
            ),
            "TRIANGLES" | "INDICES" => {
                ensure!(
                    matches!(self.byte_stride, 2 | 4),
                    "GLB meshopt index stride must be two or four"
                );
                ensure!(
                    self.filter() == "NONE",
                    "GLB meshopt index modes cannot use a filter"
                );
                ensure!(
                    self.mode != "TRIANGLES" || self.count.is_multiple_of(3),
                    "GLB meshopt TRIANGLES count must be divisible by three"
                );
            }
            _ => bail!("Unsupported GLB meshopt compression mode {}", self.mode),
        }
        match self.filter() {
            "NONE" => {}
            "OCTAHEDRAL" => ensure!(
                matches!(self.byte_stride, 4 | 8),
                "GLB meshopt OCTAHEDRAL filter requires stride four or eight"
            ),
            "QUATERNION" => ensure!(
                self.byte_stride == 8,
                "GLB meshopt QUATERNION filter requires stride eight"
            ),
            "EXPONENTIAL" => ensure!(
                self.byte_stride.is_multiple_of(4),
                "GLB meshopt EXPONENTIAL filter requires four-byte components"
            ),
            filter => bail!("Unsupported GLB meshopt filter {filter}"),
        }
        Ok(())
    }
}

pub(super) fn validate(document: &Document, check: &impl Fn() -> Result<()>) -> Result<()> {
    let declared = document.extensions_used.iter().any(|v| v == EXTENSION);
    let required = document.extensions_required.iter().any(|v| v == EXTENSION);
    let mut compressed_references = vec![false; document.buffers.len()];
    let mut plain_references = vec![false; document.buffers.len()];
    for source in &document.buffer_views {
        check()?;
        *plain_references
            .get_mut(source.buffer)
            .context("GLB buffer view references a missing buffer")? |=
            source.extensions.meshopt.is_none();
        if let Some(compressed) = &source.extensions.meshopt {
            ensure!(
                declared,
                "GLB meshopt views require an extensionsUsed declaration"
            );
            compressed.validate_layout()?;
            ensure!(
                compressed.output_length()? == source.byte_length,
                "GLB meshopt output length disagrees with its buffer view"
            );
            ensure!(
                source
                    .byte_stride
                    .is_none_or(|v| v == compressed.byte_stride),
                "GLB meshopt stride disagrees with its buffer view"
            );
            let buffer = document
                .buffers
                .get(compressed.buffer)
                .context("GLB meshopt references a missing compressed buffer")?;
            compressed_references[compressed.buffer] = true;
            ensure!(
                compressed
                    .byte_offset
                    .checked_add(compressed.byte_length)
                    .is_some_and(|v| v <= buffer.byte_length),
                "GLB meshopt compressed range exceeds its buffer"
            );
        }
    }
    for (index, buffer) in document.buffers.iter().enumerate() {
        check()?;
        let tagged = buffer
            .extensions
            .meshopt
            .as_ref()
            .is_some_and(|v| v.fallback);
        ensure!(
            buffer.extensions.meshopt.is_none() || declared,
            "GLB meshopt fallback requires an extensionsUsed declaration"
        );
        if tagged {
            ensure!(
                !plain_references[index] && !compressed_references[index],
                "GLB meshopt fallback buffer must only supply fallback buffer views"
            );
        }
        if index > 0 && buffer.uri.is_none() {
            ensure!(
                required && !plain_references[index] && !compressed_references[index],
                "GLB placeholder buffer requires meshopt in extensionsRequired and compressed fallback views only"
            );
        }
    }
    Ok(())
}

// u32 storage provides the alignment required by native index/filter operations.
// Only these temporary bytes are cached; ModelScene retains triangles alone.
pub(super) struct DecodedView {
    words: Vec<u32>,
    length: usize,
}
impl DecodedView {
    pub fn bytes(&self) -> &[u8] {
        // SAFETY: initialized u32 storage is valid as bytes; length is at most
        // words.len()*4, and the immutable borrow prevents concurrent mutation.
        unsafe { std::slice::from_raw_parts(self.words.as_ptr().cast(), self.length) }
    }
    fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: all u32 bit patterns are valid and the exclusive borrow covers
        // the initialized allocation. The visible byte length excludes padding.
        unsafe { std::slice::from_raw_parts_mut(self.words.as_mut_ptr().cast(), self.length) }
    }
}
fn zeroed_words(length: usize) -> Result<Vec<u32>> {
    let mut words = Vec::new();
    words
        .try_reserve_exact(length)
        .context("Unable to allocate bounded GLB meshopt output")?;
    words.resize(length, 0);
    Ok(words)
}

impl<F: Fn() -> Result<()>> Decoder<'_, F> {
    pub(super) fn prepare_accessor(&mut self, index: usize) -> Result<()> {
        let accessor = self
            .document
            .accessors
            .get(index)
            .context("GLB references a missing accessor")?;
        let sources = [
            accessor.buffer_view,
            accessor.sparse.as_ref().map(|v| v.indices.buffer_view),
            accessor.sparse.as_ref().map(|v| v.values.buffer_view),
        ];
        for index in sources.into_iter().flatten() {
            if self.decoded_views.contains_key(&index) {
                continue;
            }
            let source = view(self.document, index)?;
            let Some(compressed) = &source.extensions.meshopt else {
                continue;
            };
            let length = compressed.output_length()?;
            ensure!(
                compressed.count <= MAX_VERTICES,
                "GLB meshopt selected view exceeds the 300,000-element preview limit"
            );
            ensure!(
                length <= MAX_VIEW_BYTES && compressed.byte_length <= MAX_VIEW_BYTES,
                "GLB meshopt view exceeds the 16 MiB compressed or decompressed preview limit"
            );
            let allocation = length.next_multiple_of(4);
            self.compressed_bytes = self
                .compressed_bytes
                .checked_add(compressed.byte_length)
                .context("GLB meshopt compressed work overflow")?;
            self.decompressed_bytes = self
                .decompressed_bytes
                .checked_add(allocation)
                .context("GLB meshopt decompressed allocation overflow")?;
            ensure!(
                self.compressed_bytes <= MAX_TOTAL_BYTES
                    && self.decompressed_bytes <= MAX_TOTAL_BYTES,
                "GLB meshopt exceeds the 32 MiB cumulative compressed or decompressed preview limit"
            );
            let buffer = &self.document.buffers[compressed.buffer];
            ensure!(
                compressed.buffer == 0 && buffer.uri.is_none(),
                "GLB meshopt compressed data requires the supplied embedded BIN buffer; external resources are never loaded"
            );
            let end = compressed
                .byte_offset
                .checked_add(compressed.byte_length)
                .context("GLB meshopt compressed range overflow")?;
            let bytes = self
                .bin
                .context("GLB meshopt requires a missing BIN chunk")?
                .get(compressed.byte_offset..end)
                .context("GLB meshopt compressed range exceeds the supplied BIN chunk")?;
            let decoded = decode_view(compressed, bytes, self.check)?;
            self.decoded_views.insert(index, decoded);
        }
        Ok(())
    }
}

fn decode_view(
    source: &Compression,
    bytes: &[u8],
    check: &impl Fn() -> Result<()>,
) -> Result<DecodedView> {
    source.validate_layout()?;
    let header = match source.mode.as_str() {
        "ATTRIBUTES" => 0xa0,
        "TRIANGLES" => 0xe1,
        _ => 0xd1,
    };
    ensure!(
        bytes.first() == Some(&header),
        "Unsupported GLB EXT_meshopt_compression bitstream header or version"
    );
    if source.mode == "TRIANGLES" {
        let tail = bytes
            .get(bytes.len().saturating_sub(16)..)
            .context("Truncated GLB meshopt triangle stream")?;
        ensure!(
            tail.len() == 16
                && tail[14..] == [0, 0]
                && tail[..14].iter().all(|v| v & 15 != 15 && v >> 4 != 15),
            "Invalid GLB meshopt triangle lookup table"
        );
    }
    check()?;
    let length = source.output_length()?;
    let mut output = DecodedView {
        words: zeroed_words(length.div_ceil(4))?,
        length,
    };
    let status;
    if source.mode == "ATTRIBUTES" {
        // SAFETY: mode/stride/count/ranges were checked before allocation. The
        // supplied slice is the complete stream and the aligned initialized
        // destination has count*stride bytes. meshoptimizer supports untrusted
        // streams, reports malformed data, and never retains these pointers.
        status = unsafe {
            meshopt::ffi::meshopt_decodeVertexBuffer(
                output.words.as_mut_ptr().cast(),
                source.count,
                source.byte_stride,
                bytes.as_ptr(),
                bytes.len(),
            )
        };
    } else {
        // Decode to u32 even for ushort output: the native two-byte decoder
        // truncates arbitrary stream values before the accessor can reject them.
        // This separate transient allocation is at most 1.2 MB (300k indices).
        let mut indices = zeroed_words(source.count)?;
        // SAFETY: validated index mode/count and four-byte destination; buffers
        // are initialized, independently owned, live and correctly sized.
        status = unsafe {
            if source.mode == "TRIANGLES" {
                meshopt::ffi::meshopt_decodeIndexBuffer(
                    indices.as_mut_ptr().cast(),
                    source.count,
                    4,
                    bytes.as_ptr(),
                    bytes.len(),
                )
            } else {
                meshopt::ffi::meshopt_decodeIndexSequence(
                    indices.as_mut_ptr().cast(),
                    source.count,
                    4,
                    bytes.as_ptr(),
                    bytes.len(),
                )
            }
        };
        check()?;
        ensure!(
            status == 0,
            "Malformed or truncated GLB meshopt {} bitstream (decoder {status})",
            source.mode
        );
        for (i, (value, target)) in indices
            .iter()
            .zip(output.bytes_mut().chunks_exact_mut(source.byte_stride))
            .enumerate()
        {
            if i.is_multiple_of(FILTER_BLOCK_ELEMENTS) {
                check()?;
            }
            if source.byte_stride == 2 {
                ensure!(
                    *value <= u16::MAX as u32,
                    "GLB meshopt decoded index exceeds its unsigned short representation"
                );
                target.copy_from_slice(&(*value as u16).to_le_bytes());
            } else {
                target.copy_from_slice(&value.to_le_bytes());
            }
        }
    }
    check()?;
    ensure!(
        status == 0,
        "Malformed or truncated GLB meshopt {} bitstream (decoder {status})",
        source.mode
    );
    if source.filter() != "NONE" {
        for block in output
            .bytes_mut()
            .chunks_mut(FILTER_BLOCK_ELEMENTS * source.byte_stride)
        {
            check()?;
            validate_filter_input(source, block)?;
            // The filters operate on native integer words. Convert the encoded
            // little-endian components around the call on big-endian targets.
            let component = if source.filter() == "EXPONENTIAL" {
                4
            } else {
                source.byte_stride / 4
            };
            swap_endian(block, component);
            // SAFETY: blocks start at four-byte boundaries, contain whole
            // elements, and have the mode-specific validated filter stride.
            unsafe {
                let ptr = block.as_mut_ptr().cast();
                let count = block.len() / source.byte_stride;
                match source.filter() {
                    "OCTAHEDRAL" => {
                        meshopt::ffi::meshopt_decodeFilterOct(ptr, count, source.byte_stride)
                    }
                    "QUATERNION" => {
                        meshopt::ffi::meshopt_decodeFilterQuat(ptr, count, source.byte_stride)
                    }
                    "EXPONENTIAL" => {
                        meshopt::ffi::meshopt_decodeFilterExp(ptr, count, source.byte_stride)
                    }
                    _ => unreachable!(),
                }
            }
            swap_endian(block, component);
        }
        check()?;
    }
    Ok(output)
}
fn validate_filter_input(source: &Compression, bytes: &[u8]) -> Result<()> {
    if source.filter() == "EXPONENTIAL" {
        // EXT Appendix B constrains the signed high-byte exponent even when
        // the mantissa is zero. Out-of-range decoding is unspecified and may
        // yield an apparently valid finite zero, so output checks cannot replace
        // validation of the encoded value.
        for component in bytes.as_chunks::<4>().0 {
            let exponent = component[3] as i8;
            ensure!(
                (-100..=100).contains(&exponent),
                "GLB meshopt EXPONENTIAL filter exponent is outside the supported -100 through 100 range"
            );
        }
        return Ok(());
    }
    if !matches!(source.filter(), "OCTAHEDRAL" | "QUATERNION") {
        return Ok(());
    }
    for element in bytes.chunks_exact(source.byte_stride) {
        let component = |i: usize| -> i32 {
            if source.byte_stride == 4 {
                element[i] as i8 as i32
            } else {
                i16::from_le_bytes([element[i * 2], element[i * 2 + 1]]) as i32
            }
        };
        let quaternion = source.filter() == "QUATERNION";
        let one = if quaternion {
            component(3) | 3
        } else {
            component(2)
        };
        ensure!(
            one >= if quaternion { 7 } else { 1 }
                && ((one + 1) as u32).is_power_of_two()
                && (0..if quaternion { 3 } else { 2 }).all(|i| component(i).abs() <= one),
            "GLB meshopt filter contains invalid normalized encoded components"
        );
    }
    Ok(())
}
fn swap_endian(bytes: &mut [u8], component: usize) {
    if cfg!(target_endian = "big") {
        for value in bytes.chunks_exact_mut(component) {
            value.reverse();
        }
    }
}

#[cfg(test)]
mod tests;
