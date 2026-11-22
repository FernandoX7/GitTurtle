//! Worker job, output, snapshot, preview content, and working-state types.
use super::cache::PreviewKey;
use super::session::{GitDirectories, confined_failure, ensure_found_at_path};
use crate::{graph, text::PatchPresentation};
use anyhow::Result;
use gitturtle_core::{Branch, Commit, FileChange, GitRepository, Worktree};
use gpui_kit::{RenderImage, SharedString};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

pub struct Snapshot {
    pub repository: GitRepository,
    pub branches: Vec<Branch>,
    pub worktrees: Vec<Worktree>,
    pub commits: Vec<Commit>,
    pub history_scope: gitturtle_core::HistoryScope,
    pub history_offset: usize,
    pub history_next_offset: Option<usize>,
    pub graph: Vec<graph::GraphRow>,
    pub graph_notice: Option<String>,
    /// Each commit's local and remote branches, HEAD's first, then its tags.
    pub refs: HistoryReferences,
    pub elapsed: Duration,
    /// An explicit Open whose branch or worktree scope no longer resolves
    /// reads All history instead and names the vanished scope here once.
    pub vanished_scope: Option<VanishedScope>,
    /// Whether the repository may be shallow, which a quiet read finds out:
    /// deepening it outside the app gives commits new parents, and can bring
    /// new rows, while every reference and the page stay as they were. Only
    /// a quiet read's snapshot reads it, and one that cannot says `true`.
    pub shallow: bool,
}

/// Every commit's references as History shows them, prepared with the
/// snapshot on the worker: a row reads its commit's and shares their
/// strings, and a frame reads the most any commit has, so neither joins,
/// formats or counts anything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HistoryReferences {
    by_commit: HashMap<String, CommitReferences>,
    /// The most references any commit has, which narrow History's compact
    /// pill is sized for.
    widest: usize,
}

impl HistoryReferences {
    /// Each commit's `branches`, HEAD's first and the others in Git's order,
    /// then its `tags` in Git's order, by name. A tag names the commit it
    /// peels to; one of a tree, a blob or another tag names no row.
    pub(super) fn new(branches: &[Branch], tags: &[gitturtle_core::Tag]) -> Self {
        let mut names: HashMap<&str, (Vec<&str>, Vec<&str>)> = HashMap::new();
        for branch in branches {
            let (branches, _) = names.entry(&branch.oid).or_default();
            if branch.current {
                branches.insert(0, &branch.name);
            } else {
                branches.push(&branch.name);
            }
        }
        for tag in tags.iter().filter(|tag| tag.target_kind == "commit") {
            names.entry(&tag.target_oid).or_default().1.push(&tag.name);
        }
        let by_commit: HashMap<_, _> = names
            .into_iter()
            .map(|(oid, (branches, tags))| {
                (oid.to_owned(), CommitReferences::new(&branches, &tags))
            })
            .collect();
        Self {
            widest: by_commit
                .values()
                .map(|refs| refs.names.len())
                .max()
                .unwrap_or(0),
            by_commit,
        }
    }

    /// The references of the commit `oid`, if it has any.
    pub fn get(&self, oid: &str) -> Option<&CommitReferences> {
        self.by_commit.get(oid)
    }

    /// The most references any commit has.
    pub fn widest(&self) -> usize {
        self.widest
    }

    /// Every commit's references.
    #[cfg(test)]
    pub fn commits(&self) -> impl Iterator<Item = &CommitReferences> {
        self.by_commit.values()
    }

    /// The bytes these references keep, for a retained tab's accounting.
    pub fn retained_bytes(&self) -> usize {
        self.by_commit
            .iter()
            .map(|(oid, refs)| oid.capacity() + refs.retained_bytes())
            .sum()
    }
}

/// One commit's references as History shows them, prepared with the
/// snapshot on the worker, so a row's badge, pill, tooltip and accessible
/// name share these strings rather than joining them per row and frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommitReferences {
    /// Its branches, HEAD's first and the others in Git's order, then its
    /// tags.
    pub names: Vec<SharedString>,
    /// How many of `names`, from the first, are branches.
    pub branches: usize,
    /// "+N" for the names after the first, which the badge counts.
    pub more: Option<SharedString>,
    /// Every name on its own line, a tag's as "tag ‹name›": the badge's and
    /// the pill's tooltip.
    pub lines: SharedString,
    /// Every name, comma-separated, a tag's as "tag ‹name›", as the row's
    /// accessible name says them.
    pub listed: SharedString,
}

impl CommitReferences {
    fn new(branches: &[&str], tags: &[&str]) -> Self {
        let count = branches.len() + tags.len();
        // A tag is said as one, so it reads apart from a branch of its name.
        let said: Vec<String> = branches
            .iter()
            .map(|name| (*name).to_owned())
            .chain(tags.iter().map(|name| format!("tag {name}")))
            .collect();
        Self {
            more: (count > 1).then(|| format!("+{}", count - 1).into()),
            lines: said.join("\n").into(),
            listed: said.join(", ").into(),
            branches: branches.len(),
            names: branches
                .iter()
                .chain(tags)
                .map(|name| SharedString::from((*name).to_owned()))
                .collect(),
        }
    }

    /// Whether the first name, which the badge and the pill show, is a tag.
    pub fn leads_with_tag(&self) -> bool {
        self.branches == 0
    }

    /// The bytes these strings keep, for a retained tab's accounting.
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.names.capacity() * std::mem::size_of::<SharedString>()
            + self.names.iter().map(|name| name.len()).sum::<usize>()
            + self.more.as_ref().map_or(0, |more| more.len())
            + self.lines.len()
            + self.listed.len()
    }
}

/// Decoded image metadata retained alongside the BGRA [`RenderImage`].
/// Pixel bytes live solely in `RenderImage`, avoiding a second full-resolution
/// RGBA buffer in the preview cache.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageMetadata {
    pub width: u32,
    pub height: u32,
    pub original_width: u32,
    pub original_height: u32,
    pub format: String,
}

pub struct ImageSide {
    pub animation: Option<Arc<crate::gif_playback::Timeline>>,
    pub captured: Option<Arc<[u8]>>,
    /// Whether the captured bytes offer the open action, decided with them.
    pub system_preview: Option<crate::system_preview::Offer>,
    pub literal_source: Option<Arc<str>>,
    pub image: Option<ImageMetadata>,
    /// BGRA conversion happens in place on the worker; the UI only shares this allocation.
    pub render: Option<Arc<RenderImage>>,
    pub message: Option<String>,
    pub bytes: usize,
    /// Retain only a bounded missing pointer so an explicit download can review
    /// the exact object. Normal preview construction remains entirely passive.
    pub lfs_pointer: Option<Vec<u8>>,
}

impl ImageSide {
    /// Whether its literal source is a Git LFS pointer's text: the one
    /// literal source kept with no captured bytes to open.
    pub(crate) fn literal_is_pointer(&self) -> bool {
        self.literal_source.is_some() && self.captured.is_none()
    }
}

pub enum Content {
    Rich(crate::rich_preview::Comparison),
    Conflict(Arc<crate::conflicts::Presentation>),
    Text {
        diagrams: Option<Arc<crate::rich_preview::Comparison>>,
        markdown: Option<Arc<crate::markdown_view::Comparison>>,
        patch: String,
        old: String,
        new: String,
        presentation: Arc<PatchPresentation>,
        split: Option<Arc<crate::split_diff::SplitPresentation>>,
        partial: Option<Arc<crate::partial_view::PartialActions>>,
        partial_unavailable: Option<String>,
        /// Which sides, old then new, are Git LFS pointers whose object the
        /// local store does not hold, as the reading found them: a pointer's
        /// text cannot say, since its object can be local and unreadable as
        /// text, and only a missing one is called missing or offered.
        lfs_missing: [bool; 2],
        /// Which sides, old then new, are Git LFS pointers whose object could
        /// not be used, and why: each says so in its own words and offers no
        /// download, which could not help.
        lfs_failed: [Option<gitturtle_core::LfsObjectFailure>; 2],
    },
    Images {
        old: Box<ImageSide>,
        new: Box<ImageSide>,
    },
    Notice(String),
}

impl Content {
    /// Whether this preview holds a Git LFS pointer whose local store state can
    /// change (missing object that may arrive, or failed object other than TooLarge).
    pub(crate) fn has_changeable_lfs(&self) -> bool {
        match self {
            Self::Images { old, new } => {
                old.lfs_pointer.is_some()
                    || new.lfs_pointer.is_some()
                    || old.literal_is_pointer()
                    || new.literal_is_pointer()
            }
            Self::Rich(preview) => {
                preview.old.lfs_missing
                    || preview.new.lfs_missing
                    || (preview.old.error.is_some()
                        && preview.old.metadata.format == "Git LFS pointer")
                    || (preview.new.error.is_some()
                        && preview.new.metadata.format == "Git LFS pointer")
            }
            Self::Text {
                lfs_missing,
                lfs_failed,
                ..
            } => {
                lfs_missing.contains(&true)
                    || lfs_failed
                        .iter()
                        .flatten()
                        .any(|failure| *failure != gitturtle_core::LfsObjectFailure::TooLarge)
            }
            _ => false,
        }
    }

    /// CPU allocations retained by a cache entry. UI-held Arcs and uploaded GPU
    /// textures have independent lifetimes and need their own presentation budget.
    pub(crate) fn bytes(&self) -> usize {
        match self {
            Self::Rich(preview) => preview.retained_bytes(),
            Self::Text {
                diagrams,
                markdown,
                patch,
                old,
                new,
                presentation,
                split,
                partial,
                partial_unavailable,
                lfs_missing: _,
                lfs_failed: _,
            } => {
                diagrams
                    .as_ref()
                    .map_or(0, |preview| preview.retained_bytes())
                    + markdown
                        .as_ref()
                        .map_or(0, |preview| preview.retained_bytes())
                    + patch.capacity()
                    + old.capacity()
                    + new.capacity()
                    + presentation.retained_bytes()
                    + split.as_ref().map_or(0, |split| split.retained_bytes())
                    + partial.as_ref().map_or(0, |p| p.bytes())
                    + partial_unavailable.as_ref().map_or(0, String::capacity)
            }
            Self::Images { old, new } => [old, new]
                .iter()
                .map(|side| {
                    std::mem::size_of::<ImageSide>()
                        + side
                            .image
                            .as_ref()
                            .map_or(0, |image| image.format.capacity())
                        + side.render.as_ref().map_or(0, |render| {
                            (0..render.frame_count())
                                .map(|frame| render.as_bytes(frame).map_or(0, <[u8]>::len))
                                .sum::<usize>()
                        })
                        + side
                            .animation
                            .as_ref()
                            .map_or(0, |timeline| timeline.retained_bytes())
                        + side.message.as_ref().map_or(0, String::capacity)
                        + side.lfs_pointer.as_ref().map_or(0, Vec::capacity)
                        + side.captured.as_ref().map_or(0, |bytes| bytes.len())
                        + side
                            .literal_source
                            .as_ref()
                            .map_or(0, |source| source.len())
                })
                .sum(),
            Self::Conflict(presentation) => presentation.bytes(),
            Self::Notice(message) => message.capacity(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    Branch { name: String, remote: bool },
    Worktree { path: PathBuf },
}

/// A branch or worktree scope that no longer resolves in the current local
/// snapshot. Explicit Open replaces it with All history; quiet refresh and an
/// unpinned search report it while keeping the displayed history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VanishedScope(pub Scope);

impl std::fmt::Display for VanishedScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Scope::Branch { name, remote } => write!(
                f,
                "{} branch '{name}' no longer exists in the local repository snapshot",
                if *remote { "Remote-tracking" } else { "Local" }
            ),
            Scope::Worktree { path } => write!(
                f,
                "Worktree '{}' is no longer registered in the local repository snapshot",
                path.display()
            ),
        }
    }
}

impl std::error::Error for VanishedScope {}

/// What a snapshot read does when its branch or worktree scope is gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MissingScope {
    /// Fail the read; quiet refresh then retains the displayed history.
    Fail,
    /// Read All history and record the vanished scope on the snapshot.
    AllHistory,
}

pub enum Job {
    /// Drop the idle history process on the worker when leaving its tab.
    ReleaseHistory,
    HistoryPage {
        repo: GitRepository,
        scope: gitturtle_core::HistoryScope,
        offset: usize,
        limit: usize,
    },
    ReviewText {
        content: Arc<Content>,
        options: crate::text_review::Options,
        prepare_split: bool,
    },
    PrepareSplit {
        content: Arc<Content>,
        cache_key: Option<PreviewKey>,
    },
    CompareRevisions {
        repo: GitRepository,
        before: String,
        after: String,
        mode: gitturtle_core::ComparisonMode,
    },
    RevisionTargets {
        repo: GitRepository,
    },
    TrackedPaths {
        repo: GitRepository,
        scope: gitturtle_core::PathScope,
        query: String,
    },
    TrackedPreview {
        repo: GitRepository,
        entry: gitturtle_core::TrackedPath,
        scope: gitturtle_core::PathScope,
    },
    Blame {
        repo: GitRepository,
        target: gitturtle_core::BlameTarget,
    },
    LineHistory {
        repo: GitRepository,
        oid: String,
        path: PathBuf,
        line: usize,
    },
    SearchHistory {
        repo: GitRepository,
        scope: Option<Scope>,
        pinned: Option<gitturtle_core::HistoryScope>,
        query: String,
        offset: usize,
        limit: usize,
        previous: Vec<(String, Vec<String>)>,
        remaining_bytes: usize,
    },
    FileHistory {
        repo: GitRepository,
        anchor: String,
        path: PathBuf,
        offset: usize,
        limit: usize,
    },
    QuietRefresh {
        repo: GitRepository,
        scope: Option<Scope>,
        limit: usize,
        history: bool,
        selected: Option<WorkingSelection>,
    },
    WorkingPreview {
        repo: GitRepository,
        entry: gitturtle_core::StatusEntry,
        area: gitturtle_core::ChangeArea,
        head: Option<String>,
        prepare_split: bool,
    },
    Open {
        path: PathBuf,
        scope: Option<Scope>,
        limit: usize,
        /// The work-tree root a tab's passive reopen (Refresh, Show latest,
        /// a scope chosen, a restored tab, a worktree's row) must find at
        /// `path` again, captured when the person opened the tab; `None`
        /// for an explicit open, which may land on the work tree enclosing
        /// `path` (`RepositorySession::open`).
        root: Option<PathBuf>,
        /// The Git directories the tab captured at `root` when the person
        /// opened it, which a passive reopen must find there again: a `.git`
        /// swapped for another repository's gitfile, or a `commondir`
        /// re-pointed, is another repository. `None` for an explicit open
        /// and a worktree's row.
        git_dir: Option<GitDirectories>,
    },
    Changes {
        repo: GitRepository,
        oid: String,
        parent: usize,
    },
    Preview {
        repo: GitRepository,
        file: FileChange,
        origins: crate::markdown_view::Origins,
        prepare_split: bool,
    },
}

pub enum Output {
    HistoryReleased,
    HistoryPage(HistoryPageResult),
    ReviewText(Arc<Content>, Duration),
    SplitPrepared(Arc<Content>),
    RevisionComparison(gitturtle_core::RevisionComparison),
    RevisionTargets(Vec<String>),
    TrackedPaths(gitturtle_core::TrackedPaths),
    Blame(gitturtle_core::Blame),
    LineHistory(gitturtle_core::LineHistory),
    SearchHistory(SearchResult),
    FileHistory(gitturtle_core::FileHistoryPage),
    QuietRefresh(Box<QuietRefresh>),
    WorkingPreview(FileChange, Arc<Content>, Duration),
    /// A Quick Open source's read (`Job::TrackedPreview`): its content and
    /// the file's mode as read, the working file's own where the source is
    /// the working tree, where a `chmod +x` shows before anything is staged.
    TrackedPreview {
        mode: String,
        content: Arc<Content>,
        elapsed: Duration,
    },
    Snapshot(Snapshot),
    Changes(Vec<FileChange>, Duration),
    Preview(Arc<Content>, Duration),
}

pub struct HistoryPageResult {
    pub page: gitturtle_core::HistoryTraversalPage,
    pub graph: Vec<graph::GraphRow>,
    pub graph_notice: Option<String>,
    pub elapsed: Duration,
}

pub struct SearchResult {
    pub page: gitturtle_core::HistorySearchPage,
    pub graph: Vec<graph::GraphRow>,
    pub graph_notice: Option<String>,
    pub retained_bytes: usize,
}

pub struct WorkingState {
    pub status: gitturtle_core::RepositoryStatus,
    pub profile: Result<gitturtle_core::GitProfile>,
    pub remotes: Result<Vec<gitturtle_core::Remote>>,
    pub operation: Result<Option<gitturtle_core::OperationState>>,
    /// The Git directories of the handle that read this state, the one the
    /// window held as it asked for the read (`refresh_worktree`,
    /// `try_automatic_refresh`): the shown status is labelled with them
    /// (`GitTurtle::working_identity`), never with whichever handle the
    /// window holds as the read lands.
    pub read_from: GitDirectories,
}

impl WorkingState {
    /// The working state of the shown tab's repository, read on the
    /// operation thread, as Changes' Refresh and a write's reload read it.
    /// A repository no longer at its path fails with [`RepositoryGone`]:
    /// checked from the filesystem before the read, starting no process
    /// (`GitRepository::verify_at_path`), and again from the read's own
    /// failure, since every command is confined to the repository and, its
    /// `.git` gone, fails as "not a git repository" instead of reading the
    /// work tree enclosing it.
    pub fn read(repo: &GitRepository) -> Result<Self> {
        ensure_found_at_path(repo)?;
        let state = Self::read_with_checkpoint(repo, || Ok(()))
            .map_err(|error| confined_failure(repo, error))?;
        state.confirmed(repo)
    }

    /// This state, unless a part of it failed because its repository went
    /// away while it was read: its `.git` gone after the status, a later
    /// part fails as "not a git repository", and the repository gone is the
    /// answer ([`RepositoryGone`]), not an "Operation state: …" banner
    /// beside the tab's mark. Checked from the filesystem, and only when a
    /// part failed.
    pub(super) fn confirmed(self, repo: &GitRepository) -> Result<Self> {
        if self.operation.is_err() || self.profile.is_err() || self.remotes.is_err() {
            ensure_found_at_path(repo)?;
        }
        Ok(self)
    }

    pub(super) fn read_with_checkpoint(
        repo: &GitRepository,
        checkpoint: impl Fn() -> Result<()>,
    ) -> Result<Self> {
        checkpoint()?;
        let status = repo.status()?;
        checkpoint()?;
        let profile = repo.profile();
        checkpoint()?;
        let remotes = repo.remotes();
        checkpoint()?;
        let operation = repo.operation_state();
        checkpoint()?;
        Ok(Self {
            status,
            profile,
            remotes,
            operation,
            read_from: GitDirectories::of(repo),
        })
    }
}

pub struct WorkingSelection {
    pub path: PathBuf,
    pub area: gitturtle_core::ChangeArea,
    pub file: Option<FileChange>,
    pub content: Option<Arc<Content>>,
    pub prepare_split: bool,
}

pub struct QuietRefresh {
    pub working: WorkingState,
    pub snapshot: Option<Result<QuietHistory>>,
    pub preview: Option<Result<QuietPreview>>,
}

pub enum QuietHistory {
    Refreshed(Snapshot),
    /// A branch/worktree scope can disappear externally. Its existing immutable
    /// history remains usable while fresh branch and worktree metadata is shown.
    Retained {
        metadata: Snapshot,
        error: String,
        /// The line of `error` that says what went wrong, where its failure
        /// carries one (`gitturtle_core::failure_summary`).
        summary: Option<String>,
        /// The scope that vanished, when that is why the read failed; the
        /// view reports one vanished scope once.
        vanished: Option<VanishedScope>,
    },
}

pub enum QuietPreview {
    Unchanged,
    Absent,
    Changed {
        file: FileChange,
        content: Arc<Content>,
    },
}
