//! Opt-in release measurement of what one Git read costs from its call to
//! its return, one read of each command policy, through the public API. Each
//! sampled call starts exactly one `git` process, so a sample is that
//! process and the wait for it, with nothing else in it:
//!
//! - `passive`: [`GitRepository::branches`], one `git for-each-ref` through
//!   `run_git` and so `bounded_output`, with the 15 s read deadline.
//!   History's snapshot asks it when a repository opens.
//! - `write-policy`: [`GitRepository::remotes`] on a repository with no
//!   remote, one `git remote` through `checked_write_output` and so
//!   `bounded_write_output`, with the same 15 s deadline. The working read
//!   asks it when a repository opens. It asks twice more for each remote it
//!   finds, which is why the fixture has none.
//!
//! The fixture is one commit of one file on `main`, built in a temporary
//! directory. After 20 untimed rounds, 500 rounds take one sample of each
//! kind in turn, so a drift of the host falls on both. The samples go to the
//! file `GITTURTLE_READ_COST_BENCH_OUT` names, one line a sample in the order
//! taken, `kind,nanoseconds`; with the variable unset the test does nothing.
//! It asserts what each read returned and never a time.
//!
//! Build with `cargo test --locked --release -p gitturtle-core --test
//! read_cost_bench --no-run`, then run the executable Cargo names:
//!
//! ```text
//! GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
//!   GITTURTLE_READ_COST_BENCH_OUT=/absolute/path/read-cost.csv \
//!   target/release/deps/read_cost_bench-<hash> \
//!   --ignored --exact one_read_of_each_policy_release_benchmark --nocapture
//! ```
//!
//! The two configuration variables are the gate's: a write-policy read takes
//! the caller's normal Git configuration, as the product means it to, and
//! the measurement should not. `--nocapture` shows each kind's median.
//!
//! The process counts were taken on a debug run with a wrapper named `git`
//! first on `PATH` that logs its arguments and then runs Git in its place:
//! after the fixture's own commands and the three of the open, the log is
//! 520 times one `for-each-ref` followed by one `remote`.

use gitturtle_core::{Branch, GitRepository};
use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

const OUTPUT: &str = "GITTURTLE_READ_COST_BENCH_OUT";
const WARM_UPS: usize = 20;
const SAMPLES: usize = 500;
const PASSIVE: &str = "passive";
const WRITE_POLICY: &str = "write-policy";

/// Git for the fixture, never the caller's: an empty system and global
/// configuration, a fixed identity and date, and no inherited `GIT_*`
/// variable, any of which could aim it at another repository or configure
/// it. It starts no maintenance, which would run on behind the samples.
fn git(root: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    let output = command
        .arg("-C")
        .arg(root)
        .args(["-c", "init.templateDir=", "-c", "gc.auto=0"])
        .args(["-c", "maintenance.auto=false"])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Bench")
        .env("GIT_AUTHOR_EMAIL", "bench@example.invalid")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
        .env("GIT_COMMITTER_NAME", "Bench")
        .env("GIT_COMMITTER_EMAIL", "bench@example.invalid")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("Git's answer is UTF-8")
        .trim_end()
        .to_owned()
}

/// The median of `kind`'s samples, in nanoseconds, with how many there are.
fn median(samples: &[(&str, Duration)], kind: &str) -> (usize, u128) {
    let mut nanoseconds: Vec<u128> = samples
        .iter()
        .filter(|(sampled, _)| *sampled == kind)
        .map(|(_, elapsed)| elapsed.as_nanos())
        .collect();
    nanoseconds.sort_unstable();
    let middle = nanoseconds.len() / 2;
    let median = if nanoseconds.len().is_multiple_of(2) {
        (nanoseconds[middle - 1] + nanoseconds[middle]) / 2
    } else {
        nanoseconds[middle]
    };
    (nanoseconds.len(), median)
}

#[test]
#[ignore = "Opt-in release benchmark: set GITTURTLE_READ_COST_BENCH_OUT and run with --release --ignored"]
fn one_read_of_each_policy_release_benchmark() {
    let Some(output) = std::env::var_os(OUTPUT).filter(|path| !path.is_empty()) else {
        eprintln!("{OUTPUT} names no output file: nothing is measured");
        return;
    };
    let output = PathBuf::from(output);
    let fixture = tempfile::tempdir().expect("a fixture directory");
    let root = fixture.path().join("bench");
    std::fs::create_dir(&root).expect("the fixture root");
    git(&root, &["init", "--quiet", "--initial-branch=main"]);
    std::fs::write(root.join("file.txt"), "one line\n").expect("a file");
    git(&root, &["add", "--", "file.txt"]);
    git(&root, &["commit", "--quiet", "--message=fixture"]);
    let expected = [Branch {
        name: "main".into(),
        oid: git(&root, &["rev-parse", "HEAD"]),
        remote: false,
        current: true,
    }];
    let repo = GitRepository::open(&root).expect("the fixture opens");

    // Only the call is timed; what it returned is checked after the clock.
    let passive = || {
        let start = Instant::now();
        let branches = repo.branches();
        let elapsed = start.elapsed();
        assert_eq!(
            branches.expect("the passive read"),
            expected,
            "the fixture's one branch"
        );
        elapsed
    };
    let write_policy = || {
        let start = Instant::now();
        let remotes = repo.remotes();
        let elapsed = start.elapsed();
        let remotes = remotes.expect("the write-policy read");
        assert!(remotes.is_empty(), "the fixture has no remote: {remotes:?}");
        elapsed
    };
    for _ in 0..WARM_UPS {
        passive();
        write_policy();
    }
    let mut samples = Vec::with_capacity(2 * SAMPLES);
    for _ in 0..SAMPLES {
        samples.push((PASSIVE, passive()));
        samples.push((WRITE_POLICY, write_policy()));
    }
    let lines = samples
        .iter()
        .fold(String::new(), |mut lines, (kind, elapsed)| {
            let _ = writeln!(lines, "{kind},{}", elapsed.as_nanos());
            lines
        });
    std::fs::write(&output, lines).expect("write samples");
    for kind in [PASSIVE, WRITE_POLICY] {
        let (count, median) = median(&samples, kind);
        eprintln!("{kind}: {count} samples, median {median} ns");
    }
}
