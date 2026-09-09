//! Publication fixtures use local disposable remotes only.
use gitturtle_core::PublicationInspection;
use gitturtle_core::{
    GitRepository, InteractiveRebaseCommand, LeasedPublishPlan, OperationControl, RebaseAction,
    RewriteReview, SeriesChange, WriteCommand, run_controlled,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    remote: PathBuf,
    base: String,
    original: String,
}
fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
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
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("worktree");
        let remote = temp.path().join("remote.git");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&remote).unwrap();
        git(&root, &["init", "-b", "main"]);
        git(&remote, &["init", "--bare", "-b", "main"]);
        for path in [&root, &remote] {
            git(path, &["config", "user.name", "Rewrite fixture"]);
            git(path, &["config", "user.email", "rewrite@example.invalid"]);
            git(path, &["config", "commit.gpgsign", "false"]);
        }
        git(&root, &["config", "core.hooksPath", ".git/hooks"]);
        git(&remote, &["config", "core.hooksPath", "hooks"]);
        fs::write(root.join("base"), "base\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-m", "base"]);
        let base = git(&root, &["rev-parse", "HEAD"]);
        for name in ["one", "two", "three"] {
            fs::write(root.join(name), format!("{name}\n")).unwrap();
            git(&root, &["add", name]);
            git(&root, &["commit", "-m", name]);
        }
        let original = git(&root, &["rev-parse", "HEAD"]);
        git(
            &root,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&root, &["push", "-u", "origin", "main"]);
        Self {
            _temp: temp,
            root,
            remote,
            base,
            original,
        }
    }
    fn repo(&self) -> GitRepository {
        GitRepository::open(&self.root).unwrap()
    }
    fn rewrite(&self) -> RewriteReview {
        let repo = self.repo();
        let plan = repo.interactive_rebase_plan(&self.base).unwrap();
        let mut steps = plan.steps();
        steps.swap(0, 1);
        steps[2].action = RebaseAction::Drop;
        repo.execute_interactive_rebase(&InteractiveRebaseCommand::Start {
            plan,
            steps,
            acknowledge_published: true,
        })
        .unwrap();
        repo.rewrite_review(&self.base, &self.original, "main")
            .unwrap()
    }
    fn plan(&self, review: &RewriteReview) -> LeasedPublishPlan {
        self.repo()
            .leased_publish_plan(review, "origin", "main")
            .unwrap()
    }
    fn remote_head(&self) -> String {
        git(&self.remote, &["rev-parse", "refs/heads/main"])
    }
    #[cfg(unix)]
    fn hook(&self, name: &str, script: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = self.remote.join("hooks").join(name);
        fs::write(&path, script).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn correspondence_tracks_reorder_and_drop_and_exact_lease_publishes_only_named_branch() {
    let fixture = Fixture::new();
    let review = fixture.rewrite();
    assert_eq!(review.original.len(), 3);
    assert_eq!(review.rewritten.len(), 2);
    assert_eq!(review.rows.iter().filter(|row| row.reordered).count(), 2);
    assert_eq!(
        review
            .rows
            .iter()
            .filter(|row| row.change == SeriesChange::Dropped)
            .count(),
        1
    );
    assert_eq!(
        fixture.remote_head(),
        fixture.original,
        "local rewrite must never publish"
    );
    let plan = fixture.plan(&review);
    assert_eq!(plan.expected_remote_oid, fixture.original);
    fixture
        .repo()
        .execute(&WriteCommand::PublishRewrite(Arc::new(plan)))
        .unwrap();
    assert_eq!(fixture.remote_head(), review.rewritten_head);
    assert_eq!(
        git(
            &fixture.remote,
            &["for-each-ref", "--format=%(refname)", "refs/heads/"]
        ),
        "refs/heads/main"
    );
}

#[test]
fn changed_patch_is_a_labelled_possible_match_and_new_and_empty_commits_are_honest() {
    let fixture = Fixture::new();
    let _ = fixture.rewrite();
    fs::write(fixture.root.join("one"), "changed contents\n").unwrap();
    git(&fixture.root, &["add", "one"]);
    git(&fixture.root, &["commit", "--amend", "--no-edit"]);
    git(
        &fixture.root,
        &["commit", "--allow-empty", "-m", "new empty commit"],
    );
    let review = fixture
        .repo()
        .rewrite_review(&fixture.base, &fixture.original, "main")
        .unwrap();
    assert!(
        review
            .rows
            .iter()
            .any(|row| row.change == SeriesChange::Possible && row.ambiguous)
    );
    assert!(
        review
            .rows
            .iter()
            .any(|row| row.change == SeriesChange::Added)
    );
}

#[test]
fn remote_movement_refuses_the_reviewed_lease_without_a_force_fallback() {
    let fixture = Fixture::new();
    let review = fixture.rewrite();
    let plan = fixture.plan(&review);
    let tree = git(
        &fixture.remote,
        &["rev-parse", &format!("{}^{{tree}}", fixture.original)],
    );
    let moved = git(
        &fixture.remote,
        &[
            "commit-tree",
            &tree,
            "-p",
            &fixture.original,
            "-m",
            "concurrent remote work",
        ],
    );
    git(
        &fixture.remote,
        &["update-ref", "refs/heads/main", &moved, &fixture.original],
    );
    let error = fixture
        .repo()
        .execute_leased_publish(&plan)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("lease") || error.contains("Leased"),
        "{error}"
    );
    assert_eq!(fixture.remote_head(), moved);
    assert!(
        fixture
            .repo()
            .leased_publish_plan(&review, "origin", "main")
            .unwrap_err()
            .to_string()
            .contains("no longer points")
    );
}

#[test]
fn local_tip_or_push_destination_movement_refuses_publication() {
    let fixture = Fixture::new();
    let review = fixture.rewrite();
    let plan = fixture.plan(&review);
    git(
        &fixture.root,
        &["commit", "--allow-empty", "-m", "local movement"],
    );
    assert!(
        fixture
            .repo()
            .execute_leased_publish(&plan)
            .unwrap_err()
            .to_string()
            .contains("local branch changed")
    );
    git(&fixture.root, &["reset", "--hard", &review.rewritten_head]);
    git(
        &fixture.root,
        &[
            "remote",
            "set-url",
            "--push",
            "origin",
            fixture.root.to_str().unwrap(),
        ],
    );
    assert!(
        fixture
            .repo()
            .execute_leased_publish(&plan)
            .unwrap_err()
            .to_string()
            .contains("destination changed")
    );
    assert_eq!(fixture.remote_head(), fixture.original);
}

#[test]
fn cancellation_before_network_write_preserves_remote_and_never_retries() {
    let fixture = Fixture::new();
    let review = fixture.rewrite();
    let plan = fixture.plan(&review);
    let control = OperationControl::default();
    control.cancel();
    assert!(
        run_controlled(control, || fixture.repo().execute_leased_publish(&plan))
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
    assert_eq!(fixture.remote_head(), fixture.original);
}

#[cfg(unix)]
#[test]
fn local_pre_push_hook_receives_reviewed_destination_and_can_refuse_publication() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let review = fixture.rewrite();
    let plan = fixture.plan(&review);
    let hook = fixture.root.join(".git/hooks/pre-push");
    fs::write(&hook, "#!/bin/sh\nprintf '%s\\n' \"$1\" \"$2\" > .git/reviewed-hook-destination\ncat > .git/reviewed-hook-refs\necho 'local publication policy refused' >&2\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    let error = fixture.repo().execute_leased_publish(&plan).unwrap_err();
    assert!(format!("{error:#}").contains("local publication policy refused"));
    assert_eq!(fixture.remote_head(), fixture.original);
    assert_eq!(
        git(&fixture.root, &["rev-parse", "HEAD"]),
        review.rewritten_head
    );
    let destinations =
        fs::read_to_string(fixture.root.join(".git/reviewed-hook-destination")).unwrap();
    assert_eq!(
        destinations.lines().collect::<Vec<_>>(),
        [plan.remote_url.as_str(), plan.remote_url.as_str()]
    );
    let refs = fs::read_to_string(fixture.root.join(".git/reviewed-hook-refs")).unwrap();
    let fields: Vec<_> = refs.split_whitespace().collect();
    assert_eq!(
        fields,
        [
            plan.new_oid.as_str(),
            plan.new_oid.as_str(),
            plan.remote_ref.as_str(),
            plan.expected_remote_oid.as_str()
        ]
    );
}

#[test]
fn remote_movement_requires_fetched_objects_then_a_separate_new_series_and_lease_review() {
    let fixture = Fixture::new();
    let review = fixture.rewrite();
    let tree = git(
        &fixture.remote,
        &["rev-parse", &format!("{}^{{tree}}", fixture.original)],
    );
    let moved = git(
        &fixture.remote,
        &[
            "commit-tree",
            &tree,
            "-p",
            &fixture.original,
            "-m",
            "new remote work to review",
        ],
    );
    git(
        &fixture.remote,
        &["update-ref", "refs/heads/main", &moved, &fixture.original],
    );
    let error = fixture
        .repo()
        .inspect_rewrite_publication(&review, "origin", "main")
        .unwrap_err();
    assert!(error.to_string().contains("Explicitly Fetch"), "{error:#}");
    assert_eq!(fixture.remote_head(), moved);
    git(&fixture.root, &["fetch", "--no-tags", "origin"]);
    let PublicationInspection::RemoteChanged(changed) = fixture
        .repo()
        .inspect_rewrite_publication(&review, "origin", "main")
        .unwrap()
    else {
        panic!("Remote movement must produce a new series review, not publication permission");
    };
    assert_eq!(changed.original_head, moved);
    assert_eq!(fixture.remote_head(), moved);
    let PublicationInspection::Ready(plan) = fixture
        .repo()
        .inspect_rewrite_publication(&changed, "origin", "main")
        .unwrap()
    else {
        panic!("Separately reviewed new series should prepare a fresh lease");
    };
    assert_eq!(plan.expected_remote_oid, moved);
    fixture.repo().execute_leased_publish(&plan).unwrap();
    assert!(matches!(
        fixture
            .repo()
            .inspect_rewrite_publication(&changed, "origin", "main")
            .unwrap(),
        PublicationInspection::AlreadyPublished { .. }
    ));
}

#[test]
fn missing_original_objects_are_explicit_and_never_fetched_by_local_review() {
    let fixture = Fixture::new();
    let review = fixture.rewrite();
    let object = fixture
        .root
        .join(".git/objects")
        .join(&fixture.original[..2])
        .join(&fixture.original[2..]);
    fs::remove_file(&object).unwrap();
    let error = fixture
        .repo()
        .rewrite_review(&fixture.base, &fixture.original, "main")
        .unwrap_err();
    assert!(
        error.to_string().contains("Original series is unavailable"),
        "{error:#}"
    );
    assert!(!object.exists());
    assert_eq!(fixture.remote_head(), fixture.original);
    assert_eq!(
        git(&fixture.root, &["rev-parse", "HEAD"]),
        review.rewritten_head
    );
}

#[test]
fn message_only_and_whitespace_changes_remain_changed_even_with_the_same_patch_fingerprint() {
    let fixture = Fixture::new();
    fixture.rewrite();
    git(
        &fixture.root,
        &["commit", "--amend", "-m", "Reworded first note"],
    );
    let review = fixture
        .repo()
        .rewrite_review(&fixture.base, &fixture.original, "main")
        .unwrap();
    assert!(
        review
            .rows
            .iter()
            .any(|row| row.change == SeriesChange::Changed)
    );
    fs::write(fixture.root.join("one"), "one  \n").unwrap();
    git(&fixture.root, &["add", "one"]);
    git(&fixture.root, &["commit", "--amend", "-m", "one"]);
    let review = fixture
        .repo()
        .rewrite_review(&fixture.base, &fixture.original, "main")
        .unwrap();
    assert!(
        review
            .rows
            .iter()
            .any(|row| row.change == SeriesChange::Changed)
    );
}

#[cfg(unix)]
#[test]
fn protected_branch_rejection_preserves_the_remote() {
    let fixture = Fixture::new();
    let review = fixture.rewrite();
    let plan = fixture.plan(&review);
    fixture.hook(
        "pre-receive",
        "#!/bin/sh\necho 'protected branch fixture' >&2\nexit 1\n",
    );
    let error = format!(
        "{:#}",
        fixture.repo().execute_leased_publish(&plan).unwrap_err()
    );
    assert!(error.contains("protected branch"), "{error}");
    assert_eq!(fixture.remote_head(), fixture.original);
}

#[cfg(unix)]
#[test]
fn cancellation_after_remote_update_reports_uncertainty_without_replaying() {
    use std::time::{Duration, Instant};
    let fixture = Fixture::new();
    let review = fixture.rewrite();
    let plan = fixture.plan(&review);
    fixture.hook(
        "post-receive",
        "#!/bin/sh\nprintf done > post-receive-entered\nwhile :; do sleep 1; done\n",
    );
    let repo = fixture.repo();
    let control = OperationControl::default();
    let running = control.clone();
    let thread =
        std::thread::spawn(move || run_controlled(running, || repo.execute_leased_publish(&plan)));
    let started = Instant::now();
    while !fixture.remote.join("post-receive-entered").exists()
        && started.elapsed() < Duration::from_secs(10)
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    let updated = fixture.remote.join("post-receive-entered").exists();
    control.cancel();
    let result = thread.join().unwrap();
    assert!(updated, "Remote did not reach post-receive hook");
    let error = format!("{:#}", result.unwrap_err());
    assert!(
        error.contains("may have applied partially or remotely"),
        "{error}"
    );
    assert_eq!(fixture.remote_head(), review.rewritten_head);
    assert!(
        fixture
            .repo()
            .leased_publish_plan(&review, "origin", "main")
            .is_err(),
        "A second attempt needs a new remote review"
    );
}
