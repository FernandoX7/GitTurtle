use crate::pdf_helper::HelperCommand;
use crate::views::tests::{draw, history_window_from, settle};
use crate::*;
use core::prelude::v1::test;
use gitturtle_preview::pdf_helper::PROTOCOL_VERSION;
use std::path::Path;
use std::time::{Duration, Instant};

/// Longer than a test's drains wait: a helper still running when it
/// should have been stopped fails the test rather than meet its deadline.
const LONG: Duration = Duration::from_secs(60);

/// A fake that never answers and ends with the test process.
pub(crate) const BLOCKS: &str = "while kill -0 $PPID 2>/dev/null; do sleep 0.05; done";

/// How soon a helper whose check failed is gone, counted once the call
/// that failed it has returned: the client checks at least every 25 ms,
/// and the bound stays far from a fake's deadline while it leaves a
/// loaded host's scheduling room.
pub(crate) const PROMPTLY: Duration = Duration::from_secs(1);

const AVIF: &[u8] = include_bytes!("../../../preview/tests/fixtures/half-red-blue.avif");
const JP2: &[u8] = include_bytes!("../../../preview/tests/fixtures/half-red-blue.jp2");
const J2K: &[u8] = include_bytes!("../../../preview/tests/fixtures/half-red-blue.j2k");
const HEIC: &[u8] = include_bytes!("../../../preview/tests/fixtures/half-red-blue.heic");

/// `base` with `name` after it: bytes of the image's own, so a helper of
/// its own, that still sniff as `base`'s format.
fn named_bytes(base: &[u8], name: &str) -> Vec<u8> {
    [base, name.as_bytes()].concat()
}

/// From now on, `bytes` decode through a shell fake that writes its pid
/// to `pid` and then runs `script`, stopped at `deadline`.
fn name_helper(bytes: &[u8], pid: &Path, script: &str, deadline: Duration) {
    let script = format!(
        "PATH=/usr/bin:/bin; printf %s $$ > '{}'; {script}",
        pid.display()
    );
    crate::image_helper::named::name(
        bytes,
        HelperCommand::new("/bin/sh", ["-c", script.as_str()]),
        deadline,
    );
}

/// A script printing a 4 × 2 image reply that echoes format `code` with
/// `flags`, kept as `name` in `directory`.
fn image_reply(directory: &Path, name: &str, code: u8, flags: u16) -> String {
    let mut frame = b"GTIr".to_vec();
    frame.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    frame.extend_from_slice(&[0, code]);
    frame.extend_from_slice(&flags.to_le_bytes());
    for value in [4u32, 2, 4, 2, 1, 2, 3] {
        frame.extend_from_slice(&value.to_le_bytes());
    }
    frame.extend_from_slice(&[200; 4 * 2 * 4]);
    printing(directory, name, &frame)
}

/// A script printing an image error reply of wire `code`.
fn error_reply(directory: &Path, name: &str, code: u8) -> String {
    let mut frame = b"GTIr".to_vec();
    frame.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    frame.extend_from_slice(&[1, code]);
    frame.extend_from_slice(&0u32.to_le_bytes());
    printing(directory, name, &frame)
}

/// A script printing `bytes`, kept as `name` in `directory`.
fn printing(directory: &Path, name: &str, bytes: &[u8]) -> String {
    let path = directory.join(name);
    std::fs::write(&path, bytes).unwrap();
    format!("exec cat '{}'", path.display())
}

/// The pid a fake wrote, once it has.
pub(crate) fn pid_in(path: &Path) -> libc::pid_t {
    let started = Instant::now();
    loop {
        if let Some(pid) = std::fs::read_to_string(path)
            .ok()
            .and_then(|pid| pid.parse().ok())
        {
            return pid;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the fake never started"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// The process is gone and reaped: not even a zombie is left.
fn reaped(pid: libc::pid_t) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    let result = unsafe { libc::kill(pid, 0) };
    result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

/// Whether `pid` is reaped [`PROMPTLY`] after `since`, when the call
/// that failed its check returned.
pub(crate) fn reaped_within(pid: libc::pid_t, since: Instant) -> bool {
    while since.elapsed() < PROMPTLY {
        if reaped(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    reaped(pid)
}

/// A `git fast-import` stream of one commit adding `files`.
fn added(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut input = b"commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> 1700000000 +0000\ndata 6\nimages\n".to_vec();
    for (path, bytes) in files {
        input.extend_from_slice(
            format!("M 100644 inline {path}\ndata {}\n", bytes.len()).as_bytes(),
        );
        input.extend_from_slice(bytes);
        input.push(b'\n');
    }
    input.extend_from_slice(b"done\n");
    input
}

/// Selects the commit's `path`, which enters Compare.
fn select(app: &Entity<GitTurtle>, path: &str, cx: &mut VisualTestContext) {
    let index = app.read_with(cx, |app, _| {
        app.files
            .iter()
            .position(|file| file.path() == Path::new(path))
            .expect("the commit adds it")
    });
    cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(index, window, cx)));
}

/// Compares the commit's `path` once its preview has arrived.
async fn compare(app: &Entity<GitTurtle>, path: &str, cx: &mut VisualTestContext) {
    select(app, path, cx);
    app.read_with(cx, |app, _| app.worker.wait_idle());
    settle(app, cx).await;
    draw(cx);
}

/// Whether After's caption draws `details` beneath its name.
fn after_says(details: &str, cx: &mut VisualTestContext) -> bool {
    let selector: &'static str = format!("image-side-details-1: {details}").leak();
    cx.debug_bounds(selector).is_some()
}

/// What After says in its image's place, read as static text, if it
/// draws `reason` there whole.
fn after_reason(reason: &str, cx: &mut VisualTestContext) -> Option<String> {
    crate::native_accessibility::written::forget();
    draw(cx);
    let selector: &'static str = format!("image-side-empty-1: {reason}").leak();
    cx.debug_bounds(selector)?;
    crate::native_accessibility::written::labels()
        .into_iter()
        .find(|(written, _)| written.starts_with("image-side-empty") && written.ends_with('1'))
        .and_then(|(_, name)| name)
}

/// Captions on Linux are macOS's without "macOS codec", built from the
/// reply's format echo and flags: each flag adds its words, the first
/// frame of an animation in place of the first image. Each side offers
/// Open in default app.
#[gpui::test]
async fn helper_images_are_captioned_from_their_echo_and_flags(cx: &mut TestAppContext) {
    let replies = tempfile::tempdir().unwrap();
    let cases: [(&str, &[u8], u8, u16, &str); 7] = [
        ("still.avif", AVIF, 1, 0, "AVIF · first image"),
        (
            "animation.avif",
            AVIF,
            1,
            1,
            "AVIF · first frame of an animation",
        ),
        (
            "cropped.avif",
            AVIF,
            1,
            2,
            "AVIF · first image · crop not applied",
        ),
        (
            "hdr.avif",
            AVIF,
            1,
            4,
            "AVIF · first image · HDR not tone-mapped",
        ),
        (
            "profile.avif",
            AVIF,
            1,
            8,
            "AVIF · first image · color profile not applied",
        ),
        ("picture.jp2", JP2, 2, 0, "JPEG 2000 / JP2 · first image"),
        (
            "codestream.j2k",
            J2K,
            3,
            0,
            "JPEG 2000 / J2K codestream · first image",
        ),
    ];
    let images: Vec<Vec<u8>> = cases
        .iter()
        .map(|(path, base, ..)| named_bytes(base, path))
        .collect();
    for ((path, _, code, flags, _), bytes) in cases.iter().zip(&images) {
        let reply = image_reply(replies.path(), path, *code, *flags);
        let pid = replies.path().join(format!("{path}.pid"));
        name_helper(bytes, &pid, &reply, LONG);
    }
    let files: Vec<(&str, &[u8])> = cases
        .iter()
        .zip(&images)
        .map(|((path, ..), bytes)| (*path, bytes.as_slice()))
        .collect();
    let (_fixture, app, cx) = history_window_from(cx, added(&files)).await;
    cx.simulate_resize(size(px(1480.), px(973.)));
    for (path, _, _, _, caption) in cases {
        compare(&app, path, cx).await;
        let details = format!("4 × 2 · {caption}");
        assert!(after_says(&details, cx), "{path} draws {details:?}");
        assert!(
            cx.debug_bounds("image-system-preview-1").is_some(),
            "{path} offers Open in default app"
        );
    }
}

/// An image that fails: its path, the bytes it is made from, its fake's
/// script and deadline (none for an image no helper is named for) and
/// the sentence it says.
type Failing<'a> = (&'a str, &'a [u8], Option<(&'a str, Duration)>, &'a str);

/// Each helper failure and a document error say why where After's image
/// would be, drawn and read as static text in the client's fixed words;
/// HEIF/HEIC, which Linux never decodes, says so. Each side still offers
/// Open in default app, and each helper is reaped.
#[gpui::test]
async fn helper_failures_say_why_and_keep_open_in_default_app(cx: &mut TestAppContext) {
    let fakes = tempfile::tempdir().unwrap();
    let short = Duration::from_millis(200);
    let grid = error_reply(fakes.path(), "grid", 10);
    let cases: [Failing<'_>; 6] = [
        (
            "slow.avif",
            AVIF,
            Some(("exec sleep 30", short)),
            "The image took longer than 10 seconds to decode and was stopped.",
        ),
        (
            "stopped.jp2",
            JP2,
            Some(("exit 3", LONG)),
            "The image decoder stopped on this image.",
        ),
        (
            "unreadable.j2k",
            J2K,
            Some(("exit 0", LONG)),
            "The image decoder returned an unreadable result.",
        ),
        // No helper is named for it, so its helper cannot start.
        (
            "unstartable.avif",
            AVIF,
            None,
            "The image decoder could not start.",
        ),
        (
            "grid.avif",
            AVIF,
            Some((grid.as_str(), LONG)),
            "AVIF image grids are not decoded on Linux yet",
        ),
        (
            "photo.heic",
            HEIC,
            None,
            "HEIF / HEIC images are not decoded on Linux",
        ),
    ];
    let images: Vec<Vec<u8>> = cases
        .iter()
        .map(|(path, base, ..)| named_bytes(base, path))
        .collect();
    for ((path, _, helper, _), bytes) in cases.iter().zip(&images) {
        if let Some((script, deadline)) = helper {
            let pid = fakes.path().join(format!("{path}.pid"));
            name_helper(bytes, &pid, script, *deadline);
        }
    }
    let files: Vec<(&str, &[u8])> = cases
        .iter()
        .zip(&images)
        .map(|((path, ..), bytes)| (*path, bytes.as_slice()))
        .collect();
    let (_fixture, app, cx) = history_window_from(cx, added(&files)).await;
    cx.simulate_resize(size(px(1480.), px(973.)));
    for (path, _, helper, sentence) in cases {
        compare(&app, path, cx).await;
        assert_eq!(
            after_reason(sentence, cx).as_deref(),
            Some(sentence),
            "{path} draws and reads its reason in its image's place"
        );
        assert!(
            cx.debug_bounds("image-system-preview-1").is_some(),
            "{path} offers Open in default app"
        );
        if helper.is_some() {
            let pid = pid_in(&fakes.path().join(format!("{path}.pid")));
            assert!(reaped(pid), "{path}'s helper is reaped");
        }
    }
}

/// A slow AVIF superseded by another file's selection: its helper is
/// killed and reaped promptly, counted from the selection's return so
/// the view's own work never counts, its late result never reaches the
/// view, and Compare shows the file selected next.
#[gpui::test]
async fn another_selection_drops_a_slow_image_and_kills_its_helper(cx: &mut TestAppContext) {
    let fakes = tempfile::tempdir().unwrap();
    let slow = named_bytes(AVIF, "another selection");
    let started = fakes.path().join("slow");
    name_helper(&slow, &started, BLOCKS, LONG);
    let png = crate::system_preview::tests::png(None);
    let input = added(&[("figure.png", &png), ("slow.avif", &slow)]);
    let (_fixture, app, cx) = history_window_from(cx, &input).await;
    select(&app, "slow.avif", cx);
    let helper = pid_in(&started);
    select(&app, "figure.png", cx);
    assert!(
        reaped_within(helper, Instant::now()),
        "the slow image's helper is killed and reaped as the selection changes"
    );
    app.read_with(cx, |app, _| app.worker.wait_idle());
    settle(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert!(app.mode == WorkspaceMode::Compare);
        let Some(Content::Images { new, .. }) = app.content.as_deref() else {
            panic!("the PNG's comparison is shown");
        };
        assert_eq!(
            new.image.as_ref().map(|image| image.format.as_str()),
            Some("PNG")
        );
    });
}

/// Back while a JP2 decodes: its helper is killed and reaped promptly
/// after Back returns, nothing is left loading, and its late result
/// never reopens Compare.
#[gpui::test]
async fn back_drops_a_slow_image_and_never_reopens_compare(cx: &mut TestAppContext) {
    let fakes = tempfile::tempdir().unwrap();
    let slow = named_bytes(JP2, "Back");
    let started = fakes.path().join("slow");
    name_helper(&slow, &started, BLOCKS, LONG);
    let (_fixture, app, cx) = history_window_from(cx, added(&[("slow.jp2", &slow)])).await;
    select(&app, "slow.jp2", cx);
    let helper = pid_in(&started);
    cx.update(|window, cx| app.update(cx, |app, cx| app.back_to_history(window, cx)));
    assert!(
        reaped_within(helper, Instant::now()),
        "the slow image's helper is killed and reaped on Back"
    );
    app.read_with(cx, |app, _| app.worker.wait_idle());
    settle(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert!(app.mode == WorkspaceMode::History, "Compare stays closed");
        assert_eq!(app.loading, None);
        assert!(app.content.is_none(), "no comparison arrives");
    });
}

/// A switch to another repository tab while an AVIF decodes: its helper
/// is killed and reaped promptly after the switch returns, and its late
/// result reaches neither tab, so the tab shown stays in History.
#[gpui::test]
async fn a_tab_switch_drops_a_slow_image_and_kills_its_helper(cx: &mut TestAppContext) {
    let fakes = tempfile::tempdir().unwrap();
    let slow = named_bytes(AVIF, "a tab switch");
    let started = fakes.path().join("slow");
    name_helper(&slow, &started, BLOCKS, LONG);
    let (_fixture, app, cx) = history_window_from(cx, added(&[("slow.avif", &slow)])).await;
    let second = tempfile::tempdir().unwrap();
    let repo = crate::views::tests::fixture_repository(second.path().join("second"));
    std::fs::write(repo.path().join("notes.txt"), "second notes\n").unwrap();
    crate::tags::tests::git(repo.path(), &["add", "notes.txt"]);
    crate::tags::tests::git(repo.path(), &["commit", "--quiet", "-m", "second notes"]);
    let path = repo.path().to_owned();
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_repository_tab(path, window, cx)));
    settle(&app, cx).await;
    cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
    settle(&app, cx).await;
    select(&app, "slow.avif", cx);
    let helper = pid_in(&started);
    cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx)));
    assert!(
        reaped_within(helper, Instant::now()),
        "the slow image's helper is killed and reaped as the tab changes"
    );
    app.read_with(cx, |app, _| app.worker.wait_idle());
    settle(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert_eq!(app.repository_tabs.active, Some(1));
        assert!(app.mode == WorkspaceMode::History, "Compare stays closed");
        assert!(app.content.is_none(), "no comparison arrives");
    });
}
