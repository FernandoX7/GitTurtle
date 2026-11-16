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

    fn close_dialog_internal(&mut self) -> Option<FocusHandle> {
        self.focused_input = None;
        self.active_dialogs
            .pop()
            .and_then(|d| d.previous_focused_handle)
            .and_then(|h| h.upgrade())
    }

    pub fn close_dialog(&mut self, window: &mut Window, cx: &mut Context<'_, Root>) {
        let closing = !self.active_dialogs.is_empty();
        // GitTurtle patch: an Enter still down waits for its key-up, so its
        // repeats press nothing this close gives focus to.
        if closing {
            gpui_base::dialog_closing(window, cx);
        }
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
        // GitTurtle patch: as `close_dialog` waits for an Enter still down.
        if closing {
            gpui_base::dialog_closing(window, cx);
        }
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
        Self::step_focus(true, window, cx);
    }

    fn on_action_tab_prev(&mut self, _: &TabPrev, window: &mut Window, cx: &mut Context<Self>) {
        Self::step_focus(false, window, cx);
    }

    /// Moves focus to the next Tab stop, or with `forward` false the
    /// previous one, inside the active focus trap if there is one.
    ///
    /// GitTurtle patch: a step that would leave the trap moves straight to
    /// the trap's own first stop (Tab past its last) or last stop
    /// (Shift-Tab before its first), and a trap with no stop keeps focus
    /// where it was, so focus never leaves an open dialog and no key reaches
    /// the page behind it, where it could act. Upstream walked the window on
    /// from the stop outside the trap and gave up after 100 stops, leaving
    /// focus on that page with the dialog still open (31 saved themes behind
    /// GitTurtle's theme editor).
    fn step_focus(forward: bool, window: &mut Window, cx: &mut App) {
        let step = |window: &mut Window, cx: &mut App| {
            if forward {
                window.focus_next(cx)
            } else {
                window.focus_prev(cx)
            }
        };
        let Some(trap) = gpui_base::active_focus_trap(window, cx) else {
            step(window, cx);
            return;
        };
        let before = window.focused(cx);
        step(window, cx);
        if trap.contains_focused(window, cx) {
            return;
        }
        if let Some(target) = Self::trap_end(&trap, forward, window, cx).or(before) {
            window.focus(&target, cx);
        }
    }

    /// GitTurtle patch: the first Tab stop inside `trap`, or with `first`
    /// false its last, in the order `focus_next` visits them. GPUI orders a
    /// tracked handle ahead of everything its element holds, so stepping on
    /// from the trap's own handle visits the trap's stops and then one stop
    /// past them, never the rest of the window. It moves focus as it steps,
    /// within this handler, before any frame draws or any listener sees it;
    /// the caller focuses what it returns.
    fn trap_end(
        trap: &FocusHandle,
        first: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<FocusHandle> {
        // The trap's own handle is its first stop when it is a Tab stop.
        let mut ends = trap.tab_stop.then(|| (trap.clone(), trap.clone()));
        window.focus(trap, cx);
        while !(first && ends.is_some()) {
            window.focus_next(cx);
            let Some(focused) = window.focused(cx) else {
                break;
            };
            if !trap.contains_focused(window, cx) {
                break;
            }
            match &mut ends {
                // Back at its first stop, every stop in the window is the
                // trap's; at its last, the step moved nothing.
                Some((head, tail)) if focused == *head || focused == *tail => break,
                Some((_, tail)) => *tail = focused,
                None if focused == *trap => break,
                None => ends = Some((focused.clone(), focused)),
            }
        }
        ends.map(|(head, tail)| if first { head } else { tail })
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
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                .children(Root::render_dialog_layer(window, cx))
        }
    }

    const HANDLER: &str = "menu-item: Handler · MenuItem";
    const DISPATCHED: &str = "menu-item: Dispatched · MenuItem";
    const NOTHING: &str = "menu-item: Nothing · MenuItem";
    const NESTED: &str = "menu-item: Nested · MenuItem";

    fn dialog_menus(cx: &mut TestAppContext) -> (Entity<DialogMenus>, &mut VisualTestContext) {
        cx.update(crate::init);
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
                Root::new(view, window, cx).bordered(false)
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
        written: Rc<std::cell::Cell<usize>>,
        acted: Rc<std::cell::Cell<usize>>,
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
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                .children(Root::render_dialog_layer(window, cx))
        }
    }

    fn held_enter_page(cx: &mut TestAppContext) -> (Entity<HeldEnterPage>, &mut VisualTestContext) {
        cx.update(crate::init);
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
                Root::new(view, window, cx).bordered(false)
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

    /// More stops on each side of a focus trap than upstream's walk took.
    const STOPS_AROUND_THE_TRAP: usize = 150;

    /// A page of [`STOPS_AROUND_THE_TRAP`] Tab stops, a focus trap holding
    /// `inside`, and as many stops again after it.
    struct TrapPage {
        before: Vec<FocusHandle>,
        trap: FocusHandle,
        inside: Vec<FocusHandle>,
        after: Vec<FocusHandle>,
    }

    impl Render for TrapPage {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            use gpui_base::FocusTrapElement as _;
            let stop = |handle: &FocusHandle| div().track_focus(handle).size(px(2.));
            div()
                .size_full()
                .children(self.before.iter().map(stop))
                .child(
                    div()
                        .focus_trap("trap", &self.trap)
                        .children(self.inside.iter().map(stop)),
                )
                .children(self.after.iter().map(stop))
        }
    }

    fn tab_stops(count: usize, cx: &mut App) -> Vec<FocusHandle> {
        (0..count)
            .map(|_| cx.focus_handle().tab_stop(true))
            .collect()
    }

    fn trap_page(
        inside: usize,
        cx: &mut TestAppContext,
    ) -> (Entity<TrapPage>, &mut VisualTestContext) {
        cx.update(crate::init);
        let slot = Rc::new(std::cell::RefCell::new(None));
        let (_, cx) = cx.add_window_view({
            let slot = slot.clone();
            move |window, cx| {
                let view = cx.new(|cx| TrapPage {
                    before: tab_stops(STOPS_AROUND_THE_TRAP, cx),
                    trap: cx.focus_handle(),
                    inside: tab_stops(inside, cx),
                    after: tab_stops(STOPS_AROUND_THE_TRAP, cx),
                });
                *slot.borrow_mut() = Some(view.clone());
                Root::new(view, window, cx).bordered(false)
            }
        });
        settle(cx);
        let view = slot.borrow().clone().expect("the page");
        (view, cx)
    }

    /// GitTurtle patch: Tab and Shift-Tab never take focus out of a focus
    /// trap, however many stops lie around it: Tab from its last stop moves
    /// to its first, Shift-Tab from its first to its last, and inside it
    /// each moves one stop, also from the trap's own handle, which a dialog
    /// focuses as it opens. Upstream walked the window from the stop outside
    /// the trap and gave up after 100 stops, leaving focus on the page behind
    /// an open dialog (31 saved themes behind GitTurtle's theme editor).
    #[gpui::test]
    fn tab_wraps_inside_a_focus_trap_past_any_number_of_stops(cx: &mut TestAppContext) {
        let (view, cx) = trap_page(3, cx);
        let (trap, inside) = view.read_with(cx, |view, _| (view.trap.clone(), view.inside.clone()));
        for (key, to) in [("tab", 0), ("shift-tab", 2)] {
            focus(&trap, cx);
            assert!(focused(&trap, cx), "the trap holds focus");
            cx.simulate_keystrokes(key);
            settle(cx);
            assert!(
                focused(&inside[to], cx),
                "{key} from the trap's own handle reaches its stop {to}"
            );
        }
        focus(&inside[0], cx);
        let presses = [
            ("shift-tab", 2),
            ("shift-tab", 1),
            ("shift-tab", 0),
            ("shift-tab", 2),
            ("tab", 0),
            ("tab", 1),
            ("tab", 2),
            ("tab", 0),
        ];
        for (press, (key, to)) in presses.into_iter().enumerate() {
            cx.simulate_keystrokes(key);
            settle(cx);
            assert!(
                cx.update(|window, cx| trap.contains_focused(window, cx)),
                "press {press}, {key}, keeps focus in the trap"
            );
            assert!(
                focused(&inside[to], cx),
                "press {press}, {key}, reaches stop {to}"
            );
        }
    }

    /// GitTurtle patch: a focus trap with no Tab stop of its own keeps focus
    /// where it was on Tab and Shift-Tab, however many stops lie around it.
    #[gpui::test]
    fn tab_in_a_focus_trap_without_stops_moves_nothing(cx: &mut TestAppContext) {
        let (view, cx) = trap_page(0, cx);
        let trap = view.read_with(cx, |view, _| view.trap.clone());
        focus(&trap, cx);
        for key in ["tab", "shift-tab", "tab"] {
            cx.simulate_keystrokes(key);
            settle(cx);
            assert!(focused(&trap, cx), "{key} leaves focus on the trap");
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
