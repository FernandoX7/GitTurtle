use gitturtle_core::{BranchCommand, GitRepository, RemoteConfig, WriteCommand};
use std::{fs, path::PathBuf, process::Command};
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
        fixture.git(&["config", "user.name", "Branch Fixture"]);
        fixture.git(&["config", "user.email", "branch@example.invalid"]);
        fixture.git(&["config", "commit.gpgsign", "false"]);
        fixture.git(&["config", "core.hooksPath", ".git/hooks"]);
        fixture.write("file.txt", "baseline\n");
        fixture.write("other.txt", "other baseline\n");
        fixture.git(&["add", "--all"]);
        fixture.git(&["commit", "-m", "Baseline"]);
        fixture
    }
    fn repo(&self) -> GitRepository {
        GitRepository::open(&self.root).unwrap()
    }
    fn write(&self, path: &str, bytes: &str) {
        fs::write(self.root.join(path), bytes).unwrap();
    }
    fn git(&self, arguments: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(arguments)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim_end().into()
    }
    fn execute(&self, command: BranchCommand) {
        self.repo()
            .execute(&WriteCommand::Branch(command.into()))
            .unwrap();
    }
    fn local_remote(&self, name: &str) -> PathBuf {
        let destination = self.root.parent().unwrap().join(format!("{name}.git"));
        self.git(&["init", "--bare", destination.to_str().unwrap()]);
        self.execute(BranchCommand::AddRemote {
            remote: RemoteConfig::new(name, destination.to_str().unwrap()),
        });
        self.git(&["push", name, "main:main"]);
        destination
    }
    fn remote(&self, name: &str) -> RemoteConfig {
        self.repo()
            .remote_configs()
            .unwrap()
            .into_iter()
            .find(|remote| remote.name == name)
            .unwrap()
    }
    fn extra_commit(&self) -> String {
        let tree = self.git(&["rev-parse", "HEAD^{tree}"]);
        self.git(&[
            "commit-tree",
            &tree,
            "-p",
            "HEAD",
            "-m",
            "Independent commit",
        ])
    }
    fn index(&self) -> Vec<u8> {
        fs::read(self.root.join(".git/index")).unwrap()
    }
}

#[test]
fn tracking_branch_creation_is_local_explicit_and_preserves_dirty_work() {
    let f = Fixture::new();
    let remote = f.local_remote("origin");
    let head = f.git(&["rev-parse", "HEAD"]);
    f.write("other.txt", "staged unrelated\n");
    f.git(&["add", "other.txt"]);
    f.write("other.txt", "unstaged unrelated\n");
    let plan = f
        .repo()
        .tracking_branch_plan("feature/local", "refs/remotes/origin/main", false)
        .unwrap();
    // No server is needed: branch creation uses the locally available ref.
    fs::rename(&remote, remote.with_extension("offline")).unwrap();
    f.execute(BranchCommand::CreateTracking { plan });
    assert_eq!(f.git(&["rev-parse", "feature/local"]), head);
    assert_eq!(f.git(&["branch", "--show-current"]), "main");
    assert_eq!(f.git(&["config", "branch.feature/local.remote"]), "origin");
    assert_eq!(
        f.git(&["config", "branch.feature/local.merge"]),
        "refs/heads/main"
    );
    let plan = f
        .repo()
        .tracking_branch_plan("feature/active", "refs/remotes/origin/main", true)
        .unwrap();
    f.execute(BranchCommand::CreateTracking { plan });
    assert_eq!(f.git(&["branch", "--show-current"]), "feature/active");
    assert_eq!(f.git(&["show", ":other.txt"]), "staged unrelated");
    assert_eq!(
        fs::read_to_string(f.root.join("other.txt")).unwrap(),
        "unstaged unrelated\n"
    );
}

#[test]
fn tracking_creation_refuses_stale_tips_names_configuration_and_head_aliases() {
    for changed in ["tip", "name", "config"] {
        let f = Fixture::new();
        f.local_remote("origin");
        let plan = f
            .repo()
            .tracking_branch_plan("chosen", "refs/remotes/origin/main", false)
            .unwrap();
        match changed {
            "tip" => {
                f.git(&["update-ref", "refs/remotes/origin/main", &f.extra_commit()]);
            }
            "name" => {
                f.git(&["branch", "chosen"]);
            }
            _ => {
                f.git(&[
                    "config",
                    "remote.origin.fetch",
                    "+refs/heads/*:refs/remotes/elsewhere/*",
                ]);
            }
        }
        let before = f.git(&["for-each-ref", "--format=%(refname) %(objectname)"]);
        assert!(
            f.repo()
                .execute(&WriteCommand::Branch(
                    BranchCommand::CreateTracking { plan }.into()
                ))
                .is_err()
        );
        assert_eq!(
            f.git(&["for-each-ref", "--format=%(refname) %(objectname)"]),
            before
        );
        assert_eq!(f.git(&["branch", "--show-current"]), "main");
    }
    let f = Fixture::new();
    f.local_remote("origin");
    f.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);
    assert!(
        f.repo()
            .tracking_branch_plan("alias", "refs/remotes/origin/HEAD", false)
            .is_err()
    );
    assert!(
        f.repo()
            .tracking_branch_plan("invalid", "refs/heads/main", false)
            .is_err()
    );
}

#[test]
fn tracking_checkout_refusal_does_not_create_branch_or_overwrite_files() {
    let f = Fixture::new();
    f.local_remote("origin");
    f.git(&["switch", "-c", "source"]);
    f.write("file.txt", "remote changes\n");
    f.git(&["commit", "-am", "Remote tip"]);
    f.git(&["update-ref", "refs/remotes/origin/source", "HEAD"]);
    f.git(&["switch", "main"]);
    f.write("file.txt", "local dirty work\n");
    let plan = f
        .repo()
        .tracking_branch_plan("chosen", "refs/remotes/origin/source", true)
        .unwrap();
    assert!(
        f.repo()
            .execute(&WriteCommand::Branch(
                BranchCommand::CreateTracking { plan }.into()
            ))
            .is_err()
    );
    assert!(
        !f.repo()
            .branches()
            .unwrap()
            .iter()
            .any(|branch| !branch.remote && branch.name == "chosen")
    );
    assert_eq!(f.git(&["branch", "--show-current"]), "main");
    assert_eq!(
        fs::read_to_string(f.root.join("file.txt")).unwrap(),
        "local dirty work\n"
    );
}

#[test]
fn renaming_current_branch_preserves_tip_tracking_description_and_unrelated_work() {
    let f = Fixture::new();
    f.local_remote("origin");
    f.git(&["branch", "--set-upstream-to=origin/main", "main"]);
    f.git(&["config", "branch.main.description", "A useful description"]);
    f.write("file.txt", "staged work\n");
    f.git(&["add", "file.txt"]);
    f.write("file.txt", "later work\n");
    let head = f.git(&["rev-parse", "HEAD"]);
    let index = f.index();
    let plan = f.repo().branch_plan("main").unwrap();
    assert_eq!(plan.checked_out_in, vec![f.repo().path().to_path_buf()]);
    f.execute(BranchCommand::Rename {
        plan,
        new_name: "primary/new".into(),
    });
    assert_eq!(f.git(&["branch", "--show-current"]), "primary/new");
    assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(
        f.git(&["config", "branch.primary/new.description"]),
        "A useful description"
    );
    assert_eq!(f.git(&["config", "branch.primary/new.remote"]), "origin");
    assert_eq!(f.index(), index);
    assert_eq!(
        fs::read_to_string(f.root.join("file.txt")).unwrap(),
        "later work\n"
    );
}

#[test]
fn rename_refuses_collisions_stale_tips_and_branches_in_other_worktrees() {
    let f = Fixture::new();
    f.git(&["branch", "topic"]);
    let original = f.repo().branch_plan("topic").unwrap();
    assert!(
        f.repo()
            .execute(&WriteCommand::Branch(
                BranchCommand::Rename {
                    plan: original.clone(),
                    new_name: "main".into()
                }
                .into()
            ))
            .is_err()
    );
    f.git(&["update-ref", "refs/heads/topic", &f.extra_commit()]);
    assert!(
        f.repo()
            .execute(&WriteCommand::Branch(
                BranchCommand::Rename {
                    plan: original,
                    new_name: "stale".into()
                }
                .into()
            ))
            .is_err()
    );
    let plan = f.repo().branch_plan("topic").unwrap();
    f.git(&["worktree", "add", "../linked", "topic"]);
    assert!(
        f.repo()
            .execute(&WriteCommand::Branch(
                BranchCommand::Rename {
                    plan,
                    new_name: "occupied".into()
                }
                .into()
            ))
            .is_err()
    );
    let plan = f.repo().branch_plan("topic").unwrap();
    assert!(
        f.repo()
            .execute(&WriteCommand::Branch(
                BranchCommand::Rename {
                    plan,
                    new_name: "occupied".into()
                }
                .into()
            ))
            .unwrap_err()
            .to_string()
            .contains("another worktree")
    );
    assert!(
        f.repo()
            .branches()
            .unwrap()
            .iter()
            .any(|branch| branch.name == "topic")
    );
}

#[test]
fn rename_review_rejects_invalid_destinations_and_stale_context_before_writing() {
    let f = Fixture::new();
    f.git(&["branch", "topic"]);
    f.git(&["branch", "group/child"]);
    let repo = f.repo();
    let original = repo.branch_plan("topic").unwrap();
    f.write("file.txt", "local staged work\n");
    f.git(&["add", "file.txt"]);
    f.write("file.txt", "later unstaged work\n");
    let index = f.index();
    let refs = f.git(&["show-ref"]);
    let config = fs::read(f.root.join(".git/config")).unwrap();
    for name in [
        "bad name",
        "bad..branch",
        "-option",
        "HEAD",
        "topic",
        "main",
        "main/nested",
        "group",
        "group/child/nested",
    ] {
        let error = repo
            .rename_branch_plan(&original, name)
            .expect_err("Invalid rename review succeeded");
        if matches!(name, "bad name" | "bad..branch") {
            let message = format!("{error:#}");
            assert!(message.contains("feature/my-change"), "{message}");
            assert!(message.contains("spaces"), "{message}");
            assert!(!message.ends_with(':'), "{message}");
        }
    }
    assert_eq!(f.git(&["show-ref"]), refs);
    assert_eq!(f.index(), index);
    assert_eq!(fs::read(f.root.join(".git/config")).unwrap(), config);
    assert_eq!(
        fs::read(f.root.join("file.txt")).unwrap(),
        b"later unstaged work\n"
    );
    let reviewed = repo
        .rename_branch_plan(&original, "reviewed/topic")
        .unwrap();
    f.execute(BranchCommand::Rename {
        plan: reviewed,
        new_name: "reviewed/topic".into(),
    });
    assert_eq!(
        repo.branch_plan("reviewed/topic").unwrap().oid,
        original.oid
    );
    assert_eq!(f.index(), index);
    assert!(repo.rename_branch_plan(&original, "stale").is_err());
    let current = repo.branch_plan("reviewed/topic").unwrap();
    let other = Fixture::new();
    assert!(
        other
            .repo()
            .rename_branch_plan(&current, "wrong-repository")
            .is_err()
    );
    f.git(&["worktree", "add", "../linked", "reviewed/topic"]);
    assert!(repo.rename_branch_plan(&current, "stale-worktree").is_err());
    let occupied = repo.branch_plan("reviewed/topic").unwrap();
    assert!(
        repo.rename_branch_plan(&occupied, "occupied")
            .unwrap_err()
            .to_string()
            .contains("another worktree")
    );
}

#[test]
fn safe_delete_protects_unmerged_current_and_worktree_branches() {
    let f = Fixture::new();
    f.git(&["branch", "merged"]);
    f.git(&["branch", "unmerged", &f.extra_commit()]);
    f.git(&["worktree", "add", "-b", "occupied", "../linked"]);
    f.write("file.txt", "unrelated work\n");
    let index = f.index();
    for branch in ["main", "occupied", "unmerged"] {
        let plan = f.repo().branch_plan(branch).unwrap();
        if branch == "unmerged" {
            assert_eq!(plan.unmerged_commits, Some(1));
        }
        assert!(
            f.repo()
                .execute(&WriteCommand::Branch(BranchCommand::Delete { plan }.into()))
                .is_err()
        );
    }
    let plan = f.repo().branch_plan("merged").unwrap();
    assert_eq!(plan.unmerged_commits, Some(0));
    f.execute(BranchCommand::Delete { plan });
    assert!(
        !f.repo()
            .branches()
            .unwrap()
            .iter()
            .any(|branch| branch.name == "merged")
    );
    assert_eq!(f.index(), index);
    assert_eq!(
        fs::read_to_string(f.root.join("file.txt")).unwrap(),
        "unrelated work\n"
    );
}

#[test]
fn safe_delete_uses_upstream_and_refuses_a_stale_merge_target() {
    let f = Fixture::new();
    f.local_remote("origin");
    let commit = f.extra_commit();
    f.git(&["branch", "published", &commit]);
    f.git(&["update-ref", "refs/remotes/origin/published", &commit]);
    f.git(&["branch", "--set-upstream-to=origin/published", "published"]);
    let plan = f.repo().branch_plan("published").unwrap();
    assert_eq!(
        plan.merge_target.as_ref().unwrap().name,
        "refs/remotes/origin/published"
    );
    assert_eq!(plan.unmerged_commits, Some(0));
    let old_head = f.git(&["rev-parse", "HEAD"]);
    f.git(&["update-ref", "refs/remotes/origin/published", &old_head]);
    assert!(
        f.repo()
            .execute(&WriteCommand::Branch(BranchCommand::Delete { plan }.into()))
            .is_err()
    );
    f.git(&["update-ref", "refs/remotes/origin/published", &commit]);
    f.execute(BranchCommand::Delete {
        plan: f.repo().branch_plan("published").unwrap(),
    });
    assert_eq!(
        f.git(&["rev-parse", "refs/remotes/origin/published"]),
        commit
    );
}

#[test]
fn explicit_upstream_set_change_and_unset_preserve_working_contents() {
    let f = Fixture::new();
    f.local_remote("origin");
    f.git(&["branch", "topic"]);
    f.write("file.txt", "unrelated work\n");
    let index = f.index();
    let plan = f
        .repo()
        .upstream_plan("topic", Some("refs/remotes/origin/main"))
        .unwrap();
    f.execute(BranchCommand::SetUpstream { plan });
    assert_eq!(f.git(&["config", "branch.topic.remote"]), "origin");
    assert_eq!(f.git(&["config", "branch.topic.merge"]), "refs/heads/main");
    let plan = f
        .repo()
        .upstream_plan("topic", Some("refs/heads/main"))
        .unwrap();
    f.execute(BranchCommand::SetUpstream { plan });
    assert_eq!(f.git(&["config", "branch.topic.remote"]), ".");
    let plan = f.repo().upstream_plan("topic", None).unwrap();
    f.execute(BranchCommand::SetUpstream { plan });
    assert!(f.repo().branch_plan("topic").unwrap().upstream.is_none());
    assert!(
        f.repo()
            .upstream_plan("main", Some("refs/heads/main"))
            .is_err()
    );
    assert!(
        f.repo()
            .upstream_plan("topic", Some("refs/remotes/origin/missing"))
            .is_err()
    );
    assert_eq!(f.index(), index);
    assert_eq!(
        fs::read_to_string(f.root.join("file.txt")).unwrap(),
        "unrelated work\n"
    );
}

#[test]
fn upstream_refuses_stale_configuration_or_a_branch_occupied_elsewhere() {
    let f = Fixture::new();
    f.local_remote("origin");
    f.git(&["branch", "topic"]);
    let plan = f
        .repo()
        .upstream_plan("topic", Some("refs/remotes/origin/main"))
        .unwrap();
    f.git(&["config", "branch.topic.remote", "."]);
    f.git(&["config", "branch.topic.merge", "refs/heads/main"]);
    assert!(
        f.repo()
            .execute(&WriteCommand::Branch(
                BranchCommand::SetUpstream { plan }.into()
            ))
            .is_err()
    );
    f.git(&["worktree", "add", "../linked", "topic"]);
    let plan = f
        .repo()
        .upstream_plan("topic", Some("refs/remotes/origin/main"))
        .unwrap();
    assert!(
        f.repo()
            .execute(&WriteCommand::Branch(
                BranchCommand::SetUpstream { plan }.into()
            ))
            .unwrap_err()
            .to_string()
            .contains("another worktree")
    );
    assert_eq!(f.git(&["config", "branch.topic.remote"]), ".");
}

#[test]
fn remote_add_and_edit_multiple_urls_are_atomic_and_preserve_other_settings() {
    let f = Fixture::new();
    let mut remote = RemoteConfig::new("team/origin", "/unavailable/first.git");
    remote
        .urls
        .push("https://example.invalid/second.git".into());
    remote.push_urls = vec![
        "ssh://git@example.invalid/one.git".into(),
        "ssh://git@example.invalid/two.git".into(),
    ];
    f.execute(BranchCommand::AddRemote { remote });
    f.git(&[
        "config",
        "remote.team/origin.proxy",
        "socks5://localhost:9999",
    ]);
    f.git(&["config", "remote.team/origin.prune", "true"]);
    let expected = f.remote("team/origin");
    assert_eq!(expected.urls.len(), 2);
    let mut replacement = expected.clone();
    replacement.urls = vec!["/still/offline.git".into()];
    replacement.push_urls.clear();
    replacement.fetch_refspecs = vec![
        "+refs/heads/release/*:refs/remotes/team/origin/release/*".into(),
        "^refs/heads/release/private".into(),
    ];
    let index = f.index();
    f.execute(BranchCommand::EditRemote {
        expected,
        replacement: replacement.clone(),
    });
    let actual = f.remote("team/origin");
    assert_eq!(actual.urls, replacement.urls);
    assert!(actual.push_urls.is_empty());
    assert_eq!(actual.fetch_refspecs, replacement.fetch_refspecs);
    assert_eq!(
        f.git(&["config", "remote.team/origin.proxy"]),
        "socks5://localhost:9999"
    );
    assert_eq!(f.git(&["config", "remote.team/origin.prune"]), "true");
    assert_eq!(f.git(&["config", "user.name"]), "Branch Fixture");
    assert_eq!(f.index(), index);
    assert!(!f.root.join(".git/config.lock").exists());
}

#[test]
fn remote_edit_refuses_stale_settings_refs_invalid_specs_and_existing_lock() {
    let f = Fixture::new();
    f.local_remote("origin");
    let expected = f.remote("origin");
    let replacement = RemoteConfig::new("origin", "/new/offline.git");
    f.git(&["config", "remote.origin.proxy", "new proxy"]);
    assert!(
        f.repo()
            .execute(&WriteCommand::Branch(
                BranchCommand::EditRemote {
                    expected,
                    replacement: replacement.clone()
                }
                .into()
            ))
            .is_err()
    );
    let expected = f.remote("origin");
    f.git(&["update-ref", "refs/remotes/origin/main", &f.extra_commit()]);
    assert!(
        f.repo()
            .execute(&WriteCommand::Branch(
                BranchCommand::EditRemote {
                    expected,
                    replacement: replacement.clone()
                }
                .into()
            ))
            .is_err()
    );
    let expected = f.remote("origin");
    let config = fs::read(f.root.join(".git/config")).unwrap();
    let mut invalid = replacement.clone();
    invalid.fetch_refspecs = vec!["+refs/heads/*:refs/remotes/origin/no-wildcard".into()];
    assert!(
        f.repo()
            .execute(&WriteCommand::Branch(
                BranchCommand::EditRemote {
                    expected: expected.clone(),
                    replacement: invalid
                }
                .into()
            ))
            .is_err()
    );
    assert_eq!(fs::read(f.root.join(".git/config")).unwrap(), config);
    f.write(".git/config.lock", "another operation");
    assert!(
        f.repo()
            .execute(&WriteCommand::Branch(
                BranchCommand::EditRemote {
                    expected,
                    replacement
                }
                .into()
            ))
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(f.root.join(".git/config.lock")).unwrap(),
        "another operation"
    );
    assert_eq!(fs::read(f.root.join(".git/config")).unwrap(), config);
}

#[test]
fn remote_removal_reports_affected_relationships_and_preserves_local_and_shared_refs() {
    let f = Fixture::new();
    let server = f.local_remote("origin");
    f.git(&["branch", "--set-upstream-to=origin/main", "main"]);
    f.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);
    let mut mirror = RemoteConfig::new("mirror", server.to_str().unwrap());
    mirror.fetch_refspecs = vec!["+refs/heads/shared:refs/remotes/origin/shared".into()];
    f.execute(BranchCommand::AddRemote { remote: mirror });
    f.git(&["update-ref", "refs/remotes/origin/shared", "HEAD"]);
    let expected = f.remote("origin");
    assert_eq!(expected.upstream_branches, vec!["main"]);
    assert!(
        expected
            .tracking_refs
            .iter()
            .any(|reference| reference.name == "refs/remotes/origin/main")
    );
    assert!(
        !expected
            .tracking_refs
            .iter()
            .any(|reference| reference.name == "refs/remotes/origin/shared")
    );
    let head = f.git(&["rev-parse", "HEAD"]);
    f.write("file.txt", "unrelated work\n");
    f.execute(BranchCommand::RemoveRemote { expected });
    assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(f.git(&["rev-parse", "refs/remotes/origin/shared"]), head);
    assert!(
        !f.repo()
            .remote_configs()
            .unwrap()
            .iter()
            .any(|remote| remote.name == "origin")
    );
    assert!(f.repo().branch_plan("main").unwrap().upstream.is_none());
    assert_eq!(
        fs::read_to_string(f.root.join("file.txt")).unwrap(),
        "unrelated work\n"
    );
    assert!(
        server.join("refs/heads/main").exists(),
        "local remote removal changed the server"
    );
}

#[test]
fn remote_removal_refuses_new_tracking_relationships_and_external_definitions() {
    let f = Fixture::new();
    f.local_remote("origin");
    let expected = f.remote("origin");
    f.git(&["branch", "--set-upstream-to=origin/main", "main"]);
    assert!(
        f.repo()
            .execute(&WriteCommand::Branch(
                BranchCommand::RemoveRemote { expected }.into()
            ))
            .is_err()
    );
    f.write(".git/imported-config", "[remote \"imported\"]\n    url = /offline/imported.git\n    fetch = +refs/heads/*:refs/remotes/imported/*\n");
    f.git(&["config", "include.path", "imported-config"]);
    let expected = f.remote("imported");
    let before = fs::read(f.root.join(".git/imported-config")).unwrap();
    assert!(
        f.repo()
            .execute(&WriteCommand::Branch(
                BranchCommand::RemoveRemote {
                    expected: expected.clone()
                }
                .into()
            ))
            .unwrap_err()
            .to_string()
            .contains("configuration source")
    );
    assert!(
        f.repo()
            .execute(&WriteCommand::Branch(
                BranchCommand::EditRemote {
                    expected,
                    replacement: RemoteConfig::new("imported", "/replacement.git")
                }
                .into()
            ))
            .is_err()
    );
    assert_eq!(
        fs::read(f.root.join(".git/imported-config")).unwrap(),
        before
    );
}

#[test]
fn all_preparations_are_passive_and_cross_repository_plans_are_rejected() {
    let f = Fixture::new();
    f.local_remote("origin");
    f.git(&["branch", "topic"]);
    f.write("file.txt", "dirty\n");
    f.write(".gitattributes", "file.txt filter=fixture\n");
    f.git(&["config", "filter.fixture.clean", "touch filter-ran; cat"]);
    f.git(&["config", "core.fsmonitor", "touch fsmonitor-ran"]);
    let index = f.index();
    let config = fs::read(f.root.join(".git/config")).unwrap();
    let branch = f.repo().branch_plan("topic").unwrap();
    let tracking = f
        .repo()
        .tracking_branch_plan("tracking", "refs/remotes/origin/main", false)
        .unwrap();
    let upstream = f
        .repo()
        .upstream_plan("topic", Some("refs/remotes/origin/main"))
        .unwrap();
    let remote = f.remote("origin");
    assert_eq!(f.index(), index);
    assert_eq!(fs::read(f.root.join(".git/config")).unwrap(), config);
    assert!(!f.root.join("filter-ran").exists());
    assert!(!f.root.join("fsmonitor-ran").exists());
    let other = Fixture::new();
    for command in [
        BranchCommand::Delete { plan: branch },
        BranchCommand::CreateTracking { plan: tracking },
        BranchCommand::SetUpstream { plan: upstream },
        BranchCommand::RemoveRemote { expected: remote },
    ] {
        assert!(
            other
                .repo()
                .execute(&WriteCommand::Branch(command.into()))
                .is_err()
        );
    }
    assert_eq!(f.index(), index);
}

#[test]
fn linked_worktree_remote_edit_updates_shared_config_and_preserves_private_settings() {
    let f = Fixture::new();
    f.local_remote("origin");
    f.git(&["worktree", "add", "-b", "linked", "../linked"]);
    f.git(&["config", "extensions.worktreeConfig", "true"]);
    let linked = GitRepository::open(f.root.parent().unwrap().join("linked")).unwrap();
    linked
        .execute(&WriteCommand::SetIdentity {
            name: "Private Author".into(),
            email: "private@example.invalid".into(),
        })
        .unwrap();
    let expected = linked
        .remote_configs()
        .unwrap()
        .into_iter()
        .find(|remote| remote.name == "origin")
        .unwrap();
    let mut replacement = expected.clone();
    replacement.urls = vec!["/offline/replacement.git".into()];
    linked
        .execute(&WriteCommand::Branch(
            BranchCommand::EditRemote {
                expected,
                replacement,
            }
            .into(),
        ))
        .unwrap();
    assert_eq!(f.remote("origin").urls, vec!["/offline/replacement.git"]);
    assert_eq!(linked.profile().unwrap().name, "Private Author");
    assert_eq!(f.repo().profile().unwrap().name, "Branch Fixture");
    assert!(linked.status().unwrap().entries.is_empty());
    assert!(f.repo().status().unwrap().entries.is_empty());
}

#[test]
fn inherited_remote_options_and_upstream_relationships_are_not_partially_removed() {
    for inherited in ["option", "upstream"] {
        let f = Fixture::new();
        f.local_remote("origin");
        let imported = if inherited == "option" {
            "[remote \"origin\"]\n    proxy = socks5://localhost:9000\n"
        } else {
            "[branch \"main\"]\n    remote = origin\n    merge = refs/heads/main\n"
        };
        f.write(".git/imported-config", imported);
        f.git(&["config", "include.path", "imported-config"]);
        let expected = f.remote("origin");
        let config = fs::read(f.root.join(".git/config")).unwrap();
        let refs = f.git(&["for-each-ref", "--format=%(refname) %(objectname)"]);
        let error = f
            .repo()
            .execute(&WriteCommand::Branch(
                BranchCommand::RemoveRemote { expected }.into(),
            ))
            .unwrap_err();
        assert!(error.to_string().contains("configuration source"));
        assert_eq!(fs::read(f.root.join(".git/config")).unwrap(), config);
        assert_eq!(
            fs::read_to_string(f.root.join(".git/imported-config")).unwrap(),
            imported
        );
        assert_eq!(
            f.git(&["for-each-ref", "--format=%(refname) %(objectname)"]),
            refs
        );
    }
}

#[test]
fn duplicate_remote_is_preserved_and_option_like_urls_remain_literal() {
    let f = Fixture::new();
    f.execute(BranchCommand::AddRemote {
        remote: RemoteConfig::new("literal", "-literal-url"),
    });
    assert_eq!(f.remote("literal").urls, vec!["-literal-url"]);
    let before = fs::read(f.root.join(".git/config")).unwrap();
    assert!(
        f.repo()
            .execute(&WriteCommand::Branch(
                BranchCommand::AddRemote {
                    remote: RemoteConfig::new("literal", "replacement")
                }
                .into()
            ))
            .is_err()
    );
    let mut invalid = RemoteConfig::new("invalid", "valid-url");
    invalid.fetch_refspecs = vec!["+^refs/heads/private".into()];
    assert!(
        f.repo()
            .execute(&WriteCommand::Branch(
                BranchCommand::AddRemote { remote: invalid }.into()
            ))
            .is_err()
    );
    assert_eq!(fs::read(f.root.join(".git/config")).unwrap(), before);
    assert!(!f.root.join(".git/config.lock").exists());
}
