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
        self.working_travel = RowTravel::Idle;
        if cx.reduce_motion() {
            return;
        }
        let (Some(repository), Some(&workspace::WorkingRow::File(index, from))) =
            (self.path.clone(), self.working_rows.get(position))
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
