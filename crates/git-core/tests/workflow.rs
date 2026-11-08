use gitturtle_core::{
    BlameTarget, ChangeArea, ChangeStatus, FoundAtPath, GitRepository, HistoryCancellation,
    TextPreview, WriteCommand,
};
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
    assert!(error.to_string().contains("configured hooks"));
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
    // A branch's first push records the destination as its upstream.
    assert_eq!(f.git(&["config", "branch.main.remote"]), "origin");
    assert_eq!(f.git(&["config", "branch.main.merge"]), "refs/heads/main");
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
    let push_error = repo.execute(&push).unwrap_err().to_string(); // No force and no silent retry.
    assert!(!push_error.starts_with("Branches have diverged"));
    let head = f.git(&["rev-parse", "HEAD"]);
    let staged = f.git(&["ls-files", "--stage"]);
    f.write("file.txt", "independent unstaged work\n");
    f.write("untracked-note.txt", "independent untracked work\n");
    let error = repo
        .execute(&WriteCommand::Pull {
            remote: "origin".into(),
            branch: "main".into(),
        })
        .unwrap_err();
    let diagnostic = format!("{error:#}");
    assert!(
        diagnostic.starts_with("Branches have diverged; choose Merge or Rebase to continue."),
        "{diagnostic}"
    );
    assert!(diagnostic.contains("Git stderr:\n"), "{diagnostic}");
    assert!(diagnostic.contains("\nFrom "), "{diagnostic}");
    assert!(
        diagnostic.contains("fatal: Not possible to fast-forward, aborting."),
        "{diagnostic}"
    );
    assert!(diagnostic.contains("Git stdout:"));
    assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(f.git(&["ls-files", "--stage"]), staged);
    assert_eq!(
        fs::read(f.root.join("file.txt")).unwrap(),
        b"independent unstaged work\n"
    );
    assert_eq!(
        fs::read(f.root.join("untracked-note.txt")).unwrap(),
        b"independent untracked work\n"
    );
    assert!(repo.operation_state().unwrap().is_none());
    assert_eq!(git(&remote, &["log", "-1", "--format=%s"]), "clone three");
}

/// Fetch and Pull say whether they brought anything new from the refs before
/// and after Git ran: a Fetch from any ref it updated, a Pull from whether
/// HEAD moved, so a Pull after a Fetch that already brought the commits still
/// reports what it fast-forwarded.
#[test]
fn fetch_and_pull_report_whether_they_brought_anything_new() {
    let f = Fixture::new();
    f.write("file.txt", "one");
    f.commit("one");
    let remote = bare_remote(&f, "origin");
    let repo = f.repo();
    repo.execute(&push("origin", "main")).unwrap();
    let copy = f.temp.path().join("clone");
    let cloned = GitRepository::clone_repository(remote.to_str().unwrap(), &copy).unwrap();
    for (key, value) in [
        ("user.name", "Clone Author"),
        ("user.email", "clone@example.invalid"),
        ("commit.gpgsign", "false"),
        ("core.hooksPath", ".git/hooks"),
    ] {
        git(&copy, &["config", key, value]);
    }
    let fetch = WriteCommand::Fetch {
        remote: "origin".into(),
    };
    let pull = WriteCommand::Pull {
        remote: "origin".into(),
        branch: "main".into(),
    };
    // A remote added and pushed to rather than cloned has no
    // `refs/remotes/origin/HEAD`, which Git's fetch then creates
    // (`remote.origin.followRemoteHEAD`): an alias of a branch it already
    // has, so nothing new.
    let aliases = || git(&f.root, &["for-each-ref", "refs/remotes/origin/HEAD"]);
    assert_eq!(aliases(), "");
    assert!(repo.execute(&fetch).unwrap().up_to_date);
    let refs = || {
        git(
            &copy,
            &["for-each-ref", "--format=%(objectname) %(refname)"],
        )
    };
    let head = || git(&copy, &["rev-parse", "HEAD"]);

    // Nothing new on the remote: both are up to date and change no ref.
    let before = refs();
    assert!(cloned.execute(&fetch).unwrap().up_to_date);
    assert!(cloned.execute(&pull).unwrap().up_to_date);
    assert_eq!(refs(), before);

    // A new commit on main: Fetch brings it, and the Pull after it, whose own
    // fetch brings nothing more, still fast-forwards HEAD.
    f.write("file.txt", "two");
    let two = f.commit("two");
    repo.execute(&push("origin", "main")).unwrap();
    let fetched = cloned.execute(&fetch).unwrap();
    assert!(!fetched.up_to_date, "{fetched:?}");
    assert_eq!(git(&copy, &["rev-parse", "refs/remotes/origin/main"]), two);
    assert!(cloned.execute(&fetch).unwrap().up_to_date);
    let pulled = cloned.execute(&pull).unwrap();
    assert!(!pulled.up_to_date, "{pulled:?}");
    assert_eq!(head(), two);
    assert!(cloned.execute(&pull).unwrap().up_to_date);

    // A Pull straight from the remote, with no Fetch first, also moves HEAD.
    f.write("file.txt", "three");
    let three = f.commit("three");
    repo.execute(&push("origin", "main")).unwrap();
    assert!(!cloned.execute(&pull).unwrap().up_to_date);
    assert_eq!(head(), three);

    // A new branch elsewhere on the remote is new to a Fetch, while a Pull
    // of main, which HEAD already contains, brings nothing new.
    f.git(&["switch", "--create", "feature"]);
    f.write("feature.txt", "feature");
    f.commit("feature");
    repo.execute(&push("origin", "feature")).unwrap();
    assert!(!cloned.execute(&fetch).unwrap().up_to_date);
    assert!(git(&copy, &["for-each-ref", "refs/remotes/origin/feature"]).contains("feature"));
    assert!(cloned.execute(&pull).unwrap().up_to_date);
    assert_eq!(head(), three);
    // The Pull left FETCH_HEAD listing main alone and this Fetch lists both
    // branches again, all of them stored in refs: nothing new.
    let fetch_head = copy.join(".git/FETCH_HEAD");
    let after_pull = fs::read_to_string(&fetch_head).unwrap();
    assert!(cloned.execute(&fetch).unwrap().up_to_date);
    assert_ne!(fs::read_to_string(&fetch_head).unwrap(), after_pull);

    // A local commit ahead of the remote: Git has nothing to pull.
    fs::write(copy.join("local.txt"), "local").unwrap();
    cloned.execute(&WriteCommand::StageAll).unwrap();
    let local = cloned
        .execute(&WriteCommand::Commit {
            message: "local".into(),
        })
        .unwrap();
    assert!(!local.up_to_date, "only Fetch and Pull report it");
    assert!(cloned.execute(&pull).unwrap().up_to_date);
    assert_eq!(Some(head()), local.commit_oid);
}

/// A remote with no `remote.<name>.fetch` refspec stores nothing in refs:
/// Git's Fetch brings the remote's HEAD into FETCH_HEAD alone, so the
/// commits FETCH_HEAD lists decide whether it brought anything new. A
/// FETCH_HEAD that is a symbolic link is never followed, and the Fetch then
/// reads as new rather than up to date.
#[test]
fn a_fetch_that_stores_no_ref_reports_new_commits_from_fetch_head() {
    let f = Fixture::new();
    f.write("file.txt", "one");
    let one = f.commit("one");
    let remote = bare_remote(&f, "origin");
    let repo = f.repo();
    repo.execute(&push("origin", "main")).unwrap();
    let plain = f.temp.path().join("plain");
    let fetcher = GitRepository::init(&plain, "main").unwrap();
    git(
        &plain,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&plain, &["config", "--unset-all", "remote.origin.fetch"]);
    let fetch = WriteCommand::Fetch {
        remote: "origin".into(),
    };
    let refs = || git(&plain, &["for-each-ref"]);
    let fetch_head = plain.join(".git/FETCH_HEAD");
    let listed = || fs::read_to_string(&fetch_head).unwrap();

    // The first Fetch brings `one`, which no ref names.
    let fetched = fetcher.execute(&fetch).unwrap();
    assert!(!fetched.up_to_date, "{fetched:?}");
    assert_eq!(refs(), "");
    assert!(listed().starts_with(&one), "{}", listed());
    assert!(fetcher.execute(&fetch).unwrap().up_to_date);

    // A new commit arrives in FETCH_HEAD alone, while the refs stay empty.
    f.write("file.txt", "two");
    let two = f.commit("two");
    repo.execute(&push("origin", "main")).unwrap();
    let fetched = fetcher.execute(&fetch).unwrap();
    assert!(!fetched.up_to_date, "{fetched:?}");
    assert_eq!(refs(), "");
    assert!(listed().starts_with(&two), "{}", listed());
    git(&plain, &["cat-file", "-e", &format!("{two}^{{commit}}")]);
    assert!(fetcher.execute(&fetch).unwrap().up_to_date);

    #[cfg(unix)]
    {
        // Git writes through the link, so its target, unchanged by a Fetch
        // that brings nothing, cannot say that nothing arrived.
        let target = f.temp.path().join("fetch-head-target");
        fs::rename(&fetch_head, &target).unwrap();
        std::os::unix::fs::symlink(&target, &fetch_head).unwrap();
        let fetched = fetcher.execute(&fetch).unwrap();
        assert!(!fetched.up_to_date, "{fetched:?}");
        let link = fs::symlink_metadata(&fetch_head).unwrap();
        assert!(link.file_type().is_symlink());
        assert!(fs::read_to_string(&target).unwrap().starts_with(&two));
    }
}

/// A repository whose `.git` is a symbolic link to its directory, as Git
/// follows it, reads FETCH_HEAD through that link: a Fetch that brings
/// nothing is up to date. Its FETCH_HEAD as a symbolic link still fails
/// closed. Before, the directory's no-follow open failed, so every Fetch
/// after the first read as new.
#[cfg(unix)]
#[test]
fn a_fetch_through_a_linked_git_directory_reads_its_fetch_head() {
    let f = Fixture::new();
    f.write("file.txt", "one");
    f.commit("one");
    bare_remote(&f, "origin");
    f.repo().execute(&push("origin", "main")).unwrap();
    let directory = f.temp.path().join("linked-git-directory");
    fs::rename(f.root.join(".git"), &directory).unwrap();
    std::os::unix::fs::symlink(&directory, f.root.join(".git")).unwrap();
    let repo = f.repo();
    let fetch = WriteCommand::Fetch {
        remote: "origin".into(),
    };

    repo.execute(&fetch).unwrap();
    let fetch_head = directory.join("FETCH_HEAD");
    assert!(fs::symlink_metadata(&fetch_head).unwrap().is_file());
    let fetched = repo.execute(&fetch).unwrap();
    assert!(fetched.up_to_date, "{fetched:?}");

    let target = f.temp.path().join("fetch-head-target");
    fs::rename(&fetch_head, &target).unwrap();
    std::os::unix::fs::symlink(&target, &fetch_head).unwrap();
    let fetched = repo.execute(&fetch).unwrap();
    assert!(!fetched.up_to_date, "{fetched:?}");
}

fn bare_remote(f: &Fixture, name: &str) -> PathBuf {
    let remote = f.temp.path().join(format!("{name}.git"));
    git(
        f.temp.path(),
        &[
            "init",
            "--bare",
            "--initial-branch=main",
            remote.to_str().unwrap(),
        ],
    );
    f.git(&["remote", "add", name, remote.to_str().unwrap()]);
    remote
}
fn push(remote: &str, branch: &str) -> WriteCommand {
    WriteCommand::Push {
        remote: remote.into(),
        local_branch: branch.into(),
        remote_branch: branch.into(),
    }
}
fn upstream_config(f: &Fixture, branch: &str) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(&f.root)
        .args(["config", "--get-regexp", &format!("^branch\\.{branch}\\.")])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim_end().into()
}

#[test]
fn push_to_another_remote_keeps_an_existing_upstream() {
    let f = Fixture::new();
    f.write("file.txt", "one");
    f.commit("one");
    let origin = bare_remote(&f, "origin");
    let lan = bare_remote(&f, "lan");
    let repo = f.repo();
    repo.execute(&push("origin", "main")).unwrap();
    assert_eq!(
        f.git(&["rev-parse", "--abbrev-ref", "main@{upstream}"]),
        "origin/main"
    );
    f.write("file.txt", "two");
    let tip = f.commit("two");
    repo.execute(&push("lan", "main")).unwrap();
    assert_eq!(git(&lan, &["rev-parse", "refs/heads/main"]), tip);
    assert_ne!(git(&origin, &["rev-parse", "refs/heads/main"]), tip);
    assert_eq!(f.git(&["config", "branch.main.remote"]), "origin");
    assert_eq!(f.git(&["config", "branch.main.merge"]), "refs/heads/main");
    assert_eq!(
        f.git(&["rev-parse", "--abbrev-ref", "main@{upstream}"]),
        "origin/main"
    );
    // Pushing to the upstream itself still works and leaves the configuration.
    repo.execute(&push("origin", "main")).unwrap();
    assert_eq!(git(&origin, &["rev-parse", "refs/heads/main"]), tip);
    assert_eq!(
        upstream_config(&f, "main"),
        "branch.main.remote origin\nbranch.main.merge refs/heads/main"
    );
}

#[test]
fn push_sets_upstream_for_a_branch_without_one_only_after_success() {
    let f = Fixture::new();
    f.write("file.txt", "one");
    f.commit("one");
    let lan = bare_remote(&f, "lan");
    let repo = f.repo();
    // The remote already has a diverging main, so this push is rejected.
    let other = f.temp.path().join("other");
    git(
        f.temp.path(),
        &["clone", lan.to_str().unwrap(), other.to_str().unwrap()],
    );
    fs::write(other.join("other.txt"), "other").unwrap();
    git(&other, &["add", "other.txt"]);
    git(
        &other,
        &[
            "-c",
            "user.name=Other",
            "-c",
            "user.email=other@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "other",
        ],
    );
    git(&other, &["push", "origin", "HEAD:refs/heads/main"]);
    let remote_tip = git(&lan, &["rev-parse", "refs/heads/main"]);
    assert!(repo.execute(&push("lan", "main")).is_err());
    assert_eq!(git(&lan, &["rev-parse", "refs/heads/main"]), remote_tip);
    assert_eq!(upstream_config(&f, "main"), "");
    // A successful first push of a new branch records its upstream.
    f.git(&["switch", "--create", "topic"]);
    f.write("topic.txt", "topic");
    let tip = f.commit("topic");
    repo.execute(&push("lan", "topic")).unwrap();
    assert_eq!(git(&lan, &["rev-parse", "refs/heads/topic"]), tip);
    assert_eq!(
        upstream_config(&f, "topic"),
        "branch.topic.remote lan\nbranch.topic.merge refs/heads/topic"
    );
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
    // Pin the format too: an inherited `gpg.format=ssh` would bypass this signer.
    f.git(&["config", "gpg.format", "openpgp"]);
    f.git(&["config", "gpg.program", "/usr/bin/false"]);
    assert!(repo.profile().unwrap().signing);
    let error = repo
        .execute(&WriteCommand::Commit {
            message: "Must sign".into(),
        })
        .unwrap_err()
        .to_string();
    assert!(error.contains("Signing remains enabled"), "{error}");
    assert!(repo.history(5).unwrap().is_empty());
    assert!(
        repo.status()
            .unwrap()
            .entries
            .iter()
            .any(|e| e.staged.is_some())
    );
}

#[cfg(unix)]
#[test]
fn ssh_failures_keep_configured_program_and_provide_safe_recovery_guidance() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    f.write("tracked", "base\n");
    let head = f.commit("base");
    f.write("tracked", "local work\n");
    let program = f.temp.path().join("ssh-fixture");
    f.git(&["config", "core.sshCommand", program.to_str().unwrap()]);
    f.git(&[
        "remote",
        "add",
        "origin",
        "ssh://fixture.invalid/repository",
    ]);
    for (diagnostic, guidance) in [
        (
            "Permission denied (publickey).",
            "configured SSH identity and agent",
        ),
        ("Host key verification failed.", "only after verification"),
    ] {
        fs::write(
            &program,
            format!("#!/bin/sh\nprintf '%s\\n' '{diagnostic}' >&2\nexit 1\n"),
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        let error = f
            .repo()
            .execute(&WriteCommand::Fetch {
                remote: "origin".into(),
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains(diagnostic), "{error}");
        assert!(error.contains(guidance), "{error}");
        assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
        assert!(f.git(&["diff", "--cached"]).is_empty());
        assert_eq!(fs::read(f.root.join("tracked")).unwrap(), b"local work\n");
    }
}

#[cfg(unix)]
#[test]
fn local_http_authentication_failure_reports_helper_recovery_without_prompting() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };
    let f = Fixture::new();
    f.write("tracked", "base\n");
    let head = f.commit("base");
    let server = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    server.set_nonblocking(true).unwrap();
    let address = server.local_addr().unwrap();
    let stopped = Arc::new(AtomicBool::new(false));
    let stop = Arc::clone(&stopped);
    let worker = std::thread::spawn(move || {
        let start = Instant::now();
        while !stop.load(Ordering::Relaxed) && start.elapsed() < Duration::from_secs(5) {
            match server.accept() {
                Ok((mut socket, _)) => {
                    // macOS accept(2) inherits the listener's O_NONBLOCK; read the
                    // request before answering so closing cannot reset the connection.
                    socket.set_nonblocking(false).unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    let _ = socket.read(&mut [0; 4096]);
                    let _ = socket.write_all(b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=fixture\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(error) => panic!("{error}"),
            }
        }
    });
    f.git(&["config", "credential.helper", ""]);
    f.git(&["config", "http.proxy", ""]);
    f.git(&[
        "config",
        "--add",
        "credential.helper",
        "gitturtle-fixture-missing-helper",
    ]);
    f.git(&[
        "remote",
        "add",
        "origin",
        &format!("http://{address}/repository"),
    ]);
    let result = f.repo().execute(&WriteCommand::Fetch {
        remote: "origin".into(),
    });
    stopped.store(true, Ordering::Relaxed);
    worker.join().unwrap();
    let error = result.unwrap_err().to_string();
    assert!(error.contains("credential.helper"), "{error}");
    assert!(error.contains("could not run"), "{error}");
    assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
    assert!(f.git(&["diff", "--cached"]).is_empty());
    assert_eq!(fs::read(f.root.join("tracked")).unwrap(), b"base\n");
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

/// A write runs at its repository's path, so with the repository's `.git`
/// gone Git would find the work tree enclosing that path and write there:
/// Stage all once staged the inner folder into the enclosing repository's
/// index, and Commit committed it. Each write asks first and refuses,
/// naming what Git finds there now, before Git starts.
#[test]
fn a_write_refuses_once_its_repository_is_gone_from_its_path() {
    let f = Fixture::new();
    f.write("outer.txt", "outer\n");
    f.commit("outer");
    let inner = f.root.join("inner");
    GitRepository::init(&inner, "main").unwrap();
    git(&inner, &["config", "user.name", "Inner Author"]);
    git(&inner, &["config", "user.email", "inner@example.invalid"]);
    git(&inner, &["config", "commit.gpgsign", "false"]);
    fs::write(inner.join("inner.txt"), "inner\n").unwrap();
    let repo = GitRepository::open(&inner).unwrap();
    assert_eq!(repo.found_at_path().unwrap(), FoundAtPath::Same);
    repo.execute(&WriteCommand::StageAll).unwrap();
    repo.execute(&WriteCommand::Commit {
        message: "inner".into(),
    })
    .unwrap();

    fs::remove_dir_all(inner.join(".git")).unwrap();
    fs::write(inner.join("inner.txt"), "later\n").unwrap();
    let outer = f.repo();
    assert_eq!(
        repo.found_at_path().unwrap(),
        FoundAtPath::Other(outer.path().to_owned())
    );
    for command in [
        WriteCommand::StageAll,
        WriteCommand::Commit {
            message: "into the enclosing repository".into(),
        },
    ] {
        let error = format!("{:#}", repo.execute(&command).unwrap_err());
        assert!(
            error.contains("The repository changed")
                && error.contains(&outer.path().display().to_string())
                && error.contains("Git was not started"),
            "{error}"
        );
    }
    assert!(f.git(&["diff", "--cached", "--name-only"]).is_empty());
    assert_eq!(f.git(&["log", "--format=%s"]), "outer");

    fs::remove_dir_all(&inner).unwrap();
    assert_eq!(repo.found_at_path().unwrap(), FoundAtPath::None);
    let error = format!("{:#}", repo.execute(&WriteCommand::StageAll).unwrap_err());
    assert!(error.contains("finds no repository there"), "{error}");
}

/// A nested repository `inner` inside the fixture's work tree, whose file
/// `shared.txt` the enclosing repository also tracks, with one commit each.
fn nested_repository(f: &Fixture) -> PathBuf {
    f.write("inner/shared.txt", "outer line\n");
    f.commit("outer");
    let inner = f.root.join("inner");
    git(&inner, &["init", "--quiet", "--initial-branch=main"]);
    git(&inner, &["config", "user.name", "Inner Author"]);
    git(&inner, &["config", "user.email", "inner@example.invalid"]);
    git(&inner, &["config", "commit.gpgsign", "false"]);
    fs::write(inner.join("shared.txt"), "inner line\n").unwrap();
    git(&inner, &["add", "shared.txt"]);
    git(&inner, &["commit", "--quiet", "-m", "inner"]);
    inner
}

/// Every command of an opened repository is confined to it: with its
/// `.git` gone, Git stops with "not a git repository" instead of reading
/// the work tree enclosing it, as the old handle's status, history and
/// attribution once did, each showing the enclosing repository's state.
#[test]
fn an_opened_repository_s_reads_never_reach_the_enclosing_repository() {
    let f = Fixture::new();
    let inner = nested_repository(&f);
    let repo = GitRepository::open(&inner).unwrap();
    assert_eq!(repo.history_page(0, 10).unwrap()[0].subject, "inner");

    fs::rename(inner.join(".git"), f.temp.path().join("inner-git-away")).unwrap();
    fs::write(inner.join("shared.txt"), "changed\n").unwrap();
    let not_a_repository = |error: anyhow::Error| {
        let error = format!("{error:#}");
        assert!(error.contains("not a git repository"), "{error}");
    };
    not_a_repository(repo.status().map(|_| ()).unwrap_err());
    not_a_repository(repo.history_page(0, 10).map(|_| ()).unwrap_err());
    not_a_repository(
        repo.blame(
            &BlameTarget::Working {
                path: "shared.txt".into(),
            },
            &HistoryCancellation::default(),
        )
        .map(|_| ())
        .unwrap_err(),
    );
    // The diagnosis, unconfined, still names what Git finds there.
    assert_eq!(
        repo.verify_at_path().unwrap(),
        FoundAtPath::Other(f.repo().path().to_owned())
    );
}

/// The confinement's ceiling is the parent of the folder holding `.git`,
/// so every layout Git discovers from the repository's root still works:
/// a linked worktree, a repository opened from a subfolder, one whose
/// `core.worktree` is a folder inside the one holding `.git`, and a
/// submodule; a hook's `git -C ..` still finds the enclosing repository,
/// and it sees the ceiling.
#[cfg(unix)]
#[test]
fn confined_commands_keep_every_layout_git_discovers_from_the_root() {
    let f = Fixture::new();
    let inner = nested_repository(&f);
    let outer_root = f.repo().path().to_owned();

    let linked = f.temp.path().join("linked");
    git(
        &inner,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ],
    );
    let repo = GitRepository::open(&linked).unwrap();
    fs::write(linked.join("shared.txt"), "linked\n").unwrap();
    repo.execute(&WriteCommand::StageAll).unwrap();
    repo.execute(&WriteCommand::Commit {
        message: "linked".into(),
    })
    .unwrap();
    assert_eq!(repo.status().unwrap().branch.as_deref(), Some("linked"));
    assert_eq!(repo.verify_at_path().unwrap(), FoundAtPath::Same);

    fs::create_dir(inner.join("sub")).unwrap();
    let repo = GitRepository::open(inner.join("sub")).unwrap();
    assert_eq!(repo.path(), inner.canonicalize().unwrap());
    assert_eq!(repo.status().unwrap().branch.as_deref(), Some("main"));

    // core.worktree a folder inside the one holding `.git`: a ceiling at
    // the root's parent would stop Git before it reached that `.git`.
    let holder = f.temp.path().join("holder");
    git(
        f.temp.path(),
        &[
            "init",
            "--quiet",
            "--initial-branch=main",
            holder.to_str().unwrap(),
        ],
    );
    let worktree = holder.join("tree");
    fs::create_dir(&worktree).unwrap();
    git(
        &holder,
        &["config", "core.worktree", worktree.to_str().unwrap()],
    );
    git(&holder, &["config", "user.name", "Redirected Author"]);
    git(
        &holder,
        &["config", "user.email", "redirected@example.invalid"],
    );
    git(&holder, &["config", "commit.gpgsign", "false"]);
    fs::write(worktree.join("file.txt"), "redirected\n").unwrap();
    let repo = GitRepository::open(&worktree).unwrap();
    assert_eq!(repo.path(), worktree.canonicalize().unwrap());
    repo.execute(&WriteCommand::StageAll).unwrap();
    repo.execute(&WriteCommand::Commit {
        message: "redirected".into(),
    })
    .unwrap();
    assert!(repo.status().unwrap().entries.is_empty());
    assert_eq!(repo.verify_at_path().unwrap(), FoundAtPath::Same);

    let module = Fixture::new();
    module.write("module.txt", "module\n");
    module.commit("module");
    f.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        "--quiet",
        module.root.to_str().unwrap(),
        "module",
    ]);
    let checkout = f.root.join("module");
    fs::write(checkout.join("module.txt"), "changed in the checkout\n").unwrap();
    let repo = GitRepository::open(&checkout).unwrap();
    assert_eq!(repo.verify_at_path().unwrap(), FoundAtPath::Same);
    assert_eq!(repo.status().unwrap().entries.len(), 1);
    assert_eq!(repo.history_page(0, 1).unwrap()[0].subject, "module");

    // A hook climbing above the repository's root still reaches the
    // enclosing repository; Git never treats the folder it starts in as
    // above a ceiling.
    let hook = inner.join(".git/hooks/pre-commit");
    fs::write(
        &hook,
        "#!/bin/sh\ngit -C .. rev-parse --show-toplevel > ../hook-found\nprintf '%s' \"$GIT_CEILING_DIRECTORIES\" > ../hook-ceiling\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    let repo = GitRepository::open(&inner).unwrap();
    fs::write(inner.join("shared.txt"), "hooked\n").unwrap();
    repo.execute(&WriteCommand::StageAll).unwrap();
    repo.execute(&WriteCommand::Commit {
        message: "hooked".into(),
    })
    .unwrap();
    assert_eq!(
        fs::read_to_string(f.root.join("hook-found"))
            .unwrap()
            .trim_end(),
        outer_root.to_str().unwrap()
    );
    let ceiling = fs::read_to_string(f.root.join("hook-ceiling")).unwrap();
    assert!(
        ceiling.ends_with(outer_root.to_str().unwrap()),
        "the ceiling is the parent of the folder holding .git: {ceiling}"
    );
}

/// The repository's identity is its root and the Git directory Git
/// reported at open, compared as captured: a `.git` swapped for another
/// repository's gitfile, or the root turned into a link to another
/// repository, is another repository, and a write refuses before Git
/// starts, leaving that repository's index alone. A clone made again at
/// the same path is the same root and `.git` path, and keeps working.
#[cfg(unix)]
#[test]
fn a_swapped_git_entry_or_a_linked_root_is_another_repository() {
    let f = Fixture::new();
    f.write("file.txt", "base\n");
    f.commit("base");
    let other = Fixture::new();
    other.write("file.txt", "other\n");
    other.commit("other");
    let other_git = other.repo().git_dir().to_owned();
    let repo = f.repo();
    assert_eq!(repo.git_dir(), f.root.join(".git").canonicalize().unwrap());
    f.write("file.txt", "changed\n");
    let refused = |repo: &GitRepository| {
        let error = format!("{:#}", repo.execute(&WriteCommand::StageAll).unwrap_err());
        assert!(
            error.contains("The repository changed") && error.contains("Git was not started"),
            "{error}"
        );
        assert!(other.git(&["diff", "--cached", "--name-only"]).is_empty());
    };

    let away = f.temp.path().join("git-away");
    fs::rename(f.root.join(".git"), &away).unwrap();
    fs::write(
        f.root.join(".git"),
        format!("gitdir: {}\n", other_git.display()),
    )
    .unwrap();
    assert_eq!(
        repo.verify_at_path().unwrap(),
        FoundAtPath::Other(other_git.clone())
    );
    refused(&repo);
    fs::remove_file(f.root.join(".git")).unwrap();
    fs::rename(&away, f.root.join(".git")).unwrap();
    assert_eq!(repo.verify_at_path().unwrap(), FoundAtPath::Same);

    let moved = f.temp.path().join("work-away");
    fs::rename(&f.root, &moved).unwrap();
    std::os::unix::fs::symlink(&other.root, &f.root).unwrap();
    assert_eq!(
        repo.verify_at_path().unwrap(),
        FoundAtPath::Other(other.repo().path().to_owned())
    );
    refused(&repo);
    fs::remove_file(&f.root).unwrap();

    // Cloned again at the same path.
    git(
        f.temp.path(),
        &[
            "clone",
            "--quiet",
            moved.to_str().unwrap(),
            f.root.to_str().unwrap(),
        ],
    );
    git(&f.root, &["config", "user.name", "Fixture Author"]);
    git(
        &f.root,
        &["config", "user.email", "fixture@example.invalid"],
    );
    assert_eq!(repo.verify_at_path().unwrap(), FoundAtPath::Same);
    f.write("file.txt", "after the clone\n");
    repo.execute(&WriteCommand::StageAll).unwrap();
    assert_eq!(f.git(&["diff", "--cached", "--name-only"]), "file.txt");
}

/// `core.worktree` re-pointed after open moves the work tree without
/// touching the `.git` entry: Stage all once staged the other folder's
/// files. A changed configuration file makes the write ask Git first, and
/// it refuses before Git starts; an edit that leaves the work tree where it
/// was (a name) still writes.
#[test]
fn a_work_tree_moved_by_configuration_after_open_is_refused() {
    let f = Fixture::new();
    f.write("file.txt", "base\n");
    f.commit("base");
    let repo = f.repo();
    let elsewhere = f.temp.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    fs::write(elsewhere.join("foreign.txt"), "foreign\n").unwrap();
    f.git(&["config", "core.worktree", elsewhere.to_str().unwrap()]);
    let error = format!("{:#}", repo.execute(&WriteCommand::StageAll).unwrap_err());
    assert!(
        error.contains("The repository changed") && error.contains("Git was not started"),
        "{error}"
    );
    assert!(f.git(&["diff", "--cached", "--name-only"]).is_empty());

    f.git(&["config", "--unset", "core.worktree"]);
    f.git(&["config", "user.name", "Renamed Author"]);
    f.write("file.txt", "changed\n");
    repo.execute(&WriteCommand::StageAll).unwrap();
    assert_eq!(f.git(&["diff", "--cached", "--name-only"]), "file.txt");
}

/// A gitfile whose path differs from the captured Git directory only by a
/// trailing space names another folder for Git, which strips only line
/// ends: the look-alike is another repository and the write refuses,
/// leaving its index alone.
#[test]
fn a_gitfile_naming_a_look_alike_git_directory_is_another_repository() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("work");
    let store = temp.path().join("store.git");
    git(
        temp.path(),
        &[
            "init",
            "--quiet",
            "--separate-git-dir",
            store.to_str().unwrap(),
            root.to_str().unwrap(),
        ],
    );
    let repo = GitRepository::open(&root).unwrap();
    assert_eq!(repo.verify_at_path().unwrap(), FoundAtPath::Same);
    let look_alike = PathBuf::from(format!("{} ", repo.git_dir().display()));
    let other = Fixture::new();
    fs::rename(other.root.join(".git"), &look_alike).unwrap();
    fs::write(
        root.join(".git"),
        format!("gitdir: {}\n", look_alike.display()),
    )
    .unwrap();
    fs::write(root.join("file.txt"), "into the look-alike\n").unwrap();
    let error = format!("{:#}", repo.execute(&WriteCommand::StageAll).unwrap_err());
    assert!(
        error.contains("The repository changed") && error.contains("Git was not started"),
        "{error}"
    );
    let staged = Command::new("git")
        .arg("--git-dir")
        .arg(&look_alike)
        .args(["ls-files", "--cached"])
        .output()
        .unwrap();
    assert!(
        staged.stdout.is_empty(),
        "the look-alike's index is untouched"
    );
}

/// A linked worktree's `commondir` re-pointed to another repository uses
/// that repository's refs and objects with no `.git` entry changed: the
/// changed file makes the write ask Git, and it refuses before Git starts.
#[test]
fn a_linked_worktree_whose_commondir_is_re_pointed_is_refused() {
    let f = Fixture::new();
    f.write("file.txt", "base\n");
    f.commit("base");
    let linked = f.temp.path().join("linked");
    f.git(&[
        "worktree",
        "add",
        "--quiet",
        "-b",
        "linked",
        linked.to_str().unwrap(),
    ]);
    let repo = GitRepository::open(&linked).unwrap();
    let other = Fixture::new();
    other.write("other.txt", "other\n");
    other.commit("other");
    let other_git = other.repo().git_dir().to_owned();
    fs::write(
        repo.git_dir().join("commondir"),
        format!("{}\n", other_git.display()),
    )
    .unwrap();
    fs::write(linked.join("file.txt"), "changed\n").unwrap();
    let error = format!("{:#}", repo.execute(&WriteCommand::StageAll).unwrap_err());
    assert!(error.contains("Git was not started"), "{error}");
    assert_eq!(other.git(&["diff", "--cached", "--name-only"]), "");
}

/// Points the existing link `link` at `target`.
#[cfg(unix)]
fn re_point(link: &Path, target: &Path) {
    fs::remove_file(link).unwrap();
    std::os::unix::fs::symlink(target, link).unwrap();
}

/// A plain repository whose `commondir` reaches its own Git directory
/// through a link has no second directory, so nothing but the file says it
/// leads anywhere: re-pointing the link gives it another repository's refs
/// and objects, so the check refuses before Git starts. Kept out of the
/// guard because the two directories are one, a real commit once moved the
/// other repository's HEAD. Its link back as it was, the write runs.
#[cfg(unix)]
#[test]
fn a_commondir_through_a_link_to_its_own_git_directory_re_pointed_is_refused() {
    let f = Fixture::new();
    f.write("file.txt", "base\n");
    f.commit("base");
    let own = f.root.join(".git").canonicalize().unwrap();
    let link = f.temp.path().join("own-link");
    std::os::unix::fs::symlink(&own, &link).unwrap();
    fs::write(own.join("commondir"), format!("{}\n", link.display())).unwrap();
    let repo = f.repo();
    assert_eq!(repo.common_dir(), own);
    assert_eq!(repo.verify_at_path().unwrap(), FoundAtPath::Same);

    let other = Fixture::new();
    other.write("other.txt", "other\n");
    other.commit("other");
    let other_git = other.repo().git_dir().to_owned();
    let other_head = other.git(&["rev-parse", "HEAD"]);
    re_point(&link, &other_git);
    f.write("file.txt", "changed\n");
    for command in [
        WriteCommand::StageAll,
        WriteCommand::Commit {
            message: "lands where".into(),
        },
    ] {
        let error = format!("{:#}", repo.execute(&command).unwrap_err());
        assert!(error.contains("Git was not started"), "{error}");
    }
    assert_eq!(other.git(&["rev-parse", "HEAD"]), other_head);
    assert_eq!(other.git(&["diff", "--cached", "--name-only"]), "");

    re_point(&link, &own);
    assert_eq!(repo.verify_at_path().unwrap(), FoundAtPath::Same);
    repo.execute(&WriteCommand::StageAll).unwrap();
    assert_eq!(f.git(&["diff", "--cached", "--name-only"]), "file.txt");
}

/// A linked worktree whose `commondir` reaches the main Git directory
/// through a link: re-pointing the link gives it another repository's refs
/// and objects with none of the files the check stamps changed, so the
/// check resolves the stored path again and refuses before Git starts. Its
/// link back as it was, the check holds again and the write runs.
#[cfg(unix)]
#[test]
fn a_commondir_through_a_link_re_pointed_after_open_is_refused() {
    let f = Fixture::new();
    f.write("file.txt", "base\n");
    f.commit("base");
    let linked = f.temp.path().join("linked");
    f.git(&[
        "worktree",
        "add",
        "--quiet",
        "-b",
        "linked",
        linked.to_str().unwrap(),
    ]);
    let private = GitRepository::open(&linked).unwrap().git_dir().to_owned();
    let main_git = f.root.join(".git").canonicalize().unwrap();
    let link = f.temp.path().join("common-link");
    std::os::unix::fs::symlink(&main_git, &link).unwrap();
    fs::write(private.join("commondir"), format!("{}\n", link.display())).unwrap();
    let repo = GitRepository::open(&linked).unwrap();
    assert_eq!(repo.common_dir(), main_git);
    assert_eq!(repo.verify_at_path().unwrap(), FoundAtPath::Same);

    let other = Fixture::new();
    other.write("other.txt", "other\n");
    other.commit("other");
    let other_git = other.repo().git_dir().to_owned();
    re_point(&link, &other_git);
    fs::write(linked.join("file.txt"), "changed\n").unwrap();
    let error = format!("{:#}", repo.execute(&WriteCommand::StageAll).unwrap_err());
    assert!(error.contains("Git was not started"), "{error}");
    assert_eq!(other.git(&["diff", "--cached", "--name-only"]), "");
    assert!(
        matches!(
            repo.verify_at_path().unwrap(),
            FoundAtPath::Other(found) if found.canonicalize().unwrap() == other_git
        ),
        "the other repository's common directory is named"
    );

    re_point(&link, &main_git);
    assert_eq!(repo.verify_at_path().unwrap(), FoundAtPath::Same);
    repo.execute(&WriteCommand::StageAll).unwrap();
    assert_eq!(
        git(&linked, &["diff", "--cached", "--name-only"]),
        "file.txt"
    );
}

/// `extensions.worktreeConfig` spelled so that only Git can read it: `0b0` is
/// zero to a C23 `strtoimax`, so Git goes by the shared `core.worktree`,
/// through a link, and never reads `config.worktree`. Read here as true, the
/// guard lay on `config.worktree`, which still led to the root, so a re-point
/// of the link Git really reads passed with no process started and Stage all
/// staged the other tree's files. Refused before Git starts now. Where the C
/// library takes no binary, Git refuses the value and this layout cannot be
/// made.
#[cfg(unix)]
#[test]
fn a_flag_only_git_can_read_guards_the_work_tree_git_reads() {
    let f = Fixture::new();
    f.write("seed.txt", "base\n");
    f.commit("base");
    let spelling = f.temp.path().join("spelling");
    fs::write(&spelling, "[x]\n\tk = 0b0\n").unwrap();
    let read = Command::new("git")
        .args(["config", "--file"])
        .arg(&spelling)
        .args(["--type=bool", "--get", "x.k"])
        .output()
        .unwrap();
    if !read.status.success() {
        return;
    }
    assert_eq!(String::from_utf8_lossy(&read.stdout).trim(), "false");

    let root = f.root.canonicalize().unwrap();
    f.write("sub1/file.txt", "tree\n");
    f.write("sub3/foreign.txt", "foreign\n");
    let tree = root.join("sub1");
    let link = f.temp.path().join("tree-link");
    std::os::unix::fs::symlink(&tree, &link).unwrap();
    f.git(&["config", "core.repositoryformatversion", "1"]);
    f.git(&["config", "extensions.worktreeConfig", "0b0"]);
    f.git(&["config", "core.worktree", link.to_str().unwrap()]);
    fs::write(
        root.join(".git/config.worktree"),
        format!("[core]\n\tworktree = {}\n", tree.display()),
    )
    .unwrap();
    let repo = GitRepository::open(&tree).unwrap();
    assert_eq!(repo.path(), tree);
    assert_eq!(repo.verify_at_path().unwrap(), FoundAtPath::Same);

    let index = fs::read(root.join(".git/index")).unwrap();
    re_point(&link, &root.join("sub3"));
    let error = format!("{:#}", repo.execute(&WriteCommand::StageAll).unwrap_err());
    assert!(
        error.contains("The repository changed") && error.contains("Git was not started"),
        "{error}"
    );
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);

    re_point(&link, &tree);
    assert_eq!(repo.verify_at_path().unwrap(), FoundAtPath::Same);
    repo.execute(&WriteCommand::StageAll).unwrap();
}

/// `core.worktree` reaching the work tree through a link: re-pointing the
/// link moves the work tree with no configuration file touched and no
/// `.git` entry changed, so the check resolves the stored path again and
/// refuses before Git starts, leaving the index alone. Its link back as it
/// was, the check holds again and the write runs.
#[cfg(unix)]
#[test]
fn a_core_worktree_through_a_link_re_pointed_after_open_is_refused() {
    let f = Fixture::new();
    f.write("file.txt", "base\n");
    f.commit("base");
    let root = f.root.canonicalize().unwrap();
    let link = f.temp.path().join("tree-link");
    std::os::unix::fs::symlink(&root, &link).unwrap();
    f.git(&["config", "core.worktree", link.to_str().unwrap()]);
    let repo = f.repo();
    assert_eq!(repo.path(), root);
    assert_eq!(repo.verify_at_path().unwrap(), FoundAtPath::Same);

    let elsewhere = f.temp.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    fs::write(elsewhere.join("foreign.txt"), "foreign\n").unwrap();
    f.write("file.txt", "changed\n");
    re_point(&link, &elsewhere);
    let error = format!("{:#}", repo.execute(&WriteCommand::StageAll).unwrap_err());
    assert!(
        error.contains("The repository changed") && error.contains("Git was not started"),
        "{error}"
    );
    assert!(f.git(&["diff", "--cached", "--name-only"]).is_empty());

    re_point(&link, &root);
    assert_eq!(repo.verify_at_path().unwrap(), FoundAtPath::Same);
    repo.execute(&WriteCommand::StageAll).unwrap();
    assert_eq!(f.git(&["diff", "--cached", "--name-only"]), "file.txt");
}
