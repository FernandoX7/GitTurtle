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
    /// Coverage recovered; supersedes a prior warning in a coalesced burst.
    pub recovered: bool,
}

impl LocalChange {
    pub(crate) fn merge(&mut self, next: Self) {
        self.worktree |= next.worktree;
        self.git |= next.git;
        self.rescan |= next.rescan;
        if next.recovered {
            self.error = None;
            self.recovered = true;
        }
        if next.error.is_some() {
            self.error = next.error;
            self.recovered = false;
        }
    }
    pub(crate) fn is_empty(&self) -> bool {
        !self.worktree && !self.git && !self.rescan && self.error.is_none() && !self.recovered
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
    let mut registrations = Registrations::new(&roots);
    registrations.start(&mut watcher, &roots);
    let fingerprints = Fingerprints::new(&roots);
    let (mut delivery, changes) = Delivery::channel();
    if let Some(error) = registrations.diagnostic() {
        delivery.send(LocalChange {
            rescan: true,
            error: Some(error),
            ..Default::default()
        });
    }
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
    let mut policy_paths = std::collections::HashSet::new();
    let mut overflow_reconciled = false;
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
                for path in &event.paths {
                    if policy_paths.len() < MAX_EVENTS
                        && (registrations.policy_file(path)
                            || (path.starts_with(&roots.worktree)
                                && path.file_name().is_some_and(|name| name == ".gitignore"))
                            || path == &roots.private_git.join("index")
                            || path == &roots.common_git.join("info/exclude")
                            || path == &roots.common_git.join("config")
                            || path == &roots.private_git.join("config.worktree"))
                    {
                        policy_paths.insert(path.clone());
                        change.worktree = true;
                    }
                    if [&roots.worktree, &roots.private_git, &roots.common_git].contains(&path)
                        && matches!(
                            event.kind,
                            EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(_))
                        )
                    {
                        policy_paths.insert(path.clone());
                        change.rescan = true;
                    }
                }
                change.rescan |= event.need_rescan();
                let previous = registrations.diagnostic();
                registrations.update(&mut watcher, &event, &roots);
                let next = registrations.diagnostic();
                if next != previous {
                    change.error = next;
                    change.recovered = change.error.is_none();
                    change.rescan = true;
                }
                if !passive_event(&event) {
                    for path in &event.paths {
                        let kind = roots.classify(path);
                        if kind != ChangeKind::Ignore
                            && registrations.relevant(path, kind)
                            && fingerprints.changed(path, kind)
                        {
                            if kind == ChangeKind::Worktree {
                                change.worktree = true;
                            } else {
                                change.git = true;
                            }
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                overflow_reconciled = false;
            }
            Ok(Message::Wake) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        change.rescan |= signals.overflow.swap(false, Ordering::AcqRel);
        if change.rescan && !overflow_reconciled {
            // One bounded coverage reconciliation per continuous overflow
            // burst; quiet re-arms it. Normal edits never walk the whole tree.
            policy_paths.insert(roots.worktree.join(".gitignore"));
            policy_paths.insert(roots.private_git.join("index"));
            overflow_reconciled = true;
        }
        if let Some(error) = signals
            .error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            change.error = Some(error);
        }
        burst.push(change, Instant::now());
        if let Some(mut change) = burst.take_due(Instant::now()) {
            if !policy_paths.is_empty() {
                let previous = registrations.diagnostic();
                registrations.refresh_policy(
                    &mut watcher,
                    &roots,
                    &policy_paths.drain().collect::<Vec<_>>(),
                );
                let next = registrations.diagnostic();
                if previous != next {
                    change.error = next;
                    change.recovered = change.error.is_none();
                    change.rescan = true;
                }
            }
            if !delivery.send(change) {
                break;
            }
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

const MAX_DIRECTORIES: usize = 16_384;
const MAX_SCAN_ENTRIES: usize = 200_000;
const MAX_SCAN_TIME: Duration = Duration::from_secs(2);
const MAX_DEFERRED_ROOTS: usize = 256;

struct Registrations {
    directories: std::collections::HashSet<PathBuf>,
    deferred: HashMap<PathBuf, String>,
    policy: Option<gitturtle_core::LocalWatchPolicy>,
    policy_error: Option<String>,
    worktree: PathBuf,
    scan_started: Instant,
    scan_entries: usize,
    directory_limit: usize,
}

impl Registrations {
    fn new(roots: &WatchRoots) -> Self {
        let policy = gitturtle_core::GitRepository::open(&roots.worktree)
            .and_then(|repository| repository.local_watch_policy());
        let (policy, policy_error) = match policy {
            Ok(policy) => (Some(policy), None),
            Err(error) => (
                None,
                Some(bounded_error(format!("Working-tree coverage: {error:#}"))),
            ),
        };
        Self {
            directories: Default::default(),
            deferred: Default::default(),
            policy,
            policy_error,
            worktree: roots.worktree.clone(),
            scan_started: Instant::now(),
            scan_entries: 0,
            directory_limit: MAX_DIRECTORIES,
        }
    }

    fn diagnostic(&self) -> Option<String> {
        self.policy_error.clone().or_else(|| {
            self.deferred.iter().min_by_key(|(path, _)| *path)
                .map(|(path, error)| bounded_error(format!("{error}\nDirectory: {}\nRegistered directories: {}. Git metadata registrations are retained. Refresh retries local coverage.", path.display(), self.directories.len())))
        })
    }

    fn defer(&mut self, path: &Path, error: impl std::fmt::Display) {
        if self.deferred.keys().any(|root| path.starts_with(root)) {
            return;
        }
        self.deferred.retain(|root, _| !root.starts_with(path));
        if self.deferred.len() < MAX_DEFERRED_ROOTS {
            self.deferred.insert(path.to_owned(), bounded_error(error));
        } else {
            self.policy_error.get_or_insert_with(|| "More than 256 directory subtrees could not be watched. Refresh to retry coverage.".into());
        }
    }

    fn start(&mut self, watcher: &mut RecommendedWatcher, roots: &WatchRoots) {
        self.begin_scan();
        // Register every essential root before spending watches on descendants.
        // A worktree budget/error must never discard HEAD/index/ref monitoring.
        for path in [&roots.private_git, &roots.common_git, &roots.worktree] {
            self.register(watcher, path);
        }
        if cfg!(target_os = "linux") {
            for path in [&roots.private_git, &roots.common_git, &roots.worktree] {
                if let Some(parent) = path.parent() {
                    self.register(watcher, parent);
                }
            }
            self.register_policy_parents(watcher);
        }
        if cfg!(target_os = "linux") {
            for root in [&roots.private_git, &roots.common_git] {
                for name in [
                    "refs",
                    "refs/heads",
                    "refs/remotes",
                    "refs/tags",
                    "info",
                    "rebase-merge",
                    "rebase-apply",
                    "sequencer",
                ] {
                    self.register(watcher, &root.join(name));
                }
            }
        }
        for root in [&roots.private_git, &roots.common_git] {
            self.add(watcher, root, roots, true);
        }
        if self.policy.is_some() {
            self.add(watcher, &roots.worktree, roots, false);
        }
    }

    fn register(&mut self, watcher: &mut RecommendedWatcher, path: &Path) -> bool {
        if self.directories.contains(path) {
            return true;
        }
        if self.directories.len() >= self.directory_limit {
            self.defer(path, "The local directory-watch budget is full");
            return false;
        }
        if path.starts_with(&self.worktree)
            && path
                .ancestors()
                .take_while(|parent| *parent != self.worktree)
                .any(|parent| {
                    fs::symlink_metadata(parent).is_ok_and(|metadata| metadata.is_symlink())
                })
        {
            return false;
        }
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if transient_directory_error(&error) => return false,
            Err(error) => {
                self.defer(path, error);
                return false;
            }
        };
        if !metadata.is_dir() || metadata.is_symlink() {
            return false;
        }
        let mode = if cfg!(target_os = "linux") {
            RecursiveMode::NonRecursive
        } else {
            RecursiveMode::Recursive
        };
        match watcher.watch(path, mode) {
            Ok(()) => {
                self.directories.insert(path.to_owned());
                true
            }
            Err(error) if matches!(&error.kind, notify::ErrorKind::Io(error) if transient_directory_error(error)) => {
                false
            }
            Err(error) if matches!(error.kind, notify::ErrorKind::PathNotFound) => false,
            Err(error) => {
                self.defer(path, error);
                false
            }
        }
    }

    fn relevant(&mut self, path: &Path, kind: ChangeKind) -> bool {
        if kind != ChangeKind::Worktree {
            return true;
        }
        let Some(policy) = self.policy.as_mut() else {
            return true;
        };
        let directory = fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir());
        // Deleted watched directories still need status and coverage cleanup.
        if self.directories.contains(path) {
            return true;
        }
        policy.includes(path, directory).unwrap_or(true)
    }

    fn eligible(&mut self, path: &Path, roots: &WatchRoots, metadata: bool) -> bool {
        if metadata {
            return matches!(roots.classify(path), ChangeKind::Git | ChangeKind::GitRoot);
        }
        if path == roots.worktree {
            return true;
        }
        if !path.starts_with(&roots.worktree)
            || path.starts_with(&roots.private_git)
            || path.starts_with(&roots.common_git)
            || path == roots.worktree.join(".git")
        {
            return false;
        }
        match self
            .policy
            .as_mut()
            .map(|policy| policy.includes(path, true))
        {
            Some(Ok(included)) => included,
            Some(Err(error)) => {
                self.defer(path, error);
                false
            }
            None => false,
        }
    }

    fn add(
        &mut self,
        watcher: &mut RecommendedWatcher,
        root: &Path,
        roots: &WatchRoots,
        metadata: bool,
    ) {
        self.deferred.remove(root);
        if !cfg!(target_os = "linux") {
            // Keep native recursive subscriptions on FSEvents and other
            // platforms. The explicit traversal is for Linux inotify only.
            if !self
                .directories
                .iter()
                .any(|parent| root.starts_with(parent))
                && self.eligible(root, roots, metadata)
            {
                self.register(watcher, root);
            }
            return;
        }
        // notify's Linux recursive setup follows directory symlinks. Register
        // bounded explicit directories instead, using the same policy on every
        // platform. Existing registrations may still contain new descendants.
        let mut pending = VecDeque::from([root.to_owned()]);
        while let Some(path) = pending.pop_front() {
            if !self.eligible(&path, roots, metadata) {
                continue;
            }
            if self.scan_entries >= MAX_SCAN_ENTRIES || self.scan_started.elapsed() >= MAX_SCAN_TIME
            {
                self.defer(root, "Directory coverage reached its bounded scan budget");
                break;
            }
            if !self.register(watcher, &path) {
                continue;
            }
            let children = match fs::read_dir(&path) {
                Ok(children) => children,
                Err(error) if transient_directory_error(&error) => continue,
                Err(error) => {
                    self.defer(&path, error);
                    continue;
                }
            };
            for entry in children {
                self.scan_entries += 1;
                if self.scan_entries >= MAX_SCAN_ENTRIES
                    || pending.len() >= MAX_DIRECTORIES
                    || self.scan_started.elapsed() >= MAX_SCAN_TIME
                {
                    self.defer(root, "Directory coverage reached its bounded scan budget");
                    return;
                }
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) if transient_directory_error(&error) => continue,
                    Err(error) => {
                        self.defer(&path, error);
                        continue;
                    }
                };
                match entry.file_type() {
                    Ok(kind) if kind.is_dir() => pending.push_back(entry.path()),
                    Ok(_) => {}
                    Err(error) if transient_directory_error(&error) => {}
                    Err(error) => self.defer(&path, error),
                }
            }
        }
    }

    fn remove(&mut self, watcher: &mut RecommendedWatcher, path: &Path) -> bool {
        if let Some(policy) = &mut self.policy {
            policy.forget_directory(path);
        }
        let removed: Vec<_> = self
            .directories
            .iter()
            .filter(|directory| directory.starts_with(path))
            .cloned()
            .collect();
        for directory in &removed {
            let _ = watcher.unwatch(directory);
            self.directories.remove(directory);
        }
        self.deferred
            .retain(|directory, _| !directory.starts_with(path));
        !removed.is_empty()
    }

    fn update(&mut self, watcher: &mut RecommendedWatcher, event: &Event, roots: &WatchRoots) {
        self.begin_scan();
        let mut freed = false;
        if matches!(
            event.kind,
            EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
        ) {
            for path in &event.paths {
                if roots.classify(path) != ChangeKind::Ignore || self.directories.contains(path) {
                    freed |= self.remove(watcher, path);
                }
            }
        }
        if matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(_))
        ) {
            for path in &event.paths {
                // Parent and ignore-file sentinels also report unrelated
                // siblings. Route by repository scope before filesystem or
                // ignore-policy reads; only our own roots need recovery.
                let metadata = match roots.classify(path) {
                    ChangeKind::Git | ChangeKind::GitRoot => true,
                    ChangeKind::Worktree => false,
                    ChangeKind::Ignore => continue,
                };
                if fs::symlink_metadata(path)
                    .is_ok_and(|metadata| metadata.is_dir() && !metadata.is_symlink())
                {
                    if path == &roots.worktree {
                        for git in [&roots.private_git, &roots.common_git] {
                            if git.starts_with(path) {
                                self.add(watcher, git, roots, true);
                            }
                        }
                    }
                    self.add(watcher, path, roots, metadata);
                }
            }
        }
        if freed && !self.deferred.is_empty() {
            // Recovery is driven by freed registrations, never a timer/retry
            // loop. One bounded pass handles the remembered omitted subtrees.
            let deferred = std::mem::take(&mut self.deferred);
            for path in deferred.keys() {
                let metadata =
                    matches!(roots.classify(path), ChangeKind::Git | ChangeKind::GitRoot);
                self.add(watcher, path, roots, metadata);
                if self.directories.len() >= self.directory_limit {
                    break;
                }
            }
            for (path, error) in deferred {
                if !self.directories.contains(&path) && path.is_dir() {
                    self.defer(&path, error);
                }
            }
        }
        for root in [&roots.worktree, &roots.private_git, &roots.common_git] {
            if event.paths.contains(root) && !root.is_dir() {
                self.defer(
                    root,
                    "A watched repository directory moved or became unavailable",
                );
            }
        }
    }

    fn refresh_policy(
        &mut self,
        watcher: &mut RecommendedWatcher,
        roots: &WatchRoots,
        paths: &[PathBuf],
    ) {
        self.begin_scan();
        let roots_changed = paths
            .iter()
            .any(|path| [&roots.worktree, &roots.private_git, &roots.common_git].contains(&path));
        let index_changed = roots_changed
            || paths
                .iter()
                .any(|path| path == &roots.private_git.join("index"));
        let all_rules = roots_changed
            || paths.iter().any(|path| {
                self.policy_file(path)
                    || path == &roots.common_git.join("info/exclude")
                    || path == &roots.common_git.join("config")
                    || path == &roots.private_git.join("config.worktree")
            });
        let mut changed_rules: Vec<_> = paths
            .iter()
            .filter(|path| {
                path.starts_with(&roots.worktree)
                    && path.file_name().is_some_and(|name| name == ".gitignore")
            })
            .filter_map(|path| path.parent().map(Path::to_owned))
            .collect();
        if all_rules || index_changed {
            let previous_tracked: Vec<_> = self
                .policy
                .as_ref()
                .into_iter()
                .flat_map(|policy| policy.tracked_directories())
                .filter(|path| self.directories.contains(path))
                .collect();
            match gitturtle_core::GitRepository::open(&self.worktree)
                .and_then(|repository| repository.local_watch_policy())
            {
                Ok(policy) => {
                    let tracked: Vec<_> = policy.tracked_directories().collect();
                    self.policy = Some(policy);
                    self.policy_error = None;
                    // Release formerly tracked directories that are now
                    // ignored before admitting the new tracked ancestors.
                    // This inspects only known paths, not the working tree.
                    for path in previous_tracked {
                        if self
                            .policy
                            .as_mut()
                            .is_some_and(|policy| matches!(policy.includes(&path, true), Ok(false)))
                        {
                            let _ = watcher.unwatch(&path);
                            self.directories.remove(&path);
                            self.deferred.retain(|root, _| !root.starts_with(&path));
                            if let Some(policy) = &mut self.policy {
                                policy.forget_directory(&path);
                            }
                        }
                    }
                    if cfg!(target_os = "linux") {
                        self.register_policy_parents(watcher);
                    }
                    // Index changes add only newly tracked ancestors, without
                    // rescanning every existing worktree directory.
                    for path in tracked {
                        if !self.directories.contains(&path) {
                            self.add(watcher, &path, roots, false);
                        }
                    }
                }
                Err(error) => self.policy_error = Some(bounded_error(error)),
            }
        }
        if all_rules {
            changed_rules = vec![roots.worktree.clone()];
        }
        changed_rules.sort();
        changed_rules.dedup();
        for root in changed_rules {
            if let Some(policy) = &mut self.policy {
                policy.invalidate_ignores();
            }
            let candidates: Vec<_> = self
                .directories
                .iter()
                .filter(|directory| directory.starts_with(&root))
                .cloned()
                .collect();
            for path in candidates {
                if roots.classify(&path) == ChangeKind::Worktree
                    && !self.eligible(&path, roots, false)
                {
                    self.remove(watcher, &path);
                }
            }
            self.deferred.retain(|path, _| !path.starts_with(&root));
            self.add(watcher, &root, roots, false);
        }
    }

    fn begin_scan(&mut self) {
        self.scan_started = Instant::now();
        self.scan_entries = 0;
    }

    fn policy_file(&self, path: &Path) -> bool {
        self.policy
            .as_ref()
            .is_some_and(|policy| policy.ignore_files().iter().any(|file| file == path))
    }

    fn register_policy_parents(&mut self, watcher: &mut RecommendedWatcher) {
        let parents: Vec<_> = self
            .policy
            .as_ref()
            .into_iter()
            .flat_map(|policy| policy.ignore_files())
            .filter_map(|path| path.parent().map(Path::to_owned))
            .collect();
        for parent in parents {
            if parent.is_dir() {
                self.register(watcher, &parent);
            }
        }
    }
}

fn transient_directory_error(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{FutureExt, StreamExt};
    use notify::event::{AccessKind, AccessMode, DataChange};
    use std::process::{Command, Output};

    struct Fixture {
        _directory: tempfile::TempDir,
        path: PathBuf,
        roots: WatchRoots,
    }

    impl Fixture {
        fn new() -> Self {
            let mut builder = tempfile::Builder::new();
            builder.prefix("gitturtle-watch-");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                // Create the boundary privately, even with a permissive umask.
                builder.permissions(fs::Permissions::from_mode(0o700));
            }
            let directory = builder.tempdir().unwrap();
            let path = directory.path().canonicalize().unwrap();
            let roots = WatchRoots {
                worktree: path.join("linked"),
                common_git: path.join("main/.git"),
                private_git: path.join("main/.git/worktrees/linked"),
            };
            fs::create_dir_all(path.join("main")).unwrap();
            let git = |args: &[&str]| fixture_git(&path.join("main"), args);
            git(&["init", "-b", "main"]);
            git(&["commit", "--allow-empty", "-m", "Initial"]);
            git(&[
                "worktree",
                "add",
                "-b",
                "topic",
                roots.worktree.to_str().unwrap(),
            ]);
            fs::write(roots.worktree.join("file.txt"), "before\n").unwrap();
            Self {
                _directory: directory,
                path,
                roots,
            }
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

    fn isolate_fixture_git(command: &mut Command) {
        // -C does not override inherited GIT_DIR/GIT_WORK_TREE/GIT_INDEX_FILE.
        // Remove Git targeting, templates, and configuration before any fixture
        // command can write; leave the runner's ordinary process setup intact.
        let git_environment: Vec<_> = std::env::vars_os()
            .map(|(name, _)| name)
            .chain(command.get_envs().map(|(name, _)| name.to_owned()))
            .filter(|name| name.to_string_lossy().starts_with("GIT_"))
            .collect();
        for name in git_environment {
            command.env_remove(name);
        }
        command
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0");
    }

    fn fixture_git_command(path: &Path, args: &[&str]) -> Command {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(path)
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "init.templateDir=",
            ])
            .args(args);
        isolate_fixture_git(&mut command);
        command
    }

    fn fixture_git(path: &Path, args: &[&str]) -> Output {
        let output = fixture_git_command(path, args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    #[test]
    fn fixture_git_cannot_redirect_writes_into_another_repository() {
        let fixture = Fixture::new();
        let unrelated = Fixture::new();
        let unrelated_index = unrelated.roots.private_git.join("index");
        let before = fs::read(&unrelated_index).unwrap();
        let mut command = fixture_git_command(&fixture.roots.worktree, &["add", "--", "file.txt"]);
        command
            .env("GIT_DIR", &unrelated.roots.private_git)
            .env("GIT_WORK_TREE", &unrelated.roots.worktree)
            .env("GIT_INDEX_FILE", &unrelated_index)
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "core.worktree")
            .env("GIT_CONFIG_VALUE_0", &unrelated.roots.worktree);
        isolate_fixture_git(&mut command);
        let output = command.output().unwrap();
        assert!(output.status.success(), "{:?}", output.stderr);
        assert_eq!(fs::read(&unrelated_index).unwrap(), before);
        assert_eq!(
            fixture_git(&fixture.roots.worktree, &["show", ":file.txt"]).stdout,
            b"before\n"
        );
    }

    #[test]
    #[cfg(unix)]
    fn fixture_directory_is_private_and_resolves_temporary_directory_aliases() {
        use std::os::unix::fs::PermissionsExt;
        let fixture = Fixture::new();
        assert_eq!(
            fs::metadata(&fixture.path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(fixture.path.canonicalize().unwrap(), fixture.path);
        assert!(fixture.roots.worktree.starts_with(&fixture.path));
        assert!(fixture.roots.private_git.starts_with(&fixture.path));
        assert!(fixture.roots.common_git.starts_with(&fixture.path));
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

    #[cfg(target_os = "linux")]
    fn registrations(fixture: &Fixture) -> (RecommendedWatcher, Registrations) {
        let watcher = notify::recommended_watcher(|_: notify::Result<Event>| {}).unwrap();
        (watcher, Registrations::new(&fixture.roots))
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn sibling_directories_never_expand_or_degrade_parent_sentinel_coverage() {
        use notify::event::{CreateKind, RemoveKind, RenameMode};
        let fixture = Fixture::new();
        let (mut watcher, mut registrations) = registrations(&fixture);
        registrations.start(&mut watcher, &fixture.roots);
        let original = registrations.directories.clone();
        for sibling in [
            fixture.path.join("linked-peer"),
            fixture.path.join("main/build-peer"),
        ] {
            for index in 0..32 {
                fs::create_dir_all(sibling.join(index.to_string())).unwrap();
            }
            registrations.update(
                &mut watcher,
                &Event::new(EventKind::Create(CreateKind::Folder)).add_path(sibling.clone()),
                &fixture.roots,
            );
            let renamed = sibling.with_extension("renamed");
            fs::rename(&sibling, &renamed).unwrap();
            registrations.update(
                &mut watcher,
                &Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
                    .add_path(sibling)
                    .add_path(renamed.clone()),
                &fixture.roots,
            );
            assert!(
                registrations.diagnostic().is_none(),
                "{:?}",
                registrations.diagnostic()
            );
            assert_eq!(registrations.scan_entries, 0);
            assert_eq!(registrations.directories, original);
            fs::remove_dir_all(&renamed).unwrap();
            registrations.update(
                &mut watcher,
                &Event::new(EventKind::Remove(RemoveKind::Folder)).add_path(renamed),
                &fixture.roots,
            );
            assert!(registrations.diagnostic().is_none());
            assert_eq!(registrations.scan_entries, 0);
            assert_eq!(registrations.directories, original);
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn native_sibling_churn_does_not_request_refresh_or_degrade_coverage() {
        let fixture = Fixture::new();
        let (watcher, changes, bridge) = fixture.observe();
        for sibling in [
            fixture.path.join("linked-peer"),
            fixture.path.join("main/build-peer"),
        ] {
            fs::create_dir_all(sibling.join("child")).unwrap();
            fs::write(sibling.join("child/scratch"), "unrelated").unwrap();
            // Keep the create visible to the actor before the later cleanup.
            std::thread::sleep(Duration::from_millis(50));
            let renamed = sibling.with_extension("renamed");
            fs::rename(sibling, &renamed).unwrap();
            std::thread::sleep(Duration::from_millis(50));
            fs::remove_dir_all(renamed).unwrap();
        }
        assert!(
            matches!(
                changes.recv_timeout(Duration::from_millis(1500)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "unrelated sibling churn must not request a refresh or coverage warning"
        );
        for (path, metadata) in [
            (fixture.roots.worktree.join("file.txt"), false),
            (fixture.roots.private_git.join("config.worktree"), true),
        ] {
            fs::write(path, "# a subsequent local change\n").unwrap();
            let change = changes.recv_timeout(Duration::from_secs(6)).unwrap();
            assert!(change.error.is_none() && !change.rescan, "{change:?}");
            assert!(
                if metadata {
                    change.git
                } else {
                    change.worktree
                },
                "{change:?}"
            );
        }
        drop(watcher);
        bridge.join().unwrap();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn ignored_build_tree_is_pruned_before_the_directory_limit() {
        let fixture = Fixture::new();
        let build = fixture.roots.worktree.join("large-output");
        fs::create_dir(&build).unwrap();
        fs::write(
            fixture.roots.worktree.join(".gitignore"),
            "/large-output/\n",
        )
        .unwrap();
        for index in 0..MAX_DIRECTORIES + 2 {
            fs::create_dir(build.join(index.to_string())).unwrap();
        }
        let (mut watcher, mut registrations) = registrations(&fixture);
        registrations.start(&mut watcher, &fixture.roots);
        assert!(
            registrations.diagnostic().is_none(),
            "{:?}",
            registrations.diagnostic()
        );
        assert!(!registrations.directories.contains(&build));
        assert!(registrations.directories.len() < 40);
        assert!(
            registrations
                .directories
                .contains(&fixture.roots.common_git.join("refs/heads"))
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn disappearing_and_replaced_directories_are_normal_registration_races() {
        let fixture = Fixture::new();
        let (mut watcher, mut registrations) = registrations(&fixture);
        let vanished = fixture.roots.worktree.join("gone");
        fs::create_dir_all(vanished.join("child")).unwrap();
        fs::remove_dir_all(&vanished).unwrap();
        assert!(!registrations.register(&mut watcher, &vanished));
        fs::write(&vanished, "replaced with a file").unwrap();
        assert!(!registrations.register(&mut watcher, &vanished.join("child")));
        registrations.add(&mut watcher, &vanished, &fixture.roots, false);
        assert!(registrations.diagnostic().is_none());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn directory_budget_preserves_metadata_and_recovers_when_subtrees_are_removed() {
        use notify::event::RemoveKind;
        let fixture = Fixture::new();
        let bulk = fixture.roots.worktree.join("bulk");
        for index in 0..40 {
            fs::create_dir_all(bulk.join(index.to_string())).unwrap();
        }
        let (mut watcher, mut registrations) = registrations(&fixture);
        registrations.directory_limit = 30;
        registrations.start(&mut watcher, &fixture.roots);
        assert!(registrations.diagnostic().is_some());
        assert!(
            registrations
                .directories
                .contains(&fixture.roots.private_git)
        );
        assert!(
            registrations
                .directories
                .contains(&fixture.roots.common_git.join("refs/heads"))
        );
        fs::remove_dir_all(&bulk).unwrap();
        registrations.update(
            &mut watcher,
            &Event::new(EventKind::Remove(RemoveKind::Folder)).add_path(bulk),
            &fixture.roots,
        );
        assert!(
            registrations.diagnostic().is_none(),
            "{:?}",
            registrations.diagnostic()
        );
        assert!(
            registrations
                .directories
                .contains(&fixture.roots.common_git.join("refs/heads"))
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn edited_ignore_rules_reconcile_only_affected_coverage() {
        let fixture = Fixture::new();
        let folder = fixture.roots.worktree.join("generated");
        fs::create_dir_all(folder.join("nested")).unwrap();
        let ignore = fixture.roots.worktree.join(".gitignore");
        fs::write(&ignore, "/generated/\n").unwrap();
        let (mut watcher, mut registrations) = registrations(&fixture);
        registrations.start(&mut watcher, &fixture.roots);
        assert!(!registrations.directories.contains(&folder));
        fs::write(&ignore, "").unwrap();
        registrations.refresh_policy(&mut watcher, &fixture.roots, std::slice::from_ref(&ignore));
        assert!(registrations.directories.contains(&folder.join("nested")));
        fs::write(&ignore, "/generated/\n").unwrap();
        registrations.refresh_policy(&mut watcher, &fixture.roots, &[ignore]);
        assert!(!registrations.directories.contains(&folder));
        assert!(registrations.diagnostic().is_none());
    }

    #[test]
    fn recovery_supersedes_old_warning_in_a_coalesced_delivery() {
        let mut change = LocalChange {
            error: Some("limited".into()),
            ..Default::default()
        };
        change.merge(LocalChange {
            recovered: true,
            ..Default::default()
        });
        assert!(change.recovered);
        assert!(change.error.is_none());
        change.merge(LocalChange {
            error: Some("unavailable".into()),
            ..Default::default()
        });
        assert!(!change.recovered);
        assert_eq!(change.error.as_deref(), Some("unavailable"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn index_changes_add_tracked_paths_inside_an_ignored_directory() {
        let fixture = Fixture::new();
        let directory = fixture.roots.worktree.join("ignored/nested");
        fs::create_dir_all(&directory).unwrap();
        fs::write(fixture.roots.worktree.join(".gitignore"), "/ignored/\n").unwrap();
        fs::write(directory.join("tracked"), "tracked\n").unwrap();
        let (mut watcher, mut registrations) = registrations(&fixture);
        registrations.start(&mut watcher, &fixture.roots);
        assert!(!registrations.directories.contains(&directory));
        fixture_git(
            &fixture.roots.worktree,
            &["add", "--force", "--", "ignored/nested/tracked"],
        );
        registrations.refresh_policy(
            &mut watcher,
            &fixture.roots,
            &[fixture.roots.private_git.join("index")],
        );
        assert!(registrations.directories.contains(&directory));
        assert!(registrations.relevant(&directory.join("tracked"), ChangeKind::Worktree));
        assert!(!registrations.relevant(&directory.join("noise"), ChangeKind::Worktree));
        assert!(registrations.diagnostic().is_none());
    }

    #[test]
    fn native_directory_churn_keeps_notifications_available_without_race_warnings() {
        let fixture = Fixture::new();
        let (watcher, changes, bridge) = fixture.observe();
        for index in 0..150 {
            let path = fixture
                .roots
                .worktree
                .join(format!("temporary-{index}/child"));
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("scratch"), "temporary").unwrap();
            fs::remove_dir_all(path.parent().unwrap()).unwrap();
        }
        let first = changes.recv_timeout(Duration::from_secs(8)).unwrap();
        assert!(first.error.is_none(), "{first:?}");
        fs::write(fixture.roots.worktree.join("file.txt"), "after cleanup\n").unwrap();
        let after = changes.recv_timeout(Duration::from_secs(8)).unwrap();
        assert!(after.worktree && after.error.is_none(), "{after:?}");
        drop(watcher);
        bridge.join().unwrap();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn replacing_private_git_root_reloads_completed_index_and_keeps_metadata_watches() {
        use notify::event::{CreateKind, RemoveKind};
        let fixture = Fixture::new();
        let directory = fixture.roots.worktree.join("ignored/nested");
        fs::create_dir_all(&directory).unwrap();
        fs::write(fixture.roots.worktree.join(".gitignore"), "/ignored/\n").unwrap();
        fs::write(directory.join("tracked"), "tracked\n").unwrap();
        let (mut watcher, mut registrations) = registrations(&fixture);
        registrations.start(&mut watcher, &fixture.roots);
        assert!(!registrations.relevant(&directory.join("tracked"), ChangeKind::Worktree));
        fixture_git(
            &fixture.roots.worktree,
            &["add", "--force", "--", "ignored/nested/tracked"],
        );
        let parked = fixture.path.join("parked-private");
        fs::rename(&fixture.roots.private_git, &parked).unwrap();
        registrations.update(
            &mut watcher,
            &Event::new(EventKind::Remove(RemoveKind::Folder))
                .add_path(fixture.roots.private_git.clone()),
            &fixture.roots,
        );
        fs::rename(parked, &fixture.roots.private_git).unwrap();
        registrations.update(
            &mut watcher,
            &Event::new(EventKind::Create(CreateKind::Folder))
                .add_path(fixture.roots.private_git.clone()),
            &fixture.roots,
        );
        registrations.refresh_policy(
            &mut watcher,
            &fixture.roots,
            std::slice::from_ref(&fixture.roots.private_git),
        );
        assert!(registrations.relevant(&directory.join("tracked"), ChangeKind::Worktree));
        assert!(
            registrations
                .directories
                .contains(&fixture.roots.private_git)
        );
        assert!(
            registrations
                .directories
                .contains(&fixture.roots.common_git.join("refs/heads"))
        );
        assert!(
            registrations.diagnostic().is_none(),
            "{:?}",
            registrations.diagnostic()
        );
    }
    #[test]
    #[cfg(target_os = "linux")]
    fn index_changes_release_directories_that_become_ignored() {
        let fixture = Fixture::new();
        let directory = fixture.roots.worktree.join("ignored/nested");
        fs::create_dir_all(&directory).unwrap();
        fs::write(fixture.roots.worktree.join(".gitignore"), "/ignored/\n").unwrap();
        fs::write(directory.join("tracked"), "tracked\n").unwrap();
        let git = |args: &[&str]| fixture_git(&fixture.roots.worktree, args);
        git(&["add", "--force", "--", "ignored/nested/tracked"]);
        let (mut watcher, mut registrations) = registrations(&fixture);
        registrations.start(&mut watcher, &fixture.roots);
        assert!(registrations.directories.contains(&directory));
        git(&["rm", "--cached", "--", "ignored/nested/tracked"]);
        registrations.refresh_policy(
            &mut watcher,
            &fixture.roots,
            &[fixture.roots.private_git.join("index")],
        );
        assert!(!registrations.directories.contains(&directory));
        assert!(
            !registrations
                .directories
                .contains(&fixture.roots.worktree.join("ignored"))
        );
        assert!(directory.join("tracked").is_file());
        assert!(registrations.diagnostic().is_none());
    }
}
