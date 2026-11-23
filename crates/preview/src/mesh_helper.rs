//! Binary IPC frames for confined Linux 3D native decoders (`ufbx` for `.obj`/`.fbx`
//! and `meshopt` for GLB `EXT_meshopt_compression` views).
//!
//! Runs inside the same seccomp-confined helper process as PDF pages and AVIF/JP2
//! images (`pdf_helper::serve` / `serve_buffer`). Only validated triangle coordinates
//! or raw decompressed view bytes cross back; untrusted error strings never cross.
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};

use anyhow::{Result, ensure};

use crate::MAX_INPUT_BYTES;
use crate::model3d::{
    MAX_MODEL_TRIANGLES, MeshoptFilter, MeshoptMode, MeshoptViewJob, ModelTriangle,
    decode_meshopt_job, decode_ufbx_triangles,
};
use crate::pdf_helper::PROTOCOL_VERSION;

/// Request and reply magics for confined `ufbx` (`.obj`/`.fbx`) triangle decoding.
pub(crate) const UFBX_REQUEST_MAGIC: [u8; 4] = *b"GTMq";
const UFBX_REPLY_MAGIC: [u8; 4] = *b"GTMr";

/// Request and reply magics for confined batched GLB `EXT_meshopt_compression` decoding.
pub(crate) const MESHOPT_REQUEST_MAGIC: [u8; 4] = *b"GTXq";
const MESHOPT_REPLY_MAGIC: [u8; 4] = *b"GTXr";

pub const UFBX_REQUEST_HEADER_BYTES: usize = 16;
const REPLY_PREFIX_BYTES: usize = 9;
const STATUS_OK: u8 = 0;
const STATUS_FAILED: u8 = 1;

const FORMAT_OBJ: u8 = 0;
const FORMAT_FBX: u8 = 1;

const MODE_ATTRIBUTES: u8 = 0;
const MODE_TRIANGLES: u8 = 1;
const MODE_INDICES: u8 = 2;

const FILTER_NONE: u8 = 0;
const FILTER_OCTAHEDRAL: u8 = 1;
const FILTER_QUATERNION: u8 = 2;
const FILTER_EXPONENTIAL: u8 = 3;

const MAX_MESHOPT_VIEWS: usize = 1024;
const MAX_MESHOPT_VIEW_BYTES: usize = 16 * 1024 * 1024;
const MAX_MESHOPT_TOTAL_BYTES: usize = 32 * 1024 * 1024;
const MAX_MESHOPT_VIEW_COUNT: usize = 300_000;
const MESHOPT_VIEW_HEADER_BYTES: usize = 24;

/// Maximum header bytes added to a `GTXq` batch request beyond the 32 MiB
/// cumulative compressed payload limit.
pub const MAX_MESHOPT_REQUEST_OVERHEAD: usize = 12 + MAX_MESHOPT_VIEWS * MESHOPT_VIEW_HEADER_BYTES;

/// Maximum byte length of a `GTMr` reply (100,000 triangles * 72 bytes + header).
pub const MAX_UFBX_REPLY_BYTES: usize = REPLY_PREFIX_BYTES + 4 + MAX_MODEL_TRIANGLES * 72;

/// Maximum byte length of a `GTXr` reply (32 MiB cumulative views + headers).
pub const MAX_MESHOPT_REPLY_BYTES: usize =
    REPLY_PREFIX_BYTES + 4 + MAX_MESHOPT_VIEWS * 8 + MAX_MESHOPT_TOTAL_BYTES;

/// Fixed error codes returned by the confined mesh helper (never untrusted strings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshErrorKind {
    Request,
    InputTooLarge,
    DecodeFailed,
    DecoderPanicked,
}

impl MeshErrorKind {
    fn code(self) -> u8 {
        match self {
            Self::Request => 1,
            Self::InputTooLarge => 2,
            Self::DecodeFailed => 3,
            Self::DecoderPanicked => 4,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Request),
            2 => Some(Self::InputTooLarge),
            3 => Some(Self::DecodeFailed),
            4 => Some(Self::DecoderPanicked),
            _ => None,
        }
    }
}

fn u32_at(bytes: &[u8], start: usize) -> u32 {
    u32::from_le_bytes([
        bytes[start],
        bytes[start + 1],
        bytes[start + 2],
        bytes[start + 3],
    ])
}

fn wire(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn unreadable(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}

/// Encodes the 16-byte header for a confined `ufbx` (`.obj`/`.fbx`) request.
/// Write the header followed directly by `bytes`.
pub fn encode_ufbx_request(bytes: &[u8], obj: bool) -> Result<[u8; UFBX_REQUEST_HEADER_BYTES]> {
    ensure!(
        bytes.len() <= MAX_INPUT_BYTES,
        "3D source exceeds the 32 MiB preview limit"
    );
    let mut header = [0u8; UFBX_REQUEST_HEADER_BYTES];
    header[..4].copy_from_slice(&UFBX_REQUEST_MAGIC);
    header[4..8].copy_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    header[8] = if obj { FORMAT_OBJ } else { FORMAT_FBX };
    header[12..16].copy_from_slice(&wire(bytes.len()).to_le_bytes());
    Ok(header)
}

/// Reads exactly one `GTMr` reply and then EOF.
pub fn read_ufbx_reply(
    mut input: impl Read,
) -> io::Result<Result<Vec<ModelTriangle>, MeshErrorKind>> {
    let mut prefix = [0u8; REPLY_PREFIX_BYTES];
    input.read_exact(&mut prefix)?;
    if prefix[..4] != UFBX_REPLY_MAGIC {
        return Err(unreadable("the mesh reply has the wrong magic"));
    }
    if u32_at(&prefix, 4) != PROTOCOL_VERSION {
        return Err(unreadable("the mesh reply has another protocol version"));
    }
    let reply = match prefix[8] {
        STATUS_OK => {
            let mut count_bytes = [0u8; 4];
            input.read_exact(&mut count_bytes)?;
            let count = u32::from_le_bytes(count_bytes) as usize;
            if count > MAX_MODEL_TRIANGLES {
                return Err(unreadable("the mesh reply exceeds the triangle limit"));
            }
            let mut triangles = Vec::with_capacity(count);
            let mut tri_buf = [0u8; 72];
            for _ in 0..count {
                input.read_exact(&mut tri_buf)?;
                let mut tri = [[0.0f64; 3]; 3];
                for (vertex_idx, vertex) in tri.iter_mut().enumerate() {
                    for (axis_idx, coord) in vertex.iter_mut().enumerate() {
                        let start = (vertex_idx * 3 + axis_idx) * 8;
                        let value = f64::from_le_bytes([
                            tri_buf[start],
                            tri_buf[start + 1],
                            tri_buf[start + 2],
                            tri_buf[start + 3],
                            tri_buf[start + 4],
                            tri_buf[start + 5],
                            tri_buf[start + 6],
                            tri_buf[start + 7],
                        ]);
                        if !value.is_finite() || value.abs() > crate::model3d::MAX_MODEL_COORD {
                            return Err(unreadable(
                                "the mesh reply contains non-finite or out-of-range coordinates",
                            ));
                        }
                        *coord = value;
                    }
                }
                triangles.push(tri);
            }
            Ok(triangles)
        }
        STATUS_FAILED => {
            let mut code = [0u8; 1];
            input.read_exact(&mut code)?;
            Err(MeshErrorKind::from_code(code[0])
                .ok_or_else(|| unreadable("the mesh reply has an unknown error kind"))?)
        }
        _ => return Err(unreadable("the mesh reply has an unknown status")),
    };
    if input.read(&mut [0])? != 0 {
        return Err(unreadable("the mesh reply is followed by more bytes"));
    }
    Ok(reply)
}

fn write_ufbx_reply(
    output: &mut impl Write,
    reply: &std::result::Result<Vec<ModelTriangle>, MeshErrorKind>,
) -> io::Result<()> {
    let mut prefix = [0u8; REPLY_PREFIX_BYTES];
    prefix[..4].copy_from_slice(&UFBX_REPLY_MAGIC);
    prefix[4..8].copy_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    match reply {
        Ok(triangles) => {
            prefix[8] = STATUS_OK;
            output.write_all(&prefix)?;
            output.write_all(&wire(triangles.len()).to_le_bytes())?;
            let mut tri_buf = [0u8; 72];
            for tri in triangles {
                for (vertex_idx, vertex) in tri.iter().enumerate() {
                    for (axis_idx, coord) in vertex.iter().enumerate() {
                        let start = (vertex_idx * 3 + axis_idx) * 8;
                        tri_buf[start..start + 8].copy_from_slice(&coord.to_le_bytes());
                    }
                }
                output.write_all(&tri_buf)?;
            }
        }
        Err(kind) => {
            prefix[8] = STATUS_FAILED;
            output.write_all(&prefix)?;
            output.write_all(&[kind.code()])?;
        }
    }
    Ok(())
}

pub(crate) fn serve_ufbx(mut input: impl Read, mut output: impl Write) -> io::Result<()> {
    let reply = read_ufbx_request(&mut input).and_then(|(obj, bytes)| {
        catch_unwind(AssertUnwindSafe(|| {
            decode_ufbx_triangles(&bytes, obj, || Ok(())).map_err(|_| MeshErrorKind::DecodeFailed)
        }))
        .unwrap_or(Err(MeshErrorKind::DecoderPanicked))
    });
    write_ufbx_reply(&mut output, &reply)?;
    output.flush()
}

pub(crate) fn serve_ufbx_slice(rest: &[u8], mut output: impl Write) -> io::Result<()> {
    let reply = parse_ufbx_slice(rest).and_then(|(obj, bytes)| {
        catch_unwind(AssertUnwindSafe(|| {
            decode_ufbx_triangles(bytes, obj, || Ok(())).map_err(|_| MeshErrorKind::DecodeFailed)
        }))
        .unwrap_or(Err(MeshErrorKind::DecoderPanicked))
    });
    write_ufbx_reply(&mut output, &reply)?;
    output.flush()
}

fn read_ufbx_request(input: &mut impl Read) -> std::result::Result<(bool, Vec<u8>), MeshErrorKind> {
    let mut header = [0u8; UFBX_REQUEST_HEADER_BYTES - 4];
    input
        .read_exact(&mut header)
        .map_err(|_| MeshErrorKind::Request)?;
    if u32_at(&header, 0) != PROTOCOL_VERSION {
        return Err(MeshErrorKind::Request);
    }
    let obj = match header[4] {
        FORMAT_OBJ => true,
        FORMAT_FBX => false,
        _ => return Err(MeshErrorKind::Request),
    };
    if header[5..8] != [0, 0, 0] {
        return Err(MeshErrorKind::Request);
    }
    let length = u32_at(&header, 8) as usize;
    if length > MAX_INPUT_BYTES {
        return Err(MeshErrorKind::InputTooLarge);
    }
    let mut bytes = vec![0u8; length];
    input
        .read_exact(&mut bytes)
        .map_err(|_| MeshErrorKind::Request)?;
    if input.read(&mut [0]).map_err(|_| MeshErrorKind::Request)? != 0 {
        return Err(MeshErrorKind::Request);
    }
    Ok((obj, bytes))
}

fn parse_ufbx_slice(rest: &[u8]) -> std::result::Result<(bool, &[u8]), MeshErrorKind> {
    let header = rest
        .get(..UFBX_REQUEST_HEADER_BYTES - 4)
        .ok_or(MeshErrorKind::Request)?;
    if u32_at(header, 0) != PROTOCOL_VERSION {
        return Err(MeshErrorKind::Request);
    }
    let obj = match header[4] {
        FORMAT_OBJ => true,
        FORMAT_FBX => false,
        _ => return Err(MeshErrorKind::Request),
    };
    if header[5..8] != [0, 0, 0] {
        return Err(MeshErrorKind::Request);
    }
    let length = u32_at(header, 8) as usize;
    if length > MAX_INPUT_BYTES {
        return Err(MeshErrorKind::InputTooLarge);
    }
    let start = UFBX_REQUEST_HEADER_BYTES - 4;
    let end = start.checked_add(length).ok_or(MeshErrorKind::Request)?;
    if rest.len() != end {
        return Err(MeshErrorKind::Request);
    }
    let body = rest.get(start..end).ok_or(MeshErrorKind::Request)?;
    Ok((obj, body))
}

/// Encodes one batched `GTXq` request containing all `EXT_meshopt_compression`
/// views needed for a single GLB file.
pub fn encode_meshopt_request(jobs: &[MeshoptViewJob]) -> Result<Vec<u8>> {
    ensure!(
        !jobs.is_empty() && jobs.len() <= MAX_MESHOPT_VIEWS,
        "GLB meshopt batch view count is out of bounds"
    );
    let mut total_compressed = 0usize;
    let mut total_output = 0usize;
    for job in jobs {
        ensure!(
            job.count > 0 && job.count <= MAX_MESHOPT_VIEW_COUNT,
            "GLB meshopt view element count exceeds bounds"
        );
        ensure!(
            job.output_len <= MAX_MESHOPT_VIEW_BYTES
                && job.compressed.len() <= MAX_MESHOPT_VIEW_BYTES,
            "GLB meshopt view exceeds the 16 MiB limit"
        );
        total_compressed = total_compressed
            .checked_add(job.compressed.len())
            .ok_or_else(|| anyhow::anyhow!("GLB meshopt request size overflow"))?;
        total_output = total_output
            .checked_add(job.output_len)
            .ok_or_else(|| anyhow::anyhow!("GLB meshopt output size overflow"))?;
    }
    ensure!(
        total_compressed <= MAX_MESHOPT_TOTAL_BYTES && total_output <= MAX_MESHOPT_TOTAL_BYTES,
        "GLB meshopt exceeds the 32 MiB cumulative limit"
    );
    let mut frame =
        Vec::with_capacity(12 + jobs.len() * MESHOPT_VIEW_HEADER_BYTES + total_compressed);
    frame.extend_from_slice(&MESHOPT_REQUEST_MAGIC);
    frame.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    frame.extend_from_slice(&wire(jobs.len()).to_le_bytes());
    for job in jobs {
        frame.extend_from_slice(&wire(job.view_index).to_le_bytes());
        let mode_byte = match job.mode {
            MeshoptMode::Attributes => MODE_ATTRIBUTES,
            MeshoptMode::Triangles => MODE_TRIANGLES,
            MeshoptMode::Indices => MODE_INDICES,
        };
        let filter_byte = match job.filter {
            MeshoptFilter::None => FILTER_NONE,
            MeshoptFilter::Octahedral => FILTER_OCTAHEDRAL,
            MeshoptFilter::Quaternion => FILTER_QUATERNION,
            MeshoptFilter::Exponential => FILTER_EXPONENTIAL,
        };
        frame.push(mode_byte);
        frame.push(filter_byte);
        frame.extend_from_slice(&[0u8, 0u8]);
        frame.extend_from_slice(&wire(job.count).to_le_bytes());
        frame.extend_from_slice(&wire(job.stride).to_le_bytes());
        frame.extend_from_slice(&wire(job.output_len).to_le_bytes());
        frame.extend_from_slice(&wire(job.compressed.len()).to_le_bytes());
        frame.extend_from_slice(&job.compressed);
    }
    Ok(frame)
}

/// Reads exactly one `GTXr` reply and then EOF.
pub fn read_meshopt_reply(
    mut input: impl Read,
) -> io::Result<Result<HashMap<usize, Vec<u8>>, MeshErrorKind>> {
    let mut prefix = [0u8; REPLY_PREFIX_BYTES];
    input.read_exact(&mut prefix)?;
    if prefix[..4] != MESHOPT_REPLY_MAGIC {
        return Err(unreadable("the meshopt reply has the wrong magic"));
    }
    if u32_at(&prefix, 4) != PROTOCOL_VERSION {
        return Err(unreadable("the meshopt reply has another protocol version"));
    }
    let reply = match prefix[8] {
        STATUS_OK => {
            let mut count_bytes = [0u8; 4];
            input.read_exact(&mut count_bytes)?;
            let count = u32::from_le_bytes(count_bytes) as usize;
            if count == 0 || count > MAX_MESHOPT_VIEWS {
                return Err(unreadable("the meshopt reply view count is out of bounds"));
            }
            let mut views = HashMap::with_capacity(count);
            let mut total_bytes = 0usize;
            for _ in 0..count {
                let mut hdr = [0u8; 8];
                input.read_exact(&mut hdr)?;
                let view_index = u32_at(&hdr, 0) as usize;
                let output_len = u32_at(&hdr, 4) as usize;
                if output_len > MAX_MESHOPT_VIEW_BYTES {
                    return Err(unreadable("a meshopt reply view exceeds 16 MiB"));
                }
                total_bytes = total_bytes
                    .checked_add(output_len)
                    .filter(|&sum| sum <= MAX_MESHOPT_TOTAL_BYTES)
                    .ok_or_else(|| unreadable("the meshopt reply exceeds 32 MiB"))?;
                let mut buf = vec![0u8; output_len];
                input.read_exact(&mut buf)?;
                if views.insert(view_index, buf).is_some() {
                    return Err(unreadable("duplicate view index in meshopt reply"));
                }
            }
            Ok(views)
        }
        STATUS_FAILED => {
            let mut code = [0u8; 1];
            input.read_exact(&mut code)?;
            Err(MeshErrorKind::from_code(code[0])
                .ok_or_else(|| unreadable("the meshopt reply has an unknown error kind"))?)
        }
        _ => return Err(unreadable("the meshopt reply has an unknown status")),
    };
    if input.read(&mut [0])? != 0 {
        return Err(unreadable("the meshopt reply is followed by more bytes"));
    }
    Ok(reply)
}

fn write_meshopt_reply(
    output: &mut impl Write,
    reply: &std::result::Result<Vec<(usize, Vec<u8>)>, MeshErrorKind>,
) -> io::Result<()> {
    let mut prefix = [0u8; REPLY_PREFIX_BYTES];
    prefix[..4].copy_from_slice(&MESHOPT_REPLY_MAGIC);
    prefix[4..8].copy_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    match reply {
        Ok(views) => {
            prefix[8] = STATUS_OK;
            output.write_all(&prefix)?;
            output.write_all(&wire(views.len()).to_le_bytes())?;
            for (view_index, data) in views {
                output.write_all(&wire(*view_index).to_le_bytes())?;
                output.write_all(&wire(data.len()).to_le_bytes())?;
                output.write_all(data)?;
            }
        }
        Err(kind) => {
            prefix[8] = STATUS_FAILED;
            output.write_all(&prefix)?;
            output.write_all(&[kind.code()])?;
        }
    }
    Ok(())
}

fn run_meshopt_jobs(
    jobs: Vec<MeshoptViewJob>,
) -> std::result::Result<Vec<(usize, Vec<u8>)>, MeshErrorKind> {
    let mut out = Vec::with_capacity(jobs.len());
    for job in jobs {
        let decoded = decode_meshopt_job(&job).map_err(|_| MeshErrorKind::DecodeFailed)?;
        if decoded.len() != job.output_len {
            return Err(MeshErrorKind::DecodeFailed);
        }
        out.push((job.view_index, decoded));
    }
    Ok(out)
}

pub(crate) fn serve_meshopt(mut input: impl Read, mut output: impl Write) -> io::Result<()> {
    let reply = read_meshopt_request(&mut input).and_then(|jobs| {
        catch_unwind(AssertUnwindSafe(|| run_meshopt_jobs(jobs)))
            .unwrap_or(Err(MeshErrorKind::DecoderPanicked))
    });
    write_meshopt_reply(&mut output, &reply)?;
    output.flush()
}

pub(crate) fn serve_meshopt_slice(rest: &[u8], mut output: impl Write) -> io::Result<()> {
    let mut cursor = io::Cursor::new(rest);
    serve_meshopt(&mut cursor, &mut output)
}

fn read_meshopt_request(
    input: &mut impl Read,
) -> std::result::Result<Vec<MeshoptViewJob>, MeshErrorKind> {
    let mut header = [0u8; 8];
    input
        .read_exact(&mut header)
        .map_err(|_| MeshErrorKind::Request)?;
    if u32_at(&header, 0) != PROTOCOL_VERSION {
        return Err(MeshErrorKind::Request);
    }
    let view_count = u32_at(&header, 4) as usize;
    if view_count == 0 || view_count > MAX_MESHOPT_VIEWS {
        return Err(MeshErrorKind::Request);
    }
    let mut jobs = Vec::with_capacity(view_count);
    let mut total_compressed = 0usize;
    let mut total_output = 0usize;
    for _ in 0..view_count {
        let mut entry = [0u8; MESHOPT_VIEW_HEADER_BYTES];
        input
            .read_exact(&mut entry)
            .map_err(|_| MeshErrorKind::Request)?;
        let view_index = u32_at(&entry, 0) as usize;
        let mode = match entry[4] {
            MODE_ATTRIBUTES => MeshoptMode::Attributes,
            MODE_TRIANGLES => MeshoptMode::Triangles,
            MODE_INDICES => MeshoptMode::Indices,
            _ => return Err(MeshErrorKind::Request),
        };
        let filter = match entry[5] {
            FILTER_NONE => MeshoptFilter::None,
            FILTER_OCTAHEDRAL => MeshoptFilter::Octahedral,
            FILTER_QUATERNION => MeshoptFilter::Quaternion,
            FILTER_EXPONENTIAL => MeshoptFilter::Exponential,
            _ => return Err(MeshErrorKind::Request),
        };
        if entry[6..8] != [0, 0] {
            return Err(MeshErrorKind::Request);
        }
        let count = u32_at(&entry, 8) as usize;
        let stride = u32_at(&entry, 12) as usize;
        let output_len = u32_at(&entry, 16) as usize;
        let compressed_len = u32_at(&entry, 20) as usize;
        if count == 0
            || count > MAX_MESHOPT_VIEW_COUNT
            || output_len > MAX_MESHOPT_VIEW_BYTES
            || compressed_len > MAX_MESHOPT_VIEW_BYTES
        {
            return Err(MeshErrorKind::InputTooLarge);
        }
        total_compressed = total_compressed
            .checked_add(compressed_len)
            .filter(|&sum| sum <= MAX_MESHOPT_TOTAL_BYTES)
            .ok_or(MeshErrorKind::InputTooLarge)?;
        total_output = total_output
            .checked_add(output_len)
            .filter(|&sum| sum <= MAX_MESHOPT_TOTAL_BYTES)
            .ok_or(MeshErrorKind::InputTooLarge)?;
        let mut compressed = vec![0u8; compressed_len];
        input
            .read_exact(&mut compressed)
            .map_err(|_| MeshErrorKind::Request)?;
        jobs.push(MeshoptViewJob {
            view_index,
            mode,
            filter,
            count,
            stride,
            output_len,
            compressed,
        });
    }
    if input.read(&mut [0]).map_err(|_| MeshErrorKind::Request)? != 0 {
        return Err(MeshErrorKind::Request);
    }
    Ok(jobs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf_helper::{serve, serve_buffer};

    const OBJ: &[u8] = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";

    #[test]
    fn ufbx_frame_round_trips_through_serve_and_serve_buffer() {
        let header = encode_ufbx_request(OBJ, true).unwrap();
        let mut request = Vec::from(header);
        request.extend_from_slice(OBJ);

        let mut pipe_reply = Vec::new();
        serve(&request[..], &mut pipe_reply).unwrap();
        let tris = read_ufbx_reply(&pipe_reply[..]).unwrap().unwrap();
        assert_eq!(tris.len(), 1);
        assert_eq!(tris[0][0], [0.0, 0.0, 0.0]);

        let mut buf_reply = Vec::new();
        serve_buffer(request, &mut buf_reply).unwrap();
        let buf_tris = read_ufbx_reply(&buf_reply[..]).unwrap().unwrap();
        assert_eq!(buf_tris, tris);
    }

    #[test]
    fn ufbx_rejects_bad_version_and_non_finite_reply() {
        let mut header = encode_ufbx_request(OBJ, true).unwrap();
        header[4..8].copy_from_slice(&999u32.to_le_bytes());
        let mut request = Vec::from(header);
        request.extend_from_slice(OBJ);
        let mut reply = Vec::new();
        serve(&request[..], &mut reply).unwrap();
        assert_eq!(
            read_ufbx_reply(&reply[..]).unwrap().unwrap_err(),
            MeshErrorKind::Request
        );

        // Non-finite coordinate injected into reply is rejected by reader.
        let mut forged = Vec::new();
        forged.extend_from_slice(&UFBX_REPLY_MAGIC);
        forged.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        forged.push(STATUS_OK);
        forged.extend_from_slice(&1u32.to_le_bytes());
        forged.extend_from_slice(&f64::NAN.to_le_bytes());
        forged.extend_from_slice(&[0u8; 64]);
        assert!(read_ufbx_reply(&forged[..]).is_err());
    }

    #[test]
    fn meshopt_frame_round_trips_valid_glb_fixture() {
        let glb = include_bytes!("../tests/fixtures/models/glb/meshopt-arch-before.glb");
        let direct = crate::model3d::decode_geometry(glb, "arch.glb", || Ok(())).unwrap();
        let via_helper = crate::model3d::decode_glb_geometry_with_meshopt(
            glb,
            || Ok(()),
            |jobs| {
                assert!(!jobs.is_empty());
                let req = encode_meshopt_request(jobs)?;
                let mut reply = Vec::new();
                serve_buffer(req, &mut reply)?;
                read_meshopt_reply(&reply[..])?
                    .map_err(|kind| anyhow::anyhow!("meshopt helper error: {kind:?}"))
            },
        )
        .unwrap();
        assert_eq!(
            direct.scene.triangle_count(),
            via_helper.scene.triangle_count()
        );
    }
}
