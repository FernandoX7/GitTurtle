//! Native accessibility semantics, keyboard navigation, and local OS choices.
//! No setting is changed here: macOS display preferences are read on startup
//! and on workspace display-change notifications, then applied to the toolkit's
//! motion policy. Foreground activation remains a fallback read.
use crate::*;
use gpui_kit::component::menu::PopupMenu;

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
        KeyBinding::new("shift-f10", ManageNavigation, Some("GitTurtleNavigation")),
        // A file list's cursor row opens its menu from the keyboard, as a
        // right-click opens it: a row too narrow for its Stage or Unstage
        // control moves that action into the menu. Linux reports the Menu
        // key as `menu`; macOS keyboards have none.
        KeyBinding::new("shift-f10", RowActions, Some("GitTurtleList")),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("menu", RowActions, Some("GitTurtleList")),
    ]);
}

/// A row's menu opened from the keyboard beside the row it acts on: the
/// project pane's cursor row with Shift-F10, and a file list's with
/// Shift-F10 or the Menu key ([`RowActions`]). A right-click menu belongs to
/// its row; this one is placed from the cursor row's geometry and takes the
/// keys until it closes. Its first Down reaches its first action, the kit's
/// menu keys passing over entries that cannot act, and Escape returns focus
/// to the list or tree that opened it, its action context. Focus that leaves
/// it otherwise, to a dialog one of its items opens or with the page that
/// drew it, closes it as well, so it never reappears over another
/// repository tab or mode.
pub(super) struct KeyboardMenu {
    menu: Entity<PopupMenu>,
    position: Point<Pixels>,
    /// The window's size when the menu opened, which its overlay covers. The
    /// menu also closes on a press outside itself, past an overlay that a
    /// larger window has outgrown.
    viewport: Size<Pixels>,
    _subscriptions: [Subscription; 2],
}

impl KeyboardMenu {
    /// Builds the row's menu with `build`, the builder its right-click menu
    /// uses, focuses it, and calls `dismissed` once it closes.
    pub(super) fn open(
        position: Point<Pixels>,
        focus: FocusHandle,
        build: impl FnOnce(PopupMenu) -> PopupMenu + 'static,
        dismissed: fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>),
        window: &mut Window,
        cx: &mut Context<GitTurtle>,
    ) -> Self {
        let menu = PopupMenu::build(window, cx, move |menu, _, _| {
            build(menu).action_context(focus)
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
            position,
            viewport: window.viewport_size(),
            _subscriptions: [dismiss, blur],
        }
    }

    /// The bottom of the `row`th row of a uniform list of `height` tall rows,
    /// `inset` from its leading edge: the list's origin, its scroll offset
    /// and the row's index place it, within the list's visible part.
    pub(super) fn below_row(
        scroll: &UniformListScrollHandle,
        row: usize,
        height: Pixels,
        inset: Pixels,
    ) -> Point<Pixels> {
        let state = scroll.0.borrow();
        let bounds = state.base_handle.bounds();
        let bottom = bounds.top() + state.base_handle.offset().y + height * (row as f32 + 1.);
        point(
            bounds.left() + inset,
            bottom.clamp(bounds.top(), bounds.bottom()),
        )
    }

    /// Whether focus is in the menu, as it is until an item, Escape or a
    /// press outside it moves focus.
    pub(super) fn has_focus(&self, window: &Window, cx: &App) -> bool {
        self.menu.focus_handle(cx).contains_focused(window, cx)
    }

    /// The menu over a window-wide overlay, `id`, that a press outside the
    /// menu dismisses through `dismiss`.
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
                    .child(
                        anchored()
                            .position(self.position)
                            .snap_to_window_with_margin(px(8.))
                            .child(
                                // A press inside the menu must reach its
                                // item, not the overlay behind it.
                                div()
                                    .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                                    .child(self.menu.clone()),
                            ),
                    ),
            ),
        )
        .with_priority(gpui_kit::base::POPUP_PRIORITY)
        .into_any_element()
    }
}

pub(super) fn polite(builder: &mut A11ySubtreeBuilder<'_>) {
    builder
        .parent_node()
        .set_live(gpui::accesskit::Live::Polite);
    builder.parent_node().set_live_atomic();
}
pub(super) fn assertive(builder: &mut A11ySubtreeBuilder<'_>) {
    builder
        .parent_node()
        .set_live(gpui::accesskit::Live::Assertive);
    builder.parent_node().set_live_atomic();
}

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
        // The content mask is what the ancestors let through; GPUI scales
        // layout bounds to the node's bounds the same way.
        let visible = bounds.intersect(&window.content_mask().bounds);
        let scale = window.scale_factor();
        let physical = |edge: Pixels| f64::from(f32::from(edge) * scale);
        let visible = gpui::accesskit::Rect {
            x0: physical(visible.left()),
            y0: physical(visible.top()),
            x1: physical(visible.right()),
            y1: physical(visible.bottom()),
        };
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
    }

    thread_local! {
        static NODES: RefCell<HashMap<String, Written>> = Default::default();
        /// The wrappers prepainting, innermost last.
        static OPEN: RefCell<Vec<String>> = Default::default();
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
        let parent = OPEN.with(|open| open.borrow().last().cloned());
        NODES.with(|nodes| {
            nodes
                .borrow_mut()
                .insert(id.clone(), Written { node, parent })
        });
        OPEN.with(|open| open.borrow_mut().push(id.clone()));
        Some(id)
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

    /// Forgets every node [`get`] would return, so that what a test reads
    /// after its next draw comes from that draw.
    pub(crate) fn forget() {
        NODES.with(|nodes| nodes.borrow_mut().clear());
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
            let Some(Written { node, parent }) = get(id) else {
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
        pub(crate) fn tree(
            self,
            window: gpui::accesskit::Rect,
            focus: Option<&str>,
        ) -> gpui::accesskit::TreeUpdate {
            use gpui::accesskit::{Node, NodeId, Role, Tree, TreeId, TreeUpdate};
            let id = |key: &str| {
                let index = self
                    .nodes
                    .iter()
                    .position(|(added, ..)| added == key)
                    .unwrap_or_else(|| panic!("no node {key} was added"));
                NodeId(index as u64 + 1)
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
    pub(super) fn manage_navigation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
            || self.operation_busy.is_some()
        {
            return;
        }
        match self.nav_cursor.and_then(|index| self.nav_rows.get(index)) {
            Some(NavRow::Branch(index, _)) => {
                let branch = &self.branches[*index];
                self.open_contextual_branch(branch.name.clone(), branch.remote, window, cx);
            }
            Some(NavRow::Worktree(index)) => {
                self.open_worktree_actions(self.worktrees[*index].clone(), None, window, cx);
            }
            _ => {}
        }
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
}
