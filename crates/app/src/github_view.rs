//! Native GitHub collaboration. Repository/PR actions capture their destination;
//! network work exists only behind an explicit button or submit confirmation.
use crate::github::{
    self, Action, Client, ComparisonTarget, NewPull, PullRequest, PullStatus, Repository,
    ReviewDraft, ReviewEvent,
    conversations::CapturedThread,
    drafts::{self, Draft},
    transport::SecureStore,
};
use crate::native_accessibility::LiveRegion as _;
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
    let parts = crate::phrase_runs(&text);
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
    /// Returns once the panel's local read, which [`Self::open_github`]
    /// starts once the dialog's frame is drawn, has landed and been drawn. It
    /// runs on the app's operation executor, a thread `run_until_parked`
    /// cannot wait for: once it drains the read has answered, which holds the
    /// test clock, and the reply lands as the test parks.
    #[cfg(test)]
    pub(crate) fn github_local_read_lands(app: &Entity<Self>, cx: &mut VisualTestContext) {
        crate::views::tests::draw(cx);
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::draw(cx);
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
                // Reopening onto its destination field, its first part, the panel shows its
                // body from the top, the banner above the field included. Closed with the body
                // scrolled down, it reopened there, the field at the body's edge and what lies
                // above it out of view, since the field alone was whole. An open PR or a new
                // one keeps the body where it was, beside what reopening focuses.
                if panel.pull.is_none() && !panel.create {
                    panel.review.body_scroll.set_offset(point(px(0.), px(0.)));
                }
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

    /// Submits a request of the panel over `repository`, of `kind`, on the
    /// operation executor, where it runs as the window's busy spell until it
    /// answers, as a write does, labelled with the kind's busy line: Git
    /// writes and preparations refuse meanwhile and say they wait for the
    /// GitHub request ([`Self::busy_reason`]), its Cancel is the operation
    /// bar's, and a working-state read asked for meanwhile waits for it,
    /// then runs as it ends ([`Self::read_deferred_working`]). That holds
    /// after the panel has closed and dropped its reply, as closing during a
    /// read does, since the executor runs the request until it stops. Such a
    /// read once queued behind a review being sent, with nothing saying why,
    /// for as long as GitHub took to answer. Returns the request's control
    /// and its reply, or, submitting nothing while another write,
    /// preparation or request runs, why it waits.
    fn submit_github_request<T: Send + 'static>(
        &mut self,
        kind: RequestKind,
        repository: std::path::PathBuf,
        operation: impl FnOnce(OperationControl) -> anyhow::Result<T> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<
        (
            OperationControl,
            futures::channel::oneshot::Receiver<anyhow::Result<T>>,
        ),
        &'static str,
    > {
        if self.operation_busy.is_some() {
            return Err(self.busy_reason());
        }
        let label = kind.label();
        let control = self.begin_operation_control(window, cx);
        let worker_control = control.clone();
        let response = self.operations.submit(move || operation(worker_control));
        self.operation_busy = Some(label);
        self.operation_repository = Some(repository);
        let (reply, replied) = futures::channel::oneshot::channel();
        cx.spawn_in(window, async move |this, cx| {
            let result = response
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!(INTERRUPTED)));
            let _ = this.update_in(cx, |this, window, cx| {
                if this.operation_busy == Some(label) {
                    this.operation_busy = None;
                    this.operation_repository = None;
                    this.finish_operation_control(window, cx);
                }
                this.read_deferred_working(window, cx);
                cx.notify();
            });
            let _ = reply.send(result);
        })
        .detach();
        cx.notify();
        Ok((control, replied))
    }
}
#[derive(Clone)]
struct Confirmation {
    action: Action,
    account: String,
}
/// The lines the panel says through the window's announcements ([`Panel::announce`]): its
/// notice, its failure, drawn in its body or in the reply composer, and refused input, with
/// the presses that answered with them.
#[derive(Default, PartialEq)]
struct Heard {
    notice: Option<String>,
    error: Option<String>,
    cancelled: Option<&'static str>,
    issue: Option<DraftIssue>,
    presses: Presses,
}
/// How many presses the panel has answered with its failure line and with its refused-input
/// line, so that a press answered as the one before it was is said again: the lines alone
/// stay the same, and a repeated refusal was silent.
#[derive(Clone, Copy, Default, PartialEq)]
struct Presses {
    error: usize,
    issue: usize,
}
/// The control a request or refusal of the panel started from, where its failure, or that
/// Cancel stopped it, answers ([`Panel::outcome`]): feedback answers on the control the person
/// touched. The panel records it as the request starts or the press is refused, and restores it
/// as the request answers. While a conversation was open, every outcome once answered in the
/// reply's composer, a comparison's cancel from Overview included.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Origin {
    /// The composer of the reply to this conversation: its Review reply… and the Send of its
    /// reply. Its outcome stands in that composer, under the draft, while that composer is
    /// drawn, and otherwise on the panel's top line, never in another conversation's composer:
    /// a reply refused on one conversation once showed under the draft of the next one that
    /// Reply opened.
    Reply(Box<CapturedThread>),
    /// Every other control (Open captured local comparison, Connect, Disconnect, Resolve and
    /// Reopen, Refresh PRs, the templates): its outcome stands on the panel's top line.
    Panel,
}
/// What a request of the panel does, which its busy line names, as the
/// window's operation bar does while it runs as the window's busy spell
/// ([`GitTurtle::submit_github_request`]), and why the controls it refuses
/// wait ([`Self::reason`]). Connecting the account and Disconnect once read
/// "Loading your requested PR context…" there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestKind {
    /// Reads PR context from GitHub: the list, a PR with its checks, files
    /// and conversations, or a submission's preparation.
    Load,
    /// Sends one confirmed outbound action.
    Send,
    /// Connects the GitHub CLI's signed-in account.
    Connect,
    /// Removes GitTurtle's stored authorization.
    Disconnect,
    /// Compares a PR's captured base and head in the local repository, for
    /// its Changes.
    Compare,
    /// Reads a new PR's templates from its base revision in the local
    /// repository.
    Templates,
}
/// `$lead`, then what a Connect stopped part-way may have left, from whether the platform's
/// store holds GitTurtle's authorization ([`SecureStore::HOLDS`]): on a Mac it may already be
/// stored, which Disconnect removes; elsewhere nothing is stored. Linux once said it may
/// already be stored, where nothing can be.
macro_rules! after_stopped_connect {
    ($lead:literal) => {
        if SecureStore::HOLDS {
            concat!(
                $lead,
                " Authorization may already be stored; Disconnect removes it."
            )
        } else {
            concat!(
                $lead,
                " GitTurtle stores no authorization on this platform."
            )
        }
    };
}
/// The connection pane's help: what Connect does, from whether the platform's store holds
/// GitTurtle's authorization ([`SecureStore::HOLDS`]), as a stopped Connect is worded
/// (`after_stopped_connect`): on a Mac it saves the authorization in the store
/// [`SecureStore::NAME`] names; elsewhere the store saves none, so Connect cannot finish
/// there. Linux once said Connect saved it in secure storage, where nothing can be stored.
fn connection_help() -> String {
    let connect = if SecureStore::HOLDS {
        format!(
            "Connect uses your current GitHub CLI account and saves GitTurtle authorization in {}.",
            SecureStore::NAME
        )
    } else {
        "Connect uses your current GitHub CLI account, but GitTurtle stores no authorization on \
         this platform, so Connect cannot finish here yet."
            .to_owned()
    };
    format!("{connect} Refresh and PR selection are explicit network reads.")
}
impl RequestKind {
    pub(crate) const ALL: [Self; 6] = [
        Self::Load,
        Self::Send,
        Self::Connect,
        Self::Disconnect,
        Self::Compare,
        Self::Templates,
    ];
    /// Its busy line.
    fn label(self) -> &'static str {
        match self {
            Self::Load => "Loading your requested PR context…",
            Self::Send => "Sending the captured review…",
            Self::Connect => "Connecting your GitHub account…",
            Self::Disconnect => "Disconnecting GitTurtle from GitHub…",
            Self::Compare => "Comparing the PR's changes locally…",
            Self::Templates => "Reading the PR templates locally…",
        }
    }
    /// Why a control the window's busy spell refuses waits while this runs
    /// as the spell ([`GitTurtle::busy_reason`]): a request to GitHub names
    /// GitHub, and the panel's local work names itself. One reason, "Wait
    /// for the GitHub request to finish", once stood for every kind, though
    /// Disconnect, a PR's Changes and its templates send nothing to GitHub.
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::Load | Self::Send | Self::Connect => "Wait for the GitHub request to finish",
            Self::Disconnect => "Wait for GitTurtle to disconnect from GitHub",
            Self::Compare => "Wait for the pull request comparison to finish",
            Self::Templates => "Wait for the pull request templates to load",
        }
    }
    /// The operation bar's Cancel while this runs as the busy spell
    /// (`GitTurtle::operation_cancel`): its accessible name, which says what
    /// it cancels, and its tooltip, which says what stops and what may
    /// already have happened. It once named "the running Git operation" and
    /// said it stopped Git and its child processes.
    pub(crate) fn cancel(self) -> (&'static str, &'static str) {
        match self {
            Self::Load => (
                "Cancel the running GitHub request",
                "Stop the GitHub request. It only reads, so nothing on GitHub changes.",
            ),
            Self::Send => (
                "Cancel the running GitHub request",
                "Stop the GitHub request. GitHub may already have applied it; check the destination before submitting again.",
            ),
            Self::Connect => (
                "Cancel connecting your GitHub account",
                after_stopped_connect!("Stop connecting."),
            ),
            Self::Disconnect => (
                "Cancel disconnecting GitTurtle from GitHub",
                "Keep GitTurtle's stored authorization, unless its removal has already begun.",
            ),
            Self::Compare => (
                "Cancel the pull request comparison",
                "Stop comparing the PR's changes locally. Nothing changes.",
            ),
            Self::Templates => (
                "Cancel reading the pull request templates",
                "Stop reading the PR templates locally. Nothing changes.",
            ),
        }
    }
    /// What the panel's own Cancel says it asked for while this runs, as the
    /// operation bar's Cancel says in its tooltip ([`Self::cancel`]): what
    /// stops and what may already have happened. It once said "Your review
    /// draft is retained" while Connect, Disconnect or a comparison ran,
    /// which involve no review draft.
    fn requested(self) -> &'static str {
        match self {
            Self::Load => {
                "Cancellation requested. The request only reads; your drafts are retained."
            }
            Self::Send => {
                "Cancellation requested. Check GitHub before resubmitting; an accepted action is never replayed."
            }
            Self::Connect => after_stopped_connect!("Cancellation requested."),
            Self::Disconnect => {
                "Cancellation requested. GitTurtle keeps its stored authorization unless its removal has already begun."
            }
            Self::Compare => {
                "Cancellation requested. Comparing the PR's changes locally changes nothing."
            }
            Self::Templates => {
                "Cancellation requested. Reading the PR templates locally changes nothing."
            }
        }
    }
    /// What the panel says in its failure's place once Cancel stopped this
    /// request before it changed anything ([`Panel::outcome`]), where the
    /// request can tell: a local read stopped by its cancellation ends with
    /// [`gitturtle_core::ReadCancelled`], and Connect and Disconnect, asked
    /// to stop before they reach the account, with [`NotStarted`]. A
    /// cancelled PR comparison once showed the failure telling the person to
    /// Fetch, and a cancelled Disconnect a failure saying the authorization
    /// was kept. A request to GitHub has none: the GitHub CLI's stop reaches
    /// it as a failure, and a sent action's outcome stays uncertain.
    /// `connected` is whether the panel shows a connected account, which a
    /// stopped Connect leaves as it was: it once said no account was
    /// connected while one was.
    fn cancelled(self, connected: bool) -> Option<&'static str> {
        match self {
            Self::Load | Self::Send => None,
            Self::Connect if connected => Some("Cancelled · the account is unchanged"),
            Self::Connect => Some("Cancelled · no account was connected"),
            Self::Disconnect => Some("Cancelled · GitTurtle's authorization was kept"),
            Self::Compare => Some("Cancelled · the PR's changes were not compared"),
            Self::Templates => Some("Cancelled · the PR templates were not read"),
        }
    }
    /// Whether it sends an outbound action, whose outcome may be uncertain
    /// once cancelled: the panel stays open while it runs.
    fn writing(self) -> bool {
        self == Self::Send
    }
}
/// The request of the panel that `label`, the window's busy spell, runs, if
/// it is one.
pub(crate) fn running_request(label: &str) -> Option<RequestKind> {
    RequestKind::ALL
        .into_iter()
        .find(|kind| kind.label() == label)
}
/// Each request's busy line, the window's busy label while it runs.
#[cfg(test)]
pub(crate) fn request_labels() -> [&'static str; RequestKind::ALL.len()] {
    RequestKind::ALL.map(RequestKind::label)
}
/// Whether a request's failure is its cancellation, which stopped it before
/// it changed anything, rather than a failure it met on its own.
fn stopped_by_cancel(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<gitturtle_core::ReadCancelled>()
        .is_some()
        || error.downcast_ref::<NotStarted>().is_some()
}
/// The error a request ends with when Cancel came before it began, so it
/// reached nothing: Connect read no token and Disconnect removed nothing.
#[derive(Debug)]
struct NotStarted;
impl std::fmt::Display for NotStarted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cancelled before it began")
    }
}
impl std::error::Error for NotStarted {}
/// Ends a request with [`NotStarted`] where Cancel came before it began.
fn not_cancelled(control: &OperationControl) -> anyhow::Result<()> {
    if control.is_cancelled() {
        return Err(NotStarted.into());
    }
    Ok(())
}
/// Where Connect and Disconnect reach the account: the GitHub CLI's signed-in account and
/// the platform's secure store ([`github::transport::SecureStore`]).
#[cfg(not(test))]
mod account {
    use crate::github::transport::{CredentialStore, GhTransport, SecureStore};
    use gitturtle_core::OperationControl;
    pub(super) fn connect(control: &OperationControl) -> anyhow::Result<String> {
        GhTransport::connect_existing(control)
    }
    pub(super) fn remove() -> anyhow::Result<()> {
        SecureStore.remove()
    }
}
/// A test build's stand-in for the account, which answers as a signed-in GitHub CLI and an
/// emptied store would, so no test starts the CLI, reads a developer's token or removes their
/// stored authorization (on macOS a Keychain item), even where a cancellation guard
/// regressed: a Disconnect test once reached the real store's removal with nothing but the
/// guard in its way.
#[cfg(test)]
mod account {
    use gitturtle_core::OperationControl;
    /// The account the stand-in connects.
    pub(super) const LOGIN: &str = "stand-in-reviewer";
    pub(super) fn connect(_: &OperationControl) -> anyhow::Result<String> {
        Ok(LOGIN.into())
    }
    pub(super) fn remove() -> anyhow::Result<()> {
        Ok(())
    }
}
/// What a request's reply says when none arrives.
const INTERRUPTED: &str = "GitHub operation was interrupted. Its outcome may be uncertain; check the destination before submitting again.";
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
    /// What the panel last said through the window's announcements ([`Panel::announce`]),
    /// cleared as it closes, so that it says again what it reopens with.
    heard: Heard,
    error: Option<String>,
    /// That Cancel stopped the last request ([`RequestKind::cancelled`]),
    /// said in `error`'s place until the next request.
    cancelled: Option<&'static str>,
    /// Where `error` or `cancelled` answers: the control the request or refusal they answer
    /// started from ([`Origin`]).
    origin: Origin,
    /// The presses answered with `error` and `draft_issue` ([`Presses`]).
    presses: Presses,
    pending: bool,
    /// The busy row's Cancel, which hands focus to the shown section as the
    /// row goes.
    cancel_request: focus_ring::LoadingCancel,
    request_generation: u64,
    local_loaded: bool,
    local_loading: bool,
    local_generation: u64,
    local_task: Option<Task<()>>,
    /// What the request the panel sent last does, which its busy line names
    /// while it runs.
    request: RequestKind,
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
            account:None,notice:Some("Offline until you choose Connect or Refresh. GitHub.com is supported; Git authentication and Push keep their existing behavior.".into()),heard:Heard::default(),error:None,cancelled:None,origin:Origin::Panel,presses:Presses::default(),pending:false,cancel_request:focus_ring::LoadingCancel::new(cx),request_generation:0,local_loaded:false,local_loading:false,local_generation:0,local_task:None,request:RequestKind::Load,closed:false,
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
                            Err(error) => this.local_read_failed(format!("{error:#}")),
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
                            Err(error) => this.local_read_failed(format!("{error:#}")),
                        }
                    }
                    _ => this
                        .local_read_failed("Could not read repository remotes and saved drafts."),
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
    /// Answers a press with `error` on the failure line, said at once however often the same
    /// refusal repeats ([`Presses`]): on the panel's top line ([`Origin::Panel`]).
    fn refuse(&mut self, error: impl Into<String>) {
        self.refuse_at(Origin::Panel, error);
    }
    /// Says that local work no request carried failed, on the panel's top line: the panel's
    /// own read as it opens, which no control asked for, or a Files row's preparation, which
    /// runs beside the requests. A preparation's failure once kept the last request's origin
    /// and stood in an open reply's composer after a refused Review reply….
    fn local_read_failed(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
        self.origin = Origin::Panel;
    }
    /// [`Self::refuse`] for a press on the control `origin`, where the failure line answers.
    fn refuse_at(&mut self, origin: Origin, error: impl Into<String>) {
        self.error = Some(error.into());
        self.origin = origin;
        self.presses.error = self.presses.error.wrapping_add(1);
    }
    /// Whether the failure line answers in the open reply's composer, rather than on the
    /// panel's top line: only for what was asked from that composer, the composer of the
    /// conversation it was asked for ([`Origin`]).
    fn answers_in_reply(&self) -> bool {
        matches!(
            (&self.origin, &self.conversations.active),
            (Origin::Reply(asked), Some(open)) if **asked == *open
        )
    }
    /// Answers a press with `issue` on the refused-input line, said at once however often it
    /// repeats ([`Presses`]).
    fn refuse_input(&mut self, issue: DraftIssue) {
        self.draft_issue = Some(issue);
        self.presses.issue = self.presses.issue.wrapping_add(1);
    }
    /// Sends `operation`, a request of `kind`, as the window's busy spell
    /// ([`GitTurtle::submit_github_request`]) and hands its reply to
    /// `receive`. Returns whether it was sent. Nothing is, and nothing more
    /// is said, while the panel's own request runs, which its busy line
    /// names and whose controls are disabled meanwhile, so no press reaches
    /// here then, or once the panel has closed, with no line to say it in.
    /// Nothing is sent either while the window's busy spell runs a Git
    /// write, a preparation or a request the panel left running as it
    /// closed, and then the panel's failure line says why, in the spell's
    /// own reason ([`GitTurtle::busy_reason`]). Its outcome answers on the panel's top line
    /// ([`Origin::Panel`]).
    fn run<T: Send + 'static>(
        &mut self,
        kind: RequestKind,
        operation: impl FnOnce(OperationControl) -> anyhow::Result<T> + Send + 'static,
        receive: impl FnOnce(anyhow::Result<T>, &mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.run_from(Origin::Panel, kind, operation, receive, window, cx)
    }
    /// [`Self::run`] for a request started from the control `origin`, which it records as it
    /// starts, so its failure, or that Cancel stopped it, answers there whatever the panel
    /// said meanwhile.
    fn run_from<T: Send + 'static>(
        &mut self,
        origin: Origin,
        kind: RequestKind,
        operation: impl FnOnce(OperationControl) -> anyhow::Result<T> + Send + 'static,
        receive: impl FnOnce(anyhow::Result<T>, &mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.pending || self.closed {
            return false;
        }
        let repository = self.repo.path().to_owned();
        let submitted = self.owner.update(cx, |owner, cx| {
            owner.submit_github_request(kind, repository, operation, window, cx)
        });
        let Ok(submitted) = submitted else {
            self.refuse_at(origin, "The repository window is unavailable.");
            return false;
        };
        let (control, response) = match submitted {
            Ok(submitted) => submitted,
            Err(reason) => {
                self.refuse_at(origin, format!("{reason}, then try again."));
                cx.notify();
                return false;
            }
        };
        self.control = Some(control);
        self.request_generation = self.request_generation.wrapping_add(1);
        let generation = self.request_generation;
        self.pending = true;
        self.request = kind;
        self.error = None;
        self.cancelled = None;
        self.origin = origin.clone();
        self.notice = None;
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!(INTERRUPTED)));
            let _ = this.update_in(cx, |this, window, cx| {
                if generation != this.request_generation {
                    return;
                }
                this.pending = false;
                this.control = None;
                // The busy row goes: focus on its Cancel moves to the section
                // the panel shows, as when it opens, before the reply moves it
                // anywhere else.
                if !this.closed && this.cancel_request.holds_focus(window) {
                    this.focus_visible_section(window, cx);
                }
                // It answers where it started, though a refusal meanwhile answered elsewhere.
                this.origin = origin;
                if !this.closed {
                    // Cancel stopped it: the panel says so where a failure
                    // would stand, in place of what asked for the cancel. A
                    // failure the request met before the cancel reached it
                    // ends with its own error and is shown as one.
                    match (result, kind.cancelled(this.account.is_some())) {
                        (Err(error), Some(line)) if stopped_by_cancel(&error) => {
                            this.cancelled = Some(line);
                            this.notice = None;
                        }
                        (result, _) => receive(result, this, window, cx),
                    }
                }
                // `run` cleared the failure line, so one there now is this press's answer.
                if this.error.is_some() {
                    this.presses.error = this.presses.error.wrapping_add(1);
                }
                cx.notify();
            });
        }));
        cx.notify();
        true
    }
    fn connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.review.fixture {
            self.notice=Some("Offline fixture account: offline-reviewer. No credential or network access is available.".into());
            cx.notify();
            return;
        }
        self.run(
            RequestKind::Connect,
            |control| {
                not_cancelled(&control)?;
                account::connect(&control)
            },
            |result, this, _, _| match result {
                Ok(login) => {
                    this.notice = Some(format!(
                        "Connected as {login}; authorization is stored in {}.",
                        SecureStore::NAME
                    ));
                    this.account = Some(login);
                }
                Err(error) => this.error = Some(format!("{error:#}")),
            },
            window,
            cx,
        );
    }
    /// Removes GitTurtle's stored authorization, unless Cancel came before
    /// the removal began: then it keeps the authorization and says so. Its
    /// removal once ignored Cancel.
    fn disconnect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run(
            RequestKind::Disconnect,
            |control| {
                not_cancelled(&control)?;
                account::remove()
            },
            |result, this, _, _| match result {
                Ok(()) => {
                    this.account = None;
                    this.notice = Some("GitTurtle authorization removed. GitHub CLI and ordinary Git authentication retain their settings.".into());
                }
                Err(error) => this.error = Some(error.to_string()),
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
                self.refuse(error.to_string());
                return;
            }
        };
        let fixture = self.review.fixture;
        let moved = self.review.fixture_moved;
        self.run(RequestKind::Load,move|control|{let transport=UiTransport::open(fixture,moved)?;let login=transport.login().to_owned();Ok((Client(transport).list(&repository,page,&control)?,login))},|result,this,_,_|match result {Ok((page,login))=>{this.create=false;this.review.show_list=true;this.confirm=None;this.rows=page.items;this.page=page.page;this.has_next=page.has_next;this.account=Some(login);this.notice=Some("PR list refreshed. Selecting a PR explicitly loads its current review and check status.".into());},Err(error)=>this.error=Some(format!("{error:#}"))},window,cx);
    }
    fn inspect(&mut self, pull: PullRequest, window: &mut Window, cx: &mut Context<Self>) {
        self.save_draft(window, cx);
        let repository = match self.destination(cx) {
            Ok(repo) => repo,
            Err(error) => {
                self.refuse(error.to_string());
                return;
            }
        };
        let fixture = self.review.fixture;
        let moved = self.review.fixture_moved;
        self.run(RequestKind::Load,move|control|{
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
                this.review.opened.set(true);
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
        let cancellation = HistoryCancellation::default();
        self.local_control = Some(cancellation.clone());
        let stop = cancellation.clone();
        let sent = self.run(RequestKind::Templates,move|_|github::local_templates(&repo,&revision,&cancellation),|result,this,_,_|match result {Ok(templates)=>{this.templates=templates;this.notice=Some("Templates came from the locally available base revision. Choose one to append its exact text; repository HTML is never executed.".into());},Err(error)=>this.error=Some(format!("{error:#}"))},window,cx);
        // The read goes through its own cancellation, as a PR's Changes
        // does, which the panel's Cancel and the operation bar's stop; it
        // once read on to the end and showed the templates after Cancel.
        if sent {
            let _ = self
                .owner
                .update(cx, |owner, _| owner.cancel_with_operation(stop));
        }
    }
    fn review_submission(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_draft(window, cx);
        let draft = match self.current_draft(cx) {
            Ok(draft) => draft,
            Err(issue) => {
                self.refuse_input(issue);
                self.save_status = issue.save_message().into();
                cx.notify();
                return;
            }
        };
        self.draft_issue = None;
        if let Draft::Review(review) = &draft {
            if review.composing.is_some() {
                self.refuse(
                    "Add or discard the unfinished inline comment before reviewing submission.",
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
                self.refuse(error.to_string());
                cx.notify();
                return;
            }
        }
        let fixture = self.review.fixture;
        let moved = self.review.fixture_moved;
        if let Draft::Pull(pull) = draft {
            self.run(
                RequestKind::Load,
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
            self.refuse("Connect and refresh the GitHub account before submission.");
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
        // The confirmation goes once its action is sent: one a running Git
        // write refuses stays for Send once the write ends. Taken first, it
        // went with the refusal, and the submission had to be reviewed again.
        let Some(confirmation) = self.confirm.clone() else {
            return;
        };
        self.save_draft(window, cx);
        let fixture = self.review.fixture;
        let moved = self.review.fixture_moved;
        let sent_action = confirmation.action.clone();
        // A reply's Send answers in the reply's composer, where the reply was written; every
        // other action's, a conversation's Resolve or Reopen included, on the panel's top line.
        let origin = match &confirmation.action {
            Action::Reply { target, .. } => Origin::Reply(Box::new(target.clone())),
            _ => Origin::Panel,
        };
        let sent = self.run_from(origin, RequestKind::Send, move |control| {
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
        if sent {
            self.confirm = None;
        }
    }
    /// Says the lines the panel shows through the window's announcements, once each, as the
    /// panel opens with them and whenever they change: its notice politely
    /// (`copy_feedback::announce`), and its failure and refused input at once
    /// (`copy_feedback::alert`), both leaving a control reading Copied as it is. The lines
    /// are statuses, no live regions: an adapter announces a live region only while
    /// AccessKit's filter keeps its node, which it does not once another child lies between
    /// the node and the body's edge, and again each time the body scrolls it back. At 18 pt in
    /// a 461 × 490 window the notice lies below the fold, under the banner whose Move fixture
    /// head changes it, and a failure lands wherever the body has scrolled. Nor are they
    /// alerts: AT-SPI reports a line's `showing` change as the body scrolls it in or out
    /// (vendored adapter), and Orca presents an alert, a notification there, as it becomes
    /// showing, so a failure the window's alert said could be said again from its line. A
    /// press answered as the one before it was is said again, a fresh alert node for each
    /// press, as a refused Toggle Sidebar is: the lines alone stay the same.
    fn announce(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = Heard {
            notice: self.notice.clone(),
            error: self.error.clone(),
            cancelled: self.cancelled,
            issue: self.draft_issue,
            presses: self.presses,
        };
        if shown == self.heard {
            return;
        }
        let heard = std::mem::replace(&mut self.heard, shown);
        let shown = &self.heard;
        // A cancel the person asked for is said politely, as the notice is, not as an alert.
        let notice = [
            shown
                .notice
                .clone()
                .filter(|_| shown.notice != heard.notice),
            shown
                .cancelled
                .filter(|_| shown.cancelled != heard.cancelled)
                .map(str::to_owned),
        ]
        .into_iter()
        .flatten()
        .reduce(|notice, cancelled| format!("{notice} {cancelled}"));
        // A failure and refused input that change together, or that presses answer together,
        // are one alert, as the alert node holds one.
        let alert = [
            shown.error.clone().filter(|_| {
                shown.error != heard.error || shown.presses.error != heard.presses.error
            }),
            shown
                .issue
                .filter(|_| {
                    shown.issue != heard.issue || shown.presses.issue != heard.presses.issue
                })
                .map(|issue| issue.message().to_owned()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
        // The frame being drawn has drawn the window's announcements already.
        window.defer(cx, move |window, cx| {
            if let Some(notice) = notice {
                copy_feedback::announce(window, cx, notice);
            }
            if !alert.is_empty() {
                copy_feedback::alert(window, cx, alert);
            }
        });
    }
    /// The line in the failure's place, `failure` or `cancelled` its id: the last request's
    /// failure in the warning colour, or, where its Cancel stopped it, that it was cancelled
    /// ([`RequestKind::cancelled`]), muted as the notice is, since the person asked for it.
    /// Both are statuses; the window says the failure as an alert and the cancel politely
    /// ([`Self::announce`]).
    fn outcome(
        &self,
        failure: &'static str,
        cancelled: &'static str,
        cx: &App,
    ) -> Option<native_accessibility::StaticText<Stateful<Div>>> {
        let p = palette(cx);
        match (&self.error, self.cancelled) {
            (Some(error), _) => Some(label(failure, error.clone()).text_color(rgb(p.warning))),
            (None, Some(line)) => Some(label(cancelled, line).text_color(rgb(p.muted))),
            (None, None) => None,
        }
        .map(|line| line.role(Role::Status))
    }
    /// The panel's top line while a conversation is open: `lines`, its notice, its failure or
    /// cancel and refused input, then `busy`, the busy row with its Cancel, on the panel's
    /// surface above the body with a divider under them, where they stay in view, rather than
    /// among the content under its toolbar. As the line appears, changes height or clears, the
    /// body's scroll moves by as much, in the same frame and before the body places its
    /// content, so nothing drawn in the conversation moves: what the line takes from the
    /// body's top is still the body's, reached by scrolling up, and when the line clears the
    /// body takes it back with the scroll. The reveal the body last made keeps its place, so
    /// the shorter body reveals nothing anew. Among the content, a line appearing pushed the
    /// conversation, the draft being written included, down by its height. Where the content
    /// does not fill the body, there is nothing to scroll, and the conversation moves down by
    /// the line rather than lie under it. Text lines taller than half the body's room scroll
    /// within the line, so the body keeps room for the conversation, and the line's scrollbar
    /// cues the rest ([`crate::tags::ScrollCue`]); a long remedy was once cut there with no
    /// sign of more.
    fn pin_top_line(
        &self,
        lines: Vec<AnyElement>,
        busy: Option<AnyElement>,
        window: &Window,
        cx: &App,
    ) -> Div {
        let p = palette(cx);
        let drawn = self.review.top_line.clone();
        let scroll = self.review.body_scroll.clone();
        let revealed = self.review.revealed_focus.clone();
        // The text holds no control, so its rows keep no room for rings.
        let text = &self.review.top_line_scroll;
        let cue = crate::tags::ScrollCue::new(text, Pixels::ZERO);
        let line = (!lines.is_empty() || busy.is_some()).then(|| {
            div()
                .debug_selector(|| "github-top-line".into())
                .flex()
                .flex_col()
                .gap_2()
                .pb_2()
                .border_b_1()
                .border_color(rgb(p.border))
                .when(!lines.is_empty(), |line| {
                    line.child(
                        cue.list(
                            div()
                                .on_children_prepainted(cue.observe(|_, _, _| {}))
                                .id("github-top-line-scroll")
                                .debug_selector(|| "github-top-line-scroll".into())
                                .track_scroll(text)
                                .max_h(body_max_height(window) / 2.)
                                .overflow_y_scroll()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .children(lines),
                            "github-top-line-scrollbar",
                        ),
                    )
                })
                .children(busy)
        });
        // Prepainted before the body, which reads its scroll after.
        div()
            .flex_shrink_0()
            .on_children_prepainted(move |line, _, _| {
                let height = line.first().map_or(Pixels::ZERO, |line| line.size.height);
                let grew = height - drawn.replace(height);
                if grew != Pixels::ZERO {
                    let offset = scroll.offset();
                    scroll.set_offset(point(offset.x, offset.y - grew));
                    let mut revealed = revealed.borrow_mut();
                    revealed.viewport.origin.y += grew;
                    revealed.viewport.size.height -= grew;
                }
            })
            .children(line)
    }
    /// The panel's top line among the content, under the toolbar, while no conversation is
    /// open: `lines`, then `busy`, as one part of the content whose lines each start a block of
    /// the body. It records in `flowing` where it lies in the frame, from which the content
    /// records the room it takes there, its gap to the part after it included
    /// ([`review::ReviewState::top_line`]): as a reply opens and the line moves above the body
    /// ([`Self::pin_top_line`]), the body's scroll moves by only the difference, so the
    /// conversation below stays where it was drawn. With no room recorded there, the line
    /// moving above the body counted as a new line and scrolled the conversation up by its
    /// whole height.
    fn flow_top_line(
        &self,
        lines: Vec<AnyElement>,
        busy: Option<AnyElement>,
        flowing: std::rc::Rc<std::cell::Cell<Option<(Pixels, Pixels)>>>,
    ) -> Option<Div> {
        if lines.is_empty() && busy.is_none() {
            return None;
        }
        let blocks = self.review.body_blocks.clone();
        Some(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .on_children_prepainted(move |line, _, _| {
                    blocks.report(&line);
                    if let (Some(first), Some(last)) = (line.first(), line.last()) {
                        flowing.set(Some((first.top(), last.bottom())));
                    }
                })
                .children(lines)
                .children(busy),
        )
    }
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending && self.request.writing() {
            self.refuse("A GitHub action is in progress for this captured destination. Wait for the result, or choose Cancel and inspect GitHub before resubmitting.");
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
        self.heard = Heard::default();
        self.review.opened.set(false);
        self.request_generation = self.request_generation.wrapping_add(1);
        self.pending = false;
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
        let stop = cancellation.clone();
        let sent = self.run(RequestKind::Compare, move |_| {
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
        // The comparison reads through its own cancellation, which the
        // panel's Cancel stops; the operation bar's Cancel stops it too.
        if sent {
            let _ = self
                .owner
                .update(cx, |owner, _| owner.cancel_with_operation(stop));
        }
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
            let _ = this.update_in(cx, |this, _, cx| {
                match result {
                    Ok(Ok(())) => {
                        this.saved.retain(|draft| draft.key() != key);
                        this.notice =
                            Some("The selected saved draft was discarded locally.".into());
                    }
                    _ => this.refuse(
                        "Could not discard the selected draft; its stored text was preserved.",
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
        self.announce(window, cx);
        let p = palette(cx);
        let pending = self.pending;
        let scroll = self.review.body_scroll.clone();
        let observed = self.review.revealed_focus.clone();
        let show_editor =
            self.create || (self.pull.is_some() && self.review.section == Section::Review);
        // The room a focused control's ring takes outside its edge.
        let room = appearance::button_ring_room(cx);
        let blocks = self.review.body_blocks.clone();
        // Where the line holding the open PR's title starts, for showing the PR as it opens.
        let title_line = std::rc::Rc::new(std::cell::Cell::new(px(0.)));
        let line = title_line.clone();
        // The panel's top line: its notice, the failure or cancel that answers there, refused
        // input, then the busy row with its Cancel. While a conversation is open it stands
        // above the body ([`Self::pin_top_line`]); otherwise among the content, under the
        // toolbar.
        let pinned = self.conversations.active.is_some();
        let mut lines = Vec::new();
        lines.extend(self.notice.as_ref().map(|notice| {
            label("github-notice", notice.clone())
                .debug_selector(|| "github-notice".into())
                .ml(text_inset())
                .role(Role::Status)
                .text_color(rgb(p.muted))
                .into_any_element()
        }));
        lines.extend(
            self.outcome("github-error", "github-cancelled", cx)
                .filter(|_| !self.answers_in_reply())
                .map(|line| line.ml(text_inset()).into_any_element()),
        );
        lines.extend(self.draft_issue.map(|issue| {
            label("github-draft-validation", issue.message())
                .ml(text_inset())
                .role(Role::Status)
                .text_color(rgb(p.warning))
                .into_any_element()
        }));
        // Pinned, the row is always in view, so it is no part the body reveals.
        let busy = pending.then(|| {
            if pinned {
                div()
            } else {
                self.part("busy", div(), cx)
            }
            .flex()
            .items_center()
            .gap_2()
            .pl(text_inset())
            .child(
                label("github-busy", self.request.label())
                    .role(Role::Status)
                    .live_region(gpui::accesskit::Live::Polite, true),
            )
            .child(
                button("github-cancel", "Cancel", "", false)
                    .track_focus(self.cancel_request.handle())
                    .debug_selector(|| "github-cancel".into())
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(control) = &this.control {
                            control.cancel();
                        }
                        if let Some(control) = &this.local_control {
                            control.cancel();
                        }
                        this.notice = Some(this.request.requested().into());
                        cx.notify();
                    })),
            )
            .into_any_element()
        });
        // Where the line lies among the content this frame, its top and bottom, if it does.
        let flowing = std::rc::Rc::new(std::cell::Cell::new(None));
        let (flowing_line, top_line) = if pinned {
            (None, Some(self.pin_top_line(lines, busy, window, cx)))
        } else {
            self.review.top_line.set(Pixels::ZERO);
            (self.flow_top_line(lines, busy, flowing.clone()), None)
        };
        // Each part of the content starts a block of the body ([`review::Blocks`]), and the
        // line among it records the room it takes there, up to the part after it
        // ([`Self::flow_top_line`]).
        let starts = blocks.starts();
        let drawn = self.review.top_line.clone();
        let parts = move |children: Vec<Bounds<Pixels>>, window: &mut Window, cx: &mut App| {
            if let Some((top, bottom)) = flowing.get()
                && let Some(next) = children.iter().find(|part| part.top() >= bottom)
            {
                drawn.set(next.top() - top);
            }
            starts(children, window, cx);
        };
        // The panel's content, its text one inset in from this edge. Each part of it starts a
        // block of the body, where a reveal may bring the body's top edge.
        let content = div().debug_selector(||"github-panel-content".into()).on_children_prepainted(parts).min_w_0().flex().flex_col().gap_2()
            .when(self.review.fixture,|element|element.child(self.part("banner",div(),cx).flex().flex_wrap().items_center().gap_2().py_2().px(text_inset()).rounded(px(6.)).bg(rgb(p.subtle))
                .child(label("github-fixture-mode","OFFLINE FIXTURE · native review · no network or credentials").debug_selector(||"github-fixture-mode".into()).min_w_0().text_color(rgb(p.muted)))
                .child(button("github-fixture-move",if self.review.fixture_moved{"Fixture head moved"}else{"Move fixture head"},"",false).debug_selector(||"github-fixture-move".into()).disabled(pending||self.review.fixture_moved).on_click(cx.listener(|this,_,_,cx|{this.review.fixture_moved=true;this.confirm=None;this.notice=Some("Fixture head moved remotely. This view and its draft retain the original commit. Submit to verify stale-head refusal, or Refresh PRs and reopen to inspect the new head.".into());cx.notify();})))))
            .child(self.part("toolbar",div(),cx).flex().flex_wrap().items_center().gap_2()
                // Focus moved onto the field, by a click, by Name repository below, which
                // reveals it even when it holds focus already, or by the panel reopening onto
                // it, scrolls it into view with its ring, as a Tab onto it does, and only where
                // it is not whole already. It is a part of its own, as each control in the row
                // is: at 18 pt in a 461 × 490 window the row is taller than the body, and
                // revealing the whole row scrolled a field already whole, and the banner above
                // it out of view.
                .child(self.part("destination",div(),cx).debug_selector(||"github-destination".into()).w(appearance::ui_size(280.)).min_w(px(190.)).child(div().on_children_prepainted(self.reveal_field_on_focus(self.destination.read(cx).focus_handle(cx))).child(Input::new(&self.destination).aria_label("GitHub destination owner and repository").disabled(pending))))
                .child(self.part("refresh",div(),cx).child(button("github-refresh",if self.review.fixture{"Refresh fixture PRs"}else{"Refresh PRs"},"refresh",false).disabled(pending).on_click(cx.listener(|this,_,window,cx|this.refresh(1,window,cx)))))
                .child(self.part("create",div(),cx).child(button("github-create","New pull request","plus",false).disabled(pending||self.review.fixture).on_click(cx.listener(|this,_,window,cx|this.create(window,cx)))))
                .child(div().flex_1()).child(self.part("account",div(),cx).child(button("github-account-options",self.account.as_ref().map(|account|format!("Account · {account}")).unwrap_or_else(||"Connect account".into()),"",self.review.connection_open).debug_selector(||"github-account-options".into()).toggled(self.review.connection_open).disabled(pending).on_click(cx.listener(|this,_,_,cx|{this.review.connection_open= !this.review.connection_open;cx.notify();})))))
            .when(self.review.connection_open,|element|element.child(self.part("connection",div(),cx).p_3().border_1().border_color(rgb(p.border)).rounded(px(6.)).flex().flex_col().gap_2()
                .child(label("github-worktree",format!("Local worktree: {}",self.repo.path().display())).text_color(rgb(p.muted)))
                .child(label("github-connection-help",connection_help()).text_color(rgb(p.muted)))
                .child(div().flex().flex_wrap().gap_2()
                    .child(button("github-connect","Connect GitHub CLI account","",false).debug_selector(||"github-connect".into()).disabled(pending||self.review.fixture).on_click(cx.listener(|this,_,window,cx|this.connect(window,cx))))
                    .child(copy_button(copy_feedback::Mark::new("github-sign-in-help",SIGN_IN_COMMAND),"Copy sign-in command","",cx).on_click(|_,window,cx|copy_feedback::copy(window,cx,Some(copy_feedback::Mark::new("github-sign-in-help",SIGN_IN_COMMAND)),"sign-in command",SIGN_IN_COMMAND.into())))
                    .child(button("github-disconnect","Disconnect GitTurtle","",false).debug_selector(||"github-disconnect".into()).disabled(pending||self.review.fixture).on_click(cx.listener(|this,_,window,cx|this.disconnect(window,cx)))))))
            .children(flowing_line)
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
                // The PR's header and its section tabs, shown from the title's line as the PR
                // opens.
                .child(div().flex().flex_col().gap_2().on_children_prepainted(self.reveal_opened(title_line,cx)).child(self.part("pull-header",div(),cx).flex().flex_col().gap_2()
                    .child(div().flex().flex_wrap().items_center().gap_2().on_children_prepainted(move|children,_,_|{
                        // The title's line: the Button's where the title stands beside it, its
                        // own where it wraps below.
                        if let [back,title]=children[..]{line.set(if title.top()<back.bottom(){back.top().min(title.top())}else{title.top()});}
                    }).child(button("github-back-list",if self.review.show_list{"Hide PR list"}else{"Pull requests"},"arrow-left",false).debug_selector(||"github-back-list".into()).mr(-text_inset()).disabled(pending).on_click(cx.listener(|this,_,window,cx|{this.save_draft(window,cx);this.review.show_list= !this.review.show_list;cx.notify();})))
                        .child(label("github-pull-title",format!("#{} · {}",pull.number,pull.title)).debug_selector(||"github-pull-title".into()).min_w_0().ml(text_inset()).font_weight(FontWeight::SEMIBOLD).text_size(appearance::ui_text(15.))))
                    .child(label("github-branch-context",format!("{} → {} · {}",pull.head.branch,pull.base.branch,if pull.draft{"Draft PR"}else{"Open PR"})).ml(text_inset()).text_color(rgb(p.muted))))
                .child(self.render_tabs(cx)))
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
                .child(div().debug_selector(||"github-review-summary".into()).on_children_prepainted(self.reveal_field_on_focus(self.body.read(cx).focus_handle(cx))).child(Textarea::new(&self.body).h(appearance::ui_size(130.)).aria_label(if self.create{"Pull request description"}else{"Review summary"}).disabled(pending)))
                // The draft status beside Review submission… where it fits there whole, at the
                // panel's text inset; otherwise on a line of its own below it, wrapping, so its
                // remedy stays readable and the button stays inside the panel (at 18 pt a failed
                // save's status pushed the button past the panel's right edge).
                .child(self.part("submit",div(),cx).flex().flex_wrap_reverse().items_center().gap_2().child(label("github-draft-status",self.save_status.clone()).debug_selector(||"github-draft-status".into()).min_w_0().mx(text_inset()).role(Role::Status).polite().text_color(rgb(p.muted)))
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
        let body = cue.list(
            div()
                .on_children_prepainted(cue.observe(move |_, window, cx| {
                    review::record_shown(
                        &observed,
                        &scroll,
                        |context| context.contains(review::BODY),
                        window,
                        cx,
                    );
                    blocks.finish();
                }))
                .key_context(review::BODY)
                .id("github-panel")
                .debug_selector(|| "github-panel".into())
                .track_scroll(&self.review.body_scroll)
                .min_w_0()
                .min_h_0()
                .max_h(body_max_height(window))
                .p(room)
                .mx(-room)
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .child(content),
            "github-panel-scrollbar",
        );
        match top_line {
            // The line and the body share the body's height, the body taking what the line
            // leaves.
            Some(top_line) => div()
                .min_w_0()
                .max_h(body_max_height(window))
                .flex()
                .flex_col()
                .child(top_line)
                .child(body.min_h_0())
                .into_any_element(),
            None => body.into_any_element(),
        }
    }
}

/// Submits a request of `kind` over `app`'s repository, as the panel sends
/// one, that answers at once once the operation executor runs it, for a
/// test of what the window says while a request runs: hold the executor
/// first to keep it running.
#[cfg(test)]
pub(crate) fn submit_test_request(
    app: &mut GitTurtle,
    kind: RequestKind,
    window: &mut Window,
    cx: &mut Context<GitTurtle>,
) {
    let repository = app.path.clone().expect("a repository");
    let submitted = app.submit_github_request(kind, repository, |_| anyhow::Ok(()), window, cx);
    assert!(submitted.is_ok(), "{kind:?}: nothing else runs");
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

    /// A request the panel sends holds the window's operation executor, so
    /// it runs as the window's busy spell, as a write does, labelled with
    /// the panel's busy line: the header's progress says what runs once the
    /// spell looks busy, Changes' Refresh is refused, and a working-state
    /// read asked for meanwhile, as Settings asks on its return, waits for
    /// the request rather than queueing behind it, and runs as it ends. Such
    /// a read once queued behind a review being sent, with nothing saying
    /// why, for as long as GitHub took to answer.
    #[gpui::test]
    async fn a_working_read_waits_for_a_github_request_and_says_why(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, history_script, history_window_from, settle};
        use crate::workspace::{BUSY_LOOK_DELAY, refusals};
        let (fixture, app, cx) = history_window_from(cx, history_script(2, 3)).await;
        // Wide enough that the header draws the busy look's progress label.
        cx.simulate_resize(size(px(1600.), px(900.)));
        let root = fixture.path().join("narrow");
        std::fs::write(root.join("draft.txt"), "draft\n").unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        let destination = "gitturtle-fixture/native-review";
        let panel = cx.update(|window, cx| {
            let repo = app.read(cx).repository.clone().unwrap();
            let panel = cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx));
            panel.update(cx, |panel, cx| {
                panel.review.fixture = true;
                panel.account = Some("offline-reviewer".into());
                panel
                    .destination
                    .update(cx, |input, cx| input.set_value(destination, window, cx));
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
            panel
        });
        settle_panel(&app, cx).await;
        let entries = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                app.work_status.as_ref().map(|status| {
                    status
                        .entries
                        .iter()
                        .map(|entry| entry.path.display().to_string())
                        .collect::<Vec<_>>()
                })
            })
            .unwrap_or_default()
        };
        type Request = fn(&mut Panel, &mut Window, &mut Context<Panel>);
        let requests: [(&str, Request, &str); 2] = [
            (
                "Refresh PRs",
                |panel, window, cx| panel.refresh(1, window, cx),
                RequestKind::Load.label(),
            ),
            (
                "a review comment",
                |panel, window, cx| {
                    let repository = Repository::parse("gitturtle-fixture/native-review").unwrap();
                    panel.confirm = Some(Confirmation {
                        action: Action::Comment {
                            pull: github::review::fixture_pull(false).capture(repository),
                            body: "Looks right.".into(),
                        },
                        account: "offline-reviewer".into(),
                    });
                    panel.submit(window, cx);
                },
                RequestKind::Send.label(),
            ),
        ];
        for (index, (request, send, label)) in requests.into_iter().enumerate() {
            // The request held as a slow network holds it.
            let held = app.update(cx, |app, _| app.operations.hold());
            cx.update(|window, cx| panel.update(cx, |panel, cx| send(panel, window, cx)));
            assert!(
                panel.read_with(cx, |panel, _| panel.pending),
                "{request} is sent"
            );
            let later = format!("later-{index}.txt");
            std::fs::write(root.join(&later), "later\n").unwrap();
            cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
            app.read_with(cx, |app, _| {
                assert!(
                    app.status_task.is_none(),
                    "{request}: no read queues behind it"
                );
                assert_eq!(
                    app.operation_busy,
                    Some(label),
                    "{request}: the window is busy"
                );
                assert!(
                    app.working_read_pending(),
                    "{request}: the read waits for it"
                );
            });
            cx.executor().advance_clock(BUSY_LOOK_DELAY);
            draw(cx);
            assert!(
                cx.debug_bounds("operation-progress").is_some(),
                "{request}: the header says what runs"
            );
            assert_eq!(
                app.read_with(cx, |app, _| app.operation_shown()),
                Some(label)
            );
            assert_eq!(
                refusals::get("refresh-working").map(|refusal| refusal.announced),
                Some(true),
                "{request}: Changes' Refresh is refused"
            );
            drop(held);
            app.read_with(cx, |app, _| app.operations.drain());
            cx.executor().run_until_parked();
            settle(&app, cx).await;
            app.read_with(cx, |app, _| {
                assert_eq!(app.operation_busy, None, "{request}: it ended");
                assert_eq!(app.working_read_deferred, None, "{request}");
            });
            assert!(
                !panel.read_with(cx, |panel, _| panel.pending),
                "{request}: it answered"
            );
            assert!(
                entries(cx).contains(&later),
                "{request}: the read asked for meanwhile ran as it ended"
            );
        }
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// The panel over a window showing History, on the offline fixture,
    /// signed in as its reviewer with its destination named, as
    /// `a_working_read_waits_for_a_github_request_and_says_why` opens it.
    async fn busy_fixture_panel(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        Entity<Panel>,
        &mut VisualTestContext,
    ) {
        busy_fixture_panel_from(cx, crate::views::tests::history_script(2, 3)).await
    }

    /// [`busy_fixture_panel`] with a new PR's base, `main`, holding a PR template.
    async fn templated_fixture_panel(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        Entity<Panel>,
        &mut VisualTestContext,
    ) {
        const MESSAGE: &str = "Add the PR template";
        const TEMPLATE: &str = "## What changes\n";
        let script = crate::views::tests::history_script(2, 3);
        let script = format!(
            "{}commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> 1700000002 +0000\n\
             data {}\n{MESSAGE}\nM 100644 inline .github/pull_request_template.md\ndata {}\n\
             {TEMPLATE}\ndone\n",
            script.strip_suffix("done\n").expect("the script's end"),
            MESSAGE.len(),
            TEMPLATE.len(),
        );
        busy_fixture_panel_from(cx, script).await
    }

    /// [`busy_fixture_panel`] over the repository `script` imports.
    async fn busy_fixture_panel_from(
        cx: &mut TestAppContext,
        script: String,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        Entity<Panel>,
        &mut VisualTestContext,
    ) {
        let (fixture, app, cx) = crate::views::tests::history_window_from(cx, script).await;
        let panel = cx.update(|window, cx| {
            let repo = app.read(cx).repository.clone().unwrap();
            let panel = cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx));
            panel.update(cx, |panel, cx| {
                panel.review.fixture = true;
                panel.account = Some("offline-reviewer".into());
                panel.destination.update(cx, |input, cx| {
                    input.set_value("gitturtle-fixture/native-review", window, cx)
                });
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
            panel
        });
        settle_panel(&app, cx).await;
        (fixture, app, panel, cx)
    }

    /// A review comment confirmed for sending.
    fn confirmed_comment() -> Confirmation {
        let repository = Repository::parse("gitturtle-fixture/native-review").unwrap();
        Confirmation {
            action: Action::Comment {
                pull: github::review::fixture_pull(false).capture(repository),
                body: "Looks right.".into(),
            },
            account: "offline-reviewer".into(),
        }
    }

    /// Send pressed while a Git write runs is refused: the panel says why
    /// and sends nothing, and the confirmation stays, so Send sends it once
    /// the write has ended. The refusal once took the confirmation with it,
    /// and the submission had to be reviewed again before it could be sent.
    #[gpui::test]
    async fn a_confirmation_a_git_write_refuses_stays_to_send(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        // Stage all, held as a slow disk holds it.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.write(
                    gitturtle_core::WriteCommand::StageAll,
                    "Staging files…",
                    window,
                    cx,
                )
            })
        });
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.confirm = Some(confirmed_comment());
                panel.submit(window, cx);
            })
        });
        draw(cx);
        panel.read_with(cx, |panel, _| {
            assert!(!panel.pending, "nothing is sent");
            assert!(panel.confirm.is_some(), "the confirmation stays");
            assert_eq!(
                panel.error.as_deref(),
                Some("Wait for the current Git operation to finish, then try again.")
            );
        });
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            Some("Staging files…"),
            "the write still runs"
        );
        assert!(cx.debug_bounds("github-confirmation").is_some());
        assert!(cx.debug_bounds("github-error").is_some(), "it says why");
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle(&app, cx).await;
        // Send again, the write ended: the confirmed comment is sent.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.submit(window, cx)));
        draw(cx);
        panel.read_with(cx, |panel, _| {
            assert!(panel.pending, "the comment is sent");
            assert!(panel.confirm.is_none(), "its confirmation goes");
            assert_eq!(panel.error, None);
        });
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            Some(RequestKind::Send.label())
        );
        assert!(cx.debug_bounds("github-confirmation").is_none());
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle_panel(&app, cx).await;
        panel.read_with(cx, |panel, _| {
            assert!(!panel.pending, "it answered");
            assert_eq!(panel.error, None, "the fixture accepted it");
        });
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// Each kind of request the panel sends names itself in the panel's
    /// busy line and the window's operation bar, and what the busy spell
    /// refuses meanwhile gives that kind's reason: Projects' tooltip, and the
    /// palette's and the Linux Main Menu's rows. Connecting the account and
    /// Disconnect once read "Loading your requested PR context…", and the
    /// refusals said to wait for "the current Git operation". A Git write's
    /// refusals still name the Git operation.
    #[gpui::test]
    async fn each_github_request_names_itself_and_what_refusals_wait_for(cx: &mut TestAppContext) {
        use crate::command_palette::CommandId;
        use crate::views::tests::{draw, settle};
        use crate::workspace::{BUSY_LOOK_DELAY, BUSY_REASON, refusals};
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        // Wide enough that the header draws the busy look's progress label,
        // and Changes, whose header has Projects, where History's root has
        // Back to Projects.
        cx.simulate_resize(size(px(1600.), px(900.)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        // Once the spell looks busy: Projects' tooltip and Tags' row.
        let refusals_once_busy = |cx: &mut VisualTestContext| {
            cx.executor().advance_clock(BUSY_LOOK_DELAY);
            draw(cx);
            (
                refusals::tooltip("projects").map(|tooltip| tooltip.to_string()),
                cx.update(|window, cx| {
                    app.read(cx)
                        .menu_command_reason(CommandId::Tags, window, cx)
                }),
            )
        };
        let mut labels = Vec::new();
        for kind in RequestKind::ALL {
            // The request held as a slow network holds it.
            let held = app.update(cx, |app, _| app.operations.hold());
            let sent = cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.run(
                        kind,
                        |_| Ok(()),
                        |_: anyhow::Result<()>, _, _, _| {},
                        window,
                        cx,
                    )
                })
            });
            assert!(sent, "{kind:?} is sent");
            let (tooltip, palette) = refusals_once_busy(cx);
            assert_eq!(
                app.read_with(cx, |app, _| app.operation_shown()),
                Some(kind.label()),
                "{kind:?}: the operation bar names it"
            );
            assert!(cx.debug_bounds("operation-progress").is_some());
            assert_eq!(
                panel.read_with(cx, |panel, _| panel.request.label()),
                kind.label(),
                "{kind:?}: so does the panel's busy line"
            );
            assert!(cx.debug_bounds("github-busy").is_some());
            assert_eq!(
                tooltip,
                Some(format!("Open Projects · {}", kind.reason())),
                "{kind:?}: Projects waits for it"
            );
            assert_eq!(palette, Some(kind.reason()), "{kind:?}: so does Tags");
            labels.push(kind.label());
            drop(held);
            app.read_with(cx, |app, _| app.operations.drain());
            settle_panel(&app, cx).await;
            assert_eq!(app.read_with(cx, |app, _| app.operation_busy), None);
        }
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(
            labels.len(),
            RequestKind::ALL.len(),
            "each kind has its own line"
        );
        // Stage all, held: its refusals name the Git operation.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.write(
                    gitturtle_core::WriteCommand::StageAll,
                    "Staging files…",
                    window,
                    cx,
                )
            })
        });
        let (tooltip, palette) = refusals_once_busy(cx);
        assert_eq!(
            tooltip.as_deref(),
            Some(format!("Open Projects · {BUSY_REASON}").as_str())
        );
        assert_eq!(palette, Some(BUSY_REASON));
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle(&app, cx).await;
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// A PR from the window's first commit to its latest, both local, for
    /// its Changes.
    fn local_pull(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> PullRequest {
        let (base, head) = app.read_with(cx, |app, _| {
            (
                app.commits.last().unwrap().oid.clone(),
                app.commits.first().unwrap().oid.clone(),
            )
        });
        assert_ne!(base, head);
        serde_json::from_value(serde_json::json!({
            "number": 7, "title": "Local", "body": null, "state": "open", "draft": false,
            "user": {"login": "river-chen"},
            "head": {"sha": head, "ref": "feature", "label": "river-chen:feature"},
            "base": {"sha": base, "ref": "main", "label": "gitturtle-fixture:main"}
        }))
        .unwrap()
    }

    /// The panel's local work, started from the panel's own callers, names
    /// itself in what the busy spell refuses meanwhile: a new PR's
    /// templates, read from its base in the local repository, say to wait
    /// for the pull request templates, and a PR's Changes, compared there,
    /// for the pull request comparison, in Projects' tooltip and the
    /// palette's and the Linux Main Menu's rows. Both once said to wait for
    /// "the GitHub request", which neither sends, and the templates read
    /// ran as a PR context load, "Loading your requested PR context…".
    #[gpui::test]
    async fn the_panels_local_work_names_itself_in_what_refusals_wait_for(cx: &mut TestAppContext) {
        use crate::command_palette::CommandId;
        use crate::views::tests::{draw, settle};
        use crate::workspace::{BUSY_LOOK_DELAY, refusals};
        type Start = fn(&mut Panel, &mut Window, &mut Context<Panel>);
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        // Wide enough that the header draws the busy look's progress label,
        // and Changes, whose header has Projects.
        cx.simulate_resize(size(px(1600.), px(900.)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        let pull = local_pull(&app, cx);
        cx.update(|_, cx| panel.update(cx, |panel, _| panel.pull = Some(pull)));
        // The templates first: the comparison, once it lands, closes the
        // panel to show its changes.
        let work: [(&str, Start, &str, &str); 2] = [
            (
                "Templates",
                Panel::load_templates,
                "Reading the PR templates locally…",
                "Wait for the pull request templates to load",
            ),
            (
                "Changes",
                Panel::changes,
                "Comparing the PR's changes locally…",
                "Wait for the pull request comparison to finish",
            ),
        ];
        for (name, start, label, reason) in work {
            // Held as a large repository holds it.
            let held = app.update(cx, |app, _| app.operations.hold());
            cx.update(|window, cx| panel.update(cx, |panel, cx| start(panel, window, cx)));
            cx.executor().advance_clock(BUSY_LOOK_DELAY);
            draw(cx);
            assert_eq!(
                app.read_with(cx, |app, _| app.operation_shown()),
                Some(label),
                "{name}: the operation bar names it"
            );
            assert_eq!(
                refusals::tooltip("projects").map(|tooltip| tooltip.to_string()),
                Some(format!("Open Projects · {reason}")),
                "{name}: Projects waits for it"
            );
            assert_eq!(
                cx.update(|window, cx| {
                    app.read(cx)
                        .menu_command_reason(CommandId::Tags, window, cx)
                }),
                Some(reason),
                "{name}: so does Tags"
            );
            drop(held);
            app.read_with(cx, |app, _| app.operations.drain());
            settle_panel(&app, cx).await;
            assert_eq!(
                app.read_with(cx, |app, _| app.operation_busy),
                None,
                "{name} ended"
            );
        }
    }

    /// Connect GitHub CLI account and Disconnect GitTurtle, pressed in the
    /// panel, each name themselves in the window's operation bar and the
    /// panel's busy line: their own buttons choose the request's kind, which
    /// a test sending each kind through `Panel::run` cannot tell. Both once
    /// read "Loading your requested PR context…". Cancelled from the panel
    /// before they run, neither reaches the account: the account stays and
    /// the panel says it was cancelled, where Disconnect's removal once
    /// ignored Cancel. A test build reaches the account's stand-in
    /// (`account`), which would connect it or empty it, so a guard that
    /// regressed fails here rather than reading a developer's GitHub CLI
    /// token or removing their stored authorization.
    #[gpui::test]
    async fn connect_and_disconnect_name_themselves_from_their_buttons(cx: &mut TestAppContext) {
        use crate::copy_feedback::tests::click;
        use crate::views::tests::draw;
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        // Dialogs slide in on the wall clock, which the test clock does not
        // hold; opened at rest, every frame draws the panel in place. The
        // offline fixture leaves Connect and Disconnect unavailable.
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
        cx.update(|_, cx| {
            cx.set_reduce_motion(true);
            panel.update(cx, |panel, _| panel.review.fixture = false);
        });
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;
        click(cx, "github-account-options");
        cx.run_until_parked();
        draw(cx);
        for (button, kind) in [
            ("github-connect", RequestKind::Connect),
            ("github-disconnect", RequestKind::Disconnect),
        ] {
            // Held as a slow GitHub CLI or secure store holds it.
            let held = app.update(cx, |app, _| app.operations.hold());
            click(cx, button);
            draw(cx);
            assert_eq!(
                app.read_with(cx, |app, _| app.operation_busy),
                Some(kind.label()),
                "{button}: the operation bar names it"
            );
            assert_eq!(
                panel.read_with(cx, |panel, _| panel.request.label()),
                kind.label(),
                "{button}: so does the panel's busy line"
            );
            click(cx, "github-cancel");
            drop(held);
            app.read_with(cx, |app, _| app.operations.drain());
            settle_panel(&app, cx).await;
            draw(cx);
            panel.read_with(cx, |panel, _| {
                assert!(!panel.pending, "{button}: it answered");
                assert_eq!(
                    panel.account.as_deref(),
                    Some("offline-reviewer"),
                    "{button}: the account stays"
                );
                assert_eq!(
                    (panel.error.as_deref(), panel.cancelled),
                    (None, kind.cancelled(true)),
                    "{button}: the panel says it was cancelled"
                );
            });
        }
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// Connect stopped by Cancel leaves the account as it was and says so: with an account
    /// connected, "Cancelled · the account is unchanged", where it once said no account was
    /// connected while the toolbar named the connected one; with none, that none was.
    #[gpui::test]
    async fn a_cancelled_connect_says_what_it_left_of_the_account(cx: &mut TestAppContext) {
        use crate::copy_feedback::tests::click;
        use crate::views::tests::draw;
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        // Opened at rest, so a click lands where the last frame drew its button. The offline
        // fixture leaves Connect unavailable.
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
        cx.update(|_, cx| {
            cx.set_reduce_motion(true);
            panel.update(cx, |panel, _| panel.review.fixture = false);
        });
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;
        click(cx, "github-account-options");
        cx.run_until_parked();
        draw(cx);
        for (account, says) in [
            (
                Some("offline-reviewer"),
                "Cancelled · the account is unchanged",
            ),
            (None, "Cancelled · no account was connected"),
        ] {
            cx.update(|_, cx| {
                panel.update(cx, |panel, cx| {
                    panel.account = account.map(str::to_owned);
                    cx.notify();
                })
            });
            draw(cx);
            // Held as a slow GitHub CLI holds it, then the panel's Cancel.
            let held = app.update(cx, |app, _| app.operations.hold());
            click(cx, "github-connect");
            draw(cx);
            click(cx, "github-cancel");
            drop(held);
            app.read_with(cx, |app, _| app.operations.drain());
            settle_panel(&app, cx).await;
            draw(cx);
            assert_eq!(
                panel.read_with(cx, |panel, _| (panel.account.clone(), panel.cancelled)),
                (account.map(str::to_owned), Some(says)),
                "{account:?}: the account stays and the panel says so"
            );
            assert!(
                cx.debug_bounds("github-cancelled").is_some(),
                "{account:?}: on the panel's top line"
            );
        }
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// The panel's own Cancel says what it asked for in the request's own words
    /// (`RequestKind::requested`), as the operation bar's Cancel does in its tooltip: it said
    /// "Your review draft is retained" while Connect, Disconnect or a comparison ran, where no
    /// review draft is involved. Connect's success names the platform's store from one place
    /// (`SecureStore::NAME`): it named macOS Keychain on every platform.
    #[gpui::test]
    async fn the_panel_says_what_its_cancel_and_connect_did(cx: &mut TestAppContext) {
        use crate::copy_feedback::tests::click;
        use crate::views::tests::draw;
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
        cx.update(|_, cx| {
            cx.set_reduce_motion(true);
            panel.update(cx, |panel, _| panel.review.fixture = false);
        });
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;
        for kind in RequestKind::ALL {
            let held = app.update(cx, |app, _| app.operations.hold());
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.run(
                        kind,
                        |_| Ok(()),
                        |_: anyhow::Result<()>, _, _, _| {},
                        window,
                        cx,
                    )
                })
            });
            draw(cx);
            click(cx, "github-cancel");
            let notice = panel.read_with(cx, |panel, _| panel.notice.clone());
            assert_eq!(notice.as_deref(), Some(kind.requested()), "{kind:?}");
            if kind == RequestKind::Connect {
                // What a stopped Connect may have left, in this notice and the operation
                // bar's tooltip: only a Mac's store can hold the authorization.
                for line in [notice.as_deref().unwrap_or_default(), kind.cancel().1] {
                    if cfg!(target_os = "macos") {
                        assert!(
                            line.ends_with(
                                "Authorization may already be stored; Disconnect removes it."
                            ),
                            "{line}"
                        );
                    } else {
                        assert!(
                            line.ends_with("GitTurtle stores no authorization on this platform.")
                                && !line.contains("may already be stored"),
                            "nothing can be stored here: {line}"
                        );
                    }
                }
            }
            if matches!(
                kind,
                RequestKind::Connect | RequestKind::Disconnect | RequestKind::Compare
            ) {
                assert!(
                    !kind.requested().contains("review draft"),
                    "{kind:?} involves no review draft"
                );
            }
            drop(held);
            app.read_with(cx, |app, _| app.operations.drain());
            settle_panel(&app, cx).await;
        }

        // Connect, reaching the test build's stand-in for the account.
        click(cx, "github-account-options");
        cx.run_until_parked();
        draw(cx);
        click(cx, "github-connect");
        app.read_with(cx, |app, _| app.operations.drain());
        settle_panel(&app, cx).await;
        let notice = panel.read_with(cx, |panel, _| panel.notice.clone().unwrap_or_default());
        assert_eq!(
            notice,
            format!(
                "Connected as {}; authorization is stored in {}.",
                account::LOGIN,
                SecureStore::NAME
            )
        );
        #[cfg(not(target_os = "macos"))]
        assert!(
            !notice.contains("Keychain"),
            "no Keychain outside macOS: {notice}"
        );
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// While a request of the panel runs, the operation bar's Cancel, labelled
    /// or as its icon, is named after what it cancels and its tooltip says
    /// what stops, from the request's kind (`RequestKind::cancel`): it was
    /// named "Cancel the running Git operation" and said it stopped "Git and
    /// its child processes" while the panel connected an account. A Git
    /// write's Cancel still names the Git operation. A new PR's templates,
    /// read in the local repository, stop at the bar's Cancel, as its name
    /// says: none shows and the panel says the read was cancelled, where the
    /// read once ran to its end and showed them. The fixture's base holds a
    /// template, which the read finds uncancelled: with none, a read that
    /// ignored Cancel showed nothing all the same.
    #[gpui::test]
    async fn the_operation_bars_cancel_names_what_it_cancels(cx: &mut TestAppContext) {
        use crate::authentication::{CANCEL_LABELS, CANCEL_NAME, CANCEL_TOOLTIP};
        let (_fixture, app, panel, cx) = templated_fixture_panel(cx).await;
        // Cancel as the window would build it now, labelled or as its icon:
        // its accessible name and its tooltip.
        let cancel = |icon: bool, cx: &mut VisualTestContext| {
            let owner = app.clone();
            let node = crate::views::tests::view_node(cx, move |window, cx| {
                use gpui::Element as _;
                let button = owner
                    .update(cx, |app, cx| app.operation_cancel(icon, cx))
                    .expect("Cancel")
                    .base(window, cx);
                let element = RenderOnce::render(button, window, cx).into_element();
                let mut node =
                    gpui::accesskit::Node::new(element.a11y_role().expect("Cancel's role"));
                element.write_a11y_info(&mut node);
                node
            });
            let tooltip = crate::views::CONTROL_TOOLTIPS
                .with(|tooltips| tooltips.borrow().get("cancel-git-operation").cloned());
            (node.label().map(str::to_owned), tooltip)
        };
        let says = |name: &str, stops: &str, icon: bool| {
            let tooltip = if icon {
                format!("{} · {stops}", CANCEL_LABELS[0])
            } else {
                stops.to_owned()
            };
            (Some(name.to_owned()), Some(tooltip))
        };
        for kind in RequestKind::ALL {
            // The request held as a slow network holds it.
            let held = app.update(cx, |app, _| app.operations.hold());
            let sent = cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.run(
                        kind,
                        |_| Ok(()),
                        |_: anyhow::Result<()>, _, _, _| {},
                        window,
                        cx,
                    )
                })
            });
            assert!(sent, "{kind:?} is sent");
            let (name, stops) = kind.cancel();
            for icon in [false, true] {
                assert_eq!(
                    cancel(icon, cx),
                    says(name, stops, icon),
                    "{kind:?}: Cancel{} names what it cancels",
                    if icon { " as its icon" } else { "" }
                );
            }
            drop(held);
            app.read_with(cx, |app, _| app.operations.drain());
            settle_panel(&app, cx).await;
        }
        // A Git write's Cancel names the Git operation.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.write(
                    gitturtle_core::WriteCommand::StageAll,
                    "Staging files…",
                    window,
                    cx,
                )
            })
        });
        for icon in [false, true] {
            assert_eq!(
                cancel(icon, cx),
                says(CANCEL_NAME, CANCEL_TOOLTIP, icon),
                "a Git write"
            );
        }
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;

        // The templates read finds the base's template.
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.load_templates(window, cx)));
        app.read_with(cx, |app, _| app.operations.drain());
        settle_panel(&app, cx).await;
        assert_eq!(
            panel.read_with(cx, |panel, _| panel
                .templates
                .iter()
                .map(|template| template.path.clone())
                .collect::<Vec<_>>()),
            [".github/pull_request_template.md"],
            "the base holds a template"
        );
        cx.update(|_, cx| panel.update(cx, |panel, _| panel.templates.clear()));

        // The templates read, held, then the bar's Cancel.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.load_templates(window, cx)));
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            Some(RequestKind::Templates.label()),
            "the templates read runs as the busy spell"
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.cancel_operation(window, cx)));
        assert!(
            panel.read_with(cx, |panel, _| panel
                .local_control
                .as_ref()
                .is_some_and(HistoryCancellation::is_cancelled)),
            "the bar's Cancel stops the templates read"
        );
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle_panel(&app, cx).await;
        assert_eq!(app.read_with(cx, |app, _| app.operation_busy), None);
        panel.read_with(cx, |panel, _| {
            assert!(panel.templates.is_empty(), "no template shows");
            assert_eq!(
                (panel.error.as_deref(), panel.cancelled),
                (None, RequestKind::Templates.cancelled(false)),
                "the panel says the read was cancelled"
            );
        });
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// The operation bar's Cancel (`GitTurtle::cancel_operation`) stops a
    /// PR's Changes, as the panel's own Cancel does: the comparison reads
    /// through its own cancellation, which the bar's Cancel once never
    /// reached, so the comparison ran on and opened.
    #[gpui::test]
    async fn the_operation_bars_cancel_stops_a_prs_changes(cx: &mut TestAppContext) {
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        let pull = local_pull(&app, cx);
        cx.update(|_, cx| panel.update(cx, |panel, _| panel.pull = Some(pull)));
        // Changes, held as a large comparison holds it, then the bar's Cancel.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.changes(window, cx)));
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            Some(RequestKind::Compare.label()),
            "Changes runs as the busy spell"
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.cancel_operation(window, cx)));
        assert!(
            panel.read_with(cx, |panel, _| panel
                .local_control
                .as_ref()
                .is_some_and(HistoryCancellation::is_cancelled)),
            "the bar's Cancel stops the comparison"
        );
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle_panel(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(app.operation_busy, None, "Changes ended");
            assert!(!app.revision_inspection.is_active(), "no comparison opens");
        });
        assert_eq!(
            panel.read_with(cx, |panel, _| (panel.error.clone(), panel.cancelled)),
            (None, RequestKind::Compare.cancelled(false)),
            "the panel says it did not compare, as cancelled"
        );
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// A PR's Changes stopped by Cancel, the panel's own or the operation
    /// bar's, says so in the failure's place: "Cancelled", muted, a status
    /// the window says politely rather than as an alert, since the person
    /// asked for it, until the next request clears it as it clears a
    /// failure. It once showed the failure telling the person to Fetch. A
    /// failure the comparison met before the cancel reached it, as a
    /// captured revision Git cannot take, still shows its error there.
    #[gpui::test]
    async fn a_cancelled_prs_changes_say_they_were_cancelled(cx: &mut TestAppContext) {
        use crate::copy_feedback::tests::{alerted, announced, click};
        use crate::native_accessibility::written;
        use crate::views::tests::draw;
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        // Opened at rest, so a click lands where the last frame drew its button.
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
        let pull = local_pull(&app, cx);
        cx.update(|_, cx| {
            cx.set_reduce_motion(true);
            panel.update(cx, |panel, _| panel.pull = Some(pull.clone()));
        });
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;
        let line = RequestKind::Compare
            .cancelled(false)
            .expect("Changes says it was cancelled");

        // Changes, held as a large comparison holds it, then the panel's Cancel.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.changes(window, cx)));
        draw(cx);
        click(cx, "github-cancel");
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle_panel(&app, cx).await;
        draw(cx);
        draw(cx);
        assert!(
            !app.read_with(cx, |app, _| app.revision_inspection.is_active()),
            "no comparison opens"
        );
        assert_eq!(
            panel.read_with(cx, |panel, _| (panel.error.clone(), panel.cancelled)),
            (None, Some(line)),
            "it says it was cancelled, not that it failed"
        );
        assert!(cx.debug_bounds("github-error").is_none(), "no failure line");
        let cancelled = cx
            .debug_bounds("github-cancelled")
            .expect("the cancel's line");
        assert_eq!(
            written::get("github-cancelled").map(|line| line.node.role()),
            Some(Role::Status),
            "a status"
        );
        assert_eq!(
            announced(cx).map(|(_, text)| text).as_deref(),
            Some(line),
            "said politely"
        );
        assert_eq!(alerted(cx), None, "not as an alert");

        // A captured base Git cannot take fails before the bar's Cancel reaches the
        // comparison. Sending it clears the cancel's line, as it clears a failure.
        let mut broken = pull;
        broken.base.sha = String::new();
        cx.update(|_, cx| panel.update(cx, |panel, _| panel.pull = Some(broken)));
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.changes(window, cx)));
        draw(cx);
        assert!(
            cx.debug_bounds("github-cancelled").is_none(),
            "the next request clears the cancel's line"
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.cancel_operation(window, cx)));
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle_panel(&app, cx).await;
        draw(cx);
        draw(cx);
        assert_eq!(
            panel.read_with(cx, |panel, _| panel.cancelled),
            None,
            "a failure is no cancel"
        );
        let failure = cx
            .debug_bounds("github-error")
            .expect("the failure it met shows");
        assert_eq!(
            failure.top(),
            cancelled.top(),
            "the cancel's line stood in the failure's place"
        );
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
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

    /// The offline fixture's notice, as the panel opens with it.
    const OFFLINE: &str = "Offline review fixture · fixed sample data; account connection and \
                           network transport are unavailable in this mode.";
    /// The notice Move fixture head leaves.
    const MOVED: &str = "Fixture head moved remotely. This view and its draft retain the \
                         original commit. Submit to verify stale-head refusal, or Refresh PRs \
                         and reopen to inspect the new head.";

    /// A window on an empty repository and the panel on the offline fixture, closed and warm
    /// with the fixture's notice, for [`open_notice_panel`]. Dialogs open at rest, so a click
    /// lands where the last frame drew its button. Keep the returned directory until the test
    /// ends.
    fn notice_panel(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        Entity<Panel>,
        &mut VisualTestContext,
    ) {
        let (fixture, app, cx) = repository_window(cx);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let panel = cx.update(|window, cx| {
            let repo = app.read(cx).repository.clone().unwrap();
            let panel = cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx));
            panel.update(cx, |panel, cx| {
                panel.review.fixture = true;
                panel.account = Some("offline-reviewer".into());
                panel.notice = Some(OFFLINE.into());
                panel.destination.update(cx, |input, cx| {
                    input.set_value("gitturtle-fixture/native-review", window, cx)
                });
                panel.close(window, cx);
            });
            panel
        });
        (fixture, app, panel, cx)
    }

    /// Opens the warm panel at `interface` pt in a `width` × `height` window, its body at its
    /// top.
    async fn open_notice_panel(
        app: &Entity<GitTurtle>,
        panel: &Entity<Panel>,
        (interface, width, height): (u8, f32, f32),
        cx: &mut VisualTestContext,
    ) {
        use crate::tags::tests::draw;
        cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
        cx.simulate_resize(size(px(width), px(height)));
        draw(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
        settle_panel(app, cx).await;
        let scroll = panel.read_with(cx, |panel, _| panel.review.body_scroll.clone());
        scroll.set_offset(point(px(0.), px(0.)));
        draw(cx);
        draw(cx);
    }

    /// Scrolls the body so the notice's top lies at `top`, as far as the body scrolls.
    fn scroll_notice_to(cx: &mut VisualTestContext, scroll: &ScrollHandle, top: Pixels) {
        let notice = cx.debug_bounds("github-notice").expect("the notice");
        let offset =
            (scroll.offset().y + (top - notice.top())).clamp(-scroll.max_offset().y, px(0.));
        scroll.set_offset(point(px(0.), offset));
        crate::tags::tests::draw(cx);
        crate::tags::tests::draw(cx);
    }

    /// The window's scale: one pixel in AccessKit's physical bounds.
    fn pixel(cx: &mut VisualTestContext) -> f64 {
        f64::from(cx.update(|window, _| window.scale_factor()))
    }

    /// `bounds` in the physical window pixels of AccessKit's bounds.
    fn physical_rect(cx: &mut VisualTestContext, bounds: Bounds<Pixels>) -> gpui::accesskit::Rect {
        let scale = pixel(cx);
        let edge = |value: Pixels| f64::from(f32::from(value)) * scale;
        gpui::accesskit::Rect {
            x0: edge(bounds.left()),
            y0: edge(bounds.top()),
            x1: edge(bounds.right()),
            y1: edge(bounds.bottom()),
        }
    }

    /// Whether two rectangles in physical pixels agree to half a pixel.
    fn same_rect(a: gpui::accesskit::Rect, b: gpui::accesskit::Rect) -> bool {
        [(a.x0, b.x0), (a.y0, b.y0), (a.x1, b.x1), (a.y1, b.y1)]
            .into_iter()
            .all(|(a, b)| (a - b).abs() <= 0.5)
    }

    /// Every node an assistive technology reaches in the window's tree as the next frame
    /// writes it, through AccessKit's filter, the dialog's own node added for the dialog.
    fn reachable_nodes(cx: &mut VisualTestContext) -> Vec<native_accessibility::Reachable> {
        use crate::native_accessibility::{reachable, written};
        written::forget();
        crate::tags::tests::draw(cx);
        let size = cx.update(|window, _| window.viewport_size());
        let window = physical_rect(cx, Bounds::new(point(px(0.), px(0.)), size));
        let tree = written::Assembly::default()
            .node("dialog", gpui::accesskit::Node::new(Role::Dialog), None)
            .everything(Some("dialog"))
            .tree(window, None);
        reachable(tree)
    }

    /// The panel's notice, a status, reports the part of itself the panel's scrolling body
    /// draws, one pixel inside the body at its fold where the body draws less than a pixel of
    /// it, and where the body draws none of it, its layout: where it lies. It reported its
    /// layout where the body cut it, so at 18 pt in a 461 × 490 window AT-SPI gave a notice
    /// the fold cut y 569–731, over Back to repository; then an empty rectangle on the body's
    /// edge where the body drew none of it, which AccessKit keeps as the child past the fold
    /// and AT-SPI reported 0 px tall at y 557; then, half a pixel above the fold, its layout
    /// over Back to repository again, showing. On Linux the notice the body draws none of is
    /// visible but not showing on AT-SPI, at the extents of its layout, and one it draws some
    /// of shows: reported where it lies and showing, it read as shown over Back to repository.
    #[gpui::test]
    async fn the_notice_reports_the_part_the_panel_draws(cx: &mut TestAppContext) {
        use crate::native_accessibility::written;
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let scroll = panel.read_with(cx, |panel, _| panel.review.body_scroll.clone());
        // The notice's node, the part of the body the window draws and the notice's layout,
        // as the next frame writes them.
        let measure = |cx: &mut VisualTestContext| {
            written::forget();
            crate::tags::tests::draw(cx);
            let body = native_accessibility::clipping_node("github-panel")
                .expect("the body clips what it holds")
                .bounds()
                .expect("bounds");
            let node = written::get("github-notice")
                .expect("the notice writes its node")
                .node;
            assert_eq!(node.role(), Role::Status);
            let layout = cx.debug_bounds("github-notice").expect("the notice");
            (
                node.bounds().expect("bounds"),
                body,
                physical_rect(cx, layout),
            )
        };
        let mut cut = 0;
        for sizes in [(13, 461., 490.), (18, 461., 490.), (13, 1480., 980.)] {
            let (interface, width, height) = sizes;
            let at = format!("{interface} pt in {width} × {height}");
            open_notice_panel(&app, &panel, sizes, cx).await;
            let scale = pixel(cx);
            let (notice, body, layout) = measure(cx);
            let (top, bottom) = (layout.y0.max(body.y0), layout.y1.min(body.y1));
            #[cfg(target_os = "linux")]
            {
                use crate::native_accessibility::atspi_state_tests::{next_frame, reached};
                use accesskit_atspi_common::State;
                let found = reached(next_frame(cx));
                // The notice's own node, not the window's announcement of it,
                // which the test's tree records without bounds.
                let shown = found
                    .iter()
                    .find(|found| {
                        found.name == OFFLINE
                            && found.extents != accesskit_atspi_common::Rect::INVALID
                    })
                    .unwrap_or_else(|| panic!("{at}: AT-SPI reaches the notice"));
                assert_eq!(
                    (shown.extents.y, shown.extents.y + shown.extents.height),
                    (notice.y0 as i32, notice.y1 as i32),
                    "{at}: AT-SPI reports the notice at its node's bounds"
                );
                assert_eq!(
                    (
                        shown.state.contains(State::Visible),
                        shown.state.contains(State::Showing)
                    ),
                    (true, bottom > top),
                    "{at}: the notice is visible, and shows only where the body draws it: {:?}",
                    shown.state
                );
            }
            if bottom - top >= scale {
                assert!(
                    (notice.y0 - top).abs() <= 0.5 && (notice.y1 - bottom).abs() <= 0.5,
                    "{at}: the notice reports {notice:?}, not the part of it the body draws, \
                     {top}–{bottom}"
                );
            } else if bottom > top {
                assert!(
                    (notice.y1 - notice.y0 - scale).abs() <= 0.01
                        && notice.y0 >= body.y0 - 0.01
                        && notice.y1 <= body.y1 + 0.01,
                    "{at}: the notice the body draws less than a pixel of reports {notice:?}, \
                     not one pixel inside the body"
                );
            } else {
                assert!(
                    same_rect(notice, layout),
                    "{at}: the notice the body draws none of reports {notice:?}, not where it \
                     lies, {layout:?}"
                );
            }
            if layout.y0 >= body.y1 {
                // Below the fold. Scrolled so the fold cuts it 10 px below its top, it reports
                // those 10 px, while its layout runs on past the fold.
                let fold = px((body.y1 / scale) as f32);
                scroll_notice_to(cx, &scroll, fold - px(10.));
                let (notice, body, layout) = measure(cx);
                assert!(
                    layout.y1 > body.y1 + scale,
                    "{at}: the fold cuts the notice"
                );
                assert!(
                    (notice.y0 - layout.y0).abs() <= 0.5
                        && (notice.y1 - body.y1).abs() <= 0.5
                        && (notice.y1 - notice.y0 - 10. * scale).abs() <= 0.5,
                    "{at}: the notice the fold cuts reports {notice:?}, not its 10 px above \
                     the fold at {}",
                    body.y1
                );
                // Half a pixel above the fold, it reports one pixel inside the body at the
                // fold, where the body draws it, and shows; its layout runs on over Back to
                // repository.
                scroll_notice_to(cx, &scroll, fold - px(0.5));
                let (notice, body, layout) = measure(cx);
                assert!(
                    layout.y0 < body.y1 && body.y1 - layout.y0 < scale,
                    "{at}: the body draws less than a pixel of the notice {layout:?}, above its \
                     fold at {}",
                    body.y1
                );
                assert!(
                    (notice.y0 - (body.y1 - scale)).abs() <= 0.01
                        && (notice.y1 - body.y1).abs() <= 0.01,
                    "{at}: the notice half a pixel above the fold reports {notice:?}, not one \
                     pixel inside the body at its fold, {}",
                    body.y1
                );
                #[cfg(target_os = "linux")]
                {
                    use crate::native_accessibility::atspi_state_tests::{next_frame, reached};
                    use accesskit_atspi_common::State;
                    let found = reached(next_frame(cx));
                    // The notice's own node, not the window's announcement.
                    let shown = found
                        .iter()
                        .find(|found| {
                            found.name == OFFLINE
                                && found.extents != accesskit_atspi_common::Rect::INVALID
                        })
                        .unwrap_or_else(|| panic!("{at}: AT-SPI reaches the notice"));
                    assert_eq!(
                        (shown.extents.y, shown.extents.y + shown.extents.height),
                        (notice.y0 as i32, notice.y1 as i32),
                        "{at}: AT-SPI reports the notice half a pixel above the fold inside the \
                         body"
                    );
                    assert!(
                        shown.state.contains(State::Visible | State::Showing),
                        "{at}: the notice the body draws half a pixel of shows: {:?}",
                        shown.state
                    );
                }
                cut += 1;
            }
            escape(cx);
        }
        assert!(
            cut > 0,
            "the body's fold lies above the notice at some size"
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// No node an assistive technology reaches in the panel reports an empty rectangle, or one
    /// less than a pixel across, at 13 and 18 pt in a 461 × 490 window: with the notice, which
    /// at 18 pt lies below the body's fold, the child past it that AccessKit keeps, and
    /// reports where it lies; without it, where the pull request list is that child; and with
    /// a failure's alert, which is announced at once as it appears. Each undrawn one reported
    /// an empty rectangle on the body's edge (y 557 at 18 pt), as the pager's "Page 1 · up to
    /// 50 PRs" did at 13 pt. The alert's own node is no live region: the window's alert says it.
    /// The list past the fold reports a pixel beyond a corner, so no platform's filter reaches
    /// its pull requests: reported at its layout, it kept #42 and #43 reachable, showing on
    /// AT-SPI and activatable, though the body draws neither.
    #[gpui::test]
    async fn no_panel_node_reports_an_empty_rectangle(cx: &mut TestAppContext) {
        use crate::native_accessibility::written;
        const FAILURE: &str = "Could not read repository remotes and saved drafts.";
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let reach = |cx: &mut VisualTestContext, at: &str| {
            let found = reachable_nodes(cx);
            let scale = pixel(cx);
            for node in &found {
                if let Some(bounds) = node.bounds {
                    assert!(
                        bounds.x1 - bounds.x0 >= scale - 0.01
                            && bounds.y1 - bounds.y0 >= scale - 0.01,
                        "{at}: {:?} {:?} is reached at {bounds:?}, less than a pixel across",
                        node.role,
                        node.name
                    );
                }
            }
            found
        };
        // A node reached past the fold of its clipping ancestor.
        let past_fold = |node: &native_accessibility::Reachable| {
            node.bounds
                .zip(node.clip)
                .is_some_and(|(bounds, clip)| bounds.y0 >= clip.y1 - 0.01)
        };
        for sizes in [(13, 461., 490.), (18, 461., 490.)] {
            let (interface, width, height) = sizes;
            let at = format!("{interface} pt in {width} × {height}");
            cx.update(|_, cx| {
                panel.update(cx, |panel, _| {
                    panel.notice = Some(OFFLINE.into());
                    panel.error = None;
                })
            });
            open_notice_panel(&app, &panel, sizes, cx).await;
            let found = reach(cx, &format!("{at}, with the notice"));
            // The notice's own line, no live region, not the window's
            // announcement of it.
            let notice = found.iter().find(|node| {
                node.name.as_deref() == Some(OFFLINE) && node.live == gpui::accesskit::Live::Off
            });
            if interface == 18 {
                let notice = notice.unwrap_or_else(|| panic!("{at}: AccessKit keeps the notice"));
                assert!(
                    past_fold(notice),
                    "{at}: the notice reports where it lies, past the fold: {notice:?}"
                );
            }

            cx.update(|_, cx| {
                panel.update(cx, |panel, cx| {
                    panel.notice = None;
                    if interface == 18 {
                        let pull = github::review::fixture_pull(false);
                        let mut other = pull.clone();
                        other.number = 43;
                        panel.rows = vec![pull, other];
                    }
                    cx.notify();
                })
            });
            let found = reach(cx, &format!("{at}, without the notice"));
            if interface == 18 {
                assert!(
                    found
                        .iter()
                        .any(|node| node.role == Role::ScrollView && past_fold(node)),
                    "{at}: the pull request list is kept past the fold where it lies: {found:?}"
                );
                // The body draws none of the list, which reports a pixel beyond a corner,
                // outside which its rows lie, so no platform's filter reaches a row.
                let scale = pixel(cx);
                let list = native_accessibility::clipping_node("github-pr-list")
                    .and_then(|node| node.bounds())
                    .expect("the list clips its rows");
                let layout = cx.debug_bounds("github-pr-list").expect("the list");
                let layout = physical_rect(cx, layout);
                assert!(
                    native_accessibility::beyond_a_corner(list, layout, scale),
                    "{at}: the list the body draws none of reports {list:?}, not a pixel \
                     beyond a corner of its {layout:?}"
                );
                let rows = panel.read_with(cx, |panel, _| {
                    panel
                        .rows
                        .iter()
                        .map(|pull| format!("#{} · {}", pull.number, pull.title))
                        .collect::<Vec<_>>()
                });
                assert!(
                    !rows.is_empty() && cx.debug_bounds("github-pr-42").is_some(),
                    "{at}: the list lays out its pull requests"
                );
                for row in &rows {
                    assert!(
                        !found.iter().any(|node| node.name.as_deref() == Some(row)),
                        "{at}: {row}, which the body does not draw, is out of reach: {found:?}"
                    );
                }
                #[cfg(target_os = "linux")]
                {
                    use crate::native_accessibility::atspi_state_tests::{next_frame, reached};
                    use accesskit_atspi_common::State;
                    for node in reached(next_frame(cx))
                        .iter()
                        .filter(|node| rows.contains(&node.name))
                    {
                        assert!(
                            node.state.contains(State::Visible)
                                && !node.state.contains(State::Showing),
                            "{at}: AT-SPI reports {} showing: {:?}",
                            node.name,
                            node.state
                        );
                    }
                }
                cx.update(|_, cx| panel.update(cx, |panel, _| panel.rows.clear()));
            }

            #[cfg(target_os = "linux")]
            let mut bus = {
                use crate::native_accessibility::atspi_state_tests::{Bus, next_frame};
                Bus::new(next_frame(cx))
            };
            cx.update(|_, cx| {
                panel.update(cx, |panel, cx| {
                    panel.error = Some(FAILURE.into());
                    cx.notify();
                })
            });
            // The frame that draws the failure's line, before the window's alert.
            let found = reach(cx, &format!("{at}, with a failure"));
            assert_eq!(
                written::announced("github-error"),
                None,
                "{at}: the failure's line is no live region of its own"
            );
            assert!(
                found
                    .iter()
                    .any(|node| node.role == Role::Status && node.name.as_deref() == Some(FAILURE)),
                "{at}: the failure's line is in reach"
            );
            crate::tags::tests::draw(cx);
            assert_eq!(
                crate::copy_feedback::tests::alerted(cx)
                    .map(|(_, text)| text)
                    .as_deref(),
                Some(FAILURE),
                "{at}: the window's alert says the failure"
            );
            #[cfg(target_os = "linux")]
            {
                use crate::native_accessibility::atspi_state_tests::next_frame;
                bus.update(next_frame(cx));
                assert_eq!(
                    bus.announced()
                        .iter()
                        .filter(|text| *text == FAILURE)
                        .count(),
                    1,
                    "{at}: AT-SPI announces the failure once, from the window's alert"
                );
            }
            cx.update(|_, cx| panel.update(cx, |panel, _| panel.error = None));
            escape(cx);
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The window's announcement says the panel's notice as the panel opens with it, again as
    /// it reopens with it unchanged, and as it changes, wherever the body has scrolled it:
    /// Move fixture head changes it from the banner while, at 18 pt in a 461 × 490 window, it
    /// lies below the fold. Scrolling the body to it announces nothing, and the notice is no
    /// live region of its own. A copy button keeps reading Copied for its 1.5 s through a
    /// notice, which announced as a confirmation without a button would end it at once.
    #[gpui::test]
    async fn the_window_announces_the_panels_notice(cx: &mut TestAppContext) {
        use crate::copy_feedback::{
            RESTORE_AFTER,
            tests::{Shows, announced, click, shows, wait},
        };
        use crate::native_accessibility::written;
        use crate::tags::tests::draw;
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let scroll = panel.read_with(cx, |panel, _| panel.review.body_scroll.clone());
        for sizes in [(13, 461., 490.), (18, 461., 490.)] {
            let (interface, width, height) = sizes;
            let at = format!("{interface} pt in {width} × {height}");
            cx.update(|_, cx| {
                panel.update(cx, |panel, _| {
                    panel.notice = Some(OFFLINE.into());
                    panel.review.fixture_moved = false;
                })
            });
            open_notice_panel(&app, &panel, sizes, cx).await;
            assert_eq!(
                announced(cx).map(|(_, text)| text).as_deref(),
                Some(OFFLINE),
                "{at}: the panel announces the notice it opens with"
            );
            assert_eq!(
                written::announced("github-notice"),
                None,
                "{at}: the notice is no live region of its own"
            );

            click(cx, "github-fixture-move");
            draw(cx);
            draw(cx);
            let (heard, notice) = announced(cx).expect("an announcement");
            assert_eq!(notice, MOVED, "{at}: the changed notice is announced");
            let body = cx.debug_bounds("github-panel").expect("the body");
            let shown = cx.debug_bounds("github-notice").expect("the notice");
            assert_eq!(
                shown.top() < body.bottom(),
                interface == 13,
                "{at}: the body draws the changed notice at 13 pt only"
            );

            scroll_notice_to(cx, &scroll, body.top());
            assert_eq!(
                announced(cx).map(|(id, _)| id),
                Some(heard.clone()),
                "{at}: scrolling the notice into view announces nothing"
            );

            // Closed until the announcement has gone, and reopened with the notice unchanged.
            escape(cx);
            wait(cx, RESTORE_AFTER);
            assert_eq!(announced(cx), None, "{at}: the announcement has gone");
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
            settle_panel(&app, cx).await;
            draw(cx);
            draw(cx);
            let (again, notice) = announced(cx).expect("an announcement");
            assert_eq!(
                (notice.as_str(), again != heard),
                (MOVED, true),
                "{at}: the reopened panel announces its notice afresh"
            );
            escape(cx);
        }

        // A copy, then a notice.
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.notice = Some(OFFLINE.into());
                panel.review.fixture_moved = false;
            })
        });
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;
        click(cx, "github-account-options");
        cx.run_until_parked();
        draw(cx);
        click(cx, "github-sign-in-help");
        assert_eq!(shows(cx, "github-sign-in-help"), Shows::Copied);
        click(cx, "github-fixture-move");
        draw(cx);
        draw(cx);
        assert_eq!(
            announced(cx).map(|(_, text)| text).as_deref(),
            Some(MOVED),
            "the notice is announced"
        );
        assert_eq!(
            shows(cx, "github-sign-in-help"),
            Shows::Copied,
            "the sign-in command still reads Copied"
        );
        wait(cx, RESTORE_AFTER);
        assert_eq!(shows(cx, "github-sign-in-help"), Shows::Rest);
        assert_eq!(announced(cx), None);
        escape(cx);
    }

    /// The window's alert says the panel's failure and refused input at once, as they appear
    /// or change, wherever the body has scrolled their lines, which are no live regions of
    /// their own. A failure the body draws is announced once, from the window's alert alone,
    /// and drawing the panel again says nothing more. A failure landing while the body has
    /// scrolled far past its line, which AccessKit's filter leaves out, was never announced.
    /// A copy button keeps reading Copied for its 1.5 s through an alert. The lines are
    /// statuses rather than alerts: AT-SPI reports a line becoming showing as the body
    /// scrolls it into view, and Orca presents an alert, a notification there, when it does.
    #[gpui::test]
    async fn the_window_alerts_the_panels_failures(cx: &mut TestAppContext) {
        use crate::copy_feedback::{
            RESTORE_AFTER,
            tests::{Shows, alerted, click, shows, wait},
        };
        use crate::native_accessibility::written;
        use crate::tags::tests::draw;
        const FAILURE: &str = "Could not read repository remotes and saved drafts.";
        const LATER: &str = "Conversations could not be loaded.";
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let scroll = panel.read_with(cx, |panel, _| panel.review.body_scroll.clone());
        let fail = |cx: &mut VisualTestContext, error: Option<&str>| {
            cx.update(|_, cx| {
                panel.update(cx, |panel, cx| {
                    panel.error = error.map(str::to_owned);
                    cx.notify();
                })
            });
        };

        // At 13 pt in a 461 × 490 window the body draws the failure's line.
        open_notice_panel(&app, &panel, (13, 461., 490.), cx).await;
        #[cfg(target_os = "linux")]
        let mut bus = {
            use crate::native_accessibility::atspi_state_tests::{Bus, next_frame};
            Bus::new(next_frame(cx))
        };
        fail(cx, Some(FAILURE));
        draw(cx);
        draw(cx);
        let body = cx.debug_bounds("github-panel").expect("the body");
        let line = cx.debug_bounds("github-error").expect("the failure's line");
        assert!(
            line.top() < body.bottom() && line.bottom() > body.top(),
            "the body draws the failure's line"
        );
        let (heard, text) = alerted(cx).expect("an alert");
        assert_eq!(text, FAILURE, "the window's alert says the failure");
        assert_eq!(
            written::announced("github-error"),
            None,
            "the failure's line is no live region of its own"
        );
        assert_eq!(
            written::get("github-error").map(|line| line.node.role()),
            Some(Role::Status),
            "the failure's line is a status, no alert"
        );
        #[cfg(target_os = "linux")]
        {
            use crate::native_accessibility::atspi_state_tests::{next_frame, reached};
            use accesskit_atspi_common::Role as AtspiRole;
            let frame = next_frame(cx);
            let roles: Vec<AtspiRole> = reached(frame.clone())
                .into_iter()
                .filter(|node| node.name == FAILURE)
                .map(|node| node.role)
                .collect();
            assert!(
                roles.contains(&AtspiRole::StatusBar)
                    && roles
                        .iter()
                        .filter(|role| **role == AtspiRole::Notification)
                        .count()
                        <= 1,
                "the failure's line is a status bar on AT-SPI, and only the window's alert a \
                 notification, which Orca presents as it becomes showing: {roles:?}"
            );
            bus.update(frame);
            assert_eq!(
                bus.announced(),
                vec![FAILURE.to_owned()],
                "AT-SPI announces the failure once"
            );
        }
        cx.update(|_, cx| panel.update(cx, |_, cx| cx.notify()));
        draw(cx);
        draw(cx);
        assert_eq!(
            alerted(cx).map(|(id, _)| id),
            Some(heard),
            "drawing the panel again alerts nothing new"
        );
        #[cfg(target_os = "linux")]
        {
            use crate::native_accessibility::atspi_state_tests::next_frame;
            bus.update(next_frame(cx));
            assert_eq!(bus.announced(), Vec::<String>::new(), "nor on AT-SPI");
        }

        // Refused input is said at once too.
        cx.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                panel.draft_issue = Some(DraftIssue::Destination);
                cx.notify();
            })
        });
        draw(cx);
        draw(cx);
        assert_eq!(
            alerted(cx).map(|(_, text)| text).as_deref(),
            Some(DraftIssue::Destination.message()),
            "the window's alert says the refused input"
        );
        assert_eq!(written::announced("github-draft-validation"), None);
        assert_eq!(
            written::get("github-draft-validation").map(|line| line.node.role()),
            Some(Role::Status),
            "the refused input's line is a status, no alert"
        );
        escape(cx);
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.error = None;
                panel.draft_issue = None;
            })
        });

        // A PR's Overview at 18 pt, the body scrolled to its end, far below the failure's line.
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.pull = Some(github::review::fixture_pull(false));
                panel.review.section = Section::Overview;
                panel.review.show_list = false;
            })
        });
        open_notice_panel(&app, &panel, (18, 461., 490.), cx).await;
        scroll.set_offset(point(px(0.), -scroll.max_offset().y));
        draw(cx);
        draw(cx);
        fail(cx, Some(LATER));
        let found = reachable_nodes(cx);
        let body = cx.debug_bounds("github-panel").expect("the body");
        let line = cx.debug_bounds("github-error").expect("the failure's line");
        assert!(
            line.bottom() < body.top(),
            "the body has scrolled past the failure's line: {line:?} above {body:?}"
        );
        assert!(
            !found
                .iter()
                .any(|node| node.bounds.is_some() && node.name.as_deref() == Some(LATER)),
            "AccessKit's filter leaves out the failure's line"
        );
        draw(cx);
        assert_eq!(
            alerted(cx).map(|(_, text)| text).as_deref(),
            Some(LATER),
            "the window's alert says the failure the body has scrolled past"
        );
        escape(cx);
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.error = None;
                panel.pull = None;
            })
        });

        // A copy, then a failure.
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;
        click(cx, "github-account-options");
        cx.run_until_parked();
        draw(cx);
        click(cx, "github-sign-in-help");
        assert_eq!(shows(cx, "github-sign-in-help"), Shows::Copied);
        fail(cx, Some(FAILURE));
        draw(cx);
        draw(cx);
        assert_eq!(alerted(cx).map(|(_, text)| text).as_deref(), Some(FAILURE));
        assert_eq!(
            shows(cx, "github-sign-in-help"),
            Shows::Copied,
            "the sign-in command still reads Copied"
        );
        wait(cx, RESTORE_AFTER);
        assert_eq!(shows(cx, "github-sign-in-help"), Shows::Rest);
        assert_eq!(alerted(cx), None);
        escape(cx);
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// While a reply is open, the failure of what was asked from its composer, or that Cancel
    /// stopped it ([`Origin::Reply`]), stands in the reply's own composer, under the draft and
    /// above Review reply…, where the reader writing the reply looks and where a sent reply's
    /// refusal answers: beside the draft, never over it, whole in view with it wherever the
    /// composer fits the body, at 13 pt in a 461 × 490 window and 18 pt in a 1,480 × 800 one.
    /// The line at the panel's top, far above the conversation, is not drawn then. The window
    /// says the failure once, as an alert, and the cancel politely.
    #[gpui::test]
    async fn a_failure_stands_in_the_reply_composer(cx: &mut TestAppContext) {
        use crate::copy_feedback::tests::{alerted, announced};
        use crate::tags::tests::draw;
        const FAILURE: &str = "The captured reply context is unavailable or changed.";
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let pull = github::review::fixture_pull(false);
        let identity = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let target = page.items[0].target.clone();
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Overview;
                panel.receive_threads(page, None);
                panel.conversations.active = Some(target.clone());
            })
        });
        let set = |cx: &mut VisualTestContext, error: Option<&str>, cancelled| {
            cx.update(|_, cx| {
                panel.update(cx, |panel, cx| {
                    // As a request from the reply's composer answers.
                    panel.origin = Origin::Reply(Box::new(target.clone()));
                    panel.error = error.map(str::to_owned);
                    panel.cancelled = cancelled;
                    cx.notify();
                })
            });
            draw(cx);
            draw(cx);
        };
        let beside_the_draft = |cx: &mut VisualTestContext, id: &'static str, at: &str| {
            let body = cx.debug_bounds("github-panel").expect("the body");
            let composer = cx
                .debug_bounds("github-reply-composer")
                .expect("the reply's composer");
            let context = cx
                .debug_bounds("github-reply-context")
                .expect("the reply's context");
            let line = cx.debug_bounds(id).expect("the line");
            let draft = cx.update(|_, _| appearance::ui_size(100.));
            assert!(
                composer.top() <= line.top() && line.bottom() <= composer.bottom(),
                "{at}: {id} {line:?} stands in the composer {composer:?}"
            );
            assert!(
                line.top() >= context.bottom() + draft,
                "{at}: {id} {line:?} lies under the draft, below {context:?} by its {draft:?}"
            );
            assert!(
                body.top() <= context.bottom() && line.bottom() <= body.bottom(),
                "{at}: {id} {line:?} is whole in view with the draft below {context:?} in \
                 {body:?}"
            );
            assert!(
                cx.debug_bounds("github-error").is_none()
                    && cx.debug_bounds("github-cancelled").is_none(),
                "{at}: the panel's top line is not drawn"
            );
        };
        for (interface, width, height) in [(13, 461., 490.), (18, 1480., 800.)] {
            let at = format!("{interface} pt in {width} × {height}");
            open_notice_panel(&app, &panel, (interface, width, height), cx).await;
            // The reader is writing the reply.
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    // As Reply does, which reveals the draft.
                    *panel.review.revealed_focus.borrow_mut() = Default::default();
                    let input = panel.conversations.input.read(cx).focus_handle(cx);
                    window.focus(&input, cx);
                    cx.notify();
                })
            });
            draw(cx);
            draw(cx);

            set(cx, Some(FAILURE), None);
            beside_the_draft(cx, "github-reply-error", &at);
            let (heard, text) = alerted(cx).expect("an alert");
            assert_eq!(text, FAILURE, "{at}: the window says the failure");
            cx.update(|_, cx| panel.update(cx, |_, cx| cx.notify()));
            draw(cx);
            draw(cx);
            assert_eq!(
                alerted(cx).map(|(id, _)| id),
                Some(heard),
                "{at}: and says it once"
            );

            let line = RequestKind::Compare.cancelled(false);
            set(cx, None, line);
            beside_the_draft(cx, "github-reply-cancelled", &at);
            assert_eq!(
                announced(cx).map(|(_, text)| text).as_deref(),
                line,
                "{at}: the cancel is said politely"
            );
            escape(cx);
            set(cx, None, None);
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// While a reply is open, an outcome answers on the control the person touched
    /// ([`Origin`]): a PR's Changes, asked for from Overview's Open captured local comparison
    /// and stopped by the panel's Cancel, says it was cancelled on the panel's top line, above
    /// the conversation, where every outcome once answered under the reply's draft while a
    /// conversation was open; a reply's Send that is refused still answers in the reply's
    /// composer, under its draft.
    #[gpui::test]
    async fn outcomes_answer_where_they_were_asked_for_while_a_reply_is_open(
        cx: &mut TestAppContext,
    ) {
        use crate::copy_feedback::tests::click;
        use crate::views::tests::draw;
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        // Opened at rest, so a click lands where the last frame drew its button.
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
        let pull = local_pull(&app, cx);
        let identity = github::review::fixture_pull(false)
            .capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let target = page.items[0].target.clone();
        cx.update(|window, cx| {
            cx.set_reduce_motion(true);
            panel.update(cx, |panel, cx| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Overview;
                panel.receive_threads(page, None);
                panel.conversations.active = Some(target.clone());
                panel
                    .conversations
                    .input
                    .update(cx, |input, cx| input.set_value("Exact reply", window, cx));
            })
        });
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;
        let composer = cx
            .debug_bounds("github-reply-composer")
            .expect("the reply is open");

        // Open captured local comparison, held as a large comparison holds it, then the
        // panel's Cancel.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.changes(window, cx)));
        draw(cx);
        click(cx, "github-cancel");
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle_panel(&app, cx).await;
        draw(cx);
        draw(cx);
        assert_eq!(
            panel.read_with(cx, |panel, _| (panel.error.clone(), panel.cancelled)),
            (None, RequestKind::Compare.cancelled(false)),
            "Changes was cancelled"
        );
        let line = cx
            .debug_bounds("github-cancelled")
            .expect("the cancel answers on the panel's top line");
        assert!(
            line.bottom() <= composer.top(),
            "the line {line:?} stands above the conversation, not in the reply {composer:?}"
        );
        assert!(
            cx.debug_bounds("github-reply-cancelled").is_none(),
            "nothing answers under the draft"
        );

        // The reply's Send, refused: the fixture's head moved.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.review.fixture_moved = true;
                panel.confirm = Some(Confirmation {
                    account: target.account.clone(),
                    action: Action::Reply {
                        target: target.clone(),
                        body: "Exact reply".into(),
                    },
                });
                panel.submit(window, cx);
            })
        });
        app.read_with(cx, |app, _| app.operations.drain());
        settle_panel(&app, cx).await;
        draw(cx);
        draw(cx);
        assert!(
            panel.read_with(cx, |panel, _| panel.error.is_some()),
            "the reply is refused"
        );
        let composer = cx
            .debug_bounds("github-reply-composer")
            .expect("the reply stays open");
        let line = cx
            .debug_bounds("github-reply-error")
            .expect("the refusal answers in the reply's composer");
        assert!(
            composer.top() <= line.top() && line.bottom() <= composer.bottom(),
            "the refusal {line:?} stands in the composer {composer:?}"
        );
        assert!(
            cx.debug_bounds("github-error").is_none()
                && cx.debug_bounds("github-cancelled").is_none(),
            "the panel's top line is not drawn"
        );
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
    }

    /// A reply refused on one conversation stays that conversation's ([`Origin::Reply`]):
    /// Reply on another opens its composer with nothing under its draft, and the refusal,
    /// whose composer is no longer drawn, stands on the panel's top line, said once. It once
    /// stood under the other conversation's draft, which no one had reviewed.
    #[gpui::test]
    async fn a_refused_reply_stays_with_its_conversation(cx: &mut TestAppContext) {
        use crate::copy_feedback::tests::alerted;
        use crate::tags::tests::draw;
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let pull = github::review::fixture_pull(false);
        let identity = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let open = page
            .items
            .iter()
            .filter(|thread| thread.reply_available())
            .map(|thread| thread.target.clone())
            .collect::<Vec<_>>();
        let [first, second, ..] = &open[..] else {
            panic!("two of the fixture's conversations accept a reply");
        };
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Overview;
                panel.receive_threads(page, None);
            })
        });
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;

        // Reply on the first, then Review reply… with nothing written: refused there.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.begin_reply(first.clone(), window, cx);
                panel.review_reply(window, cx);
            })
        });
        draw(cx);
        draw(cx);
        let refusal = panel
            .read_with(cx, |panel, _| panel.error.clone())
            .expect("an empty reply is refused");
        assert!(
            cx.debug_bounds("github-reply-error").is_some(),
            "the refusal answers in the first conversation's composer"
        );
        let (heard, text) = alerted(cx).expect("an alert");
        assert_eq!(text, refusal, "the window says the refusal");

        // Reply on the second.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.begin_reply(second.clone(), window, cx)
            })
        });
        draw(cx);
        draw(cx);
        assert_eq!(
            panel.read_with(cx, |panel, _| panel.conversations.active.clone()),
            Some(second.clone()),
            "the second conversation's composer is open"
        );
        assert!(
            cx.debug_bounds("github-reply-error").is_none()
                && cx.debug_bounds("github-reply-cancelled").is_none(),
            "nothing answers under the second conversation's draft"
        );
        assert!(
            cx.debug_bounds("github-error").is_some(),
            "the first conversation's refusal stands on the panel's top line"
        );
        assert_eq!(
            alerted(cx).map(|(id, _)| id),
            Some(heard),
            "and is said once"
        );
        escape(cx);
    }

    /// A Files row whose preparation fails says so on the panel's top line, though the last
    /// outcome before it, a refused Review reply…, answered in the open reply's composer: no
    /// request carried the preparation, so it takes none's origin. It once kept that origin
    /// and stood under the reply's draft.
    #[gpui::test]
    async fn a_file_preparation_failure_answers_on_the_top_line(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let pull = github::review::fixture_pull(false);
        let identity = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let target = page.items[0].target.clone();
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Files;
                panel.receive_threads(page, None);
                // GitHub returned a path the panel refuses to prepare.
                panel.review.files = vec![github::review::PullFile {
                    filename: "../outside".into(),
                    previous_filename: None,
                    status: "modified".into(),
                    additions: 1,
                    deletions: 0,
                    patch: Some("@@ -0,0 +1 @@\n+line\n".into()),
                    rows: Vec::new(),
                    unavailable: None,
                }];
            })
        });
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;

        // Review reply… with nothing written: refused in the reply's composer.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.begin_reply(target.clone(), window, cx);
                panel.review_reply(window, cx);
            })
        });
        draw(cx);
        draw(cx);
        assert!(
            cx.debug_bounds("github-reply-error").is_some(),
            "the refusal answers in the reply's composer"
        );

        // The file, chosen, fails to prepare off the UI thread.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| panel.select_review_file(0, window, cx))
        });
        cx.run_until_parked();
        draw(cx);
        draw(cx);
        assert_eq!(
            panel
                .read_with(cx, |panel, _| panel.error.clone())
                .as_deref(),
            Some("GitHub returned an invalid changed-file path"),
            "the preparation failed"
        );
        assert!(
            cx.debug_bounds("github-error").is_some(),
            "its failure stands on the panel's top line"
        );
        assert!(
            cx.debug_bounds("github-reply-error").is_none(),
            "nothing answers under the reply's draft"
        );
        escape(cx);
    }

    /// The connection pane's help says what Connect does on this platform, from whether its
    /// store holds GitTurtle's authorization (`SecureStore::HOLDS`): on a Mac it saves it in
    /// macOS Keychain; elsewhere, where the store saves none, it names no storage and says
    /// Connect cannot finish there. Linux once said Connect saved it in secure storage.
    #[gpui::test]
    async fn the_connection_help_says_what_connect_keeps(cx: &mut TestAppContext) {
        use crate::copy_feedback::tests::click;
        use crate::native_accessibility::written;
        use crate::tags::tests::draw;
        let (_fixture, app, panel, cx) = notice_panel(cx);
        open_notice_panel(&app, &panel, (13, 1480., 980.), cx).await;
        click(cx, "github-account-options");
        draw(cx);
        draw(cx);
        let help = written::get("github-connection-help")
            .map(|line| line.node)
            .and_then(|node| node.label().or(node.value()).map(str::to_owned))
            .expect("the connection pane says what Connect does");
        assert!(
            help.starts_with("Connect uses your current GitHub CLI account"),
            "{help}"
        );
        #[cfg(target_os = "macos")]
        assert!(
            help.contains("saves GitTurtle authorization in macOS Keychain."),
            "Connect saves the authorization where it says: {help}"
        );
        #[cfg(not(target_os = "macos"))]
        {
            for storage in ["saves", "storage", "Keychain"] {
                assert!(
                    !help.contains(storage),
                    "no storage where nothing is stored ({storage}): {help}"
                );
            }
            assert!(
                help.contains("GitTurtle stores no authorization on this platform"),
                "it says nothing is stored: {help}"
            );
        }
        escape(cx);
    }

    /// While a conversation is open, the panel's top line stands above the body
    /// ([`Panel::pin_top_line`]), and as it appears, changes or clears, nothing drawn in the
    /// conversation moves, in the very frame it changes: a PR's Changes asked for while a reply
    /// is being written, its busy row appearing, then the operation bar's Cancel and the
    /// "Cancelled" line in place of the row, and the line clearing, at 18 pt in a 461 × 490
    /// window, whose body is about 185 px tall, and at 13 pt in a 1,480 × 980 one. Focus stays
    /// in the draft, and the body scrolled to its top shows its first row whole below the
    /// line. Among the content under the toolbar, the line pushed the conversation, the draft
    /// included, down by its height.
    #[gpui::test]
    async fn the_top_line_moves_nothing_in_an_open_conversation(cx: &mut TestAppContext) {
        use crate::views::tests::draw;
        // The conversation's rows, in the order the body draws them, and its draft's composer.
        const ROWS: [&str; 10] = [
            "github-fixture-mode",
            "github-destination",
            "github-pull-title",
            "github-branch-context",
            "github-captured-identities",
            "github-threads",
            "github-open-changes",
            "github-reply-composer",
            "github-reply-context",
            "github-recovery-toggle",
        ];
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
        let pull = local_pull(&app, cx);
        let identity = github::review::fixture_pull(false)
            .capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let target = page.items[0].target.clone();
        cx.update(|_, cx| {
            cx.set_reduce_motion(true);
            panel.update(cx, |panel, _| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Overview;
                panel.receive_threads(page, None);
            })
        });
        let scroll = panel.read_with(cx, |panel, _| panel.review.body_scroll.clone());
        // Where the body places its content: its top with its scroll, which the rows lie below
        // by their places in the content.
        let placed = |scroll: &ScrollHandle| scroll.bounds().top() + scroll.offset().y;
        // `change`, then the frame that draws it, in one update, and where that frame placed
        // the body's content, read before anything the frame deferred runs. A test window draws
        // itself as an update ends, and again after what that frame deferred, so a scroll moved
        // a frame late, after a frame drawn without it, would be drawn by the time the frame's
        // bounds could be read.
        let frame =
            |cx: &mut VisualTestContext,
             change: &dyn Fn(&mut Panel, &mut Window, &mut Context<Panel>)| {
                cx.update(|window, cx| {
                    panel.update(cx, |panel, cx| {
                        change(panel, window, cx);
                        cx.notify();
                    });
                    window.simulate_next_frame(cx);
                    window.draw(cx).clear(cx);
                    placed(&scroll)
                })
            };
        let rows = |cx: &mut VisualTestContext| {
            ROWS.map(|row| (row, cx.debug_bounds(row).expect("the row is drawn")))
        };
        let in_draft = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let input = panel.read(cx).conversations.input.read(cx).focus_handle(cx);
                input.is_focused(window)
            })
        };
        for (interface, width, height) in [(18, 461., 490.), (13, 1480., 980.)] {
            let at = format!("{interface} pt in {width} × {height}");
            // As the request that loaded the PR leaves the line: empty.
            cx.update(|_, cx| {
                panel.update(cx, |panel, _| {
                    panel.notice = None;
                    panel.cancelled = None;
                })
            });
            open_notice_panel(&app, &panel, (interface, width, height), cx).await;
            // Reply, written in its draft, with the body scrolled to show the draft from its
            // context line at the body's top, so the scroll has room to move either way.
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.begin_reply(target.clone(), window, cx);
                    panel
                        .conversations
                        .input
                        .update(cx, |input, cx| input.set_value("Exact reply", window, cx));
                })
            });
            draw(cx);
            let body = cx.debug_bounds("github-panel").expect("the body");
            let context = cx
                .debug_bounds("github-reply-context")
                .expect("the draft's context");
            scroll.set_offset(point(
                px(0.),
                (scroll.offset().y - (context.top() - body.top()))
                    .clamp(-scroll.max_offset().y, px(0.)),
            ));
            draw(cx);
            let body = cx.debug_bounds("github-panel").expect("the body");
            let context = cx
                .debug_bounds("github-reply-context")
                .expect("the draft's context");
            assert!(
                context.top() >= body.top() && context.bottom() <= body.bottom(),
                "{at}: the draft's context {context:?} is in view in {body:?}"
            );
            assert!(
                cx.debug_bounds("github-top-line").is_none(),
                "{at}: the line has nothing to say"
            );
            assert!(in_draft(cx), "{at}: the draft holds focus");
            let first = ROWS
                .into_iter()
                .find(|row| {
                    cx.debug_bounds(row)
                        .is_some_and(|bounds| bounds.bottom() > body.top())
                })
                .expect("a row is in view");
            let still = rows(cx);
            let content = placed(&scroll);
            let in_frame = |drawn: Pixels, after: &str| {
                assert_eq!(
                    drawn, content,
                    "{at}: {after}, the frame placed the body's content where it was"
                );
            };
            let unmoved = |cx: &mut VisualTestContext, after: &str| {
                assert_eq!(
                    rows(cx),
                    still,
                    "{at}: {after}, nothing in the conversation moved, its first row in view \
                     {first} included"
                );
                assert!(in_draft(cx), "{at}: {after}, focus stays in the draft");
            };

            // Open captured local comparison, held as a large comparison holds it: the line
            // appears with the busy row.
            let held = app.update(cx, |app, _| app.operations.hold());
            let drawn = frame(cx, &|panel, window, cx| panel.changes(window, cx));
            in_frame(drawn, "in the frame the busy row appears");
            assert!(
                cx.debug_bounds("github-busy").is_some(),
                "{at}: the busy row stands on the line"
            );
            unmoved(cx, "in the frame the busy row appears");

            // The operation bar's Cancel, which leaves focus where it is; the "Cancelled"
            // line replaces the busy row as the comparison answers.
            cx.update(|window, cx| app.update(cx, |app, cx| app.cancel_operation(window, cx)));
            drop(held);
            app.read_with(cx, |app, _| app.operations.drain());
            settle_panel(&app, cx).await;
            assert!(
                panel.read_with(cx, |panel, _| !panel.pending && panel.cancelled.is_some()),
                "{at}: Changes was cancelled"
            );
            // The answer lands in a task, and the window draws it as the task's update ends.
            draw(cx);
            let body = cx.debug_bounds("github-panel").expect("the body");
            let cancelled = cx
                .debug_bounds("github-cancelled")
                .unwrap_or_else(|| panic!("{at}: the cancel answers on the panel's top line"));
            assert!(
                cancelled.bottom() <= body.top() && cx.debug_bounds("github-busy").is_none(),
                "{at}: the line {cancelled:?} stands above the body {body:?}"
            );
            unmoved(cx, "in the frame the cancel answers");
            draw(cx);
            unmoved(cx, "in the frames after it");

            // The body's top, under the line, is still the body's.
            let offset = scroll.offset();
            scroll.set_offset(point(px(0.), px(0.)));
            draw(cx);
            let line = cx.debug_bounds("github-top-line").expect("the line");
            let body = cx.debug_bounds("github-panel").expect("the body");
            let top = cx.debug_bounds(ROWS[0]).expect("the body's first row");
            assert!(
                top.top() >= line.bottom() && top.top() >= body.top(),
                "{at}: scrolled to its top, the body shows its first row {top:?} below the \
                 line {line:?}"
            );
            scroll.set_offset(offset);
            draw(cx);
            unmoved(cx, "scrolled back");

            // The line clears, as the next request clears it.
            let drawn = frame(cx, &|panel, _, _| panel.cancelled = None);
            in_frame(drawn, "in the frame the line clears");
            assert!(
                cx.debug_bounds("github-top-line").is_none(),
                "{at}: the line cleared"
            );
            unmoved(cx, "in the frame the line clears");
            cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
            settle_panel(&app, cx).await;
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Reply opened while the panel's top line says something takes the line from among the
    /// content ([`Panel::flow_top_line`]): a "Cancelled" moves above the body
    /// ([`Panel::pin_top_line`]), and a notice from before goes (`Panel::begin_reply`). Either
    /// way the conversation drawn below the line stays where it was in the frame the reply
    /// opens, the body scrolled partway, at 13 pt in a 461 × 490 window and in a 1,480 × 980
    /// one. The panel once took the line above the body for a new line,
    /// though its room among the content went with it, and scrolled the conversation up by
    /// the line's whole height (65 px for the fixture's notice at 461 × 490).
    #[gpui::test]
    async fn opening_a_reply_keeps_the_conversation_where_it_was(cx: &mut TestAppContext) {
        use crate::views::tests::draw;
        // Rows of the conversation below the line, above the composer that opens.
        const ROWS: [&str; 5] = [
            "github-pull-title",
            "github-branch-context",
            "github-captured-identities",
            "github-threads",
            "github-open-changes",
        ];
        let (_fixture, app, panel, cx) = busy_fixture_panel(cx).await;
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
        settle_panel(&app, cx).await;
        let pull = local_pull(&app, cx);
        let identity = github::review::fixture_pull(false)
            .capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let target = page.items[0].target.clone();
        cx.update(|_, cx| {
            cx.set_reduce_motion(true);
            panel.update(cx, |panel, _| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Overview;
                panel.receive_threads(page, None);
            })
        });
        let scroll = panel.read_with(cx, |panel, _| panel.review.body_scroll.clone());
        let placed = |scroll: &ScrollHandle| scroll.bounds().top() + scroll.offset().y;
        let rows = |cx: &mut VisualTestContext| {
            ROWS.map(|row| cx.debug_bounds(row).expect("the row is drawn").top())
        };
        for (interface, width, height) in [(13, 461., 490.), (13, 1480., 980.)] {
            for (said, notice, cancelled) in [
                ("the notice", Some(OFFLINE), None),
                ("a cancel", None, RequestKind::Compare.cancelled(false)),
            ] {
                let at = format!("{said} at {interface} pt in {width} × {height}");
                cx.update(|_, cx| {
                    panel.update(cx, |panel, _| {
                        panel.conversations.active = None;
                        panel.notice = notice.map(str::to_owned);
                        panel.cancelled = cancelled;
                        // Reply saves the drafts and says so under the section tabs, which the
                        // status already says here, so only the line changes as it opens.
                        panel.save_status = "Saving local draft…".into();
                    })
                });
                open_notice_panel(&app, &panel, (interface, width, height), cx).await;
                // The body scrolled partway: the line among the content above its top, the
                // PR's identities at it.
                let body = cx.debug_bounds("github-panel").expect("the body");
                let identities = cx
                    .debug_bounds("github-captured-identities")
                    .expect("the identities");
                scroll.set_offset(point(
                    px(0.),
                    (scroll.offset().y - (identities.top() - body.top()))
                        .clamp(-scroll.max_offset().y, px(0.)),
                ));
                draw(cx);
                assert!(
                    panel.read_with(cx, |panel, _| panel.conversations.active.is_none()),
                    "{at}: no reply is open"
                );
                let before = rows(cx);
                let content = placed(&scroll);

                // Reply, and the frame that draws it, read before the composer's reveal,
                // which the frame defers, scrolls on to the draft.
                let opened = cx.update(|window, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.begin_reply(target.clone(), window, cx)
                    });
                    window.simulate_next_frame(cx);
                    window.draw(cx).clear(cx);
                    placed(&scroll)
                });
                // Reply clears a notice from before; a "Cancelled" moves above the body.
                assert_eq!(
                    cx.debug_bounds("github-top-line").is_some(),
                    notice.is_none(),
                    "{at}: the line stands above the body only for the cancel"
                );
                // Each row where the frame that opened the reply drew it: where the content
                // places it, from where that frame placed the content.
                let after = rows(cx);
                let now = placed(&scroll);
                let drawn = after.map(|top| top - now + opened);
                assert_eq!(
                    drawn, before,
                    "{at}: the rows {ROWS:?} stayed where they were as the reply opened \
                     (content placed at {content:?}, then {opened:?})"
                );
                cx.update(|window, cx| panel.update(cx, |panel, cx| panel.close(window, cx)));
                settle_panel(&app, cx).await;
            }
        }
        cx.update(|window, cx| {
            panel.update(cx, |panel, _| panel.conversations.active = None);
            appearance::apply_text_sizes(13, 12, window, cx)
        });
    }

    /// Reply says why where the person is looking: opening a conversation's composer clears a
    /// notice from before that is not about that conversation, a resolved conversation's here,
    /// and a reply refused there shows its failure within the body, with focus still in the
    /// draft, at 13 pt in a 461 × 490 window. A notice about the conversation Reply opens, its
    /// reply just sent, stays. Pinned above the body for as long as the reply was open, the
    /// stale notice kept room the composer needed, and the failure stood below the body's clip.
    #[gpui::test]
    async fn reply_clears_a_stale_notice_and_shows_its_refusal(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let pull = github::review::fixture_pull(false);
        let identity = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let open = page
            .items
            .iter()
            .filter(|thread| thread.reply_available())
            .map(|thread| thread.target.clone())
            .collect::<Vec<_>>();
        let [resolved, other, ..] = &open[..] else {
            panic!("two of the fixture's conversations accept a reply");
        };
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Overview;
                panel.receive_threads(page, None);
            })
        });
        open_notice_panel(&app, &panel, (13, 461., 490.), cx).await;
        let in_draft = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let input = panel.read(cx).conversations.input.read(cx).focus_handle(cx);
                input.is_focused(window)
            })
        };

        // One conversation resolved, as Send to GitHub finishes it; then Reply on another.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                let action = Action::SetThreadResolved {
                    target: resolved.clone(),
                    resolved: true,
                };
                panel.finish_conversation_action(&action, window, cx);
            })
        });
        draw(cx);
        assert!(
            cx.debug_bounds("github-notice").is_some(),
            "the panel says the conversation was resolved"
        );
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| panel.begin_reply(other.clone(), window, cx))
        });
        draw(cx);
        assert!(
            cx.debug_bounds("github-notice").is_none()
                && cx.debug_bounds("github-top-line").is_none(),
            "the resolved conversation's notice went as Reply opened another"
        );

        // Review reply… with nothing written: refused in the composer, shown in the body.
        cx.update(|window, cx| panel.update(cx, |panel, cx| panel.review_reply(window, cx)));
        draw(cx);
        draw(cx);
        let body = cx.debug_bounds("github-panel").expect("the body");
        let line = cx
            .debug_bounds("github-reply-error")
            .expect("the refusal answers in the composer");
        assert!(
            body.top() <= line.top() && line.bottom() <= body.bottom(),
            "the refusal {line:?} stands within the body {body:?}"
        );
        assert!(in_draft(cx), "focus stays in the draft");

        // The reply sent, as Send to GitHub finishes it: Reply on the same conversation keeps
        // the notice about it.
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                let action = Action::Reply {
                    target: other.clone(),
                    body: panel.conversations.input.read(cx).value().to_string(),
                };
                panel.finish_conversation_action(&action, window, cx);
                panel.begin_reply(other.clone(), window, cx);
            })
        });
        draw(cx);
        assert!(
            cx.debug_bounds("github-notice").is_some(),
            "the notice about the reply sent on this conversation stays"
        );
        escape(cx);
    }

    /// The top line's text, where it is taller than its room above an open conversation, scrolls
    /// within the line, and the line's scrollbar cues the rest, its thumb in a gutter clear of
    /// the text: the fixture's notice with a long failure at 18 pt in a 461 × 490 window. A
    /// short line keeps no gutter and paints no thumb. The text once scrolled with no sign of
    /// more, a long remedy cut at the line's edge.
    #[gpui::test]
    async fn the_top_lines_long_text_cues_its_scrollbar(cx: &mut TestAppContext) {
        use crate::tags::tests::{draw, painted_thumb};
        use gpui_kit::component::scroll::Scrollbar;
        const FAILURE: &str = "PR changes need their captured head, base, and common ancestor \
                               locally. Use an explicit Fetch if needed, then reopen the PR. \
                               The comparison could not find the captured base revision in \
                               the local repository, nor the captured head revision, nor a \
                               common ancestor of the two.";
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let pull = github::review::fixture_pull(false);
        let identity = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let page = Client(UiTransport::Fixture { moved: false })
            .review_threads(&identity, None, &OperationControl::default())
            .unwrap();
        let target = page.items[0].target.clone();
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.pull = Some(pull);
                panel.review.show_list = false;
                panel.review.section = Section::Overview;
                panel.receive_threads(page, None);
                panel.conversations.active = Some(target);
            })
        });
        let bar = Scrollbar::width();
        for (interface, width, height, failure, cued) in [
            (18, 461., 490., Some(FAILURE), true),
            (13, 1480., 980., None, false),
        ] {
            let at = format!("{interface} pt in {width} × {height}");
            cx.update(|_, cx| {
                panel.update(cx, |panel, _| {
                    // A failure of a request from the panel, which answers on its top line.
                    panel.origin = Origin::Panel;
                    panel.error = failure.map(str::to_owned);
                })
            });
            open_notice_panel(&app, &panel, (interface, width, height), cx).await;
            draw(cx);
            let text = cx
                .debug_bounds("github-top-line-scroll")
                .expect("the line's text");
            let notice = cx.debug_bounds("github-notice").expect("the notice");
            assert_eq!(
                notice.right() < text.right() - px(0.5),
                cued,
                "{at}: the notice {notice:?} keeps a gutter in {text:?} only where cued"
            );
            // The scrollbar's viewport, which its track fills.
            let track = Bounds::new(
                point(text.right() - bar - px(4.), text.top()),
                size(bar + px(4.), text.size.height),
            );
            let thumb = painted_thumb(cx, track);
            assert_eq!(
                thumb.is_some(),
                cued,
                "{at}: a scrollbar ({thumb:?}) only where the line's text is taller than its room"
            );
            escape(cx);
        }
        cx.update(|_, cx| panel.update(cx, |panel, _| panel.error = None));
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Each press the panel refuses is said again, with a fresh alert node, though it refuses
    /// it as it refused the one before: Review submission… pressed twice with no pull request
    /// chosen. The lines alone stay the same, so the second press was silent.
    #[gpui::test]
    async fn the_window_alerts_each_refused_press(cx: &mut TestAppContext) {
        use crate::copy_feedback::tests::alerted;
        use crate::tags::tests::draw;
        let (_fixture, app, panel, cx) = notice_panel(cx);
        open_notice_panel(&app, &panel, (13, 461., 490.), cx).await;
        #[cfg(target_os = "linux")]
        let mut bus = {
            use crate::native_accessibility::atspi_state_tests::{Bus, next_frame};
            Bus::new(next_frame(cx))
        };
        let refused = DraftIssue::Selection.message();
        let mut heard = Vec::new();
        for press in ["first", "second"] {
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| panel.review_submission(window, cx))
            });
            draw(cx);
            draw(cx);
            let (id, text) = alerted(cx).unwrap_or_else(|| panic!("the {press} press is said"));
            assert_eq!(text, refused, "the {press} press is refused");
            heard.push(id);
            #[cfg(target_os = "linux")]
            {
                use crate::native_accessibility::atspi_state_tests::next_frame;
                bus.update(next_frame(cx));
                assert_eq!(
                    bus.announced(),
                    vec![refused.to_owned()],
                    "AT-SPI announces the {press} press"
                );
            }
        }
        assert_ne!(heard[0], heard[1], "each press is a fresh alert node");
        escape(cx);
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
        let gutter = crate::tags::ScrollCue::gutter();
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

    /// The sizes the reveal tests walk: interface size and window, a half tile at the default
    /// and the largest size and a wide window.
    const REVEAL_SIZES: [(u8, f32, f32); 4] = [
        (13, 461., 490.),
        (18, 461., 490.),
        (13, 1480., 800.),
        (18, 1480., 800.),
    ];

    /// A window whose root marks every text run (`width_walk::text_mark`), with the app's key
    /// bindings, Midnight and dialogs at rest (`focus_ring::sweep::prepare`), and the panel
    /// open on the offline fixture: the destination named, its first PR listed, signed in as
    /// the fixture's reviewer, with the first page of that PR's conversations, as the native
    /// review loads it. Keep the returned directory until the test ends.
    async fn fixture_panel(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        Entity<Panel>,
        PullRequest,
        &mut VisualTestContext,
    ) {
        let (fixture, app, cx) = crate::views::tests::styled_history_window(
            cx,
            b"commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> 1700000000 +0000\ndata 13\nfixture notes\nM 100644 inline notes.txt\ndata 6\nnotes\n\ndone\n",
            |root| root.text_bg(crate::width_walk::text_mark()),
        )
        .await;
        crate::focus_ring::sweep::prepare(&app, cx);
        let repo = cx.read(|cx| app.read(cx).repository.clone().unwrap());
        let panel = cx.update(|window, cx| {
            cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx))
        });
        app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.executor().run_until_parked();
        let pull = github::review::fixture_pull(false);
        let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let threads = Client(UiTransport::Fixture { moved: false })
            .review_threads(&captured, None, &OperationControl::default())
            .unwrap();
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.destination.update(cx, |input, cx| {
                    input.set_value("gitturtle-fixture/native-review", window, cx)
                });
                panel.rows = vec![pull.clone()];
                panel.review.fixture = true;
                panel.account = Some("offline-reviewer".into());
                panel.receive_threads(threads, None);
                panel.close(window, cx);
            });
            app.update(cx, |app, cx| app.open_github(window, cx));
        });
        cx.executor().run_until_parked();
        (fixture, app, panel, pull, cx)
    }

    /// Shows `pull`'s Overview at `interface` pt in a `width` × `height` window, the body and
    /// the conversation list at their tops and focus on the conversations pane, as the panel
    /// opens on it.
    fn show_overview(
        panel: &Entity<Panel>,
        pull: &PullRequest,
        (interface, width, height): (u8, f32, f32),
        cx: &mut VisualTestContext,
    ) {
        crate::focus_ring::sweep::interface_size(cx, interface);
        cx.simulate_resize(size(px(width), px(height)));
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.pull = Some(pull.clone());
                panel.review.section = Section::Overview;
                panel.review.show_list = false;
                panel.review.body_scroll.set_offset(point(px(0.), px(0.)));
                panel
                    .conversations
                    .list_scroll
                    .set_offset(point(px(0.), px(0.)));
                panel.focus_visible_section(window, cx);
                cx.notify();
            })
        });
        for _ in 0..3 {
            crate::width_walk::frame(cx);
        }
    }

    /// The lines of text the last frame drew inside `container`, a box that clips them, that
    /// its top edge cuts: each line's run, marked by the window's root, starts above the edge
    /// and ends below it by more than half a pixel.
    fn lines_cut_at_top(
        cx: &mut VisualTestContext,
        container: Bounds<Pixels>,
    ) -> Vec<Bounds<Pixels>> {
        let mark = gpui_kit::Background::from(crate::width_walk::text_mark());
        let scale = cx.update(|window, _| window.scale_factor());
        let clip = |value: gpui_kit::ScaledPixels| px(value.as_f32() / scale);
        let slack = px(0.5);
        quads(cx)
            .into_iter()
            .filter(|(line, quad)| {
                let mask = quad.content_mask.bounds;
                quad.background == mark
                    && clip(mask.top()) >= container.top() - slack
                    && clip(mask.bottom()) <= container.bottom() + slack
                    && line.left() >= container.left() - px(1.)
                    && line.right() <= container.right() + px(1.)
                    && line.top() < container.top() - slack
                    && line.bottom() > container.top() + slack
            })
            .map(|(line, _)| line)
            .collect()
    }

    /// Tab through a PR's Overview on the offline fixture, at 13 and 18 pt in a 461 × 490
    /// window and in a 1,480 × 800 one: where a stop scrolls the body or the conversation list,
    /// the scrolled box's top edge lies at the start of a block, cutting no line, as long as
    /// the box has content left below to show; and wrapping from the dialog's last stop to its
    /// first, the banner's, returns the body to its top. A conversation's reveal once cut its
    /// author line at the list's top, and the wrap left the body 6.5 px (13 pt) and 9 px (18
    /// pt) down, the banner's top cut. Every stop draws its ring whole (`focus_ring::sweep`),
    /// but for the conversations pane, which is taller than the body in a 461 × 490 window and
    /// shows its top edge there, its bottom below the fold.
    #[gpui::test]
    async fn tab_reveals_land_on_whole_lines_and_wrap_to_the_body_top(cx: &mut TestAppContext) {
        use crate::focus_ring::sweep;
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        let (body_scroll, list_scroll) = cx.read(|cx| {
            let panel = panel.read(cx);
            (
                panel.review.body_scroll.clone(),
                panel.conversations.list_scroll.clone(),
            )
        });
        let mut rings = Vec::new();
        for sizes in REVEAL_SIZES {
            let (interface, width, height) = sizes;
            let at = format!("{interface} pt in {width} × {height}");
            show_overview(&panel, &pull, sizes, cx);
            let mut offsets = (body_scroll.offset().y, list_scroll.offset().y);
            let mut wrapped = false;
            let mut list_moves = 0;
            for stop in 0..60 {
                cx.simulate_keystrokes("tab");
                for _ in 0..3 {
                    crate::width_walk::frame(cx);
                }
                let part = cx.update(|window, _| {
                    window
                        .context_stack()
                        .iter()
                        .rev()
                        .find_map(|context| context.get(review::PART).cloned())
                });
                let now = (body_scroll.offset().y, list_scroll.offset().y);
                let ends = (-body_scroll.max_offset().y, -list_scroll.max_offset().y);
                let here = format!("{at}, stop {stop} in {part:?}");
                if now.0 != offsets.0 && now.0 > ends.0 + px(0.5) {
                    let body = cx.debug_bounds("github-panel").expect("the body");
                    let cut = lines_cut_at_top(cx, body);
                    assert!(
                        cut.is_empty(),
                        "{here}: the body's top edge {body:?} cuts {cut:?}"
                    );
                }
                if now.1 != offsets.1 {
                    list_moves += 1;
                    let list = cx.debug_bounds("github-threads").expect("the list");
                    let cut = lines_cut_at_top(cx, list);
                    if now.1 > ends.1 + px(0.5) {
                        assert!(
                            cut.is_empty(),
                            "{here}: the list's top edge {list:?} cuts {cut:?}"
                        );
                    }
                }
                offsets = now;
                if part.as_ref().is_some_and(|part| part.as_ref() == "banner") {
                    assert_eq!(
                        now.0,
                        px(0.),
                        "{here}: the wrap returns the body to its top"
                    );
                    wrapped = true;
                    break;
                }
            }
            assert!(wrapped, "{at}: Tab wraps to the banner");
            assert!(list_moves > 0, "{at}: Tab scrolls the conversation list");
            // Each stop's ring, Tab round from the pane.
            show_overview(&panel, &pull, sizes, cx);
            let mut failures = Vec::new();
            sweep::walk(&app, cx, &at, interface, &mut failures);
            rings.extend(failures);
        }
        sweep::assert_whole_except(
            rings,
            &[
                (
                    "13 pt in 461 × 490 at 13 pt, stop 21",
                    "a clip cuts the right, bottom, left",
                ),
                (
                    "18 pt in 461 × 490 at 18 pt, stop 21",
                    "a clip cuts the right, bottom, left",
                ),
            ],
        );
    }

    /// Closing the panel with Escape after Tab has scrolled a PR's Overview, and opening it
    /// again, restores focus to the conversations pane, which draws its keyboard ring on its
    /// edge with its top edge drawn inside the body's clip, the room a ring takes clear of it:
    /// in a 461 × 490 window, at 13 and 18 pt, the pane is taller than the body and once showed
    /// its bottom with its top above the clip. In a 1,480 × 800 window the pane fits, and its
    /// ring is whole.
    #[gpui::test]
    async fn a_reopened_prs_conversations_pane_draws_its_rings_top_edge(cx: &mut TestAppContext) {
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        for sizes in REVEAL_SIZES {
            let (interface, width, height) = sizes;
            let at = format!("{interface} pt in {width} × {height}");
            show_overview(&panel, &pull, sizes, cx);
            // Into the conversation list, scrolling the body down to it.
            cx.simulate_keystrokes("tab tab");
            crate::width_walk::frame(cx);
            escape(cx);
            assert!(
                !cx.update(|window, cx| window.has_active_dialog(cx)),
                "{at}: Escape closes the panel"
            );
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
            for _ in 0..4 {
                crate::width_walk::frame(cx);
            }
            let section = cx.read(|cx| panel.read(cx).review.section_focus.clone());
            assert!(
                cx.update(|window, _| section.is_focused(window)),
                "{at}: the reopened panel restores focus to the conversations pane"
            );
            let (body, pane) = (
                cx.debug_bounds("github-panel").expect("the body"),
                cx.debug_bounds("github-thread-section").expect("the pane"),
            );
            let accent: Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
            let rings: Vec<_> = crate::focus_ring::painted_outlines(cx, accent)
                .into_iter()
                .filter(|ring| near(ring.bounds, pane))
                .collect();
            let [ring] = rings[..] else {
                panic!("{at}: one ring on the pane's edge {pane:?}: {rings:?}");
            };
            let room = cx.update(|_, cx| appearance::button_ring_room(cx));
            assert!(
                ring.sides[0] && pane.top() >= body.top() + room - px(0.5),
                "{at}: the ring's top edge lies inside the body {body:?}, its room clear: {ring:?}"
            );
            if pane.size.height + room * 2. <= body.size.height {
                assert!(
                    ring.whole,
                    "{at}: the pane fits, and its ring is whole: {ring:?}"
                );
            }
            if width > 1000. {
                assert!(
                    ring.whole,
                    "{at}: the wide window fits the pane whole: {ring:?}"
                );
            }
        }
    }

    /// The panel closed with its body scrolled to its end reopens onto its destination field
    /// with the body back at its top, at 13 and 18 pt in a 461 × 490 window: the field whole
    /// with its ring and the banner above it in view, rather than at the offset it closed at:
    /// at 13 pt it reopened 93 px down, the field just inside the body's top edge and the
    /// banner above it hidden, since the field alone was whole and nothing revealed more.
    #[gpui::test]
    async fn reopening_after_scrolling_returns_the_body_to_its_top(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let (scroll, field) = cx.read(|cx| {
            let panel = panel.read(cx);
            (
                panel.review.body_scroll.clone(),
                panel.destination.read(cx).focus_handle(cx),
            )
        });
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        let mut scrolled = 0;
        for interface in [13, 18] {
            let at = format!("{interface} pt");
            open_notice_panel(&app, &panel, (interface, 461., 490.), cx).await;
            if scroll.max_offset().y <= px(0.) {
                escape(cx);
                continue;
            }
            scrolled += 1;
            scroll.set_offset(point(px(0.), -scroll.max_offset().y));
            draw(cx);
            draw(cx);
            escape(cx);
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
            settle_panel(&app, cx).await;
            for _ in 0..3 {
                draw(cx);
            }
            assert!(
                cx.update(|window, _| field.is_focused(window)),
                "{at}: the panel reopens onto its destination field"
            );
            assert_eq!(
                scroll.offset().y,
                px(0.),
                "{at}: the body is back at its top"
            );
            let body = cx.debug_bounds("github-panel").expect("the body");
            let destination = cx
                .debug_bounds("github-destination")
                .expect("the destination field");
            let banner = cx.debug_bounds("github-fixture-mode").expect("the banner");
            assert!(
                destination.top() - room >= body.top()
                    && destination.bottom() + room <= body.bottom()
                    && banner.top() >= body.top(),
                "{at}: the field {destination:?} with its ring and the banner {banner:?} lie in \
                 view in {body:?}"
            );
            escape(cx);
        }
        assert!(scrolled > 0, "some size scrolls the body");
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Reopening the panel onto its destination field keeps the body at its top wherever the
    /// field is whole there with its ring, as at 13 and 18 pt in a 461 × 490 window, so the
    /// banner and its Move fixture head stay in view, whether a key or the pointer came last.
    /// At 18 pt it scrolled the body down, by 129.5 px the field's row to its top, and the
    /// banner out of view: the field kept a line's room clear of the body's edges rather than
    /// its ring's, which alone scrolled 112 px, and after a key its row, taller than the body,
    /// revealed itself rather than the field. In a 461 × 470 window, where the field is not
    /// whole at the body's top, reopening scrolls the body until it is. A PR's Review reopens
    /// onto its summary, a field too: where the body shows it whole with its ring, at 13 pt in
    /// a 461 × 490 window and 18 pt in a 1,480 × 800 one, the body stays where it was, where a
    /// line's room clear of its edges scrolled it on.
    #[gpui::test]
    async fn reopening_onto_a_whole_field_keeps_the_body_at_its_top(cx: &mut TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, app, panel, cx) = notice_panel(cx);
        let (scroll, field) = cx.read(|cx| {
            let panel = panel.read(cx);
            (
                panel.review.body_scroll.clone(),
                panel.destination.read(cx).focus_handle(cx),
            )
        });
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        for (interface, width, height) in [(13, 461., 490.), (18, 461., 490.), (18, 461., 470.)] {
            for pointer in [false, true] {
                let at = format!(
                    "{interface} pt in {width} × {height}, reopened after the {}",
                    if pointer { "pointer" } else { "keyboard" }
                );
                cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
                cx.simulate_resize(size(px(width), px(height)));
                draw(cx);
                cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
                settle_panel(&app, cx).await;
                escape(cx);
                assert!(
                    !cx.update(|window, cx| window.has_active_dialog(cx)),
                    "{at}: Escape closes the panel"
                );
                // The body at its top, as a panel opens that nothing has scrolled.
                scroll.set_offset(point(px(0.), px(0.)));
                if pointer {
                    cx.simulate_mouse_move(point(px(4.), px(4.)), None, Modifiers::default());
                }
                cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
                settle_panel(&app, cx).await;
                for _ in 0..3 {
                    crate::width_walk::frame(cx);
                }
                assert!(
                    cx.update(|window, _| field.is_focused(window)),
                    "{at}: the panel reopens onto the destination field"
                );
                let offset = scroll.offset().y;
                let body = cx.debug_bounds("github-panel").expect("the body");
                let shown = cx.debug_bounds("github-destination").expect("the field");
                assert!(
                    shown.top() - room >= body.top() - px(0.5)
                        && shown.bottom() + room <= body.bottom() + px(0.5),
                    "{at}: the field {shown:?} is whole with its ring in the body {body:?}"
                );
                // Where the field lies with the body at its top.
                let (top, bottom) = (shown.top() - offset, shown.bottom() - offset);
                let whole_at_top =
                    top - room >= body.top() - px(0.5) && bottom + room <= body.bottom() + px(0.5);
                assert_eq!(
                    whole_at_top,
                    height > 480.,
                    "{at}: the field is whole at the body's top in the taller window only"
                );
                if whole_at_top {
                    assert_eq!(offset, px(0.), "{at}: the body stays at its top");
                    let banner = cx
                        .debug_bounds("github-fixture-move")
                        .expect("Move fixture head");
                    assert!(
                        banner.top() >= body.top() && banner.bottom() <= body.bottom(),
                        "{at}: Move fixture head {banner:?} stays in view in {body:?}"
                    );
                } else {
                    assert!(offset < px(0.), "{at}: the body scrolls to the field");
                }
                escape(cx);
            }
        }

        // A PR's Review, reopened onto its summary where the body shows it whole with its
        // ring and less than a line's room below it.
        let summary = cx.read(|cx| panel.read(cx).body.read(cx).focus_handle(cx));
        cx.update(|_, cx| {
            panel.update(cx, |panel, _| {
                panel.pull = Some(github::review::fixture_pull(false));
                panel.review.section = Section::Review;
                panel.review.show_list = false;
            })
        });
        // Opened and closed once, so the draft status the destination's close left ("Draft
        // not saved: choose a pull request…", three lines at 13 pt in 461 × 490) gives way to
        // the PR's own, and nothing above the summary moves across the reopens below: the
        // status shrank by 39 px across the first one and took the summary out of the place
        // that leaves it less than a line's room.
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
        settle_panel(&app, cx).await;
        escape(cx);
        for (interface, width, height) in [(13, 461., 490.), (18, 1480., 800.)] {
            for pointer in [false, true] {
                let at = format!(
                    "Review at {interface} pt in {width} × {height}, reopened after the {}",
                    if pointer { "pointer" } else { "keyboard" }
                );
                cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
                cx.simulate_resize(size(px(width), px(height)));
                draw(cx);
                cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
                settle_panel(&app, cx).await;
                for _ in 0..3 {
                    crate::width_walk::frame(cx);
                }
                let body = cx.debug_bounds("github-panel").expect("the body");
                let shown = cx
                    .debug_bounds("github-review-summary")
                    .expect("the summary");
                let line = cx.update(|window, _| window.rem_size()) + px(2.);
                let placed = (scroll.offset().y + (body.bottom() - room - px(1.) - shown.bottom()))
                    .clamp(-scroll.max_offset().y, px(0.));
                scroll.set_offset(point(px(0.), placed));
                draw(cx);
                draw(cx);
                let shown = cx
                    .debug_bounds("github-review-summary")
                    .expect("the summary");
                assert!(
                    shown.top() - room >= body.top()
                        && shown.bottom() + room <= body.bottom()
                        && shown.bottom() + line > body.bottom(),
                    "{at}: the summary {shown:?} is whole with its ring in the body {body:?}, \
                     with less than a line's room below it"
                );
                escape(cx);
                if pointer {
                    cx.simulate_mouse_move(point(px(4.), px(4.)), None, Modifiers::default());
                }
                cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
                settle_panel(&app, cx).await;
                for _ in 0..3 {
                    crate::width_walk::frame(cx);
                }
                assert!(
                    cx.update(|window, _| summary.is_focused(window)),
                    "{at}: the panel reopens onto the summary"
                );
                assert_eq!(
                    scroll.offset().y,
                    placed,
                    "{at}: the body stays where it was"
                );
                assert_eq!(
                    cx.debug_bounds("github-review-summary"),
                    Some(shown),
                    "{at}: the summary stays where it was placed"
                );
                escape(cx);
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Opening a PR from the list, by the keyboard, at 13 and 18 pt in a 461 × 490 window shows
    /// it from its title: the title whole in the body with Overview, the tab it opens to and its
    /// first stop, whole below it, and at 13 pt its captured identities too, where the PR once
    /// opened showing only its title, the tabs below the fold (Overview's top at 464 px against
    /// a body ending at 355.5 px at 13 pt). At 18 pt the tabs wrap, and Review's row lies below
    /// the fold. In a 1,480 × 800
    /// window, which shows all of that as it is, the body stays at its top, the controls and
    /// notices above the PR in view.
    #[gpui::test]
    async fn opening_a_pr_shows_it_from_its_title(cx: &mut TestAppContext) {
        let (_fixture, app, panel, pull, cx) = fixture_panel(cx).await;
        let body_scroll = cx.read(|cx| panel.read(cx).review.body_scroll.clone());
        for (interface, width, height) in REVEAL_SIZES {
            let at = format!("{interface} pt in {width} × {height}");
            crate::focus_ring::sweep::interface_size(cx, interface);
            cx.simulate_resize(size(px(width), px(height)));
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.pull = None;
                    panel.review.show_list = true;
                    panel.review.body_scroll.set_offset(point(px(0.), px(0.)));
                    panel.focus_visible_section(window, cx);
                    cx.notify();
                })
            });
            crate::width_walk::frame(cx);
            // The keyboard drives, as Return on a PR row does: focus reveals.
            cx.simulate_keystrokes("tab");
            crate::width_walk::frame(cx);
            body_scroll.set_offset(point(px(0.), px(0.)));
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| panel.inspect(pull.clone(), window, cx))
            });
            settle_panel(&app, cx).await;
            for _ in 0..3 {
                crate::width_walk::frame(cx);
            }
            assert!(
                cx.read(|cx| panel.read(cx).pull.is_some() && !panel.read(cx).review.show_list),
                "{at}: the PR opened"
            );
            let body = cx.debug_bounds("github-panel").expect("the body");
            let inside = |bounds: Bounds<Pixels>| {
                bounds.top() >= body.top() - px(0.5) && bounds.bottom() <= body.bottom() + px(0.5)
            };
            let title = cx.debug_bounds("github-pull-title").expect("the title");
            assert!(
                inside(title),
                "{at}: the title {title:?} lies whole in the body {body:?}"
            );
            let overview = cx
                .debug_bounds("github-section-0")
                .expect("the Overview tab");
            assert!(
                inside(overview),
                "{at}: Overview {overview:?}, the PR's first stop, lies whole in the body \
                 {body:?}, below the title {title:?} and the branches {:?}",
                cx.debug_bounds("github-branch-context")
            );
            if interface == 13 {
                // And the summary of what the PR compares, below the tabs.
                let identities = cx
                    .debug_bounds("github-captured-identities")
                    .expect("the captured identities");
                assert!(
                    inside(identities),
                    "{at}: the captured identities {identities:?} lie whole in the body {body:?}"
                );
            }
            if width > 1000. {
                assert_eq!(
                    body_scroll.offset().y,
                    px(0.),
                    "{at}: the wide window shows the PR without scrolling"
                );
            }
        }
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
