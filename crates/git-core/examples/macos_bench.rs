//! Read-only synchronous core measurements for the macOS milestone.
//! Emits raw CSV; fixture/provenance collection and JSON conversion are documented
//! in docs/benchmarks/2026-09-08-macos-milestone-backend.md.
use anyhow::{Context, Result, ensure};
use gitturtle_core::{Blame, BlameTarget, GitRepository, HistoryCancellation, IgnoreDestination};
use sha2::{Digest, Sha256};
use std::{env, path::PathBuf, time::Instant};

const WARMUPS: usize = 3;
const SAMPLES: usize = 24;

fn fingerprint(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn measure<T>(
    name: &str,
    mut read: impl FnMut() -> Result<T>,
    summarize: impl Fn(&T) -> String,
) -> Result<()> {
    let mut previous = None;
    for index in 0..WARMUPS + SAMPLES {
        let start = Instant::now();
        let result = read()?;
        let elapsed = start.elapsed().as_secs_f64() * 1000.;
        // Validation and destruction occur after timing, never inside a sample.
        let summary = summarize(&result);
        if let Some(previous) = &previous {
            ensure!(
                previous == &summary,
                "{name} result changed during measurement"
            );
        }
        previous = Some(summary.clone());
        println!(
            "{name},{},{},{elapsed:.6},{summary}",
            if index < WARMUPS {
                "warmup"
            } else {
                "measured"
            },
            if index < WARMUPS {
                index + 1
            } else {
                index - WARMUPS + 1
            }
        );
    }
    Ok(())
}

fn blame_summary(blame: &Blame) -> String {
    format!(
        "lines={};uncommitted={};sha256={}",
        blame.lines.len(),
        blame
            .lines
            .iter()
            .filter(|line| line.attribution.is_none())
            .count(),
        fingerprint(format!("{:?}", blame.lines).as_bytes()),
    )
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let repository = PathBuf::from(
        args.next()
            .context("Usage: macos_bench REPOSITORY TRACKED_PATH UNTRACKED_PATH LINE")?,
    );
    let tracked = PathBuf::from(args.next().context("Missing tracked relative path")?);
    let untracked = PathBuf::from(args.next().context("Missing untracked relative path")?);
    let line: usize = args
        .next()
        .context("Missing one-based line number")?
        .into_string()
        .map_err(|_| anyhow::anyhow!("Invalid line number"))?
        .parse()?;
    ensure!(args.next().is_none(), "Unexpected argument");
    ensure!(
        !cfg!(debug_assertions),
        "Build this harness in release mode"
    );
    let repo = GitRepository::open(repository)?;
    let initial = repo.status()?;
    let head = initial
        .head
        .clone()
        .context("Fixture requires a committed HEAD")?;
    ensure!(
        initial
            .entries
            .iter()
            .any(|entry| entry.path == untracked && entry.untracked),
        "Fixture requires the selected untracked path"
    );
    let target = BlameTarget::Committed {
        oid: head.clone(),
        path: tracked.clone(),
    };
    let working = BlameTarget::Working {
        path: tracked.clone(),
    };
    println!("series,phase,sample,ms,result");
    measure(
        "committed_blame",
        || repo.blame(&target, &HistoryCancellation::default()),
        blame_summary,
    )?;
    measure(
        "working_blame",
        || repo.blame(&working, &HistoryCancellation::default()),
        blame_summary,
    )?;
    measure(
        "line_history",
        || repo.line_history(&head, &tracked, line, &HistoryCancellation::default()),
        |history| {
            format!(
                "commits={};truncated={};sha256={}",
                history.commits.len(),
                history.truncated,
                fingerprint(format!("{:?}", history.commits).as_bytes())
            )
        },
    )?;
    measure(
        "tag_list",
        || repo.tags(),
        |tags| {
            format!(
                "tags={};annotated={};truncated={};sha256={}",
                tags.tags.len(),
                tags.tags.iter().filter(|tag| tag.annotated).count(),
                tags.truncated,
                fingerprint(format!("{:?}", tags.tags).as_bytes())
            )
        },
    )?;
    measure(
        "status",
        || repo.status(),
        |status| {
            format!(
                "entries={};sha256={}",
                status.entries.len(),
                fingerprint(format!("{status:?}").as_bytes())
            )
        },
    )?;
    measure(
        "ignore_file_shared_plan",
        || repo.ignore_plan(&untracked, false, IgnoreDestination::Shared),
        |plan| {
            format!(
                "rule_bytes={};tracked_paths={};sha256={}",
                plan.rule.len(),
                plan.tracked_paths,
                fingerprint(format!("{plan:?}").as_bytes())
            )
        },
    )?;
    measure(
        "ignore_directory_local_plan",
        || repo.ignore_plan(&untracked, true, IgnoreDestination::Local),
        |plan| {
            format!(
                "rule_bytes={};tracked_paths={};sha256={}",
                plan.rule.len(),
                plan.tracked_paths,
                fingerprint(format!("{plan:?}").as_bytes())
            )
        },
    )?;
    ensure!(
        repo.status()? == initial,
        "Fixture status changed during measurement"
    );
    Ok(())
}
