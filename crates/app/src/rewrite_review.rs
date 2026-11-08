//! A retained native commit-series inspector and a separate reviewed lease push.
use crate::*;
use gitturtle_core::{
    LeasedPublishPlan, OperationControl, PublicationInspection, RewriteReview, WriteCommand,
};
use gpui_kit::component::{WindowExt, dialog::DialogFooter};
use gpui_kit::prelude::FluentBuilder;

impl GitTurtle {
    pub(super) fn open_rewrite_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_series_review(window, cx);
    }

    /// Opens the rewritten series' review, as [`Self::open_rewrite_review`]
    /// does, and returns it, which a test waits on for its read
    /// (`focus_ring::sweep::Opened`).
    pub(super) fn open_series_review(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<SeriesBrowser>> {
        if self.operation_busy.is_some() || self.page != AppPage::Repository {
            return None;
        }
        let repo = self.repository.clone()?;
        let owner = cx.entity().downgrade();
        let remote = self.remote_name.read(cx).value().trim().to_owned();
        let branch = self.remote_branch.read(cx).value().trim().to_owned();
        let form = cx.new(|cx| SeriesBrowser::new(owner, repo, remote, branch, window, cx));
        open_series_dialog(form.clone(), window, cx);
        #[cfg(test)]
        OPENED.with(|opened| opened.replace(Some(form.downgrade())));
        Some(form)
    }
}

#[cfg(test)]
thread_local! {
    /// The review opened last, for a test that opens it as a person does,
    /// from the palette, whose command keeps no handle to it.
    static OPENED: std::cell::RefCell<Option<WeakEntity<SeriesBrowser>>> =
        const { std::cell::RefCell::new(None) };
}

/// The review opened since this was last asked, if it is still open.
#[cfg(test)]
pub(crate) fn take_opened() -> Option<Entity<SeriesBrowser>> {
    OPENED.with(|opened| opened.take()?.upgrade())
}

/// The focus sweep (`focus_ring::sweep`) walks the rewritten series' review,
/// and the width walk (`width_walk`) reads it, once its series has been
/// read. The read runs on the review's own thread, which neither
/// `run_until_parked` nor the drain of the app's executor waits for: a walk
/// that began while it ran read "Reading captured local history…" with
/// Refresh series disabled, which the reply then replaced.
#[cfg(test)]
impl crate::focus_ring::sweep::Opened for Option<Entity<SeriesBrowser>> {
    fn finish(self, cx: &mut gpui::VisualTestContext) {
        let Some(review) = self else {
            return;
        };
        // Every read the review submitted has answered once its thread
        // drains, and the reply lands on this thread when it next runs.
        review.read_with(cx, |review, _| review.metadata.drain());
        cx.executor().run_until_parked();
        assert!(
            review.read_with(cx, |review, _| !review.pending),
            "the rewritten series' read lands"
        );
    }
}

/// Opens the series review's dialog around `form`.
fn open_series_dialog(form: Entity<SeriesBrowser>, window: &mut Window, cx: &mut App) {
    window.open_alert_dialog(cx, move |dialog, window, cx| {
        let close = form.clone();
        let cancel = form.clone();
        let shown = form.clone();
        // In the dialog's content, as the profile form is: the body takes
        // what the cap leaves it and scrolls the rest, revealing the control
        // Tab moves onto, with the ring's room below it, which the footer's
        // gap gives back. In the dialog's own scrolling body, at 18 pt in a
        // 461 × 490 window, the body was taller than what the cap left, so
        // that body cut the series list's ring at its foot and revealed
        // nothing.
        dialog
            .title("Review rewritten series")
            .width(appearance::dialog_width(px(980.), window))
            .max_h(appearance::dialog_max_height(window))
            .gap(crate::tags::DIALOG_FOOTER_GAP - appearance::button_ring_room(cx))
            .content(move |content, _, _| content.min_h_0().child(shown.clone()))
            .footer(DialogFooter::new().child(
                button("close-series-review", "Back to repository", "", false).on_click(
                    move |_, window, cx| close.update(cx, |form, cx| form.close(window, cx)),
                ),
            ))
            .on_cancel(move |_, window, cx| {
                cancel.update(cx, |form, cx| form.back(window, cx));
                false
            })
    });
}

/// What the review dialog's height cap ([`appearance::dialog_max_height`])
/// leaves its content in `window`, as [`open_series_dialog`] lays the dialog
/// out: the cap less its 1 px edge above and below, the kit's 16 px padding
/// above the title and below the footer, the title's one line, a rem with a
/// line height of one, and the half rem under it, the footer's gap, which
/// gives back the ring's room the content keeps below its body, and Back to
/// repository's compact Button, 28 px at the default interface size
/// (`crate::button_frame`). The body takes that room as the dialog lays it
/// out; the series list, whose height is set before layout, keeps the rest
/// of the body its room from it.
fn content_room(window: &Window, cx: &App) -> Pixels {
    const EDGE: Pixels = px(1.);
    const PADDING: Pixels = px(16.);
    let footer_gap = crate::tags::DIALOG_FOOTER_GAP - appearance::button_ring_room(cx);
    appearance::dialog_max_height(window)
        - EDGE * 2.
        - PADDING * 2.
        - window.rem_size() * 1.5
        - footer_gap
        - appearance::ui_size(28.)
}

/// A series row's height, its rule beneath it included, on the window's
/// device pixels, as layout snaps the row: the list's height and its scroll
/// are whole rows of it, and a row unsnapped at 18 pt, about 94.15 px, drew
/// 94 px, so six of them left a pixel of a seventh in the list.
fn series_row_height(window: &Window) -> Pixels {
    window.pixel_snap(appearance::ui_size(68.))
}

/// How many whole rows the series list shows for `rows` in `window`: as
/// many as the content's room ([`content_room`]) holds inside the list's
/// 1 px border once the rest of the body keeps its room, at least one and
/// at most six, and none past the series' own rows, though a list with none
/// keeps one row's height for the state that says why. The list is that
/// many rows tall and scrolls by whole rows ([`SeriesBrowser::keep_row_whole`]),
/// so every row it draws is whole (DESIGN.md, "Content stays whole"): the
/// rest of the room as its height, at 13 pt in a 461 × 490 window it cut its
/// second row 13.5 px down.
fn rows_shown(rows: usize, window: &Window, cx: &App) -> usize {
    let room = content_room(window, cx) - appearance::ui_size(210.) - list_border(window) * 2.;
    let fit = (f32::from(room) / f32::from(series_row_height(window))).floor();
    // Never past the series' rows, nor below one where there are none:
    // `clamp(fit, 1, 0)` panics.
    (fit.max(0.) as usize).clamp(1, rows.clamp(1, 6))
}

/// The series list's 1 px border as layout draws it, on the window's
/// device pixels: two thirds of a pixel at 1.5× and eight sevenths at
/// 1.75×, where a flat 2 px for both sides showed a device pixel of the
/// next row, or cut one off the last.
fn list_border(window: &Window) -> Pixels {
    window.pixel_snap(px(1.))
}

/// Scrolls `scroll` the least that shows `bounds`, drawn in this frame,
/// whole with `margin` above and below it, once the frame has painted,
/// unless something has scrolled it since.
fn reveal_whole(
    scroll: &ScrollHandle,
    bounds: Bounds<Pixels>,
    margin: Pixels,
    window: &mut Window,
    cx: &mut App,
) {
    let view = scroll.bounds();
    let delta = if bounds.bottom() + margin > view.bottom() {
        view.bottom() - (bounds.bottom() + margin)
    } else if bounds.top() - margin < view.top() {
        view.top() - (bounds.top() - margin)
    } else {
        return;
    };
    let scroll = scroll.clone();
    let previous = scroll.offset();
    window.defer(cx, move |window, _| {
        if scroll.offset() != previous {
            return;
        }
        let y = (previous.y + delta).clamp(-scroll.max_offset().y, px(0.));
        scroll.set_offset(point(previous.x, y));
        window.refresh();
    });
}

/// The series list's key context, where Return is the list's own
/// (`interactive_rebase::init`): it inspects the selected commit's files
/// rather than taking the review dialog's default action, which closed the
/// review.
pub(crate) const SERIES_CONTEXT: &str = "RewriteSeries";

/// The series list's accessible description while it lists a series: its
/// keys.
const SERIES_KEYS: &str = "Up and Down select a commit pair. Return inspects its rewritten files, or the original's where it has no rewritten commit.";

/// What the list's place says while the series is read.
const READING: &str = "Reading captured local history…";

/// What an inspected commit's file box says while its changed files are
/// read.
const READING_FILES: &str = "Reading changed files…";

/// What the preview's box says while a chosen file's preview is read.
const READING_PREVIEW: &str = "Reading preview…";

/// What the chosen file's row says at its end while its preview is read
/// below the body's view ([`SeriesBrowser::preview_read_hidden`]).
const READING_ROW: &str = "Reading…";

/// The title a failed read of a chosen file's preview keeps above the
/// read's error in the preview's box (DESIGN.md, "Empty and edge states").
const PREVIEW_UNAVAILABLE: &str = "Preview unavailable";

/// What the preview's box says for a commit that changed no files.
const NO_FILES: &str = "This commit has no changed files.";

/// What the preview's box says before a file is chosen, and why the text
/// modes wait then.
const CHOOSE_FILE: &str = "Select a file to inspect its captured commit bytes.";

/// Why the text modes wait for a preview with no patch or source, as an
/// image's or a document's.
const NO_TEXT: &str = "This file's preview has no patch or source.";

/// Why Diagrams waits for a Mermaid file (`.mmd`, `.mermaid`) whose text
/// was read and holds no diagrams.
const NO_DIAGRAMS: &str = "This file has no Mermaid diagrams.";

/// Why Diagrams waits for a Mermaid file stored as a link: its text names
/// the link's target, which no preview follows.
const LINKED: &str = "This file is stored as a link, which Diagrams never follows.";

/// Why Diagrams waits for a Mermaid file whose Git LFS object the local
/// store does not hold: its text is the object's pointer.
const NO_LFS_OBJECT: &str =
    "This file's Git LFS object is not in the local store. No download was attempted.";

/// Why Diagrams waits for any other file without diagrams: what it shows.
const NOT_DIAGRAMS: &str = "Diagrams shows Mermaid files and the Mermaid diagrams in Markdown.";

/// The text modes' names, by mode, and the keys their tooltips are drawn
/// under ([`refusing`]).
const TEXT_MODES: [(&str, &str); 4] = [
    ("Patch", "series-text-mode-0"),
    ("Before source", "series-text-mode-1"),
    ("After source", "series-text-mode-2"),
    ("Diagrams", "series-text-mode-3"),
];

/// What the list's place says where the captured series has no commit on
/// either side, which a rebase that rewrote at least one never leaves.
const NO_COMMITS: &str = "The captured series has no commits to compare.";

/// Why Check publication destination… and Refresh series wait while a
/// destination check runs.
const CHECKING: &str = "Checking the publication destination. Cancel remote check stops it.";

/// `control`, unavailable while `refused` says why, which it then says in
/// its tooltip and its accessible description, as a branch dialog's refused
/// actions do (`branch_actions::dialog_action`): the review's actions were
/// dimmed with nothing to say they waited for a series.
fn refusing(control: Button, id: &'static str, refused: Option<SharedString>) -> impl IntoElement {
    let disabled = refused.is_some();
    described(control.disabled(disabled), id, refused)
}

/// `control`, saying `reason`, where it has one, in its tooltip and its
/// accessible description.
fn described(control: Button, id: &'static str, reason: Option<SharedString>) -> impl IntoElement {
    use native_accessibility::DrawnButton as _;
    control
        .when_some(reason, |control, reason| {
            let control = Described(control).aria_description(reason.clone()).0;
            crate::workspace::waiting_tooltip(control, id, reason.to_string())
        })
        .node_recorded()
}

/// The title a failed read of the series keeps above its explanation
/// (DESIGN.md, "Empty and edge states").
const UNREVIEWABLE: &str = "Series not reviewable";

/// The menu's name for the rebase form, which the review offers where no
/// rebase captured a series.
const EDIT_COMMITS: &str = "Edit local commits…";

/// Why Edit local commits… waits once the review's repository is no longer
/// the one its window shows: it would open the rebase form on another.
const NOT_SHOWN: &str = "Return to this review's repository first";

/// What the review draws in the series list's place with no series to show
/// ([`SeriesBrowser::empty_reason`]).
struct Empty {
    /// A failure's title, above its explanation, `line`.
    title: Option<&'static str>,
    line: SharedString,
    /// The whole state, read as the list's accessible description and the
    /// description of each action that waits for a series.
    description: SharedString,
    /// Whether no rebase captured a series, which offers Edit local commits….
    uncaptured: bool,
}

/// The prepaint of the actions below the series, `observe`, recording in
/// tests where each frame put them and whether it drew a read under way
/// ([`painted_actions`]).
fn action_row(
    pending: bool,
    observe: impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static,
) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
    #[cfg(not(test))]
    let _ = pending;
    move |bounds, window, cx| {
        #[cfg(test)]
        if let Some(first) = bounds.first() {
            painted_actions::record(first.top(), pending);
        }
        observe(bounds, window, cx)
    }
}

/// Test-only: where each painted frame put the actions below the series,
/// the top of Review messages, and whether that frame drew a read under
/// way. A test window draws a frame a read's landing asks for inside the
/// update that asked, so bounds read after it show only the last frame; this
/// record keeps each (as `diff_view::painted_rows` does).
#[cfg(test)]
mod painted_actions {
    use super::*;

    thread_local! {
        static TOPS: std::cell::RefCell<Option<Vec<(Pixels, bool)>>> =
            const { std::cell::RefCell::new(None) };
    }

    /// Starts recording, forgetting anything recorded before.
    pub(super) fn watch() {
        TOPS.with_borrow_mut(|tops| *tops = Some(Vec::new()));
    }

    /// What each frame painted since [`watch`], in order; recording stops.
    pub(super) fn take() -> Vec<(Pixels, bool)> {
        TOPS.with_borrow_mut(|tops| tops.take().unwrap_or_default())
    }

    pub(super) fn record(top: Pixels, pending: bool) {
        TOPS.with_borrow_mut(|tops| {
            if let Some(tops) = tops {
                tops.push((top, pending));
            }
        });
    }
}

/// A kit Button lent GPUI's stateful element methods for its accessible
/// description, which the kit's Button does not offer, as a branch dialog's
/// actions are: its interactivity is its base Button's, whose node GPUI
/// writes with the description.
struct Described(Button);

impl InteractiveElement for Described {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.0.interactivity()
    }
}

impl StatefulInteractiveElement for Described {}

pub(crate) struct SeriesBrowser {
    owner: WeakEntity<GitTurtle>,
    repo: GitRepository,
    metadata: SerialExecutor,
    worker: Worker,
    control: OperationControl,
    network_control: Option<OperationControl>,
    generation: u64,
    /// This showing of the file preview, which a line beneath an open
    /// control belongs to; [`Self::clear_content`] starts a new one.
    showing: crate::system_preview::Showing,
    task: Option<Task<()>>,
    review: Option<Arc<RewriteReview>>,
    selected: usize,
    focus: FocusHandle,
    /// Back to series, which takes focus as the inspector replaces the
    /// series list: left on the list, focus rested on a control no longer
    /// drawn, so keys reached the window's root and Escape never reached
    /// the review.
    back: FocusHandle,
    rows_scroll: UniformListScrollHandle,
    /// How many whole rows the series list showed, and how tall they were,
    /// as last drawn ([`Self::follow_rows`]).
    rows_layout: Option<(usize, Pixels)>,
    files_scroll: UniformListScrollHandle,
    inspecting: bool,
    messages: Option<[Option<Entity<EditorState>>; 2]>,
    notice: Option<String>,
    inspect_label: String,
    files: Vec<FileChange>,
    selected_file: Option<usize>,
    content: Option<Arc<Content>>,
    editor: Option<Entity<EditorState>>,
    /// The patch editor's decorations, kept for its life like Compare's.
    decoration: Option<crate::editor_find::PatchDecorations>,
    patch: Option<Entity<diff_view::DiffView>>,
    text_mode: usize,
    pending: bool,
    /// The height the series list's empty state took as last drawn at
    /// rest, which it keeps while Refresh series reads again.
    empty_height: std::rc::Rc<std::cell::Cell<Pixels>>,
    /// Where the preview's box held its content as last drawn, which tells
    /// whether a chosen file's read there lies below the body's view
    /// ([`Self::preview_read_hidden`]).
    preview_bounds: std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    /// Why the chosen file's preview could not be read, which the
    /// preview's box says, as the text modes do.
    preview_error: Option<String>,
    /// The scroll of a failure taller than the preview's box
    /// ([`Self::preview_prompt`]), from its top for each showing.
    failure_scroll: ScrollHandle,
    /// The focus of a failure taller than the preview's box, which holds a
    /// Tab stop only while the failure overflows
    /// ([`Self::preview_prompt`]).
    failure_focus: FocusHandle,
    /// The answer of the last line beneath an open control in the preview
    /// that landed, recorded for every line as it lands, in view or not, so
    /// the body scrolls to each click's line once ([`Self::opener`]).
    line_revealed: std::rc::Rc<std::cell::Cell<Option<usize>>>,
    network_pending: bool,
    error: Option<String>,
    remote: Entity<InputState>,
    remote_branch: Entity<InputState>,
    publication: Option<LeasedPublishPlan>,
    _subscriptions: Vec<Subscription>,
    closed: bool,
    /// The body's scroll, which reveals the control Tab moves onto: its
    /// actions and publication fields lie below its fold in a half tile.
    body: crate::focus_reveal::FocusReveal,
}

impl Drop for SeriesBrowser {
    fn drop(&mut self) {
        self.control.cancel();
        self.worker.cancel();
        if let Some(control) = &self.network_control {
            control.cancel();
        }
    }
}

impl SeriesBrowser {
    fn new(
        owner: WeakEntity<GitTurtle>,
        repo: GitRepository,
        remote: String,
        branch: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            owner,
            repo,
            metadata: SerialExecutor::new("rewrite-series-review"),
            worker: Worker::new(),
            control: OperationControl::default(),
            network_control: None,
            generation: 0,
            showing: crate::system_preview::Showing::next(),
            task: None,
            review: None,
            selected: 0,
            focus: cx.focus_handle().tab_stop(true),
            back: cx.focus_handle().tab_stop(true),
            rows_scroll: UniformListScrollHandle::new(),
            rows_layout: None,
            files_scroll: UniformListScrollHandle::new(),
            inspecting: false,
            messages: None,
            notice: None,
            inspect_label: String::new(),
            files: vec![],
            selected_file: None,
            content: None,
            editor: None,
            decoration: None,
            patch: None,
            text_mode: 0,
            pending: false,
            empty_height: Default::default(),
            preview_bounds: Default::default(),
            preview_error: None,
            failure_scroll: ScrollHandle::new(),
            failure_focus: cx.focus_handle(),
            line_revealed: Default::default(),
            network_pending: false,
            error: None,
            remote: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(remote)
                    .placeholder("Named remote")
            }),
            remote_branch: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(branch)
                    .placeholder("Remote branch")
            }),
            publication: None,
            _subscriptions: vec![],
            closed: false,
            body: crate::focus_reveal::FocusReveal::new(crate::focus_reveal::Trigger::Keyboard),
        };
        for input in [this.remote.clone(), this.remote_branch.clone()] {
            this._subscriptions.push(cx.subscribe_in(&input, window, |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    if this.publication.take().is_some() { this.notice = Some("Destination changed. Check the named remote branch again before publishing.".into()); }
                    cx.notify();
                }
            }));
        }
        this.load(window, cx);
        this
    }
    fn current(&self, cx: &App) -> bool {
        !self.closed
            && self.owner.upgrade().is_some_and(|owner| {
                let owner = owner.read(cx);
                owner.page == AppPage::Repository && owner.path.as_deref() == Some(self.repo.path())
            })
    }
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.cancel_read();
        if let Some(control) = &self.network_control {
            control.cancel();
        }
        window.close_dialog(cx);
        if let Some(owner) = self.owner.upgrade() {
            let focus = owner.read(cx).app_focus.clone();
            focus.focus(window, cx);
        }
        window.refresh();
    }
    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.inspecting {
            self.inspecting = false;
            self.cancel_read();
            self.focus.focus(window, cx);
            cx.notify();
        } else {
            self.close(window, cx);
        }
    }
    fn cancel_read(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.control.cancel();
        self.worker.cancel();
        self.task = None;
        self.pending = false;
    }
    fn clear_content(&mut self) {
        self.showing = crate::system_preview::Showing::next();
        self.content = None;
        self.editor = None;
        self.decoration = None;
        self.patch = None;
        self.selected_file = None;
        self.preview_error = None;
        self.failure_scroll = ScrollHandle::new();
    }
    /// Opens captured bytes for this showing's open controls through the
    /// window's `GitTurtle`, whose keyboard focus ring it draws. A line
    /// saying why a click opened nothing that lands outside the body's view,
    /// as beneath a control a person scrolled to the view's foot, is
    /// revealed whole by the body's least scroll, with its ring's room, and
    /// focus stays on the control: the line once answered below the fold,
    /// so the click seemed to do nothing.
    fn opener(&self, cx: &App) -> crate::system_preview::Opener {
        let ring = self
            .owner
            .upgrade()
            .map(|owner| owner.read(cx).focus_ring.clone())
            .unwrap_or_default();
        let (body, room) = (self.body.scroll().clone(), appearance::button_ring_room(cx));
        let revealed = self.line_revealed.clone();
        crate::system_preview::Opener::new(self.owner.clone(), self.showing)
            .ringed_by(ring)
            .on_line(move |answer, line, window, cx| {
                // As each click's line lands, never again once a person
                // scrolls away from it.
                if revealed.replace(Some(answer)) != Some(answer) {
                    reveal_whole(&body, line, room, window, cx);
                }
            })
    }
    /// CPU bytes the open preview retains: its prepared content, which includes the patch
    /// presentation, with the same reserve for native editors as a repository tab, plus the
    /// patch editor's decorations. This transient review has no retained-bytes bound of its
    /// own, and a modal stays out of the repository tabs' bound because tabs cannot switch
    /// while it is open; its preview worker's cache bounds the content separately.
    #[cfg_attr(not(test), allow(dead_code))]
    fn preview_retained_bytes(&self) -> usize {
        self.content
            .as_ref()
            .map_or(0, |content| content.bytes().saturating_mul(4))
            + self
                .decoration
                .as_ref()
                .map_or(0, crate::editor_find::PatchDecorations::retained_bytes)
    }
    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_read();
        self.clear_content();
        self.review = None;
        self.publication = None;
        self.notice = None;
        self.error = None;
        self.pending = true;
        self.control = OperationControl::default();
        let generation = self.generation;
        let repo = self.repo.clone();
        let response = self
            .metadata
            .submit_controlled(self.control.clone(), move || {
                let Some(capture) = recovery_drafts::latest_rewrite(repo.path())? else {
                    return Ok(None);
                };
                repo.rewrite_review(&capture.base, &capture.original, &capture.branch)
                    .map(Some)
            });
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!("Series review ended without a result")));
            let _ = this.update_in(cx, |this, window, cx| {
                if this.generation != generation || !this.current(cx) {
                    return;
                }
                this.pending = false;
                this.task = None;
                match result {
                    Ok(Some(review)) => {
                        this.list(review, window, cx);
                        this.focus.focus(window, cx);
                    }
                    Ok(None) => {}
                    Err(error) => this.error = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        }));
    }
    /// Why the review shows no series: its read under way, the load's
    /// failure, with its title, or, with neither, that no rebase captured
    /// one, which offers Edit local commits….
    fn empty_reason(&self) -> Empty {
        let (title, line): (Option<&'static str>, SharedString) = if self.pending {
            (None, READING.into())
        } else if self.review.is_some() {
            (None, NO_COMMITS.into())
        } else if let Some(error) = &self.error {
            (Some(UNREVIEWABLE), error.clone().into())
        } else {
            (None, recovery_drafts::NO_CAPTURE.into())
        };
        let description =
            title.map_or_else(|| line.clone(), |title| format!("{title}. {line}").into());
        Empty {
            title,
            line,
            description,
            uncaptured: !self.pending && self.review.is_none() && self.error.is_none(),
        }
    }
    /// Whether an inspected commit's changed files are read, which its file
    /// box says.
    fn reading_files(&self) -> bool {
        self.inspecting && self.messages.is_none() && self.pending && self.selected_file.is_none()
    }
    /// Whether a chosen file's preview is read, which the preview's box
    /// says.
    fn reading_preview(&self) -> bool {
        self.inspecting && self.pending && self.selected_file.is_some()
    }
    /// Why the text modes refuse a press, if they do: they stay in their row
    /// whatever the preview shows, so the preview beneath them keeps its
    /// place as a file is chosen and its preview read, and wait for a
    /// preview with text, as the stash browser's do.
    /// A preview that is a notice, as one past the text preview's limits or
    /// a binary file's, gives the notice's own words, and a failed read the
    /// read's error: both once said the file had no patch or source.
    fn modes_refusal(&self) -> Option<SharedString> {
        match self.content.as_deref() {
            Some(Content::Text { .. }) => None,
            Some(Content::Notice(notice)) => Some(notice.clone().into()),
            Some(_) => Some(NO_TEXT.into()),
            None if self.reading_preview() => Some(READING_PREVIEW.into()),
            None => match &self.preview_error {
                Some(error) => Some(error.clone().into()),
                None if self.selected_file.is_some() => Some(NO_TEXT.into()),
                None => Some(CHOOSE_FILE.into()),
            },
        }
    }
    /// Why Diagrams refuses a press, if it does: it stays in the modes' row
    /// for every preview, as the other modes do, and waits for text with
    /// Mermaid diagrams, saying so where a preview has none; with no preview
    /// it gives the row's own reason ([`Self::modes_refusal`]). Left out of
    /// the row unless the text had diagrams, its return wrapped the row at
    /// 18 pt in a 461 × 490 window and moved the preview's box.
    fn diagrams_refusal(&self) -> Option<SharedString> {
        match self.content.as_deref() {
            Some(Content::Text {
                diagrams: Some(_), ..
            }) => None,
            Some(Content::Text { old, new, .. }) => Some(self.no_diagrams(old, new).into()),
            Some(Content::Images { .. } | Content::Rich(_)) => Some(NOT_DIAGRAMS.into()),
            _ => self.modes_refusal(),
        }
    }
    /// Why the chosen file's text, `old` and `new`, has no diagrams: a
    /// Mermaid file stored as a link, or whose Git LFS object the local
    /// store lacks, says so, since what it stands for could hold diagrams,
    /// and only one whose text was read says it has none. "This file has no
    /// Mermaid diagrams." once stood for every text file without them. It
    /// judges the side Diagrams would draw, the one after the change or, for
    /// a file the change removed, the one before: judged by either side, a
    /// Mermaid file changed from a link to a regular file read as stored as
    /// a link.
    fn no_diagrams(&self, old: &str, new: &str) -> &'static str {
        let Some(file) = self.selected_file.and_then(|index| self.files.get(index)) else {
            return NOT_DIAGRAMS;
        };
        let (path, mode, text) = if file.new_path.is_some() {
            (file.new_path.as_deref(), file.new_mode.as_str(), new)
        } else {
            (file.old_path.as_deref(), file.old_mode.as_str(), old)
        };
        let mermaid = path
            .and_then(|path| path.extension())
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("mmd") || extension.eq_ignore_ascii_case("mermaid")
            });
        if !mermaid {
            NOT_DIAGRAMS
        } else if mode == "120000" {
            LINKED
        } else if gitturtle_preview::detect_lfs_pointer(text.as_bytes()).is_some() {
            NO_LFS_OBJECT
        } else {
            NO_DIAGRAMS
        }
    }
    /// Why Edit local commits… refuses a press, and whether it looks it:
    /// once the review's repository is no longer the one shown, that;
    /// otherwise the palette's own reason for its Edit local commits…
    /// (`CommandId::Rebase`), a running write's, which it takes the
    /// disabled look for only once the write looks busy, or a merge, rebase
    /// or cherry-pick under way, which the branch menu leaves the item out
    /// for. Mid-merge it was offered, and opened the form over the merge.
    fn edit_refusal(&self, window: &Window, cx: &App) -> Option<(SharedString, bool)> {
        let Some(owner) = self.owner.upgrade() else {
            return Some((NOT_SHOWN.into(), true));
        };
        let owner = owner.read(cx);
        if owner.path.as_deref() != Some(self.repo.path()) || owner.page != AppPage::Repository {
            return Some((NOT_SHOWN.into(), true));
        }
        let reason =
            owner.menu_command_reason(crate::command_palette::CommandId::Rebase, window, cx)?;
        let dimmed = owner.operation_busy.is_none() || owner.operation_shown().is_some();
        Some((reason.into(), dimmed))
    }
    /// Edit local commits…, beside the line saying no rebase captured a
    /// series: it opens the form as the menu's item does, and is refused
    /// with the palette's reason where the palette refuses its own
    /// ([`Self::edit_refusal`]).
    fn edit_commits(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let refusal = self.edit_refusal(window, cx);
        let dimmed = refusal.as_ref().is_some_and(|(_, dimmed)| *dimmed);
        let refused = refusal.is_some();
        let action = crate::workspace::refusing(
            crate::empty_state::action("series-edit-commits", EDIT_COMMITS, "")
                // The body's own handle, which a test reads focus from.
                .track_focus(&self.body.focus("series-edit-commits", cx))
                .on_click(cx.listener(|this, _, window, cx| {
                    // Refused before the review closes.
                    if this.edit_refusal(window, cx).is_some() {
                        return;
                    }
                    let owner = this.owner.clone();
                    this.close(window, cx);
                    let _ = owner.update(cx, |owner, cx| {
                        owner.open_interactive_rebase(None, window, cx)
                    });
                })),
            || "series-edit-commits".into(),
            refused,
            dimmed,
        );
        described(
            action,
            "series-edit-commits",
            refusal.map(|(reason, _)| reason),
        )
    }
    /// Why each action beneath the series refuses a press, if it does:
    /// Review messages, Inspect original files, Inspect rewritten files,
    /// Check publication destination… and Refresh series ([`refusing`]).
    /// With no series, `empty` says why, the first four wait for one.
    fn refusals(
        &self,
        empty: Option<&SharedString>,
        selected: Option<&gitturtle_core::SeriesRow>,
    ) -> [Option<SharedString>; 5] {
        let side = |missing: bool, why: &'static str| {
            empty
                .cloned()
                .or_else(|| missing.then(|| SharedString::from(why)))
        };
        let checking = self.network_pending.then(|| SharedString::from(CHECKING));
        [
            empty.cloned(),
            side(
                selected.is_some_and(|row| row.original.is_none()),
                "No original commit matches this one. Inspect rewritten files shows it.",
            ),
            side(
                selected.is_some_and(|row| row.rewritten.is_none()),
                "No rewritten commit matches this one. Inspect original files shows it.",
            ),
            empty.cloned().or_else(|| checking.clone()),
            if self.pending {
                Some(READING.into())
            } else {
                checking
            },
        ]
    }
    fn select(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .review
            .as_ref()
            .is_none_or(|review| row >= review.rows.len())
        {
            return;
        }
        self.selected = row;
        self.focus.focus(window, cx);
        self.keep_row_whole(row, window, cx);
        cx.notify();
    }
    /// Scrolls the series list the least that shows `row` whole, to a row's
    /// edge, so each row the list draws is whole ([`rows_shown`]): from a
    /// row's edge, the selected row's top or foot meets the list's; from
    /// between rows, as a wheel leaves it, the nearest row's edge that keeps
    /// the row whole. Centred, a two-row list showed half rows above and
    /// below the selection.
    fn keep_row_whole(&self, row: usize, window: &Window, cx: &App) {
        let rows = self.review.as_ref().map_or(0, |review| review.rows.len());
        let shown = rows_shown(rows, window, cx);
        let height = f32::from(series_row_height(window));
        let scroll = self.rows_scroll.0.borrow().base_handle.clone();
        let offset = scroll.offset();
        let top = (-f32::from(offset.y) / height).round() * height;
        // The least and most offsets that show the row whole, each at a
        // row's edge, and the list's reach.
        let least = row.saturating_sub(shown - 1) as f32 * height;
        let most = row as f32 * height;
        let reach = rows.saturating_sub(shown) as f32 * height;
        let top = top.clamp(least, most).clamp(0., reach);
        scroll.set_offset(point(offset.x, px(-top)));
    }
    /// Keeps the selected row whole as the series list's rows change under
    /// it with no key, in the frame that changes them: where the list shows
    /// another count of whole rows (`shown`), as a resize or a text size
    /// makes it, or its rows take another snapped height (`height`), as a
    /// display scale or a text size makes them, its offset is carried over
    /// in rows and the list scrolled the least that shows the selected row
    /// whole ([`Self::keep_row_whole`]). Only a key revealed it: tiled from a
    /// 1,480 × 800 window, where the list showed five rows, to a 461 × 490
    /// half tile, where it shows one, it showed row 1 with row 5 selected,
    /// and Return inspected a row nobody saw.
    fn follow_rows(&mut self, shown: usize, height: Pixels, window: &Window, cx: &App) {
        let Some((was_shown, was_height)) = self.rows_layout.replace((shown, height)) else {
            return;
        };
        if (was_shown, was_height) == (shown, height) {
            return;
        }
        if was_height != height && was_height > Pixels::ZERO {
            let scroll = self.rows_scroll.0.borrow().base_handle.clone();
            let offset = scroll.offset();
            let rows = f32::from(offset.y) / f32::from(was_height);
            scroll.set_offset(point(offset.x, height * rows));
        }
        self.keep_row_whole(self.selected, window, cx);
    }
    /// Lists `review` with its first row selected, shown whole by the least
    /// scroll: a reload, and a destination check that found the remote
    /// changed, selected row 0 and left the list where the last series had
    /// scrolled it, showing another row.
    fn list(&mut self, review: RewriteReview, window: &Window, cx: &App) {
        self.review = Some(Arc::new(review));
        self.selected = 0;
        self.keep_row_whole(0, window, cx);
    }
    fn inspect(&mut self, original: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(review) = &self.review else {
            return;
        };
        let Some(row) = review.rows.get(self.selected) else {
            return;
        };
        let commit = if original {
            row.original.map(|index| &review.original[index].commit)
        } else {
            row.rewritten.map(|index| &review.rewritten[index].commit)
        };
        let Some(commit) = commit else {
            return;
        };
        let oid = commit.oid.clone();
        self.inspect_label = format!(
            "{} {} · {}",
            if original { "Original" } else { "Rewritten" },
            short_oid(&oid),
            commit.subject
        );
        self.cancel_read();
        self.clear_content();
        self.inspecting = true;
        self.back.focus(window, cx);
        self.messages = None;
        self.files.clear();
        self.error = None;
        self.pending = true;
        let generation = self.generation;
        let response = self.worker.submit(Job::Changes {
            repo: self.repo.clone(),
            oid,
            parent: 0,
        });
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, _, cx| {
                if this.generation != generation || !this.current(cx) || !this.inspecting { return; }
                this.pending = false; this.task = None;
                match result { Ok(Ok(Output::Changes(files, _))) => { this.files = files; this.files_scroll.scroll_to_item(0, ScrollStrategy::Top); }, Ok(Err(error)) => this.error = Some(format!("{error:#}")), _ => this.error = Some("Changed-file read was interrupted. Return to the series and select the commit again.".into()) }
                cx.notify();
            });
        }));
        cx.notify();
    }
    /// Return's inspection: the selected row's rewritten commit, or its
    /// original where the row has none, as a dropped commit has only its
    /// original side, so Return always inspects the row's commit.
    fn inspect_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let original = self
            .review
            .as_ref()
            .and_then(|review| review.rows.get(self.selected))
            .is_some_and(|row| row.rewritten.is_none());
        self.inspect(original, window, cx);
    }
    fn inspect_messages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(review) = &self.review else {
            return;
        };
        let Some(row) = review.rows.get(self.selected) else {
            return;
        };
        let messages = [
            row.original
                .map(|index| review.original[index].commit.message.clone()),
            row.rewritten
                .map(|index| review.rewritten[index].commit.message.clone()),
        ];
        self.cancel_read();
        self.clear_content();
        self.inspecting = true;
        self.back.focus(window, cx);
        self.inspect_label = "Original and rewritten commit messages".into();
        self.messages = Some(messages.map(|message| {
            message.map(|message| text::editor(&message, "text", None, window, cx))
        }));
        cx.notify();
    }
    fn file(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(file) = self.files.get(index).cloned() else {
            return;
        };
        self.cancel_read();
        self.clear_content();
        self.selected_file = Some(index);
        self.pending = true;
        self.error = None;
        let generation = self.generation;
        let response = self.worker.submit(Job::Preview {
            origins: Default::default(),
            repo: self.repo.clone(),
            file,
        });
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.generation != generation || !this.current(cx) || !this.inspecting {
                    return;
                }
                this.pending = false;
                this.task = None;
                match result {
                    Ok(Ok(Output::Preview(content, _))) => {
                        this.content = Some(content);
                        this.prepare_editor(window, cx);
                    }
                    // Said in the preview's box, where the preview would
                    // draw: said at the body's end, below the box that
                    // still asked for a file to be chosen, it lay out of
                    // view in a short window.
                    Ok(Err(error)) => this.preview_error = Some(format!("{error:#}")),
                    _ => {
                        this.preview_error =
                            Some("File preview was interrupted. Select it again.".into())
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    /// Whether a chosen file's read, which the preview's box says in its
    /// middle, lies below the body's view as last drawn, as at 18 pt in a
    /// 461 × 490 window, where the preview lies wholly below the fold: the
    /// chosen row says it then. A press never moves what was pressed, so the
    /// body never scrolls to show the read; scrolled ~170 px, the file list
    /// slid out from under the pointer.
    fn preview_read_hidden(&self, window: &Window) -> bool {
        self.reading_preview()
            && self.preview_bounds.get().is_some_and(|preview| {
                let line = crate::empty_state::block_height(1, false, window);
                preview.center().y + line / 2. > self.body.scroll().bounds().bottom()
            })
    }
    /// The text mode the preview draws: the person's choice, except that
    /// Diagrams, while the chosen file's text has no diagrams for it to
    /// draw, gives way to Patch, the file's default text mode as Compare's
    /// Unified is, without forgetting the choice, which a file with diagrams
    /// draws again. The modes' selected look, the editor the box builds and
    /// the ring on its edge follow it. Left as the drawn mode, Diagrams drew
    /// no mode selected, while the row refused it, and a bare sentence in
    /// the box.
    fn drawn_mode(&self) -> usize {
        match self.content.as_deref() {
            Some(Content::Text { diagrams: None, .. }) if self.text_mode == 3 => 0,
            _ => self.text_mode,
        }
    }
    fn prepare_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mode = self.drawn_mode();
        if mode == 3 {
            return;
        }
        let Some(Content::Text {
            patch,
            old,
            new,
            presentation,
            ..
        }) = self.content.as_deref()
        else {
            return;
        };
        let value = [patch, old, new][mode];
        let (editor, decoration) = text::editor_with_decorations(
            value,
            if mode == 0 { "diff" } else { "text" },
            (mode == 0).then_some(presentation.as_ref()),
            window,
            cx,
        );
        self.decoration = decoration;
        self.patch = (mode == 0).then(|| diff_view::new(editor.clone(), presentation, window, cx));
        self.editor = Some(editor);
    }
    fn check_remote(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(review) = self.review.clone() else {
            return;
        };
        if self.network_pending || !self.current(cx) {
            return;
        }
        let remote = self.remote.read(cx).value().trim().to_owned();
        let branch = self.remote_branch.read(cx).value().trim().to_owned();
        let repo = self.repo.clone();
        let form = cx.entity().downgrade();
        let _ = self.owner.update(cx, |owner, cx| {
            if owner.operation_busy.is_some() { return; }
            owner.operation_busy = Some("Checking rewrite publication destination…");
            let control = owner.begin_operation_control(window, cx); self.network_control = Some(control.clone()); self.network_pending = true; self.publication = None; self.error = None;
            let response = owner.operations.submit_controlled(control, move || repo.inspect_rewrite_publication(&review, &remote, &branch));
            owner.operation_task = Some(cx.spawn_in(window, async move |owner, cx| {
                let result = response.await.unwrap_or_else(|_| Err(anyhow::anyhow!("Remote inspection ended without a result. Check it again explicitly before publishing")));
                let _ = owner.update_in(cx, |owner, window, cx| {
                    owner.operation_busy = None; owner.finish_operation_control(window, cx); owner.read_deferred_working(window, cx);
                    let path = owner.path.clone(); let page = owner.page;
                    if let Some(form) = form.upgrade() { form.update(cx, |form, cx| {
                        form.network_pending = false; form.network_control = None;
                        if form.closed || page != AppPage::Repository || path.as_deref() != Some(form.repo.path()) { return; }
                        match result {
                            Ok(PublicationInspection::Ready(plan)) => { form.publication = Some(plan); form.notice = None; },
                            Ok(PublicationInspection::RemoteChanged(review)) => { form.list(review, window, cx); form.publication = None; form.notice = Some("The remote changed. Original now shows its newly inspected series. Review these commits and files, then check the destination again to prepare a fresh lease.".into()); },
                            Ok(PublicationInspection::AlreadyPublished { remote, remote_ref, oid }) => { form.publication = None; form.notice = Some(format!("{remote} · {remote_ref} already points to {oid}. No publication is needed.")); },
                            Err(error) => form.error = Some(format!("{error:#}"))
                        }
                        cx.notify();
                    }); }
                    cx.notify();
                });
            })); cx.notify();
        });
        cx.notify();
    }
    fn publish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(plan) = self.publication.clone() else {
            return;
        };
        if self.network_pending
            || !self.current(cx)
            || plan.remote != self.remote.read(cx).value().trim()
            || plan.remote_ref
                != format!("refs/heads/{}", self.remote_branch.read(cx).value().trim())
        {
            return;
        }
        self.close(window, cx);
        let _ = self.owner.update(cx, |owner, cx| {
            if owner.path.as_deref() == Some(plan.root.as_path()) {
                owner.write(
                    WriteCommand::PublishRewrite(Arc::new(plan)),
                    "Publishing rewritten branch…",
                    window,
                    cx,
                );
            }
        });
    }
    /// The focus of the one editor the preview shows, the patch or the
    /// source, or of a failure said in its place, none of which draws a ring
    /// of its own: the preview rings its own edge for it, which a ring
    /// outside the box would lose to the box's clip. The commit messages,
    /// two editors side by side, ring each its own text instead
    /// ([`Self::render_content`]).
    fn text_focus(&self, cx: &App) -> Vec<FocusHandle> {
        if self.messages.is_some() {
            return Vec::new();
        }
        let mode = self.drawn_mode();
        match self.content.as_deref() {
            Some(Content::Text { .. }) if mode == 3 => Vec::new(),
            Some(Content::Text { .. }) if mode == 0 => self
                .patch
                .iter()
                .map(|patch| patch.read(cx).focus_handle(cx))
                .collect(),
            Some(Content::Text { .. }) => self
                .editor
                .iter()
                .map(|editor| editor.read(cx).focus_handle(cx))
                .collect(),
            None if self.preview_error.is_some() => vec![self.failure_focus.clone()],
            _ => Vec::new(),
        }
    }

    /// Scrolls the failure in the preview's box a line, `down` or up, as far
    /// as its ends.
    fn scroll_failure(&self, down: bool, window: &Window, cx: &mut Context<Self>) {
        let scroll = &self.failure_scroll;
        let line = crate::empty_state::block_height(1, false, window);
        let (at, end) = (-scroll.offset().y, scroll.max_offset().y.max(Pixels::ZERO));
        let to = (if down { at + line } else { at - line }).clamp(Pixels::ZERO, end);
        if to != at {
            scroll.set_offset(point(scroll.offset().x, -to));
            cx.notify();
        }
    }

    /// The inspected file, `width` wide and `height` tall as its box is in
    /// the frame it is drawn.
    fn render_content(
        &self,
        width: Pixels,
        height: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(messages) = &self.messages {
            // Each message's text, below its heading, rings its own 1 px edge
            // while its editor holds focus from the keyboard, as Split's
            // sides do (`FocusRing::edge`): one ring around the whole
            // preview for either editor told nothing as Tab moved between
            // them.
            let ring = self
                .owner
                .upgrade()
                .map(|owner| owner.read(cx).focus_ring.clone());
            let accent = palette(cx).accent;
            let mut edges: Vec<_> = messages
                .iter()
                .map(|editor| {
                    let focus = editor.as_ref()?.read(cx).focus_handle(cx);
                    ring.as_ref()?.edge(&focus, rgb(accent))
                })
                .collect();
            return div()
                .size_full()
                .flex()
                .gap_2()
                .children(messages.iter().enumerate().map(|(side, editor)| {
                    let edge = edges[side].take();
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p_2()
                        .child(if side == 0 {
                            "Original message"
                        } else {
                            "Rewritten message"
                        })
                        .child(editor.as_ref().map_or_else(
                            || {
                                div()
                                    .child("Commit absent in this series")
                                    .into_any_element()
                            },
                            |editor| {
                                div()
                                    .debug_selector(move || format!("series-message-{side}"))
                                    .flex_1()
                                    .min_h_0()
                                    .relative()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        editor_find::Editor::new(editor)
                                            .readonly(true)
                                            .size_full()
                                            .aria_label(if side == 0 {
                                                "Original commit message"
                                            } else {
                                                "Rewritten commit message"
                                            }),
                                    )
                                    .children(edge)
                                    .into_any_element()
                            },
                        ))
                }))
                .into_any_element();
        }
        // Diagrams is drawn only for text with diagrams ([`Self::drawn_mode`]).
        let mode = self.drawn_mode();
        match self.content.as_deref() {
            Some(Content::Text {
                diagrams: Some(preview),
                ..
            }) if mode == 3 => {
                rich_preview::render_comparison(preview, false, self.opener(cx), width, window, cx)
            }
            Some(Content::Text { .. }) if mode == 0 => self.patch.as_ref().map_or_else(
                || div().into_any_element(),
                |patch| patch.clone().into_any_element(),
            ),
            Some(Content::Text { .. }) => self.editor.as_ref().map_or_else(
                || div().into_any_element(),
                |editor| {
                    editor_find::Editor::new(editor)
                        .readonly(true)
                        .size_full()
                        .aria_label("Commit file source")
                        .into_any_element()
                },
            ),
            Some(Content::Rich(preview)) => {
                rich_preview::render_comparison(preview, false, self.opener(cx), width, window, cx)
            }
            Some(Content::Images { old, new }) => div()
                .size_full()
                .flex()
                .children(
                    [("Before", old), ("After", new)]
                        .into_iter()
                        .map(|(label, side)| {
                            div()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .p_2()
                                .child(if side.animation.is_some() {
                                    format!("{label} · first frame")
                                } else {
                                    label.into()
                                })
                                .child(
                                    div()
                                        .flex_1()
                                        .min_h_0()
                                        .when_some(side.render.clone(), |element, image| {
                                            element.child(crate::gif_playback::static_image(image))
                                        })
                                        .when(side.render.is_none(), |element| {
                                            element.child(
                                                side.message.clone().unwrap_or("Absent".into()),
                                            )
                                        }),
                                )
                        }),
                )
                .into_any_element(),
            Some(Content::Notice(message)) => div().p_3().child(message.clone()).into_any_element(),
            // The chosen file's preview being read says so in its own box,
            // which keeps its place and size for the preview.
            None if self.reading_preview() => {
                crate::empty_state::EmptyState::new("series-preview-reading", READING_PREVIEW)
                    .pane(cx)
            }
            _ => self.preview_prompt(height, window, cx),
        }
    }
    /// What the preview's box says with nothing to preview, as a designed
    /// empty state in the box (DESIGN.md, "Empty and edge states"): why the
    /// chosen file's preview could not be read, that the commit changed no
    /// files, or which to choose. A failed read said "Select a file…" here
    /// with its file chosen, its error drawn below the box at the body's
    /// end. A failure taller than the box, `height` tall, as a five-line
    /// error is at 18 pt in a 461 × 490 window, scrolls inside it, cued by
    /// its scrollbar as every scrolling list is (`tags::ScrollCue`): centred
    /// in the box, which clips, it lost its first and last lines' edges.
    /// While it overflows it is a Tab stop, as the GitHub confirmation's
    /// region is, which Up and Down scroll and whose focus rings the box's
    /// own edge ([`Self::text_focus`]) and, lying inside the box, reveals
    /// it. Made to fit while focused, it keeps focus and its ring until the
    /// next Tab, which leaves it for good; otherwise a failure that fits
    /// tracks no focus, so a press on it takes focus from nothing.
    fn preview_prompt(
        &self,
        height: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(error) = &self.preview_error {
            let state = crate::empty_state::EmptyState::new("series-preview-failed", error.clone())
                .titled(Some(PREVIEW_UNAVAILABLE))
                .centred(cx)
                .w_full()
                .min_h(height);
            let scroll = &self.failure_scroll;
            let cue = crate::tags::ScrollCue::new(scroll, Pixels::ZERO);
            let overflows = cue.kept();
            let focus = &self.failure_focus;
            let tracked = overflows || focus.is_focused(window);
            return cue
                .list(
                    div()
                        .on_children_prepainted(cue.observe(|_, _, _| {}))
                        .id("series-preview-failure")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(scroll)
                        .when(tracked, |failure| {
                            failure
                                // The handle carries the Tab stop: GPUI
                                // ignores the `tab_stop` of the div that
                                // tracks it.
                                .track_focus(&focus.clone().tab_stop(overflows))
                                .role(Role::Group)
                                .aria_label(PREVIEW_UNAVAILABLE)
                                .on_key_down(cx.listener(
                                    |this, event: &KeyDownEvent, window, cx| {
                                        let down = match event.keystroke.key.as_str() {
                                            "up" => false,
                                            "down" => true,
                                            _ => return,
                                        };
                                        this.scroll_failure(down, window, cx);
                                        cx.stop_propagation();
                                    },
                                ))
                        })
                        .child(state),
                    "series-preview-failure-scrollbar",
                )
                .size_full()
                .into_any_element();
        }
        let line = if self.files.is_empty() && !self.pending {
            NO_FILES
        } else {
            CHOOSE_FILE
        };
        crate::empty_state::EmptyState::new("series-preview-empty", line).pane(cx)
    }
    fn row(&self, index: usize, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(review) = &self.review else {
            return div().into_any_element();
        };
        let Some(row) = review.rows.get(index) else {
            return div().into_any_element();
        };
        let p = palette(cx);
        let selected = index == self.selected;
        let label = format!(
            "{}{}{}",
            row.change.label(),
            if row.reordered { " · reordered" } else { "" },
            if row.ambiguous {
                " · inspect correspondence"
            } else {
                ""
            }
        );
        let commit_labels = [
            (row.original, &review.original, "Original"),
            (row.rewritten, &review.rewritten, "Rewritten"),
        ]
        .map(|(commit_index, commits, side)| {
            commit_index.map_or_else(
                || (format!("{side}: absent"), "—".to_owned()),
                |commit_index| {
                    let commit = &commits[commit_index].commit;
                    let text = format!(
                        "{} · {} {}",
                        commit_index + 1,
                        short_oid(&commit.oid),
                        commit.subject
                    );
                    (format!("{side}: {text}"), text)
                },
            )
        });
        div()
            .id(("series-row", index))
            .debug_selector(move || format!("series-row-{index}"))
            .role(Role::ListBoxOption)
            .aria_selected(selected)
            .when(selected, |row| row.aria_active_descendant())
            .aria_position_in_set(index + 1)
            .aria_size_of_set(review.rows.len())
            .aria_label(format!(
                "{label}. {}. {}",
                commit_labels[0].0, commit_labels[1].0
            ))
            .w_full()
            .min_w_0()
            .h(series_row_height(window))
            .px_3()
            .py_2()
            .flex()
            .flex_col()
            .gap_1()
            .border_b_1()
            .border_color(rgb(p.border))
            .bg(rgb(if selected { p.selected } else { p.panel }))
            .hover(|style| style.bg(rgb(p.row_hover(selected))))
            .cursor_pointer()
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_size(appearance::ui_text(11.))
                    .text_color(rgb(p.muted))
                    .truncate()
                    .child(label),
            )
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .gap_3()
                    .children(commit_labels.into_iter().map(|(_, text)| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(appearance::ui_text(12.))
                            .child(text)
                    })),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.select(index, window, cx)))
            .into_any_element()
    }
}

impl Render for SeriesBrowser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        // The body takes the room the dialog's height cap leaves it as the
        // dialog lays out its title, footer and padding: the `min_h_0` chain
        // from the dialog's content down to the body shrinks it there, and
        // an inspector, which fills that room, asks for the whole cap. A
        // window less 250 px once stood for that room, a bound the body
        // stopped at: at 13 pt in a 461 × 490 window it ended the dialog
        // 53.5 px short of its cap's foot.
        let cap = appearance::dialog_max_height(window);
        // The body scrolls, so it clips its controls on both axes. It keeps
        // the focus ring's room around them, giving it back through its
        // margin, and a wrapper keeps the room below them inside the
        // dialog's clip, so the inputs and buttons at its edges draw their
        // rings whole, as the profile form does.
        let room = appearance::button_ring_room(cx);
        // Its actions and Back to series, ghost Buttons at the dialog's text
        // edge, stand the label or icon of each line's first on that edge,
        // their frames their pull before it (DESIGN.md, "One left edge"), so
        // the body's left side keeps the pull with the ring's room.
        let lead = crate::ghost_pull(window) + room;
        // A control Tab moves onto scrolls into view with its ring's room
        // (`focus_reveal`): the series, each row of actions and fields, and
        // Back to series.
        self.body.reveal(window, cx);
        let [
            messages,
            original,
            rewritten,
            reload,
            check,
            stop,
            publish,
            cancel,
        ] = [
            "inspect-series-messages",
            "inspect-original-commit",
            "inspect-rewritten-commit",
            "reload-series",
            "inspect-publication-remote",
            "cancel-remote-inspection",
            "publish-reviewed-rewrite",
            "cancel-publication-review",
        ]
        .map(|id| self.body.focus(id, cx));
        let [remote, remote_branch] =
            [&self.remote, &self.remote_branch].map(|field| field.read(cx).focus_handle(cx));
        let modes: Vec<_> = (0..4usize)
            .map(|mode| self.body.focus(("series-text-mode", mode), cx))
            .collect();
        // While a commit is inspected, its file list and its preview, each
        // tracked by a handle no Tab stops on, so focus on a row or on a
        // control inside the preview reveals the whole list or the preview:
        // at 18 pt in a 461 × 490 window both lie past the body's fold, and
        // Tab onto them drew no ring. The list keeps its rows' ring room
        // inside its edge, which cut every side of a row's ring, and the
        // text modes wrap, as After source ran past the body's right edge.
        // A press on either has GPUI focus only the control pressed, whose
        // own listener runs first, never the wrapper's handle, which draws
        // no ring and which a press beside the controls focused, so the next
        // key showed none anywhere.
        let [files_focus, preview_focus] =
            ["series-files", "series-file-preview"].map(|id| self.body.focus(id, cx));
        // The preview's one editor, the patch or the source, which draws no
        // ring of its own, rings the preview's own 1 px edge while it holds
        // focus from the keyboard, as the series does its list's and
        // Compare's text pane its own (`FocusRing::edge`): focused by Tab,
        // the source drew no ring at all. The commit messages ring each its
        // own text (`render_content`).
        let text_ringed = self.owner.upgrade().is_some_and(|owner| {
            let ring = &owner.read(cx).focus_ring;
            self.text_focus(cx).iter().any(|focus| ring.shows(focus))
        });
        let cue = crate::tags::ScrollCue::revealing(&self.body, room);
        // The series draws the window's keyboard ring on its own 1 px edge,
        // after a key, until a pointer press.
        let ringed = self
            .owner
            .upgrade()
            .is_some_and(|owner| owner.read(cx).focus_ring.shows(&self.focus));
        let series = cx.entity().downgrade();
        let row_count = self.review.as_ref().map_or(0, |review| review.rows.len());
        // Whole rows inside the list's 1 px border, and while they overflow
        // it the scrollbar every scrolling list cues the rest with: a list
        // cut at the rest of the body's room ended mid-row with nothing to
        // say more lay below.
        let row_height = series_row_height(window);
        let shown = rows_shown(row_count, window, cx);
        if row_count > 0 {
            self.follow_rows(shown, row_height, window, cx);
        }
        let list_height = row_height * shown as f32 + list_border(window) * 2.;
        let rows_cue =
            crate::tags::ScrollCue::new(&self.rows_scroll.0.borrow().base_handle, Pixels::ZERO);
        let gutter = crate::tags::ScrollCue::gutter();
        let selected = self
            .review
            .as_ref()
            .and_then(|review| review.rows.get(self.selected))
            .cloned();
        let publication_valid = self.publication.as_ref().is_some_and(|plan| {
            plan.remote == self.remote.read(cx).value().trim()
                && plan.remote_ref
                    == format!("refs/heads/{}", self.remote_branch.read(cx).value().trim())
        });
        // With no series to show, the list's place says why (DESIGN.md,
        // "Empty and edge states"), and so do the list's accessible
        // description and each action the missing series refuses.
        let empty = (row_count == 0).then(|| self.empty_reason());
        let modes_refused = self.modes_refusal();
        let diagrams_refused = self.diagrams_refusal();
        let drawn_mode = self.drawn_mode();
        let [
            messages_refused,
            original_refused,
            rewritten_refused,
            check_refused,
            reload_refused,
        ] = self.refusals(
            empty.as_ref().map(|empty| &empty.description),
            selected.as_ref(),
        );
        div().debug_selector(|| "rewrite-review-content".into()).flex().flex_col().min_h_0().pb(room).child(cue.list(div().on_children_prepainted(cue.observe(self.body.observe())).id("rewrite-review-body").debug_selector(|| "rewrite-review-body".into()).min_w_0().min_h_0().p(room).m(-room).pl(lead).ml(-lead).when(self.inspecting, |element| element.h(cap)).overflow_y_scroll().track_scroll(self.body.scroll()).flex().flex_col().gap_2()
            .on_action(cx.listener(|this, _: &gpui_kit::component::input::Escape, window, cx| { this.back(window, cx); cx.stop_propagation(); }))
            .on_action(cx.listener(|this, _: &ClearSearch, window, cx| { this.back(window, cx); cx.stop_propagation(); }))
            .when(!self.inspecting, |element| element
                .child(div().text_size(appearance::ui_text(12.)).text_color(rgb(p.muted)).child("Original → rewritten · unique patch fingerprints identify likely correspondence. Changed patches may match by unique subject only. Ambiguous, combined and empty commits remain explicit; inspect both sides before publishing."))
                .when_some(self.review.as_ref(), |element, review| element.child(div().text_size(appearance::ui_text(11.)).child(format!("{} · original {} → rewritten {} · base {}", review.branch, short_oid(&review.original_head), short_oid(&review.rewritten_head), short_oid(&review.base)))))
                // Reported from a wrapper of its own: a list that tracks its
                // scroll hands its own children's bounds to the scroll
                // handle, not to its `on_children_prepainted`, so the
                // series, reported by the body itself, was never revealed
                // and its ring was cut at the body's foot at 18 pt in a
                // 461 × 490 window.
                // With no series, a designed empty state in the rows' place
                // says why, at the rows' text inset, the list as tall as
                // its wrapped lines need: the load's error, drawn at the
                // body's end in a scrolling box the body shrank to nothing
                // once it scrolled, left an empty list with no text.
                .child(div().flex().flex_col().flex_shrink_0().on_children_prepainted(self.body.control(self.focus.clone(), room)).child(div().when(row_count > 0, |list| list.on_children_prepainted(rows_cue.observe(|_, _, _| {}))).id("series-focus-list").debug_selector(|| "series-focus-list".into()).w_full().min_w_0().map(|list| if empty.is_some() { list.min_h(list_height) } else { list.h(list_height) }).flex_shrink_0().border_1().border_color(rgb(if ringed { p.accent } else { p.border })).rounded(px(6.)).overflow_hidden().track_focus(&self.focus).key_context(SERIES_CONTEXT).role(if empty.is_some() { Role::Group } else { Role::ListBox }).aria_label("Original and rewritten commits").aria_description(empty.as_ref().map_or_else(|| SERIES_KEYS.into(), |empty| empty.description.clone()))
                    // Return inspects as it goes down, never on a held key's
                    // repeats, as the dialog's own Enter acts. A key on Edit
                    // local commits… inside it is that Button's own.
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| { if !this.focus.is_focused(window) { return; } match event.keystroke.key.as_str() { "up" => this.select(this.selected.saturating_sub(1), window, cx), "down" => this.select(this.selected + 1, window, cx), "enter" if !event.is_held && gpui_kit::base::enter_press_began_on(&this.focus, window, cx) => this.inspect_selected(window, cx), "enter" => {}, _ => return } cx.stop_propagation(); }))
                    .map(|list| match empty {
                        Some(empty) => {
                            let line = crate::empty_state::EmptyState::new("series-empty", empty.line).titled(empty.title);
                            let line = if empty.uncaptured { line.action(self.edit_commits(window, cx)) } else { line };
                            // While Refresh series reads again, the state keeps
                            // the height the last one took at rest, so nothing
                            // below it moves.
                            let kept = self.empty_height.clone();
                            let record = !self.pending;
                            let first_read = self.pending && self.empty_height.get() == Pixels::ZERO;
                            list.child(div().on_children_prepainted(move |bounds, _, _| if record { kept.set(bounds.first().map_or(Pixels::ZERO, |state| state.size.height)); })
                                .child(div().px_3().py_2().when(self.pending, |state| state.min_h(self.empty_height.get()))
                                    // The first read, with no height kept,
                                    // holds the room of the state it most
                                    // likely lands in, no rebase captured
                                    // with Edit local commits… below its
                                    // line, measured at the list's width, so
                                    // the actions below never move on open:
                                    // kept from nothing, the reading line
                                    // took one line and they dropped.
                                    .map(|state| if first_read {
                                        state.child(div().relative().w_full().child(crate::empty_state::block_reserve(recovery_drafts::NO_CAPTURE, true)).child(div().absolute().top_0().left_0().w_full().child(line.block(cx))))
                                    } else {
                                        state.child(line.block(cx))
                                    })))
                        }
                        // The rows reach into the gutter the cue keeps
                        // beside them while they overflow, as the stash
                        // browser's saved files do, so the track runs along
                        // their scroll's right edge and covers no row.
                        None => list.flex().flex_col().child(uniform_list("rewrite-series-list", row_count, cx.processor(|this, range: std::ops::Range<usize>, window, cx| range.map(|index| this.row(index, window, cx)).collect::<Vec<_>>())).flex_1().min_h_0().mr(-gutter).pr(gutter).track_scroll(&self.rows_scroll)),
                    })
                    .map(|list| if row_count == 0 { crate::native_accessibility::clips_children(list).into_any_element() } else { rows_cue.list(list, "rewrite-series-scrollbar").into_any_element() })))
                .child(crate::edge_row(window).on_children_prepainted(action_row(self.pending, self.body.controls(vec![Some(messages.clone()), Some(original.clone()), Some(rewritten.clone()), Some(reload.clone())], room)))
                    .child(refusing(crate::edge_button("inspect-series-messages", "Review messages").track_focus(&messages).debug_selector(|| "inspect-series-messages".into()).on_click(cx.listener(|this, _, window, cx| this.inspect_messages(window, cx))), "inspect-series-messages", messages_refused))
                    .child(refusing(crate::edge_button("inspect-original-commit", "Inspect original files").track_focus(&original).debug_selector(|| "inspect-original-commit".into()).on_click(cx.listener(|this, _, window, cx| this.inspect(true, window, cx))), "inspect-original-commit", original_refused))
                    .child(refusing(crate::edge_button("inspect-rewritten-commit", "Inspect rewritten files").track_focus(&rewritten).debug_selector(|| "inspect-rewritten-commit".into()).on_click(cx.listener(|this, _, window, cx| this.inspect(false, window, cx))), "inspect-rewritten-commit", rewritten_refused))
                    .child(refusing(crate::edge_button("reload-series", "Refresh series").track_focus(&reload).debug_selector(|| "reload-series".into()).on_click(cx.listener(|this, _, window, cx| this.load(window, cx))), "reload-series", reload_refused)))
                .child(div().flex().flex_wrap().items_center().gap_2().on_children_prepainted(self.body.controls(vec![Some(remote), Some(remote_branch)], room))
                    .child(div().w(px(150.)).child(Input::new(&self.remote).aria_label("Publication remote").disabled(self.network_pending)))
                    .child(div().w(px(170.)).child(Input::new(&self.remote_branch).aria_label("Publication remote branch").disabled(self.network_pending))))
                // Check publication destination… and Cancel remote check,
                // ghost Buttons, on a line of their own below the fields,
                // pulled to the text edge as the actions above are
                // (`edge_row`): wrapped below the fields at 18 pt in a
                // 461 × 490 window, Check stood its frame on the edge and
                // its label its inset past it.
                .child(crate::edge_row(window).on_children_prepainted(self.body.controls(vec![Some(check.clone()), Some(stop.clone())], room))
                    .child(refusing(crate::edge_button("inspect-publication-remote", if self.network_pending { "Checking remote…" } else { "Check publication destination…" }).track_focus(&check).debug_selector(|| "inspect-publication-remote".into()).on_click(cx.listener(|this, _, window, cx| this.check_remote(window, cx))), "inspect-publication-remote", check_refused))
                    .when(self.network_pending, |element| element.child(crate::edge_button("cancel-remote-inspection", "Cancel remote check").track_focus(&stop).debug_selector(|| "cancel-remote-inspection".into()).on_click(cx.listener(|this, _, _, cx| { if let Some(control) = &this.network_control { control.cancel(); } cx.notify(); })))))
                .when_some(self.notice.as_ref(), |element, notice| element.child(div().text_size(appearance::ui_text(12.)).text_color(rgb(p.warning)).child(notice.clone())))
                .when_some(self.publication.as_ref(), |element, plan| element.child(div().p_3().flex_shrink_0().flex().flex_col().gap_2().border_1().border_color(rgb(p.warning)).rounded(px(6.))
                    .child(div().text_size(appearance::ui_text(12.)).child(format!("Publish to {} · {}\n{}\nExpected remote: {}\nReviewed replacement: {}", plan.remote, plan.remote_ref, workspace::display_remote_url(&plan.remote_url), plan.expected_remote_oid, plan.new_oid)))
                    .child(div().text_size(appearance::ui_text(11.)).child("This replaces the named remote branch's history. Collaborators may need to update their local branches. The exact lease refuses intervening remote updates; failures and uncertain outcomes are never retried automatically."))
                    .child(div().flex().gap_2().on_children_prepainted(self.body.controls(vec![Some(publish.clone()), Some(cancel.clone())], room)).child(button("publish-reviewed-rewrite", "Publish with exact lease", "", true).track_focus(&publish).disabled(!publication_valid || self.network_pending).on_click(cx.listener(|this, _, window, cx| this.publish(window, cx))))
                        .child(button("cancel-publication-review", "Cancel publication review", "", false).track_focus(&cancel).on_click(cx.listener(|this, _, _, cx| { this.publication = None; cx.notify(); }))))))
            )
            .when(self.inspecting, |element| element
                // Back to series stands its icon on the text edge; the
                // inspected commit beside it takes the rest of the row and
                // wraps there, never onto a line of its own, which would
                // start at the row's pulled left, its pull before the edge.
                .child(crate::edge_row(window).items_center().on_children_prepainted(self.body.controls(vec![Some(self.back.clone())], room)).child(button("back-rewrite-series", "Back to series", "arrow-left", false).track_focus(&self.back).debug_selector(|| "back-rewrite-series".into()).flex_shrink(1.).on_click(cx.listener(|this, _, window, cx| this.back(window, cx)))).child(div().debug_selector(|| "rewrite-inspect-label".into()).flex_1().min_w_0().text_size(appearance::ui_text(12.)).child(self.inspect_label.clone())))
                // The changed files being read say so in the file box's
                // place, at the rows' inset, the box keeping its fixed
                // height: said after the preview, the read lay below the
                // view at 18 pt in a 461 × 490 window.
                .when(self.messages.is_none(), |element| element.child(div().debug_selector(|| "series-files".into()).track_focus(&files_focus).on_any_mouse_down(|_, window, _| window.prevent_default()).on_children_prepainted(self.body.control(files_focus.clone(), room)).h(px(130.)).flex_shrink_0().border_1().border_color(rgb(p.border)).rounded(px(6.)).overflow_hidden().map(|files| if self.reading_files() { files.child(div().px_3().py_2().child(crate::empty_state::EmptyState::new("series-files-reading", READING_FILES).block(cx))) } else { files.child(uniform_list("series-files", self.files.len(), cx.processor(|this, range: std::ops::Range<usize>, window, cx| range.map(|index| {
                    let file = &this.files[index];
                    let control = button(("series-file", index), file.path().to_string_lossy().into_owned(), "", this.selected_file == Some(index)).flex_1().min_w_0().h(appearance::ui_size(30.)).on_click(cx.listener(move |this, _, window, cx| this.file(index, window, cx)));
                    // The chosen row says its preview's read where the
                    // preview lies below the view, in its accessible
                    // description too, and nothing scrolls.
                    let reading = this.selected_file == Some(index) && this.preview_read_hidden(window);
                    let control = if reading { Described(control).aria_description(SharedString::from(READING_PREVIEW)).0 } else { control };
                    div().w_full().h(appearance::ui_size(30.)).flex().items_center().gap_2().child({ use native_accessibility::DrawnButton as _; control.node_recorded() })
                        .when(reading, |row| row.child(div().debug_selector(|| "series-file-reading".into()).flex_shrink_0().pr_2().text_size(appearance::ui_text(11.)).text_color(rgb(palette(cx).muted)).child(READING_ROW)))
                        .into_any_element()
                }).collect::<Vec<_>>())).size_full().p(room).track_scroll(&self.files_scroll)) })))
                // The text modes keep their row for every preview, waiting
                // for one with text and saying why, so the preview below
                // keeps its place and size as its read comes and goes and
                // from one file to the next: the row left as an image or
                // document landed and came back with the next choice,
                // moving the preview 34.5 px each way.
                .when(self.messages.is_none(), |element| element.child(div().flex().flex_wrap().gap_2().on_children_prepainted(self.body.controls(modes.iter().cloned().map(Some).collect(), room)).children(TEXT_MODES.into_iter().enumerate().map(|(mode, (label, key))| { let refused = if mode == 3 { diagrams_refused.clone() } else { modes_refused.clone() }; refusing(button(("series-text-mode", mode), label, "", drawn_mode == mode && refused.is_none()).debug_selector(move || key.into()).track_focus(&modes[mode]).on_click(cx.listener(move |this, _, window, cx| { this.text_mode = mode; this.prepare_editor(window, cx); cx.notify(); })), key, refused) }))))
                // Built for the box's width in the frame it is drawn
                // (`in_frame`), so the open controls take the form that
                // width leaves them in the first frame after a resize.
                .child(div().debug_selector(|| "series-file-preview".into()).track_focus(&preview_focus).on_any_mouse_down(|_, window, _| window.prevent_default()).on_children_prepainted(self.body.control(preview_focus.clone(), room)).flex_1().min_h(appearance::ui_size(100.)).flex().flex_col().border_1().border_color(rgb(if text_ringed { p.accent } else { p.border })).overflow_hidden().child(crate::in_frame::in_frame(move |bounds, window, cx| series.update(cx, |this, cx| { this.preview_bounds.set(Some(bounds)); this.render_content(bounds.size.width, bounds.size.height, window, cx) }).unwrap_or_else(|_| div().into_any_element())).named("rewrite-comparison"))))
            // A failed publication check or push, below the series it
            // concerns; the load's own error says why in the list's place.
            // It keeps its height in the scrolling body, which shrank a
            // scrolling box with no least height of its own to nothing.
            .when_some(self.error.as_ref().filter(|_| self.review.is_some()), |element, error| element.child(div().max_h(appearance::ui_size(100.)).flex_shrink_0().id("series-review-error").overflow_y_scroll().text_size(appearance::ui_text(12.)).text_color(rgb(p.warning)).child(error.clone()))), "rewrite-review-scrollbar").min_h_0())
    }
}

/// Presses Check publication destination… in a series review of `app`'s
/// repository that names only its worktree, for the regression of the reads
/// a check's end runs in `page_navigation::tests`. The check needs a
/// remote the fixture has none of, so Git refuses it as soon as it runs.
#[cfg(test)]
pub(crate) fn check_publication(app: &Entity<GitTurtle>, window: &mut Window, cx: &mut App) {
    let repo = app.read(cx).repository.clone().expect("a repository");
    let review = RewriteReview {
        root: repo.path().to_owned(),
        branch: "main".into(),
        base: String::new(),
        original_head: String::new(),
        rewritten_head: String::new(),
        original: Vec::new(),
        rewritten: Vec::new(),
        rows: Vec::new(),
    };
    let browser = cx.new(|cx| {
        SeriesBrowser::new(
            app.downgrade(),
            repo,
            "origin".into(),
            "main".into(),
            window,
            cx,
        )
    });
    browser.update(cx, |browser, cx| {
        browser.review = Some(Arc::new(review));
        browser.check_remote(window, cx);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;

    /// The series review's unified patch keeps its decorations for the editor's life, and its
    /// preview figure counts them along with the presentation they were built from.
    #[gpui::test]
    async fn series_patch_preview_counts_its_decorations_and_presentation(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let repository = GitRepository::init(fixture.path().join("repository"), "main").unwrap();
        cx.update(gpui_kit::init);
        let captured: std::rc::Rc<std::cell::RefCell<Option<Entity<SeriesBrowser>>>> =
            Default::default();
        let output = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let browser = cx.new(|cx| {
                SeriesBrowser::new(
                    WeakEntity::new_invalid(),
                    repository,
                    "origin".into(),
                    "main".into(),
                    window,
                    cx,
                )
            });
            *output.borrow_mut() = Some(browser.clone());
            gpui_kit::component::Root::new(browser, window, cx)
        });
        cx.run_until_parked();
        let browser = captured.borrow().as_ref().unwrap().clone();
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.content = Some(Arc::new(text::decorated_patch_content()));
                browser.text_mode = 0;
                browser.prepare_editor(window, cx);
                let Some(Content::Text { presentation, .. }) = browser.content.as_deref() else {
                    unreachable!()
                };
                let presentation = presentation.retained_bytes();
                let decoration = browser.decoration.clone().expect("kept with the editor");
                let decorated = decoration.retained_bytes();
                assert!(decorated > 0, "a patch with changes has decorations");

                let with = browser.preview_retained_bytes();
                browser.decoration = None;
                let without = browser.preview_retained_bytes();
                assert_eq!(with - without, decorated);
                browser.content = None;
                assert!(without - browser.preview_retained_bytes() >= presentation);

                // A source mode replaces the patch editor and releases its decorations.
                browser.content = Some(Arc::new(text::decorated_patch_content()));
                browser.decoration = Some(decoration);
                browser.text_mode = 1;
                browser.prepare_editor(window, cx);
                assert!(browser.decoration.is_none());
                browser.clear_content();
                assert_eq!(browser.preview_retained_bytes(), 0);
            })
        });
    }

    /// An inspected commit's description, beside Back to series in a row
    /// pulled before the dialog's text edge, takes the rest of that row and
    /// wraps there, at 13 and 18 pt in a 461 × 490 window: a subject longer
    /// than the room beside the Button wrapped onto a line of its own, which
    /// started at the row's pulled left, 10 px before the text edge at 13 pt
    /// and 14 px at 18 pt.
    #[gpui::test]
    async fn an_inspected_commits_description_wraps_beside_back_to_series(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::tags::tests::{draw, set_interface_text_size, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        let browser = cx.update(|window, cx| {
            let repo = app
                .read(cx)
                .repository
                .clone()
                .expect("the window's repository");
            let browser = cx.new(|cx| {
                SeriesBrowser::new(
                    app.downgrade(),
                    repo,
                    "origin".into(),
                    "main".into(),
                    window,
                    cx,
                )
            });
            open_series_dialog(browser.clone(), window, cx);
            browser
        });
        cx.update(|_, cx| {
            browser.update(cx, |browser, cx| {
                browser.inspecting = true;
                browser.inspect_label = format!(
                    "Rewritten 0123456789 · {}",
                    "Keep the subject of a rewritten commit whole ".repeat(3)
                );
                cx.notify();
            })
        });
        for interface in [13, 18] {
            set_interface_text_size(cx, &app, interface);
            let back = cx
                .debug_bounds("back-rewrite-series")
                .expect("Back to series");
            let label = cx
                .debug_bounds("rewrite-inspect-label")
                .expect("the inspected commit's description");
            assert!(
                label.left() >= back.right(),
                "at {interface} pt the description at {label:?} starts left of Back to series' \
                 right side, {:?}",
                back.right()
            );
            assert!(
                label.size.height > back.size.height,
                "at {interface} pt the description wraps beside the Button: {label:?}"
            );
        }
    }

    /// The series review over a window of the tagged fixture at 18 pt in a
    /// 461 × 490 window, inspecting its last commit, whose one changed file's
    /// preview has landed in Patch: the fixture's directory, the window's
    /// app, the review and the window.
    fn inspected_commit(
        cx: &mut gpui_kit::TestAppContext,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        Entity<SeriesBrowser>,
        &mut gpui_kit::VisualTestContext,
    ) {
        use crate::focus_ring::sweep;
        use crate::tags::tests::{draw, git, set_interface_text_size, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let notes = repo.path().join("notes.txt");
        for (text, message) in [
            ("first\n", "Add notes"),
            ("second\nthird\n", "Rewrite notes"),
        ] {
            std::fs::write(&notes, text).unwrap();
            git(repo.path(), &["add", "notes.txt"]);
            git(repo.path(), &["commit", "--quiet", "-m", message]);
        }
        // A fresh repository keeps its branch as a loose reference.
        let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
        let files = repo.changes_with_renames(head.trim(), 0).unwrap();
        let (app, cx) = window(cx, &repo);
        sweep::prepare(&app, cx);
        cx.simulate_resize(size(px(461.), px(490.)));
        set_interface_text_size(cx, &app, 18);
        let browser = cx.update(|window, cx| {
            let repo = app
                .read(cx)
                .repository
                .clone()
                .expect("the window's repository");
            let browser = cx.new(|cx| {
                SeriesBrowser::new(
                    app.downgrade(),
                    repo,
                    "origin".into(),
                    "main".into(),
                    window,
                    cx,
                )
            });
            open_series_dialog(browser.clone(), window, cx);
            browser
        });
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.inspecting = true;
                browser.inspect_label = "Rewritten".into();
                browser.files = files;
                browser.file(0, window, cx);
            })
        });
        for _ in 0..400 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(50));
            draw(cx);
            if browser.read_with(cx, |browser, _| browser.content.is_some()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            browser.read_with(cx, |browser, _| browser.content.is_some()),
            "the inspected file's preview lands"
        );
        draw(cx);
        (fixture, app, browser, cx)
    }

    /// What holds focus in the inspected commit, for a failure to name.
    fn focus_name(browser: &Entity<SeriesBrowser>, cx: &mut gpui_kit::VisualTestContext) -> String {
        cx.update(|window, cx| {
            let browser = browser.read(cx);
            let Some(focused) = window.focused(cx) else {
                return "nothing".into();
            };
            if focused == browser.back {
                return "Back to series".into();
            }
            if browser.text_focus(cx).contains(&focused) {
                return "the preview's editor".into();
            }
            for (id, name) in [
                ("series-files", "the file list's own handle"),
                ("series-file-preview", "the preview's own handle"),
            ] {
                if focused == browser.body.focus(id, cx) {
                    return name.into();
                }
            }
            format!("{focused:?}")
        })
    }

    /// The outlines the last frame painted in `color` of the palette, by
    /// their bounds.
    fn outlines_in(
        cx: &mut gpui_kit::VisualTestContext,
        color: impl Fn(&appearance::Palette) -> u32,
    ) -> Vec<Bounds<Pixels>> {
        let color: Hsla = cx.read(|cx| rgb(color(&palette(cx))).into());
        crate::focus_ring::painted_outlines(cx, color)
            .into_iter()
            .map(|outline| outline.bounds)
            .collect()
    }

    /// While a commit is inspected at 18 pt in a 461 × 490 window, its file
    /// list and its preview lie past the body's fold: Tab and Shift+Tab onto
    /// a file's row and into the preview reveal each, in Patch and in After
    /// source, and every stop draws a whole ring inside the body, the editor,
    /// which draws none of its own, on the preview's 1 px edge. Neither was
    /// revealed, so Tab past the text modes landed below the fold with no
    /// ring; the list cut its rows' rings on every side, After source ran
    /// past the body's right edge, and the editor, focused by Tab, drew no
    /// ring at all. A click in the editor focuses it unmarked, the preview's
    /// edge in the border's colour, as a click leaves every list.
    #[gpui::test]
    async fn an_inspected_commits_files_and_preview_scroll_into_view(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::focus_ring::sweep;
        use crate::tags::tests::draw;
        let (_fixture, app, browser, cx) = inspected_commit(cx);
        let mut failures = Vec::new();
        for (mode, name) in [(0, "Patch"), (2, "After source")] {
            cx.update(|window, cx| {
                browser.update(cx, |browser, cx| {
                    browser.text_mode = mode;
                    browser.prepare_editor(window, cx);
                    cx.notify();
                })
            });
            draw(cx);
            for key in ["tab", "shift-tab"] {
                let mut reached = Vec::new();
                let mut edged = false;
                let mut walked = Vec::new();
                sweep::walk_each(
                    &app,
                    cx,
                    key,
                    &format!("the inspected commit's {name} by {key}"),
                    18,
                    &mut walked,
                    &mut |cx, stop, rings| {
                        let (inside, text) = cx.update(|window, cx| {
                            let browser = browser.read(cx);
                            let inside = ["series-files", "series-file-preview"]
                                .into_iter()
                                .filter(|id| {
                                    browser.body.focus(*id, cx).contains_focused(window, cx)
                                })
                                .collect::<Vec<_>>();
                            let text = browser
                                .text_focus(cx)
                                .iter()
                                .any(|focus| focus.is_focused(window));
                            (inside, text)
                        });
                        reached.extend(inside.iter().copied());
                        // The editor, which draws no ring of its own, rings
                        // the preview's 1 px edge, which the walk finds whole.
                        if text {
                            edged = true;
                            let preview =
                                cx.debug_bounds("series-file-preview").expect("the preview");
                            let drawn: Vec<_> = rings.iter().map(|ring| ring.bounds).collect();
                            if !sweep::rings_edge(&drawn, preview) {
                                failures.push(format!(
                                    "{stop}: the editor draws no ring on the preview's edge, \
                                     {preview:?}, only {drawn:?}"
                                ));
                            }
                        }
                    },
                );
                failures.extend(walked);
                for id in ["series-files", "series-file-preview"] {
                    if !reached.contains(&id) {
                        failures.push(format!(
                            "{name} by {key} never focused a control inside {id}"
                        ));
                    }
                }
                if !edged {
                    failures.push(format!(
                        "{name} by {key} never focused the preview's editor"
                    ));
                }
            }
        }
        // Tab onto the source's editor reveals the preview; with the window
        // blurred, a click in the part of it the body shows focuses the
        // editor again, unmarked until a key.
        let editor_focused = |cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|window, cx| {
                browser
                    .read(cx)
                    .text_focus(cx)
                    .iter()
                    .any(|focus| focus.is_focused(window))
            })
        };
        for _ in 0..40 {
            if editor_focused(cx) {
                break;
            }
            cx.simulate_keystrokes("tab");
            draw(cx);
        }
        cx.update(|window, cx| window.blur(cx));
        draw(cx);
        let preview = cx.debug_bounds("series-file-preview").expect("the preview");
        let body = cx.debug_bounds("rewrite-review-body").expect("the body");
        let shown = preview.intersect(&body);
        let clicked = shown.size.height > px(20.);
        if clicked {
            cx.simulate_click(shown.center(), Modifiers::none());
            draw(cx);
        }
        let focused = clicked && editor_focused(cx);
        let preview = cx.debug_bounds("series-file-preview").expect("the preview");
        if !clicked {
            failures.push(format!(
                "Tab onto the source's editor leaves the preview, {preview:?}, outside the body, \
                 {body:?}, for a click"
            ));
        } else if !focused {
            failures.push(format!(
                "a click in the source leaves focus on {}, not its editor",
                focus_name(&browser, cx)
            ));
        } else if sweep::rings_edge(&outlines_in(cx, |p| p.accent), preview) {
            failures.push(format!(
                "a click in the source rings the preview's edge, {preview:?}, in the accent"
            ));
        } else if !sweep::rings_edge(&outlines_in(cx, |p| p.border), preview) {
            failures.push(format!(
                "after a click in the source the preview's edge, {preview:?}, is not the border's \
                 colour"
            ));
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Review messages with both commits shows two editors side by side, at
    /// 18 pt in a 461 × 490 window: Tab and Shift+Tab onto either ring that
    /// message's text below its heading, never the other's or the whole
    /// preview, as Split rings its focused side; one ring around the whole
    /// preview for either told nothing as Tab moved between them. A click in
    /// a message's text focuses its editor unmarked.
    #[gpui::test]
    async fn review_messages_ring_the_focused_messages_text(cx: &mut gpui_kit::TestAppContext) {
        use crate::focus_ring::sweep;
        use crate::tags::tests::draw;
        const TEXTS: [&str; 2] = ["series-message-0", "series-message-1"];
        let (_fixture, app, browser, cx) = inspected_commit(cx);
        // As Review messages opens them (`inspect_messages`).
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.clear_content();
                browser.back.focus(window, cx);
                browser.inspect_label = "Original and rewritten commit messages".into();
                browser.messages = Some(
                    [
                        "Add notes\n\nThe first notes.",
                        "Rewrite notes\n\nThe second notes.",
                    ]
                    .map(|message| Some(text::editor(message, "text", None, window, cx))),
                );
                cx.notify();
            })
        });
        draw(cx);
        let editors = |cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|window, cx| {
                let browser = browser.read(cx);
                let messages = browser.messages.as_ref().expect("both messages");
                [0, 1].map(|side| {
                    messages[side]
                        .as_ref()
                        .is_some_and(|editor| editor.read(cx).focus_handle(cx).is_focused(window))
                })
            })
        };
        let mut failures = Vec::new();
        for key in ["tab", "shift-tab"] {
            let mut reached = [false; 2];
            let mut walked = Vec::new();
            sweep::walk_each(
                &app,
                cx,
                key,
                &format!("the commit messages by {key}"),
                18,
                &mut walked,
                &mut |cx, stop, rings| {
                    let focused = editors(cx);
                    let drawn: Vec<_> = rings.iter().map(|ring| ring.bounds).collect();
                    for side in 0..2 {
                        let text = cx.debug_bounds(TEXTS[side]).expect("the message's text");
                        let ringed = sweep::rings_edge(&drawn, text);
                        if focused[side] {
                            reached[side] = true;
                            if !ringed {
                                failures.push(format!(
                                    "{stop}: message {side}'s editor draws no ring on its text's \
                                     edge, {text:?}, only {drawn:?}"
                                ));
                            }
                        } else if ringed {
                            failures.push(format!(
                                "{stop}: message {side}'s text rings without its editor's focus"
                            ));
                        }
                    }
                    let preview = cx.debug_bounds("series-file-preview").expect("the preview");
                    if sweep::rings_edge(&drawn, preview) {
                        failures.push(format!("{stop}: the whole preview rings, {preview:?}"));
                    }
                },
            );
            failures.extend(walked);
            for (side, reached) in reached.into_iter().enumerate() {
                if !reached {
                    failures.push(format!("{key} never focused message {side}'s editor"));
                }
            }
        }
        let text = cx
            .debug_bounds(TEXTS[1])
            .expect("the rewritten message's text");
        cx.simulate_click(text.center(), Modifiers::none());
        draw(cx);
        let text = cx
            .debug_bounds(TEXTS[1])
            .expect("the rewritten message's text");
        if !editors(cx)[1] {
            failures.push("a click in the rewritten message leaves its editor unfocused".into());
        } else if sweep::rings_edge(&outlines_in(cx, |p| p.accent), text) {
            failures.push(format!(
                "a click in the rewritten message rings its text, {text:?}, in the accent"
            ));
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// A press on the inspected commit's file list or preview beside their
    /// controls, as on the list's room below its rows or on the preview's
    /// "Select a file…" line, at 13 pt in a 1280 × 800 window, leaves focus
    /// where it was, and the next key shows its ring there: both track a
    /// handle no Tab stops on, which such a press focused, so the next key
    /// showed no ring anywhere.
    #[gpui::test]
    async fn a_press_beside_the_inspected_commits_controls_keeps_focus(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::tags::tests::{draw, set_interface_text_size};
        let (_fixture, app, browser, cx) = inspected_commit(cx);
        cx.simulate_resize(size(px(1280.), px(800.)));
        set_interface_text_size(cx, &app, 13);
        cx.update(|_, cx| browser.update(cx, |browser, _| browser.clear_content()));
        let back = cx.update(|window, cx| {
            let back = browser.read(cx).back.clone();
            back.focus(window, cx);
            app.read(cx).focus_ring.mark(window);
            back
        });
        draw(cx);
        let list = cx.debug_bounds("series-files").expect("the file list");
        let preview = cx.debug_bounds("series-file-preview").expect("the preview");
        let mut failures = Vec::new();
        for (place, at) in [
            (
                "the file list's room below its rows",
                point(list.center().x, list.bottom() - px(6.)),
            ),
            (
                "the preview's line",
                point(preview.left() + px(30.), preview.top() + px(20.)),
            ),
        ] {
            cx.simulate_click(at, Modifiers::none());
            draw(cx);
            cx.simulate_keystrokes("down");
            draw(cx);
            let (kept, ringed) = cx.update(|window, cx| {
                (
                    back.is_focused(window),
                    app.read(cx).focus_ring.shows(&back),
                )
            });
            if !kept || !ringed {
                failures.push(format!(
                    "a press on {place} at {at:?} and a key leave focus on {}, its ring shown: \
                     {ringed}",
                    focus_name(&browser, cx)
                ));
                // The next press starts from Back to series again.
                cx.update(|window, cx| back.focus(window, cx));
                draw(cx);
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// An inspected commit's document, in the series review's file box, which
    /// keeps 100 px, says why a click on Open in default app opened nothing on
    /// one whole line beneath its header's controls, inside the box and the
    /// body's view, its whole answer in the line's tooltip, and the page
    /// beneath gives way below its minimum for it, at 13 and 18 pt in a
    /// 461 × 490 window: the room beneath the header held no line above that
    /// minimum, so the line drew none and the click changed nothing on
    /// screen. With the text modes' row kept above every preview, the line
    /// lay 25.5 px below the view at 13 pt while the body stopped at the
    /// window less 250 px, short of the room the dialog's cap leaves it.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn an_inspected_document_says_why_on_one_whole_line(cx: &mut gpui_kit::TestAppContext) {
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let pdf = |text: &str| {
            format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n")
        };
        let document = repo.path().join("document.pdf");
        for (text, message) in [("first", "Add a document"), ("second", "Rewrite it")] {
            std::fs::write(&document, pdf(text)).unwrap();
            git(repo.path(), &["add", "document.pdf"]);
            git(repo.path(), &["commit", "--quiet", "-m", message]);
        }
        // A fresh repository keeps its branch as a loose reference.
        let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
        let files = repo.changes_with_renames(head.trim(), 0).unwrap();
        let (app, cx) = window(cx, &repo);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        for text in [13, 18] {
            let at = format!("the series review at {text} pt in 461 × 490");
            size_to(&app, cx, text, 461., 490.);
            let browser = cx.update(|window, cx| {
                let repo = app
                    .read(cx)
                    .repository
                    .clone()
                    .expect("the window's repository");
                let browser = cx.new(|cx| {
                    SeriesBrowser::new(
                        app.downgrade(),
                        repo,
                        "origin".into(),
                        "main".into(),
                        window,
                        cx,
                    )
                });
                open_series_dialog(browser.clone(), window, cx);
                browser
            });
            // The rewritten commit's files, as Inspect rewritten files lists
            // them, and its document chosen.
            cx.update(|window, cx| {
                browser.update(cx, |browser, cx| {
                    browser.inspecting = true;
                    browser.inspect_label = "Rewritten".into();
                    browser.files = files.clone();
                    browser.file(0, window, cx);
                })
            });
            for _ in 0..400 {
                cx.executor()
                    .advance_clock(std::time::Duration::from_millis(50));
                draw(cx);
                if cx.debug_bounds("system-preview-1").is_some() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            // Where the document's controls lie below the body's view, a
            // person scrolls the body to them before pressing one.
            let body = cx.debug_bounds("rewrite-review-body").expect("the body");
            let control = crate::tags::tests::rendered(cx, "system-preview-1");
            if !lies_in(control, body) {
                crate::tags::tests::scroll_down(
                    cx,
                    body.origin + point(px(4.), px(4.)),
                    gpui::ScrollDelta::Pixels(point(px(0.), -body.size.height)),
                );
            }
            crate::system_preview::tests::a_failed_open_answers_on_one_line(
                cx,
                &at,
                [
                    "system-preview-1",
                    "system-preview-1/note",
                    "rich-sides/body",
                ],
                &["series-file-preview", "rewrite-review-body"],
            );
            cx.update(|window, cx| window.close_dialog(cx));
            draw(cx);
        }
    }

    /// A click on an inspected document's Open in default app whose control
    /// a person scrolled to the body's foot, so only the controls' row of
    /// the preview shows, answers where it is seen: the line saying why
    /// nothing opened, which lands below the view, is revealed whole by the
    /// body's least scroll, with its ring's room, and focus stays on the
    /// control, at 18 pt in a 461 × 490 window, where the preview lies below
    /// the view as the inspector opens. The line's bounds reached only
    /// tests, and it answered out of sight.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn a_failed_open_at_the_bodys_foot_is_revealed(cx: &mut gpui_kit::TestAppContext) {
        use crate::system_preview::tests::{queue, seen_as_svg};
        use crate::tags::tests::{draw, git, rendered, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let pdf = |text: &str| {
            format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n")
        };
        let document = repo.path().join("document.pdf");
        for (text, message) in [("first", "Add a document"), ("second", "Rewrite it")] {
            std::fs::write(&document, pdf(text)).unwrap();
            git(repo.path(), &["add", "document.pdf"]);
            git(repo.path(), &["commit", "--quiet", "-m", message]);
        }
        // A fresh repository keeps its branch as a loose reference.
        let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
        let files = repo.changes_with_renames(head.trim(), 0).unwrap();
        let (app, cx) = window(cx, &repo);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        {
            let text = 18;
            let at = format!("the series review at {text} pt in 461 × 490");
            size_to(&app, cx, text, 461., 490.);
            let browser = cx.update(|window, cx| {
                let repo = app
                    .read(cx)
                    .repository
                    .clone()
                    .expect("the window's repository");
                let browser = cx.new(|cx| {
                    SeriesBrowser::new(
                        app.downgrade(),
                        repo,
                        "origin".into(),
                        "main".into(),
                        window,
                        cx,
                    )
                });
                open_series_dialog(browser.clone(), window, cx);
                browser
            });
            cx.update(|window, cx| {
                browser.update(cx, |browser, cx| {
                    browser.inspecting = true;
                    browser.inspect_label = "Rewritten".into();
                    browser.files = files.clone();
                    browser.file(0, window, cx);
                })
            });
            for _ in 0..400 {
                cx.executor()
                    .advance_clock(std::time::Duration::from_millis(50));
                draw(cx);
                if cx.debug_bounds("system-preview-1").is_some() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            // The body scrolled until the controls' row, with its ring's
            // room, ends at the view's foot.
            let room = cx.update(|_, cx| appearance::button_ring_room(cx));
            let scroll = browser.read_with(cx, |browser, _| browser.body.scroll().clone());
            let control = rendered(cx, "system-preview-1");
            let body = rendered(cx, "rewrite-review-body");
            let offset = scroll.offset().y - (control.bottom() + room - body.bottom());
            assert!(
                offset <= px(0.) && -offset <= scroll.max_offset().y,
                "{at}: the body can scroll the control {control:?} to its foot {body:?}"
            );
            scroll.set_offset(point(px(0.), offset));
            draw(cx);
            let (control, body) = (
                rendered(cx, "system-preview-1"),
                rendered(cx, "rewrite-review-body"),
            );
            assert!(
                (control.bottom() + room - body.bottom()).abs() < px(0.5),
                "{at}: the control {control:?} ends the body's view {body:?}"
            );
            queue(Err(seen_as_svg("a PDF document")));
            cx.simulate_click(control.center(), Modifiers::none());
            let pressed = cx.update(|window, cx| window.focused(cx));
            cx.run_until_parked();
            draw(cx);
            let line = rendered(cx, "system-preview-1/note");
            let body = rendered(cx, "rewrite-review-body");
            assert!(
                line.top() >= body.top() + room - px(0.5)
                    && line.bottom() + room <= body.bottom() + px(0.5),
                "{at}: the line {line:?} lies whole in the body's view {body:?}, with its \
                 ring's room"
            );
            let control = rendered(cx, "system-preview-1");
            assert!(
                lies_in(control, body),
                "{at}: the control {control:?} stays in the body's view {body:?}"
            );
            assert_eq!(
                cx.update(|window, cx| window.focused(cx)),
                pressed,
                "{at}: focus stays where the press put it"
            );
            cx.update(|window, cx| window.close_dialog(cx));
            draw(cx);
        }
    }

    /// A model the series review shows, an inspected commit's 3D file,
    /// focused from the keyboard keeps its canvas's accent ring while the
    /// pointer moves over it and away, at 13 pt in a 1,480 × 980 window, as
    /// Compare's does: the review's opener carries the window's focus ring
    /// (`Opener::ringed_by`), which the canvas asks.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn an_inspected_models_canvas_keeps_its_ring_while_the_pointer_moves(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let model = |scale: &str| format!("v 0 0 0\nv {scale} 0 0\nv 0 {scale} 0\nf 1 2 3\n");
        let shape = repo.path().join("shape.obj");
        for (scale, message) in [("1", "Add a model"), ("2", "Grow it")] {
            std::fs::write(&shape, model(scale)).unwrap();
            git(repo.path(), &["add", "shape.obj"]);
            git(repo.path(), &["commit", "--quiet", "-m", message]);
        }
        let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
        let files = repo.changes_with_renames(head.trim(), 0).unwrap();
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        let browser = cx.update(|window, cx| {
            let repo = app
                .read(cx)
                .repository
                .clone()
                .expect("the window's repository");
            let browser = cx.new(|cx| {
                SeriesBrowser::new(
                    app.downgrade(),
                    repo,
                    "origin".into(),
                    "main".into(),
                    window,
                    cx,
                )
            });
            open_series_dialog(browser.clone(), window, cx);
            browser
        });
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.inspecting = true;
                browser.inspect_label = "Rewritten".into();
                browser.files = files;
                browser.file(0, window, cx);
            })
        });
        for _ in 0..400 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(50));
            draw(cx);
            if cx.debug_bounds("model-canvas-1").is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        crate::focus_ring::sweep::prepare(&app, cx);
        crate::model_view::tests::assert_the_canvas_keeps_its_ring(
            cx,
            "model-canvas-1",
            "the series review",
        );
    }

    /// An inspected document's comparison takes its open controls' form
    /// from its box's width in the frame it is drawn ([`crate::in_frame`]):
    /// every frame drawn as the window narrows and widens between 461 and
    /// 1,481 px at 13 and 18 pt lays the comparison out as the frame it
    /// settles in, and the walk meets After's control labelled and as its
    /// icon. Built from the box's width a frame before, the first frame after
    /// a resize drew the form of the width before.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn an_inspected_comparison_takes_its_form_in_the_frame_it_is_drawn(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let pdf = |text: &str| {
            format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n")
        };
        let document = repo.path().join("document.pdf");
        for (text, message) in [("first", "Add a document"), ("second", "Rewrite it")] {
            std::fs::write(&document, pdf(text)).unwrap();
            git(repo.path(), &["add", "document.pdf"]);
            git(repo.path(), &["commit", "--quiet", "-m", message]);
        }
        let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
        let files = repo.changes_with_renames(head.trim(), 0).unwrap();
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1481.), px(980.)));
        draw(cx);
        let browser = cx.update(|window, cx| {
            let repo = app
                .read(cx)
                .repository
                .clone()
                .expect("the window's repository");
            let browser = cx.new(|cx| {
                SeriesBrowser::new(
                    app.downgrade(),
                    repo,
                    "origin".into(),
                    "main".into(),
                    window,
                    cx,
                )
            });
            open_series_dialog(browser.clone(), window, cx);
            browser
        });
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.inspecting = true;
                browser.inspect_label = "Rewritten".into();
                browser.files = files;
                browser.file(0, window, cx);
            })
        });
        for _ in 0..400 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(50));
            draw(cx);
            if cx.debug_bounds("system-preview-1").is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let met = crate::system_preview::tests::open_controls_take_their_form_in_the_frame(
            cx,
            "the series review",
            "rewrite-comparison",
        );
        assert_eq!(
            met.into_iter().collect::<Vec<_>>(),
            ["icon", "labelled"],
            "the walk meets both forms"
        );
    }

    /// A rewritten commit's 3D model and PDF, in the series review's file
    /// box, which keeps 100 px at the interface size, keep each side's own
    /// header at its floor, one control whole with the larger of its ring's
    /// room and the header's padding above and below, beneath which the
    /// model or page gives way, and Tab reveals each of the header's
    /// controls whole with its ring there, in a 461 × 490 window, at 13 and
    /// 18 pt. The floor once counted the ring's room and the padding
    /// together, about 6 px more than either needed. The box once kept an
    /// unscaled 100 px, as the error box beneath it did, which at 18 pt left
    /// the model's side 44 px, less than its header's floor and rule, so the
    /// header took the side and the box's clip cut its controls' rings.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn an_inspected_side_header_keeps_its_floor_in_the_file_box(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::focus_reveal::tests::control;
        use crate::system_preview::tests::{Shown, assert_a_header_keeps_its_floor};
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let pdf = |text: &str| {
            format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n")
        };
        let document = repo.path().join("document.pdf");
        let model = repo.path().join("shape.obj");
        for (text, scale, message) in [("first", "2", "Add them"), ("second", "1", "Rewrite them")]
        {
            std::fs::write(&document, pdf(text)).unwrap();
            std::fs::write(
                &model,
                format!("v 0 0 0\nv {scale} 0 0\nv 0 {scale} 0\nf 1 2 3\n"),
            )
            .unwrap();
            git(repo.path(), &["add", "document.pdf", "shape.obj"]);
            git(repo.path(), &["commit", "--quiet", "-m", message]);
        }
        // A fresh repository keeps its branch as a loose reference.
        let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
        let files = repo.changes_with_renames(head.trim(), 0).unwrap();
        let index = |path: &str| {
            files
                .iter()
                .position(|file| file.path() == std::path::Path::new(path))
                .expect("the commit changes it")
        };
        let (document, model) = (index("document.pdf"), index("shape.obj"));
        let (app, cx) = window(cx, &repo);
        cx.update(|_, cx| appearance::ThemeChoice::Midnight.apply(None, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        let browser = cx.update(|window, cx| {
            let repo = app
                .read(cx)
                .repository
                .clone()
                .expect("the window's repository");
            let browser = cx.new(|cx| {
                SeriesBrowser::new(
                    app.downgrade(),
                    repo,
                    "origin".into(),
                    "main".into(),
                    window,
                    cx,
                )
            });
            open_series_dialog(browser.clone(), window, cx);
            browser
        });
        cx.update(|_, cx| {
            browser.update(cx, |browser, _| {
                browser.inspecting = true;
                browser.inspect_label = "Rewritten".into();
                browser.files = files;
            })
        });
        let open = |cx: &mut gpui_kit::VisualTestContext, index: usize, selector: &'static str| {
            cx.update(|window, cx| {
                browser.update(cx, |browser, cx| browser.file(index, window, cx))
            });
            for _ in 0..400 {
                cx.executor()
                    .advance_clock(std::time::Duration::from_millis(50));
                draw(cx);
                if cx.debug_bounds(selector).is_some() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("{selector} never drawn");
        };
        let view = browser.entity_id();
        let mut cameras = vec![control("model-external", 1)];
        cameras.extend((5..10).map(|index| control("model-camera", index)));
        cameras.extend([control("model-views", 1), control("model-link", 1)]);
        open(cx, model, "model-side-1/header");
        for (text, floor) in [(13, 34.), (18, 48.)] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            draw(cx);
            assert_a_header_keeps_its_floor(
                cx,
                &format!("the model's After at {text} pt"),
                (view, "model-side-1"),
                (px(floor), Shown::Floor),
                &control("model-source", 1),
                &cameras,
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        draw(cx);
        open(cx, document, "system-preview-1");
        browser.update(cx, |browser, cx| {
            let Some(Content::Rich(preview)) = browser.content.as_deref() else {
                panic!("a changed PDF is a document comparison");
            };
            browser.content = Some(Arc::new(crate::pdf_view::tests::paged(preview)));
            cx.notify();
        });
        draw(cx);
        crate::pdf_view::tests::drain_pages();
        draw(cx);
        // At 13 pt the narrowest form's run takes a line beneath its
        // heading; at 18 pt it wraps to a second, the "more" menu on its own.
        for (text, floor, shown) in [(13, 41., Shown::FloorOneLine), (18, 57., Shown::Floor)] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            draw(cx);
            for side in [0, 1] {
                // The narrowest header a narrow box's side takes, its run on
                // a line beneath its heading, which scrolls at its floor,
                // from the page number (Previous is unavailable on the first
                // page).
                let controls = ["pdf-forward", "pdf-more"]
                    .map(|name| control(name, side))
                    .to_vec();
                let name = if side == 0 {
                    "pdf-side-0"
                } else {
                    "pdf-side-1"
                };
                assert_eq!(
                    crate::pdf_view::DRAWN_FORMS.with_borrow(|drawn| drawn.get(&side).copied()),
                    Some(crate::pdf_view::HeaderForm::Narrowest),
                    "{name} at {text} pt"
                );
                assert_a_header_keeps_its_floor(
                    cx,
                    &format!("{name} at {text} pt"),
                    (view, name),
                    (px(floor), shown),
                    &control("pdf-page-number", side),
                    &controls,
                );
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The rewritten series' review, opened with the app's series keys on a
    /// disposable repository of three commits whose last two a rebase's
    /// start captured, as it captures them, before `rewrite` (Git argument
    /// lists, run in order) rewrote them, its series read.
    async fn rewritten_review<'a>(
        cx: &'a mut gpui_kit::TestAppContext,
        rewrite: &[&[&str]],
    ) -> (
        tempfile::TempDir,
        Entity<SeriesBrowser>,
        &'a mut gpui::VisualTestContext,
    ) {
        use crate::focus_ring::sweep::Opened as _;
        use crate::tags::tests::{draw, git};
        use gitturtle_core::InteractiveRebaseCommand;
        let (fixture, app, cx) =
            crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 2))
                .await;
        // The review opens at rest, so its geometry is read where it stays.
        cx.update(|_, cx| {
            crate::interactive_rebase::init(cx);
            cx.set_reduce_motion(true);
        });
        let repo = app.read_with(cx, |app, _| app.repository.clone().expect("a repository"));
        git(repo.path(), &["reset", "--quiet", "--hard", "main"]);
        let plan = repo.interactive_rebase_plan("main~2").unwrap();
        let steps = plan.steps();
        let command = WriteCommand::InteractiveRebase(Arc::new(InteractiveRebaseCommand::Start {
            plan,
            steps,
            acknowledge_published: false,
        }));
        let mut saved = app
            .update(cx, |app, _| app.prepare_rewrite_capture(&command))
            .expect("a rebase start captures its series");
        app.read_with(cx, |app, _| app.preferences_writer.drain());
        assert!(
            matches!(saved.try_recv(), Ok(Some(Ok(())))),
            "the series is captured"
        );
        for args in rewrite {
            git(repo.path(), args);
        }
        let review =
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_series_review(window, cx)));
        review.clone().finish(cx);
        let review = review.expect("the review opens");
        draw(cx);
        (fixture, review, cx)
    }

    /// A key-down of Enter, not marked held, with no key-up after it: a
    /// fresh press, or under X11 each repeat of a held one; with `down`
    /// false, its key-up.
    fn enter(cx: &mut gpui::VisualTestContext, down: bool) {
        let keystroke = gpui::Keystroke::parse("enter").unwrap();
        if down {
            cx.simulate_event(gpui::KeyDownEvent {
                keystroke,
                is_held: false,
                prefer_character_input: false,
            });
        } else {
            cx.simulate_event(gpui::KeyUpEvent { keystroke });
        }
        crate::tags::tests::draw(cx);
    }

    /// Return on a commit row of a rewritten series inspects that commit's
    /// files, as Return on a History commit loads its files, and the review
    /// stays open: the review dialog took the key for its default action,
    /// which closed the review before the series list's own Return was
    /// reached. Back to series takes the keys from the list the inspector
    /// replaces, so Escape goes back to the series, keeping the row, and
    /// closes the review from there: left on the list, which is no longer
    /// drawn, focus sent Escape to the window's root, where the review never
    /// saw it. Driven by the route a person takes: a
    /// series captured before its rewrite, the review opened on it, the row
    /// chosen with Down and Return and Escape dispatched as keys.
    #[gpui::test]
    async fn return_on_a_series_row_inspects_its_files_and_escape_closes_the_review(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::tags::tests::draw;
        // The newest commit reworded.
        let (_fixture, review, cx) = rewritten_review(
            cx,
            &[&[
                "commit",
                "--quiet",
                "--amend",
                "-m",
                "fixture notes 2, reworded",
            ]],
        )
        .await;
        let rows = review.read_with(cx, |review, _| {
            review.review.as_ref().map_or(0, |review| review.rows.len())
        });
        assert_eq!(rows, 2, "the rewritten series' two commits");
        assert!(
            cx.update(|window, cx| review.read(cx).focus.is_focused(window)),
            "the loaded series takes the keys"
        );

        cx.simulate_keystrokes("down");
        draw(cx);
        assert_eq!(review.read_with(cx, |review, _| review.selected), 1);
        cx.simulate_keystrokes("enter");
        review.read_with(cx, |review, _| review.worker.wait_idle());
        draw(cx);
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "Return leaves the review open"
        );
        let (inspecting, label, files) = review.read_with(cx, |review, _| {
            (
                review.inspecting,
                review.inspect_label.clone(),
                review
                    .files
                    .iter()
                    .map(|file| file.path().to_string_lossy().into_owned())
                    .collect::<Vec<_>>(),
            )
        });
        assert!(inspecting, "Return inspects the selected commit");
        assert!(
            cx.update(|window, cx| review.read(cx).back.is_focused(window)),
            "Back to series takes the keys from the series list it replaces"
        );
        assert!(
            label.starts_with("Rewritten ") && label.ends_with("· fixture notes 2, reworded"),
            "the selected row's rewritten commit: {label:?}"
        );
        assert_eq!(files, ["notes.txt"], "its changed files are listed");
        assert!(
            cx.debug_bounds("series-files").is_some(),
            "the inspector draws its file list"
        );

        cx.simulate_keystrokes("escape");
        draw(cx);
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "Escape in the inspector goes back to the series"
        );
        assert_eq!(
            review.read_with(cx, |review, _| (review.inspecting, review.selected)),
            (false, 1),
            "the series keeps its row"
        );
        cx.simulate_keystrokes("escape");
        draw(cx);
        assert!(
            !cx.update(|window, cx| window.has_active_dialog(cx)),
            "Escape on the series closes the review"
        );
    }

    /// A held Return on a series row inspects once. Under X11 a held key's
    /// repeats arrive as fresh presses, not marked held: the first inspected
    /// and moved focus to Back to series, the next pressed it, which focused
    /// the list again, the next inspected again, so the review toggled and
    /// started and cancelled a changed-files read with every repeat. Back to
    /// series acts on a press of its own, once the held one comes up.
    #[gpui::test]
    async fn a_held_return_on_a_series_row_inspects_once(cx: &mut gpui_kit::TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, review, cx) = rewritten_review(
            cx,
            &[&[
                "commit",
                "--quiet",
                "--amend",
                "-m",
                "fixture notes 2, reworded",
            ]],
        )
        .await;
        enter(cx, true);
        let generation = review.read_with(cx, |review, _| review.generation);
        assert!(
            review.read_with(cx, |review, _| review.inspecting),
            "the press inspects"
        );
        for repeat in 1..=4 {
            enter(cx, true);
            assert_eq!(
                review.read_with(cx, |review, _| (review.inspecting, review.generation)),
                (true, generation),
                "repeat {repeat} of the held Return leaves the inspection as it is"
            );
        }
        enter(cx, false);
        review.read_with(cx, |review, _| review.worker.wait_idle());
        draw(cx);
        assert_eq!(
            review.read_with(cx, |review, _| review.files.len()),
            1,
            "the one read lands"
        );
        enter(cx, true);
        assert!(
            !review.read_with(cx, |review, _| review.inspecting),
            "a press of its own on Back to series goes back to the series"
        );
        enter(cx, true);
        assert!(
            !review.read_with(cx, |review, _| review.inspecting),
            "and its repeats on the series list inspect nothing"
        );
    }

    /// Return on a row whose commit the rewrite dropped, which has only its
    /// original side, inspects that original commit's files: it inspected
    /// only a rewritten side, so Return there did nothing.
    #[gpui::test]
    async fn return_on_a_dropped_commits_row_inspects_its_original(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::tags::tests::draw;
        // The newest commit dropped.
        let (_fixture, review, cx) =
            rewritten_review(cx, &[&["reset", "--quiet", "--hard", "main~1"]]).await;
        let dropped = review.read_with(cx, |review, _| {
            review.review.as_ref().and_then(|review| {
                review
                    .rows
                    .iter()
                    .position(|row| row.rewritten.is_none() && row.original.is_some())
            })
        });
        let dropped = dropped.expect("a row with only its original side");
        for _ in 0..dropped {
            cx.simulate_keystrokes("down");
        }
        draw(cx);
        assert_eq!(review.read_with(cx, |review, _| review.selected), dropped);
        cx.simulate_keystrokes("enter");
        review.read_with(cx, |review, _| review.worker.wait_idle());
        draw(cx);
        let (inspecting, label, files) = review.read_with(cx, |review, _| {
            (
                review.inspecting,
                review.inspect_label.clone(),
                review
                    .files
                    .iter()
                    .map(|file| file.path().to_string_lossy().into_owned())
                    .collect::<Vec<_>>(),
            )
        });
        assert!(inspecting, "Return inspects the dropped commit");
        assert!(
            label.starts_with("Original ") && label.ends_with("· fixture notes 2"),
            "its original side: {label:?}"
        );
        assert_eq!(files, ["notes.txt"], "its changed files are listed");
    }

    /// A `git fast-import` stream on `main`: a base commit and three after
    /// it, each adding a file of its own, so a rebase that drops the second
    /// replays the third without a conflict. The third adds `large.txt`,
    /// `large` bytes of numbered lines, where `large` is not zero.
    fn series_script(large: usize) -> String {
        let mut input = String::new();
        for commit in 0..4 {
            let (path, content) = if commit == 3 && large > 0 {
                let mut content = String::new();
                let mut line = 0;
                while content.len() < large {
                    content.push_str(&format!("line {line}\n"));
                    line += 1;
                }
                ("large.txt".to_owned(), content)
            } else {
                (format!("file-{commit}.txt"), format!("file {commit}\n"))
            };
            let message = format!("series commit {commit}");
            input.push_str(&format!(
                "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> {} +0000\ndata {}\n{message}\nM 100644 inline {path}\ndata {}\n{content}\n",
                1_700_000_000 + commit,
                message.len(),
                content.len(),
            ));
        }
        input + "done\n"
    }

    /// The app on [`series_script`]'s repository, checked out, with dialogs
    /// opening at rest.
    async fn series_window(
        cx: &mut gpui_kit::TestAppContext,
        large: usize,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        &mut gpui::VisualTestContext,
    ) {
        let (fixture, app, cx) =
            crate::views::tests::history_window_from(cx, series_script(large)).await;
        cx.update(|_, cx| {
            crate::interactive_rebase::init(cx);
            cx.set_reduce_motion(true);
        });
        let repo = app.read_with(cx, |app, _| app.repository.clone().expect("a repository"));
        crate::tags::tests::git(repo.path(), &["reset", "--quiet", "--hard", "main"]);
        crate::views::tests::settle(&app, cx).await;
        (fixture, app, cx)
    }

    /// `text` pt in a `width` × `height` window.
    fn size_to(
        app: &Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
        text: u8,
        width: f32,
        height: f32,
    ) {
        cx.simulate_resize(size(px(width), px(height)));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.interface_text_size = text;
                appearance::apply_text_sizes(text, app.settings.code_text_size, window, cx);
                cx.notify();
            })
        });
        crate::tags::tests::draw(cx);
    }

    /// Runs a rebase through Edit local commits as a person does: the form
    /// opened on `base`, Load commits pressed, the second commit dropped
    /// with Down and D, Review rebase… pressed and the alert's Start rebase
    /// pressed; returns once the rebase and the series' capture before it
    /// have landed.
    async fn rebase_dropping_the_second(
        app: &Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
        base: &str,
    ) {
        use crate::tags::tests::draw;
        let head = |cx: &mut gpui::VisualTestContext| {
            let repo = app.read_with(cx, |app, _| app.repository.clone().expect("a repository"));
            let output = std::process::Command::new("git")
                .args(["rev-parse", "main"])
                .current_dir(repo.path())
                .output()
                .unwrap();
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        };
        let before = head(cx);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_interactive_rebase(Some(base.into()), window, cx)
            })
        });
        draw(cx);
        let load = cx
            .debug_bounds("load-rebase-commits")
            .expect("Load commits");
        cx.simulate_click(load.center(), Modifiers::none());
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(app, cx).await;
        cx.simulate_keystrokes("down d");
        draw(cx);
        let review = cx
            .debug_bounds("review-interactive-rebase")
            .expect("Review rebase…");
        cx.simulate_click(review.center(), Modifiers::none());
        draw(cx);
        let start = cx
            .debug_bounds("dialog-ok: Start rebase")
            .expect("the alert's Start rebase");
        cx.simulate_click(start.center(), Modifiers::none());
        // The operation waits for the capture, saved on the preferences
        // thread, before Git starts.
        for _ in 0..4 {
            app.read_with(cx, |app, _| {
                app.preferences_writer.drain();
                app.operations.drain();
            });
            crate::views::tests::settle(app, cx).await;
        }
        assert!(
            app.read_with(cx, |app, _| app.operation_busy.is_none()),
            "the rebase has ended"
        );
        assert_ne!(head(cx), before, "the rebase rewrote main");
    }

    /// The review the palette's Review rewritten branch series… opens, its
    /// read landed.
    fn review_from_palette(
        app: &Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
    ) -> Entity<SeriesBrowser> {
        use crate::tags::tests::draw;
        let _ = take_opened();
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_command_palette(window, cx)));
        draw(cx);
        cx.simulate_input("Review rewritten branch series");
        draw(cx);
        // Its Return goes down and comes up, as a person's does: the
        // review it opens takes no Return until that key-up.
        enter(cx, true);
        enter(cx, false);
        let review = take_opened().expect("the palette opens the review");
        lands(&review, cx);
        review
    }

    /// Lands the review's read.
    fn lands(review: &Entity<SeriesBrowser>, cx: &mut gpui::VisualTestContext) {
        use crate::focus_ring::sweep::Opened as _;
        Some(review.clone()).finish(cx);
        crate::tags::tests::draw(cx);
    }

    /// Whether `inner` lies whole inside `outer`, to a quarter pixel.
    fn lies_in(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
        let slack = px(0.25);
        inner.left() >= outer.left() - slack
            && inner.top() >= outer.top() - slack
            && inner.right() <= outer.right() + slack
            && inner.bottom() <= outer.bottom() + slack
    }

    /// What the review says with no series to show: its line, its title
    /// where it has one, and whether Edit local commits… stands beside it.
    struct Says<'a> {
        line: &'a str,
        title: Option<&'a str>,
        edit: bool,
    }

    impl Says<'_> {
        /// The whole state, as its accessible description reads it.
        fn description(&self) -> String {
            self.title.map_or_else(
                || self.line.to_owned(),
                |title| format!("{title}. {}", self.line),
            )
        }
    }

    /// No rebase captured a series.
    const UNCAPTURED: Says<'static> = Says {
        line: recovery_drafts::NO_CAPTURE,
        title: None,
        edit: true,
    };

    /// The review, with no series to show, says why in the list's place,
    /// as `says` gives it: drawn as a designed empty state whole in the
    /// list and in view once Tab moves onto the list, a failure's title
    /// above its line, and read whole as the list's accessible description
    /// and each disabled action's; Edit local commits… stands below the
    /// line where no rebase captured a series, and Refresh series is
    /// offered, Tab moving onto each in turn from the list. Returns with
    /// Refresh series focused.
    fn says_why(
        review: &Entity<SeriesBrowser>,
        cx: &mut gpui::VisualTestContext,
        says: &Says,
        at: &str,
    ) {
        let reason = says.line;
        let described = says.description();
        let described = described.as_str();
        use crate::native_accessibility::written;
        use crate::tags::tests::draw;
        assert!(
            review.read_with(cx, |review, _| review
                .review
                .as_ref()
                .is_none_or(|review| review.rows.is_empty())),
            "{at}: no series to show"
        );
        // From the dialog's shell as it opens, Tab moves onto the list.
        if !cx.update(|window, cx| review.read(cx).focus.is_focused(window)) {
            cx.simulate_keystrokes("tab");
            draw(cx);
        }
        assert!(
            cx.update(|window, cx| review.read(cx).focus.is_focused(window)),
            "{at}: Tab moves onto the list"
        );
        assert_eq!(
            crate::empty_state::shown("series-empty").as_deref(),
            Some(reason),
            "{at}: the list's place says why"
        );
        let line = cx
            .debug_bounds("series-empty-line")
            .unwrap_or_else(|| panic!("{at}: the reason is drawn"));
        let list = cx
            .debug_bounds("series-focus-list")
            .unwrap_or_else(|| panic!("{at}: the list is drawn"));
        let body = cx
            .debug_bounds("rewrite-review-body")
            .unwrap_or_else(|| panic!("{at}: the body is drawn"));
        assert!(line.size.height > px(0.), "{at}: the reason has a line");
        assert!(
            lies_in(line, list),
            "{at}: the reason at {line:?} lies whole in the list, {list:?}"
        );
        assert!(
            lies_in(list, body),
            "{at}: the focused list at {list:?} lies whole in the body's view, {body:?}"
        );
        let title = cx.debug_bounds("series-empty-title");
        match says.title {
            Some(_) => {
                let title = title.unwrap_or_else(|| panic!("{at}: the failure's title is drawn"));
                assert!(
                    lies_in(title, list) && title.bottom() <= line.top(),
                    "{at}: the title at {title:?} lies in the list, {list:?}, above its line, \
                     {line:?}"
                );
                // Drawn as the app's title-and-explanation panes draw theirs
                // (`main::empty`): one 14 px line, its fixed `gap_2` above
                // its line.
                let (title_line, gap) = cx.update(|window, _| {
                    let text = appearance::ui_text(crate::empty_state::TITLE_TEXT);
                    (
                        window
                            .text_style()
                            .line_height
                            .to_pixels(text.into(), window.rem_size()),
                        window.rem_size() * 0.5,
                    )
                });
                let near = |a: Pixels, b: Pixels| (a - b).abs() < px(0.5);
                assert!(
                    near(title.size.height, title_line),
                    "{at}: the title, {title:?}, is one 14 px line, {title_line:?} tall"
                );
                assert!(
                    near(line.top() - title.bottom(), gap),
                    "{at}: the title, {title:?}, stands {gap:?} above its line, {line:?}"
                );
            }
            None => assert!(title.is_none(), "{at}: the state has no title"),
        }
        let edit = cx.debug_bounds("series-edit-commits");
        assert_eq!(
            edit.is_some_and(|edit| lies_in(edit, list) && edit.top() >= line.bottom()),
            says.edit,
            "{at}: Edit local commits… stands below the line where no rebase captured a series: \
             {edit:?}"
        );
        let list_node = written::get("series-focus-list")
            .unwrap_or_else(|| panic!("{at}: the list's node is written"))
            .node;
        assert_eq!(
            list_node.description(),
            Some(described),
            "{at}: the list's accessible description is the reason"
        );
        for id in [
            "inspect-series-messages",
            "inspect-original-commit",
            "inspect-rewritten-commit",
            "inspect-publication-remote",
        ] {
            let node = written::get(id)
                .unwrap_or_else(|| panic!("{at}: {id}'s node is written"))
                .node;
            assert!(node.is_disabled(), "{at}: {id} waits for a series");
            assert_eq!(
                node.description(),
                Some(described),
                "{at}: {id} says why it is unavailable"
            );
        }
        if says.edit {
            let node = written::get("series-edit-commits")
                .unwrap_or_else(|| panic!("{at}: Edit local commits…' node is written"))
                .node;
            assert!(!node.is_disabled(), "{at}: Edit local commits… is offered");
            cx.simulate_keystrokes("tab");
            draw(cx);
            let edit_focus =
                cx.update(|_, cx| review.read(cx).body.focus("series-edit-commits", cx));
            assert!(
                cx.update(|window, _| edit_focus.is_focused(window)),
                "{at}: Tab moves from the list onto Edit local commits…"
            );
            // Its ring whole inside every clip around it, and in the body's
            // view.
            crate::tags::tests::assert_rings_whole(cx, &["series-edit-commits"]);
            let edit = cx
                .debug_bounds("series-edit-commits")
                .unwrap_or_else(|| panic!("{at}: Edit local commits… is drawn"));
            assert!(
                lies_in(edit, body),
                "{at}: Edit local commits… at {edit:?} lies in the body's view, {body:?}"
            );
        }
        let reload = written::get("reload-series")
            .unwrap_or_else(|| panic!("{at}: Refresh series' node is written"))
            .node;
        assert!(!reload.is_disabled(), "{at}: Refresh series is offered");
        assert_eq!(
            reload.description(),
            None,
            "{at}: Refresh series is offered"
        );
        cx.simulate_keystrokes("tab");
        draw(cx);
        assert!(
            cx.update(|window, cx| {
                let review = review.read(cx);
                review.body.focus("reload-series", cx).is_focused(window)
            }),
            "{at}: Tab moves from the list onto Refresh series"
        );
    }

    /// Presses Refresh series with Return, as [`says_why`] leaves it
    /// focused, and lands the read it starts, which the review's thread
    /// holds for a frame: while it reads, and once it has read, Refresh
    /// series stays where it was, the state keeping its height. It keeps
    /// focus, and Shift+Tab, past the Buttons that wait for a series and
    /// Edit local commits… where `says` offers it, moves back onto the list.
    fn refresh(
        review: &Entity<SeriesBrowser>,
        cx: &mut gpui::VisualTestContext,
        says: &Says,
        at: &str,
    ) {
        let read = review.read_with(cx, |review, _| review.generation);
        let place = |cx: &mut gpui::VisualTestContext| {
            cx.debug_bounds("reload-series")
                .unwrap_or_else(|| panic!("{at}: Refresh series is drawn"))
                .top()
        };
        let before = place(cx);
        let state = |cx: &mut gpui::VisualTestContext| {
            cx.debug_bounds("series-focus-list")
                .unwrap_or_else(|| panic!("{at}: the list is drawn"))
                .size
                .height
        };
        let kept = state(cx);
        let (release, held) = std::sync::mpsc::channel::<()>();
        review.read_with(cx, |review, _| {
            drop(review.metadata.submit(move || {
                let _ = held.recv();
                Ok(())
            }))
        });
        enter(cx, true);
        enter(cx, false);
        assert!(
            review.read_with(cx, |review, _| review.pending),
            "{at}: Return on Refresh series reads the capture again"
        );
        assert_eq!(
            crate::empty_state::shown("series-empty").as_deref(),
            Some(READING),
            "{at}: the list's place says the read is under way"
        );
        assert_eq!(
            state(cx),
            kept,
            "{at}: the state keeps the height it took at rest while it reads"
        );
        assert_eq!(
            place(cx),
            before,
            "{at}: Refresh series stays put while it reads"
        );
        drop(release);
        lands(review, cx);
        assert_ne!(
            review.read_with(cx, |review, _| review.generation),
            read,
            "{at}: Refresh series reads the capture again"
        );
        assert_eq!(
            place(cx),
            before,
            "{at}: Refresh series stays put once read"
        );
        cx.simulate_keystrokes(if says.edit {
            "shift-tab shift-tab"
        } else {
            "shift-tab"
        });
        crate::tags::tests::draw(cx);
    }

    /// The palette's Review rewritten branch series… in a repository with
    /// no captured rebase says so in the list's place, at 13 pt in a 1,480 ×
    /// 973 window and at 18 pt in a 461 × 490 half tile; QA-106e saw an
    /// empty list, disabled Buttons and no text: the load's error was drawn
    /// at the body's end in a scrolling box the body shrank to nothing.
    /// Refresh series, pressed with Return, reads again and says so again,
    /// moving nothing. Edit local commits…, offered beside the line, is
    /// refused with the menu's reason while a write runs, and otherwise
    /// opens the rebase form in the review's place, as the menu's item does.
    #[gpui::test]
    async fn the_palettes_review_says_why_it_has_no_series(cx: &mut gpui_kit::TestAppContext) {
        use crate::native_accessibility::written;
        use crate::tags::tests::draw;
        let (_fixture, app, cx) = series_window(cx, 0).await;
        // The app's focus ring, which `says_why` finds whole.
        crate::focus_ring::sweep::prepare(&app, cx);
        for (text, width, height) in [(13, 1480., 973.), (18, 461., 490.)] {
            let at = format!("{text} pt in {width} × {height}");
            size_to(&app, cx, text, width, height);
            let review = review_from_palette(&app, cx);
            says_why(&review, cx, &UNCAPTURED, &at);
            refresh(&review, cx, &UNCAPTURED, &at);
            says_why(&review, cx, &UNCAPTURED, &format!("{at}, refreshed"));
            cx.simulate_keystrokes("escape");
            draw(cx);
            assert!(
                !cx.update(|window, cx| window.has_active_dialog(cx)),
                "{at}: Escape closes the review"
            );
        }

        let _review = review_from_palette(&app, cx);
        app.update(cx, |app, _| app.operation_busy = Some("Fetching…"));
        draw(cx);
        let busy = app.read_with(cx, |app, _| app.busy_reason());
        let node = written::get("series-edit-commits")
            .expect("Edit local commits…' node is written")
            .node;
        assert!(node.is_disabled(), "a write refuses Edit local commits…");
        assert_eq!(
            node.description(),
            Some(busy),
            "Edit local commits… says why with the menu's reason"
        );
        app.update(cx, |app, _| app.operation_busy = None);
        draw(cx);
        // Tab moves onto the list, then onto the action, revealing it.
        cx.simulate_keystrokes("tab tab");
        draw(cx);
        enter(cx, true);
        enter(cx, false);
        assert!(
            cx.debug_bounds("load-rebase-commits").is_some()
                && cx.debug_bounds("series-focus-list").is_none(),
            "Edit local commits… opens the rebase form in the review's place"
        );
    }

    /// The palette's review opens with its first read holding the room of
    /// the state it most likely lands in, no rebase captured, with Edit
    /// local commits… below its line, at 18 pt in a 461 × 490 half tile:
    /// where it lands there, the actions below the list stand where they
    /// settle in every frame painted from open to settled (kept from nothing,
    /// the reading line took one line, and the actions dropped as the state
    /// landed); where it lands on the load's failure, a series past the
    /// review's bound, they move once, as it lands.
    #[gpui::test]
    async fn the_actions_stand_still_from_open_to_settled(cx: &mut gpui_kit::TestAppContext) {
        // Where each painted frame from open to settled put the actions, and
        // where they settled; the review's read has landed.
        fn opened(
            app: &Entity<GitTurtle>,
            cx: &mut gpui::VisualTestContext,
        ) -> (Entity<SeriesBrowser>, Vec<(Pixels, bool)>, Pixels) {
            size_to(app, cx, 18, 461., 490.);
            painted_actions::watch();
            let review = review_from_palette(app, cx);
            crate::tags::tests::draw(cx);
            let painted = painted_actions::take();
            assert!(
                review.read_with(cx, |review, _| !review.pending),
                "the read lands"
            );
            assert!(
                painted.iter().any(|(_, pending)| *pending),
                "a frame drew the read under way: {painted:?}"
            );
            let settled = cx
                .debug_bounds("inspect-series-messages")
                .expect("Review messages is drawn")
                .top();
            (review, painted, settled)
        }
        let moved = |a: Pixels, b: Pixels| (a - b).abs() >= px(0.5);
        let moves = |painted: &[(Pixels, bool)]| {
            painted
                .windows(2)
                .filter(|pair| moved(pair[0].0, pair[1].0))
                .count()
        };
        {
            let (_fixture, app, cx) = series_window(cx, 0).await;
            let (review, painted, settled) = opened(&app, cx);
            assert!(
                review.read_with(cx, |review, _| review.review.is_none()
                    && review.error.is_none()),
                "no rebase captured a series"
            );
            assert_eq!(
                crate::empty_state::shown("series-empty").as_deref(),
                Some(recovery_drafts::NO_CAPTURE),
                "the likely state lands"
            );
            assert!(
                painted.iter().all(|(top, _)| !moved(*top, settled)),
                "every frame from open to settled puts the actions at {settled:?}: {painted:?}"
            );
        }
        // A listed series moves them twice: once as it lands, and by the
        // ring's room a frame later, as the body reveals the list its
        // landing focuses (queued).
        for (outcome, large) in [("the load's failure", 4_500_000)] {
            let (_fixture, app, cx) = series_window(cx, large).await;
            size_to(&app, cx, 13, 1480., 973.);
            rebase_dropping_the_second(&app, cx, "main~3").await;
            let (review, painted, settled) = opened(&app, cx);
            let (failed, listed) = review.read_with(cx, |review, _| {
                (
                    review.error.is_some(),
                    review
                        .review
                        .as_ref()
                        .is_some_and(|review| !review.rows.is_empty()),
                )
            });
            assert_eq!(
                (failed, listed),
                (large > 0, large == 0),
                "the read lands on {outcome}"
            );
            assert!(
                moves(&painted) <= 1,
                "{outcome}: the actions move at most once from open to settled: {painted:?}"
            );
            assert!(
                painted.last().is_some_and(|(top, _)| !moved(*top, settled)),
                "{outcome}: the last frame puts them where they settle, {settled:?}: {painted:?}"
            );
        }
    }

    /// While an inspected commit's changed files are read, its file box says
    /// so, and while a chosen file's preview is read, the preview's box does,
    /// each line inside the box being read and in the body's view, at 13 pt
    /// in a 1,480 × 973 window; at 18 pt in a 461 × 490 half tile, where the
    /// preview lies below the view, the chosen row says it, in its accessible
    /// description too. The file box and the preview keep their place and
    /// size as each read comes and goes, and the body's scroll never moves: a
    /// choice that scrolled ~170 px slid the file list out from under the
    /// pointer. Said in a line after the preview, the read lay below the view
    /// in the half tile and took a line's height from the preview at each
    /// read at 13 pt.
    #[gpui::test]
    async fn the_inspector_says_each_read_in_the_box_it_fills(cx: &mut gpui_kit::TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, review, cx) = rewritten_review(
            cx,
            &[&[
                "commit",
                "--quiet",
                "--amend",
                "-m",
                "fixture notes 2, reworded",
            ]],
        )
        .await;
        let app = cx.update(|_, cx| review.read(cx).owner.upgrade().expect("the app"));
        // Asked and drawn in one frame, before the worker's reply lands.
        let frame = |cx: &mut gpui::VisualTestContext| {
            cx.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            })
        };
        let boxes = |cx: &mut gpui::VisualTestContext| {
            ["series-files", "series-file-preview"].map(|id| {
                cx.debug_bounds(id)
                    .unwrap_or_else(|| panic!("{id} is drawn"))
            })
        };
        // The body's scroll, which no choice moves.
        let offset = |cx: &mut gpui::VisualTestContext| {
            review.read_with(cx, |review, _| review.body.scroll().offset())
        };
        // The fills the last frame painted in exactly the bounds of `id`.
        let fills = |cx: &mut gpui::VisualTestContext, id: &'static str| {
            let bounds = cx
                .debug_bounds(id)
                .unwrap_or_else(|| panic!("{id} is drawn"));
            cx.update(|window, _| {
                let scale = window.scale_factor();
                let near = |device: f32, logical: Pixels| {
                    (device / scale - f32::from(logical)).abs() < 0.5
                };
                window
                    .painted_quads()
                    .iter()
                    .filter(|quad| {
                        near(quad.bounds.origin.x.as_f32(), bounds.left())
                            && near(quad.bounds.origin.y.as_f32(), bounds.top())
                            && near(quad.bounds.size.width.as_f32(), bounds.size.width)
                            && near(quad.bounds.size.height.as_f32(), bounds.size.height)
                    })
                    .map(|quad| quad.background)
                    .collect::<Vec<_>>()
            })
        };
        // The state `id` says `line` inside the box `within` and in the
        // body's view.
        let says = |cx: &mut gpui::VisualTestContext,
                    id: &str,
                    line: &str,
                    within: &'static str,
                    at: &str| {
            assert_eq!(
                crate::empty_state::shown(id).as_deref(),
                Some(line),
                "{at}: {within} says its read"
            );
            let drawn = cx
                .debug_bounds(format!("{id}-line").leak())
                .unwrap_or_else(|| panic!("{at}: {id}'s line is drawn"));
            let within_bounds = cx.debug_bounds(within).expect("the box");
            let body = cx.debug_bounds("rewrite-review-body").expect("the body");
            assert!(drawn.size.height > px(0.), "{at}: {id} has a line");
            assert!(
                lies_in(drawn, within_bounds),
                "{at}: {id}'s line at {drawn:?} lies in {within}, {within_bounds:?}"
            );
            assert!(
                lies_in(drawn, body),
                "{at}: {id}'s line at {drawn:?} lies in the body's view, {body:?}"
            );
        };
        for (text, width, height) in [(13, 1480., 973.), (18, 461., 490.)] {
            let at = format!("{text} pt in {width} × {height}");
            size_to(&app, cx, text, width, height);
            cx.update(|window, cx| {
                review.update(cx, |review, cx| review.inspect(false, window, cx))
            });
            frame(cx);
            let reading = boxes(cx);
            says(
                cx,
                "series-files-reading",
                READING_FILES,
                "series-files",
                &at,
            );
            assert!(
                cx.debug_bounds("series-preview-reading").is_none(),
                "{at}: the preview reads nothing yet"
            );
            review.read_with(cx, |review, _| review.worker.wait_idle());
            draw(cx);
            assert!(
                cx.debug_bounds("series-files-reading").is_none(),
                "{at}: the line goes once the files land"
            );
            assert_eq!(boxes(cx), reading, "{at}: nothing moves as the files land");
            // The preview's box says which file to choose, as a designed
            // empty state in the box.
            assert_eq!(
                crate::empty_state::shown("series-preview-empty").as_deref(),
                Some(CHOOSE_FILE),
                "{at}: the preview's box says which file to choose"
            );
            let prompt = cx
                .debug_bounds("series-preview-empty-line")
                .unwrap_or_else(|| panic!("{at}: the prompt is drawn"));
            assert!(
                lies_in(prompt, reading[1]),
                "{at}: the prompt at {prompt:?} lies in the preview's box, {:?}",
                reading[1]
            );
            let scrolled = offset(cx);
            cx.update(|window, cx| review.update(cx, |review, cx| review.file(0, window, cx)));
            frame(cx);
            assert_eq!(
                offset(cx),
                scrolled,
                "{at}: choosing a file scrolls nothing"
            );
            // In view in the wide window, the modes wait for the patch with
            // none drawn selected: Patch is painted as Before source is.
            if text == 13 {
                assert_eq!(
                    fills(cx, "series-text-mode-0"),
                    fills(cx, "series-text-mode-1"),
                    "{at}: no mode is drawn selected while the modes wait"
                );
            }
            // In the half tile the preview lies wholly below the body's
            // view, so the chosen row says the read.
            let body = cx.debug_bounds("rewrite-review-body").expect("the body");
            let [files, preview] = boxes(cx);
            let hidden = text == 18;
            assert_eq!(
                preview.center().y > body.bottom(),
                hidden,
                "{at}: the preview, {preview:?}, against the body's view, {body:?}"
            );
            let row = cx.debug_bounds("series-file-reading");
            if hidden {
                let row = row.unwrap_or_else(|| panic!("{at}: the chosen row says its read"));
                assert!(
                    lies_in(row, files) && lies_in(row, body),
                    "{at}: the row's read at {row:?} lies in the file box, {files:?}, in view, \
                     {body:?}"
                );
                let node = crate::native_accessibility::written::get("series-file-0")
                    .unwrap_or_else(|| panic!("{at}: the chosen row's node is written"))
                    .node;
                assert_eq!(
                    node.description(),
                    Some(READING_PREVIEW),
                    "{at}: the chosen row's accessible description says its read"
                );
            } else {
                says(
                    cx,
                    "series-preview-reading",
                    READING_PREVIEW,
                    "series-file-preview",
                    &at,
                );
                assert!(
                    row.is_none(),
                    "{at}: the row says nothing where the preview's read is seen"
                );
            }
            assert_eq!(
                boxes(cx),
                reading,
                "{at}: nothing moves as the preview is read"
            );
            review.read_with(cx, |review, _| review.worker.wait_idle());
            draw(cx);
            assert!(
                review.read_with(cx, |review, _| matches!(
                    review.content.as_deref(),
                    Some(Content::Text { .. })
                )),
                "{at}: the file's patch lands"
            );
            assert!(
                cx.debug_bounds("series-preview-reading").is_none()
                    && cx.debug_bounds("series-file-reading").is_none(),
                "{at}: the read goes once the preview lands"
            );
            assert_eq!(offset(cx), scrolled, "{at}: nor does its landing");
            if text == 13 {
                assert_ne!(
                    fills(cx, "series-text-mode-0"),
                    fills(cx, "series-text-mode-1"),
                    "{at}: Patch is drawn selected once the patch lands"
                );
            }
            assert_eq!(
                boxes(cx),
                reading,
                "{at}: nothing moves as the preview lands"
            );
            cx.update(|window, cx| review.update(cx, |review, cx| review.back(window, cx)));
            draw(cx);
        }
    }

    /// The review's dialog as the last frame painted it: the smallest quad
    /// edged on every side around the body, which holds no edged box of its
    /// own around it.
    fn dialog_frame(cx: &mut gpui::VisualTestContext) -> Bounds<Pixels> {
        let body = cx.debug_bounds("rewrite-review-body").expect("the body");
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let logical = |value: gpui_kit::ScaledPixels| px(value.as_f32() / scale);
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| {
                    let widths = quad.border_widths;
                    [widths.top, widths.right, widths.bottom, widths.left]
                        .iter()
                        .all(|width| width.as_f32() > 0.)
                })
                .map(|quad| {
                    Bounds::from_corners(
                        point(logical(quad.bounds.left()), logical(quad.bounds.top())),
                        point(logical(quad.bounds.right()), logical(quad.bounds.bottom())),
                    )
                })
                .filter(|frame| {
                    frame.top() < body.top()
                        && frame.bottom() > body.bottom()
                        && frame.left() < body.left()
                        && frame.right() > body.right()
                })
                .min_by(|a, b| a.size.height.partial_cmp(&b.size.height).unwrap())
                .expect("the dialog's frame")
        })
    }

    /// The review's body takes the room the dialog's height cap leaves it,
    /// measured as the dialog lays out its title, footer and padding: an
    /// inspector fills it, the dialog's foot on its cap's, and the series
    /// running past it at 18 pt in a 461 × 490 window takes all of it too,
    /// at 13 and 18 pt in that window and in a 1,480 × 800 one; the content
    /// takes [`content_room`], which the series list keeps its rest from. A
    /// window less 250 px once stood for that room, which the body stopped
    /// at: at 13 pt in the 461 × 490 window it ended the dialog 53.5 px short
    /// of its cap's foot.
    #[gpui::test]
    async fn the_body_takes_the_room_the_dialog_cap_leaves(cx: &mut gpui_kit::TestAppContext) {
        use crate::tags::tests::{draw, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let browser = cx.update(|window, cx| {
            let repo = app
                .read(cx)
                .repository
                .clone()
                .expect("the window's repository");
            let browser = cx.new(|cx| {
                SeriesBrowser::new(
                    app.downgrade(),
                    repo,
                    "origin".into(),
                    "main".into(),
                    window,
                    cx,
                )
            });
            open_series_dialog(browser.clone(), window, cx);
            browser
        });
        lands(&browser, cx);
        let slack = px(0.5);
        for (text, width, height) in [
            (13, 461., 490.),
            (18, 461., 490.),
            (13, 1480., 800.),
            (18, 1480., 800.),
        ] {
            let at = format!("{text} pt in {width} × {height}");
            size_to(&app, cx, text, width, height);
            let (foot, room) = cx.update(|window, cx| {
                (
                    crate::theme_editor::status_bar_top(window) - appearance::DIALOG_MARGIN,
                    content_room(window, cx),
                )
            });
            for inspecting in [false, true] {
                let state = if inspecting {
                    "inspecting"
                } else {
                    "the series"
                };
                cx.update(|_, cx| {
                    browser.update(cx, |browser, cx| {
                        browser.inspecting = inspecting;
                        cx.notify();
                    })
                });
                // The inspector lays out its final room in the first frame
                // that draws it, and keeps it once everything has settled.
                let frames: &[&str] = if inspecting {
                    &["in its first frame", "settled"]
                } else {
                    &["settled"]
                };
                for &drawn in frames {
                    if drawn == "settled" {
                        draw(cx);
                        draw(cx);
                    } else {
                        cx.update(|window, cx| {
                            window.refresh();
                            window.draw(cx).clear(cx);
                        });
                    }
                    let state = format!("{state}, {drawn}");
                    let frame = dialog_frame(cx);
                    let content = cx
                        .debug_bounds("rewrite-review-content")
                        .expect("the review's content");
                    let body = cx.debug_bounds("rewrite-review-body").expect("the body");
                    let scrolls = browser.read_with(cx, |browser, _| {
                        browser.body.scroll().max_offset().y > px(0.)
                    });
                    if !inspecting && !scrolls {
                        assert!(
                            frame.bottom() <= foot + slack,
                            "{at}: {state} fits its dialog {frame:?} inside its cap's foot {foot:?}"
                        );
                        continue;
                    }
                    assert!(
                        (frame.bottom() - foot).abs() <= slack,
                        "{at}: {state}'s dialog {frame:?} ends at its cap's foot {foot:?}"
                    );
                    assert!(
                        (content.size.height - room).abs() <= slack,
                        "{at}: {state}'s content {content:?} takes the room the cap leaves, {room:?}"
                    );
                    assert!(
                        (body.bottom() - content.bottom()).abs() <= slack,
                        "{at}: {state}'s body {body:?} reaches the content's foot {content:?}"
                    );
                }
            }
        }
    }

    /// A review of `rows` commit pairs that name no commit, opened in
    /// `app`'s window, its read landed, with focus on its series.
    fn listed_review(
        app: &Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
        rows: usize,
    ) -> Entity<SeriesBrowser> {
        let browser = cx.update(|window, cx| {
            let repo = app
                .read(cx)
                .repository
                .clone()
                .expect("the window's repository");
            let browser = cx.new(|cx| {
                SeriesBrowser::new(
                    app.downgrade(),
                    repo,
                    "origin".into(),
                    "main".into(),
                    window,
                    cx,
                )
            });
            open_series_dialog(browser.clone(), window, cx);
            browser
        });
        lands(&browser, cx);
        let row = gitturtle_core::SeriesRow {
            original: None,
            rewritten: None,
            change: gitturtle_core::SeriesChange::Changed,
            reordered: false,
            ambiguous: false,
        };
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.review = Some(Arc::new(RewriteReview {
                    root: browser.repo.path().to_owned(),
                    branch: "main".into(),
                    base: String::new(),
                    original_head: String::new(),
                    rewritten_head: String::new(),
                    original: Vec::new(),
                    rewritten: Vec::new(),
                    rows: vec![row; rows],
                }));
                browser.selected = 0;
                browser.focus.focus(window, cx);
                cx.notify();
            })
        });
        crate::tags::tests::draw(cx);
        browser
    }

    /// The series list is as tall as the whole rows the content's room
    /// holds, inside its 1 px border, at least one, and scrolls by whole
    /// rows, its scrollbar cueing the rest: with eight pairs at 13 and 18 pt
    /// in a 461 × 490 window and in a 1,480 × 800 one, at the test display's
    /// 2× and at 13 pt in the larger window at 1.5× and 1.75× too, every row
    /// it draws lies whole inside it as Down selects each row in turn and Up
    /// each one back. Cut at the rest of the body's room, at 13 pt in the
    /// 461 × 490 window it showed one 70 px row and 13.5 px of the next, and
    /// centred the selection between half rows; with a flat 2 px for its
    /// border, which layout snaps to a device pixel on each side, it showed
    /// a device pixel of the next row at 1.5× and cut one off the last at
    /// 1.75×.
    #[gpui::test]
    async fn the_series_list_shows_whole_rows(cx: &mut gpui_kit::TestAppContext) {
        use crate::tags::tests::{draw, rendered, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let rows = 8;
        let browser = listed_review(&app, cx, rows);
        let slack = px(0.05);
        for (text, width, height, scale) in [
            (13, 461., 490., 2.),
            (18, 461., 490., 2.),
            (13, 1480., 800., 2.),
            (18, 1480., 800., 2.),
            (13, 1480., 800., 1.5),
            (13, 1480., 800., 1.75),
        ] {
            let at = format!("{text} pt in {width} × {height} at {scale}×");
            size_to(&app, cx, text, width, height);
            // A resize takes the test display's 2×, so the scale follows it.
            cx.update(|window, _| window.set_scale_factor(scale));
            draw(cx);
            // The border as layout draws it, a device pixel at 1.5×.
            let border = cx.update(|window, _| window.pixel_snap(px(1.)));
            // From the top, as the review opens.
            cx.update(|window, cx| {
                browser.update(cx, |browser, cx| {
                    browser.select(0, window, cx);
                    browser
                        .rows_scroll
                        .0
                        .borrow()
                        .base_handle
                        .set_offset(point(px(0.), px(0.)));
                })
            });
            draw(cx);
            let row = cx.update(|window, _| series_row_height(window));
            let list = rendered(cx, "series-focus-list");
            let shown = (list.size.height - border * 2.) / row;
            assert!(
                shown >= 1. && (shown - shown.round()).abs() * f32::from(row) < 0.05,
                "{at}: the list {list:?} holds whole rows of {row:?} inside its border"
            );
            assert!(
                (shown.round() as usize) < rows,
                "{at}: the list shows {shown} of its {rows} rows, which overflow it"
            );
            assert!(
                cx.debug_bounds("rewrite-series-scrollbar").is_some(),
                "{at}: the list's scrollbar cues the rows past its foot"
            );
            let inside = Bounds::from_corners(
                list.origin + point(border, border),
                list.bottom_right() - point(border, border),
            );
            let mut walk = vec!["down"; rows - 1];
            walk.extend(vec!["up"; rows - 1]);
            for (step, key) in walk.into_iter().enumerate() {
                cx.simulate_keystrokes(key);
                draw(cx);
                let selected = browser.read_with(cx, |browser, _| browser.selected);
                let drawn: Vec<_> = (0..rows)
                    .filter_map(|index| {
                        let selector: &'static str = format!("series-row-{index}").leak();
                        cx.debug_bounds(selector).map(|bounds| (index, bounds))
                    })
                    .filter(|(_, bounds)| bounds.intersects(&inside))
                    .collect();
                assert!(
                    drawn.iter().any(|(index, _)| *index == selected),
                    "{at}: after {key} (step {step}) the selected row {selected} is drawn"
                );
                for (index, bounds) in drawn {
                    assert!(
                        bounds.top() >= inside.top() - slack
                            && bounds.bottom() <= inside.bottom() + slack,
                        "{at}: after {key} (step {step}) row {index} {bounds:?} lies whole \
                         in the list {inside:?}"
                    );
                }
            }
        }
    }

    /// The rows of `rows` the series list draws inside its border in the
    /// last frame painted, each with whether it lies whole there.
    fn rows_seen(cx: &mut gpui::VisualTestContext, rows: usize) -> Vec<(usize, bool)> {
        let border = cx.update(|window, _| window.pixel_snap(px(1.)));
        let list = crate::tags::tests::rendered(cx, "series-focus-list");
        let inside = Bounds::from_corners(
            list.origin + point(border, border),
            list.bottom_right() - point(border, border),
        );
        let slack = px(0.05);
        (0..rows)
            .filter_map(|index| {
                let selector: &'static str = format!("series-row-{index}").leak();
                let bounds = cx.debug_bounds(selector)?;
                let seen = bounds.top() < inside.bottom() - slack
                    && bounds.bottom() > inside.top() + slack;
                let whole = bounds.top() >= inside.top() - slack
                    && bounds.bottom() <= inside.bottom() + slack;
                seen.then_some((index, whole))
            })
            .collect()
    }

    /// Whether the series list draws row `row` whole inside its border, and
    /// no other row, in the last frame painted.
    fn shows_only(cx: &mut gpui::VisualTestContext, row: usize, rows: usize) -> bool {
        rows_seen(cx, rows) == [(row, true)]
    }

    /// The selected row stays whole in the series list, by the least scroll,
    /// in the first frame after its rows change under it with no key: row 5
    /// of eight, selected at 13 pt in a 1,480 × 800 window, where the list
    /// shows five rows, row 5 at their foot, stays the row the list shows
    /// as the window is tiled to a 461 × 490 half tile, where it shows one,
    /// as the text grows to 18 pt there, and as the display's scale goes
    /// from 2× to 1.25×, where a row's snapped height goes from 94 to
    /// 94.4 px. Only a key revealed it: the half tile's list showed row 1
    /// with row 5 selected, and Return inspected a row nobody saw.
    #[gpui::test]
    async fn the_selected_row_stays_whole_as_its_rows_change(cx: &mut gpui_kit::TestAppContext) {
        use crate::tags::tests::{draw, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let rows = 8;
        let browser = listed_review(&app, cx, rows);
        size_to(&app, cx, 13, 1480., 800.);
        cx.update(|window, cx| browser.update(cx, |browser, cx| browser.select(5, window, cx)));
        draw(cx);
        let shown = cx.update(|window, cx| rows_shown(rows, window, cx));
        assert_eq!(
            rows_seen(cx, rows),
            (6 - shown..6).map(|row| (row, true)).collect::<Vec<_>>(),
            "the larger window's list shows its {shown} whole rows, row 5 at their foot"
        );
        assert!(
            shown > 1,
            "the larger window's list shows more than one row"
        );
        // One frame after each change, which no key follows.
        let frame = |cx: &mut gpui::VisualTestContext| {
            cx.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            })
        };
        cx.simulate_resize(size(px(461.), px(490.)));
        frame(cx);
        assert!(
            shows_only(cx, 5, rows),
            "tiled to 461 × 490, the one-row list shows the selected row 5 whole"
        );
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.interface_text_size = 18;
                appearance::apply_text_sizes(18, app.settings.code_text_size, window, cx);
                cx.notify();
            })
        });
        frame(cx);
        assert!(
            shows_only(cx, 5, rows),
            "at 18 pt there, the list shows row 5 whole"
        );
        let before = cx.update(|window, _| series_row_height(window));
        cx.update(|window, _| window.set_scale_factor(1.25));
        frame(cx);
        let after = cx.update(|window, _| series_row_height(window));
        assert_ne!(before, after, "the scale changes a row's snapped height");
        assert!(
            shows_only(cx, 5, rows),
            "at 1.25×, the list shows row 5 whole, {after:?} tall"
        );
        assert_eq!(
            browser.read_with(cx, |browser, _| browser.selected),
            5,
            "nothing changed the selection"
        );
    }

    /// The text modes keep their row for every preview, Diagrams among
    /// them, refused with their reason where it has no text or no Mermaid
    /// diagrams, so the preview's box keeps its place and size before a file
    /// is chosen and as an image's, a text file's, the image's again,
    /// another image's, a Mermaid file's, a document's and a notice's
    /// previews are chosen and land, at 13 and 18 pt in a 461 × 490 half
    /// tile and in a 1,480 × 800 window: the row left as an image landed and
    /// came back with the next choice, moving the preview 34.5 px each way,
    /// and Diagrams, there only for text with diagrams, wrapped the row at
    /// 18 pt in the half tile. A notice's modes give its own words.
    #[gpui::test]
    async fn the_preview_keeps_its_box_between_images_and_text(cx: &mut gpui_kit::TestAppContext) {
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let png = crate::system_preview::tests::png;
        for (round, message) in [("first", "Add files"), ("second", "Change them")] {
            std::fs::write(repo.path().join("a.png"), png(Some(round.as_bytes()))).unwrap();
            std::fs::write(
                repo.path().join("b.png"),
                png(Some(format!("{round} b").as_bytes())),
            )
            .unwrap();
            std::fs::write(repo.path().join("notes.txt"), format!("{round}\n")).unwrap();
            std::fs::write(
                repo.path().join("diagram.mmd"),
                format!("flowchart LR\n  A[{round}] --> B\n"),
            )
            .unwrap();
            std::fs::write(
                repo.path().join("document.pdf"),
                format!("%PDF-1.4\n% {round}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n"),
            )
            .unwrap();
            // Past the text preview's line limit, which a notice says.
            std::fs::write(
                repo.path().join("long.txt"),
                format!("{round}\n").repeat(gitturtle_core::MAX_DIFF_LINES + 1),
            )
            .unwrap();
            git(repo.path(), &["add", "."]);
            git(repo.path(), &["commit", "--quiet", "-m", message]);
        }
        // A fresh repository keeps its branch as a loose reference.
        let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
        let files = repo.changes_with_renames(head.trim(), 0).unwrap();
        let index = |path: &str| {
            files
                .iter()
                .position(|file| file.path() == std::path::Path::new(path))
                .unwrap_or_else(|| panic!("{path} changed"))
        };
        let (image, other, text_file) = (index("a.png"), index("b.png"), index("notes.txt"));
        let (mermaid, document, long) = (
            index("diagram.mmd"),
            index("document.pdf"),
            index("long.txt"),
        );
        let (app, cx) = window(cx, &repo);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let browser = cx.update(|window, cx| {
            let repo = app
                .read(cx)
                .repository
                .clone()
                .expect("the window's repository");
            let browser = cx.new(|cx| {
                SeriesBrowser::new(
                    app.downgrade(),
                    repo,
                    "origin".into(),
                    "main".into(),
                    window,
                    cx,
                )
            });
            open_series_dialog(browser.clone(), window, cx);
            browser
        });
        let frame = |cx: &mut gpui::VisualTestContext| {
            cx.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            })
        };
        let preview = |cx: &mut gpui::VisualTestContext| {
            cx.debug_bounds("series-file-preview")
                .expect("the preview's box is drawn")
        };
        for (text, width, height) in [
            (13, 461., 490.),
            (18, 461., 490.),
            (13, 1480., 800.),
            (18, 1480., 800.),
        ] {
            let at = format!("{text} pt in {width} × {height}");
            size_to(&app, cx, text, width, height);
            cx.update(|_, cx| {
                browser.update(cx, |browser, cx| {
                    browser.inspecting = true;
                    browser.inspect_label = "Rewritten".into();
                    browser.files = files.clone();
                    browser.clear_content();
                    cx.notify();
                })
            });
            draw(cx);
            let kept = preview(cx);
            for (name, chosen, kind) in [
                ("an image", image, "images"),
                ("a text file", text_file, "text"),
                ("the image again", image, "images"),
                ("another image", other, "images"),
                ("a Mermaid file", mermaid, "diagrams"),
                ("a document", document, "document"),
                ("a notice", long, "notice"),
            ] {
                let is_text = kind == "text" || kind == "diagrams";
                cx.update(|window, cx| {
                    browser.update(cx, |browser, cx| browser.file(chosen, window, cx))
                });
                frame(cx);
                assert_eq!(preview(cx), kept, "{at}: choosing {name} moves nothing");
                browser.read_with(cx, |browser, _| browser.worker.wait_idle());
                draw(cx);
                let (landed, notice) =
                    browser.read_with(cx, |browser, _| match browser.content.as_deref() {
                        Some(Content::Text {
                            diagrams: Some(_), ..
                        }) => (Some("diagrams"), None),
                        Some(Content::Text { .. }) => (Some("text"), None),
                        Some(Content::Images { .. }) => (Some("images"), None),
                        Some(Content::Rich(_)) => (Some("document"), None),
                        Some(Content::Notice(notice)) => (Some("notice"), Some(notice.clone())),
                        _ => (None, None),
                    });
                assert_eq!(landed, Some(kind), "{at}: {name}'s preview lands");
                assert_eq!(preview(cx), kept, "{at}: {name}'s landing moves nothing");
                let mode = |index: usize| {
                    crate::native_accessibility::written::get(&format!("series-text-mode-{index}"))
                        .unwrap_or_else(|| panic!("{at}: mode {index} is drawn for {name}"))
                        .node
                };
                let patch = mode(0);
                assert_eq!(
                    patch.is_disabled(),
                    !is_text,
                    "{at}: Patch waits for a preview with text, for {name}"
                );
                if !is_text {
                    assert_eq!(
                        patch.description(),
                        Some(notice.as_deref().unwrap_or(NO_TEXT)),
                        "{at}: Patch says why it waits, for {name}"
                    );
                }
                let diagrams = mode(3);
                assert_eq!(
                    diagrams.is_disabled(),
                    kind != "diagrams",
                    "{at}: Diagrams waits for text with diagrams, for {name}"
                );
                if kind != "diagrams" {
                    assert_eq!(
                        diagrams.description(),
                        Some(notice.as_deref().unwrap_or(NOT_DIAGRAMS)),
                        "{at}: Diagrams says why it waits, for {name}"
                    );
                }
            }
        }
    }

    /// A chosen file whose preview cannot be read, a link whose object is
    /// gone, says why
    /// in the preview's box, where the preview would draw, as a designed
    /// empty state, and the text modes give the read's error as their
    /// reason: the box went on asking for a file to be chosen with its file
    /// chosen, the error drawn below it at the body's end, and the modes said
    /// the file had no patch or source.
    #[gpui::test]
    async fn a_failed_preview_read_says_why_in_its_box(cx: &mut gpui_kit::TestAppContext) {
        use crate::tags::tests::{draw, git, rendered, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        std::fs::write(repo.path().join("notes.txt"), "notes\n").unwrap();
        git(repo.path(), &["add", "notes.txt"]);
        git(repo.path(), &["commit", "--quiet", "-m", "Add notes"]);
        // A fresh repository keeps its branch as a loose reference.
        let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
        let mut files = repo.changes_with_renames(head.trim(), 0).unwrap();
        // A stored link, which the preview never renders, whose object is
        // gone: its read fails, where a missing regular file's side says so
        // in a preview of its own.
        files[0].new_mode = "120000".into();
        files[0].new_oid = Some("0123456789abcdef0123456789abcdef01234567".into());
        let (app, cx) = window(cx, &repo);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let browser = cx.update(|window, cx| {
            let repo = app
                .read(cx)
                .repository
                .clone()
                .expect("the window's repository");
            let browser = cx.new(|cx| {
                SeriesBrowser::new(
                    app.downgrade(),
                    repo,
                    "origin".into(),
                    "main".into(),
                    window,
                    cx,
                )
            });
            open_series_dialog(browser.clone(), window, cx);
            browser
        });
        lands(&browser, cx);
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.inspecting = true;
                browser.inspect_label = "Rewritten".into();
                browser.files = files.clone();
                browser.file(0, window, cx);
            })
        });
        browser.read_with(cx, |browser, _| browser.worker.wait_idle());
        draw(cx);
        let error = browser
            .read_with(cx, |browser, _| browser.preview_error.clone())
            .expect("the preview's read fails");
        assert_eq!(
            crate::empty_state::shown("series-preview-failed").as_deref(),
            Some(error.as_str()),
            "the preview's box says why its read failed"
        );
        let (said, kept) = (
            rendered(cx, "series-preview-failed"),
            rendered(cx, "series-file-preview"),
        );
        assert!(
            lies_in(said, kept),
            "the failure {said:?} is said in the preview's box {kept:?}"
        );
        // Under its title, as every failure keeps its title above its
        // explanation (DESIGN.md, "Empty and edge states"): it had none.
        let (title, line) = (
            cx.debug_bounds("series-preview-failed-title")
                .expect("the failure's title is drawn"),
            rendered(cx, "series-preview-failed-line"),
        );
        assert!(
            lies_in(title, kept) && title.bottom() <= line.top(),
            "the title {title:?} lies in the box {kept:?}, above the error {line:?}"
        );
        assert!(
            crate::native_accessibility::written::labels()
                .iter()
                .any(|(_, name)| name.as_deref() == Some("Preview unavailable")),
            "the title reads \"Preview unavailable\""
        );
        assert!(
            cx.debug_bounds("series-preview-empty").is_none(),
            "the box no longer asks for a file to be chosen"
        );
        assert!(
            cx.debug_bounds("series-review-error").is_none(),
            "the failure is not said again at the body's end"
        );
        for mode in [0, 1, 2, 3] {
            let node =
                crate::native_accessibility::written::get(&format!("series-text-mode-{mode}"))
                    .unwrap_or_else(|| panic!("mode {mode} is drawn"))
                    .node;
            assert!(node.is_disabled(), "mode {mode} waits for a preview");
            assert_eq!(
                node.description(),
                Some(error.as_str()),
                "mode {mode} gives the read's error as its reason"
            );
        }
        // A failure that fits draws no scrollbar and keeps no gutter at
        // either scale: laid out as tall as its scroller, and an overflow
        // being strictly past zero, a float's remainder would draw one, and
        // make it a Tab stop with nothing to scroll.
        for (scale, width, height) in [(1., 1480., 800.), (1.5, 1480., 800.), (1.5, 1481., 803.)] {
            let at = format!("at 13 pt in {width} × {height} at {scale}×");
            size_to(&app, cx, 13, width, height);
            // A resize takes the test display's 2×, so the scale follows it.
            cx.update(|window, _| window.set_scale_factor(scale));
            draw(cx);
            assert!(
                cx.debug_bounds("series-preview-failure-scrollbar")
                    .is_none(),
                "{at}: a failure that fits draws no scrollbar"
            );
            assert_eq!(
                browser.read_with(cx, |browser, _| browser.failure_scroll.max_offset().y),
                px(0.),
                "{at}: a failure that fits has nothing to scroll, so keeps no gutter"
            );
        }
    }

    /// A review in `app`'s window, its read landed, inspecting `files` as
    /// a commit's, with none chosen.
    fn inspecting(
        app: &Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
        files: &[FileChange],
    ) -> Entity<SeriesBrowser> {
        let browser = cx.update(|window, cx| {
            let repo = app
                .read(cx)
                .repository
                .clone()
                .expect("the window's repository");
            let browser = cx.new(|cx| {
                SeriesBrowser::new(
                    app.downgrade(),
                    repo,
                    "origin".into(),
                    "main".into(),
                    window,
                    cx,
                )
            });
            open_series_dialog(browser.clone(), window, cx);
            browser
        });
        lands(&browser, cx);
        cx.update(|_, cx| {
            browser.update(cx, |browser, cx| {
                browser.inspecting = true;
                browser.inspect_label = "Rewritten".into();
                browser.files = files.to_vec();
                cx.notify();
            })
        });
        crate::tags::tests::draw(cx);
        browser
    }

    /// Chooses the inspected commit's file `index` and lands its preview.
    fn choose(browser: &Entity<SeriesBrowser>, cx: &mut gpui::VisualTestContext, index: usize) {
        cx.update(|window, cx| browser.update(cx, |browser, cx| browser.file(index, window, cx)));
        browser.read_with(cx, |browser, _| browser.worker.wait_idle());
        crate::tags::tests::draw(cx);
    }

    /// The changed files of `repo`'s last commit, and a lookup of each by
    /// its path.
    fn last_commit_files(repo: &GitRepository) -> Vec<FileChange> {
        // A fresh repository keeps its branch as a loose reference.
        let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
        repo.changes_with_renames(head.trim(), 0).unwrap()
    }

    fn file_index(files: &[FileChange], path: &str) -> usize {
        files
            .iter()
            .position(|file| file.path() == std::path::Path::new(path))
            .unwrap_or_else(|| panic!("{path} changed"))
    }

    /// The text modes the last frame drew selected: those that paint a fill
    /// over their bounds at rest, with the pointer away, which a ghost mode
    /// at rest does not.
    fn selected_modes(cx: &mut gpui::VisualTestContext) -> Vec<usize> {
        (0..4)
            .filter(|mode| {
                let key = TEXT_MODES[*mode].1;
                !crate::views::tests::painted_fills(cx, key).is_empty()
            })
            .collect()
    }

    /// Diagrams, chosen on a Mermaid file, gives way while it cannot apply:
    /// on `notes.txt` the modes draw Patch, the file's default text mode as
    /// Compare's Unified is, selected, and the box its patch, while the
    /// person's choice is kept, so the Mermaid file chosen again draws
    /// Diagrams selected and its diagrams. Left as the drawn mode, it drew
    /// no mode selected, while the row refused Diagrams, and the box the
    /// bare "No Mermaid diagrams on this file; use Source.".
    #[gpui::test]
    async fn diagrams_gives_way_to_the_patch_on_a_file_without_them(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        std::fs::write(repo.path().join("notes.txt"), "notes\n").unwrap();
        std::fs::write(repo.path().join("diagram.mmd"), "flowchart LR\n  A --> B\n").unwrap();
        git(repo.path(), &["add", "."]);
        git(
            repo.path(),
            &["commit", "--quiet", "-m", "Add notes and a diagram"],
        );
        let files = last_commit_files(&repo);
        let (mermaid, notes) = (
            file_index(&files, "diagram.mmd"),
            file_index(&files, "notes.txt"),
        );
        let (app, cx) = window(cx, &repo);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        size_to(&app, cx, 13, 1480., 800.);
        let browser = inspecting(&app, cx, &files);
        choose(&browser, cx, mermaid);
        let diagrams = cx.debug_bounds(TEXT_MODES[3].1).expect("Diagrams is drawn");
        cx.simulate_click(diagrams.center(), Modifiers::none());
        cx.simulate_mouse_move(point(px(-1.), px(-1.)), None, Modifiers::default());
        draw(cx);
        assert_eq!(
            selected_modes(cx),
            [3],
            "Diagrams is chosen and drawn selected"
        );
        choose(&browser, cx, notes);
        assert_eq!(
            browser.read_with(cx, |browser, _| browser.text_mode),
            3,
            "the person's choice of Diagrams is kept"
        );
        assert_eq!(
            selected_modes(cx),
            [0],
            "notes.txt, with no diagrams, draws Patch selected"
        );
        let (patch, text) = cx.update(|_, cx| {
            let browser = browser.read(cx);
            (
                browser.patch.is_some(),
                browser
                    .editor
                    .as_ref()
                    .map(|editor| editor.read(cx).value().to_string()),
            )
        });
        assert!(patch, "the box draws notes.txt's patch");
        assert!(
            text.is_some_and(|text| text.contains("+notes")),
            "the patch is notes.txt's"
        );
        choose(&browser, cx, mermaid);
        assert_eq!(
            selected_modes(cx),
            [3],
            "the Mermaid file chosen again draws Diagrams selected"
        );
        assert!(
            browser.read_with(cx, |browser, _| browser.patch.is_none()),
            "and its diagrams, not a patch"
        );
    }

    /// A preview failure taller than the preview's box, a five-line error
    /// at 18 pt in a 461 × 490 half tile, scrolls inside the box with the
    /// shared scroll cue rather than being cut: at its top the state starts
    /// whole in the box, and scrolled by the wheel to its end, the error's
    /// last line ends in it. Centred in a box that clips, it lost its first
    /// and last lines' edges.
    #[gpui::test]
    async fn a_long_preview_failure_scrolls_in_its_box(cx: &mut gpui_kit::TestAppContext) {
        use crate::tags::tests::{draw, git, rendered, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        std::fs::write(repo.path().join("notes.txt"), "notes\n").unwrap();
        git(repo.path(), &["add", "notes.txt"]);
        git(repo.path(), &["commit", "--quiet", "-m", "Add notes"]);
        let files = last_commit_files(&repo);
        let (app, cx) = window(cx, &repo);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        size_to(&app, cx, 18, 461., 490.);
        let browser = inspecting(&app, cx, &files);
        let error = (1..=5)
            .map(|line| format!("Line {line} of the read's error"))
            .collect::<Vec<_>>()
            .join("\n");
        cx.update(|_, cx| {
            browser.update(cx, |browser, cx| {
                browser.selected_file = Some(0);
                browser.preview_error = Some(error.clone());
                cx.notify();
            })
        });
        draw(cx);
        let slack = px(0.25);
        let kept = rendered(cx, "series-file-preview");
        let line = rendered(cx, "series-preview-failed-line");
        assert!(
            line.size.height > kept.size.height / 2.,
            "the error's five lines, {line:?}, fill most of the box, {kept:?}"
        );
        let top = cx
            .debug_bounds("series-preview-failed-title")
            .unwrap_or(line)
            .top();
        assert!(
            top >= kept.top() - slack,
            "at its top, the failure starts at {top:?} inside the box, {kept:?}"
        );
        assert!(
            cx.debug_bounds("series-preview-failure-scrollbar")
                .is_some(),
            "the box's scrollbar cues the failure's rest"
        );
        // The box lies wholly below the body's fold here, so the wheel
        // first brings it into the body's view, as a person's does: over
        // the box's place while the body hides it, it reaches no scroller.
        let wheel = |at: gpui::Point<Pixels>, cx: &mut gpui::VisualTestContext| {
            cx.simulate_event(gpui::ScrollWheelEvent {
                position: at,
                delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-300.))),
                modifiers: Modifiers::default(),
                touch_phase: gpui::TouchPhase::Moved,
            });
            draw(cx);
        };
        let body = rendered(cx, "rewrite-review-body");
        wheel(body.center(), cx);
        let (kept, body) = (
            rendered(cx, "series-file-preview"),
            rendered(cx, "rewrite-review-body"),
        );
        assert!(
            body.contains(&kept.center()),
            "the wheel brought the box, {kept:?}, into the body's view, {body:?}"
        );
        for _ in 0..3 {
            wheel(kept.center(), cx);
        }
        let (kept, line) = (
            rendered(cx, "series-file-preview"),
            rendered(cx, "series-preview-failed-line"),
        );
        assert!(
            line.bottom() <= kept.bottom() + slack,
            "scrolled to its end, the error's last line ends at {:?} inside the box, {kept:?}",
            line.bottom()
        );
    }

    /// `app`'s review inspecting the last commit of `repo`, its first file
    /// chosen and its read failed with `error`.
    fn failed_read(
        app: &Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
        repo: &GitRepository,
        error: &str,
    ) -> Entity<SeriesBrowser> {
        let browser = inspecting(app, cx, &last_commit_files(repo));
        cx.update(|_, cx| {
            browser.update(cx, |browser, cx| {
                browser.selected_file = Some(0);
                browser.preview_error = Some(error.into());
                cx.notify();
            })
        });
        crate::tags::tests::draw(cx);
        browser
    }

    /// A repository in `path` whose last commit adds `notes.txt`.
    fn committed_notes(path: &std::path::Path) -> GitRepository {
        use crate::tags::tests::{git, tagged_repository};
        let repo = tagged_repository(path);
        std::fs::write(repo.path().join("notes.txt"), "notes\n").unwrap();
        git(repo.path(), &["add", "notes.txt"]);
        git(repo.path(), &["commit", "--quiet", "-m", "Add notes"]);
        repo
    }

    /// Whether the failure in the preview's box holds focus.
    fn failure_focused(browser: &Entity<SeriesBrowser>, cx: &mut gpui::VisualTestContext) -> bool {
        cx.update(|window, cx| browser.read(cx).failure_focus.is_focused(window))
    }

    /// A failure taller than the preview's box, a five-line error at 18 pt
    /// in a 461 × 490 half tile, is a Tab stop while it overflows, by Tab
    /// and by Shift+Tab: focus on it rings the box's own 1 px edge whole and
    /// brings the box, which lies below the body's fold, into view; Up and
    /// Down scroll it a line at a time, to its ends and no further, and move
    /// nothing else. Made to fit by a resize while it holds focus, it keeps
    /// focus and its ring until the next Tab, which leaves it for good. It
    /// scrolled only by the pointer: the keyboard could not reach its last
    /// lines.
    #[gpui::test]
    async fn a_long_preview_failure_is_a_tab_stop_that_up_and_down_scroll(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::focus_ring::sweep;
        use crate::tags::tests::{draw, rendered, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = committed_notes(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        size_to(&app, cx, 18, 461., 490.);
        let error = (1..=5)
            .map(|line| format!("Line {line} of the read's error"))
            .collect::<Vec<_>>()
            .join("\n");
        let browser = failed_read(&app, cx, &repo, &error);
        let mut failures = Vec::new();
        for key in ["tab", "shift-tab"] {
            let mut reached = false;
            let mut walked = Vec::new();
            sweep::walk_each(
                &app,
                cx,
                key,
                &format!("the long failure by {key}"),
                18,
                &mut walked,
                &mut |cx, stop, rings| {
                    if !failure_focused(&browser, cx) {
                        return;
                    }
                    reached = true;
                    let (kept, body) = (
                        rendered(cx, "series-file-preview"),
                        rendered(cx, "rewrite-review-body"),
                    );
                    let drawn: Vec<_> = rings.iter().map(|ring| ring.bounds).collect();
                    if !sweep::rings_edge(&drawn, kept) {
                        failures.push(format!(
                            "{stop}: the failure draws no ring on the box's edge, {kept:?}, only \
                             {drawn:?}"
                        ));
                    }
                    if !lies_in(kept, body) {
                        failures.push(format!(
                            "{stop}: the box, {kept:?}, is not revealed in the body, {body:?}"
                        ));
                    }
                },
            );
            failures.extend(walked);
            if !reached {
                failures.push(format!("{key} never focused the long failure"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        for _ in 0..40 {
            if failure_focused(&browser, cx) {
                break;
            }
            cx.simulate_keystrokes("tab");
            draw(cx);
        }
        assert!(failure_focused(&browser, cx), "Tab reaches the failure");
        let scrolled = |cx: &mut gpui::VisualTestContext| {
            browser.read_with(cx, |browser, _| {
                (
                    -browser.failure_scroll.offset().y,
                    browser.failure_scroll.max_offset().y,
                )
            })
        };
        let body_at = |cx: &mut gpui::VisualTestContext| {
            browser.read_with(cx, |browser, _| browser.body.scroll().offset())
        };
        let press = |key: &str, cx: &mut gpui::VisualTestContext| {
            cx.simulate_keystrokes(key);
            draw(cx);
        };
        let (at, end) = scrolled(cx);
        assert_eq!(at, px(0.), "the failure shows from its top");
        assert!(end > px(0.), "the failure overflows the box");
        let line = cx.update(|window, _| crate::empty_state::block_height(1, false, window));
        let body = body_at(cx);
        press("down", cx);
        assert_eq!(scrolled(cx).0, line.min(end), "Down scrolls it a line");
        for _ in 0..40 {
            press("down", cx);
        }
        assert_eq!(scrolled(cx).0, end, "Down stops at its end");
        let (kept, last) = (
            rendered(cx, "series-file-preview"),
            rendered(cx, "series-preview-failed-line"),
        );
        assert!(
            last.bottom() <= kept.bottom() + px(0.25),
            "at its end, the error's last line ends at {:?} inside the box, {kept:?}",
            last.bottom()
        );
        press("up", cx);
        assert_eq!(
            scrolled(cx).0,
            (end - line).max(px(0.)),
            "Up scrolls it back a line"
        );
        assert_eq!(body_at(cx), body, "Up and Down move only the failure");
        assert!(failure_focused(&browser, cx), "Up and Down keep focus");
        // A resize that makes it fit leaves focus and the ring where they
        // are, until the next Tab.
        size_to(&app, cx, 13, 1480., 800.);
        assert_eq!(scrolled(cx).1, px(0.), "the failure fits the larger box");
        assert!(
            failure_focused(&browser, cx),
            "made to fit, it keeps focus until the next Tab"
        );
        assert!(
            cx.update(|window, _| window
                .context_stack()
                .iter()
                .any(|context| context.contains("Dialog"))),
            "made to fit, it is still drawn as what holds focus, so keys still reach the dialog"
        );
        let kept = rendered(cx, "series-file-preview");
        assert!(
            sweep::rings_edge(&outlines_in(cx, |p| p.accent), kept),
            "made to fit, it keeps its ring on the box's edge, {kept:?}"
        );
        press("tab", cx);
        assert!(
            !failure_focused(&browser, cx),
            "the next Tab leaves the failure that fits"
        );
        let mut walked = Vec::new();
        sweep::walk_each(
            &app,
            cx,
            "tab",
            "the failure once it fits",
            13,
            &mut walked,
            &mut |cx, stop, _| {
                assert!(
                    !failure_focused(&browser, cx),
                    "{stop}: a failure that fits is no Tab stop"
                );
            },
        );
        assert!(walked.is_empty(), "{}", walked.join("\n"));
    }

    /// A failure that fits the preview's box is no Tab stop, by Tab or by
    /// Shift+Tab, and a press on it takes focus from nothing: there is
    /// nothing in it for a key to reach.
    #[gpui::test]
    async fn a_short_preview_failure_is_no_tab_stop(cx: &mut gpui_kit::TestAppContext) {
        use crate::focus_ring::sweep;
        use crate::tags::tests::{draw, rendered, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = committed_notes(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        size_to(&app, cx, 13, 1480., 800.);
        let browser = failed_read(&app, cx, &repo, "The read's error");
        for key in ["tab", "shift-tab"] {
            let mut walked = Vec::new();
            sweep::walk_each(
                &app,
                cx,
                key,
                &format!("the short failure by {key}"),
                13,
                &mut walked,
                &mut |cx, stop, _| {
                    assert!(
                        !failure_focused(&browser, cx),
                        "{stop}: a failure that fits is no Tab stop"
                    );
                },
            );
            assert!(walked.is_empty(), "{}", walked.join("\n"));
        }
        let held = cx.update(|window, cx| window.focused(cx));
        let line = rendered(cx, "series-preview-failed-line");
        cx.simulate_click(line.center(), Modifiers::none());
        draw(cx);
        assert!(
            !failure_focused(&browser, cx),
            "a press on a failure that fits does not focus it"
        );
        assert_eq!(
            cx.update(|window, cx| window.focused(cx)),
            held,
            "a press on a failure that fits leaves focus where it was"
        );
    }

    /// Diagrams says why a file shows none: a Mermaid file stored as a
    /// link, that Diagrams never follows a link; one whose Git LFS object is
    /// not in the local store, that; and a file that is not Mermaid or
    /// Markdown, what Diagrams shows. "This file has no Mermaid diagrams."
    /// stood for all three, though the link's target or the object could
    /// hold diagrams. The reason judges the side Diagrams would draw, the
    /// one after the change or, for a file it removed, the one before: a
    /// Mermaid file changed from a link to a regular file once read as
    /// stored as a link.
    #[gpui::test]
    async fn diagrams_says_why_a_file_shows_none(cx: &mut gpui_kit::TestAppContext) {
        use crate::tags::tests::{git, tagged_repository, window};
        use std::os::unix::fs::symlink;
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let diagram = "flowchart LR\n  A --> B\n";
        symlink("notes.txt", repo.path().join("was-link.mmd")).unwrap();
        symlink("notes.txt", repo.path().join("gone-link.mmd")).unwrap();
        std::fs::write(repo.path().join("now-link.mmd"), diagram).unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "--quiet", "-m", "Add the sides"]);
        std::fs::remove_file(repo.path().join("was-link.mmd")).unwrap();
        std::fs::write(repo.path().join("was-link.mmd"), diagram).unwrap();
        std::fs::remove_file(repo.path().join("gone-link.mmd")).unwrap();
        std::fs::remove_file(repo.path().join("now-link.mmd")).unwrap();
        symlink("notes.txt", repo.path().join("now-link.mmd")).unwrap();
        std::fs::write(repo.path().join("notes.txt"), "notes\n").unwrap();
        std::fs::write(repo.path().join("link.mmd"), "flowchart LR\n  A --> B\n").unwrap();
        // A Git LFS pointer whose object the local store does not hold.
        std::fs::write(
            repo.path().join("missing.mmd"),
            format!(
                "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize 42\n",
                "a".repeat(64)
            ),
        )
        .unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "--quiet", "-m", "Add files"]);
        let mut files = last_commit_files(&repo);
        let (link, missing, notes) = (
            file_index(&files, "link.mmd"),
            file_index(&files, "missing.mmd"),
            file_index(&files, "notes.txt"),
        );
        let (was_link, now_link, gone_link) = (
            file_index(&files, "was-link.mmd"),
            file_index(&files, "now-link.mmd"),
            file_index(&files, "gone-link.mmd"),
        );
        // Stored as a link, whose text names its target.
        files[link].new_mode = "120000".into();
        let (app, cx) = window(cx, &repo);
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let browser = inspecting(&app, cx, &files);
        for (name, index, reason) in [
            ("a Mermaid file stored as a link", link, LINKED),
            (
                "a Mermaid file whose LFS object is missing",
                missing,
                NO_LFS_OBJECT,
            ),
            ("a text file", notes, NOT_DIAGRAMS),
            ("a link changed to a regular file", was_link, NO_DIAGRAMS),
            ("a regular file changed to a link", now_link, LINKED),
            ("a link the commit removed", gone_link, LINKED),
        ] {
            choose(&browser, cx, index);
            assert!(
                browser.read_with(cx, |browser, _| matches!(
                    browser.content.as_deref(),
                    Some(Content::Text { diagrams: None, .. })
                )),
                "{name}'s text lands, with no diagrams"
            );
            let node = crate::native_accessibility::written::get(TEXT_MODES[3].1)
                .unwrap_or_else(|| panic!("Diagrams is drawn for {name}"))
                .node;
            assert!(node.is_disabled(), "Diagrams waits, for {name}");
            assert_eq!(
                node.description(),
                Some(reason),
                "Diagrams says why, for {name}"
            );
        }
    }

    /// Whether the pointer resting on action `id` shows the tooltip `text`,
    /// as the window draws it under the pointer; the pointer then leaves the
    /// window until the tooltip has gone.
    fn hover_shows(cx: &mut gpui::VisualTestContext, id: &'static str, text: &str) -> bool {
        use crate::tags::tests::draw;
        const SHOW_DELAY: std::time::Duration = std::time::Duration::from_millis(600);
        let action = cx.debug_bounds(id).expect("the action is drawn");
        cx.simulate_mouse_move(action.center(), None, Modifiers::default());
        cx.executor().advance_clock(SHOW_DELAY);
        draw(cx);
        let shown = cx
            .debug_bounds(format!("{id}/tooltip/{text}").leak())
            .is_some();
        cx.simulate_mouse_move(point(px(-1.), px(-1.)), None, Modifiers::default());
        cx.executor().advance_clock(SHOW_DELAY);
        draw(cx);
        shown
    }

    /// Mid-merge, where the palette refuses its Edit local commits… and the
    /// branch menu leaves it out, the review's Edit local commits…, beside
    /// the line saying no rebase captured a series, is refused with the
    /// palette's reason, in its tooltip and its accessible description, and
    /// a press opens nothing: it was offered, refused only while a write
    /// ran. Once the review's repository is no longer the one shown, it says
    /// so, where it said to wait for a Git operation.
    #[gpui::test]
    async fn edit_local_commits_waits_for_a_merge_under_way(cx: &mut gpui_kit::TestAppContext) {
        use crate::native_accessibility::written;
        use crate::tags::tests::draw;
        let (_fixture, app, cx) = series_window(cx, 0).await;
        size_to(&app, cx, 13, 1480., 973.);
        let repo = app.read_with(cx, |app, _| app.repository.clone().expect("a repository"));
        let git = |args: &[&str]| {
            let identity = [
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ];
            crate::tags::tests::git(repo.path(), &[&identity[..], args].concat());
        };
        git(&["checkout", "--quiet", "-b", "side", "main~1"]);
        std::fs::write(repo.path().join("side.txt"), "side\n").unwrap();
        git(&["add", "side.txt"]);
        git(&["commit", "--quiet", "-m", "side"]);
        git(&["checkout", "--quiet", "main"]);
        git(&["merge", "--quiet", "--no-commit", "--no-ff", "side"]);
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        crate::views::tests::settle(&app, cx).await;
        assert!(
            app.read_with(cx, |app, _| app.integration_state.is_some()),
            "a merge is under way"
        );
        let reason = cx
            .update(|window, cx| {
                app.read(cx).menu_command_reason(
                    crate::command_palette::CommandId::Rebase,
                    window,
                    cx,
                )
            })
            .expect("the palette refuses Edit local commits… mid-merge");
        let _review = review_from_palette(&app, cx);
        assert_eq!(
            crate::empty_state::shown("series-empty").as_deref(),
            Some(recovery_drafts::NO_CAPTURE),
            "the review has no captured series to show"
        );
        let node = written::get("series-edit-commits")
            .expect("Edit local commits…' node is written")
            .node;
        assert!(node.is_disabled(), "the merge refuses Edit local commits…");
        assert_eq!(
            node.description(),
            Some(reason),
            "Edit local commits… says why with the palette's reason"
        );
        assert!(
            hover_shows(cx, "series-edit-commits", reason),
            "its tooltip says why"
        );
        let edit = cx
            .debug_bounds("series-edit-commits")
            .expect("Edit local commits… is drawn");
        cx.simulate_click(edit.center(), Modifiers::none());
        draw(cx);
        assert!(
            cx.debug_bounds("load-rebase-commits").is_none()
                && cx.debug_bounds("series-focus-list").is_some(),
            "a press on the refused Edit local commits… opens nothing"
        );
        // The repository no longer the one shown.
        app.update(cx, |app, cx| {
            app.page = AppPage::Settings;
            cx.notify();
        });
        draw(cx);
        let node = written::get("series-edit-commits")
            .expect("Edit local commits…' node is written")
            .node;
        assert!(
            node.is_disabled(),
            "a repository no longer shown refuses it"
        );
        assert_eq!(
            node.description(),
            Some(NOT_SHOWN),
            "Edit local commits… says its repository is no longer shown"
        );
    }

    /// With a capture present, Check publication destination… is offered,
    /// and Tab onto it at 18 pt in a 461 × 490 window reveals it with its
    /// ring whole, below the publication fields.
    #[gpui::test]
    async fn check_publication_destination_draws_its_ring_whole(cx: &mut gpui_kit::TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, review, cx) = rewritten_review(
            cx,
            &[&[
                "commit",
                "--quiet",
                "--amend",
                "-m",
                "fixture notes 2, reworded",
            ]],
        )
        .await;
        let app = cx.update(|_, cx| review.read(cx).owner.upgrade().expect("the app"));
        crate::focus_ring::sweep::prepare(&app, cx);
        cx.simulate_resize(size(px(461.), px(490.)));
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        draw(cx);
        let check = cx.update(|_, cx| review.read(cx).body.focus("inspect-publication-remote", cx));
        for _ in 0..12 {
            if cx.update(|window, _| check.is_focused(window)) {
                break;
            }
            cx.simulate_keystrokes("tab");
            draw(cx);
        }
        assert!(
            cx.update(|window, _| check.is_focused(window)),
            "Tab reaches Check publication destination…"
        );
        crate::tags::tests::assert_rings_whole(cx, &["inspect-publication-remote"]);
        let body = cx
            .debug_bounds("rewrite-review-body")
            .expect("the body is drawn");
        let control = cx
            .debug_bounds("inspect-publication-remote")
            .expect("Check publication destination… is drawn");
        assert!(
            lies_in(control, body),
            "Check publication destination… at {control:?} is revealed in the body, {body:?}"
        );
    }

    /// A rebase run through Edit local commits' Start rebase is reviewed
    /// from the palette, as QA-106e reviewed one: its capture is found and
    /// the review lists its series, and Refresh series lists it again,
    /// selecting its first row and showing it whole at 18 pt in a 461 × 490
    /// half tile, where the list shows one row: with the last row selected
    /// by Down before it, the reload selected row 0 and went on showing the
    /// last.
    #[gpui::test]
    async fn a_rebase_started_in_edit_local_commits_is_reviewed_from_the_palette(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let (_fixture, app, cx) = series_window(cx, 0).await;
        size_to(&app, cx, 13, 1480., 973.);
        rebase_dropping_the_second(&app, cx, "main~3").await;
        size_to(&app, cx, 18, 461., 490.);
        let review = review_from_palette(&app, cx);
        let listed = |cx: &mut gpui::VisualTestContext| {
            review.read_with(cx, |review, _| {
                (
                    review.error.clone(),
                    review.review.as_ref().map(|review| {
                        review
                            .rows
                            .iter()
                            .map(|row| row.change.label())
                            .collect::<Vec<_>>()
                    }),
                )
            })
        };
        let (error, rows) = listed(cx);
        assert_eq!(error, None, "the captured series is read");
        let rows = rows.expect("the review lists the captured series");
        assert_eq!(rows.len(), 3, "its three original commits: {rows:?}");
        assert!(
            rows.contains(&"Dropped / unmatched"),
            "the dropped commit is listed: {rows:?}"
        );
        assert!(
            cx.debug_bounds("series-empty").is_none(),
            "a listed series draws no empty state"
        );
        cx.simulate_keystrokes("down down");
        crate::tags::tests::draw(cx);
        assert!(
            shows_only(cx, 2, rows.len()),
            "Down selects the last row and shows it alone"
        );
        // Refresh series, reached by Tab, which reveals it below the half
        // tile's fold, and pressed with Return: a click at its place there
        // reached nothing, so the series was never read again.
        let reload = cx.update(|_, cx| review.read(cx).body.focus("reload-series", cx));
        for _ in 0..6 {
            if cx.update(|window, _| reload.is_focused(window)) {
                break;
            }
            cx.simulate_keystrokes("tab");
            crate::tags::tests::draw(cx);
        }
        assert!(
            cx.update(|window, _| reload.is_focused(window)),
            "Tab moves onto Refresh series"
        );
        let read = review.read_with(cx, |review, _| review.generation);
        enter(cx, true);
        enter(cx, false);
        lands(&review, cx);
        assert_ne!(
            review.read_with(cx, |review, _| review.generation),
            read,
            "Refresh series reads the series again"
        );
        assert_eq!(
            listed(cx).1.map(|rows| rows.len()),
            Some(3),
            "Refresh series lists it again"
        );
        assert_eq!(
            review.read_with(cx, |review, _| review.selected),
            0,
            "the reload selects the series' first row"
        );
        assert!(
            shows_only(cx, 0, rows.len()),
            "the list shows the selected first row whole"
        );
    }

    /// QA-106e's route: the palette's review before any capture, then a
    /// rebase through Edit local commits' Start rebase, the review opened
    /// again and Refresh series pressed. Its series holds a commit whose
    /// patch is past the per-command bound on what a read keeps (4 MiB), as
    /// QA-106e's two 30 MB images were, so the capture is found but the
    /// bounded review refuses the series: the review says why in the list's
    /// place, where it drew an empty list with no text.
    #[gpui::test]
    async fn a_series_past_the_review_bound_says_why_after_its_rebase(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let (_fixture, app, cx) = series_window(cx, 4_500_000).await;
        // The app's focus ring, which `says_why` finds whole.
        crate::focus_ring::sweep::prepare(&app, cx);
        size_to(&app, cx, 18, 461., 490.);
        let review = review_from_palette(&app, cx);
        says_why(&review, cx, &UNCAPTURED, "before the rebase");
        cx.simulate_keystrokes("escape");
        crate::tags::tests::draw(cx);
        size_to(&app, cx, 13, 1480., 973.);
        rebase_dropping_the_second(&app, cx, "main~3").await;
        size_to(&app, cx, 18, 461., 490.);
        let review = review_from_palette(&app, cx);
        let error = review
            .read_with(cx, |review, _| review.error.clone())
            .expect("the bounded review refuses the series");
        assert_eq!(
            error,
            "This series is too large to review here: its patches pass the review's 4 MiB read \
             limit. Compare it with `git range-diff`.",
            "the capture is found and its series is past the bound"
        );
        let says = Says {
            line: &error,
            title: Some(UNREVIEWABLE),
            edit: false,
        };
        says_why(&review, cx, &says, "after the rebase");
        refresh(&review, cx, &says, "after the rebase");
        says_why(&review, cx, &says, "after Refresh series");
    }
}
