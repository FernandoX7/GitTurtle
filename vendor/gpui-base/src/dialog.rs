use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};

use gpui::{
    AnyElement, App, ClickEvent, FocusHandle, Global, InteractiveElement as _, IntoElement,
    KeyBinding, KeyDownEvent, KeyUpEvent, MouseButton, NoAction, ParentElement, Pixels,
    RenderOnce, Role, StatefulInteractiveElement as _, StyleRefinement, Styled, WeakFocusHandle,
    Window, WindowId, actions, anchored, deferred, div, point, prelude::FluentBuilder as _, px,
};
use smallvec::SmallVec;

use crate::actions::{Cancel, Confirm};
use crate::{FocusTrapElement as _, StyledExt as _};

const CONTEXT: &str = "Dialog";
/// GitTurtle patch: the key context of a [`crate::Button`], in which a
/// dialog leaves Enter to the Button ([`enter_presses_button`]).
pub(crate) const BUTTON_CONTEXT: &str = "Button";
type Decision = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) -> bool>;
type Closed = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type CloseRequest = Rc<dyn Fn(bool, &mut Window, &mut App)>;
type OpenRequest = Rc<dyn Fn(&mut Window, &mut App)>;
type OpenChange = Rc<dyn Fn(bool, DialogChangeReason, &mut Window, &mut App)>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DialogChangeReason {
    TriggerPress,
    BackdropPress,
    Cancel,
    Confirm,
    Imperative,
}

// GitTurtle patch: a modal states which kind it is, so assistive technology
// announces a form, browser or picker as a dialog and only a confirmation
// that interrupts with a warning, or an error, as an alert.
/// The kind of modal a dialog is: its accessible role.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DialogKind {
    /// A dialog (`Role::Dialog`): a form, browser, picker or sheet.
    #[default]
    Dialog,
    /// An alert dialog (`Role::AlertDialog`): a confirmation that interrupts
    /// with a warning, or an error the person must acknowledge.
    Alert,
}

impl DialogKind {
    fn role(self) -> Role {
        match self {
            Self::Dialog => Role::Dialog,
            Self::Alert => Role::AlertDialog,
        }
    }
}

#[derive(Clone)]
pub struct DialogHandle {
    open: Rc<Cell<bool>>,
    on_open_change: Rc<RefCell<Option<OpenChange>>>,
}

impl DialogHandle {
    pub fn new(open: bool) -> Self {
        Self {
            open: Rc::new(Cell::new(open)),
            on_open_change: Rc::new(RefCell::new(None)),
        }
    }
    pub fn is_open(&self) -> bool {
        self.open.get()
    }
    pub fn open(&self, window: &mut Window, cx: &mut App) {
        self.set_open(true, DialogChangeReason::Imperative, window, cx);
    }
    pub fn close(&self, window: &mut Window, cx: &mut App) {
        self.set_open(false, DialogChangeReason::Imperative, window, cx);
    }
    pub(crate) fn set_open(
        &self,
        open: bool,
        reason: DialogChangeReason,
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.open.replace(open) == open {
            return;
        }
        let callback = self.on_open_change.borrow().clone();
        if let Some(callback) = callback {
            callback(open, reason, window, cx);
        }
        window.refresh();
    }
}

fn request_open_change(
    handle: &Option<DialogHandle>,
    callback: &Option<OpenChange>,
    open: bool,
    reason: DialogChangeReason,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(handle) = handle {
        handle.set_open(open, reason, window, cx);
    } else if let Some(callback) = callback {
        callback(open, reason, window, cx);
    }
}

actions!(
    dialog,
    [
        /// GitTurtle patch: Enter reached a dialog with nothing inside it
        /// binding the key; the dialog confirms only on a fresh press
        /// ([`Dialog`]'s key-down listener).
        #[doc(hidden)]
        EnterKey
    ]
);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", Cancel, Some(CONTEXT)),
        // GitTurtle patch: Enter is no longer bound to `Confirm`, which a
        // held key's repeats dispatched as well as its first press. Bound
        // to nothing in the dialog, it reaches no binding outside it; bound
        // to `EnterKey` after that, at the same depth, it marks the press
        // as the dialog's for the dialog's key-down listener, which
        // confirms only on a fresh press.
        KeyBinding::new("enter", NoAction, Some(CONTEXT)),
        KeyBinding::new("enter", EnterKey, Some(CONTEXT)),
        // GitTurtle patch: on a focused Button, Enter is the Button's own.
        KeyBinding::new(
            "enter",
            NoAction,
            Some(&format!("{CONTEXT} > {BUTTON_CONTEXT}")),
        ),
    ]);
    // GitTurtle patch: every Enter that goes down, whatever then takes it,
    // and what had focus as its press began; any other key ends that press.
    // On macOS a Command-Enter (a list's, menu's or combobox's secondary
    // Enter) is such another key and records nothing: Cocoa sends no key-up
    // for a key pressed with Command held, so a recorded one stayed down and
    // took the next Return on another control for its repeat. Every other
    // Enter, with Shift, Control, Alt, Fn or, off macOS, Super held, records
    // as a plain one, since its key-up arrives: under X11 each repeat of a
    // held key carries the modifiers held as it repeats, so an unrecorded
    // Shift- or Control-Enter that opened a confirmation, or moved focus to
    // a Button, had the plain repeat after the modifier came up confirm it
    // or press that Button. Residual, unverified without a Mac: Enter pressed
    // alone, then Command, then Enter released; if Cocoa drops that key-up
    // too, the press stays recorded, and the next plain Return presses no
    // Button but the one the press began on, nor confirms a dialog opened
    // meanwhile, until its own key-up ends the press: one Return swallowed,
    // which fails safe.
    cx.intercept_keystrokes(|event, window, cx| {
        let id = window.window_handle().window_id();
        if event.keystroke.key == ENTER && !key_up_withheld(&event.keystroke.modifiers) {
            let focused = window.focused(cx).map(|focus| focus.downgrade());
            let press = cx.default_global::<EnterPress>();
            press.down.insert(id);
            press.began.entry(id).or_insert(focused);
        } else if cx
            .try_global::<EnterPress>()
            .is_some_and(|press| press.began.contains_key(&id))
        {
            cx.global_mut::<EnterPress>().began.remove(&id);
        }
    })
    .detach();
}

const ENTER: &str = "enter";

/// GitTurtle patch: whether a key pressed with `modifiers` held sends no
/// key-up, as Cocoa sends none for a key pressed with Command held on macOS.
/// A key pressed with any other modifier sends one, and off macOS so does a
/// key pressed with Super, the platform modifier there.
fn key_up_withheld(modifiers: &gpui::Modifiers) -> bool {
    cfg!(target_os = "macos") && modifiers.platform
}

/// GitTurtle patch: Enter's state in each window, so a dialog opened by an
/// Enter press takes no Enter until that press comes up: under X11 a held
/// key's repeats arrive as fresh presses, which no repeat flag marks.
#[derive(Default)]
struct EnterPress {
    /// The windows where Enter went down and has not come up.
    down: HashSet<WindowId>,
    /// The windows where a dialog opened while Enter was down.
    blocked: HashSet<WindowId>,
    /// What had focus in each window as its Enter press began: at its first
    /// key-down since that window's last Enter key-up or other key, which
    /// under X11 is the one key-down of a held Enter that is not a repeat
    /// ([`enter_press_began_on`]).
    began: HashMap<WindowId, Option<WeakFocusHandle>>,
}

impl Global for EnterPress {}

/// GitTurtle patch: a dialog opened in `window` while Enter is down takes
/// no Enter, neither its default action nor a press of its Buttons, until
/// that Enter comes up ([`note_key_up`]). An opener calls this as it opens
/// the dialog, as the component `Root` does.
pub fn dialog_opened(window: &Window, cx: &mut App) {
    let window = window.window_handle().window_id();
    let press = cx.default_global::<EnterPress>();
    if press.down.contains(&window) {
        press.blocked.insert(window);
    }
}

/// GitTurtle patch: Enter coming up in `window` ends the press a dialog
/// waited out. A dialog's host notes it, and so does whatever holds the
/// window's root, as the component `Root` does, since an Enter can come up
/// with no dialog open.
pub fn note_key_up(event: &KeyUpEvent, window: &Window, cx: &mut App) {
    if event.keystroke.key != ENTER {
        return;
    }
    let window = window.window_handle().window_id();
    if let Some(press) = cx.try_global::<EnterPress>()
        && (press.down.contains(&window)
            || press.blocked.contains(&window)
            || press.began.contains_key(&window))
    {
        let press = cx.global_mut::<EnterPress>();
        press.down.remove(&window);
        press.blocked.remove(&window);
        press.began.remove(&window);
    }
}

/// GitTurtle patch: whether the Enter press going down in `window` began
/// with `focus` focused, or no press is recorded. A control that acts on
/// Enter as the key goes down asks this, so a held Enter acts once, on the
/// control it began on: under X11 a held key's repeats arrive as fresh
/// presses, not marked held, so a press that moved focus to a Button had
/// the next repeat press that Button. The press ends at its key-up or at
/// any other key, as a held key's repeats do.
pub fn enter_press_began_on(focus: &FocusHandle, window: &Window, cx: &App) -> bool {
    cx.try_global::<EnterPress>()
        .and_then(|press| press.began.get(&window.window_handle().window_id()))
        .is_none_or(|began| began.as_ref().is_some_and(|began| began == focus))
}

/// GitTurtle patch: whether `window`'s dialogs wait for an Enter press that
/// opened one to come up.
fn enter_blocked(window: &Window, cx: &App) -> bool {
    cx.try_global::<EnterPress>().is_some_and(|press| {
        press
            .blocked
            .contains(&window.window_handle().window_id())
    })
}

/// GitTurtle patch: whether `event` is Enter going down on a focused Button
/// in a dialog, which activates that Button rather than the dialog's default
/// action: the focus ring shows the control the keyboard acts on, and Enter
/// there took the dialog's [`Confirm`], pressing a control other than the one
/// it marks. On the Button the binding above leaves the key unbound, so it
/// reaches the Button, which acts as it goes down, never on a held key's
/// repeats, as a popover's trigger and a menu act, and takes it no further.
/// In a field, on the dialog itself and on anything else that is not a
/// Button, Enter is the dialog's [`Confirm`] as before; Escape is unchanged.
pub(crate) fn enter_presses_button(event: &KeyDownEvent, window: &Window) -> bool {
    event.keystroke.key == ENTER
        && !event.keystroke.modifiers.modified()
        && window
            .context_stack()
            .iter()
            .any(|context| context.contains(CONTEXT))
}

impl Dialog {
    pub fn on_ok(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.on_ok = Rc::new(handler);
        self
    }

    pub fn on_cancel(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.on_cancel = Rc::new(handler);
        self
    }

    pub fn on_close(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_close = Rc::new(handler);
        self
    }
}

/// Unstyled modal host owning focus, keyboard actions, dismissal, and callback ordering.
#[derive(IntoElement)]
pub struct Dialog {
    style: StyleRefinement,
    focus: FocusHandle,
    role: Role,
    layer: usize,
    keyboard: bool,
    overlay_closable: bool,
    topmost: bool,
    dismiss_below_y: Pixels,
    backdrop: Option<AnyElement>,
    popup: Option<AnyElement>,
    children: SmallVec<[AnyElement; 2]>,
    on_ok: Decision,
    on_cancel: Decision,
    on_close: Closed,
    request_close: CloseRequest,
    handle: Option<DialogHandle>,
    open: bool,
    on_open_change: Option<OpenChange>,
}

/// Unstyled trigger that owns pointer activation for opening a dialog.
#[derive(IntoElement)]
pub struct DialogTrigger {
    trigger: AnyElement,
    open: OpenRequest,
    handle: Option<DialogHandle>,
}

impl DialogTrigger {
    pub fn new(trigger: impl IntoElement) -> Self {
        Self {
            trigger: trigger.into_any_element(),
            open: Rc::new(|_, _| {}),
            handle: None,
        }
    }
    pub fn handle(mut self, handle: DialogHandle) -> Self {
        self.handle = Some(handle);
        self
    }

    pub fn on_open(mut self, open: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.open = Rc::new(open);
        self
    }
}

impl RenderOnce for DialogTrigger {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                if let Some(handle) = self.handle.as_ref() {
                    handle.set_open(true, DialogChangeReason::TriggerPress, window, cx);
                }
                (self.open)(window, cx);
                cx.stop_propagation();
            })
            .child(self.trigger)
    }
}

macro_rules! dialog_part {
    ($(#[$meta:meta])* $name:ident, $id:literal) => {
        $(#[$meta])*
        #[derive(IntoElement)]
        pub struct $name {
            style: StyleRefinement,
            children: SmallVec<[AnyElement; 2]>,
        }

        impl $name {
            pub fn new() -> Self {
                Self {
                    style: StyleRefinement::default(),
                    children: SmallVec::new(),
                }
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl Styled for $name {
            fn style(&mut self) -> &mut StyleRefinement {
                &mut self.style
            }
        }

        impl ParentElement for $name {
            fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
                self.children.extend(elements);
            }
        }

        impl RenderOnce for $name {
            fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
                div()
                    .id($id)
                    .children(self.children)
                    .refine_style(&self.style)
            }
        }
    };
}

dialog_part!(
    /// Unstyled backdrop part rendered behind a dialog popup.
    DialogBackdrop,
    "dialog-backdrop"
);

dialog_part!(
    /// Unstyled popup part containing dialog content.
    DialogPopup,
    "dialog-popup"
);

/// Unstyled title slot for a dialog surface.
#[derive(IntoElement)]
pub struct DialogTitle {
    base: gpui::Div,
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 2]>,
}

impl DialogTitle {
    pub fn new() -> Self {
        Self {
            base: div(),
            style: StyleRefinement::default(),
            children: SmallVec::new(),
        }
    }
}

impl Default for DialogTitle {
    fn default() -> Self {
        Self::new()
    }
}
impl Styled for DialogTitle {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl ParentElement for DialogTitle {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}
impl RenderOnce for DialogTitle {
    fn render(mut self, _: &mut Window, _: &mut App) -> impl IntoElement {
        crate::dialog_label::capture_title(&mut self.children);
        self.base
            .id("dialog-title")
            .children(self.children)
            .refine_style(&self.style)
    }
}

/// Unstyled descriptive-content slot for a dialog surface.
#[derive(IntoElement)]
pub struct DialogDescription {
    base: gpui::Div,
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 2]>,
}

impl DialogDescription {
    pub fn new() -> Self {
        Self {
            base: div(),
            style: StyleRefinement::default(),
            children: SmallVec::new(),
        }
    }
}

impl Default for DialogDescription {
    fn default() -> Self {
        Self::new()
    }
}
impl Styled for DialogDescription {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl ParentElement for DialogDescription {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}
impl RenderOnce for DialogDescription {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base
            .id("dialog-description")
            .children(self.children)
            .refine_style(&self.style)
    }
}

/// Wrapper that dispatches the dialog cancel action when activated.
#[derive(IntoElement)]
pub struct DialogClose {
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 1]>,
}

impl DialogClose {
    pub fn new() -> Self {
        Self {
            style: StyleRefinement::default(),
            children: SmallVec::new(),
        }
    }
}
impl Default for DialogClose {
    fn default() -> Self {
        Self::new()
    }
}
impl ParentElement for DialogClose {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}
impl Styled for DialogClose {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl RenderOnce for DialogClose {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .id("dialog-close")
            .on_click(|_, window, cx| window.dispatch_action(Box::new(Cancel), cx))
            .children(self.children)
            .refine_style(&self.style)
    }
}

impl Dialog {
    pub fn new(cx: &mut App) -> Self {
        Self {
            style: StyleRefinement::default(),
            focus: cx.focus_handle(),
            role: Role::Dialog,
            layer: 0,
            keyboard: true,
            overlay_closable: true,
            topmost: true,
            dismiss_below_y: px(0.),
            backdrop: None,
            popup: None,
            children: SmallVec::new(),
            on_ok: Rc::new(|_, _, _| true),
            on_cancel: Rc::new(|_, _, _| true),
            on_close: Rc::new(|_, _, _| {}),
            request_close: Rc::new(|_, _, _| {}),
            handle: None,
            open: true,
            on_open_change: None,
        }
    }
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }
    pub fn handle(mut self, handle: DialogHandle) -> Self {
        if let Some(callback) = self.on_open_change.as_ref() {
            *handle.on_open_change.borrow_mut() = Some(callback.clone());
        }
        self.handle = Some(handle);
        self
    }
    pub fn on_open_change(
        mut self,
        handler: impl Fn(bool, DialogChangeReason, &mut Window, &mut App) + 'static,
    ) -> Self {
        let handler: OpenChange = Rc::new(handler);
        if let Some(handle) = self.handle.as_ref() {
            *handle.on_open_change.borrow_mut() = Some(handler.clone());
        }
        self.on_open_change = Some(handler);
        self
    }

    pub fn backdrop(mut self, element: impl IntoElement) -> Self {
        self.backdrop = Some(element.into_any_element());
        self
    }
    pub fn popup(mut self, element: impl IntoElement) -> Self {
        self.popup = Some(element.into_any_element());
        self
    }
    pub fn close_on_escape(mut self, value: bool) -> Self {
        self.keyboard = value;
        self
    }
    pub fn close_on_backdrop_press(mut self, value: bool) -> Self {
        self.overlay_closable = value;
        self
    }
    pub fn dismiss_below_y(mut self, value: Pixels) -> Self {
        self.dismiss_below_y = value;
        self
    }
    pub(crate) fn role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }
    /// Sets the kind of modal, which decides its accessible role.
    pub fn kind(self, kind: DialogKind) -> Self {
        self.role(kind.role())
    }
    #[doc(hidden)]
    pub fn layer(mut self, index: usize, topmost: bool) -> Self {
        self.layer = index;
        self.topmost = topmost;
        self
    }
    #[doc(hidden)]
    pub fn focus_handle(mut self, value: FocusHandle) -> Self {
        self.focus = value;
        self
    }
    #[doc(hidden)]
    pub fn request_close(
        mut self,
        handler: impl Fn(bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.request_close = Rc::new(handler);
        self
    }
}

impl Styled for Dialog {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl ParentElement for Dialog {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Dialog {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let open = self
            .handle
            .as_ref()
            .map_or(self.open, DialogHandle::is_open);
        if !open {
            return div().into_any_element();
        }
        let request_close = self.request_close;
        let cancel = self.on_cancel.clone();
        let confirm = self.on_ok.clone();
        let closed = self.on_close.clone();
        let overlay_closable = self.overlay_closable && self.topmost;
        let dismiss_below_y = self.dismiss_below_y;
        let escape_handle = self.handle.clone();
        let confirm_handle = self.handle.clone();
        let backdrop_handle = self.handle.clone();
        let escape_change = self.on_open_change.clone();
        let confirm_change = self.on_open_change.clone();
        let backdrop_change = self.on_open_change.clone();
        let viewport = window.viewport_size();
        let keyboard = self.keyboard;
        // GitTurtle patch: set as `EnterKey` reaches the dialog and taken by
        // its key-down listener in the same press.
        let enter = Rc::new(Cell::new(false));

        deferred(
            anchored().position(point(px(0.), px(0.))).child(
                div()
                    .id(("dialog-host", self.layer))
                    .absolute()
                    .top_0()
                    .left_0()
                    .w(viewport.width)
                    .h(viewport.height)
                    .role(self.role)
                    .track_focus(&self.focus)
                    .focus_trap(format!("dialog-{}", self.layer), &self.focus)
                    .when(self.keyboard, |this| this.key_context(CONTEXT))
                    .map(|this| {
                        let request_cancel = request_close.clone();
                        let request_confirm = request_close.clone();
                        let closed_cancel = closed.clone();
                        let (marked, pressed) = (enter.clone(), enter.clone());
                        this.when(keyboard, |this| {
                            this.on_action(move |_: &EnterKey, _, cx| {
                                marked.set(true);
                                cx.propagate();
                            })
                            // GitTurtle patch: Enter confirms the dialog as
                            // it goes down, never on a held key's repeats,
                            // and not at all until an Enter that opened the
                            // dialog comes up. Taken in the capture phase,
                            // ahead of anything inside the dialog, as the
                            // binding to `Confirm` took it; a press something
                            // inside binds (a field, a Button) reaches it as
                            // before, unless the dialog waits out its opener.
                            .capture_key_down(move |event, window, cx| {
                                if event.keystroke.key != ENTER
                                    || event.keystroke.modifiers.modified()
                                {
                                    return;
                                }
                                let marked = pressed.replace(false);
                                if enter_blocked(window, cx) {
                                    cx.stop_propagation();
                                    return;
                                }
                                if !marked {
                                    return;
                                }
                                if !event.is_held
                                    && let Some(focused) = window.focused(cx)
                                {
                                    focused.dispatch_action(
                                        &Confirm { secondary: false },
                                        window,
                                        cx,
                                    );
                                }
                                cx.stop_propagation();
                            })
                            .capture_key_up(|event, window, cx| note_key_up(event, window, cx))
                        })
                        .when(keyboard, |this| {
                            this.on_action(|_: &crate::input::Escape, window, cx| {
                                // Inputs bind Escape to their own action. Bridge it
                                // only after child Find/completion/IME handlers had
                                // the chance to consume it, preserving first-Escape
                                // dismissal of those transient input surfaces.
                                window.dispatch_action(Box::new(Cancel), cx);
                                cx.stop_propagation();
                            })
                        })
                        .on_action(move |_: &Cancel, window, cx| {
                            let event = ClickEvent::default();
                            if cancel(&event, window, cx) {
                                request_open_change(
                                    &escape_handle,
                                    &escape_change,
                                    false,
                                    DialogChangeReason::Cancel,
                                    window,
                                    cx,
                                );
                                request_cancel(false, window, cx);
                                closed_cancel(&event, window, cx);
                            }
                        })
                        .on_action(move |_: &Confirm, window, cx| {
                            let event = ClickEvent::default();
                            if confirm(&event, window, cx) {
                                request_open_change(
                                    &confirm_handle,
                                    &confirm_change,
                                    false,
                                    DialogChangeReason::Confirm,
                                    window,
                                    cx,
                                );
                                request_confirm(true, window, cx);
                                closed(&event, window, cx);
                            }
                        })
                    })
                    .when_some(self.backdrop, |this, backdrop| {
                        let cancel = self.on_cancel.clone();
                        let closed = self.on_close.clone();
                        let request_close = request_close.clone();
                        this.child(
                            div()
                                // The backdrop covers the host, so a caller's
                                // `absolute()` surface has a box to fill.
                                .absolute()
                                .inset_0()
                                .on_any_mouse_down(move |event, window, cx| {
                                    if event.position.y < dismiss_below_y {
                                        return;
                                    }
                                    let button = event.button;
                                    cx.stop_propagation();
                                    let event = ClickEvent::default();
                                    if button == MouseButton::Left
                                        && overlay_closable
                                        && cancel(&event, window, cx)
                                    {
                                        request_open_change(
                                            &backdrop_handle,
                                            &backdrop_change,
                                            false,
                                            DialogChangeReason::BackdropPress,
                                            window,
                                            cx,
                                        );
                                        request_close(false, window, cx);
                                        closed(&event, window, cx);
                                    }
                                })
                                .child(backdrop),
                        )
                    })
                    .children(self.popup)
                    .children(self.children)
                    .refine_style(&self.style),
            ),
        )
        .with_priority(10 + self.layer)
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Context, Render, point};
    use std::{cell::RefCell, rc::Rc};

    struct TriggerHarness {
        handle: DialogHandle,
    }
    impl Render for TriggerHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            DialogTrigger::new(div().size(px(100.))).handle(self.handle.clone())
        }
    }

    #[gpui::test]
    fn trigger_opens_shared_handle_and_reports_reason(cx: &mut gpui::TestAppContext) {
        let handle = DialogHandle::new(false);
        let changes = Rc::new(RefCell::new(Vec::new()));
        *handle.on_open_change.borrow_mut() = Some({
            let changes = changes.clone();
            Rc::new(move |open, reason, _, _| changes.borrow_mut().push((open, reason)))
        });
        let (_, cx) = cx.add_window_view({
            let handle = handle.clone();
            move |_, _| TriggerHarness { handle }
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.simulate_click(point(px(20.), px(20.)), Default::default());

        assert!(handle.is_open());
        assert_eq!(
            &*changes.borrow(),
            &[(true, DialogChangeReason::TriggerPress)]
        );
    }

    /// The backdrop is the dimming surface every caller hands over as an
    /// `absolute()` element, so it has to have the host's box to resolve
    /// against — a collapsed wrapper leaves it zero-sized and invisible.
    #[gpui::test]
    fn the_backdrop_fills_the_host(cx: &mut gpui::TestAppContext) {
        use gpui::{Bounds, canvas};
        use std::cell::Cell;

        struct Harness {
            focus: FocusHandle,
            bounds: Rc<Cell<Bounds<Pixels>>>,
        }
        impl Render for Harness {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let bounds = self.bounds.clone();
                Dialog::new(cx)
                    .open(true)
                    .focus_handle(self.focus.clone())
                    .backdrop(
                        canvas(
                            move |bounds_of_backdrop, _, _| bounds.set(bounds_of_backdrop),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .popup(div().size(px(100.)))
            }
        }

        cx.update(crate::init);
        let bounds = Rc::new(Cell::new(Bounds::default()));
        let (_, cx) = cx.add_window_view({
            let bounds = bounds.clone();
            move |_, cx| Harness {
                focus: cx.focus_handle(),
                bounds,
            }
        });
        let viewport = cx.update(|window, cx| {
            let viewport = window.viewport_size();
            window.draw(cx).clear(cx);
            viewport
        });

        assert_eq!(
            bounds.get().size,
            viewport,
            "a zero-sized backdrop paints no overlay behind the dialog"
        );
    }

    /// GitTurtle patch: Enter on a focused Button in a dialog presses that
    /// Button, once, as the key goes down: a held key's repeats and its
    /// release press nothing more, and the dialog's default action does not
    /// run. On the dialog itself and on a focused element that is not a
    /// Button, Enter is the dialog's [`Confirm`], as before. Bound to
    /// [`Confirm`] in the dialog, Enter on the Button ran the default action
    /// and pressed nothing. A press that began on another control presses
    /// the Button nothing once focus moves to it, as a press that opens or
    /// focuses a control moves it: under X11 the held press's repeats arrive
    /// not marked held, and the first pressed the Button. On macOS a
    /// Command-Enter, whose key-up Cocoa never sends, begins no press; off
    /// macOS a Super-Enter, whose key-up arrives, begins one as Enter does.
    #[gpui::test]
    fn enter_on_a_focused_button_presses_it_once(cx: &mut gpui::TestAppContext) {
        use gpui::{KeyDownEvent, KeyUpEvent, Keystroke};
        use std::cell::Cell;

        struct Harness {
            dialog: FocusHandle,
            field: FocusHandle,
            button: FocusHandle,
            confirms: Rc<Cell<usize>>,
            presses: Rc<Cell<usize>>,
        }
        impl Render for Harness {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let (confirms, presses) = (self.confirms.clone(), self.presses.clone());
                Dialog::new(cx)
                    .open(true)
                    .focus_handle(self.dialog.clone())
                    .on_ok(move |_, _, _| {
                        confirms.set(confirms.get() + 1);
                        false
                    })
                    .child(div().track_focus(&self.field).size(px(20.)))
                    .child(
                        crate::Button::new("press")
                            .track_focus(&self.button)
                            .size(px(20.))
                            .on_click(move |_, _, _| presses.set(presses.get() + 1)),
                    )
            }
        }

        cx.update(crate::init);
        let (confirms, presses) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let (view, cx) = cx.add_window_view({
            let (confirms, presses) = (confirms.clone(), presses.clone());
            move |_, cx| Harness {
                dialog: cx.focus_handle(),
                field: cx.focus_handle(),
                button: cx.focus_handle(),
                confirms,
                presses,
            }
        });
        let enter = Keystroke::parse("enter").unwrap();
        let down = |cx: &mut gpui::VisualTestContext, is_held: bool| {
            cx.simulate_event(KeyDownEvent {
                keystroke: enter.clone(),
                is_held,
                prefer_character_input: false,
            })
        };
        let focus = |cx: &mut gpui::VisualTestContext, pick: fn(&Harness) -> FocusHandle| {
            let handle = view.read_with(cx, |harness, _| pick(harness));
            cx.update(|window, cx| {
                window.focus(&handle, cx);
                window.draw(cx).clear(cx);
            });
        };

        focus(cx, |harness| harness.button.clone());
        down(cx, false);
        assert_eq!((presses.get(), confirms.get()), (1, 0), "Enter presses the Button");
        down(cx, true);
        down(cx, true);
        cx.simulate_event(KeyUpEvent {
            keystroke: enter.clone(),
        });
        assert_eq!(
            (presses.get(), confirms.get()),
            (1, 0),
            "a held Enter and its release press nothing more"
        );

        focus(cx, |harness| harness.dialog.clone());
        down(cx, false);
        assert_eq!((presses.get(), confirms.get()), (1, 1), "on the dialog, Confirm");

        focus(cx, |harness| harness.field.clone());
        down(cx, false);
        assert_eq!((presses.get(), confirms.get()), (1, 2), "beside the Button, Confirm");

        focus(cx, |harness| harness.button.clone());
        down(cx, false);
        assert_eq!(
            (presses.get(), confirms.get()),
            (1, 2),
            "a press that began beside the Button presses nothing"
        );
        cx.simulate_event(KeyUpEvent {
            keystroke: enter.clone(),
        });
        down(cx, false);
        assert_eq!(
            (presses.get(), confirms.get()),
            (2, 2),
            "a press of its own presses it"
        );

        // On macOS a Command-Enter is no press of Enter's: Cocoa sends no
        // key-up for a key pressed with Command held, and a recorded
        // secondary Enter took the next Return on another control for its
        // repeat. Off macOS the same keystroke is Super-Enter, whose key-up
        // arrives, so begun beside the Button it presses that Button nothing
        // until it comes up.
        cx.simulate_event(KeyUpEvent {
            keystroke: enter.clone(),
        });
        focus(cx, |harness| harness.field.clone());
        cx.simulate_event(KeyDownEvent {
            keystroke: Keystroke::parse("cmd-enter").unwrap(),
            is_held: false,
            prefer_character_input: false,
        });
        focus(cx, |harness| harness.button.clone());
        down(cx, false);
        #[cfg(target_os = "macos")]
        assert_eq!(
            (presses.get(), confirms.get()),
            (3, 2),
            "a Return after a Command-Enter that never came up presses the Button"
        );
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(
                (presses.get(), confirms.get()),
                (2, 2),
                "a repeat of a Super-Enter begun beside the Button presses nothing"
            );
            cx.simulate_event(KeyUpEvent {
                keystroke: enter.clone(),
            });
            down(cx, false);
            assert_eq!(
                (presses.get(), confirms.get()),
                (3, 2),
                "a press of its own after the Super-Enter's key-up presses it"
            );
        }
    }

    /// A probe of two dialogs: A, focused on its Button, which opens B over
    /// it as a reviewed write's last step opens, and B, whose default action
    /// counts.
    struct Pair {
        a: FocusHandle,
        button: FocusHandle,
        b: FocusHandle,
        b_open: Rc<Cell<bool>>,
        confirms: Rc<Cell<usize>>,
    }

    impl Render for Pair {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let (b, b_open, confirms) = (self.b.clone(), self.b_open.clone(), self.confirms.clone());
            div()
                .child(
                    Dialog::new(cx)
                        .open(!self.b_open.get())
                        .focus_handle(self.a.clone())
                        .child(
                            crate::Button::new("open-b")
                                .track_focus(&self.button)
                                .size(px(20.))
                                .on_click(move |_, window, cx| {
                                    b_open.set(true);
                                    dialog_opened(window, cx);
                                    window.focus(&b, cx);
                                    window.refresh();
                                }),
                        ),
                )
                .child(
                    Dialog::new(cx)
                        .open(self.b_open.get())
                        .layer(1, true)
                        .focus_handle(self.b.clone())
                        .on_ok(move |_, _, _| {
                            confirms.set(confirms.get() + 1);
                            false
                        })
                        .child(div().size(px(20.))),
                )
        }
    }

    fn pair(
        cx: &mut gpui::TestAppContext,
    ) -> (gpui::Entity<Pair>, &mut gpui::VisualTestContext) {
        cx.update(crate::init);
        let (view, cx) = cx.add_window_view(|_, cx| Pair {
            a: cx.focus_handle(),
            button: cx.focus_handle(),
            b: cx.focus_handle(),
            b_open: Rc::new(Cell::new(false)),
            confirms: Rc::new(Cell::new(0)),
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        (view, cx)
    }

    fn enter(cx: &mut gpui::VisualTestContext, is_held: Option<bool>) {
        let keystroke = gpui::Keystroke::parse("enter").unwrap();
        match is_held {
            Some(is_held) => cx.simulate_event(gpui::KeyDownEvent {
                keystroke,
                is_held,
                prefer_character_input: false,
            }),
            None => cx.simulate_event(gpui::KeyUpEvent { keystroke }),
        }
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    /// GitTurtle patch: Enter on a dialog confirms it once, as the key goes
    /// down; a held key's repeats confirm nothing more. Bound to
    /// [`Confirm`], every repeat confirmed again.
    #[gpui::test]
    fn a_held_enter_confirms_a_dialog_once(cx: &mut gpui::TestAppContext) {
        let (view, cx) = pair(cx);
        let (b, b_open, confirms) = view.read_with(cx, |pair, _| {
            (pair.b.clone(), pair.b_open.clone(), pair.confirms.clone())
        });
        b_open.set(true);
        cx.update(|window, cx| {
            window.focus(&b, cx);
            window.draw(cx).clear(cx);
        });
        enter(cx, Some(false));
        assert_eq!(confirms.get(), 1, "a fresh Enter confirms");
        enter(cx, Some(true));
        enter(cx, Some(true));
        enter(cx, None);
        assert_eq!(confirms.get(), 1, "its repeats and release confirm nothing");
        enter(cx, Some(false));
        assert_eq!(confirms.get(), 2, "a fresh press confirms again");
    }

    /// GitTurtle patch: a dialog an Enter press opened takes no Enter until
    /// that press comes up, even a key-down not marked held, as each repeat
    /// arrives under X11; then a fresh Enter confirms it.
    #[gpui::test]
    fn a_dialog_waits_for_the_enter_that_opened_it(cx: &mut gpui::TestAppContext) {
        let (view, cx) = pair(cx);
        let (button, b, confirms) = view.read_with(cx, |pair, _| {
            (pair.button.clone(), pair.b.clone(), pair.confirms.clone())
        });
        cx.update(|window, cx| {
            window.focus(&button, cx);
            window.draw(cx).clear(cx);
        });
        enter(cx, Some(false));
        assert!(cx.update(|window, _| b.is_focused(window)), "the press opened B");
        enter(cx, Some(false));
        enter(cx, Some(false));
        assert_eq!(confirms.get(), 0, "no confirm before the opening press comes up");
        enter(cx, None);
        enter(cx, Some(false));
        assert_eq!(confirms.get(), 1, "a fresh press after it confirms");
    }

    /// A dialog with a field and a Button inside it, as a reviewed write's
    /// confirmation holds its own, and a control outside it.
    struct ButtonDialog {
        dialog: FocusHandle,
        field: FocusHandle,
        outside: FocusHandle,
        button: FocusHandle,
        open: Rc<Cell<bool>>,
        presses: Rc<Cell<usize>>,
        confirms: Rc<Cell<usize>>,
    }

    impl Render for ButtonDialog {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let (presses, confirms) = (self.presses.clone(), self.confirms.clone());
            div()
                .child(div().track_focus(&self.outside).size(px(20.)))
                .child(
                    Dialog::new(cx)
                        .open(self.open.get())
                        .focus_handle(self.dialog.clone())
                        .on_ok(move |_, _, _| {
                            confirms.set(confirms.get() + 1);
                            false
                        })
                        .child(div().track_focus(&self.field).size(px(20.)))
                        .child(
                            crate::Button::new("write")
                                .track_focus(&self.button)
                                .size(px(20.))
                                .on_click(move |_, _, _| presses.set(presses.get() + 1)),
                        ),
                )
        }
    }

    fn button_dialog(
        cx: &mut gpui::TestAppContext,
        open: bool,
    ) -> (gpui::Entity<ButtonDialog>, &mut gpui::VisualTestContext) {
        cx.update(crate::init);
        let (view, cx) = cx.add_window_view(move |_, cx| ButtonDialog {
            dialog: cx.focus_handle(),
            field: cx.focus_handle(),
            outside: cx.focus_handle(),
            button: cx.focus_handle(),
            open: Rc::new(Cell::new(open)),
            presses: Rc::new(Cell::new(0)),
            confirms: Rc::new(Cell::new(0)),
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        (view, cx)
    }

    /// A key-down of `keys` not marked held, as each X11 repeat arrives.
    fn key_down(cx: &mut gpui::VisualTestContext, keys: &str) {
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: gpui::Keystroke::parse(keys).unwrap(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn focus_on(cx: &mut gpui::VisualTestContext, handle: &FocusHandle) {
        cx.update(|window, cx| {
            window.focus(handle, cx);
            window.refresh();
            window.draw(cx).clear(cx);
        });
    }

    /// A held Shift- or Control-Enter that opens a confirmation, as a
    /// one-line field's Enter (which takes either as Enter) opens a reviewed
    /// write's, is an Enter press: under X11, once the modifier comes up,
    /// the held key's next repeat arrives as a plain Enter not marked held,
    /// and the confirmation takes none until the key-up.
    fn a_dialog_opened_by_a_held_modified_enter_waits_for_its_key_up(
        cx: &mut gpui::TestAppContext,
        opener: &str,
    ) {
        let (view, cx) = pair(cx);
        let (a, b, b_open, confirms) = view.read_with(cx, |pair, _| {
            (pair.a.clone(), pair.b.clone(), pair.b_open.clone(), pair.confirms.clone())
        });
        focus_on(cx, &a);
        key_down(cx, opener);
        cx.update(|window, cx| {
            b_open.set(true);
            dialog_opened(window, cx);
        });
        focus_on(cx, &b);
        enter(cx, Some(false));
        enter(cx, Some(false));
        assert_eq!(
            confirms.get(),
            0,
            "{opener}: a confirmation opened while Enter is held took a repeat of that press"
        );
        enter(cx, None);
        enter(cx, Some(false));
        assert_eq!(confirms.get(), 1, "{opener}: a fresh press after its key-up confirms");
    }

    /// GitTurtle patch: a confirmation opened by a held Shift-Enter waits for
    /// its key-up. Left unrecorded, as every modified Enter once was, the
    /// plain repeat after Shift came up confirmed it.
    #[gpui::test]
    fn a_dialog_opened_by_a_held_shift_enter_waits_for_its_key_up(
        cx: &mut gpui::TestAppContext,
    ) {
        a_dialog_opened_by_a_held_modified_enter_waits_for_its_key_up(cx, "shift-enter");
    }

    /// GitTurtle patch: as a Shift-Enter, a Control-Enter.
    #[gpui::test]
    fn a_dialog_opened_by_a_held_control_enter_waits_for_its_key_up(
        cx: &mut gpui::TestAppContext,
    ) {
        a_dialog_opened_by_a_held_modified_enter_waits_for_its_key_up(cx, "ctrl-enter");
    }

    /// GitTurtle patch: a Button focused in a dialog a held Shift-Enter
    /// opened takes no plain repeat of that press, nor does the dialog.
    #[gpui::test]
    fn a_button_in_a_dialog_opened_by_a_held_shift_enter_takes_no_repeat(
        cx: &mut gpui::TestAppContext,
    ) {
        let (view, cx) = button_dialog(cx, false);
        let (outside, button, open, presses, confirms) = view.read_with(cx, |dialog, _| {
            (
                dialog.outside.clone(),
                dialog.button.clone(),
                dialog.open.clone(),
                dialog.presses.clone(),
                dialog.confirms.clone(),
            )
        });
        focus_on(cx, &outside);
        key_down(cx, "shift-enter");
        cx.update(|window, cx| {
            open.set(true);
            dialog_opened(window, cx);
        });
        focus_on(cx, &button);
        enter(cx, Some(false));
        assert_eq!(
            (presses.get(), confirms.get()),
            (0, 0),
            "a Button in a dialog opened while Enter is held took a repeat of that press"
        );
    }

    /// GitTurtle patch: Control going down and up while an Enter that moved
    /// focus to a Button stays held, so that under X11 its repeats arrive as
    /// a Control-Enter and then a plain Enter, none marked held, lets no
    /// repeat press that Button: the Control-Enter does not end the press.
    #[gpui::test]
    fn a_control_tap_during_a_held_enter_lets_no_repeat_press_the_button(
        cx: &mut gpui::TestAppContext,
    ) {
        let (view, cx) = button_dialog(cx, true);
        let (field, button, presses) = view.read_with(cx, |dialog, _| {
            (dialog.field.clone(), dialog.button.clone(), dialog.presses.clone())
        });
        focus_on(cx, &field);
        enter(cx, Some(false));
        focus_on(cx, &button);
        enter(cx, Some(false));
        assert_eq!(presses.get(), 0, "a repeat on the Button focus moved to presses nothing");
        key_down(cx, "ctrl-enter");
        enter(cx, Some(false));
        assert_eq!(
            presses.get(),
            0,
            "a Control tap during the held press let its next repeat press the Button"
        );
    }
}
