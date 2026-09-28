//! The Omarchy theme (Linux): the desktop's current Omarchy theme as a GitTurtle palette.
//!
//! Omarchy keeps the applied theme under `~/.local/state/omarchy/current/`: `theme/` holds
//! its files, `colors.toml` among them, `theme.name` its name, and `background` links the
//! wallpaper. `omarchy-theme-set` removes `theme/`, moves the next theme into place, and only
//! then rewrites `theme.name`.
//!
//! Nothing here writes or spawns a process. Every read runs on the background executor: once
//! at launch, when a window regains focus, when the theme is selected, and, while it is
//! selected, once the writes of a switch settle under a non-recursive watch of `current/`.
//! [`Omarchy`] is the one owner of what the reads found. The picker card, the Follow system
//! switch and `apply_appearance` all read it, and it changes (notifying its observers) only
//! when what it holds changes, so a reread that finds the same theme applies nothing.

mod palette;

use super::Palette;
use super::custom::{ResolvedTheme, ThemeSelection};
use futures::{StreamExt as _, channel::mpsc, channel::oneshot, future::Either};
use gpui_kit::{App, AsyncApp, Global, Task};
use palette::Mapped;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The file holding the applied theme's name, beside `theme/`.
const NAME_FILE: &str = "theme.name";
/// The directory `omarchy-theme-set` replaces on each switch.
const THEME_DIRECTORY: &str = "theme";
const COLORS_FILE: &str = "colors.toml";
/// `theme.name` holds one short name; anything longer is not one.
const MAX_NAME_FILE_BYTES: usize = 1024;
/// The longest theme name the card shows.
const MAX_NAME_BYTES: usize = 64;
/// A switch's writes settle when this passes without another (the local-refresh quiet period).
pub(crate) const QUIET_PERIOD: Duration = Duration::from_millis(250);
/// A stream of writes that never settles is read after this anyway.
const LONGEST_SETTLE: Duration = Duration::from_secs(2);
/// How long launch waits for the first read before opening the window.
const STARTUP_WAIT: Duration = Duration::from_millis(200);

/// `~/.local/state/omarchy/current`, where Omarchy keeps the applied theme.
fn default_root() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    home.is_absolute()
        .then(|| home.join(".local/state/omarchy/current"))
}

/// What one read of `current/` found.
#[derive(Clone, Debug, PartialEq)]
struct Reading {
    /// `colors.toml` is a regular file, so Settings offers the card.
    available: bool,
    /// The title-cased `theme.name`, when it holds a usable name.
    name: Option<String>,
    /// The mapped palette, or why there is none.
    outcome: Result<Mapped, String>,
}

/// Read, parse and map the theme under `root`, timing it for `gitturtle.omarchy_reread_ms`.
fn read(root: &Path) -> Reading {
    let started = Instant::now();
    let name = read_name(&root.join(NAME_FILE));
    let reading = match read_colors(&root.join(THEME_DIRECTORY).join(COLORS_FILE)) {
        Ok(bytes) => Reading {
            available: true,
            name,
            outcome: palette::parse(&bytes).map(|colors| palette::map(&colors)),
        },
        Err((available, problem)) => Reading {
            available,
            name,
            outcome: Err(problem),
        },
    };
    if crate::trace_enabled() {
        eprintln!(
            "gitturtle.omarchy_reread_ms={:.3}",
            started.elapsed().as_secs_f64() * 1000.
        );
    }
    reading
}

/// Open `path` as a regular file, following links, and read at most `limit` bytes plus one.
/// A directory, FIFO or device is refused before it is opened and again after, and the
/// nonblocking open keeps a FIFO swapped in between from stalling the read.
fn read_bounded(path: &Path, limit: usize) -> std::io::Result<Vec<u8>> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let not_regular = || std::io::Error::other("not a regular file");
    if !std::fs::metadata(path)?.is_file() {
        return Err(not_regular());
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(not_regular());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// `colors.toml`'s bytes, or whether it is still a regular file and why it cannot be used.
fn read_colors(path: &Path) -> Result<Vec<u8>, (bool, String)> {
    match read_bounded(path, palette::MAX_COLORS_BYTES) {
        Ok(bytes) => Ok(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err((false, format!("{COLORS_FILE} is missing")))
        }
        Err(error) => {
            let regular = std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file());
            let problem = if regular {
                format!("{COLORS_FILE} could not be read ({})", error.kind())
            } else {
                format!("{COLORS_FILE} is not a regular file")
            };
            Err((regular, problem))
        }
    }
}

/// `theme.name` title-cased for the card ("tokyo-night" → "Tokyo Night"), bounded to
/// [`MAX_NAME_BYTES`]; `None` when it is missing, empty, too long, not UTF-8 or not one line.
fn read_name(path: &Path) -> Option<String> {
    let bytes = read_bounded(path, MAX_NAME_FILE_BYTES).ok()?;
    if bytes.len() > MAX_NAME_FILE_BYTES {
        return None;
    }
    display_name(std::str::from_utf8(&bytes).ok()?)
}

fn display_name(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.chars().any(char::is_control) {
        return None;
    }
    let mut name = raw
        .split(|character: char| character == '-' || character == '_' || character.is_whitespace())
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut characters = word.chars();
            characters
                .next()
                .map(|first| first.to_uppercase().chain(characters).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ");
    if name.len() > MAX_NAME_BYTES {
        let mut end = MAX_NAME_BYTES;
        while !name.is_char_boundary(end) {
            end -= 1;
        }
        name.truncate(end);
    }
    (!name.is_empty()).then_some(name)
}

/// What the reads found: the one owner the picker card, the Follow system switch and the
/// application path read. It changes only when what it holds does.
pub(crate) struct Omarchy {
    shown: Shown,
    /// Bumped with every change of `shown`.
    revision: u64,
    /// When the read behind the latest change reached the UI thread.
    arrived: Option<Instant>,
}

impl Global for Omarchy {}

#[derive(Clone, Debug, Default, PartialEq)]
struct Shown {
    /// No read has finished yet.
    pending: bool,
    available: bool,
    name: Option<String>,
    problem: Option<String>,
    /// The last palette read successfully this session, and the name read with it.
    last_good: Option<(Mapped, Option<String>)>,
}

impl Shown {
    /// The state after `reading`: its availability, name and problem, and its palette when it
    /// has one. A reading without one keeps the last good palette.
    fn after(&self, reading: Reading) -> Self {
        let (problem, last_good) = match reading.outcome {
            Ok(mapped) => (None, Some((mapped, reading.name.clone()))),
            Err(problem) => (Some(problem), self.last_good.clone()),
        };
        Self {
            pending: false,
            available: reading.available,
            name: reading.name,
            problem,
            last_good,
        }
    }

    fn description(&self) -> String {
        if self.pending {
            return "Reading your Omarchy theme…".into();
        }
        let Some(problem) = &self.problem else {
            return self.name.as_ref().map_or_else(
                || "Follows your Omarchy theme".into(),
                |name| format!("Follows {name}"),
            );
        };
        match &self.last_good {
            None => format!(
                "Unavailable: {problem}. Using {}.",
                super::ThemeChoice::default().label()
            ),
            Some((_, Some(name))) => format!("Unavailable: {problem}. Keeping {name}."),
            Some((_, None)) => format!("Unavailable: {problem}. Keeping the last colors read."),
        }
    }
}

/// The machinery behind [`Omarchy`]: the reread queue and the watch.
struct Follower {
    root: PathBuf,
    /// The latest reread started; a result from an older one is dropped.
    generation: u64,
    requests: mpsc::UnboundedSender<Request>,
    following: bool,
    watch: Watch,
    /// Bumped whenever following starts or stops, so a watcher that finishes starting after
    /// that is dropped.
    epoch: u64,
    /// Launch's wait for the first read.
    ready: Option<oneshot::Sender<()>>,
    _rereads: Task<()>,
    /// Test-only: the paths of every event the watcher delivered, whether or not it asked
    /// for a reread, in delivery order.
    #[cfg(test)]
    delivered: Delivered,
}

impl Global for Follower {}

#[cfg(test)]
type Delivered = std::sync::Arc<std::sync::Mutex<Vec<PathBuf>>>;

enum Watch {
    Off,
    Starting,
    On(#[allow(dead_code, reason = "held so the watch lasts")] notify::RecommendedWatcher),
    /// `current/` could not be watched; focus return tries again.
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Request {
    /// Read once the writes under `current/` settle.
    Settled,
    /// Read now: launch, focus return, or the theme was just selected.
    Now,
}

/// Launch's wait for the first read (see [`ready`]).
pub(crate) struct Initial(Option<oneshot::Receiver<()>>);

/// Read the desktop's Omarchy theme and follow it while it is `selected`.
pub(crate) fn start(selected: bool, cx: &mut App) -> Initial {
    let initial = install(default_root(), cx);
    follow(selected, cx);
    initial
}

/// Launch waits this long at most for the first read, so the first frame already shows a
/// selected Omarchy theme.
pub(crate) async fn ready(initial: Initial, cx: &AsyncApp) {
    if let Some(first) = initial.0 {
        let deadline = cx.background_executor().timer(STARTUP_WAIT);
        futures::future::select(first, deadline).await;
    }
}

/// Own the state for `root` (the directory standing in for `current/`) and start its first
/// read. Without a root nothing is read and no card is offered.
pub(crate) fn install(root: Option<PathBuf>, cx: &mut App) -> Initial {
    let Some(root) = root else {
        return Initial(None);
    };
    let (requests, receiver) = mpsc::unbounded();
    let (ready, first) = oneshot::channel();
    let _ = requests.unbounded_send(Request::Now);
    let rereads = cx.spawn(async move |cx| reread(receiver, cx).await);
    cx.set_global(Follower {
        root,
        generation: 0,
        requests,
        following: false,
        watch: Watch::Off,
        epoch: 0,
        ready: Some(ready),
        _rereads: rereads,
        #[cfg(test)]
        delivered: Default::default(),
    });
    cx.set_global(Omarchy {
        shown: Shown {
            pending: true,
            ..Shown::default()
        },
        revision: 0,
        arrived: None,
    });
    Initial(Some(first))
}

/// One reread at a time: a watched write waits for the quiet period (or a `Now`), and
/// everything queued meanwhile joins that reread.
async fn reread(mut requests: mpsc::UnboundedReceiver<Request>, cx: &mut AsyncApp) {
    while let Some(request) = requests.next().await {
        if request == Request::Settled {
            let executor = cx.background_executor().clone();
            let first = executor.now();
            loop {
                let quiet = executor.timer(QUIET_PERIOD);
                match futures::future::select(requests.next(), quiet).await {
                    Either::Left((None, _)) => return,
                    Either::Left((Some(Request::Settled), _))
                        if executor.now().duration_since(first) < LONGEST_SETTLE => {}
                    Either::Left(_) | Either::Right(_) => break,
                }
            }
        }
        while requests.try_recv().is_ok() {}
        let Some((generation, root)) = cx.update(begin) else {
            return;
        };
        let reading = cx
            .background_executor()
            .spawn(async move { read(&root) })
            .await;
        cx.update(|cx| finish(generation, reading, cx));
    }
}

/// Start a reread: its generation and the root to read.
fn begin(cx: &mut App) -> Option<(u64, PathBuf)> {
    cx.has_global::<Follower>().then(|| {
        let follower = cx.global_mut::<Follower>();
        follower.generation += 1;
        (follower.generation, follower.root.clone())
    })
}

/// Take a reread's result. A result from an older reread than the latest one started is
/// dropped, and one that changes nothing leaves [`Omarchy`] untouched.
fn finish(generation: u64, reading: Reading, cx: &mut App) {
    let Some(follower) = cx.try_global::<Follower>() else {
        return;
    };
    if follower.generation != generation {
        return;
    }
    if let Some(ready) = cx.global_mut::<Follower>().ready.take() {
        let _ = ready.send(());
    }
    let Some(state) = cx.try_global::<Omarchy>() else {
        return;
    };
    let shown = state.shown.after(reading);
    if shown == state.shown {
        return;
    }
    let state = cx.global_mut::<Omarchy>();
    state.shown = shown;
    state.revision += 1;
    state.arrived = Some(Instant::now());
}

/// Watch `current/` while the Omarchy theme is selected, and stop when another is. Selecting
/// it also reads it again, so a theme changed while it was not followed is picked up.
pub(crate) fn follow(selected: bool, cx: &mut App) {
    let Some(follower) = cx.try_global::<Follower>() else {
        return;
    };
    if follower.following == selected {
        return;
    }
    let follower = cx.global_mut::<Follower>();
    follower.following = selected;
    follower.epoch += 1;
    follower.watch = Watch::Off;
    if selected {
        let _ = follower.requests.unbounded_send(Request::Now);
        start_watch(cx);
    }
}

/// A window regained focus: read again, and retry a watch that could not start.
pub(crate) fn refresh(cx: &mut App) {
    let Some(follower) = cx.try_global::<Follower>() else {
        return;
    };
    let _ = follower.requests.unbounded_send(Request::Now);
    if follower.following && matches!(follower.watch, Watch::Failed) {
        start_watch(cx);
    }
}

/// Create the watcher on the background executor: setting it up touches the filesystem.
fn start_watch(cx: &mut App) {
    let follower = cx.global_mut::<Follower>();
    follower.watch = Watch::Starting;
    let epoch = follower.epoch;
    let root = follower.root.clone();
    let requests = follower.requests.clone();
    #[cfg(test)]
    let delivered = follower.delivered.clone();
    let starting = cx.background_executor().spawn(async move {
        watch(
            root,
            requests,
            #[cfg(test)]
            delivered,
        )
    });
    cx.spawn(async move |cx| {
        let watcher = starting.await;
        cx.update(|cx| {
            if !cx.has_global::<Follower>() {
                return;
            }
            let follower = cx.global_mut::<Follower>();
            if follower.following && follower.epoch == epoch {
                follower.watch = watcher.map_or(Watch::Failed, Watch::On);
            }
        });
    })
    .detach();
}

fn watch(
    root: PathBuf,
    requests: mpsc::UnboundedSender<Request>,
    #[cfg(test)] delivered: Delivered,
) -> notify::Result<notify::RecommendedWatcher> {
    use notify::Watcher as _;
    let watched = root.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let asks = match &event {
            Ok(event) => settles(&watched, event),
            // An error may have lost events: read again to be sure.
            Err(_) => true,
        };
        if asks {
            let _ = requests.unbounded_send(Request::Settled);
        }
        // Recorded after the request is queued, so a test that sees the path knows it is.
        #[cfg(test)]
        if let Ok(event) = &event {
            delivered
                .lock()
                .unwrap()
                .extend(event.paths.iter().cloned());
        }
    })?;
    watcher.watch(&root, notify::RecursiveMode::NonRecursive)?;
    Ok(watcher)
}

/// Whether an event in `current/` can change the theme: one on `theme/`, which a switch
/// replaces, or on `theme.name`, which it writes last. `background`, the staging `next-theme`
/// and reads cannot.
fn settles(root: &Path, event: &notify::Event) -> bool {
    if matches!(event.kind, notify::EventKind::Access(_)) {
        return false;
    }
    event.need_rescan()
        || event.paths.iter().any(|path| {
            path.parent() == Some(root)
                && path
                    .file_name()
                    .is_some_and(|name| name == THEME_DIRECTORY || name == NAME_FILE)
        })
}

/// The Omarchy theme's palette for a resolved Omarchy selection, once a read has found one;
/// any other selection unchanged.
pub(crate) fn resolve(resolved: ResolvedTheme, cx: &App) -> ResolvedTheme {
    if resolved.selection != ThemeSelection::Omarchy {
        return resolved;
    }
    match cx
        .try_global::<Omarchy>()
        .and_then(|state| state.shown.last_good.as_ref())
    {
        Some((mapped, _)) => ResolvedTheme {
            palette: mapped.palette,
            is_light: mapped.is_light,
            ..resolved
        },
        None => resolved,
    }
}

/// What the Settings picker shows for the Omarchy theme.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Card {
    /// `colors.toml` is a regular file: the card is offered even when not selected.
    pub offered: bool,
    /// The palette its miniature draws: the one the theme applies.
    pub palette: Palette,
    pub description: String,
    pub revision: u64,
    pub arrived: Option<Instant>,
}

/// The card, or `None` when nothing is read on this desktop.
pub(crate) fn card(cx: &App) -> Option<Card> {
    let state = cx.try_global::<Omarchy>()?;
    Some(Card {
        offered: state.shown.available,
        palette: state.shown.last_good.as_ref().map_or_else(
            || super::ThemeChoice::default().palette(),
            |(mapped, _)| mapped.palette,
        ),
        description: state.shown.description(),
        revision: state.revision,
        arrived: state.arrived,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appearance::ThemeChoice;
    use gpui_kit::{self as gpui, AppContext as _, Entity, TestAppContext, VisualTestContext};
    use std::cell::Cell;
    use std::rc::Rc;

    const TOKYO_NIGHT: &[u8] = include_bytes!("../../tests/fixtures/omarchy/tokyo-night.toml");
    const WHITE: &[u8] = include_bytes!("../../tests/fixtures/omarchy/white.toml");
    const LATTE: &[u8] = include_bytes!("../../tests/fixtures/omarchy/catppuccin-latte.toml");

    fn mapped(bytes: &[u8]) -> Mapped {
        palette::map(&palette::parse(bytes).unwrap())
    }

    /// A stand-in for `current/`: `theme/colors.toml`, `theme.name` and a `background` link.
    fn current(parent: &Path, colors: &[u8], name: &str) -> PathBuf {
        let root = parent.join("current");
        std::fs::create_dir_all(root.join(THEME_DIRECTORY).join("backgrounds")).unwrap();
        std::fs::write(root.join(THEME_DIRECTORY).join(COLORS_FILE), colors).unwrap();
        std::fs::write(root.join(NAME_FILE), format!("{name}\n")).unwrap();
        std::os::unix::fs::symlink(
            root.join(THEME_DIRECTORY).join("backgrounds"),
            root.join("background"),
        )
        .unwrap();
        root
    }

    #[test]
    fn theme_names_are_title_cased_and_bounded() {
        assert_eq!(
            display_name("tokyo-night\n").as_deref(),
            Some("Tokyo Night")
        );
        assert_eq!(
            display_name("catppuccin-latte").as_deref(),
            Some("Catppuccin Latte")
        );
        assert_eq!(display_name("  retro-82 ").as_deref(), Some("Retro 82"));
        assert_eq!(display_name("rosé_pine").as_deref(), Some("Rosé Pine"));
        for unusable in ["", "  \n", "tokyo\nnight", "tokyo\u{7}night", "-_-"] {
            assert_eq!(display_name(unusable), None, "{unusable:?}");
        }
        let long = display_name(&"a".repeat(200)).unwrap();
        assert_eq!(long.len(), MAX_NAME_BYTES);
        // Bounded on a character boundary.
        let accented = display_name(&"é".repeat(40)).unwrap();
        assert!(accented.len() <= MAX_NAME_BYTES && accented.starts_with('É'));
    }

    /// A readable theme is offered with its name; a missing, special, oversized, malformed or
    /// unnamed one says why or falls back to the plain caption, without blocking on a FIFO.
    #[test]
    fn reads_report_what_they_found() {
        let fixture = tempfile::tempdir().unwrap();
        let root = current(fixture.path(), TOKYO_NIGHT, "tokyo-night");
        let colors = root.join(THEME_DIRECTORY).join(COLORS_FILE);
        assert_eq!(
            read(&root),
            Reading {
                available: true,
                name: Some("Tokyo Night".into()),
                outcome: Ok(mapped(TOKYO_NIGHT)),
            }
        );

        // A linked colors.toml is followed.
        let elsewhere = fixture.path().join("white.toml");
        std::fs::write(&elsewhere, WHITE).unwrap();
        std::fs::remove_file(&colors).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &colors).unwrap();
        assert_eq!(read(&root).outcome, Ok(mapped(WHITE)));
        std::fs::remove_file(&colors).unwrap();

        let unavailable = |problem: &str| (false, Err::<Mapped, String>(problem.into()));
        let seen = |root: &Path| {
            let reading = read(root);
            (reading.available, reading.outcome)
        };
        assert_eq!(seen(&root), unavailable("colors.toml is missing"));
        std::fs::create_dir(&colors).unwrap();
        assert_eq!(
            seen(&root),
            unavailable("colors.toml is not a regular file")
        );
        std::fs::remove_dir(&colors).unwrap();
        let fifo = std::ffi::CString::new(colors.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert_eq!(
            seen(&root),
            unavailable("colors.toml is not a regular file")
        );
        std::fs::remove_file(&colors).unwrap();

        let mut oversized = TOKYO_NIGHT.to_vec();
        oversized.resize(palette::MAX_COLORS_BYTES + 4096, b'\n');
        std::fs::write(&colors, &oversized).unwrap();
        assert_eq!(
            seen(&root),
            (true, Err("colors.toml is larger than 16 KiB".into()))
        );
        std::fs::write(&colors, "[colors]\nbackground = \"#000000\"\n").unwrap();
        assert_eq!(
            seen(&root),
            (
                true,
                Err("colors.toml is missing a background color".into())
            )
        );

        std::fs::write(&colors, TOKYO_NIGHT).unwrap();
        for unusable in [&b""[..], b"\xff\xfe", &[b'a'; MAX_NAME_FILE_BYTES + 1]] {
            std::fs::write(root.join(NAME_FILE), unusable).unwrap();
            assert_eq!(read(&root).name, None);
        }
        std::fs::remove_file(root.join(NAME_FILE)).unwrap();
        let reading = read(&root);
        assert_eq!(reading.name, None);
        assert_eq!(
            Shown::default().after(reading).description(),
            "Follows your Omarchy theme"
        );
    }

    /// A failed read keeps the last good palette for the session and says why; without one,
    /// the default theme stands in.
    #[test]
    fn a_failed_read_keeps_the_last_good_palette_and_says_why() {
        let pending = Shown {
            pending: true,
            ..Shown::default()
        };
        assert_eq!(pending.description(), "Reading your Omarchy theme…");
        let problem = || Reading {
            available: true,
            name: Some("Broken".into()),
            outcome: Err("colors.toml is missing a background color".into()),
        };
        let first = pending.after(problem());
        assert_eq!(first.last_good, None);
        assert_eq!(
            first.description(),
            "Unavailable: colors.toml is missing a background color. Using Midnight."
        );
        let good = first.after(Reading {
            available: true,
            name: Some("Tokyo Night".into()),
            outcome: Ok(mapped(TOKYO_NIGHT)),
        });
        assert_eq!(good.description(), "Follows Tokyo Night");
        let broken = good.after(problem());
        assert_eq!(
            broken.last_good,
            Some((mapped(TOKYO_NIGHT), Some("Tokyo Night".into())))
        );
        assert_eq!(
            broken.description(),
            "Unavailable: colors.toml is missing a background color. Keeping Tokyo Night."
        );
    }

    /// Of two rereads, the one started last wins even when the older result arrives after
    /// it, and a reread that finds what is already held changes nothing.
    #[gpui::test]
    fn a_stale_reread_never_replaces_a_newer_one(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let root = current(fixture.path(), TOKYO_NIGHT, "tokyo-night");
        cx.update(|cx| drop(install(Some(root), cx)));
        cx.run_until_parked();
        let revision = || cx.read(|cx| card(cx).unwrap().revision);
        assert_eq!(revision(), 1, "the first read finished");
        let reading = |bytes: &[u8], name: &str| Reading {
            available: true,
            name: Some(name.into()),
            outcome: Ok(mapped(bytes)),
        };

        let (older, _) = cx.update(begin).unwrap();
        let (newer, _) = cx.update(begin).unwrap();
        cx.update(|cx| finish(newer, reading(WHITE, "White"), cx));
        cx.update(|cx| finish(older, reading(LATTE, "Catppuccin Latte"), cx));
        let shown = cx.read(|cx| card(cx).unwrap());
        assert_eq!(shown.palette, mapped(WHITE).palette);
        assert_eq!(shown.description, "Follows White");
        assert_eq!(shown.revision, 2);

        cx.update(|cx| finish(newer, reading(WHITE, "White"), cx));
        assert_eq!(revision(), 2, "the same theme again changes nothing");
    }

    fn open_app(cx: &mut TestAppContext) -> (Entity<crate::GitTurtle>, &mut VisualTestContext) {
        // GitTurtle::new starts a real preferences worker, and the watcher delivers from
        // its own thread.
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::image_lifetime::init(cx);
            crate::theme_editor::init(cx);
        });
        let captured: Rc<std::cell::RefCell<Option<Entity<crate::GitTurtle>>>> = Rc::default();
        let output = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                crate::GitTurtle::new(
                    None,
                    crate::Preferences::default(),
                    crate::repository_tabs::Session::default(),
                    crate::activity::State::default(),
                    crate::recovery_drafts::State::default(),
                    window,
                    cx,
                )
            });
            *output.borrow_mut() = Some(app.clone());
            gpui_kit::component::Root::new(app, window, cx)
        });
        let app = captured.borrow().clone().unwrap();
        (app, cx)
    }

    /// Wait (in real time, bounded) until the watcher has delivered an event for `path`.
    /// Inotify keeps a watch's events in order, so every earlier write's event, and the
    /// reread it asked for, is queued by then.
    fn delivered(cx: &mut VisualTestContext, path: &Path) {
        for _ in 0..500 {
            let seen = cx.read(|cx| {
                cx.global::<Follower>()
                    .delivered
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|delivered| delivered == path)
            });
            if seen {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the watcher delivered no event for {path:?}");
    }

    /// What `omarchy-theme-set` does: stage the next theme, remove `theme/`, move the staged
    /// one into place, and only then rewrite `theme.name`; then an unrelated sentinel write
    /// the test waits for.
    fn switch(root: &Path, colors: &[u8], name: &str, sentinel: &str) {
        let next = root.join("next-theme");
        std::fs::create_dir_all(next.join("backgrounds")).unwrap();
        std::fs::write(next.join(COLORS_FILE), colors).unwrap();
        std::fs::remove_dir_all(root.join(THEME_DIRECTORY)).unwrap();
        std::fs::rename(&next, root.join(THEME_DIRECTORY)).unwrap();
        std::fs::write(root.join(NAME_FILE), format!("{name}\n")).unwrap();
        std::fs::write(root.join(sentinel), b"").unwrap();
    }

    /// While selected, a theme switch applies its palette once, after the writes settle; a
    /// change of wallpaper applies nothing and reads nothing; rewriting the same theme reads
    /// it again and applies nothing; and choosing another theme stops the watch.
    #[gpui::test]
    fn a_selected_theme_follows_each_switch_once(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let root = current(fixture.path(), TOKYO_NIGHT, "tokyo-night");
        cx.update(|cx| drop(install(Some(root.clone()), cx)));
        let (app, cx) = open_app(cx);
        cx.run_until_parked();
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.choose_theme(ThemeSelection::Omarchy, window, cx)
            })
        });
        cx.run_until_parked();
        assert!(cx.read(|cx| matches!(cx.global::<Follower>().watch, Watch::On(_))));
        assert_eq!(
            cx.read(|cx| *cx.global::<Palette>()),
            mapped(TOKYO_NIGHT).palette
        );
        let applied = Rc::new(Cell::new(0));
        let count = applied.clone();
        cx.update(|_, cx| {
            cx.observe_global::<Palette>(move |_| count.set(count.get() + 1))
                .detach()
        });
        let generation =
            |cx: &mut VisualTestContext| cx.read(|cx| cx.global::<Follower>().generation);

        switch(&root, WHITE, "white", "switched");
        delivered(cx, &root.join("switched"));
        cx.run_until_parked();
        assert_eq!(applied.get(), 0, "nothing applies before the writes settle");
        cx.executor().advance_clock(QUIET_PERIOD);
        cx.run_until_parked();
        assert_eq!(applied.get(), 1, "the switch applies once");
        assert_eq!(cx.read(|cx| *cx.global::<Palette>()), mapped(WHITE).palette);
        assert_eq!(cx.read(|cx| card(cx).unwrap().description), "Follows White");

        let read_so_far = generation(cx);
        std::fs::remove_file(root.join("background")).unwrap();
        std::os::unix::fs::symlink(root.join("switched"), root.join("background")).unwrap();
        std::fs::write(root.join("wallpaper-changed"), b"").unwrap();
        delivered(cx, &root.join("wallpaper-changed"));
        cx.run_until_parked();
        cx.executor().advance_clock(QUIET_PERIOD * 2);
        cx.run_until_parked();
        assert_eq!(applied.get(), 1, "a wallpaper change applies nothing");
        assert_eq!(generation(cx), read_so_far, "and reads nothing");

        std::fs::write(root.join(NAME_FILE), "white\n").unwrap();
        std::fs::write(root.join("rewritten"), b"").unwrap();
        delivered(cx, &root.join("rewritten"));
        cx.executor().advance_clock(QUIET_PERIOD);
        cx.run_until_parked();
        assert_eq!(generation(cx), read_so_far + 1, "the rewrite is read");
        assert_eq!(applied.get(), 1, "the same theme applies nothing");

        switch(&root, LATTE, "catppuccin-latte", "switched-again");
        delivered(cx, &root.join("switched-again"));
        cx.executor().advance_clock(QUIET_PERIOD);
        cx.run_until_parked();
        assert_eq!(applied.get(), 2, "the next switch applies once more");
        assert_eq!(cx.read(|cx| *cx.global::<Palette>()), mapped(LATTE).palette);

        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.choose_theme(ThemeSelection::BuiltIn(ThemeChoice::Nord), window, cx)
            })
        });
        cx.run_until_parked();
        assert!(cx.read(|cx| matches!(cx.global::<Follower>().watch, Watch::Off)));
    }
}
