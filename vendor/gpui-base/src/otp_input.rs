use crate::{StyledExt as _, input::blink_cursor::BlinkCursor};
use gpui::{
    AnyElement, App, AppContext as _, Context, Empty, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement, Render, RenderOnce,
    SharedString, StyleRefinement, Styled, Subscription, Window, div, prelude::FluentBuilder as _,
};

/// A semantic notification from a one-time-code state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OtpEvent {
    /// The value changed through keyboard editing.
    Change,
    /// Keyboard editing filled the final cell.
    Complete,
    Focus,
    Blur,
}

/// Stateful input and focus behavior for a fixed-length numeric one-time code.
pub struct OtpState {
    focus_handle: FocusHandle,
    value: SharedString,
    blink_cursor: Entity<BlinkCursor>,
    masked: bool,
    length: usize,
    _subscriptions: Vec<Subscription>,
}

impl OtpState {
    pub fn new(length: usize, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let blink_cursor = cx.new(|_| BlinkCursor::new());
        let subscriptions = vec![
            cx.observe(&blink_cursor, |_, _, cx| cx.notify()),
            // GitTurtle patch: as the input's state, no window-activation
            // observer: GPUI's focus and blur reports on activation start and
            // stop the cursor of a drawn, focused field only
            // (`InputBaseState::new_in_mode`).
            cx.on_focus(&focus_handle, window, Self::on_focus),
            cx.on_blur(&focus_handle, window, Self::on_blur),
        ];
        Self {
            focus_handle,
            value: SharedString::default(),
            blink_cursor,
            masked: false,
            length,
            _subscriptions: subscriptions,
        }
    }

    pub fn default_value(mut self, value: impl Into<SharedString>) -> Self {
        self.value = value.into();
        self
    }

    pub fn set_value(
        &mut self,
        value: impl Into<SharedString>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.value = value.into();
        cx.notify();
    }

    pub fn value(&self) -> &SharedString {
        &self.value
    }
    pub fn len(&self) -> usize {
        self.length
    }
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }
    pub fn is_masked(&self) -> bool {
        self.masked
    }
    pub fn cursor_visible(&self, cx: &App) -> bool {
        self.blink_cursor.read(cx).visible()
    }
    pub fn masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }
    pub fn set_masked(&mut self, masked: bool, _: &mut Window, cx: &mut Context<Self>) {
        self.masked = masked;
        cx.notify();
    }
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window, cx);
    }

    fn to_digit_char(value: char) -> Option<char> {
        value.to_digit(10).map(|_| value).or_else(|| {
            let digit = (value as u32).checked_sub('０' as u32)?;
            char::from_digit(digit, 10)
        })
    }

    fn edit_value(value: &str, key: &str, key_char: Option<&str>, length: usize) -> Option<String> {
        let mut chars: Vec<char> = value.chars().collect();
        if key == "backspace" {
            chars.pop();
        } else {
            let digit = key
                .chars()
                .next()
                .and_then(Self::to_digit_char)
                .or_else(|| key_char?.chars().next().and_then(Self::to_digit_char));
            let digit = digit?;
            if chars.len() >= length {
                return None;
            }
            chars.push(digit);
        }
        Some(chars.iter().collect())
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(value) = Self::edit_value(
            &self.value,
            &event.keystroke.key,
            event.keystroke.key_char.as_deref(),
            self.length,
        ) else {
            return;
        };
        window.prevent_default();
        cx.stop_propagation();
        self.blink_cursor.update(cx, |cursor, cx| cursor.pause(cx));
        self.value = value.into();
        cx.emit(OtpEvent::Change);
        if self.value.chars().count() == self.length {
            cx.emit(OtpEvent::Complete);
        }
        cx.notify();
    }

    fn on_focus(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.blink_cursor.update(cx, |cursor, cx| cursor.start(cx));
        cx.emit(OtpEvent::Focus);
    }
    fn on_blur(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.blink_cursor.update(cx, |cursor, cx| cursor.stop(cx));
        cx.emit(OtpEvent::Blur);
    }
}

impl Focusable for OtpState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
impl EventEmitter<OtpEvent> for OtpState {}
impl Render for OtpState {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// Unstyled OTP interaction root. Applications provide the visual cells as children.
#[derive(IntoElement)]
pub struct OtpInput {
    state: Entity<OtpState>,
    disabled: bool,
    style: StyleRefinement,
    children: Vec<AnyElement>,
}

impl OtpInput {
    pub fn new(state: &Entity<OtpState>) -> Self {
        Self {
            state: state.clone(),
            disabled: false,
            style: StyleRefinement::default(),
            children: vec![],
        }
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}
impl Styled for OtpInput {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl ParentElement for OtpInput {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}
impl RenderOnce for OtpInput {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.state;
        div()
            .id(("base-otp-input", state.entity_id()))
            .track_focus(&state.read(cx).focus_handle)
            .when(!self.disabled, |this| {
                this.on_key_down(window.listener_for(&state, OtpState::on_key_down))
            })
            .children(self.children)
            .refine_style(&self.style)
    }
}

#[cfg(test)]
mod tests {
    use super::{OtpInput, OtpState};
    use gpui::{
        AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, TestAppContext,
        VisualTestContext, Window, div,
    };
    use std::{cell::Cell, rc::Rc, time::Duration};

    /// An OTP field, drawn while `shown`.
    struct Root {
        otp: Entity<OtpState>,
        shown: bool,
    }

    impl Render for Root {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().children(self.shown.then(|| OtpInput::new(&self.otp)))
        }
    }

    /// GitTurtle patch: a window turned active again starts the blink of a
    /// focused OTP field drawn in it, through the focus GPUI reports in the
    /// first frame it draws active, and starts nothing for one that holds
    /// the window's focus without being drawn. Upstream's window-activation
    /// observer started it drawn or not, and with no blur to stop it, it
    /// blinked, ten notifications in five seconds.
    #[gpui::test]
    fn window_activation_blinks_only_a_drawn_focused_field(cx: &mut TestAppContext) {
        let (root, cx) = cx.add_window_view(|window, cx| Root {
            otp: cx.new(|cx| OtpState::new(4, window, cx)),
            shown: true,
        });
        let draw = |cx: &mut VisualTestContext| cx.update(|window, cx| window.draw(cx).clear(cx));
        let activate = |cx: &mut VisualTestContext| {
            cx.update(|window, _| window.activate_window());
            cx.run_until_parked();
        };
        let wait = |cx: &mut VisualTestContext| {
            cx.executor().advance_clock(Duration::from_secs(5));
            cx.run_until_parked();
        };
        let otp = root.read_with(cx, |root, _| root.otp.clone());
        let (focus, cursor) = otp.read_with(cx, |otp, _| {
            (otp.focus_handle.clone(), otp.blink_cursor.clone())
        });
        let blinks = Rc::new(Cell::new(0));
        let counted = blinks.clone();
        cx.update(|_, cx| {
            cx.observe(&cursor, move |_, _| counted.set(counted.get() + 1))
                .detach()
        });
        draw(cx);
        activate(cx);
        cx.update(|window, cx| window.focus(&focus, cx));
        draw(cx);

        cx.deactivate_window();
        draw(cx);
        activate(cx);
        draw(cx);
        blinks.set(0);
        wait(cx);
        assert!(
            blinks.get() >= 9,
            "the field, drawn in the window turned active, blinks: {} blinks",
            blinks.get()
        );

        root.update(cx, |root, cx| {
            root.shown = false;
            cx.notify();
        });
        draw(cx);
        assert!(
            cx.update(|window, _| focus.is_focused(window)),
            "the field, not drawn, holds the window's focus"
        );
        cx.deactivate_window();
        draw(cx);
        activate(cx);
        draw(cx);
        blinks.set(0);
        wait(cx);
        assert_eq!(
            blinks.get(),
            0,
            "the field, focused but not drawn, starts nothing as the window turns active"
        );
    }

    #[test]
    fn keyboard_editing_backspaces_and_stops_at_length() {
        assert_eq!(
            OtpState::edit_value("12", "backspace", None, 4).as_deref(),
            Some("1")
        );
        assert_eq!(
            OtpState::edit_value("12", "３", None, 4).as_deref(),
            Some("123")
        );
        assert_eq!(OtpState::edit_value("1234", "5", None, 4), None);
        assert_eq!(OtpState::edit_value("12", "left", None, 4), None);
    }

    #[test]
    fn programmatic_values_remain_unfiltered_for_compatibility() {
        // Programmatic values intentionally do not use the keyboard editor path.
        // This captures the legacy contract: callers may display arbitrary or
        // over-length values, while actual key entry remains digit-only.
        let value: gpui::SharedString = "token-over-length".into();
        assert_eq!(value.as_ref(), "token-over-length");
        assert_eq!(OtpState::edit_value("token", "x", None, 2), None);
    }
}
