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

/// Where the pointer pressed a row's own Stage or Unstage, until it moves.
/// The rows that write produces put another row, or a heading, where the
/// pressed row stood, and its control would light up under the still
/// pointer and show its tooltip though the person never moved to it. Once
/// they do, what arrived there shows neither until the pointer moves.
pub(crate) struct StillPointer {
    at: Point<Pixels>,
    /// The pressed row: its place in the list, its path and its group.
    position: usize,
    path: PathBuf,
    area: ChangeArea,
    /// The list's new rows put something else under the pointer.
    moved: bool,
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
    let entry = status.entries.iter().position(|entry| entry.path == path)?;
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

    /// Remembers the pointer where the row at `position`'s own Stage or
    /// Unstage was pressed and accepted.
    pub(super) fn hold_still_pointer(&mut self, position: usize, window: &Window) {
        self.still_pointer = None;
        let Some(&workspace::WorkingRow::File(index, area)) =
            self.actionable_working_rows().get(position)
        else {
            return;
        };
        let Some(entry) = self
            .work_status
            .as_ref()
            .and_then(|status| status.entries.get(index))
        else {
            return;
        };
        self.still_pointer = Some(StillPointer {
            at: window.mouse_position(),
            position,
            path: entry.path.clone(),
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
    /// A pointer that has left its place, onto a control outside the list
    /// whose own moves the list does not see, is no longer still: what it
    /// rests on keeps its hover and its tooltip, which hiding the window's
    /// tooltip here once cancelled for good.
    pub(super) fn rows_moved_under_pointer(&mut self, window: &Window, cx: &mut App) {
        if self
            .still_pointer
            .as_ref()
            .is_some_and(|still| still.at != window.mouse_position())
        {
            self.still_pointer = None;
        }
        let status = self.work_status.as_ref();
        let Some(still) = self.still_pointer.as_mut() else {
            return;
        };
        let stays = match self.working_rows.get(still.position) {
            Some(&workspace::WorkingRow::File(index, area)) => {
                area == still.area
                    && status
                        .and_then(|status| status.entries.get(index))
                        .is_some_and(|entry| entry.path == still.path)
            }
            _ => false,
        };
        if !stays && !still.moved {
            gpui_kit::component::Root::hide_tooltip(window, cx);
        }
        still.moved |= !stays;
    }

    /// The pointer is at `position`: once it has moved, controls under it
    /// light up again. A frame's synthetic move, at the same place, does not
    /// count.
    pub(super) fn pointer_moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if let Some(still) = &self.still_pointer
            && still.at != position
        {
            let moved = still.moved;
            self.still_pointer = None;
            if moved {
                cx.notify();
            }
        }
    }

    /// Whether the list's rows moved under a pointer that has not moved
    /// since it pressed a row's Stage or Unstage.
    pub(super) fn still_over_moved_rows(&self) -> bool {
        self.still_pointer.as_ref().is_some_and(|still| still.moved)
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
mod tests {
    use super::*;
    use crate::views::tests::{draw, fixture_git, history_script, history_window_from};
    // The glob above also brings GPUI's `test` attribute, which the
    // `gpui::test` expansion would otherwise reach for.
    use core::prelude::v1::test;

    /// A 1480 × 980 window in Working Changes on a fixture with alpha.txt and
    /// beta.txt staged and notes.txt edited, so its rows are Staged 2, alpha,
    /// beta, Unstaged 1 and notes, under reduced motion where `reduce`.
    async fn working(
        cx: &mut TestAppContext,
        reduce: bool,
    ) -> (tempfile::TempDir, Entity<GitTurtle>, &mut VisualTestContext) {
        let (fixture, app, cx) = history_window_from(cx, history_script(40, 3)).await;
        cx.update(|_, cx| cx.set_reduce_motion(reduce));
        let root = fixture.path().join("narrow");
        fixture_git(&root, &["reset", "--hard", "--quiet"]);
        std::fs::write(root.join("alpha.txt"), "alpha\n").unwrap();
        std::fs::write(root.join("beta.txt"), "beta\n").unwrap();
        fixture_git(&root, &["add", "alpha.txt", "beta.txt"]);
        std::fs::write(root.join("notes.txt"), "edited\n").unwrap();
        cx.simulate_resize(size(px(1480.), px(980.)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        rows_arrived(&app, cx).await;
        draw(cx);
        assert_eq!(app.read_with(cx, |app, _| app.working_rows.len()), 5);
        // The list shows every row, so each one's place can be compared.
        let pane = cx.debug_bounds("working-pane").unwrap();
        assert!(row(cx, 4).bottom() <= pane.bottom(), "{pane:?}");
        (fixture, app, cx)
    }

    /// Waits for the status read and the rows it produces, without moving
    /// the clock.
    async fn rows_arrived(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        for _ in 0..8 {
            cx.executor().run_until_parked();
            let tasks = app.update(cx, |app, _| {
                [app.task.take(), app.status_task.take()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
            });
            for task in tasks {
                task.await;
            }
            // The rows are built on the path filter's own thread, which
            // `run_until_parked` cannot wait for.
            app.update(cx, |app, _| app.file_paths.executor().drain());
            cx.executor().run_until_parked();
            if !app.read_with(cx, |app, _| app.working_paths.pending) {
                break;
            }
        }
    }

    /// Clicks the Stage or Unstage control of the row at `position` and
    /// waits for the write and the rows it produces.
    async fn toggle(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, position: usize) {
        let selector: &'static str = format!("working-stage-{position}").leak();
        let control = cx.debug_bounds(selector).expect(selector);
        cx.simulate_click(control.center(), Modifiers::default());
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the write");
        write.await;
        rows_arrived(app, cx).await;
    }

    fn row(cx: &mut VisualTestContext, position: usize) -> Bounds<Pixels> {
        let selector: &'static str = format!("working-row-{position}").leak();
        cx.debug_bounds(selector).expect(selector)
    }

    fn flying(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> Option<usize> {
        app.read_with(cx, |app, _| match &app.working_travel {
            RowTravel::Flying { position, .. } => Some(*position),
            _ => None,
        })
    }

    /// Advances the executor's clock by `ms` and draws the frames it asks for.
    fn advance(cx: &mut VisualTestContext, ms: u64) {
        cx.executor().advance_clock(Duration::from_millis(ms));
        cx.executor().run_until_parked();
        draw(cx);
    }

    /// `actual` is `expected` to within the device pixel a row's origin
    /// snaps to while it travels.
    fn assert_near(at: &str, actual: Pixels, expected: Pixels) {
        assert!(
            (actual - expected).abs() < px(0.5),
            "{at}: {actual:?}, expected {expected:?}"
        );
    }

    /// `actual` is the slot `expected`, to within layout's rounding.
    fn assert_slot(at: &str, actual: Bounds<Pixels>, expected: Bounds<Pixels>) {
        for (part, actual, expected) in [
            ("top", actual.top(), expected.top()),
            ("left", actual.left(), expected.left()),
            ("width", actual.size.width, expected.size.width),
            ("height", actual.size.height, expected.size.height),
        ] {
            assert!(
                (actual - expected).abs() < px(0.01),
                "{at}: {part} {actual:?}, expected {expected:?}"
            );
        }
    }

    /// Staging notes.txt moves its row from the foot of Unstaged to the foot
    /// of Staged. Its first frame draws it where the user left it, though
    /// the write's feedback may have moved the list; every other row is
    /// already in its new place. Halfway through the 150 ms it has covered
    /// the ease-out's 87.5%; a frame before the end it lies on its slot; and
    /// at 150 ms it is drawn in the list itself, in exactly the slot below
    /// beta, with no flight left. Unstaging alpha then sends its row the
    /// other way, from the top of Staged to the foot of Unstaged.
    #[gpui::test]
    async fn a_staged_row_travels_to_its_new_place(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = working(cx, false).await;
        let notes = row(cx, 4);
        toggle(&app, cx, 4).await;
        assert_eq!(flying(&app, cx), Some(3));
        draw(cx);
        // The list's layout is final: beta and the Unstaged heading keep
        // their slots, and the row is drawn where it stood, about to rise.
        let beta = row(cx, 2);
        let (height, slot) = (beta.size.height, beta.bottom());
        assert_near("the heading", row(cx, 4).top(), slot + height);
        let start = row(cx, 3);
        assert_slot(
            "the first frame",
            start,
            Bounds::new(point(beta.left(), notes.top()), beta.size),
        );
        assert_ne!(start.top(), slot);
        advance(cx, 75);
        assert_near(
            "halfway",
            row(cx, 3).top(),
            notes.top() + (slot - notes.top()) * ease_out(0.5),
        );
        // A drawn flight asks for the next frame.
        assert!(cx.update(|window, cx| window.simulate_next_frame(cx)) > 0);
        advance(cx, 74);
        assert_eq!(flying(&app, cx), Some(3));
        let landed = Bounds::new(point(beta.left(), slot), beta.size);
        let late = row(cx, 3);
        assert_near("a frame before the end", late.top(), slot);
        assert_slot("a frame before the end", late, landed);
        advance(cx, 1);
        assert_eq!(flying(&app, cx), None);
        assert_slot("landed", row(cx, 3), landed);
        // Landed, it asks for none.
        assert_eq!(cx.update(|window, cx| window.simulate_next_frame(cx)), 0);

        // [Staged 3, alpha, beta, notes, Unstaged 0]: alpha leaves Staged
        // for the foot of Unstaged, three rows below.
        let alpha = row(cx, 1);
        toggle(&app, cx, 1).await;
        assert_eq!(flying(&app, cx), Some(4));
        draw(cx);
        // [Staged 2, beta, notes, Unstaged 1, alpha]
        let slot = row(cx, 3).bottom();
        assert_near("alpha's first frame", row(cx, 4).top(), alpha.top());
        advance(cx, 75);
        assert_near(
            "alpha halfway",
            row(cx, 4).top(),
            alpha.top() + (slot - alpha.top()) * ease_out(0.5),
        );
        advance(cx, 75);
        assert_eq!(flying(&app, cx), None);
        assert_slot(
            "alpha landed",
            row(cx, 4),
            Bounds::new(point(alpha.left(), slot), alpha.size),
        );
    }

    /// What the last frame painted over the heading at row `heading`: each
    /// quad's part inside it, clipped as it is drawn, with its fill and
    /// border, in a stable order. A part thinner than half a device pixel,
    /// an edge two snapped bounds share, is no cover.
    fn painted_over(cx: &mut VisualTestContext, heading: usize) -> Vec<String> {
        let heading = row(cx, heading);
        cx.update(|window, _| {
            let heading = heading.scale(window.scale_factor());
            let mut painted: Vec<_> = window
                .painted_quads()
                .into_iter()
                .filter_map(|quad| {
                    let shown = quad
                        .bounds
                        .intersect(&quad.content_mask.bounds)
                        .intersect(&heading);
                    (shown.size.width.as_f32() > 0.5 && shown.size.height.as_f32() > 0.5).then(
                        || {
                            format!(
                                "{shown:?} {:?} {:?} {:?}",
                                quad.background, quad.border_color, quad.border_widths
                            )
                        },
                    )
                })
                .collect();
            painted.sort();
            painted
        })
    }

    /// How tall a part of the travelling row, drawn at `travelling`, the last
    /// frame painted, in device pixels: the clipped height of the row's own
    /// fill, the quad that spans its bounds under a mask no taller than the
    /// row. Only the travelling row is drawn under such a mask, the band of
    /// the list's clip it may show in; a row or a heading the list lays out
    /// in the same place, as beta takes alpha's in the first frame of
    /// alpha's flight, is drawn under the list's whole clip.
    fn row_shown(cx: &mut VisualTestContext, travelling: Bounds<Pixels>) -> f32 {
        cx.update(|window, _| {
            let travelling = travelling.scale(window.scale_factor());
            let near = |a: gpui_kit::ScaledPixels, b: gpui_kit::ScaledPixels| {
                (a.as_f32() - b.as_f32()).abs() <= 1.
            };
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| {
                    !quad.background.is_transparent()
                        && quad.content_mask.bounds.size.height.as_f32()
                            <= travelling.size.height.as_f32() + 1.
                        && near(quad.bounds.top(), travelling.top())
                        && near(quad.bounds.bottom(), travelling.bottom())
                        && near(quad.bounds.left(), travelling.left())
                        && near(quad.bounds.right(), travelling.right())
                })
                .map(|quad| {
                    quad.bounds
                        .intersect(&quad.content_mask.bounds)
                        .size
                        .height
                        .as_f32()
                })
                .fold(0., f32::max)
        })
    }

    /// How much of the band from `top` to `bottom` lies inside `clip`'s
    /// height and outside every one of `covers`, in device pixels at `scale`.
    fn uncovered_height(
        (top, bottom): (Pixels, Pixels),
        clip: Bounds<Pixels>,
        covers: &[Bounds<Pixels>],
        scale: f32,
    ) -> f32 {
        let mut parts = vec![(top.max(clip.top()), bottom.min(clip.bottom()))];
        for cover in covers {
            parts = parts
                .into_iter()
                .flat_map(|(start, end)| {
                    [
                        (start, end.min(cover.top())),
                        (start.max(cover.bottom()), end),
                    ]
                })
                .filter(|(start, end)| start < end)
                .collect();
        }
        parts
            .iter()
            .fold(0., |sum, (start, end)| sum + f32::from(*end - *start))
            * scale
    }

    /// What [`assert_headings_on_top`] saw: how many frames drew the flight,
    /// the list's viewport, and the place and bounds of each section heading
    /// the list drew.
    struct Flown {
        frames: usize,
        viewport: Bounds<Pixels>,
        headings: Vec<(usize, Bounds<Pixels>)>,
    }

    /// Draws every frame of the flight under way, 16 ms apart, until it
    /// lands, and then the list at rest. In every frame, the first included,
    /// each section heading the list draws paints exactly what it paints at
    /// rest, and the row shows all of itself that lies in the list's
    /// viewport and under no heading: it passes above the rows and beneath
    /// the headings, which stay whole and on top.
    fn assert_headings_on_top(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        at: &str,
    ) -> Flown {
        let scale = cx.update(|window, _| window.scale_factor());
        // The probe counts the frames the window draws, so none goes unread.
        app.update(cx, |app, _| {
            app.working_probe.line.set(workspace::ProbeLine::Watch)
        });
        let mut headings: Vec<usize> = Vec::new();
        let mut frames = Vec::new();
        while let Some(position) = flying(app, cx) {
            let index = frames.len();
            app.read_with(cx, |app, _| app.working_probe.frames.borrow_mut().clear());
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
            let drawn = app.read_with(cx, |app, _| app.working_probe.frames.borrow().len());
            assert_eq!(drawn, 1, "{at}, frame {index}: one frame drawn");
            if index == 0 {
                // The list's layout is final in the first frame.
                let rows = app.read_with(cx, |app, _| app.working_rows.clone());
                headings = (0..rows.len())
                    .filter(|&place| {
                        matches!(rows[place], workspace::WorkingRow::Heading(..))
                            && cx
                                .debug_bounds(format!("working-row-{place}").leak())
                                .is_some()
                    })
                    .collect();
                assert!(!headings.is_empty(), "{at}: the list draws a heading");
            }
            let viewport = app.read_with(cx, |app, _| {
                app.working_scroll.0.borrow().base_handle.bounds()
            });
            let travelling = row(cx, position);
            let covers: Vec<_> = headings.iter().map(|&heading| row(cx, heading)).collect();
            let expected = uncovered_height(
                (travelling.top(), travelling.bottom()),
                viewport,
                &covers,
                scale,
            );
            let shown = row_shown(cx, travelling);
            assert!(
                (shown - expected).abs() <= 1.,
                "{at}, frame {index}: the row at {travelling:?} shows {shown} device px, \
                 expected {expected} in {viewport:?} under the headings {covers:?}"
            );
            let painted: Vec<_> = headings
                .iter()
                .map(|&heading| painted_over(cx, heading))
                .collect();
            frames.push((travelling, viewport, painted));
            cx.executor().advance_clock(Duration::from_millis(16));
            cx.executor().run_until_parked();
        }
        app.update(cx, |app, _| {
            app.working_probe.line.set(workspace::ProbeLine::Off)
        });
        draw(cx);
        let rest: Vec<_> = headings
            .iter()
            .map(|&heading| painted_over(cx, heading))
            .collect();
        for (index, (travelling, _, painted)) in frames.iter().enumerate() {
            for ((heading, painted), rest) in headings.iter().zip(painted).zip(&rest) {
                assert!(
                    painted == rest,
                    "{at}, frame {index} of {}: the row at {travelling:?} covers the heading \
                     at row {heading}, {:?}: painted\n{}\nwhere at rest\n{}",
                    frames.len(),
                    row(cx, *heading),
                    painted.join("\n"),
                    rest.join("\n")
                );
            }
        }
        let viewport = frames.first().map_or_else(Bounds::default, |frame| frame.1);
        Flown {
            frames: frames.len(),
            viewport,
            headings: headings
                .iter()
                .map(|&heading| (heading, row(cx, heading)))
                .collect(),
        }
    }

    /// Draws every frame of the flight under way, 16 ms apart, cuts as
    /// cleanly as [`assert_headings_on_top`], and says how many frames there
    /// were, at least nine of the 150 ms.
    fn assert_flight(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, at: &str) -> Flown {
        let flown = assert_headings_on_top(app, cx, at);
        assert!(flown.frames >= 9, "{at}: flies for {} frames", flown.frames);
        flown
    }

    /// Stages or unstages the row at `position`, as [`toggle`] does, and
    /// checks that it flies to `lands`.
    async fn toggle_flying(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        position: usize,
        lands: usize,
    ) {
        toggle(app, cx, position).await;
        assert_eq!(
            flying(app, cx),
            Some(lands),
            "row {position} lands at {lands}"
        );
    }

    /// Stages or unstages the row at `position` with no flight, as reduced
    /// motion does, to set the list back between sizes.
    async fn toggle_still(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, position: usize) {
        cx.update(|_, cx| cx.set_reduce_motion(true));
        toggle(app, cx, position).await;
        cx.update(|_, cx| cx.set_reduce_motion(false));
        draw(cx);
        assert_eq!(flying(app, cx), None);
    }

    /// The section headings are not rows: a travelling row passes beneath
    /// them, and they stay whole and on top in every frame of its flight, at
    /// 13 and 18 pt. Staging notes.txt draws it first where it stood, which
    /// the Unstaged heading now takes, and it rises from beneath the heading
    /// into its slot; unstaging alpha.txt sends it down past beta and notes
    /// and beneath the Unstaged heading to the foot of the list. The row
    /// was drawn over the heading, covering it whole in the first frame.
    #[gpui::test]
    async fn a_travelling_row_passes_beneath_the_section_headings(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = working(cx, false).await;
        // At 18 pt a taller window keeps every row in view.
        for (text, height) in [(13, 980.), (18, 1300.)] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(1480.), px(height)));
            draw(cx);
            assert!(row(cx, 4).bottom() <= cx.debug_bounds("working-pane").unwrap().bottom());
            // [Staged 2, alpha, beta, Unstaged 1, notes]
            toggle_flying(&app, cx, 4, 3).await;
            // [Staged 3, alpha, beta, notes, Unstaged 0]
            let flown = assert_flight(&app, cx, &format!("{text} pt, notes"));
            assert_eq!(flown.headings.len(), 2);
            toggle_flying(&app, cx, 1, 4).await;
            // [Staged 2, beta, notes, Unstaged 1, alpha]
            assert_flight(&app, cx, &format!("{text} pt, alpha"));
            // Back as it was for the next size.
            toggle_still(&app, cx, 4).await;
            toggle_still(&app, cx, 3).await;
        }
    }

    /// A heading the list's edge cuts stays on top of the travelling row
    /// too, in the part the list shows, at 13 and 18 pt: with eight files
    /// staged and eight not, and the list scrolled so the Unstaged heading
    /// lands 15 px past its top edge, a08 leaves Staged from beneath it
    /// and drops into Unstaged; scrolled so the heading lands 15 px past
    /// its bottom edge, a08 goes back from beneath it into Staged.
    #[gpui::test]
    async fn a_heading_the_lists_edge_cuts_stays_on_top(cx: &mut TestAppContext) {
        let (fixture, app, cx) = history_window_from(cx, history_script(40, 3)).await;
        let root = fixture.path().join("narrow");
        fixture_git(&root, &["reset", "--hard", "--quiet"]);
        for index in 1..=8 {
            for group in ["a", "b"] {
                std::fs::write(root.join(format!("{group}{index:02}.txt")), "file\n").unwrap();
            }
        }
        for index in 1..=8 {
            fixture_git(&root, &["add", &format!("a{index:02}.txt")]);
        }
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        rows_arrived(&app, cx).await;
        // [Staged 8, a01…a08, Unstaged 8, b01…b08]
        assert_eq!(app.read_with(cx, |app, _| app.working_rows.len()), 18);
        let scroll_to = |cx: &mut VisualTestContext, offset: Pixels| {
            app.update(cx, |app, _| {
                app.working_scroll
                    .0
                    .borrow()
                    .base_handle
                    .set_offset(point(px(0.), offset))
            });
            cx.update(|window, _| window.refresh());
            draw(cx);
        };
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(1480.), px(980.)));
            draw(cx);
            let height = row(cx, 0).size.height;
            // The Unstaged heading will take a08's place, 15 px past the
            // list's top edge.
            scroll_to(cx, -(height * 8. + px(15.)));
            toggle_flying(&app, cx, 8, 9).await;
            // [Staged 7, a01…a07, Unstaged 9, a08, b01…b08]
            let flown = assert_flight(&app, cx, &format!("{text} pt, a08 from Staged"));
            let (_, heading) = *flown
                .headings
                .iter()
                .find(|(place, _)| *place == 8)
                .expect("the Unstaged heading is drawn");
            assert!(
                heading.top() < flown.viewport.top() && heading.bottom() > flown.viewport.top(),
                "{text} pt: the list's top edge cuts the heading {heading:?}, {:?}",
                flown.viewport
            );
            // The Unstaged heading will take a08's place, 15 px past the
            // list's bottom edge.
            let viewport = flown.viewport.size.height;
            scroll_to(cx, viewport - height * 10. + px(15.));
            toggle_flying(&app, cx, 9, 8).await;
            // [Staged 8, a01…a08, Unstaged 8, b01…b08]
            let flown = assert_flight(&app, cx, &format!("{text} pt, a08 from Unstaged"));
            let (_, heading) = *flown
                .headings
                .iter()
                .find(|(place, _)| *place == 9)
                .expect("the Unstaged heading is drawn");
            assert!(
                heading.top() < flown.viewport.bottom()
                    && heading.bottom() > flown.viewport.bottom(),
                "{text} pt: the list's bottom edge cuts the heading {heading:?}, {:?}",
                flown.viewport
            );
            scroll_to(cx, px(0.));
        }
    }

    /// With Directories on, a travelling row passes above the directory
    /// rows, which are rows, and beneath the section headings, at 13 and 18
    /// pt: notes.txt rises from below the list's last row and beneath the
    /// Unstaged heading, and alpha.txt drops past beta and notes, beneath
    /// the Unstaged heading and over the repository root's directory row
    /// below it.
    #[gpui::test]
    async fn a_travelling_row_passes_above_directory_rows(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = working(cx, false).await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.working_selection.grouped = true;
                app.filter_working_paths(window, cx);
            })
        });
        rows_arrived(&app, cx).await;
        // A taller window keeps every row in view.
        cx.simulate_resize(size(px(1480.), px(1300.)));
        draw(cx);
        let directory = |app: &Entity<GitTurtle>, cx: &mut VisualTestContext, place: usize| {
            app.read_with(cx, |app, _| {
                matches!(
                    app.working_rows[place],
                    workspace::WorkingRow::Directory(..)
                )
            })
        };
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            draw(cx);
            // [Staged 2, root, alpha, beta, Unstaged 1, root, notes]
            assert_eq!(app.read_with(cx, |app, _| app.working_rows.len()), 7);
            assert!(directory(&app, cx, 1) && directory(&app, cx, 5));
            assert!(row(cx, 6).bottom() <= cx.debug_bounds("working-pane").unwrap().bottom());
            toggle_flying(&app, cx, 6, 4).await;
            // [Staged 3, root, alpha, beta, notes, Unstaged 0]
            assert_flight(&app, cx, &format!("{text} pt, notes"));
            toggle_flying(&app, cx, 2, 6).await;
            // [Staged 2, root, beta, notes, Unstaged 1, root, alpha]
            assert!(directory(&app, cx, 5));
            assert_flight(&app, cx, &format!("{text} pt, alpha"));
            toggle_still(&app, cx, 6).await;
            toggle_still(&app, cx, 4).await;
        }
    }

    /// With a path filter set, staging a row leaves the list as it was until
    /// the filter's new rows land. In the frames before the filter's worker
    /// answers every row stays where it was, the Staged heading included,
    /// no "Filtering working paths…" line opens above them and the commit
    /// action keeps its label; then the row travels from where it stood,
    /// as it does without a filter. The list emptied for those frames, and
    /// the line pushed the heading 21 px down, before the flight began.
    #[gpui::test]
    async fn a_filtered_list_holds_its_rows_until_the_new_ones_land(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = working(cx, false).await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.working_filter
                    .update(cx, |input, cx| input.set_value(".txt", window, cx));
                app.filter_working_paths(window, cx);
            })
        });
        rows_arrived(&app, cx).await;
        draw(cx);
        // Every file matches: [Staged 2, alpha, beta, Unstaged 1, notes].
        assert_eq!(app.read_with(cx, |app, _| app.working_rows.len()), 5);
        let before: Vec<_> = (0..5).map(|position| row(cx, position)).collect();
        let label = app.read_with(cx, |app, cx| app.commit_action_label(cx));

        // The filter's worker waits until the frames before it are drawn.
        let held = app.update(cx, |app, _| app.file_paths.executor().hold());
        let control = cx.debug_bounds("working-stage-4").expect("notes' Stage");
        cx.simulate_click(control.center(), Modifiers::default());
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the write");
        write.await;
        // The write's status read arrives and asks the filter for rows.
        for _ in 0..8 {
            cx.executor().run_until_parked();
            if app.read_with(cx, |app, _| app.working_paths.pending) {
                break;
            }
            let tasks = app.update(cx, |app, _| {
                [app.task.take(), app.status_task.take()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
            });
            for task in tasks {
                task.await;
            }
        }
        draw(cx);
        assert!(app.read_with(cx, |app, _| app.working_paths.pending));
        for (position, before) in before.iter().enumerate() {
            let selector: &'static str = format!("working-row-{position}").leak();
            assert_eq!(
                cx.debug_bounds(selector),
                Some(*before),
                "row {position} before the filter answers"
            );
        }
        assert!(
            cx.debug_bounds("working-filter-pending").is_none(),
            "no filtering line while rows are shown"
        );
        assert_eq!(
            app.read_with(cx, |app, cx| app.commit_action_label(cx)),
            label
        );
        // Their counts stay with them: two staged until the new rows land.
        assert_eq!(
            app.read_with(cx, |app, _| app.working_paths.staged_count),
            2
        );
        // Another read finds the same status meanwhile, as a quiet refresh
        // after the write's own read can: the row still waits for the rows
        // being prepared, which land it.
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        let read = app
            .update(cx, |app, _| app.status_task.take())
            .expect("the read");
        read.await;
        cx.executor().run_until_parked();
        app.read_with(cx, |app, _| {
            assert!(app.working_paths.pending);
            assert!(matches!(app.working_travel, RowTravel::Departed(_)));
        });

        drop(held);
        rows_arrived(&app, cx).await;
        assert_eq!(
            app.read_with(cx, |app, _| app.working_paths.staged_count),
            3
        );
        assert_eq!(flying(&app, cx), Some(3));
        draw(cx);
        // [Staged 3, alpha, beta, notes, Unstaged 0]: the heading has not
        // moved, and notes leaves from where it stood.
        assert_eq!(row(cx, 0), before[0]);
        assert_near("the first frame", row(cx, 3).top(), before[4].top());
        advance(cx, 150);
        assert_eq!(flying(&app, cx), None);
        assert_near("landed", row(cx, 3).top(), before[2].bottom());
    }

    /// A tab left while its list still shows the rows of an earlier status,
    /// before the newer status's rows land, keeps none of them: they index
    /// the earlier status, which the tab does not keep. Returning, the list
    /// shows "Filtering working paths…" until its own rows land, here two
    /// files after notes.txt's edit was discarded outside the app; the kept
    /// rows read the newer status's two entries as three and drew notes from
    /// past their end.
    #[gpui::test]
    async fn a_tab_left_before_new_rows_land_keeps_no_earlier_rows(cx: &mut TestAppContext) {
        let (fixture, app, cx) = working(cx, false).await;
        let root = fixture.path().join("narrow");
        let other = crate::views::tests::fixture_repository(fixture.path().join("other"))
            .path()
            .to_owned();
        let held = app.update(cx, |app, _| app.file_paths.executor().hold());
        fixture_git(&root, &["checkout", "--", "notes.txt"]);
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        for _ in 0..8 {
            cx.executor().run_until_parked();
            if app.read_with(cx, |app, _| app.working_paths.pending) {
                break;
            }
            let task = app.update(cx, |app, _| app.status_task.take());
            if let Some(task) = task {
                task.await;
            }
        }
        draw(cx);
        // The earlier rows stay on screen meanwhile.
        app.read_with(cx, |app, _| {
            assert!(app.working_paths.retains_rows());
            assert_eq!(app.working_rows.len(), 5);
            assert_eq!(app.work_status.as_ref().unwrap().entries.len(), 2);
        });
        assert!(cx.debug_bounds("working-row-4").is_some());

        cx.update(|window, cx| {
            app.update(cx, |app, cx| app.open_repository_tab(other, window, cx))
        });
        drop(held);
        crate::views::tests::settle(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        draw(cx);
        app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::Working);
            assert!(!app.working_paths.retains_rows());
        });
        rows_arrived(&app, cx).await;
        draw(cx);
        // [Staged 2, alpha, beta, Unstaged 0]
        assert_eq!(app.read_with(cx, |app, _| app.working_rows.len()), 4);
        assert!(cx.debug_bounds("working-row-3").is_some());
        assert!(cx.debug_bounds("working-row-4").is_none());
    }

    /// Where one frame laid the list and the composer out, inside their
    /// borders, and whether it drew the probe line above them
    /// (`workspace::WorkingProbe`), under `at`.
    struct Frame {
        at: String,
        line: bool,
        list: Bounds<Pixels>,
        composer: Bounds<Pixels>,
    }

    /// The frames the window drew since the last call, as the probe recorded
    /// them, under `at`.
    fn recorded(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, at: &str) -> Vec<Frame> {
        let drawn =
            cx.read(|cx| std::mem::take(&mut *app.read(cx).working_probe.frames.borrow_mut()));
        drawn
            .into_iter()
            .enumerate()
            .map(|(again, frame)| {
                let at = match again {
                    0 => at.to_owned(),
                    again => format!("{at}, drawn again ({again})"),
                };
                Frame {
                    line: frame.line,
                    list: frame.list.unwrap_or_else(|| panic!("{at}: the list")),
                    composer: frame
                        .composer
                        .unwrap_or_else(|| panic!("{at}: the composer")),
                    at,
                }
            })
            .collect()
    }

    /// Draws one frame, as the window draws one, and records under `at` what
    /// it laid out and what the frames its layout asks for, which a test
    /// window draws at once, laid out, after the frames the window drew since
    /// the last.
    fn one_frame(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        frames: &mut Vec<Frame>,
        at: String,
    ) {
        frames.extend(recorded(app, cx, &format!("before {at}")));
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
        frames.extend(recorded(app, cx, &at));
    }

    /// Where the list and the composer settle with the probe line drawn as
    /// `line` says.
    fn settled_under(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        line: workspace::ProbeLine,
    ) -> (Bounds<Pixels>, Bounds<Pixels>) {
        app.update(cx, |app, cx| {
            app.working_probe.line.set(line);
            cx.notify();
        });
        draw(cx);
        let settled = recorded(app, cx, "settled").pop().expect("a frame");
        (settled.list, settled.composer)
    }

    /// A 1480 × 980 window in Changes with six edited files, alpha and beta
    /// staged, as the native reviews' fixture has them: [Staged 2, alpha,
    /// beta, Unstaged 4, delta, epsilon, gamma, zeta].
    async fn six_files(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, Entity<GitTurtle>, &mut VisualTestContext) {
        let (fixture, app, cx) = history_window_from(cx, history_script(40, 3)).await;
        let root = fixture.path().join("narrow");
        fixture_git(&root, &["reset", "--hard", "--quiet"]);
        for name in ["alpha", "beta", "gamma", "delta", "epsilon", "zeta"] {
            std::fs::write(root.join(format!("{name}.txt")), format!("{name}\n")).unwrap();
        }
        fixture_git(&root, &["add", "alpha.txt", "beta.txt"]);
        cx.simulate_resize(size(px(1480.), px(980.)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        rows_arrived(&app, cx).await;
        draw(cx);
        assert_eq!(app.read_with(cx, |app, _| app.working_rows.len()), 8);
        (fixture, app, cx)
    }

    /// The row of `name` in `area`, in the list as drawn.
    fn position_of(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        name: &str,
        area: ChangeArea,
    ) -> usize {
        app.read_with(cx, |app, _| {
            let status = app.work_status.as_ref().expect("a status");
            app.actionable_working_rows()
                .iter()
                .position(|row| {
                    matches!(row, workspace::WorkingRow::File(index, at)
                        if *at == area && status.entries[*index].path == Path::new(name))
                })
                .unwrap_or_else(|| panic!("{name} in {area:?}"))
        })
    }

    /// Presses the Stage or Unstage control of `name`'s row in `area` and
    /// draws a frame for each step of the write, two where a step can ask
    /// for another: the press, the write's outcome, its status read, the
    /// rows that read produces, each 16 ms of the row's flight; then the
    /// same for a quiet read of the same status, as the local watcher's
    /// after a write.
    async fn write_frames(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        name: &str,
        area: ChangeArea,
        frames: &mut Vec<Frame>,
    ) {
        let what = format!("{name} from {area:?}");
        let selector: &'static str =
            format!("working-stage-{}", position_of(app, cx, name, area)).leak();
        let control = cx.debug_bounds(selector).expect(selector);
        // The rows wait for the frames before them to be drawn.
        let mut held = Some(app.update(cx, |app, _| app.file_paths.executor().hold()));
        cx.simulate_click(control.center(), Modifiers::default());
        one_frame(app, cx, frames, format!("{what}: the press"));
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the write");
        write.await;
        for read in ["its read", "a quiet read"] {
            if read == "a quiet read" {
                held = Some(app.update(cx, |app, _| app.file_paths.executor().hold()));
                cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
            }
            for step in 0..8 {
                one_frame(app, cx, frames, format!("{what}: {read} {step}"));
                one_frame(app, cx, frames, format!("{what}: {read} {step}, again"));
                let tasks = app.update(cx, |app, _| {
                    [app.task.take(), app.status_task.take()]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                });
                for task in tasks {
                    task.await;
                }
                cx.executor().run_until_parked();
                // The status is read; its rows are being prepared.
                one_frame(app, cx, frames, format!("{what}: {read} {step}, read"));
                one_frame(
                    app,
                    cx,
                    frames,
                    format!("{what}: {read} {step}, read again"),
                );
                drop(held.take());
                app.update(cx, |app, _| app.file_paths.executor().drain());
                cx.executor().run_until_parked();
                let settled = app.read_with(cx, |app, _| {
                    app.status_task.is_none()
                        && !app.working_paths.pending
                        && !matches!(app.working_travel, RowTravel::Departed(_))
                });
                if settled {
                    break;
                }
            }
            for frame in 0..12 {
                one_frame(app, cx, frames, format!("{what}: {read}, frame {frame}"));
                cx.executor().advance_clock(Duration::from_millis(16));
                cx.executor().run_until_parked();
            }
            assert_eq!(flying(app, cx), None);
        }
    }

    /// A row's Stage and Unstage lay the list out in the same bounds in every
    /// frame the window draws from the press to the row's landing, and
    /// through the quiet read after it, at 13 and 18 pt: the frame the write
    /// starts in, the one its outcome arrives in, the one its status read
    /// arrives in, the one its rows land in and each frame of the flight.
    /// Its rows and headings keep their number, so the list keeps its height
    /// throughout, and the composer below it its place.
    ///
    /// A 21 px line drawn above the body in one frame while each write's
    /// rows are prepared, as "Filtering working paths…" once was, lays the
    /// list and the composer out in that frame as the body settles them with
    /// the line, and in the next as it settles them without: both are sized
    /// from the body's height in the frame they are drawn in
    /// (`workspace::WorkingBody`). Sized from the frame before, the
    /// composer grew the 23 px of a whole row less 21 px a frame after the
    /// line came and was 23 px taller a frame after it left.
    #[gpui::test]
    async fn the_list_keeps_its_bounds_in_every_frame_of_a_write(cx: &mut TestAppContext) {
        use workspace::ProbeLine;
        let (_fixture, app, cx) = six_files(cx).await;
        // At 18 pt a taller window keeps every row in view.
        for (text, height) in [(13, 980.), (18, 1300.)] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(1480.), px(height)));
            draw(cx);
            // Settled with the line, where the body is 21 px shorter, and
            // without it.
            let lined = settled_under(&app, cx, ProbeLine::Steady);
            let rest = settled_under(&app, cx, ProbeLine::Watch);
            assert_ne!(lined, rest, "{text} pt: the line moves the list");
            let mut frames = Vec::new();
            let (staged, unstaged) = (ChangeArea::Staged, ChangeArea::Unstaged);
            for (name, area) in [("delta.txt", unstaged), ("alpha.txt", staged)] {
                let from = frames.len();
                app.update(cx, |app, _| app.working_probe.line.set(ProbeLine::Once));
                write_frames(&app, cx, name, area, &mut frames).await;
                frames.extend(recorded(&app, cx, &format!("after {name}")));
                let lines = frames[from..].iter().filter(|frame| frame.line).count();
                assert_eq!(lines, 1, "{text} pt: {name}'s write draws the line once");
            }
            app.update(cx, |app, _| app.working_probe.line.set(ProbeLine::Off));
            let moved: Vec<_> = frames
                .iter()
                .filter(|frame| {
                    (frame.list, frame.composer) != if frame.line { lined } else { rest }
                })
                .map(|frame| {
                    format!(
                        "{}{}: list {:?}, composer {:?}",
                        frame.at,
                        if frame.line { " under the line" } else { "" },
                        frame.list,
                        frame.composer
                    )
                })
                .collect();
            assert!(
                moved.is_empty(),
                "{text} pt, {} frames: at rest list {:?}, composer {:?}; under the line list \
                 {:?}, composer {:?}; moved in {}",
                frames.len(),
                rest.0,
                rest.1,
                lined.0,
                lined.1,
                moved.join("; ")
            );
            // Back as it was for the next size.
            write_frames(&app, cx, "delta.txt", staged, &mut Vec::new()).await;
            write_frames(&app, cx, "alpha.txt", unstaged, &mut Vec::new()).await;
        }
    }

    /// Whether the last frame drew the busy look: the progress label,
    /// Cancel, "Refreshing…", the tab's "· working", Stage all or Unstage
    /// all, which stay available through the writes below, in the disabled
    /// colour, the branch picker or the Git profile picker dimmed, or the
    /// commit action reading "Working…".
    fn looks_busy(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> Vec<&'static str> {
        let disabled =
            cx.update(|_, cx| gpui_kit::component::Theme::global(cx).disabled_foreground());
        let mut drawn = Vec::new();
        if cx.debug_bounds("operation-progress").is_some() {
            drawn.push("the progress label");
        }
        if cx.debug_bounds("cancel-git-operation").is_some() {
            drawn.push("Cancel");
        }
        if cx.debug_bounds("working-refreshing").is_some() {
            drawn.push("Refreshing…");
        }
        if app.read_with(cx, |app, _| {
            app.repository_tabs
                .tabs
                .iter()
                .any(|tab| app.tab_label(tab).0.ends_with(" · working"))
        }) {
            drawn.push("the tab's · working");
        }
        for (control, text) in [
            ("Stage all", "stage-all/rest/text"),
            ("Unstage all", "unstage-all/rest/text"),
        ] {
            if crate::appearance::drawn_color(text) == Some(disabled) {
                drawn.push(control);
            }
        }
        for (control, key) in [
            ("the branch picker", "current-branch"),
            ("the profile picker", "profile-button"),
        ] {
            let refusal = crate::workspace::refusals::get(key)
                .unwrap_or_else(|| panic!("{control} is drawn"));
            if refusal.dimmed {
                drawn.push(control);
            }
        }
        if app.read_with(cx, |app, cx| app.commit_action_label(cx)) == "Working…" {
            drawn.push("Commit's Working…");
        }
        drawn
    }

    /// A row's Stage that lands with its rows within the busy look's delay
    /// never looks busy: in every frame from the press to the landing no
    /// progress label or Cancel is drawn, the tab does not read "· working",
    /// Stage all and Unstage all keep their resting colour, the branch and
    /// profile pickers are not dimmed and the commit action does not read
    /// "Working…", where the write once dimmed every control for a frame and
    /// the commit action read "Working…" in it. Acceptance does not
    /// wait: another row's Stage pressed while the write runs is refused at
    /// once, and its row does not take the travelling row's place. A write
    /// still running when the delay ends looks busy then, and not a
    /// millisecond before.
    #[gpui::test]
    async fn a_quick_write_never_looks_busy(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = six_files(cx).await;
        let (_, palette) = crate::appearance::test_palettes().remove(0);
        cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
        draw(cx);
        // Each inspected frame records the pickers afresh, so `looks_busy`
        // reads what that frame drew rather than an earlier one.
        let frame = |cx: &mut VisualTestContext| {
            crate::workspace::refusals::forget();
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
        };
        frame(cx);
        assert!(looks_busy(&app, cx).is_empty(), "at rest");

        // [Staged 2, alpha, beta, Unstaged 4, delta, epsilon, gamma, zeta]:
        // delta's Stage, with the write held so its frames can be drawn.
        let held = app.update(cx, |app, _| app.operations.hold());
        let delta = cx.debug_bounds("working-stage-4").expect("delta's Stage");
        cx.simulate_click(delta.center(), Modifiers::default());
        let mut seen = Vec::new();
        frame(cx);
        seen.push(("the press", looks_busy(&app, cx)));
        assert!(app.read_with(cx, |app, _| app.operation_busy.is_some()));
        // Epsilon's Stage looks available and is refused at once.
        let epsilon = cx.debug_bounds("working-stage-5").expect("epsilon's Stage");
        cx.simulate_click(epsilon.center(), Modifiers::default());
        frame(cx);
        seen.push(("a second press", looks_busy(&app, cx)));
        app.read_with(cx, |app, _| match &app.working_travel {
            RowTravel::Departed(departure) => {
                assert_eq!(departure.path, Path::new("delta.txt"))
            }
            _ => panic!("delta's departure was replaced"),
        });
        drop(held);
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the write");
        write.await;
        for step in 0..8 {
            frame(cx);
            seen.push(("the write's outcome and reads", looks_busy(&app, cx)));
            let tasks = app.update(cx, |app, _| {
                [app.task.take(), app.status_task.take()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
            });
            for task in tasks {
                task.await;
            }
            cx.executor().run_until_parked();
            frame(cx);
            seen.push(("the status read", looks_busy(&app, cx)));
            app.update(cx, |app, _| app.file_paths.executor().drain());
            cx.executor().run_until_parked();
            if flying(&app, cx).is_some() || step == 7 {
                break;
            }
        }
        assert_eq!(flying(&app, cx), Some(3), "delta travels to Staged");
        frame(cx);
        seen.push(("the rows' landing", looks_busy(&app, cx)));
        let flashed: Vec<_> = seen
            .iter()
            .filter(|(_, drawn)| !drawn.is_empty())
            .map(|(at, drawn)| format!("{at}: {}", drawn.join(", ")))
            .collect();
        assert!(flashed.is_empty(), "a quick write looked busy: {flashed:?}");
        assert_eq!(
            app.read_with(cx, |app, _| app.working_paths.staged_count),
            3,
            "only delta was staged"
        );
        advance(cx, ROW_TRAVEL.as_millis() as u64);

        // [Staged 3, alpha, beta, delta, Unstaged 3, epsilon, gamma, zeta]:
        // epsilon's Stage, held past the delay.
        let held = app.update(cx, |app, _| app.operations.hold());
        let epsilon = cx.debug_bounds("working-stage-5").expect("epsilon's Stage");
        cx.simulate_click(epsilon.center(), Modifiers::default());
        frame(cx);
        assert!(looks_busy(&app, cx).is_empty(), "the press");
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY - Duration::from_millis(1));
        cx.executor().run_until_parked();
        frame(cx);
        assert!(looks_busy(&app, cx).is_empty(), "a millisecond before");
        cx.executor().advance_clock(Duration::from_millis(1));
        cx.executor().run_until_parked();
        frame(cx);
        assert_eq!(
            looks_busy(&app, cx),
            [
                "the progress label",
                "Cancel",
                "the tab's · working",
                "Stage all",
                "Unstage all",
                "the branch picker",
                "the profile picker",
                "Commit's Working…"
            ],
            "once the delay passed"
        );
        drop(held);
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the write");
        write.await;
        rows_arrived(&app, cx).await;
        frame(cx);
        assert!(looks_busy(&app, cx).is_empty(), "landed");
    }

    /// While a write's busy look waits, every control the write leaves
    /// refusing a press is announced unavailable at once, though it still
    /// looks available, and is dimmed with it once the look shows: a row's
    /// Stage, Stage all, Refresh and the header's Projects and Pull requests
    /// were announced enabled through the delay, and each refused a press.
    /// The Actions menu opened then offers none of its items, each of which
    /// refuses while a write runs, where it listed them all available. A
    /// control announced unavailable writes AccessKit's disabled property,
    /// as a disabled one does.
    #[gpui::test]
    async fn a_refusing_control_is_announced_unavailable_before_it_dims(cx: &mut TestAppContext) {
        use crate::workspace::refusals::{self, Refusal};
        let (_fixture, app, cx) = six_files(cx).await;
        let frame = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
        };
        let controls = [
            "working-stage-5",
            "stage-all",
            "unstage-all",
            "refresh-working",
            "projects",
            "pull-requests",
        ];
        let states = |announced, dimmed| {
            controls
                .iter()
                .map(|key| (*key, Some(Refusal { announced, dimmed })))
                .collect::<Vec<_>>()
        };
        let seen = || {
            controls
                .iter()
                .map(|key| (*key, refusals::get(key)))
                .collect::<Vec<_>>()
        };
        refusals::forget();
        frame(cx);
        assert_eq!(seen(), states(false, false), "at rest");

        // [Staged 2, alpha, beta, Unstaged 4, delta, epsilon, gamma, zeta]:
        // delta's Stage, with the write held through the delay.
        let held = app.update(cx, |app, _| app.operations.hold());
        let delta = cx.debug_bounds("working-stage-4").expect("delta's Stage");
        cx.simulate_click(delta.center(), Modifiers::default());
        frame(cx);
        assert_eq!(seen(), states(true, false), "the press");

        // The Actions menu, opened before the write looks busy.
        let actions = cx
            .debug_bounds("working-recovery-menu")
            .expect("the Actions menu");
        cx.simulate_click(actions.center(), Modifiers::default());
        frame(cx);
        for item in [
            "Ignore selected untracked file…",
            "Save changes to stash…",
            "Browse stashes…",
            "Amend last commit…",
            "Undo last local commit…",
        ] {
            assert!(
                cx.debug_bounds(format!("menu-item: {item} · MenuItem disabled").leak())
                    .is_some(),
                "{item} disabled in the menu opened while the write runs"
            );
        }
        assert!(
            cx.debug_bounds("menu-item: Browse stashes… · MenuItem")
                .is_none()
        );
        cx.simulate_keystrokes("escape");
        frame(cx);

        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        cx.executor().run_until_parked();
        frame(cx);
        assert_eq!(seen(), states(true, true), "once the write looks busy");
        drop(held);
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the write");
        write.await;
        rows_arrived(&app, cx).await;
        advance(cx, ROW_TRAVEL.as_millis() as u64);
        assert_eq!(seen(), states(false, false), "landed");

        // An Actions menu opened while a write runs is built again, in place,
        // when the write ends: Browse stashes… reads available without
        // reopening it.
        let held = held_write(&app, cx);
        cx.simulate_click(actions.center(), Modifiers::default());
        frame(cx);
        assert!(
            cx.debug_bounds("menu-item: Browse stashes… · MenuItem disabled")
                .is_some(),
            "Browse stashes… disabled while the write runs"
        );
        release(&app, held, cx).await;
        frame(cx);
        assert!(
            cx.debug_bounds("menu-item: Browse stashes… · MenuItem")
                .is_some(),
            "Browse stashes… available in the open menu once the write ends"
        );
        cx.simulate_keystrokes("escape");
        frame(cx);

        // The node each writes: announced unavailable alone, disabled.
        let nodes = crate::worktrees::ring_room_tests::row_nodes(
            cx,
            [(false, false), (true, false), (true, true)]
                .into_iter()
                .map(|(refused, dimmed)| {
                    crate::workspace::refusing(
                        crate::button("refusing-probe", "Stage", "", false),
                        || "refusing-probe".into(),
                        refused,
                        dimmed,
                    )
                })
                .collect(),
        );
        assert_eq!(
            nodes
                .iter()
                .map(|node| node.is_disabled())
                .collect::<Vec<_>>(),
            [false, true, true]
        );
    }

    /// A write's busy look waits in History, Targets and the Activity
    /// dialog as it does in Changes: Latest, Targets' branch and remote
    /// fields and Activity's Refresh and Browse reflog refuse at once and
    /// are announced so, the fields read-only, but dim only once the write
    /// has run 200 ms. They dimmed the moment any write started.
    #[gpui::test]
    async fn history_targets_and_activity_dim_only_once_a_write_looks_busy(
        cx: &mut TestAppContext,
    ) {
        use crate::workspace::refusals::{self, Refusal};
        let (_fixture, app, cx) = history_window_from(cx, history_script(40, 3)).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.git_actions_open = true;
                cx.notify();
            })
        });
        draw(cx);
        let frame = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
        };
        let page = [
            "history-newest",
            "branch-name",
            "remote-name",
            "remote-branch",
        ];
        let dialog = ["activity-refresh", "activity-reflog"];
        let seen = |keys: &[&'static str]| {
            keys.iter()
                .map(|key| (*key, refusals::get(key)))
                .collect::<Vec<_>>()
        };
        let states = |keys: &[&'static str], announced, dimmed| {
            keys.iter()
                .map(|key| (*key, Some(Refusal { announced, dimmed })))
                .collect::<Vec<_>>()
        };
        refusals::forget();
        frame(cx);
        assert_eq!(seen(&page), states(&page, false, false), "at rest");

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
        frame(cx);
        assert_eq!(seen(&page), states(&page, true, false), "the write starts");
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_activity(window, cx)));
        frame(cx);
        assert_eq!(
            seen(&dialog),
            states(&dialog, true, false),
            "Activity opened as it starts"
        );
        // Browse reflog… looks available and is refused, and Activity stays
        // open: it closed with no Reflog to follow it.
        let reflog = cx
            .debug_bounds("activity-reflog")
            .expect("Activity's Browse reflog…");
        cx.simulate_click(reflog.center(), Modifiers::default());
        frame(cx);
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "Activity stays open"
        );
        assert!(cx.debug_bounds("activity-reflog").is_some());

        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        cx.executor().run_until_parked();
        frame(cx);
        assert_eq!(
            [seen(&page), seen(&dialog)].concat(),
            [states(&page, true, true), states(&dialog, true, true)].concat(),
            "once the write looks busy"
        );
        drop(held);
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the write");
        write.await;
        crate::views::tests::settle(&app, cx).await;
    }

    /// What each of `keys` was last built with (`workspace::refusing`).
    fn refusals_of(keys: &[&'static str]) -> Vec<(&'static str, Option<Refusal>)> {
        keys.iter()
            .map(|key| (*key, crate::workspace::refusals::get(key)))
            .collect()
    }

    use crate::workspace::refusals::Refusal;

    /// Draws a frame, as the window would after a notify.
    fn next_frame(cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
    }

    /// Starts a Stage all and holds it, as a slow write.
    fn held_write(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> crate::operations::Held {
        held(app, cx, gitturtle_core::WriteCommand::StageAll)
    }

    /// Starts `command` and holds it, as a slow write.
    fn held(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        command: gitturtle_core::WriteCommand,
    ) -> crate::operations::Held {
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| app.write(command, "Working…", window, cx))
        });
        next_frame(cx);
        held
    }

    /// Lets the held write finish and its reads settle.
    async fn release(
        app: &Entity<GitTurtle>,
        held: crate::operations::Held,
        cx: &mut VisualTestContext,
    ) {
        drop(held);
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the write");
        write.await;
        crate::views::tests::settle(app, cx).await;
    }

    /// Clicks the control `selector` draws, and draws the frame after it.
    fn press(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx.debug_bounds(selector).expect(selector);
        cx.simulate_click(bounds.center(), Modifiers::default());
        next_frame(cx);
    }

    /// Whether a dialog is open.
    fn dialog_open(cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| window.has_active_dialog(cx))
    }

    /// Closes the open dialog, its closing motion included.
    fn close_dialog(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.close_dialog(cx));
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.executor().run_until_parked();
        next_frame(cx);
    }

    /// The other controls a write leaves unavailable wait for its busy look
    /// too. The branch picker and the Git profile picker, whose menus open
    /// until then, look available and offer none of the items the write
    /// refuses, and a menu opened then is built again when the write ends;
    /// diverged branches' Merge… and Rebase…, History search's Restart, the
    /// status bar's Refresh, a paused merge's Continue, Abort… and Keep
    /// files…, and the Worktrees and Reflog dialogs' actions refuse at once
    /// and are announced so, and dim once the write has run 200 ms. Pressed
    /// while they still look available, each changes nothing: no dialog or
    /// confirmation opens, no rescan waits, the search keeps its results, no
    /// editor is asked for and no review is read. Each dimmed the moment any
    /// write started, a quick Stage's too.
    #[gpui::test]
    async fn pickers_banners_and_dialogs_dim_only_once_a_write_looks_busy(cx: &mut TestAppContext) {
        let (fixture, app, cx) = history_window_from(cx, history_script(40, 3)).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        // The import checked nothing out; the reset does, and gives HEAD a
        // reflog entry.
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        fixture_git(&repository, &["reset", "-q", "--hard"]);
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        crate::views::tests::settle(&app, cx).await;
        // The branch has diverged from its upstream, as a status read says,
        // the local watch could not start, the profile store could not be
        // read and a search has found its match.
        app.update(cx, |app, cx| {
            let status = app.work_status.as_deref().cloned().unwrap_or_default();
            app.work_status = Some(Arc::new(RepositoryStatus {
                upstream: Some("origin/main".into()),
                ahead: 1,
                behind: 1,
                ..status
            }));
            app.fail_profile_store("The profile store could not be read.");
            cx.notify();
        });
        crate::width_walk::unwatch(&app, cx);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.search.update(cx, |input, cx| {
                    input.set_value("fixture notes 3", window, cx)
                });
                app.history_query_changed(window, cx);
            })
        });
        crate::history_search::settle_history_search(&app, cx).await;
        draw(cx);
        let pickers = ["current-branch", "profile-button"];
        let refused = [
            "merge-upstream",
            "rebase-upstream",
            "history-search-restart",
            "retry-local-refresh",
        ];
        let states = |keys: &[&'static str], announced, dimmed| {
            keys.iter()
                .map(|key| (*key, Some(Refusal { announced, dimmed })))
                .collect::<Vec<_>>()
        };
        crate::workspace::refusals::forget();
        next_frame(cx);
        assert_eq!(
            [refusals_of(&pickers), refusals_of(&refused)].concat(),
            [
                states(&pickers, false, false),
                states(&refused, false, false)
            ]
            .concat(),
            "at rest"
        );
        assert!(!app.read_with(cx, |app, _| app.automatic.rescan_queued()));

        let held = held_write(&app, cx);
        assert_eq!(
            [refusals_of(&pickers), refusals_of(&refused)].concat(),
            [
                states(&pickers, false, false),
                states(&refused, true, false)
            ]
            .concat(),
            "the write starts"
        );
        // Pressed while they look available, Merge…, Restart and Refresh
        // change nothing.
        for control in [
            "merge-upstream",
            "history-search-restart",
            "retry-local-refresh",
        ] {
            press(cx, control);
        }
        cx.executor().run_until_parked();
        assert!(!dialog_open(cx), "Merge… opens no review");
        assert!(
            !app.read_with(cx, |app, _| app.history_search_pending()),
            "Restart leaves the search as it was"
        );
        assert!(
            !app.read_with(cx, |app, _| app.automatic.rescan_queued()),
            "Refresh queues no rescan"
        );
        // Each picker's menu, opened as the write starts, offers none of the
        // items it refuses; Activity and the profile store's retry still
        // work.
        for (picker, disabled, enabled) in [
            (
                "current-branch",
                &[
                    "Find or create a branch…",
                    "Manage another branch…",
                    "Tags…",
                    "Worktrees…",
                ][..],
                "GitTurtle activity…",
            ),
            (
                "profile-button",
                &["Manage profiles…"][..],
                "Retry profile storage",
            ),
        ] {
            press(cx, picker);
            for item in disabled {
                assert!(
                    cx.debug_bounds(format!("menu-item: {item} · MenuItem disabled").leak())
                        .is_some(),
                    "{picker}: {item} disabled in the menu opened while the write runs"
                );
            }
            assert!(
                cx.debug_bounds(format!("menu-item: {enabled} · MenuItem").leak())
                    .is_some(),
                "{picker}: {enabled} stays available"
            );
            cx.simulate_keystrokes("escape");
            next_frame(cx);
        }

        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        cx.executor().run_until_parked();
        next_frame(cx);
        assert_eq!(
            [refusals_of(&pickers), refusals_of(&refused)].concat(),
            [states(&pickers, true, true), states(&refused, true, true)].concat(),
            "once the write looks busy"
        );
        release(&app, held, cx).await;

        // A menu opened while a write runs is built again, in place, when it
        // ends: its items read available without reopening it.
        for (picker, item) in [
            ("current-branch", "Find or create a branch…"),
            ("profile-button", "Manage profiles…"),
        ] {
            let held = held_write(&app, cx);
            press(cx, picker);
            assert!(
                cx.debug_bounds(format!("menu-item: {item} · MenuItem disabled").leak())
                    .is_some(),
                "{picker}: {item} disabled while the write runs"
            );
            release(&app, held, cx).await;
            next_frame(cx);
            assert!(
                cx.debug_bounds(format!("menu-item: {item} · MenuItem").leak())
                    .is_some(),
                "{picker}: {item} available in the open menu once the write ends"
            );
            cx.simulate_keystrokes("escape");
            next_frame(cx);
        }

        // The Worktrees dialog, open as a write starts elsewhere. Its editor
        // is one that cannot start, so a press that went through would only
        // fail, never open anything.
        app.update(cx, |app, _| {
            app.settings.external_editor = "/nonexistent/gitturtle-test-editor".into()
        });
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_worktree_manager(window, cx)));
        // Its own reads list the worktrees and inspect the selected one.
        crate::worktrees::tests::settle_open_manager(&app, cx).await;
        let actions = [
            "open-managed-worktree",
            "edit-managed-worktree",
            "remove-managed-worktree",
            "force-remove-managed-worktree",
            "review-create-worktree",
        ];
        crate::workspace::refusals::forget();
        next_frame(cx);
        let rest: Vec<_> = refusals_of(&actions)
            .into_iter()
            .filter_map(|(key, refusal)| Some((key, refusal?)))
            .collect();
        assert!(!rest.is_empty(), "the dialog draws its actions");
        assert_eq!(
            rest.iter().find(|(key, _)| *key == "edit-managed-worktree"),
            Some(&(
                "edit-managed-worktree",
                Refusal {
                    announced: false,
                    dimmed: false
                }
            )),
            "Editor is available at rest"
        );
        let shown: Vec<_> = rest.iter().map(|(key, _)| *key).collect();
        let held = held_write(&app, cx);
        assert_eq!(
            refusals_of(&shown),
            rest.iter()
                .map(|(key, at_rest)| (
                    *key,
                    Some(Refusal {
                        announced: true,
                        dimmed: at_rest.dimmed
                    })
                ))
                .collect::<Vec<_>>(),
            "Worktrees as the write starts"
        );
        press(cx, "edit-managed-worktree");
        app.read_with(cx, |app, _| app.preferences_writer.drain());
        cx.executor().run_until_parked();
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_error.clone()),
            None,
            "Editor asks for no editor"
        );
        assert!(dialog_open(cx), "Worktrees stays open");
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        cx.executor().run_until_parked();
        next_frame(cx);
        assert_eq!(
            refusals_of(&shown),
            states(&shown, true, true),
            "Worktrees once the write looks busy"
        );
        release(&app, held, cx).await;
        close_dialog(cx);

        // The Reflog, its newest entry selected, open as a write starts, its
        // content scrolled to its end, where the review action lies.
        let browser = crate::reflog::ReflogBrowser::open_selected(&app, cx).await;
        draw(cx);
        let explanation = cx
            .debug_bounds("reflog-explanation")
            .expect("the Reflog's explanation");
        cx.simulate_mouse_move(explanation.center(), None, Modifiers::default());
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: explanation.center(),
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-2000.))),
            modifiers: Modifiers::default(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        draw(cx);
        let review = ["review-reflog-recovery"];
        crate::workspace::refusals::forget();
        next_frame(cx);
        assert_eq!(
            refusals_of(&review),
            states(&review, false, false),
            "Reflog at rest"
        );
        let held = held_write(&app, cx);
        assert_eq!(
            refusals_of(&review),
            states(&review, true, false),
            "Reflog as the write starts"
        );
        press(cx, "review-reflog-recovery");
        assert!(
            !browser.read_with(cx, |browser, _| browser.started_a_read()),
            "Review recovery branch… reads no review"
        );
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        cx.executor().run_until_parked();
        next_frame(cx);
        assert_eq!(
            refusals_of(&review),
            states(&review, true, true),
            "Reflog once the write looks busy"
        );
        release(&app, held, cx).await;
        close_dialog(cx);

        // A merge paused before its commit, with nothing left to resolve.
        let identity = [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ];
        fixture_git(&repository, &["checkout", "-q", "-b", "side"]);
        std::fs::write(repository.join("side.txt"), "side\n").unwrap();
        fixture_git(&repository, &["add", "side.txt"]);
        fixture_git(
            &repository,
            &[&identity[..], &["commit", "-q", "-m", "side"]].concat(),
        );
        fixture_git(&repository, &["checkout", "-q", "main"]);
        fixture_git(
            &repository,
            &[
                &identity[..],
                &["merge", "-q", "--no-commit", "--no-ff", "side"],
            ]
            .concat(),
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        crate::views::tests::settle(&app, cx).await;
        let paused = ["continue-operation", "abort-operation", "stop-operation"];
        crate::workspace::refusals::forget();
        next_frame(cx);
        assert_eq!(
            refusals_of(&paused),
            states(&paused, false, false),
            "paused at rest"
        );
        let held = held_write(&app, cx);
        assert_eq!(
            refusals_of(&paused),
            states(&paused, true, false),
            "paused as the write starts"
        );
        for control in paused {
            press(cx, control);
            cx.executor().run_until_parked();
            assert!(!dialog_open(cx), "{control} opens no confirmation");
        }
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        cx.executor().run_until_parked();
        next_frame(cx);
        assert_eq!(
            refusals_of(&paused),
            states(&paused, true, true),
            "paused once the write looks busy"
        );
        release(&app, held, cx).await;
        drop(fixture);
    }

    /// With `keys` drawn at rest in the open dialog `at`, a write starts:
    /// each is announced unavailable at once with its resting look, a press
    /// on each while it still looks available leaves the dialog `unchanged`,
    /// and each dims once the write has run 200 ms. Then the write ends.
    async fn refused_then_dimmed(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        at: &str,
        keys: &[&'static str],
        unchanged: impl Fn(&Entity<GitTurtle>, &mut VisualTestContext) -> Result<(), String>,
    ) {
        let states = |announced, dimmed| {
            keys.iter()
                .map(|key| (*key, Some(Refusal { announced, dimmed })))
                .collect::<Vec<_>>()
        };
        crate::workspace::refusals::forget();
        next_frame(cx);
        assert_eq!(refusals_of(keys), states(false, false), "{at} at rest");
        let held = held_write(app, cx);
        assert_eq!(
            refusals_of(keys),
            states(true, false),
            "{at} as the write starts"
        );
        for key in keys {
            press(cx, key);
            cx.executor().run_until_parked();
            next_frame(cx);
            if let Err(change) = unchanged(app, cx) {
                panic!("{at}: {key} pressed while the write runs {change}");
            }
        }
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        cx.executor().run_until_parked();
        next_frame(cx);
        assert_eq!(
            refusals_of(keys),
            states(true, true),
            "{at} once the write looks busy"
        );
        release(app, held, cx).await;
    }

    /// Tags, its tag inspector, Stashes and the rebase dialogs, open as a
    /// write starts, wait for its busy look as the Worktrees and Reflog
    /// dialogs do: Create tag…, a tag's row, Delete local tag…, Push to…,
    /// Drop…, Review restore…, Load commits, Review rebase… and Review last
    /// rewritten series… are announced unavailable at once, pressed while
    /// they still look available change nothing, and dim once the write has
    /// run 200 ms. Each refused a press silently while it was announced and
    /// drawn available; Load commits set the form's error and Review last
    /// rewritten series… closed the dialog before its review refused.
    #[gpui::test]
    async fn tags_stashes_and_rebase_dim_only_once_a_write_looks_busy(cx: &mut TestAppContext) {
        use crate::tags::tests::{add_remotes, git, read_lands, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        add_remotes(fixture.path(), &repo, 1);
        std::fs::write(repo.path().join("notes.txt"), "edited\n").unwrap();
        git(
            repo.path(),
            &["stash", "push", "--include-untracked", "-m", "kept"],
        );
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        let drawn = |selector: &'static str| {
            move |_: &Entity<GitTurtle>, cx: &mut VisualTestContext| {
                if dialog_open(cx) && cx.debug_bounds(selector).is_some() {
                    Ok(())
                } else {
                    Err(format!("closed the dialog drawing {selector}"))
                }
            }
        };

        cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
        read_lands(&app, cx).await;
        draw(cx);
        refused_then_dimmed(
            &app,
            cx,
            "Tags",
            &["create-tag", "tag-row-0"],
            drawn("tags-list"),
        )
        .await;
        close_dialog(cx);

        let tag = repo.tags().unwrap().tags[0].clone();
        cx.update(|window, cx| app.update(cx, |app, cx| app.inspect_tag(tag, window, cx)));
        read_lands(&app, cx).await;
        draw(cx);
        refused_then_dimmed(
            &app,
            cx,
            "the tag inspector",
            &["delete-local-tag", "push-tag-remote-0"],
            drawn("tag-remote-list"),
        )
        .await;
        close_dialog(cx);

        crate::recovery::open_selected_stash(&app, cx).await;
        draw(cx);
        refused_then_dimmed(
            &app,
            cx,
            "Stashes",
            &["drop-saved-stash", "restore-saved-stash"],
            drawn("stash-browser"),
        )
        .await;
        close_dialog(cx);

        crate::interactive_rebase::open_loaded(&app, cx).await;
        draw(cx);
        let form = drawn("rebase-list-focus");
        refused_then_dimmed(
            &app,
            cx,
            "the rebase form",
            &[
                "load-rebase-commits",
                "review-interactive-rebase",
                "review-previous-rewrite",
            ],
            move |app, cx| {
                form(app, cx)?;
                if app.read_with(cx, |app, cx| {
                    crate::interactive_rebase::form_changed(app, cx)
                }) {
                    Err("changed the form".into())
                } else {
                    Ok(())
                }
            },
        )
        .await;
        close_dialog(cx);
        drop(fixture);
    }

    /// Opens the right-click menu of the row `selector` draws, near its
    /// leading edge, and draws it.
    fn right_click(cx: &mut VisualTestContext, selector: &'static str) {
        let row = cx.debug_bounds(selector).expect(selector);
        let pointer = point(row.left() + px(16.), row.center().y);
        cx.simulate_mouse_move(pointer, None, Modifiers::default());
        cx.simulate_mouse_down(pointer, gpui::MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(pointer, gpui::MouseButton::Right, Modifiers::default());
        cx.executor().run_until_parked();
        next_frame(cx);
        next_frame(cx);
    }

    /// Whether the open menu offers `item`: drawn available, drawn
    /// disabled, or not drawn.
    fn offered(cx: &mut VisualTestContext, item: &str) -> Option<bool> {
        let drawn = |cx: &mut VisualTestContext, role: &str| {
            cx.debug_bounds(format!("menu-item: {item} · {role}").leak())
                .is_some()
        };
        if drawn(cx, "MenuItem") {
            Some(true)
        } else if drawn(cx, "MenuItem disabled") {
            Some(false)
        } else {
            None
        }
    }

    /// A right-click menu on a History commit, a navigator branch or a
    /// working row follows writes as the pickers' menus do: opened at rest,
    /// it is built again in place, its items disabled, when a write starts
    /// and available again when it ends; opened while a write runs, it
    /// offers none of the items the write refuses until it ends. Each read
    /// the write once, as it opened, and kept its items so.
    #[gpui::test]
    async fn right_click_menus_follow_writes(cx: &mut TestAppContext) {
        let (fixture, app, cx) = history_window_from(cx, history_script(40, 3)).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        fixture_git(&repository, &["reset", "-q", "--hard"]);
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        crate::views::tests::settle(&app, cx).await;
        let branch = app.read_with(cx, |app, _| {
            app.nav_rows
                .iter()
                .position(|row| matches!(row, crate::NavRow::Branch(..)))
                .expect("a branch in the navigator")
        });
        let branch: &'static str = format!("nav-row-{branch}").leak();
        // An Unstage all with nothing staged, which moves no row.
        let write =
            |cx: &mut VisualTestContext| held(&app, cx, gitturtle_core::WriteCommand::UnstageAll);
        let rows = [
            ("history-row-0", "Revert this commit…"),
            (branch, "Branch actions…"),
            ("working", "Delete untracked file…"),
        ];
        for (row, item) in rows {
            let row = if row == "working" {
                // An untracked file in Changes.
                std::fs::write(repository.join("extra.txt"), "extra\n").unwrap();
                cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
                rows_arrived(&app, cx).await;
                draw(cx);
                let position = position_of(&app, cx, "extra.txt", ChangeArea::Unstaged);
                format!("working-row-{position}").leak()
            } else {
                row
            };
            right_click(cx, row);
            assert_eq!(offered(cx, item), Some(true), "{row}: {item} at rest");
            let held = write(cx);
            next_frame(cx);
            assert_eq!(
                offered(cx, item),
                Some(false),
                "{row}: {item} in the open menu once a write starts"
            );
            release(&app, held, cx).await;
            next_frame(cx);
            assert_eq!(
                offered(cx, item),
                Some(true),
                "{row}: {item} in the open menu once the write ends"
            );
            cx.simulate_keystrokes("escape");
            next_frame(cx);
            let held = write(cx);
            right_click(cx, row);
            assert_eq!(
                offered(cx, item),
                Some(false),
                "{row}: {item} in a menu opened while a write runs"
            );
            release(&app, held, cx).await;
            next_frame(cx);
            assert_eq!(
                offered(cx, item),
                Some(true),
                "{row}: {item} in that menu once the write ends"
            );
            cx.simulate_keystrokes("escape");
            next_frame(cx);
        }
        drop(fixture);
    }

    /// Refreshes the working state, as the local watcher would after a
    /// change outside the app, and draws the frame after it.
    async fn refreshed(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        crate::views::tests::settle(app, cx).await;
        next_frame(cx);
    }

    /// A commit's Revert and Cherry-pick refuse while a merge, rebase,
    /// cherry-pick or revert is paused, as its menu says
    /// (`GitTurtle::commit_menu_state`). A History commit's right-click menu
    /// open as a merge pauses is built again in place, its items disabled,
    /// and available again once the merge is abandoned; a press on one
    /// prepares nothing. Enter on a merge's Revert in a keyboard's menu,
    /// which was built as it opened at rest and is not built again, asks for
    /// no mainline and prepares nothing either, and reports the core's
    /// refusal, as Review restore… in Stashes does without reading a plan;
    /// it closed the menu and did nothing visible. The menus were built
    /// again only as a write
    /// started or ended, so a cherry-pick that stopped on conflicts left
    /// those items offered until the menu was opened again, and a press read
    /// a plan that the core then refused.
    #[gpui::test]
    async fn commit_actions_refuse_while_an_integration_is_paused(cx: &mut TestAppContext) {
        const REVERT: &str = "Revert this commit…";
        const CHERRY_PICK: &str = "Cherry-pick this commit…";
        // main ends in a merge, whose Revert asks for a mainline first. It
        // is in History from the start; the commits made below come into it
        // only as History reads them again.
        let mut script = history_script(40, 3);
        script.truncate(script.len() - "done\n".len());
        for (reference, message, change) in [
            (
                "topic",
                "topic",
                "from refs/heads/main\nM 100644 inline topic.txt\ndata 6\ntopic\n",
            ),
            ("main", "merge topic", "merge refs/heads/topic\n"),
        ] {
            script.push_str(&format!(
                "commit refs/heads/{reference}\ncommitter Fixture <fixture@example.invalid> 1700000100 +0000\ndata {}\n{message}\n{change}",
                message.len(),
            ));
        }
        script.push_str("done\n");
        let (fixture, app, cx) = history_window_from(cx, script).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        cx.update(|_, cx| {
            crate::shortcuts::bind_keys(cx);
            crate::native_accessibility::bind_keys(cx);
            // The Stashes dialog then opens where it rests: its slide runs
            // on the clock on the wall, and Review restore… moved about
            // 56 px in the 100 ms after it was drawn, so a press under load
            // missed it.
            cx.set_reduce_motion(true);
        });
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        fixture_git(&repository, &["reset", "-q", "--hard"]);
        let identity = [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ];
        let git =
            |arguments: &[&str]| fixture_git(&repository, &[&identity[..], arguments].concat());
        // A saved stash, and a side branch to merge without its commit.
        std::fs::write(repository.join("notes.txt"), "stashed\n").unwrap();
        git(&["stash", "push", "-q", "-m", "kept"]);
        git(&["checkout", "-q", "-b", "side"]);
        std::fs::write(repository.join("side.txt"), "side\n").unwrap();
        git(&["add", "side.txt"]);
        git(&["commit", "-q", "-m", "side"]);
        git(&["checkout", "-q", "main"]);
        let pause = || git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
        let paused = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| app.integration_state.is_some())
        };
        // Nothing prepared, nothing asked and nothing reported.
        let quiet = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                (app.operation_busy, app.operation_error.clone())
            })
        };
        refreshed(&app, cx).await;

        right_click(cx, "history-row-0");
        assert_eq!(offered(cx, REVERT), Some(true), "Revert at rest");
        assert_eq!(offered(cx, CHERRY_PICK), Some(true), "Cherry-pick at rest");
        pause();
        refreshed(&app, cx).await;
        assert!(paused(cx), "the merge paused");
        for item in [REVERT, CHERRY_PICK] {
            assert_eq!(
                offered(cx, item),
                Some(false),
                "{item} once the merge pauses"
            );
        }
        let held = app.update(cx, |app, _| app.operations.hold());
        let revert = cx
            .debug_bounds(format!("menu-item: {REVERT} · MenuItem disabled").leak())
            .expect("Revert drawn");
        cx.simulate_click(revert.center(), Modifiers::default());
        cx.executor().run_until_parked();
        assert_eq!(
            quiet(cx),
            (None, None),
            "a press on Revert prepares nothing"
        );
        assert!(!dialog_open(cx), "a press on Revert asks nothing");
        drop(held);
        git(&["merge", "--abort"]);
        refreshed(&app, cx).await;
        assert!(!paused(cx), "the merge was abandoned");
        if offered(cx, REVERT).is_none() {
            // The press closed the menu.
            right_click(cx, "history-row-0");
        }
        for item in [REVERT, CHERRY_PICK] {
            assert_eq!(offered(cx, item), Some(true), "{item} once the merge ends");
        }
        cx.simulate_keystrokes("escape");
        next_frame(cx);

        // A keyboard's menu, opened at rest.
        // A keyboard's menu on a merge, opened at rest.
        let merge = app.read_with(cx, |app, _| {
            app.commits
                .iter()
                .position(|commit| commit.parents.len() > 1)
                .expect("main's merge")
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.select_commit(merge, window, cx);
                app.pane = Pane::History;
                window.focus(&app.focus, cx);
            })
        });
        crate::views::tests::settle(&app, cx).await;
        cx.simulate_keystrokes("shift-f10");
        next_frame(cx);
        assert_eq!(
            offered(cx, REVERT),
            Some(true),
            "the keyboard's menu at rest"
        );
        pause();
        refreshed(&app, cx).await;
        assert!(paused(cx), "the merge paused again");
        let held = app.update(cx, |app, _| app.operations.hold());
        // Its cursor starts on Revert, still offered.
        assert_eq!(
            offered(cx, REVERT),
            Some(true),
            "the keyboard's menu paused"
        );
        cx.simulate_keystrokes("enter");
        cx.executor().run_until_parked();
        assert_eq!(
            quiet(cx),
            (None, Some(crate::recovery::PAUSED_REFUSAL.into())),
            "Enter on Revert prepares nothing and says why"
        );
        assert!(!dialog_open(cx), "Enter on Revert asks nothing");
        drop(held);
        app.update(cx, |app, _| app.operation_error = None);

        crate::recovery::open_selected_stash(&app, cx).await;
        next_frame(cx);
        let held = app.update(cx, |app, _| app.operations.hold());
        press(cx, "restore-saved-stash");
        cx.executor().run_until_parked();
        assert_eq!(
            quiet(cx),
            (None, Some(crate::recovery::PAUSED_REFUSAL.into())),
            "Review restore… reads no plan and says why"
        );
        drop(held);

        // The Changes Actions menu, opened while the merge is paused, offers
        // Amend and Undo in place once it is abandoned.
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        crate::views::tests::settle(&app, cx).await;
        next_frame(cx);
        press(cx, "working-recovery-menu");
        let last_commit = ["Amend last commit…", "Undo last local commit…"];
        for item in last_commit {
            assert_eq!(offered(cx, item), Some(false), "{item} while paused");
        }
        git(&["merge", "--abort"]);
        refreshed(&app, cx).await;
        assert!(!paused(cx), "the merge was abandoned again");
        for item in last_commit {
            assert_eq!(offered(cx, item), Some(true), "{item} once the merge ends");
        }
        cx.simulate_keystrokes("escape");
        next_frame(cx);
        drop(fixture);
    }

    /// However a right-click menu closes, by Escape, a press outside it or
    /// a resize, as a tiling compositor's re-tile does, focus returns to the
    /// list it was opened from, whose keys reach it again: Down moves the
    /// selection and Shift-F10 opens the keyboard's menu. So it does after a
    /// right-click on another row, or a third, or on the same row again,
    /// closes the menu open there and opens the row's own. The resize's
    /// dismissal skipped the menu's own Cancel and left focus on the menu,
    /// which no frame drew, so Down, Space and Shift-F10 stopped reaching
    /// the list; a menu opened by a right-click on another row returned
    /// focus to the menu it closed, which no frame drew either. The app's
    /// keys are bound here as the app binds them; a test window binds none
    /// of them (`views::tests::init_app`).
    #[gpui::test]
    async fn a_right_click_menus_focus_returns_to_its_list_however_it_closes(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, cx) = six_files(cx).await;
        cx.update(|_, cx| {
            crate::shortcuts::bind_keys(cx);
            crate::native_accessibility::bind_keys(cx);
        });
        let list = app.read_with(cx, |app, _| app.file_focus.clone());
        let focused = |cx: &mut VisualTestContext| cx.update(|window, _| list.is_focused(window));
        // [Staged 2, alpha, beta, Unstaged 4, delta, epsilon, gamma, zeta]
        let row_of = |cx: &mut VisualTestContext, position: usize| {
            app.read_with(cx, |app, _| match app.actionable_working_rows()[position] {
                workspace::WorkingRow::File(index, area) => Some((index, area)),
                _ => None,
            })
        };
        let (delta, epsilon) = (row_of(cx, 4), row_of(cx, 5));
        let item = "Delete untracked file…";
        // How far below the pointer a right-click menu draws its item.
        let below = |cx: &mut VisualTestContext, pointer: Point<Pixels>| {
            cx.debug_bounds(format!("menu-item: {item} · MenuItem").leak())
                .map(|drawn| f32::from(drawn.top() - pointer.y))
        };
        let (delta_row, epsilon_row, gamma_row) =
            ("working-row-4", "working-row-5", "working-row-6");
        // The rows each case right-clicks in turn, each while the menu the
        // one before opened is still open, and how it closes the last.
        let cases: [(&str, &[&'static str]); 6] = [
            ("Escape", &[delta_row]),
            ("a press outside", &[delta_row]),
            ("a resize", &[delta_row]),
            ("Escape", &[epsilon_row, delta_row]),
            ("Escape", &[epsilon_row, delta_row, gamma_row]),
            ("Escape", &[delta_row, delta_row]),
        ];
        let mut offset = None;
        for (closing, rows) in cases {
            let case = format!("{closing} after a right-click on {rows:?}");
            // delta selected, the list focused.
            let row = cx.debug_bounds(delta_row).expect("delta's row");
            cx.simulate_click(row.center(), Modifiers::default());
            cx.update(|window, cx| list.focus(window, cx));
            next_frame(cx);
            assert_eq!(app.read_with(cx, |app, _| app.working_selected), delta);
            for (turn, selector) in rows.iter().enumerate() {
                // The first right-click where `right_click` presses; a later
                // one beside the open menu's leading edge, which starts at
                // the pointer before, so that it lands on the row.
                let bounds = cx.debug_bounds(selector).expect(selector);
                let inset = if turn == 0 { px(16.) } else { px(6.) };
                let pointer = point(bounds.left() + inset, bounds.center().y);
                cx.simulate_mouse_move(pointer, None, Modifiers::default());
                cx.simulate_mouse_down(pointer, gpui::MouseButton::Right, Modifiers::default());
                cx.simulate_mouse_up(pointer, gpui::MouseButton::Right, Modifiers::default());
                cx.executor().run_until_parked();
                next_frame(cx);
                next_frame(cx);
                // The row's own menu, at its pointer.
                let drawn = below(cx, pointer).expect("the row's menu opened");
                let offset = *offset.get_or_insert(drawn);
                assert!(
                    (drawn - offset).abs() < 1.,
                    "{case}: {selector}'s menu opened at its pointer"
                );
                assert!(!focused(cx), "{case}: the menu holds focus");
            }
            match closing {
                "Escape" => cx.simulate_keystrokes("escape"),
                "a press outside" => {
                    // On delta's row, before the pointer the menu opened at,
                    // outside the menu's fill. The list's foot lies under the
                    // menu's lower items in this window.
                    let outside = point(row.left() + px(6.), row.bottom() - px(4.));
                    cx.simulate_click(outside, Modifiers::default());
                }
                _ => {
                    let viewport = cx.update(|window, _| window.viewport_size());
                    cx.simulate_resize(size(viewport.width - px(80.), viewport.height - px(80.)));
                }
            }
            cx.executor().run_until_parked();
            next_frame(cx);
            assert_eq!(offered(cx, item), None, "{case}: the menu closed");
            assert!(focused(cx), "{case}: focus is back on the list");
            cx.simulate_keystrokes("down");
            next_frame(cx);
            assert_eq!(
                app.read_with(cx, |app, _| app.working_selected),
                epsilon,
                "{case}: Down reached the list"
            );
            cx.simulate_keystrokes("shift-f10");
            next_frame(cx);
            assert!(
                app.read_with(cx, |app, _| app.file_menu.is_some()),
                "{case}: Shift-F10 opened the keyboard's menu"
            );
            assert_eq!(offered(cx, item), Some(true), "{case}: epsilon's menu");
            cx.simulate_keystrokes("escape");
            next_frame(cx);
            assert!(
                app.read_with(cx, |app, _| app.file_menu.is_none()) && focused(cx),
                "{case}: Escape closed the keyboard's menu"
            );
        }
    }

    /// The Settings page's identity fields and actions wait for a write's
    /// busy look too: they refuse at once, the fields read-only, and dim,
    /// with Save reading "Working…", only once the write has run 200 ms. The
    /// page, a view of its own, is drawn again when the look shows.
    #[gpui::test]
    async fn settings_identity_dims_only_once_a_write_looks_busy(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 1)).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        // The page is a view of its own, drawn again only when what it shows
        // changes.
        crate::workspace::refusals::forget();
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.show_settings(window, cx);
                app.identity_name
                    .update(cx, |input, cx| input.set_value("Another Name", window, cx));
            })
        });
        draw(cx);
        let controls = [
            "settings-field-Name",
            "settings-field-Email",
            "save-repository-identity",
            "settings-manage-profiles",
        ];
        let states = |announced, dimmed| {
            controls
                .iter()
                .map(|key| (*key, Some(Refusal { announced, dimmed })))
                .collect::<Vec<_>>()
        };
        assert_eq!(refusals_of(&controls), states(false, false), "at rest");
        let held = held_write(&app, cx);
        assert_eq!(
            refusals_of(&controls),
            states(true, false),
            "the write starts"
        );
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        cx.executor().run_until_parked();
        next_frame(cx);
        assert_eq!(
            refusals_of(&controls),
            states(true, true),
            "once the write looks busy"
        );
        release(&app, held, cx).await;
    }

    /// After gamma's Stage moves its row, epsilon's row takes its place under
    /// the pointer, which has not moved: epsilon's Stage draws no hover fill
    /// and no tooltip shows there, as while the person never moved to it.
    /// Nor does any control's that was under the pointer: the pressed one's
    /// "Stage gamma.txt", requested before the rows moved, whether the press
    /// came at once, while that tooltip waited out its delay or while it
    /// showed, once appeared over epsilon's row about a second after the
    /// press and stayed until the pointer moved; and gamma's own control in
    /// Staged, which starts its flight under the pointer, said "Staged".
    /// Once the pointer moves a pixel, epsilon's Stage lights up and its own
    /// tooltip shows. A pointer that leaves for Fetch before the rows land
    /// is not still: Fetch's tooltip shows there, where hiding the window's
    /// tooltip as the rows landed once cancelled it for good.
    #[gpui::test]
    async fn a_still_pointer_lights_nothing_the_rows_moved_under_it(cx: &mut TestAppContext) {
        // How long the pointer rests on gamma's Stage before pressing it:
        // not at all, less than the tooltip's 500 ms delay, and past it; and
        // whether it then leaves the list for Fetch before the rows land.
        for (rest, leaves) in [
            (None, false),
            (Some(300), false),
            (Some(600), false),
            (None, true),
        ] {
            let (_fixture, app, cx) = six_files(cx).await;
            let (_, palette) = crate::appearance::test_palettes().remove(0);
            cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
            // Tall enough that every row is in view.
            cx.simulate_resize(size(px(1480.), px(1300.)));
            draw(cx);
            let repository = app.read_with(cx, |app, _| app.path.clone());
            let tooltip = |name: &str, area: ChangeArea| -> &'static str {
                format!(
                    "{}/tooltip/rest",
                    workspace::confirmations::file(&repository, Path::new(name), area).control()
                )
                .leak()
            };
            let (gamma, epsilon) = (
                tooltip("gamma.txt", ChangeArea::Unstaged),
                tooltip("epsilon.txt", ChangeArea::Unstaged),
            );
            // Every tooltip, whatever its control: the kit's rounded popup
            // on the popover fill, at whatever opacity its fade has reached,
            // apart from what rests on that fill before the press (the
            // fields; a row, which may take the same fill, is square).
            let popover: Hsla =
                cx.read(|cx| *gpui_kit::component::Theme::global(cx).tokens.popover);
            let popups = |cx: &mut VisualTestContext| -> Vec<Bounds<ScaledPixels>> {
                cx.update(|window, _| {
                    window
                        .painted_quads()
                        .into_iter()
                        .filter(|quad| {
                            quad.corner_radii.top_left > ScaledPixels(0.)
                                && quad.background.as_solid().is_some_and(|fill| {
                                    fill.a > 0.
                                        && Hsla {
                                            a: popover.a,
                                            ..fill
                                        } == popover
                                })
                        })
                        .map(|quad| quad.bounds)
                        .collect()
                })
            };
            let resting = popups(cx);
            let tooltips = |cx: &mut VisualTestContext| {
                popups(cx)
                    .into_iter()
                    .filter(|popup| !resting.contains(popup))
                    .count()
            };
            let fills = |cx: &mut VisualTestContext| {
                let control = cx.debug_bounds("working-stage-6").expect("row 6's Stage");
                crate::appearance::painted_button(cx, control).0
            };
            // [Staged 2, alpha, beta, Unstaged 4, delta, epsilon, gamma, zeta]
            assert_eq!(position_of(&app, cx, "gamma.txt", ChangeArea::Unstaged), 6);
            let pointer = cx.debug_bounds("working-stage-6").unwrap().center();
            if let Some(rest) = rest {
                cx.simulate_mouse_move(pointer, None, Modifiers::default());
                draw(cx);
                advance(cx, rest);
                let showing = rest > 500;
                assert_eq!(
                    (cx.debug_bounds(gamma).is_some(), tooltips(cx)),
                    (showing, usize::from(showing)),
                    "{rest} ms on gamma's Stage"
                );
            }
            if leaves {
                let fetch = cx.debug_bounds("fetch").expect("Fetch").center();
                cx.simulate_click(pointer, Modifiers::default());
                cx.simulate_mouse_move(fetch, None, Modifiers::default());
                draw(cx);
                let write = app
                    .update(cx, |app, _| app.operation_task.take())
                    .expect("the write");
                write.await;
                rows_arrived(&app, cx).await;
                advance(cx, ROW_TRAVEL.as_millis() as u64);
                advance(cx, 500);
                assert_eq!(tooltips(cx), 1, "Fetch's tooltip under the pointer");
                continue;
            }
            toggle(&app, cx, 6).await;
            advance(cx, ROW_TRAVEL.as_millis() as u64);
            assert_eq!(flying(&app, cx), None);
            // [Staged 3, alpha, beta, gamma, Unstaged 3, delta, epsilon, zeta]
            assert_eq!(
                position_of(&app, cx, "epsilon.txt", ChangeArea::Unstaged),
                6
            );
            assert!(
                cx.debug_bounds("working-stage-6")
                    .unwrap()
                    .contains(&pointer)
            );
            assert_eq!(fills(cx), [], "{rest:?}: no hover under the still pointer");
            for after in [400, 800, 1200, 1600] {
                advance(cx, 400);
                assert_eq!(tooltips(cx), 0, "{rest:?}: no tooltip {after} ms on");
            }

            cx.simulate_mouse_move(pointer + point(px(1.), px(0.)), None, Modifiers::default());
            draw(cx);
            assert_eq!(
                fills(cx).len(),
                1,
                "{rest:?}: the moved pointer lights it up"
            );
            advance(cx, 1000);
            assert_eq!(
                (cx.debug_bounds(epsilon).is_some(), tooltips(cx)),
                (true, 1),
                "{rest:?}: and shows its own tooltip"
            );
        }
    }

    /// Under reduced motion the row is in its slot in the first frame the
    /// new rows draw, and nothing departs or flies.
    #[gpui::test]
    async fn reduced_motion_shows_the_final_place_at_once(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = working(cx, true).await;
        toggle(&app, cx, 4).await;
        app.read_with(cx, |app, _| {
            assert!(matches!(app.working_travel, RowTravel::Idle));
        });
        draw(cx);
        let beta = row(cx, 2);
        assert_slot(
            "the first frame",
            row(cx, 3),
            Bounds::new(point(beta.left(), beta.bottom()), beta.size),
        );
    }

    /// A Stage that fails leaves its row where it was and nothing waiting: a
    /// later refresh that finds the file staged from outside the app moves
    /// the row at once, since only the user's own Stage or Unstage travels.
    #[gpui::test]
    async fn only_a_finished_stage_moves_a_row(cx: &mut TestAppContext) {
        let (fixture, app, cx) = working(cx, false).await;
        let root = fixture.path().join("narrow");
        let lock = root.join(".git").join("index.lock");
        std::fs::write(&lock, "").unwrap();
        toggle(&app, cx, 4).await;
        app.read_with(cx, |app, _| {
            assert!(app.operation_error.is_some() || app.error.is_some());
            assert!(matches!(app.working_travel, RowTravel::Idle));
        });
        assert_eq!(
            app.read_with(cx, |app, _| app.working_paths.staged_count),
            2
        );
        std::fs::remove_file(&lock).unwrap();
        fixture_git(&root, &["add", "notes.txt"]);
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        rows_arrived(&app, cx).await;
        assert_eq!(
            app.read_with(cx, |app, _| app.working_paths.staged_count),
            3
        );
        assert_eq!(flying(&app, cx), None);
        draw(cx);
        let beta = row(cx, 2);
        assert_slot(
            "after the refresh",
            row(cx, 3),
            Bounds::new(point(beta.left(), beta.bottom()), beta.size),
        );
    }
}
