use gitturtle_core::{ComparisonMode, GitRepository, HistoryCancellation, PathScope};
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn fixture() -> (TempDir, GitRepository) {
    let temp = TempDir::new().unwrap();
    let repo = GitRepository::init(temp.path().join("repo"), "main").unwrap();
    git(repo.path(), &["config", "user.name", "Inspection Fixture"]);
    git(
        repo.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    fs::write(repo.path().join("file.txt"), "original\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-m", "base"]);
    (temp, repo)
}
#[test]
fn endpoint_and_merge_base_directions_remain_passive_and_pinned() {
    let (_temp, repo) = fixture();
    let path = repo.path();
    let c = HistoryCancellation::default();
    let base = git(path, &["rev-parse", "HEAD"]);
    git(path, &["branch", "topic"]);
    fs::write(path.join("main.txt"), "main\n").unwrap();
    git(path, &["add", "."]);
    git(path, &["commit", "-m", "main"]);
    let main = git(path, &["rev-parse", "HEAD"]);
    git(path, &["checkout", "topic"]);
    git(path, &["mv", "file.txt", "renamed.txt"]);
    git(path, &["commit", "-m", "rename"]);
    git(path, &["tag", "tip"]);
    fs::write(path.join("untracked.txt"), "untouched\n").unwrap();
    let index = fs::read(path.join(".git/index")).unwrap();
    let endpoints = repo
        .compare_revisions("main", "tip", ComparisonMode::Endpoints, &c)
        .unwrap();
    assert_eq!(endpoints.base_oid, main);
    assert_eq!(endpoints.files.len(), 2);
    let branching = repo
        .compare_revisions("main", "tip", ComparisonMode::SinceBranching, &c)
        .unwrap();
    assert_eq!(branching.base_oid, base);
    assert_eq!(branching.files.len(), 1);
    assert_eq!(
        branching.files[0].old_path.as_deref(),
        Some(Path::new("file.txt"))
    );
    let reverse = repo
        .compare_revisions("tip", "main", ComparisonMode::Endpoints, &c)
        .unwrap();
    assert_eq!(reverse.files.len(), 2);
    assert_eq!(fs::read(path.join(".git/index")).unwrap(), index);
    assert_eq!(git(path, &["branch", "--show-current"]), "topic");
    git(path, &["branch", "-f", "main", "tip"]);
    assert_eq!(endpoints.before.oid, main);
}
#[test]
fn ambiguous_missing_and_unrelated_revisions_are_explicit() {
    let (_temp, repo) = fixture();
    let path = repo.path();
    let c = HistoryCancellation::default();
    git(path, &["tag", "main"]);
    assert!(
        repo.resolve_inspection_revision("main", &c)
            .unwrap_err()
            .to_string()
            .contains("Ambiguous")
    );
    assert!(
        repo.resolve_inspection_revision("refs/heads/main", &c)
            .is_ok()
    );
    assert!(repo.resolve_inspection_revision("--all", &c).is_err());
    assert!(repo.resolve_inspection_revision("missing", &c).is_err());
    git(path, &["checkout", "--orphan", "unrelated"]);
    git(path, &["commit", "-m", "unrelated root"]);
    assert!(
        repo.compare_revisions(
            "refs/heads/main",
            "unrelated",
            ComparisonMode::SinceBranching,
            &c
        )
        .is_err()
    );
    assert!(
        repo.compare_revisions(
            "refs/heads/main",
            "unrelated",
            ComparisonMode::Endpoints,
            &c
        )
        .is_ok()
    );
}
#[test]
fn tracked_search_supports_unicode_deletion_pinning_and_cancel() {
    let (_temp, repo) = fixture();
    let path = repo.path();
    let c = HistoryCancellation::default();
    fs::write(path.join("résumé 🐢.txt"), "unicode\n").unwrap();
    git(path, &["add", "."]);
    git(path, &["commit", "-m", "unicode"]);
    fs::remove_file(path.join("résumé 🐢.txt")).unwrap();
    fs::write(path.join("untracked.txt"), "not tracked\n").unwrap();
    let paths = repo
        .search_tracked_paths(&PathScope::Worktree, "RÉSUMÉ", &c)
        .unwrap();
    assert_eq!(paths.entries.len(), 1);
    assert!(!paths.truncated);
    assert!(repo.read_tracked_working_file(&paths.entries[0]).is_err());
    let revision = repo
        .search_tracked_paths(&PathScope::Revision("HEAD".into()), "", &c)
        .unwrap();
    assert_eq!(revision.entries.len(), 2);
    assert_eq!(
        revision.scope,
        PathScope::Revision(git(path, &["rev-parse", "HEAD"]))
    );
    c.cancel();
    assert!(
        repo.search_tracked_paths(&PathScope::Worktree, "", &c)
            .is_err()
    );
}
#[cfg(unix)]
#[test]
fn quick_open_retains_filename_bytes_and_never_follows_symlinks() {
    use std::os::unix::{ffi::OsStringExt, fs::symlink};
    let (_temp, repo) = fixture();
    let path = repo.path();
    let c = HistoryCancellation::default();
    let name = std::ffi::OsString::from_vec(b"raw-\xff.txt".to_vec());
    // APFS can refuse non-UTF-8 filesystem names; Git's index still stores
    // them. Manufacture the tracked record through Git's byte protocol.
    let oid = git(path, &["rev-parse", "HEAD:file.txt"]);
    let mut child = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["update-index", "-z", "--index-info"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    let mut input = format!("100644 {oid}\t").into_bytes();
    input.extend_from_slice(b"raw-\xff.txt\0");
    child.stdin.take().unwrap().write_all(&input).unwrap();
    assert!(child.wait().unwrap().success());
    symlink("/etc/passwd", path.join("link")).unwrap();
    git(path, &["add", "link"]);
    let paths = repo
        .search_tracked_paths(&PathScope::Worktree, "", &c)
        .unwrap();
    assert!(paths.entries.iter().any(|e| e.path == Path::new(&name)));
    let link = paths
        .entries
        .iter()
        .find(|e| e.path == Path::new("link"))
        .unwrap();
    assert_eq!(
        repo.read_tracked_working_file(link).unwrap(),
        ("120000".into(), b"/etc/passwd".to_vec())
    );
}

#[test]
fn tracked_path_index_and_index_fingerprint_match_and_invalidate() {
    let (_temp, repo) = fixture();
    let path = repo.path();
    let c = HistoryCancellation::default();
    fs::create_dir_all(path.join("src/nested")).unwrap();
    fs::write(path.join("src/nested/Résumé.rs"), "fn main() {}\n").unwrap();
    fs::write(path.join("src/App.rs"), "pub struct App;\n").unwrap();
    git(path, &["add", "."]);
    git(path, &["commit", "-m", "add files"]);

    let fp1 = repo.index_fingerprint().unwrap().expect("index exists");
    let fp1_again = repo.index_fingerprint().unwrap().expect("index exists");
    assert_eq!(fp1, fp1_again);

    let index = repo.load_tracked_paths(&PathScope::Worktree, &c).unwrap();
    assert_eq!(index.len(), 3);
    assert!(!index.is_empty());
    assert!(index.memory_bytes() > 0);
    assert!(!index.truncated());

    for query in ["", "APP", "résumé", "RÉSUMÉ", "missing"] {
        let expected = repo
            .search_tracked_paths(&PathScope::Worktree, query, &c)
            .unwrap();
        let actual = index.search(query, &c).unwrap();
        assert_eq!(actual.entries, expected.entries, "query={query:?}");
        assert_eq!(actual.truncated, expected.truncated, "query={query:?}");
        assert_eq!(actual.scope, expected.scope, "query={query:?}");
    }

    // Modifying `.git/index` via `git add` changes the fingerprint immediately.
    fs::write(path.join("src/App.rs"), "pub struct App2;\n").unwrap();
    git(path, &["add", "src/App.rs"]);
    let fp2 = repo.index_fingerprint().unwrap().expect("index exists");
    assert_ne!(fp1, fp2);

    // Revision-scoped load pins the resolved commit OID.
    let head_oid = git(path, &["rev-parse", "HEAD"]);
    let rev_index = repo
        .load_tracked_paths(&PathScope::Revision("HEAD".into()), &c)
        .unwrap();
    assert_eq!(rev_index.scope(), &PathScope::Revision(head_oid));
    assert_eq!(rev_index.len(), 3);
}
