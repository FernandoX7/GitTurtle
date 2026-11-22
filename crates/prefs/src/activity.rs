//! Pure bounded app activity persistence and diagnostic categorization.

use crate::preferences;
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

pub const LIMIT: usize = 200;
pub const BYTE_LIMIT: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Outcome {
    Running,
    Success,
    Failure,
    Cancelled,
    Uncertain,
}
impl Outcome {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Running => "In progress",
            Self::Success => "Succeeded",
            Self::Failure => "Failed",
            Self::Cancelled => "Cancelled · inspect state",
            Self::Uncertain => "Outcome uncertain",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: u64,
    pub time: i64,
    pub repository: String,
    pub operation: String,
    pub target: String,
    pub outcome: Outcome,
    pub explanation: String,
}

pub fn safe(value: &str, limit: usize) -> String {
    gitturtle_core::redact_diagnostic(value)
        .chars()
        .take(limit)
        .collect()
}
pub fn retained_explanation(
    success: bool,
    cancelled: bool,
    uncertain: bool,
    diagnostic: &str,
) -> &'static str {
    if success {
        return "Git reported that the requested operation completed.";
    }
    if cancelled {
        return "Cancellation was requested. Some changes may have completed; inspect current state before another action.";
    }
    if uncertain {
        return "The operation ended without a confirmed result. Inspect local state; verifying a remote outcome requires an explicit network action.";
    }
    let diagnostic = diagnostic.to_ascii_lowercase();
    if diagnostic.contains("authenticat")
        || diagnostic.contains("credential")
        || diagnostic.contains("permission denied (publickey)")
    {
        "Authentication failed or was unavailable. Check the configured credential helper or SSH agent before another attempt."
    } else if diagnostic.contains("signing") || diagnostic.contains("gpg") {
        "Configured signing did not complete. Check the signing tool and key; GitTurtle did not fall back to an unsigned operation."
    } else if diagnostic.contains("conflict") {
        "Git reported conflicts or unresolved paths. Inspect Working Changes and the current operation before continuing."
    } else if diagnostic.contains("changed after")
        || diagnostic.contains("review again")
        || diagnostic.contains("stale")
        || diagnostic.contains("changed since")
    {
        "The reviewed target changed. Refresh and review the operation again."
    } else if diagnostic.contains("missing object")
        || diagnostic.contains("object is unavailable")
        || diagnostic.contains("object failed")
        || diagnostic.contains("verification")
    {
        "A required object was missing or failed verification. Inspect the selected object and source."
    } else if diagnostic.contains("permission denied") || diagnostic.contains("read-only file") {
        "Filesystem permissions prevented the operation. Inspect the destination and current repository state."
    } else {
        "Git reported a failure. Inspect current state and the repository's error details before retrying; GitTurtle does not retry writes automatically."
    }
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

pub fn read_entries() -> anyhow::Result<Vec<Entry>> {
    let path = preferences::settings_path()?.with_file_name("activity.json");
    let bytes = match preferences::read_store(&path, BYTE_LIMIT) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut entries: Vec<Entry> = serde_json::from_slice(&bytes)?;
    if entries.len() > LIMIT {
        entries.drain(..entries.len() - LIMIT);
    }
    for entry in &mut entries {
        entry.repository = safe(&entry.repository, 4096);
        entry.operation = safe(&entry.operation, 160);
        entry.target = safe(&entry.target, 2048);
        entry.explanation = retained_explanation(
            entry.outcome == Outcome::Success,
            entry.outcome == Outcome::Cancelled,
            entry.outcome == Outcome::Uncertain,
            &entry.explanation,
        )
        .into();
        if entry.outcome == Outcome::Running {
            entry.outcome = Outcome::Uncertain;
            entry.explanation = "The application ended before this operation's result was recorded. Inspect current local state; any remote verification needs an explicit network action.".into();
        }
    }
    Ok(entries)
}

pub fn begin_entry(
    entries: &mut Vec<Entry>,
    repository: &Path,
    operation: &str,
    target: &str,
) -> u64 {
    let id = now().max(entries.last().map_or(0, |entry| entry.id.saturating_add(1)));
    entries.push(Entry {
        id,
        time: (id / 1000) as i64,
        repository: safe(&repository.display().to_string(), 4096),
        operation: safe(operation.trim_end_matches('…'), 160),
        target: safe(target, 2048),
        outcome: Outcome::Running,
        explanation: "Submitted once to GitTurtle's serialized operation executor.".into(),
    });
    if entries.len() > LIMIT {
        entries.remove(0);
    }
    id
}

pub fn finish_entry(
    entries: &mut [Entry],
    id: u64,
    success: bool,
    cancelled: bool,
    uncertain: bool,
    message: &str,
) {
    if let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) {
        entry.outcome = if success {
            Outcome::Success
        } else if cancelled {
            Outcome::Cancelled
        } else if uncertain {
            Outcome::Uncertain
        } else {
            Outcome::Failure
        };
        entry.explanation = retained_explanation(success, cancelled, uncertain, message).into();
    }
}

pub fn save_entries(entries: &[Entry]) -> anyhow::Result<()> {
    let path = preferences::settings_path()?.with_file_name("activity.json");
    let bytes = serde_json::to_vec(entries)?;
    anyhow::ensure!(
        bytes.len() as u64 <= BYTE_LIMIT,
        "Activity history exceeds storage limit"
    );
    preferences::atomic_write(&path, &bytes)
}
