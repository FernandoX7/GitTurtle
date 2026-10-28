//! Opt-in release benchmark of History's snapshot read, `read_snapshot`, as a
//! launch or a new tab runs it (a new session) and as an explicit Refresh runs
//! it (the window's session again). The record and its procedure are in
//! `docs/benchmarks/2026-10-28-history-snapshot-tags.md`.

use super::*;
use std::fmt::Write as _;

#[test]
#[ignore = "Opt-in release benchmark: see docs/benchmarks/2026-10-28-history-snapshot-tags.md"]
fn read_snapshot_release_benchmark() {
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set"));
    let repository = PathBuf::from(var("GITTURTLE_SNAPSHOT_BENCH_REPO"));
    let output = PathBuf::from(var("GITTURTLE_SNAPSHOT_BENCH_OUT"));
    let samples: usize = std::env::var("GITTURTLE_SNAPSHOT_BENCH_SAMPLES")
        .map_or(40, |value| value.parse().expect("a sample count"));
    let cancellation = Cancellation {
        generation: 1,
        latest: Arc::new(AtomicU64::new(1)),
        history: gitturtle_core::HistoryCancellation::default(),
    };
    let read = |session: &mut RepositorySession| {
        let start = Instant::now();
        let snapshot = read_snapshot(
            repository.clone(),
            None,
            MissingScope::Fail,
            crate::history_paging::PAGE_SIZE,
            &cancellation,
            session,
        )
        .expect("snapshot");
        let elapsed = start.elapsed().as_secs_f64() * 1000.;
        // Freeing the snapshot happens after the UI accepts it; not timed.
        (elapsed, snapshot.commits.len(), snapshot.branches.len())
    };
    let mut refresh_session = RepositorySession::default();
    for _ in 0..5 {
        read(&mut RepositorySession::default());
        read(&mut refresh_session);
    }
    let (mut load, mut refresh, mut shape) = (Vec::new(), Vec::new(), (0, 0));
    for _ in 0..samples {
        let (ms, commits, branches) = read(&mut RepositorySession::default());
        load.push(ms);
        shape = (commits, branches);
        refresh.push(read(&mut refresh_session).0);
    }
    let list = |values: &[f64]| {
        values.iter().fold(String::new(), |mut text, value| {
            let _ = write!(text, "{}{value:.4}", if text.is_empty() { "" } else { "," });
            text
        })
    };
    std::fs::write(
        output,
        format!(
            "{{\"commits\":{},\"branches\":{},\"load_ms\":[{}],\"refresh_ms\":[{}]}}\n",
            shape.0,
            shape.1,
            list(&load),
            list(&refresh)
        ),
    )
    .expect("write samples");
}
