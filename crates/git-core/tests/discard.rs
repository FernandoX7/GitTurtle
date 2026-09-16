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
