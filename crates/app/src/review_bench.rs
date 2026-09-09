//! Opt-in release measurements of production presentation and path helpers.
//! No GPUI window, application preferences, repository writes, or network work
//! occur inside timed regions. The runner supplies a disposable seed repository.
use crate::{
    path_filter,
    split_diff::SplitPresentation,
    text::PatchPresentation,
    text_review::{self, Options},
    worker::Content,
};
use gitturtle_core::{ChangeStatus, FileChange, GitRepository, RepositoryStatus};
use serde_json::{Value, json};
use std::{
    hint::black_box,
    path::PathBuf,
    sync::{Arc, atomic::AtomicU64},
    time::Instant,
};

const WARMUPS: usize = 3;
const SAMPLES: usize = 40;
const TEXT_LINES: usize = 20_000;
const PATHS: usize = 50_000;

fn fingerprint(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for &byte in bytes {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn sample<T>(name: &str, mut run: impl FnMut() -> T, describe: impl Fn(&T) -> Value) -> Value {
    let mut attempts = Vec::new();
    let mut expected = None;
    for attempt in 0..WARMUPS + SAMPLES {
        let started = Instant::now();
        let result = black_box(run());
        let milliseconds = started.elapsed().as_secs_f64() * 1000.;
        let description = describe(&result);
        if let Some(expected) = &expected {
            assert_eq!(&description, expected, "unstable result in {name}");
        } else {
            expected = Some(description);
        }
        attempts.push(json!({"warmup": attempt < WARMUPS, "milliseconds": milliseconds}));
        drop(result);
    }
    json!({"name": name, "attempts": attempts, "result": expected})
}

fn text_fixture() -> (String, String, String) {
    let mut old = Vec::new();
    let mut new = Vec::new();
    let mut changed = Vec::new();
    for i in 0..TEXT_LINES {
        let ending = if i + 1 == TEXT_LINES {
            ""
        } else if i % 3 == 0 {
            "\r\n"
        } else {
            "\n"
        };
        let before =
            format!("let valeur_{i:05} = lookup(\"École/🐢\", {i}); // revision source{ending}");
        let after = if i % 100 == 40 || i + 1 == TEXT_LINES {
            changed.push(i);
            before
                .replace("lookup", "resolve")
                .replace("revision source", "reviewed résultat")
        } else if i % 100 == 41 {
            changed.push(i);
            before.replace(" = ", "=").replace(", ", ",\t")
        } else {
            before.clone()
        };
        old.push(before);
        new.push(after);
    }
    let mut groups: Vec<std::ops::Range<usize>> = Vec::new();
    for changed in changed {
        let span = changed.saturating_sub(3)..(changed + 4).min(TEXT_LINES);
        if let Some(last) = groups.last_mut()
            && last.end >= span.start
        {
            last.end = last.end.max(span.end);
        } else {
            groups.push(span);
        }
    }
    let mut patch =
        String::from("diff --git a/École.rs b/École.rs\n--- a/École.rs\n+++ b/École.rs\n");
    let push = |patch: &mut String, prefix: char, line: &str| {
        patch.push(prefix);
        patch.push_str(line);
        if !line.ends_with('\n') {
            patch.push_str("\n\\ No newline at end of file\n");
        }
    };
    for group in groups {
        patch.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            group.start + 1,
            group.len(),
            group.start + 1,
            group.len()
        ));
        let mut i = group.start;
        while i < group.end {
            if old[i] == new[i] {
                push(&mut patch, ' ', &old[i]);
                i += 1;
            } else {
                let start = i;
                while i < group.end && old[i] != new[i] {
                    i += 1;
                }
                for line in &old[start..i] {
                    push(&mut patch, '-', line);
                }
                for line in &new[start..i] {
                    push(&mut patch, '+', line);
                }
            }
        }
    }
    (old.concat(), new.concat(), patch)
}

fn path_fixture(seed: &RepositoryStatus) -> (Vec<FileChange>, RepositoryStatus) {
    let mut status = seed.clone();
    let seed = status
        .entries
        .first()
        .expect("runner supplies one untracked seed file")
        .clone();
    status.entries.clear();
    let mut files = Vec::with_capacity(PATHS);
    for i in 0..PATHS {
        let path = PathBuf::from(format!(
            "src/module_{:03}/École_{:02}/review_🐢_{i:05}.rs",
            i % 100,
            i % 23
        ));
        let old = (i % 7 == 0)
            .then(|| PathBuf::from(format!("legacy/module_{:03}/ancien_{i:05}.rs", i % 100)));
        let mut entry = seed.clone();
        entry.path = path.clone();
        entry.original_path = old.clone();
        entry.staged = (i % 4 == 0 || old.is_some()).then_some(if old.is_some() {
            ChangeStatus::Renamed
        } else {
            ChangeStatus::Modified
        });
        entry.unstaged = (i % 3 != 0).then_some(ChangeStatus::Modified);
        entry.untracked = entry.staged.is_none() && entry.unstaged.is_none();
        entry.conflicted = i % 997 == 0;
        status.entries.push(entry);
        files.push(FileChange {
            old_oid: Some("1".repeat(40)),
            new_oid: Some("2".repeat(40)),
            old_mode: "100644".into(),
            new_mode: "100644".into(),
            status: if old.is_some() {
                ChangeStatus::Renamed
            } else {
                ChangeStatus::Modified
            },
            old_path: old.or_else(|| Some(path.clone())),
            new_path: Some(path),
        });
    }
    (files, status)
}

fn content_dimensions(content: &Arc<Content>) -> Value {
    let Content::Text {
        patch,
        old,
        new,
        presentation,
        split,
        ..
    } = content.as_ref()
    else {
        panic!("text result required")
    };
    json!({"patch_bytes": patch.len(), "patch_fnv1a64": fingerprint(patch.as_bytes()), "old_bytes": old.len(), "new_bytes": new.len(), "changes": presentation.change_rows.len(), "patch_metadata_bytes": presentation.retained_bytes(), "split_retained_bytes": split.retained_bytes()})
}
fn index_dimensions(index: &path_filter::Index) -> Value {
    json!({"paths": index.len(), "normalized_bytes": index.iter().flat_map(|entry| entry.iter()).map(String::len).sum::<usize>()})
}
fn match_dimensions(matches: &Arc<[usize]>) -> Value {
    json!({"count": matches.len(), "first": matches.first(), "last": matches.last(), "index_sum": matches.iter().sum::<usize>()})
}
fn rss_kib() -> Option<u64> {
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

#[test]
#[ignore = "Opt-in release CPU benchmark: run scripts/bench-app-review.py for fixture, source identity, raw samples and report"]
fn prepared_review_and_cached_paths_release_benchmark() {
    assert!(
        !black_box(cfg!(debug_assertions)),
        "performance evidence requires --release"
    );
    let output = std::env::var_os("GITTURTLE_REVIEW_BENCH_OUTPUT")
        .expect("runner supplies result destination");
    let fixture = std::env::var_os("GITTURTLE_REVIEW_BENCH_FIXTURE")
        .expect("runner supplies disposable seed repository");
    let rss_start = rss_kib();
    let (old, new, patch) = text_fixture();
    let presentation = Arc::new(PatchPresentation::prepare(&patch));
    assert_eq!(presentation.change_rows.len(), 201);
    let split = Arc::new(SplitPresentation::prepare(&old, &new, &presentation));
    let original = Arc::new(Content::Text {
        diagrams: None,
        patch: patch.clone(),
        old: old.clone(),
        new: new.clone(),
        presentation: Arc::clone(&presentation),
        split,
        partial: None,
        partial_unavailable: None,
    });
    let seed = GitRepository::open(fixture).unwrap().status().unwrap();
    let (files, status) = path_fixture(&seed);
    let cancellation = AtomicU64::new(1);
    let history_index = path_filter::prepare_paths(
        files
            .iter()
            .map(|file| [file.old_path.as_deref(), file.new_path.as_deref()]),
        &cancellation,
        1,
    )
    .unwrap();
    let working_index = path_filter::prepare_paths(
        status
            .entries
            .iter()
            .map(|entry| [Some(entry.path.as_path()), entry.original_path.as_deref()]),
        &cancellation,
        1,
    )
    .unwrap();
    let rss_fixture = rss_kib();
    let mut series = Vec::new();
    series.push(sample("patch_intraline_201_hunks", || PatchPresentation::prepare(black_box(&patch)), |p| json!({"rows": p.rows.len(), "change_blocks": p.change_rows.len(), "retained_bytes": p.retained_bytes()})));
    series.push(sample(
        "split_intraline_20k_lines",
        || SplitPresentation::prepare(black_box(&old), black_box(&new), &presentation),
        |p| json!({"retained_bytes": p.retained_bytes()}),
    ));
    let long_line_patch = format!(
        "@@ -1 +1 @@\n-{}old\n+{}new\n",
        "x".repeat(24 * 1024),
        "x".repeat(24 * 1024)
    );
    series.push(sample(
        "intraline_24k_byte_line_fallback",
        || PatchPresentation::prepare(black_box(&long_line_patch)),
        |p| json!({"rows": p.rows.len(), "retained_bytes": p.retained_bytes()}),
    ));
    for (name, options) in [
        (
            "review_hide_whitespace_context3",
            Options {
                hide_whitespace: true,
                context: 3,
            },
        ),
        (
            "review_expand_context48",
            Options {
                hide_whitespace: false,
                context: 48,
            },
        ),
        (
            "review_expand_context192",
            Options {
                hide_whitespace: false,
                context: 192,
            },
        ),
        (
            "review_hide_whitespace_context48",
            Options {
                hide_whitespace: true,
                context: 48,
            },
        ),
    ] {
        series.push(sample(
            name,
            || text_review::prepare(black_box(&original), options, || Ok(())).unwrap(),
            content_dimensions,
        ));
    }
    series.push(sample(
        "history_prepare_index_50k",
        || {
            path_filter::prepare_paths(
                files
                    .iter()
                    .map(|file| [file.old_path.as_deref(), file.new_path.as_deref()]),
                &cancellation,
                1,
            )
            .unwrap()
        },
        index_dimensions,
    ));
    series.push(sample(
        "working_prepare_index_50k",
        || {
            path_filter::prepare_paths(
                status
                    .entries
                    .iter()
                    .map(|entry| [Some(entry.path.as_path()), entry.original_path.as_deref()]),
                &cancellation,
                1,
            )
            .unwrap()
        },
        index_dimensions,
    ));
    for (name, query) in [
        ("history_cached_selective_50k", "module_017/"),
        ("history_cached_all_50k", ".rs"),
        ("history_cached_absent_50k", "missing_review_path"),
        ("history_cached_renamed_50k", "ancien_"),
    ] {
        series.push(sample(
            name,
            || path_filter::matching(black_box(&history_index), query, &cancellation, 1).unwrap(),
            match_dimensions,
        ));
    }
    for (name, query, grouped) in [
        ("working_cached_flat_selective_50k", "module_017/", false),
        ("working_cached_grouped_all_50k", ".rs", true),
        ("working_cached_grouped_selective_50k", "module_017/", true),
        (
            "working_cached_grouped_absent_50k",
            "missing_review_path",
            true,
        ),
    ] {
        series.push(sample(name, || {
            let matched = path_filter::matching(black_box(&working_index), query, &cancellation, 1).unwrap();
            let rows = super::rows(&status, &matched, grouped, &cancellation, 1).unwrap();
            let staged = status.entries.iter().filter(|entry| entry.staged.is_some()).count();
            let conflicted = status.entries.iter().any(|entry| entry.conflicted);
            (matched, rows, staged, conflicted)
        }, |(matched, rows, staged, conflicted)| json!({"matched": match_dimensions(matched), "rows": rows.len(), "staged": staged, "conflicted": conflicted})));
    }
    let rss_end = rss_kib();
    let result = json!({
        "schema": 1, "warmups_per_series": WARMUPS, "samples_per_series": SAMPLES,
        "boundary": "Production in-memory preparation/matching/row helpers, including result allocation; excludes setup, fingerprints, result destruction, serial-queue scheduling, UI callbacks, editors, rendering and Git reads.",
        "fixture": {"source_lines_per_side": TEXT_LINES, "old_bytes": old.len(), "new_bytes": new.len(), "patch_bytes": patch.len(), "old_fnv1a64": fingerprint(old.as_bytes()), "new_fnv1a64": fingerprint(new.as_bytes()), "patch_fnv1a64": fingerprint(patch.as_bytes()), "original": content_dimensions(&original), "history_index": index_dimensions(&history_index), "working_index": index_dimensions(&working_index), "description": "Deterministic synthetic 201-hunk Unicode text, mixed LF/CRLF, no final newline, 200 whitespace-only replacements; 50,000 synthetic path records cloned from a real untracked seed status with mixed staged/unstaged/untracked/conflict flags and 7,143 renamed paths."},
        "cache": "Inputs and cached indexes allocated before timing; three warmups per series; fresh output each sample. No filesystem cache flush or application preview cache.",
        "process_rss_kib": {"before_fixture": rss_start, "after_fixture": rss_fixture, "after_all_samples": rss_end, "meaning": "Point-in-time ps RSS for this release test process; allocator-retained memory and loaded libraries included; not a product or GPU memory cap."},
        "series": series
    });
    std::fs::write(output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
}
