//! A list's keyboard focus ring, kept while the pointer moves.
//!
//! GPUI's `focus_visible` styles a focused element only while the last input
//! was a key, so merely moving the pointer took the ring off a list that kept
//! keyboard focus. Here a key press shows the ring of whatever holds focus,
//! and the ring stays whatever the pointer does until a pointer press, as GTK
//! and browsers' `:focus-visible` behave: a click still focuses a list without
//! marking it, and the next key marks it.
use crate::*;
use std::{cell::Cell, rc::Rc};

/// One window's keyboard focus ring, owned by its [`GitTurtle`].
#[derive(Default)]
pub(crate) struct FocusRing {
    /// A key was pressed after the last pointer press. Shared with the
    /// window's pointer-press listener, [`FocusRing::pointer_listener`].
    keyboard: Rc<Cell<bool>>,
    /// The focus whose ring the frame being built draws.
    ringed: Option<FocusHandle>,
}

impl FocusRing {
    /// Reads, before a frame builds its lists, which focus draws its ring.
    pub(crate) fn begin_frame(&mut self, window: &Window, cx: &App) {
        // GPUI marks every key press as keyboard input and redraws the
        // window when input turns from the pointer to keys, so a frame
        // follows each press that shows the ring.
        if window.last_input_was_keyboard() {
            self.keyboard.set(true);
        }
        self.ringed = self.keyboard.get().then(|| window.focused(cx)).flatten();
    }

    /// Whether `focus` draws its keyboard ring this frame.
    pub(crate) fn shows(&self, focus: &FocusHandle) -> bool {
        self.ringed.as_ref() == Some(focus)
    }

    /// An empty element whose paint ends the ring at any pointer press in the
    /// window, over a dialog's backdrop included: GPUI calls a window's mouse
    /// listeners for every press, whatever lies under the pointer.
    pub(crate) fn pointer_listener(&self) -> impl IntoElement {
        let keyboard = self.keyboard.clone();
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let keyboard = keyboard.clone();
                window.on_mouse_event(move |_: &MouseDownEvent, phase, window, _| {
                    if phase == DispatchPhase::Capture && keyboard.replace(false) {
                        window.refresh();
                    }
                });
            },
        )
        .absolute()
        .size_0()
    }
}
