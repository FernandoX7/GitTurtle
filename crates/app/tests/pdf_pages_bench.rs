//! The release measurement of Linux PDF pages through the real helper
//! (section 5 of `docs/development/2026-11-02-linux-pdf-pages-architecture.md`).
//!
//! Opt-in and ignored by default. Every regular `*.pdf` file in the directory
//! named by `GITTURTLE_PDF_BENCH_DIR` is measured, never one reached through a
//! symbolic link, and one past the 32 MiB input limit is refused unread
//! beyond it, with two documents the test
//! generates itself: a 300-page dense-text document and a 32 MiB image-heavy
//! file. Each fixture's first, middle and last page is requested
//! `GITTURTLE_PDF_BENCH_RUNS` times (default 20) from a new helper, as the app
//! does, both through an instrumented exchange that times every phase and
//! through the app's own client for the whole request. The helper's own peak
//! resident memory, its `VmHWM`, is read from `/proc` in untimed requests of
//! their own, while the helper waits in its reply's write (`wait4`'s
//! `ru_maxrss` is no measure: the helper carries this process's high-water
//! mark across its `exec`). The helper's start, its `VmSize` once it is
//! confined, and the time from a kill to the reap are measured apart. The
//! JSON goes to the file named by `GITTURTLE_PDF_BENCH_OUT`. It names each
//! fixture by its file name in that directory and the executable by its file
//! name, the commit built into it (looked for in its first 256 MiB) and
//! whether the source tree it was built from was clean, never by an absolute
//! path, and no helper's reply is read past the largest the client accepts.
//!
//! The app crate has no library target, so this test compiles
//! `src/pdf_helper.rs` as its own module, as `tests/pdf_helper.rs` does.
#![cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

#[allow(
    dead_code,
    reason = "the measurement uses the client and its constants"
)]
#[path = "../src/pdf_helper.rs"]
mod pdf_helper;

use std::fs;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use gitturtle_preview::MAX_INPUT_BYTES;
use gitturtle_preview::pdf_helper::{
    MAX_REPLY_BYTES, PROTOCOL_VERSION, encode_request, read_reply,
};
use pdf_helper::{Client, DEADLINE, HELPER_FLAG, HelperCommand, Slots};
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

/// Bytes to the largest any reader here takes: `limit`, and one more, which
/// tells a source past it from one that ends there.
fn read_bounded(source: impl Read, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    source
        .take(u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// A fixture from the directory: only a regular file, never one a symbolic
/// link names, and only within the input limit, read no further than one
/// byte past it.
fn read_fixture(path: &Path) -> Result<Vec<u8>, &'static str> {
    let regular = fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file());
    if !regular {
        return Err("not a regular file");
    }
    let file = fs::File::open(path).map_err(|_| "unreadable")?;
    let bytes = read_bounded(file, MAX_INPUT_BYTES).map_err(|_| "unreadable")?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err("larger than the 32 MiB input limit");
    }
    Ok(bytes)
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

/// One request through an instrumented exchange: blocking pipes, each phase
/// timed from this side, and the helper's own phase timings.
fn instrumented(pdf: &[u8], page: usize) -> Value {
    let header = encode_request(page, pdf).expect("a request");
    let started = Instant::now();
    let mut child = spawn();
    let spawned = started.elapsed();
    let mut input = child.stdin.take().expect("stdin");
    let mut output = child.stdout.take().expect("stdout");
    // Reading on a thread of its own keeps a reply larger than the pipe from
    // blocking the write.
    // No further than one byte past the largest reply the client accepts.
    let reader = std::thread::spawn(move || {
        let reply = read_bounded(&mut output, MAX_REPLY_BYTES).unwrap_or_default();
        (reply, Instant::now())
    });
    let written = input
        .write_all(&header)
        .and_then(|()| input.write_all(pdf))
        .map(|()| started.elapsed());
    drop(input);
    let (reply, read) = reader.join().expect("the reader");
    let exited = reap(child);
    let request = started.elapsed();
    let read = read - started;
    // Each phase on its own: the spawn; writing the request, which a request
    // larger than the pipe makes wait for the helper to start and read it;
    // the reply's last byte after the request's; and reaping after the reply.
    let mut run = json!({
        "spawn_us": micros(spawned),
        "write_us": written.as_ref().ok().map(|written| micros(written.saturating_sub(spawned))),
        "reply_us": written.as_ref().ok().map(|written| micros(read.saturating_sub(*written))),
        "reap_us": micros(request.saturating_sub(read)),
        "request_us": micros(request),
        "reply_bytes": reply.len(),
        "exited_0": exited,
    });
    match read_reply(reply.as_slice()) {
        Ok(Ok(rendered)) => {
            let phases = rendered.timings;
            let helper = u64::from(phases.parse_us)
                + u64::from(phases.render_us)
                + u64::from(phases.text_us);
            run["parse_us"] = json!(phases.parse_us);
            run["render_us"] = json!(phases.render_us);
            run["text_us"] = json!(phases.text_us);
            // Start, transfer and exit around the helper's own work.
            run["overhead_us"] = json!(micros(request).saturating_sub(helper));
            run["start_transfer_parse_us"] = json!(
                micros(request)
                    .saturating_sub(u64::from(phases.render_us) + u64::from(phases.text_us))
            );
            run["pixels"] = json!([rendered.image.width, rendered.image.height]);
            run["page_count"] = json!(rendered.page_count);
            run["text"] = json!(match &rendered.text {
                Ok(text) if text.text.is_some() => "present",
                Ok(_) => "absent",
                Err(_) => "failed",
            });
        }
        // The client's wording only: a reply's bytes never reach the record.
        Ok(Err(kind)) => {
            run["error"] = json!(pdf_helper::Failure::Document { kind, page }.to_string());
        }
        Err(_) => run["error"] = json!("unreadable"),
    }
    run
}

/// One whole request through the app's client, as the page lane makes it.
fn through_client(pdf: &[u8], page: usize, slots: &Slots) -> Value {
    let client = Client::with(HelperCommand::new(BINARY, arguments()), DEADLINE, slots);
    let started = Instant::now();
    let result = client.render(pdf, page, &|| Ok(()));
    json!({
        "request_us": micros(started.elapsed()),
        // `Display` only: the wording a person would read.
        "error": result.err().map(|failure| failure.to_string()),
    })
}

/// The number after `key` in a `/proc/<pid>/status` text.
fn status_value(status: &str, key: &str) -> Option<u64> {
    status
        .lines()
        .find_map(|line| line.strip_prefix(key))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse().ok())
}

/// A new helper's start: from the spawn until it is confined (named, with
/// more seccomp filters than this process) and waiting for its request, its
/// `VmSize` and `VmRSS` then, and the time from a kill to the reap.
fn start_and_kill() -> Value {
    let own = fs::read_to_string("/proc/self/status").expect("own status");
    let own_filters = status_value(&own, "Seccomp_filters:").unwrap_or(0);
    let started = Instant::now();
    let mut child = spawn();
    let path = format!("/proc/{}/status", child.id());
    let status = loop {
        let status = fs::read_to_string(&path).unwrap_or_default();
        if status.contains("Name:\tgitturtle-pdf")
            && status_value(&status, "Seccomp_filters:").unwrap_or(0) > own_filters
        {
            break status;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the helper confines itself"
        );
        // Polled every 100 µs, so the probe takes no core from the helper
        // it times on a busy host; the start reads up to 100 µs late.
        std::thread::sleep(Duration::from_micros(100));
    };
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

/// The time from a kill to the reap of a helper at work on `page`, killed
/// once its whole request is written.
fn kill_at_work(pdf: &[u8], page: usize) -> u64 {
    let header = encode_request(page, pdf).expect("a request");
    let mut child = spawn();
    let mut input = child.stdin.take().expect("stdin");
    let output = child.stdout.take().expect("stdout");
    let _ = input.write_all(&header).and_then(|()| input.write_all(pdf));
    let killed = Instant::now();
    child.kill().expect("the kill");
    let _ = reap(child);
    drop((input, output));
    micros(killed.elapsed())
}

/// Requests per page whose helper's peak is read.
const PEAK_RUNS: usize = 5;
/// How long a helper may take to begin its reply: past its own 12-second
/// alarm, which ends it.
const PEAK_WAIT: Duration = Duration::from_secs(20);

/// The helper's own peak resident set in KiB, its `VmHWM`, for one request.
/// Its reply's pipe holds a single page and nothing reads it until the peak
/// is read, so a helper whose reply has begun, its work and every allocation
/// for it done, waits in its write for as long as the reply is larger than
/// that page: a page's pixels always are. `None` where the helper ends
/// first, as a failure's short reply lets it.
fn peak_kib(pdf: &[u8], page: usize) -> Option<u64> {
    let header = encode_request(page, pdf).expect("a request");
    let mut child = spawn();
    let mut input = child.stdin.take().expect("stdin");
    let mut output = child.stdout.take().expect("stdout");
    let reply = output.as_raw_fd();
    // SAFETY: resizes a pipe this process holds, still empty, to the least
    // the kernel allows, one page.
    let resized = unsafe { libc::fcntl(reply, libc::F_SETPIPE_SZ, 1) };
    assert!(resized > 0, "the reply pipe holds one page");
    // The helper reads the whole request before it replies.
    let _ = input.write_all(&header).and_then(|()| input.write_all(pdf));
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
    let _ = read_bounded(&mut output, MAX_REPLY_BYTES);
    let _ = reap(child);
    peak
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
    "reply_us",
    "reap_us",
    "parse_us",
    "render_us",
    "text_us",
    "overhead_us",
    "start_transfer_parse_us",
    "request_us",
];

/// A PDF of `objects`, numbered from 1 in order; object 1 is the catalog.
fn assemble(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

fn stream(dictionary: &str, data: &[u8]) -> Vec<u8> {
    let mut object = format!("<< {dictionary} /Length {} >>\nstream\n", data.len()).into_bytes();
    object.extend_from_slice(data);
    object.extend_from_slice(b"\nendstream");
    object
}

/// `pages` A4 pages, each drawn by its content stream, with Helvetica as
/// `/F1` and, where given, page `i`'s image as `/Im0`.
fn document(contents: Vec<Vec<u8>>, images: Vec<Option<Vec<u8>>>) -> Vec<u8> {
    let count = contents.len();
    // 1 catalog, 2 pages, 3 font; then per page: page, content, image.
    let first = |page: usize| 4 + page * 3;
    let kids: Vec<String> = (0..count)
        .map(|page| format!("{} 0 R", first(page)))
        .collect();
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!(
            "<< /Type /Pages /Count {count} /Kids [{}] >>",
            kids.join(" ")
        )
        .into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    ];
    for (page, (content, image)) in contents.into_iter().zip(images).enumerate() {
        let object = first(page);
        let xobject = if image.is_some() {
            format!(" /XObject << /Im0 {} 0 R >>", object + 2)
        } else {
            String::new()
        };
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 3 0 R >>{xobject} >> /Contents {} 0 R >>",
                object + 1
            )
            .into_bytes(),
        );
        objects.push(stream("", &content));
        // A placeholder keeps the numbering where a page has no image.
        objects.push(image.unwrap_or_else(|| b"null".to_vec()));
    }
    assemble(&objects)
}

/// 300 pages of dense text: 70 lines of 100 characters each, in 8 pt.
fn dense_text() -> Vec<u8> {
    let words = "repository history inspection compares revisions of every tracked file ";
    let contents = (0..300)
        .map(|page| {
            let mut content = b"BT /F1 8 Tf 10 TL 30 810 Td ".to_vec();
            for line in 0..70 {
                let text: String = words
                    .chars()
                    .cycle()
                    .skip((page * 7 + line * 3) % 50)
                    .take(100)
                    .collect();
                content.extend_from_slice(format!("({text}) Tj T* ").as_bytes());
            }
            content.extend_from_slice(b"ET");
            content
        })
        .collect::<Vec<_>>();
    document(contents, vec![None; 300])
}

/// About 32 MiB of eight full-page uncompressed RGB images, seven of
/// 1,200 × 1,200 and one of 1,000 × 1,000, each with a caption.
fn image_heavy() -> Vec<u8> {
    let mut contents = Vec::new();
    let mut images = Vec::new();
    for page in 0..8u32 {
        let edge: u32 = if page == 7 { 1000 } else { 1200 };
        let mut pixels = Vec::with_capacity((edge * edge * 3) as usize);
        let mut seed = 0x9E37_79B9_u32 ^ page;
        for y in 0..edge {
            for x in 0..edge {
                // A gradient with a little noise, so no two rows repeat.
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let noise = (seed & 0x1f) as u8;
                pixels.extend_from_slice(&[
                    (x * 255 / edge) as u8 ^ noise,
                    (y * 255 / edge) as u8,
                    ((x + y + page * 40) % 256) as u8,
                ]);
            }
        }
        images.push(Some(stream(
            &format!("/Type /XObject /Subtype /Image /Width {edge} /Height {edge} /ColorSpace /DeviceRGB /BitsPerComponent 8"),
            &pixels,
        )));
        contents.push(
            format!(
                "q 595 0 0 760 0 82 cm /Im0 Do Q BT /F1 12 Tf 30 40 Td (Image page {}) Tj ET",
                page + 1
            )
            .into_bytes(),
        );
    }
    let pdf = document(contents, images);
    assert!(
        pdf.len() <= gitturtle_preview::MAX_INPUT_BYTES && pdf.len() > 31 * 1024 * 1024,
        "the image-heavy fixture is just under the 32 MiB input limit: {}",
        pdf.len()
    );
    pdf
}

/// The first, middle and last zero-based pages of a `count`-page document.
fn pages_of(count: usize) -> Vec<usize> {
    let mut pages = vec![0, count / 2, count.saturating_sub(1)];
    pages.dedup();
    pages
}

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

/// Measures every fixture and writes the JSON record. Release: `cargo test
/// --locked --release -p gitturtle --test pdf_pages_bench -- --ignored
/// --nocapture` with the three variables set.
#[test]
#[ignore = "a release measurement; set GITTURTLE_PDF_BENCH_OUT and GITTURTLE_PDF_BENCH_DIR"]
fn measure_pdf_pages_through_the_helper() {
    let out = PathBuf::from(
        std::env::var_os("GITTURTLE_PDF_BENCH_OUT").expect("GITTURTLE_PDF_BENCH_OUT"),
    );
    let directory = std::env::var_os("GITTURTLE_PDF_BENCH_DIR").map(PathBuf::from);
    let runs: usize = std::env::var("GITTURTLE_PDF_BENCH_RUNS")
        .ok()
        .and_then(|runs| runs.parse().ok())
        .unwrap_or(20)
        .max(2);
    let mut skipped = Vec::new();
    let mut fixtures: Vec<(String, &str, Vec<u8>)> = vec![
        (
            "generated-dense-text-300.pdf".into(),
            "generated",
            dense_text(),
        ),
        (
            "generated-image-heavy-32mib.pdf".into(),
            "generated",
            image_heavy(),
        ),
    ];
    if let Some(directory) = &directory {
        let mut listed: Vec<PathBuf> = fs::read_dir(directory)
            .expect("GITTURTLE_PDF_BENCH_DIR is readable")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|extension| extension == "pdf"))
            .collect();
        listed.sort();
        for path in listed {
            // Its name in the directory, never the path to it.
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            match read_fixture(&path) {
                Ok(pdf) => fixtures.push((name, "directory", pdf)),
                Err(reason) => skipped.push(json!({ "name": name, "skipped": reason })),
            }
        }
    }
    let slots = Slots::new(2);
    let starts: Vec<Value> = (0..runs).map(|_| start_and_kill()).collect();
    let mut records = Vec::new();
    for (name, source, pdf) in &fixtures {
        let count = match read_count(pdf) {
            Some(count) => count,
            None => {
                records.push(json!({ "name": name, "source": source, "bytes": pdf.len(), "sha256": sha256(pdf), "error": "the first page did not render" }));
                continue;
            }
        };
        let mut pages = Vec::new();
        for page in pages_of(count) {
            let measured: Vec<Value> = (0..runs).map(|_| instrumented(pdf, page)).collect();
            let client: Vec<Value> = (0..runs)
                .map(|_| through_client(pdf, page, &slots))
                .collect();
            let peaks: Vec<Value> = (0..PEAK_RUNS)
                .map(|_| json!({ "vm_hwm_kib": peak_kib(pdf, page) }))
                .collect();
            pages.push(json!({
                "page": page,
                "first": measured[0],
                "warm": summary(&measured[1..], RUN_METRICS),
                "client_first_us": client[0]["request_us"],
                "client_warm": summary(&client[1..], &["request_us"]),
                "errors": measured.iter().filter_map(|run| run["error"].as_str()).chain(client.iter().filter_map(|run| run["error"].as_str())).collect::<Vec<_>>(),
                "kill_to_reap_at_work_us": kill_at_work(pdf, page),
                // `n` counts the requests whose peak was read.
                "helper_peak": summary(&peaks, &["vm_hwm_kib"]),
            }));
        }
        records.push(json!({
            "name": name,
            "source": source,
            "bytes": pdf.len(),
            "sha256": sha256(pdf),
            "page_count": count,
            "pages": pages,
        }));
    }
    let record = json!({
        "measured": "Linux PDF pages through the confined helper, one helper per request",
        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "executable": executable(),
        "host": host(),
        "runs_per_page": runs,
        "fixture_directory_given": directory.is_some(),
        "skipped_fixtures": skipped,
        "start": {
            "first": starts[0],
            "warm": summary(&starts[1..], &["start_us", "vm_size_kib", "vm_rss_kib", "kill_to_reap_idle_us"]),
        },
        "fixtures": records,
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

/// The document's page count from a request for its first page.
fn read_count(pdf: &[u8]) -> Option<usize> {
    let slots = Slots::new(1);
    let client = Client::with(HelperCommand::new(BINARY, arguments()), DEADLINE, &slots);
    client
        .render(pdf, 0, &|| Ok(()))
        .ok()
        .map(|reply| reply.page_count)
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A fixture is read only from a regular file in the directory: a symbolic
/// link to one, even one in the directory itself, and a directory named
/// like a PDF are skipped, so the measurement never reads past the directory
/// it was given.
#[test]
fn only_regular_fixture_files_are_read() {
    let directory = tempfile::tempdir().expect("a directory");
    let regular = directory.path().join("regular.pdf");
    fs::write(&regular, b"%PDF-1.7\n").expect("a fixture");
    let linked = directory.path().join("linked.pdf");
    std::os::unix::fs::symlink(&regular, &linked).expect("a link");
    let folder = directory.path().join("folder.pdf");
    fs::create_dir(&folder).expect("a folder");
    assert_eq!(read_fixture(&regular), Ok(b"%PDF-1.7\n".to_vec()));
    assert_eq!(read_fixture(&linked), Err("not a regular file"));
    assert_eq!(read_fixture(&folder), Err("not a regular file"));
}

/// The bytes this thread has read through system calls, from any source
/// (`rchar` in its `/proc` I/O counts): other tests' threads read apart.
fn read_by_this_thread() -> u64 {
    let counts = fs::read_to_string("/proc/thread-self/io").expect("this thread's I/O counts");
    status_value(&counts, "rchar:").expect("a read count")
}

/// A fixture past the 32 MiB input limit is refused, read no further than
/// one byte past it, and a helper's reply is read no further than one byte
/// past the largest the client accepts, however much a source holds.
#[test]
fn fixtures_and_replies_are_read_within_their_limits() {
    let directory = tempfile::tempdir().expect("a directory");
    let large = directory.path().join("large.pdf");
    let file = fs::File::create(&large).expect("a fixture");
    // Twice the limit, so what is read tells a bounded read from a whole
    // one: both refuse the fixture.
    let length = 2 * MAX_INPUT_BYTES;
    file.set_len(u64::try_from(length).unwrap())
        .expect("a sparse fixture");
    let before = read_by_this_thread();
    let refused = read_fixture(&large).map(|bytes| bytes.len());
    let read = read_by_this_thread() - before;
    assert_eq!(refused, Err("larger than the 32 MiB input limit"));
    // One byte past the limit, with the first count's own few hundred.
    let bound = u64::try_from(MAX_INPUT_BYTES).unwrap() + 1;
    assert!(
        (bound..bound + 4096).contains(&read),
        "read {read} bytes of a {length}-byte fixture"
    );
    let endless = std::io::repeat(0);
    assert_eq!(
        read_bounded(endless, MAX_REPLY_BYTES)
            .expect("a read")
            .len(),
        MAX_REPLY_BYTES + 1
    );
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
/// a one-page document's helper peaks at a fraction of it. `wait4`'s
/// `ru_maxrss`, recorded before, was at least this process's high-water
/// mark, which the helper carries across its `exec`.
#[test]
fn the_helpers_peak_is_its_own() {
    // Written, so every page of it is resident.
    let ballast = std::hint::black_box(vec![1_u8; 256 << 20]);
    let pdf = document(
        vec![b"BT /F1 12 Tf 30 800 Td (One page) Tj ET".to_vec()],
        vec![None],
    );
    let peak = peak_kib(&pdf, 0).expect("the helper's peak, read in its reply's write");
    assert!(
        (1..128 << 10).contains(&peak),
        "the helper peaked at {peak} KiB"
    );
    drop(ballast);
}
