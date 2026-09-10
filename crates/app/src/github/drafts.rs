//! Separate, bounded non-secret local collaboration drafts. Never overwrite an
//! unsupported/corrupt store and never discard a draft after an uncertain write.
use super::*;
use std::path::Path;

const VERSION: u32 = 1;
const MAX_STORE: usize = 16 * 1024 * 1024;
const MAX_DRAFTS: usize = 128;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum Draft {
    Pull(NewPull),
    Review(ReviewDraft),
}
impl Draft {
    pub fn key(&self) -> String {
        match self {
            Self::Pull(p) => format!("create:{}:{}:{}", p.repository.label(), p.head, p.base),
            Self::Review(p) => format!(
                "review:{}:{}:{}:{}",
                p.pull.repository.label(),
                p.pull.number,
                p.pull.head.sha,
                p.pull.base.sha
            ),
        }
    }
    #[cfg(test)]
    pub fn text(&self) -> &str {
        match self {
            Self::Pull(p) => &p.body,
            Self::Review(p) => &p.body,
        }
    }
    pub fn recovery_text(&self) -> String {
        match self {
            Self::Pull(p) => format!("{}\n\n{}", p.title, p.body),
            Self::Review(p) => {
                let mut text = format!(
                    "{} #{}\nHead {}\nBase {}\n{}\n\n{}",
                    p.pull.repository.label(),
                    p.pull.number,
                    p.pull.head.sha,
                    p.pull.base.sha,
                    p.event.label(),
                    p.body
                );
                for comment in p.comments.iter().chain(p.composing.iter()) {
                    text.push_str(&format!(
                        "\n\n{}\n{}",
                        super::review::position_label(comment),
                        comment.body
                    ));
                }
                text
            }
        }
    }
}
#[derive(Default, Serialize, Deserialize)]
struct Store {
    version: u32,
    entries: Vec<Draft>,
}
pub(crate) fn path() -> Result<PathBuf> {
    Ok(crate::preferences::settings_path()?.with_file_name("github-drafts.json"))
}
fn load_at(path: &Path) -> Result<Store> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Store {
                version: VERSION,
                entries: vec![],
            });
        }
        Err(error) => return Err(error.into()),
    };
    ensure!(
        metadata.file_type().is_file() && metadata.len() <= MAX_STORE as u64,
        "GitHub draft store is unsafe or exceeds its size limit"
    );
    let store: Store = serde_json::from_slice(&std::fs::read(path)?)
        .map_err(|_| anyhow!("GitHub draft store is invalid; it was preserved for recovery"))?;
    ensure!(
        store.version == VERSION && store.entries.len() <= MAX_DRAFTS,
        "GitHub draft store version or size is unsupported; it was preserved for recovery"
    );
    Ok(store)
}
pub(crate) fn load() -> Result<Vec<Draft>> {
    Ok(load_at(&path()?)?.entries)
}
pub(crate) fn save(draft: Draft) -> Result<()> {
    save_at(&path()?, draft)
}
pub(crate) fn save_batch(batch: &std::collections::HashMap<String, Draft>) -> Result<()> {
    for draft in batch.values() {
        save(draft.clone())?;
    }
    Ok(())
}
fn save_at(path: &Path, draft: Draft) -> Result<()> {
    let mut store = load_at(path)?;
    let key = draft.key();
    if let Some(entry) = store.entries.iter_mut().find(|entry| entry.key() == key) {
        *entry = draft;
    } else {
        ensure!(
            store.entries.len() < MAX_DRAFTS,
            "GitHub draft store is full. Explicitly discard an older draft before saving another."
        );
        store.entries.push(draft);
    }
    let bytes = serde_json::to_vec_pretty(&store)?;
    ensure!(
        bytes.len() <= MAX_STORE,
        "GitHub drafts exceed the 16 MiB limit. Existing drafts were preserved."
    );
    crate::preferences::atomic_write(path, &bytes)
}
pub(crate) fn remove_exact(draft: &Draft) -> Result<()> {
    let path = path()?;
    let mut store = load_at(&path)?;
    let submitted = serde_json::to_value(draft)?;
    store
        .entries
        .retain(|entry| serde_json::to_value(entry).ok().as_ref() != Some(&submitted));
    crate::preferences::atomic_write(&path, &serde_json::to_vec_pretty(&store)?)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn draft(body: &str) -> Draft {
        Draft::Pull(NewPull {
            repository: Repository::parse("fixture/repo").unwrap(),
            title: "Title".into(),
            body: body.into(),
            head: "feature".into(),
            base: "main".into(),
            draft: true,
            expected_head: None,
            expected_base: None,
        })
    }
    #[test]
    fn exact_text_survives_reopen_and_bad_store_is_not_replaced() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("drafts.json");
        save_at(&path, draft("\n  exact text\n")).unwrap();
        assert_eq!(
            load_at(&path).unwrap().entries[0].text(),
            "\n  exact text\n"
        );
        std::fs::write(&path, b"invalid").unwrap();
        assert!(save_at(&path, draft("replacement")).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"invalid");
    }
    #[test]
    fn pending_attempt_survives_reopen_until_an_observed_outcome_is_recorded() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("attempts.json");
        record_attempt_at(
            &path,
            "fixture/repo #7".into(),
            "Pending; check GitHub".into(),
        )
        .unwrap();
        assert_eq!(
            attempts_at(&path).unwrap()[0].outcome,
            "Pending; check GitHub"
        );
        record_attempt_at(&path, "fixture/repo #7".into(), "Accepted action 91".into()).unwrap();
        let reopened = attempts_at(&path).unwrap();
        assert_eq!(reopened.len(), 1);
        assert_eq!(reopened[0].outcome, "Accepted action 91");
    }
    #[test]
    fn changed_head_keeps_older_review_recoverable() {
        let old = ReviewDraft {
            pull: CapturedPull {
                repository: Repository::parse("fixture/repo").unwrap(),
                number: 7,
                head: Revision {
                    sha: "1".repeat(40),
                    branch: "feature".into(),
                    label: String::new(),
                },
                base: Revision {
                    sha: "2".repeat(40),
                    branch: "main".into(),
                    label: String::new(),
                },
            },
            body: "Preserve this".into(),
            event: ReviewEvent::Comment,
            comments: vec![],
            composing: None,
        };
        let mut moved = old.clone();
        moved.pull.head.sha = "3".repeat(40);
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("drafts.json");
        save_at(&path, Draft::Review(old)).unwrap();
        save_at(&path, Draft::Review(moved)).unwrap();
        assert_eq!(load_at(&path).unwrap().entries.len(), 2);
    }
}

/// Written only by the operation executor, separately from coalesced text
/// drafts. A recorded pending attempt survives window loss or process exit.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Attempt {
    pub destination: String,
    pub outcome: String,
}
fn attempts_path() -> Result<PathBuf> {
    Ok(path()?.with_file_name("github-attempts.json"))
}
pub(crate) fn attempts() -> Result<Vec<Attempt>> {
    attempts_at(&attempts_path()?)
}
fn attempts_at(path: &Path) -> Result<Vec<Attempt>> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(error.into()),
    };
    ensure!(
        metadata.file_type().is_file() && metadata.len() <= 1024 * 1024,
        "GitHub attempt record is unsafe or too large"
    );
    let attempts: Vec<Attempt> = serde_json::from_slice(&std::fs::read(path)?)
        .map_err(|_| anyhow!("GitHub attempt record is invalid and was preserved"))?;
    ensure!(
        attempts.len() <= 128,
        "GitHub attempt record exceeds its entry limit"
    );
    Ok(attempts)
}
pub(crate) fn record_attempt(destination: String, outcome: String) -> Result<()> {
    record_attempt_at(&attempts_path()?, destination, outcome)
}
fn record_attempt_at(path: &Path, destination: String, outcome: String) -> Result<()> {
    ensure!(
        destination.len() <= 4096 && outcome.len() <= 4096,
        "GitHub attempt identity exceeds its limit"
    );
    let mut attempts = attempts_at(path)?;
    if let Some(attempt) = attempts
        .iter_mut()
        .find(|attempt| attempt.destination == destination)
    {
        attempt.outcome = outcome;
    } else {
        if attempts.len() == 128 {
            attempts.remove(0);
        }
        attempts.push(Attempt {
            destination,
            outcome,
        });
    }
    crate::preferences::atomic_write(path, &serde_json::to_vec_pretty(&attempts)?)
}
