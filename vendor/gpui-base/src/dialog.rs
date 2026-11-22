use crate::TestSupportExt as _;
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};

use gpui::{
    AnyElement, App, ClickEvent, FocusHandle, Global, InteractiveElement as _, IntoElement,
    KeyBinding, KeyDownEvent, KeyUpEvent, MouseButton, NoAction, ParentElement, Pixels,
    RenderOnce, Role, SharedString, StatefulInteractiveElement as _, StyleRefinement, Styled,
    WeakFocusHandle, Window, WindowId, actions, anchored, deferred, div, point,
    prelude::FluentBuilder as _, px,
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
    // or press that Button.
    //
    // Registered once per application, however often `init` runs, as it
    // does for each app a test builds.
    if cx.has_global::<EnterInterceptor>() {
        return;
    }
    cx.set_global(EnterInterceptor);
    cx.intercept_keystrokes(|event, window, cx| {
        let id = window.window_handle().window_id();
        if event.keystroke.key == ENTER {
            // Any Enter going down, a Command-Enter included, cancels the end
            // a key-up in another window scheduled ([`end_released`]).
            cx.default_global::<EnterPress>().key_downs += 1;
        }
        if event.keystroke.key == ENTER && !key_up_withheld(&event.keystroke.modifiers) {
            let focused = window.focused(cx).map(|focus| focus.downgrade());
            let press = cx.default_global::<EnterPress>();
            let repeat = press.released.remove(&id).is_some()
                || (press.down.contains(&id) && press.began.contains_key(&id));
            press.down.insert(id);
            press.began.entry(id).or_insert(focused);
            if repeat {
                press.repeats.insert(id);
            } else {
                press.repeats.remove(&id);
            }
            if repeat && press.blocked.contains(&id) {
                cx.stop_propagation();
            }
        } else if cx
            .try_global::<EnterPress>()
            .is_some_and(|press| press.began.contains_key(&id) || press.released.contains_key(&id))
        {
            // Another key: no repeat of the Enter follows it, and a press
            // whose key-up came before it has ended.
            let press = cx.global_mut::<EnterPress>();
            press.began.remove(&id);
            press.repeats.remove(&id);
            if press.released.remove(&id).is_some() {
                press.blocked.remove(&id);
            }
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
    /// The windows whose latest Enter key-down repeated the press recorded
    /// there ([`enter_press_is_repeat`]), until another key or the press's
    /// end.
    repeats: HashSet<WindowId>,
    /// The windows whose Enter came up while a press was recorded, with the
    /// key-up's number: the press ends once the application next runs a
    /// task ([`end_released`]), unless an Enter goes down first, in that
    /// window or, for a key-up in another window, in any window, as the
    /// press of an input method's repeat follows its release together.
    released: HashMap<WindowId, u64>,
    /// The number of the last Enter key-up that left a press `released`.
    key_ups: u64,
    /// The number of Enter key-downs in every window, a Command-Enter
    /// included: one after a key-up in another window cancels the end that
    /// key-up scheduled ([`end_released`]).
    key_downs: u64,
}

impl Global for EnterPress {}

/// GitTurtle patch: marks the application whose Enter key-downs [`init`]'s
/// interceptor records, so a later `init` registers no second one.
struct EnterInterceptor;

impl Global for EnterInterceptor {}

/// GitTurtle patch: a dialog opened in `window` while Enter is down takes
/// no Enter, neither its default action nor a press of its Buttons, until
/// that Enter comes up ([`note_key_up`]). An opener calls this as it opens
/// the dialog, as the component `Root` does.
pub fn dialog_opened(window: &Window, cx: &mut App) {
    let window = window.window_handle().window_id();
    let press = cx.default_global::<EnterPress>();
    if press.down.contains(&window) || press.released.contains_key(&window) {
        press.blocked.insert(window);
    }
}

/// GitTurtle patch: an Enter still down as a dialog in `window` closes waits
/// for its key-up, as one that opened a dialog does ([`dialog_opened`]): the
/// close gives focus back to a control, and that press's repeats would press
/// it. A closer calls this as it closes the dialog, as the component `Root`
/// does.
pub fn dialog_closing(window: &Window, cx: &mut App) {
    dialog_opened(window, cx);
}

/// GitTurtle patch: Enter coming up in `window` ends the press a dialog
/// waited out, once the application next runs a task, unless an Enter goes
/// down before that, as an input method's repeat follows its release
/// ([`EnterPress::released`]). A dialog's host notes it, and so does
/// whatever holds the window's root, as `Root` does, since an
/// Enter can come up with no dialog open.
pub fn note_key_up(event: &KeyUpEvent, window: &Window, cx: &mut App) {
    if event.keystroke.key != ENTER {
        return;
    }
    let window = window.window_handle().window_id();
    let Some(press) = cx.try_global::<EnterPress>() else {
        return;
    };
    if press.released.contains_key(&window) {
        return;
    }
    if !(press.down.contains(&window)
        || press.blocked.contains(&window)
        || press.began.contains_key(&window))
    {
        debug_assert!(press.down.iter().all(|held| !press.released.contains_key(held)));
        let key_downs = press.key_downs;
        let held: SmallVec<[WindowId; 1]> = press.down.iter().copied().collect();
        for held in held {
            release(held, Some(key_downs), cx);
        }
        return;
    }
    release(window, None, cx);
}

/// GitTurtle patch: Enter came up for the press recorded in `window`, in
/// that window, or in another with `key_downs` the Enter key-downs counted
/// as it came up ([`end_released`]).
fn release(window: WindowId, key_downs: Option<u64>, cx: &mut App) {
    let press = cx.global_mut::<EnterPress>();
    press.down.remove(&window);
    if key_downs.is_none()
        && !press.blocked.contains(&window)
        && !press.began.contains_key(&window)
    {
        return;
    }
    press.key_ups += 1;
    let key_up = press.key_ups;
    press.released.insert(window, key_up);
    cx.spawn(async move |cx| cx.update(|cx| end_released(window, key_up, key_downs, cx)))
        .detach();
}

/// GitTurtle patch: ends the press in `window` whose Enter came up as key-up
/// `key_up`, unless an Enter went down since in that window, or, for a
/// key-up in another window, with `key_downs` the count as it came up, in
/// any window.
fn end_released(window: WindowId, key_up: u64, key_downs: Option<u64>, cx: &mut App) {
    if cx
        .try_global::<EnterPress>()
        .is_some_and(|press| press.released.get(&window) == Some(&key_up))
    {
        let press = cx.global_mut::<EnterPress>();
        press.released.remove(&window);
        if key_downs.is_some_and(|key_downs| key_downs != press.key_downs) {
            press.down.insert(window);
        } else {
            press.blocked.remove(&window);
            press.began.remove(&window);
            press.repeats.remove(&window);
        }
    }
}

/// GitTurtle patch: whether the Enter press going down in `window` began
/// with `focus` focused, or no press is recorded. A control that acts on
/// Enter as the key goes down asks this, so a held Enter acts once, on the
/// control it began on.
pub fn enter_press_began_on(focus: &FocusHandle, window: &Window, cx: &App) -> bool {
    cx.try_global::<EnterPress>()
        .and_then(|press| press.began.get(&window.window_handle().window_id()))
        .is_none_or(|began| began.as_ref().is_some_and(|began| began == focus))
}

/// GitTurtle patch: whether the Enter key-down `window` is dispatching
/// repeats the press recorded there.
pub fn enter_press_is_repeat(window: &Window, cx: &App) -> bool {
    cx.try_global::<EnterPress>()
        .is_some_and(|press| press.repeats.contains(&window.window_handle().window_id()))
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
/// action.
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
    accessibility_label: Option<SharedString>,
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
        dialog_part!($(#[$meta])* $name, $id, false);
    };
    ($(#[$meta:meta])* $name:ident, $id:literal, $occlude:literal) => {
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
                    // The popup sits over the backdrop, whose press dismisses
                    // the dialog, so it keeps presses on itself from falling through.
                    .when($occlude, |this| this.occlude())
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
    "dialog-popup",
    true
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
    trigger: Option<AnyElement>,
    /// The focus node activation dispatches from, filled at render and read
    /// when clicked, so the trigger built earlier reaches it too.
    anchor: Rc<RefCell<Option<FocusHandle>>>,
}

impl DialogClose {
    pub fn new() -> Self {
        Self {
            style: StyleRefinement::default(),
            children: SmallVec::new(),
            trigger: None,
            anchor: Rc::default(),
        }
    }

    /// Styles a button with the accessible name "Close" and cancel activation.
    ///
    /// The supplied button already supports pointer, keyboard, and accessibility
    /// activation. The builder only needs to supply presentation.
    /// The wrapper does not also handle clicks when a trigger is supplied.
    pub fn trigger<E: IntoElement>(mut self, build: impl FnOnce(crate::Button) -> E) -> Self {
        let anchor = self.anchor.clone();
        let button = crate::Button::new("close")
            .accessibility_label("Close")
            .on_click(move |_, window, cx| Self::activate(&anchor, window, cx));
        self.trigger = Some(build(button).into_any_element());
        self
    }

    /// Dispatches [`Cancel`] from the control's own focus node, so it reaches
    /// the dialog the control sits in whatever holds focus at that moment: a
    /// surface that keeps taking focus back (a native web view, an
    /// always-on-top window) would otherwise leave the control inert.
    fn activate(anchor: &RefCell<Option<FocusHandle>>, window: &mut Window, cx: &mut App) {
        // Clone out before dispatching so the dialog's handlers never run
        // while the cell is borrowed.
        let anchor = anchor.borrow().clone();
        match anchor {
            Some(anchor) => anchor.dispatch_action(&Cancel, window, cx),
            None => window.dispatch_action(Box::new(Cancel), cx),
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
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let anchor = window
            .use_keyed_state("dialog-close-anchor", cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        *self.anchor.borrow_mut() = Some(anchor.clone());
        let cell = self.anchor;
        div()
            .id("dialog-close")
            // A zero-size, out-of-flow node that tracks the anchor: it is never
            // hovered, so it takes no focus on its own and does not enter the
            // Tab order, but it sits inside this dialog's dispatch path.
            .child(div().absolute().size_0().track_focus(&anchor))
            .when(self.trigger.is_none(), |this| {
                this.on_click(move |_, window, cx| Self::activate(&cell, window, cx))
            })
            .children(self.trigger)
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
            accessibility_label: None,
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
    /// Names the dialog for accessibility clients (its title, typically).
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
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
                    .test_support()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w(viewport.width)
                    .h(viewport.height)
                    // A popup in normal flow lands centered; `refine_style`
                    // below lets the caller lay the host out differently.
                    .flex()
                    .items_center()
                    .justify_center()
                    .role(self.role)
                    .when_some(self.accessibility_label.clone(), |this, label| {
                        this.aria_label(label)
                    })
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
                            // under X11 too ([`enter_press_is_repeat`]),
                            // and not at all until an Enter that opened the
                            // dialog comes up.
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
                                // Not marked held, nor repeating the recorded
                                // press, as X11 sends a held key's repeats.
                                if !event.is_held
                                    && !enter_press_is_repeat(window, cx)
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

    #[gpui::test]
    fn a_dialog_takes_an_accessible_name(cx: &mut gpui::TestAppContext) {
        struct Named;
        impl Render for Named {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                div().child(
                    Dialog::new(cx)
                        .accessibility_label("Discard draft?")
                        .popup(div().size(px(10.))),
                )
            }
        }
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|_, _| Named);
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let dialog = cx.update(|window, _| {
            crate::test_support::snapshots(window)
                .into_iter()
                .find(|node| node.role() == Some(Role::Dialog))
                .expect("a dialog node")
        });
        assert_eq!(dialog.label(), Some("Discard draft?"));
    }

    #[gpui::test]
    fn close_trigger_supplies_accessible_button(cx: &mut gpui::TestAppContext) {
        use gpui::{Element as _, accesskit, canvas};
        use std::sync::{Arc, Mutex};

        struct Probe(Arc<Mutex<Option<accesskit::Node>>>);
        impl Render for Probe {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let captured = self.0.clone();
                canvas(
                    move |_, window, cx| {
                        DialogClose::new().trigger(|button| {
                            let element = button.render(window, cx).into_element();
                            let mut node = accesskit::Node::new(element.a11y_role().unwrap());
                            element.write_a11y_info(&mut node);
                            *captured.lock().unwrap() = Some(node);
                            element
                        });
                    },
                    |_, _, _, _| {},
                )
            }
        }

        let captured = Arc::new(Mutex::new(None));
        let result = captured.clone();
        let (_, cx) = cx.add_window_view(move |_, _| Probe(captured));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let node = result.lock().unwrap().take().unwrap();
        assert_eq!(node.role(), Role::Button);
        assert_eq!(node.label(), Some("Close"));
        assert!(node.supports_action(accesskit::Action::Click));
    }

    #[gpui::test]
    fn close_trigger_activates_once_and_respects_cancel_veto(cx: &mut gpui::TestAppContext) {
        use gpui::{KeyDownEvent, KeyUpEvent, Keystroke};

        struct Harness {
            focus: FocusHandle,
            button_focus: FocusHandle,
            handle: DialogHandle,
            attempts: Rc<Cell<usize>>,
        }
        impl Render for Harness {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let attempts = self.attempts.clone();
                let button_focus = self.button_focus.clone();
                Dialog::new(cx)
                    .handle(self.handle.clone())
                    .focus_handle(self.focus.clone())
                    .on_cancel(move |_, _, _| {
                        attempts.set(attempts.get() + 1);
                        attempts.get() > 1
                    })
                    .popup(
                        DialogClose::new().trigger(move |button| {
                            button.size(px(100.)).track_focus(&button_focus)
                        }),
                    )
            }
        }

        cx.update(crate::init);
        let handle = DialogHandle::new(true);
        let attempts = Rc::new(Cell::new(0));
        let (view, cx) = cx.add_window_view({
            let handle = handle.clone();
            let attempts = attempts.clone();
            move |_, cx| Harness {
                focus: cx.focus_handle(),
                button_focus: cx.focus_handle(),
                handle,
                attempts,
            }
        });
        let viewport = cx.update(|window, cx| {
            view.read(cx).focus.clone().focus(window, cx);
            window.draw(cx).clear(cx);
            window.viewport_size()
        });
        let popup_center = point(viewport.width / 2., viewport.height / 2.);
        cx.simulate_click(popup_center, Default::default());
        cx.run_until_parked();
        assert_eq!(attempts.get(), 1);
        assert!(handle.is_open(), "on_cancel can veto pointer dismissal");

        cx.update(|window, cx| {
            view.read(cx).button_focus.clone().focus(window, cx);
            window.draw(cx).clear(cx);
        });
        let keystroke = Keystroke::parse("space").unwrap();
        cx.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(KeyUpEvent { keystroke });
        cx.run_until_parked();
        assert_eq!(attempts.get(), 2);
        assert!(!handle.is_open(), "Space uses the same cancel decision");
    }

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

    /// The backdrop sits under the popup, so a press on the popup must not
    /// reach it and dismiss the dialog.
    #[gpui::test]
    fn pressing_the_popup_does_not_dismiss(cx: &mut gpui::TestAppContext) {
        struct Harness(DialogHandle);
        impl Render for Harness {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                Dialog::new(cx)
                    .handle(self.0.clone())
                    .backdrop(div().absolute().size_full())
                    .popup(DialogPopup::new().size(px(100.)))
            }
        }

        cx.update(crate::init);
        let handle = DialogHandle::new(true);
        let (_, cx) = cx.add_window_view({
            let handle = handle.clone();
            move |_, _| Harness(handle)
        });
        let viewport = cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            window.viewport_size()
        });

        cx.simulate_click(
            point(viewport.width / 2., viewport.height / 2.),
            Default::default(),
        );
        assert!(
            handle.is_open(),
            "a press on the popup keeps the dialog open"
        );

        cx.simulate_click(point(px(5.), px(5.)), Default::default());
        assert!(!handle.is_open(), "a press on the backdrop still dismisses");
    }

    #[gpui::test]
    fn the_popup_is_centered_by_default(cx: &mut gpui::TestAppContext) {
        use gpui::{Bounds, canvas};
        use std::cell::Cell;

        struct Harness(Rc<Cell<Bounds<Pixels>>>);
        impl Render for Harness {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let bounds = self.0.clone();
                Dialog::new(cx).popup(
                    canvas(move |popup, _, _| bounds.set(popup), |_, _, _, _| {}).size(px(100.)),
                )
            }
        }

        cx.update(crate::init);
        let bounds = Rc::new(Cell::new(Bounds::default()));
        let (_, cx) = cx.add_window_view({
            let bounds = bounds.clone();
            move |_, _| Harness(bounds)
        });
        let viewport = cx.update(|window, cx| {
            let viewport = window.viewport_size();
            window.draw(cx).clear(cx);
            viewport
        });

        assert_eq!(
            bounds.get().center(),
            point(viewport.width / 2., viewport.height / 2.)
        );
    }

    /// GitTurtle patch: Enter on a focused Button in a dialog presses that
    /// Button, once, as the key goes down: a held key's repeats and its
    /// release press nothing more, and the dialog's default action does not
    /// run.
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
        assert_eq!(
            (presses.get(), confirms.get()),
            (1, 0),
            "Enter presses the Button"
        );
        down(cx, true);
        down(cx, true);
        down(cx, false);
        down(cx, false);
        cx.simulate_event(KeyUpEvent {
            keystroke: enter.clone(),
        });
        assert_eq!(
            (presses.get(), confirms.get()),
            (1, 0),
            "a held Enter, under X11 too, and its release press nothing more"
        );

        focus(cx, |harness| harness.dialog.clone());
        down(cx, false);
        assert_eq!(
            (presses.get(), confirms.get()),
            (1, 1),
            "on the dialog, Confirm"
        );
        cx.simulate_event(KeyUpEvent {
            keystroke: enter.clone(),
        });

        focus(cx, |harness| harness.field.clone());
        down(cx, false);
        assert_eq!(
            (presses.get(), confirms.get()),
            (1, 2),
            "beside the Button, Confirm"
        );

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

    struct Pair {
        a: FocusHandle,
        button: FocusHandle,
        b: FocusHandle,
        b_open: Rc<Cell<bool>>,
        confirms: Rc<Cell<usize>>,
    }

    impl Render for Pair {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let (b, b_open, confirms) =
                (self.b.clone(), self.b_open.clone(), self.confirms.clone());
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

    fn pair(cx: &mut gpui::TestAppContext) -> (gpui::Entity<Pair>, &mut gpui::VisualTestContext) {
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
        enter(cx, Some(false));
        enter(cx, Some(false));
        enter(cx, None);
        assert_eq!(confirms.get(), 1, "its repeats and release confirm nothing");
        enter(cx, Some(false));
        assert_eq!(confirms.get(), 2, "a fresh press confirms again");
    }

    #[gpui::test]
    fn init_again_records_each_enter_once(cx: &mut gpui::TestAppContext) {
        let (view, cx) = pair(cx);
        cx.update(|_, cx| crate::init(cx));
        let (b, b_open, confirms) = view.read_with(cx, |pair, _| {
            (pair.b.clone(), pair.b_open.clone(), pair.confirms.clone())
        });
        b_open.set(true);
        cx.update(|window, cx| {
            window.focus(&b, cx);
            window.draw(cx).clear(cx);
        });
        enter(cx, Some(false));
        assert!(
            !cx.update(|window, cx| enter_press_is_repeat(window, cx)),
            "the press is no repeat of itself"
        );
        assert_eq!(confirms.get(), 1, "the press confirms the dialog once");
        enter(cx, None);
    }

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
        assert!(
            cx.update(|window, _| b.is_focused(window)),
            "the press opened B"
        );
        enter(cx, Some(false));
        enter(cx, Some(false));
        assert_eq!(
            confirms.get(),
            0,
            "no confirm before the opening press comes up"
        );
        enter(cx, None);
        enter(cx, Some(false));
        assert_eq!(confirms.get(), 1, "a fresh press after it confirms");
    }

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

    fn key_down(cx: &mut gpui::VisualTestContext, keys: &'static str) {
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

    fn a_dialog_opened_by_a_held_modified_enter_waits_for_its_key_up(
        cx: &mut gpui::TestAppContext,
        opener: &'static str,
    ) {
        let (view, cx) = pair(cx);
        let (a, b, b_open, confirms) = view.read_with(cx, |pair, _| {
            (
                pair.a.clone(),
                pair.b.clone(),
                pair.b_open.clone(),
                pair.confirms.clone(),
            )
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
        assert_eq!(
            confirms.get(),
            1,
            "{opener}: a fresh press after its key-up confirms"
        );
    }

    #[gpui::test]
    fn a_dialog_opened_by_a_held_shift_enter_waits_for_its_key_up(cx: &mut gpui::TestAppContext) {
        a_dialog_opened_by_a_held_modified_enter_waits_for_its_key_up(cx, "shift-enter");
    }

    #[gpui::test]
    fn a_dialog_opened_by_a_held_control_enter_waits_for_its_key_up(cx: &mut gpui::TestAppContext) {
        a_dialog_opened_by_a_held_modified_enter_waits_for_its_key_up(cx, "ctrl-enter");
    }

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

    #[gpui::test]
    fn a_control_tap_during_a_held_enter_lets_no_repeat_press_the_button(
        cx: &mut gpui::TestAppContext,
    ) {
        let (view, cx) = button_dialog(cx, true);
        let (field, button, presses) = view.read_with(cx, |dialog, _| {
            (
                dialog.field.clone(),
                dialog.button.clone(),
                dialog.presses.clone(),
            )
        });
        focus_on(cx, &field);
        enter(cx, Some(false));
        focus_on(cx, &button);
        enter(cx, Some(false));
        assert_eq!(
            presses.get(),
            0,
            "a repeat on the Button focus moved to presses nothing"
        );
        key_down(cx, "ctrl-enter");
        enter(cx, Some(false));
        assert_eq!(
            presses.get(),
            0,
            "a Control tap during the held press let its next repeat press the Button"
        );
    }

    fn input_method_repeat(cx: &mut gpui::VisualTestContext) {
        let keystroke = gpui::Keystroke::parse("enter").unwrap();
        cx.update(|window, cx| {
            window.dispatch_event(
                gpui::PlatformInput::KeyUp(gpui::KeyUpEvent {
                    keystroke: keystroke.clone(),
                }),
                cx,
            );
        });
        cx.update(|window, cx| {
            window.dispatch_event(
                gpui::PlatformInput::KeyDown(gpui::KeyDownEvent {
                    keystroke,
                    is_held: false,
                    prefer_character_input: false,
                }),
                cx,
            );
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    struct RepeatProbe {
        focus: FocusHandle,
        seen: Rc<RefCell<Vec<bool>>>,
    }

    impl Render for RepeatProbe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let seen = self.seen.clone();
            div()
                .size_full()
                .track_focus(&self.focus)
                .capture_key_up(|event, window, cx| note_key_up(event, window, cx))
                .on_key_down(move |event, window, cx| {
                    if event.keystroke.key == ENTER {
                        seen.borrow_mut().push(enter_press_is_repeat(window, cx));
                    }
                })
        }
    }

    #[gpui::test]
    fn a_key_down_knows_it_repeats_the_enter_press(cx: &mut gpui::TestAppContext) {
        cx.update(crate::init);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let (_, cx) = cx.add_window_view({
            let seen = seen.clone();
            |window, cx| {
                let focus = cx.focus_handle();
                window.focus(&focus, cx);
                RepeatProbe { focus, seen }
            }
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let down = |cx: &mut gpui::VisualTestContext, is_held| {
            enter(cx, Some(is_held));
            seen.borrow()
                .last()
                .copied()
                .expect("the root saw the key-down")
        };
        assert!(!down(cx, false), "the first key-down is the press");
        assert!(down(cx, true), "a held repeat");
        assert!(down(cx, false), "an X11 repeat, not marked held");
        input_method_repeat(cx);
        assert_eq!(
            seen.borrow().last(),
            Some(&true),
            "a release and its press together"
        );
        enter(cx, None);
        assert!(!down(cx, false), "the first key-down after the key-up");
        assert!(down(cx, false), "its X11 repeat");
        cx.simulate_keystrokes("a");
        assert!(!down(cx, false), "the first key-down after another key");
        enter(cx, None);
        assert_eq!(seen.borrow().len(), 7, "every Enter reached the root");
    }

    #[gpui::test]
    fn a_dialog_waits_out_an_input_method_s_repeats(cx: &mut gpui::TestAppContext) {
        let (view, cx) = pair(cx);
        let (button, b, confirms) = view.read_with(cx, |pair, _| {
            (pair.button.clone(), pair.b.clone(), pair.confirms.clone())
        });
        focus_on(cx, &button);
        enter(cx, Some(false));
        assert!(
            cx.update(|window, _| b.is_focused(window)),
            "the press opened B"
        );
        input_method_repeat(cx);
        input_method_repeat(cx);
        input_method_repeat(cx);
        assert_eq!(confirms.get(), 0, "no repeat of the opening press confirms");
        enter(cx, None);
        enter(cx, Some(false));
        assert_eq!(confirms.get(), 1, "a fresh press after its key-up confirms");
    }

    struct Blank(FocusHandle);

    impl Render for Blank {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .track_focus(&self.0)
                .capture_key_up(|event, window, cx| note_key_up(event, window, cx))
        }
    }

    fn blank_window(cx: &mut gpui::TestAppContext) -> gpui::VisualTestContext {
        let other = cx.add_window(|window, cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            Blank(focus)
        });
        gpui::VisualTestContext::from_window(other.into(), cx)
    }

    fn activate(cx: &mut gpui::VisualTestContext) {
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
    }

    fn open_b_with_a_held_enter(
        cx: &mut gpui::VisualTestContext,
        view: &gpui::Entity<Pair>,
    ) -> Rc<Cell<usize>> {
        let (button, b, confirms) = view.read_with(cx, |pair, _| {
            (pair.button.clone(), pair.b.clone(), pair.confirms.clone())
        });
        activate(cx);
        focus_on(cx, &button);
        enter(cx, Some(false));
        assert!(
            cx.update(|window, _| b.is_focused(window)),
            "the press opened B"
        );
        enter(cx, Some(false));
        assert_eq!(confirms.get(), 0, "a repeat confirms nothing");
        confirms
    }

    #[gpui::test]
    fn a_held_enter_waits_out_a_focus_bounce(cx: &mut gpui::TestAppContext) {
        let (view, cx) = pair(cx);
        let confirms = open_b_with_a_held_enter(cx, &view);
        cx.deactivate_window();
        activate(cx);
        enter(cx, Some(false));
        enter(cx, Some(false));
        assert_eq!(
            confirms.get(),
            0,
            "no repeat after the focus came back confirms"
        );
        enter(cx, None);
        enter(cx, Some(false));
        assert_eq!(confirms.get(), 1, "a fresh press after the key-up confirms");
    }

    #[gpui::test]
    fn a_dialog_opened_while_its_window_lost_focus_waits_for_the_key_up(
        cx: &mut gpui::TestAppContext,
    ) {
        let (view, cx) = pair(cx);
        let (a, b, b_open, confirms) = view.read_with(cx, |pair, _| {
            (
                pair.a.clone(),
                pair.b.clone(),
                pair.b_open.clone(),
                pair.confirms.clone(),
            )
        });
        activate(cx);
        focus_on(cx, &a);
        enter(cx, Some(false));
        cx.deactivate_window();
        cx.update(|window, cx| {
            b_open.set(true);
            dialog_opened(window, cx);
        });
        focus_on(cx, &b);
        activate(cx);
        enter(cx, Some(false));
        enter(cx, Some(false));
        assert_eq!(
            confirms.get(),
            0,
            "no repeat of the press that asked for it confirms"
        );
        enter(cx, None);
        enter(cx, Some(false));
        assert_eq!(confirms.get(), 1, "a fresh press after the key-up confirms");
    }

    #[gpui::test]
    fn an_enter_released_in_another_window_ends_its_press(cx: &mut gpui::TestAppContext) {
        let mut other = blank_window(cx);
        let (view, cx) = pair(cx);
        let confirms = open_b_with_a_held_enter(cx, &view);
        activate(&mut other);
        other.update(|window, cx| window.draw(cx).clear(cx));
        other.simulate_event(gpui::KeyUpEvent {
            keystroke: gpui::Keystroke::parse("enter").unwrap(),
        });
        activate(cx);
        enter(cx, Some(false));
        assert_eq!(
            confirms.get(),
            1,
            "the first Enter after the window is active again was taken as a repeat of a press \
             whose key-up came up in another window"
        );
    }

    #[gpui::test]
    fn an_input_method_s_repeat_in_another_window_ends_no_press(cx: &mut gpui::TestAppContext) {
        let mut other = blank_window(cx);
        let (view, cx) = pair(cx);
        let confirms = open_b_with_a_held_enter(cx, &view);
        activate(&mut other);
        other.update(|window, cx| window.draw(cx).clear(cx));
        input_method_repeat(&mut other);
        activate(cx);
        enter(cx, Some(false));
        assert_eq!(
            confirms.get(),
            0,
            "an input method's repeat in another window ended the press, so the next Enter in \
             its window, the key still down, confirmed the dialog the press opened"
        );
        enter(cx, None);
        enter(cx, Some(false));
        assert_eq!(confirms.get(), 1, "a fresh press after the key-up confirms");
    }

    #[gpui::test]
    fn an_input_method_s_repeat_in_another_window_keeps_a_press_held_past_another_key(
        cx: &mut gpui::TestAppContext,
    ) {
        let mut other = blank_window(cx);
        let (view, cx) = pair(cx);
        let (a, b, b_open, confirms) = view.read_with(cx, |pair, _| {
            (
                pair.a.clone(),
                pair.b.clone(),
                pair.b_open.clone(),
                pair.confirms.clone(),
            )
        });
        activate(cx);
        focus_on(cx, &a);
        enter(cx, Some(false));
        key_down(cx, "a");
        activate(&mut other);
        other.update(|window, cx| window.draw(cx).clear(cx));
        input_method_repeat(&mut other);
        activate(cx);
        cx.update(|window, cx| {
            b_open.set(true);
            dialog_opened(window, cx);
        });
        focus_on(cx, &b);
        enter(cx, Some(false));
        assert_eq!(
            confirms.get(),
            0,
            "an input method's repeat in another window cleared the press's mark, so a dialog \
             opened after it took the next Enter, the key still down"
        );
        enter(cx, None);
        enter(cx, Some(false));
        assert_eq!(confirms.get(), 1, "a fresh press after the key-up confirms");
    }

    #[gpui::test]
    fn an_enter_released_where_it_repeated_ends_only_that_window_s_press(
        cx: &mut gpui::TestAppContext,
    ) {
        let mut other = blank_window(cx);
        let (view, cx) = pair(cx);
        let confirms = open_b_with_a_held_enter(cx, &view);
        activate(&mut other);
        other.update(|window, cx| window.draw(cx).clear(cx));
        enter(&mut other, Some(false));
        enter(&mut other, None);
        activate(cx);
        enter(cx, Some(false));
        assert_eq!(
            confirms.get(),
            0,
            "a key-up in another window that recorded a press of its own ended the press where \
             the key first went down, so its next Enter confirmed the dialog the press opened"
        );
        enter(cx, None);
        enter(cx, Some(false));
        assert_eq!(
            confirms.get(),
            1,
            "a fresh press after its own key-up confirms"
        );
    }

    #[gpui::test]
    fn an_enter_released_in_another_application_swallows_one_return(cx: &mut gpui::TestAppContext) {
        let (view, cx) = pair(cx);
        let confirms = open_b_with_a_held_enter(cx, &view);
        cx.deactivate_window();
        activate(cx);
        enter(cx, Some(false));
        assert_eq!(confirms.get(), 0, "the next Return is swallowed");
        enter(cx, None);
        enter(cx, Some(false));
        assert_eq!(confirms.get(), 1, "and the one after its key-up confirms");
    }

    actions!(dialog_tests, [Probe]);

    struct Bound {
        dialog: FocusHandle,
        inside: FocusHandle,
        outside: FocusHandle,
        open: Rc<Cell<bool>>,
        inside_acts: Rc<Cell<usize>>,
        outside_acts: Rc<Cell<usize>>,
        confirms: Rc<Cell<usize>>,
    }

    impl Render for Bound {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let (inside_acts, outside_acts, confirms) = (
                self.inside_acts.clone(),
                self.outside_acts.clone(),
                self.confirms.clone(),
            );
            let (open, dialog) = (self.open.clone(), self.dialog.clone());
            div()
                .capture_key_up(|event, window, cx| note_key_up(event, window, cx))
                .child(
                    div()
                        .key_context("Probe")
                        .track_focus(&self.outside)
                        .size(px(20.))
                        .on_action(move |_: &Probe, window, cx| {
                            outside_acts.set(outside_acts.get() + 1);
                            if !open.get() {
                                open.set(true);
                                dialog_opened(window, cx);
                                window.focus(&dialog, cx);
                                window.refresh();
                            }
                        }),
                )
                .child(
                    Dialog::new(cx)
                        .open(self.open.get())
                        .focus_handle(self.dialog.clone())
                        .on_ok(move |_, _, _| {
                            confirms.set(confirms.get() + 1);
                            false
                        })
                        .child(
                            div()
                                .key_context("Probe")
                                .track_focus(&self.inside)
                                .size(px(20.))
                                .on_action(move |_: &Probe, _, cx| {
                                    inside_acts.set(inside_acts.get() + 1);
                                    cx.propagate();
                                }),
                        ),
                )
        }
    }

    #[gpui::test]
    fn a_held_enter_reaches_nothing_in_or_behind_the_dialog_it_opened(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::init);
        cx.update(|cx| {
            cx.bind_keys([
                KeyBinding::new("enter", Probe, Some("Probe")),
                KeyBinding::new("shift-enter", Probe, Some("Probe")),
            ])
        });
        let (view, cx) = cx.add_window_view(|_, cx| Bound {
            dialog: cx.focus_handle(),
            inside: cx.focus_handle(),
            outside: cx.focus_handle(),
            open: Rc::new(Cell::new(false)),
            inside_acts: Rc::new(Cell::new(0)),
            outside_acts: Rc::new(Cell::new(0)),
            confirms: Rc::new(Cell::new(0)),
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let (inside, outside, open, inside_acts, outside_acts, confirms) =
            view.read_with(cx, |bound, _| {
                (
                    bound.inside.clone(),
                    bound.outside.clone(),
                    bound.open.clone(),
                    bound.inside_acts.clone(),
                    bound.outside_acts.clone(),
                    bound.confirms.clone(),
                )
            });
        let acts = || (outside_acts.get(), inside_acts.get(), confirms.get());

        focus_on(cx, &outside);
        enter(cx, Some(false));
        assert!(open.get(), "the press opened the dialog");
        assert_eq!(acts(), (1, 0, 0));
        focus_on(cx, &inside);
        enter(cx, Some(true));
        enter(cx, Some(true));
        assert_eq!(acts(), (1, 0, 0), "held repeats reach nothing inside");
        enter(cx, Some(false));
        enter(cx, Some(false));
        key_down(cx, "shift-enter");
        assert_eq!(
            acts(),
            (1, 0, 0),
            "repeats as X11 sends them reach nothing inside"
        );
        input_method_repeat(cx);
        input_method_repeat(cx);
        assert_eq!(
            acts(),
            (1, 0, 0),
            "an input method's repeats reach nothing inside"
        );

        open.set(false);
        focus_on(cx, &outside);
        enter(cx, Some(true));
        enter(cx, Some(false));
        input_method_repeat(cx);
        assert_eq!(acts(), (1, 0, 0), "nor behind the dialog once it has gone");

        enter(cx, None);
        enter(cx, Some(false));
        assert_eq!(acts(), (2, 0, 0), "a fresh press after the key-up acts");
        enter(cx, None);
        focus_on(cx, &inside);
        enter(cx, Some(false));
        assert_eq!(acts(), (2, 1, 1), "and inside, the field and the dialog");
    }
}
