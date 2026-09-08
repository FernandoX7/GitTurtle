//! Bounded, cancellable history reads. These APIs belong on a worker thread.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

pub const MAX_HISTORY_PAGE: usize = 500;
const MAX_SEARCH_SCAN: usize = 50_000;
const MAX_SEARCH_BYTES: usize = 64 * 1024 * 1024;
const MAX_HISTORY_RECORD: usize = 2 * 1024 * 1024;
const MAX_FILE_HISTORY_BYTES: usize = 32 * 1024 * 1024;
const MAX_HISTORY_REFS: usize = 16_384;
const MAX_HISTORY_REF_BYTES: usize = MAX_HISTORY_REFS * 65;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryScope {
    /// Begin a new snapshot of local refs, including remote-tracking refs and
    /// detached HEAD. Continue with the returned page's pinned scope. Refresh
    /// by starting again with AllRefs and offset zero. No network access.
    AllRefs,
    /// Immutable tips returned for an AllRefs search session.
    PinnedRefs(Vec<String>),
    /// Ancestors of one immutable, full commit OID.
    FromCommit(String),
}

#[derive(Clone, Debug, Default)]
pub struct HistoryCancellation(Arc<AtomicBool>);

impl HistoryCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    fn check(&self) -> Result<()> {
        ensure!(!self.is_cancelled(), "History read cancelled");
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistorySearchStop {
    Exhausted,
    PageFull,
    ScanLimit,
    ByteLimit,
    TimeLimit,
}

impl HistorySearchStop {
    pub fn label(self) -> &'static str {
        match self {
            Self::Exhausted => "Search complete",
            Self::PageFull => "More history is available",
            Self::ScanLimit => "Scanned 50,000 commits; continue searching",
            Self::ByteLimit => "Read 64 MiB of history; continue searching",
            Self::TimeLimit => "Search paused at its time limit; continue searching",
        }
    }
}

#[derive(Clone, Debug)]
pub struct HistorySearchPage {
    pub scope: HistoryScope,
    pub commits: Vec<Commit>,
    /// Number of commits inspected in this request, including nonmatches.
    pub scanned: usize,
    /// Resume the same query/scope here. `None` alone means exhaustive.
    pub next_offset: Option<usize>,
    pub stop: HistorySearchStop,
}

#[derive(Clone, Debug)]
pub struct FileHistoryEntry {
    pub commit: Commit,
    /// Exact revision paths and blob IDs, including the absent deletion side.
    pub change: FileChange,
    /// This history follows the first-parent lineage through merges. The
    /// commit retains all actual parent OIDs for other comparison choices.
    pub parent_oid: Option<String>,
}

#[derive(Clone, Debug)]
pub struct FileHistoryPage {
    pub anchor: String,
    pub path: PathBuf,
    pub entries: Vec<FileHistoryEntry>,
    pub next_offset: Option<usize>,
}

impl GitRepository {
    /// Literal, case-insensitive substring search of subject, description,
    /// author name and full hash. An empty query lists history. Offsets count
    /// scanned commits, not matches. Each call inspects at most 50,000 commits,
    /// 64 MiB and 15 seconds; a budget stop always returns a continuation.
    pub fn search_history(
        &self,
        scope: &HistoryScope,
        query: &str,
        offset: usize,
        limit: usize,
        cancellation: &HistoryCancellation,
    ) -> Result<HistorySearchPage> {
        ensure!(
            (1..=MAX_HISTORY_PAGE).contains(&limit),
            "History page must contain 1–500 results"
        );
        ensure!(query.len() <= 4096, "History query exceeds 4 KiB");
        cancellation.check()?;
        let started = Instant::now();
        let scope = match scope {
            HistoryScope::AllRefs => {
                ensure!(
                    offset == 0,
                    "Continue all-ref search with the pinned scope returned by its first page"
                );
                self.pin_history_refs(cancellation)?
            }
            other => other.clone(),
        };
        let mut command = history_command(&self.path);
        command.arg(format!("--skip={offset}"));
        let input = match &scope {
            HistoryScope::PinnedRefs(refs) => {
                ensure!(
                    refs.len() <= MAX_HISTORY_REFS,
                    "History scope exceeds 16,384 reference tips"
                );
                let mut input = Vec::new();
                for oid in refs {
                    validate_oid(oid)?;
                    input.extend_from_slice(oid.as_bytes());
                    input.push(b'\n');
                }
                command.arg("--stdin");
                Some(input)
            }
            HistoryScope::FromCommit(oid) => {
                validate_oid(oid)?;
                command.arg(oid);
                None
            }
            HistoryScope::AllRefs => unreachable!(),
        };
        command.arg("--");
        let query = query.to_lowercase();
        let mut page = HistorySearchPage {
            scope,
            commits: Vec::new(),
            scanned: 0,
            next_offset: None,
            stop: HistorySearchStop::Exhausted,
        };
        let mut pending = Vec::new();
        let mut fields = Vec::new();
        let mut record_bytes = 0;
        let mut total_bytes = 0usize;
        if matches!(&page.scope, HistoryScope::PinnedRefs(refs) if refs.is_empty()) {
            return Ok(page);
        }
        let end = stream_history(
            command,
            input,
            cancellation,
            GIT_TIMEOUT.saturating_sub(started.elapsed()),
            |chunk| {
                total_bytes = total_bytes.saturating_add(chunk.len());
                if total_bytes > MAX_SEARCH_BYTES {
                    page.stop = HistorySearchStop::ByteLimit;
                    return Ok(false);
                }
                for &byte in chunk {
                    record_bytes += 1;
                    ensure!(
                        record_bytes <= MAX_HISTORY_RECORD,
                        "One commit's metadata exceeds the 2 MiB history limit"
                    );
                    if byte != 0 {
                        pending.push(byte);
                        continue;
                    }
                    fields.push(std::mem::take(&mut pending));
                    if fields.len() != 6 {
                        continue;
                    }
                    let commit = parse_commit_fields(&fields)?;
                    fields.clear();
                    record_bytes = 0;
                    page.scanned += 1;
                    if [&commit.oid, &commit.author, &commit.subject, &commit.body]
                        .iter()
                        .any(|value| value.to_lowercase().contains(&query))
                    {
                        page.commits.push(commit);
                    }
                    if page.commits.len() == limit {
                        page.stop = HistorySearchStop::PageFull;
                        return Ok(false);
                    }
                    if page.scanned == MAX_SEARCH_SCAN {
                        page.stop = HistorySearchStop::ScanLimit;
                        return Ok(false);
                    }
                }
                Ok(true)
            },
        )?;
        if end == ReadEnd::TimedOut {
            page.stop = HistorySearchStop::TimeLimit;
        }
        if page.stop == HistorySearchStop::Exhausted {
            ensure!(
                pending.is_empty() && fields.is_empty(),
                "Incomplete commit metadata from Git"
            );
        } else {
            page.next_offset = Some(
                offset
                    .checked_add(page.scanned)
                    .context("History offset overflow")?,
            );
        }
        Ok(page)
    }

    fn pin_history_refs(&self, cancellation: &HistoryCancellation) -> Result<HistoryScope> {
        let mut command = git_command(&self.path);
        command.args(["rev-parse", "--revs-only", "--all", "HEAD"]);
        let mut bytes = Vec::new();
        let end = stream_history(command, None, cancellation, GIT_TIMEOUT, |chunk| {
            ensure!(
                bytes.len().saturating_add(chunk.len()) <= MAX_HISTORY_REF_BYTES,
                "History scope exceeds its reference snapshot limit"
            );
            bytes.extend_from_slice(chunk);
            Ok(true)
        })?;
        ensure!(
            end != ReadEnd::TimedOut,
            "Reference snapshot exceeded its 15 second time limit"
        );
        let mut refs = Vec::new();
        for line in bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            let oid = text(line);
            validate_oid(&oid)?;
            refs.push(oid);
        }
        refs.sort_unstable();
        refs.dedup();
        ensure!(
            refs.len() <= MAX_HISTORY_REFS,
            "History scope exceeds 16,384 reference tips"
        );
        Ok(HistoryScope::PinnedRefs(refs))
    }

    /// One file's rename-following first-parent history from an immutable
    /// commit. Merge rows compare against their real first parent; side-branch
    /// commits are represented by their merge. This explicit lineage avoids
    /// Git --follow's ambiguous path tracking across non-linear history.
    /// Paths are literal repository-relative bytes, including deleted paths.
    /// Paging replays the prefix to track skipped renames, within the same
    /// 32 MiB/15 second limits; deep histories can require an older anchor.
    pub fn file_history(
        &self,
        oid: &str,
        path: &Path,
        offset: usize,
        limit: usize,
        cancellation: &HistoryCancellation,
    ) -> Result<FileHistoryPage> {
        validate_oid(oid)?;
        ensure!(
            (1..=MAX_HISTORY_PAGE).contains(&limit),
            "File-history page must contain 1–500 results"
        );
        ensure!(
            !path.as_os_str().is_empty()
                && path.as_os_str().len() <= 16 * 1024
                && path
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_))),
            "Expected a literal repository-relative file path"
        );
        cancellation.check()?;
        let prefix_count = offset
            .checked_add(limit)
            .and_then(|count| count.checked_add(1))
            .context("File-history offset overflow")?;
        let mut command = history_command(&self.path);
        command
            .args([
                "--follow",
                "--first-parent",
                "--diff-merges=first-parent",
                "--raw",
                "--no-abbrev",
                "--root",
                "--find-renames=50%",
                "-l1000",
            ])
            // --skip suppresses a rename before --follow can update its path,
            // losing all revisions under the older name. Read the bounded
            // prefix and remove earlier rows only after following the lineage.
            .arg(format!("--max-count={prefix_count}"))
            .arg(oid)
            .arg("--")
            .arg(path);
        let mut bytes = Vec::new();
        let end = stream_history(command, None, cancellation, GIT_TIMEOUT, |chunk| {
            ensure!(
                bytes.len().saturating_add(chunk.len()) <= MAX_FILE_HISTORY_BYTES,
                "File-history prefix exceeds its 32 MiB limit. Open an older revision as the history anchor, or request a smaller page"
            );
            bytes.extend_from_slice(chunk);
            Ok(true)
        })?;
        ensure!(
            end != ReadEnd::TimedOut,
            "File-history prefix exceeded its 15 second time limit. Retry, or open an older revision as the history anchor"
        );
        let mut entries = parse_file_history(&bytes, cancellation)?;
        entries.drain(..offset.min(entries.len()));
        let next_offset = if entries.len() > limit {
            entries.truncate(limit);
            Some(
                offset
                    .checked_add(limit)
                    .context("File-history offset overflow")?,
            )
        } else {
            None
        };
        Ok(FileHistoryPage {
            anchor: oid.to_owned(),
            path: path.to_owned(),
            entries,
            next_offset,
        })
    }
}

fn history_command(path: &Path) -> Command {
    let mut command = git_command(path);
    command.arg("--literal-pathspecs").args([
        "-c",
        "log.follow=false",
        "log",
        "--topo-order",
        "--no-show-signature",
        "--no-decorate",
        "--no-notes",
        "--no-ext-diff",
        "--no-textconv",
        "--encoding=UTF-8",
        "-z",
        "--format=%H%x00%P%x00%an%x00%at%x00%s%x00%b",
    ]);
    command
}

fn parse_commit_fields(fields: &[impl AsRef<[u8]>]) -> Result<Commit> {
    ensure!(fields.len() == 6, "Malformed commit metadata from Git");
    let oid = text(fields[0].as_ref());
    validate_oid(&oid)?;
    let parents = text(fields[1].as_ref())
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for parent in &parents {
        validate_oid(parent)?;
    }
    Ok(Commit {
        oid,
        parents,
        author: text(fields[2].as_ref()),
        timestamp: std::str::from_utf8(fields[3].as_ref())?
            .parse()
            .context("Invalid commit timestamp")?,
        subject: text(fields[4].as_ref()),
        body: text(fields[5].as_ref()).trim_end_matches('\n').to_owned(),
    })
}

fn parse_file_history(
    bytes: &[u8],
    cancellation: &HistoryCancellation,
) -> Result<Vec<FileHistoryEntry>> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    ensure!(bytes.last() == Some(&0), "Incomplete file history from Git");
    let fields = bytes[..bytes.len() - 1]
        .split(|b| *b == 0)
        .collect::<Vec<_>>();
    let mut cursor = 0;
    let mut entries = Vec::new();
    while cursor < fields.len() {
        cancellation.check()?;
        ensure!(
            fields.len() - cursor >= 6,
            "Incomplete file-history metadata"
        );
        ensure!(
            fields[cursor..cursor + 6]
                .iter()
                .map(|f| f.len())
                .sum::<usize>()
                <= MAX_HISTORY_RECORD,
            "One commit's metadata exceeds the 2 MiB history limit"
        );
        let commit = parse_commit_fields(&fields[cursor..cursor + 6])?;
        cursor += 6;
        let mut changes = Vec::new();
        while cursor < fields.len() && fields[cursor].starts_with(b"\n:") {
            let header = &fields[cursor][1..];
            let renamed = header
                .rsplit(|b| *b == b' ')
                .next()
                .is_some_and(|status| status.starts_with(b"R"));
            let count = if renamed { 3 } else { 2 };
            ensure!(
                fields.len() - cursor >= count,
                "Incomplete file-history paths"
            );
            let mut raw = header.to_vec();
            raw.push(0);
            for field in &fields[cursor + 1..cursor + count] {
                raw.extend_from_slice(field);
                raw.push(0);
            }
            changes.extend(parse_changes(&raw)?);
            cursor += count;
        }
        ensure!(
            changes.len() == 1,
            "Git returned an ambiguous file-history comparison"
        );
        entries.push(FileHistoryEntry {
            parent_oid: commit.parents.first().cloned(),
            commit,
            change: changes.remove(0),
        });
    }
    Ok(entries)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadEnd {
    Complete,
    Stopped,
    TimedOut,
}

enum ReadMessage {
    Stdout(Vec<u8>),
    StdoutEnd,
    Stderr(Vec<u8>),
    Error(std::io::Error),
}

/// Drains both pipes through a bounded queue. Cancellation and the deadline
/// cover the process and descendants which retain its pipes after it exits.
fn stream_history(
    mut command: Command,
    input: Option<Vec<u8>>,
    cancellation: &HistoryCancellation,
    timeout: Duration,
    mut consume: impl FnMut(&[u8]) -> Result<bool>,
) -> Result<ReadEnd> {
    cancellation.check()?;
    isolate_process_group(&mut command);
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Unable to start Git history read")?;
    let input_writer = input.map(|bytes| {
        let mut stdin = child.stdin.take().expect("Requested Git input pipe");
        thread::spawn(move || stdin.write_all(&bytes))
    });
    let mut stdout = child.stdout.take().context("Missing Git output pipe")?;
    let stderr = child.stderr.take().context("Missing Git error pipe")?;
    let (send, receive) = mpsc::sync_channel(8);
    let errors = send.clone();
    let output_reader = thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            let message = match stdout.read(&mut chunk) {
                Ok(0) => {
                    let _ = send.send(ReadMessage::StdoutEnd);
                    break;
                }
                Ok(n) => ReadMessage::Stdout(chunk[..n].to_vec()),
                Err(error) => {
                    let _ = send.send(ReadMessage::Error(error));
                    break;
                }
            };
            if send.send(message).is_err() {
                break;
            }
        }
    });
    let error_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let message = match stderr.take(128 * 1024 + 1).read_to_end(&mut bytes) {
            Ok(_) => ReadMessage::Stderr(bytes),
            Err(error) => ReadMessage::Error(error),
        };
        let _ = errors.send(message);
    });
    let start = Instant::now();
    let result = (|| {
        let mut stdout_done = false;
        let mut stderr = None;
        let mut status = None;
        loop {
            cancellation.check()?;
            if start.elapsed() >= timeout {
                return Ok(ReadEnd::TimedOut);
            }
            match receive.recv_timeout(Duration::from_millis(5)) {
                Ok(ReadMessage::Stdout(bytes)) => {
                    if !consume(&bytes)? {
                        return Ok(ReadEnd::Stopped);
                    }
                }
                Ok(ReadMessage::StdoutEnd) => stdout_done = true,
                Ok(ReadMessage::Stderr(bytes)) => {
                    ensure!(
                        bytes.len() <= 128 * 1024,
                        "Git history error output exceeded its limit"
                    );
                    stderr = Some(bytes);
                }
                Ok(ReadMessage::Error(error)) => return Err(error.into()),
                Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {}
            }
            if status.is_none() {
                status = child.try_wait()?;
            }
            if stdout_done && let (Some(status), Some(stderr)) = (status, stderr.as_ref()) {
                ensure!(
                    status.success(),
                    "Git history read failed: {}",
                    text(stderr).trim()
                );
                return Ok(ReadEnd::Complete);
            }
        }
    })();
    // Always close the receiver before joining: a bounded sender may otherwise
    // remain blocked when a page fills while Git is still producing output.
    terminate_process_group(&child);
    let _ = child.kill();
    let _ = child.wait();
    drop(receive);
    let _ = output_reader.join();
    let _ = error_reader.join();
    if let Some(writer) = input_writer {
        let written = writer
            .join()
            .map_err(|_| anyhow::anyhow!("Git history input writer stopped"))?;
        if matches!(result, Ok(ReadEnd::Complete)) {
            written.context("Send history reference snapshot")?;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn cancellation_terminates_active_read_and_its_descendants() {
        let temp = tempfile::TempDir::new().unwrap();
        let marker = temp.path().join("survived");
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            "(sleep 0.3; touch \"$1\") & echo ready; wait",
            "fixture",
        ]);
        command.arg(&marker);
        let cancel = HistoryCancellation::default();
        let cancel_on_output = cancel.clone();
        let started = Instant::now();
        let error = stream_history(command, None, &cancel, Duration::from_secs(5), |_| {
            cancel_on_output.cancel();
            Ok(true)
        })
        .unwrap_err();
        assert!(error.to_string().contains("cancelled"));
        assert!(started.elapsed() < Duration::from_secs(2));
        thread::sleep(Duration::from_millis(400));
        assert!(!marker.exists(), "Cancelled Git descendant remained active");
    }

    #[cfg(unix)]
    #[test]
    fn deadline_includes_pipe_holders_after_the_parent_exits() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 10 & exit 0"]);
        let started = Instant::now();
        let end = stream_history(
            command,
            None,
            &HistoryCancellation::default(),
            Duration::from_millis(30),
            |_| Ok(true),
        )
        .unwrap();
        assert_eq!(end, ReadEnd::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn a_full_page_stops_a_producer_without_blocking_on_the_bounded_queue() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "while :; do printf 'history record\\n'; done"]);
        let started = Instant::now();
        let end = stream_history(
            command,
            None,
            &HistoryCancellation::default(),
            Duration::from_secs(2),
            |_| Ok(false),
        )
        .unwrap();
        assert_eq!(end, ReadEnd::Stopped);
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
