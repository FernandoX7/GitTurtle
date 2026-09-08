use gitturtle_core::{
    ChangeStatus, GitRepository, HistoryCancellation, HistoryScope, HistorySearchStop,
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use tempfile::TempDir;

struct Fixture {
    _temp: TempDir,
    path: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("repo");
        GitRepository::init(&path, "main").unwrap();
        let this = Self { _temp: temp, path };
        this.git(&["config", "user.name", "Åda Fixture"]);
        this.git(&["config", "user.email", "fixture@example.invalid"]);
        this
    }
    fn command(&self) -> Command {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&self.path)
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
            ])
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null");
        command
    }
    fn git(&self, args: &[&str]) -> String {
        let out = self.command().args(args).output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim_end().to_owned()
    }
    fn repo(&self) -> GitRepository {
        GitRepository::open(&self.path).unwrap()
    }
    fn git_input(&self, args: &[&str], input: &[u8]) -> String {
        let mut child = self
            .command()
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_owned()
    }
    fn write(&self, name: &str, value: &str) {
        fs::write(self.path.join(name), value).unwrap();
    }
    fn commit(&self, message: &str) -> String {
        self.git(&["add", "--all"]);
        self.git(&["commit", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }
    fn import_history(&self, count: usize) {
        let mut input = Vec::new();
        for i in 0..count {
            let message = if i == 0 {
                "oldest literal [a.*b]\n\nDescription needle\n".to_owned()
            } else {
                format!("commit {i}\n")
            };
            input.extend_from_slice(format!("commit refs/heads/main\ncommitter Åda Fixture <fixture@example.invalid> {} +0000\ndata {}\n{}\n", 1_700_000_000+i, message.len(), message).as_bytes());
        }
        input.extend_from_slice(b"done\n");
        let mut child = self
            .command()
            .args(["fast-import", "--quiet"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&input).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn search_reaches_old_history_and_matches_message_author_and_hash_literally() {
    let fixture = Fixture::new();
    fixture.import_history(620);
    let repo = fixture.repo();
    let cancel = HistoryCancellation::default();
    let page = repo
        .search_history(&HistoryScope::AllRefs, "[A.*B]", 0, 20, &cancel)
        .unwrap();
    assert_eq!(page.commits.len(), 1);
    assert_eq!(page.scanned, 620);
    assert_eq!(page.stop, HistorySearchStop::Exhausted);
    assert_eq!(page.next_offset, None);
    let oldest = &page.commits[0];
    assert!(oldest.subject.contains("[a.*b]"));
    let scope = &page.scope;
    assert_eq!(
        repo.search_history(scope, "description NEEDLE", 0, 20, &cancel)
            .unwrap()
            .commits[0]
            .oid,
        oldest.oid
    );
    let author = repo
        .search_history(scope, "ÅDA fixture", 0, 2, &cancel)
        .unwrap();
    assert_eq!(author.commits.len(), 2);
    assert_eq!(author.next_offset, Some(2));
    assert_eq!(author.stop, HistorySearchStop::PageFull);
    assert_eq!(
        repo.search_history(scope, &oldest.oid[..12].to_uppercase(), 0, 20, &cancel)
            .unwrap()
            .commits[0]
            .oid,
        oldest.oid
    );
    assert!(
        repo.search_history(scope, "^oldest", 0, 20, &cancel)
            .unwrap()
            .commits
            .is_empty()
    );
}

#[test]
fn search_pages_pin_all_ref_tips_across_moves_and_support_a_single_ancestry() {
    let fixture = Fixture::new();
    fixture.import_history(5);
    let repo = fixture.repo();
    let cancel = HistoryCancellation::default();
    let baseline = repo
        .search_history(&HistoryScope::AllRefs, "", 0, 20, &cancel)
        .unwrap();
    let first = repo
        .search_history(&HistoryScope::AllRefs, "", 0, 2, &cancel)
        .unwrap();
    fixture.git(&["update-ref", "refs/heads/main", &baseline.commits[3].oid]);
    fixture.git(&["update-ref", "refs/heads/extra", &baseline.commits[4].oid]);
    let second = repo
        .search_history(&first.scope, "", first.next_offset.unwrap(), 20, &cancel)
        .unwrap();
    let combined = first
        .commits
        .iter()
        .chain(&second.commits)
        .map(|c| &c.oid)
        .collect::<Vec<_>>();
    assert_eq!(
        combined,
        baseline.commits.iter().map(|c| &c.oid).collect::<Vec<_>>()
    );
    let single = repo
        .search_history(
            &HistoryScope::FromCommit(baseline.commits[3].oid.clone()),
            "",
            0,
            20,
            &cancel,
        )
        .unwrap();
    assert_eq!(single.commits.len(), 2);
    assert_eq!(
        repo.search_history(&HistoryScope::AllRefs, "", 0, 20, &cancel)
            .unwrap()
            .commits
            .len(),
        2
    );
    assert!(
        repo.search_history(&HistoryScope::AllRefs, "", 2, 20, &cancel)
            .unwrap_err()
            .to_string()
            .contains("pinned scope")
    );
}

#[test]
fn all_refs_includes_detached_head_and_handles_unborn_repositories() {
    let fixture = Fixture::new();
    let repo = fixture.repo();
    let cancel = HistoryCancellation::default();
    let empty = repo
        .search_history(&HistoryScope::AllRefs, "", 0, 10, &cancel)
        .unwrap();
    assert!(empty.commits.is_empty());
    assert_eq!(empty.stop, HistorySearchStop::Exhausted);
    fixture.write("file", "base\n");
    fixture.commit("base");
    fixture.git(&["switch", "--detach"]);
    fixture.write("file", "detached\n");
    let detached = fixture.commit("detached");
    assert_eq!(
        repo.search_history(&HistoryScope::AllRefs, "detached", 0, 10, &cancel)
            .unwrap()
            .commits[0]
            .oid,
        detached
    );
}

#[test]
fn file_history_follows_renames_deletions_and_pages_with_exact_revision_paths() {
    let fixture = Fixture::new();
    fixture.write("old name", "one\ntwo\nthree\n");
    let root = fixture.commit("create");
    fixture.git(&["mv", "old name", "renamed"]);
    let renamed = fixture.commit("rename");
    fixture.write("unrelated", "unrelated\n");
    let before_delete = fixture.commit("unrelated");
    fixture.git(&["rm", "renamed"]);
    let deleted = fixture.commit("delete");
    let repo = fixture.repo();
    let cancel = HistoryCancellation::default();
    let all = repo
        .file_history(&deleted, Path::new("renamed"), 0, 20, &cancel)
        .unwrap();
    assert_eq!(
        all.entries
            .iter()
            .map(|e| &e.commit.oid)
            .collect::<Vec<_>>(),
        vec![&deleted, &renamed, &root]
    );
    assert_eq!(all.entries[0].change.status, ChangeStatus::Deleted);
    assert_eq!(all.entries[0].parent_oid.as_ref(), Some(&before_delete));
    assert_eq!(all.entries[0].change.new_path, None);
    assert_eq!(all.entries[0].change.new_oid, None);
    assert_eq!(
        all.entries[1].change.old_path.as_deref(),
        Some(Path::new("old name"))
    );
    assert_eq!(
        all.entries[1].change.new_path.as_deref(),
        Some(Path::new("renamed"))
    );
    assert_eq!(
        all.entries[2].change.new_path.as_deref(),
        Some(Path::new("old name"))
    );
    assert_eq!(
        repo.blob(all.entries[1].change.old_oid.as_ref().unwrap())
            .unwrap(),
        b"one\ntwo\nthree\n"
    );
    let first = repo
        .file_history(&deleted, Path::new("renamed"), 0, 1, &cancel)
        .unwrap();
    let rest = repo
        .file_history(
            &deleted,
            Path::new("renamed"),
            first.next_offset.unwrap(),
            20,
            &cancel,
        )
        .unwrap();
    assert_eq!(
        rest.entries
            .iter()
            .map(|e| &e.commit.oid)
            .collect::<Vec<_>>(),
        vec![&renamed, &root]
    );
}

#[test]
fn merge_file_history_uses_real_first_parent_and_keeps_all_parent_choices() {
    let fixture = Fixture::new();
    fixture.write("old", "one\ntwo\nthree\n");
    let root = fixture.commit("create");
    fixture.git(&["switch", "-c", "topic"]);
    fixture.git(&["mv", "old", "new"]);
    let topic = fixture.commit("topic rename");
    fixture.git(&["switch", "main"]);
    fixture.write("independent", "main work\n");
    let main = fixture.commit("main work");
    fixture.git(&["merge", "--no-ff", "--no-edit", "topic"]);
    let merge = fixture.git(&["rev-parse", "HEAD"]);
    let page = fixture
        .repo()
        .file_history(
            &merge,
            Path::new("new"),
            0,
            20,
            &HistoryCancellation::default(),
        )
        .unwrap();
    assert_eq!(
        page.entries
            .iter()
            .map(|e| &e.commit.oid)
            .collect::<Vec<_>>(),
        vec![&merge, &root]
    );
    assert_eq!(page.entries[0].parent_oid.as_ref(), Some(&main));
    assert_eq!(page.entries[0].commit.parents, vec![main, topic]);
    assert_eq!(page.entries[0].change.status, ChangeStatus::Renamed);
    assert_eq!(
        page.entries[0].change.old_path.as_deref(),
        Some(Path::new("old"))
    );
    assert_eq!(
        page.entries[0].change.new_path.as_deref(),
        Some(Path::new("new"))
    );
    let older = fixture
        .repo()
        .file_history(
            &merge,
            Path::new("new"),
            1,
            1,
            &HistoryCancellation::default(),
        )
        .unwrap();
    assert_eq!(older.entries.len(), 1);
    assert_eq!(older.entries[0].commit.oid, root);
    assert_eq!(
        older.entries[0].change.new_path.as_deref(),
        Some(Path::new("old"))
    );
}

#[test]
fn a_page_after_a_pure_rename_retains_history_under_the_previous_name() {
    let fixture = Fixture::new();
    fixture.write("alpha.txt", "unchanged stored content\n");
    let initial = fixture.commit("Initial");
    fixture.git(&["mv", "alpha.txt", "beta.txt"]);
    let renamed = fixture.commit("Rename");
    let repo = fixture.repo();
    let cancel = HistoryCancellation::default();
    let first = repo
        .file_history(&renamed, Path::new("beta.txt"), 0, 1, &cancel)
        .unwrap();
    assert_eq!(first.entries.len(), 1);
    assert_eq!(first.entries[0].commit.oid, renamed);
    assert_eq!(first.next_offset, Some(1));
    let second = repo
        .file_history(
            &renamed,
            Path::new("beta.txt"),
            first.next_offset.unwrap(),
            1,
            &cancel,
        )
        .unwrap();
    assert_eq!(second.entries.len(), 1);
    assert_eq!(second.entries[0].commit.oid, initial);
    assert_eq!(
        second.entries[0].change.new_path.as_deref(),
        Some(Path::new("alpha.txt"))
    );
    assert_eq!(second.next_offset, None);
}

#[cfg(unix)]
#[test]
fn file_history_is_byte_safe_literal_and_passive_with_hostile_helpers() {
    use std::os::unix::ffi::OsStringExt;
    let fixture = Fixture::new();
    let path = PathBuf::from(std::ffi::OsString::from_vec(
        b":(glob)odd\n\xff.txt".to_vec(),
    ));
    fixture.write(".gitattributes", "* diff=hostile filter=hostile\n");
    fixture.git(&["add", ".gitattributes"]);
    let blob = fixture.git_input(&["hash-object", "-w", "--stdin"], b"stored text\n");
    let mut index_entry = format!("100644 {blob}\t").into_bytes();
    index_entry.extend_from_slice(path.as_os_str().as_encoded_bytes());
    index_entry.push(0);
    fixture.git_input(&["update-index", "-z", "--index-info"], &index_entry);
    fixture.git(&["commit", "-m", "byte path"]);
    let oid = fixture.git(&["rev-parse", "HEAD"]);
    let marker = fixture.path.join("helper-ran");
    let hostile = format!("touch '{}'; cat", marker.display());
    for key in [
        "diff.hostile.command",
        "diff.hostile.textconv",
        "filter.hostile.clean",
        "filter.hostile.smudge",
        "core.fsmonitor",
    ] {
        fixture.git(&["config", key, &hostile]);
    }
    let index = fs::read(fixture.path.join(".git/index")).unwrap();
    let page = fixture
        .repo()
        .file_history(&oid, &path, 0, 20, &HistoryCancellation::default())
        .unwrap();
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].change.new_path.as_ref(), Some(&path));
    assert!(!marker.exists());
    assert_eq!(fs::read(fixture.path.join(".git/index")).unwrap(), index);
    assert_eq!(
        fixture
            .repo()
            .blob(page.entries[0].change.new_oid.as_ref().unwrap())
            .unwrap(),
        b"stored text\n"
    );
}

#[test]
fn all_ref_snapshot_accepts_annotated_tags_and_noncommit_references() {
    let fixture = Fixture::new();
    fixture.import_history(2);
    fixture.git(&["tag", "-a", "release", "-m", "release annotation"]);
    let blob = fixture.git_input(&["hash-object", "-w", "--stdin"], b"tagged blob\n");
    fixture.git(&["tag", "blob", &blob]);
    fixture.git(&[
        "tag",
        "-a",
        "annotated-blob",
        "-m",
        "blob annotation",
        &blob,
    ]);
    let tree = fixture.git(&["rev-parse", "HEAD^{tree}"]);
    fixture.git(&["tag", "tree", &tree]);
    let page = fixture
        .repo()
        .search_history(
            &HistoryScope::AllRefs,
            "",
            0,
            20,
            &HistoryCancellation::default(),
        )
        .unwrap();
    assert_eq!(page.commits.len(), 2);
    assert_eq!(page.stop, HistorySearchStop::Exhausted);
}

#[test]
fn bounds_invalid_revisions_and_cancelled_requests_fail_explicitly() {
    let fixture = Fixture::new();
    fixture.import_history(1);
    let repo = fixture.repo();
    let cancel = HistoryCancellation::default();
    assert!(
        repo.search_history(&HistoryScope::AllRefs, "", 0, 501, &cancel)
            .is_err()
    );
    assert!(
        repo.search_history(
            &HistoryScope::FromCommit("HEAD --all".into()),
            "",
            0,
            10,
            &cancel
        )
        .is_err()
    );
    assert!(
        repo.search_history(
            &HistoryScope::FromCommit("f".repeat(40)),
            "",
            0,
            10,
            &cancel
        )
        .is_err()
    );
    let oid = fixture.git(&["rev-parse", "HEAD"]);
    assert!(
        repo.file_history(&oid, Path::new("../escape"), 0, 10, &cancel)
            .is_err()
    );
    cancel.cancel();
    assert!(
        repo.search_history(&HistoryScope::AllRefs, "", 0, 10, &cancel)
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
    assert!(
        repo.file_history(&oid, Path::new("file"), 0, 10, &cancel)
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
}

#[test]
fn a_scan_budget_returns_continuation_before_claiming_an_empty_search() {
    let fixture = Fixture::new();
    fixture.import_history(50_005);
    let repo = fixture.repo();
    let cancel = HistoryCancellation::default();
    let first = repo
        .search_history(&HistoryScope::AllRefs, "oldest literal", 0, 20, &cancel)
        .unwrap();
    assert!(first.commits.is_empty());
    assert_eq!(first.stop, HistorySearchStop::ScanLimit);
    assert_eq!(first.next_offset, Some(50_000));
    let rest = repo
        .search_history(
            &first.scope,
            "oldest literal",
            first.next_offset.unwrap(),
            20,
            &cancel,
        )
        .unwrap();
    assert_eq!(rest.commits.len(), 1);
    assert_eq!(rest.scanned, 5);
    assert_eq!(rest.stop, HistorySearchStop::Exhausted);
    assert_eq!(rest.next_offset, None);
}
