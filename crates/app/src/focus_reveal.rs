//! Scrolls a focused control into its scrolling container's view.
//!
//! When focus moves onto a control that lies wholly or partly outside the
//! container, the container scrolls by the least amount that shows the whole
//! control plus a margin: to the nearest edge, without centring or animation.
//! Only a change of focus reveals, or a page that opens with focus on a
//! control ([`FocusReveal::reveal_next_focus`]), or a resize or text-size
//! change that cuts the focused control the last frame showed whole. That
//! last reveal follows how focus was placed, not the last input: focus the
//! keyboard placed stays in view when the window is then resized by
//! dragging its edge, which moves the pointer, as the app's own focus ring
//! stays through pointer moves; shrunk until the Stashes detail cut Review
//! restore…, the window once left it cut. Scrolling away from a focused
//! control, or drawing again with the same focus, leaves the container
//! where it is, and so does a resize after the person has scrolled the
//! control out of view.
//!
//! A resize is revealed in the frame after the one that lays the container
//! out at its new size, since no frame knows that size before its layout.
//! Dragging a window's edge resizes it again and again, and the next resize
//! can be laid out before that reveal's frame is drawn, so a frame that
//! requests a reveal counts the control as shown where the reveal will put
//! it: the next resize that cuts it reveals it again, and the control stays
//! in view through the whole drag rather than only its first step.
//!
//! The reveal lands in the frame that first draws the new focus. GPUI lays a
//! frame out, prepaints and paints it, so a control's bounds are known only
//! after the container has already placed its children at this frame's
//! offset. The container's children therefore report where they lie while
//! they are prepainted, and the view that builds the container calls
//! [`FocusReveal::reveal`] while it renders: when focus has moved onto a
//! control the last frame drew, the offset is set from that frame's bounds
//! before this frame is laid out, so the frame prepaints and paints the
//! control whole. A focus change rarely moves a control, but one that the
//! last frame did not draw or that has moved since is still revealed: its
//! bounds in the frame being prepainted request one reveal, applied after
//! painting unless focus or a user's scroll has superseded it.
//!
//! A control can lie in two revealing containers, one inside the other, as
//! an open control in a side's own header does in the Stashes detail: the
//! header's region reveals it in its own scroll, and the detail shows it
//! there ([`FocusReveal::control_within`]). The outer container's reveal
//! runs the inner one's first, in the same render, and reveals the control
//! where the inner scroll has just moved it, so both show it whole in the
//! frame that first draws the focus. Revealed from where the last frame drew
//! it, before the header's region scrolled, the control lay below the
//! detail's edge for one frame after Shift+Tab onto it, until the request
//! its bounds made scrolled it in. The outer container's own scroll moves
//! the inner one whole and reveals nothing in it: the inner container stays
//! where the person left it.

use gpui_kit::*;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

/// What the container last drew with. A reveal is due only when this changes,
/// so a render without a focus change scrolls nothing. The container's size
/// counts, not where it lies in the window: inside another container that
/// scrolls, it moves with that scroll, which changes nothing it shows. Judged
/// by where it lay, a side header's region scrolled back to its focused
/// control whenever the Stashes detail around it scrolled after a key, as a
/// trackpad's scroll leaves the keyboard the last input.
#[derive(Default, PartialEq)]
struct Observation {
    focus: Option<FocusHandle>,
    size: Size<Pixels>,
    rem_size: Pixels,
}

/// A control a frame drew: its focus handle, where it lay with the container
/// at offset zero, the margin it is revealed with, and, for a control inside
/// a revealing container of its own ([`FocusReveal::control_within`]), that
/// container and the offset its scroll drew the control at.
struct Drawn {
    focus: WeakFocusHandle,
    bounds: Bounds<Pixels>,
    margin: Pixels,
    within: Option<(FocusReveal, Pixels)>,
}

/// The controls one frame drew.
#[derive(Default)]
struct Frame {
    /// Reported by [`FocusReveal::control`] and [`FocusReveal::controls`].
    controls: Vec<Drawn>,
    /// The container's own first children, from [`FocusReveal::items`]: their
    /// handles in order and their margin. The scroll handle keeps where each
    /// lies with the container at offset zero.
    items: Option<(Rc<[FocusHandle]>, Pixels)>,
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

/// One scrolling container's scroll handle, the focus it last drew with,
/// where its controls lay, and stable focus handles for the controls it
/// reveals.
#[derive(Clone)]
pub(crate) struct FocusReveal {
    scroll: ScrollHandle,
    trigger: Trigger,
    /// The next frame reveals a focus change whatever made it
    /// ([`Self::reveal_next_focus`]).
    next_focus: Rc<Cell<bool>>,
    observed: Rc<RefCell<Observation>>,
    /// Whether the focus the container last drew with was placed in a way
    /// the trigger reveals, or by a page that asked for it to be revealed:
    /// a resize that cuts its control reveals it whatever the last input
    /// has been since.
    placed: Rc<Cell<bool>>,
    /// Whether the last frame showed the control holding focus whole in the
    /// container's view, so a resize that cuts it is what cut it: judged at
    /// the offset a reveal the frame requested sets once the frame is
    /// painted (`pending`).
    shown: Rc<Cell<bool>>,
    /// The offset a reveal requested in the frame being prepainted sets
    /// once that frame is painted ([`Self::request`]), which the frame's
    /// end judges `shown` at. Judged at the offset the frame was painted
    /// at, the control a resize had just cut counted as cut though the
    /// reveal shows it whole before another frame is drawn, so where a
    /// drag's next resize was laid out before the reveal's own frame, that
    /// resize revealed nothing and the control stayed cut for the rest of
    /// the drag.
    pending: Rc<Cell<Option<Pixels>>>,
    controls: Rc<RefCell<HashMap<ElementId, FocusHandle>>>,
    /// The controls reported so far in the frame being prepainted.
    drawing: Rc<RefCell<Vec<Drawn>>>,
    /// The controls the last prepainted frame drew.
    drawn: Rc<RefCell<Frame>>,
    #[cfg(test)]
    painted: Rc<RefCell<Vec<tests::Painted>>>,
}

impl FocusReveal {
    pub(crate) fn new(trigger: Trigger) -> Self {
        Self {
            scroll: ScrollHandle::new(),
            trigger,
            next_focus: Rc::default(),
            observed: Rc::default(),
            placed: Rc::default(),
            shown: Rc::default(),
            pending: Rc::default(),
            controls: Rc::default(),
            drawing: Rc::default(),
            drawn: Rc::default(),
            #[cfg(test)]
            painted: Rc::default(),
        }
    }

    /// The container's scroll handle, for `track_scroll`.
    pub(crate) fn scroll(&self) -> &ScrollHandle {
        &self.scroll
    }

    /// The focus handle the control `id` tracks, the same one in every frame,
    /// so the container can tell which of its controls holds focus. A Button
    /// takes it through `track_focus`.
    pub(crate) fn focus(&self, id: impl Into<ElementId>, cx: &App) -> FocusHandle {
        self.controls
            .borrow_mut()
            .entry(id.into())
            .or_insert_with(|| cx.focus_handle())
            .clone()
    }

    /// Has the next frame the container draws reveal the control focus has
    /// moved onto whatever moved it there, as a page that opens with focus
    /// on a control needs, whether a key or a click opened it: Settings in a
    /// half tile at 18 pt, whose first control lies below its fold. It
    /// reveals the control even when the container last drew focus on it, as
    /// when the page opens again, scrolled to its top, onto the control it
    /// was left with: reopened by Ctrl+, after Back, Settings left the
    /// Omarchy theme's card below the fold.
    pub(crate) fn reveal_next_focus(&self) {
        self.next_focus.set(true);
    }

    /// Whether focus lies on one of the controls the container last drew,
    /// or inside one, as on a list's row or an editor's text: a view that
    /// scrolls the container for another reason leaves it where it is
    /// rather than scroll the focused control out of view.
    pub(crate) fn holds_focus(&self, window: &Window, cx: &App) -> bool {
        self.drawn_focus(window, cx).is_some()
    }

    /// Where the control holding focus lies in the frame being painted, if
    /// the container drew it: read after the container has prepainted, as
    /// its scroll cue's fades are (`tags::ScrollCue::revealing`), which stop
    /// short of the control's ring.
    pub(crate) fn focused_bounds(&self, window: &Window, cx: &App) -> Option<Bounds<Pixels>> {
        self.drawn_focus(window, cx)
            .map(|(bounds, _)| bounds + point(px(0.), self.scroll.offset().y))
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

    /// For the render of the view that builds the container, in every frame:
    /// when focus has moved, in a way the trigger reveals, onto a control
    /// the last frame drew outside the container's view, scrolls the
    /// container now, so the frame being rendered lays the control out
    /// revealed and the first frame that draws the focus shows it whole.
    pub(crate) fn reveal(&self, window: &Window, cx: &App) {
        let observed = self.observation(window, cx);
        if !self.due(&observed, window) {
            return;
        }
        // A control in a revealing container of its own is revealed there
        // first, so this container shows it where that scroll has moved it.
        if let Some(inner) = self.drawn_within(window, cx) {
            inner.reveal(window, cx);
        }
        if let Some((bounds, margin)) = self.drawn_focus(window, cx) {
            let at = bounds + point(px(0.), self.scroll.offset().y);
            if let Some(offset) = self.target(at, margin, self.scroll.bounds()) {
                self.scroll.set_offset(offset);
            }
        }
    }

    /// For the scrolling container's `on_children_prepainted`, after every
    /// reveal listener inside it has run: records the focus this frame drew
    /// with and the controls it drew. Focus outside the container is
    /// recorded too, so returning to the same control reveals it again.
    pub(crate) fn observe(&self) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        let this = self.clone();
        move |_, window, cx| this.finish(None, window, cx)
    }

    /// For the `on_children_prepainted` of an element inside the container
    /// whose children together make up one control: reveals them, `margin`
    /// clear of the container's edges, when focus has moved into `focus`.
    pub(crate) fn control(
        &self,
        focus: FocusHandle,
        margin: Pixels,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        self.reported(focus, margin, None)
    }

    /// [`Self::control`] for a control that also lies in `inner`, a
    /// revealing container inside this one that reveals it in its own
    /// scroll, as a side's own header does its open control in the Stashes
    /// detail. When focus moves onto the control, this container's
    /// [`Self::reveal`] runs `inner`'s first and shows the control where
    /// `inner` has scrolled it in that frame, not where the last frame drew
    /// it.
    pub(crate) fn control_within(
        &self,
        focus: FocusHandle,
        margin: Pixels,
        inner: &FocusReveal,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        self.reported(focus, margin, Some(inner.clone()))
    }

    /// [`Self::control`], its control inside `within` where given.
    fn reported(
        &self,
        focus: FocusHandle,
        margin: Pixels,
        within: Option<FocusReveal>,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        let this = self.clone();
        let weak = focus.downgrade();
        move |bounds, window, cx| {
            let Some(bounds) = bounds.into_iter().reduce(|a, b| a.union(&b)) else {
                return;
            };
            let within = within
                .as_ref()
                .map(|inner| (inner.clone(), inner.scroll.offset().y));
            this.record(weak.clone(), bounds, margin, within);
            if focus.contains_focused(window, cx) {
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
            let mut focused = None;
            for (handle, bounds) in focus.iter().zip(bounds) {
                let Some(handle) = handle else { continue };
                this.record(handle.downgrade(), bounds, margin, None);
                if focused.is_none() && handle.contains_focused(window, cx) {
                    focused = Some(bounds);
                }
            }
            if let Some(bounds) = focused {
                this.request(bounds, margin, window, cx);
            }
        }
    }

    /// For the `on_children_prepainted` of an element inside the container
    /// that holds a group of controls, such as a list that scrolls on its
    /// own, `focus` holding their handles: reveals the element's children
    /// together, `margin` clear of the container's edges, when focus has
    /// moved onto any of them. The list reveals the control inside itself;
    /// the container shows the list, whose bounds the list's own scroll does
    /// not move, so the two reveals agree in the frame focus moves.
    pub(crate) fn group(
        &self,
        focus: Vec<FocusHandle>,
        margin: Pixels,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        let this = self.clone();
        move |bounds, window, cx| {
            let Some(bounds) = bounds.into_iter().reduce(|a, b| a.union(&b)) else {
                return;
            };
            for handle in &focus {
                this.record(handle.downgrade(), bounds, margin, None);
            }
            if focus
                .iter()
                .any(|handle| handle.contains_focused(window, cx))
            {
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
        let focus: Rc<[FocusHandle]> = focus.into();
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
            this.finish(Some((focus.clone(), margin)), window, cx);
        }
    }

    fn observation(&self, window: &Window, cx: &App) -> Observation {
        Observation {
            focus: window.focused(cx),
            size: self.scroll.bounds().size,
            rem_size: window.rem_size(),
        }
    }

    /// Whether a reveal is due: focus has changed since the last frame in a
    /// way the trigger reveals; a page opening asked for its focus to be
    /// revealed ([`Self::reveal_next_focus`]) even onto the control it last
    /// drew focused; or, focus unchanged, the container's size or the text
    /// size has changed since a frame that showed the control whole, focus
    /// having been placed there in a way the trigger reveals, so the change
    /// is what cuts it.
    fn due(&self, observed: &Observation, window: &Window) -> bool {
        let last = self.observed.borrow();
        self.next_focus.get()
            || if last.focus != observed.focus {
                self.revealing(window)
            } else {
                *last != *observed && self.placed.get() && self.shown.get()
            }
    }

    /// Whether a focus change now is one the trigger reveals.
    fn revealing(&self, window: &Window) -> bool {
        self.trigger == Trigger::AnyFocus || window.last_input_was_keyboard()
    }

    /// Records that the frame being prepainted drew the control `focus`
    /// tracks at `bounds`, where it lies at the container's current offset,
    /// inside the revealing container `within` at the offset given there.
    fn record(
        &self,
        focus: WeakFocusHandle,
        bounds: Bounds<Pixels>,
        margin: Pixels,
        within: Option<(FocusReveal, Pixels)>,
    ) {
        let bounds = bounds - point(px(0.), self.scroll.offset().y);
        self.drawing.borrow_mut().push(Drawn {
            focus,
            bounds,
            margin,
            within,
        });
    }

    /// Ends the frame being prepainted: keeps the controls it drew, its own
    /// first children's handles when they are the controls, and the focus it
    /// drew with.
    fn finish(
        &self,
        items: Option<(Rc<[FocusHandle]>, Pixels)>,
        window: &mut Window,
        cx: &mut App,
    ) {
        {
            let mut drawn = self.drawn.borrow_mut();
            let mut drawing = self.drawing.borrow_mut();
            std::mem::swap(&mut drawn.controls, &mut *drawing);
            drawing.clear();
            drawn.items = items;
        }
        let observed = self.observation(window, cx);
        if self.next_focus.get() {
            self.placed.set(true);
        } else if self.observed.borrow().focus != observed.focus {
            self.placed.set(self.revealing(window));
        }
        // Where a reveal this frame requested will leave the container: it
        // shows the control whole there before any later resize is drawn.
        let offset = self
            .pending
            .take()
            .unwrap_or_else(|| self.scroll.offset().y);
        let viewport = self.scroll.bounds();
        let slack = px(0.01);
        self.shown
            .set(self.drawn_focus(window, cx).is_some_and(|(bounds, _)| {
                let drawn = bounds + point(px(0.), offset);
                drawn.top() + slack >= viewport.top() && drawn.bottom() <= viewport.bottom() + slack
            }));
        *self.observed.borrow_mut() = observed;
        self.next_focus.set(false);
        #[cfg(test)]
        self.painted.borrow_mut().push(tests::Painted {
            control: self.focused_bounds(window, cx),
            viewport: self.scroll.bounds(),
            offset: self.scroll.offset().y,
            max_offset: self.scroll.max_offset().y,
        });
    }

    /// The control holding focus among those the last prepainted frame drew:
    /// where it lies with the container at offset zero, moved as far as a
    /// revealing container it lies in has scrolled since, and its margin.
    fn drawn_focus(&self, window: &Window, cx: &App) -> Option<(Bounds<Pixels>, Pixels)> {
        let drawn = self.drawn.borrow();
        let item = drawn.items.as_ref().and_then(|(focus, margin)| {
            let index = focus
                .iter()
                .position(|handle| handle.contains_focused(window, cx))?;
            Some((self.scroll.bounds_for_item(index)?, *margin))
        });
        item.or_else(|| {
            let control = Self::holding_focus(&drawn.controls, window, cx)?;
            let moved = control
                .within
                .as_ref()
                .map_or(px(0.), |(inner, at)| inner.scroll.offset().y - *at);
            Some((control.bounds + point(px(0.), moved), control.margin))
        })
    }

    /// The revealing container inside this one that holds the control
    /// holding focus, if the last prepainted frame drew it there.
    fn drawn_within(&self, window: &Window, cx: &App) -> Option<FocusReveal> {
        let drawn = self.drawn.borrow();
        let control = Self::holding_focus(&drawn.controls, window, cx)?;
        control.within.as_ref().map(|(inner, _)| inner.clone())
    }

    /// The control among `controls` that holds focus or contains it.
    fn holding_focus<'a>(controls: &'a [Drawn], window: &Window, cx: &App) -> Option<&'a Drawn> {
        controls.iter().find(|control| {
            control
                .focus
                .upgrade()
                .is_some_and(|focus| focus.contains_focused(window, cx))
        })
    }

    /// The offset that shows `bounds`, where the control lies at the current
    /// offset, `margin` clear of `viewport`'s edges by the least scroll, if
    /// the container can move at all.
    fn target(
        &self,
        bounds: Bounds<Pixels>,
        margin: Pixels,
        viewport: Bounds<Pixels>,
    ) -> Option<Point<Pixels>> {
        if viewport.size.height <= Pixels::ZERO {
            return None;
        }
        let delta = if bounds.top() - margin < viewport.top() {
            viewport.top() - (bounds.top() - margin)
        } else if bounds.bottom() + margin > viewport.bottom() {
            viewport.bottom() - (bounds.bottom() + margin)
        } else {
            return None;
        };
        let previous = self.scroll.offset();
        let offset = point(
            previous.x,
            (previous.y + delta).clamp(-self.scroll.max_offset().y, px(0.)),
        );
        (offset != previous).then_some(offset)
    }

    /// Reveals `bounds`, where the control is drawn in this frame, if a
    /// reveal is due ([`Self::due`]) and [`Self::reveal`] has not already
    /// shown it: as after a resize, which only this frame's layout shows.
    fn request(&self, bounds: Bounds<Pixels>, margin: Pixels, window: &mut Window, cx: &mut App) {
        let observed = self.observation(window, cx);
        if !self.due(&observed, window) {
            return;
        }
        let viewport = self.scroll.bounds();
        let Some(offset) = self.target(bounds, margin, viewport) else {
            return;
        };
        let scroll = self.scroll.clone();
        let previous = scroll.offset();
        self.pending.set(Some(offset.y));
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

    /// One frame the container was prepainted and painted in, as it ended:
    /// where the control holding focus was drawn, if the container drew it,
    /// the container's visible bounds, and its offset and reach.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct Painted {
        pub(crate) control: Option<Bounds<Pixels>>,
        pub(crate) viewport: Bounds<Pixels>,
        pub(crate) offset: Pixels,
        pub(crate) max_offset: Pixels,
    }

    impl FocusReveal {
        /// Every frame painted since the last call.
        pub(crate) fn take_painted(&self) -> Vec<Painted> {
            std::mem::take(&mut *self.painted.borrow_mut())
        }
    }

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

    /// Focus is on the control before `controls` in tab order, and the list
    /// is long enough to scroll: Tab moves through every control and
    /// Shift+Tab back, and in every frame painted after each key, the first
    /// that draws the new focus included, the focused control grown by the
    /// installed ring's room lies inside the list's visible bounds.
    pub(crate) fn assert_every_frame_reveals(
        cx: &mut VisualTestContext,
        list: &FocusReveal,
        selector: &'static str,
        controls: &[Control],
    ) {
        let room = ring_room(cx);
        let step = |cx: &mut VisualTestContext, key: &str, (id, control): &Control| {
            list.take_painted();
            cx.simulate_keystrokes(key);
            draw(cx);
            assert_eq!(
                focused(cx, list).as_ref(),
                Some(id),
                "{key} focuses {control}"
            );
            let frames = list.take_painted();
            assert!(!frames.is_empty(), "{key} onto {control} paints {selector}");
            let slack = px(0.01);
            for (index, frame) in frames.iter().enumerate() {
                let ring = frame
                    .control
                    .unwrap_or_else(|| panic!("frame {index} after {key} draws {control}"))
                    .dilate(room);
                let viewport = frame.viewport;
                assert!(
                    ring.top() + slack >= viewport.top()
                        && ring.bottom() <= viewport.bottom() + slack,
                    "frame {index} of {} after {key} paints {control} with its ring, {ring:?}, \
                     outside {selector}, {viewport:?}",
                    frames.len()
                );
            }
        };
        let (_, last) = controls.last().expect("controls");
        assert!(
            bounds(cx, last).bottom() > rendered(cx, selector).bottom(),
            "{selector} is long enough to scroll"
        );
        for control in controls {
            step(cx, "tab", control);
        }
        assert!(list.scroll().offset().y < px(0.), "Tab scrolled {selector}");
        for control in controls.iter().rev().skip(1) {
            step(cx, "shift-tab", control);
        }
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
