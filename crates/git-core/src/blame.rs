//! Passive attribution of immutable text and bounded raw working snapshots.
use super::*;
use crate::history::{ReadEnd, history_command, parse_commit_fields, stream_history};
use std::collections::HashMap;

const MAX_BLAME_OUTPUT: usize = 32 * 1024 * 1024;
pub const MAX_LINE_HISTORY: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlameTarget {
    Committed {
        oid: String,
        path: PathBuf,
    },
    /// Always the raw working file relative to current HEAD, even when opened
    /// from a staged row. Staged-only and unstaged edits are both uncommitted.
    Working {
        path: PathBuf,
    },
}

impl BlameTarget {
    pub fn path(&self) -> &Path {
        match self {
            Self::Committed { path, .. } | Self::Working { path } => path,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribution {
    pub oid: String,
    pub author: String,
    pub timestamp: i64,
    pub subject: String,
    /// Literal path at the originating commit, including earlier rename names.
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlameLine {
    pub text: String,
    pub original_line: usize,
    /// None means this line does not match a line in HEAD.
    pub attribution: Option<Arc<Attribution>>,
}

#[derive(Clone, Debug)]
pub struct Blame {
    pub target: BlameTarget,
    pub anchor: Option<String>,
    /// Git treats missing shallow ancestors as boundaries, not read errors.
    pub shallow: bool,
    pub lines: Vec<BlameLine>,
}

#[derive(Clone, Debug)]
pub struct LineHistory {
    pub anchor: String,
    pub path: PathBuf,
    pub line: usize,
    pub commits: Vec<Commit>,
    pub truncated: bool,
}

impl GitRepository {
    /// Default Git attribution follows whole-file renames across all parents.
    /// It does not search for copies or moved lines across files. No textconv,
    /// filters, external diff, object fetching or index refresh is performed.
    pub fn blame(&self, target: &BlameTarget, cancellation: &HistoryCancellation) -> Result<Blame> {
        validate_blame_path(target.path())?;
        check_cancel(cancellation)?;
        let mut command = git_command(&self.path);
        command.args(["rev-parse", "--is-shallow-repository"]);
        let shallow = trim_line(&read_attribution(command, cancellation, 1024)?) == b"true";
        match target {
            BlameTarget::Committed { oid, path } => {
                let lines = self.committed_blame(oid, path, cancellation)?;
                Ok(Blame {
                    target: target.clone(),
                    anchor: Some(oid.clone()),
                    shallow,
                    lines,
                })
            }
            BlameTarget::Working { path } => {
                ensure!(
                    !self.bare,
                    "Working attribution is unavailable in a bare repository"
                );
                let status = self.status()?;
                check_cancel(cancellation)?;
                let entry = status.entries.iter().find(|entry| entry.path == *path);
                ensure!(
                    !entry.is_some_and(|entry| entry.conflicted),
                    "Resolve this file's conflicts before reading working attribution"
                );
                let base_path = entry
                    .and_then(|entry| entry.original_path.as_deref())
                    .unwrap_or(path);
                let (mode, bytes) = crate::work::read_worktree_file(&self.path, path, "")?;
                ensure!(
                    mode == "100644" || mode == "100755",
                    "Attribution is available for regular text files; symbolic-link targets are never followed"
                );
                let source = bounded_text(&bytes)?;
                check_cancel(cancellation)?;
                let base = if let Some(oid) = &status.head {
                    if self.blame_blob(oid, base_path, cancellation)?.is_some() {
                        self.committed_blame(oid, base_path, cancellation)?
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                };
                let lines = map_working_lines(&base, source, cancellation)?;
                Ok(Blame {
                    target: target.clone(),
                    anchor: status.head,
                    shallow,
                    lines,
                })
            }
        }
    }

    fn blame_blob(
        &self,
        oid: &str,
        path: &Path,
        cancellation: &HistoryCancellation,
    ) -> Result<Option<Vec<u8>>> {
        validate_oid(oid)?;
        validate_blame_path(path)?;
        let mut command = git_command(&self.path);
        command
            .arg("--literal-pathspecs")
            .args(["ls-tree", "-z", oid, "--"])
            .arg(path);
        let bytes = read_attribution(command, cancellation, 32 * 1024)?;
        if bytes.is_empty() {
            return Ok(None);
        }
        let metadata = bytes
            .split(|byte| *byte == b'\t')
            .next()
            .context("Missing tree entry")?;
        let fields: Vec<_> = metadata.split(|byte| *byte == b' ').collect();
        ensure!(
            fields.len() == 3
                && (fields[0] == b"100644" || fields[0] == b"100755")
                && fields[1] == b"blob",
            "Attribution is available for regular text files; symbolic links and submodules are not followed"
        );
        let blob = std::str::from_utf8(fields[2])?;
        ensure!(
            self.blob_size(blob)? <= MAX_DIFF_BYTES,
            "Attribution exceeds the 2 MiB text limit"
        );
        check_cancel(cancellation)?;
        let bytes = self.blob(blob)?;
        bounded_text(&bytes)?;
        Ok(Some(bytes))
    }

    fn committed_blame(
        &self,
        oid: &str,
        path: &Path,
        cancellation: &HistoryCancellation,
    ) -> Result<Vec<BlameLine>> {
        let source = self.blame_blob(oid, path, cancellation)?.context("This file is absent at the selected commit. Choose its previous revision to inspect attribution")?;
        if source.is_empty() {
            return Ok(Vec::new());
        }
        let mut command = git_command(&self.path);
        command
            .arg("--literal-pathspecs")
            .args([
                "-c",
                "blame.ignoreRevsFile=",
                "blame",
                "--line-porcelain",
                "--no-textconv",
                "--encoding=UTF-8",
                oid,
                "--",
            ])
            .arg(path);
        let bytes = read_attribution(command, cancellation, MAX_BLAME_OUTPUT)?;
        parse_blame(&bytes, bounded_text(&source)?, cancellation)
    }

    /// Trace one committed line along first parents, with Git's rename/line
    /// range heuristics. Copies and other merge-parent lineages are not scanned.
    /// A full page reports truncation rather than an exhaustive history claim.
    pub fn line_history(
        &self,
        oid: &str,
        path: &Path,
        line: usize,
        cancellation: &HistoryCancellation,
    ) -> Result<LineHistory> {
        validate_oid(oid)?;
        validate_blame_path(path)?;
        ensure!(
            (1..=MAX_DIFF_LINES).contains(&line),
            "Select a valid source line"
        );
        let source = self
            .blame_blob(oid, path, cancellation)?
            .context("The line's file is absent at this revision")?;
        ensure!(
            line <= bounded_text(&source)?.split_inclusive('\n').count(),
            "The selected line no longer exists at this revision"
        );
        let mut range = OsString::from(format!("{line},{line}:"));
        range.push(path.as_os_str());
        let mut command = history_command(&self.path);
        command
            .args([
                "--first-parent",
                "--no-patch",
                "--find-renames=50%",
                "-l1000",
            ])
            .arg(format!("--max-count={}", MAX_LINE_HISTORY + 1))
            .arg("-L")
            .arg(range)
            .arg(oid);
        let bytes = read_attribution(command, cancellation, 8 * 1024 * 1024)?;
        let mut fields: Vec<_> = bytes.split(|byte| *byte == 0).collect();
        if fields.last() == Some(&&b""[..]) {
            fields.pop();
        }
        ensure!(
            fields.len().is_multiple_of(6),
            "Incomplete line-history metadata"
        );
        let mut commits = Vec::new();
        for fields in fields.as_chunks::<6>().0 {
            check_cancel(cancellation)?;
            commits.push(parse_commit_fields(fields)?);
        }
        let truncated = commits.len() > MAX_LINE_HISTORY;
        commits.truncate(MAX_LINE_HISTORY);
        Ok(LineHistory {
            anchor: oid.into(),
            path: path.into(),
            line,
            commits,
            truncated,
        })
    }
}

fn check_cancel(cancellation: &HistoryCancellation) -> Result<()> {
    ensure!(!cancellation.is_cancelled(), "Attribution read cancelled");
    Ok(())
}

fn validate_blame_path(path: &Path) -> Result<()> {
    ensure!(
        !path.as_os_str().is_empty()
            && path.as_os_str().len() <= 16 * 1024
            && path
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_))),
        "Expected a literal repository-relative file path"
    );
    Ok(())
}

fn bounded_text(bytes: &[u8]) -> Result<&str> {
    ensure!(
        bytes.len() <= MAX_DIFF_BYTES,
        "Attribution exceeds the 2 MiB text limit"
    );
    ensure!(
        !bytes.contains(&0),
        "Attribution is unavailable for binary files"
    );
    let text = std::str::from_utf8(bytes).context("Attribution requires UTF-8 text")?;
    ensure!(
        text.split_inclusive('\n').count() <= MAX_DIFF_LINES,
        "Attribution exceeds the 100,000-line limit"
    );
    Ok(text)
}

fn read_attribution(
    command: Command,
    cancellation: &HistoryCancellation,
    limit: usize,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let end = stream_history(command, None, cancellation, GIT_TIMEOUT, |chunk| {
        ensure!(
            bytes.len().saturating_add(chunk.len()) <= limit,
            "Attribution output exceeds its {} MiB limit",
            limit / 1024 / 1024
        );
        bytes.extend_from_slice(chunk);
        Ok(true)
    })?;
    ensure!(
        end == ReadEnd::Complete,
        "Attribution exceeded its 15 second read limit. Choose a newer or smaller file revision"
    );
    Ok(bytes)
}

fn map_working_lines(
    base: &[BlameLine],
    source: &str,
    cancellation: &HistoryCancellation,
) -> Result<Vec<BlameLine>> {
    let old: String = base.iter().map(|line| line.text.as_str()).collect();
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .timeout(Duration::from_millis(250))
        .diff_lines(old.as_str(), source);
    let mut lines = Vec::new();
    for change in diff.iter_all_changes() {
        check_cancel(cancellation)?;
        if change.tag() == similar::ChangeTag::Delete {
            continue;
        }
        let attribution = change.old_index().and_then(|index| base.get(index));
        lines.push(BlameLine {
            text: change.value().into(),
            original_line: attribution.map_or(0, |line| line.original_line),
            attribution: attribution.and_then(|line| line.attribution.clone()),
        });
    }
    Ok(lines)
}

fn parse_blame(
    bytes: &[u8],
    source: &str,
    cancellation: &HistoryCancellation,
) -> Result<Vec<BlameLine>> {
    let mut records = bytes.split(|byte| *byte == b'\n').peekable();
    let mut source_lines = source.split_inclusive('\n');
    let mut lines = Vec::new();
    let mut interned: HashMap<(String, PathBuf), Arc<Attribution>> = HashMap::new();
    while let Some(header) = records.next() {
        if header.is_empty() && records.peek().is_none() {
            break;
        }
        check_cancel(cancellation)?;
        let fields: Vec<_> = header.split(|byte| *byte == b' ').collect();
        ensure!(fields.len() >= 3, "Malformed attribution header");
        let oid = std::str::from_utf8(fields[0])?.to_owned();
        validate_oid(&oid)?;
        let original_line = std::str::from_utf8(fields[1])?.parse()?;
        let final_line: usize = std::str::from_utf8(fields[2])?.parse()?;
        ensure!(
            final_line == lines.len() + 1,
            "Attribution line sequence changed"
        );
        let mut author = None;
        let mut timestamp = None;
        let mut subject = None;
        let mut path = None;
        let mut ended = false;
        for record in records.by_ref() {
            if record.starts_with(b"\t") {
                ended = true;
                break;
            }
            if let Some(value) = record.strip_prefix(b"author ") {
                author = Some(text(value));
            }
            if let Some(value) = record.strip_prefix(b"author-time ") {
                timestamp = Some(std::str::from_utf8(value)?.parse()?);
            }
            if let Some(value) = record.strip_prefix(b"summary ") {
                subject = Some(text(value));
            }
            if let Some(value) = record.strip_prefix(b"filename ") {
                path = Some(porcelain_path(value)?);
            }
        }
        ensure!(ended, "Incomplete attribution record");
        let path = path.context("Missing attribution path")?;
        let attribution = Attribution {
            oid: oid.clone(),
            path: path.clone(),
            author: author.context("Missing attribution author")?,
            timestamp: timestamp.context("Missing attribution time")?,
            subject: subject.context("Missing attribution subject")?,
        };
        let attribution = interned
            .entry((oid, path))
            .or_insert_with(|| Arc::new(attribution))
            .clone();
        lines.push(BlameLine {
            text: source_lines
                .next()
                .context("Attribution has excess source lines")?
                .into(),
            original_line,
            attribution: Some(attribution),
        });
    }
    ensure!(
        source_lines.next().is_none(),
        "Attribution did not cover every source line"
    );
    Ok(lines)
}

/// Git's C-quoted porcelain paths encode non-UTF8 bytes with octal escapes.
fn porcelain_path(bytes: &[u8]) -> Result<PathBuf> {
    if !bytes.starts_with(b"\"") {
        return Ok(path_from_bytes(bytes));
    }
    ensure!(
        bytes.ends_with(b"\"") && bytes.len() >= 2,
        "Incomplete quoted attribution path"
    );
    let mut decoded = Vec::new();
    let mut index = 1;
    while index < bytes.len() - 1 {
        let byte = bytes[index];
        index += 1;
        if byte != b'\\' {
            decoded.push(byte);
            continue;
        }
        ensure!(
            index < bytes.len() - 1,
            "Incomplete attribution path escape"
        );
        let byte = bytes[index];
        index += 1;
        decoded.push(match byte {
            b'a' => 7,
            b'b' => 8,
            b't' => b'\t',
            b'n' => b'\n',
            b'v' => 11,
            b'f' => 12,
            b'r' => b'\r',
            b'\\' | b'"' => byte,
            b'0'..=b'3' => {
                ensure!(
                    index + 1 < bytes.len() - 1
                        && bytes[index..index + 2]
                            .iter()
                            .all(|b| (b'0'..=b'7').contains(b)),
                    "Invalid octal attribution path escape"
                );
                let value =
                    (byte - b'0') * 64 + (bytes[index] - b'0') * 8 + (bytes[index + 1] - b'0');
                index += 2;
                value
            }
            _ => bail!("Unknown attribution path escape"),
        });
    }
    Ok(path_from_bytes(&decoded))
}
