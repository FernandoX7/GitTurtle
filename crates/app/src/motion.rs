//! Motion with intent: the short, reduced-motion-aware transitions that
//! explain a change the user caused (DESIGN.md, Motion).
//!
//! One transition lives here: a Working Changes row that its own Stage or
//! Unstage moves between Staged and Unstaged travels from where it was to
//! its new place, so the eye follows it. The list's layout is final in the
//! first frame: every other row takes its new place at once, and the
//! travelling row's slot is an empty placeholder that records its bounds and
//! the list's clip while the list is prepainted ([`slot`]). The row itself is
//! drawn by [`TravellingRow`], beside the list inside the same pane, above
//! the rows it passes, beneath the section headings, which are not rows and
//! stay whole on top, and under the list's clip, from where it was towards
//! that slot. Under reduced motion nothing departs, so the row is in its
//! slot in the first frame.

use crate::*;
use gitturtle_core::{ChangeArea, RepositoryStatus};
use std::{
    cell::{Cell, RefCell},
    path::Path,
    rc::Rc,
    time::Duration,
};

/// How long a row takes to travel between Staged and Unstaged.
pub(crate) const ROW_TRAVEL: Duration = Duration::from_millis(150);

/// The ease-out curve every transition takes: fast from its start, settling
/// into its end.
fn ease_out(progress: f32) -> f32 {
    gpui_kit::base::animation::ease_out_cubic(progress)
}

/// A Working Changes row's move between Staged and Unstaged after the user
/// staged or unstaged it.
#[derive(Default)]
pub(crate) enum RowTravel {
    #[default]
    Idle,
    /// Stage or Unstage was asked for this row; the rows the write produces
    /// have not arrived.
    Departed(Departure),
    /// The row arrived at `position` and travels there from its departure
    /// until `_settle` ends the flight; replacing the flight drops it.
    Flying {
        departure: Departure,
        position: usize,
        started: std::time::Instant,
        _settle: Task<()>,
    },
}

/// Where a row stood when the user staged or unstaged it.
pub(crate) struct Departure {
    repository: PathBuf,
    path: PathBuf,
    from: ChangeArea,
    /// The top of the row in the window, where the eye left it. Feedback
    /// that the write adds above the list can move the list itself, so the
    /// row travels from this place rather than from its old slot.
    top: Pixels,
}

impl Departure {
    fn to(&self) -> ChangeArea {
        match self.from {
            ChangeArea::Staged => ChangeArea::Unstaged,
            ChangeArea::Unstaged => ChangeArea::Staged,
        }
    }
}

/// One frame of a flight.
#[derive(Clone)]
pub(crate) struct Flight {
    /// The row's index in the list, where it lands.
    pub(crate) position: usize,
    /// The top of the row in the window when it departed.
    departed: Pixels,
    /// How far along the flight is, eased: 0 where it departed, 1 landed.
    eased: f32,
    pub(crate) slot: Slot,
}

/// Where the pointer pressed a row's own Stage or Unstage, or a heading's
/// Stage all or Unstage all, until it moves. The rows that write produces
/// put another row, or a heading, where the pressed row stood, and its
/// control would light up under the still pointer and show its tooltip
/// though the person never moved to it. Once they do, what arrived there
/// shows neither, and takes no press from the pointer
/// ([`GitTurtle::passes_over_press`]), until the pointer moves
/// ([`StillPointer::moved_to`]).
pub(crate) struct StillPointer {
    /// Where the pointer came to rest.
    at: Point<Pixels>,
    /// The pressed row: its place in the list, its path, which a heading
    /// does not have, and its group.
    position: usize,
    path: Option<PathBuf>,
    area: ChangeArea,
    /// The list's new rows put something else under the pointer.
    moved: bool,
}

/// How far along either axis, in logical pixels, a pointer may be from where
/// it came to rest and still not have moved ([`StillPointer::moved_to`]): the
/// same at every display scale and text size. A second click in place is
/// never on the same pixel, since a hand drifts a pixel between two clicks
/// and Wayland reports fractions of one, so a rule that any change of place
/// ends would not protect the person it was written for. The square this
/// reaches is the Linux platform's own click-distance square,
/// `DOUBLE_CLICK_DISTANCE` in the vendored `gpui-pre-linux`
/// (`src/linux/platform.rs`, private to that crate): a press it counts into
/// a multi-click is at most 5 px along each axis from the one before, so "in
/// place" for this rule and "the same click" for the platform are one
/// region, and a double-click's second press is always a press in place.
/// Measured in a straight line, such a press 4 px along and 4 px up was
/// 5.7 px away, counted as moved and pressed the Stage all that had arrived
/// (decided 2026-11-15, DESIGN.md's Motion).
pub(crate) const STILL_POINTER_REACH: Pixels = px(5.);

impl StillPointer {
    /// Whether a pointer at `position` has moved: it is more than
    /// [`STILL_POINTER_REACH`] along either axis from where it came to rest,
    /// not from where its last event left it, so a drift adds up. The one
    /// answer for all
    /// that ask, so a control lights exactly when it takes a press: the rows
    /// as they land, which decide what the hover shows
    /// ([`GitTurtle::rows_moved_under_pointer`]), the list's pointer-move
    /// handler ([`GitTurtle::pointer_moved`]) and a press's own place
    /// ([`GitTurtle::passes_over_press`]). Any change of place at all once
    /// counted, so a second click 1 px from a row's Stage pressed the Stage
    /// all that had taken the row's place and staged every file.
    fn moved_to(&self, position: Point<Pixels>) -> bool {
        let away = position - self.at;
        away.x.abs() > STILL_POINTER_REACH || away.y.abs() > STILL_POINTER_REACH
    }
}

/// The landing slot's bounds and the list's clip, recorded while the list is
/// prepainted, so the row lands exactly where the list puts it, after any
/// scroll the list applied in that frame, and the places of the section
/// headings the list built in that frame, which the row passes beneath.
#[derive(Clone, Default)]
pub(crate) struct Slot {
    bounds: Rc<Cell<Option<SlotBounds>>>,
    headings: Rc<RefCell<Vec<usize>>>,
}

impl Slot {
    /// The list built a section heading at `position` in this frame.
    pub(crate) fn heading(&self, position: usize) {
        self.headings.borrow_mut().push(position);
    }
}

#[derive(Clone, Copy)]
struct SlotBounds {
    slot: Bounds<Pixels>,
    clip: Bounds<Pixels>,
}

/// The row a write moved, found with the rows it produced: where `path` left
/// its `from` group and stands in the other. A row still in `from` did not
/// leave (the write failed, or the file changed again), so it does not
/// travel. Runs with the rows on the worker, never on the UI thread.
pub(crate) fn landing(
    status: &RepositoryStatus,
    rows: &[workspace::WorkingRow],
    path: &Path,
    from: ChangeArea,
) -> Option<usize> {
    let entry = status
        .entries
        .iter()
        .position(|entry| crate::working_selection::same_path(&entry.path, path))?;
    let mut landing = None;
    for (position, row) in rows.iter().enumerate() {
        if let workspace::WorkingRow::File(index, area) = *row
            && index == entry
        {
            if area == from {
                return None;
            }
            landing = Some(position);
        }
    }
    landing
}

/// The landing slot in the list: an empty row of the list's height that
/// records where the list lays it out and the list's clip.
pub(crate) fn slot(slot: Slot, height: Pixels) -> AnyElement {
    canvas(
        move |bounds, window, _| {
            slot.bounds.set(Some(SlotBounds {
                slot: bounds,
                clip: window.content_mask().bounds,
            }))
        },
        |_, _, _, _| {},
    )
    .w_full()
    .h(height)
    .into_any_element()
}

impl GitTurtle {
    /// Records where a row stands as its Stage or Unstage is submitted, so
    /// that it can travel when the rows the write produces arrive. Under
    /// reduced motion nothing departs.
    pub(super) fn depart_working_row(&mut self, position: usize, window: &Window, cx: &App) {
        // A write in flight refuses another (`write_from`), whose row must
        // not take the place of the row that write moves.
        if self.operation_busy.is_some() {
            return;
        }
        self.working_travel = RowTravel::Idle;
        if cx.reduce_motion() {
            return;
        }
        // Rows kept on screen while new ones are prepared do not depart.
        let (Some(repository), Some(&workspace::WorkingRow::File(index, from))) = (
            self.path.clone(),
            self.actionable_working_rows().get(position),
        ) else {
            return;
        };
        let Some(entry) = self
            .work_status
            .as_ref()
            .and_then(|status| status.entries.get(index))
        else {
            return;
        };
        // Where the list last laid the row out: its viewport's top, scrolled,
        // and as many rows down as its index, as the list places its rows.
        let (viewport, scroll) = {
            let scroll = self.working_scroll.0.borrow();
            (scroll.base_handle.bounds(), scroll.base_handle.offset().y)
        };
        let height = window.pixel_snap(px(self.settings.density.file_row_height()));
        self.working_travel = RowTravel::Departed(Departure {
            repository,
            path: entry.path.clone(),
            from,
            top: viewport.top() + scroll + height * position as f32,
        });
    }

    /// Remembers the pointer where the control of the row at `position`, a
    /// file's Stage or Unstage or a heading's Stage all or Unstage all, was
    /// pressed and accepted. The row is named from the rows as drawn, which
    /// the press was aimed at: a row's control still takes a press while the
    /// rows a write produced are prepared, and named from the rows that act
    /// (`actionable_working_rows`, none then) that press forgot the pointer
    /// the write's own press had held, so the Stage all the new rows then
    /// put under it lit up and a third click staged every file.
    pub(super) fn hold_still_pointer(&mut self, position: usize, window: &Window) {
        self.still_pointer = None;
        let (path, area) = match self.working_rows.get(position) {
            Some(&workspace::WorkingRow::File(index, area)) => {
                let Some(entry) = self
                    .shown_working_status()
                    .and_then(|status| status.entries.get(index))
                else {
                    return;
                };
                (Some(entry.path.clone()), area)
            }
            Some(&workspace::WorkingRow::Heading(area, _)) => (None, area),
            _ => return,
        };
        self.still_pointer = Some(StillPointer {
            at: window.mouse_position(),
            position,
            path,
            area,
            moved: false,
        });
    }

    /// New rows were installed. Where the pressed row no longer stands in
    /// its place under the still pointer, what arrived there shows no hover
    /// and no tooltip until the pointer moves, and the tooltip the pressed
    /// control asked for, shown or still waiting out its delay, goes with
    /// it: the window's tooltip keeps a request until its control's hover
    /// ends, which a control no longer drawn never reports, so "Stage
    /// gamma.txt" once showed over epsilon's row a second after the press.
    /// A pointer that has left its place ([`StillPointer::moved_to`]), onto
    /// a control outside the list whose own moves the list does not see, is
    /// no longer still: what it rests on keeps its hover and its tooltip,
    /// which hiding the window's tooltip here once cancelled for good.
    pub(super) fn rows_moved_under_pointer(&mut self, window: &Window, cx: &mut App) {
        if self
            .still_pointer
            .as_ref()
            .is_some_and(|still| still.moved_to(window.mouse_position()))
        {
            self.still_pointer = None;
        }
        let status = self.workspace.work_status.as_ref();
        let Some(still) = self.still_pointer.as_mut() else {
            return;
        };
        let stays = match self.workspace.working_rows.get(still.position) {
            Some(&workspace::WorkingRow::File(index, area)) => {
                area == still.area
                    && status
                        .and_then(|status| status.entries.get(index))
                        .zip(still.path.as_deref())
                        .is_some_and(|(entry, path)| {
                            crate::working_selection::same_path(&entry.path, path)
                        })
            }
            Some(&workspace::WorkingRow::Heading(area, _)) => {
                area == still.area && still.path.is_none()
            }
            _ => false,
        };
        if !stays && !still.moved {
            gpui_kit::component::hide_tooltip(window, cx);
        }
        still.moved |= !stays;
    }

    /// The pointer is at `position`: once it has moved, controls under it
    /// light up again. A frame's synthetic move, at the same place, does not
    /// count, nor does a hand's drift within [`STILL_POINTER_REACH`] of
    /// where the pointer came to rest ([`StillPointer::moved_to`]).
    pub(super) fn pointer_moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if let Some(still) = &self.still_pointer
            && still.moved_to(position)
        {
            let moved = still.moved;
            self.still_pointer = None;
            if moved {
                cx.notify();
            }
        }
    }

    /// Whether the list's rows moved under a pointer that has not moved
    /// since it pressed a row's Stage or Unstage or a heading's Stage all or
    /// Unstage all.
    pub(super) fn still_over_moved_rows(&self) -> bool {
        self.still_pointer.as_ref().is_some_and(|still| still.moved)
    }

    /// Whether `press` on a control of the list that starts a write, a
    /// row's Stage or Unstage or a heading's Stage all or Unstage all, is
    /// passed over: a pointer's press on what the rows moved under a pointer
    /// that has not moved since, whatever the click's count. The press asks
    /// what the hover asks ([`Self::still_over_moved_rows`]), so a control
    /// takes a pointer's press exactly when the list lights it: the person
    /// aimed at what stood there before, and what arrived writes nothing
    /// until the pointer moves. Passed over only as a double-click's second
    /// press, a second click half a second after a row's Stage, past the
    /// double-click interval, pressed the Stage all that had taken the row's
    /// place and staged every file. A press says where the pointer is, as a
    /// move does ([`Self::pointer_moved`], and by the same distance,
    /// [`StillPointer::moved_to`]): one made more than
    /// [`STILL_POINTER_REACH`] from where the pointer rested comes from a
    /// pointer that has moved, whether or not the list saw it move, and is
    /// taken, and one nearer is a click in place, passed over as a press on
    /// the very pixel is. Enter and Space aim at the
    /// control holding focus, wherever the pointer rests, and are never
    /// passed over, nor is an assistive technology's Click, which names its
    /// control and reaches these handlers as the keyboard's click
    /// (`workspace::PressedThrough`).
    pub(super) fn passes_over_press(&mut self, press: &ClickEvent, cx: &mut Context<Self>) -> bool {
        if press.is_keyboard() {
            return false;
        }
        self.pointer_moved(press.position(), cx);
        self.still_over_moved_rows()
    }

    /// The departed row the next rows should look for, in this repository.
    /// A departure from another repository is dropped.
    pub(super) fn working_departure(&mut self) -> Option<(PathBuf, ChangeArea)> {
        match &self.working_travel {
            RowTravel::Departed(departure) if self.path.as_ref() == Some(&departure.repository) => {
                Some((departure.path.clone(), departure.from))
            }
            RowTravel::Departed(_) => {
                self.working_travel = RowTravel::Idle;
                None
            }
            _ => None,
        }
    }

    /// The list is being rebuilt: a flight's position no longer names its
    /// row, so it ends. A departure waits for the new rows.
    pub(super) fn interrupt_working_travel(&mut self) {
        if matches!(self.working_travel, RowTravel::Flying { .. }) {
            self.working_travel = RowTravel::Idle;
        }
    }

    /// New rows arrived. A departed row that landed starts its flight; one
    /// that did not land once its write has finished stays where the rows
    /// put it, so a later refresh never moves it.
    pub(super) fn land_working_row(&mut self, landing: Option<usize>, cx: &mut Context<Self>) {
        // A flight is not a departure: it ends when the list is rebuilt
        // (`interrupt_working_travel`) or when it settles.
        let departure = match std::mem::take(&mut self.working_travel) {
            RowTravel::Departed(departure) => departure,
            travel => {
                self.working_travel = travel;
                return;
            }
        };
        let Some(position) = landing.filter(|_| !cx.reduce_motion()) else {
            if self.operation_busy.is_some() && landing.is_none() {
                self.working_travel = RowTravel::Departed(departure);
            }
            return;
        };
        // The flight's end draws the row in its slot, whatever frames the
        // window drew on the way.
        let settle = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ROW_TRAVEL).await;
            let _ = this.update(cx, |this, cx| {
                this.working_travel = RowTravel::Idle;
                cx.notify();
            });
        });
        self.working_travel = RowTravel::Flying {
            departure,
            position,
            started: cx.background_executor().now(),
            _settle: settle,
        };
    }

    /// This frame of the row's flight, if one is under way in the shown
    /// repository and the row at its landing is still the one that left.
    pub(super) fn working_flight(&self, cx: &App) -> Option<Flight> {
        let RowTravel::Flying {
            departure,
            position,
            started,
            ..
        } = &self.working_travel
        else {
            return None;
        };
        if cx.reduce_motion() || self.path.as_ref() != Some(&departure.repository) {
            return None;
        }
        let workspace::WorkingRow::File(index, area) = *self.working_rows.get(*position)? else {
            return None;
        };
        if area != departure.to()
            || self.work_status.as_ref()?.entries.get(index)?.path != departure.path
        {
            return None;
        }
        let elapsed = cx
            .background_executor()
            .now()
            .saturating_duration_since(*started);
        let progress = elapsed.as_secs_f32() / ROW_TRAVEL.as_secs_f32();
        (progress < 1.).then(|| Flight {
            position: *position,
            departed: departure.top,
            eased: ease_out(progress),
            slot: Slot::default(),
        })
    }
}

/// The part of a travelling row's band, from `top` to `bottom` in the
/// window, that no section heading covers, each heading given by its top
/// and bottom. A heading is not a row: the row passes beneath it, so the
/// heading stays whole and on top. Every row of the list is as tall as the
/// travelling row, so a heading covers its top or its bottom and what
/// remains is one band, empty where a heading covers it all.
fn uncovered(
    mut top: Pixels,
    mut bottom: Pixels,
    headings: impl IntoIterator<Item = (Pixels, Pixels)>,
) -> (Pixels, Pixels) {
    for (heading_top, heading_bottom) in headings {
        if heading_top >= bottom || heading_bottom <= top {
            continue;
        }
        if heading_top <= top {
            top = top.max(heading_bottom);
        } else if heading_bottom >= bottom {
            bottom = bottom.min(heading_top);
        } else if heading_top - top >= bottom - heading_bottom {
            // A heading shorter than the row: the larger part shows.
            bottom = heading_top;
        } else {
            top = heading_bottom;
        }
    }
    (top, bottom.max(top))
}

/// Draws a travelling row over the list, from where it departed to the slot
/// the list laid out for it, under the list's clip and beneath the section
/// headings ([`uncovered`]). It takes no room of its own. Where the list did
/// not lay the slot out because it lies outside the list's viewport, nothing
/// is drawn and no further frame is asked for.
pub(crate) struct TravellingRow {
    flight: Flight,
    row: AnyElement,
}

impl TravellingRow {
    pub(crate) fn new(flight: Flight, row: AnyElement) -> Self {
        Self { flight, row }
    }
}

impl IntoElement for TravellingRow {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for TravellingRow {
    type RequestLayoutState = ();
    /// The list's clip less the section headings, where the row was laid
    /// out.
    type PrepaintState = Option<Bounds<Pixels>>;

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
            ..Style::default()
        };
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        let SlotBounds { slot, clip } = self.flight.slot.bounds.get()?;
        let departed = self.flight.departed;
        let top = departed + (slot.origin.y - departed) * self.flight.eased;
        // The list lays every row out at the slot's height, so a heading `n`
        // rows from the slot lies `n` slot heights from it.
        let height = slot.size.height;
        let headings = self.flight.slot.headings.borrow();
        let (shown_top, shown_bottom) = uncovered(
            top,
            top + height,
            headings.iter().map(|&heading| {
                let at = slot.origin.y + height * (heading as f32 - self.flight.position as f32);
                (at, at + height)
            }),
        );
        let clip = Bounds::from_corners(
            point(clip.left(), shown_top),
            point(clip.right(), shown_bottom),
        )
        .intersect(&clip);
        self.row.layout_as_root(
            size(
                AvailableSpace::Definite(slot.size.width),
                AvailableSpace::Definite(slot.size.height),
            ),
            window,
            cx,
        );
        window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
            self.row.prepaint_at(point(slot.origin.x, top), window, cx);
        });
        window.request_animation_frame();
        Some(clip)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        clip: &mut Option<Bounds<Pixels>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(clip) = *clip else {
            return;
        };
        window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
            self.row.paint(window, cx);
        });
    }
}

#[cfg(test)]
#[path = "motion_tests.rs"]
mod tests;
