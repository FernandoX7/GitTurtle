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

fn label(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Stateful<Div> {
    let text = text.into();
    div()
        .id(id)
        .role(Role::Label)
        .aria_label(text.clone())
        .child(text)
        .text_size(crate::appearance::ui_text(12.))
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
    pub(super) fn open_github(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation_busy.is_some() || self.page != AppPage::Repository {
            return;
        }
        let Some(repo) = self.repository.clone() else {
            return;
        };
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
                if panel.review.section == Section::Files
                    && panel.review.mode == 0
                    && let Some(index) = panel.review.selected_file
                    && panel.review.files[index].rows.is_empty()
                    && panel.review.files[index].unavailable.is_none()
                {
                    panel.select_review_file(index, window, cx);
                }
                panel.focus_visible_section(window, cx);
            });
        });
        window.open_alert_dialog(cx, move |dialog, window, _| {
            let close = form.clone();
            let cancel = form.clone();
            dialog
                .title("GitHub pull requests")
                .width((window.viewport_size().width - px(48.)).clamp(px(320.), px(1100.)))
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
            body:cx.new(|cx|TextareaState::new(window,cx).rows(5).placeholder("Description or review text — saved locally")),
            rows:vec![],page:1,has_next:false,pull:None,status:None,description:None,create:false,draft:true,event:ReviewEvent::Comment,discussion:true,
            account:None,notice:Some("Offline until you choose Connect or Refresh. GitHub.com is supported; Git authentication and Push keep their existing behavior.".into()),error:None,pending:false,request_generation:0,local_loaded:false,local_loading:false,local_generation:0,local_task:None,writing:false,closed:false,
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
            let _=this.update_in(cx,|this,window,cx|{if generation!=this.request_generation{return;}this.pending=false;this.writing=false;this.control=None;if !this.closed {receive(result,this,window,cx);}cx.notify();});
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
                        this.notice = Some(format!("{notice}. Refresh PRs to open the new pull request."));
                        this.focus_visible_section(window, cx);
                    }
                }
            },
            Err(error) => this.error = Some(format!("{error:#}")),
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
        self.review.cancel_preparation();
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
            let focus = owner.read(cx).app_focus.clone();
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
        div().on_children_prepainted(move|_,window,cx|{*observed.borrow_mut()=review::FocusObservation{focus:window.focused(cx),viewport:scroll.bounds(),rem_size:window.rem_size()};}).id("github-panel").track_scroll(&self.review.body_scroll).min_w_0().max_h((window.viewport_size().height-window.rem_size()*(220. / f32::from(appearance::DEFAULT_INTERFACE_TEXT_SIZE))).clamp(px(120.),px(660.))).overflow_y_scroll().flex().flex_col().gap_2()
            .when(self.review.fixture,|element|element.child(div().flex().flex_wrap().items_center().gap_2().p_2().rounded(px(6.)).bg(rgb(p.subtle))
                .child(label("github-fixture-mode","OFFLINE FIXTURE · native review · no network or credentials").text_color(rgb(p.muted)))
                .child(button("github-fixture-move",if self.review.fixture_moved{"Fixture head moved"}else{"Move fixture head"},"",false).disabled(pending||self.review.fixture_moved).on_click(cx.listener(|this,_,_,cx|{this.review.fixture_moved=true;this.confirm=None;this.notice=Some("Fixture head moved remotely. This view and its draft retain the original commit. Submit to verify stale-head refusal, or Refresh PRs and reopen to inspect the new head.".into());cx.notify();})))))
            .child(div().flex().flex_wrap().items_center().gap_2()
                .child(div().w(appearance::ui_size(280.)).min_w(px(190.)).child(Input::new(&self.destination).aria_label("GitHub destination owner and repository").disabled(pending)))
                .child(button("github-refresh",if self.review.fixture{"Refresh fixture PRs"}else{"Refresh PRs"},"refresh-cw",false).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.refresh(1,window,cx))))
                .child(button("github-create","New pull request","plus",false).disabled(pending||self.review.fixture).on_click(cx.listener(|this,_,window,cx|this.create(window,cx))))
                .child(div().flex_1()).child(button("github-account-options",self.account.as_ref().map(|account|format!("Account · {account}")).unwrap_or_else(||"Connect account".into()),"",self.review.connection_open).toggled(self.review.connection_open).disabled(pending).on_click(cx.listener(|this,_,_,cx|{this.review.connection_open= !this.review.connection_open;cx.notify();}))))
            .when(self.review.connection_open,|element|element.child(div().p_3().border_1().border_color(rgb(p.border)).rounded(px(6.)).flex().flex_col().gap_2()
                .child(label("github-worktree",format!("Local worktree: {}",self.repo.path().display())).text_color(rgb(p.muted)))
                .child(label("github-connection-help","Connect uses your current GitHub CLI account and saves GitTurtle authorization in secure storage. Refresh and PR selection are explicit network reads.").text_color(rgb(p.muted)))
                .child(div().flex().flex_wrap().gap_2()
                    .child(button("github-connect","Connect GitHub CLI account","",false).disabled(pending||self.review.fixture).on_click(cx.listener(|this,_,window,cx|this.connect(window,cx))))
                    .child(button("github-sign-in-help","Copy sign-in command","",false).on_click(|_,_,cx|cx.write_to_clipboard(ClipboardItem::new_string("gh auth login --hostname github.com --web --skip-ssh-key".into()))))
                    .child(button("github-disconnect","Disconnect GitTurtle","",false).disabled(pending||self.review.fixture).on_click(cx.listener(|this,_,window,cx|this.run(false,|_|SecureStore.remove(),|result,this,_,_|match result{Ok(())=>{this.account=None;this.notice=Some("GitTurtle authorization removed. GitHub CLI and ordinary Git authentication retain their settings.".into());},Err(error)=>this.error=Some(error.to_string())},window,cx)))))))
            .when_some(self.notice.as_ref(),|element,notice|element.child(label("github-notice",notice.clone()).role(Role::Status).a11y_synthetic_children(native_accessibility::polite).text_color(rgb(p.muted))))
            .when_some(self.error.as_ref(),|element,error|element.child(label("github-error",error.clone()).role(Role::Alert).a11y_synthetic_children(native_accessibility::assertive).text_color(rgb(p.warning))))
            .when_some(self.draft_issue,|element,issue|element.child(label("github-draft-validation",issue.message()).role(Role::Alert).a11y_synthetic_children(native_accessibility::assertive).text_color(rgb(p.warning))))
            .when(pending,|element|element.child(div().flex().items_center().gap_2()
                .child(label("github-busy",if self.writing{"Sending the captured review…"}else{"Loading your requested PR context…"}).role(Role::Status).a11y_synthetic_children(|builder|{native_accessibility::polite(builder);builder.parent_node().set_busy();}))
                .child(button("github-cancel","Cancel","",false).on_click(cx.listener(|this,_,_,cx|{if let Some(control)=&this.control{control.cancel();}
                    if let Some(control)=&this.local_control{control.cancel();}this.notice=Some(if this.writing{"Cancellation requested. Check GitHub before resubmitting; an accepted action is never replayed."}else{"Cancellation requested. Your review draft is retained."}.into());cx.notify();})))))
            .when(!self.create && (self.review.show_list||self.pull.is_none()),|element|element
                .child(div().id("github-pr-list").max_h(px(200.)).overflow_y_scroll().flex().flex_col().gap_1()
                    .children(self.rows.iter().map(|pull|{let pull=pull.clone();let selected=self.pull.as_ref().is_some_and(|p|p.number==pull.number);let number=pull.number;let name=format!("#{} · {}",pull.number,pull.title);let context=format!("{} · {} → {} · {}",if pull.draft{"Draft"}else{"Open"},pull.head.branch,pull.base.branch,pull.user.login);
                        div().id(("github-pr-card",pull.number)).p_3().rounded(px(7.)).border_1().border_color(rgb(if selected{p.accent}else{p.border})).bg(rgb(p.panel)).flex().flex_col().gap_1()
                            .child(button(("github-pr",pull.number),name,"",selected).w_full().disabled(pending).on_click(cx.listener(move|this,_,window,cx|this.inspect(pull.clone(),window,cx))))
                            .child(label(("github-pr-context",number),context).text_color(rgb(p.muted)))
                    }))
                    .when(self.rows.is_empty(),|element|element.child(div().p_5().flex().flex_col().gap_2().child(label("github-list-empty","Your pull requests, ready to review").font_weight(FontWeight::SEMIBOLD)).child(label("github-list-empty-help","Choose a repository and Refresh PRs to load open requests. Opening this panel reads local settings and drafts only.").text_color(rgb(p.muted))))))
                .child(div().flex().items_center().gap_2().child(button("github-previous","Previous PR page","",false).disabled(pending||self.page<=1).on_click(cx.listener(|this,_,window,cx|this.refresh(this.page-1,window,cx))))
                    .child(label("github-page",format!("Page {} · up to {} PRs",self.page,github::PAGE_SIZE)).text_color(rgb(p.muted)))
                    .child(button("github-next","Next PR page","",false).disabled(pending||!self.has_next).on_click(cx.listener(|this,_,window,cx|this.refresh(this.page+1,window,cx))))))
            .when_some(self.pull.as_ref(),|element,pull|element
                .child(div().flex().flex_col().gap_2()
                    .child(div().flex().items_center().gap_2().child(button("github-back-list",if self.review.show_list{"Hide PR list"}else{"Pull requests"},"arrow-left",false).disabled(pending).on_click(cx.listener(|this,_,window,cx|{this.save_draft(window,cx);this.review.show_list= !this.review.show_list;cx.notify();})))
                        .child(label("github-pull-title",format!("#{} · {}",pull.number,pull.title)).font_weight(FontWeight::SEMIBOLD).text_size(appearance::ui_text(15.))))
                    .child(label("github-branch-context",format!("{} → {} · {}",pull.head.branch,pull.base.branch,if pull.draft{"Draft PR"}else{"Open PR"})).text_color(rgb(p.muted))))
                .child(self.render_tabs(cx))
                .when(self.review.section==Section::Overview,|element|element
                    .child(label("github-captured-identities",format!("Before · {}\nAfter  · {}",pull.base.sha,pull.head.sha)).text_color(rgb(p.muted)))
                    .when_some(self.description.as_ref(),|element,description|element.child(div().h(px(120.)).flex_shrink_0().child(crate::editor_find::Editor::new(description).readonly(true).h_full().aria_label("Pull request description, exact Markdown source"))))
                    .when_some(self.status.as_ref(),|element,status|element.child(div().p_3().bg(rgb(p.subtle)).rounded(px(6.)).flex().flex_col().gap_1()
                        .child(label("github-review-status",format!("Reviews · {}",if status.reviews.is_empty(){"None reported".into()}else{status.reviews.join(" · ")})))
                        .child(label("github-check-status",format!("Checks · {}{}",if status.checks.is_empty(){"None reported".into()}else{status.checks.join(" · ")},if status.incomplete{" · Additional status entries are outside this preview"}else{""})))))
                    .child(self.render_threads(None,cx))
                    .child(button("github-open-changes","Open captured local comparison","",false).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.changes(window,cx)))))
                .when(self.review.section==Section::Files,|element|element.child(self.render_files(cx)))
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
                .when(!self.create,|element|element.child(div().flex().flex_wrap().gap_1()
                    .children([ReviewEvent::Comment,ReviewEvent::Approve,ReviewEvent::RequestChanges].map(|event|button(event.label(),event.label(),"",!self.discussion&&self.event==event).toggled(!self.discussion&&self.event==event).disabled(pending).on_click(cx.listener(move|this,_,window,cx|{this.discussion=false;this.event=event;this.confirm=None;this.save_draft(window,cx);}))))
                    .child(button("github-discussion","Discussion only","",self.discussion).toggled(self.discussion).disabled(pending||!self.review.comments.is_empty()||self.review.composing.is_some()).on_click(cx.listener(|this,_,_,cx|{this.discussion=true;this.confirm=None;cx.notify();})))))
                .child(div().debug_selector(||"github-review-summary".into()).on_children_prepainted(self.reveal_on_focus(self.body.read(cx).focus_handle(cx))).child(Textarea::new(&self.body).h(appearance::ui_size(130.)).aria_label(if self.create{"Pull request description"}else{"Review summary"}).disabled(pending)))
                .child(div().flex().items_center().gap_2().child(label("github-draft-status",self.save_status.clone()).role(Role::Status).a11y_synthetic_children(native_accessibility::polite).text_color(rgb(p.muted)))
                    .child(div().flex_1()).child(button("github-review-submit","Review submission…","",true).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.review_submission(window,cx))))))
            .child(self.render_reply_composer(cx))
            .when_some(self.confirm.as_ref(),|element,confirmation|element.child(div().debug_selector(||"github-confirmation".into()).on_children_prepainted(self.reveal_on_focus(self.conversations.confirm_focus.clone())).track_focus(&self.conversations.confirm_focus).tab_stop(true).p_3().border_1().border_color(rgb(p.accent)).rounded(px(7.)).flex().flex_col().gap_2()
                .child(label("github-confirm-destination",format!("{}\nAccount · {}",confirmation.action.destination(),confirmation.account)).font_weight(FontWeight::SEMIBOLD))
                .child(div().id("github-confirm-text-scroll").max_h(px(180.)).overflow_y_scroll().child(label("github-confirm-text",match &confirmation.action{Action::Create(pull)=>format!("{}\n\n{}",pull.title,pull.body),Action::Comment{body,..}|Action::Reply{body,..}=>body.clone(),Action::SetThreadResolved{resolved,..}=>if *resolved{"Resolve this conversation"}else{"Reopen this conversation"}.into(),Action::Review(review)=>format!("{} review · {} inline comments\n\n{}{}",review.event.label(),review.comments.len(),review.body,review.comments.iter().map(|comment|format!("\n\n{}\n{}",github::review::position_label(comment),comment.body)).collect::<String>())})))
                .child(label("github-confirm-guidance","Send performs one outbound action with these exact captured targets and comments. Inspect the destination before repeating an uncertain attempt.").text_color(rgb(p.muted)))
                .child(div().flex().gap_2().child(button("github-confirm-send",if self.review.fixture{"Send to offline fixture"}else{"Send to GitHub"},"",true).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.submit(window,cx))))
                    .child(button("github-edit-submission","Keep editing","",false).disabled(pending).on_click(cx.listener(|this,_,window,cx|{this.confirm=None;this.focus_visible_section(window,cx);cx.notify();}))))))
            .child(div().flex().items_center().gap_2().child(button("github-recovery-toggle",format!("Recover drafts · {}",self.saved.len()),"",self.review.recovery_open).toggled(self.review.recovery_open).on_click(cx.listener(|this,_,_,cx|{this.review.recovery_open= !this.review.recovery_open;cx.notify();})))
                .child(label("github-passive-policy","Drafts stay on this Mac · outbound actions are explicit").text_color(rgb(p.muted))))
            .when(self.review.recovery_open,|element|element
                .child(label("github-saved-heading","Captured drafts · Copy includes all inline text and original positions; older heads are never remapped").text_color(rgb(p.muted)))
                .child(div().id("github-saved-drafts").max_h(px(160.)).overflow_y_scroll().flex().flex_col().gap_2().children(self.saved.iter().filter(|draft| !matches!(draft,Draft::Reply(reply) if reply.body.is_empty())).take(128).enumerate().map(|(index,draft)|{let draft=draft.clone();let discard=draft.clone();div().flex().flex_col().gap_1().child(label(("github-draft-identity",index),match &draft {Draft::Reply(reply)=>format!("Reply · {} #{} · {} · {}",reply.target.pull.repository.label(),reply.target.pull.number,reply.target.path,reply.target.account),_=>draft.key()}).text_color(rgb(p.muted)))
                    .child(div().flex().gap_2().child(button(("github-copy-draft",index),"Copy complete draft","",false).on_click(move|_,_,cx|cx.write_to_clipboard(ClipboardItem::new_string(draft.recovery_text()))))
                        .child(button(("github-discard-draft",index),"Discard saved draft","",false).disabled(pending).on_click(cx.listener(move|this,_,window,cx|this.discard_draft(discard.clone(),window,cx)))))})))
                .when(!self.attempts.is_empty(),|element|element.child(label("github-attempt-heading","Previous outbound attempts · inspect the destination before repeating"))
                    .child(div().id("github-attempt-list").max_h(px(100.)).overflow_y_scroll().flex().flex_col().gap_1().children(self.attempts.iter().rev().take(10).enumerate().map(|(index,attempt)|label(("github-attempt",index),format!("{}: {}",attempt.destination,attempt.outcome)))))))
    }
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
        // This full app integration test intentionally performs local reads on
        // the real serial executors. GPUI's supported I/O mode accepts their
        // cross-thread wakes; the barriers below drain them before teardown.
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
        let repo = GitRepository::open(fixture.path()).unwrap();
        let view: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let captured = view.clone();
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
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = view.borrow().as_ref().unwrap().clone();
        cx.update(|window, cx| {
            // This is the same parent-entity update boundary as command-palette
            // activation. The previous constructor synchronously reborrowed it.
            app.update(cx, |app, cx| app.open_github(window, cx));
            assert!(window.has_active_dialog(cx));
        });
        let (operations_done, preferences_done) = app.read_with(cx, |app, _| {
            (
                app.operations.submit_read(|| Ok(())),
                app.preferences_writer.submit_read(|| Ok(())),
            )
        });
        operations_done.await.unwrap().unwrap();
        preferences_done.await.unwrap().unwrap();
        cx.executor().run_until_parked();
        cx.update(|window, cx| {
            assert!(window.has_active_dialog(cx));
            window.close_dialog(cx);
            assert!(!window.has_active_dialog(cx));
        });
    }
}
