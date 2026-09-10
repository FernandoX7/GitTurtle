//! Authoritative native conversations and independent, captured reply drafts.
use super::*;
use crate::github::{
    conversations::{CapturedThread, CursorPage, Thread},
    drafts::ReplyDraft,
};

pub(super) struct State {
    pub threads: Vec<Thread>,
    pub cursors: Vec<Option<String>>,
    pub next_cursor: Option<String>,
    pub total: u32,
    pub loaded: bool,
    pub pagination_notice: Option<String>,
    pub invalid_threads: std::collections::HashSet<String>,
    pub active: Option<CapturedThread>,
    pub input: Entity<TextareaState>,
    pub deferred: DeferredDraft,
    pub timer: Option<Task<()>>,
    pub confirm_focus: FocusHandle,
}
impl State {
    pub fn new(window: &mut Window, cx: &mut Context<Panel>) -> Self {
        Self {
            threads: vec![],
            cursors: vec![None],
            next_cursor: None,
            total: 0,
            loaded: false,
            pagination_notice: None,
            invalid_threads: Default::default(),
            active: None,
            input: cx.new(|cx| {
                TextareaState::new(window, cx)
                    // Bounded prose layout lets Tab traverse controls instead of indenting.
                    .auto_grow(3, 3)
                    .placeholder("Write a reply to this conversation…")
            }),
            deferred: DeferredDraft::default(),
            timer: None,
            confirm_focus: cx.focus_handle(),
        }
    }
    pub fn reset_content(&mut self) {
        self.threads.clear();
        self.cursors = vec![None];
        self.next_cursor = None;
        self.total = 0;
        self.loaded = false;
        self.pagination_notice = None;
        self.invalid_threads.clear();
        self.active = None;
    }
}

fn reply_matches(thread: &Thread, target: &CapturedThread, account: Option<&str>) -> bool {
    thread.target == *target && account == Some(target.account.as_str()) && thread.reply_available()
}

fn accept_page_cursor(
    history: &mut Vec<Option<String>>,
    cursor: Option<String>,
    next: Option<String>,
) -> Option<String> {
    if let Some(index) = history.iter().position(|old| old == &cursor) {
        history.truncate(index + 1);
    } else {
        history.push(cursor);
    }
    next.filter(|next| {
        history.len() < 1000 && !history.iter().any(|old| old.as_ref() == Some(next))
    })
}

impl Panel {
    pub(super) fn conversation_bytes(&self, cx: &App) -> usize {
        self.conversations
            .threads
            .iter()
            .map(Thread::retained_bytes)
            .sum::<usize>()
            + self
                .conversations
                .cursors
                .iter()
                .filter_map(Option::as_ref)
                .map(String::len)
                .sum::<usize>()
            + self
                .conversations
                .next_cursor
                .as_ref()
                .map_or(0, String::len)
            + self
                .conversations
                .active
                .as_ref()
                .map_or(0, CapturedThread::retained_bytes)
            + self.conversations.input.read(cx).value().len() * 3
            + self
                .conversations
                .invalid_threads
                .iter()
                .map(String::len)
                .sum::<usize>()
            + self
                .conversations
                .pagination_notice
                .as_ref()
                .map_or(0, String::len)
            + self
                .conversations
                .deferred
                .draft
                .as_ref()
                .map_or(0, |d| match d {
                    Draft::Reply(r) => r.body.len() + r.target.retained_bytes(),
                    _ => 0,
                })
    }
    pub(super) fn reply_snapshot(&self, cx: &App) -> Option<Draft> {
        self.conversations.active.as_ref().map(|target| {
            Draft::Reply(ReplyDraft {
                target: target.clone(),
                body: self.conversations.input.read(cx).value().to_string(),
            })
        })
    }
    pub(super) fn defer_reply_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.conversations.timer = None;
        self.conversations.deferred.clear();
        if let Some(draft) = self.reply_snapshot(cx) {
            let generation = self.conversations.deferred.replace(draft);
            self.save_status = "Saving local reply…".into();
            let timer = cx.background_executor().timer(DRAFT_QUIET_PERIOD);
            self.conversations.timer = Some(cx.spawn_in(window, async move |this, cx| {
                timer.await;
                let _ = this.update_in(cx, |this, window, cx| {
                    if let Some(draft) = this.conversations.deferred.take(generation) {
                        this.conversations.timer = None;
                        if !this.closed {
                            this.persist_draft(draft, window, cx);
                        }
                    }
                });
            }));
        }
        cx.notify();
    }
    pub(super) fn save_reply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.conversations.timer = None;
        self.conversations.deferred.clear();
        if let Some(draft) = self.reply_snapshot(cx) {
            self.persist_draft(draft, window, cx);
        }
    }
    pub(super) fn receive_threads(&mut self, page: CursorPage<Thread>, cursor: Option<String>) {
        self.conversations.invalid_threads.clear();
        self.conversations.threads = page.items;
        self.conversations.total = page.total_count;
        let had_next = page.next_cursor.is_some();
        self.conversations.next_cursor =
            accept_page_cursor(&mut self.conversations.cursors, cursor, page.next_cursor);
        self.conversations.pagination_notice = (had_next && self.conversations.next_cursor.is_none()).then(||"Further conversation pages are unavailable: the provider repeated a cursor or this inspection reached its 1,000-page limit. Refresh conversations to start a new inspection.".into());
        self.conversations.loaded = true;
    }
    fn invalidate_thread_context(&mut self, target: Option<&CapturedThread>) {
        for thread in &mut self.conversations.threads {
            if target.is_none_or(|target| &thread.target == target) {
                thread.viewer_can_reply = false;
                thread.viewer_can_resolve = false;
                thread.viewer_can_unresolve = false;
                self.conversations
                    .invalid_threads
                    .insert(thread.target.thread_id.clone());
            }
        }
        self.confirm = None;
    }
    fn receive_thread(&mut self, thread: Thread) {
        if let Some(old) = self
            .conversations
            .threads
            .iter_mut()
            .find(|old| old.target == thread.target)
        {
            self.conversations
                .invalid_threads
                .remove(&thread.target.thread_id);
            *old = thread;
        }
    }
    pub(super) fn restore_reply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let saved = self.saved.iter().find_map(|draft| match draft {
            Draft::Reply(reply)
                if !reply.body.is_empty()
                    && !drafts::reply_was_completed(&self.attempts, reply)
                    && self.conversations.threads.iter().any(|thread| {
                        reply_matches(thread, &reply.target, self.account.as_deref())
                    }) =>
            {
                Some(reply.clone())
            }
            _ => None,
        });
        if let Some(reply) = saved {
            self.conversations.active = Some(reply.target);
            self.conversations
                .input
                .update(cx, |input, cx| input.set_value(reply.body, window, cx));
            self.notice =
                Some("Unfinished reply restored with its exact conversation and account.".into());
        }
    }
    fn thread_page(&mut self, cursor: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pull) = self.captured(cx) else {
            return;
        };
        self.save_draft(window, cx);
        let fixture = self.review.fixture;
        let moved = self.review.fixture_moved;
        let requested = cursor.clone();
        self.confirm = None;
        self.run(
            false,
            move |control| {
                Client(UiTransport::open(fixture, moved)?).review_threads(
                    &pull,
                    requested.as_deref(),
                    &control,
                )
            },
            move |result, this, _, _| match result {
                Ok(page) => this.receive_threads(page, cursor),
                Err(error) => {
                    this.invalidate_thread_context(None);
                    this.error = Some(format!("Conversations could not be loaded: {error:#}. Cached conversation actions are disabled until a successful explicit refresh."))
                }
            },
            window,
            cx,
        );
    }
    fn thread_comments(
        &mut self,
        target: CapturedThread,
        cursor: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.save_draft(window, cx);
        let fixture = self.review.fixture;
        let moved = self.review.fixture_moved;
        let captured = target.clone();
        self.confirm = None;
        self.run(
            false,
            move |control| {
                Client(UiTransport::open(fixture, moved)?).thread_comments(
                    &target,
                    cursor.as_deref(),
                    &control,
                )
            },
            move |result, this, _, _| match result {
                Ok(thread) => this.receive_thread(thread),
                Err(error) => {
                    this.invalidate_thread_context(Some(&captured));
                    this.error = Some(format!(
                        "Conversation context could not be loaded: {error:#}. This conversation’s actions are disabled until a successful explicit refresh."
                    ))
                }
            },
            window,
            cx,
        );
    }
    fn begin_reply(&mut self, target: CapturedThread, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending
            || !self
                .conversations
                .threads
                .iter()
                .any(|thread| reply_matches(thread, &target, self.account.as_deref()))
        {
            self.error = Some("This conversation cannot accept a reply in the captured account and PR context. Refresh conversations explicitly, or copy the saved draft.".into());
            cx.notify();
            return;
        }
        self.save_draft(window, cx);
        if self.conversations.active.as_ref() != Some(&target) {
            let body = self
                .saved
                .iter()
                .find_map(|draft| match draft {
                    Draft::Reply(reply)
                        if reply.target == target
                            && !drafts::reply_was_completed(&self.attempts, reply) =>
                    {
                        Some(reply.body.clone())
                    }
                    _ => None,
                })
                .unwrap_or_default();
            self.conversations.active = Some(target);
            self.conversations
                .input
                .update(cx, |input, cx| input.set_value(body, window, cx));
        }
        self.confirm = None;
        *self.review.revealed_focus.borrow_mut() = Default::default();
        self.conversations
            .input
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
        cx.notify();
    }
    fn review_reply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_draft(window, cx);
        let Some(Draft::Reply(reply)) = self.reply_snapshot(cx) else {
            return;
        };
        if let Err(error) = github::validate_text(&reply.body, true) {
            self.error = Some(error.to_string());
            cx.notify();
            return;
        }
        if !self
            .conversations
            .threads
            .iter()
            .any(|thread| reply_matches(thread, &reply.target, self.account.as_deref()))
        {
            self.error = Some("The captured reply context is unavailable or changed. Refresh this conversation explicitly; the original draft remains available to copy.".into());
            cx.notify();
            return;
        }
        self.confirm = Some(Confirmation {
            account: reply.target.account.clone(),
            action: Action::Reply {
                target: reply.target,
                body: reply.body,
            },
        });
        self.focus_confirmation(window, cx);
    }
    fn review_resolution(
        &mut self,
        target: CapturedThread,
        resolved: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.save_draft(window, cx);
        let valid = self.account.as_deref() == Some(target.account.as_str())
            && self.conversations.threads.iter().any(|thread| {
                thread.target == target
                    && if resolved {
                        !thread.is_resolved && thread.viewer_can_resolve
                    } else {
                        thread.is_resolved && thread.viewer_can_unresolve
                    }
            });
        if !valid || self.pending {
            self.error = Some("GitHub has not granted this account permission to change this conversation in the captured context. Refresh conversations to check again.".into());
            cx.notify();
            return;
        }
        self.confirm = Some(Confirmation {
            account: target.account.clone(),
            action: Action::SetThreadResolved { target, resolved },
        });
        self.focus_confirmation(window, cx);
    }
    pub(super) fn focus_confirmation(&self, window: &mut Window, cx: &mut Context<Self>) {
        *self.review.revealed_focus.borrow_mut() = Default::default();
        self.conversations.confirm_focus.focus(window, cx);
        cx.notify();
    }
    pub(super) fn finish_conversation_action(
        &mut self,
        action: &Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            Action::Reply { target, body } => {
                // The same coalesced saver replaces any queued pre-send snapshot.
                // Exact text/target matching preserves edits made after capture.
                let matching = matches!(self.reply_snapshot(cx), Some(Draft::Reply(reply)) if reply.target == *target && reply.body == *body);
                if matching {
                    self.conversations.timer = None;
                    self.conversations.deferred.clear();
                    self.conversations.active = None;
                    self.conversations
                        .input
                        .update(cx, |input, cx| input.set_value("", window, cx));
                }
                let newer = matches!(self.reply_snapshot(cx),Some(Draft::Reply(reply)) if reply.target == *target && reply.body != *body && !reply.body.is_empty()) || self.saved.iter().any(|draft| matches!(draft,Draft::Reply(reply) if reply.target == *target && reply.body != *body && !reply.body.is_empty()));
                if matching || !newer {
                    self.persist_draft(
                        Draft::Reply(ReplyDraft {
                            target: target.clone(),
                            body: String::new(),
                        }),
                        window,
                        cx,
                    );
                }
                self.notice = Some(
                    "Reply sent. Refresh this conversation to read GitHub’s latest comments."
                        .into(),
                );
                self.focus_visible_section(window, cx);
            }
            Action::SetThreadResolved { target, resolved } => {
                if let Some(thread) = self
                    .conversations
                    .threads
                    .iter_mut()
                    .find(|thread| thread.target == *target)
                {
                    thread.is_resolved = *resolved;
                    // Permissions must be obtained again from GitHub before another mutation.
                    thread.viewer_can_resolve = false;
                    thread.viewer_can_unresolve = false;
                }
                self.notice = Some(format!(
                    "Conversation {}. Refresh conversations to update available actions.",
                    if *resolved { "resolved" } else { "reopened" }
                ));
                self.review.section_focus.focus(window, cx);
            }
            _ => {}
        }
    }
    pub(super) fn render_reply_composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(target) = self.conversations.active.as_ref() else {
            return div().into_any_element();
        };
        let p = palette(cx);
        let valid = self
            .conversations
            .threads
            .iter()
            .any(|thread| reply_matches(thread, target, self.account.as_deref()));
        div().debug_selector(||"github-reply-composer".into()).on_children_prepainted(self.reveal_on_focus(self.conversations.input.read(cx).focus_handle(cx)))
            .min_w_0().p_3().rounded(px(7.)).border_1().border_color(rgb(p.border)).bg(rgb(p.subtle)).flex().flex_col().gap_2()
            .child(label("github-reply-heading", format!("Reply to conversation · {}", target.path)).font_weight(FontWeight::SEMIBOLD))
            .child(label("github-reply-context", format!("{} · #{} · {} · captured head {}",target.account,target.pull.number,target.pull.repository.label(),target.pull.head.sha)).text_color(rgb(p.muted)))
            .when(!valid,|element|element.child(label("github-reply-unavailable","Original context is unavailable on this page. Refresh or return to its conversation before sending. Your exact draft remains local.").text_color(rgb(p.warning))))
            .child(Textarea::new(&self.conversations.input).h(appearance::ui_size(100.)).aria_label("Reply to captured GitHub conversation, saved locally").disabled(self.pending))
            .child(div().flex().flex_wrap().items_center().gap_2()
                .child(button("github-review-reply","Review reply…","",true).disabled(self.pending||!valid).on_click(cx.listener(|this,_,window,cx|this.review_reply(window,cx))))
                .child(button("github-close-reply","Save and close reply","",false).disabled(self.pending).on_click(cx.listener(|this,_,window,cx|{this.save_reply(window,cx);this.conversations.active=None;this.confirm=None;this.focus_visible_section(window,cx);cx.notify();})))
                .child(button("github-copy-reply","Copy reply","",false).on_click(cx.listener(|this,_,_,cx|{if let Some(draft)=this.reply_snapshot(cx){cx.write_to_clipboard(ClipboardItem::new_string(draft.recovery_text()));}})))
                .child(label("github-reply-save",self.save_status.clone()).role(Role::Status).text_color(rgb(p.muted))))
            .into_any_element()
    }
    pub(super) fn render_threads(&self, path: Option<&str>, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let visible = self
            .conversations
            .threads
            .iter()
            .filter(|thread| path.is_none_or(|path| thread.target.path == path))
            .collect::<Vec<_>>();
        let unresolved = visible.iter().filter(|thread| !thread.is_resolved).count();
        let previous = self
            .conversations
            .cursors
            .len()
            .checked_sub(2)
            .map(|index| self.conversations.cursors[index].clone());
        let next = self.conversations.next_cursor.clone();
        div().id("github-thread-section").min_w_0().role(Role::Group).aria_label("GitHub review conversations with authoritative resolution status")
            .tab_stop(true).track_focus(&self.review.section_focus).border_1().border_color(rgb(p.border)).rounded(px(7.)).focus_visible(|style|style.border_color(rgb(p.accent))).flex().flex_col().gap_2().p_3()
            .child(div().flex().flex_wrap().items_center().gap_2()
                .child(label("github-conversations-heading",format!("Conversations · {} unresolved on this page",unresolved)).font_weight(FontWeight::SEMIBOLD))
                .child(div().flex_1())
                .child(button("github-threads-refresh","Refresh conversations","refresh-cw",false).disabled(self.pending).on_click(cx.listener(|this,_,window,cx|this.thread_page(None,window,cx)))))
            .child(div().id("github-threads").min_w_0().max_h(appearance::ui_size(360.)).overflow_y_scroll().flex().flex_col().gap_3()
                .when(visible.is_empty(),|element|element.child(label("github-no-threads",if self.conversations.loaded{"No conversations for this file on the loaded page."}else{"Conversations are unavailable. Refresh conversations to load their current status."}).text_color(rgb(p.muted))))
                .children(visible.into_iter().enumerate().map(|(index,thread)|{
                    let target=thread.target.clone(); let reply=target.clone(); let resolve=target.clone(); let refresh=target.clone(); let more=target.clone();
                    let resolved=thread.is_resolved; let stale=self.conversations.invalid_threads.contains(&target.thread_id); let cursor=thread.next_comments_cursor.clone();
                    let opening_missing=target.root_comment_id.as_ref().is_some_and(|root|!thread.comments.iter().any(|comment|&comment.id==root));
                    let can_reply=reply_matches(thread,&target,self.account.as_deref());
                    let can_resolve=if resolved{thread.viewer_can_unresolve}else{thread.viewer_can_resolve};
                    div().id(("github-conversation",index)).min_w_0().p_3().bg(rgb(p.panel)).border_1().border_color(rgb(p.border)).rounded(px(6.)).flex().flex_col().gap_2()
                        .child(div().flex().flex_wrap().items_center().gap_2()
                            .child(label(("github-thread-path",index),thread.location()).font_weight(FontWeight::SEMIBOLD))
                            .child(div().flex_1())
                            .child(label(("github-thread-status",index),if stale{"Status needs refresh"}else if resolved{"Resolved"}else{"Unresolved"}).px_2().py_1().rounded(px(4.)).bg(rgb(p.subtle)).text_color(rgb(if resolved{p.muted}else{p.accent}))))
                        .when(stale,|element|element.child(label(("github-thread-stale",index),"The last refresh could not validate this conversation. Comments and drafts are retained; refresh successfully before replying or changing its status.").text_color(rgb(p.warning))))
                        .when(thread.is_outdated,|element|element.child(label(("github-thread-outdated",index),format!("Outdated context · original commit {}",target.original_commit_id.as_deref().unwrap_or("unavailable"))).text_color(rgb(p.muted))))
                        .when(opening_missing && !thread.context_incomplete,|element|element.child(label(("github-thread-earlier",index),"Earlier comments are on a previous page. Opening comments returns to the start of this exact conversation.").text_color(rgb(p.muted))))
                        .when(thread.context_incomplete,|element|element.child(label(("github-thread-incomplete",index),"Opening or earlier context is unavailable on this page. Replies keep their original conversation identity.").text_color(rgb(p.warning))))
                        .children(thread.comments.iter().enumerate().map(|(comment_index,comment)|{
                            let is_reply=comment.reply_to.is_some() || comment_index>0;
                            div().min_w_0().when(is_reply,|element|element.ml_3().pl_3().border_l_1().border_color(rgb(p.border))).flex().flex_col().gap_1().py_1()
                                .child(div().flex().flex_wrap().items_center().gap_2()
                                    .child(label(("github-conversation-author",index * 1000 + comment_index),comment.author.clone()).font_weight(FontWeight::SEMIBOLD))
                                    .child(label(("github-conversation-time",index * 1000 + comment_index),format!("{}{}",comment.created_at.replace('T'," ").trim_end_matches('Z'),if is_reply{" · reply"}else{""})).text_color(rgb(p.muted))))
                                .child(label(("github-conversation-body",index * 1000 + comment_index),comment.body.clone()).min_w_0())
                        }))
                        .child(div().flex().flex_wrap().items_center().gap_2().pt_1()
                            .child(button(("github-thread-reply",index),"Reply","",false).disabled(self.pending||!can_reply).on_click(cx.listener(move|this,_,window,cx|this.begin_reply(reply.clone(),window,cx))))
                            .child(button(("github-thread-resolution",index),if resolved{"Reopen"}else{"Resolve"},"",false).disabled(self.pending||!can_resolve).on_click(cx.listener(move|this,_,window,cx|this.review_resolution(resolve.clone(),!resolved,window,cx))))
                            .child(button(("github-thread-reload",index),if opening_missing{"Opening comments"}else{"Refresh thread"},"refresh-cw",false).disabled(self.pending).on_click(cx.listener(move|this,_,window,cx|this.thread_comments(refresh.clone(),None,window,cx))))
                            .when(cursor.is_some(),|element|element.child(button(("github-thread-more",index),"Next comment page","",false).disabled(self.pending).on_click(cx.listener(move|this,_,window,cx|this.thread_comments(more.clone(),cursor.clone(),window,cx)))))
                            .child(label(("github-thread-count",index),format!("{} of {} comments",thread.comments.len(),thread.comments_total)).text_color(rgb(p.muted))))
                        .when(!stale && (!can_reply || !can_resolve),|element|element.child(label(("github-thread-permissions",index),"Unavailable actions need complete context and GitHub permission; refresh to check again.").text_color(rgb(p.muted))))
                })))
            .when_some(self.conversations.pagination_notice.as_ref(),|element,notice|element.child(label("github-thread-pagination-limit",notice.clone()).text_color(rgb(p.warning))))
            .child(div().flex().flex_wrap().items_center().gap_2()
                .child(button("github-threads-previous","Previous conversations","",false).disabled(self.pending||previous.is_none()).on_click(cx.listener(move|this,_,window,cx|{if let Some(cursor)=previous.clone(){this.thread_page(cursor,window,cx);}})))
                .child(label("github-threads-page",format!("Page {} · {} total conversations",self.conversations.cursors.len(),self.conversations.total)).text_color(rgb(p.muted)))
                .child(button("github-threads-next","Next conversations","",false).disabled(self.pending||next.is_none()).on_click(cx.listener(move|this,_,window,cx|this.thread_page(next.clone(),window,cx)))))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::component::Root;
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn conversation_paging_keeps_forward_navigation_and_reports_real_bounds() {
        let mut history = vec![None];
        assert_eq!(
            accept_page_cursor(&mut history, None, Some("second".into())),
            Some("second".into())
        );
        assert_eq!(
            accept_page_cursor(&mut history, Some("second".into()), Some("third".into())),
            Some("third".into())
        );
        assert_eq!(
            accept_page_cursor(&mut history, None, Some("second".into())),
            Some("second".into()),
            "Previous must retain the legitimate next page"
        );
        assert_eq!(
            accept_page_cursor(&mut history, Some("second".into()), Some("second".into())),
            None,
            "a repeated current cursor is incomplete, not exhausted"
        );
        let mut history = (0..999)
            .map(|n| Some(format!("page-{n}")))
            .collect::<Vec<_>>();
        assert_eq!(
            accept_page_cursor(
                &mut history,
                Some("page-999".into()),
                Some("page-1000".into())
            ),
            None
        );
        assert_eq!(history.len(), 1000);
    }

    #[gpui::test]
    async fn reply_focus_navigation_recovery_and_exact_completion(cx: &mut TestAppContext) {
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
        app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.executor().run_until_parked();
        let pull = github::review::fixture_pull(false);
        let identity = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let target = page.items[0].target.clone();
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.destination.update(cx, |input, cx| {
                    input.set_value("gitturtle-fixture/native-review", window, cx)
                });
                panel.account = Some("offline-reviewer".into());
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.receive_threads(page, None);
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        cx.executor().run_until_parked();
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.begin_reply(target.clone(), window, cx)
            })
        });
        cx.simulate_input("Immediate reply café");
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                assert!(
                    panel
                        .conversations
                        .input
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window)
                );
                assert_eq!(
                    panel.conversations.input.read(cx).value(),
                    "Immediate reply café"
                );
                panel.conversations.input.update(cx, |input, cx| {
                    input.set_value(" Exact reply\r\n  café\n", window, cx);
                    cx.emit(InputEvent::Change);
                });
                panel.body.update(cx, |input, cx| {
                    input.set_value("Independent summary", window, cx);
                    cx.emit(InputEvent::Change);
                });
                panel.review.composing = Some(github::LineComment {
                    path: target.path.clone(),
                    body: "Inline still here".into(),
                    line: 2,
                    side: github::DiffSide::Right,
                    start_line: None,
                    start_side: None,
                });
                panel.review.input.update(cx, |input, cx| {
                    input.set_value("Inline still here", window, cx)
                });
            })
        });
        // All three prose composers participate in the native focus chain.
        let editors = cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.review.section = Section::Review;
                cx.notify();
                panel.focus_visible_section(window, cx);
                [
                    panel.conversations.input.clone(),
                    panel.review.input.clone(),
                    panel.body.clone(),
                ]
            })
        });
        for editor in editors {
            let before = cx.update(|window, cx| {
                editor.read(cx).focus_handle(cx).focus(window, cx);
                editor.read(cx).value().to_string()
            });
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.simulate_keystrokes("tab");
            cx.update(|window, cx| {
                assert!(
                    !editor.read(cx).focus_handle(cx).is_focused(window),
                    "Tab must leave a prose composer for its next control"
                );
                assert_eq!(
                    editor.read(cx).value(),
                    before,
                    "Tab must not insert whitespace into prose"
                );
            });
            cx.simulate_keystrokes("shift-tab");
            cx.update(|window, cx| {
                assert!(
                    editor.read(cx).focus_handle(cx).is_focused(window),
                    "Shift+Tab returns from the adjacent control to its composer"
                );
                assert_eq!(editor.read(cx).value(), before);
            });
        }
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.review.section = Section::Overview;
                panel.focus_visible_section(window, cx);
                cx.notify();
            })
        });
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }
        let composer = cx.debug_bounds("github-reply-composer").unwrap();
        let viewport = cx.read(|cx| panel.read(cx).review.body_scroll.bounds());
        assert!(
            composer.size.height > px(100.)
                && composer.top() >= viewport.top()
                && composer.bottom() <= viewport.bottom(),
            "reply composer {composer:?} should fit inside {viewport:?}"
        );
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.save_draft(window, cx);
                panel.review.section = Section::Files;
                panel.review.mode = 2;
                panel.close(window, cx);
            })
        });
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
        cx.executor().run_until_parked();
        cx.update(|window,cx|panel.update(cx,|panel,cx| {
            assert_eq!(panel.conversations.active.as_ref(),Some(&target));
            assert_eq!(panel.conversations.input.read(cx).value()," Exact reply\r\n  café\n");
            assert_eq!(panel.body.read(cx).value(),"Independent summary");
            assert!(panel.conversations.input.read(cx).focus_handle(cx).is_focused(window));
            panel.review_reply(window,cx);
            assert!(matches!(panel.confirm.as_ref().map(|c|&c.action),Some(Action::Reply { target:captured,.. }) if captured==&target));
            assert!(panel.conversations.confirm_focus.is_focused(window));
        }));
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }
        let confirmation = cx.debug_bounds("github-confirmation").unwrap();
        let viewport = cx.read(|cx| panel.read(cx).review.body_scroll.bounds());
        assert!(
            confirmation.bottom() <= viewport.bottom(),
            "confirmation must reveal its send action"
        );
        // Exercise actual asynchronous refresh failures with a moved provider head.
        // A failed single-thread read invalidates only its exact context; a failed
        // page read invalidates every retained thread whose snapshot it checked.
        for whole_page in [false, true] {
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.review.fixture = true;
                    panel.review.fixture_moved = true;
                    if whole_page {
                        panel.thread_page(None, window, cx);
                    } else {
                        panel.thread_comments(target.clone(), None, window, cx);
                    }
                })
            });
            app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
                .await
                .unwrap()
                .unwrap();
            cx.executor().run_until_parked();
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    assert!(
                        panel
                            .error
                            .as_ref()
                            .is_some_and(|error| error.contains("could not be loaded"))
                    );
                    assert_eq!(panel.conversations.active.as_ref(), Some(&target));
                    assert_eq!(
                        panel.conversations.input.read(cx).value(),
                        " Exact reply\r\n  café\n"
                    );
                    let thread = &panel.conversations.threads[0];
                    assert!(
                        !thread.reply_available()
                            && !thread.viewer_can_resolve
                            && !thread.viewer_can_unresolve,
                        "all rendered action buttons use the invalidated availability"
                    );
                    assert!(
                        !thread.comments.is_empty(),
                        "failed refresh retains visible conversation text"
                    );
                    assert_eq!(
                        panel.conversations.invalid_threads.len(),
                        if whole_page {
                            panel.conversations.threads.len()
                        } else {
                            1
                        }
                    );
                    if !whole_page {
                        assert!(
                            panel.conversations.threads[1].viewer_can_reply,
                            "unrelated conversation keeps its validated permissions"
                        );
                    }
                    panel.review_reply(window, cx);
                    assert!(
                        panel.confirm.is_none(),
                        "composer cannot review an unvalidated target"
                    );
                    panel.review_resolution(target.clone(), true, window, cx);
                    assert!(
                        panel.confirm.is_none(),
                        "resolution cannot use stale cached permissions"
                    );
                    panel.review.fixture_moved = false;
                    if whole_page {
                        panel.thread_page(None, window, cx);
                    } else {
                        panel.thread_comments(target.clone(), None, window, cx);
                    }
                })
            });
            app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
                .await
                .unwrap()
                .unwrap();
            cx.executor().run_until_parked();
            cx.update(|window, cx| panel.update(cx, |panel, cx| {
                assert!(panel.conversations.invalid_threads.is_empty());
                assert!(panel.conversations.threads[0].reply_available());
                assert!(panel.conversations.threads[0].viewer_can_resolve);
                assert_eq!(panel.conversations.input.read(cx).value(), " Exact reply\r\n  café\n");
                panel.review_reply(window, cx);
                assert!(matches!(panel.confirm.as_ref().map(|confirmation|&confirmation.action), Some(Action::Reply {target:captured,..}) if captured == &target));
            }));
        }
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.account = Some("another-account".into());
                panel.confirm = None;
                panel.review_reply(window, cx);
                assert!(panel.confirm.is_none());
                assert!(panel.error.is_some());
                panel.account = Some(target.account.clone());
                let original = Draft::Reply(ReplyDraft {
                    target: target.clone(),
                    body: " Exact reply\r\n  café\n".into(),
                });
                panel.conversations.active = None;
                panel.saved = vec![original.clone()];
                panel.conversations.threads[0].target.pull.head.sha = "9".repeat(40);
                panel.restore_reply(window, cx);
                assert!(
                    panel.conversations.active.is_none(),
                    "old head stays copy-only"
                );
                panel.conversations.threads[0].target = target.clone();
                panel.restore_reply(window, cx);
                assert_eq!(panel.conversations.active.as_ref(), Some(&target));
                panel.finish_conversation_action(
                    &Action::Reply {
                        target: target.clone(),
                        body: " Exact reply\r\n  café\n".into(),
                    },
                    window,
                    cx,
                );
                assert!(panel.conversations.active.is_none());
                assert!(
                    !panel
                        .saved
                        .iter()
                        .any(|draft| draft.key() == original.key()),
                    "successful reply cleanup removes its recovery row and count immediately"
                );
                panel.restore_reply(window, cx);
                assert!(
                    panel.conversations.active.is_none(),
                    "confirmed sent text cannot silently restore"
                );
                panel.close(window, cx);
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
        assert!(
            !saved
                .iter()
                .any(|d| matches!(d,Draft::Reply(reply) if reply.target==target)),
            "successful exact reply deletion is durable"
        );
        app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.executor().run_until_parked();
    }
}
