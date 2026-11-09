//! Classify observed open failures without guessing a macOS privacy diagnosis.
use std::{io, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    Permission,
    Missing,
    NotRepository,
    Timeout,
    Unreadable,
    Other,
}

fn permission_denied(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::PermissionDenied || matches!(error.raw_os_error(), Some(1 | 13))
}

fn classify(error: &anyhow::Error) -> Failure {
    for cause in error.chain() {
        // Decided from Git's own words, never from the message, which shows
        // them as a terminal leaves them.
        if cause.is::<gitturtle_core::NotARepository>() {
            return Failure::NotRepository;
        }
        // Its own type before the error it carries: "timed out" in that
        // error's words is no Git deadline.
        if let Some(unchecked) = cause.downcast_ref::<gitturtle_core::CouldNotCheck>() {
            return if permission_denied(&unchecked.source) {
                Failure::Permission
            } else {
                Failure::Unreadable
            };
        }
        if let Some(error) = cause.downcast_ref::<io::Error>() {
            if permission_denied(error) {
                return Failure::Permission;
            }
            if error.kind() == io::ErrorKind::NotFound {
                return Failure::Missing;
            }
        }
    }
    let diagnostic = format!("{error:#}").to_ascii_lowercase();
    if diagnostic.contains("permission denied") || diagnostic.contains("operation not permitted") {
        Failure::Permission
    } else if gitturtle_core::says_no_repository(&diagnostic) {
        Failure::NotRepository
    } else if diagnostic.contains("deadline") || diagnostic.contains("timed out") {
        Failure::Timeout
    } else {
        Failure::Other
    }
}

/// Whether an open failed because Git finds no repository at the folder, or
/// the folder is missing: what a tab's passive reopen finds once its
/// repository is gone, which marks the tab as Refresh marks it.
pub(super) fn finds_no_repository(error: &anyhow::Error) -> bool {
    matches!(classify(error), Failure::NotRepository | Failure::Missing)
}

/// Runs on the repository worker; no filesystem probe belongs in UI error handling.
pub(super) fn explain(path: &Path, error: anyhow::Error) -> anyhow::Error {
    let failure = classify(&error);
    let guidance = match failure {
        Failure::Permission => {
            if cfg!(target_os = "macos") {
                "Repository access was denied. Choose the folder with Open Repository (Command-O). If macOS still refuses access, check System Settings → Privacy & Security → Files and Folders for GitTurtle, then retry. A shell and GitTurtle can have different access permissions."
            } else {
                "Repository access was denied. Check folder permissions and access to its Git metadata, then choose the folder again with Open Repository."
            }
        }
        Failure::Missing => {
            if unavailable_volume(path) {
                "The repository's volume is unavailable. Reconnect or mount that volume, then choose the repository with Open Repository."
            } else {
                "The repository folder or a required path is missing. It may have moved or been removed. Choose its current location with Open Repository; saved drafts remain available."
            }
        }
        Failure::NotRepository => {
            "This folder is not an available Git repository. Choose an existing repository or linked worktree with Open Repository. If it moved, check that its Git metadata still points to the correct location."
        }
        Failure::Timeout => {
            if cfg!(target_os = "macos") {
                "Git exceeded the local read deadline. This does not establish a permission failure: slow or unavailable storage can also cause it. Choose the folder again with Open Repository (Command-O), check its volume and any macOS access prompt, then retry. No fetch was requested."
            } else {
                "Git exceeded the local read deadline. Check that the repository's storage is available, then retry. No fetch was requested."
            }
        }
        Failure::Unreadable => {
            "This repository's folder could not be read, so whether it is still there is not known. Check that its volume or network share is available and readable. When the folder can be read again, choose it with Open Repository."
        }
        Failure::Other => {
            "The local repository could not be opened. Review the diagnostic and choose its current folder with Open Repository."
        }
    };
    error.context(format!("{guidance}\nFolder: {}", path.display()))
}

fn unavailable_volume(path: &Path) -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    let Ok(relative) = path.strip_prefix("/Volumes") else {
        return false;
    };
    let Some(volume) = relative.components().next() else {
        return false;
    };
    std::fs::metadata(Path::new("/Volumes").join(volume))
        .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_os_access_error_preserves_evidence() {
        let error =
            anyhow::Error::from(io::Error::from_raw_os_error(13)).context("Opening repository");
        assert_eq!(classify(&error), Failure::Permission);
        let diagnostic = format!("{:#}", explain(Path::new("/example"), error));
        assert!(diagnostic.contains("Permission denied"));
        assert!(diagnostic.contains("/example"));
    }

    /// An open of a folder whose mount point holds a carriage return, as
    /// `GitRepository::open` reports it: the message keeps only what follows
    /// the return, "b)", and the folder is still told apart as holding no
    /// repository, with that guidance.
    #[test]
    fn a_folder_with_no_repository_is_told_apart_by_its_type() {
        let error = anyhow::Error::from(gitturtle_core::NotARepository(
            "Git rev-parse failed: b)".into(),
            None,
        ))
        .context("Opening repository");
        assert_eq!(classify(&error), Failure::NotRepository);
        assert!(finds_no_repository(&error), "{error:#}");
        let diagnostic = format!("{:#}", explain(Path::new("/mnt/a\rb"), error));
        assert!(
            diagnostic.contains("not an available Git repository"),
            "{diagnostic}"
        );
    }

    /// A repository's reads once its `.git` is gone, as a quiet read, Changes'
    /// read, the watcher's setup and History's latest-history read
    /// (`history_updates`) meet them: each failure carries the type
    /// Git's own words decide, so a tab whose repository is gone is told apart
    /// whatever the message keeps of those words (a path Git names holding a
    /// carriage return leaves only what follows it), and the message is
    /// Git's, after ours, as it was.
    #[test]
    fn a_read_whose_repository_is_gone_is_told_apart_by_its_type() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().join("work");
        let repo = gitturtle_core::GitRepository::init(&root, "main").unwrap();
        std::fs::remove_dir_all(root.join(".git")).unwrap();
        let directories = repo.git_directories().expect_err("no repository there");
        let status = repo.status().expect_err("no repository there");
        // History's page of a commit, as its read of a created commit asks
        // for it: the stream is Git's first command there.
        let cancellation = gitturtle_core::HistoryCancellation::default();
        let history = repo
            .history_traversal(
                &gitturtle_core::HistoryScope::FromCommit("1".repeat(40)),
                &cancellation,
            )
            .and_then(|mut traversal| traversal.next_page(1, &cancellation))
            .map(|_| ())
            .expect_err("no repository there");
        for (error, ours) in [
            (directories, "Git rev-parse failed"),
            (status, "Unable to read working-copy status"),
            (history, "Git history read failed"),
        ] {
            assert!(error.is::<gitturtle_core::NotARepository>(), "{error:#}");
            assert!(finds_no_repository(&error), "{error:#}");
            assert_eq!(
                format!("{error:#}"),
                format!(
                    "{ours}: fatal: not a git repository (or any of the parent directories): .git"
                )
            );
        }
    }

    /// A folder that could not be checked is told apart by its type and
    /// says it could not be read and why, never that it is gone or missing;
    /// "timed out" in the operating system's words is no Git deadline.
    #[test]
    fn a_folder_that_could_not_be_checked_says_so() {
        let unchecked = |code| {
            gitturtle_core::CouldNotCheck::unless_absent(
                Path::new("/mnt/share/project"),
                io::Error::from_raw_os_error(code),
            )
        };
        for code in [libc::EIO, libc::ETIMEDOUT] {
            assert_eq!(classify(&unchecked(code)), Failure::Unreadable, "{code}");
            assert!(!finds_no_repository(&unchecked(code)), "{code}");
            let said = format!(
                "{:#}",
                explain(Path::new("/mnt/share/project"), unchecked(code))
            );
            assert!(said.starts_with("This repository's folder could not be read"), "{said}");
            assert!(said.contains("Could not read /mnt/share/project"), "{said}");
            assert!(!said.contains("gone") && !said.contains("missing"), "{said}");
            // The Projects page shows these words too, and has no Refresh.
            assert!(said.contains("Open Repository") && !said.contains("refresh"), "{said}");
        }
        assert_eq!(classify(&unchecked(libc::EACCES)), Failure::Permission);
        assert!(!finds_no_repository(&unchecked(libc::EACCES)));
    }

    /// Nothing at the path stays the plain error it was, read as missing,
    /// through a typed helper that keeps every other error apart.
    #[test]
    fn nothing_at_the_path_stays_missing() {
        let error = gitturtle_core::CouldNotCheck::unless_absent(
            Path::new("/mnt/share/project"),
            io::Error::from(io::ErrorKind::NotFound),
        );
        assert!(!gitturtle_core::could_not_check(&error), "{error:#}");
        assert_eq!(classify(&error), Failure::Missing);
    }

    /// The words say only what holds wherever they show: a new or cold tab
    /// lands on Projects with no repository, where nothing reads again.
    #[test]
    fn a_folder_that_could_not_be_checked_promises_no_later_read() {
        let said = format!(
            "{:#}",
            explain(
                Path::new("/mnt/share/project"),
                gitturtle_core::CouldNotCheck::unless_absent(
                    Path::new("/mnt/share/project"),
                    io::Error::from_raw_os_error(libc::EIO),
                ),
            )
        );
        assert!(
            said.contains("When the folder can be read again, choose it with Open Repository."),
            "{said}"
        );
        assert!(!said.contains("next active"), "{said}");
    }

    #[test]
    fn deadline_does_not_imply_permission_failure() {
        let error = anyhow::anyhow!("Git read deadline exceeded");
        assert_eq!(classify(&error), Failure::Timeout);
        assert_eq!(
            classify(&anyhow::anyhow!("fatal: not a git repository")),
            Failure::NotRepository
        );
        assert_eq!(
            classify(&anyhow::anyhow!(
                "fatal: gitfile does not point to a valid repository: /a/.git"
            )),
            Failure::NotRepository
        );
        assert_eq!(
            classify(&io::Error::from(io::ErrorKind::NotFound).into()),
            Failure::Missing
        );
    }
}
