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
                        .is_some_and(|entry| {
                            crate::working_selection::same_path(&entry.path, &still.path)
                        })
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
                        if *at == area && status.entries[*index].path.as_os_str() == std::ffi::OsStr::new(name))
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
    /// prepares nothing. A merge's Revert that a press still reaches while
    /// paused, as Enter could in a keyboard's menu built only as it opened,
    /// asks for no mainline and prepares nothing either, and reports the
    /// core's refusal; it closed the menu and did nothing visible. Review
    /// restore… in Stashes has the core read the pause and says why in
    /// Stashes, which stays open, and nowhere else. The menus were built
    /// again only as a write started or ended, so a cherry-pick that
    /// stopped on conflicts left those items offered until the menu was
    /// opened again, and a press read a plan that the core then refused.
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

        // A merge's Revert, as a press that still reached it while paused
        // chose it.
        let merge = app.read_with(cx, |app, _| {
            app.commits
                .iter()
                .position(|commit| commit.parents.len() > 1)
                .map(|merge| crate::recovery::commit_target(&app.commits[merge]))
                .expect("main's merge")
        });
        pause();
        refreshed(&app, cx).await;
        assert!(paused(cx), "the merge paused again");
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.choose_commit_recovery(gitturtle_core::RecoveryKind::Revert, merge, window, cx)
            })
        });
        cx.executor().run_until_parked();
        assert_eq!(
            quiet(cx),
            (None, Some(crate::recovery::PAUSED_REFUSAL.into())),
            "Revert prepares nothing and says why"
        );
        assert!(!dialog_open(cx), "Revert asks for no mainline");
        drop(held);
        app.update(cx, |app, _| app.operation_error = None);

        // Review restore… has the core read the pause rather than refuse
        // from the window's, and says why in Stashes, which stays open.
        crate::recovery::open_selected_stash(&app, cx).await;
        next_frame(cx);
        press(cx, "restore-saved-stash");
        app.read_with(cx, |app, _| app.operations.drain());
        cx.executor().run_until_parked();
        next_frame(cx);
        assert_eq!(
            quiet(cx),
            (None, None),
            "Review restore… says why in Stashes, not in the window"
        );
        assert!(
            dialog_open(cx) && cx.debug_bounds("stash-restore-refusal").is_some(),
            "Stashes stays open with its refusal"
        );
        assert_eq!(
            app.read_with(cx, crate::recovery::stash_restore_line),
            Some(crate::recovery::PAUSED_REFUSAL.into()),
            "the refusal is the core's, for the paused merge"
        );
        cx.update(|window, cx| window.close_all_dialogs(cx));
        next_frame(cx);

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

    /// The rows `keyboard_row_menus_follow_writes_and_pauses` opens a menu
    /// on from the keyboard.
    #[derive(Clone, Copy, Debug)]
    enum KeyedRow {
        /// HEAD's row in History, whose menu adds Amend and Undo.
        Commit,
        /// A navigator branch, by its row.
        Branch(usize),
        /// A navigator worktree, by its row, whose menu holds three items.
        Worktree(usize),
        /// extra.txt's untracked row in Changes.
        Working,
    }

    impl KeyedRow {
        /// The items its menu holds that a write refuses.
        fn items(self) -> &'static [&'static str] {
            match self {
                Self::Commit => &COMMIT_ITEMS,
                Self::Branch(_) => &["Branch actions…"],
                Self::Worktree(_) => &WORKTREE_ITEMS,
                Self::Working => &["Ignore…", "Delete untracked file…"],
            }
        }
    }

    /// A worktree's menu: every item a write refuses.
    const WORKTREE_ITEMS: [&str; 3] = [
        "Worktree actions…",
        "Remove worktree…",
        "Force remove worktree…",
    ];

    /// HEAD's menu: every item a write or a paused integration refuses.
    const COMMIT_ITEMS: [&str; 4] = [
        "Revert this commit…",
        "Cherry-pick this commit…",
        "Amend last commit…",
        "Undo last local commit…",
    ];

    /// Selects `row`, focuses its list or tree, and opens its menu with
    /// `key`.
    async fn open_keyed_menu(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        row: KeyedRow,
        key: &str,
    ) {
        match row {
            KeyedRow::Commit | KeyedRow::Branch(_) | KeyedRow::Worktree(_) => {
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| {
                        app.show_history(window, cx);
                        let head = app
                            .work_status
                            .as_ref()
                            .and_then(|status| status.head.clone());
                        let head = app
                            .commits
                            .iter()
                            .position(|commit| Some(&commit.oid) == head.as_ref())
                            .expect("HEAD in History");
                        app.select_commit(head, window, cx);
                        app.pane = Pane::History;
                    })
                });
                crate::views::tests::settle(app, cx).await;
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| match row {
                        KeyedRow::Branch(row) | KeyedRow::Worktree(row) => {
                            app.nav_cursor = Some(row);
                            window.focus(&app.nav_focus, cx);
                        }
                        _ => window.focus(&app.focus, cx),
                    })
                });
            }
            KeyedRow::Working => {
                cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
                rows_arrived(app, cx).await;
                draw(cx);
                let position = position_of(app, cx, "extra.txt", ChangeArea::Unstaged);
                let bounds = cx
                    .debug_bounds(format!("working-row-{position}").leak())
                    .expect("extra.txt's row");
                cx.simulate_click(bounds.center(), Modifiers::default());
                let list = app.read_with(cx, |app, _| app.file_focus.clone());
                cx.update(|window, cx| list.focus(window, cx));
            }
        }
        next_frame(cx);
        cx.simulate_keystrokes(key);
        next_frame(cx);
        let opened = app.read_with(cx, |app, _| match row {
            KeyedRow::Commit => app.commit_menu.is_some(),
            KeyedRow::Branch(_) | KeyedRow::Worktree(_) => app.nav_menu.is_some(),
            KeyedRow::Working => app.file_menu.is_some(),
        });
        assert!(opened, "{key} opened {row:?}'s menu");
    }

    /// Whether the open keyboard's menu holds focus.
    fn focus_in_keyed_menu(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| {
            let app = app.read(cx);
            [&app.commit_menu, &app.nav_menu, &app.file_menu]
                .into_iter()
                .flatten()
                .any(|menu| menu.has_focus(window, cx))
        })
    }

    /// Asserts that the open menu draws each of `items` available, or
    /// disabled, with focus in the menu.
    fn keyed_items(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        items: &[&str],
        available: bool,
        when: &str,
    ) {
        for item in items {
            assert_eq!(
                offered(cx, item),
                Some(available),
                "{when}: {item} {}",
                if available { "available" } else { "disabled" }
            );
        }
        assert!(
            focus_in_keyed_menu(app, cx),
            "{when}: focus stays in the menu"
        );
    }

    /// Escape closes the open keyboard's menu.
    fn close_keyed_menu(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, when: &str) {
        cx.simulate_keystrokes("escape");
        cx.executor().run_until_parked();
        next_frame(cx);
        assert!(
            app.read_with(cx, |app, _| app.commit_menu.is_none()
                && app.nav_menu.is_none()
                && app.file_menu.is_none()),
            "{when}: Escape closed the menu"
        );
    }

    /// Enter in a commit's keyboard menu while a merge is paused, twice
    /// without the error dismissed between: each prepares nothing and
    /// reports the core's refusal in the window's error, announced at once,
    /// the second by a fresh node, so it is heard again, the menu open and
    /// focused with its items disabled.
    fn enter_is_refused(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, when: &str) {
        let held = app.update(cx, |app, _| app.operations.hold());
        let mut nodes = Vec::new();
        for press in ["Enter", "Enter again"] {
            cx.simulate_keystrokes("enter");
            cx.executor().run_until_parked();
            next_frame(cx);
            assert_eq!(
                app.read_with(cx, |app, _| (
                    app.operation_busy,
                    app.operation_error.clone()
                )),
                (None, Some(crate::recovery::PAUSED_REFUSAL.into())),
                "{when}, {press}: prepares nothing and says why"
            );
            assert!(
                cx.debug_bounds("operation-error-summary").is_some(),
                "{when}, {press}: the window's error draws the refusal"
            );
            let mut announced =
                crate::native_accessibility::written::announced_from("operation-error-summary-");
            assert_eq!(
                announced.len(),
                1,
                "{when}, {press}: one refusal announced: {announced:?}"
            );
            let (node, how) = announced.pop().expect("the refusal");
            assert_eq!(
                how,
                crate::native_accessibility::written::Announced::ASSERTIVE,
                "{when}, {press}: the refusal is announced at once"
            );
            // Its name, which AT-SPI announces, and its value, which macOS
            // does.
            let written = crate::native_accessibility::written::get(&node)
                .unwrap_or_else(|| panic!("{when}, {press}: the refusal's node"));
            assert_eq!(
                (written.node.label(), written.node.value()),
                (
                    Some(crate::recovery::PAUSED_REFUSAL),
                    Some(crate::recovery::PAUSED_REFUSAL)
                ),
                "{when}, {press}: the refusal is its node's name and value"
            );
            nodes.push(node);
            assert!(!dialog_open(cx), "{when}, {press}: asks nothing");
            keyed_items(
                app,
                cx,
                &COMMIT_ITEMS,
                false,
                &format!("{when}, after {press}"),
            );
        }
        assert_ne!(
            nodes[0], nodes[1],
            "{when}: the refusal asked for again is a fresh node, heard again"
        );
        drop(held);
        app.update(cx, |app, _| app.operation_error = None);
    }

    /// A row's menu opened from the keyboard, by Shift-F10 and on Linux by
    /// the Menu key, follows writes and pauses as a right-click menu does,
    /// built again in place with focus kept in it: a History commit's, a
    /// navigator branch's and an untracked working row's items are disabled
    /// while a write runs, whether it started before or after the menu
    /// opened, and available once it ends. A commit's Revert, Cherry-pick,
    /// Amend and Undo are disabled while a merge is paused, whether it paused
    /// before or after the menu opened, and Enter there reports the core's
    /// refusal in the window's error, announced at once, preparing nothing
    /// and leaving the menu open and focused; they are available once the
    /// merge is abandoned. Each read the write and the pause once, as it
    /// opened: a menu opened at rest offered Revert under a paused merge and
    /// one opened during a quick write kept its items disabled after it.
    #[gpui::test]
    async fn keyboard_row_menus_follow_writes_and_pauses(cx: &mut TestAppContext) {
        let (fixture, app, cx) = history_window_from(cx, history_script(40, 3)).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        cx.update(|_, cx| {
            crate::shortcuts::bind_keys(cx);
            crate::native_accessibility::bind_keys(cx);
        });
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let identity = [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ];
        let git =
            |arguments: &[&str]| fixture_git(&repository, &[&identity[..], arguments].concat());
        git(&["reset", "-q", "--hard"]);
        // A side branch to merge without its commit, and an untracked file.
        git(&["checkout", "-q", "-b", "side"]);
        std::fs::write(repository.join("side.txt"), "side\n").unwrap();
        git(&["add", "side.txt"]);
        git(&["commit", "-q", "-m", "side"]);
        git(&["checkout", "-q", "main"]);
        std::fs::write(repository.join("extra.txt"), "extra\n").unwrap();
        // History takes in the side branch's commit now, above HEAD, rather
        // than under an open menu, which its row moving would close.
        refs_quietly_refreshed(&app, cx).await;
        let paused = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| app.integration_state.is_some())
        };
        let write =
            |cx: &mut VisualTestContext| held(&app, cx, gitturtle_core::WriteCommand::UnstageAll);
        let branch = app.read_with(cx, |app, _| {
            app.nav_rows
                .iter()
                .position(|row| matches!(row, crate::NavRow::Branch(..)))
                .expect("a branch in the navigator")
        });
        let keys: &[&str] = if cfg!(target_os = "macos") {
            &["shift-f10"]
        } else {
            &["shift-f10", "menu"]
        };
        for key in keys {
            for row in [
                KeyedRow::Commit,
                KeyedRow::Branch(branch),
                KeyedRow::Working,
            ] {
                let items = row.items();
                let on = format!("{key} on {row:?}");
                // Opened at rest, as a write starts and ends.
                open_keyed_menu(&app, cx, row, key).await;
                keyed_items(&app, cx, items, true, &format!("{on} at rest"));
                let held = write(cx);
                next_frame(cx);
                keyed_items(&app, cx, items, false, &format!("{on} once a write starts"));
                release(&app, held, cx).await;
                next_frame(cx);
                keyed_items(&app, cx, items, true, &format!("{on} once the write ends"));
                close_keyed_menu(&app, cx, &on);
                // Opened while a write runs, until it ends.
                let held = write(cx);
                open_keyed_menu(&app, cx, row, key).await;
                keyed_items(
                    &app,
                    cx,
                    items,
                    false,
                    &format!("{on} opened during a write"),
                );
                release(&app, held, cx).await;
                next_frame(cx);
                keyed_items(&app, cx, items, true, &format!("{on} once that write ends"));
                close_keyed_menu(&app, cx, &on);
            }

            // A commit's menu opened at rest, as a merge pauses and ends.
            let on = format!("{key} on a commit");
            open_keyed_menu(&app, cx, KeyedRow::Commit, key).await;
            keyed_items(&app, cx, &COMMIT_ITEMS, true, &format!("{on} at rest"));
            git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
            refreshed(&app, cx).await;
            assert!(paused(cx), "{on}: the merge paused");
            keyed_items(
                &app,
                cx,
                &COMMIT_ITEMS,
                false,
                &format!("{on} once a merge pauses"),
            );
            enter_is_refused(&app, cx, &format!("{on} once a merge pauses"));
            git(&["merge", "--abort"]);
            refreshed(&app, cx).await;
            assert!(!paused(cx), "{on}: the merge was abandoned");
            keyed_items(
                &app,
                cx,
                &COMMIT_ITEMS,
                true,
                &format!("{on} once the merge ends"),
            );
            close_keyed_menu(&app, cx, &on);
            // Opened while a merge is paused, until it ends.
            git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
            refreshed(&app, cx).await;
            assert!(paused(cx), "{on}: the merge paused again");
            open_keyed_menu(&app, cx, KeyedRow::Commit, key).await;
            keyed_items(
                &app,
                cx,
                &COMMIT_ITEMS,
                false,
                &format!("{on} opened while paused"),
            );
            enter_is_refused(&app, cx, &format!("{on} opened while paused"));
            git(&["merge", "--abort"]);
            refreshed(&app, cx).await;
            assert!(!paused(cx), "{on}: the merge was abandoned again");
            keyed_items(
                &app,
                cx,
                &COMMIT_ITEMS,
                true,
                &format!("{on} once that merge ends"),
            );
            close_keyed_menu(&app, cx, &on);
        }
        drop(fixture);
    }

    /// Lets every quiet read the window has asked for land: each runs on
    /// the window's reader, a thread `run_until_parked` cannot wait for.
    fn quiet_reads_land(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        for _ in 0..8 {
            if !app.read_with(cx, |app, _| app.automatic.quiet_read_requested()) {
                return;
            }
            app.read_with(cx, |app, _| app.worker.wait_idle());
            cx.executor().run_until_parked();
        }
        panic!("a quiet read asked for never ran");
    }

    /// Reads the repository again as the local watcher does after a change
    /// outside the app, its refs and History included, lets the read land
    /// and draws the frame after it.
    async fn refs_quietly_refreshed(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.queue_automatic_refresh(
                    crate::local_refresh::LocalChange {
                        git: true,
                        worktree: true,
                        ..Default::default()
                    },
                    window,
                    cx,
                )
            })
        });
        quiet_reads_land(app, cx);
        crate::views::tests::settle(app, cx).await;
        next_frame(cx);
    }

    /// A commit's keyboard menu opened on HEAD while an Undo runs closes,
    /// with focus back on History's list, once the write's quiet reload
    /// takes that commit from the row it opened beside. Built again for the
    /// row it opened on, it offered Revert and Cherry-pick of the commit
    /// the Undo had left behind, over the row of the new HEAD.
    #[gpui::test]
    async fn a_keyboard_commit_menu_closes_once_a_write_takes_its_commit(cx: &mut TestAppContext) {
        let (fixture, app, cx) = history_window_from(cx, history_script(40, 3)).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        cx.update(|_, cx| {
            crate::shortcuts::bind_keys(cx);
            crate::native_accessibility::bind_keys(cx);
        });
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        fixture_git(&repository, &["reset", "-q", "--hard"]);
        refreshed(&app, cx).await;
        let head = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                app.work_status
                    .as_ref()
                    .and_then(|status| status.head.clone())
                    .expect("HEAD")
            })
        };
        let before = head(cx);
        let repo = app.read_with(cx, |app, _| app.repository.clone().expect("the repository"));
        let plan = repo
            .recovery_plan(gitturtle_core::RecoveryKind::Undo, Some(&before), None)
            .expect("an Undo plan");
        let undo = gitturtle_core::WriteCommand::Recovery(std::sync::Arc::new(
            gitturtle_core::RecoveryCommand::Undo { plan },
        ));
        // Shift-F10 on HEAD while the Undo runs.
        let write = held(&app, cx, undo);
        open_keyed_menu(&app, cx, KeyedRow::Commit, "shift-f10").await;
        keyed_items(&app, cx, &COMMIT_ITEMS, false, "during the Undo");
        release(&app, write, cx).await;
        quiet_reads_land(&app, cx);
        crate::views::tests::settle(&app, cx).await;
        next_frame(cx);
        let after = head(cx);
        assert_ne!(after, before, "the Undo moved HEAD");
        assert_eq!(
            app.read_with(cx, |app, _| app
                .visible
                .first()
                .and_then(|index| app.commits.get(*index))
                .map(|commit| commit.oid.clone())),
            Some(after),
            "the row the menu opened beside holds the new HEAD"
        );
        assert!(
            app.read_with(cx, |app, _| app.commit_menu.is_none()),
            "the menu closed once its commit left the row"
        );
        assert_eq!(
            offered(cx, "Revert this commit…"),
            None,
            "no Revert of the undone commit is offered"
        );
        assert!(
            cx.update(|window, cx| app.read(cx).focus.is_focused(window)),
            "focus is back on History's list"
        );
        drop(fixture);
    }

    /// How a commit's menu opens: as a working row's does
    /// ([`Opening`]), or from the selected commit's Actions button in the
    /// inspector's message toolbar.
    #[derive(Clone, Copy, Debug)]
    enum CommitOpening {
        RightClick,
        Keyboard,
        Actions,
    }

    /// A commit's menu opened on HEAD, as `opening` opens it, is built again
    /// in place, a keyboard's with focus kept in it, once HEAD moves off the
    /// commit while the commit stays at its row, as a `git reset` outside
    /// the app moves it with another branch still on the commit: it no
    /// longer offers Amend or Undo, which act on HEAD, and offers them again
    /// once HEAD returns. Built again only as a write or a pause started or
    /// ended, it kept offering Amend and Undo of a commit that was no longer
    /// HEAD; HEAD counts for a commit's menu alone, so each names its commit
    /// to the menu that follows it.
    async fn a_commit_menu_follows_head(cx: &mut TestAppContext, opening: CommitOpening) {
        const AMEND: &str = "Amend last commit…";
        const UNDO: &str = "Undo last local commit…";
        let (fixture, app, cx) = clean_history_for_menus(cx).await;
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let git = |arguments: &[&str]| fixture_git(&repository, arguments);
        git(&["branch", "keep"]);
        refs_quietly_refreshed(&app, cx).await;
        let head = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                app.work_status
                    .as_ref()
                    .and_then(|status| status.head.clone())
                    .expect("HEAD")
            })
        };
        let available = |cx: &mut VisualTestContext, items: &[&str], when: &str| {
            for item in items {
                assert_eq!(
                    offered(cx, item),
                    Some(true),
                    "{opening:?} {when}: {item} available"
                );
            }
            if matches!(opening, CommitOpening::Keyboard) {
                assert!(
                    focus_in_keyed_menu(&app, cx),
                    "{opening:?} {when}: focus stays in the menu"
                );
            }
        };
        let opened = head(cx);
        match opening {
            CommitOpening::Keyboard => {
                open_keyed_menu(&app, cx, KeyedRow::Commit, "shift-f10").await
            }
            // HEAD's row, the newest commit.
            CommitOpening::RightClick => right_click(cx, "history-row-0"),
            CommitOpening::Actions => {
                cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(0, window, cx)));
                crate::views::tests::settle(&app, cx).await;
                next_frame(cx);
                press(cx, "commit-recovery-menu");
                next_frame(cx);
            }
        }
        available(cx, &COMMIT_ITEMS, "on HEAD");
        git(&["reset", "-q", "--hard", "HEAD~1"]);
        refs_quietly_refreshed(&app, cx).await;
        assert_ne!(head(cx), opened, "{opening:?}: the reset moved HEAD");
        assert_eq!(
            app.read_with(cx, |app, _| app
                .visible
                .first()
                .and_then(|index| app.commits.get(*index))
                .map(|commit| commit.oid.clone())),
            Some(opened.clone()),
            "{opening:?}: keep holds the commit at its row"
        );
        assert_eq!(
            offered(cx, AMEND),
            None,
            "{opening:?}: no Amend once HEAD moves off"
        );
        assert_eq!(
            offered(cx, UNDO),
            None,
            "{opening:?}: no Undo once HEAD moves off"
        );
        available(cx, &COMMIT_ITEMS[..2], "once HEAD moves off");
        git(&["reset", "-q", "--hard", "keep"]);
        refs_quietly_refreshed(&app, cx).await;
        assert_eq!(head(cx), opened, "{opening:?}: HEAD returned");
        available(cx, &COMMIT_ITEMS, "once HEAD returns");
        cx.simulate_keystrokes("escape");
        next_frame(cx);
        drop(fixture);
    }

    #[gpui::test]
    async fn a_keyboard_commit_menu_follows_head(cx: &mut TestAppContext) {
        a_commit_menu_follows_head(cx, CommitOpening::Keyboard).await;
    }

    #[gpui::test]
    async fn a_right_click_commit_menu_follows_head(cx: &mut TestAppContext) {
        a_commit_menu_follows_head(cx, CommitOpening::RightClick).await;
    }

    #[gpui::test]
    async fn the_commit_actions_menu_follows_head(cx: &mut TestAppContext) {
        a_commit_menu_follows_head(cx, CommitOpening::Actions).await;
    }

    /// Whether the open menu's keyboard cursor is on `item`, available: the
    /// item's row draws the toolkit's `list_active` fill, which only the
    /// keyboard's cursor draws.
    fn cursor_on(cx: &mut VisualTestContext, item: &str) -> bool {
        let bounds = cx
            .debug_bounds(format!("menu-item: {item} · MenuItem").leak())
            .unwrap_or_else(|| panic!("{item} offered"));
        let fill = cx.update(|_, cx| {
            gpui_kit::Background::from(gpui_kit::component::Theme::global(cx).list_active)
        });
        crate::appearance::painted_row(cx, bounds).0.contains(&fill)
    }

    /// Commits outside the app, as a terminal beside it does, moving HEAD
    /// to a new commit of the same tree, which leaves every file's status as
    /// it was, what is staged included.
    fn commit_outside(repository: &std::path::Path) {
        fixture_git(
            repository,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-q",
                "--allow-empty",
                "--only",
                "-m",
                "Outside",
            ],
        );
    }

    /// A working row's right-click menu, its keyboard's cursor moved to
    /// Delete untracked file…, keeps it there as HEAD moves under it, as a
    /// commit outside the app moves it: the menu reads nothing HEAD changes
    /// and is not built again. Built again whenever HEAD moved, it lost its
    /// highlight.
    #[gpui::test]
    async fn a_right_click_working_menu_keeps_its_cursor_as_head_moves(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = six_files_for_menus(cx).await;
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let head = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                app.work_status
                    .as_ref()
                    .and_then(|status| status.head.clone())
            })
        };
        open_row_menu(&app, cx, "gamma.txt", Opening::RightClick);
        cx.simulate_keystrokes("down down");
        next_frame(cx);
        assert!(
            cursor_on(cx, "Delete untracked file…"),
            "Down twice moved the cursor to Delete untracked file…"
        );
        let before = head(cx);
        commit_outside(&repository);
        quietly_refreshed(&app, cx);
        next_frame(cx);
        assert_ne!(head(cx), before, "the commit moved HEAD");
        assert!(
            cursor_on(cx, "Delete untracked file…"),
            "the cursor stays on Delete untracked file… as HEAD moves"
        );
        assert!(!cursor_on(cx, "Ignore…"), "and only there");
    }

    /// A navigator worktree's keyboard menu, its cursor moved to Remove
    /// worktree…, keeps it there as HEAD moves, as a commit outside the app
    /// moves it, which the refresh after it reads with the navigator's rows:
    /// a branch's or a worktree's menu reads nothing HEAD changes and is not
    /// built again. Built again whenever HEAD moved, its cursor went back to
    /// Worktree actions…, so Enter just after the refresh reviewed the
    /// worktree rather than removing it. A branch's menu, built the same
    /// way, holds one item, so its cursor cannot show it.
    #[gpui::test]
    async fn a_keyboard_worktree_menu_keeps_its_cursor_as_head_moves(cx: &mut TestAppContext) {
        let (fixture, app, cx) = history_window_from(cx, history_script(40, 3)).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        cx.update(|_, cx| {
            crate::shortcuts::bind_keys(cx);
            crate::native_accessibility::bind_keys(cx);
        });
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        fixture_git(&repository, &["reset", "-q", "--hard"]);
        refs_quietly_refreshed(&app, cx).await;
        let row = |cx: &mut VisualTestContext| {
            app.update(cx, |app, cx| {
                app.nav_mode = crate::NavMode::Worktrees;
                app.rebuild_navigation(cx);
                app.nav_rows
                    .iter()
                    .position(|row| matches!(row, crate::NavRow::Worktree(_)))
            })
        };
        let row = row(cx).expect("the repository's own worktree in the navigator");
        let head = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                app.work_status
                    .as_ref()
                    .and_then(|status| status.head.clone())
            })
        };
        open_keyed_menu(&app, cx, KeyedRow::Worktree(row), "shift-f10").await;
        keyed_items(&app, cx, &WORKTREE_ITEMS, true, "at rest");
        assert!(
            cursor_on(cx, "Worktree actions…"),
            "it opens on its first item"
        );
        cx.simulate_keystrokes("down");
        next_frame(cx);
        assert!(cursor_on(cx, "Remove worktree…"), "Down moved the cursor");
        let before = head(cx);
        commit_outside(&repository);
        refs_quietly_refreshed(&app, cx).await;
        assert_ne!(head(cx), before, "the commit moved HEAD");
        assert!(
            app.read_with(cx, |app, _| app.nav_menu.is_some()),
            "the menu stays open on its row"
        );
        keyed_items(&app, cx, &WORKTREE_ITEMS, true, "after HEAD moved");
        assert!(
            cursor_on(cx, "Remove worktree…"),
            "the cursor stays on Remove worktree… as HEAD moves"
        );
        assert!(!cursor_on(cx, "Worktree actions…"), "and only there");
        cx.simulate_keystrokes("escape");
        next_frame(cx);
        drop(fixture);
    }

    /// A navigator branch's keyboard menu stays open through a quiet refresh
    /// that changes nothing, and closes, with focus back on the tree, once
    /// its branch is deleted or renamed outside the app and the refresh
    /// moves the tree's rows: its row then holds another branch, which
    /// Branch actions… would have acted on the old name of.
    #[gpui::test]
    async fn a_keyboard_branch_menu_closes_once_its_branch_goes(cx: &mut TestAppContext) {
        let (fixture, app, cx) = history_window_from(cx, history_script(40, 3)).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        cx.update(|_, cx| {
            crate::shortcuts::bind_keys(cx);
            crate::native_accessibility::bind_keys(cx);
        });
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let git = |arguments: &[&str]| fixture_git(&repository, arguments);
        git(&["reset", "-q", "--hard"]);
        git(&["branch", "doomed"]);
        git(&["branch", "renamed"]);
        refs_quietly_refreshed(&app, cx).await;
        let row_of = |cx: &mut VisualTestContext, name: &str| {
            app.read_with(cx, |app, _| {
                app.nav_rows.iter().position(|row| {
                    matches!(row, crate::NavRow::Branch(branch, _)
                        if app.branches[*branch].name == name)
                })
            })
        };
        let item = ["Branch actions…"];
        let changes: [(&str, &[&str]); 2] = [
            ("doomed", &["branch", "-D", "doomed"]),
            ("renamed", &["branch", "-m", "renamed", "zz-renamed"]),
        ];
        for (name, change) in changes {
            let row = row_of(cx, name).unwrap_or_else(|| panic!("{name} in the navigator"));
            open_keyed_menu(&app, cx, KeyedRow::Branch(row), "shift-f10").await;
            keyed_items(&app, cx, &item, true, &format!("{name} at rest"));
            refs_quietly_refreshed(&app, cx).await;
            assert!(
                app.read_with(cx, |app, _| app.nav_menu.is_some()),
                "{name}: a refresh that changes nothing keeps the menu"
            );
            keyed_items(&app, cx, &item, true, &format!("{name} after that refresh"));
            git(change);
            refs_quietly_refreshed(&app, cx).await;
            assert_eq!(row_of(cx, name), None, "{name} left the navigator");
            assert!(
                app.read_with(cx, |app, _| app.nav_menu.is_none()),
                "{name}: the menu closed once its branch left the row"
            );
            assert_eq!(offered(cx, item[0]), None, "{name}: nothing is offered");
            assert!(
                cx.update(|window, cx| app.read(cx).nav_focus.is_focused(window)),
                "{name}: focus is back on the tree"
            );
        }
        drop(fixture);
    }

    /// A changed file's keyboard menu, whose File history… reads nothing a
    /// write or a pause changes, is built once: a write's start and end leave
    /// it as it was, and its keyboard's cursor with it. Following writes, it
    /// was built again at each, its cursor sent back to the first item.
    #[gpui::test]
    async fn a_changed_files_keyboard_menu_is_built_once(cx: &mut TestAppContext) {
        let (fixture, app, cx) = history_window_from(cx, history_script(40, 3)).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        cx.update(|_, cx| {
            crate::shortcuts::bind_keys(cx);
            crate::native_accessibility::bind_keys(cx);
        });
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        fixture_git(&repository, &["reset", "-q", "--hard"]);
        refreshed(&app, cx).await;
        // HEAD's first changed file highlighted, as Back from Compare leaves
        // it, its list focused.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.show_history(window, cx);
                app.select_commit(0, window, cx);
            })
        });
        crate::views::tests::settle(&app, cx).await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                assert!(!app.files.is_empty(), "the commit's changed files");
                app.selected_file = Some(0);
                app.pane = Pane::Files;
                window.focus(&app.file_focus, cx);
                cx.notify();
            })
        });
        next_frame(cx);
        cx.simulate_keystrokes("shift-f10");
        next_frame(cx);
        let menu = app.read_with(cx, |app, _| {
            app.file_menu
                .as_ref()
                .expect("the changed file's menu")
                .menu()
                .clone()
        });
        assert_eq!(offered(cx, "File history…"), Some(true), "the menu opened");
        // How often the menu is built again.
        let built = std::rc::Rc::new(std::cell::Cell::new(0));
        let _built = cx.update(|_, cx| {
            let built = built.clone();
            cx.observe(&menu, move |_, _| built.set(built.get() + 1))
        });
        let write = held_write(&app, cx);
        next_frame(cx);
        release(&app, write, cx).await;
        next_frame(cx);
        assert_eq!(built.get(), 0, "a write's start and end build it no more");
        assert_eq!(
            offered(cx, "File history…"),
            Some(true),
            "the menu stays open"
        );
        cx.simulate_keystrokes("escape");
        next_frame(cx);
        drop(fixture);
    }

    /// However a right-click menu closes, by Escape, a press outside it on
    /// the Changes heading, which gives focus to the window's root, or a
    /// resize, as a tiling compositor's re-tile does, focus returns to the
    /// list it was opened from, whose keys reach it again: Down moves the
    /// selection and Shift-F10 opens the keyboard's menu. So it does after a right-click
    /// on another row, or a third, or on the same row again, closes the menu
    /// open there and opens the row's own, and when the menu's row goes, as
    /// a quiet refresh takes away the row of a file deleted outside the app,
    /// with no dismissal at all. The resize's dismissal skipped the menu's
    /// own Cancel and left focus on the menu, which no frame drew, so Down,
    /// Space and Shift-F10 stopped reaching the list; the press on the
    /// heading left focus on the root, where nothing showed it; a menu
    /// opened by a right-click on another row returned focus to the menu it
    /// closed, which no frame drew either; and a menu that went with its row
    /// left focus on its released handle, nothing focused. The app's keys are
    /// bound here as the app binds them; a test window binds none of them
    /// (`views::tests::init_app`).
    #[gpui::test]
    async fn a_right_click_menus_focus_returns_to_its_list_however_it_closes(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, cx) = six_files(cx).await;
        // Tall enough that every row is drawn, zeta's too, after the resize.
        cx.simulate_resize(size(px(1480.), px(1300.)));
        draw(cx);
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
        let (delta_row, epsilon_row, gamma_row, zeta_row) = (
            "working-row-4",
            "working-row-5",
            "working-row-6",
            "working-row-7",
        );
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        // The rows each case right-clicks in turn, each while the menu the
        // one before opened is still open, and how it closes the last. Its
        // row going takes zeta's, the last row, away, so it comes last.
        let cases: [(&str, &[&'static str]); 7] = [
            ("Escape", &[delta_row]),
            ("a press outside", &[delta_row]),
            ("a resize", &[delta_row]),
            ("Escape", &[epsilon_row, delta_row]),
            ("Escape", &[epsilon_row, delta_row, gamma_row]),
            ("Escape", &[delta_row, delta_row]),
            ("its row going", &[zeta_row]),
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
                let drawn = below(cx, pointer)
                    .unwrap_or_else(|| panic!("{case}: {selector}'s menu opened"));
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
                    // On the Changes heading above the list, which gives
                    // focus to the window's root. A press on a row focused
                    // the list itself, whatever the menu's dismissal did.
                    let heading = cx
                        .debug_bounds("working-header-label")
                        .expect("the Changes heading");
                    cx.simulate_click(heading.center(), Modifiers::default());
                }
                "a resize" => {
                    let viewport = cx.update(|window, _| window.viewport_size());
                    cx.simulate_resize(size(viewport.width - px(80.), viewport.height - px(80.)));
                }
                _ => {
                    // Deleted outside the app, its row goes with the quiet
                    // refresh after it, and the menu with the row: no
                    // dismissal comes.
                    std::fs::remove_file(repository.join("zeta.txt")).unwrap();
                    refreshed(&app, cx).await;
                    assert_eq!(
                        app.read_with(cx, |app, _| app.working_rows.len()),
                        7,
                        "{case}: zeta's row went"
                    );
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

    /// How a working row's menu opens: a right-click on the row, or
    /// Shift-F10 on the list's cursor row.
    #[derive(Clone, Copy, Debug)]
    enum Opening {
        RightClick,
        Keyboard,
    }

    /// The working row menus' Ignore…, which an untracked file's row offers.
    const IGNORE_ITEM: &str = "menu-item: Ignore… · MenuItem";

    /// Reads the working state again as the local watcher does after a
    /// change outside the app, quietly, and draws the frame after its rows
    /// land. A Refresh reads it aloud, selecting the working file again,
    /// which moves focus to the list. The read runs on the window's reader,
    /// a thread `run_until_parked` cannot wait for.
    fn quietly_refreshed(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.queue_automatic_refresh(
                    crate::local_refresh::LocalChange {
                        worktree: true,
                        ..Default::default()
                    },
                    window,
                    cx,
                )
            })
        });
        for _ in 0..8 {
            if !app.read_with(cx, |app, _| app.automatic.quiet_read_requested()) {
                break;
            }
            app.read_with(cx, |app, _| app.worker.wait_idle());
            cx.executor().run_until_parked();
        }
        assert!(
            !app.read_with(cx, |app, _| app.automatic.quiet_read_requested()),
            "the quiet read ran"
        );
        crate::path_filter::filters_land(app, cx);
        next_frame(cx);
    }

    /// Opens the menu of `name`'s Unstaged row as `opening` opens it, the
    /// list focused and, from the keyboard, its cursor on the row. Returns
    /// the row's bounds and its menu's Ignore…'s.
    fn open_row_menu(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        name: &str,
        opening: Opening,
    ) -> (Bounds<Pixels>, Bounds<Pixels>) {
        open_row_menu_showing(app, cx, name, opening, IGNORE_ITEM)
    }

    /// [`open_row_menu`] for a menu that draws `item`, whose bounds it
    /// returns with the row's.
    fn open_row_menu_showing(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        name: &str,
        opening: Opening,
        item: &'static str,
    ) -> (Bounds<Pixels>, Bounds<Pixels>) {
        let selector: &'static str = format!(
            "working-row-{}",
            position_of(app, cx, name, ChangeArea::Unstaged)
        )
        .leak();
        let row = cx.debug_bounds(selector).expect(selector);
        let list = app.read_with(cx, |app, _| app.file_focus.clone());
        match opening {
            Opening::RightClick => {
                cx.update(|window, cx| list.focus(window, cx));
                let pointer = point(row.left() + px(16.), row.center().y);
                cx.simulate_mouse_move(pointer, None, Modifiers::default());
                cx.simulate_mouse_down(pointer, gpui::MouseButton::Right, Modifiers::default());
                cx.simulate_mouse_up(pointer, gpui::MouseButton::Right, Modifiers::default());
            }
            Opening::Keyboard => {
                cx.simulate_click(row.center(), Modifiers::default());
                cx.update(|window, cx| list.focus(window, cx));
                next_frame(cx);
                cx.simulate_keystrokes("shift-f10");
            }
        }
        cx.executor().run_until_parked();
        next_frame(cx);
        next_frame(cx);
        let item = cx
            .debug_bounds(item)
            .unwrap_or_else(|| panic!("{opening:?}: {name}'s menu opened"));
        (row, item)
    }

    /// [`six_files`] in a window tall enough that every row is drawn, with
    /// the app's keys bound as the app binds them (a test window binds none
    /// of them) and alpha selected: [Staged 2, alpha, beta, Unstaged 4,
    /// delta, epsilon, gamma, zeta].
    async fn six_files_for_menus(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, Entity<GitTurtle>, &mut VisualTestContext) {
        let (fixture, app, cx) = six_files(cx).await;
        cx.simulate_resize(size(px(1480.), px(1300.)));
        draw(cx);
        cx.update(|_, cx| {
            crate::shortcuts::bind_keys(cx);
            crate::native_accessibility::bind_keys(cx);
        });
        let alpha = cx.debug_bounds("working-row-1").expect("alpha's row");
        cx.simulate_click(alpha.center(), Modifiers::default());
        next_frame(cx);
        (fixture, app, cx)
    }

    /// Deletes `name` outside the app and lets the quiet refresh after it
    /// take its row away.
    fn delete_outside(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, name: &str) {
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let rows = app.read_with(cx, |app, _| app.working_rows.len());
        std::fs::remove_file(repository.join(name)).unwrap();
        quietly_refreshed(app, cx);
        cx.executor().run_until_parked();
        next_frame(cx);
        assert_eq!(
            app.read_with(cx, |app, _| app.working_rows.len()),
            rows - 1,
            "{name}'s row went"
        );
    }

    /// A working row's menu, opened as `opening` opens it, belongs to its
    /// file: a quiet refresh that takes an earlier file's row away, as one
    /// deleted outside the app, leaves it open on its own row, drawn beside
    /// the row where it now stands, and its Ignore… acts on its file. Kept
    /// by the row's place among the rows, a right-click's menu passed to the
    /// row after its own, and a keyboard's stayed where it opened, beside
    /// whichever row moved there.
    async fn a_row_menu_stays_on_its_row_as_an_earlier_row_goes(
        cx: &mut TestAppContext,
        opening: Opening,
    ) {
        let (_fixture, app, cx) = six_files_for_menus(cx).await;
        let list = app.read_with(cx, |app, _| app.file_focus.clone());
        let focused = |cx: &mut VisualTestContext| cx.update(|window, _| list.is_focused(window));
        // gamma's menu, then delta, before it, deleted outside the app.
        let (row, item) = open_row_menu(&app, cx, "gamma.txt", opening);
        assert!(!focused(cx), "{opening:?}: gamma's menu holds focus");
        delete_outside(&app, cx, "delta.txt");
        let selector: &'static str = format!(
            "working-row-{}",
            position_of(&app, cx, "gamma.txt", ChangeArea::Unstaged)
        )
        .leak();
        let moved = cx.debug_bounds(selector).expect(selector);
        assert!(
            (row.top() - moved.size.height - moved.top()).abs() < px(0.5),
            "{opening:?}: gamma's row {moved:?} took delta's place above {row:?}"
        );
        let drawn = cx
            .debug_bounds(IGNORE_ITEM)
            .unwrap_or_else(|| panic!("{opening:?}: gamma's menu stayed open"));
        assert!(!focused(cx), "{opening:?}: gamma's menu keeps focus");
        let (before, after) = (item.origin - row.origin, drawn.origin - moved.origin);
        assert!(
            (before.x - after.x).abs() < px(0.5) && (before.y - after.y).abs() < px(0.5),
            "{opening:?}: gamma's menu {drawn:?} beside its row {moved:?} as it opened, \
             {item:?} beside {row:?}"
        );
        // Its Ignore… acts on gamma.
        cx.simulate_click(drawn.center(), Modifiers::default());
        cx.executor().run_until_parked();
        draw(cx);
        let selected = crate::native_accessibility::written::get("ignore-selected-path")
            .and_then(|written| written.node.value().map(str::to_owned));
        assert_eq!(
            selected.as_deref(),
            Some("Selected untracked file: gamma.txt"),
            "{opening:?}: Ignore… in gamma's menu"
        );
    }

    /// A working row's menu, opened as `opening` opens it, closes with its
    /// row when a quiet refresh takes the row away, as after its file is
    /// deleted outside the app, and focus returns to the list, whose keys
    /// reach it again. Kept by the row's place among the rows, a
    /// right-click's menu passed to the row after its own, and a keyboard's
    /// stayed open where it opened, each still offering to act on the file
    /// that had gone.
    async fn a_row_menu_closes_with_its_row(cx: &mut TestAppContext, opening: Opening) {
        let (_fixture, app, cx) = six_files_for_menus(cx).await;
        let list = app.read_with(cx, |app, _| app.file_focus.clone());
        let focused = |cx: &mut VisualTestContext| cx.update(|window, _| list.is_focused(window));
        let beta = app.read_with(cx, |app, _| match app.actionable_working_rows()[2] {
            workspace::WorkingRow::File(index, area) => (index, area),
            _ => panic!("beta's row"),
        });
        // epsilon's menu, on the row before gamma's, then epsilon deleted
        // outside the app.
        open_row_menu(&app, cx, "epsilon.txt", opening);
        assert!(!focused(cx), "{opening:?}: epsilon's menu holds focus");
        delete_outside(&app, cx, "epsilon.txt");
        assert_eq!(
            offered(cx, "Ignore…"),
            None,
            "{opening:?}: epsilon's menu closed with its row"
        );
        assert!(
            app.read_with(cx, |app, _| app.file_menu.is_none()),
            "{opening:?}: no keyboard's menu is open"
        );
        assert!(focused(cx), "{opening:?}: focus is back on the list");
        cx.simulate_keystrokes("down");
        next_frame(cx);
        assert_eq!(
            app.read_with(cx, |app, _| app.working_selected),
            Some(beta),
            "{opening:?}: Down reached the list"
        );
    }

    #[gpui::test]
    async fn a_right_click_menu_stays_on_its_row_as_an_earlier_row_goes(cx: &mut TestAppContext) {
        a_row_menu_stays_on_its_row_as_an_earlier_row_goes(cx, Opening::RightClick).await;
    }

    #[gpui::test]
    async fn a_keyboard_menu_stays_on_its_row_as_an_earlier_row_goes(cx: &mut TestAppContext) {
        a_row_menu_stays_on_its_row_as_an_earlier_row_goes(cx, Opening::Keyboard).await;
    }

    #[gpui::test]
    async fn a_right_click_menu_closes_with_its_row(cx: &mut TestAppContext) {
        a_row_menu_closes_with_its_row(cx, Opening::RightClick).await;
    }

    #[gpui::test]
    async fn a_keyboard_menu_closes_with_its_row(cx: &mut TestAppContext) {
        a_row_menu_closes_with_its_row(cx, Opening::Keyboard).await;
    }

    /// A working row's menu, opened as `opening` opens it on an untracked
    /// file, is built again when the file is added and edited outside the
    /// app: its row stays, and its items act on the file as it now is,
    /// Ignore… unavailable for a tracked file and Discard changes… in place
    /// of Delete untracked file…. Built as it opened, it still offered to
    /// ignore the file and to delete it, which the delete then refused.
    async fn a_row_menu_follows_its_files_status(cx: &mut TestAppContext, opening: Opening) {
        let (_fixture, app, cx) = six_files_for_menus(cx).await;
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        open_row_menu(&app, cx, "gamma.txt", opening);
        assert_eq!(
            offered(cx, "Delete untracked file…"),
            Some(true),
            "{opening:?}"
        );
        fixture_git(&repository, &["add", "gamma.txt"]);
        std::fs::write(repository.join("gamma.txt"), "gamma\nedited\n").unwrap();
        quietly_refreshed(&app, cx);
        cx.executor().run_until_parked();
        next_frame(cx);
        assert_eq!(
            offered(cx, "Discard changes…"),
            Some(true),
            "{opening:?}: gamma's menu offers to discard its edit"
        );
        assert_eq!(offered(cx, "Delete untracked file…"), None, "{opening:?}");
        assert!(
            cx.debug_bounds("menu-item: Ignore…, untracked files only · MenuItem disabled")
                .is_some(),
            "{opening:?}: Ignore… is unavailable for a tracked file"
        );
    }

    #[gpui::test]
    async fn a_right_click_menu_follows_its_files_status(cx: &mut TestAppContext) {
        a_row_menu_follows_its_files_status(cx, Opening::RightClick).await;
    }

    #[gpui::test]
    async fn a_keyboard_menu_follows_its_files_status(cx: &mut TestAppContext) {
        a_row_menu_follows_its_files_status(cx, Opening::Keyboard).await;
    }

    /// A working row's menu, opened as `opening` opens it at 18 pt in a
    /// 461 px window, where the row has no room for its Stage control and
    /// the menu leads with Stage: after a quiet refresh takes an earlier
    /// row away, its Stage departs from gamma's row where it now stands.
    /// Holding the row's place from when it opened, it departed from zeta's.
    async fn a_row_menus_stage_departs_from_its_row(cx: &mut TestAppContext, opening: Opening) {
        let (_fixture, app, cx) = six_files_for_menus(cx).await;
        cx.update(|window, cx| crate::appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(1300.)));
        draw(cx);
        // gamma in view and selected, which the refresh keeps in view.
        let gamma = position_of(&app, cx, "gamma.txt", ChangeArea::Unstaged);
        app.update(cx, |app, _| {
            app.working_scroll
                .scroll_to_item(gamma, ScrollStrategy::Center)
        });
        next_frame(cx);
        let row = cx
            .debug_bounds(format!("working-row-{gamma}").leak())
            .expect("gamma's row in view");
        cx.simulate_click(row.center(), Modifiers::default());
        next_frame(cx);
        open_row_menu(&app, cx, "gamma.txt", opening);
        delete_outside(&app, cx, "delta.txt");
        let selector: &'static str = format!(
            "working-row-{}",
            position_of(&app, cx, "gamma.txt", ChangeArea::Unstaged)
        )
        .leak();
        let row = cx.debug_bounds(selector).expect(selector);
        let stage = cx
            .debug_bounds("menu-item: Stage · MenuItem")
            .unwrap_or_else(|| panic!("{opening:?}: the menu leads with Stage"));
        // The write held, so its rows cannot land and start the flight
        // before the departure is read: a quick one, on its own thread, once
        // did within the click's run until parked.
        let write = app.update(cx, |app, _| app.operations.hold());
        cx.simulate_click(stage.center(), Modifiers::default());
        let departed = app.read_with(cx, |app, _| match &app.working_travel {
            RowTravel::Departed(departure) => Some((departure.path.clone(), departure.top)),
            _ => None,
        });
        let Some((path, top)) = departed else {
            panic!("{opening:?}: gamma's row departed");
        };
        assert_eq!(path, Path::new("gamma.txt"), "{opening:?}");
        assert!(
            (top - row.top()).abs() < px(0.5),
            "{opening:?}: from gamma's row at {row:?}, not {top:?}"
        );
        release(&app, write, cx).await;
        crate::views::tests::settle(&app, cx).await;
        cx.update(|window, cx| crate::appearance::apply_text_sizes(13, 12, window, cx));
    }

    #[gpui::test]
    async fn a_right_click_menus_stage_departs_from_its_row(cx: &mut TestAppContext) {
        a_row_menus_stage_departs_from_its_row(cx, Opening::RightClick).await;
    }

    #[gpui::test]
    async fn a_keyboard_menus_stage_departs_from_its_row(cx: &mut TestAppContext) {
        a_row_menus_stage_departs_from_its_row(cx, Opening::Keyboard).await;
    }

    /// A working row's menu, opened as `opening` opens it, closes when its
    /// row leaves the part of the list drawn, as when the list scrolls past
    /// it, and focus returns to the list. A right-click's menu went with its
    /// row, no longer drawn, while a keyboard's stayed, pinned to the
    /// list's edge. (A refresh keeps the cursor row, a keyboard menu's row,
    /// in view, so files arriving above it scroll the list rather than
    /// push the row out.)
    async fn a_row_menu_closes_as_its_row_leaves_the_list(
        cx: &mut TestAppContext,
        opening: Opening,
    ) {
        let (_fixture, app, cx) = six_files(cx).await;
        cx.update(|_, cx| {
            crate::shortcuts::bind_keys(cx);
            crate::native_accessibility::bind_keys(cx);
        });
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let list = app.read_with(cx, |app, _| app.file_focus.clone());
        let focused = |cx: &mut VisualTestContext| cx.update(|window, _| list.is_focused(window));
        // Eight more files after gamma, so that the list scrolls.
        for n in 1..=8 {
            std::fs::write(repository.join(format!("m{n}.txt")), "more\n").unwrap();
        }
        quietly_refreshed(&app, cx);
        open_row_menu(&app, cx, "delta.txt", opening);
        let last = app.read_with(cx, |app, _| app.working_rows.len() - 1);
        app.update(cx, |app, _| {
            app.working_scroll.scroll_to_item(last, ScrollStrategy::Top)
        });
        next_frame(cx);
        cx.executor().run_until_parked();
        next_frame(cx);
        let delta: &'static str = format!(
            "working-row-{}",
            position_of(&app, cx, "delta.txt", ChangeArea::Unstaged)
        )
        .leak();
        assert!(
            cx.debug_bounds(delta).is_none(),
            "{opening:?}: delta's row left the part of the list drawn"
        );
        assert_eq!(
            offered(cx, "Ignore…"),
            None,
            "{opening:?}: delta's menu closed"
        );
        assert!(
            app.read_with(cx, |app, _| app.file_menu.is_none()),
            "{opening:?}: no keyboard's menu is open"
        );
        assert!(focused(cx), "{opening:?}: focus is back on the list");
    }

    #[gpui::test]
    async fn a_right_click_menu_closes_as_its_row_leaves_the_list(cx: &mut TestAppContext) {
        a_row_menu_closes_as_its_row_leaves_the_list(cx, Opening::RightClick).await;
    }

    #[gpui::test]
    async fn a_keyboard_menu_closes_as_its_row_leaves_the_list(cx: &mut TestAppContext) {
        a_row_menu_closes_as_its_row_leaves_the_list(cx, Opening::Keyboard).await;
    }

    /// A keyboard's menu on `sub/`, an untracked repository beside `sub`, a
    /// tracked file whose deletion is staged, stays `sub/`'s through a quiet
    /// refresh that moves both rows, which `Path` holds equal. Git lists
    /// the repository only once `sub` has left the index, so the two rows
    /// are in different groups; this keeps them apart through a refresh,
    /// and `same_path_tells_sub_from_sub_slash` the comparison itself.
    #[gpui::test]
    async fn a_keyboard_menu_tells_sub_from_sub_slash(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = six_files_for_menus(cx).await;
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        std::fs::write(repository.join("sub"), "sub\n").unwrap();
        fixture_git(&repository, &["add", "sub"]);
        fixture_git(
            &repository,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "sub",
                "--",
                "sub",
            ],
        );
        fixture_git(&repository, &["rm", "--quiet", "sub"]);
        fixture_git(&repository, &["init", "--quiet", "sub"]);
        std::fs::write(repository.join("sub/inner.txt"), "inner\n").unwrap();
        quietly_refreshed(&app, cx);
        let (sub, slash) = (
            position_of(&app, cx, "sub", ChangeArea::Staged),
            position_of(&app, cx, "sub/", ChangeArea::Unstaged),
        );
        assert!(
            sub < slash,
            "sub in Staged, row {sub}, above sub/ in Unstaged, row {slash}"
        );
        let (row, item) = open_row_menu(&app, cx, "sub/", Opening::Keyboard);
        assert_eq!(offered(cx, "Delete untracked file…"), Some(true));
        delete_outside(&app, cx, "delta.txt");
        let selector: &'static str = format!(
            "working-row-{}",
            position_of(&app, cx, "sub/", ChangeArea::Unstaged)
        )
        .leak();
        let moved = cx.debug_bounds(selector).expect(selector);
        assert_eq!(
            offered(cx, "Delete untracked file…"),
            Some(true),
            "sub/'s menu, not sub's"
        );
        assert_eq!(
            offered(cx, "Discard changes…"),
            None,
            "sub/'s menu, not sub's"
        );
        let drawn = cx.debug_bounds(IGNORE_ITEM).expect("sub/'s menu");
        let (before, after) = (item.origin - row.origin, drawn.origin - moved.origin);
        assert!(
            (before.y - after.y).abs() < px(0.5),
            "beside sub/'s row {moved:?}: {drawn:?}"
        );
    }

    /// Shift-F10 on a cursor row that a wheel scrolled out of the list opens
    /// its menu beside the row, scrolled back into view the least it needs.
    /// It opened pinned to the list's edge, beside no row.
    #[gpui::test]
    async fn a_keyboard_menu_opens_beside_its_row_in_view(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = six_files(cx).await;
        cx.update(|_, cx| {
            crate::shortcuts::bind_keys(cx);
            crate::native_accessibility::bind_keys(cx);
        });
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        for n in 1..=8 {
            std::fs::write(repository.join(format!("m{n}.txt")), "more\n").unwrap();
        }
        quietly_refreshed(&app, cx);
        let delta = position_of(&app, cx, "delta.txt", ChangeArea::Unstaged);
        let selector: &'static str = format!("working-row-{delta}").leak();
        let row = cx.debug_bounds(selector).expect(selector);
        cx.simulate_click(row.center(), Modifiers::default());
        let list = app.read_with(cx, |app, _| app.file_focus.clone());
        cx.update(|window, cx| list.focus(window, cx));
        let last = app.read_with(cx, |app, _| app.working_rows.len() - 1);
        app.update(cx, |app, _| {
            app.working_scroll.scroll_to_item(last, ScrollStrategy::Top)
        });
        next_frame(cx);
        assert!(cx.debug_bounds(selector).is_none(), "delta scrolled away");
        cx.simulate_keystrokes("shift-f10");
        cx.executor().run_until_parked();
        next_frame(cx);
        next_frame(cx);
        let row = cx.debug_bounds(selector).expect("delta's row in view");
        let item = cx.debug_bounds(IGNORE_ITEM).expect("delta's menu open");
        assert!(
            item.top() >= row.bottom() && item.top() < row.bottom() + row.size.height,
            "delta's menu {item:?} below its row {row:?}"
        );
    }

    /// A working row's menu, opened as `opening` opens it, built again for
    /// its file's new status, as after gamma is added and edited outside the
    /// app, is built from that status when a write starts and ends too: a
    /// right-click's write rebuild used the builder the menu opened with,
    /// and a keyboard's the file as the menu opened, which both brought back
    /// Delete untracked file… for a tracked file.
    async fn a_rebuilt_row_menu_follows_writes_with_its_new_items(
        cx: &mut TestAppContext,
        opening: Opening,
    ) {
        let (_fixture, app, cx) = six_files_for_menus(cx).await;
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        open_row_menu(&app, cx, "gamma.txt", opening);
        fixture_git(&repository, &["add", "gamma.txt"]);
        std::fs::write(repository.join("gamma.txt"), "gamma\nedited\n").unwrap();
        quietly_refreshed(&app, cx);
        assert_eq!(
            offered(cx, "Discard changes…"),
            Some(true),
            "{opening:?}: rebuilt for gamma"
        );
        let write = held(
            &app,
            cx,
            gitturtle_core::WriteCommand::Stage {
                paths: vec![PathBuf::from("zeta.txt")],
            },
        );
        next_frame(cx);
        assert_eq!(
            offered(cx, "Discard changes…"),
            Some(false),
            "{opening:?}: unavailable while the write runs"
        );
        assert_eq!(
            offered(cx, "Delete untracked file…"),
            None,
            "{opening:?}: while the write runs"
        );
        release(&app, write, cx).await;
        next_frame(cx);
        assert_eq!(
            offered(cx, "Discard changes…"),
            Some(true),
            "{opening:?}: available once the write ends"
        );
        assert_eq!(
            offered(cx, "Delete untracked file…"),
            None,
            "{opening:?}: once the write ends"
        );
    }

    #[gpui::test]
    async fn a_rebuilt_right_click_menu_follows_writes_with_its_new_items(cx: &mut TestAppContext) {
        a_rebuilt_row_menu_follows_writes_with_its_new_items(cx, Opening::RightClick).await;
    }

    #[gpui::test]
    async fn a_rebuilt_keyboard_menu_follows_writes_with_its_new_items(cx: &mut TestAppContext) {
        a_rebuilt_row_menu_follows_writes_with_its_new_items(cx, Opening::Keyboard).await;
    }

    /// The items of gamma's menu, by the names each is drawn and announced
    /// with: available, refused, or refused with the condition it waits for.
    const GAMMA_ITEMS: [&str; 4] = [
        "Ignore…",
        "Ignore…, untracked files only",
        "Delete untracked file…",
        "Discard changes…",
    ];

    use crate::native_accessibility::menu_tests::cursor_among;

    /// A working row's menu, opened as `opening` opens it, its keyboard's
    /// cursor moved down to Delete untracked file…, keeps the cursor on that
    /// item as a Stage of another file starts and ends: while the write
    /// refuses every item the cursor rests on none, and once the write's
    /// rows land it is back on Delete untracked file…. Moved up to Ignore…,
    /// which gamma added and edited outside the app can no longer take, it
    /// rests on none, and it is back on Ignore… once gamma is untracked
    /// again. Each build once sent a right-click's cursor away and a
    /// keyboard's back to Ignore…, so Enter just after the write chose
    /// another item than the one the keys had reached; later the refused
    /// Ignore… sent it to Discard changes…, where Enter offered to discard
    /// gamma's changes.
    async fn a_row_menu_keeps_its_cursor_across_a_write(cx: &mut TestAppContext, opening: Opening) {
        let (_fixture, app, cx) = six_files_for_menus(cx).await;
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        open_row_menu(&app, cx, "gamma.txt", opening);
        // A right-click's menu opens with no cursor, a keyboard's with it
        // on Ignore….
        cx.simulate_keystrokes(match opening {
            Opening::RightClick => "down down",
            Opening::Keyboard => "down",
        });
        next_frame(cx);
        assert_eq!(
            cursor_among(cx, &GAMMA_ITEMS),
            ["Delete untracked file…"],
            "{opening:?}: the keys moved the cursor down"
        );
        let write = held(
            &app,
            cx,
            gitturtle_core::WriteCommand::Stage {
                paths: vec![PathBuf::from("delta.txt")],
            },
        );
        next_frame(cx);
        assert_eq!(
            offered(cx, "Delete untracked file…"),
            Some(false),
            "{opening:?}: refused while the write runs"
        );
        assert_eq!(
            cursor_among(cx, &GAMMA_ITEMS),
            Vec::<&str>::new(),
            "{opening:?}: no item holds the cursor while every item is refused"
        );
        release(&app, write, cx).await;
        next_frame(cx);
        assert_eq!(
            offered(cx, "Delete untracked file…"),
            Some(true),
            "{opening:?}: available once the write ends"
        );
        assert_eq!(
            cursor_among(cx, &GAMMA_ITEMS),
            ["Delete untracked file…"],
            "{opening:?}: the cursor is back on its item once the write ends"
        );

        cx.simulate_keystrokes("up");
        next_frame(cx);
        assert_eq!(
            cursor_among(cx, &GAMMA_ITEMS),
            ["Ignore…"],
            "{opening:?}: Up moved the cursor to Ignore…"
        );
        fixture_git(&repository, &["add", "gamma.txt"]);
        std::fs::write(repository.join("gamma.txt"), "gamma\nedited\n").unwrap();
        quietly_refreshed(&app, cx);
        assert_eq!(
            offered(cx, "Discard changes…"),
            Some(true),
            "{opening:?}: rebuilt for gamma"
        );
        assert_eq!(
            cursor_among(cx, &GAMMA_ITEMS),
            Vec::<&str>::new(),
            "{opening:?}: Ignore… refused, the cursor rests on none"
        );
        fixture_git(&repository, &["reset", "-q", "--", "gamma.txt"]);
        quietly_refreshed(&app, cx);
        assert_eq!(
            offered(cx, "Delete untracked file…"),
            Some(true),
            "{opening:?}: rebuilt for gamma untracked again"
        );
        assert_eq!(
            cursor_among(cx, &GAMMA_ITEMS),
            ["Ignore…"],
            "{opening:?}: back on Ignore… once it can act again"
        );
    }

    #[gpui::test]
    async fn a_right_click_row_menu_keeps_its_cursor_across_a_write(cx: &mut TestAppContext) {
        a_row_menu_keeps_its_cursor_across_a_write(cx, Opening::RightClick).await;
    }

    #[gpui::test]
    async fn a_keyboard_row_menu_keeps_its_cursor_across_a_write(cx: &mut TestAppContext) {
        a_row_menu_keeps_its_cursor_across_a_write(cx, Opening::Keyboard).await;
    }

    /// The items of notes.txt's menu in [`narrow_notes`]: available,
    /// refused, or refused with the condition it waits for.
    const NOTES_ITEMS: [&str; 4] = [
        "Stage",
        "Ignore…, untracked files only",
        "Discard changes…",
        "Discard changes…, after conflict resolution",
    ];

    /// notes.txt's menu's Stage, drawn available.
    const NOTES_STAGE: &str = "menu-item: Stage · MenuItem";

    /// Git with `arguments` in `repository` under a fixture identity.
    fn committing_git(repository: &Path, arguments: &[&str]) -> std::process::Output {
        crate::views::tests::fixture_git_output(
            repository,
            &[
                &[
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                ][..],
                arguments,
            ]
            .concat(),
        )
    }

    /// Whether notes.txt in `repository` is unmerged.
    fn notes_unmerged(repository: &Path) -> bool {
        let unmerged = committing_git(repository, &["ls-files", "-u", "--", "notes.txt"]);
        unmerged.status.success() && !unmerged.stdout.is_empty()
    }

    /// Whether `app` runs or holds a write.
    fn writing(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> bool {
        app.read_with(cx, |app, _| {
            app.operation_busy.is_some() || app.operation_task.is_some()
        })
    }

    /// Working Changes at 18 pt in a 461 px window, where a row has no room
    /// for its Stage control and its menu leads with Stage, with the app's
    /// keys bound: notes.txt changed on a side branch and on main, then
    /// edited. Returns the repository with the window. The window is active,
    /// as the app's is while someone works in it, so focus that leaves an
    /// open menu closes it; a test window starts inactive, where a keyboard's
    /// menu that a conflicted file's resolution draft took focus from stayed
    /// open, unlike the app's.
    async fn narrow_notes(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        &mut VisualTestContext,
        PathBuf,
    ) {
        let (fixture, app, cx) = history_window_from(cx, history_script(40, 3)).await;
        let repository = fixture.path().join("narrow");
        fixture_git(&repository, &["reset", "--hard", "--quiet"]);
        fixture_git(&repository, &["checkout", "-q", "-b", "side"]);
        std::fs::write(repository.join("notes.txt"), "side\n").unwrap();
        assert!(
            committing_git(&repository, &["commit", "-q", "-am", "side"])
                .status
                .success()
        );
        fixture_git(&repository, &["checkout", "-q", "main"]);
        std::fs::write(repository.join("notes.txt"), "main\n").unwrap();
        assert!(
            committing_git(&repository, &["commit", "-q", "-am", "main"])
                .status
                .success()
        );
        std::fs::write(repository.join("notes.txt"), "edited\n").unwrap();
        cx.update(|window, cx| {
            window.activate_window();
            crate::appearance::apply_text_sizes(18, 12, window, cx);
        });
        cx.simulate_resize(size(px(461.), px(1300.)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        rows_arrived(&app, cx).await;
        draw(cx);
        cx.update(|_, cx| {
            crate::shortcuts::bind_keys(cx);
            crate::native_accessibility::bind_keys(cx);
        });
        (fixture, app, cx, repository)
    }

    /// A working row's menu, opened as `opening` opens it in
    /// [`narrow_notes`], its cursor moved to Discard changes…, as a merge
    /// outside the app leaves the file conflicted and the item can no
    /// longer act. A right-click's menu is built again with the cursor
    /// resting on none, and Enter runs nothing: the file stays conflicted.
    /// Moved to the nearest item that could act, the cursor fell to Stage,
    /// and Enter ran `git add` on the conflicted file at once, marking it
    /// resolved. A keyboard's opening selected the row, so the conflicted
    /// file's preview, landing with the status, focuses its resolution
    /// draft, and the menu, its focus gone, closes, leaving focus in the
    /// draft and the file conflicted.
    async fn a_row_menus_cursor_rests_as_its_file_becomes_conflicted(
        cx: &mut TestAppContext,
        opening: Opening,
    ) {
        let (_fixture, app, cx, repository) = narrow_notes(cx).await;
        open_row_menu_showing(&app, cx, "notes.txt", opening, NOTES_STAGE);
        // A right-click's menu opens with no cursor, a keyboard's with it on
        // Stage; Down passes over the refused Ignore….
        cx.simulate_keystrokes(match opening {
            Opening::RightClick => "down down",
            Opening::Keyboard => "down",
        });
        next_frame(cx);
        assert_eq!(
            cursor_among(cx, &NOTES_ITEMS),
            ["Discard changes…"],
            "{opening:?}: the keys moved the cursor to Discard changes…"
        );

        fixture_git(&repository, &["checkout", "-q", "--", "notes.txt"]);
        assert!(
            !committing_git(&repository, &["merge", "-q", "side"])
                .status
                .success(),
            "the merge stops on notes.txt"
        );
        quietly_refreshed(&app, cx);
        match opening {
            Opening::RightClick => {
                assert_eq!(
                    offered(cx, "Discard changes…, after conflict resolution"),
                    Some(false),
                    "rebuilt for the conflicted file"
                );
                assert_eq!(offered(cx, "Stage"), Some(true), "Stage offered");
                assert_eq!(
                    cursor_among(cx, &NOTES_ITEMS),
                    Vec::<&str>::new(),
                    "the cursor rests on none"
                );
                cx.simulate_keystrokes("enter");
                cx.executor().run_until_parked();
                next_frame(cx);
                assert_eq!(offered(cx, "Stage"), Some(true), "the menu stays open");
            }
            Opening::Keyboard => {
                cx.executor().run_until_parked();
                next_frame(cx);
                assert!(
                    app.read_with(cx, |app, _| app.file_menu.is_none()),
                    "the keyboard's menu closed"
                );
                assert_eq!(offered(cx, "Stage"), None, "no menu drawn");
                assert!(
                    cx.update(|window, cx| {
                        app.read(cx)
                            .conflict_view
                            .as_ref()
                            .is_some_and(|view| view.read(cx).resolution_focused(window, cx))
                    }),
                    "focus in the conflicted file's resolution draft"
                );
            }
        }
        assert!(!writing(&app, cx), "{opening:?}: no write started");
        assert!(
            notes_unmerged(&repository),
            "{opening:?}: notes.txt is still conflicted"
        );
        cx.update(|window, cx| crate::appearance::apply_text_sizes(13, 12, window, cx));
    }

    #[gpui::test]
    async fn a_right_click_menus_cursor_rests_as_its_file_becomes_conflicted(
        cx: &mut TestAppContext,
    ) {
        a_row_menus_cursor_rests_as_its_file_becomes_conflicted(cx, Opening::RightClick).await;
    }

    #[gpui::test]
    async fn a_keyboard_menu_closes_as_its_file_becomes_conflicted(cx: &mut TestAppContext) {
        a_row_menus_cursor_rests_as_its_file_becomes_conflicted(cx, Opening::Keyboard).await;
    }

    /// A working row's keyboard menu closes, focus on the list, as soon as a
    /// status read moves the selection off its row, as one that finds gamma
    /// staged outside the app moves it to gamma's Staged row; a read that
    /// keeps the selection on its row leaves focus in the menu. The rows the
    /// read brings are held here, so the menu's own row, which they take
    /// away, cannot close it first: kept focused, it stayed over a row the
    /// selection had left until they landed.
    #[gpui::test]
    async fn a_keyboard_menu_closes_as_the_selection_leaves_its_row(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = six_files_for_menus(cx).await;
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let list = app.read_with(cx, |app, _| app.file_focus.clone());
        let menu_focused = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                app.read(cx)
                    .file_menu
                    .as_ref()
                    .is_some_and(|menu| menu.has_focus(window, cx))
            })
        };
        let gamma = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                let index = app.working_selected.expect("a selection").0;
                app.work_status.as_ref().expect("a status").entries[index]
                    .path
                    .clone()
            })
        };
        open_row_menu(&app, cx, "gamma.txt", Opening::Keyboard);
        let reread = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)))
        };
        // A read that leaves gamma unstaged selects its row again.
        reread(cx);
        crate::views::tests::settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        assert!(
            menu_focused(cx),
            "the read kept the selection on the menu's row"
        );
        let filter = app.update(cx, |app, _| app.file_paths.executor().hold());
        fixture_git(&repository, &["add", "gamma.txt"]);
        reread(cx);
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(
            app.read_with(cx, |app, _| app.working_selected.map(|(_, area)| area)),
            Some(ChangeArea::Staged),
            "the selection moved to gamma's Staged row"
        );
        assert_eq!(gamma(cx), PathBuf::from("gamma.txt"));
        assert!(
            app.read_with(cx, |app, _| app.file_menu.is_none()),
            "the menu closed as the selection left its row"
        );
        assert!(
            cx.update(|window, _| list.is_focused(window)),
            "focus is on the list"
        );
        drop(filter);
        crate::path_filter::filters_land(&app, cx);
    }

    /// Holds quiet reads from starting, or lets them start and land.
    fn hold_quiet_reads(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, held: bool) {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.automatic.hold_quiet_reads(held);
                app.try_automatic_refresh(window, cx);
            })
        });
        if !held {
            for _ in 0..8 {
                if !app.read_with(cx, |app, _| app.automatic.quiet_read_requested()) {
                    break;
                }
                app.read_with(cx, |app, _| app.worker.wait_idle());
                cx.executor().run_until_parked();
            }
            crate::path_filter::filters_land(app, cx);
        }
        next_frame(cx);
    }

    /// Asks for a quiet read as the local watcher does when a file is saved
    /// outside the app, through the repository's subscription, or as the
    /// status bar's Refresh does with `refresh`, and notifies, as any other
    /// change in the window would, which a keyboard's menu follows.
    fn quiet_read_asked(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, refresh: bool) {
        if refresh {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.retry_automatic_refresh(window, cx))
            });
        } else {
            // The subscription is made on the operation thread.
            app.read_with(cx, |app, _| app.operations.drain());
            cx.executor().run_until_parked();
            app.update(cx, |app, _| {
                app.automatic
                    .report_local_change(crate::local_refresh::LocalChange {
                        worktree: true,
                        ..Default::default()
                    })
            });
            cx.executor().run_until_parked();
        }
        assert!(
            app.read_with(cx, |app, _| app.automatic.quiet_read_requested()),
            "a quiet read was asked for"
        );
        app.update(cx, |_, cx| cx.notify());
        next_frame(cx);
    }

    /// A working row's menu, opened as `opening` opens it, stays refused
    /// once a write that reloads the repository quietly ends, as Create
    /// branch does, until that reload lands, held here: built again as the
    /// write ended, its items acted on what the window showed before the
    /// write, as a commit's Revert offered a commit an Undo had just taken
    /// away. A quiet read the watcher asks for, as a file saved outside the
    /// app brings, or the status bar's Refresh asks for refuses nothing.
    async fn a_row_menu_waits_for_its_writes_reload(cx: &mut TestAppContext, opening: Opening) {
        let (_fixture, app, cx) = six_files_for_menus(cx).await;
        open_row_menu(&app, cx, "gamma.txt", opening);
        let delete = |cx: &mut VisualTestContext| offered(cx, "Delete untracked file…");
        assert_eq!(delete(cx), Some(true), "{opening:?}: at rest");
        hold_quiet_reads(&app, cx, true);
        let write = held(
            &app,
            cx,
            gitturtle_core::WriteCommand::CreateBranch {
                name: "side-q9".into(),
                start_point: None,
            },
        );
        assert_eq!(delete(cx), Some(false), "{opening:?}: while the write runs");
        release(&app, write, cx).await;
        next_frame(cx);
        assert!(
            app.read_with(cx, |app, _| app.operation_busy.is_none()),
            "the write ended"
        );
        assert_eq!(
            delete(cx),
            Some(false),
            "{opening:?}: refused until the write's reload lands"
        );
        hold_quiet_reads(&app, cx, false);
        assert_eq!(
            delete(cx),
            Some(true),
            "{opening:?}: available once it lands"
        );
        for (source, refresh) in [("the watcher", false), ("Refresh", true)] {
            hold_quiet_reads(&app, cx, true);
            quiet_read_asked(&app, cx, refresh);
            assert_eq!(
                delete(cx),
                Some(true),
                "{opening:?}: a quiet read {source} asks for refuses nothing"
            );
            hold_quiet_reads(&app, cx, false);
            assert_eq!(delete(cx), Some(true), "{opening:?}: after {source}'s read");
        }
    }

    #[gpui::test]
    async fn a_right_click_menu_waits_for_its_writes_reload(cx: &mut TestAppContext) {
        a_row_menu_waits_for_its_writes_reload(cx, Opening::RightClick).await;
    }

    #[gpui::test]
    async fn a_keyboard_menu_waits_for_its_writes_reload(cx: &mut TestAppContext) {
        a_row_menu_waits_for_its_writes_reload(cx, Opening::Keyboard).await;
    }

    /// A working row's menu, opened as `opening` opens it, stays refused
    /// once a Stage of its file ends, until the status read the Stage asks
    /// for as it ends lands, held here, as a write's quiet reload is waited
    /// for (`a_row_menu_waits_for_its_writes_reload`). Built again as the
    /// Stage ended, from the status the window still showed, it offered
    /// Delete untracked file… for the file the Stage had just added, until
    /// the read landed and built it once more.
    async fn a_row_menu_waits_for_its_writes_status_read(
        cx: &mut TestAppContext,
        opening: Opening,
    ) {
        let (_fixture, app, cx) = six_files_for_menus(cx).await;
        open_row_menu(&app, cx, "gamma.txt", opening);
        let delete = |cx: &mut VisualTestContext| offered(cx, "Delete untracked file…");
        assert_eq!(delete(cx), Some(true), "{opening:?}: at rest");
        let write = held(
            &app,
            cx,
            gitturtle_core::WriteCommand::Stage {
                paths: vec![PathBuf::from("gamma.txt")],
            },
        );
        assert_eq!(delete(cx), Some(false), "{opening:?}: while the Stage runs");
        // The status read the Stage asks for as it ends waits behind this.
        let status = app.update(cx, |app, _| app.operations.hold());
        drop(write);
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the write");
        write.await;
        cx.executor().run_until_parked();
        next_frame(cx);
        assert!(
            app.read_with(cx, |app, _| app.operation_busy.is_none()
                && app.status_task.is_some()),
            "{opening:?}: the Stage ended, and its status read is held"
        );
        assert_eq!(
            delete(cx),
            Some(false),
            "{opening:?}: refused until the Stage's status read lands"
        );
        drop(status);
        crate::views::tests::settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        next_frame(cx);
        assert_eq!(
            app.read_with(cx, |app, _| app.work_status.as_ref().and_then(|status| {
                status
                    .entries
                    .iter()
                    .find(|entry| entry.path == std::path::Path::new("gamma.txt"))
                    .map(|entry| entry.untracked)
            })),
            Some(false),
            "{opening:?}: gamma is staged"
        );
        assert_eq!(
            delete(cx),
            None,
            "{opening:?}: nothing offers to delete gamma once the read lands"
        );
    }

    #[gpui::test]
    async fn a_right_click_menu_waits_for_its_writes_status_read(cx: &mut TestAppContext) {
        a_row_menu_waits_for_its_writes_status_read(cx, Opening::RightClick).await;
    }

    #[gpui::test]
    async fn a_keyboard_menu_waits_for_its_writes_status_read(cx: &mut TestAppContext) {
        a_row_menu_waits_for_its_writes_status_read(cx, Opening::Keyboard).await;
    }

    /// A working row's keyboard menu stays refused once a Stage ends and
    /// Refresh, pressed while the status read the Stage asked for runs,
    /// reads the status again in its place, until that new read lands, both
    /// held here, and offers its items once it has. Replaced unmarked, the
    /// Stage's read was no longer counted, and the menu was built again from
    /// the status the window still showed, offering its items, before
    /// either read had landed.
    #[gpui::test]
    async fn a_keyboard_menu_waits_for_a_refresh_in_place_of_its_writes_status_read(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, cx) = six_files_for_menus(cx).await;
        open_row_menu(&app, cx, "gamma.txt", Opening::Keyboard);
        let delete = |cx: &mut VisualTestContext| offered(cx, "Delete untracked file…");
        assert_eq!(delete(cx), Some(true), "at rest");
        let write = held(
            &app,
            cx,
            gitturtle_core::WriteCommand::Stage {
                paths: vec![PathBuf::from("zeta.txt")],
            },
        );
        assert_eq!(delete(cx), Some(false), "while the Stage runs");
        // The status read the Stage asks for as it ends, and the one Refresh
        // asks for in its place, wait behind this.
        let status = app.update(cx, |app, _| app.operations.hold());
        drop(write);
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the write");
        write.await;
        cx.executor().run_until_parked();
        next_frame(cx);
        let asked = app.read_with(cx, |app, _| {
            assert!(
                app.operation_busy.is_none() && app.status_task.is_some(),
                "the Stage ended, and its status read is held"
            );
            app.work_generation
        });
        assert_eq!(
            delete(cx),
            Some(false),
            "refused while the Stage's status read runs"
        );
        // Refresh's shortcut, pressed in the menu, reaches the window.
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-r"
        } else {
            "ctrl-r"
        });
        cx.executor().run_until_parked();
        next_frame(cx);
        assert!(
            app.read_with(cx, |app, _| app.work_generation != asked
                && app.status_task.is_some()),
            "Refresh read the status again in place of the Stage's read, held too"
        );
        assert_eq!(
            delete(cx),
            Some(false),
            "refused until the read that replaced the Stage's lands"
        );
        assert!(focus_in_keyed_menu(&app, cx), "focus stays in the menu");
        drop(status);
        crate::views::tests::settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        next_frame(cx);
        assert!(
            app.read_with(cx, |app, _| app.status_task.is_none()
                && !app.write_unsettled()),
            "the read that replaced the Stage's landed"
        );
        assert_eq!(delete(cx), Some(true), "available once it lands");
    }

    /// Reads the working state again quietly, as the local watcher does
    /// after a change outside the app, and lands the read but not the rows
    /// it brings, which a hold on the filter's thread keeps preparing.
    fn quietly_read_while_rows_held(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.queue_automatic_refresh(
                    crate::local_refresh::LocalChange {
                        worktree: true,
                        ..Default::default()
                    },
                    window,
                    cx,
                )
            })
        });
        for _ in 0..8 {
            if !app.read_with(cx, |app, _| app.automatic.quiet_read_requested()) {
                break;
            }
            app.read_with(cx, |app, _| app.worker.wait_idle());
            cx.executor().run_until_parked();
        }
        next_frame(cx);
        assert!(
            app.read_with(cx, |app, _| app.working_paths.pending),
            "the read landed and its rows are being prepared"
        );
    }

    /// A working row's right-click menu is built again once for each change
    /// it follows, by the window or by its row: once as a write starts,
    /// once as it ends, once for the status read the write asks for, as the
    /// read's rows land rather than as the read does, and once as its
    /// file's status changes outside the app. Waiting for the filter's rows
    /// through a write, it is built again once for each change of the
    /// reason its refused items state: as the write starts, as it looks
    /// busy, as it ends, and as a newer status lands and its rows become
    /// why, with no write starting or ending; not as the write's reload
    /// lands, and once as the rows do. Built again from the file and the
    /// write's state together, by its row, and for the write as well, by
    /// the window, it was built twice as each write started, and twice more
    /// as the read landed, before its rows.
    #[gpui::test]
    async fn a_right_click_working_menu_is_built_once_per_change(cx: &mut TestAppContext) {
        use crate::working_selection::Preparation;
        let (_fixture, app, cx) = six_files_for_menus(cx).await;
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        open_row_menu(&app, cx, "gamma.txt", Opening::RightClick);
        let menu = cx
            .update(|_, cx| crate::native_accessibility::open_pointer_menus(cx))
            .pop()
            .expect("gamma's menu")
            .entity_id();
        // How often the menu has been built again since the last count, as
        // the window or its row builds it.
        let mut counted = crate::native_accessibility::pointer_menu_builds(menu);
        let mut built = move || {
            let builds = crate::native_accessibility::pointer_menu_builds(menu);
            builds - std::mem::replace(&mut counted, builds)
        };
        let delete = |cx: &mut VisualTestContext| offered(cx, "Delete untracked file…");
        let write = held(
            &app,
            cx,
            gitturtle_core::WriteCommand::Stage {
                paths: vec![PathBuf::from("zeta.txt")],
            },
        );
        next_frame(cx);
        assert_eq!(built(), 1, "built once as the write starts");
        assert_eq!(delete(cx), Some(false), "while the write runs");
        // The status read the Stage asks for as it ends waits behind this.
        let status = app.update(cx, |app, _| app.operations.hold());
        drop(write);
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the write");
        write.await;
        cx.executor().run_until_parked();
        next_frame(cx);
        assert!(
            app.read_with(cx, |app, _| app.operation_busy.is_none()
                && app.status_task.is_some()),
            "the write ended, and its status read is held"
        );
        assert_eq!(built(), 1, "built once as the write ends");
        // The read lands; the rows it brings wait behind this. It is
        // answered and handled where the window handles it, not taken and
        // awaited, which would leave no read in flight before it landed.
        let rows = app.update(cx, |app, _| app.file_paths.executor().hold());
        drop(status);
        app.read_with(cx, |app, _| app.operations.drain());
        cx.executor().run_until_parked();
        next_frame(cx);
        assert!(
            app.read_with(cx, |app, _| !app.write_unsettled()
                && app.working_paths.pending),
            "the read landed, and its rows are held"
        );
        assert_eq!(built(), 0, "not built while its rows are prepared");
        drop(rows);
        crate::path_filter::filters_land(&app, cx);
        next_frame(cx);
        assert_eq!(built(), 1, "built once as its rows land");
        assert_eq!(delete(cx), Some(true), "once the rows land");
        fixture_git(&repository, &["add", "gamma.txt"]);
        std::fs::write(repository.join("gamma.txt"), "gamma\nedited\n").unwrap();
        quietly_refreshed(&app, cx);
        next_frame(cx);
        assert_eq!(built(), 1, "built once as gamma's status changes");
        assert_eq!(
            offered(cx, "Discard changes…"),
            Some(true),
            "for gamma's edit"
        );
        // The filter's rows wait behind this; gamma matches its query.
        let waiting = |cx: &mut VisualTestContext, reason: &str| {
            offered(cx, &format!("Discard changes…, {reason}"))
        };
        let rows = app.update(cx, |app, _| app.file_paths.executor().hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.working_filter
                    .update(cx, |input, cx| input.set_value("a", window, cx));
                app.filter_working_paths(window, cx);
            })
        });
        next_frame(cx);
        assert_eq!(built(), 0, "not built as the filter's rows are prepared");
        let write = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.write(
                    gitturtle_core::WriteCommand::CreateBranch {
                        name: "side-q9".into(),
                        start_point: None,
                    },
                    "Creating branch…",
                    window,
                    cx,
                )
            })
        });
        next_frame(cx);
        assert!(
            app.read_with(cx, |app, _| app.operation_busy.is_some()
                && app.operation_shown().is_none()),
            "the write runs, and does not look busy yet"
        );
        assert_eq!(built(), 1, "built once as the write starts");
        assert_eq!(waiting(cx, "filtering…"), Some(false), "for the filter");
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        cx.executor().run_until_parked();
        next_frame(cx);
        assert!(
            app.read_with(cx, |app, _| app.operation_shown().is_some()
                && app.working_paths.pending),
            "the write looks busy, and the filter's rows are still prepared"
        );
        assert_eq!(built(), 1, "built once as the write looks busy");
        assert_eq!(
            waiting(cx, "creating branch…"),
            Some(false),
            "naming the write"
        );
        drop(write);
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the write");
        write.await;
        cx.executor().run_until_parked();
        next_frame(cx);
        assert!(
            app.read_with(cx, |app, _| app.operation_busy.is_none()
                && app.working_paths.pending),
            "the write ended, and the filter's rows are still prepared"
        );
        assert_eq!(built(), 1, "built once as the write ends");
        assert_eq!(
            waiting(cx, "filtering…"),
            Some(false),
            "for the filter again"
        );
        quiet_reads_land(&app, cx);
        next_frame(cx);
        assert!(
            app.read_with(cx, |app, _| !app.write_unsettled()
                && app.working_paths.pending
                && app.working_paths.preparation == Preparation::Filter),
            "the write's reload landed, and the filter's rows are still prepared"
        );
        assert_eq!(built(), 0, "not built as the write's reload lands");
        std::fs::write(repository.join("omega.txt"), "omega\n").unwrap();
        quietly_read_while_rows_held(&app, cx);
        assert!(
            app.read_with(cx, |app, _| !app.write_unsettled()
                && app.working_paths.preparation == Preparation::Status),
            "the filter prepares the newer status's rows, and no write is unsettled"
        );
        assert_eq!(
            built(),
            1,
            "built once as the newer status's rows become why"
        );
        assert_eq!(
            waiting(cx, "refreshing…"),
            Some(false),
            "for the newer status"
        );
        drop(rows);
        crate::path_filter::filters_land(&app, cx);
        next_frame(cx);
        assert_eq!(built(), 1, "built once as the rows land");
        assert_eq!(
            offered(cx, "Discard changes…"),
            Some(true),
            "once the rows land"
        );
    }

    /// A working row's right-click menu waits for the rows a status read
    /// brings: a Stage of zeta ends after gamma, untracked as the menu
    /// opened, was added and edited outside the app, and the Stage's read
    /// lands with its rows held. The menu keeps its items refused as the
    /// write left them, rather than offering Delete untracked file… for the
    /// file as the previous rows, still drawn, show it, and once the rows
    /// land it offers Discard changes…. A menu opened on the previous rows
    /// while new ones are prepared refuses its items, as a click on those
    /// rows refuses, each saying why ("Refreshing…"), until the rows land
    /// and build it again; it once showed them refused with no reason.
    /// Built again as the read landed, the menu offered Delete untracked
    /// file… for a file the read had found added, until the rows landed.
    #[gpui::test]
    async fn a_right_click_working_menu_waits_for_its_rows(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = six_files_for_menus(cx).await;
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        open_row_menu(&app, cx, "gamma.txt", Opening::RightClick);
        let delete = |cx: &mut VisualTestContext| offered(cx, "Delete untracked file…");
        let discard = |cx: &mut VisualTestContext| offered(cx, "Discard changes…");
        let write = held(
            &app,
            cx,
            gitturtle_core::WriteCommand::Stage {
                paths: vec![PathBuf::from("zeta.txt")],
            },
        );
        assert_eq!(delete(cx), Some(false), "while the Stage runs");
        fixture_git(&repository, &["add", "gamma.txt"]);
        std::fs::write(repository.join("gamma.txt"), "gamma\nedited\n").unwrap();
        let rows = app.update(cx, |app, _| app.file_paths.executor().hold());
        // The Stage ends and its status read lands where the window handles
        // it, the read not taken and awaited.
        drop(write);
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the write");
        write.await;
        app.read_with(cx, |app, _| app.operations.drain());
        cx.executor().run_until_parked();
        next_frame(cx);
        assert!(
            app.read_with(cx, |app, _| !app.write_unsettled()
                && app.working_paths.pending),
            "the Stage and its read landed, and its rows are held"
        );
        assert_eq!(
            delete(cx),
            Some(false),
            "refused while the rows are prepared, not offered for gamma as the previous rows show it"
        );
        assert_eq!(discard(cx), None, "built from the previous rows no more");
        drop(rows);
        crate::path_filter::filters_land(&app, cx);
        next_frame(cx);
        assert_eq!(
            discard(cx),
            Some(true),
            "for gamma's edit once its rows land"
        );
        assert_eq!(delete(cx), None, "once its rows land");
        cx.simulate_keystrokes("escape");
        next_frame(cx);
        // Opened on the previous rows while a new file's rows are prepared.
        let gamma: &'static str = format!(
            "working-row-{}",
            position_of(&app, cx, "gamma.txt", ChangeArea::Unstaged)
        )
        .leak();
        let rows = app.update(cx, |app, _| app.file_paths.executor().hold());
        std::fs::write(repository.join("omega.txt"), "omega\n").unwrap();
        quietly_read_while_rows_held(&app, cx);
        right_click(cx, gamma);
        assert_eq!(
            offered(cx, "Discard changes…, refreshing…"),
            Some(false),
            "a menu opened on rows being replaced refuses its items, each saying why"
        );
        assert_eq!(discard(cx), None, "none refused with nothing to say why");
        drop(rows);
        crate::path_filter::filters_land(&app, cx);
        next_frame(cx);
        assert_eq!(discard(cx), Some(true), "available once the rows land");
        cx.simulate_keystrokes("escape");
        next_frame(cx);
    }

    /// A working row's menu, opened as `opening` opens it at rest, keeps
    /// its items while the rows of a newer status are prepared, and is
    /// built again at once, every item refused and saying why, as a write
    /// starts meanwhile: "Refreshing…", for the rows, since the write shows
    /// none of its busy look yet; once the rows land it is built from them,
    /// refusing what the write refuses, and once the write's reload lands it
    /// offers its items again. Waiting for the rows, it kept offering Delete
    /// untracked file… through the write until the rows landed.
    async fn a_row_menu_refuses_a_write_started_while_its_rows_are_prepared(
        cx: &mut TestAppContext,
        opening: Opening,
    ) {
        let (_fixture, app, cx) = six_files_for_menus(cx).await;
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        open_row_menu(&app, cx, "gamma.txt", opening);
        let delete = |cx: &mut VisualTestContext| offered(cx, "Delete untracked file…");
        // Until the held write looks busy, the rows are why.
        let waiting =
            |cx: &mut VisualTestContext| offered(cx, "Delete untracked file…, refreshing…");
        assert_eq!(delete(cx), Some(true), "{opening:?}: at rest");
        let rows = app.update(cx, |app, _| app.file_paths.executor().hold());
        std::fs::write(repository.join("omega.txt"), "omega\n").unwrap();
        quietly_read_while_rows_held(&app, cx);
        assert_eq!(
            delete(cx),
            Some(true),
            "{opening:?}: kept as it was built while the rows are prepared"
        );
        let write = held(
            &app,
            cx,
            gitturtle_core::WriteCommand::CreateBranch {
                name: "side-q9".into(),
                start_point: None,
            },
        );
        assert!(
            app.read_with(cx, |app, _| app.operation_busy.is_some()
                && app.working_paths.pending),
            "{opening:?}: a write runs while the rows are prepared"
        );
        assert_eq!(
            waiting(cx),
            Some(false),
            "{opening:?}: refused at once as the write starts, saying why"
        );
        assert_eq!(delete(cx), None, "{opening:?}: offered no more");
        drop(rows);
        crate::path_filter::filters_land(&app, cx);
        next_frame(cx);
        assert_eq!(
            delete(cx),
            Some(false),
            "{opening:?}: built from the new rows, refused while the write runs"
        );
        assert_eq!(waiting(cx), None, "{opening:?}: the rows have landed");
        release(&app, write, cx).await;
        quiet_reads_land(&app, cx);
        crate::path_filter::filters_land(&app, cx);
        next_frame(cx);
        assert_eq!(
            delete(cx),
            Some(true),
            "{opening:?}: offered once the write's reload lands"
        );
    }

    #[gpui::test]
    async fn a_right_click_menu_refuses_a_write_started_while_its_rows_are_prepared(
        cx: &mut TestAppContext,
    ) {
        a_row_menu_refuses_a_write_started_while_its_rows_are_prepared(cx, Opening::RightClick)
            .await;
    }

    #[gpui::test]
    async fn a_keyboard_menu_refuses_a_write_started_while_its_rows_are_prepared(
        cx: &mut TestAppContext,
    ) {
        a_row_menu_refuses_a_write_started_while_its_rows_are_prepared(cx, Opening::Keyboard).await;
    }

    /// What the rows a Working row's menu waits for are prepared for.
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Cause {
        /// The filter's query.
        Filter,
        /// A newer status, as a quiet read lands it.
        Status,
    }

    /// A working row's menu waiting for rows being prepared for `cause`
    /// refuses every item, each stating why at its row's end
    /// (`working_rows_wait`): "Filtering…" for the filter's query, as the
    /// list says "Filtering working paths…" until its first rows land, and
    /// "Refreshing…" for a newer status; while a write started meanwhile
    /// runs, `through_write`, what the rows are for until the write looks
    /// busy, `BUSY_LOOK_DELAY` after the rows began to be prepared, and
    /// then the write, by the label it runs under, as Fetch, Pull and Push
    /// name it, built again as it looks busy, and once more what the rows
    /// are for as it ends, built again then. Named as it started, the write
    /// was named inside the delay in which a write that lands sooner shows
    /// none of its busy look. A right-click's menu opens on the rows
    /// being prepared; a keyboard's, which Shift-F10 on those rows does not
    /// open, opens at rest and keeps its items until a write starts. Once
    /// the rows land, and any write's reload, it offers its items. Every
    /// item once said "Refreshing…", for a filter's rows too and through a
    /// write, though the Changes header says so only while a status is read.
    async fn a_waiting_row_menu_states_why_by_its_cause(
        cx: &mut TestAppContext,
        opening: Opening,
        cause: Cause,
        through_write: bool,
    ) {
        let at = format!("{opening:?}, {cause:?}");
        let (_fixture, app, cx) = six_files_for_menus(cx).await;
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let gamma: &'static str = format!(
            "working-row-{}",
            position_of(&app, cx, "gamma.txt", ChangeArea::Unstaged)
        )
        .leak();
        let waiting = |cx: &mut VisualTestContext, reason: &str| {
            offered(cx, &format!("Delete untracked file…, {reason}"))
        };
        let delete = |cx: &mut VisualTestContext| offered(cx, "Delete untracked file…");
        if let Opening::Keyboard = opening {
            open_row_menu(&app, cx, "gamma.txt", opening);
            assert_eq!(delete(cx), Some(true), "{at}: at rest");
        }
        let rows = app.update(cx, |app, _| app.file_paths.executor().hold());
        if cause == Cause::Filter {
            // Every file but epsilon matches.
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.working_filter
                        .update(cx, |input, cx| input.set_value("a", window, cx));
                    app.filter_working_paths(window, cx);
                })
            });
            next_frame(cx);
        } else {
            std::fs::write(repository.join("omega.txt"), "omega\n").unwrap();
            quietly_read_while_rows_held(&app, cx);
        }
        assert!(
            app.read_with(cx, |app, _| app.working_paths.pending),
            "{at}: the rows are prepared"
        );
        let reason = if cause == Cause::Filter {
            "filtering…"
        } else {
            "refreshing…"
        };
        match opening {
            Opening::RightClick => {
                right_click(cx, gamma);
                assert_eq!(
                    waiting(cx, reason),
                    Some(false),
                    "{at}: opened on rows being prepared, refused, saying {reason:?}"
                );
                assert_eq!(
                    delete(cx),
                    None,
                    "{at}: none refused with nothing to say why"
                );
            }
            Opening::Keyboard => assert_eq!(
                delete(cx),
                Some(true),
                "{at}: kept as it was built while the rows are prepared"
            ),
        }
        if through_write {
            let write = app.update(cx, |app, _| app.operations.hold());
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.write(
                        gitturtle_core::WriteCommand::CreateBranch {
                            name: "side-q9".into(),
                            start_point: None,
                        },
                        "Creating branch…",
                        window,
                        cx,
                    )
                })
            });
            next_frame(cx);
            assert!(
                app.read_with(cx, |app, _| app.operation_busy.is_some()
                    && app.operation_shown().is_none()),
                "{at}: the write runs, and does not look busy yet"
            );
            assert_eq!(
                waiting(cx, reason),
                Some(false),
                "{at}: built again as the write starts, saying {reason:?} until it looks busy"
            );
            assert_eq!(
                waiting(cx, "creating branch…"),
                None,
                "{at}: not named before it looks busy"
            );
            assert_eq!(delete(cx), None, "{at}: offered no more");
            cx.executor()
                .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
            cx.executor().run_until_parked();
            next_frame(cx);
            assert!(
                app.read_with(cx, |app, _| app.operation_shown().is_some()
                    && app.working_paths.pending),
                "{at}: the write looks busy, and the rows are still prepared"
            );
            assert_eq!(
                waiting(cx, "creating branch…"),
                Some(false),
                "{at}: built again as the write looks busy, naming it"
            );
            assert_eq!(waiting(cx, reason), None, "{at}: the write is why");
            // The write ends while the rows are still prepared.
            drop(write);
            let task = app
                .update(cx, |app, _| app.operation_task.take())
                .expect("the write");
            task.await;
            cx.executor().run_until_parked();
            next_frame(cx);
            assert!(
                app.read_with(cx, |app, _| app.operation_busy.is_none()
                    && app.working_paths.pending),
                "{at}: the write ended, and the rows are still prepared"
            );
            assert_eq!(
                waiting(cx, reason),
                Some(false),
                "{at}: built again as the write ends, saying {reason:?}"
            );
            assert_eq!(
                waiting(cx, "creating branch…"),
                None,
                "{at}: the ended write is not named"
            );
        }
        drop(rows);
        crate::path_filter::filters_land(&app, cx);
        if through_write {
            quiet_reads_land(&app, cx);
            crate::path_filter::filters_land(&app, cx);
        }
        next_frame(cx);
        assert_eq!(delete(cx), Some(true), "{at}: offered once the rows land");
        cx.simulate_keystrokes("escape");
        next_frame(cx);
    }

    #[gpui::test]
    async fn a_right_click_menu_waiting_for_filtered_rows_says_filtering(cx: &mut TestAppContext) {
        a_waiting_row_menu_states_why_by_its_cause(cx, Opening::RightClick, Cause::Filter, false)
            .await;
    }

    #[gpui::test]
    async fn a_right_click_menu_waiting_for_a_newer_status_says_refreshing(
        cx: &mut TestAppContext,
    ) {
        a_waiting_row_menu_states_why_by_its_cause(cx, Opening::RightClick, Cause::Status, false)
            .await;
    }

    #[gpui::test]
    async fn a_right_click_menu_waiting_through_a_write_names_the_write(cx: &mut TestAppContext) {
        a_waiting_row_menu_states_why_by_its_cause(cx, Opening::RightClick, Cause::Status, true)
            .await;
    }

    #[gpui::test]
    async fn a_keyboard_menu_waiting_for_filtered_rows_names_a_write_then_says_filtering(
        cx: &mut TestAppContext,
    ) {
        a_waiting_row_menu_states_why_by_its_cause(cx, Opening::Keyboard, Cause::Filter, true)
            .await;
    }

    #[gpui::test]
    async fn a_keyboard_menu_waiting_for_a_newer_status_names_a_write_then_says_refreshing(
        cx: &mut TestAppContext,
    ) {
        a_waiting_row_menu_states_why_by_its_cause(cx, Opening::Keyboard, Cause::Status, true)
            .await;
    }

    /// A working row's menu waiting for the filter's rows, each item saying
    /// "Filtering…", is built again as a newer status lands meanwhile, as a
    /// file added outside the app brings: the rows the filter then prepares
    /// are that status's, and each item says "Refreshing…" until they land
    /// (`working_rows_wait`). A right-click's menu opens on the filter's
    /// rows; a keyboard's opens at rest and waits with them once a write
    /// that starts and ends meanwhile has built it again. Either kept saying
    /// "Filtering…" until the rows landed, since the status's landing starts
    /// no write and ends none.
    async fn a_waiting_row_menu_follows_a_newer_status_under_the_filter(
        cx: &mut TestAppContext,
        opening: Opening,
    ) {
        use crate::working_selection::Preparation;
        let at = format!("{opening:?}");
        let (_fixture, app, cx) = six_files_for_menus(cx).await;
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let gamma: &'static str = format!(
            "working-row-{}",
            position_of(&app, cx, "gamma.txt", ChangeArea::Unstaged)
        )
        .leak();
        let waiting = |cx: &mut VisualTestContext, reason: &str| {
            offered(cx, &format!("Delete untracked file…, {reason}"))
        };
        let delete = |cx: &mut VisualTestContext| offered(cx, "Delete untracked file…");
        if let Opening::Keyboard = opening {
            open_row_menu(&app, cx, "gamma.txt", opening);
            assert_eq!(delete(cx), Some(true), "{at}: at rest");
        }
        let rows = app.update(cx, |app, _| app.file_paths.executor().hold());
        // Every file but epsilon matches.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.working_filter
                    .update(cx, |input, cx| input.set_value("a", window, cx));
                app.filter_working_paths(window, cx);
            })
        });
        next_frame(cx);
        match opening {
            Opening::RightClick => right_click(cx, gamma),
            Opening::Keyboard => {
                let write = held(
                    &app,
                    cx,
                    gitturtle_core::WriteCommand::CreateBranch {
                        name: "side-q9".into(),
                        start_point: None,
                    },
                );
                drop(write);
                let task = app
                    .update(cx, |app, _| app.operation_task.take())
                    .expect("the write");
                task.await;
                cx.executor().run_until_parked();
                quiet_reads_land(&app, cx);
                next_frame(cx);
            }
        }
        assert!(
            app.read_with(cx, |app, _| !app.write_unsettled()
                && app.working_paths.pending
                && app.working_paths.preparation == Preparation::Filter),
            "{at}: the filter's rows are prepared, and no write is unsettled"
        );
        assert_eq!(
            waiting(cx, "filtering…"),
            Some(false),
            "{at}: waiting for the filter's rows, saying so"
        );
        std::fs::write(repository.join("omega.txt"), "omega\n").unwrap();
        quietly_read_while_rows_held(&app, cx);
        assert!(
            app.read_with(cx, |app, _| !app.write_unsettled()
                && app.working_paths.preparation == Preparation::Status),
            "{at}: the filter prepares the newer status's rows, and no write is unsettled"
        );
        assert_eq!(
            waiting(cx, "refreshing…"),
            Some(false),
            "{at}: built again for the newer status's rows, saying so"
        );
        assert_eq!(
            waiting(cx, "filtering…"),
            None,
            "{at}: the filter's own rows are no longer why"
        );
        drop(rows);
        crate::path_filter::filters_land(&app, cx);
        next_frame(cx);
        assert_eq!(delete(cx), Some(true), "{at}: offered once the rows land");
        cx.simulate_keystrokes("escape");
        next_frame(cx);
    }

    #[gpui::test]
    async fn a_right_click_menu_waiting_for_filtered_rows_follows_a_newer_status(
        cx: &mut TestAppContext,
    ) {
        a_waiting_row_menu_follows_a_newer_status_under_the_filter(cx, Opening::RightClick).await;
    }

    #[gpui::test]
    async fn a_keyboard_menu_waiting_for_filtered_rows_follows_a_newer_status(
        cx: &mut TestAppContext,
    ) {
        a_waiting_row_menu_follows_a_newer_status_under_the_filter(cx, Opening::Keyboard).await;
    }

    /// A History window on a clean worktree, wide enough for the action bar
    /// and the navigator, with the app's keys bound. Keep the returned
    /// directory until the test ends.
    async fn clean_history_for_menus(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, Entity<GitTurtle>, &mut VisualTestContext) {
        let (fixture, app, cx) = history_window_from(cx, history_script(40, 3)).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        cx.update(|_, cx| {
            crate::shortcuts::bind_keys(cx);
            crate::native_accessibility::bind_keys(cx);
        });
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        fixture_git(&repository, &["reset", "-q", "--hard"]);
        refreshed(&app, cx).await;
        (fixture, app, cx)
    }

    /// A Checkout of the branch already out: it changes nothing, and
    /// reloads the repository quietly as it ends, as every write that can
    /// move a ref does (`refresh_after_write`).
    fn checkout_main() -> gitturtle_core::WriteCommand {
        gitturtle_core::WriteCommand::Checkout {
            branch: "main".into(),
        }
    }

    /// A navigator branch's keyboard menu stays refused once a write that
    /// reloads the repository quietly ends, until that reload lands, held
    /// here, as a working row's menu does
    /// (`a_row_menu_waits_for_its_writes_reload`): built again as the write
    /// ended, Branch actions… acted on a branch a delete had just removed.
    #[gpui::test]
    async fn a_keyboard_branch_menu_waits_for_its_writes_reload(cx: &mut TestAppContext) {
        let (fixture, app, cx) = clean_history_for_menus(cx).await;
        let main = app.read_with(cx, |app, _| {
            app.nav_rows.iter().position(|row| {
                matches!(row, crate::NavRow::Branch(branch, _)
                    if app.branches[*branch].name == "main")
            })
        });
        let main = main.expect("main in the navigator");
        let item = ["Branch actions…"];
        open_keyed_menu(&app, cx, KeyedRow::Branch(main), "shift-f10").await;
        keyed_items(&app, cx, &item, true, "main at rest");
        hold_quiet_reads(&app, cx, true);
        let write = held(&app, cx, checkout_main());
        keyed_items(&app, cx, &item, false, "while the write runs");
        release(&app, write, cx).await;
        next_frame(cx);
        assert!(
            app.read_with(cx, |app, _| app.operation_busy.is_none()),
            "the write ended"
        );
        keyed_items(&app, cx, &item, false, "until the write's reload lands");
        hold_quiet_reads(&app, cx, false);
        keyed_items(&app, cx, &item, true, "once it lands");
        close_keyed_menu(&app, cx, "the branch's menu");
        drop(fixture);
    }

    /// The branch picker's menu, an AppDropdown opened as a write that
    /// reloads the repository quietly runs, stays refused once the write
    /// ends, until that reload lands, held here, as a row's menu does.
    #[gpui::test]
    async fn the_branch_picker_waits_for_its_writes_reload(cx: &mut TestAppContext) {
        const FIND: &str = "Find or create a branch…";
        let (fixture, app, cx) = clean_history_for_menus(cx).await;
        hold_quiet_reads(&app, cx, true);
        let write = held(&app, cx, checkout_main());
        press(cx, "current-branch");
        assert_eq!(offered(cx, FIND), Some(false), "while the write runs");
        release(&app, write, cx).await;
        next_frame(cx);
        assert_eq!(
            offered(cx, FIND),
            Some(false),
            "the picker's menu waits for the write's reload"
        );
        hold_quiet_reads(&app, cx, false);
        assert_eq!(
            offered(cx, FIND),
            Some(true),
            "the picker's menu once the reload lands"
        );
        cx.simulate_keystrokes("escape");
        next_frame(cx);
        drop(fixture);
    }

    /// Blame opened once a write has ended, before its quiet reload lands,
    /// holds the reload off until it closes: a write closes Blame as it
    /// starts, and Blame's own read puts a reload that runs back to wait
    /// (`cancel_automatic_read`), as the reload held here waits. The reload
    /// refuses nothing meanwhile, so the branch picker's menu offers its
    /// items while Blame stays open, and closing Blame runs it. Counted
    /// while it waited, the reload left every menu that follows writes
    /// refused, with nothing to say why, for as long as Blame stayed open,
    /// and after it closed until the next local change, which alone started
    /// the reload.
    #[gpui::test]
    async fn blame_held_open_across_a_write_refuses_nothing(cx: &mut TestAppContext) {
        const FIND: &str = "Find or create a branch…";
        let (fixture, app, cx) = clean_history_for_menus(cx).await;
        hold_quiet_reads(&app, cx, true);
        let write = held(&app, cx, checkout_main());
        release(&app, write, cx).await;
        next_frame(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_blame(0, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        // Blame now holds the reload as the hold did.
        hold_quiet_reads(&app, cx, false);
        assert_eq!(
            app.read_with(cx, |app, _| (
                app.operation_busy.is_none(),
                app.blame.is_visible(),
                app.automatic.quiet_read_requested()
            )),
            (true, true, true),
            "the write ended, Blame stays open, and the write's reload waits for it"
        );
        press(cx, "current-branch");
        assert_eq!(
            offered(cx, FIND),
            Some(true),
            "the picker's menu offers its items while Blame holds the reload"
        );
        cx.simulate_keystrokes("escape");
        next_frame(cx);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.close_blame(window, cx);
            })
        });
        next_frame(cx);
        quiet_reads_land(&app, cx);
        assert!(
            !app.read_with(cx, |app, _| app.automatic.quiet_read_requested()),
            "closing Blame ran the write's reload"
        );
        drop(fixture);
    }

    /// A project row's keyboard menu in the project pane offers Open
    /// project as soon as a write ends: opening another project reads
    /// nothing the write's quiet reload brings, so it does not wait for that
    /// reload as other menus do; on Settings, where no quiet read runs, the
    /// reload waits for the window's return and refuses nothing meanwhile.
    /// Read from `write_unsettled`, Open project stayed refused after the
    /// write until the reload landed, and on Settings until the window
    /// returned to the repository.
    #[gpui::test]
    async fn a_project_row_waits_for_no_writes_reload(cx: &mut TestAppContext) {
        const OPEN: &str = "Open project";
        let (fixture, app, cx) = clean_history_for_menus(cx).await;
        app.read_with(cx, |app, _| app.preferences_writer.drain());
        app.update(cx, |app, cx| {
            let mut library = crate::project_library::ProjectLibrary::default();
            library.remember(std::path::Path::new("/projects/alpha"));
            app.set_project_library(library);
            app.settings.project_pane = true;
            cx.notify();
        });
        next_frame(cx);
        let open_menu = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.move_project_cursor(true, true, window, cx);
                    window.focus(&app.project_pane.focus, cx);
                })
            });
            next_frame(cx);
            cx.simulate_keystrokes("shift-f10");
            cx.executor().run_until_parked();
            next_frame(cx);
            assert!(
                offered(cx, OPEN).is_some(),
                "Shift-F10 opened the project's menu"
            );
        };
        open_menu(cx);
        assert_eq!(offered(cx, OPEN), Some(true), "at rest");
        hold_quiet_reads(&app, cx, true);
        let write = held(&app, cx, checkout_main());
        assert_eq!(offered(cx, OPEN), Some(false), "while the write runs");
        release(&app, write, cx).await;
        next_frame(cx);
        assert!(
            app.read_with(cx, |app, _| app.operation_busy.is_none()
                && app.automatic.quiet_read_requested()),
            "the write ended, and its reload is held"
        );
        assert_eq!(
            offered(cx, OPEN),
            Some(true),
            "once the write ends, before its reload lands"
        );
        hold_quiet_reads(&app, cx, false);
        cx.simulate_keystrokes("escape");
        next_frame(cx);

        // A write that ends while Settings is shown.
        let write = held(&app, cx, checkout_main());
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
        next_frame(cx);
        release(&app, write, cx).await;
        next_frame(cx);
        assert!(
            app.read_with(cx, |app, _| app.page == AppPage::Settings
                && app.operation_busy.is_none()
                && app.automatic.quiet_read_requested()),
            "the write ended on Settings, and its reload waits for the repository"
        );
        assert!(
            !app.read_with(cx, |app, _| app.write_unsettled()),
            "a reload that waits for the window's return refuses nothing"
        );
        open_menu(cx);
        assert_eq!(
            offered(cx, OPEN),
            Some(true),
            "on Settings once the write ends"
        );
        cx.simulate_keystrokes("escape");
        next_frame(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
        next_frame(cx);
        quiet_reads_land(&app, cx);
        assert!(
            !app.read_with(cx, |app, _| app.automatic.quiet_read_requested()),
            "returning to the repository ran the write's reload"
        );
        drop(fixture);
    }

    /// Lets the quiet reload a write asked for as it ended, held until now,
    /// start, and leaves it running.
    fn reload_runs(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.automatic.hold_quiet_reads(false);
                app.try_automatic_refresh(window, cx);
            })
        });
        assert!(
            app.read_with(cx, |app, _| app.automatic.quiet_read_running()),
            "the write's reload runs"
        );
    }

    /// Asserts that a selection's read runs, which has put the write's
    /// reload back to wait for it (`cancel_automatic_read`).
    fn selection_holds_the_reload(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        assert!(
            app.read_with(cx, |app, _| app.loading.is_some()
                && !app.automatic.quiet_read_running()
                && app.automatic.quiet_read_requested()),
            "the selection's read runs, and the write's reload waits for it"
        );
    }

    /// A commit selected while a write's quiet reload runs puts the reload
    /// back to wait for the selection's read, which starts it again as it
    /// lands; menus stay refused throughout. Down, Up and Shift-F10 on HEAD,
    /// pressed as an Undo's reload ran, once opened HEAD's menu offering
    /// Revert, Cherry-pick, Amend and Undo of the commit the Undo had just
    /// taken away: a reload waiting for a selection's read refused nothing,
    /// as one an open inspection holds does, so the menu's items were
    /// available, refused once the read landed and the reload ran, and
    /// available again, its cursor reset at each.
    #[gpui::test]
    async fn a_commit_selected_as_a_writes_reload_runs_keeps_menus_refused(
        cx: &mut TestAppContext,
    ) {
        let (fixture, app, cx) = clean_history_for_menus(cx).await;
        let head = app.read_with(cx, |app, _| {
            let head = app
                .work_status
                .as_ref()
                .and_then(|status| status.head.clone());
            app.commits
                .iter()
                .position(|commit| Some(&commit.oid) == head.as_ref())
                .expect("HEAD in History")
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.show_history(window, cx);
                app.select_commit(head, window, cx);
                app.pane = Pane::History;
            })
        });
        crate::views::tests::settle(&app, cx).await;
        hold_quiet_reads(&app, cx, true);
        let write = held(&app, cx, checkout_main());
        release(&app, write, cx).await;
        next_frame(cx);
        let list = app.read_with(cx, |app, _| app.focus.clone());
        cx.update(|window, cx| list.focus(window, cx));
        next_frame(cx);
        reload_runs(&app, cx);
        // Each key is taken at once, before the selection's read can land.
        cx.update(|window, cx| {
            for key in ["down", "up", "shift-f10"] {
                window.dispatch_keystroke(Keystroke::parse(key).expect("a keystroke"), cx);
            }
        });
        next_frame(cx);
        selection_holds_the_reload(&app, cx);
        assert!(
            app.read_with(cx, |app, _| app.selected_commit == Some(head)
                && app.commit_menu.is_some()),
            "Down and Up came back to HEAD, and Shift-F10 opened its menu"
        );
        keyed_items(
            &app,
            cx,
            &COMMIT_ITEMS,
            false,
            "while the selection's read holds the write's reload",
        );
        // Held, the reload the landing read starts waits here rather than
        // landing on the reader's thread while the read is settled.
        app.update(cx, |app, _| app.automatic.hold_quiet_reads(true));
        crate::views::tests::settle(&app, cx).await;
        assert!(
            app.read_with(cx, |app, _| app.loading.is_none()
                && app.automatic.quiet_read_requested()),
            "the selection's read landed, and the write's reload waits to run again"
        );
        keyed_items(
            &app,
            cx,
            &COMMIT_ITEMS,
            false,
            "once the selection's read lands, before the write's reload",
        );
        hold_quiet_reads(&app, cx, false);
        keyed_items(
            &app,
            cx,
            &COMMIT_ITEMS,
            true,
            "once the write's reload lands",
        );
        close_keyed_menu(&app, cx, "HEAD's menu");
        drop(fixture);
    }

    /// A working row selected while a write's quiet reload runs, as a click
    /// on another row after a Commit, puts the reload back to wait for the
    /// row's read; the row's menu, opened as `opening` opens it before that
    /// read lands, stays refused until the reload lands. It was built from
    /// the row's entry as the write had left it, before the reload, as a
    /// just-committed file's Delete untracked file… or Discard changes….
    async fn a_row_selected_as_a_writes_reload_runs_keeps_its_menu_refused(
        cx: &mut TestAppContext,
        opening: Opening,
    ) {
        const DELETE: &str = "Delete untracked file…";
        let (_fixture, app, cx) = six_files_for_menus(cx).await;
        hold_quiet_reads(&app, cx, true);
        let write = held(
            &app,
            cx,
            gitturtle_core::WriteCommand::CreateBranch {
                name: "side-q9".into(),
                start_point: None,
            },
        );
        release(&app, write, cx).await;
        next_frame(cx);
        let selector: &'static str = format!(
            "working-row-{}",
            position_of(&app, cx, "gamma.txt", ChangeArea::Unstaged)
        )
        .leak();
        let row = cx.debug_bounds(selector).expect(selector);
        let list = app.read_with(cx, |app, _| app.file_focus.clone());
        reload_runs(&app, cx);
        // The click and the menu are taken at once, before the row's read
        // can land.
        let press = |window: &mut Window, cx: &mut App, button, position| {
            window.dispatch_event(
                PlatformInput::MouseDown(MouseDownEvent {
                    position,
                    modifiers: Modifiers::default(),
                    button,
                    click_count: 1,
                    first_mouse: false,
                }),
                cx,
            );
            window.dispatch_event(
                PlatformInput::MouseUp(MouseUpEvent {
                    position,
                    modifiers: Modifiers::default(),
                    button,
                    click_count: 1,
                }),
                cx,
            );
        };
        cx.update(|window, cx| {
            press(window, cx, gpui::MouseButton::Left, row.center());
            list.focus(window, cx);
        });
        next_frame(cx);
        cx.update(|window, cx| match opening {
            Opening::RightClick => press(
                window,
                cx,
                gpui::MouseButton::Right,
                point(row.left() + px(16.), row.center().y),
            ),
            Opening::Keyboard => {
                window.dispatch_keystroke(Keystroke::parse("shift-f10").expect("a keystroke"), cx);
            }
        });
        next_frame(cx);
        next_frame(cx);
        selection_holds_the_reload(&app, cx);
        assert!(
            app.read_with(cx, |app, _| app
                .working_selected
                .and_then(|(index, _)| app.work_status.as_ref()?.entries.get(index))
                .is_some_and(
                    |entry| entry.path.as_os_str() == std::ffi::OsStr::new("gamma.txt")
                )),
            "{opening:?}: the click selected gamma"
        );
        assert_eq!(
            offered(cx, DELETE),
            Some(false),
            "{opening:?}: refused while the row's read holds the write's reload"
        );
        crate::views::tests::settle(&app, cx).await;
        quiet_reads_land(&app, cx);
        crate::path_filter::filters_land(&app, cx);
        next_frame(cx);
        assert_eq!(
            offered(cx, DELETE),
            Some(true),
            "{opening:?}: available once the write's reload lands"
        );
        cx.simulate_keystrokes("escape");
        next_frame(cx);
    }

    #[gpui::test]
    async fn a_row_selected_as_a_writes_reload_runs_keeps_its_right_click_menu_refused(
        cx: &mut TestAppContext,
    ) {
        a_row_selected_as_a_writes_reload_runs_keeps_its_menu_refused(cx, Opening::RightClick)
            .await;
    }

    #[gpui::test]
    async fn a_row_selected_as_a_writes_reload_runs_keeps_its_keyboard_menu_refused(
        cx: &mut TestAppContext,
    ) {
        a_row_selected_as_a_writes_reload_runs_keeps_its_menu_refused(cx, Opening::Keyboard).await;
    }

    /// A write that ends while Settings is shown leaves its quiet reload
    /// waiting for the repository, and returning there through the
    /// repository's own tab, as its entry in the tab strip, the tab menu,
    /// the tab's shortcut and opening the project again do
    /// (`switch_repository_tab`), starts it, as Back does
    /// (`return_from_page`). It once waited for the next local change or a
    /// regained focus, every menu that follows writes refused meanwhile.
    #[gpui::test]
    async fn returning_through_the_active_tab_runs_a_writes_reload(cx: &mut TestAppContext) {
        const FIND: &str = "Find or create a branch…";
        let (fixture, app, cx) = clean_history_for_menus(cx).await;
        let write = held(&app, cx, checkout_main());
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
        next_frame(cx);
        release(&app, write, cx).await;
        next_frame(cx);
        assert!(
            app.read_with(cx, |app, _| app.page == AppPage::Settings
                && app.operation_busy.is_none()
                && app.automatic.quiet_read_requested()),
            "the write ended on Settings, and its reload waits for the repository"
        );
        let tab = app.read_with(cx, |app, _| {
            app.repository_tabs.active.expect("the repository's tab")
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| app.switch_repository_tab(tab, window, cx))
        });
        next_frame(cx);
        assert!(
            app.read_with(cx, |app, _| app.page == AppPage::Repository
                && app.automatic.quiet_read_running()),
            "back on the repository through its tab, the write's reload runs"
        );
        quiet_reads_land(&app, cx);
        next_frame(cx);
        press(cx, "current-branch");
        assert_eq!(
            offered(cx, FIND),
            Some(true),
            "the picker's menu once the reload lands"
        );
        cx.simulate_keystrokes("escape");
        next_frame(cx);
        drop(fixture);
    }

    /// The page the window leaves the repository for: Settings, or the hub.
    #[derive(Clone, Copy, Debug)]
    enum Away {
        Settings,
        Projects,
    }

    /// Leaves the repository for `away`, as its button does, and draws the
    /// page, which takes focus on its first control.
    fn leave_for(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, away: Away) {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| match away {
                Away::Settings => app.show_settings(window, cx),
                Away::Projects => app.show_projects(window, cx),
            })
        });
        next_frame(cx);
        assert!(
            app.read_with(cx, |app, _| app.page != AppPage::Repository),
            "{away:?}: the repository is hidden"
        );
    }

    /// Returns to the repository through its own tab, as its entry in the
    /// tab strip does (`switch_repository_tab`), and draws the frames focus
    /// returns in.
    fn return_through_the_tab(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        let tab = app.read_with(cx, |app, _| {
            app.repository_tabs.active.expect("the repository's tab")
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| app.switch_repository_tab(tab, window, cx))
        });
        next_frame(cx);
        next_frame(cx);
        assert!(
            app.read_with(cx, |app, _| app.page == AppPage::Repository),
            "back on the repository through its tab"
        );
    }

    /// Focuses `focus`, which names `what`, leaves the repository for
    /// Settings and then for the hub, and returns each time through the
    /// repository's own tab: focus is back on `focus` each time, as Back
    /// puts it back (`resume_repository_page`).
    fn focus_returns_through_the_tab(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        focus: &FocusHandle,
        what: &str,
    ) {
        let focused = |cx: &mut VisualTestContext| cx.update(|window, _| focus.is_focused(window));
        for away in [Away::Settings, Away::Projects] {
            cx.update(|window, cx| window.focus(focus, cx));
            next_frame(cx);
            assert!(focused(cx), "{what}: focused before the window leaves");
            leave_for(app, cx, away);
            assert!(!focused(cx), "{what}: {away:?} took focus as it opened");
            return_through_the_tab(app, cx);
            assert!(
                focused(cx),
                "{what}: focus returned from {away:?} through the repository's tab"
            );
        }
    }

    /// Returning through the repository's own tab, from Settings or the
    /// hub, puts focus back on History's commit list where it was as the
    /// window left, as Back does (`resume_repository_page`).
    #[gpui::test]
    async fn returning_through_the_active_tab_puts_history_focus_back(cx: &mut TestAppContext) {
        let (fixture, app, cx) = clean_history_for_menus(cx).await;
        let list = app.read_with(cx, |app, _| app.focus.clone());
        focus_returns_through_the_tab(&app, cx, &list, "History's commit list");
        drop(fixture);
    }

    /// Returning through the repository's own tab puts focus back in
    /// Working's commit description where it was as the window left, its
    /// exact field rather than the list (`resume_repository_page`): without
    /// the return, focus stayed on the Settings or hub control the window
    /// had left, which no longer draws.
    #[gpui::test]
    async fn returning_through_the_active_tab_puts_the_commit_editors_focus_back(
        cx: &mut TestAppContext,
    ) {
        let (fixture, app, cx) = six_files_for_menus(cx).await;
        let description = app.read_with(cx, |app, cx| app.commit_message.read(cx).focus_handle(cx));
        focus_returns_through_the_tab(&app, cx, &description, "the commit description");
        drop(fixture);
    }

    /// Returning through the repository's own tab puts focus back in the
    /// Find field of Working's preview where it was as the window left,
    /// not on the preview's editor or the list (`resume_repository_page`).
    #[gpui::test]
    async fn returning_through_the_active_tab_puts_find_focus_back(cx: &mut TestAppContext) {
        let (fixture, app, cx) = six_files_for_menus(cx).await;
        crate::views::tests::settle(&app, cx).await;
        let editor = app.read_with(cx, |app, cx| {
            app.patch_editor
                .as_ref()
                .expect("alpha's preview has its editor")
                .focus_handle(cx)
        });
        cx.update(|window, cx| window.focus(&editor, cx));
        next_frame(cx);
        #[cfg(target_os = "macos")]
        cx.simulate_keystrokes("cmd-f");
        #[cfg(not(target_os = "macos"))]
        cx.simulate_keystrokes("ctrl-f");
        next_frame(cx);
        let find = cx
            .update(|window, cx| window.focused(cx))
            .expect("Find takes focus");
        assert!(
            !cx.update(|window, _| editor.is_focused(window)),
            "Find's field holds focus, not the editor"
        );
        focus_returns_through_the_tab(&app, cx, &find, "the preview's Find field");
        drop(fixture);
    }

    /// A working row's preview that lands while Settings is shown builds no
    /// editor, the page hidden (`receive`); returning through the
    /// repository's own tab builds it, as Back does
    /// (`resume_repository_page`), so the preview is drawn without the row
    /// being chosen again.
    #[gpui::test]
    async fn returning_through_the_active_tab_builds_a_preview_that_landed_meanwhile(
        cx: &mut TestAppContext,
    ) {
        let (fixture, app, cx) = six_files_for_menus(cx).await;
        crate::views::tests::settle(&app, cx).await;
        let gamma = app.read_with(cx, |app, _| {
            app.work_status
                .as_ref()
                .expect("a status")
                .entries
                .iter()
                .position(|entry| entry.path.as_os_str() == std::ffi::OsStr::new("gamma.txt"))
                .expect("gamma's entry")
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.select_working(gamma, ChangeArea::Unstaged, window, cx);
                app.show_settings(window, cx);
            })
        });
        crate::views::tests::settle(&app, cx).await;
        assert!(
            app.read_with(cx, |app, _| app.page == AppPage::Settings
                && app.working_selected == Some((gamma, ChangeArea::Unstaged))
                && app.content.is_some()
                && app.patch_editor.is_none()),
            "gamma's preview landed on Settings and built no editor"
        );
        return_through_the_tab(&app, cx);
        assert!(
            app.read_with(cx, |app, _| app.patch_editor.is_some()),
            "back through the tab, gamma's preview has its editor"
        );
        drop(fixture);
    }

    /// A working row's side selected with nothing to show stops the
    /// window's reads (`invalidate_read`), a write's running quiet reload
    /// with them, and starts that reload again, as History's file filter
    /// hiding the selected file and Working rows landing without the
    /// selected row do. With no read of its own to start it, the reload once
    /// waited for the next local change, every menu that follows writes
    /// refused meanwhile.
    #[gpui::test]
    async fn a_selection_with_nothing_to_show_runs_a_writes_reload_again(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = six_files_for_menus(cx).await;
        hold_quiet_reads(&app, cx, true);
        let write = held(
            &app,
            cx,
            gitturtle_core::WriteCommand::CreateBranch {
                name: "side-q9".into(),
                start_point: None,
            },
        );
        release(&app, write, cx).await;
        next_frame(cx);
        reload_runs(&app, cx);
        let gamma = app.read_with(cx, |app, _| {
            app.work_status
                .as_ref()
                .expect("a status")
                .entries
                .iter()
                .position(|entry| entry.path.as_os_str() == std::ffi::OsStr::new("gamma.txt"))
                .expect("gamma's entry")
        });
        // gamma is untracked: its Staged side has nothing to show.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.select_working(gamma, ChangeArea::Staged, window, cx)
            })
        });
        assert!(
            app.read_with(cx, |app, _| app.working_selected.is_none()),
            "nothing to show, nothing selected"
        );
        assert!(
            app.read_with(cx, |app, _| app.automatic.quiet_read_running()
                && app.write_unsettled()),
            "the write's reload runs again, and menus wait for it"
        );
        quiet_reads_land(&app, cx);
        crate::path_filter::filters_land(&app, cx);
        assert!(
            !app.read_with(cx, |app, _| app.write_unsettled()),
            "menus are offered once the reload lands"
        );
    }

    /// Ends a write that reloads the repository quietly as it ends, as
    /// Create branch does, its reload held from starting.
    async fn writes_reload_waits(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        hold_quiet_reads(app, cx, true);
        let write = held(
            app,
            cx,
            gitturtle_core::WriteCommand::CreateBranch {
                name: "side-q9".into(),
                start_point: None,
            },
        );
        release(app, write, cx).await;
        crate::path_filter::filters_land(app, cx);
        next_frame(cx);
        assert!(
            app.read_with(cx, |app, _| app.write_unsettled()
                && !app.automatic.quiet_read_running()),
            "the write ended, and its reload waits"
        );
    }

    /// Starts the write's reload, held until now, with the path filter's
    /// newest answer already waiting on the window's thread, where it lands
    /// first, as the reload runs. The filter's lane is a thread
    /// `run_until_parked` cannot wait for: drained before the reload starts,
    /// its answer is queued on the window's thread ahead of the reload's.
    fn filter_lands_as_the_reload_runs(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        app.read_with(cx, |app, _| app.file_paths.drain());
        reload_runs(app, cx);
        cx.executor().run_until_parked();
    }

    /// Working rows that land without the selected row, as a path filter
    /// that leaves its file out, stop the window's reads (`invalidate_read`),
    /// a write's running quiet reload with them, and start that reload
    /// again. Nothing else started it: it waited for the next local change,
    /// every menu that follows writes refused meanwhile.
    #[gpui::test]
    async fn working_rows_without_the_selection_run_a_writes_reload_again(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = six_files_for_menus(cx).await;
        writes_reload_waits(&app, cx).await;
        assert!(
            app.read_with(cx, |app, _| app.working_selected.is_some()),
            "alpha is selected"
        );
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.working_filter
                    .update(cx, |input, cx| input.set_value("zeta", window, cx));
                app.filter_working_paths(window, cx);
            })
        });
        filter_lands_as_the_reload_runs(&app, cx);
        assert!(
            app.read_with(cx, |app, _| app.working_selected.is_none()
                && !app.working_paths.pending),
            "the rows landed without alpha's"
        );
        quiet_reads_land(&app, cx);
        crate::path_filter::filters_land(&app, cx);
        assert!(
            !app.read_with(cx, |app, _| app.write_unsettled()),
            "the reload ran again and landed, and menus are offered"
        );
    }

    /// History's file filter that hides the selected file stops the
    /// window's reads (`invalidate_read`), a write's running quiet reload
    /// with them, and starts that reload again. Nothing else started it: it
    /// waited for the next local change, every menu that follows writes
    /// refused meanwhile.
    #[gpui::test]
    async fn a_file_filter_hiding_the_selection_runs_a_writes_reload_again(
        cx: &mut TestAppContext,
    ) {
        let (fixture, app, cx) = clean_history_for_menus(cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        writes_reload_waits(&app, cx).await;
        assert!(
            app.read_with(cx, |app, _| app.mode == WorkspaceMode::History
                && app.selected_file == Some(0)),
            "the commit's file is highlighted in History"
        );
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.file_filter
                    .update(cx, |input, cx| input.set_value("absent", window, cx));
                app.change_file_filter(window, cx);
            })
        });
        filter_lands_as_the_reload_runs(&app, cx);
        assert!(
            app.read_with(cx, |app, _| app.selected_file.is_none()
                && !app.file_paths.pending),
            "the filter landed and hid the file"
        );
        quiet_reads_land(&app, cx);
        assert!(
            !app.read_with(cx, |app, _| app.write_unsettled()),
            "the reload ran again and landed, and menus are offered"
        );
        drop(fixture);
    }

    /// A read the worker drops unanswered, with no newer read in its place,
    /// ends all the same: the window stops loading and starts the quiet
    /// reads it held off, a write's reload among them. Left loading, it held
    /// that reload off until another read ran, every menu that follows
    /// writes refused meanwhile.
    #[gpui::test]
    async fn a_read_dropped_unanswered_runs_a_writes_reload(cx: &mut TestAppContext) {
        let (fixture, app, cx) = clean_history_for_menus(cx).await;
        writes_reload_waits(&app, cx).await;
        let reader = app.read_with(cx, |app, _| app.worker.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.automatic.hold_quiet_reads(false);
                app.select_commit(1, window, cx);
            })
        });
        assert!(
            app.read_with(cx, |app, _| app.loading.is_some()
                && !app.automatic.quiet_read_running()),
            "the selection's read waits on the held reader, and the reload waits for it"
        );
        // Dropped unanswered, as a cancel that leaves the read's generation
        // as it was drops it.
        app.read_with(cx, |app, _| app.worker.cancel());
        drop(reader);
        cx.executor().run_until_parked();
        assert!(
            app.read_with(cx, |app, _| app.loading.is_none()),
            "the dropped read ended"
        );
        quiet_reads_land(&app, cx);
        assert!(
            !app.read_with(cx, |app, _| app.write_unsettled()),
            "the reload ran and landed, and menus are offered"
        );
        drop(fixture);
    }

    /// Shift-F10 in an open right-click menu, on a History commit, a
    /// navigator branch or a working row, opens no keyboard's menu beside
    /// it: one menu is open at a time, and the right-click menu, the row's
    /// already, keeps the keys and stays until Escape returns focus to its
    /// list or tree. Drawn inside its row, the menu passed the key on to the
    /// list or tree, which opened its cursor row's menu and focused it; the
    /// next frame the right-click menu took focus back and the keyboard's
    /// menu closed, both drawn for that frame.
    #[gpui::test]
    async fn shift_f10_in_a_right_click_menu_opens_no_second_menu(cx: &mut TestAppContext) {
        let (fixture, app, cx) = history_window_from(cx, history_script(40, 3)).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        cx.update(|_, cx| {
            crate::shortcuts::bind_keys(cx);
            crate::native_accessibility::bind_keys(cx);
        });
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        fixture_git(&repository, &["reset", "-q", "--hard"]);
        refreshed(&app, cx).await;
        let branch = app.read_with(cx, |app, _| {
            app.nav_rows
                .iter()
                .position(|row| matches!(row, crate::NavRow::Branch(..)))
                .expect("a branch in the navigator")
        });
        // The navigator's cursor on it, where its keyboard menu would open.
        app.update(cx, |app, _| app.nav_cursor = Some(branch));
        let branch: &'static str = format!("nav-row-{branch}").leak();
        // Whether the navigator's, History's or a file list's keyboard menu
        // opened at all: a test window draws as each key's work settles, so
        // one that closed in the frame after it would be gone by then.
        let opened = std::rc::Rc::new(std::cell::Cell::new(false));
        let _opened = cx.update(|_, cx| {
            let opened = opened.clone();
            cx.observe(&app, move |app, cx| {
                let app = app.read(cx);
                if app.nav_menu.is_some() || app.commit_menu.is_some() || app.file_menu.is_some() {
                    opened.set(true);
                }
            })
        });
        let (history, navigator, files) = app.read_with(cx, |app, _| {
            (
                app.focus.clone(),
                app.nav_focus.clone(),
                app.file_focus.clone(),
            )
        });
        let rows = [
            ("history-row-0", "Revert this commit…", history),
            (branch, "Branch actions…", navigator),
            ("working", "Delete untracked file…", files),
        ];
        for (row, item, list) in rows {
            let row = if row == "working" {
                // An untracked file in Changes.
                std::fs::write(repository.join("extra.txt"), "extra\n").unwrap();
                cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
                rows_arrived(&app, cx).await;
                draw(cx);
                let position = position_of(&app, cx, "extra.txt", ChangeArea::Unstaged);
                let row: &'static str = format!("working-row-{position}").leak();
                // Selected, where its keyboard menu would open.
                let bounds = cx.debug_bounds(row).expect(row);
                cx.simulate_click(bounds.center(), Modifiers::default());
                row
            } else {
                row
            };
            let focused =
                |cx: &mut VisualTestContext| cx.update(|window, _| list.is_focused(window));
            cx.update(|window, cx| list.focus(window, cx));
            next_frame(cx);
            right_click(cx, row);
            assert_eq!(offered(cx, item), Some(true), "{row}: its menu opened");
            assert!(!focused(cx), "{row}: the menu holds focus");
            opened.set(false);
            cx.simulate_keystrokes("shift-f10");
            next_frame(cx);
            assert!(!opened.get(), "{row}: Shift-F10 opened no keyboard's menu");
            assert_eq!(
                offered(cx, item),
                Some(true),
                "{row}: the right-click menu stays"
            );
            assert!(!focused(cx), "{row}: the menu keeps the keys");
            cx.simulate_keystrokes("escape");
            cx.executor().run_until_parked();
            next_frame(cx);
            assert_eq!(offered(cx, item), None, "{row}: Escape closed the menu");
            assert!(focused(cx), "{row}: focus is back on its list or tree");
        }
        drop(fixture);
    }

    /// A right-click menu, once closed, is let go with its observers, by
    /// Escape or by a resize: a write that starts and ends after it builds
    /// it again no more. Kept with its row, a closed menu was built again on
    /// every write's start and end, and dismissed again on every resize,
    /// until its row was right-clicked again or stopped being drawn.
    #[gpui::test]
    async fn a_closed_right_click_menu_is_let_go(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = six_files(cx).await;
        for closing in ["Escape", "a resize"] {
            right_click(cx, "working-row-4");
            let menu = cx
                .update(|_, cx| crate::native_accessibility::open_pointer_menus(cx))
                .pop()
                .unwrap_or_else(|| panic!("{closing}: the open menu"));
            // How often the menu is built again, held weakly.
            let built = std::rc::Rc::new(std::cell::Cell::new(0));
            let _built = cx.update(|_, cx| {
                let built = built.clone();
                cx.observe(&menu.upgrade().expect("the menu"), move |_, _| {
                    built.set(built.get() + 1)
                })
            });
            if closing == "Escape" {
                cx.simulate_keystrokes("escape");
            } else {
                let viewport = cx.update(|window, _| window.viewport_size());
                cx.simulate_resize(size(viewport.width - px(40.), viewport.height - px(40.)));
            }
            cx.executor().run_until_parked();
            next_frame(cx);
            assert_eq!(
                offered(cx, "Delete untracked file…"),
                None,
                "{closing}: closed"
            );
            assert!(
                menu.upgrade().is_none(),
                "{closing}: the closed menu is let go"
            );
            built.set(0);
            let held = held_write(&app, cx);
            next_frame(cx);
            release(&app, held, cx).await;
            next_frame(cx);
            assert_eq!(
                built.get(),
                0,
                "{closing}: a write after it builds the closed menu again no more"
            );
            assert!(
                cx.update(|_, cx| crate::native_accessibility::open_pointer_menus(cx))
                    .is_empty(),
                "{closing}: no menu is recorded open"
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
