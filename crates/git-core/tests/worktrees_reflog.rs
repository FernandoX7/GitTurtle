use gitturtle_core::{GitRepository, WorktreeCommand, WriteCommand};
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
        let root = temp.path().join("main");
        GitRepository::init(&root, "main").unwrap();
        let root = GitRepository::open(&root).unwrap().path().to_owned();
        let f = Self { _temp: temp, root };
        f.git(&["config", "user.name", "Recovery Fixture"]);
        f.git(&["config", "user.email", "fixture@example.invalid"]);
        f.git(&["config", "commit.gpgSign", "false"]);
        fs::write(f.root.join("tracked"), "base\n").unwrap();
        f.git(&["add", "."]);
        f.git(&["commit", "-m", "Initial"]);
        f
    }
    fn repo(&self) -> GitRepository {
        GitRepository::open(&self.root).unwrap()
    }
    fn git(&self, args: &[&str]) -> String {
        git_at(&self.root, args)
    }
    fn destination(&self, name: &str) -> PathBuf {
        self.root.parent().unwrap().join(name)
    }
    fn create(&self, branch: &str) -> PathBuf {
        let repo = self.repo();
        let destination = self.destination(branch);
        let plan = repo
            .create_worktree_plan(&destination, branch, true, "HEAD")
            .unwrap();
        repo.execute(&WriteCommand::Worktree(Arc::new(WorktreeCommand::Create(
            plan,
        ))))
        .unwrap();
        destination
    }
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
fn worktree_creation_preserves_source_index_work_and_private_config() {
    let f = Fixture::new();
    let repo = f.repo();
    f.git(&["config", "extensions.worktreeConfig", "true"]);
    f.git(&["config", "--worktree", "user.name", "Private Main"]);
    fs::write(f.root.join("tracked"), "staged\n").unwrap();
    f.git(&["add", "tracked"]);
    fs::write(f.root.join("tracked"), "unstaged\n").unwrap();
    let head = f.git(&["rev-parse", "HEAD"]);
    let index = f.git(&["write-tree"]);
    let destination = f.create("parallel");
    assert_eq!(git_at(&destination, &["rev-parse", "HEAD"]), head);
    assert_eq!(f.git(&["write-tree"]), index);
    assert_eq!(fs::read(f.root.join("tracked")).unwrap(), b"unstaged\n");
    // Git initializes the new worktree's config from the source; subsequent
    // private edits stay separate, matching native worktree semantics.
    git_at(
        &destination,
        &["config", "--worktree", "user.name", "Private Linked"],
    );
    assert_eq!(
        git_at(&destination, &["config", "user.name"]),
        "Private Linked"
    );
    assert_eq!(f.git(&["config", "user.name"]), "Private Main");
    assert_eq!(
        repo.git_directories().unwrap().1,
        GitRepository::open(&destination)
            .unwrap()
            .git_directories()
            .unwrap()
            .1
    );
    assert!(
        repo.create_worktree_plan(&f.destination("occupied"), "parallel", false, "HEAD")
            .is_err()
    );
    assert!(
        repo.create_worktree_plan(&f.destination("occupied-main"), "main", false, "HEAD")
            .is_err()
    );
}

#[test]
fn existing_branch_creation_refuses_moving_ref_and_occupied_destination() {
    let f = Fixture::new();
    let repo = f.repo();
    f.git(&["branch", "ready"]);
    let destination = f.destination("ready-tree");
    let plan = repo
        .create_worktree_plan(&destination, "ready", false, "HEAD")
        .unwrap();
    fs::write(f.root.join("tracked"), "next\n").unwrap();
    f.git(&["commit", "-am", "Next"]);
    f.git(&["branch", "-f", "ready", "HEAD"]);
    assert!(
        repo.execute(&WriteCommand::Worktree(Arc::new(WorktreeCommand::Create(
            plan
        ))))
        .is_err()
    );
    assert!(!destination.exists());
    let plan = repo
        .create_worktree_plan(&destination, "ready", false, "HEAD")
        .unwrap();
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("unrelated"), "preserve").unwrap();
    assert!(
        repo.execute(&WriteCommand::Worktree(Arc::new(WorktreeCommand::Create(
            plan
        ))))
        .is_err()
    );
    assert_eq!(
        fs::read(destination.join("unrelated")).unwrap(),
        b"preserve"
    );
    let other = f.destination("ready-again");
    let plan = repo
        .create_worktree_plan(&other, "ready", false, "HEAD")
        .unwrap();
    repo.execute(&WriteCommand::Worktree(Arc::new(WorktreeCommand::Create(
        plan,
    ))))
    .unwrap();
    assert_eq!(
        git_at(&other, &["symbolic-ref", "--short", "HEAD"]),
        "ready"
    );
}

#[cfg(unix)]
#[test]
fn failed_checkout_hook_reports_retained_branch_without_cleanup_or_retry() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let repo = f.repo();
    let original = f.git(&["rev-parse", "HEAD"]);
    let hook = f.root.join(".git/hooks/post-checkout");
    fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o700)).unwrap();
    let plan = repo
        .create_worktree_plan(&f.destination("hook-failed"), "hook-branch", true, "HEAD")
        .unwrap();
    let error = repo
        .execute(&WriteCommand::Worktree(Arc::new(WorktreeCommand::Create(
            plan,
        ))))
        .unwrap_err()
        .to_string();
    assert!(error.contains("Branch 'hook-branch' remains"));
    assert_eq!(f.git(&["rev-parse", "hook-branch"]), original);
    assert_eq!(f.git(&["rev-parse", "HEAD"]), original);
    assert!(repo.status().unwrap().entries.is_empty());
}

#[test]
fn worktree_removal_refuses_dirty_ignored_locked_main_missing_and_stale_identity() {
    let f = Fixture::new();
    let repo = f.repo();
    let destination = f.create("parallel");
    let trees = repo.worktrees().unwrap();
    assert!(
        repo.worktree_details(&trees[0])
            .unwrap()
            .removal_blocked
            .is_some()
    );
    let tree = trees.iter().find(|tree| tree.path == destination).unwrap();
    let clean = repo.worktree_details(tree).unwrap();
    assert!(clean.removal_blocked.is_none());
    fs::write(destination.join("untracked"), "precious\n").unwrap();
    assert!(
        repo.execute(&WriteCommand::Worktree(Arc::new(WorktreeCommand::Remove(
            clean.clone()
        ))))
        .is_err()
    );
    assert_eq!(
        fs::read(destination.join("untracked")).unwrap(),
        b"precious\n"
    );
    fs::remove_file(destination.join("untracked")).unwrap();
    f.git(&["config", "core.excludesFile", "/dev/null"]);
    fs::write(f.root.join(".git/info/exclude"), "ignored\n").unwrap();
    fs::write(destination.join("ignored"), "ignored but valuable\n").unwrap();
    let dirty = repo.worktree_details(tree).unwrap();
    assert_eq!(dirty.ignored_files, 1);
    assert!(dirty.removal_blocked.is_some());
    fs::remove_file(destination.join("ignored")).unwrap();
    f.git(&["worktree", "lock", destination.to_str().unwrap()]);
    assert!(
        repo.execute(&WriteCommand::Worktree(Arc::new(WorktreeCommand::Remove(
            clean.clone()
        ))))
        .is_err()
    );
    let locked = repo
        .worktrees()
        .unwrap()
        .into_iter()
        .find(|tree| tree.path == destination)
        .unwrap();
    assert!(
        repo.worktree_details(&locked)
            .unwrap()
            .removal_blocked
            .unwrap()
            .contains("locked")
    );
    f.git(&["worktree", "unlock", destination.to_str().unwrap()]);
    let moved = f.destination("temporarily-away");
    fs::rename(&destination, &moved).unwrap();
    let missing = repo
        .worktrees()
        .unwrap()
        .into_iter()
        .find(|tree| tree.path == destination)
        .unwrap();
    assert!(repo.worktree_details(&missing).unwrap().missing);
    assert!(
        repo.execute(&WriteCommand::Worktree(Arc::new(WorktreeCommand::Remove(
            clean
        ))))
        .is_err()
    );
    fs::rename(moved, &destination).unwrap();
    let tree = repo
        .worktrees()
        .unwrap()
        .into_iter()
        .find(|tree| tree.path == destination)
        .unwrap();
    let clean = repo.worktree_details(&tree).unwrap();
    repo.execute(&WriteCommand::Worktree(Arc::new(WorktreeCommand::Remove(
        clean,
    ))))
    .unwrap();
    assert!(!destination.exists());
    assert_eq!(
        f.git(&["rev-parse", "parallel"]),
        f.git(&["rev-parse", "HEAD"])
    );
}

#[test]
fn reflog_recovery_preserves_current_head_index_work_and_refuses_stale_entries() {
    let f = Fixture::new();
    let repo = f.repo();
    let initial = f.git(&["rev-parse", "HEAD"]);
    fs::write(f.root.join("tracked"), "second\n").unwrap();
    f.git(&["commit", "-am", "Second"]);
    let page = repo.reflog("HEAD").unwrap();
    assert_eq!(page.entries[1].oid, initial);
    let plan = repo
        .reflog_recovery_plan(&page.entries[1], "recovery/initial")
        .unwrap();
    fs::write(f.root.join("tracked"), "staged\n").unwrap();
    f.git(&["add", "tracked"]);
    fs::write(f.root.join("tracked"), "independent\n").unwrap();
    let head = f.git(&["rev-parse", "HEAD"]);
    let index = f.git(&["write-tree"]);
    repo.execute(&WriteCommand::RecoverReflog(Arc::new(plan)))
        .unwrap();
    assert_eq!(f.git(&["rev-parse", "recovery/initial"]), initial);
    assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(f.git(&["write-tree"]), index);
    assert_eq!(fs::read(f.root.join("tracked")).unwrap(), b"independent\n");
    let plan = repo
        .reflog_recovery_plan(&page.entries[1], "stale-recovery")
        .unwrap();
    f.git(&[
        "update-ref",
        "-m",
        "External reflog update",
        "HEAD",
        &head,
        &head,
    ]);
    // update-ref on an unchanged value need not add an entry; a real commit does.
    f.git(&["commit", "-m", "Third"]);
    assert!(
        repo.execute(&WriteCommand::RecoverReflog(Arc::new(plan)))
            .is_err()
    );
    assert!(
        !repo
            .branches()
            .unwrap()
            .iter()
            .any(|branch| branch.name == "stale-recovery")
    );
}

#[test]
fn reflog_bounds_missing_objects_private_heads_and_symlink_safety() {
    let f = Fixture::new();
    let repo = f.repo();
    let destination = f.create("parallel");
    let linked = GitRepository::open(&destination).unwrap();
    assert_ne!(
        repo.reflog("HEAD").unwrap().entries[0].message,
        linked.reflog("HEAD").unwrap().entries[0].message
    );
    assert_eq!(
        repo.reflog("refs/heads/main").unwrap().entries[0].oid,
        linked.reflog("refs/heads/main").unwrap().entries[0].oid
    );
    assert!(repo.reflog("../../outside").is_err());
    let head = f.git(&["rev-parse", "HEAD"]);
    let missing = "1".repeat(head.len());
    let line = format!(
        "{head} {missing} Fixture <fixture@example.invalid> 1700000000 +0000\tMissing object\n"
    );
    fs::write(
        f.root.join(".git/logs/refs/heads/missing"),
        line.repeat(1001),
    )
    .unwrap();
    let page = repo.reflog("refs/heads/missing").unwrap();
    assert_eq!(page.entries.len(), 1000);
    assert!(page.truncated);
    assert!(
        repo.inspect_reflog_commit(&page.entries[0])
            .unwrap_err()
            .to_string()
            .contains("unavailable")
    );
    assert!(
        repo.reflog_recovery_plan(&page.entries[0], "cannot-recover")
            .is_err()
    );
    #[cfg(unix)]
    {
        fs::remove_file(f.root.join(".git/logs/refs/heads/missing")).unwrap();
        std::os::unix::fs::symlink(
            f.root.join("tracked"),
            f.root.join(".git/logs/refs/heads/missing"),
        )
        .unwrap();
        assert!(repo.reflog("refs/heads/missing").is_err());
    }
}
