//! Opt-in release benchmark of Changes' working read, [`WorkingState::read`],
//! as Changes' Refresh and a write's reload run it, and of a write through
//! [`GitRepository::execute`]: staging one modified file and unstaging it.
//! Both run Git at the repository's path, so a check that the repository is
//! still the one found there costs each of them a process.
//!
//! Build with `cargo test --locked --release -p gitturtle --bin gitturtle
//! --no-run`, then run the executable with `--ignored --exact
//! worker::working_bench::working_read_and_stage_release_benchmark` and
//! `GITTURTLE_WORKING_BENCH_OUT` naming the JSON file it writes
//! (`GITTURTLE_WORKING_BENCH_SAMPLES`, default 60). It builds its own
//! fixture: 300 committed files of 40 lines, 30 of them modified.

use super::*;
use gitturtle_core::WriteCommand;
use std::fmt::Write as _;

#[test]
#[ignore = "Opt-in release benchmark: set GITTURTLE_WORKING_BENCH_OUT and run with --release --ignored"]
fn working_read_and_stage_release_benchmark() {
    let output = PathBuf::from(
        std::env::var("GITTURTLE_WORKING_BENCH_OUT")
            .expect("GITTURTLE_WORKING_BENCH_OUT is not set"),
    );
    let samples: usize = std::env::var("GITTURTLE_WORKING_BENCH_SAMPLES")
        .map_or(60, |value| value.parse().expect("a sample count"));
    let fixture = tempfile::tempdir_in("/tmp").expect("a fixture directory");
    let root = fixture.path().join("bench");
    let git = |args: &[&str]| {
        let result = std::process::Command::new("git")
            .args([
                "-c",
                "init.templateDir=",
                "-c",
                "user.name=Bench",
                "-c",
                "user.email=bench@example.invalid",
            ])
            .args(args)
            .current_dir(&root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git runs");
        assert!(
            result.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    };
    std::fs::create_dir(&root).expect("the fixture root");
    git(&["init", "--initial-branch=main"]);
    let body = |file: usize, edited: bool| {
        (0..40).fold(String::new(), |mut text, line| {
            let _ = writeln!(
                text,
                "file {file} line {line}{}",
                if edited && line % 7 == 0 {
                    " edited"
                } else {
                    ""
                }
            );
            text
        })
    };
    for file in 0..300 {
        let directory = root.join(format!("dir{}", file % 12));
        std::fs::create_dir_all(&directory).expect("a directory");
        std::fs::write(directory.join(format!("file{file}.txt")), body(file, false))
            .expect("a file");
    }
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "fixture"]);
    for file in (0..300).step_by(10) {
        let path = root
            .join(format!("dir{}", file % 12))
            .join(format!("file{file}.txt"));
        std::fs::write(path, body(file, true)).expect("an edit");
    }
    let repo = GitRepository::open(&root).expect("the fixture opens");
    let staged = vec![PathBuf::from("dir0/file0.txt")];
    let time = |f: &mut dyn FnMut()| {
        let start = Instant::now();
        f();
        start.elapsed().as_secs_f64() * 1000.
    };
    let round = |read: &mut Vec<f64>, stage: &mut Vec<f64>, unstage: &mut Vec<f64>| {
        read.push(time(&mut || {
            let state = WorkingState::read(&repo).expect("the working read");
            assert_eq!(
                state.status.entries.len(),
                30,
                "the fixture's modified files"
            );
        }));
        stage.push(time(&mut || {
            repo.execute(&WriteCommand::Stage {
                paths: staged.clone(),
            })
            .expect("the stage");
        }));
        unstage.push(time(&mut || {
            repo.execute(&WriteCommand::Unstage {
                paths: staged.clone(),
            })
            .expect("the unstage");
        }));
    };
    let (mut read, mut stage, mut unstage) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..5 {
        round(&mut read, &mut stage, &mut unstage);
    }
    let (mut read, mut stage, mut unstage) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..samples {
        round(&mut read, &mut stage, &mut unstage);
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
            "{{\"read_ms\":[{}],\"stage_ms\":[{}],\"unstage_ms\":[{}]}}\n",
            list(&read),
            list(&stage),
            list(&unstage)
        ),
    )
    .expect("write samples");
}
