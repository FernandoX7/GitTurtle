//! Recovery text is application data, never a replayable Git operation.
//! Shares the commit draft coalescer and atomic writer, but has independent
//! bounds and storage so large resolutions cannot crowd out ordinary drafts.
use crate::*;
use anyhow::{Context as _, Result, ensure};
use gpui_kit::component::{WindowExt, dialog::DialogFooter};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::Read,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_ENTRIES: usize = 256;
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_TEXT: usize = 2 * 1024 * 1024;
type Batch = HashMap<Key, Option<Draft>>;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(super) struct Key {
    worktree: Vec<u8>,
    file: Vec<u8>,
    kind: Kind,
    source: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(super) enum Kind {
    Conflict,
    RebaseMessage,
    RewriteSeries,
}

fn path_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str().as_encoded_bytes().to_vec()
}
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
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
    pub(super) fn conflict(root: &Path, file: &Path, source: String) -> Self {
        Self {
            worktree: path_bytes(root),
            file: path_bytes(file),
            kind: Kind::Conflict,
            source,
        }
    }
    pub(super) fn message(expected: &gitturtle_core::InteractiveRebaseResume) -> Self {
        Self {
            worktree: path_bytes(&expected.root),
            file: vec![],
            kind: Kind::RebaseMessage,
            source: expected.draft_identity(),
        }
    }
    fn same_location(&self, other: &Self) -> bool {
        self.worktree == other.worktree && self.kind == other.kind && self.file == other.file
    }
    fn description(&self) -> String {
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
pub(super) struct Draft {
    pub key: Key,
    pub text: String,
    updated: u64,
}

#[derive(Default)]
pub(super) struct State {
    entries: HashMap<Key, Draft>,
    saver: commit_drafts::CoalescingSaver<Key, Option<Draft>>,
    pub error: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Store {
    version: u32,
    drafts: Vec<Draft>,
}

fn store_path() -> Result<PathBuf> {
    Ok(preferences::settings_path()?.with_file_name("recovery-drafts.json"))
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

impl State {
    /// Invoked before GPUI starts. Loading never creates directories or writes.
    pub(super) fn load() -> Self {
        match store_path().and_then(|path| read(&path)) {
            Ok(entries) => Self {
                entries,
                ..Self::default()
            },
            Err(error) => Self {
                error: Some(format!("Recovery drafts could not be read: {error:#}")),
                ..Self::default()
            },
        }
    }
    pub(super) fn latest(&self, key: &Key) -> Option<Draft> {
        self.entries
            .values()
            .filter(|draft| draft.key.same_location(key))
            .max_by_key(|draft| draft.updated)
            .cloned()
    }
    pub(super) fn status(&self) -> String {
        if let Some(error) = &self.error {
            format!(
                "Draft save failed: {error}. Text remains available; Retry save or Copy saved text."
            )
        } else if self.saver.is_pending() {
            "Saving recovery draft… Keep the app open until Saved appears.".into()
        } else if self.entries.is_empty() {
            "Edits save automatically outside the repository".into()
        } else {
            "Recovery draft saved locally · no file or index write".into()
        }
    }
}

fn validate(entries: &HashMap<Key, Draft>) -> Result<()> {
    ensure!(
        entries.len() <= MAX_ENTRIES,
        "Recovery storage holds at most 256 drafts. Copy and discard older drafts to free space"
    );
    let mut total = 0usize;
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
        total = total.saturating_add(draft.text.len());
    }
    ensure!(
        total <= MAX_BYTES,
        "Recovery storage is full. Copy and discard older drafts to free space"
    );
    Ok(())
}

fn read(path: &Path) -> Result<HashMap<Key, Draft>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take((MAX_BYTES * 2 + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_BYTES * 2,
        "Recovery store exceeds its input limit"
    );
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

fn save_at(batch: &Batch, path: &Path) -> Result<()> {
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
pub(super) struct SavedRewrite {
    pub branch: String,
    pub base: String,
    pub original: String,
}

pub(super) fn latest_rewrite(root: &Path) -> Result<SavedRewrite> {
    let entries = read(&store_path()?)?;
    let draft = entries.values().filter(|draft| draft.key.worktree == path_bytes(root) && draft.key.kind == Kind::RewriteSeries).max_by_key(|draft| draft.updated).context("No captured interactive rebase is available for this worktree. Start a rebase in GitTurtle to capture its original series before rewriting")?;
    Ok(serde_json::from_str(&draft.text)?)
}

impl GitTurtle {
    /// Enqueue before the accepted Start, on the same serialized preferences
    /// writer as drafts. The operation waits for this result before calling Git.
    pub(super) fn prepare_rewrite_capture(
        &mut self,
        command: &gitturtle_core::WriteCommand,
    ) -> Option<futures::channel::oneshot::Receiver<Result<()>>> {
        let gitturtle_core::WriteCommand::InteractiveRebase(command) = command else {
            return None;
        };
        let gitturtle_core::InteractiveRebaseCommand::Start { plan, .. } = command.as_ref() else {
            return None;
        };
        let key = Key {
            worktree: path_bytes(&plan.root),
            file: vec![],
            kind: Kind::RewriteSeries,
            source: plan.review_identity(),
        };
        let capture = SavedRewrite {
            branch: plan.branch.clone(),
            base: plan.base.clone(),
            original: plan.head.clone(),
        };
        let text = serde_json::to_string(&capture).expect("string-only capture is serializable");
        let draft = Draft {
            key: key.clone(),
            text,
            updated: now(),
        };
        self.recovery_drafts
            .entries
            .insert(key.clone(), draft.clone());
        Some(self.preferences_writer.submit(move || {
            save_at(&HashMap::from([(key, Some(draft))]), &store_path()?)
                .context("Save the original series before rewriting; Git was not started")
        }))
    }
    pub(super) fn persist_recovery_draft(
        &mut self,
        key: Key,
        text: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let draft = text.map(|text| Draft {
            key: key.clone(),
            text,
            updated: now(),
        });
        if let Some(draft) = &draft {
            // Do not silently evict old or stale recoverable text when full.
            let old = self
                .recovery_drafts
                .entries
                .insert(key.clone(), draft.clone());
            if let Err(error) = validate(&self.recovery_drafts.entries) {
                if let Some(old) = old {
                    self.recovery_drafts.entries.insert(key, old);
                } else {
                    self.recovery_drafts.entries.remove(&key);
                }
                self.recovery_drafts.error = Some(format!("{error:#}"));
                cx.notify();
                return;
            }
        } else {
            self.recovery_drafts.entries.remove(&key);
        }
        let response =
            self.recovery_drafts
                .saver
                .queue_with(&self.preferences_writer, key, draft, |batch| {
                    save_at(batch, &store_path()?)
                });
        if let Some(response) = response {
            self.recovery_drafts.error = None;
            cx.spawn_in(window, async move |this, cx| {
                let result = response
                    .await
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("Save ended without confirmation")));
                let _ = this.update_in(cx, |this, window, cx| {
                    this.recovery_drafts.error = result.err().map(|error| format!("{error:#}"));
                    if let Some(view) = &this.conflict_view {
                        view.update(cx, |view, cx| {
                            view.durable_status = this.recovery_drafts.status();
                            cx.notify();
                        });
                    }
                    this.refresh_rebase_message(cx);
                    window.refresh();
                    cx.notify();
                });
            })
            .detach();
        }
        if let Some(view) = &self.conflict_view {
            view.update(cx, |view, cx| {
                view.durable_status = self.recovery_drafts.status();
                cx.notify();
            });
        }
        cx.notify();
    }

    pub(super) fn open_recovery_drafts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let owner = cx.entity().downgrade();
        let form = cx.new(|_| RecoveryList { owner });
        window.open_alert_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Saved recovery drafts")
                .width(px(720.))
                .child(form.clone())
                .footer(DialogFooter::new())
        });
    }
}

struct RecoveryList {
    owner: WeakEntity<GitTurtle>,
}
impl Render for RecoveryList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let Some(owner) = self.owner.upgrade() else {
            return div();
        };
        let mut drafts: Vec<_> = owner
            .read(cx)
            .recovery_drafts
            .entries
            .values()
            .cloned()
            .collect();
        drafts.sort_by_key(|draft| std::cmp::Reverse(draft.updated));
        let status = owner.read(cx).recovery_drafts.status();
        div().flex().flex_col().gap_3().child("Draft text is retained until you discard it, including completed, aborted, moved or removed worktrees. Copying never stages or continues an operation. Open the matching conflict or rebase message to check whether restoration is safe.")
            .child(div().text_color(rgb(p.muted)).child(status))
            .child(div().id("saved-recovery-list").max_h((window.viewport_size().height - px(280.)).max(px(120.))).overflow_y_scroll().flex().flex_col().gap_2().children(drafts.into_iter().enumerate().map(|(index, draft)| {
                let copy = draft.text.clone(); let retry = draft.clone(); let discard = draft.key.clone();
                div().p_3().border_1().border_color(rgb(p.border)).rounded(px(6.)).child(div().text_size(crate::appearance::ui_text(12.)).child(draft.key.description())).child(div().text_color(rgb(p.muted)).child(format!("{} bytes · source {}", draft.text.len(), &draft.key.source[..12])))
                    .child(div().flex().gap_2().child(button(("copy-recovery", index), "Copy exact text", "", false).on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))))
                    .child(button(("retry-recovery", index), "Retry save", "", false).on_click(cx.listener(move |this, _, window, cx| { let _ = this.owner.update(cx, |owner, cx| owner.persist_recovery_draft(retry.key.clone(), Some(retry.text.clone()), window, cx)); cx.notify(); })))
                    .child(button(("discard-recovery", index), "Discard saved draft", "", false).on_click(cx.listener(move |this, _, window, cx| { let _ = this.owner.update(cx, |owner, cx| owner.persist_recovery_draft(discard.clone(), None, window, cx)); cx.notify(); }))))
            })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    fn fixture(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "gitturtle-recovery-{}-{name}-{}",
            std::process::id(),
            now()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path.join("drafts.json")
    }
    fn key(source: char) -> Key {
        Key::conflict(
            Path::new("/canonical/worktree"),
            Path::new("conflict.txt"),
            source.to_string().repeat(64),
        )
    }
    fn draft(key: Key, text: &str) -> Draft {
        Draft {
            key,
            text: text.into(),
            updated: now(),
        }
    }
    #[test]
    fn restart_preserves_exact_bytes_and_refuses_stale_identity() {
        let path = fixture("restart");
        let key = key('a');
        let text = " Résolution 🐢\r\n\r\n# retain whitespace  \nno trailing newline";
        save_at(
            &HashMap::from([(key.clone(), Some(draft(key.clone(), text)))]),
            &path,
        )
        .unwrap();
        let state = State {
            entries: read(&path).unwrap(),
            ..State::default()
        };
        let recovered = state.latest(&key).unwrap();
        assert_eq!(recovered.text.as_bytes(), text.as_bytes());
        assert_eq!(recovered.key, key);
        let stale = Key {
            source: "b".repeat(64),
            ..key.clone()
        };
        let saved = state.latest(&stale).unwrap();
        assert_ne!(saved.key, stale);
        assert_eq!(saved.text, text);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn failed_store_is_preserved_and_other_worktrees_survive_updates() {
        let path = fixture("merge");
        let first = key('a');
        let second = Key {
            worktree: path_bytes(Path::new("/other/worktree")),
            ..first.clone()
        };
        for (key, text) in [(first.clone(), "first"), (second.clone(), "other")] {
            save_at(
                &HashMap::from([(key.clone(), Some(draft(key, text)))]),
                &path,
            )
            .unwrap();
        }
        save_at(&HashMap::from([(first.clone(), None)]), &path).unwrap();
        assert_eq!(read(&path).unwrap().get(&second).unwrap().text, "other");
        std::fs::write(&path, b"corrupt but recoverable storage").unwrap();
        assert!(
            save_at(
                &HashMap::from([(first.clone(), Some(draft(first, "new")))]),
                &path
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"corrupt but recoverable storage"
        );
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn non_utf8_worktree_and_file_names_round_trip() {
        use std::os::unix::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_vec(b"/worktree/\xff".to_vec()));
        assert_eq!(path_from_bytes(&path_bytes(&path)), path);
    }

    #[test]
    fn crash_child() {
        let Some(path) = std::env::var_os("GITTURTLE_RECOVERY_CRASH_FIXTURE") else {
            return;
        };
        let path = PathBuf::from(path);
        let key = key('a');
        let saver = commit_drafts::CoalescingSaver::default();
        let writer = SerialExecutor::new("recovery-crash-fixture");
        let output = path.clone();
        let response = saver
            .queue_with(
                &writer,
                key.clone(),
                Some(draft(key, "Forced termination 🐢\r\n  exact text\n")),
                move |batch| save_at(batch, &output),
            )
            .unwrap();
        futures::executor::block_on(response).unwrap().unwrap();
        std::fs::write(path.with_extension("ready"), "saved").unwrap();
        // The parent terminates this process after the completed durable save.
        loop {
            std::thread::park_timeout(std::time::Duration::from_secs(1));
        }
    }

    #[test]
    fn forced_termination_after_saved_keeps_exact_recoverable_text() {
        let path = fixture("forced-termination");
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "recovery_drafts::tests::crash_child",
                "--nocapture",
            ])
            .env("GITTURTLE_RECOVERY_CRASH_FIXTURE", &path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let started = std::time::Instant::now();
        while !path.with_extension("ready").exists()
            && started.elapsed() < std::time::Duration::from_secs(10)
        {
            if child.try_wait().unwrap().is_some() {
                panic!("Recovery save child exited before confirmation");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let saved = path.with_extension("ready").exists();
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(saved, "Child did not confirm the durable save");
        assert_eq!(
            read(&path).unwrap().get(&key('a')).unwrap().text.as_bytes(),
            "Forced termination 🐢\r\n  exact text\n".as_bytes()
        );
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
