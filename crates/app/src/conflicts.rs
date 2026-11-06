use crate::appearance::CodeFont;
use crate::native_accessibility::LiveRegion as _;
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
/// The viewer's block steps, in `shortcuts.rs` notation: the tooltips name
/// these keys and `block_step` matches them.
const PREVIOUS_BLOCK: &str = "primary-alt-up";
const NEXT_BLOCK: &str = "primary-alt-down";

/// Whether `keystroke` steps to the previous block (`Some(true)`), the next
/// (`Some(false)`) or neither.
fn block_step(keystroke: &Keystroke) -> Option<bool> {
    let modifiers = &keystroke.modifiers;
    if !(modifiers.secondary() && modifiers.alt && !modifiers.shift) {
        return None;
    }
    match keystroke.key.as_str() {
        "up" => Some(true),
        "down" => Some(false),
        _ => None,
    }
}

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
    /// The review's body, which scrolls as one where the window is too
    /// short for it, and reveals the control the keyboard moves onto.
    body: crate::focus_reveal::FocusReveal,
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
            body: crate::focus_reveal::FocusReveal::new(crate::focus_reveal::Trigger::Keyboard),
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
        // The result draft opens with the view but takes no focus: the view
        // is built wherever a conflicted preview lands, a quiet refresh's
        // among them, and a passive read never moves focus. Opened, it took
        // focus out of the file list, closing a row's keyboard menu open
        // there. Whoever builds the view for the person's choice of the file
        // focuses the draft ([`GitTurtle::ensure_conflict_view`]).
        if this.initial_draft.is_some() || this.block_mode {
            this.build_resolution_editor(window, cx);
            if this.initial_draft.is_some() {
                this.schedule_blocks(window, cx);
            }
        }
        this
    }

    /// Whether the resolution draft holds the window's focus.
    #[cfg(test)]
    pub(crate) fn resolution_focused(&self, window: &Window, cx: &App) -> bool {
        self.resolution
            .as_ref()
            .is_some_and(|editor| editor.focus_handle(cx).is_focused(window))
    }

    /// Opens the result draft, if it is not open, and focuses it: Edit
    /// result, a block step and Restore saved draft, each asked for.
    fn open_resolution_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.build_resolution_editor(window, cx);
        self.focus_resolution(window, cx);
    }

    /// Focuses the result draft, if it is open, and has the review's body
    /// show it whole in the next frame, as the person's action that moved
    /// focus there asked: Next or Previous, Edit result, Restore saved draft
    /// and opening the file by its row, by a click or a key alike. Revealed
    /// only for the keyboard, a click on Next in a 461 × 490 window left the
    /// draft focused below the body's fold, about 15 px of it in view.
    fn focus_resolution(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = &self.resolution {
            self.body.reveal_next_focus();
            window.focus(&editor.focus_handle(cx), cx);
        }
        cx.notify();
    }

    /// Opens the result draft, if it is not open, leaving focus where it is.
    fn build_resolution_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn render_side(&self, side: usize, room: Pixels, cx: &mut Context<Self>) -> AnyElement {
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
            .child(div().flex_1().min_h_0().when_some(self.readers[side].as_ref().map(|reader| reader.focus_handle(cx)), |pane, focus| pane.on_children_prepainted(self.body.control(focus, room))).children(self.readers[side].as_ref().filter(|_| !self.block_mode || !self.block_pending).map(|reader| crate::editor_find::Editor::new(reader).readonly(true).bordered(false).h(relative(1.)).text_size(crate::appearance::code_text()).aria_label(format!("Conflict version: {}", self.presentation.label(side))))))
            .when(self.block_mode && self.block_pending, |el| el.child(div().p_4().text_size(crate::appearance::ui_text(12.)).child("Updating conflict blocks…")))
            .when(self.readers[side].is_none(), |el| el.child(div().p_4().text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)).child(if self.block_mode && self.blocks.is_empty() { "No unresolved blocks remain. Review the result, then explicitly save or stage it." } else if self.block_mode && side == 0 { "This marker style does not include a base block. Choose Whole file to inspect the full common ancestor." } else if versions[side].is_none() { "File absent in this version" } else { "Binary or large content · choose a complete version or use an external editor." })))
            .into_any_element()
    }
}

impl Render for ConflictView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                "{} · selected {} · result line {}",
                crate::counted(self.blocks.len(), "unresolved block", "unresolved blocks"),
                self.block_selected + 1,
                self.blocks[self.block_selected].first_line
            )
        };
        // Where the window is too short for the review, its body scrolls as
        // one, and a control the keyboard moves onto, a Button, a version's
        // text or the result draft, scrolls into view whole with its ring's
        // room in the frame that first draws the focus. Each control reports
        // where it lies to the body.
        let room = crate::appearance::button_ring_room(cx);
        self.body.reveal(window, cx);
        let [drafts, branches, base, block_mode, whole_mode, external] = [
            "saved-conflict-drafts",
            "conflict-branches",
            "conflict-base",
            "conflict-block-mode",
            "conflict-whole-mode",
            "external-conflict-editor",
        ]
        .map(|id| self.body.focus(id, cx));
        let [previous, next, accept_current, accept_incoming, accept_both] = [
            "previous-conflict-block",
            "next-conflict-block",
            "accept-current-block",
            "accept-incoming-block",
            "accept-both-block",
        ]
        .map(|id| self.body.focus(id, cx));
        let [
            copy_saved,
            restore_saved,
            use_current,
            use_incoming,
            edit_result,
        ] = [
            "copy-saved-conflict",
            "restore-saved-conflict",
            "resolve-current",
            "resolve-incoming",
            "manual-resolution",
        ]
        .map(|id| self.body.focus(id, cx));
        let [stage_working, save_draft, save_result] = [
            "mark-conflict-resolved",
            "save-conflict-draft",
            "save-conflict-resolution",
        ]
        .map(|id| self.body.focus(id, cx));
        let draft_focus = self
            .resolution
            .as_ref()
            .map(|editor| editor.focus_handle(cx));
        let sides = if self.base {
            vec![self.render_side(0, room, cx)]
        } else {
            vec![self.render_side(1, room, cx), self.render_side(2, room, cx)]
        };
        // Its scrollbar, while it overflows, runs in a gutter at the body's
        // right, clear of the controls and their rings; along a track too
        // short for a thumb, the body's edges fade instead (`ScrollCue`).
        let cue = crate::tags::ScrollCue::revealing(&self.body, Pixels::ZERO);
        let body = div().on_children_prepainted(cue.observe(self.body.observe())).id("conflict-review").debug_selector(|| "conflict-review".into()).flex().flex_col()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if let Some(previous) = block_step(&event.keystroke) {
                    this.select_block(previous, window, cx); cx.stop_propagation();
                }
            }))
            .child(div().px_3().py_2().flex().flex_wrap().items_center().gap_2().border_b_1().border_color(rgb(p.border))
                .on_children_prepainted(self.body.controls(vec![Some(drafts.clone()), Some(branches.clone()), Some(base.clone()), Some(block_mode.clone()), Some(whole_mode.clone()), None, Some(external.clone())], room))
                .child(button("saved-conflict-drafts", "Saved drafts…", "", false).track_focus(&drafts).debug_selector(|| "saved-conflict-drafts".into()).on_click(cx.listener(|_, _, _, cx| cx.emit(ConflictEvent::RecoveryDrafts))))
                .child(button("conflict-branches", "Both sides", "", !self.base).track_focus(&branches).debug_selector(|| "conflict-branches".into()).toggled(!self.base).on_click(cx.listener(|this, _, window, cx| { this.base = false; this.prepare_readers(window, cx); cx.notify(); })))
                .child(button("conflict-base", "Base", "", self.base).track_focus(&base).debug_selector(|| "conflict-base".into()).toggled(self.base).on_click(cx.listener(|this, _, window, cx| { this.base = true; this.prepare_readers(window, cx); cx.notify(); })))
                .child(button("conflict-block-mode", "Selected block", "", self.block_mode).track_focus(&block_mode).debug_selector(|| "conflict-block-mode".into()).toggled(self.block_mode).disabled(self.blocks.is_empty()).on_click(cx.listener(|this, _, window, cx| { this.block_mode = true; this.readers = [None, None, None]; this.prepare_readers(window, cx); cx.notify(); })))
                .child(button("conflict-whole-mode", "Whole file", "", !self.block_mode).track_focus(&whole_mode).debug_selector(|| "conflict-whole-mode".into()).toggled(!self.block_mode).on_click(cx.listener(|this, _, window, cx| { this.block_mode = false; this.readers = [None, None, None]; this.prepare_readers(window, cx); cx.notify(); })))
                .child(div().flex_1())
                .child(button("external-conflict-editor", "Open in editor", "", false).track_focus(&external).debug_selector(|| "external-conflict-editor".into()).tooltip("Open the working file with its default application. Refresh after saving, then stage the resolved file.")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ConflictEvent::OpenEditor)))))
            .when(self.presentation.result.is_some(), |element| element.child(div().px_3().py_2().flex().flex_col().gap_2().border_b_1().border_color(rgb(p.border))
                .child(div().id("conflict-block-status").role(Role::Status).polite().aria_label(status.clone()).text_size(crate::appearance::ui_text(12.)).child(status))
                .child(div().flex().flex_wrap().gap_2().items_center()
                    .on_children_prepainted(self.body.controls(vec![Some(previous.clone()), Some(next.clone()), Some(accept_current.clone()), Some(accept_incoming.clone()), Some(accept_both.clone())], room))
                    .child(button("previous-conflict-block", "Previous", "", false).track_focus(&previous).debug_selector(|| "previous-conflict-block".into()).accessibility_label("Previous conflict block").tooltip(format!("Previous conflict · {}", shortcuts::key_label(PREVIOUS_BLOCK))).disabled(blocked || self.block_selected == 0).on_click(cx.listener(|this, _, window, cx| this.select_block(true, window, cx))))
                    .child(button("next-conflict-block", "Next", "", false).track_focus(&next).debug_selector(|| "next-conflict-block".into()).accessibility_label("Next conflict block").tooltip(format!("Next conflict · {}", shortcuts::key_label(NEXT_BLOCK))).disabled(blocked || self.block_selected + 1 >= self.blocks.len()).on_click(cx.listener(|this, _, window, cx| this.select_block(false, window, cx))))
                    .child(button("accept-current-block", "Accept current", "", false).track_focus(&accept_current).debug_selector(|| "accept-current-block".into()).accessibility_label(format!("Accept current block from {current} into the result draft")).tooltip(format!("Use only this block from {current} in the result draft")).disabled(blocked).on_click(cx.listener(|this, _, window, cx| this.accept_block(ConflictBlockChoice::Current, window, cx))))
                    .child(button("accept-incoming-block", "Accept incoming", "", false).track_focus(&accept_incoming).debug_selector(|| "accept-incoming-block".into()).accessibility_label(format!("Accept incoming block from {incoming} into the result draft")).tooltip(format!("Use only this block from {incoming} in the result draft")).disabled(blocked).on_click(cx.listener(|this, _, window, cx| this.accept_block(ConflictBlockChoice::Incoming, window, cx))))
                    .child(button("accept-both-block", "Accept both", "", false).track_focus(&accept_both).debug_selector(|| "accept-both-block".into()).accessibility_label("Accept both versions of this block, current then incoming, into the result draft").tooltip("Keep current, then incoming, for this block; review ordering in the result draft").disabled(blocked).on_click(cx.listener(|this, _, window, cx| this.accept_block(ConflictBlockChoice::Both, window, cx)))))
                .children(self.block_issue.as_ref().map(|issue| div().id("conflict-block-issue").alert(issue.clone()).text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.warning)).child(issue.clone())))))
            // The versions keep their 100 px however short the window: the
            // body scrolls instead.
            .child(div().flex_1().min_h(px(100.)).flex().debug_selector(|| "conflict-versions".into()).children(sides))
            .child(div().px_3().py_2().flex().flex_col().gap_2().bg(rgb(p.subtle)).border_t_1().border_color(rgb(p.border))
                .child(div().id("conflict-draft-storage-status").role(Role::Status).aria_label(self.durable_status.clone()).polite().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)).child(self.durable_status.clone()))
                .when_some(self.durable_saved.clone(), |element, (valid, text)| {
                    let copy = text.clone();
                    // Keyed by this file's draft.
                    let mark = copy_feedback::Mark::new("copy-saved-conflict", &self.durable_key);
                    let copy_mark = mark.clone();
                    element.child(div().flex().flex_wrap().items_center().gap_2()
                        .on_children_prepainted(self.body.controls(vec![None, Some(copy_saved.clone()), Some(restore_saved.clone())], room))
                        .child(div().text_size(crate::appearance::ui_text(11.)).text_color(rgb(if valid { p.muted } else { p.warning })).child(if valid { "Saved draft matches this operation, index stages and working file." } else { "Saved draft belongs to changed source state. Recover its text without replacing this file." }))
                        .child(copy_feedback::copy_button(mark, "Copy saved text", "", cx).track_focus(&copy_saved).debug_selector(|| "copy-saved-conflict".into()).on_click(move |_, window, cx| copy_feedback::copy(window, cx, Some(copy_mark.clone()), "saved text", copy.clone())))
                        .child(button("restore-saved-conflict", "Restore saved draft", "", false).track_focus(&restore_saved).debug_selector(|| "restore-saved-conflict".to_string()).disabled(!valid).on_click(cx.listener(move |this, _, window, cx| {
                            this.restore_saved_draft(window, cx);
                        }))))
                })
                .child(div().flex().flex_wrap().items_center().gap_2()
                    .on_children_prepainted(self.body.controls(vec![None, Some(use_current.clone()), Some(use_incoming.clone()), None, Some(edit_result.clone())], room))
                    .child(div().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)).child("Whole-file actions:"))
                    .child(button("resolve-current", "Use current file…", "", false).track_focus(&use_current).debug_selector(|| "resolve-current".into()).accessibility_label(format!("Use current whole file from {current}, then review before replacing and staging")).tooltip(format!("Replace and stage the entire file with {current}"))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(ConflictEvent::Resolve(ConflictResolution::Current)))))
                    .child(button("resolve-incoming", "Use incoming file…", "", false).track_focus(&use_incoming).debug_selector(|| "resolve-incoming".into()).accessibility_label(format!("Use incoming whole file from {incoming}, then review before replacing and staging")).tooltip(format!("Replace and stage the entire file with {incoming}"))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(ConflictEvent::Resolve(ConflictResolution::Incoming)))))
                    .child(div().flex_1())
                    .child(button("manual-resolution", "Edit result", "", self.resolution.is_some()).track_focus(&edit_result).debug_selector(|| "manual-resolution".into()).disabled(self.presentation.result.is_none())
                        .on_click(cx.listener(|this, _, window, cx| this.open_resolution_editor(window, cx)))))
                .children(self.resolution.as_ref().zip(draft_focus).map(|(editor, focus)| div().capture_action(cx.listener(Self::check_paste))
                    .child(div().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)).child("Result draft · block choices and manual edits stay here until you explicitly save"))
                    .child(div().debug_selector(|| "conflict-result-draft".into()).on_children_prepainted(self.body.control(focus, room)).child(resolution_textarea(editor, cx)))))
                .children(self.draft_error.map(|issue| div().id("conflict-draft-error").alert(issue.explanation()).text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.warning)).child(issue.explanation())))
                .child(div().flex().flex_wrap().items_center().gap_2()
                    .on_children_prepainted(self.body.controls(vec![Some(stage_working.clone()), self.resolution.as_ref().map(|_| save_draft.clone()), self.resolution.as_ref().map(|_| save_result.clone())], room))
                    .child(button("mark-conflict-resolved", "Stage edited file…", "", false).track_focus(&stage_working).debug_selector(|| "mark-conflict-resolved".into()).disabled(cannot_stage_working).tooltip("Stage the saved working file. Refresh after external editing.").on_click(cx.listener(|_, _, _, cx| cx.emit(ConflictEvent::Resolve(ConflictResolution::MarkResolved)))))
                    .children(self.resolution.as_ref().map(|_| button("save-conflict-draft", "Save draft…", "", false).track_focus(&save_draft).debug_selector(|| "save-conflict-draft".into()).on_click(cx.listener(|this, _, _, cx| {
                        cx.emit(ConflictEvent::Resolve(ConflictResolution::Save { bytes: this.accepted_draft.as_bytes().to_vec() }));
                    }))))
                    .children(self.resolution.as_ref().map(|_| button("save-conflict-resolution", "Save and stage result…", "", false).track_focus(&save_result).debug_selector(|| "save-conflict-resolution".to_string()).disabled(cannot_stage_draft).on_click(cx.listener(|this, _, _, cx| {
                        cx.emit(ConflictEvent::Resolve(ConflictResolution::Manual { bytes: this.accepted_draft.as_bytes().to_vec() }));
                    }))))));
        // The body is the column itself, so where everything fits the
        // versions take the room the rest leaves, as they did before it
        // scrolled, and where it does not they keep their 100 px and the rest
        // its own height, and the column scrolls.
        let body = body
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(self.body.scroll());
        cue.list(body, "conflict-review-scrollbar").size_full()
    }
}

/// The whole lines the manual result draft shows at every code size: as
/// many as its fixed 160 px box held whole at the default code size, where
/// the field's 16 px pitch, taken from the interface size, left a ninth line
/// cut at its foot.
const DRAFT_LINES: f32 = 8.;

/// The code editor's line height, a multiple of the code text (the kit's
/// `EDITOR_LINE_HEIGHT`): the pitch the read-only versions above the draft
/// draw their lines at.
const CODE_LINE_HEIGHT: f32 = 1.5;

/// The manual result draft's editable text, drawn in the code font at the
/// code size's own line height, [`DRAFT_LINES`] whole lines tall inside the
/// field's padding and border, so it grows with the code size and its lines
/// neither crowd nor overlap: a fixed 160 px box with the interface size's
/// pitch drew 18 pt code at a 16 px pitch. A longer draft scrolls inside it.
fn resolution_textarea(editor: &Entity<TextareaState>, cx: &App) -> Textarea {
    let line = (crate::appearance::code_text() * CODE_LINE_HEIGHT).round();
    // The multi-line field's padding above and below its lines, the kit's
    // medium `input_py` at every size, and its 1 px border.
    let inset = gpui_kit::component::Size::default().input_py() + px(1.);
    Textarea::new(editor)
        .h(line * DRAFT_LINES + inset * 2.)
        .text_size(crate::appearance::code_text())
        .line_height(line)
        .code_font(cx)
        .aria_label("Manual conflict resolution result")
}

impl GitTurtle {
    /// Builds the conflict view for `presentation` unless one is open.
    /// `activated` says the preview answers the person's explicit choice of
    /// the file, a click on its row or Enter on it
    /// ([`GitTurtle::activate_working_preview`]): the view's result draft,
    /// where it opens, takes focus. Any other preview shows the view and
    /// leaves focus where it is: one a passive read lands, as a quiet
    /// refresh or a write's status read does finding the selected file
    /// conflicted, and one an arrow key's selection lands, which selects
    /// rather than activates, so the next arrow moves on through the list
    /// rather than the draft's caret.
    pub(super) fn ensure_conflict_view(
        &mut self,
        presentation: Arc<Presentation>,
        activated: bool,
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
        view.update(cx, |view, cx| {
            view.durable_status = self.recovery_drafts.status_for(&persistent_key);
            if let Some(recovered) = recovered {
                view.durable_saved = Some((recovered.key == persistent_key, recovered.text));
            }
            if activated {
                view.focus_resolution(window, cx);
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

    /// Activates the open conflict view, as the person's choice of its file
    /// does: its result draft, where it is open, takes focus. Returns
    /// whether a view was open.
    pub(super) fn activate_conflict_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(view) = self.conflict_view.clone() else {
            return false;
        };
        view.update(cx, |view, cx| view.focus_resolution(window, cx));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::tests::{draw as settle, painted_fades, painted_thumb, rendered};
    use core::prelude::v1::test;
    use gitturtle_core::ConflictSide;
    use gpui_kit::component::scroll::Scrollbar;
    use std::{cell::RefCell, rc::Rc};

    /// The manual result draft is code: it draws in the code family with its
    /// features, as the read-only panes' editors do.
    #[gpui::test]
    fn resolution_textarea_uses_the_code_font(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            gpui_kit::component::Theme::global_mut(cx).mono_font_family = "Desktop Mono".into();
        });
        let cx = cx.add_empty_window();
        let text = cx.update(|window, cx| {
            let editor = cx.new(|cx| TextareaState::new(window, cx));
            resolution_textarea(&editor, cx).style().text.clone()
        });
        assert_eq!(text.font_family.as_deref(), Some("Desktop Mono"));
        let features = text.font_features.expect("code features");
        assert_eq!(features.is_calt_enabled(), Some(false));
        assert!(features.tag_value_list().contains(&("liga".into(), 0)));
    }

    /// A conflict view over the working file `text`, alone in its window,
    /// with its result draft open.
    fn conflict_window(
        cx: &mut TestAppContext,
        text: String,
    ) -> (Entity<ConflictView>, &mut VisualTestContext) {
        cx.update(gpui_kit::init);
        let captured = Rc::new(RefCell::new(None));
        let observed = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let mut snapshot = snapshot();
            snapshot.working.as_mut().unwrap().bytes = text.into_bytes();
            let presentation = Arc::new(Presentation::prepare(snapshot));
            let key = recovery_drafts::Key::conflict(
                std::path::Path::new("isolated-conflict-fixture"),
                &presentation.snapshot.path,
                presentation.identity.durable.clone(),
            );
            let view =
                cx.new(|cx| ConflictView::new(presentation, key, None, MAX_DIFF_BYTES, window, cx));
            *captured.borrow_mut() = Some(view.clone());
            gpui_kit::component::Root::new(view, window, cx)
        });
        let view = observed.borrow_mut().take().unwrap();
        (view, cx)
    }

    fn draw(cx: &mut VisualTestContext) {
        for _ in 0..2 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }
    }

    /// A review of a two-block conflict with its result draft open and a
    /// matching saved draft offered, so every one of its controls is drawn.
    fn review_window(cx: &mut TestAppContext) -> (Entity<ConflictView>, &mut VisualTestContext) {
        let mut text = String::new();
        for _ in 0..2 {
            text.push_str("<<<<<<< current\ncurrent\n=======\nincoming\n>>>>>>> incoming\n");
            for line in 0..20 {
                text.push_str(&format!("line {line}\n"));
            }
        }
        let (view, cx) = conflict_window(cx, text);
        cx.update(|_, cx| {
            crate::appearance::ThemeChoice::Midnight.apply(None, cx);
            view.update(cx, |view, cx| {
                view.durable_saved = Some((true, "saved\n".into()));
                cx.notify();
            })
        });
        (view, cx)
    }

    /// The review's controls, by the debug selector each is drawn under,
    /// which is also the id its Button tracks focus under.
    const REVIEW_CONTROLS: [&str; 19] = [
        "saved-conflict-drafts",
        "conflict-branches",
        "conflict-base",
        "conflict-block-mode",
        "conflict-whole-mode",
        "external-conflict-editor",
        "previous-conflict-block",
        "next-conflict-block",
        "accept-current-block",
        "accept-incoming-block",
        "accept-both-block",
        "copy-saved-conflict",
        "restore-saved-conflict",
        "resolve-current",
        "resolve-incoming",
        "manual-resolution",
        "mark-conflict-resolved",
        "save-conflict-draft",
        "save-conflict-resolution",
    ];

    /// The control of the review holding focus: a Button by its selector,
    /// the result draft as `conflict-result-draft`, a version's text as
    /// `base-version`, `current-version` or `incoming-version`.
    fn focused_control(
        cx: &mut VisualTestContext,
        view: &Entity<ConflictView>,
    ) -> Option<&'static str> {
        cx.update(|window, cx| {
            let view = view.read(cx);
            if let Some(id) = view.body.focused(window) {
                return REVIEW_CONTROLS
                    .into_iter()
                    .find(|control| ElementId::from(*control) == id);
            }
            if view.resolution_focused(window, cx) {
                return Some("conflict-result-draft");
            }
            view.readers.iter().enumerate().find_map(|(side, reader)| {
                reader
                    .as_ref()
                    .filter(|reader| reader.focus_handle(cx).contains_focused(window, cx))
                    .map(|_| ["base-version", "current-version", "incoming-version"][side])
            })
        })
    }

    /// With the body at its top and focus on its first control, presses
    /// `key` until focus reaches the result draft, which keeps Tab and
    /// Shift+Tab for indenting, or comes back to a control it has visited,
    /// and returns the controls focus lay on in order, the first control
    /// among them for Tab. After each press the focused control, grown by its
    /// ring's room, lies inside the review's body as drawn, and so it does in
    /// every frame painted after the press, the first that draws the focus
    /// included.
    fn walk(
        cx: &mut VisualTestContext,
        view: &Entity<ConflictView>,
        key: &str,
        at: &str,
    ) -> Vec<&'static str> {
        const FIRST: &str = "saved-conflict-drafts";
        let body = cx.read(|cx| view.read(cx).body.clone());
        let room = cx.read(crate::appearance::button_ring_room);
        let slack = px(0.01);
        cx.update(|window, cx| {
            body.scroll().set_offset(Point::default());
            body.focus(FIRST, cx).focus(window, cx);
        });
        settle(cx);
        assert_eq!(focused_control(cx, view), Some(FIRST), "{at}");
        let mut visited = Vec::new();
        if key == "tab" {
            visited.push(FIRST);
        }
        for _ in 0..REVIEW_CONTROLS.len() + 4 {
            body.take_painted();
            cx.simulate_keystrokes(key);
            settle(cx);
            let control = focused_control(cx, view)
                .unwrap_or_else(|| panic!("{at}: {key} focuses a control of the review"));
            if visited.contains(&control) {
                break;
            }
            let review = cx.debug_bounds("conflict-review").expect("the review");
            let drawn = match control {
                "conflict-result-draft" => cx.debug_bounds(control),
                control if control.ends_with("-version") => None,
                control => cx.debug_bounds(control),
            };
            if let Some(drawn) = drawn {
                let ring = drawn.dilate(room);
                assert!(
                    ring.top() + slack >= review.top() && ring.bottom() <= review.bottom() + slack,
                    "{at}: after {key}, {control} with its ring, {ring:?}, lies outside the \
                     review's body, {review:?}"
                );
            }
            let frames = body.take_painted();
            assert!(
                !frames.is_empty(),
                "{at}: {key} onto {control} paints the body"
            );
            for (index, frame) in frames.iter().enumerate() {
                let ring = frame
                    .control
                    .unwrap_or_else(|| panic!("{at}: frame {index} after {key} draws {control}"))
                    .dilate(room);
                assert!(
                    ring.top() + slack >= frame.viewport.top()
                        && ring.bottom() <= frame.viewport.bottom() + slack,
                    "{at}: frame {index} after {key} paints {control} with its ring, {ring:?}, \
                     outside the body, {:?}",
                    frame.viewport
                );
            }
            visited.push(control);
            if control == "conflict-result-draft" {
                break;
            }
        }
        visited
    }

    /// Every control Tab or Shift+Tab reaches in a short window is drawn
    /// whole with its ring inside the review's body, which scrolls, in every
    /// frame after the key. In a 461 × 490 half tile, at 13 pt with 12 pt
    /// code and at 18 pt with 18 pt code, the review is taller than the
    /// window: laid out at the window's height, it left Save draft… at
    /// y 643.5 and 1,008.5 and every action below the result draft under the
    /// window's foot, out of reach, and the result draft cut by it. The body
    /// scrolls as one, the versions keeping their 100 px and the draft its
    /// eight whole lines, and its scrollbar runs in a gutter at its right,
    /// clear of every control and ring. Once the draft has no unresolved
    /// block, Shift+Tab reaches Save and stage result… whole too.
    #[gpui::test]
    async fn a_short_window_scrolls_the_review_and_tab_reveals_each_control(
        cx: &mut TestAppContext,
    ) {
        cx.executor().allow_parking();
        let (view, cx) = review_window(cx);
        cx.simulate_resize(size(px(461.), px(490.)));
        let body = cx.read(|cx| view.read(cx).body.clone());
        let room = cx.read(crate::appearance::button_ring_room);
        for (interface, code, pitch) in [(13, 12, px(18.)), (18, 18, px(27.))] {
            let at = format!("{interface} pt, code {code} pt, two blocks unresolved");
            cx.update(|window, cx| {
                crate::appearance::apply_text_sizes(interface, code, window, cx);
                body.scroll().set_offset(Point::default());
            });
            settle(cx);
            assert!(
                body.scroll().max_offset().y > px(0.),
                "{at}: the review is taller than its body"
            );
            assert_eq!(
                rendered(cx, "conflict-versions").size.height,
                px(100.),
                "{at}: the versions keep their 100 px"
            );
            assert_eq!(
                rendered(cx, "conflict-result-draft").size.height,
                pitch * 8. + px(18.),
                "{at}: the draft keeps its eight whole lines"
            );

            // The scrollbar runs the body's height in its gutter, at its
            // right, and the controls and their rings end before it.
            let review = rendered(cx, "conflict-review");
            assert!(
                cx.debug_bounds("conflict-review-scrollbar").is_some(),
                "{at}: the body draws its scrollbar"
            );
            let track = Bounds::from_corners(
                point(review.right() - Scrollbar::width(), review.top()),
                review.bottom_right(),
            );
            assert!(
                painted_thumb(cx, track).is_some(),
                "{at}: the scrollbar paints its thumb at the body's right, in {track:?}"
            );
            for control in REVIEW_CONTROLS
                .into_iter()
                .chain(["conflict-result-draft", "conflict-versions"])
            {
                let ring = rendered(cx, control).dilate(room);
                assert!(
                    ring.right() <= track.left() + px(0.01),
                    "{at}: {control} with its ring, {ring:?}, reaches the scrollbar, {track:?}"
                );
            }

            let forward = walk(cx, &view, "tab", &at);
            assert_eq!(
                forward,
                [
                    "saved-conflict-drafts",
                    "conflict-branches",
                    "conflict-base",
                    "conflict-block-mode",
                    "conflict-whole-mode",
                    "external-conflict-editor",
                    "next-conflict-block",
                    "accept-current-block",
                    "accept-incoming-block",
                    "accept-both-block",
                    "current-version",
                    "incoming-version",
                    "copy-saved-conflict",
                    "restore-saved-conflict",
                    "resolve-current",
                    "resolve-incoming",
                    "manual-resolution",
                    "conflict-result-draft",
                ],
                "{at}: Tab's stops"
            );
            assert!(
                body.scroll().offset().y < px(0.),
                "{at}: Tab scrolled the body"
            );
            let backward = walk(cx, &view, "shift-tab", &at);
            assert_eq!(
                backward,
                ["save-conflict-draft", "conflict-result-draft"],
                "{at}: Shift+Tab's stops"
            );
        }

        // The saved draft, which holds no conflict, restored as Restore
        // saved draft does: once it is parsed, Save and stage result… acts.
        cx.update(|window, cx| view.update(cx, |view, cx| view.restore_saved_draft(window, cx)));
        let parse = cx.update(|_, cx| view.update(cx, |view, _| view.block_task.take()));
        parse.expect("restoring parses the draft").await;
        for (interface, code) in [(13, 12), (18, 18)] {
            let at = format!("{interface} pt, code {code} pt, resolved");
            cx.update(|window, cx| {
                crate::appearance::apply_text_sizes(interface, code, window, cx)
            });
            settle(cx);
            assert!(
                body.scroll().max_offset().y > px(0.),
                "{at}: the review is taller than its body"
            );
            assert_eq!(
                walk(cx, &view, "tab", &at),
                [
                    "saved-conflict-drafts",
                    "conflict-branches",
                    "conflict-base",
                    "conflict-whole-mode",
                    "external-conflict-editor",
                    "resolve-current",
                    "resolve-incoming",
                    "manual-resolution",
                    "conflict-result-draft",
                ],
                "{at}: Tab's stops"
            );
            assert_eq!(
                walk(cx, &view, "shift-tab", &at),
                [
                    "save-conflict-resolution",
                    "save-conflict-draft",
                    "conflict-result-draft"
                ],
                "{at}: Shift+Tab's stops"
            );
        }
        cx.update(|window, cx| crate::appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A click that moves focus into the result draft, on Next or Previous or
    /// on Edit result, has the body show the draft whole with its ring's
    /// room in a 461 × 490 window, at 13 pt with 12 pt code and at 18 pt with
    /// 18 pt code: the draft, 162 and 234 px tall, is shorter than the
    /// 490 px body at both, so neither shows only its top. Revealed only for
    /// the keyboard, a click on Next at 13 pt focused the draft at y 475–637
    /// with the body still at its top, about 15 px of it in view.
    #[gpui::test]
    fn a_click_that_focuses_the_draft_shows_it_whole(cx: &mut TestAppContext) {
        let (view, cx) = review_window(cx);
        cx.simulate_resize(size(px(461.), px(490.)));
        let body = cx.read(|cx| view.read(cx).body.clone());
        let room = cx.read(crate::appearance::button_ring_room);
        let slack = px(0.01);
        let assert_draft_shown = |cx: &mut VisualTestContext, after: &str| {
            assert_eq!(
                focused_control(cx, &view),
                Some("conflict-result-draft"),
                "{after} focuses the draft"
            );
            let review = rendered(cx, "conflict-review");
            let ring = rendered(cx, "conflict-result-draft").dilate(room);
            assert!(
                ring.top() + slack >= review.top() && ring.bottom() <= review.bottom() + slack,
                "after {after}, the draft with its ring, {ring:?}, lies outside the body, \
                 {review:?}, at offset {:?}",
                body.scroll().offset().y
            );
        };
        for (interface, code, step) in [
            (13, 12, "next-conflict-block"),
            (18, 18, "previous-conflict-block"),
        ] {
            let at = format!("{interface} pt, code {code} pt");
            // Focus elsewhere and the body at its top, where the draft lies
            // past its foot.
            cx.update(|window, cx| {
                crate::appearance::apply_text_sizes(interface, code, window, cx);
                window.blur(cx);
                body.scroll().set_offset(Point::default());
            });
            settle(cx);
            let review = rendered(cx, "conflict-review");
            assert!(
                rendered(cx, "conflict-result-draft").bottom() > review.bottom(),
                "{at}: the draft lies past the body's foot"
            );
            let center = rendered(cx, step).center();
            cx.simulate_click(center, Modifiers::default());
            settle(cx);
            assert_draft_shown(cx, &format!("{at}: a click on {step}"));

            // Edit result scrolled to the body's foot, the draft below it.
            cx.update(|window, cx| window.blur(cx));
            settle(cx);
            let edit = rendered(cx, "manual-resolution");
            let offset = body.scroll().offset().y - (edit.bottom() + px(4.) - review.bottom());
            cx.update(|window, _| {
                body.scroll().set_offset(point(px(0.), offset.min(px(0.))));
                window.refresh();
            });
            settle(cx);
            assert!(
                rendered(cx, "conflict-result-draft").bottom() > review.bottom(),
                "{at}: below Edit result, the draft lies past the body's foot"
            );
            let center = rendered(cx, "manual-resolution").center();
            cx.simulate_click(center, Modifiers::default());
            settle(cx);
            assert_draft_shown(cx, &format!("{at}: a click on Edit result"));
        }
        cx.update(|window, cx| crate::appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Where the review fits, as in a 1,480 × 800 window at 13 pt with 12 pt
    /// code and at 18 pt with 18 pt code, nothing scrolls, the body keeps no
    /// gutter and draws no scrollbar, and the review and every control lie
    /// where they did before the body scrolled, the versions taking the
    /// room the rest leaves.
    #[gpui::test]
    fn a_review_that_fits_scrolls_nothing_and_moves_nothing(cx: &mut TestAppContext) {
        /// An element's debug selector and where it was drawn: x, y, width
        /// and height.
        type Drawn = (&'static str, (f32, f32, f32, f32));
        /// Where each was drawn before the body scrolled (`82f71ae1`), at
        /// 13 pt with 12 pt code.
        const AT_13: [Drawn; 22] = [
            ("conflict-review", (0., 0., 1480., 800.)),
            ("conflict-versions", (0., 110., 1480., 368.)),
            ("saved-conflict-drafts", (9.5, 6.5, 114., 28.)),
            ("conflict-branches", (130., 6.5, 92., 28.)),
            ("conflict-base", (228.5, 6.5, 49., 28.)),
            ("conflict-block-mode", (284., 6.5, 121., 28.)),
            ("conflict-whole-mode", (411.5, 6.5, 92., 28.)),
            ("external-conflict-editor", (1349.5, 6.5, 121., 28.)),
            ("previous-conflict-block", (9.5, 74.5, 78., 28.)),
            ("next-conflict-block", (94., 74.5, 49., 28.)),
            ("accept-current-block", (149.5, 74.5, 121., 28.)),
            ("accept-incoming-block", (277., 74.5, 128., 28.)),
            ("accept-both-block", (411.5, 74.5, 100., 28.)),
            ("copy-saved-conflict", (452., 510., 150., 28.)),
            ("restore-saved-conflict", (608.5, 510., 157., 28.)),
            ("resolve-current", (142., 544.5, 143., 28.)),
            ("resolve-incoming", (291.5, 544.5, 150., 28.)),
            ("manual-resolution", (1370.5, 544.5, 100., 28.)),
            ("conflict-result-draft", (9.5, 597., 1461., 162.)),
            ("mark-conflict-resolved", (9.5, 765.5, 150., 28.)),
            ("save-conflict-draft", (166., 765.5, 100., 28.)),
            ("save-conflict-resolution", (272.5, 765.5, 179., 28.)),
        ];
        /// The same at 18 pt with 18 pt code.
        const AT_18: [Drawn; 22] = [
            ("conflict-review", (0., 0., 1480., 800.)),
            ("conflict-versions", (0., 152., 1480., 193.)),
            ("saved-conflict-drafts", (13.5, 9., 158., 39.)),
            ("conflict-branches", (180.5, 9., 128., 39.)),
            ("conflict-base", (317.5, 9., 68., 39.)),
            ("conflict-block-mode", (394.5, 9., 168., 39.)),
            ("conflict-whole-mode", (571.5, 9., 128., 39.)),
            ("external-conflict-editor", (1298.5, 9., 168., 39.)),
            ("previous-conflict-block", (13.5, 103., 108., 39.)),
            ("next-conflict-block", (130.5, 103., 68., 39.)),
            ("accept-current-block", (207.5, 103., 168., 39.)),
            ("accept-incoming-block", (384.5, 103., 178., 39.)),
            ("accept-both-block", (571.5, 103., 138., 39.)),
            ("copy-saved-conflict", (626.5, 388.5, 208.5, 39.)),
            ("restore-saved-conflict", (844., 388.5, 218., 39.)),
            ("resolve-current", (196.5, 436.5, 198., 39.)),
            ("resolve-incoming", (403.5, 436.5, 208., 39.)),
            ("manual-resolution", (1328.5, 436.5, 138., 39.)),
            ("conflict-result-draft", (13.5, 509., 1453., 234.)),
            ("mark-conflict-resolved", (13.5, 752., 208., 39.)),
            ("save-conflict-draft", (230.5, 752., 138., 39.)),
            ("save-conflict-resolution", (377.5, 752., 248., 39.)),
        ];
        let (view, cx) = review_window(cx);
        cx.simulate_resize(size(px(1480.), px(800.)));
        let body = cx.read(|cx| view.read(cx).body.clone());
        for (interface, code, laid_out) in [(13, 12, AT_13), (18, 18, AT_18)] {
            let at = format!("{interface} pt, code {code} pt");
            cx.update(|window, cx| {
                crate::appearance::apply_text_sizes(interface, code, window, cx)
            });
            settle(cx);
            assert_eq!(
                body.scroll().max_offset().y,
                px(0.),
                "{at}: the review fits its body"
            );
            assert_eq!(body.scroll().offset().y, px(0.), "{at}");
            assert!(
                cx.debug_bounds("conflict-review-scrollbar").is_none(),
                "{at}: a body that fits draws no scrollbar"
            );
            let moved: Vec<_> = laid_out
                .into_iter()
                .filter_map(|(selector, (x, y, width, height))| {
                    let before = Bounds::new(point(px(x), px(y)), size(px(width), px(height)));
                    let drawn = cx.debug_bounds(selector);
                    (drawn != Some(before))
                        .then(|| format!("{selector}: {drawn:?}, was {before:?}"))
                })
                .collect();
            assert!(moved.is_empty(), "{at}: moved\n{}", moved.join("\n"));
        }
        cx.update(|window, cx| crate::appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Along a track shorter than a scrollbar's thumb moves on, 56 px, as in
    /// a window 50 px tall, the body draws no scrollbar and fades the edges
    /// it continues past instead: the bottom's at its top, and the top's
    /// alone at its end.
    #[gpui::test]
    fn a_body_too_short_for_a_thumb_fades_the_edges_it_continues_past(cx: &mut TestAppContext) {
        let (view, cx) = review_window(cx);
        cx.simulate_resize(size(px(461.), px(50.)));
        settle(cx);
        let body = cx.read(|cx| view.read(cx).body.clone());
        let review = rendered(cx, "conflict-review");
        assert!(
            body.scroll().max_offset().y > px(0.),
            "the review overflows"
        );
        assert!(
            cx.debug_bounds("conflict-review-scrollbar").is_none(),
            "no scrollbar along a 50 px track"
        );
        assert_eq!(
            painted_fades(cx, review),
            (None, Some(px(8.))),
            "at its top, the bottom edge fades"
        );
        cx.update(|window, _| {
            body.scroll()
                .set_offset(point(px(0.), -body.scroll().max_offset().y));
            window.refresh();
        });
        settle(cx);
        assert_eq!(
            painted_fades(cx, review),
            (Some(px(8.)), None),
            "at its end, the top edge alone fades"
        );
    }

    /// The manual result draft shows eight whole lines of code at every
    /// code size, at the code size's own line height, the pitch of the
    /// versions above it: 18 px at the default 12 px, 20 px at 13 and 27 px
    /// at 18, whatever the interface size. Its fixed 160 px box drew them
    /// at the interface size's 16 px pitch (23 px at 18 pt), so 18 pt code
    /// crowded its lines into each other, and held eight whole lines and a
    /// cut ninth. The box grows with its lines, and a draft longer than
    /// eight lines scrolls inside it.
    #[gpui::test]
    fn the_result_draft_shows_whole_code_lines_at_the_code_size(cx: &mut TestAppContext) {
        let mut text =
            String::from("<<<<<<< current\ncurrent\n=======\nincoming\n>>>>>>> incoming\n");
        for line in 0..40 {
            text.push_str(&format!("line {line}\n"));
        }
        let second_line = text.find('\n').unwrap() + 1;
        let (view, cx) = conflict_window(cx, text);
        cx.simulate_resize(size(px(1480.), px(980.)));
        let editor = cx.read(|cx| view.read(cx).resolution.clone().expect("the draft opens"));
        let caret_at = |offset: usize, cx: &mut VisualTestContext| {
            editor.update(cx, |editor, cx| {
                editor.set_selected_range(offset..offset, cx)
            });
            draw(cx);
            cx.read(|cx| editor.read(cx).cursor_layout().expect("a caret").0)
        };
        for (interface, code, pitch) in [
            (13, 12, px(18.)),
            (13, 13, px(20.)),
            (13, 18, px(27.)),
            (18, 12, px(18.)),
            (18, 18, px(27.)),
        ] {
            let at = format!("{interface} pt, code {code} pt");
            cx.update(|window, cx| {
                crate::appearance::apply_text_sizes(interface, code, window, cx);
                editor.update(cx, |editor, cx| {
                    editor.set_scroll_offset(Point::default(), cx)
                });
            });
            draw(cx);
            let first = caret_at(0, cx);
            let second = caret_at(second_line, cx);
            assert_eq!(
                second.top() - first.top(),
                pitch,
                "{at}: the drawn rows' pitch"
            );
            assert_eq!(
                cx.read(|cx| editor.read(cx).line_height()),
                Some(pitch),
                "{at}"
            );
            let lines = cx.read(|cx| editor.read(cx).input_bounds());
            assert_eq!(
                lines.size.height,
                pitch * 8.,
                "{at}: eight whole lines, none cut"
            );
            let draft = cx.debug_bounds("conflict-result-draft").unwrap();
            assert_eq!(
                draft.size.height,
                pitch * 8. + px(18.),
                "{at}: the lines inside the field's padding and border"
            );

            cx.update(|_, cx| {
                editor.update(cx, |editor, cx| {
                    editor.set_scroll_offset(point(px(0.), px(-100_000.)), cx)
                })
            });
            draw(cx);
            let scrolled = cx.read(|cx| editor.read(cx).scroll_offset().y);
            assert!(
                scrolled < -pitch * 30. && scrolled > px(-100_000.),
                "{at}: the 45-line draft scrolls inside its box, to {scrolled:?}"
            );
            assert_eq!(
                cx.debug_bounds("conflict-result-draft"),
                Some(draft),
                "{at}"
            );
        }
        cx.update(|window, cx| crate::appearance::apply_text_sizes(13, 12, window, cx));
    }

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
    fn the_advertised_block_keys_step_between_blocks() {
        let parse = |key: &str| {
            Keystroke::parse(&key.replace(
                "primary",
                if cfg!(target_os = "macos") {
                    "cmd"
                } else {
                    "ctrl"
                },
            ))
            .unwrap()
        };
        assert_eq!(block_step(&parse(PREVIOUS_BLOCK)), Some(true));
        assert_eq!(block_step(&parse(NEXT_BLOCK)), Some(false));
        assert_eq!(block_step(&parse("alt-down")), None);
        assert_eq!(block_step(&parse("primary-alt-left")), None);
        #[cfg(not(target_os = "macos"))]
        {
            // Super belongs to the desktop on Linux; the tooltip names Ctrl.
            assert_eq!(block_step(&parse("cmd-alt-down")), None);
            assert_eq!(shortcuts::key_label(NEXT_BLOCK), "Ctrl+Alt+Down");
        }
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
