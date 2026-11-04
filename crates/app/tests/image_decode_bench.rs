//! The release measurement of Linux AVIF and JPEG 2000 images through the
//! real helper (section 5 of
//! `docs/development/2026-11-02-linux-avif-jpeg2000-architecture.md`, and
//! requirement 18 of its section 7).
//!
//! Opt-in and ignored by default. Every regular `*.avif`, `*.jp2`, `*.j2k`,
//! `*.png` and `*.jpg` file in the directory named by
//! `GITTURTLE_IMAGE_BENCH_DIR` is measured (`scripts/create-image-bench-inputs.py`
//! writes section 5's set), never one reached through a symbolic link, and
//! one past the 32 MiB input limit is refused from its size, unread. Each
//! image is decoded `GITTURTLE_IMAGE_BENCH_RUNS` times (default 20) at
//! History's 1,600-pixel edge and routed by its content as the app routes it
//! (`image_helper::decode_with`): AVIF and JPEG 2000 go to a new confined
//! helper per request, timed phase by phase through an instrumented exchange
//! and as a whole through the app's own client; PNG and JPEG go to
//! `gitturtle_preview::decode_image` in this process, for comparison. The
//! helper's own peak resident memory, its `VmHWM`, is read from `/proc` in
//! untimed requests of their own, while the helper waits in its reply's
//! write (`wait4`'s `ru_maxrss` is no measure: the helper carries this
//! process's high-water mark across its `exec`). The helper's start, its
//! `VmSize` once it is confined, and the time from a kill to the reap are
//! measured apart. Requirement 18's line is its designated input's peak,
//! the 30 MP 12-bit 4:4:4 AVIF with alpha and film grain, and only where
//! every run accepted it; the largest accepted AVIF has a line of its own.
//! A confined image of at most 512 KiB, the most a Markdown side can send
//! for each of its 32 images, is also decoded at a Markdown document's
//! 1,000-pixel edge through the app's client, and the slowest of those (the
//! security review's T) is reported with 32 times its time and with the
//! side's time as the document's one-deadline total bounds it. The JSON
//! goes to the file named by
//! `GITTURTLE_IMAGE_BENCH_OUT`. It names each image by its file name in that
//! directory and its SHA-256, and the executable by its file name, the commit
//! built into it (looked for in its first 256 MiB) and whether the source
//! tree it was built from was clean, never by an absolute path, and no
//! helper's reply is read past the largest the client accepts at that edge.
//!
//! The app crate has no library target, so this test compiles
//! `src/pdf_helper.rs` and `src/image_helper.rs` as its own modules, as
//! `tests/image_helper.rs` does.
#![cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

#[allow(
    dead_code,
    reason = "the measurement uses the runner, the client and their constants"
)]
#[path = "../src/pdf_helper.rs"]
mod pdf_helper;

#[allow(dead_code, reason = "the measurement uses the client and its routing")]
#[path = "../src/image_helper.rs"]
mod image_helper;

use std::fs;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use gitturtle_preview::MAX_INPUT_BYTES;
use gitturtle_preview::pdf_helper::{
    ConfinedFormat, MAX_HELPER_IMAGE_EDGE, PROTOCOL_VERSION, encode_image_request,
    max_image_reply_bytes, read_image_reply,
};
use image_helper::{Client, Failure};
use pdf_helper::{DEADLINE, HELPER_FLAG, HelperCommand, Runner, Slots};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const BINARY: &str = env!("CARGO_BIN_EXE_gitturtle");
/// The commit the build script embeds in this package's targets, the
/// executable's among them.
const REVISION: &str = env!("GITTURTLE_BUILD_REVISION");
/// Whether the source tree the build script saw was "clean", "modified" or
/// "unknown", as `--build-info` reports it as `source_tree`: one script run
/// builds this test and the executable.
const TREE: &str = env!("GITTURTLE_BUILD_TREE");
/// The most of the executable read to find its commit: a release build is
/// about 100 MB.
const MAX_EXECUTABLE_BYTES: usize = 256 * 1024 * 1024;
/// History's preview edge (`worker.rs`), which is also the most the helper
/// returns.
const EDGE: u32 = MAX_HELPER_IMAGE_EDGE;
/// The file name extensions measured; any other file is not listed.
const EXTENSIONS: [&str; 5] = ["avif", "jp2", "j2k", "png", "jpg"];
/// Half of the helper's 1 GiB `RLIMIT_DATA`, the largest accepted image's
/// peak budget (requirement 18).
const PEAK_BUDGET_KIB: u64 = 512 * 1024;
/// A Markdown document's image edge (`markdown_view.rs`).
const MARKDOWN_EDGE: u32 = 1_000;
/// A Markdown side's most images, each of which may name the same asset.
const MARKDOWN_IMAGES: u64 = 32;
/// The largest confined image a Markdown side can send to a helper for each
/// of its 32 images: its 16 MiB of captured asset bytes over 32
/// (`docs/document-previews.md`).
const MARKDOWN_SHARE_BYTES: usize = 16 * 1024 * 1024 / 32;

/// Bytes to the largest any reader here takes: `limit`, and one more, which
/// tells a source past it from one that ends there.
fn read_bounded(source: impl Read, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    source
        .take(u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// An image from the directory: only a regular file, never one a symbolic
/// link names, and only within the input limit. It is opened without
/// following a link and without waiting on a FIFO, and its kind and size are
/// read from the open file before a byte of it is: a link, a directory, a
/// FIFO or a file past the limit is refused unread. A file that grows past
/// the limit after that is read no further than one byte past it.
fn read_input(path: &Path) -> Result<Vec<u8>, &'static str> {
    let file = match fs::File::options()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        // `O_NOFOLLOW` refuses a link as its last component with `ELOOP`.
        Err(error) if error.raw_os_error() == Some(libc::ELOOP) => {
            return Err("not a regular file");
        }
        Err(_) => return Err("unreadable"),
    };
    let metadata = file.metadata().map_err(|_| "unreadable")?;
    if !metadata.is_file() {
        return Err("not a regular file");
    }
    if metadata.len() > u64::try_from(MAX_INPUT_BYTES).unwrap_or(u64::MAX) {
        return Err("larger than the 32 MiB input limit");
    }
    let bytes = read_bounded(file, MAX_INPUT_BYTES).map_err(|_| "unreadable")?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err("larger than the 32 MiB input limit");
    }
    Ok(bytes)
}

/// Whether `path` names one of the measured kinds by its extension.
fn listed(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            EXTENSIONS
                .iter()
                .any(|listed| listed.eq_ignore_ascii_case(extension))
        })
}

/// The helper's arguments for this process as its parent.
fn arguments() -> [String; 3] {
    [
        HELPER_FLAG.to_owned(),
        PROTOCOL_VERSION.to_string(),
        std::process::id().to_string(),
    ]
}

/// A helper started as the client starts one.
fn spawn() -> Child {
    Command::new(BINARY)
        .args(arguments())
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the helper starts")
}

fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

/// Reaps `child`, returning whether it exited 0.
fn reap(mut child: Child) -> bool {
    child.wait().expect("the helper is reaped").success()
}

/// A reply read no further than one byte past `limit`, with when its first
/// bytes arrived and when it ended.
fn read_timed(source: impl Read, limit: usize) -> (Vec<u8>, Option<Instant>, Instant) {
    let mut source = source.take(u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1));
    let mut bytes = Vec::new();
    let mut first = None;
    let mut buffer = vec![0; 64 * 1024];
    loop {
        match source.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                first.get_or_insert_with(Instant::now);
                bytes.extend_from_slice(&buffer[..read]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    (bytes, first, Instant::now())
}

/// One request through an instrumented exchange: blocking pipes, each phase
/// timed from this side, and the helper's own phase timings.
fn instrumented(bytes: &[u8], format: ConfinedFormat) -> Value {
    let header = encode_image_request(format, EDGE, bytes).expect("a request");
    let cap = max_image_reply_bytes(EDGE);
    let started = Instant::now();
    let mut child = spawn();
    let spawned = started.elapsed();
    let mut input = child.stdin.take().expect("stdin");
    let output = child.stdout.take().expect("stdout");
    // Reading on a thread of its own keeps a reply larger than the pipe from
    // blocking the write; no further than one byte past the largest reply
    // the client accepts at this edge.
    let reader = std::thread::spawn(move || read_timed(output, cap));
    let written = input
        .write_all(&header)
        .and_then(|()| input.write_all(bytes))
        .map(|()| started.elapsed());
    drop(input);
    let (reply, first, last) = reader.join().expect("the reader");
    let exited = reap(child);
    let request = started.elapsed();
    let first = first.map(|first| first - started);
    let last = last - started;
    // Each phase on its own: the spawn; writing the request, which a request
    // larger than the pipe makes wait for the helper to start and read it;
    // from the request's last byte to the reply's first (the helper's parse,
    // decode, conversion and resize, and the start of its write, and for a
    // request the pipe holds whole, which the write leaves there before the
    // helper has started, its start and confinement as well); the rest of the
    // reply; and reaping after it.
    let mut run = json!({
        "spawn_us": micros(spawned),
        "write_us": written.as_ref().ok().map(|written| micros(written.saturating_sub(spawned))),
        "decode_and_reply_us": written.as_ref().ok().zip(first).map(|(written, first)| micros(first.saturating_sub(*written))),
        "reply_read_us": first.map(|first| micros(last.saturating_sub(first))),
        "reap_us": micros(request.saturating_sub(last)),
        "request_us": micros(request),
        "reply_bytes": reply.len(),
        "exited_0": exited,
    });
    match read_image_reply(reply.as_slice(), format, EDGE) {
        Ok(Ok(decoded)) => {
            let phases = decoded.timings;
            let helper = u64::from(phases.parse_us)
                + u64::from(phases.decode_us)
                + u64::from(phases.convert_us);
            run["helper_parse_us"] = json!(phases.parse_us);
            run["helper_decode_us"] = json!(phases.decode_us);
            run["helper_convert_us"] = json!(phases.convert_us);
            // Start, transfer and exit around the helper's own work.
            run["overhead_us"] = json!(micros(request).saturating_sub(helper));
            run["pixels"] = json!([decoded.image.width, decoded.image.height]);
            run["original"] = json!([decoded.image.original_width, decoded.image.original_height]);
            // Built from the format echo and the flags alone.
            run["caption"] = json!(decoded.flags.caption(decoded.format));
        }
        // The client's wording only: a reply's bytes never reach the record.
        Ok(Err(kind)) => run["error"] = json!(Failure::Document(kind).to_string()),
        Err(_) => run["error"] = json!(Failure::Unreadable.to_string()),
    }
    run
}

/// One whole decode at `edge` as the app makes it: through
/// `image_helper::decode_with`, which sends a confined format to a new helper
/// on `slots` and decodes any other image in this process.
fn through_app(bytes: &[u8], name: &str, slots: &Slots, edge: u32) -> Value {
    let client = Client::with(Runner::with(
        HelperCommand::new(BINARY, arguments()),
        DEADLINE,
        slots,
    ));
    let started = Instant::now();
    let result = image_helper::decode_with(&client, bytes, name, edge, &|| Ok(()));
    let request = started.elapsed();
    match result {
        Ok(image) => json!({
            "request_us": micros(request),
            "pixels": [image.width, image.height],
            "original": [image.original_width, image.original_height],
            "caption": image.format,
        }),
        // `Display` only: the wording a person would read.
        Err(error) => json!({ "request_us": micros(request), "error": error.to_string() }),
    }
}

/// The number after `key` in a `/proc/<pid>/status` text.
fn status_value(status: &str, key: &str) -> Option<u64> {
    status
        .lines()
        .find_map(|line| line.strip_prefix(key))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse().ok())
}

/// The seccomp filters this process runs under, which a helper inherits
/// and then adds its own to.
fn own_filters() -> u64 {
    let own = fs::read_to_string("/proc/self/status").expect("own status");
    status_value(&own, "Seccomp_filters:").unwrap_or(0)
}

/// `child`'s `/proc` status once it is confined: named as the helper and
/// with more seccomp filters than `inherited`. The helper confines itself
/// before it reads its request. `None` where it has already ended, as a
/// refused image's helper can before it is seen. Its state is read first: an
/// ended process keeps its name and filters until it is reaped.
fn confined(child: &Child, inherited: u64) -> Option<String> {
    let path = format!("/proc/{}/status", child.id());
    let started = Instant::now();
    loop {
        let status = fs::read_to_string(&path).unwrap_or_default();
        if status.contains("State:\tZ") {
            return None;
        }
        // One helper process serves PDF pages and images alike.
        if status.contains("Name:\tgitturtle-pdf")
            && status_value(&status, "Seccomp_filters:").unwrap_or(0) > inherited
        {
            return Some(status);
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the helper confines itself"
        );
        // Polled every 100 µs, so the probe takes no core from the helper
        // it times on a busy host; confinement reads up to 100 µs late.
        std::thread::sleep(Duration::from_micros(100));
    }
}

/// A new helper's start: from the spawn until it is confined (named, with
/// more seccomp filters than this process) and waiting for its request, its
/// `VmSize` and `VmRSS` then, and the time from a kill to the reap.
fn start_and_kill() -> Value {
    let inherited = own_filters();
    let started = Instant::now();
    let mut child = spawn();
    // It waits for its request, so it never ends first.
    let status = confined(&child, inherited).expect("the helper confines itself");
    let confined = started.elapsed();
    let killed = Instant::now();
    child.kill().expect("the kill");
    let _ = reap(child);
    json!({
        "start_us": micros(confined),
        "vm_size_kib": status_value(&status, "VmSize:"),
        "vm_rss_kib": status_value(&status, "VmRSS:"),
        "kill_to_reap_idle_us": micros(killed.elapsed()),
    })
}

/// The time from a kill to the reap of a helper at work on `bytes`, killed
/// once its whole request is written and it is confined, never while it
/// still starts, as a request the pipe holds whole would otherwise let it be.
/// Its reply's pipe holds one page, as `peak_kib`'s does, so a helper whose
/// work is done waits in its reply's write rather than ending first: the kill
/// finds it decoding or in that write (`timed_kill`). `None` where it ended
/// before it was seen confined, as a refused image's helper can.
fn kill_at_work(bytes: &[u8], format: ConfinedFormat) -> Option<u64> {
    let header = encode_image_request(format, EDGE, bytes).expect("a request");
    let inherited = own_filters();
    let mut child = spawn();
    let mut input = child.stdin.take().expect("stdin");
    let output = child.stdout.take().expect("stdout");
    // SAFETY: resizes a pipe this process holds, still empty, to the least
    // the kernel allows, one page.
    let resized = unsafe { libc::fcntl(output.as_raw_fd(), libc::F_SETPIPE_SZ, 1) };
    assert!(resized > 0, "the reply pipe holds one page");
    let _ = input
        .write_all(&header)
        .and_then(|()| input.write_all(bytes));
    timed_kill(child, inherited, (input, output))
}

/// The time from a kill to the reap of `child` once it is seen confined
/// (`confined`), with `pipes`, this process's ends of its pipes, open until
/// it is reaped. `None`, the helper reaped and no kill timed, where it ended
/// before it was seen confined, as the PDF bench records it: a kill there
/// would time the reap of a process that had already exited.
fn timed_kill(mut child: Child, inherited: u64, pipes: impl Sized) -> Option<u64> {
    if confined(&child, inherited).is_none() {
        let _ = reap(child);
        return None;
    }
    let killed = Instant::now();
    child.kill().expect("the kill");
    let _ = reap(child);
    drop(pipes);
    Some(micros(killed.elapsed()))
}

/// Requests per image whose helper's peak is read.
const PEAK_RUNS: usize = 5;
/// How long a helper may take to begin its reply: past its own 12-second
/// alarm, which ends it.
const PEAK_WAIT: Duration = Duration::from_secs(20);

/// The helper's own peak resident set in KiB, its `VmHWM`, for one request.
/// Its reply's pipe holds a single page and nothing reads it until the peak
/// is read, so a helper whose reply has begun, its work and every allocation
/// for it done, waits in its write for as long as the reply is larger than
/// that page: an image of 64 × 64 pixels or more always is, with 4 KiB and
/// 16 KiB pages. `None` where the helper ends first, as a smaller image's
/// reply lets it, and wherever the reply is not an image the client accepts
/// (`accepted_peak`).
fn peak_kib(bytes: &[u8], format: ConfinedFormat) -> Option<u64> {
    let header = encode_image_request(format, EDGE, bytes).expect("a request");
    let mut child = spawn();
    let mut input = child.stdin.take().expect("stdin");
    let mut output = child.stdout.take().expect("stdout");
    let reply = output.as_raw_fd();
    // SAFETY: resizes a pipe this process holds, still empty, to the least
    // the kernel allows, one page.
    let resized = unsafe { libc::fcntl(reply, libc::F_SETPIPE_SZ, 1) };
    assert!(resized > 0, "the reply pipe holds one page");
    // The helper reads the whole request before it replies.
    let _ = input
        .write_all(&header)
        .and_then(|()| input.write_all(bytes));
    drop(input);
    let path = format!("/proc/{}/status", child.id());
    let started = Instant::now();
    let peak = loop {
        let mut queued: libc::c_int = 0;
        // SAFETY: FIONREAD writes the count of bytes in the pipe to the int.
        let replying =
            unsafe { libc::ioctl(reply, libc::FIONREAD, &mut queued) } == 0 && queued > 0;
        // Read once the reply has begun, the status holds the helper's peak;
        // an ended helper's has no `VmHWM`.
        let peak = status_value(&fs::read_to_string(&path).unwrap_or_default(), "VmHWM:");
        if replying || peak.is_none() {
            break peak.filter(|_| replying);
        }
        if started.elapsed() > PEAK_WAIT {
            let _ = child.kill();
            break None;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    // No further than one byte past the largest reply the client accepts.
    let reply = read_bounded(&mut output, max_image_reply_bytes(EDGE)).unwrap_or_default();
    let _ = reap(child);
    accepted_peak(peak, &reply, format)
}

/// `peak` where `reply` is an image the client accepts, else `None`: a
/// refused image's helper can still be alive, its peak readable, while its
/// short error reply waits in the pipe, and that peak is no accepted image's.
fn accepted_peak(peak: Option<u64>, reply: &[u8], format: ConfinedFormat) -> Option<u64> {
    peak.filter(|_| matches!(read_image_reply(reply, format, EDGE), Ok(Ok(_))))
}

/// p50, p95 and max of each numeric `metric` over `runs`.
fn summary(runs: &[Value], metrics: &[&str]) -> Value {
    let mut summary = serde_json::Map::new();
    for metric in metrics {
        let mut values: Vec<u64> = runs
            .iter()
            .filter_map(|run| run[*metric].as_u64())
            .collect();
        if values.is_empty() {
            continue;
        }
        values.sort_unstable();
        let at = |quantile: f64| {
            values[((values.len() as f64 * quantile).ceil() as usize).clamp(1, values.len()) - 1]
        };
        summary.insert(
            (*metric).to_owned(),
            json!({ "p50": at(0.5), "p95": at(0.95), "max": values[values.len() - 1], "n": values.len() }),
        );
    }
    Value::Object(summary)
}

const RUN_METRICS: &[&str] = &[
    "spawn_us",
    "write_us",
    "decode_and_reply_us",
    "reply_read_us",
    "reap_us",
    "helper_parse_us",
    "helper_decode_us",
    "helper_convert_us",
    "overhead_us",
    "request_us",
];

fn host() -> Value {
    let cpu = fs::read_to_string("/proc/cpuinfo")
        .unwrap_or_default()
        .lines()
        .find(|line| line.starts_with("model name") || line.starts_with("CPU part"))
        .map(|line| line.split(':').nth(1).unwrap_or("").trim().to_owned());
    json!({
        "arch": std::env::consts::ARCH,
        "kernel": fs::read_to_string("/proc/sys/kernel/osrelease").map(|release| release.trim().to_owned()).ok(),
        "cpu": cpu,
        "cpus": std::thread::available_parallelism().map(usize::from).ok(),
    })
}

/// The measurement of one confined image: the instrumented exchange, the
/// app's client, the helper's peak and a kill at work.
fn through_helper(
    bytes: &[u8],
    name: &str,
    format: ConfinedFormat,
    runs: usize,
    slots: &Slots,
) -> Value {
    let measured: Vec<Value> = (0..runs).map(|_| instrumented(bytes, format)).collect();
    let app: Vec<Value> = (0..runs)
        .map(|_| through_app(bytes, name, slots, EDGE))
        .collect();
    let peaks: Vec<Value> = (0..PEAK_RUNS)
        .map(|_| json!({ "vm_hwm_kib": peak_kib(bytes, format) }))
        .collect();
    json!({
        "route": "helper",
        "first": measured[0],
        "warm": summary(&measured[1..], RUN_METRICS),
        // The client's first request follows the instrumented runs, so it is
        // this image's first through the client, not its first decode: that
        // is `first`.
        "app_first": app[0],
        "app_warm": summary(&app[1..], &["request_us"]),
        "errors": errors(&measured, &app),
        // `null` where the helper ended before it was seen confined.
        "kill_to_reap_at_work_us": kill_at_work(bytes, format),
        // `n` counts the requests whose peak was read.
        "helper_peak": summary(&peaks, &["vm_hwm_kib"]),
        "markdown": (bytes.len() <= MARKDOWN_SHARE_BYTES)
            .then(|| at_markdown_edge(bytes, name, runs, slots)),
    })
}

/// A confined image within Markdown's share, decoded `runs` times at a
/// Markdown document's edge through the app's client.
fn at_markdown_edge(bytes: &[u8], name: &str, runs: usize, slots: &Slots) -> Value {
    let app: Vec<Value> = (0..runs)
        .map(|_| through_app(bytes, name, slots, MARKDOWN_EDGE))
        .collect();
    json!({
        "edge": MARKDOWN_EDGE,
        "app_first": app[0],
        "app_warm": summary(&app[1..], &["request_us"]),
        "errors": errors(&[], &app),
    })
}

/// T for the security review's note N1: of the confined images within
/// Markdown's share that every run at its edge decoded, the one whose
/// slowest request there, first or warm, took longest, and the helper time
/// it gives a side of 32 such images, one helper slot's work for the side:
/// 32 × T unbounded, and as the document total bounds it.
fn slowest_markdown_image(records: &[Value]) -> Value {
    let slowest = records
        .iter()
        .filter(|record| {
            record["markdown"]["errors"]
                .as_array()
                .is_some_and(Vec::is_empty)
        })
        .filter_map(|record| {
            let markdown = &record["markdown"];
            let first = markdown["app_first"]["request_us"].as_u64();
            let warm = markdown["app_warm"]["request_us"]["max"].as_u64();
            first.max(warm).map(|slowest| (slowest, record))
        })
        .max_by_key(|(slowest, _)| *slowest);
    json!({
        "name": slowest.map(|(_, record)| &record["name"]),
        "bytes": slowest.map(|(_, record)| &record["bytes"]),
        "edge": MARKDOWN_EDGE,
        "share_bytes": MARKDOWN_SHARE_BYTES,
        "slowest_request_us": slowest.map(|(slowest, _)| slowest),
        "warm_request_us": slowest.map(|(_, record)| &record["markdown"]["app_warm"]["request_us"]),
        "images_per_side": MARKDOWN_IMAGES,
        "document_deadline_us": micros(DEADLINE),
        "per_side_unbounded_us": slowest.map(|(slowest, _)| slowest.saturating_mul(MARKDOWN_IMAGES)),
        // `crates/app/src/image_helper.rs`'s `DocumentImages` sends a side's
        // next confined image only while its helpers have run less than one
        // deadline in total, so the last one sent starts under it and runs
        // at most T more: under the deadline plus T, at most two deadlines.
        "per_side_bounded_us": slowest.map(|(slowest, _)| {
            slowest
                .saturating_mul(MARKDOWN_IMAGES)
                .min(micros(DEADLINE).saturating_add(slowest))
        }),
    })
}

/// The measurement of an image the app decodes in its own process.
fn in_process(bytes: &[u8], name: &str, runs: usize, slots: &Slots) -> Value {
    let app: Vec<Value> = (0..runs)
        .map(|_| through_app(bytes, name, slots, EDGE))
        .collect();
    json!({
        "route": "in-process",
        "app_first": app[0],
        "app_warm": summary(&app[1..], &["request_us"]),
        "errors": errors(&[], &app),
    })
}

/// Every run's error sentence, in run order.
fn errors<'a>(measured: &'a [Value], app: &'a [Value]) -> Vec<&'a str> {
    measured
        .iter()
        .chain(app)
        .filter_map(|run| run["error"].as_str())
        .collect()
}

/// The image whose helper peaked highest among the `records` that `include`
/// keeps, with that peak against half of `RLIMIT_DATA`. Only an accepted
/// image's peak is recorded (`accepted_peak`), so a refused one never wins.
fn largest_peak(records: &[Value], include: impl Fn(&Value) -> bool) -> Value {
    let largest = records
        .iter()
        .filter(|record| include(record))
        .filter_map(|record| {
            record["helper_peak"]["vm_hwm_kib"]["max"]
                .as_u64()
                .map(|peak| (peak, record))
        })
        .max_by_key(|(peak, _)| *peak);
    json!({
        "name": largest.map(|(_, record)| &record["name"]),
        "format": largest.map(|(_, record)| &record["format"]),
        "vm_hwm_kib": largest.map(|(peak, _)| peak),
        "budget_kib": PEAK_BUDGET_KIB,
        "within_budget": largest.map(|(peak, _)| peak <= PEAK_BUDGET_KIB),
    })
}

/// What names an input whose alpha item is itself a 4:4:4 picture with film
/// grain, as the generator writes one: no encoder makes such an alpha item
/// for a photograph, but nothing refuses it, and with grain each item's
/// decode holds two full pictures, so its peak passes requirement 18's.
const HOSTILE_ALPHA: &str = "hostile-alpha";

fn hostile(record: &Value) -> bool {
    record["name"]
        .as_str()
        .is_some_and(|name| name.contains(HOSTILE_ALPHA))
}

/// Requirement 18's designated input, section 5's 30 MP 12-bit 4:4:4 AVIF
/// with alpha and film grain, as `scripts/create-image-bench-inputs.py`
/// names it.
const REQUIREMENT_18_INPUT: &str = "avif-6720x4480-12bit-444-alpha-grain.avif";

/// Requirement 18's line: the designated input's peak against half of
/// `RLIMIT_DATA`, measured only when every run accepted it, each timed
/// request and each peak request. Otherwise `designated_measured` is false
/// with the reason, the line has no peak and `within_budget` is `null`: no
/// other AVIF stands in for it, however large.
fn requirement_18(records: &[Value]) -> Value {
    let record = records
        .iter()
        .find(|record| record["name"] == REQUIREMENT_18_INPUT);
    let timed_out = Failure::TimedOut.to_string();
    let reason = match record {
        None => Some("absent: not among the measured inputs (see skipped_inputs)"),
        Some(record) if record["format"] != "Avif" => Some("not read as an AVIF"),
        Some(record) => {
            let errors = record["errors"].as_array();
            let peaks = record["helper_peak"]["vm_hwm_kib"]["n"].as_u64();
            if errors.is_some_and(|errors| {
                errors
                    .iter()
                    .any(|error| error.as_str() == Some(timed_out.as_str()))
            }) {
                Some("timed out")
            } else if !errors.is_some_and(Vec::is_empty) {
                Some("refused")
            } else if peaks != u64::try_from(PEAK_RUNS).ok() {
                Some("its peak was not read in every peak request")
            } else {
                None
            }
        }
    };
    let peak = reason
        .is_none()
        .then(|| record.and_then(|record| record["helper_peak"]["vm_hwm_kib"]["max"].as_u64()))
        .flatten();
    json!({
        "name": REQUIREMENT_18_INPUT,
        "designated_measured": peak.is_some(),
        "reason": reason,
        "errors": record.map(|record| &record["errors"]),
        "vm_hwm_kib": peak,
        "budget_kib": PEAK_BUDGET_KIB,
        "within_budget": peak.map(|peak| peak <= PEAK_BUDGET_KIB),
    })
}

/// The peaks the record reports: requirement 18's designated input's, the
/// largest accepted AVIF's of any kind and apart from it the largest
/// accepted image's of any confined format, each against half of
/// `RLIMIT_DATA`, and each hostile-alpha input on a line of its own, with
/// its errors where it was not accepted. None of the first three counts a
/// hostile-alpha input.
fn peak_lines(records: &[Value]) -> Value {
    let hostile_alpha: Vec<Value> = records
        .iter()
        .filter(|record| hostile(record))
        .map(|record| {
            let peak = record["helper_peak"]["vm_hwm_kib"]["max"].as_u64();
            json!({
                "name": record["name"],
                "vm_hwm_kib": peak,
                "budget_kib": PEAK_BUDGET_KIB,
                "within_budget": peak.map(|peak| peak <= PEAK_BUDGET_KIB),
                "errors": record["errors"],
            })
        })
        .collect();
    json!({
        "requirement_18": requirement_18(records),
        "largest_accepted_avif": largest_peak(records, |record| {
            record["format"] == "Avif" && !hostile(record)
        }),
        "largest_accepted": largest_peak(records, |record| !hostile(record)),
        "hostile_alpha": hostile_alpha,
    })
}

/// Measures every image and writes the JSON record. Release: `cargo test
/// --locked --release -p gitturtle --test image_decode_bench -- --ignored
/// --nocapture` with `GITTURTLE_IMAGE_BENCH_DIR` and
/// `GITTURTLE_IMAGE_BENCH_OUT` set.
#[test]
#[ignore = "a release measurement; set GITTURTLE_IMAGE_BENCH_OUT and GITTURTLE_IMAGE_BENCH_DIR"]
fn measure_images_through_the_helper() {
    let out = PathBuf::from(
        std::env::var_os("GITTURTLE_IMAGE_BENCH_OUT").expect("GITTURTLE_IMAGE_BENCH_OUT"),
    );
    let directory = PathBuf::from(
        std::env::var_os("GITTURTLE_IMAGE_BENCH_DIR").expect("GITTURTLE_IMAGE_BENCH_DIR"),
    );
    let runs: usize = std::env::var("GITTURTLE_IMAGE_BENCH_RUNS")
        .ok()
        .and_then(|runs| runs.parse().ok())
        .unwrap_or(20)
        .max(2);
    let mut paths: Vec<PathBuf> = fs::read_dir(&directory)
        .expect("GITTURTLE_IMAGE_BENCH_DIR is readable")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| listed(path))
        .collect();
    paths.sort();
    // The app's two helper slots; the measurement makes one request at a time.
    let slots = Slots::new(2);
    let starts: Vec<Value> = (0..runs).map(|_| start_and_kill()).collect();
    let mut skipped = Vec::new();
    let mut records = Vec::new();
    for path in paths {
        // Its name in the directory, never the path to it.
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let bytes = match read_input(&path) {
            Ok(bytes) => bytes,
            Err(reason) => {
                skipped.push(json!({ "name": name, "skipped": reason }));
                continue;
            }
        };
        eprintln!("measuring {name}");
        let format = gitturtle_preview::confined_image_format(&bytes);
        let mut record = match format {
            Some(format) => through_helper(&bytes, &name, format, runs, &slots),
            None => in_process(&bytes, &name, runs, &slots),
        };
        record["name"] = json!(name);
        record["bytes"] = json!(bytes.len());
        record["sha256"] = json!(sha256(&bytes));
        record["format"] = json!(format.map(|format| format!("{format:?}")));
        records.push(record);
    }
    let record = json!({
        "measured": "Linux AVIF and JPEG 2000 images through the confined helper, one helper per request; PNG and JPEG in process",
        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "executable": executable(),
        "host": host(),
        "edge": EDGE,
        "runs_per_image": runs,
        "skipped_inputs": skipped,
        "start": {
            "first": starts[0],
            "warm": summary(&starts[1..], &["start_us", "vm_size_kib", "vm_rss_kib", "kill_to_reap_idle_us"]),
        },
        "helper_peaks": peak_lines(&records),
        "slowest_markdown_image": slowest_markdown_image(&records),
        "images": records,
    });
    fs::write(&out, serde_json::to_vec_pretty(&record).expect("JSON"))
        .expect("the record is written");
    eprintln!("wrote {}", out.display());
}

/// The executable measured: its file name, size, the commit its build
/// embeds, with whether its bytes hold that commit, and whether the source
/// tree it was built from was clean, never its path.
fn executable() -> Value {
    executable_at(Path::new(BINARY), MAX_EXECUTABLE_BYTES)
}

/// The record of the executable at `path`, of which the first `limit` bytes
/// alone are read for the commit: whether they hold it is `null` where it
/// is not among them and the file runs past them.
fn executable_at(path: &Path, limit: usize) -> Value {
    let bytes = fs::File::open(path)
        .and_then(|file| read_bounded(file, limit))
        .unwrap_or_default();
    let found = !REVISION.is_empty()
        && bytes
            .windows(REVISION.len())
            .any(|window| window == REVISION.as_bytes());
    json!({
        "name": path.file_name().map(|name| name.to_string_lossy().into_owned()),
        "bytes": fs::metadata(path).map(|metadata| metadata.len()).ok(),
        "revision": REVISION,
        "revision_embedded": (found || bytes.len() <= limit).then_some(found),
        "source_tree": TREE,
    })
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The bytes this thread has read through system calls, from any source
/// (`rchar` in its `/proc` I/O counts): other tests' threads read apart.
fn read_by_this_thread() -> u64 {
    let counts = fs::read_to_string("/proc/thread-self/io").expect("this thread's I/O counts");
    status_value(&counts, "rchar:").expect("a read count")
}

/// What reading `path` as an input returns, and how many bytes this thread
/// read meanwhile, the second count's own few hundred among them.
fn read_counted(path: &Path) -> (Result<usize, &'static str>, u64) {
    let before = read_by_this_thread();
    let result = read_input(path).map(|bytes| bytes.len());
    (result, read_by_this_thread() - before)
}

/// Fewer bytes than any refused input here holds: only the read counts' own.
const UNREAD: u64 = 4096;

/// An input is read only from a regular file in the directory. A symbolic
/// link to a 1 MiB image in the directory itself is refused without a byte
/// of the image read; so are a directory and a FIFO named like images, and
/// the FIFO, which no one writes, without waiting on it.
#[test]
fn only_regular_input_files_are_read() {
    let directory = tempfile::tempdir().expect("a directory");
    let regular = directory.path().join("regular.avif");
    fs::write(&regular, vec![7_u8; 1 << 20]).expect("an input");
    let linked = directory.path().join("linked.avif");
    std::os::unix::fs::symlink(&regular, &linked).expect("a link");
    let folder = directory.path().join("folder.jp2");
    fs::create_dir(&folder).expect("a folder");
    let fifo = directory.path().join("fifo.png");
    let name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).expect("a C path");
    // SAFETY: a NUL-terminated path this test owns.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0, "a FIFO");

    let (read, count) = read_counted(&regular);
    assert_eq!(read, Ok(1 << 20));
    assert!(count >= 1 << 20, "read {count} bytes of the image");
    for refused in [&linked, &folder, &fifo] {
        let (read, count) = read_counted(refused);
        assert_eq!(read, Err("not a regular file"), "{}", refused.display());
        assert!(
            count < UNREAD,
            "read {count} bytes for {}",
            refused.display()
        );
    }
    assert!(listed(&linked) && listed(&directory.path().join("photo.JPG")));
    assert!(!listed(&directory.path().join("photo.jpeg.txt")));
}

/// An input past the 32 MiB input limit is refused from its size without a
/// byte of it read, and a helper's reply is read no further than one byte
/// past the largest the client accepts, however much a source holds.
#[test]
fn inputs_and_replies_are_read_within_their_limits() {
    let directory = tempfile::tempdir().expect("a directory");
    let large = directory.path().join("large.avif");
    let file = fs::File::create(&large).expect("an input");
    // Twice the limit, so a read of any of it shows in the count.
    let length = 2 * MAX_INPUT_BYTES;
    file.set_len(u64::try_from(length).unwrap())
        .expect("a sparse input");
    let (refused, count) = read_counted(&large);
    assert_eq!(refused, Err("larger than the 32 MiB input limit"));
    assert!(
        count < UNREAD,
        "read {count} bytes of a {length}-byte input"
    );
    let cap = max_image_reply_bytes(EDGE);
    assert_eq!(
        read_bounded(std::io::repeat(0), cap).expect("a read").len(),
        cap + 1
    );
    let (timed, first, _) = read_timed(std::io::repeat(0), cap);
    assert_eq!(timed.len(), cap + 1);
    assert!(first.is_some());
}

/// The executable is read for its commit no further than the limit: one past
/// it is not looked for, so whether it is embedded is left open, and one
/// within it is found. The record gives the file's whole size and the
/// state of the source tree the build saw.
#[test]
fn the_executable_is_read_within_its_limit() {
    let directory = tempfile::tempdir().expect("a directory");
    let path = directory.path().join("gitturtle");
    let mut bytes = vec![0; 4096];
    bytes.extend_from_slice(REVISION.as_bytes());
    fs::write(&path, &bytes).expect("an executable");
    let past = executable_at(&path, 4096);
    assert_eq!(past["revision_embedded"], Value::Null, "{past}");
    assert_eq!(past["bytes"], json!(bytes.len()), "{past}");
    let within = executable_at(&path, bytes.len());
    assert_eq!(within["revision_embedded"], json!(true), "{within}");
    assert_eq!(within["name"], json!("gitturtle"), "{within}");
    assert_eq!(within["source_tree"], json!(TREE), "{within}");
    assert!(["clean", "modified", "unknown"].contains(&TREE), "{TREE}");
}

/// The peak recorded is the helper's own: while this process holds 256 MiB,
/// the helper decoding a 1,024 × 768 JPEG 2000 fixture peaks at a fraction
/// of it, read while it waits in its reply's write.
#[test]
fn the_helpers_peak_is_its_own() {
    // Written, so every page of it is resident.
    let ballast = std::hint::black_box(vec![1_u8; 256 << 20]);
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../preview/tests/fixtures/jpeg2000-large.jp2");
    let bytes = fs::read(fixture).expect("the fixture");
    let format = gitturtle_preview::confined_image_format(&bytes).expect("a JP2");
    let peak = peak_kib(&bytes, format).expect("the helper's peak, read in its reply's write");
    assert!(
        (1..128 << 10).contains(&peak),
        "the helper peaked at {peak} KiB"
    );
    drop(ballast);
}

/// Waits until `child` has exited, leaving it unreaped (`WNOWAIT`), so its
/// `/proc` status is still there to read.
fn exited_unreaped(child: &Child) {
    // SAFETY: an all-zero `siginfo_t` is valid, and `waitid` writes only it.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    loop {
        // SAFETY: waits on a child of this process, writing only `info`.
        let waited = unsafe {
            libc::waitid(
                libc::P_PID,
                child.id(),
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        if waited == 0 {
            return;
        }
        let error = std::io::Error::last_os_error();
        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted, "{error}");
    }
}

/// A kill at work is timed only for a helper seen confined: one that has
/// already ended, as a refused image's helper can before it is seen, records
/// none (`null` in the JSON), and one waiting for its request records one.
#[test]
fn a_kill_at_work_is_timed_only_for_a_confined_helper() {
    let inherited = own_filters();
    let mut ended = spawn();
    // Its request's pipe closes before a byte, so it ends at the header.
    drop(ended.stdin.take());
    let output = ended.stdout.take().expect("stdout");
    exited_unreaped(&ended);
    assert_eq!(timed_kill(ended, inherited, output), None);
    // It waits for its request, so it never ends first.
    let mut waiting = spawn();
    let pipes = (waiting.stdin.take(), waiting.stdout.take());
    assert!(timed_kill(waiting, inherited, pipes).is_some());
}

/// The helper's whole reply to `bytes` at History's edge.
fn reply_to(bytes: &[u8], format: ConfinedFormat) -> Vec<u8> {
    let header = encode_image_request(format, EDGE, bytes).expect("a request");
    let mut child = spawn();
    let mut input = child.stdin.take().expect("stdin");
    let output = child.stdout.take().expect("stdout");
    let reader = std::thread::spawn(move || read_bounded(output, max_image_reply_bytes(EDGE)));
    let _ = input
        .write_all(&header)
        .and_then(|()| input.write_all(bytes));
    drop(input);
    let reply = reader.join().expect("the reader").expect("the reply");
    let _ = reap(child);
    reply
}

/// A peak counts only for an image the client accepts: it is kept for the
/// helper's reply to the 1,024 × 768 JPEG 2000 fixture, and not for its
/// reply to the same file branded JPX without JP2 compatibility, which the
/// helper refuses, nor for a reply cut short.
#[test]
fn peaks_belong_to_accepted_images() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../preview/tests/fixtures/jpeg2000-large.jp2");
    let bytes = fs::read(fixture).expect("the fixture");
    let format = ConfinedFormat::Jp2;
    let accepted = reply_to(&bytes, format);
    assert_eq!(accepted_peak(Some(1), &accepted, format), Some(1));
    assert_eq!(
        accepted_peak(Some(1), &accepted[..accepted.len() - 1], format),
        None
    );
    let mut jpx = bytes;
    // `ftyp`'s brand and its one compatible brand.
    assert_eq!(&jpx[16..20], b"ftyp");
    jpx[20..24].copy_from_slice(b"jpx ");
    jpx[28..32].copy_from_slice(b"jpx ");
    let refused = reply_to(&jpx, format);
    assert!(
        matches!(
            read_image_reply(refused.as_slice(), format, EDGE),
            Ok(Err(_))
        ),
        "the helper refuses the JPX file"
    );
    assert_eq!(accepted_peak(Some(1), &refused, format), None);
    assert_eq!(peak_kib(&jpx, format), None);
}

/// Requirement 18's line measures its designated input alone, against half
/// of the helper's data limit, where every run accepted it, even where
/// another AVIF or a JPEG 2000 image peaked higher; the largest accepted
/// AVIF and the largest accepted image of any format are reported apart. An
/// image whose peak was never read, as a refused one's is not, has none.
#[test]
fn requirement_18_measures_its_designated_input() {
    let peak = |max: u64, n: usize| json!({ "vm_hwm_kib": { "max": max, "n": n } });
    let designated = json!({ "name": REQUIREMENT_18_INPUT, "format": "Avif", "errors": [], "helper_peak": peak(500_000, PEAK_RUNS) });
    let records = [
        json!({ "name": "small.avif", "format": "Avif", "errors": [], "helper_peak": peak(40_000, PEAK_RUNS) }),
        json!({ "name": "large.avif", "format": "Avif", "errors": [], "helper_peak": peak(600_000, PEAK_RUNS) }),
        json!({ "name": "cmyk.jp2", "format": "Jp2", "errors": [], "helper_peak": peak(700_000, PEAK_RUNS) }),
        json!({ "name": "refused.avif", "format": "Avif", "errors": ["Decoded image exceeds the 256 MiB limit"], "helper_peak": {} }),
        json!({ "name": "photo.png", "format": null }),
        designated.clone(),
    ];
    let lines = peak_lines(&records);
    let line = &lines["requirement_18"];
    assert_eq!(line["name"], json!(REQUIREMENT_18_INPUT), "{lines}");
    assert_eq!(line["designated_measured"], json!(true), "{lines}");
    assert_eq!(line["reason"], Value::Null, "{lines}");
    assert_eq!(line["vm_hwm_kib"], json!(500_000), "{lines}");
    assert_eq!(line["within_budget"], json!(true), "{lines}");
    assert_eq!(line["budget_kib"], json!(524_288), "{lines}");
    let avif = &lines["largest_accepted_avif"];
    assert_eq!(avif["name"], json!("large.avif"), "{lines}");
    assert_eq!(avif["within_budget"], json!(false), "{lines}");
    let any = &lines["largest_accepted"];
    assert_eq!(any["name"], json!("cmyk.jp2"), "{lines}");
    assert_eq!(any["format"], json!("Jp2"), "{lines}");
    let none = peak_lines(&records[3..5]);
    assert_eq!(none["largest_accepted_avif"]["name"], Value::Null);
    assert_eq!(none["largest_accepted"]["vm_hwm_kib"], Value::Null);

    // Not accepted on every run: no peak, no verdict and the reason, while
    // a smaller accepted AVIF within the budget keeps its own line.
    let mut refused = designated.clone();
    refused["errors"] = json!(["Decoded image exceeds the 256 MiB limit"]);
    refused["helper_peak"] = json!({});
    let mut slow = designated.clone();
    slow["errors"] = json!([Failure::TimedOut.to_string()]);
    let mut partly = designated.clone();
    partly["helper_peak"] = peak(500_000, PEAK_RUNS - 1);
    let mut misread = designated;
    misread["format"] = json!("Jp2");
    for (designated, reason) in [
        (
            None,
            "absent: not among the measured inputs (see skipped_inputs)",
        ),
        (Some(refused), "refused"),
        (Some(slow), "timed out"),
        (Some(partly), "its peak was not read in every peak request"),
        (Some(misread), "not read as an AVIF"),
    ] {
        let fallback = reason == "refused";
        let records: Vec<Value> = [records[0].clone()].into_iter().chain(designated).collect();
        let lines = peak_lines(&records);
        let line = &lines["requirement_18"];
        assert_eq!(line["name"], json!(REQUIREMENT_18_INPUT), "{lines}");
        assert_eq!(line["designated_measured"], json!(false), "{lines}");
        assert_eq!(line["reason"], json!(reason), "{lines}");
        assert_eq!(line["vm_hwm_kib"], Value::Null, "{lines}");
        assert_eq!(line["within_budget"], Value::Null, "{lines}");
        if fallback {
            let avif = &lines["largest_accepted_avif"];
            assert_eq!(avif["name"], json!("small.avif"), "{lines}");
            assert_eq!(avif["within_budget"], json!(true), "{lines}");
        }
    }
}

/// A hostile-alpha input is reported on its own line against the same
/// budget, never as the largest accepted AVIF nor as the largest accepted
/// image, however high it peaked, and never as requirement 18's; one not
/// accepted keeps its errors there and no peak.
#[test]
fn the_hostile_alpha_peak_has_its_own_line() {
    let records = [
        json!({ "name": "large.avif", "format": "Avif", "helper_peak": { "vm_hwm_kib": { "max": 400_000 } } }),
        json!({ "name": "avif-hostile-alpha.avif", "format": "Avif", "errors": [], "helper_peak": { "vm_hwm_kib": { "max": 640_000 } } }),
        json!({ "name": "avif-hostile-alpha-refused.avif", "format": "Avif", "errors": ["The image decoder stopped."], "helper_peak": {} }),
    ];
    let lines = peak_lines(&records);
    assert_eq!(
        lines["requirement_18"]["designated_measured"],
        json!(false),
        "{lines}"
    );
    assert_eq!(
        lines["largest_accepted_avif"]["name"],
        json!("large.avif"),
        "{lines}"
    );
    assert_eq!(
        lines["largest_accepted"]["name"],
        json!("large.avif"),
        "{lines}"
    );
    let hostile = &lines["hostile_alpha"];
    assert_eq!(
        hostile[0]["name"],
        json!("avif-hostile-alpha.avif"),
        "{lines}"
    );
    assert_eq!(hostile[0]["vm_hwm_kib"], json!(640_000), "{lines}");
    assert_eq!(hostile[0]["within_budget"], json!(false), "{lines}");
    assert_eq!(hostile[1]["vm_hwm_kib"], Value::Null, "{lines}");
    assert_eq!(
        hostile[1]["errors"],
        json!(["The image decoder stopped."]),
        "{lines}"
    );
}

/// T is the slowest confined image within Markdown's share that every run
/// at its edge decoded, by its slowest request there, first or warm, and 32
/// times that: an image refused or timed out at that edge, however slow, and
/// one too large to be measured there are not counted.
#[test]
fn the_slowest_markdown_image_decoded_every_time() {
    let markdown = |first: u64, max: u64, errors: Value| json!({ "app_first": { "request_us": first }, "app_warm": { "request_us": { "max": max } }, "errors": errors });
    let records = [
        json!({ "name": "fast.avif", "bytes": 1000, "markdown": markdown(9_000, 8_000, json!([])) }),
        json!({ "name": "slow.jp2", "bytes": 2000, "markdown": markdown(40_000, 300_000, json!([])) }),
        json!({ "name": "first.avif", "bytes": 3000, "markdown": markdown(200_000, 100_000, json!([])) }),
        json!({ "name": "timed-out.avif", "bytes": 4000, "markdown": markdown(10_000_000, 10_000_000, json!(["The image took too long to decode."])) }),
        json!({ "name": "large.avif", "bytes": 40_000_000, "markdown": null }),
    ];
    let slowest = slowest_markdown_image(&records);
    assert_eq!(slowest["name"], json!("slow.jp2"), "{slowest}");
    assert_eq!(slowest["slowest_request_us"], json!(300_000), "{slowest}");
    // Under a deadline in all, the document total does not bound the side.
    assert_eq!(
        slowest["per_side_unbounded_us"],
        json!(9_600_000),
        "{slowest}"
    );
    assert_eq!(
        slowest["per_side_bounded_us"],
        json!(9_600_000),
        "{slowest}"
    );
    assert_eq!(slowest["share_bytes"], json!(524_288), "{slowest}");
    let first = slowest_markdown_image(&records[2..]);
    assert_eq!(first["name"], json!("first.avif"), "{first}");
    let none = slowest_markdown_image(&records[3..]);
    assert_eq!(none["name"], Value::Null, "{none}");
    assert_eq!(none["per_side_bounded_us"], Value::Null, "{none}");
}

/// A side whose 32 images would run past one deadline in all is bounded by
/// the document total: its helpers stop being sent images once they have
/// run one 10 s deadline, so the side's figure is at most that deadline
/// plus the last image's T, never 32 × T, which the record keeps apart.
#[test]
fn the_markdown_side_is_bounded_by_the_document_total() {
    let markdown = json!({ "app_first": { "request_us": 2_000_000 }, "app_warm": { "request_us": { "max": 1_500_000 } }, "errors": [] });
    let slowest = slowest_markdown_image(&[
        json!({ "name": "slow.jp2", "bytes": 2000, "markdown": markdown }),
    ]);
    assert_eq!(slowest["slowest_request_us"], json!(2_000_000), "{slowest}");
    assert_eq!(
        slowest["document_deadline_us"],
        json!(10_000_000),
        "{slowest}"
    );
    assert_eq!(
        slowest["per_side_unbounded_us"],
        json!(64_000_000),
        "{slowest}"
    );
    assert_eq!(
        slowest["per_side_bounded_us"],
        json!(12_000_000),
        "{slowest}"
    );
}
