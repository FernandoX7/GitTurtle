use crate::{ActiveTheme, Disableable, StyledExt, h_flex};
use gpui::{
    A11ySubtreeBuilder, AnyElement, App, Bounds, ClickEvent, Div, Element, ElementId,
    GlobalElementId, InspectorElementId, InteractiveElement, IntoElement, LayoutId, MouseButton,
    ParentElement, Pixels, RenderOnce, Role, SharedString, Stateful,
    StatefulInteractiveElement as _, StyleRefinement, Styled, Window, div,
    prelude::FluentBuilder as _,
};
use smallvec::SmallVec;

/// GitTurtle patch: what a menu entry is. Upstream rendered every entry as a
/// menu item and dimmed a label as a disabled one, so a screen reader counted
/// a caption as one more command and the caption drew as unavailable.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Entry {
    /// A command or choice, which acts when it is enabled.
    Item,
    /// Text that titles or explains the items around it and acts on nothing.
    Label,
    /// The rule between groups of items.
    Separator,
}

#[derive(IntoElement)]
pub(crate) struct MenuItemElement {
    id: ElementId,
    group_name: SharedString,
    aria_label: Option<SharedString>,
    style: StyleRefinement,
    disabled: bool,
    selected: bool,
    /// GitTurtle patch: the selection is the keyboard's cursor rather than
    /// the pointer's hover, so it is marked as a list's selected row is.
    keyboard: bool,
    /// GitTurtle patch: the menu's rows moved under a pointer that has not
    /// moved since, so no item draws the pointer's hover.
    still: bool,
    /// GitTurtle patch: `Some(checked)` for a checkable item, announced as a
    /// check menu item with its state; `None` for a plain item.
    checked: Option<bool>,
    /// GitTurtle patch: an item unless the menu says otherwise.
    entry: Entry,
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
            keyboard: false,
            still: false,
            checked: None,
            entry: Entry::Item,
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

    /// Whether a selection came from the keyboard (Up, Down, or Right into a
    /// submenu) rather than the pointer.
    pub(crate) fn keyboard(mut self, keyboard: bool) -> Self {
        self.keyboard = keyboard;
        self
    }

    /// Whether the menu's rows moved under a still pointer, which lights no
    /// item until it moves.
    pub(crate) fn still(mut self, still: bool) -> Self {
        self.still = still;
        self
    }

    /// Make the item a check menu item in the given state, or a plain one.
    pub(crate) fn checked(mut self, checked: Option<bool>) -> Self {
        self.checked = checked;
        self
    }

    /// Render the entry as `entry`: only an item acts, takes the highlight and
    /// is announced as a menu item.
    pub(crate) fn entry(mut self, entry: Entry) -> Self {
        self.entry = entry;
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
        let item = self.entry == Entry::Item;
        let role = match self.entry {
            Entry::Item if self.checked.is_some() => Role::MenuItemCheckBox,
            Entry::Item => Role::MenuItem,
            // A label is text, and a separator AT-SPI's separator, so neither
            // is counted or announced as one more command.
            Entry::Label => Role::Label,
            Entry::Separator => Role::Splitter,
        };
        let toggled = self.checked.filter(|_| item).map(|checked| {
            if checked {
                gpui::accesskit::Toggled::True
            } else {
                gpui::accesskit::Toggled::False
            }
        });
        // Only an item can be disabled; a label or separator acts on nothing
        // and is not announced as unavailable.
        let disabled = item && self.disabled;
        let interactive = item && !self.disabled;
        // GitTurtle patch: the keyboard's cursor is marked as a list's
        // selected row is, with the list's selected fill and a 2 px leading
        // marker in its active border, laid over the item and following its
        // corners so it moves nothing. Upstream drew it in the hover fill,
        // which is the cursor's only cue and stood 1.19:1 off the menu in
        // GitTurtle's Midnight; the marker holds 3:1 against the fill and the
        // menu. The pointer's hover keeps that quieter fill.
        let cursor = interactive && self.selected && self.keyboard;
        let marker = cursor.then(|| {
            let mut marker = div()
                .absolute()
                .top_0()
                .left_0()
                .bottom_0()
                .right_0()
                .border_l_2()
                .border_color(cx.theme().list_active_border);
            marker.style().corner_radii = self.style.corner_radii.clone();
            marker
        });
        let content = h_flex()
            .id(self.id)
            .role(role)
            .when_some(toggled, |this, toggled| this.aria_toggled(toggled))
            .when_some(self.aria_label, |this, label| this.aria_label(label))
            .when(item, |this| this.aria_selected(self.selected))
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
            .when(interactive, |this| {
                this.when(!cursor, |this| {
                    // GitTurtle patch: under a still pointer the hover draws
                    // nothing, but stays registered: GPUI refreshes an
                    // element's own group hover only on a pointer move, and
                    // only through the listener this registers, so the row
                    // the pointer had lit would light again once it moved.
                    let still = self.still;
                    let (fill, text) = (cx.theme().tokens.accent, cx.theme().accent_foreground);
                    this.group_hover(self.group_name, move |this| {
                        if still {
                            this
                        } else {
                            this.bg(fill).text_color(text)
                        }
                    })
                })
                .when(self.selected && !cursor, |this| {
                    this.bg(cx.theme().tokens.accent)
                        .text_color(cx.theme().accent_foreground)
                })
                .when(cursor, |this| this.bg(cx.theme().list_active))
                .when_some(self.on_click, |this, on_click| {
                    this.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .on_click(on_click)
                })
            })
            .when(disabled, |this| {
                this.text_color(cx.theme().disabled_foreground())
            })
            .when(self.entry == Entry::Label, |this| {
                this.text_color(crate::menu_label_color(cx))
            })
            .children(self.children)
            .children(marker);
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
