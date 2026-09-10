//! Recovery text is application data, never a replayable Git operation.
//! Shares the commit draft coalescer and atomic writer, but has independent
//! bounds and storage so large resolutions cannot crowd out ordinary drafts.
use crate::*;
use anyhow::{Context as _, Result, ensure};
use futures::{
    FutureExt,
    future::{BoxFuture, Shared},
};
use gpui_kit::component::{WindowExt, dialog::DialogFooter};
use gpui_kit::prelude::FluentBuilder;
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    fs::File,
    io::Read,
    path::Path,
    rc::Rc,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_ENTRIES: usize = 256;
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_TEXT: usize = 2 * 1024 * 1024;
type Batch = HashMap<Key, Option<Draft>>;
type SaveCompletion = Shared<BoxFuture<'static, std::result::Result<(), String>>>;

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
    save_completion: Rc<RefCell<Option<SaveCompletion>>>,
    save_generation: u64,
    pub error: Option<String>,
    browser: Option<WeakEntity<RecoveryList>>,
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
    /// Keep the actual accepted save independent of the window and its view.
    /// The toolkit still bounds the total native shutdown wait to 200 ms.
    fn install_quit_observer(&self, cx: &mut App) {
        let completion = self.save_completion.clone();
        cx.on_app_quit(move |_| {
            let completion = completion.borrow().clone();
            async move {
                if let Some(completion) = completion {
                    let _ = completion.await;
                }
            }
        })
        .detach();
    }

    fn queue_save(
        &self,
        writer: &SerialExecutor,
        key: Key,
        draft: Option<Draft>,
        save: impl FnMut(&Batch) -> Result<()> + Send + 'static,
    ) -> Option<SaveCompletion> {
        let response = self.saver.queue_with(writer, key, draft, save)?;
        let completion = response
            .map(|result| match result {
                Ok(Ok(())) => Ok(()),
                Ok(Err(error)) => Err(format!("{error:#}")),
                Err(_) => Err("Save ended without confirmation".into()),
            })
            .boxed()
            .shared();
        *self.save_completion.borrow_mut() = Some(completion.clone());
        Some(completion)
    }

    fn rows(&self) -> Vec<RecoveryRow> {
        let mut rows: Vec<_> = self
            .entries
            .values()
            .map(|draft| RecoveryRow {
                key: draft.key.clone(),
                bytes: draft.text.len(),
                updated: draft.updated,
            })
            .collect();
        rows.sort_by_key(|row| std::cmp::Reverse(row.updated));
        rows
    }
    fn next_timestamp(&self) -> u64 {
        now().max(
            self.entries
                .values()
                .map(|draft| draft.updated)
                .max()
                .unwrap_or(0)
                .saturating_add(1),
        )
    }
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
            .get(key)
            .or_else(|| {
                self.entries
                    .values()
                    .filter(|draft| draft.key.same_location(key))
                    .max_by_key(|draft| draft.updated)
            })
            .cloned()
    }
    pub(super) fn status(&self) -> String {
        self.save_status(!self.entries.is_empty())
    }
    pub(super) fn status_for(&self, key: &Key) -> String {
        self.save_status(self.entries.contains_key(key))
    }
    fn save_status(&self, saved: bool) -> String {
        if let Some(error) = &self.error {
            format!(
                "Draft save failed: {error}. Text remains available; Retry save or Copy saved text."
            )
        } else if self.saver.is_pending() {
            "Saving recovery draft… Keep the app open until Saved appears.".into()
        } else if !saved {
            "Edits save automatically outside the repository".into()
        } else {
            "Recovery draft saved locally · no file or index write".into()
        }
    }
}

fn validate_budget(entries: &HashMap<Key, Draft>) -> Result<()> {
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
fn validate(entries: &HashMap<Key, Draft>) -> Result<()> {
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
    /// Await the accepted recovery save after normal quit or last-window close,
    /// without needing another slot in the serialized preference queue. Only a
    /// confirmed Saved state proves durability beyond GPUI's 200 ms quit grace.
    pub(super) fn install_draft_quit_observer(&mut self, cx: &mut Context<Self>) {
        self.recovery_drafts.install_quit_observer(cx);
        // Preserve the existing best-effort drain for other preferences during
        // ordinary Quit. Recovery durability no longer depends on this extra
        // queue slot or on the view surviving until the quit callback.
        self.subscriptions.push(cx.on_app_quit(|this, _| {
            let barrier = this.preferences_writer.submit(|| Ok(()));
            async move {
                let _ = barrier.await;
            }
        }));
    }
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
            updated: self.recovery_drafts.next_timestamp(),
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
            updated: self.recovery_drafts.next_timestamp(),
        });
        if let Some(draft) = &draft {
            // Do not silently evict old or stale recoverable text when full.
            let old = self
                .recovery_drafts
                .entries
                .insert(key.clone(), draft.clone());
            if let Err(error) = validate_budget(&self.recovery_drafts.entries) {
                if let Some(old) = old {
                    self.recovery_drafts.entries.insert(key, old);
                } else {
                    self.recovery_drafts.entries.remove(&key);
                }
                self.recovery_drafts.error = Some(format!("{error:#}"));
                self.refresh_recovery_status(cx);
                cx.notify();
                return;
            }
        } else {
            self.recovery_drafts.entries.remove(&key);
        }
        let response =
            self.recovery_drafts
                .queue_save(&self.preferences_writer, key, draft, |batch| {
                    save_at(batch, &store_path()?)
                });
        if let Some(response) = response {
            self.recovery_drafts.save_generation =
                self.recovery_drafts.save_generation.wrapping_add(1);
            let generation = self.recovery_drafts.save_generation;
            self.recovery_drafts.error = None;
            cx.spawn_in(window, async move |this, cx| {
                let result = response.await;
                let _ = this.update_in(cx, |this, window, cx| {
                    // A newer accepted batch may have finished before this UI
                    // reply was polled. Do not clear its failure with an older
                    // success or replace a newer success with an old failure.
                    if this.recovery_drafts.save_generation == generation {
                        this.recovery_drafts.error = result.err();
                    }
                    this.refresh_recovery_status(cx);
                    window.refresh();
                    cx.notify();
                });
            })
            .detach();
        }
        self.refresh_recovery_status(cx);
        cx.notify();
    }

    fn refresh_recovery_status(&self, cx: &mut Context<Self>) {
        if let Some(view) = &self.conflict_view {
            let view = view.downgrade();
            let owner = cx.entity().downgrade();
            cx.defer(move |cx| {
                let (Some(owner), Some(view)) = (owner.upgrade(), view.upgrade()) else {
                    return;
                };
                // Read only after the active editor/input update has ended.
                let status = owner
                    .read(cx)
                    .recovery_drafts
                    .status_for(&view.read(cx).durable_key);
                view.update(cx, |view, cx| {
                    view.durable_status = status;
                    cx.notify();
                });
            });
        }
        self.refresh_rebase_message(cx);
        if let Some(browser) = self
            .recovery_drafts
            .browser
            .as_ref()
            .and_then(WeakEntity::upgrade)
        {
            let rows = self.recovery_drafts.rows();
            let status = self.recovery_drafts.status();
            let browser = browser.downgrade();
            cx.defer(move |cx| {
                let _ = browser.update(cx, |browser, cx| {
                    browser.rows = rows;
                    browser.status = status;
                    cx.notify();
                });
            });
        }
    }

    pub(super) fn open_recovery_drafts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let owner = cx.entity().downgrade();
        let rows = self.recovery_drafts.rows();
        let status = self.recovery_drafts.status();
        let return_focus = self.app_focus.clone();
        let form = cx.new(|_| RecoveryList {
            owner,
            rows,
            status,
            return_focus,
            scroll: UniformListScrollHandle::new(),
        });
        self.recovery_drafts.browser = Some(form.downgrade());
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let close = form.clone();
            let cancel = form.clone();
            dialog
                .title("Saved recovery drafts")
                .width(px(720.))
                .child(form.clone())
                .footer(DialogFooter::new().child(
                    button("close-saved-recovery", "Close", "", false).on_click(
                        move |_, window, cx| close.update(cx, |form, cx| form.close(window, cx)),
                    ),
                ))
                .on_cancel(move |_, window, cx| {
                    cancel.update(cx, |form, cx| form.close(window, cx));
                    false
                })
        });
    }
}

struct RecoveryList {
    owner: WeakEntity<GitTurtle>,
    scroll: UniformListScrollHandle,
    rows: Vec<RecoveryRow>,
    status: String,
    return_focus: FocusHandle,
}

struct RecoveryRow {
    key: Key,
    bytes: usize,
    updated: u64,
}

impl RecoveryList {
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.close_dialog(cx);
        self.return_focus.focus(window, cx);
        window.refresh();
    }
    fn row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.rows.get(index) else {
            return div().into_any_element();
        };
        let p = palette(cx);
        let key = &row.key;
        let bytes = row.bytes;
        let copy = key.clone();
        let retry = key.clone();
        let discard = key.clone();
        div()
            .h(appearance::ui_size(110.))
            .p_3()
            .border_b_1()
            .border_color(rgb(p.border))
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .truncate()
                    .text_size(appearance::ui_text(12.))
                    .child(key.description()),
            )
            .child(
                div()
                    .text_size(appearance::ui_text(11.))
                    .text_color(rgb(p.muted))
                    .child(format!("{bytes} bytes · source {}", &key.source[..12])),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        button(("copy-recovery", index), "Copy exact text", "", false).on_click(
                            cx.listener(move |this, _, _, cx| {
                                if let Some(owner) = this.owner.upgrade() {
                                    let text = owner
                                        .read(cx)
                                        .recovery_drafts
                                        .entries
                                        .get(&copy)
                                        .map(|draft| draft.text.clone());
                                    if let Some(text) = text {
                                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                                    }
                                }
                            }),
                        ),
                    )
                    .child(
                        button(("retry-recovery", index), "Retry save", "", false).on_click(
                            cx.listener(move |this, _, window, cx| {
                                let _ = this.owner.update(cx, |owner, cx| {
                                    let text = owner
                                        .recovery_drafts
                                        .entries
                                        .get(&retry)
                                        .map(|draft| draft.text.clone());
                                    if let Some(text) = text {
                                        owner.persist_recovery_draft(
                                            retry.clone(),
                                            Some(text),
                                            window,
                                            cx,
                                        );
                                    }
                                });
                            }),
                        ),
                    )
                    .child(
                        button(
                            ("discard-recovery", index),
                            "Discard saved draft",
                            "",
                            false,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                let _ = this.owner.update(cx, |owner, cx| {
                                    owner.persist_recovery_draft(discard.clone(), None, window, cx)
                                });
                            },
                        )),
                    ),
            )
            .into_any_element()
    }
}
impl Render for RecoveryList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let count = self.rows.len();
        let status = self.status.clone();
        let available = (window.viewport_size().height - px(300.)).clamp(px(120.), px(560.));
        let height = (appearance::ui_size(110.) * count.min(5) as f32 + px(2.))
            .max(px(120.))
            .min(available);
        div().flex().flex_col().gap_3().child("Draft text is retained until you discard it, including completed, aborted, moved or removed worktrees. Copying never stages or continues an operation. Open the matching conflict or rebase message to check whether restoration is safe.")
            .child(div().text_color(rgb(p.muted)).child(status))
            .child(div().h(height).border_1().border_color(rgb(p.border)).rounded(px(6.)).overflow_hidden()
                .when(count == 0, |list| list.child(empty("No saved recovery drafts", "Conflict results and rebase messages appear here after you edit them.")))
                .when(count > 0, |list| list.child(uniform_list("saved-recovery-list", count, cx.processor(|this, range: std::ops::Range<usize>, _, cx| range.map(|index| this.row(index, cx)).collect::<Vec<_>>() )).size_full().track_scroll(&self.scroll))))
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
        assert!(state.status_for(&key).contains("saved locally"));
        let stale = Key {
            source: "b".repeat(64),
            ..key.clone()
        };
        let saved = state.latest(&stale).unwrap();
        assert_ne!(saved.key, stale);
        assert_eq!(saved.text, text);
        assert_eq!(
            state.status_for(&stale),
            "Edits save automatically outside the repository"
        );
        let message = Key {
            kind: Kind::RebaseMessage,
            file: vec![],
            ..key.clone()
        };
        assert_eq!(
            state.status_for(&message),
            "Edits save automatically outside the repository",
            "A saved conflict must not confirm an untouched rebase message"
        );
        assert!(state.status().contains("saved locally"));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn returning_to_an_earlier_source_prefers_its_valid_draft_and_keeps_newer_stale_text() {
        let original = key('a');
        let changed = key('b');
        let mut first = draft(original.clone(), "valid for original source\r\n");
        first.updated = 1;
        let mut second = draft(changed.clone(), "newer draft for changed source\n");
        second.updated = 2;
        let state = State {
            entries: HashMap::from([(original.clone(), first), (changed.clone(), second)]),
            ..State::default()
        };
        assert_eq!(state.latest(&original).unwrap().key, original);
        assert_eq!(state.latest(&changed).unwrap().key, changed);
        assert_eq!(
            state.latest(&key('c')).unwrap().text,
            "newer draft for changed source\n"
        );
        assert_eq!(
            state.entries.len(),
            2,
            "Source changes never remove recoverable text"
        );
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

    #[gpui::test]
    async fn final_recovery_draft_survives_removed_window_and_full_save_queue(
        cx: &mut TestAppContext,
    ) {
        struct RecoveryEditor {
            recovery: State,
        }
        impl Render for RecoveryEditor {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div()
            }
        }

        cx.executor().allow_parking();
        cx.executor().set_block_on_ticks(100..=100);
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("recovery-drafts.json");
        let writer = SerialExecutor::new("recovery-quit-fixture");
        let (started, running) = futures::channel::oneshot::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let blocker = writer.submit(move || {
            let _ = started.send(());
            gate.recv()?;
            Ok(())
        });
        running.await.unwrap();
        let queued: Vec<_> = (0..7).map(|_| writer.submit(|| Ok(()))).collect();
        let (editor, window_cx) = cx.add_window_view(|_, cx| {
            let recovery = State::default();
            recovery.install_quit_observer(cx);
            RecoveryEditor { recovery }
        });
        let output = path.clone();
        let identity = key('a');
        editor.update(window_cx, |editor, _| {
            drop(editor.recovery.queue_save(
                &writer,
                identity.clone(),
                Some(draft(identity.clone(), "first")),
                move |batch| save_at(batch, &output),
            ));
            assert!(
                editor
                    .recovery
                    .queue_save(
                        &writer,
                        identity.clone(),
                        Some(draft(
                            identity.clone(),
                            "Final resolution 🐢\r\n  exact text\n"
                        )),
                        |_| unreachable!()
                    )
                    .is_none()
            );
        });
        assert!(writer.submit(|| Ok(())).await.unwrap().is_err());
        let weak = editor.downgrade();
        drop(editor);
        window_cx.update(|window, _| window.remove_window());
        assert!(weak.upgrade().is_none());
        assert!(!path.exists());
        window_cx
            .executor()
            .spawn(async move {
                release.send(()).unwrap();
            })
            .detach();
        cx.quit();
        // GPUI shutdown must finish the actual accepted save before we drain
        // any of the external executor replies below.
        assert_eq!(
            read(&path).unwrap().get(&identity).unwrap().text.as_bytes(),
            "Final resolution 🐢\r\n  exact text\n".as_bytes()
        );
        blocker.await.unwrap().unwrap();
        for response in queued {
            response.await.unwrap().unwrap();
        }
    }
}
