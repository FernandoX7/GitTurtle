//! The component layer passes titles as erased elements. Capture supported text
//! only while their own modal host lays out, then name that host by a synthetic
//! title node. A styled title that names itself (a `div` with an ID, a role
//! and an accessible name) names the host too, and, as GPUI writes that title
//! as a node of its own, no synthetic node repeats it. No title survives
//! outside its owning rendered modal element.
use gpui::{AnyElement, Div, Element, SharedString, Stateful, Text};
use std::{cell::RefCell, rc::Rc};

type Label = Rc<RefCell<Option<Title>>>;
thread_local! {
    static SCOPES: RefCell<Vec<Label>> = const { RefCell::new(Vec::new()) };
}

/// A captured title: its text, and whether part of it is an element that
/// GPUI writes as an accessibility node of its own.
struct Title {
    text: SharedString,
    node: bool,
}

#[derive(Default)]
pub(crate) struct DialogLabel(Label);
impl DialogLabel {
    pub(crate) fn enter(&self) -> Scope {
        self.0.borrow_mut().take();
        SCOPES.with(|scopes| scopes.borrow_mut().push(self.0.clone()));
        Scope
    }

    pub(crate) fn get(&self) -> Option<SharedString> {
        self.0.borrow().as_ref().map(|title| title.text.clone())
    }

    /// Whether the title has a node of its own, which already reads it in
    /// the dialog: then no synthetic title node repeats it.
    pub(crate) fn titles_itself(&self) -> bool {
        self.0.borrow().as_ref().is_some_and(|title| title.node)
    }
}

pub(crate) struct Scope;
impl Drop for Scope {
    fn drop(&mut self) {
        // This synchronous layout scope is unwound on errors/panics as well.
        SCOPES.with(|scopes| {
            scopes.borrow_mut().pop();
        });
    }
}

pub(crate) fn capture_title(children: &mut [AnyElement]) {
    let label = SCOPES.with(|scopes| scopes.borrow().last().cloned());
    let Some(label) = label else {
        return;
    };
    if label.borrow().is_some() {
        return;
    }
    let mut text = String::new();
    let mut node = false;
    const MAX_BYTES: usize = 16 * 1024;
    for child in children {
        let value = readable_text(child, 0);
        let Some((value, own_node)) = value else {
            continue;
        };
        node |= own_node;
        if !text.is_empty() {
            text.push(' ');
        }
        let available = MAX_BYTES.saturating_sub(text.len());
        let mut end = value.len().min(available);
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        text.push_str(&value[..end]);
        if end < value.len() || text.len() == MAX_BYTES {
            break;
        }
    }
    if !text.is_empty() {
        *label.borrow_mut() = Some(Title {
            text: text.into(),
            node,
        });
    }
}

/// The text `element` reads as, and whether GPUI writes it as a node of its
/// own.
fn readable_text(element: &mut AnyElement, depth: usize) -> Option<(SharedString, bool)> {
    // Component Dialog and AlertDialog first erase their title and then pass
    // it through ParentElement::child, which wraps AnyElement again. Preserve
    // those plain titles without traversing arbitrary rich component trees.
    if depth == 16 {
        return None;
    }
    if let Some(inner) = element.downcast_mut::<AnyElement>() {
        return readable_text(inner, depth + 1);
    }
    if let Some(value) = element.downcast_mut::<SharedString>() {
        Some((value.clone(), false))
    } else if let Some(value) = element.downcast_mut::<&'static str>() {
        Some((SharedString::from(*value), false))
    } else if let Some(value) = element.downcast_mut::<Text>() {
        Some((value.text().clone(), false))
    } else if let Some(title) = element.downcast_mut::<Stateful<Div>>() {
        // A styled title that names itself, a label with an ID, a role and
        // an accessible name: GPUI writes it as a node of its own.
        accessible_name(title).map(|name| (name, true))
    } else {
        None
    }
}

/// The accessible name `element` writes, if it has a role and a name.
fn accessible_name(element: &impl Element) -> Option<SharedString> {
    let role = element.a11y_role()?;
    let mut node = gpui::accesskit::Node::new(role);
    element.write_a11y_info(&mut node);
    node.label()
        .filter(|label| !label.is_empty())
        .map(|label| SharedString::from(label.to_owned()))
}
