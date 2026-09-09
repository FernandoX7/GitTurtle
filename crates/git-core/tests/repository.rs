use gitturtle_core::{ChangeStatus, GitRepository, TextPreview};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use tempfile::TempDir;

struct Fixture {
    temp: TempDir,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("repository");
        fs::create_dir(&root).unwrap();
        let fixture = Self { temp, root };
        fixture.git(&["init", "-b", "main"]);
        fixture.git(&["config", "user.name", "Fixture Author"]);
        fixture.git(&["config", "user.email", "fixture@example.invalid"]);
        fixture.git(&["config", "commit.gpgsign", "false"]);
        fixture
    }
    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
            ])
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim_end().into()
    }
    fn write(&self, path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    fn commit(&self, message: &str) -> String {
        self.git(&["add", "--all"]);
        self.git(&["commit", "--allow-empty", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }
    fn open(&self) -> GitRepository {
        GitRepository::open(&self.root).unwrap()
    }
    fn git_stdin(&self, args: &[&str], bytes: &[u8]) -> String {
        let mut child = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(bytes).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim_end().into()
    }
}

#[test]
fn empty_repository_and_nested_discovery() {
    let f = Fixture::new();
    let repo = f.open();
    assert!(repo.history(100).unwrap().is_empty());
    assert!(repo.branches().unwrap().is_empty());
    assert!(repo.history(0).unwrap().is_empty());
    assert_eq!(repo.worktrees().unwrap().len(), 1);
    let nested = f.root.join("nested/deeper");
    fs::create_dir_all(&nested).unwrap();
    assert_eq!(GitRepository::open(nested).unwrap().path(), repo.path());
    assert!(!repo.is_bare());
    assert!(GitRepository::open(f.temp.path()).is_err());
}

#[test]
fn branches_remote_aliases_and_worktree_states() {
    let f = Fixture::new();
    let oid = f.commit("initial");
    f.git(&["branch", "feature/redesign"]);
    f.git(&["update-ref", "refs/remotes/origin/main", &oid]);
    f.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);
    let temp_path = fs::canonicalize(f.temp.path()).unwrap();
    let linked = temp_path.join("linked tree\nwith newline");
    f.git(&[
        "worktree",
        "add",
        linked.to_str().unwrap(),
        "feature/redesign",
    ]);
    f.git(&[
        "worktree",
        "lock",
        "--reason",
        "fixture",
        linked.to_str().unwrap(),
    ]);
    let detached = temp_path.join("detached");
    f.git(&[
        "worktree",
        "add",
        "--detach",
        detached.to_str().unwrap(),
        &oid,
    ]);
    let removed = temp_path.join("removed");
    f.git(&[
        "worktree",
        "add",
        "--detach",
        removed.to_str().unwrap(),
        &oid,
    ]);
    fs::remove_dir_all(&removed).unwrap();
    let repo = f.open();
    let branches = repo.branches().unwrap();
    assert_eq!(branches.len(), 3);
    assert!(
        branches
            .iter()
            .any(|b| b.name == "main" && b.current && !b.remote)
    );
    assert!(
        branches
            .iter()
            .any(|b| b.name == "origin/main" && b.remote && !b.current)
    );
    let trees = repo.worktrees().unwrap();
    assert_eq!(trees.len(), 4);
    assert!(
        trees.iter().any(|t| t.path == linked
            && t.locked
            && t.branch.as_deref() == Some("feature/redesign"))
    );
    assert!(trees.iter().any(|t| t.path == detached && t.detached));
    assert!(trees.iter().any(|t| t.path == removed && t.prunable));
    let linked_repo = GitRepository::open(&linked).unwrap();
    assert!(
        linked_repo
            .branches()
            .unwrap()
            .iter()
            .any(|b| b.name == "feature/redesign" && b.current)
    );
    assert_eq!(linked_repo.history(10).unwrap(), repo.history(10).unwrap());
}

#[test]
fn root_and_modified_text_and_image_content() {
    let f = Fixture::new();
    f.write("hello.rs", b"fn main() {\n    println!(\"before\");\n}\n");
    let image = b"\x89PNG\r\n\x1a\n\0binary pixels";
    f.write("art/new.png", image);
    let root = f.commit("initial\n\nDetailed body.\nSecond line.");
    let repo = f.open();
    let root_files = repo.changes(&root, 0).unwrap();
    assert_eq!(root_files.len(), 2);
    assert!(
        root_files
            .iter()
            .all(|file| file.status == ChangeStatus::Added && file.old_oid.is_none())
    );
    let image_file = root_files
        .iter()
        .find(|f| f.path() == Path::new("art/new.png"))
        .unwrap();
    assert_eq!(
        repo.blob(image_file.new_oid.as_ref().unwrap()).unwrap(),
        image
    );
    assert_eq!(repo.text_preview(image_file).unwrap(), TextPreview::Binary);
    let text = root_files
        .iter()
        .find(|f| f.path() == Path::new("hello.rs"))
        .unwrap();
    let patch = repo.diff(text).unwrap();
    assert!(patch.contains("--- /dev/null\n+++ b/hello.rs"));
    assert!(patch.contains("+fn main()"));
    assert!(repo.changes(&root, 1).is_err());
    f.write("hello.rs", b"fn main() {\n    println!(\"after\");\n}\n");
    let second = f.commit("update");
    let changed = repo.changes(&second, 0).unwrap();
    assert_eq!(changed.len(), 1);
    let patch = repo.diff(&changed[0]).unwrap();
    assert!(patch.contains("-    println!(\"before\");"));
    assert!(patch.contains("+    println!(\"after\");"));
    let history = repo.history(20).unwrap();
    assert_eq!(history[0].oid, second);
    assert_eq!(history[1].oid, root);
    assert_eq!(history[1].body, "Detailed body.\nSecond line.");
    assert_eq!(history[1].author, "Fixture Author");
    assert!(history[1].parents.is_empty());
}

#[test]
fn merge_parent_selection_and_topological_history() {
    let f = Fixture::new();
    f.write("shared.txt", "base\n");
    let root = f.commit("base");
    f.git(&["checkout", "-b", "feature"]);
    f.write("feature.txt", "feature\n");
    let feature = f.commit("feature");
    f.git(&["checkout", "main"]);
    f.write("main.txt", "main\n");
    let main = f.commit("main");
    f.git(&["merge", "--no-ff", "feature", "-m", "merge"]);
    let merge = f.git(&["rev-parse", "HEAD"]);
    let repo = f.open();
    let first = repo.changes(&merge, 0).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].path(), Path::new("feature.txt"));
    let second = repo.changes(&merge, 1).unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].path(), Path::new("main.txt"));
    assert!(repo.changes(&merge, 2).is_err());
    let history = repo.history(100).unwrap();
    assert_eq!(history[0].parents, vec![main, feature.clone()]);
    assert_eq!(history.last().unwrap().oid, root);
    for (index, commit) in history.iter().enumerate() {
        for parent in &commit.parents {
            let parent_index = history.iter().position(|c| &c.oid == parent).unwrap();
            assert!(parent_index > index);
        }
    }
    let scoped = repo.history_from(&feature, 100).unwrap();
    assert_eq!(scoped.len(), 2);
    assert_eq!(scoped[0].oid, feature);
}

#[cfg(unix)]
#[test]
fn paths_preserve_non_utf8_tabs_newlines_and_renames() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let f = Fixture::new();
    let old = PathBuf::from(std::ffi::OsString::from_vec(
        b"- odd\tname\n\xff.txt".to_vec(),
    ));
    let new = PathBuf::from(std::ffi::OsString::from_vec(b"renamed\n\xfe.txt".to_vec()));
    // macOS filesystems cannot create these names, but Git trees can contain
    // them (e.g. a repository authored on Linux). Construct raw tree fixtures.
    let blob = f.git_stdin(&["hash-object", "-w", "--stdin"], b"some unique content\n");
    let tree_for = |path: &Path| {
        let mut entry = format!("100644 blob {blob}\t").into_bytes();
        entry.extend_from_slice(path.as_os_str().as_bytes());
        entry.push(0);
        f.git_stdin(&["mktree", "-z"], &entry)
    };
    let tree = tree_for(&old);
    let root = f.git(&["commit-tree", &tree, "-m", "unusual filename"]);
    f.git(&["update-ref", "refs/heads/main", &root]);
    let repo = f.open();
    let initial = repo.changes(&root, 0).unwrap();
    assert_eq!(initial[0].new_path.as_deref(), Some(old.as_path()));
    let tree = tree_for(&new);
    let renamed = f.git(&["commit-tree", &tree, "-p", &root, "-m", "rename"]);
    f.git(&["update-ref", "refs/heads/main", &renamed]);
    let fast = repo.changes(&renamed, 0).unwrap();
    assert_eq!(fast.len(), 2); // initial path does not do expensive similarity work
    let detected = repo.changes_with_renames(&renamed, 0).unwrap();
    assert_eq!(detected.len(), 1);
    assert_eq!(detected[0].status, ChangeStatus::Renamed);
    assert_eq!(detected[0].old_path.as_deref(), Some(old.as_path()));
    assert_eq!(detected[0].new_path.as_deref(), Some(new.as_path()));
    let tree = f.git_stdin(&["mktree", "-z"], b"");
    let deleted = f.git(&["commit-tree", &tree, "-p", &renamed, "-m", "delete"]);
    let deletion = repo.changes(&deleted, 0).unwrap();
    assert_eq!(deletion[0].status, ChangeStatus::Deleted);
    assert!(deletion[0].new_oid.is_none());
    assert!(repo.diff(&deletion[0]).unwrap().contains("+++ /dev/null"));
}

#[test]
fn missing_wrong_type_and_invalid_ids_do_not_poison_batch_reader() {
    let f = Fixture::new();
    f.write("file", "contents\n");
    let oid = f.commit("initial");
    let repo = f.open();
    let files = repo.changes(&oid, 0).unwrap();
    let blob = files[0].new_oid.as_deref().unwrap();
    assert!(repo.blob("--help").is_err());
    assert!(repo.blob(&format!("{blob}\n{blob}")).is_err());
    assert!(
        repo.blob(&"0".repeat(40))
            .unwrap_err()
            .to_string()
            .contains("not available locally")
    );
    assert_eq!(repo.blob(blob).unwrap(), b"contents\n");
    assert!(
        repo.blob(&oid)
            .unwrap_err()
            .to_string()
            .contains("not a file blob")
    );
    assert_eq!(repo.blob(blob).unwrap(), b"contents\n");
    assert!(repo.changes(blob, 0).is_err());
}

#[test]
fn large_and_non_utf8_files_have_explicit_preview_states() {
    let f = Fixture::new();
    f.write("large.txt", vec![b'x'; gitturtle_core::MAX_DIFF_BYTES + 1]);
    f.write("legacy.txt", b"legacy \xff encoding\n");
    f.write(
        "many-lines.txt",
        "x\n".repeat(gitturtle_core::MAX_DIFF_LINES + 1),
    );
    f.write(
        "asset.png",
        b"version https://git-lfs.github.com/spec/v1\noid sha256:0123456789abcdef\nsize 42\n",
    );
    let oid = f.commit("content limits");
    let repo = f.open();
    for file in repo.changes(&oid, 0).unwrap() {
        match file.path().to_str().unwrap() {
            "large.txt" | "many-lines.txt" => assert!(matches!(
                repo.text_preview(&file).unwrap(),
                TextPreview::TooLarge { .. }
            )),
            "legacy.txt" => assert_eq!(repo.text_preview(&file).unwrap(), TextPreview::Binary),
            "asset.png" => assert!(
                repo.blob(file.new_oid.as_deref().unwrap())
                    .unwrap()
                    .starts_with(b"version https://git-lfs")
            ),
            _ => unreachable!(),
        }
    }
}

#[cfg(unix)]
#[test]
fn modes_symlinks_and_submodule_commits_are_inspected_without_following() {
    use std::os::unix::{fs::PermissionsExt, fs::symlink};
    let f = Fixture::new();
    f.write("script", "#!/bin/sh\nexit 0\n");
    let root = f.commit("plain script");
    fs::set_permissions(f.root.join("script"), fs::Permissions::from_mode(0o755)).unwrap();
    let executable = f.commit("make executable");
    let repo = f.open();
    let mode = repo.changes(&executable, 0).unwrap();
    assert_eq!(mode[0].old_mode, "100644");
    assert_eq!(mode[0].new_mode, "100755");
    assert!(
        repo.diff(&mode[0])
            .unwrap()
            .contains("old mode 100644\nnew mode 100755")
    );
    fs::remove_file(f.root.join("script")).unwrap();
    symlink("/a/target/that/does/not/exist", f.root.join("script")).unwrap();
    let link = f.commit("replace with symlink");
    let changes = repo.changes(&link, 0).unwrap();
    assert_eq!(changes[0].status, ChangeStatus::TypeChanged);
    assert_eq!(changes[0].new_mode, "120000");
    assert_eq!(
        repo.blob(changes[0].new_oid.as_deref().unwrap()).unwrap(),
        b"/a/target/that/does/not/exist"
    );
    f.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{root},module"),
    ]);
    f.git(&["commit", "-m", "add gitlink"]);
    let submodule = f.git(&["rev-parse", "HEAD"]);
    let changes = repo.changes(&submodule, 0).unwrap();
    assert!(changes[0].is_submodule());
    assert!(matches!(
        repo.text_preview(&changes[0]).unwrap(),
        TextPreview::Submodule { .. }
    ));
    let blob = f.git(&["rev-parse", &format!("{root}:script")]);
    f.git(&[
        "update-index",
        "--cacheinfo",
        &format!("100644,{blob},module"),
    ]);
    f.git(&["commit", "-m", "replace gitlink with regular file"]);
    let regular = f.git(&["rev-parse", "HEAD"]);
    let changes = repo.changes(&regular, 0).unwrap();
    let sources = repo.text_preview_with_sources(&changes[0]).unwrap();
    assert!(matches!(sources.preview, TextPreview::Patch(_)));
    assert_eq!(
        sources.old,
        format!("Subproject commit {root}\n").as_bytes()
    );
    assert_eq!(sources.new, b"#!/bin/sh\nexit 0\n");
}

fn snapshot(path: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, path: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            let meta = fs::symlink_metadata(&path).unwrap();
            if meta.is_dir() {
                walk(root, &path, out);
            } else if meta.is_file() {
                out.insert(
                    path.strip_prefix(root).unwrap().into(),
                    fs::read(&path).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(path, path, &mut out);
    out
}

#[cfg(unix)]
#[test]
fn read_operations_leave_repository_unchanged_and_never_run_configured_helpers() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    f.write("example.txt", "before\n");
    f.write(".gitattributes", "*.txt diff=evil filter=evil\n");
    f.commit("initial");
    f.write("example.txt", "after\n");
    let oid = f.commit("update");
    let helper = f.temp.path().join("helper.sh");
    let marker = f.temp.path().join("helper-ran");
    fs::write(
        &helper,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
    )
    .unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
    for key in [
        "diff.external",
        "diff.evil.textconv",
        "filter.evil.smudge",
        "core.fsmonitor",
        "credential.helper",
    ] {
        f.git(&["config", key, helper.to_str().unwrap()]);
    }
    f.git(&["config", "core.hooksPath", helper.to_str().unwrap()]);
    f.write("untracked.txt", "must not be indexed\n");
    let before = snapshot(&f.root);
    let repo = f.open();
    repo.branches().unwrap();
    repo.worktrees().unwrap();
    repo.history(100).unwrap();
    let changes = repo.changes(&oid, 0).unwrap();
    repo.diff(&changes[0]).unwrap();
    repo.changes_with_renames(&oid, 0).unwrap();
    assert!(repo.blob(&"0".repeat(40)).is_err());
    drop(repo);
    assert!(!marker.exists(), "An external helper ran");
    assert_eq!(
        snapshot(&f.root),
        before,
        "A read operation changed repository files"
    );
}

#[test]
fn bare_repository_can_be_inspected() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("bare.git");
    let result = Command::new("git")
        .args(["init", "--bare"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(result.status.success());
    let repo = GitRepository::open(&path).unwrap();
    assert!(repo.is_bare());
    assert_eq!(repo.name(), "bare.git");
    assert!(repo.history(10).unwrap().is_empty());
    assert_eq!(repo.worktrees().unwrap().len(), 1);
}

#[test]
fn partial_clone_reads_do_not_fetch_missing_promisor_blobs() {
    let source = Fixture::new();
    source.write("remote-content.txt", "available only in the source\n");
    let commit = source.commit("promisor fixture");
    let blob = source.git(&["rev-parse", "HEAD:remote-content.txt"]);
    source.git(&["config", "uploadpack.allowFilter", "true"]);
    let clone_path = source.temp.path().join("partial");
    let output = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "clone",
            "--filter=blob:none",
            "--no-checkout",
        ])
        .arg(format!("file://{}", source.root.display()))
        .arg(&clone_path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let before = snapshot(&clone_path);
    let repo = GitRepository::open(&clone_path).unwrap();
    assert_eq!(repo.history(10).unwrap()[0].oid, commit);
    let changes = repo.changes(&commit, 0).unwrap();
    assert_eq!(changes[0].new_oid.as_deref(), Some(blob.as_str()));
    let unavailable = repo.blob(&blob).unwrap_err().to_string();
    assert!(
        unavailable.contains("not available locally"),
        "{unavailable}"
    );
    assert!(repo.blob_size(&blob).is_err());
    drop(repo);
    assert_eq!(
        snapshot(&clone_path),
        before,
        "Missing-object lookup changed the partial clone"
    );
}

#[test]
fn sha256_object_ids_are_supported() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("sha256");
    fs::create_dir(&root).unwrap();
    let f = Fixture { temp, root };
    f.git(&["init", "--object-format=sha256", "-b", "main"]);
    f.git(&["config", "user.name", "Fixture Author"]);
    f.git(&["config", "user.email", "fixture@example.invalid"]);
    f.git(&["config", "commit.gpgsign", "false"]);
    f.write("content", "sha256 content\n");
    let commit = f.commit("sha256");
    assert_eq!(commit.len(), 64);
    let repo = f.open();
    assert_eq!(repo.history(10).unwrap()[0].oid, commit);
    let changes = repo.changes(&commit, 0).unwrap();
    let blob = changes[0].new_oid.as_deref().unwrap();
    assert_eq!(blob.len(), 64);
    assert_eq!(repo.blob_size(blob).unwrap(), 15);
    assert_eq!(repo.blob(blob).unwrap(), b"sha256 content\n");
}

#[test]
fn history_paging_matches_full_order_and_diff_sources_are_reused() {
    let f = Fixture::new();
    f.write("text", "before\n");
    f.commit("first");
    f.write("text", "after\n");
    let anchor = f.commit("second");
    let repo = f.open();
    let full = repo.history(20).unwrap();
    assert_eq!(repo.history_page(0, 1).unwrap(), full[..1]);
    assert_eq!(repo.history_page(1, 1).unwrap(), full[1..]);
    assert!(repo.history_page(2, 1).unwrap().is_empty());
    assert_eq!(repo.history_from_page(&anchor, 1, 1).unwrap(), full[1..]);
    let change = repo.changes(&anchor, 0).unwrap().remove(0);
    let sources = repo.text_preview_with_sources(&change).unwrap();
    assert_eq!(sources.old, b"before\n");
    assert_eq!(sources.new, b"after\n");
    assert!(matches!(sources.preview, TextPreview::Patch(_)));
    f.commit("new tip");
    assert_eq!(repo.history_from_page(&anchor, 0, 20).unwrap(), full);
}

#[cfg(unix)]
#[test]
fn local_lfs_objects_are_verified_bounded_and_shared_across_worktrees() {
    use sha2::{Digest, Sha256};
    let f = Fixture::new();
    let commit = f.commit("initial");
    let bytes = b"locally available LFS media\n";
    let oid = format!("{:x}", Sha256::digest(bytes));
    let object = f
        .root
        .join(".git/lfs/objects")
        .join(&oid[..2])
        .join(&oid[2..4])
        .join(&oid);
    let repo = f.open();
    let before = snapshot(&f.root);
    assert_eq!(
        repo.local_lfs_object(&oid, bytes.len() as u64, 1024)
            .unwrap(),
        None
    );
    assert_eq!(
        snapshot(&f.root),
        before,
        "A missing lookup created storage directories"
    );
    fs::create_dir_all(object.parent().unwrap()).unwrap();
    fs::write(&object, bytes).unwrap();
    let before = snapshot(&f.root);
    assert_eq!(
        repo.local_lfs_object(&oid, bytes.len() as u64, 1024)
            .unwrap(),
        Some(bytes.to_vec())
    );
    assert_eq!(snapshot(&f.root), before);
    assert!(repo.local_lfs_object(&oid, bytes.len() as u64, 2).is_err());
    assert!(repo.local_lfs_object(&oid, 1, 1024).is_err());
    assert!(repo.local_lfs_object("../../etc/passwd", 1, 1024).is_err());
    let linked = f.temp.path().join("lfs linked");
    f.git(&[
        "worktree",
        "add",
        "--detach",
        linked.to_str().unwrap(),
        &commit,
    ]);
    let linked_repo = GitRepository::open(&linked).unwrap();
    assert_eq!(
        linked_repo
            .local_lfs_object(&oid, bytes.len() as u64, 1024)
            .unwrap(),
        Some(bytes.to_vec())
    );
    fs::write(&object, vec![b'x'; bytes.len()]).unwrap();
    assert!(
        repo.local_lfs_object(&oid, bytes.len() as u64, 1024)
            .unwrap_err()
            .to_string()
            .contains("SHA-256")
    );
    let empty_oid = format!("{:x}", Sha256::digest([]));
    assert_eq!(
        repo.local_lfs_object(&empty_oid, 0, 0).unwrap(),
        Some(vec![])
    );
}

#[cfg(unix)]
#[test]
fn local_lfs_honors_relative_custom_storage_and_rejects_symlinks() {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    f.commit("initial");
    f.git(&["config", "lfs.storage", "custom-media"]);
    let bytes = b"media";
    let oid = format!("{:x}", Sha256::digest(bytes));
    let storage = f.root.join(".git/custom-media");
    let object = storage
        .join("objects")
        .join(&oid[..2])
        .join(&oid[2..4])
        .join(&oid);
    fs::create_dir_all(object.parent().unwrap()).unwrap();
    fs::write(&object, bytes).unwrap();
    let repo = f.open();
    assert_eq!(
        repo.local_lfs_object(&oid, 5, 1024).unwrap(),
        Some(bytes.to_vec())
    );
    fs::remove_file(&object).unwrap();
    let external = f.temp.path().join("external");
    fs::write(&external, bytes).unwrap();
    symlink(&external, &object).unwrap();
    assert!(repo.local_lfs_object(&oid, 5, 1024).is_err());
    fs::remove_file(&object).unwrap();
    fs::remove_dir(object.parent().unwrap()).unwrap();
    let external_dir = f.temp.path().join("external-directory");
    fs::create_dir(&external_dir).unwrap();
    fs::write(external_dir.join(&oid), bytes).unwrap();
    symlink(&external_dir, object.parent().unwrap()).unwrap();
    assert!(repo.local_lfs_object(&oid, 5, 1024).is_err());
}
