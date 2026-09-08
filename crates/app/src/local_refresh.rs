//! Native filesystem notifications, coalesced into one pending local refresh.
//! Call `watch` off UI; callbacks enqueue only. The actor inspects changed
//! paths without reading Git objects, invoking Git, or polling trees.

use anyhow::{Context, Result, ensure};
use futures::{Stream, channel::mpsc as async_mpsc};
use notify::{
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{MetadataKind, ModifyKind},
};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    task::{Context as PollContext, Poll},
    time::{Duration, Instant, SystemTime},
};

const QUIET_PERIOD: Duration = Duration::from_millis(250);
const MAX_DELAY: Duration = Duration::from_secs(2);
const MAX_EVENTS: usize = 64;
const MAX_EVENT_PATHS: usize = 16;
const MAX_FINGERPRINTS: usize = 4096;
const MAX_ERROR_BYTES: usize = 2048;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchRoots {
    pub worktree: PathBuf,
    pub private_git: PathBuf,
    pub common_git: PathBuf,
}

impl WatchRoots {
    fn normalize(self) -> Result<Self> {
        fn directory(path: &Path) -> Result<PathBuf> {
            ensure!(path.is_absolute(), "Watch roots must be absolute");
            let canonical = path
                .canonicalize()
                .context("Resolve local watch directory")?;
            ensure!(
                fs::metadata(&canonical)?.is_dir(),
                "Watch root is not a directory"
            );
            Ok(canonical)
        }
        Ok(Self {
            worktree: directory(&self.worktree)?,
            private_git: directory(&self.private_git)?,
            common_git: directory(&self.common_git)?,
        })
    }

    fn watched_roots(&self) -> Vec<&Path> {
        let mut roots = vec![
            self.worktree.as_path(),
            self.private_git.as_path(),
            self.common_git.as_path(),
        ];
        roots.sort_by_key(|path| path.components().count());
        let mut result: Vec<&Path> = Vec::new();
        for root in roots {
            if !result.iter().any(|parent| root.starts_with(parent)) {
                result.push(root);
            }
        }
        result
    }

    fn classify(&self, path: &Path) -> ChangeKind {
        // Private directories are checked first because they may live inside
        // the common Git directory's worktrees/ administration tree.
        for root in [&self.private_git, &self.common_git] {
            if let Ok(relative) = path.strip_prefix(root) {
                if relative.as_os_str().is_empty() {
                    return ChangeKind::GitRoot;
                }
                if relative
                    .components()
                    .any(|part| part.as_os_str().as_encoded_bytes().ends_with(b".lock"))
                {
                    return ChangeKind::Ignore;
                }
                let first = relative.components().next().unwrap().as_os_str();
                let relevant = [
                    "HEAD",
                    "index",
                    "config",
                    "config.worktree",
                    "packed-refs",
                    "shallow",
                    "commondir",
                    "gitdir",
                    "locked",
                    "ORIG_HEAD",
                    "FETCH_HEAD",
                    "MERGE_HEAD",
                    "MERGE_MSG",
                    "MERGE_MODE",
                    "AUTO_MERGE",
                    "CHERRY_PICK_HEAD",
                    "REVERT_HEAD",
                    "BISECT_LOG",
                    "BISECT_START",
                    "refs",
                    "objects",
                    "worktrees",
                    "rebase-merge",
                    "rebase-apply",
                    "sequencer",
                    "info",
                ]
                .iter()
                .any(|name| first == *name)
                    || relative.starts_with("logs/refs/stash");
                return if relevant {
                    ChangeKind::Git
                } else {
                    ChangeKind::Ignore
                };
            }
        }
        if path == self.worktree.join(".git") {
            ChangeKind::Git
        } else if path.starts_with(&self.worktree) {
            ChangeKind::Worktree
        } else {
            ChangeKind::Ignore
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalChange {
    pub worktree: bool,
    pub git: bool,
    /// Lost events or a watch failure: read current local state without
    /// guessing which paths changed.
    pub rescan: bool,
    pub error: Option<String>,
}

impl LocalChange {
    pub(crate) fn merge(&mut self, next: Self) {
        self.worktree |= next.worktree;
        self.git |= next.git;
        self.rescan |= next.rescan;
        if next.error.is_some() {
            self.error = next.error;
        }
    }
    pub(crate) fn is_empty(&self) -> bool {
        !self.worktree && !self.git && !self.rescan && self.error.is_none()
    }
}

/// Drop wakes the actor; native watch teardown stays off the UI thread.
pub struct LocalWatcher {
    stop: Arc<AtomicBool>,
    sender: mpsc::SyncSender<Message>,
}

impl Drop for LocalWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.sender.try_send(Message::Wake);
    }
}

/// One merged event slot and one wake token bound delivery while UI is busy.
pub struct LocalChanges {
    wake: async_mpsc::Receiver<()>,
    pending: Arc<Mutex<Option<LocalChange>>>,
}

impl Stream for LocalChanges {
    type Item = LocalChange;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut PollContext<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.wake).poll_next(cx) {
            Poll::Ready(Some(())) => Poll::Ready(
                self.pending
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take(),
            ),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

struct Delivery {
    wake: async_mpsc::Sender<()>,
    pending: Arc<Mutex<Option<LocalChange>>>,
}

impl Delivery {
    fn channel() -> (Self, LocalChanges) {
        let (sender, receiver) = async_mpsc::channel(1);
        let pending = Arc::new(Mutex::new(None));
        (
            Self {
                wake: sender,
                pending: Arc::clone(&pending),
            },
            LocalChanges {
                wake: receiver,
                pending,
            },
        )
    }
    fn send(&mut self, change: LocalChange) -> bool {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(current) = pending.as_mut() {
            current.merge(change);
            !self.wake.is_closed()
        } else {
            *pending = Some(change);
            self.wake.try_send(()).is_ok()
        }
    }
}

enum Message {
    Event(Event),
    Wake,
}
#[derive(Default)]
struct Signals {
    overflow: AtomicBool,
    error: Mutex<Option<String>>,
}

fn bounded_error(error: impl std::fmt::Display) -> String {
    let mut message = error.to_string();
    if message.len() > MAX_ERROR_BYTES {
        let mut end = MAX_ERROR_BYTES;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
        message.push('…');
    }
    message
}

fn passive_event(event: &Event) -> bool {
    matches!(
        event.kind,
        EventKind::Access(_) | EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime))
    )
}

/// Setup performs local I/O and must run off UI. Git discovery supplies real
/// private/common directory paths rather than guessing from `.git` layout.
pub fn watch(roots: WatchRoots) -> Result<(LocalWatcher, LocalChanges)> {
    let roots = roots.normalize()?;
    let (sender, receiver) = mpsc::sync_channel(MAX_EVENTS);
    let signals = Arc::new(Signals::default());
    let callback_sender = sender.clone();
    let callback_signals = Arc::clone(&signals);
    let mut watcher =
        notify::recommended_watcher(move |event: notify::Result<Event>| match event {
            Ok(mut event) => {
                if passive_event(&event) && !event.need_rescan() {
                    return;
                }
                if event.paths.len() > MAX_EVENT_PATHS {
                    event.paths.truncate(MAX_EVENT_PATHS);
                    callback_signals.overflow.store(true, Ordering::Release);
                }
                if let Err(mpsc::TrySendError::Full(_)) =
                    callback_sender.try_send(Message::Event(event))
                {
                    callback_signals.overflow.store(true, Ordering::Release);
                    // The actor may drain the full queue before this store.
                    // Wake it again so overflow cannot wait for another edit.
                    let _ = callback_sender.try_send(Message::Wake);
                }
            }
            Err(error) => {
                *callback_signals
                    .error
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(bounded_error(error));
                callback_signals.overflow.store(true, Ordering::Release);
                let _ = callback_sender.try_send(Message::Wake);
            }
        })
        .context("Create native local-change watcher")?;
    let mut registrations = Registrations::default();
    for path in roots.watched_roots() {
        registrations
            .add(&mut watcher, path)
            .with_context(|| format!("Watch {}", path.display()))?;
    }
    let fingerprints = Fingerprints::new(&roots);
    let (delivery, changes) = Delivery::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let actor_stop = Arc::clone(&stop);
    std::thread::Builder::new()
        .name("gitturtle-local-changes".into())
        .spawn(move || {
            run_actor(
                watcher,
                registrations,
                roots,
                fingerprints,
                receiver,
                signals,
                actor_stop,
                delivery,
            )
        })
        .context("Start local-change watcher")?;
    Ok((LocalWatcher { stop, sender }, changes))
}

#[derive(Default)]
struct Burst {
    first: Option<Instant>,
    last: Option<Instant>,
    change: LocalChange,
}

impl Burst {
    fn push(&mut self, change: LocalChange, now: Instant) {
        if change.is_empty() {
            return;
        }
        self.first.get_or_insert(now);
        self.last = Some(now);
        self.change.merge(change);
    }
    fn deadline(&self) -> Option<Instant> {
        Some((self.last? + QUIET_PERIOD).min(self.first? + MAX_DELAY))
    }
    fn take_due(&mut self, now: Instant) -> Option<LocalChange> {
        if self.deadline().is_none_or(|deadline| deadline > now) {
            return None;
        }
        self.first = None;
        self.last = None;
        Some(std::mem::take(&mut self.change))
    }
}

#[allow(clippy::too_many_arguments)]
fn run_actor(
    mut watcher: RecommendedWatcher,
    mut registrations: Registrations,
    roots: WatchRoots,
    mut fingerprints: Fingerprints,
    receiver: mpsc::Receiver<Message>,
    signals: Arc<Signals>,
    stop: Arc<AtomicBool>,
    mut delivery: Delivery,
) {
    let mut burst = Burst::default();
    while !stop.load(Ordering::Acquire) {
        let message = match burst.deadline() {
            Some(deadline) => {
                receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            }
            None => receiver
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected),
        };
        if stop.load(Ordering::Acquire) {
            break;
        }
        let mut change = LocalChange::default();
        match message {
            Ok(Message::Event(event)) => {
                change.rescan |= event.need_rescan();
                if let Err(error) = registrations.update(&mut watcher, &event) {
                    change.rescan = true;
                    change.error = Some(bounded_error(error));
                }
                if !passive_event(&event) {
                    for path in &event.paths {
                        let kind = roots.classify(path);
                        if kind != ChangeKind::Ignore && fingerprints.changed(path, kind) {
                            if kind == ChangeKind::Worktree {
                                change.worktree = true;
                            } else {
                                change.git = true;
                            }
                        }
                    }
                }
            }
            Ok(Message::Wake) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        change.rescan |= signals.overflow.swap(false, Ordering::AcqRel);
        if let Some(error) = signals
            .error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            change.error = Some(error);
        }
        burst.push(change, Instant::now());
        if let Some(change) = burst.take_due(Instant::now())
            && !delivery.send(change)
        {
            break;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChangeKind {
    Ignore,
    Worktree,
    Git,
    GitRoot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Fingerprint {
    Missing,
    Unavailable(std::io::ErrorKind),
    File {
        length: u64,
        modified: Option<SystemTime>,
        directory: bool,
        symlink: bool,
        readonly: bool,
        #[cfg(unix)]
        unix: (u64, u64, u32, i64, i64),
    },
    GitRoot(Vec<Fingerprint>),
}

const ROOT_METADATA: &[&str] = &[
    "HEAD",
    "index",
    "config",
    "config.worktree",
    "packed-refs",
    "shallow",
    "commondir",
    "gitdir",
    "locked",
    "ORIG_HEAD",
    "FETCH_HEAD",
    "MERGE_HEAD",
    "MERGE_MSG",
    "AUTO_MERGE",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "rebase-merge",
    "rebase-apply",
    "sequencer",
    "refs",
    "info",
];

fn fingerprint(path: &Path) -> Fingerprint {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Fingerprint::File {
            length: metadata.len(),
            modified: metadata.modified().ok(),
            directory: metadata.is_dir(),
            symlink: metadata.is_symlink(),
            readonly: metadata.permissions().readonly(),
            #[cfg(unix)]
            unix: {
                use std::os::unix::fs::MetadataExt;
                (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.mode(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                )
            },
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Fingerprint::Missing,
        Err(error) => Fingerprint::Unavailable(error.kind()),
    }
}

fn root_fingerprint(path: &Path) -> Fingerprint {
    Fingerprint::GitRoot(
        ROOT_METADATA
            .iter()
            .map(|name| fingerprint(&path.join(name)))
            .collect(),
    )
}

struct Fingerprints {
    values: HashMap<PathBuf, Fingerprint>,
    order: VecDeque<PathBuf>,
}

impl Fingerprints {
    fn new(roots: &WatchRoots) -> Self {
        let mut values = Self {
            values: HashMap::new(),
            order: VecDeque::new(),
        };
        for root in [&roots.private_git, &roots.common_git] {
            values.insert(root.clone(), root_fingerprint(root));
            for name in ROOT_METADATA {
                let path = root.join(name);
                values.insert(path.clone(), fingerprint(&path));
            }
        }
        values
    }
    fn changed(&mut self, path: &Path, kind: ChangeKind) -> bool {
        let next = if kind == ChangeKind::GitRoot {
            root_fingerprint(path)
        } else {
            fingerprint(path)
        };
        if self.values.get(path) == Some(&next) {
            return false;
        }
        self.insert(path.to_owned(), next);
        true
    }
    fn insert(&mut self, path: PathBuf, next: Fingerprint) {
        if !self.values.contains_key(&path) {
            if self.values.len() >= MAX_FINGERPRINTS
                && let Some(oldest) = self.order.pop_front()
            {
                self.values.remove(&oldest);
            }
            self.order.push_back(path.clone());
        }
        self.values.insert(path, next);
    }
}

#[derive(Default)]
struct Registrations {
    #[cfg(target_os = "linux")]
    directories: std::collections::HashSet<PathBuf>,
}

impl Registrations {
    #[cfg(not(target_os = "linux"))]
    fn add(&mut self, watcher: &mut RecommendedWatcher, path: &Path) -> Result<()> {
        watcher.watch(path, RecursiveMode::Recursive)?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn add(&mut self, watcher: &mut RecommendedWatcher, path: &Path) -> Result<()> {
        // notify7's recursive inotify setup follows directory symlinks. Use
        // bounded nonrecursive registrations and event-driven subtree adds.
        let mut pending = vec![path.to_owned()];
        while let Some(path) = pending.pop() {
            let metadata = fs::symlink_metadata(&path)?;
            if !metadata.is_dir() || metadata.is_symlink() || self.directories.contains(&path) {
                continue;
            }
            ensure!(
                self.directories.len() < 16_384,
                "Automatic refresh reached the local directory-watch limit. Use Refresh to check all current changes."
            );
            watcher.watch(&path, RecursiveMode::NonRecursive)?;
            self.directories.insert(path.clone());
            for entry in fs::read_dir(&path)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    ensure!(
                        pending.len() + self.directories.len() < 16_384,
                        "Automatic refresh reached the local directory-watch limit. Use Refresh to check all current changes."
                    );
                    pending.push(entry.path());
                }
            }
        }
        Ok(())
    }

    fn update(&mut self, watcher: &mut RecommendedWatcher, event: &Event) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            if matches!(
                event.kind,
                EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
            ) {
                for path in &event.paths {
                    if !self.directories.contains(path) {
                        continue;
                    }
                    let removed: Vec<_> = self
                        .directories
                        .iter()
                        .filter(|directory| directory.starts_with(path))
                        .cloned()
                        .collect();
                    for directory in removed {
                        let _ = watcher.unwatch(&directory);
                        self.directories.remove(&directory);
                    }
                }
            }
            if matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(_))
            ) {
                for path in &event.paths {
                    if fs::symlink_metadata(path)
                        .is_ok_and(|metadata| metadata.is_dir() && !metadata.is_symlink())
                    {
                        self.add(watcher, path)?;
                    }
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = (watcher, event);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{FutureExt, StreamExt};
    use notify::event::{AccessKind, AccessMode, DataChange};
    use std::sync::atomic::AtomicU64;

    static DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        path: PathBuf,
        roots: WatchRoots,
    }

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "gitturtle-watch-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            let path = path.canonicalize().unwrap();
            let roots = WatchRoots {
                worktree: path.join("linked"),
                common_git: path.join("main/.git"),
                private_git: path.join("main/.git/worktrees/linked"),
            };
            fs::create_dir_all(&roots.worktree).unwrap();
            fs::create_dir_all(roots.common_git.join("refs/heads")).unwrap();
            fs::create_dir_all(&roots.private_git).unwrap();
            fs::write(
                roots.worktree.join(".git"),
                format!("gitdir: {}\n", roots.private_git.display()),
            )
            .unwrap();
            fs::write(roots.worktree.join("file.txt"), "before\n").unwrap();
            fs::write(roots.private_git.join("HEAD"), "ref: refs/heads/topic\n").unwrap();
            fs::write(roots.private_git.join("index"), "fixture-index").unwrap();
            fs::write(
                roots.common_git.join("refs/heads/topic"),
                "1111111111111111111111111111111111111111\n",
            )
            .unwrap();
            Self { path, roots }
        }

        fn observe(
            &self,
        ) -> (
            LocalWatcher,
            mpsc::Receiver<LocalChange>,
            std::thread::JoinHandle<()>,
        ) {
            let (watcher, mut changes) = watch(self.roots.clone()).unwrap();
            let (sender, receiver) = mpsc::channel();
            let bridge = std::thread::spawn(move || {
                futures::executor::block_on(async move {
                    while let Some(change) = changes.next().await {
                        if sender.send(change).is_err() {
                            break;
                        }
                    }
                })
            });
            (watcher, receiver, bridge)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn bursts_wait_for_quiet_but_continuous_activity_has_a_maximum_delay() {
        let start = Instant::now();
        let mut burst = Burst::default();
        burst.push(
            LocalChange {
                worktree: true,
                ..Default::default()
            },
            start,
        );
        assert!(burst.take_due(start + Duration::from_millis(249)).is_none());
        burst.push(
            LocalChange {
                git: true,
                ..Default::default()
            },
            start + Duration::from_millis(100),
        );
        assert_eq!(burst.deadline(), Some(start + Duration::from_millis(350)));
        for millis in (300..=1900).step_by(200) {
            burst.push(
                LocalChange {
                    worktree: true,
                    ..Default::default()
                },
                start + Duration::from_millis(millis),
            );
        }
        assert_eq!(burst.deadline(), Some(start + MAX_DELAY));
        assert_eq!(
            burst.take_due(start + MAX_DELAY),
            Some(LocalChange {
                worktree: true,
                git: true,
                ..Default::default()
            })
        );
        assert!(burst.deadline().is_none());
    }

    #[test]
    fn delivery_merges_many_changes_without_an_unbounded_refresh_queue() {
        let (mut delivery, mut changes) = Delivery::channel();
        for _ in 0..10_000 {
            assert!(delivery.send(LocalChange {
                worktree: true,
                ..Default::default()
            }));
        }
        assert!(delivery.send(LocalChange {
            git: true,
            rescan: true,
            error: Some("Native events were lost".into()),
            ..Default::default()
        }));
        let first = futures::executor::block_on(changes.next()).unwrap();
        assert!(first.worktree && first.git && first.rescan);
        assert_eq!(first.error.as_deref(), Some("Native events were lost"));
        assert!(changes.next().now_or_never().is_none());
        assert!(delivery.send(LocalChange {
            git: true,
            ..Default::default()
        }));
        assert_eq!(
            futures::executor::block_on(changes.next()),
            Some(LocalChange {
                git: true,
                ..Default::default()
            })
        );
        drop(changes);
        assert!(!delivery.send(LocalChange {
            worktree: true,
            ..Default::default()
        }));
    }

    #[test]
    fn resolved_linked_worktree_paths_and_git_lock_filter_preserve_scope() {
        let fixture = Fixture::new();
        let roots = &fixture.roots;
        assert_eq!(
            roots.watched_roots(),
            [roots.worktree.as_path(), roots.common_git.as_path()]
        );
        for path in [
            roots.private_git.join("HEAD"),
            roots.private_git.join("index"),
            roots.common_git.join("refs/heads/topic"),
            roots.common_git.join("objects/ab/object"),
        ] {
            assert_eq!(roots.classify(&path), ChangeKind::Git);
        }
        assert_eq!(
            roots.classify(&roots.private_git.join("index.lock")),
            ChangeKind::Ignore
        );
        assert_eq!(
            roots.classify(&roots.common_git.join("refs/heads/topic.lock")),
            ChangeKind::Ignore
        );
        assert_eq!(
            roots.classify(&roots.private_git.join("COMMIT_EDITMSG")),
            ChangeKind::Ignore
        );
        assert_eq!(
            roots.classify(&roots.worktree.join("important.lock")),
            ChangeKind::Worktree
        );
        assert_eq!(
            roots.classify(&fixture.path.join("unrelated")),
            ChangeKind::Ignore
        );
        assert!(passive_event(&Event::new(EventKind::Access(
            AccessKind::Open(AccessMode::Read)
        ))));
        assert!(passive_event(&Event::new(EventKind::Modify(
            ModifyKind::Metadata(MetadataKind::AccessTime)
        ))));
        assert!(!passive_event(&Event::new(EventKind::Modify(
            ModifyKind::Data(DataChange::Content)
        ))));
    }

    #[test]
    fn fingerprinting_suppresses_duplicate_events_and_index_lock_directory_noise() {
        let fixture = Fixture::new();
        let roots = &fixture.roots;
        let mut fingerprints = Fingerprints::new(roots);
        let lock = roots.private_git.join("index.lock");
        fs::write(&lock, "temporary index").unwrap();
        fs::remove_file(lock).unwrap();
        assert!(!fingerprints.changed(&roots.private_git, ChangeKind::GitRoot));
        assert!(!fingerprints.changed(&roots.private_git.join("index"), ChangeKind::Git));
        fs::write(roots.private_git.join("index"), "updated fixture index").unwrap();
        assert!(fingerprints.changed(&roots.private_git, ChangeKind::GitRoot));
        assert!(!fingerprints.changed(&roots.private_git, ChangeKind::GitRoot));
        let path = roots.worktree.join("file.txt");
        assert!(fingerprints.changed(&path, ChangeKind::Worktree));
        assert!(!fingerprints.changed(&path, ChangeKind::Worktree));
        fs::remove_file(&path).unwrap();
        assert!(fingerprints.changed(&path, ChangeKind::Worktree));
        assert!(!fingerprints.changed(&path, ChangeKind::Worktree));
    }

    #[test]
    fn fingerprint_storage_remains_bounded_during_large_change_sets() {
        let fixture = Fixture::new();
        let mut fingerprints = Fingerprints::new(&fixture.roots);
        for index in 0..MAX_FINGERPRINTS * 2 {
            fingerprints.insert(fixture.path.join(index.to_string()), Fingerprint::Missing);
        }
        assert_eq!(fingerprints.values.len(), MAX_FINGERPRINTS);
        assert_eq!(fingerprints.order.len(), MAX_FINGERPRINTS);
    }

    #[test]
    fn native_notifications_detect_worktree_private_and_common_git_changes() {
        for area in 0..3 {
            let fixture = Fixture::new();
            let (watcher, changes, bridge) = fixture.observe();
            let path = match area {
                0 => fixture.roots.worktree.join("file.txt"),
                1 => fixture.roots.private_git.join("HEAD"),
                _ => fixture.roots.common_git.join("refs/heads/topic"),
            };
            fs::write(path, "a changed value that is longer than before\n").unwrap();
            let change = changes
                .recv_timeout(Duration::from_secs(6))
                .expect("native local notification");
            if area == 0 {
                assert!(change.worktree, "{change:?}");
            } else {
                assert!(change.git, "{change:?}");
            }
            assert!(change.error.is_none(), "{change:?}");
            drop(watcher);
            bridge.join().unwrap();
        }
    }

    #[test]
    fn native_access_and_index_lock_transients_do_not_request_refresh() {
        let fixture = Fixture::new();
        let (watcher, changes, bridge) = fixture.observe();
        let lock = fixture.roots.private_git.join("index.lock");
        fs::write(&lock, "temporary index").unwrap();
        fs::remove_file(lock).unwrap();
        fs::read(fixture.roots.worktree.join("file.txt")).unwrap();
        fs::read(fixture.roots.private_git.join("index")).unwrap();
        assert!(matches!(
            changes.recv_timeout(Duration::from_millis(1500)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        drop(watcher);
        bridge.join().unwrap();
    }
}
