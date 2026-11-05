// Copyright 2023 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file) or the MIT license (found in
// the LICENSE-MIT file), at your option.

use accesskit::{ActionHandler, ActionRequest};
use accesskit_consumer::{NodeId, Tree};
use std::collections::{HashMap, VecDeque};
use std::fmt::{Debug, Formatter};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::WindowBounds;

/// This is an implementation detail of `accesskit_unix`, required for robust
/// state transitions with minimal overhead.
pub trait ActionHandlerNoMut {
    fn do_action(&self, request: ActionRequest);
}

/// This is an implementation detail of `accesskit_unix`, required for robust
/// state transitions with minimal overhead.
pub struct ActionHandlerWrapper<H: ActionHandler + Send>(Mutex<H>);

impl<H: 'static + ActionHandler + Send> ActionHandlerWrapper<H> {
    pub fn new(inner: H) -> Self {
        Self(Mutex::new(inner))
    }
}

impl<H: ActionHandler + Send> ActionHandlerNoMut for ActionHandlerWrapper<H> {
    fn do_action(&self, request: ActionRequest) {
        self.0.lock().unwrap().do_action(request)
    }
}

pub(crate) struct Context {
    pub(crate) app_context: Arc<RwLock<AppContext>>,
    pub(crate) tree: RwLock<Tree>,
    pub(crate) action_handler: Arc<dyn ActionHandlerNoMut + Send + Sync>,
    pub(crate) root_window_bounds: RwLock<WindowBounds>,
    pub(crate) removed: Mutex<Removed>,
}

/// GitTurtle patch (GITTURTLE-PATCH.md): selectable items, such as a list's
/// rows, that left the tree while their parent stayed, each kept on the bus
/// with its `Accessible` interface and answering that parent
/// (`PlatformNode::parent`), the most recent [`Removed::KEPT`] of them. A screen reader that last spoke such a node, as
/// Orca does a list row a search filters out, asks for its parent at the next
/// move; an object it cannot answer that for leaves Orca no common ancestor,
/// and it reads the window, the dialog and the list again.
#[derive(Debug, Default)]
pub(crate) struct Removed {
    parents: HashMap<NodeId, NodeId>,
    /// The kept nodes, each once, oldest first.
    order: VecDeque<NodeId>,
}

impl Removed {
    /// How many removed nodes stay on the bus.
    pub(crate) const KEPT: usize = 256;

    /// The parent `id` had when it left the tree, while it is kept.
    pub(crate) fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.parents.get(&id).copied()
    }

    /// Keeps `id`, which left the tree from `parent`, and returns the nodes
    /// no longer kept, oldest first, whose objects leave the bus.
    pub(crate) fn keep(&mut self, id: NodeId, parent: NodeId) -> Vec<NodeId> {
        if self.parents.insert(id, parent).is_none() {
            self.order.push_back(id);
        }
        let mut dropped = Vec::new();
        while self.order.len() > Self::KEPT {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.parents.remove(&oldest);
            dropped.push(oldest);
        }
        dropped
    }

    /// Forgets `id`, which is back in the tree; true if it was kept.
    pub(crate) fn forget(&mut self, id: NodeId) -> bool {
        if self.parents.remove(&id).is_none() {
            return false;
        }
        if let Some(index) = self.order.iter().rposition(|&kept| kept == id) {
            self.order.remove(index);
        }
        true
    }
}

impl Debug for Context {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("app_context", &self.app_context)
            .field("tree", &self.tree)
            .field("action_handler", &"ActionHandler")
            .field("root_window_bounds", &self.root_window_bounds)
            .field("removed", &self.removed)
            .finish()
    }
}

impl Context {
    pub(crate) fn new(
        app_context: &Arc<RwLock<AppContext>>,
        tree: Tree,
        action_handler: Arc<dyn ActionHandlerNoMut + Send + Sync>,
        root_window_bounds: WindowBounds,
    ) -> Arc<Self> {
        Arc::new(Self {
            app_context: Arc::clone(app_context),
            tree: RwLock::new(tree),
            action_handler,
            root_window_bounds: RwLock::new(root_window_bounds),
            removed: Mutex::new(Removed::default()),
        })
    }

    pub(crate) fn lock_removed(&self) -> MutexGuard<'_, Removed> {
        self.removed.lock().unwrap()
    }

    pub(crate) fn read_tree(&self) -> RwLockReadGuard<'_, Tree> {
        self.tree.read().unwrap()
    }

    pub(crate) fn read_root_window_bounds(&self) -> RwLockReadGuard<'_, WindowBounds> {
        self.root_window_bounds.read().unwrap()
    }

    pub fn do_action(&self, request: ActionRequest) {
        self.action_handler.do_action(request);
    }

    pub(crate) fn read_app_context(&self) -> RwLockReadGuard<'_, AppContext> {
        self.app_context.read().unwrap()
    }

    pub(crate) fn write_app_context(&self) -> RwLockWriteGuard<'_, AppContext> {
        self.app_context.write().unwrap()
    }
}

#[derive(Debug)]
pub struct AppContext {
    pub(crate) name: Option<String>,
    pub(crate) toolkit_name: Option<String>,
    pub(crate) toolkit_version: Option<String>,
    pub(crate) id: Option<i32>,
    pub(crate) adapters: Vec<(usize, Arc<Context>)>,
}

impl AppContext {
    pub fn new(name: Option<String>) -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(Self {
            name,
            toolkit_name: None,
            toolkit_version: None,
            id: None,
            adapters: Vec::new(),
        }))
    }

    pub(crate) fn adapter_index(&self, id: usize) -> Result<usize, usize> {
        self.adapters.binary_search_by(|adapter| adapter.0.cmp(&id))
    }

    pub(crate) fn push_adapter(&mut self, id: usize, context: &Arc<Context>) {
        self.adapters.push((id, Arc::clone(context)));
    }

    pub(crate) fn remove_adapter(&mut self, id: usize) {
        if let Ok(index) = self.adapter_index(id) {
            self.adapters.remove(index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Removed;
    use accesskit::{Node, NodeId as LocalNodeId, Role, Tree as TreeData, TreeId, TreeUpdate};
    use accesskit_consumer::{NodeId, Tree};

    /// The IDs of a list and of its `count` rows.
    fn list_and_rows(count: u64) -> (NodeId, Vec<NodeId>) {
        let rows: Vec<_> = (1..=count).map(LocalNodeId).collect();
        let mut list = Node::new(Role::ListBox);
        list.set_children(rows.clone());
        let mut nodes = vec![(LocalNodeId(0), list)];
        nodes.extend(
            rows.iter()
                .map(|&row| (row, Node::new(Role::ListBoxOption))),
        );
        let tree = Tree::new(
            TreeUpdate {
                nodes,
                tree: Some(TreeData::new(LocalNodeId(0))),
                tree_id: TreeId::ROOT,
                focus: LocalNodeId(0),
            },
            false,
        );
        let list = tree.state().root();
        (list.id(), list.children().map(|row| row.id()).collect())
    }

    /// GitTurtle patch: a row that came back into its list and was taken
    /// out again is the most recently kept, so the next one kept beyond the
    /// bound pushes out the oldest other row, not it: pushed out, its
    /// object would leave the bus, and AT-SPI's cache, as it is taken away.
    #[test]
    fn a_row_kept_again_after_it_came_back_is_the_newest() {
        let (list, ids) = list_and_rows(Removed::KEPT as u64 + 2);
        let mut removed = Removed::default();
        for &row in &ids[..Removed::KEPT] {
            assert!(removed.keep(row, list).is_empty());
        }
        assert!(removed.forget(ids[0]));
        assert!(removed.keep(ids[Removed::KEPT], list).is_empty());
        assert_eq!(removed.keep(ids[0], list), vec![ids[1]]);
        assert_eq!(removed.parent(ids[0]), Some(list));
        assert_eq!(removed.keep(ids[Removed::KEPT + 1], list), vec![ids[2]]);
        assert_eq!(removed.parent(ids[0]), Some(list));
    }

    /// GitTurtle patch: the queue of kept rows holds each kept row once and
    /// no other, however often one row comes back and is taken away again,
    /// as a search typed and erased does to the rows it filters out.
    #[test]
    fn a_row_taken_away_and_back_again_holds_one_place_in_the_queue() {
        let (list, ids) = list_and_rows(1);
        let mut removed = Removed::default();
        for _ in 0..600 {
            assert!(removed.keep(ids[0], list).is_empty());
            assert!(removed.order.len() <= Removed::KEPT + 1);
            assert!(removed.forget(ids[0]));
            assert!(removed.order.len() <= Removed::KEPT + 1);
        }
        assert!(removed.order.is_empty());
    }
}
