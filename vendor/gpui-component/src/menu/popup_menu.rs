use crate::ThemeStyled as _;
use crate::actions::{Cancel, Confirm, SelectDown, SelectUp};
use crate::actions::{SelectLeft, SelectRight};
use crate::menu::menu_item::{Entry, MenuItemElement};
use crate::scroll::{Scrollbar, ScrollbarMode};
use crate::{ActiveTheme, ElementExt, Icon, IconName, Sizable as _, h_flex, v_flex};
use crate::{Side, Size, kbd::Kbd};
use gpui::{
    Action, Anchor, AnyElement, App, AppContext, Bounds, Context, DismissEvent, Edges, Entity,
    EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement, KeyBinding,
    ParentElement, Pixels, Render, Role, ScrollHandle, SharedString, StatefulInteractiveElement,
    Styled, WeakEntity, Window, anchored, deferred, div, prelude::FluentBuilder, px, rems,
};
use gpui::{
    ClickEvent, Half, MouseDownEvent, MouseMoveEvent, OwnedMenuItem, Point, ScrollWheelEvent,
    Subscription,
};

use std::rc::Rc;

const CONTEXT: &str = "PopupMenu";

/// GitTurtle patch: the room a menu keeps from its window's edges, the
/// margin gpui-base's `Popup` host snaps a dropdown menu to and the context
/// menu's `snap_to_window_with_margin(px(8.))`.
const WINDOW_INSET: Pixels = px(8.);

/// GitTurtle patch: a row's text, kept on one line. Upstream let a row's
/// text wrap wherever the menu was narrower than it, which an item's fixed
/// height then cut; a row now ends with an ellipsis instead, its whole text
/// staying its accessible name. The text and its row shrink below their
/// content (`min_w_0`), so a menu capped at its window's width
/// ([`PopupMenu::max_width`]) truncates its long rows rather than
/// overflowing. A test build names the text in a debug selector,
/// `menu-row-text: <text>`, since GPUI's test scene does not expose glyphs.
fn menu_row_text(text: SharedString) -> gpui::Div {
    let shown = text.clone();
    div()
        .min_w_0()
        .truncate()
        .debug_selector(move || format!("menu-row-text: {shown}"))
        .child(text)
}

/// GitTurtle patch: how a scrollable menu's rows meet its cap
/// ([`PopupMenu::max_h`]).
///
/// Upstream capped the items column but let its rows shrink, as flex items
/// do, so a capped menu squeezed every row, its gaps with them, until its
/// rows fit or reached their text's height, and cut its last row with no
/// scrollbar. Each row now keeps its height (`flex_shrink_0`), and a menu
/// whose rows run past its cap ends after the last whole row above it, with
/// the padding an uncapped menu keeps below its last row, and scrolls the
/// rest with an always-visible scrollbar in a gutter beside its rows, as a
/// scrolling list cues its overflow.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Fold {
    /// Every row fits under the cap: the menu is as tall as its rows and
    /// draws no scrollbar.
    Fits,
    /// The rows run past the cap: the items column is this tall and scrolls.
    At(Pixels),
}

/// GitTurtle patch: the [`Fold`] of a menu whose rows lie at `rows`, each
/// row's top and bottom below the top of the items column and whether the
/// menu may end after it ([`PopupMenuItem::ends_fold`]), in a column that
/// keeps `padding` below its last row and is no taller than `cap`. The fold
/// lies that padding below the last row that may end it, or at the next
/// row's top where the gap between rows is narrower, so no sliver of that
/// row shows; where no row that may end it fits, the column takes the whole
/// cap.
fn fold_rows(
    rows: impl IntoIterator<Item = (Pixels, Pixels, bool)>,
    padding: Pixels,
    cap: Pixels,
) -> Fold {
    // The last row the menu may end after: its end with the padding, and
    // the top of the row that follows it.
    let mut fold: Option<(Pixels, Option<Pixels>)> = None;
    for (top, bottom, ends_fold) in rows {
        if let Some((_, next @ None)) = &mut fold {
            *next = Some(top);
        }
        if bottom + padding > cap {
            return Fold::At(
                fold.map_or(cap, |(end, next)| next.map_or(end, |next| end.min(next))),
            );
        }
        if ends_fold {
            fold = Some((bottom + padding, None));
        }
    }
    Fold::Fits
}

/// GitTurtle patch: the item the keyboard's cursor was on as its menu was
/// built again ([`PopupMenu::rebuild`]): its name ([`PopupMenuItem::name`]),
/// where it has one, its order among the items of that name, and its place
/// among the menu's entries, which follows the item while a rebuild finds
/// it.
#[derive(Clone, Debug, PartialEq)]
struct KeptCursor {
    name: Option<SharedString>,
    ordinal: usize,
    index: usize,
}

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Confirm { secondary: false }, Some(CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(CONTEXT)),
        KeyBinding::new("up", SelectUp, Some(CONTEXT)),
        KeyBinding::new("down", SelectDown, Some(CONTEXT)),
        KeyBinding::new("left", SelectLeft, Some(CONTEXT)),
        KeyBinding::new("right", SelectRight, Some(CONTEXT)),
    ]);
}

/// An menu item in a popup menu.
pub enum PopupMenuItem {
    /// A menu separator item.
    Separator,
    /// A non-interactive label item.
    Label(SharedString),
    /// A standard menu item.
    Item {
        icon: Option<Icon>,
        label: SharedString,
        disabled: bool,
        checked: bool,
        /// GitTurtle patch: whether the item toggles a state, so it is
        /// announced as a check menu item, ticked or not.
        checkable: bool,
        /// GitTurtle patch: the name assistive technology reads, when it
        /// should say more than the label shows.
        accessibility_label: Option<SharedString>,
        is_link: bool,
        action: Option<Box<dyn Action>>,
        // For link item
        handler: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
    },
    /// A menu item with custom element render.
    ElementItem {
        icon: Option<Icon>,
        disabled: bool,
        checked: bool,
        /// GitTurtle patch: as for [`PopupMenuItem::Item`].
        checkable: bool,
        /// GitTurtle patch: the name assistive technology reads; an element
        /// item has no label of its own.
        accessibility_label: Option<SharedString>,
        /// GitTurtle patch: the name a menu built again finds the item by,
        /// where its accessible name says more than the command it is
        /// ([`PopupMenuItem::named`]).
        name: Option<SharedString>,
        action: Option<Box<dyn Action>>,
        render: Box<dyn Fn(&mut Window, &mut App) -> AnyElement + 'static>,
        handler: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
    },
    /// A submenu item that opens another popup menu.
    ///
    /// NOTE: This is only supported when the parent menu is not `scrollable`.
    Submenu {
        icon: Option<Icon>,
        label: SharedString,
        disabled: bool,
        /// GitTurtle patch: as for [`PopupMenuItem::Item`].
        accessibility_label: Option<SharedString>,
        menu: Entity<PopupMenu>,
    },
}

impl FluentBuilder for PopupMenuItem {}
impl PopupMenuItem {
    /// Create a new menu item with the given label.
    #[inline]
    pub fn new(label: impl Into<SharedString>) -> Self {
        PopupMenuItem::Item {
            icon: None,
            label: label.into(),
            disabled: false,
            checked: false,
            checkable: false,
            accessibility_label: None,
            action: None,
            is_link: false,
            handler: None,
        }
    }

    /// Create a new menu item with custom element render.
    #[inline]
    pub fn element<F, E>(builder: F) -> Self
    where
        F: Fn(&mut Window, &mut App) -> E + 'static,
        E: IntoElement,
    {
        PopupMenuItem::ElementItem {
            icon: None,
            disabled: false,
            checked: false,
            checkable: false,
            accessibility_label: None,
            name: None,
            action: None,
            render: Box::new(move |window, cx| builder(window, cx).into_any_element()),
            handler: None,
        }
    }

    /// Create a new submenu item that opens another popup menu.
    #[inline]
    pub fn submenu(label: impl Into<SharedString>, menu: Entity<PopupMenu>) -> Self {
        PopupMenuItem::Submenu {
            icon: None,
            label: label.into(),
            disabled: false,
            accessibility_label: None,
            menu,
        }
    }

    /// Create a separator menu item.
    #[inline]
    pub fn separator() -> Self {
        PopupMenuItem::Separator
    }

    /// Creates a label menu item.
    #[inline]
    pub fn label(label: impl Into<SharedString>) -> Self {
        PopupMenuItem::Label(label.into())
    }

    /// Set the icon for the menu item.
    ///
    /// Only works for [`PopupMenuItem::Item`], [`PopupMenuItem::ElementItem`] and [`PopupMenuItem::Submenu`].
    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        match &mut self {
            PopupMenuItem::Item { icon: i, .. } => {
                *i = Some(icon.into());
            }
            PopupMenuItem::ElementItem { icon: i, .. } => {
                *i = Some(icon.into());
            }
            PopupMenuItem::Submenu { icon: i, .. } => {
                *i = Some(icon.into());
            }
            _ => {}
        }
        self
    }

    /// Set the action for the menu item.
    ///
    /// Only works for [`PopupMenuItem::Item`] and [`PopupMenuItem::ElementItem`].
    pub fn action(mut self, action: Box<dyn Action>) -> Self {
        match &mut self {
            PopupMenuItem::Item { action: a, .. } => {
                *a = Some(action);
            }
            PopupMenuItem::ElementItem { action: a, .. } => {
                *a = Some(action);
            }
            _ => {}
        }
        self
    }

    /// Set the disabled state for the menu item.
    ///
    /// Only works for [`PopupMenuItem::Item`], [`PopupMenuItem::ElementItem`] and [`PopupMenuItem::Submenu`].
    pub fn disabled(mut self, disabled: bool) -> Self {
        match &mut self {
            PopupMenuItem::Item { disabled: d, .. } => {
                *d = disabled;
            }
            PopupMenuItem::ElementItem { disabled: d, .. } => {
                *d = disabled;
            }
            PopupMenuItem::Submenu { disabled: d, .. } => {
                *d = disabled;
            }
            _ => {}
        }
        self
    }

    /// Set checked state for the menu item.
    ///
    /// NOTE: If `check_side` is [`Side::Left`], the icon will replace with a check icon.
    ///
    /// GitTurtle patch: this also makes the item checkable, so it is announced
    /// as a check menu item whether it is ticked or not.
    pub fn checked(mut self, checked: bool) -> Self {
        match &mut self {
            PopupMenuItem::Item {
                checked: c,
                checkable,
                ..
            }
            | PopupMenuItem::ElementItem {
                checked: c,
                checkable,
                ..
            } => {
                *c = checked;
                *checkable = true;
            }
            _ => {}
        }
        self
    }

    /// GitTurtle patch: set the name assistive technology reads for the item
    /// when it should say more than the label shows, as an icon button's
    /// accessible name does: "More context, 3 lines shown" for an item that
    /// shows "More context (3 lines)". The label stays what is drawn.
    ///
    /// Only works for [`PopupMenuItem::Item`], [`PopupMenuItem::ElementItem`] and [`PopupMenuItem::Submenu`].
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        match &mut self {
            PopupMenuItem::Item {
                accessibility_label,
                ..
            }
            | PopupMenuItem::ElementItem {
                accessibility_label,
                ..
            }
            | PopupMenuItem::Submenu {
                accessibility_label,
                ..
            } => *accessibility_label = Some(label.into()),
            _ => {}
        }
        self
    }

    /// GitTurtle patch: the name a menu built again finds an element item
    /// by ([`PopupMenu::rebuild`]), its command, where its accessible name
    /// says more, as an item that states the condition it waits for does:
    /// found by that name, the refused item is the one the keyboard's
    /// cursor was on, and the cursor rests while it waits
    /// ([`CursorRests`]). Other entries are found by their label.
    pub fn named(mut self, name: impl Into<SharedString>) -> Self {
        if let PopupMenuItem::ElementItem { name: own, .. } = &mut self {
            *own = Some(name.into());
        }
        self
    }

    /// Add a click handler for the menu item.
    ///
    /// Only works for [`PopupMenuItem::Item`] and [`PopupMenuItem::ElementItem`].
    pub fn on_click<F>(mut self, handler: F) -> Self
    where
        F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    {
        match &mut self {
            PopupMenuItem::Item { handler: h, .. } => {
                *h = Some(Rc::new(handler));
            }
            PopupMenuItem::ElementItem { handler: h, .. } => {
                *h = Some(Rc::new(handler));
            }
            _ => {}
        }
        self
    }

    /// Create a link menu item.
    #[inline]
    pub fn link(label: impl Into<SharedString>, href: impl Into<String>) -> Self {
        let href = href.into();
        PopupMenuItem::Item {
            icon: None,
            label: label.into(),
            disabled: false,
            checked: false,
            checkable: false,
            accessibility_label: None,
            action: None,
            is_link: true,
            handler: Some(Rc::new(move |_, _, cx| cx.open_url(&href))),
        }
    }

    #[inline]
    fn is_clickable(&self) -> bool {
        !matches!(self, PopupMenuItem::Separator)
            && matches!(
                self,
                PopupMenuItem::Item {
                    disabled: false,
                    ..
                } | PopupMenuItem::ElementItem {
                    disabled: false,
                    ..
                } | PopupMenuItem::Submenu {
                    disabled: false,
                    ..
                }
            )
    }

    #[inline]
    fn is_separator(&self) -> bool {
        matches!(self, PopupMenuItem::Separator)
    }

    /// GitTurtle patch: whether a scrolling menu may end after this entry
    /// ([`Fold`]): after a row that offers something, not after a separator
    /// or a heading whose rows lie below the fold.
    fn ends_fold(&self) -> bool {
        !matches!(self, PopupMenuItem::Separator | PopupMenuItem::Label(_))
    }

    fn has_left_icon(&self, check_side: Side) -> bool {
        match self {
            PopupMenuItem::Item { icon, checked, .. } => {
                icon.is_some() || (check_side.is_left() && *checked)
            }
            PopupMenuItem::ElementItem { icon, checked, .. } => {
                icon.is_some() || (check_side.is_left() && *checked)
            }
            PopupMenuItem::Submenu { icon, .. } => icon.is_some(),
            _ => false,
        }
    }

    #[inline]
    /// GitTurtle patch: `Some(checked)` for a checkable item, `None` otherwise.
    fn check_state(&self) -> Option<bool> {
        match self {
            PopupMenuItem::Item {
                checked, checkable, ..
            }
            | PopupMenuItem::ElementItem {
                checked, checkable, ..
            } => checkable.then_some(*checked),
            _ => None,
        }
    }

    fn is_checked(&self) -> bool {
        match self {
            PopupMenuItem::Item { checked, .. } => *checked,
            PopupMenuItem::ElementItem { checked, .. } => *checked,
            _ => false,
        }
    }

    fn a11y_label(&self) -> Option<SharedString> {
        match self {
            PopupMenuItem::Item {
                label,
                accessibility_label,
                ..
            }
            | PopupMenuItem::Submenu {
                label,
                accessibility_label,
                ..
            } => Some(accessibility_label.as_ref().unwrap_or(label).clone()),
            PopupMenuItem::ElementItem {
                accessibility_label,
                ..
            } => accessibility_label.clone(),
            PopupMenuItem::Label(label) => Some(label.clone()),
            PopupMenuItem::Separator => None,
        }
    }

    /// GitTurtle patch: the name a menu built again finds this item by, so
    /// the keyboard's cursor stays on it ([`PopupMenu::rebuild`]): an item's
    /// or a submenu's label, or an element item's accessible name, which is
    /// all it has to tell it apart. A label or a separator has none: the
    /// cursor never rests on one.
    fn name(&self) -> Option<&SharedString> {
        match self {
            PopupMenuItem::Item { label, .. } | PopupMenuItem::Submenu { label, .. } => Some(label),
            PopupMenuItem::ElementItem {
                accessibility_label,
                name,
                ..
            } => name.as_ref().or(accessibility_label.as_ref()),
            PopupMenuItem::Label(_) | PopupMenuItem::Separator => None,
        }
    }
}

/// GitTurtle patch: a menu built again left the keyboard's cursor resting
/// because the item it was on is still offered but can no longer act, as
/// while a write refuses it ([`PopupMenu::rebuild`]). Emitted once, as the
/// rest begins, not again for each rebuild while it lasts, and never for a
/// menu whose selection the pointer owns, which keeps no cursor to rest. An
/// application can say why the item waits, as an item that states its
/// condition does in its accessible name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CursorRests {
    /// The item's name, by which the menu finds it ([`PopupMenuItem::named`]).
    pub name: SharedString,
    /// The item's accessible name, which says why it waits where the item
    /// states it.
    pub label: SharedString,
}

pub struct PopupMenu {
    pub(crate) focus_handle: FocusHandle,
    pub(crate) menu_items: Vec<PopupMenuItem>,
    /// The focus handle of Entity to handle actions.
    pub(crate) action_context: Option<FocusHandle>,
    /// The focus to restore on dismiss. Unlike `action_context`, this does not
    /// change where actions are dispatched: they still bubble from the menu's
    /// own focus path (through the trigger element's ancestors).
    pub(crate) previous_focus_handle: Option<FocusHandle>,
    selected_index: Option<usize>,
    /// GitTurtle patch: the keyboard placed the selection (Up, Down, or Right
    /// into a submenu), so the item draws as the keyboard's cursor rather than
    /// the pointer's hover; the pointer entering an item clears it.
    keyboard_selection: bool,
    /// GitTurtle patch: the item the keyboard's cursor was on when a rebuild
    /// found it gone or unable to act, as while a write refuses it, so the
    /// cursor rests on nothing: a later rebuild that offers it able to act
    /// places the cursor there again ([`PopupMenu::rebuild`]), and Up and
    /// Down start from its place. A menu key or the pointer that moves the
    /// selection forgets it. While it is held nothing is selected.
    kept_cursor: Option<KeptCursor>,
    /// GitTurtle patch: the pointer took the selection, or took the
    /// keyboard's cursor off its row, since a menu key last placed it, so a
    /// rebuild selects nothing, whatever its builder selects
    /// ([`PopupMenu::rebuild`]). A menu key or a dismissal clears it.
    pointer_selection: bool,
    /// GitTurtle patch: where the pointer rested when a wheel scrolled the
    /// rows under it, the menu was laid out at another place or size around
    /// it, or the menu was built again under it, until it moves from there.
    /// Meanwhile no row draws the pointer's hover or takes its selection:
    /// the person never moved to the row that arrived under it.
    still_pointer: Option<Point<Pixels>>,
    /// GitTurtle patch: the item GPUI last reported under the pointer, which
    /// takes the selection once a still pointer moves onto it
    /// ([`PopupMenu::pointer_moved`]).
    hovered_index: Option<usize>,
    /// GitTurtle patch: the item a press of the primary button went down on,
    /// by its index; only a release on that item acts
    /// ([`PopupMenu::on_click`]). A rebuild forgets it.
    pressed: Option<usize>,
    /// GitTurtle patch: how many times the menu has been built again
    /// ([`PopupMenu::rebuild`]); a row drawn before the latest rebuild
    /// records no press ([`PopupMenu::pressed_on`]).
    builds: usize,
    /// GitTurtle patch: where the last frame drew each drawn row, by its
    /// item's index, scrolled and clipped as drawn; none after a rebuild
    /// until the next layout ([`PopupMenu::rows_laid_out`]).
    rows: Vec<Bounds<Pixels>>,
    /// GitTurtle patch: the surface's bounds as the last frame laid it out,
    /// or `None` before its first layout ([`PopupMenu::surface_laid_out`]).
    surface: Option<Bounds<Pixels>>,
    /// GitTurtle patch: where the pointer was as the last frame laid the
    /// surface out. A hover report with the pointer still there came from
    /// rows that moved, not from the pointer.
    pointer_at_layout: Option<Point<Pixels>>,
    min_width: Option<Pixels>,
    max_width: Option<Pixels>,
    max_height: Option<Pixels>,
    bounds: Bounds<Pixels>,
    size: Size,
    check_side: Side,

    /// The parent menu of this menu, if this is a submenu
    parent_menu: Option<WeakEntity<Self>>,
    scrollable: bool,
    /// GitTurtle patch: where a scrollable menu's items met its cap when it
    /// was last laid out, or `None` before its first layout.
    fold: Option<Fold>,
    external_link_icon: bool,
    scroll_handle: ScrollHandle,
    // This will update on render
    submenu_anchor: (Anchor, Pixels),

    /// Paint priority for this menu layer. The top-level menu starts at 1 and
    /// each nested submenu increments it, so deeper levels are always drawn on
    /// top of shallower ones. This fixes background content (e.g. the
    /// underlying list) bleeding through multi-level submenus, which happens
    /// when nested `anchored` popovers share the same paint order.
    ///
    /// The top-level menu relies on its container (e.g. `Popover`,
    /// `ContextMenu`) to `deferred`-draw it, and each submenu is deferred once
    /// in `render_item` with `priority + 1`. Keeping a single deferred layer
    /// per level matters because GPUI caps nested deferred depth (see
    /// `prepaint_deferred_draws`).
    priority: usize,

    _subscriptions: Vec<Subscription>,
}

impl PopupMenu {
    pub(crate) fn new(cx: &mut App) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            action_context: None,
            previous_focus_handle: None,
            parent_menu: None,
            menu_items: Vec::new(),
            selected_index: None,
            keyboard_selection: false,
            kept_cursor: None,
            pointer_selection: false,
            still_pointer: None,
            hovered_index: None,
            pressed: None,
            builds: 0,
            rows: Vec::new(),
            surface: None,
            pointer_at_layout: None,
            min_width: None,
            max_width: None,
            max_height: None,
            check_side: Side::Left,
            bounds: Bounds::default(),
            scrollable: false,
            fold: None,
            scroll_handle: ScrollHandle::default(),
            external_link_icon: true,
            size: Size::default(),
            submenu_anchor: (Anchor::TopLeft, Pixels::ZERO),
            priority: gpui_base::POPUP_PRIORITY,
            _subscriptions: vec![],
        }
    }

    pub fn build(
        window: &mut Window,
        cx: &mut App,
        f: impl FnOnce(Self, &mut Window, &mut Context<PopupMenu>) -> Self,
    ) -> Entity<Self> {
        cx.new(|cx| f(Self::new(cx), window, cx))
    }

    /// Set the focus handle of Entity to handle actions.
    ///
    /// When the menu is dismissed or before an action is triggered, the focus will be returned to this handle.
    ///
    /// Then the action will be dispatched to this handle.
    pub fn action_context(mut self, handle: FocusHandle) -> Self {
        self.action_context = Some(handle);
        self
    }

    pub(crate) fn set_action_context(
        &mut self,
        action_context: Option<FocusHandle>,
        cx: &mut Context<Self>,
    ) {
        self.action_context = action_context.clone();

        for item in &self.menu_items {
            if let PopupMenuItem::Submenu { menu, .. } = item {
                menu.update(cx, |menu, cx| {
                    menu.set_action_context(action_context.clone(), cx);
                });
            }
        }
    }

    /// Set the focus to restore when the menu is dismissed, without changing
    /// where actions are dispatched.
    pub(crate) fn set_previous_focus(
        &mut self,
        handle: Option<FocusHandle>,
        cx: &mut Context<Self>,
    ) {
        self.previous_focus_handle = handle.clone();

        for item in &self.menu_items {
            if let PopupMenuItem::Submenu { menu, .. } = item {
                menu.update(cx, |menu, cx| {
                    menu.set_previous_focus(handle.clone(), cx);
                });
            }
        }
    }

    /// Set min width of the popup menu, default is 120px
    pub fn min_w(mut self, width: impl Into<Pixels>) -> Self {
        self.min_width = Some(width.into());
        self
    }

    /// Set max width of the popup menu, default is 500px
    pub fn max_w(mut self, width: impl Into<Pixels>) -> Self {
        self.max_width = Some(width.into());
        self
    }

    /// Set max height of the popup menu, default is half of the window height
    pub fn max_h(mut self, height: impl Into<Pixels>) -> Self {
        self.max_height = Some(height.into());
        self
    }

    /// Set the menu to be scrollable to show vertical scrollbar.
    ///
    /// NOTE: If this is true, the sub-menus will cannot be support.
    pub fn scrollable(mut self, scrollable: bool) -> Self {
        self.scrollable = scrollable;
        self
    }

    /// Set the side to show check icon, default is `Side::Left`.
    pub fn check_side(mut self, side: Side) -> Self {
        self.check_side = side;
        self
    }

    /// Set the menu to show external link icon, default is true.
    pub fn external_link_icon(mut self, visible: bool) -> Self {
        self.external_link_icon = visible;
        self
    }

    /// Add Menu Item
    pub fn menu(self, label: impl Into<SharedString>, action: Box<dyn Action>) -> Self {
        self.menu_with_disabled(label, action, false)
    }

    /// Add Menu Item with enable state
    pub fn menu_with_enable(
        mut self,
        label: impl Into<SharedString>,
        action: Box<dyn Action>,
        enable: bool,
    ) -> Self {
        self.add_menu_item(label, None, action, !enable, None);
        self
    }

    /// Add Menu Item with disabled state
    pub fn menu_with_disabled(
        mut self,
        label: impl Into<SharedString>,
        action: Box<dyn Action>,
        disabled: bool,
    ) -> Self {
        self.add_menu_item(label, None, action, disabled, None);
        self
    }

    /// Add label
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.menu_items.push(PopupMenuItem::label(label.into()));
        self
    }

    /// Add Menu to open link
    pub fn link(self, label: impl Into<SharedString>, href: impl Into<String>) -> Self {
        self.link_with_disabled(label, href, false)
    }

    /// Add Menu to open link with disabled state
    pub fn link_with_disabled(
        mut self,
        label: impl Into<SharedString>,
        href: impl Into<String>,
        disabled: bool,
    ) -> Self {
        let href = href.into();
        self.menu_items
            .push(PopupMenuItem::link(label, href).disabled(disabled));
        self
    }

    /// Add Menu to open link
    pub fn link_with_icon(
        self,
        label: impl Into<SharedString>,
        icon: impl Into<Icon>,
        href: impl Into<String>,
    ) -> Self {
        self.link_with_icon_and_disabled(label, icon, href, false)
    }

    /// Add Menu to open link with icon and disabled state
    fn link_with_icon_and_disabled(
        mut self,
        label: impl Into<SharedString>,
        icon: impl Into<Icon>,
        href: impl Into<String>,
        disabled: bool,
    ) -> Self {
        let href = href.into();
        self.menu_items.push(
            PopupMenuItem::link(label, href)
                .icon(icon)
                .disabled(disabled),
        );
        self
    }

    /// Add Menu Item with Icon.
    pub fn menu_with_icon(
        self,
        label: impl Into<SharedString>,
        icon: impl Into<Icon>,
        action: Box<dyn Action>,
    ) -> Self {
        self.menu_with_icon_and_disabled(label, icon, action, false)
    }

    /// Add Menu Item with Icon and disabled state
    pub fn menu_with_icon_and_disabled(
        mut self,
        label: impl Into<SharedString>,
        icon: impl Into<Icon>,
        action: Box<dyn Action>,
        disabled: bool,
    ) -> Self {
        self.add_menu_item(label, Some(icon.into()), action, disabled, None);
        self
    }

    /// Add Menu Item with check icon
    pub fn menu_with_check(
        self,
        label: impl Into<SharedString>,
        checked: bool,
        action: Box<dyn Action>,
    ) -> Self {
        self.menu_with_check_and_disabled(label, checked, action, false)
    }

    /// Add Menu Item with check icon and disabled state
    pub fn menu_with_check_and_disabled(
        mut self,
        label: impl Into<SharedString>,
        checked: bool,
        action: Box<dyn Action>,
        disabled: bool,
    ) -> Self {
        self.add_menu_item(label, None, action, disabled, Some(checked));
        self
    }

    /// Add Menu Item with custom element render.
    pub fn menu_element<F, E>(self, action: Box<dyn Action>, builder: F) -> Self
    where
        F: Fn(&mut Window, &mut App) -> E + 'static,
        E: IntoElement,
    {
        self.menu_element_with_check(false, action, builder)
    }

    /// Add Menu Item with custom element render with disabled state.
    pub fn menu_element_with_disabled<F, E>(
        self,
        action: Box<dyn Action>,
        disabled: bool,
        builder: F,
    ) -> Self
    where
        F: Fn(&mut Window, &mut App) -> E + 'static,
        E: IntoElement,
    {
        self.menu_element_with_check_and_disabled(false, action, disabled, builder)
    }

    /// Add Menu Item with custom element render with icon.
    pub fn menu_element_with_icon<F, E>(
        self,
        icon: impl Into<Icon>,
        action: Box<dyn Action>,
        builder: F,
    ) -> Self
    where
        F: Fn(&mut Window, &mut App) -> E + 'static,
        E: IntoElement,
    {
        self.menu_element_with_icon_and_disabled(icon, action, false, builder)
    }

    /// Add Menu Item with custom element render with check state
    pub fn menu_element_with_check<F, E>(
        self,
        checked: bool,
        action: Box<dyn Action>,
        builder: F,
    ) -> Self
    where
        F: Fn(&mut Window, &mut App) -> E + 'static,
        E: IntoElement,
    {
        self.menu_element_with_check_and_disabled(checked, action, false, builder)
    }

    /// Add Menu Item with custom element render with icon and disabled state
    fn menu_element_with_icon_and_disabled<F, E>(
        mut self,
        icon: impl Into<Icon>,
        action: Box<dyn Action>,
        disabled: bool,
        builder: F,
    ) -> Self
    where
        F: Fn(&mut Window, &mut App) -> E + 'static,
        E: IntoElement,
    {
        self.menu_items.push(
            PopupMenuItem::element(builder)
                .action(action)
                .icon(icon)
                .disabled(disabled),
        );
        self
    }

    /// Add Menu Item with custom element render with check state and disabled state
    fn menu_element_with_check_and_disabled<F, E>(
        mut self,
        checked: bool,
        action: Box<dyn Action>,
        disabled: bool,
        builder: F,
    ) -> Self
    where
        F: Fn(&mut Window, &mut App) -> E + 'static,
        E: IntoElement,
    {
        self.menu_items.push(
            PopupMenuItem::element(builder)
                .action(action)
                .checked(checked)
                .disabled(disabled),
        );
        self
    }

    /// Add a separator Menu Item
    pub fn separator(mut self) -> Self {
        if self.menu_items.is_empty() {
            return self;
        }

        if let Some(PopupMenuItem::Separator) = self.menu_items.last() {
            return self;
        }

        self.menu_items.push(PopupMenuItem::separator());
        self
    }

    /// Add a Submenu
    pub fn submenu(
        self,
        label: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
        f: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> Self {
        self.submenu_with_icon(None, label, window, cx, f)
    }

    /// Add a Submenu item with icon
    pub fn submenu_with_icon(
        mut self,
        icon: Option<Icon>,
        label: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
        f: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> Self {
        let submenu = PopupMenu::build(window, cx, f);
        let parent_menu = cx.entity().downgrade();
        let parent_priority = self.priority;
        submenu.update(cx, |view, _| {
            view.parent_menu = Some(parent_menu);
            view.priority = parent_priority + 1;
        });

        self.menu_items.push(
            PopupMenuItem::submenu(label, submenu).when_some(icon, |this, icon| this.icon(icon)),
        );
        self
    }

    /// Add menu item.
    pub fn item(mut self, item: impl Into<PopupMenuItem>) -> Self {
        let item: PopupMenuItem = item.into();
        self.menu_items.push(item);
        self
    }

    /// Replace all menu items by re-running a builder on this menu, keeping its
    /// identity (focus, parent menu, layer priority).
    ///
    /// For menus whose content arrives asynchronously after the menu is shown,
    /// e.g. swapping a "loading…" placeholder for the loaded items:
    ///
    /// ```ignore
    /// cx.spawn_in(window, async move |menu, cx| {
    ///     let items = fetch_items().await;
    ///     _ = menu.update_in(cx, |menu, window, cx| {
    ///         menu.rebuild(window, cx, |menu, _, _| {
    ///             items.into_iter().fold(menu, |menu, item| {
    ///                 menu.menu(item.label, Box::new(item.action))
    ///             })
    ///         });
    ///     });
    /// })
    /// .detach();
    /// ```
    ///
    /// GitTurtle patch: the keyboard's cursor stays on the item it was on,
    /// found again by its name ([`PopupMenuItem::name`]) and its order among
    /// the items of that name wherever the builder now places it, as an item
    /// added above it moves it, or, for an item without a name, at its place
    /// where the item there has none either. It never moves to another item: where its item is gone or
    /// can no longer act, as while a write refuses it or once its name
    /// states a condition it waits for, the cursor rests on nothing and the
    /// menu remembers the item (`kept_cursor`), however often it is
    /// built again, until a rebuild offers the item able to act and places
    /// the cursor there again, or a menu key or the pointer moves the
    /// selection. From rest, Up and Down start from the item's place. A
    /// remembered item takes the place of any selection the builder makes
    /// ([`Self::select_first_item`]); with none, the builder's selection
    /// stands, unless the pointer owns the selection. Upstream cleared the
    /// selection, so an open menu built again, as its content arrived or the
    /// state its items read changed, lost its cursor, and a caller that put
    /// one back could only put it on the first item; moved to the nearest
    /// item that could act, the cursor once rested on an action the person
    /// had not chosen, which Enter then ran. A selection the pointer made is
    /// cleared, as upstream: the pointer's own place decides what it lights.
    /// Once the pointer has taken the selection, or taken the keyboard's
    /// cursor off its row, since a menu key last placed it
    /// (`pointer_selection`), the rebuild selects nothing, though its builder
    /// selects an item, as a menu opened from the keyboard is built on its
    /// first item, until a menu key places the cursor again: the builder's
    /// first item once took the cursor of a keyboard-opened menu the pointer
    /// had moved over, and Enter ran that item. A capped menu scrolls to the
    /// cursor only where the rebuild moves it to another row, as its item
    /// moved or it returns from rest, and the builder's selection, which a
    /// kept cursor or the pointer's ownership replaces, scrolls nothing: a
    /// menu the wheel scrolled stays where it was.
    ///
    /// Built again under a pointer resting over the menu as last laid out,
    /// while the keyboard does not own the cursor, the pointer is still from
    /// there, as after a wheel ([`Self::rows_scrolled`]): no row takes the
    /// selection or draws the pointer's hover until the pointer moves, which
    /// selects the row under it ([`Self::pointer_moved`]). The rows can move
    /// under the pointer though the menu keeps its place and size, which the
    /// layout's comparison of the surface misses
    /// ([`Self::surface_laid_out`]): a capped menu scrolled to its end, built
    /// again shorter, keeps its height while GPUI clamps its offset to the
    /// shorter content, moving every row down; and GPUI then reported the row
    /// that arrived hovered, which took the selection, so Enter ran it, as a
    /// branch switcher's row checks its branch out. Nothing tells such a row
    /// from one that stayed, and the rebuild has cleared the pointer's
    /// selection, so a row it lit would be one Enter does not run. A
    /// keyboard's cursor needs none of this: the hover's own rule keeps it
    /// from rows that arrive under a pointer that has not moved, and the
    /// pointer taking it as it leaves the cursor's row must stand.
    pub fn rebuild(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        f: impl FnOnce(Self, &mut Window, &mut Context<Self>) -> Self,
    ) {
        let cursor = self.keyboard_cursor();
        let row = self.selected_index;
        let resting = self.kept_cursor.is_some();
        let pointer = cursor.is_none() && self.pointer_selection;
        let mut menu = std::mem::replace(self, Self::new(cx));
        menu.menu_items.clear();
        menu.selected_index = None;
        menu.kept_cursor = None;
        // A press before the rebuild went down on the item that stood at its
        // index then, which an item of the same label may now replace, as
        // Undo last local commit… acts on another commit once HEAD moves:
        // its release acts on nothing.
        menu.pressed = None;
        menu.builds = menu.builds.wrapping_add(1);
        // The builder's selection, which a kept cursor or the pointer's
        // ownership replaces, scrolls a handle nothing draws.
        let scroll = (cursor.is_some() || pointer).then(|| std::mem::take(&mut menu.scroll_handle));
        *self = f(menu, window, cx);
        if let Some(scroll) = scroll {
            self.scroll_handle = scroll;
        }
        if let Some(cursor) = cursor {
            // A rest that begins here, its item still offered but refused,
            // is reported once; a rest that goes on is not reported again.
            if let Some(waiting) = self.place_kept_cursor(cursor, row).filter(|_| !resting)
                && let Some(item) = self.menu_items.get(waiting)
                && let (Some(name), Some(label)) = (item.name().cloned(), item.a11y_label())
            {
                cx.emit(CursorRests { name, label });
            }
        } else if pointer {
            // The builder's selection cleared the flag; the pointer still
            // owns the selection, for the next rebuild as well.
            self.selected_index = None;
            self.keyboard_selection = false;
            self.pointer_selection = true;
        }
        // Rows built again under a pointer resting over the menu can move
        // under it though the menu keeps its place and size, so no row the
        // pointer has not moved to takes the selection or draws its hover.
        let at = window.mouse_position();
        if !self.keyboard_selection && self.surface.is_some_and(|surface| surface.contains(&at)) {
            self.still_pointer = Some(at);
        }
        // The rows laid out were the old items', and GPUI reports nothing
        // for a row no longer drawn, so the hover of one is forgotten.
        self.rows.clear();
        let drawn = self.drawn_items().count();
        self.hovered_index = self.hovered_index.filter(|ix| *ix < drawn);
        cx.notify();
    }

    /// GitTurtle patch: the item the keyboard's cursor is on, or the item a
    /// rebuild found it could not place it on and remembers, for a rebuild
    /// to keep ([`Self::rebuild`]); `None` where the pointer made the
    /// selection or nothing holds it.
    fn keyboard_cursor(&self) -> Option<KeptCursor> {
        if let Some(kept) = &self.kept_cursor {
            return Some(kept.clone());
        }
        let index = self.selected_index.filter(|_| self.keyboard_selection)?;
        let name = self
            .menu_items
            .get(index)
            .and_then(PopupMenuItem::name)
            .cloned();
        let ordinal = name.as_ref().map_or(0, |name| {
            self.menu_items[..index]
                .iter()
                .filter(|item| item.name() == Some(name))
                .count()
        });
        Some(KeptCursor {
            name,
            ordinal,
            index,
        })
    }

    /// GitTurtle patch: where `cursor`'s item stands among the items, if
    /// anywhere: the item of its name in its order among the items of that
    /// name, so items added above it, one of its name among them, leave it
    /// on its own; or, for an item without a name, the entry at its place
    /// where that has none either. Taken nearest its old place, a repeated
    /// name once moved the cursor to the other item of that name.
    fn find_kept(&self, cursor: &KeptCursor) -> Option<usize> {
        match &cursor.name {
            Some(name) => self
                .menu_items
                .iter()
                .enumerate()
                .filter(|(_, item)| item.name() == Some(name))
                .nth(cursor.ordinal)
                .map(|(ix, _)| ix),
            None => self
                .menu_items
                .get(cursor.index)
                .filter(|item| item.name().is_none())
                .map(|_| cursor.index),
        }
    }

    /// GitTurtle patch: places the keyboard's `cursor`, kept across a
    /// rebuild, on its item where that item can act, scrolling a capped
    /// menu to it only where that is not `row`, the cursor's row before the
    /// rebuild; otherwise nothing is selected and the menu remembers the
    /// item, at the place it now stands where it is still offered, for the
    /// next rebuild and the menu keys. Returns the item's index where the
    /// cursor rests because the item is offered but refused.
    fn place_kept_cursor(&mut self, cursor: KeptCursor, row: Option<usize>) -> Option<usize> {
        let found = self.find_kept(&cursor);
        match found.filter(|ix| self.menu_items[*ix].is_clickable()) {
            Some(ix) => {
                self.selected_index = Some(ix);
                self.keyboard_selection = true;
                if row != Some(ix) {
                    self.scroll_handle.scroll_to_item(ix);
                }
                None
            }
            None => {
                self.selected_index = None;
                self.kept_cursor = Some(KeptCursor {
                    index: found.unwrap_or(cursor.index),
                    ..cursor
                });
                found
            }
        }
    }

    fn add_menu_item(
        &mut self,
        label: impl Into<SharedString>,
        icon: Option<Icon>,
        action: Box<dyn Action>,
        disabled: bool,
        // GitTurtle patch: `Some` for the check builders, which make the item
        // checkable; `None` for a plain item.
        checked: Option<bool>,
    ) -> &mut Self {
        self.menu_items.push(
            PopupMenuItem::new(label)
                .when_some(icon, |item, icon| item.icon(icon))
                .disabled(disabled)
                .when_some(checked, |item, checked| item.checked(checked))
                .action(action),
        );
        self
    }

    pub(super) fn with_menu_items<I>(
        mut self,
        items: impl IntoIterator<Item = I>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self
    where
        I: Into<OwnedMenuItem>,
    {
        for item in items {
            match item.into() {
                OwnedMenuItem::Action {
                    name,
                    action,
                    checked,
                    disabled,
                    ..
                } => {
                    // GitTurtle patch: a platform menu action says only whether
                    // it is ticked, so only a ticked one is announced as checkable.
                    self = if checked {
                        self.menu_with_check_and_disabled(
                            name,
                            checked,
                            action.boxed_clone(),
                            disabled,
                        )
                    } else {
                        self.menu_with_disabled(name, action.boxed_clone(), disabled)
                    }
                }
                OwnedMenuItem::Separator => {
                    self = self.separator();
                }
                OwnedMenuItem::Submenu(submenu) => {
                    self = self.submenu(submenu.name, window, cx, move |menu, window, cx| {
                        menu.with_menu_items(submenu.items.clone(), window, cx)
                    })
                }
                OwnedMenuItem::SystemMenu(_) => {}
            }
        }

        if self.menu_items.len() > 20 {
            self.scrollable = true;
        }

        self
    }

    pub(crate) fn active_submenu(&self) -> Option<Entity<PopupMenu>> {
        if let Some(ix) = self.selected_index {
            if let Some(item) = self.menu_items.get(ix) {
                return match item {
                    PopupMenuItem::Submenu { menu, .. } => Some(menu.clone()),
                    _ => None,
                };
            }
        }

        None
    }

    pub fn is_empty(&self) -> bool {
        self.menu_items.is_empty()
    }

    fn clickable_menu_items(&self) -> impl Iterator<Item = (usize, &PopupMenuItem)> {
        self.menu_items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.is_clickable())
    }

    /// GitTurtle patch: a press went down on the row drawn for the item at
    /// `ix` as the menu stood after `builds` rebuilds. GPUI delivers a press
    /// to the rows of the last frame drawn, which can come before the menu
    /// is drawn again after a rebuild: such a press records nothing, so its
    /// release runs nothing.
    fn pressed_on(&mut self, ix: usize, builds: usize) {
        self.pressed = (self.builds == builds).then_some(ix);
    }

    /// GitTurtle patch: a click acts only where its press went down on the
    /// item at `ix` as the menu was last built ([`Self::rebuild`],
    /// [`Self::pressed_on`]). Rows are wired, and GPUI keeps a pending press,
    /// by the item's index, so a press and release that straddled a rebuild
    /// ran whatever item the rebuild put at the pressed index, which the
    /// person never pressed. A label is not an item's identity: kept for an
    /// item of the same label, a press on Undo last local commit… ran the
    /// Undo of the commit HEAD had moved to, and one on a project group's
    /// "Archive" the other group of that name, so a press across a rebuild
    /// runs nothing and the person clicks again.
    fn on_click(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        window.prevent_default();
        if self.pressed.take() != Some(ix) {
            return;
        }
        self.selected_index = Some(ix);
        self.kept_cursor = None;
        self.confirm(&Confirm { secondary: false }, window, cx);
    }

    fn confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        match self.selected_index {
            Some(index) => {
                // GitTurtle patch: only an entry that can act acts. The
                // pointer's hover selects any row, a disabled item or a
                // label among them, and Enter then ran a disabled item's
                // handler.
                let item = self
                    .menu_items
                    .get(index)
                    .filter(|item| item.is_clickable());
                match item {
                    Some(PopupMenuItem::Item {
                        handler, action, ..
                    }) => {
                        if let Some(handler) = handler {
                            handler(&ClickEvent::default(), window, cx);
                        } else if let Some(action) = action.as_ref() {
                            self.dispatch_confirm_action(action, window, cx);
                        }

                        self.dismiss(&Cancel, window, cx)
                    }
                    Some(PopupMenuItem::ElementItem {
                        handler, action, ..
                    }) => {
                        if let Some(handler) = handler {
                            handler(&ClickEvent::default(), window, cx);
                        } else if let Some(action) = action.as_ref() {
                            self.dispatch_confirm_action(action, window, cx);
                        }
                        self.dismiss(&Cancel, window, cx)
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn dispatch_confirm_action(
        &self,
        action: &Box<dyn Action>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(context) = self.action_context.as_ref() {
            context.focus(window, cx);
        }

        window.dispatch_action(action.boxed_clone(), cx);
    }

    /// Select `ix` for the keyboard, this method's only caller (Up, Down, a
    /// submenu's first item and a keyboard-opened menu's).
    fn set_selected_index(&mut self, ix: usize, cx: &mut Context<Self>) {
        // GitTurtle patch: a key that moves the cursor ends a rebuild's
        // remembering of the item it was on, and the pointer's ownership of
        // the selection ([`Self::rebuild`]).
        self.kept_cursor = None;
        self.pointer_selection = false;
        if self.selected_index != Some(ix) || !self.keyboard_selection {
            self.selected_index = Some(ix);
            self.keyboard_selection = true;
            self.scroll_handle.scroll_to_item(ix);
            cx.notify();
        }
    }

    // GitTurtle patch: Up and Down move only between items that can act.
    // Upstream's first Down, and Down past the last item, selected the first
    // entry whatever it was, so a menu that opens with a label (the project
    // pane's row name, the profile menu's heading) highlighted nothing that
    // could act, and Enter did nothing. A label, separator or disabled item is
    // passed over, and a menu with none that can act selects nothing.

    /// The first item that can act, if any.
    fn first_clickable_index(&self) -> Option<usize> {
        self.clickable_menu_items().next().map(|(ix, _)| ix)
    }

    /// GitTurtle patch: places the keyboard's cursor on the first item that
    /// can act, as a menu the keyboard opened shows it, so Enter acts on
    /// that item and Down moves on from it; a menu with none selects
    /// nothing. Upstream highlighted nothing until the first Down. The
    /// caller decides when a menu was opened from the keyboard.
    pub fn select_first_item(&mut self, cx: &mut Context<Self>) {
        if let Some(ix) = self.first_clickable_index() {
            self.set_selected_index(ix, cx);
        }
    }

    /// GitTurtle patch: where Up and Down start from: the cursor's item, and
    /// `true`, or, while a rebuild rests the cursor, the place of the item it
    /// remembers ([`Self::rebuild`]), and `false`: Up moves to the last item
    /// that can act before that place, and Down to the first at or after it,
    /// since the remembered item, refused there or gone, cannot take it.
    fn cursor_place(&self) -> Option<(usize, bool)> {
        match (self.selected_index, &self.kept_cursor) {
            (Some(ix), _) => Some((ix, true)),
            (None, Some(kept)) => Some((kept.index, false)),
            (None, None) => None,
        }
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let previous = self
            .cursor_place()
            .and_then(|(ix, _)| {
                self.clickable_menu_items()
                    .filter(|(i, _)| *i < ix)
                    .last()
                    .map(|(i, _)| i)
            })
            .or_else(|| self.clickable_menu_items().last().map(|(ix, _)| ix));
        if let Some(ix) = previous {
            self.set_selected_index(ix, cx);
        }
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let next = self
            .cursor_place()
            .and_then(|(ix, on_it)| {
                self.clickable_menu_items()
                    .find(|(i, _)| *i > ix || (!on_it && *i == ix))
                    .map(|(i, _)| i)
            })
            .or_else(|| self.first_clickable_index());
        if let Some(ix) = next {
            self.set_selected_index(ix, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        let handled = if matches!(self.submenu_anchor.0, Anchor::TopLeft | Anchor::BottomLeft) {
            self._unselect_submenu(window, cx)
        } else {
            self._select_submenu(window, cx)
        };

        if self.parent_side(cx).is_left() {
            self._focus_parent_menu(window, cx);
        }

        if handled {
            return;
        }

        // For parent AppMenuBar to handle.
        if self.parent_menu.is_none() {
            cx.propagate();
        }
    }

    fn select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        let handled = if matches!(self.submenu_anchor.0, Anchor::TopLeft | Anchor::BottomLeft) {
            self._select_submenu(window, cx)
        } else {
            self._unselect_submenu(window, cx)
        };

        if self.parent_side(cx).is_right() {
            self._focus_parent_menu(window, cx);
        }

        if handled {
            return;
        }

        // For parent AppMenuBar to handle.
        if self.parent_menu.is_none() {
            cx.propagate();
        }
    }

    fn _select_submenu(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Some(active_submenu) = self.active_submenu() {
            // Focus the submenu, so that can be handle the action.
            active_submenu.update(cx, |view, cx| {
                // GitTurtle patch: the submenu's first item that can act.
                if let Some(ix) = view.first_clickable_index() {
                    view.set_selected_index(ix, cx);
                }
                view.focus_handle.focus(window, cx);
            });
            cx.notify();
            return true;
        }

        return false;
    }

    fn _unselect_submenu(&mut self, _: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Some(active_submenu) = self.active_submenu() {
            active_submenu.update(cx, |view, cx| {
                view.selected_index = None;
                cx.notify();
            });
            return true;
        }

        return false;
    }

    fn _focus_parent_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(parent) = self.parent_menu.as_ref() else {
            return;
        };
        let Some(parent) = parent.upgrade() else {
            return;
        };

        self.selected_index = None;
        parent.update(cx, |view, cx| {
            view.focus_handle.focus(window, cx);
            cx.notify();
        });
    }

    fn parent_side(&self, cx: &App) -> Side {
        let Some(parent) = self.parent_menu.as_ref() else {
            return Side::Left;
        };

        let Some(parent) = parent.upgrade() else {
            return Side::Left;
        };

        match parent.read(cx).submenu_anchor.0 {
            Anchor::TopLeft | Anchor::BottomLeft => Side::Left,
            Anchor::TopRight | Anchor::BottomRight => Side::Right,
            // Center anchors are not used for submenu positioning, but we must cover them.
            _ => Side::Left,
        }
    }

    /// Dismiss the menu and the entire parent chain.
    ///
    /// The submenu is closed together with its parent, same as macOS menus.
    fn dismiss(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_index = None;
        self.kept_cursor = None;
        self.pointer_selection = false;
        cx.emit(DismissEvent);

        // Focus back to the previous focused handle, unless the item's click
        // handler has already moved focus elsewhere (e.g. opened a dialog and
        // focused its input) -- stealing focus back would break that.
        let focus_moved_away =
            window.focused(cx).is_some() && !self.focus_handle.contains_focused(window, cx);
        if !focus_moved_away {
            if let Some(handle) = self
                .previous_focus_handle
                .as_ref()
                .or(self.action_context.as_ref())
            {
                window.focus(handle, cx);
            }
        }

        let Some(parent_menu) = self.parent_menu.clone() else {
            return;
        };

        // Dismiss parent menu, when this menu is dismissed
        _ = parent_menu.update(cx, |view, cx| {
            view.dismiss(&Cancel, window, cx);
        });
    }

    fn handle_dismiss(
        &mut self,
        position: &Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Do not dismiss, if click inside the parent menu
        if let Some(parent) = self.parent_menu.as_ref() {
            if let Some(parent) = parent.upgrade() {
                if parent.read(cx).bounds.contains(position) {
                    return;
                }
            }
        }

        // Do not dismiss, if there have an active submenu, the click may be
        // inside the submenu, let the submenu to handle it.
        //
        // Otherwise the submenu will be dismissed before its item's `on_click`.
        if self.active_submenu().is_some() {
            return;
        }

        self.dismiss(&Cancel, window, cx);
    }

    fn on_mouse_down_out(
        &mut self,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_dismiss(&e.position, window, cx);
    }

    fn render_key_binding(
        &self,
        action: Option<Box<dyn Action>>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Kbd> {
        let action = action?;

        match self
            .action_context
            .as_ref()
            .or(self.previous_focus_handle.as_ref())
            .and_then(|handle| Kbd::binding_for_action_in(action.as_ref(), handle, window))
        {
            Some(kbd) => Some(kbd),
            // Fallback to App level key binding
            None => Kbd::binding_for_action(action.as_ref(), None, window),
        }
        .map(|this| {
            this.p_0()
                .flex_nowrap()
                .border_0()
                .bg(gpui::transparent_white())
        })
    }

    fn render_icon(
        has_icon: bool,
        checked: bool,
        icon: Option<Icon>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        if !has_icon {
            return None;
        }

        let icon = if let Some(icon) = icon {
            icon.clone()
        } else if checked {
            Icon::new(IconName::Check)
        } else {
            Icon::empty()
        };

        Some(icon.xsmall())
    }

    /// GitTurtle patch: the widest the menu draws, its own maximum (500 px by
    /// default) but never wider than its window less [`WINDOW_INSET`] on
    /// either side. Upstream capped a menu only at its maximum, so in a
    /// window narrower than that a menu with a long row, such as a long Git
    /// identity, ran off the window's edge; it now keeps inside the window
    /// and its rows end with an ellipsis ([`menu_row_text`]).
    fn max_width(&self, window: &Window) -> Pixels {
        let room = window.viewport_size().width - WINDOW_INSET * 2.;
        self.max_width.unwrap_or(px(500.)).min(room)
    }

    /// The height of an item's row at the menu's size.
    fn item_height(&self) -> Pixels {
        match self.size {
            Size::Small => px(20.),
            _ => px(26.),
        }
    }

    /// The entries the items column draws: every entry but a closing
    /// separator.
    fn drawn_items(&self) -> impl Iterator<Item = &PopupMenuItem> {
        let count = self.menu_items.len();
        self.menu_items
            .iter()
            .enumerate()
            .filter(move |(ix, item)| !(*ix + 1 == count && item.is_separator()))
            .map(|(_, item)| item)
    }

    /// GitTurtle patch: the [`Fold`] under `cap` before the menu has been
    /// laid out, from the heights its entries are drawn at: an item's fixed
    /// row, a separator's rule and margins, and a label or element counted
    /// as an item's row. The first layout corrects it where a row differs
    /// ([`PopupMenu::measured_fold`]), so the first frame of a capped menu
    /// usually draws as every later one does.
    fn estimated_fold(&self, cap: Pixels, window: &Window) -> Fold {
        // The items column's `p_1` and `gap_y_0p5`, and the `my_0p5` around
        // a separator's 2 px rule, each snapped to device pixels as GPUI's
        // layout snaps them.
        let rem = window.rem_size();
        let (padding, gap) = (
            window.pixel_snap(rem * 0.25),
            window.pixel_snap(rem * 0.125),
        );
        let margin = gap;
        let item = self.item_height();
        let mut next = padding;
        let rows = self.drawn_items().map(|entry| {
            let (margin, height) = if entry.is_separator() {
                (margin, px(2.))
            } else {
                (Pixels::ZERO, item)
            };
            let top = next + margin;
            next = top + height + margin + gap;
            (top, top + height, entry.ends_fold())
        });
        fold_rows(rows, padding, cap)
    }

    /// GitTurtle patch: the [`Fold`] under `cap` as this frame laid the
    /// items column out, read from its scroll handle, which records each
    /// row's bounds before scrolling them; its `rows` first children are the
    /// drawn entries.
    fn measured_fold(&self, rows: usize, cap: Pixels, window: &Window) -> Fold {
        let top = self.scroll_handle.bounds().top();
        let padding = window.pixel_snap(window.rem_size() * 0.25);
        let rows = self
            .drawn_items()
            .take(rows)
            .enumerate()
            .filter_map(|(ix, entry)| {
                let row = self.scroll_handle.bounds_for_item(ix)?;
                Some((row.top() - top, row.bottom() - top, entry.ends_fold()))
            });
        fold_rows(rows, padding, cap)
    }

    /// GitTurtle patch: a wheel over the items column moved its rows under a
    /// pointer that stays where it was. The column's own scroll listener has
    /// already added the notch to its offset, which its next layout clamps;
    /// where that moves the rows, the row the pointer had selected gives the
    /// selection up and none lights until the pointer moves. A keyboard's
    /// selection stays where it is. Upstream lit two rows at once: the row
    /// that arrived took the selection, and the row that left kept its own
    /// hover, which GPUI refreshes only on a pointer move.
    fn rows_scrolled(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let max = self.scroll_handle.max_offset().y;
        let now = self.scroll_handle.offset().y;
        let before = now - event.delta.pixel_delta(window.line_height()).y;
        let clamp = |y: Pixels| y.clamp(-max, Pixels::ZERO);
        if clamp(now) == clamp(before) {
            return;
        }
        self.still_pointer = Some(event.position);
        if !self.keyboard_selection {
            self.selected_index = None;
        }
        cx.notify();
    }

    /// GitTurtle patch: the pointer moved over the menu. Once it leaves the
    /// place a wheel left it, the row under it takes the selection, as one
    /// the pointer enters does; a frame's synthetic move at the same place
    /// does not count. The row GPUI last reported under the pointer takes it
    /// only where the last frame drew that row under the pointer
    /// (`rows`): GPUI reports nothing for a row a rebuild stopped drawing,
    /// so its index once outlived it, and the pointer's first move onto no
    /// row, as onto the menu's padding, gave the selection to whichever item
    /// a later rebuild put at that index, far from the pointer, a keyboard's
    /// cursor among it, and Enter ran it.
    ///
    /// A move that finds no reported row drawn under it, as one in the frame
    /// after a rebuild, before the layout has recorded the new rows, or one
    /// that lands between a recorded row and the hit area GPUI snapped for
    /// it, leaves the pointer still at its new place, so the next move
    /// decides: cleared there, the row under the pointer stayed unlit until
    /// the pointer crossed into another row, since GPUI reports no hover for
    /// a row the pointer stays on and every later move found the pointer no
    /// longer still.
    fn pointer_moved(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.still_pointer.is_none_or(|at| at == event.position) {
            return;
        }
        let under = |ix: &usize| {
            self.rows
                .get(*ix)
                .is_some_and(|row| row.contains(&event.position))
        };
        match self.hovered_index.filter(under) {
            Some(ix) => {
                self.still_pointer = None;
                self.selected_index = Some(ix);
                self.keyboard_selection = false;
                self.kept_cursor = None;
                self.pointer_selection = true;
            }
            None => self.still_pointer = Some(event.position),
        }
        cx.notify();
    }

    /// GitTurtle patch: the surface was laid out at `bounds`. Where the last
    /// frame laid it out at another place or size, with the pointer over it
    /// then or now, its rows moved under a pointer that stayed where it was,
    /// as a row's menu follows its row when rows arrive above it, a menu
    /// opened above its button changes height, or a menu built again in
    /// place grows wider or taller, as History's commit menu gains its Amend
    /// and Undo items when its commit becomes HEAD. Once the frame is drawn
    /// GPUI reports the row that arrived under the pointer hovered, and
    /// upstream gave it the selection, so Enter ran an item the person never
    /// pointed at. As after a wheel ([`PopupMenu::rows_scrolled`]), no row
    /// takes the selection until the pointer moves, and a keyboard's stays
    /// where it is. A pointer's selection stays while its row stays under the
    /// pointer, as after a move sideways or by less than a row, where GPUI
    /// reports no change of hover: the person put it there and it is still
    /// there. Its row leaving the pointer gives it up as that report runs
    /// (the hover listener), and one on a row the pointer had already left,
    /// as a submenu's row keeps it, is given up here. The first layout moves
    /// nothing. Compared by its origin alone, a menu that grew in place let
    /// the row that arrived take the selection, a keyboard's cursor among
    /// it.
    ///
    /// A notify in prepaint only marks the view: the frame being drawn has
    /// rendered it already, and the draw clears the mark without asking for
    /// another, so a row cleared here, or lit only by a hover a still pointer
    /// no longer draws, stayed drawn lit while Enter did nothing. The next
    /// frame is asked for from outside the draw.
    fn surface_laid_out(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let pointer = window.mouse_position();
        self.pointer_at_layout = Some(pointer);
        let Some(was) = self.surface.replace(bounds) else {
            return;
        };
        if was == bounds || !(was.contains(&pointer) || bounds.contains(&pointer)) {
            return;
        }
        self.still_pointer = Some(pointer);
        if !self.keyboard_selection && self.selected_index != self.hovered_index {
            self.selected_index = None;
        }
        cx.defer_in(window, |_, _, cx| cx.notify());
    }

    /// GitTurtle patch: the items column laid its rows out, `laid_out` being
    /// their bounds as GPUI hands them to a column that does not scroll. A
    /// scrollable column's scroll handle records them instead, before its
    /// offset, which the column's layout has clamped by now, moves them; the
    /// rows are recorded as drawn, moved by that offset and clipped to the
    /// column ([`PopupMenu::pointer_moved`]).
    fn rows_laid_out(&mut self, laid_out: Vec<Bounds<Pixels>>) {
        self.rows = if self.scrollable {
            let (offset, column) = (self.scroll_handle.offset(), self.scroll_handle.bounds());
            (0..self.scroll_handle.children_count())
                .filter_map(|ix| self.scroll_handle.bounds_for_item(ix))
                .map(|row| Bounds::new(row.origin + offset, row.size).intersect(&column))
                .collect()
        } else {
            laid_out
        };
    }

    /// Calculate the anchor corner and left offset for child submenu
    fn update_submenu_menu_anchor(&mut self, window: &Window) {
        let bounds = self.bounds;
        let max_width = self.max_width(window);
        let (anchor, left) = if max_width + bounds.origin.x > window.bounds().size.width {
            (Anchor::TopRight, -px(16.))
        } else {
            (Anchor::TopLeft, bounds.size.width - px(8.))
        };

        let is_bottom_pos = bounds.origin.y + bounds.size.height > window.bounds().size.height;
        self.submenu_anchor = if is_bottom_pos {
            (anchor.other_side_along(gpui::Axis::Vertical), left)
        } else {
            (anchor, left)
        };
    }

    fn render_item(
        &self,
        ix: usize,
        item: &PopupMenuItem,
        options: RenderOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> MenuItemElement {
        let has_left_icon = options.has_left_icon;
        let is_left_check = options.check_side.is_left() && item.is_checked();
        let right_check_icon = if options.check_side.is_right() && item.is_checked() {
            Some(Icon::new(IconName::Check).xsmall())
        } else {
            None
        };

        let selected = self.selected_index == Some(ix);
        const EDGE_PADDING: Pixels = px(4.);
        const INNER_PADDING: Pixels = px(8.);

        let is_submenu = matches!(item, PopupMenuItem::Submenu { .. });
        let group_name = format!("{}:item-{}", cx.entity().entity_id(), ix);

        let item_height = self.item_height();
        let radius = match self.size {
            Size::Small => options.radius.half(),
            _ => options.radius,
        };

        let this = MenuItemElement::new(ix, &group_name)
            .relative()
            // GitTurtle patch: a row keeps its height in a menu capped
            // shorter than its rows, which scrolls them instead ([`Fold`]).
            .flex_shrink_0()
            .text_sm()
            .py_0()
            .px(INNER_PADDING)
            .rounded(radius)
            .items_center()
            .selected(selected)
            .keyboard(self.keyboard_selection)
            .checked(item.check_state())
            .still(self.still_pointer.is_some())
            .on_hover(cx.listener(move |this, hovered, window, cx| {
                // GitTurtle patch: a row a wheel or the menu's layout moved
                // under a still pointer, or out from under it, keeps the
                // selection where it was until the pointer moves
                // ([`PopupMenu::pointer_moved`]). Where rows moved under a
                // pointer that has not moved since the menu was laid out
                // while the keyboard owns the cursor, however they moved,
                // the pointer is still from there: the hover never moves the
                // keyboard's cursor, so the row that arrived only records
                // its hover.
                let pointer = window.mouse_position();
                if this.keyboard_selection
                    && this.still_pointer.is_none()
                    && this.pointer_at_layout == Some(pointer)
                {
                    this.still_pointer = Some(pointer);
                }
                let still = this.still_pointer.is_some();
                if *hovered {
                    this.hovered_index = Some(ix);
                    if !still {
                        this.selected_index = Some(ix);
                        this.keyboard_selection = false;
                        // The pointer's selection ends a rebuild's
                        // remembering, and is its own until a menu key
                        // moves it ([`PopupMenu::rebuild`]).
                        this.kept_cursor = None;
                        this.pointer_selection = true;
                    }
                } else {
                    if this.hovered_index == Some(ix) {
                        this.hovered_index = None;
                    }
                    if !still && !is_submenu && this.selected_index == Some(ix) {
                        // TODO: Better handle the submenu unselection when hover out
                        this.selected_index = None;
                        // The pointer took the selection, a keyboard's
                        // cursor among it, off this row.
                        this.pointer_selection = true;
                    } else if still && !this.keyboard_selection && this.selected_index == Some(ix) {
                        // The row the pointer selected moved out from under
                        // the still pointer, which selects nothing until it
                        // moves ([`PopupMenu::surface_laid_out`]).
                        this.selected_index = None;
                    }
                }

                cx.notify();
            }))
            .when_some(item.a11y_label(), |this, label| this.aria_label(label));

        match item {
            PopupMenuItem::Separator => this
                .entry(Entry::Separator)
                .h_auto()
                .p_0()
                .my_0p5()
                .mx_neg_1()
                .border_b(px(2.))
                .border_color(crate::menu_separator(cx)),
            PopupMenuItem::Label(label) => {
                // GitTurtle patch: a label is a heading in secondary text, not
                // an unavailable item, and it differs from an item by weight as
                // well as colour, at its own size. Its row sets the size and
                // weight and its text the colour, and a test build names each,
                // because GPUI's test scene does not expose glyphs. Its text
                // keeps one line, as an item's does ([`menu_row_text`]).
                let color = crate::menu_label_color(cx);
                let size = crate::menu_label_size(window, cx);
                let weight = crate::MENU_LABEL_WEIGHT;
                let text = label.clone();
                let style_text = label.clone();
                this.entry(Entry::Label).cursor_default().child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .cursor_default()
                        .items_center()
                        .gap_x_1()
                        .text_size(size)
                        .font_weight(weight)
                        .debug_selector(move || {
                            format!(
                                "menu-label-style: {style_text} · {:.2} px · weight {}",
                                f32::from(size),
                                weight.0
                            )
                        })
                        .children(Self::render_icon(has_left_icon, false, None, window, cx))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(color)
                                .debug_selector(move || {
                                    format!("menu-label: {text} · {:?}", gpui::Rgba::from(color))
                                })
                                .child(label.clone()),
                        ),
                )
            }
            PopupMenuItem::ElementItem {
                render,
                icon,
                disabled,
                ..
            } => this
                .when(!disabled, |this| {
                    let builds = self.builds;
                    this.on_press(cx.listener(move |this, _, _, _| this.pressed_on(ix, builds)))
                        .on_click(
                            cx.listener(move |this, _, window, cx| this.on_click(ix, window, cx)),
                        )
                })
                .disabled(*disabled)
                .child(
                    h_flex()
                        .flex_1()
                        .min_h(item_height)
                        .items_center()
                        .gap_x_1()
                        .children(Self::render_icon(
                            has_left_icon,
                            is_left_check,
                            icon.clone(),
                            window,
                            cx,
                        ))
                        .child((render)(window, cx))
                        .children(right_check_icon.map(|icon| icon.ml_3())),
                ),
            PopupMenuItem::Item {
                icon,
                label,
                action,
                disabled,
                is_link,
                ..
            } => {
                let show_link_icon = *is_link && self.external_link_icon;
                let action = action.as_ref().map(|action| action.boxed_clone());
                // A disabled item dims its shortcut with its label.
                let key = self.render_key_binding(action, window, cx).map(|key| {
                    key.when(*disabled, |key| {
                        key.text_color(cx.theme().disabled_foreground())
                    })
                });

                this.when(!disabled, |this| {
                    let builds = self.builds;
                    this.on_press(cx.listener(move |this, _, _, _| this.pressed_on(ix, builds)))
                        .on_click(
                            cx.listener(move |this, _, window, cx| this.on_click(ix, window, cx)),
                        )
                })
                .disabled(*disabled)
                .h(item_height)
                .gap_x_1()
                .children(Self::render_icon(
                    has_left_icon,
                    is_left_check,
                    icon.clone(),
                    window,
                    cx,
                ))
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_3()
                        .items_center()
                        .justify_between()
                        .when(!show_link_icon, |this| {
                            this.child(menu_row_text(label.clone()))
                        })
                        .children(right_check_icon)
                        .when(show_link_icon, |this| {
                            this.child(
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .justify_between()
                                    .gap_1p5()
                                    .child(menu_row_text(label.clone()))
                                    .child(
                                        Icon::new(IconName::ExternalLink)
                                            .xsmall()
                                            .text_color(cx.theme().muted_foreground),
                                    ),
                            )
                        })
                        .children(key),
                )
            }
            PopupMenuItem::Submenu {
                icon,
                label,
                menu,
                disabled,
                ..
            } => this
                .selected(selected)
                .disabled(*disabled)
                .items_start()
                .child(
                    h_flex()
                        .min_h(item_height)
                        .size_full()
                        .min_w_0()
                        .items_center()
                        .gap_x_1()
                        .children(Self::render_icon(
                            has_left_icon,
                            false,
                            icon.clone(),
                            window,
                            cx,
                        ))
                        .child(
                            h_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_2()
                                .items_center()
                                .justify_between()
                                .child(menu_row_text(label.clone()))
                                .child(
                                    Icon::new(IconName::ChevronRight)
                                        .xsmall()
                                        .text_color(cx.theme().muted_foreground),
                                ),
                        ),
                )
                .when(selected, |this| {
                    this.child({
                        let (anchor, left) = self.submenu_anchor;
                        let is_bottom_pos =
                            matches!(anchor, Anchor::BottomLeft | Anchor::BottomRight);
                        deferred(
                            anchored()
                                .anchor(anchor)
                                .child(
                                    div()
                                        .id("submenu")
                                        .occlude()
                                        .when(is_bottom_pos, |this| this.bottom_0())
                                        .when(!is_bottom_pos, |this| this.top_neg_1())
                                        .left(left)
                                        .child(menu.clone()),
                                )
                                .snap_to_window_with_margin(Edges::all(EDGE_PADDING)),
                        )
                        .with_priority(self.priority + 1)
                    })
                }),
        }
    }
}

impl FluentBuilder for PopupMenu {}
impl EventEmitter<DismissEvent> for PopupMenu {}
impl EventEmitter<CursorRests> for PopupMenu {}
impl Focusable for PopupMenu {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[derive(Clone, Copy)]
struct RenderOptions {
    has_left_icon: bool,
    check_side: Side,
    radius: Pixels,
}

impl Render for PopupMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.update_submenu_menu_anchor(window);

        // Submenus attached via the public `item()` + `PopupMenuItem::submenu()`
        // path (from contexts that only have the menu value, e.g. a table
        // delegate's `context_menu`) have no parent wired at construction time.
        // Wire them here so the dismiss chain, click-outside checks and keyboard
        // navigation treat them the same as `submenu()`-built children.
        let parent = cx.entity().downgrade();
        let parent_priority = self.priority;
        for item in &self.menu_items {
            if let PopupMenuItem::Submenu { menu, .. } = item {
                if menu.read(cx).parent_menu.is_none() {
                    menu.update(cx, |menu, _| {
                        menu.parent_menu = Some(parent.clone());
                        menu.priority = parent_priority + 1;
                    });
                }
            }
        }

        let view = cx.entity().clone();
        let rows_view = view.clone();
        let items_count = self.menu_items.len();

        let max_height = self.max_height.unwrap_or_else(|| {
            let window_half_height = window.window_bounds().get_bounds().size.height * 0.5;
            window_half_height.min(px(450.))
        });
        // GitTurtle patch: where a scrollable menu's rows meet its cap, as
        // its last layout measured it, or estimated before the first.
        let fold = self.scrollable.then(|| {
            self.fold
                .unwrap_or_else(|| self.estimated_fold(max_height, window))
        });
        let column = match fold {
            Some(Fold::At(height)) => Some(height),
            _ => None,
        };
        let rows = self.drawn_items().count();

        let has_left_icon = self
            .menu_items
            .iter()
            .any(|item| item.has_left_icon(self.check_side));

        let max_width = self.max_width(window);
        let options = RenderOptions {
            has_left_icon,
            check_side: self.check_side,
            radius: cx.theme().radius.min(px(8.)),
        };

        v_flex()
            .id("popup-menu")
            .role(Role::Menu)
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::dismiss))
            .on_mouse_down_out(cx.listener(Self::on_mouse_down_out))
            .on_mouse_move(cx.listener(Self::pointer_moved))
            .popover_style(cx)
            .text_color(cx.theme().popover_foreground)
            .relative()
            .occlude()
            .child(
                v_flex()
                    // GitTurtle patch: where each row was drawn, for the
                    // pointer's move ([`PopupMenu::rows_laid_out`]).
                    .on_children_prepainted(move |laid_out, _, cx| {
                        rows_view.update(cx, |menu, _| menu.rows_laid_out(laid_out));
                    })
                    .id("items")
                    .p_1()
                    .gap_y_0p5()
                    .min_w(rems(8.))
                    .when_some(self.min_width, |this, min_width| this.min_w(min_width))
                    .max_w(max_width)
                    .when(self.scrollable, |this| {
                        this.max_h(column.unwrap_or(max_height))
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll_handle)
                            .on_scroll_wheel(cx.listener(Self::rows_scrolled))
                    })
                    // GitTurtle patch: while the rows overflow, they keep a
                    // gutter at their right where the scrollbar's track
                    // runs, so its thumb covers no row.
                    .when(column.is_some(), |this| this.pr(Scrollbar::width()))
                    .children(
                        self.menu_items
                            .iter()
                            .enumerate()
                            // Ignore last separator
                            .filter(|(ix, item)| !(*ix + 1 == items_count && item.is_separator()))
                            .map(|(ix, item)| self.render_item(ix, item, options, window, cx)),
                    ),
            )
            // GitTurtle patch: the menu's bounds are taken from the surface,
            // the items column's own bounds before scrolling. Upstream took
            // them from a child of the scrolling column, which moved with
            // its offset and, being as tall as the column, made a menu whose
            // rows fit scroll by the column's padding. Each layout also
            // measures the fold, and a menu whose rows met its cap otherwise
            // than this frame drew asks for the next frame, from outside the
            // draw: a notify here only marks the view, and the draw clears
            // the mark, so the menu kept the fold it was first drawn with
            // until something else drew it. A surface laid out at another
            // place or size than the last frame's moved its rows under the
            // pointer.
            .on_prepaint(move |bounds, window, cx| {
                view.update(cx, |menu, cx| {
                    menu.surface_laid_out(bounds, window, cx);
                    menu.bounds = bounds;
                    if menu.scrollable {
                        let measured = menu.measured_fold(rows, max_height, window);
                        menu.fold = Some(measured);
                        if Some(measured) != fold {
                            cx.defer_in(window, |_, _, cx| cx.notify());
                        }
                    }
                })
            })
            .when(column.is_some(), |this| {
                // TODO: When the menu is limited by `overflow_y_scroll`, the sub-menu will cannot be displayed.
                // GitTurtle patch: the scrollbar of a menu whose rows
                // overflow stays visible, as a scrolling list's does, rather
                // than showing only while it scrolls; a menu whose rows fit
                // draws none.
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .debug_selector(|| "popup-menu-scrollbar".to_string())
                        .child(
                            Scrollbar::vertical(&self.scroll_handle)
                                .id("popup-menu-scrollbar")
                                .mode(ScrollbarMode::Always)
                                .viewport_from_layout(),
                        ),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use gpui::{Modifiers, TestAppContext, VisualTestContext};

    /// A menu under a heading, as a row's menu opens with its name, of
    /// `items`, each a name and whether it can act.
    fn entries(menu: PopupMenu, items: &[(&'static str, bool)]) -> PopupMenu {
        items
            .iter()
            .fold(menu.label("Heading"), |menu, (name, enabled)| {
                menu.item(PopupMenuItem::new(*name).disabled(!enabled))
            })
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
    }

    /// A window whose root view is a focused menu of `items`, with the menu
    /// keys bound.
    fn open<'a>(
        cx: &'a mut TestAppContext,
        items: &'static [(&'static str, bool)],
    ) -> (Entity<PopupMenu>, &'a mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(Theme::default());
            init(cx);
        });
        let (menu, cx) = cx.add_window_view(|window, cx| {
            let menu = entries(PopupMenu::new(cx), items);
            menu.focus_handle.focus(window, cx);
            menu
        });
        draw(cx);
        (menu, cx)
    }

    /// The item the keyboard's cursor is on, by name.
    fn cursor(menu: &Entity<PopupMenu>, cx: &mut VisualTestContext) -> Option<String> {
        menu.read_with(cx, |menu, _| {
            let ix = menu.selected_index.filter(|_| menu.keyboard_selection)?;
            Some(menu.menu_items.get(ix)?.name()?.to_string())
        })
    }

    /// Builds the open menu again with `items`, as a caller does when what
    /// its items read changes.
    fn rebuilt(
        menu: &Entity<PopupMenu>,
        cx: &mut VisualTestContext,
        items: &'static [(&'static str, bool)],
    ) {
        cx.update(|window, cx| {
            menu.update(cx, |menu, cx| {
                menu.rebuild(window, cx, |menu, _, _| entries(menu, items))
            })
        });
        draw(cx);
    }

    const FOUR: &[(&str, bool)] = &[
        ("One", true),
        ("Two", true),
        ("Three", true),
        ("Four", true),
    ];

    /// A menu built again keeps the keyboard's cursor on the item it was on,
    /// found by its name wherever the item now stands, and the menu keys
    /// move on from there; it takes the place of the first item a builder
    /// that opens from the keyboard selects. Upstream's rebuild cleared the
    /// selection, so the cursor was lost, or put back on the first item.
    #[gpui::test]
    fn a_rebuilt_menu_keeps_the_keyboard_cursor_on_its_item(cx: &mut TestAppContext) {
        let (menu, cx) = open(cx, FOUR);
        cx.simulate_keystrokes("down down");
        draw(cx);
        assert_eq!(cursor(&menu, cx).as_deref(), Some("Two"), "Down twice");

        rebuilt(&menu, cx, FOUR);
        assert_eq!(
            cursor(&menu, cx).as_deref(),
            Some("Two"),
            "built again as it was"
        );

        const ZERO_FIRST: &[(&str, bool)] = &[
            ("Zero", true),
            ("One", true),
            ("Two", true),
            ("Three", true),
            ("Four", true),
        ];
        rebuilt(&menu, cx, ZERO_FIRST);
        assert_eq!(
            cursor(&menu, cx).as_deref(),
            Some("Two"),
            "built again with an item above it, by its name rather than its place"
        );
        cx.simulate_keystrokes("down");
        draw(cx);
        assert_eq!(
            cursor(&menu, cx).as_deref(),
            Some("Three"),
            "Down moves on from it"
        );

        cx.update(|window, cx| {
            menu.update(cx, |menu, cx| {
                menu.rebuild(window, cx, |menu, _, cx| {
                    let mut menu = entries(menu, ZERO_FIRST);
                    menu.select_first_item(cx);
                    menu
                })
            })
        });
        draw(cx);
        assert_eq!(
            cursor(&menu, cx).as_deref(),
            Some("Three"),
            "the kept cursor wins over the builder's first item"
        );
    }

    /// Builds the open menu again with `items` as a menu opened from the
    /// keyboard is built, its first item that can act selected
    /// ([`PopupMenu::select_first_item`]).
    fn rebuilt_from_keyboard(
        menu: &Entity<PopupMenu>,
        cx: &mut VisualTestContext,
        items: &'static [(&'static str, bool)],
    ) {
        cx.update(|window, cx| {
            menu.update(cx, |menu, cx| {
                menu.rebuild(window, cx, |menu, _, cx| {
                    let mut menu = entries(menu, items);
                    menu.select_first_item(cx);
                    menu
                })
            })
        });
        draw(cx);
    }

    const OPEN_RENAME: &[(&str, bool)] = &[("Open", true), ("Rename", true)];
    const OPEN_REFUSED: &[(&str, bool)] = &[("Open", false), ("Rename", true)];

    /// Where the cursor's item can no longer act, a rebuild rests the
    /// cursor on nothing, though another item can act, and the builder's
    /// first item does not take it; built so again, it still rests; once its
    /// item can act again, the cursor is back on it. Moved to the nearest
    /// item that could act, the cursor of a project's menu rested on Rename
    /// as a write refused Open, stayed there once the write ended, and Enter
    /// renamed where the person had chosen to open.
    #[gpui::test]
    fn a_rebuilt_menu_rests_the_cursor_until_its_item_can_act(cx: &mut TestAppContext) {
        let (menu, cx) = open(cx, OPEN_RENAME);
        cx.simulate_keystrokes("down");
        draw(cx);
        assert_eq!(cursor(&menu, cx).as_deref(), Some("Open"), "Down");

        rebuilt_from_keyboard(&menu, cx, OPEN_REFUSED);
        assert_eq!(cursor(&menu, cx).as_deref(), None, "Open refused");
        rebuilt_from_keyboard(&menu, cx, OPEN_REFUSED);
        assert_eq!(
            cursor(&menu, cx).as_deref(),
            None,
            "built again, Open still refused"
        );
        rebuilt_from_keyboard(&menu, cx, OPEN_RENAME);
        assert_eq!(
            cursor(&menu, cx).as_deref(),
            Some("Open"),
            "back on Open once it can act"
        );

        rebuilt(&menu, cx, &[("Rename", true)]);
        assert_eq!(cursor(&menu, cx).as_deref(), None, "Open gone");
        rebuilt(
            &menu,
            cx,
            &[("New", true), ("Open", true), ("Rename", true)],
        );
        assert_eq!(
            cursor(&menu, cx).as_deref(),
            Some("Open"),
            "back on Open once it is offered again, where it now stands"
        );
    }

    /// From a cursor a rebuild rests, Up and Down start from the place of
    /// the item it remembers: Up moves to the item that can act before it
    /// and Down to the one after it, or, where the item is gone, to the item
    /// that took its place.
    #[gpui::test]
    fn the_menu_keys_start_from_a_resting_cursors_item(cx: &mut TestAppContext) {
        const TWO_REFUSED: &[(&str, bool)] = &[
            ("One", true),
            ("Two", false),
            ("Three", true),
            ("Four", true),
        ];
        const TWO_GONE: &[(&str, bool)] = &[("One", true), ("Three", true), ("Four", true)];
        for (items, key, after) in [
            (TWO_REFUSED, "down", "Three"),
            (TWO_REFUSED, "up", "One"),
            (TWO_GONE, "down", "Three"),
            (TWO_GONE, "up", "One"),
        ] {
            let (menu, cx) = open(cx, FOUR);
            cx.simulate_keystrokes("down down");
            draw(cx);
            assert_eq!(cursor(&menu, cx).as_deref(), Some("Two"), "Down twice");
            rebuilt(&menu, cx, items);
            assert_eq!(cursor(&menu, cx).as_deref(), None, "{items:?}: at rest");
            cx.simulate_keystrokes(key);
            draw(cx);
            assert_eq!(
                cursor(&menu, cx).as_deref(),
                Some(after),
                "{items:?}: {key} from rest"
            );
        }
    }

    /// A menu key or the pointer that moves the selection from a cursor a
    /// rebuild rests ends the remembering: once the remembered item can act
    /// again, the cursor stays where the key moved it, and after the
    /// pointer's selection nothing is selected.
    #[gpui::test]
    fn a_key_or_the_pointer_ends_a_resting_cursors_remembering(cx: &mut TestAppContext) {
        const TWO_REFUSED: &[(&str, bool)] = &[
            ("One", true),
            ("Two", false),
            ("Three", true),
            ("Four", true),
        ];
        let (menu, cx) = open(cx, FOUR);
        cx.simulate_keystrokes("down down");
        draw(cx);
        rebuilt(&menu, cx, TWO_REFUSED);
        assert_eq!(cursor(&menu, cx).as_deref(), None, "at rest");
        cx.simulate_keystrokes("down");
        draw(cx);
        rebuilt(&menu, cx, FOUR);
        assert_eq!(
            cursor(&menu, cx).as_deref(),
            Some("Three"),
            "where Down moved it, not back on Two"
        );

        cx.simulate_keystrokes("up");
        draw(cx);
        assert_eq!(cursor(&menu, cx).as_deref(), Some("Two"), "Up");
        rebuilt(&menu, cx, TWO_REFUSED);
        assert_eq!(cursor(&menu, cx).as_deref(), None, "at rest again");
        let four = cx
            .debug_bounds("menu-item: Four · MenuItem")
            .expect("Four drawn");
        cx.simulate_mouse_move(four.center(), None, Modifiers::default());
        draw(cx);
        assert_eq!(
            menu.read_with(cx, |menu, _| (menu.selected_index, menu.keyboard_selection)),
            (Some(4), false),
            "the pointer on Four"
        );
        rebuilt(&menu, cx, FOUR);
        assert_eq!(
            menu.read_with(cx, |menu, _| menu.selected_index),
            None,
            "nothing selected, not Two"
        );
    }

    /// A rebuild finds the cursor's item by its order among the items of
    /// its name: the second of two Copy items stays the cursor's as items
    /// are added above both. Taken nearest its old place, the first Copy,
    /// which the added items moved there, took the cursor.
    #[gpui::test]
    fn a_rebuilt_menu_finds_a_repeated_name_by_its_order(cx: &mut TestAppContext) {
        let (menu, cx) = open(cx, &[("Copy", true), ("Other", true), ("Copy", true)]);
        cx.simulate_keystrokes("down down down");
        draw(cx);
        let selected =
            |cx: &mut VisualTestContext| menu.read_with(cx, |menu, _| menu.selected_index);
        assert_eq!(
            selected(cx),
            Some(3),
            "Down three times, on the second Copy"
        );
        rebuilt(
            &menu,
            cx,
            &[
                ("New", true),
                ("Paste", true),
                ("Copy", true),
                ("Other", true),
                ("Copy", true),
            ],
        );
        assert_eq!(selected(cx), Some(5), "still on the second Copy");
    }

    /// Twelve items under a heading.
    const TWELVE: &[(&str, bool)] = &[
        ("Item 0", true),
        ("Item 1", true),
        ("Item 2", true),
        ("Item 3", true),
        ("Item 4", true),
        ("Item 5", true),
        ("Item 6", true),
        ("Item 7", true),
        ("Item 8", true),
        ("Item 9", true),
        ("Item 10", true),
        ("Item 11", true),
    ];

    /// [`TWELVE`] with Item 5 refused.
    const ITEM_5_REFUSED: &[(&str, bool)] = &[
        ("Item 0", true),
        ("Item 1", true),
        ("Item 2", true),
        ("Item 3", true),
        ("Item 4", true),
        ("Item 5", false),
        ("Item 6", true),
        ("Item 7", true),
        ("Item 8", true),
        ("Item 9", true),
        ("Item 10", true),
        ("Item 11", true),
    ];

    /// Builds the open menu again with `items`, capped at a few rows, as a
    /// menu opened from the keyboard is built, its first item that can act
    /// selected.
    fn rebuilt_capped(
        menu: &Entity<PopupMenu>,
        cx: &mut VisualTestContext,
        items: &'static [(&'static str, bool)],
    ) {
        cx.update(|window, cx| {
            menu.update(cx, |menu, cx| {
                menu.rebuild(window, cx, |menu, _, cx| {
                    let mut menu = entries(menu, items).max_h(px(120.)).scrollable(true);
                    menu.select_first_item(cx);
                    menu
                })
            })
        });
        draw(cx);
    }

    /// A rebuild that leaves the cursor on its row scrolls a capped menu
    /// nowhere, however the wheel left it, and neither does the builder's
    /// selection of its first item; one that moves the cursor to another
    /// row, as it returns from rest, scrolls its row into view. The rebuild
    /// once scrolled back to the cursor every time.
    #[gpui::test]
    fn a_rebuild_scrolls_a_capped_menu_only_as_the_cursor_changes_row(cx: &mut TestAppContext) {
        let (menu, cx) = open(cx, TWELVE);
        rebuilt_capped(&menu, cx, TWELVE);
        cx.simulate_keystrokes("down down down down down");
        draw(cx);
        assert_eq!(
            cursor(&menu, cx).as_deref(),
            Some("Item 5"),
            "Down five times"
        );
        let offset = |cx: &mut VisualTestContext| {
            menu.read_with(cx, |menu, _| menu.scroll_handle.offset().y)
        };
        let max = menu.read_with(cx, |menu, _| menu.scroll_handle.max_offset().y);
        assert!(max > px(0.), "the capped menu scrolls");
        menu.update(cx, |menu, _| {
            menu.scroll_handle.set_offset(gpui::point(px(0.), -max))
        });
        draw(cx);
        assert_eq!(offset(cx), -max, "scrolled to the end, past Item 5");

        rebuilt_capped(&menu, cx, TWELVE);
        assert_eq!(cursor(&menu, cx).as_deref(), Some("Item 5"), "built again");
        assert_eq!(offset(cx), -max, "the cursor on its row, the menu stays");
        rebuilt_capped(&menu, cx, ITEM_5_REFUSED);
        assert_eq!(cursor(&menu, cx).as_deref(), None, "Item 5 refused");
        assert_eq!(offset(cx), -max, "at rest, the menu stays");

        rebuilt_capped(&menu, cx, TWELVE);
        assert_eq!(
            cursor(&menu, cx).as_deref(),
            Some("Item 5"),
            "back on Item 5"
        );
        let (viewport, item) = (
            menu.read_with(cx, |menu, _| menu.scroll_handle.bounds()),
            cx.debug_bounds("menu-item: Item 5 · MenuItem")
                .expect("Item 5"),
        );
        assert!(
            item.top() >= viewport.top() && item.bottom() <= viewport.bottom(),
            "Item 5 {item:?} scrolled into the menu's view {viewport:?}"
        );
    }

    /// The pointer on a row selects it, and a rebuild whose builder selects
    /// its first item, as a menu opened from the keyboard is built, then
    /// selects nothing: the pointer owns the selection. The pointer leaving
    /// the row that holds the keyboard's cursor takes the cursor away, and a
    /// rebuild then remembers no item and selects none, though the menu had
    /// remembered that item while it was refused and put the cursor back on
    /// it. A key press stops GPUI reporting the pointer's row hovered until
    /// the pointer moves, so the cursor the menu opening from the keyboard
    /// puts on the row under the still pointer is one it can leave. The
    /// builder's first item once took the cursor each time, so Enter ran an
    /// item neither the keys nor the pointer had chosen.
    #[gpui::test]
    fn a_rebuild_after_the_pointer_leaves_the_cursors_row_keeps_no_cursor(cx: &mut TestAppContext) {
        const TWO_FIRST: &[(&str, bool)] = &[("Two", true), ("Three", true)];
        const TWO_REFUSED: &[(&str, bool)] = &[("Two", false), ("Three", true)];
        let (menu, cx) = open(cx, TWO_FIRST);
        let two = cx
            .debug_bounds("menu-item: Two · MenuItem")
            .expect("Two drawn");
        let three = cx
            .debug_bounds("menu-item: Three · MenuItem")
            .expect("Three drawn");
        let selection = |cx: &mut VisualTestContext| {
            menu.read_with(cx, |menu, _| (menu.selected_index, menu.keyboard_selection))
        };
        cx.simulate_mouse_move(three.center(), None, Modifiers::default());
        draw(cx);
        assert_eq!(selection(cx), (Some(2), false), "the pointer on Three");
        rebuilt_from_keyboard(&menu, cx, TWO_FIRST);
        assert_eq!(
            selection(cx).0,
            None,
            "the pointer's selection: the builder's first item, Two, does not take it"
        );

        cx.simulate_mouse_move(two.center(), None, Modifiers::default());
        draw(cx);
        assert_eq!(selection(cx), (Some(1), false), "the pointer on Two");
        menu.update(cx, |menu, cx| menu.select_first_item(cx));
        draw(cx);
        assert_eq!(
            cursor(&menu, cx).as_deref(),
            Some("Two"),
            "opened from the keyboard, the cursor on Two, under the pointer"
        );
        rebuilt(&menu, cx, TWO_REFUSED);
        assert_eq!(cursor(&menu, cx).as_deref(), None, "Two refused");
        rebuilt(&menu, cx, TWO_FIRST);
        assert_eq!(cursor(&menu, cx).as_deref(), Some("Two"), "back on Two");

        cx.simulate_mouse_move(
            gpui::point(two.center().x, three.bottom() + px(120.)),
            None,
            Modifiers::default(),
        );
        draw(cx);
        assert_eq!(selection(cx).0, None, "the pointer left Two's row");
        rebuilt_from_keyboard(&menu, cx, TWO_FIRST);
        assert_eq!(
            selection(cx).0,
            None,
            "built again from the keyboard, nothing selected"
        );
    }

    /// A selection the pointer made is cleared by a rebuild, as upstream:
    /// the pointer's place decides what it lights. It stays cleared where
    /// the builder selects its first item, as a menu opened from the
    /// keyboard is built, however often the menu is built again, until a
    /// menu key moves the cursor, which a rebuild then keeps.
    #[gpui::test]
    fn a_rebuilt_menu_clears_the_pointers_selection(cx: &mut TestAppContext) {
        let (menu, cx) = open(cx, FOUR);
        let three = cx
            .debug_bounds("menu-item: Three · MenuItem")
            .expect("Three drawn");
        cx.simulate_mouse_move(three.center(), None, Modifiers::default());
        draw(cx);
        let selected =
            |cx: &mut VisualTestContext| menu.read_with(cx, |menu, _| menu.selected_index);
        assert_eq!(selected(cx), Some(3), "the pointer on Three");
        rebuilt(&menu, cx, FOUR);
        assert_eq!(selected(cx), None, "nothing selected");
        rebuilt_from_keyboard(&menu, cx, FOUR);
        assert_eq!(
            selected(cx),
            None,
            "nothing selected by a keyboard's builder"
        );
        rebuilt_from_keyboard(&menu, cx, FOUR);
        assert_eq!(selected(cx), None, "nor built so again");

        cx.simulate_keystrokes("down down");
        draw(cx);
        assert_eq!(cursor(&menu, cx).as_deref(), Some("Two"), "Down twice");
        rebuilt_from_keyboard(&menu, cx, FOUR);
        assert_eq!(
            cursor(&menu, cx).as_deref(),
            Some("Two"),
            "the keys' cursor kept"
        );
    }

    /// A view that draws a menu `top` below the window's top and `left` in
    /// from its left, as a row's menu is drawn beside its row, at the size
    /// its rows give it.
    struct Placed {
        menu: Entity<PopupMenu>,
        top: Pixels,
        left: Pixels,
    }

    impl Render for Placed {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .items_start()
                .pt(self.top)
                .pl(self.left)
                .child(self.menu.clone())
        }
    }

    /// The names of the items that ran, in order.
    type Ran = Rc<std::cell::RefCell<Vec<&'static str>>>;

    /// [`entries`] whose items, when they act, add their name to `ran`.
    fn acting(menu: PopupMenu, items: &[(&'static str, bool)], ran: &Ran) -> PopupMenu {
        items
            .iter()
            .fold(menu.label("Heading"), |menu, (name, enabled)| {
                let (ran, name) = (ran.clone(), *name);
                menu.item(
                    PopupMenuItem::new(name)
                        .disabled(!enabled)
                        .on_click(move |_, _, _| ran.borrow_mut().push(name)),
                )
            })
    }

    /// A focused menu of `items` that run into `ran`, drawn at the window's
    /// top left at the size its rows give it, with the menu keys bound and
    /// its fills painted.
    fn placed<'a>(
        cx: &'a mut TestAppContext,
        items: &'static [(&'static str, bool)],
        ran: &Ran,
    ) -> (Entity<Placed>, Entity<PopupMenu>, &'a mut VisualTestContext) {
        cx.update(|cx| {
            // The kit's default colors are transparent; the pointer's fill
            // and the keyboard cursor's are set so the frame paints them.
            let mut colors = crate::theme::ThemeColor::default();
            colors.accent = gpui::red();
            colors.list_active = gpui::blue();
            cx.set_global(Theme::from(&colors));
            init(cx);
        });
        let ran = ran.clone();
        let (placed, cx) = cx.add_window_view(move |window, cx| {
            let menu = cx.new(|cx| acting(PopupMenu::new(cx), items, &ran));
            let focus = menu.read(cx).focus_handle.clone();
            focus.focus(window, cx);
            Placed {
                menu,
                top: px(0.),
                left: px(0.),
            }
        });
        draw(cx);
        let menu = placed.read_with(cx, |placed, _| placed.menu.clone());
        (placed, menu, cx)
    }

    /// The items of [`FOUR`] drawn with a fill of their own, read from the
    /// frame on screen: the pointer's selection or hover, or the keyboard's
    /// cursor.
    fn lit(cx: &mut VisualTestContext) -> Vec<&'static str> {
        let four: Vec<_> = FOUR.iter().map(|(name, _)| *name).collect();
        lit_among(cx, &four)
    }

    /// The items of `names` drawn with a fill of their own, as [`lit`].
    fn lit_among(cx: &mut VisualTestContext, names: &[&'static str]) -> Vec<&'static str> {
        names
            .iter()
            .copied()
            .filter(|name| {
                let row = drawn_item(cx, name);
                cx.update(|window, _| {
                    let scale = window.scale_factor();
                    let near = |device: gpui::ScaledPixels, logical: Pixels| {
                        (device.as_f32() - logical.as_f32() * scale).abs() <= 1.
                    };
                    window.painted_quads().iter().any(|quad| {
                        !quad.background.is_transparent()
                            && near(quad.bounds.left(), row.left())
                            && near(quad.bounds.top(), row.top())
                            && near(quad.bounds.right(), row.right())
                            && near(quad.bounds.bottom(), row.bottom())
                    })
                })
            })
            .collect()
    }

    /// Builds the open menu again with `items` that run into `ran`.
    fn rebuilt_acting(
        menu: &Entity<PopupMenu>,
        cx: &mut VisualTestContext,
        items: &'static [(&'static str, bool)],
        ran: &Ran,
    ) {
        let ran = ran.clone();
        cx.update(|window, cx| {
            menu.update(cx, |menu, cx| {
                menu.rebuild(window, cx, |menu, _, _| acting(menu, items, &ran))
            })
        });
        draw(cx);
    }

    /// The item named `name`, as drawn.
    fn drawn_item(cx: &mut VisualTestContext, name: &str) -> Bounds<Pixels> {
        cx.debug_bounds(format!("menu-item: {name} · MenuItem").leak())
            .unwrap_or_else(|| panic!("{name} drawn"))
    }

    /// A menu drawn elsewhere under a pointer that stays put, as a row's
    /// menu follows its row when rows arrive above it, lets none of the rows
    /// that arrive under the pointer take the selection until it moves: the
    /// row the pointer had selected gives the selection up, and a keyboard's
    /// cursor stays on its item. GPUI reports the row that arrives under a
    /// still pointer hovered once the frame is drawn, and upstream gave it
    /// the selection, so Enter ran an item the person never pointed at.
    #[gpui::test]
    fn a_menu_that_moves_under_the_still_pointer_selects_nothing(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(Theme::default());
            init(cx);
        });
        let (placed, cx) = cx.add_window_view(|_, cx| Placed {
            menu: cx.new(|cx| entries(PopupMenu::new(cx), FOUR)),
            top: px(0.),
            left: px(0.),
        });
        draw(cx);
        let menu = placed.read_with(cx, |placed, _| placed.menu.clone());
        let item = |cx: &mut VisualTestContext, name: &str| {
            cx.debug_bounds(format!("menu-item: {name} · MenuItem").leak())
                .unwrap_or_else(|| panic!("{name} drawn"))
        };
        let selection = |cx: &mut VisualTestContext| {
            menu.read_with(cx, |menu, _| (menu.selected_index, menu.keyboard_selection))
        };
        let place = |cx: &mut VisualTestContext, top: Pixels| {
            placed.update(cx, |placed, cx| {
                placed.top = top;
                cx.notify();
            });
            draw(cx);
        };
        let three = item(cx, "Three");
        let pitch = three.top() - item(cx, "Two").top();
        let pointer = three.center();
        cx.simulate_mouse_move(pointer, None, Modifiers::default());
        draw(cx);
        assert_eq!(selection(cx), (Some(3), false), "the pointer on Three");

        place(cx, pitch);
        assert!(
            item(cx, "Two").contains(&pointer),
            "the menu moved down a row, Two under the pointer"
        );
        assert_eq!(
            selection(cx).0,
            None,
            "nothing selected: Two arrived under the still pointer"
        );
        cx.simulate_mouse_move(
            pointer + gpui::point(px(1.), px(0.)),
            None,
            Modifiers::default(),
        );
        draw(cx);
        assert_eq!(selection(cx), (Some(2), false), "the moved pointer on Two");

        menu.update(cx, |menu, cx| menu.select_first_item(cx));
        draw(cx);
        assert_eq!(
            cursor(&menu, cx).as_deref(),
            Some("One"),
            "the cursor on One"
        );
        place(cx, px(0.));
        assert!(
            item(cx, "Three").contains(&pointer),
            "the menu moved up a row, Three under the pointer"
        );
        assert_eq!(
            cursor(&menu, cx).as_deref(),
            Some("One"),
            "the keyboard's cursor stays on One"
        );
    }

    /// A menu built again wider at its place, as History's commit menu gains
    /// its Amend and Undo items when its commit becomes HEAD, under a pointer
    /// resting just beside it, brings Two under the pointer: the keyboard's
    /// cursor stays on Three and Enter runs Three. Compared by its origin
    /// alone, the menu that grew had not moved, so Two took the selection
    /// from the keyboard, and Enter ran Two.
    #[gpui::test]
    fn a_menu_that_grows_wider_under_the_still_pointer_keeps_the_cursor(cx: &mut TestAppContext) {
        const WIDER: &[(&str, bool)] = &[
            ("One", true),
            ("Two", true),
            ("Three", true),
            ("Four", true),
            ("Undo the last local commit on this branch…", true),
        ];
        let ran = Ran::default();
        let (_, menu, cx) = placed(cx, FOUR, &ran);
        cx.simulate_keystrokes("down down down");
        draw(cx);
        assert_eq!(cursor(&menu, cx).as_deref(), Some("Three"), "Down thrice");
        let surface = menu.read_with(cx, |menu, _| menu.bounds);
        let pointer = gpui::point(surface.right() + px(12.), drawn_item(cx, "Two").center().y);
        cx.simulate_mouse_move(pointer, None, Modifiers::default());
        draw(cx);
        assert_eq!(
            cursor(&menu, cx).as_deref(),
            Some("Three"),
            "the pointer resting beside the menu"
        );

        rebuilt_acting(&menu, cx, WIDER, &ran);
        assert!(
            drawn_item(cx, "Two").contains(&pointer),
            "the menu grew wider, Two under the pointer"
        );
        assert_eq!(
            cursor(&menu, cx).as_deref(),
            Some("Three"),
            "the keyboard's cursor stays on Three"
        );
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert_eq!(*ran.borrow(), ["Three"], "Enter runs Three");
    }

    /// A menu built again taller at its place under a pointer resting just
    /// below it brings the item it adds, Five, under the pointer: nothing is
    /// selected and Enter runs nothing until the pointer moves, which
    /// selects Five. Compared by its origin alone, the menu had not moved,
    /// so Five took the selection and Enter ran it.
    #[gpui::test]
    fn a_menu_that_grows_taller_under_the_still_pointer_selects_nothing(cx: &mut TestAppContext) {
        const FIVE: &[(&str, bool)] = &[
            ("One", true),
            ("Two", true),
            ("Three", true),
            ("Four", true),
            ("Five", true),
        ];
        let ran = Ran::default();
        let (_, menu, cx) = placed(cx, FOUR, &ran);
        let selection = |cx: &mut VisualTestContext| {
            menu.read_with(cx, |menu, _| (menu.selected_index, menu.keyboard_selection))
        };
        let surface = menu.read_with(cx, |menu, _| menu.bounds);
        let pointer = gpui::point(drawn_item(cx, "Four").center().x, surface.bottom() + px(6.));
        cx.simulate_mouse_move(pointer, None, Modifiers::default());
        draw(cx);
        assert_eq!(selection(cx).0, None, "the pointer resting below the menu");

        rebuilt_acting(&menu, cx, FIVE, &ran);
        assert!(
            drawn_item(cx, "Five").contains(&pointer),
            "the menu grew taller, Five under the pointer"
        );
        assert_eq!(selection(cx).0, None, "nothing selected");
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert!(
            ran.borrow().is_empty(),
            "Enter runs nothing: {:?}",
            ran.borrow()
        );

        cx.simulate_mouse_move(
            pointer + gpui::point(px(1.), px(0.)),
            None,
            Modifiers::default(),
        );
        draw(cx);
        assert_eq!(selection(cx), (Some(5), false), "the moved pointer on Five");
    }

    /// A menu moved sideways, or down by less than a row, under the pointer
    /// on Three keeps Three selected, drawn lit, and Enter runs it, in the
    /// frames the move itself asks for: the item under the pointer is the
    /// one the person put it on. A keyboard's cursor on One with the pointer
    /// on Three, which draws its hover, keeps only the cursor drawn once a
    /// move makes the pointer still. The prepaint once gave the pointer's
    /// selection up with a notify that asked for no frame, so Three stayed
    /// drawn lit while Enter did nothing, until something else drew the
    /// menu; and the still pointer's hover stayed drawn.
    #[gpui::test]
    fn a_menu_moved_under_the_pointer_keeps_the_item_still_under_it(cx: &mut TestAppContext) {
        let ran = Ran::default();
        let (placed, menu, cx) = placed(cx, FOUR, &ran);
        let selection = |cx: &mut VisualTestContext| {
            menu.read_with(cx, |menu, _| (menu.selected_index, menu.keyboard_selection))
        };
        // Moves the menu with no draw of its own: the frames are those the
        // move asks for, as the app's are.
        let moved = |cx: &mut VisualTestContext, left: Pixels, top: Pixels| {
            placed.update(cx, |placed, cx| {
                placed.left = left;
                placed.top = top;
                cx.notify();
            });
        };
        let three = drawn_item(cx, "Three");
        let pitch = three.top() - drawn_item(cx, "Two").top();
        let pointer = three.center();
        cx.simulate_mouse_move(pointer, None, Modifiers::default());
        draw(cx);
        assert_eq!(selection(cx), (Some(3), false), "the pointer on Three");
        assert_eq!(lit(cx), ["Three"], "Three lit");

        for (left, top, at) in [
            (px(8.), px(0.), "moved 8 px sideways"),
            (px(8.), pitch / 3., "moved down a third of a row"),
        ] {
            moved(cx, left, top);
            assert!(
                drawn_item(cx, "Three").contains(&pointer),
                "{at}: Three still under the pointer"
            );
            assert_eq!(selection(cx), (Some(3), false), "{at}: Three selected");
            assert_eq!(lit(cx), ["Three"], "{at}: Three drawn lit");
        }
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert_eq!(*ran.borrow(), ["Three"], "Enter runs Three");

        cx.simulate_mouse_move(
            pointer + gpui::point(px(1.), px(0.)),
            None,
            Modifiers::default(),
        );
        draw(cx);
        assert_eq!(
            selection(cx),
            (Some(3), false),
            "the moved pointer on Three"
        );
        menu.update(cx, |menu, cx| menu.select_first_item(cx));
        draw(cx);
        assert_eq!(
            cursor(&menu, cx).as_deref(),
            Some("One"),
            "the cursor on One"
        );
        assert_eq!(
            lit(cx),
            ["One", "Three"],
            "the cursor on One, the pointer's hover on Three"
        );
        moved(cx, px(0.), pitch / 3.);
        assert!(
            drawn_item(cx, "Three").contains(&(pointer + gpui::point(px(1.), px(0.)))),
            "moved 8 px back, Three still under the pointer"
        );
        assert_eq!(cursor(&menu, cx).as_deref(), Some("One"), "the cursor kept");
        assert_eq!(
            lit(cx),
            ["One"],
            "only the cursor drawn under the still pointer"
        );
    }

    /// A capped menu whose rows are not the heights it estimates before its
    /// first layout, as a tall element row is not, draws the fold that
    /// layout measures in the frame the measure asks for, without another
    /// draw. The measure's notify once ran in prepaint, which asks for no
    /// frame, so the menu kept the estimated fold it was first drawn with,
    /// cutting a row or leaving a sliver of one, until something else drew
    /// it.
    #[gpui::test]
    fn a_capped_menu_draws_its_measured_fold_without_another_draw(cx: &mut TestAppContext) {
        let (menu, cx) = open(cx, TWELVE);
        cx.update(|window, cx| {
            menu.update(cx, |menu, cx| {
                menu.rebuild(window, cx, |menu, _, _| {
                    let tall = PopupMenuItem::element(|_, _| div().h(px(90.)).child("Tall"));
                    entries(menu.item(tall), TWELVE)
                        .max_h(px(160.))
                        .scrollable(true)
                })
            })
        });
        let (fold, drawn) = menu.read_with(cx, |menu, _| {
            (menu.fold, menu.scroll_handle.bounds().size.height)
        });
        assert_eq!(
            fold,
            Some(Fold::At(drawn)),
            "the items column drawn as tall as the measured fold"
        );
        assert!(
            cx.debug_bounds("popup-menu-scrollbar").is_some(),
            "the fold drawn with its scrollbar"
        );
    }

    /// Rows that move under a pointer that has not moved, while the
    /// keyboard owns the cursor, leave the cursor where it is, though the
    /// menu itself stays where it was: a capped menu scrolled a row, as a
    /// rebuild scrolls the cursor's row into view, brings Item 2 under the
    /// pointer resting on Item 1, and the cursor stays on Item 0 until the
    /// pointer moves, which selects Item 2. The hover once took the
    /// keyboard's cursor for the row that arrived.
    #[gpui::test]
    fn rows_scrolled_under_the_still_pointer_keep_the_keyboards_cursor(cx: &mut TestAppContext) {
        let (menu, cx) = open(cx, TWELVE);
        rebuilt_capped(&menu, cx, TWELVE);
        let selection = |cx: &mut VisualTestContext| {
            menu.read_with(cx, |menu, _| (menu.selected_index, menu.keyboard_selection))
        };
        let (one, two) = (drawn_item(cx, "Item 1"), drawn_item(cx, "Item 2"));
        let pointer = one.center();
        cx.simulate_mouse_move(pointer, None, Modifiers::default());
        draw(cx);
        assert_eq!(selection(cx), (Some(2), false), "the pointer on Item 1");
        menu.update(cx, |menu, cx| menu.select_first_item(cx));
        draw(cx);
        assert_eq!(
            cursor(&menu, cx).as_deref(),
            Some("Item 0"),
            "the keyboard's cursor on Item 0"
        );

        let pitch = two.top() - one.top();
        menu.update(cx, |menu, _| {
            menu.scroll_handle.set_offset(gpui::point(px(0.), -pitch))
        });
        draw(cx);
        assert!(
            drawn_item(cx, "Item 2").contains(&pointer),
            "scrolled a row, Item 2 under the pointer"
        );
        assert_eq!(
            cursor(&menu, cx).as_deref(),
            Some("Item 0"),
            "the keyboard's cursor stays on Item 0"
        );

        cx.simulate_mouse_move(
            pointer + gpui::point(px(1.), px(0.)),
            None,
            Modifiers::default(),
        );
        draw(cx);
        assert_eq!(
            selection(cx),
            (Some(3), false),
            "the moved pointer on Item 2"
        );
    }

    /// A capped menu scrolled to the end of its rows, built again with an
    /// item fewer under the pointer resting on Item 10, keeps its place and
    /// size, but GPUI clamps the rows' offset to the shorter content, so the
    /// rows move down a row and Item 9 comes under the pointer: no item is
    /// selected or drawn lit and Enter runs nothing until the pointer moves,
    /// which selects Item 9. The surface had not moved, so Item 9 took the
    /// selection and Enter ran it, as a branch switcher's row checks its
    /// branch out at once.
    #[gpui::test]
    fn a_capped_menu_built_again_shorter_under_the_still_pointer_selects_nothing(
        cx: &mut TestAppContext,
    ) {
        const ELEVEN: &[(&str, bool)] = TWELVE.split_at(11).0;
        let ran = Ran::default();
        let (_, menu, cx) = placed(cx, TWELVE, &ran);
        let capped = |cx: &mut VisualTestContext, items: &'static [(&'static str, bool)]| {
            let ran = ran.clone();
            cx.update(|window, cx| {
                menu.update(cx, |menu, cx| {
                    menu.rebuild(window, cx, |menu, _, _| {
                        acting(menu, items, &ran).max_h(px(120.)).scrollable(true)
                    })
                })
            });
            draw(cx);
        };
        let selection = |cx: &mut VisualTestContext| {
            menu.read_with(cx, |menu, _| (menu.selected_index, menu.keyboard_selection))
        };
        capped(cx, TWELVE);
        let max = menu.read_with(cx, |menu, _| menu.scroll_handle.max_offset().y);
        assert!(max > px(0.), "the capped menu scrolls");
        menu.update(cx, |menu, _| {
            menu.scroll_handle.set_offset(gpui::point(px(0.), -max))
        });
        draw(cx);
        let pointer = drawn_item(cx, "Item 10").center();
        cx.simulate_mouse_move(pointer, None, Modifiers::default());
        draw(cx);
        assert_eq!(selection(cx), (Some(11), false), "the pointer on Item 10");
        let surface = menu.read_with(cx, |menu, _| menu.bounds);

        capped(cx, ELEVEN);
        assert_eq!(
            menu.read_with(cx, |menu, _| menu.bounds),
            surface,
            "the menu keeps its place and size"
        );
        assert!(
            drawn_item(cx, "Item 9").contains(&pointer),
            "the rows moved down a row, Item 9 under the pointer"
        );
        assert_eq!(selection(cx).0, None, "nothing selected");
        let rows = ["Item 8", "Item 9", "Item 10"];
        assert!(lit_among(cx, &rows).is_empty(), "nothing drawn lit");
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert!(
            ran.borrow().is_empty(),
            "Enter runs nothing: {:?}",
            ran.borrow()
        );

        cx.simulate_mouse_move(
            pointer + gpui::point(px(1.), px(0.)),
            None,
            Modifiers::default(),
        );
        draw(cx);
        assert_eq!(
            selection(cx),
            (Some(10), false),
            "the moved pointer on Item 9"
        );
        assert_eq!(lit_among(cx, &rows), ["Item 9"], "Item 9 drawn lit");
    }

    /// The item GPUI last reported under the pointer is known by its index,
    /// and GPUI reports nothing for a row no longer drawn: with the pointer
    /// on Five, a rebuild without Five, then one with a separator that puts
    /// Four at Five's index away from the pointer, a move onto the menu's
    /// padding, where no row lies, selects nothing and Enter runs nothing.
    /// That move once gave the selection to the row at the index GPUI had
    /// last reported, Four, though the pointer never went near it, and Enter
    /// ran Four.
    #[gpui::test]
    fn a_move_onto_no_row_selects_no_row_the_pointer_left_by_a_rebuild(cx: &mut TestAppContext) {
        const FIVE: &[(&str, bool)] = &[
            ("One", true),
            ("Two", true),
            ("Three", true),
            ("Four", true),
            ("Five", true),
        ];
        let ran = Ran::default();
        let (_, menu, cx) = placed(cx, FIVE, &ran);
        let selection = |cx: &mut VisualTestContext| {
            menu.read_with(cx, |menu, _| (menu.selected_index, menu.keyboard_selection))
        };
        let pointer = drawn_item(cx, "Five").center();
        cx.simulate_mouse_move(pointer, None, Modifiers::default());
        draw(cx);
        assert_eq!(selection(cx), (Some(5), false), "the pointer on Five");

        rebuilt_acting(&menu, cx, FOUR, &ran);
        let r = ran.clone();
        cx.update(|window, cx| {
            menu.update(cx, |menu, cx| {
                menu.rebuild(window, cx, move |menu, _, _| {
                    let menu = acting(menu, &FOUR[..1], &r).separator();
                    FOUR[1..].iter().fold(menu, |menu, (name, _)| {
                        let (r, name) = (r.clone(), *name);
                        menu.item(
                            PopupMenuItem::new(name)
                                .on_click(move |_, _, _| r.borrow_mut().push(name)),
                        )
                    })
                })
            })
        });
        draw(cx);
        let four = drawn_item(cx, "Four");
        assert_eq!(
            menu.read_with(cx, |menu, _| menu.menu_items[5].name().cloned()),
            Some("Four".into()),
            "Four at Five's index"
        );
        assert!(!four.contains(&pointer), "Four away from the pointer");
        let surface = menu.read_with(cx, |menu, _| menu.bounds);
        let padding = gpui::point(four.center().x, surface.top() + px(2.));
        let heading = cx
            .debug_bounds("menu-item: Heading · Label")
            .expect("Heading drawn");
        assert!(
            ["One", "Two", "Three", "Four"]
                .into_iter()
                .map(|name| drawn_item(cx, name))
                .chain([heading])
                .all(|row| !row.contains(&padding)),
            "no row at {padding:?}"
        );

        cx.simulate_mouse_move(padding, None, Modifiers::default());
        draw(cx);
        assert_eq!(selection(cx).0, None, "on no row, nothing selected");
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert!(
            ran.borrow().is_empty(),
            "Enter runs nothing: {:?}",
            ran.borrow()
        );
    }

    /// The row GPUI last reported under the pointer takes the selection on
    /// a move only where the last frame drew it under the pointer: with
    /// that report on Four, an index still in range but a row away from the
    /// pointer, as a rebuild can leave it, a still pointer's move onto the
    /// menu's padding selects nothing and Enter runs nothing. Without the
    /// check that move gave Four the selection.
    #[gpui::test]
    fn a_move_onto_padding_selects_no_reported_row_away_from_it(cx: &mut TestAppContext) {
        let ran = Ran::default();
        let (_, menu, cx) = placed(cx, FOUR, &ran);
        let selection = |cx: &mut VisualTestContext| {
            menu.read_with(cx, |menu, _| (menu.selected_index, menu.keyboard_selection))
        };
        let pointer = drawn_item(cx, "One").center();
        cx.simulate_mouse_move(pointer, None, Modifiers::default());
        draw(cx);
        assert_eq!(selection(cx), (Some(1), false), "the pointer on One");
        // Built again under the resting pointer, which is still from there.
        rebuilt_acting(&menu, cx, FOUR, &ran);
        assert!(
            menu.read_with(cx, |menu, _| menu.still_pointer.is_some()),
            "still after the rebuild"
        );
        // GPUI's last report, by index, on Four, away from the pointer.
        let four = drawn_item(cx, "Four");
        menu.update(cx, |menu, _| menu.hovered_index = Some(4));
        assert!(!four.contains(&pointer), "Four away from the pointer");
        let surface = menu.read_with(cx, |menu, _| menu.bounds);
        let padding = gpui::point(four.center().x, surface.top() + px(2.));
        let heading = cx
            .debug_bounds("menu-item: Heading · Label")
            .expect("Heading drawn");
        assert!(
            ["One", "Two", "Three", "Four"]
                .into_iter()
                .map(|name| drawn_item(cx, name))
                .chain([heading])
                .all(|row| !row.contains(&padding)),
            "no row at {padding:?}"
        );

        cx.simulate_mouse_move(padding, None, Modifiers::default());
        draw(cx);
        assert_eq!(selection(cx).0, None, "on no row, nothing selected");
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert!(
            ran.borrow().is_empty(),
            "Enter runs nothing: {:?}",
            ran.borrow()
        );
    }

    /// A pointer move in the frame after a rebuild, before the layout has
    /// recorded the new rows, finds no row drawn under it and selects
    /// nothing, but leaves the pointer still at its new place, so the next
    /// move, after the layout, selects the row under it. Cleared there, the
    /// pointer was no longer still, so each later move within the row
    /// returned at once, and GPUI reports no hover for a row the pointer
    /// stays on: the row stayed unselected, and Enter ran nothing, until the
    /// pointer crossed into another row.
    #[gpui::test]
    fn a_move_before_rebuilt_rows_are_laid_out_lets_the_next_move_decide(cx: &mut TestAppContext) {
        let ran = Ran::default();
        let (_, menu, cx) = placed(cx, FOUR, &ran);
        let selection = |cx: &mut VisualTestContext| {
            menu.read_with(cx, |menu, _| (menu.selected_index, menu.keyboard_selection))
        };
        let two = drawn_item(cx, "Two");
        let pointer = gpui::point(two.left() + px(8.), two.center().y);
        cx.simulate_mouse_move(pointer, None, Modifiers::default());
        draw(cx);
        assert_eq!(selection(cx), (Some(2), false), "the pointer on Two");

        // Built again, and the pointer moves within Two before a frame lays
        // the new rows out.
        let r = ran.clone();
        let moved = pointer + gpui::point(px(2.), px(0.));
        cx.update(|window, cx| {
            menu.update(cx, |menu, cx| {
                menu.rebuild(window, cx, move |menu, _, _| acting(menu, FOUR, &r))
            });
            assert!(
                menu.read(cx).rows.is_empty(),
                "no rows laid out since the rebuild"
            );
            window.dispatch_event(
                gpui::PlatformInput::MouseMove(MouseMoveEvent {
                    position: moved,
                    pressed_button: None,
                    modifiers: Modifiers::default(),
                }),
                cx,
            );
        });
        draw(cx);
        assert_eq!(selection(cx).0, None, "the move found no row laid out");

        // The next move within Two, after the layout, selects it.
        cx.simulate_mouse_move(
            moved + gpui::point(px(2.), px(0.)),
            None,
            Modifiers::default(),
        );
        draw(cx);
        assert_eq!(
            selection(cx),
            (Some(2), false),
            "the next move within Two selects it"
        );
        assert_eq!(lit(cx), ["Two"], "and only Two is lit");
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert_eq!(*ran.borrow(), ["Two"], "Enter runs Two");
    }

    /// A press and its release that straddle a rebuild run nothing: rows
    /// are wired, and GPUI keeps a pending press, by the item's index, so
    /// with the press on Three and a rebuild without One, which puts Four
    /// at Three's index and place, the release ran Four, which the person
    /// never pressed. A fresh click then runs the row drawn under the
    /// pointer.
    #[gpui::test]
    fn a_press_and_release_across_a_rebuild_run_nothing(cx: &mut TestAppContext) {
        const WITHOUT_ONE: &[(&str, bool)] = &[("Two", true), ("Three", true), ("Four", true)];
        let ran = Ran::default();
        let (_, menu, cx) = placed(cx, FOUR, &ran);
        let pointer = drawn_item(cx, "Three").center();
        cx.simulate_mouse_move(pointer, None, Modifiers::default());
        draw(cx);
        cx.simulate_mouse_down(pointer, gpui::MouseButton::Left, Modifiers::default());
        draw(cx);
        rebuilt_acting(&menu, cx, WITHOUT_ONE, &ran);
        assert_eq!(
            menu.read_with(cx, |menu, _| menu.menu_items[3].name().cloned()),
            Some("Four".into()),
            "Four at Three's index"
        );
        assert!(
            drawn_item(cx, "Four").contains(&pointer),
            "Four drawn under the pointer"
        );
        cx.simulate_mouse_up(pointer, gpui::MouseButton::Left, Modifiers::default());
        draw(cx);
        assert!(
            ran.borrow().is_empty(),
            "the release runs nothing: {:?}",
            ran.borrow()
        );

        cx.simulate_click(pointer, Modifiers::default());
        draw(cx);
        assert_eq!(*ran.borrow(), ["Four"], "a fresh click runs Four");
    }

    /// The primary button going down at `at`, delivered as GPUI delivers
    /// it, to the rows of the last frame drawn.
    fn press_at(at: Point<Pixels>, window: &mut Window, cx: &mut App) {
        window.dispatch_event(
            gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                button: gpui::MouseButton::Left,
                position: at,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
    }

    /// A press that goes down after a rebuild but before the menu is drawn
    /// again reaches the rows of the last frame drawn: on Three's row, with
    /// One gone and Four at Three's index, it records nothing, and its
    /// release runs nothing. It recorded Three's index, and the release ran
    /// Four. A fresh click then runs Four, the row drawn under the pointer.
    #[gpui::test]
    fn a_press_on_rows_drawn_before_a_rebuild_runs_nothing(cx: &mut TestAppContext) {
        const WITHOUT_ONE: &[(&str, bool)] = &[("Two", true), ("Three", true), ("Four", true)];
        let ran = Ran::default();
        let (_, menu, cx) = placed(cx, FOUR, &ran);
        let pointer = drawn_item(cx, "Three").center();
        cx.simulate_mouse_move(pointer, None, Modifiers::default());
        draw(cx);
        let r = ran.clone();
        cx.update(|window, cx| {
            menu.update(cx, |menu, cx| {
                menu.rebuild(window, cx, move |menu, _, _| acting(menu, WITHOUT_ONE, &r))
            });
            press_at(pointer, window, cx);
        });
        draw(cx);
        assert!(
            drawn_item(cx, "Four").contains(&pointer),
            "Four drawn under the pointer, at Three's index"
        );
        cx.simulate_mouse_up(pointer, gpui::MouseButton::Left, Modifiers::default());
        draw(cx);
        assert!(
            ran.borrow().is_empty(),
            "the release runs nothing: {:?}",
            ran.borrow()
        );
        cx.simulate_click(pointer, Modifiers::default());
        draw(cx);
        assert_eq!(*ran.borrow(), ["Four"], "a fresh click runs Four");
    }

    /// A menu of `items`, each running into `ran` as `name@target`, as an
    /// item of one label acts on whatever its menu was built for.
    fn targeted(
        menu: PopupMenu,
        items: &[&'static str],
        target: &'static str,
        ran: &Rc<std::cell::RefCell<Vec<String>>>,
    ) -> PopupMenu {
        items.iter().fold(menu.label("Heading"), |menu, name| {
            let (ran, name) = (ran.clone(), *name);
            menu.item(
                PopupMenuItem::new(name)
                    .on_click(move |_, _, _| ran.borrow_mut().push(format!("{name}@{target}"))),
            )
        })
    }

    /// A press across a rebuild runs nothing, whatever stands at its index
    /// with whatever label: a label is not an item's identity. Pressed on
    /// Two, built for A, and released after a rebuild for B with the same
    /// labels, as Undo last local commit… is built for the commit HEAD moves
    /// to, it ran Two for B; pressed on Two's row drawn for A after that
    /// rebuild, before a draw, the same; and with two items named Archive,
    /// the first pressed and a rebuild without One putting the second
    /// Archive at its index, it ran the other Archive. A fresh click then
    /// runs the item drawn under the pointer.
    #[gpui::test]
    fn a_press_across_a_rebuild_runs_nothing_whatever_its_label(cx: &mut TestAppContext) {
        const SAME: &[&str] = &["One", "Two", "Three"];
        const ARCHIVES: &[&str] = &["One", "Archive", "Archive"];
        const SHIFTED: &[&str] = &["Archive", "Archive"];
        let ran = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        cx.update(|cx| {
            cx.set_global(Theme::default());
            init(cx);
        });
        let r = ran.clone();
        let (placed, cx) = cx.add_window_view(move |window, cx| {
            let menu = cx.new(|cx| targeted(PopupMenu::new(cx), SAME, "A", &r));
            menu.read(cx).focus_handle.clone().focus(window, cx);
            Placed {
                menu,
                top: px(0.),
                left: px(0.),
            }
        });
        draw(cx);
        let menu = placed.read_with(cx, |placed, _| placed.menu.clone());
        let rebuilt_for = |cx: &mut VisualTestContext, items: &'static [&'static str], target| {
            let r = ran.clone();
            cx.update(|window, cx| {
                menu.update(cx, |menu, cx| {
                    menu.rebuild(window, cx, move |menu, _, _| {
                        targeted(menu, items, target, &r)
                    })
                })
            });
        };
        let left = gpui::MouseButton::Left;

        // Pressed for A, released after a rebuild for B.
        let two = drawn_item(cx, "Two").center();
        cx.simulate_mouse_move(two, None, Modifiers::default());
        draw(cx);
        cx.simulate_mouse_down(two, left, Modifiers::default());
        rebuilt_for(cx, SAME, "B");
        draw(cx);
        cx.simulate_mouse_up(two, left, Modifiers::default());
        draw(cx);
        assert!(
            ran.borrow().is_empty(),
            "same label, new target: {:?}",
            ran.borrow()
        );

        // Pressed on Two's row drawn for B, after a rebuild for C, before a
        // draw.
        let r = ran.clone();
        cx.update(|window, cx| {
            menu.update(cx, |menu, cx| {
                menu.rebuild(window, cx, move |menu, _, _| targeted(menu, SAME, "C", &r))
            });
            press_at(two, window, cx);
        });
        draw(cx);
        cx.simulate_mouse_up(two, left, Modifiers::default());
        draw(cx);
        assert!(
            ran.borrow().is_empty(),
            "a stale frame's press: {:?}",
            ran.borrow()
        );

        // Two items named Archive: the second shifts into the first's index.
        rebuilt_for(cx, ARCHIVES, "D");
        draw(cx);
        // Both items named Archive share a selector; the first lies a row
        // below One.
        let one = drawn_item(cx, "One");
        let first = gpui::point(one.center().x, one.center().y + one.size.height);
        assert_eq!(
            menu.read_with(cx, |menu, _| menu.menu_items[2].name().cloned()),
            Some("Archive".into()),
            "the first Archive at index 2"
        );
        cx.simulate_mouse_move(first, None, Modifiers::default());
        draw(cx);
        cx.simulate_mouse_down(first, left, Modifiers::default());
        rebuilt_for(cx, SHIFTED, "E");
        draw(cx);
        cx.simulate_mouse_up(first, left, Modifiers::default());
        draw(cx);
        assert!(
            ran.borrow().is_empty(),
            "a duplicate label shifted into the pressed index: {:?}",
            ran.borrow()
        );

        cx.simulate_click(first, Modifiers::default());
        draw(cx);
        assert_eq!(
            *ran.borrow(),
            ["Archive@E"],
            "a fresh click runs the Archive drawn under it"
        );
    }

    /// A rebuild that rests the keyboard's cursor because its item is still
    /// offered but refused reports that item once, as the rest begins: its
    /// name and its accessible name, which an element item that states why
    /// it waits carries apart from the name it is found by. A rebuild while
    /// the rest goes on reports nothing more, a rest after the item could
    /// act again is reported again, an item that is gone reports nothing,
    /// and a menu whose selection the pointer owns reports nothing.
    #[gpui::test]
    fn a_rebuild_that_rests_the_cursor_reports_the_waiting_item_once(cx: &mut TestAppContext) {
        const ONE_REFUSED: &[(&str, bool)] = &[
            ("One", false),
            ("Two", true),
            ("Three", true),
            ("Four", true),
        ];
        const WITHOUT_ONE: &[(&str, bool)] = &[("Two", true), ("Three", true), ("Four", true)];
        let (menu, cx) = open(cx, FOUR);
        let reports = Rc::new(std::cell::RefCell::new(Vec::<CursorRests>::new()));
        cx.update(|_, cx| {
            let reports = reports.clone();
            cx.subscribe(&menu, move |_, rest: &CursorRests, _| {
                reports.borrow_mut().push(rest.clone())
            })
            .detach();
        });
        let taken = |reports: &Rc<std::cell::RefCell<Vec<CursorRests>>>| {
            std::mem::take(&mut *reports.borrow_mut())
                .into_iter()
                .map(|rest| (rest.name.to_string(), rest.label.to_string()))
                .collect::<Vec<_>>()
        };
        cx.simulate_keystrokes("down");
        draw(cx);
        assert_eq!(cursor(&menu, cx).as_deref(), Some("One"), "Down");

        rebuilt(&menu, cx, ONE_REFUSED);
        assert_eq!(cursor(&menu, cx), None, "at rest");
        assert_eq!(
            taken(&reports),
            [("One".to_string(), "One".to_string())],
            "the rest begins"
        );
        rebuilt(&menu, cx, ONE_REFUSED);
        assert!(taken(&reports).is_empty(), "the rest goes on");

        rebuilt(&menu, cx, FOUR);
        assert_eq!(cursor(&menu, cx).as_deref(), Some("One"), "back on One");
        assert!(taken(&reports).is_empty(), "no rest");
        // An element item stating why it waits, found by its command.
        cx.update(|window, cx| {
            menu.update(cx, |menu, cx| {
                menu.rebuild(window, cx, |menu, _, _| {
                    FOUR[1..].iter().fold(
                        menu.label("Heading").item(
                            PopupMenuItem::element(|_, _| div().child("One"))
                                .named("One")
                                .accessibility_label("One, available when the write finishes")
                                .disabled(true),
                        ),
                        |menu, (name, _)| menu.item(PopupMenuItem::new(*name)),
                    )
                })
            })
        });
        draw(cx);
        assert_eq!(
            taken(&reports),
            [(
                "One".to_string(),
                "One, available when the write finishes".to_string()
            )],
            "a new rest, on the item that says why"
        );

        rebuilt(&menu, cx, FOUR);
        assert_eq!(cursor(&menu, cx).as_deref(), Some("One"), "back on One");
        rebuilt(&menu, cx, WITHOUT_ONE);
        assert!(taken(&reports).is_empty(), "One gone, nothing waits");

        // The pointer takes the selection: nothing rests.
        rebuilt(&menu, cx, FOUR);
        cx.simulate_keystrokes("down");
        draw(cx);
        let two = cx
            .debug_bounds("menu-item: Two · MenuItem")
            .expect("Two drawn");
        cx.simulate_mouse_move(two.center(), None, Modifiers::default());
        draw(cx);
        rebuilt(
            &menu,
            cx,
            &[
                ("One", true),
                ("Two", false),
                ("Three", true),
                ("Four", true),
            ],
        );
        assert!(
            taken(&reports).is_empty(),
            "the pointer's selection reports nothing"
        );
    }

    #[gpui::test]
    fn popup_menu_item_a11y_label_uses_visible_label(cx: &mut gpui::TestAppContext) {
        let submenu = cx.update(|cx| cx.new(|cx| PopupMenu::new(cx)));

        assert_eq!(PopupMenuItem::new("Open").a11y_label(), Some("Open".into()));
        assert_eq!(
            PopupMenuItem::link("Docs", "https://example.com").a11y_label(),
            Some("Docs".into())
        );
        assert_eq!(
            PopupMenuItem::label("Recent files").a11y_label(),
            Some("Recent files".into())
        );
        assert_eq!(
            PopupMenuItem::submenu("More", submenu).a11y_label(),
            Some("More".into())
        );
        assert_eq!(PopupMenuItem::separator().a11y_label(), None);
        assert_eq!(PopupMenuItem::element(|_, _| div()).a11y_label(), None);
        assert_eq!(
            PopupMenuItem::new("More context (3 lines)")
                .accessibility_label("More context, 3 lines shown")
                .a11y_label(),
            Some("More context, 3 lines shown".into())
        );
    }
}
