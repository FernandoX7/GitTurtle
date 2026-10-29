//! Native accessibility semantics, keyboard navigation, and local OS choices.
//! No setting is changed here: macOS display preferences are read on startup
//! and on workspace display-change notifications, then applied to the toolkit's
//! motion policy. Foreground activation remains a fallback read.
use crate::*;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};

#[cfg(any(target_os = "macos", test))]
mod display_changes;

pub(super) fn observe_display_preferences(cx: &mut Context<GitTurtle>) -> Option<Task<()>> {
    #[cfg(target_os = "macos")]
    {
        Some(display_changes::subscribe(cx))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = cx;
        None
    }
}

gpui_kit::actions!(
    gitturtle,
    [
        NextNavigation,
        PreviousNavigation,
        FirstNavigation,
        LastNavigation,
        ExpandNavigation,
        CollapseNavigation,
        ActivateNavigation,
        ManageNavigation,
        RowActions
    ]
);

pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", NextNavigation, Some("GitTurtleNavigation")),
        KeyBinding::new("up", PreviousNavigation, Some("GitTurtleNavigation")),
        KeyBinding::new("home", FirstNavigation, Some("GitTurtleNavigation")),
        KeyBinding::new("end", LastNavigation, Some("GitTurtleNavigation")),
        KeyBinding::new("right", ExpandNavigation, Some("GitTurtleNavigation")),
        KeyBinding::new("left", CollapseNavigation, Some("GitTurtleNavigation")),
        KeyBinding::new("enter", ActivateNavigation, Some("GitTurtleNavigation")),
        KeyBinding::new("space", ActivateNavigation, Some("GitTurtleNavigation")),
        // A tree's or a list's cursor row opens its menu from the keyboard,
        // as a right-click opens it: the navigator's branch or worktree, the
        // project list's row, History's commit, and a file list's row, whose
        // menu takes a Stage or Unstage control a narrow row has no room
        // for. Linux reports the Menu key as `menu`; macOS keyboards have
        // none.
        KeyBinding::new("shift-f10", ManageNavigation, Some("GitTurtleNavigation")),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("menu", ManageNavigation, Some("GitTurtleNavigation")),
        KeyBinding::new("shift-f10", RowActions, Some("GitTurtleList")),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("menu", RowActions, Some("GitTurtleList")),
    ]);
}

/// The row a menu opens beside: a keyboard's cursor row, within the list's
/// visible part ([`KeyboardMenu::cursor_row`]), or the row a pointer pressed
/// ([`RowAnchor::pointer`]). It holds where the menu's leading edge starts,
/// the row's top edge, above which a menu that does not fit below ends, and
/// where the menu's top goes when it opens below: the row's foot, or the
/// pointer. A menu can end at the
/// list's trailing inset instead ([`RowAnchor::trailing`]).
#[derive(Clone, Copy, Debug)]
pub(super) struct RowAnchor {
    /// Where the menu's leading edge starts.
    pub(super) x: Pixels,
    top: Pixels,
    /// Where the menu's top goes when it opens below the row.
    below: Pixels,
    /// Where the menu's trailing edge ends, in place of starting at `x`.
    trailing: Option<Pixels>,
}

impl RowAnchor {
    /// The row painted at `row` that a pointer pressed at `pointer`: its menu
    /// starts at the pointer, below it, or lies wholly above the row.
    pub(super) fn pointer(pointer: Point<Pixels>, row: Bounds<Pixels>) -> Self {
        Self {
            x: pointer.x,
            top: row.top(),
            below: pointer.y.clamp(row.top(), row.bottom()),
            trailing: None,
        }
    }

    /// The tallest the menu's items may be with its fill inside `room`: the
    /// room below where that holds about four rows ([`MENU_FLOOR`]), and
    /// otherwise the larger of the room below and the room above the row,
    /// where it goes when the room below is too short for it ([`RowMenu`]).
    fn cap(&self, room: Bounds<Pixels>) -> Pixels {
        let below = room.bottom() - self.below;
        let above = self.top - MENU_EDGE - room.top();
        if below >= appearance::ui_size(MENU_FLOOR) {
            below
        } else {
            below.max(above)
        }
        .max(Pixels::ZERO)
    }

    /// The menu ends at `right`, the list's trailing inset, rather than
    /// starting at the row's leading inset: History's commit menu, which
    /// would otherwise lie over the graph's lanes.
    pub(super) fn trailing(self, right: Pixels) -> Self {
        Self {
            trailing: Some(right),
            ..self
        }
    }
}

/// A row's menu opened from the keyboard beside the row it acts on, with
/// Shift-F10 or the Menu key: the navigator's and the project pane's cursor
/// row ([`ManageNavigation`]), and History's commit list's and a file
/// list's ([`RowActions`]). A right-click menu belongs to
/// its row; this one is placed from the cursor row's geometry and takes the
/// keys until it closes. It opens with the keyboard's cursor on its first
/// action, the kit's menu keys passing over entries that cannot act, below
/// its row, or wholly above it where the room below is too short
/// ([`RowMenu`]), with its edge [`MENU_MARGIN`] above the status bar either
/// way; Escape returns focus to the list or tree that opened it, its action
/// context. Focus that leaves it otherwise, to a dialog one of its items
/// opens or with the page that drew it, closes it as well, so it never
/// reappears over another repository tab or mode.
pub(super) struct KeyboardMenu {
    menu: Entity<PopupMenu>,
    row: RowAnchor,
    /// The window's size when the menu opened, which its overlay covers. The
    /// menu also closes on a press outside itself, past an overlay that a
    /// larger window has outgrown.
    viewport: Size<Pixels>,
    /// Where the menu's fill may lie when it opened ([`menu_bounds`]).
    room: Bounds<Pixels>,
    _subscriptions: [Subscription; 2],
}

impl KeyboardMenu {
    /// Builds the row's menu with `build`, the builder its right-click menu
    /// uses, focuses it, and calls `dismissed` once it closes. Its items are
    /// no taller than the room below `row` where that holds about four rows
    /// ([`MENU_FLOOR`]), and otherwise no taller than the larger of the room
    /// below and the room above, where it goes when the room below is too
    /// short for it.
    pub(super) fn open(
        row: RowAnchor,
        focus: FocusHandle,
        build: impl FnOnce(PopupMenu) -> PopupMenu + 'static,
        dismissed: fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>),
        window: &mut Window,
        cx: &mut Context<GitTurtle>,
    ) -> Self {
        let room = menu_bounds(window);
        let cap = row.cap(room);
        let menu = PopupMenu::build(window, cx, move |menu, window, cx| {
            open_menu(
                build(menu).action_context(focus),
                true,
                None,
                Some(cap),
                window,
                cx,
            )
        });
        let dismiss = cx.subscribe_in(
            &menu,
            window,
            move |this, _, _: &DismissEvent, window, cx| dismissed(this, window, cx),
        );
        let handle = menu.focus_handle(cx);
        let blur = cx.on_blur(&handle, window, move |this, window, cx| {
            dismissed(this, window, cx)
        });
        window.focus(&handle, cx);
        Self {
            menu,
            row,
            viewport: window.viewport_size(),
            room,
            _subscriptions: [dismiss, blur],
        }
    }

    /// The `row`th row of a uniform list of `height` tall rows, its menu
    /// starting `inset` from the list's leading edge: the list's origin, its
    /// scroll offset and the row's index place it, within the list's visible
    /// part.
    pub(super) fn cursor_row(
        scroll: &UniformListScrollHandle,
        row: usize,
        height: Pixels,
        inset: Pixels,
    ) -> RowAnchor {
        let state = scroll.0.borrow();
        let bounds = state.base_handle.bounds();
        let top = bounds.top() + state.base_handle.offset().y + height * row as f32;
        let visible = |y: Pixels| y.clamp(bounds.top(), bounds.bottom());
        RowAnchor {
            x: bounds.left() + inset,
            top: visible(top),
            below: visible(top + height),
            trailing: None,
        }
    }

    /// Whether focus is in the menu, as it is until an item, Escape or a
    /// press outside it moves focus.
    pub(super) fn has_focus(&self, window: &Window, cx: &App) -> bool {
        self.menu.focus_handle(cx).contains_focused(window, cx)
    }

    /// The menu over a window-wide overlay, `id`, that a press outside the
    /// menu dismisses through `dismiss`. The overlay covers the status bar,
    /// so a press there closes the menu too.
    pub(super) fn render(
        &self,
        id: &'static str,
        dismiss: fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>),
        cx: &mut Context<GitTurtle>,
    ) -> AnyElement {
        deferred(
            anchored().position(Point::default()).child(
                div()
                    .id(id)
                    .debug_selector(move || id.into())
                    .occlude()
                    .w(self.viewport.width)
                    .h(self.viewport.height)
                    .on_any_mouse_down(cx.listener(move |this, _, window, cx| {
                        dismiss(this, window, cx);
                    }))
                    .child(RowMenu {
                        row: self.row,
                        room: self.room,
                        // A press inside the menu must reach its item, not
                        // the overlay behind it.
                        menu: div()
                            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                            .child(self.menu.clone())
                            .into_any_element(),
                    }),
            ),
        )
        .with_priority(gpui_kit::base::POPUP_PRIORITY)
        .into_any_element()
    }
}

/// A row's menu, placed beside its row once its size is known: below the
/// row, or a pointer's menu below the pointer, where it fits in `room`
/// there, and otherwise wholly above the row, its edge clear of it, at the
/// row's leading inset or the pointer, or ending at its trailing inset, and
/// inside `room` either way. GPUI's anchor, which placed a keyboard's menu
/// before, slid a menu that ran past the window up just far enough, over its
/// own row (5.5 px at 18 pt in a 461 × 490 window), and moved it by whole
/// pixels, which can put half a pixel past the room; the kit's context menu
/// did the same to a pointer's.
struct RowMenu {
    row: RowAnchor,
    room: Bounds<Pixels>,
    menu: AnyElement,
}

impl RowMenu {
    /// Where the menu's fill, `size`, starts.
    fn origin(&self, size: Size<Pixels>) -> Point<Pixels> {
        let room = self.room;
        let y = if self.row.below + size.height <= room.bottom() {
            self.row.below
        } else {
            (self.row.top - MENU_EDGE - size.height).max(room.top())
        };
        let x = self
            .row
            .trailing
            .map_or(self.row.x, |right| right - size.width);
        let x = x.min(room.right() - size.width).max(room.left());
        point(x, y)
    }
}

impl IntoElement for RowMenu {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for RowMenu {
    /// The menu's layout.
    type RequestLayoutState = LayoutId;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, LayoutId) {
        let menu = self.menu.request_layout(window, cx);
        // Laid out as GPUI's anchor lays out its child, out of the overlay's
        // flow.
        let style = Style {
            position: Position::Absolute,
            display: Display::Flex,
            ..Style::default()
        };
        (window.request_layout(style, [menu], cx), menu)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        menu: &mut LayoutId,
        window: &mut Window,
        cx: &mut App,
    ) {
        let laid = window.layout_bounds(*menu);
        let offset = self.origin(laid.size) - laid.origin;
        window.with_element_offset(offset, |window| self.menu.prepaint(window, cx));
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut LayoutId,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.menu.paint(window, cx);
    }
}

/// How far a menu stays above the status bar's top edge: the 8 px the kit's
/// popup hosts keep from the window's edges. A keyboard's menu keeps it from
/// its drawn edge, [`MENU_EDGE`] outside its fill ([`menu_bounds`]).
const MENU_MARGIN: Pixels = px(8.);
/// The band outside a menu's fill where the kit draws its edge, a ring in
/// the popup's shadow. A menu whose fill ended [`MENU_MARGIN`] above the
/// status bar showed its edge 7 px above it.
const MENU_EDGE: Pixels = px(1.);
/// The least height a menu's items take, at the default interface size,
/// where its top lies too near the status bar for more: about four rows. A
/// keyboard's menu there moves above its row instead ([`RowMenu`]).
const MENU_FLOOR: f32 = 120.;

/// Where a keyboard's menu's fill may lie in `window`, its edge
/// [`MENU_MARGIN`] inside the window's top edge and above the status bar,
/// and its fill [`MENU_MARGIN`] inside the window's sides, as the kit's
/// popup hosts keep it, all inside the client inset.
fn menu_bounds(window: &Window) -> Bounds<Pixels> {
    let inset = window.client_inset().unwrap_or_default();
    let viewport = window.viewport_size();
    let edge = MENU_MARGIN + MENU_EDGE;
    Bounds::from_corners(
        point(inset + MENU_MARGIN, inset + edge),
        point(
            viewport.width - inset - MENU_MARGIN,
            crate::theme_editor::status_bar_top(window) - edge,
        ),
    )
}

/// The tallest a menu's items may be when its top edge is at `top`: down to
/// [`MENU_MARGIN`] above the status bar, so that it never covers the bar,
/// and no less than [`MENU_FLOOR`].
pub(crate) fn menu_room(top: Pixels, window: &Window) -> Pixels {
    (menu_foot(window) - top).max(appearance::ui_size(MENU_FLOOR))
}

/// Where a Button's dropdown opens and how tall its items may be there, so
/// that it never covers its Button, which a second click must reach.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DropdownRoom {
    /// Above the Button rather than below it.
    above: bool,
    /// The tallest its items may be on that side.
    room: Pixels,
}

impl DropdownRoom {
    /// For a Button painted at `trigger`: below it, down to [`menu_foot`],
    /// where that room holds the least rows ([`MENU_FLOOR`]); otherwise
    /// above it, up to the window's top margin, where that room holds them;
    /// otherwise on the roomier side, with the whole rows that side holds.
    /// The kit's popover keeps a quarter rem from the Button on either side.
    fn of(trigger: Bounds<Pixels>, window: &Window) -> Self {
        let gap = window.rem_size() * 0.25;
        let floor = appearance::ui_size(MENU_FLOOR);
        let below = menu_foot(window) - trigger.bottom() - gap;
        let above = trigger.top() - gap - menu_bounds(window).top();
        if below >= floor || below >= above {
            Self {
                above: false,
                room: below.max(px(0.)),
            }
        } else {
            Self {
                above: true,
                room: above,
            }
        }
    }
}

/// The lowest a popup's fill reaches in `window`: its drawn edge, the ring
/// the kit paints in the band outside the fill
/// ([`gpui_kit::component::POPOVER_EDGE_WIDTH`]), [`MENU_MARGIN`] above the
/// status bar. A menu whose fill ended 8 px above the bar drew its edge 7 px
/// above it.
pub(crate) fn menu_foot(window: &Window) -> Pixels {
    crate::theme_editor::status_bar_top(window)
        - MENU_MARGIN
        - gpui_kit::component::POPOVER_EDGE_WIDTH
}

/// Keeps every kit popup in the app, a dropdown or a right-click menu, above
/// [`menu_foot`]: the kit's window-bounded placement moves one that would
/// run lower up, as it moves one that would leave the window, so a menu
/// opened at a low row or taller than the room below its button never
/// covers the status bar or comes nearer it than its margin.
pub(crate) fn keep_popups_above_the_status_bar(cx: &mut App) {
    gpui_kit::base::set_popup_bottom_inset(cx, |window| {
        window.viewport_size().height - menu_foot(window)
    });
}

/// `menu` as every menu in the app opens. Opened from the keyboard (the
/// Menu key, Shift-F10, F10, or Enter or Space on a menu's button), its
/// first item that can act carries the keyboard's cursor, so Enter acts on
/// it and Down moves on from it, as native menus do; opened by a pointer, it
/// highlights nothing until the pointer or a key moves. With its top edge
/// at `top`, where the opener knows it, its items are no taller than the
/// room above the status bar, nor than `max` where the menu has a cap of its
/// own, and scroll past that.
pub(crate) fn open_menu(
    menu: PopupMenu,
    keyboard: bool,
    top: Option<Pixels>,
    max: Option<Pixels>,
    window: &Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let room = top.map(|top| menu_room(top, window));
    open_menu_in(menu, keyboard, room, max, cx)
}

/// [`open_menu`] for a menu whose items may be `room` tall where it opens.
fn open_menu_in(
    mut menu: PopupMenu,
    keyboard: bool,
    room: Option<Pixels>,
    max: Option<Pixels>,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    if keyboard {
        menu.select_first_item(cx);
    }
    if let Some(cap) = match (room, max) {
        (Some(room), Some(max)) => Some(room.min(max)),
        (room, max) => room.or(max),
    } {
        menu = menu.max_h(cap).scrollable(true);
    }
    menu
}

/// A menu item that reads as its command, `label`, with the condition it
/// depends on, `detail`, apart from it: in quieter secondary text at the
/// row's trailing edge, where a shortcut stands. An item unavailable for
/// that condition, disabled, states its reason this way, the command dim and
/// the reason legible, as the command palette states an unavailable
/// command's. Assistive technology hears both, the command first. A test
/// build names the command's text and the detail's text and colour in debug
/// selectors (`menu-command: <label>`, `menu-detail: <detail> · <colour>`),
/// as the scene does not expose glyphs.
pub(crate) fn detailed_item(label: &'static str, detail: &'static str) -> PopupMenuItem {
    PopupMenuItem::element(move |_, cx| {
        let color = gpui_kit::component::Theme::global(cx).muted_foreground;
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .debug_selector(move || format!("menu-command: {label}"))
                    .child(label),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(color)
                    .debug_selector(move || {
                        format!("menu-detail: {detail} · {:?}", gpui::Rgba::from(color))
                    })
                    .child(detail),
            )
    })
    .accessibility_label(format!("{label}, {}", detail.to_lowercase()))
}

/// A menu's builder, run each time its menu opens.
type BuildMenu = dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu;
/// What a dropdown calls with its menu's new open state.
type OpenChange = dyn Fn(&bool, &mut Window, &mut App);

/// A Button and the menu it opens below itself ([`AppMenu`]), which
/// [`open_menu`] opens: the Button's bounds in the last frame place the
/// menu's top.
#[derive(IntoElement)]
pub(crate) struct AppDropdown {
    trigger: Button,
    max: Option<Pixels>,
    build: std::rc::Rc<BuildMenu>,
    on_open_change: Option<std::rc::Rc<OpenChange>>,
    /// The app whose writes the menu's items follow ([`Self::follows_writes`]).
    writes: Option<WeakEntity<crate::GitTurtle>>,
}

/// What a menu that follows writes reads as it is built
/// ([`AppDropdown::follows_writes`], [`PointerMenu::follows_writes`]):
/// whether a write runs, and whether a merge, rebase, cherry-pick or revert
/// is paused, which disables a commit's Revert, Cherry-pick, Amend and Undo
/// (`GitTurtle::commit_menu_state`) and the Changes Actions menu's Save
/// changes to stash…, Amend and Undo. The menu is built again when either
/// changes; built again only for the write, a menu open as a cherry-pick
/// stopped on conflicts offered those items once the write had ended, until
/// it was opened again.
#[derive(Clone, Copy, PartialEq, Eq)]
struct MenuInputs {
    writing: bool,
    paused: bool,
}

impl MenuInputs {
    fn of(app: &crate::GitTurtle) -> Self {
        Self {
            writing: app.operation_busy.is_some(),
            paused: app.integration_state.is_some(),
        }
    }
}

impl AppDropdown {
    /// Builds the open menu again, in place, when a write in `app` starts or
    /// ends, or an integration pauses or ends ([`MenuInputs`]): a menu
    /// opened while a write ran, its items the write refuses disabled, kept
    /// them disabled after a quick write had ended.
    pub(crate) fn follows_writes(mut self, app: WeakEntity<crate::GitTurtle>) -> Self {
        self.writes = Some(app);
        self
    }

    /// Calls `callback` with the menu's new open state when it opens or
    /// closes, as the kit's dropdown does.
    pub(crate) fn on_open_change(
        mut self,
        callback: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_open_change = Some(std::rc::Rc::new(callback));
        self
    }
}

/// A Button's dropdown menu as the app's menus open ([`open_menu`]), in
/// place of the kit's `dropdown_menu`.
pub(crate) trait AppMenu {
    /// The menu `build` makes, each time the Button opens it.
    fn app_menu(
        self,
        build: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> AppDropdown;

    /// [`AppMenu::app_menu`] for a menu that also caps its own height at
    /// `max`.
    fn app_menu_within(
        self,
        max: Pixels,
        build: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> AppDropdown;
}

impl AppMenu for Button {
    fn app_menu(
        self,
        build: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> AppDropdown {
        AppDropdown {
            trigger: self,
            max: None,
            build: std::rc::Rc::new(build),
            on_open_change: None,
            writes: None,
        }
    }

    fn app_menu_within(
        self,
        max: Pixels,
        build: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> AppDropdown {
        AppDropdown {
            max: Some(max),
            ..self.app_menu(build)
        }
    }
}

impl RenderOnce for AppDropdown {
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        use gpui_kit::{base::ElementExt as _, component::menu::DropdownMenu as _};
        // The trigger's bounds as the last frame painted them: the menu is
        // built in the frame after its Button was pressed, before that
        // frame's own layout.
        let key = format!(
            "app-menu-anchor:{:?}",
            self.trigger.interactivity().element_id
        );
        let anchor = window
            .use_keyed_state(SharedString::from(key), cx, |_, _| {
                std::rc::Rc::new(std::cell::Cell::new(None::<Bounds<Pixels>>))
            })
            .read(cx)
            .clone();
        let painted = anchor.clone();
        let (build, max, on_open_change) = (self.build, self.max, self.on_open_change);
        let writes = self.writes;
        // The kit's popup host only moves a dropdown into the window, so one
        // taller than the room below its Button moved up over it; the side
        // is chosen here instead, from the same painted bounds as the room.
        let corner = match anchor
            .get()
            .map(|trigger| DropdownRoom::of(trigger, window))
        {
            Some(DropdownRoom { above: true, .. }) => gpui::Anchor::BottomLeft,
            _ => gpui::Anchor::TopLeft,
        };
        // The menu as it opens, or as a write's start or end builds it again,
        // its rows capped at the room of its side.
        let bounds = anchor.clone();
        let open = std::rc::Rc::new(
            move |menu: PopupMenu,
                  keyboard: bool,
                  window: &mut Window,
                  cx: &mut Context<PopupMenu>| {
                let menu = build(menu, window, cx);
                let room = bounds
                    .get()
                    .map(|trigger| DropdownRoom::of(trigger, window).room);
                open_menu_in(menu, keyboard, room, max, cx)
            },
        );
        let dropdown = self
            .trigger
            .on_prepaint(move |bounds, _, _| painted.set(Some(bounds)))
            .dropdown_menu_with_anchor(corner, move |menu, window, cx| {
                // A resized window, as a tiling compositor re-tiles it,
                // closes the menu: its rows were capped for the side the
                // size it opened in gave it, and a larger or smaller window
                // could turn it to the other side, over its Button.
                let viewport = window.viewport_size();
                cx.observe_window_bounds(window, move |_, window, cx| {
                    if window.viewport_size() != viewport {
                        cx.emit(gpui::DismissEvent);
                    }
                })
                .detach();
                let keyboard = window.last_input_was_keyboard();
                let menu = open(menu, keyboard, window, cx);
                if let Some(app) = writes.as_ref().and_then(WeakEntity::upgrade) {
                    let mut inputs = MenuInputs::of(app.read(cx));
                    let open = open.clone();
                    cx.observe_in(&app, window, move |menu, app, window, cx| {
                        let now = MenuInputs::of(app.read(cx));
                        if now != inputs {
                            inputs = now;
                            menu.rebuild(window, cx, |menu, window, cx| {
                                open(menu, keyboard, window, cx)
                            });
                        }
                    })
                    .detach();
                }
                menu
            });
        match on_open_change {
            Some(callback) => dropdown
                .on_open_change(move |open, window, cx| callback(open, window, cx))
                .into_any_element(),
            None => dropdown.into_any_element(),
        }
    }
}

/// An element's right-click menu as the app's menus open ([`open_menu`]),
/// in place of the kit's `context_menu`: its top is the pointer's place.
pub(crate) trait AppContextMenu:
    InteractiveElement + ParentElement + IntoElement + Sized + 'static
{
    /// The menu `build` makes, each time a right-click on the row opens it.
    #[track_caller]
    fn app_context_menu(
        mut self,
        build: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> PointerMenu<Self> {
        // Stable across frames, so the open menu is kept from one to the
        // next.
        let caller = std::panic::Location::caller();
        let id = self
            .interactivity()
            .element_id
            .clone()
            .map(|id| ElementId::Name(format!("app-context-menu-{id:?}").into()))
            .unwrap_or_else(|| ElementId::CodeLocation(*caller));
        PointerMenu {
            id,
            element: Some(self),
            build: std::rc::Rc::new(build),
            writes: None,
        }
    }
}

impl<E: InteractiveElement + ParentElement + IntoElement + 'static> AppContextMenu for E {}

/// A row and the menu a right-click on it opens at the pointer
/// ([`AppContextMenu`]). The kit's context menu placed the menu's corner at
/// the pointer and only moved it into the window, which the status bar's
/// inset shortens, so on a row near the bar it slid up over the row it was
/// opened on. This one places it as a keyboard's row menu is placed
/// ([`RowMenu`]): at the pointer, below it, where the room down to the bar
/// holds about four rows or the whole menu, its items capped at that room;
/// otherwise wholly above the row, its edge clear of it, capped at the
/// larger room. A resized window closes it, as it closes a button's menu,
/// since the row it was placed beside may have moved.
pub(crate) struct PointerMenu<E> {
    id: ElementId,
    element: Option<E>,
    build: std::rc::Rc<BuildMenu>,
    /// The app whose writes the menu's items follow ([`Self::follows_writes`]).
    writes: Option<WeakEntity<crate::GitTurtle>>,
}

impl<E> PointerMenu<E> {
    /// Builds the open menu again, in place, when a write in `app` starts or
    /// ends, or an integration pauses or ends ([`MenuInputs`]), as
    /// [`AppDropdown::follows_writes`] does a button's: the menu read them as
    /// it opened and kept its items so.
    pub(crate) fn follows_writes(mut self, app: WeakEntity<crate::GitTurtle>) -> Self {
        self.writes = Some(app);
        self
    }

    fn shared(id: &GlobalElementId, window: &mut Window) -> SharedPointerMenu {
        window.with_optional_element_state::<SharedPointerMenu, _>(Some(id), |state, _| {
            let state = state.flatten().unwrap_or_default();
            (state.clone(), Some(state))
        })
    }
}

/// The menu a [`PointerMenu`] opened, kept from frame to frame.
#[derive(Default)]
struct PointerMenuState {
    menu: Option<Entity<PopupMenu>>,
    open: bool,
    /// The row and the pointer it opened at.
    anchor: Option<RowAnchor>,
    /// Holds this state weakly: a strong hold was a cycle through the app's
    /// subscriber set that kept the menu after its row was gone.
    _dismiss: Option<Subscription>,
}

type SharedPointerMenu = std::rc::Rc<std::cell::RefCell<PointerMenuState>>;

/// The right-click menus open in the app's windows, with the focus each
/// returns to as it closes ([`PointerMenu`]). A right-click on a row while
/// another row's menu is open closes that menu as the press starts, but
/// focus leaves it only once the press has been handled, as its dismissal
/// arrives; the new menu read the focus it returns to meanwhile and took the
/// closed menu's, which no frame draws, so the list's keys stopped reaching
/// it. It takes the closed menu's own instead, as the kit's context menu
/// takes an earlier menu's.
#[derive(Default)]
struct OpenPointerMenus(Vec<(WeakEntity<PopupMenu>, Option<FocusHandle>)>);

impl Global for OpenPointerMenus {}

impl OpenPointerMenus {
    /// Where a menu opening now returns focus: where an open right-click
    /// menu that holds focus returns it, or else the focus itself.
    fn restore_target(window: &Window, cx: &App) -> Option<FocusHandle> {
        let holder = cx.try_global::<Self>().and_then(|open| {
            open.0.iter().find(|(menu, _)| {
                menu.upgrade()
                    .is_some_and(|menu| menu.focus_handle(cx).contains_focused(window, cx))
            })
        });
        match holder {
            Some((_, previous)) => previous.clone(),
            None => window.focused(cx),
        }
    }

    /// Records `menu`, open, returning focus to `previous`; a menu that is
    /// gone is forgotten.
    fn open(menu: &Entity<PopupMenu>, previous: Option<FocusHandle>, cx: &mut App) {
        let open = &mut cx.default_global::<Self>().0;
        open.retain(|(menu, _)| menu.upgrade().is_some());
        open.push((menu.downgrade(), previous));
    }

    /// Forgets `menu`, closed or released.
    fn close(menu: EntityId, cx: &mut App) {
        cx.default_global::<Self>()
            .0
            .retain(|(open, _)| open.entity_id() != menu);
    }
}

impl<E: ParentElement + IntoElement + 'static> IntoElement for PointerMenu<E> {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl<E: ParentElement + IntoElement + 'static> Element for PointerMenu<E> {
    type RequestLayoutState = AnyElement;
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, AnyElement) {
        let shared = Self::shared(id.expect("the menu's id"), window);
        let shown = {
            let state = shared.borrow();
            state
                .menu
                .clone()
                .zip(state.anchor)
                .filter(|(menu, _)| state.open && !menu.read(cx).is_empty())
        };
        let overlay = shown.map(|(menu, row)| {
            // Focused, so that its keys reach it.
            if !menu.focus_handle(cx).contains_focused(window, cx) {
                menu.focus_handle(cx).focus(window, cx);
            }
            let viewport = window.viewport_size();
            deferred(
                anchored().position(Point::default()).child(
                    div()
                        .w(viewport.width)
                        .h(viewport.height)
                        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                        .child(RowMenu {
                            row,
                            room: menu_bounds(window),
                            menu: menu.into_any_element(),
                        }),
                ),
            )
            .with_priority(gpui_kit::base::POPUP_PRIORITY)
            .into_any_element()
        });
        let mut element = self
            .element
            .take()
            .expect("the row")
            .children(overlay)
            .into_any_element();
        (element.request_layout(window, cx), element)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        element: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) -> Hitbox {
        element.prepaint(window, cx);
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut AnyElement,
        hitbox: &mut Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.paint(window, cx);
        let shared = Self::shared(id.expect("the menu's id"), window);
        let (build, writes, hitbox) = (self.build.clone(), self.writes.clone(), hitbox.clone());
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if !phase.bubble() || event.button != MouseButton::Right || !hitbox.is_hovered(window) {
                return;
            }
            // Focus returns where it was before the menu opened, or, while
            // an earlier right-click menu holds it, this row's or another's,
            // where that one returns it ([`OpenPointerMenus`]).
            let previous = OpenPointerMenus::restore_target(window, cx);
            let row = RowAnchor::pointer(event.position, hitbox.bounds);
            {
                let mut state = shared.borrow_mut();
                // Its dismissal, which no longer reaches this state, would
                // have forgotten it.
                if let Some(menu) = state.menu.take() {
                    OpenPointerMenus::close(menu.entity_id(), cx);
                }
                state._dismiss = None;
                state.open = true;
                state.anchor = Some(row);
            }
            // Built after this event, as the kit builds its own.
            let (shared, build, writes) = (shared.clone(), build.clone(), writes.clone());
            window.defer(cx, move |window, cx| {
                let cap = row.cap(menu_bounds(window));
                let open = std::rc::Rc::new(
                    move |menu: PopupMenu, window: &mut Window, cx: &mut Context<PopupMenu>| {
                        let menu = build(menu, window, cx);
                        // No action context: an action item dispatches from
                        // the menu, drawn inside the row, and bubbles through
                        // the row's ancestors, as the kit's context menu has
                        // it; focus returns on dismissal below.
                        open_menu_in(menu, false, Some(cap), None, cx)
                    },
                );
                let menu = PopupMenu::build(window, cx, move |menu, window, cx| {
                    let viewport = window.viewport_size();
                    cx.observe_window_bounds(window, move |_, window, cx| {
                        if window.viewport_size() != viewport {
                            cx.emit(gpui::DismissEvent);
                        }
                    })
                    .detach();
                    if let Some(app) = writes.as_ref().and_then(WeakEntity::upgrade) {
                        let mut inputs = MenuInputs::of(app.read(cx));
                        let open = open.clone();
                        cx.observe_in(&app, window, move |menu, app, window, cx| {
                            let now = MenuInputs::of(app.read(cx));
                            if now != inputs {
                                inputs = now;
                                menu.rebuild(window, cx, |menu, window, cx| open(menu, window, cx));
                            }
                        })
                        .detach();
                    }
                    open(menu, window, cx)
                });
                OpenPointerMenus::open(&menu, previous.clone(), cx);
                // A row that stops being drawn while its menu is open, as a
                // quiet refresh takes a deleted file's row away, drops this
                // state and the menu with it, and no dismissal comes: focus
                // the menu holds as it goes returns where it was, as a
                // dismissal returns it below. It stayed on the menu's
                // released handle, where nothing was focused, so the list's
                // keys stopped reaching it.
                window
                    .observe_release(&menu, cx, {
                        let (previous, released) = (previous.clone(), menu.entity_id());
                        move |menu, window, cx| {
                            OpenPointerMenus::close(released, cx);
                            if let Some(previous) = &previous
                                && (menu.focus_handle(cx).contains_focused(window, cx)
                                    || window.focused(cx).is_none())
                            {
                                window.focus(previous, cx);
                            }
                        }
                    })
                    .detach();
                // However it closes, by Escape, an item, a press outside or a
                // resize, focus the menu still holds returns where it was
                // before it opened: a resize's dismissal, which skips the
                // menu's own Cancel, left focus on the menu no frame draws,
                // so the list's keys stopped reaching it. So does focus that
                // a press outside gave to what holds that place, the
                // window's root, which a press on the status bar or a
                // heading focuses: nothing showed focus there and the list's
                // keys reached nothing, where a press outside a keyboard's
                // menu returns focus to its list. An item that moved focus
                // on, to a dialog it opened, or a press on another list or
                // control, keeps it there.
                let dismiss = window.subscribe(&menu, cx, {
                    let shared = std::rc::Rc::downgrade(&shared);
                    move |menu, _: &gpui::DismissEvent, window, cx| {
                        let handle = menu.focus_handle(cx);
                        let closed = menu.entity_id();
                        OpenPointerMenus::close(closed, cx);
                        if let Some(previous) = &previous
                            && (handle.contains_focused(window, cx)
                                || window
                                    .focused(cx)
                                    .is_some_and(|focused| focused.contains(previous, window)))
                        {
                            window.focus(previous, cx);
                        }
                        if let Some(shared) = shared.upgrade() {
                            shared.borrow_mut().open = false;
                        }
                        window.refresh();
                    }
                });
                let mut state = shared.borrow_mut();
                state.menu = Some(menu);
                state._dismiss = Some(dismiss);
                window.refresh();
            });
        });
    }
}

/// An element with an ID whose node is a live region: an assistive
/// technology announces what it says when it changes, at once for a failure
/// or a refusal ([`assertive`](LiveRegion::assertive)), after what it is
/// saying for a status ([`polite`](LiveRegion::polite)), and not at all
/// where continuous progress would repeat itself (`Live::Off`). A live
/// region is read as a whole, so a changed word repeats its line rather than
/// the word alone.
///
/// GPUI writes these properties through the node's synthetic children,
/// which it builds only while an assistive technology is active, and never
/// in a test window; tests read what the element asked for from
/// `written::announced` instead, keyed by its element ID as it was built.
pub(crate) trait LiveRegion: StatefulInteractiveElement + Element {
    /// Announced at once, interrupting: a failure, a refusal or invalid
    /// input.
    fn assertive(self) -> Self {
        self.live_region(gpui::accesskit::Live::Assertive, false)
    }

    /// Announced once the assistive technology has finished what it is
    /// saying: a status, a count or a completion.
    fn polite(self) -> Self {
        self.live_region(gpui::accesskit::Live::Polite, false)
    }

    /// Announced as `live` says, read as a whole unless it is off, and
    /// `busy` while what it reports is still under way.
    fn live_region(self, live: gpui::accesskit::Live, busy: bool) -> Self {
        let atomic = live != gpui::accesskit::Live::Off;
        #[cfg(test)]
        written::live(&self, written::Announced { live, atomic, busy });
        self.a11y_synthetic_children(move |builder| {
            let node = builder.parent_node();
            node.set_live(live);
            if atomic {
                node.set_live_atomic();
            }
            if busy {
                node.set_busy();
            }
        })
    }
}

impl<E: StatefulInteractiveElement + Element> LiveRegion for E {}

/// `element`, a container that scrolls or clips what it holds, as the
/// accessibility node AccessKit expects of one: it clips its children, and its
/// bounds are the part of it that its ancestors leave visible.
///
/// GPUI gives each node its layout bounds and never says that a node clips,
/// so a control scrolled out of a list, a Reflog entry below the dialog's
/// edge or a composer field above its viewport stayed in the tree, `visible`
/// and `showing` on AT-SPI, where a screen reader could find and activate a
/// control the window does not draw. AccessKit's filter leaves out each child
/// of a clipping node that lies wholly outside that node's bounds, keeping
/// the first such child past each edge, so that an assistive technology can
/// move into it, and any focused node, wherever it lies. The filter compares
/// a child with its nearest clipping parent only, so a viewport inside
/// another viewport reports the part of itself that the outer one shows, and
/// each child is measured against what is drawn. A container without a role
/// of its own becomes a scroll view; one with a role, such as a list box,
/// keeps it. Children keep their own bounds, as AccessKit and the platforms
/// report a partly visible control whole; a row wider than its list, such as
/// History's commit rows, is wrapped itself, so it reports the part the list
/// draws. The wrapper styles, holds and handles input as the element it
/// wraps. Focus and the Tab order do not change: the wrapper adds no
/// dispatch node, focus handle or listener.
pub(crate) fn clips_children<E: IntoElement>(element: E) -> ClipsChildren<E::Element> {
    ClipsChildren {
        inner: element.into_element(),
        visible: None,
    }
}

/// The element [`clips_children`] returns.
pub(crate) struct ClipsChildren<E> {
    inner: E,
    /// The part of the element that its ancestors leave visible, in the
    /// physical window pixels of AccessKit's bounds, measured as it
    /// prepaints. GPUI writes the node before it prepaints the element, so
    /// the node takes these bounds afterwards (`a11y_synthetic_children`).
    visible: Option<gpui::accesskit::Rect>,
}

impl<E: Element> IntoElement for ClipsChildren<E> {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

// The wrapper styles, holds and handles input as the element it wraps, so a
// row keeps its click and context menu (`ContextMenuExt` takes an element
// that is all three).
impl<E: InteractiveElement> InteractiveElement for ClipsChildren<E> {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.inner.interactivity()
    }
}

impl<E: StatefulInteractiveElement> StatefulInteractiveElement for ClipsChildren<E> {}

impl<E: ParentElement> ParentElement for ClipsChildren<E> {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.inner.extend(elements);
    }
}

impl<E: Styled> Styled for ClipsChildren<E> {
    fn style(&mut self) -> &mut StyleRefinement {
        self.inner.style()
    }
}

impl<E: Element> Element for ClipsChildren<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.inner.id()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.inner.source_location()
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.inner.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let visible = drawn_part(bounds, window);
        self.visible = Some(visible);
        #[cfg(test)]
        let written = written::open(self, Some(visible));
        let state = self
            .inner
            .prepaint(id, inspector, bounds, layout, window, cx);
        #[cfg(test)]
        written::close(written);
        state
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner
            .paint(id, inspector, bounds, layout, prepaint, window, cx);
    }

    fn a11y_role(&self) -> Option<Role> {
        Some(self.inner.a11y_role().unwrap_or(Role::ScrollView))
    }

    fn write_a11y_info(&self, node: &mut gpui::accesskit::Node) {
        self.inner.write_a11y_info(node);
        node.set_clips_children();
    }

    fn a11y_synthetic_children(
        &mut self,
        prepaint: &mut Self::PrepaintState,
        builder: &mut A11ySubtreeBuilder,
    ) {
        if let Some(visible) = self.visible {
            builder.parent_node().set_bounds(visible);
        }
        self.inner.a11y_synthetic_children(prepaint, builder);
    }
}

/// `page`, everything an open modal dialog covers, as a node that leaves the
/// accessibility tree while `covered`.
///
/// The window draws each dialog over the page, and the dialog's focus trap
/// keeps the keyboard inside it, but the page's nodes stayed in the tree:
/// with the Reflog open, AT-SPI still reached History's lists and buttons,
/// so a screen reader could browse and activate controls the dialog covers.
/// AccessKit's adapters do not act on a dialog's `modal` flag beyond
/// reporting it (AT-SPI's `modal` state, macOS's `AXModal`); every adapter
/// leaves out a hidden node with all it holds, so the page is hidden while
/// a dialog is open and the dialogs, beside it, are what remains. The node
/// is a generic container, which AccessKit's filter passes over, so while
/// nothing covers the page an assistive technology finds the same tree as
/// before. Focus and the Tab order do not change: the wrapper adds no
/// dispatch node, focus handle or listener, and a dialog that closes returns
/// focus to its opener as before.
pub(crate) fn beneath_modal<E: IntoElement>(page: E, covered: bool) -> BeneathModal<E::Element> {
    BeneathModal {
        inner: page.into_element(),
        covered,
    }
}

/// The element [`beneath_modal`] returns.
pub(crate) struct BeneathModal<E> {
    inner: E,
    /// Whether an open dialog covers the page.
    covered: bool,
}

impl<E: Element> IntoElement for BeneathModal<E> {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl<E: Element> Element for BeneathModal<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.inner.id()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.inner.source_location()
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.inner.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        #[cfg(test)]
        let written = written::open(self, None);
        let state = self
            .inner
            .prepaint(id, inspector, bounds, layout, window, cx);
        #[cfg(test)]
        written::close(written);
        state
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner
            .paint(id, inspector, bounds, layout, prepaint, window, cx);
    }

    fn a11y_role(&self) -> Option<Role> {
        Some(self.inner.a11y_role().unwrap_or(Role::GenericContainer))
    }

    fn write_a11y_info(&self, node: &mut gpui::accesskit::Node) {
        self.inner.write_a11y_info(node);
        if self.covered {
            node.set_hidden();
        }
    }

    fn a11y_synthetic_children(
        &mut self,
        prepaint: &mut Self::PrepaintState,
        builder: &mut A11ySubtreeBuilder,
    ) {
        self.inner.a11y_synthetic_children(prepaint, builder);
    }
}

/// `element`, an element that gives itself a role, such as a group or an
/// alert, with the node GPUI writes for it recorded for tests as this
/// module's wrappers record theirs ([`written`]), so a test reads which node
/// holds the nodes drawn inside it. GPUI writes a node only for an element
/// with an ID and a role, and makes each node the child of the nearest
/// element around it that writes one: a container that takes its role only
/// in some layouts moves what it holds under another node as the layout
/// changes, which the test window, which never builds a tree, cannot show
/// otherwise. In a frame where the element has no role it records nothing,
/// as GPUI writes nothing. It changes nothing else: it lays out, draws and
/// writes its node as the element does.
pub(crate) fn recorded<E: IntoElement>(element: E) -> Recorded<E::Element> {
    Recorded {
        inner: element.into_element(),
    }
}

/// The element [`recorded`] returns.
pub(crate) struct Recorded<E> {
    inner: E,
}

impl<E: Element> IntoElement for Recorded<E> {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl<E: Element> Element for Recorded<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.inner.id()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.inner.source_location()
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.inner.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        // With its layout bounds, as GPUI writes it.
        #[cfg(test)]
        let written = written::open(self, Some(physical(bounds, window.scale_factor())));
        let state = self
            .inner
            .prepaint(id, inspector, bounds, layout, window, cx);
        #[cfg(test)]
        written::close(written);
        state
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner
            .paint(id, inspector, bounds, layout, prepaint, window, cx);
    }

    fn a11y_role(&self) -> Option<Role> {
        self.inner.a11y_role()
    }

    fn write_a11y_info(&self, node: &mut gpui::accesskit::Node) {
        self.inner.write_a11y_info(node);
    }

    fn a11y_synthetic_children(
        &mut self,
        prepaint: &mut Self::PrepaintState,
        builder: &mut A11ySubtreeBuilder,
    ) {
        self.inner.a11y_synthetic_children(prepaint, builder);
    }
}

/// `element`, which draws text or stands for it, as static text that an
/// assistive technology reads as `text`: a label whose text is its value, or,
/// where the caller gives it a role of its own (a heading, a status, an
/// alert), that node named by `text` as its label property.
///
/// AccessKit names a label (`Role::Label`) by its value and never by its
/// label property: the AT-SPI and Windows adapters announce the value as the
/// node's name, and macOS reports it as the static text's `AXValue`. Each
/// empty and edge state's line was a `div` with the label role and an
/// `aria_label`, which reached AT-SPI as a label with no name, so a screen
/// reader found none of them. GPUI's own text element writes its text as the
/// value too. The element's other properties, such as an `aria_description`
/// for what its tooltip adds, stay as it writes them.
///
/// Its bounds are the part of it the window draws, as a clipping
/// container's are ([`clips_children`]): text that a scrolling body, a list
/// or a document cuts is reported where it is drawn rather than where its
/// layout runs on. GPUI gives each node its layout bounds, so the GitHub
/// panel's notice reached AT-SPI 162 px tall over the Back to repository
/// button at 18 pt in a 461 px window, and the inspector's message title
/// ran 56 px past its document's foot over "Changed files". The node stays
/// where it is in the tree, so a document read as a whole still reads it.
/// Focus and the Tab order do not change: the wrapper adds no dispatch
/// node, focus handle or listener, and the element needs an ID for GPUI to
/// write a node at all.
pub(crate) fn static_text<E: IntoElement>(
    element: E,
    text: impl Into<SharedString>,
) -> StaticText<E::Element> {
    StaticText {
        inner: element.into_element(),
        text: text.into(),
        visible: None,
    }
}

/// The element [`static_text`] returns.
pub(crate) struct StaticText<E> {
    inner: E,
    text: SharedString,
    /// The part of the text its ancestors leave visible, in the physical
    /// window pixels of AccessKit's bounds, measured as it prepaints, as
    /// [`ClipsChildren`] measures it.
    visible: Option<gpui::accesskit::Rect>,
}

impl<E: Element> IntoElement for StaticText<E> {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl<E: Element> Element for StaticText<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.inner.id()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.inner.source_location()
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.inner.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let visible = drawn_part(bounds, window);
        self.visible = Some(visible);
        #[cfg(test)]
        let written = written::open(self, Some(visible));
        let state = self
            .inner
            .prepaint(id, inspector, bounds, layout, window, cx);
        #[cfg(test)]
        written::close(written);
        state
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner
            .paint(id, inspector, bounds, layout, prepaint, window, cx);
    }

    /// A label, unless the caller gave the text a role of its own, as a
    /// heading, a status or an alert takes.
    fn a11y_role(&self) -> Option<Role> {
        Some(self.inner.a11y_role().unwrap_or(Role::Label))
    }

    /// The text as a label's value, or as the name of the role the caller
    /// gave it, which adapters read from the label property.
    fn write_a11y_info(&self, node: &mut gpui::accesskit::Node) {
        self.inner.write_a11y_info(node);
        if node.role() == Role::Label {
            node.set_value(self.text.to_string());
        } else if node.label().is_none() {
            node.set_label(self.text.to_string());
        }
    }

    /// The node takes the drawn part of the text as its bounds here: GPUI
    /// writes it before it prepaints the element.
    fn a11y_synthetic_children(
        &mut self,
        prepaint: &mut Self::PrepaintState,
        builder: &mut A11ySubtreeBuilder,
    ) {
        if let Some(visible) = self.visible {
            builder.parent_node().set_bounds(visible);
        }
        self.inner.a11y_synthetic_children(prepaint, builder);
    }
}

// Static text styles, holds children and takes a tooltip as the element it
// wraps does, so a call site styles the text after naming it.
impl<E: Styled> Styled for StaticText<E> {
    fn style(&mut self) -> &mut StyleRefinement {
        self.inner.style()
    }
}

impl<E: ParentElement> ParentElement for StaticText<E> {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.inner.extend(elements);
    }
}

impl<E: InteractiveElement> InteractiveElement for StaticText<E> {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.inner.interactivity()
    }
}

impl<E: StatefulInteractiveElement> StatefulInteractiveElement for StaticText<E> {}

/// `text` drawn in a `div` with ID `id`, which is also its debug selector,
/// as static text read by `text`: the shape of every label that draws what
/// it says, such as Tags', Blame's and the ignore form's lines, each of which
/// was a label named only by its label property and so read without a name.
pub(crate) fn text(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
) -> StaticText<Stateful<Div>> {
    let (id, text) = (id.into(), text.into());
    let selector = id.clone();
    static_text(
        div()
            .id(id)
            .debug_selector(move || selector.to_string())
            .child(text.clone()),
        text,
    )
}

/// A dialog's styled title drawing `text`: a label read by its value, as
/// static text is, from which the toolkit's dialog also takes its name: it
/// reads a `Stateful<Div>` title as AccessKit's adapters read a label
/// (`gpui-base`'s `dialog_label`), so the title stays that element rather
/// than [`StaticText`]. Each title was a label named only by its label
/// property, which named the dialog and left the title itself without a
/// name; then it kept both, which macOS reports as the title's `AXTitle`
/// and `AXValue`, so VoiceOver could read it twice.
pub(crate) fn dialog_title(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
) -> Stateful<Div> {
    let (id, text) = (id.into(), text.into());
    let selector = id.clone();
    let title = div()
        .id(id)
        .debug_selector(move || selector.to_string())
        .role(Role::Label)
        .aria_value(text.clone())
        .child(text);
    #[cfg(test)]
    written::built(&title);
    title
}

/// The part of `bounds`, an element's layout as it prepaints, that its
/// ancestors leave visible, in the physical window pixels of AccessKit's
/// bounds. The content mask is what the ancestors let through; GPUI scales
/// layout bounds to a node's bounds the same way. An element the mask leaves
/// out entirely is drawn nowhere: its part is empty, on the mask's edge
/// nearest to it, where `Bounds::intersect` leaves an empty rectangle at the
/// element's own corner, past the clip (the GitHub panel's notice below its
/// body's fold, at the notice's top over Back to repository).
fn drawn_part(bounds: Bounds<Pixels>, window: &Window) -> gpui::accesskit::Rect {
    let mask = physical(window.content_mask().bounds, window.scale_factor());
    let laid_out = physical(bounds, window.scale_factor());
    let (x0, x1) = (mask.x0, mask.x1.max(mask.x0));
    let (y0, y1) = (mask.y0, mask.y1.max(mask.y0));
    gpui::accesskit::Rect {
        x0: laid_out.x0.clamp(x0, x1),
        y0: laid_out.y0.clamp(y0, y1),
        x1: laid_out.x1.clamp(x0, x1),
        y1: laid_out.y1.clamp(y0, y1),
    }
}

/// `bounds` in the physical window pixels of AccessKit's bounds, at `scale`.
fn physical(bounds: Bounds<Pixels>, scale: f32) -> gpui::accesskit::Rect {
    let physical = |edge: Pixels| f64::from(f32::from(edge) * scale);
    gpui::accesskit::Rect {
        x0: physical(bounds.left()),
        y0: physical(bounds.top()),
        x1: physical(bounds.right()),
        y1: physical(bounds.bottom()),
    }
}

/// Test-only: a probe drawn inside an element with an ID that records, under
/// `key`, the ID path that element is drawn at, from which GPUI derives its
/// accessibility node's ID and keys its state (a tooltip's among them), so a
/// test can check that the node keeps its ID from frame to frame. A changed
/// path is a new node, which assistive technology announces again if it is
/// live. Absolutely positioned, it moves nothing; outside tests, nothing.
pub(crate) fn node_path_probe(key: impl FnOnce() -> String) -> Option<AnyElement> {
    #[cfg(test)]
    {
        let key = key();
        Some(
            canvas(
                move |_, window, _| {
                    // The probe's own ID ends the path; the rest is the
                    // element's.
                    let path = window.with_global_id("node-path-probe".into(), |id, _| {
                        id[..id.len() - 1]
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(".")
                    });
                    written::NODE_PATHS.with(|paths| paths.borrow_mut().insert(key, path));
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_0()
            .into_any_element(),
        )
    }
    #[cfg(not(test))]
    {
        let _ = key;
        None
    }
}

/// The nodes this module's wrappers write, read back for tests: the test
/// platform never builds a window's accessibility tree.
#[cfg(test)]
pub(crate) mod written {
    use super::*;
    use std::{cell::RefCell, collections::HashMap};

    /// A node a wrapper wrote as it last prepainted, and the wrapper nearest
    /// around it, by element ID, if any: GPUI makes a node the child of the
    /// node of the nearest element around it that has one.
    #[derive(Clone, Debug)]
    pub(crate) struct Written {
        pub node: gpui::accesskit::Node,
        pub parent: Option<String>,
        /// When it was written, counting every node a wrapper writes:
        /// GPUI writes a frame's nodes in the order the tree holds them.
        pub order: u64,
    }

    /// What a [`LiveRegion`] asked its node to announce: how, whether as a
    /// whole, and whether what it reports is still under way.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) struct Announced {
        pub live: gpui::accesskit::Live,
        pub atomic: bool,
        pub busy: bool,
    }

    impl Announced {
        /// A failure's or a refusal's line, announced at once as a whole.
        pub(crate) const ASSERTIVE: Self = Self {
            live: gpui::accesskit::Live::Assertive,
            atomic: true,
            busy: false,
        };

        /// Writes these properties on `node`, as GPUI's synthetic children
        /// write them on the element's own.
        fn write(self, node: &mut gpui::accesskit::Node) {
            node.set_live(self.live);
            if self.atomic {
                node.set_live_atomic();
            }
            if self.busy {
                node.set_busy();
            }
        }
    }

    thread_local! {
        static NODES: RefCell<HashMap<String, Written>> = Default::default();
        /// What each live region asked for as it was last built, by
        /// element ID: GPUI writes it only while an assistive technology
        /// is active.
        static LIVE: RefCell<HashMap<String, Announced>> = Default::default();
        /// The wrappers prepainting, innermost last.
        static OPEN: RefCell<Vec<String>> = Default::default();
        /// The next node's [`Written::order`].
        static NEXT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
        /// The ID path each [`super::node_path_probe`] last saw, by key.
        pub(super) static NODE_PATHS: RefCell<HashMap<String, String>> = Default::default();
    }

    /// The ID path the element holding the [`super::node_path_probe`] under
    /// `key` was last drawn at.
    pub(crate) fn node_path(key: &str) -> Option<String> {
        NODE_PATHS.with(|paths| paths.borrow().get(key).cloned())
    }

    /// Records the node `element` writes, at `bounds` if given, as the
    /// element starts to prepaint what it holds. GPUI writes a node only
    /// for an element with an ID; so does this.
    pub(super) fn open<E: Element>(
        element: &E,
        bounds: Option<gpui::accesskit::Rect>,
    ) -> Option<String> {
        let id = element.id()?.to_string();
        let role = element.a11y_role()?;
        let mut node = gpui::accesskit::Node::new(role);
        element.write_a11y_info(&mut node);
        if let Some(bounds) = bounds {
            node.set_bounds(bounds);
        }
        if let Some(announced) = announced(&id) {
            announced.write(&mut node);
        }
        let parent = OPEN.with(|open| open.borrow().last().cloned());
        let order = NEXT.with(|next| next.replace(next.get() + 1));
        NODES.with(|nodes| {
            nodes.borrow_mut().insert(
                id.clone(),
                Written {
                    node,
                    parent,
                    order,
                },
            )
        });
        OPEN.with(|open| open.borrow_mut().push(id.clone()));
        Some(id)
    }

    /// Records the node an element GPUI writes itself, such as a dialog's
    /// title, as it is built: without bounds or the wrapper around it.
    pub(super) fn built<E: Element>(element: &E) {
        let (Some(id), Some(role)) = (element.id(), element.a11y_role()) else {
            return;
        };
        let mut node = gpui::accesskit::Node::new(role);
        element.write_a11y_info(&mut node);
        let order = NEXT.with(|next| next.replace(next.get() + 1));
        NODES.with(|nodes| {
            nodes.borrow_mut().insert(
                id.to_string(),
                Written {
                    node,
                    parent: None,
                    order,
                },
            )
        });
    }

    /// Every label recorded since [`forget`], by element ID, with the name
    /// AccessKit's adapters give it: its value.
    pub(crate) fn labels() -> Vec<(String, Option<String>)> {
        NODES.with(|nodes| {
            let mut labels: Vec<(u64, String, Option<String>)> = nodes
                .borrow()
                .iter()
                .filter(|(_, written)| written.node.role() == Role::Label)
                .map(|(id, written)| {
                    (
                        written.order,
                        id.clone(),
                        written.node.value().map(str::to_owned),
                    )
                })
                .collect();
            labels.sort();
            labels.into_iter().map(|(_, id, name)| (id, name)).collect()
        })
    }

    /// Ends what [`open`] began, once the element has prepainted.
    pub(super) fn close(opened: Option<String>) {
        if opened.is_some() {
            OPEN.with(|open| open.borrow_mut().pop());
        }
    }

    /// What the wrapper with element ID `id` wrote as it last prepainted.
    pub(crate) fn get(id: &str) -> Option<Written> {
        NODES.with(|nodes| nodes.borrow().get(id).cloned())
    }

    /// Whether the wrapper `id` was last drawn inside the wrapper `outer`.
    pub(crate) fn lies_in(id: &str, outer: &str) -> bool {
        let mut parent = get(id).and_then(|written| written.parent);
        while let Some(id) = parent {
            if id == outer {
                return true;
            }
            parent = get(&id).and_then(|written| written.parent);
        }
        false
    }

    /// The wrappers the wrapper `id` was last drawn inside, by element ID
    /// with what each wrote, innermost first.
    pub(crate) fn ancestors(id: &str) -> Vec<(String, Written)> {
        let mut found = Vec::new();
        let mut parent = get(id).and_then(|written| written.parent);
        while let Some(id) = parent {
            let Some(written) = get(&id) else {
                break;
            };
            parent = written.parent.clone();
            found.push((id, written));
        }
        found
    }

    /// The wrappers drawn inside the wrapper `outer` as they last
    /// prepainted, by element ID.
    pub(crate) fn inside(outer: &str) -> Vec<String> {
        let ids: Vec<String> = NODES.with(|nodes| nodes.borrow().keys().cloned().collect());
        ids.into_iter().filter(|id| lies_in(id, outer)).collect()
    }

    /// Records what the live region `element` asks its node to announce, as
    /// it is built: GPUI writes it only while an assistive technology is
    /// active.
    pub(super) fn live<E: Element>(element: &E, announced: Announced) {
        if let Some(id) = element.id() {
            LIVE.with(|live| live.borrow_mut().insert(id.to_string(), announced));
        }
    }

    /// What the live region with element ID `id` asked its node to
    /// announce as it was built in the window's latest draw ([`draw_begins`])
    /// or, for an element built outside one, since [`forget`]; `None` for an
    /// element that is not one.
    pub(crate) fn announced(id: &str) -> Option<Announced> {
        LIVE.with(|live| live.borrow().get(id).copied())
    }

    /// A window's draw begins: what [`announced`] returns, and what [`open`]
    /// copies onto the nodes it records, comes from this draw's builds from
    /// now on, so an element drawn again without a live region is no longer
    /// one. The app's window calls it as its root view renders, before
    /// anything else in the window is built. Recorded nodes stay until
    /// [`forget`], as a cached view's wrappers are drawn again without
    /// prepainting; a live region in a cached view is therefore not
    /// announced after a draw that did not build it again.
    pub(crate) fn draw_begins() {
        LIVE.with(|live| live.borrow_mut().clear());
    }

    /// Forgets every node [`get`] would return and every live region
    /// [`announced`] would, so that what a test reads after its next draw
    /// comes from that draw.
    pub(crate) fn forget() {
        NODES.with(|nodes| nodes.borrow_mut().clear());
        LIVE.with(|live| live.borrow_mut().clear());
    }

    /// A window's tree as GPUI builds it, assembled for a test from the
    /// nodes the wrappers wrote, each where it was drawn, and the nodes the
    /// test adds for elements without a wrapper, such as a dialog's own node
    /// or a list's rows, at their rendered bounds.
    #[derive(Default)]
    pub(crate) struct Assembly {
        /// Each node by its key, with its parent's key, `None` for the root.
        nodes: Vec<(String, gpui::accesskit::Node, Option<String>)>,
    }

    impl Assembly {
        /// Adds the node the wrapper `id` last wrote, under the wrapper it
        /// was drawn in, which it adds first if it is not yet added, and so
        /// on out; the outermost goes under `outside` (the root if `None`).
        /// A wrapper that was not drawn adds nothing.
        pub(crate) fn written(mut self, id: &str, outside: Option<&str>) -> Self {
            let Some(Written { node, parent, .. }) = get(id) else {
                return self;
            };
            if self.has(id) {
                return self;
            }
            let parent = match parent {
                Some(parent) => {
                    self = self.written(&parent, outside);
                    Some(parent)
                }
                None => outside.map(str::to_owned),
            };
            self.nodes.push((id.to_owned(), node, parent));
            self
        }

        /// Every node the wrappers wrote, in the order they wrote them,
        /// each under the wrapper it was drawn in. Those drawn in none are
        /// the page and, while one is open, what the dialog layer drew
        /// beside it: these go under `dialog`, a node the caller adds for
        /// the dialog's own, when given.
        pub(crate) fn everything(mut self, dialog: Option<&str>) -> Self {
            let mut written: Vec<(String, Written)> =
                NODES.with(|nodes| nodes.borrow().clone().into_iter().collect());
            written.sort_by_key(|(_, written)| written.order);
            for (id, Written { node, parent, .. }) in written {
                let parent =
                    parent.or_else(|| dialog.filter(|_| id != "gitturtle-page").map(str::to_owned));
                self.nodes.push((id, node, parent));
            }
            self
        }

        /// Whether a node `key` was added.
        pub(crate) fn has(&self, key: &str) -> bool {
            self.nodes.iter().any(|(added, ..)| added == key)
        }

        /// Adds `node` under `parent`, the root if `None`, as `key`.
        pub(crate) fn node(
            mut self,
            key: &str,
            node: gpui::accesskit::Node,
            parent: Option<&str>,
        ) -> Self {
            self.nodes
                .push((key.to_owned(), node, parent.map(str::to_owned)));
            self
        }

        /// The tree: a window root at `window` holding the nodes, in the
        /// order they were added, with focus on `focus`, the root if `None`.
        /// A node's ID comes from its key, as GPUI's comes from its
        /// element's, so the trees of successive frames update one another.
        pub(crate) fn tree(
            self,
            window: gpui::accesskit::Rect,
            focus: Option<&str>,
        ) -> gpui::accesskit::TreeUpdate {
            use gpui::accesskit::{Node, NodeId, Role, Tree, TreeId, TreeUpdate};
            use std::hash::{Hash, Hasher};
            let id = |key: &str| {
                assert!(self.has(key), "no node {key} was added");
                let mut hasher = std::hash::DefaultHasher::new();
                key.hash(&mut hasher);
                // Never the root's 0.
                NodeId((hasher.finish() >> 1) + 1)
            };
            let children = |parent: Option<&str>| {
                self.nodes
                    .iter()
                    .filter(|(_, _, of)| of.as_deref() == parent)
                    .map(|(key, ..)| id(key))
                    .collect::<Vec<_>>()
            };
            let mut nodes: Vec<(NodeId, Node)> = self
                .nodes
                .iter()
                .map(|(key, node, _)| {
                    let mut node = node.clone();
                    node.set_children(children(Some(key)));
                    (id(key), node)
                })
                .collect();
            let mut root = Node::new(Role::Window);
            root.set_bounds(window);
            root.set_children(children(None));
            nodes.push((NodeId(0), root));
            TreeUpdate {
                nodes,
                tree: Some(Tree::new(NodeId(0))),
                tree_id: TreeId::ROOT,
                focus: focus.map_or(NodeId(0), id),
            }
        }
    }
}

/// The node the [`clips_children`] element with `id` wrote as it last
/// prepainted, with its role, clipping flag and visible bounds.
#[cfg(test)]
pub(crate) fn clipping_node(id: &str) -> Option<gpui::accesskit::Node> {
    written::get(id)
        .map(|written| written.node)
        .filter(|node| node.clips_children())
}

/// Forgets every node [`clipping_node`] would return, so that what a test
/// reads after its next draw comes from that draw.
#[cfg(test)]
pub(crate) fn forget_clipping_nodes() {
    written::forget();
}

/// A node an assistive technology reaches, as every AccessKit adapter
/// filters the tree (`accesskit_consumer::common_filter`).
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct Reachable {
    pub role: Role,
    pub label: Option<String>,
    /// The name the adapters announce: a label's value, otherwise its label
    /// (`accesskit_consumer::Node::label_comes_from_value`).
    pub name: Option<String>,
    pub bounds: Option<gpui::accesskit::Rect>,
    /// The bounds of the nearest node around it that clips its children.
    pub clip: Option<gpui::accesskit::Rect>,
}

/// Every node an assistive technology reaches walking `update`'s tree from
/// its root through AccessKit's own filter, the one the AT-SPI, macOS and
/// Windows adapters apply, in tree order.
#[cfg(test)]
pub(crate) fn reachable(update: gpui::accesskit::TreeUpdate) -> Vec<Reachable> {
    use accesskit_consumer::{Node, Tree, common_filter};
    fn walk(node: Node<'_>, clip: Option<gpui::accesskit::Rect>, found: &mut Vec<Reachable>) {
        found.push(Reachable {
            role: node.role(),
            label: node.label(),
            name: if node.label_comes_from_value() {
                node.value()
            } else {
                node.label()
            },
            bounds: node.bounding_box(),
            clip,
        });
        let clip = if node.clips_children() {
            node.bounding_box()
        } else {
            clip
        };
        for child in node.filtered_children(common_filter) {
            walk(child, clip, found);
        }
    }
    let tree = Tree::new(update, true);
    let mut found = Vec::new();
    walk(tree.state().root(), None, &mut found);
    found
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct DisplayPreferences {
    pub reduce_motion: bool,
    pub increase_contrast: bool,
    pub reduce_transparency: bool,
}

#[cfg(target_os = "macos")]
pub(super) fn display_preferences() -> Option<DisplayPreferences> {
    let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
    Some(DisplayPreferences {
        reduce_motion: workspace.accessibilityDisplayShouldReduceMotion(),
        increase_contrast: workspace.accessibilityDisplayShouldIncreaseContrast(),
        reduce_transparency: workspace.accessibilityDisplayShouldReduceTransparency(),
    })
}
#[cfg(not(target_os = "macos"))]
pub(super) fn display_preferences() -> Option<DisplayPreferences> {
    None
}

pub(super) fn sync_preferences(cx: &mut App) {
    if let Some(preferences) = display_preferences() {
        cx.set_reduce_motion(preferences.reduce_motion);
    }
}

pub(super) fn next_cursor(
    rows: &[NavRow],
    cursor: Option<usize>,
    forward: bool,
    boundary: bool,
) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    let navigable = |index: usize| !matches!(rows[index], NavRow::Section(..));
    let current = cursor.filter(|index| *index < rows.len() && navigable(*index));
    if current.is_none() && !boundary {
        return (0..rows.len()).find(|index| navigable(*index));
    }
    if boundary {
        if forward {
            (0..rows.len()).rev().find(|index| navigable(*index))
        } else {
            (0..rows.len()).find(|index| navigable(*index))
        }
    } else {
        let current = current.expect("validated cursor");
        if forward {
            ((current + 1)..rows.len())
                .find(|index| navigable(*index))
                .or(Some(current))
        } else {
            (0..current)
                .rev()
                .find(|index| navigable(*index))
                .or(Some(current))
        }
    }
}
impl GitTurtle {
    pub(super) fn move_navigation(
        &mut self,
        forward: bool,
        boundary: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != AppPage::Repository
            || window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
        {
            return;
        }
        self.nav_cursor = next_cursor(&self.nav_rows, self.nav_cursor, forward, boundary);
        if let Some(index) = self.nav_cursor {
            self.nav_scroll
                .scroll_to_item(index, ScrollStrategy::Center);
        }
        window.focus(&self.nav_focus, cx);
        cx.notify();
    }
    pub(super) fn expand_navigation(
        &mut self,
        expand: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        let Some(index) = self.nav_cursor else {
            return;
        };
        let Some(row) = self.nav_rows.get(index).cloned() else {
            return;
        };
        if let NavRow::Folder { key, expanded, .. } = &row
            && *expanded != expand
        {
            if expand {
                self.expanded_folders.insert(key.clone());
            } else {
                self.expanded_folders.remove(key);
            }
            self.rebuild_navigation(cx);
            cx.notify();
            return;
        }
        let depth = match row {
            NavRow::Folder { depth, .. } | NavRow::Branch(_, depth) => depth,
            _ => return,
        };
        if expand {
            if matches!(self.nav_rows.get(index+1),Some(NavRow::Folder {depth:next,..})|Some(NavRow::Branch(_,next)) if *next>depth){self.move_navigation(true,false,window,cx);}
        }else if depth>0 && let Some(parent)=(0..index).rev().find(|index|matches!(self.nav_rows[*index],NavRow::Folder {depth:parent,..} if parent<depth)) {
            self.nav_cursor=Some(parent);self.nav_scroll.scroll_to_item(parent,ScrollStrategy::Center);cx.notify();
        }
    }
    pub(super) fn activate_navigation(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != AppPage::Repository
            || window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
        {
            return;
        }
        let Some(row) = self.nav_rows.get(index).cloned() else {
            return;
        };
        self.nav_cursor = Some(index);
        self.limit = 500;
        match row {
            NavRow::Folder { key, .. } => {
                if !self.expanded_folders.remove(&key) {
                    self.expanded_folders.insert(key);
                }
                self.rebuild_navigation(cx);
            }
            NavRow::All => {
                if let Some(path) = self.path.clone() {
                    self.open(path, None, window, cx);
                }
            }
            NavRow::Branch(index, _) => {
                if let Some(path) = self.path.clone() {
                    let branch = &self.branches[index];
                    self.open(
                        path,
                        Some((
                            branch.name.clone(),
                            worker::Scope::Branch {
                                name: branch.name.clone(),
                                remote: branch.remote,
                            },
                        )),
                        window,
                        cx,
                    );
                }
            }
            NavRow::Worktree(index) => {
                let tree = &self.worktrees[index];
                let path = tree.path.clone();
                let scope = Some((
                    tree.branch.clone().unwrap_or("Detached worktree".into()),
                    worker::Scope::Worktree { path: path.clone() },
                ));
                self.open(path, scope, window, cx);
            }
            NavRow::Section(..) => return,
        }
        window.focus(&self.nav_focus, cx);
        cx.notify();
    }
}

#[cfg(all(test, target_os = "linux"))]
pub(crate) mod atspi_state_tests;
#[cfg(test)]
mod control_tests;
#[cfg(test)]
mod dialog_tests;
#[cfg(test)]
mod input_tests;
#[cfg(test)]
pub(crate) mod menu_tests;
#[cfg(test)]
mod state_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[gpui::test]
    fn rendered_buttons_report_disabled_and_tab_selection(cx: &mut TestAppContext) {
        use gpui::Element as _;
        use gpui_kit::base::Button as BaseButton;
        type Nodes = Arc<std::sync::Mutex<Vec<gpui::accesskit::Node>>>;
        struct Probe(Nodes);
        impl Render for Probe {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let nodes = self.0.clone();
                canvas(
                    move |_, window, cx| {
                        let mut capture = |id: &'static str, disabled, role, selected| {
                            let button = BaseButton::new(id)
                                .disabled(disabled)
                                .role(role)
                                .selected(selected)
                                .accessibility_label("Send review")
                                .on_click(|_, _, _| {});
                            let element = RenderOnce::render(button, window, cx).into_element();
                            let mut node = gpui::accesskit::Node::new(element.a11y_role().unwrap());
                            element.write_a11y_info(&mut node);
                            node
                        };
                        *nodes.lock().unwrap() = vec![
                            capture("enabled", false, Role::Button, false),
                            capture("disabled", true, Role::Button, false),
                            capture("selected-tab", false, Role::Tab, true),
                            capture("unselected-tab", false, Role::Tab, false),
                            capture("selected-button", false, Role::Button, true),
                        ];
                    },
                    |_, _, _, _| {},
                )
            }
        }
        let nodes: Nodes = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = nodes.clone();
        let (_, cx) = cx.add_window_view(move |_, _| Probe(nodes));
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        let nodes = observed.lock().unwrap();
        let [
            enabled,
            disabled,
            selected_tab,
            unselected_tab,
            selected_button,
        ] = nodes.as_slice()
        else {
            panic!("five rendered nodes expected");
        };
        assert_eq!(enabled.role(), Role::Button);
        assert_eq!(disabled.role(), Role::Button);
        assert_eq!(enabled.label(), Some("Send review"));
        assert_eq!(disabled.label(), Some("Send review"));
        assert!(!enabled.is_disabled());
        assert!(disabled.is_disabled());
        assert!(enabled.supports_action(gpui::accesskit::Action::Click));
        assert!(!disabled.supports_action(gpui::accesskit::Action::Click));
        assert_eq!(selected_tab.role(), Role::Tab);
        assert_eq!(unselected_tab.role(), Role::Tab);
        assert_eq!(selected_tab.is_selected(), Some(true));
        assert_eq!(unselected_tab.is_selected(), Some(false));
        assert_eq!(selected_tab.toggled(), None);
        assert_eq!(unselected_tab.toggled(), None);
        assert_eq!(selected_button.is_selected(), None);
        assert_eq!(selected_button.toggled(), None);
    }

    #[gpui::test]
    fn focus_traps_preserve_named_dialog_and_group_semantics(cx: &mut TestAppContext) {
        use gpui::Element as _;
        use gpui_kit::base::FocusTrapElement as _;
        cx.update(|cx| {
            let focus = cx.focus_handle();
            for role in [Role::Dialog, Role::AlertDialog, Role::Group] {
                let element = div()
                    .id("captured-dialog")
                    .role(role)
                    .aria_label("Review captured destination")
                    .aria_description("Only this reviewed repository is affected")
                    .focus_trap("dialog-trap", &focus);
                let mut node = gpui::accesskit::Node::new(element.a11y_role().unwrap());
                element.write_a11y_info(&mut node);
                assert_eq!(node.role(), role);
                assert_eq!(node.label(), Some("Review captured destination"));
                assert_eq!(
                    node.description(),
                    Some("Only this reviewed repository is affected")
                );
                assert_eq!(node.is_modal(), role != Role::Group);
            }
        });
    }

    #[test]
    fn navigator_skips_sections_stops_at_bounds_and_recovers_removed_cursor() {
        let rows = vec![
            NavRow::All,
            NavRow::Section("Branches", 2),
            NavRow::Branch(0, 0),
            NavRow::Branch(1, 0),
        ];
        assert_eq!(next_cursor(&rows, Some(0), true, false), Some(2));
        assert_eq!(next_cursor(&rows, Some(2), false, false), Some(0));
        assert_eq!(next_cursor(&rows, Some(3), true, false), Some(3));
        assert_eq!(next_cursor(&rows, Some(2), false, true), Some(0));
        assert_eq!(next_cursor(&rows, Some(0), true, true), Some(3));
        assert_eq!(next_cursor(&rows, Some(99), false, false), Some(0));
        assert_eq!(next_cursor(&rows, None, true, false), Some(0));
        assert_eq!(next_cursor(&[], None, true, false), None);
    }

    /// A line drawn as an assertive live region and drawn again without
    /// one is no longer one: neither what it asks to announce nor the node
    /// its wrapper records keeps the earlier draw's flags, as each window
    /// draw starts the live regions afresh.
    #[gpui::test]
    fn a_line_drawn_again_without_a_live_region_announces_nothing(cx: &mut TestAppContext) {
        struct Line(std::rc::Rc<std::cell::Cell<bool>>);
        impl Render for Line {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                written::draw_begins();
                let line = text("probe-line", "Could not save").role(Role::Alert);
                if self.0.get() {
                    line.assertive().into_any_element()
                } else {
                    line.into_any_element()
                }
            }
        }
        let live = std::rc::Rc::new(std::cell::Cell::new(true));
        let shown = live.clone();
        let (view, cx) = cx.add_window_view(move |_, _| Line(shown));
        let draw = |cx: &mut VisualTestContext| {
            view.update(cx, |_, cx| cx.notify());
            cx.update(|window, cx| window.draw(cx).clear(cx));
        };
        written::forget();
        draw(cx);
        assert_eq!(
            written::announced("probe-line"),
            Some(written::Announced::ASSERTIVE)
        );
        let node = written::get("probe-line")
            .expect("the line is recorded")
            .node;
        assert_eq!(node.live(), Some(gpui::accesskit::Live::Assertive));

        live.set(false);
        draw(cx);
        assert_eq!(written::announced("probe-line"), None);
        let node = written::get("probe-line")
            .expect("the line is recorded")
            .node;
        assert_eq!(node.live(), None, "no earlier flag reaches the node");
        assert!(!node.is_live_atomic());
    }
}
