//! Worker-owned repository session, identity validation, and retained readers.
use super::prepare::graph_notice;
use super::repository_identity::RepositoryIdentity;
use super::types::HistoryPageResult;
use super::{Cancellation, GRAPH_EDGE_LIMIT, GRAPH_LANE_LIMIT};
use crate::graph;
use anyhow::Result;
use gitturtle_core::GitRepository;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

/// A tab's repository that a passive read no longer finds at its path: Git
/// finds another repository there, the work tree enclosing it once its
/// `.git` is gone (deleted, moved, a failed clone cleaned up), or none.
/// The tab stays on the repository the person opened, marked unavailable
/// with this reason (`GitTurtle::tab_repository_gone`), and never shows
/// the other one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepositoryGone {
    /// The tab's repository, its work-tree root.
    pub path: PathBuf,
    /// The repository Git finds there now, by its root, if any.
    pub found: Option<PathBuf>,
}

impl std::fmt::Display for RepositoryGone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "The repository at {} is gone. ", self.path.display())?;
        match &self.found {
            Some(found) => write!(
                f,
                "Git now finds the repository at {} there, which this tab does not open.",
                found.display()
            )?,
            None => write!(f, "Git finds no repository there.")?,
        }
        write!(
            f,
            " Restore its .git and refresh, or choose a repository with Open Repository."
        )
    }
}

impl std::error::Error for RepositoryGone {}

/// A tab with no Git directories captured, as an older session saved every
/// tab, whose folder Git finds inside another repository. That may be all it
/// ever was: such a session kept the folder the person chose, so there may
/// be no `.git` to restore and "is gone" leaves nothing to do. It says what
/// the person can do: Open Repository on the folder takes the tab to the
/// repository found there, by the fresh capture a marked tab's own folder
/// gets (`GitTurtle::recapture_marked_tab`). The read itself opens nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoRepositoryOfItsOwn {
    /// The tab's folder.
    pub path: PathBuf,
    /// The repository Git finds there, by its root.
    pub found: PathBuf,
}

impl std::fmt::Display for NoRepositoryOfItsOwn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "This tab's folder {} has no repository of its own: Git finds the repository at {} there, which this tab does not open. To open that repository here, choose this folder with Open Repository; otherwise close this tab.",
            self.path.display(),
            self.found.display()
        )
    }
}

impl std::error::Error for NoRepositoryOfItsOwn {}

/// The Git directories a tab captured at its root as its snapshot was
/// accepted, which a passive reopen of that root must find there again
/// ([`RepositorySession::open`]): the Git directory Git reported and its
/// common directory ([`GitRepository::git_dir`],
/// [`GitRepository::common_dir`]). A `.git` swapped for another
/// repository's gitfile changes the first; a linked worktree's `commondir`
/// re-pointed at another repository keeps the root and the first, yet its
/// refs and objects, and every write's, are that other repository's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitDirectories {
    pub git_dir: PathBuf,
    pub common: PathBuf,
    /// Their inodes as captured at the repository's open, which a session
    /// saves beside these paths.
    pub inodes: DirectoryInodes,
    /// Whether these came from a saved session. Only such a tab compares
    /// [`Self::inodes`] with the directories it finds: a tab opened in this
    /// run, or one whose reopen was accepted, holds [`Self::of`]'s.
    pub restored: bool,
}

/// The inodes of a tab's [`GitDirectories`], each with its birth time where
/// the filesystem records one ([`GitRepository::git_dir_birth`]), which
/// tells a directory from one created after it was deleted that took its
/// inode number, and with its device, which tells the same inode number on
/// another filesystem ([`GitRepository::git_dir_device`]). `None` where a
/// session saved none, as every older session (one saved before birth
/// times were kept has its inodes alone), and where the platform or
/// filesystem has none: there is then nothing to compare, or the inode
/// alone. A session never saves a device, which can change across boots and
/// rollbacks (decided 2026-11-08), so only a comparison within the run has
/// one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DirectoryInodes {
    pub git_dir: Option<u64>,
    pub common: Option<u64>,
    pub git_dir_birth: Option<Duration>,
    pub common_birth: Option<Duration>,
    pub git_dir_device: Option<u64>,
    pub common_device: Option<u64>,
}

impl GitDirectories {
    pub fn of(repository: &GitRepository) -> Self {
        Self {
            git_dir: repository.git_dir().to_owned(),
            common: repository.common_dir().to_owned(),
            inodes: DirectoryInodes {
                git_dir: repository.git_dir_inode(),
                common: repository.common_dir_inode(),
                git_dir_birth: repository.git_dir_birth(),
                common_birth: repository.common_dir_birth(),
                git_dir_device: repository.git_dir_device(),
                common_device: repository.common_dir_device(),
            },
            restored: false,
        }
    }

    /// The directory of `repository` that differs from these, by its path
    /// or, for a restored tab, by the inode it saved or, where both these
    /// and the open have one, the birth time, the Git directory before the
    /// common one; `None` where both match. A session never saves the
    /// device, so a restored tab has none to compare: in a run, the app
    /// compares it only in `repository_tabs::same_git_directories` and
    /// `RepositoryIdentity`, and core before a write (`Identity::replaced`). Reads what the repository's open captured, so it
    /// touches no filesystem.
    fn mismatch(&self, repository: &GitRepository) -> Option<PathBuf> {
        fn apart<T: PartialEq>(restored: bool, saved: Option<T>, found: Option<T>) -> bool {
            restored
                && saved
                    .zip(found)
                    .is_some_and(|(saved, found)| saved != found)
        }
        let restored = self.restored;
        if repository.git_dir() != self.git_dir
            || apart(restored, self.inodes.git_dir, repository.git_dir_inode())
            || apart(
                restored,
                self.inodes.git_dir_birth,
                repository.git_dir_birth(),
            )
        {
            Some(repository.git_dir().to_owned())
        } else if repository.common_dir() != self.common
            || apart(restored, self.inodes.common, repository.common_dir_inode())
            || apart(
                restored,
                self.inodes.common_birth,
                repository.common_dir_birth(),
            )
        {
            Some(repository.common_dir().to_owned())
        } else {
            None
        }
    }
}

/// The folders above a tab's captured root as they resolve now, where they
/// resolve to other folders than the ones captured: a root saved through a
/// linked folder (macOS reaches every temporary folder through `/var`, a
/// link to `/private/var`; a person's projects folder can be a link in
/// their home), or one whose folder was moved and linked back from its old
/// place. A passive reopen compares the tab's root and Git directories
/// through them, on the worker as it reads the repository, never at launch;
/// the root's own name and everything below those folders are compared as
/// captured, so a root turned into a link to another repository, a `.git`
/// swapped for another repository's, or a re-pointed `commondir` is still
/// another repository ([`RepositorySession::open`]). Compared as captured,
/// every such root was another repository, its tab marked " ·
/// unavailable" where it opened before.
pub(super) struct FoldersAbove {
    captured: PathBuf,
    now: PathBuf,
}

impl FoldersAbove {
    /// `None` where `root` is reached as captured, as every root Git
    /// reported is (it is `canonical`, the folder the reopen asked for
    /// resolved), and where its folders cannot be resolved.
    fn of(root: &Path, canonical: &Path) -> Option<Self> {
        if root == canonical {
            return None;
        }
        root.file_name()?;
        let captured = root.parent()?;
        let now = captured.canonicalize().ok()?;
        (now != captured).then(|| Self {
            captured: captured.to_owned(),
            now,
        })
    }

    /// `path` reached through these folders as they resolve now; as
    /// captured where it lies outside them, as a linked worktree's Git
    /// directories in another folder do.
    fn through(&self, path: &Path) -> PathBuf {
        match path.strip_prefix(&self.captured) {
            Ok(rest) if rest.as_os_str().is_empty() => self.now.clone(),
            Ok(rest) => self.now.join(rest),
            Err(_) => path.to_owned(),
        }
    }
}

/// Fails with [`RepositoryGone`] where `repo` is no longer the repository
/// at its path: its `.git` gone, swapped for another repository's, or its
/// root turned into a link to another folder. From the filesystem alone
/// while it holds; Git is asked only once it does not, to name what it
/// finds, or for a repository whose commands cannot be confined.
pub(super) fn ensure_found_at_path(repo: &GitRepository) -> Result<()> {
    let found = match repo.verify_at_path()? {
        gitturtle_core::FoundAtPath::Same => return Ok(()),
        gitturtle_core::FoundAtPath::Other(found) => Some(found),
        gitturtle_core::FoundAtPath::None => None,
    };
    Err(RepositoryGone {
        path: repo.path().to_owned(),
        found,
    }
    .into())
}

/// An open of `requested` that core refused, as the reader reports it: a
/// repository that changed while it was opened is [`RepositoryGone`], and so
/// is a tab's root whose work tree now lies outside its folder, where an
/// explicit open of such a layout says so in its own words. One that Git
/// still reports at its path, only the capture having failed, stays
/// [`gitturtle_core::ChangedWhileOpening`]: it once read "is gone. Git now
/// finds the repository at P there", the one path twice, of a healthy
/// repository.
pub(super) fn refused_open(requested: &Path, passive: bool, error: anyhow::Error) -> anyhow::Error {
    match gitturtle_core::CoreError::from_anyhow(&error) {
        Some(gitturtle_core::CoreError::ChangedWhileOpening { path, found })
            if found.as_ref() != Some(path) =>
        {
            RepositoryGone {
                path: requested.to_owned(),
                found: found.clone(),
            }
            .into()
        }
        Some(gitturtle_core::CoreError::WorkTreeOutside { work_tree, .. }) if passive => {
            RepositoryGone {
                path: requested.to_owned(),
                found: Some(work_tree.clone()),
            }
            .into()
        }
        _ => error,
    }
}

/// What a failed open says: `repository_access::explain`'s reasons, but for
/// a refusal that says what happened in its own words.
pub(super) fn worded_open_failure(path: &Path, error: anyhow::Error) -> anyhow::Error {
    if error.is::<RepositoryGone>()
        || error.is::<NoRepositoryOfItsOwn>()
        || matches!(
            gitturtle_core::CoreError::from_anyhow(&error),
            Some(
                gitturtle_core::CoreError::WorkTreeOutside { .. }
                    | gitturtle_core::CoreError::ChangedWhileOpening { .. }
            )
        )
    {
        error
    } else {
        crate::repository_access::explain(path, error)
    }
}

/// A tab's Open that found its repository gone, as [`NoRepositoryOfItsOwn`]
/// where the tab captured no Git directories (`uncaptured`) and Git finds a
/// repository enclosing its folder. Any other failure is left as it is.
pub(super) fn no_repository_of_its_own(uncaptured: bool, error: anyhow::Error) -> anyhow::Error {
    if !uncaptured {
        return error;
    }
    match error.downcast::<RepositoryGone>() {
        Ok(RepositoryGone {
            path,
            found: Some(found),
        }) if path
            .canonicalize()
            .is_ok_and(|inside| inside != found && inside.starts_with(&found)) =>
        {
            NoRepositoryOfItsOwn { path, found }.into()
        }
        Ok(gone) => gone.into(),
        Err(error) => error,
    }
}

/// A read of `repo` that failed as "not a git repository", as
/// [`RepositoryGone`]: its commands are confined to it, so that is how a
/// read fails once its `.git` went away, and the diagnosis names what Git
/// finds there now. Any other failure stays its own, with no process.
pub(crate) fn confined_failure(repo: &GitRepository, error: anyhow::Error) -> anyhow::Error {
    if error.is::<RepositoryGone>() || !gitturtle_core::finds_no_repository(&error) {
        return error;
    }
    let found = match repo.found_at_path() {
        Ok(gitturtle_core::FoundAtPath::Other(found)) => Some(found),
        Ok(gitturtle_core::FoundAtPath::None) => None,
        Ok(gitturtle_core::FoundAtPath::Same) | Err(_) => return error,
    };
    RepositoryGone {
        path: repo.path().to_owned(),
        found,
    }
    .into()
}

/// Any failure of a read of `repo` as [`RepositoryGone`] where `repo` is
/// gone or no longer the repository at its path, for reads whose steps can
/// fail in other words as its `.git` moves while they run, as the watcher's
/// setup resolving the directories Git reported. A "not a git repository"
/// failure is Git's to decide ([`confined_failure`]): a `.git` half removed
/// (its HEAD deleted, the folder and `config` left) still holds for the
/// filesystem check, which kept the failure its own and the tab unmarked.
/// Any other failure is checked from the filesystem first
/// ([`GitRepository::verify_at_path`]). A repository still there keeps the
/// failure its own. Unlike [`confined_failure`], it checks whatever the
/// failure, so it is for reads that rarely fail, never a hot path.
pub(crate) fn gone_unless_found(repo: &GitRepository, error: anyhow::Error) -> anyhow::Error {
    if error.is::<RepositoryGone>() {
        return error;
    }
    if gitturtle_core::finds_no_repository(&error) {
        return confined_failure(repo, error);
    }
    let found = match repo.verify_at_path() {
        Ok(gitturtle_core::FoundAtPath::Other(found)) => Some(found),
        Ok(gitturtle_core::FoundAtPath::None) => None,
        Ok(gitturtle_core::FoundAtPath::Same) | Err(_) => return error,
    };
    RepositoryGone {
        path: repo.path().to_owned(),
        found,
    }
    .into()
}

/// A quiet read's failed reopen of `repo`, as [`RepositoryGone`] with
/// nothing found where Git now finds no repository at its path, so the tab
/// is read as Refresh reads it, whose Open says why
/// (`repository_access::explain`). Any other failure, as a deadline or an
/// access error, stays its own. Git is asked only once the reopen failed,
/// and not at all when the reopen already says the repository is gone.
pub(super) fn nothing_found_is_gone(repo: &GitRepository, error: anyhow::Error) -> anyhow::Error {
    if error.is::<RepositoryGone>() {
        return error;
    }
    match repo.found_at_path() {
        Ok(gitturtle_core::FoundAtPath::None) => RepositoryGone {
            path: repo.path().to_owned(),
            found: None,
        }
        .into(),
        _ => error,
    }
}

/// A marked tab's Changes read that failed, with why its repository cannot
/// be opened now, in an Open's own words ([`with_current_mark_reason`]). The
/// read's own failure stays whole, for a mark gone by the time it lands.
#[derive(Debug)]
pub(crate) struct MarkedReadFailed {
    pub(crate) reason: String,
    pub(crate) read: anyhow::Error,
}

impl std::fmt::Display for MarkedReadFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#}", self.read)
    }
}

impl std::error::Error for MarkedReadFailed {}

/// A marked tab's failed Changes read, with the reason its Open would give
/// now ([`MarkedReadFailed`]): the reasons only an Open can give
/// (`repository_access::explain`), by the same passive reopen of the tab's
/// captured root, which keeps nothing it opens and can never open another
/// repository there. No reason for a folder that could not be checked, or
/// one the reopen found after all: the mark then stays as it is. Runs on
/// the read's own thread, only once that read failed.
pub(crate) fn with_current_mark_reason(
    error: anyhow::Error,
    path: &Path,
    root: &Path,
    git_dir: Option<&GitDirectories>,
) -> anyhow::Error {
    if gitturtle_core::could_not_check(&error) {
        return error;
    }
    match RepositorySession::default().open(path, Some(root), git_dir) {
        Err(reopen) if !gitturtle_core::could_not_check(&reopen) => MarkedReadFailed {
            reason: format!("{:#}", worded_open_failure(path, reopen)),
            read: error,
        }
        .into(),
        _ => error,
    }
}

#[cfg(test)]
thread_local! {
    /// Runs at each checkpoint of a quiet read's working read on this
    /// thread: a test moves a repository's `.git` part way through.
    pub(super) static QUIET_CHECKPOINT: std::cell::RefCell<Option<Box<dyn FnMut()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn quiet_checkpoint() {
    QUIET_CHECKPOINT.with_borrow_mut(|hook| {
        if let Some(hook) = hook {
            hook();
        }
    });
}

pub(super) struct RetainedRepository {
    canonical_root: PathBuf,
    pub(super) repository: Arc<GitRepository>,
    pub(super) identity: RepositoryIdentity,
}

/// Retain one current worktree session, independently of preview eviction and
/// UI selection. Replacing it and dropping its process-owning Git handle happen
/// on the worker. Linked worktrees never share a session merely because their
/// common object directory is the same: HEAD and local configuration differ.
#[derive(Default)]
pub(super) struct RepositorySession {
    pub(super) current: Option<RetainedRepository>,
    pub(super) history: Option<OrdinaryHistory>,
}

pub(super) struct OrdinaryHistory {
    path: PathBuf,
    pub(super) traversal: gitturtle_core::HistoryTraversal,
    graph: graph::GraphCursor,
}

impl RepositorySession {
    /// The repository at `requested`. An explicit open (`captured` `None`)
    /// takes what Git discovery finds, the work tree enclosing a folder
    /// inside one included. A tab's passive reopen names the root `captured`
    /// when the person opened it, and finds that root or nothing: where Git
    /// now finds another repository at `requested`, its `.git` gone and a
    /// work tree enclosing it, the open fails with [`RepositoryGone`] rather
    /// than move the tab to another repository; where it finds none, it
    /// fails as any open of a folder that is not a repository does.
    ///
    /// The captured root is compared as it was captured, never resolved
    /// again, but for the folders above it ([`FoldersAbove`]): a root since
    /// turned into a link to another repository resolves elsewhere and is
    /// that other repository, gone from the tab, while one reached through
    /// a linked folder above it, as macOS reaches every temporary folder
    /// through `/var`, is the repository it was. At the same root, the Git
    /// directories found must be `git_dir`, the ones the tab captured
    /// there, where it names them (through the same folders): not another
    /// repository's that a swapped `.git` leads to, nor another
    /// repository's refs and objects that a linked worktree's re-pointed
    /// `commondir` leads to. The retained repository answers only where its
    /// own directories are the captured ones; otherwise Git is asked again,
    /// and a mismatch found fails as `RepositoryGone`, naming the directory
    /// that differs. A tab restored from a session also compares the inodes
    /// it saved ([`GitDirectories::restored`]), which the same paths through
    /// a folder above re-pointed at another repository cannot match; it is
    /// the open here that reads them, never the launch.
    pub(super) fn open(
        &mut self,
        requested: &Path,
        captured: Option<&Path>,
        git_dir: Option<&GitDirectories>,
    ) -> Result<GitRepository> {
        let canonical = requested.canonicalize().map_err(|error| {
            gitturtle_core::CoreError::could_not_check_unless_absent(requested, error)
        })?;
        let folders = captured.and_then(|root| FoldersAbove::of(root, &canonical));
        let through = |path: &Path| match &folders {
            Some(folders) => folders.through(path),
            None => path.to_owned(),
        };
        let captured = captured.map(through);
        let git_dir = git_dir.map(|directories| GitDirectories {
            git_dir: through(&directories.git_dir),
            common: through(&directories.common),
            ..*directories
        });
        let git_dir = git_dir.as_ref();
        if self
            .current
            .as_ref()
            .is_some_and(|current| !current.identity.is_current())
        {
            self.current = None;
            self.history = None;
        }
        if let Some(current) = &self.current
            && current.canonical_root == canonical
            && captured.as_ref().is_none_or(|root| *root == canonical)
            && git_dir.is_none_or(|git_dir| git_dir.mismatch(&current.repository).is_none())
        {
            return Ok(current.repository.as_ref().clone());
        }
        // A nested directory needs Git discovery; it may still resolve to the
        // existing root, in which case keep the original persistent reader.
        // Git finding no repository there fails as it always has, with the
        // reasons `repository_access::explain` gives a folder that is not
        // one; only another repository found is `RepositoryGone` (see
        // `refused_open`).
        let passive = captured.is_some();
        let discovered = GitRepository::open(&canonical)
            .map_err(|error| refused_open(requested, passive, error))?;
        let canonical_root = discovered.path().canonicalize()?;
        if captured
            .as_ref()
            .is_some_and(|root| *root != canonical_root)
        {
            return Err(RepositoryGone {
                path: requested.to_owned(),
                found: Some(discovered.path().to_owned()),
            }
            .into());
        }
        if let Some(found) = git_dir.and_then(|git_dir| git_dir.mismatch(&discovered)) {
            return Err(RepositoryGone {
                path: requested.to_owned(),
                found: Some(found),
            }
            .into());
        }
        if let Some(current) = &self.current
            && current.canonical_root == canonical_root
            && GitDirectories::of(&discovered)
                .mismatch(&current.repository)
                .is_none()
        {
            return Ok(current.repository.as_ref().clone());
        }
        let repository = Arc::new(discovered);
        let identity = RepositoryIdentity::capture(&repository)?;
        self.history = None;
        self.current = Some(RetainedRepository {
            canonical_root,
            repository: Arc::clone(&repository),
            identity,
        });
        Ok(repository.as_ref().clone())
    }

    pub(super) fn history_page(
        &mut self,
        repo: &GitRepository,
        scope: &gitturtle_core::HistoryScope,
        offset: usize,
        limit: usize,
        cancellation: &Cancellation,
    ) -> Result<HistoryPageResult> {
        let start = Instant::now();
        let matches = self.history.as_ref().is_some_and(|history| {
            history.path == repo.path()
                && history.traversal.scope() == scope
                && history.traversal.offset() <= offset
        });
        if !matches {
            self.history = None;
            self.history = Some(OrdinaryHistory {
                path: repo.path().to_owned(),
                traversal: repo.history_traversal(scope, &cancellation.history)?,
                graph: graph::GraphCursor::default(),
            });
        }
        let result = (|| {
            let history = self.history.as_mut().expect("History cursor initialized");
            // Back/restore replays a captured stream once, discarding each
            // bounded page and retaining only the ancestry frontier. Sequential
            // browsing does not enter this loop or replay any prior records.
            while history.traversal.offset() < offset {
                cancellation.check()?;
                let count =
                    (offset - history.traversal.offset()).min(gitturtle_core::MAX_HISTORY_PAGE);
                let page = history.traversal.next_page(count, &cancellation.history)?;
                history
                    .graph
                    .append(&page.commits, GRAPH_LANE_LIMIT, GRAPH_EDGE_LIMIT, || {
                        cancellation.check()
                    })?;
                // A position past the history's end, as a restored tab's
                // saved page is where a shallower fetch cut it, reads as
                // the empty page at the end, which tells the window where
                // the history ends (`receive_history_page`).
                if page.next_offset.is_none() {
                    break;
                }
            }
            let page = history.traversal.next_page(
                limit.clamp(1, gitturtle_core::MAX_HISTORY_PAGE),
                &cancellation.history,
            )?;
            let (graph, hidden) =
                history
                    .graph
                    .append(&page.commits, GRAPH_LANE_LIMIT, GRAPH_EDGE_LIMIT, || {
                        cancellation.check()
                    })?;
            cancellation.check()?;
            Ok(HistoryPageResult {
                page,
                graph,
                graph_notice: hidden.then(graph_notice),
                elapsed: start.elapsed(),
            })
        })();
        if result.is_err() {
            self.history = None;
        }
        result
    }
}
