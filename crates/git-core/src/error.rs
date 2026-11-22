//! Typed crate-boundary errors for `gitturtle-core`.

use std::path::{Path, PathBuf};
use thiserror::Error;

/// Typed boundary errors emitted by `gitturtle-core` so callers can match
/// exhaustively instead of downcasting ad-hoc marker structs.
#[derive(Debug, Error)]
pub enum CoreError {
    /// A repository `GitRepository::open` refused because Git reported a
    /// different repository (or none) once identity files were stamped.
    #[error("{}", format_changed_while_opening(.path, .found.as_deref()))]
    ChangedWhileOpening {
        /// The work-tree root (or bare repository's Git directory) Git reported.
        path: PathBuf,
        /// What Git finds there now, if anything (`Some(path)` when Git still
        /// reports the same repository and only capture failed).
        found: Option<PathBuf>,
    },

    /// A folder's metadata could not be read for a reason other than absence
    /// (e.g. lost search permission or unresponsive mount).
    #[error("Could not read {}", .path.display())]
    CouldNotCheck {
        /// The path whose metadata could not be read.
        path: PathBuf,
        /// Underlying OS I/O error.
        #[source]
        source: std::io::Error,
    },

    /// A repository `GitRepository::open` refused because `core.worktree`
    /// points outside the folder holding its `.git` entry.
    #[error(
        "GitTurtle opens a repository only inside the folder Git found it in: the work tree of the repository at {} is {} (core.worktree).",
        .path.display(),
        .work_tree.display()
    )]
    WorkTreeOutside {
        /// The folder holding the `.git` entry (or bare Git directory).
        path: PathBuf,
        /// The work tree Git reports for it.
        work_tree: PathBuf,
    },

    /// Git reported that no repository holds the requested path.
    #[error("{message}")]
    NotARepository {
        /// Full formatted report (e.g. `"Git rev-parse failed: …"`).
        message: String,
        /// Compact banner summary line, when Git emitted one.
        summary: Option<String>,
    },

    /// A failed Git command's full report paired with its compact banner line.
    #[error("{report}")]
    GitFailure {
        /// Full formatted report for `Details…`.
        report: String,
        /// Compact banner line (core's headline or first redacted `fatal:`/`error:` line).
        summary: String,
    },

    /// A write refused before Git started (for example, repository identity
    /// changed or vanished at the captured path).
    #[error("{0}")]
    WriteRefused(String),

    /// A read stopped because its `HistoryCancellation` token fired.
    #[error("History read cancelled")]
    ReadCancelled,
}

impl CoreError {
    /// Finds the first [`CoreError`] anywhere in `error`'s cause chain.
    pub fn from_anyhow(error: &anyhow::Error) -> Option<&Self> {
        error.chain().find_map(|cause| cause.downcast_ref::<Self>())
    }

    /// Wraps a metadata read failure as [`CoreError::CouldNotCheck`] unless the
    /// OS error simply means nothing exists at `path`.
    pub fn could_not_check_unless_absent(path: &Path, source: std::io::Error) -> anyhow::Error {
        if matches!(
            source.kind(),
            std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
        ) {
            source.into()
        } else {
            Self::CouldNotCheck {
                path: path.to_owned(),
                source,
            }
            .into()
        }
    }

    /// Compact banner summary line carried by this error, if any.
    pub fn summary(&self) -> Option<&str> {
        match self {
            Self::GitFailure { summary, .. } => Some(summary.as_str()),
            Self::NotARepository { summary, .. } => summary.as_deref(),
            Self::ChangedWhileOpening { .. }
            | Self::CouldNotCheck { .. }
            | Self::WorkTreeOutside { .. }
            | Self::WriteRefused(_)
            | Self::ReadCancelled => None,
        }
    }
}

fn format_changed_while_opening(path: &Path, found: Option<&Path>) -> String {
    let suffix = match found {
        Some(found) if found == path => ".".to_owned(),
        Some(found) => format!(": Git now finds {} there.", found.display()),
        None => ": Git now finds no repository there.".to_owned(),
    };
    format!(
        "The repository at {} changed while it was opened{} Open it again.",
        path.display(),
        suffix
    )
}
