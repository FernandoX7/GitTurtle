//! Release-only passive review-path measurements. Fixture construction and
//! provenance/inventory reporting live in scripts/bench-review.py.
use anyhow::{Context, Result, ensure};
use gitturtle_core::{
    ComparisonMode, GitRepository, HistoryCancellation, LfsDownloadTarget, PathScope,
    text_conflict_blocks,
};
use sha2::{Digest, Sha256};
use std::{env, path::PathBuf, time::Instant};

const WARMUPS: usize = 3;
const SAMPLES: usize = 40;

fn json(value: &str) -> String {
    let mut out = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                out.push_str(&format!("\\u{:04x}", character as u32))
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

fn fingerprint(value: &impl std::fmt::Debug) -> String {
    format!("{:x}", Sha256::digest(format!("{value:?}").as_bytes()))
}

fn measure<T: std::fmt::Debug>(
    name: &str,
    mut read: impl FnMut() -> Result<T>,
    describe: impl Fn(&T) -> String,
) -> bool {
    eprintln!("Measuring {name}: {WARMUPS} warmups, {SAMPLES} attempts");
    let mut previous = None;
    let mut stable = true;
    for attempt in 0..WARMUPS + SAMPLES {
        let start = Instant::now();
        let result = read();
        let elapsed = start.elapsed().as_secs_f64() * 1000.;
        // Fingerprints, validation, printing and destruction are outside timing.
        let (success, digest, details, error, changed) = match &result {
            Ok(value) => {
                let digest = fingerprint(value);
                let changed = previous
                    .as_ref()
                    .is_some_and(|previous| previous != &digest);
                previous = Some(digest.clone());
                stable &= !changed;
                (true, digest, describe(value), String::new(), changed)
            }
            Err(error) => {
                stable = false;
                (
                    false,
                    String::new(),
                    String::new(),
                    format!("{error:#}"),
                    false,
                )
            }
        };
        println!(
            "{{\"series\":{},\"phase\":{},\"sample\":{},\"elapsed_ms\":{elapsed:.9},\"success\":{success},\"result_changed\":{changed},\"result_sha256\":{},\"details\":{},\"error\":{}}}",
            json(name),
            json(if attempt < WARMUPS {
                "warmup"
            } else {
                "measured"
            }),
            if attempt < WARMUPS {
                attempt + 1
            } else {
                attempt - WARMUPS + 1
            },
            json(&digest),
            json(&details),
            json(&error),
        );
    }
    stable
}

fn conflict_document(blocks: usize) -> String {
    let mut source = String::new();
    for block in 0..blocks {
        source.push_str(&format!("unchanged prefix {block:04}\r\n<<<<<<< current\r\ncurrent version 🐢 {block:04}\r\n||||||| common ancestor\r\nbase version {block:04}\r\n=======\r\nincoming version Ω {block:04}\r\n>>>>>>> incoming\r\nunchanged suffix {block:04}\r\n"));
    }
    source
}

fn main() -> Result<()> {
    ensure!(
        !cfg!(debug_assertions),
        "Build review_bench in release mode"
    );
    let mut args = env::args_os().skip(1);
    let root = PathBuf::from(
        args.next()
            .context("Usage: review_bench DISPOSABLE_REPOSITORY")?,
    );
    ensure!(args.next().is_none(), "Unexpected argument");
    let repo = GitRepository::open(root)?;
    let initial = repo.status()?;
    let before = "refs/heads/review-before";
    let after = "refs/heads/main";
    let pinned = repo
        .resolve_inspection_revision(after, &HistoryCancellation::default())?
        .oid;
    let linked = repo
        .worktrees()?
        .into_iter()
        .find(|tree| tree.branch.as_deref() == Some("bench-linked"))
        .context("Fixture has no bench-linked worktree")?;
    let lfs = repo
        .search_tracked_paths(
            &PathScope::Revision(pinned.clone()),
            "assets/turtle-pointer.bin",
            &HistoryCancellation::default(),
        )?
        .entries
        .into_iter()
        .next()
        .context("Fixture has no LFS pointer")?;
    let target = LfsDownloadTarget {
        path: lfs.path,
        pointer: repo.blob(&lfs.oid)?,
        blob_oid: Some(lfs.oid),
    };
    let small_blocks = conflict_document(256);
    let large_blocks = conflict_document(2048);
    let mut stable = true;
    for (name, mode) in [
        ("compare_endpoints", ComparisonMode::Endpoints),
        ("compare_since_branching", ComparisonMode::SinceBranching),
    ] {
        stable &= measure(
            name,
            || repo.compare_revisions(before, after, mode, &HistoryCancellation::default()),
            |result| {
                format!(
                    "files={};base={};before={};after={}",
                    result.files.len(),
                    result.base_oid,
                    result.before.oid,
                    result.after.oid
                )
            },
        );
    }
    for (name, scope, query) in [
        (
            "quick_open_worktree_selective",
            PathScope::Worktree,
            "module_001",
        ),
        (
            "quick_open_revision_selective",
            PathScope::Revision(pinned.clone()),
            "module_001",
        ),
        (
            "quick_open_revision_no_matches",
            PathScope::Revision(pinned.clone()),
            "missing-review-match",
        ),
        (
            "quick_open_worktree_bounded_results",
            PathScope::Worktree,
            "module_",
        ),
    ] {
        stable &= measure(
            name,
            || repo.search_tracked_paths(&scope, query, &HistoryCancellation::default()),
            |result| {
                format!(
                    "matches={};scanned={};truncated={}",
                    result.entries.len(),
                    result.total_scanned,
                    result.truncated
                )
            },
        );
    }
    for (name, source) in [
        ("conflict_parse_256", &small_blocks),
        ("conflict_parse_2048", &large_blocks),
    ] {
        stable &= measure(
            name,
            || text_conflict_blocks(std::hint::black_box(source)),
            |result| format!("bytes={};blocks={}", source.len(), result.len()),
        );
    }
    stable &= measure(
        "interactive_rebase_plan_50",
        || repo.interactive_rebase_plan("refs/heads/rebase-base"),
        |plan| {
            format!(
                "commits={};published_refs={};base={};head={}",
                plan.commits.len(),
                plan.known_published_refs.len(),
                plan.base,
                plan.head
            )
        },
    );
    stable &= measure(
        "worktree_list",
        || repo.worktrees(),
        |trees| format!("worktrees={}", trees.len()),
    );
    stable &= measure(
        "selected_worktree_details",
        || repo.worktree_details(&linked),
        |details| {
            format!(
                "changed={};ignored={};main={};missing={};locked={}",
                details.changed_files,
                details.ignored_files,
                details.main,
                details.missing,
                details.tree.locked
            )
        },
    );
    stable &= measure(
        "head_reflog",
        || repo.reflog("HEAD"),
        |page| {
            format!(
                "entries={};truncated={}",
                page.entries.len(),
                page.truncated
            )
        },
    );
    stable &= measure(
        "missing_lfs_download_plan",
        || repo.lfs_download_plan(&target, "origin"),
        |plan| {
            format!(
                "size={};oid={};pointer_blob={}",
                plan.size,
                plan.oid,
                plan.target.blob_oid.as_deref().unwrap_or("working")
            )
        },
    );
    let status_unchanged = repo.status()? == initial;
    println!(
        "{{\"type\":\"validation\",\"status_unchanged\":{status_unchanged},\"results_stable\":{stable},\"initial_head\":{},\"warmups_per_series\":{WARMUPS},\"measured_attempts_per_series\":{SAMPLES}}}",
        json(initial.head.as_deref().unwrap_or(""))
    );
    ensure!(
        stable && status_unchanged,
        "Measurement encountered errors, changed results, or changed status; retain the raw attempts"
    );
    Ok(())
}
