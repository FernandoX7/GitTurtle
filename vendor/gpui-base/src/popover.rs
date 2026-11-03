use std::{rc::Rc, sync::LazyLock};

use gpui::{
    Anchor, AnyElement, App, Context, DismissEvent, ElementId, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, KeyBinding, KeyContext, KeyDownEvent,
    KeybindingKeystroke, Keystroke, MouseButton, NoAction, ParentElement as _, Pixels, Render,
    RenderOnce, Role, StatefulInteractiveElement as _, Subscription, Window, div,
    prelude::FluentBuilder as _,
};

use crate::{
    DeferredPopover, GlobalState, Popup, Selectable,
    actions::{Cancel, Confirm},
};

const CONTEXT: &str = "Popover";

/// GitTurtle patch: the key context a popover's trigger has beside
/// [`CONTEXT`], which its content has alone.
const TRIGGER_CONTEXT: &str = "PopoverTrigger";

/// GitTurtle patch: the keys that open or close a popover from its trigger.
/// [`CONTEXT`] binds them to [`Confirm`], in the trigger and the content
/// alike; in the trigger they are also bound to [`NoAction`], after it,
/// which outranks it at the same depth, so their key-down reaches the
/// trigger's listener ([`open_key`]) with its repeat flag instead.
const OPEN_KEYS: [&str; 2] = ["enter", "space"];

/// GitTurtle patch: [`OPEN_KEYS`] parsed once, as [`KeyBinding::new`]
/// parses them, for [`open_key`] to match a key-down against as their
/// bindings match it.
static OPEN_KEYSTROKES: LazyLock<[KeybindingKeystroke; 2]> = LazyLock::new(|| {
    OPEN_KEYS.map(|keys| {
        KeybindingKeystroke::from_keystroke(Keystroke::parse(keys).expect("a popover's open key"))
    })
});

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", Cancel, Some(CONTEXT)),
        KeyBinding::new("enter", Confirm { secondary: false }, Some(CONTEXT)),
        KeyBinding::new("space", Confirm { secondary: false }, Some(CONTEXT)),
    ]);
    cx.bind_keys(OPEN_KEYS.map(|keys| KeyBinding::new(keys, NoAction, Some(TRIGGER_CONTEXT))));
}

/// GitTurtle patch: a popover's trigger opens or closes it with Enter or
/// Space only as the key goes down, never on a held key's repeats, as a
/// menu confirms only on a fresh press. Its key-down, which no binding takes
/// in the trigger ([`OPEN_KEYS`]), dispatches the [`Confirm`] the binding
/// did, on the focused element, so whatever an application captures of it
/// still runs, while an auto-repeat, whose `is_held` is set, does nothing;
/// either way the key goes no further, as the binding took it. It is taken
/// in the capture phase, ahead of anything inside the trigger, which the
/// binding kept it from too. A key that goes down in the content, whose
/// innermost popover context is not a trigger's, passes on to the content's
/// own binding, as before. Bound to [`Confirm`], a still-held Enter or Space
/// that had chosen a dropdown's item reopened its menu once focus returned
/// to the button. Under X11, and on Wayland with an input method, a held key
/// arrives as fresh presses, which this cannot tell apart
/// (`GITTURTLE-PATCH.md`).
fn open_key(event: &KeyDownEvent, window: &mut Window, cx: &mut App) {
    let in_trigger = OPEN_KEYSTROKES
        .iter()
        .any(|keystroke| event.keystroke.should_match(keystroke))
        && window
            .context_stack()
            .iter()
            .rfind(|context| context.contains(CONTEXT))
            .is_some_and(|context| context.contains(TRIGGER_CONTEXT));
    if !in_trigger {
        return;
    }
    if !event.is_held
        && let Some(focused) = window.focused(cx)
    {
        focused.dispatch_action(&Confirm { secondary: false }, window, cx);
    }
    cx.stop_propagation();
}

/// GitTurtle patch: a trigger's key context, [`CONTEXT`] as before and
/// [`TRIGGER_CONTEXT`] beside it.
fn trigger_context() -> KeyContext {
    let mut context = KeyContext::default();
    context.add(CONTEXT);
    context.add(TRIGGER_CONTEXT);
    context
}

type OpenChangeHandler = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

/// State and focus lifecycle for an unstyled popover.
///
/// Applications own trigger/content construction, positioning, appearance, and
/// animation. This state owns the controlled open lifecycle, dismissal, focus
/// capture and restoration, and deferred-popup registration.
pub struct PopoverState {
    focus_handle: FocusHandle,
    tracked_focus_handle: Option<FocusHandle>,
    previous_focus_handle: Option<FocusHandle>,
    open: bool,
    on_open_change: Option<OpenChangeHandler>,
    dismiss_subscription: Option<Subscription>,
    /// Held while open, so that a state collected without being closed — this
    /// lives in element state — takes its registration with it.
    deferred_context: Option<DeferredPopover>,
}

impl PopoverState {
    pub fn new(default_open: bool, cx: &mut App) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            tracked_focus_handle: None,
            previous_focus_handle: None,
            open: default_open,
            on_open_change: None,
            dismiss_subscription: None,
            deferred_context: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.toggle_open(window, cx);
        }
    }

    pub fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            self.toggle_open(window, cx);
        }
    }

    #[doc(hidden)]
    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.open = open;
        self.deferred_context = open.then(|| GlobalState::register_deferred_popover(cx));
    }

    #[doc(hidden)]
    pub fn sync_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.transition_to(open, false, window, cx);
    }

    #[doc(hidden)]
    pub fn toggle_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.transition_to(!self.open, true, window, cx);
    }

    fn transition_to(
        &mut self,
        opening: bool,
        announce: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.open == opening {
            return;
        }
        if opening {
            self.previous_focus_handle = window.focused(cx);
        }
        self.set_open(opening, cx);

        if self.open {
            self.tracked_focus_handle
                .clone()
                .unwrap_or_else(|| self.focus_handle.clone())
                .focus(window, cx);

            // GitTurtle patch: the callback dismisses the state the
            // subscription hands it, which the window upgrades from a weak
            // handle as each event arrives. Upstream it captured
            // `cx.entity()`, a strong handle that the app's subscriber set
            // keeps until this subscription, which the state holds, drops: a
            // popover taken out of the tree while open kept its state, and
            // the deferred-popup registration the state holds, for the app's
            // life: the subscriber set and that registry are the app's, and
            // closing the popover's window clears neither.
            self.dismiss_subscription =
                Some(
                    window.subscribe(&cx.entity(), cx, |state, _: &DismissEvent, window, cx| {
                        state.update(cx, |state, cx| state.dismiss(window, cx));
                        window.refresh();
                    }),
                );
        } else {
            self.dismiss_subscription = None;
            if let Some(previous) = self.previous_focus_handle.take() {
                if self.focus_handle.contains_focused(window, cx) {
                    previous.focus(window, cx);
                }
            }
        }

        if announce && let Some(callback) = self.on_open_change.as_ref() {
            callback(&opening, window, cx);
        }
        cx.notify();
    }

    #[doc(hidden)]
    pub fn track_focus(&mut self, focus_handle: Option<FocusHandle>) {
        self.tracked_focus_handle = focus_handle;
    }

    #[doc(hidden)]
    pub fn set_on_open_change(&mut self, handler: Option<OpenChangeHandler>) {
        self.on_open_change = handler;
    }

    #[doc(hidden)]
    pub fn on_action_cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss(window, cx);
    }
}

impl Focusable for PopoverState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PopoverState {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl EventEmitter<DismissEvent> for PopoverState {}

type TriggerBuilder = Box<dyn FnOnce(bool, &Window, &App) -> AnyElement>;
type ContentBuilder =
    Box<dyn FnOnce(&mut PopoverState, &mut Window, &mut Context<PopoverState>) -> AnyElement>;

/// An unstyled popover owning trigger interaction, open state, dismissal, and focus.
#[derive(IntoElement)]
pub struct Popover {
    id: ElementId,
    anchor: Anchor,
    default_open: bool,
    open: Option<bool>,
    tracked_focus_handle: Option<FocusHandle>,
    trigger: Option<TriggerBuilder>,
    content: Option<ContentBuilder>,
    mouse_button: MouseButton,
    overlay_closable: bool,
    on_open_change: Option<OpenChangeHandler>,
    /// GitTurtle patch: the popup host's window margin, where the caller
    /// sets one ([`Popover::margin`]).
    margin: Option<Pixels>,
}

impl Popover {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            anchor: Anchor::TopLeft,
            default_open: false,
            open: None,
            tracked_focus_handle: None,
            trigger: None,
            content: None,
            mouse_button: MouseButton::Left,
            overlay_closable: true,
            on_open_change: None,
            margin: None,
        }
    }

    pub fn anchor(mut self, anchor: impl Into<Anchor>) -> Self {
        self.anchor = anchor.into();
        self
    }

    /// GitTurtle patch: keeps the surface `margin` inside the window, in
    /// place of the popup host's 8 px ([`Popup::margin`]).
    pub fn margin(mut self, margin: Pixels) -> Self {
        self.margin = Some(margin);
        self
    }

    pub fn mouse_button(mut self, mouse_button: MouseButton) -> Self {
        self.mouse_button = mouse_button;
        self
    }

    pub fn trigger<T>(mut self, trigger: T) -> Self
    where
        T: Selectable + IntoElement + 'static,
    {
        self.trigger = Some(Box::new(|is_open, _, _| {
            let selected = trigger.is_selected();
            trigger.selected(selected || is_open).into_any_element()
        }));
        self
    }

    /// Supplies a trigger builder for higher-level presentation facades.
    #[doc(hidden)]
    pub fn trigger_with(
        mut self,
        trigger: impl FnOnce(bool, &Window, &App) -> AnyElement + 'static,
    ) -> Self {
        self.trigger = Some(Box::new(trigger));
        self
    }

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = Some(open);
        self
    }

    pub fn track_focus(mut self, handle: &FocusHandle) -> Self {
        self.tracked_focus_handle = Some(handle.clone());
        self
    }

    pub fn overlay_closable(mut self, closable: bool) -> Self {
        self.overlay_closable = closable;
        self
    }

    pub fn on_open_change(
        mut self,
        callback: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_open_change = Some(Rc::new(callback));
        self
    }

    pub fn content<F, E>(mut self, content: F) -> Self
    where
        E: IntoElement,
        F: FnOnce(&mut PopoverState, &mut Window, &mut Context<PopoverState>) -> E + 'static,
    {
        self.content = Some(Box::new(move |state, window, cx| {
            content(state, window, cx).into_any_element()
        }));
        self
    }
}

impl RenderOnce for Popover {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, cx| {
            PopoverState::new(self.default_open, cx)
        });
        state.update(cx, |state, cx| {
            state.track_focus(self.tracked_focus_handle);
            state.set_on_open_change(self.on_open_change);
            if let Some(open) = self.open {
                state.sync_open(open, window, cx);
            }
        });

        let open = state.read(cx).is_open();
        let focus_handle = state.read(cx).focus_handle(cx);
        let Some(trigger) = self.trigger else {
            return div().id("empty").into_any_element();
        };
        let parent_view_id = window.current_view();
        let mut popup = Popup::new(self.id, trigger(open, window, cx)).anchor(self.anchor);
        if let Some(margin) = self.margin {
            popup = popup.margin(margin);
        }
        let popup = popup
            .key_context(trigger_context())
            .capture_key_down(open_key)
            .on_action({
                let state = state.clone();
                move |_: &Confirm, window, cx| {
                    state.update(cx, |state, cx| state.toggle_open(window, cx));
                    cx.notify(parent_view_id);
                }
            })
            .on_mouse_down(self.mouse_button, {
                let state = state.clone();
                move |_, window, cx| {
                    cx.stop_propagation();
                    state.update(cx, |state, cx| {
                        if state.is_open() == open {
                            state.toggle_open(window, cx);
                        }
                    });
                    cx.notify(parent_view_id);
                }
            });
        if !open {
            return popup.into_any_element();
        }

        let content = div()
            .id("content")
            // A popover surface is a non-modal dialog: it takes focus and is
            // dismissed with Escape, which is what this role tells assistive
            // technology to expect.
            .role(Role::Dialog)
            .occlude()
            .tab_group()
            .track_focus(&focus_handle)
            .key_context(CONTEXT)
            .on_action(window.listener_for(&state, PopoverState::on_action_cancel))
            .when_some(self.content, |this, content| {
                this.child(state.update(cx, |state, cx| (content)(state, window, cx)))
            })
            .when(self.overlay_closable, |this| {
                this.on_mouse_down_out({
                    let state = state.clone();
                    move |_, window, cx| {
                        state.update(cx, |state, cx| state.dismiss(window, cx));
                        cx.notify(parent_view_id);
                    }
                })
            });
        popup.content(content).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, Context, Render, Styled as _, point, px};
    use std::{cell::RefCell, rc::Rc};

    /// Popover state lives in element state, which is collected as soon as it
    /// stops being rendered — a trigger scrolled out of a virtual list, a panel
    /// closed with its menu open. A registration that outlived it would leave
    /// the application believing a popup is open for the rest of the session.
    #[gpui::test]
    fn a_state_dropped_while_open_closes_the_deferred_context(cx: &mut gpui::TestAppContext) {
        let state = cx.update(|cx| {
            GlobalState::init(cx);
            let state = cx.new(|cx| PopoverState::new(false, cx));
            state.update(cx, |state, cx| state.set_open(true, cx));
            assert!(GlobalState::is_in_deferred_context(cx));
            state
        });

        cx.update(|_| drop(state));
        cx.update(|cx| assert!(!GlobalState::is_in_deferred_context(cx)));
    }

    #[gpui::test]
    fn open_state_registers_and_unregisters_deferred_context(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            GlobalState::init(cx);
            let state = cx.new(|cx| PopoverState::new(false, cx));

            state.update(cx, |state, cx| state.set_open(true, cx));
            assert!(state.read(cx).is_open());
            assert!(GlobalState::is_in_deferred_context(cx));

            state.update(cx, |state, cx| state.set_open(false, cx));
            assert!(!state.read(cx).is_open());
            assert!(!GlobalState::is_in_deferred_context(cx));
        });
    }

    struct PopoverHarness {
        changes: Rc<RefCell<Vec<bool>>>,
        default_open: bool,
    }

    struct KeyboardPopoverHarness {
        trigger_focus: FocusHandle,
    }

    impl Render for KeyboardPopoverHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            Popover::new("keyboard-popover")
                .trigger(
                    crate::Button::new("keyboard-trigger")
                        .track_focus(&self.trigger_focus)
                        .child("Open"),
                )
                .content(|_, _, _| {
                    div()
                        .debug_selector(|| "keyboard-popover-content".into())
                        .size(px(40.))
                })
        }
    }

    impl Render for PopoverHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let changes = self.changes.clone();
            Popover::new("base-popover")
                .default_open(self.default_open)
                .trigger_with(|_, _, _| div().child("Open").into_any_element())
                .content(|_, _, _| {
                    div()
                        .debug_selector(|| "base-popover-content".into())
                        .size(px(40.))
                })
                .on_open_change(move |open, _, _| changes.borrow_mut().push(*open))
        }
    }

    #[gpui::test]
    fn unstyled_popover_owns_pointer_open_and_outside_dismiss(cx: &mut gpui::TestAppContext) {
        cx.update(crate::init);
        let changes = Rc::new(RefCell::new(Vec::new()));
        let (_, cx) = cx.add_window_view({
            let changes = changes.clone();
            move |_, _| PopoverHarness {
                changes,
                default_open: false,
            }
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));

        cx.simulate_click(point(px(20.), px(10.)), Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(cx.debug_bounds("base-popover-content").is_some());

        cx.simulate_click(point(px(300.), px(300.)), Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(cx.debug_bounds("base-popover-content").is_none());
        assert_eq!(&*changes.borrow(), &[true, false]);
    }

    #[gpui::test]
    fn default_open_renders_content_without_activation(cx: &mut gpui::TestAppContext) {
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|_, _| PopoverHarness {
            changes: Rc::new(RefCell::new(Vec::new())),
            default_open: true,
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(cx.debug_bounds("base-popover-content").is_some());
    }

    #[gpui::test]
    fn keyboard_activation_opens_the_popover(cx: &mut gpui::TestAppContext) {
        cx.update(crate::init);
        let (view, cx) = cx.add_window_view(|_, cx| KeyboardPopoverHarness {
            trigger_focus: cx.focus_handle(),
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));

        cx.update(|window, cx| {
            let focus = view.read(cx).trigger_focus.clone();
            focus.focus(window, cx);
        });
        cx.simulate_keystrokes("enter");
        cx.update(|window, cx| window.draw(cx).clear(cx));

        assert!(cx.debug_bounds("keyboard-popover-content").is_some());
    }

    /// A popover whose content holds an item that emits the state's dismiss
    /// event when clicked, as a chosen menu item does.
    struct DismissalHarness {
        trigger_focus: FocusHandle,
        changes: Rc<RefCell<Vec<bool>>>,
    }

    impl Render for DismissalHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let changes = self.changes.clone();
            let trigger_focus = self.trigger_focus.clone();
            Popover::new("dismissal-popover")
                .trigger_with(move |_, _, _| {
                    div()
                        .id("dismissal-trigger")
                        .debug_selector(|| "dismissal-trigger".into())
                        .track_focus(&trigger_focus)
                        .w(px(80.))
                        .h(px(20.))
                        .child("Open")
                        .into_any_element()
                })
                .content(|_, _, cx| {
                    div()
                        .debug_selector(|| "dismissal-content".into())
                        .size(px(40.))
                        .child(
                            div()
                                .id("dismissal-item")
                                .debug_selector(|| "dismissal-item".into())
                                .size(px(20.))
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(DismissEvent))),
                        )
                })
                .on_open_change(move |open, _, _| changes.borrow_mut().push(*open))
        }
    }

    /// GitTurtle patch: the dismiss subscription reaches its state through
    /// the entity it is handed rather than one it captured. Escape, the
    /// trigger pressed again and an item's dismiss event each still close
    /// the popover, and Escape and the item return focus to the trigger it
    /// was opened from; a press outside is
    /// `unstyled_popover_owns_pointer_open_and_outside_dismiss`.
    #[gpui::test]
    fn every_dismissal_still_closes_the_popover(cx: &mut gpui::TestAppContext) {
        cx.update(crate::init);
        let changes = Rc::new(RefCell::new(Vec::new()));
        let (view, cx) = cx.add_window_view({
            let changes = changes.clone();
            move |_, cx| DismissalHarness {
                trigger_focus: cx.focus_handle(),
                changes,
            }
        });
        let draw = |cx: &mut gpui::VisualTestContext| {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        };
        let trigger_focus = view.read_with(cx, |view, _| view.trigger_focus.clone());
        let open_from_the_keyboard = |cx: &mut gpui::VisualTestContext| {
            cx.update(|window, cx| trigger_focus.focus(window, cx));
            cx.simulate_keystrokes("enter");
            draw(cx);
            assert!(
                cx.debug_bounds("dismissal-content").is_some(),
                "Enter opens it"
            );
        };
        let trigger_focused = |cx: &mut gpui::VisualTestContext| {
            cx.update(|window, _| trigger_focus.is_focused(window))
        };
        draw(cx);
        let trigger = cx
            .debug_bounds("dismissal-trigger")
            .expect("the trigger is drawn")
            .center();

        open_from_the_keyboard(cx);
        cx.simulate_keystrokes("escape");
        draw(cx);
        assert!(cx.debug_bounds("dismissal-content").is_none(), "Escape");
        assert!(trigger_focused(cx), "Escape returns focus to the trigger");

        cx.simulate_click(trigger, Default::default());
        draw(cx);
        assert!(
            cx.debug_bounds("dismissal-content").is_some(),
            "a press opens it"
        );
        cx.simulate_click(trigger, Default::default());
        draw(cx);
        assert!(
            cx.debug_bounds("dismissal-content").is_none(),
            "the trigger pressed again"
        );

        open_from_the_keyboard(cx);
        let item = cx
            .debug_bounds("dismissal-item")
            .expect("the item is drawn")
            .center();
        cx.simulate_click(item, Default::default());
        draw(cx);
        assert!(
            cx.debug_bounds("dismissal-content").is_none(),
            "the item's dismiss event"
        );
        assert!(
            trigger_focused(cx),
            "the item's dismissal returns focus to the trigger"
        );

        assert_eq!(&*changes.borrow(), &[true, false, true, false, true, false]);
    }

    /// GitTurtle patch: Enter or Space on a popover's trigger opens it only
    /// as the key goes down, never on the repeats of a held key: held on the
    /// trigger, the key opens nothing, a fresh press opens the popover, and
    /// once Escape has closed it and returned focus to the trigger, the key
    /// still held reopens nothing, while a fresh press does. Enter in the
    /// content still closes it, through the content's own binding. Bound to
    /// `Confirm` in the trigger, each repeat toggled the popover, so a key
    /// still held after a dropdown's item was chosen reopened its menu.
    /// A repeat is a key-down marked `is_held`, as macOS and Wayland without
    /// an input method mark it; under X11, and on Wayland with an input
    /// method such as fcitx5, a held key arrives as fresh presses, which the
    /// trigger cannot tell from new ones.
    #[gpui::test]
    fn a_held_key_on_the_trigger_opens_nothing(cx: &mut gpui::TestAppContext) {
        cx.update(crate::init);
        let changes = Rc::new(RefCell::new(Vec::new()));
        let (view, cx) = cx.add_window_view({
            let changes = changes.clone();
            move |_, cx| DismissalHarness {
                trigger_focus: cx.focus_handle(),
                changes,
            }
        });
        let draw = |cx: &mut gpui::VisualTestContext| {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        };
        let held = |cx: &mut gpui::VisualTestContext, key: &str| {
            cx.simulate_event(KeyDownEvent {
                keystroke: Keystroke::parse(key).unwrap().with_simulated_ime(),
                is_held: true,
                prefer_character_input: false,
            });
            draw(cx);
        };
        let open =
            |cx: &mut gpui::VisualTestContext| cx.debug_bounds("dismissal-content").is_some();
        let trigger_focus = view.read_with(cx, |view, _| view.trigger_focus.clone());
        draw(cx);
        for key in ["enter", "space"] {
            cx.update(|window, cx| trigger_focus.focus(window, cx));
            draw(cx);
            held(cx, key);
            held(cx, key);
            assert!(!open(cx), "{key} held on the trigger opens nothing");
            cx.simulate_keystrokes(key);
            draw(cx);
            assert!(open(cx), "a fresh {key} opens it");
            cx.simulate_keystrokes("escape");
            draw(cx);
            assert!(!open(cx), "Escape closes it");
            assert!(
                cx.update(|window, _| trigger_focus.is_focused(window)),
                "Escape returns focus to the trigger"
            );
            for _ in 0..3 {
                held(cx, key);
            }
            assert!(!open(cx), "{key} still held reopens nothing");
            cx.simulate_keystrokes(key);
            draw(cx);
            assert!(open(cx), "a fresh {key} reopens it");
            cx.simulate_keystrokes("enter");
            draw(cx);
            assert!(!open(cx), "Enter in the content closes it");
        }
        assert_eq!(
            &*changes.borrow(),
            &[true, false, true, false, true, false, true, false]
        );
    }

    /// A view holding only a popover, as a page holds its dropdown. The
    /// popover's content keeps a weak handle to its state.
    struct PopoverPage {
        popover_state: Rc<RefCell<Option<gpui::WeakEntity<PopoverState>>>>,
    }

    impl Render for PopoverPage {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let popover_state = self.popover_state.clone();
            Popover::new("page-popover")
                .trigger_with(|_, _, _| {
                    div()
                        .debug_selector(|| "page-popover-trigger".into())
                        .w(px(80.))
                        .h(px(20.))
                        .child("Open")
                        .into_any_element()
                })
                .content(move |_, _, cx| {
                    *popover_state.borrow_mut() = Some(cx.weak_entity());
                    div()
                        .debug_selector(|| "page-popover-content".into())
                        .size(px(40.))
                })
        }
    }

    /// An input and, below it while there is one, a [`PopoverPage`].
    struct InputAbovePage {
        input: gpui::Entity<crate::input::InputState>,
        page: Option<gpui::Entity<PopoverPage>>,
    }

    impl Render for InputAbovePage {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .debug_selector(|| "input-slot".into())
                        .flex()
                        .w(px(200.))
                        .h(px(30.))
                        .child(self.input.clone()),
                )
                .children(self.page.clone())
        }
    }

    /// What a popover taken out of the tree while open leaves behind.
    #[derive(Debug, PartialEq)]
    struct LeftBehind {
        popover_state_alive: bool,
        popup_registered: bool,
        input_menus_opened: Vec<&'static str>,
    }

    /// GitTurtle patch: a popover whose view is taken away while it is open
    /// — a page left by a shortcut with its dropdown's menu open — goes with
    /// it. Its dismiss subscription held its own state, so the state outlived
    /// its element, for the app's life, and kept its deferred-popup
    /// registration, which is the app's, and every input in every window
    /// then refused its right-click menu for the rest of the session.
    #[gpui::test]
    fn a_popover_taken_away_while_open_lets_its_state_go(cx: &mut gpui::TestAppContext) {
        use gpui::Modifiers;

        cx.update(crate::init);
        let menus = Rc::new(RefCell::new(Vec::new()));
        let popover_state = Rc::new(RefCell::new(None));
        let (view, cx) = cx.add_window_view({
            let menus = menus.clone();
            let popover_state = popover_state.clone();
            move |window, cx| {
                cx.set_global(crate::theme::Theme::default());
                let input = cx.new(|cx| {
                    let mut input = crate::input::InputState::new(window, cx);
                    input.on_context_menu(Rc::new(move |_, _, _, _, _| {
                        menus.borrow_mut().push("input");
                    }));
                    input
                });
                let page = cx.new(|_| PopoverPage { popover_state });
                InputAbovePage {
                    input,
                    page: Some(page),
                }
            }
        });
        let draw = |cx: &mut gpui::VisualTestContext| {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        };
        draw(cx);
        let trigger = cx
            .debug_bounds("page-popover-trigger")
            .expect("the trigger is drawn")
            .center();
        let input = cx
            .debug_bounds("input-slot")
            .expect("the input is drawn")
            .center();

        cx.simulate_click(trigger, Modifiers::default());
        draw(cx);
        assert!(
            cx.debug_bounds("page-popover-content").is_some(),
            "the press opened the popover"
        );
        assert!(
            cx.update(|_, cx| GlobalState::is_in_deferred_context(cx)),
            "the open popover registered its deferred popup"
        );
        let state = popover_state
            .borrow_mut()
            .take()
            .expect("the open popover built its content");
        let page = view.update(cx, |view, cx| {
            cx.notify();
            view.page.take().expect("the page is shown").downgrade()
        });

        draw(cx);
        assert!(page.upgrade().is_none(), "the page view is gone");
        let popover_state_alive = state.upgrade().is_some();
        let popup_registered = cx.update(|_, cx| GlobalState::is_in_deferred_context(cx));

        cx.simulate_mouse_move(input, None, Modifiers::default());
        cx.simulate_mouse_down(input, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(input, MouseButton::Right, Modifiers::default());
        // The input shows its menu in a deferred callback.
        cx.run_until_parked();

        assert_eq!(
            LeftBehind {
                popover_state_alive,
                popup_registered,
                input_menus_opened: menus.borrow().clone(),
            },
            LeftBehind {
                popover_state_alive: false,
                popup_registered: false,
                input_menus_opened: vec!["input"],
            }
        );
    }
}
