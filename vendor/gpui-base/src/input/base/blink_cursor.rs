use gpui::{Context, Pixels, Task, px};
use instant::Duration;

static INTERVAL: Duration = Duration::from_millis(500);
static PAUSE_DELAY: Duration = Duration::from_millis(300);

// On Windows, Linux, we should use integer to avoid blurry cursor.
#[cfg(not(target_os = "macos"))]
pub(super) const CURSOR_WIDTH: Pixels = px(2.);
#[cfg(target_os = "macos")]
pub(super) const CURSOR_WIDTH: Pixels = px(1.5);

/// To manage the Input cursor blinking.
///
/// It will start blinking with a interval of 500ms.
/// Every loop will notify the view to update the `visible`, and Input will observe this update to touch repaint.
///
/// The input painter will check if this in visible state, then it will draw the cursor.
pub(crate) struct BlinkCursor {
    visible: bool,
    paused: bool,
    epoch: usize,

    _task: Task<()>,
}

impl BlinkCursor {
    pub(crate) fn new() -> Self {
        Self {
            // GitTurtle patch: shown, so the frame that first focuses an
            // input draws its caret (see `start`).
            visible: true,
            paused: false,
            epoch: 0,
            _task: Task::ready(()),
        }
    }

    /// Start the blinking
    ///
    /// GitTurtle patch: the caret shows at once and first blinks off one
    /// interval later. Upstream toggled it, so focus, given back as a frame
    /// is drawn and reported after it, drew a second frame that only showed
    /// or hid the caret, and a programmatic focus, which starts the cursor
    /// and then reports the focus that starts it again, hid it at once.
    pub(crate) fn start(&mut self, cx: &mut Context<Self>) {
        if !self.visible {
            self.visible = true;
            cx.notify();
        }
        // A pause blinks again as it ends, as upstream's start left it to.
        if self.paused {
            return;
        }
        let epoch = self.next_epoch();
        self.schedule(epoch, cx);
    }

    pub(crate) fn stop(&mut self, cx: &mut Context<Self>) {
        self.epoch = 0;
        // GitTurtle patch: shown for the next focus, which draws it in its
        // first frame; an unfocused input draws no caret. A stop also ends
        // a pause, whose own end now finds a later epoch and blinks nothing,
        // so the next start blinks rather than wait for it.
        self.visible = true;
        self.paused = false;
        cx.notify();
    }

    fn next_epoch(&mut self) -> usize {
        self.epoch += 1;
        self.epoch
    }

    fn blink(&mut self, epoch: usize, cx: &mut Context<Self>) {
        if self.paused || epoch != self.epoch {
            self.visible = true;
            return;
        }

        self.visible = !self.visible;
        cx.notify();

        // Schedule the next blink
        let epoch = self.next_epoch();
        self.schedule(epoch, cx);
    }

    /// Blinks one interval from now, unless another epoch has begun.
    fn schedule(&mut self, epoch: usize, cx: &mut Context<Self>) {
        self._task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(INTERVAL).await;
            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| this.blink(epoch, cx));
            }
        });
    }

    pub(crate) fn visible(&self) -> bool {
        // Keep showing the cursor if paused
        self.paused || self.visible
    }

    /// Whether the cursor blinks: started, and not stopped since. A new
    /// cursor and a stopped one are at epoch 0, which every start, pause and
    /// blink leaves.
    fn started(&self) -> bool {
        self.epoch != 0
    }

    /// Pause the blinking, and delay 500ms to resume the blinking.
    pub(crate) fn pause(&mut self, cx: &mut Context<Self>) {
        // GitTurtle patch: a cursor that is not blinking, a new one or one
        // stopped on blur, pauses nothing and schedules nothing, so text set
        // in an unfocused field never starts it blinking. With the caret left
        // shown for the next focus, the shown caret's pause of an edit
        // (`replace_text_in_range`) once started an unfocused field blinking
        // for good, redrawing the window twice a second while idle.
        if !self.started() {
            return;
        }
        self.paused = true;
        self.visible = true;
        cx.notify();

        // delay 500ms to start the blinking
        let epoch = self.next_epoch();
        self._task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PAUSE_DELAY).await;

            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| {
                    this.paused = false;
                    this.blink(epoch, cx);
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, TestAppContext};
    use std::{cell::Cell, rc::Rc};

    /// GitTurtle patch: a start shows the caret at once, notifying only if
    /// it was hidden, and blinks it off one interval later; a second start,
    /// as a programmatic focus and the focus it reports make, keeps it shown
    /// and notifies nothing; a stop leaves it shown for the next focus; and
    /// a start during a pause leaves the pause to resume the blinking.
    #[gpui::test]
    fn a_start_shows_the_caret_at_once_and_blinks_an_interval_later(cx: &mut TestAppContext) {
        let cursor = cx.new(|_| BlinkCursor::new());
        let notified = Rc::new(Cell::new(0));
        let counted = notified.clone();
        cx.update(|cx| {
            cx.observe(&cursor, move |_, _| counted.set(counted.get() + 1))
                .detach()
        });
        let visible = |cx: &mut TestAppContext| cursor.read_with(cx, |cursor, _| cursor.visible());
        let wait = |cx: &mut TestAppContext, time: Duration| {
            cx.executor().advance_clock(time);
            cx.run_until_parked();
        };
        assert!(visible(cx), "shown before the first focus");
        for start in ["the first start", "a second start"] {
            cursor.update(cx, |cursor, cx| cursor.start(cx));
            cx.run_until_parked();
            assert!(visible(cx), "{start} shows the caret");
            assert_eq!(
                notified.get(),
                0,
                "{start} of a shown caret notifies nothing"
            );
        }
        wait(cx, INTERVAL - Duration::from_millis(1));
        assert!(visible(cx), "shown until an interval has passed");
        wait(cx, Duration::from_millis(1));
        assert!(!visible(cx), "blinks off an interval after the start");
        wait(cx, INTERVAL);
        assert!(visible(cx), "and on again an interval later");
        wait(cx, INTERVAL);
        assert!(!visible(cx), "and off again, blinking on");
        cursor.update(cx, |cursor, cx| cursor.stop(cx));
        assert!(
            visible(cx),
            "a stop leaves the caret shown for the next focus"
        );
        let before = notified.get();
        cursor.update(cx, |cursor, cx| cursor.start(cx));
        cx.run_until_parked();
        assert!(visible(cx), "the next start shows it");
        assert_eq!(notified.get(), before, "without a notify of its own");
        // Typing pauses the blink; a start meanwhile leaves the pause to end
        // it, after which the caret blinks again.
        cursor.update(cx, |cursor, cx| cursor.pause(cx));
        cursor.update(cx, |cursor, cx| cursor.start(cx));
        wait(cx, PAUSE_DELAY - Duration::from_millis(1));
        assert!(visible(cx), "shown through the pause");
        wait(cx, Duration::from_millis(1));
        assert!(!visible(cx), "blinks off as the pause ends");
        wait(cx, INTERVAL);
        assert!(visible(cx), "and blinks on");
        // An edit pauses the blink just before a blur stops it, as a warm
        // tab's switch sets a focused field's text: the next start blinks.
        cursor.update(cx, |cursor, cx| {
            cursor.pause(cx);
            cursor.stop(cx);
            cursor.start(cx);
        });
        wait(cx, INTERVAL);
        assert!(!visible(cx), "a start after a stopped pause blinks off");
        wait(cx, INTERVAL);
        assert!(visible(cx), "and on again");
    }

    /// GitTurtle patch: a cursor that is not blinking, a new one or one
    /// stopped on blur, pauses nothing: text set in an unfocused field, which
    /// pauses a shown caret, notifies nothing and schedules no blink.
    #[gpui::test]
    fn a_cursor_not_blinking_pauses_nothing(cx: &mut TestAppContext) {
        let fresh = cx.new(|_| BlinkCursor::new());
        let stopped = cx.new(|_| BlinkCursor::new());
        stopped.update(cx, |cursor, cx| {
            cursor.start(cx);
            cursor.stop(cx);
        });
        cx.run_until_parked();
        for (cursor, which) in [(fresh, "a new cursor"), (stopped, "a stopped cursor")] {
            let notified = Rc::new(Cell::new(0));
            let counted = notified.clone();
            cx.update(|cx| {
                cx.observe(&cursor, move |_, _| counted.set(counted.get() + 1))
                    .detach()
            });
            cursor.update(cx, |cursor, cx| cursor.pause(cx));
            cx.executor().advance_clock(Duration::from_secs(5));
            cx.run_until_parked();
            assert_eq!(notified.get(), 0, "{which} notifies nothing for 5 s");
            assert!(
                cursor.read_with(cx, |cursor, _| cursor.visible()),
                "{which} keeps its caret shown for the next focus"
            );
        }
    }
}
