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
    /// Whether the row lies elsewhere than `other` did, by more than
    /// rounding.
    fn moved_from(&self, other: Self) -> bool {
        let apart = |a: Pixels, b: Pixels| (a - b).abs() > px(0.5);
        apart(self.x, other.x) || apart(self.top, other.top) || apart(self.below, other.below)
    }

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

/// Where a row's menu goes ([`RowMenu`]), read as the menu is prepainted,
/// after its row's list or the row itself has been prepainted in that
/// frame: a menu that follows its row stays beside it in the frame that
/// rows coming or going around it move it.
#[derive(Clone)]
pub(super) enum RowPlace {
    /// Where the row was as the menu opened.
    Opened(RowAnchor),
    /// A right-click's row, which writes where it lies, from its bounds and
    /// the pointer's place in it, as it is prepainted ([`PointerMenu`]).
    Pointer(std::rc::Rc<std::cell::Cell<RowAnchor>>),
    /// A uniform list's row, where the list lays it out ([`ListRow`]).
    Listed(ListRow),
}

impl RowPlace {
    /// Where the row lies, within its list's visible part.
    fn anchor(&self) -> RowAnchor {
        match self {
            Self::Opened(row) => *row,
            Self::Pointer(row) => row.get(),
            Self::Listed(row) => row.anchor(),
        }
    }

    /// Whether the row is drawn: a listed row that the list's viewport shows
    /// any of, as the list draws only those. A right-click's row draws its
    /// menu only while it is drawn itself.
    fn drawn(&self) -> bool {
        match self {
            Self::Listed(row) => row.drawn(),
            Self::Opened(_) | Self::Pointer(_) => true,
        }
    }
}

impl From<RowAnchor> for RowPlace {
    fn from(row: RowAnchor) -> Self {
        Self::Opened(row)
    }
}

impl From<ListRow> for RowPlace {
    fn from(row: ListRow) -> Self {
        Self::Listed(row)
    }
}

/// The `row`th row of a uniform list of `height` tall rows, its keyboard
/// menu starting `inset` from the list's leading edge, placed as
/// [`KeyboardMenu::cursor_row`] places it from the list's bounds and scroll
/// in the frame being drawn. Its owner moves it among the rows as rows come
/// or go before it ([`KeyboardMenu::follow_row`]).
#[derive(Clone)]
pub(super) struct ListRow {
    scroll: UniformListScrollHandle,
    row: usize,
    height: Pixels,
    inset: Pixels,
}

impl ListRow {
    pub(super) fn new(
        scroll: &UniformListScrollHandle,
        row: usize,
        height: Pixels,
        inset: Pixels,
    ) -> Self {
        Self {
            scroll: scroll.clone(),
            row,
            height,
            inset,
        }
    }

    fn anchor(&self) -> RowAnchor {
        KeyboardMenu::cursor_row(&self.scroll, self.row, self.height, self.inset)
    }

    fn drawn(&self) -> bool {
        let state = self.scroll.0.borrow();
        let bounds = state.base_handle.bounds();
        let top = bounds.top() + state.base_handle.offset().y + self.height * self.row as f32;
        top < bounds.bottom() && top + self.height > bounds.top()
    }

    /// Scrolls the list the least that shows the whole row, as
    /// `ScrollStrategy::Nearest` does, at once rather than at the list's next
    /// layout, so a menu opened beside it is placed and sized beside the row
    /// in view: opened for a row a wheel had scrolled away, it lay pinned to
    /// the list's edge, beside no row.
    fn reveal(&self) {
        let state = self.scroll.0.borrow();
        let bounds = state.base_handle.bounds();
        let offset = state.base_handle.offset();
        let top = bounds.top() + offset.y + self.height * self.row as f32;
        let shift = if top < bounds.top() {
            bounds.top() - top
        } else if top + self.height > bounds.bottom() {
            (bounds.bottom() - top - self.height).max(bounds.top() - top)
        } else {
            return;
        };
        state
            .base_handle
            .set_offset(point(offset.x, offset.y + shift));
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
///
/// A menu whose items read the window's state follows writes and pauses as
/// a right-click menu does ([`Self::following`], [`MenuInputs`]): built
/// again in place, still focused, when a write starts or ends, an
/// integration pauses or ends or, for a commit's, its commit becomes or
/// stops being HEAD, its keyboard's cursor kept where the keys left it
/// ([`Self::rebuild`]); and closed, focus
/// back on its list or tree, once its row no longer holds what it was
/// opened on. It read both
/// once, as it opened, so one opened at rest offered Revert while a merge
/// paused under it and one opened during a quick write kept its items
/// disabled after the write; built again for the row it opened on, it
/// offered a commit's Revert once an Undo had taken that commit from the row.
pub(super) struct KeyboardMenu {
    menu: Entity<PopupMenu>,
    row: RowPlace,
    /// The window's size when the menu opened, which its overlay covers. The
    /// menu also closes on a press outside itself, past an overlay that a
    /// larger window has outgrown.
    viewport: Size<Pixels>,
    /// Where the menu's fill may lie when it opened ([`menu_bounds`]).
    room: Bounds<Pixels>,
    /// Where its row was as it opened.
    opened: RowAnchor,
    /// Its action context, and the tallest its items may be.
    focus: FocusHandle,
    cap: Pixels,
    /// What Enter in the menu reports while an integration is paused
    /// ([`Self::refuses_while_paused`]).
    paused: Option<fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>)>,
    _subscriptions: [Subscription; 2],
    /// Keeps a [`Self::following`] or [`Self::followed_by`] menu with the
    /// window's state.
    _follows: Option<Subscription>,
}

/// How a [`KeyboardMenu`] opened by [`KeyboardMenu::following`] builds its
/// items from the window's state.
type BuildFrom = dyn Fn(PopupMenu, &GitTurtle) -> PopupMenu;

impl KeyboardMenu {
    /// Builds the row's menu once with `build`, the builder its right-click
    /// menu uses, focuses it, and calls `dismissed` once it closes: a menu
    /// whose items read nothing a write or a pause changes, as a changed
    /// file's File history…, which nothing builds again. Its items are no
    /// taller than the
    /// room below `row` where that holds about four rows ([`MENU_FLOOR`]),
    /// and otherwise no taller than the larger of the room below and the room
    /// above, where it goes when the room below is too short for it. Opened
    /// beside a list's row ([`ListRow`]), it is drawn where the list lays
    /// that row out in each frame.
    pub(super) fn open(
        row: impl Into<RowPlace>,
        focus: FocusHandle,
        build: impl FnOnce(PopupMenu) -> PopupMenu + 'static,
        dismissed: fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>),
        window: &mut Window,
        cx: &mut Context<GitTurtle>,
    ) -> Self {
        Self::built(row.into(), focus, build, dismissed, window, cx)
    }

    /// [`Self::open`] for a menu whose items read the window's state: from
    /// `app` as it opens, while the window is being updated, and from the
    /// window itself after. It is built again in place, as [`Self::rebuild`]
    /// builds it, when a write starts or ends or an integration pauses or
    /// ends, and a commit's menu, its `commit` given, also once that commit
    /// becomes or stops being HEAD ([`MenuInputs`]), and closes through
    /// `dismissed`, which returns
    /// focus to its list or tree, as soon as `holds` finds that its row no
    /// longer holds what the menu acts on, at the place the menu opened
    /// beside: a write's quiet reload can take a commit from History's row
    /// (an Undo or an Amend), and a branch deleted or renamed moves the
    /// navigator's rows.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn following(
        row: impl Into<RowPlace>,
        focus: FocusHandle,
        build: impl Fn(PopupMenu, &GitTurtle) -> PopupMenu + 'static,
        holds: impl Fn(&GitTurtle) -> bool + 'static,
        commit: Option<MenuCommit>,
        dismissed: fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>),
        app: &GitTurtle,
        window: &mut Window,
        cx: &mut Context<GitTurtle>,
    ) -> Self {
        let build: std::rc::Rc<BuildFrom> = std::rc::Rc::new(build);
        let first = build.clone();
        let mut this = Self::built(
            row.into(),
            focus,
            move |menu| first(menu, app),
            dismissed,
            window,
            cx,
        );
        // The window is being updated as the menu opens, so it reads `app`
        // now and the window itself once the menu is open.
        let mut inputs = MenuInputs::of(app, commit.as_ref());
        let mut gone = false;
        let (focus, cap) = (this.focus.clone(), this.cap);
        this.watch(window, cx, move |app, menu, window, cx| {
            if gone {
                return;
            }
            if !holds(app) {
                // Closed as Escape closes it, its focus back on the list or
                // tree it was opened from.
                gone = true;
                menu.update(cx, |_, cx| cx.emit(DismissEvent));
                return;
            }
            let now = MenuInputs::of(app, commit.as_ref());
            if now != inputs {
                inputs = now;
                let app = &*app;
                Self::build_again(
                    menu,
                    focus.clone(),
                    cap,
                    |menu| build(menu, app),
                    window,
                    cx,
                );
            }
        });
        this
    }

    /// The menu, opened by [`Self::open`], is kept with the window's state
    /// by its owner: `follow` runs whenever a write starts or ends or an
    /// integration pauses or ends ([`MenuInputs`], read from `app` as the
    /// menu opens), as a Working Changes row's menu is built again from its
    /// file as it then stands (`GitTurtle::follow_working_menu`), rather
    /// than from what the file was as the menu opened, and whenever the
    /// reason `wait` gives for the rows its row's list is preparing changes
    /// ([`PointerMenu::waits_while`]), which its items, refused meanwhile,
    /// state: a newer status that landed while the filter's rows were
    /// prepared left a menu saying "Filtering…" until the rows landed.
    pub(super) fn followed_by(
        mut self,
        follow: fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>),
        wait: RowsWait,
        app: &GitTurtle,
        window: &mut Window,
        cx: &mut Context<GitTurtle>,
    ) -> Self {
        let mut inputs = (MenuInputs::of(app, None), wait(app));
        self.watch(window, cx, move |app, _, window, cx| {
            let now = (MenuInputs::of(app, None), wait(app));
            if now != inputs {
                inputs = now;
                follow(app, window, cx);
            }
        });
        self
    }

    /// Calls `changed` with the window's state, and the menu, each time the
    /// window changes while the menu is open.
    fn watch(
        &mut self,
        window: &mut Window,
        cx: &mut Context<GitTurtle>,
        mut changed: impl FnMut(
            &mut GitTurtle,
            &Entity<PopupMenu>,
            &mut Window,
            &mut Context<GitTurtle>,
        ) + 'static,
    ) {
        let (menu, owner) = (self.menu.downgrade(), cx.entity());
        self._follows = Some(cx.observe_in(&owner, window, move |app, _, window, cx| {
            if let Some(menu) = menu.upgrade() {
                changed(app, &menu, window, cx);
            }
        }));
    }

    /// The menu built with `build` beside `row`, focused.
    fn built(
        row: RowPlace,
        focus: FocusHandle,
        build: impl FnOnce(PopupMenu) -> PopupMenu,
        dismissed: fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>),
        window: &mut Window,
        cx: &mut Context<GitTurtle>,
    ) -> Self {
        if let RowPlace::Listed(listed) = &row {
            listed.reveal();
        }
        let room = menu_bounds(window);
        let opened = row.anchor();
        let cap = opened.cap(room);
        let menu = PopupMenu::build(window, cx, |menu, window, cx| {
            Self::finish(build(menu), focus.clone(), cap, window, cx)
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
            opened,
            focus,
            cap,
            paused: None,
            _subscriptions: [dismiss, blur],
            _follows: None,
        }
    }

    /// The menu's items, opened from the keyboard in `focus`'s context, no
    /// taller than `cap`.
    fn finish(
        menu: PopupMenu,
        focus: FocusHandle,
        cap: Pixels,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        open_menu(
            menu.action_context(focus),
            true,
            None,
            Some(cap),
            window,
            cx,
        )
    }

    /// Builds the menu again in place with `build`, as it opened: the row's
    /// file changed under it, and its items act on what the row now shows.
    /// Its keyboard's cursor stays on the item it was on, by its label, and
    /// never moves to another: where that item is gone or can no longer act,
    /// as while a write refuses it or once it states a condition it waits
    /// for, the cursor rests on none until the item can act again, when it
    /// returns to it, or a key or the pointer moves it (the kit's
    /// `PopupMenu::rebuild`). Once the pointer has taken the selection, or
    /// taken the cursor off its row, nothing is selected after the rebuild
    /// until a key places the cursor again, though the builder opens the menu
    /// on its first item. A right-click's menu built again
    /// ([`PointerMenu::built_from`]) keeps its cursor the same way. The
    /// cursor once returned to the first item that can act, and a
    /// right-click's lost it, so Enter just after a write ended could choose
    /// another item than the one the keys had reached; later it moved to the
    /// nearest item that could act, so a conflicted file's refused Discard
    /// changes… left it on Stage, and Enter marked the file resolved; and a
    /// menu the pointer had moved over took the builder's first item, Stage
    /// on a narrow Working row, where Enter staged the file.
    pub(super) fn rebuild(
        &self,
        build: impl FnOnce(PopupMenu) -> PopupMenu,
        window: &mut Window,
        cx: &mut App,
    ) {
        Self::build_again(&self.menu, self.focus.clone(), self.cap, build, window, cx);
    }

    /// [`Self::rebuild`] for the open `menu`, in `focus`'s context and no
    /// taller than `cap`. The builder opens the menu from the keyboard,
    /// on its first item that can act, as it opened; the kit's rebuild
    /// then puts the cursor back where the keys had left it, or rests it on
    /// none while that item cannot act. Where the kit remembers no item, the
    /// builder's cursor on the first item stands, as for a menu that opened
    /// with every item refused, unless the pointer has taken the selection
    /// or the cursor off its row since a key last placed it: then nothing is
    /// selected.
    fn build_again(
        menu: &Entity<PopupMenu>,
        focus: FocusHandle,
        cap: Pixels,
        build: impl FnOnce(PopupMenu) -> PopupMenu,
        window: &mut Window,
        cx: &mut App,
    ) {
        menu.update(cx, |menu, cx| {
            menu.rebuild(window, cx, move |menu, window, cx| {
                Self::finish(build(menu), focus, cap, window, cx)
            })
        });
    }

    /// The open menu, for tests that watch it built again.
    #[cfg(test)]
    pub(super) fn menu(&self) -> &Entity<PopupMenu> {
        &self.menu
    }

    /// The menu of a row whose every item a paused merge, rebase,
    /// cherry-pick or revert refuses, a commit's: Enter in it then reports
    /// the refusal through `refuse` and keeps the menu open, focused, rather
    /// than doing nothing. Built again disabled, its items take no Enter, and
    /// the keyboard's cursor rests on none of them.
    pub(super) fn refuses_while_paused(
        mut self,
        refuse: fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>),
    ) -> Self {
        self.paused = Some(refuse);
        self
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

    /// The menu, opened beside a list's row ([`ListRow`]), now acts on the
    /// `row`th row, where rows that came or went before its own moved it:
    /// it is drawn beside that row from the next frame.
    pub(super) fn follow_row(&mut self, row: usize) {
        if let RowPlace::Listed(listed) = &mut self.row {
            listed.row = row;
        }
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
        let owner = cx.entity().downgrade();
        let close: CloseMenu = std::rc::Rc::new(move |window, cx| {
            let owner = owner.clone();
            window.defer(cx, move |window, cx| {
                let _ = owner.update(cx, |this, cx| dismiss(this, window, cx));
            });
        });
        // A press inside the menu must reach its item, not the overlay
        // behind it.
        let mut menu = div().on_any_mouse_down(|_, _, cx| cx.stop_propagation());
        if let Some(refuse) = self.paused {
            // Before the menu's own Enter, which does nothing with every
            // item disabled.
            menu = menu.capture_action(cx.listener(
                move |this, _: &gpui_kit::component::dialog::Confirm, window, cx| {
                    if this.integration_state.is_some() {
                        cx.stop_propagation();
                        refuse(this, window, cx);
                    }
                },
            ));
        }
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
                        row: self.row.clone(),
                        opened: self.opened,
                        close,
                        room: self.room,
                        menu: menu.child(self.menu.clone()).into_any_element(),
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
/// did the same to a pointer's. Its row is read as it is prepainted
/// ([`RowPlace`]), so a menu that follows its row stays beside it in the
/// frame a refresh moves the row. A row that leaves the part of its list
/// drawn, or moves where the menu, sized for the room where it opened, fits
/// neither below it nor wholly above it, closes the menu instead
/// (`close`), as a right-click's row that stops being drawn takes its menu
/// with it: pinned to the list's edge, or clamped over its own row, the
/// menu no longer lay beside the row it acts on.
struct RowMenu {
    row: RowPlace,
    /// Where the row was as the menu opened.
    opened: RowAnchor,
    room: Bounds<Pixels>,
    menu: AnyElement,
    close: CloseMenu,
}

/// Closes a row's menu as its dismissal does, returning focus where that
/// returns it, once the frame being drawn ends.
type CloseMenu = std::rc::Rc<dyn Fn(&mut Window, &mut App)>;

/// Whether a row's menu, opened beside its row at `opened`, leaves it now
/// that the row lies at `now`, drawn or not, its fill `size` tall in
/// `room`: the row is not drawn, being out of the part of its list shown,
/// or it moved, by more than rounding, where the fill, sized for the room
/// where it opened, fits neither below it nor wholly above it, where
/// [`RowMenu::origin`] would clamp it over the row. A menu whose row has not
/// moved stays where it opened, even fitting on neither side, as in a
/// window too short for it; once that row moves it closes too. A keyboard's
/// menu opens with its row in view ([`ListRow::reveal`]).
fn leaves_its_row(
    opened: RowAnchor,
    (now, drawn): (RowAnchor, bool),
    size: Size<Pixels>,
    room: Bounds<Pixels>,
) -> bool {
    let fits =
        now.below + size.height <= room.bottom() || now.top - MENU_EDGE - size.height >= room.top();
    !drawn || (now.moved_from(opened) && !fits)
}

impl RowMenu {
    /// Where the menu's fill, `size`, starts beside `row`.
    fn origin(&self, row: RowAnchor, size: Size<Pixels>) -> Point<Pixels> {
        let room = self.room;
        let y = if row.below + size.height <= room.bottom() {
            row.below
        } else {
            (row.top - MENU_EDGE - size.height).max(room.top())
        };
        let x = row.trailing.map_or(row.x, |right| right - size.width);
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
    /// Whether the menu is drawn in this frame, or closes.
    type PrepaintState = bool;

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
    ) -> bool {
        let laid = window.layout_bounds(*menu);
        let row = self.row.anchor();
        if leaves_its_row(self.opened, (row, self.row.drawn()), laid.size, self.room) {
            (self.close)(window, cx);
            return false;
        }
        let offset = self.origin(row, laid.size) - laid.origin;
        window.with_element_offset(offset, |window| self.menu.prepaint(window, cx));
        true
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut LayoutId,
        shown: &mut bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        if *shown {
            self.menu.paint(window, cx);
        }
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

/// Keeps the menus the kit places in the app above [`menu_foot`]: its
/// dropdowns ([`AppDropdown`], on the kit's dropdown menu and popover) and,
/// on Linux, the menu a right-click in an input or text area shows, as the
/// commit title's and description's, which the kit draws in the window (its
/// `NativeMenu` fallback); on macOS that menu is the system's own, which
/// this inset does not reach. The kit's window-bounded placement moves one
/// that would run lower up, as it moves one that would leave the window, so
/// a menu taller than the room below its button or pointer never covers the
/// status bar or comes nearer it than its margin. The app's row menus, a
/// right-click's ([`PointerMenu`]) and a keyboard's ([`KeyboardMenu`]),
/// place themselves beside their row inside [`menu_bounds`] and do not use
/// the kit's context menu, the other popup this inset reaches.
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
    /// The commit a commit's menu acts on ([`Self::follows_head_of`]).
    commit: Option<MenuCommit>,
}

/// What a menu that follows writes reads as it is built
/// ([`AppDropdown::follows_writes`], [`PointerMenu::follows_writes`],
/// [`KeyboardMenu`]):
/// whether a write runs, which alone refuses a project row's Open project
/// (`GitTurtle::operation_busy`), whether a write runs or its quiet reload
/// has yet to land, which refuses every other item that writes
/// (`GitTurtle::write_unsettled`), whether a merge, rebase, cherry-pick
/// or revert is paused, which disables a commit's Revert, Cherry-pick, Amend and Undo
/// (`GitTurtle::commit_menu_state`) and the Changes Actions menu's Save
/// changes to stash…, Amend and Undo, and, for a commit's menu alone
/// ([`AppDropdown::follows_head_of`], [`PointerMenu::follows_head_of`],
/// [`KeyboardMenu::following`]'s `commit`), whether its commit is HEAD,
/// which adds Amend and Undo.
/// The menu is built again when any of them changes; built again only for
/// the write, a menu open as a cherry-pick stopped on conflicts offered
/// those items once the write had ended, until it was opened again, one
/// built again only as the reload landed kept a project row's Open project
/// refused after the write, and a commit's menu kept Amend and Undo once a
/// `git reset` outside the app had moved HEAD off its commit. HEAD counts
/// for no other menu, which reads nothing it changes: counted for every
/// menu, a branch's, a worktree's, a project's or a working row's open menu
/// was built again whenever HEAD moved, as a commit made or a reset outside
/// the app moves it, its keyboard's cursor back on its first item.
#[derive(Clone, PartialEq, Eq)]
struct MenuInputs {
    running: bool,
    writing: bool,
    paused: bool,
    /// Whether a commit's menu's commit is HEAD; `None` for every other
    /// menu.
    head: Option<bool>,
}

/// The commit a commit's menu acts on, read as its object ID
/// ([`MenuInputs`]): the commit's own shared target, so a History row that
/// names it in every frame it draws allocates nothing for it.
pub(crate) type MenuCommit = std::sync::Arc<dyn AsRef<str>>;

impl MenuInputs {
    /// What a menu reads from `app`, a commit's menu also whether its
    /// `commit` is HEAD.
    fn of(app: &crate::GitTurtle, commit: Option<&MenuCommit>) -> Self {
        Self {
            running: app.operation_busy.is_some(),
            writing: app.write_unsettled(),
            paused: app.integration_state.is_some(),
            head: commit.map(|commit| {
                app.work_status
                    .as_ref()
                    .and_then(|status| status.head.as_deref())
                    == Some((**commit).as_ref())
            }),
        }
    }

    /// Whether a write started, or its reload or status read began to
    /// refuse, since `before`: what a menu waiting for its rows is built
    /// again for at once ([`PointerMenu::waits_while`]).
    fn starts_a_write(&self, before: &Self) -> bool {
        (self.running && !before.running) || (self.writing && !before.writing)
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

    /// The menu is `commit`'s, which adds Amend and Undo while the commit
    /// is HEAD: following writes ([`Self::follows_writes`]), it is built
    /// again as well once the commit becomes or stops being HEAD
    /// ([`MenuInputs`]).
    pub(crate) fn follows_head_of(mut self, commit: MenuCommit) -> Self {
        self.commit = Some(commit);
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
            commit: None,
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
        let (writes, commit) = (self.writes, self.commit);
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
                    let commit = commit.clone();
                    let mut inputs = MenuInputs::of(app.read(cx), commit.as_ref());
                    let open = open.clone();
                    cx.observe_in(&app, window, move |menu, app, window, cx| {
                        let now = MenuInputs::of(app.read(cx), commit.as_ref());
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
        // next: the row's own ID, so a row keyed by what it shows, as a
        // Working Changes file is by its path's bytes, keeps its open menu
        // as rows come or go around it. The menu's state lies at that ID and
        // the row's, drawn inside it, at that ID twice, so they never meet,
        // and the ID costs a clone of the row's rather than a new one each
        // frame.
        let caller = std::panic::Location::caller();
        let id = self
            .interactivity()
            .element_id
            .clone()
            .unwrap_or(ElementId::CodeLocation(*caller));
        PointerMenu {
            id,
            element: Some(self),
            build: std::rc::Rc::new(build),
            writes: None,
            commit: None,
            inputs: None,
            wait: None,
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
/// since the row it was placed beside may have moved. The open menu is kept
/// by the row's element ID, so it stays with what that ID names: a row keyed
/// by its path keeps its menu while rows come or go around it, the menu
/// drawn beside the row where it now lies ([`Pressed`]), and the menu goes
/// with the row when the row is no longer drawn.
pub(crate) struct PointerMenu<E> {
    id: ElementId,
    element: Option<E>,
    build: std::rc::Rc<BuildMenu>,
    /// The app whose writes the menu's items follow ([`Self::follows_writes`]).
    writes: Option<WeakEntity<crate::GitTurtle>>,
    /// The commit a commit's menu acts on ([`Self::follows_head_of`]).
    commit: Option<MenuCommit>,
    /// What the row builds its menu from in this frame ([`Self::built_from`]).
    inputs: Option<BuiltFrom>,
    /// Why that app is preparing the rows that replace the row's, or `None`
    /// while it is not ([`Self::waits_while`]).
    wait: Option<RowsWait>,
}

/// Why an app is preparing the rows that replace a row's, which the row's
/// menu states on each item it refuses meanwhile, or `None` while it is not
/// ([`PointerMenu::waits_while`]).
pub(crate) type RowsWait = fn(&crate::GitTurtle) -> Option<&'static str>;

/// What a row built its open menu from, compared by value.
#[derive(Clone)]
struct BuiltFrom {
    value: std::rc::Rc<dyn std::any::Any>,
    same: fn(&dyn std::any::Any, &dyn std::any::Any) -> bool,
}

impl BuiltFrom {
    fn same_as(&self, other: &Self) -> bool {
        (self.same)(&*self.value, &*other.value)
    }
}

fn same_value<T: PartialEq + 'static>(a: &dyn std::any::Any, b: &dyn std::any::Any) -> bool {
    a.downcast_ref::<T>() == b.downcast_ref::<T>()
}

impl<E> PointerMenu<E> {
    /// The row builds its menu from `inputs`, such as the file it shows: an
    /// open menu built from other inputs is built again in place from this
    /// frame's, so its items act on what the row now shows, as a write's
    /// start or end builds it again ([`Self::follows_writes`]). A menu kept
    /// as it opened offered to delete an untracked file that had since been
    /// added and edited, which the delete then refused. What a write's start
    /// or end changes stays out of `inputs`, which `follows_writes` follows
    /// already: in both, it built the menu twice for each. Built again, the
    /// menu keeps its keyboard's cursor where the keys had left it, as a
    /// keyboard's menu does ([`KeyboardMenu::rebuild`]); it lost it, since
    /// the kit's `PopupMenu::rebuild` cleared its selection.
    pub(crate) fn built_from<T: PartialEq + 'static>(mut self, inputs: T) -> Self {
        self.inputs = Some(BuiltFrom {
            value: std::rc::Rc::new(inputs),
            same: same_value::<T>,
        });
        self
    }

    /// Builds the open menu again, in place, when a write in `app` starts or
    /// ends, or an integration pauses or ends ([`MenuInputs`]), as
    /// [`AppDropdown::follows_writes`] does a button's: the menu read them as
    /// it opened and kept its items so.
    pub(crate) fn follows_writes(mut self, app: WeakEntity<crate::GitTurtle>) -> Self {
        self.writes = Some(app);
        self
    }

    /// The menu is `commit`'s, which adds Amend and Undo while the commit
    /// is HEAD, as [`AppDropdown::follows_head_of`] says of a button's.
    pub(crate) fn follows_head_of(mut self, commit: MenuCommit) -> Self {
        self.commit = Some(commit);
        self
    }

    /// The row is one of a list's rows that `wait` finds being replaced in
    /// the app the menu follows ([`Self::follows_writes`]), and says why, as
    /// Working Changes' rows are while those of a newer status or of the
    /// filter's query are prepared: the rows still drawn then are the
    /// previous ones, held in place, and a click or key on them acts on
    /// nothing. While `wait` finds them replaced, the open menu is built
    /// again neither from the row's inputs nor as a write's reload lands,
    /// but it is as a write starts or ends, which its builder answers by
    /// refusing its items, each saying why as it then stands, the write
    /// while it runs, as it does every build until the new rows are drawn;
    /// a menu built so is built again as the reason `wait` gives changes, as
    /// a newer status lands while the filter's rows are prepared, so each
    /// item says why as it now stands. Once the new rows are drawn, it is
    /// built again from them, once, where either changed meanwhile or it
    /// opened meanwhile. A Working row's menu built again as a Stage's
    /// status read landed, before the read's rows had, offered Delete
    /// untracked file… for a file the read had found added, as the previous
    /// rows still showed it, until the rows landed; one that waited as a
    /// write started kept offering the items that write refused until the
    /// rows landed; one that said "Filtering…" kept saying so after a
    /// status read had landed and the rows were that status's.
    pub(crate) fn waits_while(mut self, wait: RowsWait) -> Self {
        self.wait = Some(wait);
        self
    }

    /// Why `writes`, the app the menu follows, is preparing the rows that
    /// replace the row's, as `wait` reads it now, or `None` while it is not
    /// ([`Self::waits_while`]).
    fn wait(
        wait: Option<RowsWait>,
        writes: Option<&WeakEntity<crate::GitTurtle>>,
        cx: &App,
    ) -> Option<&'static str> {
        let app = writes.and_then(WeakEntity::upgrade)?;
        wait.and_then(|wait| wait(app.read(cx)))
    }

    fn shared(id: &GlobalElementId, window: &mut Window) -> SharedPointerMenu {
        window.with_optional_element_state::<SharedPointerMenu, _>(Some(id), |state, _| {
            let state = state.flatten().unwrap_or_default();
            (state.clone(), Some(state))
        })
    }
}

/// The menu a [`PointerMenu`] opened, kept from frame to frame while it is
/// open. Its dismissal lets it go: a closed menu that stayed here kept its
/// subscriptions, so it was built again on every write's start and end and
/// dismissed again on every resize, until its row was right-clicked again or
/// stopped being drawn.
#[derive(Default)]
struct PointerMenuState {
    menu: Option<Entity<PopupMenu>>,
    open: bool,
    /// Where the pointer opened it, in its row.
    pressed: Option<Pressed>,
    /// The row's builder the menu is built with, which a write's rebuild
    /// reads too, and what the row built it from.
    built: Option<(LatestBuild, Option<BuiltFrom>)>,
    /// Builds the open menu with that builder, as it opened.
    open_with: Option<std::rc::Rc<OpenMenu>>,
    /// The menu waited, its row's rows being prepared, to be built again
    /// for a write, or opened while they were: built again once they are
    /// drawn ([`PointerMenu::waits_while`]).
    waited: bool,
    /// Holds this state weakly: a strong hold was a cycle through the app's
    /// subscriber set that kept the menu after its row was gone.
    _dismiss: Option<Subscription>,
}

type OpenMenu = dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu;

/// The row's builder an open right-click menu is built with, replaced when
/// the row builds it again ([`PointerMenu::built_from`]).
type LatestBuild = std::rc::Rc<std::cell::RefCell<std::rc::Rc<BuildMenu>>>;

type SharedPointerMenu = std::rc::Rc<std::cell::RefCell<PointerMenuState>>;

/// Where a right-click opened its row's menu: the pointer's place from the
/// row's origin, and the menu's place in the frame being drawn, which the
/// row writes from its bounds as it is prepainted, before the menu, drawn
/// after the window's other elements, reads it ([`RowPlace::Pointer`]). A
/// row that rows coming or going above it move keeps its menu beside it;
/// placed once where the pointer pressed, the menu stayed there, beside
/// whatever row a refresh moved into that place.
#[derive(Clone)]
pub(super) struct Pressed {
    pointer: Point<Pixels>,
    place: std::rc::Rc<std::cell::Cell<RowAnchor>>,
    /// Where the menu went as it opened.
    opened: RowAnchor,
}

impl Pressed {
    /// A press at `pointer` on the row painted at `row`.
    fn at(pointer: Point<Pixels>, row: Bounds<Pixels>) -> Self {
        let opened = RowAnchor::pointer(pointer, row);
        Self {
            pointer: pointer - row.origin,
            place: std::rc::Rc::new(std::cell::Cell::new(opened)),
            opened,
        }
    }

    /// The row lies at `row` in this frame.
    fn place(&self, row: Bounds<Pixels>) {
        self.place
            .set(RowAnchor::pointer(row.origin + self.pointer, row));
    }
}

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

#[cfg(test)]
thread_local! {
    /// How often each right-click menu has been built, its opening among
    /// them, by the menu's entity ([`pointer_menu_builds`]).
    static POINTER_MENU_BUILDS: std::cell::RefCell<std::collections::HashMap<EntityId, usize>> =
        Default::default();
}

/// How often the right-click menu `menu` has been built, its opening among
/// them, for tests that count its rebuilds: one its row builds again is
/// notified in that frame's draw, which no observer of the menu hears.
#[cfg(test)]
pub(crate) fn pointer_menu_builds(menu: EntityId) -> usize {
    POINTER_MENU_BUILDS.with(|builds| builds.borrow().get(&menu).copied().unwrap_or_default())
}

/// The right-click menus recorded open, for tests that hold one weakly to
/// see that it is let go once it closes.
#[cfg(test)]
pub(crate) fn open_pointer_menus(cx: &App) -> Vec<WeakEntity<PopupMenu>> {
    cx.try_global::<OpenPointerMenus>()
        .map(|open| open.0.iter().map(|(menu, _)| menu.clone()).collect())
        .unwrap_or_default()
}

impl<E: ParentElement + IntoElement + 'static> IntoElement for PointerMenu<E> {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl<E: ParentElement + IntoElement + 'static> Element for PointerMenu<E> {
    /// The row, and where its open menu was pressed open.
    type RequestLayoutState = (AnyElement, Option<Pressed>);
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
    ) -> (LayoutId, (AnyElement, Option<Pressed>)) {
        let shared = Self::shared(id.expect("the menu's id"), window);
        let shown = {
            let state = shared.borrow();
            state
                .menu
                .clone()
                .zip(state.pressed.clone())
                .filter(|(menu, _)| state.open && !menu.read(cx).is_empty())
        };
        let pressed = shown.as_ref().map(|(_, pressed)| pressed.clone());
        let overlay = shown.map(|(menu, pressed)| {
            // Built from what the row showed when it was built; the row
            // shows something else now, so it is built again from that. Its
            // rows being replaced, it waits for the new ones.
            let preparing = Self::wait(self.wait, self.writes.as_ref(), cx).is_some();
            let rebuild = {
                let mut state = shared.borrow_mut();
                let state = &mut *state;
                match (&mut state.built, &state.open_with) {
                    (Some((build, then)), Some(open_with)) if !preparing => {
                        let changed = self.inputs.as_ref().is_some_and(|now| {
                            !then.as_ref().is_some_and(|then| then.same_as(now))
                        });
                        if changed || state.waited {
                            *build.borrow_mut() = self.build.clone();
                            if let Some(now) = &self.inputs {
                                *then = Some(now.clone());
                            }
                            state.waited = false;
                            Some(open_with.clone())
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            };
            if let Some(open_with) = rebuild {
                menu.update(cx, |menu, cx| {
                    menu.rebuild(window, cx, |menu, window, cx| open_with(menu, window, cx))
                });
            }
            // Focused, so that its keys reach it.
            if !menu.focus_handle(cx).contains_focused(window, cx) {
                menu.focus_handle(cx).focus(window, cx);
            }
            let close: CloseMenu = {
                let menu = menu.downgrade();
                std::rc::Rc::new(move |window, cx| {
                    let menu = menu.clone();
                    window.defer(cx, move |_, cx| {
                        let _ = menu.update(cx, |_, cx| cx.emit(gpui::DismissEvent));
                    });
                })
            };
            let viewport = window.viewport_size();
            deferred(
                anchored().position(Point::default()).child(
                    div()
                        .w(viewport.width)
                        .h(viewport.height)
                        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                        // The row's menu is open already, and it holds the
                        // keys: Shift-F10 or the Menu key in it opens no
                        // keyboard's menu beside it. Drawn inside the row,
                        // the menu passed both on to the list or tree, which
                        // opened its cursor row's menu and focused it; the
                        // next frame this menu took focus back and that one
                        // closed, both drawn for a frame.
                        .on_action(|_: &RowActions, _, _| {})
                        .on_action(|_: &ManageNavigation, _, _| {})
                        .child(RowMenu {
                            row: RowPlace::Pointer(pressed.place),
                            opened: pressed.opened,
                            room: menu_bounds(window),
                            menu: menu.into_any_element(),
                            close,
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
        (element.request_layout(window, cx), (element, pressed))
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (element, pressed): &mut (AnyElement, Option<Pressed>),
        window: &mut Window,
        cx: &mut App,
    ) -> Hitbox {
        // Where the row lies in this frame, which its menu, prepainted
        // after the window's other elements, is placed beside.
        if let Some(pressed) = pressed {
            pressed.place(bounds);
        }
        element.prepaint(window, cx);
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (element, _): &mut (AnyElement, Option<Pressed>),
        hitbox: &mut Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.paint(window, cx);
        let shared = Self::shared(id.expect("the menu's id"), window);
        let (build, writes, hitbox) = (self.build.clone(), self.writes.clone(), hitbox.clone());
        let (inputs, wait, commit) = (self.inputs.clone(), self.wait, self.commit.clone());
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
                state.pressed = Some(Pressed::at(event.position, hitbox.bounds));
            }
            // Built after this event, as the kit builds its own, with the
            // builder of the frame it was opened in, or a later one that the
            // row built it again with (`built_from`), which a write's
            // rebuild uses too.
            let build = std::rc::Rc::new(std::cell::RefCell::new(build.clone()));
            let (shared, writes, inputs) = (shared.clone(), writes.clone(), inputs.clone());
            let commit = commit.clone();
            window.defer(cx, move |window, cx| {
                let cap = row.cap(menu_bounds(window));
                let latest = build.clone();
                // Why its rows were being replaced as it was last built,
                // which its items, refused then, say, or `None` where they
                // were not ([`PointerMenu::waits_while`]).
                let stated = std::rc::Rc::new(std::cell::Cell::new(None));
                let open: std::rc::Rc<OpenMenu> = std::rc::Rc::new({
                    let (stated, writes) = (stated.clone(), writes.clone());
                    move |menu: PopupMenu, window: &mut Window, cx: &mut Context<PopupMenu>| {
                        #[cfg(test)]
                        POINTER_MENU_BUILDS.with(|builds| {
                            *builds.borrow_mut().entry(cx.entity_id()).or_default() += 1;
                        });
                        stated.set(Self::wait(wait, writes.as_ref(), cx));
                        let build = latest.borrow().clone();
                        let menu = build(menu, window, cx);
                        // No action context: an action item dispatches from
                        // the menu, drawn inside the row, and bubbles through
                        // the row's ancestors, as the kit's context menu has
                        // it; focus returns on dismissal below.
                        open_menu_in(menu, false, Some(cap), None, cx)
                    }
                });
                let open_with = open.clone();
                // Opened on rows being replaced, it is built again once the
                // new ones are drawn ([`PointerMenu::waits_while`]).
                let waited = Self::wait(wait, writes.as_ref(), cx).is_some();
                let waiting = std::rc::Rc::downgrade(&shared);
                let menu = PopupMenu::build(window, cx, move |menu, window, cx| {
                    let viewport = window.viewport_size();
                    cx.observe_window_bounds(window, move |_, window, cx| {
                        if window.viewport_size() != viewport {
                            cx.emit(gpui::DismissEvent);
                        }
                    })
                    .detach();
                    if let Some(app) = writes.as_ref().and_then(WeakEntity::upgrade) {
                        let mut inputs = MenuInputs::of(app.read(cx), commit.as_ref());
                        let open = open.clone();
                        cx.observe_in(&app, window, move |menu, app, window, cx| {
                            let now = MenuInputs::of(app.read(cx), commit.as_ref());
                            let reason = wait.and_then(|wait| wait(app.read(cx)));
                            // Its items, refused while its rows are
                            // replaced, say why as they were built, which
                            // can change with no write starting or ending,
                            // as a newer status lands while the filter's
                            // rows are prepared and the rows are that
                            // status's: "Filtering…" stayed until they
                            // landed.
                            let reworded = reason.is_some()
                                && stated.get().is_some_and(|stated| Some(stated) != reason);
                            if now == inputs && !reworded {
                                return;
                            }
                            // A write's start or end changes why the items
                            // of a menu waiting for its rows are refused:
                            // that write while it runs.
                            let restated = reworded
                                || now.starts_a_write(&inputs)
                                || (inputs.running && !now.running);
                            inputs = now;
                            // Its rows being replaced, the builder would read
                            // the rows the window no longer acts on: the
                            // rows' landing builds it again. A write that
                            // starts or ends meanwhile builds it now, its
                            // builder refusing every item while the rows are
                            // prepared, so it never offers what that write
                            // refuses, and each item says why as it then
                            // stands, as a menu whose reason changed is.
                            if reason.is_some() {
                                if let Some(state) = waiting.upgrade() {
                                    state.borrow_mut().waited = true;
                                }
                                if !restated {
                                    return;
                                }
                            }
                            menu.rebuild(window, cx, |menu, window, cx| open(menu, window, cx));
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
                        // Closed, the menu is let go, with this subscription
                        // and its write and resize observers, once this
                        // handler, which is that subscription, has returned.
                        let shared = shared.clone();
                        window.defer(cx, move |_, _| {
                            let Some(shared) = shared.upgrade() else {
                                return;
                            };
                            let mut state = shared.borrow_mut();
                            if state
                                .menu
                                .as_ref()
                                .is_some_and(|menu| menu.entity_id() == closed)
                            {
                                state.menu = None;
                                state._dismiss = None;
                                state.built = None;
                                state.open_with = None;
                            }
                        });
                        window.refresh();
                    }
                });
                let mut state = shared.borrow_mut();
                state.menu = Some(menu);
                state._dismiss = Some(dismiss);
                state.built = Some((build, inputs));
                state.open_with = Some(open_with);
                state.waited = waited;
                window.refresh();
            });
        });
    }
}

/// The most an accessible name or value carries of a long text, 16 KiB.
const ACCESSIBLE_TEXT_BYTES: usize = 16 * 1024;

/// `text` as an accessible name or value carries it: whole up to 16 KiB,
/// otherwise cut between characters to end in an ellipsis within that
/// bound. Text it borrows is copied only as far as it keeps it, and text it
/// owns is kept without a copy. A failure's report, which Git's output can
/// make megabytes long, or a model's details would otherwise reach
/// assistive technology whole in every frame that draws them.
pub(crate) fn bounded_accessible_text<'a>(
    text: impl Into<std::borrow::Cow<'a, str>>,
) -> SharedString {
    let text = text.into();
    if text.len() <= ACCESSIBLE_TEXT_BYTES {
        return SharedString::from(text.into_owned());
    }
    let end = text.floor_char_boundary(ACCESSIBLE_TEXT_BYTES - '…'.len_utf8());
    format!("{}…", &text[..end]).into()
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

    /// An alert that says `text`, announced at once: a failure, a refusal
    /// or invalid input, `text` its name and its value together. AccessKit's
    /// adapters announce a live node as it is added by different
    /// properties, AT-SPI (Orca) its name and macOS its value, so an alert
    /// named alone could go unheard on macOS, as the narrow rail's refusal
    /// and the copy announcement set both for. Every assertive alert is
    /// written through this ([`refusal_line`] among them); tests read its
    /// node through [`recorded`] and [`written::get`].
    fn alert(self, text: impl Into<SharedString>) -> Self {
        let text = text.into();
        self.role(Role::Alert)
            .aria_label(text.clone())
            .aria_value(text)
            .assertive()
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

/// A form's line saying why its last review was refused or could not be
/// prepared, `text`: an alert, announced at once as a whole, whether it
/// appears in the open form or is there as the form opens again. Such a form
/// closes as its review is asked for and opens again with the line, and
/// `opening`, the form's count of its openings, gives the line a fresh node
/// each time: opened again within a frame, as a refusal while an
/// integration is paused opens it, the form kept the line's node, which an
/// assistive technology announces only as it is added or its text changes,
/// so the same reason again went unannounced. The text is the node's name
/// and its value, as the narrow rail's refusal has it: AccessKit's adapters
/// announce a live node as it is added by different properties, AT-SPI
/// (Orca) its name and macOS its value. `style` lays the line out. Tests
/// find the line by its debug selector, `id`, what it asks to announce
/// under `<id>-<opening>` ([`written::announced_from`]) and its node there
/// ([`written::get`]).
pub(crate) fn refusal_line(
    id: &'static str,
    opening: usize,
    text: impl Into<SharedString>,
    style: impl FnOnce(Stateful<Div>) -> Stateful<Div>,
) -> AnyElement {
    let text = text.into();
    recorded(style(
        div()
            .id((id, opening))
            .debug_selector(move || id.to_owned())
            .alert(text.clone())
            .child(text),
    ))
    .into_any_element()
}

/// `element`, a container that scrolls or clips what it holds, as the
/// accessibility node AccessKit expects of one: it clips its children, and its
/// bounds are the part of it that its ancestors leave visible, or, where they
/// leave none of it, one pixel beyond a corner of it, outside its extent
/// along both axes and outside what they leave, so its children lie outside
/// it too, wherever it scrolls them (`drawn_part`).
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
/// each child is measured against what is drawn. Reported at its layout, a
/// viewport the outer one draws none of, which that filter keeps as the
/// child past an edge, kept every child inside it, so the GitHub panel's
/// pull requests, past its body's fold at 18 pt in a 461 × 490 window, could
/// be reached and activated undrawn. A container without a role
/// of its own becomes a scroll view; one with a role, such as a list box,
/// keeps it. Children keep their own bounds, as AccessKit and the platforms
/// report a partly visible control whole, so each node the container holds
/// directly, which the filter measures against it, must keep within its
/// extent across the axis it scrolls along, within its width for a list, or
/// be wrapped itself: a row wider than its list is wrapped, and reports the
/// part the list draws or, drawn nowhere, a pixel beyond its own corner.
/// History's commit rows are wrapped too, though they keep within their
/// list, as wide as the commit table: that table runs on sideways past its
/// viewport, so each row reports the part the window draws, as the list
/// does. A list the window draws none of reports a pixel beyond a corner
/// that lies outside every row within its width ([`aside`]); an unwrapped
/// row running on past the list's right, scrolled across its top or bottom,
/// would meet the pixel beyond that right corner, which a list just past
/// the right of what is drawn reports, and the filter would keep that row
/// and its neighbours, showing on AT-SPI, though nothing of them is drawn.
/// The width walk's
/// `every_list_keeps_its_rows_within_its_width_or_wraps_them` tests check
/// the rows of the app's scrolling lists for it. The wrapper styles, holds
/// and handles input as the element it
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
        let visible = drawn_part(bounds, window, Undrawn::Edge(Clipping::innermost()));
        self.visible = Some(visible);
        #[cfg(test)]
        let written = written::open(self, Some(visible), physical(bounds, window.scale_factor()));
        // GPUI writes a node only for an element with an ID.
        let _clipping = self.inner.id().map(|_| Clipping::open(visible));
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
        let written = written::open(self, None, physical(bounds, window.scale_factor()));
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
///
/// Outside tests it costs nothing at run time, so it needs no `cfg(test)`
/// twin: it is its element, transparent, so the same size; it reports the
/// element's ID, so GPUI keeps one ID, one element state and one node for
/// both; it returns the element's own layout node, adding none; it adds no
/// allocation or lookup; and its methods only forward, the recording in
/// `prepaint`, its one addition, being `cfg(test)`. A product build compiles
/// one forwarding impl per wrapped type, nothing per frame.
#[repr(transparent)]
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
        let written = written::open(
            self,
            Some(physical(bounds, window.scale_factor())),
            physical(bounds, window.scale_factor()),
        );
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
/// ran 56 px past its document's foot over "Changed files". Text the window
/// draws none of reports where it lies (`drawn_part`). The node stays where
/// it is in the tree, so a document read as a whole still reads it.
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
        let visible = drawn_part(bounds, window, Undrawn::Layout);
        self.visible = Some(visible);
        #[cfg(test)]
        let written = written::open(self, Some(visible), physical(bounds, window.scale_factor()));
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

/// What [`drawn_part`] reports for an element its ancestors leave none of
/// visible.
#[derive(Clone, Copy)]
enum Undrawn {
    /// Text: its whole layout, where it lies.
    Layout,
    /// A clipping container: a pixel beyond one of its corners, outside its
    /// layout along both axes and outside what is drawn, which leaves its
    /// children outside it ([`aside`]), and, wherever a corner allows,
    /// outside the bounds its nearest clipping ancestor reported, if any.
    Edge(Option<gpui::accesskit::Rect>),
}

thread_local! {
    /// The bounds each [`ClipsChildren`] that writes a node reported as it
    /// prepaints what it holds, innermost last: where a container nested in
    /// one reports its own, such as a wrapped row in a list the window draws
    /// none of, or one under a drawn list's border, it takes a corner clear
    /// of the bounds around it wherever one is ([`aside`]). GPUI
    /// prepaints a window's elements one inside another, on this thread.
    static CLIPS: std::cell::RefCell<Vec<gpui::accesskit::Rect>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// A [`ClipsChildren`]'s entry on [`CLIPS`], removed when it has prepainted
/// what it holds.
struct Clipping;

impl Clipping {
    fn open(bounds: gpui::accesskit::Rect) -> Self {
        CLIPS.with(|clips| clips.borrow_mut().push(bounds));
        Self
    }

    /// The bounds the innermost open [`ClipsChildren`] reported.
    fn innermost() -> Option<gpui::accesskit::Rect> {
        CLIPS.with(|clips| clips.borrow().last().copied())
    }
}

impl Drop for Clipping {
    fn drop(&mut self) {
        CLIPS.with(|clips| clips.borrow_mut().pop());
    }
}

/// The part of `bounds`, an element's layout as it prepaints, that its
/// ancestors leave visible, in the physical window pixels of AccessKit's
/// bounds. The content mask is what the ancestors let through; GPUI scales
/// layout bounds to a node's bounds the same way.
///
/// Where the mask shows less than a pixel across of the element, but some of
/// it, the element reports one pixel inside the mask at the edge it crosses:
/// its whole layout there, as for one the mask leaves out, ran on over the
/// controls past that edge, the GitHub panel's notice half a pixel above its
/// body's fold over Back to repository, while AT-SPI reported it showing.
///
/// An element the mask leaves out reports as `undrawn` says. Text keeps its
/// whole layout: that is where it lies. AccessKit's filter keeps the first
/// child past each edge of a clipping node, for an assistive technology to
/// move into, and every other control it keeps there reports its layout, as
/// GPUI writes it. Such text once reported an empty rectangle on the mask's
/// edge nearest to it, so the GitHub panel's notice, below its body's fold
/// at 18 pt in a 461 × 490 window, reached AT-SPI 0 px tall on the body's
/// bottom edge (y 557 in physical pixels). A clipping container reports a
/// pixel beyond a corner of itself instead ([`clips_children`], [`aside`]):
/// AccessKit's filter measures each child against its clipping parent
/// alone, so a list the window draws none of, reported at its layout, kept
/// every row inside it, and the GitHub panel's pull requests, past its
/// body's fold at 18 pt in a 461 × 490 window, were reached, showing,
/// wherever the list lay.
fn drawn_part(bounds: Bounds<Pixels>, window: &Window, undrawn: Undrawn) -> gpui::accesskit::Rect {
    let scale = window.scale_factor();
    drawn_rect(
        physical(bounds, scale),
        physical(window.content_mask().bounds, scale),
        f64::from(scale),
        undrawn,
    )
}

/// [`drawn_part`] of `laid_out` in `mask`, in physical pixels `pixel` wide.
fn drawn_rect(
    laid_out: gpui::accesskit::Rect,
    mask: gpui::accesskit::Rect,
    pixel: f64,
    undrawn: Undrawn,
) -> gpui::accesskit::Rect {
    let x = drawn_span(laid_out.x0, laid_out.x1, mask.x0, mask.x1, pixel);
    let y = drawn_span(laid_out.y0, laid_out.y1, mask.y0, mask.y1, pixel);
    match (x, y, undrawn) {
        (Some((x0, x1)), Some((y0, y1)), _) => gpui::accesskit::Rect { x0, y0, x1, y1 },
        (_, _, Undrawn::Layout) => laid_out,
        (_, _, Undrawn::Edge(clip)) => aside(laid_out, mask, pixel, clip),
    }
}

/// The part of `start..end` that `from..to` shows along one axis: the
/// overlap, at least `pixel` long, or, where it is shorter but not empty,
/// `pixel` inside `from..to` at the edge the element crosses (its whole
/// extent where it crosses none, being shorter than `pixel` itself); `None`
/// where nothing of it shows.
fn drawn_span(start: f64, end: f64, from: f64, to: f64, pixel: f64) -> Option<(f64, f64)> {
    let (low, high) = (start.max(from), end.min(to));
    if high <= low {
        None
    } else if high - low >= pixel {
        Some((low, high))
    } else if end > to {
        Some((to - pixel, to))
    } else if start < from {
        Some((from, from + pixel))
    } else {
        Some((low, high))
    }
}

/// Where a clipping container the mask shows none of reports itself, in
/// physical pixels `pixel` wide: one pixel beyond one of its corners, outside
/// its extent along both axes, the first of its top left, bottom left, top
/// right and bottom right corners whose pixel lies clear of `mask` and of
/// `clip`, the bounds its nearest clipping ancestor reported, or the top left
/// where none does.
///
/// Being outside the container along both axes, the pixel lies outside each
/// child that keeps within the container's extent along one of them, and
/// AccessKit's filter, measuring each child the container holds against it,
/// keeps none of those. That is all it guarantees. A container scrolls what
/// it holds past its edges along one axis and keeps it within its extent
/// along the other only where every child fits across it: a child that runs
/// on past the container's right and is scrolled across its top or bottom
/// meets the pixel beyond that right corner, which a container within a
/// pixel past the mask's right takes, and the filter keeps that child and
/// its neighbours. So a row wider than its list is wrapped itself
/// ([`clips_children`]) and reports its own bounds. The left corners come
/// first because content runs on past a container's right, as History's
/// commit list runs on past the viewport that scrolls it sideways, and lies
/// past its left only where the container scrolls sideways, within its
/// height.
///
/// The top left corner meets `mask` where the container lies within a pixel
/// past the mask's right or bottom, so a later one, on the side facing away
/// from it, is taken. It meets `clip` where a wrapped row starts at the
/// corner of a list the window draws none of, whose own pixel lies there, or
/// lies in the ancestor's border: GPUI draws a container's children inside
/// its bounds less its border, so `clip` reaches past `mask`, and a wrapped
/// row with only its head under a drawn list's 1 px bottom border is drawn
/// nowhere, while the pixel beyond its top left corner lies in the list's
/// left border, inside the list; it takes its bottom left, below the list.
/// Where every corner's pixel meets `mask` or `clip`, the top left one is
/// reported anyway, inside what is drawn or the bounds of the clipping
/// ancestor, which the filter then measures the container inside: one that
/// collapses to no height inside the mask, as a `flex_1().min_h_0()` list
/// can, or one that an element clipping without a node of its own, such as
/// an `overflow_hidden` div no wrapper reports, leaves out inside the part
/// of the ancestor that is drawn. What the container holds still lies
/// outside that pixel, as above.
///
/// It was a strip a pixel across along the container's own extent, beside
/// its leading edge across the axis the mask left it out along (its left
/// for a container past a fold above or below, its top for one past a
/// side), which met the children scrolled past that edge: a list scrolled
/// down that lay wholly beside the drawn area reported a strip above its
/// top, which the row scrolled to end on that top crossed, and the filter
/// kept that row and the rows before and after it; a row of items scrolled
/// sideways past a fold above or below met the item scrolled to end on its
/// left the same way. Before that, it lay along the scroll axis, against the
/// container's edge nearest the mask, where the children a scrolling
/// container holds past that edge run on (a list above a body's fold, not
/// scrolled to its end) and, with less than a pixel between them, against
/// the mask's edge inside the container: a list at y 0–100 over a body from
/// 100, at scale 1, reported y 99–100, and the filter, keeping that list as
/// the child past the fold, kept the row ending there and the one before it,
/// reachable and activatable though nothing of them is drawn. Against the
/// far edge instead, it met the entries the Reflog's list runs on with past
/// its foot, at the window height where that list starts on the content's
/// fold.
fn aside(
    laid_out: gpui::accesskit::Rect,
    mask: gpui::accesskit::Rect,
    pixel: f64,
    clip: Option<gpui::accesskit::Rect>,
) -> gpui::accesskit::Rect {
    use gpui::accesskit::Rect;
    let (left, right) = (
        (laid_out.x0 - pixel, laid_out.x0),
        (laid_out.x1, laid_out.x1 + pixel),
    );
    let (top, bottom) = (
        (laid_out.y0 - pixel, laid_out.y0),
        (laid_out.y1, laid_out.y1 + pixel),
    );
    let corners = [(left, top), (left, bottom), (right, top), (right, bottom)];
    let corner = |((x0, x1), (y0, y1)): ((f64, f64), (f64, f64))| Rect { x0, y0, x1, y1 };
    let clear = |beyond: &Rect| {
        beyond.intersect(mask).is_empty()
            && clip.is_none_or(|clip| beyond.intersect(clip).is_empty())
    };
    corners
        .into_iter()
        .map(corner)
        .find(clear)
        .unwrap_or_else(|| corner(corners[0]))
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
        /// The element's layout bounds as it prepainted, in the physical
        /// window pixels of AccessKit's bounds, whatever bounds the node
        /// reports; `None` for a node recorded as it is built ([`built`]).
        pub layout: Option<gpui::accesskit::Rect>,
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

    /// Records the node `element` writes, at `bounds` if given, laid out at
    /// `layout`, as the element starts to prepaint what it holds. GPUI
    /// writes a node only for an element with an ID; so does this.
    pub(super) fn open<E: Element>(
        element: &E,
        bounds: Option<gpui::accesskit::Rect>,
        layout: gpui::accesskit::Rect,
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
                    layout: Some(layout),
                },
            )
        });
        OPEN.with(|open| open.borrow_mut().push(id.clone()));
        Some(id)
    }

    /// Records the node an element GPUI writes itself, such as a dialog's
    /// title or the window's alert, as it is built: without bounds or the
    /// wrapper around it, and live as the element asked to be.
    pub(crate) fn built<E: Element>(element: &E) {
        let (Some(id), Some(role)) = (element.id(), element.a11y_role()) else {
            return;
        };
        let mut node = gpui::accesskit::Node::new(role);
        element.write_a11y_info(&mut node);
        if let Some(announced) = announced(&id.to_string()) {
            announced.write(&mut node);
        }
        let order = NEXT.with(|next| next.replace(next.get() + 1));
        NODES.with(|nodes| {
            nodes.borrow_mut().insert(
                id.to_string(),
                Written {
                    node,
                    parent: None,
                    order,
                    layout: None,
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

    /// Every live region [`announced`] would return whose element ID starts
    /// with `prefix`, by element ID in order: a line with a fresh ID for
    /// each opening of its form ([`super::refusal_line`]) is found by its
    /// stem.
    pub(crate) fn announced_from(prefix: &str) -> Vec<(String, Announced)> {
        LIVE.with(|live| {
            let mut found: Vec<(String, Announced)> = live
                .borrow()
                .iter()
                .filter(|(id, _)| id.starts_with(prefix))
                .map(|(id, announced)| (id.clone(), *announced))
                .collect();
            found.sort_by(|a, b| a.0.cmp(&b.0));
            found
        })
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

/// Whether `bounds` is one pixel `pixel` across beyond a corner of `layout`,
/// outside it along both axes, as a container the window draws none of
/// reports itself ([`clips_children`]).
#[cfg(test)]
pub(crate) fn beyond_a_corner(
    bounds: gpui::accesskit::Rect,
    layout: gpui::accesskit::Rect,
    pixel: f64,
) -> bool {
    let at = |edge: f64, to: f64| (edge - to).abs() < 0.01;
    at(bounds.x1 - bounds.x0, pixel)
        && at(bounds.y1 - bounds.y0, pixel)
        && (at(bounds.x1, layout.x0) || at(bounds.x0, layout.x1))
        && (at(bounds.y1, layout.y0) || at(bounds.y0, layout.y1))
}

/// The rows among `rows`, by debug selector, that the last frame laid out
/// past a side of `list`, a [`clips_children`] container by element ID,
/// without a wrapper of their own, and how many of `rows` it laid out at
/// all. A list the window draws none of reports a pixel beyond a corner,
/// which lies outside each row within the list's width ([`aside`]); one
/// running on past a side, scrolled across the top or bottom, meets the
/// pixel beyond that corner, so AccessKit's filter would keep it and its
/// neighbours, though nothing of them is drawn. A row is wrapped where a
/// clipping wrapper inside `list` was laid out at its bounds.
#[cfg(test)]
pub(crate) fn rows_past_their_list(
    cx: &mut gpui::VisualTestContext,
    list: &str,
    rows: impl IntoIterator<Item = String>,
) -> (usize, Vec<String>) {
    let Some(layout) = written::get(list).and_then(|written| written.layout) else {
        return (0, Vec::new());
    };
    let wrapped: Vec<gpui::accesskit::Rect> = written::inside(list)
        .iter()
        .filter_map(|id| written::get(id))
        .filter(|written| written.node.clips_children())
        .filter_map(|written| written.layout)
        .collect();
    let scale = cx.update(|window, _| window.scale_factor());
    let at = |a: f64, b: f64| (a - b).abs() < 0.01;
    let (mut laid_out, mut past) = (0, Vec::new());
    for selector in rows {
        let Some(bounds) = cx.debug_bounds(selector.clone().leak()) else {
            continue;
        };
        laid_out += 1;
        let row = physical(bounds, scale);
        let within = row.x0 >= layout.x0 - 0.01 && row.x1 <= layout.x1 + 0.01;
        let is_wrapped = wrapped.iter().any(|wrapper| {
            at(wrapper.x0, row.x0)
                && at(wrapper.y0, row.y0)
                && at(wrapper.x1, row.x1)
                && at(wrapper.y1, row.y1)
        });
        if !within && !is_wrapped {
            past.push(format!(
                "{selector} at {row:?} runs past a side of {list} at {layout:?}, unwrapped"
            ));
        }
    }
    (laid_out, past)
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
    /// How the adapters announce it, its own or the nearest node's around
    /// it that says (`accesskit_consumer::Node::live`).
    pub live: gpui::accesskit::Live,
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
            live: node.live(),
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

    /// A list the body draws none of, ending exactly on the body's top fold
    /// or starting exactly on its bottom one, reports a pixel beyond its top
    /// left corner, outside itself and the body, so AccessKit's filter, which
    /// keeps the list as the child past the fold beside a line the body
    /// draws, reaches none of its rows: not the rows it runs on with past its
    /// foot, nor its wrapped rows, which clip what they hold and report a
    /// pixel of their own, clear of the list's: the first row, whose top left
    /// corner is the list's, beyond its bottom left one.
    /// With no gap at all between the list and the
    /// body, the list's pixel lay against the fold, inside the list: y 99–100
    /// for a list at y 0–100 over a body from 100, at scale 1, so the filter
    /// kept the row ending there and the one before it, reachable and
    /// activatable though nothing of them is drawn; below, the pixel covered
    /// the first row. Against the list's far edge instead, it met the rows
    /// the list runs on with past its foot.
    #[gpui::test]
    fn a_list_on_the_fold_reaches_none_of_its_rows(cx: &mut TestAppContext) {
        const ROWS: usize = 8;
        struct Fold {
            below: bool,
        }
        impl Render for Fold {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                // Eight rows of 30 px in a list 100 px tall, which runs on
                // past its foot, the fourth across it; every other row,
                // from the first, is wrapped, as History's are, and reports
                // what the list draws of it.
                let rows = (0..ROWS).map(|index| {
                    let row = div()
                        .id(ElementId::Name(format!("fold-row-{index}").into()))
                        .debug_selector(move || format!("fold-row-{index}"))
                        .role(Role::Button)
                        .aria_label(format!("Row {index}"))
                        .h(px(30.))
                        .flex_shrink_0();
                    if index % 2 == 0 {
                        clips_children(row).into_any_element()
                    } else {
                        row.into_any_element()
                    }
                });
                // Out of flow, ending on the body's top or starting on its
                // bottom.
                let list = clips_children(
                    div()
                        .id("fold-list")
                        .debug_selector(|| "fold-list".into())
                        .role(Role::ListBox)
                        .aria_label("Rows")
                        .absolute()
                        .left_0()
                        .top(if self.below { px(200.) } else { px(-100.) })
                        .w(px(100.))
                        .h(px(100.))
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .children(rows),
                )
                .into_any_element();
                let line = text("fold-line", "Drawn").h(px(20.)).into_any_element();
                // The tree holds the list beside the line, on the side of
                // the fold it lies past.
                let children = if self.below {
                    [line, list]
                } else {
                    [list, line]
                };
                div().size_full().pt(px(100.)).child(clips_children(
                    div()
                        .id("fold-body")
                        .debug_selector(|| "fold-body".into())
                        .aria_label("Body")
                        .relative()
                        .w(px(100.))
                        .h(px(200.))
                        .overflow_hidden()
                        .children(children),
                ))
            }
        }

        for below in [false, true] {
            let (_view, cx) = cx.add_window_view(move |_, _| Fold { below });
            written::forget();
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let scale = cx.update(|window, _| f64::from(window.scale_factor()));
            let physical = |bounds: Bounds<Pixels>| super::physical(bounds, scale as f32);
            let at = if below { "below" } else { "above" };
            let body = physical(cx.debug_bounds("fold-body").expect("the body"));
            let layout = physical(cx.debug_bounds("fold-list").expect("the list"));
            assert_eq!(
                if below { layout.y0 } else { layout.y1 },
                if below { body.y1 } else { body.y0 },
                "{at}: the list {layout:?} lies on the body's {body:?} fold"
            );
            let last = physical(cx.debug_bounds("fold-row-7").expect("the last row"));
            assert!(
                last.y0 >= layout.y1,
                "{at}: the list runs on past its foot to {last:?}"
            );
            let reported = clipping_node("fold-list")
                .and_then(|node| node.bounds())
                .expect("the list clips its rows");

            // The body holds the line and the list, in the order it draws
            // them, and the list its rows.
            let line = written::get("fold-line").expect("the line").node;
            let assembly = written::Assembly::default().written("fold-body", None);
            let mut assembly = if below {
                assembly
                    .node("fold-line", line, Some("fold-body"))
                    .written("fold-list", None)
            } else {
                assembly
                    .written("fold-list", None)
                    .node("fold-line", line, Some("fold-body"))
            };
            for index in 0..ROWS {
                let key = format!("fold-row-{index}");
                assembly = if index % 2 == 0 {
                    assembly.written(&key, None)
                } else {
                    let mut row = gpui::accesskit::Node::new(Role::Button);
                    row.set_label(format!("Row {index}"));
                    row.set_bounds(physical(
                        cx.debug_bounds(key.clone().leak()).expect("the row"),
                    ));
                    assembly.node(&key, row, Some("fold-list"))
                };
            }
            let window = cx.update(|window, _| {
                physical(Bounds::new(point(px(0.), px(0.)), window.viewport_size()))
            });
            let found = reachable(assembly.tree(window, None));
            assert!(
                found
                    .iter()
                    .any(|node| node.label.as_deref() == Some("Rows")),
                "{at}: AccessKit's filter keeps the list as the child past the fold: {found:?}"
            );
            let rows: Vec<_> = found
                .iter()
                .filter(|node| node.role == Role::Button)
                .collect();
            assert!(
                rows.is_empty(),
                "{at}: the body draws no row, yet {rows:?} are reached in the list's {reported:?}"
            );
            assert!(
                beyond_a_corner(reported, layout, scale)
                    && (reported.x1 - layout.x0).abs() < 0.01
                    && (reported.y1 - layout.y0).abs() < 0.01
                    && reported.intersect(body).is_empty(),
                "{at}: the list reports {reported:?}, not a pixel beyond the top left corner of \
                 its {layout:?}, outside the body's {body:?}"
            );
            for index in (0..ROWS).step_by(2) {
                let row = clipping_node(&format!("fold-row-{index}"))
                    .and_then(|node| node.bounds())
                    .expect("the wrapped row clips what it holds");
                assert!(
                    row.intersect(reported).is_empty(),
                    "{at}: row {index} reports {row:?}, clear of the list's {reported:?}"
                );
            }
        }
    }

    /// [`drawn_rect`] reports a container the mask shows none of as a pixel
    /// beyond one of its corners, outside its extent along both axes, at
    /// either scale: its top left, or, where that would meet the mask or the
    /// bounds its nearest clipping ancestor reported, the first of its bottom
    /// left, top right and bottom right that meets neither.
    #[test]
    fn an_undrawn_container_reports_a_pixel_beyond_a_corner() {
        use gpui::accesskit::Rect;
        let rect = |x0, y0, x1, y1| Rect { x0, y0, x1, y1 };
        let mask = rect(0., 200., 600., 600.);
        for (pixel, layout, clip, expected) in [
            // On the fold above, at scale 1 and 2, and less than a pixel
            // or more than one above it: beyond its top left corner.
            (
                1.,
                rect(0., 100., 100., 200.),
                None,
                rect(-1., 99., 0., 100.),
            ),
            (2., rect(10., 0., 110., 200.), None, rect(8., -2., 10., 0.)),
            (2., rect(10., 0., 110., 199.), None, rect(8., -2., 10., 0.)),
            (1., rect(0., 0., 100., 190.), None, rect(-1., -1., 0., 0.)),
            // On the fold below, and less than a pixel below it, where the
            // top left would lie in the mask: beyond its bottom left corner.
            // More than a pixel below, or at the mask's left edge: its top
            // left.
            (
                1.,
                rect(10., 600., 110., 700.),
                None,
                rect(9., 700., 10., 701.),
            ),
            (
                2.,
                rect(10., 601., 110., 800.),
                None,
                rect(8., 800., 10., 802.),
            ),
            (
                1.,
                rect(10., 700., 110., 800.),
                None,
                rect(9., 699., 10., 700.),
            ),
            (
                1.,
                rect(0., 600., 100., 700.),
                None,
                rect(-1., 599., 0., 600.),
            ),
            // Past the right side, on its fold, where both left corners lie
            // in the mask: beyond its top right corner. Beyond that fold, or
            // past the left side: its top left.
            (
                1.,
                rect(600., 300., 700., 400.),
                None,
                rect(700., 299., 701., 300.),
            ),
            (
                1.,
                rect(700., 300., 800., 400.),
                None,
                rect(699., 299., 700., 300.),
            ),
            (
                1.,
                rect(-100., 300., 0., 400.),
                None,
                rect(-101., 299., -100., 300.),
            ),
            // Starting at the top left corner of a list on the fold above,
            // whose pixel lies beyond that corner: beyond its bottom left.
            (
                1.,
                rect(0., 100., 100., 130.),
                Some(rect(-1., 99., 0., 100.)),
                rect(-1., 130., 0., 131.),
            ),
            // With its head half a pixel under a drawn list's 1 px bottom
            // border, where the pixel beyond its top left corner would lie in
            // the list's left border, which the list's bounds hold and the
            // mask leaves out: beyond its bottom left, below the list.
            (
                1.,
                rect(0., 600.5, 600., 630.),
                Some(rect(-1., 199., 601., 601.)),
                rect(-1., 630., 0., 631.),
            ),
            // On the right fold, where the top right corner would meet the
            // clip: beyond its bottom right.
            (
                1.,
                rect(600., 300., 700., 400.),
                Some(rect(695., 290., 710., 310.)),
                rect(700., 400., 701., 401.),
            ),
        ] {
            let drawn = drawn_rect(layout, mask, pixel, Undrawn::Edge(clip));
            assert_eq!(
                drawn, expected,
                "{layout:?} in {mask:?} at {pixel} px a pixel, in {clip:?}"
            );
            assert!(
                drawn.intersect(layout).is_empty()
                    && drawn.intersect(mask).is_empty()
                    && clip.is_none_or(|clip| drawn.intersect(clip).is_empty()),
                "{drawn:?} lies outside {layout:?}, {mask:?} and {clip:?}"
            );
        }

        // Collapsed to no height inside the mask, as a `flex_1().min_h_0()`
        // list can be, with every corner's pixel in the mask: beyond its top
        // left corner all the same, outside its extent but inside the mask.
        let collapsed = rect(10., 300., 110., 300.);
        let drawn = drawn_rect(collapsed, mask, 1., Undrawn::Edge(None));
        assert_eq!(
            drawn,
            rect(9., 299., 10., 300.),
            "{collapsed:?} in {mask:?}"
        );
        assert!(
            !drawn.intersect(mask).is_empty(),
            "{drawn:?} lies in {mask:?}"
        );
    }

    /// A wrapped row that a list scrolled down draws none of, with only its
    /// foot under the list's 1 px top border, or only its head under its
    /// bottom border, reports bounds outside the list's, so AccessKit's
    /// filter keeps it only as the neighbour of the nearest row drawn, not
    /// the row past it as its neighbour, and AT-SPI reads it as off screen.
    /// GPUI draws the rows inside the list's bounds less its border, and the
    /// list reports its bounds whole: both strips beside the row lay in the
    /// list's side borders, inside those bounds, so the filter measured the
    /// row inside the list, kept the row past it too, and AT-SPI read the
    /// row as showing. Under the bottom border the pixel beyond the row's
    /// top left corner lies in the left border, inside the list, so the row
    /// takes the one beyond its bottom left, below the list.
    #[gpui::test]
    fn a_row_under_its_lists_border_lies_outside_the_list(cx: &mut TestAppContext) {
        const ROWS: usize = 8;
        struct Bordered {
            scroll: ScrollHandle,
        }
        impl Render for Bordered {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let rows = (0..ROWS).map(|index| {
                    clips_children(
                        div()
                            .id(ElementId::Name(format!("bordered-row-{index}").into()))
                            .debug_selector(move || format!("bordered-row-{index}"))
                            .role(Role::Button)
                            .aria_label(format!("Row {index}"))
                            .h(px(30.))
                            .flex_shrink_0(),
                    )
                });
                div().size_full().p(px(50.)).child(clips_children(
                    div()
                        .id("bordered-list")
                        .debug_selector(|| "bordered-list".into())
                        .role(Role::ListBox)
                        .aria_label("Rows")
                        .w(px(100.))
                        .h(px(100.))
                        .border_1()
                        .border_color(rgb(0x000000))
                        .overflow_scroll()
                        .track_scroll(&self.scroll)
                        .flex()
                        .flex_col()
                        .children(rows),
                ))
            }
        }

        // The rows start under the border; 60.5 px down, the second row
        // ends half a pixel inside it.
        let scroll = ScrollHandle::new();
        scroll.set_offset(point(px(0.), px(-60.5)));
        let (_view, cx) = cx.add_window_view({
            let scroll = scroll.clone();
            move |_, _| Bordered { scroll }
        });
        written::forget();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let scale = cx.update(|window, _| f64::from(window.scale_factor()));
        let physical = |bounds: Bounds<Pixels>| super::physical(bounds, scale as f32);
        let layout = physical(cx.debug_bounds("bordered-list").expect("the list"));
        let row = physical(cx.debug_bounds("bordered-row-1").expect("the second row"));
        assert!(
            row.y1 > layout.y0 && row.y1 < layout.y0 + scale && row.x0 > layout.x0,
            "the second row {row:?} ends inside the list's {layout:?} top border"
        );
        let list = clipping_node("bordered-list")
            .and_then(|node| node.bounds())
            .expect("the list clips its rows");
        assert_eq!(list, layout, "the list is drawn whole");
        let reported = clipping_node("bordered-row-1")
            .and_then(|node| node.bounds())
            .expect("the wrapped row clips what it holds");
        assert!(
            reported.intersect(list).is_empty(),
            "the second row, drawn nowhere, reports {reported:?} inside the list's {list:?}"
        );

        let assembly = || {
            (0..ROWS).fold(written::Assembly::default(), |assembly, index| {
                assembly.written(&format!("bordered-row-{index}"), None)
            })
        };
        let window = cx.update(|window, _| {
            physical(Bounds::new(point(px(0.), px(0.)), window.viewport_size()))
        });
        let found = reachable(assembly().tree(window, None));
        let rows: Vec<_> = found
            .iter()
            .filter(|node| node.role == Role::Button)
            .filter_map(|node| node.label.clone())
            .collect();
        assert!(
            rows.starts_with(&["Row 1".to_string(), "Row 2".to_string()]),
            "AccessKit's filter keeps the second row as the first drawn row's neighbour, and \
             not the first row: {rows:?}"
        );

        #[cfg(target_os = "linux")]
        {
            use accesskit_atspi_common::State;
            let found = atspi_state_tests::reached(assembly().tree(window, None));
            let row = found
                .iter()
                .find(|node| node.name == "Row 1")
                .expect("AT-SPI reaches the second row");
            assert!(
                row.state.contains(State::Visible) && !row.state.contains(State::Showing),
                "the second row is visible but off screen: {row:?}"
            );
        }

        // 21.5 px down, the fifth row starts half a pixel inside the bottom
        // border.
        scroll.set_offset(point(px(0.), px(-21.5)));
        written::forget();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let row = physical(cx.debug_bounds("bordered-row-4").expect("the fifth row"));
        assert!(
            row.y0 < layout.y1 && row.y0 > layout.y1 - scale && row.x0 > layout.x0,
            "the fifth row {row:?} starts inside the list's {layout:?} bottom border"
        );
        let reported = clipping_node("bordered-row-4")
            .and_then(|node| node.bounds())
            .expect("the wrapped row clips what it holds");
        assert!(
            reported.intersect(list).is_empty(),
            "the fifth row, drawn nowhere, reports {reported:?} inside the list's {list:?}"
        );
        let found = reachable(assembly().tree(window, None));
        let rows: Vec<_> = found
            .iter()
            .filter(|node| node.role == Role::Button)
            .filter_map(|node| node.label.clone())
            .collect();
        assert_eq!(
            rows,
            (0..5)
                .map(|index| format!("Row {index}"))
                .collect::<Vec<_>>(),
            "AccessKit's filter keeps the fifth row as the last drawn row's neighbour, and not \
             the sixth"
        );

        #[cfg(target_os = "linux")]
        {
            use accesskit_atspi_common::State;
            let found = atspi_state_tests::reached(assembly().tree(window, None));
            let row = found
                .iter()
                .find(|node| node.name == "Row 4")
                .expect("AT-SPI reaches the fifth row");
            assert!(
                row.state.contains(State::Visible) && !row.state.contains(State::Showing),
                "the fifth row is visible but off screen: {row:?}"
            );
        }
    }

    /// A list scrolled down that lies wholly beside what its body draws, and
    /// a row of items scrolled sideways that lies wholly above or below it,
    /// report a pixel beyond a corner, outside the body, so AccessKit's
    /// filter, which keeps each as the child past the body's fold beside a
    /// line the body draws, reaches none of what they hold, those scrolled
    /// past their leading edge included. A strip along
    /// the container's own extent beside its leading edge across the fold
    /// met them: beside the list's top, the row scrolled to end on that top,
    /// which the filter kept with the rows before and after it; beside the
    /// row's left, the item scrolled to end on that left, with its
    /// neighbours.
    #[gpui::test]
    fn a_scrolled_container_past_a_fold_reaches_none_of_its_children(cx: &mut TestAppContext) {
        const ITEMS: usize = 10;
        #[derive(Clone, Copy, Debug, PartialEq)]
        enum Past {
            Side,
            Above,
            Below,
        }
        struct Folded {
            past: Past,
            scroll: ScrollHandle,
        }
        impl Render for Folded {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let vertical = self.past == Past::Side;
                // Ten items of 30 px in a container 100 px long, scrolled
                // 150 px along it: the fifth ends on its leading edge.
                let items = (0..ITEMS).map(|index| {
                    let item = div()
                        .id(ElementId::Name(format!("folded-item-{index}").into()))
                        .debug_selector(move || format!("folded-item-{index}"))
                        .role(Role::Button)
                        .aria_label(format!("Item {index}"))
                        .flex_shrink_0();
                    if vertical {
                        item.h(px(30.))
                    } else {
                        item.w(px(30.))
                    }
                });
                let container = div()
                    .id("folded")
                    .debug_selector(|| "folded".into())
                    .role(Role::ListBox)
                    .aria_label("Items")
                    .absolute()
                    .w(px(100.))
                    .h(px(100.))
                    .flex()
                    .track_scroll(&self.scroll)
                    .children(items);
                let container = match self.past {
                    // On the body's right fold, beside it.
                    Past::Side => container
                        .left(px(100.))
                        .top(px(50.))
                        .flex_col()
                        .overflow_y_scroll(),
                    // Ending on the body's top fold, or starting on its
                    // bottom one.
                    Past::Above => container.left_0().top(px(-100.)).overflow_x_scroll(),
                    Past::Below => container.left_0().top(px(200.)).overflow_x_scroll(),
                };
                let container = clips_children(container).into_any_element();
                let line = text("folded-line", "Drawn").h(px(20.)).into_any_element();
                // The tree holds the container beside the line.
                let children = if self.past == Past::Above {
                    [container, line]
                } else {
                    [line, container]
                };
                div().size_full().p(px(100.)).child(clips_children(
                    div()
                        .id("folded-body")
                        .debug_selector(|| "folded-body".into())
                        .aria_label("Body")
                        .relative()
                        .w(px(100.))
                        .h(px(200.))
                        .overflow_hidden()
                        .children(children),
                ))
            }
        }

        for past in [Past::Side, Past::Above, Past::Below] {
            let scroll = ScrollHandle::new();
            scroll.set_offset(if past == Past::Side {
                point(px(0.), px(-150.))
            } else {
                point(px(-150.), px(0.))
            });
            let (_view, cx) = cx.add_window_view(move |_, _| Folded { past, scroll });
            written::forget();
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let scale = cx.update(|window, _| f64::from(window.scale_factor()));
            let physical = |bounds: Bounds<Pixels>| super::physical(bounds, scale as f32);
            let at = format!("{past:?}");
            let body = physical(cx.debug_bounds("folded-body").expect("the body"));
            let layout = physical(cx.debug_bounds("folded").expect("the container"));
            let fifth = physical(cx.debug_bounds("folded-item-4").expect("the fifth item"));
            let (on_fold, scrolled) = match past {
                Past::Side => (layout.x0 == body.x1, fifth.y1 == layout.y0),
                Past::Above => (layout.y1 == body.y0, fifth.x1 == layout.x0),
                Past::Below => (layout.y0 == body.y1, fifth.x1 == layout.x0),
            };
            assert!(
                on_fold && scrolled,
                "{at}: the container {layout:?} lies on the body's {body:?} fold, scrolled so \
                 that the fifth item {fifth:?} ends on its leading edge"
            );
            let reported = clipping_node("folded")
                .and_then(|node| node.bounds())
                .expect("the container clips its items");

            // The body holds the line and the container, in the order it
            // draws them, and the container its items.
            let line = written::get("folded-line").expect("the line").node;
            let assembly = written::Assembly::default().written("folded-body", None);
            let mut assembly = if past == Past::Above {
                assembly
                    .written("folded", None)
                    .node("folded-line", line, Some("folded-body"))
            } else {
                assembly
                    .node("folded-line", line, Some("folded-body"))
                    .written("folded", None)
            };
            for index in 0..ITEMS {
                let key = format!("folded-item-{index}");
                let mut item = gpui::accesskit::Node::new(Role::Button);
                item.set_label(format!("Item {index}"));
                item.set_bounds(physical(
                    cx.debug_bounds(key.clone().leak()).expect("the item"),
                ));
                assembly = assembly.node(&key, item, Some("folded"));
            }
            let window = cx.update(|window, _| {
                physical(Bounds::new(point(px(0.), px(0.)), window.viewport_size()))
            });
            let found = reachable(assembly.tree(window, None));
            assert!(
                found
                    .iter()
                    .any(|node| node.label.as_deref() == Some("Items")),
                "{at}: AccessKit's filter keeps the container as the child past the fold: \
                 {found:?}"
            );
            let items: Vec<_> = found
                .iter()
                .filter(|node| node.role == Role::Button)
                .collect();
            assert!(
                items.is_empty(),
                "{at}: the body draws no item, yet {items:?} are reached in the container's \
                 {reported:?}"
            );
            // The top left corner, or, on the body's right fold, where that
            // one would lie in the body, the top right.
            let side = if past == Past::Side {
                (reported.x0 - layout.x1).abs()
            } else {
                (reported.x1 - layout.x0).abs()
            };
            assert!(
                beyond_a_corner(reported, layout, scale)
                    && side < 0.01
                    && (reported.y1 - layout.y0).abs() < 0.01
                    && reported.intersect(body).is_empty(),
                "{at}: the container reports {reported:?}, not a pixel beyond the top corner of \
                 its {layout:?} outside the body's {body:?}"
            );
        }
    }

    /// An alert says its text as both its name and its value, announced at
    /// once: AT-SPI announces a new live node's name and macOS its value,
    /// so an alert named alone, as every one but the forms' refusal lines
    /// and the narrow rail's refusal was, could go unheard on macOS.
    #[gpui::test]
    fn an_alert_is_named_and_valued_by_its_text(cx: &mut TestAppContext) {
        struct Line;
        impl Render for Line {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                written::draw_begins();
                recorded(div().id("probe-alert").alert("Could not save"))
            }
        }
        let (_, cx) = cx.add_window_view(|_, _| Line);
        written::forget();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(
            written::announced("probe-alert"),
            Some(written::Announced::ASSERTIVE)
        );
        let node = written::get("probe-alert")
            .expect("the alert is recorded")
            .node;
        assert_eq!(node.role(), Role::Alert);
        assert_eq!(
            (node.label(), node.value()),
            (Some("Could not save"), Some("Could not save"))
        );
    }
}
