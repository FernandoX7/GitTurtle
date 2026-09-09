//! Recovery mutations run only in disposable repositories and local remotes.
use gitturtle_core::{
    ConflictResolution, GitRepository, IntegrationCommand, OperationKind, RecoveryCommand,
    RecoveryKind, WriteCommand, WriteOutcome,
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::Arc,
};
use tempfile::TempDir;

struct Fixture {
    _temp: TempDir,
    root: PathBuf,
}

fn create_stash(f: &Fixture, name: &str, include_untracked: bool) -> gitturtle_core::StashEntry {
    let plan = f.repo().stash_create_plan(include_untracked).unwrap();
    f.recover(RecoveryCommand::CreateStash {
        plan,
        name: name.into(),
    })
    .unwrap();
    f.repo().stash_list(0, 20).unwrap().entries.remove(0)
}

#[test]
fn named_stash_inspection_preserves_staged_unstaged_and_untracked_versions() {
    let f = Fixture::new();
    let base = f.base();
    f.write(".git/info/exclude", "ignored.txt\n");
    f.write("ignored.txt", "ignored stays local\n");
    f.write("file.txt", "staged version\n");
    f.git(&["add", "file.txt"]);
    f.write("file.txt", "working version\n");
    f.write("untracked.txt", "untracked version\n");
    let stash = create_stash(&f, "Before reorganizing the turtle", true);
    assert!(stash.name.contains("Before reorganizing the turtle"));
    assert_eq!(stash.ordinal, 0);
    assert_eq!(f.head(), base);
    assert_eq!(f.read("file.txt"), b"base\n");
    assert_eq!(f.staged("file.txt"), b"base\n");
    assert!(!f.root.join("untracked.txt").exists());
    assert_eq!(f.read("ignored.txt"), b"ignored stays local\n");
    let repo = f.repo();
    let before_index = f.index();
    let snapshot = repo.stash_snapshot(&stash).unwrap();
    assert_eq!(snapshot.base_oid, base);
    assert_eq!(snapshot.stash.oid, stash.oid);
    let stage = snapshot
        .staged
        .iter()
        .find(|file| file.path() == Path::new("file.txt"))
        .unwrap();
    let working = snapshot
        .unstaged
        .iter()
        .find(|file| file.path() == Path::new("file.txt"))
        .unwrap();
    let untracked = snapshot
        .untracked
        .iter()
        .find(|file| file.path() == Path::new("untracked.txt"))
        .unwrap();
    assert_eq!(
        repo.blob(stage.old_oid.as_ref().unwrap()).unwrap(),
        b"base\n"
    );
    assert_eq!(
        repo.blob(stage.new_oid.as_ref().unwrap()).unwrap(),
        b"staged version\n"
    );
    assert_eq!(
        repo.blob(working.old_oid.as_ref().unwrap()).unwrap(),
        b"staged version\n"
    );
    assert_eq!(
        repo.blob(working.new_oid.as_ref().unwrap()).unwrap(),
        b"working version\n"
    );
    assert_eq!(
        repo.blob(untracked.new_oid.as_ref().unwrap()).unwrap(),
        b"untracked version\n"
    );
    assert_eq!(f.index(), before_index);
}

#[test]
fn stash_apply_restores_reviewed_staging_without_removing_the_stash_or_other_work() {
    let f = Fixture::new();
    f.base();
    f.write("file.txt", "staged version\n");
    f.git(&["add", "file.txt"]);
    f.write("file.txt", "working version\n");
    f.write("untracked.txt", "stashed untracked\n");
    let stash = create_stash(&f, "Keep staging", true);
    f.write("independent.txt", "independent unstaged work\n");
    f.write("other.txt", "other untracked work\n");
    let plan = f.repo().stash_apply_plan(&stash, true).unwrap();
    f.recover(RecoveryCommand::ApplyStash { plan }).unwrap();
    assert_eq!(f.staged("file.txt"), b"staged version\n");
    assert_eq!(f.read("file.txt"), b"working version\n");
    assert_eq!(f.read("untracked.txt"), b"stashed untracked\n");
    assert_eq!(f.read("independent.txt"), b"independent unstaged work\n");
    assert_eq!(f.read("other.txt"), b"other untracked work\n");
    assert_eq!(
        f.repo().stash_list(0, 20).unwrap().entries[0].oid,
        stash.oid
    );
}

#[test]
fn stash_conflict_preserves_saved_entry_and_unrelated_work() {
    let f = Fixture::new();
    f.base();
    f.write("file.txt", "stashed conflicting change\n");
    let stash = create_stash(&f, "Conflict recovery", false);
    f.write("file.txt", "new committed change\n");
    f.commit("diverged after stash");
    f.write("independent.txt", "independent work\n");
    f.write("other.txt", "untracked work\n");
    let plan = f.repo().stash_apply_plan(&stash, false).unwrap();
    let error = f.recover(RecoveryCommand::ApplyStash { plan }).unwrap_err();
    let diagnostic = format!("{error:#}");
    assert!(
        diagnostic.starts_with("Stash restoration produced conflicts. The stash remains saved."),
        "{diagnostic}"
    );
    assert!(
        diagnostic.contains("Git stdout:\nAuto-merging file.txt"),
        "{diagnostic}"
    );
    assert!(
        diagnostic.contains("CONFLICT (content): Merge conflict in file.txt"),
        "{diagnostic}"
    );
    assert!(diagnostic.contains("Git stderr:"), "{diagnostic}");
    assert!(diagnostic.contains("has no Continue or Abort operation"));
    assert!(!diagnostic.contains("then use Continue"));
    assert!(f.repo().operation_state().unwrap().is_none());
    assert!(
        f.repo()
            .status()
            .unwrap()
            .entries
            .iter()
            .any(|entry| entry.conflicted && entry.path == Path::new("file.txt"))
    );
    assert_eq!(
        f.repo().stash_list(0, 20).unwrap().entries[0].oid,
        stash.oid
    );
    assert_eq!(f.read("independent.txt"), b"independent work\n");
    assert_eq!(f.read("other.txt"), b"untracked work\n");
}

#[test]
fn failed_stash_apply_reports_git_lock_failure_without_claiming_conflicts() {
    let f = Fixture::new();
    f.base();
    f.write("file.txt", "saved changes\n");
    let stash = create_stash(&f, "Retain on ordinary failure", false);
    f.write("independent.txt", "independent work\n");
    f.write("other.txt", "untracked work\n");
    let plan = f.repo().stash_apply_plan(&stash, false).unwrap();
    let head = f.head();
    let index = f.index();
    // Passive preparation can read the existing index while a different writer
    // owns its lock. The actual stash write must fail and leave that lock alone.
    f.write(".git/index.lock", "another writer owns this lock\n");
    let error = f.recover(RecoveryCommand::ApplyStash { plan }).unwrap_err();
    let diagnostic = format!("{error:#}");
    assert!(
        diagnostic.starts_with("Stash restoration did not complete. The stash remains saved."),
        "{diagnostic}"
    );
    assert!(!diagnostic.contains("produced conflicts"));
    assert!(diagnostic.contains("Git stderr:"));
    assert!(
        diagnostic.contains("An index.lock is present in this working copy"),
        "{diagnostic}"
    );
    assert!(!diagnostic.contains("then use Continue"));
    assert_eq!(f.head(), head);
    assert_eq!(f.index(), index);
    assert_eq!(f.read("file.txt"), b"base\n");
    assert_eq!(f.read("independent.txt"), b"independent work\n");
    assert_eq!(f.read("other.txt"), b"untracked work\n");
    assert_eq!(
        f.read(".git/index.lock"),
        b"another writer owns this lock\n"
    );
    assert!(
        !f.repo()
            .status()
            .unwrap()
            .entries
            .iter()
            .any(|entry| entry.conflicted)
    );
    assert_eq!(
        f.repo().stash_list(0, 20).unwrap().entries[0].oid,
        stash.oid
    );
}

#[test]
fn failed_stash_restore_keeps_untracked_collision_and_saved_entry() {
    let f = Fixture::new();
    f.base();
    f.write("untracked.txt", "saved content\n");
    let stash = create_stash(&f, "Untracked recovery", true);
    f.write("untracked.txt", "new independent content\n");
    f.write("other.txt", "keep this too\n");
    let head = f.head();
    let result = f
        .repo()
        .stash_apply_plan(&stash, false)
        .and_then(|plan| f.recover(RecoveryCommand::ApplyStash { plan }));
    assert!(result.is_err());
    assert_eq!(f.head(), head);
    assert_eq!(f.read("untracked.txt"), b"new independent content\n");
    assert_eq!(f.read("other.txt"), b"keep this too\n");
    assert_eq!(
        f.repo().stash_list(0, 20).unwrap().entries[0].oid,
        stash.oid
    );
}

#[test]
fn stash_drop_rechecks_reflog_position_and_only_removes_the_reviewed_entry() {
    let f = Fixture::new();
    f.base();
    f.write("file.txt", "first stash\n");
    let first = create_stash(&f, "First", false);
    f.write("file.txt", "second stash\n");
    let second = create_stash(&f, "Second", false);
    let before = f.repo().stash_list(0, 20).unwrap();
    assert!(
        f.recover(RecoveryCommand::DropStash {
            stash: first.clone()
        })
        .is_err()
    );
    assert_eq!(
        f.repo()
            .stash_list(0, 20)
            .unwrap()
            .entries
            .iter()
            .map(|stash| &stash.oid)
            .collect::<Vec<_>>(),
        before
            .entries
            .iter()
            .map(|stash| &stash.oid)
            .collect::<Vec<_>>()
    );
    let reviewed = f.repo().stash_list(0, 1).unwrap();
    assert!(reviewed.next_offset.is_some());
    let older = f
        .repo()
        .stash_list(reviewed.next_offset.unwrap(), 1)
        .unwrap()
        .entries
        .remove(0);
    assert_eq!(older.oid, first.oid);
    f.recover(RecoveryCommand::DropStash { stash: older })
        .unwrap();
    let remaining = f.repo().stash_list(0, 20).unwrap();
    assert_eq!(remaining.entries.len(), 1);
    assert_eq!(remaining.entries[0].oid, second.oid);
}

#[test]
fn stash_create_and_apply_refuse_stale_working_index_and_cross_repository_plans() {
    let f = Fixture::new();
    f.base();
    f.write("file.txt", "reviewed bytes\n");
    let plan = f.repo().stash_create_plan(false).unwrap();
    f.write("file.txt", "changed since review\n");
    assert!(
        f.recover(RecoveryCommand::CreateStash {
            plan,
            name: "stale".into()
        })
        .is_err()
    );
    assert_eq!(f.read("file.txt"), b"changed since review\n");
    assert!(f.repo().stash_list(0, 20).unwrap().entries.is_empty());
    let stash = create_stash(&f, "Fresh", false);
    let plan = f.repo().stash_apply_plan(&stash, false).unwrap();
    f.write("independent.txt", "new staged content\n");
    f.git(&["add", "independent.txt"]);
    let index = f.index();
    assert!(f.recover(RecoveryCommand::ApplyStash { plan }).is_err());
    assert_eq!(f.index(), index);
    let other = f.copied_repository();
    let other_index = other.index();
    assert_eq!(
        other.repo().stash_list(0, 20).unwrap().entries[0].oid,
        stash.oid
    );
    assert!(
        other
            .recover(RecoveryCommand::DropStash {
                stash: stash.clone()
            })
            .is_err()
    );
    assert_eq!(other.index(), other_index);
    assert_eq!(
        other.repo().stash_list(0, 20).unwrap().entries[0].oid,
        stash.oid
    );
    assert_eq!(
        f.repo().stash_list(0, 20).unwrap().entries[0].oid,
        stash.oid
    );
}

#[test]
fn stash_without_untracked_leaves_other_files_and_empty_stash_requests_are_explicit() {
    let f = Fixture::new();
    f.base();
    assert!(f.repo().stash_create_plan(false).is_err());
    f.write("file.txt", "tracked work\n");
    f.write("untracked.txt", "keep local\n");
    let stash = create_stash(&f, "Tracked only", false);
    assert_eq!(f.read("untracked.txt"), b"keep local\n");
    assert!(
        f.repo()
            .stash_snapshot(&stash)
            .unwrap()
            .untracked
            .is_empty()
    );
    assert!(f.repo().stash_list(0, 501).is_err());
}

#[test]
fn duplicate_stash_reflog_entries_cannot_impersonate_a_previously_reviewed_drop() {
    let f = Fixture::new();
    f.base();
    f.write("file.txt", "saved work\n");
    let reviewed = create_stash(&f, "Same description", false);
    // Storing the current OID is a Git no-op. Move the ref through another
    // stash, then restore the original OID as a genuinely new reflog entry.
    f.write("file.txt", "intermediate saved work\n");
    create_stash(&f, "Intermediate", false);
    let output = Fixture::command_at(&f.root)
        .args(["stash", "store", "--message", &reviewed.name, &reviewed.oid])
        .env(
            "GIT_COMMITTER_DATE",
            format!("@{} +0000", reviewed.timestamp),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let current = f.repo().stash_list(0, 20).unwrap();
    assert_eq!(current.entries.len(), 3);
    assert_eq!(current.entries[0].oid, reviewed.oid);
    assert_eq!(current.entries[0].name, reviewed.name);
    assert_eq!(current.entries[0].timestamp, reviewed.timestamp);
    assert!(
        f.recover(RecoveryCommand::DropStash {
            stash: reviewed.clone()
        })
        .is_err()
    );
    assert!(f.repo().stash_apply_plan(&reviewed, false).is_err());
    assert_eq!(f.repo().stash_list(0, 20).unwrap().entries.len(), 3);
}

#[test]
fn undo_rechecks_remote_publication_after_the_plan_was_reviewed() {
    let f = Fixture::new();
    f.base();
    f.write("file.txt", "local work\n");
    let tip = f.commit("initially local");
    let plan = f
        .repo()
        .recovery_plan(RecoveryKind::Undo, None, None)
        .unwrap();
    let bare = f._temp.path().join("remote.git");
    assert!(
        Command::new("git")
            .args(["init", "--bare"])
            .arg(&bare)
            .output()
            .unwrap()
            .status
            .success()
    );
    f.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
    f.git(&["push", "-u", "origin", "main"]);
    let index = f.index();
    assert!(f.recover(RecoveryCommand::Undo { plan }).is_err());
    assert_eq!(f.head(), tip);
    assert_eq!(f.index(), index);
}

#[test]
fn linked_worktree_stash_uses_shared_storage_but_only_changes_the_selected_worktree() {
    let f = Fixture::new();
    let main = f.base();
    let linked = f._temp.path().join("linked");
    f.git(&["worktree", "add", "-b", "linked", linked.to_str().unwrap()]);
    let repo = GitRepository::open(&linked).unwrap();
    fs::write(linked.join("file.txt"), b"linked local work\n").unwrap();
    let main_index = f.index();
    let plan = repo.stash_create_plan(false).unwrap();
    assert!(
        f.recover(RecoveryCommand::CreateStash {
            plan: plan.clone(),
            name: "Wrong worktree".into()
        })
        .is_err()
    );
    repo.execute(&WriteCommand::Recovery(
        RecoveryCommand::CreateStash {
            plan,
            name: "Linked work".into(),
        }
        .into(),
    ))
    .unwrap();
    assert_eq!(f.head(), main);
    assert_eq!(f.index(), main_index);
    assert_eq!(f.read("file.txt"), b"base\n");
    assert_eq!(fs::read(linked.join("file.txt")).unwrap(), b"base\n");
    let shared = f.repo().stash_list(0, 20).unwrap();
    assert_eq!(shared.entries.len(), 1);
    assert!(shared.entries[0].name.contains("Linked work"));
    let stash = repo.stash_list(0, 20).unwrap().entries.remove(0);
    assert_eq!(stash.oid, shared.entries[0].oid);
    let plan = repo.stash_apply_plan(&stash, false).unwrap();
    assert!(
        f.recover(RecoveryCommand::ApplyStash { plan: plan.clone() })
            .is_err()
    );
    repo.execute(&WriteCommand::Recovery(
        RecoveryCommand::ApplyStash { plan }.into(),
    ))
    .unwrap();
    assert_eq!(
        fs::read(linked.join("file.txt")).unwrap(),
        b"linked local work\n"
    );
    assert_eq!(f.head(), main);
    assert_eq!(f.index(), main_index);
    assert_eq!(f.read("file.txt"), b"base\n");
    assert_eq!(f.repo().stash_list(0, 20).unwrap().entries.len(), 1);
}

#[cfg(unix)]
#[test]
fn stash_inspection_remains_passive_with_newline_paths_and_configured_helpers() {
    let f = Fixture::new();
    f.base();
    f.write("line\nbreak.txt", "stored bytes\n");
    f.git(&["add", "line\nbreak.txt"]);
    let stash = create_stash(&f, "Byte-safe snapshot", false);
    f.executable(
        ".git/read-helper",
        "#!/bin/sh\ntouch .git/helper-ran\ncat\n",
    );
    let helper = f.root.join(".git/read-helper");
    for key in [
        "diff.hostile.command",
        "diff.hostile.textconv",
        "filter.hostile.clean",
        "filter.hostile.smudge",
        "core.fsmonitor",
    ] {
        f.git(&["config", key, helper.to_str().unwrap()]);
    }
    f.write(".git/info/attributes", "* diff=hostile filter=hostile\n");
    let index = f.index();
    let stash_ref = f.git(&["rev-parse", "refs/stash"]);
    let repo = f.repo();
    let snapshot = repo.stash_snapshot(&stash).unwrap();
    let file = snapshot
        .staged
        .iter()
        .find(|file| file.path() == Path::new("line\nbreak.txt"))
        .unwrap();
    assert_eq!(
        repo.blob(file.new_oid.as_ref().unwrap()).unwrap(),
        b"stored bytes\n"
    );
    assert_eq!(f.index(), index);
    assert_eq!(f.git(&["rev-parse", "refs/stash"]), stash_ref);
    assert!(!f.root.join(".git/helper-ran").exists());
}

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("work");
        GitRepository::init(&root, "main").unwrap();
        let fixture = Self { _temp: temp, root };
        for (key, value) in [
            ("user.name", "Recovery Fixture"),
            ("user.email", "recovery@example.invalid"),
            ("commit.gpgsign", "false"),
            ("core.hooksPath", ".git/hooks"),
            ("core.fsmonitor", "false"),
            ("rerere.enabled", "false"),
        ] {
            fixture.git(&["config", key, value]);
        }
        fixture
    }

    fn command_at(path: &Path) -> Command {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(path)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_EDITOR", "true");
        for name in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_INDEX_FILE",
            "GIT_PREFIX",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_PARAMETERS",
        ] {
            command.env_remove(name);
        }
        command
    }

    fn output(&self, args: &[&str]) -> Output {
        Self::command_at(&self.root).args(args).output().unwrap()
    }
    fn git(&self, args: &[&str]) -> String {
        let output = self.output(args);
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_owned()
    }
    fn git_input(&self, args: &[&str], input: &[u8]) -> Vec<u8> {
        let mut child = Self::command_at(&self.root)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }
    fn write(&self, path: &str, bytes: impl AsRef<[u8]>) {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    fn read(&self, path: &str) -> Vec<u8> {
        fs::read(self.root.join(path)).unwrap()
    }
    fn repo(&self) -> GitRepository {
        GitRepository::open(&self.root).unwrap()
    }
    fn copied_repository(&self) -> Self {
        fn copy_tree(source: &Path, destination: &Path) {
            fs::create_dir_all(destination).unwrap();
            for entry in fs::read_dir(source).unwrap() {
                let entry = entry.unwrap();
                let target = destination.join(entry.file_name());
                if entry.file_type().unwrap().is_dir() {
                    copy_tree(&entry.path(), &target);
                } else {
                    fs::copy(entry.path(), target).unwrap();
                }
            }
        }
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("work");
        copy_tree(&self.root, &root);
        Self { _temp: temp, root }
    }
    fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"])
    }
    fn index(&self) -> Vec<u8> {
        self.read(".git/index")
    }
    fn staged(&self, path: &str) -> Vec<u8> {
        self.output(&["show", &format!(":{path}")]).stdout
    }
    fn commit(&self, message: &str) -> String {
        self.git(&["add", "--all"]);
        self.git(&["commit", "--message", message]);
        self.head()
    }
    fn base(&self) -> String {
        self.write("file.txt", "base\n");
        self.write("independent.txt", "independent base\n");
        self.commit("base")
    }
    fn recover(&self, command: RecoveryCommand) -> anyhow::Result<WriteOutcome> {
        self.repo().execute(&WriteCommand::Recovery(command.into()))
    }
    fn raw_message(&self, oid: &str) -> Vec<u8> {
        let commit = self.git_input(&["cat-file", "commit", oid], b"");
        let start = commit
            .windows(2)
            .position(|bytes| bytes == b"\n\n")
            .unwrap()
            + 2;
        commit[start..].to_vec()
    }
    #[cfg(unix)]
    fn executable(&self, path: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        self.write(path, body);
        fs::set_permissions(self.root.join(path), fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn amend_preserves_exact_message_and_includes_only_the_reviewed_index() {
    let f = Fixture::new();
    let original = f.base();
    f.write("file.txt", "staged replacement\n");
    f.git(&["add", "file.txt"]);
    f.write("file.txt", "later unstaged work\n");
    f.write("independent.txt", "independent edits\n");
    f.write("untracked.txt", "untracked work\n");
    let plan = f
        .repo()
        .recovery_plan(RecoveryKind::Amend, None, None)
        .unwrap();
    assert_eq!(plan.current.head, original);
    assert_eq!(plan.target.message.as_bytes(), f.raw_message(&original));
    let message = "  Revised title  \n\n# Keep this explanation\n\n  Retain indentation and trailing spaces  \n\n".to_owned();
    f.recover(RecoveryCommand::Amend {
        plan,
        message: message.clone(),
    })
    .unwrap();
    assert_ne!(f.head(), original);
    assert_eq!(f.raw_message(&f.head()), message.as_bytes());
    assert_eq!(
        f.output(&["show", "HEAD:file.txt"]).stdout,
        b"staged replacement\n"
    );
    assert_eq!(f.staged("file.txt"), b"staged replacement\n");
    assert_eq!(f.read("file.txt"), b"later unstaged work\n");
    assert_eq!(f.read("independent.txt"), b"independent edits\n");
    assert_eq!(f.read("untracked.txt"), b"untracked work\n");
}

#[cfg(unix)]
#[test]
fn amend_runs_configured_hooks_and_does_not_bypass_hook_or_signing_failure() {
    for failure in ["none", "hook", "signing"] {
        let f = Fixture::new();
        let head = f.base();
        f.write("file.txt", "staged\n");
        f.git(&["add", "file.txt"]);
        f.write("file.txt", "unstaged\n");
        f.executable(".git/hooks/pre-commit", if failure == "hook" {
            "#!/bin/sh\nprintf 'ran' > .git/hook-ran\nprintf 'fixture hook refused\\n' >&2\nexit 1\n"
        } else { "#!/bin/sh\nprintf 'ran' > .git/hook-ran\n" });
        if failure == "signing" {
            f.executable(
                ".git/signing-failure",
                "#!/bin/sh\nprintf 'fixture signing refused\\n' >&2\nexit 1\n",
            );
            f.git(&["config", "commit.gpgsign", "true"]);
            f.git(&["config", "gpg.format", "openpgp"]);
            f.git(&[
                "config",
                "gpg.program",
                f.root.join(".git/signing-failure").to_str().unwrap(),
            ]);
        }
        let plan = f
            .repo()
            .recovery_plan(RecoveryKind::Amend, None, None)
            .unwrap();
        let result = f.recover(RecoveryCommand::Amend {
            plan,
            message: "Amended through configured Git\n".into(),
        });
        assert_eq!(f.read(".git/hook-ran"), b"ran");
        if failure == "none" {
            assert!(result.is_ok());
            assert_ne!(f.head(), head);
        } else {
            assert!(result.is_err());
            assert_eq!(f.head(), head);
        }
        assert_eq!(f.staged("file.txt"), b"staged\n");
        assert_eq!(f.read("file.txt"), b"unstaged\n");
    }
}

#[test]
fn undo_local_commit_moves_only_head_preserving_exact_index_and_working_files() {
    let f = Fixture::new();
    let base = f.base();
    f.write("file.txt", "last committed change\n");
    let tip = f.commit("local work");
    f.write("file.txt", "new staged work\n");
    f.git(&["add", "file.txt"]);
    f.write("file.txt", "new unstaged work\n");
    f.write("independent.txt", "other local edits\n");
    f.write("untracked.txt", "untracked\n");
    let plan = f
        .repo()
        .recovery_plan(RecoveryKind::Undo, None, None)
        .unwrap();
    assert_eq!(plan.current.head, tip);
    assert!(plan.known_published_refs.is_empty());
    let index = f.index();
    f.recover(RecoveryCommand::Undo { plan }).unwrap();
    assert_eq!(f.head(), base);
    assert_eq!(f.index(), index);
    assert_eq!(f.staged("file.txt"), b"new staged work\n");
    assert_eq!(f.read("file.txt"), b"new unstaged work\n");
    assert_eq!(f.read("independent.txt"), b"other local edits\n");
    assert_eq!(f.read("untracked.txt"), b"untracked\n");
    assert_eq!(
        f.git(&["reflog", "--format=%H", "-n", "2"]).lines().nth(1),
        Some(tip.as_str())
    );
}

#[test]
fn undo_refuses_root_merge_and_commits_known_to_remote_history() {
    let f = Fixture::new();
    f.base();
    assert!(
        f.repo()
            .recovery_plan(RecoveryKind::Undo, None, None)
            .is_err()
    );
    f.write("file.txt", "second\n");
    let published = f.commit("published commit");
    let bare = f._temp.path().join("remote.git");
    let output = Command::new("git")
        .args(["init", "--bare"])
        .arg(&bare)
        .output()
        .unwrap();
    assert!(output.status.success());
    f.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
    f.git(&["push", "-u", "origin", "main"]);
    let error = f
        .repo()
        .recovery_plan(RecoveryKind::Undo, None, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("publish") || error.contains("remote"));
    assert_eq!(f.head(), published);
    f.git(&["switch", "-c", "side"]);
    f.write("side.txt", "side\n");
    f.commit("side");
    f.git(&["switch", "main"]);
    f.write("main.txt", "main\n");
    f.commit("main");
    f.git(&["merge", "--no-ff", "--no-edit", "side"]);
    let merge = f.head();
    assert!(
        f.repo()
            .recovery_plan(RecoveryKind::Undo, None, None)
            .is_err()
    );
    assert_eq!(f.head(), merge);
}

#[test]
fn recovery_plans_reject_changed_index_head_and_another_repository() {
    let f = Fixture::new();
    f.base();
    f.write("file.txt", "second\n");
    f.commit("second");
    let repo = f.repo();
    let head = f.head();
    let plan = repo.recovery_plan(RecoveryKind::Undo, None, None).unwrap();
    f.write("file.txt", "changed after review\n");
    f.git(&["add", "file.txt"]);
    let index = f.index();
    assert!(f.recover(RecoveryCommand::Undo { plan }).is_err());
    assert_eq!(f.head(), head);
    assert_eq!(f.index(), index);
    let plan = repo.recovery_plan(RecoveryKind::Amend, None, None).unwrap();
    f.commit("a newer commit");
    let newer = f.head();
    assert!(
        f.recover(RecoveryCommand::Amend {
            plan,
            message: "stale amendment\n".into()
        })
        .is_err()
    );
    assert_eq!(f.head(), newer);
    let plan = repo.recovery_plan(RecoveryKind::Undo, None, None).unwrap();
    let other = f.copied_repository();
    let other_head = other.head();
    let other_index = other.index();
    assert_eq!(other_head, f.head());
    assert_eq!(other_index, f.index());
    assert!(other.recover(RecoveryCommand::Undo { plan }).is_err());
    assert_eq!(other.head(), other_head);
    assert_eq!(other.index(), other_index);
}

#[test]
fn revert_published_change_preserves_unrelated_unstaged_work() {
    let f = Fixture::new();
    f.base();
    f.write("file.txt", "published change\n");
    let target = f.commit("published feature");
    f.git(&["update-ref", "refs/remotes/origin/main", &target]);
    f.write("independent.txt", "keep unrelated work\n");
    f.write("untracked.txt", "keep untracked\n");
    let plan = f
        .repo()
        .recovery_plan(RecoveryKind::Revert, Some(&target), None)
        .unwrap();
    assert_eq!(plan.target.oid, target);
    f.recover(RecoveryCommand::Revert { plan }).unwrap();
    assert_ne!(f.head(), target);
    assert_eq!(f.git(&["rev-parse", "HEAD^"]), target);
    assert_eq!(f.read("file.txt"), b"base\n");
    assert_eq!(f.staged("file.txt"), b"base\n");
    assert_eq!(f.read("independent.txt"), b"keep unrelated work\n");
    assert_eq!(f.read("untracked.txt"), b"keep untracked\n");
    assert!(f.repo().operation_state().unwrap().is_none());
}

#[test]
fn cherry_pick_selected_commit_preserves_unrelated_unstaged_work() {
    let f = Fixture::new();
    f.base();
    f.git(&["switch", "-c", "source"]);
    f.write("file.txt", "picked change\n");
    let target = f.commit("pick this description\n\nFull explanation");
    f.git(&["switch", "main"]);
    f.write("main.txt", "main history\n");
    let head = f.commit("main history");
    f.write("independent.txt", "keep unrelated work\n");
    let plan = f
        .repo()
        .recovery_plan(RecoveryKind::CherryPick, Some(&target), None)
        .unwrap();
    f.recover(RecoveryCommand::CherryPick { plan }).unwrap();
    assert_eq!(f.git(&["rev-parse", "HEAD^"]), head);
    assert_eq!(f.read("file.txt"), b"picked change\n");
    assert_eq!(f.raw_message(&f.head()), f.raw_message(&target));
    assert_eq!(f.read("independent.txt"), b"keep unrelated work\n");
    assert!(f.repo().operation_state().unwrap().is_none());
}

fn conflicted_recovery(kind: RecoveryKind) -> (Fixture, String) {
    let f = Fixture::new();
    f.base();
    let target = if kind == RecoveryKind::CherryPick {
        f.git(&["switch", "-c", "source"]);
        f.write("file.txt", "incoming\n");
        let target = f.commit("source change");
        f.git(&["switch", "main"]);
        target
    } else {
        f.write("file.txt", "target change\n");
        f.commit("change to revert")
    };
    f.write("file.txt", "later conflicting work\n");
    f.commit("current change");
    f.write(
        "independent.txt",
        "independent edits before the operation\n",
    );
    f.write("untracked.txt", "unrelated preserved\n");
    let plan = f.repo().recovery_plan(kind, Some(&target), None).unwrap();
    let command = match kind {
        RecoveryKind::CherryPick => RecoveryCommand::CherryPick { plan },
        RecoveryKind::Revert => RecoveryCommand::Revert { plan },
        _ => unreachable!(),
    };
    assert!(f.recover(command).is_err());
    let operation = f.repo().operation_state().unwrap().unwrap();
    assert_eq!(
        operation.kind,
        if kind == RecoveryKind::CherryPick {
            OperationKind::CherryPick
        } else {
            OperationKind::Revert
        }
    );
    assert!(
        f.repo()
            .status()
            .unwrap()
            .entries
            .iter()
            .any(|entry| entry.conflicted)
    );
    (f, target)
}

#[test]
fn cherry_pick_and_revert_conflicts_resolve_and_continue_through_existing_workflow() {
    for kind in [RecoveryKind::CherryPick, RecoveryKind::Revert] {
        let (f, _) = conflicted_recovery(kind);
        let head = f.head();
        let repo = f.repo();
        let preview = repo.conflict_preview(Path::new("file.txt")).unwrap();
        assert!(preview.base.is_some() && preview.current.is_some() && preview.incoming.is_some());
        repo.execute_integration(&IntegrationCommand::Resolve {
            expected: Arc::new(preview),
            resolution: ConflictResolution::Manual {
                bytes: b"reviewed resolution\n".to_vec(),
            },
        })
        .unwrap();
        let expected = repo.operation_state().unwrap().unwrap();
        repo.execute_integration(&IntegrationCommand::Continue { expected })
            .unwrap();
        assert!(repo.operation_state().unwrap().is_none());
        assert_ne!(f.head(), head);
        assert_eq!(f.read("file.txt"), b"reviewed resolution\n");
        assert_eq!(f.staged("file.txt"), b"reviewed resolution\n");
        assert_eq!(f.read("untracked.txt"), b"unrelated preserved\n");
    }
}

#[test]
fn cherry_pick_and_revert_abort_restore_preoperation_files_and_preserve_untracked_work() {
    for kind in [RecoveryKind::CherryPick, RecoveryKind::Revert] {
        for edit_after in [false, true] {
            let (f, _) = conflicted_recovery(kind);
            let independent: &[u8] = if edit_after {
                f.write("independent.txt", "independent edits after the operation\n");
                f.write("after.txt", "untracked after the operation\n");
                b"independent edits after the operation\n"
            } else {
                b"independent edits before the operation\n"
            };
            let head = f.head();
            let repo = f.repo();
            let expected = repo.operation_state().unwrap().unwrap();
            repo.execute_integration(&IntegrationCommand::Abort { expected })
                .unwrap();
            assert_eq!(f.head(), head);
            assert_eq!(f.read("file.txt"), b"later conflicting work\n");
            assert_eq!(f.staged("file.txt"), b"later conflicting work\n");
            assert_eq!(f.read("untracked.txt"), b"unrelated preserved\n");
            assert_eq!(f.read("independent.txt"), independent);
            assert_eq!(f.staged("independent.txt"), b"independent base\n");
            if edit_after {
                assert_eq!(f.read("after.txt"), b"untracked after the operation\n");
            }
            assert!(repo.operation_state().unwrap().is_none());
        }
    }
}

#[test]
fn cherry_pick_and_revert_abort_still_protect_independently_staged_work() {
    for kind in [RecoveryKind::CherryPick, RecoveryKind::Revert] {
        let (f, _) = conflicted_recovery(kind);
        let repo = f.repo();
        f.git(&["add", "independent.txt"]);
        let expected = repo.operation_state().unwrap().unwrap();
        let index = f.index();
        let head = f.head();
        assert!(
            repo.execute_integration(&IntegrationCommand::Abort {
                expected: expected.clone()
            })
            .is_err()
        );
        assert_eq!(f.head(), head);
        assert_eq!(f.index(), index);
        assert_eq!(
            f.staged("independent.txt"),
            b"independent edits before the operation\n"
        );
        repo.execute_integration(&IntegrationCommand::Quit { expected })
            .unwrap();
        assert!(repo.operation_state().unwrap().is_none());
        assert_eq!(f.index(), index);
        assert_eq!(
            f.read("independent.txt"),
            b"independent edits before the operation\n"
        );
    }
}

#[cfg(unix)]
#[test]
fn replay_refuses_ignored_file_and_symlink_ancestors_before_any_mutation() {
    use std::os::unix::fs::symlink;
    for linked in [false, true] {
        let f = Fixture::new();
        f.base();
        f.git(&["switch", "-c", "source"]);
        f.write("folder/child.txt", "incoming file\n");
        let target = f.commit("introduce nested path");
        f.git(&["switch", "main"]);
        f.write(".git/info/exclude", "folder\n");
        let outside = f._temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"outside content\n").unwrap();
        if linked {
            symlink(&outside, f.root.join("folder")).unwrap();
        } else {
            f.write("folder", "ignored blocking file\n");
        }
        let head = f.head();
        let index = f.index();
        assert!(
            f.repo()
                .recovery_plan(RecoveryKind::CherryPick, Some(&target), None)
                .is_err()
        );
        assert_eq!(f.head(), head);
        assert_eq!(f.index(), index);
        if linked {
            assert_eq!(fs::read_link(f.root.join("folder")).unwrap(), outside);
        } else {
            assert_eq!(f.read("folder"), b"ignored blocking file\n");
        }
        assert_eq!(
            fs::read(outside.join("sentinel")).unwrap(),
            b"outside content\n"
        );
        assert!(!outside.join("child.txt").exists());
    }
}

#[test]
fn merge_recovery_requires_an_explicit_valid_mainline_parent() {
    let f = Fixture::new();
    f.base();
    f.git(&["switch", "-c", "source"]);
    f.write("source.txt", "source\n");
    f.commit("source");
    f.git(&["switch", "main"]);
    f.write("main.txt", "main\n");
    f.commit("main");
    f.git(&["merge", "--no-ff", "--no-edit", "source"]);
    let merge = f.head();
    for kind in [RecoveryKind::Revert, RecoveryKind::CherryPick] {
        assert!(f.repo().recovery_plan(kind, Some(&merge), None).is_err());
        assert!(f.repo().recovery_plan(kind, Some(&merge), Some(0)).is_err());
        assert!(f.repo().recovery_plan(kind, Some(&merge), Some(3)).is_err());
    }
    let plan = f
        .repo()
        .recovery_plan(RecoveryKind::Revert, Some(&merge), Some(1))
        .unwrap();
    f.recover(RecoveryCommand::Revert { plan }).unwrap();
    assert!(!f.root.join("source.txt").exists());
    assert_eq!(f.read("main.txt"), b"main\n");
}

#[test]
fn linked_worktree_recovery_changes_only_its_own_head_and_index() {
    let f = Fixture::new();
    let main = f.base();
    let linked = f._temp.path().join("linked");
    f.git(&["worktree", "add", "-b", "linked", linked.to_str().unwrap()]);
    let repo = GitRepository::open(&linked).unwrap();
    fs::write(linked.join("file.txt"), b"linked committed\n").unwrap();
    for args in [
        &["add", "file.txt"][..],
        &["-c", "commit.gpgsign=false", "commit", "-m", "linked work"][..],
    ] {
        assert!(
            Fixture::command_at(&linked)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let main_index = f.index();
    let (private_git, _) = repo.git_directories().unwrap();
    let private_index = fs::read(private_git.join("index")).unwrap();
    let plan = repo.recovery_plan(RecoveryKind::Undo, None, None).unwrap();
    assert!(
        f.recover(RecoveryCommand::Undo { plan: plan.clone() })
            .is_err()
    );
    repo.execute(&WriteCommand::Recovery(
        RecoveryCommand::Undo { plan }.into(),
    ))
    .unwrap();
    assert_eq!(f.head(), main);
    assert_eq!(f.index(), main_index);
    assert_eq!(fs::read(private_git.join("index")).unwrap(), private_index);
    assert_eq!(
        fs::read(linked.join("file.txt")).unwrap(),
        b"linked committed\n"
    );
    assert_eq!(f.read("file.txt"), b"base\n");
}
