//! Native PR file navigation, precise inline selection and a durable collected review.
use super::*;
use crate::appearance::CodeFont;
use crate::github::{
    CapturedPull, LineComment,
    review::{self as model, PullFile, UiTransport},
};

gpui_kit::actions!(
    github_review,
    [
        ReviewUp,
        ReviewDown,
        ReviewHome,
        ReviewEnd,
        ReviewPageUp,
        ReviewPageDown,
        ReviewExtendUp,
        ReviewExtendDown,
        ReviewAccept
    ]
);
#[derive(Default)]
struct ReviewBindings;
impl gpui_kit::Global for ReviewBindings {}
pub(super) fn init(cx: &mut App) {
    if cx.try_global::<ReviewBindings>().is_some() {
        return;
    }
    cx.set_global(ReviewBindings);
    // Let GPUI's button key-up activation run instead of Dialog's default Confirm.
    cx.bind_keys([KeyBinding::new(
        "enter",
        gpui_kit::NoAction,
        Some("GitTurtleGithubButton"),
    )]);
    for context in ["GitTurtleGithubFiles", "GitTurtleGithubPatch"] {
        cx.bind_keys([
            KeyBinding::new("up", ReviewUp, Some(context)),
            KeyBinding::new("down", ReviewDown, Some(context)),
            KeyBinding::new("home", ReviewHome, Some(context)),
            KeyBinding::new("end", ReviewEnd, Some(context)),
            KeyBinding::new("pageup", ReviewPageUp, Some(context)),
            KeyBinding::new("pagedown", ReviewPageDown, Some(context)),
            KeyBinding::new("shift-up", ReviewExtendUp, Some(context)),
            KeyBinding::new("shift-down", ReviewExtendDown, Some(context)),
            KeyBinding::new("enter", ReviewAccept, Some(context)),
        ]);
    }
}
#[derive(Default, PartialEq)]
pub(super) struct FocusObservation {
    pub focus: Option<FocusHandle>,
    pub viewport: Bounds<Pixels>,
    pub rem_size: Pixels,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Section {
    Overview,
    Files,
    Review,
}
pub(super) struct ReviewState {
    pub section: Section,
    pub body_scroll: ScrollHandle,
    pub revealed_focus: std::rc::Rc<std::cell::RefCell<FocusObservation>>,
    pub files: Vec<PullFile>,
    pub file_page: u32,
    pub files_next: bool,
    pub selected_file: Option<usize>,
    pub selected_row: Option<usize>,
    pub anchor: Option<usize>,
    pub comments: Vec<LineComment>,
    pub composing: Option<LineComment>,
    pub input: Entity<TextareaState>,
    pub source: Option<Entity<EditorState>>,
    /// The Source editor's patch decorations, kept for the editor's life so Find can remove
    /// their backgrounds under its matches and a palette change redecorates them.
    source_decorations: Option<editor_find::PatchDecorations>,
    /// Compare's unified-patch view around the Source editor: old and new line-number
    /// columns, change tints and no editor folds, from the same presentation. `None` while the
    /// file prepares and above Compare's bounds, where Source keeps the editor's own numbers.
    source_view: Option<Entity<crate::diff_view::DiffView>>,
    /// The Source box's height inside its border in the last layout, which its fill to the
    /// file list's bottom sets and its whole-line inset leaves alone.
    source_height: std::rc::Rc<std::cell::Cell<Pixels>>,
    /// The Changes box's height inside its border in the last layout, set as Source's is.
    patch_height: std::rc::Rc<std::cell::Cell<Pixels>>,
    /// The selected file's patch presentation, prepared with its rows on the read worker
    /// within Compare's text bounds; `None` while preparing and above them.
    presentation: Option<text::PatchPresentation>,
    _palette: Subscription,
    pub mode: usize,
    pub file_focus: FocusHandle,
    pub line_focus: FocusHandle,
    pub section_focus: FocusHandle,
    pub preparing: bool,
    /// A preparation the panel's close cancelled, resumed when it reopens.
    suspended: bool,
    prepare_generation: u64,
    prepare_task: Option<Task<()>>,
    pub file_scroll: UniformListScrollHandle,
    pub line_scroll: UniformListScrollHandle,
    pub fixture: bool,
    pub fixture_moved: bool,
    pub show_list: bool,
    pub connection_open: bool,
    pub recovery_open: bool,
}
impl ReviewState {
    pub fn new(window: &mut Window, cx: &mut Context<Panel>) -> Self {
        Self {
            section: Section::Overview,
            body_scroll: ScrollHandle::new(),
            revealed_focus: Default::default(),
            files: vec![],
            file_page: 1,
            files_next: false,
            selected_file: None,
            selected_row: None,
            anchor: None,
            comments: vec![],
            composing: None,
            input: cx.new(|cx| {
                TextareaState::new(window, cx)
                    // Bounded prose layout lets Tab traverse controls instead of indenting.
                    .auto_grow(3, 3)
                    .placeholder("Explain this change…")
            }),
            source: None,
            source_decorations: None,
            source_view: None,
            source_height: Default::default(),
            patch_height: Default::default(),
            presentation: None,
            // Compare's patch is redecorated where the app applies a palette; this panel
            // follows the applied palette itself.
            _palette: cx.observe_global::<crate::appearance::Palette>(|panel, cx| {
                panel.review.refresh_theme(cx)
            }),
            mode: 0,
            file_focus: cx.focus_handle(),
            line_focus: cx.focus_handle(),
            section_focus: cx.focus_handle(),
            preparing: false,
            suspended: false,
            prepare_generation: 0,
            prepare_task: None,
            file_scroll: UniformListScrollHandle::new(),
            line_scroll: UniformListScrollHandle::new(),
            fixture: std::env::var("GITTURTLE_GITHUB_FIXTURE").is_ok_and(|value| value == "review"),
            fixture_moved: false,
            show_list: true,
            connection_open: false,
            recovery_open: false,
        }
    }
    pub fn cancel_preparation(&mut self) {
        self.prepare_generation = self.prepare_generation.wrapping_add(1);
        self.prepare_task = None;
        self.preparing = false;
        self.suspended = false;
    }
    /// Cancel a pending preparation as the panel closes, remembering it for the reopen.
    pub fn suspend_preparation(&mut self) {
        let suspended = self.preparing;
        self.cancel_preparation();
        self.suspended = suspended;
    }
    /// The selected file whose preparation closing cancelled, once; a failed preparation is
    /// not resumed.
    pub fn take_suspended(&mut self) -> Option<usize> {
        std::mem::take(&mut self.suspended)
            .then_some(self.selected_file)
            .flatten()
    }
    pub fn reset_content(&mut self) {
        self.cancel_preparation();
        self.files.clear();
        self.selected_file = None;
        self.selected_row = None;
        self.anchor = None;
        self.release_file();
        self.file_page = 1;
        self.files_next = false;
    }
    /// Release the selected file's Source editor, its view, decorations and presentation.
    fn release_file(&mut self) {
        self.source = None;
        self.source_view = None;
        self.source_decorations = None;
        self.presentation = None;
    }
    /// Give the Source editor Compare's presentation once it and its file's presentation both
    /// exist, whichever comes first: the decorations, and Compare's unified-patch view, which
    /// takes over the editor's line numbers and folds. A file above the bounds, or not
    /// prepared, keeps plain text with the editor's own line numbers.
    fn decorate_source(&mut self, window: &mut Window, cx: &mut App) {
        if self.source_decorations.is_none()
            && let (Some(source), Some(presentation)) = (&self.source, &self.presentation)
        {
            self.source_decorations = Some(text::decorate(source, presentation, cx));
            self.source_view = Some(crate::diff_view::new(
                source.clone(),
                presentation,
                window,
                cx,
            ));
        }
    }
    fn refresh_theme(&self, cx: &mut App) {
        if let (Some(decorations), Some(presentation)) =
            (&self.source_decorations, &self.presentation)
        {
            text::refresh_theme(decorations, presentation, cx);
        }
    }
}

/// Prepare Source's patch decorations only within Compare's text bounds. Compare applies
/// `gitturtle_core::MAX_DIFF_BYTES` and `MAX_DIFF_LINES` to each blob side in git-core, counting
/// `\n` bytes; this path applies the same constants, the same way, to the supplied patch text.
/// A larger patch is drawn as plain text.
fn source_presentation(patch: &str) -> Option<text::PatchPresentation> {
    (patch.len() <= gitturtle_core::MAX_DIFF_BYTES
        && patch.bytes().filter(|byte| *byte == b'\n').count() <= gitturtle_core::MAX_DIFF_LINES)
        .then(|| text::PatchPresentation::prepare(patch))
}
impl Panel {
    pub(super) fn reveal_on_focus(
        &self,
        focus: FocusHandle,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        let scroll = self.review.body_scroll.clone();
        let revealed = self.review.revealed_focus.clone();
        move |bounds, window, cx| {
            let observed = FocusObservation {
                focus: window.focused(cx),
                viewport: scroll.bounds(),
                rem_size: window.rem_size(),
            };
            let previous_focus = revealed.borrow();
            // Dialog entrance animation can move the whole viewport without
            // changing what fits inside it. It must not undo manual scrolling.
            if (previous_focus.focus == observed.focus
                && previous_focus.viewport.size == observed.viewport.size
                && previous_focus.rem_size == observed.rem_size)
                || !focus.contains_focused(window, cx)
            {
                return;
            }
            let Some(bounds) = bounds.into_iter().reduce(|a, b| a.union(&b)) else {
                return;
            };
            let viewport = observed.viewport;
            let inset = window.rem_size() + px(2.);
            let delta = if bounds.top() < viewport.top() + inset {
                viewport.top() + inset - bounds.top()
            } else if bounds.bottom() > viewport.bottom() - inset {
                viewport.bottom() - inset - bounds.bottom()
            } else {
                return;
            };
            let previous = scroll.offset();
            let offset = point(
                previous.x,
                (previous.y + delta).clamp(-scroll.max_offset().y, px(0.)),
            );
            if offset == previous {
                return;
            }
            let scroll = scroll.clone();
            window.defer(cx, move |window, cx| {
                if window.focused(cx) == observed.focus
                    && scroll.offset() == previous
                    && scroll.bounds() == viewport
                {
                    scroll.set_offset(offset);
                    window.refresh();
                }
            });
        }
    }
    pub(super) fn focus_visible_section(&self, window: &mut Window, cx: &mut Context<Self>) {
        *self.review.revealed_focus.borrow_mut() = Default::default();
        if self.create {
            self.title.read(cx).focus_handle(cx).focus(window, cx);
            return;
        }
        if self.pull.is_none() {
            self.destination.read(cx).focus_handle(cx).focus(window, cx);
            return;
        }
        if self.conversations.active.is_some() {
            self.conversations
                .input
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx);
            return;
        }
        match self.review.section {
            Section::Review => {
                if self.review.composing.is_some() {
                    self.review
                        .input
                        .read(cx)
                        .focus_handle(cx)
                        .focus(window, cx);
                } else {
                    self.body.read(cx).focus_handle(cx).focus(window, cx);
                }
            }
            Section::Files => self.review.file_focus.focus(window, cx),
            Section::Overview => self.review.section_focus.focus(window, cx),
        }
    }
    fn focus_file_content(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.review.selected_file.is_none() || self.review.preparing {
            self.review.file_focus.focus(window, cx);
            return;
        }
        match self.review.mode {
            1 => {
                if let Some(source) = &self.review.source {
                    source.read(cx).focus_handle(cx).focus(window, cx);
                } else {
                    self.review.file_focus.focus(window, cx);
                }
            }
            2 => self.review.section_focus.focus(window, cx),
            _ => {
                if self
                    .review
                    .selected_file
                    .is_some_and(|n| !self.review.files[n].rows.is_empty())
                {
                    self.review.line_focus.focus(window, cx);
                } else {
                    self.review.file_focus.focus(window, cx);
                }
            }
        }
    }
    /// Count retained model/text payloads, including conservative editor copies.
    /// Toolkit allocation overhead and process/GPU memory are separate budgets.
    pub(super) fn retained_review_bytes(&self, cx: &App) -> usize {
        let comment =
            |c: &LineComment| c.body.len() + c.path.len() + std::mem::size_of::<LineComment>();
        let review = |r: &ReviewDraft| {
            r.body.len()
                + r.comments.iter().map(comment).sum::<usize>()
                + r.composing.as_ref().map_or(0, comment)
        };
        let pull = |p: &PullRequest| {
            p.title.len()
                + p.body.as_ref().map_or(0, String::len)
                + p.head.branch.len()
                + p.base.branch.len()
                + p.head.sha.len()
                + p.base.sha.len()
        };
        self.review
            .files
            .iter()
            .map(|file| {
                file.filename.len()
                    + file.previous_filename.as_ref().map_or(0, String::len)
                    + file.patch.as_ref().map_or(0, |p| p.len() * 3)
                    + file.rows.len() * std::mem::size_of::<model::PatchRow>()
                    + file.rows.iter().map(|row| row.text.len()).sum::<usize>()
            })
            .sum::<usize>()
            + self
                .review
                .presentation
                .as_ref()
                .map_or(0, text::PatchPresentation::retained_bytes)
            + self
                .review
                .source_decorations
                .as_ref()
                .map_or(0, editor_find::PatchDecorations::retained_bytes)
            + self.conversation_bytes(cx)
            + self.review.comments.iter().map(comment).sum::<usize>()
            + self.review.composing.as_ref().map_or(0, comment)
            + (self.body.read(cx).value().len()
                + self.review.input.read(cx).value().len()
                + self.title.read(cx).value().len())
                * 3
            + (self.destination.read(cx).value().len()
                + self.head.read(cx).value().len()
                + self.base.read(cx).value().len())
                * 3
            + self
                .templates
                .iter()
                .map(|t| t.path.len() + t.revision.len() + t.body.len())
                .sum::<usize>()
            + self
                .attempts
                .iter()
                .map(|attempt| {
                    attempt.destination.len()
                        + attempt.outcome.len()
                        + attempt
                            .completed_reply_sha256
                            .as_ref()
                            .map_or(0, |receipts| {
                                receipts.iter().map(String::len).sum::<usize>()
                            })
                })
                .sum::<usize>()
            + self.account.as_ref().map_or(0, String::len)
            + self.notice.as_ref().map_or(0, String::len)
            + self.error.as_ref().map_or(0, String::len)
            + self.save_status.len()
            + self.repo.path().as_os_str().as_encoded_bytes().len()
            + self.confirm.as_ref().map_or(0, |confirmation| {
                confirmation.account.len()
                    + match &confirmation.action {
                        Action::Create(p) => {
                            p.body.len() + p.title.len() + p.head.len() + p.base.len()
                        }
                        Action::Comment { body, .. } => body.len(),
                        Action::Reply { target, body } => target.retained_bytes() + body.len(),
                        Action::SetThreadResolved { target, .. } => target.retained_bytes(),
                        Action::Review(r) => review(r),
                    }
            })
            + self.rows.iter().map(pull).sum::<usize>()
            + self.pull.as_ref().map_or(0, |p| pull(p) * 3)
            + self.status.as_ref().map_or(0, |s| {
                s.reviews
                    .iter()
                    .chain(&s.checks)
                    .map(String::len)
                    .sum::<usize>()
            })
            + self
                .saved
                .iter()
                .map(|draft| match draft {
                    Draft::Pull(p) => p.body.len() + p.title.len(),
                    Draft::Review(p) => review(p),
                    Draft::Reply(p) => p.target.retained_bytes() + p.body.len(),
                })
                .sum::<usize>()
    }
    fn review_key(&mut self, key: &str, extend: bool, window: &mut Window, cx: &mut Context<Self>) {
        let is_file = self.review.file_focus.is_focused(window);
        if !is_file && !self.review.line_focus.is_focused(window) {
            return;
        }
        if key == "enter" {
            if is_file {
                if self.review.selected_file.is_none() {
                    self.select_review_file(0, window, cx);
                } else {
                    self.focus_file_content(window, cx);
                }
            } else {
                self.begin_comment(window, cx);
            }
            return;
        }
        let (current, count) = if is_file {
            (
                self.review.selected_file.unwrap_or(0),
                self.review.files.len(),
            )
        } else {
            (
                self.review.selected_row.unwrap_or(0),
                self.review
                    .selected_file
                    .map_or(0, |n| self.review.files[n].rows.len()),
            )
        };
        let index = match key {
            "up" => current.saturating_sub(1),
            "down" => (current + 1).min(count.saturating_sub(1)),
            "home" => 0,
            "end" => count.saturating_sub(1),
            "pageup" => current.saturating_sub(8),
            "pagedown" => (current + 8).min(count.saturating_sub(1)),
            _ => return,
        };
        if is_file {
            self.select_review_file(index, window, cx);
        } else {
            self.select_review_line(index, extend, window, cx);
        }
    }

    pub(super) fn captured(&self, cx: &App) -> Option<CapturedPull> {
        self.pull
            .as_ref()
            .and_then(|pull| self.destination(cx).ok().map(|repo| pull.capture(repo)))
    }
    pub(super) fn restore_review(
        &mut self,
        draft: Option<ReviewDraft>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.review.comments = draft
            .as_ref()
            .map(|d| d.comments.clone())
            .unwrap_or_default();
        self.review.composing = draft.as_ref().and_then(|d| d.composing.clone());
        let text = self
            .review
            .composing
            .as_ref()
            .map(|c| c.body.clone())
            .unwrap_or_default();
        self.review
            .input
            .update(cx, |input, cx| input.set_value(text, window, cx));
        self.discussion = draft.as_ref().is_some_and(|draft| draft.discussion)
            && self.review.comments.is_empty()
            && self.review.composing.is_none();
    }
    fn change_section(&mut self, section: Section, window: &mut Window, cx: &mut Context<Self>) {
        self.save_draft(window, cx);
        self.review.section = section;
        self.confirm = None;
        self.focus_visible_section(window, cx);
        cx.notify();
    }
    fn file_page(&mut self, page: u32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pull) = self.captured(cx) else {
            return;
        };
        let fixture = self.review.fixture;
        let moved = self.review.fixture_moved;
        self.run(
            false,
            move |control| Client(UiTransport::open(fixture, moved)?).files(&pull, page, &control),
            |result, this, _, _| match result {
                Ok(page) => this.receive_file_page(page),
                Err(error) => this.error = Some(format!("{error:#}")),
            },
            window,
            cx,
        );
    }
    fn receive_file_page(&mut self, page: github::Page<PullFile>) {
        self.review.cancel_preparation();
        self.review.files = page.items;
        self.review.file_page = page.page;
        self.review.files_next = page.has_next;
        self.review.selected_file = None;
        self.review.selected_row = None;
        self.review.anchor = None;
        self.review.release_file();
        self.review.mode = 0;
    }
    pub(super) fn select_review_file(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.review.files.len() {
            return;
        }
        if self.review.selected_file == Some(index)
            && (self.review.preparing
                || !self.review.files[index].rows.is_empty()
                || self.review.files[index].unavailable.is_some())
        {
            self.review.file_focus.focus(window, cx);
            return;
        }
        self.review.cancel_preparation();
        for file in &mut self.review.files {
            file.rows.clear();
            file.rows.shrink_to_fit();
            file.unavailable = None;
        }
        self.review.selected_file = Some(index);
        self.review.selected_row = None;
        self.review.anchor = None;
        self.review.release_file();
        self.review.mode = 0;
        self.review
            .file_scroll
            .scroll_to_item(index, ScrollStrategy::Center);
        self.review
            .line_scroll
            .scroll_to_item(0, ScrollStrategy::Top);
        self.review.file_focus.focus(window, cx);
        self.prepare_review_file(index, window, cx);
    }
    /// Prepare a file's inline rows and Source presentation off the UI thread, leaving its
    /// mode, scroll and Source editor as they are; Source takes the decorations on arrival.
    /// The clone shares the patch's allocation with the view rather than copying it.
    pub(super) fn prepare_review_file(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut file = self.review.files[index].clone();
        let generation = self.review.prepare_generation;
        // Pure CPU over the supplied patch the panel owns: no repository, network or write, so
        // it stays off the serial operations executor that carries accepted writes. Dropping
        // the task cancels work that has not started. A panic on a supplied patch becomes the
        // error the operations executor gives a panicking job (`operations.rs`), as Compare's
        // worker guards its preparation, instead of unwinding into the awaiting UI task.
        let prepared = cx.background_spawn(async move {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                file.prepare()?;
                let presentation = file.patch.as_deref().and_then(source_presentation);
                anyhow::Ok((file, presentation))
            }))
            .unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Operation interrupted unexpectedly. Refresh the repository before retrying; it may have changed."
                ))
            })
        });
        self.review.preparing = true;
        self.review.prepare_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = prepared.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.closed
                    || generation != this.review.prepare_generation
                    || this.review.selected_file != Some(index)
                {
                    return;
                }
                this.review.preparing = false;
                match result {
                    Ok((file, presentation)) => {
                        this.review.files[index] = file;
                        this.review.presentation = presentation;
                        this.review.decorate_source(window, cx);
                    }
                    Err(error) => this.error = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn select_review_line(
        &mut self,
        index: usize,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(file) = self
            .review
            .selected_file
            .and_then(|n| self.review.files.get(n))
        else {
            return;
        };
        if file.rows.get(index).is_none() {
            return;
        }
        if !extend || self.review.anchor.is_none() {
            self.review.anchor = Some(index);
        }
        self.review.selected_row = Some(index);
        self.review
            .line_scroll
            .scroll_to_item(index, ScrollStrategy::Center);
        self.review.line_focus.focus(window, cx);
        cx.notify();
    }
    fn begin_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        *self.review.revealed_focus.borrow_mut() = Default::default();
        if self.review.composing.is_some() {
            self.review
                .input
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx);
            return;
        }
        let selection = self
            .review
            .selected_file
            .and_then(|file| Some((file, self.review.anchor?, self.review.selected_row?)))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Select an added or deleted line. {} or {} / {} extends a same-side range.",
                    shortcuts::SHIFT_CLICK,
                    shortcuts::key_label("shift-up"),
                    shortcuts::key_label("shift-down")
                )
            })
            .and_then(|(file, start, end)| self.review.files[file].selection(start, end));
        match selection {
            Ok(comment) => {
                self.confirm = None;
                self.review.composing = Some(comment);
                self.review
                    .input
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.review
                    .input
                    .read(cx)
                    .focus_handle(cx)
                    .focus(window, cx);
                self.save_draft(window, cx);
            }
            Err(error) => self.error = Some(error.to_string()),
        }
        cx.notify();
    }
    fn add_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut comment) = self.review.composing.clone() else {
            return;
        };
        comment.body = self.review.input.read(cx).value().to_string();
        if let Err(error) = github::validate_text(&comment.body, true) {
            self.error = Some(error.to_string());
            cx.notify();
            return;
        }
        if self.review.comments.len() >= 100 {
            self.error=Some("A review supports at most 100 inline comments. Finish this review before adding more.".into());
            cx.notify();
            return;
        }
        self.review.comments.push(comment);
        self.review.composing = None;
        self.confirm = None;
        self.discussion = false;
        self.review
            .input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.save_draft(window, cx);
        self.focus_visible_section(window, cx);
        cx.notify();
    }
    fn edit_comment(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        *self.review.revealed_focus.borrow_mut() = Default::default();
        if self.review.composing.is_some() {
            self.error =
                Some("Add or discard the unfinished inline comment before editing another.".into());
            cx.notify();
            return;
        }
        if index >= self.review.comments.len() {
            return;
        }
        self.confirm = None;
        let comment = self.review.comments.remove(index);
        let body = comment.body.clone();
        self.review.composing = Some(comment);
        self.review
            .input
            .update(cx, |input, cx| input.set_value(body, window, cx));
        self.save_draft(window, cx);
        self.review
            .input
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
        cx.notify();
    }
    fn discard_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.review.composing = None;
        self.review
            .input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.save_draft(window, cx);
        self.focus_visible_section(window, cx);
        cx.notify();
    }
    fn review_source(&mut self, mode: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.review.mode = mode;
        if mode == 0
            && let Some(index) = self.review.selected_file
            && !self.review.preparing
            && self.review.files[index].rows.is_empty()
            && self.review.files[index].unavailable.is_none()
        {
            self.select_review_file(index, window, cx);
        }
        if mode == 1
            && self.review.source.is_none()
            && let Some(patch) = self
                .review
                .selected_file
                .and_then(|n| self.review.files[n].patch.as_deref())
        {
            // Plain text with the editor's own line numbers; Compare's decorations and gutter
            // follow from the presentation the file's preparation made, now or when it arrives.
            let source = text::editor(patch, "diff", None, window, cx);
            self.review.source = Some(source);
            self.review.decorate_source(window, cx);
        }
        self.focus_file_content(window, cx);
        cx.notify();
    }
    pub(super) fn render_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let count = self.review.comments.len();
        div()
            .flex()
            .items_center()
            .gap_1()
            .border_b_1()
            .border_color(rgb(palette(cx).border))
            .pb_2()
            .children(
                [
                    (Section::Overview, "Overview".to_owned()),
                    (
                        Section::Files,
                        format!(
                            "Files · {}{}",
                            self.review.files.len(),
                            if self.review.files_next { "+" } else { "" }
                        ),
                    ),
                    (Section::Review, format!("Review · {count}")),
                ]
                .into_iter()
                .enumerate()
                .map(|(i, (section, name))| {
                    button(
                        ("github-section", i),
                        name,
                        "",
                        self.review.section == section,
                    )
                    .toggled(self.review.section == section)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.change_section(section, window, cx)
                    }))
                }),
            )
            .child(div().flex_1())
            .child(
                label("github-local-save", self.save_status.clone())
                    .text_color(rgb(palette(cx).muted)),
            )
            .into_any_element()
    }
    fn render_comment_composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let Some(comment) = self.review.composing.as_ref() else {
            return div().into_any_element();
        };
        div()
            .debug_selector(|| "github-inline-composer".into())
            .on_children_prepainted(
                self.reveal_on_focus(self.review.input.read(cx).focus_handle(cx)),
            )
            .p_3()
            .rounded(px(7.))
            .bg(rgb(p.subtle))
            .border_1()
            .border_color(rgb(p.border))
            .flex()
            .flex_col()
            .gap_2()
            .child(
                label(
                    "github-composer-position",
                    format!("Draft inline comment · {}", model::position_label(comment)),
                )
                .font_weight(FontWeight::SEMIBOLD),
            )
            .child(
                Textarea::new(&self.review.input)
                    .h(appearance::ui_size(100.))
                    .aria_label("Inline review comment, saved locally with its captured position")
                    .disabled(self.pending),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        button("github-add-comment", "Add to review", "plus", true)
                            .disabled(self.pending)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.add_comment(window, cx)),
                            ),
                    )
                    .child(
                        button("github-discard-composer", "Discard this comment", "", false)
                            .disabled(self.pending)
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.discard_composer(window, cx)
                                }),
                            ),
                    )
                    .child(
                        label(
                            "github-composer-local",
                            "Stays local until you submit the review",
                        )
                        .text_color(rgb(p.muted)),
                    ),
            )
            .into_any_element()
    }
    pub(super) fn render_files(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let selected = self
            .review
            .selected_file
            .and_then(|n| self.review.files.get(n));
        let file_list = div()
            .w(appearance::ui_size(210.))
            .min_w(px(150.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .id("github-file-focus")
                    .debug_selector(|| "github-file-focus".into())
                    .key_context("GitTurtleGithubFiles")
                    .on_action(cx.listener(|this, _: &ReviewUp, window, cx| {
                        this.review_key("up", false, window, cx);
                        cx.stop_propagation();
                    }))
                    .on_action(cx.listener(|this, _: &ReviewDown, window, cx| {
                        this.review_key("down", false, window, cx);
                        cx.stop_propagation();
                    }))
                    .on_action(cx.listener(|this, _: &ReviewHome, window, cx| {
                        this.review_key("home", false, window, cx);
                        cx.stop_propagation();
                    }))
                    .on_action(cx.listener(|this, _: &ReviewEnd, window, cx| {
                        this.review_key("end", false, window, cx);
                        cx.stop_propagation();
                    }))
                    .on_action(cx.listener(|this, _: &ReviewPageUp, window, cx| {
                        this.review_key("pageup", false, window, cx);
                        cx.stop_propagation();
                    }))
                    .on_action(cx.listener(|this, _: &ReviewPageDown, window, cx| {
                        this.review_key("pagedown", false, window, cx);
                        cx.stop_propagation();
                    }))
                    .on_action(cx.listener(|this, _: &ReviewExtendUp, window, cx| {
                        this.review_key("up", true, window, cx);
                        cx.stop_propagation();
                    }))
                    .on_action(cx.listener(|this, _: &ReviewExtendDown, window, cx| {
                        this.review_key("down", true, window, cx);
                        cx.stop_propagation();
                    }))
                    .on_action(cx.listener(|this, _: &ReviewAccept, window, cx| {
                        this.review_key("enter", false, window, cx);
                        cx.stop_propagation();
                    }))
                    .h(FILE_LIST_HEIGHT)
                    .border_1()
                    .border_color(rgb(p.border))
                    .rounded(px(6.))
                    .overflow_hidden()
                    .tab_stop(true)
                    .track_focus(&self.review.file_focus)
                    .focus_visible(|style| style.border_color(rgb(p.accent)))
                    .role(Role::ListBox)
                    .aria_label("Pull request changed files")
                    .aria_description(format!(
                        "Up and Down select a file. {} moves to its patch.",
                        shortcuts::ENTER
                    ))
                    .child(
                        uniform_list(
                            "github-file-list",
                            self.review.files.len(),
                            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                range
                                    .map(|index| {
                                        let file = &this.review.files[index];
                                        let selected = this.review.selected_file == Some(index);
                                        let path = file.filename.clone();
                                        let hover = palette(cx).row_hover(selected);
                                        div()
                                            .id(("github-file", index))
                                            .w_full()
                                            .h(appearance::ui_size(54.))
                                            .px_3()
                                            .flex()
                                            .flex_col()
                                            .justify_center()
                                            .gap_1()
                                            .border_l_2()
                                            .border_color(rgb(if selected {
                                                palette(cx).accent
                                            } else {
                                                palette(cx).panel
                                            }))
                                            .bg(rgb(if selected {
                                                palette(cx).selected
                                            } else {
                                                palette(cx).panel
                                            }))
                                            .hover(move |style| style.bg(rgb(hover)))
                                            .role(Role::ListBoxOption)
                                            .aria_label(format!(
                                                "{} · {} · {} additions, {} deletions",
                                                file.filename,
                                                file.status,
                                                file.additions,
                                                file.deletions
                                            ))
                                            .aria_selected(selected)
                                            .cursor_pointer()
                                            .child(
                                                div()
                                                    .text_size(appearance::ui_text(12.))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .truncate()
                                                    .child(path.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(appearance::ui_text(10.))
                                                    .text_color(rgb(palette(cx).muted))
                                                    .child(format!(
                                                        "{} · +{} −{}",
                                                        file.status, file.additions, file.deletions
                                                    )),
                                            )
                                            .tooltip(move |window, cx| {
                                                Tooltip::new(path.clone()).build(window, cx)
                                            })
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.select_review_file(index, window, cx)
                                            }))
                                            .into_any_element()
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .size_full()
                        .track_scroll(&self.review.file_scroll),
                    ),
            )
            .child(
                div()
                    .min_h(appearance::ui_size(PAGER_ROW))
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        button("github-files-previous", "Previous", "", false)
                            .debug_selector(|| "github-files-previous".into())
                            .disabled(self.pending || self.review.file_page <= 1)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.file_page(this.review.file_page - 1, window, cx)
                            })),
                    )
                    .child(label(
                        "github-files-page",
                        format!("{}", self.review.file_page),
                    ))
                    .child(
                        button("github-files-next", "Next", "", false)
                            .disabled(self.pending || !self.review.files_next)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.file_page(this.review.file_page + 1, window, cx)
                            })),
                    ),
            )
            .child(
                label(
                    "github-file-bound",
                    "100 files per page · GitHub limit 3,000",
                )
                .text_color(rgb(p.muted)),
            );
        let content = if let Some(file) = selected {
            let title = file
                .previous_filename
                .as_ref()
                .map(|old| format!("{} → {}", old, file.filename))
                .unwrap_or_else(|| file.filename.clone());
            // Changes' box and Source's end where the file list beside them ends, so switching
            // modes keeps the box's edge; Changes' range hint lies below it, beside the list's
            // pager, where it took the box's last 26 px only in Changes. Its first line centres
            // on the pager's row, where the page buttons centre their labels, wrapped or not.
            div().flex_1().min_w_0().flex().flex_col().gap_2()
                .child(div().flex().flex_col().gap_2().when(self.review.mode<2,|content|content.h(FILE_LIST_HEIGHT))
                    .child(label("github-file-title",title).flex_shrink_0().font_weight(FontWeight::SEMIBOLD))
                    .child(div().flex_shrink_0().flex().flex_wrap().items_center().gap_1().children(["Changes","Source","Threads"].into_iter().enumerate().map(|(mode,name)|button(("github-file-mode",mode),name,"",self.review.mode==mode).toggled(self.review.mode==mode).on_click(cx.listener(move|this,_,window,cx|this.review_source(mode,window,cx)))))
                        .child(div().flex_1()).child(button("github-inline-start",if self.review.composing.is_some(){"Continue comment"}else{"Comment on selection"},"plus",false).disabled(self.pending||self.review.preparing||file.unavailable.is_some()).on_click(cx.listener(|this,_,window,cx|this.begin_comment(window,cx)))))
                    .child(match self.review.mode {1=>self.render_source(cx),2=>self.render_threads(Some(&file.filename),cx),_=>self.render_patch(window,cx)}))
                .when(self.review.mode==0,|element|element.child(div().flex_shrink_0().pt(appearance::ui_size((PAGER_ROW-HINT_LINE)/2.)).child(label("github-range-hint",format!("Click a changed line · {} or {} / {} selects a same-side range · {} comments",shortcuts::SHIFT_CLICK,shortcuts::key_label("shift-up"),shortcuts::key_label("shift-down"),shortcuts::ENTER)).debug_selector(||"github-range-hint".into()).line_height(appearance::ui_size(HINT_LINE)).text_color(rgb(p.muted)))))
                .into_any_element()
        } else {
            div().flex_1().min_w_0().p_5().flex().flex_col().gap_2().child(label("github-choose-file","Choose a file to begin reviewing").font_weight(FontWeight::SEMIBOLD)).child(label("github-file-help","Changes keep their captured PR identity. Inline drafts remain local as you move between files.").text_color(rgb(p.muted))).into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .gap_3()
                    .min_w_0()
                    .child(file_list)
                    .child(content),
            )
            .child(self.render_comment_composer(cx))
            .into_any_element()
    }
    /// The exact patch, filling its column down to the file list's bottom edge in whole lines:
    /// what a line does not fill, less than a line, lies below the text, in the editor's own
    /// background.
    fn render_source(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(source) = &self.review.source else {
            return label("github-no-source","No text patch was supplied. Open the captured local comparison to inspect available source or binary content.").into_any_element();
        };
        let p = palette(cx);
        let laid_out = self.review.source_height.clone();
        let line = source
            .read(cx)
            .line_height()
            .unwrap_or(appearance::code_text() * EDITOR_LINE_HEIGHT);
        // The lines fill the box below the editor's top padding and its Find bar; its bottom
        // padding lies past the clip at the last whole line.
        let text =
            (laid_out.get() - EDITOR_PADDING - editor_find::panel_height(source, cx)).max(px(0.));
        let slack = text - line * (text / line).floor();
        let content = match &self.review.source_view {
            Some(view) => view.clone().into_any_element(),
            None => editor_find::Editor::new(source)
                .readonly(true)
                .bordered(false)
                .h_full()
                .aria_label("Exact GitHub supplied patch source")
                .into_any_element(),
        };
        div()
            .debug_selector(|| "github-source-patch".into())
            .flex_1()
            .min_h_0()
            .min_w_0()
            .border_1()
            .border_color(rgb(p.border))
            .rounded(px(6.))
            .overflow_hidden()
            // What the editor's text area paints, so the strip below the text reads as part of it.
            .bg(rgb(p.canvas))
            .on_children_prepainted(move |children, window, cx| {
                // The inset follows the height inside the border, never the other way round,
                // so this settles in the next frame.
                if let Some(inner) = children.first()
                    && laid_out.get() != inner.size.height
                {
                    laid_out.set(inner.size.height);
                    window.defer(cx, |window, _| window.refresh());
                }
            })
            .child(
                div().size_full().pb(slack).child(
                    // Ends at the last whole line's bottom and cuts what the editor paints
                    // below it, its bottom padding and the top of the next line, which it
                    // overruns this by.
                    div()
                        .debug_selector(|| "github-source-text".into())
                        .relative()
                        .size_full()
                        .overflow_hidden()
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .bottom(-EDITOR_PADDING)
                                .child(content),
                        ),
                ),
            )
            .into_any_element()
    }
    fn render_patch(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let Some(file) = self
            .review
            .selected_file
            .and_then(|n| self.review.files.get(n))
        else {
            return div().into_any_element();
        };
        // In the patch's place, which ends with the file list.
        if self.review.preparing {
            return div()
                .flex_1()
                .min_h_0()
                .child(
                    label("github-preparing-file", "Preparing the selected file…")
                        .role(Role::Status),
                )
                .into_any_element();
        }
        if let Some(reason) = &file.unavailable {
            return div()
                .flex_1()
                .min_h_0()
                .child(
                    div()
                        .p_4()
                        .bg(rgb(p.subtle))
                        .rounded(px(6.))
                        .child(label("github-patch-unavailable", reason.clone())),
                )
                .into_any_element();
        }
        // Whole rows down to the file list's bottom edge, as Source shows whole lines, below
        // the top inset Source's editor keeps above its text: what a row does not fill lies
        // below them, measured from the box's height in the last layout, which the insets
        // cannot change.
        let laid_out = self.review.patch_height.clone();
        let measured = laid_out.clone();
        let row_height = patch_row_height(window.scale_factor());
        let room = (laid_out.get() - EDITOR_PADDING).max(px(0.));
        let slack = room - row_height * (room / row_height).floor();
        // Source's gutter columns for the file: Compare's width for its widest line number.
        let widest = file
            .rows
            .iter()
            .flat_map(|row| [row.old, row.new])
            .flatten()
            .max()
            .unwrap_or(0);
        let column = px(
            crate::diff_view::line_number_column_width(u64::from(widest))
                * appearance::code_scale(),
        );
        div().id("github-patch-focus").key_context("GitTurtleGithubPatch").on_action(cx.listener(|this,_:&ReviewUp,window,cx|{this.review_key("up",false,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewDown,window,cx|{this.review_key("down",false,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewHome,window,cx|{this.review_key("home",false,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewEnd,window,cx|{this.review_key("end",false,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewPageUp,window,cx|{this.review_key("pageup",false,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewPageDown,window,cx|{this.review_key("pagedown",false,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewExtendUp,window,cx|{this.review_key("up",true,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewExtendDown,window,cx|{this.review_key("down",true,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewAccept,window,cx|{this.review_key("enter",false,window,cx);cx.stop_propagation();})).debug_selector(||"github-patch".into()).flex_1().min_h_0().min_w_0().border_1().border_color(rgb(p.border)).rounded(px(6.)).overflow_hidden()
            // A context row's surface, so the strip below the rows reads as part of the patch.
            .bg(rgb(p.canvas))
            .tab_stop(true).track_focus(&self.review.line_focus).focus_visible(|style|style.border_color(rgb(p.accent))).role(Role::ListBox).aria_label("Captured pull request patch lines").aria_description("Up and Down move through lines. Shift extends a same-side range. Return starts an inline comment. Source offers exact text selection and Find.")

            .child(div().size_full().on_children_prepainted(move |children, window, cx| {
                // The inset follows the height inside the border, never the other way round,
                // so this settles in the next frame.
                if let Some(inner) = children.first()
                    && measured.get() != inner.size.height
                {
                    measured.set(inner.size.height);
                    window.defer(cx, |window, _| window.refresh());
                }
            }).child(div().size_full().pt(EDITOR_PADDING).pb(slack).child(div().debug_selector(||"github-patch-rows".into()).size_full().overflow_hidden().child(uniform_list("github-patch-rows",file.rows.len(),cx.processor(move|this,range:std::ops::Range<usize>,_,cx|range.map(|index|{let file=&this.review.files[this.review.selected_file.expect("visible file")];let row=&file.rows[index];let selected=this.review.anchor.zip(this.review.selected_row).is_some_and(|(a,b)|(a.min(b)..=a.max(b)).contains(&index));let p=palette(cx);let background=if selected{p.selected}else{match row.kind{b'+'=>p.added_background,b'-'=>p.removed_background,b'@'=>p.subtle,_=>p.canvas}};let row_label=format!("Before {} · After {} · {}",row.old.map(|n|n.to_string()).unwrap_or_default(),row.new.map(|n|n.to_string()).unwrap_or_default(),row.text);
                // The selection's edge overlays the row, so its text keeps Source's place.
                patch_row(index,row,column,row_height,cx).bg(rgb(background)).when(selected,|row|row.child(div().absolute().left_0().top_0().bottom_0().w(px(2.)).bg(rgb(p.accent)))).role(Role::ListBoxOption).aria_label(row_label).aria_selected(selected).cursor_pointer()
                    .on_click(cx.listener(move|this,event:&ClickEvent,window,cx|this.select_review_line(index,event.modifiers().shift,window,cx))).into_any_element()
            }).collect::<Vec<_>>())).size_full().track_scroll(&self.review.line_scroll))))).into_any_element()
    }
    pub(super) fn render_collected(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        div().flex().flex_col().gap_2()
            .child(label("github-review-heading",format!("{} inline comments in this review",self.review.comments.len())).ml(super::text_inset()).font_weight(FontWeight::SEMIBOLD))
            .when(self.review.comments.is_empty(),|element|element.child(label("github-review-empty","Select changed lines in Files to add inline feedback, or send a review summary below.").ml(super::text_inset()).text_color(rgb(p.muted))))
            .child(div().id("github-collected-comments").debug_selector(||"github-collected-comments".into()).flex_shrink_0().max_h(px(230.)).overflow_y_scroll().flex().flex_col().gap_2().children(self.review.comments.iter().enumerate().map(|(index,comment)|div().flex_shrink_0().p_3().border_1().border_color(rgb(p.border)).rounded(px(6.)).flex().flex_col().gap_2()
                .child(label(("github-draft-position",index),model::position_label(comment)).font_weight(FontWeight::SEMIBOLD))
                .child(label(("github-draft-body",index),comment.body.clone()))
                .child(div().flex().gap_2().child(button(("github-edit-inline",index),"Edit","",false).disabled(self.pending||self.review.composing.is_some()).on_click(cx.listener(move|this,_,window,cx|this.edit_comment(index,window,cx))))
                    .child(button(("github-remove-inline",index),"Remove","",false).disabled(self.pending).on_click(cx.listener(move|this,_,window,cx|{if index<this.review.comments.len(){this.review.comments.remove(index);this.confirm=None;this.save_draft(window,cx);this.focus_visible_section(window,cx);}})))))))
            .child(self.render_comment_composer(cx)).into_any_element()
    }
}
fn patch_line(index: usize, cx: &App) -> Stateful<Div> {
    div()
        .id(("github-patch-line", index))
        .text_size(appearance::code_text())
        .code_font(cx)
}

/// The changed-file list's height; Source fills its column down to the same edge.
const FILE_LIST_HEIGHT: Pixels = px(324.);
/// The file list's pager row: a compact Button's height at the default interface size.
const PAGER_ROW: f32 = 28.;
/// The range hint's line height at the default interface size. Half of what the pager's row
/// has beyond it lies above the hint, so its first line centres on the row, where the page
/// buttons centre their labels; at the top of its row it stood 4 px higher.
const HINT_LINE: f32 = 20.;
/// The kit editor's line height, a multiple of its text size.
const EDITOR_LINE_HEIGHT: f32 = 1.5;
/// The kit editor's padding above and below its text. In Source only the top padding lies
/// between the inside of the box's border and the text; the bottom one runs past the clip.
/// Changes keeps the same inset above its rows, so both modes start their text at one height.
const EDITOR_PADDING: Pixels = px(8.);
/// Where Source's editor starts a line's text after Compare's gutter, read from the kit: the
/// padding beside the text of the Input the editor draws in, which keeps the default size,
/// and the margin the kit's code editor keeps before its text where its own line numbers
/// would end; Compare's gutter takes their place. Each Changes row starts its text this far
/// after its gutter. Counting only the padding, Changes' text began 10 px before Source's.
fn source_text_inset() -> Pixels {
    gpui_kit::component::Size::default().input_px()
        + gpui_kit::base::input::LINE_NUMBER_RIGHT_MARGIN
}
/// A Changes row's height: the code text's line at 1.65, on whole device pixels at `scale`.
/// Layout rounds an authored height to device pixels, so a row of 19.8 px at 12 pt is drawn
/// 20 px tall at 2x: counted at 19.8 px, the box's rows ran 2.6 px past its clip and cut the
/// last one. Snapped here, the height counted is the height drawn.
fn patch_row_height(scale: f32) -> Pixels {
    px((f32::from(appearance::code_text()) * 1.65 * scale).round() / scale)
}

/// One patch row's layout, `height` tall, in Source's geometry: the old and new line numbers
/// right-aligned in `column`-wide columns, [`crate::diff_view::CELL_PADDING`] inside each
/// column's right edge, where Compare's gutter paints them, then the line's text
/// [`source_text_inset`] after the gutter, where Source's editor starts it. The gutter is one
/// box as wide as both columns, as Source's is, so both round its width alike.
fn patch_row(
    index: usize,
    row: &model::PatchRow,
    column: Pixels,
    height: Pixels,
    cx: &App,
) -> Stateful<Div> {
    let p = palette(cx);
    let padding = px(crate::diff_view::CELL_PADDING * appearance::code_scale());
    let number = |side: &'static str, number: Option<u32>| {
        div()
            .pr(padding)
            .flex()
            .justify_end()
            .text_color(rgb(p.muted))
            .child(
                div()
                    .debug_selector(move || format!("github-patch-{side}-{index}"))
                    .flex_shrink_0()
                    .child(number.map(|n| n.to_string()).unwrap_or_default()),
            )
    };
    patch_line(index, cx)
        .debug_selector(move || format!("github-patch-line-{index}"))
        .relative()
        .w_full()
        .h(height)
        .flex()
        .items_center()
        .child(
            div()
                .w(column * 2.)
                .flex_shrink_0()
                .flex()
                .child(number("old", row.old).w(column).flex_shrink_0())
                .child(number("new", row.new).flex_1()),
        )
        .child(
            div()
                .debug_selector(move || format!("github-patch-text-{index}"))
                .ml(source_text_inset())
                .min_w_0()
                .flex_1()
                .truncate()
                .text_color(rgb(if row.kind == b'@' { p.hunk } else { p.text }))
                .child(row.text.clone()),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::component::Root;
    use std::{cell::RefCell, rc::Rc};

    #[gpui::test]
    fn patch_lines_use_the_code_font(cx: &mut TestAppContext) {
        let font = shaped_code_font(cx, |probe, cx| {
            patch_line(0, cx).child(probe).into_any_element()
        });
        assert_eq!(font.family, "Desktop Mono");
        assert_eq!(font.features.is_calt_enabled(), Some(false));
        assert!(font.features.tag_value_list().contains(&("liga".into(), 0)));
    }

    /// At every code size Settings offers, the line-number columns take Compare's width for
    /// the file's widest number, scaled with the code size, and fit it in the code font: each
    /// number ends [`crate::diff_view::CELL_PADDING`] inside its column's right edge, as
    /// Compare's gutter paints it, without crossing the column's left edge, and the patch text
    /// starts [`source_text_inset`] after both columns.
    #[gpui::test]
    fn line_numbers_fit_the_widest_number_at_every_code_size(cx: &mut TestAppContext) {
        struct Rows(model::PatchRow);
        impl Render for Rows {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let widest = self.0.old.max(self.0.new).unwrap_or(0);
                let column = px(
                    crate::diff_view::line_number_column_width(u64::from(widest))
                        * appearance::code_scale(),
                );
                div()
                    .w(px(900.))
                    .child(patch_row(0, &self.0, column, px(20.), cx))
            }
        }
        cx.update(|cx| {
            gpui_kit::init(cx);
            gpui_kit::component::Theme::global_mut(cx).mono_font_family = "Desktop Mono".into();
        });
        let row = model::PatchRow {
            old: Some(99_999),
            new: Some(12_345),
            kind: b' ',
            text: "context".into(),
            hunk: 0,
        };
        let (_, cx) = cx.add_window_view(move |_, _| Rows(row));
        let measure = |code: u8, cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|window, cx| {
                appearance::apply_text_sizes(13, code, window, cx);
                window.draw(cx).clear(cx);
            });
            let bounds = |selector, cx: &mut gpui_kit::VisualTestContext| {
                cx.debug_bounds(selector).expect(selector)
            };
            let line = bounds("github-patch-line-0", cx);
            let old = bounds("github-patch-old-0", cx);
            let new = bounds("github-patch-new-0", cx);
            let text = bounds("github-patch-text-0", cx);
            let scale = appearance::code_scale();
            let column = crate::diff_view::line_number_column_width(99_999) * scale;
            let padding = crate::diff_view::CELL_PADDING * scale;
            // The number as the column's text element shapes it.
            let number = cx.update(|window, cx| {
                let family = gpui_kit::component::Theme::global(cx)
                    .mono_font_family
                    .clone();
                let font = gpui_kit::Font {
                    features: appearance::code_font_features_for(&family),
                    ..gpui_kit::font(family)
                };
                let run = TextRun {
                    len: "99999".len(),
                    font,
                    color: gpui_kit::black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                window
                    .text_system()
                    .shape_line("99999".into(), appearance::code_text(), &[run], None)
                    .width
            });
            let at = |x: f32| line.left() + px(x);
            // Layout puts each edge on a device pixel, half a pixel at 2x.
            let near = |a: Pixels, b: Pixels| (a - b).abs() < px(0.51);
            assert!(
                old.size.width >= number,
                "{code} pt: {old:?} clips {number:?}"
            );
            assert!(
                old.left() >= line.left(),
                "{code} pt: {old:?} crosses {line:?}"
            );
            assert!(
                near(old.right(), at(column - padding)),
                "{code} pt: {old:?}"
            );
            assert!(
                new.left() >= at(column),
                "{code} pt: {new:?} crosses the old column"
            );
            assert!(
                near(new.right(), at(column * 2. - padding)),
                "{code} pt: {new:?}"
            );
            assert!(
                near(text.left(), at(column * 2.) + source_text_inset()),
                "{code} pt: {text:?} after {new:?}"
            );
        };
        for code in appearance::CODE_TEXT_RANGE {
            measure(code, cx);
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    #[gpui::test]
    async fn native_review_keys_warm_context_and_destination_flush(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["-c", "init.templateDir=", "init", "--initial-branch=main"])
                .arg(fixture.path())
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap()
                .status
                .success()
        );
        let repo = GitRepository::open(fixture.path()).unwrap();
        let panel_repo = repo.clone();
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let output = captured.clone();
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
        });
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    None,
                    Preferences::default(),
                    repository_tabs::Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                app.page = AppPage::Repository;
                app.path = Some(repo.path().to_owned());
                app.repository = Some(repo);
                app
            });
            *output.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        cx.update(|_, cx| {
            gpui_kit::component::Theme::global_mut(cx).font_size = px(18.);
        });
        cx.simulate_resize(size(px(1000.), px(680.)));
        let app = captured.borrow().as_ref().unwrap().clone();
        let panel = cx.update(|window, cx| {
            cx.new(|cx| Panel::new(app.downgrade(), panel_repo, "main".into(), window, cx))
        });
        app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.executor().run_until_parked();
        let pull = model::fixture_pull(false);
        let captured_pull =
            pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured_pull, 1, &OperationControl::default())
            .unwrap()
            .items;
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.destination.update(cx, |input, cx| {
                    input.set_value("gitturtle-fixture/native-review", window, cx)
                });
                panel.pull = Some(pull);
                panel.review.files = files;
                panel.review.section = Section::Files;
                panel.review.show_list = false;
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        cx.executor().run_until_parked();
        cx.update(|window, cx| {
            assert!(window.has_active_dialog(cx));
            assert!(panel.read(cx).review.file_focus.is_focused(window));
        });
        cx.simulate_keystrokes("enter");
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.select_review_file(1, window, cx);
                panel.select_review_file(0, window, cx);
            })
        });
        // File preparation runs on the GPUI background executor.
        cx.executor().run_until_parked();
        cx.update(|window, cx| {
            assert!(window.has_active_dialog(cx));
            assert_eq!(panel.read(cx).review.selected_file, Some(0));
            assert!(!panel.read(cx).review.preparing);
            assert!(!panel.read(cx).review.files[0].rows.is_empty());
            assert!(panel.read(cx).review.files[1].rows.is_empty());
            panel.update(cx, |panel, cx| {
                panel.select_review_line(3, false, window, cx)
            });
        });
        cx.simulate_keystrokes("shift-down enter");
        cx.update(|window, cx| {
            assert!(window.has_active_dialog(cx));
            let draft = panel.read(cx).review.composing.as_ref().unwrap();
            assert_eq!((draft.start_line, draft.line), (Some(2), 3));
            assert!(
                panel
                    .read(cx)
                    .review
                    .input
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            );
            panel.update(cx, |panel, cx| {
                panel.review.input.update(cx, |input, cx| {
                    input.set_value(" Exact unfinished text before navigation\\n", window, cx);
                    cx.emit(InputEvent::Change);
                });
            });
        });
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }
        let composer = cx
            .debug_bounds("github-inline-composer")
            .expect("rendered inline composer");
        let viewport = cx.read(|cx| panel.read(cx).review.body_scroll.bounds());
        assert!(viewport.size.height <= px(380.));
        assert!(
            composer.size.height > px(100.) && composer.size.width > px(300.),
            "composer must retain useful native geometry"
        );
        assert!(
            composer.top() >= viewport.top() && composer.bottom() <= viewport.bottom(),
            "focused composer {composer:?} must be inside {viewport:?}"
        );
        cx.update(|window, cx| {
            panel
                .read(cx)
                .review
                .body_scroll
                .set_offset(point(px(0.), px(0.)));
            window.refresh();
        });
        for _ in 0..2 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }
        cx.read(|cx| {
            assert_eq!(
                panel.read(cx).review.body_scroll.offset().y,
                px(0.),
                "unchanged focus must not override manual scrolling"
            )
        });
        cx.update(|_, cx| assert!(panel.read(cx).deferred_draft.is_pending()));
        // This is a real InputEvent transition inside the typing quiet period.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.destination.update(cx, |input, cx| {
                    input.set_value("gitturtle-fixture/another-destination", window, cx);
                    cx.emit(InputEvent::Change);
                })
            })
        });
        app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.executor().run_until_parked();
        let saved = app
            .read_with(cx, |app, _| app.preferences_writer.submit(drafts::load))
            .await
            .unwrap()
            .unwrap();
        let stored = saved
            .into_iter()
            .find_map(|draft| match draft {
                Draft::Review(review) if review.pull == captured_pull => Some(review),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            stored.composing.as_ref().unwrap().body,
            " Exact unfinished text before navigation\\n"
        );
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.pull = Some(model::fixture_pull(false));
                panel.destination.update(cx, |input, cx| {
                    input.set_value("gitturtle-fixture/native-review", window, cx)
                });
                panel.restore_review(Some(stored), window, cx);
                panel.review.section = Section::Review;
                panel.close(window, cx);
            })
        });
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
        cx.executor().run_until_parked();
        cx.update(|window, cx| {
            assert!(window.has_active_dialog(cx));
            assert!(panel.read(cx).review.section == Section::Review);
            assert_eq!(
                panel.read(cx).review.input.read(cx).value(),
                " Exact unfinished text before navigation\\n"
            );
            assert!(
                panel
                    .read(cx)
                    .review
                    .input
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            );
            panel.update(cx, |panel, cx| panel.close(window, cx));
        });
        let mut page = Client(UiTransport::Fixture { moved: false })
            .files(&captured_pull, 1, &OperationControl::default())
            .unwrap();
        let replacement = page.items.remove(1);
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.closed = false;
                panel.review.section = Section::Files;
                panel.receive_file_page(page);
                panel.select_review_file(0, window, cx);
                panel.receive_file_page(github::Page {
                    items: vec![replacement],
                    page: 2,
                    has_next: false,
                });
            })
        });
        // File preparation runs on the GPUI background executor.
        cx.executor().run_until_parked();
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                assert_eq!(panel.review.file_page, 2);
                assert_eq!(panel.review.files[0].filename, "docs/review/界面-guide.md");
                assert!(panel.review.files[0].rows.is_empty());
                assert!(!panel.review.preparing);
                assert!(panel.review.selected_file.is_none());
                panel.close(window, cx);
            })
        });
        app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.executor().run_until_parked();
    }
    /// Source draws the supplied patch as plain text and takes the decorations the file's
    /// background preparation made, read back through the handle Find masks: they reach the
    /// editor whether Source opens before the preparation finishes or after it, equal Compare's
    /// for the same text, follow a palette change and count in the retained bytes. A file
    /// superseded before its preparation finished decorates nothing, neither its own Source
    /// nor the next file's. `source_presentation_stops_at_compares_bounds` covers the bounds.
    #[gpui::test]
    async fn source_patch_takes_the_decorations_its_file_preparation_made(cx: &mut TestAppContext) {
        use crate::appearance::ThemeChoice;
        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["-c", "init.templateDir=", "init", "--initial-branch=main"])
                .arg(fixture.path())
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap()
                .status
                .success()
        );
        let repo = GitRepository::open(fixture.path()).unwrap();
        let panel_repo = repo.clone();
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let output = captured.clone();
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
            ThemeChoice::ALL[0].apply(None, cx);
        });
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    None,
                    Preferences::default(),
                    repository_tabs::Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                app.page = AppPage::Repository;
                app.path = Some(repo.path().to_owned());
                app.repository = Some(repo);
                app
            });
            *output.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = captured.borrow().as_ref().unwrap().clone();
        let panel = cx.update(|window, cx| {
            cx.new(|cx| Panel::new(app.downgrade(), panel_repo, "main".into(), window, cx))
        });
        // The panel's local read (remotes and saved drafts) runs on the operations executor;
        // settle it once so it cannot land during the steps below.
        app.read_with(cx, |app, _| app.operations.drain());
        cx.run_until_parked();
        // File preparation runs on the GPUI background executor.
        let settle = |cx: &mut gpui_kit::VisualTestContext| cx.run_until_parked();
        let patches = [
            "@@ -1,2 +1,2 @@\n context\n-old value\n+new value\n".to_owned(),
            "@@ -1 +1,2 @@\n-before\n+after\n+more\n".to_owned(),
        ];
        // Counts match each patch, so a prepared file has inline rows.
        let file = |name: &str, patch: &String, additions| PullFile {
            filename: name.into(),
            previous_filename: None,
            status: "modified".into(),
            additions,
            deletions: 1,
            patch: Some(patch.as_str().into()),
            rows: Vec::new(),
            unavailable: None,
        };
        let files = vec![
            file("src/a.rs", &patches[0], 1),
            file("src/b.rs", &patches[1], 2),
        ];
        // The decorations Compare's unified diff takes for the same text in the applied palette.
        let compare = |patch: &str, cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|window, cx| {
                let presentation = text::PatchPresentation::prepare(patch);
                // The editor stays alive while `drawn` reads the ranges it holds.
                let (_editor, decorations) =
                    text::editor_with_decorations(patch, "diff", Some(&presentation), window, cx);
                decorations
                    .expect("a prepared patch is decorated")
                    .drawn(cx)
            })
        };
        let source = |cx: &mut gpui_kit::VisualTestContext| {
            cx.read(|cx| panel.read(cx).review.source.clone().expect("Source editor"))
        };
        let has_view = |cx: &mut gpui_kit::VisualTestContext| {
            cx.read(|cx| panel.read(cx).review.source_view.is_some())
        };
        let decorations = |editor: &Entity<EditorState>, cx: &mut gpui_kit::VisualTestContext| {
            cx.read(|cx| editor_find::patch_layer(editor, cx).map(|layer| layer.drawn(cx)))
        };

        // Source opens before the file's preparation has finished: plain text at once.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.review.files = files;
                panel.review.section = Section::Files;
                panel.select_review_file(0, window, cx);
                panel.review_source(1, window, cx);
            })
        });
        let first = source(cx);
        assert_eq!(decorations(&first, cx), None);
        assert!(
            !has_view(cx),
            "Compare's gutter waits for the presentation prepared off the UI thread"
        );
        settle(cx);
        assert!(
            has_view(cx),
            "the prepared presentation gives Source Compare's gutter"
        );
        let expected = compare(&patches[0], cx);
        assert_eq!(
            decorations(&first, cx).as_ref(),
            Some(&expected),
            "the prepared decorations reach the open Source editor"
        );
        cx.read(|cx| assert_eq!(first.read(cx).value(), patches[0].as_str()));

        // The presentation and the decorations kept for the editor count toward the budget.
        cx.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                let with = panel.retained_review_bytes(cx);
                let presentation = panel.review.presentation.take().expect("prepared");
                let kept = panel.review.source_decorations.take().expect("kept");
                let counted = presentation.retained_bytes() + kept.retained_bytes();
                assert_eq!(with - panel.retained_review_bytes(cx), counted);
                assert!(counted > 0);
                panel.review.presentation = Some(presentation);
                panel.review.source_decorations = Some(kept);
            })
        });

        // A palette change redecorates it as Compare would be.
        cx.update(|_, cx| ThemeChoice::ALL[1].apply(None, cx));
        cx.run_until_parked();
        let recolored = compare(&patches[0], cx);
        assert_ne!(recolored, expected);
        assert_eq!(decorations(&first, cx), Some(recolored.clone()));

        // A file superseded before its preparation finished decorates nothing: its Source is
        // plain text while it is selected, and its preparation's result never lands, neither
        // as its rows nor on the next file's Source, opened before either finished.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.select_review_file(1, window, cx);
                panel.review_source(1, window, cx);
            })
        });
        let superseded = source(cx);
        assert_eq!(decorations(&superseded, cx), None);
        assert!(!has_view(cx));
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.select_review_file(0, window, cx);
                panel.review_source(1, window, cx);
            })
        });
        let current = source(cx);
        settle(cx);
        assert_ne!(current, superseded);
        cx.read(|cx| assert!(panel.read(cx).review.files[1].rows.is_empty()));
        assert_eq!(decorations(&current, cx), Some(recolored));
        assert!(has_view(cx));

        // Source opened after its file's preparation finished takes the decorations at once.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| panel.select_review_file(1, window, cx))
        });
        settle(cx);
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.review_source(1, window, cx)));
        let later = source(cx);
        cx.read(|cx| assert!(!panel.read(cx).review.files[1].rows.is_empty()));
        let expected = compare(&patches[1], cx);
        assert_eq!(decorations(&later, cx), Some(expected));
        assert!(has_view(cx));
    }
    /// Source decorations are prepared up to Compare's bounds and not one byte or one line
    /// beyond. Lines are `\n` bytes, as git-core counts them for Compare, so a patch's last
    /// line without one does not count.
    #[test]
    fn source_presentation_stops_at_compares_bounds() {
        use gitturtle_core::{MAX_DIFF_BYTES, MAX_DIFF_LINES};
        // Bytes: one hunk whose context line fills the patch, with no trailing newline.
        let header = "@@ -1 +1 @@\n ";
        let at_bytes = format!("{header}{}", "x".repeat(MAX_DIFF_BYTES - header.len()));
        assert_eq!(at_bytes.len(), MAX_DIFF_BYTES);
        assert!(source_presentation(&at_bytes).is_some());
        assert!(source_presentation(&format!("{at_bytes}x")).is_none());
        // Lines: MAX_DIFF_LINES newlines, the last context line unterminated.
        let at_lines = format!(
            "@@ -1,{0} +1,{0} @@\n{1} ",
            MAX_DIFF_LINES,
            " \n".repeat(MAX_DIFF_LINES - 1)
        );
        let newlines = |patch: &str| patch.bytes().filter(|byte| *byte == b'\n').count();
        assert_eq!(newlines(&at_lines), MAX_DIFF_LINES);
        assert!(at_lines.len() < MAX_DIFF_BYTES);
        assert!(source_presentation(&at_lines).is_some());
        let over_lines = format!("{at_lines}\n ");
        assert_eq!(newlines(&over_lines), MAX_DIFF_LINES + 1);
        assert!(source_presentation(&over_lines).is_none());
    }

    /// The PR review's Source draws the fixture's `src/review/session.rs` patch as Compare draws
    /// its unified patch: the same view around the editor, so the same old and new line-number
    /// columns, gutter tint and decorations, and no line numbers or folds of the editor's own.
    /// Its preparation stays off the UI thread and is cancelled on a file change and on close.
    #[gpui::test]
    async fn source_patch_draws_compares_gutter(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["-c", "init.templateDir=", "init", "--initial-branch=main"])
                .arg(fixture.path())
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap()
                .status
                .success()
        );
        let repo = GitRepository::open(fixture.path()).unwrap();
        let panel_repo = repo.clone();
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let output = captured.clone();
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
        });
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    None,
                    Preferences::default(),
                    repository_tabs::Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                app.page = AppPage::Repository;
                app.path = Some(repo.path().to_owned());
                app.repository = Some(repo);
                app
            });
            *output.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        cx.simulate_resize(size(px(1000.), px(680.)));
        let app = captured.borrow().as_ref().unwrap().clone();
        let panel = cx.update(|window, cx| {
            cx.new(|cx| Panel::new(app.downgrade(), panel_repo, "main".into(), window, cx))
        });
        app.read_with(cx, |app, _| app.operations.drain());
        cx.run_until_parked();
        let pull = model::fixture_pull(false);
        let captured_pull =
            pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured_pull, 1, &OperationControl::default())
            .unwrap()
            .items;
        let index = files
            .iter()
            .position(|file| file.filename == "src/review/session.rs")
            .expect("fixture file");
        let other = (index + 1) % files.len();
        let patch = files[index].patch.clone().expect("fixture patch");
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.pull = Some(pull);
                panel.review.files = files;
                panel.review.section = Section::Files;
                panel.review.show_list = false;
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.select_review_file(index, window, cx);
                panel.review_source(1, window, cx);
            })
        });
        cx.run_until_parked();
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }
        // Compare's unified patch for the same text, built as Compare builds it, drawn where
        // Source draws its text: inside the frame's one-pixel border, above the inset that
        // keeps its lines whole, as tall as the editor, which runs past the clip at its last
        // whole line by its bottom padding.
        let clip = cx
            .debug_bounds("github-source-text")
            .expect("rendered Source text");
        let inner = Bounds::new(
            clip.origin,
            size(clip.size.width, clip.size.height + EDITOR_PADDING),
        );
        let presentation = text::PatchPresentation::prepare(&patch);
        let (compare, compare_decorations, compare_view) = cx.update(|window, cx| {
            let (editor, decorations) =
                text::editor_with_decorations(&patch, "diff", Some(&presentation), window, cx);
            let view = crate::diff_view::new(editor.clone(), &presentation, window, cx);
            (
                editor,
                decorations.expect("a prepared patch is decorated"),
                view,
            )
        });
        for _ in 0..3 {
            let view = compare_view.clone();
            cx.draw(inner.origin, inner.size, move |_, _| {
                div().w(inner.size.width).h(inner.size.height).child(view)
            });
            cx.run_until_parked();
        }
        let source = cx.read(|cx| panel.read(cx).review.source.clone().expect("Source editor"));
        // Gutter width (both number columns), where the editor's text starts within it (no
        // line numbers or fold markers of its own), its size and its line height.
        let geometry = |editor: &Entity<EditorState>, cx: &mut gpui_kit::VisualTestContext| {
            cx.read(|cx| {
                let state = editor.read(cx);
                let input = state.input_bounds();
                let text = state.text_bounds().expect("laid out");
                (
                    input.origin.x - inner.origin.x,
                    text.origin.x - input.origin.x,
                    input.size,
                    state.line_height().expect("laid out"),
                )
            })
        };
        let drawn = geometry(&source, cx);
        assert_eq!(
            drawn,
            geometry(&compare, cx),
            "Source draws Compare's gutter"
        );
        let columns = px(2. * presentation.column_width * crate::appearance::code_scale());
        // The editor's own inset follows the gutter, so the text starts past both columns.
        assert!(drawn.0 >= columns, "old and new number columns: {drawn:?}");
        cx.read(|cx| {
            assert_eq!(
                editor_find::patch_layer(&source, cx).map(|layer| layer.drawn(cx)),
                Some(compare_decorations.drawn(cx)),
                "Source tints its lines as Compare does"
            );
            let review = &panel.read(cx).review;
            let prepared = review.presentation.as_ref().expect("prepared");
            assert_eq!(
                prepared.rows, presentation.rows,
                "the gutter's numbers and tints"
            );
            assert_eq!(prepared.change_rows, presentation.change_rows);
            assert_eq!(prepared.column_width, presentation.column_width);
        });

        // A file change while Source waits for its presentation cancels it, and so does close:
        // nothing lands, so neither file's Source gains a gutter afterwards.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.select_review_file(other, window, cx);
                panel.review_source(1, window, cx);
                assert!(panel.review.preparing);
                assert!(panel.review.source_view.is_none());
                panel.select_review_file(index, window, cx);
                panel.review_source(1, window, cx);
                assert!(panel.review.source_view.is_none());
                panel.close(window, cx);
                assert!(!panel.review.preparing);
            })
        });
        cx.run_until_parked();
        cx.read(|cx| {
            let review = &panel.read(cx).review;
            assert!(review.presentation.is_none());
            assert!(review.source_view.is_none());
            assert!(review.files.iter().all(|file| file.rows.is_empty()));
        });
    }
    #[gpui::test]
    async fn warm_reopen_retries_interrupted_local_read_and_template_payload_evicts(
        cx: &mut TestAppContext,
    ) {
        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["-c", "init.templateDir=", "init", "--initial-branch=main"])
                .arg(fixture.path())
                .output()
                .unwrap()
                .status
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(fixture.path())
                .args([
                    "remote",
                    "add",
                    "origin",
                    "https://github.com/gitturtle-fixture/held-read.git"
                ])
                .output()
                .unwrap()
                .status
                .success()
        );
        let repo = GitRepository::open(fixture.path()).unwrap();
        let panel_repo = repo.clone();
        let observed: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let output = observed.clone();
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
        });
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    None,
                    Preferences::default(),
                    repository_tabs::Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                app.page = AppPage::Repository;
                app.repository = Some(repo);
                app
            });
            *output.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = observed.borrow().as_ref().unwrap().clone();
        let stored = Draft::Pull(NewPull {
            repository: Repository::parse("gitturtle-fixture/held-read").unwrap(),
            title: "Recover this title".into(),
            body: "Exact stored recovery".into(),
            head: "feature".into(),
            base: "main".into(),
            draft: true,
            expected_head: None,
            expected_base: None,
        });
        let key = stored.key();
        app.read_with(cx, |app, _| {
            app.preferences_writer.submit(move || drafts::save(stored))
        })
        .await
        .unwrap()
        .unwrap();
        let (release, gate) = std::sync::mpsc::channel();
        let (started, running) = std::sync::mpsc::channel();
        let held = app.read_with(cx, |app, _| {
            app.operations.submit(move || {
                started.send(())?;
                gate.recv()?;
                Ok(())
            })
        });
        running.recv_timeout(Duration::from_secs(5)).unwrap();
        let panel = cx.update(|window, cx| {
            cx.new(|cx| {
                Panel::new(
                    app.downgrade(),
                    panel_repo.clone(),
                    "main".into(),
                    window,
                    cx,
                )
            })
        });
        cx.executor().run_until_parked();
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                assert!(panel.local_loading);
                assert!(!panel.local_loaded);
                panel.close(window, cx);
            })
        });
        release.send(()).unwrap();
        held.await.unwrap().unwrap();
        app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.executor().run_until_parked();
        cx.read(|cx| assert!(!panel.read(cx).local_loaded));
        let (release, gate) = std::sync::mpsc::channel();
        let (started, running) = std::sync::mpsc::channel();
        let held = app.read_with(cx, |app, _| {
            app.operations.submit(move || {
                started.send(())?;
                gate.recv()?;
                Ok(())
            })
        });
        running.recv_timeout(Duration::from_secs(5)).unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
        cx.executor().run_until_parked();
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                assert!(panel.local_loading);
                panel.destination.update(cx, |input, cx| {
                    input.set_value("gitturtle-fixture/user-edited", window, cx)
                });
                panel.title.update(cx, |input, cx| {
                    input.set_value("Unsaved edited title", window, cx)
                });
            })
        });
        release.send(()).unwrap();
        held.await.unwrap().unwrap();
        app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.executor().run_until_parked();
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                assert!(panel.local_loaded);
                assert!(panel.saved.iter().any(|draft| draft.key() == key));
                assert_eq!(
                    panel.destination.read(cx).value(),
                    "gitturtle-fixture/user-edited"
                );
                assert_eq!(panel.title.read(cx).value(), "Unsaved edited title");
                panel.close(window, cx);
            })
        });
        // Each actual panel owns the maximum supported template payload. The
        // count limit alone would retain four; the payload limit must evict.
        cx.update(|window, cx| {
            let mut warm = WarmPanels::default();
            for index in 0..4 {
                let form = cx.new(|cx| {
                    let mut panel = Panel::new(
                        app.downgrade(),
                        panel_repo.clone(),
                        "main".into(),
                        window,
                        cx,
                    );
                    panel.closed = true;
                    panel.templates = (0..32)
                        .map(|n| github::Template {
                            path: format!(".github/PULL_REQUEST_TEMPLATE/{n}.md"),
                            revision: "1".repeat(40),
                            body: "x".repeat(github::MAX_TEXT),
                        })
                        .collect();
                    panel.attempts.push(drafts::Attempt {
                        completed_reply_sha256: None,
                        destination: "fixture attempt".into(),
                        outcome: "Observed result".repeat(200),
                    });
                    panel
                });
                let bytes = form.read(cx).retained_review_bytes(cx);
                assert!(bytes > 8 * 1024 * 1024);
                warm.retain(WarmPanel {
                    owner: app.entity_id(),
                    worktree: std::path::PathBuf::from(format!("logical-fixture-{index}")),
                    panel: form,
                    bytes,
                });
            }
            assert_eq!(warm.0.len(), 3);
            assert_eq!(
                warm.0[0].worktree,
                std::path::PathBuf::from("logical-fixture-1")
            );
            assert!(warm.0.iter().map(|entry| entry.bytes).sum::<usize>() <= 32 * 1024 * 1024);
        });
        app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.executor().run_until_parked();
    }
    /// In a 1480 × 980 window at 13 and 18 pt, Source fills its column down to the changed-file
    /// list's bottom edge, as plain text while its file prepares and decorated once it has, and
    /// shows whole lines: its text is a whole number of lines tall, and what a line does not
    /// fill, less than a line, lies below the text inside the box. It counted its lines over the
    /// box less the editor's padding above and below, though the bottom padding lies past its
    /// clip, so 39 px at 2x lay below 13 lines of 36 px, room for a 14th.
    #[gpui::test]
    async fn source_reaches_the_file_lists_bottom_in_whole_lines(cx: &mut TestAppContext) {
        let (_fixture, app, panel, cx) = review_panel(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.apply_appearance(window, cx)));
        cx.simulate_resize(size(px(1480.), px(980.)));
        let pull = model::fixture_pull(false);
        let captured_pull =
            pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured_pull, 1, &OperationControl::default())
            .unwrap()
            .items;
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.pull = Some(pull);
                panel.review.files = files;
                panel.review.section = Section::Files;
                panel.review.show_list = false;
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        cx.run_until_parked();
        let canvas = midnight_canvas(cx);
        // Frames without running the file's preparation.
        let frames = |cx: &mut gpui_kit::VisualTestContext| {
            for _ in 0..3 {
                cx.update(|window, cx| window.draw(cx).clear(cx));
            }
        };
        for (interface, code) in [(13, 12), (18, 15)] {
            cx.update(|window, cx| appearance::apply_text_sizes(interface, code, window, cx));
            for file in [0, 1] {
                cx.update(|window, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.select_review_file(file, window, cx);
                        panel.review_source(1, window, cx);
                    })
                });
                for decorated in [false, true] {
                    if decorated {
                        cx.run_until_parked();
                    }
                    frames(cx);
                    let at = format!("{interface} pt, file {file}, decorated {decorated}");
                    assert_eq!(
                        cx.read(|cx| panel.read(cx).review.source_view.is_some()),
                        decorated,
                        "{at}"
                    );
                    let source = cx.debug_bounds("github-source-patch").expect("Source");
                    let list = cx.debug_bounds("github-file-focus").expect("file list");
                    assert!(
                        (source.bottom() - list.bottom()).abs() < px(0.01),
                        "{at}: Source {source:?} ends with the file list {list:?}"
                    );
                    let editor = cx.read(|cx| panel.read(cx).review.source.clone().unwrap());
                    let (text, line) = cx.read(|cx| {
                        let editor = editor.read(cx);
                        (
                            editor.input_bounds(),
                            editor.line_height().expect("laid out"),
                        )
                    });
                    let lines = text.size.height / line;
                    assert!(
                        lines >= 5. && (lines - lines.round()).abs() < 0.01,
                        "{at}: {text:?} is {lines} lines of {line:?}"
                    );
                    // Inside the 1 px border: the editor's bottom padding lies past the clip.
                    let below = source.bottom() - px(1.) - text.bottom();
                    assert!(
                        below > px(-0.01) && below < line,
                        "{at}: {below:?} below the text {text:?} in {source:?}"
                    );
                    // Nothing the box paints reaches below its last whole line, where the
                    // top of a 14th line once showed, and the strip below takes the canvas
                    // the text area paints.
                    let clip = cx.debug_bounds("github-source-text").expect("Source text");
                    assert!(
                        (clip.bottom() - text.bottom()).abs() < px(0.01),
                        "{at}: {clip:?} ends at the last whole line {text:?}"
                    );
                    let (fills, lowest) = painted_in(cx, source);
                    assert_eq!(fills, vec![canvas], "{at}: the box's fill");
                    assert!(
                        lowest <= clip.bottom() + px(0.01),
                        "{at}: paints down to {lowest:?}, below {clip:?}"
                    );
                }
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.run_until_parked();
    }
    /// Applies Midnight, whose kit input background (#181f2c over Midnight's surfaces) is
    /// lighter than the canvas its editors' text areas paint (#10151f), and returns that canvas.
    fn midnight_canvas(cx: &mut gpui_kit::VisualTestContext) -> gpui_kit::Background {
        cx.update(|window, cx| appearance::ThemeChoice::Midnight.apply(Some(window), cx));
        let (canvas, input) = cx.update(|_, cx| {
            (
                gpui_kit::Hsla::from(rgb(palette(cx).canvas)),
                gpui_kit::component::Theme::global(cx).input_background(),
            )
        });
        assert_ne!(
            input, canvas,
            "the strip's old fill differs from the canvas"
        );
        canvas.into()
    }

    /// What the last frame painted in `frame`: the fills of the quads at its bounds, its own,
    /// and the lowest bottom, as clipped, of every other quad drawn inside it.
    fn painted_in(
        cx: &mut gpui_kit::VisualTestContext,
        frame: Bounds<Pixels>,
    ) -> (Vec<gpui_kit::Background>, Pixels) {
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let logical = |bounds: Bounds<gpui_kit::ScaledPixels>| {
                let value = |value: gpui_kit::ScaledPixels| px(value.as_f32() / scale);
                Bounds::from_corners(
                    point(value(bounds.left()), value(bounds.top())),
                    point(value(bounds.right()), value(bounds.bottom())),
                )
            };
            let near = |a: Pixels, b: Pixels| (a - b).abs() < px(0.5);
            let (mut fills, mut lowest) = (Vec::new(), frame.top());
            for quad in window.painted_quads() {
                let drawn = logical(quad.bounds);
                if near(drawn.left(), frame.left())
                    && near(drawn.top(), frame.top())
                    && near(drawn.right(), frame.right())
                    && near(drawn.bottom(), frame.bottom())
                {
                    if !quad.background.is_transparent() {
                        fills.push(quad.background);
                    }
                    continue;
                }
                let shown = drawn.intersect(&logical(quad.content_mask.bounds));
                if shown.size.height > px(0.)
                    && shown.size.width > px(0.)
                    && shown.left() >= frame.left() - px(0.5)
                    && shown.right() <= frame.right() + px(0.5)
                    && shown.top() >= frame.top() - px(0.5)
                    && shown.bottom() <= frame.bottom() + px(0.5)
                    && !quad.background.is_transparent()
                {
                    lowest = lowest.max(shown.bottom());
                }
            }
            (fills, lowest)
        })
    }

    /// In a 1480 × 980 window at 13 and 18 pt, Changes' box ends at the changed-file list's
    /// bottom edge, as Source's does, with its range hint below it, on the line of the file
    /// list's page buttons. Its rows fill the box in whole rows, counted at the height they are
    /// drawn: what a row does not fill, less than a row, lies below them inside the box, no row
    /// runs past the rows' clip, and the box paints nothing below its last whole row and fills
    /// that strip with the canvas a context row paints.
    #[gpui::test]
    async fn changes_reach_the_file_lists_bottom_in_whole_rows(cx: &mut TestAppContext) {
        let (_fixture, app, panel, cx) = review_panel(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.apply_appearance(window, cx)));
        cx.simulate_resize(size(px(1480.), px(980.)));
        let pull = model::fixture_pull(false);
        let captured_pull =
            pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured_pull, 1, &OperationControl::default())
            .unwrap()
            .items;
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.pull = Some(pull);
                panel.review.files = files;
                panel.review.section = Section::Files;
                panel.review.show_list = false;
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        cx.run_until_parked();
        let canvas = midnight_canvas(cx);
        for (interface, code) in [(13, 12), (18, 15)] {
            cx.update(|window, cx| appearance::apply_text_sizes(interface, code, window, cx));
            for file in [0, 1] {
                cx.update(|window, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.select_review_file(file, window, cx);
                        panel.review_source(0, window, cx);
                    })
                });
                cx.run_until_parked();
                for _ in 0..3 {
                    cx.update(|window, cx| window.draw(cx).clear(cx));
                }
                let at = format!("{interface} pt, file {file}");
                let list = cx.debug_bounds("github-file-focus").expect("file list");
                let patch = cx.debug_bounds("github-patch").expect("Changes");
                assert!(
                    (patch.bottom() - list.bottom()).abs() < px(0.01),
                    "{at}: Changes {patch:?} ends with the file list {list:?}"
                );
                let hint = cx.debug_bounds("github-range-hint").expect("range hint");
                assert!(
                    hint.top() >= patch.bottom(),
                    "{at}: the hint {hint:?} lies below the box {patch:?}"
                );
                // On the pager's line: the hint's first line, one of two at 18 pt, centres where
                // Previous centres its label, within the device pixel layout snaps it to. At the
                // top of its row it stood 4 px above.
                let previous = cx.debug_bounds("github-files-previous").expect("Previous");
                let line = appearance::ui_size(HINT_LINE);
                let lines = (hint.size.height / line).round();
                let first = hint.top() + hint.size.height / lines / 2.;
                assert!(
                    lines >= 1. && (first - previous.center().y).abs() < px(0.5),
                    "{at}: the hint {hint:?} centres its first line off Previous {previous:?}"
                );
                let rows = cx.debug_bounds("github-patch-rows").expect("rows");
                // The rows as drawn: at 12 pt a row of 19.8 px is drawn 20 px tall at 2x, and
                // counted at 19.8 px the 13th row of the first file was cut 2.6 px short.
                let row = cx
                    .debug_bounds("github-patch-line-0")
                    .expect("row")
                    .size
                    .height;
                let whole = rows.size.height / row;
                assert!(
                    whole >= 5. && (rows.size.height - row * whole.round()).abs() < px(0.01),
                    "{at}: {rows:?} is {whole} rows of {row:?}"
                );
                let drawn = (0..)
                    .map_while(|index| cx.debug_bounds(format!("github-patch-line-{index}").leak()))
                    .collect::<Vec<_>>();
                assert!(
                    drawn.iter().all(|line| line.size.height == row),
                    "{at}: {drawn:?}"
                );
                let last = drawn.last().expect("rows drawn");
                assert!(
                    last.bottom() <= rows.bottom() + px(0.01),
                    "{at}: the last row drawn {last:?} runs past the rows' clip {rows:?}"
                );
                // Inside the 1 px border.
                let below = patch.bottom() - px(1.) - rows.bottom();
                assert!(
                    below > px(-0.01) && below < row,
                    "{at}: {below:?} below the rows {rows:?} in {patch:?}"
                );
                let (fills, lowest) = painted_in(cx, patch);
                assert_eq!(fills, vec![canvas], "{at}: the box's fill");
                assert!(
                    lowest <= rows.bottom() + px(0.01),
                    "{at}: paints down to {lowest:?}, below {rows:?}"
                );
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.run_until_parked();
    }

    /// In a 1480 × 980 window at 13 and 18 pt, the review box's bottom edge stays where it is
    /// between Changes and Source, level with the changed-file list's bottom: Changes' range
    /// hint took the box's last 26 px only in Changes, so switching modes moved the edge. The
    /// hint lies below the box in Changes and is not drawn in Source.
    #[gpui::test]
    async fn switching_modes_keeps_the_review_box_edge(cx: &mut TestAppContext) {
        let (_fixture, app, panel, cx) = review_panel(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.apply_appearance(window, cx)));
        cx.simulate_resize(size(px(1480.), px(980.)));
        let pull = model::fixture_pull(false);
        let captured_pull =
            pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured_pull, 1, &OperationControl::default())
            .unwrap()
            .items;
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.pull = Some(pull);
                panel.review.files = files;
                panel.review.section = Section::Files;
                panel.review.show_list = false;
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        cx.run_until_parked();
        for (interface, code) in [(13, 12), (18, 15)] {
            cx.update(|window, cx| appearance::apply_text_sizes(interface, code, window, cx));
            for file in [0, 1] {
                let mut edges = Vec::new();
                for (mode, selector) in [(0, "github-patch"), (1, "github-source-patch")] {
                    cx.update(|window, cx| {
                        panel.update(cx, |panel, cx| {
                            panel.select_review_file(file, window, cx);
                            panel.review_source(mode, window, cx);
                        })
                    });
                    cx.run_until_parked();
                    for _ in 0..3 {
                        cx.update(|window, cx| window.draw(cx).clear(cx));
                    }
                    let at = format!("{interface} pt, file {file}, {selector}");
                    let shown = cx.debug_bounds(selector).expect(selector);
                    let list = cx.debug_bounds("github-file-focus").expect("file list");
                    let hint = cx.debug_bounds("github-range-hint");
                    match mode {
                        0 => assert!(
                            hint.is_some_and(|hint| hint.top() >= shown.bottom()),
                            "{at}: the hint {hint:?} lies below the box {shown:?}"
                        ),
                        _ => assert_eq!(hint, None, "{at}"),
                    }
                    // From the list's top: the panel's body may scroll between the modes.
                    edges.push((shown.bottom() - list.top(), list.size.height));
                }
                assert!(
                    edges
                        .iter()
                        .all(|&(edge, list)| (edge - list).abs() < px(0.01)),
                    "{interface} pt, file {file}: Changes and Source end {edges:?} below the \
                     list's top (box, list)"
                );
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.run_until_parked();
    }

    /// In a 1480 × 980 window at 13 and 18 pt, Changes and decorated Source start their text at
    /// one place in the box they share: the first row and the first line one inset below the
    /// box's top, the editor's top padding, the old and new line numbers right-aligned where
    /// Compare's gutter paints Source's, and the code text where the editor draws its first
    /// character. Changes started flush with the box, its numbers left-aligned in columns that
    /// kept 36 px, so at 15 pt code its text began 12.5 px before Source's; then, measured
    /// against the editor's text area rather than its drawn text, 10 px before it.
    #[gpui::test]
    async fn changes_and_source_start_their_text_at_one_place(cx: &mut TestAppContext) {
        let (_fixture, app, panel, cx) = review_panel(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.apply_appearance(window, cx)));
        cx.simulate_resize(size(px(1480.), px(980.)));
        let pull = model::fixture_pull(false);
        let captured_pull =
            pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured_pull, 1, &OperationControl::default())
            .unwrap()
            .items;
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.pull = Some(pull);
                panel.review.files = files;
                panel.review.section = Section::Files;
                panel.review.show_list = false;
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        cx.run_until_parked();
        let show = |file: usize, mode: usize, cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.select_review_file(file, window, cx);
                    panel.review_source(mode, window, cx);
                })
            });
            cx.run_until_parked();
            for _ in 0..3 {
                cx.update(|window, cx| window.draw(cx).clear(cx));
            }
        };
        for (interface, code) in [(13, 12), (18, 15)] {
            cx.update(|window, cx| appearance::apply_text_sizes(interface, code, window, cx));
            for file in [0, 1] {
                let at = format!("{interface} pt, file {file}");
                show(file, 0, cx);
                // The first row with both line numbers: the hunk's header has none.
                let numbered = cx.read(|cx| {
                    panel.read(cx).review.files[file]
                        .rows
                        .iter()
                        .position(|row| row.old.is_some() && row.new.is_some())
                        .expect("a context row")
                });
                let bounds = |selector: String, cx: &mut gpui_kit::VisualTestContext| {
                    cx.debug_bounds(selector.clone().leak())
                        .unwrap_or_else(|| panic!("{selector}"))
                };
                let patch = bounds("github-patch".into(), cx);
                let first = bounds("github-patch-line-0".into(), cx);
                let text = bounds(format!("github-patch-text-{numbered}"), cx);
                let old = bounds(format!("github-patch-old-{numbered}"), cx);
                let new = bounds(format!("github-patch-new-{numbered}"), cx);
                let changes = (
                    first.top() - patch.top(),
                    text.left() - patch.left(),
                    old.right() - patch.left(),
                    new.right() - patch.left(),
                );

                show(file, 1, cx);
                assert!(
                    cx.read(|cx| panel.read(cx).review.source_view.is_some()),
                    "{at}: Source draws Compare's gutter"
                );
                let source = cx.debug_bounds("github-source-patch").expect("Source");
                let editor = cx.read(|cx| panel.read(cx).review.source.clone().unwrap());
                // Where the editor draws the first line's first character: past the margin
                // its code editor keeps inside the text area, not at the text area's edge.
                let (input, drawn) = cx.read(|cx| {
                    let editor = editor.read(cx);
                    (
                        editor.input_bounds(),
                        editor.range_to_bounds(&(0..0)).expect("laid out"),
                    )
                });
                // Compare's gutter paints each number this far inside its column's right edge,
                // its columns starting inside the box's 1 px border.
                let scale = appearance::code_scale();
                let column = cx.read(|cx| {
                    panel
                        .read(cx)
                        .review
                        .presentation
                        .as_ref()
                        .unwrap()
                        .column_width
                }) * scale;
                let padding = crate::diff_view::CELL_PADDING * scale;
                let painted = |columns: f32| px(1. + column * columns - padding);
                let shown = (
                    input.top() - source.top(),
                    drawn.left() - source.left(),
                    painted(1.),
                    painted(2.),
                );
                assert_eq!(
                    changes.0,
                    px(1.) + EDITOR_PADDING,
                    "{at}: the first row's inset"
                );
                assert!(
                    (changes.0 - shown.0).abs() < px(0.01)
                        && (changes.1 - shown.1).abs() < px(0.01),
                    "{at}: Changes starts its text at {changes:?}, Source at {shown:?} \
                     (top, text x, old and new numbers' right edges)"
                );
                // Within the device pixel layout puts each edge on.
                assert!(
                    (changes.2 - shown.2).abs() < px(0.51)
                        && (changes.3 - shown.3).abs() < px(0.51),
                    "{at}: Changes' numbers end at {changes:?}, Source's at {shown:?} \
                     (top, text x, old and new numbers' right edges)"
                );
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.run_until_parked();
    }

    /// A repository page and a review panel for it, built as the views above build them, with
    /// the panel's local read settled.
    fn review_panel(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        Entity<Panel>,
        &mut gpui_kit::VisualTestContext,
    ) {
        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["-c", "init.templateDir=", "init", "--initial-branch=main"])
                .arg(fixture.path())
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap()
                .status
                .success()
        );
        let repo = GitRepository::open(fixture.path()).unwrap();
        let panel_repo = repo.clone();
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let output = captured.clone();
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
        });
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    None,
                    Preferences::default(),
                    repository_tabs::Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                app.page = AppPage::Repository;
                app.path = Some(repo.path().to_owned());
                app.repository = Some(repo);
                app
            });
            *output.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = captured.borrow().as_ref().unwrap().clone();
        let panel = cx.update(|window, cx| {
            cx.new(|cx| Panel::new(app.downgrade(), panel_repo, "main".into(), window, cx))
        });
        app.read_with(cx, |app, _| app.operations.drain());
        cx.run_until_parked();
        (fixture, app, panel, cx)
    }
    /// 8,000 deletions and 8,000 additions of 520 bytes, just under the 8 MiB response bound:
    /// 16,001 inline rows, inside the 20,000-row limit, and above Compare's bounds, so no
    /// presentation is prepared.
    const LARGE_LINES: usize = 8_000;
    fn large_file(name: &str, fill: char) -> PullFile {
        let mut patch = format!("@@ -1,{LARGE_LINES} +1,{LARGE_LINES} @@\n");
        for marker in ['-', '+'] {
            for line in 0..LARGE_LINES {
                patch.push(marker);
                patch.push_str(&format!("{line:08} "));
                patch.extend(std::iter::repeat_n(fill, 510));
                patch.push('\n');
            }
        }
        assert!(patch.len() <= github::MAX_RESPONSE && patch.len() > 8_000_000);
        PullFile {
            filename: name.into(),
            previous_filename: None,
            status: "modified".into(),
            additions: LARGE_LINES as u32,
            deletions: LARGE_LINES as u32,
            patch: Some(patch.into()),
            rows: Vec::new(),
            unavailable: None,
        }
    }
    /// Selecting a file hands its preparation the patch the view holds, not a copy made on
    /// the UI thread, and the prepared file keeps that allocation.
    #[gpui::test]
    async fn selecting_a_review_file_shares_its_patch_with_the_preparation(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, panel, cx) = review_panel(cx);
        let file = large_file("src/large.rs", 'a');
        // Compared, never read: the address of the bytes the view holds.
        let held = file.patch.as_deref().expect("a patch").as_ptr();
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.review.files = vec![file];
                panel.review.section = Section::Files;
                panel.select_review_file(0, window, cx);
            })
        });
        cx.run_until_parked();
        cx.read(|cx| {
            let review = &panel.read(cx).review;
            assert!(!review.preparing);
            assert_eq!(review.files[0].rows.len(), 2 * LARGE_LINES + 1);
            assert_eq!(
                review.files[0].patch.as_deref().expect("a patch").as_ptr(),
                held,
                "the preparation took the view's allocation"
            );
        });
        app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.run_until_parked();
    }
    /// Closing the panel cancels the selected file's pending preparation; reopening resumes it
    /// without the file being chosen again, in Changes and in Source, which keeps its editor
    /// and takes the decorations Compare would draw. A preparation that failed is not repeated.
    #[gpui::test]
    async fn reopening_resumes_an_interrupted_file_preparation(cx: &mut TestAppContext) {
        let (_fixture, app, panel, cx) = review_panel(cx);
        let patches = [
            "@@ -1,2 +1,2 @@\n context\n-old value\n+new value\n",
            "@@ -1 +1,2 @@\n-before\n+after\n+more\n",
        ];
        let file = |name: &str, patch: &str, additions| PullFile {
            filename: name.into(),
            previous_filename: None,
            status: "modified".into(),
            additions,
            deletions: 1,
            patch: Some(patch.into()),
            rows: Vec::new(),
            unavailable: None,
        };
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.review.files = vec![
                    file("src/a.rs", patches[0], 1),
                    file("src/b.rs", patches[1], 2),
                    // A path the preparation refuses.
                    file("../outside.rs", patches[0], 1),
                ];
                panel.review.section = Section::Files;
            })
        });
        let close_and_reopen = |cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
            // The cancelled preparation's result, if any, has nowhere to land.
            cx.run_until_parked();
            cx.read(|cx| {
                let review = &panel.read(cx).review;
                assert!(!review.preparing);
                assert!(review.files.iter().all(|file| file.rows.is_empty()));
                assert!(review.presentation.is_none());
            });
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
            app.read_with(cx, |app, _| app.operations.drain());
            cx.run_until_parked();
        };

        // Changes: the rows and presentation arrive after reopening.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| panel.select_review_file(0, window, cx))
        });
        close_and_reopen(cx);
        cx.read(|cx| {
            let review = &panel.read(cx).review;
            assert_eq!((review.selected_file, review.mode), (Some(0), 0));
            assert!(!review.preparing);
            assert_eq!(review.files[0].rows.len(), 4);
            assert!(review.presentation.is_some());
        });

        // Source: the editor opened before the close is kept and decorated, with Compare's
        // gutter, as Compare decorates the same text.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.select_review_file(1, window, cx);
                panel.review_source(1, window, cx);
            })
        });
        let source = cx.read(|cx| panel.read(cx).review.source.clone().expect("Source editor"));
        close_and_reopen(cx);
        let expected = cx.update(|window, cx| {
            let presentation = text::PatchPresentation::prepare(patches[1]);
            let (_editor, decorations) =
                text::editor_with_decorations(patches[1], "diff", Some(&presentation), window, cx);
            decorations
                .expect("a prepared patch is decorated")
                .drawn(cx)
        });
        cx.read(|cx| {
            let review = &panel.read(cx).review;
            assert_eq!((review.selected_file, review.mode), (Some(1), 1));
            assert_eq!(review.files[1].rows.len(), 4);
            assert_eq!(
                review.source.as_ref(),
                Some(&source),
                "Source keeps its editor"
            );
            assert!(
                review.source_view.is_some(),
                "Source takes Compare's gutter"
            );
            assert_eq!(
                editor_find::patch_layer(&source, cx).map(|layer| layer.drawn(cx)),
                Some(expected)
            );
        });

        // A failed preparation reports once; reopening does not run it again.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| panel.select_review_file(2, window, cx))
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                assert!(panel.error.take().is_some(), "the refused path is reported");
            })
        });
        close_and_reopen(cx);
        cx.read(|cx| {
            let panel = panel.read(cx);
            assert_eq!(panel.review.selected_file, Some(2));
            assert_eq!(panel.error, None, "the failed preparation is not repeated");
        });
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.run_until_parked();
    }
    /// The UI-thread time of `select_review_file` between two files whose patches are just
    /// under the 8 MiB response bound: after the previous file finished preparing, and while it
    /// is still preparing. Only the handler is timed; preparation runs untimed between samples.
    #[gpui::test]
    #[ignore = "Opt-in release benchmark: set GITTURTLE_REVIEW_SELECT_BENCH_OUTPUT and run with --release --ignored"]
    async fn select_review_file_release_benchmark(cx: &mut TestAppContext) {
        const WARMUPS: usize = 5;
        const SAMPLES: usize = 60;
        assert!(
            !std::hint::black_box(cfg!(debug_assertions)),
            "performance evidence requires --release"
        );
        let output = std::env::var_os("GITTURTLE_REVIEW_SELECT_BENCH_OUTPUT")
            .expect("the runner supplies the result destination");
        let (_fixture, app, panel, cx) = review_panel(cx);
        let first = large_file("src/first.rs", 'a');
        let patch_bytes = first.patch.as_deref().map_or(0, str::len);
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.review.files = vec![first, large_file("src/second.rs", 'b')];
                panel.review.section = Section::Files;
            })
        });
        let select = |index: usize, cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    let started = std::time::Instant::now();
                    panel.select_review_file(index, window, cx);
                    started.elapsed().as_secs_f64() * 1000.
                })
            })
        };
        let mut series = Vec::new();
        for (name, prepared) in [("after_preparation", true), ("while_preparing", false)] {
            select(0, cx);
            cx.run_until_parked();
            let mut samples = Vec::new();
            for attempt in 0..WARMUPS + SAMPLES {
                let index = (attempt + 1) % 2;
                let milliseconds = select(index, cx);
                if prepared {
                    cx.run_until_parked();
                    cx.read(|cx| {
                        let review = &panel.read(cx).review;
                        assert!(!review.preparing);
                        assert_eq!(review.files[index].rows.len(), 2 * LARGE_LINES + 1);
                    });
                } else {
                    cx.read(|cx| assert!(panel.read(cx).review.preparing));
                }
                if attempt >= WARMUPS {
                    samples.push(milliseconds);
                }
            }
            cx.run_until_parked();
            series.push(serde_json::json!({"name": name, "milliseconds": samples}));
        }
        let result = serde_json::json!({
            "schema": 1,
            "warmups_per_series": WARMUPS,
            "samples_per_series": SAMPLES,
            "patch_bytes": patch_bytes,
            "rows_per_file": 2 * LARGE_LINES + 1,
            "boundary": "One `select_review_file` call on the UI thread, from before the call to its return; excludes GPUI's effect flush, rendering and the background preparation.",
            "series": series,
        });
        std::fs::write(output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
        app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.run_until_parked();
    }
}
