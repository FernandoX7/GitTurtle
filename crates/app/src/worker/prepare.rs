//! Worker request execution, snapshot assembly, diff/text preparation, and media decoding.
use super::cache::{PreviewCache, PreviewKey, TrackedPathCache};
#[cfg(test)]
use super::session::quiet_checkpoint;
use super::session::{
    GitDirectories, RepositorySession, confined_failure, ensure_found_at_path,
    no_repository_of_its_own, nothing_found_is_gone, worded_open_failure,
};
#[cfg(test)]
use super::types::Lane;
use super::types::{
    Content, HistoryReferences, ImageMetadata, ImageSide, Job, MissingScope, Output, QuietHistory,
    QuietPreview, QuietRefresh, Scope, SearchResult, Snapshot, VanishedScope, WorkingSelection,
    WorkingState,
};
use super::{Cancellation, PREVIEW_EDGE};
#[cfg(test)]
use super::{GRAPH_EDGE_LIMIT, GRAPH_LANE_LIMIT};
use crate::{graph, text::PatchPresentation};
use anyhow::{Result, anyhow, ensure};
use gitturtle_core::{
    BoundedBlob, Branch, Commit, FileChange, GitRepository, TextPreview, Worktree,
};
use gitturtle_preview::{
    ImagePreview, MAX_INPUT_BYTES, detect_lfs_pointer, is_image_path, metadata,
};
use gpui_kit::RenderImage;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};

#[cfg(test)]
pub(super) fn execute(
    job: Job,
    cache: &mut PreviewCache,
    cancellation: &Cancellation,
    session: &mut RepositorySession,
) -> Result<Output> {
    match job.lane() {
        Lane::Metadata => execute_metadata(job, cancellation, session),
        Lane::Preview => execute_preview(job, cache, cancellation),
    }
}

fn search_tracked_paths_cached(
    repo: &GitRepository,
    scope: &gitturtle_core::PathScope,
    query: &str,
    cache: Option<&Arc<Mutex<TrackedPathCache>>>,
    cancellation: &Cancellation,
) -> Result<gitturtle_core::TrackedPaths> {
    let Some(cache) = cache else {
        return repo.search_tracked_paths(scope, query, &cancellation.history);
    };
    match scope {
        gitturtle_core::PathScope::Worktree => {
            let fingerprint = repo.index_fingerprint()?;
            cancellation.check()?;
            if let Some(fp) = fingerprint.as_ref() {
                let hit = cache
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get_worktree(repo.path(), fp);
                if let Some(index) = hit {
                    return index.search(query, &cancellation.history);
                }
            }
            let index = Arc::new(repo.load_tracked_paths(scope, &cancellation.history)?);
            cancellation.check()?;
            if let Some(fp) = fingerprint {
                cache.lock().unwrap_or_else(|e| e.into_inner()).insert(
                    repo.path(),
                    Some(fp),
                    Arc::clone(&index),
                );
            }
            index.search(query, &cancellation.history)
        }
        gitturtle_core::PathScope::Revision(spec) => {
            let hit = cache
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_revision(repo.path(), spec);
            if let Some(index) = hit {
                return index.search(query, &cancellation.history);
            }
            let index = Arc::new(repo.load_tracked_paths(scope, &cancellation.history)?);
            cancellation.check()?;
            cache.lock().unwrap_or_else(|e| e.into_inner()).insert(
                repo.path(),
                None,
                Arc::clone(&index),
            );
            index.search(query, &cancellation.history)
        }
    }
}

pub(super) fn execute_metadata(
    job: Job,
    cancellation: &Cancellation,
    session: &mut RepositorySession,
) -> Result<Output> {
    let start = Instant::now();
    cancellation.check()?;
    match job {
        Job::ReleaseHistory => {
            session.history = None;
            session.search = None;
            Ok(Output::HistoryReleased)
        }
        Job::HistoryPage {
            repo,
            scope,
            offset,
            limit,
        } => session
            .history_page(&repo, &scope, offset, limit, cancellation)
            .map(Output::HistoryPage),
        Job::CompareRevisions {
            repo,
            before,
            after,
            mode,
        } => {
            let result = repo.compare_revisions(&before, &after, mode, &cancellation.history)?;
            cancellation.check()?;
            Ok(Output::RevisionComparison(result))
        }
        Job::RevisionTargets { repo } => {
            gitturtle_core::run_cancellable_inspection(cancellation.history.clone(), || {
                let mut targets = vec!["HEAD".into()];
                targets.extend(repo.branches()?.into_iter().take(10_000).map(|b| {
                    format!(
                        "refs/{}/{}",
                        if b.remote { "remotes" } else { "heads" },
                        b.name
                    )
                }));
                cancellation.check()?;
                targets.extend(
                    repo.tags()?
                        .tags
                        .into_iter()
                        .map(|tag| format!("refs/tags/{}", tag.name)),
                );
                cancellation.check()?;
                Ok(Output::RevisionTargets(targets))
            })
        }
        Job::TrackedPaths {
            repo,
            scope,
            query,
            cache,
        } => Ok(Output::TrackedPaths(search_tracked_paths_cached(
            &repo,
            &scope,
            &query,
            cache.as_ref(),
            cancellation,
        )?)),
        Job::SearchHistory {
            repo,
            scope,
            pinned,
            query,
            filter,
            offset,
            limit,
            previous,
            remaining_bytes,
        } => {
            let pinned = if let Some(pinned) = pinned {
                pinned
            } else {
                let branches = repo.branches()?;
                cancellation.check()?;
                let worktrees = repo.worktrees()?;
                cancellation.check()?;
                match scope_oid(scope.as_ref(), &branches, &worktrees)? {
                    Some(oid) if oid.bytes().all(|byte| byte == b'0') => {
                        gitturtle_core::HistoryScope::PinnedRefs(Vec::new())
                    }
                    Some(oid) => gitturtle_core::HistoryScope::FromCommit(oid.to_owned()),
                    None => gitturtle_core::HistoryScope::AllRefs,
                }
            };
            let page = session.search_history_page(
                &repo,
                &pinned,
                (&query, &filter),
                (offset, limit),
                cancellation,
            )?;
            cancellation.check()?;
            let retained_bytes = page
                .commits
                .iter()
                .map(commit_metadata_bytes)
                .sum::<usize>();
            ensure!(
                retained_bytes <= remaining_bytes,
                "Search results exceed the 64 MiB display budget. Narrow the query to inspect these matches."
            );
            // Filtered search hits are discontinuous and render without graph edges
            // (`filtered = true`), so never run the full lane-frontier layout or emit
            // a spurious "Graph connections hidden" banner when >128 hits accumulate.
            let total_rows = previous.len().saturating_add(page.commits.len());
            let node = graph::GraphRow {
                width: 1,
                ..Default::default()
            };
            let mut graph = Vec::with_capacity(total_rows);
            for _ in 0..total_rows {
                cancellation.check()?;
                graph.push(node.clone());
            }
            Ok(Output::SearchHistory(SearchResult {
                page,
                graph,
                graph_notice: None,
                retained_bytes,
            }))
        }
        Job::Blame { repo, target } => {
            let blame = repo.blame(&target, &cancellation.history)?;
            cancellation.check()?;
            Ok(Output::Blame(blame))
        }
        Job::LineHistory {
            repo,
            oid,
            path,
            line,
        } => {
            let history = repo.line_history(&oid, &path, line, &cancellation.history)?;
            cancellation.check()?;
            Ok(Output::LineHistory(history))
        }
        Job::FileHistory {
            repo,
            anchor,
            path,
            offset,
            limit,
        } => {
            let page = repo.file_history(&anchor, &path, offset, limit, &cancellation.history)?;
            cancellation.check()?;
            Ok(Output::FileHistory(page))
        }
        Job::QuietRefresh {
            repo,
            scope,
            limit,
            history,
            selected,
        } => {
            // A quiet read reopens the tab's repository passively: where Git
            // no longer finds it at its path, every read there would read the
            // work tree enclosing it, so it reads nothing ([`RepositoryGone`]).
            // The retained session answers from its filesystem identities
            // without starting Git while the repository is unchanged. Where
            // Git finds no repository there at all, the read fails as gone
            // with nothing found, so the tab is read as Refresh reads it and
            // its Open gives the reasons any folder that is not a repository
            // gets; it once said Git's own "not a git repository" as a local
            // refresh error and left the tab unmarked.
            let root = Some(repo.path());
            let captured = GitDirectories::of(&repo);
            // The handle's own check, before the session's reopen: the
            // session answers from its directories' and links' identities,
            // which a `core.worktree` re-pointed in the repository's
            // configuration leaves as they were while it moves the work
            // tree; the handle's check compares those configuration files
            // and asks Git once one changed (`GitRepository::verify_at_path`).
            ensure_found_at_path(&repo)?;
            session
                .open(repo.path(), root, Some(&captured))
                .map_err(|error| nothing_found_is_gone(&repo, error))?;
            let mut working = WorkingState::read_with_checkpoint(&repo, || {
                #[cfg(test)]
                quiet_checkpoint();
                cancellation.check()
            })
            .map_err(|error| confined_failure(&repo, error))?;
            cancellation.check()?;
            let snapshot = history.then(|| {
                match read_snapshot(
                    repo.path().to_owned(),
                    (root, Some(&captured)),
                    scope.as_ref(),
                    MissingScope::Fail,
                    limit,
                    cancellation,
                    session,
                ) {
                    Ok(snapshot) => Ok(QuietHistory::Refreshed(worker_shallow(snapshot, &repo))),
                    Err(error) => {
                        cancellation.check()?;
                        // A zero-row unscoped snapshot reads current navigation
                        // metadata without loading a replacement history page.
                        let metadata = read_snapshot(
                            repo.path().to_owned(),
                            (root, Some(&captured)),
                            None,
                            MissingScope::Fail,
                            0,
                            cancellation,
                            session,
                        )?;
                        Ok(QuietHistory::Retained {
                            metadata,
                            vanished: error.downcast_ref::<VanishedScope>().cloned(),
                            summary: gitturtle_core::failure_summary(&error),
                            error: format!("{error:#}"),
                        })
                    }
                }
            });
            cancellation.check()?;
            let preview = selected.map(
                |WorkingSelection {
                     path,
                     area: previous_area,
                     file: previous_file,
                     content: previous_content,
                     prepare_split,
                 }| {
                    let Some(entry) = working
                        .status
                        .entries
                        .iter()
                        .find(|entry| entry.path == path)
                        .cloned()
                    else {
                        return Ok(QuietPreview::Absent);
                    };
                    let area = crate::workspace::retained_area(
                        previous_area,
                        entry.staged.is_some(),
                        entry.unstaged.is_some() || entry.untracked || entry.conflicted,
                    );
                    let Some(mut area) = area else {
                        return Ok(QuietPreview::Absent);
                    };
                    let (mut file, mut content) = prepare_working_preview(
                        &repo,
                        &entry,
                        area,
                        working.status.head.as_deref(),
                        prepare_split,
                        cancellation,
                    )?;
                    // If an external `git add` raced between WorkingState::read
                    // and worktree_preview for an Unstaged view, the index OID
                    // changed underneath us and old/new would mix pre-add index
                    // with post-edit worktree. Re-read status and preview once
                    // so the displayed diff stays self-consistent.
                    if area == gitturtle_core::ChangeArea::Unstaged
                        && let Ok(current_index_oid) = repo.index_blob_oid(&entry.path)
                        && current_index_oid.as_deref() != entry.index_oid()
                    {
                        let fresh =
                            WorkingState::read_with_checkpoint(&repo, || cancellation.check())
                                .map_err(|error| confined_failure(&repo, error))?;
                        working = fresh;
                        let Some(fresh_entry) = working
                            .status
                            .entries
                            .iter()
                            .find(|item| item.path == path)
                            .cloned()
                        else {
                            return Ok(QuietPreview::Absent);
                        };
                        let Some(fresh_area) = crate::workspace::retained_area(
                            previous_area,
                            fresh_entry.staged.is_some(),
                            fresh_entry.unstaged.is_some()
                                || fresh_entry.untracked
                                || fresh_entry.conflicted,
                        ) else {
                            return Ok(QuietPreview::Absent);
                        };
                        let (re_file, re_content) = prepare_working_preview(
                            &repo,
                            &fresh_entry,
                            fresh_area,
                            working.status.head.as_deref(),
                            prepare_split,
                            cancellation,
                        )?;
                        area = fresh_area;
                        file = re_file;
                        content = re_content;
                    }
                    if area == previous_area
                        && previous_file.as_ref() == Some(&file)
                        && previous_content
                            .as_ref()
                            .is_some_and(|previous| content_unchanged(previous, &content))
                    {
                        Ok(QuietPreview::Unchanged)
                    } else {
                        Ok(QuietPreview::Changed { file, content })
                    }
                },
            );
            cancellation.check()?;
            // A read already running when the repository's `.git` went away
            // fails part way, its commands confined to it: its operation
            // state, its History or its comparison fails as "not a git
            // repository", which once showed as an "Operation state: …" or
            // "History scope changed: …" banner beside the tab's mark. Any
            // part failed, the repository is checked again (from the
            // filesystem while it holds), and a repository gone is the whole
            // read's answer, so the tab is marked and says only that.
            let working = if matches!(snapshot, Some(Err(_) | Ok(QuietHistory::Retained { .. })))
                || matches!(preview, Some(Err(_)))
            {
                ensure_found_at_path(&repo)?;
                working
            } else {
                working.confirmed(&repo)?
            };
            Ok(Output::QuietRefresh(Box::new(QuietRefresh {
                working,
                snapshot,
                preview,
            })))
        }
        Job::Open {
            path,
            scope,
            limit,
            root,
            git_dir,
        } => {
            // An explicit Open (Refresh, Show latest, a restored tab) resolves
            // the scope again; a vanished one opens All history, never an
            // unavailable repository. A tab's repository gone from its root
            // says so in its own words.
            let mut snapshot = read_snapshot(
                path.clone(),
                (root.as_deref(), git_dir.as_ref()),
                scope.as_ref(),
                MissingScope::AllHistory,
                limit,
                cancellation,
                session,
            )
            .map_err(|error| {
                // A tab's own saved folder, not a worktree row's reopen of
                // the worktree it names (`State::reopen_root`): that row is
                // no tab's folder, and captures no directories either.
                let uncaptured = root.is_some()
                    && git_dir.is_none()
                    && !matches!(&scope, Some(Scope::Worktree { path: tree }) if *tree == path);
                worded_open_failure(&path, no_repository_of_its_own(uncaptured, error))
            })?;
            cancellation.check()?;
            snapshot.elapsed = start.elapsed();
            Ok(Output::Snapshot(Box::new(snapshot)))
        }
        Job::Changes { repo, oid, parent } => {
            let changes = repo.changes_with_renames(&oid, parent)?;
            cancellation.check()?;
            Ok(Output::Changes(changes, start.elapsed()))
        }
        Job::Preview { .. }
        | Job::WorkingPreview { .. }
        | Job::TrackedPreview { .. }
        | Job::ReviewText { .. }
        | Job::PrepareSplit { .. } => {
            anyhow::bail!("Preview job routed to metadata lane")
        }
    }
}

pub(super) fn execute_preview(
    job: Job,
    cache: &mut PreviewCache,
    cancellation: &Cancellation,
) -> Result<Output> {
    let start = Instant::now();
    cancellation.check()?;
    match job {
        Job::ReviewText {
            content,
            options,
            prepare_split,
        } => {
            let mut content =
                crate::text_review::prepare(&content, options, || cancellation.check())?;
            if prepare_split
                && options == crate::text_review::Options::default()
                && let Some(upgraded) = ensure_split_in_content(&content)
            {
                cancellation.check()?;
                content = upgraded;
            }
            Ok(Output::ReviewText(content, start.elapsed()))
        }
        Job::PrepareSplit { content, cache_key } => {
            let upgraded = if let Some(upgraded) = ensure_split_in_content(&content) {
                cancellation.check()?;
                if let Some(key) = cache_key
                    && cache.entries.contains_key(&key)
                {
                    cache.insert(key, Arc::clone(&upgraded));
                }
                upgraded
            } else {
                content
            };
            Ok(Output::SplitPrepared(upgraded))
        }
        Job::TrackedPreview { repo, entry, scope } => {
            let mut file = repo.tracked_path_change(&entry);
            if matches!(scope, gitturtle_core::PathScope::Revision(_)) {
                let mode = file.new_mode.clone();
                let origins = crate::markdown_view::Origins::revisions(
                    None,
                    match scope {
                        gitturtle_core::PathScope::Revision(oid) => Some(oid),
                        _ => None,
                    },
                );
                let content = prepare_preview(&repo, &file, &origins, false, cache, cancellation)?;
                return Ok(Output::TrackedPreview {
                    mode,
                    content,
                    elapsed: start.elapsed(),
                });
            }
            let (mode, bytes) = repo.read_tracked_working_file(&entry)?;
            cancellation.check()?;
            file.new_mode = mode;
            let mut lfs = LfsSides::default();
            let mut content = if let Some(content) =
                supplied_nontext_content(&repo, &file, &[], &bytes, cancellation)?
            {
                content
            } else if let Some(content) =
                resolved_lfs_text_content(&repo, &file, &[], &bytes, &mut lfs, false, cancellation)?
            {
                content
            } else if bytes.len() > gitturtle_core::MAX_DIFF_BYTES
                || bytes.iter().filter(|b| **b == b'\n').count() > gitturtle_core::MAX_DIFF_LINES
            {
                Content::Notice("File exceeds the 2 MiB / 100,000-line text preview limit. File history remains available.".into())
            } else if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
                Content::Notice(
                    "Binary or non-UTF-8 working file. File history remains available.".into(),
                )
            } else {
                let source = String::from_utf8(bytes)?;
                // Quick Open is a source inspection, so no synthetic staged
                // patch or selected-edit actions are manufactured.
                prepared_text(String::new(), String::new(), source, false)
            };
            lfs.mark(&mut content);
            attach_mermaid(&mut content, &file, lfs.pointers, cancellation)?;
            attach_markdown_assets(
                &mut content,
                &repo,
                &file,
                &crate::markdown_view::Origins {
                    old: None,
                    new: Some(gitturtle_core::PreviewAssetScope::Worktree),
                },
                cancellation,
            )?;
            cancellation.check()?;
            Ok(Output::TrackedPreview {
                mode: file.new_mode,
                content: Arc::new(content),
                elapsed: start.elapsed(),
            })
        }
        Job::WorkingPreview {
            repo,
            entry,
            area,
            head,
            prepare_split,
        } => {
            let (file, content) = prepare_working_preview(
                &repo,
                &entry,
                area,
                head.as_deref(),
                prepare_split,
                cancellation,
            )?;
            Ok(Output::WorkingPreview(file, content, start.elapsed()))
        }
        Job::Preview {
            repo,
            file,
            origins,
            prepare_split,
        } => {
            let content =
                prepare_preview(&repo, &file, &origins, prepare_split, cache, cancellation)?;
            Ok(Output::Preview(content, start.elapsed()))
        }
        _ => anyhow::bail!("Metadata job routed to preview lane"),
    }
}

fn prepare_working_preview(
    repo: &GitRepository,
    entry: &gitturtle_core::StatusEntry,
    area: gitturtle_core::ChangeArea,
    head: Option<&str>,
    prepare_split: bool,
    cancellation: &Cancellation,
) -> Result<(FileChange, Arc<Content>)> {
    if entry.conflicted {
        let presentation =
            crate::conflicts::Presentation::prepare(repo.conflict_preview(&entry.path)?);
        cancellation.check()?;
        return Ok((
            presentation.file(),
            Arc::new(Content::Conflict(Arc::new(presentation))),
        ));
    }
    if entry.is_untracked_directory() {
        let file = FileChange {
            old_path: None,
            new_path: Some(entry.path.clone()),
            old_oid: None,
            new_oid: None,
            status: gitturtle_core::ChangeStatus::Added,
            old_mode: "000000".into(),
            new_mode: "040000".into(),
        };
        return Ok((
            file,
            Arc::new(Content::Notice(
                "Untracked folder or nested repository. Add an ignore rule from its menu or inspect it outside GitTurtle.".into(),
            )),
        ));
    }
    let preview = repo.worktree_preview(entry, area)?;
    cancellation.check()?;
    let file = preview.file;
    let mut partial_diff = preview.partial;
    let mut unavailable = preview.partial_unavailable;
    let mut lfs = LfsSides::default();
    let mut content = if let Some(content) =
        supplied_nontext_content(repo, &file, &preview.old, &preview.new, cancellation)?
    {
        content
    } else if let Some(content) = resolved_lfs_text_content(
        repo,
        &file,
        &preview.old,
        &preview.new,
        &mut lfs,
        prepare_split,
        cancellation,
    )? {
        partial_diff = None;
        unavailable = Some("This view shows verified LFS object content. Partial staging is unavailable because Git stages the underlying pointer; use whole-file staging.".into());
        content
    } else {
        match preview.preview {
            TextPreview::Patch(patch) => prepared_text(
                patch,
                String::from_utf8(preview.old)?,
                String::from_utf8(preview.new)?,
                prepare_split,
            ),
            TextPreview::Binary => {
                Content::Notice("Binary file · no text comparison is available.".into())
            }
            TextPreview::TooLarge {
                old_bytes,
                new_bytes,
            } => Content::Notice(format!(
                "Text preview exceeds the display budget · before {} · after {}.",
                crate::counted(old_bytes, "byte", "bytes"),
                crate::counted(new_bytes, "byte", "bytes")
            )),
            TextPreview::Submodule { old_oid, new_oid } => Content::Notice(format!(
                "Submodule · {} → {}",
                old_oid.as_deref().unwrap_or("absent"),
                new_oid.as_deref().unwrap_or("absent")
            )),
        }
    };
    lfs.mark(&mut content);
    cancellation.check()?;
    attach_mermaid(&mut content, &file, lfs.pointers, cancellation)?;
    let origins = match area {
        gitturtle_core::ChangeArea::Staged => crate::markdown_view::Origins {
            old: head.map(|oid| gitturtle_core::PreviewAssetScope::Revision(oid.to_owned())),
            new: Some(gitturtle_core::PreviewAssetScope::Index),
        },
        gitturtle_core::ChangeArea::Unstaged => crate::markdown_view::Origins {
            old: Some(gitturtle_core::PreviewAssetScope::Index),
            new: Some(gitturtle_core::PreviewAssetScope::Worktree),
        },
    };
    attach_markdown_assets(&mut content, repo, &file, &origins, cancellation)?;
    if let Content::Text {
        patch,
        presentation,
        split,
        split_partial,
        partial,
        partial_unavailable,
        ..
    } = &mut content
    {
        if let Some(diff) = partial_diff {
            let diff = Arc::new(diff);
            match crate::partial_view::prepare(patch, presentation, Arc::clone(&diff)) {
                Ok(actions) => {
                    *partial = Some(Arc::new(actions));
                    if let Some(split_pres) = split.as_deref()
                        && let Ok(split_actions) =
                            gitturtle_diff_model::split_diff::prepare_split_partial(
                                split_pres, diff,
                            )
                    {
                        *split_partial = Some(Arc::new(split_actions));
                    }
                }
                Err(error) => unavailable = Some(error.to_string()),
            }
        }
        *partial_unavailable = unavailable;
    }
    cancellation.check()?;
    // Mutable worktree/index comparisons are deliberately not cached.
    Ok((file, Arc::new(content)))
}

fn prepare_preview(
    repo: &GitRepository,
    file: &FileChange,
    origins: &crate::markdown_view::Origins,
    prepare_split: bool,
    cache: &mut PreviewCache,
    cancellation: &Cancellation,
) -> Result<Arc<Content>> {
    if origins.new == Some(gitturtle_core::PreviewAssetScope::Worktree) {
        return prepare_revision_vs_working_preview(
            repo,
            file,
            origins,
            prepare_split,
            cancellation,
        );
    }
    let key = PreviewKey::with_origins(repo.path(), file, origins.clone());
    if let Some(content) = cache.get(&key) {
        if prepare_split
            && matches!(content.as_ref(), Content::Text { split: None, .. })
            && let Some(upgraded) = ensure_split_in_content(&content)
        {
            cancellation.check()?;
            cache.insert(key, Arc::clone(&upgraded));
            return Ok(upgraded);
        }
        return Ok(content);
    }
    let mut lfs = LfsSides::default();
    let mut content = text_content(repo, file, &mut lfs, prepare_split, cancellation)?;
    attach_markdown_assets(&mut content, repo, file, origins, cancellation)?;
    let cacheable = !lfs.store_can_change()
        && match &content {
            Content::Images { old, new } => old.message.is_none() && new.message.is_none(),
            Content::Rich(preview) => preview.old.error.is_none() && preview.new.error.is_none(),
            Content::Text {
                old, new, markdown, ..
            } => {
                markdown
                    .as_ref()
                    .is_none_or(|markdown| markdown.cacheable())
                    && ![old, new]
                        .iter()
                        .any(|source| detect_lfs_pointer(source.as_bytes()).is_some())
            }
            _ => true,
        };
    cancellation.check()?;
    let content = Arc::new(content);
    if cacheable {
        cache.insert(key, Arc::clone(&content));
    }
    Ok(content)
}

fn prepare_revision_vs_working_preview(
    repo: &GitRepository,
    file: &FileChange,
    origins: &crate::markdown_view::Origins,
    prepare_split: bool,
    cancellation: &Cancellation,
) -> Result<Arc<Content>> {
    cancellation.check()?;
    let preview = repo.preview_revision_vs_working(file)?;
    cancellation.check()?;
    let file = preview.file;
    let mut lfs = LfsSides::default();
    let mut content = if let Some(content) =
        supplied_nontext_content(repo, &file, &preview.old, &preview.new, cancellation)?
    {
        content
    } else if let Some(content) = resolved_lfs_text_content(
        repo,
        &file,
        &preview.old,
        &preview.new,
        &mut lfs,
        prepare_split,
        cancellation,
    )? {
        content
    } else {
        match preview.preview {
            TextPreview::Patch(patch) => prepared_text(
                patch,
                String::from_utf8(preview.old)?,
                String::from_utf8(preview.new)?,
                prepare_split,
            ),
            TextPreview::Binary => {
                Content::Notice("Binary file · no text comparison is available.".into())
            }
            TextPreview::TooLarge {
                old_bytes,
                new_bytes,
            } => Content::Notice(format!(
                "Text preview exceeds the display budget · before {} · after {}.",
                crate::counted(old_bytes, "byte", "bytes"),
                crate::counted(new_bytes, "byte", "bytes")
            )),
            TextPreview::Submodule { old_oid, new_oid } => Content::Notice(format!(
                "Submodule · {} → {}",
                old_oid.as_deref().unwrap_or("absent"),
                new_oid.as_deref().unwrap_or("absent")
            )),
        }
    };
    lfs.mark(&mut content);
    cancellation.check()?;
    attach_mermaid(&mut content, &file, lfs.pointers, cancellation)?;
    attach_markdown_assets(&mut content, repo, &file, origins, cancellation)?;
    Ok(Arc::new(content))
}

fn commit_metadata_bytes(commit: &Commit) -> usize {
    commit.history_bytes()
}

/// The snapshot of the repository at `path`: the one Git discovery finds,
/// or, for a tab's passive reopen, the one at `root` or none
/// ([`RepositorySession::open`]).
pub(super) fn read_snapshot(
    path: PathBuf,
    (root, git_dir): (Option<&Path>, Option<&GitDirectories>),
    scope: Option<&Scope>,
    missing: MissingScope,
    limit: usize,
    cancellation: &Cancellation,
    session: &mut RepositorySession,
) -> Result<Snapshot> {
    let start = Instant::now();
    cancellation.check()?;
    let repository = session.open(&path, root, git_dir)?;
    cancellation.check()?;
    // Overlap passive `worktrees()` with `branches()` on one bounded scoped
    // thread (same fallback-to-sequential discipline as `tags()` below).
    let (branches, worktrees) = std::thread::scope(|threads| -> Result<_> {
        let beside = std::thread::Builder::new()
            .name("gitturtle-worktrees".into())
            .spawn_scoped(threads, || repository.worktrees())
            .ok();
        let branches = repository.branches()?;
        cancellation.check()?;
        let worktrees = match beside {
            Some(read) => read
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))?,
            None => repository.worktrees()?,
        };
        Ok((branches, worktrees))
    })?;
    cancellation.check()?;
    let (oid, vanished_scope) = match scope_oid(scope, &branches, &worktrees) {
        Ok(oid) => (oid, None),
        Err(vanished) if missing == MissingScope::AllHistory => (None, Some(vanished)),
        Err(vanished) => return Err(vanished.into()),
    };
    let scope = match oid {
        Some(oid) if oid.bytes().all(|byte| byte == b'0') => {
            gitturtle_core::HistoryScope::PinnedRefs(Vec::new())
        }
        Some(oid) => gitturtle_core::HistoryScope::FromCommit(oid.to_owned()),
        None => gitturtle_core::HistoryScope::AllRefs,
    };
    // The tag and stash reads need nothing from the history page, so one scoped
    // thread reads tags, bounded stashes, and commit references while this one
    // reads the page: a snapshot runs at most one auxiliary thread beside its
    // page, and the scope joins it, whatever the page's outcome, before returning.
    let (
        (commits, graph, graph_notice, history_scope, history_next_offset),
        (refs, tags, tags_truncated, stashes),
    ) = std::thread::scope(|threads| -> Result<_> {
        let references = || snapshot_sidebar_refs(&repository, &branches, cancellation);
        let beside = std::thread::Builder::new()
            .name("gitturtle-tags".into())
            .spawn_scoped(threads, references)
            .ok();
        let page = if limit == 0 {
            (Vec::new(), Vec::new(), None, scope, None)
        } else {
            // An explicit or quiet refresh captures new tips and starts a
            // new traversal. Ordinary subsequent pages use the immutable
            // returned scope.
            session.history = None;
            let result = session.history_page(&repository, &scope, 0, limit, cancellation)?;
            let pinned = session
                .history
                .as_ref()
                .expect("Live history traversal")
                .traversal
                .scope()
                .clone();
            (
                result.page.commits,
                result.graph,
                result.graph_notice,
                pinned,
                result.page.next_offset,
            )
        };
        cancellation.check()?;
        let refs = match beside {
            Some(read) => read
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
            // A thread the system cannot start reads them after the page.
            None => references(),
        };
        Ok((page, refs?))
    })?;
    cancellation.check()?;
    Ok(Snapshot {
        repository,
        branches,
        worktrees,
        tags,
        tags_truncated,
        stashes,
        commits,
        history_scope,
        history_offset: 0,
        history_next_offset,
        graph,
        graph_notice,
        refs,
        elapsed: start.elapsed(),
        vanished_scope,
        shallow: false,
    })
}

/// `snapshot` with whether `repo` is shallow, `true` where that cannot be
/// read, as the landing then draws whatever it installs.
pub(super) fn worker_shallow(mut snapshot: Snapshot, repo: &GitRepository) -> Snapshot {
    snapshot.shallow = repo.is_shallow().unwrap_or(true);
    snapshot
}

const SIDEBAR_STASH_LIMIT: usize = 50;

type SidebarRefs = (
    HistoryReferences,
    Vec<gitturtle_core::Tag>,
    bool,
    Vec<gitturtle_core::StashEntry>,
);

fn snapshot_sidebar_refs(
    repository: &GitRepository,
    branches: &[Branch],
    cancellation: &Cancellation,
) -> Result<SidebarRefs> {
    let (tags, tags_truncated) =
        gitturtle_core::run_cancellable_inspection(cancellation.history.clone(), || {
            repository.tags()
        })
        .map(|list| (list.tags, list.truncated))
        .unwrap_or_default();
    cancellation.check()?;
    let stashes = if repository.is_bare() {
        Vec::new()
    } else {
        gitturtle_core::run_cancellable_inspection(cancellation.history.clone(), || {
            repository.stash_list(0, SIDEBAR_STASH_LIMIT)
        })
        .map(|page| page.entries)
        .unwrap_or_default()
    };
    cancellation.check()?;
    let refs = HistoryReferences::new(branches, &tags);
    Ok((refs, tags, tags_truncated, stashes))
}

#[cfg(test)]
pub(super) fn layout_graph(
    commits: &[Commit],
    cancellation: &Cancellation,
) -> Result<(Vec<graph::GraphRow>, Option<String>)> {
    if graph_within_budget(commits, cancellation)? {
        return Ok((graph::layout(commits, || cancellation.check())?, None));
    }
    // Never truncate individual edges: that would imply incorrect ancestry.
    // A node-only fallback keeps the full history list and inspectors usable.
    let node = graph::GraphRow {
        width: 1,
        ..Default::default()
    };
    let rows = commits
        .iter()
        .map(|_| {
            cancellation.check()?;
            Ok(node.clone())
        })
        .collect::<Result<_>>()?;
    Ok((rows, Some(graph_notice())))
}

pub(super) fn graph_notice() -> String {
    "Graph connections hidden for this history. Select a branch to view a smaller graph.".into()
}

/// Preflight the graph's frontier and exact edge count without building lane
/// geometry. This bounds both the quadratic wide-history case and unusual
/// commits containing enormous parent lists before calling the layout engine.
#[cfg(test)]
fn graph_within_budget(commits: &[Commit], cancellation: &Cancellation) -> Result<bool> {
    use std::collections::HashSet;
    let mut frontier = HashSet::<&str>::new();
    let mut parents = HashSet::new();
    let mut edges = 0usize;
    let mut parent_entries = 0usize;
    for commit in commits {
        cancellation.check()?;
        let before_width = frontier.len() + usize::from(!frontier.contains(commit.oid.as_str()));
        if before_width > GRAPH_LANE_LIMIT {
            return Ok(false);
        }
        frontier.remove(commit.oid.as_str());
        parents.clear();
        for parent in &commit.parents {
            parent_entries += 1;
            if parent_entries > GRAPH_EDGE_LIMIT {
                return Ok(false);
            }
            parents.insert(parent.as_str());
            frontier.insert(parent.as_str());
            if frontier.len() > GRAPH_LANE_LIMIT {
                return Ok(false);
            }
        }
        edges += before_width - 1 + parents.len();
        if edges > GRAPH_EDGE_LIMIT {
            return Ok(false);
        }
    }
    Ok(true)
}

fn scope_oid<'a>(
    scope: Option<&Scope>,
    branches: &'a [Branch],
    worktrees: &'a [Worktree],
) -> std::result::Result<Option<&'a str>, VanishedScope> {
    let Some(scope) = scope else {
        return Ok(None);
    };
    let resolved = match scope {
        Scope::Branch { name, remote } => branches
            .iter()
            .find(|branch| branch.name == *name && branch.remote == *remote)
            .map(|branch| branch.oid.as_str()),
        Scope::Worktree { path } => worktrees
            .iter()
            .find(|worktree| worktree.path == *path)
            .map(|worktree| worktree.oid.as_str()),
    };
    resolved
        .map(Some)
        .ok_or_else(|| VanishedScope(scope.clone()))
}

pub(super) fn image_change(file: &FileChange) -> bool {
    !file.is_submodule()
        && file.old_mode != "120000"
        && file.new_mode != "120000"
        && [file.old_path.as_deref(), file.new_path.as_deref()]
            .into_iter()
            .flatten()
            .any(is_image_path)
}

fn regular_preview(file: &FileChange) -> bool {
    !file.is_submodule() && file.old_mode != "120000" && file.new_mode != "120000"
}

/// Detection operates on captured bytes, never a filesystem filename. Extension
/// hints still give corrupt/empty images an honest decoder error; plain source
/// named .png remains literal text. Stored symlinks and gitlinks bypass rendering.
fn supplied_nontext_content(
    repo: &GitRepository,
    file: &FileChange,
    old: &[u8],
    new: &[u8],
    cancellation: &Cancellation,
) -> Result<Option<Content>> {
    if !regular_preview(file) {
        return Ok(None);
    }
    cancellation.check()?;
    let image = [old, new].iter().any(|bytes| metadata::is_image(bytes))
        || (image_change(file)
            && [old, new].iter().any(|bytes| {
                !metadata::is_literal_text(bytes) || detect_lfs_pointer(bytes).is_some()
            }))
        || (image_change(file)
            && ((file.old_path.is_some() && old.is_empty())
                || (file.new_path.is_some() && new.is_empty())));
    if image {
        let old = working_image_side(
            repo,
            old.to_vec(),
            file.old_path.is_some(),
            &file
                .old_path
                .as_deref()
                .unwrap_or(file.path())
                .to_string_lossy(),
            cancellation,
        );
        cancellation.check()?;
        let new = working_image_side(
            repo,
            new.to_vec(),
            file.new_path.is_some(),
            &file
                .new_path
                .as_deref()
                .unwrap_or(file.path())
                .to_string_lossy(),
            cancellation,
        );
        cancellation.check()?;
        return Ok(Some(Content::Images {
            old: Box::new(old),
            new: Box::new(new),
        }));
    }
    let model = [file.old_path.as_deref(), file.new_path.as_deref()]
        .into_iter()
        .flatten()
        .any(gitturtle_preview::model3d::is_model_path);
    // Models keep independently usable sides when only one LFS object is local.
    // Other documents retain their existing literal-pointer comparison path.
    if !model
        && [old, new]
            .iter()
            .any(|bytes| detect_lfs_pointer(bytes).is_some())
    {
        return Ok(None);
    }
    if !model
        && ![old, new]
            .iter()
            .any(|bytes| !metadata::is_literal_text(bytes) || bytes.starts_with(b"%PDF-"))
    {
        return Ok(None);
    }
    let side = |bytes: &[u8], path: Option<&Path>| -> Result<Arc<crate::rich_preview::Side>> {
        let Some(path) = path else {
            return Ok(Arc::new(crate::rich_preview::Side::unavailable(
                file.path(),
                false,
                None,
            )));
        };
        if bytes.len() > MAX_INPUT_BYTES {
            return Ok(Arc::new(crate::rich_preview::Side::unavailable(
                path,
                true,
                Some(format!(
                    "File contains {} bytes, exceeding the 32 MiB captured preview limit.",
                    bytes.len()
                )),
            )));
        }
        Ok(Arc::new(captured_rich_side(
            repo,
            bytes.to_vec(),
            path,
            if model { "model" } else { "file" },
            cancellation,
        )?))
    };
    Ok(Some(Content::Rich(crate::rich_preview::Comparison {
        old: side(old, file.old_path.as_deref())?,
        new: side(new, file.new_path.as_deref())?,
    })))
}

fn captured_rich_side(
    repo: &GitRepository,
    bytes: Vec<u8>,
    path: &Path,
    kind: &str,
    cancellation: &Cancellation,
) -> Result<crate::rich_preview::Side> {
    cancellation.check()?;
    let Some(pointer) = detect_lfs_pointer(&bytes) else {
        return crate::rich_preview::Side::prepare(bytes, path, || cancellation.check());
    };
    let local = repo.local_lfs_read(&pointer.oid, pointer.size, MAX_INPUT_BYTES);
    cancellation.check()?;
    let absent = matches!(local, Ok(gitturtle_core::LocalLfsObject::Absent));
    let error = match local {
        Ok(gitturtle_core::LocalLfsObject::Verified(local)) => {
            return crate::rich_preview::Side::prepare(local, path, || cancellation.check());
        }
        Ok(gitturtle_core::LocalLfsObject::Absent) => format!(
            "This {kind}’s LFS object is unavailable in the local store ({}). No download was attempted.",
            crate::counted(pointer.size, "byte", "bytes")
        ),
        Ok(gitturtle_core::LocalLfsObject::Failed(failure, _)) => {
            crate::lfs_download::failure_sentence(failure)
        }
        Err(error) => format!("This {kind}’s local LFS object could not be verified: {error:#}."),
    };
    let mut side = crate::rich_preview::Side::unavailable(path, true, Some(error));
    side.lfs_missing = absent;
    side.metadata = metadata::inspect(&bytes, path);
    side.metadata.format = "Git LFS pointer".into();
    // Keep the captured identity available to Copy and explicit download actions.
    // Its error prevents immutable caching while the local store can change.
    side.metadata.source = std::str::from_utf8(&bytes).ok().map(Arc::from);
    side.capture(bytes.into(), &|| cancellation.check())?;
    Ok(side)
}

/// A history comparison that is not text, read from both blobs: a side over
/// the 32 MiB captured preview limit, or that cannot be read, says so beside
/// the other. Whether the result is cached is `Job::Preview`'s one decision.
fn nontext_history_content(
    repo: &GitRepository,
    file: &FileChange,
    known_sizes: [Option<usize>; 2],
    cancellation: &Cancellation,
) -> Result<Option<Content>> {
    if !regular_preview(file) {
        return Ok(None);
    }
    cancellation.check()?;
    let [old_raw, new_raw] = repo.blobs_bounded_independent(
        [
            file.old_oid.as_deref().map(|oid| (oid, known_sizes[0])),
            file.new_oid.as_deref().map(|oid| (oid, known_sizes[1])),
        ],
        MAX_INPUT_BYTES,
    );
    let unpack = |side: Option<Result<BoundedBlob>>| -> Result<Vec<u8>> {
        match side {
            None => Ok(Vec::new()),
            Some(Ok(BoundedBlob::Bytes(bytes))) => Ok(bytes),
            Some(Ok(BoundedBlob::TooLarge(size))) => Err(anyhow!(
                "File has {size} bytes, exceeding the 32 MiB captured preview limit"
            )),
            Some(Err(err)) => Err(err),
        }
    };
    let old = unpack(old_raw);
    let new = unpack(new_raw);
    cancellation.check()?;
    if let (Ok(old), Ok(new)) = (&old, &new) {
        return supplied_nontext_content(repo, file, old, new, cancellation);
    }
    if image_change(file)
        || [old.as_ref().ok(), new.as_ref().ok()]
            .into_iter()
            .flatten()
            .any(|bytes| metadata::is_image(bytes))
    {
        let side = |bytes: Result<Vec<u8>>, path: Option<&Path>| {
            let name = path.unwrap_or(file.path()).to_string_lossy();
            match bytes {
                Ok(bytes) => working_image_side(repo, bytes, path.is_some(), &name, cancellation),
                Err(error) => ImageSide {
                    animation: None,
                    image: None,
                    render: None,
                    message: Some(format!("{error:#}")),
                    bytes: 0,
                    lfs_pointer: None,
                    captured: None,
                    system_preview: None,
                    literal_source: None,
                },
            }
        };
        let old = side(old, file.old_path.as_deref());
        cancellation.check()?;
        let new = side(new, file.new_path.as_deref());
        cancellation.check()?;
        return Ok(Some(Content::Images {
            old: Box::new(old),
            new: Box::new(new),
        }));
    }
    let model = [file.old_path.as_deref(), file.new_path.as_deref()]
        .into_iter()
        .flatten()
        .any(gitturtle_preview::model3d::is_model_path);
    // Keep readable and missing sides independent; a missing historical object
    // never substitutes a working file and is never cached as permanently absent.
    let side =
        |bytes: Result<Vec<u8>>, path: Option<&Path>| -> Result<Arc<crate::rich_preview::Side>> {
            let Some(path) = path else {
                return Ok(Arc::new(crate::rich_preview::Side::unavailable(
                    file.path(),
                    false,
                    None,
                )));
            };
            Ok(Arc::new(match bytes {
                Ok(bytes) => captured_rich_side(
                    repo,
                    bytes,
                    path,
                    if model { "model" } else { "file" },
                    cancellation,
                )?,
                Err(error) => {
                    crate::rich_preview::Side::unavailable(path, true, Some(format!("{error:#}")))
                }
            }))
        };
    Ok(Some(Content::Rich(crate::rich_preview::Comparison {
        old: side(old, file.old_path.as_deref())?,
        new: side(new, file.new_path.as_deref())?,
    })))
}

pub(super) fn prepared_text(
    patch: String,
    old: String,
    new: String,
    prepare_split: bool,
) -> Content {
    let presentation = Arc::new(PatchPresentation::prepare(&patch));
    let split = prepare_split.then(|| {
        Arc::new(crate::split_diff::SplitPresentation::prepare(
            &old,
            &new,
            &presentation,
        ))
    });
    Content::Text {
        diagrams: None,
        markdown: None,
        patch,
        old,
        new,
        presentation,
        split,
        split_partial: None,
        partial: None,
        partial_unavailable: None,
        lfs_missing: [false; 2],
        lfs_failed: [None; 2],
    }
}

/// Builds an upgraded `Content::Text` with `split: Some(...)` from one whose
/// split presentation was deferred until Split mode was chosen. Returning a new
/// `Arc<Content>` keeps cached `Content` values immutable so `PreviewCache::bytes`
/// stays exact across upgrades and evictions.
fn ensure_split_in_content(content: &Arc<Content>) -> Option<Arc<Content>> {
    let Content::Text {
        diagrams,
        markdown,
        patch,
        old,
        new,
        presentation,
        split: None,
        split_partial: _,
        partial,
        partial_unavailable,
        lfs_missing,
        lfs_failed,
    } = content.as_ref()
    else {
        return None;
    };
    let split = Arc::new(crate::split_diff::SplitPresentation::prepare(
        old,
        new,
        presentation,
    ));
    let split_partial = partial.as_ref().and_then(|actions| {
        gitturtle_diff_model::split_diff::prepare_split_partial(&split, Arc::clone(&actions.diff))
            .ok()
            .map(Arc::new)
    });
    Some(Arc::new(Content::Text {
        diagrams: diagrams.clone(),
        markdown: markdown.clone(),
        patch: patch.clone(),
        old: old.clone(),
        new: new.clone(),
        presentation: Arc::clone(presentation),
        split: Some(split),
        split_partial,
        partial: partial.clone(),
        partial_unavailable: partial_unavailable.clone(),
        lfs_missing: *lfs_missing,
        lfs_failed: *lfs_failed,
    }))
}

/// What the local store said of each side's Git LFS object
/// ([`GitRepository::resolved_lfs_text`]), old then new.
#[derive(Clone, Copy, Default)]
pub(super) struct LfsSides {
    pub(super) missing: [bool; 2],
    pub(super) failed: [Option<gitturtle_core::LfsObjectFailure>; 2],
    /// Which sides are stored as pointers, whatever their objects: what a
    /// side resolved to no longer shows it.
    pub(super) pointers: [bool; 2],
}

impl LfsSides {
    /// Whether what these sides say could change under the same pointers: a
    /// missing object can still arrive, and a corrupt object or a store that
    /// cannot be read can be repaired, so none of them is kept, whatever the
    /// other side holds, a notice that shows neither side included. Being
    /// over the preview limit is the pointer's alone.
    fn store_can_change(self) -> bool {
        self.missing.contains(&true)
            || self
                .failed
                .iter()
                .flatten()
                .any(|failure| *failure != gitturtle_core::LfsObjectFailure::TooLarge)
    }

    /// Records them on `content`. Text shows each side beside these marks;
    /// a notice, which shows neither side, says what is wrong with any side
    /// that failed, so a binary comparison does not hide it.
    pub(super) fn mark(self, content: &mut Content) {
        match content {
            Content::Text {
                lfs_missing,
                lfs_failed,
                ..
            } => {
                *lfs_missing = self.missing;
                *lfs_failed = self.failed;
            }
            Content::Notice(message) => {
                for (side, failure) in ["Before", "After"].into_iter().zip(self.failed) {
                    if let Some(failure) = failure {
                        let words = crate::lfs_download::failure_words(failure);
                        message.push_str(&format!("\n\n{side}: {words}."));
                    }
                }
            }
            _ => {}
        }
    }
}

fn attach_mermaid(
    content: &mut Content,
    file: &FileChange,
    stored_pointers: [bool; 2],
    cancellation: &Cancellation,
) -> Result<()> {
    if let Content::Text {
        old,
        new,
        markdown,
        lfs_failed,
        ..
    } = content
        && markdown.is_none()
    {
        *markdown =
            crate::markdown_view::prepare(old, new, file, *lfs_failed, stored_pointers, || {
                cancellation.check()
            })?
            .map(Arc::new);
    }
    if let Content::Text {
        old,
        new,
        diagrams,
        markdown,
        lfs_failed,
        ..
    } = content
        && diagrams.is_none()
    {
        *diagrams = if let Some(markdown) = markdown {
            markdown.diagrams().map(Arc::new)
        } else {
            crate::rich_preview::prepare_mermaid(old, new, file, *lfs_failed, || {
                cancellation.check()
            })?
            .map(Arc::new)
        };
    }
    Ok(())
}

fn attach_markdown_assets(
    content: &mut Content,
    repo: &GitRepository,
    file: &FileChange,
    origins: &crate::markdown_view::Origins,
    cancellation: &Cancellation,
) -> Result<()> {
    if let Content::Text {
        markdown: Some(markdown),
        ..
    } = content
        && let Some(markdown) = Arc::get_mut(markdown)
    {
        crate::markdown_view::capture_assets(markdown, repo, file, origins, &cancellation.history)?;
    }
    cancellation.check()
}

/// Compare prepared mutable content off UI before deciding whether native
/// editors, image views, or partial selection snapshots need replacement.
pub(super) fn content_unchanged(previous: &Content, next: &Content) -> bool {
    match (previous, next) {
        (
            Content::Text {
                patch: a,
                old: ao,
                new: an,
                partial_unavailable: au,
                partial: ap,
                diagrams: ad,
                markdown: am,
                lfs_missing: aq,
                lfs_failed: ar,
                ..
            },
            Content::Text {
                patch: b,
                old: bo,
                new: bn,
                partial_unavailable: bu,
                partial: bp,
                diagrams: bd,
                markdown: bm,
                lfs_missing: bq,
                lfs_failed: br,
                ..
            },
        ) => {
            a == b
                && aq == bq
                && ar == br
                && match (am, bm) {
                    (None, None) => true,
                    (Some(a), Some(b)) => a.same_source(b),
                    _ => false,
                }
                && match (ad, bd) {
                    (None, None) => true,
                    (Some(a), Some(b)) => a.old.same_source(&b.old) && a.new.same_source(&b.new),
                    _ => false,
                }
                && ao == bo
                && an == bn
                && au == bu
                && ap.as_ref().map(|partial| &partial.diff)
                    == bp.as_ref().map(|partial| &partial.diff)
        }
        (Content::Notice(a), Content::Notice(b)) => a == b,
        (Content::Rich(a), Content::Rich(b)) => {
            a.old.same_source(&b.old) && a.new.same_source(&b.new)
        }
        (Content::Conflict(a), Content::Conflict(b)) => a.snapshot == b.snapshot,
        (Content::Images { old: ao, new: an }, Content::Images { old: bo, new: bn }) => {
            [ao, an].into_iter().zip([bo, bn]).all(|(a, b)| {
                a.bytes == b.bytes
                    && a.captured == b.captured
                    && a.message == b.message
                    && a.image == b.image
                    && match (&a.render, &b.render) {
                        (None, None) => true,
                        (Some(a), Some(b)) => {
                            a.frame_count() == b.frame_count()
                                && (0..a.frame_count())
                                    .all(|frame| a.as_bytes(frame) == b.as_bytes(frame))
                        }
                        _ => false,
                    }
            })
        }
        _ => false,
    }
}

/// The history comparison of `file`. `lfs` receives what the local store said
/// of each side's Git LFS object, for the caller's decision to keep it.
fn text_content(
    repo: &GitRepository,
    file: &FileChange,
    lfs: &mut LfsSides,
    prepare_split: bool,
    cancellation: &Cancellation,
) -> Result<Content> {
    let sources = match repo.text_preview_with_sources(file) {
        Ok(sources) => sources,
        Err(error) => {
            if let Some(content) = nontext_history_content(repo, file, [None, None], cancellation)?
            {
                return Ok(content);
            }
            return Err(error);
        }
    };
    cancellation.check()?;
    if let TextPreview::TooLarge {
        old_bytes,
        new_bytes,
    } = sources.preview
    {
        let known_sizes = [
            file.old_oid.as_ref().map(|_| old_bytes),
            file.new_oid.as_ref().map(|_| new_bytes),
        ];
        if let Some(content) = nontext_history_content(repo, file, known_sizes, cancellation)? {
            return Ok(content);
        }
        // `TextPreview::TooLarge` returns empty `sources.old`/`sources.new`,
        // which would otherwise hide an LFS pointer on the smaller side and
        // permanently cache the large-file notice. Probe any <=1 KiB side for
        // an LFS pointer so missing/failing LFS state prevents cache poisoning.
        for (index, oid) in [file.old_oid.as_deref(), file.new_oid.as_deref()]
            .into_iter()
            .enumerate()
        {
            if let Some(oid) = oid
                && let Ok(BoundedBlob::Bytes(bytes)) = repo.blob_bounded(oid, 1024, None)
                && let Some(pointer) = detect_lfs_pointer(&bytes)
            {
                lfs.pointers[index] = true;
                match repo.local_lfs_read(&pointer.oid, pointer.size, MAX_INPUT_BYTES) {
                    Ok(gitturtle_core::LocalLfsObject::Absent) => lfs.missing[index] = true,
                    Ok(gitturtle_core::LocalLfsObject::Failed(failure, _)) => {
                        lfs.failed[index] = Some(failure);
                    }
                    _ => {}
                }
            }
        }
    } else if let Some(content) =
        supplied_nontext_content(repo, file, &sources.old, &sources.new, cancellation)?
    {
        return Ok(content);
    }
    if let Some(mut content) = resolved_lfs_text_content(
        repo,
        file,
        &sources.old,
        &sources.new,
        lfs,
        prepare_split,
        cancellation,
    )? {
        lfs.mark(&mut content);
        attach_mermaid(&mut content, file, lfs.pointers, cancellation)?;
        return Ok(content);
    }
    let mut content = match sources.preview {
        TextPreview::Patch(patch) => prepared_text(
            patch,
            String::from_utf8(sources.old)?,
            String::from_utf8(sources.new)?,
            prepare_split,
        ),
        TextPreview::Binary => Content::Notice(
            "Binary or non-UTF-8 file changed. Raw object IDs and file metadata are available above.".into(),
        ),
        TextPreview::TooLarge { old_bytes, new_bytes } => Content::Notice(format!(
            "Large file · {old_bytes} → {new_bytes} bytes\nThis change exceeds the automatic text preview budget. File metadata remains available."
        )),
        TextPreview::Submodule { old_oid, new_oid } => Content::Notice(format!(
            "Submodule reference changed\n\nBefore: {}\nAfter: {}",
            old_oid.as_deref().unwrap_or("Absent"), new_oid.as_deref().unwrap_or("Absent")
        )),
    };
    lfs.mark(&mut content);
    attach_mermaid(&mut content, file, lfs.pointers, cancellation)?;
    Ok(content)
}

/// The comparison with the Git LFS objects the local store could show, or
/// `None` to keep the stored texts. `lfs` receives which sides' objects are
/// truly absent or unusable either way, for the caller to mark on what it
/// shows before it attaches Mermaid, which reads those marks.
fn resolved_lfs_text_content(
    repo: &GitRepository,
    file: &FileChange,
    old: &[u8],
    new: &[u8],
    lfs: &mut LfsSides,
    prepare_split: bool,
    cancellation: &Cancellation,
) -> Result<Option<Content>> {
    let resolved = repo.resolved_lfs_text(file, old, new)?;
    *lfs = LfsSides {
        missing: resolved.missing,
        failed: resolved.failed,
        pointers: resolved.pointers,
    };
    let Some(sources) = resolved.text else {
        return Ok(None);
    };
    if ([file.old_path.as_deref(), file.new_path.as_deref()]
        .into_iter()
        .flatten()
        .any(gitturtle_preview::model3d::is_model_path)
        || [sources.old.as_slice(), sources.new.as_slice()]
            .iter()
            .any(|bytes| !metadata::is_literal_text(bytes) || bytes.starts_with(b"%PDF-")))
        && let Some(content) =
            supplied_nontext_content(repo, file, &sources.old, &sources.new, cancellation)?
    {
        return Ok(Some(content));
    }
    Ok(Some(match sources.preview {
        TextPreview::Patch(patch) => prepared_text(
            patch,
            String::from_utf8(sources.old)?,
            String::from_utf8(sources.new)?,
            prepare_split,
        ),
        TextPreview::Binary => Content::Notice("Verified LFS object downloaded. Its content is binary or not UTF-8 and has no text preview.".into()),
        TextPreview::TooLarge { old_bytes, new_bytes } => Content::Notice(format!("Verified LFS content exceeds the text display limit · {old_bytes} → {new_bytes} bytes.")),
        TextPreview::Submodule { .. } => Content::Notice("Submodules do not have an LFS text preview.".into()),
    }))
}

pub(super) fn working_image_side(
    repo: &GitRepository,
    mut bytes: Vec<u8>,
    present: bool,
    name: &str,
    cancellation: &Cancellation,
) -> ImageSide {
    if !present {
        return ImageSide {
            animation: None,
            captured: None,
            system_preview: None,
            literal_source: None,
            image: None,
            render: None,
            message: None,
            bytes: 0,
            lfs_pointer: None,
        };
    }
    let mut missing_pointer = None;
    // The side is left holding its pointer's text, not an image's bytes.
    let mut stays_pointer = false;
    let mut animation = None;
    let mut preview_notice = None;
    let result = (|| -> Result<(ImageMetadata, Arc<RenderImage>, usize)> {
        cancellation.check()?;
        ensure!(
            bytes.len() <= MAX_INPUT_BYTES,
            "Image exceeds the 32 MiB input limit"
        );
        if let Some(pointer) = detect_lfs_pointer(&bytes) {
            // Before the read, so a read that fails outright leaves it too.
            stays_pointer = true;
            match repo.local_lfs_read(&pointer.oid, pointer.size, MAX_INPUT_BYTES)? {
                gitturtle_core::LocalLfsObject::Verified(local) => {
                    bytes = local;
                    stays_pointer = false;
                }
                gitturtle_core::LocalLfsObject::Absent => {
                    missing_pointer = Some(bytes.clone());
                    return Err(anyhow!(
                        "Git LFS object is unavailable in the local store · {}. No download was attempted.",
                        crate::counted(pointer.size, "byte", "bytes")
                    ));
                }
                gitturtle_core::LocalLfsObject::Failed(failure, _) => {
                    return Err(anyhow!(crate::lfs_download::failure_sentence(failure)));
                }
            }
        }
        cancellation.check()?;
        if gitturtle_preview::animation::is_gif(&bytes) {
            match crate::gif_playback::prepare(&bytes, || cancellation.check()) {
                Ok(prepared) => {
                    cancellation.check()?;
                    animation = Some(prepared.timeline);
                    return Ok((prepared.image, prepared.render, bytes.len()));
                }
                Err(error) => {
                    cancellation.check()?;
                    preview_notice = Some(format!(
                        "Static first frame only; animation unavailable: {error:#}"
                    ));
                }
            }
        }
        // AVIF and JPEG 2000 decode in the confined helper on Linux, which
        // the reader's check kills as soon as a newer request supersedes
        // this one. An image decoded in this process checks nothing until it
        // ends, so the same preview asked for again meanwhile takes the
        // decode over (`Cancellation::unobserved`).
        let decode =
            || crate::image_helper::decode(&bytes, name, PREVIEW_EDGE, &|| cancellation.check());
        let decoded = if decoded_in_process(&bytes) {
            cancellation.unobserved(decode)??
        } else {
            decode()?
        };
        cancellation.check()?;
        let (metadata, render) = render_image_with_metadata(decoded)?;
        Ok((metadata, render, bytes.len()))
    })();
    let literal_source = (bytes.len() <= gitturtle_preview::MAX_SVG_BYTES)
        .then(|| std::str::from_utf8(&bytes).ok().map(Arc::<str>::from))
        .flatten();
    let captured =
        (bytes.len() <= MAX_INPUT_BYTES && !stays_pointer).then(|| Arc::<[u8]>::from(bytes));
    // A cancelled decision offers nothing; the caller's next check ends the job.
    let system_preview = captured.as_deref().and_then(|bytes| {
        crate::system_preview::offer(bytes, &|| cancellation.check())
            .ok()
            .flatten()
    });
    match result {
        Ok((image, render, bytes)) => ImageSide {
            animation,
            captured,
            system_preview,
            literal_source,
            image: Some(image),
            render: Some(render),
            message: preview_notice,
            bytes,
            lfs_pointer: None,
        },
        Err(error) => ImageSide {
            animation: None,
            captured,
            system_preview,
            literal_source,
            image: None,
            render: None,
            message: Some(format!("{error:#}")),
            bytes: 0,
            lfs_pointer: missing_pointer,
        },
    }
}

/// Whether [`crate::image_helper::decode`] decodes `bytes` in this process,
/// where it checks nothing until it ends, rather than in the confined helper,
/// which runs the reader's check at least every 25 ms.
fn decoded_in_process(bytes: &[u8]) -> bool {
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        gitturtle_preview::confined_image_format(bytes).is_none()
    }
    #[cfg(not(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    {
        let _ = bytes;
        true
    }
}

pub(crate) fn render_image(mut preview: ImagePreview) -> Result<Arc<RenderImage>> {
    // GPUI RenderImage expects BGRA although image::Frame names the buffer RGBA.
    // Swizzle in place so large images do not duplicate their decoded pixel buffer.
    let (pixels, remainder) = preview.rgba.as_chunks_mut::<4>();
    ensure!(remainder.is_empty(), "Invalid preview pixel buffer");
    for pixel in pixels {
        pixel.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(preview.width, preview.height, preview.rgba)
        .ok_or_else(|| anyhow!("Invalid preview dimensions"))?;
    Ok(Arc::new(RenderImage::new([image::Frame::new(buffer)])))
}

pub(super) fn render_image_with_metadata(
    mut preview: ImagePreview,
) -> Result<(ImageMetadata, Arc<RenderImage>)> {
    let metadata = ImageMetadata {
        width: preview.width,
        height: preview.height,
        original_width: preview.original_width,
        original_height: preview.original_height,
        format: std::mem::take(&mut preview.format),
    };
    let render = render_image(preview)?;
    Ok((metadata, render))
}
