//! Bounded, read-only backend measurements. See docs/benchmarks/everyday-bench.md.
use anyhow::{Context, Result, bail, ensure};
use gitturtle_core::{
    ChangeArea, GitRepository, HistoryCancellation, HistoryScope, RepositoryStatus, StatusEntry,
    TextPreview, WorktreePreview,
};
use sha2::{Digest, Sha256};
use std::{
    env,
    ffi::OsString,
    fmt::Write,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const USAGE: &str = "Usage: everyday_bench REPOSITORY [--file RELATIVE_PATH] [--query TEXT]
  [--scope all|head] [--commit FULL_OID] [--parent N] [--warmup N] [--samples N]
  [--pages N] [--page-size N] [--budget-seconds N] [--label TEXT] [--build-label TEXT]

Defaults: all local refs, empty query, HEAD/parent 0, 2 warmups, 12 samples,
2 pages of 100 entries, 180-second budget. Writes no repository files; JSON goes
to stdout. See docs/benchmarks/everyday-bench.md for measurement boundaries.";

struct Options {
    repository: PathBuf,
    file: Option<PathBuf>,
    query: String,
    all_refs: bool,
    commit: Option<String>,
    parent: usize,
    warmup: usize,
    samples: usize,
    pages: usize,
    page_size: usize,
    budget_seconds: usize,
    label: String,
    build_label: Option<String>,
}

impl Options {
    fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self> {
        let mut args = args.into_iter();
        let repository = args.next().context(USAGE)?.into();
        let mut options = Self {
            repository,
            file: None,
            query: String::new(),
            all_refs: true,
            commit: None,
            parent: 0,
            warmup: 2,
            samples: 12,
            pages: 2,
            page_size: 100,
            budget_seconds: 180,
            label: "unspecified fixture".into(),
            build_label: None,
        };
        while let Some(flag) = args.next() {
            let flag = flag.to_str().context("Options must be UTF-8")?;
            let value = args
                .next()
                .with_context(|| format!("Missing value for {flag}"))?;
            if flag == "--file" {
                let path = PathBuf::from(value);
                ensure!(
                    !path.as_os_str().is_empty()
                        && path.as_os_str().len() <= 16 * 1024
                        && path
                            .components()
                            .all(|part| matches!(part, Component::Normal(_))),
                    "--file must be a literal repository-relative file path"
                );
                options.file = Some(path);
                continue;
            }
            let value = value
                .into_string()
                .map_err(|_| anyhow::anyhow!("{flag} requires UTF-8"))?;
            match flag {
                "--query" => {
                    ensure!(value.len() <= 4096, "Query exceeds 4 KiB");
                    options.query = value;
                }
                "--scope" => {
                    options.all_refs = match value.as_str() {
                        "all" => true,
                        "head" => false,
                        _ => bail!("--scope must be all or head"),
                    }
                }
                "--commit" => {
                    ensure!(
                        [40, 64].contains(&value.len())
                            && value.bytes().all(|b| b.is_ascii_hexdigit()),
                        "--commit requires a full hexadecimal commit OID"
                    );
                    options.commit = Some(value);
                }
                "--parent" => options.parent = number(&value, 0, 31)?,
                "--warmup" => options.warmup = number(&value, 0, 10)?,
                "--samples" => options.samples = number(&value, 1, 100)?,
                "--pages" => options.pages = number(&value, 1, 4)?,
                "--page-size" => options.page_size = number(&value, 1, 500)?,
                "--budget-seconds" => options.budget_seconds = number(&value, 1, 1800)?,
                "--label" => {
                    ensure!(value.len() <= 1024, "Fixture label exceeds 1 KiB");
                    options.label = value;
                }
                "--build-label" => {
                    ensure!(value.len() <= 1024, "Build label exceeds 1 KiB");
                    options.build_label = Some(value);
                }
                _ => bail!("Unknown option {flag}\n{USAGE}"),
            }
        }
        Ok(options)
    }
}

fn number(value: &str, minimum: usize, maximum: usize) -> Result<usize> {
    let value: usize = value.parse().context("Expected an integer option value")?;
    ensure!(
        (minimum..=maximum).contains(&value),
        "Value must be between {minimum} and {maximum}"
    );
    Ok(value)
}

// The core deliberately has no JSON dependency. This small output-only value
// type escapes every string; repository paths retain their exact encoded bytes.
#[derive(Clone)]
enum Json {
    Null,
    Bool(bool),
    Int(u64),
    Float(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(&'static str, Json)>),
}
impl Json {
    fn write(&self, out: &mut String) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Self::Int(value) => {
                write!(out, "{value}").unwrap();
            }
            Self::Float(value) => {
                assert!(value.is_finite());
                write!(out, "{value:.6}").unwrap();
            }
            Self::String(value) => {
                out.push('"');
                for character in value.chars() {
                    match character {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        c if c < ' ' => {
                            write!(out, "\\u{:04x}", c as u32).unwrap();
                        }
                        c => out.push(c),
                    }
                }
                out.push('"');
            }
            Self::Array(values) => {
                out.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    value.write(out);
                }
                out.push(']');
            }
            Self::Object(values) => {
                out.push('{');
                for (index, (key, value)) in values.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    string(key).write(out);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
    fn encoded(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }
}
fn object(values: impl IntoIterator<Item = (&'static str, Json)>) -> Json {
    Json::Object(values.into_iter().collect())
}
fn string(value: impl ToString) -> Json {
    Json::String(value.to_string())
}
fn integer(value: usize) -> Json {
    Json::Int(value as u64)
}
fn optional(value: Option<&str>) -> Json {
    value.map_or(Json::Null, string)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn path_info(path: &Path) -> Json {
    object([
        ("display", string(path.display())),
        (
            "encoded_bytes_hex",
            string(
                path.as_os_str()
                    .as_encoded_bytes()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
            ),
        ),
    ])
}

struct Measurements<'a> {
    options: &'a Options,
    started: Instant,
    series: Vec<Json>,
}
impl Measurements<'_> {
    fn budget_available(&self) -> bool {
        self.started.elapsed().as_secs() < self.options.budget_seconds as u64
    }

    fn skip(&mut self, name: &'static str, reason: &str) {
        self.series.push(object([
            ("name", string(name)),
            ("skipped", string(reason)),
        ]));
    }

    /// Timing stops immediately after the API returns. Result validation,
    /// checksums, JSON preparation and dropping result buffers are excluded.
    fn measure<T>(
        &mut self,
        name: &'static str,
        mut read: impl FnMut() -> Result<T>,
        summarize: impl Fn(&T) -> Json,
    ) {
        eprintln!("Measuring {name}…");
        let mut warmup = Vec::new();
        let mut samples = Vec::new();
        let mut successful = Vec::new();
        let mut first_result = None;
        let mut result_changed = false;
        for index in 0..self.options.warmup + self.options.samples {
            if !self.budget_available() {
                break;
            }
            let started = Instant::now();
            let result = read();
            let elapsed = started.elapsed().as_secs_f64() * 1000.;
            let sample = match result {
                Ok(result) => {
                    let summary = summarize(&result);
                    let fingerprint = digest(summary.encoded().as_bytes());
                    if let Some(first) = &first_result {
                        result_changed |= first != &fingerprint;
                    } else {
                        first_result = Some(fingerprint);
                    }
                    if index >= self.options.warmup {
                        successful.push(elapsed);
                    }
                    object([
                        ("ms", Json::Float(elapsed)),
                        ("ok", Json::Bool(true)),
                        ("result", summary),
                    ])
                }
                Err(error) => object([
                    ("ms", Json::Float(elapsed)),
                    ("ok", Json::Bool(false)),
                    ("error", string(error)),
                ]),
            };
            if index < self.options.warmup {
                warmup.push(sample);
            } else {
                samples.push(sample);
            }
        }
        let completed = samples.len() == self.options.samples;
        self.series.push(object([
            ("name", string(name)),
            ("warmup", Json::Array(warmup)),
            ("samples", Json::Array(samples)),
            ("successful_stats", statistics(&successful)),
            ("completed", Json::Bool(completed)),
            ("budget_stopped", Json::Bool(!completed)),
            ("result_changed", Json::Bool(result_changed)),
        ]));
    }
}

fn statistics(samples: &[f64]) -> Json {
    if samples.is_empty() {
        return object([
            ("n", integer(0)),
            ("p50_ms", Json::Null),
            ("p95_ms", Json::Null),
            ("max_ms", Json::Null),
        ]);
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    object([
        ("n", integer(sorted.len())),
        ("p50_ms", Json::Float(percentile(&sorted, 50))),
        ("p95_ms", Json::Float(percentile(&sorted, 95))),
        ("max_ms", Json::Float(*sorted.last().unwrap())),
    ])
}
fn percentile(sorted: &[f64], percent: usize) -> f64 {
    sorted[(sorted.len() * percent).div_ceil(100).saturating_sub(1)]
}

fn status_info(status: &RepositoryStatus) -> Json {
    object([
        ("head", optional(status.head.as_deref())),
        ("entries", integer(status.entries.len())),
        (
            "staged",
            integer(
                status
                    .entries
                    .iter()
                    .filter(|entry| entry.staged.is_some())
                    .count(),
            ),
        ),
        (
            "unstaged",
            integer(
                status
                    .entries
                    .iter()
                    .filter(|entry| entry.unstaged.is_some())
                    .count(),
            ),
        ),
        (
            "untracked",
            integer(
                status
                    .entries
                    .iter()
                    .filter(|entry| entry.untracked)
                    .count(),
            ),
        ),
        (
            "conflicts",
            integer(
                status
                    .entries
                    .iter()
                    .filter(|entry| entry.conflicted)
                    .count(),
            ),
        ),
        (
            "snapshot_sha256",
            string(digest(format!("{status:?}").as_bytes())),
        ),
    ])
}

fn preview_info(preview: &WorktreePreview) -> Json {
    let kind = match &preview.preview {
        TextPreview::Patch(_) => "text",
        TextPreview::Binary => "binary",
        TextPreview::TooLarge { .. } => "too_large",
        TextPreview::Submodule { .. } => "submodule",
    };
    object([
        ("path", path_info(preview.file.path())),
        (
            "old_path",
            preview
                .file
                .old_path
                .as_deref()
                .map_or(Json::Null, path_info),
        ),
        (
            "new_path",
            preview
                .file
                .new_path
                .as_deref()
                .map_or(Json::Null, path_info),
        ),
        ("kind", string(kind)),
        ("old_oid", optional(preview.file.old_oid.as_deref())),
        ("new_oid", optional(preview.file.new_oid.as_deref())),
        ("old_mode", string(&preview.file.old_mode)),
        ("new_mode", string(&preview.file.new_mode)),
        ("old_bytes", integer(preview.old.len())),
        ("new_bytes", integer(preview.new.len())),
        ("old_sha256", string(digest(&preview.old))),
        ("new_sha256", string(digest(&preview.new))),
        (
            "partial_hunks",
            integer(preview.partial.as_ref().map_or(0, |diff| diff.hunks.len())),
        ),
        (
            "partial_changed_lines",
            integer(preview.partial.as_ref().map_or(0, |diff| {
                diff.hunks
                    .iter()
                    .flat_map(|hunk| &hunk.lines)
                    .filter(|line| line.change_id.is_some())
                    .count()
            })),
        ),
        (
            "partial_retained_bytes",
            integer(preview.partial.as_ref().map_or(0, |diff| diff.bytes())),
        ),
        (
            "partial_unavailable",
            optional(preview.partial_unavailable.as_deref()),
        ),
    ])
}

fn selected_entry<'a>(
    status: &'a RepositoryStatus,
    path: Option<&Path>,
    area: ChangeArea,
) -> Option<&'a StatusEntry> {
    status.entries.iter().find(|entry| {
        !entry.conflicted
            && path.is_none_or(|path| entry.path == path)
            && match area {
                ChangeArea::Staged => entry.staged.is_some(),
                ChangeArea::Unstaged => entry.unstaged.is_some() || entry.untracked,
            }
    })
}

struct Pages {
    pages: usize,
    entries: usize,
    scanned: Option<usize>,
    next: Option<usize>,
    stop: String,
    ids: Vec<String>,
}
fn pages_info(pages: &Pages) -> Json {
    object([
        ("pages", integer(pages.pages)),
        ("entries", integer(pages.entries)),
        ("search_scanned", pages.scanned.map_or(Json::Null, integer)),
        ("next_offset", pages.next.map_or(Json::Null, integer)),
        ("stop", string(&pages.stop)),
        (
            "result_oids_sha256",
            string(digest(pages.ids.join("\n").as_bytes())),
        ),
    ])
}

fn main() -> Result<()> {
    if env::args_os().nth(1).is_some_and(|arg| arg == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    let options = Options::parse(env::args_os().skip(1))?;
    let started = Instant::now();
    let unix_seconds = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let repo = GitRepository::open(&options.repository)?;
    let initial_status = (!repo.is_bare()).then(|| repo.status()).transpose()?;
    let head = if let Some(status) = &initial_status {
        status.head.clone()
    } else {
        repo.history(1)?.first().map(|commit| commit.oid.clone())
    };
    let commit = options.commit.as_ref().or(head.as_ref());
    let cancellation = HistoryCancellation::default();
    // Capture mutable refs once. Every measured request starts from the same
    // immutable tips, and continuation uses the core's returned scan offset.
    let scope = if options.all_refs {
        Some(
            repo.search_history(&HistoryScope::AllRefs, "", 0, 1, &cancellation)?
                .scope,
        )
    } else {
        head.as_ref()
            .map(|head| HistoryScope::FromCommit(head.clone()))
    };
    let setup_changes = commit
        .map(|commit| repo.changes(commit, options.parent))
        .transpose()?;
    let selected_path = options.file.clone().or_else(|| {
        initial_status
            .as_ref()
            .and_then(|status| {
                status
                    .entries
                    .iter()
                    .find(|entry| !entry.conflicted)
                    .map(|entry| entry.path.clone())
            })
            .or_else(|| {
                setup_changes
                    .as_ref()
                    .and_then(|changes| changes.first().map(|file| file.path().to_owned()))
            })
    });
    let setup_ms = started.elapsed().as_secs_f64() * 1000.;
    let deadline = started + Duration::from_secs(options.budget_seconds as u64);
    let mut measurements = Measurements {
        options: &options,
        started,
        series: Vec::new(),
    };
    if let Some(status) = &initial_status {
        measurements.measure("status", || repo.status(), status_info);
        for (name, area) in [
            ("staged_worktree_preview", ChangeArea::Staged),
            ("unstaged_worktree_preview", ChangeArea::Unstaged),
        ] {
            if let Some(entry) = selected_entry(status, options.file.as_deref(), area) {
                measurements.measure(name, || repo.worktree_preview(entry, area), preview_info);
            } else {
                measurements.skip(
                    name,
                    "No eligible change in the captured status for this area/path",
                );
            }
        }
    } else {
        measurements.skip("status", "Bare repository");
        measurements.skip("staged_worktree_preview", "Bare repository");
        measurements.skip("unstaged_worktree_preview", "Bare repository");
    }

    if let Some(scope) = &scope {
        measurements.measure(
            "pinned_history_search_pages",
            || {
                let mut pages = Pages {
                    pages: 0,
                    entries: 0,
                    scanned: Some(0),
                    next: Some(0),
                    stop: String::new(),
                    ids: Vec::new(),
                };
                let mut scope = scope.clone();
                for _ in 0..options.pages {
                    let Some(offset) = pages.next else {
                        break;
                    };
                    if Instant::now() >= deadline {
                        bail!("Harness budget reached between pages; incomplete sample excluded from latency summary");
                    }
                    let page = repo.search_history(
                        &scope,
                        &options.query,
                        offset,
                        options.page_size,
                        &cancellation,
                    )?;
                    scope = page.scope;
                    pages.pages += 1;
                    pages.entries += page.commits.len();
                    *pages.scanned.as_mut().unwrap() += page.scanned;
                    pages.next = page.next_offset;
                    pages.stop = page.stop.label().into();
                    pages
                        .ids
                        .extend(page.commits.into_iter().map(|commit| commit.oid));
                }
                Ok(pages)
            },
            pages_info,
        );
    } else {
        measurements.skip(
            "pinned_history_search_pages",
            "No HEAD for the requested scope",
        );
    }

    if let (Some(commit), Some(path)) = (commit, selected_path.as_ref()) {
        measurements.measure(
            "file_history_follow_first_parent_pages",
            || {
                let mut pages = Pages {
                    pages: 0,
                    entries: 0,
                    scanned: None,
                    next: Some(0),
                    stop: String::new(),
                    ids: Vec::new(),
                };
                for _ in 0..options.pages {
                    let Some(offset) = pages.next else {
                        break;
                    };
                    if Instant::now() >= deadline {
                        bail!("Harness budget reached between pages; incomplete sample excluded from latency summary");
                    }
                    let page =
                        repo.file_history(commit, path, offset, options.page_size, &cancellation)?;
                    pages.pages += 1;
                    pages.entries += page.entries.len();
                    pages.next = page.next_offset;
                    pages.stop = if pages.next.is_some() {
                        "Page full"
                    } else {
                        "Exhausted"
                    }
                    .into();
                    pages.ids.extend(
                        page.entries
                            .into_iter()
                            .map(|entry| format!("{}:{:?}", entry.commit.oid, entry.change)),
                    );
                }
                Ok(pages)
            },
            pages_info,
        );
    } else {
        measurements.skip(
            "file_history_follow_first_parent_pages",
            "No commit or selected path",
        );
    }

    if let Some(commit) = commit {
        for (name, renames) in [
            ("changed_files", false),
            ("changed_files_with_renames", true),
        ] {
            measurements.measure(
                name,
                || {
                    if renames {
                        repo.changes_with_renames(commit, options.parent)
                    } else {
                        repo.changes(commit, options.parent)
                    }
                },
                |files| {
                    object([
                        ("files", integer(files.len())),
                        (
                            "result_sha256",
                            string(digest(format!("{files:?}").as_bytes())),
                        ),
                    ])
                },
            );
        }
    } else {
        measurements.skip("changed_files", "No selected commit");
        measurements.skip("changed_files_with_renames", "No selected commit");
    }

    let final_status = initial_status.as_ref().map(|_| repo.status());
    let scope_info = match scope {
        Some(HistoryScope::PinnedRefs(tips)) => object([
            ("kind", string("pinned_local_refs")),
            ("tips", Json::Array(tips.into_iter().map(string).collect())),
        ]),
        Some(HistoryScope::FromCommit(oid)) => {
            object([("kind", string("from_commit")), ("oid", string(oid))])
        }
        _ => Json::Null,
    };
    let final_status_info = match &final_status {
        Some(Ok(status)) => status_info(status),
        Some(Err(error)) => object([("error", string(error))]),
        None => Json::Null,
    };
    let report = object([
        ("schema_version", integer(1)),
        ("started_unix_seconds", Json::Int(unix_seconds)),
        ("fixture_label", string(&options.label)),
        ("build_label", optional(options.build_label.as_deref())),
        (
            "software",
            object([
                ("crate_version", string(env!("CARGO_PKG_VERSION"))),
                ("target_os", string(env::consts::OS)),
                ("target_arch", string(env::consts::ARCH)),
                ("debug_assertions", Json::Bool(cfg!(debug_assertions))),
                (
                    "available_parallelism",
                    integer(std::thread::available_parallelism().map_or(0, usize::from)),
                ),
            ]),
        ),
        ("head_start", optional(head.as_deref())),
        ("selected_commit", optional(commit.map(String::as_str))),
        ("selected_parent_index", integer(options.parent)),
        (
            "file_history_path",
            selected_path.as_deref().map_or(Json::Null, path_info),
        ),
        ("search_scope", scope_info),
        ("query", string(&options.query)),
        (
            "configuration",
            object([
                ("warmup_per_operation", integer(options.warmup)),
                ("samples_per_operation", integer(options.samples)),
                ("max_pages_per_sample", integer(options.pages)),
                ("page_size", integer(options.page_size)),
                ("budget_seconds", integer(options.budget_seconds)),
            ]),
        ),
        ("setup_ms", Json::Float(setup_ms)),
        (
            "total_ms",
            Json::Float(started.elapsed().as_secs_f64() * 1000.),
        ),
        (
            "initial_status",
            initial_status.as_ref().map_or(Json::Null, status_info),
        ),
        ("final_status", final_status_info),
        (
            "status_unchanged",
            match (&initial_status, &final_status) {
                (Some(before), Some(Ok(after))) => Json::Bool(before == after),
                _ => Json::Null,
            },
        ),
        ("series", Json::Array(measurements.series)),
        (
            "percentiles",
            string(
                "Nearest rank over successful measured samples; warmups and failed attempts excluded; raw attempts retained",
            ),
        ),
        (
            "boundary",
            string(
                "Synchronous core API return; excludes status lookup for preview selection, result checksums/validation, app worker queues, presentation metadata, editors, image decoding and GPUI frames",
            ),
        ),
        (
            "cache_conditions",
            string(
                "One retained GitRepository/object reader; no app content cache; OS caches not flushed. Setup reads status, pins refs and reads selected changed files. Per-sample result checksums run outside timing and warm memory caches.",
            ),
        ),
        (
            "limits",
            string(
                "Budget checked before samples and between pages; active core calls retain their own bounds. A full/page/scan/byte/time stop is not exhaustive history. Status equality does not detect arbitrary unchanged-status edits; inspect preview result_changed and fixture stability.",
            ),
        ),
    ]);
    println!("{}", report.encoded());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn json_escapes_control_bytes_quotes_and_unicode() {
        assert_eq!(
            string("a\n\r\t\0\"\\é").encoded(),
            "\"a\\n\\r\\t\\u0000\\\"\\\\é\""
        );
    }
    #[test]
    fn nearest_rank_retains_the_tail_and_empty_stats_are_not_zero_latency() {
        assert_eq!(percentile(&[1., 2., 3., 90.], 50), 2.);
        assert_eq!(percentile(&[1., 2., 3., 90.], 95), 90.);
        assert_eq!(
            statistics(&[]).encoded(),
            "{\"n\":0,\"p50_ms\":null,\"p95_ms\":null,\"max_ms\":null}"
        );
    }
    #[test]
    fn arguments_bound_work_and_preserve_literal_relative_paths() {
        let options = Options::parse(
            ["repo", "--file", "-odd name.txt", "--samples", "3"].map(OsString::from),
        )
        .unwrap();
        assert_eq!(options.file.as_deref(), Some(Path::new("-odd name.txt")));
        assert_eq!(options.samples, 3);
        for args in [
            ["repo", "--samples", "0"],
            ["repo", "--pages", "5"],
            ["repo", "--file", "../outside"],
        ] {
            assert!(Options::parse(args.map(OsString::from)).is_err());
        }
    }

    #[test]
    fn failed_attempts_remain_visible_and_do_not_enter_success_percentiles() {
        let options =
            Options::parse(["repo", "--warmup", "0", "--samples", "2"].map(OsString::from))
                .unwrap();
        let mut measurements = Measurements {
            options: &options,
            started: Instant::now(),
            series: Vec::new(),
        };
        let mut attempt = 0;
        measurements.measure(
            "fixture",
            || {
                attempt += 1;
                if attempt == 1 {
                    bail!("fixture read failure");
                }
                Ok(7)
            },
            |value| integer(*value),
        );
        let report = measurements.series[0].encoded();
        assert!(report.contains("\"error\":\"fixture read failure\""));
        assert!(report.contains("\"successful_stats\":{\"n\":1,"));
        assert!(report.contains("\"completed\":true"));
    }
}
