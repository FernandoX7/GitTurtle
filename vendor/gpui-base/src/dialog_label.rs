//! The component layer passes titles as erased elements. Capture supported text
//! only while their own modal host lays out, then name that host by a synthetic
//! title node. No title survives outside its owning rendered modal element.
use gpui::{AnyElement, SharedString, Text};
use std::{cell::RefCell, rc::Rc};

type Label = Rc<RefCell<Option<SharedString>>>;
thread_local! {
    static SCOPES: RefCell<Vec<Label>> = const { RefCell::new(Vec::new()) };
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
        self.0.borrow().clone()
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
    const MAX_BYTES: usize = 16 * 1024;
    for child in children {
        let value = if let Some(value) = child.downcast_mut::<SharedString>() {
            Some(value.clone())
        } else if let Some(value) = child.downcast_mut::<&'static str>() {
            Some(SharedString::from(*value))
        } else {
            child
                .downcast_mut::<Text>()
                .map(|value| value.text().clone())
        };
        let Some(value) = value else {
            continue;
        };
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
        *label.borrow_mut() = Some(text.into());
    }
}
