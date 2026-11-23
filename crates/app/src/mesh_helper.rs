//! Confined Linux 3D native decoders (`ufbx` for `.obj`/`.fbx` and `meshopt`
//! for GLB `EXT_meshopt_compression` views).
//!
//! On Linux x86-64 and AArch64 production builds, `.obj`, `.fbx`, and GLB
//! `EXT_meshopt_compression` buffer views run inside GitTurtle's seccomp
//! helper process (`crate::pdf_helper::Runner`) using the same two slots and
//! 10 s deadline as PDF pages and AVIF/JPEG 2000 images. They never fall back
//! to an in-process native decode on helper failure. Plain GLB, STL, 3MF, and
//! STEP stay pure-Rust in-process.
//!
//! Exposed `#[doc(hidden)]` on Linux x86-64/AArch64 so `tests/mesh_helper.rs`
//! exercises the real `gitturtle` helper binary under seccomp.

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
use std::collections::HashMap;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
use std::fmt;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
use std::path::Path;

use gitturtle_preview::model3d::ModelGeometry;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
use gitturtle_preview::model3d::{
    MeshoptViewJob, ModelTriangle, decode_glb_geometry_with_meshopt, finish_ufbx_geometry,
};
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
use gitturtle_preview::pdf_helper::{
    MAX_MESHOPT_REPLY_BYTES, MAX_UFBX_REPLY_BYTES, MeshErrorKind, encode_meshopt_request,
    encode_ufbx_request, read_meshopt_reply, read_ufbx_reply,
};

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
use crate::pdf_helper::{Outcome, Runner};

/// Decodes 3D model geometry from supplied bytes. On Linux x86-64/AArch64,
/// native C/C++ decoders (`ufbx` and `meshopt`) run in the confined helper.
pub fn decode_geometry(
    bytes: &[u8],
    name: &str,
    check: &dyn Fn() -> anyhow::Result<()>,
) -> anyhow::Result<ModelGeometry> {
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64"),
        not(test)
    ))]
    {
        static CLIENT: std::sync::OnceLock<Client<'static>> = std::sync::OnceLock::new();
        let client = CLIENT.get_or_init(Client::new);
        decode_geometry_with(client, bytes, name, check)
    }
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64"),
        test
    ))]
    {
        // Unit tests inside `cargo test --lib` run under the test harness binary
        // rather than `bin/gitturtle`; exercise the exact binary IPC framing
        // in-process while `tests/mesh_helper.rs` tests the real seccomp binary.
        decode_geometry_framed_in_process(bytes, name, check)
    }
    #[cfg(not(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    {
        gitturtle_preview::model3d::decode_geometry(bytes, name, check)
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64"),
    test
))]
fn decode_geometry_framed_in_process(
    bytes: &[u8],
    name: &str,
    check: &dyn Fn() -> anyhow::Result<()>,
) -> anyhow::Result<ModelGeometry> {
    check()?;
    let ext = Path::new(name)
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "obj" | "fbx" => {
            let obj = ext == "obj";
            let header = encode_ufbx_request(bytes, obj)?;
            let mut request = Vec::with_capacity(header.len() + bytes.len());
            request.extend_from_slice(&header);
            request.extend_from_slice(bytes);
            let mut reply = Vec::new();
            gitturtle_preview::pdf_helper::serve_buffer(request, &mut reply)?;
            let triangles = read_ufbx_reply(&reply[..])
                .map_err(|_| anyhow::anyhow!("{}", Failure::Unreadable))?
                .map_err(|kind| anyhow::anyhow!("{}", Failure::Document(kind)))?;
            check()?;
            finish_ufbx_geometry(triangles, obj)
        }
        "glb" => decode_glb_geometry_with_meshopt(bytes, check, |jobs| {
            check()?;
            let request = encode_meshopt_request(jobs)?;
            let mut reply = Vec::new();
            gitturtle_preview::pdf_helper::serve_buffer(request, &mut reply)?;
            let decoded = read_meshopt_reply(&reply[..])
                .map_err(|_| anyhow::anyhow!("{}", Failure::Unreadable))?
                .map_err(|kind| anyhow::anyhow!("{}", Failure::Document(kind)))?;
            check()?;
            Ok(decoded)
        }),
        _ => gitturtle_preview::model3d::decode_geometry(bytes, name, check),
    }
}

/// Decodes 3D model geometry through a specific confined helper [`Client`].
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub fn decode_geometry_with(
    client: &Client<'_>,
    bytes: &[u8],
    name: &str,
    check: &dyn Fn() -> anyhow::Result<()>,
) -> anyhow::Result<ModelGeometry> {
    check()?;
    let ext = Path::new(name)
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "obj" | "fbx" => {
            let obj = ext == "obj";
            let triangles = client.decode_ufbx(bytes, obj, check).map_err(worded)?;
            check()?;
            finish_ufbx_geometry(triangles, obj)
        }
        "glb" => decode_glb_geometry_with_meshopt(bytes, check, |jobs| {
            client.decode_meshopt(jobs, check).map_err(worded)
        }),
        _ => gitturtle_preview::model3d::decode_geometry(bytes, name, check),
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn worded(failure: Failure) -> anyhow::Error {
    match failure {
        Failure::Cancelled(error) => error,
        failure => anyhow::anyhow!("{failure}"),
    }
}

/// Decodes `.obj`/`.fbx` meshes and GLB `EXT_meshopt_compression` view batches
/// in confined helper processes.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub struct Client<'a> {
    runner: Runner<'a>,
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
impl Client<'static> {
    pub fn new() -> Self {
        Self {
            runner: Runner::new(),
        }
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
impl Default for Client<'static> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
impl<'a> Client<'a> {
    #[cfg(any(test, feature = "test-support"))]
    pub fn with(runner: Runner<'a>) -> Self {
        Self { runner }
    }

    pub fn decode_ufbx(
        &self,
        bytes: &[u8],
        obj: bool,
        check: &dyn Fn() -> anyhow::Result<()>,
    ) -> Result<Vec<ModelTriangle>, Failure> {
        let header = encode_ufbx_request(bytes, obj).map_err(Failure::Refused)?;
        let (outcome, _) = self
            .runner
            .run_timed([&header[..], bytes], MAX_UFBX_REPLY_BYTES, check);
        classify_ufbx(outcome)
    }

    pub fn decode_meshopt(
        &self,
        jobs: &[MeshoptViewJob],
        check: &dyn Fn() -> anyhow::Result<()>,
    ) -> Result<HashMap<usize, Vec<u8>>, Failure> {
        let frame = encode_meshopt_request(jobs).map_err(Failure::Refused)?;
        let (outcome, _) =
            self.runner
                .run_timed([frame.as_slice(), &[]], MAX_MESHOPT_REPLY_BYTES, check);
        classify_meshopt(outcome)
    }
}

/// Fixed failure categories returned by [`Client`].
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[derive(Debug)]
pub enum Failure {
    Cancelled(anyhow::Error),
    Refused(anyhow::Error),
    Document(MeshErrorKind),
    TimedOut,
    Stopped { signal: Option<i32> },
    Unreadable,
    CouldNotStart,
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sentence = match self {
            Self::Cancelled(error) | Self::Refused(error) => return write!(formatter, "{error}"),
            Self::Document(kind) => match kind {
                MeshErrorKind::InputTooLarge => "3D source exceeds the 32 MiB preview limit",
                MeshErrorKind::DecodeFailed => "Could not decode 3D model geometry.",
                MeshErrorKind::DecoderPanicked => "The 3D model decoder stopped on this file.",
                MeshErrorKind::Request => "The 3D model decoder returned an unreadable result.",
            },
            Self::TimedOut => "3D model decoding timed out after 10 seconds.",
            Self::Stopped { .. } => "The 3D model decoder stopped on this file.",
            Self::Unreadable => "The 3D model decoder returned an unreadable result.",
            Self::CouldNotStart => "The 3D model decoder could not start.",
        };
        formatter.write_str(sentence)
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn classify_outcome<T>(
    outcome: Outcome,
    parse: impl FnOnce(&[u8]) -> std::io::Result<Result<T, MeshErrorKind>>,
) -> Result<T, Failure> {
    match outcome {
        Outcome::Replied(bytes) => match parse(&bytes) {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(kind)) => Err(Failure::Document(kind)),
            Err(_) => Err(Failure::Unreadable),
        },
        Outcome::Cancelled(error) => Err(Failure::Cancelled(error)),
        Outcome::TimedOut => Err(Failure::TimedOut),
        Outcome::Oversized => Err(Failure::Unreadable),
        Outcome::CouldNotStart => Err(Failure::CouldNotStart),
        Outcome::Stopped { signal } => Err(Failure::Stopped { signal }),
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn classify_ufbx(outcome: Outcome) -> Result<Vec<ModelTriangle>, Failure> {
    classify_outcome(outcome, |bytes| read_ufbx_reply(bytes))
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn classify_meshopt(outcome: Outcome) -> Result<HashMap<usize, Vec<u8>>, Failure> {
    classify_outcome(outcome, |bytes| read_meshopt_reply(bytes))
}

#[cfg(test)]
mod boundary_tests {
    use std::path::Path;

    fn non_test_prefix(source: &str) -> &str {
        source.split("\n#[cfg(test)]").next().unwrap_or(source)
    }

    fn visit_rs_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit_rs_files(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }

    /// Defense-in-depth: production code in `crates/app/src` must route 3D model
    /// geometry decoding through `crate::mesh_helper::decode_geometry` so native
    /// `ufbx` and `meshopt` decoders never bypass Linux seccomp confinement.
    #[test]
    fn app_production_code_never_calls_unconfined_model3d_decoders_directly() {
        let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        visit_rs_files(&src_dir, &mut files);
        let mut violations = Vec::new();
        for file in files {
            let rel = file
                .strip_prefix(&src_dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if rel == "mesh_helper.rs" || rel.ends_with("_tests.rs") || rel.contains("/tests") {
                continue;
            }
            let content = std::fs::read_to_string(&file).unwrap();
            let prod = non_test_prefix(&content);
            for forbidden in [
                "model3d::decode_geometry(",
                "decode_ufbx_triangles(",
                "decode_meshopt_job(",
                "decode_glb_geometry_with_meshopt(",
            ] {
                if prod.contains(forbidden) {
                    violations.push(format!("{rel}: calls `{forbidden}` outside mesh_helper.rs"));
                }
            }
        }
        assert!(
            violations.is_empty(),
            "unconfined 3D decoder call(s) in production app code:\n{}",
            violations.join("\n")
        );
    }
}
