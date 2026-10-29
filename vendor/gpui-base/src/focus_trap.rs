use gpui::{
    AnyElement, App, Bounds, Element, ElementId, FocusHandle, Global, GlobalElementId,
    InteractiveElement, Interactivity, IntoElement, LayoutId, ParentElement, Pixels,
    StatefulInteractiveElement, StyleRefinement, Styled, WeakFocusHandle, Window,
};
use std::collections::HashMap;

/// Initialize the focus trap manager as a global
pub fn init(cx: &mut App) {
    cx.set_global(FocusTrapManager::new());
}

/// An extension trait to add `focus_trap` functionality to interactive elements.
pub trait FocusTrapElement: InteractiveElement + Sized {
    /// Enable focus trap for this element.
    ///
    /// When enabled, focus will automatically cycle within this container
    /// instead of escaping to parent elements. This is useful for modal dialogs,
    /// sheets, and other overlay components.
    ///
    /// The focus trap works by:
    /// 1. Registering this element as a focus trap container
    /// 2. When Tab/Shift-Tab is pressed, Root intercepts the event
    /// 3. If focus would leave the container, it cycles back to the beginning/end
    ///
    /// # Example
    ///
    /// ```ignore
    /// v_flex()
    ///     .child(Button::new("btn1").label("Button 1"))
    ///     .child(Button::new("btn2").label("Button 2"))
    ///     .child(Button::new("btn3").label("Button 3"))
    ///     .focus_trap("trap1", &self.container_focus_handle)
    /// // Pressing Tab will cycle: btn1 -> btn2 -> btn3 -> btn1
    /// // Focus will not escape to elements outside this container
    /// ```
    ///
    /// See also: <https://github.com/focus-trap/focus-trap-react>
    fn focus_trap(
        self,
        id: impl Into<ElementId>,
        focus_handle: &FocusHandle,
    ) -> FocusTrapContainer<Self>
    where
        Self: ParentElement + Styled + Element + 'static,
    {
        FocusTrapContainer::new(id, focus_handle.clone(), self)
    }
}
impl<T: InteractiveElement + Sized> FocusTrapElement for T {}

/// Global state to manage all focus trap containers
pub(crate) struct FocusTrapManager {
    /// Map from container element ID to its focus trap info
    traps: HashMap<GlobalElementId, WeakFocusHandle>,
    /// GitTurtle patch: the node each modal trap was last laid out to write,
    /// with the synthetic title it adds, if any, read by
    /// [`focused_modal_nodes`] and [`focused_modal_titles`]. Debug builds
    /// only, as GPUI keeps its own node provenance.
    #[cfg(debug_assertions)]
    modals: HashMap<GlobalElementId, (gpui::accesskit::Node, Option<gpui::accesskit::Node>)>,
}

impl Global for FocusTrapManager {}

impl FocusTrapManager {
    /// Create a new focus trap manager
    fn new() -> Self {
        Self {
            traps: HashMap::new(),
            #[cfg(debug_assertions)]
            modals: HashMap::new(),
        }
    }

    pub(crate) fn global(cx: &App) -> &Self {
        cx.global::<FocusTrapManager>()
    }

    fn global_mut(cx: &mut App) -> &mut Self {
        cx.global_mut::<FocusTrapManager>()
    }

    /// Register a focus trap container
    fn register_trap(id: &GlobalElementId, container_handle: WeakFocusHandle, cx: &mut App) {
        let this = Self::global_mut(cx);
        this.traps.insert(id.clone(), container_handle);
        this.cleanup();
    }

    /// Find which focus trap contains the currently focused element
    fn find_active_trap(window: &Window, cx: &App) -> Option<FocusHandle> {
        for (_id, container_handle) in Self::global(cx).traps.iter() {
            let Some(container) = container_handle.upgrade() else {
                continue;
            };

            if container.contains_focused(window, cx) {
                return Some(container.clone());
            }
        }
        None
    }

    /// Cleanup any traps with dropped handles
    fn cleanup(&mut self) {
        self.traps.retain(|_, handle| handle.upgrade().is_some());
        #[cfg(debug_assertions)]
        {
            let traps = &self.traps;
            self.modals.retain(|id, _| traps.contains_key(id));
        }
    }
}

impl Default for FocusTrapManager {
    fn default() -> Self {
        Self::new()
    }
}

/// A wrapper element that implements focus trap behavior.
///
/// This element wraps another element and registers it as a focus trap container.
/// Focus will automatically cycle within the container when Tab/Shift-Tab is pressed.
pub struct FocusTrapContainer<E: InteractiveElement + ParentElement + Styled + Element> {
    id: ElementId,
    focus_handle: FocusHandle,
    base: E,
    dialog_label: crate::dialog_label::DialogLabel,
}

impl<E: InteractiveElement + ParentElement + Styled + Element> FocusTrapContainer<E> {
    pub(crate) fn new(id: impl Into<ElementId>, focus_handle: FocusHandle, child: E) -> Self {
        Self {
            id: id.into(),
            base: child.track_focus(&focus_handle),
            focus_handle,
            dialog_label: Default::default(),
        }
    }

    /// GitTurtle patch: the title node a modal `dialog` named by its plain
    /// captured title adds to read it, if it adds one. A title that is a
    /// node of its own already reads in the dialog, which takes its name; a
    /// synthetic title would repeat it.
    ///
    /// The title is static text, so its text is its value: AccessKit's
    /// adapters name a label by its value and never by its label property
    /// (AT-SPI announces the value as the name; macOS reports it as the
    /// static text's `AXValue`). Upstream set only the label property, so
    /// AT-SPI reached the title of a plain-text dialog, such as GitTurtle's
    /// command palette, without a name. No label property repeats it, which
    /// macOS would report as the title's `AXTitle` beside its value.
    fn synthetic_title(&self, dialog: &gpui::accesskit::Node) -> Option<gpui::accesskit::Node> {
        if !matches!(dialog.role(), gpui::Role::Dialog | gpui::Role::AlertDialog)
            || !dialog.labelled_by().is_empty()
            || self.dialog_label.titles_itself()
        {
            return None;
        }
        let label = self.dialog_label.get()?;
        if dialog.label() != Some(label.as_ref()) {
            return None;
        }
        let mut title = gpui::accesskit::Node::new(gpui::Role::Label);
        title.set_value(label.to_string());
        Some(title)
    }
}

impl<E: InteractiveElement + ParentElement + Styled + Element> IntoElement
    for FocusTrapContainer<E>
{
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}
impl<E: InteractiveElement + ParentElement + Styled + Element> ParentElement
    for FocusTrapContainer<E>
{
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.base.extend(elements);
    }
}
impl<E: InteractiveElement + ParentElement + Styled + Element> InteractiveElement
    for FocusTrapContainer<E>
{
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}
impl<E: InteractiveElement + ParentElement + Styled + Element> StatefulInteractiveElement
    for FocusTrapContainer<E>
{
}
impl<E: InteractiveElement + ParentElement + Styled + Element> Styled for FocusTrapContainer<E> {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl<E: InteractiveElement + ParentElement + Styled + Element + 'static> Element
    for FocusTrapContainer<E>
{
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    // GitTurtle patch: trapping keyboard focus must preserve the wrapped
    // element's accessible role, metadata and synthetic subtree.
    fn a11y_role(&self) -> Option<gpui::Role> {
        self.base.a11y_role()
    }

    fn write_a11y_info(&self, node: &mut gpui::accesskit::Node) {
        self.base.write_a11y_info(node);
        if matches!(node.role(), gpui::Role::Dialog | gpui::Role::AlertDialog) {
            node.set_modal();
            if node.label().is_none() && node.labelled_by().is_empty() {
                node.set_label(
                    self.dialog_label
                        .get()
                        .unwrap_or_else(|| {
                            if node.role() == gpui::Role::AlertDialog {
                                "Alert dialog"
                            } else {
                                "Dialog"
                            }
                            .into()
                        })
                        .to_string(),
                );
            }
        }
    }

    fn a11y_synthetic_children(
        &mut self,
        prepaint: &mut Self::PrepaintState,
        builder: &mut gpui::A11ySubtreeBuilder,
    ) {
        Element::a11y_synthetic_children(&mut self.base, prepaint, builder);
        if let Some(title) = self.synthetic_title(builder.parent_node()) {
            let title_id = builder.synthetic_node_id("dialog-visible-title");
            if builder.push_child(title_id, title) {
                builder.parent_node().set_labelled_by([title_id]);
            }
        }
    }

    fn request_layout(
        &mut self,
        global_id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        // Register this focus trap with the manager
        FocusTrapManager::register_trap(global_id.unwrap(), self.focus_handle.downgrade(), cx);
        let role = self
            .base
            .a11y_role()
            .filter(|role| matches!(role, gpui::Role::Dialog | gpui::Role::AlertDialog));
        let _label_scope = role.is_some().then(|| self.dialog_label.enter());
        let layout = self.base.request_layout(global_id, None, window, cx);
        // The title is captured by now: record the node GPUI would write,
        // which it writes only while an assistive technology is active.
        #[cfg(debug_assertions)]
        if let (Some(role), Some(id)) = (role, global_id) {
            let mut node = gpui::accesskit::Node::new(role);
            self.write_a11y_info(&mut node);
            let title = self.synthetic_title(&node);
            FocusTrapManager::global_mut(cx)
                .modals
                .insert(id.clone(), (node, title));
        }
        layout
    }

    fn prepaint(
        &mut self,
        global_id: Option<&gpui::GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.base
            .prepaint(global_id, inspector_id, bounds, request_layout, window, cx)
    }

    fn paint(
        &mut self,
        global_id: Option<&gpui::GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.base.paint(
            global_id,
            inspector_id,
            bounds,
            request_layout,
            prepaint,
            window,
            cx,
        )
    }
}

/// Returns the focus handle for the active focus trap, if any.
#[doc(hidden)]
pub fn active_focus_trap(window: &Window, cx: &App) -> Option<FocusHandle> {
    FocusTrapManager::find_active_trap(window, cx)
}

/// GitTurtle patch: the accessibility nodes of the modal dialogs that hold
/// focus, each as its focus trap last laid it out to write: role, modal
/// state and name. GPUI writes a node only while an assistive technology is
/// active, which a test never is. Debug builds only.
#[cfg(debug_assertions)]
#[doc(hidden)]
pub fn focused_modal_nodes(window: &Window, cx: &App) -> Vec<gpui::accesskit::Node> {
    let manager = FocusTrapManager::global(cx);
    manager
        .traps
        .iter()
        .filter(|(_, handle)| {
            handle
                .upgrade()
                .is_some_and(|handle| handle.contains_focused(window, cx))
        })
        .filter_map(|(id, _)| manager.modals.get(id).map(|(node, _)| node.clone()))
        .collect()
}

/// GitTurtle patch: the synthetic title node each modal dialog holding
/// focus adds to read its plain-text title, as its focus trap last laid it
/// out to write; a dialog whose title is a node of its own adds none. GPUI
/// writes a window's tree only while an assistive technology is active,
/// which a test never is. Debug builds only.
#[cfg(debug_assertions)]
#[doc(hidden)]
pub fn focused_modal_titles(window: &Window, cx: &App) -> Vec<gpui::accesskit::Node> {
    let manager = FocusTrapManager::global(cx);
    manager
        .traps
        .iter()
        .filter(|(_, handle)| {
            handle
                .upgrade()
                .is_some_and(|handle| handle.contains_focused(window, cx))
        })
        .filter_map(|(id, _)| manager.modals.get(id).and_then(|(_, title)| title.clone()))
        .collect()
}
