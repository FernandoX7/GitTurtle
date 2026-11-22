use crate::{
    ActiveTheme, ElementExt, Placement,
    dialog::{Dialog, DialogMotion},
    input::AnyInputState,
    native_menu::FallbackMenuOverlay,
    notification::{Notification, NotificationList},
    sheet::Sheet,
    tooltip::tooltip_renderer,
    touch_selection::WindowTouchSelectionOverlay,
    window_border,
};
use gpui::{
    App, AppContext, Bounds, Context, DefiniteLength, DispatchPhase, ElementId, Entity,
    FocusHandle, GlobalElementId, InteractiveElement, IntoElement, MouseMoveEvent,
    ParentElement as _, Pixels, Render, RenderOnce, Styled, WeakFocusHandle, Window, canvas, div,
    prelude::FluentBuilder as _,
};
use gpui_base::{TextSelection, TextSelectionScopeId, motion::Presence};
use std::{any::TypeId, cell::Cell, rc::Rc};

pub(crate) fn init(cx: &mut App) {
    gpui_base::Root::register_plugin::<WindowState>(cx, WindowState::new);
}

/// Component-owned window state and presentation; Base owns the actual root.
pub(crate) struct WindowState {
    pub(crate) active_sheet: Option<ActiveSheet>,
    pub(crate) active_dialogs: Vec<ActiveDialog>,
    pub(super) focused_input: Option<AnyInputState>,
    pub notification: Entity<NotificationList>,
    pub(crate) tooltip_overlay: Entity<gpui_base::TooltipOverlay>,
    tooltip_hovered_trigger: Option<GlobalElementId>,
    tooltip_trigger_lifetime: std::rc::Weak<()>,
    /// Bounds of the tooltip trigger last pressed, whose tooltip stays
    /// dismissed until the pointer moves outside them.
    tooltip_pressed_trigger: Option<Bounds<Pixels>>,
    pub(crate) native_menu_overlay: Entity<FallbackMenuOverlay>,
    touch_selection_overlay: Entity<WindowTouchSelectionOverlay>,
    sheet_size: Option<DefiniteLength>,
    /// Source of stable dialog identities, including their rendered subtrees.
    next_dialog_id: usize,
    /// GitTurtle patch: a close asked the window to draw before its next
    /// input and no frame has been drawn since ([`draw_before_next_input`]).
    draw_pending: bool,
}

#[derive(Clone)]
pub(crate) struct ActiveSheet {
    focus_handle: FocusHandle,
    /// The previous focused handle before opening the Sheet.
    previous_focused_handle: Option<WeakFocusHandle>,
    placement: Placement,
    selection_scope: TextSelectionScopeId,
    builder: Rc<dyn Fn(Sheet, &mut Window, &mut App) -> Sheet + 'static>,
}

#[derive(Clone)]
pub(crate) struct ActiveDialog {
    /// Stable for the dialog's lifetime, independently of its stack position.
    id: usize,
    focus_handle: FocusHandle,
    /// The previous focused handle before opening the Dialog.
    previous_focused_handle: Option<WeakFocusHandle>,
    selection_scope: TextSelectionScopeId,
    builder: Rc<dyn Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static>,
    /// Whether closing plays an exit transition, refreshed on every render.
    ///
    /// `DialogEntrance::None`, reduced motion, and a zero theme duration all
    /// close in a single frame.
    exit_enabled: Rc<Cell<bool>>,
    /// Retained after closing until its exit transition finishes.
    pub(crate) closing: bool,
}

impl ActiveDialog {
    pub(crate) fn new(
        id: usize,
        focus_handle: FocusHandle,
        previous_focused_handle: Option<WeakFocusHandle>,
        selection_scope: TextSelectionScopeId,
        builder: impl Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static,
    ) -> Self {
        Self {
            id,
            focus_handle,
            previous_focused_handle,
            selection_scope,
            builder: Rc::new(builder),
            exit_enabled: Rc::new(Cell::new(false)),
            closing: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }
}

/// Keys one dialog's shared entrance and exit transition.
fn dialog_presence_id(id: usize) -> ElementId {
    ElementId::NamedInteger("dialog-presence".into(), id as u64)
}

/// GitTurtle patch: notifies `gpui_base::Root` directly whenever `WindowState`
/// opens or closes a dialog or sheet, so views observing `window.root::<Root>()`
/// (such as `GitTurtle::render`'s modal state check) react in the same effect
/// cycle without waiting for the plugin's deferred observer.
fn notify_root(window: &Window, cx: &mut App) {
    if let Some(root) = window.root::<gpui_base::Root>().flatten() {
        root.update(cx, |_, cx| cx.notify());
    }
}

/// GitTurtle patch: has `window` draw again once the effects of what closed
/// a dialog or sheet have run, before the platform hands it the next input.
///
/// GPUI hit-tests a press against the frame the window last drew, and a
/// window draws when the platform next asks for a frame, or as a key event
/// arrives (`Window::dispatch_key_event` draws a dirty window first), never
/// as a mouse event does. So a modal taken away here stayed in that frame,
/// its backdrop and its Buttons taking every press, until the window drew
/// again: a click on a History row before the Return that closed the
/// branch dialog came up was swallowed by the gone dialog's backdrop, and
/// a click where its Rename… had been pressed it. Drawn here, a press after
/// the close acts on what the window draws without the modal: the control
/// the close gave focus back to and the page beneath. The frame is
/// presented as the platform next asks for one, as any frame drawn there is.
///
/// Closes in one effect cycle draw once ([`WindowState::draw_pending`]): the
/// draw waits one more turn of the cycle, behind every effect queued by the
/// code that closed them, and a frame the window draws first, as for a key
/// event, takes its place. Drawn ahead of the platform's frame, it replaces
/// that frame's draw, except where an effect queued later still changes what
/// the window shows: then that frame draws again.
fn draw_before_next_input(root: &mut WindowState, window: &mut Window, cx: &mut App) {
    if std::mem::replace(&mut root.draw_pending, true) {
        return;
    }
    window.defer(cx, |window, cx| {
        window.defer(cx, |window, cx| {
            let pending = WindowState::try_update(window, cx, |root, _, _| {
                std::mem::take(&mut root.draw_pending)
            });
            if pending == Some(true) {
                window.draw(cx).clear(cx);
            }
        });
    });
}

/// GitTurtle patch: has `window` draw again before its next input once a
/// popup that a key closed has gone: a menu, a [`crate::select::SelectState`]
/// or a color picker's popover, each of which stayed in the frame presses are
/// hit-tested against until the window next drew, as a dialog did
/// ([`draw_before_next_input`]). Deferred, so a close inside an update of
/// [`WindowState`] cannot reenter it; a window without a [`WindowState`] draws
/// nothing.
pub(crate) fn draw_after_popup_close(window: &mut Window, cx: &mut App) {
    window.defer(cx, |window, cx| {
        WindowState::try_update(window, cx, |root, window, cx| {
            draw_before_next_input(root, window, cx)
        });
    });
}

/// Hides this window's tooltip, shown or still waiting out its delay.
///
/// GitTurtle patch: the overlay forgets a tooltip only when its trigger's
/// hover ends, a press hides it, or another trigger replaces it. A
/// trigger that is no longer drawn ends no hover, so an application that
/// moves a control away from under a pointer that stays still cancels the
/// tooltip it asked for here.
pub fn hide_tooltip(window: &Window, cx: &mut App) {
    if let Some(overlay) = WindowState::tooltip_overlay(window, cx) {
        overlay.update(cx, |overlay, cx| overlay.hide(cx));
    }
}

impl WindowState {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            active_sheet: None,
            active_dialogs: Vec::new(),
            focused_input: None,
            notification: cx.new(|cx| NotificationList::new(window, cx)),
            tooltip_overlay: cx
                .new(|_| gpui_base::TooltipOverlay::new().render_with(tooltip_renderer())),
            tooltip_hovered_trigger: None,
            tooltip_trigger_lifetime: std::rc::Weak::new(),
            tooltip_pressed_trigger: None,
            native_menu_overlay: cx.new(|_| FallbackMenuOverlay::new()),
            touch_selection_overlay: cx.new(|cx| WindowTouchSelectionOverlay::new(window, cx)),
            sheet_size: None,
            next_dialog_id: 0,
            draw_pending: false,
        }
    }

    fn entity(window: &Window, cx: &App) -> Option<Entity<Self>> {
        window.root::<gpui_base::Root>()??.read(cx).plugin::<Self>()
    }

    fn allocate_text_selection_scope(&mut self) -> TextSelectionScopeId {
        TextSelectionScopeId::new()
    }

    pub(crate) fn active_text_selection_scope(&self) -> TextSelectionScopeId {
        self.active_dialogs
            .iter()
            .rev()
            .find(|dialog| !dialog.closing)
            .map(|dialog| dialog.selection_scope)
            .or_else(|| {
                self.active_sheet
                    .as_ref()
                    .map(|sheet| sheet.selection_scope)
            })
            .unwrap_or_default()
    }

    pub fn update<F, R>(window: &mut Window, cx: &mut App, f: F) -> R
    where
        F: FnOnce(&mut Self, &mut Window, &mut Context<Self>) -> R,
    {
        let root = Self::entity(window, cx).expect(ROOT_MISSING);

        root.update(cx, |root, cx| f(root, window, cx))
    }

    pub(crate) fn try_read<'a>(window: &Window, cx: &'a App) -> Option<&'a Self> {
        Self::entity(window, cx).map(|root| root.read(cx))
    }

    pub(crate) fn try_update<F, R>(window: &mut Window, cx: &mut App, f: F) -> Option<R>
    where
        F: FnOnce(&mut Self, &mut Window, &mut Context<Self>) -> R,
    {
        let root = Self::entity(window, cx)?;
        Some(root.update(cx, |root, cx| f(root, window, cx)))
    }

    pub fn read<'a>(window: &'a Window, cx: &'a App) -> &'a Self {
        Self::entity(window, cx).expect(ROOT_MISSING).read(cx)
    }

    fn notification_layer(
        root: &Entity<WindowState>,
        window: &Window,
        cx: &App,
    ) -> Option<impl IntoElement + use<>> {
        let active_sheet_placement = root.read(cx).active_sheet.clone().map(|d| {
            crate::sheet::physical_placement(d.placement, gpui_base::LayoutDirection::global(cx))
        });

        let sheet_size = root.read(cx).sheet_size;
        let (mt, mr, mb, ml) = match active_sheet_placement {
            Some(Placement::Top) => (sheet_size, None, None, None),
            Some(Placement::Right) => (None, sheet_size, None, None),
            Some(Placement::Bottom) => (None, None, sheet_size, None),
            Some(Placement::Left) => (None, None, None, sheet_size),
            _ => (None, None, None, None),
        };

        // Keep toasts out from under the system bars and the on-screen keyboard.
        let safe_insets = window_border::window_safe_insets(window);

        Some(
            div()
                .absolute()
                .top(safe_insets.top)
                .right(safe_insets.right)
                .bottom(safe_insets.bottom)
                .left(safe_insets.left)
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .when_some(mt, |this, offset| this.mt(offset))
                        .when_some(mr, |this, offset| this.mr(offset))
                        .when_some(mb, |this, offset| this.mb(offset))
                        .when_some(ml, |this, offset| this.ml(offset))
                        .child(root.read(cx).notification.clone()),
                ),
        )
    }

    fn sheet_layer(
        root: Entity<WindowState>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<impl IntoElement + use<>> {
        if let Some(active_sheet) = root.read(cx).active_sheet.clone() {
            let mut sheet = Sheet::new(window, cx);
            sheet = (active_sheet.builder)(sheet, window, cx);
            sheet.focus_handle = active_sheet.focus_handle.clone();
            sheet.placement = crate::sheet::physical_placement(
                active_sheet.placement,
                gpui_base::LayoutDirection::global(cx),
            );
            sheet.selection_scope = active_sheet.selection_scope;

            let size = sheet.size;

            return Some(
                div()
                    .relative()
                    .child(sheet)
                    .on_prepaint(move |_, _, cx| root.update(cx, |r, _| r.sheet_size = Some(size))),
            );
        }

        None
    }

    fn dialog_layer(
        root: &Entity<WindowState>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<impl IntoElement + use<>> {
        let active_dialogs = root.read(cx).active_dialogs.clone();

        if active_dialogs.is_empty() {
            return None;
        }

        let mut rendered: Vec<(usize, Dialog)> = Vec::with_capacity(active_dialogs.len());
        let mut stack_depth = 0;

        for (index, active_dialog) in active_dialogs.iter().enumerate() {
            let mut dialog = Dialog::new(cx);

            dialog = (active_dialog.builder)(dialog, window, cx);

            // Give the dialog the focus handle, because `dialog` is a temporary value, is not possible to
            // keep the focus handle in the dialog.
            //
            // So we keep the focus handle in the `active_dialog`, this is owned by the `WindowState`.
            dialog.focus_handle = active_dialog.focus_handle.clone();
            dialog.selection_scope = active_dialog.selection_scope;

            dialog.layer_ix = index;
            // A dialog that is leaving must not push the next one down.
            dialog.stack_depth = stack_depth;
            if !active_dialog.closing {
                stack_depth += 1;
            }
            dialog.id = active_dialog.id;

            // GitTurtle patch: a closed dialog leaves immediately rather than
            // staying mounted behind a closing click-swallowing shield; see
            // `draw_before_next_input`.
            active_dialog.exit_enabled.set(false);

            let sample =
                Presence::new(dialog_presence_id(active_dialog.id), !active_dialog.closing)
                    .transition(dialog.motion_transition(cx))
                    .sample(window, cx);
            if active_dialog.closing && !sample.should_render() {
                Self::schedule_dialog_removal(root, active_dialog.id, window, cx);
                continue;
            }
            dialog.motion = Some(DialogMotion {
                visibility: sample.progress,
                closing: active_dialog.closing,
            });

            rendered.push((index, dialog));
        }

        if rendered.is_empty() {
            return None;
        }

        // The overlay belongs to the newest dialog still open. With none left,
        // the newest closing dialog keeps the dim through its exit so the
        // backdrop fades with the surface instead of switching off at once.
        // A closing dialog below an open overlay never paints its own backdrop:
        // the constant dim beneath it hands over without a flash.
        let overlay_slot = rendered
            .iter()
            .enumerate()
            .filter(|(_, (index, dialog))| !active_dialogs[*index].closing && dialog.has_overlay())
            .map(|(slot, _)| slot)
            .next_back()
            .or_else(|| {
                rendered
                    .iter()
                    .rposition(|(_, dialog)| dialog.has_overlay())
            });
        if let Some(slot) = overlay_slot {
            rendered[slot].1.props.overlay_visible = true;
        }

        let closing_scopes: Vec<_> = active_dialogs
            .iter()
            .filter(|dialog| dialog.closing)
            .map(|dialog| dialog.focus_handle.clone())
            .collect();

        // Named so a test can assert the layer actually reached the screen. A
        // dialog that opens into a root which never renders this layer looks
        // exactly like one that does not open.
        Some(
            div()
                .debug_selector(|| "dialog-layer".to_string())
                .children(
                    rendered
                        .into_iter()
                        .map(|(_, dialog)| div().id(("dialog-instance", dialog.id)).child(dialog)),
                )
                .when(!closing_scopes.is_empty(), |layer| {
                    layer.on_prepaint(move |_, window, cx| {
                        // Skip every exiting scope, including when close-all
                        // leaves several old tab groups mounted at once.
                        let mut visited = Vec::new();
                        while closing_scopes
                            .iter()
                            .any(|scope| scope.contains_focused(window, cx))
                        {
                            let current = window.focused(cx);
                            if visited.contains(&current) {
                                window.blur(cx);
                                break;
                            }
                            visited.push(current);
                            if window.modifiers().shift {
                                window.focus_prev(cx);
                            } else {
                                window.focus_next(cx);
                            }
                        }
                    })
                }),
        )
    }

    /// Unmounts a dialog once its exit transition has finished.
    ///
    /// The removal is deferred because the layer runs while the window state
    /// that owns the dialog is rendering, and removing it mid-render would
    /// update an entity that is still on the stack.
    fn schedule_dialog_removal(
        root: &Entity<WindowState>,
        dialog_id: usize,
        window: &mut Window,
        cx: &mut App,
    ) {
        let root = root.clone();
        window.defer(cx, move |window, cx| {
            let removed = root.update(cx, |state, cx| {
                let before = state.active_dialogs.len();
                state
                    .active_dialogs
                    .retain(|dialog| !(dialog.closing && dialog.id == dialog_id));
                if state.active_dialogs.len() != before {
                    cx.notify();
                    true
                } else {
                    false
                }
            });
            if removed {
                notify_root(window, cx);
            }
        });
    }

    pub fn open_dialog<F>(
        &mut self,
        build: F,
        window: &mut Window,
        cx: &mut Context<'_, WindowState>,
    ) where
        F: Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static,
    {
        // GitTurtle patch: every close gives focus back as the dialog goes
        // (`Dialog`'s close request), so a dialog opened after another closed
        // takes the focus that close gave back, or a control focused since,
        // and needs no pending restore carried over from the closed one.
        let previous_focused_handle = window.focused(cx).map(|h| h.downgrade());

        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        // GitTurtle patch: a dialog opened by an Enter press waits for that
        // press to come up before it takes Enter.
        gpui_base::dialog_opened(window, cx);

        let id = self.next_dialog_id;
        self.next_dialog_id += 1;
        let selection_scope = self.allocate_text_selection_scope();
        self.active_dialogs.push(ActiveDialog::new(
            id,
            focus_handle,
            previous_focused_handle,
            selection_scope,
            build,
        ));
        // Opening a modal confines selection to it; drop any background
        // selection so it cannot linger (or be copied) under the modal.
        gpui_base::TextSelection::clear(window, cx);
        cx.notify();
        notify_root(window, cx);
    }

    /// GitTurtle patch: a dialog that recorded one of `menus` as its opener
    /// gives focus back to `opener` as it closes: those menus are going, and
    /// `opener` is the control that opened them
    /// (`PopupMenu::return_dialogs_to_opener`). Any other dialog keeps the
    /// opener it recorded.
    pub(crate) fn return_dialogs_to(&mut self, menus: &[FocusHandle], opener: &FocusHandle) {
        for dialog in &mut self.active_dialogs {
            let from_menu = dialog
                .previous_focused_handle
                .as_ref()
                .is_some_and(|previous| menus.iter().any(|menu| *previous == *menu));
            if from_menu {
                dialog.previous_focused_handle = Some(opener.downgrade());
            }
        }
    }

    /// Closes the newest dialog that is not already closing.
    ///
    /// A dialog with an exit transition stays in `active_dialogs`, marked
    /// `closing`, until [`WindowState::dialog_layer`] unmounts it once the
    /// transition ends. Without one, it is removed as before.
    fn close_dialog_internal(&mut self, window: &mut Window, cx: &mut App) -> Option<FocusHandle> {
        self.focused_input = None;

        let ix = self
            .active_dialogs
            .iter()
            .rposition(|dialog| !dialog.closing)?;
        let previous_focused_handle = self.active_dialogs[ix]
            .previous_focused_handle
            .clone()
            .and_then(|handle| handle.upgrade());

        if self.active_dialogs[ix].exit_enabled.get() {
            Self::begin_dialog_exit(&mut self.active_dialogs[ix], window, cx);
        } else {
            self.active_dialogs.remove(ix);
        }

        previous_focused_handle
    }

    fn begin_dialog_exit(dialog: &mut ActiveDialog, window: &mut Window, cx: &mut App) {
        if dialog.closing {
            return;
        }
        dialog.closing = true;
        // Do not leave the input handler active during deferred focus restoration.
        if dialog.focus_handle.contains_focused(window, cx) {
            window.blur(cx);
        }
    }

    pub fn close_dialog(&mut self, window: &mut Window, cx: &mut Context<'_, WindowState>) {
        let closing = self.active_dialogs.iter().any(|dialog| !dialog.closing);
        // GitTurtle patch: an Enter still down waits for its key-up, so its
        // repeats press nothing this close gives focus to.
        if closing {
            gpui_base::dialog_closing(window, cx);
        }
        if let Some(handle) = self.close_dialog_internal(window, cx) {
            window.focus(&handle, cx);
        }
        gpui_base::TextSelection::clear(window, cx);
        cx.notify();
        notify_root(window, cx);
        if closing {
            draw_before_next_input(self, window, cx);
        }
    }

    pub fn close_all_dialogs(&mut self, window: &mut Window, cx: &mut Context<'_, WindowState>) {
        self.focused_input = None;
        let closing = self.active_dialogs.iter().any(|dialog| !dialog.closing);
        // GitTurtle patch: as `close_dialog` waits for an Enter still down.
        if closing {
            gpui_base::dialog_closing(window, cx);
        }
        let previous_focused_handle = self
            .active_dialogs
            .first()
            .and_then(|d| d.previous_focused_handle.clone());

        // Dialogs with an exit transition leave by animating; the rest go now.
        self.active_dialogs
            .retain(|dialog| dialog.closing || dialog.exit_enabled.get());
        for dialog in &mut self.active_dialogs {
            Self::begin_dialog_exit(dialog, window, cx);
        }

        if let Some(handle) = previous_focused_handle.and_then(|h| h.upgrade()) {
            window.focus(&handle, cx);
        }
        gpui_base::TextSelection::clear(window, cx);
        cx.notify();
        notify_root(window, cx);
        if closing {
            draw_before_next_input(self, window, cx);
        }
    }

    pub fn open_sheet_at<F>(
        &mut self,
        placement: Placement,
        build: F,
        window: &mut Window,
        cx: &mut Context<'_, WindowState>,
    ) where
        F: Fn(Sheet, &mut Window, &mut App) -> Sheet + 'static,
    {
        let previous_focused_handle = self
            .active_sheet
            .take()
            .and_then(|s| s.previous_focused_handle)
            .or_else(|| window.focused(cx).map(|h| h.downgrade()));

        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        let selection_scope = self.allocate_text_selection_scope();
        self.active_sheet = Some(ActiveSheet {
            focus_handle,
            previous_focused_handle,
            placement,
            selection_scope,
            builder: Rc::new(build),
        });
        // Opening a modal confines selection to it; drop any background
        // selection so it cannot linger (or be copied) under the modal.
        gpui_base::TextSelection::clear(window, cx);
        cx.notify();
        notify_root(window, cx);
    }

    pub fn close_sheet(&mut self, window: &mut Window, cx: &mut Context<'_, WindowState>) {
        self.focused_input = None;
        if self.active_sheet.is_some() {
            draw_before_next_input(self, window, cx);
        }
        if let Some(previous_handle) = self
            .active_sheet
            .as_ref()
            .and_then(|s| s.previous_focused_handle.as_ref())
            .and_then(|h| h.upgrade())
        {
            window.focus(&previous_handle, cx);
        }
        self.active_sheet = None;
        gpui_base::TextSelection::clear(window, cx);
        cx.notify();
        notify_root(window, cx);
    }

    pub fn push_notification(
        &mut self,
        note: impl Into<Notification>,
        window: &mut Window,
        cx: &mut Context<'_, WindowState>,
    ) {
        self.notification
            .update(cx, |view, cx| view.push(note, window, cx));
        cx.notify();
    }

    /// Removes all notifications whose id matches `T`, including ones registered with
    /// either [`Notification::id`] or [`Notification::id1`] (any key).
    pub fn remove_notification<T: Sized + 'static>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, WindowState>,
    ) {
        self.notification.update(cx, |view, cx| {
            view.close_by_type(TypeId::of::<T>(), window, cx);
        });
        cx.notify();
    }

    /// Removes the notification matching the given type and element id (paired with [`Notification::id1`]).
    pub fn remove_notification1<T: Sized + 'static>(
        &mut self,
        key: impl Into<ElementId>,
        window: &mut Window,
        cx: &mut Context<'_, WindowState>,
    ) {
        let key = key.into();
        self.notification.update(cx, |view, cx| {
            view.close((TypeId::of::<T>(), key), window, cx);
        });
        cx.notify();
    }

    pub fn clear_notifications(&mut self, window: &mut Window, cx: &mut Context<'_, WindowState>) {
        self.notification
            .update(cx, |view, cx| view.clear(window, cx));
        cx.notify();
    }

    /// Get the tooltip overlay entity for this window.
    pub(crate) fn tooltip_overlay(
        window: &Window,
        cx: &App,
    ) -> Option<Entity<gpui_base::TooltipOverlay>> {
        let root = Self::entity(window, cx)?;
        Some(root.read(cx).tooltip_overlay.clone())
    }

    /// Only the current trigger may request a hide. Hover listeners run in
    /// reverse paint order, so a new trigger can enter before the old one leaves.
    pub(crate) fn tooltip_overlay_for_trigger(
        trigger: &GlobalElementId,
        lifetime: std::rc::Weak<()>,
        hovered: bool,
        window: &Window,
        cx: &mut App,
    ) -> Option<Entity<gpui_base::TooltipOverlay>> {
        Self::entity(window, cx)?.update(cx, |state, _| {
            if hovered {
                state.tooltip_hovered_trigger = Some(trigger.clone());
                state.tooltip_trigger_lifetime = lifetime;
            } else if state.tooltip_hovered_trigger.as_ref() == Some(trigger) {
                state.tooltip_hovered_trigger = None;
                state.tooltip_trigger_lifetime = std::rc::Weak::new();
            } else {
                return None;
            }
            Some(state.tooltip_overlay.clone())
        })
    }

    fn dismiss_removed_tooltip(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(trigger) = self.tooltip_hovered_trigger.clone() else {
            return;
        };
        let state = cx.weak_entity();
        // Defer until GPUI has retained this frame's element states and dropped
        // absent ones. A reused cached subtree keeps its trigger alive.
        window.defer(cx, move |_, cx| {
            _ = state.update(cx, |state, cx| {
                if state.tooltip_hovered_trigger.as_ref() != Some(&trigger)
                    || state.tooltip_trigger_lifetime.upgrade().is_some()
                {
                    return;
                }
                state.tooltip_hovered_trigger = None;
                state.tooltip_trigger_lifetime = std::rc::Weak::new();
                state
                    .tooltip_overlay
                    .update(cx, |overlay, cx| overlay.hide(cx));
            });
        });
    }

    /// Keeps the tooltip of the trigger at `bounds` dismissed until the
    /// pointer moves outside them.
    pub(crate) fn press_tooltip_trigger(bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        Self::try_update(window, cx, |state, _, cx| {
            state.tooltip_pressed_trigger = Some(bounds);
            cx.notify();
        });
    }

    pub(crate) fn is_tooltip_trigger_pressed(
        bounds: Bounds<Pixels>,
        window: &Window,
        cx: &App,
    ) -> bool {
        Self::try_read(window, cx)
            .is_some_and(|state| state.tooltip_pressed_trigger == Some(bounds))
    }

    /// Ends the press on the first pointer move outside the trigger. It is
    /// window-level because a removed or moved trigger never reports a
    /// hover-leave.
    fn tooltip_press_listener(&self, cx: &Context<Self>) -> Option<impl IntoElement + use<>> {
        let bounds = self.tooltip_pressed_trigger?;
        let state = cx.entity().downgrade();
        Some(canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase != DispatchPhase::Capture || bounds.contains(&event.position) {
                        return;
                    }
                    _ = state.update(cx, |state, cx| {
                        if state.tooltip_pressed_trigger == Some(bounds) {
                            state.tooltip_pressed_trigger = None;
                            cx.notify();
                        }
                    });
                });
            },
        ))
    }

    /// Get the fallback native-menu overlay entity for this window.
    pub(crate) fn native_menu_overlay(
        window: &Window,
        cx: &App,
    ) -> Option<Entity<FallbackMenuOverlay>> {
        let root = Self::entity(window, cx)?;
        Some(root.read(cx).native_menu_overlay.clone())
    }
}

impl gpui_base::RootPlugin for WindowState {
    fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_removed_tooltip(window, cx);
        // GitTurtle patch: this frame draws what a close asked for.
        self.draw_pending = false;
        window.set_rem_size(cx.theme().font_size);
        TextSelection::activate_scope(self.active_text_selection_scope(), window, cx);
    }

    fn style(&self, surface: &mut gpui::Stateful<gpui::Div>, window: &mut Window, cx: &mut App) {
        use gpui::Refineable as _;
        // The surface's background must follow the window's rounded corners.
        let radii = window_border::window_corner_radii(window, cx);
        surface.style().refine(
            &gpui::StyleRefinement::default()
                .font_family(cx.theme().font_family.clone())
                .bg(cx.theme().tokens.background)
                .text_color(cx.theme().foreground)
                .rounded_tl(radii.top_left)
                .rounded_tr(radii.top_right)
                .rounded_bl(radii.bottom_left)
                .rounded_br(radii.bottom_right),
        );
        // Text starts on the right in a right-to-left interface; elements
        // that set their own alignment keep it.
        if gpui_base::LayoutDirection::global(cx).is_rtl() {
            surface.style().text.text_align = Some(gpui::TextAlign::Right);
        }
    }

    fn decorate(
        &self,
        surface: gpui::AnyElement,
        _root: &gpui_base::Root,
        _window: &mut Window,
        _cx: &mut App,
    ) -> impl IntoElement {
        window_border().child(surface)
    }
}

impl Render for WindowState {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .inset_0()
            .child(WindowStateLayers { root: cx.entity() })
            .child(self.touch_selection_overlay.clone())
            .child(self.tooltip_overlay.clone())
            .children(self.tooltip_press_listener(cx))
            .child(self.native_menu_overlay.clone())
    }
}

const ROOT_MISSING: &str =
    "component window state is missing; call gpui_component::init before gpui_kit::open_window";

/// Window-level layers, always mounted once after the application content.
/// Child view caching does not affect their ownership or rendering.
#[derive(IntoElement)]
struct WindowStateLayers {
    root: Entity<WindowState>,
}

impl RenderOnce for WindowStateLayers {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let root = self.root;
        div()
            .absolute()
            .inset_0()
            .debug_selector(|| "root-layers".to_string())
            .children(WindowState::sheet_layer(root.clone(), window, cx))
            .children(WindowState::dialog_layer(&root, window, cx))
            .children(WindowState::notification_layer(&root, window, cx))
    }
}

/// Test input that arrives before the window's next frame, as a press right
/// after a key that closed a popup does.
#[cfg(test)]
pub(crate) mod before_a_frame {
    use gpui::{
        AnyWindowHandle, App, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, MouseButton,
        MouseDownEvent, MouseUpEvent, Pixels, PlatformInput, Point, VisualTestContext,
    };

    /// How many times an event's delivery yields to the effects queued
    /// before it, so all of them have run first.
    const EFFECT_ROUNDS: usize = 32;

    /// A press of `key` going down, or coming up.
    pub(crate) fn key(key: &'static str, down: bool) -> PlatformInput {
        let keystroke = Keystroke::parse(key).expect("a keystroke");
        if down {
            PlatformInput::KeyDown(KeyDownEvent {
                keystroke,
                is_held: false,
                prefer_character_input: false,
            })
        } else {
            PlatformInput::KeyUp(KeyUpEvent { keystroke })
        }
    }

    /// A left click at `at`: its press and its release.
    pub(crate) fn click(at: Point<Pixels>) -> [PlatformInput; 2] {
        [
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: at,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }),
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position: at,
                modifiers: Modifiers::default(),
                click_count: 1,
            }),
        ]
    }

    /// Delivers `events` as a platform hands over input that arrives before
    /// the window's next frame: each once every effect the one before it
    /// queued has run, with no frame drawn between them but those the window
    /// draws itself. A test window otherwise draws after every event.
    pub(crate) fn deliver(cx: &mut VisualTestContext, events: Vec<PlatformInput>) {
        fn next(
            window: AnyWindowHandle,
            mut events: std::vec::IntoIter<PlatformInput>,
            rounds: usize,
            cx: &mut App,
        ) {
            if rounds > 0 {
                cx.defer(move |cx| next(window, events, rounds - 1, cx));
                return;
            }
            let Some(event) = events.next() else {
                return;
            };
            window
                .update(cx, |_, window, cx| window.dispatch_event(event, cx))
                .expect("the window is open");
            cx.defer(move |cx| next(window, events, EFFECT_ROUNDS, cx));
        }
        cx.update(|window, cx| {
            let handle = window.window_handle();
            let mut events = events.into_iter();
            if let Some(first) = events.next() {
                window.dispatch_event(first, cx);
            }
            cx.defer(move |cx| next(handle, events, EFFECT_ROUNDS, cx));
        });
        cx.run_until_parked();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Root;
    use gpui::{
        Bounds, Context, IntoElement, Pixels, Render, StatefulInteractiveElement as _,
        TestAppContext, VisualTestContext, Window, canvas, div, px, size,
    };
    use std::{cell::Cell, rc::Rc};

    struct BottomBarDemo {
        bar: Rc<Cell<Option<Bounds<Pixels>>>>,
    }

    impl Render for BottomBarDemo {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let bar = self.bar.clone();
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(div().flex_1().w_full())
                .child(div().h(px(40.)).w_full().child(
                    canvas(move |bounds, _, _| bar.set(Some(bounds)), |_, _, _, _| {}).size_full(),
                ))
        }
    }

    /// #3857: with every component layer installed (window border, overlay
    /// layers), a `size_full` view still fills the window height.
    #[gpui::test]
    fn size_full_view_fills_window_height(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let bar = Rc::new(Cell::new(None));
        let (_, cx) = cx.add_window_view({
            let bar = bar.clone();
            move |window, cx| {
                let view = cx.new(|_| BottomBarDemo { bar });
                Root::new(view, window, cx)
            }
        });
        cx.simulate_resize(size(px(800.), px(600.)));
        cx.run_until_parked();
        let viewport = cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            window.viewport_size()
        });
        let bounds = bar.get().expect("bar painted");
        assert_eq!(
            bounds.bottom(),
            viewport.height,
            "bar {bounds:?}, viewport {viewport:?}"
        );
    }

    /// A view that counts the frames that draw it.
    struct Counted(Rc<Cell<usize>>);

    impl Render for Counted {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.0.set(self.0.get() + 1);
            div()
        }
    }

    /// GitTurtle patch: two dialogs closed in one handler have the window
    /// draw once before its next input, after every effect the handler
    /// queued, not once for each close and again for the notification the
    /// second close queued after the first one's draw. (A test window draws
    /// whatever is left dirty as its effects end, as the platform's next
    /// frame would.)
    #[gpui::test]
    fn two_closes_in_one_handler_draw_once(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let drawn = Rc::new(Cell::new(0));
        let counted = drawn.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|_| Counted(counted));
            Root::new(view, window, cx)
        });
        cx.update(|window, cx| {
            WindowState::update(window, cx, |root, window, cx| {
                root.open_dialog(|dialog, _, _| dialog, window, cx);
                root.open_dialog(|dialog, _, _| dialog, window, cx);
            })
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let before = drawn.get();
        cx.update(|window, cx| {
            WindowState::update(window, cx, |root, window, cx| {
                root.close_dialog(window, cx);
                root.close_dialog(window, cx);
            })
        });
        assert_eq!(drawn.get() - before, 1, "two closes draw once");
    }

    /// A page whose clicks are counted, holding a dropdown and a right-click
    /// menu whose items count theirs.
    struct MenuPage {
        page: Rc<Cell<usize>>,
        acted: Rc<Cell<usize>>,
    }

    impl Render for MenuPage {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            use crate::{
                button::Button,
                menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
            };
            let (page, dropdown, context) =
                (self.page.clone(), self.acted.clone(), self.acted.clone());
            div()
                .id("page")
                .size_full()
                .on_click(move |_, _, _| page.set(page.get() + 1))
                .child(
                    Button::new("dropdown")
                        .debug_selector(|| "dropdown".into())
                        .label("Open")
                        .dropdown_menu(move |menu, _, _| {
                            let acted = dropdown.clone();
                            menu.item(
                                PopupMenuItem::new("Act")
                                    .on_click(move |_, _, _| acted.set(acted.get() + 1)),
                            )
                        }),
                )
                .child(
                    div()
                        .id("row")
                        .debug_selector(|| "row".into())
                        .mt(px(40.))
                        .w(px(200.))
                        .h(px(28.))
                        .context_menu(move |menu, _, _| {
                            let acted = context.clone();
                            menu.item(
                                PopupMenuItem::new("Act")
                                    .on_click(move |_, _, _| acted.set(acted.get() + 1)),
                            )
                        }),
                )
        }
    }

    const ACT: &str = "menu-item: Act · MenuItem";

    fn settle(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }

    /// GitTurtle patch: a click given right after the key that closes a
    /// dropdown or a right-click menu, before the window's next frame, lands
    /// on the page under where the menu was and never on the gone menu,
    /// before or after the key's release and whichever key closed it.
    /// GPUI hit-tests a press against the frame the window last drew, and a
    /// key closing a menu left it in that frame until the next one.
    #[gpui::test]
    fn a_click_after_the_key_that_closes_a_menu_lands_on_the_page(cx: &mut TestAppContext) {
        use super::before_a_frame::{click, deliver, key};
        use gpui::{Modifiers, MouseButton};

        cx.update(crate::init);
        let page = Rc::new(Cell::new(0));
        let acted = Rc::new(Cell::new(0));
        let (_, cx) = cx.add_window_view({
            let (page, acted) = (page.clone(), acted.clone());
            move |window, cx| {
                let view = cx.new(|_| MenuPage { page, acted });
                Root::new(view, window, cx)
            }
        });
        settle(cx);
        let dropdown = cx.debug_bounds("dropdown").expect("the dropdown").center();
        let row = cx.debug_bounds("row").expect("the row").center();

        for right_click in [false, true] {
            for (closing, chosen) in [("escape", 0), ("enter", 1)] {
                for key_up_first in [false, true] {
                    let at = format!(
                        "{} closed by {closing}, the click {} its release",
                        if right_click {
                            "a right-click menu"
                        } else {
                            "a dropdown"
                        },
                        if key_up_first { "after" } else { "before" },
                    );
                    if right_click {
                        cx.simulate_mouse_down(row, MouseButton::Right, Modifiers::default());
                        cx.simulate_mouse_up(row, MouseButton::Right, Modifiers::default());
                    } else {
                        cx.simulate_click(dropdown, Modifiers::default());
                    }
                    settle(cx);
                    let item = cx
                        .debug_bounds(ACT)
                        .unwrap_or_else(|| panic!("{at}: the menu opened"))
                        .center();
                    // The keyboard's cursor, which Enter acts on.
                    cx.simulate_keystrokes("down");
                    settle(cx);

                    let (page_before, acted_before) = (page.get(), acted.get());
                    let [press, release] = click(item);
                    let (down, up) = (key(closing, true), key(closing, false));
                    deliver(
                        cx,
                        if key_up_first {
                            vec![down, up, press, release]
                        } else {
                            vec![down, press, release, up]
                        },
                    );
                    settle(cx);

                    assert!(cx.debug_bounds(ACT).is_none(), "{at}: the menu closed");
                    assert_eq!(page.get(), page_before + 1, "{at}: the page took the click");
                    assert_eq!(
                        acted.get(),
                        acted_before + chosen,
                        "{at}: the gone item took no click"
                    );
                }
            }
        }
    }

    gpui::actions!(root_tests, [OpenBodyDialog]);

    /// Opens a dialog that draws `dialog-body`.
    fn open_body_dialog(window: &mut Window, cx: &mut App) {
        use crate::WindowExt as _;
        window.open_dialog(cx, |dialog, _, _| {
            dialog.child(
                div()
                    .debug_selector(|| "dialog-body".into())
                    .size(px(10.)),
            )
        });
    }

    /// A page with a focusable field, a dropdown on a focus handle of its own
    /// and one on the Button's keyed handle, and a right-click row. Every
    /// menu offers a dialog from a handler, from a dispatched action and from
    /// a submenu's item, a dialog from a handler that first moves focus, and
    /// an item that opens nothing.
    struct DialogMenus {
        field: FocusHandle,
        trigger: FocusHandle,
        keyed_shown: bool,
    }

    fn dialog_menu_items(
        menu: crate::menu::PopupMenu,
        field: FocusHandle,
        window: &mut Window,
        cx: &mut Context<crate::menu::PopupMenu>,
    ) -> crate::menu::PopupMenu {
        use crate::menu::PopupMenuItem;
        menu.item(
            PopupMenuItem::new("Handler").on_click(|_, window, cx| open_body_dialog(window, cx)),
        )
        .menu("Dispatched", Box::new(OpenBodyDialog))
        .item(PopupMenuItem::new("Nothing").on_click(|_, _, _| {}))
        .item(PopupMenuItem::new("Moved").on_click(move |_, window, cx| {
            field.focus(window, cx);
            open_body_dialog(window, cx);
        }))
        .submenu("More", window, cx, |menu, _, _| {
            menu.item(
                PopupMenuItem::new("Nested").on_click(|_, window, cx| open_body_dialog(window, cx)),
            )
        })
    }

    impl Render for DialogMenus {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            use crate::{
                button::Button,
                menu::{ContextMenuExt as _, DropdownMenu as _},
            };
            let (own, keyed, untabbed, row) = (
                self.field.clone(),
                self.field.clone(),
                self.field.clone(),
                self.field.clone(),
            );
            div()
                .id("page")
                .size_full()
                .on_action(|_: &OpenBodyDialog, window, cx| open_body_dialog(window, cx))
                .child(
                    div()
                        .id("field")
                        .w(px(100.))
                        .h(px(20.))
                        .track_focus(&self.field),
                )
                .child(
                    Button::new("own")
                        .debug_selector(|| "own".into())
                        .label("Own")
                        .track_focus(&self.trigger)
                        .dropdown_menu(move |menu, window, cx| {
                            dialog_menu_items(menu, own.clone(), window, cx)
                        }),
                )
                .when(self.keyed_shown, |this| {
                    this.child(
                        Button::new("keyed")
                            .debug_selector(|| "keyed".into())
                            .label("Keyed")
                            .dropdown_menu(move |menu, window, cx| {
                                dialog_menu_items(menu, keyed.clone(), window, cx)
                            }),
                    )
                })
                .child(
                    Button::new("untabbed")
                        .debug_selector(|| "untabbed".into())
                        .label("Untabbed")
                        .tab_stop(false)
                        .dropdown_menu(move |menu, window, cx| {
                            dialog_menu_items(menu, untabbed.clone(), window, cx)
                        }),
                )
                .child(
                    div()
                        .id("row")
                        .debug_selector(|| "row".into())
                        .mt(px(40.))
                        .w(px(200.))
                        .h(px(28.))
                        .context_menu(move |menu, window, cx| {
                            dialog_menu_items(menu, row.clone(), window, cx)
                        }),
                )
        }
    }

    const HANDLER: &str = "menu-item: Handler · MenuItem";
    const DISPATCHED: &str = "menu-item: Dispatched · MenuItem";
    const NOTHING: &str = "menu-item: Nothing · MenuItem";
    const NESTED: &str = "menu-item: Nested · MenuItem";

    fn dialog_menus(cx: &mut TestAppContext) -> (Entity<DialogMenus>, &mut VisualTestContext) {
        cx.update(|cx| {
            crate::init(cx);
            cx.set_reduce_motion(true);
        });
        let slot = Rc::new(std::cell::RefCell::new(None));
        let (_, cx) = cx.add_window_view({
            let slot = slot.clone();
            move |window, cx| {
                let view = cx.new(|cx| DialogMenus {
                    field: cx.focus_handle(),
                    trigger: cx.focus_handle(),
                    keyed_shown: true,
                });
                *slot.borrow_mut() = Some(view.clone());
                Root::new(view, window, cx)
            }
        });
        settle(cx);
        let view = slot.borrow().clone().expect("the page");
        (view, cx)
    }

    fn dialog_open(cx: &mut VisualTestContext) -> bool {
        cx.debug_bounds("dialog-body").is_some()
    }

    fn focus(handle: &FocusHandle, cx: &mut VisualTestContext) {
        cx.update(|window, cx| handle.focus(window, cx));
        settle(cx);
    }

    fn focused(handle: &FocusHandle, cx: &mut VisualTestContext) -> bool {
        cx.update(|window, _| handle.is_focused(window))
    }

    /// A whole press of Enter: a dialog it opens takes no Enter, and leaves
    /// none to the window behind it, until the key comes up.
    fn enter(cx: &mut VisualTestContext) {
        let keystroke = gpui::Keystroke::parse("enter").expect("a keystroke");
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(gpui::KeyUpEvent { keystroke });
        settle(cx);
    }

    /// The keyboard's cursor down onto the menu's `index`th row, and Enter.
    fn choose_by_key(index: usize, cx: &mut VisualTestContext) {
        for _ in 0..=index {
            cx.simulate_keystrokes("down");
            settle(cx);
        }
        enter(cx);
    }

    fn press(cx: &mut VisualTestContext, selector: &'static str) {
        let at = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is drawn"))
            .center();
        cx.simulate_click(at, gpui::Modifiers::default());
        settle(cx);
    }

    fn right_press(cx: &mut VisualTestContext, selector: &'static str) {
        use gpui::{Modifiers, MouseButton};
        let at = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is drawn"))
            .center();
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
        settle(cx);
    }

    /// Closes the open dialog with Escape, or as a footer's button does.
    #[track_caller]
    fn close_the_dialog(at: &str, escape: bool, cx: &mut VisualTestContext) {
        use crate::WindowExt as _;
        assert!(dialog_open(cx), "{at}: the item opened its dialog");
        if escape {
            cx.simulate_keystrokes("escape");
        } else {
            cx.update(|window, cx| window.close_dialog(cx));
        }
        settle(cx);
        assert!(!dialog_open(cx), "the dialog closed");
    }

    /// GitTurtle patch: a dialog a menu item opens gives focus back, on
    /// Escape or close, to the control that opened the menu: a dropdown's
    /// trigger, whichever input opened it and wherever focus was before, a
    /// right-click menu's element, where focus was as it was pressed, and for
    /// a submenu the top menu's. An item's handler runs on the menu, as ever,
    /// so the dialog recorded the menu's focus, and the menu went as the item
    /// finished: closing focused nothing, and keys reached nothing until a
    /// click.
    #[gpui::test]
    fn a_dialog_from_a_menu_item_returns_focus_to_what_opened_the_menu(cx: &mut TestAppContext) {
        let (view, cx) = dialog_menus(cx);
        let (field, trigger) =
            view.read_with(cx, |view, _| (view.field.clone(), view.trigger.clone()));

        for escape in [true, false] {
            for (name, selector, index) in [
                ("Handler", Some(HANDLER), 0),
                ("Dispatched", Some(DISPATCHED), 1),
                ("Nested", None, 4),
            ] {
                let at = format!("{name}, escape {escape}");
                let by_key = |cx: &mut VisualTestContext| {
                    if name == "Nested" {
                        for _ in 0..=index {
                            cx.simulate_keystrokes("down");
                            settle(cx);
                        }
                        cx.simulate_keystrokes("right");
                        settle(cx);
                        choose_by_key(0, cx);
                    } else {
                        choose_by_key(index, cx);
                    }
                };

                // A dropdown from the keyboard: focus was on its trigger.
                focus(&trigger, cx);
                enter(cx);
                by_key(cx);
                close_the_dialog(&at, escape, cx);
                assert!(focused(&trigger, cx), "{at}: key-opened dropdown");

                // From the pointer: a Button refuses focus on mouse down, so
                // focus stayed on the field; the trigger still takes it.
                focus(&field, cx);
                press(cx, "own");
                match selector {
                    Some(selector) => press(cx, selector),
                    None => by_key(cx),
                }
                close_the_dialog(&at, escape, cx);
                assert!(focused(&trigger, cx), "{at}: pointer-opened dropdown");

                // A right-click: focus was on the field as it was pressed.
                focus(&field, cx);
                right_press(cx, "row");
                match selector {
                    Some(selector) => press(cx, selector),
                    None => by_key(cx),
                }
                close_the_dialog(&at, escape, cx);
                assert!(focused(&field, cx), "{at}: right-click menu");
            }
        }

        // A pointer's click on a submenu's item, which leaves focus on the
        // parent menu, so the dialog records the parent as its opener.
        focus(&trigger, cx);
        enter(cx);
        for _ in 0..=4 {
            cx.simulate_keystrokes("down");
            settle(cx);
        }
        press(cx, NESTED);
        close_the_dialog("a click in a submenu", true, cx);
        assert!(focused(&trigger, cx), "a click in a submenu");

        // A trigger on the Button's own keyed handle takes focus too: Enter
        // on it opens its menu again.
        focus(&field, cx);
        press(cx, "keyed");
        press(cx, HANDLER);
        close_the_dialog("keyed", true, cx);
        assert!(!focused(&field, cx), "the keyed trigger took focus");
        enter(cx);
        assert!(
            cx.debug_bounds(HANDLER).is_some(),
            "Enter on the trigger opens its menu again"
        );
        cx.simulate_keystrokes("escape");
        settle(cx);
    }

    /// The control that opened the menu gone by the time the dialog closes,
    /// the dialog's fallback stands: it focuses nothing.
    #[gpui::test]
    fn a_dialog_from_a_menu_whose_trigger_went_focuses_nothing(cx: &mut TestAppContext) {
        let (view, cx) = dialog_menus(cx);
        let field = view.read_with(cx, |view, _| view.field.clone());
        focus(&field, cx);
        press(cx, "keyed");
        press(cx, HANDLER);
        view.update(cx, |view, cx| {
            view.keyed_shown = false;
            cx.notify();
        });
        settle(cx);
        settle(cx);
        close_the_dialog("gone", true, cx);
        cx.update(|window, cx| {
            assert!(window.focused(cx).is_none(), "focus falls to nothing");
        });
    }

    /// What a dialog from a menu must not change: an item that opens no
    /// dialog leaves focus where the menu's closing left it, whichever way
    /// it opened; a handler that moves focus before opening a dialog keeps
    /// what it chose; and a dialog opened any other way returns to the focus
    /// it opened on.
    #[gpui::test]
    fn focus_a_menu_item_or_a_plain_dialog_keeps_is_as_ever(cx: &mut TestAppContext) {
        let (view, cx) = dialog_menus(cx);
        let (field, trigger) =
            view.read_with(cx, |view, _| (view.field.clone(), view.trigger.clone()));

        // Opens nothing.
        focus(&trigger, cx);
        enter(cx);
        choose_by_key(2, cx);
        assert!(focused(&trigger, cx), "a key-opened dropdown's Nothing");
        focus(&field, cx);
        press(cx, "own");
        press(cx, NOTHING);
        assert!(focused(&field, cx), "a pointer-opened dropdown's Nothing");
        focus(&field, cx);
        right_press(cx, "row");
        press(cx, NOTHING);
        assert!(focused(&field, cx), "a right-click menu's Nothing");

        // Its handler moved focus to the field first, and the dialog keeps
        // that, as the Linux Main Menu has it.
        focus(&trigger, cx);
        enter(cx);
        choose_by_key(3, cx);
        close_the_dialog("moved", true, cx);
        assert!(focused(&field, cx), "the handler's own choice");

        // A dialog opened with no menu involved.
        focus(&field, cx);
        cx.update(open_body_dialog);
        settle(cx);
        close_the_dialog("plain", true, cx);
        assert!(focused(&field, cx), "opened from the field");
        focus(&trigger, cx);
        cx.update(open_body_dialog);
        settle(cx);
        close_the_dialog("plain", true, cx);
        assert!(focused(&trigger, cx), "opened from the trigger");
    }

    /// GitTurtle patch: a Button that is no Tab stop takes no focus from a
    /// dialog its menu opened, whether or not it is drawn visibly: a row's
    /// menu Button that shows only on hover can be drawn at opacity 0. The
    /// dialog's fallback, and the application's focus repair after it, stand.
    #[gpui::test]
    fn a_dialog_from_a_no_tab_stop_trigger_leaves_it_unfocused(cx: &mut TestAppContext) {
        let (view, cx) = dialog_menus(cx);
        let field = view.read_with(cx, |view, _| view.field.clone());
        focus(&field, cx);
        press(cx, "untabbed");
        press(cx, HANDLER);
        close_the_dialog("no Tab stop", true, cx);
        cx.update(|window, cx| {
            assert!(window.focused(cx).is_none(), "the fallback focuses nothing");
        });
    }

    /// A page for the Enter a dialog's close leaves down: Write, a Button
    /// that counts its presses, a dropdown whose first item is under the
    /// keyboard's cursor as its menu opens, and a right-click row. Each
    /// menu's Handler opens a dialog Enter confirms.
    struct HeldEnterPage {
        write: FocusHandle,
        written: Rc<Cell<usize>>,
        acted: Rc<Cell<usize>>,
    }

    /// Opens a dialog Enter confirms: its own close, or, with `all`, every
    /// dialog's.
    fn open_confirming_dialog(all: bool, window: &mut Window, cx: &mut App) {
        use crate::WindowExt as _;
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .on_ok(move |_, window, cx| {
                    if all {
                        window.close_all_dialogs(cx);
                    }
                    !all
                })
                .child(
                    div()
                        .debug_selector(|| "dialog-body".into())
                        .size(px(10.)),
                )
        });
    }

    impl Render for HeldEnterPage {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            use crate::{
                button::Button,
                menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
            };
            let (written, acted) = (self.written.clone(), self.acted.clone());
            div()
                .id("page")
                .size_full()
                .child(
                    Button::new("write")
                        .debug_selector(|| "write".into())
                        .label("Write")
                        .track_focus(&self.write)
                        .on_click(move |_, _, _| written.set(written.get() + 1)),
                )
                .child(
                    Button::new("more")
                        .debug_selector(|| "more".into())
                        .label("More")
                        .dropdown_menu(move |menu, _, cx| {
                            let acted = acted.clone();
                            let mut menu = menu
                                .item(
                                    PopupMenuItem::new("Act")
                                        .on_click(move |_, _, _| acted.set(acted.get() + 1)),
                                )
                                .item(PopupMenuItem::new("Handler").on_click(|_, window, cx| {
                                    open_confirming_dialog(false, window, cx)
                                }));
                            menu.select_first_item(cx);
                            menu
                        }),
                )
                .child(
                    div()
                        .id("row")
                        .debug_selector(|| "row".into())
                        .mt(px(40.))
                        .w(px(200.))
                        .h(px(28.))
                        .context_menu(|menu, _, _| {
                            menu.item(PopupMenuItem::new("Handler").on_click(|_, window, cx| {
                                open_confirming_dialog(false, window, cx)
                            }))
                        }),
                )
        }
    }

    fn held_enter_page(cx: &mut TestAppContext) -> (Entity<HeldEnterPage>, &mut VisualTestContext) {
        cx.update(|cx| {
            crate::init(cx);
            cx.set_reduce_motion(true);
        });
        let slot = Rc::new(std::cell::RefCell::new(None));
        let (_, cx) = cx.add_window_view({
            let slot = slot.clone();
            move |window, cx| {
                let view = cx.new(|cx| HeldEnterPage {
                    write: cx.focus_handle(),
                    written: Default::default(),
                    acted: Default::default(),
                });
                *slot.borrow_mut() = Some(view.clone());
                Root::new(view, window, cx)
            }
        });
        settle(cx);
        let view = slot.borrow().clone().expect("the page");
        (view, cx)
    }

    /// Enter going down: fresh, or `held` as a repeat macOS and Wayland mark.
    fn enter_down(held: bool, cx: &mut VisualTestContext) {
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: gpui::Keystroke::parse("enter").expect("a keystroke"),
            is_held: held,
            prefer_character_input: false,
        });
        settle(cx);
    }

    fn enter_up(cx: &mut VisualTestContext) {
        cx.simulate_event(gpui::KeyUpEvent {
            keystroke: gpui::Keystroke::parse("enter").expect("a keystroke"),
        });
        settle(cx);
    }

    /// GitTurtle patch: an Enter still down as a dialog closes waits for its
    /// key-up, whichever way the dialog opened and closed. Held past the
    /// repeat delay it pressed the Button the close gave focus back to, a
    /// write the person never aimed at. A fresh press after the key-up acts.
    #[gpui::test]
    fn the_enter_that_closes_a_dialog_presses_nothing_focus_returns_to(cx: &mut TestAppContext) {
        let (view, cx) = held_enter_page(cx);
        let (write, written) =
            view.read_with(cx, |view, _| (view.write.clone(), view.written.clone()));
        for (route, all) in [("a menu", false), ("no menu", false), ("close all", true)] {
            focus(&write, cx);
            if route == "a menu" {
                right_press(cx, "row");
                press(cx, HANDLER);
            } else {
                cx.update(|window, cx| open_confirming_dialog(all, window, cx));
                settle(cx);
            }
            assert!(dialog_open(cx), "{route}: its dialog opened");
            enter_down(false, cx);
            assert!(!dialog_open(cx), "{route}: Enter confirmed it");
            assert!(focused(&write, cx), "{route}: focus went back to Write");
            let before = written.get();
            enter_down(true, cx);
            enter_up(cx);
            assert_eq!(
                written.get(),
                before,
                "{route}: the held Enter and its key-up press nothing"
            );
            enter(cx);
            assert_eq!(written.get(), before + 1, "{route}: a fresh Enter presses");
        }
    }

    /// GitTurtle patch: under X11, whose repeats are not marked held, the
    /// Enter that confirms a dialog from a dropdown's menu neither opens the
    /// dropdown the close gave focus back to nor, on the next repeat, runs
    /// the item under the cursor it opened on. A fresh press opens it.
    #[gpui::test]
    fn an_unmarked_repeat_of_the_enter_that_closes_a_dialog_reopens_no_dropdown(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = held_enter_page(cx);
        let acted = view.read_with(cx, |view, _| view.acted.clone());
        press(cx, "more");
        press(cx, HANDLER);
        assert!(dialog_open(cx), "its dialog opened");
        enter_down(false, cx);
        assert!(!dialog_open(cx), "Enter confirmed it");
        for repeat in 0..3 {
            enter_down(false, cx);
            assert!(
                cx.debug_bounds(ACT).is_none(),
                "repeat {repeat} reopens no menu"
            );
        }
        assert_eq!(acted.get(), 0, "no repeat ran the item");
        enter_up(cx);
        enter(cx);
        assert!(cx.debug_bounds(ACT).is_some(), "a fresh Enter opens the menu");
        cx.simulate_keystrokes("escape");
        settle(cx);
    }

    /// GitTurtle patch: opening and closing a dialog notify `gpui_base::Root`
    /// directly so views observing `window.root::<Root>()` react immediately.
    #[gpui::test]
    fn dialog_open_and_close_notify_root(cx: &mut TestAppContext) {
        use crate::WindowExt as _;

        cx.update(|cx| {
            crate::init(cx);
            cx.set_reduce_motion(true);
        });
        let notifications = Rc::new(Cell::new(0usize));
        let counted = notifications.clone();
        let (root, cx) = cx.add_window_view(move |window, cx| {
            let view = cx.new(|_| Counted(Rc::new(Cell::new(0))));
            Root::new(view, window, cx)
        });
        cx.update(|_, cx| {
            cx.observe(&root, move |_, _| {
                counted.set(counted.get() + 1);
            })
            .detach();
        });
        settle(cx);
        let before_open = notifications.get();
        cx.update(|window, cx| window.open_dialog(cx, |dialog, _, _| dialog));
        assert!(
            notifications.get() > before_open,
            "opening a dialog notifies Root"
        );
        let before_close = notifications.get();
        cx.update(|window, cx| window.close_dialog(cx));
        assert!(
            notifications.get() > before_close,
            "closing a dialog notifies Root"
        );
    }
}
