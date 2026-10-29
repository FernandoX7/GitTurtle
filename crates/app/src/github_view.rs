//! Native GitHub collaboration. Repository/PR actions capture their destination;
//! network work exists only behind an explicit button or submit confirmation.
use crate::github::{
    self, Action, Client, ComparisonTarget, NewPull, PullRequest, PullStatus, Repository,
    ReviewDraft, ReviewEvent,
    drafts::{self, Draft},
    transport::{CredentialStore, GhTransport, SecureStore},
};
use crate::*;
use futures::FutureExt;
use gitturtle_core::{HistoryCancellation, OperationControl};
use gpui_kit::{
    component::{WindowExt, dialog::DialogFooter},
    prelude::FluentBuilder,
};
use std::time::Duration;
mod conversations;
mod review;
use crate::github::review::UiTransport;
use review::Section;

/// [`native_accessibility::text`] at the panel's 12 pt.
fn label(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
) -> native_accessibility::StaticText<Stateful<Div>> {
    native_accessibility::text(id, text).text_size(crate::appearance::ui_text(12.))
}
/// A [`label`] of " · "-separated phrases that keeps each phrase whole: a line
/// breaks after a separator, between two phrases, and inside a phrase only
/// where that phrase alone is wider than the line. Each phrase draws as
/// `{id}-{index}`, the separator ending the phrase before it; on one line
/// they read as the text does, and the accessible name is the whole text.
fn phrases(
    id: &'static str,
    text: impl Into<SharedString>,
) -> native_accessibility::StaticText<Stateful<Div>> {
    let text = text.into();
    let parts: Vec<SharedString> = {
        let mut parts: Vec<&str> = text.split(" · ").collect();
        let last = parts.pop().unwrap_or_default().to_owned();
        parts
            .into_iter()
            .map(|phrase| format!("{phrase} · ").into())
            .chain([last.into()])
            .collect()
    };
    native_accessibility::static_text(div().id(id).debug_selector(move || id.into()), text)
        .flex()
        .flex_wrap()
        .text_size(crate::appearance::ui_text(12.))
        .children(parts.into_iter().enumerate().map(move |(index, phrase)| {
            div()
                .debug_selector(move || format!("{id}-{index}"))
                .child(phrase)
        }))
}
/// Where the panel's text starts, in from its edge: a compact Button's label
/// inset, [`crate::LABEL_INSET`] at the interface size. The dialog's title, the
/// fixture banner, the notices, the PR rows and the labels between them start
/// here, where the Buttons at the edge (the pager, Recover drafts, the tabs)
/// start their labels. They once started at four insets: the title at the
/// edge, the banner 6.5 px in, the Buttons' labels 10 px and the PR rows'
/// text 20.75 px, inside their cards' border and padding.
fn text_inset() -> Pixels {
    appearance::ui_size(crate::LABEL_INSET)
}
/// The tallest the panel's scrolling body grows in `window`, its padding included: the
/// window less the dialog's chrome around it, scaled with the interface text.
fn body_max_height(window: &Window) -> Pixels {
    (window.viewport_size().height
        - window.rem_size() * (220. / f32::from(appearance::DEFAULT_INTERFACE_TEXT_SIZE)))
    .clamp(px(120.), px(660.))
}
/// One line of a PR row: left-aligned in its column and cut with an ellipsis
/// where the row is too narrow, the whole text in the row's tooltip.
fn row_text(selector: String, text: String) -> Div {
    div()
        .debug_selector(move || selector.clone())
        .flex_1()
        .min_w_0()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(text)
}
/// Preserve native focused-button Enter/Space activation inside this long-lived
/// dialog; its default Return-to-confirm action must not bypass Panel::close.
fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    symbol: &str,
    active: bool,
) -> Button {
    crate::button(id, label, symbol, active).key_context("GitTurtleGithubButton")
}
/// [`copy_feedback::copy_button`] with the same activation as [`button`].
fn copy_button(
    mark: copy_feedback::Mark,
    label: impl Into<SharedString>,
    tooltip: impl Into<SharedString>,
    cx: &App,
) -> Button {
    copy_feedback::copy_button(mark, label, tooltip, cx).key_context("GitTurtleGithubButton")
}

#[derive(Default)]
struct WarmPanels(Vec<WarmPanel>);
impl gpui_kit::Global for WarmPanels {}
struct WarmPanel {
    owner: gpui_kit::EntityId,
    worktree: std::path::PathBuf,
    panel: Entity<Panel>,
    bytes: usize,
}
impl WarmPanels {
    fn retain(&mut self, entry: WarmPanel) {
        self.0
            .retain(|old| old.owner != entry.owner || old.worktree != entry.worktree);
        self.0.push(entry);
        while self.0.len() > 8
            || self.0.iter().map(|entry| entry.bytes).sum::<usize>() > 32 * 1024 * 1024
        {
            self.0.remove(0);
        }
    }
}
impl GitTurtle {
    /// Open the panel from the header's Pull requests button, full or
    /// compact, which closing returns focus to.
    pub(super) fn open_github_from_header(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let opener = self.pull_requests_focus.clone();
        self.open_github_returning_to(Some(opener), window, cx);
    }
    /// Open the panel from the command palette, which has already returned
    /// focus to what it opened over, or from anywhere else: closing returns
    /// focus to what is focused now.
    pub(super) fn open_github(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let previous = window.focused(cx);
        self.open_github_returning_to(previous, window, cx);
    }
    fn open_github_returning_to(
        &mut self,
        opener: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() || self.page != AppPage::Repository {
            return;
        }
        let Some(repo) = self.repository.clone() else {
            return;
        };
        // Closing returns focus here, and so does the repair a frame after
        // the dialog closes. The window's root shows no focus, so with
        // nothing else focused the mode's own list takes it instead.
        self.modal_return_focus = opener.filter(|focus| *focus != self.app_focus);
        review::init(cx);
        let owner = cx.entity().downgrade();
        let branch = self.remote_branch.read(cx).value().to_string();
        let owner_id = cx.entity_id();
        let worktree = repo.path().to_owned();
        let warm = cx.default_global::<WarmPanels>();
        let cached = warm
            .0
            .iter()
            .position(|entry| entry.owner == owner_id && entry.worktree == worktree)
            .map(|index| warm.0.remove(index).panel);
        let form = if let Some(form) = cached {
            form.update(cx, |panel, cx| {
                panel.closed = false;
                panel.confirm = None;
                cx.notify();
            });
            form
        } else {
            cx.new(|cx| Panel::new(owner, repo, branch, window, cx))
        };
        let focus_form = form.clone();
        window.defer(cx, move |window, cx| {
            focus_form.update(cx, |panel, cx| {
                panel.load_local(window, cx);
                // Closing cancelled a pending preparation; resume it in whichever mode the
                // file was left, keeping its scroll and Source editor.
                if let Some(index) = panel.review.take_suspended() {
                    panel.prepare_review_file(index, window, cx);
                }
                panel.focus_visible_section(window, cx);
            });
        });
        window.open_alert_dialog(cx, move |dialog, window, _| {
            let close = form.clone();
            let cancel = form.clone();
            dialog
                // In the dialog title's own style, at the panel's text inset.
                .title(
                    native_accessibility::dialog_title("github-title", "GitHub pull requests")
                        .ml(text_inset()),
                )
                .on_ok(|_, _, _| false)
                .width(appearance::dialog_width(px(1100.), window))
                // No `appearance::dialog_max_height`: the panel bounds its
                // own scrolling body from the window (`body_max_height`),
                // which reveals the control Tab moves onto; the dialog's cap
                // would clip that body and scroll it a second time.
                .child(form.clone())
                .footer(DialogFooter::new().child(
                    button("github-close", "Back to repository", "", false).on_click(
                        move |_, window, cx| close.update(cx, |this, cx| this.close(window, cx)),
                    ),
                ))
                .on_cancel(move |_, window, cx| {
                    cancel.update(cx, |this, cx| this.close(window, cx));
                    false
                })
        });
    }
}
struct Confirmation {
    action: Action,
    account: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DraftIssue {
    Destination,
    Selection,
}
impl DraftIssue {
    fn message(self) -> &'static str {
        match self {
            Self::Destination => {
                "Enter the destination as owner/repository or a GitHub.com remote URL."
            }
            Self::Selection => "Choose a pull request or select New pull request first.",
        }
    }
    fn save_message(self) -> &'static str {
        match self {
            Self::Destination => {
                "Draft not saved: enter a valid owner/repository destination. Keep this window open or copy your text."
            }
            Self::Selection => {
                "Draft not saved: choose a pull request or select New pull request. Keep this window open or copy your text."
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct DraftFields {
    title: String,
    body: String,
    head: String,
    base: String,
}
fn capture_draft(
    destination: &str,
    creating: bool,
    draft: bool,
    event: ReviewEvent,
    pull: Option<&PullRequest>,
    fields: DraftFields,
) -> Result<Draft, DraftIssue> {
    if !creating && pull.is_none() {
        return Err(DraftIssue::Selection);
    }
    let repository = Repository::parse(destination).map_err(|_| DraftIssue::Destination)?;
    if creating {
        Ok(Draft::Pull(NewPull {
            repository,
            title: fields.title,
            body: fields.body,
            head: fields.head,
            base: fields.base,
            draft,
            expected_head: None,
            expected_base: None,
        }))
    } else {
        Ok(Draft::Review(ReviewDraft {
            pull: pull.expect("selection checked above").capture(repository),
            body: fields.body,
            event,
            comments: vec![],
            composing: None,
            discussion: false,
        }))
    }
}
fn empty_pull_draft(draft: &Draft) -> bool {
    matches!(draft, Draft::Pull(pull) if pull.title.is_empty() && pull.body.is_empty())
}
/// One unpublished form snapshot, regardless of how often typing changes its
/// destination key. Only a quiet form or an explicit transition reaches the
/// durable saver; previously accepted destination drafts remain independent.
#[derive(Default)]
struct DeferredDraft {
    generation: u64,
    draft: Option<Draft>,
}
impl DeferredDraft {
    fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.draft = None;
    }
    fn replace(&mut self, draft: Draft) -> u64 {
        self.clear();
        self.draft = Some(draft);
        self.generation
    }
    fn take(&mut self, generation: u64) -> Option<Draft> {
        if self.generation == generation {
            self.draft.take()
        } else {
            None
        }
    }
    fn is_pending(&self) -> bool {
        self.draft.is_some()
    }
}
const DRAFT_QUIET_PERIOD: Duration = Duration::from_millis(500);
/// The GitHub CLI sign-in command that Copy sign-in command copies.
const SIGN_IN_COMMAND: &str = "gh auth login --hostname github.com --web --skip-ssh-key";
type DraftSaveReply = futures::channel::oneshot::Receiver<anyhow::Result<()>>;
type DraftSaveCompletion = futures::future::Shared<futures::future::BoxFuture<'static, bool>>;

fn track_save_completion(
    latest: &mut Option<DraftSaveCompletion>,
    response: Option<DraftSaveReply>,
) -> Option<DraftSaveCompletion> {
    response.map(|response| {
        let completion = response
            .map(|result| matches!(result, Ok(Ok(()))))
            .boxed()
            .shared();
        *latest = Some(completion.clone());
        completion
    })
}

fn retain_window_close_completion(response: DraftSaveCompletion, cx: &mut App) {
    let mut response = Some(response);
    // The panel and its subscriptions disappear before macOS's asynchronous
    // terminate callback. Keep this accepted completion at app lifetime instead.
    cx.on_app_quit(move |_| {
        let response = response.take();
        async move {
            if let Some(response) = response {
                let _ = response.await;
            }
        }
    })
    .detach();
}

fn apply_save_completion(
    status: &mut String,
    response_generation: u64,
    latest_generation: u64,
    current: Result<Draft, DraftIssue>,
    pending: bool,
    succeeded: bool,
) {
    if response_generation == latest_generation {
        *status = finished_save_status(current, pending, succeeded).into();
    }
}

fn finished_save_status(
    current: Result<Draft, DraftIssue>,
    pending: bool,
    succeeded: bool,
) -> &'static str {
    match current {
        Err(issue) => issue.save_message(),
        Ok(draft) if empty_pull_draft(&draft) => "",
        Ok(_) if !succeeded => "Draft save failed — keep this window open or copy the text",
        Ok(_) if pending => "Saving local draft…",
        Ok(_) => "Draft saved locally",
    }
}
struct Panel {
    review: review::ReviewState,
    conversations: conversations::State,
    owner: WeakEntity<GitTurtle>,
    repo: GitRepository,
    destination: Entity<InputState>,
    head: Entity<InputState>,
    base: Entity<InputState>,
    title: Entity<InputState>,
    body: Entity<TextareaState>,
    rows: Vec<PullRequest>,
    page: u32,
    has_next: bool,
    pull: Option<PullRequest>,
    status: Option<PullStatus>,
    description: Option<Entity<EditorState>>,
    create: bool,
    draft: bool,
    event: ReviewEvent,
    discussion: bool,
    account: Option<String>,
    notice: Option<String>,
    error: Option<String>,
    pending: bool,
    /// The busy row's Cancel, which hands focus to the shown section as the
    /// row goes.
    cancel_request: focus_ring::LoadingCancel,
    request_generation: u64,
    local_loaded: bool,
    local_loading: bool,
    local_generation: u64,
    local_task: Option<Task<()>>,
    writing: bool,
    closed: bool,
    confirm: Option<Confirmation>,
    control: Option<OperationControl>,
    task: Option<Task<()>>,
    saved: Vec<Draft>,
    save_status: String,
    draft_issue: Option<DraftIssue>,
    saver: commit_drafts::CoalescingSaver<String, Draft>,
    deferred_draft: DeferredDraft,
    draft_timer: Option<Task<()>>,
    save_response_generation: u64,
    save_completion: Option<DraftSaveCompletion>,
    subscriptions: Vec<Subscription>,
    templates: Vec<github::Template>,
    attempts: Vec<drafts::Attempt>,
    local_control: Option<HistoryCancellation>,
}
impl Panel {
    fn new(
        owner: WeakEntity<GitTurtle>,
        repo: GitRepository,
        branch: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this=Self {
            review:review::ReviewState::new(window,cx),
            conversations:conversations::State::new(window,cx),
            owner,repo,destination:cx.new(|cx|InputState::new(window,cx).placeholder("owner/repository")),
            head:cx.new(|cx|InputState::new(window,cx).default_value(branch).placeholder("Published head branch (owner:branch for a fork)")),
            base:cx.new(|cx|InputState::new(window,cx).default_value("main")),title:cx.new(|cx|InputState::new(window,cx).placeholder("Pull request title")),
            body:cx.new(|cx|TextareaState::new(window,cx).auto_grow(5,5).placeholder("Description or review text — saved locally")),
            rows:vec![],page:1,has_next:false,pull:None,status:None,description:None,create:false,draft:true,event:ReviewEvent::Comment,discussion:true,
            account:None,notice:Some("Offline until you choose Connect or Refresh. GitHub.com is supported; Git authentication and Push keep their existing behavior.".into()),error:None,pending:false,cancel_request:focus_ring::LoadingCancel::new(cx),request_generation:0,local_loaded:false,local_loading:false,local_generation:0,local_task:None,writing:false,closed:false,
            confirm:None,control:None,task:None,saved:vec![],save_status:String::new(),draft_issue:None,saver:Default::default(),deferred_draft:Default::default(),draft_timer:None,save_response_generation:0,save_completion:None,subscriptions:vec![],templates:vec![],attempts:vec![],local_control:None,
        };
        if this.review.fixture {
            this.destination.update(cx, |input, cx| {
                input.set_value("gitturtle-fixture/native-review", window, cx)
            });
            this.account = Some("offline-reviewer".into());
            this.notice=Some("Offline review fixture · fixed sample data; account connection and network transport are unavailable in this mode.".into());
        }
        this.subscriptions.push(cx.subscribe_in(
            &this.review.input,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.confirm = None;
                    this.defer_draft_save(window, cx);
                }
            },
        ));
        this.subscriptions.push(cx.subscribe_in(
            &this.conversations.input,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.confirm = None;
                    this.defer_reply_save(window, cx);
                }
            },
        ));
        for input in [this.head.clone(), this.base.clone(), this.title.clone()] {
            this.subscriptions.push(cx.subscribe_in(
                &input,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.confirm = None;
                        this.defer_draft_save(window, cx);
                    }
                },
            ));
        }
        this.subscriptions.push(cx.subscribe_in(
            &this.body,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.confirm = None;
                    this.defer_draft_save(window, cx);
                }
            },
        ));
        this.subscriptions.push(cx.subscribe_in(
            &this.destination,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    // Destination edits arrive after the field changed. Flush the
                    // old captured review snapshot before clearing its identity.
                    if let Some(draft @ Draft::Review(_)) = this.deferred_draft.draft.take() {
                        this.draft_timer = None;
                        this.persist_draft(draft, window, cx);
                    }
                    this.save_reply(window, cx);
                    this.conversations.reset_content();
                    this.review.reset_content();
                    this.review.comments.clear();
                    this.review.composing = None;
                    this.review.show_list = true;
                    this.rows.clear();
                    this.pull = None;
                    this.status = None;
                    this.description = None;
                    this.confirm = None;
                    this.page = 1;
                    this.has_next = false;
                    this.defer_draft_save(window, cx);
                    cx.notify();
                }
            },
        ));
        let panel = cx.weak_entity();
        let panel_window = window.window_handle().window_id();
        this.subscriptions.push(cx.on_window_closed(move |cx, id| {
            if id == panel_window {
                // GPUI delivers this before releasing the window's entities.
                if let Ok(Some(response)) =
                    panel.update(cx, |this, cx| this.flush_draft_for_shutdown(cx))
                {
                    retain_window_close_completion(response, cx);
                }
            }
        }));
        this.subscriptions.push(cx.on_app_quit(|this, cx| {
            let response = this.flush_draft_for_shutdown(cx);
            async move {
                if let Some(response) = response {
                    let _ = response.await;
                }
            }
        }));
        // The parent opens this entity while it is already being updated.
        // Wait until GPUI returns that parent to the app before borrowing its
        // serial executor for passive local metadata and draft reads.
        cx.defer_in(window, |this, window, cx| {
            if !this.closed {
                this.load_local(window, cx);
            }
        });
        this
    }
    fn load_local(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed || self.local_loaded || self.local_loading {
            return;
        }
        self.local_loading = true;
        self.local_generation = self.local_generation.wrapping_add(1);
        let generation = self.local_generation;
        let repo = self.repo.clone();
        let response = self.owner.update(cx, |owner, _| {
            owner.operations.submit_read(move || {
                Ok((repo.remote_configs()?, drafts::load(), drafts::attempts()))
            })
        });
        let Ok(response) = response else {
            self.local_loading = false;
            return;
        };
        self.local_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.closed || generation != this.local_generation {
                    return;
                }
                this.local_loading = false;
                match result {
                    Ok(Ok((remotes, saved, attempts))) => {
                        this.local_loaded = true;
                        if this.destination.read(cx).value().is_empty()
                            && let Some(destination) = remotes
                                .iter()
                                .flat_map(|remote| remote.urls.iter())
                                .find_map(|url| Repository::parse(url).ok())
                        {
                            this.destination.update(cx, |input, cx| {
                                input.set_value(destination.label(), window, cx)
                            });
                        }
                        match attempts {
                            Ok(attempts) => this.attempts = attempts,
                            Err(error) => this.error = Some(format!("{error:#}")),
                        }
                        match saved {
                            Ok(mut saved) => {
                                for draft in std::mem::take(&mut this.saved) {
                                    let key = draft.key();
                                    saved.retain(|old| old.key() != key);
                                    saved.push(draft);
                                }
                                this.saved = saved;
                            }
                            Err(error) => this.error = Some(format!("{error:#}")),
                        }
                    }
                    _ => {
                        this.error =
                            Some("Could not read repository remotes and saved drafts.".into())
                    }
                }
                cx.notify();
            });
        }));
    }
    fn destination(&self, cx: &App) -> anyhow::Result<Repository> {
        Repository::parse(&self.destination.read(cx).value())
    }
    fn current_draft(&self, cx: &App) -> Result<Draft, DraftIssue> {
        let mut draft = capture_draft(
            &self.destination.read(cx).value(),
            self.create,
            self.draft,
            self.event,
            self.pull.as_ref(),
            DraftFields {
                title: self.title.read(cx).value().to_string(),
                body: self.body.read(cx).value().to_string(),
                head: self.head.read(cx).value().to_string(),
                base: self.base.read(cx).value().to_string(),
            },
        )?;
        if let Draft::Review(review) = &mut draft {
            review.discussion = self.discussion;
            review.comments = self.review.comments.clone();
            review.composing = self.review.composing.clone().map(|mut comment| {
                comment.body = self.review.input.read(cx).value().to_string();
                comment
            });
        }
        Ok(draft)
    }

    fn draft_to_save(&mut self, cx: &App) -> Option<Draft> {
        let draft = match self.current_draft(cx) {
            Ok(draft) => draft,
            Err(issue) => {
                self.save_status = issue.save_message().into();
                if self.draft_issue.is_some() {
                    self.draft_issue = Some(issue);
                }
                return None;
            }
        };
        self.draft_issue = None;
        if empty_pull_draft(&draft) {
            self.save_status.clear();
            return None;
        }
        Some(draft)
    }
    fn defer_draft_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft_timer = None;
        self.deferred_draft.clear();
        if let Some(draft) = self.draft_to_save(cx) {
            let generation = self.deferred_draft.replace(draft);
            self.save_status = "Saving local draft…".into();
            let timer = cx.background_executor().timer(DRAFT_QUIET_PERIOD);
            self.draft_timer = Some(cx.spawn_in(window, async move |this, cx| {
                timer.await;
                let _ = this.update_in(cx, |this, window, cx| {
                    let Some(draft) = this.deferred_draft.take(generation) else {
                        return;
                    };
                    this.draft_timer = None;
                    if !this.closed {
                        this.persist_draft(draft, window, cx);
                    }
                });
            }));
        }
        cx.notify();
    }
    fn save_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_reply(window, cx);
        // Explicit close/selection/review commits the current form immediately.
        // A superseded timer must never enqueue its older destination afterward.
        self.draft_timer = None;
        self.deferred_draft.clear();
        if let Some(draft) = self.draft_to_save(cx) {
            self.persist_draft(draft, window, cx);
        }
        cx.notify();
    }
    fn flush_draft_for_shutdown(&mut self, cx: &mut App) -> Option<DraftSaveCompletion> {
        self.draft_timer = None;
        self.deferred_draft.clear();
        let reply = if self.closed {
            None
        } else {
            self.reply_snapshot(cx)
        };
        self.conversations.timer = None;
        self.conversations.deferred.clear();
        let draft = if self.closed {
            None
        } else {
            self.draft_to_save(cx)
        };
        self.closed = true;
        if let Ok(response) = self.owner.update(cx, |owner, _| {
            let mut response = None;
            for draft in draft.into_iter().chain(reply) {
                let queued = self.saver.queue_with(
                    &owner.preferences_writer,
                    draft.key(),
                    draft,
                    drafts::save_batch,
                );
                if queued.is_some() {
                    response = queued;
                }
            }
            response
        }) {
            let _ = track_save_completion(&mut self.save_completion, response);
        }
        // Await the actual accepted save, including one this edit coalesced
        // into. A separate barrier could be refused by the bounded executor.
        self.save_completion.clone()
    }
    fn persist_draft(&mut self, draft: Draft, window: &mut Window, cx: &mut Context<Self>) {
        let key = draft.key();
        if matches!(&draft, Draft::Reply(reply) if reply.body.is_empty()) {
            // Empty replies are serialized deletions, not recoverable snapshots.
            // Queue the deletion below while removing it from the visible cache.
            self.saved.retain(|saved| saved.key() != key);
        } else if let Some(old) = self.saved.iter_mut().find(|d| d.key() == key) {
            *old = draft.clone();
        } else {
            self.saved.push(draft.clone());
        }
        let response = match self.owner.update(cx, |owner, _| {
            self.saver
                .queue_with(&owner.preferences_writer, key, draft, drafts::save_batch)
        }) {
            Ok(response) => response,
            Err(_) => {
                self.save_status =
                    "Draft save failed — keep this window open or copy the text".into();
                cx.notify();
                return;
            }
        };
        self.save_status = "Saving local draft…".into();
        if let Some(response) = track_save_completion(&mut self.save_completion, response) {
            self.save_response_generation = self.save_response_generation.wrapping_add(1);
            let response_generation = self.save_response_generation;
            cx.spawn_in(window, async move |this, cx| {
                let succeeded = response.await;
                let _ = this.update_in(cx, |this, _, cx| {
                    // A previous destination's reply must not claim the
                    // currently invalid or newly edited form has been saved.
                    let current = this.current_draft(cx);
                    apply_save_completion(
                        &mut this.save_status,
                        response_generation,
                        this.save_response_generation,
                        current,
                        this.deferred_draft.is_pending()
                            || this.conversations.deferred.is_pending()
                            || this.saver.is_pending(),
                        succeeded,
                    );
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }
    fn run<T: Send + 'static>(
        &mut self,
        writing: bool,
        operation: impl FnOnce(OperationControl) -> anyhow::Result<T> + Send + 'static,
        receive: impl FnOnce(anyhow::Result<T>, &mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending || self.closed {
            return;
        }
        let control = OperationControl::default();
        let worker_control = control.clone();
        let response = self.owner.update(cx, |owner, _| {
            owner.operations.submit(move || operation(worker_control))
        });
        let Ok(response) = response else {
            self.error = Some("The repository window is unavailable.".into());
            return;
        };
        self.control = Some(control);
        self.request_generation = self.request_generation.wrapping_add(1);
        let generation = self.request_generation;
        self.pending = true;
        self.writing = writing;
        self.error = None;
        self.notice = None;
        self.task=Some(cx.spawn_in(window,async move|this,cx|{
            let result=response.await.unwrap_or_else(|_|Err(anyhow::anyhow!("GitHub operation was interrupted. Its outcome may be uncertain; check the destination before submitting again.")));
            let _=this.update_in(cx,|this,window,cx|{if generation!=this.request_generation{return;}this.pending=false;this.writing=false;this.control=None;
                // The busy row goes: focus on its Cancel moves to the section
                // the panel shows, as when it opens, before the reply moves it
                // anywhere else.
                if !this.closed && this.cancel_request.holds_focus(window) {this.focus_visible_section(window,cx);}
                if !this.closed {receive(result,this,window,cx);}cx.notify();});
        }));
        cx.notify();
    }
    fn connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.review.fixture {
            self.notice=Some("Offline fixture account: offline-reviewer. No credential or network access is available.".into());
            cx.notify();
            return;
        }
        self.run(
            false,
            |control| GhTransport::connect_existing(&control),
            |result, this, _, _| match result {
                Ok(login) => {
                    this.notice = Some(format!(
                        "Connected as {login}; authorization is stored in macOS Keychain."
                    ));
                    this.account = Some(login);
                }
                Err(error) => this.error = Some(format!("{error:#}")),
            },
            window,
            cx,
        );
    }
    fn refresh(&mut self, page: u32, window: &mut Window, cx: &mut Context<Self>) {
        self.save_draft(window, cx);
        let repository = match self.destination(cx) {
            Ok(repo) => repo,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        let fixture = self.review.fixture;
        let moved = self.review.fixture_moved;
        self.run(false,move|control|{let transport=UiTransport::open(fixture,moved)?;let login=transport.login().to_owned();Ok((Client(transport).list(&repository,page,&control)?,login))},|result,this,_,_|match result {Ok((page,login))=>{this.create=false;this.review.show_list=true;this.confirm=None;this.rows=page.items;this.page=page.page;this.has_next=page.has_next;this.account=Some(login);this.notice=Some("PR list refreshed. Selecting a PR explicitly loads its current review and check status.".into());},Err(error)=>this.error=Some(format!("{error:#}"))},window,cx);
    }
    fn inspect(&mut self, pull: PullRequest, window: &mut Window, cx: &mut Context<Self>) {
        self.save_draft(window, cx);
        let repository = match self.destination(cx) {
            Ok(repo) => repo,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        let fixture = self.review.fixture;
        let moved = self.review.fixture_moved;
        self.run(false,move|control|{
            let mut client=Client(UiTransport::open(fixture,moved)?);
            let pull=client.pull(&repository,pull.number,&control)?;
            let captured=pull.capture(repository);
            let status=client.status(&captured,&control);
            let files=client.files(&captured,1,&control);
            let comments=client.review_threads(&captured,None,&control);
            Ok((pull,captured,status,files,comments))
        },|result,this,window,cx|match result {
            Ok((pull,captured,status,files,comments))=>{
                this.create=false;this.confirm=None;this.review.reset_content();this.conversations.reset_content();this.review.show_list=false;this.review.section=Section::Overview;
                this.description=pull.body.as_ref().map(|body|text::editor(body,"markdown",None,window,cx));
                let mut errors=vec![];
                match status {Ok(status)=>this.status=Some(status),Err(error)=>{this.status=None;errors.push(format!("Review/check status: {error:#}"));}}
                match files {Ok(page)=>{this.review.files=page.items;this.review.file_page=page.page;this.review.files_next=page.has_next;},Err(error)=>errors.push(format!("Changed files: {error:#}"))}
                match comments {Ok(page)=>{this.receive_threads(page,None);},Err(error)=>errors.push(format!("Discussions: {error:#}"))}
                this.pull=Some(pull);
                let draft=this.saved.iter().find_map(|draft|match draft {Draft::Review(review) if review.pull==captured=>Some(review.clone()),_=>None});
                this.body.update(cx,|input,cx|input.set_value(draft.as_ref().map(|d|d.body.clone()).unwrap_or_default(),window,cx));
                this.event=draft.as_ref().map(|d|d.event).unwrap_or(ReviewEvent::Comment);
                let had_review=draft.is_some();
                this.restore_review(draft,window,cx);
                this.restore_reply(window,cx);
                let had_saved=had_review||this.conversations.active.is_some();
                let older=this.saved.iter().any(|draft| match draft {
                    Draft::Review(review) => review.pull.repository==captured.repository && review.pull.number==captured.number && review.pull!=captured,
                    Draft::Reply(reply) => !reply.body.is_empty() && reply.target.pull.repository==captured.repository && reply.target.pull.number==captured.number && (reply.target.pull!=captured || this.account.as_deref()!=Some(reply.target.account.as_str())),
                    _ => false,
                });
                if older { this.notice=Some("Drafts from another captured head or account remain in Recover drafts. Copy their exact text and original identities; targets are never remapped.".into()); }
                else if !had_saved { this.notice=None; }
                this.error=(!errors.is_empty()).then(||errors.join("\n"));
                this.save_status=if had_saved {"Draft restored locally"}else{"No local draft yet"}.into();
                this.focus_visible_section(window,cx);
            },Err(error)=>this.error=Some(format!("{error:#}"))
        },window,cx);
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.create {
            self.save_draft(window, cx);
            return;
        }
        self.save_draft(window, cx);
        self.create = true;
        self.conversations.reset_content();
        self.review.reset_content();
        self.review.comments.clear();
        self.review.composing = None;
        self.pull = None;
        self.status = None;
        self.description = None;
        self.confirm = None;
        self.body
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.notice=Some("Use a branch already published to GitHub. Creating a PR never pushes a branch. Choose Draft or Ready, then review the destination.".into());
        self.save_draft(window, cx);
        cx.notify();
    }
    fn load_templates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let repo = self.repo.clone();
        let revision = self.base.read(cx).value().to_string();
        self.run(false,move|_|github::local_templates(&repo,&revision),|result,this,_,_|match result {Ok(templates)=>{this.templates=templates;this.notice=Some("Templates came from the locally available base revision. Choose one to append its exact text; repository HTML is never executed.".into());},Err(error)=>this.error=Some(format!("{error:#}"))},window,cx);
    }
    fn review_submission(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_draft(window, cx);
        let draft = match self.current_draft(cx) {
            Ok(draft) => draft,
            Err(issue) => {
                self.draft_issue = Some(issue);
                self.save_status = issue.save_message().into();
                cx.notify();
                return;
            }
        };
        self.draft_issue = None;
        if let Draft::Review(review) = &draft {
            if review.composing.is_some() {
                self.error = Some(
                    "Add or discard the unfinished inline comment before reviewing submission."
                        .into(),
                );
                cx.notify();
                return;
            }
            if let Err(error) = github::validate_text(
                &review.body,
                self.discussion
                    || review.event == ReviewEvent::RequestChanges
                    || (review.event == ReviewEvent::Comment && review.comments.is_empty()),
            ) {
                self.error = Some(error.to_string());
                cx.notify();
                return;
            }
        }
        let fixture = self.review.fixture;
        let moved = self.review.fixture_moved;
        if let Draft::Pull(pull) = draft {
            self.run(
                false,
                move |control| {
                    let transport = UiTransport::open(fixture, moved)?;
                    let account = transport.login().to_owned();
                    Ok((Client(transport).prepare_create(pull, &control)?, account))
                },
                |result, this, window, cx| match result {
                    Ok((pull, account)) => {
                        this.account = Some(account.clone());
                        this.confirm = Some(Confirmation {
                            action: Action::Create(pull),
                            account,
                        });
                        this.focus_confirmation(window, cx);
                    }
                    Err(error) => this.error = Some(format!("{error:#}")),
                },
                window,
                cx,
            );
            return;
        }
        let Some(account) = self.account.clone() else {
            self.error = Some("Connect and refresh the GitHub account before submission.".into());
            return;
        };
        self.confirm = Some(Confirmation {
            account,
            action: match draft {
                Draft::Pull(_) | Draft::Reply(_) => unreachable!(),
                Draft::Review(p) if self.discussion => Action::Comment {
                    pull: p.pull,
                    body: p.body,
                },
                Draft::Review(p) => Action::Review(p),
            },
        });
        self.focus_confirmation(window, cx);
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(confirmation) = self.confirm.take() else {
            return;
        };
        self.save_draft(window, cx);
        let fixture = self.review.fixture;
        let moved = self.review.fixture_moved;
        let sent_action = confirmation.action.clone();
        self.run(true, move |control| {
            let transport = UiTransport::open(fixture, moved)?;
            anyhow::ensure!(transport.login() == confirmation.account, "The connected GitHub account changed. Review the destination again before submitting.");
            let destination = format!("{} · account {}", confirmation.action.destination(), confirmation.account);
            drafts::record_attempt(destination.clone(), "Submission started; if no final outcome appears, inspect GitHub before sending again.".into())?;
            let completed_reply = match &confirmation.action {
                Action::Reply { target, body } => Some(drafts::ReplyDraft { target: target.clone(), body: body.clone() }),
                _ => None,
            };
            let result = Client(transport).execute(confirmation.action, &control);
            let outcome = match &result { Ok(notice) => notice.clone(), Err(error) => format!("{error:#}") };
            if let Err(error) = drafts::record_reply_outcome(destination, outcome.chars().take(4000).collect(), completed_reply.as_ref().filter(|_|result.is_ok())) {
                return Err(anyhow::anyhow!("The GitHub action finished but saving its outcome failed: {error}. Check GitHub before resubmitting."));
            }
            result
        }, move |result, this, window, cx| match result {
            Ok(notice) => {
                if matches!(sent_action, Action::Reply {..} | Action::SetThreadResolved {..}) {
                    this.finish_conversation_action(&sent_action, window, cx);
                } else {
                    this.notice = Some(format!("{notice}. The local draft remains available; submitting it again creates another outbound action."));
                    if matches!(sent_action, Action::Create(_)) {
                        this.create = false; this.review.show_list = true;
                        this.notice = Some("Pull request created. Refresh PRs to open it.".into());
                        this.focus_visible_section(window, cx);
                    }
                }
            },
            Err(error) => {
                this.error = Some(format!("{error:#}"));
                if this.conversations.active.is_some() { this.focus_visible_section(window,cx); }
            },
        }, window, cx);
    }
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending && self.writing {
            self.error=Some("A GitHub action is in progress for this captured destination. Wait for the result, or choose Cancel and inspect GitHub before resubmitting.".into());
            cx.notify();
            return;
        }
        if let Some(control) = &self.control {
            control.cancel();
        }
        if let Some(control) = &self.local_control {
            control.cancel();
        }
        self.save_draft(window, cx);
        self.closed = true;
        self.request_generation = self.request_generation.wrapping_add(1);
        self.pending = false;
        self.writing = false;
        self.task = None;
        self.review.suspend_preparation();
        self.local_generation = self.local_generation.wrapping_add(1);
        self.local_loading = false;
        self.local_task = None;
        if let Some(owner) = self.owner.upgrade() {
            let entry = WarmPanel {
                owner: owner.entity_id(),
                worktree: self.repo.path().to_owned(),
                panel: cx.entity(),
                bytes: self.retained_review_bytes(cx),
            };
            cx.default_global::<WarmPanels>().retain(entry);
        }
        window.close_dialog(cx);
        if let Some(owner) = self.owner.upgrade() {
            let focus = owner.read(cx).modal_return(window);
            focus.focus(window, cx);
        }
    }
    fn changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pull) = self
            .pull
            .as_ref()
            .and_then(|pull| self.destination(cx).ok().map(|repo| pull.capture(repo)))
        else {
            return;
        };
        let target = ComparisonTarget {
            worktree: self.repo.path().to_owned(),
            pull,
        };
        let repo = self.repo.clone();
        let cancellation = HistoryCancellation::default();
        self.local_control = Some(cancellation.clone());
        self.run(false, move |_| {
            let comparison = repo.compare_revisions(&target.pull.base.sha, &target.pull.head.sha, gitturtle_core::ComparisonMode::SinceBranching, &cancellation)?;
            Ok((target.worktree, comparison))
        }, |result, this, window, cx| match result {
            Ok((path, comparison)) => {
                this.close(window, cx);
                let _ = this.owner.update(cx, |owner, cx| {
                    if owner.path.as_ref() == Some(&path) && owner.page == AppPage::Repository { owner.show_revision_comparison(comparison, window, cx); }
                });
            },
            Err(error) => this.error = Some(format!("PR changes need their captured head, base, and common ancestor locally. Use an explicit Fetch if needed, then reopen the PR. {error:#}")),
        }, window, cx);
    }
    fn discard_draft(&mut self, draft: Draft, window: &mut Window, cx: &mut Context<Self>) {
        let key = draft.key();
        let response = self.owner.update(cx, |owner, _| {
            owner
                .preferences_writer
                .submit(move || drafts::remove_exact(&draft))
        });
        let Ok(response) = response else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ =
                this.update_in(cx, |this, _, cx| {
                    match result {
                        Ok(Ok(())) => {
                            this.saved.retain(|draft| draft.key() != key);
                            this.notice =
                                Some("The selected saved draft was discarded locally.".into());
                        }
                        _ => this.error = Some(
                            "Could not discard the selected draft; its stored text was preserved."
                                .into(),
                        ),
                    }
                    cx.notify();
                });
        })
        .detach();
    }
}
impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let pending = self.pending;
        let scroll = self.review.body_scroll.clone();
        let observed = self.review.revealed_focus.clone();
        let show_editor =
            self.create || (self.pull.is_some() && self.review.section == Section::Review);
        // The room a focused control's ring takes outside its edge.
        let room = appearance::button_ring_room(cx);
        // The panel's content, its text one inset in from this edge.
        let content = div().debug_selector(||"github-panel-content".into()).min_w_0().flex().flex_col().gap_2()
            .when(self.review.fixture,|element|element.child(self.part("banner",div(),cx).flex().flex_wrap().items_center().gap_2().py_2().px(text_inset()).rounded(px(6.)).bg(rgb(p.subtle))
                .child(label("github-fixture-mode","OFFLINE FIXTURE · native review · no network or credentials").debug_selector(||"github-fixture-mode".into()).min_w_0().text_color(rgb(p.muted)))
                .child(button("github-fixture-move",if self.review.fixture_moved{"Fixture head moved"}else{"Move fixture head"},"",false).disabled(pending||self.review.fixture_moved).on_click(cx.listener(|this,_,_,cx|{this.review.fixture_moved=true;this.confirm=None;this.notice=Some("Fixture head moved remotely. This view and its draft retain the original commit. Submit to verify stale-head refusal, or Refresh PRs and reopen to inspect the new head.".into());cx.notify();})))))
            .child(self.part("toolbar",div(),cx).flex().flex_wrap().items_center().gap_2()
                // Focus moved onto the field, by a click or by Name repository below, which
                // reveals it even when it holds focus already, scrolls it into view with its
                // ring, as a Tab onto it does.
                .child(div().debug_selector(||"github-destination".into()).w(appearance::ui_size(280.)).min_w(px(190.)).on_children_prepainted(self.reveal_on_focus(self.destination.read(cx).focus_handle(cx))).child(Input::new(&self.destination).aria_label("GitHub destination owner and repository").disabled(pending)))
                .child(button("github-refresh",if self.review.fixture{"Refresh fixture PRs"}else{"Refresh PRs"},"refresh",false).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.refresh(1,window,cx))))
                .child(button("github-create","New pull request","plus",false).disabled(pending||self.review.fixture).on_click(cx.listener(|this,_,window,cx|this.create(window,cx))))
                .child(div().flex_1()).child(button("github-account-options",self.account.as_ref().map(|account|format!("Account · {account}")).unwrap_or_else(||"Connect account".into()),"",self.review.connection_open).debug_selector(||"github-account-options".into()).toggled(self.review.connection_open).disabled(pending).on_click(cx.listener(|this,_,_,cx|{this.review.connection_open= !this.review.connection_open;cx.notify();}))))
            .when(self.review.connection_open,|element|element.child(self.part("connection",div(),cx).p_3().border_1().border_color(rgb(p.border)).rounded(px(6.)).flex().flex_col().gap_2()
                .child(label("github-worktree",format!("Local worktree: {}",self.repo.path().display())).text_color(rgb(p.muted)))
                .child(label("github-connection-help","Connect uses your current GitHub CLI account and saves GitTurtle authorization in secure storage. Refresh and PR selection are explicit network reads.").text_color(rgb(p.muted)))
                .child(div().flex().flex_wrap().gap_2()
                    .child(button("github-connect","Connect GitHub CLI account","",false).disabled(pending||self.review.fixture).on_click(cx.listener(|this,_,window,cx|this.connect(window,cx))))
                    .child(copy_button(copy_feedback::Mark::new("github-sign-in-help",SIGN_IN_COMMAND),"Copy sign-in command","",cx).on_click(|_,window,cx|copy_feedback::copy(window,cx,Some(copy_feedback::Mark::new("github-sign-in-help",SIGN_IN_COMMAND)),"sign-in command",SIGN_IN_COMMAND.into())))
                    .child(button("github-disconnect","Disconnect GitTurtle","",false).disabled(pending||self.review.fixture).on_click(cx.listener(|this,_,window,cx|this.run(false,|_|SecureStore.remove(),|result,this,_,_|match result{Ok(())=>{this.account=None;this.notice=Some("GitTurtle authorization removed. GitHub CLI and ordinary Git authentication retain their settings.".into());},Err(error)=>this.error=Some(error.to_string())},window,cx)))))))
            .when_some(self.notice.as_ref(),|element,notice|element.child(label("github-notice",notice.clone()).debug_selector(||"github-notice".into()).ml(text_inset()).role(Role::Status).a11y_synthetic_children(native_accessibility::polite).text_color(rgb(p.muted))))
            .when_some(self.error.as_ref().filter(|_|self.conversations.active.is_none()),|element,error|element.child(label("github-error",error.clone()).ml(text_inset()).role(Role::Alert).a11y_synthetic_children(native_accessibility::assertive).text_color(rgb(p.warning))))
            .when_some(self.draft_issue,|element,issue|element.child(label("github-draft-validation",issue.message()).ml(text_inset()).role(Role::Alert).a11y_synthetic_children(native_accessibility::assertive).text_color(rgb(p.warning))))
            .when(pending,|element|element.child(self.part("busy",div(),cx).flex().items_center().gap_2().pl(text_inset())
                .child(label("github-busy",if self.writing{"Sending the captured review…"}else{"Loading your requested PR context…"}).role(Role::Status).a11y_synthetic_children(|builder|{native_accessibility::polite(builder);builder.parent_node().set_busy();}))
                .child(button("github-cancel","Cancel","",false).track_focus(self.cancel_request.handle()).debug_selector(||"github-cancel".into()).on_click(cx.listener(|this,_,_,cx|{if let Some(control)=&this.control{control.cancel();}
                    if let Some(control)=&this.local_control{control.cancel();}this.notice=Some(if this.writing{"Cancellation requested. Check GitHub before resubmitting; an accepted action is never replayed."}else{"Cancellation requested. Your review draft is retained."}.into());cx.notify();})))))
            .when(!self.create && (self.review.show_list||self.pull.is_none()),|element|element
                // The list scrolls, so it clips its rows on both axes. Its full-width rows keep
                // their rings' room inside that clip: its sides give the room back through their
                // margin, so the rows' text keeps the panel's edge, and above the first row and
                // below the last the room is the list's own (a negative margin there collapsed
                // the panel's scrolling body to its padding).
                .child(crate::native_accessibility::clips_children(self.part("pr-list",div(),cx).id("github-pr-list").debug_selector(||"github-pr-list".into()).flex_shrink_0().max_h(px(200.)+room*2.).px(room).mx(-room).py(room).overflow_y_scroll().flex().flex_col().gap_1()
                    .children(self.rows.iter().map(|pull|{let pull=pull.clone();let selected=self.pull.as_ref().is_some_and(|p|p.number==pull.number);let number=pull.number;let name=format!("#{} · {}",pull.number,pull.title);let context=format!("{} · {} → {} · {}",if pull.draft{"Draft"}else{"Open"},pull.head.branch,pull.base.branch,pull.user.login);
                        // A list row: the title Button spans it, its label at the panel's text inset,
                        // and the context line below, outside the Button, takes the same inset. The
                        // selected row keeps the selected surface under both lines.
                        div().id(("github-pr-card",pull.number)).flex_shrink_0().pb_1().rounded(px(7.)).when(selected,|row|row.bg(rgb(p.selected))).flex().flex_col()
                            .child(crate::button_frame(("github-pr",number),&name,"",selected).key_context("GitTurtleGithubButton").debug_selector(move||format!("github-pr-{number}")).w_full().accessibility_label(name.clone()).tooltip(name.clone()).child(row_text(format!("github-pr-title-{number}"),name)).disabled(pending).on_click(cx.listener(move|this,_,window,cx|this.inspect(pull.clone(),window,cx))))
                            .child(native_accessibility::static_text(div().id(("github-pr-context",number)),context.clone()).flex().mx(text_inset()).text_size(appearance::ui_text(12.)).text_color(rgb(p.muted)).tooltip({let context=context.clone();move|window,cx|Tooltip::new(context.clone()).build(window,cx)}).child(row_text(format!("github-pr-context-{number}"),context)))
                    }))
                    // One line with the next step: a repository with no GitHub remote names
                    // the destination first, and one with a destination reads its PRs with
                    // Refresh PRs, above. Opening the panel itself contacts nothing. Name
                    // repository moves to a name field, not a search, so it draws no glyph.
                    .when(self.rows.is_empty(),|element|element.child(div().py_5().px(text_inset()).child(if self.local_loaded&&self.destination.read(cx).value().trim().is_empty(){
                        EmptyState::new("github-list-empty","No GitHub remote · Name the repository to load its pull requests").tooltip("Enter the destination as owner/repository or a GitHub.com remote URL. Opening this panel reads local settings and drafts only.")
                            .action(crate::empty_state::action("github-choose-destination","Name repository","").disabled(pending).on_click(cx.listener(|this,_,window,cx|{let focus=this.destination.read(cx).focus_handle(cx);window.focus(&focus,cx);
                                // The field may hold focus already, as it does once the panel opens:
                                // the next frame reveals it all the same.
                                *this.review.revealed_focus.borrow_mut()=Default::default();cx.notify();})))
                    }else{
                        EmptyState::new("github-list-empty","No pull requests shown · Refresh PRs loads the open ones").tooltip("Opening this panel reads local settings and drafts only.")
                    }.block(cx))))))
                // A row that cannot hold its controls wraps them below, whole. A label after a
                // Button takes the panel's text inset, which the Button gives back, so beside it
                // nothing moves and on a line of its own the label starts at the panel's edge
                // for text.
                .child(self.part("pr-pager",div(),cx).flex().flex_wrap().items_center().gap_2().child(button("github-previous","Previous PR page","",false).debug_selector(||"github-previous".into()).mr(-text_inset()).disabled(pending||self.page<=1).on_click(cx.listener(|this,_,window,cx|this.refresh(this.page-1,window,cx))))
                    .child(phrases("github-page",format!("Page {} · up to {} PRs",self.page,github::PAGE_SIZE)).min_w_0().ml(text_inset()).text_color(rgb(p.muted)))
                    .child(button("github-next","Next PR page","",false).disabled(pending||!self.has_next).on_click(cx.listener(|this,_,window,cx|this.refresh(this.page+1,window,cx))))))
            .when_some(self.pull.as_ref(),|element,pull|element
                .child(self.part("pull-header",div(),cx).flex().flex_col().gap_2()
                    .child(div().flex().flex_wrap().items_center().gap_2().child(button("github-back-list",if self.review.show_list{"Hide PR list"}else{"Pull requests"},"arrow-left",false).debug_selector(||"github-back-list".into()).mr(-text_inset()).disabled(pending).on_click(cx.listener(|this,_,window,cx|{this.save_draft(window,cx);this.review.show_list= !this.review.show_list;cx.notify();})))
                        .child(label("github-pull-title",format!("#{} · {}",pull.number,pull.title)).debug_selector(||"github-pull-title".into()).min_w_0().ml(text_inset()).font_weight(FontWeight::SEMIBOLD).text_size(appearance::ui_text(15.))))
                    .child(label("github-branch-context",format!("{} → {} · {}",pull.head.branch,pull.base.branch,if pull.draft{"Draft PR"}else{"Open PR"})).ml(text_inset()).text_color(rgb(p.muted))))
                .child(self.render_tabs(cx))
                .when(self.review.section==Section::Overview,|element|element
                    .child(label("github-captured-identities",format!("Before · {}\nAfter  · {}",pull.base.sha,pull.head.sha)).ml(text_inset()).text_color(rgb(p.muted)))
                    .when_some(self.description.as_ref(),|element,description|element.child(self.part("description",div(),cx).h(px(120.)).flex_shrink_0().child(crate::editor_find::Editor::new(description).readonly(true).h_full().aria_label("Pull request description, exact Markdown source"))))
                    .when_some(self.status.as_ref(),|element,status|element.child(div().py_3().px(text_inset()).bg(rgb(p.subtle)).rounded(px(6.)).flex().flex_col().gap_1()
                        .child(label("github-review-status",format!("Reviews · {}",if status.reviews.is_empty(){"None reported".into()}else{status.reviews.join(" · ")})))
                        .child(label("github-check-status",format!("Checks · {}{}",if status.checks.is_empty(){"None reported".into()}else{status.checks.join(" · ")},if status.incomplete{" · Additional status entries are outside this preview"}else{""})))))
                    .child(self.render_threads(None,window,cx))
                    .child(self.part("open-changes",div(),cx).flex().child(button("github-open-changes","Open captured local comparison","",false).debug_selector(||"github-open-changes".into()).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.changes(window,cx))))))
                .when(self.review.section==Section::Files,|element|element.child(self.render_files(window,cx)))
                .when(self.review.section==Section::Review,|element|element.child(self.render_collected(cx))))
            .when(self.create,|element|element
                .child(button("github-create-back","Pull requests","arrow-left",false).disabled(pending).on_click(cx.listener(|this,_,window,cx|{this.save_draft(window,cx);this.create=false;this.confirm=None;this.review.show_list=true;this.focus_visible_section(window,cx);cx.notify();})))
                .child(label("github-create-fields","New pull request · published head → destination base").font_weight(FontWeight::SEMIBOLD))
                .child(Input::new(&self.title).aria_label("Pull request title").disabled(pending))
                .child(div().flex().gap_2().child(div().flex_1().child(Input::new(&self.head).aria_label("Published head branch").disabled(pending))).child(div().flex_1().child(Input::new(&self.base).aria_label("Destination base branch").disabled(pending))))
                .child(div().flex().gap_2().children([(true,"Draft"),(false,"Ready")].map(|(draft,name)|button(name,name,"",self.draft==draft).toggled(self.draft==draft).disabled(pending).on_click(cx.listener(move|this,_,window,cx|{this.draft=draft;this.confirm=None;this.save_draft(window,cx);}))))
                    .child(button("github-load-templates","Load local base templates","",false).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.load_templates(window,cx)))))
                .children(self.templates.iter().enumerate().map(|(index,template)|{let template=template.clone();button(("github-template",index),format!("Append {} · {}",template.path,&template.revision[..8]),"",false).disabled(pending).on_click(cx.listener(move|this,_,window,cx|{let body=format!("{}{}{}",this.body.read(cx).value(),if this.body.read(cx).value().is_empty(){""}else{"\n\n"},template.body);this.body.update(cx,|input,cx|input.set_value(body,window,cx));this.save_draft(window,cx);} ))})))
            .when(show_editor,|element|element
                .when(!self.create,|element|element.child(self.part("events",div(),cx).flex().flex_wrap().gap_1()
                    .children([ReviewEvent::Comment,ReviewEvent::Approve,ReviewEvent::RequestChanges].map(|event|button(event.label(),event.label(),"",!self.discussion&&self.event==event).toggled(!self.discussion&&self.event==event).disabled(pending).on_click(cx.listener(move|this,_,window,cx|{this.discussion=false;this.event=event;this.confirm=None;this.save_draft(window,cx);}))))
                    .child(button("github-discussion","Discussion only","",self.discussion).toggled(self.discussion).disabled(pending||!self.review.comments.is_empty()||self.review.composing.is_some()).on_click(cx.listener(|this,_,_,cx|{this.discussion=true;this.confirm=None;cx.notify();})))))
                .child(div().debug_selector(||"github-review-summary".into()).on_children_prepainted(self.reveal_on_focus(self.body.read(cx).focus_handle(cx))).child(Textarea::new(&self.body).h(appearance::ui_size(130.)).aria_label(if self.create{"Pull request description"}else{"Review summary"}).disabled(pending)))
                // The draft status beside Review submission… where it fits there whole, at the
                // panel's text inset; otherwise on a line of its own below it, wrapping, so its
                // remedy stays readable and the button stays inside the panel (at 18 pt a failed
                // save's status pushed the button past the panel's right edge).
                .child(self.part("submit",div(),cx).flex().flex_wrap_reverse().items_center().gap_2().child(label("github-draft-status",self.save_status.clone()).debug_selector(||"github-draft-status".into()).min_w_0().mx(text_inset()).role(Role::Status).a11y_synthetic_children(native_accessibility::polite).text_color(rgb(p.muted)))
                    .child(button("github-review-submit","Review submission…","",true).debug_selector(||"github-review-submit".into()).ml_auto().disabled(pending).on_click(cx.listener(|this,_,window,cx|this.review_submission(window,cx))))))
            .child(self.render_reply_composer(cx))
            .when_some(self.confirm.as_ref(),|element,confirmation|element.child(div().debug_selector(||"github-confirmation".into()).on_children_prepainted(self.reveal_on_focus(self.conversations.confirm_focus.clone())).track_focus(&self.conversations.confirm_focus).tab_stop(true).p_3().border_1().border_color(rgb(p.accent)).rounded(px(7.)).flex().flex_col().gap_2()
                .child(label("github-confirm-destination",format!("{}\nAccount · {}",confirmation.action.destination(),confirmation.account)).font_weight(FontWeight::SEMIBOLD))
                .child(div().id("github-confirm-text-scroll").debug_selector(||"github-confirm-text-scroll".into()).flex_shrink_0().max_h(px(180.)).overflow_y_scroll().child(label("github-confirm-text",match &confirmation.action{Action::Create(pull)=>format!("{}\n\n{}",pull.title,pull.body),Action::Comment{body,..}|Action::Reply{body,..}=>body.clone(),Action::SetThreadResolved{resolved,..}=>if *resolved{"Resolve this conversation"}else{"Reopen this conversation"}.into(),Action::Review(review)=>format!("{} review · {} inline comments\n\n{}{}",review.event.label(),review.comments.len(),review.body,review.comments.iter().map(|comment|format!("\n\n{}\n{}",github::review::position_label(comment),comment.body)).collect::<String>())})))
                .child(label("github-confirm-guidance","Send performs one outbound action with these exact captured targets and comments. Inspect the destination before repeating an uncertain attempt.").text_color(rgb(p.muted)))
                .child(div().flex().gap_2().child(button("github-confirm-send",if self.review.fixture{"Send to offline fixture"}else{"Send to GitHub"},"",true).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.submit(window,cx))))
                    .child(button("github-edit-submission","Keep editing","",false).disabled(pending).on_click(cx.listener(|this,_,window,cx|{this.confirm=None;this.focus_visible_section(window,cx);cx.notify();}))))))
            .child(self.part("recovery",div(),cx).flex().flex_wrap().items_center().gap_2().child(button("github-recovery-toggle",format!("Recover drafts · {}",self.saved.len()),"",self.review.recovery_open).debug_selector(||"github-recovery-toggle".into()).mr(-text_inset()).toggled(self.review.recovery_open).on_click(cx.listener(|this,_,_,cx|{this.review.recovery_open= !this.review.recovery_open;cx.notify();})))
                .child(phrases("github-passive-policy",if cfg!(target_os="macos"){"Drafts stay on this Mac · outbound actions are explicit"}else{"Drafts stay on this computer · outbound actions are explicit"}).min_w_0().ml(text_inset()).text_color(rgb(p.muted))))
            .when(self.review.recovery_open,|element|element
                .child(label("github-saved-heading","Captured drafts · Copy includes all inline text and original positions; older heads are never remapped").ml(text_inset()).text_color(rgb(p.muted)))
                .child(crate::native_accessibility::clips_children(div().id("github-saved-drafts").debug_selector(||"github-saved-drafts".into()).flex_shrink_0().max_h(px(160.)).overflow_y_scroll().flex().flex_col().gap_2().children(self.saved.iter().filter(|draft| !matches!(draft,Draft::Reply(reply) if reply.body.is_empty())).take(128).enumerate().map(|(index,draft)|{let draft=draft.clone();let discard=draft.clone();let mark=copy_feedback::Mark::new(("github-copy-draft",index),draft.key());let copy_mark=mark.clone();div().flex_shrink_0().flex().flex_col().gap_1().child(label(("github-draft-identity",index),match &draft {Draft::Reply(reply)=>format!("Reply · {} #{} · {} · {}",reply.target.pull.repository.label(),reply.target.pull.number,reply.target.path,reply.target.account),_=>draft.key()}).ml(text_inset()).text_color(rgb(p.muted)))
                    .child(div().flex().gap_2().child(copy_button(mark,"Copy complete draft", "", cx).on_click(move|_,window,cx|copy_feedback::copy(window,cx,Some(copy_mark.clone()),"draft",draft.recovery_text())))
                        .child(button(("github-discard-draft",index),"Discard saved draft","",false).disabled(pending).on_click(cx.listener(move|this,_,window,cx|this.discard_draft(discard.clone(),window,cx)))))}))))
                .when(!self.attempts.is_empty(),|element|element.child(label("github-attempt-heading","Previous outbound attempts · inspect the destination before repeating").ml(text_inset()))
                    .child(crate::native_accessibility::clips_children(div().id("github-attempt-list").debug_selector(||"github-attempt-list".into()).flex_shrink_0().max_h(px(100.)).overflow_y_scroll().flex().flex_col().gap_1().children(self.attempts.iter().rev().take(10).enumerate().map(|(index,attempt)|label(("github-attempt",index),format!("{}: {}",attempt.destination,attempt.outcome)).ml(text_inset()).flex_shrink_0()))))));
        // The body scrolls inside the room its controls' focus rings take, which its sides
        // give back through their margin; above and below, the dialog's clip of its body
        // allows no margin, so the room is the body's own. While the body overflows, its
        // scrollbar shows the rest is there.
        let cue = crate::tags::ScrollCue::new(&scroll, room);
        cue.list(
            div()
                .on_children_prepainted(cue.observe(move |_, window, cx| {
                    *observed.borrow_mut() = review::FocusObservation {
                        focus: window.focused(cx),
                        viewport: scroll.bounds(),
                        rem_size: window.rem_size(),
                    };
                }))
                .id("github-panel")
                .debug_selector(|| "github-panel".into())
                .track_scroll(&self.review.body_scroll)
                .min_w_0()
                .max_h(body_max_height(window))
                .p(room)
                .mx(-room)
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .child(content),
            "github-panel-scrollbar",
        )
    }
}

/// Opens the panel over `app`'s repository on the offline review fixture,
/// listing its two PRs with the first one open, as the native review shows
/// it (`width_walk`). The caller settles the window.
#[cfg(test)]
pub(crate) fn open_offline_fixture(app: &Entity<GitTurtle>, window: &mut Window, cx: &mut App) {
    let repo = app.read(cx).repository.clone().unwrap();
    let panel = cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx));
    let pull = github::review::fixture_pull(false);
    let mut other = pull.clone();
    other.number = 43;
    panel.update(cx, |panel, cx| {
        panel.rows = vec![pull.clone(), other];
        panel.review.fixture = true;
        panel.account = Some("offline-reviewer".into());
        panel.pull = Some(pull);
        panel.notice = Some("Fixture PRs refreshed.".into());
        panel.close(window, cx);
    });
    app.update(cx, |app, cx| app.open_github(window, cx));
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::component::Root;
    use std::{cell::RefCell, rc::Rc};

    fn entered_fields() -> DraftFields {
        DraftFields {
            title: "Keep this title".into(),
            body: "Exact body\r\n  with spaces and café\n".into(),
            head: "topic/branch".into(),
            base: "main".into(),
        }
    }

    #[test]
    fn destination_typing_persists_only_settled_draft_and_preserves_prior_destinations() {
        use std::{
            collections::HashMap,
            sync::{Arc, Mutex},
        };

        let executor = operations::SerialExecutor::new("github-deferred-draft-test");
        let saver = commit_drafts::CoalescingSaver::default();
        let stored = Arc::new(Mutex::new(HashMap::<String, Draft>::new()));
        let persist = |draft: Draft| {
            let output = stored.clone();
            // Deliberately drop the UI reply: accepted writes must survive it.
            let _ = saver.queue_with(&executor, draft.key(), draft, move |batch| {
                output.lock().unwrap().extend(batch.clone());
                Ok(())
            });
            futures::executor::block_on(executor.submit(|| Ok(())))
                .unwrap()
                .unwrap();
        };
        let capture = |destination: &str| {
            capture_draft(
                destination,
                true,
                true,
                ReviewEvent::Comment,
                None,
                entered_fields(),
            )
            .unwrap()
        };
        let previous = capture("gitturtle-fixture/previous");
        let previous_key = previous.key();
        persist(previous);

        let mut deferred = DeferredDraft::default();
        let mut old_ticket = None;
        let destination = "milestone-qa";
        for end in 1..=destination.len() {
            let ticket = deferred.replace(capture(&format!(
                "gitturtle-fixture/{}",
                &destination[..end],
            )));
            if let Some(old) = old_ticket {
                assert!(
                    deferred.take(old).is_none(),
                    "superseded timer published a prefix"
                );
            }
            old_ticket = Some(ticket);
            assert_eq!(stored.lock().unwrap().len(), 1);
        }
        assert_eq!(
            finished_save_status(
                Ok(capture("gitturtle-fixture/milestone-qa")),
                deferred.is_pending(),
                true,
            ),
            "Saving local draft…",
            "an earlier successful save must not mark a pending edit saved",
        );
        let final_draft = deferred.take(old_ticket.unwrap()).unwrap();
        let final_key = final_draft.key();
        persist(final_draft);
        assert!(!deferred.is_pending());
        let saved = stored.lock().unwrap();
        assert_eq!(
            saved.len(),
            2,
            "intermediate destinations must not consume recovery slots"
        );
        assert!(saved.contains_key(&previous_key));
        let Draft::Pull(final_draft) = &saved[&final_key] else {
            unreachable!()
        };
        assert_eq!(
            final_draft.repository.label(),
            "gitturtle-fixture/milestone-qa"
        );
        assert_eq!(final_draft.title, entered_fields().title);
        assert_eq!(final_draft.body, entered_fields().body);
    }

    #[test]
    fn correcting_destination_captures_existing_text_and_distinguishes_missing_selection() {
        let fields = entered_fields();
        let capture = |destination, creating| {
            capture_draft(
                destination,
                creating,
                true,
                ReviewEvent::Comment,
                None,
                fields.clone(),
            )
        };
        for invalid in ["", "invalid destination", "owner"] {
            assert_eq!(capture(invalid, true).unwrap_err(), DraftIssue::Destination);
        }
        assert_eq!(
            capture("invalid destination", false).unwrap_err(),
            DraftIssue::Selection
        );
        assert_eq!(
            capture("fixture/repository", false).unwrap_err(),
            DraftIssue::Selection
        );
        let Draft::Pull(corrected) = capture("fixture/repository", true).unwrap() else {
            panic!("create form produces a pull request draft");
        };
        assert_eq!(corrected.repository.label(), "fixture/repository");
        assert_eq!(corrected.title, fields.title);
        assert_eq!(corrected.body, fields.body);
        assert_eq!(corrected.head, fields.head);
        assert_eq!(corrected.base, fields.base);
        assert!(corrected.draft);
        assert_eq!(corrected.expected_head, None);
        assert_eq!(corrected.expected_base, None);
    }

    #[test]
    fn save_completion_keeps_invalid_destination_and_failed_storage_unsaved() {
        assert_eq!(
            finished_save_status(Err(DraftIssue::Destination), false, true),
            DraftIssue::Destination.save_message(),
        );
        let capture = || {
            capture_draft(
                "fixture/repository",
                true,
                true,
                ReviewEvent::Comment,
                None,
                entered_fields(),
            )
        };
        assert_eq!(
            finished_save_status(capture(), true, true),
            "Saving local draft…"
        );
        assert_eq!(
            finished_save_status(capture(), false, true),
            "Draft saved locally"
        );
        assert_eq!(
            finished_save_status(capture(), true, false),
            "Draft save failed — keep this window open or copy the text"
        );
    }

    #[test]
    fn out_of_order_save_replies_cannot_replace_a_newer_failure_or_success() {
        let capture = || {
            capture_draft(
                "fixture/repository",
                true,
                true,
                ReviewEvent::Comment,
                None,
                entered_fields(),
            )
        };
        let mut status = String::new();
        // The failing saver retains its unsaved batch; there is no scheduled
        // retry. An older success must not leave the form stuck on Saving.
        apply_save_completion(&mut status, 2, 2, capture(), true, false);
        let failure = status.clone();
        assert!(failure.contains("failed"));
        apply_save_completion(&mut status, 1, 2, capture(), true, true);
        assert_eq!(status, failure);

        apply_save_completion(&mut status, 3, 3, capture(), false, true);
        assert_eq!(status, "Draft saved locally");
        apply_save_completion(&mut status, 2, 3, capture(), false, false);
        assert_eq!(status, "Draft saved locally");
    }

    #[test]
    fn shutdown_awaits_actual_coalesced_save_when_no_barrier_slot_is_available() {
        use std::sync::{Arc, Mutex, mpsc};

        let executor = operations::SerialExecutor::new("github-full-quit-queue-test");
        let (started, running) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let first = executor.submit(move || {
            started.send(())?;
            gate.recv()?;
            Ok(())
        });
        running.recv().unwrap();
        let queued: Vec<_> = (0..7).map(|_| executor.submit(|| Ok(()))).collect();
        let saver = commit_drafts::CoalescingSaver::default();
        let saved = Arc::new(Mutex::new(Vec::<String>::new()));
        let output = saved.clone();
        let capture = |body: &str| {
            let mut fields = entered_fields();
            fields.body = body.into();
            capture_draft(
                "fixture/repository",
                true,
                true,
                ReviewEvent::Comment,
                None,
                fields,
            )
            .unwrap()
        };
        let draft = capture("Before closing");
        let mut completion = None;
        let ui_reply = track_save_completion(
            &mut completion,
            saver.queue_with(&executor, draft.key(), draft, move |batch| {
                output
                    .lock()
                    .unwrap()
                    .extend(batch.values().map(|draft| draft.text().to_owned()));
                Ok(())
            }),
        )
        .unwrap();
        drop(ui_reply);
        assert!(
            futures::executor::block_on(executor.submit(|| Ok(())))
                .unwrap()
                .is_err(),
            "the draft occupied the eighth slot, so an extra barrier must be refused",
        );
        let final_draft = capture("Last characters before closing");
        let response = saver.queue_with(&executor, final_draft.key(), final_draft, |_| {
            panic!("the close edit must join the already accepted save")
        });
        assert!(response.is_none());
        assert!(track_save_completion(&mut completion, response).is_none());
        // The panel and its UI reply can go away; the independent completion
        // still represents the entire coalesced job, without another queue slot.
        drop(saver);
        release.send(()).unwrap();
        assert!(futures::executor::block_on(completion.unwrap()));
        assert_eq!(
            *saved.lock().unwrap(),
            vec!["Last characters before closing"]
        );
        futures::executor::block_on(first).unwrap().unwrap();
        for response in queued {
            futures::executor::block_on(response).unwrap().unwrap();
        }
    }

    #[gpui::test]
    fn window_close_completion_survives_removed_window_until_app_shutdown(cx: &mut TestAppContext) {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        // Shutdown excludes tasks from its own foreground session to avoid
        // reentering App. Preference completions come from the background.
        cx.executor().set_block_on_ticks(100..=100);
        let window = cx.add_window(|_, _| gpui::Empty);
        let (complete, response) = futures::channel::oneshot::channel();
        let mut response = Some(response);
        cx.update(|cx| {
            cx.on_window_closed(move |cx, id| {
                if id == window.window_id() {
                    let completion = track_save_completion(&mut None, response.take()).unwrap();
                    retain_window_close_completion(completion, cx);
                }
            })
            .detach();
            window
                .update(cx, |_, window, _| window.remove_window())
                .unwrap();
            assert!(cx.windows().is_empty());
        });
        assert!(
            !complete.is_canceled(),
            "closing the window dropped its final save completion",
        );
        let waited = Arc::new(AtomicBool::new(false));
        let completed = waited.clone();
        cx.executor()
            .spawn(async move {
                complete.send(Ok(())).unwrap();
                completed.store(true, Ordering::SeqCst);
            })
            .detach();
        assert!(!waited.load(Ordering::SeqCst));
        // This executes GPUI's real shutdown observer collection and bounded
        // future wait after the window and its view have already been released.
        cx.quit();
        assert!(waited.load(Ordering::SeqCst));
    }

    #[gpui::test]
    async fn opening_github_from_an_active_repository_update_defers_local_reads(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, cx) = repository_window(cx);
        open_panel(&app, cx).await;
        cx.update(|window, cx| {
            assert!(window.has_active_dialog(cx));
            window.close_dialog(cx);
            assert!(!window.has_active_dialog(cx));
        });
    }

    /// The sign-in command's copy button, drawn by the panel's own view in
    /// the dialog layer, reads Copied in its own bounds and restores: a copy
    /// redraws the whole window, not only the view that owns its button.
    #[gpui::test]
    async fn the_sign_in_command_reads_copied_inside_the_panel(cx: &mut TestAppContext) {
        use crate::copy_feedback::{RESTORE_AFTER, tests::*};
        let (_fixture, app, cx) = repository_window(cx);
        // Dialogs slide in on the wall clock, which the test clock does not
        // hold: a loaded host let the panel move between the frame that
        // measured a button and the frame its press drew, and the click
        // missed. They open at rest, where every frame draws them in place.
        cx.update(|_, cx| cx.set_reduce_motion(true));
        open_panel(&app, cx).await;
        crate::commit_message::tests::draw(cx);
        click(cx, "github-account-options");
        cx.run_until_parked();
        let bounds = |cx: &mut VisualTestContext| cx.debug_bounds("github-sign-in-help").unwrap();
        let rest = bounds(cx);
        assert_eq!(shows(cx, "github-sign-in-help"), Shows::Rest);

        click(cx, "github-sign-in-help");
        assert_eq!(
            clipboard(cx),
            "gh auth login --hostname github.com --web --skip-ssh-key"
        );
        assert_eq!(shows(cx, "github-sign-in-help"), Shows::Copied);
        assert_eq!(bounds(cx), rest);
        assert_eq!(announced(cx).unwrap().1, "Copied sign-in command");

        wait(cx, RESTORE_AFTER);
        assert_eq!(shows(cx, "github-sign-in-help"), Shows::Rest);
        assert_eq!(bounds(cx), rest);
        assert_eq!(announced(cx), None);
        cx.update(|window, cx| window.close_dialog(cx));
    }

    /// Escape closes the panel and focus returns to what opened it, drawn as
    /// keyboard focus is drawn there: the header's Pull requests button, full
    /// or compact, takes the Button ring; from the command palette, opened by
    /// the shortcut or a menu's Command Palette item, the control the palette
    /// opened over takes it back, and with only the window's root focused,
    /// which shows nothing, History's list takes it with its accent outline.
    #[gpui::test]
    async fn escape_returns_visible_focus_to_what_opened_the_panel(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, app, cx) = repository_window(cx);
        cx.update(|window, cx| {
            shortcuts::bind_keys(cx);
            app.update(cx, |app, cx| app.apply_appearance(window, cx));
        });
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        let focused = |cx: &mut VisualTestContext, handle: fn(&GitTurtle) -> &FocusHandle| {
            cx.update(|window, cx| handle(app.read(cx)).is_focused(window))
        };
        let full = cx.debug_bounds("pull-requests").unwrap().size.width;

        // The header's button, which a click leaves unfocused.
        for height in [980., 360.] {
            cx.simulate_resize(size(px(1480.), px(height)));
            draw(cx);
            let compact = cx.debug_bounds("pull-requests").unwrap().size.width < full;
            assert_eq!(compact, height < 980., "the {height} px header's button");
            crate::copy_feedback::tests::click(cx, "pull-requests");
            settle_panel(&app, cx).await;
            assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
            assert!(!focused(cx, |app| &app.pull_requests_focus));
            escape(cx);
            assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
            assert!(
                focused(cx, |app| &app.pull_requests_focus),
                "the {height} px header's Pull requests button takes focus back"
            );
            assert!(ringed(cx, "pull-requests"), "the {height} px header's ring");
        }
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);

        // The palette, from History's list and from the window's root.
        for root in [false, true] {
            cx.update(|window, cx| {
                let focus = if root {
                    app.read(cx).app_focus.clone()
                } else {
                    app.read(cx).focus.clone()
                };
                window.focus(&focus, cx);
            });
            draw(cx);
            cx.dispatch_action(ShowCommandPalette);
            draw(cx);
            cx.simulate_input("GitHub pull requests");
            cx.simulate_keystrokes("enter");
            settle_panel(&app, cx).await;
            assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
            assert!(!focused(cx, |app| &app.focus));
            escape(cx);
            assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
            assert!(
                focused(cx, |app| &app.focus),
                "from the {} History's list takes focus",
                if root { "root" } else { "list" }
            );
            assert!(outlined(cx, "history-pane"), "History's accent outline");
        }
    }

    /// A request that answers while keyboard focus is on the busy row's
    /// Cancel takes the row away and hands focus to the section the panel
    /// shows, before a PR is chosen its destination field, drawn with its
    /// accent edge, and Escape still closes the panel. Focus stayed on the
    /// Cancel no frame drew, where Escape reached nothing.
    #[gpui::test]
    async fn a_request_landing_while_its_cancel_holds_focus_leaves_focus_in_the_panel(
        cx: &mut TestAppContext,
    ) {
        use crate::tags::tests::draw;
        let (_fixture, app, cx) = repository_window(cx);
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        let panel = cx.update(|window, cx| {
            let repo = app.read(cx).repository.clone().unwrap();
            let panel = cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx));
            panel.update(cx, |panel, cx| {
                panel.review.fixture = true;
                panel.destination.update(cx, |input, cx| {
                    input.set_value("gitturtle-fixture/native-review", window, cx)
                });
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
            panel
        });
        settle_panel(&app, cx).await;
        draw(cx);

        // Refresh while the operation thread is held, as by a slow request,
        // with keyboard focus on the busy row's Cancel.
        let held = app.read_with(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.refresh(1, window, cx)));
        draw(cx);
        let cancel = panel.read_with(cx, |panel, _| panel.cancel_request.handle().clone());
        cx.update(|window, cx| window.focus(&cancel, cx));
        draw(cx);
        assert!(cx.debug_bounds("github-cancel").is_some());
        assert!(
            !cx.update(|window, _| window.context_stack().is_empty()),
            "the Cancel holds focus, drawn"
        );

        drop(held);
        settle_panel(&app, cx).await;
        draw(cx);
        assert!(
            cx.debug_bounds("github-cancel").is_none(),
            "the reply takes the busy row away"
        );
        let (destination, text) = cx.read(|cx| {
            let field = panel.read(cx).destination.read(cx);
            (field.focus_handle(cx), field.input_bounds())
        });
        assert!(
            cx.update(|window, _| destination.is_focused(window)),
            "focus moves from the Cancel to the destination field"
        );
        assert!(
            !cx.update(|window, _| window.context_stack().is_empty()),
            "the frame draws the field that holds focus"
        );
        // The field's own accent edge shows its focus whole. The kit's ring
        // outside it is cut at the panel's left edge, whose scroll clip keeps
        // no ring room, as it is when the panel opens with focus there.
        let accent = cx.read(|cx| gpui_kit::component::Theme::global(cx).ring);
        let edges: Vec<_> = focus_ring::painted_outlines(cx, accent)
            .into_iter()
            .filter(|edge| edge.bounds.contains(&text.center()))
            .collect();
        assert!(
            matches!(edges[..], [edge] if edge.whole && edge.width == px(1.)),
            "the destination field draws its accent edge whole: {edges:?}"
        );

        escape(cx);
        assert!(
            !cx.update(|window, cx| window.has_active_dialog(cx)),
            "Escape closes the panel"
        );
    }

    /// Each PR row is one text column: the title, a Button's label, and the
    /// context line below the Button start at the Button's label inset and
    /// end at the same edge, whatever the title's length, and each keeps one
    /// line, cut with an ellipsis, in a wide and a narrow window.
    #[gpui::test]
    async fn pull_request_rows_share_one_text_column(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, app, cx) = repository_window(cx);
        let repo = cx.read(|cx| app.read(cx).repository.clone().unwrap());
        let panel = cx.update(|window, cx| {
            cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx))
        });
        app.read_with(cx, |app, _| app.operations.drain());
        cx.run_until_parked();
        let short = github::review::fixture_pull(false);
        let mut long = short.clone();
        long.number = 43;
        long.title = "Keep every captured review position through a repository change ".repeat(6);
        long.head.branch = format!("feature/{}", "a-long-topic-branch-name-".repeat(6));
        long.user.login = "a-reviewer-with-a-long-account-name".into();
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.rows = vec![short, long];
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        settle_panel(&app, cx).await;
        let inset = appearance::ui_size(crate::LABEL_INSET);
        for width in [1480., 640.] {
            cx.simulate_resize(size(px(width), px(980.)));
            draw(cx);
            let lines = |cx: &mut VisualTestContext, number: u64| {
                let mut bounds = |selector: String| {
                    cx.debug_bounds(selector.clone().leak())
                        .unwrap_or_else(|| panic!("{selector} at {width} px"))
                };
                (
                    bounds(format!("github-pr-{number}")),
                    bounds(format!("github-pr-title-{number}")),
                    bounds(format!("github-pr-context-{number}")),
                )
            };
            let (_, short_title, short_context) = lines(cx, 42);
            for number in [42, 43] {
                let (button, title, context) = lines(cx, number);
                let at = format!("#{number} at {width} px: title {title:?}, context {context:?}");
                assert!(
                    (title.left() - (button.left() + inset)).abs() < px(0.01),
                    "{at}"
                );
                assert!((title.left() - context.left()).abs() < px(0.01), "{at}");
                assert!((title.right() - context.right()).abs() < px(0.01), "{at}");
                assert!(context.right() <= button.right() - inset + px(0.01), "{at}");
                assert_eq!(title.size.height, short_title.size.height, "{at}: one line");
                assert_eq!(
                    context.size.height, short_context.size.height,
                    "{at}: one line"
                );
            }
        }
        cx.update(|window, cx| window.close_dialog(cx));
    }

    /// In a 1480 × 980 window, as the native review shows the panel, at 13
    /// and 18 pt, Tab reaches the controls along the panel's sides and each
    /// draws its focus ring whole inside every clip around it (the focus
    /// sweep, `focus_ring::sweep`): the PR list's full-width rows, whose rings
    /// the list cut at its own edges, Recover drafts at the left and Comment
    /// on selection at the right, with the body's scrollbar gutter and
    /// without it.
    #[gpui::test]
    async fn the_panel_keeps_its_edge_controls_rings_inside_its_clips(cx: &mut TestAppContext) {
        use crate::focus_ring::sweep;
        let (_fixture, app, cx) = repository_window(cx);
        sweep::prepare(&app, cx);
        let repo = cx.read(|cx| app.read(cx).repository.clone().unwrap());
        let panel = cx.update(|window, cx| {
            cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx))
        });
        app.read_with(cx, |app, _| app.operations.drain());
        cx.run_until_parked();
        let pull = github::review::fixture_pull(false);
        let mut other = pull.clone();
        other.number = 43;
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.rows = vec![pull.clone(), other];
                panel.review.fixture = true;
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        settle_panel(&app, cx).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured, 1, &OperationControl::default())
            .unwrap()
            .items;
        let scroll = cx.read(|cx| panel.read(cx).review.body_scroll.clone());
        let mut failures = Vec::new();
        let mut gutters = Vec::new();
        for interface in sweep::SIZES {
            sweep::interface_size(cx, interface);
            for (view, controls) in [
                (
                    "list",
                    &["github-pr-42", "github-pr-43", "github-recovery-toggle"][..],
                ),
                (
                    "Files",
                    &["github-inline-start", "github-recovery-toggle"][..],
                ),
            ] {
                cx.update(|window, cx| {
                    panel.update(cx, |panel, cx| {
                        if view == "Files" {
                            panel.pull = Some(pull.clone());
                            panel.review.files = files.clone();
                            panel.review.section = Section::Files;
                            panel.review.show_list = false;
                            panel.select_review_file(0, window, cx);
                        } else {
                            panel.pull = None;
                            panel.review.show_list = true;
                        }
                        panel.focus_visible_section(window, cx);
                        cx.notify();
                    })
                });
                settle_panel(&app, cx).await;
                let surface = format!("the GitHub panel's {view}");
                let rings = sweep::walk(&app, cx, &surface, interface, &mut failures);
                gutters.push(scroll.max_offset().y > px(0.));
                // Each control's ring, as the walk saw it, lies outside its edge.
                for &control in controls {
                    let bounds = cx
                        .debug_bounds(control)
                        .unwrap_or_else(|| panic!("{control} in {surface} at {interface} pt"));
                    let ringed = rings.iter().any(|ring| {
                        ring.size.width - bounds.size.width >= px(5.99)
                            && ring.size.height - bounds.size.height >= px(5.99)
                            && (ring.center().x - bounds.center().x).abs() < px(0.5)
                    });
                    assert!(
                        ringed,
                        "Tab rings {control} in {surface} at {interface} pt: {bounds:?} among \
                         {rings:?}"
                    );
                }
            }
        }
        sweep::assert_whole(failures);
        assert!(
            gutters.contains(&true) && gutters.contains(&false),
            "the body overflows in some views and fits in others: {gutters:?}"
        );
    }

    /// The panel's text starts at one edge in a 1480 × 980 window at 13 and
    /// 18 pt: the dialog's title, the fixture banner, a notice and each PR
    /// row's two lines start where the pager's and Recover drafts' Buttons,
    /// at the panel's edge, start their labels. They started at four: the
    /// title at the edge, the banner 6.5 px in, the Buttons' labels 10 px and
    /// the PR rows' text 20.75 px, inside their cards. In a PR's Files, the
    /// file-page bound below the files' pager starts there too, a phrase to a
    /// line inside the file list's column; it started at the edge. The column
    /// beside the file list starts its own text one inset in from its edge,
    /// where the Changes button starts its label: the prompt before a file is
    /// chosen, then the file's path and the range hint, with the patch box at
    /// the edge. The path and hint started at the edge, the prompt 16 px in.
    /// The draft status beside the section tabs ends one inset in from the
    /// panel's right edge, where the Buttons there end their labels.
    #[gpui::test]
    async fn the_panel_starts_its_text_at_one_edge(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, app, cx) = repository_window(cx);
        let repo = cx.read(|cx| app.read(cx).repository.clone().unwrap());
        let panel = cx.update(|window, cx| {
            cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx))
        });
        app.read_with(cx, |app, _| app.operations.drain());
        cx.run_until_parked();
        let pull = github::review::fixture_pull(false);
        let mut other = pull.clone();
        other.number = 43;
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.rows = vec![pull, other];
                panel.review.fixture = true;
                panel.notice = Some("Fixture PRs refreshed.".into());
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        settle_panel(&app, cx).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        for interface in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            draw(cx);
            let at = |cx: &mut VisualTestContext, selector: &'static str| {
                cx.debug_bounds(selector)
                    .unwrap_or_else(|| panic!("{selector} at {interface} pt"))
            };
            let edge = at(cx, "github-panel-content").left();
            let text = edge + text_inset();
            let starts = [
                ("title", at(cx, "github-title").left()),
                ("banner", at(cx, "github-fixture-mode").left()),
                ("notice", at(cx, "github-notice").left()),
                ("PR title", at(cx, "github-pr-title-42").left()),
                ("PR context", at(cx, "github-pr-context-42").left()),
                // A Button at the edge starts its label the inset in.
                ("pager", at(cx, "github-previous").left() + text_inset()),
                (
                    "drafts",
                    at(cx, "github-recovery-toggle").left() + text_inset(),
                ),
            ];
            // Within the device pixel layout puts an edge on: at 18 pt the inset is 13.85 px.
            for (name, start) in starts {
                assert!(
                    (start - text).abs() < px(0.5),
                    "{interface} pt: {name} starts at {start:?}, not {text:?}: {starts:?}"
                );
            }
        }
        // The first PR's Files section, before a file is chosen and then with its first file in
        // Changes.
        let pull = github::review::fixture_pull(false);
        let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured, 1, &OperationControl::default())
            .unwrap()
            .items;
        cx.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                panel.pull = Some(pull);
                panel.review.files = files;
                panel.review.section = Section::Files;
                panel.review.show_list = false;
                panel.save_status = "Draft saved locally".into();
                cx.notify();
            })
        });
        settle_panel(&app, cx).await;
        for (chosen, interface) in [(false, 13), (false, 18), (true, 13), (true, 18)] {
            if chosen && interface == 13 {
                cx.update(|window, cx| {
                    panel.update(cx, |panel, cx| panel.select_review_file(0, window, cx))
                });
                settle_panel(&app, cx).await;
            }
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            draw(cx);
            let at = |cx: &mut VisualTestContext, selector: &'static str| {
                cx.debug_bounds(selector)
                    .unwrap_or_else(|| panic!("{selector} at {interface} pt in Files"))
            };
            // The column beside the file list starts its text where the mode buttons at its
            // edge start their labels, and its boxes at that edge.
            let column = at(cx, "github-file-column").left();
            let starts = if chosen {
                vec![
                    ("path", at(cx, "github-file-title").left()),
                    (
                        "Changes",
                        at(cx, "github-file-mode-0").left() + text_inset(),
                    ),
                    ("range hint", at(cx, "github-range-hint").left()),
                    ("patch box", at(cx, "github-patch").left() + text_inset()),
                ]
            } else {
                vec![("choose a file", at(cx, "github-choose-file").left())]
            };
            for (name, start) in &starts {
                assert!(
                    (*start - (column + text_inset())).abs() < px(0.5),
                    "{interface} pt, Files' column at {column:?}: {name} starts at {start:?}: \
                     {starts:?}"
                );
            }
            // The draft status ends where the Buttons at the panel's right edge end their
            // labels.
            let end = at(cx, "github-panel-content").right() - text_inset();
            let saved = at(cx, "github-local-save").right();
            assert!(
                (saved - end).abs() < px(0.5),
                "{interface} pt: the draft status ends at {saved:?}, not {end:?}"
            );
            let text = at(cx, "github-panel-content").left() + text_inset();
            let list = at(cx, "github-file-focus");
            let bound = at(cx, "github-file-bound");
            let starts = [
                ("title", at(cx, "github-title").left()),
                (
                    "pager",
                    at(cx, "github-files-previous").left() + text_inset(),
                ),
                ("file bound", bound.left()),
            ];
            for (name, start) in starts {
                assert!(
                    (start - text).abs() < px(0.5),
                    "{interface} pt, Files: {name} starts at {start:?}, not {text:?}: {starts:?}"
                );
            }
            // Below the file list, across the panel, the bound is one line as tall as the
            // page number's, with its separator: in the list's column it took two.
            let line = at(cx, "github-files-page").size.height;
            assert!(
                bound.top() >= list.bottom() && (bound.size.height - line).abs() < px(0.01),
                "{interface} pt: the file bound {bound:?} is not one line {line:?} below the \
                 file list {list:?}"
            );
            let (first, last) = (at(cx, "github-file-bound-0"), at(cx, "github-file-bound-1"));
            assert!(
                first.top() == last.top() && first.left() == bound.left(),
                "{interface} pt: the bound's phrases {first:?} and {last:?} share no line"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        cx.update(|window, cx| window.close_dialog(cx));
    }

    /// The local draft status at the end of a PR's section tabs ends at the panel's label
    /// inset whatever its length, and gives way to the tabs rather than run past the panel:
    /// where it fits beside them whole it stays on their line, centred on it; where it does
    /// not, it takes a line of its own below them inside the panel's text column, wrapping
    /// there, so its whole text, the failed save's remedy included, stays drawn. The tabs
    /// keep their place. At 13 and 18 pt in a 1480 × 980 and a 1000 × 700 window, with
    /// "Draft saved locally", the failed save's status and the unsaved draft's longer one:
    /// at 18 pt in 1480 × 980 the unsaved draft's status was 1,067 px wide on one line and
    /// ran past the window.
    #[gpui::test]
    async fn the_draft_status_gives_way_to_the_tabs(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, app, cx) = repository_window(cx);
        let repo = cx.read(|cx| app.read(cx).repository.clone().unwrap());
        let panel = cx.update(|window, cx| {
            cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx))
        });
        app.read_with(cx, |app, _| app.operations.drain());
        cx.run_until_parked();
        let pull = github::review::fixture_pull(false);
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.rows = vec![pull.clone()];
                panel.review.fixture = true;
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        settle_panel(&app, cx).await;
        cx.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                panel.pull = Some(pull);
                panel.review.section = Section::Overview;
                panel.review.show_list = false;
                cx.notify();
            })
        });
        settle_panel(&app, cx).await;
        let saved = "Draft saved locally";
        let failed = "Draft save failed — keep this window open or copy the text";
        let unsaved = DraftIssue::Selection.save_message();
        for (interface, width, height) in [
            (13, 1480., 980.),
            (18, 1480., 980.),
            (13, 1000., 700.),
            (18, 1000., 700.),
        ] {
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            let mut tabs = None;
            for status in [saved, failed, unsaved] {
                cx.update(|_, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.save_status = status.into();
                        cx.notify();
                    })
                });
                draw(cx);
                let at = format!("{interface} pt in {width} × {height}, {status:?}");
                let bounds = |cx: &mut VisualTestContext, selector: &'static str| {
                    cx.debug_bounds(selector)
                        .unwrap_or_else(|| panic!("{at}: {selector}"))
                };
                let edge = bounds(cx, "github-panel-content");
                let row = ["github-section-0", "github-section-1", "github-section-2"]
                    .map(|selector| bounds(cx, selector));
                let shown = bounds(cx, "github-local-save");
                let (line, whole) = cx.update(|window, _| {
                    let size = appearance::ui_text(12.);
                    (
                        window
                            .text_style()
                            .line_height
                            .to_pixels(size.into(), window.rem_size()),
                        crate::views::shaped_width(
                            status,
                            size,
                            window.text_style().font_weight,
                            window,
                        ),
                    )
                });
                // The tabs stay where the short status leaves them in the panel, which the
                // dialog places.
                let placed = row.map(|tab| tab.origin - edge.origin);
                assert_eq!(*tabs.get_or_insert(placed), placed, "{at}: the tabs moved");
                let slack = px(0.5);
                let (start, end) = (edge.left() + text_inset(), edge.right() - text_inset());
                let last = row[2];
                if shown.top() < last.bottom() {
                    // Beside the tabs, whole on one line centred on theirs, ending where the
                    // Buttons at the panel's right edge end their labels.
                    assert!(
                        shown.left() >= last.right()
                            && (shown.right() - end).abs() < slack
                            && shown.size.width + slack >= whole
                            && shown.size.height <= line + slack
                            && (shown.center().y - last.center().y).abs() < slack,
                        "{at}: {shown:?} ({whole:?} on one line) beside {last:?}, ending at \
                         {end:?}"
                    );
                } else {
                    // Below them, from the panel's text inset to at most that end, its lines
                    // holding the whole text: one line as wide as the text, or several that
                    // span the column.
                    let lines = (shown.size.height / line).round();
                    let holds = if lines == 1. {
                        shown.size.width + slack >= whole
                    } else {
                        (shown.right() - end).abs() < slack && shown.size.width * lines >= whole
                    };
                    assert!(
                        status != saved
                            && shown.top() >= last.bottom()
                            && (shown.left() - start).abs() < slack
                            && shown.right() <= end + slack
                            && lines >= 1.
                            && (shown.size.height - line * lines).abs() < slack
                            && holds,
                        "{at}: {shown:?} ({whole:?}) below {last:?} from {start:?} to {end:?}, \
                         {lines} lines of {line:?}"
                    );
                }
                // The unsaved draft's status, 1,067 px on one line at 18 pt, wraps below the
                // tabs inside the 1480 px window.
                if interface == 18 && status == unsaved {
                    assert!(
                        shown.top() >= last.bottom() && shown.size.height > line * 1.5,
                        "{at}: {shown:?} wraps below {last:?}"
                    );
                }
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        cx.update(|window, cx| window.close_dialog(cx));
    }

    /// Review's draft status gives way to Review submission… as the status beside the tabs
    /// gives way to them: where it fits beside the button whole it stays on its line, from
    /// the panel's text inset; where it does not, it takes a line of its own below the
    /// button, wrapping inside the panel's text column, and the button keeps the panel's
    /// right edge. At 18 pt in a 1280 × 800 window the unsaved draft's status pushed the
    /// button past the panel's right edge, where Tab focused it with nothing drawn. At 13
    /// and 18 pt in a 1480 × 980 and a 1000 × 700 window, with "Draft saved locally", the
    /// failed save's status and the unsaved draft's longer one.
    #[gpui::test]
    async fn the_draft_status_gives_way_to_review_submission(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, app, cx) = repository_window(cx);
        let repo = cx.read(|cx| app.read(cx).repository.clone().unwrap());
        let panel = cx.update(|window, cx| {
            cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx))
        });
        app.read_with(cx, |app, _| app.operations.drain());
        cx.run_until_parked();
        let pull = github::review::fixture_pull(false);
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.rows = vec![pull.clone()];
                panel.review.fixture = true;
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        settle_panel(&app, cx).await;
        cx.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                panel.pull = Some(pull);
                panel.review.section = Section::Review;
                panel.review.show_list = false;
                cx.notify();
            })
        });
        settle_panel(&app, cx).await;
        let saved = "Draft saved locally";
        let failed = "Draft save failed — keep this window open or copy the text";
        let unsaved = DraftIssue::Selection.save_message();
        let mut wrapped = false;
        for (interface, width, height) in [
            (13, 1480., 980.),
            (18, 1480., 980.),
            (13, 1000., 700.),
            (18, 1000., 700.),
        ] {
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            for status in [saved, failed, unsaved] {
                cx.update(|_, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.save_status = status.into();
                        cx.notify();
                    })
                });
                draw(cx);
                let at = format!("{interface} pt in {width} × {height}, {status:?}");
                let bounds = |cx: &mut VisualTestContext, selector: &'static str| {
                    cx.debug_bounds(selector)
                        .unwrap_or_else(|| panic!("{at}: {selector}"))
                };
                let edge = bounds(cx, "github-panel-content");
                let submit = bounds(cx, "github-review-submit");
                let shown = bounds(cx, "github-draft-status");
                let (line, whole) = cx.update(|window, _| {
                    let size = appearance::ui_text(12.);
                    (
                        window
                            .text_style()
                            .line_height
                            .to_pixels(size.into(), window.rem_size()),
                        crate::views::shaped_width(
                            status,
                            size,
                            window.text_style().font_weight,
                            window,
                        ),
                    )
                });
                let slack = px(0.5);
                let (start, end) = (edge.left() + text_inset(), edge.right() - text_inset());
                // The button keeps the panel's right edge.
                assert!(
                    (submit.right() - edge.right()).abs() < slack,
                    "{at}: Review submission… {submit:?} ends at the panel's edge {edge:?}"
                );
                assert!(
                    (shown.left() - start).abs() < slack && shown.right() <= end + slack,
                    "{at}: the status {shown:?} lies from {start:?} to at most {end:?}"
                );
                let lines = (shown.size.height / line).round();
                if shown.top() < submit.bottom() {
                    // Beside the button, whole on one line centred on it.
                    assert!(
                        shown.right() <= submit.left()
                            && lines == 1.
                            && shown.size.width + slack >= whole
                            && (shown.center().y - submit.center().y).abs() < slack,
                        "{at}: {shown:?} ({whole:?} on one line) beside {submit:?}"
                    );
                } else {
                    // Below it, its lines holding the whole text.
                    wrapped = true;
                    assert!(
                        status != saved
                            && (shown.size.height - line * lines).abs() < slack
                            && shown.size.width * lines + slack >= whole,
                        "{at}: {shown:?} ({whole:?}) below {submit:?} in {lines} lines of \
                         {line:?}"
                    );
                }
            }
        }
        assert!(wrapped, "a long status takes a line of its own somewhere");
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        cx.update(|window, cx| window.close_dialog(cx));
    }

    /// A PR's Overview keeps its rows inside the panel at 13 and 18 pt in a 461 and a
    /// 1,480 px window, below the body's fold too, where the width walk sees nothing drawn
    /// (`width_walk`): a row that cannot hold its label beside its Button wraps the label
    /// below it, at the panel's text inset. The PR title beside Pull requests, the PR
    /// pager's page and the drafts policy beside Recover drafts ran past a narrow panel on
    /// one line, as the Conversations heading did past its pane at 18 pt.
    #[gpui::test]
    async fn a_prs_overview_wraps_its_rows_inside_a_narrow_panel(cx: &mut TestAppContext) {
        use crate::width_walk::{Page, frame, open};
        let (_fixture, _app, cx, _) = open(Page::GitHub, cx).await;
        let mut wrapped = Vec::new();
        for (interface, width) in [(13, 461.), (18, 461.), (13, 1480.), (18, 1480.)] {
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            cx.simulate_resize(size(px(width), px(980.)));
            for _ in 0..3 {
                frame(cx);
            }
            let at = format!("{interface} pt in {width} px");
            let bounds = |cx: &mut VisualTestContext, selector: &'static str| {
                cx.debug_bounds(selector)
                    .unwrap_or_else(|| panic!("{at}: {selector}"))
            };
            let edge = bounds(cx, "github-panel-content");
            let slack = px(0.5);
            for (button, text) in [
                ("github-back-list", "github-pull-title"),
                ("github-previous", "github-page"),
                ("github-recovery-toggle", "github-passive-policy"),
            ] {
                let (button, text) = (bounds(cx, button), bounds(cx, text));
                assert!(
                    text.right() <= edge.right() + slack && button.right() <= edge.right() + slack,
                    "{at}: {button:?} and {text:?} lie inside the panel {edge:?}"
                );
                if text.top() >= button.bottom() {
                    // Below its Button, at the panel's text inset.
                    wrapped.push(at.clone());
                    assert!(
                        (text.left() - (edge.left() + text_inset())).abs() < slack,
                        "{at}: {text:?} below {button:?} starts at the panel's text inset"
                    );
                } else {
                    // Beside it, where the Button's gap puts it.
                    assert!(
                        text.left() >= button.right(),
                        "{at}: {text:?} beside {button:?}"
                    );
                }
            }
            let pane = bounds(cx, "github-thread-section");
            let heading = bounds(cx, "github-conversations-heading");
            assert!(
                heading.right() <= pane.right() + slack,
                "{at}: the Conversations heading {heading:?} lies inside its pane {pane:?}"
            );
        }
        assert!(
            !wrapped.is_empty(),
            "a narrow panel wraps a label below its Button"
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The panel's body cues what lies below its fold with the shared always-visible
    /// scrollbar (`tags::ScrollCue`) only while it overflows: a PR's Files in a 1000 × 680
    /// window overflow at 13 and 18 pt, and the thumb follows the body's offset in a gutter
    /// clear of the rings of the controls at its right; there the content narrows by that
    /// gutter, the track and the ring's room. The PR list with none loaded in a 1480 × 1200
    /// window fits: the body draws no thumb and its content keeps the body's width less the
    /// ring's room on either side.
    #[gpui::test]
    async fn the_panel_cues_its_fold_only_while_its_body_overflows(cx: &mut TestAppContext) {
        use crate::tags::tests::{assert_no_scroll_cue, assert_scroll_cue, draw};
        let (_fixture, app, cx) = repository_window(cx);
        // The dialog opens at rest, so the thumb is read where the track was measured.
        cx.update(|_, cx| {
            appearance::ThemeChoice::Midnight.apply(None, cx);
            cx.set_reduce_motion(true);
        });
        let repo = cx.read(|cx| app.read(cx).repository.clone().unwrap());
        let panel = cx.update(|window, cx| {
            cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx))
        });
        app.read_with(cx, |app, _| app.operations.drain());
        cx.run_until_parked();
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| panel.close(window, cx));
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        settle_panel(&app, cx).await;
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        let scroll = cx.read(|cx| panel.read(cx).review.body_scroll.clone());
        cx.simulate_resize(size(px(1480.), px(1200.)));
        draw(cx);
        assert_no_scroll_cue(cx, &app, "github-panel", room, &["github-panel-content"]);

        let pull = github::review::fixture_pull(false);
        let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured, 1, &OperationControl::default())
            .unwrap()
            .items;
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.rows = vec![pull.clone()];
                panel.pull = Some(pull);
                panel.review.files = files;
                panel.review.section = Section::Files;
                panel.review.show_list = false;
                panel.select_review_file(0, window, cx);
            })
        });
        settle_panel(&app, cx).await;
        cx.simulate_resize(size(px(1000.), px(680.)));
        draw(cx);
        let controls = [
            "github-panel-content",
            "github-inline-start",
            "github-account-options",
        ];
        assert_scroll_cue(cx, &app, &scroll, "github-panel", room, &controls);
        // The content gives the gutter up: the track and the ring's room beside it.
        let ring = appearance::BUTTON_FOCUS_RING;
        let gutter = gpui_kit::component::scroll::Scrollbar::width() + ring.gap + ring.width;
        let (body, content) = (
            cx.debug_bounds("github-panel").expect("the body"),
            cx.debug_bounds("github-panel-content")
                .expect("its content"),
        );
        assert!(
            (body.right() - room - gutter - content.right()).abs() < px(0.01)
                && (content.left() - (body.left() + room)).abs() < px(0.01),
            "the content {content:?} keeps the ring's room and the gutter {gutter:?} inside the \
             body {body:?}"
        );
        cx.update(|window, cx| window.close_dialog(cx));
    }

    /// Escape, the frame after the dialog closes, and the app's repair of
    /// modal focus 300 ms later.
    fn escape(cx: &mut VisualTestContext) {
        cx.simulate_keystrokes("escape");
        crate::tags::tests::draw(cx);
        crate::copy_feedback::tests::wait(cx, Duration::from_millis(400));
        crate::tags::tests::draw(cx);
    }

    /// The last frame's quads with their bounds in logical pixels.
    fn quads(cx: &mut VisualTestContext) -> Vec<(Bounds<Pixels>, gpui_kit::Quad)> {
        cx.update(|window, _| {
            let scale = window.scale_factor();
            window
                .painted_quads()
                .into_iter()
                .map(|quad| {
                    let b = quad.bounds;
                    let bounds = Bounds::from_corners(
                        point(px(b.left().as_f32() / scale), px(b.top().as_f32() / scale)),
                        point(
                            px(b.right().as_f32() / scale),
                            px(b.bottom().as_f32() / scale),
                        ),
                    );
                    (bounds, quad)
                })
                .collect()
        })
    }

    fn near(a: Bounds<Pixels>, b: Bounds<Pixels>) -> bool {
        [
            (a.left(), b.left()),
            (a.top(), b.top()),
            (a.right(), b.right()),
            (a.bottom(), b.bottom()),
        ]
        .into_iter()
        .all(|(a, b)| (a - b).abs() < px(0.01))
    }

    /// The window painted the installed Button focus ring around `selector`.
    fn ringed(cx: &mut VisualTestContext, selector: &'static str) -> bool {
        let control = cx.debug_bounds(selector).expect(selector);
        let (ring, color, scale) = cx.update(|window, cx| {
            let theme = gpui_kit::component::Theme::global(cx);
            let ring = theme.button_focus_ring;
            (ring, theme.ring.alpha(ring.opacity), window.scale_factor())
        });
        assert_eq!(ring, appearance::BUTTON_FOCUS_RING);
        let around = control.dilate(ring.gap + ring.width);
        quads(cx).into_iter().any(|(bounds, quad)| {
            quad.background.is_transparent()
                && quad.border_color == color
                && quad.border_widths.top.as_f32() == ring.width.as_f32() * scale
                && near(bounds, around)
        })
    }

    /// `selector` painted its 1 px keyboard-focus outline in the accent.
    fn outlined(cx: &mut VisualTestContext, selector: &'static str) -> bool {
        let pane = cx.debug_bounds(selector).expect(selector);
        let (accent, scale) =
            cx.update(|window, cx| (Hsla::from(rgb(palette(cx).accent)), window.scale_factor()));
        quads(cx).into_iter().any(|(bounds, quad)| {
            quad.border_color == accent
                && quad.border_widths.top.as_f32() == scale
                && near(bounds, pane)
        })
    }

    /// The panel opened from the active repository's update, as command
    /// palette activation opens it, with its local reads drained.
    async fn open_panel(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            // This is the same parent-entity update boundary as command-palette
            // activation. The previous constructor synchronously reborrowed it.
            app.update(cx, |app, cx| app.open_github(window, cx));
            assert!(window.has_active_dialog(cx));
        });
        settle_panel(app, cx).await;
    }

    /// The open panel with its local reads drained.
    async fn settle_panel(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        let (operations_done, preferences_done) = app.read_with(cx, |app, _| {
            (
                app.operations.submit_read(|| Ok(())),
                app.preferences_writer.submit_read(|| Ok(())),
            )
        });
        operations_done.await.unwrap().unwrap();
        preferences_done.await.unwrap().unwrap();
        cx.executor().run_until_parked();
    }

    /// Asserts that the [`phrases`] label `selector`, drawing `text` at `size`,
    /// breaks its lines only between phrases, and returns how many it drew.
    /// Each line the last frame drew there is as wide as a run of whole
    /// phrases shaped in one line, give or take a separator's trailing space.
    /// The window's root marks every text run with `width_walk::text_mark`.
    pub(super) fn assert_phrases_whole(
        cx: &mut VisualTestContext,
        selector: &'static str,
        text: &str,
        size: Pixels,
        at: &str,
    ) -> usize {
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{at}: {selector}"));
        let mark = gpui_kit::Background::from(crate::width_walk::text_mark());
        let (scale, quads) = cx.update(|window, _| (window.scale_factor(), window.painted_quads()));
        let logical = |value: gpui_kit::ScaledPixels| px(value.0 / scale);
        // Each line's top, left and right.
        let mut lines: Vec<(Pixels, Pixels, Pixels)> = Vec::new();
        for quad in quads.iter().filter(|quad| quad.background == mark) {
            let (left, top) = (logical(quad.bounds.left()), logical(quad.bounds.top()));
            let (right, bottom) = (logical(quad.bounds.right()), logical(quad.bounds.bottom()));
            if left < bounds.left() - px(0.5)
                || right > bounds.right() + px(0.5)
                || top < bounds.top() - px(0.5)
                || bottom > bounds.bottom() + px(0.5)
            {
                continue;
            }
            match lines.iter_mut().find(|line| (line.0 - top).abs() < px(0.5)) {
                Some(line) => {
                    line.1 = line.1.min(left);
                    line.2 = line.2.max(right);
                }
                None => lines.push((top, left, right)),
            }
        }
        assert!(!lines.is_empty(), "{at}: {selector} drew no text");
        let parts: Vec<&str> = text.split(" · ").collect();
        let runs: Vec<(String, Pixels)> = cx.update(|window, _| {
            let mut runs = Vec::new();
            for start in 0..parts.len() {
                for end in start..parts.len() {
                    let mut run = parts[start..=end].join(" · ");
                    if end + 1 < parts.len() {
                        run.push_str(" ·");
                    }
                    for run in [run.clone(), format!("{run} ")] {
                        let width =
                            crate::views::shaped_width(&run, size, FontWeight::NORMAL, window);
                        runs.push((run, width));
                    }
                }
            }
            runs
        });
        // Layout puts each phrase's edges on device pixels, so a line of several phrases can
        // be a pixel or two wider than its run shaped whole; runs that differ by a word differ
        // by far more than this.
        let slack = size / 4.;
        for &(top, left, right) in &lines {
            let width = right - left;
            assert!(
                runs.iter().any(|(_, run)| (width - *run).abs() < slack),
                "{at}: {selector}'s line at {top:?} is {width:?} wide, which no run of whole \
                 phrases is: it breaks inside a phrase. Lines {lines:?}, runs {runs:?}"
            );
        }
        lines.len()
    }

    /// A window on an empty repository, on the Repository page. Keep the
    /// returned directory until the test ends.
    fn repository_window(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, Entity<GitTurtle>, &mut VisualTestContext) {
        // This full app integration test intentionally performs local reads on
        // the real serial executors. GPUI's supported I/O mode accepts their
        // cross-thread wakes; the barriers in `open_panel` drain them before
        // teardown.
        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let result = std::process::Command::new("git")
            .args(["-c", "init.templateDir=", "init", "--initial-branch=main"])
            .arg(fixture.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(result.status.success());
        crate::views::tests::fixture_identity(fixture.path());
        let repo = GitRepository::open(fixture.path()).unwrap();
        let view: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let captured = view.clone();
        crate::views::tests::init_app(cx);
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
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = view.borrow().as_ref().unwrap().clone();
        (fixture, app, cx)
    }
}
