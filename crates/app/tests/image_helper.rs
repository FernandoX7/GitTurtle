//! The Linux image client: the real binary decoding AVIF and JPEG 2000 under
//! its seccomp filter, and the client against shell fakes. Requirements 13,
//! 14 and 16 of the security review in
//! `docs/development/2026-11-02-linux-avif-jpeg2000-architecture.md`.
//!
//! The app crate has no library target, so this test compiles
//! `src/pdf_helper.rs` and `src/image_helper.rs` as its own modules.
#![cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

#[allow(dead_code, reason = "each test uses part of the module")]
#[path = "../src/pdf_helper.rs"]
mod pdf_helper;

#[allow(dead_code, reason = "each test uses part of the module")]
#[path = "../src/image_helper.rs"]
mod image_helper;

/// Checked-in codec fixtures from the preview crate, and the hostile inputs
/// its tests build, rebuilt here from the same issues.
mod fixtures {
    use std::path::PathBuf;

    /// Fixtures that must not decode; every other AVIF, JP2 and J2K must.
    pub(crate) const HOSTILE: [&str; 5] = [
        "avif-alpha-smaller-than-image.avif",
        "avif-frame-larger-than-ispe.avif",
        "avif-grid.avif",
        "avif-reserved-colour.avif",
        "avif-sequence-without-still.avif",
    ];

    fn directory() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../preview/tests/fixtures")
    }

    pub(crate) fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(directory().join(name)).unwrap_or_else(|error| panic!("{name}: {error}"))
    }

    /// Every AVIF, JP2 and J2K fixture but the hostile ones, by name.
    pub(crate) fn benign() -> Vec<(String, Vec<u8>)> {
        let mut found: Vec<(String, Vec<u8>)> = std::fs::read_dir(directory())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| {
                [".avif", ".jp2", ".j2k"]
                    .iter()
                    .any(|extension| name.ends_with(extension))
                    && !HOSTILE.contains(&name.as_str())
            })
            .map(|name| {
                let bytes = fixture(&name);
                (name, bytes)
            })
            .collect();
        found.sort();
        found
    }

    fn set(bytes: &mut [u8], offset: usize, value: &[u8]) {
        bytes[offset..offset + value.len()].copy_from_slice(value);
    }

    fn position(bytes: &[u8], kind: &[u8; 4]) -> usize {
        bytes.windows(4).position(|found| found == kind).unwrap()
    }

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
            .collect()
    }

    /// https://github.com/LaurenzV/hayro/issues/1549: a 97-byte codestream
    /// declaring 100000 × 100000 that asked hayro-jpeg2000 for about 95 GB.
    pub(crate) fn issue_1549() -> Vec<u8> {
        let bytes = hex(
            "ff4fff5100290000000186a0000186a00000000000000000000186a0000186a000000000000000000001070101ff52000c00000001000004040001ff5c00044040ff90000a0000000000000001ff9300000000000000000000000000000000ffd9",
        );
        assert_eq!(bytes.len(), 97);
        bytes
    }

    /// https://github.com/LaurenzV/hayro/pull/1558 (comment of 2026-10-22):
    /// #1549 at 16383 × 16383 with 1 × 1 tiles, 25.8 GB of tiles.
    pub(crate) fn one_pixel_tiles() -> Vec<u8> {
        let mut bytes = issue_1549();
        for offset in [8, 12] {
            set(&mut bytes, offset, &16_383u32.to_be_bytes());
        }
        for offset in [24, 28] {
            set(&mut bytes, offset, &1u32.to_be_bytes());
        }
        bytes
    }

    /// A JP2 box of `kind` around `payload`, with an explicit length.
    fn jp2_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut bytes = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        bytes.extend_from_slice(kind);
        bytes.extend_from_slice(payload);
        bytes
    }

    /// The whole top-level box of `kind` (with an explicit length) in `bytes`.
    fn whole_box(bytes: &[u8], kind: &[u8; 4]) -> Vec<u8> {
        let start = position(bytes, kind) - 4;
        let length = u32::from_be_bytes(bytes[start..start + 4].try_into().unwrap());
        bytes[start..start + length as usize].to_vec()
    }

    /// The preview crate's real `hayro-jpeg2000` 0.4.1 panic past every
    /// preflight bound (requirement 12, kept for this test): a one-column
    /// 7-bit palette whose `cmap` maps column 0 twice.
    pub(crate) fn hayro_panic() -> Vec<u8> {
        let palette = fixture("jpeg2000-palette.jp2");
        let one_column = jp2_box(b"pclr", &[0, 4, 1, 6, 0, 40, 80, 127]);
        let cmap = jp2_box(b"cmap", &[[0, 0, 1, 0]; 2].concat());
        let header = [whole_box(&palette, b"ihdr"), one_column, cmap].concat();
        let mut bytes = fixture("jpeg2000-rgb.jp2")[..32].to_vec();
        bytes.extend(jp2_box(b"jp2h", &header));
        bytes.extend_from_slice(&palette[position(&palette, b"jp2c") - 4..]);
        bytes
    }

    /// A codestream marker segment: the marker, then a length that counts
    /// itself, then `body`.
    fn segment(marker: u8, body: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0xff, marker];
        bytes.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
        bytes.extend_from_slice(body);
        bytes
    }

    /// A one-component 8-bit J2K codestream with `levels` decomposition
    /// levels of the reversible 5/3 transform in its main header and SIZ's
    /// `[Xsiz, Ysiz, XOsiz, YOsiz, XTsiz, YTsiz, XTOsiz, YTOsiz]`, then one
    /// tile-part per tile with the tile-part header `markers` and an empty
    /// packet (a zero header bit) per resolution level.
    pub(crate) fn codestream(levels: u8, size: [u32; 8], markers: &[u8]) -> Vec<u8> {
        let mut image_and_tiles = 0u16.to_be_bytes().to_vec();
        for value in size {
            image_and_tiles.extend_from_slice(&value.to_be_bytes());
        }
        // One component of unsigned 8-bit samples, sampled 1 × 1.
        image_and_tiles.extend_from_slice(&[0, 1, 7, 1, 1]);
        // LRCP, one layer, no colour transform; 16 × 16 code blocks.
        let cod = [0, 0, 0, 1, 0, levels, 2, 2, 0, 1];
        // No quantization, two guard bits, exponent 8 for every sub-band.
        let mut qcd = vec![0x40];
        qcd.resize(3 * usize::from(levels) + 2, 8 << 3);
        let mut bytes = vec![0xff, 0x4f];
        bytes.extend(segment(0x51, &image_and_tiles));
        bytes.extend(segment(0x52, &cod));
        bytes.extend(segment(0x5c, &qcd));
        let [width, height, x, y, tile_width, tile_height, tile_x, tile_y] = size;
        let tiles = (width - tile_x).div_ceil(tile_width) * (height - tile_y).div_ceil(tile_height);
        assert!(x >= tile_x && y >= tile_y);
        for tile in 0..tiles as u16 {
            let data = vec![0; usize::from(levels) + 1];
            let length = (12 + markers.len() + 2 + data.len()) as u32;
            let mut sot = tile.to_be_bytes().to_vec();
            sot.extend_from_slice(&length.to_be_bytes());
            sot.extend_from_slice(&[0, 1]);
            bytes.extend(segment(0x90, &sot));
            bytes.extend_from_slice(markers);
            bytes.extend_from_slice(&[0xff, 0x93]);
            bytes.extend(data);
        }
        bytes.extend_from_slice(&[0xff, 0xd9]);
        bytes
    }

    /// https://github.com/LaurenzV/hayro/issues/1409 (fixed after 0.4.1):
    /// an empty resolution level at reduced resolution. 1003 × 8 from
    /// x 1 in 1000-wide tiles leaves the second tile x 1001 to 1004, which
    /// three skipped levels (a 125-pixel edge) shrink to no column at all.
    pub(crate) fn empty_resolution_level() -> Vec<u8> {
        codestream(5, [1_004, 8, 1, 0, 1_000, 8, 1, 0], &[])
    }

    /// #1409's other reduced-resolution panic: a tile-part header whose COD
    /// lowers the tile to no decomposition levels below the three that a
    /// 128-pixel edge skips in a 1024 × 64 image coded with five.
    pub(crate) fn tile_part_lowers_levels() -> Vec<u8> {
        let cod = segment(0x52, &[0, 0, 0, 1, 0, 0, 2, 2, 0, 1]);
        let qcd = segment(0x5c, &[0x40, 8 << 3]);
        codestream(5, [1_024, 64, 0, 0, 1_024, 64, 0, 0], &[cod, qcd].concat())
    }

    /// The 8-bit alpha AVIF whose first OBU, its sequence header, claims more
    /// bytes than the item holds.
    pub(crate) fn truncated_obu() -> Vec<u8> {
        let mut bytes = fixture("avif-yuv420-8bit-alpha.avif");
        let mdat = position(&bytes, b"mdat") + 4;
        assert_eq!(
            bytes[mdat], 0x0a,
            "the item starts with its sequence header"
        );
        bytes[mdat + 1] = 0x7f;
        bytes
    }
}

/// The real binary as the image helper, under its filter.
mod real {
    use std::fs;
    use std::io::{self, Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use gitturtle_preview::confined_image_format;
    use gitturtle_preview::pdf_helper::{
        ConfinedFormat, ImageErrorKind, ImageReply, PROTOCOL_VERSION, encode_image_request,
        read_image_reply,
    };

    use super::fixtures::{self, fixture};
    use super::image_helper::{Client, Failure};
    use super::pdf_helper::{DEADLINE, HELPER_FLAG, HelperCommand, Runner, Slots};

    const BINARY: &str = env!("CARGO_BIN_EXE_gitturtle");
    const MIB: u64 = 1024 * 1024;

    /// One image through the client and the real binary, with the app's
    /// deadline, on slots of its own; how long the request took.
    fn decode(bytes: &[u8], edge: u32) -> (Result<ImageReply, Failure>, Duration) {
        let format = confined_image_format(bytes).expect("a confined format");
        let command = HelperCommand::new(
            BINARY,
            [
                HELPER_FLAG.to_string(),
                PROTOCOL_VERSION.to_string(),
                std::process::id().to_string(),
            ],
        );
        // One at a time: a helper may hold its whole 1 GiB.
        let slots = Slots::new(1);
        let client = Client::with(Runner::with(command, DEADLINE, &slots));
        let started = Instant::now();
        let result = client.decode(bytes, format, edge, &|| Ok(()));
        (result, started.elapsed())
    }

    fn signaled(result: &Result<ImageReply, Failure>) -> Option<i32> {
        match result {
            Err(Failure::Stopped { signal }) => *signal,
            _ => None,
        }
    }

    fn pixel(reply: &ImageReply, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * reply.image.width + x) * 4) as usize;
        reply.image.rgba[at..at + 4].try_into().unwrap()
    }

    fn near(actual: [u8; 4], expected: [u8; 4], tolerance: u8) -> bool {
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual.abs_diff(expected) <= tolerance)
    }

    /// Every benign codec fixture decodes in the confined helper within the
    /// deadline and a second, never meeting the filter; the shapes the
    /// review names (4:0:0, alpha, film grain, 12 bits) are among them.
    #[test]
    fn every_benign_fixture_round_trips_in_the_confined_helper() {
        let benign = fixtures::benign();
        let names: Vec<&str> = benign.iter().map(|(name, _)| name.as_str()).collect();
        for required in [
            "avif-yuv400-8bit.avif",
            "avif-yuv400-10bit.avif",
            "avif-yuv400-12bit.avif",
            "avif-yuv420-8bit-alpha.avif",
            "avif-yuv420-12bit-alpha.avif",
            "avif-film-grain.avif",
            "half-red-blue.avif",
            "half-red-blue.jp2",
            "half-red-blue.j2k",
            "jpeg2000-rgb.j2k",
        ] {
            assert!(names.contains(&required), "{required} is missing");
        }
        let mut wrong = Vec::new();
        for (name, bytes) in &benign {
            let (result, elapsed) = decode(bytes, 1_600);
            eprintln!(
                "{name}: {:?} after {elapsed:?}",
                result.as_ref().map(|reply| (
                    reply.image.width,
                    reply.image.height,
                    &reply.image.format
                ))
            );
            if signaled(&result) == Some(libc::SIGSYS) {
                wrong.push(format!("{name} met the filter"));
            }
            if elapsed >= DEADLINE + Duration::from_secs(1) {
                wrong.push(format!("{name} took {elapsed:?}"));
            }
            match &result {
                Ok(reply) if reply.image.format.starts_with(reply.format.name()) => {}
                other => wrong.push(format!("{name}: {other:?}")),
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");

        // A few pixels: half red over half blue, grey stays grey, and alpha
        // survives the helper. An edge past the helper's 1,600 is asked and
        // read as 1,600.
        let (reply, _) = decode(&fixture("half-red-blue.avif"), 2_000);
        let reply = reply.unwrap();
        assert_eq!((reply.image.width, reply.image.height), (64, 64));
        assert!(near(pixel(&reply, 32, 8), [255, 0, 0, 255], 12));
        assert!(near(pixel(&reply, 32, 56), [0, 0, 255, 255], 12));
        let (reply, _) = decode(&fixture("avif-yuv400-12bit.avif"), 1_600);
        let [red, green, blue, alpha] = pixel(&reply.unwrap(), 32, 10);
        assert!(red == green && green == blue && alpha == 255);
        let (reply, _) = decode(&fixture("avif-yuv420-12bit-alpha.avif"), 32);
        let reply = reply.unwrap();
        assert_eq!((reply.image.width, reply.image.height), (32, 24));
        assert!(reply.image.rgba.chunks(4).any(|pixel| pixel[3] < 200));
        assert!(reply.image.rgba.chunks(4).any(|pixel| pixel[3] == 255));
    }

    /// Each hostile input ends in its own error kind within the deadline and
    /// a second, and none meets the filter.
    #[test]
    fn hostile_images_end_in_their_error_kinds_and_never_meet_the_filter() {
        use ImageErrorKind::*;
        let cases: [(&str, Vec<u8>, u32, ImageErrorKind); 11] = [
            ("#1549", fixtures::issue_1549(), 1_600, EdgeLimit),
            (
                "#1558 1 × 1 tiles",
                fixtures::one_pixel_tiles(),
                1_600,
                PixelLimit,
            ),
            (
                "#1409 empty resolution level",
                fixtures::empty_resolution_level(),
                125,
                DecoderFailed,
            ),
            (
                "#1409 tile-part lowers the levels",
                fixtures::tile_part_lowers_levels(),
                128,
                DecoderFailed,
            ),
            (
                "hayro-jpeg2000 panic past the preflight",
                fixtures::hayro_panic(),
                1_600,
                DecoderFailed,
            ),
            (
                "truncated OBU",
                fixtures::truncated_obu(),
                1_600,
                AvifUnreadable,
            ),
            (
                "frame larger than ispe",
                fixture("avif-frame-larger-than-ispe.avif"),
                1_600,
                AvifSizeMismatch,
            ),
            (
                "alpha smaller than the image",
                fixture("avif-alpha-smaller-than-image.avif"),
                1_600,
                AvifSizeMismatch,
            ),
            // dav1d-rs panics on the reserved colour values past every check;
            // the helper contains the panic under its filter.
            (
                "reserved colour values",
                fixture("avif-reserved-colour.avif"),
                1_600,
                DecoderFailed,
            ),
            ("image grid", fixture("avif-grid.avif"), 1_600, AvifGrid),
            (
                "sequence without a still image",
                fixture("avif-sequence-without-still.avif"),
                1_600,
                AvifNoStill,
            ),
        ];
        let mut wrong = Vec::new();
        for (case, bytes, edge, kind) in cases {
            let (result, elapsed) = decode(&bytes, edge);
            eprintln!("{case}: {result:?} after {elapsed:?}");
            if signaled(&result) == Some(libc::SIGSYS) {
                wrong.push(format!("{case} met the filter"));
            }
            if elapsed >= DEADLINE + Duration::from_secs(1) {
                wrong.push(format!("{case} took {elapsed:?}"));
            }
            if !matches!(result, Err(Failure::Document(found)) if found == kind) {
                wrong.push(format!("{case}: {result:?}, expected {kind:?}"));
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
        // Each #1409 codestream decodes without its trigger: at full
        // resolution, and with the tile-part's levels left as they are.
        let controls = [
            ("all levels kept", fixtures::empty_resolution_level(), 1_600),
            (
                "no tile-part COD",
                fixtures::codestream(5, [1_024, 64, 0, 0, 1_024, 64, 0, 0], &[]),
                128,
            ),
        ];
        for (case, bytes, edge) in controls {
            let (result, _) = decode(&bytes, edge);
            assert!(result.is_ok(), "{case}: {result:?}");
        }
    }

    /// What a test saw of one helper it started itself.
    struct Observed {
        /// `/proc/<pid>/status` after the decode, while the reply waits.
        status: String,
        reply: io::Result<Result<ImageReply, ImageErrorKind>>,
        stderr: String,
        code: Option<i32>,
        signal: Option<i32>,
        /// `wait4`'s `ru_maxrss`, in KiB.
        max_rss: u64,
        elapsed: Duration,
    }

    fn field<'a>(status: &'a str, name: &str) -> &'a str {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))
            .map(str::trim)
            .unwrap_or_else(|| panic!("no {name} in\n{status}"))
    }

    fn kib(status: &str, name: &str) -> u64 {
        field(status, name)
            .trim_end_matches("kB")
            .trim()
            .parse()
            .unwrap()
    }

    /// Seccomp filters this test thread runs under, which the helper
    /// inherits (none on a desktop, one in a container).
    fn own_filters() -> u32 {
        let status = fs::read_to_string("/proc/thread-self/status").unwrap();
        field(&status, "Seccomp_filters").parse().unwrap()
    }

    fn pipe() -> (OwnedFd, OwnedFd) {
        let mut fds = [0; 2];
        // SAFETY: fills two descriptors this function then owns.
        assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
        // SAFETY: both were just opened.
        unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) }
    }

    fn set_non_blocking(fd: &OwnedFd, on: bool) {
        // SAFETY: reads and sets the status flags of a pipe this test owns.
        unsafe {
            let flags = libc::fcntl(fd.as_raw_fd(), libc::F_GETFL);
            let flags = if on {
                flags | libc::O_NONBLOCK
            } else {
                flags & !libc::O_NONBLOCK
            };
            assert_eq!(libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags), 0);
        }
    }

    /// Fills the pipe that `fd` writes until it takes no byte more; returns
    /// how many it holds.
    fn fill(fd: &OwnedFd) -> usize {
        set_non_blocking(fd, true);
        let mut filled = 0;
        for size in [4096, 1] {
            let block = vec![b'-'; size];
            loop {
                // SAFETY: writes from a local buffer to a pipe this test owns.
                let written =
                    unsafe { libc::write(fd.as_raw_fd(), block.as_ptr().cast(), block.len()) };
                if written < 0 {
                    assert_eq!(
                        io::Error::last_os_error().raw_os_error(),
                        Some(libc::EAGAIN)
                    );
                    break;
                }
                filled += written as usize;
            }
        }
        // The helper's own writes must block.
        set_non_blocking(fd, false);
        filled
    }

    /// Bytes waiting in the pipe `fd` belongs to.
    fn unread(fd: &impl AsRawFd) -> usize {
        let mut count: libc::c_int = 0;
        // SAFETY: FIONREAD writes one int into a local.
        assert_eq!(
            unsafe { libc::ioctl(fd.as_raw_fd(), libc::FIONREAD, &mut count) },
            0
        );
        count as usize
    }

    fn state(pid: u32) -> Option<char> {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        stat[stat.rfind(')')? + 1..].trim_start().chars().next()
    }

    /// Starts the real binary as the helper of this process with its error
    /// output on a pipe and its reply pipe already full, sends `bytes`, and
    /// reads `/proc/<pid>/status` once the helper has read the whole request
    /// and sleeps: its first reply write is blocked, after the decode, so
    /// the status shows the decoded helper before it exits. Then reads the
    /// reply and reaps the helper with `wait4`.
    fn observe(bytes: &[u8], edge: u32) -> Observed {
        let format: ConfinedFormat = confined_image_format(bytes).unwrap();
        let header = encode_image_request(format, edge, bytes).unwrap();
        let (reply_read, reply_write) = pipe();
        let filled = fill(&reply_write);
        let started = Instant::now();
        let mut child = {
            // Dropped once spawned, so only the helper holds the write end.
            let mut command = Command::new(BINARY);
            command
                .args([
                    HELPER_FLAG.to_string(),
                    PROTOCOL_VERSION.to_string(),
                    std::process::id().to_string(),
                ])
                .env_clear()
                .current_dir("/")
                .stdin(Stdio::piped())
                .stdout(Stdio::from(reply_write))
                .stderr(Stdio::piped());
            command.spawn().unwrap()
        };
        let pid = child.id();
        let mut input = child.stdin.take().unwrap();
        // A helper that died early refuses the rest; the status says why.
        let _ = input
            .write_all(&header)
            .and_then(|()| input.write_all(bytes));
        let bound = DEADLINE + Duration::from_secs(1);
        let mut status = String::new();
        loop {
            if started.elapsed() >= bound {
                // No helper outlives a failed test.
                let _ = child.kill();
                let _ = child.wait();
                panic!("the helper never blocked");
            }
            match state(pid) {
                // Only a sleep after the request is consumed is the reply's.
                Some('S') if unread(&input) == 0 => {
                    // A helper that exits meanwhile shows as a zombie next.
                    if let Ok(read) = fs::read_to_string(format!("/proc/{pid}/status")) {
                        status = read;
                        if field(&status, "State").starts_with('S') {
                            break;
                        }
                    }
                }
                Some('Z') | None => break,
                _ => {}
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        drop(input);
        let mut output = fs::File::from(reply_read);
        let mut reply = Vec::new();
        output.read_to_end(&mut reply).unwrap();
        assert!(reply.len() >= filled && reply[..filled].iter().all(|&byte| byte == b'-'));
        let mut stderr = String::new();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        let mut wait_status = 0;
        // SAFETY: zeroed plain data for the kernel to fill.
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: reaps this test's own child into locals; `child` is never
        // waited for again.
        let reaped = unsafe { libc::wait4(pid as libc::pid_t, &mut wait_status, 0, &mut usage) };
        assert_eq!(reaped, pid as libc::pid_t);
        let elapsed = started.elapsed();
        Observed {
            status,
            reply: read_image_reply(&reply[filled..], format, edge),
            stderr,
            code: libc::WIFEXITED(wait_status).then(|| libc::WEXITSTATUS(wait_status)),
            signal: libc::WIFSIGNALED(wait_status).then(|| libc::WTERMSIG(wait_status)),
            max_rss: usage.ru_maxrss as u64,
            elapsed,
        }
    }

    /// Requirement 13: a helper that has decoded an AVIF with dav1d is still
    /// confined by its own filter, with no new privileges and one thread.
    #[test]
    fn an_image_helper_is_confined_with_one_thread_after_decoding() {
        let inherited = own_filters();
        let observed = observe(&fixture("avif-film-grain.avif"), 1_600);
        assert_eq!(observed.signal, None, "{}", observed.stderr);
        assert_eq!(observed.code, Some(0));
        let status = &observed.status;
        for name in [
            "Name",
            "State",
            "Seccomp",
            "Seccomp_filters",
            "NoNewPrivs",
            "Threads",
        ] {
            eprintln!("{name}: {}", field(status, name));
        }
        assert_eq!(field(status, "Name"), "gitturtle-pdf");
        assert_eq!(field(status, "Seccomp"), "2");
        let filters: u32 = field(status, "Seccomp_filters").parse().unwrap();
        assert_eq!(filters, inherited + 1, "its own filter on top:\n{status}");
        assert_eq!(field(status, "NoNewPrivs"), "1");
        assert_eq!(field(status, "Threads"), "1");
        let reply = observed.reply.unwrap().unwrap();
        assert_eq!(reply.image.format, "AVIF · first image");
        assert!(observed.elapsed < DEADLINE + Duration::from_secs(1));
    }

    /// Requirements 5 and 13: dav1d refuses a 16384 × 16384 frame under a
    /// 64 × 64 `ispe` through `frame_size_limit` before allocating it. It
    /// logs the refusal on the helper's error output, which proves its
    /// logger's `write` passes the filter, and the helper stays small.
    #[test]
    fn a_frame_larger_than_its_ispe_is_refused_by_dav1d_through_the_filter() {
        let observed = observe(&fixture("avif-frame-larger-than-ispe.avif"), 1_600);
        let peak = kib(&observed.status, "VmHWM");
        eprintln!(
            "dav1d logged: {:?}; the helper's VmHWM {peak} KiB, ru_maxrss {} KiB, after {:?}",
            observed.stderr.trim_end(),
            observed.max_rss,
            observed.elapsed
        );
        assert_eq!(observed.signal, None, "{}", observed.stderr);
        assert_eq!(observed.code, Some(0));
        assert!(
            observed
                .stderr
                .contains("Frame size 16384x16384 exceeds limit"),
            "{:?}",
            observed.stderr
        );
        assert_eq!(
            observed.reply.unwrap().unwrap_err(),
            ImageErrorKind::AvifSizeMismatch
        );
        // A 16384 × 16384 frame would need at least 384 MiB of planes.
        assert!(peak < 64 * MIB / 1024, "VmHWM {peak} KiB");
        assert!(
            observed.max_rss < 64 * MIB / 1024,
            "ru_maxrss {} KiB",
            observed.max_rss
        );
        assert!(observed.elapsed < DEADLINE + Duration::from_secs(1));
    }
}

/// The client against `/bin/sh -c` fakes: each outcome, the reply cap, the
/// frame cross-checks, cancellation and the slots it shares with PDF pages.
mod client {
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::process::ExitStatusExt;
    use std::path::{Path, PathBuf};
    use std::process::ExitStatus;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    use anyhow::anyhow;
    use gitturtle_preview::pdf_helper::{
        ConfinedFormat, IMAGE_REQUEST_HEADER_BYTES, ImageErrorKind, ImageFlags, ImageReply,
        PROTOCOL_VERSION, encode_image_request, max_image_reply_bytes,
    };

    use super::fixtures::fixture;
    use super::image_helper::{self, Client, Failure, classify_exit};
    use super::pdf_helper::{
        self, DEADLINE, HELPER_FLAG, HELPERS, HelperCommand, Runner, Slots, exit,
    };

    const LONG: Duration = Duration::from_secs(30);

    /// A shell fake that first writes its pid to `pid`. `exec` keeps that pid
    /// for the program it runs.
    fn fake(pid: &Path, script: &str) -> HelperCommand {
        let script = format!(
            "PATH=/usr/bin:/bin; printf %s $$ > '{}'; {script}",
            pid.display()
        );
        HelperCommand::new("/bin/sh", ["-c", script.as_str()])
    }

    fn avif() -> Vec<u8> {
        fixture("half-red-blue.avif")
    }

    fn jp2() -> Vec<u8> {
        fixture("half-red-blue.jp2")
    }

    /// `bytes`, which are `format`, at `edge` through `command`.
    fn decode_with(
        command: HelperCommand,
        deadline: Duration,
        bytes: &[u8],
        format: ConfinedFormat,
        edge: u32,
    ) -> Result<ImageReply, Failure> {
        let slots = Slots::new(2);
        Client::with(Runner::with(command, deadline, &slots)).decode(
            bytes,
            format,
            edge,
            &|| Ok(()),
        )
    }

    /// The pid a fake wrote, once it has.
    fn pid_in(path: &Path) -> libc::pid_t {
        let started = Instant::now();
        loop {
            if let Some(pid) = fs::read_to_string(path)
                .ok()
                .and_then(|pid| pid.parse().ok())
            {
                return pid;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the fake never started"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    /// The process is reaped: not even a zombie is left.
    fn reaped(pid: libc::pid_t) -> bool {
        // SAFETY: signal 0 only checks that the process exists.
        let result = unsafe { libc::kill(pid, 0) };
        result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    }

    fn prefix(magic: &[u8; 4], status: u8) -> Vec<u8> {
        let mut frame = magic.to_vec();
        frame.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        frame.push(status);
        frame
    }

    /// An image error reply of wire `code` (the kind's position, plus one).
    fn image_error(code: u8) -> Vec<u8> {
        let mut frame = prefix(b"GTIr", 1);
        frame.push(code);
        frame.extend_from_slice(&0u32.to_le_bytes());
        frame
    }

    /// An image reply echoing format `code` with `flags`, `original` and
    /// output `size`, then `rgba`.
    fn image_frame(
        code: u8,
        flags: u16,
        original: (u32, u32),
        size: (u32, u32),
        rgba: &[u8],
    ) -> Vec<u8> {
        let mut frame = prefix(b"GTIr", 0);
        frame.push(code);
        frame.extend_from_slice(&flags.to_le_bytes());
        for value in [original.0, original.1, size.0, size.1, 1, 2, 3] {
            frame.extend_from_slice(&value.to_le_bytes());
        }
        frame.extend_from_slice(rgba);
        frame
    }

    /// A PDF error reply of PDF kind `code` with no detail.
    fn pdf_error(code: u8) -> Vec<u8> {
        let mut frame = prefix(b"GTPr", 1);
        frame.push(code);
        frame.extend_from_slice(&0u32.to_le_bytes());
        frame
    }

    /// A 2 × 1 AVIF image at edge 1,600 whose pixels say SECRET.
    fn secret_frame(flags: u16) -> Vec<u8> {
        image_frame(1, flags, (2, 1), (2, 1), b"SECRETSE")
    }

    struct Fakes {
        directory: tempfile::TempDir,
    }

    impl Fakes {
        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
            }
        }

        fn path(&self, name: &str) -> PathBuf {
            self.directory.path().join(name)
        }

        /// A file holding `bytes`, for a fake to `cat`.
        fn file(&self, name: &str, bytes: &[u8]) -> String {
            let path = self.path(name);
            fs::write(&path, bytes).unwrap();
            format!("'{}'", path.display())
        }
    }

    const UNREADABLE: &str = "The image decoder returned an unreadable result.";
    const STOPPED: &str = "The image decoder stopped on this image.";
    const TIMED_OUT: &str = "The image took longer than 10 seconds to decode and was stopped.";
    const COULD_NOT_START: &str = "The image decoder could not start.";

    /// Requirement 16 and the design's acceptance (3): every failure gives
    /// its own sentence, never helper bytes, and leaves no process; a reply
    /// for the other frame, a flag the echoed format cannot carry and a
    /// reply one byte over its edge's cap are unreadable.
    #[test]
    fn each_image_failure_has_its_own_sentence_and_leaves_no_process() {
        let fakes = Fakes::new();
        // A valid 4 × 4 reply fills the edge-4 cap exactly; one byte more is
        // over it. The fake keeps running, so only the cap ends it early.
        let cap = max_image_reply_bytes(4);
        let exact = image_frame(1, 0, (4, 4), (4, 4), &[7; 64]);
        assert_eq!(exact.len(), cap);
        let mut over = exact.clone();
        over.push(b'S');
        let over = fakes.file("over", &over);
        let mut trailing = secret_frame(0);
        trailing.extend_from_slice(b"SECRET");
        let trailing = fakes.file("trailing", &trailing);
        let wrong_echo = fakes.file(
            "wrong-echo",
            &image_frame(3, 0, (2, 1), (2, 1), b"SECRETSE"),
        );
        // 2 × 1 scales to 2 × 1, not 1 × 1.
        let size = fakes.file("size", &image_frame(1, 0, (2, 1), (1, 1), b"SECR"));
        let pdf_reply = fakes.file("pdf-reply", &pdf_error(6));
        let jp2_flag = fakes.file("jp2-flag", &image_frame(2, 1, (2, 1), (2, 1), b"SECRETSE"));
        let unknown_flag = fakes.file("unknown-flag", &secret_frame(1 << 4));
        let unknown_kind = fakes.file("unknown-kind", &image_error(99));
        // JPEG 2000 cannot report an AVIF grid.
        let avif_kind = fakes.file("avif-kind", &image_error(10));
        let grid = fakes.file("grid", &image_error(10));
        let decoder_failed = fakes.file("decoder-failed", &image_error(8));
        type Expected = fn(&Failure) -> bool;
        let unreadable: Expected = |f| matches!(f, Failure::Unreadable);
        let cases: Vec<(&str, String, ConfinedFormat, u32, Expected, &str)> = vec![
            (
                "deadline",
                "exec sleep 30".into(),
                ConfinedFormat::Avif,
                1_600,
                |f| matches!(f, Failure::TimedOut),
                TIMED_OUT,
            ),
            (
                "oversized reply",
                format!(
                    "head -c {} /dev/zero; exec sleep 30",
                    max_image_reply_bytes(1_600) + 1
                ),
                ConfinedFormat::Avif,
                1_600,
                unreadable,
                UNREADABLE,
            ),
            (
                "one byte over the cap for its edge",
                format!("cat {over}; exec sleep 30"),
                ConfinedFormat::Avif,
                4,
                unreadable,
                UNREADABLE,
            ),
            (
                "wrong echo",
                format!("exec cat {wrong_echo}"),
                ConfinedFormat::Avif,
                1_600,
                unreadable,
                UNREADABLE,
            ),
            (
                "size mismatch",
                format!("exec cat {size}"),
                ConfinedFormat::Avif,
                1_600,
                unreadable,
                UNREADABLE,
            ),
            (
                "trailing bytes",
                format!("exec cat {trailing}"),
                ConfinedFormat::Avif,
                1_600,
                unreadable,
                UNREADABLE,
            ),
            (
                "PDF reply to an image request",
                format!("exec cat {pdf_reply}"),
                ConfinedFormat::Avif,
                1_600,
                unreadable,
                UNREADABLE,
            ),
            (
                "AVIF-only flag on a JP2 echo",
                format!("exec cat {jp2_flag}"),
                ConfinedFormat::Jp2,
                1_600,
                unreadable,
                UNREADABLE,
            ),
            (
                "unknown flag",
                format!("exec cat {unknown_flag}"),
                ConfinedFormat::Avif,
                1_600,
                unreadable,
                UNREADABLE,
            ),
            (
                "unknown kind",
                format!("exec cat {unknown_kind}"),
                ConfinedFormat::Avif,
                1_600,
                unreadable,
                UNREADABLE,
            ),
            (
                "AVIF kind for a JP2",
                format!("exec cat {avif_kind}"),
                ConfinedFormat::Jp2,
                1_600,
                unreadable,
                UNREADABLE,
            ),
            (
                "no reply",
                "exit 0".into(),
                ConfinedFormat::Avif,
                1_600,
                unreadable,
                UNREADABLE,
            ),
            (
                "early exit",
                "echo SECRET >&2; exit 3".into(),
                ConfinedFormat::Avif,
                1_600,
                |f| matches!(f, Failure::Stopped { signal: None }),
                STOPPED,
            ),
            (
                "valid reply, bad exit",
                format!("cat {grid}; exit 1"),
                ConfinedFormat::Avif,
                1_600,
                |f| matches!(f, Failure::Stopped { signal: None }),
                STOPPED,
            ),
            // Only deaths that dump no core: `every_ending_is_classified`
            // covers the others with synthesized statuses.
            (
                "SIGKILL not sent by the client",
                "kill -KILL $$".into(),
                ConfinedFormat::Avif,
                1_600,
                |f| {
                    matches!(
                        f,
                        Failure::Stopped {
                            signal: Some(libc::SIGKILL)
                        }
                    )
                },
                STOPPED,
            ),
            (
                "alarm",
                "kill -ALRM $$".into(),
                ConfinedFormat::Avif,
                1_600,
                |f| matches!(f, Failure::TimedOut),
                TIMED_OUT,
            ),
            (
                "confinement unavailable",
                format!("exit {}", exit::CONFINEMENT),
                ConfinedFormat::Avif,
                1_600,
                |f| matches!(f, Failure::CouldNotStart),
                COULD_NOT_START,
            ),
            (
                "reply not written",
                format!("exit {}", exit::REPLY),
                ConfinedFormat::Avif,
                1_600,
                |f| matches!(f, Failure::Stopped { signal: None }),
                STOPPED,
            ),
            (
                "decoder panic",
                format!("exec cat {decoder_failed}"),
                ConfinedFormat::Jp2,
                1_600,
                |f| matches!(f, Failure::Document(ImageErrorKind::DecoderFailed)),
                STOPPED,
            ),
            (
                "document error",
                format!("exec cat {grid}"),
                ConfinedFormat::Avif,
                1_600,
                |f| matches!(f, Failure::Document(ImageErrorKind::AvifGrid)),
                "AVIF image grids are not decoded on Linux yet",
            ),
        ];
        // Long enough that a reply the cap does not stop meets it instead.
        let deadline = Duration::from_secs(3);
        let mut wrong = Vec::new();
        for (index, (case, script, format, edge, expected, sentence)) in cases.iter().enumerate() {
            let pid = fakes.path(&format!("pid-{index}"));
            let bytes = match format {
                ConfinedFormat::Avif => avif(),
                _ => jp2(),
            };
            let started = Instant::now();
            let result = decode_with(fake(&pid, script), deadline, &bytes, *format, *edge);
            let elapsed = started.elapsed();
            let Err(failure) = result else {
                wrong.push(format!("{case}: an image"));
                continue;
            };
            let message = failure.to_string();
            if !expected(&failure) || message != *sentence || message.contains("SECRET") {
                wrong.push(format!("{case}: {failure:?}, {message}"));
            }
            // Only the deadline case waits for the deadline, and ends within
            // the contract's second past it.
            let bound = if *case == "deadline" {
                deadline + Duration::from_secs(1)
            } else {
                deadline
            };
            if elapsed >= bound {
                wrong.push(format!("{case} waited {elapsed:?}"));
            }
            if !reaped(pid_in(&pid)) {
                wrong.push(format!("{case} left a process"));
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");

        // The reply that fills the cap exactly is an image.
        let exact = fakes.file("exact", &exact);
        let reply = decode_with(
            fake(&fakes.path("pid-exact"), &format!("exec cat {exact}")),
            LONG,
            &avif(),
            ConfinedFormat::Avif,
            4,
        )
        .unwrap();
        assert_eq!(reply.image.rgba, [7; 64]);

        let missing = HelperCommand::new("/nonexistent/gitturtle-helper", [""; 0]);
        let failure = decode_with(missing, LONG, &avif(), ConfinedFormat::Avif, 1_600).unwrap_err();
        assert!(matches!(failure, Failure::CouldNotStart), "{failure:?}");
        assert_eq!(failure.to_string(), COULD_NOT_START);
    }

    /// The PDF client refuses an image reply as unreadable, as the image
    /// client refuses a PDF reply.
    #[test]
    fn an_image_reply_to_a_pdf_request_is_unreadable() {
        let fakes = Fakes::new();
        let pid = fakes.path("pid");
        // Wire code 6 is the image kind PixelLimit and the PDF kind Locked.
        let reply = fakes.file("image-reply", &image_error(6));
        let slots = Slots::new(2);
        let pdf = b"%PDF-1.7\n%%EOF\n";
        let failure =
            pdf_helper::Client::with(fake(&pid, &format!("exec cat {reply}")), LONG, &slots)
                .render(pdf, 0, &|| Ok(()))
                .unwrap_err();
        assert!(
            matches!(failure, pdf_helper::Failure::Unreadable),
            "{failure:?}"
        );
        assert_eq!(
            failure.to_string(),
            "The PDF renderer returned an unreadable result."
        );
        assert!(reaped(pid_in(&pid)));
    }

    #[test]
    fn a_valid_reply_returns_the_image_captioned_from_the_echo_and_flags() {
        let fakes = Fakes::new();
        let animation = fakes.file("animation", &secret_frame(1));
        let pid = fakes.path("pid");
        let reply = decode_with(
            fake(&pid, &format!("exec cat {animation}")),
            LONG,
            &avif(),
            ConfinedFormat::Avif,
            1_600,
        )
        .unwrap();
        assert_eq!(
            reply.flags,
            ImageFlags {
                animation: true,
                ..ImageFlags::default()
            }
        );
        assert_eq!(reply.image.format, "AVIF · first frame of an animation");
        assert_eq!(
            (reply.image.original_width, reply.image.original_height),
            (2, 1)
        );
        assert_eq!(reply.image.rgba, b"SECRETSE");
        assert!(reaped(pid_in(&pid)));
    }

    /// Wait statuses built here: a fake helper killed by SIGSYS, SIGABRT or
    /// SIGXCPU would leave a core file from the shell.
    #[test]
    fn every_ending_is_classified() {
        let signaled = |signal: i32| ExitStatus::from_raw(signal);
        let exited = |code: i32| ExitStatus::from_raw(code << 8);
        let frame = secret_frame(0);
        let avif = ConfinedFormat::Avif;
        for signal in [
            libc::SIGSYS,
            libc::SIGABRT,
            libc::SIGSEGV,
            libc::SIGBUS,
            libc::SIGKILL,
            libc::SIGTERM,
        ] {
            let failure = classify_exit(signaled(signal), &frame, avif, 1_600).unwrap_err();
            assert!(
                matches!(failure, Failure::Stopped { signal: Some(by) } if by == signal),
                "{signal}: {failure:?}"
            );
            assert_eq!(failure.to_string(), STOPPED);
        }
        for signal in [libc::SIGXCPU, libc::SIGALRM] {
            let result = classify_exit(signaled(signal), &frame, avif, 1_600);
            assert!(matches!(result, Err(Failure::TimedOut)), "{signal}");
        }
        for code in [
            exit::ARGUMENTS,
            exit::PARENT,
            exit::SETUP,
            exit::CONFINEMENT,
        ] {
            let result = classify_exit(exited(code), &frame, avif, 1_600);
            assert!(matches!(result, Err(Failure::CouldNotStart)), "{code}");
        }
        for code in [exit::REPLY, 1, 3, 255] {
            let result = classify_exit(exited(code), &frame, avif, 1_600);
            assert!(
                matches!(result, Err(Failure::Stopped { signal: None })),
                "{code}"
            );
        }
        assert!(classify_exit(exited(0), &frame, avif, 1_600).is_ok());
        let empty = classify_exit(exited(0), b"", avif, 1_600);
        assert!(matches!(empty, Err(Failure::Unreadable)));
    }

    /// Requirement 14: a confined format never reaches `decode_image`,
    /// whatever the helper's failure, and every other image never starts a
    /// helper.
    #[test]
    fn a_confined_format_is_never_decoded_in_process() {
        let fakes = Fakes::new();
        // A kind every confined format can report.
        let too_large = fakes.file("too-large", &image_error(7));
        let slots = Slots::new(2);
        let deadline = Duration::from_millis(300);
        let check = || -> anyhow::Result<()> { Ok(()) };
        let cases = [
            ("exit 0".to_string(), UNREADABLE),
            (format!("exit {}", exit::CONFINEMENT), COULD_NOT_START),
            ("exit 3".to_string(), STOPPED),
            ("exec sleep 30".to_string(), TIMED_OUT),
            (
                format!("exec cat {too_large}"),
                "Decoded image exceeds the 256 MiB limit",
            ),
        ];
        for (index, (script, sentence)) in cases.into_iter().enumerate() {
            for (name, bytes) in [
                ("image.avif", avif()),
                ("image.jp2", jp2()),
                ("image.j2k", fixture("half-red-blue.j2k")),
            ] {
                let pid = fakes.path(&format!("pid-{index}-{name}"));
                let client = Client::with(Runner::with(fake(&pid, &script), deadline, &slots));
                let error =
                    image_helper::decode_with(&client, &bytes, name, 1_600, &check).unwrap_err();
                assert_eq!(error.to_string(), sentence, "{script} {name}");
                assert!(reaped(pid_in(&pid)), "{script} {name}");
            }
        }
        let missing = Client::with(Runner::with(
            HelperCommand::new("/nonexistent/gitturtle-helper", [""; 0]),
            deadline,
            &slots,
        ));
        let error =
            image_helper::decode_with(&missing, &avif(), "image.avif", 1_600, &check).unwrap_err();
        assert_eq!(error.to_string(), COULD_NOT_START);
        // A PNG decodes in this process; its helper never starts.
        // The runner checks before it takes a slot, and reaps a helper it
        // started before it returns, so no check and no pid file once the
        // call returns mean no helper ever started.
        let pid = fakes.path("pid-png");
        let client = Client::with(Runner::with(fake(&pid, "exit 0"), deadline, &slots));
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(64, 48, image::Rgba([255, 0, 0, 255]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let checks = AtomicUsize::new(0);
        let counted = || -> anyhow::Result<()> {
            checks.fetch_add(1, Ordering::SeqCst);
            Ok(())
        };
        let image =
            image_helper::decode_with(&client, png.get_ref(), "image.png", 32, &counted).unwrap();
        assert_eq!((image.width, image.height), (32, 24));
        assert_eq!(checks.load(Ordering::SeqCst), 0, "a helper ran for a PNG");
        assert!(!pid.exists(), "a helper started for a PNG");
    }

    #[test]
    fn refused_input_starts_no_helper() {
        let fakes = Fakes::new();
        let slots = Slots::new(2);
        let client = Client::with(Runner::with(
            fake(&fakes.path("pid"), "exit 0"),
            LONG,
            &slots,
        ));
        // The runner checks before it takes a slot, so a refusal that never
        // reached it leaves the check uncalled.
        let checks = AtomicUsize::new(0);
        let counted = || -> anyhow::Result<()> {
            checks.fetch_add(1, Ordering::SeqCst);
            Ok(())
        };
        for (bytes, format, edge) in [
            (avif(), ConfinedFormat::Avif, 0),
            (jp2(), ConfinedFormat::Avif, 1_600),
        ] {
            let failure = client.decode(&bytes, format, edge, &counted).unwrap_err();
            assert!(matches!(failure, Failure::Refused(_)), "{failure:?}");
        }
        assert_eq!(checks.load(Ordering::SeqCst), 0, "a refused request ran");
        assert!(!fakes.path("pid").exists());
    }

    /// An edge past the helper's 1,600 pixels is asked for as 1,600, and the
    /// reply is capped and checked at 1,600, as `decode_image` caps its own
    /// bound silently.
    #[test]
    fn an_edge_past_the_helper_bound_decodes_at_1_600() {
        let fakes = Fakes::new();
        let bytes = avif();
        let header = encode_image_request(ConfinedFormat::Avif, 1_600, &bytes).unwrap();
        let header = fakes.file("header", &header);
        // 2,000 × 1,000 scales to 1,600 × 800 at edge 1,600.
        let rgba = vec![9; 1_600 * 800 * 4];
        let reply = fakes.file(
            "reply",
            &image_frame(1, 0, (2_000, 1_000), (1_600, 800), &rgba),
        );
        let pid = fakes.path("pid");
        // The fake replies only to a request header asking for 1,600.
        let script = format!(
            "head -c {IMAGE_REQUEST_HEADER_BYTES} | cmp -s - {header} || exit 3; exec cat {reply}"
        );
        let reply = decode_with(
            fake(&pid, &script),
            LONG,
            &bytes,
            ConfinedFormat::Avif,
            2_000,
        )
        .unwrap();
        assert_eq!((reply.image.width, reply.image.height), (1_600, 800));
        assert_eq!(
            (reply.image.original_width, reply.image.original_height),
            (2_000, 1_000)
        );
        assert!(reaped(pid_in(&pid)));
    }

    #[test]
    fn the_app_decodes_images_with_this_executable_and_the_pdf_slots() {
        let expected = [
            "/proc/self/exe".to_string(),
            HELPER_FLAG.to_string(),
            PROTOCOL_VERSION.to_string(),
            std::process::id().to_string(),
        ];
        let expected = expected.each_ref().map(OsStr::new);
        let client = Client::new();
        let (command, deadline, slots) = client.runner().parts();
        assert_eq!(command.argv(), expected);
        assert_eq!(deadline, DEADLINE);
        assert!(std::ptr::eq(slots, &HELPERS));
        let pdf = pdf_helper::Client::new();
        assert!(std::ptr::eq(pdf.parts().2, slots));
    }

    #[test]
    fn cancellation_leaves_no_helper_within_100_ms() {
        let fakes = Fakes::new();
        let slots = Slots::new(2);
        let cancelled = AtomicBool::new(false);
        let check = || match cancelled.load(Ordering::SeqCst) {
            true => Err(anyhow!("A newer image replaced this one")),
            false => Ok(()),
        };
        let pid_path = fakes.path("pid");
        let bytes = avif();
        thread::scope(|scope| {
            let request = scope.spawn(|| {
                let client =
                    Client::with(Runner::with(fake(&pid_path, "exec sleep 30"), LONG, &slots));
                let result = client.decode(&bytes, ConfinedFormat::Avif, 1_600, &check);
                (result, Instant::now())
            });
            let pid = pid_in(&pid_path);
            let cancelled_at = Instant::now();
            cancelled.store(true, Ordering::SeqCst);
            let (result, returned) = request.join().unwrap();
            let failure = result.unwrap_err();
            assert_eq!(failure.to_string(), "A newer image replaced this one");
            assert!(matches!(failure, Failure::Cancelled(_)));
            assert!(reaped(pid));
            assert!(returned - cancelled_at < Duration::from_millis(100));
        });
    }

    /// Images and PDF pages take the same two slots: with a page and an
    /// image running, a third request of either kind waits, still cancels
    /// within 100 ms without starting a helper, and starts once one ends.
    #[test]
    fn images_and_pdf_pages_share_two_slots_and_a_waiting_request_still_cancels() {
        let fakes = Fakes::new();
        let grid = fakes.file("grid", &image_error(10));
        let slots = Slots::new(2);
        let release = AtomicBool::new(false);
        let running_check = || match release.load(Ordering::SeqCst) {
            true => Err(anyhow!("released")),
            false => Ok(()),
        };
        let cancel_waiting = AtomicBool::new(false);
        let waiting_check = || match cancel_waiting.load(Ordering::SeqCst) {
            true => Err(anyhow!("cancelled while waiting")),
            false => Ok(()),
        };
        let pids = [0, 1, 2, 3, 4].map(|index| fakes.path(&format!("pid-{index}")));
        let (pdf, image) = (b"%PDF-1.7\n%%EOF\n".as_slice(), avif());
        // Until `count` requests wait in the slots, each having found both
        // taken for a whole tick; a failure ends every request first.
        let waiting_for = |count: usize| {
            let started = Instant::now();
            while slots.waiting() != count {
                if started.elapsed() > Duration::from_secs(10) {
                    cancel_waiting.store(true, Ordering::SeqCst);
                    release.store(true, Ordering::SeqCst);
                    panic!("{} requests wait for a slot, not {count}", slots.waiting());
                }
                thread::sleep(Duration::from_millis(1));
            }
        };
        thread::scope(|scope| {
            let page = scope.spawn(|| {
                pdf_helper::Client::with(fake(&pids[0], "exec sleep 30"), LONG, &slots)
                    .render(pdf, 0, &running_check)
                    .map(|_| ())
                    .map_err(|failure| failure.to_string())
            });
            let picture = scope.spawn(|| {
                Client::with(Runner::with(fake(&pids[1], "exec sleep 30"), LONG, &slots))
                    .decode(&image, ConfinedFormat::Avif, 1_600, &running_check)
                    .map(|_| ())
                    .map_err(|failure| failure.to_string())
            });
            let started = [pid_in(&pids[0]), pid_in(&pids[1])];
            let waiting_image = scope.spawn(|| {
                let result = Client::with(Runner::with(fake(&pids[2], "exit 0"), LONG, &slots))
                    .decode(&image, ConfinedFormat::Avif, 1_600, &waiting_check);
                (
                    result.map(|_| ()).map_err(|failure| failure.to_string()),
                    Instant::now(),
                )
            });
            let waiting_page = scope.spawn(|| {
                let result = pdf_helper::Client::with(fake(&pids[3], "exit 0"), LONG, &slots)
                    .render(pdf, 0, &waiting_check);
                (
                    result.map(|_| ()).map_err(|failure| failure.to_string()),
                    Instant::now(),
                )
            });
            // Both wait for a slot, so neither has started a helper.
            waiting_for(2);
            assert!(
                !pids[2].exists() && !pids[3].exists(),
                "a third helper started"
            );
            let cancelled_at = Instant::now();
            cancel_waiting.store(true, Ordering::SeqCst);
            for waiting in [waiting_image, waiting_page] {
                let (result, returned) = waiting.join().unwrap();
                assert_eq!(result.unwrap_err(), "cancelled while waiting");
                assert!(returned - cancelled_at < Duration::from_millis(100));
            }
            assert!(!pids[2].exists() && !pids[3].exists());

            // A later image starts once the page or the image ends.
            let later = scope.spawn(|| {
                Client::with(Runner::with(
                    fake(&pids[4], &format!("exec cat {grid}")),
                    LONG,
                    &slots,
                ))
                .decode(&image, ConfinedFormat::Avif, 1_600, &|| Ok(()))
            });
            waiting_for(1);
            assert!(!pids[4].exists(), "a third helper started");
            release.store(true, Ordering::SeqCst);
            assert_eq!(page.join().unwrap(), Err("released".to_string()));
            assert_eq!(picture.join().unwrap(), Err("released".to_string()));
            assert!(started.iter().all(|&pid| reaped(pid)));
            let failure = later.join().unwrap().unwrap_err();
            assert!(
                matches!(failure, Failure::Document(ImageErrorKind::AvifGrid)),
                "{failure:?}"
            );
        });
    }
}
