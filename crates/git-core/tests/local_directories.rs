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
