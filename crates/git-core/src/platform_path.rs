//! macOS LaunchServices `PATH` hygiene for child processes.
//!
//! Applications launched from Finder, Dock, or Spotlight inherit a minimal
//! LaunchServices `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin`), which misses
//! Homebrew (`/opt/homebrew/bin`, `/usr/local/bin`) and MacGPG2
//! (`/usr/local/MacGPG2/bin`). Augmenting `PATH` per `Command` (never mutating
//! process-wide environment variables) ensures `git`, `git-lfs`, `gpg`,
//! `pinentry-mac`, `ssh-keygen`, `gh`, editor CLI shims, and custom commands
//! resolve consistently without breaking user-specified `PATH` precedence.

use std::{
    collections::HashSet,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Command,
};

/// Trusted tool directories probed on macOS when LaunchServices omits them.
pub const MACOS_TOOL_DIRS: &[&str] = &[
    "/opt/homebrew/bin",
    "/opt/homebrew/sbin",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/usr/local/MacGPG2/bin",
];

const MACOS_DEFAULT_SYSTEM_DIRS: &[&str] = &["/usr/bin", "/bin", "/usr/sbin", "/sbin"];

/// Computes an augmented macOS `PATH` value from an existing `PATH` without
/// mutating global process state.
///
/// - When `current` is absent, empty, or contains only minimal LaunchServices
///   system directories (`/usr/bin`, `/bin`, `/usr/sbin`, `/sbin`), trusted
///   Homebrew/local/MacGPG2 directories are placed **before** `/usr/bin` so
///   Homebrew `git`, `git-lfs`, `gpg`, and `pinentry-mac` take precedence over
///   Apple's older `/usr/bin/git` stub.
/// - When `current` already contains custom directories (for example interactive
///   shell shims, Nix/mise prefixes, or test overrides), existing order is
///   preserved and missing trusted directories are appended.
/// - Exact duplicate entries are removed while keeping first-seen order.
pub fn augmented_macos_path(current: Option<&OsStr>) -> OsString {
    let existing: Vec<PathBuf> = current
        .map(std::env::split_paths)
        .into_iter()
        .flatten()
        .filter(|path| path.is_absolute())
        .collect();

    let is_minimal_launchservices = existing.is_empty()
        || existing.iter().all(|dir| {
            MACOS_DEFAULT_SYSTEM_DIRS
                .iter()
                .any(|sys| dir == Path::new(sys))
        });

    let mut ordered: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut push_unique = |path: PathBuf| {
        if seen.insert(path.clone()) {
            ordered.push(path);
        }
    };

    if is_minimal_launchservices {
        for dir in MACOS_TOOL_DIRS {
            push_unique(PathBuf::from(dir));
        }
        for dir in existing {
            push_unique(dir);
        }
        for dir in MACOS_DEFAULT_SYSTEM_DIRS {
            push_unique(PathBuf::from(dir));
        }
    } else {
        for dir in existing {
            push_unique(dir);
        }
        for dir in MACOS_TOOL_DIRS {
            push_unique(PathBuf::from(dir));
        }
        for dir in MACOS_DEFAULT_SYSTEM_DIRS {
            push_unique(PathBuf::from(dir));
        }
    }

    std::env::join_paths(ordered).unwrap_or_else(|_| {
        OsString::from(
            "/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/local/sbin:/usr/local/MacGPG2/bin:/usr/bin:/bin:/usr/sbin:/sbin",
        )
    })
}

/// Applies macOS LaunchServices `PATH` hygiene onto `command`.
/// No-op on non-macOS platforms.
pub fn apply_platform_path(command: &mut Command) {
    #[cfg(target_os = "macos")]
    {
        command.env(
            "PATH",
            augmented_macos_path(std::env::var_os("PATH").as_deref()),
        );
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = command;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(path: &OsStr) -> Vec<PathBuf> {
        std::env::split_paths(path).collect()
    }

    #[test]
    fn minimal_launchservices_path_promotes_homebrew_and_macgpg2_before_usr_bin() {
        let out = augmented_macos_path(Some(OsStr::new("/usr/bin:/bin:/usr/sbin:/sbin")));
        let dirs = parts(&out);
        let brew = dirs
            .iter()
            .position(|d| d == Path::new("/opt/homebrew/bin"))
            .expect("homebrew bin present");
        let local = dirs
            .iter()
            .position(|d| d == Path::new("/usr/local/bin"))
            .expect("usr/local/bin present");
        let macgpg = dirs
            .iter()
            .position(|d| d == Path::new("/usr/local/MacGPG2/bin"))
            .expect("MacGPG2 bin present");
        let usr_bin = dirs
            .iter()
            .position(|d| d == Path::new("/usr/bin"))
            .expect("usr/bin present");
        assert!(brew < usr_bin, "homebrew must precede /usr/bin: {dirs:?}");
        assert!(
            local < usr_bin,
            "usr/local/bin must precede /usr/bin: {dirs:?}"
        );
        assert!(macgpg < usr_bin, "MacGPG2 must precede /usr/bin: {dirs:?}");
    }

    #[test]
    fn custom_user_path_preserves_precedence_and_appends_missing_trusted_dirs() {
        let out = augmented_macos_path(Some(OsStr::new(
            "/custom/shims:/usr/bin:/opt/homebrew/bin:/usr/bin",
        )));
        let dirs = parts(&out);
        assert_eq!(dirs[0], PathBuf::from("/custom/shims"));
        assert_eq!(dirs[1], PathBuf::from("/usr/bin"));
        assert_eq!(dirs[2], PathBuf::from("/opt/homebrew/bin"));
        // Deduplicated exact entries while appending missing macOS dirs.
        assert_eq!(
            dirs.iter().filter(|d| *d == Path::new("/usr/bin")).count(),
            1
        );
        assert!(dirs.contains(&PathBuf::from("/usr/local/bin")));
        assert!(dirs.contains(&PathBuf::from("/usr/local/MacGPG2/bin")));
    }

    #[test]
    fn missing_or_empty_path_yields_complete_macos_search_path() {
        for input in [None, Some(OsStr::new(""))] {
            let out = augmented_macos_path(input);
            let dirs = parts(&out);
            assert_eq!(dirs[0], PathBuf::from("/opt/homebrew/bin"));
            assert!(dirs.contains(&PathBuf::from("/usr/bin")));
            assert!(dirs.contains(&PathBuf::from("/sbin")));
        }
    }

    #[test]
    fn relative_and_empty_path_entries_are_dropped() {
        let out = augmented_macos_path(Some(OsStr::new(".:./bin::relative/tools:/usr/bin:/bin")));
        let dirs = parts(&out);
        assert!(dirs.iter().all(|d| d.is_absolute()), "got {dirs:?}");
        assert!(!dirs.contains(&PathBuf::from(".")));
        assert!(!dirs.contains(&PathBuf::from("./bin")));
        assert!(!dirs.contains(&PathBuf::from("relative/tools")));
        // With only system dirs remaining after stripping relative entries,
        // Homebrew takes precedence over /usr/bin.
        assert_eq!(dirs[0], PathBuf::from("/opt/homebrew/bin"));
    }
}
