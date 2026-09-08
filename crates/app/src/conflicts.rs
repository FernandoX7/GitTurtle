use crate::*;
use gitturtle_core::{
    ConflictContent, ConflictPreview, ConflictResolution, IntegrationCommand, MAX_DIFF_BYTES,
    MAX_DIFF_LINES, OperationKind, OperationState, WriteCommand,
};
use gpui_kit::component::input::Paste;
use gpui_kit::prelude::FluentBuilder;
use std::ops::Range;

const MAX_RETAINED_DRAFT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone)]
struct ConflictIdentity {
    path: PathBuf,
    head: Option<String>,
    operation: Option<OperationState>,
    sides: [Option<(String, String)>; 3],
}

impl From<&ConflictPreview> for ConflictIdentity {
    fn from(snapshot: &ConflictPreview) -> Self {
        let mut operation = snapshot.operation.clone();
        if let Some(operation) = &mut operation {
            operation.staged_paths = Vec::new();
        }
        Self {
            path: snapshot.path.clone(),
            head: snapshot.head.clone(),
            operation,
            sides: [&snapshot.base, &snapshot.current, &snapshot.incoming].map(|side| {
                side.as_ref()
                    .map(|side| (side.oid.clone(), side.content.mode.clone()))
            }),
        }
    }
}

pub struct Draft {
    identity: ConflictIdentity,
    text: String,
}

impl Draft {
    fn matches(&self, snapshot: &ConflictPreview) -> bool {
        self.identity.path == snapshot.path
            && self.identity.head == snapshot.head
            && match (&self.identity.operation, &snapshot.operation) {
                (Some(old), Some(new)) => old.same_operation(new),
                (None, None) => true,
                _ => false,
            }
            && self
                .identity
                .sides
                .iter()
                .zip([&snapshot.base, &snapshot.current, &snapshot.incoming])
                .all(|(old, new)| {
                    old.as_ref().map(|(oid, mode)| (oid, mode))
                        == new.as_ref().map(|v| (&v.oid, &v.content.mode))
                })
    }
}

pub struct Presentation {
    pub snapshot: Arc<ConflictPreview>,
    sources: [Option<String>; 3],
    labels: [String; 3],
    result: Option<String>,
    identity: ConflictIdentity,
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
        let labels = [&snapshot.base, &snapshot.current, &snapshot.incoming]
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
        Self {
            identity: ConflictIdentity::from(&snapshot),
            snapshot: Arc::new(snapshot),
            sources,
            labels,
            result,
        }
    }

    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.labels.iter().map(String::capacity).sum::<usize>()
            + self
                .sources
                .iter()
                .flatten()
                .map(String::capacity)
                .sum::<usize>()
            + self.result.as_ref().map_or(0, String::capacity)
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
}

pub struct ConflictView {
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
}

impl EventEmitter<ConflictEvent> for ConflictView {}

impl ConflictView {
    pub fn new(
        presentation: Arc<Presentation>,
        initial_draft: Option<String>,
        draft_limit: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
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
        if this.initial_draft.is_some() {
            this.open_resolution_editor(window, cx);
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

    fn prepare_readers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for side in if self.base { vec![0] } else { vec![1, 2] } {
            if self.readers[side].is_none()
                && let Some(source) = &self.presentation.sources[side]
            {
                self.readers[side] = Some(text::editor(source, "text", None, window, cx));
            }
        }
    }

    fn render_side(&self, side: usize, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let versions = [
            &self.presentation.snapshot.base,
            &self.presentation.snapshot.current,
            &self.presentation.snapshot.incoming,
        ];
        let description =
            versions[side]
                .as_ref()
                .map_or("This version has no file.".into(), |version| {
                    format!(
                        "{} · {} · {}",
                        short_oid(&version.oid),
                        format_bytes(version.content.bytes.len()),
                        version.content.mode
                    )
                });
        div().flex_1().min_w_0().h_full().flex().flex_col().border_r_1().border_color(rgb(p.border))
            .child(div().px_3().py_2().flex().flex_col().gap_1().bg(rgb(p.panel))
                .child(div().truncate().text_size(px(12.)).font_weight(FontWeight::MEDIUM).child(self.presentation.label(side)))
                .child(div().truncate().text_size(px(10.)).text_color(rgb(p.muted)).child(description)))
            .child(div().flex_1().min_h_0().children(self.readers[side].as_ref().map(|reader| crate::editor_find::Editor::new(reader).readonly(true).bordered(false).h(relative(1.)).text_size(px(12.)).aria_label(format!("Conflict version: {}", self.presentation.label(side))))))
            .when(self.readers[side].is_none(), |el| el.child(div().p_4().text_size(px(12.)).text_color(rgb(p.muted)).child(if versions[side].is_none() { "File absent in this version" } else { "Binary or large content · choose a complete version or use an external editor." })))
            .into_any_element()
    }
}

impl Render for ConflictView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let current = self.presentation.label(1);
        let incoming = self.presentation.label(2);
        div().size_full().flex().flex_col()
            .child(div().px_3().py_2().flex().items_center().gap_2().border_b_1().border_color(rgb(p.border))
                .child(button("conflict-branches", "Both sides", "", !self.base).on_click(cx.listener(|this, _, window, cx| { this.base = false; this.prepare_readers(window, cx); cx.notify(); })))
                .child(button("conflict-base", "Base", "", self.base).on_click(cx.listener(|this, _, window, cx| { this.base = true; this.prepare_readers(window, cx); cx.notify(); })))
                .child(div().flex_1())
                .child(button("external-conflict-editor", "Open in editor", "", false).tooltip("Open the working file with its default application. Refresh after saving, then stage the resolved file.")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ConflictEvent::OpenEditor)))))
            .child(div().flex_1().min_h(px(100.)).flex().children(if self.base { vec![self.render_side(0, cx)] } else { vec![self.render_side(1, cx), self.render_side(2, cx)] }))
            .child(div().px_3().py_2().flex().flex_col().gap_2().bg(rgb(p.subtle)).border_t_1().border_color(rgb(p.border))
                .child(div().text_size(px(11.)).text_color(rgb(p.muted)).child("Choose a complete version, or edit the resolution and stage this file."))
                .child(div().flex().items_center().gap_2()
                    .child(button("resolve-current", format!("Use {current}…"), "", false).max_w(px(240.)).tooltip("Replace the working file with this complete version and stage it")
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(ConflictEvent::Resolve(ConflictResolution::Current)))))
                    .child(button("resolve-incoming", format!("Use {incoming}…"), "", false).max_w(px(240.)).tooltip("Replace the working file with this complete version and stage it")
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(ConflictEvent::Resolve(ConflictResolution::Incoming)))))
                    .child(div().flex_1())
                    .child(button("manual-resolution", "Edit resolution", "", self.resolution.is_some()).disabled(self.presentation.result.is_none())
                        .on_click(cx.listener(|this, _, window, cx| this.open_resolution_editor(window, cx)))))
                .children(self.resolution.as_ref().map(|editor| div().capture_action(cx.listener(Self::check_paste))
                    .child(Textarea::new(editor).h(px(180.)).text_size(px(12.)).aria_label("Manual conflict resolution"))))
                .children(self.draft_error.map(|issue| div().text_size(px(11.)).text_color(rgb(p.warning)).child(issue.explanation())))
                .child(div().flex().items_center().gap_2()
                    .child(div().flex_1().text_size(px(11.)).text_color(rgb(p.muted)).child("After external editing, refresh this file before staging."))
                    .child(button("mark-conflict-resolved", "Stage edited file…", "", false).on_click(cx.listener(|_, _, _, cx| cx.emit(ConflictEvent::Resolve(ConflictResolution::MarkResolved)))))
                    .children(self.resolution.as_ref().map(|_| button("save-conflict-resolution", "Save and stage resolution…", "", false).on_click(cx.listener(|this, _, _, cx| {
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
        let draft = self
            .conflict_drafts
            .get(&draft_key)
            .filter(|draft| draft.matches(&presentation.snapshot))
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
        let view = cx
            .new(|cx| ConflictView::new(Arc::clone(&presentation), draft, draft_limit, window, cx));
        let snapshot = Arc::clone(&presentation.snapshot);
        let identity = presentation.identity.clone();
        let path = self.path.clone();
        self.conflict_subscription = Some(cx.subscribe_in(&view, window, move |this, _, event, window, cx| {
            if this.path != path { return; }
            // Draft text is local UI state and must survive an unrelated plan
            // read. Only executable actions are disabled while Git is busy.
            if let ConflictEvent::Draft(value) = event {
                this.conflict_drafts.insert(draft_key.clone(), Draft { identity: identity.clone(), text: value.clone() });
                return;
            }
            if this.operation_busy.is_some() { return; }
            match event {
                ConflictEvent::Draft(_) => unreachable!("drafts are saved before the operation guard"),
                ConflictEvent::Resolve(resolution) => {
                    let file = snapshot.path.to_string_lossy();
                    let effect = match resolution {
                        ConflictResolution::Manual { .. } => "Save the text you entered and stage this file.",
                        ConflictResolution::Current | ConflictResolution::Incoming => "Replace the working file with the selected complete version and stage it. Choosing an absent version deletes the file.",
                        ConflictResolution::MarkResolved => "Stage the working file exactly as reviewed. Ensure all conflict markers have been resolved.",
                    };
                    this.confirm_git_write(format!("Resolve {file}"), format!("{effect}\n\nOnly {file} is staged. A newer edit or operation state will require a fresh review."), "Resolve file",
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
    fn drafts_follow_the_conflict_identity_across_working_edits_but_never_new_conflicts() {
        let original = Arc::new(snapshot());
        let draft = Draft {
            identity: ConflictIdentity::from(original.as_ref()),
            text: "keep this manual resolution\n".into(),
        };
        let mut refreshed = original.as_ref().clone();
        refreshed.working.as_mut().unwrap().bytes = b"saved by an external editor\n".to_vec();
        assert!(draft.matches(&refreshed));
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
