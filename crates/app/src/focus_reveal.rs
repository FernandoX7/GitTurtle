//! Scrolls a focused control into its scrolling container's view.
//!
//! When focus moves onto a control that lies wholly or partly outside the
//! container, the container scrolls by the least amount that shows the whole
//! control plus a margin: to the nearest edge, without centring or animation.
//! Only a change of focus reveals. Scrolling away from a focused control, or
//! drawing again with the same focus, leaves the container where it is.
//!
//! Layout only measures: the container's children report their bounds while
//! they are prepainted, and one reveal is applied after painting unless focus
//! or a user's scroll has superseded it in the meantime.

use gpui_kit::*;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

/// What the container last drew with. A reveal is due only when this changes,
/// so a render without a focus change scrolls nothing.
#[derive(Default, PartialEq)]
struct Observation {
    focus: Option<FocusHandle>,
    viewport: Bounds<Pixels>,
    rem_size: Pixels,
}

/// Which focus changes reveal a control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Trigger {
    /// Every focus change, from the keyboard or a pointer: the project hub's
    /// form fields.
    AnyFocus,
    /// Focus moved by the keyboard, as GPUI's `focus_visible` judges it: the
    /// last input was a key. Clicking a partly visible row focuses it and
    /// scrolls nothing.
    Keyboard,
}

/// One scrolling container's scroll handle, the focus it last drew with, and
/// stable focus handles for the controls it reveals.
#[derive(Clone)]
pub(crate) struct FocusReveal {
    scroll: ScrollHandle,
    trigger: Trigger,
    observed: Rc<RefCell<Observation>>,
    controls: Rc<RefCell<HashMap<ElementId, FocusHandle>>>,
}

impl FocusReveal {
    pub(crate) fn new(trigger: Trigger) -> Self {
        Self {
            scroll: ScrollHandle::new(),
            trigger,
            observed: Rc::default(),
            controls: Rc::default(),
        }
    }

    /// The container's scroll handle, for `track_scroll`.
    pub(crate) fn scroll(&self) -> &ScrollHandle {
        &self.scroll
    }

    /// The focus handle the control `id` tracks, the same one in every frame,
    /// so the container can tell which of its controls holds focus. A Button
    /// takes it through `track_focus`.
    pub(crate) fn focus(&self, id: impl Into<ElementId>, cx: &mut App) -> FocusHandle {
        self.controls
            .borrow_mut()
            .entry(id.into())
            .or_insert_with(|| cx.focus_handle())
            .clone()
    }

    /// The control holding focus among those [`Self::focus`] handed out.
    #[cfg(test)]
    pub(crate) fn focused(&self, window: &Window) -> Option<ElementId> {
        self.controls
            .borrow()
            .iter()
            .find(|(_, handle)| handle.is_focused(window))
            .map(|(id, _)| id.clone())
    }

    /// For the scrolling container's `on_children_prepainted`, after every
    /// reveal listener inside it has run: records the focus this frame drew
    /// with. Focus outside the container is recorded too, so returning to
    /// the same control reveals it again.
    pub(crate) fn observe(&self) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        let this = self.clone();
        move |_, window, cx| *this.observed.borrow_mut() = this.observation(window, cx)
    }

    /// For the `on_children_prepainted` of an element inside the container
    /// whose children together make up one control: reveals them, `margin`
    /// clear of the container's edges, when focus has moved into `focus`.
    pub(crate) fn control(
        &self,
        focus: FocusHandle,
        margin: Pixels,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        let this = self.clone();
        move |bounds, window, cx| {
            if !focus.contains_focused(window, cx) {
                return;
            }
            if let Some(bounds) = bounds.into_iter().reduce(|a, b| a.union(&b)) {
                this.request(bounds, margin, window, cx);
            }
        }
    }

    /// For the `on_children_prepainted` of an element inside the container
    /// whose children include controls, `focus` holding each child's handle
    /// in order: reveals the child that has gained focus, `margin` clear of
    /// the container's edges.
    pub(crate) fn controls(
        &self,
        focus: Vec<Option<FocusHandle>>,
        margin: Pixels,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        let this = self.clone();
        move |bounds, window, cx| {
            let focused = focus.iter().zip(bounds).find(|(handle, _)| {
                handle
                    .as_ref()
                    .is_some_and(|handle| handle.contains_focused(window, cx))
            });
            if let Some((_, bounds)) = focused {
                this.request(bounds, margin, window, cx);
            }
        }
    }

    /// For the scrolling container's own `on_children_prepainted`, when its
    /// first children are the controls, `focus` holding each one's handle in
    /// order: reveals the control that has gained focus, `margin` clear of
    /// the container's edges, then records the frame as [`Self::observe`]
    /// does.
    pub(crate) fn items(
        &self,
        focus: Vec<FocusHandle>,
        margin: Pixels,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        let this = self.clone();
        move |_, window, cx| {
            // A container that tracks its scroll keeps its children's bounds
            // in the handle, laid out before its own scroll offset applies.
            let focused = focus
                .iter()
                .position(|handle| handle.contains_focused(window, cx))
                .and_then(|index| this.scroll.bounds_for_item(index));
            if let Some(bounds) = focused {
                let offset = this.scroll.offset();
                this.request(bounds + point(px(0.), offset.y), margin, window, cx);
            }
            *this.observed.borrow_mut() = this.observation(window, cx);
        }
    }

    fn observation(&self, window: &Window, cx: &App) -> Observation {
        Observation {
            focus: window.focused(cx),
            viewport: self.scroll.bounds(),
            rem_size: window.rem_size(),
        }
    }

    /// Reveals `bounds`, where the control is drawn in this frame, if focus
    /// has changed since the last frame in a way the trigger reveals.
    fn request(&self, bounds: Bounds<Pixels>, margin: Pixels, window: &mut Window, cx: &mut App) {
        let observed = self.observation(window, cx);
        if *self.observed.borrow() == observed
            || (self.trigger == Trigger::Keyboard && !window.last_input_was_keyboard())
        {
            return;
        }
        let viewport = observed.viewport;
        let delta = if bounds.top() - margin < viewport.top() {
            viewport.top() - (bounds.top() - margin)
        } else if bounds.bottom() + margin > viewport.bottom() {
            viewport.bottom() - (bounds.bottom() + margin)
        } else {
            return;
        };
        let scroll = self.scroll.clone();
        let previous = scroll.offset();
        let offset = point(
            previous.x,
            (previous.y + delta).clamp(-scroll.max_offset().y, px(0.)),
        );
        if offset == previous {
            return;
        }
        // Apply one reveal after painting, unless focus or a user's scroll
        // has already superseded this request.
        window.defer(cx, move |window, cx| {
            if window.focused(cx) == observed.focus
                && scroll.offset() == previous
                && scroll.bounds() == viewport
            {
                scroll.set_offset(offset);
                window.refresh();
            }
        });
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::tags::tests::{content_mask, draw, holds, rendered};
    use gpui_kit::component::Theme;

    /// A control of a revealing list: the id it tracks focus under and the
    /// debug selector it is drawn under.
    pub(crate) type Control = (ElementId, &'static str);

    /// The control `name`, `index` tracks focus under, drawn as `name-index`.
    pub(crate) fn control(name: &'static str, index: usize) -> Control {
        (
            ElementId::from((name, index)),
            format!("{name}-{index}").leak(),
        )
    }

    /// The room the installed Button focus ring takes outside a Button.
    fn ring_room(cx: &mut VisualTestContext) -> Pixels {
        cx.read(|cx| {
            let ring = Theme::global(cx).button_focus_ring;
            ring.gap + ring.width
        })
    }

    fn focused(cx: &mut VisualTestContext, list: &FocusReveal) -> Option<ElementId> {
        cx.update(|window, _| list.focused(window))
    }

    fn bounds(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
        cx.debug_bounds(selector)
            .unwrap_or_else(|| panic!("rendered {selector}"))
    }

    /// `control` holds focus, and grown by the installed ring's gap plus
    /// width it lies inside the bounds `list` draws its rows in.
    fn assert_revealed(
        cx: &mut VisualTestContext,
        list: &FocusReveal,
        selector: &'static str,
        (id, control): &Control,
        key: &str,
    ) {
        assert_eq!(
            focused(cx, list).as_ref(),
            Some(id),
            "{key} focuses {control}"
        );
        let ring = bounds(cx, control).dilate(ring_room(cx));
        let viewport = rendered(cx, selector);
        let slack = px(0.01);
        assert!(
            ring.top() + slack >= viewport.top() && ring.bottom() <= viewport.bottom() + slack,
            "after {key}, {control} and its ring, {ring:?}, lie outside {selector}, {viewport:?}"
        );
    }

    /// Grown by the ring's room, `control` lies inside the content mask it
    /// paints in, every clipping ancestor's: the dialog shows the list whole.
    fn assert_unclipped(cx: &mut VisualTestContext, control: &'static str) {
        let mask = content_mask(cx, control);
        let ring = bounds(cx, control).dilate(ring_room(cx));
        assert!(
            holds(mask, ring),
            "{control}'s ring {ring:?} is clipped to {mask:?}"
        );
    }

    /// Focus is on the control before `controls` in tab order, and the list
    /// is long enough to scroll: Tab moves through every
    /// control and Shift+Tab back, and after each key the focused control
    /// and its ring lie inside the list.
    pub(crate) fn assert_tab_reveals(
        cx: &mut VisualTestContext,
        list: &FocusReveal,
        selector: &'static str,
        controls: &[Control],
    ) {
        let (_, last) = controls.last().expect("controls");
        assert!(
            bounds(cx, last).bottom() > rendered(cx, selector).bottom(),
            "{selector} is long enough to scroll"
        );
        for control in controls {
            cx.simulate_keystrokes("tab");
            draw(cx);
            assert_revealed(cx, list, selector, control, "tab");
        }
        assert!(list.scroll().offset().y < px(0.), "Tab scrolled {selector}");
        assert_unclipped(cx, last);
        for control in controls.iter().rev().skip(1) {
            cx.simulate_keystrokes("shift-tab");
            draw(cx);
            assert_revealed(cx, list, selector, control, "shift-tab");
        }
        assert_unclipped(cx, controls[0].1);
    }

    /// Tabs from wherever focus starts until `first` holds focus, then steps
    /// back once onto the control before it: the list's filter, or the
    /// action between the filter and the list.
    pub(crate) fn focus_filter(cx: &mut VisualTestContext, list: &FocusReveal, first: &Control) {
        for _ in 0..4 {
            cx.simulate_keystrokes("tab");
            draw(cx);
            if focused(cx, list).as_ref() == Some(&first.0) {
                cx.simulate_keystrokes("shift-tab");
                draw(cx);
                assert_eq!(
                    focused(cx, list),
                    None,
                    "{} is the list's first stop",
                    first.1
                );
                return;
            }
        }
        panic!("Tab reaches {}", first.1);
    }

    /// After focus has scrolled `selector`: a user's scroll stays where it is
    /// while focus does not change, and a click on a partly visible control
    /// scrolls nothing. The kit's Buttons keep focus where it was when
    /// pressed, so the click leaves it on the control Tab moved onto.
    pub(crate) fn assert_steady(
        cx: &mut VisualTestContext,
        list: &FocusReveal,
        selector: &'static str,
        controls: &[Control],
    ) {
        let viewport = rendered(cx, selector);
        let focused_before = focused(cx, list);
        // The user scrolls until a control lies across the list's lower edge.
        let (id, control) = &controls[controls.len() / 2];
        assert_ne!(focused_before.as_ref(), Some(id));
        let target = bounds(cx, control);
        let shift = viewport.bottom() - target.size.height / 2. - target.top();
        let away = point(px(0.), list.scroll().offset().y + shift);
        assert!(away.y < px(0.) && -away.y < list.scroll().max_offset().y);
        cx.update(|window, _| {
            list.scroll().set_offset(away);
            window.refresh();
        });
        // A render without a focus change scrolls nothing.
        draw(cx);
        draw(cx);
        assert_eq!(
            list.scroll().offset(),
            away,
            "redraws keep the user's scroll"
        );
        assert_eq!(focused(cx, list), focused_before);

        let target = bounds(cx, control);
        assert!(target.top() < viewport.bottom() && target.bottom() > viewport.bottom());
        let position = point(target.center().x, (target.top() + viewport.bottom()) / 2.);
        cx.simulate_click(position, Modifiers::default());
        draw(cx);
        assert_eq!(
            focused(cx, list),
            focused_before,
            "clicking {control} keeps focus"
        );
        assert_eq!(
            list.scroll().offset(),
            away,
            "clicking {control} leaves {selector} where it was"
        );
    }
}
