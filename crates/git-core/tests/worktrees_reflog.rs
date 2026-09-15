use gitturtle_core::{GitRepository, WorktreeCommand, WorktreeDetails, WriteCommand};
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
    fn removal_plan(&self, destination: &Path) -> WorktreeDetails {
        let repo = self.repo();
        let tree = repo
            .worktrees()
            .unwrap()
            .into_iter()
            .find(|tree| tree.path == destination)
            .unwrap();
        repo.worktree_details(&tree).unwrap()
    }
}

fn remove(plan: WorktreeDetails) -> WriteCommand {
    WriteCommand::Worktree(Arc::new(WorktreeCommand::Remove(plan)))
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
    fs::write(destination.join("tracked"), "valuable tracked edits\n").unwrap();
    assert!(repo.execute(&remove(clean.clone())).is_err());
    git_at(&destination, &["add", "tracked"]);
    assert!(repo.execute(&remove(clean.clone())).is_err());
    assert_eq!(
        fs::read(destination.join("tracked")).unwrap(),
        b"valuable tracked edits\n"
    );
    assert_eq!(
        git_at(&destination, &["show", ":tracked"]),
        "valuable tracked edits"
    );
    git_at(
        &destination,
        &["restore", "--staged", "--worktree", "--", "tracked"],
    );
    f.git(&["config", "status.showUntrackedFiles", "no"]);
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
    assert!(repo.execute(&remove(clean.clone())).is_err());
    assert!(repo.execute(&remove(dirty)).is_err());
    assert_eq!(
        fs::read(destination.join("ignored")).unwrap(),
        b"ignored but valuable\n"
    );
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
fn worktree_removal_cleans_registration_private_metadata_and_folder_preserving_other_work() {
    let f = Fixture::new();
    let repo = f.repo();
    f.git(&["config", "extensions.worktreeConfig", "true"]);
    f.git(&["config", "--worktree", "user.name", "Private Main"]);
    let destination = f.create("remove-target");
    let sibling = f.create("sibling");
    for (path, name) in [(&destination, "Target"), (&sibling, "Sibling")] {
        git_at(path, &["config", "--worktree", "user.name", name]);
    }
    // Source and sibling worktrees can both contain independent staged and
    // unstaged work. Removal must touch only the reviewed linked worktree.
    for path in [&f.root, &sibling] {
        fs::write(path.join("tracked"), "staged\n").unwrap();
        git_at(path, &["add", "tracked"]);
        fs::write(path.join("tracked"), "unstaged\n").unwrap();
        fs::write(path.join("untracked"), "keep me\n").unwrap();
    }
    let target_private = GitRepository::open(&destination)
        .unwrap()
        .git_directories()
        .unwrap()
        .0;
    assert!(target_private.join("index").is_file());
    assert!(target_private.join("logs/HEAD").is_file());
    assert!(target_private.join("config.worktree").is_file());
    let refs = f.git(&["show-ref"]);
    let config = fs::read(f.root.join(".git/config")).unwrap();
    let mut preserved = Vec::new();
    for path in [&f.root, &sibling] {
        let private = GitRepository::open(path)
            .unwrap()
            .git_directories()
            .unwrap()
            .0;
        for file in ["HEAD", "index", "config.worktree", "logs/HEAD"] {
            let path = private.join(file);
            preserved.push((path.clone(), fs::read(path).unwrap()));
        }
        for file in ["tracked", "untracked", ".git"] {
            let path = path.join(file);
            if path.is_file() {
                preserved.push((path.clone(), fs::read(path).unwrap()));
            }
        }
    }
    let outcome = repo.execute(&remove(f.removal_plan(&destination))).unwrap();
    assert!(outcome.message.contains("branch remains available"));
    assert_eq!(
        fs::symlink_metadata(&destination).unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    assert_eq!(
        fs::symlink_metadata(&target_private).unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    let trees = repo.worktrees().unwrap();
    assert_eq!(trees.len(), 2);
    assert!(trees.iter().any(|tree| tree.path == f.root));
    assert!(trees.iter().any(|tree| tree.path == sibling));
    assert_eq!(f.git(&["show-ref"]), refs);
    assert_eq!(fs::read(f.root.join(".git/config")).unwrap(), config);
    for (path, bytes) in preserved {
        assert_eq!(fs::read(&path).unwrap(), bytes, "{}", path.display());
    }
}

#[test]
fn worktree_removal_refuses_hidden_changes_and_preserves_index_flags() {
    for flag in ["--assume-unchanged", "--skip-worktree"] {
        let f = Fixture::new();
        let repo = f.repo();
        let destination = f.create("hidden-changes");
        let reviewed = f.removal_plan(&destination);
        git_at(&destination, &["update-index", flag, "tracked"]);
        fs::write(destination.join("tracked"), "hidden valuable work\n").unwrap();
        let linked = GitRepository::open(&destination).unwrap();
        let index = linked.git_directories().unwrap().0.join("index");
        let index_before = fs::read(&index).unwrap();
        // These flags suppress the very status checks used by non-force Git
        // removal; the additional guard must independently detect them.
        assert!(linked.status().unwrap().entries.is_empty());
        let blocked = f.removal_plan(&destination);
        assert!(
            blocked
                .removal_blocked
                .as_deref()
                .unwrap()
                .contains("assume-unchanged or skip-worktree")
        );
        assert!(repo.execute(&remove(reviewed)).is_err());
        assert!(repo.execute(&remove(blocked)).is_err());
        assert_eq!(
            fs::read(destination.join("tracked")).unwrap(),
            b"hidden valuable work\n"
        );
        assert_eq!(fs::read(index).unwrap(), index_before);
        assert_eq!(repo.worktrees().unwrap().len(), 2);
    }
}

#[test]
fn worktree_removal_cleans_detached_worktree_without_claiming_a_branch_was_retained() {
    let f = Fixture::new();
    let repo = f.repo();
    let destination = f.create("detached-target");
    git_at(&destination, &["checkout", "--detach"]);
    let private = GitRepository::open(&destination)
        .unwrap()
        .git_directories()
        .unwrap()
        .0;
    let refs = f.git(&["show-ref"]);
    let outcome = repo.execute(&remove(f.removal_plan(&destination))).unwrap();
    assert!(outcome.message.contains("Removed detached worktree"));
    assert!(!outcome.message.contains("branch remains"));
    assert!(!destination.exists());
    assert!(!private.exists());
    assert_eq!(repo.worktrees().unwrap().len(), 1);
    assert_eq!(f.git(&["show-ref"]), refs);
}

#[test]
fn worktree_removal_refuses_private_locks_and_operation_state_without_cleanup() {
    let f = Fixture::new();
    let repo = f.repo();
    let destination = f.create("private-state");
    let reviewed = f.removal_plan(&destination);
    let private = GitRepository::open(&destination)
        .unwrap()
        .git_directories()
        .unwrap()
        .0;
    for file in [
        "index.lock",
        "HEAD.lock",
        "config.worktree.lock",
        "BISECT_START",
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
    ] {
        let path = private.join(file);
        let contents = format!("{}\n", f.git(&["rev-parse", "HEAD"]));
        fs::write(&path, &contents).unwrap();
        let blocked = f.removal_plan(&destination);
        assert!(blocked.removal_blocked.is_some(), "{file}");
        assert!(repo.execute(&remove(reviewed.clone())).is_err(), "{file}");
        assert!(repo.execute(&remove(blocked)).is_err(), "{file}");
        assert_eq!(fs::read_to_string(&path).unwrap(), contents);
        assert_eq!(fs::read(destination.join("tracked")).unwrap(), b"base\n");
        fs::remove_file(path).unwrap();
    }
    for name in ["sequencer", "rebase-merge", "rebase-apply"] {
        let directory = private.join(name);
        fs::create_dir(&directory).unwrap();
        let blocked = f.removal_plan(&destination);
        assert!(blocked.removal_blocked.is_some(), "{name}");
        assert!(repo.execute(&remove(reviewed.clone())).is_err(), "{name}");
        assert!(repo.execute(&remove(blocked)).is_err(), "{name}");
        assert!(directory.is_dir());
        fs::remove_dir(directory).unwrap();
    }
    assert_eq!(repo.worktrees().unwrap().len(), 2);
}

#[test]
fn worktree_removal_refuses_stale_branch_commit_and_replaced_repository() {
    let f = Fixture::new();
    let repo = f.repo();
    let destination = f.create("stale-target");
    let reviewed = f.removal_plan(&destination);
    git_at(&destination, &["checkout", "--detach"]);
    assert!(repo.execute(&remove(reviewed)).is_err());
    let reviewed = f.removal_plan(&destination);
    git_at(&destination, &["commit", "--allow-empty", "-m", "Moved"]);
    assert!(repo.execute(&remove(reviewed)).is_err());
    let reviewed = f.removal_plan(&destination);
    let moved = f.destination("original-folder");
    fs::rename(&destination, &moved).unwrap();
    let replacement = Fixture::new();
    fs::rename(&replacement.root, &destination).unwrap();
    let replacement_head = git_at(&destination, &["rev-parse", "HEAD"]);
    assert!(repo.execute(&remove(reviewed)).is_err());
    assert_eq!(
        git_at(&destination, &["rev-parse", "HEAD"]),
        replacement_head
    );
    assert_eq!(fs::read(destination.join("tracked")).unwrap(), b"base\n");
    assert_eq!(fs::read(moved.join("tracked")).unwrap(), b"base\n");
    assert_eq!(repo.worktrees().unwrap().len(), 2);
}

#[test]
fn worktree_removal_refuses_replaced_directories_at_the_same_paths() {
    fn copy_directory(source: &Path, destination: &Path) {
        fs::create_dir(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let target = destination.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_directory(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    for replace_private in [false, true] {
        let f = Fixture::new();
        let repo = f.repo();
        let destination = f.create("same-path");
        let private = GitRepository::open(&destination)
            .unwrap()
            .git_directories()
            .unwrap()
            .0;
        let reviewed = f.removal_plan(&destination);
        let replaced = if replace_private {
            &private
        } else {
            &destination
        };
        let saved = f.destination("original-directory");
        fs::rename(replaced, &saved).unwrap();
        copy_directory(&saved, replaced);
        // Branch, commit, registration, clean status, and all path strings are
        // unchanged, but this is a different checkout/administration directory.
        assert_eq!(f.removal_plan(&destination).tree, reviewed.tree);
        let error = repo.execute(&remove(reviewed)).unwrap_err();
        assert!(error.to_string().contains("changed after review"));
        assert_eq!(fs::read(destination.join("tracked")).unwrap(), b"base\n");
        assert!(private.is_dir());
        assert!(saved.is_dir());
        assert_eq!(repo.worktrees().unwrap().len(), 2);

        repo.execute(&remove(f.removal_plan(&destination))).unwrap();
        assert!(!destination.exists());
        assert!(!private.exists());
        assert!(saved.is_dir());
    }
}

#[test]
#[cfg(unix)]
fn worktree_removal_refuses_redirected_administration_and_git_files() {
    use std::os::unix::fs::symlink;

    for redirect in ["private", "worktrees", "gitfile", "alias"] {
        let f = Fixture::new();
        let repo = f.repo();
        let destination = f.create("redirected");
        let reviewed = f.removal_plan(&destination);
        let private = GitRepository::open(&destination)
            .unwrap()
            .git_directories()
            .unwrap()
            .0;
        let external = f.destination("external-administration");
        let original_index = fs::read(private.join("index")).unwrap();
        let retained_index = match redirect {
            "private" => {
                fs::rename(&private, &external).unwrap();
                fs::write(
                    external.join("commondir"),
                    f.root.join(".git").as_os_str().as_encoded_bytes(),
                )
                .unwrap();
                symlink(&external, &private).unwrap();
                external.join("index")
            }
            "worktrees" => {
                let parent = private.parent().unwrap();
                fs::rename(parent, &external).unwrap();
                let moved = external.join(private.file_name().unwrap());
                fs::write(
                    moved.join("commondir"),
                    f.root.join(".git").as_os_str().as_encoded_bytes(),
                )
                .unwrap();
                symlink(&external, parent).unwrap();
                moved.join("index")
            }
            "gitfile" => {
                fs::rename(destination.join(".git"), &external).unwrap();
                symlink(&external, destination.join(".git")).unwrap();
                private.join("index")
            }
            "alias" => {
                symlink(&private, private.parent().unwrap().join("alias")).unwrap();
                private.join("index")
            }
            _ => unreachable!(),
        };
        // A plain non-force Git removal can follow an administration-root
        // symlink and empty its target. Refuse before dispatching that write.
        let tree = repo
            .worktrees()
            .unwrap()
            .into_iter()
            .find(|tree| tree.path == destination)
            .unwrap();
        assert!(repo.worktree_details(&tree).is_err(), "{redirect}");
        assert!(repo.execute(&remove(reviewed)).is_err(), "{redirect}");
        assert_eq!(
            fs::read(&retained_index).unwrap(),
            original_index,
            "{redirect}"
        );
        assert_eq!(fs::read(destination.join("tracked")).unwrap(), b"base\n");
        assert_eq!(
            git_at(&destination, &["rev-parse", "HEAD"]),
            f.git(&["rev-parse", "HEAD"])
        );
    }
}

#[test]
#[cfg(unix)]
fn worktree_removal_preserves_byte_paths_and_stored_symlink_targets() {
    #[cfg(target_os = "linux")]
    use std::os::unix::ffi::OsStringExt;
    use std::{ffi::OsString, os::unix::fs::symlink};

    let unicode = (
        OsString::from("linked-é\nname"),
        OsString::from(".é\nvaluable.keep"),
    );
    // APFS rejects invalid UTF-8 names. Keep Unicode/newline coverage on
    // every Unix platform and exercise arbitrary filename bytes on Linux.
    #[cfg(target_os = "linux")]
    let names = [
        unicode,
        (
            OsString::from_vec(b"linked-\xff\nname".to_vec()),
            OsString::from_vec(b".\xff\nvaluable.keep".to_vec()),
        ),
    ];
    #[cfg(not(target_os = "linux"))]
    let names = [unicode];

    for (directory_name, ignored_name) in names {
        let f = Fixture::new();
        let repo = f.repo();
        let external = f.destination("external-content");
        fs::write(&external, b"valuable outside content\n").unwrap();
        symlink(&external, f.root.join("stored-link")).unwrap();
        fs::write(f.root.join(".gitignore"), b"*.keep\n").unwrap();
        f.git(&["add", "stored-link", ".gitignore"]);
        f.git(&["commit", "-m", "Stored link and ignore rule"]);
        let destination = f.root.parent().unwrap().join(directory_name);
        let plan = repo
            .create_worktree_plan(&destination, "byte-path", true, "HEAD")
            .unwrap();
        repo.execute(&WriteCommand::Worktree(Arc::new(WorktreeCommand::Create(
            plan,
        ))))
        .unwrap();
        let reviewed = f.removal_plan(&destination);
        let ignored = destination.join(ignored_name);
        fs::write(&ignored, b"ignored local content\n").unwrap();
        let blocked = f.removal_plan(&destination);
        assert_eq!(blocked.ignored_files, 1);
        assert!(repo.execute(&remove(reviewed)).is_err());
        assert!(repo.execute(&remove(blocked)).is_err());
        assert_eq!(fs::read(&ignored).unwrap(), b"ignored local content\n");
        fs::remove_file(ignored).unwrap();
        repo.execute(&remove(f.removal_plan(&destination))).unwrap();
        assert!(!destination.exists());
        assert_eq!(fs::read(&external).unwrap(), b"valuable outside content\n");
        assert_eq!(repo.worktrees().unwrap().len(), 1);
    }
}

#[test]
fn worktree_removal_refuses_current_linked_tree_and_preserves_initialized_submodules() {
    let f = Fixture::new();
    let submodule = Fixture::new();
    f.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        submodule.root.to_str().unwrap(),
        "module",
    ]);
    f.git(&["commit", "-m", "Add submodule"]);
    let repo = f.repo();
    let destination = f.create("submodule-target");
    git_at(
        &destination,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "update",
            "--init",
        ],
    );
    let linked = GitRepository::open(&destination).unwrap();
    let tree = repo
        .worktrees()
        .unwrap()
        .into_iter()
        .find(|tree| tree.path == destination)
        .unwrap();
    let current = linked.worktree_details(&tree).unwrap();
    assert!(current.current);
    assert!(
        current
            .removal_blocked
            .as_deref()
            .unwrap()
            .contains("currently open")
    );
    assert!(linked.execute(&remove(current)).is_err());
    let reviewed = f.removal_plan(&destination);
    assert!(reviewed.removal_blocked.is_none());
    let private = linked.git_directories().unwrap().0;
    let refs = f.git(&["show-ref"]);
    let error = repo.execute(&remove(reviewed)).unwrap_err();
    assert!(error.to_string().contains("did not report success"));
    assert!(format!("{error:#}").contains("submodules"));
    assert!(private.join("index").is_file());
    assert_eq!(
        fs::read(destination.join("module/tracked")).unwrap(),
        b"base\n"
    );
    assert_eq!(repo.worktrees().unwrap().len(), 2);
    assert_eq!(f.git(&["show-ref"]), refs);
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
