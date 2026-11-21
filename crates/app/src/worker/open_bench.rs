//! Opt-in release measurement of opening a repository end to end, as the
//! window's reads do it: `open` is [`Job::Open`] submitted to a [`Worker`]
//! and awaited, the snapshot History shows, and `working` is the
//! [`WorkingState::read`] of the repository that snapshot names, which
//! Changes asks for once the snapshot lands. Each sample has a new worker,
//! as a launch's window has, since a session that holds the repository
//! already answers an Open from what it kept; the reader answers one
//! request before the clock starts, so its thread's start is not timed.
//!
//! The fixture is built in a temporary directory: 200 commits on `main`,
//! `feature` (merged into `main`) and `topic`, a lightweight and an
//! annotated tag, the remote `origin` with `origin/main` (written as a
//! fetch leaves them, with nothing transferred), and twenty
//! tracked files of which two are modified beside one untracked file. After
//! 5 untimed rounds, 60 rounds each write two lines, `open,nanoseconds` and
//! `working,nanoseconds`, to the file `GITTURTLE_OPEN_BENCH_OUT` names; with
//! the variable unset the test does nothing. It asserts what the two reads
//! returned and never a time.
//!
//! Build with `cargo test --locked --release -p gitturtle --bin gitturtle
//! --no-run`, then run the executable Cargo names:
//!
//! ```text
//! GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
//!   GITTURTLE_OPEN_BENCH_OUT=/absolute/path/open.csv \
//!   target/release/deps/gitturtle-<hash> --ignored --exact \
//!   worker::open_bench::open_and_working_read_release_benchmark --nocapture
//! ```
//!
//! The two configuration variables are the gate's: the working read takes
//! the caller's normal Git configuration, as the product means it to, and
//! the measurement should not. `--nocapture` shows each kind's median.
//!
//! One sample starts 19 `git` processes, the same in each of the 65 rounds,
//! counted on a debug run with a wrapper named `git` first on `PATH` that
//! logs its arguments and then runs Git in its place. The open starts ten,
//! all passive: three `rev-parse` of `GitRepository::open`, two for the
//! session's identity, the branches' `for-each-ref`, `worktree list`, the
//! tags' `for-each-ref`, and the page's `rev-parse` and `log`. The working
//! read starts nine: `config`, `status` and `rev-parse` for the status,
//! `config` and `var` for the profile, `remote` and two `remote get-url`
//! for the one remote, and `rev-parse` for the operation state. The
//! profile's two and the remotes' three are write-policy reads; with no
//! remote the working read starts seven.
//!
//! A window starts more than this to open a repository. Left out are what
//! it asks besides: the changed files of the commit History selects
//! (`Job::Changes`), the local watcher's Git directories
//! (`check_local_watcher`), and its further reads of the working state.

use super::*;
use futures::executor::block_on;
use gitturtle_core::ChangeStatus;
use std::fmt::Write as _;
use std::io::Write as _;

const OUTPUT: &str = "GITTURTLE_OPEN_BENCH_OUT";
const WARM_UPS: usize = 5;
const SAMPLES: usize = 60;
const COMMITS: usize = 200;
const FILES: usize = 20;
/// 2026-01-01T00:00:00Z; each commit is a minute after the one before.
const EPOCH: usize = 1_767_225_600;
const IDENTITY: &str = "Bench <bench@example.invalid>";
/// The remote's address, which nothing here contacts.
const REMOTE: &str = "https://example.invalid/bench.git";

/// Git for the fixture, never the caller's: an empty system and global
/// configuration, a fixed identity and date, and no inherited `GIT_*`
/// variable, any of which could aim it at another repository or configure
/// it. It starts no maintenance, which would run on behind the samples.
/// `input` is its standard input.
fn git(directory: &Path, args: &[&str], input: &[u8]) -> String {
    let mut command = std::process::Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    let mut child = command
        .arg("-C")
        .arg(directory)
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
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("git runs");
    child
        .stdin
        .take()
        .expect("Git's input")
        .write_all(input)
        .expect("Git reads its input");
    let output = child.wait_with_output().expect("git ends");
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

fn file_path(file: usize) -> String {
    format!("src/file{file:02}.txt")
}

/// What `file` holds as the commit marked `mark` wrote it.
fn file_text(file: usize, mark: usize) -> String {
    (0..12).fold(String::new(), |mut text, line| {
        let _ = writeln!(text, "file {file:02} change {mark} line {line}");
        text
    })
}

/// The fixture's history as a `git fast-import` stream, so one process
/// writes it whole and every commit's name is the same on each run.
#[derive(Default)]
struct History {
    stream: String,
    commits: usize,
}

impl History {
    /// One commit on `branch` after `parents`, the first of which its tree
    /// starts from. It writes each of `files` anew and each of `carried`,
    /// a file with the mark of the commit whose text it takes, as a merge
    /// takes its second parent's. Returns the commit's mark.
    fn commit(
        &mut self,
        branch: &str,
        parents: &[usize],
        files: &[usize],
        carried: &[(usize, usize)],
    ) -> usize {
        self.commits += 1;
        let mark = self.commits;
        let message = format!("{branch}: change {mark}");
        let _ = write!(
            self.stream,
            "commit refs/heads/{branch}\nmark :{mark}\ncommitter {IDENTITY} {} +0000\ndata {}\n{message}\n",
            EPOCH + 60 * mark,
            message.len()
        );
        for (at, parent) in parents.iter().enumerate() {
            let _ = writeln!(
                self.stream,
                "{} :{parent}",
                if at == 0 { "from" } else { "merge" }
            );
        }
        let own = files.iter().map(|file| (*file, mark));
        for (file, written) in own.chain(carried.iter().copied()) {
            let text = file_text(file, written);
            let _ = write!(
                self.stream,
                "M 100644 inline {}\ndata {}\n{text}",
                file_path(file),
                text.len()
            );
        }
        self.stream.push('\n');
        mark
    }

    /// 200 commits: 80 on `main`, the first of which adds every file; 40
    /// on `feature` from the 40th, which the lightweight tag `v0.1` names;
    /// the merge of `feature` into `main`, which the annotated tag `v0.2`
    /// names; 39 more on `main`; and 40 on `topic` from the 20th of those.
    /// Each branch changes files of its own.
    fn fixture() -> Self {
        let mut history = Self::default();
        let every_file: Vec<usize> = (0..FILES).collect();
        let mut main = history.commit("main", &[], &every_file, &[]);
        let mut tagged = main;
        for change in 1..80 {
            main = history.commit("main", &[main], &[change % 10], &[]);
            if change == 39 {
                tagged = main;
            }
        }
        let mut feature = tagged;
        let mut merged = [(0, 0); 5];
        for change in 0..40 {
            let file = 10 + change % 5;
            feature = history.commit("feature", &[feature], &[file], &[]);
            merged[change % 5] = (file, feature);
        }
        let merge = history.commit("main", &[main, feature], &[], &merged);
        main = merge;
        let mut topic = merge;
        for change in 0..39 {
            main = history.commit("main", &[main], &[change % 10], &[]);
            if change == 19 {
                topic = main;
            }
        }
        for change in 0..40 {
            topic = history.commit("topic", &[topic], &[15 + change % 5], &[]);
        }
        let _ = write!(
            history.stream,
            "reset refs/tags/v0.1\nfrom :{tagged}\n\ntag v0.2\nfrom :{merge}\ntagger {IDENTITY} {} +0000\ndata 11\nversion 0.2\n",
            EPOCH + 60 * merge
        );
        history
    }
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
#[ignore = "Opt-in release benchmark: set GITTURTLE_OPEN_BENCH_OUT and run with --release --ignored"]
fn open_and_working_read_release_benchmark() {
    let Some(output) = std::env::var_os(OUTPUT).filter(|path| !path.is_empty()) else {
        eprintln!("{OUTPUT} names no output file: nothing is measured");
        return;
    };
    let output = PathBuf::from(output);
    let fixture = tempfile::tempdir().expect("a fixture directory");
    let root = fixture.path().join("bench");
    std::fs::create_dir(&root).expect("the fixture root");
    git(&root, &["init", "--quiet", "--initial-branch=main"], b"");
    let history = History::fixture();
    assert_eq!(history.commits, COMMITS, "the fixture's commits");
    git(
        &root,
        &["fast-import", "--quiet"],
        history.stream.as_bytes(),
    );
    git(&root, &["reset", "--quiet", "--hard"], b"");
    // The remote as a fetch leaves it, written without one: a transfer
    // would start a maintenance that runs on behind the samples.
    git(&root, &["remote", "add", "origin", REMOTE], b"");
    let tracking = ["update-ref", "refs/remotes/origin/main", "refs/heads/main"];
    git(&root, &tracking, b"");
    let upstream = ["branch", "--quiet", "--set-upstream-to=origin/main", "main"];
    git(&root, &upstream, b"");
    for file in 0..2 {
        let path = root.join(file_path(file));
        let mut text = std::fs::read_to_string(&path).expect("a tracked file");
        text.push_str("edited\n");
        std::fs::write(path, text).expect("an edit");
    }
    std::fs::write(root.join("notes.txt"), "untracked\n").expect("an untracked file");
    let head = git(&root, &["rev-parse", "HEAD"], b"");
    let tags = ["v0.1", "v0.2"].map(|tag| {
        let commit = git(&root, &["rev-parse", &format!("{tag}^{{commit}}")], b"");
        (tag, commit)
    });

    // Only the two reads are timed; what they returned is checked after.
    let sample = || {
        let worker = Worker::new();
        assert!(
            matches!(
                block_on(worker.submit(Job::ReleaseHistory)),
                Ok(Ok(Output::HistoryReleased))
            ),
            "the reader answers before the clock starts"
        );
        let start = Instant::now();
        let reply = block_on(worker.submit(Job::Open {
            path: root.clone(),
            scope: None,
            limit: crate::history_paging::PAGE_SIZE,
            root: None,
            git_dir: None,
        }));
        let open = start.elapsed();
        let Output::Snapshot(snapshot) = reply.expect("the reader answers").expect("the open")
        else {
            panic!("an Open answers with its snapshot");
        };
        let start = Instant::now();
        let state = WorkingState::read(&snapshot.repository);
        let working = start.elapsed();

        assert_eq!(snapshot.commits.len(), COMMITS, "every commit, one page");
        let merges = snapshot
            .commits
            .iter()
            .filter(|commit| commit.parents.len() == 2);
        assert_eq!(merges.count(), 1, "the merge");
        let mut branches: Vec<_> = snapshot
            .branches
            .iter()
            .map(|branch| (branch.name.as_str(), branch.remote, branch.current))
            .collect();
        branches.sort_unstable();
        assert_eq!(
            branches,
            [
                ("feature", false, false),
                ("main", false, true),
                ("origin/main", true, false),
                ("topic", false, false),
            ]
        );
        let current = snapshot.branches.iter().find(|branch| branch.current);
        assert_eq!(current.map(|branch| &branch.oid), Some(&head), "HEAD");
        assert_eq!(snapshot.worktrees.len(), 1, "the one work tree");
        for (tag, commit) in &tags {
            let names = snapshot.refs.get(commit).map(|refs| &refs.names);
            assert!(
                names.is_some_and(|names| names.iter().any(|name| &**name == *tag)),
                "the tag {tag} on its commit"
            );
        }

        let state = state.expect("the working read");
        let entries = &state.status.entries;
        let modified = |entry: &&gitturtle_core::StatusEntry| {
            !entry.untracked && entry.unstaged == Some(ChangeStatus::Modified)
        };
        assert_eq!(entries.len(), 3, "two modified files and one untracked");
        assert_eq!(entries.iter().filter(modified).count(), 2, "modified");
        assert_eq!(entries.iter().filter(|e| e.untracked).count(), 1);
        assert_eq!(state.status.branch.as_deref(), Some("main"));
        let remotes = state.remotes.expect("the remotes");
        let remotes: Vec<_> = remotes
            .iter()
            .map(|remote| (remote.name.as_str(), remote.url.as_str()))
            .collect();
        assert_eq!(remotes, [("origin", REMOTE)], "the one remote");
        assert!(matches!(state.operation, Ok(None)), "no operation");
        // The identity is the caller's by design, so only that it was read.
        assert!(state.profile.is_ok(), "the profile");

        // Untimed: the reader drops the history's idle process before the
        // worker goes, so nothing of this round ends inside the next.
        drop(block_on(worker.submit(Job::ReleaseHistory)));
        (open, working)
    };
    for _ in 0..WARM_UPS {
        sample();
    }
    let mut samples = Vec::with_capacity(2 * SAMPLES);
    for _ in 0..SAMPLES {
        let (open, working) = sample();
        samples.push(("open", open));
        samples.push(("working", working));
    }
    let lines = samples
        .iter()
        .fold(String::new(), |mut lines, (kind, elapsed)| {
            let _ = writeln!(lines, "{kind},{}", elapsed.as_nanos());
            lines
        });
    std::fs::write(&output, lines).expect("write samples");
    for kind in ["open", "working"] {
        let (count, median) = median(&samples, kind);
        eprintln!("{kind}: {count} samples, median {median} ns");
    }
}
