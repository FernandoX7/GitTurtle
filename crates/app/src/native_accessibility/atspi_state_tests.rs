//! The AT-SPI state set that `accesskit_unix` serves for `Accessible.GetState`
//! is `PlatformNode::state()` of the shared `accesskit_atspi_common` adapter.
//! Upstream 0.19.1 reported every disabled Button as enabled and sensitive and
//! every disabled Switch or CheckBox as read-only; the vendored #788 backport
//! (`vendor/accesskit_atspi_common/GITTURTLE-PATCH.md`) reports none of them.

use accesskit_atspi_common::{Adapter, AdapterCallback, AppContext, Event, InterfaceSet};
use accesskit_atspi_common::{NodeId, State, StateSet, WindowBounds};
use gpui_kit::gpui::accesskit::{
    ActionHandler, ActionRequest, Node, NodeId as LocalNodeId, Role, Tree, TreeId, TreeUpdate,
};

struct IgnoreCallbacks;

impl AdapterCallback for IgnoreCallbacks {
    fn register_interfaces(&self, _: &Adapter, _: NodeId, _: InterfaceSet) {}
    fn unregister_interfaces(&self, _: &Adapter, _: NodeId, _: InterfaceSet) {}
    fn emit_event(&self, _: &Adapter, _: Event) {}
}

impl ActionHandler for IgnoreCallbacks {
    fn do_action(&mut self, _: ActionRequest) {}
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
    for ((name, _), state) in controls.iter().zip(&states).skip(1) {
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
}
