//! Native PR file navigation, precise inline selection and a durable collected review.
use super::*;
use crate::github::{
    CapturedPull, LineComment,
    review::{self as model, PullFile, ReviewComment, UiTransport},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Section {
    Overview,
    Files,
    Review,
}
pub(super) struct ReviewState {
    pub section: Section,
    pub files: Vec<PullFile>,
    pub file_page: u32,
    pub files_next: bool,
    pub discussions: Vec<ReviewComment>,
    pub thread_page: u32,
    pub threads_next: bool,
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
            files: vec![],
            file_page: 1,
            files_next: false,
            discussions: vec![],
            thread_page: 1,
            threads_next: false,
            selected_file: None,
            selected_row: None,
            anchor: None,
            comments: vec![],
            composing: None,
            input: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .rows(3)
                    .placeholder("Explain this change…")
            }),
            source: None,
            mode: 0,
            file_focus: cx.focus_handle(),
            line_focus: cx.focus_handle(),
            file_scroll: UniformListScrollHandle::new(),
            line_scroll: UniformListScrollHandle::new(),
            fixture: std::env::var("GITTURTLE_GITHUB_FIXTURE").is_ok_and(|value| value == "review"),
            fixture_moved: false,
            show_list: true,
            connection_open: false,
            recovery_open: false,
        }
    }
    pub fn reset_content(&mut self) {
        self.files.clear();
        self.discussions.clear();
        self.selected_file = None;
        self.selected_row = None;
        self.anchor = None;
        self.source = None;
        self.file_page = 1;
        self.thread_page = 1;
        self.files_next = false;
        self.threads_next = false;
    }
}
impl Panel {
    fn captured(&self, cx: &App) -> Option<CapturedPull> {
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
        self.discussion = false;
    }
    fn change_section(&mut self, section: Section, window: &mut Window, cx: &mut Context<Self>) {
        self.save_draft(window, cx);
        self.review.section = section;
        self.confirm = None;
        if section == Section::Files {
            self.review.file_focus.focus(window, cx);
        }
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
                Ok(page) => {
                    this.review.files = page.items;
                    this.review.file_page = page.page;
                    this.review.files_next = page.has_next;
                    this.review.selected_file = None;
                    this.review.selected_row = None;
                    this.review.anchor = None;
                    this.review.source = None;
                }
                Err(error) => this.error = Some(format!("{error:#}")),
            },
            window,
            cx,
        );
    }
    fn thread_page(&mut self, page: u32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pull) = self.captured(cx) else {
            return;
        };
        let fixture = self.review.fixture;
        let moved = self.review.fixture_moved;
        self.run(
            false,
            move |control| {
                Client(UiTransport::open(fixture, moved)?).comments(&pull, page, &control)
            },
            |result, this, _, _| match result {
                Ok(page) => {
                    this.review.discussions = page.items;
                    this.review.thread_page = page.page;
                    this.review.threads_next = page.has_next;
                }
                Err(error) => this.error = Some(format!("{error:#}")),
            },
            window,
            cx,
        );
    }
    fn select_review_file(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.review.files.len() {
            return;
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
        self.review.line_focus.focus(window, cx);
        cx.notify();
    }
    fn edit_comment(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
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
        cx.notify();
    }
    fn review_source(&mut self, mode: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.review.mode = mode;
        if mode == 1
            && self.review.source.is_none()
            && let Some(patch) = self
                .review
                .selected_file
                .and_then(|n| self.review.files[n].patch.clone())
        {
            self.review.source = Some(text::editor(&patch, "diff", None, window, cx));
        }
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
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if event.keystroke.modifiers.platform || event.keystroke.modifiers.control {
                            return;
                        }
                        let current = this.review.selected_file.unwrap_or(0);
                        let index = match event.keystroke.key.as_str() {
                            "up" => current.saturating_sub(1),
                            "down" => (current + 1).min(this.review.files.len().saturating_sub(1)),
                            "home" => 0,
                            "end" => this.review.files.len().saturating_sub(1),
                            "enter" => {
                                this.review.line_focus.focus(window, cx);
                                cx.stop_propagation();
                                return;
                            }
                            _ => return,
                        };
                        this.select_review_file(index, window, cx);
                        cx.stop_propagation();
                    }))
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
                    .child(div().flex_1()).child(button("github-inline-start",if self.review.composing.is_some(){"Continue comment"}else{"Comment on selection"},"plus",false).disabled(self.pending||file.unavailable.is_some()).on_click(cx.listener(|this,_,window,cx|this.begin_comment(window,cx)))))
                .child(match self.review.mode {1=>self.review.source.as_ref().map(|source|crate::editor_find::Editor::new(source).readonly(true).h(px(246.)).aria_label("Exact GitHub supplied patch source").into_any_element()).unwrap_or_else(||label("github-no-source","No text patch was supplied. Open the captured local comparison to inspect available source or binary content.").into_any_element()),2=>self.render_threads(Some(&file.filename),cx),_=>self.render_patch(cx)})
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
        if let Some(reason) = &file.unavailable {
            return div()
                .p_4()
                .bg(rgb(p.subtle))
                .rounded(px(6.))
                .child(label("github-patch-unavailable", reason.clone()))
                .into_any_element();
        }
        div().id("github-patch-focus").h(px(246.)).min_w_0().border_1().border_color(rgb(p.border)).rounded(px(6.)).overflow_hidden().tab_stop(true).track_focus(&self.review.line_focus).focus_visible(|style|style.border_color(rgb(p.accent))).role(Role::ListBox).aria_label("Captured pull request patch lines").aria_description("Up and Down move through lines. Shift extends a same-side range. Return starts an inline comment. Source offers exact text selection and Find.")
            .on_key_down(cx.listener(|this,event:&KeyDownEvent,window,cx|{if event.keystroke.modifiers.platform||event.keystroke.modifiers.control{return;}let count=this.review.selected_file.map(|n|this.review.files[n].rows.len()).unwrap_or(0);let current=this.review.selected_row.unwrap_or(0);let index=match event.keystroke.key.as_str(){"up"=>current.saturating_sub(1),"down"=>(current+1).min(count.saturating_sub(1)),"home"=>0,"end"=>count.saturating_sub(1),"pageup"=>current.saturating_sub(8),"pagedown"=>(current+8).min(count.saturating_sub(1)),"enter"=>{this.begin_comment(window,cx);cx.stop_propagation();return;},_=>return};this.select_review_line(index,event.keystroke.modifiers.shift,window,cx);cx.stop_propagation();}))
            .child(uniform_list("github-patch-rows",file.rows.len(),cx.processor(|this,range:std::ops::Range<usize>,_,cx|range.map(|index|{let file=&this.review.files[this.review.selected_file.expect("visible file")];let row=&file.rows[index];let selected=this.review.anchor.zip(this.review.selected_row).is_some_and(|(a,b)|(a.min(b)..=a.max(b)).contains(&index));let p=palette(cx);let background=if selected{p.selected}else{match row.kind{b'+'=>p.added_background,b'-'=>p.removed_background,b'@'=>p.subtle,_=>p.canvas}};let row_label=format!("Before {} · After {} · {}",row.old.map(|n|n.to_string()).unwrap_or_default(),row.new.map(|n|n.to_string()).unwrap_or_default(),row.text);
                div().id(("github-patch-line",index)).h(px(f32::from(appearance::code_text())*1.65)).px_2().flex().items_center().gap_2().bg(rgb(background)).border_l_2().border_color(rgb(if selected{p.accent}else{background})).role(Role::ListBoxOption).aria_label(row_label).aria_selected(selected).cursor_pointer().text_size(appearance::code_text()).font_family("Menlo")
                    .child(div().w(px(36.)).flex_shrink_0().text_color(rgb(p.line_number)).child(row.old.map(|n|n.to_string()).unwrap_or_default()))
                    .child(div().w(px(36.)).flex_shrink_0().text_color(rgb(p.line_number)).child(row.new.map(|n|n.to_string()).unwrap_or_default()))
                    .child(div().min_w_0().flex_1().truncate().text_color(rgb(if row.kind==b'@'{p.hunk}else{p.text})).child(row.text.clone()))
                    .on_click(cx.listener(move|this,event:&ClickEvent,window,cx|this.select_review_line(index,event.modifiers().shift,window,cx))).into_any_element()
            }).collect::<Vec<_>>())).size_full().track_scroll(&self.review.line_scroll)).into_any_element()
    }
    pub(super) fn render_threads(&self, path: Option<&str>, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let Some(pull) = self.captured(cx) else {
            return div().into_any_element();
        };
        let threads = model::threads(&self.review.discussions);
        let visible = threads
            .into_iter()
            .filter(|thread| {
                path.is_none_or(|path| thread.comments.iter().any(|comment| comment.path == path))
            })
            .collect::<Vec<_>>();
        div().flex().flex_col().gap_2().child(div().id("github-threads").max_h(px(250.)).overflow_y_scroll().flex().flex_col().gap_2()
            .when(visible.is_empty(),|element|element.child(label("github-no-threads","No inline discussions on this loaded page.")))
            .children(visible.into_iter().map(|thread|div().p_3().rounded(px(6.)).bg(rgb(p.subtle)).border_1().border_color(rgb(p.border)).flex().flex_col().gap_2()
                .when(thread.missing_root,|element|element.child(label(("github-thread-missing",thread.root_id),"Opening comment is on another page; replies remain readable.").text_color(rgb(p.warning))))
                .children(thread.comments.iter().map(|comment|div().flex().flex_col().gap_1().child(label(("github-comment-author",comment.id),format!("{}{}",comment.user.login,if comment.in_reply_to_id.is_some(){" · reply"}else{""})).font_weight(FontWeight::SEMIBOLD))
                    .child(label(("github-comment-location",comment.id),comment.location(&pull)).text_color(rgb(if comment.outdated(&pull){p.warning}else{p.muted})))
                    .when(comment.outdated(&pull),|element|element.child(label(("github-comment-original",comment.id),format!("Original commit {}",if comment.original_commit_id.is_empty(){&comment.commit_id}else{&comment.original_commit_id})).text_color(rgb(p.muted))))
                    .child(label(("github-comment-body",comment.id),comment.body.clone())))))))
            .child(div().flex().items_center().gap_2().child(button("github-threads-previous","Previous discussions","",false).disabled(self.pending||self.review.thread_page<=1).on_click(cx.listener(|this,_,window,cx|this.thread_page(this.review.thread_page-1,window,cx))))
                .child(label("github-threads-page",format!("Page {} · up to 100 comments",self.review.thread_page)))
                .child(button("github-threads-next","Next discussions","",false).disabled(self.pending||!self.review.threads_next).on_click(cx.listener(|this,_,window,cx|this.thread_page(this.review.thread_page+1,window,cx))))).into_any_element()
    }
    pub(super) fn render_collected(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        div().flex().flex_col().gap_2()
            .child(label("github-review-heading",format!("{} inline comments in this review",self.review.comments.len())).font_weight(FontWeight::SEMIBOLD))
            .when(self.review.comments.is_empty(),|element|element.child(label("github-review-empty","Select changed lines in Files to add inline feedback, or send a review summary below.").text_color(rgb(p.muted))))
            .child(div().id("github-collected-comments").max_h(px(230.)).overflow_y_scroll().flex().flex_col().gap_2().children(self.review.comments.iter().enumerate().map(|(index,comment)|div().p_3().border_1().border_color(rgb(p.border)).rounded(px(6.)).flex().flex_col().gap_2()
                .child(label(("github-draft-position",index),model::position_label(comment)).font_weight(FontWeight::SEMIBOLD))
                .child(label(("github-draft-body",index),comment.body.clone()))
                .child(div().flex().gap_2().child(button(("github-edit-inline",index),"Edit","",false).disabled(self.pending||self.review.composing.is_some()).on_click(cx.listener(move|this,_,window,cx|this.edit_comment(index,window,cx))))
                    .child(button(("github-remove-inline",index),"Remove","",false).disabled(self.pending).on_click(cx.listener(move|this,_,window,cx|{if index<this.review.comments.len(){this.review.comments.remove(index);this.confirm=None;this.save_draft(window,cx);}})))))))
            .child(self.render_comment_composer(cx)).into_any_element()
    }
}
