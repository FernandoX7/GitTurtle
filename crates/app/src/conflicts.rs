use crate::*;
use gitturtle_core::{
    ConflictBlockChoice, ConflictContent, ConflictPreview, ConflictResolution, IntegrationCommand,
    MAX_DIFF_BYTES, MAX_DIFF_LINES, OperationKind, TextConflictBlock, WriteCommand,
    choose_conflict_block, text_conflict_blocks,
};
use gpui_kit::component::input::Paste;
use gpui_kit::prelude::FluentBuilder;
use std::ops::Range;

const MAX_RETAINED_DRAFT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone)]
struct ConflictIdentity {
    durable: String,
}

impl From<&ConflictPreview> for ConflictIdentity {
    fn from(snapshot: &ConflictPreview) -> Self {
        Self {
            durable: snapshot.draft_identity(),
        }
    }
}

pub struct Draft {
    identity: ConflictIdentity,
    text: String,
}

impl Draft {
    #[cfg(test)]
    fn matches(&self, snapshot: &ConflictPreview) -> bool {
        self.identity.durable == snapshot.draft_identity()
    }
}

pub struct Presentation {
    pub snapshot: Arc<ConflictPreview>,
    sources: [Option<String>; 3],
    labels: [String; 3],
    result: Option<String>,
    identity: ConflictIdentity,
    blocks: Vec<TextConflictBlock>,
    block_issue: Option<String>,
}

impl Presentation {
    /// Called on the read worker, including UTF-8 validation and text limits.
    pub fn prepare(snapshot: ConflictPreview) -> Self {
        let source = |content: &ConflictContent| {
            if draft_issue(&content.bytes, MAX_DIFF_BYTES).is_some() {
                return None;
            }
            std::str::from_utf8(&content.bytes).ok().map(str::to_owned)
        };
        let sources = [&snapshot.base, &snapshot.current, &snapshot.incoming]
            .map(|side| side.as_ref().and_then(|side| source(&side.content)));
        let regular =
            |content: &ConflictContent| matches!(content.mode.as_str(), "100644" | "100755");
        let result = match &snapshot.working {
            Some(working) if regular(working) => source(working),
            None if [&snapshot.current, &snapshot.incoming]
                .into_iter()
                .flatten()
                .all(|side| regular(&side.content)) =>
            {
                Some(String::new())
            }
            _ => None,
        };
        let absent = match &snapshot.operation {
            Some(operation) => operation_side_labels(
                operation.kind,
                &operation.branch,
                &operation.target_label,
                operation.commit.as_deref(),
            ),
            None => [
                "Common ancestor".into(),
                snapshot.head.as_deref().map_or_else(
                    || "Working tree".into(),
                    |head| format!("Checked-out commit {}", short_oid(head)),
                ),
                "Incoming version".into(),
            ],
        };
        let mut labels: [String; 3] = [&snapshot.base, &snapshot.current, &snapshot.incoming]
            .into_iter()
            .zip(absent)
            .map(|(side, absent)| {
                side.as_ref().map_or_else(
                    || format!("{absent} · file absent"),
                    |side| side.label.clone(),
                )
            })
            .collect::<Vec<_>>()
            .try_into()
            .expect("three conflict labels");
        if snapshot.operation.is_none()
            && result.as_deref().is_some_and(|source| {
                source.lines().any(|line| {
                    line.starts_with(">>>>>>>")
                        && line.trim_start_matches('>').trim() == "Stashed changes"
                })
            })
        {
            labels[1] = "Updated upstream · current worktree".into();
            labels[2] = "Stashed changes · incoming".into();
        }
        let (blocks, block_issue) = result.as_deref().map_or_else(
            || (Vec::new(), None),
            |source| match text_conflict_blocks(source) {
                Ok(blocks) => (blocks, None),
                Err(error) => (Vec::new(), Some(error.to_string())),
            },
        );
        Self {
            identity: ConflictIdentity::from(&snapshot),
            snapshot: Arc::new(snapshot),
            sources,
            labels,
            result,
            blocks,
            block_issue,
        }
    }

    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.identity.durable.capacity()
            + self.labels.iter().map(String::capacity).sum::<usize>()
            + self
                .sources
                .iter()
                .flatten()
                .map(String::capacity)
                .sum::<usize>()
            + self.result.as_ref().map_or(0, String::capacity)
            + self.blocks.capacity() * std::mem::size_of::<TextConflictBlock>()
            + self.block_issue.as_ref().map_or(0, String::capacity)
            + [
                &self.snapshot.base,
                &self.snapshot.current,
                &self.snapshot.incoming,
            ]
            .into_iter()
            .flatten()
            .map(|side| side.content.bytes.capacity())
            .sum::<usize>()
            + self
                .snapshot
                .working
                .as_ref()
                .map_or(0, |content| content.bytes.capacity())
    }

    pub fn file(&self) -> FileChange {
        FileChange {
            old_path: Some(self.snapshot.path.clone()),
            new_path: Some(self.snapshot.path.clone()),
            old_oid: self.snapshot.current.as_ref().map(|side| side.oid.clone()),
            new_oid: self.snapshot.incoming.as_ref().map(|side| side.oid.clone()),
            old_mode: self
                .snapshot
                .current
                .as_ref()
                .map_or("000000", |side| &side.content.mode)
                .to_owned(),
            new_mode: self
                .snapshot
                .incoming
                .as_ref()
                .map_or("000000", |side| &side.content.mode)
                .to_owned(),
            status: gitturtle_core::ChangeStatus::Modified,
        }
    }

    fn label(&self, side: usize) -> String {
        self.labels[side].clone()
    }
}

fn operation_side_labels(
    kind: OperationKind,
    branch: &str,
    target: &str,
    commit: Option<&str>,
) -> [String; 3] {
    let commit = commit
        .map(short_oid)
        .unwrap_or_else(|| "pending commit".into());
    [
        "Common ancestor".into(),
        if kind == OperationKind::Rebase {
            format!("{target} · updated base")
        } else {
            format!("{branch} · current branch")
        },
        match kind {
            OperationKind::Rebase => format!("{branch} · replayed commit {commit}"),
            OperationKind::Revert => format!("Before reverted commit {commit}"),
            _ => format!("{target} · incoming changes"),
        },
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DraftIssue {
    Bytes,
    Lines,
    Nul,
    RetainedBudget,
    Clipboard,
}

impl DraftIssue {
    fn explanation(self) -> &'static str {
        match self {
            Self::Bytes => {
                "This edit exceeds the 2 MiB resolution limit. Your previous draft is unchanged; use an external editor for larger files."
            }
            Self::Lines => {
                "This edit exceeds the 100,000-line resolution limit. Your previous draft is unchanged; use an external editor for larger files."
            }
            Self::Nul => {
                "This edit contains binary NUL bytes. Your previous draft is unchanged; choose a complete version or use an external editor."
            }
            Self::RetainedBudget => {
                "Finish and stage another saved resolution before expanding this draft. Your previous draft is unchanged."
            }
            Self::Clipboard => "The clipboard has no text to paste. Your draft is unchanged.",
        }
    }
}

fn draft_issue(bytes: &[u8], limit: usize) -> Option<DraftIssue> {
    if bytes.len() > MAX_DIFF_BYTES {
        Some(DraftIssue::Bytes)
    } else if bytes.len() > limit {
        Some(DraftIssue::RetainedBudget)
    } else if bytes.contains(&0) {
        Some(DraftIssue::Nul)
    } else if bytes
        .iter()
        .filter(|byte| **byte == b'\n')
        .take(MAX_DIFF_LINES + 1)
        .count()
        > MAX_DIFF_LINES
    {
        Some(DraftIssue::Lines)
    } else {
        None
    }
}

fn paste_issue(
    current: &str,
    selection: Range<usize>,
    inserted: &str,
    limit: usize,
) -> Option<DraftIssue> {
    let resulting_bytes = current
        .len()
        .saturating_sub(selection.len())
        .saturating_add(inserted.len());
    if resulting_bytes > MAX_DIFF_BYTES {
        return Some(DraftIssue::Bytes);
    }
    if resulting_bytes > limit {
        return Some(DraftIssue::RetainedBudget);
    }
    let mut result = String::with_capacity(resulting_bytes);
    result.push_str(&current[..selection.start]);
    result.push_str(inserted);
    result.push_str(&current[selection.end..]);
    draft_issue(result.as_bytes(), limit)
}

pub enum ConflictEvent {
    Resolve(ConflictResolution),
    Draft(String),
    OpenEditor,
    RecoveryDrafts,
}

pub struct ConflictView {
    pub(super) durable_status: String,
    pub(super) durable_key: recovery_drafts::Key,
    durable_saved: Option<(bool, String)>,
    presentation: Arc<Presentation>,
    readers: [Option<Entity<EditorState>>; 3],
    resolution: Option<Entity<TextareaState>>,
    base: bool,
    initial_draft: Option<String>,
    resolution_subscription: Option<Subscription>,
    draft_limit: usize,
    accepted_draft: String,
    accepted_selection: Range<usize>,
    draft_error: Option<DraftIssue>,
    blocks: Vec<TextConflictBlock>,
    block_source: String,
    block_issue: Option<String>,
    block_selected: usize,
    block_mode: bool,
    block_pending: bool,
    block_generation: u64,
    block_task: Option<Task<()>>,
    block_executor: SerialExecutor,
}

impl EventEmitter<ConflictEvent> for ConflictView {}

impl ConflictView {
    pub(super) fn new(
        presentation: Arc<Presentation>,
        durable_key: recovery_drafts::Key,
        initial_draft: Option<String>,
        draft_limit: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            durable_status: "Edits are automatically saved outside the repository".into(),
            durable_key,
            durable_saved: None,
            blocks: presentation.blocks.clone(),
            block_source: presentation.result.clone().unwrap_or_default(),
            block_issue: presentation.block_issue.clone(),
            block_selected: 0,
            block_mode: !presentation.blocks.is_empty(),
            block_pending: false,
            block_generation: 0,
            block_task: None,
            block_executor: SerialExecutor::new("conflict-block-review"),
            presentation,
            readers: [None, None, None],
            resolution: None,
            base: false,
            initial_draft,
            resolution_subscription: None,
            draft_limit,
            accepted_draft: String::new(),
            accepted_selection: 0..0,
            draft_error: None,
        };
        this.prepare_readers(window, cx);
        if this.initial_draft.is_some() || this.block_mode {
            this.open_resolution_editor(window, cx);
            if this.initial_draft.is_some() {
                this.schedule_blocks(window, cx);
            }
        }
        this
    }

    fn open_resolution_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.resolution.is_none()
            && let Some(value) = &self.presentation.result
        {
            let value = self.initial_draft.as_ref().unwrap_or(value);
            if let Some(issue) = draft_issue(value.as_bytes(), self.draft_limit) {
                self.draft_error = Some(issue);
                cx.notify();
                return;
            }
            self.accepted_draft = value.clone();
            let editor = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .rows(10)
                    .default_value(value.clone())
            });
            self.resolution_subscription =
                Some(
                    cx.subscribe_in(&editor, window, |this, editor, event, window, cx| {
                        if !matches!(event, InputEvent::Change) {
                            return;
                        }
                        // Inspect the borrowed rope length before materializing a large
                        // IME/drop insertion. Ordinary paste is rejected before insertion.
                        let bytes = editor.read(cx).text().len();
                        let issue = if bytes > MAX_DIFF_BYTES {
                            Some(DraftIssue::Bytes)
                        } else if bytes > this.draft_limit {
                            Some(DraftIssue::RetainedBudget)
                        } else {
                            None
                        };
                        let value = issue.is_none().then(|| editor.read(cx).value());
                        let issue = issue.or_else(|| {
                            value
                                .as_ref()
                                .and_then(|value| draft_issue(value.as_bytes(), this.draft_limit))
                        });
                        if let Some(issue) = issue {
                            let selection = this.accepted_selection.clone();
                            let scroll = editor.read(cx).scroll_offset();
                            editor.update(cx, |editor, cx| {
                                editor.set_value(this.accepted_draft.clone(), window, cx);
                                editor.set_selected_range(selection, cx);
                                editor.set_scroll_offset(scroll, cx);
                            });
                            this.draft_error = Some(issue);
                        } else if let Some(value) = value {
                            this.accepted_draft = value.to_string();
                            this.accepted_selection = editor.read(cx).selected_range();
                            this.draft_error = None;
                            cx.emit(ConflictEvent::Draft(this.accepted_draft.clone()));
                            this.schedule_blocks(window, cx);
                        }
                        cx.notify();
                    }),
                );
            self.resolution = Some(editor);
        }
        if let Some(editor) = &self.resolution {
            window.focus(&editor.focus_handle(cx), cx);
        }
        cx.notify();
    }

    fn check_paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = &self.resolution else {
            return;
        };
        let Some(clipboard) = cx.read_from_clipboard() else {
            return;
        };
        let Some(inserted) = clipboard.text() else {
            self.draft_error = Some(DraftIssue::Clipboard);
            cx.stop_propagation();
            cx.notify();
            return;
        };
        let current = editor.read(cx).value();
        let selection = editor.read(cx).selected_range();
        self.draft_error = paste_issue(&current, selection, &inserted, self.draft_limit);
        if self.draft_error.is_some() {
            cx.stop_propagation();
        }
        cx.notify();
    }

    fn restore_saved_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((true, text)) = self.durable_saved.clone() else {
            return;
        };
        if let Some(issue) = draft_issue(text.as_bytes(), self.draft_limit) {
            self.draft_error = Some(issue);
            cx.notify();
            return;
        }
        self.open_resolution_editor(window, cx);
        let Some(editor) = &self.resolution else {
            return;
        };
        // set_value deliberately suppresses InputEvent::Change. Synchronize
        // the accepted save payload and run the same guarded block review as
        // a user edit instead of leaving the original conflict state active.
        editor.update(cx, |editor, cx| editor.set_value(text.clone(), window, cx));
        self.accepted_draft = text.clone();
        self.accepted_selection = editor.read(cx).selected_range();
        self.draft_error = None;
        self.durable_saved = None;
        cx.emit(ConflictEvent::Draft(text));
        self.schedule_blocks(window, cx);
    }

    fn prepare_readers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for side in if self.base { vec![0] } else { vec![1, 2] } {
            if self.readers[side].is_none()
                && let Some(source) = self.reader_source(side)
            {
                self.readers[side] = Some(text::editor(source, "text", None, window, cx));
            }
        }
    }

    fn reader_source(&self, side: usize) -> Option<&str> {
        if self.block_mode {
            let block = self.blocks.get(self.block_selected)?;
            let range = match side {
                0 => block.base.clone()?,
                1 => block.current.clone(),
                _ => block.incoming.clone(),
            };
            self.block_source.get(range)
        } else {
            self.presentation.sources[side].as_deref()
        }
    }

    fn schedule_blocks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.block_generation = self.block_generation.wrapping_add(1);
        self.block_task = None;
        let generation = self.block_generation;
        let source = self.accepted_draft.clone();
        self.block_pending = true;
        let response = self.block_executor.submit_read(move || {
            let blocks = text_conflict_blocks(&source).map_err(|error| error.to_string());
            Ok((source, blocks))
        });
        self.block_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.block_generation != generation { return; }
                this.block_pending = false;
                match result {
                    Ok(Ok((source, blocks))) => {
                        this.block_source = source;
                        match blocks { Ok(blocks) => { this.blocks = blocks; this.block_issue = None; }, Err(issue) => { this.blocks.clear(); this.block_issue = Some(issue); } }
                        this.block_selected = this.block_selected.min(this.blocks.len().saturating_sub(1));
                    }
                    _ => { this.blocks.clear(); this.block_issue = Some("Block review was interrupted. Edit the result to retry, or save the draft and refresh.".into()); }
                }
                if this.block_mode {
                    this.readers = [None, None, None];
                    this.prepare_readers(window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn select_block(&mut self, previous: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.block_pending || self.blocks.is_empty() {
            return;
        }
        self.block_selected = if previous {
            self.block_selected.saturating_sub(1)
        } else {
            (self.block_selected + 1).min(self.blocks.len() - 1)
        };
        self.block_mode = true;
        self.readers = [None, None, None];
        self.prepare_readers(window, cx);
        self.open_resolution_editor(window, cx);
        if let Some(block) = self.blocks.get(self.block_selected)
            && let Some(editor) = &self.resolution
        {
            let range = block.range.clone();
            editor.update(cx, |editor, cx| editor.set_selected_range(range, cx));
        }
        cx.notify();
    }

    fn accept_block(
        &mut self,
        choice: ConflictBlockChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.block_pending || self.block_source != self.accepted_draft {
            return;
        }
        let Some(block) = self.blocks.get(self.block_selected) else {
            return;
        };
        let start = block.range.start;
        match choose_conflict_block(&self.block_source, block, choice) {
            Ok(value) => {
                if let Some(issue) = draft_issue(value.as_bytes(), self.draft_limit) {
                    self.draft_error = Some(issue);
                    cx.notify();
                    return;
                }
                self.accepted_draft = value.clone();
                self.accepted_selection = start..start;
                if let Some(editor) = &self.resolution {
                    editor.update(cx, |editor, cx| {
                        editor.set_value(value.clone(), window, cx);
                        editor.set_selected_range(start..start, cx);
                    });
                }
                cx.emit(ConflictEvent::Draft(value));
                self.schedule_blocks(window, cx);
            }
            Err(error) => {
                self.block_issue = Some(error.to_string());
                cx.notify();
            }
        }
    }

    pub(super) fn rescale_code(&mut self, ratio: f32, cx: &mut Context<Self>) {
        for reader in self.readers.iter().flatten() {
            reader.update(cx, |reader, cx| {
                reader.rescale_scroll_offset(ratio, cx);
            });
        }
        if let Some(editor) = &self.resolution {
            editor.update(cx, |editor, cx| {
                editor.rescale_scroll_offset(ratio, cx);
            });
        }
    }

    fn render_side(&self, side: usize, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let versions = [
            &self.presentation.snapshot.base,
            &self.presentation.snapshot.current,
            &self.presentation.snapshot.incoming,
        ];
        let description = if self.block_mode {
            self.blocks.get(self.block_selected).map_or_else(
                || "No unresolved blocks".into(),
                |block| {
                    format!(
                        "Block {} of {} · result line {}",
                        self.block_selected + 1,
                        self.blocks.len(),
                        block.first_line
                    )
                },
            )
        } else {
            versions[side]
                .as_ref()
                .map_or("This version has no file.".into(), |version| {
                    format!(
                        "{} · {} · {}",
                        short_oid(&version.oid),
                        format_bytes(version.content.bytes.len()),
                        version.content.mode
                    )
                })
        };
        div().flex_1().min_w_0().h_full().flex().flex_col().border_r_1().border_color(rgb(p.border))
            .child(div().px_3().py_2().flex().flex_col().gap_1().bg(rgb(p.panel))
                .child(div().truncate().text_size(crate::appearance::ui_text(12.)).font_weight(FontWeight::MEDIUM).child(self.presentation.label(side)))
                .child(div().truncate().text_size(crate::appearance::ui_text(10.)).text_color(rgb(p.muted)).child(description)))
            .child(div().flex_1().min_h_0().children(self.readers[side].as_ref().filter(|_| !self.block_mode || !self.block_pending).map(|reader| crate::editor_find::Editor::new(reader).readonly(true).bordered(false).h(relative(1.)).text_size(crate::appearance::code_text()).aria_label(format!("Conflict version: {}", self.presentation.label(side))))))
            .when(self.block_mode && self.block_pending, |el| el.child(div().p_4().text_size(crate::appearance::ui_text(12.)).child("Updating conflict blocks…")))
            .when(self.readers[side].is_none(), |el| el.child(div().p_4().text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)).child(if self.block_mode && self.blocks.is_empty() { "No unresolved blocks remain. Review the result, then explicitly save or stage it." } else if self.block_mode && side == 0 { "This marker style does not include a base block. Choose Whole file to inspect the full common ancestor." } else if versions[side].is_none() { "File absent in this version" } else { "Binary or large content · choose a complete version or use an external editor." })))
            .into_any_element()
    }
}

impl Render for ConflictView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let current = self.presentation.label(1);
        let incoming = self.presentation.label(2);
        let blocked = self.block_pending || self.block_issue.is_some() || self.blocks.is_empty();
        let cannot_stage_draft =
            self.block_pending || self.block_issue.is_some() || !self.blocks.is_empty();
        let cannot_stage_working =
            !self.presentation.blocks.is_empty() || self.presentation.block_issue.is_some();
        let status = if self.block_pending {
            "Updating conflict blocks…".into()
        } else if self.block_issue.is_some() {
            "Block decisions unavailable · edit the result or use a whole-file resolution".into()
        } else if self.blocks.is_empty() {
            "No unresolved blocks in the result".into()
        } else {
            format!(
                "{} unresolved blocks · selected {} · result line {}",
                self.blocks.len(),
                self.block_selected + 1,
                self.blocks[self.block_selected].first_line
            )
        };
        div().id("conflict-review").size_full().flex().flex_col()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.modifiers.platform && event.keystroke.modifiers.alt && matches!(event.keystroke.key.as_str(), "up" | "down") {
                    this.select_block(event.keystroke.key == "up", window, cx); cx.stop_propagation();
                }
            }))
            .child(div().px_3().py_2().flex().flex_wrap().items_center().gap_2().border_b_1().border_color(rgb(p.border))
                .child(button("saved-conflict-drafts", "Saved drafts…", "", false).on_click(cx.listener(|_, _, _, cx| cx.emit(ConflictEvent::RecoveryDrafts))))
                .child(button("conflict-branches", "Both sides", "", !self.base).toggled(!self.base).on_click(cx.listener(|this, _, window, cx| { this.base = false; this.prepare_readers(window, cx); cx.notify(); })))
                .child(button("conflict-base", "Base", "", self.base).toggled(self.base).on_click(cx.listener(|this, _, window, cx| { this.base = true; this.prepare_readers(window, cx); cx.notify(); })))
                .child(button("conflict-block-mode", "Selected block", "", self.block_mode).toggled(self.block_mode).disabled(self.blocks.is_empty()).on_click(cx.listener(|this, _, window, cx| { this.block_mode = true; this.readers = [None, None, None]; this.prepare_readers(window, cx); cx.notify(); })))
                .child(button("conflict-whole-mode", "Whole file", "", !self.block_mode).toggled(!self.block_mode).on_click(cx.listener(|this, _, window, cx| { this.block_mode = false; this.readers = [None, None, None]; this.prepare_readers(window, cx); cx.notify(); })))
                .child(div().flex_1())
                .child(button("external-conflict-editor", "Open in editor", "", false).tooltip("Open the working file with its default application. Refresh after saving, then stage the resolved file.")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ConflictEvent::OpenEditor)))))
            .when(self.presentation.result.is_some(), |element| element.child(div().px_3().py_2().flex().flex_col().gap_2().border_b_1().border_color(rgb(p.border))
                .child(div().id("conflict-block-status").role(Role::Status).a11y_synthetic_children(native_accessibility::polite).aria_label(status.clone()).text_size(crate::appearance::ui_text(12.)).child(status))
                .child(div().flex().flex_wrap().gap_2().items_center()
                    .child(button("previous-conflict-block", "Previous", "", false).accessibility_label("Previous conflict block").tooltip("Previous conflict · Command-Option-Up").disabled(blocked || self.block_selected == 0).on_click(cx.listener(|this, _, window, cx| this.select_block(true, window, cx))))
                    .child(button("next-conflict-block", "Next", "", false).accessibility_label("Next conflict block").tooltip("Next conflict · Command-Option-Down").disabled(blocked || self.block_selected + 1 >= self.blocks.len()).on_click(cx.listener(|this, _, window, cx| this.select_block(false, window, cx))))
                    .child(button("accept-current-block", "Accept current", "", false).accessibility_label(format!("Accept current block from {current} into the result draft")).tooltip(format!("Use only this block from {current} in the result draft")).disabled(blocked).on_click(cx.listener(|this, _, window, cx| this.accept_block(ConflictBlockChoice::Current, window, cx))))
                    .child(button("accept-incoming-block", "Accept incoming", "", false).accessibility_label(format!("Accept incoming block from {incoming} into the result draft")).tooltip(format!("Use only this block from {incoming} in the result draft")).disabled(blocked).on_click(cx.listener(|this, _, window, cx| this.accept_block(ConflictBlockChoice::Incoming, window, cx))))
                    .child(button("accept-both-block", "Accept both", "", false).accessibility_label("Accept both versions of this block, current then incoming, into the result draft").tooltip("Keep current, then incoming, for this block; review ordering in the result draft").disabled(blocked).on_click(cx.listener(|this, _, window, cx| this.accept_block(ConflictBlockChoice::Both, window, cx)))))
                .children(self.block_issue.as_ref().map(|issue| div().id("conflict-block-issue").role(Role::Alert).aria_label(issue.clone()).a11y_synthetic_children(native_accessibility::assertive).text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.warning)).child(issue.clone())))))
            .child(div().flex_1().min_h(px(100.)).flex().children(if self.base { vec![self.render_side(0, cx)] } else { vec![self.render_side(1, cx), self.render_side(2, cx)] }))
            .child(div().px_3().py_2().flex().flex_col().gap_2().bg(rgb(p.subtle)).border_t_1().border_color(rgb(p.border))
                .child(div().id("conflict-draft-storage-status").role(Role::Status).aria_label(self.durable_status.clone()).a11y_synthetic_children(native_accessibility::polite).text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)).child(self.durable_status.clone()))
                .when_some(self.durable_saved.clone(), |element, (valid, text)| {
                    let copy = text.clone();
                    element.child(div().flex().flex_wrap().items_center().gap_2()
                        .child(div().text_size(crate::appearance::ui_text(11.)).text_color(rgb(if valid { p.muted } else { p.warning })).child(if valid { "Saved draft matches this operation, index stages and working file." } else { "Saved draft belongs to changed source state. Recover its text without replacing this file." }))
                        .child(button("copy-saved-conflict", "Copy saved text", "", false).on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))))
                        .child(button("restore-saved-conflict", "Restore saved draft", "", false).debug_selector(|| "restore-saved-conflict".to_string()).disabled(!valid).on_click(cx.listener(move |this, _, window, cx| {
                            this.restore_saved_draft(window, cx);
                        }))))
                })
                .child(div().flex().flex_wrap().items_center().gap_2()
                    .child(div().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)).child("Whole-file actions:"))
                    .child(button("resolve-current", "Use current file…", "", false).accessibility_label(format!("Use current whole file from {current}, then review before replacing and staging")).tooltip(format!("Replace and stage the entire file with {current}"))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(ConflictEvent::Resolve(ConflictResolution::Current)))))
                    .child(button("resolve-incoming", "Use incoming file…", "", false).accessibility_label(format!("Use incoming whole file from {incoming}, then review before replacing and staging")).tooltip(format!("Replace and stage the entire file with {incoming}"))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(ConflictEvent::Resolve(ConflictResolution::Incoming)))))
                    .child(div().flex_1())
                    .child(button("manual-resolution", "Edit result", "", self.resolution.is_some()).disabled(self.presentation.result.is_none())
                        .on_click(cx.listener(|this, _, window, cx| this.open_resolution_editor(window, cx)))))
                .children(self.resolution.as_ref().map(|editor| div().capture_action(cx.listener(Self::check_paste))
                    .child(div().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)).child("Result draft · block choices and manual edits stay here until you explicitly save"))
                    .child(Textarea::new(editor).h(px(160.)).text_size(crate::appearance::code_text()).aria_label("Manual conflict resolution result"))))
                .children(self.draft_error.map(|issue| div().id("conflict-draft-error").role(Role::Alert).aria_label(issue.explanation()).a11y_synthetic_children(native_accessibility::assertive).text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.warning)).child(issue.explanation())))
                .child(div().flex().flex_wrap().items_center().gap_2()
                    .child(button("mark-conflict-resolved", "Stage edited file…", "", false).disabled(cannot_stage_working).tooltip("Stage the saved working file. Refresh after external editing.").on_click(cx.listener(|_, _, _, cx| cx.emit(ConflictEvent::Resolve(ConflictResolution::MarkResolved)))))
                    .children(self.resolution.as_ref().map(|_| button("save-conflict-draft", "Save draft…", "", false).on_click(cx.listener(|this, _, _, cx| {
                        cx.emit(ConflictEvent::Resolve(ConflictResolution::Save { bytes: this.accepted_draft.as_bytes().to_vec() }));
                    }))))
                    .children(self.resolution.as_ref().map(|_| button("save-conflict-resolution", "Save and stage result…", "", false).debug_selector(|| "save-conflict-resolution".to_string()).disabled(cannot_stage_draft).on_click(cx.listener(|this, _, _, cx| {
                        cx.emit(ConflictEvent::Resolve(ConflictResolution::Manual { bytes: this.accepted_draft.as_bytes().to_vec() }));
                    }))))))
    }
}

impl GitTurtle {
    pub(super) fn ensure_conflict_view(
        &mut self,
        presentation: Arc<Presentation>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.conflict_view.is_some() {
            return;
        }
        let draft_key = (
            self.path.clone().unwrap_or_default(),
            presentation.snapshot.path.clone(),
        );
        let persistent_key = recovery_drafts::Key::conflict(
            &draft_key.0,
            &draft_key.1,
            presentation.identity.durable.clone(),
        );
        let recovered = self.recovery_drafts.latest(&persistent_key);
        let draft = self
            .conflict_drafts
            .get(&draft_key)
            .filter(|draft| draft.identity.durable == presentation.identity.durable)
            .map(|draft| draft.text.clone());
        let retained_bytes = self
            .conflict_drafts
            .iter()
            .filter(|(key, _)| *key != &draft_key)
            .fold(0usize, |total, (_, draft)| {
                total.saturating_add(draft.text.len())
            });
        let draft_limit = MAX_RETAINED_DRAFT_BYTES
            .saturating_sub(retained_bytes)
            .min(MAX_DIFF_BYTES);
        let view = cx.new(|cx| {
            ConflictView::new(
                Arc::clone(&presentation),
                persistent_key.clone(),
                draft,
                draft_limit,
                window,
                cx,
            )
        });
        view.update(cx, |view, _| {
            view.durable_status = self.recovery_drafts.status_for(&persistent_key);
            if let Some(recovered) = recovered {
                view.durable_saved = Some((recovered.key == persistent_key, recovered.text));
            }
        });
        let snapshot = Arc::clone(&presentation.snapshot);
        let identity = presentation.identity.clone();
        let path = self.path.clone();
        self.conflict_subscription = Some(cx.subscribe_in(&view, window, move |this, _, event, window, cx| {
            if this.path != path { return; }
            // Draft text is local UI state and must survive an unrelated plan
            // read. Only executable actions are disabled while Git is busy.
            if let ConflictEvent::Draft(value) = event {
                this.conflict_drafts.insert(draft_key.clone(), Draft { identity: identity.clone(), text: value.clone() });
                this.persist_recovery_draft(persistent_key.clone(), Some(value.clone()), window, cx);
                return;
            }
            if this.operation_busy.is_some() { return; }
            match event {
                ConflictEvent::Draft(_) => unreachable!("drafts are saved before the operation guard"),
                ConflictEvent::RecoveryDrafts => this.open_recovery_drafts(window, cx),
                ConflictEvent::Resolve(resolution) => {
                    let file = snapshot.path.to_string_lossy();
                    let effect = match resolution {
                        ConflictResolution::Save { .. } => "Save the result draft to the working file without staging it. The file remains unresolved in the index.",
                        ConflictResolution::Manual { .. } => "Save the text you entered and stage this file.",
                        ConflictResolution::Current | ConflictResolution::Incoming => "Replace the working file with the selected complete version and stage it. Choosing an absent version deletes the file.",
                        ConflictResolution::MarkResolved => "Stage the working file exactly as reviewed. Ensure all conflict markers have been resolved.",
                    };
                    let save_only = matches!(resolution, ConflictResolution::Save { .. });
                    this.confirm_git_write(format!("{} {file}", if save_only { "Save draft for" } else { "Resolve" }), format!("{effect}\n\nOnly {file} is affected. A newer edit or operation state will require a fresh review."), if save_only { "Save draft" } else { "Resolve file" },
                        WriteCommand::Integration(IntegrationCommand::Resolve { expected: Arc::clone(&snapshot), resolution: resolution.clone() }), window, cx);
                }
                ConflictEvent::OpenEditor => {
                    let Some(repo) = this.repository.clone() else { return; };
                    let snapshot = Arc::clone(&snapshot);
                    let response = this.operations.submit_read(move || repo.conflict_editor_path(&snapshot));
                    cx.spawn_in(window, async move |this, cx| {
                        let result = response.await.unwrap_or_else(|_| Err(anyhow::anyhow!("Editor handoff interrupted")));
                        let _ = this.update_in(cx, |this, _, cx| {
                            match result {
                                Ok(path) => { cx.open_with_system(&path); this.operation_notice = Some("Edit and save the file, then refresh to review and stage the resolution.".into()); }
                                Err(error) => this.operation_error = Some(format!("{error:#}")),
                            }
                            cx.notify();
                        });
                    }).detach();
                }
            }
        }));
        self.conflict_view = Some(view);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gitturtle_core::ConflictSide;
    use std::{cell::RefCell, rc::Rc};

    fn snapshot() -> ConflictPreview {
        let side = |label: &str, oid: char, bytes: &[u8]| ConflictSide {
            label: label.into(),
            oid: oid.to_string().repeat(40),
            content: ConflictContent {
                mode: "100644".into(),
                bytes: bytes.to_vec(),
            },
        };
        ConflictPreview {
            path: PathBuf::from("conflicted.txt"),
            head: Some("a".repeat(40)),
            operation: None,
            base: Some(side("Common ancestor", 'b', b"base\n")),
            current: Some(side("feature · current branch", 'c', b"current\n")),
            incoming: Some(side("main · incoming changes", 'd', b"incoming\n")),
            working: Some(ConflictContent {
                mode: "100644".into(),
                bytes: b"draft\n".to_vec(),
            }),
        }
    }

    #[gpui::test]
    async fn restored_conflict_draft_recomputes_blocks_and_stages_exact_visible_text(
        cx: &mut TestAppContext,
    ) {
        const ORIGINAL: &str = "<<<<<<< current\ncurrent\n=======\nincoming\n>>>>>>> incoming\n";
        const RESTORED: &str = "Resolved together — 確認\n\nKeep this exact result.\n";
        cx.executor().allow_parking();
        cx.update(gpui_kit::init);
        let captured = Rc::new(RefCell::new(None));
        let observed = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let mut snapshot = snapshot();
            snapshot.working.as_mut().unwrap().bytes = ORIGINAL.as_bytes().to_vec();
            let presentation = Arc::new(Presentation::prepare(snapshot));
            let key = recovery_drafts::Key::conflict(
                std::path::Path::new("isolated-conflict-fixture"),
                &presentation.snapshot.path,
                presentation.identity.durable.clone(),
            );
            let view = cx.new(|cx| {
                let mut view =
                    ConflictView::new(presentation, key, None, MAX_DIFF_BYTES, window, cx);
                view.durable_saved = Some((true, RESTORED.into()));
                view
            });
            *captured.borrow_mut() = Some(view.clone());
            gpui_kit::component::Root::new(view, window, cx)
        });
        let view = observed.borrow_mut().take().unwrap();
        cx.simulate_resize(size(px(1480.), px(981.)));
        let drafts = Rc::new(RefCell::new(Vec::new()));
        let resolutions = Rc::new(RefCell::new(Vec::new()));
        let observed_drafts = drafts.clone();
        let observed_resolutions = resolutions.clone();
        let _subscription = cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event, _| match event {
                ConflictEvent::Draft(text) => observed_drafts.borrow_mut().push(text.clone()),
                ConflictEvent::Resolve(ConflictResolution::Manual { bytes }) => {
                    observed_resolutions.borrow_mut().push(bytes.clone());
                }
                _ => {}
            })
        });
        fn click(cx: &mut VisualTestContext, selector: &'static str) {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let bounds = cx
                .debug_bounds(selector)
                .expect("rendered conflict control");
            cx.simulate_click(bounds.center(), Modifiers::default());
        }
        click(cx, "save-conflict-resolution");
        assert!(
            resolutions.borrow().is_empty(),
            "original markers block staging"
        );

        // Hold the actual block worker so the test also exercises the pending
        // state: restored text cannot become stageable before it is parsed.
        let (release, wait) = std::sync::mpsc::channel();
        let (started, ready) = futures::channel::oneshot::channel();
        let held = cx.update(|_, cx| {
            view.read(cx).block_executor.submit_read(move || {
                let _ = started.send(());
                wait.recv().map_err(|error| anyhow::anyhow!(error))?;
                Ok(())
            })
        });
        ready.await.unwrap();
        click(cx, "restore-saved-conflict");
        cx.read(|cx| {
            let view = view.read(cx);
            assert_eq!(view.resolution.as_ref().unwrap().read(cx).value(), RESTORED);
            assert_eq!(view.accepted_draft, RESTORED);
            assert!(view.block_pending);
            assert!(view.durable_saved.is_none());
        });
        assert_eq!(drafts.borrow().as_slice(), &[RESTORED]);
        click(cx, "save-conflict-resolution");
        assert!(
            resolutions.borrow().is_empty(),
            "pending parsing blocks staging"
        );
        release.send(()).unwrap();
        held.await.unwrap().unwrap();
        let parse = cx.update(|_, cx| view.update(cx, |view, _| view.block_task.take()));
        parse.expect("restoration schedules block review").await;
        cx.read(|cx| {
            let view = view.read(cx);
            assert!(!view.block_pending);
            assert!(view.blocks.is_empty());
            assert!(view.block_issue.is_none());
            assert_eq!(view.block_source, RESTORED);
            assert!(view.readers.iter().all(Option::is_none));
            assert_eq!(view.presentation.result.as_deref(), Some(ORIGINAL));
        });
        click(cx, "save-conflict-resolution");
        assert_eq!(resolutions.borrow().as_slice(), &[RESTORED.as_bytes()]);
    }

    #[test]
    fn source_and_resolution_line_limits_preserve_raw_versions_without_building_editors() {
        let mut snapshot = snapshot();
        let large = vec![b'\n'; MAX_DIFF_LINES + 1];
        snapshot.current.as_mut().unwrap().content.bytes = large.clone();
        snapshot.working.as_mut().unwrap().bytes = large.clone();
        let presentation = Presentation::prepare(snapshot);
        assert!(presentation.sources[0].is_some());
        assert!(presentation.sources[1].is_none());
        assert!(presentation.sources[2].is_some());
        assert!(presentation.result.is_none());
        assert_eq!(
            presentation
                .snapshot
                .current
                .as_ref()
                .unwrap()
                .content
                .bytes,
            large
        );
        assert_eq!(presentation.snapshot.working.as_ref().unwrap().bytes, large);
    }

    #[test]
    fn prepared_blocks_and_stash_labels_describe_the_exact_working_result() {
        let mut preview = snapshot();
        preview.working.as_mut().unwrap().bytes = b"<<<<<<< Updated upstream\ncurrent\n||||||| base\nbase\n=======\nstashed\n>>>>>>> Stashed changes\n".to_vec();
        let presentation = Presentation::prepare(preview);
        assert_eq!(presentation.blocks.len(), 1);
        assert!(presentation.blocks[0].base.is_some());
        assert_eq!(presentation.label(1), "Updated upstream · current worktree");
        assert_eq!(presentation.label(2), "Stashed changes · incoming");
        let result = presentation.result.as_ref().unwrap();
        let chosen =
            choose_conflict_block(result, &presentation.blocks[0], ConflictBlockChoice::Both)
                .unwrap();
        assert_eq!(chosen, "current\nstashed\n");
        let draft = Draft {
            identity: presentation.identity.clone(),
            text: chosen,
        };
        assert!(draft.matches(&presentation.snapshot));
        let mut moved = presentation.snapshot.as_ref().clone();
        moved.current.as_mut().unwrap().oid = "9".repeat(40);
        assert!(!draft.matches(&moved));
    }

    #[test]
    fn text_limits_accept_complete_boundary_values_and_reject_binary_or_oversized_edits() {
        let boundary = "é".repeat(MAX_DIFF_BYTES / 2);
        assert_eq!(draft_issue(boundary.as_bytes(), MAX_DIFF_BYTES), None);
        assert_eq!(
            draft_issue(format!("{boundary}x").as_bytes(), MAX_DIFF_BYTES),
            Some(DraftIssue::Bytes)
        );
        assert_eq!(
            draft_issue(&vec![b'\n'; MAX_DIFF_LINES], MAX_DIFF_BYTES),
            None
        );
        assert_eq!(
            draft_issue(&vec![b'\n'; MAX_DIFF_LINES + 1], MAX_DIFF_BYTES),
            Some(DraftIssue::Lines)
        );
        assert_eq!(
            draft_issue(b"resolution\0hidden", MAX_DIFF_BYTES),
            Some(DraftIssue::Nul)
        );
        assert_eq!(
            draft_issue(b"resolution", 4),
            Some(DraftIssue::RetainedBudget)
        );
    }

    #[test]
    fn paste_bounds_apply_to_the_result_after_replacing_a_selection() {
        let original = "é".repeat(MAX_DIFF_BYTES / 2);
        assert_eq!(
            paste_issue(
                &original,
                0..original.len(),
                "replacement\n",
                MAX_DIFF_BYTES
            ),
            None
        );
        assert_eq!(
            paste_issue(&original, 0..0, "é", MAX_DIFF_BYTES),
            Some(DraftIssue::Bytes)
        );
        assert_eq!(
            paste_issue("before\nafter", 0..7, "\0", MAX_DIFF_BYTES),
            Some(DraftIssue::Nul)
        );
        assert_eq!(
            paste_issue("saved draft", 0..0, "larger ", 12),
            Some(DraftIssue::RetainedBudget)
        );
        assert_eq!(original.len(), MAX_DIFF_BYTES);
    }

    #[test]
    fn stale_working_edits_and_new_conflicts_refuse_restoration_but_preserve_text() {
        let original = Arc::new(snapshot());
        let draft = Draft {
            identity: ConflictIdentity::from(original.as_ref()),
            text: "keep this manual resolution\n".into(),
        };
        let mut refreshed = original.as_ref().clone();
        refreshed.working.as_mut().unwrap().bytes = b"saved by an external editor\n".to_vec();
        assert!(!draft.matches(&refreshed));
        refreshed.incoming.as_mut().unwrap().oid = "e".repeat(40);
        assert!(!draft.matches(&refreshed));
        refreshed = original.as_ref().clone();
        refreshed.current.as_mut().unwrap().content.mode = "100755".into();
        assert!(!draft.matches(&refreshed));
        refreshed = original.as_ref().clone();
        refreshed.head = Some("f".repeat(40));
        assert!(!draft.matches(&refreshed));
        refreshed = original.as_ref().clone();
        refreshed.path = PathBuf::from("another-file.txt");
        assert!(!draft.matches(&refreshed));
        let weak = Arc::downgrade(&original);
        drop(original);
        assert!(
            weak.upgrade().is_none(),
            "retained draft identities must not keep raw preview buffers alive"
        );
        assert_eq!(draft.text, "keep this manual resolution\n");
    }

    #[test]
    fn absent_side_labels_keep_merge_and_rebase_branch_roles_explicit() {
        let merge = operation_side_labels(
            OperationKind::Merge,
            "feature/local",
            "origin/main",
            Some("1234567890abcdef"),
        );
        assert_eq!(merge[1], "feature/local · current branch");
        assert_eq!(merge[2], "origin/main · incoming changes");
        let rebase = operation_side_labels(
            OperationKind::Rebase,
            "feature/local",
            "origin/main",
            Some("1234567890abcdef"),
        );
        assert_eq!(rebase[1], "origin/main · updated base");
        assert_eq!(rebase[2], "feature/local · replayed commit 1234567");
        let mut snapshot = snapshot();
        snapshot.base = None;
        snapshot.current = None;
        let presentation = Presentation::prepare(snapshot);
        assert_eq!(presentation.label(0), "Common ancestor · file absent");
        assert_eq!(
            presentation.label(1),
            "Checked-out commit aaaaaaa · file absent"
        );
        assert_eq!(presentation.label(2), "main · incoming changes");
    }
}
