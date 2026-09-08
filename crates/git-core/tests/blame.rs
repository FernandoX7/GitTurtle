use gitturtle_core::{BlameTarget, GitRepository, HistoryCancellation};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::TempDir;

struct Fixture(TempDir);
impl Fixture {
    fn new() -> Self {
        let f = Self(TempDir::new().unwrap());
        f.git(&["init", "-b", "main"]);
        f.git(&["config", "user.name", "Attribution Author"]);
        f.git(&["config", "user.email", "fixture@example.invalid"]);
        f.git(&["config", "commit.gpgsign", "false"]);
        f
    }
    fn path(&self) -> &Path {
        self.0.path()
    }
    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.path())
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
    fn write(&self, path: impl AsRef<Path>, text: impl AsRef<[u8]>) {
        fs::write(self.path().join(path), text).unwrap();
    }
    fn commit(&self, message: &str) -> String {
        self.git(&["add", "--all"]);
        self.git(&["commit", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }
    fn repo(&self) -> GitRepository {
        GitRepository::open(self.path()).unwrap()
    }
    fn git_input(&self, args: &[&str], input: &[u8]) -> String {
        use std::{io::Write, process::Stdio};
        let mut child = Command::new("git")
            .arg("-C")
            .arg(self.path())
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim_end().into()
    }
}
fn target(oid: &str, path: impl Into<PathBuf>) -> BlameTarget {
    BlameTarget::Committed {
        oid: oid.into(),
        path: path.into(),
    }
}

#[test]
fn committed_lines_keep_origin_rename_path_and_exact_source() {
    let f = Fixture::new();
    f.write("old.txt", "one\r\ntwo\r\nthree");
    let root = f.commit("Initial source");
    f.git(&["mv", "old.txt", "new.txt"]);
    f.commit("Rename file");
    f.write("new.txt", "one\r\nTWO\r\nthree");
    let latest = f.commit("Edit second line");
    let blame = f
        .repo()
        .blame(&target(&latest, "new.txt"), &HistoryCancellation::default())
        .unwrap();
    assert_eq!(blame.lines.len(), 3);
    assert_eq!(blame.lines[0].attribution.as_ref().unwrap().oid, root);
    assert_eq!(
        blame.lines[0].attribution.as_ref().unwrap().path,
        Path::new("old.txt")
    );
    assert_eq!(blame.lines[1].attribution.as_ref().unwrap().oid, latest);
    assert_eq!(
        blame
            .lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<String>(),
        "one\r\nTWO\r\nthree"
    );
}

#[test]
fn working_snapshot_labels_staged_and_unstaged_lines_without_mutation_or_filters() {
    let f = Fixture::new();
    f.write("file", "first\nsecond\nthird\n");
    let root = f.commit("Base");
    f.write("file", "first\nSTAGED\nthird\n");
    f.git(&["add", "file"]);
    f.write("file", "INSERTED\nfirst\nSTAGED\nTHIRD\n");
    f.write(".gitattributes", "file filter=trap diff=trap\n");
    f.git(&["config", "filter.trap.clean", "touch filter-ran; cat"]);
    f.git(&["config", "diff.trap.textconv", "touch textconv-ran"]);
    let index = fs::read(f.path().join(".git/index")).unwrap();
    let blame = f
        .repo()
        .blame(
            &BlameTarget::Working {
                path: "file".into(),
            },
            &HistoryCancellation::default(),
        )
        .unwrap();
    assert_eq!(blame.anchor.as_deref(), Some(root.as_str()));
    assert_eq!(
        blame
            .lines
            .iter()
            .map(|l| l.attribution.is_some())
            .collect::<Vec<_>>(),
        vec![false, true, false, false]
    );
    assert_eq!(blame.lines[1].original_line, 1);
    assert_eq!(fs::read(f.path().join(".git/index")).unwrap(), index);
    assert_eq!(
        fs::read_to_string(f.path().join("file")).unwrap(),
        "INSERTED\nfirst\nSTAGED\nTHIRD\n"
    );
    assert!(!f.path().join("filter-ran").exists());
    assert!(!f.path().join("textconv-ran").exists());
}

#[test]
fn new_and_unborn_files_have_uncommitted_attribution() {
    let f = Fixture::new();
    f.write("new", "new line\n");
    let blame = f
        .repo()
        .blame(
            &BlameTarget::Working { path: "new".into() },
            &HistoryCancellation::default(),
        )
        .unwrap();
    assert!(blame.anchor.is_none());
    assert!(blame.lines[0].attribution.is_none());
    f.write("other", "committed\n");
    f.git(&["add", "other"]);
    f.git(&["commit", "-m", "Other file"]);
    let blame = f
        .repo()
        .blame(
            &BlameTarget::Working { path: "new".into() },
            &HistoryCancellation::default(),
        )
        .unwrap();
    assert!(blame.anchor.is_some());
    assert!(blame.lines[0].attribution.is_none());
}

#[test]
fn working_staged_rename_maps_original_path() {
    let f = Fixture::new();
    f.write("old", "preserved\n");
    let root = f.commit("Base");
    f.git(&["mv", "old", "new"]);
    f.write("new", "preserved\nnew line\n");
    let blame = f
        .repo()
        .blame(
            &BlameTarget::Working { path: "new".into() },
            &HistoryCancellation::default(),
        )
        .unwrap();
    assert_eq!(blame.lines[0].attribution.as_ref().unwrap().oid, root);
    assert!(blame.lines[1].attribution.is_none());
}

#[test]
fn line_history_filters_other_lines_and_follows_rename() {
    let f = Fixture::new();
    f.write("old", "first\nsecond\n");
    let root = f.commit("Base");
    f.write("old", "FIRST\nsecond\n");
    f.commit("Other line");
    f.git(&["mv", "old", "new"]);
    f.commit("Rename");
    f.write("new", "FIRST\nSECOND\n");
    let last = f.commit("Selected line");
    let history = f
        .repo()
        .line_history(&last, Path::new("new"), 2, &HistoryCancellation::default())
        .unwrap();
    assert!(!history.truncated);
    let subjects: Vec<_> = history.commits.iter().map(|c| c.subject.as_str()).collect();
    assert!(!subjects.contains(&"Other line"));
    assert_eq!(history.commits.first().unwrap().oid, last);
    assert_eq!(history.commits.last().unwrap().oid, root);
}

#[cfg(unix)]
#[test]
fn literal_non_utf8_quoted_paths_survive_attribution() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let f = Fixture::new();
    let path = PathBuf::from(OsString::from_vec(b"-odd:\n\t\xff\"path".to_vec()));
    // APFS rejects invalid UTF-8 working filenames; Git trees retain these
    // bytes on every platform. Build raw objects without a filesystem path.
    let blob = f.git_input(&["hash-object", "-w", "--stdin"], b"text\n");
    let mut entry = format!("100644 blob {blob}\t").into_bytes();
    use std::os::unix::ffi::OsStrExt;
    entry.extend_from_slice(path.as_os_str().as_bytes());
    entry.push(0);
    let tree = f.git_input(&["mktree", "-z"], &entry);
    let root = f.git(&["commit-tree", &tree, "-m", "Odd path"]);
    let blame = f
        .repo()
        .blame(
            &target(&root, path.clone()),
            &HistoryCancellation::default(),
        )
        .unwrap();
    assert_eq!(blame.lines[0].attribution.as_ref().unwrap().path, path);
    let history = f
        .repo()
        .line_history(&root, &path, 1, &HistoryCancellation::default())
        .unwrap();
    assert_eq!(history.commits[0].oid, root);
}

#[test]
fn cancellation_limits_binary_and_absent_files_are_explicit() {
    let f = Fixture::new();
    f.write("file", "text\n");
    let root = f.commit("Base");
    let cancel = HistoryCancellation::default();
    cancel.cancel();
    assert!(
        f.repo()
            .blame(&target(&root, "file"), &cancel)
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
    f.write("file", b"binary\0text");
    assert!(
        f.repo()
            .blame(
                &BlameTarget::Working {
                    path: "file".into()
                },
                &HistoryCancellation::default()
            )
            .unwrap_err()
            .to_string()
            .contains("binary")
    );
    f.write("file", vec![b'a'; 2 * 1024 * 1024 + 1]);
    assert!(
        f.repo()
            .blame(
                &BlameTarget::Working {
                    path: "file".into()
                },
                &HistoryCancellation::default()
            )
            .unwrap_err()
            .to_string()
            .contains("2 MiB")
    );
    assert!(
        f.repo()
            .blame(&target(&root, "absent"), &HistoryCancellation::default())
            .unwrap_err()
            .to_string()
            .contains("absent")
    );
    assert!(
        f.repo()
            .blame(&target(&root, "../file"), &HistoryCancellation::default())
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn symlinks_and_symlink_directories_are_never_followed() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    f.write("file", "secret\n");
    symlink("file", f.path().join("link")).unwrap();
    let root = f.commit("Link");
    assert!(
        f.repo()
            .blame(&target(&root, "link"), &HistoryCancellation::default())
            .is_err()
    );
    assert!(
        f.repo()
            .blame(
                &BlameTarget::Working {
                    path: "link".into()
                },
                &HistoryCancellation::default()
            )
            .is_err()
    );
    symlink(f.path(), f.path().join("directory-link")).unwrap();
    assert!(
        f.repo()
            .blame(
                &BlameTarget::Working {
                    path: "directory-link/file".into()
                },
                &HistoryCancellation::default()
            )
            .is_err()
    );
}

#[test]
fn shallow_attribution_exposes_the_local_history_boundary() {
    let f = Fixture::new();
    f.write("file", "first\nsecond\n");
    f.commit("Base");
    f.write("file", "first\nSECOND\n");
    let latest = f.commit("Later");
    let clone = TempDir::new().unwrap();
    let output = Command::new("git")
        .args(["clone", "--depth=1", "--no-local"])
        .arg(f.path())
        .arg(clone.path().join("shallow"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let repo = GitRepository::open(clone.path().join("shallow")).unwrap();
    let blame = repo
        .blame(&target(&latest, "file"), &HistoryCancellation::default())
        .unwrap();
    assert!(blame.shallow);
    assert!(
        blame
            .lines
            .iter()
            .all(|line| line.attribution.as_ref().unwrap().oid == latest)
    );
}
