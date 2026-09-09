//! Every mutation is confined to a disposable local repository.
use gitturtle_core::{
    GitRepository, IntegrationCommand, InteractiveRebaseCommand, OperationControl, RebaseAction,
    WriteCommand, run_controlled,
};
use std::{fs, path::PathBuf, process::Command};

#[test]
fn message_recovery_identity_survives_restart_and_refuses_changed_index_or_git_message() {
    let fixture = Fixture::new();
    fixture.commit("first", "original message");
    let _ = fixture.start(&[RebaseAction::Reword]);
    let before = fixture.repo().interactive_rebase_resume().unwrap();
    let token = before.draft_identity();
    assert_eq!(
        fixture
            .repo()
            .interactive_rebase_resume()
            .unwrap()
            .draft_identity(),
        token
    );
    fs::write(fixture.root.join("extra"), "external staged edit").unwrap();
    fixture.git(&["add", "extra"]);
    assert_ne!(
        fixture
            .repo()
            .interactive_rebase_resume()
            .unwrap()
            .draft_identity(),
        token
    );
    fixture.git(&["reset", "--", "extra"]);
    assert_eq!(
        fixture
            .repo()
            .interactive_rebase_resume()
            .unwrap()
            .draft_identity(),
        token
    );
    let message = fixture.root.join(".git/rebase-merge/message");
    fs::write(message, "externally edited Git message\n").unwrap();
    assert_ne!(
        fixture
            .repo()
            .interactive_rebase_resume()
            .unwrap()
            .draft_identity(),
        token
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("extra")).unwrap(),
        "external staged edit"
    );
}

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("worktree");
        fs::create_dir(&root).unwrap();
        let this = Self { _temp: temp, root };
        this.git(&["init", "-b", "main"]);
        this.git(&["config", "user.name", "Rebase Fixture"]);
        this.git(&["config", "user.email", "rebase@example.invalid"]);
        this.git(&["config", "commit.gpgsign", "false"]);
        this.git(&["config", "core.hooksPath", ".git/hooks"]);
        this.commit("base", "base\n");
        this
    }
    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim_end().into()
    }
    fn commit(&self, file: &str, message: &str) -> String {
        fs::write(self.root.join(file), message).unwrap();
        self.git(&["add", file]);
        self.git(&["commit", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }
    fn repo(&self) -> GitRepository {
        GitRepository::open(&self.root).unwrap()
    }
    fn start(&self, actions: &[RebaseAction]) -> anyhow::Result<gitturtle_core::WriteOutcome> {
        let repo = self.repo();
        let plan = repo.interactive_rebase_plan(&format!("HEAD~{}", actions.len()))?;
        let mut steps = plan.steps();
        for (step, action) in steps.iter_mut().zip(actions) {
            step.action = *action;
        }
        repo.execute(&WriteCommand::InteractiveRebase(std::sync::Arc::new(
            InteractiveRebaseCommand::Start {
                plan,
                steps,
                acknowledge_published: false,
            },
        )))
    }
    fn resume(&self, message: Option<&str>) -> anyhow::Result<gitturtle_core::WriteOutcome> {
        let repo = self.repo();
        let expected = repo.interactive_rebase_resume()?;
        let message = message
            .map(str::to_owned)
            .or_else(|| expected.message.clone());
        repo.execute_interactive_rebase(&InteractiveRebaseCommand::Continue { expected, message })
    }
}

#[test]
fn reorder_drop_and_fixup_preserve_unrelated_files_and_exact_pick_message() {
    let f = Fixture::new();
    let base = f.git(&["rev-parse", "HEAD"]);
    f.commit("first", "first\n\nBody with formatting.\n\n");
    let second = f.commit("second", "second");
    f.commit("third", "third");
    f.commit("fourth", "fourth");
    let repo = f.repo();
    let plan = repo.interactive_rebase_plan(&base).unwrap();
    let original_message = plan.commits[1].message.clone();
    let mut steps = plan.steps();
    steps.swap(0, 1);
    steps[1].action = RebaseAction::Fixup;
    steps[2].action = RebaseAction::Drop;
    fs::write(f.root.join("untracked"), "keep this").unwrap();
    repo.execute_interactive_rebase(&InteractiveRebaseCommand::Start {
        plan,
        steps,
        acknowledge_published: false,
    })
    .unwrap();
    assert_eq!(
        f.git(&["rev-list", "--count", &format!("{base}..HEAD")]),
        "2"
    );
    assert_eq!(
        f.git(&["show", "HEAD^:first"]),
        "first\n\nBody with formatting."
    );
    assert_eq!(
        f.git(&["log", "-1", "--format=%B", "HEAD^"]).trim_end(),
        original_message.trim_end()
    );
    assert_ne!(f.git(&["rev-parse", "HEAD^"]), second);
    assert!(!f.root.join("third").exists());
    assert_eq!(
        fs::read_to_string(f.root.join("untracked")).unwrap(),
        "keep this"
    );
    assert!(repo.operation_state().unwrap().is_none());
}

#[test]
fn reword_and_squash_pause_for_each_native_message_and_resume_after_reopen() {
    let f = Fixture::new();
    f.commit("one", "first original\n\nfirst body");
    f.commit("two", "second original");
    f.commit("three", "third original");
    assert!(
        f.start(&[
            RebaseAction::Reword,
            RebaseAction::Squash,
            RebaseAction::Reword
        ])
        .unwrap()
        .message
        .contains("paused")
    );
    let first = f.repo().interactive_rebase_resume().unwrap();
    assert!(first.message.unwrap().contains("first original"));
    assert!(
        f.resume(Some("first edited\n\nRetained body.\n"))
            .unwrap()
            .message
            .contains("paused")
    );
    let squash = f.repo().interactive_rebase_resume().unwrap();
    let template = squash.message.unwrap();
    assert!(
        template.contains("first edited") && template.contains("second original"),
        "{template}"
    );
    assert!(
        f.resume(Some("combined\n\nBoth changes.\n"))
            .unwrap()
            .message
            .contains("paused")
    );
    f.resume(Some("third edited\n\nExact body.\n")).unwrap();
    assert_eq!(
        f.git(&["log", "-2", "--format=%s"]),
        "third edited\ncombined"
    );
    assert!(f.repo().operation_state().unwrap().is_none());
}

#[test]
fn stale_branch_base_remote_warning_and_invalid_sequences_refuse_without_mutation() {
    for stale in ["branch", "base", "remote", "working"] {
        let f = Fixture::new();
        let base = f.git(&["rev-parse", "HEAD"]);
        f.git(&["branch", "review-base", &base]);
        f.commit("one", "one");
        f.commit("two", "two");
        let repo = f.repo();
        let plan = repo.interactive_rebase_plan("review-base").unwrap();
        let steps = plan.steps();
        match stale {
            "branch" => {
                f.commit("three", "three");
            }
            "base" => {
                f.git(&["branch", "-f", "review-base", "HEAD^"]);
            }
            "remote" => {
                f.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
            }
            _ => {
                fs::write(f.root.join("one"), "unrelated changed work").unwrap();
            }
        }
        let head = f.git(&["rev-parse", "HEAD"]);
        let index = fs::read(f.root.join(".git/index")).unwrap();
        assert!(
            repo.execute_interactive_rebase(&InteractiveRebaseCommand::Start {
                plan,
                steps,
                acknowledge_published: false
            })
            .is_err()
        );
        assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
        assert_eq!(fs::read(f.root.join(".git/index")).unwrap(), index);
        assert!(repo.operation_state().unwrap().is_none());
    }
    let f = Fixture::new();
    f.commit("one", "one");
    f.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
    let plan = f.repo().interactive_rebase_plan("HEAD^").unwrap();
    assert_eq!(plan.known_published_refs, ["refs/remotes/origin/main"]);
    let mut steps = plan.steps();
    steps[0].action = RebaseAction::Squash;
    assert!(plan.validate_steps(&steps).is_err());
    steps[0].action = RebaseAction::Pick;
    assert!(
        f.repo()
            .execute_interactive_rebase(&InteractiveRebaseCommand::Start {
                plan,
                steps,
                acknowledge_published: false
            })
            .unwrap_err()
            .to_string()
            .contains("Acknowledge")
    );
}

#[test]
fn conflicts_can_abort_or_resolve_and_continue_without_losing_original_branch() {
    for abort in [false, true] {
        let f = Fixture::new();
        f.commit("file", "initial");
        let base = f.git(&["rev-parse", "HEAD"]);
        f.commit("file", "first edit");
        f.commit("file", "second edit");
        let repo = f.repo();
        let plan = repo.interactive_rebase_plan(&base).unwrap();
        let original = plan.head.clone();
        let mut steps = plan.steps();
        steps.swap(0, 1);
        assert!(
            repo.execute_interactive_rebase(&InteractiveRebaseCommand::Start {
                plan,
                steps,
                acknowledge_published: false
            })
            .is_err()
        );
        assert!(
            repo.status()
                .unwrap()
                .entries
                .iter()
                .any(|entry| entry.conflicted)
        );
        if abort {
            let expected = repo.operation_state().unwrap().unwrap();
            repo.execute_integration(&IntegrationCommand::Abort { expected })
                .unwrap();
            assert_eq!(f.git(&["rev-parse", "HEAD"]), original);
            assert_eq!(f.git(&["show", ":file"]), "second edit");
        } else {
            for _ in 0..2 {
                fs::write(f.root.join("file"), "resolution").unwrap();
                f.git(&["add", "file"]);
                let result = f.resume(None);
                if repo.operation_state().unwrap().is_none() {
                    result.unwrap();
                    break;
                }
            }
            assert!(repo.operation_state().unwrap().is_none());
            assert_eq!(f.git(&["branch", "--show-current"]), "main");
        }
    }
}

#[test]
fn stale_continue_never_amends_new_staging_or_changed_message() {
    let f = Fixture::new();
    f.commit("one", "one");
    f.start(&[RebaseAction::Reword]).unwrap();
    let repo = f.repo();
    let expected = repo.interactive_rebase_resume().unwrap();
    fs::write(f.root.join("unrelated"), "keep staged").unwrap();
    f.git(&["add", "unrelated"]);
    let head = f.git(&["rev-parse", "HEAD"]);
    assert!(
        repo.execute_interactive_rebase(&InteractiveRebaseCommand::Continue {
            expected,
            message: Some("edited".into())
        })
        .is_err()
    );
    assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(f.git(&["show", ":unrelated"]), "keep staged");
}

#[cfg(unix)]
fn executable(path: &std::path::Path, content: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, content).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[cfg(unix)]
#[test]
fn signing_and_message_hook_failures_remain_paused_and_can_be_retried_explicitly() {
    for signing in [true, false] {
        let f = Fixture::new();
        f.commit("one", "one");
        f.start(&[RebaseAction::Reword]).unwrap();
        let refusal = if signing {
            f.root.join(".git/signer")
        } else {
            f.root.join(".git/hooks/commit-msg")
        };
        executable(
            &refusal,
            "#!/bin/sh\necho fixture-rebase-refusal >&2\nexit 1\n",
        );
        if signing {
            f.git(&["config", "commit.gpgsign", "true"]);
            f.git(&["config", "gpg.format", "openpgp"]);
            f.git(&["config", "gpg.program", refusal.to_str().unwrap()]);
        }
        let head = f.git(&["rev-parse", "HEAD"]);
        let error = f.resume(Some("native edited message")).unwrap_err();
        assert!(
            format!("{error:#}").contains("fixture-rebase-refusal"),
            "{error:#}"
        );
        assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
        assert!(f.repo().operation_state().unwrap().is_some());
        if signing {
            f.git(&["config", "commit.gpgsign", "false"]);
        } else {
            fs::remove_file(refusal).unwrap();
        }
        f.resume(Some("native edited message")).unwrap();
        assert_eq!(
            f.git(&["log", "-1", "--format=%s"]),
            "native edited message"
        );
    }
}

#[cfg(unix)]
#[test]
fn cancellation_during_message_hook_preserves_paused_state_and_never_retries() {
    let f = Fixture::new();
    f.commit("one", "one");
    f.start(&[RebaseAction::Reword]).unwrap();
    executable(
        &f.root.join(".git/hooks/commit-msg"),
        "#!/bin/sh\necho started > .git/cancellation-started\nsleep 30\n",
    );
    let repo = f.repo();
    let expected = repo.interactive_rebase_resume().unwrap();
    let control = OperationControl::default();
    let cancel = control.clone();
    let marker = f.root.join(".git/cancellation-started");
    let canceller = std::thread::spawn(move || {
        for _ in 0..200 {
            if marker.exists() {
                cancel.cancel();
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("hook was not reached");
    });
    let head = f.git(&["rev-parse", "HEAD"]);
    let error = run_controlled(control, || {
        repo.execute_interactive_rebase(&InteractiveRebaseCommand::Continue {
            expected,
            message: Some("cancelled edit".into()),
        })
    })
    .unwrap_err();
    canceller.join().unwrap();
    assert!(
        format!("{error:#}").to_lowercase().contains("cancel"),
        "{error:#}"
    );
    assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
    assert!(repo.operation_state().unwrap().is_some());
    fs::remove_file(f.root.join(".git/hooks/commit-msg")).unwrap();
    let original = f.git(&["rev-parse", "ORIG_HEAD"]);
    let expected = repo.operation_state().unwrap().unwrap();
    repo.execute_integration(&IntegrationCommand::Abort { expected })
        .unwrap();
    assert_eq!(f.git(&["rev-parse", "HEAD"]), original);
}

#[test]
fn merge_ranges_and_ignored_intermediate_collisions_are_rejected() {
    let f = Fixture::new();
    let base = f.git(&["rev-parse", "HEAD"]);
    f.git(&["switch", "-c", "side"]);
    f.commit("side", "side");
    f.git(&["switch", "main"]);
    f.commit("main", "main");
    f.git(&["merge", "--no-edit", "side"]);
    assert!(
        f.repo()
            .interactive_rebase_plan(&base)
            .unwrap_err()
            .to_string()
            .contains("linear")
    );
    let f = Fixture::new();
    f.commit("temporary", "intermediate");
    f.git(&["rm", "temporary"]);
    f.git(&["commit", "-m", "remove"]);
    fs::write(f.root.join(".git/info/exclude"), "temporary\n").unwrap();
    fs::write(f.root.join("temporary"), "private ignored data").unwrap();
    assert!(
        f.start(&[RebaseAction::Pick, RebaseAction::Pick])
            .unwrap_err()
            .to_string()
            .contains("untracked or ignored")
    );
    assert_eq!(
        fs::read_to_string(f.root.join("temporary")).unwrap(),
        "private ignored data"
    );
}
