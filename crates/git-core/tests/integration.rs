use gitturtle_core::{
    ConflictResolution, GitRepository, IntegrationCommand, OperationKind, WriteCommand,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
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
        let fixture = Self { _temp: temp, root };
        for (key, value) in [
            ("user.name", "Fixture Author"),
            ("user.email", "fixture@example.invalid"),
            ("commit.gpgsign", "false"),
            ("core.hooksPath", ".git/hooks"),
            ("rerere.enabled", "false"),
        ] {
            fixture.git(&["config", key, value]);
        }
        fixture
    }

    fn repo(&self) -> GitRepository {
        GitRepository::open(&self.root).unwrap()
    }

    fn output(&self, args: &[&str]) -> Output {
        Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_EDITOR", "true")
            .output()
            .unwrap()
    }

    fn git(&self, args: &[&str]) -> String {
        let output = self.output(args);
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim_end().into()
    }

    fn write(&self, path: &str, bytes: impl AsRef<[u8]>) {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn read(&self, path: &str) -> Vec<u8> {
        fs::read(self.root.join(path)).unwrap()
    }

    fn commit(&self, message: &str) -> String {
        self.git(&["add", "--all"]);
        self.git(&["commit", "--message", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn divergent(&self, path: &str, base: &[u8], current: &[u8], incoming: &[u8]) {
        self.write(path, base);
        self.write("unrelated.txt", "baseline\n");
        self.commit("base");
        self.git(&["switch", "--create", "feature/beautiful-integration"]);
        self.write(path, incoming);
        self.commit("incoming change");
        self.git(&["switch", "main"]);
        self.write(path, current);
        self.commit("current change");
    }

    fn merge_conflict(&self) {
        assert!(
            !self
                .output(&["merge", "--no-edit", "feature/beautiful-integration"])
                .status
                .success()
        );
        assert!(
            self.repo()
                .status()
                .unwrap()
                .entries
                .iter()
                .any(|entry| entry.conflicted)
        );
    }
}

#[test]
fn merge_plan_reports_divergence_and_manual_resolution_preserves_unrelated_work() {
    let f = Fixture::new();
    f.divergent("file.txt", b"base\n", b"current\n", b"incoming\n");
    let repo = f.repo();
    let before_head = f.git(&["rev-parse", "HEAD"]);
    let plan = repo
        .integration_plan("refs/heads/feature/beautiful-integration")
        .unwrap();
    assert_eq!((plan.ahead, plan.behind), (1, 1));
    assert_eq!(plan.affected_paths, vec![PathBuf::from("file.txt")]);
    assert_eq!(plan.target_label, "feature/beautiful-integration");
    f.write("unrelated.txt", "keep my unstaged work\n");
    let error = repo
        .execute_integration(&IntegrationCommand::Merge { plan })
        .unwrap_err();
    assert!(error.to_string().contains("conflict") || error.to_string().contains("CONFLICT"));
    let preview = repo.conflict_preview(Path::new("file.txt")).unwrap();
    assert_eq!(preview.base.as_ref().unwrap().content.bytes, b"base\n");
    assert_eq!(
        preview.current.as_ref().unwrap().content.bytes,
        b"current\n"
    );
    assert_eq!(
        preview.incoming.as_ref().unwrap().content.bytes,
        b"incoming\n"
    );
    assert!(preview.current.as_ref().unwrap().label.contains("main"));
    assert!(
        preview
            .incoming
            .as_ref()
            .unwrap()
            .label
            .contains("feature/beautiful-integration")
    );
    assert_eq!(
        repo.conflict_editor_path(&preview).unwrap(),
        f.root.join("file.txt").canonicalize().unwrap()
    );
    let expected = repo.operation_state().unwrap().unwrap();
    assert_eq!(expected.kind, OperationKind::Merge);
    assert!(
        repo.execute_integration(&IntegrationCommand::Continue {
            expected: expected.clone()
        })
        .unwrap_err()
        .to_string()
        .contains("Resolve and stage")
    );
    repo.execute_integration(&IntegrationCommand::Resolve {
        expected: Arc::new(preview),
        resolution: ConflictResolution::Manual {
            bytes: b"both versions\n\nprecise formatting  \n".to_vec(),
        },
    })
    .unwrap();
    let expected = repo.operation_state().unwrap().unwrap();
    repo.execute_integration(&IntegrationCommand::Continue { expected })
        .unwrap();
    assert!(repo.operation_state().unwrap().is_none());
    assert_eq!(
        f.git(&["log", "-1", "--format=%s"]),
        "Merge branch 'feature/beautiful-integration' into main"
    );
    assert_ne!(f.git(&["rev-parse", "HEAD"]), before_head);
    assert_eq!(
        f.git(&["rev-list", "--parents", "-1", "HEAD"])
            .split_whitespace()
            .count(),
        3
    );
    assert_eq!(
        f.read("file.txt"),
        b"both versions\n\nprecise formatting  \n"
    );
    assert_eq!(f.read("unrelated.txt"), b"keep my unstaged work\n");
    assert_eq!(f.git(&["show", "HEAD:unrelated.txt"]), "baseline");
}

#[test]
fn stale_plan_rejects_a_moved_target_and_current_head_without_mutation() {
    let f = Fixture::new();
    f.divergent("file.txt", b"base\n", b"current\n", b"incoming\n");
    let repo = f.repo();
    let plan = repo
        .integration_plan("refs/heads/feature/beautiful-integration")
        .unwrap();
    let head = f.git(&["rev-parse", "HEAD"]);
    let index = f.read(".git/index");
    f.git(&[
        "update-ref",
        "refs/heads/feature/beautiful-integration",
        &head,
    ]);
    assert!(
        repo.execute_integration(&IntegrationCommand::Merge { plan })
            .unwrap_err()
            .to_string()
            .contains("destination branch changed")
    );
    assert_eq!(head, f.git(&["rev-parse", "HEAD"]));
    assert_eq!(index, f.read(".git/index"));
    let plan = repo
        .integration_plan("refs/heads/feature/beautiful-integration")
        .unwrap();
    f.write("new.txt", "new commit\n");
    f.commit("moved current branch");
    assert!(
        repo.execute_integration(&IntegrationCommand::Rebase { plan })
            .unwrap_err()
            .to_string()
            .contains("current branch changed")
    );
    assert!(repo.operation_state().unwrap().is_none());
}

#[test]
fn stale_conflict_and_editor_changes_are_rejected_before_writing_or_staging() {
    let f = Fixture::new();
    f.divergent("file.txt", b"base\n", b"current\n", b"incoming\n");
    f.merge_conflict();
    let repo = f.repo();
    let preview = repo.conflict_preview(Path::new("file.txt")).unwrap();
    let index = f.read(".git/index");
    f.write("file.txt", "external editor already saved this\n");
    for resolution in [
        ConflictResolution::Incoming,
        ConflictResolution::MarkResolved,
        ConflictResolution::Manual {
            bytes: b"stale draft".to_vec(),
        },
    ] {
        assert!(
            repo.execute_integration(&IntegrationCommand::Resolve {
                expected: Arc::new(preview.clone()),
                resolution
            })
            .unwrap_err()
            .to_string()
            .contains("changed")
        );
    }
    assert_eq!(index, f.read(".git/index"));
    assert_eq!(f.read("file.txt"), b"external editor already saved this\n");
    let current = repo.conflict_preview(Path::new("file.txt")).unwrap();
    repo.execute_integration(&IntegrationCommand::Resolve {
        expected: Arc::new(current),
        resolution: ConflictResolution::MarkResolved,
    })
    .unwrap();
    assert_eq!(
        f.git(&["show", ":file.txt"]),
        "external editor already saved this"
    );
    assert!(
        !repo
            .status()
            .unwrap()
            .entries
            .iter()
            .any(|entry| entry.conflicted)
    );
}

#[test]
fn merge_abort_preserves_unrelated_work_and_stale_continue_is_refused() {
    let f = Fixture::new();
    f.divergent("file.txt", b"base\n", b"current\n", b"incoming\n");
    let head = f.git(&["rev-parse", "HEAD"]);
    f.write("unrelated.txt", "unrelated changes\n");
    f.merge_conflict();
    let repo = f.repo();
    let expected = repo.operation_state().unwrap().unwrap();
    repo.execute_integration(&IntegrationCommand::Abort {
        expected: expected.clone(),
    })
    .unwrap();
    assert_eq!(head, f.git(&["rev-parse", "HEAD"]));
    assert_eq!(f.read("file.txt"), b"current\n");
    assert_eq!(f.read("unrelated.txt"), b"unrelated changes\n");
    assert!(
        repo.execute_integration(&IntegrationCommand::Continue { expected })
            .unwrap_err()
            .to_string()
            .contains("operation changed")
    );
}

#[test]
fn binary_conflicts_offer_both_complete_versions() {
    for (resolution, expected_bytes) in [
        (ConflictResolution::Current, b"current\0image".as_slice()),
        (ConflictResolution::Incoming, b"incoming\0image".as_slice()),
    ] {
        let f = Fixture::new();
        f.divergent(
            "-image[1]\n.bin",
            b"base\0image",
            b"current\0image",
            b"incoming\0image",
        );
        f.merge_conflict();
        let repo = f.repo();
        let preview = repo.conflict_preview(Path::new("-image[1]\n.bin")).unwrap();
        repo.execute_integration(&IntegrationCommand::Resolve {
            expected: Arc::new(preview),
            resolution,
        })
        .unwrap();
        assert_eq!(f.read("-image[1]\n.bin"), expected_bytes);
        assert!(
            !repo
                .status()
                .unwrap()
                .entries
                .iter()
                .any(|entry| entry.conflicted)
        );
        let staged = f.output(&["show", ":-image[1]\n.bin"]);
        assert!(staged.status.success());
        assert_eq!(staged.stdout, expected_bytes);
    }
}

#[test]
fn missing_incoming_side_resolves_as_explicit_deletion() {
    let f = Fixture::new();
    f.write("file.txt", "base\n");
    f.commit("base");
    f.git(&["switch", "--create", "feature/beautiful-integration"]);
    f.git(&["rm", "file.txt"]);
    f.commit("incoming deletion");
    f.git(&["switch", "main"]);
    f.write("file.txt", "current edit\n");
    f.commit("current edit");
    f.merge_conflict();
    let repo = f.repo();
    let preview = repo.conflict_preview(Path::new("file.txt")).unwrap();
    assert!(preview.base.is_some());
    assert!(preview.current.is_some());
    assert!(preview.incoming.is_none());
    repo.execute_integration(&IntegrationCommand::Resolve {
        expected: Arc::new(preview),
        resolution: ConflictResolution::Incoming,
    })
    .unwrap();
    assert!(!f.root.join("file.txt").exists());
    assert!(f.git(&["ls-files", "--unmerged"]).is_empty());
    assert!(f.git(&["ls-files"]).is_empty());
}

#[test]
fn rebase_uses_meaningful_reversed_labels_and_continues_the_replayed_commit() {
    let f = Fixture::new();
    f.divergent("file.txt", b"base\n", b"current\n", b"incoming\n");
    f.git(&["branch", "preserve-this-branch"]);
    f.git(&["config", "rebase.updateRefs", "true"]);
    let original_head = f.git(&["rev-parse", "HEAD"]);
    let repo = f.repo();
    let plan = repo
        .integration_plan("refs/heads/feature/beautiful-integration")
        .unwrap();
    let target = plan.target_oid.clone();
    assert!(
        repo.execute_integration(&IntegrationCommand::Rebase { plan })
            .is_err()
    );
    let expected = repo.operation_state().unwrap().unwrap();
    assert_eq!(expected.kind, OperationKind::Rebase);
    assert_eq!(expected.branch, "main");
    let preview = repo.conflict_preview(Path::new("file.txt")).unwrap();
    assert!(
        preview
            .current
            .as_ref()
            .unwrap()
            .label
            .contains("feature/beautiful-integration")
    );
    assert!(
        preview
            .incoming
            .as_ref()
            .unwrap()
            .label
            .contains("main · replayed commit")
    );
    assert_eq!(
        preview.current.as_ref().unwrap().content.bytes,
        b"incoming\n"
    );
    assert_eq!(
        preview.incoming.as_ref().unwrap().content.bytes,
        b"current\n"
    );
    repo.execute_integration(&IntegrationCommand::Resolve {
        expected: Arc::new(preview),
        resolution: ConflictResolution::Manual {
            bytes: b"reconciled\n".to_vec(),
        },
    })
    .unwrap();
    let expected = repo.operation_state().unwrap().unwrap();
    repo.execute_integration(&IntegrationCommand::Continue { expected })
        .unwrap();
    assert!(repo.operation_state().unwrap().is_none());
    assert_eq!(f.git(&["branch", "--show-current"]), "main");
    assert_eq!(f.git(&["rev-parse", "HEAD^"]), target);
    assert_ne!(f.git(&["rev-parse", "HEAD"]), original_head);
    assert_eq!(f.git(&["rev-parse", "preserve-this-branch"]), original_head);
    assert_eq!(f.read("file.txt"), b"reconciled\n");
}

#[test]
fn rebase_abort_restores_original_branch_and_dirty_abort_can_keep_files() {
    for extra_edits in [false, true] {
        let f = Fixture::new();
        f.divergent("file.txt", b"base\n", b"current\n", b"incoming\n");
        let original_head = f.git(&["rev-parse", "HEAD"]);
        assert!(
            !f.output(&["rebase", "feature/beautiful-integration"])
                .status
                .success()
        );
        let repo = f.repo();
        let expected = repo.operation_state().unwrap().unwrap();
        if extra_edits {
            f.write("unrelated.txt", "new work after rebase stopped\n");
            let index = f.read(".git/index");
            let conflict = f.read("file.txt");
            assert!(
                repo.execute_integration(&IntegrationCommand::Abort {
                    expected: expected.clone()
                })
                .unwrap_err()
                .to_string()
                .contains("other changed files")
            );
            assert_eq!(index, f.read(".git/index"));
            assert_eq!(conflict, f.read("file.txt"));
            repo.execute_integration(&IntegrationCommand::Quit { expected })
                .unwrap();
            assert_eq!(index, f.read(".git/index"));
            assert_eq!(conflict, f.read("file.txt"));
            assert_eq!(f.read("unrelated.txt"), b"new work after rebase stopped\n");
        } else {
            repo.execute_integration(&IntegrationCommand::Abort { expected })
                .unwrap();
            assert_eq!(f.git(&["rev-parse", "HEAD"]), original_head);
            assert_eq!(f.git(&["branch", "--show-current"]), "main");
            assert_eq!(f.read("file.txt"), b"current\n");
        }
        assert!(repo.operation_state().unwrap().is_none());
    }
}

#[test]
fn rebase_never_autostashes_dirty_work_even_when_git_config_requests_it() {
    let f = Fixture::new();
    f.divergent("file.txt", b"base\n", b"current\n", b"incoming\n");
    f.git(&["config", "rebase.autoStash", "true"]);
    let repo = f.repo();
    let plan = repo
        .integration_plan("refs/heads/feature/beautiful-integration")
        .unwrap();
    f.write("unrelated.txt", "my edits\n");
    let head = f.git(&["rev-parse", "HEAD"]);
    assert!(
        repo.execute_integration(&IntegrationCommand::Rebase { plan })
            .unwrap_err()
            .to_string()
            .contains("no automatic stash")
    );
    assert!(f.git(&["stash", "list"]).is_empty());
    assert_eq!(head, f.git(&["rev-parse", "HEAD"]));
    assert_eq!(f.read("unrelated.txt"), b"my edits\n");
}

#[test]
fn external_cherry_pick_and_revert_are_detected_and_can_continue_or_abort() {
    for kind in [OperationKind::CherryPick, OperationKind::Revert] {
        let f = Fixture::new();
        f.divergent("file.txt", b"base\n", b"current\n", b"incoming\n");
        let head = f.git(&["rev-parse", "HEAD"]);
        let args = if kind == OperationKind::CherryPick {
            vec!["cherry-pick", "feature/beautiful-integration"]
        } else {
            vec!["revert", "--no-edit", "feature/beautiful-integration"]
        };
        assert!(!f.output(&args).status.success());
        let repo = f.repo();
        let expected = repo.operation_state().unwrap().unwrap();
        assert_eq!(expected.kind, kind);
        let preview = repo.conflict_preview(Path::new("file.txt")).unwrap();
        if kind == OperationKind::CherryPick {
            repo.execute_integration(&IntegrationCommand::Resolve {
                expected: Arc::new(preview),
                resolution: ConflictResolution::Incoming,
            })
            .unwrap();
            let expected = repo.operation_state().unwrap().unwrap();
            repo.execute_integration(&IntegrationCommand::Continue { expected })
                .unwrap();
            assert_eq!(f.read("file.txt"), b"incoming\n");
            assert_ne!(f.git(&["rev-parse", "HEAD"]), head);
        } else {
            assert!(
                preview
                    .incoming
                    .as_ref()
                    .unwrap()
                    .label
                    .contains("Before reverted commit")
            );
            repo.execute_integration(&IntegrationCommand::Abort { expected })
                .unwrap();
            assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
            assert_eq!(f.read("file.txt"), b"current\n");
        }
        assert!(repo.operation_state().unwrap().is_none());
    }
}

#[cfg(unix)]
fn executable(path: &Path, source: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, source).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn conflict_reads_do_not_mutate_the_index_or_execute_configured_helpers() {
    let f = Fixture::new();
    f.divergent("file.txt", b"base\n", b"current\n", b"incoming\n");
    f.merge_conflict();
    executable(&f.root.join("helper"), "#!/bin/sh\ntouch helper-ran\ncat\n");
    f.write(".gitattributes", "*.txt filter=hostile diff=hostile\n");
    for (key, value) in [
        ("filter.hostile.clean", "./helper"),
        ("filter.hostile.smudge", "./helper"),
        ("diff.hostile.command", "./helper"),
        ("diff.hostile.textconv", "./helper"),
        ("core.fsmonitor", "./helper"),
    ] {
        f.git(&["config", key, value]);
    }
    let index = f.read(".git/index");
    let working = f.read("file.txt");
    let repo = f.repo();
    repo.status().unwrap();
    repo.operation_state().unwrap();
    repo.conflict_preview(Path::new("file.txt")).unwrap();
    assert_eq!(index, f.read(".git/index"));
    assert_eq!(working, f.read("file.txt"));
    assert!(!f.root.join("helper-ran").exists());
}

#[cfg(unix)]
#[test]
fn configured_commit_hooks_are_respected_when_continuing_a_merge() {
    let f = Fixture::new();
    f.divergent("file.txt", b"base\n", b"current\n", b"incoming\n");
    f.merge_conflict();
    let repo = f.repo();
    let preview = repo.conflict_preview(Path::new("file.txt")).unwrap();
    repo.execute_integration(&IntegrationCommand::Resolve {
        expected: Arc::new(preview),
        resolution: ConflictResolution::Incoming,
    })
    .unwrap();
    executable(
        &f.root.join(".git/hooks/pre-commit"),
        "#!/bin/sh\necho fixture-hook-refusal >&2\nexit 1\n",
    );
    let head = f.git(&["rev-parse", "HEAD"]);
    let expected = repo.operation_state().unwrap().unwrap();
    assert!(
        repo.execute_integration(&IntegrationCommand::Continue { expected })
            .unwrap_err()
            .to_string()
            .contains("fixture-hook-refusal")
    );
    assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
    assert!(repo.operation_state().unwrap().is_some());
    assert_eq!(f.git(&["show", ":file.txt"]), "incoming");
}

#[cfg(unix)]
#[test]
fn manual_resolution_preserves_executable_mode_and_uses_the_configured_clean_filter() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    f.divergent("file.txt", b"base\n", b"current\n", b"incoming\n");
    fs::set_permissions(f.root.join("file.txt"), fs::Permissions::from_mode(0o755)).unwrap();
    f.commit("executable file");
    f.merge_conflict();
    f.write(".git/info/attributes", "file.txt filter=uppercase\n");
    f.git(&[
        "config",
        "filter.uppercase.clean",
        "tr '[:lower:]' '[:upper:]'",
    ]);
    let repo = f.repo();
    let preview = repo.conflict_preview(Path::new("file.txt")).unwrap();
    assert_eq!(preview.working.as_ref().unwrap().mode, "100755");
    repo.execute_integration(&IntegrationCommand::Resolve {
        expected: Arc::new(preview),
        resolution: ConflictResolution::Manual {
            bytes: b"exact raw text\n".to_vec(),
        },
    })
    .unwrap();
    assert_eq!(f.read("file.txt"), b"exact raw text\n");
    assert_eq!(f.git(&["show", ":file.txt"]), "EXACT RAW TEXT");
    assert!(
        f.git(&["ls-files", "--stage", "--", "file.txt"])
            .starts_with("100755 ")
    );
    assert_ne!(
        fs::metadata(f.root.join("file.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
}

#[cfg(unix)]
#[test]
fn signing_failure_preserves_the_resolved_index_and_pending_merge() {
    let f = Fixture::new();
    f.divergent("file.txt", b"base\n", b"current\n", b"incoming\n");
    f.merge_conflict();
    let repo = f.repo();
    let preview = repo.conflict_preview(Path::new("file.txt")).unwrap();
    repo.execute_integration(&IntegrationCommand::Resolve {
        expected: Arc::new(preview),
        resolution: ConflictResolution::Incoming,
    })
    .unwrap();
    executable(
        &f.root.join("signer"),
        "#!/bin/sh\necho fixture-signing-refusal >&2\nexit 1\n",
    );
    f.git(&["config", "commit.gpgsign", "true"]);
    f.git(&["config", "gpg.format", "openpgp"]);
    f.git(&[
        "config",
        "gpg.program",
        f.root.join("signer").to_str().unwrap(),
    ]);
    let head = f.git(&["rev-parse", "HEAD"]);
    let expected = repo.operation_state().unwrap().unwrap();
    assert!(
        repo.execute_integration(&IntegrationCommand::Continue { expected })
            .unwrap_err()
            .to_string()
            .contains("fixture-signing-refusal")
    );
    assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(f.git(&["show", ":file.txt"]), "incoming");
    assert!(repo.operation_state().unwrap().is_some());
}

#[cfg(unix)]
#[test]
fn manual_resolution_never_follows_a_symlink_into_another_directory() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    f.divergent("nested/file.txt", b"base\n", b"current\n", b"incoming\n");
    f.merge_conflict();
    let repo = f.repo();
    let preview = repo.conflict_preview(Path::new("nested/file.txt")).unwrap();
    let outside = f._temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("file.txt"), "private\n").unwrap();
    fs::rename(f.root.join("nested"), f.root.join("saved-nested")).unwrap();
    symlink(&outside, f.root.join("nested")).unwrap();
    assert!(
        repo.execute_integration(&IntegrationCommand::Resolve {
            expected: Arc::new(preview),
            resolution: ConflictResolution::Manual {
                bytes: b"resolution\n".to_vec()
            }
        })
        .is_err()
    );
    assert_eq!(fs::read(outside.join("file.txt")).unwrap(), b"private\n");
}

#[test]
fn linked_worktree_operations_do_not_leak_to_the_main_worktree() {
    let f = Fixture::new();
    f.divergent("file.txt", b"base\n", b"current\n", b"incoming\n");
    let linked_path = f._temp.path().join("linked");
    f.git(&[
        "worktree",
        "add",
        "-b",
        "linked-branch",
        linked_path.to_str().unwrap(),
        "main",
    ]);
    let linked = GitRepository::open(&linked_path).unwrap();
    let plan = linked
        .integration_plan("refs/heads/feature/beautiful-integration")
        .unwrap();
    assert!(
        linked
            .execute_integration(&IntegrationCommand::Merge { plan })
            .is_err()
    );
    assert!(f.repo().operation_state().unwrap().is_none());
    assert_eq!(
        linked.operation_state().unwrap().unwrap().branch,
        "linked-branch"
    );
    assert_eq!(
        linked
            .conflict_preview(Path::new("file.txt"))
            .unwrap()
            .current
            .unwrap()
            .content
            .bytes,
        b"current\n"
    );
    assert_eq!(f.read("file.txt"), b"current\n");
    assert!(f.repo().status().unwrap().entries.is_empty());
}

#[test]
fn fast_forward_merge_advances_only_the_named_current_branch() {
    let f = Fixture::new();
    f.write("base.txt", "base\n");
    let base = f.commit("base");
    f.git(&["switch", "--create", "feature"]);
    f.write("feature.txt", "feature\n");
    let target = f.commit("feature");
    f.git(&["switch", "main"]);
    f.write("untracked.txt", "preserve\n");
    let repo = f.repo();
    let plan = repo.integration_plan("refs/heads/feature").unwrap();
    assert_eq!((plan.ahead, plan.behind), (0, 1));
    repo.execute(&WriteCommand::Integration(IntegrationCommand::Merge {
        plan,
    }))
    .unwrap();
    assert_ne!(base, target);
    assert_eq!(f.git(&["rev-parse", "HEAD"]), target);
    assert_eq!(f.git(&["branch", "--show-current"]), "main");
    assert_eq!(f.read("untracked.txt"), b"preserve\n");
    assert!(repo.operation_state().unwrap().is_none());
}

#[test]
fn visible_integration_names_resolve_to_full_refs_and_refuse_ambiguity() {
    let f = Fixture::new();
    f.divergent("file.txt", b"base\n", b"current\n", b"incoming\n");
    let target = f.git(&["rev-parse", "feature/beautiful-integration"]);
    f.git(&["update-ref", "refs/remotes/origin/topic", &target]);
    let repo = f.repo();
    let local = repo
        .integration_plan("feature/beautiful-integration")
        .unwrap();
    assert_eq!(local.target_ref, "refs/heads/feature/beautiful-integration");
    assert_eq!(local.target_oid, target);
    let remote = repo.integration_plan("origin/topic").unwrap();
    assert_eq!(remote.target_ref, "refs/remotes/origin/topic");
    assert_eq!(remote.target_label, "origin/topic");
    f.git(&["branch", "origin/topic", "main"]);
    assert!(
        repo.integration_plan("origin/topic")
            .unwrap_err()
            .to_string()
            .contains("both a local and remote branch")
    );
    assert_eq!(
        repo.integration_plan("refs/remotes/origin/topic")
            .unwrap()
            .target_oid,
        target
    );
    for invalid in ["missing", "--all", "HEAD", "", "../main"] {
        assert!(repo.integration_plan(invalid).is_err());
    }
    assert!(repo.operation_state().unwrap().is_none());
}

#[test]
fn upstream_integration_uses_the_configured_full_local_or_remote_ref() {
    let f = Fixture::new();
    f.divergent("file.txt", b"base\n", b"current\n", b"incoming\n");
    let target = f.git(&["rev-parse", "feature/beautiful-integration"]);
    let repo = f.repo();
    assert!(repo.upstream_integration_plan().is_err());
    f.git(&["config", "branch.main.remote", "."]);
    f.git(&[
        "config",
        "branch.main.merge",
        "refs/heads/feature/beautiful-integration",
    ]);
    let local = repo.integration_plan("@{upstream}").unwrap();
    assert_eq!(local.target_ref, "refs/heads/feature/beautiful-integration");
    assert_eq!((local.ahead, local.behind), (1, 1));
    f.git(&["remote", "add", "origin", "../local-test-remote"]);
    f.git(&["update-ref", "refs/remotes/origin/topic", &target]);
    f.git(&["config", "branch.main.remote", "origin"]);
    f.git(&["config", "branch.main.merge", "refs/heads/topic"]);
    let remote = repo.upstream_integration_plan().unwrap();
    assert_eq!(remote.target_ref, "refs/remotes/origin/topic");
    assert_eq!(remote.target_oid, target);
    assert_eq!((remote.ahead, remote.behind), (1, 1));
    assert!(repo.operation_state().unwrap().is_none());
}

#[test]
fn merge_abort_refuses_independent_staged_work_but_accepts_resolution_paths() {
    for independent_staged in [false, true] {
        let f = Fixture::new();
        f.divergent("file.txt", b"base\n", b"current\n", b"incoming\n");
        f.merge_conflict();
        let repo = f.repo();
        let preview = repo.conflict_preview(Path::new("file.txt")).unwrap();
        repo.execute_integration(&IntegrationCommand::Resolve {
            expected: Arc::new(preview),
            resolution: ConflictResolution::Incoming,
        })
        .unwrap();
        if independent_staged {
            f.write("unrelated.txt", "independent staged work\n");
            f.git(&["add", "unrelated.txt"]);
        }
        let expected = repo.operation_state().unwrap().unwrap();
        let index = f.read(".git/index");
        if independent_staged {
            assert!(
                repo.execute_integration(&IntegrationCommand::Abort {
                    expected: expected.clone()
                })
                .unwrap_err()
                .to_string()
                .contains("independent staged work")
            );
            assert_eq!(f.read("unrelated.txt"), b"independent staged work\n");
            assert_eq!(
                f.git(&["show", ":unrelated.txt"]),
                "independent staged work"
            );
            assert_eq!(f.read(".git/index"), index);
            repo.execute_integration(&IntegrationCommand::Quit { expected })
                .unwrap();
            assert_eq!(f.read(".git/index"), index);
            assert_eq!(f.read("unrelated.txt"), b"independent staged work\n");
        } else {
            repo.execute_integration(&IntegrationCommand::Abort { expected })
                .unwrap();
            assert_eq!(f.read("file.txt"), b"current\n");
            assert!(repo.status().unwrap().entries.is_empty());
        }
        assert!(repo.operation_state().unwrap().is_none());
    }
}

#[test]
fn continuation_snapshot_lists_staged_paths_and_rejects_new_staging() {
    let f = Fixture::new();
    f.divergent("file.txt", b"base\n", b"current\n", b"incoming\n");
    f.merge_conflict();
    let repo = f.repo();
    let initial = repo.operation_state().unwrap().unwrap();
    assert!(initial.staged_paths.is_empty());
    let preview = repo.conflict_preview(Path::new("file.txt")).unwrap();
    repo.execute_integration(&IntegrationCommand::Resolve {
        expected: Arc::new(preview),
        resolution: ConflictResolution::Incoming,
    })
    .unwrap();
    let prepared = repo.operation_state().unwrap().unwrap();
    assert_eq!(prepared.staged_paths, vec![PathBuf::from("file.txt")]);
    assert!(initial.same_operation(&prepared));
    assert_ne!(initial, prepared);
    f.write("unrelated.txt", "independent staged work\n");
    f.git(&["add", "unrelated.txt"]);
    let head = f.git(&["rev-parse", "HEAD"]);
    let index = f.read(".git/index");
    assert!(
        repo.execute_integration(&IntegrationCommand::Continue { expected: prepared })
            .unwrap_err()
            .to_string()
            .contains("operation changed")
    );
    assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(f.read(".git/index"), index);
    assert_eq!(f.read("unrelated.txt"), b"independent staged work\n");
    let current = repo.operation_state().unwrap().unwrap();
    assert_eq!(
        current.staged_paths,
        vec![PathBuf::from("file.txt"), PathBuf::from("unrelated.txt")]
    );
    assert!(initial.same_operation(&current));
}

#[cfg(unix)]
#[test]
fn external_reword_sequences_require_their_editor_without_skipping_message_edits() {
    for command_position in [1, 2] {
        let f = Fixture::new();
        f.divergent("file.txt", b"base\n", b"current\n", b"incoming\n");
        f.write("second.txt", "later commit\n");
        f.commit("original second subject");
        let sequence_editor = f.root.join(".git/sequence-editor");
        executable(
            &sequence_editor,
            &format!(
                "#!/bin/sh\nsed '{command_position}s/^pick /reword /' \"$1\" > \"$1.next\" && mv \"$1.next\" \"$1\"\n"
            ),
        );
        assert!(
            !f.output(&[
                "-c",
                &format!("sequence.editor={}", sequence_editor.display()),
                "rebase",
                "--interactive",
                "feature/beautiful-integration",
            ])
            .status
            .success()
        );
        let repo = f.repo();
        let preview = repo.conflict_preview(Path::new("file.txt")).unwrap();
        assert!(preview.operation.as_ref().unwrap().requires_message_editor);
        repo.execute_integration(&IntegrationCommand::Resolve {
            expected: Arc::new(preview),
            resolution: ConflictResolution::Incoming,
        })
        .unwrap();
        let expected = repo.operation_state().unwrap().unwrap();
        let todo = f.read(".git/rebase-merge/git-rebase-todo");
        let index = f.read(".git/index");
        let head = f.git(&["rev-parse", "HEAD"]);
        assert!(
            repo.execute_integration(&IntegrationCommand::Continue { expected })
                .unwrap_err()
                .to_string()
                .contains("message-editing step")
        );
        assert_eq!(f.read(".git/rebase-merge/git-rebase-todo"), todo);
        assert_eq!(f.read(".git/index"), index);
        assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
        let editor = f.root.join(".git/message-editor");
        executable(
            &editor,
            "#!/bin/sh\nprintf 'Edited through configured editor\\n' > \"$1\"\n",
        );
        let output = Command::new("git")
            .arg("-C")
            .arg(&f.root)
            .args(["rebase", "--continue"])
            .env("GIT_EDITOR", &editor)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(repo.operation_state().unwrap().is_none());
        assert!(
            f.git(&["log", "-2", "--format=%s"])
                .contains("Edited through configured editor")
        );
    }
}

#[test]
fn rebase_preserves_ignored_file_and_directory_collisions_before_any_mutation() {
    for (tracked_target, ignored_path, directory) in [
        ("new-file", "new-file", false),
        ("nested/file.txt", "nested", false),
        ("new-file", "new-file/private.txt", true),
    ] {
        let f = Fixture::new();
        f.write("base.txt", "base\n");
        f.commit("base");
        f.git(&["switch", "--create", "target"]);
        f.write(tracked_target, "target content\n");
        f.commit("target adds path");
        f.git(&["switch", "main"]);
        f.write("local.txt", "local committed work\n");
        f.commit("local commit");
        f.write(
            ".git/info/exclude",
            if directory {
                "new-file/\n"
            } else {
                ignored_path
            },
        );
        f.write(ignored_path, "private ignored work\n");
        let repo = f.repo();
        assert!(repo.status().unwrap().entries.is_empty());
        let plan = repo.integration_plan("target").unwrap();
        let head = f.git(&["rev-parse", "HEAD"]);
        let index = f.read(".git/index");
        let error = repo
            .execute_integration(&IntegrationCommand::Rebase { plan })
            .unwrap_err();
        assert!(
            error.to_string().contains("untracked or ignored"),
            "{error:#}"
        );
        assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
        assert_eq!(f.read(".git/index"), index);
        assert_eq!(f.read(ignored_path), b"private ignored work\n");
        assert!(repo.operation_state().unwrap().is_none());
    }
}

#[test]
fn rebase_protects_ignored_paths_introduced_only_by_an_intermediate_replayed_commit() {
    let f = Fixture::new();
    f.write("base.txt", "base\n");
    f.commit("base");
    f.git(&["switch", "--create", "target"]);
    f.write("target.txt", "target\n");
    f.commit("target commit");
    f.git(&["switch", "main"]);
    f.write("temporary.txt", "intermediate tracked data\n");
    f.commit("introduce temporary file");
    f.git(&["rm", "temporary.txt"]);
    f.commit("remove temporary file");
    f.write(".git/info/exclude", "temporary.txt\n");
    f.write(
        "temporary.txt",
        "ignored work must survive intermediate commits\n",
    );
    let repo = f.repo();
    let plan = repo.integration_plan("target").unwrap();
    let head = f.git(&["rev-parse", "HEAD"]);
    assert!(
        repo.execute_integration(&IntegrationCommand::Rebase { plan })
            .unwrap_err()
            .to_string()
            .contains("untracked or ignored")
    );
    assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(
        f.read("temporary.txt"),
        b"ignored work must survive intermediate commits\n"
    );
    assert!(repo.operation_state().unwrap().is_none());
}

#[test]
fn unrelated_ignored_build_outputs_survive_a_successful_rebase() {
    let f = Fixture::new();
    f.write("base.txt", "base\n");
    f.commit("base");
    f.git(&["switch", "--create", "target"]);
    f.write("target.txt", "target\n");
    let target = f.commit("target commit");
    f.git(&["switch", "main"]);
    f.write("local.txt", "local\n");
    f.commit("local commit");
    f.write(".git/info/exclude", "build/\n");
    f.write("build/result.bin", b"private\0build data");
    let repo = f.repo();
    let plan = repo.integration_plan("target").unwrap();
    repo.execute_integration(&IntegrationCommand::Rebase { plan })
        .unwrap();
    assert_eq!(f.read("build/result.bin"), b"private\0build data");
    assert_eq!(f.git(&["rev-parse", "HEAD^"]), target);
    assert!(repo.operation_state().unwrap().is_none());
}
