//! A retained native commit-series inspector and a separate reviewed lease push.
use crate::*;
use gitturtle_core::{
    LeasedPublishPlan, OperationControl, PublicationInspection, RewriteReview, WriteCommand,
};
use gpui_kit::base::ElementExt as _;
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
        let line = form.clone();
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
            .footer(
                DialogFooter::new()
                    // The review's download, while its own row is not
                    // drawn, on the leading side: a line of the footer's
                    // one button's height, built for the room Back to
                    // repository leaves it ([`SeriesBrowser::footer_line`]).
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(appearance::ui_size(28.))
                            .flex()
                            .flex_col()
                            .child(crate::in_frame::in_frame(move |bounds, window, cx| {
                                line.read(cx)
                                    .footer_line(line.downgrade(), bounds.size.width, window, cx)
                                    .unwrap_or_else(|| div().into_any_element())
                            })),
                    )
                    .child(
                        div().flex_none().child(
                            button("close-series-review", "Back to repository", "", false)
                                .debug_selector(|| "close-series-review".into())
                                .on_click(move |_, window, cx| {
                                    close.update(cx, |form, cx| form.close(window, cx))
                                }),
                        ),
                    ),
            )
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
fn series_keys() -> SharedString {
    format!(
        "Up and Down select a commit pair. {} inspects its rewritten files, or the original's where it has no rewritten commit.",
        crate::shortcuts::ENTER
    )
    .into()
}

/// What the list's place says while the series is read.
const READING: &str = "Reading captured local history…";

/// What an inspected commit's file box says while its changed files are
/// read.
const READING_FILES: &str = "Reading changed files…";

/// What the preview's box says while a chosen file's preview is read.
const READING_PREVIEW: &str = "Reading preview…";

/// What the line on the review's footer and its alert say of a download of
/// `object` that failed with `words` while the review showed another file
/// or commit: "Couldn't download flow.mmd's Before object: …".
fn failed_sentence(object: &str, words: &str) -> String {
    format!("Couldn't download {object}: {words}")
}

/// The least the download's line on the dialog's footer keeps of its text
/// ("Downloading flow.mmd's Before object") before Cancel download's word
/// gives way: its first word and an ellipsis
/// (`lfs_download::line_cancel_form`).
const LINE_LEAST: &str = "Downloading…";

/// What the chosen file's row says at its end while its preview is read
/// below the body's view ([`SeriesBrowser::preview_read_hidden`]).
const READING_ROW: &str = "Reading…";

/// The title a failed read of a chosen file's preview keeps above the
/// read's error in the preview's box (DESIGN.md, "Empty and edge states").
const PREVIEW_UNAVAILABLE: &str = "Preview unavailable";

/// The title a failed read of an inspected commit's changed files keeps
/// above the read's error in the file box (DESIGN.md, "Empty and edge
/// states").
const FILES_UNAVAILABLE: &str = "Changed files unavailable";

/// How tall the file box grows to hold a failed read whole, in logical
/// pixels; a longer failure scrolls inside it. Git's usual message runs to
/// three lines, which at 18 pt in a 461 × 490 window, wrapped, is taller than
/// the box's resting 130 px.
const FILES_FAILURE_MOST: f32 = 200.;

/// What the preview's box says when the changed files could not be read,
/// where it would otherwise say the commit changed none.
const NO_FILE_TO_PREVIEW: &str = "No file to preview: the changed files could not be read.";

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

/// Why Diagrams waits for a Mermaid file whose Git LFS object was not found
/// absent but cannot be read as text: binary, too large, failing
/// verification, or past the limit at which the store is not read at all, so
/// the reason says neither that it is there nor that it is missing.
const NO_LFS_TEXT: &str = "This file's Git LFS object can't be shown as text.";

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

/// A box's failed read, which [`SeriesBrowser::failure_box`] scrolls inside
/// it: its element's id, its scrollbar's, the name it is read by while it is
/// a stop, and its scroll and focus.
struct FailureBox<'a> {
    id: &'static str,
    scrollbar: &'static str,
    label: &'static str,
    scroll: &'a ScrollHandle,
    rest: &'a std::rc::Rc<std::cell::Cell<Pixels>>,
    focus: &'a FocusHandle,
}

#[cfg(test)]
thread_local! {
    /// The text colour the words saying how the review's download ended
    /// were last drawn in on the dialog's footer, for a test.
    static ENDED_LINE_COLOR: std::cell::Cell<Option<gpui::Hsla>> =
        const { std::cell::Cell::new(None) };
}

/// The download a Download in the review's preview asked for: the chosen
/// file whose missing object it fetches, of the commit inspected, the side
/// pressed, and the failure's words once it failed, which the row says in
/// place of its sentence (`lfs_download::AskedState`). It is said of that
/// file of that commit only, while the review shows it
/// ([`SeriesBrowser::shows`]); while the review shows another, the dialog's
/// footer says the progress and offers Cancel
/// ([`SeriesBrowser::footer_line`]).
struct ReviewDownload {
    file: FileChange,
    commit: Option<String>,
    side: usize,
    failure: Option<SharedString>,
    /// Whether the row has said the failure: one that landed while the
    /// review showed another file or commit waits for the person to come
    /// back to its file, where the row says it.
    seen: std::cell::Cell<bool>,
    /// The words its line on the dialog's footer keeps room for, its name
    /// and "Cancelling…", measured once for its object
    /// (`lfs_download::footer_progress_room`).
    footer_widest: crate::lfs_download::FooterWidest,
    /// Whether the person cancelled its write while it ran, read from the
    /// write's own state as the review follows it: a write that then fails
    /// ended by their Cancel, whatever Git reports, which is no failure
    /// ([`SeriesBrowser::follow_download`]).
    cancelled: bool,
}

/// How the review's download ended where the line on the dialog's footer
/// says it for 1.5 s, in place of its progress
/// ([`SeriesBrowser::footer_line`]).
enum EndedLine {
    /// "Downloaded flow.mmd's Before object", after a check.
    Downloaded(SharedString),
    /// "Cancelled download of flow.mmd's Before object", the person's own
    /// Cancel, in words alone in the normal text colour: no failure.
    Cancelled(SharedString),
}

impl ReviewDownload {
    /// The object it fetches, as its controls and outcomes name it:
    /// "flow.mmd's Before object".
    fn object(&self) -> String {
        format!(
            "{}'s {} object",
            self.file.path().display(),
            ["Before", "After"][self.side]
        )
    }

    /// Its Cancel download, named for the object it fetches whatever its
    /// label reads, and whether `owner`'s write was cancelled, which its
    /// name then says too: "Cancelling download of flow.mmd's Before
    /// object" (decision, 2026-11-10: pressed, it was still named "Cancel
    /// download of …", so in its icon form nothing it said told that it was
    /// cancelling).
    fn cancel(&self, owner: &GitTurtle) -> crate::lfs_download::RunningCancel {
        let cancelling = owner.operation_cancelling();
        let verb = if cancelling { "Cancelling" } else { "Cancel" };
        crate::lfs_download::RunningCancel {
            name: format!("{verb} download of {}", self.object()).into(),
            cancelling,
        }
    }
}

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
    rows_scroll_rest: std::rc::Rc<std::cell::Cell<Pixels>>,
    /// How many whole rows the series list showed, and how tall they were,
    /// as last drawn ([`Self::follow_rows`]).
    rows_layout: Option<(usize, Pixels)>,
    files_scroll: UniformListScrollHandle,
    files_scroll_rest: std::rc::Rc<std::cell::Cell<Pixels>>,
    inspecting: bool,
    /// The commit whose files are inspected, by its object ID.
    inspected: Option<String>,
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
    failure_scroll_rest: std::rc::Rc<std::cell::Cell<Pixels>>,
    /// The focus of a failure taller than the preview's box, which holds a
    /// Tab stop only while the failure overflows
    /// ([`Self::preview_prompt`]).
    failure_focus: FocusHandle,
    /// The line a failing Git LFS side says above the file's preview.
    lfs_failure_line: crate::lfs_download::FailureLine,
    /// The download a Download in the preview asked for once its write
    /// started, which the review follows as it runs and ends
    /// ([`Self::follow_download`]): it covers the operation bar, so its own
    /// Download row says the download's progress, offers its Cancel and
    /// says how it failed.
    lfs_download: Option<ReviewDownload>,
    /// What a Download asked for while its form is open, which takes
    /// [`Self::lfs_download`]'s place only as its write starts: a form
    /// opened and cancelled leaves a failure kept there for its file's
    /// return, and the next press replaces it. Recorded over that failure
    /// as the form merely opened, it lost the failure the footer kept until
    /// the person returned to its file.
    asking: Option<ReviewDownload>,
    /// The focus each side's Download in the preview tracks.
    download_focus: [FocusHandle; 2],
    /// Cancel download's focus, in the row while the download runs.
    download_cancel: FocusHandle,
    /// What the dialog's footer says for 1.5 s in place of the download's
    /// line once it succeeded, or the person cancelled it, while the row
    /// did not say it, and the timer that ends it
    /// ([`Self::footer_line`]).
    ended_line: Option<(EndedLine, Task<()>)>,
    /// Why the inspected commit's changed files could not be read, which
    /// their box says and the text modes give as their reason.
    files_error: Option<String>,
    /// The scroll of a failure taller than the file box, from its top for
    /// each showing.
    files_failure_scroll: ScrollHandle,
    files_failure_scroll_rest: std::rc::Rc<std::cell::Cell<Pixels>>,
    /// The focus of a failure taller than the file box, a Tab stop only
    /// while the failure overflows.
    files_failure_focus: FocusHandle,
    error_scroll: crate::tags::NestedScroll,
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
            rows_scroll_rest: Default::default(),
            rows_layout: None,
            files_scroll: UniformListScrollHandle::new(),
            files_scroll_rest: Default::default(),
            inspecting: false,
            inspected: None,
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
            failure_scroll_rest: Default::default(),
            failure_focus: cx.focus_handle(),
            lfs_failure_line: crate::lfs_download::FailureLine::new("series-lfs-failure", cx),
            lfs_download: None,
            asking: None,
            download_focus: [cx.focus_handle(), cx.focus_handle()],
            download_cancel: cx.focus_handle(),
            ended_line: None,
            files_error: None,
            files_failure_scroll: ScrollHandle::new(),
            files_failure_scroll_rest: Default::default(),
            files_failure_focus: cx.focus_handle(),
            error_scroll: crate::tags::NestedScroll::default(),
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
        if let Some(owner) = this.owner.upgrade() {
            this._subscriptions
                .push(cx.observe_in(&owner, window, Self::follow_download));
        }
        // Released, as a window closing its dialogs releases it, the
        // review hands a download it asked for over to Compare.
        let (owner, me) = (
            this.owner.clone(),
            crate::lfs_download::Asker::Review(cx.entity_id()),
        );
        cx.on_release(move |_, cx| {
            let _ = owner.update(cx, |owner, _| owner.lfs_downloads.left(me));
        })
        .detach();
        this.load(window, cx);
        this
    }
    /// Follows the download a Download in the preview asked for, which the
    /// window runs for this review (`lfs_download::Asker`): once its write
    /// looks busy, the pressed Download gives its place to the progress and
    /// Cancel download, and focus on it moves to Cancel, which says so once
    /// as it takes focus. As it ends, a success reads the chosen file's
    /// preview again, if it is still the file asked from, which is its
    /// feedback; a failure says its words in the row, at once to assistive
    /// technology too, and the Download comes back, taking focus from
    /// Cancel. A download the person cancelled is no failure: the row comes
    /// back at rest, or where it does not say the download, the footer says
    /// it was cancelled, and either is said politely. What a Download asked
    /// for is followed once its write starts,
    /// in place of a failure kept until then; a form cancelled before its
    /// write leaves nothing to follow.
    fn follow_download(
        &mut self,
        owner: Entity<GitTurtle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let me = crate::lfs_download::Asker::Review(cx.entity_id());
        let (running, shown, ended, cancelling) = {
            let owner = owner.read(cx);
            (
                owner.lfs_downloads.running() == Some(me),
                owner.operation_shown().is_some(),
                owner.lfs_downloads.ended(me),
                owner.operation_cancelling(),
            )
        };
        // The window refuses a press while a write runs, so the write that
        // runs for the review is the one its last form asked for.
        if running && let Some(asking) = self.asking.take() {
            self.lfs_download = Some(asking);
        }
        // Every file's row tracks the same handles, so another file's
        // Download may hold focus while this download runs: focus moved
        // onto a Cancel no row drew.
        let drawn = self
            .lfs_download
            .as_ref()
            .is_some_and(|asked| self.row_draws(asked));
        // Where focus goes from a Cancel download that the download's end
        // takes away with nothing of its own in its place: left on it, focus
        // rested on nothing drawn and keys reached the window's root.
        let away = if self.inspecting {
            self.back.clone()
        } else {
            self.focus.clone()
        };
        let current = self.current(cx);
        let Some(asked) = self
            .lfs_download
            .as_mut()
            .filter(|asked| asked.failure.is_none())
        else {
            return;
        };
        if running {
            // Read as it runs: the write's control is gone once it ends.
            asked.cancelled |= cancelling;
            if shown && drawn && self.download_focus[asked.side].is_focused(window) {
                self.download_cancel.focus(window, cx);
            }
            return;
        }
        let cancel_focused = self.download_cancel.is_focused(window);
        match ended.flatten() {
            Some(words) if !asked.cancelled => {
                asked.failure = Some(words.clone());
                // The Download comes back with focus where the row draws
                // it; Cancel on the dialog's footer leaves nothing.
                if cancel_focused && drawn {
                    self.download_focus[asked.side].focus(window, cx);
                } else if cancel_focused {
                    away.focus(window, cx);
                }
                // Where the row does not say it, the line on the dialog's
                // footer does, naming what failed, and so does the alert.
                let said = if drawn {
                    words.to_string()
                } else {
                    failed_sentence(&asked.object(), &words)
                };
                let _ = crate::copy_feedback::alert(
                    window,
                    cx,
                    native_accessibility::bounded_accessible_text(said),
                );
                cx.notify();
            }
            // Ended by the person's own Cancel, whatever Git reports, which
            // is no failure (decision, 2026-11-10: feedback in place, and a
            // person's own Cancel is not an error): the row comes back at
            // rest, its Download with focus where Cancel held it; where the
            // row does not say it, its line on the dialog's footer says so
            // for 1.5 s in the normal text colour; said once, politely,
            // never as an alert. Reported as a failure, its warning words
            // interrupted as an alert and stayed on the footer until the
            // person returned to its file.
            Some(_) => {
                let Some(asked) = self.lfs_download.take() else {
                    return;
                };
                if cancel_focused && drawn {
                    self.download_focus[asked.side].focus(window, cx);
                } else if cancel_focused {
                    away.focus(window, cx);
                }
                if current {
                    let said: SharedString =
                        format!("Cancelled download of {}", asked.object()).into();
                    crate::copy_feedback::announce(window, cx, said.clone());
                    if !drawn {
                        self.say_ended(EndedLine::Cancelled(said), cx);
                    }
                }
                cx.notify();
            }
            None => {
                let Some(asked) = self.lfs_download.take() else {
                    return;
                };
                // The side's Download or Cancel, which the read again takes
                // away: its Download too where the write ended inside the
                // busy delay, which left focus on a vanished button.
                let held = cancel_focused || self.download_focus[asked.side].is_focused(window);
                let shown = self.shows(&asked);
                if let Some(index) = shown.filter(|_| self.current(cx)) {
                    if held {
                        self.body.focus("series-file-preview", cx).focus(window, cx);
                    }
                    // Said once, politely, here: the window's notice says
                    // it beneath the review, hidden from assistive
                    // technology while the review is open.
                    crate::copy_feedback::announce(
                        window,
                        cx,
                        format!(
                            "Downloaded {}'s {} object",
                            asked.file.path().display(),
                            ["Before", "After"][asked.side]
                        ),
                    );
                    self.file(index, window, cx);
                } else if current {
                    if cancel_focused {
                        away.focus(window, cx);
                    }
                    // Its line on the dialog's footer, where the progress
                    // stood, says it for 1.5 s with a check, said once,
                    // politely, as a Git action's feedback in place is.
                    let said: SharedString = format!("Downloaded {}", asked.object()).into();
                    crate::copy_feedback::announce(window, cx, said.clone());
                    self.say_ended(EndedLine::Downloaded(said), cx);
                }
            }
        }
    }
    /// Says how the review's download ended, `line`, on the dialog's footer
    /// for 1.5 s where its line stood ([`Self::footer_line`]).
    fn say_ended(&mut self, line: EndedLine, cx: &mut Context<Self>) {
        let ends = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(crate::copy_feedback::RESTORE_AFTER)
                .await;
            let _ = this.update(cx, |this, cx| {
                this.ended_line = None;
                cx.notify();
            });
        });
        self.ended_line = Some((line, ends));
        cx.notify();
    }
    /// Where the review shows the file `asked` was asked for, of the commit
    /// it was asked from, if it still does: matched by its place in the
    /// list alone, a late end after Back and another commit read that
    /// commit's file at the same place.
    fn shows(&self, asked: &ReviewDownload) -> Option<usize> {
        if !self.inspecting || self.inspected != asked.commit {
            return None;
        }
        self.selected_file
            .filter(|index| self.files.get(*index) == Some(&asked.file))
    }
    /// Whether the preview's Download row draws what `asked` says: the
    /// review shows its file of its commit, whose side still offers its
    /// download.
    fn row_draws(&self, asked: &ReviewDownload) -> bool {
        let Some(index) = self.shows(asked) else {
            return false;
        };
        self.messages.is_none()
            && self.content.as_deref().is_some_and(|content| {
                crate::lfs_download::preview_targets(content, &self.files[index])
                    .iter()
                    .any(|(side, _)| *side == asked.side)
            })
    }
    /// Whether the preview's row says what `asked` asks, or will once the
    /// person's file, being read, lands: the line on the dialog's footer
    /// stays away meanwhile, so it does not come and go as they return. A
    /// read that ends without a preview, failed or interrupted, leaves no
    /// row, so the line stands again: counted as the row's whenever the
    /// file had no preview, the download's progress and Cancel were then
    /// drawn nowhere.
    fn row_says(&self, asked: &ReviewDownload) -> bool {
        self.row_draws(asked)
            || (self.shows(asked).is_some() && self.messages.is_none() && self.reading_preview())
    }
    /// The progress and Cancel download of the download a Download in the
    /// preview asked for, on the leading side of the dialog's footer beside
    /// Back to repository, `width` wide, while it runs and the preview's row
    /// does not say it ([`Self::row_says`]): the review covers the operation
    /// bar, so once the person chose another file or commit, or went back to
    /// the series, nothing on screen said a download ran or offered its
    /// Cancel. The footer is the dialog's persistent chrome: at the top of
    /// the scrolling body, at 18 pt in a 461 × 490 window, the line scrolled
    /// away with it, overflowed a body the series list's rows do not count,
    /// and moved the content by about 50 px as it came and went (decision,
    /// 2026-11-10). Its text keeps a room as the row's does, the narrower
    /// of its name with "· Cancelling…", measured once for the download's
    /// object, and what the line leaves beside Cancel, which stands right
    /// after it, so neither a press nor a progress update moves Cancel,
    /// and longer words end in an ellipsis inside it
    /// (`lfs_download::footer_progress_room`); the text gives way first,
    /// then Cancel's word, then its label for its icon
    /// (`lfs_download::line_cancel_form`). Cancel tracks
    /// the row's handle, so focus on it stays as it moves between the two.
    fn footer_line(
        &self,
        view: WeakEntity<Self>,
        width: Pixels,
        window: &Window,
        cx: &App,
    ) -> Option<AnyElement> {
        if let Some((ended, _)) = &self.ended_line {
            // A success after a check; the person's own Cancel in words
            // alone where the progress started, in the text's own colour.
            let (id, said, check) = match ended {
                EndedLine::Downloaded(said) => ("series-download-done", said, true),
                EndedLine::Cancelled(said) => ("series-download-cancelled", said, false),
            };
            let words = native_accessibility::text(id, said.clone())
                .min_w_0()
                .truncate()
                .text_size(appearance::ui_text(12.));
            #[cfg(test)]
            let words = words.child(
                gpui::canvas(
                    |_, window, _| ENDED_LINE_COLOR.set(Some(window.text_style().color)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_0(),
            );
            return Some(
                div()
                    .debug_selector(move || id.into())
                    .size_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(check, |line| {
                        line.child(div().flex_none().child(crate::control_icon("check")))
                    })
                    .child(words)
                    .into_any_element(),
            );
        }
        let asked = self
            .lfs_download
            .as_ref()
            .filter(|asked| !asked.seen.get())?;
        if self.row_says(asked) {
            return None;
        }
        if let Some(words) = &asked.failure {
            // Kept until the person returns to its file, where the row says
            // it ([`Self::clear_content`]).
            let said: SharedString = failed_sentence(&asked.object(), words).into();
            let whole = said.clone();
            return Some(
                div()
                    .debug_selector(|| "series-download-status".into())
                    .size_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .child(
                        native_accessibility::text("series-download-failure", said)
                            .debug_selector(|| "series-download-failure".into())
                            .min_w_0()
                            .truncate()
                            .text_size(appearance::ui_text(12.))
                            .text_color(rgb(appearance::labels(cx).warning))
                            .tooltip(move |window, cx| {
                                Tooltip::new(whole.clone()).build(window, cx)
                            }),
                    )
                    .into_any_element(),
            );
        }
        let owner = self.owner.upgrade()?;
        let owner = owner.read(cx);
        if owner.lfs_downloads.running()
            != Some(crate::lfs_download::Asker::Review(view.entity_id()))
        {
            return None;
        }
        owner.operation_shown()?;
        // It names what downloads, apart from the file shown: the
        // window's progress said only "Download object". What Git LFS
        // reports, or that the write is cancelling, follows.
        let name = format!("Downloading {}", asked.object());
        let progress = owner.operation_progress().map_or_else(
            || name.clone().into(),
            |progress| SharedString::from(format!("{name} · {progress}")),
        );
        let running = asked.cancel(owner);
        let object = asked.object();
        let form = crate::lfs_download::line_cancel_form(LINE_LEAST, width, window);
        let room = crate::lfs_download::footer_progress_room(
            &name,
            &asked.footer_widest,
            width,
            form,
            window,
        );
        let line = div()
            .id("series-download-line")
            .role(Role::Group)
            .aria_label(name);
        // Tests assemble the window's tree from what they record.
        #[cfg(test)]
        crate::native_accessibility::written::built(&line);
        Some(
            line.debug_selector(|| "series-download-status".into())
                .size_full()
                .min_w_0()
                .flex()
                .items_center()
                .child(crate::lfs_download::running_form(
                    progress,
                    room,
                    (
                        &self.download_cancel,
                        std::rc::Rc::new(move |window, cx| {
                            Self::cancel_download(&view, &object, window, cx)
                        }),
                    ),
                    running,
                    form,
                    window,
                    cx,
                ))
                .into_any_element(),
        )
    }
    /// Cancel download's press, in the row or on the dialog's footer, its
    /// Cancel naming `object`: it cancels the window's write only while that
    /// is still the download this review asked for, of that object, and
    /// otherwise does nothing, so a press on a Cancel drawn before its
    /// download ended, as another review's or Compare's runs, cancels none
    /// of theirs. It cancelled whatever write ran.
    fn cancel_download(view: &WeakEntity<Self>, object: &str, window: &mut Window, cx: &mut App) {
        let Some(view) = view.upgrade() else {
            return;
        };
        let me = crate::lfs_download::Asker::Review(view.entity_id());
        let browser = view.read(cx);
        let named = browser
            .lfs_download
            .as_ref()
            .is_some_and(|asked| asked.failure.is_none() && asked.object() == object);
        let Some(owner) = browser.owner.upgrade().filter(|_| named) else {
            return;
        };
        if owner.read(cx).lfs_downloads.running() == Some(me) {
            owner.update(cx, |owner, cx| owner.cancel_operation(window, cx));
        }
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
            // A download it asked for that still runs goes to Compare,
            // whose preview its end reads again where Compare shows its
            // object.
            let me = crate::lfs_download::Asker::Review(cx.entity_id());
            owner.update(cx, |owner, _| owner.lfs_downloads.left(me));
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
        // A failure is said of the file it was asked for, while it shows:
        // once the row has said it, leaving that file drops it, and one that
        // landed while the review showed another file or commit waits for
        // the person to come back to its file.
        if self
            .lfs_download
            .as_ref()
            .is_some_and(|asked| asked.failure.is_some() && asked.seen.get())
        {
            self.lfs_download = None;
        }
        self.failure_scroll = ScrollHandle::new();
        self.files_error = None;
        self.files_failure_scroll = ScrollHandle::new();
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
    #[cfg_attr(not(test), expect(dead_code))]
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
            None => match self.preview_error.as_ref().or(self.files_error.as_ref()) {
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
            Some(Content::Text {
                old,
                new,
                lfs_missing,
                ..
            }) => Some(self.no_diagrams(old, new, *lfs_missing).into()),
            Some(Content::Images { .. } | Content::Rich(_)) => Some(NOT_DIAGRAMS.into()),
            _ => self.modes_refusal(),
        }
    }
    /// Why the chosen file's text, `old` and `new`, has no diagrams: a
    /// Mermaid file stored as a link, or whose Git LFS object the local
    /// store lacks, says so, since what it stands for could hold diagrams,
    /// and only one whose text was read says it has none. "This file has no
    /// Mermaid diagrams." once stood for every text file without them. Only
    /// a file with no side left to draw comes here: a link or a pointer on
    /// one side stays literal and the other draws ([`Literal`]). It judges
    /// the side after the change or, for a file the change removed, the one
    /// before: judged by either side, a Mermaid file changed from a link to
    /// a blank regular file read as stored as a link. A pointer is called
    /// missing only if the reading found its object absent (`lfs_missing`,
    /// old then new): one that is local but unreadable as text says that.
    ///
    /// [`Literal`]: crate::rich_preview::Literal
    fn no_diagrams(&self, old: &str, new: &str, lfs_missing: [bool; 2]) -> &'static str {
        let Some(file) = self.selected_file.and_then(|index| self.files.get(index)) else {
            return NOT_DIAGRAMS;
        };
        let (path, mode, text, missing) = if file.new_path.is_some() {
            (
                file.new_path.as_deref(),
                file.new_mode.as_str(),
                new,
                lfs_missing[1],
            )
        } else {
            (
                file.old_path.as_deref(),
                file.old_mode.as_str(),
                old,
                lfs_missing[0],
            )
        };
        let mermaid = path
            .and_then(|path| path.extension())
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("mmd") || extension.eq_ignore_ascii_case("mermaid")
            });
        if !mermaid {
            return NOT_DIAGRAMS;
        }
        match crate::rich_preview::Literal::of(mode, text) {
            Some(crate::rich_preview::Literal::Link) => LINKED,
            Some(crate::rich_preview::Literal::LfsPointer) if missing => NO_LFS_OBJECT,
            Some(crate::rich_preview::Literal::LfsPointer) => NO_LFS_TEXT,
            None => NO_DIAGRAMS,
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
        self.inspected = Some(oid.clone());
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
            let _ = this.update_in(cx, |this, window, cx| {
                if this.generation != generation || !this.current(cx) || !this.inspecting { return; }
                this.pending = false; this.task = None;
                let failed = match result {
                    Ok(Ok(Output::Changes(files, _))) => {
                        this.files = files;
                        this.files_scroll.scroll_to_item(0, ScrollStrategy::Top);
                        None
                    }
                    Ok(Err(error)) => Some(format!("{error:#}")),
                    _ => Some("Changed-file read was interrupted. Return to the series and select the commit again.".to_owned()),
                };
                // Said in the file box, where the read was asked for, and at
                // once to assistive technology, since focus stays on Back.
                if let Some(error) = failed {
                    let _ = crate::copy_feedback::alert(
                        window,
                        cx,
                        native_accessibility::bounded_accessible_text(format!(
                            "{FILES_UNAVAILABLE}: {error}"
                        )),
                    );
                    this.files_error = Some(error);
                }
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
        self.inspected = None;
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
            prepare_split: false,
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

    /// Scrolls a failure in its box a line, `down` or up, as far as its ends.
    fn scroll_failure(scroll: &ScrollHandle, down: bool, window: &Window, cx: &mut Context<Self>) {
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
        window: &mut Window,
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
        // Diagrams says it in the failing side's own place.
        let failure = match self.content.as_deref() {
            Some(Content::Text { lfs_failed, .. }) if mode != 3 => {
                crate::lfs_download::side_failures_line(*lfs_failed, ["Before", "After"])
            }
            _ => None,
        };
        let body = match self.content.as_deref() {
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
        };
        let owner = self.owner.upgrade();
        let ring = owner
            .as_ref()
            .map(|owner| owner.read(cx).focus_ring.clone());
        // A side whose object is missing offers the download it can make,
        // into the reviewed repository, as Compare does: the review said
        // the object was missing and offered nothing.
        let targets = match (
            self.content.as_deref(),
            self.selected_file.and_then(|index| self.files.get(index)),
        ) {
            (Some(content), Some(file)) => crate::lfs_download::preview_targets(content, file),
            _ => Vec::new(),
        };
        let me = crate::lfs_download::Asker::Review(cx.entity_id());
        // The row gives way in Compare's order (`views::DownloadRegion`),
        // which has no footer here: the line first, to its one line, then
        // the buttons' word "Download", wherever its going lets two buttons
        // that stack with it share a line, and the buttons stack last, so
        // the comparison keeps a whole row in the box. With both sides'
        // objects missing at 18 pt in a 461 × 490 window, the two buttons
        // stacked with their word and cut the comparison's row by 1.5 px.
        // Measured at rest as the review draws it, saying what it asked for,
        // which keeps its lines in every state (`download_row`): Cancel
        // download's word gives way with the buttons', the progress and a
        // failure's words first, so two sides that share a line without
        // their word share it running and failed too.
        let short = !targets.is_empty() && {
            let line = if failure.is_some() {
                crate::empty_state::block_height(1, false, window) + appearance::ui_size(16.)
            } else {
                px(0.)
            };
            let [full, without] = [false, true].map(|short| {
                let row = crate::lfs_download::download_row(
                    targets.clone(),
                    false,
                    failure.is_some(),
                    short,
                    width,
                    (false, false),
                    Some(crate::lfs_download::Asked {
                        focus: self.download_focus.clone(),
                        cancel: self.download_cancel.clone(),
                        state: crate::lfs_download::AskedState::Resting,
                        on_cancel: std::rc::Rc::new(|_, _| {}),
                    }),
                    |_, _, _, _| {},
                    window,
                    cx,
                );
                crate::in_frame::measured_height(row, width, window, cx)
            });
            let device = px(1. / window.scale_factor());
            without < full
                && height - line - full < crate::views::comparison_floor(1., window) - device / 2.
        };
        let row = owner.filter(|_| !targets.is_empty()).map(|owner| {
            let owner = owner.read(cx);
            let busy = (
                owner.operation_busy.is_some(),
                owner.operation_shown().is_some(),
            );
            // It covers the operation bar, so its own row says what its
            // download does there.
            use crate::lfs_download::AskedState;
            let state = match self
                .lfs_download
                .as_ref()
                .filter(|asked| self.shows(asked).is_some())
            {
                Some(ReviewDownload {
                    failure: Some(words),
                    seen,
                    side,
                    ..
                }) => {
                    seen.set(true);
                    AskedState::Failed {
                        side: *side,
                        words: words.clone(),
                    }
                }
                Some(asked) if owner.lfs_downloads.running() == Some(me) => AskedState::Running {
                    side: asked.side,
                    progress: owner.operation_shown().map(|label| {
                        owner
                            .operation_progress()
                            .map_or_else(|| label.into(), SharedString::from)
                    }),
                    cancel: asked.cancel(owner),
                },
                _ => AskedState::Resting,
            };
            let (view, object) = (
                cx.entity().downgrade(),
                self.lfs_download
                    .as_ref()
                    .map(ReviewDownload::object)
                    .unwrap_or_default(),
            );
            let asked = crate::lfs_download::Asked {
                focus: self.download_focus.clone(),
                cancel: self.download_cancel.clone(),
                state,
                on_cancel: std::rc::Rc::new(move |window, cx| {
                    Self::cancel_download(&view, &object, window, cx)
                }),
            };
            let (view, owner, chosen) = (
                cx.entity().downgrade(),
                self.owner.clone(),
                (
                    self.selected_file
                        .and_then(|index| self.files.get(index))
                        .cloned(),
                    self.inspected.clone(),
                ),
            );
            crate::lfs_download::download_row(
                targets,
                false,
                failure.is_some(),
                short,
                width,
                busy,
                Some(asked),
                move |side, target, window, cx| {
                    // As Publish does: the form takes the window's
                    // repository, so a review the window has moved on from
                    // asks for nothing.
                    if !view.upgrade().is_some_and(|view| view.read(cx).current(cx)) {
                        return;
                    }
                    // Recorded only once the window accepts it and a form
                    // opens, never over one in flight: a press refused while
                    // the download ran once recorded a fresh request over
                    // it, whose end then read nothing again. A failure the
                    // row says clears as a Download is pressed again; one
                    // kept for its file's return stays until the new write
                    // starts ([`Self::follow_download`]).
                    if !GitTurtle::open_lfs_download_from(&owner, target, me, window, cx) {
                        return;
                    }
                    let _ = view.update(cx, |this, _| {
                        if this
                            .lfs_download
                            .as_ref()
                            .is_some_and(|asked| asked.failure.is_none())
                        {
                            return;
                        }
                        if this
                            .lfs_download
                            .as_ref()
                            .is_some_and(|kept| kept.seen.get())
                        {
                            this.lfs_download = None;
                        }
                        let (file, commit) = chosen.clone();
                        this.ended_line = None;
                        this.asking = file.map(|file| ReviewDownload {
                            file,
                            commit,
                            side,
                            failure: None,
                            cancelled: false,
                            seen: Default::default(),
                            footer_widest: Default::default(),
                        });
                    });
                },
                window,
                cx,
            )
        });
        crate::lfs_download::above_comparison_with_row(
            &self.lfs_failure_line,
            crate::lfs_download::comparison_key(
                self.selected_file.and_then(|index| self.files.get(index)),
            ),
            failure,
            row,
            body,
            ring.as_ref(),
            window,
            cx,
        )
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
            let box_ = FailureBox {
                id: "series-preview-failure",
                scrollbar: "series-preview-failure-scrollbar",
                label: PREVIEW_UNAVAILABLE,
                scroll: &self.failure_scroll,
                rest: &self.failure_scroll_rest,
                focus: &self.failure_focus,
            };
            return Self::failure_box(state, box_, window, cx)
                .size_full()
                .into_any_element();
        }
        let line = if self.files_error.is_some() {
            NO_FILE_TO_PREVIEW
        } else if self.files.is_empty() && !self.pending {
            NO_FILES
        } else {
            CHOOSE_FILE
        };
        crate::empty_state::EmptyState::new("series-preview-empty", line).pane(cx)
    }
    /// `failure` in a box it fills, scrolling inside it where it is taller,
    /// cued by its scrollbar: a Tab stop while it overflows, which Up and
    /// Down scroll, and otherwise no stop ([`Self::preview_prompt`]). Its
    /// host sizes the element.
    fn failure_box(
        failure: impl IntoElement,
        region: FailureBox<'_>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> crate::tags::CuedList {
        let FailureBox {
            id,
            scrollbar,
            label,
            scroll,
            rest,
            focus,
        } = region;
        let cue = crate::tags::ScrollCue::new(scroll, Pixels::ZERO);
        let overflows = cue.kept();
        let tracked = overflows || focus.is_focused(window);
        let keys = scroll.clone();
        let observe_scroll = scroll.clone();
        let observe_rest = rest.clone();
        cue.list(
            div()
                .on_children_prepainted(cue.observe(move |_, _, _| {
                    observe_rest.set(observe_scroll.offset().y);
                }))
                .id(id)
                .size_full()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .on_scroll(crate::tags::chain_scroll_wheel(
                    scroll.clone(),
                    rest.clone(),
                ))
                .when(tracked, |element| {
                    element
                        // The handle carries the Tab stop: GPUI ignores the
                        // `tab_stop` of the div that tracks it.
                        .track_focus(&focus.clone().tab_stop(overflows))
                        .role(Role::Group)
                        .aria_label(label)
                        .on_key_down(cx.listener(move |_, event: &KeyDownEvent, window, cx| {
                            let down = match event.keystroke.key.as_str() {
                                "up" => false,
                                "down" => true,
                                _ => return,
                            };
                            Self::scroll_failure(&keys, down, window, cx);
                            cx.stop_propagation();
                        }))
                })
                .child(failure),
            scrollbar,
        )
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
            .aria_position_in_set(index)
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
        // The file box rings its own edge the same way while a failure too
        // long for it holds focus.
        let files_ringed = self.owner.upgrade().is_some_and(|owner| {
            self.files_error.is_some() && owner.read(cx).focus_ring.shows(&self.files_failure_focus)
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
                .child(div().flex().flex_col().flex_shrink_0().on_children_prepainted(self.body.control(self.focus.clone(), room)).child(div().when(row_count > 0, |list| {
                    let (scroll, rest) = (self.rows_scroll.clone(), self.rows_scroll_rest.clone());
                    list.on_children_prepainted(rows_cue.observe(move |_, _, _| {
                        rest.set(scroll.0.borrow().base_handle.offset().y);
                    }))
                }).id("series-focus-list").debug_selector(|| "series-focus-list".into()).w_full().min_w_0().map(|list| if empty.is_some() { list.min_h(list_height) } else { list.h(list_height) }).flex_shrink_0().border_1().border_color(rgb(if ringed { p.accent } else { p.border })).rounded(px(6.)).overflow_hidden().track_focus(&self.focus).key_context(SERIES_CONTEXT).role(if empty.is_some() { Role::Group } else { Role::ListBox }).when(empty.is_none(), |list| list.aria_size_of_set(row_count)).aria_label("Original and rewritten commits").aria_description(empty.as_ref().map_or_else(series_keys, |empty| empty.description.clone()))
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
                        None => list.flex().flex_col().child(uniform_list("rewrite-series-list", row_count, cx.processor(|this, range: std::ops::Range<usize>, window, cx| range.map(|index| this.row(index, window, cx)).collect::<Vec<_>>())).flex_1().min_h_0().mr(-gutter).pr(gutter).track_scroll(&self.rows_scroll).on_scroll(crate::tags::chain_uniform_list_wheel(self.rows_scroll.clone(), self.rows_scroll_rest.clone()))),
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
                // height (a failure may grow it, within its bound): said
                // after the preview, the read lay below the view at 18 pt
                // in a 461 × 490 window.
                .when(self.messages.is_none(), |element| {
                    let files_control = self.body.control(files_focus.clone(), room);
                    let (files_scroll, files_rest) = (self.files_scroll.clone(), self.files_scroll_rest.clone());
                    element.child(div().debug_selector(|| "series-files".into()).track_focus(&files_focus).on_any_mouse_down(|_, window, _| window.prevent_default()).on_children_prepainted(move |bounds, window, cx| {
                        files_rest.set(files_scroll.0.borrow().base_handle.offset().y);
                        files_control(bounds, window, cx);
                    }).map(|files| if self.files_error.is_some() { files.flex().flex_col().min_h(px(130.)).max_h(px(FILES_FAILURE_MOST)) } else { files.h(px(130.)) }).flex_shrink_0().border_1().border_color(rgb(if files_ringed { p.accent } else { p.border })).rounded(px(6.)).overflow_hidden().map(|files| if self.reading_files() { files.child(div().px_3().py_2().child(crate::empty_state::EmptyState::new("series-files-reading", READING_FILES).block(cx))) } else if let Some(error) = &self.files_error {
                    // Said in the box whose read failed, at the rows' inset,
                    // the box growing within `FILES_FAILURE_MOST` to hold it,
                    // scrolling past that: drawn at the body's end, the
                    // failure lay below the view.
                    let failure = div().px_3().py_2().child(crate::empty_state::EmptyState::new("series-files-failed", error.clone()).titled(Some(FILES_UNAVAILABLE)).block(cx));
                    let region = FailureBox { id: "series-files-failure", scrollbar: "series-files-failure-scrollbar", label: FILES_UNAVAILABLE, scroll: &self.files_failure_scroll, rest: &self.files_failure_scroll_rest, focus: &self.files_failure_focus };
                    files.child(Self::failure_box(failure, region, window, cx).flex_1().min_h_0().w_full())
                } else { files.child(uniform_list("series-files", self.files.len(), cx.processor(|this, range: std::ops::Range<usize>, window, cx| range.map(|index| {
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
                }).collect::<Vec<_>>())).size_full().p(room).track_scroll(&self.files_scroll).on_scroll(crate::tags::chain_uniform_list_wheel(self.files_scroll.clone(), self.files_scroll_rest.clone()))) }))
                })
                // The text modes keep their row for every preview, waiting
                // for one with text and saying why, so the preview below
                // keeps its place and size as its read comes and goes and
                // from one file to the next: the row left as an image or
                // document landed and came back with the next choice,
                // moving the preview 34.5 px each way.
                .when(self.messages.is_none(), |element| element.child(div().flex().flex_wrap().gap_2().on_children_prepainted(self.body.controls(modes.iter().cloned().map(Some).collect(), room)).children(TEXT_MODES.into_iter().enumerate().map(|(mode, (label, key))| { let refused = if mode == 3 { diagrams_refused.clone() } else { modes_refused.clone() }; let active = drawn_mode == mode && refused.is_none(); refusing(button(("series-text-mode", mode), label, "", active).toggled(active).debug_selector(move || key.into()).track_focus(&modes[mode]).on_click(cx.listener(move |this, _, window, cx| { this.text_mode = mode; this.prepare_editor(window, cx); cx.notify(); })), key, refused) }))))
                // Built for the box's width in the frame it is drawn
                // (`in_frame`), so the open controls take the form that
                // width leaves them in the first frame after a resize.
                .child(div().debug_selector(|| "series-file-preview".into()).track_focus(&preview_focus).on_any_mouse_down(|_, window, _| window.prevent_default()).on_children_prepainted(self.body.control(preview_focus.clone(), room)).flex_1().min_h(appearance::ui_size(100.)).flex().flex_col().border_1().border_color(rgb(if text_ringed { p.accent } else { p.border })).overflow_hidden().child(crate::in_frame::in_frame(move |bounds, window, cx| series.update(cx, |this, cx| { this.preview_bounds.set(Some(bounds)); this.render_content(bounds.size.width, bounds.size.height, window, cx) }).unwrap_or_else(|_| div().into_any_element())).named("rewrite-comparison"))))
            // A failed publication check or push, below the series it
            // concerns; the load's own error says why in the list's place.
            // It keeps its height in the scrolling body, which shrank a
            // scrolling box with no least height of its own to nothing.
            .when_some(self.error.as_ref().filter(|_| self.review.is_some()), |element, error| {
                use native_accessibility::LiveRegion as _;
                element.child(native_accessibility::clips_children(div().max_h(appearance::ui_size(100.)).flex_shrink_0().id("series-review-error").alert(error.clone()).overflow_y_scroll().track_scroll(&self.error_scroll.handle).on_prepaint(self.error_scroll.observe()).on_scroll(self.error_scroll.wheel()).text_size(appearance::ui_text(12.)).text_color(rgb(p.warning)).child(error.clone())))
            }), "rewrite-review-scrollbar").min_h_0())
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
#[path = "rewrite_review_tests.rs"]
mod tests;
