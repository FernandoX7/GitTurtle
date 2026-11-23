use gitturtle_core::{
    BisectCommand, ChangeArea, CloneOptions, DiffAlgorithm, GitRepository, HistoryCancellation,
    IntegrationCommand, InteractiveRebaseCommand, MergeMode, PullStrategy, RebaseAction,
    RebaseStep, RecoveryCommand, SubmoduleState, TextPreview, WriteCommand,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .env_remove("GIT_ASKPASS")
        .env_remove("SSH_ASKPASS")
        .env_remove("SSH_ASKPASS_REQUIRE")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(args)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn init_repo() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("repo");
    fs::create_dir(&path).unwrap();
    git(&path, &["init", "-b", "main"]);
    git(&path, &["config", "user.name", "Ada Lovelace"]);
    git(&path, &["config", "user.email", "ada@example.com"]);
    fs::write(path.join("a.txt"), "line1\nline2\n").unwrap();
    git(&path, &["add", "a.txt"]);
    git(&path, &["commit", "-m", "initial commit"]);
    (tmp, path)
}

#[test]
fn commit_signoff_template_confinement_and_recent_messages_work_safely() {
    let (_tmp, path) = init_repo();
    let template_path = path.join(".gitmessage");
    fs::write(&template_path, "# Comment ignored\nfeat: \n\nBody line\n").unwrap();
    git(&path, &["config", "commit.template", ".gitmessage"]);

    let repo = GitRepository::open(&path).unwrap();
    let tpl = repo.commit_template_text().unwrap().expect("template");
    assert_eq!(tpl, "feat: \n\nBody line");

    // Repository-local commit.template must refuse escaping the working tree.
    git(&path, &["config", "commit.template", "/etc/hosts"]);
    assert!(repo.commit_template_text().is_err());
    git(&path, &["config", "commit.template", "../outside.txt"]);
    assert!(repo.commit_template_text().is_err());
    git(&path, &["config", "commit.template", ".gitmessage"]);

    fs::write(path.join("a.txt"), "line1\nline2\nline3\n").unwrap();
    repo.execute(&WriteCommand::StageAll).unwrap();
    let outcome = repo
        .execute(&WriteCommand::CommitWithOptions {
            message: "feat: add signoff".into(),
            signoff: true,
        })
        .unwrap();
    let oid = outcome.commit_oid.expect("commit oid");
    let raw = git(&path, &["show", "-s", "--format=%B", &oid]);
    assert!(raw.contains("Signed-off-by: Ada Lovelace <ada@example.com>"));

    let recent = repo.recent_commit_messages(5).unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].subject, "feat: add signoff");
    assert_eq!(recent[1].subject, "initial commit");
}

#[test]
fn pull_strategies_merge_rebase_autostash_and_merge_options_work() {
    let tmp = TempDir::new().unwrap();
    let bare = tmp.path().join("origin.git");
    git(tmp.path(), &["init", "--bare", bare.to_str().unwrap()]);

    let (_tmp1, repo_a) = init_repo();
    git(
        &repo_a,
        &["remote", "add", "origin", bare.to_str().unwrap()],
    );
    git(&repo_a, &["push", "-u", "origin", "main"]);

    let repo_b = tmp.path().join("clone_b");
    let cloned = GitRepository::clone_repository_with_options(
        bare.to_str().unwrap(),
        &repo_b,
        &CloneOptions {
            depth: Some(1),
            branch: Some("main".into()),
            recurse_submodules: false,
        },
    )
    .unwrap();
    git(&repo_b, &["config", "user.name", "Bob"]);
    git(&repo_b, &["config", "user.email", "bob@example.com"]);

    // Advance origin via repo_a
    fs::write(repo_a.join("remote.txt"), "from remote\n").unwrap();
    git(&repo_a, &["add", "remote.txt"]);
    git(&repo_a, &["commit", "-m", "remote commit"]);
    git(&repo_a, &["push", "origin", "main"]);

    // Advance clone_b locally so --ff-only would fail, then pull with Rebase
    fs::write(repo_b.join("local.txt"), "from local\n").unwrap();
    git(&repo_b, &["add", "local.txt"]);
    git(&repo_b, &["commit", "-m", "local commit"]);
    cloned
        .execute(&WriteCommand::PullWithStrategy {
            remote: "origin".into(),
            branch: "main".into(),
            strategy: PullStrategy::Rebase,
        })
        .unwrap();
    assert!(repo_b.join("remote.txt").exists());
    assert!(repo_b.join("local.txt").exists());

    // PullStrategy::Merge must succeed non-interactively even when pull.ff=only
    // and core.editor=/bin/false are configured.
    git(&repo_b, &["config", "pull.ff", "only"]);
    git(&repo_b, &["config", "core.editor", "/bin/false"]);
    fs::write(repo_a.join("remote2.txt"), "remote 2\n").unwrap();
    git(&repo_a, &["add", "remote2.txt"]);
    git(&repo_a, &["commit", "-m", "remote commit 2"]);
    git(&repo_a, &["push", "origin", "main"]);

    fs::write(repo_b.join("local2.txt"), "local 2\n").unwrap();
    git(&repo_b, &["add", "local2.txt"]);
    git(&repo_b, &["commit", "-m", "local commit 2"]);
    cloned
        .execute(&WriteCommand::PullWithStrategy {
            remote: "origin".into(),
            branch: "main".into(),
            strategy: PullStrategy::Merge,
        })
        .unwrap();
    assert!(repo_b.join("remote2.txt").exists());
    assert!(repo_b.join("local2.txt").exists());

    // PullStrategy::RebaseAutostash preserves dirty unstaged edits across pull.
    fs::write(repo_a.join("remote3.txt"), "remote 3\n").unwrap();
    git(&repo_a, &["add", "remote3.txt"]);
    git(&repo_a, &["commit", "-m", "remote commit 3"]);
    git(&repo_a, &["push", "origin", "main"]);
    fs::write(repo_b.join("local2.txt"), "dirty local edit\n").unwrap();
    cloned
        .execute(&WriteCommand::PullWithStrategy {
            remote: "origin".into(),
            branch: "main".into(),
            strategy: PullStrategy::RebaseAutostash,
        })
        .unwrap();
    assert!(repo_b.join("remote3.txt").exists());
    assert_eq!(
        fs::read_to_string(repo_b.join("local2.txt")).unwrap(),
        "dirty local edit\n"
    );

    // Feature branch merge --no-ff and --squash
    let repo = GitRepository::open(&repo_a).unwrap();
    git(&repo_a, &["checkout", "-b", "feat-noff"]);
    fs::write(repo_a.join("noff.txt"), "noff\n").unwrap();
    git(&repo_a, &["add", "noff.txt"]);
    git(&repo_a, &["commit", "-m", "noff commit"]);
    git(&repo_a, &["checkout", "main"]);
    let plan = repo.integration_plan("feat-noff").unwrap();
    repo.execute(&WriteCommand::Integration(
        IntegrationCommand::MergeWithOptions {
            plan: Box::new(plan),
            mode: MergeMode::NoFf,
            message: Some("Merge feat-noff explicitly".into()),
        },
    ))
    .unwrap();
    let parents = git(&repo_a, &["rev-list", "--parents", "-n", "1", "HEAD"]);
    assert_eq!(
        parents.split_whitespace().count(),
        3,
        "no-ff creates merge commit"
    );

    git(&repo_a, &["checkout", "-b", "feat-squash"]);
    fs::write(repo_a.join("squash.txt"), "sq1\n").unwrap();
    git(&repo_a, &["add", "squash.txt"]);
    git(&repo_a, &["commit", "-m", "squash commit 1"]);
    git(&repo_a, &["checkout", "main"]);
    let sq_plan = repo.integration_plan("feat-squash").unwrap();
    repo.execute(&WriteCommand::Integration(
        IntegrationCommand::MergeWithOptions {
            plan: Box::new(sq_plan),
            mode: MergeMode::Squash,
            message: None,
        },
    ))
    .unwrap();
    assert!(repo.squash_message().unwrap().is_some());
    assert!(
        repo.status()
            .unwrap()
            .entries
            .iter()
            .any(|e| e.path == Path::new("squash.txt") && e.staged.is_some())
    );
}

#[test]
fn checkout_pull_request_verifies_expected_sha() {
    let tmp = TempDir::new().unwrap();
    let bare = tmp.path().join("origin.git");
    git(tmp.path(), &["init", "--bare", bare.to_str().unwrap()]);

    let (_tmp1, repo_a) = init_repo();
    git(
        &repo_a,
        &["remote", "add", "origin", bare.to_str().unwrap()],
    );
    git(&repo_a, &["push", "-u", "origin", "main"]);

    // Publish a synthetic PR ref refs/pull/42/head on the bare remote.
    git(&repo_a, &["checkout", "-b", "pr-source"]);
    fs::write(repo_a.join("pr.txt"), "from pr 42\n").unwrap();
    git(&repo_a, &["add", "pr.txt"]);
    git(&repo_a, &["commit", "-m", "pr 42 commit"]);
    let pr_sha = git(&repo_a, &["rev-parse", "HEAD"]);
    git(&repo_a, &["push", "origin", "HEAD:refs/pull/42/head"]);
    git(&repo_a, &["checkout", "main"]);

    let repo = GitRepository::open(&repo_a).unwrap();
    // Mismatched expected SHA must be refused without creating the branch.
    let wrong_sha = "0000000000000000000000000000000000000000".to_owned();
    assert!(
        repo.execute(&WriteCommand::CheckoutPullRequest {
            remote: "origin".into(),
            number: 42,
            local_branch: "pr-42-wrong".into(),
            expected_head_sha: wrong_sha,
            expected_github_repo: None,
        })
        .is_err()
    );
    assert_eq!(repo.status().unwrap().branch.as_deref(), Some("main"));

    // Matching expected SHA checks out the exact reviewed commit.
    repo.execute(&WriteCommand::CheckoutPullRequest {
        remote: "origin".into(),
        number: 42,
        local_branch: "pr-42".into(),
        expected_head_sha: pr_sha.clone(),
        expected_github_repo: None,
    })
    .unwrap();
    assert_eq!(repo.status().unwrap().branch.as_deref(), Some("pr-42"));
    assert_eq!(git(&repo_a, &["rev-parse", "HEAD"]), pr_sha);
}

#[test]
fn stash_defaults_keep_index_staged_pop_and_branch_work() {
    let (_tmp, path) = init_repo();
    let repo = GitRepository::open(&path).unwrap();

    // Default stash name ("WIP on main") + PopStash
    fs::write(path.join("a.txt"), "changed\n").unwrap();
    let plan = repo.stash_create_plan(false).unwrap();
    repo.execute(&WriteCommand::Recovery(
        RecoveryCommand::CreateStash {
            plan,
            name: String::new(),
        }
        .into(),
    ))
    .unwrap();
    let stashes = repo.stash_list(0, 10).unwrap().entries;
    assert_eq!(stashes.len(), 1);
    assert!(stashes[0].name.contains("WIP on main"));

    let pop_plan = repo.stash_apply_plan(&stashes[0], false).unwrap();
    repo.execute(&WriteCommand::Recovery(
        RecoveryCommand::PopStash { plan: pop_plan }.into(),
    ))
    .unwrap();
    assert!(repo.stash_list(0, 10).unwrap().entries.is_empty());
    assert_eq!(fs::read_to_string(path.join("a.txt")).unwrap(), "changed\n");

    // --keep-index preserves staged changes while stashing unstaged changes
    git(&path, &["add", "a.txt"]);
    fs::write(path.join("a.txt"), "changed\nunstaged-line\n").unwrap();
    let keep_plan = repo
        .stash_create_plan_with_options(false, true, false, None)
        .unwrap();
    repo.execute(&WriteCommand::Recovery(
        RecoveryCommand::CreateStash {
            plan: keep_plan,
            name: "keep-index test".into(),
        }
        .into(),
    ))
    .unwrap();
    assert_eq!(fs::read_to_string(path.join("a.txt")).unwrap(), "changed\n");
    assert!(
        repo.status()
            .unwrap()
            .entries
            .iter()
            .any(|e| e.staged.is_some() && e.unstaged.is_none())
    );

    // Reset staged change cleanly so we can test BranchFromStash
    git(&path, &["reset", "--hard", "HEAD"]);
    let stashes = repo.stash_list(0, 10).unwrap().entries;
    let br_plan = repo.stash_branch_plan(&stashes[0]).unwrap();
    repo.execute(&WriteCommand::Recovery(
        RecoveryCommand::BranchFromStash {
            plan: br_plan,
            branch: "from-stash".into(),
        }
        .into(),
    ))
    .unwrap();
    assert_eq!(repo.status().unwrap().branch.as_deref(), Some("from-stash"));
    assert!(repo.stash_list(0, 10).unwrap().entries.is_empty());
}

#[test]
fn interactive_rebase_edit_stop_pauses_cleanly_and_continues() {
    let (_tmp, path) = init_repo();
    let base = git(&path, &["rev-parse", "HEAD"]);
    fs::write(path.join("a.txt"), "v2\n").unwrap();
    git(&path, &["add", "a.txt"]);
    git(&path, &["commit", "-m", "second"]);
    let c2 = git(&path, &["rev-parse", "HEAD"]);
    fs::write(path.join("b.txt"), "v3\n").unwrap();
    git(&path, &["add", "b.txt"]);
    git(&path, &["commit", "-m", "third"]);
    let c3 = git(&path, &["rev-parse", "HEAD"]);

    let repo = GitRepository::open(&path).unwrap();
    let plan = repo.interactive_rebase_plan(&base).unwrap();
    let steps = vec![
        RebaseStep {
            oid: c2,
            action: RebaseAction::Edit,
        },
        RebaseStep {
            oid: c3,
            action: RebaseAction::Pick,
        },
    ];
    repo.execute(&WriteCommand::InteractiveRebase(
        InteractiveRebaseCommand::Start {
            plan,
            steps,
            acknowledge_published: false,
        }
        .into(),
    ))
    .unwrap();

    let op = repo.operation_state().unwrap().expect("paused on edit");
    assert!(op.paused_for_edit);
    assert!(!op.requires_message_editor);
    let resume = repo.interactive_rebase_resume().unwrap();
    assert!(resume.message.is_none());

    // Continue directly without needing message form
    repo.execute(&WriteCommand::Integration(IntegrationCommand::Continue {
        expected: op,
    }))
    .unwrap();
    assert!(repo.operation_state().unwrap().is_none());
}

#[test]
fn bisect_lifecycle_midpoint_convergence_patch_apply_and_diff_algorithms_work() {
    let (_tmp, path) = init_repo();
    let good = git(&path, &["rev-parse", "HEAD"]);
    let mut commits = vec![good.clone()];
    for i in 1..=3 {
        fs::write(path.join("a.txt"), format!("step {i}\n")).unwrap();
        git(&path, &["add", "a.txt"]);
        git(&path, &["commit", "-m", &format!("commit {i}")]);
        commits.push(git(&path, &["rev-parse", "HEAD"]));
    }
    let bad = commits[3].clone();
    let repo = GitRepository::open(&path).unwrap();

    // Format range patch and test DiffAlgorithm options
    let cancel = HistoryCancellation::default();
    let range_patch = repo.format_patch_range(&good, &bad, &cancel).unwrap();
    assert!(range_patch.contains("Subject: [PATCH"));

    // Unsigned commit signature returns None with zero subprocesses even if repo config has hostile gpg.program
    git(&path, &["config", "gpg.program", "/bin/false"]);
    assert_eq!(repo.commit_signature(&bad, &cancel).unwrap(), None);

    // Diff algorithms all produce valid patches
    fs::write(path.join("a.txt"), "step 3\nnext\n").unwrap();
    let entry = repo.status().unwrap().entries.remove(0);
    for algo in [
        DiffAlgorithm::Patience,
        DiffAlgorithm::Myers,
        DiffAlgorithm::Histogram,
    ] {
        let preview = repo
            .worktree_preview_with_algorithm(&entry, ChangeArea::Unstaged, algo)
            .unwrap();
        assert!(matches!(preview.preview, TextPreview::Patch(_)));
    }
    // Capture patch, revert file, then apply patch via plan_apply_patch + ApplyPatch
    let mut patch_bytes = git(&path, &["diff", "--", "a.txt"]).into_bytes();
    patch_bytes.push(b'\n');
    git(&path, &["checkout", "--", "a.txt"]);
    let apply_plan = repo.plan_apply_patch(&patch_bytes, false, false).unwrap();
    assert!(apply_plan.stat_summary.contains("a.txt"));
    repo.execute(&WriteCommand::ApplyPatch {
        plan: std::sync::Arc::new(apply_plan),
        patch_bytes,
    })
    .unwrap();
    assert_eq!(
        fs::read_to_string(path.join("a.txt")).unwrap(),
        "step 3\nnext\n"
    );
    git(&path, &["checkout", "--", "a.txt"]);

    // Guided bisect start accepts non-OID revisions ("HEAD", short SHA) and
    // records both endpoints and checks out an intermediate midpoint commit.
    assert!(repo.bisect_state().unwrap().is_none());
    let short_good: String = good.chars().take(7).collect();
    repo.execute(&WriteCommand::Bisect(std::sync::Arc::new(
        BisectCommand::Start {
            bad: "HEAD".into(),
            good: short_good,
        },
    )))
    .unwrap();
    let state = repo.bisect_state().unwrap().expect("active bisect");
    assert_eq!(state.original_ref, "main");
    assert_eq!(state.bad_oid.as_deref(), Some(bad.as_str()));
    assert_eq!(state.good_oids, vec![good.clone()]);
    assert_ne!(
        state.current_oid, bad,
        "bisect start must check out a midpoint commit"
    );
    assert_ne!(
        state.current_oid, good,
        "bisect start must check out a midpoint commit"
    );
    assert_eq!(repo.status().unwrap().operation.as_deref(), Some("Bisect"));

    // Drive bisect to converge on commit 2 as first bad commit.
    let mut current = state;
    for _ in 0..4 {
        if current.culprit_oid.is_some() {
            break;
        }
        let step_is_bad = current.current_oid == commits[2] || current.current_oid == commits[3];
        let cmd = if step_is_bad {
            BisectCommand::Bad { expected: current }
        } else {
            BisectCommand::Good { expected: current }
        };
        repo.execute(&WriteCommand::Bisect(std::sync::Arc::new(cmd)))
            .unwrap();
        current = repo.bisect_state().unwrap().expect("still bisecting");
    }
    assert_eq!(current.culprit_oid.as_deref(), Some(commits[2].as_str()));

    repo.execute(&WriteCommand::Bisect(std::sync::Arc::new(
        BisectCommand::Reset { expected: current },
    )))
    .unwrap();
    assert!(repo.bisect_state().unwrap().is_none());
    assert_eq!(repo.status().unwrap().branch.as_deref(), Some("main"));
}

#[test]
fn submodules_inspection_returns_empty_without_gitmodules_and_lists_when_present() {
    let (_tmp, path) = init_repo();
    let repo = GitRepository::open(&path).unwrap();
    assert!(repo.submodules().unwrap().is_empty());

    // Create a local child repo and add it as a submodule
    let (_sub_tmp, sub_src) = init_repo();
    let out = Command::new("git")
        .current_dir(&path)
        .env_remove("GIT_ASKPASS")
        .env_remove("SSH_ASKPASS")
        .env_remove("SSH_ASKPASS_REQUIRE")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            sub_src.to_str().unwrap(),
            "libs/child",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    git(&path, &["commit", "-m", "add submodule"]);

    let subs = repo.submodules().unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].path, Path::new("libs/child"));
    assert_eq!(subs[0].state, SubmoduleState::Clean);
}
