//! Release comparison of ordinary growing-prefix reads and bounded traversal.
//! CSV output belongs outside the observed fixture. See bench-history-pagination.py.
use anyhow::{Context, Result, ensure};
use gitturtle_core::{GitRepository, HistoryCancellation, HistoryScope};
use sha2::{Digest, Sha256};
use std::{
    env,
    time::{Duration, Instant},
};

fn fingerprint(commits: &[gitturtle_core::Commit]) -> String {
    let mut digest = Sha256::new();
    for commit in commits {
        digest.update(commit.oid.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

fn main() -> Result<()> {
    ensure!(!cfg!(debug_assertions), "Use release mode");
    let mut args = env::args().skip(1);
    let path = args.next().context("repository")?;
    let count: usize = args.next().context("commit count")?.parse()?;
    let samples: usize = args.next().context("sample count")?.parse()?;
    ensure!(
        count >= 100_000 && count.is_multiple_of(500),
        "Use >=100,000 commits in 500-row pages"
    );
    let repo = GitRepository::open(&path)?;
    // Explicit warm OS-cache pass; no app metadata or preview cache is reused.
    let warmup = repo.history(count)?;
    let expected: Vec<_> = warmup.chunks(500).map(fingerprint).collect();
    ensure!(expected.len() == count / 500, "Fixture is too short");
    let selections: Vec<_> = warmup
        .iter()
        .step_by(251)
        .take(40)
        .map(|c| c.oid.clone())
        .collect();
    drop(warmup);
    println!("mode,sample,offset,rows,elapsed_ms,retained_metadata_bytes,fingerprint");
    for sample in 0..samples {
        let modes = if sample % 2 == 0 {
            ["prefix", "incremental"]
        } else {
            ["incremental", "prefix"]
        };
        for mode in modes {
            let repo = GitRepository::open(&path)?;
            let cancel = HistoryCancellation::default();
            let mut traversal = None;
            for offset in (0..count).step_by(500) {
                let started = Instant::now();
                let commits = if mode == "prefix" {
                    repo.history(offset + 500)?
                } else {
                    if traversal.is_none() {
                        traversal = Some(repo.history_traversal(&HistoryScope::AllRefs, &cancel)?);
                    }
                    traversal.as_mut().unwrap().next_page(500, &cancel)?.commits
                };
                let elapsed = started.elapsed().as_secs_f64() * 1000.;
                let page = &commits[commits.len() - 500..];
                let hash = fingerprint(page);
                ensure!(
                    hash == expected[offset / 500],
                    "Traversal order changed at {offset}"
                );
                let retained = commits.iter().map(|c| c.history_bytes()).sum::<usize>();
                println!(
                    "{mode},{sample},{offset},{},{elapsed:.6},{retained},{hash}",
                    commits.len()
                );
            }
        }
    }
    for sample in 0..20 {
        let cancel = HistoryCancellation::default();
        let mut traversal = repo.history_traversal(&HistoryScope::AllRefs, &cancel)?;
        let signal = cancel.clone();
        let cancellation = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(2));
            signal.cancel();
        });
        let started = Instant::now();
        let result = traversal.next_page(500, &cancel);
        let elapsed = started.elapsed().as_secs_f64() * 1000.;
        cancellation.join().unwrap();
        let disposition = match result {
            Ok(_) => "completed_before_signal",
            Err(_) => "cancelled",
        };
        println!("cancel,{sample},0,0,{elapsed:.6},0,{disposition}");
    }
    for (sample, oid) in selections.iter().enumerate() {
        let started = Instant::now();
        let files = repo.changes_with_renames(oid, 0)?;
        let elapsed = started.elapsed().as_secs_f64() * 1000.;
        println!("selection,{sample},0,{},{} ,0,{oid}", files.len(), elapsed);
    }
    Ok(())
}
