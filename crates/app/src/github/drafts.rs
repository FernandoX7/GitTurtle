//! Separate, bounded non-secret local collaboration drafts. Never overwrite an
//! unsupported/corrupt store and never discard a draft after an uncertain write.
use super::conversations::CapturedThread;
use super::*;
use sha2::{Digest, Sha256};
use std::path::Path;

const VERSION: u32 = 1;
const MAX_STORE: usize = 16 * 1024 * 1024;
const MAX_DRAFTS: usize = 128;
const MAX_ATTEMPTS_BYTES: usize = 1024 * 1024;
const MAX_ATTEMPTS: usize = 128;
const MAX_REPLY_RECEIPTS: usize = 128;
/// Exact text tied to the authoritative conversation and connected account that
/// were captured when composition began. A changed target gets its own entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ReplyDraft {
    pub target: CapturedThread,
    pub body: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum Draft {
    Pull(NewPull),
    Review(ReviewDraft),
    Reply(ReplyDraft),
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
            // Include every captured identity, with JSON escaping so opaque
            // provider IDs and paths cannot collide through separators.
            Self::Reply(reply) => format!("reply:{}", json!(&reply.target)),
        }
    }
    #[cfg(test)]
    pub fn text(&self) -> &str {
        match self {
            Self::Pull(p) => &p.body,
            Self::Review(p) => &p.body,
            Self::Reply(reply) => &reply.body,
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
                    if p.discussion {
                        "Discussion comment"
                    } else {
                        p.event.label()
                    },
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
            Self::Reply(reply) => {
                let target = &reply.target;
                format!(
                    "Reply · github.com/{} #{}\nAccount {} ({})\nRepository ID {}\nPull request ID {}\nThread ID {}\nOpening comment ID {}\nOriginal commit {}\nPath {}\nHead {}\nBase {}\n\n{}",
                    target.pull.repository.label(),
                    target.pull.number,
                    target.account,
                    target.account_id,
                    target.repository_id,
                    target.pull_id,
                    target.thread_id,
                    target.root_comment_id.as_deref().unwrap_or("Unavailable"),
                    target
                        .original_commit_id
                        .as_deref()
                        .unwrap_or("Unavailable"),
                    target.path,
                    target.pull.head.sha,
                    target.pull.base.sha,
                    reply.body,
                )
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
    let bytes = match crate::preferences::read_store(path, MAX_STORE as u64) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Store {
                version: VERSION,
                entries: vec![],
            });
        }
        Err(error) => return Err(error.into()),
    };
    let store: Store = serde_json::from_slice(&bytes)
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
#[cfg(test)]
pub(crate) fn save(draft: Draft) -> Result<()> {
    save_at(&path()?, draft)
}
pub(crate) fn save_batch(batch: &std::collections::HashMap<String, Draft>) -> Result<()> {
    save_batch_at(&path()?, batch.values().cloned())
}
#[cfg(test)]
fn save_at(path: &Path, draft: Draft) -> Result<()> {
    save_batch_at(path, [draft])
}
fn save_batch_at(path: &Path, drafts: impl IntoIterator<Item = Draft>) -> Result<()> {
    let mut store = load_at(path)?;
    let drafts: Vec<_> = drafts.into_iter().collect();
    // An empty reply is queued through the same serialized saver as typing.
    // Removing it here cannot race an earlier accepted save, and completed
    // conversations do not consume the allowance for unfinished work. Apply
    // removals first so a coalesced replacement can reuse a just-freed slot.
    let removals: std::collections::HashSet<_> = drafts
        .iter()
        .filter(|draft| matches!(draft, Draft::Reply(reply) if reply.body.is_empty()))
        .map(Draft::key)
        .collect();
    store
        .entries
        .retain(|entry| !removals.contains(&entry.key()));
    for draft in drafts {
        if matches!(&draft, Draft::Reply(reply) if reply.body.is_empty()) {
            continue;
        }
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
    }
    let bytes = serde_json::to_vec_pretty(&store)?;
    ensure!(
        bytes.len() <= MAX_STORE,
        "GitHub drafts exceed the 16 MiB limit. Existing drafts were preserved."
    );
    crate::preferences::atomic_write(path, &bytes)
}
pub(crate) fn remove_exact(draft: &Draft) -> Result<()> {
    remove_exact_at(&path()?, draft)
}
fn remove_exact_at(path: &Path, draft: &Draft) -> Result<()> {
    let mut store = load_at(path)?;
    let submitted = serde_json::to_value(draft)?;
    store
        .entries
        .retain(|entry| serde_json::to_value(entry).ok().as_ref() != Some(&submitted));
    crate::preferences::atomic_write(path, &serde_json::to_vec_pretty(&store)?)
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
    fn reply(body: &str) -> ReplyDraft {
        ReplyDraft {
            target: CapturedThread {
                pull: super::super::review::fixture_pull(false)
                    .capture(Repository::parse("gitturtle-fixture/native-review").unwrap()),
                account: "offline-reviewer".into(),
                account_id: "U_fixture_reviewer".into(),
                repository_id: "R_fixture_repository".into(),
                pull_id: "PR_fixture_42".into(),
                thread_id: "PRRT_fixture_7".into(),
                path: "docs/界面:review.md".into(),
                root_comment_id: Some("PRRC_fixture_70".into()),
                original_commit_id: Some("a".repeat(40)),
            },
            body: body.into(),
        }
    }
    #[test]
    fn reply_restart_preserves_exact_text_and_all_original_identities() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("drafts.json");
        // Existing version-one creation drafts remain readable when replies
        // are introduced; no destructive migration is needed.
        save_at(&path, draft("Existing description\n")).unwrap();
        let text = "\n  Reply with exact spacing\r\n界面 🐢\n\n";
        let captured = reply(text);
        save_at(&path, Draft::Reply(captured.clone())).unwrap();
        let reopened = load_at(&path).unwrap();
        assert_eq!(reopened.entries.len(), 2);
        assert_eq!(reopened.entries[0].text(), "Existing description\n");
        let Draft::Reply(restored) = &reopened.entries[1] else {
            panic!("reply expected");
        };
        assert_eq!(restored.target, captured.target);
        assert_eq!(restored.body, text);
        let recovery = reopened.entries[1].recovery_text();
        for identity in [
            captured.target.pull.repository.label(),
            format!("#{}", captured.target.pull.number),
            captured.target.account,
            captured.target.account_id,
            captured.target.repository_id,
            captured.target.pull_id,
            captured.target.thread_id,
            captured.target.path,
            captured.target.root_comment_id.unwrap(),
            captured.target.original_commit_id.unwrap(),
            captured.target.pull.head.sha,
            captured.target.pull.base.sha,
        ] {
            assert!(recovery.contains(&identity), "missing identity: {identity}");
        }
        assert!(recovery.ends_with(text));
    }

    #[test]
    fn changed_reply_targets_remain_separately_recoverable() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("drafts.json");
        let original = reply("Original reply\n");
        save_at(&path, Draft::Reply(original.clone())).unwrap();
        let mutations: &[fn(&mut CapturedThread)] = &[
            |target| target.pull.repository.owner = "another-owner".into(),
            |target| target.pull.repository.name = "another-repository".into(),
            |target| target.pull.number += 1,
            |target| target.pull.head.sha = "b".repeat(40),
            |target| target.pull.base.sha = "c".repeat(40),
            |target| target.account = "another-reviewer".into(),
            |target| target.account_id = "U_another_reviewer".into(),
            |target| target.repository_id = "R_recreated_repository".into(),
            |target| target.pull_id = "PR_another_pull".into(),
            |target| target.thread_id = "PRRT_another_thread".into(),
            |target| target.path = "docs/renamed-review.md".into(),
            |target| target.root_comment_id = None,
            |target| target.original_commit_id = None,
        ];
        for (index, mutate) in mutations.iter().enumerate() {
            let mut changed = original.clone();
            mutate(&mut changed.target);
            changed.body = format!("Changed context {index}\n");
            save_at(&path, Draft::Reply(changed)).unwrap();
        }
        let reopened = load_at(&path).unwrap();
        assert_eq!(reopened.entries.len(), mutations.len() + 1);
        assert_eq!(reopened.entries[0].text(), original.body);
        // Continuing the original conversation replaces its body only.
        let mut edited = original.clone();
        edited.body = "Continued original reply\n".into();
        save_at(&path, Draft::Reply(edited)).unwrap();
        let reopened = load_at(&path).unwrap();
        assert_eq!(reopened.entries.len(), mutations.len() + 1);
        assert_eq!(reopened.entries[0].text(), "Continued original reply\n");
        assert_eq!(reopened.entries[1].text(), "Changed context 0\n");
    }

    #[test]
    fn reply_identity_separators_cannot_overwrite_another_conversation() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("drafts.json");
        let mut first = reply("First conversation");
        first.target.thread_id = "thread:one".into();
        first.target.path = "two:notes.md".into();
        let mut second = first.clone();
        second.target.thread_id = "thread".into();
        second.target.path = "one:two:notes.md".into();
        second.body = "Second conversation".into();
        save_at(&path, Draft::Reply(first)).unwrap();
        save_at(&path, Draft::Reply(second)).unwrap();
        let reopened = load_at(&path).unwrap();
        assert_eq!(reopened.entries.len(), 2);
        assert_eq!(reopened.entries[0].text(), "First conversation");
        assert_eq!(reopened.entries[1].text(), "Second conversation");
    }

    #[test]
    fn failed_reply_batch_preserves_existing_text_and_invalid_store() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("drafts.json");
        let original = reply("Keep this reply\n");
        save_at(&path, Draft::Reply(original.clone())).unwrap();
        let before = std::fs::read(&path).unwrap();
        let mut replacements = vec![Draft::Reply(reply("Uncommitted replacement"))];
        replacements.extend((0..MAX_DRAFTS).map(|index| {
            let mut next = original.clone();
            next.target.thread_id = format!("PRRT_new_{index}");
            Draft::Reply(next)
        }));
        assert!(save_batch_at(&path, replacements).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let invalid = b"{\"version\":999,\"entries\":[]}";
        std::fs::write(&path, invalid).unwrap();
        assert!(save_at(&path, Draft::Reply(original)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), invalid);
    }

    #[test]
    fn discarding_a_captured_reply_preserves_a_newer_edit() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("drafts.json");
        let old = Draft::Reply(reply("Earlier reply\n"));
        let edited = Draft::Reply(reply("Last characters before navigating\n"));
        save_at(&path, old.clone()).unwrap();
        save_at(&path, edited.clone()).unwrap();
        remove_exact_at(&path, &old).unwrap();
        assert_eq!(load_at(&path).unwrap().entries[0].text(), edited.text());
        remove_exact_at(&path, &edited).unwrap();
        assert!(load_at(&path).unwrap().entries.is_empty());
    }

    #[test]
    fn confirmed_reply_completion_replaces_only_the_submitted_context() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("drafts.json");
        let original = reply("Successfully posted reply\n");
        let mut other = reply("Unfinished reply in another conversation\n");
        other.target.thread_id = "PRRT_another_thread".into();
        save_at(&path, Draft::Reply(other.clone())).unwrap();
        let executor = crate::operations::SerialExecutor::new("reply-completion-draft-test");
        let saver = crate::commit_drafts::CoalescingSaver::<String, Draft>::default();
        let (started, waiting) = std::sync::mpsc::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let mut first_save = true;
        let destination = path.clone();
        let pending = Draft::Reply(original.clone());
        let response = saver
            .queue_with(&executor, pending.key(), pending, move |batch| {
                if first_save {
                    first_save = false;
                    started.send(())?;
                    gate.recv()?;
                }
                save_batch_at(&destination, batch.values().cloned())
            })
            .unwrap();
        waiting
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        // Provider success arrives while the previous typing snapshot is
        // already being saved. Its same-target clear must finish afterward.
        let completed = Draft::Reply(ReplyDraft {
            body: String::new(),
            ..original
        });
        assert!(
            saver
                .queue_with(&executor, completed.key(), completed, |_| {
                    panic!("completion must join the accepted save")
                })
                .is_none()
        );
        release.send(()).unwrap();
        futures::executor::block_on(response).unwrap().unwrap();
        let reopened = load_at(&path).unwrap();
        assert_eq!(reopened.entries.len(), 1);
        assert_eq!(reopened.entries[0].text(), other.body);
    }

    #[test]
    fn completing_a_reply_frees_capacity_for_a_coalesced_new_draft() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("drafts.json");
        save_batch_at(
            &path,
            (0..MAX_DRAFTS).map(|index| {
                let mut next = reply("Existing reply");
                next.target.thread_id = format!("PRRT_thread_{index}");
                Draft::Reply(next)
            }),
        )
        .unwrap();
        let mut completed = reply("");
        completed.target.thread_id = "PRRT_thread_0".into();
        let new = Draft::Reply(reply("New conversation\n"));
        let new_key = new.key();
        save_batch_at(&path, [new, Draft::Reply(completed)]).unwrap();
        let reopened = load_at(&path).unwrap();
        assert_eq!(reopened.entries.len(), MAX_DRAFTS);
        assert!(reopened.entries.iter().any(|entry| entry.key() == new_key));
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
    fn a_failed_coalesced_batch_preserves_every_previous_draft() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("drafts.json");
        let create = |index: usize, text: &str| {
            let Draft::Pull(mut pull) = draft(text) else {
                unreachable!()
            };
            pull.head = format!("feature-{index}");
            Draft::Pull(pull)
        };
        save_batch_at(&path, (0..127).map(|index| create(index, "Original text"))).unwrap();
        let original = std::fs::read(&path).unwrap();
        assert!(
            save_batch_at(
                &path,
                [
                    create(0, "An update before the overflow"),
                    create(127, "A new draft before the overflow"),
                    create(128, "This draft exceeds the entry limit"),
                ]
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(load_at(&path).unwrap().entries.len(), 127);
        save_batch_at(
            &path,
            [create(0, "Updated exact text\n"), create(127, "New text")],
        )
        .unwrap();
        let restored = load_at(&path).unwrap();
        assert_eq!(restored.entries.len(), 128);
        assert_eq!(restored.entries[0].text(), "Updated exact text\n");
        assert_eq!(restored.entries[127].text(), "New text");
    }

    #[test]
    fn attempt_byte_and_json_escape_overflow_cannot_poison_the_existing_store() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("attempts.json");
        let attempts: Vec<_> = (0..127)
            .map(|index| Attempt {
                destination: format!("{index:03}{}", "d".repeat(4093)),
                outcome: "x".repeat(4096),
                completed_reply_sha256: None,
            })
            .collect();
        let original = serde_json::to_vec_pretty(&attempts).unwrap();
        assert!(original.len() <= MAX_ATTEMPTS_BYTES);
        std::fs::write(&path, &original).unwrap();
        // Both inputs satisfy the individual 4 KiB limits. A 128th entry and
        // escaped control characters nevertheless exceed the encoded budget.
        for (destination, outcome) in [
            ("new".repeat(1365), "x".repeat(4096)),
            (attempts[0].destination.clone(), "\u{1}".repeat(4096)),
        ] {
            assert!(record_attempt_at(&path, destination, outcome).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), original);
            assert_eq!(attempts_at(&path).unwrap().len(), 127);
        }
        record_attempt_at(
            &path,
            attempts[0].destination.clone(),
            "Observed success".into(),
        )
        .unwrap();
        assert_eq!(attempts_at(&path).unwrap()[0].outcome, "Observed success");
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
    fn earlier_completed_reply_stays_suppressed_after_later_success_and_failed_saves() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("drafts.json");
        let attempts = temp.path().join("attempts.json");
        let completed = reply("Exact reply accepted by GitHub\n");
        save_at(&path, Draft::Reply(completed.clone())).unwrap();
        record_reply_outcome_at(
            &attempts,
            completed.target.destination(),
            "GitHub accepted this reply".into(),
            Some(&completed),
        )
        .unwrap();
        let before = std::fs::read(&path).unwrap();
        let cleared = ReplyDraft {
            body: String::new(),
            ..completed.clone()
        };
        // Another coalesced form causes the entire atomic cleanup batch to be
        // refused. The old reply stays on disk, but its success proof is safe.
        let mut too_large = reply(&"x".repeat(MAX_STORE));
        too_large.target.thread_id = "PRRT_oversize".into();
        assert!(save_batch_at(&path, [Draft::Reply(cleared), Draft::Reply(too_large)]).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let later = reply("A later reply also accepted by GitHub\n");
        let mut failed_save = vec![Draft::Reply(later.clone())];
        failed_save.extend((0..MAX_DRAFTS).map(|index| {
            let mut other = reply("Another pending draft");
            other.target.thread_id = format!("PRRT_new_{index}");
            Draft::Reply(other)
        }));
        assert!(save_batch_at(&path, failed_save).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        record_attempt_at(
            &attempts,
            later.target.destination(),
            "Second reply started".into(),
        )
        .unwrap();
        record_reply_outcome_at(
            &attempts,
            later.target.destination(),
            "GitHub accepted the later reply".into(),
            Some(&later),
        )
        .unwrap();
        let Draft::Reply(restored) = load_at(&path).unwrap().entries.remove(0) else {
            panic!("reply expected");
        };
        assert!(reply_was_completed(
            &attempts_at(&attempts).unwrap(),
            &restored
        ));
        assert!(reply_was_completed(
            &attempts_at(&attempts).unwrap(),
            &later
        ));
        let encoded = std::fs::read_to_string(&attempts).unwrap();
        assert!(!encoded.contains("Exact reply accepted by GitHub"));
    }

    #[test]
    fn legacy_single_reply_receipt_migrates_without_retiring_its_proof() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("attempts.json");
        let original = reply("Previously sent reply\n");
        let destination = original.target.destination();
        std::fs::write(
            &path,
            serde_json::to_vec(&json!([{
                "destination": destination,
                "outcome": "Accepted",
                "completed_reply_sha256": reply_fingerprint(&original),
            }]))
            .unwrap(),
        )
        .unwrap();
        assert!(reply_was_completed(&attempts_at(&path).unwrap(), &original));
        let next = reply("Next sent reply\n");
        record_reply_outcome_at(&path, destination, "Accepted next".into(), Some(&next)).unwrap();
        let reopened = attempts_at(&path).unwrap();
        assert!(reply_was_completed(&reopened, &original));
        assert!(reply_was_completed(&reopened, &next));
        assert_eq!(
            reopened[0].completed_reply_sha256.as_ref().unwrap().len(),
            2
        );
    }

    #[test]
    fn reply_receipt_limit_refuses_growth_without_forgetting_completed_text() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("attempts.json");
        let replies: Vec<_> = (0..MAX_REPLY_RECEIPTS)
            .map(|index| reply(&format!("Accepted reply {index}\n")))
            .collect();
        let destination = replies[0].target.destination();
        let original = serde_json::to_vec(&vec![Attempt {
            destination: destination.clone(),
            outcome: "Accepted".into(),
            completed_reply_sha256: Some(replies.iter().map(reply_fingerprint).collect()),
        }])
        .unwrap();
        std::fs::write(&path, &original).unwrap();
        assert!(
            record_reply_outcome_at(
                &path,
                destination.clone(),
                "A further reply was accepted".into(),
                Some(&reply("Beyond the receipt allowance\n")),
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let reopened = attempts_at(&path).unwrap();
        assert!(
            replies
                .iter()
                .all(|reply| reply_was_completed(&reopened, reply))
        );
        // Re-observing an already recorded success does not consume capacity.
        record_reply_outcome_at(&path, destination, "Accepted".into(), Some(&replies[0])).unwrap();
        assert_eq!(
            attempts_at(&path).unwrap()[0]
                .completed_reply_sha256
                .as_ref()
                .unwrap()
                .len(),
            MAX_REPLY_RECEIPTS
        );
    }

    #[test]
    fn new_attempts_cannot_evict_unretired_reply_receipts() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("attempts.json");
        let protected = reply("Accepted while draft cleanup failed\n");
        let attempts: Vec<_> = (0..MAX_ATTEMPTS)
            .map(|index| Attempt {
                destination: format!("Destination {index}"),
                outcome: "Accepted".into(),
                completed_reply_sha256: Some(vec![reply_fingerprint(&protected)]),
            })
            .collect();
        let original = serde_json::to_vec(&attempts).unwrap();
        std::fs::write(&path, &original).unwrap();
        assert!(record_attempt_at(&path, "New destination".into(), "Pending".into()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        // An ordinary attempt without reply evidence remains eligible for the
        // existing latest-destination retention policy.
        let mut with_disposable = attempts;
        with_disposable[MAX_ATTEMPTS - 1].completed_reply_sha256 = None;
        std::fs::write(&path, serde_json::to_vec(&with_disposable).unwrap()).unwrap();
        record_attempt_at(&path, "New destination".into(), "Pending".into()).unwrap();
        let reopened = attempts_at(&path).unwrap();
        assert_eq!(reopened.len(), MAX_ATTEMPTS);
        assert_eq!(reopened[0].destination, "Destination 0");
        assert!(reply_was_completed(&reopened, &protected));
        assert_eq!(reopened.last().unwrap().destination, "New destination");
    }

    #[test]
    fn completion_proof_is_exact_and_survives_later_uncertain_and_resolution_attempts() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("attempts.json");
        let completed = reply("Exact sent reply\n");
        let destination = completed.target.destination();
        record_reply_outcome_at(
            &path,
            destination.clone(),
            "Accepted".into(),
            Some(&completed),
        )
        .unwrap();
        for outcome in [
            "Submission started; inspect GitHub",
            "Response was lost; outcome uncertain",
            "Conversation resolved",
        ] {
            record_attempt_at(&path, destination.clone(), outcome.into()).unwrap();
            let reopened = attempts_at(&path).unwrap();
            assert_eq!(reopened.len(), 1);
            assert_eq!(reopened[0].outcome, outcome);
            assert!(reply_was_completed(&reopened, &completed));
        }
        let reopened = attempts_at(&path).unwrap();
        let mut changed = completed.clone();
        changed.body.push(' ');
        assert!(!reply_was_completed(&reopened, &changed));
        changed = completed.clone();
        changed.target.account_id = "U_another_account".into();
        assert!(!reply_was_completed(&reopened, &changed));
        changed = completed.clone();
        changed.target.pull.head.sha = "d".repeat(40);
        assert!(!reply_was_completed(&reopened, &changed));
        changed = completed;
        changed.target.thread_id = "PRRT_another_thread".into();
        assert!(!reply_was_completed(&reopened, &changed));
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
            discussion: false,
        };
        let mut moved = old.clone();
        moved.pull.head.sha = "3".repeat(40);
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("drafts.json");
        save_at(&path, Draft::Review(old)).unwrap();
        save_at(&path, Draft::Review(moved)).unwrap();
        assert_eq!(load_at(&path).unwrap().entries.len(), 2);
    }
    #[test]
    fn collected_ranges_unfinished_text_and_discussion_mode_survive_restart() {
        let mut review = ReviewDraft {
            pull: super::super::review::fixture_pull(false)
                .capture(Repository::parse("gitturtle-fixture/native-review").unwrap()),
            body: "  Summary\n".into(),
            event: ReviewEvent::Comment,
            comments: vec![LineComment {
                path: "docs/界面.md".into(),
                line: 8,
                side: DiffSide::Right,
                start_line: Some(5),
                start_side: Some(DiffSide::Right),
                body: " Exact inline text\n\n".into(),
            }],
            composing: Some(LineComment {
                path: "src/session.rs".into(),
                line: 22,
                side: DiffSide::Left,
                start_line: None,
                start_side: None,
                body: "Unfinished before switching repositories\n".into(),
            }),
            discussion: false,
        };
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("reviews.json");
        let draft = Draft::Review(review.clone());
        save_at(&path, draft.clone()).unwrap();
        let loaded = load_at(&path).unwrap().entries.remove(0);
        assert_eq!(
            serde_json::to_value(&loaded).unwrap(),
            serde_json::to_value(&draft).unwrap()
        );
        let recovery = loaded.recovery_text();
        assert!(recovery.contains("After 5–8"));
        assert!(recovery.contains(" Exact inline text\n\n"));
        assert!(recovery.contains("Unfinished before switching repositories\n"));
        review.comments.clear();
        review.composing = None;
        review.discussion = true;
        save_at(&path, Draft::Review(review)).unwrap();
        let Draft::Review(loaded) = load_at(&path).unwrap().entries.remove(0) else {
            panic!("review expected")
        };
        assert!(loaded.discussion);
        assert_eq!(loaded.body, "  Summary\n");
    }
}

/// Written only by the operation executor, separately from coalesced text
/// drafts. A recorded pending attempt survives window loss or process exit.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Attempt {
    pub destination: String,
    pub outcome: String,
    /// Retain every confirmed reply until cleanup can be proved durable. The
    /// field keeps its original name and accepts the legacy single digest.
    #[serde(
        default,
        deserialize_with = "deserialize_reply_receipts",
        skip_serializing_if = "Option::is_none"
    )]
    pub completed_reply_sha256: Option<Vec<String>>,
}
fn deserialize_reply_receipts<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Encoding {
        Single(String),
        Multiple(Vec<String>),
    }
    Ok(
        Option::<Encoding>::deserialize(deserializer)?.map(|encoded| match encoded {
            Encoding::Single(receipt) => vec![receipt],
            Encoding::Multiple(receipts) => receipts,
        }),
    )
}
fn attempts_path() -> Result<PathBuf> {
    Ok(path()?.with_file_name("github-attempts.json"))
}
pub(crate) fn attempts() -> Result<Vec<Attempt>> {
    attempts_at(&attempts_path()?)
}
fn attempts_at(path: &Path) -> Result<Vec<Attempt>> {
    let bytes = match crate::preferences::read_store(path, MAX_ATTEMPTS_BYTES as u64) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(error.into()),
    };
    let attempts: Vec<Attempt> = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow!("GitHub attempt record is invalid and was preserved"))?;
    ensure!(
        attempts.len() <= MAX_ATTEMPTS,
        "GitHub attempt record exceeds its entry limit"
    );
    ensure!(
        attempts.iter().all(
            |attempt| attempt
                .completed_reply_sha256
                .as_ref()
                .is_none_or(|receipts| receipts.len() <= MAX_REPLY_RECEIPTS
                    && receipts.iter().all(|receipt| receipt.len() == 64
                        && receipt.bytes().all(|byte| byte.is_ascii_hexdigit())))
        ),
        "GitHub reply receipts are invalid or exceed their limit; the attempt store was preserved"
    );
    Ok(attempts)
}
pub(crate) fn record_attempt(destination: String, outcome: String) -> Result<()> {
    record_attempt_at(&attempts_path()?, destination, outcome)
}
fn record_attempt_at(path: &Path, destination: String, outcome: String) -> Result<()> {
    record_reply_outcome_at(path, destination, outcome, None)
}
pub(crate) fn record_reply_outcome(
    destination: String,
    outcome: String,
    completed: Option<&ReplyDraft>,
) -> Result<()> {
    record_reply_outcome_at(&attempts_path()?, destination, outcome, completed)
}
fn reply_fingerprint(reply: &ReplyDraft) -> String {
    // Serde's fixed struct layout includes every captured identity and exact
    // body byte. Persist only a bounded digest, never a second copy of the text.
    format!("{:x}", Sha256::digest(json!(reply).to_string().as_bytes()))
}
pub(crate) fn reply_was_completed(attempts: &[Attempt], reply: &ReplyDraft) -> bool {
    let fingerprint = reply_fingerprint(reply);
    attempts.iter().any(|attempt| {
        attempt
            .completed_reply_sha256
            .as_ref()
            .is_some_and(|receipts| receipts.contains(&fingerprint))
    })
}
fn record_reply_outcome_at(
    path: &Path,
    destination: String,
    outcome: String,
    completed: Option<&ReplyDraft>,
) -> Result<()> {
    ensure!(
        destination.len() <= 4096 && outcome.len() <= 4096,
        "GitHub attempt identity exceeds its limit"
    );
    let completed_reply_sha256 = completed.map(reply_fingerprint);
    let mut attempts = attempts_at(path)?;
    if let Some(attempt) = attempts
        .iter_mut()
        .find(|attempt| attempt.destination == destination)
    {
        attempt.outcome = outcome;
        // A later reply can succeed while every preference save still fails.
        // Never replace earlier proof: its original text may remain on disk.
        if let Some(receipt) = completed_reply_sha256 {
            let receipts = attempt.completed_reply_sha256.get_or_insert_default();
            if !receipts.contains(&receipt) {
                ensure!(
                    receipts.len() < MAX_REPLY_RECEIPTS,
                    "This conversation reached its 128 completed-reply receipt limit. Prior receipts were preserved; inspect GitHub before sending again."
                );
                receipts.push(receipt);
            }
        }
    } else {
        if attempts.len() == MAX_ATTEMPTS {
            let disposable = attempts
                .iter()
                .position(|attempt| {
                    attempt
                        .completed_reply_sha256
                        .as_ref()
                        .is_none_or(Vec::is_empty)
                })
                .ok_or_else(|| anyhow!(
                    "The GitHub attempt store is full of completed-reply receipts. Existing receipts were preserved; this attempt was not recorded."
                ))?;
            attempts.remove(disposable);
        }
        attempts.push(Attempt {
            destination,
            outcome,
            completed_reply_sha256: completed_reply_sha256.map(|receipt| vec![receipt]),
        });
    }
    let bytes = serde_json::to_vec_pretty(&attempts)?;
    ensure!(
        bytes.len() <= MAX_ATTEMPTS_BYTES,
        "GitHub attempt record exceeds its 1 MiB limit. Existing outcomes were preserved; this attempt was not recorded."
    );
    crate::preferences::atomic_write(path, &bytes)
}
