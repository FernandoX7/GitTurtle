use gitturtle_core::{ChangeArea, ChangeStatus, GitRepository, TextPreview, WriteCommand};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::TempDir;

struct Fixture {
    temp: TempDir,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("work");
        GitRepository::init(&root, "main").unwrap();
        let f = Self { temp, root };
        f.git(&["config", "user.name", "Fixture Author"]);
        f.git(&["config", "user.email", "fixture@example.invalid"]);
        f.git(&["config", "commit.gpgsign", "false"]);
        f.git(&["config", "core.hooksPath", ".git/hooks"]);
        f
    }
    fn git(&self, args: &[&str]) -> String {
        git(&self.root, args)
    }
    fn repo(&self) -> GitRepository {
        GitRepository::open(&self.root).unwrap()
    }
    fn write(&self, path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    fn commit(&self, message: &str) -> String {
        let repo = self.repo();
        repo.execute(&WriteCommand::StageAll).unwrap();
        repo.execute(&WriteCommand::Commit {
            message: message.into(),
        })
        .unwrap()
        .commit_oid
        .unwrap()
    }
}
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim_end().into()
}

#[test]
fn stage_commit_preserves_unstaged_content_and_roundtrips_history() {
    let f = Fixture::new();
    f.write("file.txt", "baseline\n");
    f.commit("first");
    f.write("file.txt", "staged version\n");
    let repo = f.repo();
    repo.execute(&WriteCommand::Stage {
        paths: vec!["file.txt".into()],
    })
    .unwrap();
    f.write("file.txt", "later work\n");
    let state = repo.status().unwrap();
    let file = &state.entries[0];
    assert_eq!(file.staged, Some(ChangeStatus::Modified));
    assert_eq!(file.unstaged, Some(ChangeStatus::Modified));
    let staged = repo.worktree_preview(file, ChangeArea::Staged).unwrap();
    assert_eq!(staged.old, b"baseline\n");
    assert_eq!(staged.new, b"staged version\n");
    let unstaged = repo.worktree_preview(file, ChangeArea::Unstaged).unwrap();
    assert_eq!(unstaged.old, b"staged version\n");
    assert_eq!(unstaged.new, b"later work\n");
    let oid = repo
        .execute(&WriteCommand::Commit {
            message: "Keep staged content\n\nA useful body.".into(),
        })
        .unwrap()
        .commit_oid
        .unwrap();
    assert_eq!(f.git(&["show", "HEAD:file.txt"]), "staged version");
    assert_eq!(fs::read(f.root.join("file.txt")).unwrap(), b"later work\n");
    assert_eq!(repo.history(1).unwrap()[0].oid, oid);
    assert_eq!(repo.history(1).unwrap()[0].subject, "Keep staged content");
    assert!(repo.status().unwrap().entries[0].staged.is_none());
}

#[test]
fn unborn_unstage_retains_files_and_identity_is_repository_local() {
    let f = Fixture::new();
    let repo = f.repo();
    repo.execute(&WriteCommand::SetIdentity {
        name: "New Author".into(),
        email: "new@example.invalid".into(),
    })
    .unwrap();
    let profile = repo.profile().unwrap();
    assert_eq!(profile.name, "New Author");
    assert_eq!(profile.email, "new@example.invalid");
    assert!(!profile.signing);
    assert_eq!(
        f.git(&["config", "--local", "user.email"]),
        "new@example.invalid"
    );
    f.write("new.txt", "first draft");
    f.write("other.txt", "other");
    repo.execute(&WriteCommand::StageAll).unwrap();
    repo.execute(&WriteCommand::Unstage {
        paths: vec!["new.txt".into()],
    })
    .unwrap();
    assert_eq!(f.git(&["ls-files"]), "other.txt");
    assert!(f.root.join("new.txt").exists());
    repo.execute(&WriteCommand::UnstageAll).unwrap();
    assert!(f.git(&["ls-files"]).is_empty());
    let state = repo.status().unwrap();
    assert_eq!(state.branch.as_deref(), Some("main"));
    assert!(state.head.is_none());
    assert!(state.entries.iter().all(|e| e.untracked));
}

#[test]
fn rename_unstage_restores_index_without_reversing_working_rename() {
    let f = Fixture::new();
    f.write("before.txt", "contents\n");
    f.commit("first");
    fs::rename(f.root.join("before.txt"), f.root.join("after.txt")).unwrap();
    let repo = f.repo();
    repo.execute(&WriteCommand::StageAll).unwrap();
    let state = repo.status().unwrap();
    let renamed = &state.entries[0];
    assert_eq!(renamed.staged, Some(ChangeStatus::Renamed));
    assert_eq!(
        renamed.original_path.as_deref(),
        Some(Path::new("before.txt"))
    );
    assert_eq!(
        repo.worktree_preview(renamed, ChangeArea::Staged)
            .unwrap()
            .old,
        b"contents\n"
    );
    repo.execute(&WriteCommand::Unstage {
        paths: renamed.paths(),
    })
    .unwrap();
    assert_eq!(f.git(&["ls-files"]), "before.txt");
    assert!(f.root.join("after.txt").exists());
    assert!(!f.root.join("before.txt").exists());
}

#[cfg(unix)]
#[test]
fn filenames_are_literal_and_byte_preserving_in_status_stage_and_unstage() {
    #[cfg(target_os = "linux")]
    use std::os::unix::ffi::OsStringExt;
    let f = Fixture::new();
    let paths: Vec<PathBuf> = vec![
        "-option.txt".into(),
        "[ab]*.txt".into(),
        "line\nbreak.txt".into(),
    ];
    #[cfg(target_os = "linux")]
    let paths = {
        let mut paths = paths;
        paths.push(std::ffi::OsString::from_vec(b"invalid-\xff.txt".to_vec()).into());
        paths
    };
    for p in &paths {
        f.write(p, "unique");
    }
    f.write("aZZ.txt", "unrelated");
    let repo = f.repo();
    let status = repo.status().unwrap();
    for p in &paths {
        assert!(status.entries.iter().any(|e| &e.path == p));
    }
    repo.execute(&WriteCommand::Stage {
        paths: paths.clone(),
    })
    .unwrap();
    assert!(
        repo.status()
            .unwrap()
            .entries
            .iter()
            .find(|e| e.path == Path::new("aZZ.txt"))
            .unwrap()
            .staged
            .is_none()
    );
    repo.execute(&WriteCommand::Commit {
        message: "Literal filenames".into(),
    })
    .unwrap();
    for p in &paths {
        f.write(p, "next");
    }
    repo.execute(&WriteCommand::Stage {
        paths: paths.clone(),
    })
    .unwrap();
    repo.execute(&WriteCommand::Unstage { paths }).unwrap();
    assert!(
        repo.status()
            .unwrap()
            .entries
            .iter()
            .all(|e| e.staged.is_none())
    );
}

#[cfg(unix)]
#[test]
fn working_preview_reads_link_targets_and_rejects_symlink_directory_escape() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    f.write("nested/file.txt", "tracked");
    f.commit("first");
    let outside = f.temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("file.txt"), "private outside content").unwrap();
    fs::remove_dir_all(f.root.join("nested")).unwrap();
    symlink(&outside, f.root.join("nested")).unwrap();
    symlink("/private/should-never-be-followed", f.root.join("link")).unwrap();
    let repo = f.repo();
    let state = repo.status().unwrap();
    let link = state
        .entries
        .iter()
        .find(|e| e.path == Path::new("link"))
        .unwrap();
    let preview = repo.worktree_preview(link, ChangeArea::Unstaged).unwrap();
    assert_eq!(preview.new, b"/private/should-never-be-followed");
    assert_eq!(preview.file.new_mode, "120000");
    // Use a legitimately captured pre-replacement status entry, then replace its
    // directory before activation. This exercises descriptor-relative traversal.
    fs::remove_file(f.root.join("nested")).unwrap();
    f.write("nested/file.txt", "changed");
    let entry = repo
        .status()
        .unwrap()
        .entries
        .into_iter()
        .find(|e| e.path == Path::new("nested/file.txt"))
        .unwrap();
    fs::remove_dir_all(f.root.join("nested")).unwrap();
    symlink(&outside, f.root.join("nested")).unwrap();
    assert!(repo.worktree_preview(&entry, ChangeArea::Unstaged).is_err());
}

#[cfg(unix)]
#[test]
fn configured_filters_and_hooks_are_preserved_only_for_explicit_writes() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    f.write(".gitattributes", "*.txt filter=upper\n");
    f.git(&["config", "filter.upper.clean", "tr '[:lower:]' '[:upper:]'"]);
    f.git(&["config", "filter.upper.smudge", "cat"]);
    f.write(
        ".git/hooks/pre-commit",
        "#!/bin/sh\nprintf 'called' > .git/hook-called\n",
    );
    fs::set_permissions(
        f.root.join(".git/hooks/pre-commit"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    f.write("file.txt", "lower case\n");
    let repo = f.repo();
    repo.execute(&WriteCommand::StageAll).unwrap();
    let staged = repo
        .status()
        .unwrap()
        .entries
        .into_iter()
        .find(|e| e.path == Path::new("file.txt"))
        .unwrap();
    assert_eq!(
        repo.worktree_preview(&staged, ChangeArea::Staged)
            .unwrap()
            .new,
        b"LOWER CASE\n"
    );
    repo.execute(&WriteCommand::Commit {
        message: "Run hook".into(),
    })
    .unwrap();
    assert_eq!(
        fs::read(f.root.join(".git/hook-called")).unwrap(),
        b"called"
    );
    f.write(
        ".git/hooks/pre-commit",
        "#!/bin/sh\necho 'fixture hook rejection' >&2\nexit 1\n",
    );
    f.write("file.txt", "new content\n");
    repo.execute(&WriteCommand::StageAll).unwrap();
    let before = f.git(&["rev-parse", "HEAD"]);
    let error = repo
        .execute(&WriteCommand::Commit {
            message: "Rejected".into(),
        })
        .unwrap_err();
    assert!(error.to_string().contains("fixture hook rejection"));
    assert_eq!(f.git(&["rev-parse", "HEAD"]), before);
}

#[test]
fn branch_creation_checkout_and_refusal_preserve_local_work() {
    let f = Fixture::new();
    f.write("file.txt", "main content");
    f.commit("main");
    let repo = f.repo();
    repo.execute(&WriteCommand::CreateBranch {
        name: "feature/work".into(),
        start_point: Some("refs/heads/main".into()),
    })
    .unwrap();
    assert_eq!(
        repo.status().unwrap().branch.as_deref(),
        Some("feature/work")
    );
    f.write("file.txt", "feature content");
    f.commit("feature");
    repo.execute(&WriteCommand::Checkout {
        branch: "main".into(),
    })
    .unwrap();
    f.write("file.txt", "uncommitted main work");
    assert!(
        repo.execute(&WriteCommand::Checkout {
            branch: "feature/work".into()
        })
        .is_err()
    );
    assert_eq!(repo.status().unwrap().branch.as_deref(), Some("main"));
    assert_eq!(
        fs::read(f.root.join("file.txt")).unwrap(),
        b"uncommitted main work"
    );
    for name in ["--detach", "bad:name", "@{-1}"] {
        assert!(
            repo.execute(&WriteCommand::CreateBranch {
                name: name.into(),
                start_point: None
            })
            .is_err()
        );
    }
}

#[test]
fn clone_fetch_fast_forward_pull_and_non_force_push_use_local_remote() {
    let f = Fixture::new();
    f.write("file.txt", "one");
    f.commit("one");
    let remote = f.temp.path().join("remote.git");
    git(
        f.temp.path(),
        &[
            "init",
            "--bare",
            "--initial-branch=main",
            remote.to_str().unwrap(),
        ],
    );
    f.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
    let repo = f.repo();
    let push = WriteCommand::Push {
        remote: "origin".into(),
        local_branch: "main".into(),
        remote_branch: "main".into(),
    };
    repo.execute(&push).unwrap();
    assert_eq!(repo.remotes().unwrap()[0].url, remote.to_str().unwrap());
    let copy = f.temp.path().join("clone");
    let cloned = GitRepository::clone_repository(remote.to_str().unwrap(), &copy).unwrap();
    git(&copy, &["config", "user.name", "Clone Author"]);
    git(&copy, &["config", "user.email", "clone@example.invalid"]);
    git(&copy, &["config", "commit.gpgsign", "false"]);
    git(&copy, &["config", "core.hooksPath", ".git/hooks"]);
    assert_eq!(cloned.history(1).unwrap()[0].subject, "one");
    f.write("file.txt", "two");
    f.commit("two");
    repo.execute(&push).unwrap();
    cloned
        .execute(&WriteCommand::Fetch {
            remote: "origin".into(),
        })
        .unwrap();
    assert_eq!(cloned.status().unwrap().behind, 1);
    cloned
        .execute(&WriteCommand::Pull {
            remote: "origin".into(),
            branch: "main".into(),
        })
        .unwrap();
    assert_eq!(fs::read(copy.join("file.txt")).unwrap(), b"two");
    fs::write(copy.join("clone.txt"), "clone change").unwrap();
    cloned.execute(&WriteCommand::StageAll).unwrap();
    cloned
        .execute(&WriteCommand::Commit {
            message: "clone three".into(),
        })
        .unwrap();
    cloned.execute(&push).unwrap();
    f.write("local.txt", "diverging");
    f.commit("local three");
    assert!(repo.execute(&push).is_err()); // No force and no silent retry.
    let head = f.git(&["rev-parse", "HEAD"]);
    assert!(
        repo.execute(&WriteCommand::Pull {
            remote: "origin".into(),
            branch: "main".into()
        })
        .is_err()
    );
    assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(git(&remote, &["log", "-1", "--format=%s"]), "clone three");
}

#[test]
fn init_and_clone_refuse_populated_destinations() {
    let f = Fixture::new();
    f.write("important", "keep me");
    assert!(GitRepository::init(&f.root, "main").is_err());
    assert!(GitRepository::clone_repository(f.root.to_str().unwrap(), &f.root).is_err());
    assert_eq!(fs::read(f.root.join("important")).unwrap(), b"keep me");
    let empty = f.temp.path().join("empty");
    fs::create_dir(&empty).unwrap();
    assert!(GitRepository::init(&empty, "custom-start").is_ok());
    assert_eq!(
        git(&empty, &["symbolic-ref", "HEAD"]),
        "refs/heads/custom-start"
    );
}

#[test]
fn conflicts_are_explicit_and_can_be_staged_after_manual_resolution() {
    let f = Fixture::new();
    f.write("file.txt", "base\n");
    f.commit("base");
    f.git(&["switch", "-c", "other"]);
    f.write("file.txt", "other\n");
    f.commit("other");
    f.git(&["switch", "main"]);
    f.write("file.txt", "main\n");
    f.commit("main");
    let result = Command::new("git")
        .arg("-C")
        .arg(&f.root)
        .args(["merge", "other"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    let repo = f.repo();
    let state = repo.status().unwrap();
    assert_eq!(state.operation.as_deref(), Some("Merge"));
    assert!(state.entries[0].conflicted);
    assert!(
        repo.worktree_preview(&state.entries[0], ChangeArea::Unstaged)
            .is_err()
    );
    f.write("file.txt", "resolved\n");
    repo.execute(&WriteCommand::Stage {
        paths: state.entries[0].paths(),
    })
    .unwrap();
    let oid = repo
        .execute(&WriteCommand::Commit {
            message: "Resolve both branches".into(),
        })
        .unwrap()
        .commit_oid
        .unwrap();
    assert_eq!(repo.history_from(&oid, 1).unwrap()[0].parents.len(), 2);
    assert!(repo.status().unwrap().operation.is_none());
}

#[test]
fn working_binary_and_deleted_sides_are_distinguished() {
    let f = Fixture::new();
    f.write("image.png", b"\x89PNG\0old");
    f.commit("old image");
    let repo = f.repo();
    f.write("image.png", b"\x89PNG\0new");
    let entry = repo.status().unwrap().entries.remove(0);
    let preview = repo.worktree_preview(&entry, ChangeArea::Unstaged).unwrap();
    assert_eq!(preview.preview, TextPreview::Binary);
    assert_eq!(preview.old, b"\x89PNG\0old");
    assert_eq!(preview.new, b"\x89PNG\0new");
    fs::remove_file(f.root.join("image.png")).unwrap();
    let entry = repo.status().unwrap().entries.remove(0);
    let preview = repo.worktree_preview(&entry, ChangeArea::Unstaged).unwrap();
    assert!(preview.file.new_path.is_none());
    assert!(preview.new.is_empty());
    assert!(!preview.old.is_empty());
}

#[cfg(unix)]
#[test]
fn passive_status_disables_clean_filters_and_does_not_refresh_index() {
    let f = Fixture::new();
    f.write(".gitattributes", "*.txt filter=hostile\n");
    f.write("file.txt", "base\n");
    f.commit("base");
    f.git(&[
        "config",
        "filter.hostile.clean",
        "touch .git/clean-ran; cat",
    ]);
    f.git(&["config", "filter.hostile.required", "true"]);
    f.write("file.txt", "dirty\n");
    let index = fs::read(f.root.join(".git/index")).unwrap();
    let repo = f.repo();
    let status = repo.status().unwrap();
    assert!(
        status
            .entries
            .iter()
            .any(|e| e.path == Path::new("file.txt"))
    );
    assert!(!f.root.join(".git/clean-ran").exists());
    assert_eq!(fs::read(f.root.join(".git/index")).unwrap(), index);
    repo.execute(&WriteCommand::Stage {
        paths: vec!["file.txt".into()],
    })
    .unwrap();
    assert!(f.root.join(".git/clean-ran").exists());
}

#[cfg(unix)]
#[test]
fn configured_signing_failure_is_reported_instead_of_creating_unsigned_commit() {
    let f = Fixture::new();
    f.write("file.txt", "first");
    let repo = f.repo();
    repo.execute(&WriteCommand::StageAll).unwrap();
    f.git(&["config", "commit.gpgsign", "true"]);
    f.git(&["config", "gpg.program", "/usr/bin/false"]);
    assert!(repo.profile().unwrap().signing);
    assert!(
        repo.execute(&WriteCommand::Commit {
            message: "Must sign".into()
        })
        .is_err()
    );
    assert!(repo.history(5).unwrap().is_empty());
    assert!(
        repo.status()
            .unwrap()
            .entries
            .iter()
            .any(|e| e.staged.is_some())
    );
}

#[test]
fn linked_worktree_status_index_branch_and_identity_remain_private() {
    let f = Fixture::new();
    f.write("file.txt", "base");
    f.commit("base");
    let linked = f.temp.path().join("linked");
    f.git(&["worktree", "add", "-b", "linked", linked.to_str().unwrap()]);
    f.git(&["config", "extensions.worktreeConfig", "true"]);
    let repo = GitRepository::open(&linked).unwrap();
    repo.execute(&WriteCommand::SetIdentity {
        name: "Linked Author".into(),
        email: "linked@example.invalid".into(),
    })
    .unwrap();
    assert_eq!(repo.profile().unwrap().name, "Linked Author");
    assert_eq!(f.repo().profile().unwrap().name, "Fixture Author");
    fs::write(linked.join("file.txt"), "linked change").unwrap();
    repo.execute(&WriteCommand::StageAll).unwrap();
    assert!(f.repo().status().unwrap().entries.is_empty());
    assert_eq!(repo.status().unwrap().branch.as_deref(), Some("linked"));
    repo.execute(&WriteCommand::Commit {
        message: "Private working copy".into(),
    })
    .unwrap();
    assert_eq!(f.git(&["log", "-1", "--format=%s"]), "base");
    assert_eq!(
        repo.history_from(&git(&linked, &["rev-parse", "HEAD"]), 1)
            .unwrap()[0]
            .author,
        "Linked Author"
    );
}
