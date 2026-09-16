use gitturtle_core::{ChangeStatus, DiscardPlan, GitRepository, StatusEntry, WriteCommand};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};
use tempfile::TempDir;

struct Fixture {
    _temp: TempDir,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("work");
        GitRepository::init(&root, "main").unwrap();
        let root = GitRepository::open(&root).unwrap().path().to_owned();
        let f = Self { _temp: temp, root };
        f.git(&["config", "user.name", "Discard Fixture"]);
        f.git(&["config", "user.email", "fixture@example.invalid"]);
        f.git(&["config", "commit.gpgSign", "false"]);
        f.write("tracked", "base\n");
        f.write("other", "other\n");
        f.git(&["add", "."]);
        f.git(&["commit", "-m", "Initial"]);
        f
    }
    fn unborn() -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("unborn");
        GitRepository::init(&root, "main").unwrap();
        let root = GitRepository::open(&root).unwrap().path().to_owned();
        Self { _temp: temp, root }
    }
    fn repo(&self) -> GitRepository {
        GitRepository::open(&self.root).unwrap()
    }
    fn git(&self, args: &[&str]) -> String {
        git_at(&self.root, args)
    }
    fn write(&self, path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    fn read(&self, path: impl AsRef<Path>) -> Vec<u8> {
        fs::read(self.root.join(path)).unwrap()
    }
    fn entry(&self, path: &str) -> StatusEntry {
        self.repo()
            .status()
            .unwrap()
            .entries
            .into_iter()
            .find(|entry| entry.path == Path::new(path))
            .unwrap_or_else(|| panic!("no status entry for {path}"))
    }
    fn row(&self, path: &str, untracked: bool) -> StatusEntry {
        self.repo()
            .status()
            .unwrap()
            .entries
            .into_iter()
            .find(|entry| entry.path == Path::new(path) && entry.untracked == untracked)
            .unwrap_or_else(|| panic!("no status row for {path} (untracked: {untracked})"))
    }
    fn plan(&self, path: &str) -> DiscardPlan {
        self.repo().discard_plan(&self.entry(path)).unwrap()
    }
}

fn discard(plan: DiscardPlan) -> WriteCommand {
    WriteCommand::Discard(Arc::new(plan))
}

fn git_at(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
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
fn discard_restores_tracked_file_from_head_and_preserves_other_work() {
    let f = Fixture::new();
    let repo = f.repo();
    f.write("tracked", "staged edit\n");
    f.git(&["add", "tracked"]);
    f.write("tracked", "unstaged edit\n");
    f.write("other", "other staged\n");
    f.git(&["add", "other"]);
    f.write("note", "untracked\n");
    let entry = f.entry("tracked");
    assert_eq!(entry.staged, Some(ChangeStatus::Modified));
    assert_eq!(entry.unstaged, Some(ChangeStatus::Modified));
    let plan = f.plan("tracked");
    assert_eq!(
        plan.head.as_deref(),
        Some(f.git(&["rev-parse", "HEAD"]).as_str())
    );
    let refs = f.git(&["show-ref"]);
    let outcome = repo.execute(&discard(plan)).unwrap();
    assert!(outcome.message.contains("Discarded changes to tracked"));
    assert_eq!(f.read("tracked"), b"base\n");
    assert_eq!(f.git(&["show", ":tracked"]), "base");
    let status = repo.status().unwrap();
    assert!(
        status
            .entries
            .iter()
            .all(|entry| entry.path != Path::new("tracked"))
    );
    let other = status
        .entries
        .iter()
        .find(|entry| entry.path == Path::new("other"))
        .unwrap();
    assert_eq!(other.staged, Some(ChangeStatus::Modified));
    assert_eq!(f.git(&["show", ":other"]), "other staged");
    assert_eq!(f.read("note"), b"untracked\n");
    assert_eq!(f.git(&["show-ref"]), refs);
}

#[test]
fn discard_handles_rename_added_and_deleted_entries() {
    let f = Fixture::new();
    let repo = f.repo();
    f.git(&["mv", "tracked", "renamed"]);
    let entry = f.entry("renamed");
    assert_eq!(entry.original_path.as_deref(), Some(Path::new("tracked")));
    let outcome = repo.execute(&discard(f.plan("renamed"))).unwrap();
    assert!(
        outcome
            .message
            .contains("Discarded the rename tracked → renamed")
    );
    assert_eq!(f.read("tracked"), b"base\n");
    assert!(!f.root.join("renamed").exists());
    assert!(repo.status().unwrap().entries.is_empty());

    f.write("new", "new\n");
    f.git(&["add", "new"]);
    assert_eq!(f.entry("new").staged, Some(ChangeStatus::Added));
    repo.execute(&discard(f.plan("new"))).unwrap();
    assert!(!f.root.join("new").exists());
    assert!(repo.status().unwrap().entries.is_empty());

    // An intent-to-add row is tracked with an empty index entry; restore
    // removes it from the index and deletes the file.
    f.write("intent", "intent\n");
    f.git(&["add", "-N", "intent"]);
    let intent = f.entry("intent");
    assert!(!intent.untracked);
    assert_eq!(intent.unstaged, Some(ChangeStatus::Added));
    repo.execute(&discard(f.plan("intent"))).unwrap();
    assert!(!f.root.join("intent").exists());
    assert!(repo.status().unwrap().entries.is_empty());

    fs::remove_file(f.root.join("tracked")).unwrap();
    assert_eq!(f.entry("tracked").unstaged, Some(ChangeStatus::Deleted));
    repo.execute(&discard(f.plan("tracked"))).unwrap();
    assert_eq!(f.read("tracked"), b"base\n");

    f.git(&["rm", "-q", "tracked"]);
    assert_eq!(f.entry("tracked").staged, Some(ChangeStatus::Deleted));
    repo.execute(&discard(f.plan("tracked"))).unwrap();
    assert_eq!(f.read("tracked"), b"base\n");
    assert_eq!(f.git(&["show", ":tracked"]), "base");
    assert!(repo.status().unwrap().entries.is_empty());
}

#[test]
fn discard_deletes_untracked_file_only_after_fresh_review_and_refuses_directories() {
    let f = Fixture::new();
    let repo = f.repo();
    f.write("scratch", "a\n");
    f.write("keep", "k\n");
    let plan = f.plan("scratch");
    assert!(plan.entry.untracked);
    f.write("scratch", "changed content\n");
    let error = repo.execute(&discard(plan)).unwrap_err();
    assert!(
        error.to_string().contains("changed after review"),
        "{error:#}"
    );
    assert_eq!(f.read("scratch"), b"changed content\n");
    let outcome = repo.execute(&discard(f.plan("scratch"))).unwrap();
    assert!(outcome.message.contains("Deleted untracked file scratch"));
    assert!(!f.root.join("scratch").exists());
    assert_eq!(f.read("keep"), b"k\n");

    let nested = f.root.join("nested");
    fs::create_dir(&nested).unwrap();
    git_at(&nested, &["init", "-q"]);
    fs::write(nested.join("inner"), "inner\n").unwrap();
    let entry = repo
        .status()
        .unwrap()
        .entries
        .into_iter()
        .find(|entry| entry.untracked && entry.path.starts_with("nested"))
        .unwrap();
    let error = repo.discard_plan(&entry).unwrap_err();
    assert!(
        error.to_string().contains("Untracked directories"),
        "{error:#}"
    );
    assert_eq!(fs::read(nested.join("inner")).unwrap(), b"inner\n");
}

#[test]
fn discard_refuses_changed_bytes_even_when_size_and_modified_time_are_restored() {
    for path in ["tracked", "untracked"] {
        let f = Fixture::new();
        let repo = f.repo();
        f.write(path, "reviewed bytes\n");
        let plan = f.plan(path);
        let absolute = f.root.join(path);
        let original = fs::metadata(&absolute).unwrap();
        f.write(path, "replaced bytes\n");
        fs::File::options()
            .write(true)
            .open(&absolute)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(original.modified().unwrap()))
            .unwrap();
        let replacement = fs::metadata(&absolute).unwrap();
        assert_eq!(original.len(), replacement.len());
        assert_eq!(
            original.modified().unwrap(),
            replacement.modified().unwrap()
        );
        assert_eq!(f.entry(path), plan.entry);
        let error = repo.execute(&discard(plan)).unwrap_err();
        assert!(
            error.to_string().contains("changed after review"),
            "{error:#}"
        );
        assert_eq!(f.read(path), b"replaced bytes\n");
        assert_eq!(f.git(&["show", ":tracked"]), "base");
    }
}

#[test]
fn discard_refuses_worktree_redirection_after_review() {
    let f = Fixture::new();
    let repo = f.repo();
    f.write("tracked", "reviewed bytes\n");
    let plan = f.plan("tracked");
    let outside = f.root.parent().unwrap().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("tracked"), b"unreviewed work\n").unwrap();
    fs::write(outside.join("other"), b"other\n").unwrap();
    f.git(&["config", "core.worktree", outside.to_str().unwrap()]);
    assert_eq!(repo.status().unwrap().entries[0], plan.entry);
    let error = repo.execute(&discard(plan)).unwrap_err();
    assert!(
        error.to_string().contains("repository changed"),
        "{error:#}"
    );
    assert_eq!(
        fs::read(outside.join("tracked")).unwrap(),
        b"unreviewed work\n"
    );
    assert_eq!(f.read("tracked"), b"reviewed bytes\n");
    assert_eq!(f.git(&["show", ":tracked"]), "base");
}

#[test]
fn discard_pins_write_target_despite_global_worktree_redirection() {
    const FIXTURE_ROOT: &str = "GITTURTLE_DISCARD_TARGET_TEST_ROOT";
    if let Some(root) = std::env::var_os(FIXTURE_ROOT) {
        let root = PathBuf::from(root);
        let outside = root.parent().unwrap().join("outside");
        assert_eq!(
            git_at(&root, &["config", "--get", "core.worktree"]),
            outside.to_str().unwrap()
        );
        let repo = GitRepository::open(&root).unwrap();
        let row = repo
            .status()
            .unwrap()
            .entries
            .into_iter()
            .find(|row| row.path == Path::new("tracked"))
            .unwrap();
        let plan = repo.discard_plan(&row).unwrap();
        let result = repo.execute(&discard(plan));
        assert_eq!(
            fs::read(outside.join("tracked")).unwrap(),
            b"unreviewed external work\n"
        );
        result.unwrap();
        assert_eq!(fs::read(root.join("tracked")).unwrap(), b"base\n");
        return;
    }

    let f = Fixture::new();
    f.write("tracked", "reviewed bytes\n");
    let outside = f.root.parent().unwrap().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("tracked"), b"unreviewed external work\n").unwrap();
    fs::write(outside.join("other"), b"other\n").unwrap();
    let global = f.root.parent().unwrap().join("global.gitconfig");
    let included = f.root.parent().unwrap().join("included.gitconfig");
    f.git(&[
        "config",
        "--file",
        included.to_str().unwrap(),
        "core.worktree",
        outside.to_str().unwrap(),
    ]);
    f.git(&[
        "config",
        "--file",
        global.to_str().unwrap(),
        &format!("includeIf.gitdir:{}.path", f.root.join(".git").display()),
        included.to_str().unwrap(),
    ]);

    // Isolate configuration in a subprocess; never mutate this test runner's
    // environment while other disposable fixtures are running in parallel.
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "discard_pins_write_target_despite_global_worktree_redirection",
            "--nocapture",
        ])
        .env(FIXTURE_ROOT, &f.root)
        .env("GIT_CONFIG_GLOBAL", &global)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn discard_restores_the_raw_reviewed_commit_despite_replace_refs() {
    let f = Fixture::new();
    let original = f.git(&["rev-parse", "HEAD"]);
    f.git(&["checkout", "-q", "-b", "replacement"]);
    fs::remove_file(f.root.join("tracked")).unwrap();
    f.write("tracked/child", "replacement tree child\n");
    f.git(&["add", "-A"]);
    let tree = f.git(&["write-tree"]);
    let replacement = f.git(&["commit-tree", &tree, "-m", "Replacement object"]);
    f.git(&["reset", "--hard"]);
    f.git(&["checkout", "-q", "main"]);
    f.write("tracked", "reviewed bytes\n");
    f.git(&["add", "tracked"]);
    f.git(&["replace", &original, &replacement]);
    let repo = f.repo();
    let plan = f.plan("tracked");
    let result = repo.execute(&discard(plan));
    assert!(
        f.root.join("tracked").is_file(),
        "Discard restored an unreviewed replacement tree: {result:?}"
    );
    result.unwrap();
    assert_eq!(f.read("tracked"), b"base\n");
    assert!(repo.status().unwrap().entries.is_empty());
}

#[test]
fn discard_refuses_git_directory_redirection_after_review() {
    let f = Fixture::new();
    let repo = f.repo();
    f.write("tracked", "reviewed bytes\n");
    let plan = f.plan("tracked");
    let old_git = f.root.parent().unwrap().join("original.git");
    fs::rename(f.root.join(".git"), &old_git).unwrap();
    fs::write(
        f.root.join(".git"),
        format!("gitdir: {}\n", old_git.display()),
    )
    .unwrap();
    assert_eq!(f.entry("tracked"), plan.entry);
    let error = repo.execute(&discard(plan)).unwrap_err();
    assert!(
        error.to_string().contains("changed after review"),
        "{error:#}"
    );
    assert_eq!(f.read("tracked"), b"reviewed bytes\n");
    assert_eq!(f.git(&["show", ":tracked"]), "base");
}

#[test]
fn discard_refuses_git_directory_replacement_at_the_same_path() {
    let f = Fixture::new();
    let repo = f.repo();
    f.write("tracked", "reviewed bytes\n");
    let plan = f.plan("tracked");
    let parent = f.root.parent().unwrap();
    let replacement = parent.join("replacement");
    git_at(
        parent,
        &[
            "clone",
            "--no-hardlinks",
            f.root.to_str().unwrap(),
            replacement.to_str().unwrap(),
        ],
    );
    fs::rename(f.root.join(".git"), parent.join("original.git")).unwrap();
    fs::rename(replacement.join(".git"), f.root.join(".git")).unwrap();
    assert_eq!(f.entry("tracked"), plan.entry);
    let error = repo.execute(&discard(plan)).unwrap_err();
    assert!(
        error.to_string().contains("changed after review"),
        "{error:#}"
    );
    assert_eq!(f.read("tracked"), b"reviewed bytes\n");
    assert_eq!(f.git(&["show", ":tracked"]), "base");
}

#[cfg(unix)]
#[test]
fn discard_refuses_hidden_permission_changes_after_review() {
    use std::os::unix::fs::PermissionsExt;

    for path in ["tracked", "untracked"] {
        let f = Fixture::new();
        let repo = f.repo();
        f.git(&["config", "core.filemode", "false"]);
        f.write(path, "reviewed bytes\n");
        fs::set_permissions(f.root.join(path), fs::Permissions::from_mode(0o644)).unwrap();
        let plan = f.plan(path);
        fs::set_permissions(f.root.join(path), fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(f.entry(path), plan.entry);
        let error = repo.execute(&discard(plan)).unwrap_err();
        assert!(
            error.to_string().contains("changed after review"),
            "{error:#}"
        );
        assert_eq!(f.read(path), b"reviewed bytes\n");
        assert_eq!(
            fs::metadata(f.root.join(path))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
    }
}

#[test]
fn discard_refuses_files_exceeding_the_review_byte_limit() {
    let f = Fixture::new();
    for path in ["tracked", "untracked"] {
        fs::File::create(f.root.join(path))
            .unwrap()
            .set_len(gitturtle_core::MAX_BLOB_BYTES as u64 + 1)
            .unwrap();
        let error = f.repo().discard_plan(&f.entry(path)).unwrap_err();
        assert!(
            error.to_string().contains("64 MiB discard review limit"),
            "{error:#}"
        );
        assert_eq!(
            fs::metadata(f.root.join(path)).unwrap().len(),
            gitturtle_core::MAX_BLOB_BYTES as u64 + 1
        );
    }
    assert_eq!(f.git(&["show", ":tracked"]), "base");
}

#[cfg(unix)]
#[test]
fn discard_refuses_unreadable_content_and_preserves_it() {
    use std::os::unix::fs::PermissionsExt;

    // Root can still read mode-000 files, so this refusal fixture requires a
    // normal user whose filesystem permissions can actually deny the read.
    if rustix::process::geteuid().is_root() {
        return;
    }
    for path in ["tracked", "untracked"] {
        let f = Fixture::new();
        let repo = f.repo();
        f.write(path, "reviewed bytes\n");
        let plan = f.plan(path);
        let absolute = f.root.join(path);
        fs::set_permissions(&absolute, fs::Permissions::from_mode(0o000)).unwrap();
        let error = repo.execute(&discard(plan)).unwrap_err();
        assert!(
            format!("{error:#}").contains("Unable to safely read"),
            "{error:#}"
        );
        fs::set_permissions(&absolute, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(f.read(path), b"reviewed bytes\n");
        assert_eq!(f.git(&["show", ":tracked"]), "base");
    }
}

#[cfg(unix)]
#[test]
fn discard_reviews_stored_symlink_targets_and_preserves_their_destinations() {
    use std::os::unix::fs::symlink;

    let f = Fixture::new();
    let repo = f.repo();
    let outside = f.root.parent().unwrap().join("outside");
    fs::write(&outside, b"precious external work\n").unwrap();
    fs::remove_file(f.root.join("tracked")).unwrap();
    symlink(&outside, f.root.join("tracked")).unwrap();
    let plan = f.plan("tracked");
    repo.execute(&discard(plan)).unwrap();
    assert_eq!(f.read("tracked"), b"base\n");
    assert_eq!(fs::read(&outside).unwrap(), b"precious external work\n");

    // Broken targets remain reviewable because only stored link text is read.
    symlink("missing-a", f.root.join("untracked")).unwrap();
    let plan = f.plan("untracked");
    fs::remove_file(f.root.join("untracked")).unwrap();
    symlink("missing-b", f.root.join("untracked")).unwrap();
    assert_eq!(f.entry("untracked"), plan.entry);
    let error = repo.execute(&discard(plan)).unwrap_err();
    assert!(
        error.to_string().contains("changed after review"),
        "{error:#}"
    );
    assert_eq!(
        fs::read_link(f.root.join("untracked")).unwrap(),
        Path::new("missing-b")
    );
    repo.execute(&discard(f.plan("untracked"))).unwrap();
    assert!(fs::symlink_metadata(f.root.join("untracked")).is_err());
}

#[cfg(unix)]
#[test]
fn discard_reviews_raw_bytes_without_filters_and_preserves_restore_filters() {
    let f = Fixture::new();
    f.write(".gitattributes", "tracked filter=reviewed\n");
    f.git(&["add", ".gitattributes"]);
    f.git(&["commit", "-q", "-m", "Attributes"]);
    f.git(&[
        "config",
        "filter.reviewed.clean",
        "touch .git/clean-ran; cat",
    ]);
    f.git(&[
        "config",
        "filter.reviewed.smudge",
        "touch .git/smudge-ran; cat",
    ]);
    f.git(&["config", "filter.reviewed.required", "true"]);
    f.write("tracked", b"\xff\x00raw work\n");
    let index = fs::read(f.root.join(".git/index")).unwrap();
    let repo = f.repo();
    let plan = f.plan("tracked");
    assert!(!f.root.join(".git/clean-ran").exists());
    assert!(!f.root.join(".git/smudge-ran").exists());
    assert_eq!(fs::read(f.root.join(".git/index")).unwrap(), index);
    assert_eq!(f.read("tracked"), b"\xff\x00raw work\n");
    repo.execute(&discard(plan)).unwrap();
    assert!(f.root.join(".git/smudge-ran").exists());
    assert_eq!(f.read("tracked"), b"base\n");
}

#[cfg(unix)]
#[test]
fn discard_preserves_non_utf8_filename_bytes() {
    use std::os::unix::ffi::OsStringExt;

    let f = Fixture::new();
    let repo = f.repo();
    let path = PathBuf::from(std::ffi::OsString::from_vec(b"odd\xff\n[ab]*".to_vec()));
    f.write(&path, "reviewed bytes\n");
    f.write("keep", "keep\n");
    let entry = repo
        .status()
        .unwrap()
        .entries
        .into_iter()
        .find(|row| row.path == path)
        .unwrap();
    let plan = repo.discard_plan(&entry).unwrap();
    repo.execute(&discard(plan)).unwrap();
    assert!(!f.root.join(path).exists());
    assert_eq!(f.read("keep"), b"keep\n");
}

#[test]
fn discard_refuses_stale_index_stale_head_conflicted_submodule_and_unborn_targets() {
    let f = Fixture::new();
    let repo = f.repo();
    f.write("tracked", "edit\n");
    let reviewed = f.plan("tracked");
    f.git(&["add", "tracked"]);
    // Staging changes the row itself, so the fresh review refuses before the
    // plan comparison runs.
    let error = repo.execute(&discard(reviewed)).unwrap_err();
    assert!(error.to_string().contains("status changed"), "{error:#}");
    assert_eq!(f.git(&["show", ":tracked"]), "edit");
    assert_eq!(f.read("tracked"), b"edit\n");

    // Move HEAD through another path; the reviewed row itself is unchanged.
    let reviewed = f.plan("tracked");
    f.write("other", "moved\n");
    f.git(&["commit", "-q", "--only", "-m", "Moved HEAD", "--", "other"]);
    assert_eq!(f.entry("tracked"), reviewed.entry);
    let error = repo.execute(&discard(reviewed)).unwrap_err();
    assert!(
        error.to_string().contains("changed after review"),
        "{error:#}"
    );
    assert_eq!(f.read("tracked"), b"edit\n");
    f.git(&["restore", "--staged", "--worktree", "--", "tracked"]);

    f.git(&["checkout", "-q", "-b", "side"]);
    f.write("tracked", "side\n");
    f.git(&["commit", "-q", "-am", "Side"]);
    f.git(&["checkout", "-q", "main"]);
    f.write("tracked", "main\n");
    f.git(&["commit", "-q", "-am", "Main"]);
    let merge = Command::new("git")
        .arg("-C")
        .arg(&f.root)
        .args(["merge", "side"])
        .output()
        .unwrap();
    assert!(!merge.status.success());
    let entry = f.entry("tracked");
    assert!(entry.conflicted);
    let error = repo.discard_plan(&entry).unwrap_err();
    assert!(error.to_string().contains("conflict"), "{error:#}");
    f.git(&["merge", "--abort"]);

    let module = Fixture::new();
    f.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        module.root.to_str().unwrap(),
        "module",
    ]);
    f.git(&["commit", "-q", "-m", "Add submodule"]);
    let checkout = f.root.join("module");
    fs::write(checkout.join("tracked"), "inside\n").unwrap();
    git_at(&checkout, &["commit", "-q", "-am", "Inside"]);
    let entry = f.entry("module");
    let error = repo.discard_plan(&entry).unwrap_err();
    assert!(error.to_string().contains("Submodule"), "{error:#}");
    assert_eq!(fs::read(checkout.join("tracked")).unwrap(), b"inside\n");

    let u = Fixture::unborn();
    u.write("first", "first\n");
    u.git(&["add", "first"]);
    u.write("loose", "loose\n");
    let error = u.repo().discard_plan(&u.entry("first")).unwrap_err();
    assert!(error.to_string().contains("no commit"), "{error:#}");
    let plan = u.plan("loose");
    assert!(plan.head.is_none());
    u.repo().execute(&discard(plan)).unwrap();
    assert!(!u.root.join("loose").exists());
    assert_eq!(u.read("first"), b"first\n");
}

#[test]
fn discard_refuses_directory_replacements_and_replaced_parent_paths() {
    let f = Fixture::new();
    let repo = f.repo();
    // A tracked file that became a directory of untracked work with a nested
    // repository deeper inside. Restore would delete the whole subtree.
    fs::remove_file(f.root.join("tracked")).unwrap();
    fs::create_dir(f.root.join("tracked")).unwrap();
    f.write("tracked/new.txt", "new work\n");
    let nested = f.root.join("tracked/sub");
    fs::create_dir(&nested).unwrap();
    git_at(&nested, &["init", "-q"]);
    let row = f.row("tracked", false);
    assert_eq!(row.unstaged, Some(ChangeStatus::Deleted));
    let error = repo.discard_plan(&row).unwrap_err();
    assert!(error.to_string().contains("now a directory"), "{error:#}");
    assert_eq!(f.read("tracked/new.txt"), b"new work\n");
    assert!(nested.join(".git").exists());
    fs::remove_dir_all(f.root.join("tracked")).unwrap();
    f.git(&["restore", "--", "tracked"]);

    // A rename whose source path was recreated as a directory.
    f.git(&["mv", "other", "renamed"]);
    fs::create_dir(f.root.join("other")).unwrap();
    f.write("other/keep.txt", "keep\n");
    let row = f.row("renamed", false);
    assert_eq!(row.original_path.as_deref(), Some(Path::new("other")));
    let error = repo.discard_plan(&row).unwrap_err();
    assert!(error.to_string().contains("now a directory"), "{error:#}");
    assert_eq!(f.read("other/keep.txt"), b"keep\n");
    fs::remove_dir_all(f.root.join("other")).unwrap();
    f.git(&["mv", "renamed", "other"]);

    // A parent path that became an untracked file or a symbolic link.
    f.write("dir/inner", "inner\n");
    f.git(&["add", "dir"]);
    f.git(&["commit", "-q", "-m", "Nested file"]);
    fs::remove_dir_all(f.root.join("dir")).unwrap();
    f.write("dir", "a file where the folder was\n");
    let row = f.row("dir/inner", false);
    assert_eq!(row.unstaged, Some(ChangeStatus::Deleted));
    let error = repo.discard_plan(&row).unwrap_err();
    assert!(error.to_string().contains("parent path"), "{error:#}");
    assert_eq!(f.read("dir"), b"a file where the folder was\n");
    fs::remove_file(f.root.join("dir")).unwrap();
    #[cfg(unix)]
    {
        let outside = f.root.parent().unwrap().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("precious"), "precious\n").unwrap();
        std::os::unix::fs::symlink(&outside, f.root.join("dir")).unwrap();
        let row = f.row("dir/inner", false);
        let error = repo.discard_plan(&row).unwrap_err();
        assert!(error.to_string().contains("parent path"), "{error:#}");
        assert!(
            fs::symlink_metadata(f.root.join("dir"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(outside.join("precious")).unwrap(), b"precious\n");
    }
}

#[test]
fn discard_refuses_file_replacing_a_committed_directory() {
    let f = Fixture::new();
    let repo = f.repo();
    f.write("dir/a", "first committed file\n");
    f.write("dir/b", "second committed file\n");
    f.git(&["add", "dir"]);
    f.git(&["commit", "-q", "-m", "Directory"]);
    fs::remove_dir_all(f.root.join("dir")).unwrap();
    f.write("dir", "replacement file\n");
    f.git(&["add", "-A"]);
    f.write("other", "unrelated staged work\n");
    f.git(&["add", "other"]);
    let row = f.entry("dir");
    assert_eq!(row.staged, Some(ChangeStatus::Added));
    let index = fs::read(f.root.join(".git/index")).unwrap();
    let status = repo.status().unwrap();
    let result = repo
        .discard_plan(&row)
        .and_then(|plan| repo.execute(&discard(plan)));
    let error = result.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("directory in the reviewed commit"),
        "{error:#}"
    );
    assert_eq!(f.read("dir"), b"replacement file\n");
    assert_eq!(fs::read(f.root.join(".git/index")).unwrap(), index);
    assert_eq!(repo.status().unwrap(), status);
    assert_eq!(f.git(&["show", ":other"]), "unrelated staged work");
}

#[test]
fn discard_refuses_deleted_file_with_staged_descendants() {
    let f = Fixture::new();
    let repo = f.repo();
    fs::remove_file(f.root.join("tracked")).unwrap();
    f.write("tracked/a", "first staged file\n");
    f.write("tracked/b", "second staged file\n");
    f.git(&["add", "-A"]);
    fs::remove_dir_all(f.root.join("tracked")).unwrap();
    let row = f.row("tracked", false);
    assert_eq!(row.staged, Some(ChangeStatus::Deleted));
    let index = fs::read(f.root.join(".git/index")).unwrap();
    let status = repo.status().unwrap();
    let result = repo
        .discard_plan(&row)
        .and_then(|plan| repo.execute(&discard(plan)));
    let error = result.unwrap_err();
    assert!(
        error.to_string().contains("staged files beneath"),
        "{error:#}"
    );
    assert!(!f.root.join("tracked").exists());
    assert_eq!(fs::read(f.root.join(".git/index")).unwrap(), index);
    assert_eq!(repo.status().unwrap(), status);
    assert_eq!(f.git(&["show", ":tracked/a"]), "first staged file");
    assert_eq!(f.git(&["show", ":tracked/b"]), "second staged file");
}

#[test]
fn discard_refuses_rename_endpoints_with_committed_or_staged_descendants() {
    for source_has_descendants in [false, true] {
        let f = Fixture::new();
        let repo = f.repo();
        if source_has_descendants {
            f.git(&["mv", "tracked", "renamed"]);
            f.write("tracked/a", "new staged source child\n");
            f.git(&["add", "tracked"]);
            fs::remove_dir_all(f.root.join("tracked")).unwrap();
        } else {
            f.write("renamed/a", "committed destination child\n");
            f.git(&["add", "renamed"]);
            f.git(&["commit", "-q", "-m", "Destination directory"]);
            f.git(&["rm", "-r", "renamed"]);
            f.git(&["mv", "tracked", "renamed"]);
        }
        let row = f.entry("renamed");
        assert_eq!(row.original_path.as_deref(), Some(Path::new("tracked")));
        let index = fs::read(f.root.join(".git/index")).unwrap();
        let status = repo.status().unwrap();
        let error = repo.discard_plan(&row).unwrap_err();
        assert!(
            error.to_string().contains(if source_has_descendants {
                "staged files beneath"
            } else {
                "directory in the reviewed commit"
            }),
            "{error:#}"
        );
        assert_eq!(f.read("renamed"), b"base\n");
        assert!(!f.root.join("tracked").exists());
        assert_eq!(fs::read(f.root.join(".git/index")).unwrap(), index);
        assert_eq!(repo.status().unwrap(), status);
    }
}

#[test]
fn discard_nested_file_preserves_staged_siblings() {
    let f = Fixture::new();
    let repo = f.repo();
    f.write("dir/inner/selected", "selected base\n");
    f.write("dir/inner/sibling", "sibling base\n");
    f.git(&["add", "dir"]);
    f.git(&["commit", "-q", "-m", "Nested files"]);
    f.write("dir/inner/selected", "selected staged\n");
    f.write("dir/inner/sibling", "unrelated staged work\n");
    f.git(&["add", "dir"]);
    f.write("dir/inner/selected", "selected unstaged\n");
    let sibling = f.entry("dir/inner/sibling");
    repo.execute(&discard(f.plan("dir/inner/selected")))
        .unwrap();
    assert_eq!(f.read("dir/inner/selected"), b"selected base\n");
    assert_eq!(f.git(&["show", ":dir/inner/selected"]), "selected base");
    assert_eq!(f.read("dir/inner/sibling"), b"unrelated staged work\n");
    assert_eq!(
        f.git(&["show", ":dir/inner/sibling"]),
        "unrelated staged work"
    );
    assert_eq!(repo.status().unwrap().entries, vec![sibling]);
}

#[test]
fn discard_refuses_staged_deletion_under_an_untracked_file_and_deletes_that_row_instead() {
    let f = Fixture::new();
    let repo = f.repo();
    f.git(&["rm", "-q", "--cached", "tracked"]);
    f.write("tracked", "kept locally\n");
    let staged = f.row("tracked", false);
    assert_eq!(staged.staged, Some(ChangeStatus::Deleted));
    let error = repo.discard_plan(&staged).unwrap_err();
    assert!(error.to_string().contains("occupies"), "{error:#}");
    assert_eq!(f.read("tracked"), b"kept locally\n");

    // A rename whose source path an untracked file occupies again.
    f.git(&["restore", "--staged", "--", "tracked"]);
    f.git(&["mv", "other", "renamed"]);
    f.write("other", "recreated\n");
    let row = f.row("renamed", false);
    let error = repo.discard_plan(&row).unwrap_err();
    assert!(error.to_string().contains("occupies"), "{error:#}");
    assert_eq!(f.read("other"), b"recreated\n");
    fs::remove_file(f.root.join("other")).unwrap();
    f.git(&["mv", "renamed", "other"]);

    // The untracked row at the same path is its own reviewed target.
    f.git(&["rm", "-q", "--cached", "tracked"]);
    f.write("tracked", "kept locally\n");
    let untracked = f.row("tracked", true);
    let outcome = repo
        .execute(&discard(repo.discard_plan(&untracked).unwrap()))
        .unwrap();
    assert!(outcome.message.contains("Deleted untracked file tracked"));
    assert!(!f.root.join("tracked").exists());
    let rows = repo.status().unwrap().entries;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].staged, Some(ChangeStatus::Deleted));
    // With the path free again, discarding the staged deletion restores it.
    repo.execute(&discard(f.plan("tracked"))).unwrap();
    assert_eq!(f.read("tracked"), b"base\n");
    assert!(repo.status().unwrap().entries.is_empty());
}

#[test]
fn discard_uses_literal_pathspecs_and_leaves_glob_siblings_changed() {
    let f = Fixture::new();
    let repo = f.repo();
    f.write("[a]*.txt", "glob\n");
    f.write("a.txt", "plain\n");
    f.git(&["add", "."]);
    f.git(&["commit", "-q", "-m", "Glob names"]);
    f.write("[a]*.txt", "glob edit\n");
    f.write("a.txt", "plain edit\n");
    repo.execute(&discard(f.plan("[a]*.txt"))).unwrap();
    assert_eq!(f.read("[a]*.txt"), b"glob\n");
    assert_eq!(f.read("a.txt"), b"plain edit\n");
    let status = repo.status().unwrap();
    assert_eq!(status.entries.len(), 1);
    assert_eq!(status.entries[0].path, Path::new("a.txt"));
}
