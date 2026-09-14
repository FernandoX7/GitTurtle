use gitturtle_core::GitRepository;
use std::{fs, path::Path, process::Command};

fn git(path: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn administration_directories_resolve_private_and_shared_linked_worktree_state() {
    let temp = tempfile::TempDir::new().unwrap();
    let main = temp.path().join("main");
    let linked = temp.path().join("linked with spaces");
    fs::create_dir(&main).unwrap();
    git(&main, &["init", "-b", "main"]);
    git(&main, &["commit", "--allow-empty", "-m", "Initial"]);
    git(
        &main,
        &["worktree", "add", "-b", "topic", linked.to_str().unwrap()],
    );

    let shared = main.join(".git").canonicalize().unwrap();
    let main_repo = GitRepository::open(&main).unwrap();
    assert_eq!(
        main_repo.git_directories().unwrap(),
        (shared.clone(), shared.clone())
    );
    let linked_repo = GitRepository::open(&linked).unwrap();
    let (private, common) = linked_repo.git_directories().unwrap();
    assert_eq!(common, shared);
    assert_ne!(private, common);
    assert!(private.starts_with(common.join("worktrees")));
    assert!(linked.join(".git").is_file());
    assert_eq!(
        fs::read(private.join("HEAD")).unwrap(),
        b"ref: refs/heads/topic\n"
    );
    assert_eq!(
        fs::read(common.join("HEAD")).unwrap(),
        b"ref: refs/heads/main\n"
    );
    assert!(private.join("index").is_file());
}

#[test]
fn local_watch_policy_preserves_tracked_ignored_paths_and_nested_git_rules() {
    let temp = tempfile::TempDir::new().unwrap();
    let path = temp.path();
    git(path, &["init", "-b", "main"]);
    fs::create_dir_all(path.join("build/keep")).unwrap();
    fs::create_dir_all(path.join("source/nested")).unwrap();
    fs::write(path.join("build/keep/tracked.txt"), "tracked\n").unwrap();
    git(path, &["add", "build/keep/tracked.txt"]);
    fs::write(
        path.join(".gitignore"),
        "/build/\n*.tmp\n/source/*\n!/source/nested/\n",
    )
    .unwrap();
    fs::write(
        path.join("source/nested/.gitignore"),
        "*.cache\n!keep.cache\n",
    )
    .unwrap();
    fs::write(path.join(".git/info/exclude"), "private/\n").unwrap();
    let global = path.join("global-ignore");
    fs::write(&global, "global/\n").unwrap();
    git(
        path,
        &["config", "core.excludesFile", global.to_str().unwrap()],
    );
    let before_index = fs::read(path.join(".git/index")).unwrap();
    let repo = GitRepository::open(path).unwrap();
    let mut policy = repo.local_watch_policy().unwrap();
    for (relative, directory, expected) in [
        ("build", true, true),
        ("build/keep", true, true),
        ("build/keep/tracked.txt", false, true),
        ("build/keep/untracked.txt", false, false),
        ("build/huge", true, false),
        ("source/nested", true, true),
        ("source/other", true, false),
        ("source/nested/hide.cache", false, false),
        ("source/nested/keep.cache", false, true),
        ("source/nested/deep/file.tmp", false, false),
        ("private", true, false),
        ("global", true, false),
        ("target", true, true),
        (".hidden", true, true),
    ] {
        assert_eq!(
            policy.includes(&path.join(relative), directory).unwrap(),
            expected,
            "{relative}"
        );
    }
    assert_eq!(fs::read(path.join(".git/index")).unwrap(), before_index);
}

#[test]
fn local_watch_policy_uses_linked_worktree_index_and_shared_excludes() {
    let temp = tempfile::TempDir::new().unwrap();
    let main = temp.path().join("main");
    let linked = temp.path().join("linked");
    fs::create_dir(&main).unwrap();
    git(&main, &["init", "-b", "main"]);
    git(&main, &["commit", "--allow-empty", "-m", "Initial"]);
    git(
        &main,
        &["worktree", "add", "-b", "topic", linked.to_str().unwrap()],
    );
    fs::create_dir_all(linked.join("ignored/deep")).unwrap();
    fs::write(linked.join("ignored/deep/tracked"), "local\n").unwrap();
    git(&linked, &["add", "ignored/deep/tracked"]);
    fs::write(main.join(".git/info/exclude"), "ignored/\n").unwrap();
    let mut policy = GitRepository::open(&linked)
        .unwrap()
        .local_watch_policy()
        .unwrap();
    assert!(policy.includes(&linked.join("ignored/deep"), true).unwrap());
    assert!(
        policy
            .includes(&linked.join("ignored/deep/tracked"), false)
            .unwrap()
    );
    assert!(
        !policy
            .includes(&linked.join("ignored/other"), true)
            .unwrap()
    );
    assert!(policy.includes(&main, true).is_err());
}

#[test]
fn local_watch_policy_releases_temporary_directory_matchers_and_source_bytes() {
    let temp = tempfile::TempDir::new().unwrap();
    let path = temp.path();
    git(path, &["init", "-b", "main"]);
    let mut policy = GitRepository::open(path)
        .unwrap()
        .local_watch_policy()
        .unwrap();
    // Missing rule files are cached too. Teardown must release those entries
    // even when a short-lived directory disappeared before its event arrived.
    for index in 0..17_000 {
        let transient = path.join(format!("transient-{index}"));
        assert!(policy.includes(&transient.join("child"), true).unwrap());
        policy.forget_directory(&transient);
    }
    let transient = path.join("large-rules");
    fs::create_dir(&transient).unwrap();
    fs::write(
        transient.join(".gitignore"),
        format!("#{}", "x".repeat(1024 * 1024 - 1)),
    )
    .unwrap();
    for _ in 0..24 {
        assert!(policy.includes(&transient.join("child"), true).unwrap());
        policy.forget_directory(&transient);
    }
}
