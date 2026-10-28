use crate::{ActiveTheme, Disableable, StyledExt, h_flex};
use gpui::{
    A11ySubtreeBuilder, AnyElement, App, Bounds, ClickEvent, Div, Element, ElementId,
    GlobalElementId, InspectorElementId, InteractiveElement, IntoElement, LayoutId, MouseButton,
    ParentElement, Pixels, RenderOnce, Role, SharedString, Stateful,
    StatefulInteractiveElement as _, StyleRefinement, Styled, Window, prelude::FluentBuilder as _,
};
use smallvec::SmallVec;

#[derive(IntoElement)]
pub(crate) struct MenuItemElement {
    id: ElementId,
    group_name: SharedString,
    aria_label: Option<SharedString>,
    style: StyleRefinement,
    disabled: bool,
    selected: bool,
    /// GitTurtle patch: `Some(checked)` for a checkable item, announced as a
    /// check menu item with its state; `None` for a plain item.
    checked: Option<bool>,
    on_click: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
    on_hover: Option<Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>>,
    children: SmallVec<[AnyElement; 2]>,
}

impl MenuItemElement {
    /// Create a new MenuItem with the given ID and group name.
    pub(crate) fn new(id: impl Into<ElementId>, group_name: impl Into<SharedString>) -> Self {
        let id: ElementId = id.into();
        Self {
            id: id.clone(),
            group_name: group_name.into(),
            aria_label: None,
            style: StyleRefinement::default(),
            disabled: false,
            selected: false,
            checked: None,
            on_click: None,
            on_hover: None,
            children: SmallVec::new(),
        }
    }

    /// Set ListItem as the selected item style.
    pub(crate) fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Make the item a check menu item in the given state, or a plain one.
    pub(crate) fn checked(mut self, checked: Option<bool>) -> Self {
        self.checked = checked;
        self
    }

    /// Set the accessible label for the menu item.
    pub(crate) fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
    }

    /// Set the disabled state of the MenuItem.
    pub(crate) fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set a handler for when the MenuItem is clicked.
    pub(crate) fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    /// Set a handler for when the mouse enters the MenuItem.
    #[allow(unused)]
    pub fn on_hover(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_hover = Some(Box::new(handler));
        self
    }
}

impl Disableable for MenuItemElement {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl Styled for MenuItemElement {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for MenuItemElement {
    fn extend(&mut self, elements: impl IntoIterator<Item = gpui::AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for MenuItemElement {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let role = if self.checked.is_some() {
            Role::MenuItemCheckBox
        } else {
            Role::MenuItem
        };
        let toggled = self.checked.map(|checked| {
            if checked {
                gpui::accesskit::Toggled::True
            } else {
                gpui::accesskit::Toggled::False
            }
        });
        let disabled = self.disabled;
        let content = h_flex()
            .id(self.id)
            .role(role)
            .when_some(toggled, |this, toggled| this.aria_toggled(toggled))
            .when_some(self.aria_label, |this, label| this.aria_label(label))
            .aria_selected(self.selected)
            .group(&self.group_name)
            .gap_x_1()
            .py_1()
            .px_2()
            .text_base()
            .text_color(cx.theme().foreground)
            .relative()
            .items_center()
            .justify_between()
            .refine_style(&self.style)
            .when_some(self.on_hover, |this, on_hover| {
                this.on_hover(move |hovered, window, cx| (on_hover)(hovered, window, cx))
            })
            .when(!self.disabled, |this| {
                this.group_hover(self.group_name, |this| {
                    this.bg(cx.theme().tokens.accent)
                        .text_color(cx.theme().accent_foreground)
                })
                .when(self.selected, |this| {
                    this.bg(cx.theme().tokens.accent)
                        .text_color(cx.theme().accent_foreground)
                })
                .when_some(self.on_click, |this, on_click| {
                    this.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .on_click(on_click)
                })
            })
            .when(self.disabled, |this| {
                this.text_color(cx.theme().disabled_foreground())
            })
            .children(self.children);
        AccessibleMenuItem { content, disabled }.described()
    }
}

/// GitTurtle patch: GPUI's Div has no aria-disabled setter, so a disabled
/// item drew dim but was announced as available. As gpui-base's Button does,
/// this keeps the rendered item and adds only the disabled flag when the
/// toolkit writes its node; AT-SPI reads it as neither enabled nor sensitive.
struct AccessibleMenuItem {
    content: Stateful<Div>,
    disabled: bool,
}

impl AccessibleMenuItem {
    /// GPUI's test platform never activates accessibility, so a test cannot
    /// read a menu's AccessKit nodes. A debug build names the node this item
    /// writes in a debug selector, `menu-item: <label> · <role>[ <toggled>][
    /// disabled]`, which a test build registers with the item's bounds.
    fn described(self) -> Self {
        #[cfg(debug_assertions)]
        {
            let mut node = gpui::accesskit::Node::new(self.a11y_role().unwrap_or(Role::Unknown));
            self.write_a11y_info(&mut node);
            if let Some(label) = node.label() {
                let selector = format!(
                    "menu-item: {label} · {:?}{}{}",
                    node.role(),
                    node.toggled()
                        .map_or(String::new(), |toggled| format!(" {toggled:?}")),
                    if node.is_disabled() { " disabled" } else { "" },
                );
                let Self { content, disabled } = self;
                return Self {
                    content: content.debug_selector(move || selector),
                    disabled,
                };
            }
        }
        self
    }
}

impl IntoElement for AccessibleMenuItem {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for AccessibleMenuItem {
    type RequestLayoutState = <Stateful<Div> as Element>::RequestLayoutState;
    type PrepaintState = <Stateful<Div> as Element>::PrepaintState;
    fn id(&self) -> Option<ElementId> {
        Element::id(&self.content)
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.content.source_location()
    }
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.content.request_layout(id, inspector, window, cx)
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
        self.content
            .prepaint(id, inspector, bounds, layout, window, cx)
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
        self.content
            .paint(id, inspector, bounds, layout, prepaint, window, cx);
    }
    fn a11y_role(&self) -> Option<Role> {
        self.content.a11y_role()
    }
    fn write_a11y_info(&self, node: &mut gpui::accesskit::Node) {
        self.content.write_a11y_info(node);
        if self.disabled {
            node.set_disabled();
        }
    }
    fn a11y_synthetic_children(
        &mut self,
        prepaint: &mut Self::PrepaintState,
        builder: &mut A11ySubtreeBuilder,
    ) {
        Element::a11y_synthetic_children(&mut self.content, prepaint, builder);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn aria_label_sets_accessible_name(_cx: &mut gpui::TestAppContext) {
        let item = MenuItemElement::new("open", "menu").aria_label("Open");

        assert_eq!(item.aria_label, Some("Open".into()));
    }
}
