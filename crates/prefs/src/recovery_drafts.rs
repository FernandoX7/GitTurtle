//! Pure bounded recovery-draft storage outside the repository.

use crate::preferences;
use anyhow::{Context as _, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const MAX_ENTRIES: usize = 256;
pub const MAX_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_TEXT: usize = 2 * 1024 * 1024;
pub type Batch = HashMap<Key, Option<Draft>>;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Key {
    pub worktree: Vec<u8>,
    pub file: Vec<u8>,
    pub kind: Kind,
    pub source: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Kind {
    Conflict,
    RebaseMessage,
    RewriteSeries,
}

pub fn path_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str().as_encoded_bytes().to_vec()
}
pub fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(bytes.to_vec()).into()
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(bytes).as_ref())
    }
}
impl Key {
    pub fn conflict(root: &Path, file: &Path, source: String) -> Self {
        Self {
            worktree: path_bytes(root),
            file: path_bytes(file),
            kind: Kind::Conflict,
            source,
        }
    }
    pub fn message(expected: &gitturtle_core::InteractiveRebaseResume) -> Self {
        Self {
            worktree: path_bytes(&expected.root),
            file: vec![],
            kind: Kind::RebaseMessage,
            source: expected.draft_identity(),
        }
    }
    pub fn same_location(&self, other: &Self) -> bool {
        self.worktree == other.worktree && self.kind == other.kind && self.file == other.file
    }
    pub fn description(&self) -> String {
        format!(
            "{} · {}",
            path_from_bytes(&self.worktree).display(),
            match self.kind {
                Kind::Conflict => format!("Conflict: {}", path_from_bytes(&self.file).display()),
                Kind::RebaseMessage => "Rebase message".into(),
                Kind::RewriteSeries => "Original commit-series review record".into(),
            }
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Draft {
    pub key: Key,
    pub text: String,
    pub updated: u64,
}

#[derive(Serialize, Deserialize)]
struct Store {
    version: u32,
    drafts: Vec<Draft>,
}

pub fn store_path() -> Result<PathBuf> {
    Ok(preferences::settings_path()?.with_file_name("recovery-drafts.json"))
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

pub fn validate_budget(entries: &HashMap<Key, Draft>) -> Result<()> {
    ensure!(
        entries.len() <= MAX_ENTRIES,
        "Recovery storage holds at most 256 drafts. Copy and discard older drafts to free space"
    );
    ensure!(
        entries.values().all(|draft| draft.text.len() <= MAX_TEXT),
        "Recovery text exceeds 2 MiB"
    );
    let total = entries.values().fold(0usize, |total, draft| {
        total.saturating_add(draft.text.len())
    });
    ensure!(
        total <= MAX_BYTES,
        "Recovery storage is full. Copy and discard older drafts to free space"
    );
    Ok(())
}

/// Full source validation stays on the persistence worker. Typing only checks
/// entry counts and recorded byte lengths, never rescans other drafts' text.
pub fn validate(entries: &HashMap<Key, Draft>) -> Result<()> {
    validate_budget(entries)?;
    for draft in entries.values() {
        ensure!(
            path_from_bytes(&draft.key.worktree).is_absolute(),
            "Recovery worktree must be absolute"
        );
        ensure!(
            draft.key.worktree.len() <= 32768
                && draft.key.file.len() <= 32768
                && draft.key.source.len() == 64
                && draft.key.source.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid recovery identity"
        );
        ensure!(
            draft.text.len() <= MAX_TEXT && !draft.text.contains('\0'),
            "Recovery text exceeds 2 MiB or contains NUL bytes"
        );
    }
    Ok(())
}

pub fn read(path: &Path) -> Result<HashMap<Key, Draft>> {
    let bytes = match preferences::read_store(path, (MAX_BYTES * 2) as u64) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(error.into()),
    };
    let store: Store = serde_json::from_slice(&bytes).context("Read recovery draft store")?;
    ensure!(
        store.version == 1,
        "Unsupported recovery draft version; existing storage was preserved"
    );
    ensure!(
        store.drafts.len() <= MAX_ENTRIES,
        "Too many stored recovery drafts"
    );
    let entries = store
        .drafts
        .into_iter()
        .map(|draft| (draft.key.clone(), draft))
        .collect();
    validate(&entries)?;
    Ok(entries)
}

pub fn save_at(batch: &Batch, path: &Path) -> Result<()> {
    let mut entries = read(path)?;
    for (key, draft) in batch {
        if let Some(draft) = draft {
            entries.insert(key.clone(), draft.clone());
        } else {
            entries.remove(key);
        }
    }
    validate(&entries)?;
    let mut drafts: Vec<_> = entries.into_values().collect();
    drafts.sort_by(|a, b| {
        a.updated
            .cmp(&b.updated)
            .then_with(|| a.key.worktree.cmp(&b.key.worktree))
            .then_with(|| a.key.source.cmp(&b.key.source))
    });
    let bytes = serde_json::to_vec(&Store { version: 1, drafts })?;
    ensure!(
        bytes.len() <= MAX_BYTES * 2,
        "Encoded recovery drafts exceed their storage limit"
    );
    preferences::atomic_write(path, &bytes)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SavedRewrite {
    pub branch: String,
    pub base: String,
    pub original: String,
}

/// What the rewritten-series review says where [`latest_rewrite`] finds no
/// capture: no rebase started in GitTurtle captured a series for the
/// worktree. Edit local commits… stands beside it.
pub const NO_CAPTURE: &str =
    "No rewritten series for this worktree · A rebase started in Edit local commits captures one";

/// The series the latest rebase started in GitTurtle in the worktree at
/// `root` captured, if one did.
pub fn latest_rewrite(root: &Path) -> Result<Option<SavedRewrite>> {
    let entries = read(&store_path()?)?;
    let Some(draft) = entries
        .values()
        .filter(|draft| {
            draft.key.worktree == path_bytes(root) && draft.key.kind == Kind::RewriteSeries
        })
        .max_by_key(|draft| draft.updated)
    else {
        return Ok(None);
    };
    Ok(Some(serde_json::from_str(&draft.text)?))
}
