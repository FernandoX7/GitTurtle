use std::{rc::Rc, sync::LazyLock};

use gpui::{
    Anchor, AnyElement, App, Context, DismissEvent, ElementId, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, KeyBinding, KeyContext, KeyDownEvent,
    KeybindingKeystroke, Keystroke, MouseButton, NoAction, ParentElement as _, Pixels, Render,
    RenderOnce, Role, SharedString, StatefulInteractiveElement as _, StyleRefinement, Styled,
    Subscription, Window, div, prelude::FluentBuilder as _,
};

use crate::{
    DeferredPopover, GlobalState, Popup, ResolvedPosition, Selectable, StyledExt as _,
    actions::{Cancel, Confirm},
};

const CONTEXT: &str = "Popover";
const CONTENT_CONTEXT: &str = "PopoverContent";
/// GitTurtle patch: the key context a popover's trigger has beside
/// [`CONTEXT`], which its content has not.
const TRIGGER_CONTEXT: &str = "PopoverTrigger";

/// GitTurtle patch: the keys that open or close a popover from its trigger.
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
        KeyBinding::new("escape", Cancel, Some(CONTENT_CONTEXT)),
        // The content is still a descendant of the trigger's dispatch node.
        // Leave activation to its controls, not the trigger or an outer dialog.
        // More deeply scoped input bindings retain precedence.
        KeyBinding::new("enter", NoAction, Some(CONTENT_CONTEXT)),
        KeyBinding::new("space", NoAction, Some(CONTENT_CONTEXT)),
    ]);
    cx.bind_keys(OPEN_KEYS.map(|keys| KeyBinding::new(keys, NoAction, Some(TRIGGER_CONTEXT))));
}

/// GitTurtle patch: a popover's trigger opens or closes it with Enter or
/// Space only as the key goes down, never on a held key's repeats.
/// In the content (whose innermost popover context is `CONTENT_CONTEXT`, not
/// `TRIGGER_CONTEXT`), this passes over the key so the content's controls handle it.
fn open_key(event: &KeyDownEvent, window: &mut Window, cx: &mut App) {
    let in_trigger = OPEN_KEYSTROKES
        .iter()
        .any(|keystroke| event.keystroke.should_match(keystroke))
        && window
            .context_stack()
            .iter()
            .rfind(|context| context.contains(CONTEXT) || context.contains(CONTENT_CONTEXT))
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

    /// Whether a popup opened from inside this popover, such as a select's
    /// menu, is open.
    fn has_nested_popup(&self, cx: &App) -> bool {
        self.deferred_context
            .as_ref()
            .is_some_and(|context| GlobalState::has_deferred_popover_after(context, cx))
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
            // Weak: the subscription is stored on this state, so a strong
            // handle would keep the state, and its deferred-popover
            // registration, alive after its trigger is gone.
            let state = cx.entity().downgrade();
            self.tracked_focus_handle
                .clone()
                .unwrap_or_else(|| self.focus_handle.clone())
                .focus(window, cx);

            self.dismiss_subscription =
                Some(
                    window.subscribe(&cx.entity(), cx, move |_, _: &DismissEvent, window, cx| {
                        _ = state.update(cx, |state, cx| state.dismiss(window, cx));
                        window.refresh();
                    }),
                );
        } else {
            self.dismiss_subscription = None;
            let previous = self.previous_focus_handle.take();
            if self.focus_handle.contains_focused(window, cx) {
                match previous {
                    Some(previous) => previous.focus(window, cx),
                    // Nothing had focus when the popover opened, so nothing has it now. Left
                    // alone, focus would stay on the closed popover's own handle, which is no
                    // longer in the tree.
                    None => window.blur(cx),
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
///
/// Enter and Space on the trigger toggle the popover. Inside its content, those
/// keys belong to the focused control instead; Escape dismisses the popover and
/// restores the previous focus.
#[derive(IntoElement)]
pub struct Popover {
    id: ElementId,
    style: StyleRefinement,
    anchor: Anchor,
    offset: gpui::Pixels,
    on_position: Option<Box<dyn Fn(ResolvedPosition, gpui::Bounds<gpui::Pixels>)>>,
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
    /// GitTurtle patch: the surface's accessible name
    /// ([`Popover::aria_label`]).
    aria_label: Option<SharedString>,
}

impl Popover {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            style: StyleRefinement::default(),
            anchor: Anchor::TopLeft,
            offset: gpui::px(0.),
            on_position: None,
            default_open: false,
            open: None,
            tracked_focus_handle: None,
            trigger: None,
            content: None,
            mouse_button: MouseButton::Left,
            overlay_closable: true,
            on_open_change: None,
            margin: None,
            aria_label: None,
        }
    }

    /// GitTurtle patch: the name assistive technology reads for the open
    /// surface, a dialog, which upstream left unnamed: Orca read a colour
    /// picker's popover as "dialog" and nothing more.
    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
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

    /// Gap from the trigger along the anchor's outward direction, zero by default.
    pub fn offset(mut self, offset: gpui::Pixels) -> Self {
        self.offset = offset;
        self
    }

    /// Observe geometry to supply presentation such as a pointer arrow.
    pub fn on_position(
        mut self,
        callback: impl Fn(ResolvedPosition, gpui::Bounds<gpui::Pixels>) + 'static,
    ) -> Self {
        self.on_position = Some(Box::new(callback));
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
            let open = trigger.is_open();
            trigger.open(open || is_open).into_any_element()
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

/// Styles the trigger container: the element that takes part in the parent
/// layout and whose bounds the popup is anchored to.
impl Styled for Popover {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
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
        let popup = Popup::new(self.id, trigger(open, window, cx))
            .refine_style(&self.style)
            .anchor(self.anchor)
            .offset(self.offset)
            .when_some(self.margin, |this, margin| this.margin(margin))
            .when_some(self.on_position, |this, callback| {
                this.on_position(callback)
            })
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
            .when_some(self.aria_label, |this, label| this.aria_label(label));
        let content = described_surface(content)
            .occlude()
            .tab_group()
            .track_focus(&focus_handle)
            .key_context(CONTENT_CONTEXT)
            .on_action(window.listener_for(&state, PopoverState::on_action_cancel))
            .when_some(self.content, |this, content| {
                this.child(state.update(cx, |state, cx| (content)(state, window, cx)))
            })
            .when(self.overlay_closable, |this| {
                this.on_mouse_down_out({
                    let state = state.clone();
                    move |_, window, cx| {
                        if !state.read(cx).has_nested_popup(cx) {
                            state.update(cx, |state, cx| state.dismiss(window, cx));
                            cx.notify(parent_view_id);
                            return;
                        }
                        // A popup opened from inside is drawn outside these
                        // bounds. A press outside it too closes it during this
                        // event; one inside it leaves it open, and is not a
                        // press outside the popover.
                        let state = state.clone();
                        window.defer(cx, move |window, cx| {
                            if !state.read(cx).has_nested_popup(cx) {
                                state.update(cx, |state, cx| state.dismiss(window, cx));
                                cx.notify(parent_view_id);
                            }
                        });
                    }
                })
            });
        popup.content(content).into_any_element()
    }
}

/// GitTurtle patch: GPUI's test platform never activates accessibility, so a
/// test cannot read a popover's AccessKit node. A debug build names the node
/// a named surface writes in a debug selector, `popover: <name> · <role>`,
/// which a test build registers with its bounds, as the component kit's menu
/// items do.
fn described_surface(content: gpui::Stateful<gpui::Div>) -> gpui::Stateful<gpui::Div> {
    #[cfg(debug_assertions)]
    {
        use gpui::Element as _;
        let mut node = gpui::accesskit::Node::new(Role::Dialog);
        content.write_a11y_info(&mut node);
        if let Some(label) = node.label() {
            let selector = format!("popover: {label} · {:?}", node.role());
            return content.debug_selector(move || selector);
        }
    }
    content
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, Context, Render, point, px};
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
        cx.update(|window, cx| assert!(window.focused(cx).is_none(), "nothing has focus yet"));

        cx.simulate_click(point(px(20.), px(10.)), Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(cx.debug_bounds("base-popover-content").is_some());
        cx.update(|window, cx| assert!(window.focused(cx).is_some(), "the popover takes focus"));

        cx.simulate_click(point(px(300.), px(300.)), Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(cx.debug_bounds("base-popover-content").is_none());
        assert_eq!(&*changes.borrow(), &[true, false]);
        // Nothing had focus when the popover opened, so nothing has it now, not the closed
        // popover's own handle.
        cx.update(|window, cx| {
            assert!(
                window.focused(cx).is_none(),
                "focus stays on {:?}",
                window.focused(cx)
            )
        });
    }

    struct NestedPopoverHarness;

    impl Render for NestedPopoverHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            Popover::new("outer-popover")
                .trigger_with(|_, _, _| div().size(px(40.)).child("Open").into_any_element())
                .content(|_, _, _| {
                    div()
                        .debug_selector(|| "outer-popover-content".into())
                        .size(px(40.))
                        .child(
                            Popover::new("inner-popover")
                                .trigger_with(|_, _, _| {
                                    div()
                                        .debug_selector(|| "inner-popover-trigger".into())
                                        .size(px(20.))
                                        .into_any_element()
                                })
                                .content(|_, _, _| {
                                    div()
                                        .debug_selector(|| "inner-popover-content".into())
                                        .size(px(120.))
                                }),
                        )
                })
        }
    }

    /// A popup opened from inside a popover, such as a select's menu, is drawn
    /// outside the popover's bounds. A press in it is not a press outside the
    /// popover; a press outside both closes both.
    #[gpui::test]
    fn a_press_in_a_nested_popup_keeps_the_popover_open(cx: &mut gpui::TestAppContext) {
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|_, _| NestedPopoverHarness);
        cx.update(|window, cx| window.draw(cx).clear(cx));

        cx.simulate_click(point(px(20.), px(10.)), Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let outer = cx
            .debug_bounds("outer-popover-content")
            .expect("the outer popover opens");

        let inner_trigger = cx
            .debug_bounds("inner-popover-trigger")
            .expect("the outer popover holds the inner trigger");
        cx.simulate_click(inner_trigger.center(), Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let inner = cx
            .debug_bounds("inner-popover-content")
            .expect("the inner popover opens");

        let in_inner_only = point(inner.right() - px(2.), inner.top() + px(2.));
        assert!(
            !outer.contains(&in_inner_only),
            "the press must land outside the outer popover's bounds"
        );
        cx.simulate_click(in_inner_only, Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(cx.debug_bounds("inner-popover-content").is_some());
        assert!(
            cx.debug_bounds("outer-popover-content").is_some(),
            "a press in the nested popup must not dismiss the popover"
        );

        cx.simulate_click(point(px(300.), px(300.)), Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(cx.debug_bounds("inner-popover-content").is_none());
        assert!(
            cx.debug_bounds("outer-popover-content").is_none(),
            "a press outside both closes both"
        );
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

    /// A trigger that keeps "open" and "selected" apart, the way a downstream
    /// sidebar row does: it is selected when it is the current view, and open
    /// only while its popover is showing.
    #[derive(IntoElement)]
    struct RecordingTrigger {
        calls: Rc<RefCell<Vec<(&'static str, bool)>>>,
        selected: bool,
        open: bool,
    }

    impl Selectable for RecordingTrigger {
        fn selected(mut self, selected: bool) -> Self {
            self.calls.borrow_mut().push(("selected", selected));
            self.selected = selected;
            self
        }

        fn is_selected(&self) -> bool {
            self.selected
        }

        fn open(mut self, open: bool) -> Self {
            self.calls.borrow_mut().push(("open", open));
            self.open = open;
            self
        }

        fn is_open(&self) -> bool {
            self.open
        }
    }

    impl RenderOnce for RecordingTrigger {
        fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
            div().size(px(40.)).child("Open")
        }
    }

    struct RecordingTriggerHarness {
        calls: Rc<RefCell<Vec<(&'static str, bool)>>>,
    }

    impl Render for RecordingTriggerHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            Popover::new("recording-popover")
                .trigger(RecordingTrigger {
                    calls: self.calls.clone(),
                    selected: false,
                    open: false,
                })
                .content(|_, _, _| div().size(px(40.)))
        }
    }

    #[gpui::test]
    fn an_open_popover_tells_its_trigger_it_is_open_not_selected(cx: &mut gpui::TestAppContext) {
        cx.update(crate::init);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let (_, cx) = cx.add_window_view({
            let calls = calls.clone();
            move |_, _| RecordingTriggerHarness { calls }
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        calls.borrow_mut().clear();

        cx.simulate_click(point(px(20.), px(10.)), Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));

        let calls = calls.borrow();
        assert!(
            calls.contains(&("open", true)),
            "an open popover marks its trigger open, got {calls:?}"
        );
        assert!(
            !calls.iter().any(|(name, _)| *name == "selected"),
            "opening must not touch the trigger's own selection, got {calls:?}"
        );
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
        let held = |cx: &mut gpui::VisualTestContext, key: &'static str| {
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
            // In 0.10.0 PopoverContent binds Enter/Space to NoAction so inner
            // controls own them; verify Enter inside content stays open and
            // Escape closes it.
            cx.simulate_keystrokes("enter");
            draw(cx);
            assert!(open(cx), "Enter in the content does not toggle the trigger");
            cx.simulate_keystrokes("escape");
            draw(cx);
            assert!(!open(cx), "Escape closes it");
        }
        assert_eq!(
            &*changes.borrow(),
            &[true, false, true, false, true, false, true, false]
        );
    }
}
