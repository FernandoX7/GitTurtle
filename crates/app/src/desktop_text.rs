//! Desktop text preferences on Linux: glyph antialiasing and text scaling.
//!
//! The toolkit's Linux backend always recommends subpixel (RGB stripe) glyph
//! rendering and never consults the session's font settings. That fringes on
//! panels without a horizontal RGB stripe, such as WOLED and QD-OLED monitors
//! or rotated screens, and diverges from GTK 4, which renders every glyph
//! grayscale. This module reads the desktop preference through the XDG
//! settings portal, falls back to fontconfig's resolved defaults when a portal
//! backend does not publish GNOME's keys, and follows portal changes while the app
//! runs. Returning to a window refreshes the snapshot and fontconfig fallback. On Wayland it also applies the desktop text scaling factor ("Large
//! Text"), where the toolkit has no DPI source; the X11 backend already scales
//! the whole window through `Xft.dpi`. Nothing here writes a setting.

use gpui_kit::AsyncApp;

/// Waits for the first applied desktop snapshot. After the startup deadline,
/// launch uses the safe defaults already installed by `start`.
#[cfg(target_os = "linux")]
pub(super) struct Initial(futures::channel::oneshot::Receiver<()>);
#[cfg(not(target_os = "linux"))]
pub(super) struct Initial;

#[cfg(target_os = "linux")]
const STARTUP_WAIT: std::time::Duration = std::time::Duration::from_millis(400);

#[cfg(target_os = "linux")]
struct Observer {
    worker: Option<gpui_kit::Task<()>>,
    cancel: futures::future::AbortHandle,
    updates: Option<gpui_kit::Task<()>>,
    refresh: futures::channel::mpsc::Sender<()>,
}
#[cfg(target_os = "linux")]
impl gpui_kit::Global for Observer {}

#[cfg(target_os = "linux")]
pub(super) fn start(cx: &mut gpui_kit::App) -> Initial {
    use futures::StreamExt as _;

    // The deadline also needs a useful first frame if the worker cannot run.
    apply(Observed::default().resolve(false), cx);
    let (sender, mut snapshots, latest) = portal::snapshots();
    let (refresh, requests) = futures::channel::mpsc::channel(0);
    let (ready, initial) = futures::channel::oneshot::channel();
    let scales_text = gpui_kit::guess_compositor() == "Wayland";
    let executor = cx.background_executor().clone();
    let (cancel, cancellation) = futures::future::AbortHandle::new_pair();
    let worker = executor.clone().spawn(async move {
        let _ = futures::future::Abortable::new(
            portal::observe(scales_text, sender, requests, executor),
            cancellation,
        )
        .await;
    });
    let updates = cx.spawn(async move |cx| {
        let mut ready = Some(ready);
        while snapshots.next().await.is_some() {
            let snapshot = latest.lock().unwrap().take();
            if let Some(snapshot) = snapshot {
                cx.update(|cx| apply(snapshot, cx));
                if let Some(ready) = ready.take() {
                    let _ = ready.send(());
                }
            }
        }
    });
    cx.set_global(Observer {
        worker: Some(worker),
        cancel,
        updates: Some(updates),
        refresh,
    });
    cx.on_app_quit(|cx| {
        let observer = cx.global_mut::<Observer>();
        observer.cancel.abort();
        observer.updates.take();
        let worker = observer.worker.take();
        async move {
            // Join cancellation before process exit, including reap of a
            // running child (bounded to 100 ms, below GPUI's shutdown budget).
            // Pending D-Bus reads and signal subscriptions are dropped.
            if let Some(worker) = worker {
                worker.await;
            }
        }
    })
    .detach();
    Initial(initial)
}

/// One coalesced read on focus return also handles fontconfig-only desktops
/// and a portal that was unavailable at launch, without an idle polling loop.
#[cfg(target_os = "linux")]
pub(super) fn refresh(cx: &mut gpui_kit::App) {
    if cx.has_global::<Observer>() {
        let _ = cx.global_mut::<Observer>().refresh.try_send(());
    }
}
#[cfg(not(target_os = "linux"))]
pub(super) fn refresh(_cx: &mut gpui_kit::App) {}

#[cfg(not(target_os = "linux"))]
pub(super) fn start(_cx: &mut gpui_kit::App) -> Initial {
    Initial
}

#[cfg(target_os = "linux")]
pub(super) async fn ready(initial: Initial, cx: &AsyncApp) {
    let timeout = cx.background_executor().timer(STARTUP_WAIT);
    futures::future::select(Box::pin(initial.0), Box::pin(timeout)).await;
}
#[cfg(not(target_os = "linux"))]
pub(super) async fn ready(_initial: Initial, _cx: &AsyncApp) {}

#[cfg(target_os = "linux")]
fn apply(snapshot: DesktopText, cx: &mut gpui_kit::App) {
    let mut changed = crate::appearance::set_desktop_text_scale(snapshot.text_scale, cx);
    if cx.text_rendering_mode() != snapshot.rendering {
        cx.set_text_rendering_mode(snapshot.rendering);
        changed = true;
    }
    if changed {
        cx.refresh_windows();
    }
}

/// The session's resolved text preferences.
#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct DesktopText {
    pub rendering: gpui_kit::TextRenderingMode,
    pub text_scale: f32,
}

/// Raw values as reported by the desktop; each source may be absent.
#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Observed {
    /// GNOME `font-rendering`: "automatic" (GNOME 47+ default) or "manual".
    pub font_rendering: Option<String>,
    /// GNOME `font-antialiasing`: "rgba", "grayscale" or "none".
    pub antialiasing: Option<String>,
    /// GNOME `text-scaling-factor`.
    pub text_scale: Option<f64>,
    /// fontconfig's `antialias|rgba` defaults for the generic sans family.
    pub fontconfig: Option<String>,
}

#[cfg(any(target_os = "linux", test))]
impl Observed {
    /// Prefers the desktop's own preference, then fontconfig, then grayscale:
    /// the one mode that cannot fringe on an unknown panel layout and matches
    /// GTK 4 on the same desktop. Text scaling applies only where the toolkit
    /// would not otherwise scale the window.
    pub(super) fn resolve(&self, scales_text: bool) -> DesktopText {
        let rendering =
            rendering_from_gnome(self.font_rendering.as_deref(), self.antialiasing.as_deref())
                .or_else(|| {
                    self.fontconfig
                        .as_deref()
                        .and_then(rendering_from_fontconfig)
                })
                .unwrap_or(gpui_kit::TextRenderingMode::Grayscale);
        let text_scale = if scales_text {
            self.text_scale
                .map(crate::appearance::desktop_text_scale_value)
                .unwrap_or(1.0)
        } else {
            1.0
        };
        DesktopText {
            rendering,
            text_scale,
        }
    }
}

/// GNOME's `font-rendering` "automatic" delegates the choice to the toolkit;
/// GTK 4 then renders grayscale regardless of `font-antialiasing`, so the app
/// matches its neighbours. "manual" (and desktops without the newer key)
/// follows the low-level antialiasing preference. The toolkit cannot disable
/// antialiasing, so "none" is rendered grayscale.
#[cfg(any(target_os = "linux", test))]
pub(super) fn rendering_from_gnome(
    font_rendering: Option<&str>,
    antialiasing: Option<&str>,
) -> Option<gpui_kit::TextRenderingMode> {
    use gpui_kit::TextRenderingMode::{Grayscale, Subpixel};
    if font_rendering == Some("automatic") {
        return Some(Grayscale);
    }
    match antialiasing? {
        "rgba" => Some(Subpixel),
        "grayscale" | "none" => Some(Grayscale),
        _ => None,
    }
}

/// Parses `fc-match --format '%{antialias}|%{rgba}'`. `rgba` uses fontconfig's
/// constants: 0 unknown, 1 rgb, 2 bgr, 3 vrgb, 4 vbgr, 5 none. The toolkit
/// renders horizontal stripes only and takes their order from the compositor,
/// so every other layout is rendered grayscale.
#[cfg(any(target_os = "linux", test))]
pub(super) fn rendering_from_fontconfig(defaults: &str) -> Option<gpui_kit::TextRenderingMode> {
    use gpui_kit::TextRenderingMode::{Grayscale, Subpixel};
    let mut fields = defaults.trim().split('|');
    let antialias = match fields.next()?.trim() {
        "True" | "true" | "1" => true,
        "False" | "false" | "0" => false,
        _ => return None,
    };
    let rgba: u8 = fields.next()?.trim().parse().ok()?;
    if fields.next().is_some() {
        return None;
    }
    Some(match (antialias, rgba) {
        (true, 1 | 2) => Subpixel,
        (true, 0 | 3 | 4 | 5) | (false, 0..=5) => Grayscale,
        _ => return None,
    })
}

#[cfg(target_os = "linux")]
mod portal {
    use super::{DesktopText, Observed};
    use ashpd::{desktop::settings::Settings, zbus, zvariant::OwnedValue};
    use futures::channel::mpsc::{self, Receiver, Sender};
    use futures::{FutureExt as _, StreamExt as _};
    use gpui_kit::BackgroundExecutor;
    use std::io::Read as _;
    use std::os::fd::AsRawFd as _;
    use std::process::{Child, Command, Stdio};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    const INTERFACE: &str = "org.gnome.desktop.interface";
    const FONT_RENDERING: &str = "font-rendering";
    const FONT_ANTIALIASING: &str = "font-antialiasing";
    const TEXT_SCALING_FACTOR: &str = "text-scaling-factor";
    const PORTAL_WAIT: Duration = Duration::from_millis(250);
    const FONTCONFIG_WAIT: Duration = Duration::from_millis(100);
    const FONTCONFIG_BYTES: usize = 128;
    const SIGNAL_BATCH: usize = 32;
    const CHANGE_COALESCE_WAIT: Duration = Duration::from_millis(16);

    pub(super) type Latest = Arc<Mutex<Option<DesktopText>>>;
    pub(super) struct Snapshots {
        latest: Latest,
        wake: Sender<()>,
    }

    pub(super) fn snapshots() -> (Snapshots, Receiver<()>, Latest) {
        // A futures sender has one reserved slot even with zero buffer slots.
        // Store the value separately so bursts replace, rather than queue, it.
        let (wake, receiver) = mpsc::channel(0);
        let latest = Arc::new(Mutex::new(None));
        (
            Snapshots {
                latest: latest.clone(),
                wake,
            },
            receiver,
            latest,
        )
    }

    impl Snapshots {
        fn send(&mut self, snapshot: DesktopText) -> bool {
            *self.latest.lock().unwrap() = Some(snapshot);
            match self.wake.try_send(()) {
                Ok(()) => true,
                Err(error) => error.is_full(),
            }
        }
    }

    async fn bounded<T>(
        executor: &BackgroundExecutor,
        future: impl Future<Output = T>,
    ) -> Option<T> {
        match futures::future::select(Box::pin(future), executor.timer(PORTAL_WAIT)).await {
            futures::future::Either::Left((value, _)) => Some(value),
            // The losing future is dropped here, not detached behind a UI timeout.
            futures::future::Either::Right(_) => None,
        }
    }

    struct Session {
        // Retain the private connection only for this subscription's lifetime.
        _settings: Settings,
        changes: zbus::proxy::SignalStream<'static>,
        observed: Observed,
    }

    async fn connect() -> Option<Session> {
        let connection = zbus::connection::Builder::session()
            .ok()?
            .max_queued(SIGNAL_BATCH)
            .build()
            .await
            .ok()?;
        let settings = Settings::with_connection(connection).await.ok()?;
        // Subscribe first, on one ordered stream. Separate per-key streams can
        // reorder coupled font-rendering/antialiasing changes.
        let mut changes = settings
            .receive_signal_with_args("SettingChanged", &[(0, INTERFACE)])
            .await
            .ok()?;
        let observed = snapshot(&mut changes, async {
            let (font_rendering, antialiasing, text_scale) = futures::join!(
                settings.read(INTERFACE, FONT_RENDERING),
                settings.read(INTERFACE, FONT_ANTIALIASING),
                settings.read(INTERFACE, TEXT_SCALING_FACTOR),
            );
            Observed {
                font_rendering: font_rendering.ok(),
                antialiasing: antialiasing.ok(),
                text_scale: text_scale.ok(),
                fontconfig: None,
            }
        })
        .await;
        Some(Session {
            _settings: settings,
            changes,
            observed,
        })
    }

    async fn snapshot(
        changes: &mut (impl futures::Stream<Item = zbus::Message> + Unpin),
        reads: impl Future<Output = Observed>,
    ) -> Observed {
        let reads = reads.fuse();
        futures::pin_mut!(reads);
        let mut updates = Observed::default();
        let mut seen = 0;
        let mut count = 0;
        let mut initial = loop {
            futures::select_biased! {
                initial = reads => break initial,
                message = changes.next().fuse() => match message {
                    Some(message) => {
                        seen |= change(&mut updates, message);
                        count += 1;
                        if count == SIGNAL_BATCH {
                            // Let the enclosing deadline/cancellation run even
                            // when the backend produces a never-idle stream.
                            yield_once().await;
                            count = 0;
                        }
                    },
                    None => break reads.await,
                },
            }
        };
        // Consume signals while the reads are in flight, so a full bounded
        // signal queue cannot block delivery of the read replies. A signal
        // received during the snapshot wins over that key's earlier read.
        if seen & 1 != 0 {
            initial.font_rendering = updates.font_rendering;
        }
        if seen & 2 != 0 {
            initial.antialiasing = updates.antialiasing;
        }
        if seen & 4 != 0 {
            initial.text_scale = updates.text_scale;
        }
        initial
    }

    fn change(observed: &mut Observed, message: zbus::Message) -> u8 {
        let body = message.body();
        let Ok((namespace, key, value)) = body.deserialize::<(&str, &str, OwnedValue)>() else {
            return 0;
        };
        if namespace != INTERFACE {
            return 0;
        }
        // Invalid values clear the old preference so resolution can fall back.
        match key {
            FONT_RENDERING => {
                observed.font_rendering = <&str>::try_from(&value).ok().map(str::to_owned);
                1
            }
            FONT_ANTIALIASING => {
                observed.antialiasing = <&str>::try_from(&value).ok().map(str::to_owned);
                2
            }
            TEXT_SCALING_FACTOR => {
                observed.text_scale = f64::try_from(&value).ok();
                4
            }
            _ => 0,
        }
    }

    async fn yield_once() {
        let mut yielded = false;
        futures::future::poll_fn(move |cx| {
            if yielded {
                std::task::Poll::Ready(())
            } else {
                yielded = true;
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            }
        })
        .await;
    }

    fn drain(session: &mut Session) -> bool {
        let mut changed = false;
        // Fixed work per turn, even if a backend continuously emits signals.
        for _ in 0..SIGNAL_BATCH {
            match session.changes.next().now_or_never() {
                Some(Some(message)) => {
                    changed |= change(&mut session.observed, message) != 0;
                }
                _ => break,
            }
        }
        changed
    }

    /// All external reads run in one app-owned background task. Both setup
    /// and child execution have deadlines; the UI retains one latest value.
    pub(super) async fn observe(
        scales_text: bool,
        mut sender: Snapshots,
        mut refresh: Receiver<()>,
        executor: BackgroundExecutor,
    ) {
        loop {
            let mut session = bounded(&executor, connect()).await.flatten();
            if let Some(session) = &mut session {
                drain(session);
            }
            let mut observed = session
                .as_ref()
                .map(|session| session.observed.clone())
                .unwrap_or_default();
            if super::rendering_from_gnome(
                observed.font_rendering.as_deref(),
                observed.antialiasing.as_deref(),
            )
            .is_none()
            {
                observed.fontconfig = fontconfig_defaults();
            }
            if !sender.send(observed.resolve(scales_text)) {
                return;
            }
            let Some(mut session) = session else {
                if refresh.next().await.is_none() {
                    return;
                }
                continue;
            };
            session.observed = observed;
            loop {
                // Focus requests and changes are bounded independently. Refresh
                // replaces the connection, including a lost/restarted portal.
                let message = futures::select_biased! {
                    request = refresh.next().fuse() => {
                        if request.is_none() { return; }
                        break;
                    },
                    message = session.changes.next().fuse() => message,
                };
                let Some(message) = message else {
                    if refresh.next().await.is_none() {
                        return;
                    }
                    break;
                };
                let changed = change(&mut session.observed, message) != 0;
                if drain(&mut session) || changed {
                    if super::rendering_from_gnome(
                        session.observed.font_rendering.as_deref(),
                        session.observed.antialiasing.as_deref(),
                    )
                    .is_none()
                    {
                        session.observed.fontconfig = fontconfig_defaults();
                    }
                    if !sender.send(session.observed.resolve(scales_text)) {
                        return;
                    }
                }
                executor.timer(CHANGE_COALESCE_WAIT).await;
            }
        }
    }

    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            // Reap even on an output-limit/read error or deadline. No detached
            // subprocess or reader thread survives a failed query.
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// fontconfig-only desktops are reread when a window regains focus.
    fn fontconfig_defaults() -> Option<String> {
        command_output(
            Command::new("fc-match").args(["--format", "%{antialias}|%{rgba}", "sans-serif"]),
            FONTCONFIG_WAIT,
        )
    }

    fn command_output(command: &mut Command, timeout: Duration) -> Option<String> {
        let mut child = ChildGuard(
            command
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .stdout(Stdio::piped())
                .spawn()
                .ok()?,
        );
        let mut stdout = child.0.stdout.take()?;
        let fd = stdout.as_raw_fd();
        // SAFETY: stdout owns this valid pipe descriptor throughout both calls.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return None;
        }
        let deadline = Instant::now() + timeout;
        let mut output = Vec::with_capacity(FONTCONFIG_BYTES);
        let mut buffer = [0; FONTCONFIG_BYTES + 1];
        loop {
            match stdout.read(&mut buffer) {
                Ok(count) => {
                    if output.len() + count > FONTCONFIG_BYTES {
                        return None;
                    }
                    output.extend_from_slice(&buffer[..count]);
                    if count == 0 {
                        if let Some(status) = child.0.try_wait().ok()? {
                            return status
                                .success()
                                .then(|| String::from_utf8(output).ok())
                                .flatten();
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return None,
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn signal(key: &str, value: OwnedValue) -> zbus::Message {
            zbus::Message::signal(
                "/org/freedesktop/portal/desktop",
                "org.freedesktop.portal.Settings",
                "SettingChanged",
            )
            .unwrap()
            .build(&(INTERFACE, key, value))
            .unwrap()
        }

        #[test]
        fn snapshot_drains_bursts_during_reads_and_keeps_latest_signals() {
            use futures::SinkExt as _;
            let (mut sender, mut changes) = mpsc::channel(0);
            let (reply, reads) = futures::channel::oneshot::channel();
            let emit = async move {
                for index in 0..100 {
                    sender
                        .send(signal(TEXT_SCALING_FACTOR, OwnedValue::from(index as f64)))
                        .await
                        .unwrap();
                }
                // Wrong type must clear the previous antialiasing value.
                sender
                    .send(signal(FONT_ANTIALIASING, OwnedValue::from(123u32)))
                    .await
                    .unwrap();
                reply
                    .send(Observed {
                        font_rendering: Some("manual".into()),
                        antialiasing: Some("rgba".into()),
                        text_scale: Some(1.25),
                        fontconfig: None,
                    })
                    .unwrap();
            };
            let (observed, ()) = futures::executor::block_on(async {
                futures::join!(snapshot(&mut changes, async { reads.await.unwrap() }), emit)
            });
            assert_eq!(observed.font_rendering.as_deref(), Some("manual"));
            assert_eq!(observed.antialiasing, None);
            assert_eq!(observed.text_scale, Some(99.0));
        }

        #[gpui_kit::test]
        async fn portal_deadline_drops_pending_work(cx: &mut gpui_kit::TestAppContext) {
            use std::sync::atomic::{AtomicBool, Ordering};
            struct Pending(Arc<AtomicBool>);
            impl Drop for Pending {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::SeqCst);
                }
            }
            let dropped = Arc::new(AtomicBool::new(false));
            let pending = Pending(dropped.clone());
            let executor = cx.executor();
            let task = executor.clone().spawn(async move {
                bounded(&executor, async move {
                    let _pending = pending;
                    futures::future::pending::<()>().await;
                })
                .await
            });
            cx.run_until_parked();
            cx.executor().advance_clock(PORTAL_WAIT);
            assert_eq!(task.await, None);
            assert!(dropped.load(Ordering::SeqCst));
        }

        #[test]
        fn continuously_ready_signals_yield_to_cancellation() {
            let message = signal(TEXT_SCALING_FACTOR, OwnedValue::from(1.25));
            let mut changes = futures::stream::repeat(message);
            // A perpetually ready source must still return Pending instead of
            // monopolizing one poll and defeating the enclosing deadline.
            assert!(
                snapshot(&mut changes, futures::future::pending())
                    .now_or_never()
                    .is_none()
            );
        }

        #[test]
        fn snapshots_replace_bursts_and_detect_receiver_shutdown() {
            let (mut sender, mut receiver, latest) = snapshots();
            for index in 0..10_000 {
                assert!(sender.send(DesktopText {
                    rendering: gpui_kit::TextRenderingMode::Grayscale,
                    text_scale: index as f32
                }));
            }
            assert_eq!(latest.lock().unwrap().take().unwrap().text_scale, 9999.0);
            assert!(matches!(receiver.next().now_or_never(), Some(Some(()))));
            assert!(receiver.next().now_or_never().is_none());
            drop(receiver);
            assert!(!sender.send(Observed::default().resolve(true)));
        }

        #[test]
        fn fontconfig_child_output_and_lifetime_are_bounded() {
            assert_eq!(
                command_output(
                    Command::new("sh").args(["-c", "printf 'True|1'"]),
                    Duration::from_secs(1)
                ),
                Some("True|1".into())
            );
            assert_eq!(
                command_output(
                    Command::new("sh").args(["-c", "printf 'True|1'; exit 1"]),
                    Duration::from_secs(1)
                ),
                None
            );
            assert_eq!(
                command_output(
                    Command::new("sh").args(["-c", "while :; do printf '0123456789'; done"]),
                    Duration::from_secs(1)
                ),
                None
            );
            // The direct child replaces its shell, so this also detects a leaked
            // sleeper without creating an unrelated descendant process.
            let pid_file = tempfile::NamedTempFile::new().unwrap();
            let started = Instant::now();
            assert_eq!(
                command_output(
                    Command::new("sh")
                        .args(["-c", "echo $$ > \"$1\"; exec sleep 30", "fixture"])
                        .arg(pid_file.path()),
                    Duration::from_millis(40)
                ),
                None
            );
            assert!(started.elapsed() < Duration::from_secs(2));
            let pid: libc::pid_t = std::fs::read_to_string(pid_file.path())
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            // SAFETY: signal zero checks existence and does not send a signal.
            assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::TextRenderingMode::{Grayscale, Subpixel};

    #[test]
    fn gnome_manual_mode_follows_the_antialiasing_key() {
        assert_eq!(
            rendering_from_gnome(Some("manual"), Some("rgba")),
            Some(Subpixel)
        );
        assert_eq!(
            rendering_from_gnome(Some("manual"), Some("grayscale")),
            Some(Grayscale)
        );
        assert_eq!(
            rendering_from_gnome(Some("manual"), Some("none")),
            Some(Grayscale)
        );
        // Desktops without the GNOME 47 key still publish the low-level one.
        assert_eq!(rendering_from_gnome(None, Some("rgba")), Some(Subpixel));
        assert_eq!(
            rendering_from_gnome(None, Some("grayscale")),
            Some(Grayscale)
        );
    }

    #[test]
    fn gnome_automatic_mode_renders_grayscale_like_gtk4() {
        assert_eq!(
            rendering_from_gnome(Some("automatic"), Some("rgba")),
            Some(Grayscale)
        );
        assert_eq!(
            rendering_from_gnome(Some("automatic"), None),
            Some(Grayscale)
        );
    }

    #[test]
    fn unknown_gnome_values_defer_to_the_next_source() {
        assert_eq!(rendering_from_gnome(None, None), None);
        assert_eq!(rendering_from_gnome(Some("manual"), None), None);
        assert_eq!(rendering_from_gnome(Some("manual"), Some("lcd-v")), None);
        assert_eq!(
            rendering_from_gnome(Some("later"), Some("rgba")),
            Some(Subpixel)
        );
    }

    #[test]
    fn fontconfig_defaults_map_horizontal_stripes_only() {
        assert_eq!(rendering_from_fontconfig("True|1\n"), Some(Subpixel));
        assert_eq!(rendering_from_fontconfig("True|2"), Some(Subpixel));
        assert_eq!(rendering_from_fontconfig("True|3"), Some(Grayscale));
        assert_eq!(rendering_from_fontconfig("True|4"), Some(Grayscale));
        assert_eq!(rendering_from_fontconfig("True|0"), Some(Grayscale));
        assert_eq!(rendering_from_fontconfig("True|5"), Some(Grayscale));
        assert_eq!(rendering_from_fontconfig("False|1"), Some(Grayscale));
        assert_eq!(rendering_from_fontconfig(""), None);
        assert_eq!(rendering_from_fontconfig("Maybe|1"), None);
        assert_eq!(rendering_from_fontconfig("True|rgb"), None);
        assert_eq!(rendering_from_fontconfig("True|9"), None);
        assert_eq!(rendering_from_fontconfig("True|1|5"), None);
    }

    #[test]
    fn resolution_prefers_desktop_then_fontconfig_then_grayscale() {
        let desktop = Observed {
            font_rendering: Some("manual".into()),
            antialiasing: Some("rgba".into()),
            text_scale: Some(1.25),
            fontconfig: Some("True|5".into()),
        };
        assert_eq!(
            desktop.resolve(true),
            DesktopText {
                rendering: Subpixel,
                text_scale: 1.25,
            }
        );
        let kde = Observed {
            fontconfig: Some("True|1".into()),
            ..Observed::default()
        };
        assert_eq!(kde.resolve(true).rendering, Subpixel);
        assert_eq!(Observed::default().resolve(true).rendering, Grayscale);
    }

    #[test]
    fn text_scaling_applies_on_wayland_only_and_stays_bounded() {
        let observed = Observed {
            text_scale: Some(1.5),
            ..Observed::default()
        };
        assert_eq!(observed.resolve(true).text_scale, 1.5);
        // The X11 backend already scales the window through Xft.dpi.
        assert_eq!(observed.resolve(false).text_scale, 1.0);
        let huge = Observed {
            text_scale: Some(40.0),
            ..Observed::default()
        };
        assert_eq!(huge.resolve(true).text_scale, 3.0);
        let broken = Observed {
            text_scale: Some(f64::NAN),
            ..Observed::default()
        };
        assert_eq!(broken.resolve(true).text_scale, 1.0);
        assert_eq!(Observed::default().resolve(true).text_scale, 1.0);
    }
}
