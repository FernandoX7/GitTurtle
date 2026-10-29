//! The AT-SPI state set that `accesskit_unix` serves for `Accessible.GetState`
//! is `PlatformNode::state()` of the shared `accesskit_atspi_common` adapter.
//! Upstream 0.19.1 reported every disabled Button as enabled and sensitive and
//! every disabled Switch, CheckBox or text input as read-only; the vendored #788
//! backport (`vendor/accesskit_atspi_common/GITTURTLE-PATCH.md`) reports none
//! of them, while an enabled read-only text input stays read-only.
//!
//! [`reached`] walks a tree through the same adapter, for the tests of what an
//! AT-SPI client finds in it, and a [`Bus`] feeds the adapter a window's
//! frames and keeps the objects it puts on the bus.

use accesskit_atspi_common::{
    Adapter, AdapterCallback, AppContext, Event, InterfaceSet, ObjectEvent,
};
use accesskit_atspi_common::{
    CoordType, NodeId, Rect, Role as AtspiRole, State, StateSet, WindowBounds,
};
use gpui_kit::component::WindowExt as _;
use gpui_kit::gpui::accesskit::{
    ActionHandler, ActionRequest, Live, Node, NodeId as LocalNodeId, Role, Tree, TreeId, TreeUpdate,
};
use gpui_kit::gpui::{Bounds, Context, Entity, VisualTestContext, Window, point, px};

struct IgnoreCallbacks;

impl AdapterCallback for IgnoreCallbacks {
    fn register_interfaces(&self, _: &Adapter, _: NodeId, _: InterfaceSet) {}
    fn unregister_interfaces(&self, _: &Adapter, _: NodeId, _: InterfaceSet) {}
    fn emit_event(&self, _: &Adapter, _: Event) {}
}

impl ActionHandler for IgnoreCallbacks {
    fn do_action(&mut self, _: ActionRequest) {}
}

/// A node an AT-SPI client reaches: its role, name, state set and extents in
/// window coordinates.
#[derive(Debug)]
pub(crate) struct Reached {
    pub role: AtspiRole,
    pub name: String,
    pub state: StateSet,
    pub extents: Rect,
}

/// Every node an AT-SPI client reaches walking `update`'s tree from its root,
/// child by child as `accesskit_unix` serves it, in tree order.
pub(crate) fn reached(update: TreeUpdate) -> Vec<Reached> {
    let adapter = Adapter::new(
        &AppContext::new(None),
        IgnoreCallbacks,
        update,
        true,
        WindowBounds::default(),
        IgnoreCallbacks,
    );
    let mut found = Vec::new();
    let mut pending = vec![adapter.root_id()];
    while let Some(id) = pending.pop() {
        let node = adapter.platform_node(id);
        found.push(Reached {
            role: node.role().unwrap(),
            name: node.name().unwrap(),
            state: node.state(),
            extents: node.extents(CoordType::Window).unwrap(),
        });
        let children = node.child_count().unwrap() as usize;
        pending.extend(
            (0..children)
                .rev()
                .filter_map(|index| node.child_at_index(index).unwrap()),
        );
    }
    found
}

fn control(role: Role, disabled: bool) -> Node {
    let mut node = Node::new(role);
    node.set_label("Control");
    if disabled {
        node.set_disabled();
    }
    node
}

#[test]
fn disabled_controls_are_neither_enabled_sensitive_nor_read_only() {
    let controls = [
        ("enabled Button", control(Role::Button, false)),
        ("disabled Button", control(Role::Button, true)),
        ("focused disabled Button", control(Role::Button, true)),
        ("disabled Switch", control(Role::Switch, true)),
        ("disabled CheckBox", control(Role::CheckBox, true)),
        ("disabled TextInput", control(Role::TextInput, true)),
        ("read-only TextInput", {
            let mut node = control(Role::TextInput, false);
            node.set_read_only();
            node
        }),
    ];
    let ids: Vec<_> = (1..=controls.len() as u64).map(LocalNodeId).collect();
    let mut window = Node::new(Role::Window);
    window.set_children(ids.clone());
    let mut nodes = vec![(LocalNodeId(0), window)];
    nodes.extend(
        ids.iter()
            .copied()
            .zip(controls.iter().map(|(_, node)| node.clone())),
    );
    let adapter = Adapter::new(
        &AppContext::new(None),
        IgnoreCallbacks,
        TreeUpdate {
            nodes,
            tree: Some(Tree::new(LocalNodeId(0))),
            tree_id: TreeId::ROOT,
            focus: ids[2],
        },
        true,
        WindowBounds::default(),
        IgnoreCallbacks,
    );
    let root = adapter.platform_node(adapter.root_id());
    let states: Vec<StateSet> = (0..controls.len())
        .map(|index| {
            let id = root.child_at_index(index).unwrap().unwrap();
            adapter.platform_node(id).state()
        })
        .collect();

    let usable = State::Enabled | State::Sensitive;
    assert!(states[0].contains(usable), "enabled Button: {states:?}");
    assert!(
        !states[0].contains(State::ReadOnly),
        "enabled Button: {states:?}"
    );
    let disabled = 1..controls.len() - 1;
    for ((name, _), state) in controls[disabled.clone()].iter().zip(&states[disabled]) {
        for unusable in [State::Enabled, State::Sensitive, State::ReadOnly] {
            assert!(
                !state.contains(unusable),
                "{name} reports {unusable:?}: {state:?}"
            );
        }
        assert!(!state.contains(State::Defunct), "{name}: {state:?}");
    }
    assert!(
        states[2].contains(State::Focusable | State::Focused),
        "a disabled Button that holds focus keeps its focus states: {:?}",
        states[2]
    );
    let read_only = &states[controls.len() - 1];
    assert!(
        read_only.contains(State::ReadOnly) && !read_only.intersects(usable),
        "an enabled read-only text input stays read-only: {read_only:?}"
    );
}

/// What `accesskit_unix` does on the bus for one window: the objects it
/// keeps there, each node's interfaces as the adapter registers and
/// unregisters them (a path with none answers every AT-SPI call with
/// "Unknown object"), and the announcements it makes.
#[derive(Clone, Default)]
struct Registry {
    objects: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<u128, InterfaceSet>>>,
    announced: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl AdapterCallback for Registry {
    fn register_interfaces(&self, _: &Adapter, id: NodeId, interfaces: InterfaceSet) {
        let mut objects = self.objects.lock().unwrap();
        let object = objects.entry(id.into()).or_insert(InterfaceSet::empty());
        *object = *object | interfaces;
    }

    fn unregister_interfaces(&self, _: &Adapter, id: NodeId, interfaces: InterfaceSet) {
        let mut objects = self.objects.lock().unwrap();
        if let Some(object) = objects.get_mut(&id.into()) {
            *object = *object ^ (*object & interfaces);
            if *object == InterfaceSet::empty() {
                objects.remove(&id.into());
            }
        }
    }

    fn emit_event(&self, _: &Adapter, event: Event) {
        if let Event::Object {
            event: ObjectEvent::Announcement(text, _),
            ..
        } = event
        {
            self.announced.lock().unwrap().push(text);
        }
    }
}

/// A window's AT-SPI adapter fed its tree frame by frame, as GPUI's Linux
/// backend feeds `accesskit_unix` one `TreeUpdate` per drawn frame, with
/// the objects it keeps on the bus.
pub(crate) struct Bus {
    adapter: Adapter,
    registry: Registry,
}

impl Bus {
    /// The adapter as it starts on a window whose tree is `update`.
    pub(crate) fn new(update: TreeUpdate) -> Self {
        let registry = Registry::default();
        let adapter = Adapter::new(
            &AppContext::new(None),
            registry.clone(),
            update,
            true,
            WindowBounds::default(),
            IgnoreCallbacks,
        );
        Self { adapter, registry }
    }

    /// The next frame's tree.
    pub(crate) fn update(&mut self, update: TreeUpdate) {
        self.adapter.update(update);
    }

    /// Every node an AT-SPI client reaches from the window, child by child,
    /// whose object is not on the bus, as "role name": a client walking the
    /// window finds it among its parent's children, and every call it then
    /// makes on it answers "Unknown object".
    pub(crate) fn unknown(&self) -> Vec<String> {
        let objects = self.registry.objects.lock().unwrap();
        let mut unknown = Vec::new();
        let mut pending = vec![self.adapter.root_id()];
        while let Some(id) = pending.pop() {
            let node = self.adapter.platform_node(id);
            if !objects.contains_key(&id.into()) {
                let role = node.role().unwrap();
                unknown.push(format!("{role:?} {:?}", node.name().unwrap()));
            }
            let children = node.child_count().unwrap() as usize;
            pending.extend(
                (0..children)
                    .rev()
                    .filter_map(|index| node.child_at_index(index).unwrap()),
            );
        }
        unknown
    }

    /// How many objects are on the bus.
    pub(crate) fn objects(&self) -> usize {
        self.registry.objects.lock().unwrap().len()
    }

    /// What was announced since the last call.
    pub(crate) fn announced(&self) -> Vec<String> {
        std::mem::take(&mut *self.registry.announced.lock().unwrap())
    }
}

/// What a window's adapter did around a dialog that Escape closed.
#[derive(Debug, PartialEq)]
pub(crate) struct Escaped {
    /// Every node an AT-SPI client reaches in the window after Escape that
    /// is not on the bus ([`Bus::unknown`]).
    pub unknown: Vec<String>,
    /// What closing the dialog announced.
    pub announced: Vec<String>,
    /// How many objects were on the bus while the dialog was open, and how
    /// many an adapter started on that frame puts there.
    pub covered: (usize, usize),
    /// The same after Escape.
    pub shown: (usize, usize),
}

/// Opens `surface`, a dialog, with `open` over the page from History's list,
/// as `focus_ring::sweep::open_dialog` does, changes the page beneath it
/// with `beneath`, and closes it with Escape. The window's adapter sees the
/// frame before the dialog, the frame it lands in, the frame after
/// `beneath` and the frame after Escape, as GPUI's Linux backend feeds it.
pub(crate) async fn closed_with_escape<O: crate::focus_ring::sweep::Opened>(
    app: &Entity<crate::GitTurtle>,
    cx: &mut VisualTestContext,
    surface: &str,
    open: impl FnOnce(&mut crate::GitTurtle, &mut Window, &mut Context<crate::GitTurtle>) -> O,
    beneath: impl FnOnce(&mut VisualTestContext),
) -> Escaped {
    let list = app.read_with(cx, |app, _| app.focus.clone());
    cx.update(|window, cx| window.focus(&list, cx));
    let mut bus = Bus::new(next_frame(cx));
    assert_eq!(bus.unknown(), Vec::<String>::new(), "{surface}: before");
    crate::focus_ring::sweep::open_dialog(app, cx, surface, open).await;
    bus.update(next_frame(cx));
    beneath(cx);
    let frame = next_frame(cx);
    bus.update(frame.clone());
    let covered = (bus.objects(), Bus::new(frame).objects());
    bus.announced();
    cx.simulate_keystrokes("escape");
    let frame = next_frame(cx);
    bus.update(frame.clone());
    assert!(
        !cx.update(|window, cx| window.has_active_dialog(cx)),
        "{surface} closes with Escape"
    );
    Escaped {
        unknown: bus.unknown(),
        announced: bus.announced(),
        covered,
        shown: (bus.objects(), Bus::new(frame).objects()),
    }
}

/// The window's tree as its next frame writes it: every node the app's
/// wrappers write, the page among them, and, while a dialog is open, a
/// modal dialog node holding what the dialog layer drew beside the page.
/// Successive frames keep each node's ID, as GPUI's do, so a [`Bus`] fed
/// them updates what it holds as the adapter does on a real window.
pub(crate) fn next_frame(cx: &mut VisualTestContext) -> TreeUpdate {
    super::written::forget();
    crate::views::tests::draw(cx);
    let mut tree = super::written::Assembly::default();
    let open = cx.update(|window, cx| window.has_active_dialog(cx));
    if open {
        let mut dialog = Node::new(Role::Dialog);
        dialog.set_modal();
        tree = tree.node("dialog", dialog, None);
    }
    let tree = tree.everything(open.then_some("dialog"));
    let size = cx.update(|window, _| window.viewport_size());
    let window = Bounds::new(point(px(0.), px(0.)), size);
    tree.tree(
        gpui_kit::gpui::accesskit::Rect {
            x0: 0.,
            y0: 0.,
            x1: f64::from(f32::from(window.right())),
            y1: f64::from(f32::from(window.bottom())),
        },
        None,
    )
}

/// A page that a modal dialog hides is a generic container, which the
/// filter passes over while it is shown (`beneath_modal`): hiding it takes
/// everything it holds off the bus, and showing it again puts back
/// everything it holds, including the nodes that changed or appeared while
/// it was hidden. The adapter acted only on a node that is itself included,
/// so a control that changed beneath a dialog left the bus for good, and a
/// line that appeared there never reached it: after the tracking-branch
/// review closed with Escape, 23 of the 30 children AT-SPI listed for the
/// window answered "Unknown object". Nothing on the page is new, so a live
/// region it holds, such as a conflict's position, is not announced again.
#[test]
fn a_page_shown_again_after_a_dialog_puts_everything_it_holds_on_the_bus() {
    let named = |role: Role, name: &str| {
        let mut node = Node::new(role);
        node.set_label(name);
        node
    };
    let [
        root,
        page,
        picker,
        fetch,
        list,
        row,
        status,
        block,
        dialog,
        confirm,
    ] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9].map(LocalNodeId);
    let frame = |covered: bool, fetch_name: &str, with_status: bool| {
        let mut window = Node::new(Role::Window);
        window.set_children(if covered {
            vec![page, dialog]
        } else {
            vec![page]
        });
        let mut page_node = Node::new(Role::GenericContainer);
        if covered {
            page_node.set_hidden();
        }
        let mut held = vec![picker, fetch, list, block];
        if with_status {
            held.push(status);
        }
        page_node.set_children(held);
        let mut list_node = named(Role::ListBox, "History");
        list_node.set_children(vec![row]);
        let mut nodes = vec![
            (root, window),
            (page, page_node),
            (picker, named(Role::Button, "Current branch")),
            (fetch, named(Role::Button, fetch_name)),
            (list, list_node),
            (row, named(Role::ListBoxOption, "Commit")),
            (block, {
                let mut status = named(Role::Status, "Conflict block 1 of 3");
                status.set_live(Live::Polite);
                status
            }),
        ];
        if with_status {
            let mut line = Node::new(Role::Label);
            line.set_value("Fetched origin");
            nodes.push((status, line));
        }
        if covered {
            let mut dialog_node = named(Role::AlertDialog, "Create tracking branch feature");
            dialog_node.set_modal();
            dialog_node.set_children(vec![confirm]);
            nodes.push((dialog, dialog_node));
            nodes.push((confirm, named(Role::Button, "Create and switch")));
        }
        TreeUpdate {
            nodes,
            tree: Some(Tree::new(root)),
            tree_id: TreeId::ROOT,
            // The dialog takes focus from its opener and gives it back.
            focus: if covered { confirm } else { picker },
        }
    };

    let mut bus = Bus::new(frame(false, "Fetch", false));
    assert_eq!(bus.unknown(), Vec::<String>::new());
    // The dialog opens over a control that changes as it does, and a line
    // appears on the page beneath it.
    bus.update(frame(true, "Fetch from origin", false));
    bus.update(frame(true, "Fetch from origin", true));
    assert_eq!(bus.unknown(), Vec::<String>::new());
    let hidden = bus.objects();
    bus.announced();
    // Escape closes it; nothing on the page changes.
    bus.update(frame(false, "Fetch from origin", true));
    assert_eq!(
        bus.unknown(),
        Vec::<String>::new(),
        "every node AT-SPI reaches after Escape is on the bus"
    );
    assert_eq!(bus.objects(), 7, "the window and the page's six nodes");
    assert_eq!(bus.announced(), Vec::<String>::new());
    assert_eq!(
        hidden, 3,
        "only the window and the dialog were on the bus while the page was hidden"
    );
}

/// A node AccessKit's filter keeps wholly outside its nearest clipping
/// ancestor, as the first child past that edge or because it holds focus, is
/// visible but not showing: off screen, as browsers report one. A node inside
/// that ancestor, or partly inside it, shows, as does one with no clipping
/// ancestor. The adapter reported every node it keeps both visible and
/// showing, so text GitTurtle reports where it lies below a body's fold, as
/// the GitHub panel's notice at 18 pt in a 461 × 490 window, read as shown
/// over Back to repository (vendored patch, `GITTURTLE-PATCH.md`).
#[test]
fn a_node_outside_its_clip_is_visible_but_not_showing() {
    use gpui_kit::gpui::accesskit::Rect as Bounds;
    let named = |role: Role, name: &str, y0: f64, y1: f64| {
        let mut node = Node::new(role);
        node.set_label(name);
        node.set_bounds(Bounds {
            x0: 0.,
            y0,
            x1: 100.,
            y1,
        });
        node
    };
    let [root, body, inside, cut, past, beyond, focused, footer] =
        [0, 1, 2, 3, 4, 5, 6, 7].map(LocalNodeId);
    let mut window = Node::new(Role::Window);
    window.set_bounds(Bounds {
        x0: 0.,
        y0: 0.,
        x1: 100.,
        y1: 500.,
    });
    window.set_children(vec![body, footer]);
    let mut body_node = named(Role::ScrollView, "Body", 0., 100.);
    body_node.set_clips_children();
    body_node.set_children(vec![inside, cut, past, beyond, focused]);
    let update = TreeUpdate {
        nodes: vec![
            (root, window),
            (body, body_node),
            (inside, named(Role::Button, "Inside", 10., 30.)),
            (cut, named(Role::Status, "Cut by the fold", 90., 120.)),
            (past, named(Role::Status, "Past the fold", 120., 200.)),
            (beyond, named(Role::Button, "Beyond", 210., 230.)),
            (focused, named(Role::Button, "Focused", 300., 320.)),
            (
                footer,
                named(Role::Button, "Back to repository", 400., 430.),
            ),
        ],
        tree: Some(Tree::new(root)),
        tree_id: TreeId::ROOT,
        focus: focused,
    };
    let found = reached(update);
    let state = |name: &str| {
        found
            .iter()
            .find(|node| node.name == name)
            .map(|node| node.state)
    };
    for name in ["Inside", "Cut by the fold", "Back to repository"] {
        let state = state(name).unwrap_or_else(|| panic!("{name} is reached"));
        assert!(
            state.contains(State::Visible | State::Showing),
            "{name} shows: {state:?}"
        );
    }
    for name in ["Past the fold", "Focused"] {
        let state = state(name).unwrap_or_else(|| panic!("{name} is reached"));
        assert!(
            state.contains(State::Visible) && !state.contains(State::Showing),
            "{name} is visible but not showing: {state:?}"
        );
    }
    assert_eq!(state("Beyond"), None, "AccessKit's filter leaves it out");
    let past = found
        .iter()
        .find(|node| node.name == "Past the fold")
        .expect("reached");
    assert_eq!(
        (past.extents.y, past.extents.height),
        (120, 80),
        "the node keeps its extents: {:?}",
        past.extents
    );
}
