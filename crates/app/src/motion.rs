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
//! the rows it passes and under the list's clip, from where it was towards
//! that slot. Under reduced motion nothing departs, so the row is in its
//! slot in the first frame.

use crate::*;
use gitturtle_core::{ChangeArea, RepositoryStatus};
use std::{cell::Cell, path::Path, rc::Rc, time::Duration};

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
/// scroll the list applied in that frame.
#[derive(Clone, Default)]
pub(crate) struct Slot(Rc<Cell<Option<SlotBounds>>>);

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
            slot.0.set(Some(SlotBounds {
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
    /// and no tooltip until the pointer moves.
    pub(super) fn rows_moved_under_pointer(&mut self) {
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

/// Draws a travelling row over the list, from where it departed to the slot
/// the list laid out for it, under the list's clip. It takes no room of its
/// own. Where the list did not lay the slot out because it lies outside the
/// list's viewport, nothing is drawn and no further frame is asked for.
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
    /// The list's clip, where the row was laid out.
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
        let SlotBounds { slot, clip } = self.flight.slot.0.get()?;
        let departed = self.flight.departed;
        let top = departed + (slot.origin.y - departed) * self.flight.eased;
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
    /// Cancel, "Refreshing…", the tab's "· working", or Stage all or Unstage
    /// all, which
    /// stay available through the writes below, in the disabled colour.
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
        drawn
    }

    /// A row's Stage that lands with its rows within the busy look's delay
    /// never looks busy: in every frame from the press to the landing no
    /// progress label or Cancel is drawn, the tab does not read "· working"
    /// and Stage all and Unstage all keep their resting colour, where the
    /// write once dimmed every control for a frame. Acceptance does not
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
        let frame = |cx: &mut VisualTestContext| {
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
                "Unstage all"
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

    /// After gamma's Stage moves its row, epsilon's row takes its place under
    /// the pointer, which has not moved: epsilon's Stage draws no hover fill
    /// and shows no tooltip, as it did while the person never moved to it.
    /// Once the pointer moves a pixel, it lights up and its tooltip shows.
    #[gpui::test]
    async fn a_still_pointer_lights_nothing_the_rows_moved_under_it(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = six_files(cx).await;
        let (_, palette) = crate::appearance::test_palettes().remove(0);
        cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
        // Tall enough that every row is in view.
        cx.simulate_resize(size(px(1480.), px(1300.)));
        draw(cx);
        let repository = app.read_with(cx, |app, _| app.path.clone());
        let tooltip: &'static str = format!(
            "{}/tooltip/rest",
            workspace::confirmations::file(
                &repository,
                Path::new("epsilon.txt"),
                ChangeArea::Unstaged
            )
            .control()
        )
        .leak();
        let fills = |cx: &mut VisualTestContext| {
            let control = cx.debug_bounds("working-stage-6").expect("row 6's Stage");
            crate::appearance::painted_button(cx, control).0
        };
        // [Staged 2, alpha, beta, Unstaged 4, delta, epsilon, gamma, zeta]
        assert_eq!(position_of(&app, cx, "gamma.txt", ChangeArea::Unstaged), 6);
        let pointer = cx.debug_bounds("working-stage-6").unwrap().center();
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
        assert_eq!(fills(cx), [], "no hover under the still pointer");
        advance(cx, 1000);
        assert!(cx.debug_bounds(tooltip).is_none(), "no tooltip");

        cx.simulate_mouse_move(pointer + point(px(1.), px(0.)), None, Modifiers::default());
        draw(cx);
        assert_eq!(fills(cx).len(), 1, "the moved pointer lights it up");
        advance(cx, 1000);
        assert!(cx.debug_bounds(tooltip).is_some(), "and shows its tooltip");
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
