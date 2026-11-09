use crate::{
    ActiveTheme, ElementExt, Placement, StyledExt,
    dialog::Dialog,
    input::{AnyInputState, Copy},
    native_menu::FallbackMenuOverlay,
    notification::{Notification, NotificationList},
    sheet::Sheet,
    tooltip::render_tooltip,
    window_border,
};
use gpui::{
    AnyView, App, AppContext, ClipboardItem, Context, DefiniteLength, ElementId, Entity,
    FocusHandle, InteractiveElement, IntoElement, KeyBinding, ParentElement as _, Pixels, Render,
    StyleRefinement, Styled, WeakFocusHandle, Window, actions, div, prelude::FluentBuilder as _,
};
use gpui_base::{TextSelection, TextSelectionLayer, TextSelectionScopeId};
use std::{any::TypeId, rc::Rc};

actions!(root, [Tab, TabPrev]);

const CONTEXT: &str = "Root";
pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", Tab, Some(CONTEXT)),
        KeyBinding::new("shift-tab", TabPrev, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-c", Copy, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-c", Copy, Some(CONTEXT)),
    ]);
}

/// Root is a view for the App window for as the top level view (Must be the first view in the window).
///
/// It is used to manage the Sheet, Dialog, and Notification.
pub struct Root {
    style: StyleRefinement,
    view: AnyView,
    pub(crate) active_sheet: Option<ActiveSheet>,
    pub(crate) active_dialogs: Vec<ActiveDialog>,
    pub(super) focused_input: Option<AnyInputState>,
    pub notification: Entity<NotificationList>,
    pub(crate) tooltip_overlay: Entity<gpui_base::TooltipOverlay>,
    pub(crate) native_menu_overlay: Entity<FallbackMenuOverlay>,
    sheet_size: Option<DefiniteLength>,
    window_shadow_size: Pixels,
    /// Render the Linux CSD `window_border` wrapper.
    bordered: bool,
    window_id: gpui::WindowId,
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
    focus_handle: FocusHandle,
    /// The previous focused handle before opening the Dialog.
    previous_focused_handle: Option<WeakFocusHandle>,
    selection_scope: TextSelectionScopeId,
    builder: Rc<dyn Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static>,
}

impl ActiveDialog {
    pub(crate) fn new(
        focus_handle: FocusHandle,
        previous_focused_handle: Option<WeakFocusHandle>,
        selection_scope: TextSelectionScopeId,
        builder: impl Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static,
    ) -> Self {
        Self {
            focus_handle,
            previous_focused_handle,
            selection_scope,
            builder: Rc::new(builder),
        }
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
/// Closes in one effect cycle draw once ([`Root::draw_pending`]): the draw
/// waits one more turn of the cycle, behind every effect queued by the code
/// that closed them, and a frame the window draws first, as for a key event,
/// takes its place. Drawn ahead of the platform's frame, it replaces that
/// frame's draw, except where an effect queued later still changes what the
/// window shows: then that frame draws again.
fn draw_before_next_input(root: &mut Root, window: &mut Window, cx: &mut App) {
    if std::mem::replace(&mut root.draw_pending, true) {
        return;
    }
    window.defer(cx, |window, cx| {
        window.defer(cx, |window, cx| {
            let pending = Root::try_update(window, cx, |root, _, _| {
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
/// [`Root`] cannot reenter it; a window without a [`Root`] draws nothing.
pub(crate) fn draw_after_popup_close(window: &mut Window, cx: &mut App) {
    window.defer(cx, |window, cx| {
        Root::try_update(window, cx, |root, window, cx| {
            draw_before_next_input(root, window, cx)
        });
    });
}

impl Root {
    /// Clears window-owned text selection synchronously.
    #[deprecated(note = "use gpui_base::TextSelection::clear instead")]
    pub fn clear_text_selection(&mut self, cx: &mut Context<Self>) {
        gpui_base::TextSelection::clear_for_window(self.window_id, cx);
    }

    /// Create a new Root view.
    pub fn new(view: impl Into<AnyView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        #[cfg(all(target_os = "macos", not(test)))]
        gpui_base::install_window_hit_test_forwarder(window);

        Self {
            style: StyleRefinement::default(),
            view: view.into(),
            active_sheet: None,
            active_dialogs: Vec::new(),
            focused_input: None,
            notification: cx.new(|cx| NotificationList::new(window, cx)),
            tooltip_overlay: cx
                .new(|_| gpui_base::TooltipOverlay::new().render_with(render_tooltip)),
            native_menu_overlay: cx.new(|_| FallbackMenuOverlay::new()),
            sheet_size: None,
            window_shadow_size: window_border::SHADOW_SIZE,
            bordered: true,
            window_id: window.window_handle().window_id(),
            draw_pending: false,
        }
    }

    fn allocate_text_selection_scope(&mut self) -> TextSelectionScopeId {
        TextSelectionScopeId::new()
    }

    pub(crate) fn active_text_selection_scope(&self) -> TextSelectionScopeId {
        self.active_dialogs
            .last()
            .map(|dialog| dialog.selection_scope)
            .or_else(|| {
                self.active_sheet
                    .as_ref()
                    .map(|sheet| sheet.selection_scope)
            })
            .unwrap_or_default()
    }

    /// Enable or disable the Linux client-side window border wrapper.
    ///
    /// Defaults to `true`. Use `bordered(false)` for layer-shell fullscreen windows
    /// or other surfaces that should not render GPUI Component's window border.
    pub fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }

    /// Set the window border shadow size for Linux client-side decorations.
    ///
    /// Default: [`window_border::SHADOW_SIZE`]
    pub fn window_shadow_size(mut self, size: impl Into<Pixels>) -> Self {
        self.window_shadow_size = size.into();
        self
    }

    pub fn update<F, R>(window: &mut Window, cx: &mut App, f: F) -> R
    where
        F: FnOnce(&mut Self, &mut Window, &mut Context<Self>) -> R,
    {
        let root = window
            .root::<Root>()
            .flatten()
            .expect("BUG: window first layer should be a gpui_component::Root.");

        root.update(cx, |root, cx| f(root, window, cx))
    }

    pub(crate) fn try_update<F, R>(window: &mut Window, cx: &mut App, f: F) -> Option<R>
    where
        F: FnOnce(&mut Self, &mut Window, &mut Context<Self>) -> R,
    {
        let root = window.root::<Root>().flatten()?;
        Some(root.update(cx, |root, cx| f(root, window, cx)))
    }

    pub fn read<'a>(window: &'a Window, cx: &'a App) -> &'a Self {
        &window
            .root::<Root>()
            .expect("The window root view should be of type `ui::Root`.")
            .unwrap()
            .read(cx)
    }

    // Render Notification layer.
    pub fn render_notification_layer(
        window: &mut Window,
        cx: &mut App,
    ) -> Option<impl IntoElement + use<>> {
        let root = window.root::<Root>()??;

        let active_sheet_placement = root.read(cx).active_sheet.clone().map(|d| d.placement);

        let sheet_size = root.read(cx).sheet_size;
        let (mt, mr, mb, ml) = match active_sheet_placement {
            Some(Placement::Top) => (sheet_size, None, None, None),
            Some(Placement::Right) => (None, sheet_size, None, None),
            Some(Placement::Bottom) => (None, None, sheet_size, None),
            Some(Placement::Left) => (None, None, None, sheet_size),
            _ => (None, None, None, None),
        };

        Some(
            div()
                .absolute()
                .inset_0()
                .when_some(mt, |this, offset| this.mt(offset))
                .when_some(mr, |this, offset| this.mr(offset))
                .when_some(mb, |this, offset| this.mb(offset))
                .when_some(ml, |this, offset| this.ml(offset))
                .child(root.read(cx).notification.clone()),
        )
    }

    /// Render the Sheet layer.
    pub fn render_sheet_layer(
        window: &mut Window,
        cx: &mut App,
    ) -> Option<impl IntoElement + use<>> {
        let root = window.root::<Root>()??;

        if let Some(active_sheet) = root.read(cx).active_sheet.clone() {
            let mut sheet = Sheet::new(window, cx);
            sheet = (active_sheet.builder)(sheet, window, cx);
            sheet.focus_handle = active_sheet.focus_handle.clone();
            sheet.placement = active_sheet.placement;
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

    /// Render the Dialog layer.
    pub fn render_dialog_layer(
        window: &mut Window,
        cx: &mut App,
    ) -> Option<impl IntoElement + use<>> {
        let root = window.root::<Root>()??;

        let active_dialogs = root.read(cx).active_dialogs.clone();

        if active_dialogs.is_empty() {
            return None;
        }

        let mut show_overlay_ix = None;

        let mut dialogs = active_dialogs
            .iter()
            .enumerate()
            .map(|(i, active_dialog)| {
                let mut dialog = Dialog::new(cx);

                dialog = (active_dialog.builder)(dialog, window, cx);

                // Give the dialog the focus handle, because `dialog` is a temporary value, is not possible to
                // keep the focus handle in the dialog.
                //
                // So we keep the focus handle in the `active_dialog`, this is owned by the `Root`.
                dialog.focus_handle = active_dialog.focus_handle.clone();
                dialog.selection_scope = active_dialog.selection_scope;

                dialog.layer_ix = i;
                // Find the dialog which one needs to show overlay.
                if dialog.has_overlay() {
                    show_overlay_ix = Some(i);
                }

                dialog
            })
            .collect::<Vec<_>>();

        if let Some(ix) = show_overlay_ix {
            if let Some(dialog) = dialogs.get_mut(ix) {
                dialog.props.overlay_visible = true;
            }
        }

        // Named so a test can assert the layer actually reached the screen. A
        // dialog that opens into a root which never renders this layer looks
        // exactly like one that does not open.
        Some(
            div()
                .debug_selector(|| "dialog-layer".to_string())
                .children(dialogs),
        )
    }

    pub fn open_dialog<F>(&mut self, build: F, window: &mut Window, cx: &mut Context<'_, Root>)
    where
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

        let selection_scope = self.allocate_text_selection_scope();
        self.active_dialogs.push(ActiveDialog::new(
            focus_handle,
            previous_focused_handle,
            selection_scope,
            build,
        ));
        // Opening a modal confines selection to it; drop any background
        // selection so it cannot linger (or be copied) under the modal.
        gpui_base::TextSelection::clear(window, cx);
        cx.notify();
    }

    fn close_dialog_internal(&mut self) -> Option<FocusHandle> {
        self.focused_input = None;
        self.active_dialogs
            .pop()
            .and_then(|d| d.previous_focused_handle)
            .and_then(|h| h.upgrade())
    }

    pub fn close_dialog(&mut self, window: &mut Window, cx: &mut Context<'_, Root>) {
        let closing = !self.active_dialogs.is_empty();
        if let Some(handle) = self.close_dialog_internal() {
            window.focus(&handle, cx);
        }
        gpui_base::TextSelection::clear(window, cx);
        cx.notify();
        if closing {
            draw_before_next_input(self, window, cx);
        }
    }

    pub fn close_all_dialogs(&mut self, window: &mut Window, cx: &mut Context<'_, Root>) {
        self.focused_input = None;
        let closing = !self.active_dialogs.is_empty();
        let previous_focused_handle = self
            .active_dialogs
            .first()
            .and_then(|d| d.previous_focused_handle.clone());
        self.active_dialogs.clear();
        if let Some(handle) = previous_focused_handle.and_then(|h| h.upgrade()) {
            window.focus(&handle, cx);
        }
        gpui_base::TextSelection::clear(window, cx);
        cx.notify();
        if closing {
            draw_before_next_input(self, window, cx);
        }
    }

    pub fn open_sheet_at<F>(
        &mut self,
        placement: Placement,
        build: F,
        window: &mut Window,
        cx: &mut Context<'_, Root>,
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
    }

    pub fn close_sheet(&mut self, window: &mut Window, cx: &mut Context<'_, Root>) {
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
    }

    pub fn push_notification(
        &mut self,
        note: impl Into<Notification>,
        window: &mut Window,
        cx: &mut Context<'_, Root>,
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
        cx: &mut Context<'_, Root>,
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
        cx: &mut Context<'_, Root>,
    ) {
        let key = key.into();
        self.notification.update(cx, |view, cx| {
            view.close((TypeId::of::<T>(), key), window, cx);
        });
        cx.notify();
    }

    pub fn clear_notifications(&mut self, window: &mut Window, cx: &mut Context<'_, Root>) {
        self.notification
            .update(cx, |view, cx| view.clear(window, cx));
        cx.notify();
    }

    /// Get the tooltip overlay entity for this window.
    pub(crate) fn tooltip_overlay(
        window: &Window,
        cx: &App,
    ) -> Option<Entity<gpui_base::TooltipOverlay>> {
        let root = window.root::<Root>()??;
        Some(root.read(cx).tooltip_overlay.clone())
    }

    /// Hides this window's tooltip, shown or still waiting out its delay.
    ///
    /// GitTurtle patch: the overlay forgets a tooltip only when its trigger's
    /// hover ends, a press hides it, or another trigger replaces it. A
    /// trigger that is no longer drawn ends no hover, so an application that
    /// moves a control away from under a pointer that stays still cancels the
    /// tooltip it asked for here.
    pub fn hide_tooltip(window: &Window, cx: &mut App) {
        if let Some(overlay) = Self::tooltip_overlay(window, cx) {
            overlay.update(cx, |overlay, cx| overlay.hide(cx));
        }
    }

    /// Get the fallback native-menu overlay entity for this window.
    pub(crate) fn native_menu_overlay(
        window: &Window,
        cx: &App,
    ) -> Option<Entity<FallbackMenuOverlay>> {
        let root = window.root::<Root>()??;
        Some(root.read(cx).native_menu_overlay.clone())
    }

    /// Return the root view of the Root.
    pub fn view(&self) -> &AnyView {
        &self.view
    }

    fn on_action_tab(&mut self, _: &Tab, window: &mut Window, cx: &mut Context<Self>) {
        // Check if we're inside a focus trap
        if let Some(container_focus_handle) = gpui_base::active_focus_trap(window, cx) {
            // We're in a focus trap - try to focus next, then check if we're still inside
            let before_focus = window.focused(cx);

            // Try normal focus navigation
            window.focus_next(cx);

            // Check if we're still in the trap
            if !container_focus_handle.contains_focused(window, cx) {
                // We jumped out of the trap - need to cycle back to the beginning
                // Find the first focusable element in the trap by continuing to focus_next
                let mut attempts = 0;
                const MAX_ATTEMPTS: usize = 100; // Prevent infinite loop

                while !container_focus_handle.contains_focused(window, cx)
                    && attempts < MAX_ATTEMPTS
                {
                    window.focus_next(cx);
                    attempts += 1;

                    // If we cycled back to where we started, restore original focus
                    if window.focused(cx) == before_focus {
                        break;
                    }
                }
            }
            return;
        }

        // Normal tab navigation
        window.focus_next(cx);
    }

    fn on_action_tab_prev(&mut self, _: &TabPrev, window: &mut Window, cx: &mut Context<Self>) {
        // Check if we're inside a focus trap
        if let Some(container_focus_handle) = gpui_base::active_focus_trap(window, cx) {
            // We're in a focus trap - try to focus previous, then check if we're still inside
            let before_focus = window.focused(cx);

            // Try normal focus navigation
            window.focus_prev(cx);

            // Check if we're still in the trap
            if !container_focus_handle.contains_focused(window, cx) {
                // We jumped out of the trap - need to cycle back to the end
                // Find the last focusable element in the trap by continuing to focus_prev
                let mut attempts = 0;
                const MAX_ATTEMPTS: usize = 100; // Prevent infinite loop

                while !container_focus_handle.contains_focused(window, cx)
                    && attempts < MAX_ATTEMPTS
                {
                    window.focus_prev(cx);
                    attempts += 1;

                    // If we cycled back to where we started, restore original focus
                    if window.focused(cx) == before_focus {
                        break;
                    }
                }
            }
            return;
        }

        // Normal tab navigation
        window.focus_prev(cx);
    }

    fn on_action_copy(&mut self, _: &Copy, window: &mut Window, cx: &mut Context<Self>) {
        let text = gpui_base::TextSelection::selected_text(window, cx)
            .trim()
            .to_string();
        if text.is_empty() {
            cx.propagate();
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }
}

impl Styled for Root {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_rem_size(cx.theme().font_size);
        // GitTurtle patch: this frame draws what a close asked for.
        self.draw_pending = false;
        let active_scope = self.active_text_selection_scope();
        TextSelection::activate_scope(active_scope, window, cx);

        let inner = div()
            .id("root")
            .key_context(CONTEXT)
            // GitTurtle patch: every Enter that comes up in the window, so a
            // dialog an Enter press opened takes Enter again once it does.
            .capture_key_up(|event, window, cx| gpui_base::note_key_up(event, window, cx))
            .on_action(cx.listener(Self::on_action_tab))
            .on_action(cx.listener(Self::on_action_tab_prev))
            .on_action(cx.listener(Self::on_action_copy))
            .relative()
            .size_full()
            .font_family(cx.theme().font_family.clone())
            .bg(cx.theme().tokens.background)
            .text_color(cx.theme().foreground)
            .refine_style(&self.style)
            .child(TextSelectionLayer)
            .child(self.view.clone())
            .child(self.tooltip_overlay.clone())
            .child(self.native_menu_overlay.clone());

        if self.bordered {
            window_border()
                .shadow_size(self.window_shadow_size)
                .child(inner)
                .into_any_element()
        } else {
            inner.into_any_element()
        }
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
    pub(crate) fn key(key: &str, down: bool) -> PlatformInput {
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
    use gpui::{StatefulInteractiveElement as _, TestAppContext, VisualTestContext, px};

    struct TestView;

    impl Render for TestView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    /// A view that counts the frames that draw it.
    struct Counted(Rc<std::cell::Cell<usize>>);

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
        let drawn = Rc::new(std::cell::Cell::new(0));
        let counted = drawn.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|_| Counted(counted));
            Root::new(view, window, cx).bordered(false)
        });
        cx.update(|window, cx| {
            Root::update(window, cx, |root, window, cx| {
                root.open_dialog(|dialog, _, _| dialog, window, cx);
                root.open_dialog(|dialog, _, _| dialog, window, cx);
            })
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let before = drawn.get();
        cx.update(|window, cx| {
            Root::update(window, cx, |root, window, cx| {
                root.close_dialog(window, cx);
                root.close_dialog(window, cx);
            })
        });
        assert_eq!(drawn.get() - before, 1, "two closes draw once");
    }

    /// A page whose clicks are counted, holding a dropdown and a right-click
    /// menu whose items count theirs.
    struct MenuPage {
        page: Rc<std::cell::Cell<usize>>,
        acted: Rc<std::cell::Cell<usize>>,
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
        let page = Rc::new(std::cell::Cell::new(0));
        let acted = Rc::new(std::cell::Cell::new(0));
        let (_, cx) = cx.add_window_view({
            let (page, acted) = (page.clone(), acted.clone());
            move |window, cx| {
                let view = cx.new(|_| MenuPage { page, acted });
                Root::new(view, window, cx).bordered(false)
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

    #[gpui::test]
    fn bordered_builder_toggles_window_border(cx: &mut TestAppContext) {
        cx.update(crate::init);

        let (default_root, _) = cx.add_window_view(|window, cx| {
            let view = cx.new(|_| TestView);
            Root::new(view, window, cx)
        });
        assert!(default_root.read_with(cx, |root, _| root.bordered));

        let (root, _) = cx.add_window_view(|window, cx| {
            let view = cx.new(|_| TestView);
            Root::new(view, window, cx).bordered(false)
        });
        assert!(!root.read_with(cx, |root, _| root.bordered));

        let (root, _) = cx.add_window_view(|window, cx| {
            let view = cx.new(|_| TestView);
            Root::new(view, window, cx).bordered(false).bordered(true)
        });
        assert!(root.read_with(cx, |root, _| root.bordered));
    }
}
