use super::*;
use futures::executor::block_on;
use gitturtle_core::ChangeStatus;
use std::sync::mpsc;

fn open_job(name: &str) -> Job {
    Job::Open {
        path: name.into(),
        scope: None,
        limit: 10,
        root: None,
        git_dir: None,
    }
}

fn job_name(job: Job) -> String {
    match job {
        Job::Open { path, .. } => path.to_string_lossy().into_owned(),
        _ => panic!("test executor expects an Open job"),
    }
}

fn notice(message: &str) -> Output {
    Output::Preview(Arc::new(Content::Notice(message.into())), Duration::ZERO)
}

fn message(output: Output) -> String {
    match output {
        Output::Preview(content, _) => match &*content {
            Content::Notice(message) => message.clone(),
            _ => panic!("expected notice"),
        },
        _ => panic!("expected preview output"),
    }
}

fn file() -> FileChange {
    FileChange {
        old_path: Some("before.txt".into()),
        new_path: Some("after.txt".into()),
        old_oid: Some("1".repeat(40)),
        new_oid: Some("2".repeat(40)),
        status: ChangeStatus::Renamed,
        old_mode: "100644".into(),
        new_mode: "100644".into(),
    }
}

fn key(name: &str) -> PreviewKey {
    let mut file = file();
    file.new_path = Some(name.into());
    PreviewKey::new(Path::new("/repository"), &file)
}

fn content(bytes: usize) -> Arc<Content> {
    Arc::new(Content::Notice("x".repeat(bytes)))
}

#[test]
fn queue_replaces_pending_and_cancels_active_at_checkpoint() {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = Worker::with_executor(move |job, _, cancellation| {
        let name = job_name(job);
        started_tx.send(name.clone())?;
        if name == "first" {
            release_rx.recv_timeout(Duration::from_secs(2))?;
        }
        cancellation.check()?;
        Ok(notice(&name))
    });
    let first = worker.submit(open_job("first"));
    assert_eq!(
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        "first"
    );
    let mut second = worker.submit(open_job("second"));
    let third = worker.submit(open_job("third"));
    // Replacement drops the pending reply synchronously and never executes it.
    assert!(second.try_recv().is_err());
    release_tx.send(()).unwrap();
    let first_error = block_on(first).unwrap().err().expect("superseded request");
    assert!(first_error.to_string().contains("superseded"));
    assert_eq!(message(block_on(third).unwrap().unwrap()), "third");
    assert_eq!(
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        "third"
    );
    assert!(started_rx.try_recv().is_err());
}

/// Waiting for the reader to go idle while a read never answers fails
/// rather than waiting for good.
#[test]
#[should_panic(expected = "the repository reader did not answer or skip its requests")]
fn waiting_for_a_read_that_never_answers_fails_rather_than_hangs() {
    let (started_tx, started_rx) = mpsc::channel();
    let (_release, held) = mpsc::channel::<()>();
    let worker = Worker::with_executor(move |_, _, _| {
        started_tx.send(())?;
        let _ = held.recv();
        Ok(notice("late"))
    });
    let _reply = worker.submit(open_job("held"));
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    worker.wait_idle_within(Duration::from_millis(50));
}

#[test]
fn explicit_cancel_invalidates_active_and_pending_without_replacement() {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = Worker::with_executor(move |_, _, cancellation| {
        started_tx.send(())?;
        release_rx.recv_timeout(Duration::from_secs(2))?;
        cancellation.check()?;
        Ok(notice("stale preview"))
    });
    let active = worker.submit(open_job("active"));
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let mut pending = worker.submit(open_job("pending"));
    worker.cancel();
    assert!(pending.try_recv().is_err());
    release_tx.send(()).unwrap();
    assert!(block_on(active).unwrap().is_err());
    assert!(started_rx.try_recv().is_err());
}

#[test]
fn dropping_worker_releases_pending_without_joining_active_work() {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = Worker::with_executor(move |_, _, cancellation| {
        started_tx.send(())?;
        release_rx.recv_timeout(Duration::from_secs(2))?;
        cancellation.check()?;
        Ok(notice("finished"))
    });
    let active = worker.submit(open_job("active"));
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let mut pending = worker.submit(open_job("pending"));
    // This must return while the active job is still waiting on release_rx.
    drop(worker);
    assert!(pending.try_recv().is_err());
    release_tx.send(()).unwrap();
    assert!(block_on(active).unwrap().is_err());
    assert!(started_rx.try_recv().is_err());
}

#[test]
fn decoder_panic_is_reported_and_worker_recovers_with_empty_cache() {
    let worker = Worker::with_executor(|job, cache, _| {
        let name = job_name(job);
        if name == "panic" {
            cache.insert(key("partial"), content(12));
            panic!("simulated decoder failure");
        }
        ensure!(cache.entries.is_empty(), "cache survives panic");
        ensure!(cache.bytes == 0, "cache accounting survives panic");
        Ok(notice("recovered"))
    });
    let error = block_on(worker.submit(open_job("panic")))
        .unwrap()
        .err()
        .expect("panic converted into error");
    assert!(error.to_string().contains("unexpectedly"));
    assert_eq!(
        message(block_on(worker.submit(open_job("next"))).unwrap().unwrap()),
        "recovered"
    );
}

#[test]
fn text_cache_budget_includes_prepared_patch_metadata() {
    let patch = "@@ -1 +1 @@\n-old\n+new\n".to_owned();
    let old = "old\n".to_owned();
    let new = "new\n".to_owned();
    let source_bytes = patch.capacity() + old.capacity() + new.capacity();
    let presentation = Arc::new(PatchPresentation::prepare(&patch));
    let split = Arc::new(crate::split_diff::SplitPresentation::prepare(
        &old,
        &new,
        &presentation,
    ));
    let metadata_bytes = presentation.retained_bytes() + split.retained_bytes();
    let content = Arc::new(Content::Text {
        diagrams: None,
        markdown: None,
        patch,
        old,
        new,
        presentation,
        split: Some(split),
        partial: None,
        partial_unavailable: None,
        lfs_missing: [false; 2],
        lfs_failed: [None; 2],
    });
    assert_eq!(content.bytes(), source_bytes + metadata_bytes);

    let mut cache = PreviewCache::with_limits(source_bytes + metadata_bytes - 1, 1);
    cache.insert(key("text"), Arc::clone(&content));
    assert!(cache.get(&key("text")).is_none());
    assert_eq!(cache.bytes, 0);

    let mut cache = PreviewCache::with_limits(source_bytes + metadata_bytes, 1);
    cache.insert(key("text"), content);
    assert!(cache.get(&key("text")).is_some());
    assert_eq!(cache.bytes, source_bytes + metadata_bytes);
}

#[test]
fn cache_eviction_respects_access_recency_and_content_bytes() {
    let value = content(8);
    let weight = value.bytes();
    let mut cache = PreviewCache::with_limits(weight * 2, 3);
    cache.insert(key("a"), Arc::clone(&value));
    cache.insert(key("b"), Arc::clone(&value));
    assert!(cache.get(&key("a")).is_some());
    cache.insert(key("c"), Arc::clone(&value));
    assert!(cache.get(&key("b")).is_none());
    assert!(cache.get(&key("a")).is_some());
    assert!(cache.get(&key("c")).is_some());
    assert_eq!(cache.bytes, weight * 2);
    assert_eq!(cache.lru.len(), 2);
}

#[test]
fn duplicate_cache_insert_replaces_weight_and_has_one_lru_entry() {
    let mut cache = PreviewCache::with_limits(100, 3);
    cache.insert(key("a"), content(8));
    let replacement = content(12);
    cache.insert(key("a"), Arc::clone(&replacement));
    assert_eq!(cache.bytes, replacement.bytes());
    assert_eq!(cache.entries.len(), 1);
    assert_eq!(cache.lru.len(), 1);
    assert!(Arc::ptr_eq(&cache.get(&key("a")).unwrap(), &replacement));
}

#[test]
fn cache_bounds_entries_and_rejects_oversized_replacements() {
    let mut cache = PreviewCache::with_limits(8, 2);
    cache.insert(key("a"), content(1));
    cache.insert(key("b"), content(1));
    cache.insert(key("c"), content(1));
    assert!(cache.get(&key("a")).is_none());
    assert_eq!(cache.entries.len(), 2);
    cache.insert(key("b"), content(9));
    assert!(cache.get(&key("b")).is_none());
    assert_eq!(cache.bytes, 1);
    let mut disabled = PreviewCache::with_limits(100, 0);
    disabled.insert(key("empty"), content(0));
    assert!(disabled.entries.is_empty());
}

#[test]
fn cache_keys_include_both_paths_modes_objects_repository_and_options() {
    let original = key("after.txt");
    let mut alternatives = Vec::new();
    let mut changed = original.clone();
    changed.old_path = Some("renamed-from.txt".into());
    alternatives.push(changed);
    let mut changed = original.clone();
    changed.new_path = Some("renamed-to.txt".into());
    alternatives.push(changed);
    let mut changed = original.clone();
    changed.old_oid = None;
    alternatives.push(changed);
    let mut changed = original.clone();
    changed.new_oid = None;
    alternatives.push(changed);
    let mut changed = original.clone();
    changed.old_mode = "120000".into();
    alternatives.push(changed);
    let mut changed = original.clone();
    changed.new_mode = "100755".into();
    alternatives.push(changed);
    let mut changed = original.clone();
    changed.repository = "/another-repository".into();
    alternatives.push(changed);
    let mut changed = original.clone();
    changed.preview_edge = 800;
    alternatives.push(changed);
    for alternative in alternatives {
        assert_ne!(original, alternative);
    }
}

#[cfg(unix)]
#[test]
fn cache_keys_preserve_non_utf8_path_bytes() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let mut first = file();
    first.old_path = Some(OsString::from_vec(b"before-\xff.txt".to_vec()).into());
    let mut second = first.clone();
    second.old_path = Some(OsString::from_vec(b"before-\xfe.txt".to_vec()).into());
    assert_eq!(
        first.old_path.as_ref().unwrap().to_string_lossy(),
        second.old_path.as_ref().unwrap().to_string_lossy()
    );
    assert_ne!(
        PreviewKey::new(Path::new("/repository"), &first),
        PreviewKey::new(Path::new("/repository"), &second)
    );
}

#[test]
fn image_detection_checks_both_names_and_excludes_gitlinks_and_symlinks() {
    let mut file = file();
    file.old_path = Some("before.PNG".into());
    file.new_path = Some("after.txt".into());
    assert!(image_change(&file));
    file.new_mode = "160000".into();
    assert!(!image_change(&file));
    file.new_mode = "120000".into();
    assert!(!image_change(&file));
    file.new_mode = "100644".into();
    file.old_mode = "120000".into();
    assert!(!image_change(&file));
    file.old_mode = "100644".into();
    file.old_path = None;
    assert!(!image_change(&file));
}

fn graph_commit(oid: String, parents: Vec<String>) -> Commit {
    Commit {
        oid,
        parents,
        author: String::new(),
        timestamp: 0,
        subject: String::new(),
        body: String::new(),
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

#[test]
fn replacement_and_explicit_cancel_signal_an_active_core_history_token() {
    for replace in [true, false] {
        let (started, start) = mpsc::channel();
        let worker = Worker::with_executor(move |job, _, cancellation| {
            if job_name(job) == "active" {
                started.send(()).unwrap();
                let deadline = Instant::now() + Duration::from_secs(2);
                while !cancellation.history.is_cancelled() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(1));
                }
                ensure!(
                    cancellation.history.is_cancelled(),
                    "Active core read was never cancelled"
                );
                Ok(notice("cancelled"))
            } else {
                Ok(notice("replacement"))
            }
        });
        let active = worker.submit(open_job("active"));
        start.recv_timeout(Duration::from_secs(1)).unwrap();
        let replacement = if replace {
            Some(worker.submit(open_job("replacement")))
        } else {
            worker.cancel();
            None
        };
        assert_eq!(message(block_on(active).unwrap().unwrap()), "cancelled");
        if let Some(replacement) = replacement {
            assert_eq!(
                message(block_on(replacement).unwrap().unwrap()),
                "replacement"
            );
        }
    }
}

#[test]
fn graph_budget_preserves_full_linear_history_and_rejects_excessive_width() {
    let linear: Vec<_> = (0..10_000)
        .map(|i| graph_commit(format!("commit-{i}"), vec![format!("commit-{}", i + 1)]))
        .collect();
    let (rows, notice) = layout_graph(&linear, &active()).unwrap();
    assert!(notice.is_none());
    assert_eq!(rows.len(), 10_000);
    assert!(
        rows.iter()
            .all(|row| row.width == 1 && row.edges.len() == 1)
    );

    let wide = [graph_commit(
        "merge".into(),
        (0..129).map(|i| format!("parent-{i}")).collect(),
    )];
    let (rows, notice) = layout_graph(&wide, &active()).unwrap();
    assert!(notice.is_some());
    assert_eq!(rows.len(), wide.len());
    assert!(
        rows.iter()
            .all(|row| row.edges.is_empty() && !row.incoming && row.width == 1)
    );
}

#[test]
fn graph_budget_bounds_continuation_edges_even_below_lane_limit() {
    let mut commits = vec![graph_commit(
        "merge".into(),
        (0..64).map(|i| format!("outside-page-{i}")).collect(),
    )];
    commits.extend(
        (0..4_000).map(|i| graph_commit(format!("commit-{i}"), vec![format!("commit-{}", i + 1)])),
    );
    let (rows, notice) = layout_graph(&commits, &active()).unwrap();
    assert!(
        notice.is_some(),
        "65 lanes across 4000 rows exceed the edge budget"
    );
    assert_eq!(rows.len(), commits.len());
    assert!(rows.iter().all(|row| row.edges.is_empty()));
}

#[test]
fn graph_preflight_stops_when_selection_is_superseded() {
    let cancellation = active();
    cancellation.latest.store(2, Ordering::Release);
    let commits = [graph_commit("head".into(), vec!["parent".into()])];
    assert!(layout_graph(&commits, &cancellation).is_err());
}

#[test]
fn search_history_over_128_matches_produces_node_only_rows_with_no_graph_notice() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();
    let run = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(path)
            .env("GIT_AUTHOR_NAME", "Tester")
            .env("GIT_AUTHOR_EMAIL", "tester@example.com")
            .env("GIT_COMMITTER_NAME", "Tester")
            .env("GIT_COMMITTER_EMAIL", "tester@example.com")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    };
    run(&["init", "-q", "-b", "main"]);
    for index in 0..135 {
        std::fs::write(path.join("log.txt"), format!("entry {index}\n")).unwrap();
        run(&["add", "log.txt"]);
        run(&["commit", "-q", "-m", &format!("needle commit {index}")]);
    }
    let repo = GitRepository::open(path).unwrap();
    let mut cache = super::PreviewCache::default();
    let mut session = super::RepositorySession::default();
    let cancel = super::Cancellation {
        generation: 1,
        latest: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1)),
        history: gitturtle_core::HistoryCancellation::default(),
        preparation: None,
    };
    let output = super::execute(
        super::Job::SearchHistory {
            repo,
            scope: None,
            pinned: None,
            query: "needle".into(),
            offset: 0,
            limit: 200,
            previous: Vec::new(),
            remaining_bytes: 2 * 1024 * 1024,
        },
        &mut cache,
        &cancel,
        &mut session,
    )
    .unwrap();
    let super::Output::SearchHistory(result) = output else {
        panic!("expected SearchHistory output");
    };
    assert_eq!(result.page.commits.len(), 135);
    assert_eq!(result.graph.len(), 135);
    assert!(
        result.graph_notice.is_none(),
        "search hits must not emit spurious Graph connections hidden banner: {:?}",
        result.graph_notice
    );
    assert!(
        result
            .graph
            .iter()
            .all(|row| row.width == 1 && row.edges.is_empty())
    );
}

#[test]
fn lazy_split_defers_allocation_until_requested_and_keeps_cache_bytes_exact() {
    use super::{Cancellation, PreviewCache, PreviewKey, RepositorySession, execute};
    use std::sync::atomic::AtomicU64;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();
    let run = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(path)
            .env("GIT_AUTHOR_NAME", "Tester")
            .env("GIT_AUTHOR_EMAIL", "tester@example.invalid")
            .env("GIT_COMMITTER_NAME", "Tester")
            .env("GIT_COMMITTER_EMAIL", "tester@example.invalid")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    };
    run(&["init", "-q"]);
    std::fs::write(path.join("notes.txt"), "one\ntwo\nthree\n").unwrap();
    run(&["add", "notes.txt"]);
    run(&["commit", "-q", "-m", "initial"]);
    std::fs::write(path.join("notes.txt"), "one\ntwo updated\nthree\n").unwrap();
    run(&["add", "notes.txt"]);
    run(&["commit", "-q", "-m", "update"]);

    let repo = GitRepository::open(path).unwrap();
    let head = repo.history(2).unwrap().into_iter().next().unwrap();
    let file = repo.changes_with_renames(&head.oid, 0).unwrap().remove(0);
    let key = PreviewKey::with_origins(repo.path(), &file, Default::default());
    let mut cache = PreviewCache::default();
    let mut session = RepositorySession::default();
    let cancel = Cancellation {
        generation: 1,
        latest: Arc::new(AtomicU64::new(1)),
        history: gitturtle_core::HistoryCancellation::default(),
        preparation: None,
    };

    // 1. Unified preview leaves split uncomputed and accounts exact bytes.
    let Output::Preview(unified, _) = execute(
        Job::Preview {
            repo: repo.clone(),
            file: file.clone(),
            origins: Default::default(),
            prepare_split: false,
        },
        &mut cache,
        &cancel,
        &mut session,
    )
    .unwrap() else {
        panic!("expected Preview");
    };
    assert!(matches!(
        unified.as_ref(),
        Content::Text { split: None, .. }
    ));
    assert_eq!(cache.bytes, unified.bytes());

    // 2. Job::PrepareSplit upgrades the cached entry and updates cache.bytes cleanly.
    let Output::SplitPrepared(upgraded) = execute(
        Job::PrepareSplit {
            content: Arc::clone(&unified),
            cache_key: Some(key.clone()),
        },
        &mut cache,
        &cancel,
        &mut session,
    )
    .unwrap() else {
        panic!("expected SplitPrepared");
    };
    assert!(matches!(
        upgraded.as_ref(),
        Content::Text { split: Some(_), .. }
    ));
    assert!(upgraded.bytes() > unified.bytes());
    assert_eq!(cache.bytes, upgraded.bytes());

    // 3. Eviction returns cache.bytes to 0 without underflow.
    cache.remove(&key);
    assert_eq!(cache.bytes, 0);

    // 4. PrepareSplit on an evicted/uncacheable key must not re-insert into cache.
    let Output::SplitPrepared(evicted_upgrade) = execute(
        Job::PrepareSplit {
            content: Arc::clone(&unified),
            cache_key: Some(key.clone()),
        },
        &mut cache,
        &cancel,
        &mut session,
    )
    .unwrap() else {
        panic!("expected SplitPrepared");
    };
    assert!(matches!(
        evicted_upgrade.as_ref(),
        Content::Text { split: Some(_), .. }
    ));
    assert!(cache.entries.is_empty());
    assert_eq!(cache.bytes, 0);

    // 5. Cache hit with prepare_split=true upgrades an un-upgraded entry in place.
    cache.insert(key.clone(), Arc::clone(&unified));
    assert_eq!(cache.bytes, unified.bytes());
    let Output::Preview(hit_upgraded, _) = execute(
        Job::Preview {
            repo,
            file,
            origins: Default::default(),
            prepare_split: true,
        },
        &mut cache,
        &cancel,
        &mut session,
    )
    .unwrap() else {
        panic!("expected Preview");
    };
    assert!(matches!(
        hit_upgraded.as_ref(),
        Content::Text { split: Some(_), .. }
    ));
    assert_eq!(cache.bytes, hit_upgraded.bytes());
    cache.remove(&key);
    assert_eq!(cache.bytes, 0);
}

fn preview_job(repo: GitRepository, file: FileChange) -> Job {
    Job::Preview {
        repo,
        file,
        origins: Default::default(),
        prepare_split: false,
    }
}

fn temp_repo_with_commits(count: usize) -> (tempfile::TempDir, GitRepository) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();
    let run = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(path)
            .env("GIT_AUTHOR_NAME", "Tester")
            .env("GIT_AUTHOR_EMAIL", "tester@example.invalid")
            .env("GIT_COMMITTER_NAME", "Tester")
            .env("GIT_COMMITTER_EMAIL", "tester@example.invalid")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    };
    run(&["init", "-q", "-b", "main"]);
    for i in 0..count {
        std::fs::write(path.join("notes.txt"), format!("entry {i}\n")).unwrap();
        run(&["add", "notes.txt"]);
        run(&["commit", "-q", "-m", &format!("commit {i}")]);
    }
    let repo = GitRepository::open(path).unwrap();
    (dir, repo)
}

#[test]
fn metadata_lane_runs_without_waiting_behind_blocked_preview_decode() {
    let (_dir, repo) = temp_repo_with_commits(1);
    let (preview_entered_tx, preview_entered_rx) = mpsc::channel();
    let (release_preview_tx, release_preview_rx) = mpsc::channel();
    let worker = Worker::with_lanes(
        move |job, _, _| Ok(notice(&format!("meta:{}", job_name(job)))),
        move |_, _, cancellation| {
            cancellation.unobserved(|| {
                let _ = preview_entered_tx.send(());
                let _ = release_preview_rx.recv_timeout(Duration::from_secs(3));
            })?;
            Ok(notice("preview"))
        },
    );
    let preview = worker.submit(preview_job(repo, file()));
    preview_entered_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();

    // While preview is blocked in `unobserved`, metadata completes immediately.
    let started = Instant::now();
    let meta = worker.submit(open_job("fast-commit-switch"));
    let meta_out = block_on(meta).unwrap().unwrap();
    assert_eq!(message(meta_out), "meta:fast-commit-switch");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "metadata waited behind blocked preview"
    );

    release_preview_tx.send(()).unwrap();
    let preview_err = block_on(preview)
        .unwrap()
        .err()
        .expect("superseded preview");
    assert!(preview_err.to_string().contains("superseded"));
    worker.wait_idle();
}

#[test]
fn preview_takeover_survives_intervening_metadata_job() {
    let (_dir, repo) = temp_repo_with_commits(1);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let decode_calls = Arc::new(AtomicU64::new(0));
    let decode_calls_worker = Arc::clone(&decode_calls);
    let worker = Worker::with_lanes(
        move |job, _, _| Ok(notice(&format!("meta:{}", job_name(job)))),
        move |_, _, cancellation| {
            decode_calls_worker.fetch_add(1, Ordering::AcqRel);
            cancellation.unobserved(|| {
                let _ = entered_tx.send(());
                let _ = release_rx.recv_timeout(Duration::from_secs(3));
            })?;
            Ok(notice("decoded-once"))
        },
    );
    let first_preview = worker.submit(preview_job(repo.clone(), file()));
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();

    // Intervening metadata job runs immediately while preview stays in unobserved decode.
    let meta = worker.submit(open_job("intervening"));
    assert_eq!(
        message(block_on(meta).unwrap().unwrap()),
        "meta:intervening"
    );

    // Returning to the same preview adopts the in-flight decode instead of starting a second.
    let adopted = worker.submit(preview_job(repo, file()));
    release_tx.send(()).unwrap();
    assert_eq!(message(block_on(adopted).unwrap().unwrap()), "decoded-once");
    assert!(block_on(first_preview).is_err());
    assert_eq!(decode_calls.load(Ordering::Acquire), 1);
    assert_eq!(worker.preparations(), 1);
    worker.wait_idle();
}

#[test]
fn lane_panics_are_isolated_between_metadata_and_preview() {
    let (_dir, repo) = temp_repo_with_commits(1);
    let worker = Worker::with_lanes(
        |job, _, _| {
            if job_name(job) == "boom" {
                panic!("simulated metadata panic");
            }
            Ok(notice("meta-ok"))
        },
        |job, cache, _| {
            if let Job::Preview { file, .. } = job
                && file.new_path.as_deref() == Some(Path::new("boom.png"))
            {
                cache.insert(key("partial"), content(16));
                panic!("simulated preview panic");
            }
            ensure!(cache.entries.is_empty(), "preview cache survived panic");
            ensure!(cache.bytes == 0, "preview cache bytes survived panic");
            Ok(notice("preview-ok"))
        },
    );

    let mut bad_file = file();
    bad_file.new_path = Some("boom.png".into());
    let prev_err = block_on(worker.submit(preview_job(repo.clone(), bad_file)))
        .unwrap()
        .err()
        .expect("preview panic converted to error");
    assert!(prev_err.to_string().contains("Preview processing failed"));

    // Metadata lane still works unaffected, and preview lane recovers with empty cache.
    assert_eq!(
        message(
            block_on(worker.submit(open_job("healthy")))
                .unwrap()
                .unwrap()
        ),
        "meta-ok"
    );
    assert_eq!(
        message(
            block_on(worker.submit(preview_job(repo.clone(), file())))
                .unwrap()
                .unwrap()
        ),
        "preview-ok"
    );

    // Metadata panic resets metadata session without harming preview lane.
    let meta_err = block_on(worker.submit(open_job("boom")))
        .unwrap()
        .err()
        .expect("metadata panic converted to error");
    assert!(meta_err.to_string().contains("metadata read failed"));
    assert_eq!(
        message(
            block_on(worker.submit(open_job("after-boom")))
                .unwrap()
                .unwrap()
        ),
        "meta-ok"
    );
    assert_eq!(
        message(
            block_on(worker.submit(preview_job(repo, file())))
                .unwrap()
                .unwrap()
        ),
        "preview-ok"
    );
}

#[test]
fn tracked_path_cache_bounds_entries_validates_fingerprint_and_evicts_repo() {
    let (_d1, repo1) = temp_repo_with_commits(1);
    let (_d2, repo2) = temp_repo_with_commits(1);
    let (_d3, repo3) = temp_repo_with_commits(1);
    let cancel = gitturtle_core::HistoryCancellation::default();
    let mut cache = TrackedPathCache::default();

    let fp1 = repo1.index_fingerprint().unwrap().unwrap();
    let idx1 = Arc::new(
        repo1
            .load_tracked_paths(&gitturtle_core::PathScope::Worktree, &cancel)
            .unwrap(),
    );
    cache.insert(repo1.path(), Some(fp1.clone()), Arc::clone(&idx1));
    assert!(cache.get_worktree(repo1.path(), &fp1).is_some());

    let fp2 = repo2.index_fingerprint().unwrap().unwrap();
    let idx2 = Arc::new(
        repo2
            .load_tracked_paths(&gitturtle_core::PathScope::Worktree, &cancel)
            .unwrap(),
    );
    cache.insert(repo2.path(), Some(fp2.clone()), idx2);
    assert_eq!(cache.len(), 2);

    // Inserting a 3rd repo evicts the least recently used (repo1).
    let fp3 = repo3.index_fingerprint().unwrap().unwrap();
    let idx3 = Arc::new(
        repo3
            .load_tracked_paths(&gitturtle_core::PathScope::Worktree, &cancel)
            .unwrap(),
    );
    cache.insert(repo3.path(), Some(fp3.clone()), idx3);
    assert_eq!(cache.len(), 2);
    assert!(cache.get_worktree(repo1.path(), &fp1).is_none());
    assert!(cache.get_worktree(repo2.path(), &fp2).is_some());

    // Changing `.git/index` invalidates repo2's fingerprint immediately.
    std::fs::write(repo2.path().join("extra.txt"), "new\n").unwrap();
    let status = std::process::Command::new("git")
        .args(["add", "extra.txt"])
        .current_dir(repo2.path())
        .status()
        .unwrap();
    assert!(status.success());
    let fp2_changed = repo2.index_fingerprint().unwrap().unwrap();
    assert_ne!(fp2, fp2_changed);
    assert!(cache.get_worktree(repo2.path(), &fp2_changed).is_none());

    cache.evict_repo(repo2.path());
    assert_eq!(cache.len(), 1);
    assert!(cache.get_worktree(repo3.path(), &fp3).is_some());
}

#[test]
fn chronological_history_page_emits_node_only_rows_and_clears_after_commit_graph() {
    let (_dir, repo) = temp_repo_with_commits(4);
    let mut session = RepositorySession {
        prefer_chronological: vec![repo.path().to_owned()],
        ..Default::default()
    };
    let cancel = active();
    let result = session
        .history_page(
            &repo,
            &gitturtle_core::HistoryScope::AllRefs,
            0,
            10,
            &cancel,
        )
        .unwrap();
    assert_eq!(
        result.page.order,
        gitturtle_core::HistoryOrder::Chronological
    );
    assert_eq!(result.page.commits.len(), 4);
    assert!(
        result
            .graph
            .iter()
            .all(|row| row.width == 1 && row.edges.is_empty())
    );
    assert!(
        result
            .graph_notice
            .as_deref()
            .is_some_and(|n| n.contains("commit-graph"))
    );

    // Writing a commit-graph lets the session automatically resume topological graphs.
    let status = std::process::Command::new("git")
        .args(["commit-graph", "write", "--reachable"])
        .current_dir(repo.path())
        .status()
        .unwrap();
    assert!(status.success());
    let topo = session
        .history_page(
            &repo,
            &gitturtle_core::HistoryScope::AllRefs,
            0,
            10,
            &cancel,
        )
        .unwrap();
    assert_eq!(topo.page.order, gitturtle_core::HistoryOrder::Topological);
    assert!(topo.graph_notice.is_none());
    assert!(session.prefer_chronological.is_empty());
}

#[test]
fn hold_lane_pauses_only_targeted_lane() {
    let (_dir, repo) = temp_repo_with_commits(1);
    let worker = Worker::with_lanes(
        |job, _, _| Ok(notice(&format!("meta:{}", job_name(job)))),
        |_, _, _| Ok(notice("preview-done")),
    );
    let hold_preview = worker.hold_lane(Lane::Preview);
    let meta = worker.submit(open_job("unblocked-meta"));
    assert_eq!(
        message(block_on(meta).unwrap().unwrap()),
        "meta:unblocked-meta"
    );
    let preview = worker.submit(preview_job(repo, file()));
    drop(hold_preview);
    assert_eq!(message(block_on(preview).unwrap().unwrap()), "preview-done");
    worker.release_search();
    worker.wait_idle();
}
