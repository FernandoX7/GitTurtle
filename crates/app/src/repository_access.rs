//! Classify observed open failures without guessing a macOS privacy diagnosis.
use std::{io, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    Permission,
    Missing,
    NotRepository,
    Timeout,
    Other,
}

fn classify(error: &anyhow::Error) -> Failure {
    for cause in error.chain() {
        if let Some(error) = cause.downcast_ref::<io::Error>() {
            if error.kind() == io::ErrorKind::PermissionDenied
                || matches!(error.raw_os_error(), Some(1 | 13))
            {
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
    } else if diagnostic.contains("not a git repository") {
        Failure::NotRepository
    } else if diagnostic.contains("deadline") || diagnostic.contains("timed out") {
        Failure::Timeout
    } else {
        Failure::Other
    }
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

    #[test]
    fn deadline_does_not_imply_permission_failure() {
        let error = anyhow::anyhow!("Git read deadline exceeded");
        assert_eq!(classify(&error), Failure::Timeout);
        assert_eq!(
            classify(&anyhow::anyhow!("fatal: not a git repository")),
            Failure::NotRepository
        );
        assert_eq!(
            classify(&io::Error::from(io::ErrorKind::NotFound).into()),
            Failure::Missing
        );
    }
}
