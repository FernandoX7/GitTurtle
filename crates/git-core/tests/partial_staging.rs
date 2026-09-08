use gitturtle_core::{
    ChangeArea, GitRepository, PartialDiff, PartialLineKind, PartialSelection, WorktreePreview,
    WriteCommand,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
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
        fixture.git(&["config", "user.name", "Partial Fixture"]);
        fixture.git(&["config", "user.email", "partial@example.invalid"]);
        fixture.git(&["config", "commit.gpgsign", "false"]);
        fixture.git(&["config", "core.hooksPath", ".git/hooks"]);
        fixture.git(&["config", "core.autocrlf", "false"]);
        fixture
    }
    fn git(&self, args: &[&str]) -> Vec<u8> {
        let result = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        result.stdout
    }
    fn repo(&self) -> GitRepository {
        GitRepository::open(&self.root).unwrap()
    }
    fn write(&self, path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
        fs::write(self.root.join(path), bytes).unwrap();
    }
    fn commit(&self) {
        self.repo().execute(&WriteCommand::StageAll).unwrap();
        self.repo()
            .execute(&WriteCommand::Commit {
                message: "Fixture baseline".into(),
            })
            .unwrap();
    }
    fn preview(&self, path: impl AsRef<Path>, area: ChangeArea) -> WorktreePreview {
        let repo = self.repo();
        let status = repo.status().unwrap();
        let entry = status
            .entries
            .iter()
            .find(|entry| entry.path == path.as_ref())
            .unwrap();
        repo.worktree_preview(entry, area).unwrap()
    }
    fn partial(&self, path: impl AsRef<Path>, area: ChangeArea) -> PartialDiff {
        let preview = self.preview(path, area);
        preview
            .partial
            .unwrap_or_else(|| panic!("{}", preview.partial_unavailable.unwrap_or_default()))
    }
    fn apply(&self, diff: PartialDiff, selection: PartialSelection) {
        self.repo()
            .execute(&WriteCommand::ApplyPartial {
                diff: std::sync::Arc::new(diff),
                selection,
            })
            .unwrap();
    }
    fn index(&self, path: &str) -> Vec<u8> {
        self.git(&["show", &format!(":{path}")])
    }
}

fn lines() -> String {
    (1..=30).map(|index| format!("line {index:02}\n")).collect()
}
fn changed_id(diff: &PartialDiff, kind: PartialLineKind, text: &str) -> usize {
    diff.hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .find(|line| line.kind == kind && line.text == text)
        .unwrap()
        .change_id
        .unwrap()
}

#[test]
fn hunk_stage_and_unstage_preserve_both_areas_and_unrelated_index_changes() {
    let f = Fixture::new();
    let baseline = lines();
    f.write("file.txt", &baseline);
    f.write("other.txt", "original\n");
    f.commit();
    let staged = baseline.replace("line 01\n", "already staged\n");
    f.write("file.txt", &staged);
    f.repo()
        .execute(&WriteCommand::Stage {
            paths: vec!["file.txt".into()],
        })
        .unwrap();
    let working = staged
        .replace("line 10\n", "first selection\n")
        .replace("line 25\n", "later selection\n");
    f.write("file.txt", &working);
    let diff = f.partial("file.txt", ChangeArea::Unstaged);
    assert_eq!(diff.hunks.len(), 2);
    // An unrelated staged edit after the displayed snapshot remains valid.
    f.write("other.txt", "unrelated staged\n");
    f.repo()
        .execute(&WriteCommand::Stage {
            paths: vec!["other.txt".into()],
        })
        .unwrap();
    f.write("other.txt", "unrelated unstaged\n");
    f.apply(diff, PartialSelection::Hunks(vec![0]));
    let selected = staged.replace("line 10\n", "first selection\n");
    assert_eq!(f.index("file.txt"), selected.as_bytes());
    assert_eq!(f.index("other.txt"), b"unrelated staged\n");
    assert_eq!(
        fs::read(f.root.join("file.txt")).unwrap(),
        working.as_bytes()
    );
    let diff = f.partial("file.txt", ChangeArea::Staged);
    assert_eq!(diff.hunks.len(), 2);
    f.apply(diff, PartialSelection::Hunks(vec![1]));
    assert_eq!(f.index("file.txt"), staged.as_bytes());
    assert_eq!(
        fs::read(f.root.join("other.txt")).unwrap(),
        b"unrelated unstaged\n"
    );
    assert_eq!(f.git(&["show", "HEAD:file.txt"]), baseline.as_bytes());
}

#[test]
fn selected_replacement_and_individual_additions_and_deletions_roundtrip() {
    let f = Fixture::new();
    f.write("file.txt", "first\nold A\nold B\nlast\n");
    f.commit();
    let working = b"first\nnew A\nnew B\nlast\n";
    f.write("file.txt", working);
    let diff = f.partial("file.txt", ChangeArea::Unstaged);
    let ids = vec![
        changed_id(&diff, PartialLineKind::Deletion, "old A\n"),
        changed_id(&diff, PartialLineKind::Addition, "new A\n"),
    ];
    f.apply(diff, PartialSelection::Lines(ids));
    assert_eq!(f.index("file.txt"), b"first\nold B\nnew A\nlast\n");
    let diff = f.partial("file.txt", ChangeArea::Staged);
    let id = changed_id(&diff, PartialLineKind::Addition, "new A\n");
    f.apply(diff, PartialSelection::Lines(vec![id]));
    assert_eq!(f.index("file.txt"), b"first\nold B\nlast\n");
    let diff = f.partial("file.txt", ChangeArea::Staged);
    let id = changed_id(&diff, PartialLineKind::Deletion, "old A\n");
    f.apply(diff, PartialSelection::Lines(vec![id]));
    assert_eq!(f.index("file.txt"), b"first\nold A\nold B\nlast\n");
    assert_eq!(fs::read(f.root.join("file.txt")).unwrap(), working);
}

#[test]
fn unborn_file_stages_selected_lines_and_unstages_without_deleting_work() {
    let f = Fixture::new();
    f.write("new.txt", "first\nsecond\nthird");
    let diff = f.partial("new.txt", ChangeArea::Unstaged);
    let id = changed_id(&diff, PartialLineKind::Addition, "second\n");
    f.apply(diff, PartialSelection::Lines(vec![id]));
    assert_eq!(f.index("new.txt"), b"second\n");
    let diff = f.partial("new.txt", ChangeArea::Staged);
    f.apply(diff, PartialSelection::Hunks(vec![0]));
    assert!(f.git(&["ls-files"]).is_empty());
    assert_eq!(
        fs::read(f.root.join("new.txt")).unwrap(),
        b"first\nsecond\nthird"
    );
}

#[test]
fn deletion_can_be_staged_partially_then_fully_and_unstaged_partially() {
    let f = Fixture::new();
    f.write("file.txt", "first\nsecond\nthird\n");
    f.commit();
    fs::remove_file(f.root.join("file.txt")).unwrap();
    let diff = f.partial("file.txt", ChangeArea::Unstaged);
    let id = changed_id(&diff, PartialLineKind::Deletion, "second\n");
    f.apply(diff, PartialSelection::Lines(vec![id]));
    assert_eq!(f.index("file.txt"), b"first\nthird\n");
    let diff = f.partial("file.txt", ChangeArea::Unstaged);
    f.apply(diff, PartialSelection::Hunks(vec![0]));
    assert!(f.git(&["ls-files"]).is_empty());
    let diff = f.partial("file.txt", ChangeArea::Staged);
    let id = changed_id(&diff, PartialLineKind::Deletion, "second\n");
    f.apply(diff, PartialSelection::Lines(vec![id]));
    assert_eq!(f.index("file.txt"), b"second\n");
    assert!(!f.root.join("file.txt").exists());
}

#[test]
fn stale_worktree_index_and_head_are_refused_without_overwriting() {
    for change in ["worktree", "index", "head"] {
        let f = Fixture::new();
        f.write("file.txt", "baseline\n");
        f.commit();
        f.write("file.txt", "selected\n");
        if change == "head" {
            f.repo().execute(&WriteCommand::StageAll).unwrap();
        }
        let area = if change == "head" {
            ChangeArea::Staged
        } else {
            ChangeArea::Unstaged
        };
        let diff = f.partial("file.txt", area);
        f.write("file.txt", "later work\n");
        if change == "index" {
            f.repo().execute(&WriteCommand::StageAll).unwrap();
        }
        if change == "head" {
            f.repo()
                .execute(&WriteCommand::Commit {
                    message: "External commit".into(),
                })
                .unwrap();
        }
        let before = fs::read(f.root.join(".git/index")).unwrap();
        assert!(
            f.repo()
                .execute(&WriteCommand::ApplyPartial {
                    diff: std::sync::Arc::new(diff),
                    selection: PartialSelection::Hunks(vec![0])
                })
                .is_err()
        );
        assert_eq!(fs::read(f.root.join(".git/index")).unwrap(), before);
        assert_eq!(fs::read(f.root.join("file.txt")).unwrap(), b"later work\n");
        assert!(!f.root.join(".git/index.lock").exists());
    }
}

#[test]
fn wrong_repository_invalid_selection_and_existing_lock_do_not_mutate() {
    let f = Fixture::new();
    f.write("file.txt", "baseline\n");
    f.commit();
    f.write("file.txt", "selected\n");
    let diff = f.partial("file.txt", ChangeArea::Unstaged);
    let before = fs::read(f.root.join(".git/index")).unwrap();
    for selection in [
        PartialSelection::Lines(vec![]),
        PartialSelection::Lines(vec![usize::MAX]),
        PartialSelection::Hunks(vec![10]),
    ] {
        assert!(
            f.repo()
                .execute(&WriteCommand::ApplyPartial {
                    diff: std::sync::Arc::new(diff.clone()),
                    selection
                })
                .is_err()
        );
    }
    let other = Fixture::new();
    assert!(
        other
            .repo()
            .execute(&WriteCommand::ApplyPartial {
                diff: std::sync::Arc::new(diff.clone()),
                selection: PartialSelection::Hunks(vec![0])
            })
            .is_err()
    );
    f.write(".git/index.lock", b"other operation");
    let error = f
        .repo()
        .execute(&WriteCommand::ApplyPartial {
            diff: std::sync::Arc::new(diff),
            selection: PartialSelection::Hunks(vec![0]),
        })
        .unwrap_err();
    assert!(error.to_string().contains("locked"));
    assert_eq!(
        fs::read(f.root.join(".git/index.lock")).unwrap(),
        b"other operation"
    );
    assert_eq!(fs::read(f.root.join(".git/index")).unwrap(), before);
}

#[test]
fn missing_final_newline_roundtrips_and_ambiguous_subset_is_refused() {
    let f = Fixture::new();
    f.write("file.txt", "before");
    f.commit();
    f.write("file.txt", "after");
    let diff = f.partial("file.txt", ChangeArea::Unstaged);
    let added = changed_id(&diff, PartialLineKind::Addition, "after");
    assert!(
        f.repo()
            .execute(&WriteCommand::ApplyPartial {
                diff: std::sync::Arc::new(diff.clone()),
                selection: PartialSelection::Lines(vec![added])
            })
            .unwrap_err()
            .to_string()
            .contains("final newline")
    );
    assert_eq!(f.index("file.txt"), b"before");
    f.apply(diff, PartialSelection::Hunks(vec![0]));
    assert_eq!(f.index("file.txt"), b"after");
    let diff = f.partial("file.txt", ChangeArea::Staged);
    f.apply(diff, PartialSelection::Hunks(vec![0]));
    assert_eq!(f.index("file.txt"), b"before");
    assert_eq!(fs::read(f.root.join("file.txt")).unwrap(), b"after");
}

#[test]
fn crlf_bytes_remain_exact_without_normalization_and_fallback_when_enabled() {
    let f = Fixture::new();
    f.write("file.txt", "first\r\nlast\r\n");
    f.commit();
    f.write("file.txt", "first\r\nadded\r\nlast\r\n");
    let diff = f.partial("file.txt", ChangeArea::Unstaged);
    f.apply(diff, PartialSelection::Hunks(vec![0]));
    assert_eq!(f.index("file.txt"), b"first\r\nadded\r\nlast\r\n");
    f.git(&["config", "core.autocrlf", "true"]);
    f.write("file.txt", "first\r\nadded\r\nanother\r\nlast\r\n");
    let preview = f.preview("file.txt", ChangeArea::Unstaged);
    assert!(preview.partial.is_none());
    assert!(
        preview
            .partial_unavailable
            .unwrap()
            .contains("line endings")
    );
}

#[test]
fn binary_attributes_filters_renames_and_modes_offer_whole_file_fallback() {
    let f = Fixture::new();
    for path in [
        "binary",
        "filtered",
        "encoded",
        "declared-binary",
        "rename",
        "mode",
    ] {
        f.write(path, "baseline\n");
    }
    f.commit();
    f.write("binary", b"changed\0binary");
    for path in ["filtered", "encoded", "declared-binary", "mode"] {
        f.write(path, "changed\n");
    }
    f.write(
        ".gitattributes",
        "filtered filter=fixture\nencoded working-tree-encoding=UTF-16\ndeclared-binary -diff\n",
    );
    f.git(&["config", "filter.fixture.clean", "touch filter-ran; cat"]);
    f.git(&["config", "filter.fixture.required", "true"]);
    for path in ["binary", "filtered", "encoded", "declared-binary"] {
        let preview = f.preview(path, ChangeArea::Unstaged);
        assert!(preview.partial.is_none(), "{path}");
        assert!(preview.partial_unavailable.is_some());
    }
    assert!(
        !f.root.join("filter-ran").exists(),
        "passive preview executed a filter"
    );
    f.git(&["mv", "rename", "renamed"]);
    assert!(f.preview("renamed", ChangeArea::Staged).partial.is_none());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(f.root.join("mode"), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(f.preview("mode", ChangeArea::Unstaged).partial.is_none());
        std::os::unix::fs::symlink("mode", f.root.join("symlink")).unwrap();
        assert!(f.preview("symlink", ChangeArea::Unstaged).partial.is_none());
    }
    f.repo()
        .execute(&WriteCommand::Stage {
            paths: vec!["filtered".into()],
        })
        .unwrap();
    assert!(
        f.root.join("filter-ran").exists(),
        "whole-file write bypassed configured filter"
    );
}

#[test]
fn changed_attributes_refuse_stale_partial_without_running_filters() {
    let f = Fixture::new();
    f.write("file.txt", "baseline\n");
    f.commit();
    f.write("file.txt", "selected\n");
    let diff = f.partial("file.txt", ChangeArea::Unstaged);
    f.write(".gitattributes", "file.txt filter=fixture\n");
    f.git(&["config", "filter.fixture.clean", "touch filter-ran; cat"]);
    assert!(
        f.repo()
            .execute(&WriteCommand::ApplyPartial {
                diff: std::sync::Arc::new(diff),
                selection: PartialSelection::Hunks(vec![0])
            })
            .is_err()
    );
    assert_eq!(f.index("file.txt"), b"baseline\n");
    assert!(!f.root.join("filter-ran").exists());
}

#[test]
fn literal_newline_tab_and_option_paths_are_updated_without_pathspec_expansion() {
    let f = Fixture::new();
    let paths: Vec<PathBuf> = vec!["-option".into(), "[ab]*".into(), "line\nbreak\tname".into()];
    #[cfg(target_os = "linux")]
    let paths = {
        use std::os::unix::ffi::OsStringExt;
        let mut paths = paths;
        paths.push(std::ffi::OsString::from_vec(b"non-\xff".to_vec()).into());
        paths
    };
    for path in &paths {
        f.write(path, "baseline\n");
    }
    f.commit();
    for path in &paths {
        f.write(path, "baseline\nselected\n");
    }
    for path in &paths {
        let diff = f.partial(path, ChangeArea::Unstaged);
        f.apply(diff, PartialSelection::Hunks(vec![0]));
        assert_eq!(
            f.preview(path, ChangeArea::Staged).new,
            b"baseline\nselected\n"
        );
        let diff = f.partial(path, ChangeArea::Staged);
        f.apply(diff, PartialSelection::Hunks(vec![0]));
        assert_eq!(f.preview(path, ChangeArea::Unstaged).old, b"baseline\n");
    }
}

#[test]
fn linked_worktree_partial_staging_updates_only_its_private_index() {
    let f = Fixture::new();
    f.write("file.txt", "baseline\n");
    f.commit();
    f.git(&["worktree", "add", "-b", "linked", "../linked"]);
    let linked = f.root.parent().unwrap().join("linked");
    fs::write(linked.join("file.txt"), "baseline\nlinked addition\n").unwrap();
    let repository = GitRepository::open(&linked).unwrap();
    let status = repository.status().unwrap();
    let diff = repository
        .worktree_preview(&status.entries[0], ChangeArea::Unstaged)
        .unwrap()
        .partial
        .unwrap();
    repository
        .execute(&WriteCommand::ApplyPartial {
            diff: std::sync::Arc::new(diff),
            selection: PartialSelection::Hunks(vec![0]),
        })
        .unwrap();
    assert_eq!(f.index("file.txt"), b"baseline\n");
    assert!(f.repo().status().unwrap().entries.is_empty());
    let status = repository.status().unwrap();
    assert_eq!(
        repository
            .worktree_preview(&status.entries[0], ChangeArea::Staged)
            .unwrap()
            .new,
        b"baseline\nlinked addition\n"
    );
}

#[test]
fn commit_message_preserves_description_whitespace_and_comment_lines() {
    let f = Fixture::new();
    f.write("file.txt", "baseline\n");
    f.repo().execute(&WriteCommand::StageAll).unwrap();
    let message = "A conventional title\n\nParagraph with trailing spaces.  \n\n    indented code\n# literal description\n\n";
    f.repo()
        .execute(&WriteCommand::Commit {
            message: message.into(),
        })
        .unwrap();
    let commit = f.git(&["cat-file", "commit", "HEAD"]);
    let body = commit.windows(2).position(|pair| pair == b"\n\n").unwrap() + 2;
    assert_eq!(&commit[body..], message.as_bytes());
}

#[test]
fn split_index_and_version_four_preserve_unrelated_entries() {
    let f = Fixture::new();
    for index in 0..30 {
        f.write(format!("similar-prefix-{index:02}.txt"), "baseline\n");
    }
    f.commit();
    f.git(&["update-index", "--index-version=4", "--split-index"]);
    f.write("similar-prefix-15.txt", "baseline\nselected\n");
    let before = f.git(&["ls-files", "--stage", "-z"]);
    let diff = f.partial("similar-prefix-15.txt", ChangeArea::Unstaged);
    f.apply(diff, PartialSelection::Hunks(vec![0]));
    assert_eq!(f.index("similar-prefix-15.txt"), b"baseline\nselected\n");
    let diff = f.partial("similar-prefix-15.txt", ChangeArea::Staged);
    f.apply(diff, PartialSelection::Hunks(vec![0]));
    assert_eq!(f.git(&["ls-files", "--stage", "-z"]), before);
    assert_eq!(
        fs::read(f.root.join("similar-prefix-15.txt")).unwrap(),
        b"baseline\nselected\n"
    );
}

#[test]
fn sha256_index_handles_partial_addition_and_removal() {
    let mut f = Fixture::new();
    f.git(&[
        "init",
        "--initial-branch=main",
        "--object-format=sha256",
        "../sha256",
    ]);
    f.root = f.root.parent().unwrap().join("sha256");
    f.git(&["config", "core.autocrlf", "false"]);
    f.write("new.txt", "first\nsecond\n");
    let diff = f.partial("new.txt", ChangeArea::Unstaged);
    let id = changed_id(&diff, PartialLineKind::Addition, "second\n");
    f.apply(diff, PartialSelection::Lines(vec![id]));
    assert_eq!(f.index("new.txt"), b"second\n");
    let diff = f.partial("new.txt", ChangeArea::Staged);
    f.apply(diff, PartialSelection::Hunks(vec![0]));
    assert!(f.git(&["ls-files"]).is_empty());
    assert_eq!(
        fs::read(f.root.join("new.txt")).unwrap(),
        b"first\nsecond\n"
    );
}

#[test]
fn passive_partial_reads_preserve_index_and_skip_configured_helpers() {
    let f = Fixture::new();
    f.write("file.txt", "baseline\n");
    f.commit();
    f.write("file.txt", "baseline\nselected\n");
    f.write(".gitattributes", "file.txt diff=fixture\n");
    f.git(&["config", "diff.fixture.textconv", "touch textconv-ran"]);
    f.git(&["config", "diff.external", "touch diff-ran"]);
    f.git(&["config", "core.fsmonitor", "touch fsmonitor-ran"]);
    let bytes = fs::read(f.root.join(".git/index")).unwrap();
    let modified = fs::metadata(f.root.join(".git/index"))
        .unwrap()
        .modified()
        .unwrap();
    let diff = f.partial("file.txt", ChangeArea::Unstaged);
    assert_eq!(diff.hunks.len(), 1);
    assert_eq!(fs::read(f.root.join(".git/index")).unwrap(), bytes);
    assert_eq!(
        fs::metadata(f.root.join(".git/index"))
            .unwrap()
            .modified()
            .unwrap(),
        modified
    );
    for path in ["textconv-ran", "diff-ran", "fsmonitor-ran"] {
        assert!(!f.root.join(path).exists(), "{path}");
    }
}

#[test]
fn unstage_filtered_index_text_does_not_clean_again_or_change_working_bytes() {
    let f = Fixture::new();
    f.write("file.txt", "FIRST\n");
    f.commit();
    f.write(".gitattributes", "file.txt filter=uppercase\n");
    f.git(&[
        "config",
        "filter.uppercase.clean",
        "touch filter-ran; tr a-z A-Z",
    ]);
    f.write("file.txt", "first\nsecond\nthird\n");
    f.repo()
        .execute(&WriteCommand::Stage {
            paths: vec!["file.txt".into()],
        })
        .unwrap();
    assert_eq!(f.index("file.txt"), b"FIRST\nSECOND\nTHIRD\n");
    fs::remove_file(f.root.join("filter-ran")).unwrap();
    let diff = f.partial("file.txt", ChangeArea::Staged);
    let id = changed_id(&diff, PartialLineKind::Addition, "SECOND\n");
    f.apply(diff, PartialSelection::Lines(vec![id]));
    assert_eq!(f.index("file.txt"), b"FIRST\nTHIRD\n");
    assert_eq!(
        fs::read(f.root.join("file.txt")).unwrap(),
        b"first\nsecond\nthird\n"
    );
    assert!(!f.root.join("filter-ran").exists());
}

#[cfg(unix)]
#[test]
fn preexisting_nested_lock_symlink_is_never_removed_or_followed() {
    let f = Fixture::new();
    f.write("file.txt", "baseline\n");
    f.commit();
    f.write("file.txt", "selected\n");
    let diff = f.partial("file.txt", ChangeArea::Unstaged);
    std::os::unix::fs::symlink("missing-target", f.root.join(".git/index.lock.lock")).unwrap();
    assert!(
        f.repo()
            .execute(&WriteCommand::ApplyPartial {
                diff: std::sync::Arc::new(diff),
                selection: PartialSelection::Hunks(vec![0])
            })
            .is_err()
    );
    assert_eq!(
        fs::read_link(f.root.join(".git/index.lock.lock")).unwrap(),
        Path::new("missing-target")
    );
    assert!(!f.root.join(".git/index.lock").exists());
    assert_eq!(f.index("file.txt"), b"baseline\n");
}
