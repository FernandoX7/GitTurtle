//! Bounded, read-only timing harness. Pass a repository path as the first arg.
use anyhow::{Context, Result};
use gitturtle_core::GitRepository;
use std::{
    env,
    time::{Duration, Instant},
};

fn main() -> Result<()> {
    let path = env::args_os().nth(1).context(
        "Usage: cargo run --release -p gitturtle-core --example inspect -- /path/to/repository",
    )?;
    let start = Instant::now();
    let repo = GitRepository::open(path)?;
    let opened = start.elapsed();
    let start = Instant::now();
    let branches = repo.branches()?;
    let worktrees = repo.worktrees()?;
    let navigation = start.elapsed();
    let start = Instant::now();
    let commits = repo.history(500)?;
    let history = start.elapsed();
    let mut file_times = Vec::new();
    let mut preview_times = Vec::new();
    let mut inspection_times = Vec::new();
    let mut missing_previews = 0;
    for commit in commits.iter().take(50) {
        let inspection_start = Instant::now();
        let start = Instant::now();
        let changes = repo.changes(&commit.oid, 0)?;
        file_times.push(start.elapsed());
        if let Some(file) = changes.first() {
            let start = Instant::now();
            if repo.text_preview(file).is_err() {
                missing_previews += 1;
            }
            preview_times.push(start.elapsed());
        }
        inspection_times.push(inspection_start.elapsed());
    }
    println!("Read-only inspection: {}", repo.name());
    println!(
        "Open: {opened:.2?}; {} branches + {} worktrees: {navigation:.2?}; {} commits: {history:.2?}",
        branches.len(),
        worktrees.len(),
        commits.len()
    );
    print_timings("Changed-file list", &mut file_times);
    print_timings("First-file text preview", &mut preview_times);
    print_timings("File list + first-file text preview", &mut inspection_times);
    println!(
        "Unavailable previews: {missing_previews}. Timings exclude UI, image decode and syntax highlighting; OS cache was not flushed."
    );
    Ok(())
}

fn print_timings(label: &str, times: &mut [Duration]) {
    if times.is_empty() {
        return;
    }
    times.sort();
    println!(
        "{label}: n={} p50={:.2?} p95={:.2?} max={:.2?}",
        times.len(),
        times[times.len() / 2],
        times[(times.len() * 95 / 100).min(times.len() - 1)],
        times[times.len() - 1]
    );
}
