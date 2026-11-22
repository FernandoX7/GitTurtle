use super::*;
use gitturtle_core::ChangeStatus;
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};

const SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><rect width="1" height="1" fill="#ff0000"/></svg>"##;
const LFS_OID: &str = "193f55b4f0be3159c3d9d19ba6d042895a869e1d9405d1bedf5a09d1f56ee6b7";

#[test]
fn ordinary_history_worker_continues_and_restores_the_same_graph_snapshot() {
    let fixture = Fixture::new();
    let mut input = Vec::new();
    for index in 0..1203 {
        let message = format!("commit {index}");
        input.extend_from_slice(format!("commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> {} +0000\ndata {}\n{}\n", 1_700_000_000 + index, message.len(), message).as_bytes());
    }
    input.extend_from_slice(b"done\n");
    fixture.git(&["fast-import", "--quiet"], &input);
    let repo = GitRepository::open(&fixture.0).unwrap();
    let expected = repo.history(2000).unwrap();
    let expected_graph = graph::layout(&expected, || Ok::<_, ()>(())).unwrap();
    let mut session = RepositorySession::default();
    let mut cache = PreviewCache::default();
    let Output::Snapshot(snapshot) = execute(
        Job::Open {
            path: fixture.0.clone(),
            scope: None,
            limit: 500,
            root: None,
            git_dir: None,
        },
        &mut cache,
        &active(),
        &mut session,
    )
    .unwrap() else {
        panic!("snapshot")
    };
    assert_eq!(snapshot.commits, expected[..500]);
    assert_eq!(snapshot.graph, expected_graph[..500]);
    let scope = snapshot.history_scope;
    fixture.git(&["update-ref", "refs/heads/main", &expected[700].oid], b"");
    let mut read = |offset, session: &mut RepositorySession| {
        let Output::HistoryPage(result) = execute(
            Job::HistoryPage {
                repo: repo.clone(),
                scope: scope.clone(),
                offset,
                limit: 500,
            },
            &mut cache,
            &active(),
            session,
        )
        .unwrap() else {
            panic!("history page")
        };
        result
    };
    let page = read(500, &mut session);
    assert_eq!(page.page.commits, expected[500..1000]);
    assert_eq!(page.graph, expected_graph[500..1000]);
    let page = read(1000, &mut session);
    assert_eq!(page.page.commits, expected[1000..]);
    assert_eq!(page.graph, expected_graph[1000..]);
    assert_eq!(page.page.next_offset, None);
    let previous = read(500, &mut session);
    assert_eq!(previous.page.commits, expected[500..1000]);
    assert_eq!(previous.graph, expected_graph[500..1000]);
    // A position past the end reads as the empty page at the end, and
    // the last page's worth of rows up to it keeps its ancestry.
    let past = read(5000, &mut session);
    assert!(past.page.commits.is_empty(), "past the end");
    assert_eq!((past.page.offset, past.page.next_offset), (1203, None));
    let last = read(703, &mut session);
    assert_eq!(last.page.commits, expected[703..]);
    assert_eq!(last.graph, expected_graph[703..]);
    execute(Job::ReleaseHistory, &mut cache, &active(), &mut session).unwrap();
    assert!(session.history.is_none());
}

struct Fixture(PathBuf);

impl Fixture {
    fn command(path: &Path) -> Command {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(path)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null");
        for name in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_PARAMETERS",
        ] {
            command.env_remove(name);
        }
        command
    }

    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "gitturtle-worker-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path).unwrap();
        let output = Self::command(&path)
            .args(["init", "--quiet"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Self(path)
    }

    fn blob(&self, bytes: &[u8]) -> String {
        self.git(&["hash-object", "-w", "--stdin"], bytes)
    }

    fn git(&self, args: &[&str], bytes: &[u8]) -> String {
        let mut child = Self::command(&self.0)
            .args(args)
            .env("GIT_AUTHOR_NAME", "GitTurtle Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "GitTurtle Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(bytes).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().into()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn active() -> Cancellation {
    Cancellation {
        generation: 1,
        latest: Arc::new(AtomicU64::new(1)),
        history: gitturtle_core::HistoryCancellation::default(),
        preparation: None,
    }
}

fn working_fixture() -> (Fixture, GitRepository, FileChange, Arc<Content>) {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("file.txt"), "base\n").unwrap();
    fixture.git(&["add", "file.txt"], b"");
    fixture.git(
        &["-c", "commit.gpgsign=false", "commit", "-qm", "Initial"],
        b"",
    );
    fs::write(fixture.0.join("file.txt"), "ours\n").unwrap();
    let repo = GitRepository::open(&fixture.0).unwrap();
    let entry = repo.status().unwrap().entries.remove(0);
    let Output::WorkingPreview(file, content, _) = execute(
        Job::WorkingPreview {
            head: None,
            repo: repo.clone(),
            entry,
            area: gitturtle_core::ChangeArea::Unstaged,
            prepare_split: false,
        },
        &mut PreviewCache::default(),
        &active(),
        &mut RepositorySession::default(),
    )
    .unwrap() else {
        panic!("expected working preview")
    };
    (fixture, repo, file, content)
}

#[test]
fn history_search_resolves_live_branch_and_worktree_scopes_and_prepares_graph() {
    let fixture = Fixture::new();
    let tree = fixture.git(&["mktree"], b"");
    let first = fixture.git(&["commit-tree", &tree, "-m", "first target"], b"");
    let next = fixture.git(
        &["commit-tree", &tree, "-p", &first, "-m", "new target"],
        b"",
    );
    fixture.git(&["update-ref", "refs/heads/main", &first], b"");
    fixture.git(&["symbolic-ref", "HEAD", "refs/heads/main"], b"");
    let repo = GitRepository::open(&fixture.0).unwrap();
    let stale = repo.branches().unwrap();
    fixture.git(&["update-ref", "refs/heads/main", &next], b"");
    assert_eq!(
        stale
            .iter()
            .find(|branch| branch.name == "main")
            .unwrap()
            .oid,
        first
    );
    for scope in [
        Scope::Branch {
            name: "main".into(),
            remote: false,
        },
        Scope::Worktree {
            path: repo.path().to_owned(),
        },
    ] {
        let Output::SearchHistory(result) = execute(
            Job::SearchHistory {
                repo: repo.clone(),
                scope: Some(scope),
                pinned: None,
                query: "target".into(),
                offset: 0,
                limit: 20,
                previous: Vec::new(),
                remaining_bytes: 1024 * 1024,
            },
            &mut PreviewCache::default(),
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap() else {
            panic!("expected search output");
        };
        assert_eq!(
            result
                .page
                .commits
                .iter()
                .map(|commit| &commit.oid)
                .collect::<Vec<_>>(),
            vec![&next, &first]
        );
        assert_eq!(
            result.page.scope,
            gitturtle_core::HistoryScope::FromCommit(next.clone())
        );
        assert_eq!(result.graph.len(), 2);
        assert!(result.retained_bytes > 0);
    }
    let error = execute(
        Job::SearchHistory {
            repo,
            scope: Some(Scope::Branch {
                name: "deleted".into(),
                remote: false,
            }),
            pinned: None,
            query: "target".into(),
            offset: 0,
            limit: 20,
            previous: Vec::new(),
            remaining_bytes: 1024,
        },
        &mut PreviewCache::default(),
        &active(),
        &mut RepositorySession::default(),
    )
    .err()
    .expect("missing branch must fail");
    assert!(error.to_string().contains("branch"));
    assert_eq!(
        error.downcast_ref::<VanishedScope>(),
        Some(&VanishedScope(Scope::Branch {
            name: "deleted".into(),
            remote: false,
        })),
        "search reports the scope change, not an unavailable repository"
    );
}

#[test]
fn changed_files_preserve_detected_rename_paths_for_preview_identity() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("before.txt"), b"one\ntwo\nthree\n").unwrap();
    fixture.git(&["add", "before.txt"], b"");
    fixture.git(
        &["-c", "commit.gpgsign=false", "commit", "-qm", "base"],
        b"",
    );
    fixture.git(&["mv", "before.txt", "after.txt"], b"");
    fixture.git(
        &["-c", "commit.gpgsign=false", "commit", "-qm", "rename"],
        b"",
    );
    let oid = fixture.git(&["rev-parse", "HEAD"], b"");
    let repo = GitRepository::open(&fixture.0).unwrap();
    let Output::Changes(files, _) = execute(
        Job::Changes {
            repo: repo.clone(),
            oid,
            parent: 0,
        },
        &mut PreviewCache::default(),
        &active(),
        &mut RepositorySession::default(),
    )
    .unwrap() else {
        panic!("expected changed files");
    };
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].status, ChangeStatus::Renamed);
    assert_eq!(files[0].old_path.as_deref(), Some(Path::new("before.txt")));
    assert_eq!(files[0].new_path.as_deref(), Some(Path::new("after.txt")));
    assert_eq!(
        PreviewKey::new(repo.path(), &files[0]).old_path.as_deref(),
        Some(Path::new("before.txt"))
    );
}

#[test]
fn search_continuation_keeps_pinned_scope_after_ref_moves_and_bounds_retained_metadata() {
    let fixture = Fixture::new();
    let tree = fixture.git(&["mktree"], b"");
    let first = fixture.git(&["commit-tree", &tree, "-m", "first"], b"");
    let next = fixture.git(&["commit-tree", &tree, "-p", &first, "-m", "next"], b"");
    fixture.git(&["update-ref", "refs/heads/main", &next], b"");
    let repo = GitRepository::open(&fixture.0).unwrap();
    let run = |pinned, offset, previous, remaining_bytes| {
        execute(
            Job::SearchHistory {
                repo: repo.clone(),
                scope: None,
                pinned,
                query: "".into(),
                offset,
                limit: 1,
                previous,
                remaining_bytes,
            },
            &mut PreviewCache::default(),
            &active(),
            &mut RepositorySession::default(),
        )
    };
    let Output::SearchHistory(page) = run(None, 0, Vec::new(), 1024 * 1024).unwrap() else {
        panic!("expected search");
    };
    fixture.git(&["update-ref", "refs/heads/main", &first], b"");
    let previous = page
        .page
        .commits
        .iter()
        .map(|commit| (commit.oid.clone(), commit.parents.clone()))
        .collect();
    let Output::SearchHistory(rest) = run(
        Some(page.page.scope),
        page.page.next_offset.unwrap(),
        previous,
        1024 * 1024,
    )
    .unwrap() else {
        panic!("expected continuation");
    };
    assert_eq!(rest.page.commits[0].oid, first);
    assert_eq!(rest.graph.len(), 2);
    assert!(
        run(None, 0, Vec::new(), 0)
            .err()
            .unwrap()
            .to_string()
            .contains("display budget")
    );
}

fn quiet_preview(
    repo: &GitRepository,
    file: &FileChange,
    content: &Arc<Content>,
    history: bool,
) -> QuietRefresh {
    let mut cache = PreviewCache::default();
    let Output::QuietRefresh(result) = execute(
        Job::QuietRefresh {
            repo: repo.clone(),
            scope: None,
            limit: 100,
            history,
            selected: Some(WorkingSelection {
                path: "file.txt".into(),
                area: gitturtle_core::ChangeArea::Unstaged,
                file: Some(file.clone()),
                content: Some(Arc::clone(content)),
                prepare_split: false,
            }),
        },
        &mut cache,
        &active(),
        &mut RepositorySession::default(),
    )
    .unwrap() else {
        panic!("expected quiet refresh")
    };
    assert!(
        cache.entries.is_empty(),
        "mutable previews must bypass the immutable cache"
    );
    *result
}

#[test]
fn quiet_status_changes_keep_identical_preview_and_read_only_metadata() {
    let (fixture, repo, file, content) = working_fixture();
    let index = fixture.0.join(".git/index");
    let before = fs::read(&index).unwrap();
    let modified = fs::metadata(&index).unwrap().modified().unwrap();
    fs::write(fixture.0.join("note.txt"), "unrelated local edit\n").unwrap();
    let refresh = quiet_preview(&repo, &file, &content, false);
    assert_eq!(refresh.working.status.entries.len(), 2);
    assert!(refresh.snapshot.is_none());
    assert!(matches!(refresh.preview, Some(Ok(QuietPreview::Unchanged))));
    assert_eq!(fs::read(&index).unwrap(), before);
    assert_eq!(fs::metadata(index).unwrap().modified().unwrap(), modified);
    assert_eq!(fs::read(fixture.0.join("file.txt")).unwrap(), b"ours\n");
}

#[test]
fn quiet_refresh_replaces_same_length_edits_and_removes_vanished_selection() {
    let (fixture, repo, file, content) = working_fixture();
    fs::write(fixture.0.join("file.txt"), "next\n").unwrap();
    let refresh = quiet_preview(&repo, &file, &content, false);
    let Some(Ok(QuietPreview::Changed { file, content })) = refresh.preview else {
        panic!("expected changed text")
    };
    let Content::Text { new, .. } = content.as_ref() else {
        panic!("expected text")
    };
    assert_eq!(new, "next\n");
    fs::write(fixture.0.join("file.txt"), "base\n").unwrap();
    let refresh = quiet_preview(&repo, &file, &content, false);
    assert!(matches!(refresh.preview, Some(Ok(QuietPreview::Absent))));
    assert!(refresh.working.status.entries.is_empty());
}

#[test]
fn quiet_git_refresh_follows_external_staging_and_new_head() {
    let (fixture, repo, file, content) = working_fixture();
    fixture.git(&["add", "file.txt"], b"");
    let refresh = quiet_preview(&repo, &file, &content, true);
    let Some(Ok(QuietPreview::Changed {
        content: staged, ..
    })) = refresh.preview
    else {
        panic!("expected staged preview")
    };
    let Content::Text {
        partial: Some(partial),
        ..
    } = staged.as_ref()
    else {
        panic!("expected staged partial actions")
    };
    assert_eq!(partial.diff.area, gitturtle_core::ChangeArea::Staged);
    let QuietHistory::Refreshed(snapshot) = refresh.snapshot.unwrap().unwrap() else {
        panic!("expected refreshed history")
    };
    assert_eq!(snapshot.commits[0].subject, "Initial");
    fixture.git(
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "External commit",
        ],
        b"",
    );
    let refresh = quiet_preview(&repo, &file, &content, true);
    assert!(matches!(refresh.preview, Some(Ok(QuietPreview::Absent))));
    let QuietHistory::Refreshed(snapshot) = refresh.snapshot.unwrap().unwrap() else {
        panic!("expected refreshed history")
    };
    assert_eq!(snapshot.commits[0].subject, "External commit");
    assert!(refresh.working.status.entries.is_empty());
}

#[test]
fn quiet_deleted_scope_keeps_history_context_but_updates_navigation_metadata() {
    let (fixture, repo, _, _) = working_fixture();
    fixture.git(&["branch", "departing"], b"");
    assert!(
        repo.branches()
            .unwrap()
            .iter()
            .any(|branch| branch.name == "departing")
    );
    fixture.git(&["branch", "-d", "departing"], b"");
    let Output::QuietRefresh(refresh) = execute(
        Job::QuietRefresh {
            repo,
            scope: Some(Scope::Branch {
                name: "departing".into(),
                remote: false,
            }),
            limit: 100,
            history: true,
            selected: None,
        },
        &mut PreviewCache::default(),
        &active(),
        &mut RepositorySession::default(),
    )
    .unwrap() else {
        panic!("expected quiet refresh")
    };
    let QuietHistory::Retained {
        metadata,
        error,
        vanished,
        ..
    } = refresh.snapshot.unwrap().unwrap()
    else {
        panic!("missing scopes must retain displayed history")
    };
    assert!(error.contains("departing"));
    assert_eq!(
        vanished,
        Some(VanishedScope(Scope::Branch {
            name: "departing".into(),
            remote: false,
        })),
        "the view reports one vanished scope once by its identity"
    );
    assert!(
        metadata
            .branches
            .iter()
            .all(|branch| branch.name != "departing")
    );
    assert!(
        metadata
            .refs
            .commits()
            .flat_map(|refs| &refs.names)
            .all(|name| name != "departing")
    );
    assert_eq!(metadata.worktrees.len(), 1);
    assert!(
        metadata.commits.is_empty(),
        "metadata fallback must not load another history page"
    );
}

#[test]
fn scoped_recovery_refresh_updates_tip_without_broadening_pinned_search() {
    let fixture = Fixture::new();
    fixture.git(&["config", "user.name", "GitTurtle Test"], b"");
    fixture.git(&["config", "user.email", "test@example.invalid"], b"");
    fixture.git(&["config", "commit.gpgsign", "false"], b"");
    fs::write(fixture.0.join("file.txt"), "base\n").unwrap();
    fixture.git(&["add", "file.txt"], b"");
    fixture.git(&["commit", "-qm", "Initial"], b"");
    let base = fixture.git(&["rev-parse", "HEAD"], b"");
    fixture.git(&["checkout", "-qb", "tracking/native-release"], b"");
    fs::write(fixture.0.join("file.txt"), "published\n").unwrap();
    fixture.git(&["commit", "-am", "Revision 125: published change"], b"");
    let published = fixture.git(&["rev-parse", "HEAD"], b"");
    fixture.git(
        &["update-ref", "refs/remotes/origin/release", &published],
        b"",
    );
    let tree = fixture.git(&["rev-parse", "HEAD^{tree}"], b"");
    let unrelated = fixture.git(
        &[
            "commit-tree",
            &tree,
            "-p",
            &base,
            "-m",
            "Revision 125: unrelated branch",
        ],
        b"",
    );
    fixture.git(&["update-ref", "refs/heads/unrelated", &unrelated], b"");
    let repo = GitRepository::open(&fixture.0).unwrap();
    let scope = Scope::Branch {
        name: "tracking/native-release".into(),
        remote: false,
    };
    let pinned = gitturtle_core::HistoryScope::FromCommit(published.clone());
    let mut cache = PreviewCache::default();
    let mut session = RepositorySession::default();
    let search = |pinned, offset| Job::SearchHistory {
        repo: repo.clone(),
        scope: Some(scope.clone()),
        pinned,
        query: "Revision 125:".into(),
        offset,
        limit: 10,
        previous: Vec::new(),
        remaining_bytes: 1024 * 1024,
    };
    let Output::SearchHistory(before) =
        execute(search(None, 0), &mut cache, &active(), &mut session).unwrap()
    else {
        panic!("expected search results")
    };
    assert_eq!(before.page.scope, pinned);
    assert_eq!(before.page.commits.len(), 1);
    assert_eq!(before.page.commits[0].oid, published);

    let plan = repo
        .recovery_plan(gitturtle_core::RecoveryKind::Revert, Some(&published), None)
        .unwrap();
    repo.execute(&gitturtle_core::WriteCommand::Recovery(
        gitturtle_core::RecoveryCommand::Revert { plan }.into(),
    ))
    .unwrap();
    let reverted = fixture.git(&["rev-parse", "HEAD"], b"");
    let index = fs::read(fixture.0.join(".git/index")).unwrap();
    for refreshed_scope in [
        scope.clone(),
        Scope::Worktree {
            path: repo.path().to_owned(),
        },
    ] {
        let Output::QuietRefresh(refresh) = execute(
            Job::QuietRefresh {
                repo: repo.clone(),
                scope: Some(refreshed_scope),
                limit: 100,
                history: true,
                selected: None,
            },
            &mut cache,
            &active(),
            &mut session,
        )
        .unwrap() else {
            panic!("expected quiet refresh")
        };
        assert_eq!(
            refresh.working.status.head.as_deref(),
            Some(reverted.as_str())
        );
        let QuietHistory::Refreshed(snapshot) = refresh.snapshot.unwrap().unwrap() else {
            panic!("valid scope must refresh")
        };
        assert_eq!(snapshot.commits[0].oid, reverted);
        assert!(
            snapshot
                .commits
                .iter()
                .any(|commit| commit.oid == published)
        );
        assert!(
            snapshot
                .commits
                .iter()
                .all(|commit| commit.oid != unrelated)
        );
    }
    let Output::SearchHistory(continued) = execute(
        search(Some(pinned.clone()), 1),
        &mut cache,
        &active(),
        &mut session,
    )
    .unwrap() else {
        panic!("expected pinned continuation")
    };
    assert_eq!(continued.page.scope, pinned);
    assert!(continued.page.commits.is_empty());
    let Output::SearchHistory(restarted) =
        execute(search(None, 0), &mut cache, &active(), &mut session).unwrap()
    else {
        panic!("expected restarted search")
    };
    assert_eq!(
        restarted.page.scope,
        gitturtle_core::HistoryScope::FromCommit(reverted.clone())
    );
    assert_eq!(
        restarted
            .page
            .commits
            .iter()
            .map(|commit| commit.oid.as_str())
            .collect::<Vec<_>>(),
        [reverted.as_str(), published.as_str()]
    );
    assert_eq!(fs::read(fixture.0.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(fixture.0.join("file.txt")).unwrap(), b"base\n");
}

#[test]
fn render_pixels_are_bgra_in_place_without_retaining_duplicate_rgba() {
    let preview = ImagePreview {
        width: 2,
        height: 1,
        original_width: 2,
        original_height: 1,
        rgba: vec![255, 30, 10, 128, 5, 70, 200, 0],
        format: "test".into(),
    };
    let (metadata, rendered) = render_image_with_metadata(preview).unwrap();
    assert_eq!(
        rendered.as_bytes(0).unwrap(),
        &[10, 30, 255, 128, 200, 70, 5, 0]
    );
    assert_eq!(metadata.width, 2);
    assert_eq!(metadata.height, 1);
    assert_eq!(metadata.format, "test");
    let expected_bytes = metadata.format.capacity() + 8 + 2 * std::mem::size_of::<ImageSide>();
    let content = Content::Images {
        old: ImageSide {
            animation: None,
            captured: None,
            system_preview: None,
            literal_source: None,
            image: Some(metadata),
            render: Some(rendered),
            message: None,
            bytes: 8,
            lfs_pointer: None,
        }
        .into(),
        new: ImageSide {
            animation: None,
            captured: None,
            system_preview: None,
            literal_source: None,
            image: None,
            render: None,
            message: None,
            bytes: 0,
            lfs_pointer: None,
        }
        .into(),
    };
    assert_eq!(content.bytes(), expected_bytes);
}

/// A comparison preview chosen again while its image is still decoded
/// in this process, a phase that checks nothing until it ends, is
/// answered by that preparation, even after another file was chosen in
/// between and superseded it: the reader prepares it once and never
/// prepares the file chosen in between. A preparation whose decode ended
/// superseded has stopped, so choosing its file again prepares it anew.
/// Chosen again, a preview was prepared from the start each time: an
/// image's bytes read and decoded twice.
#[test]
fn a_preview_chosen_again_while_it_is_prepared_is_prepared_once() {
    use futures::executor::block_on;
    use std::sync::{atomic::AtomicBool, mpsc};

    let fixture = Fixture::new();
    let repo = GitRepository::open(&fixture.0).unwrap();
    let preview = |name: &str| Job::Preview {
        repo: repo.clone(),
        file: FileChange {
            old_path: None,
            new_path: Some(name.into()),
            old_oid: None,
            new_oid: Some("2".repeat(40)),
            status: gitturtle_core::ChangeStatus::Added,
            old_mode: "000000".into(),
            new_mode: "100644".into(),
        },
        origins: Default::default(),
        prepare_split: false,
    };
    let shown = |output: Output| match output {
        Output::Preview(content, _) => match &*content {
            Content::Notice(name) => name.clone(),
            _ => panic!("expected the test's notice"),
        },
        _ => panic!("expected a preview"),
    };
    let (started_tx, started) = mpsc::channel();
    let (checked_tx, checked) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    // Holds the next preparation of "slow.png" in a phase that checks
    // nothing until it ends, as an image decoded in this process does,
    // until released; with `pause` set it then reports whether that
    // phase ended answering and waits once more before it ends.
    let hold = Arc::new(AtomicBool::new(false));
    let pause = Arc::new(AtomicBool::new(false));
    let (held, paused) = (Arc::clone(&hold), Arc::clone(&pause));
    let worker = Worker::with_executor(move |job, _, cancellation| {
        let Job::Preview { file, .. } = job else {
            unreachable!("the test asks for previews only")
        };
        let name = file.path().to_string_lossy().into_owned();
        if name == "slow.png" && held.swap(false, Ordering::AcqRel) {
            let decoded = cancellation.unobserved(|| {
                let _ = started_tx.send(name.clone());
                released.recv_timeout(Duration::from_secs(10))
            });
            if paused.swap(false, Ordering::AcqRel) {
                checked_tx.send(decoded.is_ok())?;
                released.recv_timeout(Duration::from_secs(10))?;
            }
            decoded??;
        } else {
            started_tx.send(name.clone())?;
        }
        cancellation.check()?;
        Ok(Output::Preview(
            Arc::new(Content::Notice(name)),
            Duration::ZERO,
        ))
    });
    let next_started = || started.recv_timeout(Duration::from_secs(10)).unwrap();

    // Chosen, another chosen, then chosen again while it is prepared.
    hold.store(true, Ordering::Release);
    let first = worker.submit(preview("slow.png"));
    assert_eq!(next_started(), "slow.png");
    let mut between = worker.submit(preview("other.png"));
    let again = worker.submit(preview("slow.png"));
    assert!(
        between.try_recv().is_err(),
        "the file chosen in between is never read"
    );
    release.send(()).unwrap();
    assert_eq!(shown(block_on(again).unwrap().unwrap()), "slow.png");
    worker.wait_idle();
    assert_eq!(
        started.try_iter().collect::<Vec<_>>(),
        Vec::<String>::new(),
        "slow.png is prepared once, other.png never"
    );
    assert!(
        !matches!(block_on(first), Ok(Ok(_))),
        "the request the preparation began for is answered no more"
    );
    assert_eq!(worker.preparations(), 1);

    // Stopped as its decode ends superseded, a preparation is not taken
    // over.
    hold.store(true, Ordering::Release);
    pause.store(true, Ordering::Release);
    let first = worker.submit(preview("slow.png"));
    assert_eq!(next_started(), "slow.png");
    let _between = worker.submit(preview("other.png"));
    release.send(()).unwrap();
    assert!(
        !checked.recv_timeout(Duration::from_secs(10)).unwrap(),
        "its decode ends superseded"
    );
    let again = worker.submit(preview("slow.png"));
    release.send(()).unwrap();
    assert!(
        block_on(first).unwrap().is_err(),
        "the stopped preparation ends superseded"
    );
    assert_eq!(
        next_started(),
        "slow.png",
        "chosen again, it is prepared anew"
    );
    assert_eq!(shown(block_on(again).unwrap().unwrap()), "slow.png");
    worker.wait_idle();
    assert_eq!(worker.preparations(), 3);
}

/// A comparison preview's work that watches the request's cancellation
/// token, as a Markdown document's local images do as they are captured
/// (`markdown_view::capture_assets`: its Git reads, which the token's
/// cancellation terminates, and each image's check), stops as soon as
/// another file is chosen, as Back cancels the read, and as the window's
/// worker goes with its tab: the token is cancelled then, as any read's
/// is. Such work is never taken over: choosing its file again prepares it
/// anew. Kept as a preparation another request could take over, the
/// preview once left the token alone, so the capture ran to its end and
/// the next file's preview waited behind it.
#[test]
fn superseding_back_and_closing_stop_a_previews_token_watching_work() {
    use futures::executor::block_on;
    use std::sync::mpsc;

    let fixture = Fixture::new();
    let repo = GitRepository::open(&fixture.0).unwrap();
    let preview = |name: &str| Job::Preview {
        repo: repo.clone(),
        file: FileChange {
            old_path: None,
            new_path: Some(name.into()),
            old_oid: None,
            new_oid: Some("2".repeat(40)),
            status: gitturtle_core::ChangeStatus::Added,
            old_mode: "000000".into(),
            new_mode: "100644".into(),
        },
        origins: Default::default(),
        prepare_split: false,
    };
    // "capture.md" stands for a document whose images are captured: it
    // watches only the token, for up to ten seconds, and reports whether
    // the token was cancelled.
    let (started_tx, started) = mpsc::channel();
    let (stopped_tx, stopped) = mpsc::channel();
    let worker = Worker::with_executor(move |job, _, cancellation| {
        let Job::Preview { file, .. } = job else {
            unreachable!("the test asks for previews only")
        };
        let name = file.path().to_string_lossy().into_owned();
        started_tx.send(name.clone())?;
        if name == "capture.md" {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !cancellation.history.is_cancelled() && Instant::now() < deadline {
                std::thread::yield_now();
            }
            stopped_tx.send(cancellation.history.is_cancelled())?;
        }
        cancellation.check()?;
        Ok(Output::Preview(
            Arc::new(Content::Notice(name)),
            Duration::ZERO,
        ))
    });
    fn next<T>(receiver: &mpsc::Receiver<T>) -> T {
        receiver.recv_timeout(Duration::from_secs(20)).unwrap()
    }

    // Another file chosen, then the first again: the capture stops at
    // once and its file is prepared anew. Once the capture has stopped
    // the reader may start the file chosen in between before the first is
    // chosen again, as it does whenever the test is slow to choose it;
    // that file is then superseded in turn, and answers, if at all, only
    // for itself.
    let first = worker.submit(preview("capture.md"));
    assert_eq!(next(&started), "capture.md");
    let between = worker.submit(preview("other.md"));
    assert!(next(&stopped), "choosing another file stops the capture");
    let again = worker.submit(preview("capture.md"));
    assert!(block_on(first).unwrap().is_err());
    let mut restarted = next(&started);
    let between_started = restarted == "other.md";
    if between_started {
        restarted = next(&started);
    }
    assert_eq!(restarted, "capture.md", "chosen again, prepared anew");
    if let Ok(Ok(Output::Preview(content, _))) = block_on(between) {
        assert!(
            matches!(&*content, Content::Notice(name) if name == "other.md"),
            "the file chosen in between answers only for itself"
        );
    }
    // Back cancels the read.
    worker.cancel();
    assert!(next(&stopped), "Back stops the capture");
    assert!(block_on(again).unwrap().is_err());
    worker.wait_idle();
    // The reader counts a preparation as it takes the request and checks
    // the request before the work starts, so the file chosen in between
    // may be counted, stopped before it reports starting.
    let preparations = worker.preparations();
    assert!(
        (2 + u64::from(between_started)..=3).contains(&preparations),
        "{preparations} preparations, the file chosen in between started: {between_started}"
    );

    // The tab closes, and its worker goes.
    let _closing = worker.submit(preview("capture.md"));
    assert_eq!(next(&started), "capture.md");
    drop(worker);
    assert!(next(&stopped), "closing the tab stops the capture");
}

fn added_file(fixture: &Fixture, name: &str, bytes: &[u8]) -> FileChange {
    FileChange {
        old_path: None,
        new_path: Some(name.into()),
        old_oid: None,
        new_oid: Some(fixture.blob(bytes)),
        status: ChangeStatus::Added,
        old_mode: "000000".into(),
        new_mode: "100644".into(),
    }
}

fn captured_preview(repo: &GitRepository, file: FileChange) -> Arc<Content> {
    let Output::Preview(content, _) = execute(
        Job::Preview {
            origins: Default::default(),
            repo: repo.clone(),
            file,
            prepare_split: false,
        },
        &mut PreviewCache::default(),
        &active(),
        &mut RepositorySession::default(),
    )
    .unwrap() else {
        panic!("expected captured preview")
    };
    content
}

#[test]
fn gif_worker_retains_composed_frames_and_accounts_for_every_render_buffer() {
    let fixture = Fixture::new();
    let repo = GitRepository::open(&fixture.0).unwrap();
    let mut bytes = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
        for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
            encoder
                .encode_frame(image::Frame::from_parts(
                    image::RgbaImage::from_pixel(3, 2, image::Rgba(color)),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(100, 1),
                ))
                .unwrap();
        }
    }
    let content = captured_preview(&repo, added_file(&fixture, "animation.data", &bytes));
    let Content::Images { new, .. } = &*content else {
        panic!("GIF magic should route to images");
    };
    let render = new.render.as_ref().unwrap();
    assert_eq!(render.frame_count(), 2);
    assert_eq!(new.animation.as_ref().unwrap().end_ms.len(), 2);
    assert_ne!(render.as_bytes(0), render.as_bytes(1));
    assert_eq!(new.captured.as_deref(), Some(bytes.as_slice()));
    let rendered_bytes: usize = (0..render.frame_count())
        .map(|frame| render.as_bytes(frame).unwrap().len())
        .sum();
    assert!(content.bytes() >= bytes.len() + rendered_bytes);
    assert!(
        content.bytes() < bytes.len() + rendered_bytes + render.as_bytes(0).unwrap().len() + 2048
    );
}

#[test]
fn model_worker_retains_obj_geometry_without_losing_captured_source_identity() {
    let fixture = Fixture::new();
    let repo = GitRepository::open(&fixture.0).unwrap();
    let source = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
    fs::write(fixture.0.join("triangle.obj"), "different current file").unwrap();
    let content = captured_preview(&repo, added_file(&fixture, "triangle.obj", source));
    let Content::Rich(preview) = &*content else {
        panic!("ASCII model uses geometric preview");
    };
    assert_eq!(preview.new.metadata.format, "OBJ");
    let model = preview.new.model.as_ref().expect("retained model geometry");
    assert!(preview.new.pages.is_empty());
    assert_eq!(model.bookmark().target, [0.5, 0.5, 0.]);
    assert!(model.retained_bytes() > 0);
    assert!(content.bytes() >= source.len() + model.retained_bytes());
    assert_eq!(preview.new.captured.as_deref(), Some(source.as_slice()));
    assert_eq!(
        preview.new.metadata.source.as_deref().unwrap().as_bytes(),
        source
    );
    assert_eq!(
        fs::read_to_string(fixture.0.join("triangle.obj")).unwrap(),
        "different current file"
    );
}

const GLB_BEFORE: &[u8] =
    include_bytes!("../../../preview/tests/fixtures/models/glb/assembly-before.glb");
const GLB_AFTER: &[u8] =
    include_bytes!("../../../preview/tests/fixtures/models/glb/assembly-after.glb");
const MESHOPT_GLB_BEFORE: &[u8] =
    include_bytes!("../../../preview/tests/fixtures/models/glb/meshopt-arch-before.glb");
const MESHOPT_GLB_AFTER: &[u8] =
    include_bytes!("../../../preview/tests/fixtures/models/glb/meshopt-arch-after.glb");

#[test]
fn glb_history_preserves_captured_revisions_absence_and_independent_errors() {
    check_glb_history(GLB_BEFORE, GLB_AFTER);
}

#[test]
fn meshopt_glb_history_preserves_captured_revisions_absence_and_independent_errors() {
    check_glb_history(MESHOPT_GLB_BEFORE, MESHOPT_GLB_AFTER);
}

fn check_glb_history(before: &[u8], after: &[u8]) {
    let fixture = Fixture::new();
    let repo = GitRepository::open(&fixture.0).unwrap();
    fs::write(fixture.0.join("assembly.GLB"), b"unrelated worktree").unwrap();
    let mut file = added_file(&fixture, "assembly.GLB", after);
    file.old_path = Some("assembly.glb".into());
    file.old_oid = Some(fixture.blob(before));
    file.old_mode = "100644".into();
    file.status = ChangeStatus::Renamed;
    let content = captured_preview(&repo, file.clone());
    let Content::Rich(sides) = &*content else {
        panic!("native GLB comparison")
    };
    for (side, expected) in [(&sides.old, before), (&sides.new, after)] {
        assert_eq!(side.captured.as_deref(), Some(expected));
        assert!(
            side.metadata.source.is_none(),
            "binary bytes are not staging text"
        );
        assert!(side.model.is_some(), "{:?}", side.error);
        assert!(side.pages.is_empty(), "no eager fixed frames");
    }
    assert!(
        content.bytes()
            >= before.len()
                + after.len()
                + sides.old.model.as_ref().unwrap().retained_bytes()
                + sides.new.model.as_ref().unwrap().retained_bytes()
    );

    file.old_oid = Some(fixture.blob(b"glTF truncated"));
    let invalid = captured_preview(&repo, file.clone());
    let Content::Rich(sides) = &*invalid else {
        panic!("side-specific GLB error")
    };
    assert!(sides.old.error.is_some());
    assert!(sides.old.present);
    assert_eq!(
        sides.old.captured.as_deref(),
        Some(b"glTF truncated".as_slice())
    );
    assert!(sides.new.model.is_some());

    file.old_oid = Some(fixture.blob(b""));
    let empty = captured_preview(&repo, file);
    let Content::Rich(sides) = &*empty else {
        panic!("present empty GLB side")
    };
    assert!(sides.old.present && sides.old.error.is_some());
    assert_eq!(sides.old.captured.as_deref(), Some(b"".as_slice()));
    assert!(sides.new.model.is_some());

    for deleted in [false, true] {
        let mut absent = added_file(&fixture, "assembly.GLB", after);
        if deleted {
            std::mem::swap(&mut absent.old_path, &mut absent.new_path);
            std::mem::swap(&mut absent.old_oid, &mut absent.new_oid);
            std::mem::swap(&mut absent.old_mode, &mut absent.new_mode);
            absent.status = ChangeStatus::Deleted;
        }
        let content = captured_preview(&repo, absent);
        let Content::Rich(sides) = &*content else {
            panic!("absent GLB side")
        };
        let (missing, present) = if deleted {
            (&sides.new, &sides.old)
        } else {
            (&sides.old, &sides.new)
        };
        assert!(!missing.present);
        assert!(missing.captured.is_none() && missing.error.is_none());
        assert!(present.model.is_some());
    }
    assert_eq!(
        fs::read(fixture.0.join("assembly.GLB")).unwrap(),
        b"unrelated worktree"
    );
}

#[test]
fn glb_working_preview_uses_current_bytes_without_caching_or_index_writes() {
    check_glb_working_preview(GLB_BEFORE, GLB_AFTER);
}

#[test]
fn meshopt_glb_working_preview_uses_current_bytes_without_caching_or_index_writes() {
    check_glb_working_preview(MESHOPT_GLB_BEFORE, MESHOPT_GLB_AFTER);
}

fn check_glb_working_preview(before: &[u8], after: &[u8]) {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("assembly.glb"), before).unwrap();
    fixture.git(&["add", "assembly.glb"], b"");
    fixture.git(&["-c", "commit.gpgsign=false", "commit", "-qm", "GLB"], b"");
    let repo = GitRepository::open(&fixture.0).unwrap();
    let index = fs::read(fixture.0.join(".git/index")).unwrap();
    let mut cache = PreviewCache::default();
    let mut session = RepositorySession::default();
    for bytes in [after, b"invalid GLB".as_slice(), after] {
        fs::write(fixture.0.join("assembly.glb"), bytes).unwrap();
        let entry = repo.status().unwrap().entries.remove(0);
        let Output::WorkingPreview(_, content, _) = execute(
            Job::WorkingPreview {
                head: None,
                repo: repo.clone(),
                entry,
                area: gitturtle_core::ChangeArea::Unstaged,
                prepare_split: false,
            },
            &mut cache,
            &active(),
            &mut session,
        )
        .unwrap() else {
            panic!("working GLB")
        };
        let Content::Rich(sides) = &*content else {
            panic!("rich GLB")
        };
        assert_eq!(sides.old.captured.as_deref(), Some(before));
        assert_eq!(sides.new.captured.as_deref(), Some(bytes));
        assert!(sides.old.model.is_some());
        assert_eq!(sides.new.model.is_some(), bytes == after);
        assert!(cache.entries.is_empty());
    }
    assert_eq!(fs::read(fixture.0.join(".git/index")).unwrap(), index);
}

#[test]
fn glb_lfs_miss_is_retryable_and_verified_local_bytes_enter_the_model_pipeline() {
    check_glb_lfs_retry(GLB_AFTER);
}

#[test]
fn meshopt_glb_lfs_miss_is_retryable_and_verified_local_bytes_enter_the_model_pipeline() {
    check_glb_lfs_retry(MESHOPT_GLB_AFTER);
}

fn check_glb_lfs_retry(bytes: &[u8]) {
    use sha2::{Digest, Sha256};
    let fixture = Fixture::new();
    let oid = gitturtle_core::lower_hex(Sha256::digest(bytes));
    let pointer = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {}\n",
        bytes.len()
    );
    let file = added_file(&fixture, "assembly.glb", pointer.as_bytes());
    let repo = GitRepository::open(&fixture.0).unwrap();
    let mut cache = PreviewCache::default();
    let mut session = RepositorySession::default();
    let mut preview = |expected_entries| {
        let Output::Preview(content, _) = execute(
            Job::Preview {
                origins: Default::default(),
                repo: repo.clone(),
                file: file.clone(),
                prepare_split: false,
            },
            &mut cache,
            &active(),
            &mut session,
        )
        .unwrap() else {
            panic!("LFS GLB")
        };
        assert_eq!(cache.entries.len(), expected_entries);
        content
    };
    let missing = preview(0);
    let Content::Rich(sides) = &*missing else {
        panic!("unavailable LFS model retains its captured pointer")
    };
    assert!(!sides.old.present && sides.old.error.is_none());
    assert!(sides.new.present && sides.new.error.is_some());
    assert_eq!(sides.new.captured.as_deref(), Some(pointer.as_bytes()));
    let targets = crate::lfs_download::preview_targets(&missing, &file);
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].0, 1);
    assert_eq!(targets[0].1.pointer, pointer.as_bytes());
    assert_eq!(targets[0].1.blob_oid, file.new_oid);
    let directory = fixture
        .0
        .join(".git/lfs/objects")
        .join(&oid[..2])
        .join(&oid[2..4]);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join(&oid), bytes).unwrap();
    let available = preview(1);
    let Content::Rich(sides) = &*available else {
        panic!("resolved GLB geometry")
    };
    assert_eq!(sides.new.captured.as_deref(), Some(bytes));
    assert!(sides.new.model.is_some(), "{:?}", sides.new.error);
    assert!(!sides.old.present);
    assert!(crate::lfs_download::preview_targets(&available, &file).is_empty());
    assert!(Arc::ptr_eq(&available, &preview(1)));
}

#[test]
fn meshopt_glb_lfs_comparison_keeps_verified_side_and_retries_missing_or_corrupt_side() {
    use sha2::{Digest, Sha256};
    for unavailable_side in [0, 1] {
        let fixture = Fixture::new();
        let bytes = [MESHOPT_GLB_BEFORE, MESHOPT_GLB_AFTER];
        let oids = bytes.map(|bytes| gitturtle_core::lower_hex(Sha256::digest(bytes)));
        let pointers: [String; 2] = std::array::from_fn(|i| {
            format!(
                "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n",
                oids[i],
                bytes[i].len()
            )
        });
        let objects: [PathBuf; 2] = std::array::from_fn(|i| {
            let directory = fixture
                .0
                .join(".git/lfs/objects")
                .join(&oids[i][..2])
                .join(&oids[i][2..4]);
            fs::create_dir_all(&directory).unwrap();
            directory.join(&oids[i])
        });
        let available_side = 1 - unavailable_side;
        fs::write(&objects[available_side], bytes[available_side]).unwrap();
        let mut file = added_file(&fixture, "after.GLB", pointers[1].as_bytes());
        file.old_path = Some("before.glb".into());
        file.old_oid = Some(fixture.blob(pointers[0].as_bytes()));
        file.old_mode = "100644".into();
        file.status = ChangeStatus::Renamed;
        let repo = GitRepository::open(&fixture.0).unwrap();
        let mut cache = PreviewCache::default();
        let mut session = RepositorySession::default();
        let mut preview = |expected_cached| {
            let Output::Preview(content, _) = execute(
                Job::Preview {
                    repo: repo.clone(),
                    file: file.clone(),
                    origins: Default::default(),
                    prepare_split: false,
                },
                &mut cache,
                &active(),
                &mut session,
            )
            .unwrap() else {
                panic!("LFS model comparison")
            };
            assert_eq!(cache.entries.len(), expected_cached);
            content
        };
        for corrupt in [false, true] {
            if corrupt {
                // Preserve size so verification must check the SHA-256 too.
                let mut damaged = bytes[unavailable_side].to_vec();
                damaged[0] ^= 1;
                fs::write(&objects[unavailable_side], damaged).unwrap();
            }
            let content = preview(0);
            let Content::Rich(comparison) = &*content else {
                panic!("independent LFS model sides")
            };
            let sides = [&comparison.old, &comparison.new];
            let usable = sides[available_side];
            assert!(usable.model.is_some(), "{:?}", usable.error);
            assert_eq!(usable.captured.as_deref(), Some(bytes[available_side]));
            let unavailable = sides[unavailable_side];
            assert!(unavailable.present && unavailable.model.is_none());
            let error = unavailable.error.as_deref().unwrap();
            assert!(error.contains(if corrupt {
                "local copy of this Git LFS object is damaged"
            } else {
                "unavailable in the local store"
            }));
            assert_eq!(
                unavailable.captured.as_deref(),
                Some(pointers[unavailable_side].as_bytes())
            );
            assert_eq!(
                unavailable.metadata.source.as_deref(),
                Some(pointers[unavailable_side].as_str())
            );
            let targets = crate::lfs_download::preview_targets(&content, &file);
            if corrupt {
                // A download is refused for a local object that fails
                // verification, so none is offered.
                assert!(targets.is_empty());
                continue;
            }
            assert_eq!(targets.len(), 1);
            assert_eq!(targets[0].0, unavailable_side);
            assert_eq!(targets[0].1.pointer, pointers[unavailable_side].as_bytes());
            assert_eq!(
                Some(&targets[0].1.path),
                [file.old_path.as_ref(), file.new_path.as_ref()][unavailable_side]
            );
            assert_eq!(
                targets[0].1.blob_oid.as_ref(),
                [file.old_oid.as_ref(), file.new_oid.as_ref()][unavailable_side]
            );
        }
        fs::write(&objects[unavailable_side], bytes[unavailable_side]).unwrap();
        let resolved = preview(1);
        let Content::Rich(comparison) = &*resolved else {
            panic!("both verified LFS models")
        };
        for (side, bytes) in [&comparison.old, &comparison.new].into_iter().zip(bytes) {
            assert!(side.model.is_some() && side.error.is_none());
            assert_eq!(side.captured.as_deref(), Some(bytes));
        }
        assert!(crate::lfs_download::preview_targets(&resolved, &file).is_empty());
        assert!(Arc::ptr_eq(&resolved, &preview(1)));
    }
}

#[test]
fn meshopt_glb_cache_reserves_geometry_and_frames_before_any_raster_is_requested() {
    let fixture = Fixture::new();
    let repo = GitRepository::open(&fixture.0).unwrap();
    let file = added_file(&fixture, "arch.glb", MESHOPT_GLB_AFTER);
    let key = PreviewKey::new(repo.path(), &file);
    let content = captured_preview(&repo, file);
    let weak = {
        let Content::Rich(sides) = &*content else {
            panic!("retained compressed GLB")
        };
        let model = sides.new.model.as_ref().unwrap();
        assert!(sides.new.pages.is_empty());
        assert!(model.retained_bytes() >= 36 * 3 * 3 * 8 + 2 * 720 * 720 * 4);
        assert!(content.bytes() >= MESHOPT_GLB_AFTER.len() + model.retained_bytes());
        Arc::downgrade(model)
    };
    let weight = content.bytes();
    let mut insufficient = PreviewCache::with_limits(weight - 1, 1);
    insufficient.insert(key.clone(), content.clone());
    assert!(insufficient.entries.is_empty());
    assert_eq!(insufficient.bytes, 0);
    let mut cache = PreviewCache::with_limits(weight, 1);
    cache.insert(key.clone(), content.clone());
    assert_eq!(cache.bytes, weight);
    assert!(Arc::ptr_eq(&cache.get(&key).unwrap(), &content));
    cache.remove(&key);
    assert_eq!(cache.bytes, 0);
    assert!(
        weak.upgrade().is_some(),
        "the active view still owns its model"
    );
    drop(content);
    assert!(
        weak.upgrade().is_none(),
        "the last content owner releases the model"
    );
}

#[test]
fn meshopt_glb_working_lfs_keeps_pointer_targets_and_retries_without_index_or_worktree_writes() {
    use sha2::{Digest, Sha256};
    let fixture = Fixture::new();
    let oids = [MESHOPT_GLB_BEFORE, MESHOPT_GLB_AFTER]
        .map(|bytes| gitturtle_core::lower_hex(Sha256::digest(bytes)));
    let pointers: [String; 2] = std::array::from_fn(|i| {
        format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n",
            oids[i],
            [MESHOPT_GLB_BEFORE, MESHOPT_GLB_AFTER][i].len()
        )
    });
    let path = fixture.0.join("assembly.glb");
    fs::write(&path, &pointers[0]).unwrap();
    fixture.git(&["add", "assembly.glb"], b"");
    fixture.git(&["-c", "commit.gpgsign=false", "commit", "-qm", "LFS"], b"");
    let index = fs::read(fixture.0.join(".git/index")).unwrap();
    fs::write(&path, &pointers[1]).unwrap();
    let objects: [PathBuf; 2] = std::array::from_fn(|i| {
        let directory = fixture
            .0
            .join(".git/lfs/objects")
            .join(&oids[i][..2])
            .join(&oids[i][2..4]);
        fs::create_dir_all(&directory).unwrap();
        directory.join(&oids[i])
    });
    fs::write(&objects[0], MESHOPT_GLB_BEFORE).unwrap();
    let repo = GitRepository::open(&fixture.0).unwrap();
    let mut cache = PreviewCache::default();
    for available in [false, true] {
        if available {
            fs::write(&objects[1], MESHOPT_GLB_AFTER).unwrap();
        }
        let entry = repo.status().unwrap().entries.remove(0);
        let Output::WorkingPreview(file, content, _) = execute(
            Job::WorkingPreview {
                repo: repo.clone(),
                entry,
                area: gitturtle_core::ChangeArea::Unstaged,
                head: None,
                prepare_split: false,
            },
            &mut cache,
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap() else {
            panic!("working LFS comparison")
        };
        let Content::Rich(sides) = &*content else {
            panic!("working LFS model sides")
        };
        assert!(sides.old.model.is_some() && sides.old.error.is_none());
        assert_eq!(sides.old.captured.as_deref(), Some(MESHOPT_GLB_BEFORE));
        assert_eq!(sides.new.model.is_some(), available);
        let targets = crate::lfs_download::preview_targets(&content, &file);
        if available {
            assert!(targets.is_empty());
            assert_eq!(sides.new.captured.as_deref(), Some(MESHOPT_GLB_AFTER));
        } else {
            assert_eq!(targets.len(), 1);
            assert_eq!(targets[0].0, 1);
            assert_eq!(targets[0].1.path, Path::new("assembly.glb"));
            assert!(targets[0].1.blob_oid.is_none());
            assert_eq!(targets[0].1.pointer, pointers[1].as_bytes());
        }
        assert!(cache.entries.is_empty());
        assert_eq!(fs::read(fixture.0.join(".git/index")).unwrap(), index);
        assert_eq!(fs::read(&path).unwrap(), pointers[1].as_bytes());
    }
}

#[test]
fn meshopt_glb_cancellation_is_propagated_instead_of_retaining_a_failed_side() {
    use std::cell::Cell;
    let calls = Cell::new(0);
    let prepared = crate::rich_preview::Side::prepare(
        MESHOPT_GLB_AFTER.to_vec(),
        Path::new("arch.glb"),
        || {
            calls.set(calls.get() + 1);
            Ok(())
        },
    )
    .unwrap();
    assert!(prepared.model.is_some(), "{:?}", prepared.error);
    let checkpoints = calls.get();
    drop(prepared);
    for stop_at in [1, checkpoints / 2, checkpoints] {
        let cancellation = active();
        calls.set(0);
        let result = crate::rich_preview::Side::prepare(
            MESHOPT_GLB_AFTER.to_vec(),
            Path::new("arch.glb"),
            || {
                calls.set(calls.get() + 1);
                if calls.get() >= stop_at {
                    cancellation.history.cancel();
                }
                cancellation.check()
            },
        );
        let error = result
            .err()
            .expect("cancelled preparation must not retain content");
        assert!(error.to_string().contains("superseded"));
    }
}

#[test]
fn mermaid_history_keeps_literal_renamed_sources_and_failed_diagrams() {
    let fixture = Fixture::new();
    let repo = GitRepository::open(&fixture.0).unwrap();
    let old = "flowchart LR\r\nA[Before]-->B\r\n";
    let new = "flowchart LR\r\nA[After]-->B\r\n";
    fs::write(fixture.0.join("new.mmd"), "different current source").unwrap();
    let mut file = added_file(&fixture, "new.mmd", new.as_bytes());
    file.old_path = Some("old.mermaid".into());
    file.old_oid = Some(fixture.blob(old.as_bytes()));
    file.old_mode = "100644".into();
    file.status = ChangeStatus::Renamed;
    let content = captured_preview(&repo, file);
    let Content::Text {
        old: actual_old,
        new: actual_new,
        diagrams: Some(diagrams),
        ..
    } = &*content
    else {
        panic!("Mermaid must retain text");
    };
    assert_eq!(actual_old, old);
    assert_eq!(actual_new, new);
    assert_eq!(diagrams.old.captured.as_deref(), Some(old.as_bytes()));
    assert_eq!(diagrams.new.captured.as_deref(), Some(new.as_bytes()));
    assert!(diagrams.old.pages[0].render.is_some() && diagrams.new.pages[0].render.is_some());
    assert!(content.bytes() >= diagrams.retained_bytes() + old.len() + new.len());
    let broken = "sequenceDiagram\n%%{init: {}}%%\nAlice->>Bob: literal";
    let content = captured_preview(&repo, added_file(&fixture, "broken.mmd", broken.as_bytes()));
    assert!(
        matches!(&*content, Content::Text { new, diagrams: Some(diagrams), .. } if new == broken && diagrams.new.pages[0].error.is_some())
    );
    let mut symlink = added_file(&fixture, "link.mmd", old.as_bytes());
    symlink.new_mode = "120000".into();
    assert!(matches!(
        &*captured_preview(&repo, symlink),
        Content::Text { diagrams: None, .. }
    ));
    let pointer = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{LFS_OID}\nsize {}\n",
        SVG.len()
    );
    let unavailable = captured_preview(
        &repo,
        added_file(&fixture, "missing.mmd", pointer.as_bytes()),
    );
    assert!(matches!(&*unavailable, Content::Text {diagrams: None, new, ..} if new == &pointer));
}

#[test]
fn mermaid_working_and_quick_open_keep_exact_staging_and_review_sources() {
    let fixture = Fixture::new();
    let old = "# Workflow\n```mermaid\nflowchart LR; A-->B\n```\n";
    let new = "# Workflow\n```mermaid\nflowchart LR; A-->C\n```\n";
    fs::write(fixture.0.join("README.md"), old).unwrap();
    fixture.git(&["add", "README.md"], b"");
    fixture.git(
        &["-c", "commit.gpgsign=false", "commit", "-qm", "Chart"],
        b"",
    );
    fs::write(fixture.0.join("README.md"), new).unwrap();
    let repo = GitRepository::open(&fixture.0).unwrap();
    let index = fs::read(fixture.0.join(".git/index")).unwrap();
    let Output::WorkingPreview(_, content, _) = execute(
        Job::WorkingPreview {
            head: None,
            repo: repo.clone(),
            entry: repo.status().unwrap().entries.remove(0),
            area: gitturtle_core::ChangeArea::Unstaged,
            prepare_split: false,
        },
        &mut PreviewCache::default(),
        &active(),
        &mut RepositorySession::default(),
    )
    .unwrap() else {
        panic!("working preview");
    };
    let Content::Text {
        diagrams: Some(diagrams),
        partial: Some(_),
        new: actual,
        ..
    } = &*content
    else {
        panic!("Chart retains exact partial staging");
    };
    assert_eq!(actual, new);
    let reviewed = crate::text_review::prepare(
        &content,
        crate::text_review::Options {
            hide_whitespace: true,
            context: 3,
        },
        || Ok(()),
    )
    .unwrap();
    assert!(
        matches!(&*reviewed, Content::Text { diagrams: Some(retained), partial: None, .. } if Arc::ptr_eq(retained, diagrams))
    );
    let paths = repo
        .search_tracked_paths(
            &gitturtle_core::PathScope::Worktree,
            "README",
            &gitturtle_core::HistoryCancellation::default(),
        )
        .unwrap();
    let Output::TrackedPreview { content: quick, .. } = execute(
        Job::TrackedPreview {
            repo,
            entry: paths.entries[0].clone(),
            scope: gitturtle_core::PathScope::Worktree,
        },
        &mut PreviewCache::default(),
        &active(),
        &mut RepositorySession::default(),
    )
    .unwrap() else {
        panic!("quick preview");
    };
    assert!(
        matches!(&*quick, Content::Text { diagrams: Some(diagrams), partial: None, new: actual, .. } if actual == new && !diagrams.old.present && diagrams.new.pages[0].render.is_some())
    );
    assert_eq!(fs::read(fixture.0.join(".git/index")).unwrap(), index);
    assert_eq!(
        fs::read_to_string(fixture.0.join("README.md")).unwrap(),
        new
    );
}

#[test]
fn content_detection_preserves_mislabeled_source_and_image_bytes() {
    let fixture = Fixture::new();
    let repo = GitRepository::open(&fixture.0).unwrap();
    let source = b"const literal = '<svg is just text>';\n";
    let preview = captured_preview(&repo, added_file(&fixture, "misleading.png", source));
    assert!(matches!(&*preview,Content::Text {new,..} if new.as_bytes()==source));
    let image = captured_preview(&repo, added_file(&fixture, "no-extension", SVG));
    let Content::Images { old, new } = &*image else {
        panic!("content should identify SVG")
    };
    assert!(old.image.is_none());
    assert_eq!(new.captured.as_deref(), Some(SVG));
    assert_eq!(new.literal_source.as_deref().unwrap().as_bytes(), SVG);
    assert!(image.bytes() >= SVG.len() * 2 + 8);
}

/// Whether a side offers Open in default app is decided here, with its
/// captured bytes and from them alone: a GIF named like a launcher and a
/// PDF named like a script offer their own types, and SVG and audio,
/// whatever their names, offer nothing.
#[cfg(target_os = "linux")]
#[test]
fn the_default_app_offer_is_decided_with_the_captured_bytes() {
    let fixture = Fixture::new();
    let repo = GitRepository::open(&fixture.0).unwrap();
    let gif = include_bytes!("../../../preview/tests/fixtures/disposal-transparency.gif");
    let content = captured_preview(&repo, added_file(&fixture, "launcher.desktop", gif));
    let Content::Images { old, new } = &*content else {
        panic!("GIF magic routes to images")
    };
    assert_eq!(old.system_preview, None, "an absent side offers nothing");
    let offer = new.system_preview.expect("a GIF offers the action");
    assert!(offer.tooltip().contains(" as a GIF image "));
    let svg = captured_preview(&repo, added_file(&fixture, "picture.png", SVG));
    let Content::Images { new, .. } = &*svg else {
        panic!("content should identify SVG")
    };
    assert!(new.captured.is_some());
    assert_eq!(new.system_preview, None);
    let pdf = b"%PDF-1.4\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n";
    let rich = captured_preview(&repo, added_file(&fixture, "install.sh", pdf));
    let Content::Rich(preview) = &*rich else {
        panic!("PDF is a rich side")
    };
    let offer = preview.new.system_preview.expect("PDF offers the action");
    assert!(offer.tooltip().contains(" as a PDF document "));
    let wav = b"RIFF\x24\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x44\xac\0\0\x88\x58\x01\0\x02\0\x10\0data\0\0\0\0";
    let audio = captured_preview(&repo, added_file(&fixture, "sound.wav", wav));
    let Content::Rich(preview) = &*audio else {
        panic!("WAV is captured metadata")
    };
    assert!(preview.new.captured.is_some());
    assert_eq!(preview.new.system_preview, None, "audio offers nothing");
}

#[test]
fn rich_history_keeps_exact_old_new_and_missing_sides_without_worktree_substitution() {
    let fixture = Fixture::new();
    let repo = GitRepository::open(&fixture.0).unwrap();
    let old = b"RIFF\0\0\0\0WAVE";
    let new = b"RIFF\x01\0\0\0WAVE";
    fs::write(fixture.0.join("sound.wav"), b"unrelated current worktree").unwrap();
    let mut file = added_file(&fixture, "sound.wav", new);
    file.old_path = Some("old-name.wav".into());
    file.old_oid = Some(fixture.blob(old));
    file.old_mode = "100644".into();
    file.status = ChangeStatus::Renamed;
    let preview = captured_preview(&repo, file.clone());
    let Content::Rich(sides) = &*preview else {
        panic!("expected media metadata")
    };
    assert_eq!(sides.old.captured.as_deref(), Some(old.as_slice()));
    assert_eq!(sides.new.captured.as_deref(), Some(new.as_slice()));
    assert_eq!(sides.old.metadata.format, "WAV audio");
    file.old_oid = Some("f".repeat(40));
    let missing = captured_preview(&repo, file);
    let Content::Rich(sides) = &*missing else {
        panic!("expected independent unavailable side")
    };
    assert!(sides.old.error.is_some());
    assert_eq!(sides.new.captured.as_deref(), Some(new.as_slice()));
    assert_eq!(
        fs::read(fixture.0.join("sound.wav")).unwrap(),
        b"unrelated current worktree"
    );
}

#[test]
fn quick_open_and_working_changes_share_bounded_media_routing() {
    let fixture = Fixture::new();
    let original = b"RIFF\0\0\0\0WAVE";
    let changed = b"RIFF\x01\0\0\0WAVE";
    fs::write(fixture.0.join("sound.wav"), original).unwrap();
    fixture.git(&["add", "sound.wav"], b"");
    fixture.git(
        &["-c", "commit.gpgsign=false", "commit", "-qm", "Audio"],
        b"",
    );
    fs::write(fixture.0.join("sound.wav"), changed).unwrap();
    let repo = GitRepository::open(&fixture.0).unwrap();
    let entry = repo.status().unwrap().entries.remove(0);
    let Output::WorkingPreview(_, content, _) = execute(
        Job::WorkingPreview {
            head: None,
            repo: repo.clone(),
            entry,
            area: gitturtle_core::ChangeArea::Unstaged,
            prepare_split: false,
        },
        &mut PreviewCache::default(),
        &active(),
        &mut RepositorySession::default(),
    )
    .unwrap() else {
        panic!("expected working preview")
    };
    let Content::Rich(sides) = &*content else {
        panic!("expected media preview")
    };
    assert_eq!(sides.old.captured.as_deref(), Some(original.as_slice()));
    assert_eq!(sides.new.captured.as_deref(), Some(changed.as_slice()));
    let paths = repo
        .search_tracked_paths(
            &gitturtle_core::PathScope::Worktree,
            "sound",
            &gitturtle_core::HistoryCancellation::default(),
        )
        .unwrap();
    let Output::TrackedPreview { content, .. } = execute(
        Job::TrackedPreview {
            repo,
            entry: paths.entries[0].clone(),
            scope: gitturtle_core::PathScope::Worktree,
        },
        &mut PreviewCache::default(),
        &active(),
        &mut RepositorySession::default(),
    )
    .unwrap() else {
        panic!("expected quick source")
    };
    let Content::Rich(sides) = &*content else {
        panic!("expected quick source media preview")
    };
    assert!(!sides.old.present);
    assert_eq!(sides.new.captured.as_deref(), Some(changed.as_slice()));
}

#[test]
fn stored_symlink_named_as_image_is_literal_and_never_followed() {
    let fixture = Fixture::new();
    let repo = GitRepository::open(&fixture.0).unwrap();
    let target = b"../private-file.png";
    let mut file = added_file(&fixture, "link.png", target);
    file.new_mode = "120000".into();
    let preview = captured_preview(&repo, file);
    assert!(matches!(&*preview,Content::Text {new,..} if new.as_bytes()==target));
}

#[test]
fn missing_image_object_keeps_the_readable_side_rendered() {
    let fixture = Fixture::new();
    let repo = GitRepository::open(&fixture.0).unwrap();
    let mut file = added_file(&fixture, "image.svg", SVG);
    file.old_path = Some("image.svg".into());
    file.old_mode = "100644".into();
    file.old_oid = Some("f".repeat(40));
    let preview = captured_preview(&repo, file);
    let Content::Images { old, new } = &*preview else {
        panic!("expected independent image sides")
    };
    assert!(old.message.is_some());
    assert!(new.render.is_some());
    assert_eq!(new.captured.as_deref(), Some(SVG));
}

/// Each side's Git LFS object is judged on its own. One over the preview
/// limit or failing verification stays its pointer and says which, and
/// the other side keeps its preview, where either once failed the whole
/// file. Neither offers a download, which could not help; a side whose
/// object is only missing still does. Text that keeps a pointer is never
/// cached: the store can change under the pointer.
#[test]
fn a_failing_lfs_side_leaves_the_other_sides_preview_and_offers_no_download() {
    use gitturtle_core::LfsObjectFailure::{Corrupt, TooLarge};
    use sha2::{Digest, Sha256};
    let fixture = Fixture::new();
    // The pointer to `body`, with `stored` as its object in the local store.
    let pointer = |body: &[u8], stored: Option<&[u8]>| {
        let oid = gitturtle_core::lower_hex(Sha256::digest(body));
        if let Some(stored) = stored {
            let directory = fixture
                .0
                .join(".git/lfs/objects")
                .join(&oid[..2])
                .join(&oid[2..4]);
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join(&oid), stored).unwrap();
        }
        format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {}\n",
            body.len()
        )
    };
    let oversize = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n",
        "e".repeat(64),
        33 * 1024 * 1024
    );
    // The right size and the wrong bytes.
    let corrupt = pointer(b"the true text\n", Some(b"the wrong one\n"));
    let local_body = "the local object\n";
    let local = pointer(local_body.as_bytes(), Some(local_body.as_bytes()));
    let missing = pointer(b"never stored\n", None);
    let plain = "plain text\n";
    let (oversize, corrupt, local, missing) = (
        oversize.as_str(),
        corrupt.as_str(),
        local.as_str(),
        missing.as_str(),
    );
    let repo = GitRepository::open(&fixture.0).unwrap();
    let modified = |old: &[u8], new: &[u8]| FileChange {
        old_path: Some("notes.txt".into()),
        new_path: Some("notes.txt".into()),
        old_oid: Some(fixture.blob(old)),
        new_oid: Some(fixture.blob(new)),
        status: ChangeStatus::Modified,
        old_mode: "100644".into(),
        new_mode: "100644".into(),
    };
    type Row<'a> = (
        &'a str,
        &'a str,
        &'a str,
        &'a str,
        &'a str,
        [bool; 2],
        [Option<gitturtle_core::LfsObjectFailure>; 2],
        &'a [usize],
    );
    let rows: [Row<'_>; 7] = [
        (
            "over the limit first beside plain text",
            oversize,
            plain,
            oversize,
            plain,
            [false, false],
            [Some(TooLarge), None],
            &[],
        ),
        (
            "corrupt first beside plain text",
            corrupt,
            plain,
            corrupt,
            plain,
            [false, false],
            [Some(Corrupt), None],
            &[],
        ),
        (
            "plain text first beside a corrupt object",
            plain,
            corrupt,
            plain,
            corrupt,
            [false, false],
            [None, Some(Corrupt)],
            &[],
        ),
        (
            "corrupt first beside a local object",
            corrupt,
            local,
            corrupt,
            local_body,
            [false, false],
            [Some(Corrupt), None],
            &[],
        ),
        (
            "a local object first beside one over the limit",
            local,
            oversize,
            local_body,
            oversize,
            [false, false],
            [None, Some(TooLarge)],
            &[],
        ),
        (
            "over the limit first beside a missing object",
            oversize,
            missing,
            oversize,
            missing,
            [false, true],
            [Some(TooLarge), None],
            &[1],
        ),
        (
            "both unusable",
            corrupt,
            oversize,
            corrupt,
            oversize,
            [false, false],
            [Some(Corrupt), Some(TooLarge)],
            &[],
        ),
    ];
    for (name, old, new, shown_old, shown_new, absent, failed, offered) in rows {
        let file = modified(old.as_bytes(), new.as_bytes());
        let mut cache = PreviewCache::default();
        let output = execute(
            Job::Preview {
                origins: Default::default(),
                repo: repo.clone(),
                file: file.clone(),
                prepare_split: false,
            },
            &mut cache,
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap_or_else(|error| panic!("{name}: the whole preview failed: {error:#}"));
        let Output::Preview(content, _) = output else {
            panic!("{name}: expected a preview")
        };
        let Content::Text {
            old: drawn_old,
            new: drawn_new,
            lfs_missing,
            lfs_failed,
            ..
        } = &*content
        else {
            panic!("{name}: expected text")
        };
        assert_eq!(drawn_old, shown_old, "{name}");
        assert_eq!(drawn_new, shown_new, "{name}");
        assert_eq!(*lfs_missing, absent, "{name}");
        assert_eq!(*lfs_failed, failed, "{name}");
        assert_eq!(
            crate::lfs_download::preview_targets(&content, &file)
                .iter()
                .map(|(side, _)| *side)
                .collect::<Vec<_>>(),
            offered,
            "{name}: only a missing object offers a download"
        );
        assert!(cache.entries.is_empty(), "{name}: a pointer stays uncached");
    }
    // With no text to show beside the pointer the file is a notice, which
    // still says what is wrong with the side that failed.
    let file = modified(oversize.as_bytes(), b"\0binary\n");
    let Output::Preview(content, _) = execute(
        Job::Preview {
            origins: Default::default(),
            repo,
            file,
            prepare_split: false,
        },
        &mut PreviewCache::default(),
        &active(),
        &mut RepositorySession::default(),
    )
    .unwrap() else {
        panic!("expected a preview")
    };
    let Content::Notice(message) = &*content else {
        panic!("expected the binary comparison's notice")
    };
    assert!(
        message.ends_with("\n\nBefore: this Git LFS object is over the 32 MiB preview limit."),
        "{message}"
    );
}

/// A notice saying a side's object is corrupt or its store unreadable is
/// not kept in the preview cache, since the store can change under the
/// same pointer and a repair has to show; one saying the object is over
/// the limit depends only on the pointer, so it may be kept.
#[cfg(unix)]
#[test]
fn a_notice_about_the_local_store_is_read_again_and_one_about_the_pointer_is_kept() {
    use sha2::{Digest, Sha256};
    let fixture = Fixture::new();
    let repo = GitRepository::open(&fixture.0).unwrap();
    let body = b"the true text\n";
    let oid = gitturtle_core::lower_hex(Sha256::digest(body));
    let store = fixture.0.join(".git/lfs");
    let directory = store.join("objects").join(&oid[..2]).join(&oid[2..4]);
    fs::create_dir_all(&directory).unwrap();
    let object = directory.join(&oid);
    let stored = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {}\n",
        body.len()
    );
    let oversize = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n",
        "e".repeat(64),
        33 * 1024 * 1024
    );
    let binary = fixture.blob(b"\0binary\n");
    let notice = |cache: &mut PreviewCache, old: &str| -> String {
        let file = FileChange {
            old_path: Some("notes.txt".into()),
            new_path: Some("notes.txt".into()),
            old_oid: Some(fixture.blob(old.as_bytes())),
            new_oid: Some(binary.clone()),
            status: ChangeStatus::Modified,
            old_mode: "100644".into(),
            new_mode: "100644".into(),
        };
        let Output::Preview(content, _) = execute(
            Job::Preview {
                origins: Default::default(),
                repo: repo.clone(),
                file,
                prepare_split: false,
            },
            cache,
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap() else {
            panic!("expected a preview")
        };
        // Once repaired the side resolves, and whatever the file then
        // shows carries no notice about its object.
        match &*content {
            Content::Notice(message) => message.clone(),
            _ => String::new(),
        }
    };

    // Corrupt: the right size, the wrong bytes.
    fs::write(&object, b"the wrong one\n").unwrap();
    let mut cache = PreviewCache::default();
    let says = notice(&mut cache, &stored);
    assert!(says.contains("\n\nBefore: "), "{says}");
    assert!(cache.entries.is_empty(), "a corrupt side is read again");
    // Repaired, the same pointer no longer says so.
    fs::write(&object, body).unwrap();
    let repaired = notice(&mut cache, &stored);
    assert!(!repaired.contains("Before:"), "{repaired}");

    // A store that cannot be read, repaired the same way.
    let mut cache = PreviewCache::default();
    let real = fixture.0.join(".git/lfs-real");
    fs::rename(&store, &real).unwrap();
    std::os::unix::fs::symlink(&real, &store).unwrap();
    let says = notice(&mut cache, &stored);
    assert!(
        says.contains("the local Git LFS store could not be read"),
        "{says}"
    );
    assert!(
        cache.entries.is_empty(),
        "an unreadable store is read again"
    );
    fs::remove_file(&store).unwrap();
    fs::rename(&real, &store).unwrap();
    let repaired = notice(&mut cache, &stored);
    assert!(!repaired.contains("Before:"), "{repaired}");

    // Over the limit is the pointer's own, so it may be kept.
    let mut cache = PreviewCache::default();
    let says = notice(&mut cache, &oversize);
    assert!(says.contains("over the 32 MiB preview limit"), "{says}");
    assert_eq!(cache.entries.len(), 1, "the pointer alone decides this one");
}

/// Whether a Markdown file is stored as a pointer on both sides is judged
/// as stored, not from the text a local object resolved to: with one
/// object local the other side was the only pointer left in the text, so
/// the file gained a Rendered view it never had. With both objects local
/// nothing is left a pointer and it renders as it always did, as does a
/// file with a side stored as regular text and an added file whose
/// object is local.
#[test]
fn a_markdown_file_stored_as_two_pointers_renders_only_with_both_objects_local() {
    use sha2::{Digest, Sha256};
    let fixture = Fixture::new();
    let repo = GitRepository::open(&fixture.0).unwrap();
    // `text`'s pointer, with its object in the local store if `local`.
    let pointer = |text: &str, local: bool| {
        let oid = gitturtle_core::lower_hex(Sha256::digest(text));
        if local {
            let directory = fixture
                .0
                .join(".git/lfs/objects")
                .join(&oid[..2])
                .join(&oid[2..4]);
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join(&oid), text).unwrap();
        }
        format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {}\n",
            text.len()
        )
    };
    let rendered = |old: Option<&str>, new: &str| {
        let file = FileChange {
            old_path: old.map(|_| "README.md".into()),
            new_path: Some("README.md".into()),
            old_oid: old.map(|old| fixture.blob(old.as_bytes())),
            new_oid: Some(fixture.blob(new.as_bytes())),
            status: if old.is_some() {
                ChangeStatus::Modified
            } else {
                ChangeStatus::Added
            },
            old_mode: if old.is_some() { "100644" } else { "000000" }.into(),
            new_mode: "100644".into(),
        };
        let Output::Preview(content, _) = execute(
            Job::Preview {
                origins: Default::default(),
                repo: repo.clone(),
                file,
                prepare_split: false,
            },
            &mut PreviewCache::default(),
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap() else {
            panic!("expected a preview")
        };
        let Content::Text { markdown, .. } = &*content else {
            panic!("expected text")
        };
        markdown.is_some()
    };
    let (before, after) = ("# Before\n", "# After\n");
    for (name, old, new) in [
        (
            "only the old object local",
            pointer(before, true),
            pointer("# One\n", false),
        ),
        (
            "only the new object local",
            pointer("# Two\n", false),
            pointer(after, true),
        ),
        (
            "neither object local",
            pointer("# Three\n", false),
            pointer("# Four\n", false),
        ),
    ] {
        assert!(!rendered(Some(&old), &new), "{name}");
    }
    assert!(
        rendered(None, &pointer(after, true)),
        "an added file with its object local"
    );
    for (name, old, new) in [
        (
            "both objects local",
            pointer(before, true),
            pointer(after, true),
        ),
        (
            "a regular side beside a local object",
            before.to_owned(),
            pointer(after, true),
        ),
        (
            "a regular side beside a missing object",
            before.to_owned(),
            pointer("# Five\n", false),
        ),
        (
            "a local object beside a regular side",
            pointer(before, true),
            after.to_owned(),
        ),
    ] {
        assert!(rendered(Some(&old), &new), "{name}");
    }
}

/// An image side that stays its Git LFS pointer, whatever kept its object
/// from it, holds a pointer's text and no image, so it offers nothing to
/// open and its literal source copies as a pointer. An SVG, whose
/// literal source is the image itself, keeps both.
#[test]
fn an_image_side_that_stays_its_pointer_offers_nothing_to_open() {
    use sha2::{Digest, Sha256};
    let fixture = Fixture::new();
    let repo = GitRepository::open(&fixture.0).unwrap();
    let body = b"the true object\n";
    let oid = gitturtle_core::lower_hex(Sha256::digest(body));
    let directory = fixture
        .0
        .join(".git/lfs/objects")
        .join(&oid[..2])
        .join(&oid[2..4]);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join(&oid), b"a wrong object!\n").unwrap();
    let corrupt = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {}\n",
        body.len()
    );
    let oversize = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n",
        "e".repeat(64),
        33 * 1024 * 1024
    );
    let missing = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize 42\n",
        "b".repeat(64)
    );
    let sides = |path: &str, old: &str, new: &str| {
        let file = FileChange {
            old_path: Some(path.into()),
            new_path: Some(path.into()),
            old_oid: Some(fixture.blob(old.as_bytes())),
            new_oid: Some(fixture.blob(new.as_bytes())),
            status: ChangeStatus::Modified,
            old_mode: "100644".into(),
            new_mode: "100644".into(),
        };
        let Output::Preview(content, _) = execute(
            Job::Preview {
                origins: Default::default(),
                repo: repo.clone(),
                file,
                prepare_split: false,
            },
            &mut PreviewCache::default(),
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap() else {
            panic!("expected a preview")
        };
        content
    };
    for (name, old, new) in [
        ("over the limit and corrupt", &oversize, &corrupt),
        ("missing and over the limit", &missing, &oversize),
    ] {
        let content = sides("picture.png", old, new);
        let Content::Images {
            old: before,
            new: after,
        } = &*content
        else {
            panic!("{name}: expected the image comparison")
        };
        for (side, pointer) in [(before, old), (after, new)] {
            assert_eq!(
                side.literal_source.as_deref(),
                Some(pointer.as_str()),
                "{name}"
            );
            assert!(side.captured.is_none(), "{name}: no bytes to hand over");
            assert!(side.system_preview.is_none(), "{name}: nothing to open");
            assert!(side.literal_is_pointer(), "{name}: copies as a pointer");
            assert_eq!(
                crate::image_compare::literal_copy_words(side),
                (
                    "Copy pointer",
                    "Copy this Git LFS pointer's exact text",
                    "LFS pointer"
                ),
                "{name}: its tooltip and feedback say so too"
            );
        }
    }
    const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"/>";
    let content = sides("picture.svg", SVG, "<svg/>");
    let Content::Images { old: svg, .. } = &*content else {
        panic!("expected the image comparison")
    };
    assert!(svg.literal_source.is_some() && svg.captured.is_some());
    assert!(!svg.literal_is_pointer(), "an SVG's source is its image");
    assert_eq!(
        crate::image_compare::literal_copy_words(svg).0,
        "Copy source"
    );
    assert_eq!(crate::image_compare::literal_copy_words(svg).2, "source");
}

/// A failure of the repository itself, as the read of a pointer's object
/// meets it, leaves the side its pointer as any other failure does: its
/// text is not captured as though it were the image.
#[test]
fn an_image_side_whose_object_cannot_be_read_at_all_captures_nothing() {
    let fixture = Fixture::new();
    let repo = GitRepository::open(&fixture.0).unwrap();
    let pointer = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize 42\n",
        "b".repeat(64)
    );
    fs::remove_dir_all(fixture.0.join(".git")).unwrap();
    let side = working_image_side(
        &repo,
        pointer.clone().into_bytes(),
        true,
        "picture.png",
        &active(),
    );
    assert!(side.message.is_some(), "the read's failure is said");
    assert!(side.captured.is_none(), "{:?}", side.message);
    assert!(side.system_preview.is_none(), "nothing to open");
    assert_eq!(side.literal_source.as_deref(), Some(pointer.as_str()));
}

/// An image's or a model's side whose Git LFS object cannot be used says
/// what is wrong in the words every side shares, with no raw error and
/// no mention of a download, which could not help.
#[cfg(unix)]
#[test]
fn a_failing_image_or_model_side_says_the_shared_words() {
    use sha2::{Digest, Sha256};
    const OVER: &str = "This Git LFS object is over the 32 MiB preview limit.";
    const DAMAGED: &str = "The local copy of this Git LFS object is damaged.";
    const UNREADABLE: &str = "The local Git LFS store could not be read.";
    let fixture = Fixture::new();
    let repo = GitRepository::open(&fixture.0).unwrap();
    let store = fixture.0.join(".git/lfs");
    let body = b"the true object\n";
    let oid = gitturtle_core::lower_hex(Sha256::digest(body));
    let directory = store.join("objects").join(&oid[..2]).join(&oid[2..4]);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join(&oid), b"a wrong object!\n").unwrap();
    let corrupt = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {}\n",
        body.len()
    );
    let oversize = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n",
        "e".repeat(64),
        33 * 1024 * 1024
    );
    let read = |path: &str, old: &str, new: &str| {
        let file = FileChange {
            old_path: Some(path.into()),
            new_path: Some(path.into()),
            old_oid: Some(fixture.blob(old.as_bytes())),
            new_oid: Some(fixture.blob(new.as_bytes())),
            status: ChangeStatus::Modified,
            old_mode: "100644".into(),
            new_mode: "100644".into(),
        };
        let Output::Preview(content, _) = execute(
            Job::Preview {
                origins: Default::default(),
                repo: repo.clone(),
                file,
                prepare_split: false,
            },
            &mut PreviewCache::default(),
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap() else {
            panic!("{path}: expected a preview")
        };
        content
    };
    let image = |old: &str, new: &str| {
        let content = read("picture.png", old, new);
        let Content::Images { old, new } = &*content else {
            panic!("expected the image comparison")
        };
        (old.message.clone(), new.message.clone())
    };
    let model = |old: &str, new: &str| {
        let content = read("model.glb", old, new);
        let Content::Rich(comparison) = &*content else {
            panic!("expected the model comparison")
        };
        (comparison.old.error.clone(), comparison.new.error.clone())
    };
    let owned = |words: &str| Some(words.to_owned());
    let expected = (owned(OVER), owned(DAMAGED));
    assert_eq!(image(&oversize, &corrupt), expected, "image");
    assert_eq!(model(&oversize, &corrupt), expected, "model");
    // A store that cannot be read says so of whichever side it reaches.
    let real = fixture.0.join(".git/lfs-real");
    fs::rename(&store, &real).unwrap();
    std::os::unix::fs::symlink(&real, &store).unwrap();
    let expected = (owned(OVER), owned(UNREADABLE));
    assert_eq!(image(&oversize, &corrupt), expected, "image, unreadable");
    assert_eq!(model(&oversize, &corrupt), expected, "model, unreadable");
}

/// Mermaid is attached after the sides are marked, on both the route that
/// resolves a side and the one that keeps the stored texts, so Diagrams
/// says what is wrong with a failing side rather than that it is absent.
#[test]
fn a_failing_pointer_beside_mermaid_says_what_is_wrong_in_diagrams() {
    use sha2::{Digest, Sha256};
    let fixture = Fixture::new();
    let oversize = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n",
        "e".repeat(64),
        33 * 1024 * 1024
    );
    let diagram = "flowchart LR\n  A --> B\n";
    // A local pointer to the diagram, so that side resolves.
    let oid = gitturtle_core::lower_hex(Sha256::digest(diagram));
    let directory = fixture
        .0
        .join(".git/lfs/objects")
        .join(&oid[..2])
        .join(&oid[2..4]);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join(&oid), diagram).unwrap();
    let local = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {}\n",
        diagram.len()
    );
    let repo = GitRepository::open(&fixture.0).unwrap();
    for (name, new) in [
        ("regular text", diagram),
        ("a resolved object", local.as_str()),
    ] {
        let file = FileChange {
            old_path: Some("flow.mmd".into()),
            new_path: Some("flow.mmd".into()),
            old_oid: Some(fixture.blob(oversize.as_bytes())),
            new_oid: Some(fixture.blob(new.as_bytes())),
            status: ChangeStatus::Modified,
            old_mode: "100644".into(),
            new_mode: "100644".into(),
        };
        let Output::Preview(content, _) = execute(
            Job::Preview {
                origins: Default::default(),
                repo: repo.clone(),
                file,
                prepare_split: false,
            },
            &mut PreviewCache::default(),
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap() else {
            panic!("{name}: expected a preview")
        };
        let Content::Text {
            diagrams: Some(diagrams),
            ..
        } = &*content
        else {
            panic!("{name}: the regular side draws")
        };
        assert_eq!(
            diagrams.old.metadata.details,
            [
                "This Git LFS object is over the 32 MiB preview limit; its stored text \
              is below."
            ],
            "{name}"
        );
        assert!(diagrams.new.pages[0].render.is_some(), "{name}");
    }
}

/// The same stored texts with a side's object failing differently are not
/// the same content: what the side says depends on it, and it changes
/// without the pointer's text changing.
#[test]
fn a_change_in_how_an_lfs_side_failed_is_a_content_change() {
    use gitturtle_core::LfsObjectFailure::{Corrupt, TooLarge};
    let text = |failed: [Option<gitturtle_core::LfsObjectFailure>; 2]| {
        let mut content = prepared_text("patch".into(), "old".into(), "new".into(), false);
        LfsSides {
            missing: [false; 2],
            failed,
            ..Default::default()
        }
        .mark(&mut content);
        content
    };
    assert!(content_unchanged(
        &text([Some(Corrupt), None]),
        &text([Some(Corrupt), None])
    ));
    assert!(!content_unchanged(
        &text([Some(Corrupt), None]),
        &text([Some(TooLarge), None])
    ));
    assert!(!content_unchanged(
        &text([None, None]),
        &text([Some(Corrupt), None])
    ));
}

/// A Markdown file with one pointer side keeps its Rendered view: the
/// regular side renders, and the pointer side stays literal, saying what
/// its own object is: absent, or why it cannot be used.
#[test]
fn a_mixed_markdown_preview_renders_the_regular_side() {
    let fixture = Fixture::new();
    let repo = GitRepository::open(&fixture.0).unwrap();
    let pointer = |size: u64| {
        format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {size}\n",
            "b".repeat(64)
        )
    };
    for (name, stored, says, absent) in [
        (
            "missing",
            pointer(42),
            "This side's Git LFS object is not in the local store, and no download was \
             attempted; its stored text is below.",
            true,
        ),
        (
            "over the limit",
            pointer(33 * 1024 * 1024),
            "This Git LFS object is over the 32 MiB preview limit; its stored text \
             is below.",
            false,
        ),
    ] {
        let file = FileChange {
            old_path: Some("README.md".into()),
            new_path: Some("README.md".into()),
            old_oid: Some(fixture.blob(b"# Before\n")),
            new_oid: Some(fixture.blob(stored.as_bytes())),
            status: ChangeStatus::Modified,
            old_mode: "100644".into(),
            new_mode: "100644".into(),
        };
        let Output::Preview(content, _) = execute(
            Job::Preview {
                origins: Default::default(),
                repo: repo.clone(),
                file: file.clone(),
                prepare_split: false,
            },
            &mut PreviewCache::default(),
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap() else {
            panic!("{name}: expected a preview")
        };
        let Content::Text {
            markdown: Some(markdown),
            lfs_missing,
            ..
        } = &*content
        else {
            panic!("{name}: a mixed file has a Rendered view")
        };
        assert!(
            markdown.old.literal.is_none(),
            "{name}: the regular side renders"
        );
        assert_eq!(markdown.new.literal.as_deref(), Some(says), "{name}");
        assert_eq!(*lfs_missing, [false, absent], "{name}");
        assert_eq!(
            crate::lfs_download::preview_targets(&content, &file)
                .iter()
                .map(|(side, _)| *side)
                .collect::<Vec<_>>(),
            if absent { vec![1] } else { Vec::new() },
            "{name}"
        );
    }
}

#[test]
fn lfs_preview_retries_misses_and_caches_verified_content() {
    let fixture = Fixture::new();
    let pointer = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{LFS_OID}\nsize {}\n",
        SVG.len()
    );
    let blob_oid = fixture.blob(pointer.as_bytes());
    let repository = GitRepository::open(&fixture.0).unwrap();
    let file = FileChange {
        old_path: None,
        new_path: Some("image.svg".into()),
        old_oid: None,
        new_oid: Some(blob_oid),
        status: ChangeStatus::Added,
        old_mode: "000000".into(),
        new_mode: "100644".into(),
    };
    let mut cache = PreviewCache::default();
    let mut session = RepositorySession::default();
    let mut preview = |expected_cached_entries| {
        let output = execute(
            Job::Preview {
                origins: Default::default(),
                repo: repository.clone(),
                file: file.clone(),
                prepare_split: false,
            },
            &mut cache,
            &active(),
            &mut session,
        )
        .unwrap();
        assert_eq!(cache.entries.len(), expected_cached_entries);
        match output {
            Output::Preview(content, _) => content,
            _ => panic!("expected preview"),
        }
    };
    let missing = preview(0);
    match &*missing {
        Content::Images { old, new } => {
            assert!(old.image.is_none());
            assert!(old.message.is_none());
            assert!(new.image.is_none());
            assert!(
                new.message
                    .as_ref()
                    .unwrap()
                    .contains("unavailable in the local store")
            );
        }
        _ => panic!("expected image sides"),
    }
    let store = fixture
        .0
        .join(".git/lfs/objects")
        .join(&LFS_OID[..2])
        .join(&LFS_OID[2..4]);
    fs::create_dir_all(&store).unwrap();
    let object = store.join(LFS_OID);
    fs::write(&object, SVG).unwrap();
    let available = preview(1);
    match &*available {
        Content::Images { new, .. } => {
            assert_eq!(new.bytes, SVG.len());
            assert_eq!(
                new.render.as_ref().unwrap().as_bytes(0).unwrap(),
                &[0, 0, 255, 255]
            );
            assert!(new.message.is_none());
        }
        _ => panic!("expected image sides"),
    }
    fs::remove_file(object).unwrap();
    // Cached decoded bytes remain the exact verified SHA-256 content even
    // if a separate client prunes its LFS store after our successful read.
    let cached = preview(1);
    assert!(Arc::ptr_eq(&available, &cached));
}

#[test]
fn lfs_text_preview_retries_missing_pointer_and_displays_verified_content() {
    let fixture = Fixture::new();
    let pointer = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{LFS_OID}\nsize {}\n",
        SVG.len()
    );
    let repo = GitRepository::open(&fixture.0).unwrap();
    let file = FileChange {
        old_path: None,
        new_path: Some("source.txt".into()),
        old_oid: None,
        new_oid: Some(fixture.blob(pointer.as_bytes())),
        status: ChangeStatus::Added,
        old_mode: "000000".into(),
        new_mode: "100644".into(),
    };
    let mut cache = PreviewCache::default();
    let mut session = RepositorySession::default();
    let missing = execute(
        Job::Preview {
            origins: Default::default(),
            repo: repo.clone(),
            file: file.clone(),
            prepare_split: false,
        },
        &mut cache,
        &active(),
        &mut session,
    )
    .unwrap();
    assert!(
        matches!(missing, Output::Preview(content, _) if matches!(content.as_ref(), Content::Text { new, .. } if new == &pointer))
    );
    assert!(cache.entries.is_empty());
    let directory = fixture
        .0
        .join(".git/lfs/objects")
        .join(&LFS_OID[..2])
        .join(&LFS_OID[2..4]);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join(LFS_OID), SVG).unwrap();
    let available = execute(
        Job::Preview {
            repo,
            file,
            origins: Default::default(),
            prepare_split: false,
        },
        &mut cache,
        &active(),
        &mut session,
    )
    .unwrap();
    assert!(
        matches!(available, Output::Preview(content, _) if matches!(content.as_ref(), Content::Text { new, partial, .. } if new.as_bytes() == SVG && partial.is_none()))
    );
    assert_eq!(cache.entries.len(), 1);
}

/// With one object in the local store and the other missing, the text
/// shows the first and keeps the other's pointer, and nothing is cached:
/// the missing object can still arrive.
#[test]
fn lfs_text_preview_resolves_the_side_whose_object_is_local() {
    let fixture = Fixture::new();
    let local = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{LFS_OID}\nsize {}\n",
        SVG.len()
    );
    let missing = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize 42\n",
        "b".repeat(64)
    );
    let directory = fixture
        .0
        .join(".git/lfs/objects")
        .join(&LFS_OID[..2])
        .join(&LFS_OID[2..4]);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join(LFS_OID), SVG).unwrap();
    let repo = GitRepository::open(&fixture.0).unwrap();
    let file = FileChange {
        old_path: Some("source.txt".into()),
        new_path: Some("source.txt".into()),
        old_oid: Some(fixture.blob(local.as_bytes())),
        new_oid: Some(fixture.blob(missing.as_bytes())),
        status: ChangeStatus::Modified,
        old_mode: "100644".into(),
        new_mode: "100644".into(),
    };
    let mut cache = PreviewCache::default();
    let mut session = RepositorySession::default();
    let shown = execute(
        Job::Preview {
            origins: Default::default(),
            repo,
            file,
            prepare_split: false,
        },
        &mut cache,
        &active(),
        &mut session,
    )
    .unwrap();
    assert!(
        matches!(shown, Output::Preview(content, _) if matches!(content.as_ref(), Content::Text { old, new, partial, lfs_missing, .. } if old.as_bytes() == SVG && new == &missing && partial.is_none() && *lfs_missing == [false, true]))
    );
    assert!(cache.entries.is_empty());
}

/// A pointer whose object the local store lacks, beside a binary blob,
/// leaves the comparison a notice that shows neither side; it is not
/// cached either, whichever side the pointer is on, so an object that
/// arrives is read the next time: the object's text beside the binary
/// blob, cached as any comparison whose sides both resolved.
#[test]
fn a_missing_lfs_side_beside_a_binary_notice_is_not_cached() {
    use sha2::{Digest, Sha256};
    let object = b"downloaded text\n";
    let oid = gitturtle_core::lower_hex(Sha256::digest(object));
    let pointer = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {}\n",
        object.len()
    );
    let binary = b"\0\x01\x02binary\xff";
    for pointer_is_old in [true, false] {
        let at = if pointer_is_old {
            "the pointer before"
        } else {
            "the pointer after"
        };
        let fixture = Fixture::new();
        let repo = GitRepository::open(&fixture.0).unwrap();
        let (old, new) = if pointer_is_old {
            (pointer.as_bytes(), &binary[..])
        } else {
            (&binary[..], pointer.as_bytes())
        };
        let file = FileChange {
            old_path: Some("data.bin".into()),
            new_path: Some("data.bin".into()),
            old_oid: Some(fixture.blob(old)),
            new_oid: Some(fixture.blob(new)),
            status: ChangeStatus::Modified,
            old_mode: "100644".into(),
            new_mode: "100644".into(),
        };
        let mut cache = PreviewCache::default();
        let mut session = RepositorySession::default();
        let mut preview = |cache: &mut PreviewCache| match execute(
            Job::Preview {
                origins: Default::default(),
                repo: repo.clone(),
                file: file.clone(),
                prepare_split: false,
            },
            cache,
            &active(),
            &mut session,
        )
        .unwrap()
        {
            Output::Preview(content, _) => content,
            _ => panic!("{at}: expected a preview"),
        };
        let missing = preview(&mut cache);
        assert!(
            matches!(missing.as_ref(), Content::Notice(_)),
            "{at}: the comparison is a notice"
        );
        assert!(cache.entries.is_empty(), "{at}: nothing is cached");
        let directory = fixture
            .0
            .join(".git/lfs/objects")
            .join(&oid[..2])
            .join(&oid[2..4]);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join(&oid), object).unwrap();
        let resolved = preview(&mut cache);
        let side = if pointer_is_old { 0 } else { 1 };
        assert!(
            matches!(resolved.as_ref(), Content::Rich(preview)
                if [&preview.old, &preview.new][side].captured.as_deref() == Some(&object[..])),
            "{at}: the arrived object is read"
        );
        assert_eq!(
            cache.entries.len(),
            1,
            "{at}: both sides resolved are cached"
        );
    }
}

/// A committed text file whose working copy is now a Git LFS pointer to
/// an object the local store lacks, and that pointer's text.
fn working_missing_pointer(fixture: &Fixture) -> String {
    fs::write(fixture.0.join("notes.txt"), "before\n").unwrap();
    fixture.git(&["add", "notes.txt"], b"");
    fixture.git(
        &["-c", "commit.gpgsign=false", "commit", "-qm", "Notes"],
        b"",
    );
    let pointer = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize 42\n",
        "b".repeat(64)
    );
    fs::write(fixture.0.join("notes.txt"), &pointer).unwrap();
    pointer
}

/// The side a Working preview found absent is marked, so it offers its
/// download: unmarked, the action silently goes.
#[test]
fn a_working_preview_marks_a_missing_lfs_object_and_offers_its_download() {
    let fixture = Fixture::new();
    let pointer = working_missing_pointer(&fixture);
    let repo = GitRepository::open(&fixture.0).unwrap();
    let entry = repo.status().unwrap().entries.remove(0);
    let Output::WorkingPreview(file, content, _) = execute(
        Job::WorkingPreview {
            head: None,
            repo,
            entry,
            area: gitturtle_core::ChangeArea::Unstaged,
            prepare_split: false,
        },
        &mut PreviewCache::default(),
        &active(),
        &mut RepositorySession::default(),
    )
    .unwrap() else {
        panic!("expected working preview")
    };
    let Content::Text {
        new, lfs_missing, ..
    } = &*content
    else {
        panic!("expected the pointer's text")
    };
    assert_eq!(new, &pointer);
    assert_eq!(*lfs_missing, [false, true]);
    let targets = crate::lfs_download::preview_targets(&content, &file);
    assert_eq!(
        targets.iter().map(|(side, _)| *side).collect::<Vec<_>>(),
        [1]
    );
}

/// So is the side Quick Open's working source found absent.
#[test]
fn quick_open_marks_a_missing_lfs_object_and_offers_its_download() {
    let fixture = Fixture::new();
    let pointer = working_missing_pointer(&fixture);
    let repo = GitRepository::open(&fixture.0).unwrap();
    let paths = repo
        .search_tracked_paths(
            &gitturtle_core::PathScope::Worktree,
            "notes",
            &gitturtle_core::HistoryCancellation::default(),
        )
        .unwrap();
    let entry = paths.entries[0].clone();
    let file = repo.tracked_path_change(&entry);
    let Output::TrackedPreview { content, .. } = execute(
        Job::TrackedPreview {
            repo,
            entry,
            scope: gitturtle_core::PathScope::Worktree,
        },
        &mut PreviewCache::default(),
        &active(),
        &mut RepositorySession::default(),
    )
    .unwrap() else {
        panic!("expected quick source")
    };
    let Content::Text {
        new, lfs_missing, ..
    } = &*content
    else {
        panic!("expected the pointer's text")
    };
    assert_eq!(new, &pointer);
    assert_eq!(*lfs_missing, [false, true]);
    let targets = crate::lfs_download::preview_targets(&content, &file);
    assert_eq!(
        targets.iter().map(|(side, _)| *side).collect::<Vec<_>>(),
        [1]
    );
}

/// The same stored texts with another side's object missing are not the
/// same content: what the file says and offers depends on it, and it
/// changes without the pointer's text changing.
#[test]
fn a_change_in_which_lfs_object_is_missing_is_a_content_change() {
    let text = |missing: [bool; 2]| {
        let mut content = prepared_text("patch".into(), "old".into(), "new".into(), false);
        LfsSides {
            missing,
            ..Default::default()
        }
        .mark(&mut content);
        content
    };
    assert!(content_unchanged(
        &text([true, false]),
        &text([true, false])
    ));
    assert!(!content_unchanged(
        &text([true, false]),
        &text([true, true])
    ));
}

/// A snapshot names each commit's tags after its branches, by name, an
/// annotated tag at the commit it peels to; a tag of a blob or a tree
/// names no commit. Each commit's strings are prepared once: the
/// tooltip's lines and the accessible name's list say "tag ‹name›",
/// "+N" counts the tags, and the most references any commit has is kept.
#[test]
fn snapshots_name_each_commits_tags_after_its_branches() {
    let fixture = Fixture::new();
    fixture.git(&["symbolic-ref", "HEAD", "refs/heads/main"], &[]);
    let tree = fixture.git(&["mktree"], &[]);
    let first = fixture.git(&["commit-tree", &tree, "-m", "first"], &[]);
    let second = fixture.git(&["commit-tree", &tree, "-p", &first, "-m", "second"], &[]);
    fixture.git(&["update-ref", "refs/heads/main", &second], &[]);
    fixture.git(&["update-ref", "refs/heads/side", &first], &[]);
    fixture.git(&["tag", "v0.1", &first], &[]);
    fixture.git(&["tag", "-a", "-m", "Release", "v1.0", &second], &[]);
    fixture.git(&["tag", "a-first", &first], &[]);
    let blob = fixture.blob(b"notes\n");
    fixture.git(&["tag", "blob", &blob], &[]);
    fixture.git(&["tag", "-a", "-m", "Tree", "tree", &tree], &[]);
    let snapshot = read_snapshot(
        fixture.0.clone(),
        (None, None),
        None,
        MissingScope::Fail,
        10,
        &active(),
        &mut RepositorySession::default(),
    )
    .unwrap();
    let names = |oid: &str| {
        snapshot.refs.get(oid).map(|refs| {
            refs.names
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(names(&first).unwrap(), ["side", "a-first", "v0.1"]);
    assert_eq!(names(&second).unwrap(), ["main", "v1.0"]);
    assert_eq!(names(&blob), None);
    assert_eq!(names(&tree), None);
    let first = snapshot.refs.get(&first).unwrap();
    assert_eq!(first.branches, 1);
    assert!(!first.leads_with_tag());
    assert_eq!(first.more.as_deref(), Some("+2"));
    assert_eq!(first.lines.as_ref(), "side\ntag a-first\ntag v0.1");
    assert_eq!(first.listed.as_ref(), "side, tag a-first, tag v0.1");
    assert_eq!(snapshot.refs.widest(), 3);

    // A commit with tags alone leads with a tag.
    fixture.git(&["update-ref", "-d", "refs/heads/side"], &[]);
    let snapshot = read_snapshot(
        fixture.0.clone(),
        (None, None),
        None,
        MissingScope::Fail,
        10,
        &active(),
        &mut RepositorySession::default(),
    )
    .unwrap();
    let first = snapshot.refs.get(&snapshot.commits[1].oid).unwrap();
    assert!(first.leads_with_tag());
    assert_eq!(first.listed.as_ref(), "tag a-first, tag v0.1");
    assert_eq!(snapshot.refs.widest(), 2);
}

/// The tag read runs beside the history page; a tag list Git cannot give,
/// here because of a symbolic tag reference, leaves the snapshot with its
/// history and branches and without any tags.
#[test]
fn a_tag_list_git_cannot_give_leaves_history_without_tags() {
    let fixture = Fixture::new();
    fixture.git(&["symbolic-ref", "HEAD", "refs/heads/main"], &[]);
    let tree = fixture.git(&["mktree"], &[]);
    let first = fixture.git(&["commit-tree", &tree, "-m", "first"], &[]);
    let second = fixture.git(&["commit-tree", &tree, "-p", &first, "-m", "second"], &[]);
    fixture.git(&["update-ref", "refs/heads/main", &second], &[]);
    fixture.git(&["tag", "v1.0", &second], &[]);
    fixture.git(&["tag", "v0.1", &first], &[]);
    fixture.git(
        &["symbolic-ref", "refs/tags/latest", "refs/heads/main"],
        &[],
    );
    let repository = GitRepository::open(&fixture.0).unwrap();
    assert!(repository.tags().is_err(), "Git cannot give this tag list");
    let snapshot = read_snapshot(
        fixture.0.clone(),
        (None, None),
        None,
        MissingScope::Fail,
        10,
        &active(),
        &mut RepositorySession::default(),
    )
    .unwrap();
    assert_eq!(
        snapshot
            .commits
            .iter()
            .map(|commit| commit.oid.as_str())
            .collect::<Vec<_>>(),
        [second.as_str(), first.as_str()]
    );
    let names = snapshot.refs.get(&second).unwrap();
    assert_eq!(names.names, ["main"]);
    assert_eq!(names.listed.as_ref(), "main");
    assert_eq!(snapshot.refs.get(&first), None);
    assert_eq!(snapshot.refs.widest(), 1);
}

#[test]
fn refreshing_branch_and_worktree_scopes_resolves_external_tip_movement() {
    let fixture = Fixture::new();
    fixture.git(&["symbolic-ref", "HEAD", "refs/heads/main"], &[]);
    let tree = fixture.git(&["mktree"], &[]);
    let first = fixture.git(&["commit-tree", &tree, "-m", "first"], &[]);
    fixture.git(&["update-ref", "refs/heads/main", &first], &[]);
    let scope = Scope::Branch {
        name: "main".into(),
        remote: false,
    };
    let mut session = RepositorySession::default();
    let before = read_snapshot(
        fixture.0.clone(),
        (None, None),
        Some(&scope),
        MissingScope::Fail,
        10,
        &active(),
        &mut session,
    )
    .unwrap();
    assert_eq!(before.commits[0].oid, first);
    let worktree_scope = Scope::Worktree {
        path: before.repository.path().to_owned(),
    };
    let second = fixture.git(&["commit-tree", &tree, "-p", &first, "-m", "second"], &[]);
    fixture.git(&["update-ref", "refs/heads/main", &second], &[]);
    for scope in [&scope, &worktree_scope] {
        let refreshed = read_snapshot(
            fixture.0.clone(),
            (None, None),
            Some(scope),
            MissingScope::Fail,
            10,
            &active(),
            &mut session,
        )
        .unwrap();
        assert_eq!(refreshed.commits[0].oid, second);
        assert_eq!(refreshed.commits[1].oid, first);
    }
    // A missing selection must never silently fall back to unrelated refs.
    fixture.git(&["update-ref", "-d", "refs/heads/main"], &[]);
    let error = read_snapshot(
        fixture.0.clone(),
        (None, None),
        Some(&scope),
        MissingScope::Fail,
        10,
        &active(),
        &mut session,
    )
    .err()
    .expect("deleted branch must fail");
    assert!(error.to_string().contains("no longer exists"));
}

#[test]
fn explicit_open_with_a_vanished_scope_reads_all_history_and_names_the_scope() {
    let fixture = Fixture::new();
    fixture.git(&["symbolic-ref", "HEAD", "refs/heads/main"], b"");
    let tree = fixture.git(&["mktree"], b"");
    let first = fixture.git(&["commit-tree", &tree, "-m", "first"], b"");
    let side = fixture.git(&["commit-tree", &tree, "-p", &first, "-m", "side"], b"");
    fixture.git(&["update-ref", "refs/heads/main", &first], b"");
    fixture.git(&["update-ref", "refs/heads/departing", &side], b"");
    fixture.git(&["update-ref", "refs/heads/kept", &side], b"");
    let linked = fixture.0.with_file_name(format!(
        "{}-linked",
        fixture.0.file_name().unwrap().to_string_lossy()
    ));
    fixture.git(
        &["worktree", "add", "-q", linked.to_str().unwrap(), "kept"],
        b"",
    );
    let mut session = RepositorySession::default();
    let open = |scope: Scope, session: &mut RepositorySession| {
        let Output::Snapshot(snapshot) = execute(
            Job::Open {
                path: fixture.0.clone(),
                scope: Some(scope),
                limit: 10,
                root: None,
                git_dir: None,
            },
            &mut PreviewCache::default(),
            &active(),
            session,
        )
        .unwrap() else {
            panic!("expected snapshot")
        };
        snapshot
    };
    let branch = Scope::Branch {
        name: "departing".into(),
        remote: false,
    };
    let scoped = open(branch.clone(), &mut session);
    assert_eq!(scoped.vanished_scope, None);
    let worktree = Scope::Worktree {
        path: scoped
            .worktrees
            .iter()
            .find(|worktree| worktree.branch.as_deref() == Some("kept"))
            .expect("linked worktree")
            .path
            .clone(),
    };
    assert_eq!(open(worktree.clone(), &mut session).vanished_scope, None);

    fixture.git(&["update-ref", "-d", "refs/heads/departing"], b"");
    fixture.git(
        &["worktree", "remove", "--force", linked.to_str().unwrap()],
        b"",
    );
    for scope in [branch, worktree] {
        let snapshot = open(scope.clone(), &mut session);
        assert_eq!(
            snapshot.vanished_scope,
            Some(VanishedScope(scope.clone())),
            "{scope:?}"
        );
        assert!(
            matches!(
                snapshot.history_scope,
                gitturtle_core::HistoryScope::PinnedRefs(_)
            ),
            "All history pins every current ref"
        );
        assert_eq!(
            snapshot
                .commits
                .iter()
                .map(|commit| commit.oid.as_str())
                .collect::<Vec<_>>(),
            vec![side.as_str(), first.as_str()],
            "{scope:?} reads the repository's commits from current refs"
        );
    }
    // Quiet refresh keeps failing so the displayed history is retained.
    let error = read_snapshot(
        fixture.0.clone(),
        (None, None),
        Some(&Scope::Branch {
            name: "departing".into(),
            remote: false,
        }),
        MissingScope::Fail,
        10,
        &active(),
        &mut session,
    )
    .err()
    .expect("a strict read fails");
    assert!(error.downcast_ref::<VanishedScope>().is_some());
}

#[test]
fn retained_session_reopens_replaced_repository_at_the_same_path() {
    for replace_root in [true, false] {
        let fixture = Fixture::new();
        let replacement = Fixture::new();
        let old_blob = fixture.blob(b"old repository object\n");
        let new_blob = replacement.blob(b"replacement repository object\n");
        let mut session = RepositorySession::default();
        assert_eq!(
            session
                .open(&fixture.0, None, None)
                .unwrap()
                .blob(&old_blob)
                .unwrap(),
            b"old repository object\n"
        );
        let retained = Arc::downgrade(&session.current.as_ref().unwrap().repository);
        let preserved = tempfile::tempdir().unwrap();
        let (original, incoming) = if replace_root {
            (fixture.0.clone(), replacement.0.clone())
        } else {
            (fixture.0.join(".git"), replacement.0.join(".git"))
        };
        fs::rename(&original, preserved.path().join("original")).unwrap();
        fs::rename(&incoming, &original).unwrap();
        let reopened = session.open(&fixture.0, None, None).unwrap();
        assert_eq!(
            reopened.blob(&new_blob).unwrap(),
            b"replacement repository object\n"
        );
        assert!(
            retained.upgrade().is_none(),
            "replacement must release the previous object reader owner"
        );
    }
}

#[test]
#[cfg(unix)]
fn retained_session_reopens_in_place_gitdir_redirection_with_preserved_mtime() {
    use std::os::unix::fs::MetadataExt;
    let fixture = Fixture::new();
    let replacement = Fixture::new();
    let old_blob = fixture.blob(b"old linked repository object\n");
    let new_blob = replacement.blob(b"new linked repository object\n");
    let directories = tempfile::tempdir().unwrap();
    let original = directories.path().join("admin-a");
    let incoming = directories.path().join("admin-b");
    fs::rename(fixture.0.join(".git"), &original).unwrap();
    fs::rename(replacement.0.join(".git"), &incoming).unwrap();
    let git_link = fixture.0.join(".git");
    let old_pointer = format!("gitdir: {}\n", original.display());
    let new_pointer = format!("gitdir: {}\n", incoming.display());
    assert_eq!(old_pointer.len(), new_pointer.len());
    fs::write(&git_link, old_pointer).unwrap();
    let mut session = RepositorySession::default();
    assert_eq!(
        session
            .open(&fixture.0, None, None)
            .unwrap()
            .blob(&old_blob)
            .unwrap(),
        b"old linked repository object\n"
    );
    let before = fs::metadata(&git_link).unwrap();
    std::thread::sleep(Duration::from_millis(2));
    let mut file = fs::OpenOptions::new().write(true).open(&git_link).unwrap();
    file.write_all(new_pointer.as_bytes()).unwrap();
    file.set_modified(before.modified().unwrap()).unwrap();
    drop(file);
    let after = fs::metadata(&git_link).unwrap();
    assert_eq!(before.ino(), after.ino());
    assert_eq!(before.len(), after.len());
    assert_eq!(before.modified().unwrap(), after.modified().unwrap());
    assert!(original.is_dir() && incoming.is_dir());
    assert_eq!(
        session
            .open(&fixture.0, None, None)
            .unwrap()
            .blob(&new_blob)
            .unwrap(),
        b"new linked repository object\n"
    );
}

#[test]
fn retained_session_survives_ui_clear_and_nested_opens_but_separates_worktrees() {
    let fixture = Fixture::new();
    fixture.git(&["symbolic-ref", "HEAD", "refs/heads/main"], &[]);
    let tree = fixture.git(&["mktree"], &[]);
    let oid = fixture.git(&["commit-tree", &tree, "-m", "initial"], &[]);
    fixture.git(&["update-ref", "refs/heads/main", &oid], &[]);
    let blob = fixture.blob(b"retained object reader\n");
    let mut session = RepositorySession::default();
    let ui_repository = session.open(&fixture.0, None, None).unwrap();
    assert_eq!(
        ui_repository.blob(&blob).unwrap(),
        b"retained object reader\n"
    );
    let retained = Arc::downgrade(&session.current.as_ref().unwrap().repository);
    drop(ui_repository);
    assert!(
        retained.upgrade().is_some(),
        "clearing UI must retain the process owner"
    );

    let nested = fixture.0.join("nested/directory");
    fs::create_dir_all(&nested).unwrap();
    for path in [&fixture.0, &nested] {
        let reopened = session.open(path, None, None).unwrap();
        assert!(Arc::ptr_eq(
            &retained.upgrade().unwrap(),
            &session.current.as_ref().unwrap().repository,
        ));
        assert_eq!(reopened.blob(&blob).unwrap(), b"retained object reader\n");
    }

    let linked = fixture.0.join("linked-worktree");
    fixture.git(
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "feature",
            linked.to_str().unwrap(),
        ],
        &[],
    );
    let linked_repo = session.open(&linked, None, None).unwrap();
    assert!(
        retained.upgrade().is_none(),
        "old retained owner is released on session replacement"
    );
    assert_eq!(linked_repo.path(), linked.canonicalize().unwrap());
    assert_eq!(
        linked_repo
            .branches()
            .unwrap()
            .iter()
            .find(|b| b.current)
            .unwrap()
            .name,
        "feature"
    );
    let main_repo = session.open(&fixture.0, None, None).unwrap();
    assert_eq!(
        main_repo
            .branches()
            .unwrap()
            .iter()
            .find(|b| b.current)
            .unwrap()
            .name,
        "main"
    );
}

/// A tab's passive reopen finds the repository it opened or none. With
/// the inner repository's `.git` gone, Git discovery from its root finds
/// the work tree enclosing it: an explicit open takes that work tree, as
/// a folder inside one opens it, while a quiet read from a session that
/// retained the inner repository, Changes' working read and a reopen by
/// the captured root each fail as `RepositoryGone`, naming it, and read
/// nothing of it, the enclosing repository's working state included.
#[test]
fn a_reopen_finds_the_repository_it_opened_or_none() {
    let outer = Fixture::new();
    let inner = outer.0.join("inner");
    fs::create_dir(&inner).unwrap();
    let init = Fixture::command(&inner)
        .args(["init", "--quiet"])
        .output()
        .unwrap();
    assert!(init.status.success());
    let mut session = RepositorySession::default();
    let opened = session.open(&inner, None, None).unwrap();
    let root = opened.path().to_owned();
    assert_eq!(session.open(&root, Some(&root), None).unwrap().path(), root);
    assert!(WorkingState::read(&opened).is_ok());

    fs::rename(inner.join(".git"), outer.0.join("inner-git-away")).unwrap();
    fs::write(outer.0.join("outer-change.txt"), "outer\n").unwrap();
    let outer_root = outer.0.canonicalize().unwrap();
    let gone = |error: anyhow::Error| {
        let gone = error
            .downcast::<RepositoryGone>()
            .expect("the repository is gone");
        assert_eq!(gone.path, root);
        assert_eq!(
            gone.found.map(|found| found.canonicalize().unwrap()),
            Some(outer_root.clone()),
            "it names the repository Git finds there"
        );
    };
    let quiet = execute(
        Job::QuietRefresh {
            repo: opened.clone(),
            scope: None,
            limit: 10,
            history: false,
            selected: None,
        },
        &mut PreviewCache::default(),
        &active(),
        &mut session,
    );
    match quiet {
        Ok(_) => panic!("a quiet read read the enclosing repository"),
        Err(error) => gone(error),
    }
    match WorkingState::read(&opened) {
        Ok(_) => panic!("Changes read the enclosing repository"),
        Err(error) => gone(error),
    }
    gone(session.open(&root, Some(&root), None).unwrap_err());
    let error = execute(
        Job::Open {
            path: root.clone(),
            scope: None,
            limit: 10,
            root: Some(root.clone()),
            git_dir: None,
        },
        &mut PreviewCache::default(),
        &active(),
        &mut session,
    )
    .err()
    .expect("Refresh fails");
    // A tab with no Git directories captured, as an older session's:
    // `.git` gone or never there is the same on disk, so it says what
    // works for both (`NoRepositoryOfItsOwn`).
    assert!(
        format!("{error:#}").starts_with(&format!(
            "This tab's folder {} has no repository of its own: Git finds the repository at ",
            root.display()
        )),
        "said in its own words: {error:#}"
    );
    assert_eq!(
        session
            .open(&root, None, None)
            .unwrap()
            .path()
            .canonicalize()
            .unwrap(),
        outer_root,
        "an explicit open takes the enclosing work tree"
    );

    // With the enclosing repository's `.git` gone too, Git finds no
    // repository at the root: Refresh fails with the reasons any folder
    // that is not a repository gets, never "is gone", and the working
    // read still tells Changes the tab's repository is gone, with
    // nothing found, so it marks the tab through that Open.
    fs::rename(outer.0.join(".git"), outer.0.join("outer-git-away")).unwrap();
    let error = execute(
        Job::Open {
            path: root.clone(),
            scope: None,
            limit: 10,
            root: Some(root.clone()),
            git_dir: None,
        },
        &mut PreviewCache::default(),
        &active(),
        &mut session,
    )
    .err()
    .expect("Refresh fails");
    assert!(!error.is::<RepositoryGone>(), "not gone: {error:#}");
    let said = format!("{error:#}");
    assert!(
        said.starts_with("This folder is not an available Git repository.")
            && said.contains(&format!("Folder: {}", root.display())),
        "repository_access::explain's reasons: {said}"
    );
    match WorkingState::read(&opened) {
        Ok(_) => panic!("Changes read a folder that is not a repository"),
        Err(error) => assert_eq!(
            error.downcast::<RepositoryGone>().expect("gone"),
            RepositoryGone {
                path: root.clone(),
                found: None
            }
        ),
    }
}

/// A tab's repository is its root and the Git directory Git reported
/// when it opened, compared as captured: with its `.git` swapped for
/// another repository's gitfile, or its root turned into a link to
/// another repository, Changes' working read, a quiet read and a
/// reopen by the captured root each fail as `RepositoryGone`, naming
/// what Git finds, and read nothing of the other repository; the
/// session never adopts it. Restored, the tab reads again.
#[cfg(unix)]
#[test]
fn a_reopen_never_takes_a_swapped_git_entry_or_a_linked_root() {
    let fixture = Fixture::new();
    let other = Fixture::new();
    let mut session = RepositorySession::default();
    let opened = session.open(&fixture.0, None, None).unwrap();
    let root = opened.path().to_owned();
    let other_repository = GitRepository::open(&other.0).unwrap();
    let gone = |error: anyhow::Error, found: &Path| {
        let gone = error
            .downcast::<RepositoryGone>()
            .expect("the repository is gone");
        assert_eq!(gone.path, root);
        assert_eq!(
            gone.found.map(|found| found.canonicalize().unwrap()),
            Some(found.to_owned()),
            "it names what Git finds there"
        );
    };
    let quiet = |session: &mut RepositorySession| {
        execute(
            Job::QuietRefresh {
                repo: opened.clone(),
                scope: None,
                limit: 10,
                history: false,
                selected: None,
            },
            &mut PreviewCache::default(),
            &active(),
            session,
        )
        .err()
        .expect("a quiet read reads nothing")
    };

    let away = fixture.0.with_extension("git-away");
    fs::rename(fixture.0.join(".git"), &away).unwrap();
    fs::write(
        fixture.0.join(".git"),
        format!("gitdir: {}\n", other_repository.git_dir().display()),
    )
    .unwrap();
    let other_git = other_repository.git_dir();
    let captured = GitDirectories::of(&opened);
    gone(WorkingState::read(&opened).err().expect("gone"), other_git);
    gone(quiet(&mut session), other_git);
    // A reopen by the tab's captured root and Git directory, warm, and
    // cold after the session opened another repository: the session
    // keeps no memory of the tab, which names its own Git directory.
    gone(
        session
            .open(&root, Some(&root), Some(&captured))
            .unwrap_err(),
        other_git,
    );
    let third = Fixture::new();
    session.open(&third.0, None, None).unwrap();
    gone(
        session
            .open(&root, Some(&root), Some(&captured))
            .unwrap_err(),
        other_git,
    );
    fs::remove_file(fixture.0.join(".git")).unwrap();
    fs::rename(&away, fixture.0.join(".git")).unwrap();
    assert_eq!(
        session
            .open(&root, Some(&root), Some(&captured))
            .unwrap()
            .path(),
        root
    );
    assert!(WorkingState::read(&opened).is_ok());

    let moved = fixture.0.with_extension("moved");
    fs::rename(&fixture.0, &moved).unwrap();
    std::os::unix::fs::symlink(&other.0, &fixture.0).unwrap();
    let other_root = other_repository.path().canonicalize().unwrap();
    gone(
        WorkingState::read(&opened).err().expect("gone"),
        &other_root,
    );
    gone(quiet(&mut session), &other_root);
    gone(
        session.open(&root, Some(&root), None).unwrap_err(),
        &other_root,
    );
    fs::remove_file(&fixture.0).unwrap();
    fs::rename(&moved, &fixture.0).unwrap();
}

/// A tab's root reached through a folder that is a link, as macOS
/// reaches every temporary folder through `/var` (a link to
/// `/private/var`), is the repository there, warm and cold: saved
/// through the link with no Git directories captured yet, and captured
/// at its real root with its Git directories, its folder then moved and
/// linked back from its old place. Compared as captured, each reopen
/// found the repository gone and the tab was marked " · unavailable",
/// where it opened before. The root itself turned into a link to
/// another repository, reached through the linked folder, is still
/// another repository.
#[cfg(unix)]
#[test]
fn a_reopen_through_a_linked_folder_finds_the_repository_it_captured() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let real = base.join("real");
    let root = real.join("repository");
    fs::create_dir_all(&root).unwrap();
    let init = Fixture::command(&root)
        .args(["init", "--quiet"])
        .output()
        .unwrap();
    assert!(init.status.success());
    let link = base.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let through = link.join("repository");
    let other = Fixture::new();
    let mut session = RepositorySession::default();
    let reopen = |session: &mut RepositorySession,
                  at: &Path,
                  git_dir: Option<&GitDirectories>,
                  cold: bool| {
        if cold {
            session.open(&other.0, None, None).unwrap();
        }
        session.open(at, Some(at), git_dir)
    };
    let opened = reopen(&mut session, &through, None, false)
        .unwrap_or_else(|error| panic!("saved through the link: {error:#}"));
    assert_eq!(opened.path(), root, "saved through the link");
    let captured = GitDirectories::of(&opened);
    for cold in [false, true] {
        let reopened = reopen(&mut session, &through, Some(&captured), cold)
            .unwrap_or_else(|error| panic!("through the link, cold {cold}: {error:#}"));
        assert_eq!(reopened.path(), root, "through the link, cold {cold}");
    }

    let moved = base.join("moved");
    fs::rename(&real, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &real).unwrap();
    let now = moved.join("repository");
    for cold in [false, true] {
        let reopened = reopen(&mut session, &root, Some(&captured), cold)
            .unwrap_or_else(|error| panic!("moved, cold {cold}: {error:#}"));
        assert_eq!(reopened.path(), now, "moved, cold {cold}");
    }

    fs::rename(&now, moved.join("repository-away")).unwrap();
    std::os::unix::fs::symlink(&other.0, &now).unwrap();
    let other_root = other.0.canonicalize().unwrap();
    for git_dir in [None, Some(&captured)] {
        let gone = reopen(&mut session, &through, git_dir, false)
            .expect_err("a root turned into a link is another repository")
            .downcast::<RepositoryGone>()
            .expect("gone");
        assert_eq!(gone.found.as_deref(), Some(other_root.as_path()));
    }
}

/// A folder made to hold one repository, `git init`ed.
#[cfg(unix)]
fn init_at(path: &Path) {
    fs::create_dir_all(path).unwrap();
    let init = Fixture::command(path)
        .args(["init", "--quiet"])
        .output()
        .unwrap();
    assert!(init.status.success());
}

/// A restored tab compares the inodes it saved, warm and cold: a folder
/// above its root re-pointed to another repository with the root's
/// folder name resolves to the same paths through that folder, and is
/// refused naming that repository's Git directory, the mark a swapped
/// `.git` gets. Moved on the one filesystem and linked back, the
/// repository keeps its inodes and reopens. A tab opened in this run
/// compares none, and neither does an older session that saved none.
#[cfg(unix)]
#[test]
fn a_restored_tab_tells_its_repository_by_the_inodes_it_saved() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let projects = base.join("projects");
    let root = projects.join("repository");
    init_at(&root);
    let other = Fixture::new();
    let mut session = RepositorySession::default();
    let opened = session.open(&root, None, None).unwrap();
    let captured = GitDirectories::of(&opened);
    assert!(!captured.restored, "of compares nothing");
    assert!(captured.inodes.git_dir.is_some());
    let restored = GitDirectories {
        restored: true,
        ..captured.clone()
    };
    let reopen = |session: &mut RepositorySession, named: &GitDirectories, cold: bool| {
        if cold {
            session.open(&other.0, None, None).unwrap();
        }
        session.open(&root, Some(&root), Some(named))
    };

    let moved = base.join("moved");
    fs::rename(&projects, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &projects).unwrap();
    for cold in [false, true] {
        let reopened = reopen(&mut session, &restored, cold)
            .unwrap_or_else(|error| panic!("moved, cold {cold}: {error:#}"));
        assert_eq!(reopened.path(), moved.join("repository"), "cold {cold}");
    }
    fs::remove_file(&projects).unwrap();
    fs::rename(&moved, &projects).unwrap();

    // The repository kept aside, so its inodes stay taken.
    let away = base.join("away");
    fs::rename(&projects, &away).unwrap();
    let elsewhere = base.join("elsewhere");
    let found = elsewhere.join("repository");
    init_at(&found);
    std::os::unix::fs::symlink(&elsewhere, &projects).unwrap();
    for cold in [false, true] {
        let gone = reopen(&mut session, &restored, cold)
            .expect_err("another repository by the root's folder name")
            .downcast::<RepositoryGone>()
            .expect("gone");
        assert_eq!(gone.path, root, "cold {cold}");
        assert_eq!(
            gone.found.as_deref(),
            Some(found.join(".git").as_path()),
            "cold {cold}"
        );
    }
    let older = GitDirectories {
        inodes: DirectoryInodes::default(),
        ..restored
    };
    for named in [&captured, &older] {
        assert_eq!(
            reopen(&mut session, named, true)
                .unwrap_or_else(|error| panic!("{named:?}: {error:#}"))
                .path(),
            found,
            "{named:?}"
        );
    }
}

/// Each saved inode is compared on its own, the Git directory's before
/// the common directory's, and the retained repository answers only
/// where they match: named with a different one, the reopen asks Git
/// and fails as gone, naming that directory.
#[cfg(unix)]
#[test]
fn a_restored_tab_compares_each_directory_s_inode() {
    let fixture = Fixture::new();
    let mut session = RepositorySession::default();
    let opened = session.open(&fixture.0, None, None).unwrap();
    let root = opened.path().to_owned();
    let captured = GitDirectories::of(&opened);
    let inode = |inode: Option<u64>| Some(inode.expect("an inode") + 1);
    let git_dir = GitDirectories {
        inodes: DirectoryInodes {
            git_dir: inode(captured.inodes.git_dir),
            ..captured.inodes
        },
        restored: true,
        ..captured.clone()
    };
    let common = GitDirectories {
        inodes: DirectoryInodes {
            common: inode(captured.inodes.common),
            ..captured.inodes
        },
        restored: true,
        ..captured.clone()
    };
    for (named, found) in [(&git_dir, &captured.git_dir), (&common, &captured.common)] {
        let gone = session
            .open(&root, Some(&root), Some(named))
            .unwrap_err()
            .downcast::<RepositoryGone>()
            .expect("gone");
        assert_eq!(gone.found.as_deref(), Some(found.as_path()));
    }
    let matching = GitDirectories {
        restored: true,
        ..captured
    };
    assert_eq!(
        session
            .open(&root, Some(&root), Some(&matching))
            .unwrap()
            .path(),
        root
    );
}

/// A saved birth time is compared beside the inode, each directory's on
/// its own, so a directory created after the saved one was deleted that
/// took its inode number (ext4 and XFS reuse them) is another
/// repository: here the same inodes with a later birth. A session saved
/// before birth times were kept, or a filesystem that records none, has
/// the inodes alone, which are still compared.
#[cfg(unix)]
#[test]
fn a_restored_tab_compares_each_directory_s_birth_time() {
    let fixture = Fixture::new();
    let mut session = RepositorySession::default();
    let opened = session.open(&fixture.0, None, None).unwrap();
    let root = opened.path().to_owned();
    let captured = GitDirectories::of(&opened);
    if captured.inodes.git_dir_birth.is_none() {
        // This filesystem records none: the inodes are all there is,
        // which `a_restored_tab_compares_each_directory_s_inode`
        // compares. Said, so a run here is not read as a pass.
        eprintln!(
            "a_restored_tab_compares_each_directory_s_birth_time skipped: \
             the filesystem holding {} records no birth time",
            root.display()
        );
        return;
    }
    let later =
        |birth: Option<Duration>| Some(birth.expect("a birth time") + Duration::from_nanos(1));
    let restored = |inodes: DirectoryInodes| GitDirectories {
        inodes,
        restored: true,
        ..captured.clone()
    };
    let git_dir = restored(DirectoryInodes {
        git_dir_birth: later(captured.inodes.git_dir_birth),
        ..captured.inodes
    });
    let common = restored(DirectoryInodes {
        common_birth: later(captured.inodes.common_birth),
        ..captured.inodes
    });
    let older = restored(DirectoryInodes {
        git_dir_birth: None,
        common_birth: None,
        ..captured.inodes
    });
    let older_elsewhere = restored(DirectoryInodes {
        git_dir: captured.inodes.git_dir.map(|inode| inode + 1),
        ..older.inodes
    });
    for (named, found) in [
        (&git_dir, &captured.git_dir),
        (&common, &captured.common),
        (&older_elsewhere, &captured.git_dir),
    ] {
        let gone = session
            .open(&root, Some(&root), Some(named))
            .unwrap_err()
            .downcast::<RepositoryGone>()
            .expect("gone");
        assert_eq!(gone.found.as_deref(), Some(found.as_path()), "{named:?}");
    }
    for named in [&restored(captured.inodes), &older] {
        assert_eq!(
            session
                .open(&root, Some(&root), Some(named))
                .unwrap_or_else(|error| panic!("{named:?}: {error:#}"))
                .path(),
            root
        );
    }
}

/// The session's retained repository is dropped once its Git directory
/// is born again with the same device and inode, as an overlayfs
/// copy-up leaves it (and ext4 or XFS giving a re-clone the deleted
/// directory's inode number would), so the next open captures it
/// afresh, the new birth time with it. It once compared the device and
/// inode alone and returned the handle of its first open, whose birth
/// time every capture in the run was then compared with, itself
/// included, so only a restart saw the new directory.
#[cfg(unix)]
#[test]
fn a_session_drops_a_repository_whose_git_directory_was_born_again() {
    let fixture = Fixture::new();
    let mut session = RepositorySession::default();
    let git_dir = GitRepository::open(&fixture.0)
        .unwrap()
        .git_dir()
        .to_owned();
    let born = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    repository_identity::born_for_test(Some((git_dir.clone(), born)));
    let opened = session.open(&fixture.0, None, None).unwrap();
    let root = opened.path().to_owned();
    let captured = GitDirectories::of(&opened);
    let retained = |session: &RepositorySession| {
        Arc::clone(&session.current.as_ref().expect("retained").repository)
    };
    let first = retained(&session);
    session.open(&root, Some(&root), Some(&captured)).unwrap();
    assert!(
        Arc::ptr_eq(&first, &retained(&session)),
        "the same directory keeps the retained repository"
    );

    let again = born + Duration::from_secs(1);
    repository_identity::born_for_test(Some((git_dir.clone(), again)));
    let reopened = session.open(&root, Some(&root), Some(&captured));
    let holds = (session.current.as_ref()).map(|current| current.identity.is_current());
    repository_identity::born_for_test(None);
    assert_eq!(reopened.unwrap().git_dir(), git_dir);
    assert!(
        !Arc::ptr_eq(&first, &retained(&session)),
        "a Git directory born again is captured afresh"
    );
    assert_eq!(holds, Some(true), "the new capture read the new birth time");
}

/// A repository cloned again at the tab's path has new inodes, which
/// cannot be told from another repository re-pointed there, so a
/// restored tab refuses it the same way; an explicit open has no saved
/// fields and adopts it, and a tab that then holds its own passes.
#[cfg(unix)]
#[test]
fn a_re_clone_is_refused_to_a_restored_tab_and_adopted_by_an_open() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let root = base.join("repository");
    init_at(&root);
    let mut session = RepositorySession::default();
    let captured = GitDirectories::of(&session.open(&root, None, None).unwrap());
    let restored = GitDirectories {
        restored: true,
        ..captured.clone()
    };
    // Moved aside rather than deleted, which could free the inode for
    // the new `.git` to take.
    fs::rename(&root, base.join("before")).unwrap();
    init_at(&root);
    let gone = session
        .open(&root, Some(&root), Some(&restored))
        .expect_err("a new repository at the path")
        .downcast::<RepositoryGone>()
        .expect("gone");
    assert_eq!(gone.found.as_deref(), Some(root.join(".git").as_path()));

    let adopted = GitDirectories::of(&session.open(&root, None, None).unwrap());
    assert_ne!(adopted.inodes.git_dir, captured.inodes.git_dir);
    assert_eq!(
        session
            .open(&root, Some(&root), Some(&adopted))
            .unwrap()
            .path(),
        root
    );
}

/// An open refused because only the capture failed, Git still
/// reporting the repository at its path, says what is true in core's
/// words: not gone, and the one path not named twice. One that Git now
/// finds elsewhere is the repository gone, as before.
#[test]
fn an_open_refused_at_its_own_path_is_not_the_repository_gone() {
    let root = PathBuf::from("/repository");
    let refused = |found: Option<PathBuf>| {
        let error = anyhow::Error::from(gitturtle_core::CoreError::ChangedWhileOpening {
            path: root.clone(),
            found,
        });
        worded_open_failure(&root, refused_open(&root, false, error))
    };
    let same = refused(Some(root.clone()));
    assert!(
        matches!(
            gitturtle_core::CoreError::from_anyhow(&same),
            Some(gitturtle_core::CoreError::ChangedWhileOpening { .. })
        ),
        "{same:#}"
    );
    assert_eq!(
        format!("{same:#}"),
        "The repository at /repository changed while it was opened. Open it again."
    );
    let elsewhere = refused(Some(PathBuf::from("/other")));
    assert!(elsewhere.is::<RepositoryGone>(), "{elsewhere:#}");
    assert!(
        format!("{elsewhere:#}").contains("Git now finds the repository at /other there"),
        "{elsewhere:#}"
    );
}

/// A repository whose work tree lies outside the folder Git found it
/// in: an explicit open says so in core's own words, with no reasons
/// around it and nothing to open again, and a tab's passive reopen,
/// its work tree re-pointed there, is the repository gone, naming the
/// work tree, so the tab is marked as it is for any re-point.
#[test]
fn a_work_tree_outside_its_folder_is_refused_in_core_s_words() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let root = base.join("repository");
    let elsewhere = base.join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    fs::create_dir(&root).unwrap();
    let init = Fixture::command(&root)
        .args(["init", "--quiet"])
        .output()
        .unwrap();
    assert!(init.status.success());
    let configured = Fixture::command(&root)
        .args(["config", "core.worktree"])
        .arg(&elsewhere)
        .output()
        .unwrap();
    assert!(configured.status.success());
    let said = format!(
        "GitTurtle opens a repository only inside the folder Git found it in: the work tree of the repository at {} is {} (core.worktree).",
        root.display(),
        elsewhere.display()
    );
    let mut session = RepositorySession::default();
    let explicit = session.open(&root, None, None).unwrap_err();
    assert!(
        matches!(
            gitturtle_core::CoreError::from_anyhow(&explicit),
            Some(gitturtle_core::CoreError::WorkTreeOutside { .. })
        ),
        "{explicit:#}"
    );
    let open = |path_root: Option<PathBuf>, session: &mut RepositorySession| {
        execute(
            Job::Open {
                path: root.clone(),
                scope: None,
                limit: 10,
                root: path_root,
                git_dir: None,
            },
            &mut PreviewCache::default(),
            &active(),
            session,
        )
        .err()
        .expect("the open is refused")
    };
    let error = open(None, &mut session);
    assert_eq!(format!("{error:#}"), said);
    assert!(!error.is::<RepositoryGone>(), "{error:#}");

    let reopened = session.open(&root, Some(&root), None).unwrap_err();
    assert_eq!(
        reopened.downcast::<RepositoryGone>().expect("gone"),
        RepositoryGone {
            path: root.clone(),
            found: Some(elsewhere),
        }
    );
    let error = open(Some(root.clone()), &mut session);
    assert!(error.is::<RepositoryGone>(), "marks the tab: {error:#}");
}

/// A linked worktree's `commondir` re-pointed at another repository
/// keeps its root and its own Git directory, yet its refs and objects,
/// and every write's, are that other repository's: a reopen by the
/// tab's captured identity, warm or cold, finds the repository gone,
/// naming that repository's Git directory, and with `commondir` back it
/// opens again. It once compared the root and Git directory alone, so
/// the tab took a handle bound to the other repository, and its next
/// Stage all and Commit wrote that repository's objects and branch.
#[test]
fn a_reopen_never_takes_a_re_pointed_commondir() {
    let fixture = Fixture::new();
    fixture.git(&["commit", "--quiet", "--allow-empty", "-m", "main"], &[]);
    let linked = fixture.0.join("linked");
    fixture.git(
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ],
        &[],
    );
    let mut session = RepositorySession::default();
    let opened = session.open(&linked, None, None).unwrap();
    let root = opened.path().to_owned();
    let captured = GitDirectories::of(&opened);
    assert_ne!(captured.git_dir, captured.common, "a linked worktree");
    let other = Fixture::new();
    let other_git = GitRepository::open(&other.0).unwrap().git_dir().to_owned();
    let commondir = opened.git_dir().join("commondir");
    let kept = fs::read(&commondir).unwrap();
    fs::write(&commondir, format!("{}\n", other_git.display())).unwrap();
    let gone = |error: anyhow::Error| {
        let gone = error.downcast::<RepositoryGone>().expect("gone");
        assert_eq!(gone.path, root);
        assert_eq!(gone.found.as_deref(), Some(other_git.as_path()));
    };
    gone(
        session
            .open(&root, Some(&root), Some(&captured))
            .unwrap_err(),
    );
    let third = Fixture::new();
    session.open(&third.0, None, None).unwrap();
    gone(
        session
            .open(&root, Some(&root), Some(&captured))
            .unwrap_err(),
    );
    fs::write(&commondir, kept).unwrap();
    assert_eq!(
        session
            .open(&root, Some(&root), Some(&captured))
            .unwrap()
            .path(),
        root
    );
}

/// The retained repository answers a reopen only where its own Git
/// directories are the ones the tab names: named with another common
/// directory, the reopen asks Git, which finds the repository's own,
/// and fails as gone rather than hand the retained repository over.
#[test]
fn a_retained_repository_answers_only_the_directories_it_has() {
    let fixture = Fixture::new();
    let mut session = RepositorySession::default();
    let opened = session.open(&fixture.0, None, None).unwrap();
    let root = opened.path().to_owned();
    let captured = GitDirectories::of(&opened);
    let named = GitDirectories {
        common: fixture.0.join("elsewhere"),
        ..captured.clone()
    };
    let gone = session
        .open(&root, Some(&root), Some(&named))
        .unwrap_err()
        .downcast::<RepositoryGone>()
        .expect("gone");
    assert_eq!(gone.found.as_deref(), Some(captured.common.as_path()));
    assert_eq!(
        session
            .open(&root, Some(&root), Some(&captured))
            .unwrap()
            .path(),
        root
    );
}

/// A quiet read with the `.git` swapped for another repository's
/// gitfile reads nothing of that repository. And a quiet read already
/// running when the `.git`
/// moves away (the watcher's setup rescan racing the move) fails part
/// way, its working read after the status and its History, confined to
/// the repository: the whole read is the repository gone, which marks
/// the tab, never an "Operation state: …" or "History scope changed: …"
/// banner beside the mark.
#[test]
fn a_quiet_read_whose_git_moves_part_way_is_the_repository_gone() {
    let outer = Fixture::new();
    let inner = outer.0.join("inner");
    fs::create_dir(&inner).unwrap();
    for args in [
        &["init", "--quiet"][..],
        &[
            "-c",
            "user.name=GitTurtle Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "inner",
        ],
    ] {
        let output = Fixture::command(&inner).args(args).output().unwrap();
        assert!(output.status.success());
    }
    let mut session = RepositorySession::default();
    let opened = session.open(&inner, None, None).unwrap();
    let quiet = |session: &mut RepositorySession, history: bool| {
        execute(
            Job::QuietRefresh {
                repo: opened.clone(),
                scope: None,
                limit: 10,
                history,
                selected: None,
            },
            &mut PreviewCache::default(),
            &active(),
            session,
        )
    };
    let gone = |result: Result<Output>| match result {
        Ok(_) => panic!("the read answered with another repository or a banner"),
        Err(error) => {
            let gone = error.downcast::<RepositoryGone>().expect("gone");
            assert_eq!(gone.path, opened.path());
        }
    };

    let other = Fixture::new();
    let other_git = GitRepository::open(&other.0).unwrap().git_dir().to_owned();
    let away = outer.0.join("inner-git-away");
    fs::rename(inner.join(".git"), &away).unwrap();
    fs::write(
        inner.join(".git"),
        format!("gitdir: {}\n", other_git.display()),
    )
    .unwrap();
    gone(quiet(&mut session, false));
    fs::remove_file(inner.join(".git")).unwrap();
    fs::rename(&away, inner.join(".git")).unwrap();
    assert!(quiet(&mut session, true).is_ok());

    let checkpoints = std::rc::Rc::new(std::cell::Cell::new(0));
    let (git, moved) = (inner.join(".git"), away);
    let counted = std::rc::Rc::clone(&checkpoints);
    QUIET_CHECKPOINT.with_borrow_mut(|hook| {
        *hook = Some(Box::new(move || {
            counted.set(counted.get() + 1);
            // After the status, before the rest and History.
            if counted.get() == 2 {
                fs::rename(&git, &moved).unwrap();
            }
        }))
    });
    let result = quiet(&mut session, true);
    QUIET_CHECKPOINT.with_borrow_mut(|hook| *hook = None);
    assert!(checkpoints.get() >= 2, "the move ran part way");
    gone(result);
}

/// A quiet read checks its own handle before the session's reopen:
/// with `core.worktree` re-pointed in the repository's configuration
/// after a quiet read, which changes no `.git` entry or directory the
/// session's identity holds while the work tree moves elsewhere, the
/// next quiet read is the repository gone, never the other folder's
/// status. The session alone answered with the moved work tree.
#[test]
fn a_quiet_read_after_core_worktree_is_re_pointed_is_the_repository_gone() {
    let outer = Fixture::new();
    let inner = outer.0.join("inner");
    fs::create_dir(&inner).unwrap();
    let elsewhere = outer.0.join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    fs::write(elsewhere.join("stray.txt"), "stray\n").unwrap();
    for args in [
        &["init", "--quiet"][..],
        &[
            "-c",
            "user.name=GitTurtle Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "inner",
        ],
    ] {
        let output = Fixture::command(&inner).args(args).output().unwrap();
        assert!(output.status.success());
    }
    let mut session = RepositorySession::default();
    let opened = session.open(&inner, None, None).unwrap();
    let quiet = |session: &mut RepositorySession| {
        execute(
            Job::QuietRefresh {
                repo: opened.clone(),
                scope: None,
                limit: 10,
                history: true,
                selected: None,
            },
            &mut PreviewCache::default(),
            &active(),
            session,
        )
    };
    assert!(quiet(&mut session).is_ok());
    let output = Fixture::command(&inner)
        .args(["config", "core.worktree", elsewhere.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let error = quiet(&mut session)
        .err()
        .expect("the quiet read answered with the moved work tree");
    let gone = error
        .downcast::<RepositoryGone>()
        .unwrap_or_else(|error| panic!("not gone: {error:#}"));
    assert_eq!(gone.path, opened.path());
}

/// A working read already running when the repository's `.git` moves
/// away: its status read, the parts after it fail as "not a git
/// repository", its commands confined to the repository. The read is
/// then the repository gone, which marks the tab, never an "Operation
/// state: …" banner beside the mark.
#[test]
fn a_working_read_whose_git_moves_part_way_is_the_repository_gone() {
    let outer = Fixture::new();
    let inner = outer.0.join("inner");
    fs::create_dir(&inner).unwrap();
    let init = Fixture::command(&inner)
        .args(["init", "--quiet"])
        .output()
        .unwrap();
    assert!(init.status.success());
    let opened = GitRepository::open(&inner).unwrap();
    let checkpoints = std::cell::Cell::new(0);
    let state = WorkingState::read_with_checkpoint(&opened, || {
        checkpoints.set(checkpoints.get() + 1);
        // After the status, before the rest.
        if checkpoints.get() == 2 {
            fs::rename(inner.join(".git"), outer.0.join("inner-git-away")).unwrap();
        }
        Ok(())
    })
    .unwrap();
    let operation = format!("{:#}", state.operation.as_ref().expect_err("failed"));
    assert!(operation.contains("not a git repository"), "{operation}");
    let gone = state
        .confirmed(&opened)
        .err()
        .expect("the repository is gone")
        .downcast::<RepositoryGone>()
        .expect("gone");
    assert_eq!(gone.path, opened.path());
    assert_eq!(
        gone.found.map(|found| found.canonicalize().unwrap()),
        Some(outer.0.canonicalize().unwrap())
    );
}

/// The watcher's setup failure is the repository gone whenever the
/// repository is gone or no longer one at its path, whatever the words:
/// a resolution failing as its `.git` moves is checked from the
/// filesystem, and kept its own while the repository is still there; a
/// confined "not a git repository" is Git's to decide, so a `.git` half
/// removed (HEAD deleted, the folder and `config` left), which the
/// filesystem check still takes for the same repository, is gone too.
/// That check once kept it the watcher's warning, the tab unmarked.
#[test]
fn a_watch_setup_failure_is_the_repository_gone_unless_it_is_still_there() {
    let outer = Fixture::new();
    let outer_root = outer.0.canonicalize().unwrap();
    let repository = |name: &str| {
        let root = outer.0.join(name);
        fs::create_dir(&root).unwrap();
        let init = Fixture::command(&root)
            .args(["init", "--quiet"])
            .output()
            .unwrap();
        assert!(init.status.success());
        GitRepository::open(&root).unwrap()
    };
    let gone = |repo: &GitRepository, error: anyhow::Error, after: &str| {
        let gone = gone_unless_found(repo, error)
            .downcast::<RepositoryGone>()
            .unwrap_or_else(|error| panic!("{after}: not gone: {error:#}"));
        assert_eq!(gone.path, repo.path(), "{after}");
        assert_eq!(
            gone.found.map(|found| found.canonicalize().unwrap()),
            Some(outer_root.clone()),
            "{after}: it names the repository Git finds there"
        );
    };
    // As `git_directories` fails resolving a directory moved away.
    let resolution = || {
        use anyhow::Context as _;
        outer
            .0
            .join("nowhere")
            .canonicalize()
            .context("Resolve Git administration directory")
            .expect_err("nothing there")
    };

    let moved = repository("moved");
    let kept = gone_unless_found(&moved, resolution());
    assert!(!kept.is::<RepositoryGone>(), "still there: {kept:#}");
    assert_eq!(format!("{kept:#}"), format!("{:#}", resolution()));
    fs::rename(moved.path().join(".git"), outer.0.join("moved-git-away")).unwrap();
    gone(&moved, resolution(), "its .git moved away");

    let half = repository("half");
    fs::remove_file(half.git_dir().join("HEAD")).unwrap();
    assert!(half.git_dir().join("config").is_file());
    let confined = half
        .git_directories()
        .expect_err("Git finds no repository there");
    assert!(
        gitturtle_core::finds_no_repository(&confined),
        "{confined:#}"
    );
    assert_eq!(
        half.verify_at_path().unwrap(),
        gitturtle_core::FoundAtPath::Same,
        "the filesystem check still holds"
    );
    gone(&half, confined, "its HEAD deleted");
}
