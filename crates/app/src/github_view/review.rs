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
    // Enter on a focused Button is the Button's own: bound to no action here, the dialog's
    // default Confirm never takes the key, and the Button acts as the key goes down.
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
    // The outbound confirmation's region scrolls its destination and text by line, by page
    // and to either end while it holds focus ([`Panel::scroll_confirmation`]).
    let context = Some(CONFIRMATION);
    cx.bind_keys([
        KeyBinding::new("up", ReviewUp, context),
        KeyBinding::new("down", ReviewDown, context),
        KeyBinding::new("home", ReviewHome, context),
        KeyBinding::new("end", ReviewEnd, context),
        KeyBinding::new("pageup", ReviewPageUp, context),
        KeyBinding::new("pagedown", ReviewPageDown, context),
    ]);
}
/// The key context of the outbound confirmation's scrolling region, the confirmation's stop.
pub(super) const CONFIRMATION: &str = "GitTurtleGithubConfirmation";
#[derive(Default, PartialEq)]
pub(super) struct FocusObservation {
    pub focus: Option<FocusHandle>,
    pub viewport: Bounds<Pixels>,
    pub rem_size: Pixels,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Section {
    Overview,
    Files,
    Review,
}
/// The focus handles of the panel's body Buttons that keep no handle of the panel's own, by
/// their element ids ([`Panel::button`]): kept by the panel, they outlive the closed dialog's
/// frame, so reopening gives focus back to the Button that held it ([`Panel::focus_reopened`]).
/// A Button keeps its handle in its own element state otherwise, which goes with that frame,
/// and reopening onto Comment on selection or Open captured local comparison took the
/// section's first stop instead. Each frame keeps the handles the frame before handed out and
/// the one holding focus, so none outlives the Button it was handed to by more than a frame.
#[derive(Default)]
pub(super) struct Buttons(
    std::cell::RefCell<std::collections::HashMap<ElementId, (FocusHandle, bool)>>,
);
impl Buttons {
    /// The handle of the Button `id` this frame draws.
    pub(super) fn focus(&self, id: impl Into<ElementId>, cx: &App) -> FocusHandle {
        let mut handles = self.0.borrow_mut();
        let (focus, handed) = handles
            .entry(id.into())
            .or_insert_with(|| (cx.focus_handle(), true));
        *handed = true;
        focus.clone()
    }
    /// The handle the Button `id` was last drawn with, if the panel keeps one.
    pub(super) fn kept(&self, id: impl Into<ElementId>) -> Option<FocusHandle> {
        self.0
            .borrow()
            .get(&id.into())
            .map(|(focus, _)| focus.clone())
    }
    /// As a frame of the panel starts: gives up the handles the last frame handed out to no
    /// Button, except the one holding focus.
    pub(super) fn begin_frame(&self, window: &Window) {
        self.0
            .borrow_mut()
            .retain(|_, (focus, handed)| std::mem::take(handed) || focus.is_focused(window));
    }
}
pub(super) struct ReviewState {
    pub section: Section,
    pub body_scroll: ScrollHandle,
    pub revealed_focus: std::rc::Rc<std::cell::RefCell<FocusObservation>>,
    /// Where Overview's captured identities and the last section tab, which owns them, lie
    /// in the frame being drawn, and whether they fit the body together ([`Owned`]).
    pub identities: Owned,
    /// Overview's captured identities as a group stop of their own, revealed from their top,
    /// where they cannot fit the body with the last section tab that owns them
    /// ([`Owned::apart`]): its ring on their edge, as the connection pane's is. The block
    /// tracks it only there, and while it holds focus, so a click where they fit focuses
    /// nothing no Tab reaches, and a resize that makes them fit moves no focus.
    pub identities_focus: FocusHandle,
    /// Where the body's blocks start, for its reveals.
    pub body_blocks: Blocks,
    /// The PR list's own scroll, the focus its reveals last showed and where its blocks start:
    /// a PR row's title is revealed in the list first, then where it lands in the body
    /// ([`Panel::pull_part`]).
    pub pull_scroll: ScrollHandle,
    pub pull_revealed: std::rc::Rc<std::cell::RefCell<FocusObservation>>,
    pub pull_blocks: Blocks,
    /// A PR has just opened: the next frame shows it from its title ([`Panel::reveal_opened`]).
    pub opened: std::rc::Rc<std::cell::Cell<bool>>,
    /// The room the panel's top line took in the last frame: how tall it stood above the body
    /// while a conversation is open (`Panel::pin_top_line`), or the room it took among the
    /// content while none is (`Panel::flow_top_line`). The body's scroll moves with each change
    /// of it, so the body's content stays where it was drawn: while a conversation is open,
    /// and in the frame the line moves back among the content as the conversation closes.
    pub top_line: std::rc::Rc<std::cell::Cell<Pixels>>,
    /// Whether the last frame stood the top line above the body ([`Self::top_line`]).
    pub top_line_pinned: std::cell::Cell<bool>,
    /// The scroll of the top line's text while it stands above the body, which its scrollbar
    /// cues where the text is taller than its room.
    pub top_line_scroll: ScrollHandle,
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
    /// What Files measured in the last layout to fit the body ([`Panel::files_fit`]).
    files_fit: std::rc::Rc<std::cell::Cell<FilesFit>>,
    /// The selected file's patch presentation, prepared with its rows on the read worker
    /// within Compare's text bounds; `None` while preparing and above them.
    presentation: Option<text::PatchPresentation>,
    _palette: Subscription,
    pub mode: usize,
    pub file_focus: FocusHandle,
    pub line_focus: FocusHandle,
    pub section_focus: FocusHandle,
    /// The connection pane's own stop, its ring on its edge as the conversations pane's
    /// is: Tab onto it shows the pane from its top, its "Local worktree:" line and its help,
    /// which the reveals of its controls cannot show with every line whole in a short body.
    pub connection_focus: FocusHandle,
    /// Account options, which opens and closes the connection pane: it takes focus the pane
    /// held as the pane goes ([`Panel::closing`]).
    pub account_focus: FocusHandle,
    /// Files' modes, Changes, Source and Threads: the one a click moves to takes focus
    /// Threads' Conversations pane held as it goes.
    pub mode_focus: [FocusHandle; 3],
    /// New pull request, which takes focus a PR's Conversations pane held as it goes.
    pub create_focus: FocusHandle,
    /// Files' Previous and Next page, which take focus Threads' Conversations pane held as the
    /// page they load replaces it.
    pub file_page_focus: [FocusHandle; 2],
    /// The control whose press takes a pane away, or whose request's answer does as it
    /// arrives, as a page of files replaces Threads, since the window last drew
    /// ([`Panel::closing`]): the next frame takes it ([`ReviewState::closing`]), so a press
    /// that took nothing away leaves nothing for a later frame.
    pub closer: Option<FocusHandle>,
    /// The closer of the frame the window last drew ([`ReviewState::closer`]), which the
    /// panel's guard gives focus a pane held as that frame took the pane away
    /// ([`Panel::hand_off_lost_pane_focus`]), and which the next frame replaces.
    pub closing: Option<FocusHandle>,
    /// The controls that can take a pane away which the last frame drew enabled, so tracking
    /// their focus ([`Panel::closing`]): a disabled Button tracks none, and focus given to
    /// one is drawn nowhere.
    pub closers_drawn: std::cell::RefCell<Vec<FocusHandle>>,
    /// The focus that a pane which can go with it, the Conversations pane, the connection
    /// pane or Overview's captured identities where they are a stop of their own, held in the
    /// frame the window last drew, on the pane itself or on a control
    /// inside it, as Tab after a click on the pane puts it there: recorded while the panel
    /// renders, so the guard knows the focus it finds on nothing drawn went with a pane
    /// ([`Panel::hand_off_lost_pane_focus`]).
    pub pane_held: Option<FocusHandle>,
    /// The control that held focus inside the body's view as the panel last closed, which
    /// reopening gives focus back to where a frame still draws it there
    /// ([`Panel::focus_reopened`]). Held weakly: a control's handle lives as long as its owner
    /// keeps it, the panel's for its body's Buttons ([`Buttons`]) and its fields', and a
    /// handle no owner keeps any more is no control to give focus back to.
    pub held: Option<WeakFocusHandle>,
    /// The innermost part of the body holding [`ReviewState::held`] as the panel last closed,
    /// by its key ([`PART`], or [`PULL`] for a PR row) and name, which the frame that first
    /// draws the reopened panel reveals ([`Reopening::hold`]): its place is that frame's, never
    /// one recorded at close.
    pub held_part: Option<(&'static str, SharedString)>,
    /// How the reopened panel settles its focus ([`Reopening`]).
    pub reopening: std::rc::Rc<Reopening>,
    /// The focus handles of the body's Buttons ([`Buttons`]).
    pub buttons: Buttons,
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
            identities: Default::default(),
            body_blocks: Blocks::default(),
            pull_scroll: ScrollHandle::new(),
            pull_revealed: Default::default(),
            pull_blocks: Blocks::default(),
            opened: Default::default(),
            top_line: Default::default(),
            top_line_pinned: Default::default(),
            top_line_scroll: ScrollHandle::new(),
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
            files_fit: Default::default(),
            presentation: None,
            // Compare's patch is redecorated where the app applies a palette; this panel
            // follows the applied palette itself.
            _palette: cx.observe_global::<crate::appearance::Palette>(|panel, cx| {
                panel.review.refresh_theme(cx)
            }),
            mode: 0,
            file_focus: cx.focus_handle().tab_stop(true),
            line_focus: cx.focus_handle().tab_stop(true),
            identities_focus: cx.focus_handle().tab_stop(true),
            section_focus: cx.focus_handle().tab_stop(true),
            connection_focus: cx.focus_handle().tab_stop(true),
            account_focus: cx.focus_handle().tab_stop(true),
            mode_focus: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
            create_focus: cx.focus_handle().tab_stop(true),
            file_page_focus: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
            closer: None,
            closing: None,
            closers_drawn: Default::default(),
            pane_held: None,
            held: None,
            held_part: None,
            reopening: Default::default(),
            buttons: Default::default(),
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
/// The key context entry that names a part of the panel's body ([`Panel::part`]).
pub(super) const PART: &str = "GitTurtleGithubPart";
/// The key context entry that names a part of the conversation list, a conversation's
/// actions ([`Panel::conversation_part`]).
const CONVERSATION: &str = "GitTurtleGithubConversation";
/// The key context entry that names a part of the PR list, a row's title
/// ([`Panel::pull_part`]).
pub(super) const PULL: &str = "GitTurtleGithubPull";
/// The key context of the panel's scrolling body, on the focus path of whatever it draws.
pub(super) const BODY: &str = "GitTurtleGithubBody";

#[cfg(test)]
thread_local! {
    /// The revealing parts the last frame drew, each with the focus it rings on its own edge,
    /// if any: the reveal sweep bounds a part that holds others by the focused control alone
    /// (`github_view::tests::over_wide`). The panel's render starts it afresh.
    pub(super) static PARTS_DRAWN: std::cell::RefCell<Vec<(SharedString, Option<FocusHandle>)>> =
        Default::default();
    /// While a test watches them (`Some`), the frames of the panel's body, each with its view
    /// and where each of its parts lay, by name, as it prepainted ([`record_body_frame`]).
    pub(super) static BODY_FRAMES: std::cell::RefCell<Option<Vec<BodyFrame>>> =
        Default::default();
    /// The parts of the body the frame being drawn has prepainted so far, by name.
    static PART_BOUNDS: std::cell::RefCell<Vec<(SharedString, Bounds<Pixels>)>> =
        Default::default();
}
/// A frame of the panel's body ([`BODY_FRAMES`]).
#[cfg(test)]
#[derive(Debug)]
pub(super) struct BodyFrame {
    pub view: Bounds<Pixels>,
    pub parts: Vec<(SharedString, Bounds<Pixels>)>,
    /// The conversation list's view, where the frame draws the list.
    pub list: Option<Bounds<Pixels>>,
    /// The PR list's view as the frame drew it, stale where the frame draws no PR list.
    pub pulls: Bounds<Pixels>,
    /// What held the window's focus as the frame drew.
    pub focused: Option<FocusHandle>,
}
/// For the body's `on_children_prepainted`: ends the frame of the body `scroll` scrolls, with
/// the conversation list `list` scrolls, recording it where a test watches ([`BODY_FRAMES`]).
#[cfg(test)]
pub(super) fn record_body_frame(
    scroll: &ScrollHandle,
    list: &ScrollHandle,
    pulls: &ScrollHandle,
    window: &Window,
    cx: &App,
) {
    let focused = window.focused(cx);
    let parts = PART_BOUNDS.take();
    // The list's handle holds this frame's view only where the frame drew the list.
    let list = parts
        .iter()
        .any(|(name, _)| name == "conversation-list")
        .then(|| list.bounds());
    BODY_FRAMES.with_borrow_mut(|frames| {
        if let Some(frames) = frames {
            frames.push(BodyFrame {
                view: scroll.bounds(),
                parts,
                list,
                pulls: pulls.bounds(),
                focused,
            });
        }
    });
}

#[cfg(test)]
thread_local! {
    /// The text the draft status's tooltip last showed, for a test to read back
    /// ([`Panel::render_save_status`]).
    pub(super) static SAVE_STATUS_TOOLTIP: std::cell::RefCell<Option<SharedString>> =
        const { std::cell::RefCell::new(None) };
}

/// For the `on_children_prepainted` of `scroll`'s box, the panel's body or the conversation
/// list: records in `revealed` the focus the frame being prepainted drew, which its reveals
/// have shown, once the frame before it drew that focus inside the box too, which `inside`
/// tells from a key context on its focus path. Focus that frame did not draw stays unrecorded,
/// so the next frame, whose parts find it on their focus path, reveals it: reopening the panel
/// restores focus to a pane no frame had drawn, and recording it then left the pane where the
/// body last scrolled, its ring's top edge above the clip. Once focus has moved to a control
/// outside the box, the box forgets the focus it recorded, so a control focus comes back to is
/// revealed again
/// wherever the box has scrolled since (coordinator's decision, 2026-11-05): Shift+Tab from
/// the conversations' pager back onto a conversation's last action, after the list had
/// scrolled back to its top, and from Back to repository back onto the body's last control,
/// after the body had scrolled away from it, left the control out of view and its ring
/// undrawn.
pub(super) fn record_shown(
    revealed: &std::cell::RefCell<FocusObservation>,
    scroll: &ScrollHandle,
    inside: impl Fn(&KeyContext) -> bool,
    window: &Window,
    cx: &App,
) {
    if window.context_stack().iter().any(inside) {
        *revealed.borrow_mut() = FocusObservation {
            focus: window.focused(cx),
            viewport: scroll.bounds(),
            rem_size: window.rem_size(),
        };
    } else {
        // Focus the box recorded, which no frame has drawn yet, is not focus elsewhere: the PR
        // opening records the section it focuses before any frame draws it there, so it is not
        // revealed ([`Panel::reveal_opened`]).
        let focused = window.focused(cx);
        let mut revealed = revealed.borrow_mut();
        if revealed.focus.is_some() && revealed.focus != focused {
            revealed.focus = None;
        }
    }
}

/// Whether the frame the window last drew drew its focus inside the panel's body.
fn focus_in_body(window: &Window) -> bool {
    window
        .context_stack()
        .iter()
        .any(|context| context.contains(BODY))
}

/// Where reopening the panel puts focus ([`Panel::focus_reopened`]) and how it shows it: back
/// on the control that held it as the panel closed only where a frame draws that control
/// inside the body's view, and otherwise on the shown section's first stop
/// ([`Panel::focus_visible_section`]), either revealed whole whatever input came last. Reopened
/// by a click on the header's Pull requests, the panel put focus on Files' changed-file list
/// wherever the body had last scrolled, above the body's top at 13 and 18 pt in a 461 × 490
/// window, out of view with no ring drawn, and Tab went on to Changes below it; its parts
/// revealed focus only after a key. Back on the control that held it, the frame that first
/// draws the reopened panel lays the body out and reveals the part holding that control from
/// its own layout before the body's content prepaints ([`Reopened`]), so it draws the part
/// whole: revealed once a later frame had found it, the part came into view in the third or
/// fourth frame, cut by the body's edge in the frames before. A held PR row is shown whole in
/// its own list that way first, and a section's first stop the reopen falls to, focused by no
/// frame yet, is revealed from that first frame's layout too ([`Reopening::reveal_laid_out`]):
/// whatever a reopen focuses is whole in its first painted frame.
#[derive(Default)]
pub(super) struct Reopening {
    phase: std::cell::Cell<Reopen>,
    /// Where the innermost part of the body holding focus, or a form holding it, lies in the
    /// frame being drawn, as it prepaints ([`Reopening::note`]).
    focused: std::cell::Cell<Option<Bounds<Pixels>>>,
    /// The part holding the control focus is back on, by its name, until a frame has found that
    /// control or the frames that would have found it have passed ([`Reopening::marks`],
    /// [`Reopening::settle`]).
    part: std::cell::RefCell<Option<(&'static str, SharedString)>>,
    /// What the frame being drawn reveals from its layout before the body's content prepaints
    /// ([`Reopened`]): the held part, the held PR row, the section's first stop or the outbound
    /// confirmation, as its mark laid it out ([`RevealMark`]).
    laid_out: std::cell::Cell<Option<LaidOut>>,
    /// The layout node of the PR list holding the held row, in the frame being drawn
    /// ([`Reopening::holds_row`]).
    list_laid_out: std::cell::Cell<Option<LayoutId>>,
}
/// What a frame reveals from its layout ([`Reopening::laid_out`]): a mark's layout node, whether
/// it draws focus as a ring on its own edge, and the room its reveal keeps clear.
#[derive(Clone, Copy)]
struct LaidOut {
    node: LayoutId,
    ringed: bool,
    inset: Pixels,
    kind: LaidOutKind,
}
/// How a frame reveals what its layout holds ([`Reopening::reveal_laid_out`]).
#[derive(Clone, Copy, PartialEq)]
enum LaidOutKind {
    /// The part holding the control a reopen keeps focus on, by the least that shows it where
    /// this frame lays it out at least partly in view.
    Held,
    /// That part as a row of the PR list, whose own scroll shows its title first.
    Row,
    /// The focus a reopen falls to or a confirmation takes, as a reveal after paint decides it
    /// ([`to_reveal`]) and lands it ([`landed_edge`]).
    Reveal,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Reopen {
    #[default]
    Settled,
    /// Focus is back on the control that held it, which no frame has found drawn in the body
    /// yet: nothing but the first frame that lays out its part, which reveals the part from
    /// that layout ([`Reopened`]), reveals it until one has, which decides whether it stays.
    /// The count is the body's frames drawn since.
    Held(u8),
    /// Focus stays where it is: the frame's layout reveals the shown section's first stop
    /// before its content prepaints ([`RevealMark`]), and the body's parts and forms reveal it
    /// after that frame whatever input came last, until a frame finds it drawn. The count is
    /// the body's frames drawn since.
    Reveal(u8),
}
impl Reopening {
    /// The body's frames that find a reopened focus if anything draws it: the first frame after
    /// the panel reopens is laid out while the window's last frame, whose focus path its parts
    /// read, is the page's beneath the dialog, so the second is the first to find it.
    const FRAMES: u8 = 2;
    /// Focus is back on the control that held it, inside the part of the body called `part`.
    pub(super) fn hold(&self, part: Option<(&'static str, SharedString)>) {
        self.phase.set(Reopen::Held(0));
        self.part.replace(part);
    }
    /// Focus is on the shown section's first stop.
    pub(super) fn reveal(&self) {
        self.phase.set(Reopen::Reveal(0));
    }
    /// Forgets where focus lay, as a frame of the panel starts.
    pub(super) fn begin_frame(&self) {
        self.focused.set(None);
        self.laid_out.set(None);
        self.list_laid_out.set(None);
    }
    /// Whether the control focus is back on lies in a row of the PR list, which the frame
    /// being rendered reveals in its list as well ([`Reopening::reveal_laid_out`]).
    pub(super) fn holds_row(&self) -> bool {
        self.waits()
            && self
                .part
                .borrow()
                .as_ref()
                .is_some_and(|(part, _)| *part == PULL)
    }
    /// Whether the part of the body called `name` holds the control focus is back on, which
    /// no frame has found yet: the frame being rendered marks it ([`RevealMark`]).
    fn marks(&self, key: &str, name: &SharedString) -> bool {
        self.waits()
            && self
                .part
                .borrow()
                .as_ref()
                .is_some_and(|(part, part_name)| *part == key && part_name == name)
    }
    /// Reveals what the frame being drawn laid out to reveal ([`Reopening::laid_out`]), before
    /// the body's content prepaints ([`Reopened`]), so that frame draws it whole: whatever a
    /// reopen or an opening confirmation focuses is whole in its first painted frame.
    ///
    /// [`LaidOutKind::Held`], the part holding the control focus is back on while reveals
    /// wait: where this frame's layout puts it at least partly inside `body`'s view, so focus
    /// stays there ([`Reopening::settle`]), the body scrolls by the least that shows it whole
    /// with its ring's room, as the body's reveals bring a control into view; a part taller than
    /// the view shows its top where it lies above the view or draws its ring on its own edge,
    /// and otherwise its bottom. The second frame of a reopen can differ from the first: after
    /// a width change while the panel was closed, Files lays the patch's box out from the height
    /// the first frame measured above it ([`FilesFit`]), 43.5 px taller at 18 pt in a 380 × 490
    /// window, and the box revealed in the first frame was cut in the second.
    /// [`LaidOutKind::Row`], a held PR row, is that with the row's title shown whole in its
    /// list first ([`Reopening::reveal_row`]). [`LaidOutKind::Reveal`], what a reopen falls to
    /// and an opening confirmation's region, is the reveal the frame would run after paint
    /// ([`reveal`]), decided by the same [`to_reveal`] and [`landed_edge`], so a live drag's
    /// every step is a first frame too.
    ///
    /// Returns where it then lies, whether it draws its ring on its edge, that room, and how
    /// far the body moved its content, if it moved.
    #[allow(clippy::too_many_arguments)]
    fn reveal_laid_out(
        &self,
        body: &ScrollHandle,
        blocks: &Blocks,
        list: &ScrollHandle,
        list_blocks: &Blocks,
        revealed: &std::cell::RefCell<FocusObservation>,
        window: &mut Window,
        cx: &App,
    ) -> Option<(Bounds<Pixels>, bool, Pixels, Pixels)> {
        let LaidOut {
            node,
            ringed,
            inset,
            kind,
        } = self.laid_out.take()?;
        let mut part = window.layout_bounds(node);
        if kind == LaidOutKind::Row {
            part = self.reveal_row(part, inset, list, list_blocks, window, cx)?;
        }
        let view = body.bounds();
        let (top, bottom) = (part.top() - inset, part.bottom() + inset);
        let offset = body.offset();
        let edge = match kind {
            LaidOutKind::Reveal => {
                // As the frame's reveal after paint decides it: only a change of focus or of the
                // body's size reveals, so a manual scroll stays where it is.
                to_reveal(body, revealed, None, window, cx)?;
                if top >= view.top() && bottom <= view.bottom() {
                    return None;
                }
                landed_edge(
                    body,
                    blocks,
                    view,
                    offset,
                    (top, bottom),
                    inset,
                    ringed,
                    false,
                    cx,
                )
            }
            LaidOutKind::Held | LaidOutKind::Row => {
                if part.bottom() <= view.top() || part.top() >= view.bottom() {
                    return None;
                }
                let height = view.size.height;
                if bottom - top > height {
                    if ringed || top < view.top() {
                        top
                    } else {
                        bottom - height
                    }
                } else if top < view.top() {
                    top
                } else if bottom > view.bottom() {
                    bottom - height
                } else {
                    return None;
                }
            }
        };
        let y = (offset.y + view.top() - edge).clamp(-body.max_offset().y, Pixels::ZERO);
        let moved = y - offset.y;
        if moved == Pixels::ZERO {
            return None;
        }
        body.set_offset(point(offset.x, y));
        Some((part + point(Pixels::ZERO, moved), ringed, inset, moved))
    }
    /// Where the held PR row, whose title `part` is laid out before its list has prepainted,
    /// lies in the body once its list shows it: the list, like the body, scrolls the least that
    /// shows the title with its ring's room, its rows' lines whole at the list's edges where a
    /// place allows ([`landed_edge`]), in the frame's layout, before the rows prepaint.
    /// Returns none where the list's own view shows none of it, so focus goes on as the rest of
    /// the reopen decides ([`Reopening::settle`]).
    fn reveal_row(
        &self,
        part: Bounds<Pixels>,
        inset: Pixels,
        list: &ScrollHandle,
        blocks: &Blocks,
        window: &mut Window,
        cx: &App,
    ) -> Option<Bounds<Pixels>> {
        let view = window.layout_bounds(self.list_laid_out.take()?);
        let offset = list.offset();
        // Laid out before the list has prepainted, so its own scroll is not in the place.
        let mut row = part + point(Pixels::ZERO, offset.y);
        if row.bottom() <= view.top() || row.top() >= view.bottom() {
            return None;
        }
        let (top, bottom) = (row.top() - inset, row.bottom() + inset);
        if top < view.top() || bottom > view.bottom() {
            let edge = landed_edge(
                list,
                blocks,
                view,
                offset,
                (top, bottom),
                inset,
                false,
                false,
                cx,
            );
            let y = (offset.y + view.top() - edge).clamp(-list.max_offset().y, Pixels::ZERO);
            let moved = y - offset.y;
            if moved != Pixels::ZERO {
                list.set_offset(point(offset.x, y));
                row = row + point(Pixels::ZERO, moved);
            }
        }
        Some(row)
    }
    /// Records `bounds`, the children of what holds focus, as where focus lies in this frame.
    fn note(&self, bounds: &[Bounds<Pixels>]) {
        if let Some(bounds) = bounds.iter().copied().reduce(|a, b| a.union(&b)) {
            self.focused.set(Some(bounds));
        }
    }
    /// Whether reveals wait until a frame has found the control focus is back on.
    fn waits(&self) -> bool {
        matches!(self.phase.get(), Reopen::Held(_))
    }
    /// Whether reveals run whatever input came last.
    fn reveals(&self) -> bool {
        matches!(self.phase.get(), Reopen::Reveal(_))
    }
    /// Whether the window's last frame drew its focus in the body and the frame being drawn
    /// lays it out at least partly inside `scroll`'s view.
    pub(super) fn shows_focus(&self, scroll: &ScrollHandle, window: &Window) -> bool {
        let view = scroll.bounds();
        focus_in_body(window)
            && self.focused.get().is_some_and(|focused| {
                focused.bottom() > view.top() && focused.top() < view.bottom()
            })
    }
    /// For the body's `on_children_prepainted`, once its parts have prepainted and it has
    /// recorded what it showed ([`record_shown`]): moves the reopened focus on. Held, once a
    /// frame has found the control drawn, or once the frames that would have found it have
    /// passed, focus stays where this frame shows it inside the view as a Tab stop and is
    /// revealed whole in the next frame, or otherwise goes to the shown section's first stop by
    /// `fallback`, revealed whole in the frame after that: a Button held through a read that
    /// this frame still draws disabled, as Next PR page is on the last page, is no Tab stop.
    /// The reveals recorded in `revealed` are forgotten, so they run again. Revealing, the
    /// phase ends with the frame that finds focus drawn, whose parts have revealed it.
    pub(super) fn settle(
        &self,
        scroll: &ScrollHandle,
        revealed: [&std::cell::RefCell<FocusObservation>; 3],
        fallback: impl FnOnce(&mut Window, &mut App) + 'static,
        window: &mut Window,
        cx: &mut App,
    ) {
        match self.phase.get() {
            Reopen::Settled => {}
            Reopen::Held(frames) => {
                if focus_in_body(window) || frames + 1 >= Self::FRAMES {
                    let shown = self.shows_focus(scroll, window)
                        && window.focused(cx).is_some_and(|focus| focus.tab_stop);
                    self.phase.set(Reopen::Reveal(0));
                    self.part.take();
                    for revealed in revealed {
                        *revealed.borrow_mut() = FocusObservation::default();
                    }
                    if shown {
                        window.defer(cx, |window, _| window.refresh());
                    } else {
                        window.defer(cx, fallback);
                    }
                } else {
                    self.phase.set(Reopen::Held(frames + 1));
                    window.defer(cx, |window, _| window.refresh());
                }
            }
            Reopen::Reveal(frames) => {
                if focus_in_body(window) || frames + 1 >= Self::FRAMES {
                    self.phase.set(Reopen::Settled);
                } else {
                    self.phase.set(Reopen::Reveal(frames + 1));
                    window.defer(cx, |window, _| window.refresh());
                }
            }
        }
    }
}

/// Where the blocks of a scrolling container start: its rows, its parts and the lines a reveal
/// may bring to its top edge, so the edge cuts no line ([`reveal`]). The container's own
/// children are its scroll handle's; elements inside it report their children through
/// [`Blocks::starts`] or [`Blocks::report`] while a frame prepaints, and the container keeps
/// that frame's once everything in it has prepainted ([`Blocks::finish`]). The panel's body
/// also keeps the lines drawn in it, each line of its text and each control of one line, from
/// top to bottom, which neither of its edges may fall inside once a reveal lands
/// ([`clear_of_lines`]); they report themselves while the body collects them
/// ([`Blocks::collect_lines`], [`text_lines`], [`one_line`], [`editor_rows`]), as the lines of
/// a list that reveals its own parts do to its blocks ([`clipped_lines`]).
#[derive(Clone, Default)]
pub(super) struct Blocks {
    drawing: std::rc::Rc<std::cell::RefCell<Vec<Pixels>>>,
    drawn: std::rc::Rc<std::cell::RefCell<Vec<Pixels>>>,
    lines_drawing: std::rc::Rc<std::cell::RefCell<Vec<(Pixels, Pixels)>>>,
    lines_drawn: std::rc::Rc<std::cell::RefCell<Vec<(Pixels, Pixels)>>>,
    /// The multi-line editors drawn in the container while a frame prepaints, and those of
    /// the last frame, each with the span where the container keeps a line
    /// ([`Collecting::spans`]) and its box: an editor lays its rows out as it paints, after the
    /// container has prepainted, so [`Blocks::lines`] reads them from it then
    /// ([`editor_rows`]).
    editors_drawing: std::rc::Rc<std::cell::RefCell<Vec<DrawnEditor>>>,
    editors_drawn: std::rc::Rc<std::cell::RefCell<Vec<DrawnEditor>>>,
    /// The lines of each text the body collecting them drew in the last frame, in the order
    /// they prepainted ([`text_lines`]), which a text drawn again takes as long as nothing
    /// they follow from has changed.
    texts: std::rc::Rc<std::cell::RefCell<Vec<TextLines>>>,
    /// How many times a text reported its lines to the body ([`text_lines`]), and how many
    /// of those worked them out ([`TextLines::measure`]).
    #[cfg(test)]
    text_counts: std::rc::Rc<std::cell::Cell<(usize, usize)>>,
    /// What the last reveal of the container kept whole ([`show`]): the focus it revealed and
    /// the part with its ring's room, top and bottom with the container's scroll at zero, so
    /// the reveal sweep holds a stop to what its reveal kept rather than to a part it finds by
    /// a debug selector, which the part's own element can replace
    /// (`github_view::tests::needless_cut`).
    #[cfg(test)]
    kept: std::rc::Rc<std::cell::RefCell<Option<Kept>>>,
}
/// What a reveal kept whole ([`Blocks::kept`]): the focus it revealed, and the top and bottom
/// of the part with its ring's room, with the container's scroll at zero.
#[cfg(test)]
type Kept = (Option<FocusHandle>, Pixels, Pixels);
/// The lines of a text of the panel ([`text_lines`]): how far apart its lines are drawn and
/// how many each of its children draws, with what they follow from, so a frame that draws the
/// text unchanged takes them as the last frame left them instead of reading the window's
/// text style again ([`drawn_line_height`]), which folds every style around the text into a
/// new one and allocates its font features.
///
/// What it keeps them by assumes a line's height follows the text's font size alone, as the
/// relative line heights of the styles around the panel's texts make it: the font size
/// follows the interface's text scale (`appearance::ui_text`) or, for a size in rems, the
/// window's rem size, and the height drawn snaps to the window's scale factor, which are all
/// it records beside the text and its children's sizes ([`TextLines::scale`]). A setting that
/// moves the line height apart from the font size, as a line spacing would, has to join them,
/// or a text drawn after it changed would keep its old line height wherever its children kept
/// their sizes.
#[derive(Default)]
struct TextLines {
    /// The text element, as its call site names it ([`super::label`], [`super::phrases`]).
    id: Option<ElementId>,
    /// The interface's text scale (`appearance::ui_scale`), which sizes the panel's text, the
    /// window's rem size, which sizes a text set in rems, and the window's scale factor, to
    /// which each line's height snaps.
    scale: (f32, Pixels, f32),
    /// How far apart the text's lines are drawn.
    line: Pixels,
    /// The size of each of its children, which its text, its width and its font decide as
    /// they wrap it, and how many lines that child draws.
    children: Vec<(Size<Pixels>, f32)>,
}
impl TextLines {
    /// The scales the lines of a text drawn in `window` now follow from
    /// ([`TextLines::scale`]).
    fn scales(window: &Window) -> (f32, Pixels, f32) {
        (
            crate::appearance::ui_scale(),
            window.rem_size(),
            window.scale_factor(),
        )
    }
    /// Whether these are the lines of the text `id` drawing `children` at `scale`.
    fn fits(&self, id: &ElementId, children: &[Bounds<Pixels>], scale: (f32, Pixels, f32)) -> bool {
        self.id.as_ref() == Some(id)
            && self.scale == scale
            && self.children.len() == children.len()
            && self
                .children
                .iter()
                .zip(children)
                .all(|((size, _), child)| *size == child.size)
    }
    /// Works the lines of the text `id` drawing `children` out again, in the window's text
    /// style as it prepaints, reusing the room these took.
    fn measure(
        &mut self,
        id: &ElementId,
        children: &[Bounds<Pixels>],
        scale: (f32, Pixels, f32),
        window: &Window,
    ) {
        let line = drawn_line_height(window);
        self.id = Some(id.clone());
        self.scale = scale;
        self.line = line;
        self.children.clear();
        self.children.extend(children.iter().map(|child| {
            // A text with nothing in it, as an empty status, is no line.
            let count = if line > Pixels::ZERO {
                (child.size.height / line).round()
            } else {
                0.
            };
            (child.size, count)
        }));
    }
}
/// A multi-line editor drawn in a container collecting lines, as a reader of its layout, with
/// the span where the container keeps a line and the editor's box, its padding inside it
/// ([`Blocks::editors_drawing`]).
type DrawnEditor = (
    std::rc::Rc<dyn Fn(&App) -> EditorLayout>,
    (Pixels, Pixels),
    Bounds<Pixels>,
);
/// What a multi-line editor laid out when it last painted ([`editor_lines`]): its viewport, the
/// bounds of its scrolled text and its line height, the last two once it has painted.
type EditorLayout = (Bounds<Pixels>, Option<Bounds<Pixels>>, Option<Pixels>);
/// The body collecting the lines its content draws ([`Blocks::collect_lines`]).
struct Collecting {
    blocks: Blocks,
    /// The clip the body draws its content in.
    clip: Bounds<Pixels>,
    /// The viewports of the lists inside the body around what is prepainting, outermost
    /// first ([`clipped`]), each with the blocks of a list that collects lines of its own
    /// ([`clipped_lines`]).
    lists: Vec<(Bounds<Pixels>, Option<Blocks>)>,
    /// How many texts have reported their lines in this frame so far, the place of the next
    /// in [`Blocks::texts`].
    texts: usize,
}
impl Collecting {
    /// The span of the body's content, top to bottom, where a person can see a line drawn
    /// under `mask`: inside the viewport of every list around it ([`clipped`]) and inside
    /// any other clip within the body, which `mask` shows where it stands inside the body's
    /// clip. The body's own clip bounds nothing: a reveal brings what lies beyond it into
    /// view. A list's rows it has scrolled out of itself are hidden wherever the body
    /// scrolls, even where the list runs past the body's edge and its clip there is the
    /// body's, where the body once kept their lines.
    fn visible(&self, mask: Bounds<Pixels>) -> (Pixels, Pixels) {
        let slack = px(0.5);
        let mut top = if mask.top() > self.clip.top() + slack {
            mask.top()
        } else {
            px(f32::NEG_INFINITY)
        };
        let mut bottom = if mask.bottom() < self.clip.bottom() - slack {
            mask.bottom()
        } else {
            px(f32::INFINITY)
        };
        for (list, _) in &self.lists {
            top = top.max(list.top());
            bottom = bottom.min(list.bottom());
        }
        (top, bottom)
    }
    /// The containers collecting a line drawn under `mask`, each with the span where it keeps
    /// one: the body, where a person can see it ([`Collecting::visible`]), and each list
    /// around it that collects lines of its own ([`clipped_lines`]), where the list's own
    /// scroll can bring it into view. For such a list, the rows it has scrolled out of itself
    /// and what lies past the body's clip are still its lines, while a list or a clip inside
    /// it hides what it hides. Each is handed to `each` in turn, the body's first, so a frame's
    /// texts collect their lines without gathering the containers anywhere first.
    fn spans(&self, mask: Bounds<Pixels>, mut each: impl FnMut(&Blocks, (Pixels, Pixels))) {
        let slack = px(0.5);
        each(&self.blocks, self.visible(mask));
        for (index, (_, blocks)) in self.lists.iter().enumerate() {
            let Some(blocks) = blocks else {
                continue;
            };
            // The clip the list and what holds it put on its content, which bounds nothing.
            let (outer_top, outer_bottom) = self.lists[..=index].iter().fold(
                (self.clip.top(), self.clip.bottom()),
                |(top, bottom), (list, _)| (top.max(list.top()), bottom.min(list.bottom())),
            );
            let mut top = if mask.top() > outer_top + slack {
                mask.top()
            } else {
                px(f32::NEG_INFINITY)
            };
            let mut bottom = if mask.bottom() < outer_bottom - slack {
                mask.bottom()
            } else {
                px(f32::INFINITY)
            };
            for (inner, _) in &self.lists[index + 1..] {
                top = top.max(inner.top());
                bottom = bottom.min(inner.bottom());
            }
            each(blocks, (top, bottom));
        }
    }
}
/// Keeps in `blocks` the part of the line from `top` to `bottom` that lies in `span`
/// ([`Collecting::spans`]), where a person can see it, the line whole where it lies in `span`
/// to half a pixel either way, layout's rounding: a line a list or a clip inside the container
/// hides is that list's or clip's to show, not the container's, while the part of a line that
/// a list's or a clip's edge leaves in view, as the changed-file list's foot leaves of its
/// last row, is a line the container's edges must not cut again, as an editor's row is
/// ([`editor_lines`]). A part thinner than layout's rounding is none. Kept only where they lay
/// whole, the rows a list cut reported nothing: in a 461 × 490 window, Tab onto the Review tab
/// in Files at 18 pt stopped the body's foot through the status line the changed-file list's
/// foot left in view of its second row, and Tab onto Source at 13 pt stopped the body's top
/// through the file name of its third.
fn keep_line(blocks: &Blocks, top: Pixels, bottom: Pixels, (shown, ends): (Pixels, Pixels)) {
    let slack = px(0.5);
    let whole = top >= shown - slack && bottom <= ends + slack;
    let (top, bottom) = if whole {
        (top, bottom)
    } else {
        (top.max(shown), bottom.min(ends))
    };
    if bottom - top > if whole { Pixels::ZERO } else { slack } {
        blocks.lines_drawing.borrow_mut().push((top, bottom));
    }
}
thread_local! {
    /// The body whose content is prepainting while it collects its lines
    /// ([`Blocks::collect_lines`]): GPUI prepaints a window's elements on its thread one at a
    /// time, so what reports here between the body's first child and its own
    /// `on_children_prepainted` lies inside the body.
    static COLLECTING: std::cell::RefCell<Option<Collecting>> =
        const { std::cell::RefCell::new(None) };
    /// The room the lists around what prepaints took in the last body that collected its
    /// lines ([`Collecting::lists`]), empty, for the next to fill.
    static LISTS: std::cell::RefCell<Vec<(Bounds<Pixels>, Option<Blocks>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}
#[cfg(test)]
thread_local! {
    /// Each control of one line drawn since the last body that collects lines started its
    /// frame ([`one_line`], [`Blocks::collect_lines`]), its frame and the clip it was drawn
    /// in, for the reveal sweep to read where each lies as drawn, apart from the body's record
    /// of its lines: Buttons write no accessibility node a test can read
    /// (`github_view::tests::frame_lines`).
    static ONE_LINES: std::cell::RefCell<Vec<(Bounds<Pixels>, Bounds<Pixels>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}
/// The controls of one line the last frame of the body drew, each with the clip it was drawn
/// in ([`ONE_LINES`]).
#[cfg(test)]
pub(super) fn drawn_one_lines() -> Vec<(Bounds<Pixels>, Bounds<Pixels>)> {
    ONE_LINES.with(|drawn| drawn.borrow().clone())
}
/// For the `on_children_prepainted` of a text element of the panel ([`super::label`],
/// [`super::phrases`]): each of `children`, text in the element's own style, reports each of
/// its lines a person can see to the body collecting them ([`Blocks::collect_lines`]), if one
/// is. GPUI lays a text's lines out one under another from its top, each as tall as its
/// style's line height snapped to a device pixel ([`drawn_line_height`]), which the element's
/// text style gives while it prepaints. Only the lines inside what can be seen are counted, so
/// a long text in a list, as a 65,536-character comment can be, costs the lines the list shows.
///
/// `id` names the text, as its call site does. Its line height and the lines each child draws
/// are worked out once for each change of what they follow from, its text, width or font (the
/// sizes of its children, which those decide), its size (the interface's text scale) or the
/// window's scale, and kept for the frames that draw it unchanged ([`TextLines`]): each frame
/// reports them again, where they lie then, but reads no text style for them.
pub(super) fn text_lines(
    id: &ElementId,
    children: Vec<Bounds<Pixels>>,
    window: &mut Window,
    _: &mut App,
) {
    COLLECTING.with(|collecting| {
        let Some(collecting) = &mut *collecting.borrow_mut() else {
            return;
        };
        let place = collecting.texts;
        collecting.texts += 1;
        let mut texts = collecting.blocks.texts.borrow_mut();
        if texts.len() <= place {
            texts.resize_with(place + 1, TextLines::default);
        }
        let text = &mut texts[place];
        let scale = TextLines::scales(window);
        let measure = !text.fits(id, &children, scale);
        if measure {
            text.measure(id, &children, scale, window);
        }
        #[cfg(test)]
        {
            let (drawn, measured) = collecting.blocks.text_counts.get();
            let counts = (drawn + 1, measured + usize::from(measure));
            collecting.blocks.text_counts.set(counts);
        }
        let height = text.line;
        if height <= Pixels::ZERO {
            return;
        }
        collecting.spans(window.content_mask().bounds, |blocks, span| {
            for (child, (_, count)) in children.iter().zip(&text.children) {
                let first = ((span.0 - child.top()) / height).floor().clamp(0., *count);
                let last = ((span.1 - child.top()) / height).ceil().clamp(0., *count);
                for index in first as usize..last as usize {
                    let top = child.top() + height * index as f32;
                    keep_line(blocks, top, top + height, span);
                }
            }
        });
    });
}
/// How far apart GPUI's text element draws the lines of a text in the window's text style: the
/// style's line height at its font size, snapped to a device pixel, as the element lays its
/// lines out. Not [`Window::line_height`], which rounds to a whole point first: at 17 pt the
/// panel's 12 pt text is 15.7 px and its line 25.4 px, drawn 25.5 px apart at two device pixels
/// to the point but counted 25 px apart, so the connection help's fifth line was taken for
/// 337.5..362.5 where it drew at 339.5..365, and a reveal left it cut by 2.5 px at the body's
/// foot as a stop clear of every line.
fn drawn_line_height(window: &Window) -> Pixels {
    let style = window.text_style();
    let rem = window.rem_size();
    let font_size = style.font_size.to_pixels(rem);
    window.pixel_snap(style.line_height.to_pixels(font_size.into(), rem))
}
/// `list`, a list that scrolls inside the panel's body and clips its rows, in an element that
/// tells the body collecting lines the list's viewport while the list prepaints, so the rows
/// it has scrolled out of itself report no lines and a row its edge cuts reports the part it
/// shows ([`Collecting::visible`], [`keep_line`]). The element is as tall as the list and
/// stands where it would.
pub(super) fn clipped(list: impl IntoElement) -> Div {
    clipped_with(list, None)
}
/// [`clipped`] for a list that reveals its own parts, as the conversation list does: it also
/// collects its own lines in `blocks`, all those its scroll can bring into view, so its
/// reveals stop where its edges cut none ([`Blocks::lines`]). Collecting only the body's, its
/// reveals saw no line at all and could stop with a comment's line cut at the list's edge.
pub(super) fn clipped_lines(list: impl IntoElement, blocks: &Blocks) -> Div {
    clipped_with(list, Some(blocks.clone()))
}
fn clipped_with(list: impl IntoElement, blocks: Option<Blocks>) -> Div {
    div()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .on_children_prepainted(|_, _, _| {
            COLLECTING.with(|collecting| {
                if let Some(collecting) = &mut *collecting.borrow_mut() {
                    collecting.lists.pop();
                }
            });
        })
        // Prepainted before the list, laid over it at its place in the body.
        .child(
            canvas(
                move |bounds, _, _| {
                    COLLECTING.with(|collecting| {
                        if let Some(collecting) = &mut *collecting.borrow_mut() {
                            collecting.lists.push((bounds, blocks.clone()));
                        }
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        )
        .child(list)
}
/// An element laid over its parent, a control of one line such as a Button or a field, that
/// reports the parent as one line of the body collecting them ([`Blocks::collect_lines`]): an
/// edge that cut its frame would cut its label too. Absolutely placed, it takes no room and
/// moves nothing in its parent.
pub(super) fn one_line() -> impl IntoElement {
    canvas(
        |bounds, window, _| {
            #[cfg(test)]
            ONE_LINES.with(|drawn| {
                drawn
                    .borrow_mut()
                    .push((bounds, window.content_mask().bounds))
            });
            COLLECTING.with(|collecting| {
                if let Some(collecting) = &*collecting.borrow() {
                    collecting.spans(window.content_mask().bounds, |blocks, span| {
                        keep_line(blocks, bounds.top(), bounds.bottom(), span);
                    });
                }
            });
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}
/// An element laid over `editor`, a multi-line editor such as a composer's or Files' Source,
/// that records it in each container collecting lines around it ([`Blocks::collect_lines`],
/// [`clipped_lines`]), so the container's reveals read each of its rows as a line
/// ([`Blocks::lines`]): an edge inside a row would cut its text, which reported no lines
/// before, so a reveal could stop with a row of a draft, or of Source's patch, cut at the
/// body's edge. Laid over the editor's box, padding and edge included, it counts the rows
/// drawn in that box; laid inside the clip that cuts the editor's rows, as Source's is, only
/// the part of each row that clip shows. Absolutely placed, it takes no room and moves
/// nothing.
pub(super) fn editor_rows<M: gpui_kit::base::input::MultiLineMode>(
    editor: &Entity<gpui_kit::base::input::InputBaseState<M>>,
) -> impl IntoElement {
    let editor = editor.clone();
    let layout: std::rc::Rc<dyn Fn(&App) -> EditorLayout> = std::rc::Rc::new(move |cx| {
        let editor = editor.read(cx);
        (
            editor.input_bounds(),
            editor.text_bounds(),
            editor.line_height(),
        )
    });
    canvas(
        move |bounds, window, _| {
            COLLECTING.with(|collecting| {
                if let Some(collecting) = &*collecting.borrow() {
                    collecting.spans(window.content_mask().bounds, |blocks, span| {
                        let drawn = (layout.clone(), span, bounds);
                        blocks.editors_drawing.borrow_mut().push(drawn);
                    });
                }
            });
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}
/// The rows of an editor that last painted `layout`, in that frame's coordinates, each a line
/// where a person can see some of it, added to `lines`: each row the editor draws in `frame`,
/// its box, the part of it inside that box, cut too where `span` ends ([`Collecting::spans`]),
/// at a list or a clip that ends inside the body. The editor draws its rows one under another
/// from the top of its scrolled text, each as tall as its laid-out line height, from the row
/// at its viewport's top to two past the row at its foot, and clips them only sideways, so a
/// row it has scrolled partly out of its viewport draws into its padding, and a row below the
/// viewport's foot can start there too: counted only inside the viewport, such a row
/// let a reveal stop with the body's edge at the viewport's edge, through the row's text in
/// the padding, as at 13 pt in a 461 × 490 window, where the review summary scrolled 14 px
/// drew its ninth row from 2 px below its viewport into its padding, 7 px deep, which no line
/// held. Each place for a row in its box counts, the empty ones past its text among them, as
/// the frame of rows the editor is; a part thinner than layout's rounding is none.
fn editor_lines(
    (view, text, line): EditorLayout,
    frame: Bounds<Pixels>,
    span: (Pixels, Pixels),
    lines: &mut Vec<(Pixels, Pixels)>,
) {
    let (Some(text), Some(line)) = (text, line) else {
        return;
    };
    if line <= Pixels::ZERO || view.size.height <= Pixels::ZERO {
        return;
    }
    let first = ((view.top() - text.top()) / line).floor().max(0.);
    // The rows it draws that start before its box's foot.
    let last = ((frame.bottom() - text.top()) / line)
        .ceil()
        .min(((view.bottom() - text.top()) / line).ceil() + 2.)
        .max(first);
    let (shown, ends) = (span.0.max(frame.top()), span.1.min(frame.bottom()));
    for row in first as usize..last as usize {
        let top = text.top() + line * row as f32;
        let (top, bottom) = (top.max(shown), (top + line).min(ends));
        if bottom - top > px(0.5) {
            lines.push((top, bottom));
        }
    }
}
/// An element laid over its parent, a control in a row that wraps such as the toolbar's, that
/// reports the parent's top as the start of a block of the body collecting lines
/// ([`Blocks::collect_lines`]), so a reveal can bring the body's top edge to the row it starts.
pub(super) fn starts_block() -> impl IntoElement {
    canvas(
        |bounds, _, _| {
            COLLECTING.with(|collecting| {
                if let Some(collecting) = &*collecting.borrow() {
                    collecting.blocks.drawing.borrow_mut().push(bounds.top());
                }
            });
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}
impl Blocks {
    /// The body's first child: an empty element whose prepaint starts collecting the lines
    /// the body's content draws, until the body's own `on_children_prepainted` keeps them
    /// ([`Blocks::finish`]). `clip` is the clip the body draws its content in.
    pub(super) fn collect_lines(&self) -> Div {
        let blocks = self.clone();
        div().on_children_prepainted(move |_, window, _| {
            #[cfg(test)]
            ONE_LINES.with(|drawn| drawn.borrow_mut().clear());
            let collecting = Collecting {
                blocks: blocks.clone(),
                clip: window.content_mask().bounds,
                lists: LISTS.take(),
                texts: 0,
            };
            COLLECTING.with(|slot| *slot.borrow_mut() = Some(collecting));
        })
    }
    /// The lines of the last prepainted frame, in its coordinates with the body's scroll at
    /// its offset then: those its content reported and the rows of the multi-line editors it
    /// drew, read from each editor's layout, which it keeps from the frame it last painted
    /// ([`editor_rows`]), so a reveal deferred until that frame has painted reads that frame's.
    pub(super) fn lines(&self, cx: &App) -> Vec<(Pixels, Pixels)> {
        let mut lines = self.lines_drawn.borrow().clone();
        for (layout, span, frame) in self.editors_drawn.borrow().iter() {
            editor_lines(layout(cx), *frame, *span, &mut lines);
        }
        lines
    }
    /// For an element's `on_children_prepainted`: each of its children starts a block.
    pub(super) fn starts(&self) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        let blocks = self.clone();
        move |children, _, _| blocks.report(&children)
    }
    /// Each of `children`, prepainted in the frame being drawn, starts a block.
    pub(super) fn report(&self, children: &[Bounds<Pixels>]) {
        self.drawing
            .borrow_mut()
            .extend(children.iter().map(|child| child.top()));
    }
    /// For the container's own `on_children_prepainted`, after everything in it has
    /// prepainted: keeps the starts, lines and editors this frame reported. Each of them
    /// trades places with the last frame's, which the next frame then refills, so a frame
    /// that reports as many as the last takes no new room for them.
    pub(super) fn finish(&self) {
        // Only the container that collects ends its collection: a list inside the body, such
        // as the conversation list, finishes its own blocks while the body still collects.
        // The texts it no longer draws leave their lines with it.
        COLLECTING.with(|collecting| {
            let mut collecting = collecting.borrow_mut();
            if collecting.as_ref().is_some_and(|collecting| {
                std::rc::Rc::ptr_eq(&collecting.blocks.lines_drawing, &self.lines_drawing)
            }) && let Some(ended) = collecting.take()
            {
                self.texts.borrow_mut().truncate(ended.texts);
                let mut lists = ended.lists;
                lists.clear();
                LISTS.set(lists);
            }
        });
        fn keep<T>(drawing: &std::cell::RefCell<Vec<T>>, drawn: &std::cell::RefCell<Vec<T>>) {
            let (mut drawing, mut drawn) = (drawing.borrow_mut(), drawn.borrow_mut());
            std::mem::swap(&mut *drawing, &mut *drawn);
            drawing.clear();
        }
        keep(&self.drawing, &self.drawn);
        keep(&self.lines_drawing, &self.lines_drawn);
        keep(&self.editors_drawing, &self.editors_drawn);
    }
    /// How many times a text has reported its lines to the body, and how many of those
    /// worked them out ([`text_lines`]).
    #[cfg(test)]
    pub(super) fn text_counts(&self) -> (usize, usize) {
        self.text_counts.get()
    }
    /// What the last reveal of `scroll`'s container kept whole, where `scroll` lies now, if
    /// it revealed the focus the window holds ([`Blocks::kept`]).
    #[cfg(test)]
    pub(super) fn kept(
        &self,
        scroll: &ScrollHandle,
        window: &Window,
        cx: &App,
    ) -> Option<(Pixels, Pixels)> {
        let kept = self.kept.borrow();
        let (focus, top, bottom) = kept.as_ref()?;
        let offset = scroll.offset().y;
        (*focus == window.focused(cx)).then(|| (*top + offset, *bottom + offset))
    }
    /// Forgets the lines of every text, so the next frame works each text's out afresh.
    #[cfg(test)]
    pub(super) fn forget_texts(&self) {
        self.texts.borrow_mut().clear();
    }
    /// Whether the record of each start, line and editor the next frame reports has the room
    /// the last frame's took ([`Blocks::finish`]).
    #[cfg(test)]
    pub(super) fn keeps_its_room(&self) -> bool {
        self.drawing.borrow().capacity() >= self.drawn.borrow().len()
            && self.lines_drawing.borrow().capacity() >= self.lines_drawn.borrow().len()
            && self.editors_drawing.borrow().capacity() >= self.editors_drawn.borrow().len()
            && !self.lines_drawn.borrow().is_empty()
    }
    /// The starts of the last prepainted frame, in its coordinates with `scroll` at its
    /// offset then: the container's children and those reported from inside it.
    fn at(&self, scroll: &ScrollHandle) -> Vec<Pixels> {
        let offset = scroll.offset().y;
        let mut starts = self.drawn.borrow().clone();
        starts.extend(
            (0..scroll.children_count())
                .filter_map(|index| scroll.bounds_for_item(index))
                .map(|child| child.top() + offset),
        );
        starts
    }
}

/// Where the top edge of `viewport` lands, in its frame's coordinates, when it scrolls to show
/// `top..bottom`, a part with its ring's room, whole. A part taller than the view shows the
/// side it lies beyond, its top where it lies above the view and otherwise its bottom, unless
/// `edge`, the part draws focus as a ring on its own edge, such as the conversations pane: then
/// its top, so the ring's top edge stays drawn. A part above the view brings its top to the
/// edge, which then moves back to the nearest of `starts` (each with the ring's `inset` above it)
/// that keeps the part whole, so a part at the top of a box, such as the panel's banner, shows
/// the box's top with it and the first part returns the body to its top. Below the view, the
/// least scroll leaves the part at the bottom edge and the top edge wherever that falls, so
/// the edge moves on to the first start that keeps the part whole, or to the part's own top.
/// Where the content ends at `end`, the farthest the edge can go, before the edge gets there,
/// it stops at the last start before `end` that keeps the part whole instead. Either way the
/// edge lies at the start of a block, never through a line.
fn landing(
    viewport: Bounds<Pixels>,
    top: Pixels,
    bottom: Pixels,
    starts: &[Pixels],
    inset: Pixels,
    end: Pixels,
    edge: bool,
) -> Pixels {
    let height = viewport.size.height;
    let edges = || {
        starts
            .iter()
            .map(|start| *start - inset)
            .filter(move |edge| *edge <= top && *edge >= bottom - height)
    };
    let edge = if bottom - top > height {
        if edge || top < viewport.top() {
            top
        } else {
            bottom - height
        }
    } else if top < viewport.top() {
        edges().reduce(Pixels::max).unwrap_or(top)
    } else {
        edges().reduce(Pixels::min).unwrap_or(top)
    };
    if edge <= end {
        return edge;
    }
    edges()
        .filter(|edge| *edge <= end)
        .reduce(Pixels::max)
        .unwrap_or(end)
}

/// Where the top edge of `viewport` lands instead of `edge`, where a reveal brought it
/// ([`landing`]), so that neither of the view's edges falls inside one of `lines`, the lines of
/// text and controls of one line drawn in it: a half line reads as a rendering fault, and its
/// words cannot be read. The part the reveal shows, `top..bottom` with its ring's room, stays
/// whole, and the edge stays between `first` and `last`, where the content lets it scroll.
/// Where `edge` leaves a line cut, the edge moves to the nearest place that keeps the part
/// whole with its ring and cuts no line at either edge, whichever way that lies ([`stop`]):
/// a safe place, one of `starts`, where blocks start less the ring's room, or an end of the
/// content, as [`landing`] does; or a place between two of `lines`, the top edge or the bottom
/// edge at a line's top or foot. At equal distance a safe place wins, then the one that
/// scrolls less. A reveal brings the focused control into view the way a scroll view does,
/// moving the least, as macOS's `scrollRectToVisible` does (decision, 2026-11-05): a far stop
/// skipped content a keyboard walk then never showed whole, as safe places first did at 17 pt
/// in a 440 × 490 window, where Tab onto Connect took the body to Connect's row, 386.5 px
/// down, past the connection help's second to sixth lines, which no stop of the walk showed
/// whole. A place between lines can stop inside no text, as every surface the body draws
/// reports its lines: each line of a text ([`text_lines`]), each control of one line
/// ([`one_line`]), each row of a multi-line editor ([`editor_rows`]) and the lines a list
/// shows ([`clipped`]). Where no place serves, the part whole wins and the edge stays at
/// `edge`, as it does where it landed at the content's top, the box's top shown with the
/// first part. A part taller than the view keeps the half of the view on the side the reveal
/// shows. At 18 pt in a 461 × 490 window, Tab onto Connect once left the connection help's
/// second line cut at the body's top edge.
fn clear_of_lines(
    viewport: Bounds<Pixels>,
    edge: Pixels,
    (top, bottom): (Pixels, Pixels),
    lines: &[(Pixels, Pixels)],
    starts: &[Pixels],
    (first, last): (Pixels, Pixels),
) -> Pixels {
    let height = viewport.size.height;
    // The content's ends allow layout's rounding, where the scroll stops whatever the edge.
    let slack = px(0.5);
    // A line is cut by any edge strictly inside it: an edge that stopped half a pixel inside
    // a line, as at its other end, could cut a pixel of it once the scroll is laid out. The
    // part, its ring's room about it, stays whole to the same hundredth: allowed half a pixel
    // like the content's ends, the bottom edge at the foot of a line below it stopped the top
    // edge half a pixel inside the PR list's first row at 18 pt in a 461 × 490 window, its
    // ring's top edge cut by the body's clip.
    let within = px(0.01);
    let (top, bottom) = if bottom - top <= height {
        (top, bottom)
    } else if (edge - top).abs() <= slack {
        (top, top + height / 2.)
    } else {
        (edge + height / 2., edge + height)
    };
    let cuts = Insides::new(lines, within);
    let cuts = |at: Pixels| cuts.contain(at);
    let lands = |at: Pixels| {
        at >= first - slack
            && at <= last + slack
            && at <= top + within
            && at + height >= bottom - within
            && !cuts(at)
            && !cuts(at + height)
    };
    // A reveal that lands at the content's top, as the first part's does, shows the box's top
    // with the part ([`landing`]), which no place below it keeps: it would cut what lies
    // above the part, a padding or a surface that reports no line. At 18 pt in a 1,480 × 800
    // window, the bottom edge at the foot of a line it cut took Tab wrapping to the offline
    // fixture's banner 8.5 px down, the banner's surface cut at the body's top. This is a
    // decided exception to the order of the stops below (coordinator's decision, 2026-11-05):
    // the content's top is the one stop that cuts nothing above it and the place a Tab wrap
    // returns to, so it stays even where the body's foot then cuts a line and a safe stop
    // below it clears every line. Keep it when changing that order.
    if lands(edge) || edge <= first + slack {
        return edge;
    }
    stop(viewport, edge, lines, starts, (first, last), lands).unwrap_or(edge)
}

/// The place for the top edge of `viewport` nearest `edge` among those where it `lands`
/// ([`clear_of_lines`]): the safe places, where a reveal may bring it whatever drew there, the
/// start of a block (one of `starts`) or an end of the content, `first` or `last`; and the
/// places between two of `lines`, the top edge or the bottom edge at a line's top or foot.
/// The part's own top lies inside its frame: Tab wrapping to the banner stopped there, 9 px
/// down, at 18 pt. At equal distance, to a hundredth of a pixel, a safe place wins, then the
/// one that moves the edge back toward where the view stood, scrolling less. With the
/// connection help in five lines at 18 pt, as macOS lays it out in a 461 × 490 window, no
/// block start kept Connect whole with both edges clear, and the top edge at the foot of the
/// help's first line clears every line; at 17 pt in a 440 × 490 window, Shift+Tab onto
/// Account options stops with the bottom edge at the foot of the help's fifth line.
fn stop(
    viewport: Bounds<Pixels>,
    edge: Pixels,
    lines: &[(Pixels, Pixels)],
    starts: &[Pixels],
    (first, last): (Pixels, Pixels),
    lands: impl Fn(Pixels) -> bool,
) -> Option<Pixels> {
    let height = viewport.size.height;
    let within = px(0.01);
    // Less scrolling moves the edge back toward where the view stood.
    let from = viewport.top();
    let less = |at: Pixels| (at > edge) == (from > edge);
    let safe = starts.iter().copied().chain([first, last]);
    let between = lines
        .iter()
        .flat_map(|(top, bottom)| [*top, *bottom, *top - height, *bottom - height]);
    let places: Vec<(Pixels, bool)> = safe
        .map(|at| (at, true))
        .chain(between.map(|at| (at, false)))
        .filter(|(at, _)| lands(*at))
        .collect();
    let distance = |at: Pixels| (at - edge).abs();
    let nearest = places
        .iter()
        .map(|(at, _)| distance(*at))
        .reduce(Pixels::min)?;
    places
        .into_iter()
        .filter(|(at, _)| distance(*at) <= nearest + within)
        .min_by_key(|(at, safe)| (!safe, !less(*at)))
        .map(|(at, _)| at)
}

/// The insides of a set of lines, where an edge cuts one of them ([`clear_of_lines`]): each
/// line from `within` below its top to `within` above its foot, open at both ends, merged where
/// they overlap and kept in order, so whether an edge cuts a line costs a binary search rather
/// than a pass over every line. [`clear_of_lines`] asks that of four places a line, so a pass
/// each made its stops cost the square of the lines: a long conversation's thousands of lines
/// took millions of comparisons for one reveal. Two insides that only touch stay apart, as an
/// edge at the foot of one line and the top of the next cuts neither.
struct Insides(Vec<(Pixels, Pixels)>);
impl Insides {
    fn new(lines: &[(Pixels, Pixels)], within: Pixels) -> Self {
        let mut insides: Vec<_> = lines
            .iter()
            .map(|(top, bottom)| (*top + within, *bottom - within))
            .filter(|(top, bottom)| top < bottom)
            .collect();
        insides.sort_by(|a, b| a.0.as_f32().total_cmp(&b.0.as_f32()));
        let mut merged: Vec<(Pixels, Pixels)> = Vec::with_capacity(insides.len());
        for (top, bottom) in insides {
            match merged.last_mut() {
                Some(last) if top < last.1 => last.1 = last.1.max(bottom),
                _ => merged.push((top, bottom)),
            }
        }
        Self(merged)
    }
    /// Whether `at` lies strictly inside a line's inside.
    fn contain(&self, at: Pixels) -> bool {
        let after = self.0.partition_point(|(top, _)| *top < at);
        after > 0 && at < self.0[after - 1].1
    }
}

/// Scrolls `scroll`, the panel's body or a list inside it, to show `bounds`, the children of
/// what focus has moved into, `inset` clear of its edges, landing its top edge at the start of
/// one of `blocks` ([`landing`]), after this frame paints, unless focus or a scroll supersedes
/// it. Only a change of focus since the frame `revealed` records reveals. `edge` is the focus
/// whose ring the part draws on its own edge, if it has one.
#[allow(clippy::too_many_arguments)]
fn reveal(
    scroll: &ScrollHandle,
    revealed: &std::cell::RefCell<FocusObservation>,
    blocks: &Blocks,
    bounds: Vec<Bounds<Pixels>>,
    inset: Pixels,
    edge: Option<&FocusHandle>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some((ringed, focus)) = to_reveal(scroll, revealed, edge, window, cx) else {
        return;
    };
    show(
        scroll,
        blocks,
        bounds,
        inset,
        ringed,
        move |window, cx| window.focused(cx) == focus,
        window,
        cx,
    );
}

/// Whether [`reveal`] reveals in `scroll` now, where only a change of focus since the frame
/// `revealed` records reveals, and if it does, whether the part draws focus as a ring on its
/// own edge (`edge`) and the focus it reveals.
fn to_reveal(
    scroll: &ScrollHandle,
    revealed: &std::cell::RefCell<FocusObservation>,
    edge: Option<&FocusHandle>,
    window: &Window,
    cx: &App,
) -> Option<(bool, Option<FocusHandle>)> {
    let observed = FocusObservation {
        focus: window.focused(cx),
        viewport: scroll.bounds(),
        rem_size: window.rem_size(),
    };
    let previous_focus = revealed.borrow();
    // Dialog entrance animation can move the whole viewport without
    // changing what fits inside it. It must not undo manual scrolling.
    if previous_focus.focus == observed.focus
        && previous_focus.viewport.size == observed.viewport.size
        && previous_focus.rem_size == observed.rem_size
    {
        return None;
    }
    Some((
        edge.is_some_and(|edge| edge.is_focused(window)),
        observed.focus,
    ))
}

/// Scrolls `scroll` to show `bounds`, `inset` clear of its edges, landing its top edge at the
/// start of one of `blocks` ([`landing`]), after this frame paints, unless a scroll supersedes
/// it or `still` no longer holds then; nowhere where they are in view already. Focus stays
/// where it is. `ringed` is whether the part draws focus as a ring on its own edge.
#[allow(clippy::too_many_arguments)]
pub(super) fn show(
    scroll: &ScrollHandle,
    blocks: &Blocks,
    bounds: Vec<Bounds<Pixels>>,
    inset: Pixels,
    ringed: bool,
    still: impl Fn(&Window, &App) -> bool + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    show_then(
        scroll, blocks, bounds, inset, ringed, still, None, false, window, cx,
    );
}

/// What a reveal does once its scroll has landed, with where what it showed lies then
/// ([`show_then`]).
type Landed = Box<dyn FnOnce(Bounds<Pixels>, &mut Window, &mut App)>;

/// [`show`], running `landed` with where `bounds` lie once the scroll has landed: at once
/// where they are in view already, after the scroll where it moves, and not at all where a
/// scroll or `still` supersedes it, as the PR list's reveal of a row's title then reveals
/// where the title landed in the body ([`Panel::pull_part`]). With `clear`, bounds in view
/// already still move the view where one of its edges cuts a line, to the nearest place that
/// keeps them whole and cuts none ([`clear_of_lines`]), as a reveal's landing does: a list
/// inside the view that scrolled its own rows moved lines under the view's edges without
/// moving the view, and Shift+Tab onto the fourth of twelve PR rows at 13 pt in a 461 × 490
/// window left the row above it cut at the body's top, its title in view.
#[allow(clippy::too_many_arguments)]
fn show_then(
    scroll: &ScrollHandle,
    blocks: &Blocks,
    bounds: Vec<Bounds<Pixels>>,
    inset: Pixels,
    ringed: bool,
    still: impl Fn(&Window, &App) -> bool + 'static,
    mut landed: Option<Landed>,
    clear: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(bounds) = bounds.into_iter().reduce(|a, b| a.union(&b)) else {
        return;
    };
    let viewport = scroll.bounds();
    let (top, bottom) = (bounds.top() - inset, bounds.bottom() + inset);
    #[cfg(test)]
    blocks.kept.replace(Some((
        window.focused(cx),
        top - scroll.offset().y,
        bottom - scroll.offset().y,
    )));
    let in_view = top >= viewport.top() && bottom <= viewport.bottom();
    if in_view {
        if let Some(landed) = landed.take() {
            landed(bounds, window, cx);
        }
        if !clear {
            return;
        }
    }
    let previous = scroll.offset();
    let (scroll, blocks) = (scroll.clone(), blocks.clone());
    // The blocks are known once the container has prepainted, after this part.
    window.defer(cx, move |window, cx| {
        if !still(window, cx) || scroll.offset() != previous || scroll.bounds() != viewport {
            return;
        }
        let edge = landed_edge(
            &scroll,
            &blocks,
            viewport,
            previous,
            (top, bottom),
            inset,
            ringed,
            in_view,
            cx,
        );
        scroll_edge_to(&scroll, viewport, previous, edge, window);
        if let Some(landed) = landed {
            let moved = scroll.offset().y - previous.y;
            landed(bounds + point(px(0.), moved), window, cx);
        }
    });
}

/// Where the top edge of `viewport`, `scroll`'s view at `previous`, lands for a reveal of
/// `top..bottom`, a part with its ring's room `inset` ([`landing`]), clear of the lines
/// `blocks` hold ([`clear_of_lines`]); with `in_view`, where it stands already. Read while the
/// frame prepaints, `blocks` hold the last frame's starts and lines, which the place of what
/// the reveal keeps whole never depends on ([`Reopening::reveal_laid_out`]).
#[allow(clippy::too_many_arguments)]
fn landed_edge(
    scroll: &ScrollHandle,
    blocks: &Blocks,
    viewport: Bounds<Pixels>,
    previous: Point<Pixels>,
    (top, bottom): (Pixels, Pixels),
    inset: Pixels,
    ringed: bool,
    in_view: bool,
    cx: &App,
) -> Pixels {
    let start = viewport.top() + previous.y;
    let end = start + scroll.max_offset().y;
    let starts = blocks.at(scroll);
    let edge = if in_view {
        viewport.top()
    } else {
        landing(viewport, top, bottom, &starts, inset, end, ringed)
    };
    let starts: Vec<_> = starts.into_iter().map(|start| start - inset).collect();
    let lines = blocks.lines(cx);
    clear_of_lines(viewport, edge, (top, bottom), &lines, &starts, (start, end))
}

/// Scrolls `scroll`, at `previous` over `viewport`, so its top edge lies at `edge`, a place
/// in the frame drawn at `previous`, as far as its content allows.
fn scroll_edge_to(
    scroll: &ScrollHandle,
    viewport: Bounds<Pixels>,
    previous: Point<Pixels>,
    edge: Pixels,
    window: &mut Window,
) {
    let offset = point(
        previous.x,
        (previous.y + viewport.top() - edge).clamp(-scroll.max_offset().y, px(0.)),
    );
    if offset != previous {
        scroll.set_offset(offset);
        window.refresh();
    }
}

/// Why a [`RevealMark`] marks what it is laid over.
enum MarkKind {
    /// The part of the body holding the control a reopen keeps focus on.
    Held,
    /// A PR row, for the same reveal in its own list ([`Reopening::holds_row`]).
    Row,
    /// The PR list holding such a row, whose layout gives the list's view.
    List,
    /// What `FocusHandle` focuses while a reopen reveals it, the section's first stop.
    FirstStop(FocusHandle),
    /// The outbound confirmation, while its region holds focus.
    Confirmation(FocusHandle),
}
/// An element laid over a part of the body, absolutely placed so it takes no room, `pad` in
/// from its top and bottom edges where the part has padding of its own: it keeps the part's
/// layout node for the frame's reveal before the body's content prepaints ([`Reopened`]), and
/// whether the part draws focus as a ring on its own edge (`edge`), with the room its reveal
/// keeps clear (`inset`).
pub(super) struct RevealMark {
    reopening: std::rc::Rc<Reopening>,
    edge: Option<FocusHandle>,
    /// The room its reveal keeps clear, or, with `room`, the window's room for it.
    inset: Pixels,
    room: Option<fn(&Window, &App) -> Pixels>,
    pad: gpui::Length,
    kind: MarkKind,
}
/// The room a focus ring needs, for a [`RevealMark`] ([`appearance::button_ring_room`]).
pub(super) fn ring_room(_: &Window, cx: &App) -> Pixels {
    appearance::button_ring_room(cx)
}
/// A mark over the PR list holding the held row ([`Reopening::holds_row`]).
pub(super) fn list_mark(reopening: std::rc::Rc<Reopening>) -> RevealMark {
    RevealMark {
        reopening,
        edge: None,
        inset: Pixels::ZERO,
        room: None,
        pad: px(0.).into(),
        kind: MarkKind::List,
    }
}
impl IntoElement for RevealMark {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for RevealMark {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let style = Style {
            position: Position::Absolute,
            inset: Edges {
                top: self.pad,
                bottom: self.pad,
                left: px(0.).into(),
                right: px(0.).into(),
            },
            ..Style::default()
        };
        let layout = window.request_layout(style, [], cx);
        let ringed = self
            .edge
            .as_ref()
            .is_some_and(|edge| edge.is_focused(window));
        let kind = match &self.kind {
            MarkKind::Held => Some(LaidOutKind::Held),
            MarkKind::Row => Some(LaidOutKind::Row),
            MarkKind::List => {
                self.reopening.list_laid_out.set(Some(layout));
                None
            }
            MarkKind::FirstStop(focus) => (self.reopening.reveals() && focus.is_focused(window))
                .then_some(LaidOutKind::Reveal),
            MarkKind::Confirmation(focus) => {
                (!self.reopening.waits() && focus.is_focused(window)).then_some(LaidOutKind::Reveal)
            }
        };
        if let Some(kind) = kind {
            self.reopening.laid_out.set(Some(LaidOut {
                node: layout,
                ringed,
                inset: self.room.map_or(self.inset, |room| room(window, cx)),
                kind,
            }));
        }
        (layout, ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }
}

/// The body's content, which `scroll` scrolls, laid out for the frame being drawn: in the
/// frame that first draws what a reopen or an opening confirmation focuses, it is revealed
/// whole from that frame's layout before the content prepaints, as the rebase form fits its
/// sequence (`interactive_rebase::Fitted`), so that frame draws it whole
/// ([`Reopening::reveal_laid_out`]). Revealed only once a frame had found the control drawn,
/// the part came into view in the third or fourth frame. Where the reveal moved the body and
/// one of its edges cuts a line this frame draws, the body then moves to the nearest place that
/// keeps the part whole and cuts none, as a reveal's landing does ([`show_then`]).
pub(super) struct Reopened {
    content: AnyElement,
    reopening: std::rc::Rc<Reopening>,
    scroll: ScrollHandle,
    blocks: Blocks,
    revealed: std::rc::Rc<std::cell::RefCell<FocusObservation>>,
    list: ScrollHandle,
    list_blocks: Blocks,
}
/// `content` as the body's content ([`Reopened`]).
pub(super) fn reopened(
    content: impl IntoElement,
    reopening: std::rc::Rc<Reopening>,
    scroll: ScrollHandle,
    blocks: Blocks,
    revealed: std::rc::Rc<std::cell::RefCell<FocusObservation>>,
    list: ScrollHandle,
    list_blocks: Blocks,
) -> Reopened {
    Reopened {
        content: content.into_any_element(),
        reopening,
        scroll,
        blocks,
        revealed,
        list,
        list_blocks,
    }
}
impl IntoElement for Reopened {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for Reopened {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.content.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        // The body has placed its content at its offset already: content the reveal moves is
        // prepainted where the new offset puts it.
        let Some((part, ringed, inset, moved)) = self.reopening.reveal_laid_out(
            &self.scroll,
            &self.blocks,
            &self.list,
            &self.list_blocks,
            &self.revealed,
            window,
            cx,
        ) else {
            self.content.prepaint(window, cx);
            return;
        };
        window.with_element_offset(point(Pixels::ZERO, moved), |window| {
            self.content.prepaint(window, cx)
        });
        let focus = window.focused(cx);
        show_then(
            &self.scroll,
            &self.blocks,
            vec![part],
            inset,
            ringed,
            move |window, cx| window.focused(cx) == focus,
            None,
            true,
            window,
            cx,
        );
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.content.paint(window, cx);
    }
}

/// `element`, named `name` under `key` in its key context, as a part of what `scroll`
/// scrolls: when the keyboard moves focus onto a control in it, and no smaller part under
/// `key` holds that control, `scroll` scrolls to show the part's children, `inset` clear of
/// its edges, its top edge at the start of one of `blocks` ([`reveal`]). The key context names
/// the part on the focus path of the frame that drew the control, so a control needs no focus
/// handle of the panel's. `edge` is the focus whose ring the part draws on its own edge, and
/// `owned` the content below the part that it owns, if any ([`own`]). While the panel reopens
/// (`reopening`), a part of the body holding focus records where it lies, and reveals wait for
/// the reopened focus to settle, then run whatever input came last ([`Reopening`]).
#[allow(clippy::too_many_arguments)]
fn revealing(
    element: Div,
    key: &'static str,
    name: SharedString,
    scroll: ScrollHandle,
    revealed: std::rc::Rc<std::cell::RefCell<FocusObservation>>,
    blocks: Blocks,
    inset: Pixels,
    edge: Option<FocusHandle>,
    owned: Option<Owned>,
    reopening: std::rc::Rc<Reopening>,
) -> Div {
    let mut context = KeyContext::default();
    context.set(key, name.clone());
    let part = name.clone();
    // Its first child, in the frame that first draws the panel reopened onto a control in it.
    let marked = key == PART && reopening.marks(key, &name);
    let element = if marked {
        element.child(RevealMark {
            reopening: reopening.clone(),
            edge: edge.clone(),
            inset,
            room: None,
            pad: px(0.).into(),
            kind: MarkKind::Held,
        })
    } else {
        element
    };
    element
        .key_context(context)
        // Where tests find the part (`github_view::tests::sweep_reveals`).
        .debug_selector(move || format!("github-part-{part}"))
        .on_children_prepainted(move |mut bounds, window, cx| {
            // The marker over the part is none of its children.
            if marked && !bounds.is_empty() {
                bounds.remove(0);
            }
            #[cfg(test)]
            PARTS_DRAWN.with(|parts| parts.borrow_mut().push((name.clone(), edge.clone())));
            #[cfg(test)]
            if let Some(union) = bounds.iter().copied().reduce(|a, b| a.union(&b)) {
                PART_BOUNDS.with_borrow_mut(|parts| parts.push((name.clone(), union)));
            }
            let innermost = window
                .context_stack()
                .iter()
                .rev()
                .find_map(|context| context.get(key).cloned());
            if let Some(owned) = &owned {
                owned.lay_out_part(&bounds, &scroll);
            }
            if innermost.as_ref() != Some(&name) {
                return;
            }
            if key == PART {
                reopening.note(&bounds);
            }
            if reopening.waits() || !(window.last_input_was_keyboard() || reopening.reveals()) {
                return;
            }
            let Some(owned) = owned.clone() else {
                reveal(
                    &scroll,
                    &revealed,
                    &blocks,
                    bounds,
                    inset,
                    edge.as_ref(),
                    window,
                    cx,
                );
                return;
            };
            // What the part owns lies after it, so it is laid out in this frame only once the
            // part has prepainted: the reveal takes both from this frame, once it has. Taken from
            // the frame before, the content's place, which a resize or a text size moves,
            // combined with this frame's part, could show the part with content no longer
            // there.
            let Some((ringed, focus)) = to_reveal(&scroll, &revealed, edge.as_ref(), window, cx)
            else {
                return;
            };
            let (scroll, blocks) = (scroll.clone(), blocks.clone());
            window.defer(cx, move |window, cx| {
                let mut bounds = bounds;
                own(&mut bounds, &owned, &scroll, inset);
                show(
                    &scroll,
                    &blocks,
                    bounds,
                    inset,
                    ringed,
                    move |window, cx| window.focused(cx) == focus,
                    window,
                    cx,
                );
            });
        })
}

/// Content that no control holds, which the part before it owns ([`own`]): the stops on
/// either side of it cannot always show it whole with every line whole, as Overview's captured
/// identities, six lines at 17 pt in a 440 × 490 window, lay between the last section tab's
/// reveal and the Conversations pane's, which no stop showed. Where the part and the content
/// fit their container's view together, Tab onto the part shows both; where they cannot, as
/// the identities cannot below about 430 px tall at 17 pt, the content is a group stop of its
/// own, revealed from its top ([`Owned::apart`]). Each place is the frame being drawn's, top
/// and bottom with the container's scroll at zero.
#[derive(Clone, Default)]
pub(super) struct Owned {
    /// Where the content lies, none where the frame draws none of it.
    content: std::rc::Rc<std::cell::Cell<Option<(Pixels, Pixels)>>>,
    /// Where the part that owns it lies.
    part: std::rc::Rc<std::cell::Cell<Option<(Pixels, Pixels)>>>,
    /// Whether the content cannot fit the view with the part, as the last frame that drew
    /// them laid them out.
    apart: std::rc::Rc<std::cell::Cell<bool>>,
}
impl Owned {
    /// Where the content lies in the frame being drawn, or none where it draws none.
    pub(super) fn content(&self) -> Option<(Pixels, Pixels)> {
        self.content.get()
    }
    /// Whether the content is a group stop of its own, which the frame being rendered makes
    /// it: it cannot fit the view with the part that owns it, as the last frame that drew
    /// them laid them out ([`Owned::lay_out_content`]).
    pub(super) fn apart(&self) -> bool {
        self.apart.get()
    }
    /// Forgets the content, which the frame being rendered does not draw.
    pub(super) fn forget(&self) {
        self.content.set(None);
    }
    /// Records where the part that owns the content lies, its children `bounds` in `scroll`'s
    /// container as the frame prepaints them, before the content.
    fn lay_out_part(&self, bounds: &[Bounds<Pixels>], scroll: &ScrollHandle) {
        let offset = scroll.offset().y;
        let part = bounds.iter().copied().reduce(|a, b| a.union(&b));
        self.part
            .set(part.map(|part| (part.top() - offset, part.bottom() - offset)));
    }
    /// For the `on_children_prepainted` of the element holding the content, which prepaints
    /// after the part: records where its first child lies in `scroll`'s container and
    /// whether it fits the view with the part and `inset`, the ring's room, above and below.
    /// Where that differs from what the frame was rendered with, the window renders again, so
    /// the content is a stop of its own exactly where it cannot fit.
    pub(super) fn lay_out_content(
        &self,
        scroll: ScrollHandle,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        let owned = self.clone();
        move |children, window, cx| {
            let offset = scroll.offset().y;
            let content = children
                .first()
                .map(|drawn| (drawn.top() - offset, drawn.bottom() - offset));
            owned.content.set(content);
            let (Some(content), Some(part)) = (content, owned.part.get()) else {
                return;
            };
            let inset = crate::appearance::button_ring_room(cx);
            let apart = !fits_together(part, content, inset, scroll.bounds().size.height);
            if owned.apart.replace(apart) != apart {
                window.defer(cx, |window, _| window.refresh());
            }
        }
    }
}

/// Whether `part` and `content`, each a top and bottom, fit a view `height` tall together with
/// `inset` above and below.
fn fits_together(
    part: (Pixels, Pixels),
    content: (Pixels, Pixels),
    inset: Pixels,
    height: Pixels,
) -> bool {
    part.1.max(content.1) - part.0.min(content.0) + inset * 2. <= height
}

/// The room [`Panel::reveal_on_focus`] keeps clear of the body's edges around a form's content:
/// a line's room, which holds the form's padding, edge and focus ring.
pub(super) fn reveal_room(window: &Window, _: &App) -> Pixels {
    window.rem_size() + px(2.)
}

/// Adds to `bounds`, the children of a part focus has moved into, the content it owns below
/// it (`owned`), where the part and that content fit `scroll`'s view together with `inset`,
/// the ring's room, above and below: the reveal then shows them both, and the content is
/// whole at the part's stop. Where they do not fit, the part is revealed alone, so its stop
/// still shows it whole, and the content is a stop of its own ([`Owned::apart`]). Both places
/// are this frame's, read once the frame has prepainted the content.
fn own(bounds: &mut Vec<Bounds<Pixels>>, owned: &Owned, scroll: &ScrollHandle, inset: Pixels) {
    let (Some(content), Some(part)) = (
        owned.content(),
        bounds.iter().copied().reduce(|a, b| a.union(&b)),
    ) else {
        return;
    };
    let offset = scroll.offset().y;
    let (top, bottom) = (content.0 + offset, content.1 + offset);
    if fits_together(
        (part.top(), part.bottom()),
        (top, bottom),
        inset,
        scroll.bounds().size.height,
    ) {
        bounds.push(Bounds::from_corners(
            point(part.left(), top),
            point(part.right(), bottom),
        ));
    }
}

impl Panel {
    /// The mark over what the shown section's first stop reveals as a reopen lands on it, as
    /// tall as what `focus` holds, `room` clear of the body's edges in that frame's own layout
    /// ([`Reopening::reveal_laid_out`]), `ringed` where it draws focus as a ring on its own
    /// edge, and inside the padding of a pane it stands in (`padded`).
    pub(super) fn stop_mark(
        &self,
        focus: &FocusHandle,
        room: fn(&Window, &App) -> Pixels,
        ringed: bool,
        padded: bool,
    ) -> RevealMark {
        RevealMark {
            reopening: self.review.reopening.clone(),
            edge: ringed.then(|| focus.clone()),
            inset: Pixels::ZERO,
            room: Some(room),
            pad: if padded {
                rems(0.75).into()
            } else {
                px(0.).into()
            },
            kind: MarkKind::FirstStop(focus.clone()),
        }
    }
    /// The mark over the outbound confirmation, inside its pane's padding, while its region
    /// `focus` holds focus: revealed from its frame's own layout with `inset` clear of the
    /// body's edges, its top first where it is taller than the body, as the region's ring is
    /// on its own edge ([`Reopening::reveal_laid_out`]).
    pub(super) fn confirmation_mark(&self, focus: &FocusHandle, inset: Pixels) -> RevealMark {
        RevealMark {
            reopening: self.review.reopening.clone(),
            edge: Some(focus.clone()),
            inset,
            room: None,
            pad: rems(0.75).into(),
            kind: MarkKind::Confirmation(focus.clone()),
        }
    }
    /// For the `on_children_prepainted` of the element holding a form that `focus` tracks,
    /// such as the reply composer, or holding its stop, as the outbound confirmation holds its
    /// region ([`Panel::reveal_ringed_on_focus`]), which report the content inside their
    /// padding: when focus moves into it, by the keyboard or not, the body scrolls to show its
    /// children with a line's room clear of its edges ([`reveal`]), which holds the form's
    /// padding, edge and focus ring, and not at all where they are in view with that room.
    pub(super) fn reveal_on_focus(
        &self,
        focus: FocusHandle,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        self.reveal_focus(focus, reveal_room, false)
    }
    /// [`Panel::reveal_on_focus`] for the element holding a stop that draws its ring on its own
    /// edge at its top, as the outbound confirmation's region does: taller than the body, its
    /// children show from their top, the ring's top edge in view.
    pub(super) fn reveal_ringed_on_focus(
        &self,
        focus: FocusHandle,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        self.reveal_focus(focus, reveal_room, true)
    }
    /// [`Panel::reveal_on_focus`] for the element holding a field alone, whose edge is its own,
    /// as the destination field and the review summary are: the body scrolls only where the
    /// field and its focus ring are not whole in view, and then by the least that shows them,
    /// as the panel reopens onto either. With a line's room, the panel reopening onto its
    /// destination field at 18 pt in a 461 × 490 window scrolled the body 112 px although the
    /// field was whole at its top, the banner out of view.
    pub(super) fn reveal_field_on_focus(
        &self,
        focus: FocusHandle,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        self.reveal_focus(focus, |_, cx| appearance::button_ring_room(cx), false)
    }
    /// Reveals what `focus` moves into with `room(window, cx)` clear of the body's edges, from
    /// its top where it is taller than the body and `ringed`, `focus` drawing its ring on its
    /// own edge. While the panel reopens onto it, it records where it lies and waits as the
    /// body's parts do ([`Reopening`]).
    fn reveal_focus(
        &self,
        focus: FocusHandle,
        room: fn(&Window, &App) -> Pixels,
        ringed: bool,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        let scroll = self.review.body_scroll.clone();
        let revealed = self.review.revealed_focus.clone();
        let blocks = self.review.body_blocks.clone();
        let reopening = self.review.reopening.clone();
        move |bounds, window, cx| {
            if focus.contains_focused(window, cx) {
                reopening.note(&bounds);
                if reopening.waits() {
                    return;
                }
                let inset = room(window, cx);
                let edge = ringed.then_some(&focus);
                reveal(&scroll, &revealed, &blocks, bounds, inset, edge, window, cx);
            }
        }
    }
    /// For the element holding an open PR's header and section tabs, which report the body's
    /// blocks: in the first frame after the PR opens ([`ReviewState::opened`]), where the line
    /// holding its title, which starts at `title`, and the tabs are not both in view, the body
    /// scrolls so that line is at its top, the title whole and the PR's first content below it
    /// rather than the controls above, as in a 461 × 490 window, where the PR once opened
    /// showing only its title. Focus stays on the section the PR opens to, without revealing
    /// it, which would take the title out of view again.
    pub(super) fn reveal_opened(
        &self,
        title: std::rc::Rc<std::cell::Cell<Pixels>>,
        cx: &App,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        let scroll = self.review.body_scroll.clone();
        let revealed = self.review.revealed_focus.clone();
        let blocks = self.review.body_blocks.clone();
        let opened = self.review.opened.clone();
        let inset = appearance::button_ring_room(cx);
        move |children, window, cx| {
            blocks.report(&children);
            let Some(tabs) = children.last() else {
                return;
            };
            if !opened.replace(false) {
                return;
            }
            let viewport = scroll.bounds();
            *revealed.borrow_mut() = FocusObservation {
                focus: window.focused(cx),
                viewport,
                rem_size: window.rem_size(),
            };
            let top = title.get() - inset;
            if top >= viewport.top() && tabs.bottom() + inset <= viewport.bottom() {
                return;
            }
            let previous = scroll.offset();
            let scroll = scroll.clone();
            window.defer(cx, move |window, _| {
                if scroll.offset() == previous && scroll.bounds() == viewport {
                    scroll_edge_to(&scroll, viewport, previous, top, window);
                }
            });
        }
    }
    /// `element` as the part of the panel's body called `name`. When the keyboard moves
    /// focus onto a control in it, and no smaller part inside it holds that control, the
    /// body scrolls by the least that shows the part's children with their focus rings. The
    /// part's key context names it on the focus path of the frame that drew the control, so
    /// a control needs no focus handle of the panel's to be revealed.
    pub(super) fn part(&self, name: impl Into<SharedString>, element: Div, cx: &App) -> Div {
        revealing(
            element,
            PART,
            name.into(),
            self.review.body_scroll.clone(),
            self.review.revealed_focus.clone(),
            self.review.body_blocks.clone(),
            appearance::button_ring_room(cx),
            None,
            None,
            self.review.reopening.clone(),
        )
    }
    /// [`Panel::part`] for a pane that draws `focus`'s ring on its own edge: taller than the
    /// body, it shows its top, the ring's top edge in view.
    pub(super) fn ringed_part(
        &self,
        name: &'static str,
        element: Div,
        focus: &FocusHandle,
        cx: &App,
    ) -> Div {
        revealing(
            element,
            PART,
            name.into(),
            self.review.body_scroll.clone(),
            self.review.revealed_focus.clone(),
            self.review.body_blocks.clone(),
            appearance::button_ring_room(cx),
            Some(focus.clone()),
            None,
            self.review.reopening.clone(),
        )
    }
    /// [`Panel::part`] for a part that owns the content below it (`owned`): Tab onto it
    /// shows that content with it where both fit the body ([`own`]).
    pub(super) fn owning_part(
        &self,
        name: &'static str,
        element: Div,
        owned: &Owned,
        cx: &App,
    ) -> Div {
        revealing(
            element,
            PART,
            name.into(),
            self.review.body_scroll.clone(),
            self.review.revealed_focus.clone(),
            self.review.body_blocks.clone(),
            appearance::button_ring_room(cx),
            None,
            Some(owned.clone()),
            self.review.reopening.clone(),
        )
    }
    /// `element`, a PR row's title, as the part of the PR list called `name`, which scrolls
    /// inside the panel's body and clips its rows past its 206 px cap: when the keyboard moves
    /// focus onto it, the list first scrolls by the least that shows the title with its ring's
    /// room, its row's lines whole at the list's edges where a place allows ([`reveal`]), and the
    /// body then shows where the title landed, with the ring's room and nothing more: neither
    /// the list's padding, which left Previous PR page cut at the body's foot at 17 pt in a
    /// 461 × 490 window, nor a place the list clips, to which the body once scrolled for a row
    /// below the list's fold, away from the list. A change of the body's view, as a resize
    /// makes, reveals the focused title again, as the body's own parts are.
    pub(super) fn pull_part(&self, name: SharedString, element: Div, cx: &App) -> Div {
        let mut context = KeyContext::default();
        context.set(PULL, name.clone());
        let part = name.clone();
        let (list, revealed, blocks) = (
            self.review.pull_scroll.clone(),
            self.review.pull_revealed.clone(),
            self.review.pull_blocks.clone(),
        );
        let (body, body_revealed, body_blocks) = (
            self.review.body_scroll.clone(),
            self.review.revealed_focus.clone(),
            self.review.body_blocks.clone(),
        );
        let reopening = self.review.reopening.clone();
        let inset = appearance::button_ring_room(cx);
        // Marked, in the frame that first draws the panel reopened onto this row.
        let marked = reopening.marks(PULL, &name);
        let element = if marked {
            element.child(RevealMark {
                reopening: reopening.clone(),
                edge: None,
                inset,
                room: None,
                pad: px(0.).into(),
                kind: MarkKind::Row,
            })
        } else {
            element
        };
        element
            .key_context(context)
            // Where tests find the part (`github_view::tests::over_wide`).
            .debug_selector(move || format!("github-part-{part}"))
            .on_children_prepainted(move |mut bounds, window, cx| {
                // The marker over the row is none of its children.
                if marked && !bounds.is_empty() {
                    bounds.remove(0);
                }
                #[cfg(test)]
                PARTS_DRAWN.with(|parts| parts.borrow_mut().push((name.clone(), None)));
                #[cfg(test)]
                if let Some(union) = bounds.iter().copied().reduce(|a, b| a.union(&b)) {
                    PART_BOUNDS.with_borrow_mut(|parts| parts.push((name.clone(), union)));
                }
                let innermost = window
                    .context_stack()
                    .iter()
                    .rev()
                    .find_map(|context| context.get(PULL).cloned());
                if innermost.as_ref() != Some(&name) {
                    return;
                }
                // Where the panel reopens onto the title, the part of it the list shows.
                let shown = list.bounds();
                let visible: Vec<_> = bounds
                    .iter()
                    .map(|title| title.intersect(&shown))
                    .filter(|title| title.size.height > Pixels::ZERO)
                    .collect();
                reopening.note(&visible);
                if reopening.waits() || !(window.last_input_was_keyboard() || reopening.reveals()) {
                    return;
                }
                let due = to_reveal(&list, &revealed, None, window, cx)
                    .or_else(|| to_reveal(&body, &body_revealed, None, window, cx));
                let Some((_, focus)) = due else {
                    return;
                };
                let still = move |window: &Window, cx: &App| window.focused(cx) == focus;
                let (body, body_revealed, body_blocks) =
                    (body.clone(), body_revealed.clone(), body_blocks.clone());
                let then = still.clone();
                let stood = bounds.iter().copied().reduce(|a, b| a.union(&b));
                let landed: Landed = Box::new(move |title, window, cx| {
                    // Where the list has moved the title, the body's lines, its rows among them,
                    // are the frame's before it moved: the next frame, which lays them out where
                    // the list put them, reveals the title in the body. Revealed with this
                    // frame's, Shift+Tab onto a row at 13 pt left the row above it cut at the
                    // body's top.
                    if stood.is_some_and(|stood| (stood.top() - title.top()).abs() > px(0.01)) {
                        *body_revealed.borrow_mut() = FocusObservation::default();
                        window.refresh();
                        return;
                    }
                    // Where the title is in the body's view already, the rows the list moved can
                    // lie cut at the body's edges: the body moves to clear them all the same.
                    show_then(
                        &body,
                        &body_blocks,
                        vec![title],
                        inset,
                        false,
                        then,
                        None,
                        true,
                        window,
                        cx,
                    );
                });
                show_then(
                    &list,
                    &blocks,
                    bounds,
                    inset,
                    false,
                    still,
                    Some(landed),
                    false,
                    window,
                    cx,
                );
            })
    }
    /// `element`, the actions of the conversation at `index`, as a part of the conversation
    /// list, which scrolls inside the panel's body: when the keyboard moves focus onto one of
    /// them, the list scrolls by the least that shows them with their focus rings, as the
    /// body does for its parts. The list is a part of the body ([`Panel::part`]), so the body
    /// shows the list while the list shows the actions.
    pub(super) fn conversation_part(&self, index: usize, element: Div, cx: &App) -> Div {
        revealing(
            element,
            CONVERSATION,
            index.to_string().into(),
            self.conversations.list_scroll.clone(),
            self.conversations.list_revealed.clone(),
            self.conversations.list_blocks.clone(),
            appearance::button_ring_room(cx),
            None,
            None,
            self.review.reopening.clone(),
        )
    }
    /// `button`, `disabled` or not, as a control whose press can take away a pane that holds
    /// focus, as Account options closes the connection pane, New pull request a PR's
    /// Conversations pane, and Files' modes and pages Threads' pane: it tracks `focus`, which
    /// its press records as the panel's closer ([`ReviewState::closer`]), and the panel gives
    /// that focus whatever focus the pane held as it goes, where the frame draws the control
    /// enabled ([`Panel::hand_off_lost_pane_focus`]).
    pub(super) fn closing(
        &self,
        button: gpui_kit::component::button::Button,
        focus: &FocusHandle,
        disabled: bool,
    ) -> gpui_kit::component::button::Button {
        if !disabled {
            self.review.closers_drawn.borrow_mut().push(focus.clone());
        }
        button.track_focus(focus).disabled(disabled)
    }
    /// The panel's [`button`] `id` in its body, tracking the focus handle the panel keeps for
    /// it ([`Buttons`]), so reopening gives focus back to it.
    pub(super) fn button(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        symbol: &str,
        active: bool,
        cx: &App,
    ) -> gpui_kit::component::button::Button {
        let id = id.into();
        let focus = self.review.buttons.focus(id.clone(), cx);
        button(id, label, symbol, active).track_focus(&focus)
    }
    /// `button`, a Button of the body built by a helper of its own, as a copy button is,
    /// tracking the focus handle the panel keeps for `id` ([`Buttons`]).
    pub(super) fn held_button(
        &self,
        id: impl Into<ElementId>,
        button: gpui_kit::component::button::Button,
        cx: &App,
    ) -> gpui_kit::component::button::Button {
        button.track_focus(&self.review.buttons.focus(id, cx))
    }
    /// Whether the pane that `focus` tracks draws the window's keyboard ring:
    /// after a key, and still while the pointer moves, until a pointer press.
    pub(super) fn ringed(&self, focus: &FocusHandle, cx: &App) -> bool {
        self.owner
            .upgrade()
            .is_some_and(|owner| owner.read(cx).focus_ring.shows(focus))
    }
    /// Puts focus back as the panel reopens: on the control that held it inside the body's
    /// view as the panel closed ([`ReviewState::held`]), where it still exists, or otherwise
    /// on the shown section's first stop ([`Panel::focus_visible_section`]). Either is
    /// revealed whole, whatever input came last, from the first frame's layout, and the held
    /// control keeps focus only where that frame draws it inside the body's view; elsewhere,
    /// as after a text size change while the panel was closed moved it out of view, focus
    /// goes to the section's first stop ([`Reopening`]).
    pub(super) fn focus_reopened(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.review.held.take().and_then(|held| held.upgrade()) {
            Some(held) => {
                *self.review.revealed_focus.borrow_mut() = Default::default();
                held.focus(window, cx);
                self.review.reopening.hold(self.review.held_part.take());
            }
            None => {
                self.focus_visible_section(window, cx);
                self.review.reopening.reveal();
            }
        }
        cx.notify();
    }
    /// Whether reopening, with no PR open or new PR started, puts focus on the destination
    /// field ([`Self::focus_reopened`]): no control held focus inside the body's view as the
    /// panel closed, or the field did.
    pub(super) fn reopens_onto_destination(&self, cx: &App) -> bool {
        self.pull.is_none()
            && !self.create
            && self
                .review
                .held
                .as_ref()
                .and_then(|held| held.upgrade())
                .is_none_or(|held| held == self.destination.read(cx).focus_handle(cx))
    }
    /// Whether `focus` is the outbound confirmation's region, Send or Keep editing, while a
    /// confirmation is open.
    fn confirmation_holds(&self, focus: &FocusHandle) -> bool {
        self.confirm.is_some()
            && (*focus == self.conversations.confirm_focus
                || ["github-confirm-send", "github-edit-submission"]
                    .into_iter()
                    .any(|id| self.review.buttons.kept(id).as_ref() == Some(focus)))
    }
    /// Records the control that holds focus as the panel closes, if the last frame drew it
    /// inside the body's view, for reopening to give focus back to ([`Self::focus_reopened`]).
    /// A control that cannot take focus back holds nothing, so the reopen goes straight to the
    /// shown section's first stop: the confirmation's controls, which reopening drops, and the
    /// busy row's Cancel, which closing takes away with its read, where focus sat on what no
    /// frame drew until the fallback moved it two frames later; and a
    /// Button disabled as the panel closes, no Tab stop, such as Next PR page on the last page,
    /// which the reopen once focused. A read in flight disables a Button only until closing
    /// cancels it, so with one under way a control that is no Tab stop is held all the same,
    /// and the reopen keeps the person's place on Refresh PRs, Refresh conversations or Next
    /// conversations; one the read did not alone disable fails [`Reopening::settle`]'s check.
    pub(super) fn hold_closing_focus(&mut self, window: &Window, cx: &App) {
        let shown = self
            .review
            .reopening
            .shows_focus(&self.review.body_scroll, window);
        self.review.held = window
            .focused(cx)
            .filter(|focus| {
                shown
                    && (focus.tab_stop || self.pending)
                    && focus != self.cancel_request.handle()
                    && !self.confirmation_holds(focus)
            })
            .map(|focus| focus.downgrade());
        // The innermost part on the focus path of the frame the window last drew, the panel's.
        self.review.held_part = self.review.held.as_ref().and_then(|_| {
            window.context_stack().iter().rev().find_map(|context| {
                context
                    .get(PART)
                    .map(|name| (PART, name.clone()))
                    .or_else(|| context.get(PULL).map(|name| (PULL, name.clone())))
            })
        });
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
    pub(super) fn change_section(
        &mut self,
        section: Section,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.save_draft(window, cx);
        self.review.section = section;
        self.confirm = None;
        self.focus_visible_section(window, cx);
        cx.notify();
    }
    /// Loads page `page` of the PR's files, asked for by Previous (`side` 0) or Next (1), which
    /// takes focus Threads' Conversations pane held as the page replaces it
    /// ([`ReviewState::closer`]).
    fn file_page(&mut self, page: u32, side: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pull) = self.captured(cx) else {
            return;
        };
        let fixture = self.review.fixture;
        let moved = self.review.fixture_moved;
        self.run(
            RequestKind::Load,
            move |control| Client(UiTransport::open(fixture, moved)?).files(&pull, page, &control),
            move |result, this, _, _| match result {
                Ok(page) => {
                    this.review.closer = Some(this.review.file_page_focus[side].clone());
                    this.receive_file_page(page)
                }
                Err(error) => this.error = Some(format!("{error:#}")),
            },
            window,
            cx,
        );
    }
    pub(super) fn receive_file_page(&mut self, page: github::Page<PullFile>) {
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
                    // No request carried it, so it answers on the panel's top line rather
                    // than where the last request was asked for.
                    Err(error) => this.local_read_failed(format!("{error:#}")),
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
            Err(error) => self.refuse(error.to_string()),
        }
        cx.notify();
    }
    fn add_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut comment) = self.review.composing.clone() else {
            return;
        };
        comment.body = self.review.input.read(cx).value().to_string();
        if let Err(error) = github::validate_text(&comment.body, true) {
            self.refuse(error.to_string());
            cx.notify();
            return;
        }
        if self.review.comments.len() >= 100 {
            self.refuse("A review supports at most 100 inline comments. Finish this review before adding more.");
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
    pub(super) fn review_source(
        &mut self,
        mode: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        // Focus Threads' Conversations pane holds, as a click inside it leaves it, is the panel's
        // guard's to hand on as the pane goes ([`Panel::hand_off_lost_pane_focus`]).
        if !self.review.section_focus.contains_focused(window, cx) {
            self.focus_file_content(window, cx);
        }
        cx.notify();
    }
    pub(super) fn render_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let count = self.review.comments.len();
        self.part("tabs", div(), cx)
            .flex()
            .flex_wrap()
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
                    // Each tab is a part of its own: where the tabs wrap, their part may be
                    // taller than the body of a short window at a large text size, and Tab
                    // shows the tab it moves onto with its ring.
                    // The last tab owns Overview's captured identities below the tabs, which
                    // no other stop can show whole ([`Owned`]).
                    let part = if i == 2 {
                        self.owning_part("section-2", div(), &self.review.identities, cx)
                    } else {
                        self.part(["section-0", "section-1"][i], div(), cx)
                    };
                    part.child(
                        self.button(
                            ("github-section", i),
                            name,
                            "",
                            self.review.section == section,
                            cx,
                        )
                        .debug_selector(move || format!("github-section-{i}"))
                        .toggled(self.review.section == section)
                        .on_click(cx.listener(
                            move |this, _, window, cx| this.change_section(section, window, cx),
                        )),
                    )
                }),
            )
            .child(self.render_save_status(cx))
            .into_any_element()
    }
    /// The draft status at the end of the section tabs' last line, in the room the tabs leave
    /// there, ending where the Buttons at the panel's right edge end their labels. Its basis is
    /// zero and its margin gives back the row's gap before it, so it takes only the room the
    /// line leaves, down to its two insets and an ellipsis: where the tabs leave less, it starts
    /// a line of its own below them, empty or not, so nothing moves as Reply saves the drafts
    /// and says so (a status that took its line only with text moved the rows below the tabs
    /// as it appeared, 39 px at 461 px when it always wrapped). One line, elided in its middle
    /// where it is longer than its room, with the whole text in its tooltip and accessible
    /// name, as a long failure's remedy is there (decision of 2026-11-01).
    fn render_save_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let status = self.save_status.clone();
        let tooltip = status.clone();
        // At least its insets and an ellipsis, a 1 em allowance in its 12 px text: where the
        // tabs leave less on their last line, it starts a line of its own below them, whatever
        // it says, rather than draw nothing there.
        let least = super::text_inset() * 2. + appearance::ui_text(12.);
        div()
            .flex()
            .justify_end()
            .flex_grow(1.)
            .flex_basis(px(0.))
            .min_w(least)
            .overflow_hidden()
            .ml(rems(-0.25))
            .child(
                crate::views::ellipsis_middle(
                    label("github-local-save", status.clone())
                        .debug_selector(|| "github-local-save".into())
                        .min_w_0()
                        .mx(super::text_inset())
                        .text_color(rgb(palette(cx).muted)),
                )
                .when(!status.is_empty(), |status| {
                    status.tooltip(move |window, cx| {
                        #[cfg(test)]
                        SAVE_STATUS_TOOLTIP
                            .with(|shown| *shown.borrow_mut() = Some(tooltip.clone()));
                        Tooltip::new(tooltip.clone()).build(window, cx)
                    })
                }),
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
            .child(self.stop_mark(
                &self.review.input.read(cx).focus_handle(cx),
                reveal_room,
                false,
                true,
            ))
            .map(super::bordered_pane)
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
                div().child(editor_rows(&self.review.input)).child(
                    Textarea::new(&self.review.input)
                        .h(appearance::ui_size(100.))
                        .aria_label(
                            "Inline review comment, saved locally with its captured position",
                        )
                        .disabled(self.pending),
                ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        self.button("github-add-comment", "Add to review", "plus", true, cx)
                            .disabled(self.pending)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.add_comment(window, cx)),
                            ),
                    )
                    .child(
                        self.button(
                            "github-discard-composer",
                            "Discard this comment",
                            "",
                            false,
                            cx,
                        )
                        .disabled(self.pending)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.discard_composer(window, cx)),
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
        let fit = self.files_fit(selected.is_some(), window, cx);
        let file_list = self
            .part("file-list", div(), cx)
            .map(|list| {
                if fit.stacked {
                    list.w_full()
                } else {
                    list.w(appearance::ui_size(210.)).min_w(px(150.))
                }
            })
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_2()
            // The list's box is a part of its own, so Tab onto the list shows the box with its
            // ring's room and leaves its pager, a stop of its own, to its own reveal.
            .child(
                self.part("file-rows", div(), cx)
                    .child(self.stop_mark(&self.review.file_focus, ring_room, false, false))
                    .flex()
                    .flex_col()
                    .child(crate::native_accessibility::clips_children(
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
                            .h(fit.list)
                            .border_1()
                            .border_color(rgb(if self.ringed(&self.review.file_focus, cx) {
                                p.accent
                            } else {
                                p.border
                            }))
                            .rounded(px(6.))
                            .overflow_hidden()
                            .track_focus(&self.review.file_focus)
                            .role(Role::ListBox)
                            .aria_label("Pull request changed files")
                            .aria_description(format!(
                                "Up and Down select a file. {} moves to its patch.",
                                shortcuts::ENTER
                            ))
                            // Its rows are lines of the body where the list shows them, the part it
                            // shows of one its edge cuts: the rows it has scrolled out of itself are
                            // none ([`clipped`], [`keep_line`]).
                            .child(
                                clipped(
                                    uniform_list(
                                        "github-file-list",
                                        self.review.files.len(),
                                        cx.processor(
                                            |this, range: std::ops::Range<usize>, _, cx| {
                                                range
                                                    .map(|index| {
                                                        let file = &this.review.files[index];
                                                        let selected = this.review.selected_file
                                                            == Some(index);
                                                        let path = file.filename.clone();
                                                        let hover = palette(cx).row_hover(selected);
                                                        div()
                                                            .id(("github-file", index))
                                                            .debug_selector(move || {
                                                                format!("github-file-row-{index}")
                                                            })
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
                                                            .hover(move |style| {
                                                                style.bg(rgb(hover))
                                                            })
                                                            .role(Role::ListBoxOption)
                                                            .aria_label(format!(
                                                                "{} · {} · {}, {}",
                                                                file.filename,
                                                                file.status,
                                                                crate::counted(
                                                                    file.additions,
                                                                    "addition",
                                                                    "additions"
                                                                ),
                                                                crate::counted(
                                                                    file.deletions,
                                                                    "deletion",
                                                                    "deletions"
                                                                )
                                                            ))
                                                            .aria_selected(selected)
                                                            .cursor_pointer()
                                                            // The row is one line of the body: its two
                                                            // lines of text stand on its own surface.
                                                            .child(one_line())
                                                            .child(
                                                                div()
                                                                    .text_size(appearance::ui_text(
                                                                        12.,
                                                                    ))
                                                                    .font_weight(FontWeight::MEDIUM)
                                                                    .truncate()
                                                                    .child(path.clone()),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_size(appearance::ui_text(
                                                                        10.,
                                                                    ))
                                                                    .text_color(rgb(
                                                                        palette(cx).muted
                                                                    ))
                                                                    .child(format!(
                                                                        "{} · +{} −{}",
                                                                        file.status,
                                                                        file.additions,
                                                                        file.deletions
                                                                    )),
                                                            )
                                                            .tooltip(move |window, cx| {
                                                                Tooltip::new(path.clone())
                                                                    .build(window, cx)
                                                            })
                                                            .on_click(cx.listener(
                                                                move |this, _, window, cx| {
                                                                    this.select_review_file(
                                                                        index, window, cx,
                                                                    )
                                                                },
                                                            ))
                                                            .into_any_element()
                                                    })
                                                    .collect::<Vec<_>>()
                                            },
                                        ),
                                    )
                                    .size_full()
                                    .track_scroll(&self.review.file_scroll),
                                )
                                .debug_selector(|| "github-file-rows".into())
                                .size_full(),
                            ),
                    )),
            )
            .child(
                self.part("file-pager", div(), cx)
                    .min_h(appearance::ui_size(PAGER_ROW))
                    .flex()
                    .items_center()
                    .gap_1()
                    // The page either loads replaces Threads' Conversations pane.
                    .child(
                        self.closing(
                            button("github-files-previous", "Previous", "", false),
                            &self.review.file_page_focus[0],
                            self.pending || self.review.file_page <= 1,
                        )
                        .debug_selector(|| "github-files-previous".into())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.file_page(this.review.file_page - 1, 0, window, cx)
                        })),
                    )
                    .child(
                        label("github-files-page", format!("{}", self.review.file_page))
                            .debug_selector(|| "github-files-page".into()),
                    )
                    .child(
                        self.closing(
                            button("github-files-next", "Next", "", false),
                            &self.review.file_page_focus[1],
                            self.pending || !self.review.files_next,
                        )
                        .debug_selector(|| "github-files-next".into())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.file_page(this.review.file_page + 1, 1, window, cx)
                        })),
                    ),
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
            // Stacked below the list in a narrow body, the box takes the rest of the body
            // ([`Panel::files_fit`]). The column's text starts at one left edge, where the mode
            // buttons at its edge start their labels, as the panel's does; the boxes keep the
            // column's edge.
            let measured = self.review.files_fit.clone();
            // The part holds the file's title, mode row and box and Changes' range hint, as tall
            // as they are, inside the column, which tests find by a selector of its own, so the
            // part keeps the one it names itself by ([`revealing`]): the reveal sweep reads it
            // as what a reveal of the file shows, and never found it while the column's
            // selector replaced it.
            let content = self
                .part("file-content", div(), cx)
                .min_w_0()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .when(self.review.mode < 2, |content| content.h(fit.content))
                        // The title and the mode row above the box, which the box's height
                        // leaves its five lines below.
                        .on_children_prepainted(move |children, window, cx| {
                            if let [title, _, content] = children[..] {
                                let above = content.top() - title.top();
                                FilesFit::update(
                                    &measured,
                                    |fit| fit.above = above,
                                    false,
                                    window,
                                    cx,
                                );
                            }
                        })
                        .child(
                            label("github-file-title", title)
                                .debug_selector(|| "github-file-title".into())
                                .ml(super::text_inset())
                                .flex_shrink_0()
                                .font_weight(FontWeight::SEMIBOLD),
                        )
                        .child(
                            self.part("file-modes", div(), cx)
                                .flex_shrink_0()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap_1()
                                .children(
                                    ["Changes", "Source", "Threads"]
                                        .into_iter()
                                        .enumerate()
                                        .map(|(mode, name)| {
                                            self.closing(
                                                button(
                                                    ("github-file-mode", mode),
                                                    name,
                                                    "",
                                                    self.review.mode == mode,
                                                ),
                                                &self.review.mode_focus[mode],
                                                false,
                                            )
                                            .debug_selector(move || {
                                                format!("github-file-mode-{mode}")
                                            })
                                            .toggled(self.review.mode == mode)
                                            .on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    // Another mode replaces Threads'
                                                    // Conversations pane.
                                                    this.review.closer =
                                                        Some(this.review.mode_focus[mode].clone());
                                                    this.review_source(mode, window, cx)
                                                }),
                                            )
                                        }),
                                )
                                .child(div().flex_1())
                                .child(
                                    self.button(
                                        "github-inline-start",
                                        if self.review.composing.is_some() {
                                            "Continue comment"
                                        } else {
                                            "Comment on selection"
                                        },
                                        "plus",
                                        false,
                                        cx,
                                    )
                                    .debug_selector(|| "github-inline-start".into())
                                    .disabled(
                                        self.pending
                                            || self.review.preparing
                                            || file.unavailable.is_some(),
                                    )
                                    .on_click(cx.listener(
                                        |this, _, window, cx| this.begin_comment(window, cx),
                                    )),
                                ),
                        )
                        // Changes' patch and Source's box are parts of their own, as Threads'
                        // pane is, each drawing its keyboard ring on its own edge: Tab onto one
                        // shows that box whole with its ring. Revealed as the file's part, which
                        // holds the title, the mode row and the range hint too and is taller than
                        // the body at 18 pt in a 461 × 490 window, Tab onto the patch showed the
                        // part's top and left the patch's foot, its ring with it, below the body.
                        .child(match self.review.mode {
                            1 => {
                                let element = div().flex_1().min_h_0().min_w_0().flex().flex_col();
                                match &self.review.source {
                                    Some(source) => self.ringed_part(
                                        "source",
                                        element,
                                        &source.read(cx).focus_handle(cx),
                                        cx,
                                    ),
                                    None => self.part("source", element, cx),
                                }
                                .child(self.render_source(cx))
                                .into_any_element()
                            }
                            2 => self.render_threads(Some(&file.filename), window, cx),
                            _ => self
                                .ringed_part("patch", div(), &self.review.line_focus, cx)
                                .flex_1()
                                .min_h_0()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(self.render_patch(window, cx))
                                .into_any_element(),
                        }),
                )
                .when(self.review.mode == 0, |element| {
                    element.child(
                        div()
                            .flex_shrink_0()
                            .pt(appearance::ui_size((PAGER_ROW - HINT_LINE) / 2.))
                            .child(
                                super::phrases("github-range-hint", range_hint())
                                    .ml(super::text_inset())
                                    .line_height(appearance::ui_size(HINT_LINE))
                                    .text_color(rgb(p.muted)),
                            ),
                    )
                });
            div()
                .debug_selector(|| "github-file-column".into())
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(content)
                .into_any_element()
        } else {
            div().debug_selector(||"github-file-column".into()).flex_1().min_w_0().p_5().pl(super::text_inset()).flex().flex_col().gap_2().child(label("github-choose-file","Choose a file to begin reviewing").debug_selector(||"github-choose-file".into()).font_weight(FontWeight::SEMIBOLD)).child(label("github-file-help","Changes keep their captured PR identity. Inline drafts remain local as you move between files.").text_color(rgb(p.muted))).into_any_element()
        };
        let measured = self.review.files_fit.clone();
        let (top_line, pinned, laid) = (
            self.review.top_line.clone(),
            self.review.top_line_pinned.get(),
            fit.top_line,
        );
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .debug_selector(|| "github-files".into())
                    .flex()
                    .when(fit.stacked, |columns| columns.flex_col().gap_2())
                    .when(!fit.stacked, |columns| columns.gap_3())
                    .min_w_0()
                    // How much narrower than the dialog the columns are, from which each frame
                    // takes their width as it starts, and the top line standing above the body,
                    // prepainted before it: drawn taller or shorter than the height this layout
                    // took from the body, the next frame lays Files out from it
                    // ([`Panel::files_fit`]). This runs with a file selected or not, so neither
                    // state lays Files out from a line the other measured.
                    .on_children_prepainted(move |children, window, cx| {
                        let drawn = if pinned { top_line.get() } else { Pixels::ZERO };
                        let (Some(left), Some(right)) = (
                            children
                                .iter()
                                .map(|child| child.left())
                                .reduce(Pixels::min),
                            children
                                .iter()
                                .map(|child| child.right())
                                .reduce(Pixels::max),
                        ) else {
                            return;
                        };
                        // Less the dialog's side padding, which follows the text size
                        // (`crate::dialog_ghost_room`), so the chrome measured at one size
                        // holds at the next.
                        let chrome = super::panel_width(window)
                            - (right - left)
                            - gpui_kit::component::dialog::dialog_side_padding(window, cx) * 2.;
                        FilesFit::update(
                            &measured,
                            |fit| fit.chrome = Some(chrome),
                            drawn != laid,
                            window,
                            cx,
                        );
                    })
                    .child(file_list)
                    .child(content),
            )
            // Below both columns, at the panel's text edge, where Previous starts its label:
            // across the panel's width it keeps one line, and in a narrow panel it breaks
            // between its phrases. In the file list's column it took two lines, one phrase to
            // each, without its "·".
            .child(
                super::phrases(
                    "github-file-bound",
                    "100 files per page · GitHub limit 3,000",
                )
                .ml(super::text_inset())
                .text_color(rgb(p.muted)),
            )
            .child(self.render_comment_composer(cx))
            .into_any_element()
    }
    /// The exact patch, filling its box ([`Panel::files_fit`]) in whole lines: what a line does
    /// not fill, less than a line, lies below the text, in the editor's own background.
    fn render_source(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(source) = &self.review.source else {
            return label("github-no-source","No text patch was supplied. Open the captured local comparison to inspect available source or binary content.").ml(super::text_inset()).into_any_element();
        };
        let p = palette(cx);
        // The editor draws no edge of its own, so the box draws the keyboard ring on its edge,
        // over the text, as Changes' does; Tab onto Source showed no ring at all.
        let ringed = self.ringed(&source.read(cx).focus_handle(cx), cx);
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
            .relative()
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
                    // overruns this by. Each row this clip shows whole is a line of the body,
                    // and the clip tells the body its bounds ([`clipped`]), so the part of a
                    // row under its foot is none where the clip runs past the body's edge,
                    // which was taken for the clip there and let that part count.
                    clipped(
                        div()
                            .debug_selector(|| "github-source-text".into())
                            .relative()
                            .size_full()
                            .overflow_hidden()
                            .child(editor_rows(source))
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .right_0()
                                    .bottom(-EDITOR_PADDING)
                                    .child(content),
                            ),
                    )
                    .size_full(),
                ),
            )
            .when(ringed, |pane| {
                pane.child(
                    div()
                        .absolute()
                        .inset_0()
                        .border_1()
                        .border_color(rgb(p.accent)),
                )
            })
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
                        .ml(super::text_inset())
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
        // Whole rows down to the box's foot ([`Panel::files_fit`]), as Source shows whole
        // lines, below the top inset Source's editor keeps above its text: what a row does not
        // fill lies below them, measured from the box's height in the last layout, which the
        // insets cannot change.
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
        // The keyboard ring lies over the rows' edge, so no row moves.
        let ringed = self.ringed(&self.review.line_focus, cx);
        crate::native_accessibility::clips_children(div().id("github-patch-focus").key_context("GitTurtleGithubPatch").on_action(cx.listener(|this,_:&ReviewUp,window,cx|{this.review_key("up",false,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewDown,window,cx|{this.review_key("down",false,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewHome,window,cx|{this.review_key("home",false,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewEnd,window,cx|{this.review_key("end",false,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewPageUp,window,cx|{this.review_key("pageup",false,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewPageDown,window,cx|{this.review_key("pagedown",false,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewExtendUp,window,cx|{this.review_key("up",true,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewExtendDown,window,cx|{this.review_key("down",true,window,cx);cx.stop_propagation();})).on_action(cx.listener(|this,_:&ReviewAccept,window,cx|{this.review_key("enter",false,window,cx);cx.stop_propagation();})).debug_selector(||"github-patch".into()).flex_1().min_h_0().min_w_0().border_1().border_color(rgb(p.border)).rounded(px(6.)).overflow_hidden()
            // A context row's surface, so the strip below the rows reads as part of the patch.
            .bg(rgb(p.canvas))
            .relative().track_focus(&self.review.line_focus).role(Role::ListBox).aria_label("Captured pull request patch lines").aria_description("Up and Down move through lines. Shift extends a same-side range. Return starts an inline comment. Source offers exact text selection and Find.")

            .child(div().size_full().on_children_prepainted(move |children, window, cx| {
                // The inset follows the height inside the border, never the other way round,
                // so this settles in the next frame.
                if let Some(inner) = children.first()
                    && measured.get() != inner.size.height
                {
                    measured.set(inner.size.height);
                    window.defer(cx, |window, _| window.refresh());
                }
            }).child(div().size_full().pt(EDITOR_PADDING).pb(slack).child(div().debug_selector(||"github-patch-rows".into()).size_full().overflow_hidden().child(clipped(uniform_list("github-patch-rows",file.rows.len(),cx.processor(move|this,range:std::ops::Range<usize>,_,cx|range.map(|index|{let file=&this.review.files[this.review.selected_file.expect("visible file")];let row=&file.rows[index];let selected=this.review.anchor.zip(this.review.selected_row).is_some_and(|(a,b)|(a.min(b)..=a.max(b)).contains(&index));let p=palette(cx);let background=if selected{p.selected}else{match row.kind{b'+'=>p.added_background,b'-'=>p.removed_background,b'@'=>p.subtle,_=>p.canvas}};let row_label=format!("Before {} · After {} · {}",row.old.map(|n|n.to_string()).unwrap_or_default(),row.new.map(|n|n.to_string()).unwrap_or_default(),row.text);
                // The selection's edge overlays the row, so its text keeps Source's place.
                // Each row is a line of the body where the list shows it ([`clipped`]).
                patch_row(index,row,column,row_height,cx).child(one_line()).bg(rgb(background)).when(selected,|row|row.child(div().absolute().left_0().top_0().bottom_0().w(px(2.)).bg(rgb(p.accent)))).role(Role::ListBoxOption).aria_label(row_label).aria_selected(selected).cursor_pointer()
                    .on_click(cx.listener(move|this,event:&ClickEvent,window,cx|this.select_review_line(index,event.modifiers().shift,window,cx))).into_any_element()
            }).collect::<Vec<_>>())).size_full().track_scroll(&self.review.line_scroll)).size_full())))).when(ringed, |pane| pane.child(div().absolute().inset_0().border_1().border_color(rgb(p.accent))))).into_any_element()
    }
    pub(super) fn render_collected(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        div().flex().flex_col().gap_2()
            .child(label("github-review-heading",format!("{} in this review",crate::counted(self.review.comments.len(),"inline comment","inline comments"))).ml(super::text_inset()).font_weight(FontWeight::SEMIBOLD))
            .when(self.review.comments.is_empty(),|element|element.child(label("github-review-empty","Select changed lines in Files to add inline feedback, or send a review summary below.").ml(super::text_inset()).text_color(rgb(p.muted))))
            .child(clipped(crate::native_accessibility::clips_children(div().id("github-collected-comments").debug_selector(||"github-collected-comments".into()).flex_shrink_0().max_h(px(230.)).overflow_y_scroll().flex().flex_col().gap_2().children(self.review.comments.iter().enumerate().map(|(index,comment)|div().flex_shrink_0().p_3().border_1().border_color(rgb(p.border)).rounded(px(6.)).flex().flex_col().gap_2()
                .child(label(("github-draft-position",index),model::position_label(comment)).font_weight(FontWeight::SEMIBOLD))
                .child(label(("github-draft-body",index),comment.body.clone()))
                .child(div().flex().gap_2().child(self.button(("github-edit-inline",index),"Edit","",false, cx).disabled(self.pending||self.review.composing.is_some()).on_click(cx.listener(move|this,_,window,cx|this.edit_comment(index,window,cx))))
                    .child(self.button(("github-remove-inline",index),"Remove","",false, cx).disabled(self.pending).on_click(cx.listener(move|this,_,window,cx|{if index<this.review.comments.len(){this.review.comments.remove(index);this.confirm=None;this.save_draft(window,cx);this.focus_visible_section(window,cx);}})))))))))
            .child(self.render_comment_composer(cx)).into_any_element()
    }
}
/// What Files measured in the last layout to fit the panel's body ([`Panel::files_fit`]): how
/// much narrower than the dialog the width its columns share is, the dialog's sides, the body's
/// ring room and its scroll cue's gutter, which no window or text size changes, and the height
/// of the file's title and mode row above the file's box. The top line standing above the body
/// is no measure of Files': the line measures itself in every frame it stands there, whatever
/// the section and whether a file is selected ([`ReviewState::top_line`]). Recorded here only
/// while a file was selected, Files with none laid itself out from a line an earlier selection
/// measured, or from none: at 13 pt in a 461 × 490 window with a reply open and a notice above
/// the body, its list took more than half the body, and a click on a file moved the list's
/// foot half the line a frame later.
#[derive(Clone, Copy, Default, PartialEq)]
pub(super) struct FilesFit {
    /// `None` until Files has been laid out.
    chrome: Option<Pixels>,
    above: Pixels,
}
#[cfg(test)]
thread_local! {
    /// How many times Files' measures changed and drew the window again ([`FilesFit::update`]),
    /// for a test to count the frames a resize takes to settle.
    pub(super) static FILES_FIT_REDRAWS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}
impl FilesFit {
    /// Applies `change` to the measures in `fit`, drawing the window again where they change, or
    /// where the top line drew another height than the layout took from the body (`moved`), so
    /// the next frame lays Files out from them. They settle in that frame: the dialog's width
    /// and the body's height come from the window as each frame starts, the chrome beside the
    /// columns never changes once measured, and the height above the box follows from the
    /// width the frame lays the columns out at, which nothing measured here moves.
    fn update(
        fit: &std::rc::Rc<std::cell::Cell<FilesFit>>,
        change: impl FnOnce(&mut FilesFit),
        moved: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        let mut next = fit.get();
        change(&mut next);
        if next != fit.get() || moved {
            fit.set(next);
            #[cfg(test)]
            FILES_FIT_REDRAWS.with(|redraws| redraws.set(redraws.get() + 1));
            window.defer(cx, |window, _| window.refresh());
        }
    }
}
/// How Files lays its columns out in the body ([`Panel::files_fit`]): stacked or side by side,
/// the changed-file list's height and the height of the file's box, its title, mode row and
/// patch or Source, and the height the top line standing above the body took from it.
pub(super) struct FilesLayout {
    stacked: bool,
    list: Pixels,
    content: Pixels,
    top_line: Pixels,
}
impl Panel {
    /// How Files lays its columns out in the body, from the window and what the last frame
    /// measured ([`FilesFit`]), with `file` selected or not. Side by side where the file's
    /// column beside the 210 px changed-file list (scaled with the text) holds its mode row,
    /// Changes, Source, Threads and Comment on selection, on one line, measured from the
    /// interface font at the interface size; otherwise stacked, the list above the file, where
    /// the mode row wraps inside the body's width. Stacked, the list takes at most half the
    /// body's height, scrolling inside it, and the file's box the rest of the body below the
    /// list's pager, so with the list's top at the body's top the two fill the body inside the
    /// ring's room; side by side, both boxes take the body's height less the pager's row and
    /// the ring's room, at most 324 px. Either way the patch or Source keeps at least five
    /// lines. With the file's column always beside the list and both boxes a fixed 324 px, the
    /// mode row ran past the body's right edge with its rings (at 13 pt in a 461 × 490 window
    /// the column was 153 px wide, and the rings reached 26 px past the edge), the list was
    /// taller than the body, its ring's top cut, and at 18 pt the patch and Source had no
    /// height at all.
    pub(super) fn files_fit(&self, file: bool, window: &Window, cx: &App) -> FilesLayout {
        let measured = self.review.files_fit.get();
        let room = appearance::button_ring_room(cx);
        let rem = window.rem_size();
        // The body takes the room the dialog leaves it, less the top line standing above it while
        // a conversation is open, which Files fills, on whole pixels. Sized from the dialog's
        // room alone, at 13 pt in a 461 × 490 window with a reply open and a notice above the
        // body, the stacked list took 146.5 px of a 266.5 px body. The line's height is the one
        // the last frame drew, which Files checks against this frame's as it prepaints.
        let top_line = if self.review.top_line_pinned.get() {
            self.review.top_line.get()
        } else {
            Pixels::ZERO
        };
        let body = window.pixel_snap(super::body_max_height(window) - top_line);
        let width = match measured.chrome {
            Some(chrome) => {
                super::panel_width(window)
                    - gpui_kit::component::dialog::dialog_side_padding(window, cx) * 2.
                    - chrome
            }
            None => {
                self.review.body_scroll.bounds().size.width
                    - room * 2.
                    - crate::tags::ScrollCue::gutter()
            }
        };
        let beside = appearance::ui_size(210.).max(px(150.)) + rem * 0.75;
        let stacked = width < beside + mode_row_width(self.review.composing.is_some(), window);
        let inner = body - room * 2.;
        let pager = appearance::ui_size(PAGER_ROW);
        let least = measured.above
            + if file {
                self.five_lines(window, cx)
            } else {
                px(0.)
            };
        if stacked {
            let list = FILE_LIST_HEIGHT.min(body / 2.);
            let rest = inner - list - pager - rem;
            FilesLayout {
                stacked,
                list,
                content: rest.max(least),
                top_line,
            }
        } else {
            let list = FILE_LIST_HEIGHT.min(inner - pager - rem * 0.5);
            FilesLayout {
                stacked,
                list,
                content: list.max(least),
                top_line,
            }
        }
    }
    /// The height of the selected file's box in Changes or Source that shows five whole lines
    /// of its patch inside its border, below its top inset and Source's Find bar, with a pixel
    /// to spare for layout's rounding; none in Threads, whose box takes its content's height.
    fn five_lines(&self, window: &Window, cx: &App) -> Pixels {
        let edges = px(2.) + px(1.);
        match self.review.mode {
            0 => patch_row_height(window.scale_factor()) * 5. + EDITOR_PADDING + edges,
            1 => match &self.review.source {
                Some(source) => {
                    let line = source
                        .read(cx)
                        .line_height()
                        .unwrap_or(appearance::code_text() * EDITOR_LINE_HEIGHT);
                    line * 5. + EDITOR_PADDING + editor_find::panel_height(source, cx) + edges
                }
                None => px(0.),
            },
            _ => px(0.),
        }
    }
}
/// The width the file's mode row takes on one line: Changes, Source and Threads, then Comment
/// on selection, or Continue comment while a comment is open, with its icon, each as the
/// shared Button lays it out ([`crate::labelled_button_width`]), and the row's gaps between
/// its five children, the spacer before the last among them. Measured again only when the font,
/// the sizes, the display's scale or the last Button's label change: the key they are read by
/// is read once a render (a view's render builds the font's default features at each read, an
/// allocation), where each of the four labels read it. A key without the label kept the other
/// label's layout (`github_view::tests::files_mode_row_follows_its_last_label`), and one
/// without the scale stood the file beside the list with its row wrapped where another scale's
/// device pixels widen it (`github_view::tests::files_mode_row_follows_the_display_scale`).
fn mode_row_width(composing: bool, window: &Window) -> Pixels {
    /// The font and sizes, the display's scale and whether a comment is open.
    type RowKey = (crate::views::LineKey, f32, bool);
    thread_local! {
        static ROW: std::cell::RefCell<Option<(RowKey, Pixels)>> =
            const { std::cell::RefCell::new(None) };
        static CHANGES: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
        static SOURCE: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
        static THREADS: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
        static COMMENT: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    }
    let key = (
        crate::views::LineKey::current(window),
        window.scale_factor(),
        composing,
    );
    if let Some(width) = ROW.with(|row| match &*row.borrow() {
        Some((measured, width)) if *measured == key => Some(*width),
        _ => None,
    }) {
        return width;
    }
    let plain = |label: &str, cache| {
        crate::placed_button_width(crate::label_text_width(label, cache, window), false, window)
    };
    let comment = if composing {
        "Continue comment"
    } else {
        "Comment on selection"
    };
    let width = plain("Changes", &CHANGES)
        + plain("Source", &SOURCE)
        + plain("Threads", &THREADS)
        + crate::labelled_button_width(comment, &COMMENT, window)
        + window.rem_size() * 0.25 * 4.;
    ROW.with(|row| *row.borrow_mut() = Some((key, width)));
    width
}
fn patch_line(index: usize, cx: &App) -> Stateful<Div> {
    div()
        .id(("github-patch-line", index))
        .text_size(appearance::code_text())
        .code_font(cx)
}

/// Changes' range hint below its box. Where it wraps it breaks between its phrases
/// ([`super::phrases`]): at 18 pt in a 1,480 px window it broke inside the last one, leaving
/// "comments" alone on a line.
fn range_hint() -> String {
    format!(
        "Click a changed line · {} or {} / {} selects a same-side range · {} comments",
        shortcuts::SHIFT_CLICK,
        shortcuts::key_label("shift-up"),
        shortcuts::key_label("shift-down"),
        shortcuts::ENTER
    )
}

/// The tallest the changed-file list and the file's box beside it grow ([`Panel::files_fit`]).
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
/// A Changes row's height: one of Source's lines, the kit editor's [`EDITOR_LINE_HEIGHT`]
/// times the code text, so both modes draw one row height (Changes' rows were 20 px at 1.65
/// beside Source's 18 px lines), on whole device pixels at `scale`. Layout rounds an authored
/// height to device pixels, so a row of 19.8 px was drawn 20 px tall at 2x: counted at
/// 19.8 px, the box's rows ran 2.6 px past its clip and cut the last one. Snapped here, the
/// height counted is the height drawn.
fn patch_row_height(scale: f32) -> Pixels {
    px((f32::from(appearance::code_text()) * EDITOR_LINE_HEIGHT * scale).round() / scale)
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

    /// [`clear_of_lines`] with a pass over every line for each place it asks about, as it
    /// stood, the reference [`Insides`] must agree with; it chooses among the places as
    /// [`clear_of_lines`] does ([`stop`]).
    fn clear_of_lines_by_pass(
        viewport: Bounds<Pixels>,
        edge: Pixels,
        (top, bottom): (Pixels, Pixels),
        lines: &[(Pixels, Pixels)],
        starts: &[Pixels],
        (first, last): (Pixels, Pixels),
    ) -> Pixels {
        let height = viewport.size.height;
        let slack = px(0.5);
        let within = px(0.01);
        let (top, bottom) = if bottom - top <= height {
            (top, bottom)
        } else if (edge - top).abs() <= slack {
            (top, top + height / 2.)
        } else {
            (edge + height / 2., edge + height)
        };
        let cuts = |at: Pixels| {
            lines.iter().any(|(line_top, line_bottom)| {
                at > *line_top + within && at < *line_bottom - within
            })
        };
        let lands = |at: Pixels| {
            at >= first - slack
                && at <= last + slack
                && at <= top + within
                && at + height >= bottom - within
                && !cuts(at)
                && !cuts(at + height)
        };
        if lands(edge) || edge <= first + slack {
            return edge;
        }
        stop(viewport, edge, lines, starts, (first, last), lands).unwrap_or(edge)
    }

    /// [`clear_of_lines`], which asks [`Insides`] whether an edge cuts a line, stops where a
    /// pass over every line stopped, so its stops are the same, on lines that overlap, touch,
    /// lie inside a frame, run past the view and stand apart, with parts shorter and taller
    /// than the view, scrolling either way, near the content's ends and with no line at all.
    #[test]
    fn insides_stop_where_a_pass_over_every_line_stopped() {
        // A fixed linear congruential sequence, so every run checks the same cases.
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = |below: u32| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((state >> 33) as u32) % below
        };
        let mut moved = 0;
        for case in 0..4000 {
            let height = px(60. + next(240) as f32);
            let from = px(next(800) as f32 / 2.);
            let viewport = Bounds::new(point(px(0.), from), size(px(300.), height));
            let mut lines = Vec::new();
            let mut at = px(next(40) as f32);
            for _ in 0..next(40) {
                let tall = px(10. + next(60) as f32 / 2.);
                // Apart, touching or overlapping the line before, now and then within it.
                let gap = px(next(30) as f32 / 2.) - px(5.);
                at += gap.max(px(-tall.as_f32() + 0.5));
                lines.push((at, at + tall));
                at += tall;
            }
            // Now and then a frame around some of them, as a one-line control stands beside
            // a text that wraps, so a line can lie wholly inside another that starts before it.
            for _ in 0..next(3) {
                if lines.is_empty() {
                    break;
                }
                let first = next(lines.len() as u32) as usize;
                let last = first + next((lines.len() - first) as u32) as usize;
                let (top, bottom) = (lines[first].0, lines[last].1);
                lines.push((
                    top - px(next(8) as f32 / 2.),
                    bottom + px(next(8) as f32 / 2.),
                ));
            }
            let starts: Vec<Pixels> = (0..next(12)).map(|_| px(next(1600) as f32 / 2.)).collect();
            let top = px(next(1600) as f32 / 2.);
            let part = (top, top + px(5. + next(400) as f32 / 2.));
            let first = px(next(60) as f32) - px(30.);
            let range = (first, first + px(next(1400) as f32 / 2.));
            let edge = px(next(1600) as f32 / 2.);
            let fast = clear_of_lines(viewport, edge, part, &lines, &starts, range);
            let pass = clear_of_lines_by_pass(viewport, edge, part, &lines, &starts, range);
            assert_eq!(
                fast, pass,
                "case {case}: {viewport:?}, edge {edge:?}, part {part:?}, lines {lines:?}, \
                 starts {starts:?}, range {range:?}"
            );
            moved += usize::from(fast != edge);
        }
        // Hundreds of cases move the edge (930 of them), so the stops, not only the
        // landing, are compared.
        assert!(moved > 500, "{moved} of 4000 cases move the edge");
    }
    /// A reveal scrolling down from a view at 100..300 lands its top edge at 150, inside the
    /// line at 140..160: it stops the least less that shows the line whole where the part it
    /// shows stays whole, otherwise goes on until the line is out of view, and, where the
    /// content ends before that, leaves the part whole where the reveal put it. A line its
    /// bottom edge cuts is shown whole or left out of view the same way.
    #[test]
    fn a_reveal_stops_clear_of_the_lines_at_its_edges() {
        let viewport = Bounds::new(point(px(0.), px(100.)), size(px(300.), px(200.)));
        let top_line = [(px(140.), px(160.))];
        let range = (px(0.), px(1000.));
        let part = |bottom: f32| (px(300.), px(bottom));
        assert_eq!(
            clear_of_lines(
                viewport,
                px(150.),
                part(340.),
                &top_line,
                &[px(140.)],
                range
            ),
            px(140.),
            "a little less shows the line whole, the part still whole"
        );
        assert_eq!(
            clear_of_lines(
                viewport,
                px(150.),
                part(345.),
                &top_line,
                &[px(140.), px(160.)],
                range
            ),
            px(160.),
            "where less would cut the part, a little more takes the line out of view"
        );
        assert_eq!(
            clear_of_lines(
                viewport,
                px(150.),
                part(345.),
                &top_line,
                &[px(140.), px(160.)],
                (px(0.), px(150.))
            ),
            px(150.),
            "where the content ends, the part whole wins"
        );
        let bottom_line = [(px(340.), px(360.))];
        assert_eq!(
            clear_of_lines(
                viewport,
                px(150.),
                part(335.),
                &bottom_line,
                &[px(140.)],
                range
            ),
            px(140.),
            "a little less takes the line the bottom edge cut out of view"
        );
        assert_eq!(
            clear_of_lines(
                viewport,
                px(150.),
                part(345.),
                &bottom_line,
                &[px(140.), px(160.)],
                range
            ),
            px(160.),
            "a little more shows it whole"
        );
        // A part taller than the view, shown from its top at 150 as a pane that draws its own
        // ring is: it keeps the half of the view below its top, so where the content starts
        // just above it, with no block to stop at, the reveal stays where it landed. Where
        // the content goes on above, the view stops a little less, its bottom edge at the
        // line's top, or, where that would cut another line at its top edge, at the nearest
        // place clear of both, the part's top, its leading edge, still in view either way.
        let tall = (px(150.), px(500.));
        assert_eq!(
            clear_of_lines(
                viewport,
                px(150.),
                tall,
                &bottom_line,
                &[],
                (px(145.), px(1000.))
            ),
            px(150.),
            "with nowhere clear to stop, the tall part stays where it landed"
        );
        assert_eq!(
            clear_of_lines(viewport, px(150.), tall, &bottom_line, &[px(120.)], range),
            px(140.),
            "a little less takes the line out of view, the tall part's top still in view"
        );
        let above = [(px(135.), px(145.)), (px(340.), px(360.))];
        assert_eq!(
            clear_of_lines(viewport, px(150.), tall, &above, &[px(120.)], range),
            px(135.),
            "the nearest place clear of both lines, the line above's top, wins over the block \
             start farther up, the tall part's top still in view"
        );
        // Shift+Tab: a reveal scrolling up from a view at 400..600 lands at 190, its bottom
        // edge inside the line at 380..400, and less and more are mirrored: less stops lower.
        let below = Bounds::new(point(px(0.), px(400.)), size(px(300.), px(200.)));
        let line = [(px(380.), px(400.))];
        let starts = [px(170.), px(200.)];
        assert_eq!(
            clear_of_lines(below, px(190.), (px(200.), px(260.)), &line, &starts, range),
            px(200.),
            "scrolling up, a little less shows the line whole"
        );
        assert_eq!(
            clear_of_lines(below, px(190.), (px(190.), px(260.)), &line, &starts, range),
            px(180.),
            "scrolling up, where less would cut the part, a little more takes the line out, \
             the bottom edge at its top"
        );
    }

    /// Where no block start leaves every line whole or out of view, nor the bottom edge at a
    /// line's top or foot, the reveal stops between two lines, its top edge at a line's top or
    /// foot: a view at 100..300 scrolled down to 150, its top edge inside the first of three
    /// lines at 145..175; the bottom edge at the top or foot of the line at 350..370 below them
    /// would leave the top edge inside one of the three. The nearest place clear of every line
    /// wins over a block start farther on (decision, 2026-11-05); otherwise the least less,
    /// the paragraph's first line whole, and where that would cut the part, the least more,
    /// past the line at the bottom edge. As macOS wraps the connection help at 18 pt in a
    /// 461 × 490 window, no block start kept Connect whole with both edges clear, and Tab onto
    /// it left Disconnect cut at the body's foot.
    #[test]
    fn where_no_block_start_clears_the_lines_the_edge_stops_between_two() {
        let viewport = Bounds::new(point(px(0.), px(100.)), size(px(300.), px(200.)));
        let lines = [
            (px(145.), px(155.)),
            (px(155.), px(165.)),
            (px(165.), px(175.)),
            (px(350.), px(370.)),
        ];
        let range = (px(0.), px(1000.));
        let part = |bottom: f32| (px(250.), px(bottom));
        assert_eq!(
            clear_of_lines(
                viewport,
                px(150.),
                part(330.),
                &lines,
                &[px(148.), px(240.)],
                range
            ),
            px(145.),
            "the nearest place clear of every line wins over a block start farther on"
        );
        assert_eq!(
            clear_of_lines(viewport, px(150.), part(330.), &lines, &[px(148.)], range),
            px(145.),
            "with no block start clear, the least less between two lines"
        );
        assert_eq!(
            clear_of_lines(viewport, px(150.), part(346.), &lines, &[px(148.)], range),
            px(175.),
            "where less would cut the part, the least more between two lines"
        );
    }

    /// Where no block start clears every line, the reveal stops where the view's bottom edge
    /// lies at a line's top or foot before it stops between two lines: the connection pane at
    /// 17 pt in a 440 × 490 window, as Linux lays it out, scrolled up by Shift+Tab onto Account
    /// options, its body 271 px tall. The reveal lands at Account options' row, 142, its bottom
    /// edge inside Connect; no block start keeps the control whole with both edges clear, and
    /// no line's top or foot as the top edge, which once kept the landing where it cut Connect
    /// (or, with the help's lines counted 25 px apart for 25.5, stopped at 91.5, the help's
    /// fifth line cut by 2.5 px). The bottom edge at the foot of that line, 365, clears every
    /// line, the top edge between Refresh and New pull request.
    #[test]
    fn where_no_block_start_clears_the_lines_the_bottom_edge_stops_at_one() {
        let viewport = Bounds::new(point(px(38.), px(396.)), size(px(364.), px(271.)));
        let help = (0..6).map(|line| {
            let top = px(237.5) + px(25.5) * line as f32;
            (top, top + px(25.5))
        });
        let lines: Vec<_> = [
            (px(12.5), px(46.5)),
            (px(55.), px(91.5)),
            (px(100.), px(136.5)),
            (px(145.), px(181.5)),
            (px(203.5), px(229.)),
        ]
        .into_iter()
        .chain(help)
        .chain([(px(399.), px(435.5)), (px(444.), px(480.5))])
        .collect();
        let ring = px(3.);
        let starts: Vec<_> = [12.5, 55., 100., 145., 203.5, 237.5, 399., 444.]
            .into_iter()
            .map(|start| px(start) - ring)
            .collect();
        let range = (px(9.5), px(798.5));
        let part = (px(142.), px(184.5));
        assert_eq!(
            clear_of_lines(viewport, px(142.), part, &lines, &starts, range),
            px(94.),
            "the bottom edge at the foot of the help's fifth line"
        );
        // Landed at the content's top, as Tab wrapping to the first part does, the reveal
        // keeps the box's top though its bottom edge cuts a line whose foot, 8.5 px lower,
        // would clear every line with the part's ring still whole: that place cuts the box
        // above the part, as it once cut the offline fixture's banner at 18 pt.
        let top = Bounds::new(point(px(204.), px(600.)), size(px(1072.), px(544.)));
        let banner = (px(133.), px(178.));
        let foot = [(px(649.5), px(676.5))];
        assert_eq!(
            clear_of_lines(top, px(124.), banner, &foot, &[], (px(124.), px(659.))),
            px(124.),
            "the wrap keeps the body's top"
        );
    }

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
                    .child(patch_row(0, &self.0, column, patch_row_height(1.), cx))
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

    /// A text of the panel works its lines out again when the interface's text size changes
    /// though its children keep their sizes ([`text_lines`]): a text in a label's size whose
    /// one child is held at 200 × 60 px draws three lines 19.5 px apart at 13 pt, and two
    /// 27 px apart at 18 pt, the lines a frame working its out afresh reports. Taken while
    /// only the text and its children's sizes held, its lines kept their 13 pt height at
    /// 18 pt, three of them. The test window's scale factor stays at 2, so the interface's
    /// text size, which moves its text scale and the window's rem size, stands for every
    /// scale a text's lines follow from ([`TextLines::scale`]).
    #[gpui::test]
    fn a_text_works_its_lines_out_again_at_another_text_size(cx: &mut TestAppContext) {
        struct Body(Blocks);
        impl Render for Body {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let blocks = self.0.clone();
                let lines = ElementId::from("held-text");
                div()
                    .w(px(300.))
                    .on_children_prepainted(move |_, _, _| blocks.finish())
                    .child(self.0.collect_lines())
                    // A label sets its text's size around the element that reports its lines.
                    .child(
                        div().text_size(appearance::ui_text(12.)).child(
                            div()
                                .on_children_prepainted(move |children, window, cx| {
                                    text_lines(&lines, children, window, cx)
                                })
                                .child(
                                    div()
                                        .debug_selector(|| "held-child".into())
                                        .w(px(200.))
                                        .h(px(60.))
                                        .child("Held"),
                                ),
                        ),
                    )
            }
        }
        cx.update(gpui_kit::init);
        let blocks = Blocks::default();
        let (_, cx) = cx.add_window_view({
            let blocks = blocks.clone();
            move |_, _| Body(blocks)
        });
        let draw = |interface: u8, cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|window, cx| {
                appearance::apply_text_sizes(interface, 12, window, cx);
                window.draw(cx).clear(cx);
                blocks.lines(cx)
            })
        };
        let child = |cx: &mut gpui_kit::VisualTestContext| {
            cx.debug_bounds("held-child")
                .expect("the text's child")
                .size
        };
        let small = draw(13, cx);
        let held = child(cx);
        let (_, measured) = blocks.text_counts();
        let large = draw(18, cx);
        assert_eq!(child(cx), held, "the child keeps its size at 18 pt");
        assert_eq!(
            blocks.text_counts().1,
            measured + 1,
            "the text works its lines out again at 18 pt"
        );
        blocks.forget_texts();
        assert_eq!(
            draw(18, cx),
            large,
            "the lines it reports at 18 pt are those it works out afresh"
        );
        assert_eq!(
            (small.len(), large.len()),
            (3, 2),
            "three lines at 13 pt, {small:?}, and two at 18 pt, {large:?}"
        );
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
        crate::views::tests::fixture_identity(fixture.path());
        let repo = GitRepository::open(fixture.path()).unwrap();
        let panel_repo = repo.clone();
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let output = captured.clone();
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
        // With the content running past it, the body takes the room the dialog's height cap
        // leaves it in this window: the dialog is as tall as its cap, neither shorter nor
        // taller. Its height rather than its foot, since the dialog may still be sliding in.
        let scroll = cx.read(|cx| panel.read(cx).review.body_scroll.clone());
        let viewport = scroll.bounds();
        assert!(
            scroll.max_offset().y > px(0.),
            "the content runs past the body"
        );
        let (dialog, _) = super::super::tests::dialog_frame(cx);
        let cap = cx.update(|window, _| appearance::dialog_max_height(window));
        assert!(
            (dialog.size.height - cap).abs() <= px(1.),
            "the dialog is {:?} tall, not its cap's {cap:?}",
            dialog.size.height
        );
        assert!(
            composer.size.height > px(100.) && composer.size.width > px(300.),
            "composer must retain useful native geometry"
        );
        // Its border and side padding together are the panel's text inset, as the
        // Conversations pane's are ([`super::bordered_pane`]), so its text starts there.
        let position = cx
            .debug_bounds("github-composer-position")
            .expect("the composer's position line");
        assert!(
            (position.left() - composer.left() - super::text_inset()).abs() <= px(0.25),
            "the inline composer's text {position:?} starts the panel's text inset inside \
             its frame {composer:?}"
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
        crate::views::tests::fixture_identity(fixture.path());
        let repo = GitRepository::open(fixture.path()).unwrap();
        let panel_repo = repo.clone();
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let output = captured.clone();
        crate::views::tests::init_app(cx);
        cx.update(|cx| {
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
        crate::views::tests::fixture_identity(fixture.path());
        let repo = GitRepository::open(fixture.path()).unwrap();
        let panel_repo = repo.clone();
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let output = captured.clone();
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
        crate::views::tests::fixture_identity(fixture.path());
        let repo = GitRepository::open(fixture.path()).unwrap();
        let panel_repo = repo.clone();
        let observed: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let output = observed.clone();
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
        settle_open(&app, cx);
        let canvas = midnight_canvas(cx);
        // What Source lays its lines out from: the height its box measured in the last frame
        // and the editor's line height.
        let measured = |cx: &App| {
            let review = &panel.read(cx).review;
            let line = review
                .source
                .as_ref()
                .and_then(|editor| editor.read(cx).line_height());
            (review.source_height.get(), line)
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
                    // Undecorated, the frames run nothing the file's preparation needs.
                    if decorated {
                        cx.run_until_parked();
                    }
                    draw_settled(cx, measured);
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
    /// After the panel opens: its read of local settings and drafts, on the operations thread
    /// `run_until_parked` cannot wait for, has landed, so the panel's content is final.
    fn settle_open(app: &Entity<GitTurtle>, cx: &mut gpui_kit::VisualTestContext) {
        app.read_with(cx, |app, _| app.operations.drain());
        cx.run_until_parked();
    }

    /// Draws until a frame leaves `measured` as it found it. A review box lays its rows or
    /// lines out from what its content measured in the last frame, and a frame whose layout
    /// differs corrects the measure and draws again; the frame that corrects nothing showed
    /// the box laid out from its own height. The dialog opens at rest (`review_panel`), so
    /// nothing else moves the box between frames.
    fn draw_settled<T: PartialEq + std::fmt::Debug>(
        cx: &mut gpui_kit::VisualTestContext,
        measured: impl Fn(&App) -> T,
    ) {
        let mut before = cx.read(&measured);
        for _ in 0..8 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let after = cx.read(&measured);
            if after == before {
                return;
            }
            before = after;
        }
        panic!("the review box is still measuring {before:?}");
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

    /// What the last frame painted in `frame`, a box that clips its content: the fills of the
    /// quads at its bounds, its own, and the lowest bottom, as clipped, of every other quad it
    /// clips. A quad the box does not clip is not the box's, though it may lie inside it:
    /// History's empty state draws its Open Changes button beneath the dialog, inside the
    /// Changes and Source boxes at 13 pt once the dialog is at rest, its bottom 13.5 px below
    /// Changes' last whole row and 1.5 px below Source's last whole line. Counted, it failed
    /// both tests whenever the dialog's entrance had nearly finished before they read the
    /// frame, which a loaded machine made likely; while the dialog was still sliding in, the
    /// boxes lay above the button.
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
                let clip = logical(quad.content_mask.bounds);
                let shown = drawn.intersect(&clip);
                if shown.size.height > px(0.)
                    && shown.size.width > px(0.)
                    && clip.left() >= frame.left() - px(0.5)
                    && clip.right() <= frame.right() + px(0.5)
                    && clip.top() >= frame.top() - px(0.5)
                    && clip.bottom() <= frame.bottom() + px(0.5)
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
        settle_open(&app, cx);
        let canvas = midnight_canvas(cx);
        // What Changes counts its rows over: the height its box measured in the last frame.
        let measured = |cx: &App| panel.read(cx).review.patch_height.get();
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
                draw_settled(cx, measured);
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
                // The rows as drawn, each on whole device pixels (18 px at 12 pt): when rows
                // were 19.8 px, drawn 20 px tall at 2x, counting them at 19.8 px cut the 13th
                // row of the first file 2.6 px short.
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

    /// In a 1480 × 980 window, Changes' range hint takes one line at 13 pt and wraps at
    /// 18 pt, breaking only between its phrases. It broke inside one: here inside the second,
    /// natively inside the last, leaving "comments" alone on a line.
    #[gpui::test]
    async fn the_range_hint_wraps_between_its_phrases(cx: &mut TestAppContext) {
        use gpui_kit::Styled as _;
        let (_fixture, app, panel, cx) =
            review_panel_styled(cx, |root| root.text_bg(crate::width_walk::text_mark()));
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
        settle_open(&app, cx);
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.select_review_file(0, window, cx);
                panel.review_source(0, window, cx);
            })
        });
        cx.run_until_parked();
        let measured = |cx: &App| panel.read(cx).review.patch_height.get();
        for (interface, wraps) in [(13, false), (18, true)] {
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            draw_settled(cx, measured);
            let at = format!("{interface} pt");
            // The panel's body scrolls the whole hint into view; at 18 pt its last line lies
            // below the body's fold, above the dialog's footer.
            let (hint, body) = (
                cx.debug_bounds("github-range-hint").expect("range hint"),
                cx.debug_bounds("github-panel").expect("panel's body"),
            );
            // Inside the room the body keeps for focus rings.
            let below = hint.bottom() - (body.bottom() - px(4.));
            if below > px(0.) {
                cx.read(|cx| {
                    let scroll = &panel.read(cx).review.body_scroll;
                    let offset = scroll.offset();
                    scroll.set_offset(point(offset.x, offset.y - below));
                });
                draw_settled(cx, measured);
            }
            let lines = super::super::tests::assert_phrases_whole(
                cx,
                "github-range-hint",
                &range_hint(),
                appearance::ui_text(12.),
                &at,
            );
            assert_eq!(lines > 1, wraps, "{at}: the hint draws {lines} lines");
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
    /// character; and each Changes row is as tall as one of Source's lines, where it was 20 px
    /// beside Source's 18 px at 12 pt. Changes started flush with the box, its numbers left-aligned in columns that
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
                let (input, drawn, line) = cx.read(|cx| {
                    let editor = editor.read(cx);
                    (
                        editor.input_bounds(),
                        editor.range_to_bounds(&(0..0)).expect("laid out"),
                        editor.line_height().expect("laid out"),
                    )
                });
                // One row height: a Changes row is one of Source's lines, within the device
                // pixel layout puts a row's edge on.
                assert!(
                    (first.size.height - line).abs() < px(0.51),
                    "{at}: a Changes row {first:?} beside Source's {line:?} line"
                );
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
    /// the panel's local read settled. Dialogs open at rest, as with reduced motion: the
    /// kit's entrance slides a dialog down over 250 ms of real time, so where a frame placed
    /// the panel depended on how long the machine took to draw it.
    fn review_panel(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        Entity<Panel>,
        &mut gpui_kit::VisualTestContext,
    ) {
        review_panel_styled(cx, |root| root)
    }

    /// [`review_panel`] with the window's root styled by `style`.
    fn review_panel_styled(
        cx: &mut TestAppContext,
        style: impl FnOnce(Root) -> Root + 'static,
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
        crate::views::tests::fixture_identity(fixture.path());
        let repo = GitRepository::open(fixture.path()).unwrap();
        let panel_repo = repo.clone();
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let output = captured.clone();
        crate::views::tests::init_app(cx);
        cx.update(|cx| {
            cx.set_reduce_motion(true);
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
            style(Root::new(app, window, cx))
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

    /// The focus sweep (`focus_ring::sweep`) over the panel on the offline
    /// fixture, at the default and the largest interface size: its pull
    /// request list, then a pull request's Overview with the first page of
    /// its conversations, as the native review loads it, its Files with the
    /// first file's patch open, and its Review. Tab shows every stop whole:
    /// the conversations pane fits inside the body, and the conversation list
    /// scrolls to the actions Tab moves onto. Review's first stop once drew
    /// no ring at 18 pt, with a frame drawn before the walk too: a long draft
    /// status pushed Review submission… past the panel's right edge, so the
    /// product, not the walk, was at fault
    /// (`github_view::tests::the_draft_status_gives_way_to_review_submission`).
    #[gpui::test]
    async fn the_panel_draws_a_whole_ring_at_every_stop(cx: &mut TestAppContext) {
        use crate::focus_ring::sweep;
        let (_fixture, app, cx) = crate::views::tests::history_window(cx).await;
        sweep::prepare(&app, cx);
        let repo = cx.read(|cx| app.read(cx).repository.clone().unwrap());
        let panel = cx.update(|window, cx| {
            cx.new(|cx| Panel::new(app.downgrade(), repo, "main".into(), window, cx))
        });
        app.read_with(cx, |app, _| app.operations.submit(|| Ok(())))
            .await
            .unwrap()
            .unwrap();
        cx.executor().run_until_parked();
        let pull = model::fixture_pull(false);
        let captured = pull.capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let files = Client(UiTransport::Fixture { moved: false })
            .files(&captured, 1, &OperationControl::default())
            .unwrap()
            .items;
        let threads = Client(UiTransport::Fixture { moved: false })
            .review_threads(&captured, None, &OperationControl::default())
            .unwrap();
        assert!(
            !threads.items.is_empty(),
            "the fixture's first page of conversations"
        );
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
        let mut failures = Vec::new();
        for interface in sweep::SIZES {
            sweep::interface_size(cx, interface);
            // Each change of view moves focus as the panel's own transitions
            // do, so no walk starts on a pane the panel no longer draws.
            cx.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.pull = None;
                    panel.review.show_list = true;
                    panel.focus_visible_section(window, cx);
                    cx.notify();
                })
            });
            cx.executor().run_until_parked();
            sweep::walk(
                &app,
                cx,
                "the GitHub panel's list",
                interface,
                &mut failures,
            );
            for (section, name) in [
                (Section::Overview, "the GitHub panel's Overview"),
                (Section::Files, "the GitHub panel's Files"),
                (Section::Review, "the GitHub panel's Review"),
            ] {
                cx.update(|window, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.pull = Some(pull.clone());
                        panel.review.files = files.clone();
                        panel.review.section = section;
                        panel.review.show_list = false;
                        if section == Section::Files {
                            panel.select_review_file(0, window, cx);
                        }
                        panel.focus_visible_section(window, cx);
                        cx.notify();
                    })
                });
                cx.executor().run_until_parked();
                sweep::walk(&app, cx, name, interface, &mut failures);
            }
        }
        sweep::assert_whole(failures);
    }

    /// Changes' range hint, laid out in a box swept from 461 px down to its widest run at 13
    /// and 18 pt, breaks only between whole words: no line starts with a phrase's "·" and no
    /// key name breaks across two lines, its lines read as its text does inside the box, and
    /// its accessible text is the whole hint. Where only a phrase's trailing space ran past the
    /// line, the "·" before it wrapped onto a line of its own, and where the second phrase
    /// wrapped, GPUI broke "Shift+Down" before its "+" (review of FU30-D2).
    #[gpui::test]
    fn the_range_hint_keeps_its_key_names_and_separators_whole(cx: &mut TestAppContext) {
        let keys = [
            shortcuts::SHIFT_CLICK.to_owned(),
            shortcuts::key_label("shift-up"),
            shortcuts::key_label("shift-down"),
            shortcuts::ENTER.to_owned(),
        ];
        let runs: Vec<SharedString> = crate::phrase_runs(&range_hint())
            .into_iter()
            .flat_map(|phrase| phrase.runs)
            .collect();
        crate::phrase_tests::sweep(
            cx,
            ("github-range-hint", "github-range-hint"),
            &range_hint(),
            &keys,
            move |window| crate::phrase_tests::widest(&runs, appearance::ui_text(12.), window),
            || super::super::phrases("github-range-hint", range_hint()).into_any_element(),
        );
    }
}
