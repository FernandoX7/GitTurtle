//! Native PR file navigation, precise inline selection and a durable collected review.
use super::*;
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
    pub mode: usize,
    pub file_focus: FocusHandle,
    pub line_focus: FocusHandle,
    pub section_focus: FocusHandle,
    pub preparing: bool,
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
            mode: 0,
            file_focus: cx.focus_handle(),
            line_focus: cx.focus_handle(),
            section_focus: cx.focus_handle(),
            preparing: false,
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
    }
    pub fn reset_content(&mut self) {
        self.cancel_preparation();
        self.files.clear();
        self.selected_file = None;
        self.selected_row = None;
        self.anchor = None;
        self.source = None;
        self.file_page = 1;
        self.files_next = false;
    }
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
        self.review.source = None;
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
        self.review.source = None;
        self.review.mode = 0;
        self.review
            .file_scroll
            .scroll_to_item(index, ScrollStrategy::Center);
        self.review
            .line_scroll
            .scroll_to_item(0, ScrollStrategy::Top);
        self.review.file_focus.focus(window, cx);
        let mut file = self.review.files[index].clone();
        let generation = self.review.prepare_generation;
        let response = self.owner.update(cx, |owner, _| {
            owner.operations.submit_read(move || {
                file.prepare()?;
                Ok(file)
            })
        });
        let Ok(response) = response else {
            self.error = Some("The repository window is unavailable.".into());
            cx.notify();
            return;
        };
        self.review.preparing = true;
        self.review.prepare_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, _, cx| {
                if this.closed
                    || generation != this.review.prepare_generation
                    || this.review.selected_file != Some(index)
                {
                    return;
                }
                this.review.preparing = false;
                match result {
                    Ok(Ok(file)) => this.review.files[index] = file,
                    Ok(Err(error)) => this.error = Some(format!("{error:#}")),
                    Err(_) => {
                        this.error =
                            Some("File preparation was interrupted. Choose the file again.".into())
                    }
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
        let selection=self.review.selected_file.and_then(|file|Some((file,self.review.anchor?,self.review.selected_row?))).ok_or_else(||anyhow::anyhow!("Select an added or deleted line. Shift-click or Shift-Up/Down extends a same-side range.")).and_then(|(file,start,end)|self.review.files[file].selection(start,end));
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
                .and_then(|n| self.review.files[n].patch.clone())
        {
            self.review.source = Some(text::editor(&patch, "diff", None, window, cx));
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
    pub(super) fn render_files(&self, cx: &mut Context<Self>) -> AnyElement {
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
                    .h(px(324.))
                    .border_1()
                    .border_color(rgb(p.border))
                    .rounded(px(6.))
                    .overflow_hidden()
                    .tab_stop(true)
                    .track_focus(&self.review.file_focus)
                    .focus_visible(|style| style.border_color(rgb(p.accent)))
                    .role(Role::ListBox)
                    .aria_label("Pull request changed files")
                    .aria_description("Up and Down select a file. Return moves to its patch.")
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
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        button("github-files-previous", "Previous", "", false)
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
            div().flex_1().min_w_0().flex().flex_col().gap_2()
                .child(label("github-file-title",title).font_weight(FontWeight::SEMIBOLD))
                .child(div().flex().flex_wrap().items_center().gap_1().children(["Changes","Source","Threads"].into_iter().enumerate().map(|(mode,name)|button(("github-file-mode",mode),name,"",self.review.mode==mode).toggled(self.review.mode==mode).on_click(cx.listener(move|this,_,window,cx|this.review_source(mode,window,cx)))))
                    .child(div().flex_1()).child(button("github-inline-start",if self.review.composing.is_some(){"Continue comment"}else{"Comment on selection"},"plus",false).disabled(self.pending||self.review.preparing||file.unavailable.is_some()).on_click(cx.listener(|this,_,window,cx|this.begin_comment(window,cx)))))
                .child(match self.review.mode {1=>self.review.source.as_ref().map(|source|div().h(px(246.)).flex_shrink_0().child(crate::editor_find::Editor::new(source).readonly(true).h_full().aria_label("Exact GitHub supplied patch source")).into_any_element()).unwrap_or_else(||label("github-no-source","No text patch was supplied. Open the captured local comparison to inspect available source or binary content.").into_any_element()),2=>self.render_threads(Some(&file.filename),cx),_=>self.render_patch(cx)})
                .when(self.review.mode==0,|element|element.child(label("github-range-hint","Click a changed line · Shift-click or Shift-Up/Down selects a same-side range · Return comments").text_color(rgb(p.muted))))
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
    fn render_patch(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let Some(file) = self
            .review
            .selected_file
            .and_then(|n| self.review.files.get(n))
        else {
            return div().into_any_element();
        };
        if self.review.preparing {
            return label("github-preparing-file", "Preparing the selected file…")
                .role(Role::Status)
                .into_any_element();
        }
        if let Some(reason) = &file.unavailable {
            return div()
                .p_4()
                .bg(rgb(p.subtle))
                .rounded(px(6.))
                .child(label("github-patch-unavailable", reason.clone()))
                .into_any_element();
        }
        div().id("github-patch-focus").key_context("GitTurtleGithubPatch").on_action(cx.listener(|this,_:&ReviewUp,window,cx|{this.review_key("up",false,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewDown,window,cx|{this.review_key("down",false,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewHome,window,cx|{this.review_key("home",false,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewEnd,window,cx|{this.review_key("end",false,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewPageUp,window,cx|{this.review_key("pageup",false,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewPageDown,window,cx|{this.review_key("pagedown",false,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewExtendUp,window,cx|{this.review_key("up",true,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewExtendDown,window,cx|{this.review_key("down",true,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewAccept,window,cx|{this.review_key("enter",false,window,cx);cx.stop_propagation();})).h(px(246.)).min_w_0().border_1().border_color(rgb(p.border)).rounded(px(6.)).overflow_hidden().tab_stop(true).track_focus(&self.review.line_focus).focus_visible(|style|style.border_color(rgb(p.accent))).role(Role::ListBox).aria_label("Captured pull request patch lines").aria_description("Up and Down move through lines. Shift extends a same-side range. Return starts an inline comment. Source offers exact text selection and Find.")

            .child(uniform_list("github-patch-rows",file.rows.len(),cx.processor(|this,range:std::ops::Range<usize>,_,cx|range.map(|index|{let file=&this.review.files[this.review.selected_file.expect("visible file")];let row=&file.rows[index];let selected=this.review.anchor.zip(this.review.selected_row).is_some_and(|(a,b)|(a.min(b)..=a.max(b)).contains(&index));let p=palette(cx);let background=if selected{p.selected}else{match row.kind{b'+'=>p.added_background,b'-'=>p.removed_background,b'@'=>p.subtle,_=>p.canvas}};let row_label=format!("Before {} · After {} · {}",row.old.map(|n|n.to_string()).unwrap_or_default(),row.new.map(|n|n.to_string()).unwrap_or_default(),row.text);
                div().id(("github-patch-line",index)).w_full().h(px(f32::from(appearance::code_text())*1.65)).px_2().flex().items_center().gap_2().bg(rgb(background)).border_l_2().border_color(rgb(if selected{p.accent}else{background})).role(Role::ListBoxOption).aria_label(row_label).aria_selected(selected).cursor_pointer().text_size(appearance::code_text()).font_family("Menlo")
                    .child(div().w(px(36.)).flex_shrink_0().text_color(rgb(p.muted)).child(row.old.map(|n|n.to_string()).unwrap_or_default()))
                    .child(div().w(px(36.)).flex_shrink_0().text_color(rgb(p.muted)).child(row.new.map(|n|n.to_string()).unwrap_or_default()))
                    .child(div().min_w_0().flex_1().truncate().text_color(rgb(if row.kind==b'@'{p.hunk}else{p.text})).child(row.text.clone()))
                    .on_click(cx.listener(move|this,event:&ClickEvent,window,cx|this.select_review_line(index,event.modifiers().shift,window,cx))).into_any_element()
            }).collect::<Vec<_>>())).size_full().track_scroll(&self.review.line_scroll)).into_any_element()
    }
    pub(super) fn render_collected(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        div().flex().flex_col().gap_2()
            .child(label("github-review-heading",format!("{} inline comments in this review",self.review.comments.len())).font_weight(FontWeight::SEMIBOLD))
            .when(self.review.comments.is_empty(),|element|element.child(label("github-review-empty","Select changed lines in Files to add inline feedback, or send a review summary below.").text_color(rgb(p.muted))))
            .child(div().id("github-collected-comments").debug_selector(||"github-collected-comments".into()).flex_shrink_0().max_h(px(230.)).overflow_y_scroll().flex().flex_col().gap_2().children(self.review.comments.iter().enumerate().map(|(index,comment)|div().flex_shrink_0().p_3().border_1().border_color(rgb(p.border)).rounded(px(6.)).flex().flex_col().gap_2()
                .child(label(("github-draft-position",index),model::position_label(comment)).font_weight(FontWeight::SEMIBOLD))
                .child(label(("github-draft-body",index),comment.body.clone()))
                .child(div().flex().gap_2().child(button(("github-edit-inline",index),"Edit","",false).disabled(self.pending||self.review.composing.is_some()).on_click(cx.listener(move|this,_,window,cx|this.edit_comment(index,window,cx))))
                    .child(button(("github-remove-inline",index),"Remove","",false).disabled(self.pending).on_click(cx.listener(move|this,_,window,cx|{if index<this.review.comments.len(){this.review.comments.remove(index);this.confirm=None;this.save_draft(window,cx);this.focus_visible_section(window,cx);}})))))))
            .child(self.render_comment_composer(cx)).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::component::Root;
    use std::{cell::RefCell, rc::Rc};

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
        app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
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
        app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
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
}
