use crate::*;
use gitturtle_core::{ChangeArea, ChangeStatus, StatusEntry, WriteCommand};
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::prelude::FluentBuilder;
use preferences::CommitDraft;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Copy)]
pub(super) enum WorkingRow {
    Heading(ChangeArea, usize),
    Directory(usize, ChangeArea),
    File(usize, ChangeArea),
}

#[cfg(test)]
thread_local! {
    /// The layout state the Working inspector last drew with, for tests.
    pub(crate) static WORKING_LAYOUT_STATE: std::cell::Cell<Option<EntityId>> =
        const { std::cell::Cell::new(None) };
}

pub(crate) struct WorkingInspectorLayout {
    height: Pixels,
    /// The inspector's width as last laid out, which its rows fit
    /// (`WorkingForms`).
    width: Pixels,
    list_size: Option<Size<Pixels>>,
    /// The whole inspector's height as last laid out, which no form of its
    /// rows changes, so it can choose the form (`WorkingFit`).
    inspector: Pixels,
    /// Where the composer's stops lay below the body's top, as far as the
    /// list's floor and the composer's rows place them, in the last frame
    /// where the list and the composer scrolled together
    /// (`GitTurtle::render_working_body`): the title, the description,
    /// Retry, Set your commit identity and Commit.
    scrolled_stops: Option<[f32; 5]>,
    /// The focus handles of the controls in the file list's rows, and where
    /// the one holding focus lies ([`RowControls`]).
    row_controls: RowControls,
}

// Tab and Shift+Tab on the Working page, while the list's rows' controls take
// their turn before the toolkit's own order goes on
// (`GitTurtle::tab_working_row_stops`). The toolkit's own Tab actions are
// private to it, so the window's root binds its own in a deeper key context,
// which a press tries first, and passes on what it does not move. The stop
// after the list is the composer's first only where the composer is drawn:
// with an operation in progress it lies outside the Working body, and a
// binding there left its Shift+Tab to the window's own order, which goes
// straight to the list. Only the window's root holds every such stop.
gpui_kit::actions!(gitturtle_working, [NextWorkingStop, PreviousWorkingStop]);

/// The key context the window's root adds while Working Changes shows, in
/// which Tab and Shift+Tab reach [`NextWorkingStop`] and
/// [`PreviousWorkingStop`] before the window's own.
const WORKING_PAGE: &str = "GitTurtleWorkingPage";

/// The window root's key context while Working Changes shows.
pub(crate) const WORKING_ROOT: &str = "GitTurtle GitTurtleWorkingPage";

/// Marks an application whose keymap holds the Working page's bindings.
struct WorkingKeysBound;

impl Global for WorkingKeysBound {}

/// Binds Tab and Shift+Tab in the Working page's key context, once in each
/// application, as the inspector is first rendered.
fn bind_working_keys(cx: &mut App) {
    if cx.has_global::<WorkingKeysBound>() {
        return;
    }
    cx.set_global(WorkingKeysBound);
    cx.bind_keys([
        KeyBinding::new("tab", NextWorkingStop, Some(WORKING_PAGE)),
        KeyBinding::new("shift-tab", PreviousWorkingStop, Some(WORKING_PAGE)),
    ]);
}

/// The focus handles of the controls in the file list's rows, a file's
/// Stage or Unstage and a section heading's Stage all or Unstage all, each
/// the same in every frame that draws its row, and where the one holding
/// focus lies in the frame being painted, which the floor's fades stop
/// short of (`floor_fades`). None is in GPUI's own Tab order: the list is
/// one stop, and Tab from it walks the selected row's control and the
/// headings' in the order `GitTurtle::tab_working_row_stops` gives them.
/// Keyed by the kit inside each Button, a
/// control's handle was out of the list's reach, so the floor's fades lay
/// over a focused Stage's ring at the floor's edge. A handle whose row the
/// last frame did not draw is given up, unless it holds focus while its
/// row is still among the list's rows, scrolled out of view, so focus there
/// stays on it and shows again with its row, as a revealing container
/// keeps its controls' handles (`FocusReveal::focus`). One that holds focus
/// as its row leaves the list, as a Stage's file moves to Staged, is given
/// up too, and focus goes to the list itself: kept, it went to the row's
/// Stage when a refresh brought the row back, which took the ring, and
/// Enter staged the file again. Rows that land from the path filter, and a
/// failed status read or a failed preparation of the rows that takes every
/// row away, hand that focus on before the frame that draws the list without
/// that row (`GitTurtle::release_departed_row_control`); the list's render
/// hands it on in that frame's deferred work only where the rows go
/// otherwise, as a tab's rows replaced as another repository opens, and that
/// one frame draws no ring.
#[derive(Clone, Default)]
pub(crate) struct RowControls(Rc<RefCell<RowControlsState>>);

/// The row whose control a handle tracks, as the list's rows name it.
#[derive(Clone)]
enum RowControl {
    /// A section heading's Stage all or Unstage all.
    Heading(ChangeArea),
    /// A file's Stage or Unstage, by its path in its area.
    File(std::path::PathBuf, ChangeArea),
}

/// A stop past the Working list itself in Tab's order
/// (`GitTurtle::working_row_stops`): its control's key among the
/// [`RowControls`], the row it stands in, and that row's place in the list.
struct RowStop {
    id: ElementId,
    row: RowControl,
    position: usize,
}

/// The key and selector of the Stage all or Unstage all in `area`'s heading.
fn heading_control(area: ChangeArea) -> &'static str {
    match area {
        ChangeArea::Staged => "unstage-all",
        ChangeArea::Unstaged => "stage-all",
    }
}

/// A row control's handle, the row it stands in, where the list last drew
/// that row, and whether the frame being drawn has drawn it.
struct KeptControl {
    focus: FocusHandle,
    row: RowControl,
    position: usize,
    drawn: bool,
}

#[derive(Default)]
struct RowControlsState {
    /// Each control's handle by its row's id.
    handles: std::collections::HashMap<ElementId, KeptControl>,
    /// The bounds of the control holding focus, as its row was prepainted in
    /// the frame being drawn; none where no row drawn holds it.
    focused: Option<Bounds<Pixels>>,
    /// Whether the rows' form draws a file's control (`WorkingForms`), as
    /// the list last rendered, for keys that land between frames.
    file_action: bool,
    /// The list pane's inner bounds as the frame paints it
    /// (`record_pane`).
    pane: Option<Bounds<Pixels>>,
}

impl RowControls {
    /// For the list's render, in each frame before its rows are drawn: gives
    /// up the handles of the controls the last frame did not draw, but one
    /// holding focus whose row the list still holds (`listed`), and forgets
    /// where the focused one lay. A handle holding focus whose row has left
    /// the list is given up whether or not the last frame drew it, and focus
    /// goes to the list itself (`list`), as it does when the selected row
    /// leaves, in the frame's deferred work: only the focused handle asks
    /// whether its row is listed, from where the list last drew it, so a
    /// list draws its rows at no cost for the others. `file_action` is
    /// whether this frame's rows draw a file's control.
    fn begin(
        &self,
        listed: impl Fn(&RowControl, usize) -> bool,
        list: &FocusHandle,
        file_action: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        let mut departed = None;
        {
            let mut state = self.0.borrow_mut();
            state.file_action = file_action;
            state.handles.retain(|_, kept| {
                let drawn = std::mem::take(&mut kept.drawn);
                if !kept.focus.is_focused(window) {
                    return drawn;
                }
                if listed(&kept.row, kept.position) {
                    return true;
                }
                departed = Some(kept.focus.clone());
                false
            });
            state.focused = None;
        }
        if let Some(departed) = departed {
            let list = list.clone();
            window.defer(cx, move |window, cx| {
                if departed.is_focused(window) {
                    window.focus(&list, cx);
                }
            });
        }
    }

    /// The handle of the control in the row `id`, drawn at `position` among
    /// the list's rows, which a Button takes through `track_focus`; `row`
    /// names that row the first time.
    fn focus(
        &self,
        id: ElementId,
        row: impl FnOnce() -> RowControl,
        position: usize,
        cx: &App,
    ) -> FocusHandle {
        let mut state = self.0.borrow_mut();
        let kept = state.handles.entry(id).or_insert_with(|| KeptControl {
            focus: cx.focus_handle(),
            row: row(),
            position,
            drawn: false,
        });
        kept.position = position;
        kept.drawn = true;
        kept.focus.clone()
    }

    /// `control`, a row's Stage or Unstage or a heading's Stage all or
    /// Unstage all, in the wrapper whose bounds stand for it, its only child:
    /// where `focus`, the handle it tracks, holds focus as its row is
    /// rendered, the wrapper records where it lies as it is prepainted. A
    /// focus change draws the list again, so no other row listens, and the
    /// wrapper reads the control's own bounds wherever it stands in its row.
    /// The travelling copy of a row in flight tracks no handle (`focus`
    /// none): its control would have tracked the one its row's does.
    fn control(
        controls: Option<&Self>,
        control: impl IntoElement,
        focus: Option<&FocusHandle>,
        window: &Window,
    ) -> Div {
        let wrapper = div().flex_none().flex().child(control);
        match controls.filter(|_| focus.is_some_and(|focus| focus.is_focused(window))) {
            Some(controls) => {
                let this = controls.clone();
                wrapper.on_children_prepainted(move |bounds, _, _| {
                    if let Some(control) = bounds.first() {
                        this.0.borrow_mut().focused = Some(*control);
                    }
                })
            }
            None => wrapper,
        }
    }

    /// Where the control holding focus lies in the frame being painted, if
    /// the list drew it.
    fn focused(&self) -> Option<Bounds<Pixels>> {
        self.0.borrow().focused
    }

    /// Whether the handle holding focus stands in a row that `listed`,
    /// asked from where the list last drew that row, no longer finds.
    fn departed(&self, listed: impl Fn(&RowControl, usize) -> bool, window: &Window) -> bool {
        self.0
            .borrow()
            .handles
            .values()
            .any(|kept| kept.focus.is_focused(window) && !listed(&kept.row, kept.position))
    }

    /// Whether the rows' form last drew a file's control.
    fn file_action(&self) -> bool {
        self.0.borrow().file_action
    }

    /// The key of the control holding focus, if one of these does.
    fn holding(&self, window: &Window) -> Option<ElementId> {
        self.0
            .borrow()
            .handles
            .iter()
            .find(|(_, kept)| kept.focus.is_focused(window))
            .map(|(id, _)| id.clone())
    }

    /// The handle of the control holding focus, if one of these does.
    fn held_handle(&self, window: &Window) -> Option<FocusHandle> {
        self.0
            .borrow()
            .handles
            .values()
            .find(|kept| kept.focus.is_focused(window))
            .map(|kept| kept.focus.clone())
    }

    /// Records the list pane's inner bounds as this frame paints it, for the
    /// outline drawn after the body's content ([`list_edge`]).
    fn record_pane(&self, bounds: Bounds<Pixels>) {
        self.0.borrow_mut().pane = Some(bounds);
    }

    /// The list pane's inner bounds, as [`Self::record_pane`] recorded them.
    fn pane(&self) -> Option<Bounds<Pixels>> {
        self.0.borrow().pane
    }

    /// Whether one of these handles holds focus.
    fn hold_focus(&self, window: &Window) -> bool {
        self.0
            .borrow()
            .handles
            .values()
            .any(|kept| kept.focus.is_focused(window))
    }

    /// The control holding focus, named by its selector: `working-stage-N`
    /// for a file's, `unstage-all` or `stage-all` for a heading's.
    #[cfg(test)]
    fn focused_selector(&self, window: &Window) -> Option<String> {
        self.0
            .borrow()
            .handles
            .values()
            .find(|kept| kept.focus.is_focused(window))
            .map(|kept| match kept.row {
                RowControl::File(..) => format!("working-stage-{}", kept.position),
                RowControl::Heading(area) => heading_control(area).to_string(),
            })
    }
}

/// Whether the list/composer body, `height` tall, takes the short form, with
/// the composer's tighter rhythm and heading. The form also shapes the
/// controls above the body, so it is chosen before layout from the body's
/// height as last laid out.
fn short_body(height: Pixels) -> bool {
    height < px(560.)
}

/// The Working inspector's list/composer body. It takes the height the
/// controls above it leave, at least `min`, and builds its list and composer
/// as it is prepainted, for the height it has in that frame
/// (`GitTurtle::render_working_body`), as `uniform_list` builds its rows: a
/// line that comes or goes above it moves nothing in it a frame late. Sized
/// from the frame before, the composer was 23 px taller for one frame after
/// "Filtering working paths…" left the controls.
struct WorkingBody {
    app: Entity<GitTurtle>,
    min: Pixels,
    parts: Option<WorkingBodyParts>,
}

/// What the body is built from that its height does not change, chosen as
/// the inspector renders.
struct WorkingBodyParts {
    /// Where the body's size is recorded for what is chosen before layout.
    layout: Entity<WorkingInspectorLayout>,
    /// The file list, but for the height the body gives it.
    pane: Stateful<Div>,
    short: bool,
    forms: WorkingForms,
    fit: WorkingFit,
    /// The scroll cue's gutter the body keeps at its right, which `forms`
    /// and the composer's rows are fitted beside; nothing where the body did
    /// not scroll as last laid out.
    gutter: Pixels,
    metrics: ComposerMetrics,
    /// The composer's fields' rows but the description's field
    /// (`GitTurtle::composer_rows`).
    rows: Pixels,
    /// Where its stops below the title begin among those rows.
    stops: ComposerStops,
    /// The Description section's own rows
    /// (`GitTurtle::description_section`).
    section: Pixels,
    /// The commit title's committed length (`GitTurtle::title_length`).
    title_length: usize,
    /// The file list's least height.
    floor: f32,
    /// Whether the composer is drawn, not an operation's banner.
    composer: bool,
    /// Whether the list draws rows, which may overflow it.
    listed: bool,
    staged: usize,
    busy: bool,
    identity_ready: bool,
    shown: bool,
    conflicted: bool,
    /// Whether Commit is refused for a reason of its own, whatever the
    /// window's busy spell: nothing staged, a conflict, no identity or no
    /// title.
    commit_unavailable: bool,
    commit_disabled: bool,
    commit_refused: bool,
}

impl IntoElement for WorkingBody {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for WorkingBody {
    type RequestLayoutState = ();
    /// The list and the composer, laid out in the body's bounds.
    type PrepaintState = Option<AnyElement>;

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
        // `flex_1`, at least `min` tall.
        let style = Style {
            flex_grow: 1.,
            flex_shrink: 1.,
            flex_basis: relative(0.).into(),
            min_size: Size {
                width: Length::Auto,
                height: self.min.into(),
            },
            ..Style::default()
        };
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let parts = self.parts.take()?;
        let mut body = self.app.update(cx, |app, cx| {
            app.render_working_body(parts, bounds.size, window, cx)
        });
        body.layout_as_root(
            size(
                AvailableSpace::Definite(bounds.size.width),
                AvailableSpace::Definite(bounds.size.height),
            ),
            window,
            cx,
        );
        body.prepaint_at(bounds.origin, window, cx);
        Some(body)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        body: &mut Option<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(body) = body {
            body.paint(window, cx);
        }
    }
}

/// Test-only: a 21 px line drawn above the Working inspector's list/composer
/// body, as "Filtering working paths…" was while a write's rows were
/// prepared, and a record of where each frame the window draws lays the list
/// and the composer out (`GitTurtle::working_probe`), so a test can check
/// that nothing in the body is laid out for the height it had a frame
/// before. A test window draws a frame that a layout asks for at once,
/// inside the update that asked, so `debug_bounds` read after that update
/// sees only the last of them.
#[cfg(test)]
#[derive(Default)]
pub(super) struct WorkingProbe {
    pub(super) line: std::cell::Cell<ProbeLine>,
    /// A record of each frame drawn while the probe is not `Off`.
    pub(super) frames: std::rc::Rc<RefCell<Vec<ProbeFrame>>>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum ProbeLine {
    /// No line, no record.
    #[default]
    Off,
    /// No line; each frame is recorded.
    Watch,
    /// The line in one frame, the first drawn while rows are prepared,
    /// then `Watch`.
    Once,
    /// The line in every frame.
    Steady,
}

/// Test-only: where one frame laid the list and the composer out, inside
/// their borders, and whether it drew the probe line above them.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ProbeFrame {
    pub(super) line: bool,
    pub(super) list: Option<Bounds<Pixels>>,
    pub(super) composer: Option<Bounds<Pixels>>,
    /// The composer's rows, inside the room they keep at its right.
    pub(super) rows: Option<Bounds<Pixels>>,
    /// The focus ring the list drew as the frame was painted, on its own
    /// edge or around the row control holding focus; none where it drew
    /// neither.
    pub(super) ring: Option<&'static str>,
}

#[cfg(test)]
impl WorkingProbe {
    /// Records a frame and says whether it draws the line, while rows are
    /// prepared where `pending`.
    fn frame(&self, pending: bool) -> bool {
        let line = match self.line.get() {
            ProbeLine::Off => return false,
            ProbeLine::Watch => false,
            ProbeLine::Steady => true,
            ProbeLine::Once => {
                if pending {
                    self.line.set(ProbeLine::Watch);
                }
                pending
            }
        };
        self.frames.borrow_mut().push(ProbeFrame {
            line,
            ..ProbeFrame::default()
        });
        line
    }

    /// A canvas over what `part` names in a frame's record, the list, the
    /// composer or the composer's rows, which records where this frame laid
    /// it out.
    fn recorder(
        &self,
        part: fn(&mut ProbeFrame) -> &mut Option<Bounds<Pixels>>,
    ) -> Option<impl IntoElement + use<>> {
        let frames = self.frames.clone();
        (self.line.get() != ProbeLine::Off).then(|| {
            canvas(
                move |bounds, _, _| {
                    if let Some(frame) = frames.borrow_mut().last_mut() {
                        *part(frame) = Some(bounds);
                    }
                },
                |_, _, _, _| (),
            )
            .absolute()
            .inset_0()
        })
    }

    /// A canvas that records which focus ring the list draws as the frame
    /// is painted: its own edge, where `ring` shows `list`, or the ring of
    /// the row control holding focus, which a focused Button draws, where
    /// the list drew that control (`controls`).
    fn ring_recorder(
        &self,
        ring: crate::focus_ring::FocusRing,
        list: FocusHandle,
        controls: RowControls,
    ) -> Option<impl IntoElement + use<>> {
        let frames = self.frames.clone();
        (self.line.get() != ProbeLine::Off).then(|| {
            canvas(
                |_, _, _| (),
                move |_, _, _, _| {
                    if let Some(frame) = frames.borrow_mut().last_mut() {
                        frame.ring = if ring.shows(&list) {
                            Some("the list's edge")
                        } else if controls.focused().is_some() {
                            Some("a row's control")
                        } else {
                            None
                        };
                    }
                },
            )
            .absolute()
            .inset_0()
        })
    }
}

/// A Git write's outcome in the status line ([`GitTurtle::record_outcome`]),
/// held while the control that asked confirms it. Window-lived and never
/// retained by a tab: it is drawn only while its repository is shown, and
/// its timer ends it.
pub(super) struct StatusOutcome {
    repository: PathBuf,
    text: String,
    _expiry: Task<()>,
}

/// The Changes header's height in the full form.
const WORKING_HEADER: f32 = 44.;

/// What the Working list says where its path filter matches nothing, as
/// the Tags list says "No tags match this filter".
pub(super) const WORKING_NO_MATCH: &str = "No files match this filter";
/// [`WORKING_NO_MATCH`] where it would wrap, as the commit's changed files
/// say it: three lines at 18 pt in a half tile left no room for Clear
/// filter below them.
pub(super) const WORKING_NO_MATCH_SHORT: &str = "No matches";
/// The no-match state's action, which empties the filter.
const CLEAR_FILTER: &str = "Clear filter";

/// How the Working Changes inspector's rows fit its height, chosen before
/// layout from the inspector's measured height and the heights its rows
/// would take, so no row is cut and no frame shows one form and then the
/// other.
#[derive(Clone, Copy, Debug, PartialEq)]
struct WorkingFit {
    /// The filter, the selection controls and the Changes header give way
    /// to one row: the filter beside Actions (and Refresh, where the filter
    /// keeps its room beside it), the selection controls moving into the
    /// Actions menu, so the file list keeps its floor.
    tight: bool,
    /// The composer keeps its pinned heading; in the tightest inspector it
    /// leaves, its branch staying in the commit action's tooltip and the
    /// action bar, its count in the Staged heading and the commit action.
    heading: bool,
}

/// The heights the composer's rows take, snapped to device pixels as layout
/// snaps them, from which its least height (`least`), its preferred height,
/// its Description section's field (`description_field`) and its drawn
/// height (`content`) are predicted before layout.
struct ComposerMetrics {
    /// The composer's top border.
    border: Pixels,
    /// The padding above the heading row (or, without it, above the fields)
    /// and below the commit action.
    padding: Pixels,
    /// Whether the pinned heading is drawn (`WorkingFit::heading`).
    heading: bool,
    /// The heading's line beside its 16 px icon and the staged count's
    /// badge; nothing where the heading has left.
    heading_row: Pixels,
    /// The gap between the fields' rows, which also leads them below the
    /// heading and ends them above the commit action, so its rows and the
    /// action keep one rhythm: a short composer's scaled 4 px, a taller one's
    /// 0.5 rem.
    gap: Pixels,
    /// A taller composer's branch line with the gap below it; nothing in a
    /// short one.
    branch: Pixels,
    /// A label's line: "Title", "Description".
    label: Pixels,
    /// The title's one-line field.
    field: Pixels,
    /// The commit action.
    action: Pixels,
    /// A small line's height: the hint's and a guidance note's.
    small: Pixels,
    /// A line of the description at its 12 px, whole device pixels or a
    /// little more, so a field of whole lines never cuts its last.
    text: Pixels,
}

impl ComposerMetrics {
    fn new(short: bool, heading: bool, window: &Window) -> Self {
        let rem = window.rem_size();
        let snap = |length: Pixels| window.pixel_snap(length);
        let line = |size: Pixels| snap(window.text_style().line_height.to_pixels(size.into(), rem));
        let heading_row = if heading {
            line(crate::appearance::ui_text(13.))
                .max(px(16.))
                .max(snap(rem * 0.125) * 2. + line(crate::appearance::ui_text(10.)))
        } else {
            // Without it the fields lead with the composer's own padding.
            px(0.)
        };
        // The fields keep this gap from the heading, between themselves and
        // above the commit action: a scaled 4 px in a short composer, 0.5 rem
        // in a taller one, whose fields lead with the branch line.
        let gap = snap(if short {
            crate::appearance::ui_size(4.)
        } else {
            rem * 0.5
        });
        let branch = if short {
            px(0.)
        } else {
            line(crate::appearance::ui_text(11.)).max(px(13.)) + gap
        };
        Self {
            border: snap(px(1.)),
            padding: snap(rem * if short { 0.5 } else { 0.75 }),
            heading,
            heading_row,
            gap,
            branch,
            label: line(crate::appearance::ui_text(11.)),
            field: snap(rem * 2.),
            action: snap(crate::appearance::ui_size(36.)),
            small: line(crate::appearance::ui_text(10.)),
            text: {
                let scale = window.scale_factor();
                let text = window
                    .text_style()
                    .line_height
                    .to_pixels(crate::appearance::ui_text(12.).into(), rem);
                px((f32::from(text) * scale).ceil() / scale)
            },
        }
    }

    /// The composer drawn with its rows whole and nothing more: its chrome,
    /// the fields' `rows` (`GitTurtle::composer_rows`) and the description's
    /// `field`, or, without one, as where an operation's banner stands in
    /// the composer's place, those rows less the Description section's own
    /// (`GitTurtle::description_section`): the section never leaves for
    /// room.
    fn content(&self, rows: Pixels, section: Pixels, field: Option<f32>) -> Pixels {
        self.chrome() + field.map_or(rows - section, |field| rows + px(field))
    }

    /// The scrolling fields' box in a composer `height` tall: what its
    /// border, the pinned heading and the commit action leave.
    fn fields_box(&self, height: Pixels) -> Pixels {
        height - self.chrome()
    }

    /// The fields' rows through the title's field, with the padding that
    /// leads them: the gap below the heading, or without it the composer's
    /// padding.
    fn title_rows(&self) -> Pixels {
        let lead = if self.heading { self.gap } else { self.padding };
        lead + self.branch + self.label + self.gap + self.field
    }

    /// What the composer takes beside its scrolling fields: its border, the
    /// pinned heading and the commit action.
    fn chrome(&self) -> Pixels {
        self.above_fields() + self.action + self.padding
    }

    /// What lies above the composer's fields: its border and the heading.
    fn above_fields(&self) -> Pixels {
        let heading = if self.heading {
            self.padding + self.heading_row
        } else {
            px(0.)
        };
        self.border + heading
    }

    /// The description's least field: one line inside its 8 px above and
    /// below.
    fn description_least(&self) -> Pixels {
        px(16.) + self.text
    }

    /// The composer's least height: drawn with its rows whole and one line
    /// of its description, whose label and wrapped hint count with it, for
    /// fields' `rows` (`GitTurtle::composer_rows`) and a Description section
    /// of `section`. The list/composer body keeps it beside the file list's
    /// floor (`working_list_floor`) where it can, so the controls above the
    /// list give way first, then the composer's heading (`WorkingFit`), and
    /// only then do the list and the composer scroll together, below the
    /// same height (`GitTurtle::render_working_body`), so no form keeps the
    /// heading and scrolls. Counted without the description, it let the
    /// heading scroll with the composer at 13 pt in a 461 × 490 window.
    fn least(&self, rows: Pixels, section: Pixels) -> Pixels {
        self.content(rows, section, Some(f32::from(self.description_least())))
    }

    /// The description's field in a composer `height` tall whose fields'
    /// other rows take `rows` (`GitTurtle::composer_rows`): `preferred`
    /// where it fits, else the whole lines that fit, so its Description
    /// section is whole. `None` where not even one line fits: the section
    /// never leaves for room, so the list gives up rows down to its floor,
    /// and where that is not enough the list and the composer scroll
    /// together (`GitTurtle::render_working_body`), rather than show the
    /// section's label cut at the commit action or a field drawn past it.
    fn description_field(
        &self,
        height: Pixels,
        rows: Pixels,
        preferred: f32,
        window: &Window,
    ) -> Option<f32> {
        let room = self.fields_box(height) - rows;
        if room >= px(preferred) {
            return Some(preferred);
        }
        // The room is a sum of device-pixel lengths, which at a fractional
        // scale such as 1.2 can come to a hair less than the whole lines it
        // holds: counted with no slack, a composer exactly its least height
        // kept no line (`least`), and the body scrolled where one line fit.
        let lines = ((room - px(16.) + px(0.01)) / self.text).floor();
        let field = px(16.) + self.text * lines;
        (lines >= 1.).then(|| f32::from(window.pixel_snap(field).min(room)))
    }
}

/// Where the composer's stops below the title begin among its fields' rows,
/// from the fields' top, but for the description's field
/// (`GitTurtle::composer_rows`): what a change of those rows moves each
/// stop by is known before layout (`GitTurtle::render_working_body`).
#[derive(Clone, Copy, Debug, Default)]
struct ComposerStops {
    /// The Description section, below the title's rows and any note.
    description: Pixels,
    /// The draft-save note and Retry, below the Description section.
    retry: Pixels,
    /// Set your commit identity, below the draft-save note and Retry.
    identity: Pixels,
}

/// How many of the file list's rows it keeps whole whatever else gives way:
/// those through its first file row, so the shortest list shows a file and
/// not only the headings above it (Staged, Unstaged while nothing is staged,
/// and a directory's label when grouped). A list without files keeps one.
fn working_floor_rows(rows: &[WorkingRow]) -> usize {
    rows.iter()
        .take(4)
        .position(|row| matches!(row, WorkingRow::File(..)))
        .map_or(1, |index| index + 1)
}

/// The composer's fields: a scroll view that reports to assistive
/// technology only what it shows, where they scroll on their own. Where the
/// list and the composer scroll together (`scrolls`), they scroll nothing,
/// and the body is the scroll view that reports what it shows
/// (`GitTurtle::render_working_body`): reported there too, the fields were
/// a second scroll view around the composer's stops.
fn composer_fields(scrolls: bool, fields: impl IntoElement) -> AnyElement {
    if scrolls {
        fields.into_any_element()
    } else {
        native_accessibility::clips_children(fields).into_any_element()
    }
}

/// The file list's least height: `rows` whole rows inside its border.
fn working_list_floor(rows: usize, density: crate::appearance::Density, window: &Window) -> f32 {
    f32::from(
        window.pixel_snap(px(density.file_row_height())) * rows as f32
            + window.pixel_snap(px(1.)) * 2.,
    )
}

/// The width the file list's no-match state has in an inspector `width`
/// wide, as layout draws it: the inspector on its nearest device pixel, as
/// it stands at the window's edge (a pinned inspector's width is reckoned
/// before layout, between two); the list 1 px in from its divider; and the
/// rows' text inset inside the list's border; the margin, the border and
/// the inset each on its whole device pixel, halves toward zero. Taken raw,
/// they made the room up to a device pixel narrower than drawn, at scale
/// 1.2 by the margin and the 0.75 rem inset, at scale 2 by the pinned
/// width (178.38 px for a state drawn 178.5 px wide at 18 pt), so Clear
/// filter took its icon in a list that held its label.
fn working_no_match_room(width: Pixels, window: &Window) -> Pixels {
    crate::image_compare::on_device_pixel(width, window)
        - window.pixel_snap(px(1.)) * 3.
        - window.pixel_snap(window.rem_size() * 0.75) * 2.
}

/// The fades over the file list at its floor in the short body, where the
/// list and the composer scroll together and the list keeps whole rows
/// through its first file under its own scroll, `scroll`: at each edge its
/// rows continue past, a band as wide as the list, opaque at the edge in the
/// panel the rows lie on and clear inward, `tags::EDGE_FADE` scaled and at
/// most a quarter of the list, as the shared cue's fades (`tags::ScrollCue`),
/// none at an end the list is scrolled to and none where its rows fit. The
/// body's track is the cue beside it, so the list draws no scrollbar of its
/// own; with neither, its rows past the floor once scrolled into view with
/// nothing to say they were there. Read as the frame paints, after the list
/// has laid out and clamped its offset, it follows a scroll, a resize or
/// rows that arrive in the frame that draws them; drawn over the rows and
/// inside the list's edge, it catches no pointer and lies over neither the
/// list's ring nor a row control's: a fade whose band reaches the ring of
/// the row control holding focus (`controls`), a file's Stage or a
/// heading's Stage all at the floor's edge, ends where that ring begins, as
/// the shared cue's do (`tags::fade_reach`), and returns whole in the frame
/// focus leaves. The foot's fade once lay 3 px over a focused Unstage's
/// ring at 18 pt in a 640 × 680 window.
fn floor_fades(scroll: UniformListScrollHandle, controls: RowControls) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, cx| {
            let scroll = scroll.0.borrow().base_handle.clone();
            let list = scroll.bounds();
            let reach = scroll.max_offset().y;
            if reach <= Pixels::ZERO {
                return;
            }
            let fade = appearance::ui_size(crate::tags::EDGE_FADE).min(list.size.height / 4.);
            let ring = controls
                .focused()
                .map(|control| control.dilate(appearance::button_ring_room(cx)));
            let (top_fade, bottom_fade) = crate::tags::fade_reach(list, fade, ring);
            // Half a pixel either way is an end, as for the shared cue.
            let slack = px(0.5);
            let scrolled = -scroll.offset().y;
            let surface: Hsla = rgb(palette(cx).panel).into();
            let band = |angle: f32| {
                linear_gradient(
                    angle,
                    linear_color_stop(surface.opacity(0.), 0.),
                    linear_color_stop(surface, 1.),
                )
            };
            if scrolled > slack && top_fade > Pixels::ZERO {
                let top = Bounds::new(list.origin, size(list.size.width, top_fade));
                window.paint_quad(fill(top, band(0.)));
            }
            if scrolled < reach - slack && bottom_fade > Pixels::ZERO {
                let bottom = Bounds::new(
                    point(list.left(), list.bottom() - bottom_fade),
                    size(list.size.width, bottom_fade),
                );
                window.paint_quad(fill(bottom, band(180.)));
            }
        },
    )
    .absolute()
    .size_full()
}

/// Whether the row control holding focus is out of the frame's drawn rows
/// while its ring would show: the person's own scrolling can take it there,
/// and no key does. Read as the frame paints, after its rows were prepainted.
fn held_control_undrawn(
    controls: &RowControls,
    ring: &crate::focus_ring::FocusRing,
    window: &Window,
) -> bool {
    controls.focused().is_none()
        && controls
            .held_handle(window)
            .is_some_and(|focus| ring.shows(&focus))
}

/// The list's accent outline, on the pane's outer line, painted after the
/// body's content: over the scroll cue's thumb where the body scrolls, so the
/// thumb passes beneath a whole ring. The one code path for the list's own
/// focus and for a control the list's edge stands in for
/// ([`held_control_undrawn`]), read as the frame paints, so it comes and goes
/// in the frame the control does. The pane's own border stays neutral. It
/// adds no hitbox, and the body's viewport clips it as the body clips the
/// list.
fn list_edge(
    controls: RowControls,
    ring: crate::focus_ring::FocusRing,
    list: FocusHandle,
    body: ScrollHandle,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, cx| {
            let Some(pane) = controls.pane() else {
                return;
            };
            if ring.shows(&list) || held_control_undrawn(&controls, &ring, window) {
                let border = window.pixel_snap(px(1.));
                let mask = ContentMask {
                    bounds: body.bounds(),
                };
                window.with_content_mask(Some(mask), |window| {
                    window.paint_quad(outline(
                        pane.dilate(border),
                        rgb(palette(cx).accent),
                        BorderStyle::Solid,
                    ));
                });
            }
        },
    )
    .absolute()
    .size_full()
}

/// The no-match line the file list draws in `room`: the whole line where it
/// fits on one line, otherwise its short form, which the whole line names.
fn working_no_match_line(room: Pixels, window: &Window) -> &'static str {
    if wrapped_lines(WORKING_NO_MATCH, empty_state::line_text(), room, window) == 1 {
        WORKING_NO_MATCH
    } else {
        WORKING_NO_MATCH_SHORT
    }
}

/// The file list's least height while it says that its path filter matches
/// nothing, in an inspector `width` wide: the line, wrapped in its room, and
/// Clear filter below it, inside the state's padding and the list's border,
/// so the state is drawn whole wherever the list keeps its floor.
fn working_no_match_floor(width: Pixels, window: &Window) -> f32 {
    let room = working_no_match_room(width, window);
    let line = working_no_match_line(room, window);
    let lines = wrapped_lines(line, empty_state::line_text(), room, window);
    f32::from(
        empty_state::block_height(lines, true, window)
            + appearance::ui_size(empty_state::GAP / 2.) * 2.
            + window.pixel_snap(px(1.)) * 2.,
    )
}

/// How the Working Changes inspector's rows fit its width, measured from the
/// interface font at the interface size. Where a row's labelled form fits,
/// it is drawn as before; narrower, its controls keep only their icons, with
/// their accessible names and tooltips that lead with those names, and its
/// texts leave whole rather than show a lone ellipsis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct WorkingForms {
    /// Directories, Stage and Unstage as 28 px icon controls on one row,
    /// where Stage and Unstage do not fit a row of their own labelled.
    pub(super) compact_selection: bool,
    /// The rows Directories, Stage and Unstage take: one, or two where the
    /// labelled pair wraps below Directories.
    selection_rows: usize,
    /// In the tight form's one row, Refresh stays beside Actions where the
    /// filter keeps a scaled 80 px beside both; otherwise it joins the
    /// Actions menu.
    toolbar_refresh: bool,
    /// The Changes header's Actions keeps only its caret, beside the icon-only
    /// Refresh, and "Refreshing…" leaves.
    compact_actions: bool,
    /// The Changes header's icon, then "Changes" with its count, each whole
    /// or absent: the icon leaves first, then the label and the count
    /// together, since the Changes tab or toggle in the header above already
    /// reads the count.
    header_icon: bool,
    header_label: bool,
    /// The Staged and Unstaged headings, fitted together so both keep one
    /// form: Unstage all and Stage all keep only their icons first, then the
    /// leading glyph leaves, then the group's name, the glyph returning beside
    /// the count to tell the groups apart, and last the glyph again. The count
    /// stays, naming its group in its tooltip once the name has left.
    section_action_label: bool,
    section_icon: bool,
    section_label: bool,
    /// A file row's status word, its 26 px status tile or, in its place,
    /// the tile's bare 16 px glyph, and its Stage or Unstage control, in the
    /// first of `FILE_FORMS` that keeps a short name such as "README.md"
    /// whole beside them, on every row together: the word leaves first, the
    /// tile gives way to its glyph before the control leaves, the bare
    /// glyph comes a `gap_1` from the name, and the glyph leaves last. The
    /// word joins the path's tooltip, the action the row's menu; the status
    /// stays in the tooltip and the row's accessible name.
    file_status: bool,
    file_tile: bool,
    file_action: bool,
    file_glyph: bool,
    /// The bare glyph a `gap_1` from the name rather than `gap_2`, the step
    /// before it would leave.
    file_glyph_close: bool,
    /// The file's folder below its name, drawn only beside a status mark,
    /// the tile or its glyph: status outranks location, so a row never says
    /// where its file lies without saying how it changed (at 18 pt in a 461
    /// × 490 window the rows once read their names and folders alone). The
    /// folder lies beneath the name and takes none of its room, so it
    /// leaves with the glyph's last step, never after it; the path's
    /// tooltip and the row's accessible name keep it.
    file_folder: bool,
    /// What the rows in the body keep at their right beyond their `px_3`
    /// inset while the body keeps the scroll cue's gutter: the gutter less
    /// that inset, since the cue's track lies over their trailing inset
    /// (`GitTurtle::render_working_inspector`); nothing otherwise.
    trailing: Pixels,
    /// The short composer heading's icon, then its staged badge, leave where
    /// the heading beside them could not keep "Commit to" and the branch's
    /// first character and ellipsis, so it truncates only the branch.
    heading_icon: bool,
    heading_badge: bool,
    /// The title counter keeps "suggested" after its count where the line
    /// beside "Title" has room; without it the word is in its tooltip and
    /// accessible name.
    counter_word: bool,
    /// The description's label keeps "· optional" after "Description" where
    /// they share the line; without it the field's accessible name keeps
    /// the word ("Commit description, optional").
    description_note: bool,
    /// The commit action reads "Commit" where its label does not fit beside
    /// its icon, with that label as its accessible name and leading its
    /// tooltip.
    compact_commit: bool,
    /// Its icon, which leaves where even "Commit" does not fit beside it,
    /// as beside the scroll cue's gutter at 18 pt in a 156.5 px inspector:
    /// a lone glyph is less clear than the word for a full-width primary
    /// action, so the word stays and is never cut, as the composer
    /// heading's icon leaves before its text.
    commit_icon: bool,
}

impl WorkingForms {
    /// A row's inset at its right in the body: its `px_3`, and beside the
    /// scroll cue's gutter the room the track takes beyond it, so the row's
    /// content and its controls' rings end where the track begins.
    fn row_end(&self, window: &Window) -> Pixels {
        row_end(
            self.trailing,
            window.rem_size() * 0.75,
            window.scale_factor(),
        )
    }
}

/// What the body's rows keep at their right beyond their `px_3`, `inset`,
/// while the body keeps the scroll cue's `gutter`, at `scale` device pixels
/// to the pixel ([`WorkingForms::trailing`]): the gutter less the inset as
/// the layout draws it, the composer's rows taking this room on their box
/// and their `px_3`, which the layout rounds to a device pixel, inside it.
/// The total is what is rounded up, so neither rounding brings a row's
/// content or a control's ring into the track: rounded alone, the room
/// with the composer's rounded inset fell short of the gutter, as at 13 pt
/// (9.25 px and 9.75), where the title's ring lay half a pixel over the
/// track, and at fractional scales such as 14 pt at 175 %.
pub(super) fn trailing_room(gutter: Pixels, inset: Pixels, scale: f32) -> Pixels {
    if gutter <= Pixels::ZERO {
        return Pixels::ZERO;
    }
    let drawn = (inset * scale).floor() / scale;
    ((gutter - drawn) * scale).ceil().max(Pixels::ZERO) / scale
}

/// A row's whole inset at its right in the body, its `px_3`, `inset`, with
/// the `trailing` room beside the scroll cue's gutter, rounded up as a whole
/// to `scale`'s device pixels ([`trailing_room`]); just the inset otherwise.
pub(super) fn row_end(trailing: Pixels, inset: Pixels, scale: f32) -> Pixels {
    if trailing <= Pixels::ZERO {
        return inset;
    }
    ((inset + trailing) * scale).ceil() / scale
}

/// The filter's, the title's and the description's placeholders, of which
/// each field shows as much as fits its width (`placeholder_fit`).
pub(super) const FILTER_PLACEHOLDER: &str = "Filter working paths…";
/// The filter's placeholder where the whole one does not fit: a shorter
/// whole word reads better than a cut one ("Filte…"). The History
/// inspector's changed-file filter shortens to it too.
pub(super) const FILTER_SHORT_PLACEHOLDER: &str = "Filter";
/// The filter's accessible name, whole whatever form its placeholder takes,
/// so a screen reader never hears only "Filter".
pub(super) const FILTER_NAME: &str = "Filter working paths";
pub(super) const TITLE_PLACEHOLDER: &str = "Summarize the change";
/// The title's placeholder where the whole one does not fit, rather than
/// "Summarize t…".
pub(super) const TITLE_SHORT_PLACEHOLDER: &str = "Summarize";
/// The title's placeholder where not even "Summarize" fits, as beside the
/// short body's scroll cue at 18 pt in a 461 × 490 window (89.7 px in an
/// 88.5 px field), rather than "Summari…".
pub(super) const TITLE_SHORTEST_PLACEHOLDER: &str = "Summary";
pub(super) const DESCRIPTION_PLACEHOLDER: &str = "Explain why this change is needed…";

/// The composer's texts below its title, whose wrapped lines the
/// composer's rows count (`GitTurtle::composer_rows`).
const LONG_TITLE_NOTE: &str = "A shorter title is easier to scan. This title is still valid.";
/// How long a busy spell runs before it looks busy (`GitTurtle::busy_shown`).
pub(crate) const BUSY_LOOK_DELAY: std::time::Duration = std::time::Duration::from_millis(200);
/// How long a save the person did not ask for waits, refused by the
/// preference writer's full queue, before it asks again
/// (`GitTurtle::retry_refused_save`). Asking costs one attempt to queue, so
/// it keeps asking for as long as the writer stays behind.
pub(crate) const REFUSED_SAVE_RETRY: std::time::Duration = std::time::Duration::from_millis(250);

/// The look of a busy spell: the progress label, Cancel, the tab's
/// "· working", "Refreshing…" and the disabled styling of the controls a
/// write or the reads after it make unavailable. It waits
/// [`BUSY_LOOK_DELAY`], so a write that lands with its rows sooner never
/// flashes it on controls the person did not touch. Acceptance does not
/// wait: those controls refuse at once (`write_from`, `depart_working_row`
/// and their own handlers).
#[derive(Default)]
pub(crate) struct BusyLook {
    shown: bool,
    /// Shows the look once the delay passes; dropping it cancels that.
    reveal: Option<Task<()>>,
}

/// The preview an LFS download was asked from, as `write_from` captures it
/// when the download starts: the file, the mode and the read generation.
pub(crate) type LfsView = (FileChange, WorkspaceMode, u64);

/// Why a control a running write refuses is unavailable, in its tooltip once
/// it looks busy and in the command palette's and the Linux Main Menu's row;
/// a GitHub request running as the busy spell gives its own reason
/// ([`GitTurtle::busy_reason`]).
pub(crate) const BUSY_REASON: &str = "Wait for the current Git operation to finish";

/// `control` as a busy spell leaves it: while `refused`, its own handler or
/// `write_from` refuses a press at once, and assistive technology hears it
/// unavailable at once with it, while it takes the disabled look only where
/// `dimmed`, once the spell looks busy ([`BusyLook`]). Through the delay a
/// control that looked available was announced enabled, though every press
/// was refused. `key` names it for tests (`refusals`).
pub(crate) fn refusing(
    control: Button,
    key: impl FnOnce() -> String,
    refused: bool,
    dimmed: bool,
) -> Button {
    #[cfg(test)]
    refusals::record(key(), refused || dimmed, dimmed);
    #[cfg(not(test))]
    let _ = key;
    control
        .disabled(dimmed)
        .accessibility_disabled(refused || dimmed)
}

/// `control` with `tooltip` as its text, drawn as the kit draws a text
/// tooltip, for a control the rows being prepared refuse
/// ([`GitTurtle::rows_wait`]): its text is drawn as `‹key›/tooltip/‹text›`,
/// so a test reads the tooltip shown under the pointer, as Projects' is read
/// ([`GitTurtle::refusing_with_reason`]).
pub(crate) fn waiting_tooltip(control: Button, key: &'static str, tooltip: String) -> Button {
    let tooltip = SharedString::from(tooltip);
    control.tooltip_view(move |window, cx| {
        let text = tooltip.clone();
        Tooltip::element(move |_, _| {
            let probe = text.clone();
            div()
                .debug_selector(move || format!("{key}/tooltip/{probe}"))
                .child(text.clone())
        })
        .build(window, cx)
    })
}

/// What a control that waits for a running write says in its tooltip
/// ([`GitTurtle::refusing_with_reason`]).
pub(crate) struct WaitingTooltip {
    /// What it is, which leads the tooltip.
    pub(crate) name: SharedString,
    /// Its shortcut, which ends the tooltip.
    pub(crate) shortcut: Option<SharedString>,
    /// Whether it looks refused, which adds why
    /// ([`GitTurtle::busy_reason`]).
    pub(crate) dimmed: bool,
}

impl WaitingTooltip {
    /// The name, `reason` while it looks refused, then the shortcut:
    /// every tooltip that names a shortcut ends with it ("History ·
    /// Primary+1") and a refusing one gives its reason after its name
    /// ("Open Projects · Wait for …"). Back to Projects once read
    /// "Back to Projects · Primary+[ · Wait for …". Formatted only as the
    /// tooltip shows ([`GitTurtle::refusing_with_reason`]): built with the
    /// control, it made a few allocations on every frame, however steady.
    fn text(&self, reason: &str) -> SharedString {
        #[cfg(test)]
        WAITING_TEXTS_FORMATTED.with(|formatted| formatted.set(formatted.get() + 1));
        self.compose(reason)
    }

    /// [`Self::text`]'s formatting, uncounted, as a test reads what a
    /// control was built with (`refusals::tooltip`).
    fn compose(&self, reason: &str) -> SharedString {
        let mut text = self.name.to_string();
        if self.dimmed {
            text.push_str(" · ");
            text.push_str(reason);
        }
        if let Some(shortcut) = &self.shortcut {
            text.push_str(" · ");
            text.push_str(shortcut);
        }
        text.into()
    }
}

#[cfg(test)]
thread_local! {
    /// How many times a [`WaitingTooltip`]'s text was formatted for its
    /// tooltip ([`WaitingTooltip::text`]), for a test.
    pub(crate) static WAITING_TEXTS_FORMATTED: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

impl GitTurtle {
    /// Why a control the running busy spell refuses waits: for the request
    /// of the GitHub panel that runs as the spell, which gives its own
    /// reason (`github_view::RequestKind::reason`), for a review another
    /// tab dropped, whose wait says where it runs in any tab but the one
    /// that asked for it (`page_navigation::finishing_elsewhere_reason`),
    /// or else for the Git operation ([`BUSY_REASON`]). Projects' tooltip,
    /// the palette and the window's Close once said to wait for "the
    /// current Git operation" while the GitHub panel loaded a PR or sent a
    /// review, or while another tab's dropped review finished.
    pub(crate) fn busy_reason(&self) -> &'static str {
        self.operation_busy
            .and_then(|label| {
                crate::page_navigation::finishing_elsewhere_reason(label).or_else(|| {
                    crate::github_view::running_request(label)
                        .map(crate::github_view::RequestKind::reason)
                })
            })
            .unwrap_or(BUSY_REASON)
    }

    /// [`refusing`], with a tooltip that says why once the control looks
    /// refused, as the header's Projects and History's Back to Projects say
    /// why they wait for a write. `tooltip` reads from the app what the
    /// control says and whether it looks refused, as it is built and again
    /// each time its tooltip renders, so a pointer resting on it reads the
    /// reason as the busy look begins and loses it as the look ends. The kit
    /// keeps a text tooltip as the pointer enters it (`Button::tooltip`):
    /// resting on Projects through a write still read "Open Projects" beside
    /// the dimmed button, and the reason after the write. `key` names the
    /// control for tests (`refusals`), and its tooltip `‹key›/tooltip/‹text›`.
    pub(crate) fn refusing_with_reason(
        &self,
        control: Button,
        key: &'static str,
        refused: bool,
        tooltip: fn(&GitTurtle) -> WaitingTooltip,
        cx: &Context<Self>,
    ) -> Button {
        // Its parts as the control is built, held inline or shared: the text
        // is formatted only as the tooltip shows, from the window's state
        // then, or from these where the window is gone, so a frame formats
        // none of it (a test records the parts, and formats them as it reads
        // them).
        let built = tooltip(self);
        let reason = self.busy_reason();
        #[cfg(test)]
        refusals::record_tooltip(key.into(), tooltip(self), reason);
        let owner = cx.entity().downgrade();
        let dimmed = built.dimmed;
        refusing(control, || key.into(), refused, dimmed).tooltip_view(move |window, cx| {
            let text = owner.upgrade().map_or_else(
                || built.text(reason),
                |app| {
                    let app = app.read(cx);
                    tooltip(app).text(app.busy_reason())
                },
            );
            Tooltip::element(move |_, _| {
                let probe = text.clone();
                div()
                    .debug_selector(move || format!("{key}/tooltip/{probe}"))
                    .child(text.clone())
            })
            .build(window, cx)
        })
    }
}

/// `input`, which a busy spell leaves `refused`: it takes no edit and is
/// announced read-only at once, and takes the disabled look only where
/// `dimmed`, as [`refusing`] has a control. Targets' fields dimmed the
/// moment any write started, a quick Stage's too.
pub(crate) fn refusing_input(
    input: Input,
    key: impl FnOnce() -> String,
    refused: bool,
    dimmed: bool,
) -> Input {
    #[cfg(test)]
    refusals::record(key(), refused || dimmed, dimmed);
    #[cfg(not(test))]
    let _ = key;
    input.disabled(dimmed).readonly(refused || dimmed)
}

/// What [`refusing`] last gave each control, for tests: the test platform
/// builds no accessibility tree.
#[cfg(test)]
pub(crate) mod refusals {
    use std::{cell::RefCell, collections::HashMap};

    /// Whether a control is announced unavailable, and whether it looks it.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub(crate) struct Refusal {
        pub announced: bool,
        pub dimmed: bool,
    }

    /// What a control the rows being prepared refuse was last built
    /// saying: the name of its own, if any, and its tooltip
    /// (`GitTurtle::rows_wait`).
    pub(crate) type Waiting = (Option<gpui_kit::SharedString>, Option<String>);

    thread_local! {
        static LAST: RefCell<HashMap<String, Refusal>> = Default::default();
        static TOOLTIPS: RefCell<HashMap<String, (super::WaitingTooltip, &'static str)>> =
            Default::default();
        static WAITING: RefCell<HashMap<&'static str, Waiting>> = Default::default();
    }

    pub(crate) fn record_waiting(
        key: &'static str,
        name: Option<gpui_kit::SharedString>,
        tooltip: Option<String>,
    ) {
        WAITING.with(|last| last.borrow_mut().insert(key, (name, tooltip)));
    }

    /// What the control `key` was last built saying ([`Waiting`]).
    pub(crate) fn waiting(key: &str) -> Option<Waiting> {
        WAITING.with(|last| last.borrow().get(key).cloned())
    }

    pub(crate) fn record(key: String, announced: bool, dimmed: bool) {
        LAST.with(|last| last.borrow_mut().insert(key, Refusal { announced, dimmed }));
    }

    /// Records the parts the control `key` was built with, its tooltip and
    /// the reason it gives, which [`tooltip`] formats as a test reads it:
    /// formatted as the control is built, the record made the frames a
    /// release benchmark times format it again on every frame.
    pub(crate) fn record_tooltip(
        key: String,
        tooltip: super::WaitingTooltip,
        reason: &'static str,
    ) {
        TOOLTIPS.with(|last| last.borrow_mut().insert(key, (tooltip, reason)));
    }

    /// What the control `key` was last built with.
    pub(crate) fn get(key: &str) -> Option<Refusal> {
        LAST.with(|last| last.borrow().get(key).copied())
    }

    /// The tooltip the control `key` was last built with
    /// (`GitTurtle::refusing_with_reason`).
    pub(crate) fn tooltip(key: &str) -> Option<gpui_kit::SharedString> {
        TOOLTIPS
            .with(|last| (last.borrow().get(key)).map(|(tooltip, reason)| tooltip.compose(reason)))
    }

    /// Forgets every control, so only those built after it are found.
    pub(crate) fn forget() {
        LAST.with(|last| last.borrow_mut().clear());
        TOOLTIPS.with(|last| last.borrow_mut().clear());
    }
}

const UNSTAGED_HINT: &str = "Stage files to include in your commit";
const STAGED_HINT: &str = "Only staged changes will be committed";
const DRAFT_SAVE_NOTE: &str =
    "Draft could not be saved on this computer. Keep the app open and retry.";
const CONFLICT_NOTE: &str = "Resolve conflicted files before committing.";

thread_local! {
    static SELECTION_LABELS: crate::views::TextWidth = const { RefCell::new(None) };
    static CHANGES_COUNT: crate::views::TextWidth = const { RefCell::new(None) };
    static STAGED_BADGE: crate::views::TextWidth = const { RefCell::new(None) };
    static HEADING_MIN: crate::views::TextWidth = const { RefCell::new(None) };
    static CHANGES_LABEL: RefCell<Option<(crate::views::LineKey, Pixels)>> =
        const { RefCell::new(None) };
    static ACTIONS_LABEL: RefCell<Option<(crate::views::LineKey, Pixels)>> =
        const { RefCell::new(None) };
    static REFRESHING_LABEL: RefCell<Option<(crate::views::LineKey, Pixels)>> =
        const { RefCell::new(None) };
    static DIRECTORIES_LABEL: RefCell<Option<(crate::views::LineKey, Pixels)>> =
        const { RefCell::new(None) };
    /// The wrapped lines of the selection controls' texts, by text, size and
    /// width, so a steady render lays none of them out.
    static WRAPPED_LINES: RefCell<Vec<WrappedText>> = const { RefCell::new(Vec::new()) };
    /// "Staged", "Unstaged", "Unstage all" and "Stage all".
    static SECTION_LABELS: RefCell<Option<(crate::views::LineKey, [Pixels; 4])>> =
        const { RefCell::new(None) };
    static STAGED_HEADING_COUNT: crate::views::TextWidth = const { RefCell::new(None) };
    static UNSTAGED_HEADING_COUNT: crate::views::TextWidth = const { RefCell::new(None) };
    /// A file row's name floor and its widest status word.
    static FILE_ROW_TEXTS: RefCell<Option<(crate::views::LineKey, [Pixels; 2])>> =
        const { RefCell::new(None) };
    static TITLE_NAME: RefCell<Option<(crate::views::LineKey, Pixels)>> =
        const { RefCell::new(None) };
    static TITLE_COUNTER: crate::views::TextWidth = const { RefCell::new(None) };
    /// "Description" and "· optional".
    static DESCRIPTION_LABEL: RefCell<Option<(crate::views::LineKey, Pixels)>> =
        const { RefCell::new(None) };
    static COMMIT_LABEL: crate::views::TextWidth = const { RefCell::new(None) };
    static COMMIT_WORD: RefCell<Option<(crate::views::LineKey, Pixels)>> =
        const { RefCell::new(None) };
    static CLEAR_FILTER_LABEL: crate::views::TextWidth = const { RefCell::new(None) };
    /// Each placeholder as it fits the width its field last painted at.
    static TITLE_FIT: PlaceholderFit = const { RefCell::new(None) };
    static DESCRIPTION_FIT: PlaceholderFit = const { RefCell::new(None) };
    static FILTER_FIT: PlaceholderFit = const { RefCell::new(None) };
}

/// A text's wrapped line count, with what it was laid out with: the font and
/// sizes, the text, its size and the width it wrapped in.
type WrappedText = (crate::views::LineKey, String, Pixels, Pixels, usize);

/// The lines `text` at `size` wraps to in `width`, laid out again only when
/// the text, the font, the sizes or the width change.
fn wrapped_lines(text: &str, size: Pixels, width: Pixels, window: &Window) -> usize {
    let key = crate::views::LineKey::current(window);
    WRAPPED_LINES.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(&(.., lines)) = cache
            .iter()
            .find(|(k, t, s, w, _)| *k == key && t == text && *s == size && *w == width)
        {
            return lines;
        }
        #[cfg(test)]
        crate::views::LINE_LAYOUTS.with(|count| count.set(count.get() + 1));
        let style = window.text_style();
        let lines = window
            .text_system()
            .shape_text(
                SharedString::from(text.to_owned()),
                size,
                &[style.to_run(text.len())],
                Some(width),
                None,
            )
            .map_or(1, |lines| {
                lines
                    .iter()
                    .map(|line| line.wrap_boundaries.len() + 1)
                    .sum()
            });
        if cache.len() >= 8 {
            cache.remove(0);
        }
        cache.push((key, text.to_owned(), size, width, lines));
        lines
    })
}

/// A placeholder as it fits a field's width, with the font and sizes and the
/// width it was fitted to.
pub(super) type PlaceholderFit = RefCell<Option<(crate::views::LineKey, Pixels, SharedString)>>;

/// The status words a file row shows (`render_working_row`).
const FILE_STATUS_WORDS: [&str; 6] = ["Conflict", "New", "Modified", "Deleted", "Renamed", "Type"];

/// The name a file row keeps whole at least, whatever leaves beside it: a
/// short name that nearly every repository's root holds, its capitals wider
/// than a lowercase name such as "notes.txt".
const FILE_NAME_FLOOR: &str = "README.md";

/// A file row's forms, widest first, as (status word, status tile, Stage or
/// Unstage control, bare status glyph, the glyph `gap_1` from the name): see
/// `WorkingForms::file_status`. The word leaves first; then the tile gives
/// way to its glyph, so the control stays; then the control leaves, and the
/// tile returns where it fits without it; then the bare glyph comes a
/// `gap_1` from the name; last the glyph leaves, the folder below the name
/// with it (`WorkingForms::file_folder`), only where the name's floor needs
/// the glyph's room.
pub(super) const FILE_FORMS: [(bool, bool, bool, bool, bool); 7] = [
    (true, true, true, false, false),
    (false, true, true, false, false),
    (false, false, true, true, false),
    (false, true, false, false, false),
    (false, false, false, true, false),
    (false, false, false, true, true),
    (false, false, false, false, false),
];

/// The Staged and Unstaged headings' forms, widest first, as
/// (labelled action, glyph, name): see `WorkingForms::section_action_label`.
const SECTION_FORMS: [(bool, bool, bool); 5] = [
    (true, true, true),
    (false, true, true),
    (false, false, true),
    (false, true, false),
    (false, false, false),
];

impl GitTurtle {
    /// The commit title's length in characters as committed, which its
    /// counter reads and its long-title note follows: its text without the
    /// part an input method is still composing, its marked range, which is
    /// not yet text (Pinyin's letters become one character as they are
    /// committed). The counter counted it: while fcitx5 composed "U+4e2d" in
    /// an empty title it read "6/72 suggested", for what was committed as
    /// "中", one character (QA-92). Read once a frame, as the inspector
    /// renders.
    fn title_length(&self, window: &mut Window, cx: &mut App) -> usize {
        use gpui::EntityInputHandler as _;
        self.commit_title.update(cx, |title, cx| {
            let marked = title.marked_text_range(window, cx);
            committed_length(&title.value(), marked)
        })
    }

    /// `WorkingForms` for an inspector `width` wide, whose list and composer
    /// lay their rows out as if `body` wide: less what the scroll cue's
    /// gutter takes beyond their trailing inset where they keep it, which
    /// the rows keep at their right (`WorkingForms::trailing`). Each text is
    /// laid out again only when it, the font or the sizes change, so a
    /// steady render asks the text system nothing.
    /// `short` is the composer's short form, whose heading names the branch,
    /// and `title_length` the title's committed length (`title_length`).
    fn working_forms(
        &self,
        width: Pixels,
        body: Pixels,
        short: bool,
        title_length: usize,
        window: &Window,
        cx: &App,
    ) -> WorkingForms {
        use crate::appearance::{ui_size, ui_text};
        use crate::views::{LineKey, cached_text_width, cached_width, shaped_width};
        let key = LineKey::current(window);
        let rem = window.rem_size();
        let regular = window.text_style().font_weight;
        let measure = |text: &str, size: Pixels, weight: FontWeight| {
            shaped_width(text, size, weight, window).ceil()
        };
        let (gap, tight) = (rem * 0.5, rem * 0.25);
        // Every row's `px_3`, the inspector's one leading inset.
        let room = width - rem * 0.75 * 2.;
        // The same inset inside the body, beside any gutter it keeps.
        let body_room = body - rem * 0.75 * 2.;
        let control = ui_size(28.);
        let sum = |items: &[Pixels], gap: Pixels| {
            items.iter().copied().sum::<Pixels>() + gap * items.len().saturating_sub(1) as f32
        };

        // Directories, Stage and Unstage. Stage and Unstage wrap together,
        // below Directories, before they keep only their icons.
        let (staged, unstaged) = self.selection_counts();
        let (stage, unstage) = (format!("Stage {unstaged}"), format!("Unstage {staged}"));
        let pair = cached_text_width(
            &SELECTION_LABELS,
            &format!("{stage}\n{unstage}"),
            window,
            || measure(&stage, ui_text(12.), regular) + measure(&unstage, ui_text(12.), regular),
        );
        let compact_selection = ui_size(10.) * 4. + pair + tight > room;
        let directories = cached_width(&DIRECTORIES_LABEL, &key, || {
            measure("Directories", ui_text(12.), regular)
        }) + ui_size(10.) * 2.;
        let selection_rows = if compact_selection
            || directories + tight + ui_size(10.) * 4. + pair + tight <= room
        {
            1
        } else {
            2
        };
        // The tight row: the filter, then Actions' caret and Refresh, each a
        // 28 px control a `gap_1` apart.
        let toolbar_refresh = room - (control + tight) * 2. >= ui_size(80.);

        // The Changes header: its icon, label and count, then Actions and
        // Refresh at the trailing edge.
        let total = self
            .work_status
            .as_ref()
            .map_or(0, |status| status.entries.len());
        let count = total.to_string();
        let chip = rem * 0.5 * 2.
            + cached_text_width(&CHANGES_COUNT, &count, window, || {
                measure(&count, ui_text(11.), FontWeight::MEDIUM)
            });
        let label = cached_width(&CHANGES_LABEL, &key, || {
            measure("Changes", ui_text(13.), FontWeight::SEMIBOLD)
        });
        let icon = px(17.);
        // The labelled compact helper: its padding either side, its label,
        // its 6 px gap and the caret, 0.875 rem square.
        let actions = (cached_width(&ACTIONS_LABEL, &key, || {
            shaped_width("Actions", ui_text(12.), regular, window)
        }) + ui_size(10.) * 2.
            + ui_size(6.)
            + rem * 0.875)
            .ceil();
        let refreshing = if self.status_task.is_some() {
            cached_width(&REFRESHING_LABEL, &key, || {
                measure("Refreshing…", ui_text(10.), regular)
            }) + gap
        } else {
            px(0.)
        };
        let full = sum(&[icon, label, chip], gap) + gap + refreshing + actions + gap + control;
        let compact_actions = full > room;
        let (mut header_icon, mut header_label) = (true, true);
        if compact_actions {
            let trailing = control * 2. + tight;
            let fits = |items: &[Pixels]| sum(items, gap) + gap + trailing <= room;
            if !fits(&[icon, label, chip]) {
                header_icon = false;
                header_label = fits(&[label, chip]);
            }
        }

        // The Staged and Unstaged headings, inside the file list's border and
        // its 1 px margin: the glyph, the group's name and count, and Unstage
        // all or Stage all at the trailing edge. Both take the first form in
        // which each fits.
        let (mut section_action_label, mut section_icon, mut section_label) = (true, true, true);
        if let Some((staged_rows, unstaged_rows)) = self.section_counts() {
            let [staged_name, unstaged_name, unstage_all, stage_all] =
                cached_width(&SECTION_LABELS, &key, || {
                    let name = |text| measure(text, ui_text(12.), FontWeight::SEMIBOLD);
                    let action = |text| measure(text, ui_text(12.), regular);
                    [
                        name("Staged"),
                        name("Unstaged"),
                        action("Unstage all"),
                        action("Stage all"),
                    ]
                });
            let count = |cache, rows: usize| {
                let text = rows.to_string();
                cached_text_width(cache, &text, window, || {
                    measure(&text, ui_text(11.), regular)
                })
            };
            // The labelled helper: its padding either side, icon and gap.
            let labelled = |text: Pixels| ui_size(10.) * 2. + ui_size(16.) + ui_size(6.) + text;
            let rows = [
                (
                    staged_name,
                    count(&STAGED_HEADING_COUNT, staged_rows),
                    labelled(unstage_all),
                ),
                (
                    unstaged_name,
                    count(&UNSTAGED_HEADING_COUNT, unstaged_rows),
                    labelled(stage_all),
                ),
            ];
            let section_room = body_room - px(3.);
            let fits = |(action_label, glyph, label): (bool, bool, bool)| {
                rows.iter().all(|&(name, count, action)| {
                    let glyph = if glyph { px(15.) + gap } else { px(0.) };
                    let name = if label { name + gap } else { px(0.) };
                    glyph + name + count + gap + if action_label { action } else { control }
                        <= section_room
                })
            };
            (section_action_label, section_icon, section_label) = SECTION_FORMS
                .into_iter()
                .find(|&form| fits(form))
                .unwrap_or((false, false, false));
        }

        // A file row, inside the list's 1 px margin and its border either
        // side: its leading inset, the list's (`PANE_INSET` scaled: its `pl`
        // and the 2 px selection marker), the 26 px status tile, the name,
        // then the status word and the Stage or Unstage control, and `pr_3`.
        // So the name keeps its floor, the row takes the first of
        // `FILE_FORMS` that fits: decoration gives way before an action,
        // and the name, the content, last.
        let [name_floor, status_word] = cached_width(&FILE_ROW_TEXTS, &key, || {
            let widest = FILE_STATUS_WORDS
                .iter()
                .map(|word| measure(word, ui_text(10.), regular))
                .fold(px(0.), Pixels::max);
            [
                measure(FILE_NAME_FLOOR, ui_text(12.), FontWeight::MEDIUM),
                widest,
            ]
        });
        let file_room = body - px(3.) - ui_size(crate::appearance::PANE_INSET) - rem * 0.75;
        let fits = |(word, tile, action, glyph, close): (bool, bool, bool, bool, bool)| {
            let lead = if tile {
                px(26.) + gap
            } else if glyph {
                px(16.) + if close { tight } else { gap }
            } else {
                px(0.)
            };
            let word = if word { gap + status_word } else { px(0.) };
            let action = if action { gap + control } else { px(0.) };
            lead + name_floor + word + action <= file_room
        };
        let (file_status, file_tile, file_action, file_glyph, file_glyph_close) = FILE_FORMS
            .into_iter()
            .find(|&form| fits(form))
            .unwrap_or((false, false, false, false, false));

        // The composer heading: "Commit to" and at least the branch's first
        // character and ellipsis, a truncated line keeping the characters
        // that fit strictly before its ellipsis, as `scope_name_min_width`
        // measures a cut scope name.
        // A marked tab's composer has no staged count to show.
        let branch = self.composer_branch();
        let (mut heading_icon, mut heading_badge) = (true, branch.is_some());
        if short {
            let heading = composer_heading(branch);
            let heading_min = cached_text_width(&HEADING_MIN, &heading, window, || {
                let size = ui_text(13.);
                let whole = shaped_width(&heading, size, FontWeight::SEMIBOLD, window).ceil();
                branch
                    .and_then(|branch| branch.chars().next())
                    .map_or(whole, |first| {
                        let cut = format!("Commit to {first}…");
                        (shaped_width(&cut, size, FontWeight::SEMIBOLD, window).floor() + px(1.))
                            .min(whole)
                    })
            });
            let text = format!("{} staged", self.working_paths.staged_count);
            let badge = rem * 0.375 * 2.
                + cached_text_width(&STAGED_BADGE, &text, window, || {
                    measure(&text, ui_text(10.), FontWeight::MEDIUM)
                });
            if sum(&[px(16.), heading_min, badge], gap) > body_room {
                heading_icon = false;
                heading_badge &= sum(&[heading_min, badge], gap) <= body_room;
            }
        }

        // The title's line: "Title", then its counter at the trailing edge.
        let title = cached_width(&TITLE_NAME, &key, || {
            measure("Title", ui_text(11.), FontWeight::MEDIUM)
        });
        let counter = title_counter(title_length);
        let counter = cached_text_width(&TITLE_COUNTER, &counter, window, || {
            measure(&counter, ui_text(11.), regular)
        });
        let counter_word = title + gap + counter <= body_room;
        // The description's label: "Description", `gap_1`, "· optional".
        let description_note = cached_width(&DESCRIPTION_LABEL, &key, || {
            measure("Description", ui_text(11.), FontWeight::MEDIUM)
                + tight
                + measure("· optional", ui_text(11.), regular)
        }) <= body_room;

        // The commit action, as wide as the composer: the kit's Button at its
        // default size, `px_2p5` either side, its 16 px icon, `gap_2` and its
        // label at 1 rem, inside the 1 px edge a disabled action keeps.
        let label = self.commit_action_label(cx);
        let label = cached_text_width(&COMMIT_LABEL, &label, window, || {
            measure(&label, rem, regular)
        });
        let compact_commit = rem * 0.625 * 2. + px(16.) + gap + label + px(2.) > body_room;
        let word = cached_width(&COMMIT_WORD, &key, || measure("Commit", rem, regular));
        let commit_icon =
            !compact_commit || rem * 0.625 * 2. + px(16.) + gap + word + px(2.) <= body_room;
        WorkingForms {
            compact_selection,
            selection_rows,
            toolbar_refresh,
            compact_actions,
            header_icon,
            header_label,
            section_action_label,
            section_icon,
            section_label,
            file_status,
            file_tile,
            file_action,
            file_glyph,
            file_glyph_close,
            file_folder: file_tile || file_glyph,
            heading_icon,
            heading_badge,
            counter_word,
            description_note,
            compact_commit,
            commit_icon,
            trailing: width - body,
        }
    }

    /// The Changes filter's placeholder fitted to its field, for the field's
    /// positioned wrapper in either form: the one row of the tight form,
    /// where the field can be 86 px at 18 pt, or its own row above the
    /// selection controls.
    pub(super) fn fit_filter_placeholder(&self) -> impl IntoElement {
        placeholder_fit(
            self.working_filter.clone(),
            &FILTER_FIT,
            &[FILTER_PLACEHOLDER, FILTER_SHORT_PLACEHOLDER],
            12.,
        )
    }

    /// The branch the composer commits to, as the header reads it: the one
    /// checked out, or "detached HEAD". None where the shown tab is marked,
    /// whose branch would be the one from the last read
    /// ([`GitTurtle::header_status`]).
    fn composer_branch(&self) -> Option<&str> {
        (!self.shown_tab_unavailable()).then(|| {
            self.header_status()
                .and_then(|status| status.branch.as_deref())
                .unwrap_or("detached HEAD")
        })
    }

    /// The commit action's label: what it commits, or why it cannot. It
    /// reads "Working…" only once a write looks busy (`BusyLook`), as the
    /// action dims only then: it read so the moment any write started, a
    /// quick Stage's too, for the one frame before its rows landed.
    pub(super) fn commit_action_label(&self, cx: &App) -> String {
        let staged = self.working_paths.staged_count;
        if self.operation_shown().is_some() {
            "Working…".to_owned()
        } else if self.shown_tab_unavailable() {
            // Whatever is read or kept is from before the mark.
            "Changes unavailable".to_owned()
        } else if self.work_status.is_none()
            || self.working_filter_waiting()
            || self.working_paths.error.is_some()
        {
            // Rows kept while new ones are prepared keep their counts, and
            // the action keeps its label (disabled until they land).
            if self.working_filter_waiting() {
                "Filtering working paths…"
            } else if self.working_read_pending() {
                "Reading working changes…"
            } else {
                "Changes unavailable"
            }
            .to_owned()
        } else if self.working_paths.conflicted {
            "Resolve conflicts to commit".to_owned()
        } else if !self
            .profile
            .as_ref()
            .is_some_and(|p| !p.name.is_empty() && !p.email.is_empty())
        {
            "Set your commit identity".to_owned()
        } else if staged == 0 {
            "Stage files to commit".to_owned()
        } else if self.commit_title.read(cx).value().trim().is_empty() {
            "Write a commit title".to_owned()
        } else {
            format!("Commit {}", crate::counted(staged, "file", "files"))
        }
    }

    /// The full selection controls' height at the inspector's `width`, as
    /// `render_working_selection` lays them out: its padding, the filter's
    /// field, the selection rows, the selection count's wrapped lines and any
    /// filtering notice, each a `gap_1` apart.
    fn working_controls_height(&self, forms: WorkingForms, width: Pixels, window: &Window) -> f32 {
        use crate::appearance::{ui_size, ui_text};
        let rem = window.rem_size();
        let room = width - rem * 0.75 * 2.;
        let gap = rem * 0.25;
        let line = |size: Pixels| window.text_style().line_height.to_pixels(size.into(), rem);
        let rows = forms.selection_rows as f32;
        let mut height = rem * 0.5 * 2. + rem * 2. + gap + ui_size(28.) * rows + gap * (rows - 1.);
        let (staged, unstaged) = self.selection_counts();
        let count = format!(
            "{} selected · filtering clears selection",
            staged + unstaged
        );
        let mut texts = vec![(count, ui_text(10.))];
        if self.working_filter_waiting() {
            texts.push(("Filtering working paths…".into(), ui_text(11.)));
        }
        if let Some(error) = &self.working_paths.error {
            texts.push((error.clone(), ui_text(11.)));
        }
        for (text, size) in texts {
            height += gap + line(size) * wrapped_lines(&text, size, room, window) as f32;
        }
        f32::from(height)
    }

    /// The inspector's `WorkingFit` at its measured `height` and `width`,
    /// where the list/composer body needs `body`: the list's floor and, with
    /// the composer, the composer's least height with its heading
    /// (`ComposerMetrics::least`).
    fn working_fit(
        &self,
        forms: WorkingForms,
        height: Pixels,
        width: Pixels,
        body: f32,
        window: &Window,
    ) -> WorkingFit {
        let height = f32::from(height);
        let full = self.working_controls_height(forms, width, window) + WORKING_HEADER;
        if full + body <= height {
            return WorkingFit {
                tight: false,
                heading: true,
            };
        }
        WorkingFit {
            tight: true,
            heading: working_toolbar_height(window) + body <= height,
        }
    }

    /// The height the composer's scrolling fields take, `width` wide, but for
    /// the description's field: the title's rows and any note below them,
    /// the Description section's label and hint with the gaps around its
    /// field (`description_section`), the guidance after it, and the gap
    /// above the commit action, wrapped texts laid out once per text, size
    /// and width (`wrapped_lines`), and where among them the stops below the
    /// title begin. `shown`: the list shows a status's rows, its own or
    /// those kept while a newer one's are prepared. `title_length`: the
    /// title's committed length (`title_length`).
    fn composer_rows(
        &self,
        metrics: &ComposerMetrics,
        width: Pixels,
        identity_ready: bool,
        shown: bool,
        title_length: usize,
        window: &Window,
    ) -> (Pixels, ComposerStops) {
        use crate::appearance::{ui_size, ui_text};
        let rem = window.rem_size();
        let gap = metrics.gap;
        // Every row's `px_3`, the inspector's one leading inset.
        let room = width - rem * 0.75 * 2.;
        let lines =
            |text: &str, size: Pixels, room: Pixels| wrapped_lines(text, size, room, window) as f32;
        let mut rows = metrics.title_rows();
        if title_length > 72 {
            rows += gap + metrics.small * lines(LONG_TITLE_NOTE, ui_text(10.), room);
        }
        let description = rows;
        rows += self.description_section(metrics, width, window);
        let retry = rows;
        // The guidance after it, which the section leaves room for.
        if self.draft_save_error.is_some() {
            rows += gap
                + metrics.small * lines(DRAFT_SAVE_NOTE, ui_text(10.), room)
                + rem * 0.25
                + ui_size(28.);
        }
        let identity = rows;
        if !identity_ready && shown {
            rows += gap + ui_size(28.);
        }
        if self.working_paths.conflicted {
            let text = room - px(15.) - rem * 0.5;
            rows += gap + px(15.).max(metrics.label * lines(CONFLICT_NOTE, ui_text(11.), text));
        }
        let stops = ComposerStops {
            description,
            retry,
            identity,
        };
        (rows + metrics.gap, stops)
    }

    /// The Description section's rows in the composer's fields, `width`
    /// wide, but for its field: the gap that leads it, its label, and the
    /// hint, a gap either side of the field.
    fn description_section(
        &self,
        metrics: &ComposerMetrics,
        width: Pixels,
        window: &Window,
    ) -> Pixels {
        let gap = metrics.gap;
        // Every row's `px_3`, the inspector's one leading inset.
        let room = width - window.rem_size() * 0.75 * 2.;
        let size = crate::appearance::ui_text(10.);
        gap + metrics.label
            + gap
            + gap
            + metrics.small * wrapped_lines(self.description_hint(), size, room, window) as f32
    }

    /// The hint below the commit's Description, which is also that field's
    /// accessible description.
    fn description_hint(&self) -> &'static str {
        if self.working_paths.staged_count == 0 {
            UNSTAGED_HINT
        } else {
            STAGED_HINT
        }
    }

    /// The counts the Staged and Unstaged headings show, the first row and
    /// the first Unstaged one, found by bisection: the rows hold every Staged
    /// row before every Unstaged one.
    fn section_counts(&self) -> Option<(usize, usize)> {
        let area = |row: &WorkingRow| match row {
            WorkingRow::Heading(area, _)
            | WorkingRow::Directory(_, area)
            | WorkingRow::File(_, area) => *area,
        };
        let unstaged = self
            .working_rows
            .partition_point(|row| area(row) == ChangeArea::Staged);
        match (self.working_rows.first(), self.working_rows.get(unstaged)) {
            (Some(WorkingRow::Heading(_, staged)), Some(WorkingRow::Heading(_, unstaged))) => {
                Some((*staged, *unstaged))
            }
            _ => None,
        }
    }

    /// Brings the heading whose Stage all or Unstage all confirms its write
    /// into the list's view once the rows that write produced arrive
    /// (`schedule_working_filter` after the explicit refresh that follows
    /// it), so its Staged or Unstaged is seen on the control that was
    /// pressed. Stage all moves every file into Staged above its own
    /// heading, which went past the viewport's foot with its confirmation
    /// still on it, and centring a selected file could carry Unstage all's
    /// heading above the top. The list moves in one frame, the least that
    /// shows the heading whole, and only so far as `selected`, the selected
    /// file's row, stays in view, as it does when lists change: where the
    /// two cannot be seen together, the selected file keeps the place the
    /// refresh gives it, and the outcome is still announced and kept in the
    /// status line.
    pub(super) fn reveal_confirming_heading(
        &self,
        selected: Option<usize>,
        window: &Window,
        cx: &App,
    ) {
        let Some(heading) = self.confirming_heading(cx) else {
            return;
        };
        // A list never laid out has no viewport to reveal anything in.
        let Some(size) = self.working_scroll.0.borrow().last_item_size else {
            return;
        };
        reveal_heading(
            &self.working_scroll,
            size.item.height,
            window.pixel_snap(px(self.settings.density.file_row_height())),
            self.working_rows.len(),
            selected,
            heading,
        );
    }

    /// The row of the heading whose Stage all or Unstage all reads its
    /// confirmation now, if any.
    fn confirming_heading(&self, cx: &App) -> Option<usize> {
        [ChangeArea::Staged, ChangeArea::Unstaged]
            .into_iter()
            .filter(|area| copy_feedback::confirmed(&confirmations::all(&self.path, *area), cx))
            .find_map(|confirming| {
                self.working_rows.iter().position(
                    |row| matches!(row, WorkingRow::Heading(area, _) if *area == confirming),
                )
            })
    }
}

/// Scrolls `scroll`, a list of `rows` rows `row` tall in a `viewport` tall
/// view, to show row `heading` as [`heading_reveal`] decides, beside the
/// `selected` row. The move replaces any scroll the list was about to make,
/// such as the refresh's centring of the selected row, which the decision
/// accounts for, and lands when the list is next laid out, with its viewport
/// then.
fn reveal_heading(
    scroll: &UniformListScrollHandle,
    viewport: Pixels,
    row: Pixels,
    rows: usize,
    selected: Option<usize>,
    heading: usize,
) {
    let top = -scroll.0.borrow().base_handle.offset().y;
    match heading_reveal(top, viewport, row, rows, selected, heading) {
        Some(Reveal::Nearest) => scroll.scroll_to_item(heading, ScrollStrategy::Nearest),
        Some(Reveal::Edge(strategy)) => scroll.scroll_to_item_strict(heading, strategy),
        None => {}
    }
}

/// The look of a row's control drawn as a picture of itself
/// (`render_working_row` without `RowControls`): its resting
/// label, and a pressed fill that is its resting fill, so a press on the row
/// in flight lights nothing. The kit applies a Button's pressed style to any
/// Button that is not disabled, the picture's among them, and the still
/// pointer's variant keeps the shared pressed fill for a control that acts.
fn picture_control_variant(
    p: appearance::Palette,
    cx: &App,
) -> gpui_kit::component::button::ButtonVariant {
    gpui_kit::component::button::ButtonVariant::Custom(
        gpui_kit::component::button::ButtonCustomVariant::new(cx)
            .foreground(rgb(p.control_label()).into()),
    )
}

/// How the Working list scrolls to show a confirming heading whole
/// ([`GitTurtle::reveal_confirming_heading`]). The list places the heading
/// when it is next laid out, with the viewport it is drawn in then, which
/// the frames around a write change from the one last drawn: the busy
/// composer and the controls above the list are measured again, and a
/// 1000 × 1400 test window's list went from 681 to 660 px as Stage all's
/// rows arrived.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Reveal {
    /// The least move that shows it, which keeps a selected file the list
    /// shows now in view.
    Nearest,
    /// The edge of the viewport on the selected file's side, `Bottom` below
    /// it or `Top` above it, for a selected file the list does not show
    /// now and would centre.
    Edge(ScrollStrategy),
}

/// How a list of `rows` rows `row` tall, last drawn `viewport` tall and
/// scrolled to `top`, shows row `heading`: where no file is selected, by
/// the least move; with the `selected` row, only where the two fit in the
/// viewport together with half a row to spare for the viewport's next
/// height, by the least move where the list shows that row now and
/// otherwise with the heading at the edge beside it. `None` leaves the
/// selected row where the refresh puts it.
fn heading_reveal(
    top: Pixels,
    viewport: Pixels,
    row: Pixels,
    rows: usize,
    selected: Option<usize>,
    heading: usize,
) -> Option<Reveal> {
    let Some(selected) = selected else {
        return Some(Reveal::Nearest);
    };
    let span = row * (heading.abs_diff(selected) + 1) as f32;
    if span > viewport - row / 2. {
        return None;
    }
    let top = top.clamp(
        Pixels::ZERO,
        (row * rows as f32 - viewport).max(Pixels::ZERO),
    );
    let start = row * selected as f32;
    // Within a hundredth of a pixel, as layout rounds.
    if start >= top - px(0.01) && start + row <= top + viewport + px(0.01) {
        Some(Reveal::Nearest)
    } else if heading > selected {
        Some(Reveal::Edge(ScrollStrategy::Bottom))
    } else {
        Some(Reveal::Edge(ScrollStrategy::Top))
    }
}

/// The tight form's one row: the filter's field beside 28 px controls, with
/// a `py_1` above and below that holds a focused control's ring.
fn working_toolbar_height(window: &Window) -> f32 {
    let rem = window.rem_size();
    f32::from(rem * 0.25 * 2. + (rem * 2.).max(crate::appearance::ui_size(28.)))
}

/// The title counter's whole text, for a title `length` characters long.
fn title_counter(length: usize) -> String {
    format!("{length}/72 suggested")
}

/// How many characters of `text` lie outside `marked`, the UTF-16 range an
/// input method is composing in it, if any: the text as committed.
fn committed_length(text: &str, marked: Option<std::ops::Range<usize>>) -> usize {
    let Some(marked) = marked else {
        return text.chars().count();
    };
    let mut at = 0;
    text.chars()
        .filter(|character| {
            let start = at;
            at += character.len_utf16();
            !marked.contains(&start)
        })
        .count()
}

/// `full`, or where it is wider than `room` at `size`, the most of it that
/// fits with an ellipsis, cut at a grapheme-cluster boundary after any spaces
/// and ellipsis it ends with; empty where not even its first cluster does, so
/// no lone ellipsis shows.
fn ellipsized(full: &str, room: Pixels, size: Pixels, window: &Window) -> SharedString {
    let weight = window.text_style().font_weight;
    cut_to_fit(full, |text| {
        crate::views::shaped_width(text, size, weight, window) <= room
    })
}

/// [`ellipsized`]'s cut, with `fits` saying whether a text fits its room: a
/// longer text never fits where a shorter one does not, so a binary search
/// of the grapheme-cluster boundaries finds the longest cut that fits.
fn cut_to_fit(full: &str, fits: impl Fn(&str) -> bool) -> SharedString {
    use unicode_segmentation::UnicodeSegmentation as _;
    if fits(full) {
        return SharedString::from(full.to_owned());
    }
    let stem = crate::text_cut::trim_end_clusters(full, |c| c == '…');
    // The byte ends of `stem`'s first one, two, … grapheme clusters, short
    // of all, so a cut keeps a letter's marks, an emoji sequence or a flag.
    let ends: Vec<usize> = stem
        .grapheme_indices(true)
        .skip(1)
        .map(|(end, _)| end)
        .collect();
    let cut = |clusters: usize| {
        let kept = crate::text_cut::trim_end_clusters(&stem[..ends[clusters]], char::is_whitespace);
        format!("{kept}…")
    };
    let (mut fitting, mut low, mut high) = (SharedString::default(), 0, ends.len());
    while low < high {
        let middle = (low + high) / 2;
        let text = cut(middle);
        if fits(&text) {
            fitting = text.into();
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    fitting
}

/// The first of `forms` that fits `room` whole at `size`, each a shorter
/// wording of the one before it in whole words; where none does, the last
/// as [`ellipsized`] fits it, so a word is cut only where even the shortest
/// form is too wide.
fn whole_form(forms: &[&str], room: Pixels, size: Pixels, window: &Window) -> SharedString {
    let weight = window.text_style().font_weight;
    let (shortest, wider) = forms.split_last().expect("a placeholder");
    wider
        .iter()
        .find(|form| crate::views::shaped_width(form, size, weight, window) <= room)
        .map_or_else(
            || ellipsized(shortest, room, size, window),
            |form| SharedString::from(form.to_string()),
        )
}

/// Fits `field`'s placeholder to the width the field lays its text out at:
/// the first of `forms`, its whole-word wordings from the longest, that
/// fits, else the shortest ending with an ellipsis rather than clipping a
/// glyph (`whole_form`). The field chooses it as it is prepainted, in the
/// frame that lays it out at that width
/// (`InputBaseState::set_placeholder_fit`), so a new width draws the
/// placeholder it takes in its first frame. Chosen after the field had been
/// painted, it was a frame late, and the refresh that drew it asked for a
/// second frame at every width that changed it: History's search field and
/// the changed files' filter between a 461 px half tile and 980 px at 18 pt
/// drew each such step twice. What this returns hands the field its choice
/// as it is laid out, before the field is prepainted; its parent is
/// positioned and it takes no room. The placeholder is no name: the field
/// keeps its whole accessible name (`aria_label`) whichever form shows, and
/// reports the first, whole form as its accessible placeholder at every
/// width (`InputPresentation::accessible_placeholder`). The field's
/// accessible metadata is given as it is built, before the frame lays it out:
/// following the drawn form, it kept the earlier width's form after a step
/// until something else redrew. `text` is the size the field draws its text at, at the default interface
/// size: 12 pt for most fields, 11 pt for the navigation's filter.
pub(super) fn placeholder_fit<M: gpui_kit::base::input::InputModeKind + 'static>(
    field: Entity<gpui_kit::base::input::InputBaseState<M>>,
    cache: &'static std::thread::LocalKey<PlaceholderFit>,
    forms: &'static [&'static str],
    text: f32,
) -> impl IntoElement {
    FittedPlaceholder {
        field,
        cache,
        forms,
        text,
    }
}

/// What [`placeholder_fit`] returns: as it is laid out, it hands its field
/// the choice the field makes for its width as it is prepainted.
#[derive(IntoElement)]
struct FittedPlaceholder<M: gpui_kit::base::input::InputModeKind + 'static> {
    field: Entity<gpui_kit::base::input::InputBaseState<M>>,
    cache: &'static std::thread::LocalKey<PlaceholderFit>,
    forms: &'static [&'static str],
    text: f32,
}

impl<M: gpui_kit::base::input::InputModeKind + 'static> RenderOnce for FittedPlaceholder<M> {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            field,
            cache,
            forms,
            text,
        } = self;
        field.update(cx, |state, _| {
            state.set_placeholder_fit(Some(gpui_kit::base::input::PlaceholderFit {
                whole: forms.first().copied().unwrap_or_default().into(),
                fit: std::rc::Rc::new(move |room, window| {
                    fitted_placeholder(cache, forms, room, text, window)
                }),
            }));
        });
        div().absolute().size_0()
    }
}

/// `forms` at `text` pt as they fit `room` (`whole_form`), laid out again
/// only when the room, the font or the sizes change.
fn fitted_placeholder(
    cache: &'static std::thread::LocalKey<PlaceholderFit>,
    forms: &[&str],
    room: Pixels,
    text: f32,
    window: &Window,
) -> SharedString {
    let key = crate::views::LineKey::current(window);
    cache.with(|cache| {
        let mut cache = cache.borrow_mut();
        match &*cache {
            Some((cached, width, text)) if *cached == key && *width == room => text.clone(),
            _ => {
                let fit = whole_form(forms, room, crate::appearance::ui_text(text), window);
                *cache = Some((key, room, fit.clone()));
                fit
            }
        }
    })
}

fn commit_composer_height(available: f32, row_height: f32, preferred: f32) -> f32 {
    let available = available.max(0.);
    // Reserve several visible rows at ordinary sizes and share very short
    // bodies equally. The scrolling composer must never consume the list.
    let list_reserve = (row_height * 3.5)
        .max(available * 0.30)
        .min(available * 0.50);
    // The cap scales with the interface size, as the composer's rows do.
    preferred
        .min(f32::from(crate::appearance::ui_size(440.)))
        .min(available - list_reserve)
}

impl GitTurtle {
    pub(super) fn current_commit_draft(&self, cx: &App) -> CommitDraft {
        CommitDraft {
            title: self.commit_title.read(cx).value().to_string(),
            description: self.commit_message.read(cx).value().to_string(),
        }
    }

    pub(super) fn persist_commit_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(worktree) = self.draft_repository.clone() else {
            return;
        };
        let draft = self.current_commit_draft(cx);
        self.save_commit_draft(worktree, draft, window, cx);
        cx.notify();
    }

    fn save_commit_draft(
        &mut self,
        worktree: PathBuf,
        draft: CommitDraft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if draft.is_empty() {
            self.commit_drafts.remove(&worktree);
        } else {
            self.commit_drafts.insert(worktree.clone(), draft.clone());
        }
        let Some(response) =
            self.draft_saver
                .queue(&self.preferences_writer, worktree.clone(), draft)
        else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, window, cx| {
                // A save that ends as the last did draws nothing new: each
                // tab switch saves the draft it leaves, and its end drew the
                // tab shown again once more.
                let error = match result {
                    Ok(()) => None,
                    Err(commit_drafts::DraftSaveFailure::Busy) => {
                        this.ask_again_to_save_commit_draft(worktree, window, cx);
                        return;
                    }
                    Err(commit_drafts::DraftSaveFailure::Failed(error)) => Some(error),
                };
                if this.draft_save_error != error {
                    this.draft_save_error = error;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Asks again, after the wait, for the drafts a full queue refused: the
    /// person typed them but asked for no save, so a writer that is merely
    /// behind is not a failure. Asking for `worktree`'s newest draft schedules
    /// every other draft still waiting.
    fn ask_again_to_save_commit_draft(
        &mut self,
        worktree: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.draft_saver.retry.is_some() {
            return;
        }
        let retry = self.retry_refused_save(window, cx, move |this, window, cx| {
            this.draft_saver.retry = None;
            if !this.draft_saver.is_pending() {
                return;
            }
            let draft = this
                .commit_drafts
                .get(&worktree)
                .cloned()
                .unwrap_or_default();
            this.save_commit_draft(worktree, draft, window, cx);
        });
        self.draft_saver.retry = Some(retry);
    }

    pub(super) fn restore_commit_draft(
        &mut self,
        worktree: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.draft_repository.as_ref() == Some(&worktree) {
            return;
        }
        self.persist_commit_draft(window, cx);
        let draft = self
            .commit_drafts
            .get(&worktree)
            .cloned()
            .unwrap_or_default();
        self.draft_repository = Some(worktree);
        self.commit_title
            .update(cx, |input, cx| input.set_value(draft.title, window, cx));
        self.commit_message.update(cx, |input, cx| {
            input.set_value(draft.description, window, cx)
        });
    }

    fn clear_committed_draft(
        &mut self,
        worktree: &PathBuf,
        submitted_message: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.draft_repository.as_ref() == Some(worktree) {
            if self.current_commit_draft(cx).message() != submitted_message {
                return;
            }
            self.commit_title
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.commit_message
                .update(cx, |input, cx| input.set_value("", window, cx));
        } else if self
            .commit_drafts
            .get(worktree)
            .is_none_or(|draft| draft.message() != submitted_message)
        {
            return;
        }
        self.save_commit_draft(worktree.clone(), CommitDraft::default(), window, cx);
    }

    pub(super) fn project_event(
        &mut self,
        event: &projects::ProjectEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Naming a project changes app data only, so it stays available while
        // a Git operation runs on some repository.
        if let projects::ProjectEvent::Rename(path) = event {
            self.open_rename_project(path.clone(), window, cx);
            return;
        }
        // Choosing a folder starts nothing, so a running write leaves it
        // available as it leaves naming a project.
        if matches!(event, projects::ProjectEvent::ChooseFolder) {
            self.choose_project_folder(window, cx);
            return;
        }
        if self.operation_busy.is_some() {
            return;
        }
        match event {
            projects::ProjectEvent::ChooseFolder | projects::ProjectEvent::Rename(_) => {}
            projects::ProjectEvent::Open(path) => {
                // A marked tab's row reads its folder afresh, as Open and the
                // Local workspaces row do: through `open` it was a switch or
                // a return to the tab, which never clears the mark.
                if self.repository_tabs.is_marked(path) {
                    self.open_repository_tab(path.clone(), window, cx);
                } else if self
                    .repository
                    .as_ref()
                    .is_some_and(|repo| repo.path() == path)
                {
                    self.return_from_page(window, cx);
                } else {
                    self.limit = 500;
                    self.open(path.clone(), None, window, cx);
                }
            }
            projects::ProjectEvent::Back => self.return_from_page(window, cx),
            projects::ProjectEvent::Clone {
                source,
                destination,
            } => {
                let source = source.clone();
                let destination = destination.clone();
                self.create_project(
                    "Cloning repository…",
                    destination.clone(),
                    move || GitRepository::clone_repository(&source, &destination),
                    window,
                    cx,
                );
            }
            projects::ProjectEvent::Create {
                destination,
                branch,
            } => {
                let destination = destination.clone();
                let branch = branch.clone();
                self.create_project(
                    "Creating repository…",
                    destination.clone(),
                    move || GitRepository::init(&destination, &branch),
                    window,
                    cx,
                );
            }
        }
    }

    fn create_project(
        &mut self,
        label: &'static str,
        destination: PathBuf,
        operation: impl FnOnce() -> anyhow::Result<GitRepository> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let accepted_path = self.path.clone();
        let accepted_page = self.page;
        self.operation_repository = Some(destination.clone());
        self.operation_busy = Some(label);
        self.operation_error = None;
        self.operation_notice = None;
        self.hub.update(cx, |hub, cx| hub.set_busy(true, cx));
        let activity_id =
            self.activity
                .begin(&destination, label, &destination.display().to_string());
        self.save_activity(window, cx);
        let control = self.begin_operation_control(window, cx);
        let activity_control = control.clone();
        let response = self.operations.submit_controlled(control, operation);
        self.operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| Err(anyhow::anyhow!("Repository operation stopped without a result. Inspect the destination before retrying.")));
            let _ = this.update_in(cx, |this, window, cx| {
                let explanation = result.as_ref().map(|_| "Repository operation completed.".into()).unwrap_or_else(|error| format!("{error:#}"));
                this.activity.finish(activity_id, result.is_ok(), activity_control.is_cancelled(), explanation.contains("without a result") || explanation.contains("timed out"), &explanation);
                this.save_activity(window, cx);
                this.operation_busy = None;
                this.operation_repository = None;
                this.finish_operation_control(window, cx);
                // The repository opened below reads afresh: `open` drops a
                // working read noted meanwhile, and would cancel at once a
                // quiet refresh started here.
                let opening = result.is_ok() && this.path == accepted_path && this.page == accepted_page;
                if !opening {
                    this.read_deferred_working(window, cx);
                }
                this.hub.update(cx, |hub, cx| hub.set_busy(false, cx));
                match result {
                    Ok(repo) => {
                        let done = if label.starts_with("Cloning") { "Repository cloned" } else { "Repository created" };
                        let name = this.project_name(repo.path());
                        if opening {
                            this.limit = 500; this.open(repo.path().to_owned(), None, window, cx);
                            this.operation_notice = Some(format!("{done} · {name}"));
                        } else {
                            this.register_completed_repository(repo.path().to_owned(), done, &name, window, cx);
                        }
                    }
                    Err(error) => {
                        let message = format!("{error:#}");
                        this.hub.update(cx, |hub, cx| hub.set_error(Some(message.clone()), cx));
                        if this.path == accepted_path && this.page == accepted_page {
                            this.operation_error = Some(views::OperationFailure::of(&error));
                            this.operation_notice = None;
                        } else {
                            // One part: the path and Git's message are the user's and can hold " · ".
                            this.set_status_parts(&[&format!("Repository operation failed in {}: {message}", destination.display())]);
                        }
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn show_projects(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.end_image_drag();
        // Cancels every review being prepared, which are reads, ended
        // rather than waited for, and runs the read Changes asked for
        // meanwhile, which their ends would have run.
        self.end_preparations(page_navigation::PageSwitch::Away, window, cx);
        if self.operation_busy.is_some() {
            return;
        }
        self.capture_page_return_focus(window, cx);
        self.page = AppPage::Projects;
        self.page_origin = AppPage::Repository;
        self.hub.update(cx, |hub, cx| {
            hub.set_busy(false, cx);
            hub.set_error(None, cx);
            hub.set_can_go_back(self.repository.is_some(), cx);
        });
        // Focus opens on the selected action tab, with its ring, inside the
        // page so the application's keys still reach the window's handlers.
        self.hub
            .update(cx, |hub, cx| hub.focus_selected_mode(window, cx));
        self.repaint_page(window, cx);
    }

    pub(super) fn remember_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let recent = self.begin_recent_project_save(&path);
        self.queue_recent_project_save(path, recent, window, cx);
    }

    /// Runs `retry` once [`REFUSED_SAVE_RETRY`] has passed, for a save the
    /// person did not ask for that the preference writer's full queue
    /// refused. The returned task ends the wait when it drops.
    pub(super) fn retry_refused_save(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        retry: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Task<()> {
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(REFUSED_SAVE_RETRY).await;
            let _ = this.update_in(cx, retry);
        })
    }

    /// The UI generation rejects late replies, while the worker cancellation
    /// also bounds obsolete computation when no replacement preview is needed.
    pub(super) fn invalidate_read(&mut self) {
        self.pause_history_search_for_read();
        self.cancel_automatic_read();
        self.generation = self.generation.wrapping_add(1);
        self.worker.cancel();
        self.task = None;
        self.loading = None;
    }

    /// Reads the shown repository's working state on the operation
    /// executor, in place of any status read in flight. While a write or a
    /// review's preparation runs there, in this repository or another tab's,
    /// a clone or a create included, the read waits for it to end instead
    /// ([`Self::read_deferred_working`]): queued behind it, each read only
    /// waited, each opening of Changes queued another, and once the
    /// executor's bounded queue was full the next was refused as busy, which
    /// took the shown rows away until something read again. One that
    /// replaces the read a write asked for as it ended
    /// (`status_after_write`), as Refresh or showing Working Changes then
    /// does, reads for that write in its place, so menus keep waiting for it
    /// (`write_unsettled`): replaced unmarked, the write's read was no longer
    /// counted, and a Working row's open menu was built again from the
    /// status the write had just changed before the new read landed.
    ///
    /// Behind a read the window no longer waits for, as Settings and
    /// Projects end the wait for a review's preparation whose read never
    /// ends, the read asked for waits in the executor's queue, and one asked
    /// for again while it waits, by whichever route, takes it over rather
    /// than queue another
    /// ([`operations::SerialExecutor::submit_read_once`]), so each
    /// repository keeps one slot at most: Projects, Back and Refresh pressed
    /// again and again during such a read once queued a read at each
    /// Refresh, until one was refused as busy and Changes lost its rows.
    pub(super) fn refresh_worktree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation_busy.is_some() {
            self.working_read_deferred = self.path.clone();
            return;
        }
        self.working_read_deferred = None;
        let Some(repo) = self.repository.clone() else {
            return;
        };
        self.cancel_automatic_read();
        self.ensure_local_watcher(true, window, cx);
        if self.mode == WorkspaceMode::Working {
            self.invalidate_read();
            self.clear_preview();
            self.files.clear();
            self.file_paths.reset();
            self.selected_file = None;
        }
        let for_write =
            self.status_task.is_some() && self.status_after_write == Some(self.work_generation);
        self.work_generation = self.work_generation.wrapping_add(1);
        let generation = self.work_generation;
        if for_write {
            self.status_after_write = Some(generation);
        }
        let path = repo.path().to_owned();
        self.status_task = None;
        let requested = self.request_status_read();
        // Marked already, a read that fails says what an Open would say
        // now, so the mark can state the current reason (`tab_mark_restated`).
        let marked = self
            .shown_tab_unavailable()
            .then(|| {
                self.repository_tabs
                    .reopen_root(&path, self.scope.as_ref().map(|scope| &scope.1))
            })
            .flatten();
        let response =
            self.operations
                .submit_read_once(&mut self.status_reads, path.clone(), move || {
                    worker::WorkingState::read(&repo).map_err(|error| match &marked {
                        Some((root, git_dir)) => worker::with_current_mark_reason(
                            error,
                            repo.path(),
                            root,
                            git_dir.as_ref(),
                        ),
                        None => error,
                    })
                });
        self.status_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Working-copy refresh ended without a result"
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                if this.work_generation != generation || this.path.as_ref() != Some(&path) {
                    return;
                }
                this.status_task = None;
                match result {
                    Ok(state) => {
                        let from_write = this.status_after_write == Some(generation);
                        this.apply_worktree_state(state, false, from_write, requested, window, cx);
                    }
                    Err(error) => {
                        this.work_status = None;
                        this.working_paths.reset();
                        this.integration_state = None;
                        this.clear_working_rows(window, cx);
                        this.follow_working_menu(window, cx);
                        this.working_travel = Default::default();
                        this.working_selected = None;
                        if this.mode == WorkspaceMode::Working {
                            this.invalidate_read();
                            this.clear_preview();
                            this.files.clear();
                            this.file_paths.reset();
                            this.selected_file = None;
                        }
                        this.working_read_failed(error, window, cx);
                    }
                }
                this.try_automatic_refresh(window, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// What a failed working read says. Changes' Refresh is a passive reopen
    /// too: its repository gone from its root, the tab is read as History's
    /// Refresh reads it, whose failed Open marks it and says why, never as
    /// the work tree enclosing it. A tab marked already says why in its mark
    /// and the preview, and the read adds nothing beside it, as a quiet read
    /// adds nothing: with no repository found there, those say the reasons
    /// any folder that is not one gets, and this once said "is gone" beside
    /// them. Where the read found a reason of its own, the mark takes it in
    /// those same words, and where the mark has gone meanwhile the read's
    /// own failure says what it did.
    pub(super) fn working_read_failed(
        &mut self,
        error: anyhow::Error,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let error = match error.downcast::<worker::MarkedReadFailed>() {
            Ok(failed) => {
                if self.tab_mark_restated(failed.reason, cx) {
                    return;
                }
                failed.read
            }
            Err(error) => error,
        };
        if error.is::<worker::RepositoryGone>() {
            self.tab_repository_gone(window, cx);
        } else {
            self.operation_error
                .get_or_insert_with(|| views::OperationFailure::of_read("Working tree", &error));
        }
    }

    /// Numbers a working-state read as it is asked for, the explicit read
    /// (`refresh_worktree`) and the quiet one (`try_automatic_refresh`)
    /// alike: its reply carries the number to [`Self::apply_worktree_state`],
    /// so a read asked for before a recovery review was refused, which may
    /// have read the working state before what the core refused for came,
    /// cannot say it has gone ([`Self::conflicts_ended`]).
    pub(super) fn request_status_read(&mut self) -> u64 {
        self.status_requests += 1;
        self.status_requests
    }

    /// Applies a working-state read asked for as the `requested`th
    /// ([`Self::request_status_read`]), and returns whether its entries
    /// differ from those shown. `from_write` says a write asked for it as
    /// it ended, rather than Refresh or the local watcher.
    pub(super) fn apply_worktree_state(
        &mut self,
        state: worker::WorkingState,
        quiet: bool,
        from_write: bool,
        requested: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let worker::WorkingState {
            status,
            profile,
            remotes,
            operation,
        } = state;
        let path = self.path.clone();
        // A paused integration read again as ended: a recovery refusal that
        // read the pause no longer holds.
        let ended = self.integration_state.is_some() && matches!(operation, Ok(None));
        self.integration_state = match operation {
            Ok(state) => state,
            Err(error) => {
                self.operation_error.get_or_insert_with(|| {
                    views::OperationFailure::of_read("Operation state", &error)
                });
                None
            }
        };
        if ended {
            self.integration_ended(window, cx);
        }
        // A refusal for conflicts alone, which a pause's end keeps since it
        // resolves none of them (`integration_ended`), goes in the frame a
        // read finds none left, the window's and the recovery forms' alike
        // (`conflicts_ended`): it once stayed in the window's error until
        // dismissed, saying conflicts remained after they were resolved. A
        // read asked for before the refusal leaves it.
        if !status.entries.iter().any(|entry| entry.conflicted) {
            self.conflicts_ended(requested, window, cx);
        }
        let preferred = self.working_selected.and_then(|(index, area)| {
            self.work_status
                .as_ref()?
                .entries
                .get(index)
                .map(|entry| (entry.path.clone(), area))
        });
        if !quiet {
            self.conflict_drafts.retain(|(repository, file), _| {
                Some(repository) != path.as_ref()
                    || status
                        .entries
                        .iter()
                        .any(|entry| entry.conflicted && &entry.path == file)
            });
        }
        let entries_changed = self
            .work_status
            .as_ref()
            .is_none_or(|previous| previous.entries != status.entries);
        // The rows being read hold their place through a read from outside
        // the app and a write's quiet one. A write's own read centres its
        // row as it lands instead.
        let working_anchor = if entries_changed && (quiet || !from_write) {
            self.working_scroll_anchor(window)
        } else {
            None
        };
        if entries_changed {
            // The shown rows index the earlier status; they stay on screen,
            // with the cursor among them, until the new status's land.
            let shown = self
                .work_status
                .clone()
                .filter(|_| !self.working_rows.is_empty())
                .map(|status| (status, self.working_selected));
            self.working_paths.supersede(shown);
            self.working_selected = preferred.as_ref().and_then(|(path, area)| {
                status.entries.iter().enumerate().find_map(|(i, entry)| {
                    if !crate::working_selection::same_path(&entry.path, path) {
                        return None;
                    }
                    retained_area(
                        *area,
                        entry.staged.is_some(),
                        entry.unstaged.is_some() || entry.untracked || entry.conflicted,
                    )
                    .map(|area| (i, area))
                })
            });
        }
        let remotes = match remotes {
            Ok(remotes) => remotes,
            Err(error) => {
                self.operation_error.get_or_insert_with(|| {
                    views::OperationFailure::of_read("Remote configuration", &error)
                });
                Vec::new()
            }
        };
        let branch_changed = self.work_status.as_ref().is_none_or(|previous| {
            previous.branch != status.branch || previous.upstream != status.upstream
        });
        let (default_remote, default_branch) = remote_defaults(&status, &remotes);
        // A field given the value it holds only draws the window again, as
        // `set_value` notifies: a repository without a remote gave its empty
        // remote field an empty value on every quiet read, and a tab shown
        // again drew once more as its rescan landed.
        let defaults = |input: &Entity<InputState>, default: &str, cx: &App| {
            let value = input.read(cx).value();
            (branch_changed || value.is_empty()) && value.as_ref() != default
        };
        if defaults(&self.remote_name, &default_remote, cx) {
            self.remote_name
                .update(cx, |input, cx| input.set_value(default_remote, window, cx));
        }
        if defaults(&self.remote_branch, &default_branch, cx) {
            self.remote_branch
                .update(cx, |input, cx| input.set_value(default_branch, window, cx));
        }
        self.profile = match profile {
            Ok(profile) => Some(profile),
            Err(error) => {
                self.operation_error.get_or_insert_with(|| {
                    views::OperationFailure::of_read("Git identity", &error)
                });
                None
            }
        };
        self.work_status = Some(Arc::new(status));
        // A quiet refresh that changed nothing leaves the line to whatever it
        // last reported, an operation's outcome included.
        if self.mode == WorkspaceMode::Working && (entries_changed || !quiet) {
            self.status = self.working_summary();
        }
        if entries_changed {
            self.schedule_working_filter(
                working_anchor,
                !quiet,
                working_follow(quiet, from_write),
                crate::working_selection::Preparation::Status,
                window,
                cx,
            );
        } else if !self.working_paths.pending {
            // No new rows: a departed row whose write has finished did not
            // move. Rows still being prepared for an earlier status, as a
            // quiet refresh's after the write's own can find, may yet land
            // it, and they settle the departure when they do.
            self.land_working_row(None, cx);
        }
        self.remotes = remotes;
        if !quiet && self.mode == WorkspaceMode::Working {
            self.invalidate_read();
            if let Some((index, area)) = self.working_selected {
                self.select_working(index, area, window, cx);
            } else {
                self.clear_preview();
                self.files.clear();
                self.file_paths.reset();
                self.selected_file = None;
            }
        }
        entries_changed
    }

    /// Opens Working Changes. Switching pages only reads, so it stays
    /// available while a write runs, as History does: the page then shows
    /// the write's own busy look on its write controls, and its read waits
    /// for the write to end ([`Self::refresh_worktree`]). The switch once
    /// returned while any write ran, from the toggle drawn and announced
    /// available, its shortcut and the palette alike.
    ///
    /// Leaving History or Compare ends the reviews being prepared there, as
    /// every page switch does ([`Self::end_preparations`]): nothing they
    /// bring opens over the working list, while their read still ends, the
    /// window busy until then, and then reads the working state this page
    /// asked for ([`Self::dismiss_history_plans`]). Their forms and dialogs
    /// once opened over the working list.
    pub(super) fn show_working(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_inspections(window, cx);
        self.forget_page_return();
        if self.repository.is_none() {
            self.show_projects(window, cx);
            return;
        }
        if self.mode != WorkspaceMode::Working {
            self.end_preparations(page_navigation::PageSwitch::ToChanges, window, cx);
            if self.mode == WorkspaceMode::History {
                self.history_sidebar = self.sidebar;
            }
            self.retained_history_files =
                Some((std::mem::take(&mut self.files), self.selected_file.take()));
            self.invalidate_read();
            self.clear_preview();
        }
        self.mode = WorkspaceMode::Working;
        self.status = self.working_summary();
        self.page = AppPage::Repository;
        self.sidebar = false;
        self.pane = Pane::Files;
        window.focus(&self.file_focus, cx);
        self.refresh_worktree(window, cx);
        cx.notify();
    }

    /// Drops the plans a recovery, rebase or branch action brings when
    /// its preparation, started in History or Compare, ends, as Changes
    /// opens over the view that asked for them ([`Self::show_working`]), or,
    /// `dismissed` false as History shows again before the preparation ends
    /// (`back_to_history`), wants them again, so the plan opens there. Kept
    /// dropped, History's action did nothing. Each preparation's start wants
    /// its plan again too (`cancel_recovery_read`,
    /// `cancel_interactive_rebase_action`, `cancel_branch_action`), so one
    /// Changes asks for opens there. Part of every page switch's set
    /// ([`Self::end_preparations`]).
    pub(super) fn dismiss_history_plans(&mut self, dismissed: bool) {
        self.dismiss_recovery_read(dismissed);
        self.dismiss_interactive_rebase_action(dismissed);
        self.dismiss_branch_action(dismissed);
    }

    /// Reads the shown repository's working state if it asked for a read
    /// while a write or a review's preparation ran
    /// ([`Self::refresh_worktree`]) and none runs now. Every write, clone,
    /// create and preparation calls it as it ends, whichever repository it
    /// ran in: Changes may have been opened in another tab meanwhile, whose
    /// rows the write's own reads never touch. A clone or create that opens
    /// its repository leaves both to `open`, which drops the read, and whose
    /// snapshot reads afresh. Whatever cancels a preparation, whose end then
    /// never runs, reads it too: a form's Cancel, and Settings and Projects
    /// through [`Self::read_noted_working`] ([`Self::end_preparations`]),
    /// while a switch between History and Changes waits for the cancelled
    /// read to end and reads then. A tab switch or close drops the
    /// read instead (`retain_active_tab`, `close_repository_tab`), the tab
    /// reading quietly when it is shown again, and so does `open`, as
    /// History's Refresh calls it, whose snapshot reads the working state
    /// itself. A read another tab asked for is dropped too, as the shown one
    /// did not ask. A cancel once left the read noted, so Refresh never ran,
    /// or ran unasked at the end of a later preparation.
    ///
    /// Then the quiet refresh the running write or preparation refused
    /// starts ([`Self::try_automatic_refresh`]), or, behind a read just
    /// asked for here, starts as that read lands. A change reported during
    /// a review's preparation, or a tab restored during a Fetch in another
    /// with no watch registered behind it, once kept the history and rows
    /// last read after the end, until something else read them.
    pub(super) fn read_deferred_working(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.read_noted_working(window, cx);
        self.try_automatic_refresh(window, cx);
    }

    /// The working read of [`Self::read_deferred_working`] without its
    /// quiet refresh, for Settings and Projects as they cancel a
    /// preparation ([`Self::end_preparations`]): off the repository page a
    /// quiet refresh is refused, and the return there starts it
    /// (`return_from_page`).
    pub(super) fn read_noted_working(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation_busy.is_none() && self.working_read_deferred.is_some() {
            if self.working_read_deferred == self.path {
                self.refresh_worktree(window, cx);
            } else {
                self.working_read_deferred = None;
            }
        }
    }

    /// Whether the shown repository's working state is being read, or is to
    /// be read once a running write or preparation ends: by a status read,
    /// or by a quiet read, as Commit and Fetch refresh as they end
    /// (`read_after_write`), but for one an open inspection holds back
    /// ([`Self::quiet_read_reads_working`]). Changes once said "Changes
    /// unavailable" while such a read ran with no status shown, as if
    /// nothing would read it.
    pub(super) fn working_read_pending(&self) -> bool {
        self.status_task.is_some()
            || self.quiet_read_reads_working()
            || (self.working_read_deferred.is_some() && self.working_read_deferred == self.path)
    }

    /// Records `notice`, the outcome Git reported for a write in
    /// `repository`, in the status line, which keeps it for
    /// [`copy_feedback::RESTORE_AFTER`], the time the control that asked
    /// reads its confirmation, over what the reads after the write report:
    /// the refresh that follows it once wrote Changes' working summary over
    /// "Staged notes.txt" before the row's Staged had gone. Then the line
    /// reads what they reported. A newer outcome replaces it and restarts the
    /// time, and a write submitted ends it.
    pub(super) fn record_outcome(
        &mut self,
        repository: &std::path::Path,
        notice: String,
        cx: &mut Context<Self>,
    ) {
        self.status = notice.clone();
        let expiry = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(copy_feedback::RESTORE_AFTER)
                .await;
            let _ = this.update(cx, |this, cx| {
                this.status_outcome = None;
                cx.notify();
            });
        });
        self.status_outcome = Some(StatusOutcome {
            repository: repository.to_owned(),
            text: notice,
            _expiry: expiry,
        });
    }

    /// The status bar's line on the Repository page: a read's error, else a
    /// write's outcome while it lasts and its repository is shown
    /// ([`Self::record_outcome`]), else the label of a read in progress,
    /// else the status.
    pub(super) fn status_line(&self) -> &str {
        let outcome = self
            .status_outcome
            .as_ref()
            .filter(|outcome| self.path.as_ref() == Some(&outcome.repository))
            .map(|outcome| outcome.text.as_str());
        self.error
            .as_deref()
            .or(outcome)
            .or(self.loading)
            .unwrap_or(&self.status)
    }

    /// The write's outcome the status line reads while it is fresh, if it
    /// reads one ([`Self::status_line`]): the keyboard hints beside it give
    /// it their room (`GitTurtle::fit_status_bar`).
    pub(super) fn fresh_outcome(&self) -> Option<&str> {
        let outcome = self.status_outcome.as_ref()?;
        (self.page == AppPage::Repository
            && self.error.is_none()
            && self.path.as_ref() == Some(&outcome.repository))
        .then_some(outcome.text.as_str())
    }

    /// The status bar's account of the working tree. Changes sets it when it
    /// is shown, refreshed or previewed, because History's figures describe a
    /// commit's files and a working preview holds only the previewed one.
    pub(super) fn working_summary(&self) -> String {
        let Some(status) = &self.work_status else {
            return "Reading working changes…".into();
        };
        let changes = status.entries.len();
        let staged = status
            .entries
            .iter()
            .filter(|entry| entry.staged.is_some())
            .count();
        let said = crate::counted(changes, "working change", "working changes");
        match (changes, staged) {
            (0, _) => "No working changes".into(),
            (_, 0) => said,
            _ => format!("{said} · {staged} staged"),
        }
    }

    /// Selects the working row of `index` in `area` and reads its preview.
    /// Selecting activates nothing: a conflicted file's view opens with
    /// focus left where it is, unless the person's choice of the row asks
    /// for its draft ([`Self::activate_working_preview`]).
    pub(super) fn select_working(
        &mut self,
        index: usize,
        area: ChangeArea,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_working_row(index, area, false, window, cx);
    }

    /// [`Self::select_working`] for a revision inspection's close, which
    /// restores Changes with this row selected and no preview: under an
    /// open dialog or sheet, the row's list becomes the modal's return
    /// target (`return_modal_to`), as the modal's captured opener belonged
    /// to the inspection that closed.
    pub(super) fn select_restored_working(
        &mut self,
        index: usize,
        area: ChangeArea,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_working_row(index, area, true, window, cx);
    }

    fn select_working_row(
        &mut self,
        index: usize,
        area: ChangeArea,
        restored: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() {
            return;
        }
        self.close_blame(window, cx);
        let (Some(repo), Some(status)) = (&self.repository, &self.work_status) else {
            return;
        };
        let Some(entry) = status.entries.get(index).cloned() else {
            return;
        };
        let available = if area == ChangeArea::Staged {
            entry.staged.is_some()
        } else {
            entry.unstaged.is_some() || entry.untracked || entry.conflicted
        };
        if !available {
            self.invalidate_read();
            self.working_selected = None;
            self.clear_preview();
            self.files.clear();
            self.file_paths.reset();
            self.selected_file = None;
            // The stopped read puts a running quiet read back to wait, and
            // with nothing of its own to read this starts it again: a
            // write's reload once waited for the next local change, every
            // menu that follows writes refused.
            self.try_automatic_refresh(window, cx);
            return;
        }
        let repo = repo.clone();
        self.mode = WorkspaceMode::Working;
        self.working_selected = Some((index, area));
        self.clear_preview();
        self.files.clear();
        self.file_paths.reset();
        self.selected_file = None;
        self.zoom = 0.;
        self.image_scroll.set_offset(point(px(0.), px(0.)));
        self.interaction_started = Some(Instant::now());
        // The list's keyboard menu, open on the row as a write's status read
        // selects it again, keeps focus and returns it here as it closes:
        // focused away, it closed as each write ended rather than being
        // built again ([`native_accessibility::KeyboardMenu`]). Open on
        // another row, as a read that finds the file staged moves the
        // selection to the Staged group, it closes: kept focused, it stayed
        // over a row the selection had left, its items for that row.
        let menus_row = self.file_menu_row.as_ref().is_some_and(|followed| {
            followed.target.area() == area
                && crate::working_selection::same_path(&followed.target.entry.path, &entry.path)
        });
        if !(menus_row
            && self
                .file_menu
                .as_ref()
                .is_some_and(|menu| menu.has_focus(window, cx)))
        {
            self.dismiss_file_menu(window, cx);
            // A status read landing while Settings or Projects shows selects
            // the row again for its preview, and leaves focus on that page:
            // it once moved to the hidden list, away from the control the
            // page opened on. Under a dialog or sheet focus stays in the
            // modal, which it once left for the list behind it: a read
            // landing there leaves the modal returning to the control it
            // was opened from, and only a revision inspection closed there,
            // whose restore replaced that control, returns to the list
            // (`select_restored_working`).
            if self.page == AppPage::Repository {
                if !(window.has_active_dialog(cx) || window.has_active_sheet(cx)) {
                    window.focus(&self.file_focus, cx);
                } else if restored {
                    self.return_modal_to(Some(self.file_focus.clone()));
                }
            }
        }
        self.request(
            Job::WorkingPreview {
                repo,
                entry,
                area,
                head: self
                    .work_status
                    .as_ref()
                    .and_then(|status| status.head.clone()),
            },
            "Reading working comparison…",
            window,
            cx,
        );
    }

    /// The person's explicit choice of the selected working file, a click on
    /// its row or Enter on it: its conflict's result draft takes focus, at
    /// once where its view is open, else as the preview read under way lands
    /// ([`GitTurtle::preview_activation`]). Selecting a row is not choosing
    /// it: an arrow key, a write's status read or a refresh selects it and
    /// leaves focus where it is (`select_working`). While the list refuses
    /// a choice ([`Self::working_list_refuses`]), neither a click on the
    /// selected row nor Enter chooses it: both once focused its conflict's
    /// draft, the click though the list had refused it. Returns whether
    /// anything was activated.
    pub(super) fn activate_working_preview(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.mode != WorkspaceMode::Working
            || self.working_selected.is_none()
            || self.working_list_refuses()
        {
            return false;
        }
        if self.activate_conflict_view(window, cx) {
            return true;
        }
        if self.task.is_some() {
            self.preview_activation = Some(self.generation);
            return true;
        }
        false
    }

    /// Whether focus is on a control inside Working Changes' list, such as
    /// Clear filter or a row's Stage, rather than on the list itself. Enter
    /// is then that control's, which acts on it as Space or a click does:
    /// the list's Enter (`NextPane`) once took the key from every control
    /// inside the list, so Enter on a focused Clear filter did nothing.
    pub(super) fn working_control_focused(&self, window: &Window, cx: &App) -> bool {
        !self.file_focus.is_focused(window) && self.file_focus.contains_focused(window, cx)
    }

    /// Whether a handle the Working list keeps for its rows' controls holds
    /// focus (`RowControls`), as no control of a row's travelling copy can.
    pub(super) fn row_control_focused(&self, window: &Window, cx: &App) -> bool {
        let layout = self.working_layout.take();
        let focused = layout
            .as_ref()
            .is_some_and(|layout| layout.read(cx).row_controls.hold_focus(window));
        self.working_layout.set(layout);
        focused
    }

    /// The row control holding focus, by its selector
    /// (`RowControls::focused_selector`).
    #[cfg(test)]
    pub(super) fn focused_row_control(&self, window: &Window, cx: &App) -> Option<String> {
        let layout = self.working_layout.take();
        let focused = layout
            .as_ref()
            .and_then(|layout| layout.read(cx).row_controls.focused_selector(window));
        self.working_layout.set(layout);
        focused
    }

    /// Whether the shown rows still list the row a row control stands in
    /// (`RowControls`): a heading's area, or a file's path in its area. The
    /// row at `drawn`, where the list last drew it, is asked first, and the
    /// other rows only where that one is another now, so a control holding
    /// focus costs a frame one row's check while its rows stay put.
    fn lists_row_control(&self, control: &RowControl, drawn: usize) -> bool {
        let status = self.shown_working_status();
        let stands_in = |row: &WorkingRow| match (control, row) {
            (RowControl::Heading(area), WorkingRow::Heading(at, _)) => at == area,
            (RowControl::File(path, area), WorkingRow::File(index, at)) => {
                at == area
                    && status
                        .and_then(|status| status.entries.get(*index))
                        .is_some_and(|entry| crate::working_selection::same_path(&entry.path, path))
            }
            _ => false,
        };
        self.working_rows.get(drawn).is_some_and(stands_in)
            || self.working_rows.iter().any(stands_in)
    }

    /// Gives focus to the list itself where a row control holds it whose row
    /// the shown rows no longer list, as the rows that took that row away
    /// land, as a row's Stage after Enter or Space sends its file to the
    /// other group, or as a failed status read or a failed preparation of
    /// the rows takes every row away (`clear_working_rows`): the frame that
    /// first draws the list without that row then draws the list's ring.
    /// Left to that frame's render (`RowControls::begin`), the hand-off
    /// waited for its deferred work, and the frame was drawn with focus on a
    /// handle no element drew, with no ring. Only Changes draws those
    /// controls and that list.
    pub(super) fn release_departed_row_control(&self, window: &mut Window, cx: &mut App) {
        if self.mode != WorkspaceMode::Working {
            return;
        }
        let Some(layout) = self.working_layout.take() else {
            return;
        };
        self.working_layout.set(Some(layout.clone()));
        let departed = layout.read(cx).row_controls.departed(
            |control, drawn| self.lists_row_control(control, drawn),
            window,
        );
        if departed {
            window.focus(&self.file_focus, cx);
        }
    }

    /// The handles the Working list keeps for its rows' controls, once the
    /// inspector has been drawn.
    fn working_row_controls(&self, cx: &App) -> Option<RowControls> {
        let layout = self.working_layout.take();
        let controls = layout
            .as_ref()
            .map(|layout| layout.read(cx).row_controls.clone());
        self.working_layout.set(layout);
        controls
    }

    /// Whether a section heading's Stage all or Unstage all, over `count`
    /// files, has nothing it could act on.
    fn working_heading_empty(&self, count: usize, cx: &App) -> bool {
        count == 0
            || self.working_paths.error.is_some()
            || !self.working_filter.read(cx).value().is_empty()
    }

    /// Whether a section heading's Stage all or Unstage all over `count`
    /// files looks unavailable: it then takes no focus.
    fn working_heading_unavailable(&self, count: usize, cx: &App) -> bool {
        ((self.operation_busy.is_some() || self.working_paths.pending) && self.busy_shown())
            || self.working_heading_empty(count, cx)
    }

    /// The Working list's Tab stops past the list itself, in Tab's order:
    /// the selected row's Stage or Unstage, then each group heading's Stage
    /// all or Unstage all in the list's order. The list is one stop, so no
    /// other row's control is among them. Left out too is a control that
    /// cannot take focus: a file row's where the rows' form draws none
    /// (`file_action`), one the busy look has dimmed, which the toolkit takes
    /// out of the Tab order, and one in the row landing from a flight, where
    /// the list draws an empty slot.
    fn working_row_stops(&self, cx: &App) -> Vec<RowStop> {
        let dimmed = self.operation_busy.is_some() && self.busy_shown();
        let landing = self.working_flight(cx).map(|flight| flight.position);
        let file_action = self
            .working_row_controls(cx)
            .is_some_and(|controls| controls.file_action());
        let selected = self
            .shown_working_cursor()
            .filter(|_| file_action && !dimmed)
            .and_then(|(index, area)| {
                let position = self.shown_cursor_row()?;
                let entry = self.shown_working_status()?.entries.get(index)?;
                Some(RowStop {
                    id: self.working_row_key(index, area),
                    row: RowControl::File(entry.path.clone(), area),
                    position,
                })
            });
        let headings =
            self.working_rows
                .iter()
                .enumerate()
                .filter_map(|(position, row)| match row {
                    WorkingRow::Heading(area, count)
                        if !self.working_heading_unavailable(*count, cx) =>
                    {
                        Some(RowStop {
                            id: heading_control(*area).into(),
                            row: RowControl::Heading(*area),
                            position,
                        })
                    }
                    _ => None,
                });
        selected
            .into_iter()
            .chain(headings)
            .filter(|stop| Some(stop.position) != landing)
            .collect()
    }

    /// Gives `stop`'s control focus, revealed whole with its ring room in
    /// the frame that first draws the focus. The row's control lies inside
    /// its row, so the least scroll that shows the row whole shows the ring
    /// (`ScrollStrategy::Nearest`); where the list and the composer scroll
    /// together, the list stands at the top of the body, so only the body's
    /// top shows it whole. The handle is made here where no frame has drawn
    /// the row, and the scroll asked for lands as the next frame lays the
    /// list out, in the frame that draws the focus, so focus never rests on
    /// a control the frame does not draw.
    fn focus_row_stop(&self, stop: &RowStop, window: &mut Window, cx: &mut App) {
        let Some(controls) = self.working_row_controls(cx) else {
            return;
        };
        let handle = controls.focus(stop.id.clone(), || stop.row.clone(), stop.position, cx);
        window.focus(&handle, cx);
        self.working_scroll
            .scroll_to_item(stop.position, ScrollStrategy::Nearest);
        self.working_body.scroll().set_offset(Point::default());
    }

    /// Tab (`forward`) or Shift+Tab on the Working page: moves focus through
    /// the list's stops (`working_row_stops`), and leaves any other press to
    /// the window's own order, where every row control is out of the Tab
    /// order. From the list Tab reaches the first stop, and from the last
    /// stop the stop after the list in the window's order, the composer's
    /// first or, with an operation in progress, whichever comes next outside
    /// the body; Shift+Tab walks back the same way, from that stop onto the
    /// last control and from the first control onto the list. A row's
    /// control still holding focus after the selection has left its row
    /// counts as standing in the list, so Tab goes to the first stop and
    /// Shift+Tab to the list; a press on one takes no focus, as no Button's
    /// does.
    pub(super) fn tab_working_row_stops(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The dialog layer is a child of the root this binding stands on, so
        // the action fires inside a dialog or a sheet ahead of the toolkit's
        // focus trap, and it lands a frame after the page or mode has
        // changed: both go on to the window's own order, as the root's
        // `NextPane` checks.
        if self.page != AppPage::Repository
            || self.mode != WorkspaceMode::Working
            || window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
        {
            cx.propagate();
            return;
        }
        let stops = self.working_row_stops(cx);
        let held = self
            .working_row_controls(cx)
            .and_then(|controls| controls.holding(window));
        let on_list = self.file_focus.is_focused(window);
        if !on_list && held.is_none() {
            // Shift+Tab from the stop Tab from the list lands on comes back
            // through the controls, not straight to the list.
            if !forward && !stops.is_empty() && self.stop_after_working_list(window, cx) {
                self.focus_row_stop(stops.last().expect("not empty"), window, cx);
            } else {
                cx.propagate();
            }
            return;
        }
        let at = held
            .as_ref()
            .and_then(|id| stops.iter().position(|stop| stop.id == *id));
        if forward {
            match stops.get(at.map_or(0, |index| index + 1)) {
                Some(stop) => self.focus_row_stop(stop, window, cx),
                // Every row control is out of the window's order, so from
                // the list it goes on past all of them.
                None => {
                    self.show_cursor_row(cx);
                    window.focus(&self.file_focus, cx);
                    window.focus_next(cx);
                }
            }
        } else if let Some(previous) = at.and_then(|index| index.checked_sub(1)) {
            self.focus_row_stop(&stops[previous], window, cx);
        } else if on_list {
            cx.propagate();
        } else {
            self.show_cursor_row(cx);
            window.focus(&self.file_focus, cx);
        }
    }

    /// The selected row's place among the shown rows.
    pub(super) fn shown_cursor_row(&self) -> Option<usize> {
        let (index, area) = self.shown_working_cursor()?;
        self.working_rows
            .iter()
            .position(|row| matches!(row, WorkingRow::File(i, a) if (*i, *a) == (index, area)))
    }

    /// Scrolls the selected row into view by the least scroll, as focus
    /// leaves the list's stops, a heading's reveal having taken it out of
    /// view. Only while the control holding focus was in the last frame's
    /// drawn rows: a wheel that took it out of them said where the person
    /// is reading, and a reveal never undoes manual scrolling. A key that
    /// lands before the next frame may skip one reveal.
    fn show_cursor_row(&self, cx: &App) {
        let drawn = self
            .working_row_controls(cx)
            .is_some_and(|controls| controls.focused().is_some());
        if drawn && let Some(position) = self.shown_cursor_row() {
            self.working_scroll
                .scroll_to_item(position, ScrollStrategy::Nearest);
        }
    }

    /// Whether the focus holder is the stop the window's own Tab reaches
    /// from the Working list, the composer's first where the composer is
    /// drawn: the one Shift+Tab walks back through the list's stops from.
    /// Asks the order by moving focus, which GPUI reports only once a frame
    /// ends, and puts it back.
    fn stop_after_working_list(&self, window: &mut Window, cx: &mut App) -> bool {
        let here = window.focused(cx);
        window.focus(&self.file_focus, cx);
        window.focus_next(cx);
        let next = window.focused(cx);
        match &here {
            Some(here) => window.focus(here, cx),
            None => window.blur(cx),
        }
        next.is_some() && next == here
    }

    /// Moves focus onto the row at `position`'s control if a row's or a
    /// heading's control held it before the selection moved there
    /// (`was_on_stop`): the roving stop moves with the selection, so the
    /// ring and Enter stay on the row the person is on, as the Reflog's
    /// entries take focus from the arrow keys. Where that row's control is
    /// no stop (`working_row_stops`), as where the rows draw none or it is
    /// landing from a flight, focus returns to the list, the row's own stop:
    /// a heading's control the arrow's scroll took out of view is not drawn
    /// either.
    pub(super) fn follow_selection_with_row_stop(
        &self,
        was_on_stop: bool,
        position: usize,
        window: &mut Window,
        cx: &mut App,
    ) {
        if !was_on_stop {
            return;
        }
        match self
            .working_row_stops(cx)
            .iter()
            .find(|stop| stop.position == position)
        {
            Some(stop) => self.focus_row_stop(stop, window, cx),
            None => window.focus(&self.file_focus, cx),
        }
    }

    pub(super) fn move_working_selection(
        &mut self,
        direction: i32,
        edge: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // With no file drawn the arrow has nowhere to go and refuses
        // nothing, so it shows no busy look: one there once showed it at
        // once during a write. Refused, it selects and scrolls nothing, and
        // the busy look says why at once: it once scrolled the list to the
        // row it would have selected, the selection left behind.
        if !self.working_files_drawn() || self.refuse_working_press(cx) {
            return;
        }
        let files: Vec<_> = self
            .actionable_working_rows()
            .iter()
            .enumerate()
            .filter_map(|(position, row)| match row {
                WorkingRow::File(i, area) => Some((position, *i, *area)),
                _ => None,
            })
            .collect();
        if files.is_empty() {
            return;
        }
        let current = files
            .iter()
            .position(|(_, i, area)| Some((*i, *area)) == self.working_selected)
            .unwrap_or(0);
        let next = if edge {
            if direction < 0 { 0 } else { files.len() - 1 }
        } else {
            (current as i32 + direction).clamp(0, files.len() as i32 - 1) as usize
        };
        let (position, index, area) = files[next];
        let on_stop = self.row_control_focused(window, cx);
        // An arrow that cannot move anything, Down on the last file or Up on
        // the first with no other selected, leaves focus where it is and the
        // list where it stands: it moved focus to the row's control and
        // recentred the list. One that collapses a multiselection moves it.
        if on_stop && !self.working_choice_moves(index, area, false) {
            return;
        }
        self.choose_working_selection(index, area, false, false, window, cx);
        self.working_scroll
            .scroll_to_item(position, ScrollStrategy::Center);
        self.follow_selection_with_row_stop(on_stop, position, window, cx);
    }

    /// One file row's Stage, or Unstage where `staged`, from its control or,
    /// where the control has left a narrow row, its menu. `path`, the row's
    /// file, confirms on its row's control in the area it moves to.
    fn stage_working_paths(
        &mut self,
        paths: Vec<PathBuf>,
        path: &std::path::Path,
        staged: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (command, label, arrives) = if staged {
            (
                WriteCommand::Unstage { paths },
                "Unstaging file…",
                ChangeArea::Unstaged,
            )
        } else {
            (
                WriteCommand::Stage { paths },
                "Staging file…",
                ChangeArea::Staged,
            )
        };
        let origin = confirmations::file(&self.path, path, arrives);
        self.write_from(command, label, Some(origin.into()), window, cx);
    }

    /// Submits `command` with no control to confirm it: its outcome reaches
    /// the window-wide notice, or the operation error.
    pub(super) fn write(
        &mut self,
        command: WriteCommand,
        label: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.write_from(command, label, None, window, cx);
    }

    /// Whether the window is busy: a write in flight, a status read, or a
    /// list's rows being prepared. A write's own read and rows follow it
    /// without a gap, so its spell runs from the press to the rows' landing.
    fn busy_spell(&self) -> bool {
        self.operation_busy.is_some() || self.status_task.is_some() || self.working_paths.pending
    }

    /// Follows the busy spell as each frame is built: one that begins asks
    /// for a frame [`BUSY_LOOK_DELAY`] later, which shows its look if it
    /// still runs, and one that ends drops its look.
    pub(super) fn track_busy_look(&mut self, cx: &mut Context<Self>) {
        if !self.busy_spell() {
            self.busy_look = BusyLook::default();
            return;
        }
        if self.busy_look.shown || self.busy_look.reveal.is_some() {
            return;
        }
        self.busy_look.reveal = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(BUSY_LOOK_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                this.busy_look.shown = true;
                cx.notify();
            });
        }));
    }

    /// Shows the busy spell's look at once, before [`BUSY_LOOK_DELAY`], as a
    /// press the spell refuses asks: a click, Enter, an arrow or Select All
    /// on the Working list while it refuses a choice
    /// ([`Self::refuse_working_press`]). The delay keeps a quick write from
    /// dimming controls the person did not touch; a refused press touched
    /// one, and the refusal and the look that says why then come in the
    /// same frame. A press refused in the delay once changed nothing and
    /// showed nothing, at the start of a write and while a quiet refresh's
    /// rows were prepared.
    pub(super) fn show_busy_look(&mut self, cx: &mut Context<Self>) {
        if self.busy_spell() && !self.busy_look.shown {
            self.busy_look.shown = true;
            self.busy_look.reveal = None;
            cx.notify();
        }
    }

    /// Whether the busy spell has lasted [`BUSY_LOOK_DELAY`], or a press it
    /// refused has shown its look ([`Self::show_busy_look`]), so what it
    /// makes unavailable looks unavailable.
    pub(super) fn busy_shown(&self) -> bool {
        self.busy_look.shown && self.busy_spell()
    }

    /// The write in flight, once its spell looks busy: the progress label,
    /// Cancel and the tab's "· working" wait for it.
    pub(super) fn operation_shown(&self) -> Option<&'static str> {
        self.operation_busy.filter(|_| self.busy_shown())
    }

    /// Whether Projects is refused now, and whether it looks it, wherever it
    /// is offered: the header's Projects, History's Back to Projects, File ▸
    /// Projects…, the palette and the Linux Main Menu. Refused while the
    /// busy spell runs, since each action Projects offers starts another
    /// operation, and dimmed once that spell looks busy, as everywhere else,
    /// but neither while the window waits only for a review being prepared
    /// or for a dropped preparation's read ([`Self::projects_ends_busy`]),
    /// which `show_projects` ends as it opens: the button and its shortcut
    /// do the same thing, and Projects is the way out of a read that never
    /// ends, so it is never refused for one, as its shortcut never was.
    /// Through that wait, and while a review was prepared, it once looked
    /// and was announced unavailable, giving the wait's reason, while its
    /// shortcut opened it.
    pub(crate) fn projects_refusal(&self) -> (bool, bool) {
        let refused = self.operation_busy.is_some() && !self.projects_ends_busy();
        (refused, refused && self.operation_shown().is_some())
    }

    /// Whether a dialog's actions on the repository at `path` are refused
    /// now, and whether they look it: refused while that repository is no
    /// longer the one shown or a write runs, and dimmed for a write only
    /// once it looks busy ([`BusyLook`]), as everywhere else. A dialog's
    /// own review closes it before its write submits, so a write it sees is
    /// never its own.
    pub(crate) fn dialog_refusal(&self, path: Option<&std::path::Path>) -> (bool, bool) {
        let elsewhere = self.path.as_deref() != path || self.page != AppPage::Repository;
        (
            elsewhere || self.operation_busy.is_some(),
            elsewhere || self.operation_shown().is_some(),
        )
    }
}

/// [`GitTurtle::dialog_refusal`] for a dialog that holds its window's
/// `owner`, refused and dimmed once that window is gone.
pub(crate) fn dialog_refusal(
    owner: &WeakEntity<GitTurtle>,
    path: Option<&std::path::Path>,
    cx: &App,
) -> (bool, bool) {
    owner
        .upgrade()
        .map_or((true, true), |owner| owner.read(cx).dialog_refusal(path))
}

/// A control of a dialog the window builds in its own render, as the kit's
/// alert dialogs are, given `owner`'s [`dialog_refusal`] for `path` once it
/// lays out: the window cannot be read while it renders.
pub(crate) fn dialog_refusing(
    owner: WeakEntity<GitTurtle>,
    path: Option<std::path::PathBuf>,
    build: impl FnOnce(bool, bool) -> AnyElement + 'static,
) -> DialogRefusing {
    DialogRefusing {
        owner,
        path,
        build: Box::new(build),
    }
}

/// See [`dialog_refusing`].
#[derive(IntoElement)]
pub(crate) struct DialogRefusing {
    owner: WeakEntity<GitTurtle>,
    path: Option<std::path::PathBuf>,
    build: Box<dyn FnOnce(bool, bool) -> AnyElement>,
}

impl RenderOnce for DialogRefusing {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let (refused, dimmed) = dialog_refusal(&self.owner, self.path.as_deref(), cx);
        (self.build)(refused, dimmed)
    }
}

impl GitTurtle {
    /// Submits `command`, asked for from `origin`. Once Git reports success,
    /// while its repository is still the one shown, the control `origin`
    /// marks, if any, confirms it where it was touched
    /// (`copy_feedback::confirm`), the outcome is announced once and the
    /// status line records it, and the window-wide notice stays down. A
    /// failure, an uncertain result, or a write with no `origin` or whose
    /// repository is no longer shown keeps today's error or notice; nothing
    /// is retried.
    pub(super) fn write_from(
        &mut self,
        command: WriteCommand,
        label: &'static str,
        origin: Option<confirmations::Origin>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() {
            return;
        }
        let downloading_lfs = matches!(&command, WriteCommand::DownloadLfs(_));
        let lfs_selection = if downloading_lfs {
            self.lfs_download_begins()
        } else {
            None
        };
        if !downloading_lfs {
            self.close_inspections(window, cx);
        }
        let Some(repo) = self.repository.clone() else {
            return;
        };
        let path = repo.path().to_owned();
        let ending_integration = matches!(
            &command,
            WriteCommand::Integration(
                gitturtle_core::IntegrationCommand::Abort { .. }
                    | gitturtle_core::IntegrationCommand::Quit { .. }
            )
        );
        let resolved_path = match &command {
            WriteCommand::Integration(gitturtle_core::IntegrationCommand::Resolve {
                expected,
                ..
            }) if !matches!(
                &command,
                WriteCommand::Integration(gitturtle_core::IntegrationCommand::Resolve {
                    resolution: gitturtle_core::ConflictResolution::Save { .. },
                    ..
                })
            ) =>
            {
                Some(expected.path.clone())
            }
            _ => None,
        };
        let submitted_message = match &command {
            WriteCommand::Commit { message } => Some(message.clone()),
            _ => None,
        };
        let submitted_branch = match &command {
            WriteCommand::Checkout { branch } => Some(branch.clone()),
            WriteCommand::CreateBranch { name, .. } => Some(name.clone()),
            _ => None,
        };
        let success_notice = match &command {
            WriteCommand::Integration(gitturtle_core::IntegrationCommand::Resolve {
                expected,
                resolution: gitturtle_core::ConflictResolution::Save { .. },
            }) => Some(format!(
                "Saved {} · remains unresolved until explicitly staged",
                expected.path.display()
            )),
            WriteCommand::Integration(gitturtle_core::IntegrationCommand::Resolve {
                expected,
                ..
            }) => Some(format!("Resolved and staged {}", expected.path.display())),
            WriteCommand::Integration(gitturtle_core::IntegrationCommand::Continue {
                expected,
            }) => Some(format!(
                "Continued {} on {}",
                expected.kind.label().to_lowercase(),
                expected.branch
            )),
            WriteCommand::Integration(gitturtle_core::IntegrationCommand::Abort { expected }) => {
                Some(format!(
                    "Aborted {} on {}",
                    expected.kind.label().to_lowercase(),
                    expected.branch
                ))
            }
            WriteCommand::Integration(gitturtle_core::IntegrationCommand::Quit { expected }) => {
                Some(format!(
                    "Stopped {} and kept the current files",
                    expected.kind.label().to_lowercase()
                ))
            }
            WriteCommand::Stage { paths } => Some(match paths.as_slice() {
                [path] => format!("Staged {}", path.display()),
                _ => "Staged selected changes".into(),
            }),
            WriteCommand::Unstage { paths } => Some(match paths.as_slice() {
                [path] => format!("Unstaged {}", path.display()),
                _ => "Unstaged selected changes".into(),
            }),
            WriteCommand::StageAll => Some("Staged all working changes".into()),
            WriteCommand::UnstageAll => Some("Unstaged all changes".into()),
            WriteCommand::Checkout { branch } => Some(format!("Switched to {branch}")),
            WriteCommand::CreateBranch { name, .. } => {
                Some(format!("Created and switched to {name}"))
            }
            WriteCommand::Fetch { remote } => Some(format!("Fetched {remote}")),
            WriteCommand::Pull { remote, branch } => Some(format!("Pulled {remote}/{branch}")),
            WriteCommand::Push {
                remote,
                local_branch,
                remote_branch,
            } => Some(format!("Pushed {local_branch} to {remote}/{remote_branch}")),
            WriteCommand::Branch(branch) => match branch.as_ref() {
                gitturtle_core::BranchCommand::CreateTracking { plan } => {
                    let upstream = plan.remote_ref.trim_start_matches("refs/remotes/");
                    Some(if plan.checkout {
                        format!("Created and switched to {} · tracks {upstream}", plan.name)
                    } else {
                        format!("Created {} · tracks {upstream}", plan.name)
                    })
                }
                _ => None,
            },
            _ => None,
        };
        // A Fetch or Pull that brought nothing new (`WriteOutcome::up_to_date`)
        // says so in the status line and its announcement, naming what it
        // read: a Fetch the remote, whose every ref it compared, a Pull the
        // branch it pulled. Its control still reads Fetched or Pulled, whose
        // width is the control's own.
        let up_to_date_notice = match &command {
            WriteCommand::Fetch { remote } => {
                Some(format!("Up to date with {remote} · nothing new"))
            }
            WriteCommand::Pull { remote, branch } => {
                Some(format!("Up to date with {remote}/{branch} · nothing new"))
            }
            _ => None,
        };
        let submitted_profile = if let WriteCommand::ApplyProfile(plan) = &command {
            self.submitted_profile(plan)
        } else {
            None
        };
        let tag_command = if let WriteCommand::Tag(command) = &command {
            Some(Arc::clone(command))
        } else {
            None
        };
        let worktree_command = if let WriteCommand::Worktree(command) = &command {
            Some(Arc::clone(command))
        } else {
            None
        };
        let recovery = if let WriteCommand::Recovery(command) = &command {
            Some(Arc::clone(command))
        } else {
            None
        };
        let refresh_history = matches!(
            &command,
            WriteCommand::Commit { .. }
                | WriteCommand::InteractiveRebase(_)
                | WriteCommand::Worktree(_)
                | WriteCommand::RecoverReflog(_)
                | WriteCommand::Branch(_)
                | WriteCommand::Tag(_)
                | WriteCommand::Checkout { .. }
                | WriteCommand::CreateBranch { .. }
                | WriteCommand::Fetch { .. }
                | WriteCommand::Pull { .. }
                | WriteCommand::Push { .. }
        ) || matches!(&command, WriteCommand::Integration(operation) if !matches!(operation, gitturtle_core::IntegrationCommand::Resolve { .. }))
            || matches!(&command, WriteCommand::Recovery(command) if matches!(command.as_ref(), gitturtle_core::RecoveryCommand::Amend { .. } | gitturtle_core::RecoveryCommand::Undo { .. } | gitturtle_core::RecoveryCommand::Revert { .. } | gitturtle_core::RecoveryCommand::CherryPick { .. }));
        self.operation_repository = Some(path.clone());
        self.operation_busy = Some(label);
        self.operation_error = None;
        self.operation_notice = None;
        self.status_outcome = None;
        self.work_generation = self.work_generation.wrapping_add(1);
        self.status_task = None;
        if self.mode == WorkspaceMode::Working && !downloading_lfs {
            self.invalidate_read();
            self.clear_preview();
            self.files.clear();
            self.file_paths.reset();
            self.selected_file = None;
        }
        let target = activity::target(
            &command,
            self.work_status
                .as_ref()
                .and_then(|s| s.branch.as_deref())
                .unwrap_or("Current worktree"),
        );
        let activity_id = self.activity.begin(&path, label, &target);
        self.save_activity(window, cx);
        let control = self.begin_operation_control(window, cx);
        let activity_control = control.clone();
        let rewrite_capture = self.prepare_rewrite_capture(&command);
        let response = self.operations.submit_controlled(control, move || {
            if let Some(capture) = rewrite_capture {
                futures::executor::block_on(capture).map_err(|_| {
                    anyhow::anyhow!(
                        "Original-series save ended without a result. Git was not started"
                    )
                })??;
            }
            repo.execute(&command)
        });
        self.operation_task = Some(cx.spawn_in(window, async move |this,cx| {
            let result=response.await.unwrap_or_else(|_|Err(anyhow::anyhow!("Operation ended without a result. Refresh before retrying; repository state may have changed.")));
            let _=this.update_in(cx,|this,window,cx| {
                let (success, explanation) = match &result {
                    Ok(_) => (true, "Git reported that the operation completed.".to_owned()),
                    Err(error) => (false, format!("{error:#}")),
                };
                let uncertain = explanation.contains("without a result") || explanation.contains("timed out") || explanation.contains("deadline") || explanation.contains("exceeded");
                this.activity.finish(activity_id, success, activity_control.is_cancelled(), uncertain, &explanation);
                this.save_activity(window, cx);
                this.operation_busy=None;
                this.operation_repository=None;
                this.finish_operation_control(window, cx);
                let succeeded=result.is_ok();
                let lfs_failure = result.as_ref().err().filter(|_| downloading_lfs).map(|error| views::OperationFailure::of(error).banner());
                match result {
                    Ok(outcome) => {
                        if let Some(oid) = &outcome.commit_oid { this.tab_commit_receipt(&path, oid.clone()); }
                        let notice = if let (Some(oid), Some(message)) = (outcome.commit_oid.as_ref(), submitted_message.as_deref()) {
                            format!("Committed {} · {}", short_oid(oid), message.lines().next().unwrap_or_default())
                        } else if let Some(notice) = up_to_date_notice.as_ref().filter(|_| outcome.up_to_date) {
                            notice.clone()
                        } else if let Some(notice) = &success_notice {
                            notice.clone()
                        } else {
                            outcome.message.lines().find(|line| !line.trim().is_empty()).unwrap_or("Git operation completed").to_owned()
                        };
                        match origin.filter(|_| this.path.as_ref() == Some(&path) && this.page == AppPage::Repository) {
                            Some(origin) => {
                                this.tab_operation_feedback(&path, None, None, cx);
                                this.record_outcome(&path, notice.clone(), cx);
                                copy_feedback::confirm(window, cx, origin.mark(), notice);
                            }
                            None => this.tab_operation_feedback(&path, Some(notice), None, cx),
                        }
                    }
                    Err(error) => {
                        this.tab_operation_feedback(&path, None, Some(views::OperationFailure::of(&error)), cx);
                    }
                }
                if let Some(profile) = submitted_profile { this.finish_profile_write(&path, profile, succeeded, window, cx); }
                if let Some(command) = &tag_command { this.finish_tag_write(&path, command, succeeded, cx); }
                if let Some(command) = &worktree_command { this.finish_worktree_write(&path, command, succeeded, window, cx); }
                if let Some(command) = &recovery { this.finish_recovery_write(&path, command, succeeded, cx); }
                if succeeded && ending_integration { this.conflict_drafts.retain(|(repository, _), _| repository != &path); }
                if succeeded && let Some(resolved) = &resolved_path {
                    this.conflict_drafts.remove(&(path.clone(), resolved.clone()));
                }
                if succeeded && let Some(message) = &submitted_message {
                    this.clear_committed_draft(&path, message, window, cx);
                }
                if this.path.as_ref()==Some(&path) && succeeded && submitted_branch.as_deref().is_some_and(|branch| this.branch_name.read(cx).value().trim() == branch) {
                    this.branch_name.update(cx,|input,cx|input.set_value("",window,cx));
                }
                if downloading_lfs { this.lfs_download_ends(lfs_failure); }
                let download = downloading_lfs.then_some((succeeded, lfs_selection));
                this.read_after_write(&path, refresh_history, download, window, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// The reads a write in `path` asks for as it ends: a write that
    /// `refresh_history` names refreshes quietly, keeping what History and
    /// Changes show; an LFS `download` reloads only the preview it was asked
    /// from, if that preview is still shown; any other write reads the
    /// working state. Then a read asked for while the write ran, in
    /// whichever repository is shown, runs ([`Self::read_deferred_working`]).
    pub(super) fn read_after_write(
        &mut self,
        path: &std::path::Path,
        refresh_history: bool,
        download: Option<(bool, Option<LfsView>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.path.as_deref() == Some(path) {
            if refresh_history {
                // The quiet read reads the working state too, and gives a
                // selected row the preview Changes cleared as it opened
                // (`worker::QuietPreview::Changed`).
                self.working_read_deferred = None;
                self.refresh_after_write(path.to_owned(), window, cx);
            } else if let Some((succeeded, selection)) = download {
                // Only the preview the download was asked from reloads. A
                // view that has moved on meanwhile keeps its own preview,
                // which a working read here would clear and load again,
                // losing its scroll; Changes opened meanwhile reads below.
                let same_view = selection.as_ref().is_some_and(|(file, mode, generation)| {
                    self.mode == *mode
                        && self.generation == *generation
                        && self.selected_file.and_then(|index| self.files.get(index)) == Some(file)
                });
                if succeeded && same_view {
                    if self.mode == WorkspaceMode::Working {
                        if let Some((index, area)) = self.working_selected {
                            self.select_working(index, area, window, cx);
                        }
                    } else {
                        self.read_selected_preview(window, cx);
                    }
                }
            } else {
                // Menus wait for this read as for a reload
                // (`write_unsettled`).
                self.refresh_worktree(window, cx);
                self.status_after_write = Some(self.work_generation);
            }
        }
        self.read_deferred_working(window, cx);
    }

    fn refresh_after_write(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.path.as_ref() != Some(&path) {
            return;
        }
        // A write changes repository state, not the user's browsing intent.
        // Reopening discarded the selected scope and restarted a retained query
        // over all refs. Quiet refresh resolves that same scope again while
        // keeping pinned search results, immutable comparisons and scroll/focus.
        self.queue_automatic_refresh(
            local_refresh::LocalChange {
                git: true,
                worktree: true,
                after_write: true,
                ..Default::default()
            },
            window,
            cx,
        );
    }

    /// The Working inspector's layout as last laid out, `width` being the
    /// inspector's expected width until it is first laid out. The shown tab
    /// holds it while another tab, mode or page is drawn and a warm tab keeps
    /// it, so Changes shown again lays its list and composer out where they
    /// were drawn; taken as the window renders (`render_repository`), not as
    /// the inspector is built in its frame, it outlasts the project pane
    /// coming and going too. Kept as element state, it lasted only while
    /// drawn: a Changes tab shown again laid its list and composer out from
    /// the estimates below and drew a second frame where they measured
    /// themselves. A resize, text size or density change while it is not
    /// drawn keeps it too, unlike History's list layouts: measured at the old
    /// size it is still nearer than the estimates, so the return draws the
    /// two frames a resize draws while Changes is shown, where estimates drew
    /// three.
    pub(super) fn working_inspector_layout(
        &self,
        width: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<WorkingInspectorLayout> {
        let previous_list = self.working_scroll.0.borrow().last_item_size;
        let viewport = window.viewport_size().height;
        // Measure the list/composer body after Targets, feedback, the Changes
        // header and wrapping path/selection controls have taken their space.
        let layout = self.working_layout.take().unwrap_or_else(|| {
            cx.new(|_| WorkingInspectorLayout {
                height: (viewport - px(400.)).max(px(160.)),
                width,
                list_size: previous_list.map(|size| size.item),
                inspector: (viewport - px(200.)).max(px(160.)),
                scrolled_stops: None,
                row_controls: RowControls::default(),
            })
        });
        self.working_layout.set(Some(layout.clone()));
        layout
    }

    /// Whether menus refuse writes: one runs, or one has ended and the quiet
    /// reload it asked for (`refresh_after_write`) has not landed, so the
    /// window still shows what the write found. Built in between, a menu
    /// offered its items for that state: a commit's Revert of a commit an
    /// Undo had just taken away, or a branch's actions on a name a delete
    /// had just removed. A reload that another page or an open inspection
    /// holds off (`quiet_reads_held`), as Blame or File history open as the
    /// write ends, refuses nothing until it can run: menus stayed refused
    /// there, with nothing to say why, until the window returned to History
    /// or Changes. One that waits for a history load or search, as a
    /// selection's read puts it back to wait, still refuses, since the read
    /// starts it as it ends. A write that changes only the working state, as
    /// a Stage, reads the status again instead (`status_after_write`), which
    /// menus wait for as well: built as the write ended, a working row's
    /// menu offered Delete untracked file… for a file the Stage had just
    /// added. A read the watcher asked for refuses nothing.
    pub(super) fn write_unsettled(&self) -> bool {
        self.operation_busy.is_some()
            || self
                .automatic
                .reloading_after_write(!self.quiet_reads_held())
            || (self.status_task.is_some() && self.status_after_write == Some(self.work_generation))
    }

    /// `width` is the inspector's width where the window pins it, otherwise
    /// the width it is expected to take until it is laid out; `layout` is its
    /// layout as last laid out ([`Self::working_inspector_layout`]).
    pub(super) fn render_working_inspector(
        &self,
        layout: Entity<WorkingInspectorLayout>,
        width: Pixels,
        pinned: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        bind_working_keys(cx);
        let p = palette(cx);
        let previous_list = self.working_scroll.0.borrow().last_item_size;
        #[cfg(test)]
        WORKING_LAYOUT_STATE.with(|state| state.set(Some(layout.entity_id())));
        // The short form also shapes the controls above the body, so it is
        // chosen before layout from the body's height as last laid out; the
        // list and the composer are sized from its height in this frame as
        // it is prepainted (`WorkingBody`).
        let short = short_body(layout.read(cx).height);
        let fitted_width = if pinned { width } else { layout.read(cx).width };
        // Where the list and the composer scroll together, the body keeps the
        // scroll cue's gutter at its right (`tags::ScrollCue`), which it keeps
        // where it scrolled as last laid out, the cue's own rule, also in a
        // frame that no longer scrolls (`render_working_body`): what the body
        // holds is fitted to the width that leaves in the frame that keeps it.
        // Fitted to the inspector's width, Commit's label and the file rows
        // were laid out a gutter too wide and cut at its edge, at 18 pt in a
        // 461 × 490 window. The controls above the body keep the inspector's
        // width. The track lies over the rows' trailing inset, `px_3`, rather
        // than beside it: the list and the composer keep the body's whole
        // width, the list's edge beneath the track and its focus ring painted
        // over it (`list_edge`), 3 px
        // outside the thumb the track paints 4 px in, and their rows keep the
        // gutter less that inset at their right (`WorkingForms::trailing`),
        // so each row's content and its controls' rings end where the track
        // begins. Beside the whole gutter, "README.md" was drawn 76.5 px wide
        // for its 89.7 at 18 pt in a 430 × 490 window.
        let gutter = if self.working_body.scroll().max_offset().y > Pixels::ZERO {
            crate::tags::ScrollCue::gutter()
        } else {
            Pixels::ZERO
        };
        let trailing = trailing_room(gutter, window.rem_size() * 0.75, window.scale_factor());
        let body_width = fitted_width - trailing;
        let title_length = self.title_length(window, cx);
        let forms = self.working_forms(fitted_width, body_width, short, title_length, window, cx);
        // The measured body's height must not change the header height, which
        // would oscillate at a size threshold on successive layout passes.
        let header_height = WORKING_HEADER;
        let list_layout = layout.clone();
        let list_scroll = self.working_scroll.clone();
        let selected_row = self.actionable_working_rows().iter().position(
            |row| matches!(row, WorkingRow::File(index, area) if Some((*index, *area)) == self.working_selected),
        );
        let confirming_heading = self.confirming_heading(cx);
        let (reveal_row, row_count) = (
            window.pixel_snap(px(self.settings.density.file_row_height())),
            self.working_rows.len(),
        );
        // GPUI retains this content geometry in the scroll handle even while
        // Settings hides the inspector. Density changes must not reuse a pixel
        // offset that now points at a different set of rows. Rows that change
        // in number, as when a write's rows replace the empty list of its
        // pending filter, take this path too, so a confirming heading is
        // shown as the rows' arrival showed it.
        if let Some(size) =
            previous_list.filter(|size| size.contents.height != reveal_row * row_count as f32)
        {
            if let Some(heading) = confirming_heading {
                reveal_heading(
                    &self.working_scroll,
                    size.item.height,
                    reveal_row,
                    row_count,
                    selected_row,
                    heading,
                );
            } else if let Some(selected) = selected_row {
                self.working_scroll
                    .scroll_to_item(selected, ScrollStrategy::Nearest);
            }
            // Where the list scrolls with the composer, it comes back into
            // view with its selected file, at the body's top, only while
            // focus is in the list. Anywhere else, a composer stop, the
            // filter, another control or nothing, the body keeps its
            // offset, since a redraw without a focus change scrolls nothing:
            // returned to its top, a watcher's refresh moved Commit from
            // under a pointer that had scrolled to it. The list's own scroll
            // keeps the selected file among its rows, and a composer stop
            // the change moves is revealed again (`render_working_body`).
            if selected_row.is_some() && self.file_focus.contains_focused(window, cx) {
                self.working_body.scroll().set_offset(Point::default());
            }
        }
        let staged = self.working_paths.staged_count;
        // A marked tab's rows, counts and badge are those of the last status
        // read, while the header says "Changes": it lists none and says
        // "Changes unavailable", as before any status is read.
        let marked = self.shown_tab_unavailable();
        // Of the status the shown rows index, as their counts are.
        let total = (self.shown_working_status())
            .filter(|_| !marked)
            .map_or(0, |status| status.entries.len());
        let conflicted = self.working_paths.conflicted;
        let identity_ready = self
            .profile
            .as_ref()
            .is_some_and(|p| !p.name.is_empty() && !p.email.is_empty());
        let busy = self.operation_busy.is_some();
        let refreshing = self.status_task.is_some();
        // What a busy spell makes unavailable refuses at once but looks it
        // only once the spell has lasted a moment (`BusyLook`).
        let looks_busy = self.busy_shown();
        // A result is on screen: the status's own rows, or, while a newer
        // status's are prepared, the rows kept from before, which keep the
        // layout they had.
        let shown = !marked
            && self.work_status.is_some()
            && !self.working_filter_waiting()
            && self.working_paths.error.is_none();
        // The commit waits for the rows it counts to be the status's own.
        let status_ready = shown && !self.working_paths.pending;
        // With nothing to list, one line says why (`empty_state`).
        let clean = (total == 0).then(|| {
            if shown {
                EmptyState::new(
                    "working-clean",
                    empty_state::working_clean_line(self.work_status.as_deref(), false),
                )
                .pane(cx)
            } else if !marked && self.working_read_pending() {
                // Its read may wait for a running write
                // (`Self::refresh_worktree`).
                EmptyState::new("working-reading", "Reading the working tree…").pane(cx)
            } else {
                // Refresh, in the header just above, reads it again.
                EmptyState::new("working-unavailable", "Changes unavailable")
                    .tooltip("Refresh to read the working tree again.")
                    .pane(cx)
            }
        });
        // Where the rows would start, at their text inset, the line and
        // Clear filter below it, which empties the field and moves to the
        // list (`empty_state`). Above and below, half the gap: the list is
        // as short as its floor in a half tile at 18 pt. Where the line would
        // wrap, as there, it takes its short form, and where its label does
        // not fit the list the action is its icon, named and tooltipped by
        // its label.
        let no_match = total > 0 && self.working_no_match();
        let unmatched = no_match.then(|| {
            let room = working_no_match_room(body_width, window);
            let labelled = room >= labelled_button_width(CLEAR_FILTER, &CLEAR_FILTER_LABEL, window);
            let clear = if labelled {
                empty_state::action("working-clear-filter", CLEAR_FILTER, "close")
            } else {
                crate::views::compact_control_name(
                    empty_state::action("working-clear-filter", "", "close"),
                    "working-clear-filter",
                    CLEAR_FILTER,
                )
                .tooltip(CLEAR_FILTER)
            };
            div()
                .pl_3()
                .pr(forms.row_end(window))
                .py(appearance::ui_size(empty_state::GAP / 2.))
                .child(
                    EmptyState::new("working-no-match", working_no_match_line(room, window))
                        .named(WORKING_NO_MATCH)
                        .action(clear.on_click(
                            cx.listener(|this, _, window, cx| {
                                this.clear_working_filter(window, cx)
                            }),
                        ))
                        .block(cx),
                )
        });
        // The commit waits for a write in flight and for the rows it counts
        // (`status_ready`), which its click refuses at once, and looks
        // unavailable for them once the spell looks busy.
        let commit_unavailable = staged == 0
            || conflicted
            || !identity_ready
            || self.commit_title.read(cx).value().trim().is_empty();
        let commit_disabled =
            ((busy || !status_ready) && (looks_busy || !shown)) || commit_unavailable;
        let commit_refused = busy || !status_ready || commit_unavailable;
        // An operation in progress replaces the composer with its banner.
        let composer = self
            .work_status
            .as_ref()
            .is_none_or(|status| status.operation.is_none());
        // A path filter that matches nothing: the list says so in its own
        // bounds, with Clear filter, and draws neither group's heading, as
        // with nothing to list; it keeps the room to show both whole.
        let floor = if no_match {
            working_no_match_floor(body_width, window)
        } else {
            working_list_floor(
                working_floor_rows(&self.working_rows),
                self.settings.density,
                window,
            )
        };
        // The composer's least height with its heading, a one-line
        // description counted (`ComposerMetrics::least`), chooses the form:
        // the heading leaves before the body would scroll.
        let least = if composer {
            let metrics = ComposerMetrics::new(short, true, window);
            let (rows, _) = self.composer_rows(
                &metrics,
                body_width,
                identity_ready,
                shown,
                title_length,
                window,
            );
            let section = self.description_section(&metrics, body_width, window);
            f32::from(metrics.least(rows, section))
        } else {
            0.
        };
        let fit = self.working_fit(
            forms,
            layout.read(cx).inspector,
            fitted_width,
            floor + least,
            window,
        );
        let metrics = ComposerMetrics::new(short, fit.heading, window);
        // The fields' rows but the description's field, at the interface
        // size: the composer prefers room for them whole with that field.
        let (rows, stops) = self.composer_rows(
            &metrics,
            body_width,
            identity_ready,
            shown,
            title_length,
            window,
        );
        // The Description section's own rows.
        let section = self.description_section(&metrics, body_width, window);
        let minimum = f32::from(metrics.least(rows, section));
        // A row that its Stage or Unstage moved travels to its slot above
        // the rows and beneath the section headings, and the list keeps the
        // slot empty until it lands (`motion`).
        let flight = self.working_flight(cx);
        let landing = flight
            .as_ref()
            .map(|flight| (flight.position, flight.slot.clone()));
        let row_height = px(self.settings.density.file_row_height());
        // The rows' controls take their handles from the layout, which keeps
        // where the focused one lies for the floor's fades.
        let controls = layout.read(cx).row_controls.clone();
        controls.begin(
            |control, drawn| self.lists_row_control(control, drawn),
            &self.file_focus,
            forms.file_action,
            window,
            cx,
        );
        // A wheel can take the control holding focus out of the drawn rows,
        // and a handle no element tracks takes its keys at the window's root,
        // past this page's Tab and arrow bindings. This element keeps it in
        // the frame, ahead of the rows, so a drawn row's Button, tracking the
        // same handle later in the frame, takes it back.
        let held_control = controls
            .held_handle(window)
            .map(|focus| div().absolute().size_0().track_focus(&focus));
        let row_controls = controls.clone();
        let pane_controls = controls.clone();
        let list = uniform_list(
            "working-files",
            self.working_rows.len(),
            cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
                range
                    .map(|i| match &landing {
                        Some((position, slot)) if *position == i => {
                            crate::motion::slot(slot.clone(), row_height)
                        }
                        landing => {
                            if let (Some((_, slot)), Some(WorkingRow::Heading(..))) =
                                (landing, this.working_rows.get(i))
                            {
                                slot.heading(i);
                            }
                            this.render_working_row(i, forms, Some(&row_controls), window, cx)
                        }
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .size_full()
        .track_scroll(&self.working_scroll);
        let travelling = flight.map(|flight| {
            // Its copy is a picture of the row: it tracks no handle, is no
            // Tab stop and takes no click, and assistive technology meets the
            // row only where it lands. While it flew, its Unstage took a
            // double-click's second press and unstaged the file again, and
            // assistive technology found it as a button beside the rows.
            let row = self.render_working_row(flight.position, forms, None, window, cx);
            let picture = native_accessibility::hidden_from_assistive_technology(
                div().id("working-travelling-row").size_full().child(row),
                true,
            );
            crate::motion::TravellingRow::new(flight, picture.into_any_element())
        });
        let refresh = button("refresh-working", "", "refresh", false)
            .debug_selector(|| "refresh-working".into())
            .accessibility_label("Refresh working changes")
            .tooltip(if refreshing && looks_busy {
                "Refresh working changes · Refreshing…"
            } else {
                "Refresh working changes"
            })
            .map(|refresh| {
                let refused = busy || refreshing;
                refusing(
                    refresh,
                    || "refresh-working".into(),
                    refused,
                    refused && looks_busy,
                )
            })
            .on_click(cx.listener(|this, _, window, cx| {
                // Refused at once, whatever it looks like yet.
                if this.operation_busy.is_none() && this.status_task.is_none() {
                    this.refresh_worktree(window, cx)
                }
            }));
        // Where the inspector is too short for these controls whole beside
        // the header, the list's floor and the composer's minimum, the
        // filter, Actions and Refresh share one row and the selection
        // controls move into the Actions menu (`WorkingFit`), so no control
        // is cut.
        let controls = if fit.tight {
            vec![
                div()
                    .debug_selector(|| "working-toolbar".into())
                    .flex_shrink_0()
                    .px_3()
                    .py_1()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .debug_selector(|| "working-filter".into())
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .child(
                                Input::new(&self.working_filter)
                                    .aria_label(FILTER_NAME)
                                    .text_size(crate::appearance::ui_text(12.)),
                            )
                            // Painted after the field, it fits its placeholder.
                            .child(self.fit_filter_placeholder()),
                    )
                    .child(self.render_working_recovery_menu(
                        true,
                        Some(!forms.toolbar_refresh),
                        cx,
                    ))
                    .when(forms.toolbar_refresh, |el| el.child(refresh))
                    .into_any_element(),
            ]
        } else {
            vec![
                // The selection controls fit whole; the scroll only guards a
                // prediction that missed.
                native_accessibility::clips_children(
                    div()
                        .id("working-selection")
                        .debug_selector(|| "working-selection".into())
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(self.render_working_selection(forms.compact_selection, cx)),
                )
                .into_any_element(),
                div()
                    .debug_selector(|| "working-header".into())
                    .h(px(header_height))
                    .flex_shrink_0()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgb(p.border))
                    .when(forms.header_icon, |el| {
                        el.child(
                            div()
                                .debug_selector(|| "working-header-icon".into())
                                .child(icon("commit", 17., p.accent)),
                        )
                    })
                    .when(forms.header_label, |el| {
                        el.child(
                            div()
                                .debug_selector(|| "working-header-label".into())
                                .text_size(crate::appearance::ui_text(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Changes"),
                        )
                    })
                    // The count leaves with its label: alone it would repeat
                    // the Changes tab or toggle's count above it. A marked
                    // tab has none, as the Changes tab has none.
                    .when(forms.header_label && !marked, |el| {
                        el.child(
                            div()
                                .debug_selector(|| "working-header-count".into())
                                .px_2()
                                .py_0p5()
                                .rounded(px(5.))
                                .bg(rgb(p.hover))
                                .text_size(crate::appearance::ui_text(11.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(crate::appearance::labels(cx).count))
                                .child(total.to_string()),
                        )
                    })
                    // Actions and Refresh hold the trailing edge; the texts
                    // before them give way, so neither is pushed out.
                    .child(
                        div()
                            .ml_auto()
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap_2()
                            .when(forms.compact_actions, |el| el.gap_1())
                            .when(refreshing && looks_busy && !forms.compact_actions, |el| {
                                el.child(
                                    div()
                                        .debug_selector(|| "working-refreshing".into())
                                        .text_size(crate::appearance::ui_text(10.))
                                        .text_color(rgb(p.muted))
                                        .child("Refreshing…"),
                                )
                            })
                            .child(self.render_working_recovery_menu(
                                forms.compact_actions,
                                None,
                                cx,
                            ))
                            .child(refresh),
                    )
                    .into_any_element(),
            ]
        };
        // The file list, but for the height the body gives it as it is
        // prepainted (`render_working_body`).
        let listed = total > 0 && !no_match;
        let pane = div()
            .id("working-pane")
            .debug_selector(|| "working-pane".into())
            .role(Role::ListBox)
            .aria_label("Working tree files")
            .tab_stop(true)
            .key_context("GitTurtleList")
            .track_focus(&self.file_focus)
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.pointer_moved(event.position, cx)
            }))
            .on_action(
                cx.listener(|this, _: &gpui_kit::component::input::Copy, window, cx| {
                    this.copy_list_item(crate::views::ListCopy::WorkingPaths, window, cx)
                }),
            )
            .on_action(cx.listener(
                move |this, _: &native_accessibility::RowActions, window, cx| {
                    this.open_working_row_menu(window, cx)
                },
            ))
            // Enter on the list chooses the selected row's file, as a click
            // on it does, where the arrow keys only select it; with nothing
            // to activate, the window's Open Selected List Item takes it. On
            // a control inside the list it goes on to that control
            // (`working_control_focused`).
            .on_action(cx.listener(|this, _: &NextPane, window, cx| {
                // One the list refuses chooses nothing, its busy look saying
                // why at once (`refuse_working_press`). With no file drawn,
                // clean or matching no filter, there is nothing to choose or
                // refuse, so no busy look either, as for an arrow
                // (`working_files_drawn`): Enter there during a write once
                // showed the write's look at once.
                if this.working_control_focused(window, cx)
                    || !this.working_files_drawn()
                    || this.refuse_working_press(cx)
                    || !this.activate_working_preview(window, cx)
                {
                    cx.propagate();
                }
            }))
            .border_1()
            .border_color(rgb(p.border))
            // One pixel in from the inspector's divider, which the kit
            // paints over the inspector's first pixel: the list's own
            // left edge and focus ring stay whole beside it, as the
            // commit inspector's list does beside its own edge.
            .ml(px(1.))
            // Its floor, whatever else gives way, so its focus ring
            // surrounds a list that shows a file.
            .relative()
            .flex_1()
            .min_h(px(floor))
            .overflow_hidden()
            .children(held_control)
            .when(listed, |el| el.child(list))
            .children(travelling.filter(|_| listed))
            .children(clean)
            .children(unmatched)
            .children(
                self.file_menu.as_ref().map(|menu| {
                    menu.render("working-row-menu-overlay", Self::dismiss_file_menu, cx)
                }),
            )
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, cx| {
                        pane_controls.record_pane(bounds);
                        list_layout.update(cx, |layout, cx| {
                            if layout.list_size != Some(bounds.size) {
                                // The list's height changes with the body's
                                // and with the composer's share: a confirming
                                // heading is shown again in the height it now
                                // has.
                                if layout.list_size.is_some()
                                    && let Some(heading) = confirming_heading
                                {
                                    reveal_heading(
                                        &list_scroll,
                                        bounds.size.height,
                                        reveal_row,
                                        row_count,
                                        selected_row,
                                        heading,
                                    );
                                } else if layout.list_size.is_some()
                                    && let Some(selected) = selected_row
                                {
                                    list_scroll.scroll_to_item(selected, ScrollStrategy::Nearest);
                                }
                                layout.list_size = Some(bounds.size);
                                cx.notify();
                                window.defer(cx, |window, _| window.refresh());
                            }
                        });
                    },
                )
                .absolute()
                .inset_0(),
            );
        #[cfg(test)]
        let pane = pane
            .children(self.working_probe.recorder(|frame| &mut frame.list))
            .children(self.working_probe.ring_recorder(
                self.focus_ring.clone(),
                self.file_focus.clone(),
                layout.read(cx).row_controls.clone(),
            ));
        let inspector_layout = layout.clone();
        #[cfg(test)]
        let probe = self.working_probe.frame(self.working_paths.pending);
        #[cfg(not(test))]
        let probe = false;
        div()
            .size_full()
            .min_h_0()
            .relative()
            .flex()
            .flex_col()
            .bg(rgb(p.panel))
            // The inspector's own height, which no form of its rows changes,
            // chooses the form.
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, cx| {
                        inspector_layout.update(cx, |layout, cx| {
                            if layout.inspector != bounds.size.height {
                                layout.inspector = bounds.size.height;
                                cx.notify();
                                window.defer(cx, |window, _| window.refresh());
                            }
                        });
                    },
                )
                .absolute()
                .inset_0(),
            )
            .children(controls)
            .when(probe, |el| {
                el.child(
                    div()
                        .debug_selector(|| "working-probe-line".into())
                        .flex_shrink_0()
                        .h(px(21.)),
                )
            })
            .child(WorkingBody {
                app: cx.entity(),
                // Where the controls share one row, nothing above the body
                // gives way any further: a body shorter than the list's floor
                // and the composer's minimum scrolls them together
                // (`render_working_body`) rather than draw the commit action
                // below the window.
                min: px(floor + if composer && !fit.tight { minimum } else { 0. }),
                parts: Some(WorkingBodyParts {
                    layout,
                    pane,
                    short,
                    forms,
                    fit,
                    gutter,
                    metrics,
                    rows,
                    stops,
                    section,
                    title_length,
                    floor,
                    composer,
                    listed,
                    staged,
                    busy,
                    identity_ready,
                    shown,
                    conflicted,
                    commit_unavailable,
                    commit_disabled,
                    commit_refused,
                }),
            })
            .into_any_element()
    }

    /// The Working inspector's list and composer in a body `size` large, as
    /// the body is prepainted (`WorkingBody`): the composer's share, the
    /// list's whole rows, the description's field and what is left between
    /// them, from the body's height in this frame.
    fn render_working_body(
        &self,
        parts: WorkingBodyParts,
        size: Size<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let WorkingBodyParts {
            layout,
            pane,
            short,
            forms,
            fit,
            gutter,
            metrics,
            rows,
            stops,
            section,
            title_length,
            floor,
            composer,
            listed,
            staged,
            busy,
            identity_ready,
            shown,
            conflicted,
            commit_unavailable,
            commit_disabled,
            commit_refused,
        } = parts;
        // What is chosen before layout from the body's size, the short form
        // and the rows' forms, is chosen again where this frame's size
        // changes it. The height alone sizes only what is built here, so a
        // body that grows or shrinks asks for no other frame.
        layout.update(cx, |layout, cx| {
            let changed =
                short_body(layout.height) != short_body(size.height) || layout.width != size.width;
            layout.height = size.height;
            layout.width = size.width;
            if changed {
                cx.notify();
                // Refresh is ignored while the window draws. Defer it so the
                // workspace renders again in the new form.
                window.defer(cx, |window, _| window.refresh());
            }
        });
        let p = palette(cx);
        let available = f32::from(size.height);
        let normal_description = if self.settings.density == appearance::Density::Compact {
            100.
        } else {
            128.
        };
        let description_height = ((available - 440.) * 0.5 + 64.).clamp(64., normal_description);
        let description_height = description_height.max(f32::from(metrics.description_least()));
        let composer_height = commit_composer_height(
            available,
            self.settings.density.file_row_height(),
            (description_height + if short { 180. } else { 244. })
                .max(f32::from(metrics.chrome() + rows) + description_height),
        );
        // The list keeps its floor; a share too short for the composer's
        // least height is made up below.
        let composer_height = composer_height.min(available - floor);
        // Where its rows overflow it, the list is a whole number of rows tall,
        // so no row is cut through its text at the list's edge: the composer
        // takes the remainder, or, without one, it stays below the list. At
        // its floor the list is whole rows already.
        let row = f32::from(window.pixel_snap(px(self.settings.density.file_row_height())));
        let border = f32::from(window.pixel_snap(px(1.))) * 2.;
        // What is left below the list's last whole row where it has `inner`
        // inside its border, or nothing where its rows do not overflow it.
        let leftover = |inner: f32| {
            if listed && inner > floor - border && inner < row * self.working_rows.len() as f32 {
                (inner - ((inner + 0.01) / row).floor() * row).max(0.)
            } else {
                0.
            }
        };
        let remainder = leftover(available - if composer { composer_height } else { 0. } - border);
        let composer_height = composer_height + if composer { remainder } else { 0. };
        // The Description section is drawn whole and never leaves for room.
        // Where the composer's share is too short for one line of its field,
        // the list gives up the rows that line takes, down to its floor, and
        // where even that leaves too little, the list and the composer scroll
        // together as one area (`scrolls`), the field keeping its preference,
        // and Tab and Shift+Tab reveal each of the composer's stops whole
        // (`working_body`).
        let fitted =
            |height: f32| metrics.description_field(px(height), rows, description_height, window);
        let (composer_height, description) = match composer.then(|| fitted(composer_height)) {
            Some(None) => {
                // As `WorkingFit` counts it, so a form that keeps the
                // heading never scrolls.
                let line = f32::from(metrics.least(rows, section));
                let height = line.min(available - floor);
                // What the list's whole rows leave below them, as above.
                let height = height + leftover(available - height - border);
                match fitted(height) {
                    Some(field) => (height, Some(field)),
                    None => (composer_height, None),
                }
            }
            fitted => (composer_height, fitted.flatten()),
        };
        let scrolls = composer && description.is_none();
        let list_controls = layout.read(cx).row_controls.clone();
        let description = if scrolls {
            Some(description_height)
        } else {
            description
        };
        // Its rows keep their rhythm down to the commit action, a scaled 4 px
        // in a short composer and 0.5 rem in a taller one, rather than the
        // height the composer's share leaves over gathering between the last
        // field and the action. A taller composer with its Description
        // section keeps its share and its description's field grows to take
        // what its rows leave: the field is the composer's one row that reads
        // better taller, and the action stays at the composer's foot. A short
        // composer is drawn no taller than its rows: what it does not use goes
        // back to the list in whole rows, and what is left, less than a row,
        // lies between the list's last row and the composer.
        let content = f32::from(metrics.content(rows, section, description));
        let (drawn, description) = match description {
            // Laid out at its rows' own height in the scrolling body.
            _ if scrolls => (content, description),
            Some(field) if composer && !short => (
                composer_height,
                Some(field + (composer_height - content).max(0.)),
            ),
            _ if composer => (composer_height.min(content), description),
            _ => (composer_height, description),
        };
        let below = if scrolls {
            0.
        } else if !composer {
            remainder
        } else if drawn < composer_height {
            leftover(available - drawn - border)
        } else {
            0.
        };
        // The scrolling body reveals the stop focus moves onto, whole, in
        // the frame that first draws the focus. Not scrolling, it starts at
        // its top the next time it does, revealing the control focus rests
        // on then, as when the window shrinks under the title.
        let reveal = &self.working_body;
        let title_focus = self.commit_title.read(cx).focus_handle(cx);
        let description_focus = self.commit_message.read(cx).focus_handle(cx);
        let commit_focus = reveal.focus("commit-staged", cx);
        let identity_focus = reveal.focus("configure-identity", cx);
        let retry_focus = reveal.focus("retry-commit-draft", cx);
        if scrolls {
            // A composer stop holding focus stays whole as the content
            // around it moves: the list's floor giving up a row as a file is
            // staged, the rows changing in number, or the composer's own
            // rows, as the draft-save note and its Retry come or the long
            // title's note grows. Its stops lie below the list and below
            // the composer's rows above each, which a change moves them by,
            // known before layout: moved by the floor alone, Commit was
            // drawn below the body's foot for a frame as the note and Retry
            // came above it.
            let rows = self.working_rows.len();
            let top = floor + f32::from(metrics.above_fields());
            let field = description.unwrap_or_default();
            let at = [
                top,
                top + f32::from(stops.description),
                top + f32::from(stops.retry) + field,
                top + f32::from(stops.identity) + field,
                floor + content,
            ];
            let stop_focus = [
                &title_focus,
                &description_focus,
                &retry_focus,
                &identity_focus,
                &commit_focus,
            ];
            let focused = reveal
                .holds_focus(window, cx)
                .then(|| {
                    stop_focus
                        .iter()
                        .position(|focus| focus.contains_focused(window, cx))
                })
                .flatten();
            // Each stop's foot, among the same positions: the title's rows
            // and any long-title note end where the Description section
            // begins, the section where the draft-save note and Retry begin,
            // and those where Set your commit identity begins; that and Commit
            // keep their height. Moved by its top alone, the title's stop
            // was drawn past the body's foot for the frame its long-title
            // note came in below its field.
            const FEET: [usize; 5] = [1, 2, 3, 3, 4];
            let moved = layout.update(cx, |layout, _| {
                let last = layout.scrolled_stops.replace(at);
                focused
                    .zip(last)
                    .map_or_else(Default::default, |(index, last)| {
                        crate::focus_reveal::Moved {
                            top: px(at[index] - last[index]),
                            foot: px(at[FEET[index]] - last[FEET[index]]),
                        }
                    })
            });
            let key = {
                use std::hash::{Hash as _, Hasher as _};
                let mut key = std::collections::hash_map::DefaultHasher::new();
                (floor.to_bits(), rows, content.to_bits()).hash(&mut key);
                key.finish()
            };
            reveal.content(key, moved);
            reveal.reveal(window, cx);
        } else {
            layout.update(cx, |layout, _| layout.scrolled_stops = None);
            reveal.reveal_next_focus();
            if reveal.scroll().offset() != Point::default() {
                reveal.scroll().set_offset(Point::default());
            }
        }
        let (gap, padding) = (metrics.gap, metrics.padding);
        let branch = self.composer_branch();
        let summary_length = title_length;
        #[cfg(test)]
        let (composer_probe, rows_probe) = (
            self.working_probe.recorder(|frame| &mut frame.composer),
            self.working_probe.recorder(|frame| &mut frame.rows),
        );
        #[cfg(not(test))]
        let (composer_probe, rows_probe) = (None::<Empty>, None::<Empty>);
        let pane = pane
            .when(below > 0., |el| {
                el.max_h(px(available - if composer { drawn } else { 0. } - below))
            })
            // Scrolling with the composer, the list keeps its floor: whole rows
            // through its first file, its own scroll keeping the selected file
            // in them, and a fade at each edge its rows continue past, since
            // the body's track beside it is the only scrollbar there.
            .when(scrolls, |el| {
                el.flex_none().h(px(floor)).child(floor_fades(
                    self.working_scroll.clone(),
                    layout.read(cx).row_controls.clone(),
                ))
            });
        // Scrolling, the body shows the app's scroll cue: its track in the
        // gutter at its right while what it holds overflows, otherwise the
        // fades, which stop short of the focused stop's ring
        // (`tags::ScrollCue::revealing`). It draws once more where this
        // frame's layout keeps the gutter otherwise than the frame assumed,
        // in either form, so a body that has stopped scrolling gives the
        // gutter back in the next frame.
        // Its track lies over the rows, under the list's ring, so it takes no
        // fill (`ScrollCue::over_rows`).
        let cue = crate::tags::ScrollCue::revealing(reveal, Pixels::ZERO).over_rows();
        let observe = reveal.observe();
        let body = div()
            .on_children_prepainted(cue.observe(move |bounds, window, cx| {
                if scrolls {
                    observe(bounds, window, cx)
                }
            }))
            .id("working-body")
            .debug_selector(|| "working-body".into())
            .w(size.width)
            .h(size.height)
            .flex()
            .flex_col()
            // Tracked in both forms, so where the list and the composer fit,
            // its reach is nothing: tracked only while it scrolled, it kept
            // the last scrolling frame's.
            .track_scroll(reveal.scroll())
            .when(scrolls, |el| el.overflow_y_scroll())
            .map(|el| {
                let pane = native_accessibility::clips_children(pane);
                if scrolls {
                    // Focus moving into the list, onto it or a row's control,
                    // shows the list whole. It reaches into the gutter the
                    // cue keeps at the body's right (`ScrollCue::list`), as
                    // the composer does, so the track lies over its rows'
                    // trailing inset, inside its edge.
                    el.child(
                        div()
                            .flex_none()
                            .flex()
                            .flex_col()
                            .when(gutter > Pixels::ZERO, |el| el.mr(-gutter))
                            .on_children_prepainted(reveal.control(self.file_focus.clone(), gap))
                            .child(pane),
                    )
                } else {
                    el.child(pane)
                }
            })
            .when(composer, |el| {
                el.child(
                    div()
                        .debug_selector(|| "commit-composer".into())
                        .relative()
                        .flex_shrink_0()
                        .min_h_0()
                        .when(!scrolls, |el| el.h(px(drawn)))
                        // Its surface spans the body beneath the cue's track,
                        // and its rows end the gutter less their inset short
                        // of it (`WorkingForms::trailing`).
                        .when(scrolls && gutter > Pixels::ZERO, |el| el.mr(-gutter))
                        .pr(forms.trailing)
                        .flex()
                        .flex_col()
                        // It ends at the body's foot, above the status bar: what
                        // the list's whole rows leave, less than a row, lies
                        // between the list's last row and the composer rather
                        // than below the commit action.
                        .mt_auto()
                        .bg(rgb(p.subtle))
                        .border_t_1()
                        .border_color(rgb(p.border))
                        .children(composer_probe)
                        // The heading leads the composer above its fields, as the
                        // commit action ends it below them; where the body is short
                        // the file list gives up the room for both, down to its
                        // floor, and before the list and the composer would scroll
                        // together the heading leaves (`WorkingFit`), so it never
                        // scrolls with them.
                        .when(fit.heading, |el| {
                            el.child(
                                div()
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_3()
                                    .pt_3()
                                    .when(short, |el| el.pt_2())
                                    .when(forms.heading_icon, |el| {
                                        el.child(
                                            div()
                                                .debug_selector(|| "commit-composer-icon".into())
                                                .child(icon("commit", 16., p.accent)),
                                        )
                                    })
                                    // Truncating, it keeps "Commit to" and cuts the branch.
                                    .child(
                                        div()
                                            .id("commit-composer-heading")
                                            .debug_selector(|| "commit-composer-heading".into())
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(crate::appearance::ui_text(13.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .tooltip({
                                                // Without its badge the heading names the count,
                                                // which a marked tab has none of.
                                                let heading = composer_heading(branch);
                                                let tooltip = if forms.heading_badge || branch.is_none() {
                                                    heading
                                                } else {
                                                    format!("{heading} · {staged} staged")
                                                };
                                                #[cfg(test)]
                                                crate::views::note_control_tooltip(
                                                    &"commit-composer-heading".into(),
                                                    &tooltip,
                                                );
                                                move |window, cx| {
                                                    Tooltip::new(tooltip.clone()).build(window, cx)
                                                }
                                            })
                                            .child(if short {
                                                composer_heading(branch)
                                            } else {
                                                "Create a commit".into()
                                            }),
                                    )
                                    .when(forms.heading_badge, |el| {
                                        el.child(
                                            div()
                                                .debug_selector(|| "commit-staged-count".into())
                                                .flex_shrink_0()
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded(px(4.))
                                                .bg(rgb(if staged > 0 {
                                                    p.added_background
                                                } else {
                                                    p.hover
                                                }))
                                                .text_size(crate::appearance::ui_text(10.))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(rgb(if staged > 0 {
                                                    crate::appearance::labels(cx).added
                                                } else {
                                                    crate::appearance::labels(cx).count
                                                }))
                                                .child(format!("{staged} staged")),
                                        )
                                    }),
                            )
                        })
                        .child(composer_fields(
                            scrolls,
                            div()
                                .id("commit-composer-fields")
                                .debug_selector(|| "commit-composer-fields".into())
                                // In the scrolling body they scroll with it, at
                                // their own height.
                                .when(!scrolls, |el| el.flex_1().min_h_0().overflow_y_scroll())
                                // The gap that followed the heading leads the fields;
                                // without the heading, the composer's own padding does.
                                // The fields end a gap above the commit action, as they
                                // keep it between themselves (`ComposerMetrics::gap`).
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_2()
                                        .px_3()
                                        .pt_2()
                                        .pb_2()
                                        .when(short, |el| {
                                            el.gap(crate::appearance::ui_size(4.))
                                                .pt(crate::appearance::ui_size(4.))
                                                .pb(crate::appearance::ui_size(4.))
                                        })
                                        .when(!fit.heading, |el| {
                                            if short { el.pt_2() } else { el.pt_3() }
                                        })
                                        .when(!short, |el| {
                                            el.child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_1p5()
                                                    .min_w_0()
                                                    .text_size(crate::appearance::ui_text(11.))
                                                    .text_color(rgb(p.muted))
                                                    .child(icon("branch", 13., p.muted))
                                                    // A marked tab's, in the picker's word.
                                                    .child(div().truncate().child(
                                                        branch
                                                            .unwrap_or(crate::branch_actions::UNAVAILABLE)
                                                            .to_owned(),
                                                    )),
                                            )
                                        })
                                        // The title's label, field and any note,
                                        // revealed together when focus moves onto
                                        // the field in the scrolling body.
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .gap_2()
                                                .when(short, |el| {
                                                    el.gap(crate::appearance::ui_size(4.))
                                                })
                                                .when(scrolls, |el| {
                                                    el.on_children_prepainted(
                                                        reveal.control(title_focus.clone(), gap),
                                                    )
                                                })
                                                .child(
                                                    div()
                                                        .debug_selector(|| {
                                                            "commit-title-label".into()
                                                        })
                                                        .flex()
                                                        .items_center()
                                                        .gap_2()
                                                        .text_size(crate::appearance::ui_text(11.))
                                                        .child(
                                                            div()
                                                                .debug_selector(|| {
                                                                    "commit-title-name".into()
                                                                })
                                                                .flex_1()
                                                                .font_weight(FontWeight::MEDIUM)
                                                                .child("Title"),
                                                        )
                                                        .child({
                                                            let counter = div()
                                                                .id("commit-title-counter")
                                                                .debug_selector(|| {
                                                                    "commit-title-counter".into()
                                                                })
                                                                .text_color(rgb(
                                                                    if summary_length > 72 {
                                                                        p.warning
                                                                    } else {
                                                                        p.muted
                                                                    },
                                                                ));
                                                            let whole = SharedString::from(
                                                                title_counter(summary_length),
                                                            );
                                                            if forms.counter_word {
                                                                counter
                                                                    .child(whole)
                                                                    .into_any_element()
                                                            } else {
                                                                // Without "suggested" the counter is read by its whole text.
                                                                crate::views::note_compact_label(
                                                                    "commit-title-counter",
                                                                    &whole,
                                                                );
                                                                native_accessibility::static_text(
                                                                    counter,
                                                                    whole.clone(),
                                                                )
                                                                .tooltip(move |window, cx| {
                                                                    Tooltip::new(whole.clone())
                                                                        .build(window, cx)
                                                                })
                                                                .child(format!(
                                                                    "{summary_length}/72"
                                                                ))
                                                                .into_any_element()
                                                            }
                                                        }),
                                                )
                                                .child(
                                                    div()
                                                        .debug_selector(|| "commit-title".into())
                                                        .child(
                                                            Input::new(&self.commit_title)
                                                                .readonly(busy)
                                                                .aria_label("Commit title")
                                                                .text_size(
                                                                    crate::appearance::ui_text(12.),
                                                                ),
                                                        ),
                                                )
                                                .when(summary_length > 72, |el| {
                                                    el.child(
                                                        div()
                                                            .text_size(crate::appearance::ui_text(
                                                                10.,
                                                            ))
                                                            .text_color(rgb(p.muted))
                                                            .child(LONG_TITLE_NOTE),
                                                    )
                                                }),
                                        )
                                        // Its label, field and hint, always whole
                                        // (`ComposerMetrics::description_field`) and
                                        // revealed together when focus moves onto the
                                        // field in the scrolling body. With nothing
                                        // staged, the hint says why Commit, out of the
                                        // Tab order, is unavailable.
                                        .when_some(description, |el, field| {
                                            el.child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap_2()
                                                    .when(short, |el| {
                                                        el.gap(crate::appearance::ui_size(4.))
                                                    })
                                                    .when(scrolls, |el| {
                                                        el.on_children_prepainted(reveal.control(
                                                            description_focus.clone(),
                                                            gap,
                                                        ))
                                                    })
                                                    .child(
                                                        div()
                                                            .debug_selector(|| {
                                                                "commit-description-label".into()
                                                            })
                                                            .flex()
                                                            .items_center()
                                                            .gap_1()
                                                            .text_size(crate::appearance::ui_text(
                                                                11.,
                                                            ))
                                                            .child(
                                                                div()
                                                                    .debug_selector(|| {
                                                                        "commit-description-name"
                                                                            .into()
                                                                    })
                                                                    .font_weight(FontWeight::MEDIUM)
                                                                    .child("Description"),
                                                            )
                                                            .when(forms.description_note, |el| {
                                                                el.child(
                                                            div()
                                                                .debug_selector(|| {
                                                                    "commit-description-optional"
                                                                        .into()
                                                                })
                                                                .text_color(rgb(p.muted))
                                                                .child("· optional"),
                                                        )
                                                            }),
                                                    )
                                                    // Tab and Shift-Tab move focus on and back, as on every
                                                    // other stop, rather than indenting.
                                                    .child(
                                                        shortcuts::tab_leaves(
                                                            Textarea::new(&self.commit_message)
                                                                .readonly(busy)
                                                                .aria_label(
                                                                    "Commit description, optional",
                                                                )
                                                                .aria_description(
                                                                    self.description_hint(),
                                                                )
                                                                .h(px(field))
                                                                .text_size(
                                                                    crate::appearance::ui_text(12.),
                                                                ),
                                                        )
                                                        .debug_selector(|| {
                                                            "commit-description".into()
                                                        }),
                                                    )
                                                    // The field's accessible description reads this
                                                    // with the field, so its own line stays out of the
                                                    // tree: walking past it does not read it again.
                                                    .child(native_accessibility::hidden_from_assistive_technology(
                                                        div()
                                                            .id("commit-description-hint")
                                                            .debug_selector(|| {
                                                                "commit-description-hint".into()
                                                            })
                                                            .flex()
                                                            .items_center()
                                                            .gap_2()
                                                            .text_size(crate::appearance::ui_text(
                                                                10.,
                                                            ))
                                                            .text_color(rgb(p.muted))
                                                            // Shrinking below its one line, it wraps as `description_section`
                                                            // predicts rather than running past the column at 18 pt.
                                                            .child(div().flex_1().min_w_0().child(
                                                                self.description_hint(),
                                                            )),
                                                        true,
                                                    )),
                                            )
                                        })
                                        .when_some(self.draft_save_error.as_ref(), |el, error| {
                                            el.child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap_1()
                                                    .text_size(crate::appearance::ui_text(10.))
                                                    .text_color(rgb(p.warning))
                                                    .child(DRAFT_SAVE_NOTE)
                                                    .when(scrolls, |el| {
                                                        el.on_children_prepainted(
                                                            reveal
                                                                .control(retry_focus.clone(), gap),
                                                        )
                                                    })
                                                    .child(
                                                        button(
                                                            "retry-commit-draft",
                                                            "Retry saving draft",
                                                            "refresh",
                                                            false,
                                                        )
                                                        .track_focus(&retry_focus)
                                                        .tooltip(error.clone())
                                                        .on_click(cx.listener(
                                                            |this, _, window, cx| {
                                                                this.persist_commit_draft(
                                                                    window, cx,
                                                                )
                                                            },
                                                        )),
                                                    ),
                                            )
                                        })
                                        .when(!identity_ready && shown, |el| {
                                            el.child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .when(scrolls, |el| {
                                                        el.on_children_prepainted(
                                                            reveal.control(
                                                                identity_focus.clone(),
                                                                gap,
                                                            ),
                                                        )
                                                    })
                                                    .child(
                                                        button(
                                                            "configure-identity",
                                                            "Set your commit identity",
                                                            "",
                                                            false,
                                                        )
                                                        .track_focus(&identity_focus)
                                                        .on_click(cx.listener(
                                                            |this, _, window, cx| {
                                                                this.show_settings(window, cx)
                                                            },
                                                        )),
                                                    ),
                                            )
                                        })
                                        .when(conflicted, |el| {
                                            el.child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_2()
                                                    .text_size(crate::appearance::ui_text(11.))
                                                    .text_color(rgb(p.warning))
                                                    .child(icon("file-conflict", 15., p.warning))
                                                    // Shrinking below its one line, it wraps as `composer_rows`
                                                    // predicts rather than running past the column.
                                                    .child(
                                                        div()
                                                            .debug_selector(|| {
                                                                "commit-conflict-note".into()
                                                            })
                                                            .flex_1()
                                                            .min_w_0()
                                                            .child(CONFLICT_NOTE),
                                                    ),
                                            )
                                        }),
                                ),
                        ))
                        .child(
                            div()
                                .flex_shrink_0()
                                .px_3()
                                .pb_3()
                                .when(short, |el| el.pb_2())
                                .children(rows_probe)
                                // Enabled, it is the composer's last stop, revealed
                                // down to the composer's foot in the scrolling body.
                                .when(scrolls, |el| {
                                    el.on_children_prepainted(
                                        reveal.control(commit_focus.clone(), padding),
                                    )
                                })
                                .child({
                                    use crate::native_accessibility::DrawnButton as _;
                                    // Disabled, it rests on the composer's own `subtle` and
                                    // keeps its shape with the toolkit's disabled edge; it is
                                    // as wide as the composer.
                                    let commit = Button::new("commit-staged")
                                        .track_focus(&commit_focus)
                                        .debug_selector(|| "commit-staged".into())
                                        .primary()
                                        .w_full()
                                        .h(crate::appearance::ui_size(36.))
                                        .when(forms.commit_icon, |commit| {
                                            crate::views::note_control_icon(
                                                &"commit-staged".into(),
                                                "icons/commit.svg",
                                            );
                                            commit.icon(
                                                Icon::default()
                                                    .path("icons/commit.svg")
                                                    .size(px(16.)),
                                            )
                                        });
                                    let label = self.commit_action_label(cx);
                                    let detail = branch.map_or_else(
                                        || crate::branch_actions::UNAVAILABLE_REASON.to_owned(),
                                        |branch| format!("Commit staged changes to {branch}"),
                                    );
                                    // While only the rows being prepared refuse it,
                                    // its name and tooltip say why once it dims
                                    // (`rows_wait`); a label that says why it
                                    // cannot commit is its own reason.
                                    let (named, tooltip) = self.rows_wait(
                                        "commit-staged",
                                        !shown || commit_unavailable,
                                        &label,
                                        Some(if forms.compact_commit {
                                            format!("{label} · {detail}")
                                        } else {
                                            detail
                                        }),
                                    );
                                    // Where its label does not fit, it reads "Commit" and
                                    // keeps the label as its name and leading its tooltip.
                                    if forms.compact_commit {
                                        crate::views::compact_control_label(
                                            commit,
                                            "commit-staged",
                                            label,
                                        )
                                        .label("Commit")
                                    } else {
                                        commit.label(label)
                                    }
                                    .when_some(tooltip, |commit, tooltip| {
                                        waiting_tooltip(commit, "commit-staged", tooltip)
                                    })
                                    .when_some(named, |commit, named| {
                                        commit.accessibility_label(named)
                                    })
                                    .map(|commit| {
                                        refusing(
                                            commit,
                                            || "commit-staged".into(),
                                            commit_refused,
                                            commit_disabled,
                                        )
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        // Refused at once, whatever it looks like
                                        // yet, its look showing why at once
                                        // (`refuse_busy_press`).
                                        if this.refuse_busy_press(cx) {
                                            return;
                                        }
                                        let message = this.current_commit_draft(cx).message();
                                        this.write(
                                            WriteCommand::Commit { message },
                                            "Creating commit…",
                                            window,
                                            cx,
                                        );
                                    }))
                                    .node_recorded()
                                }),
                        )
                        // Painted after both fields, it fits their placeholders.
                        .child(placeholder_fit(
                            self.commit_title.clone(),
                            &TITLE_FIT,
                            &[
                                TITLE_PLACEHOLDER,
                                TITLE_SHORT_PLACEHOLDER,
                                TITLE_SHORTEST_PLACEHOLDER,
                            ],
                            12.,
                        ))
                        .child(placeholder_fit(
                            self.commit_message.clone(),
                            &DESCRIPTION_FIT,
                            &[DESCRIPTION_PLACEHOLDER],
                            12.,
                        )),
                )
            });
        // Scrolling, it reports to assistive technology only what it shows
        // (`ScrollCue::list`). Not scrolling, its rows keep the trailing room
        // of a gutter the frame assumed (`WorkingForms::trailing`), so they
        // are laid out at the width they were fitted to, and the body gives
        // that room back in the next frame.
        let edge = list_edge(
            list_controls,
            self.focus_ring.clone(),
            self.file_focus.clone(),
            reveal.scroll().clone(),
        );
        if scrolls {
            cue.list(body, "working-body-scrollbar")
                .child(edge)
                .into_any_element()
        } else {
            body.child(edge).into_any_element()
        }
    }

    /// Whether the row drawn for the file at `index` in `area` tells
    /// assistive technology it is the item focused in the list
    /// (`aria_active_descendant`): the cursor's row, and never a `picture` of
    /// it (`motion::TravellingRow`). The list draws a slot where a flying row
    /// lands, so the copy of the cursor's row would be the only claimant:
    /// GPUI reports that claim as the tree's focus, and adapters read focus
    /// before they check that a node is hidden, so assistive technology
    /// would be given the row at takeoff and again at landing. The row is
    /// met only where it lands.
    pub(super) fn working_row_claims_focus(
        &self,
        index: usize,
        area: ChangeArea,
        picture: bool,
    ) -> bool {
        !picture && self.shown_working_cursor() == Some((index, area))
    }

    fn render_working_row(
        &self,
        position: usize,
        forms: WorkingForms,
        controls: Option<&RowControls>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = palette(cx);
        let busy = self.operation_busy.is_some();
        // A write in flight, or the rows the reads after it are preparing,
        // leave these controls inert at once (`write_from`,
        // `depart_working_row`) and dimmed once the spell looks busy.
        let looks_busy = self.busy_shown();
        // Without the list's `controls` the row is drawn as a picture of
        // itself, the travelling copy of a row in flight
        // (`motion::TravellingRow`): no Tab stop, no click, hover, tooltip or
        // menu, and no `working-stage-…` selector, so a click on it reaches
        // the row the list draws beneath, as one under a still pointer does.
        // Its control's click once unstaged the file a double-click's first
        // press had just staged.
        let picture = controls.is_none();
        // What the rows moved under a pointer that has not moved since it
        // pressed a row's Stage or Unstage or a heading's Stage all or
        // Unstage all shows no hover and no tooltip until it moves
        // (`motion::StillPointer`), nor does a picture.
        let still = picture || self.still_over_moved_rows();
        // A control takes the shared look without its hover fill, and no
        // tooltip at all, not even its confirmation's word: gamma's Stage,
        // confirmed in Staged as its flight began under the pointer, said
        // "Staged" there.
        let feedback = move |mark: copy_feedback::Mark,
                             action: copy_feedback::Action,
                             label: &'static str,
                             tooltip: String,
                             cx: &App| {
            if still {
                copy_feedback::still_feedback_button(mark, action, label, cx).with_variant(
                    if picture {
                        picture_control_variant(p, cx)
                    } else {
                        appearance::still_control_button_variant()
                    },
                )
            } else {
                copy_feedback::feedback_button(mark, action, label, tooltip, cx)
            }
        };
        // The shown rows index the shown status, an earlier one while a
        // newer status's rows are prepared.
        match self.working_rows[position] {
            WorkingRow::Directory(index, _) => {
                let parent = self
                    .shown_working_status()
                    .and_then(|s| s.entries.get(index))
                    .and_then(|entry| entry.path.parent())
                    .filter(|path| !path.as_os_str().is_empty())
                    .map_or("Repository root".into(), |path| path.display().to_string());
                div()
                    .h(px(self.settings.density.file_row_height()))
                    .pl_3()
                    .pr(forms.row_end(window))
                    .flex()
                    .items_center()
                    .text_size(crate::appearance::ui_text(11.))
                    .text_color(rgb(p.muted))
                    .child(parent)
                    .into_any_element()
            }
            WorkingRow::Heading(area, count) => {
                use crate::native_accessibility::DrawnButton as _;
                let staged = area == ChangeArea::Staged;
                let name = if staged { "Staged" } else { "Unstaged" };
                let selector = heading_control(area);
                let (action, detail) = if staged {
                    (
                        "Unstage all",
                        "Remove all staged changes from the next commit",
                    )
                } else {
                    ("Stage all", "Include all local changes in the next commit")
                };
                // Where its label does not fit, the action keeps its icon,
                // with its name and a tooltip that leads with it. Either way
                // it reads Staged or Unstaged where it was touched once Git
                // reports success, in its label's place or in its tooltip.
                let (mark, confirmation) = (
                    confirmations::all(&self.path, area),
                    if staged {
                        confirmations::UNSTAGE_ALL
                    } else {
                        confirmations::STAGE_ALL
                    },
                );
                let origin = mark.clone();
                let confirming = copy_feedback::confirmed(&mark, cx);
                let empty = self.working_heading_empty(count, cx);
                let refused = busy || self.working_paths.pending || empty;
                let unavailable = self.working_heading_unavailable(count, cx);
                // While only the rows being prepared refuse it, its name
                // and tooltip say why once it dims (`rows_wait`).
                let (named, tooltip) = self.rows_wait(
                    selector,
                    empty,
                    action,
                    Some(if forms.section_action_label {
                        detail.into()
                    } else {
                        format!("{action} · {detail}")
                    }),
                );
                let tooltip = tooltip.unwrap_or_default();
                let control = if forms.section_action_label {
                    feedback(mark, confirmation, action, tooltip, cx)
                } else {
                    crate::views::compact_control_name(
                        feedback(mark, confirmation, "", tooltip, cx),
                        selector,
                        action,
                    )
                }
                .when_some(named, |control, named| control.accessibility_label(named));
                let focus = controls.map(|controls| {
                    controls.focus(selector.into(), || RowControl::Heading(area), position, cx)
                });
                // Out of GPUI's order: Tab reaches it by the list's stops
                // (`tab_working_row_stops`), even where the rows hide it.
                let control = control
                    .when_some(focus.as_ref(), |control, focus| control.track_focus(focus))
                    .tab_stop(false);
                let part = move |part: &'static str| -> String {
                    format!(
                        "{}-heading-{part}",
                        if staged { "staged" } else { "unstaged" }
                    )
                };
                div()
                    .debug_selector(move || format!("working-row-{position}"))
                    .w_full()
                    .h(px(self.settings.density.file_row_height()))
                    .pl_3()
                    .pr(forms.row_end(window))
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgb(p.border))
                    .bg(rgb(p.subtle))
                    .when(forms.section_icon, |el| {
                        el.child(div().debug_selector(move || part("icon")).child(icon(
                            if staged { "check" } else { "code" },
                            15.,
                            if staged { p.added } else { p.modified },
                        )))
                    })
                    .when(forms.section_label, |el| {
                        el.child(
                            div()
                                .debug_selector(move || part("label"))
                                .text_size(crate::appearance::ui_text(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(name),
                        )
                    })
                    .child(
                        div()
                            .id(if staged {
                                "staged-heading-count"
                            } else {
                                "unstaged-heading-count"
                            })
                            .debug_selector(move || part("count"))
                            .text_size(crate::appearance::ui_text(11.))
                            .text_color(rgb(p.muted))
                            // Without its name the count says what it counts.
                            .when(!forms.section_label, |el| {
                                el.tooltip(move |window, cx| {
                                    Tooltip::new(format!("{name} · {count}")).build(window, cx)
                                })
                            })
                            .child(count.to_string()),
                    )
                    .child(
                        RowControls::control(
                            controls,
                            refusing(control, move || selector.into(), refused, unavailable)
                                .debug_selector(move || selector.into())
                                // The write it confirms leaves it unavailable, its
                                // group emptied or the reads after it pending, and
                                // the disabled colour, 3.19:1 on Midnight's heading,
                                // read the outcome as unavailable. While it confirms
                                // it stays inert in its resting label colour, which
                                // the kit replays over the disabled one; it takes
                                // the disabled look when the confirmation ends.
                                .when(unavailable && confirming, |control| {
                                    control.text_color(rgb(p.control_label()))
                                })
                                .on_click(cx.listener(
                                    move |this, event: &ClickEvent, window, cx| {
                                        // A double-click's second press, on the
                                        // heading the first press's write moved
                                        // under the pointer, does nothing, as on
                                        // a row's control: it wrote every
                                        // remaining file, which nobody aimed at.
                                        if event.click_count() > 1 && this.still_over_moved_rows() {
                                            return;
                                        }
                                        // Refused at once while a write runs or the
                                        // rows it counts are prepared, whatever it
                                        // looks like yet, its look showing why at
                                        // once (`refuse_busy_press`).
                                        if this.refuse_busy_press(cx) {
                                            return;
                                        }
                                        // The rows this write lands put a
                                        // file's control under the pointer,
                                        // where a double-click's second press
                                        // unstaged that file.
                                        this.hold_still_pointer(position, window);
                                        this.write_from(
                                            if staged {
                                                WriteCommand::UnstageAll
                                            } else {
                                                WriteCommand::StageAll
                                            },
                                            if staged {
                                                "Unstaging files…"
                                            } else {
                                                "Staging files…"
                                            },
                                            Some(origin.clone().into()),
                                            window,
                                            cx,
                                        )
                                    },
                                ))
                                .node_recorded(),
                            focus.as_ref(),
                            window,
                        )
                        // Its wrapper, which records the ring the floor's
                        // fades stop short of, stands at the row's end.
                        .ml_auto(),
                    )
                    .into_any_element()
            }
            WorkingRow::File(index, area) => {
                use crate::native_accessibility::DrawnButton as _;
                let entry = &self.shown_working_status().unwrap().entries[index];
                let paths = entry.paths();
                let row_path = entry.path.clone();
                let ignore_entry = std::rc::Rc::new(entry.clone());
                let menu_entry = ignore_entry.clone();
                let ignore_owner = cx.entity().downgrade();
                let ignore_repository = self.path.clone();
                let name = entry
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let parent = entry
                    .path
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .filter(|p| !p.is_empty())
                    .unwrap_or("Repository root".into());
                let active = self.working_is_selected(index, area);
                let staged = area == ChangeArea::Staged;
                let path_detail = entry.original_path.as_ref().map_or_else(
                    || entry.path.display().to_string(),
                    |original| format!("{} → {}", original.display(), entry.path.display()),
                );
                let stage_label = format!(
                    "{} {}",
                    if staged { "Unstage" } else { "Stage" },
                    entry.path.display()
                );
                let change = if staged { entry.staged } else { entry.unstaged };
                let (symbol, status_label, color, background) = if entry.conflicted {
                    ("file-conflict", "Conflict", p.warning, p.hover)
                } else if entry.untracked || change == Some(ChangeStatus::Added) {
                    ("file-added", "New", p.added, p.added_background)
                } else {
                    match change.unwrap_or(ChangeStatus::Modified) {
                        ChangeStatus::Added => ("file-added", "New", p.added, p.added_background),
                        ChangeStatus::Modified => {
                            ("file-modified", "Modified", p.modified, p.hover)
                        }
                        ChangeStatus::Deleted => {
                            ("file-deleted", "Deleted", p.removed, p.removed_background)
                        }
                        ChangeStatus::Renamed => ("file-renamed", "Renamed", p.renamed, p.hover),
                        ChangeStatus::TypeChanged => ("file-type", "Type", p.warning, p.hover),
                    }
                };
                // Without its status word the row's tooltip names the status.
                let path_detail = if forms.file_status {
                    path_detail
                } else {
                    format!("{path_detail} · {status_label}")
                };
                // Its Stage or Unstage, where it draws one, tracks the
                // handle the list keeps for it, whose ring the floor's fades
                // stop short of while it holds focus.
                let row_id = self.working_row_key(index, area);
                let stage_focus = controls.filter(|_| forms.file_action).map(|controls| {
                    controls.focus(
                        row_id.clone(),
                        || RowControl::File(entry.path.clone(), area),
                        position,
                        cx,
                    )
                });
                let row = div()
                    .id(row_id)
                    .role(Role::ListBoxOption)
                    .aria_label(format!(
                        "{} · {} · {}",
                        entry.path.display(),
                        status_label,
                        if staged { "Staged" } else { "Unstaged" }
                    ))
                    .debug_selector(move || format!("working-row-{position}"))
                    .aria_selected(active)
                    .when(self.working_row_claims_focus(index, area, picture), |row| {
                        row.aria_active_descendant()
                    })
                    .h(px(self.settings.density.file_row_height()))
                    .w_full()
                    // Its tile or glyph in the list's column, where the headings'
                    // icons and the header's stand: the rows' leading inset, the
                    // row's 2 px marker out of it (DESIGN.md, "One left edge"). A
                    // fixed 10 px stood the tile 2.25 px past the headings' icons at
                    // 13 pt.
                    .pl(crate::appearance::ui_size(crate::appearance::PANE_INSET) - px(2.))
                    .pr(forms.row_end(window))
                    .flex()
                    .items_center()
                    .map(|row| {
                        if forms.file_glyph_close {
                            row.gap_1()
                        } else {
                            row.gap_2()
                        }
                    })
                    .bg(rgb(if active { p.selected } else { p.panel }))
                    .border_l_2()
                    .border_color(if active {
                        self.selected_row_marker(cx)
                    } else {
                        rgba(0)
                    })
                    .when(!still, |row| {
                        row.hover(move |s| s.bg(rgb(p.row_hover(active))))
                    })
                    .when(!picture, |row| {
                        row.active(move |s| s.bg(rgb(p.selected))).cursor_pointer()
                    })
                    // Where the name needs its room, the tile gives way to
                    // its bare glyph, and then the glyph leaves.
                    .when(forms.file_tile, |row| {
                        row.child(
                            div()
                                .debug_selector(move || format!("working-tile-{position}"))
                                .size(px(26.))
                                .flex_shrink_0()
                                .rounded(px(7.))
                                .bg(rgb(background))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon(symbol, 16., color)),
                        )
                    })
                    .when(forms.file_glyph, |row| {
                        row.child(
                            div()
                                .debug_selector(move || format!("working-glyph-{position}"))
                                .flex_shrink_0()
                                .child(icon(symbol, 16., color)),
                        )
                    })
                    .child(
                        div()
                            .id(("working-path", position))
                            .debug_selector(move || format!("working-path-{position}"))
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            // GPUI's existing tooltip can outlive an occluding
                            // menu. Removing its builder clears that tooltip.
                            .when(
                                !window
                                    .context_stack()
                                    .iter()
                                    .any(|context| context.contains("PopupMenu"))
                                    && !still,
                                |path| {
                                    path.tooltip(move |window, cx| {
                                        let text = path_detail.clone();
                                        Tooltip::element(move |window, _| {
                                            div()
                                                .id("working-path-tooltip")
                                                .debug_selector(|| "working-path-tooltip".into())
                                                .max_w(
                                                    px(420.).min(
                                                        window.viewport_size().width - px(80.),
                                                    ),
                                                )
                                                .max_h(window.viewport_size().height * 0.5)
                                                .overflow_hidden()
                                                .whitespace_normal()
                                                .child(text.clone())
                                        })
                                        .build(window, cx)
                                    })
                                },
                            )
                            // Each line fits whole or ends in an ellipsis
                            // inside the row, never cut by half a pixel.
                            .child(
                                crate::views::ellipsis_end(
                                    div()
                                        .debug_selector(move || format!("working-name-{position}")),
                                )
                                .text_size(crate::appearance::ui_text(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .child(name),
                            )
                            .when(forms.file_folder, |path| {
                                path.child(
                                    crate::views::ellipsis_end(div().debug_selector(move || {
                                        format!("working-folder-{position}")
                                    }))
                                    .text_size(crate::appearance::ui_text(10.))
                                    .text_color(rgb(p.muted))
                                    .child(parent),
                                )
                            }),
                    )
                    .when(forms.file_status, |row| {
                        row.child(
                            div()
                                .debug_selector(move || format!("working-status-{position}"))
                                .flex_shrink_0()
                                .text_size(crate::appearance::ui_text(10.))
                                .text_color(rgb(p.text))
                                .child(status_label),
                        )
                    })
                    .when(forms.file_action, |row| {
                        // Icon-only, it reads its confirmation in its tooltip
                        // once the Stage or Unstage that moved its file here
                        // succeeded, and keeps its name.
                        row.child(RowControls::control(
                            controls,
                            feedback(
                                confirmations::file(&self.path, &entry.path, area),
                                if staged {
                                    confirmations::STAGED_FILE
                                } else {
                                    confirmations::UNSTAGED_FILE
                                },
                                "",
                                stage_label.clone(),
                                cx,
                            )
                            .when_some(stage_focus.as_ref(), |control, focus| {
                                control.track_focus(focus)
                            })
                            .accessibility_label(stage_label)
                            // No row's control is in GPUI's order: the list
                            // is one Tab stop and Tab from it reaches only
                            // the selected row's (`tab_working_row_stops`);
                            // the picture's own handle took focus and lost
                            // it as the row landed. A click still reaches
                            // each, and its node stays in the tree.
                            .tab_stop(false)
                            .map(|control| {
                                if picture {
                                    return control;
                                }
                                refusing(
                                    control.debug_selector(move || {
                                        format!("working-stage-{position}")
                                    }),
                                    move || format!("working-stage-{position}"),
                                    busy,
                                    busy && looks_busy,
                                )
                                .on_click(cx.listener(
                                    move |this, event: &ClickEvent, window, cx| {
                                        cx.stop_propagation();
                                        // A double-click's second press, on
                                        // the row the first press's write
                                        // moved under the pointer, does
                                        // nothing: it staged a file nobody
                                        // aimed at, which is worse than a
                                        // lost press.
                                        if event.click_count() > 1 && this.still_over_moved_rows() {
                                            return;
                                        }
                                        if this.operation_busy.is_none() {
                                            this.hold_still_pointer(position, window);
                                        }
                                        this.depart_working_row(position, window, cx);
                                        this.stage_working_paths(
                                            paths.clone(),
                                            &row_path,
                                            staged,
                                            window,
                                            cx,
                                        );
                                    },
                                ))
                            })
                            .node_recorded(),
                            stage_focus.as_ref(),
                            window,
                        ))
                    });
                if picture {
                    return row.into_any_element();
                }
                row.on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    // Command-click (Ctrl-click on Linux, as Select All
                    // is Ctrl-A there; it read Super) toggles the row in
                    // the multiselection and Shift-click extends it to
                    // the row. Both only select, as the arrows do: they
                    // once chose the file too, so a conflicted one's
                    // draft took focus from the list being selected in.
                    let modifiers = event.modifiers();
                    let (toggle, range) = (modifiers.secondary(), modifiers.shift);
                    // One the list refuses chooses and selects nothing,
                    // its busy look saying why at once
                    // (`refuse_working_press`).
                    this.refuse_working_press(cx);
                    this.choose_working_selection(index, area, toggle, range, window, cx);
                    // A plain click on the row chooses its file; one the
                    // list refuses chooses nothing
                    // (`activate_working_preview`).
                    if !toggle && !range && this.working_selected == Some((index, area)) {
                        this.activate_working_preview(window, cx);
                    }
                }))
                .app_context_menu(move |menu, _, cx| {
                    // Read as it is built: a write's start or end builds
                    // the open menu again. Opened on rows the filter is
                    // replacing, it refuses its items until the new rows
                    // land and build it again (`waits_while`), as a
                    // click on those rows refuses, each item saying why.
                    let (wait, waiting) =
                        ignore_owner
                            .upgrade()
                            .map_or((None, Some(REFRESHING_ROWS)), |owner| {
                                let owner = owner.read(cx);
                                (owner.menu_wait(), owner.working_rows_wait())
                            });
                    working_row_menu(
                        menu,
                        ignore_owner.clone(),
                        ignore_repository.clone(),
                        MenuRow {
                            entry: (*ignore_entry).clone(),
                            staged,
                        },
                        wait,
                        waiting,
                    )
                })
                // A file whose status changes under its open menu, as
                // one added outside the app, builds the menu again; a
                // write's start or end, and the reload or status read it
                // asks for landing (`write_unsettled`), build it again
                // through `follows_writes` alone, once for each. While
                // the rows of a newer status or of the filter's query
                // are prepared, the rows drawn are the previous ones,
                // and the menu waits for the new ones before it is
                // built again, but for a write that starts or ends
                // meanwhile, which builds it refused, and for the
                // reason its refused items state changing.
                .built_from(menu_entry)
                .follows_writes(cx.entity().downgrade())
                .waits_while(GitTurtle::working_rows_wait)
                .into_any_element()
            }
        }
    }

    /// Opens the Working Changes cursor row's menu beside the row, from
    /// Shift-F10 or the Menu key: the menu a right-click on the row opens.
    fn open_working_row_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.file_menu.is_some() || window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        let Some((index, area)) = self.working_selected else {
            return;
        };
        let Some(row) = self
            .actionable_working_rows()
            .iter()
            .position(|row| matches!(row, WorkingRow::File(i, a) if (*i, *a) == (index, area)))
        else {
            return;
        };
        let Some(entry) = self
            .work_status
            .as_ref()
            .and_then(|status| status.entries.get(index))
            .cloned()
        else {
            return;
        };
        // Drawn beside the row where the list lays it out, which installed
        // rows move as rows come or go before it (`follow_working_menu`).
        let anchor = native_accessibility::ListRow::new(
            &self.working_scroll,
            row,
            window.pixel_snap(px(self.settings.density.file_row_height())),
            appearance::ui_size(appearance::PANE_INSET),
        );
        // Its items are built from the row's file as the menu finds it each
        // time it builds them, as it opens, as rows land and as a write
        // starts or ends (`follow_working_menu`), never from a copy taken as
        // it opened.
        self.file_menu_row = Some(WorkingMenu {
            target: MenuRow {
                entry,
                staged: area == ChangeArea::Staged,
            },
            wait: None,
            waiting: None,
        });
        let Some(items) = self.working_menu_items(cx) else {
            return;
        };
        self.file_menu = Some(
            native_accessibility::KeyboardMenu::open(
                anchor,
                self.file_focus.clone(),
                items,
                Self::dismiss_file_menu,
                window,
                cx,
            )
            .followed_by(
                Self::follow_working_menu,
                Self::working_rows_wait,
                self,
                window,
                cx,
            ),
        );
        cx.notify();
    }

    /// The items of the Working Changes row's keyboard menu
    /// (`file_menu_row`), from its file as the menu last found it, refusing
    /// writes as the window does now, and everything while its rows are
    /// prepared.
    pub(super) fn working_menu_items(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<impl FnOnce(PopupMenu) -> PopupMenu + 'static> {
        let (wait, waiting) = (self.menu_wait(), self.working_rows_wait());
        let (owner, repository) = (cx.entity().downgrade(), self.path.clone());
        let followed = self.file_menu_row.as_mut()?;
        (followed.wait, followed.waiting) = (wait, waiting);
        let target = followed.target.clone();
        Some(move |menu| working_row_menu(menu, owner, repository, target, wait, waiting))
    }

    /// Why a Working row's menu refuses every item while the rows are
    /// prepared, which each item states, or `None` while none are: the write
    /// that runs, by the label it runs under, as Fetch, Pull and Push name it
    /// while it runs, once it looks busy ([`Self::operation_shown`]), and
    /// otherwise, as before then, what the rows are for ([`FILTERING_ROWS`],
    /// [`REFRESHING_ROWS`]). Every item once said "Refreshing…", for a
    /// filter's rows too, and through a write that started as the rows were
    /// prepared; later, a right-click on rows being prepared named a write
    /// as it started, inside the [`BUSY_LOOK_DELAY`] in which a write that
    /// lands sooner shows none of its busy look. The menus follow the reason
    /// as it changes, as the busy look shows (`PointerMenu::waits_while`,
    /// `KeyboardMenu::followed_by`).
    pub(super) fn working_rows_wait(&self) -> Option<&'static str> {
        use crate::working_selection::Preparation;
        self.working_paths.pending.then(|| {
            self.operation_shown()
                .unwrap_or(match self.working_paths.preparation {
                    Preparation::Filter => FILTERING_ROWS,
                    Preparation::Status => REFRESHING_ROWS,
                })
        })
    }

    /// Why the Working rows being prepared refuse the controls that act on
    /// the files they list, Stage all and Unstage all, the selection's Stage
    /// and Unstage, and Commit, where nothing else refuses them: a newer
    /// status's rows ([`REFRESHING_ROWS_REASON`]) or the filter's
    /// ([`FILTERING_ROWS_REASON`]). `None` while a write runs, whose progress
    /// label and Cancel say why, or while no rows are prepared.
    pub(super) fn rows_reason(&self) -> Option<&'static str> {
        use crate::working_selection::Preparation;
        (self.working_paths.pending && self.operation_busy.is_none()).then_some(
            match self.working_paths.preparation {
                Preparation::Filter => FILTERING_ROWS_REASON,
                Preparation::Status => REFRESHING_ROWS_REASON,
            },
        )
    }

    /// What a control the rows being prepared refuse says, `name` and
    /// `tooltip` at rest, unless it is refused for a reason of its own
    /// (`own`) that their landing would not end: while only the rows refuse
    /// it, once it dims with the busy look, an accessible name that says
    /// why ("Stage all, unavailable. Wait for the file list to refresh"),
    /// and a tooltip that adds why after what it says, or is the reason
    /// where it says nothing at rest; otherwise no name of its own and its
    /// tooltip at rest. Both change in place, moving nothing. Through a
    /// quiet refresh's preparation a refused press once showed these
    /// controls dimmed with nothing to say why: the header's "Refreshing…"
    /// follows a status read alone, and a line there would move the header
    /// on every passive refresh. The name once came at once, so every
    /// watcher-driven refresh, however short, renamed a focused Commit
    /// "Commit 2 files, unavailable. …" and back; a refresh shorter than
    /// the look's delay now changes no word, and a press the list or the
    /// control refuses shows the look, and the name with it, at once
    /// ([`Self::refuse_busy_press`]).
    /// `key` names the control for tests (`refusals::waiting`).
    pub(super) fn rows_wait(
        &self,
        key: &'static str,
        own: bool,
        name: &str,
        tooltip: Option<String>,
    ) -> (Option<SharedString>, Option<String>) {
        let reason = self.rows_reason().filter(|_| !own && self.busy_shown());
        let named =
            reason.map(|reason| SharedString::from(format!("{name}, unavailable. {reason}")));
        let tooltip = match reason {
            Some(reason) => {
                Some(tooltip.map_or(reason.to_owned(), |tooltip| format!("{tooltip} · {reason}")))
            }
            None => tooltip,
        };
        #[cfg(test)]
        refusals::record_waiting(key, named.clone(), tooltip.clone());
        #[cfg(not(test))]
        let _ = key;
        (named, tooltip)
    }

    /// Closes the file list's keyboard menu. A press outside it leaves focus
    /// in the menu, so focus returns to the list; an item or Escape has moved
    /// it already, to the list or to the dialog the item opened.
    pub(super) fn dismiss_file_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.file_menu_row = None;
        if let Some(menu) = self.file_menu.take() {
            if menu.has_focus(window, cx) || window.focused(cx).is_none() {
                window.focus(&self.file_focus, cx);
            }
            cx.notify();
        }
    }

    /// The Git action bar. `compact`, in a window too short for its full form
    /// (`views::Chrome`), it is one row of 28 px controls: the branch picker
    /// and sync state give up their width first, and Fetch, Pull, Push and
    /// Targets keep only their icons, their accessible names, and tooltips
    /// that lead with those names. Their actions, states and order are the
    /// full form's.
    /// `edge_form` is how the repository edge is drawn
    /// (`views::GitTurtle::edge_form`); the compact form draws its barest
    /// form, its shortest line or, where even that would be cut, its glyph,
    /// and a detached HEAD's picker names the commit, or, where its line
    /// would be cut, says "Detached" in the line's place. An action drawn as its
    /// icon keeps its label as its accessible name and tooltip, and a line
    /// without its caption, or a glyph, keeps the whole line as its name and
    /// in its tooltip.
    pub(super) fn render_git_actions(
        &self,
        compact: bool,
        edge_form: crate::empty_state::EdgeForm,
        sync: crate::views::SyncFit,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sync_form = sync.form;
        let p = palette(cx);
        let busy = self.operation_busy.is_some();
        // A write in flight leaves the actions inert at once (`write_from`,
        // `run_edge_action`) and dimmed once its spell looks busy.
        let looks_busy = self.operation_shown().is_some();
        let detached = self.header_status().is_none_or(|s| s.branch.is_none());
        let marked = self.shown_tab_unavailable();
        // Where the compact bar has no room for the edge's line, it draws
        // the line's glyph, and a detached HEAD's picker says the line's
        // word, the line leaving, so the bar draws one commit glyph.
        let narrow = compact && edge_form.glyph;
        let branch_picker = self.render_branch_picker(compact, narrow, sync.picker, cx);
        // The bar's text, built only when what it is made of changes
        // ([`GitActionsInputs`]), and what else the frame reads of Targets'
        // fields, borrowed from their text: Fetch's, Pull's and Push's
        // confirmations for the target each acts on.
        let (text, marks, remote_set, remote_branch_set, branch_empty) = {
            let inputs = self.git_actions_inputs(cx);
            let text = GIT_ACTIONS_TEXT.with_borrow_mut(|cache| {
                cache
                    .get_where(
                        |kept| *kept == inputs,
                        || inputs.owned(),
                        |inputs| Rc::new(GitActionsText::build(inputs, self.repository_edge())),
                    )
                    .clone()
            });
            let branch = (self.header_status())
                .and_then(|status| status.branch.as_deref())
                .unwrap_or("Detached HEAD");
            let (remote, remote_branch) = (&*inputs.remote, &*inputs.remote_branch);
            let marks = [
                confirmations::sync("fetch", &self.path, &[remote]),
                confirmations::sync("pull", &self.path, &[remote, remote_branch]),
                confirmations::sync("push", &self.path, &[branch, remote, remote_branch]),
            ];
            (
                text,
                marks,
                inputs.push_url.is_some(),
                !remote_branch.is_empty(),
                field_text(self.branch_name.read(cx)).is_empty(),
            )
        };
        // A repository at an edge (detached HEAD, no remote, no commits yet
        // or no upstream) says so in the counts' place, and its next action, if any,
        // joins the actions (`empty_state`).
        let edge = text.edge.as_ref();
        let glyph = edge.and_then(|edge| edge.glyph).filter(|_| narrow);
        let line_left = narrow && glyph.is_none();
        // The edge's action joins the actions, after Push and before
        // Targets, so in a half tile it wraps with Targets rather than
        // pushing Push to a row of its own.
        let edge_action =
            edge.and_then(|edge| edge.action).map(|action| {
                let labelled = edge_form.labelled && !compact;
                let control = if labelled {
                    button(action.id, action.label, action.symbol, false).secondary()
                } else {
                    crate::views::compact_control_name(
                        button(action.id, "", action.symbol, false).secondary(),
                        action.id,
                        action.label,
                    )
                    .tooltip(action.label)
                };
                refusing(control, move || action.id.into(), busy, looks_busy)
                    .debug_selector(move || action.id.into())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.run_edge_action(action, window, cx)
                    }))
            });
        // A line that leaves for the picker is not built at all.
        let shown = self.header_status().filter(|_| !line_left);
        let sync_state = shown.and_then(|status| {
            if let Some(edge) = edge {
                let caption = edge_form.caption && !compact;
                return Some(
                    div()
                        .px_2()
                        .when(compact, |el| el.min_w_0())
                        .child(
                            EmptyState::new(
                                "branch-edge",
                                if compact {
                                    edge.compact_line.clone()
                                } else if caption {
                                    edge.line.clone()
                                } else {
                                    edge.short_line.clone()
                                },
                            )
                            .named(edge.line.clone())
                            .tooltip(edge.explanation.clone())
                            .glyph(glyph)
                            .truncating(compact)
                            .inline(cx),
                        )
                        .into_any_element(),
                );
            }
            // The counts are always whole, in the form that fits
            // (`GitTurtle::sync_fit`), and always the state's accessible
            // name; the compact bar's tooltip leads with them too.
            let sync = text.sync.as_ref()?;
            let tooltip = if compact {
                sync.compact_tooltip.clone()
            } else {
                sync.tooltip.clone()
            };
            #[cfg(test)]
            crate::views::note_control_tooltip(&"branch-sync-state".into(), &tooltip);
            let count = |symbol: &'static str, text: &Count, word: &'static str, color: u32| {
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_1()
                    .text_color(rgb(color))
                    .child(icon(symbol, 12., color))
                    .when(sync_form != crate::views::SyncForm::Icons, |el| {
                        el.child(
                            div()
                                .debug_selector(move || format!("branch-{word}-count"))
                                .child(if sync_form == crate::views::SyncForm::Words {
                                    text.words.clone()
                                } else {
                                    text.number.clone()
                                }),
                        )
                    })
            };
            let state = div()
                .id("branch-sync-state")
                .debug_selector(|| "branch-sync-state".into())
                .flex()
                .flex_shrink_0()
                .items_center()
                .gap_2()
                .px_2()
                .text_size(crate::appearance::ui_text(11.))
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                .when(status.upstream.is_some(), |el| {
                    el.child(count(
                        "push",
                        &sync.ahead,
                        "ahead",
                        if status.ahead > 0 { p.added } else { p.muted },
                    ))
                    .child(count(
                        "pull",
                        &sync.behind,
                        "behind",
                        if status.behind > 0 {
                            p.modified
                        } else {
                            p.muted
                        },
                    ))
                });
            Some(
                crate::native_accessibility::static_text(state, sync.summary.clone())
                    .into_any_element(),
            )
        });
        let operation_state = text.operation.clone().map(|operation| {
            div()
                .debug_selector(|| "git-operation-state".into())
                .px_2()
                .py_1()
                .rounded(px(5.))
                .bg(rgb(p.hover))
                .text_size(crate::appearance::ui_text(11.))
                .when(compact, |el| el.min_w_0().truncate())
                .text_color(rgb(p.warning))
                .child(operation)
        });
        let actions = [("fetch", "Fetch"), ("pull", "Pull"), ("push", "Push")]
            .into_iter()
            .zip(&text.actions)
            .zip(marks)
            .map(|(((id, label), action), mark)| {
                // A marked tab's repository is not the one Targets name.
                let missing =
                    marked || !remote_set || (id != "fetch" && (!remote_branch_set || detached));
                let unavailable = looks_busy || missing;
                // Each reads its confirmation where it was touched once Git
                // reports success, for the target it acted on: labelled in its
                // label's place, as an icon in its tooltip. Its name stays.
                let confirmation = match id {
                    "fetch" => confirmations::FETCH,
                    "pull" => confirmations::PULL,
                    _ => confirmations::PUSH,
                };
                #[cfg(test)]
                crate::views::note_control_tooltip(
                    &id.into(),
                    if compact {
                        &action.compact_tooltip
                    } else {
                        &action.tooltip
                    },
                );
                // A surface fills only the action its state leads to: in
                // Working Changes that is Commit, so Push there is the
                // bordered secondary in the filled one's bounds
                // (`tags::bordered_in_place`), and fills elsewhere.
                let push = (id == "push").then_some(self.mode != WorkspaceMode::Working);
                let control = if compact {
                    crate::views::compact_control_label(
                        copy_feedback::feedback_button(
                            mark,
                            confirmation,
                            "",
                            action.compact_tooltip.clone(),
                            cx,
                        ),
                        id,
                        action.name.clone(),
                    )
                    .map(|button| match push {
                        Some(true) => button.primary(),
                        Some(false) => crate::tags::bordered_in_place(
                            button,
                            appearance::ui_size(crate::ICON_INSET),
                        ),
                        None => button.secondary(),
                    })
                } else {
                    // Its rest width decides where the bar wraps, so the
                    // past tense takes its icon's and label's room alone.
                    copy_feedback::confirming(
                        action_frame(id),
                        rems(0.5),
                        copy_feedback::Fit::Within,
                        mark,
                        confirmation,
                        label,
                        action.tooltip.clone(),
                        cx,
                    )
                    .map(|button| match push {
                        Some(true) => button.primary(),
                        Some(false) => crate::tags::bordered_in_place(button, action_inset()),
                        None => button,
                    })
                };
                // Disabled, each rests on the neutral `subtle`, 1.046:1 off
                // Midnight's toolbar, and keeps its shape with the toolkit's
                // disabled edge, laid over its bounds.
                refusing(control, move || id.into(), busy || missing, unavailable)
                    .debug_selector(move || id.into())
                    .accessibility_label(action.name.clone())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let remote = this.remote_name.read(cx).value().trim().to_owned();
                        let branch = this.remote_branch.read(cx).value().trim().to_owned();
                        let local_branch = this
                            .work_status
                            .as_ref()
                            .and_then(|s| s.branch.clone())
                            .unwrap_or_default();
                        let target = match id {
                            "fetch" => vec![remote.as_str()],
                            "pull" => vec![remote.as_str(), branch.as_str()],
                            _ => vec![local_branch.as_str(), remote.as_str(), branch.as_str()],
                        };
                        let origin = confirmations::sync(id, &this.path, &target);
                        let command = match id {
                            "fetch" => WriteCommand::Fetch { remote },
                            "pull" => WriteCommand::Pull { remote, branch },
                            _ => WriteCommand::Push {
                                remote,
                                local_branch,
                                remote_branch: branch,
                            },
                        };
                        this.write_from(
                            command,
                            match id {
                                "fetch" => "Fetching remote…",
                                "pull" => "Pulling fast-forward…",
                                _ => "Pushing branch…",
                            },
                            Some(origin.into()),
                            window,
                            cx,
                        );
                    }))
            });
        let open = self.git_actions_open;
        let targets = if compact {
            crate::views::compact_control_name(
                button("git-actions-toggle", "", "sliders", open),
                "git-actions-toggle",
                "Targets",
            )
            .tooltip(if open {
                "Targets · Hide branch and remote targets"
            } else {
                "Targets · Show branch and remote targets"
            })
        } else {
            button("git-actions-toggle", "Targets", "sliders", open).tooltip(if open {
                "Hide branch and remote targets"
            } else {
                "Show branch and remote targets"
            })
        }
        .debug_selector(|| "git-actions-toggle".into())
        .toggled(open)
        .on_click(cx.listener(|this, _, _, cx| {
            this.git_actions_open = !this.git_actions_open;
            cx.notify();
        }));
        let row = if compact {
            // The branch picker and sync state share a group that wraps as if
            // it had no width, so the row breaks only when the actions alone
            // overflow it; the group then gives up its room. The header's
            // compact row keeps the room a focused control's ring takes above
            // these, so this row keeps it only below them.
            div()
                .px_3()
                .pb(appearance::button_ring_room(cx))
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(branch_picker)
                        .children(sync_state)
                        .children(operation_state),
                )
                .children(actions)
                .children(edge_action)
                .child(targets)
        } else {
            div()
                .min_h(crate::appearance::ui_size(46.))
                .px_3()
                .py_1()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(branch_picker)
                .children(sync_state)
                .children(operation_state)
                .child(div().flex_1())
                .children(actions)
                .children(edge_action)
                .child(targets)
        };
        div()
            .debug_selector(|| "git-actions".into())
            .flex()
            .flex_col()
            .flex_shrink_0()
            .bg(rgb(p.panel))
            .border_b_1()
            .border_color(rgb(p.border))
            .child(row)
            .when(self.git_actions_open, |el| {
                #[cfg(test)]
                TARGETS_DESTINATION.set(Some(text.destination.clone()));
                el.child(
                    div()
                        .px_4()
                        .pb_3()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .items_end()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(action_field_label(
                                            "Find or create a branch",
                                            p.muted,
                                        ))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_1()
                                                .child(
                                                    div().w(appearance::ui_size(170.)).child(
                                                        refusing_input(
                                                            Input::new(&self.branch_name)
                                                                .aria_label(
                                                                    "Branch to switch to or create",
                                                                )
                                                                .text_size(
                                                                    crate::appearance::ui_text(12.),
                                                                ),
                                                            || "branch-name".into(),
                                                            busy,
                                                            looks_busy,
                                                        ),
                                                    ),
                                                )
                                                // Each reads Switched or Created where it was
                                                // touched once Git reports success, though the
                                                // name field it read has emptied.
                                                .child(
                                                    copy_feedback::feedback_button(
                                                        confirmations::branch_form(
                                                            "checkout-branch",
                                                            &self.path,
                                                        ),
                                                        confirmations::SWITCH,
                                                        "Switch",
                                                        "Switch to the named local branch",
                                                        cx,
                                                    )
                                                    .map(|switch| {
                                                        refusing(
                                                            switch,
                                                            || "checkout-branch".into(),
                                                            busy || branch_empty,
                                                            looks_busy || branch_empty,
                                                        )
                                                    })
                                                    .on_click(cx.listener(|this, _, window, cx| {
                                                        let branch = this
                                                            .branch_name
                                                            .read(cx)
                                                            .value()
                                                            .trim()
                                                            .to_owned();
                                                        let origin = confirmations::branch_form(
                                                            "checkout-branch",
                                                            &this.path,
                                                        );
                                                        this.write_from(
                                                            WriteCommand::Checkout { branch },
                                                            "Switching branch…",
                                                            Some(origin.into()),
                                                            window,
                                                            cx,
                                                        );
                                                    })),
                                                )
                                                .child(
                                                    copy_feedback::feedback_button(
                                                        confirmations::branch_form(
                                                            "create-branch",
                                                            &self.path,
                                                        ),
                                                        confirmations::CREATE,
                                                        "Create",
                                                        "Create and switch to a branch from HEAD",
                                                        cx,
                                                    )
                                                    .map(|create| {
                                                        refusing(
                                                            create,
                                                            || "create-branch".into(),
                                                            busy || branch_empty,
                                                            looks_busy || branch_empty,
                                                        )
                                                    })
                                                    .on_click(cx.listener(|this, _, window, cx| {
                                                        let name = this
                                                            .branch_name
                                                            .read(cx)
                                                            .value()
                                                            .trim()
                                                            .to_owned();
                                                        let origin = confirmations::branch_form(
                                                            "create-branch",
                                                            &this.path,
                                                        );
                                                        this.write_from(
                                                            WriteCommand::CreateBranch {
                                                                name,
                                                                start_point: None,
                                                            },
                                                            "Creating branch…",
                                                            Some(origin.into()),
                                                            window,
                                                            cx,
                                                        );
                                                    })),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .w(px(1.))
                                        .h(crate::appearance::ui_size(32.))
                                        .mx_1()
                                        .bg(rgb(p.border)),
                                )
                                .child(
                                    div()
                                        .w(appearance::ui_size(120.))
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(action_field_label("Remote", p.muted))
                                        .child(refusing_input(
                                            Input::new(&self.remote_name)
                                                .aria_label("Git remote target")
                                                .text_size(crate::appearance::ui_text(12.)),
                                            || "remote-name".into(),
                                            busy,
                                            looks_busy,
                                        )),
                                )
                                .child(
                                    div()
                                        .w(appearance::ui_size(200.))
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(action_field_label("Remote branch", p.muted))
                                        .child(refusing_input(
                                            Input::new(&self.remote_branch)
                                                .aria_label("Remote branch target")
                                                .text_size(crate::appearance::ui_text(12.)),
                                            || "remote-branch".into(),
                                            busy,
                                            looks_busy,
                                        )),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .text_size(crate::appearance::ui_text(11.))
                                .text_color(rgb(p.muted))
                                .child(icon("remote", 12., p.muted))
                                .child(div().min_w_0().truncate().child(text.destination.clone())),
                        ),
                )
            })
            .into_any_element()
    }

    /// What the Git action bar's text is made of now, borrowed.
    fn git_actions_inputs<'a>(&'a self, cx: &'a App) -> GitActionsInputs<'a> {
        let remote = field_text(self.remote_name.read(cx));
        let push_url = (self.remotes.iter())
            .find(|configured| configured.name == *remote)
            .map(|configured| Cow::Borrowed(configured.push_url.as_str()));
        GitActionsInputs {
            status: self.header_status().map(|status| SyncInputs {
                branch: status.branch.as_deref().map(Cow::Borrowed),
                head: status.head.is_some(),
                detached: (status.head.as_deref())
                    .filter(|_| status.branch.is_none())
                    .map(Cow::Borrowed),
                upstream: status.upstream.as_deref().map(Cow::Borrowed),
                ahead: status.ahead,
                behind: status.behind,
                operation: status.operation.as_deref().map(Cow::Borrowed),
            }),
            remotes: !self.remotes.is_empty(),
            remote_branch: field_text(self.remote_branch.read(cx)),
            remote,
            push_url,
            busy: self.operation_shown(),
            unavailable: self.shown_tab_unavailable(),
        }
    }
}

/// What the Git action bar's text ([`GitActionsText`]) is made of, borrowed
/// from the window's state, so a frame compares it with what each kept text
/// was built from without building anything: the bar formatted its edge's
/// lines or its counts with their tooltip, Fetch's, Pull's and Push's
/// tooltips and names, the operation in progress and the branch and target
/// they name in every frame, and copied Targets' fields to read them, only
/// to draw the same text. The text is built again only when one of these
/// changes (`GitTurtle::render_git_actions`); it reads no font, size or
/// width, and holds every form a frame can draw, so a resize, a text size
/// or the compact bar builds nothing. Each field is everything one of the
/// text's builders reads, so a change any of them would show changes this.
#[derive(PartialEq)]
struct GitActionsInputs<'a> {
    /// The working status, once read.
    status: Option<SyncInputs<'a>>,
    /// Whether a remote is configured, which decides the edge with the
    /// status (`RepositoryEdge::of`).
    remotes: bool,
    /// Targets' remote and remote branch, trimmed, which Fetch, Pull and
    /// Push act on.
    remote: Cow<'a, str>,
    remote_branch: Cow<'a, str>,
    /// The push URL of the configured remote `remote` names, if one does:
    /// Fetch, Pull and Push wait for one, and Targets says where Push goes.
    push_url: Option<Cow<'a, str>>,
    /// The running write's label once its spell looks busy, which Fetch,
    /// Pull and Push say in place of their own tooltips.
    busy: Option<&'static str>,
    /// Whether the shown tab is marked, which Fetch, Pull and Push refuse
    /// for, saying the mark ahead of any remote or branch from before.
    unavailable: bool,
}

/// The parts of the working status the action bar's text shows
/// ([`GitActionsInputs`]).
#[derive(PartialEq)]
struct SyncInputs<'a> {
    branch: Option<Cow<'a, str>>,
    /// Whether HEAD has a commit, which an unborn branch's edge reads, and
    /// the commit where no branch is checked out, which a detached HEAD's
    /// edge names; another commit on a branch changes neither.
    head: bool,
    detached: Option<Cow<'a, str>>,
    upstream: Option<Cow<'a, str>>,
    ahead: u64,
    behind: u64,
    operation: Option<Cow<'a, str>>,
}

impl GitActionsInputs<'_> {
    /// These inputs as a key a cache keeps, owning what they borrowed.
    fn owned(&self) -> GitActionsInputs<'static> {
        let text = |text: &Cow<str>| Cow::Owned(String::from(&**text));
        GitActionsInputs {
            status: self.status.as_ref().map(|status| SyncInputs {
                branch: status.branch.as_ref().map(text),
                head: status.head,
                detached: status.detached.as_ref().map(text),
                upstream: status.upstream.as_ref().map(text),
                ahead: status.ahead,
                behind: status.behind,
                operation: status.operation.as_ref().map(text),
            }),
            remotes: self.remotes,
            remote: text(&self.remote),
            remote_branch: text(&self.remote_branch),
            push_url: self.push_url.as_ref().map(text),
            busy: self.busy,
            unavailable: self.unavailable,
        }
    }
}

/// The Git action bar's text, as [`GitActionsInputs`] make it, each string
/// shared, so a frame hands it to its element without copying it.
struct GitActionsText {
    /// The edge's lines, where the repository is at one.
    edge: Option<EdgeText>,
    /// The counts' text, where no edge takes their place.
    sync: Option<SyncText>,
    /// "… in progress", where an operation is.
    operation: Option<SharedString>,
    /// Fetch's, Pull's and Push's, in that order.
    actions: [ActionText; 3],
    /// What Targets says under its fields: where Push goes, or that no
    /// remote is configured.
    destination: SharedString,
}

/// An edge's lines in each form the bar draws, its glyph and its action
/// (`empty_state::RepositoryEdge`).
struct EdgeText {
    line: SharedString,
    short_line: SharedString,
    compact_line: SharedString,
    explanation: SharedString,
    glyph: Option<&'static str>,
    action: Option<crate::empty_state::EdgeAction>,
}

/// The counts' state: its accessible name, its tooltip in the full and the
/// compact bar, and each count in both forms the counts fit in.
struct SyncText {
    summary: SharedString,
    tooltip: SharedString,
    compact_tooltip: SharedString,
    ahead: Count,
    behind: Count,
}

/// A count as a number, and with its word.
struct Count {
    number: SharedString,
    words: SharedString,
}

/// Fetch's, Pull's or Push's tooltip in the full and the compact bar, and
/// its accessible name.
struct ActionText {
    tooltip: SharedString,
    compact_tooltip: SharedString,
    name: SharedString,
}

impl GitActionsText {
    /// The text `inputs` make, with the edge they put the repository at.
    fn build(inputs: &GitActionsInputs, edge: Option<crate::empty_state::RepositoryEdge>) -> Self {
        #[cfg(test)]
        GIT_ACTIONS_TEXT_BUILT.with(|built| built.set(built.get() + 1));
        let status = inputs.status.as_ref();
        let detached = status.is_none_or(|status| status.branch.is_none());
        let branch = status
            .and_then(|status| status.branch.as_deref())
            .unwrap_or("Detached HEAD");
        let (remote, remote_branch) = (&*inputs.remote, &*inputs.remote_branch);
        let target = format!("{remote}/{remote_branch}");
        let edge = edge.map(|edge| EdgeText {
            line: edge.line().into(),
            short_line: edge.short_line().into(),
            compact_line: edge.compact_line().into(),
            explanation: edge.explanation().into(),
            glyph: edge.glyph(),
            action: edge.action(),
        });
        let sync = status.filter(|_| edge.is_none()).map(|status| {
            let explanation = status.upstream.as_ref().map_or_else(
                || "No upstream is configured.".to_owned(),
                |upstream| {
                    format!("Compared with local {upstream}. Fetch to update remote information.")
                },
            );
            let summary = if status.upstream.is_some() {
                format!("{} ahead, {} behind", status.ahead, status.behind)
            } else {
                "No upstream".into()
            };
            let count = |value: u64, word: &str| Count {
                number: value.to_string().into(),
                words: format!("{value} {word}").into(),
            };
            SyncText {
                compact_tooltip: format!("{summary} · {explanation}").into(),
                tooltip: explanation.into(),
                summary: summary.into(),
                ahead: count(status.ahead, "ahead"),
                behind: count(status.behind, "behind"),
            }
        });
        let actions =
            [("fetch", "Fetch"), ("pull", "Pull"), ("push", "Push")].map(|(id, label)| {
                let tooltip: SharedString = if let Some(busy) = inputs.busy {
                    busy.into()
                } else if inputs.unavailable {
                    // The mark's word, as the branch picker says it: the
                    // status is absent, which read as a detached HEAD, and
                    // Fetch named the remote from before.
                    crate::branch_actions::UNAVAILABLE_REASON.into()
                } else if inputs.push_url.is_none() {
                    "No configured remote selected · Check Targets".into()
                } else if id != "fetch" && detached {
                    "Switch to a local branch before pulling or pushing".into()
                } else if id != "fetch" && remote_branch.is_empty() {
                    "Choose a remote branch in Targets".into()
                } else {
                    match id {
                        "fetch" => format!("Fetch from {remote}"),
                        "pull" => format!("Pull {target} into {branch} · fast-forward only"),
                        _ => format!("Push {branch} to {target}"),
                    }
                    .into()
                };
                ActionText {
                    compact_tooltip: format!("{label} · {tooltip}").into(),
                    name: format!("{label}: {tooltip}").into(),
                    tooltip,
                }
            });
        Self {
            operation: (status.and_then(|status| status.operation.as_deref()))
                .map(|operation| format!("{operation} in progress").into()),
            // A marked tab's remote and branch are those from before, which
            // the line once named as where Push goes.
            destination: if inputs.unavailable {
                crate::branch_actions::UNAVAILABLE_REASON.into()
            } else {
                inputs.push_url.as_deref().map_or_else(
                    || {
                        "No remote configured · branch and commit actions are available locally"
                            .into()
                    },
                    |url| format!("Push to {target} · {}", display_remote_url(url)).into(),
                )
            },
            edge,
            sync,
            actions,
        }
    }
}

/// [`GitActionsText`] by what it was made of, for the inputs drawn lately,
/// so windows and repository tabs drawn in turn, and a state drawn again,
/// such as the one before a write, keep their own.
type GitActionsTextCache =
    RefCell<crate::views::Measured<GitActionsInputs<'static>, Rc<GitActionsText>>>;

thread_local! {
    static GIT_ACTIONS_TEXT: GitActionsTextCache = const { RefCell::new(crate::views::Measured::new()) };
}

#[cfg(test)]
thread_local! {
    /// How many times the Git action bar's text was built
    /// ([`GitActionsText::build`]), for a test.
    static GIT_ACTIONS_TEXT_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// The line Targets last drew under its fields, for a test to read back.
    pub(crate) static TARGETS_DESTINATION: std::cell::RefCell<Option<SharedString>> =
        const { std::cell::RefCell::new(None) };
}

/// The composer's heading where it names its branch
/// (`GitTurtle::composer_branch`): "Commit to" that branch, or, on a marked
/// tab, the mark's reason as the branch picker says it.
fn composer_heading(branch: Option<&str>) -> String {
    branch.map_or_else(
        || crate::branch_actions::UNAVAILABLE_REASON.to_owned(),
        |branch| format!("Commit to {branch}"),
    )
}

/// `input`'s text without the whitespace around it, borrowed from its rope
/// where the rope keeps it in one piece, as it keeps a field's few words, so
/// a frame reads a field without the two copies `InputState::value` makes.
fn field_text(input: &InputState) -> Cow<'_, str> {
    match input.text().as_str() {
        Some(text) => Cow::Borrowed(text.trim()),
        None => Cow::Owned(input.text().to_string().trim().to_owned()),
    }
}

/// The space beside a labelled toolbar action's content: the 0.75 rem it took
/// as `px_3` at the root rem the app sets, the interface text's 13 px.
/// The file row a Working Changes menu acts on: its status entry and whether
/// it is the staged row.
#[derive(Clone)]
pub(super) struct MenuRow {
    pub(super) entry: StatusEntry,
    pub(super) staged: bool,
}

impl MenuRow {
    pub(super) fn area(&self) -> ChangeArea {
        if self.staged {
            ChangeArea::Staged
        } else {
            ChangeArea::Unstaged
        }
    }
}

/// The Working Changes row a keyboard menu acts on (`GitTurtle::file_menu_row`).
pub(super) struct WorkingMenu {
    /// Its file as the menu's items were last built from it, by path and
    /// group.
    pub(super) target: MenuRow,
    /// Why a write refused the items as they were last built, which each
    /// states (`GitTurtle::menu_wait`), or `None` where none did: built
    /// again as that changes, so an ended write is named no more once its
    /// status read is all the items wait for.
    pub(super) wait: Option<native_accessibility::MenuWait>,
    /// Why the items were built refused while the rows were prepared, which
    /// each states (`GitTurtle::working_rows_wait`), or `None` where they
    /// were not: the rows' landing builds them again, and so does a change
    /// of that reason meanwhile.
    pub(super) waiting: Option<&'static str>,
}

/// A Working Changes file row's element ID: its group and its path's bytes,
/// all of them, so `sub` and `sub/`, which `Path` holds equal, are two rows,
/// and a name that is not UTF-8 keeps every byte. The state GPUI keeps for
/// the row, its open right-click menu among it, stays with the file as rows
/// come or go around it, and goes with the row when the file leaves the
/// list. Keyed by the file's index in the status, a menu open on a row
/// passed to the next row when a refresh took an earlier file away, its
/// items still acting on the file it was opened on. The bytes are written
/// as text one way only: valid UTF-8 as itself with each backslash doubled,
/// and any other byte as `\xNN`.
pub(super) fn working_row_id(path: &std::path::Path, area: ChangeArea) -> ElementId {
    use std::fmt::Write as _;
    let mut id = String::from(match area {
        ChangeArea::Staged => "staged-file:",
        ChangeArea::Unstaged => "unstaged-file:",
    });
    for chunk in path.as_os_str().as_encoded_bytes().utf8_chunks() {
        for c in chunk.valid().chars() {
            if c == '\\' {
                id.push('\\');
            }
            id.push(c);
        }
        for byte in chunk.invalid() {
            let _ = write!(id, "\\x{byte:02x}");
        }
    }
    ElementId::Name(id.into())
}

/// The element IDs of the rows of one status's files, made as the list
/// first draws each row and reused in every frame after, a clone of a
/// shared string, until the rows show another status
/// (`GitTurtle::working_row_key`). The status is held weakly, which keeps
/// its allocation, so another status never takes its address.
#[derive(Default)]
pub(super) struct WorkingRowIds {
    status: std::sync::Weak<gitturtle_core::RepositoryStatus>,
    ids: HashMap<(usize, bool), ElementId>,
}

impl WorkingRowIds {
    /// The ID of the row of `status`'s `index`th file in `area`.
    pub(super) fn get(
        &mut self,
        status: &Arc<gitturtle_core::RepositoryStatus>,
        index: usize,
        area: ChangeArea,
    ) -> ElementId {
        if !std::ptr::eq(self.status.as_ptr(), Arc::as_ptr(status)) {
            self.status = Arc::downgrade(status);
            self.ids.clear();
        }
        self.ids
            .entry((index, area == ChangeArea::Staged))
            .or_insert_with(|| working_row_id(&status.entries[index].path, area))
            .clone()
    }
}

/// What a Working row's menu item states beside it while the rows of a newer
/// status are prepared, as a read of it lands.
pub(super) const REFRESHING_ROWS: &str = "Refreshing…";
/// What it states while the rows the filter's query or the Directories
/// grouping shows are prepared, as the list says "Filtering working paths…"
/// until the first of them land.
pub(super) const FILTERING_ROWS: &str = "Filtering…";
/// Why a control acting on the listed files waits while a newer status's
/// rows are prepared, in its accessible name at once and its tooltip once
/// it dims (`GitTurtle::rows_wait`).
pub(super) const REFRESHING_ROWS_REASON: &str = "Wait for the file list to refresh";
/// Why it waits while the rows of the filter's query or the Directories
/// grouping are prepared.
pub(super) const FILTERING_ROWS_REASON: &str = "Wait for the filter to finish";

/// A Working Changes file row's menu, which a right-click on the row and
/// Shift-F10 or the Menu key on the list's cursor row open alike. It leads
/// with Stage or Unstage at every width, whether or not the row draws that
/// control, so one row's menu reads the same however wide the window is.
/// Each item acts only on the repository the menu was built for, and
/// on its file by path: a Stage or Unstage departs from where the file's row
/// stands when it acts, which rows coming or going before it may have moved
/// since the menu opened. `wait`, a write or what it reads after it
/// (`GitTurtle::menu_wait`), refuses every item; so does `waiting`, while the
/// rows are prepared (`GitTurtle::working_rows_wait`), whatever else an item
/// waits for. Each item they refuse states why, as every menu's items do
/// (`native_accessibility::waiting_item`): the write's or the rows' label at
/// its row's end and, in its accessible name, "Stage, available when staging
/// file finishes" or "…, available when filtering finishes"; an item the row
/// cannot take states its own condition instead, as an item a row cannot take
/// does.
pub(super) fn working_row_menu(
    menu: PopupMenu,
    owner: WeakEntity<GitTurtle>,
    repository: Option<PathBuf>,
    target: MenuRow,
    wait: Option<native_accessibility::MenuWait>,
    waiting: Option<&'static str>,
) -> PopupMenu {
    let MenuRow { entry, staged } = target;
    // Refused with nothing to say why, a menu opened as the rows were
    // prepared showed every item dim and was announced so, and one built as
    // a write ran, its rows standing, did so as well.
    let preparing = waiting.is_some();
    let wait = waiting.map(native_accessibility::MenuWait::named).or(wait);
    // While the rows are prepared, their landing is what every item waits
    // for, whatever else it waits for.
    let item = |label: &'static str, refused: bool| {
        native_accessibility::waiting_item(label, refused && !preparing, wait)
    };
    let discard_owner = owner.clone();
    let discard_repository = repository.clone();
    let discard_entry = entry.clone();
    let stage_owner = owner.clone();
    let stage_repository = repository.clone();
    let paths = entry.paths();
    let path = entry.path.clone();
    let menu = menu.item(
        item(if staged { "Unstage" } else { "Stage" }, false).on_click(move |_, window, cx| {
            let _ = stage_owner.update(cx, |this, cx| {
                if this.path == stage_repository {
                    let area = if staged {
                        ChangeArea::Staged
                    } else {
                        ChangeArea::Unstaged
                    };
                    if let Some(row) = this.working_row_position(&path, area) {
                        // A double-click's second press lands where this
                        // write moves a row or a heading under the pointer,
                        // as on a row's own control. Held on every confirm:
                        // for a keyboard's it costs only hover staying off
                        // until the pointer moves.
                        if this.operation_busy.is_none() {
                            this.hold_still_pointer(row, window);
                        }
                        this.depart_working_row(row, window, cx);
                    }
                    this.stage_working_paths(paths.clone(), &path, staged, window, cx);
                }
            });
        }),
    );
    // An action the row cannot take reads as its command with the condition
    // it waits for apart from it, not as one phrase, and the menu keys rest
    // on it, so that condition is heard; while the rows are prepared, that
    // is their landing, whatever else it waits for.
    let ignore = if entry.untracked || preparing {
        item("Ignore…", staged)
    } else {
        native_accessibility::detailed_item("Ignore…", "Untracked files only")
    };
    let discard = if discard_entry.conflicted && !preparing {
        native_accessibility::detailed_item("Discard changes…", "After conflict resolution")
    } else if discard_entry.untracked {
        item("Delete untracked file…", false)
    } else {
        item("Discard changes…", false)
    };
    menu.item(ignore.on_click(move |_, window, cx| {
        let _ = owner.update(cx, |this, cx| {
            if this.path == repository {
                this.open_ignore(entry.clone(), window, cx);
            }
        });
    }))
    // Discard reverts the whole file to HEAD, so both the staged and
    // unstaged rows offer the same action.
    .item(discard.on_click(move |_, window, cx| {
        let _ = discard_owner.update(cx, |this, cx| {
            if this.path == discard_repository {
                this.open_discard(discard_entry.clone(), window, cx);
            }
        });
    }))
}

fn action_inset() -> Pixels {
    appearance::ui_text(13.) * 0.75
}

/// The Git writes that confirm on the control that asked for them
/// ([`GitTurtle::write_from`]): each control's icon at rest and the word it
/// reads once Git reports success, after the check or, in the action bar's
/// labelled Fetch, Pull and Push, alone (`copy_feedback::Fit`).
pub(super) mod confirmations {
    use crate::copy_feedback::{Action, Mark};
    use gitturtle_core::{BranchCommand, ChangeArea, WriteCommand};
    use std::path::{Path, PathBuf};

    /// A file row's control in Staged, which unstages at rest and reads
    /// Staged once that file's Stage succeeded.
    pub(crate) const STAGED_FILE: Action = Action {
        icon: "minus",
        done: "Staged",
        state: "staged",
    };
    /// A file row's control in Unstaged, which stages at rest and reads
    /// Unstaged once that file's Unstage succeeded.
    pub(crate) const UNSTAGED_FILE: Action = Action {
        icon: "plus",
        done: "Unstaged",
        state: "unstaged",
    };
    pub(crate) const STAGE_ALL: Action = Action {
        icon: "plus",
        done: "Staged",
        state: "staged",
    };
    pub(crate) const UNSTAGE_ALL: Action = Action {
        icon: "minus",
        done: "Unstaged",
        state: "unstaged",
    };
    pub(crate) const FETCH: Action = Action {
        icon: "fetch",
        done: "Fetched",
        state: "fetched",
    };
    pub(crate) const PULL: Action = Action {
        icon: "pull",
        done: "Pulled",
        state: "pulled",
    };
    pub(crate) const PUSH: Action = Action {
        icon: "push",
        done: "Pushed",
        state: "pushed",
    };
    pub(crate) const CREATE: Action = Action {
        icon: "branch-add",
        done: "Created",
        state: "created",
    };
    pub(crate) const SWITCH: Action = Action {
        icon: "branch",
        done: "Switched",
        state: "switched",
    };

    /// The control of `path`'s row in `area` of `repository`'s working
    /// changes. A file staged arrives in Staged, so its Stage confirms on the
    /// control of its row there, the row it moved to, and an Unstage on its
    /// row in Unstaged.
    pub(crate) fn file(repository: &Option<PathBuf>, path: &Path, area: ChangeArea) -> Mark {
        Mark::per_subject(
            match area {
                ChangeArea::Staged => "staged-file-action",
                ChangeArea::Unstaged => "unstaged-file-action",
            },
            (repository, path),
        )
    }

    /// Stage all, or in Staged Unstage all, of `repository`.
    pub(crate) fn all(repository: &Option<PathBuf>, area: ChangeArea) -> Mark {
        Mark::new(
            match area {
                ChangeArea::Staged => "unstage-all",
                ChangeArea::Unstaged => "stage-all",
            },
            repository,
        )
    }

    /// The action bar's Fetch, Pull or Push (`id`) of `repository` toward
    /// `target`, its remote and branches, so changing Targets leaves the
    /// next target's control at rest.
    pub(crate) fn sync(id: &'static str, repository: &Option<PathBuf>, target: &[&str]) -> Mark {
        Mark::new(id, (repository, target))
    }

    /// Targets' Create (`create-branch`) or Switch (`checkout-branch`) of
    /// `repository`, whose name field empties once the branch is created.
    pub(crate) fn branch_form(id: &'static str, repository: &Option<PathBuf>) -> Mark {
        Mark::new(id, repository)
    }

    /// The branch picker while it shows `branch` of `repository`: a switch
    /// chosen from its menu confirms once the picker names that branch.
    pub(crate) fn picker(repository: &Option<PathBuf>, branch: &str) -> Mark {
        Mark::new("current-branch", (repository, branch))
    }

    /// The branch picker while it shows `branch` of `repository`, a tracking
    /// branch just created and switched to: it reads [`CREATE`]'s word
    /// rather than [`SWITCH`]'s.
    pub(crate) fn picker_created(repository: &Option<PathBuf>, branch: &str) -> Mark {
        Mark::new("current-branch", (repository, branch, CREATE.state))
    }

    /// Where a write confirms once Git reports success
    /// (`GitTurtle::write_from`); a write with none keeps the window-wide
    /// notice.
    #[derive(Clone, Debug)]
    pub(crate) enum Origin {
        /// The control that asked reads its word, the outcome is announced
        /// once and the status line records it.
        Control(Mark),
        /// No control reads anything: the outcome is announced once and the
        /// status line records it.
        StatusLine,
    }

    impl Origin {
        pub(crate) fn mark(self) -> Option<Mark> {
            match self {
                Self::Control(mark) => Some(mark),
                Self::StatusLine => None,
            }
        }
    }

    impl From<Mark> for Origin {
        fn from(mark: Mark) -> Self {
            Self::Control(mark)
        }
    }

    /// Where a write confirmed in a review dialog (`confirm_git_write`)
    /// confirms in `repository`. A tracking branch, created from the branch
    /// picker's menu or a remote branch's details, confirms on the picker
    /// once the picker names it where Git switched to it, and otherwise in
    /// the status line; every other reviewed write keeps the window-wide
    /// notice (`None`).
    pub(crate) fn reviewed(repository: &Option<PathBuf>, command: &WriteCommand) -> Option<Origin> {
        match command {
            WriteCommand::Branch(branch) => match branch.as_ref() {
                BranchCommand::CreateTracking { plan } if plan.checkout => {
                    Some(picker_created(repository, &plan.name).into())
                }
                BranchCommand::CreateTracking { .. } => Some(Origin::StatusLine),
                _ => None,
            },
            _ => None,
        }
    }
}

/// The action bar's labelled Fetch, Pull or Push frame, 34 px high, without
/// content: `copy_feedback::confirming` draws its icon and label, `gap_2`
/// apart as the kit's medium Button sets them.
pub(crate) fn action_frame(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .secondary()
        .h(crate::appearance::ui_size(34.))
        .px(action_inset())
        .text_size(crate::appearance::ui_text(12.))
        .font_weight(FontWeight::MEDIUM)
}

fn action_field_label(label: &'static str, color: u32) -> impl IntoElement {
    div()
        .text_size(crate::appearance::ui_text(10.))
        .text_color(rgb(color))
        .font_weight(FontWeight::MEDIUM)
        .child(label)
}

/// How the Working list brings the selected file's row into view, where it
/// was in view, as the rows of a read that changed them land. A write's own
/// read centres it where the write moved it out of view, so it lands with
/// context around it, and the quiet read a write asks for leaves the list on
/// the rows being read. A read from outside the app, Refresh's or the local
/// watcher's, moves the list only as far as shows the row, whether or not
/// the rows' number changed: one file leaving Unstaged for Staged keeps it.
pub(super) fn working_follow(quiet: bool, from_write: bool) -> Option<ScrollStrategy> {
    match (from_write, quiet) {
        (false, _) => Some(ScrollStrategy::Nearest),
        (true, false) => Some(ScrollStrategy::Center),
        (true, true) => None,
    }
}

pub(super) fn retained_area(
    preferred: ChangeArea,
    staged: bool,
    unstaged: bool,
) -> Option<ChangeArea> {
    match preferred {
        ChangeArea::Staged if staged => Some(ChangeArea::Staged),
        ChangeArea::Unstaged if unstaged => Some(ChangeArea::Unstaged),
        _ if staged => Some(ChangeArea::Staged),
        _ if unstaged => Some(ChangeArea::Unstaged),
        _ => None,
    }
}

fn remote_defaults(
    status: &gitturtle_core::RepositoryStatus,
    remotes: &[gitturtle_core::Remote],
) -> (String, String) {
    if let Some(upstream) = &status.upstream
        && let Some((remote, branch)) = remotes
            .iter()
            .filter_map(|remote| {
                upstream
                    .strip_prefix(&format!("{}/", remote.name))
                    .map(|branch| (remote, branch))
            })
            .max_by_key(|(remote, _)| remote.name.len())
    {
        return (remote.name.clone(), branch.into());
    }
    let remote = remotes
        .iter()
        .find(|remote| remote.name == "origin")
        .or(remotes.first())
        .map(|remote| remote.name.clone())
        .unwrap_or_default();
    (remote, status.branch.clone().unwrap_or_default())
}

pub(super) fn display_remote_url(url: &str) -> String {
    let without_query = url.split(['?', '#']).next().unwrap_or(url);
    if let Some((scheme, remainder)) = without_query.split_once("://") {
        let (authority, path) = remainder
            .split_once('/')
            .map(|(authority, path)| (authority, format!("/{path}")))
            .unwrap_or((remainder, String::new()));
        let host = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        format!("{scheme}://{host}{path}")
    } else {
        without_query.into()
    }
}

#[cfg(test)]
mod confirmation_tests;

#[cfg(test)]
mod tests {
    use super::{
        commit_composer_height, display_remote_url, remote_defaults, retained_area, working_row_id,
    };
    use core::prelude::v1::test;
    use gitturtle_core::ChangeArea;
    use gpui_kit::gpui;

    /// Where the tab is marked, the banner beneath the Git action bar offers
    /// nothing from before: a merge stopped on a conflict, read before the
    /// mark, draws no "Merge in progress" with Complete merge and Abort, and
    /// branches the status from before found diverged draw no Merge… and
    /// Rebase…. The banner read the operation and the status kept beside
    /// the mark, so a marked tab offered to continue, abort or integrate
    /// what it could no longer read.
    #[gpui::test]
    async fn a_marked_tab_s_banner_offers_nothing_from_before(cx: &mut gpui::TestAppContext) {
        use crate::views::tests::{draw, fixture_git, fixture_git_output, settle};
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        let diverge = |app: &mut crate::GitTurtle| {
            let status = std::sync::Arc::make_mut(app.work_status.as_mut().expect("read"));
            (status.ahead, status.behind) = (1, 2);
            status.upstream = Some("origin/main".into());
        };
        app.update(cx, |app, cx| {
            diverge(app);
            cx.notify();
        });
        draw(cx);
        assert!(cx.debug_bounds("merge-upstream").is_some(), "diverged");
        for (key, value) in [
            ("user.name", "Fixture"),
            ("user.email", "fixture@example.invalid"),
            ("commit.gpgsign", "false"),
        ] {
            fixture_git(&root, &["config", key, value]);
        }
        let branch = fixture_git_output(&root, &["branch", "--show-current"]);
        let branch = String::from_utf8_lossy(&branch.stdout).trim().to_owned();
        fixture_git(&root, &["checkout", "-q", "-b", "side"]);
        std::fs::write(root.join("conflict.txt"), "side\n").unwrap();
        fixture_git(&root, &["add", "conflict.txt"]);
        fixture_git(&root, &["commit", "-q", "-m", "Side"]);
        fixture_git(&root, &["checkout", "-q", &branch]);
        std::fs::write(root.join("conflict.txt"), "main\n").unwrap();
        fixture_git(&root, &["add", "conflict.txt"]);
        fixture_git(&root, &["commit", "-q", "-m", "Main"]);
        let merge = fixture_git_output(&root, &["merge", "side"]);
        assert!(!merge.status.success(), "the merge stops on its conflict");
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        settle(&app, cx).await;
        draw(cx);
        assert!(app.read_with(cx, |app, _| app.integration_state.is_some()));
        assert!(cx.debug_bounds("operation-banner").is_some(), "the merge");

        std::fs::rename(root.join(".git"), fixture.path().join("git-away")).unwrap();
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", fixture.path().join("nowhere").display()),
        )
        .unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&crate::Refresh, window, cx)));
        settle(&app, cx).await;
        draw(cx);
        assert!(app.read_with(cx, |app, _| app.shown_tab_unavailable()));
        assert!(
            app.read_with(cx, |app, _| app.integration_state.is_some()),
            "the merge read before the mark is kept beside it"
        );
        for id in ["operation-banner", "merge-upstream", "rebase-upstream"] {
            assert!(cx.debug_bounds(id).is_none(), "the merge kept: {id}");
        }
        app.update(cx, |app, cx| {
            app.integration_state = None;
            if app.work_status.is_none() {
                app.work_status = Some(Default::default());
            }
            diverge(app);
            cx.notify();
        });
        draw(cx);
        for id in ["operation-banner", "merge-upstream", "rebase-upstream"] {
            assert!(cx.debug_bounds(id).is_none(), "diverged before: {id}");
        }
    }

    /// A placeholder too wide for its field is cut between whole grapheme
    /// clusters: a letter with its combining marks, a ZWJ emoji sequence or
    /// a flag that does not fit whole beside the ellipsis is left out whole,
    /// as is the space before it. Every character here is one unit wide.
    #[test]
    fn a_cut_placeholder_keeps_whole_grapheme_clusters() {
        use crate::text_cut::tests::{CLUSTERS, assert_whole_prefix};
        for (case, cluster) in CLUSTERS {
            let full = format!("Commit to {cluster}feature");
            for room in 11..=10 + cluster.chars().count() {
                let shown = super::cut_to_fit(&full, |text| text.chars().count() <= room);
                let kept = shown.strip_suffix('…').expect("a cut placeholder");
                assert_whole_prefix(&full, kept, case);
                assert_eq!(kept, "Commit to", "{case} in a room of {room}");
            }
        }
    }

    #[test]
    fn small_working_inspector_reserves_files_after_wrapping_controls() {
        // Reported 680-point content: 490-point inspector less 165 points of
        // filter/selection controls and header, with Compact rows at UI 18.
        let body = 490. - 165.;
        let row = 34. * 18. / 13.;
        let composer = commit_composer_height(body, row, 244.);
        assert!(body - composer >= 3. * row);
        assert!(composer >= 36. * 18. / 13. + 48.);
        for body in [0., 80., 160., 325., 600., 1000.] {
            let composer = commit_composer_height(body, row, 344.);
            assert!((0. ..=body).contains(&composer));
            assert!(body - composer >= body * 0.30);
        }
    }

    #[test]
    fn staging_preserves_path_selection_and_moves_area_only_when_needed() {
        assert_eq!(
            retained_area(ChangeArea::Unstaged, true, false),
            Some(ChangeArea::Staged)
        );
        assert_eq!(
            retained_area(ChangeArea::Staged, false, true),
            Some(ChangeArea::Unstaged)
        );
        assert_eq!(
            retained_area(ChangeArea::Unstaged, true, true),
            Some(ChangeArea::Unstaged)
        );
        assert_eq!(retained_area(ChangeArea::Staged, false, false), None);
    }

    #[test]
    fn remote_defaults_follow_the_new_branch_and_its_upstream() {
        let remotes = [
            gitturtle_core::Remote {
                name: "origin".into(),
                url: "local".into(),
                push_url: "local".into(),
            },
            gitturtle_core::Remote {
                name: "team/upstream".into(),
                url: "other".into(),
                push_url: "other".into(),
            },
        ];
        let mut status = gitturtle_core::RepositoryStatus {
            branch: Some("feature/new".into()),
            ..Default::default()
        };
        assert_eq!(
            remote_defaults(&status, &remotes),
            ("origin".into(), "feature/new".into())
        );
        status.upstream = Some("team/upstream/different-name".into());
        assert_eq!(
            remote_defaults(&status, &remotes),
            ("team/upstream".into(), "different-name".into())
        );
    }

    /// A file row's ID keeps every byte of its path: `sub` deleted and an
    /// untracked repository at `sub/` are two Unstaged rows, which `Path`,
    /// equal for both, gave one ID; a name that is not UTF-8 keeps its
    /// bytes; and a backslash in a name is never read as an escape.
    #[test]
    #[cfg(unix)]
    fn working_row_ids_keep_every_byte_of_the_path() {
        use std::os::unix::ffi::OsStrExt as _;
        let id = |path: &[u8], area| {
            working_row_id(
                std::path::Path::new(std::ffi::OsStr::from_bytes(path)),
                area,
            )
        };
        let unstaged = ChangeArea::Unstaged;
        for (one, other) in [
            (&b"sub"[..], &b"sub/"[..]),
            (b"a\xff", b"a\xfe"),
            (b"a\xff", b"a\\xff"),
            (b"a//b", b"a/b"),
            (b"./a", b"a"),
        ] {
            assert_ne!(
                id(one, unstaged),
                id(other, unstaged),
                "{one:?} and {other:?}"
            );
        }
        assert_ne!(id(b"a", unstaged), id(b"a", ChangeArea::Staged));
        assert_eq!(id(b"a\xff", unstaged), id(b"a\xff", unstaged));
    }

    /// The rows' lookups by path (a menu's row, a departure's landing, the
    /// cursor kept through a refresh, the scroll anchor, the selection's
    /// files) compare every byte, as the rows' IDs do: `sub` and `sub/`,
    /// which `Path` holds equal, are two files.
    #[test]
    fn same_path_tells_sub_from_sub_slash() {
        use crate::working_selection::same_path;
        use std::path::Path;
        assert!(!same_path(Path::new("sub"), Path::new("sub/")));
        assert!(!same_path(Path::new("a//b"), Path::new("a/b")));
        assert!(same_path(Path::new("sub/"), Path::new("sub/")));
    }

    #[test]
    fn displayed_remote_address_does_not_expose_embedded_credentials_or_query() {
        assert_eq!(
            display_remote_url("https://user:secret@example.invalid/team/repo?token=private"),
            "https://example.invalid/team/repo"
        );
        assert_eq!(
            display_remote_url("git@example.invalid:team/repo.git"),
            "git@example.invalid:team/repo.git"
        );
    }
}

#[cfg(test)]
mod targets_focus_tests {
    use crate::tags::tests::{draw, tagged_repository, window};
    use crate::*;
    use core::prelude::v1::test;

    /// Targets' Switch turns disabled while it holds focus: a completed switch
    /// clears the branch field. Tab and Shift+Tab, as keystrokes through the
    /// window, still leave it for its neighbours, past the Create button that
    /// the empty field disables too.
    #[gpui::test]
    fn tab_and_shift_tab_leave_a_focused_switch_that_turns_disabled(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        let set_branch = |cx: &mut VisualTestContext, value: &'static str| {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.git_actions_open = true;
                    app.branch_name
                        .update(cx, |input, cx| input.set_value(value, window, cx));
                    cx.notify();
                })
            });
            draw(cx);
        };
        let press = |cx: &mut VisualTestContext, keys: &str| {
            cx.simulate_keystrokes(keys);
            draw(cx);
            cx.update(|window, cx| window.focused(cx))
                .expect("a focused control")
        };

        set_branch(cx, "topic");
        let (branch, remote) = cx.read(|cx| {
            let app = app.read(cx);
            (
                app.branch_name.read(cx).focus_handle(cx),
                app.remote_name.read(cx).focus_handle(cx),
            )
        });
        cx.update(|window, cx| branch.focus(window, cx));
        draw(cx);
        // Enabled, Targets reads: branch field, Switch, Create, Remote field.
        let switch = press(cx, "tab");
        let create = press(cx, "tab");
        assert!(switch != branch && create != switch && create != remote);
        assert_eq!(press(cx, "tab"), remote, "Create precedes the Remote field");
        assert_eq!(press(cx, "shift-tab"), create);
        assert_eq!(press(cx, "shift-tab"), switch);

        set_branch(cx, "");
        assert_eq!(
            cx.update(|window, cx| window.focused(cx)),
            Some(switch.clone()),
            "the disabled Switch keeps focus until the keyboard moves it"
        );
        assert_eq!(
            press(cx, "tab"),
            remote,
            "Tab leaves the disabled Switch, past the disabled Create"
        );
        assert_eq!(
            press(cx, "shift-tab"),
            branch,
            "Shift+Tab passes over both disabled buttons"
        );

        set_branch(cx, "topic");
        assert_eq!(press(cx, "tab"), switch);
        set_branch(cx, "");
        assert_eq!(
            press(cx, "shift-tab"),
            branch,
            "Shift+Tab leaves the disabled Switch for the branch field"
        );
    }
}

#[cfg(test)]
mod composer_metrics_tests {
    use super::ComposerMetrics;
    use crate::views::tests::history_window;
    use crate::*;
    use core::prelude::v1::test;

    /// A composer exactly its least height keeps one line of its
    /// description at every scale, the default size and 18 pt, short or
    /// taller, with its heading or without: its rows are sums of
    /// device-pixel lengths, which at a fractional scale such as 1.2 come
    /// to a hair less than the line they hold, and counted with no slack the
    /// line rounded down to none, so the body scrolled where one line fit.
    #[gpui::test]
    async fn a_composer_its_least_height_keeps_one_line_at_every_scale(cx: &mut TestAppContext) {
        let (_fixture, _app, cx) = history_window(cx).await;
        let default_scale = cx.update(|window, _| window.scale_factor());
        let mut misses = Vec::new();
        for scale in [1., 1.2, 1.25, 1.5, 1.75, 2.] {
            cx.update(|window, _| window.set_scale_factor(scale));
            for text in [13, 18] {
                cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                cx.update(|window, _| {
                    for (short, heading) in
                        [(true, true), (true, false), (false, true), (false, false)]
                    {
                        let metrics = ComposerMetrics::new(short, heading, window);
                        let preferred = f32::from(metrics.description_least()) + 100.;
                        // The fields' other rows and the Description
                        // section's, each a whole number of device pixels.
                        for other in 0..200 {
                            for section in 20..60 {
                                let section = metrics.label + px(section as f32 / scale);
                                let rows =
                                    metrics.title_rows() + px(other as f32 / scale) + section;
                                let least = metrics.least(rows, section);
                                let field =
                                    metrics.description_field(least, rows, preferred, window);
                                if field.is_none() {
                                    misses.push((scale, text, short, heading, rows));
                                }
                            }
                        }
                    }
                });
            }
        }
        cx.update(|window, cx| {
            window.set_scale_factor(default_scale);
            appearance::apply_text_sizes(13, 12, window, cx)
        });
        assert!(
            misses.is_empty(),
            "{} of the least heights keep no line, the first {:?}",
            misses.len(),
            misses.first()
        );
    }
}

#[cfg(test)]
mod background_clone_tests {
    use crate::views::tests::{draw, history_window, settle};
    use crate::*;
    use core::prelude::v1::test;

    /// A clone that fails while another repository shows says so in the
    /// status line as one part, its destination's path whole: started from
    /// Projects into a folder whose name holds " · ", from a local folder
    /// that is not a repository, and left for the repository before Git
    /// answers, the status reads "Repository operation failed in <path>:
    /// <Git's report>", the repository's own error untouched, and in a
    /// 461 × 490 half tile, where it does not fit, the line keeps it whole
    /// up to its ellipsis rather than giving way at the separator inside
    /// the path. Set as a plain status, the line gave way there and read
    /// the path's head as if it were all of it.
    #[gpui::test]
    async fn a_clone_that_fails_in_the_background_keeps_its_path_one_part(cx: &mut TestAppContext) {
        let (fixture, app, cx) = history_window(cx).await;
        let folder = fixture.path().join("clones · b");
        let source = fixture.path().join("not a repository");
        std::fs::create_dir(&folder).unwrap();
        std::fs::create_dir(&source).unwrap();
        let destination = folder.join("copy");
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.show_projects(window, cx);
                app.project_event(
                    &projects::ProjectEvent::Clone {
                        source: source.display().to_string(),
                        destination: destination.clone(),
                    },
                    window,
                    cx,
                );
                app.return_from_page(window, cx);
            })
        });
        assert_eq!(
            app.read_with(cx, |app, _| app.page),
            AppPage::Repository,
            "the repository shows while Git clones"
        );
        // The clone answers on the operation executor, a thread of its own.
        app.read_with(cx, |app, _| app.operations.drain());
        settle(&app, cx).await;
        let (status, error) = app.read_with(cx, |app, _| {
            (app.status.clone(), app.operation_error_text())
        });
        let head = format!("Repository operation failed in {}: ", destination.display());
        assert!(
            status.starts_with(&head) && status.len() > head.len(),
            "the status says the clone failed, where and why: {status:?}"
        );
        assert_eq!(error, None, "the repository's own error says nothing");
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        let drawn =
            cx.update(|window, cx| app.update(cx, |app, cx| app.status_line_drawn(window, cx)));
        let line = cx.debug_bounds("status-line").expect("the status line");
        let need = cx.update(|window, _| {
            let weight = window.text_style().font_weight;
            crate::views::shaped_width(&status, appearance::ui_text(10.), weight, window)
        });
        assert!(
            need > line.size.width,
            "the status, {need:?} wide, does not fit the line {line:?}"
        );
        assert_eq!(
            drawn, status,
            "the line keeps the status whole up to its ellipsis, the path in it"
        );
        drop(fixture);
    }
}

#[cfg(test)]
mod filled_action_tests {
    use crate::views::tests::{
        draw, fixture_git, fixture_identity, history_window, painted_edges, painted_fills, settle,
    };
    use crate::*;
    use core::prelude::v1::test;

    /// A surface fills only the action its state leads to (decision,
    /// 2026-11-02). In Working Changes that is Commit, so Push there is the
    /// bordered secondary, edged in the toolkit's secondary edge (the
    /// dialog's), in the bounds it fills in History and with its icon and
    /// label where they were, full and compact, at 13 and 18 pt, in Midnight
    /// and Daylight. Working Changes drew Commit and Push as two filled
    /// primaries.
    #[gpui::test]
    async fn working_changes_fills_commit_alone_and_push_moves_nothing(cx: &mut TestAppContext) {
        let (fixture, app, cx) = history_window(cx).await;
        let worktree = fixture.path().join("narrow");
        let remote = fixture.path().join("remote.git");
        fixture_git(
            fixture.path(),
            &[
                "init",
                "--bare",
                "--quiet",
                "--initial-branch=main",
                remote.to_str().unwrap(),
            ],
        );
        fixture_git(&worktree, &["reset", "--hard", "--quiet"]);
        fixture_git(
            &worktree,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        fixture_git(
            &worktree,
            &["push", "--quiet", "--set-upstream", "origin", "main"],
        );
        fixture_identity(&worktree);
        std::fs::write(worktree.join("another.txt"), "another\n").unwrap();
        fixture_git(&worktree, &["add", "another.txt"]);
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        settle(&app, cx).await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.commit_title
                    .update(cx, |title, cx| title.set_value("Another", window, cx))
            })
        });
        let bounds = |cx: &mut VisualTestContext, selector: &'static str| {
            cx.debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} is drawn"))
        };
        let mut failures = Vec::new();
        for (window, compact) in [
            (size(px(1480.), px(980.)), false),
            (size(px(461.), px(400.)), true),
        ] {
            cx.simulate_resize(window);
            for text in [13, 18] {
                cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                for theme in [
                    appearance::ThemeChoice::Midnight,
                    appearance::ThemeChoice::Daylight,
                ] {
                    let palette = theme.palette();
                    cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
                    let at = format!("{} at {text} pt in {window:?}", theme.label());
                    let accent =
                        gpui_kit::Background::from(gpui_kit::Hsla::from(rgb(palette.accent)));

                    cx.update(|window, cx| app.update(cx, |app, cx| app.show_history(window, cx)));
                    settle(&app, cx).await;
                    draw(cx);
                    let filled = (bounds(cx, "push"), bounds(cx, "push/rest"));
                    // The compact Push is its icon's square, snapped to
                    // whole pixels.
                    assert_eq!(
                        (filled.0.size.width - appearance::ui_size(28.)).abs() < px(1.),
                        compact,
                        "{at}: compact {compact}: {filled:?}"
                    );
                    if !painted_fills(cx, "push").contains(&accent) {
                        failures.push(format!("{at}: History's Push is not filled"));
                    }

                    cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
                    settle(&app, cx).await;
                    draw(cx);
                    let bordered = (bounds(cx, "push"), bounds(cx, "push/rest"));
                    if bordered != filled {
                        failures.push(format!("{at}: Push moved from {filled:?} to {bordered:?}"));
                    }
                    let fills = painted_fills(cx, "push");
                    if fills.contains(&accent) {
                        failures.push(format!("{at}: Working's Push is filled: {fills:?}"));
                    }
                    let edges = painted_edges(cx, "push");
                    if edges != [gpui_kit::Hsla::from(rgb(palette.dialog_edge()))] {
                        failures.push(format!("{at}: Working's Push edges {edges:?}"));
                    }
                    // A 400 px window scrolls the list and the composer
                    // together: Commit is painted where the body shows it.
                    cx.update(|_, cx| {
                        let scroll = app.read(cx).working_body.scroll().clone();
                        scroll.set_offset(point(px(0.), -scroll.max_offset().y));
                    });
                    draw(cx);
                    if !painted_fills(cx, "commit-staged").contains(&accent) {
                        failures.push(format!("{at}: Commit is not filled"));
                    }
                }
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        drop(fixture);
    }
}

#[cfg(test)]
mod git_actions_text_tests {
    use super::GIT_ACTIONS_TEXT_BUILT;
    use crate::native_accessibility::written;
    use crate::views::tests::{draw, history_window, hold_quiet_reads_for_test};
    use crate::*;
    use core::prelude::v1::test;

    /// The Git action bar builds its text only when what it shows changes
    /// ([`super::GitActionsInputs`]): frames at rest build none of it, where
    /// every frame formatted the edge's lines or the counts and their
    /// tooltip, Fetch's, Pull's and Push's tooltips and names, the operation
    /// in progress and the branch and target they name, and copied both
    /// Targets fields twice, only to draw the same text. A remote chosen in
    /// Targets, an upstream with counts, changed counts, another remote
    /// branch typed in Targets, a switched branch, a running write once it
    /// looks busy, an operation in progress and a detached HEAD each build
    /// it once, in the frame that shows them, and the bar draws the new
    /// lines, tooltips and names; a write that does not look busy yet shows
    /// nothing new and builds nothing, its end finds the text from before it
    /// kept, and the compact form, which draws the counts' and the actions'
    /// other forms, builds nothing either. What only the edge reads builds
    /// it once alone, the line following: a detached HEAD at another commit,
    /// a remote configured where Targets names none, and an unborn branch's
    /// first commit.
    #[gpui::test]
    async fn the_git_actions_build_their_text_only_when_what_they_show_changes(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, cx) = history_window(cx).await;
        hold_quiet_reads_for_test(&app, cx);
        cx.simulate_resize(size(px(1480.), px(800.)));
        draw(cx);
        let built = || GIT_ACTIONS_TEXT_BUILT.with(std::cell::Cell::get);
        // The text built by frames at rest.
        let at_rest = |cx: &mut VisualTestContext| {
            let before = built();
            for _ in 0..3 {
                app.update(cx, |_, cx| cx.notify());
                draw(cx);
            }
            built() - before
        };
        // The text built by `update` and the frames that draw it.
        let builds =
            |cx: &mut VisualTestContext,
             update: &dyn Fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>)| {
                let before = built();
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| {
                        update(app, window, cx);
                        cx.notify()
                    })
                });
                draw(cx);
                built() - before
            };
        // Each change builds the text once, in the frame that draws it, and
        // frames at rest after it build none.
        let change =
            |what: &str,
             cx: &mut VisualTestContext,
             update: &dyn Fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>)| {
                assert_eq!(builds(cx, update), 1, "{what}: the text is built once");
                assert_eq!(at_rest(cx), 0, "{what}: and not again at rest");
            };
        fn status(app: &mut GitTurtle) -> &mut gitturtle_core::RepositoryStatus {
            Arc::make_mut(app.work_status.as_mut().expect("the status is read"))
        }
        let tooltip = |id: &str| {
            crate::views::CONTROL_TOOLTIPS.with(|tooltips| tooltips.borrow().get(id).cloned())
        };
        let actions = || ["fetch", "pull", "push"].map(|id| tooltip(id).unwrap_or_default());
        let summary = || {
            written::get("branch-sync-state").and_then(|line| line.node.value().map(str::to_owned))
        };
        let edge = || crate::empty_state::shown("branch-edge").map(|line| line.to_string());
        let explanation = || {
            written::get("branch-edge-line")
                .and_then(|line| line.node.description().map(str::to_owned))
        };
        assert_eq!(at_rest(cx), 0, "frames at rest build none of the text");
        assert_eq!(edge().as_deref(), Some("No remote"));
        assert_eq!(
            actions(),
            ["No configured remote selected · Check Targets"; 3],
            "no remote: each action says so"
        );

        change("a remote", cx, &|app, window, cx| {
            app.remotes.push(gitturtle_core::Remote {
                name: "origin".into(),
                url: "https://example.invalid/fixture.git".into(),
                push_url: "https://example.invalid/fixture.git".into(),
            });
            app.remote_name
                .update(cx, |input, cx| input.set_value("origin", window, cx));
            app.remote_branch
                .update(cx, |input, cx| input.set_value("main", window, cx));
        });
        assert_eq!(edge().as_deref(), Some("No upstream · Push sets one"));
        assert_eq!(
            actions(),
            [
                "Fetch from origin",
                "Pull origin/main into main · fast-forward only",
                "Push main to origin/main",
            ],
            "a remote: each action names its target"
        );

        change("an upstream with counts", cx, &|app, _, _| {
            let status = status(app);
            status.upstream = Some("origin/main".into());
            status.ahead = 2;
            status.behind = 1;
        });
        assert!(
            cx.debug_bounds("branch-edge-line").is_none(),
            "the counts take the edge's place"
        );
        assert_eq!(summary().as_deref(), Some("2 ahead, 1 behind"));
        assert_eq!(
            tooltip("branch-sync-state").as_deref(),
            Some("Compared with local origin/main. Fetch to update remote information.")
        );

        change("changed counts", cx, &|app, _, _| status(app).ahead = 3);
        assert_eq!(summary().as_deref(), Some("3 ahead, 1 behind"));

        change("another remote branch", cx, &|app, window, cx| {
            app.remote_branch
                .update(cx, |input, cx| input.set_value("  release ", window, cx));
        });
        assert_eq!(
            actions()[1..],
            [
                "Pull origin/release into main · fast-forward only",
                "Push main to origin/release",
            ],
            "another remote branch: Pull and Push name it, trimmed"
        );

        change("a switched branch", cx, &|app, _, _| {
            status(app).branch = Some("feature/x".into())
        });
        assert_eq!(
            actions()[1..],
            [
                "Pull origin/release into feature/x · fast-forward only",
                "Push feature/x to origin/release",
            ],
            "a switched branch: Pull and Push name it"
        );

        assert_eq!(
            builds(cx, &|app, _, _| app.operation_busy =
                Some("Fetching remote…")),
            0,
            "a write that does not look busy yet shows nothing new"
        );
        let before = built();
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        draw(cx);
        assert!(
            app.read_with(cx, |app, _| app.operation_shown().is_some()),
            "the write looks busy"
        );
        assert_eq!(
            built() - before,
            1,
            "a running write: the text is built once"
        );
        assert_eq!(at_rest(cx), 0, "a running write: and not again at rest");
        assert_eq!(actions(), ["Fetching remote…"; 3]);
        assert_eq!(
            builds(cx, &|app, _, _| app.operation_busy = None),
            0,
            "the write's end finds the text it had before"
        );
        assert_eq!(actions()[0], "Fetch from origin");

        change("an operation in progress", cx, &|app, _, _| {
            status(app).operation = Some("Merge".into())
        });
        let operation = cx
            .debug_bounds("git-operation-state")
            .expect("the bar draws the operation");
        change("another operation", cx, &|app, _, _| {
            status(app).operation = Some("Cherry-pick".into())
        });
        assert!(
            cx.debug_bounds("git-operation-state")
                .is_some_and(|wider| wider.size.width > operation.size.width),
            "the bar draws the new operation's words"
        );

        // The compact bar draws the counts' and the actions' other forms,
        // built with the rest.
        let before = built();
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        assert_eq!(built(), before, "the compact form builds nothing");
        assert_eq!(
            tooltip("fetch").as_deref(),
            Some("Fetch · Fetch from origin"),
            "the compact Fetch leads its tooltip with its label"
        );
        assert_eq!(
            crate::views::COMPACT_LABELS
                .with(|names| names.borrow().get("push").cloned())
                .as_deref(),
            Some("Push: Push feature/x to origin/release")
        );
        assert_eq!(
            tooltip("branch-sync-state").as_deref(),
            Some(
                "3 ahead, 1 behind · Compared with local origin/main. Fetch to update remote information."
            ),
            "the compact counts lead their tooltip with the counts"
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        cx.simulate_resize(size(px(1480.), px(800.)));
        draw(cx);

        let head = app.read_with(cx, |app, _| {
            app.work_status
                .as_ref()
                .and_then(|status| status.head.clone())
                .expect("the fixture's commit")
        });
        change("a detached HEAD", cx, &|app, _, _| {
            status(app).branch = None
        });
        assert_eq!(edge(), Some(format!("Detached at {}", &head[..7])));
        assert_eq!(
            explanation(),
            Some(format!(
                "HEAD is at commit {} with no branch. Create a branch here to keep new commits on it.",
                &head[..7]
            ))
        );
        assert_eq!(
            actions()[1..],
            ["Switch to a local branch before pulling or pushing"; 2],
            "a detached HEAD: Pull and Push say why they wait"
        );

        // The edge's own inputs, each changed alone: which commit a
        // detached HEAD is at, whether any remote is configured where
        // Targets names none of them, and whether the branch has a commit.
        let other = "0123456789abcdef0123456789abcdef01234567";
        change("detached at another commit", cx, &|app, _, _| {
            status(app).head = Some(other.into())
        });
        assert_eq!(edge(), Some(format!("Detached at {}", &other[..7])));

        change("an unborn branch with no remote", cx, &|app, window, cx| {
            let status = status(app);
            status.branch = Some("main".into());
            status.head = None;
            status.upstream = None;
            app.remotes.clear();
            app.remote_name
                .update(cx, |input, cx| input.set_value("elsewhere", window, cx));
        });
        assert_eq!(edge().as_deref(), Some("No remote"));

        change("a remote Targets does not name", cx, &|app, _, _| {
            app.remotes.push(gitturtle_core::Remote {
                name: "origin".into(),
                url: "https://example.invalid/fixture.git".into(),
                push_url: "https://example.invalid/fixture.git".into(),
            });
        });
        assert_eq!(
            edge().as_deref(),
            Some("No commits to push yet"),
            "a remote, though Targets names another: no longer No remote"
        );

        change("the branch's first commit", cx, &|app, _, _| {
            status(app).head = Some(head.clone())
        });
        assert_eq!(
            edge().as_deref(),
            Some("No upstream · Push sets one"),
            "a commit to push: no longer No commits to push yet"
        );
    }
}

#[cfg(test)]
mod draft_autosave_tests {
    use crate::{
        GitTurtle,
        operations::SerialExecutor,
        preferences::{CommitDraft, Preferences},
        workspace::REFUSED_SAVE_RETRY,
    };
    use gpui_kit::{Entity, TestAppContext, VisualTestContext, gpui};
    use std::path::Path;

    fn writer_answers(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        app.read_with(cx, |app, _| app.preferences_writer.drain());
        cx.executor().run_until_parked();
    }

    fn typed(app: &Entity<GitTurtle>, worktree: &Path, title: &str, cx: &mut VisualTestContext) {
        let draft = CommitDraft {
            title: title.into(),
            description: String::new(),
        };
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.save_commit_draft(worktree.to_owned(), draft, window, cx)
            })
        });
        cx.executor().run_until_parked();
    }

    fn error(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> Option<String> {
        app.read_with(cx, |app, _| app.draft_save_error.clone())
    }

    fn stored_title(worktree: &Path) -> Option<String> {
        Preferences::load()
            .commit_drafts
            .get(worktree)
            .map(|draft| draft.title.clone())
    }

    /// Typing saves the draft by itself, so a preference writer that is
    /// merely behind is not a failure: a queue other saves have filled
    /// refuses the save in silence and it asks again until the newest draft
    /// is stored, where a refusal once read as a failed save beneath the
    /// commit message and lost the last keystroke's save.
    #[gpui::test]
    async fn a_writer_that_is_behind_never_fails_a_draft_save(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        writer_answers(&app, cx);
        let worktree = fixture.path().join("worktree");

        let held = app.read_with(cx, |app, _| app.preferences_writer.hold_reached());
        let fillers: Vec<_> = (0..8)
            .map(|_| app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(()))))
            .collect();
        for mut filler in fillers {
            assert!(matches!(filler.try_recv(), Ok(None)), "the queue fills");
        }
        typed(&app, &worktree, "Earlier title", cx);
        typed(&app, &worktree, "Newest title", cx);
        assert_eq!(error(&app, cx), None);
        // Still full: refused again, and still nothing to report.
        cx.executor().advance_clock(REFUSED_SAVE_RETRY);
        cx.executor().run_until_parked();
        assert_eq!(error(&app, cx), None);

        drop(held);
        writer_answers(&app, cx);
        assert_eq!(stored_title(&worktree), None, "only asking again brings it");
        cx.executor().advance_clock(REFUSED_SAVE_RETRY);
        cx.executor().run_until_parked();
        writer_answers(&app, cx);
        assert_eq!(
            stored_title(&worktree).as_deref(),
            Some("Newest title"),
            "the newest draft reaches the store"
        );
        assert_eq!(error(&app, cx), None);
    }

    /// A writer whose thread has stopped can never take the save, so asking
    /// again could not help: the composer says so.
    #[gpui::test]
    async fn a_stopped_writer_still_reports_a_draft_save(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        writer_answers(&app, cx);
        cx.update(|_, cx| {
            app.update(cx, |app, _| {
                app.preferences_writer = SerialExecutor::stopped("stopped");
            })
        });
        typed(&app, &fixture.path().join("worktree"), "Title", cx);
        let error = error(&app, cx).expect("the failure is reported");
        assert!(error.contains("Background executor"), "{error}");
    }
}

#[cfg(test)]
mod write_outcome_tests {
    use crate::native_accessibility::written;
    use crate::views::tests::{draw, fixture_git, fixture_git_output, history_window_from, settle};
    use crate::*;
    use core::prelude::v1::test;

    /// A `git fast-import` commit on `branch` adding `file`, marked `mark`,
    /// from `from` where it starts a branch.
    fn commit(branch: &str, mark: usize, from: Option<usize>, file: &str) -> String {
        let from = from.map_or(String::new(), |from| format!("from :{from}\n"));
        format!(
            "commit refs/heads/{branch}\nmark :{mark}\ncommitter Fixture <fixture@example.invalid> {} +0000\ndata {}\n{file}\n{from}M 100644 inline {file}.txt\ndata {}\n{file}\n\n",
            1_700_000_000 + mark,
            file.len(),
            file.len() + 1,
        )
    }

    /// A rebase in the app reports its outcome line alone, in the status
    /// line, its accessible value and the notice banner: Git writes each
    /// step's progress before it on one line, "Rebasing (1/3)\rRebasing
    /// (2/3)\rRebasing (3/3)\rSuccessfully rebased and updated
    /// refs/heads/main.", and a terminal leaves only the outcome
    /// (`gitturtle_core::GitRepository::execute`). QA-106e at 18 pt in a
    /// 461 × 490 half tile found the status reading "Rebasing (1/3)Rebasing
    /// (2/3)Rebasing (3/3)Successfully rebased…", the bar drawing
    /// "Rebasing (1/3) Reb…" and the banner wrapping all three steps before
    /// the outcome.
    #[gpui::test]
    async fn a_rebase_reports_its_outcome_without_its_progress(cx: &mut TestAppContext) {
        const OUTCOME: &str = "Successfully rebased and updated refs/heads/main.";
        let stream = [
            commit("main", 1, None, "base"),
            commit("side", 2, Some(1), "side"),
            commit("main", 3, None, "one"),
            commit("main", 4, None, "two"),
            commit("main", 5, None, "three"),
        ]
        .concat()
            + "done\n";
        let (fixture, app, cx) = history_window_from(cx, stream).await;
        let repository = fixture.path().join("narrow");
        fixture_git(&repository, &["reset", "--hard", "--quiet"]);
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        settle(&app, cx).await;
        let plan = app
            .read_with(cx, |app, _| app.repository.clone())
            .expect("a repository")
            .integration_plan("side")
            .expect("a plan to rebase onto side");
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.write(
                    gitturtle_core::WriteCommand::Integration(
                        gitturtle_core::IntegrationCommand::Rebase { plan },
                    ),
                    "Rebasing…",
                    window,
                    cx,
                )
            })
        });
        let rebase = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the rebase");
        rebase.await;
        settle(&app, cx).await;
        let log = fixture_git_output(&repository, &["log", "--format=%s"]);
        assert_eq!(
            String::from_utf8_lossy(&log.stdout),
            "three\ntwo\none\nside\nbase\n",
            "Git replayed three commits"
        );
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_notice.clone())
                .as_deref(),
            Some(OUTCOME),
            "the banner's text and accessible name"
        );
        for (points, width, height) in [(18, 461., 490.), (13, 1480., 973.)] {
            cx.update(|window, cx| appearance::apply_text_sizes(points, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            written::forget();
            draw(cx);
            let at = format!("{points} pt in {width} × {height}");
            let value =
                written::get("status-line").and_then(|line| line.node.value().map(str::to_owned));
            assert_eq!(
                value.as_deref(),
                Some(OUTCOME),
                "{at}: its accessible value"
            );
            let drawn =
                cx.update(|window, cx| app.update(cx, |app, cx| app.status_line_drawn(window, cx)));
            let kept = drawn.strip_suffix('…').unwrap_or(&drawn);
            assert!(
                !kept.is_empty() && OUTCOME.starts_with(kept),
                "{at}: the line draws the outcome, {drawn:?}"
            );
            if width > 1000. {
                assert_eq!(drawn, OUTCOME, "{at}: whole");
            }
            assert!(
                cx.debug_bounds("operation-notice-text").is_some(),
                "{at}: the banner is drawn"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }
}
