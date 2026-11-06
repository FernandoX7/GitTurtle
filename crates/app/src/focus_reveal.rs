//! Scrolls a focused control into its scrolling container's view.
//!
//! When focus moves onto a control that lies wholly or partly outside the
//! container, the container scrolls by the least amount that shows the whole
//! control plus a margin: to the nearest edge, without centring or animation.
//! A control taller than the view with its margins shows its leading edge,
//! its top a margin below the view's, from either side: it holds the label
//! and the first action, and both passes described below then agree, where
//! the nearest edge showed its foot first and then its top, a swing between
//! edges. Only a change of focus reveals, or a page that opens with focus on a
//! control ([`FocusReveal::reveal_next_focus`]), or a resize or text-size
//! change that cuts the focused control the last frame showed whole. That
//! last reveal follows how focus was placed, not the last input: focus the
//! keyboard placed stays in view when the window is then resized by
//! dragging its edge, which moves the pointer, as the app's own focus ring
//! stays through pointer moves; shrunk until the Stashes detail cut Review
//! restore…, the window once left it cut. Scrolling away from a focused
//! control, or drawing again with the same focus, leaves the container
//! where it is, and so does a resize after the person has scrolled the
//! control out of view. A control taller than the view, revealed by its
//! leading edge, counts as shown while that edge is in view with its margin:
//! a resize after which it would fit whole but is cut reveals it whole, one
//! that cuts its leading edge reveals it by that edge again, and any other
//! leaves it where it is. Counted as cut because it never fitted whole, it
//! stayed cut when a resize made room for it or moved its top out of view.
//! A control a frame showed whole that a resize makes taller than the view
//! is revealed by its leading edge, as a focus change reveals one.
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
//!
//! The focus handles the reveal hands out for its controls' ids
//! ([`FocusReveal::focus`]) live as long as their controls are drawn: once a
//! frame's controls are known, it keeps those the frame drew, the one
//! holding focus and any something else still holds, so a control scrolled
//! out of a list that draws only what it shows keeps focus, and the reveal
//! finds it again once it is drawn.
//! Kept for every id ever handed out, a long session's walks through long
//! lists grew them by each row.

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

/// The focus handle [`FocusReveal::focus`] hands out for one control id,
/// and whether it has been handed out since the last frame's controls were
/// known.
struct Kept {
    /// Held by the reveal between frames; let go of only for a moment as a
    /// frame ends, to learn whether anything else holds it
    /// ([`FocusReveal::release_undrawn`]).
    focus: Option<FocusHandle>,
    handed: bool,
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

/// Why a reveal is due ([`FocusReveal::due`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Due {
    /// Focus has moved in a way the trigger reveals, or a page asked for its
    /// focus to be revealed.
    Focus,
    /// The container's size or the text size has changed under focus the
    /// last frame showed so.
    Resize(Shown),
}

/// How a frame showed the control holding focus ([`FocusReveal::finish`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shown {
    /// Whole in the container's view.
    Whole,
    /// Taller than the view with its margins, by its leading edge, which
    /// lies in the view with its margin ([`leads`]).
    Leading,
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
    /// How the last frame showed the control holding focus, if it did:
    /// whole in the container's view, or, for one taller than the view, by
    /// its leading edge, so a resize that cuts it is what cut it: judged at
    /// the offset a reveal the frame requested sets once the frame is
    /// painted (`pending`).
    shown: Rc<Cell<Option<Shown>>>,
    /// The offset a reveal requested in the frame being prepainted sets
    /// once that frame is painted ([`Self::request`]), which the frame's
    /// end judges `shown` at. Judged at the offset the frame was painted
    /// at, the control a resize had just cut counted as cut though the
    /// reveal shows it whole before another frame is drawn, so where a
    /// drag's next resize was laid out before the reveal's own frame, that
    /// resize revealed nothing and the control stayed cut for the rest of
    /// the drag.
    pending: Rc<Cell<Option<Pixels>>>,
    /// The focus handles [`Self::focus`] has handed out, by control id:
    /// those of the controls the last frame drew, of the one holding focus
    /// wherever it is, as one a person has scrolled out of a list that
    /// draws only what it shows, so it keeps focus and a reveal finds it
    /// again once it is drawn, and of any something else still holds
    /// ([`Self::release_undrawn`]).
    controls: Rc<RefCell<HashMap<ElementId, Kept>>>,
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

    /// The focus handle the control `id` tracks, the same one in every frame
    /// that draws it, so the container can tell which of its controls holds
    /// focus. A Button takes it through `track_focus`. A control no frame
    /// draws gives its handle up once that frame's controls are known,
    /// unless it holds focus or something else still holds the handle
    /// ([`Self::release_undrawn`]).
    pub(crate) fn focus(&self, id: impl Into<ElementId>, cx: &App) -> FocusHandle {
        let mut controls = self.controls.borrow_mut();
        let kept = controls.entry(id.into()).or_insert_with(|| Kept {
            focus: None,
            handed: false,
        });
        kept.handed = true;
        kept.focus.get_or_insert_with(|| cx.focus_handle()).clone()
    }

    /// Once a frame's controls are known: keeps the handles handed out for
    /// it ([`Self::focus`]), the one holding focus or containing it, and any
    /// that something else still holds, and gives up the rest. Each
    /// control's handle is the same in every frame that draws it, and focus
    /// scrolled out with its control stays on it, while a list walked from
    /// end to end keeps no handle for a row it no longer draws.
    ///
    /// The handle holding focus is kept whether or not anything else holds
    /// it: the focus the container last drew with (`observed`) is not it
    /// where focus moved while the container drew no frame, as when a
    /// handle held for a row focused it and was let go while the page was
    /// covered, and given up there, it was the row's last handle and the
    /// window lost focus. One something else holds is kept as long as it
    /// is held there, as by a dialog that returns focus to its row once it
    /// closes: the dialog may focus the row and let its handle go in one
    /// step, before any frame ends, so held only weakly by the reveal, that
    /// was the row's last handle and the window lost focus.
    fn release_undrawn(&self, window: &Window, cx: &App) {
        self.controls.borrow_mut().retain(|_, kept| {
            let Some(focus) = kept.focus.take() else {
                return false;
            };
            if std::mem::take(&mut kept.handed)
                || focus.is_focused(window)
                || focus.contains_focused(window, cx)
            {
                kept.focus = Some(focus);
                return true;
            }
            // Let the reveal's own handle go: where nothing else holds it,
            // it is gone, and otherwise the reveal holds it again.
            let released = focus.downgrade();
            drop(focus);
            kept.focus = released.upgrade();
            kept.focus.is_some()
        });
    }

    /// How many control ids the reveal holds a handle for.
    #[cfg(test)]
    pub(crate) fn kept(&self) -> usize {
        self.controls.borrow().len()
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
            .find(|(_, kept)| {
                kept.focus
                    .as_ref()
                    .is_some_and(|focus| focus.is_focused(window))
            })
            .map(|(id, _)| id.clone())
    }

    /// For the render of the view that builds the container, in every frame:
    /// when focus has moved, in a way the trigger reveals, onto a control
    /// the last frame drew outside the container's view, scrolls the
    /// container now, so the frame being rendered lays the control out
    /// revealed and the first frame that draws the focus shows it whole.
    pub(crate) fn reveal(&self, window: &Window, cx: &App) {
        let observed = self.observation(window, cx);
        let Some(due) = self.due(&observed, window) else {
            return;
        };
        // A control in a revealing container of its own is revealed there
        // first, so this container shows it where that scroll has moved it.
        if let Some(inner) = self.drawn_within(window, cx) {
            inner.reveal(window, cx);
        }
        if let Some((bounds, margin)) = self.drawn_focus(window, cx) {
            let at = bounds + point(px(0.), self.scroll.offset().y);
            if let Some(offset) = self.target(at, margin, self.scroll.bounds(), due) {
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

    /// Whether a reveal is due, and why: focus has changed since the last
    /// frame in a way the trigger reveals; a page opening asked for its
    /// focus to be revealed ([`Self::reveal_next_focus`]) even onto the
    /// control it last drew focused; or, focus unchanged, the container's
    /// size or the text size has changed since a frame that showed the
    /// control whole, or a control taller than the view by its leading edge
    /// (`shown`), focus having been placed there in a way the trigger
    /// reveals, so the change is what cuts it.
    fn due(&self, observed: &Observation, window: &Window) -> Option<Due> {
        let last = self.observed.borrow();
        if self.next_focus.get() {
            Some(Due::Focus)
        } else if last.focus != observed.focus {
            self.revealing(window).then_some(Due::Focus)
        } else if *last != *observed && self.placed.get() {
            self.shown.get().map(Due::Resize)
        } else {
            None
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
    /// drew with, and gives up the handles of controls it did not draw
    /// ([`Self::release_undrawn`]).
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
        // After the last frame's items have given way to this frame's, so
        // they hold no handle this frame did not hand out.
        self.release_undrawn(window, cx);
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
        // A control taller than the view, which a reveal shows by its
        // leading edge, is shown while that edge is, with its margin, as the
        // edge a resize leaves where it is (`target`): never whole, it
        // counted as cut, so a resize that made room for it left it cut.
        self.shown
            .set(self.drawn_focus(window, cx).and_then(|(bounds, margin)| {
                let drawn = bounds + point(px(0.), offset);
                if drawn.top() + slack >= viewport.top()
                    && drawn.bottom() <= viewport.bottom() + slack
                {
                    Some(Shown::Whole)
                } else if tall(drawn, margin, viewport) && leads(drawn, margin, viewport) {
                    Some(Shown::Leading)
                } else {
                    None
                }
            }));
        *self.observed.borrow_mut() = observed;
        self.next_focus.set(false);
        #[cfg(test)]
        self.painted.borrow_mut().push(tests::Painted {
            focus: window.focused(cx),
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
    /// the container can move at all. A control taller than the view, with
    /// its margins, shows its leading edge, which holds its label and first
    /// action: its top `margin` below the view's, from wherever it lies, so
    /// the render's reveal and the request its bounds make agree. Shown by
    /// the least scroll, it showed its foot when it lay below the view, and
    /// the request then scrolled back to its top, so the page swung between
    /// its edges. After a resize (`due`) that follows a frame showing such a
    /// control by its leading edge, it stays where it is while that edge
    /// lies in the view with its margin: only a resize that cuts the edge
    /// reveals it by it again. One the last frame showed whole is revealed
    /// by its leading edge wherever a resize leaves it too tall to fit:
    /// kept where it was, a 500 px control Tab had shown whole kept 153 px
    /// of itself in a view a resize had cut to 450 px.
    fn target(
        &self,
        bounds: Bounds<Pixels>,
        margin: Pixels,
        viewport: Bounds<Pixels>,
        due: Due,
    ) -> Option<Point<Pixels>> {
        if viewport.size.height <= Pixels::ZERO {
            return None;
        }
        let delta = if tall(bounds, margin, viewport) {
            if due == Due::Resize(Shown::Leading) && leads(bounds, margin, viewport) {
                return None;
            }
            let delta = viewport.top() - (bounds.top() - margin);
            // Already there, to within what layout's arithmetic leaves.
            if delta.abs() < px(0.01) {
                return None;
            }
            delta
        } else if bounds.top() - margin < viewport.top() {
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
        let Some(due) = self.due(&observed, window) else {
            return;
        };
        let viewport = self.scroll.bounds();
        let Some(offset) = self.target(bounds, margin, viewport, due) else {
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

/// Whether a control at `bounds` is taller than `viewport` with `margin`
/// above and below it, so a reveal shows its leading edge.
fn tall(bounds: Bounds<Pixels>, margin: Pixels, viewport: Bounds<Pixels>) -> bool {
    bounds.size.height + margin * 2. > viewport.size.height
}

/// Whether the leading edge of a control at `bounds` lies in `viewport` with
/// `margin` on either side of it: where a frame counts a tall control as
/// shown ([`Shown::Leading`]) and where a resize after such a frame leaves
/// it. Counted shown wherever its top lay above the view's foot, a control
/// the person had scrolled to within a pixel or two of that foot was then
/// scrolled almost a whole view by a 1 px wider window.
fn leads(bounds: Bounds<Pixels>, margin: Pixels, viewport: Bounds<Pixels>) -> bool {
    let slack = px(0.01);
    bounds.top() - margin + slack >= viewport.top()
        && bounds.top() + margin <= viewport.bottom() + slack
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::tags::tests::{content_mask, draw, holds, rendered};
    use core::prelude::v1::test;
    use gpui_kit::component::Theme;

    /// One frame the container was prepainted and painted in, as it ended:
    /// the window's focus, where the control holding focus was drawn, if the
    /// container drew it, the container's visible bounds, and its offset and
    /// reach.
    #[derive(Clone, Debug)]
    pub(crate) struct Painted {
        pub(crate) focus: Option<FocusHandle>,
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

    /// The height of [`Tall`]'s page's view.
    const VIEW: f32 = 200.;

    /// A revealing page, [`VIEW`] tall, of a short control, a control taller
    /// than the view and another short control, so far apart that each lies
    /// outside the view while either of the others is shown.
    struct Tall {
        page: FocusReveal,
        controls: [FocusHandle; 3],
    }

    impl Render for Tall {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.page.reveal(window, cx);
            let room = ring_room_of(cx);
            let control = |index: usize, height: f32| {
                let focus = self.controls[index].clone();
                div()
                    .flex_shrink_0()
                    .on_children_prepainted(self.page.control(focus.clone(), room))
                    .child(
                        div()
                            .id(("tall-control", index))
                            .track_focus(&focus.tab_stop(true))
                            .h(px(height)),
                    )
            };
            let gap = || div().flex_shrink_0().h(px(VIEW * 1.5));
            div().size_full().child(
                div()
                    .on_children_prepainted(self.page.observe())
                    .id("tall-page")
                    .h(px(VIEW))
                    .overflow_y_scroll()
                    .track_scroll(self.page.scroll())
                    .flex()
                    .flex_col()
                    .child(control(0, 20.))
                    .child(gap())
                    .child(control(1, VIEW * 2.5))
                    .child(gap())
                    .child(control(2, 20.)),
            )
        }
    }

    /// [`ring_room`] where a view renders.
    fn ring_room_of(cx: &App) -> Pixels {
        let ring = Theme::global(cx).button_focus_ring;
        ring.gap + ring.width
    }

    /// A control taller than its page's view, reached by Tab from the
    /// control above it, which leaves it below the view, and by Shift+Tab
    /// from the control below it, which leaves it above: in every frame
    /// painted after the key, ten and more, the page holds one offset, the
    /// one that lines the control's top up with the view's top with the
    /// ring's room. Shown by the least scroll, the first frame after Tab
    /// showed its foot and the request its bounds made scrolled back to its
    /// top a frame later, and after Shift+Tab the request scrolled from its
    /// top to its foot.
    #[gpui::test]
    fn a_control_taller_than_the_view_shows_its_top_and_stays(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let page = FocusReveal::new(Trigger::Keyboard);
        let built = page.clone();
        let handles: Rc<RefCell<Vec<FocusHandle>>> = Rc::default();
        let kept = handles.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let view = cx.new(|cx| {
                let controls = [cx.focus_handle(), cx.focus_handle(), cx.focus_handle()];
                *kept.borrow_mut() = controls.to_vec();
                Tall {
                    page: built,
                    controls,
                }
            });
            gpui_kit::component::Root::new(view, window, cx)
        });
        let controls = handles.borrow().clone();
        let room = ring_room(cx);
        draw(cx);
        cx.update(|window, cx| window.focus(&controls[0], cx));
        draw(cx);
        // A key no binding takes, so the window has switched to keyboard
        // input before the walk. That switch leaves the window to be drawn
        // again, and GPUI draws a window left so before it dispatches a key,
        // with focus not yet moved: a frame that no platform presents, since
        // the frame request draws again after the key and presents only that.
        cx.simulate_keystrokes("f13");
        draw(cx);
        let focused = |cx: &mut VisualTestContext, index: usize| {
            cx.update(|window, _| controls[index].is_focused(window))
        };
        assert!(focused(cx, 0), "the first control holds focus");
        let slack = px(0.01);
        for (key, from) in [("tab", "below"), ("tab", ""), ("shift-tab", "above")] {
            page.take_painted();
            cx.simulate_keystrokes(key);
            for _ in 0..4 {
                draw(cx);
            }
            let frames = page.take_painted();
            if from.is_empty() {
                assert!(focused(cx, 2), "Tab moves on to the last control");
                continue;
            }
            assert!(focused(cx, 1), "{key} focuses the tall control");
            assert!(frames.len() >= 10, "{key} paints {} frames", frames.len());
            let settled = frames[0].offset;
            assert!(settled < px(0.), "{key} scrolls the page");
            for (frame, painted) in frames.iter().enumerate() {
                let control = painted
                    .control
                    .unwrap_or_else(|| panic!("frame {frame} after {key} draws the control"));
                assert!(
                    control.size.height + room * 2. > painted.viewport.size.height,
                    "the control, {control:?}, is taller than the view, {:?}",
                    painted.viewport
                );
                assert!(
                    (painted.offset - settled).abs() < slack
                        && (control.top() - room - painted.viewport.top()).abs() < slack,
                    "frame {frame} after {key} from {from} the view is at offset {:?}, \
                     not {settled:?}, and draws the control's top at {:?}, not {:?}",
                    painted.offset,
                    control.top(),
                    painted.viewport.top() + room
                );
            }
            for frame in [2, 5, 10] {
                assert_eq!(
                    frames[frame - 1].offset,
                    settled,
                    "{key}: the offset after frame {frame}"
                );
            }
        }
    }

    /// The height of [`Resizable`]'s tall control.
    const TALL: f32 = 500.;

    /// A revealing page as large as its window, of a short control, a gap as
    /// tall as the window is wide, a control [`TALL`] px tall, a gap and
    /// another short control: the window's height is the view's, and
    /// narrowing the window moves the tall control up the page by as much.
    struct Resizable {
        page: FocusReveal,
        controls: [FocusHandle; 3],
    }

    impl Render for Resizable {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.page.reveal(window, cx);
            let room = ring_room_of(cx);
            let control = |index: usize, height: f32| {
                let focus = self.controls[index].clone();
                div()
                    .flex_shrink_0()
                    .on_children_prepainted(self.page.control(focus.clone(), room))
                    .child(
                        div()
                            .id(("resizable-control", index))
                            .track_focus(&focus.tab_stop(true))
                            .h(px(height)),
                    )
            };
            div().size_full().child(
                div()
                    .on_children_prepainted(self.page.observe())
                    .id("resizable-page")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(self.page.scroll())
                    .flex()
                    .flex_col()
                    .child(control(0, 20.))
                    .child(div().flex_shrink_0().h(window.viewport_size().width))
                    .child(control(1, TALL))
                    .child(div().flex_shrink_0().h(px(TALL)))
                    .child(control(2, 20.)),
            )
        }
    }

    /// A [`Resizable`] page at 600 × 400 with the keyboard's focus on its
    /// first control, its three controls' handles, and the room the
    /// installed ring takes.
    fn resizable(
        cx: &mut TestAppContext,
    ) -> (
        FocusReveal,
        Vec<FocusHandle>,
        Pixels,
        &mut VisualTestContext,
    ) {
        cx.update(gpui_kit::init);
        let page = FocusReveal::new(Trigger::Keyboard);
        let built = page.clone();
        let handles: Rc<RefCell<Vec<FocusHandle>>> = Rc::default();
        let kept = handles.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let view = cx.new(|cx| {
                let controls = [cx.focus_handle(), cx.focus_handle(), cx.focus_handle()];
                *kept.borrow_mut() = controls.to_vec();
                Resizable {
                    page: built,
                    controls,
                }
            });
            gpui_kit::component::Root::new(view, window, cx)
        });
        let controls = handles.borrow().clone();
        let room = ring_room(cx);
        cx.simulate_resize(size(px(600.), px(400.)));
        draw(cx);
        cx.update(|window, cx| window.focus(&controls[0], cx));
        draw(cx);
        // A key no binding takes, so the window has switched to keyboard
        // input before the walk, as in the test above.
        cx.simulate_keystrokes("f13");
        draw(cx);
        (page, controls, room, cx)
    }

    /// The last frame the page painted once drawing has settled: where it
    /// drew the focused control, and its view.
    fn settled(cx: &mut VisualTestContext, page: &FocusReveal) -> (Bounds<Pixels>, Bounds<Pixels>) {
        page.take_painted();
        draw(cx);
        let frame = page.take_painted().pop().expect("the page paints");
        (
            frame.control.expect("the page draws the focused control"),
            frame.viewport,
        )
    }

    /// The window resized to `width` × `height`, drawn until it settles.
    fn resize(cx: &mut VisualTestContext, width: f32, height: f32) {
        cx.simulate_resize(size(px(width), px(height)));
        for _ in 0..3 {
            draw(cx);
        }
    }

    /// Tab from the first control onto the tall one, which the page shows
    /// by its top, the ring's room below the view's top.
    fn tab_onto_tall(
        cx: &mut VisualTestContext,
        page: &FocusReveal,
        controls: &[FocusHandle],
        room: Pixels,
    ) {
        cx.simulate_keystrokes("tab");
        draw(cx);
        assert!(
            cx.update(|window, _| controls[1].is_focused(window)),
            "Tab focuses the tall control"
        );
        let (control, view) = settled(cx, page);
        assert!(
            control.size.height + room * 2. > view.size.height
                && (control.top() - room - view.top()).abs() < px(0.01),
            "Tab shows the tall control {control:?} by its top in {view:?}"
        );
    }

    /// A control taller than the view, focused by Tab and revealed by its
    /// top, counts as shown while its top is in view, so a resize after
    /// which it would fit whole but is cut reveals it whole: narrowed, the
    /// window moves it up the page, its top 97 px above the view it now
    /// fits in. Counted as cut because it never fitted whole, it stayed
    /// there.
    #[gpui::test]
    fn a_resize_that_makes_room_for_a_tall_control_reveals_it_whole(cx: &mut TestAppContext) {
        let (page, controls, room, cx) = resizable(cx);
        tab_onto_tall(cx, &page, &controls, room);
        resize(cx, 500., 800.);
        let slack = px(0.01);
        let (control, view) = settled(cx, &page);
        assert!(
            control.size.height + room * 2. <= view.size.height
                && control.top() - room + slack >= view.top()
                && control.bottom() + room <= view.bottom() + slack,
            "a resize that makes room for the tall control shows it, {control:?}, whole in \
             {view:?}"
        );
        assert!(
            cx.update(|window, _| controls[1].is_focused(window)),
            "the tall control keeps focus"
        );
    }

    /// A control taller than the view, focused by Tab and revealed by its
    /// top, counts as shown while its top is in view, so a resize that cuts
    /// that top, narrowing and shortening the window to leave it 97 px above
    /// a view it is still taller than, reveals it by its top again, and one
    /// that leaves the top in view with the ring's room, here where the
    /// person has scrolled it 40 px down the view, leaves the page where it
    /// is. Counted as cut because it never fitted whole, it stayed where the
    /// resize put it, its top out of view.
    #[gpui::test]
    fn a_resize_that_cuts_a_tall_controls_top_reveals_it_by_its_top(cx: &mut TestAppContext) {
        let (page, controls, room, cx) = resizable(cx);
        tab_onto_tall(cx, &page, &controls, room);
        let slack = px(0.01);
        resize(cx, 500., 350.);
        let (control, view) = settled(cx, &page);
        assert!(
            control.size.height + room * 2. > view.size.height
                && (control.top() - room - view.top()).abs() < slack,
            "a resize that cuts the tall control's top shows it, {control:?}, by its top \
             again in {view:?}"
        );

        // The person scrolls its top 40 px down the view; a resize that
        // leaves the top in view moves nothing.
        let scrolled = page.scroll().offset() + point(px(0.), px(40.));
        cx.update(|window, _| {
            page.scroll().set_offset(scrolled);
            window.refresh();
        });
        draw(cx);
        resize(cx, 500., 330.);
        assert_eq!(
            page.scroll().offset(),
            scrolled,
            "a resize that leaves the tall control's top in view leaves the page"
        );
    }

    /// A control Tab showed whole, which a resize then makes taller than the
    /// view, is revealed by its top, as before the rule that leaves a tall
    /// control shown by its top where a resize keeps that top in view: in a
    /// 600 × 800 window Tab shows the 500 px control whole, its top 297 px
    /// down the view, and shortened to 450 px the window kept it there, 153
    /// of its 500 px in view.
    #[gpui::test]
    fn a_resize_that_makes_a_whole_control_tall_reveals_it_by_its_top(cx: &mut TestAppContext) {
        let (page, controls, room, cx) = resizable(cx);
        resize(cx, 600., 800.);
        cx.simulate_keystrokes("tab");
        draw(cx);
        assert!(
            cx.update(|window, _| controls[1].is_focused(window)),
            "Tab focuses the tall control"
        );
        let slack = px(0.01);
        let (control, view) = settled(cx, &page);
        assert!(
            control.size.height + room * 2. <= view.size.height
                && control.top() - room > view.top() + slack
                && (control.bottom() + room - view.bottom()).abs() < slack,
            "Tab shows the control {control:?} whole at the foot of {view:?}"
        );
        resize(cx, 600., 450.);
        let (control, view) = settled(cx, &page);
        assert!(
            control.size.height + room * 2. > view.size.height
                && (control.top() - room - view.top()).abs() < slack,
            "a resize that makes the control taller than the view shows it, {control:?}, by \
             its top in {view:?}"
        );
    }

    /// A control taller than the view counts as shown by its top only while
    /// that top lies in the view with the ring's room, where a resize leaves
    /// it: scrolled by the person until its top is 2 px above the view's
    /// foot, a 1 px wider window leaves the page where it is. Counted as
    /// shown wherever its top lay above the foot, that resize scrolled the
    /// page almost a whole view to show the top at the view's top.
    #[gpui::test]
    fn a_tall_control_scrolled_to_the_views_foot_is_left_there(cx: &mut TestAppContext) {
        let (page, controls, room, cx) = resizable(cx);
        tab_onto_tall(cx, &page, &controls, room);
        // The tall control's top, 620 px down the page, 2 px above the foot
        // of the 400 px view.
        let scrolled = point(px(0.), px(-222.));
        cx.update(|window, _| {
            page.scroll().set_offset(scrolled);
            window.refresh();
        });
        draw(cx);
        let (control, view) = settled(cx, &page);
        assert!(
            (view.bottom() - control.top() - px(2.)).abs() < px(0.01),
            "the person scrolls the control's top, {control:?}, 2 px above the foot of {view:?}"
        );
        resize(cx, 601., 400.);
        assert_eq!(
            page.scroll().offset(),
            scrolled,
            "a 1 px wider window leaves the page where the person scrolled it"
        );
    }

    /// A control taller than the view that a click focused is never
    /// scrolled: not by the click, which leaves its top 50 px above the
    /// view, nor by a resize after which it would fit whole but is cut.
    #[gpui::test]
    fn a_tall_control_focused_by_a_click_is_never_scrolled(cx: &mut TestAppContext) {
        let (page, controls, _, cx) = resizable(cx);
        // The tall control's top, 620 px down the page, 50 px above the view.
        let away = point(px(0.), px(-670.));
        cx.update(|window, _| {
            page.scroll().set_offset(away);
            window.refresh();
        });
        draw(cx);
        cx.simulate_click(point(px(300.), px(200.)), Modifiers::default());
        draw(cx);
        assert!(
            cx.update(|window, _| controls[1].is_focused(window)),
            "the click focuses the tall control"
        );
        assert_eq!(page.scroll().offset(), away, "the click scrolls nothing");
        resize(cx, 500., 800.);
        let (control, view) = settled(cx, &page);
        assert!(
            control.size.height < view.size.height && control.top() < view.top(),
            "the resize leaves the control {control:?}, which would fit, cut in {view:?}"
        );
        assert_eq!(page.scroll().offset(), away, "the resize scrolls nothing");
    }

    /// How many rows [`Long`]'s list has, and how tall each is.
    const ROWS: usize = 500;
    const ROW: f32 = 20.;
    /// The rows [`Long`] draws past each end of its view.
    const OVERSCAN: usize = 2;
    /// The most rows [`Long`] draws in a frame.
    const LONG_DRAWN: usize = (VIEW / ROW) as usize + 1 + 2 * OVERSCAN;

    /// A revealing list of [`ROWS`] rows, [`VIEW`] tall, that draws only
    /// the rows in its view and [`OVERSCAN`] past each end, as a list that
    /// draws only what it shows does, each row tracking the handle its
    /// reveal hands out for it. While `hidden`, it draws nothing, as a page
    /// another page covers.
    struct Long {
        page: FocusReveal,
        hidden: Rc<Cell<bool>>,
    }

    impl Render for Long {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            if self.hidden.get() {
                return div().into_any_element();
            }
            self.page.reveal(window, cx);
            let room = ring_room_of(cx);
            let top = (-f32::from(self.page.scroll().offset().y) / ROW) as usize;
            let first = top.saturating_sub(OVERSCAN);
            let end = (top + LONG_DRAWN - OVERSCAN).min(ROWS);
            let rows = (first..end).map(|index| {
                let focus = self.page.focus(("long-row", index), cx);
                div()
                    .flex_shrink_0()
                    .on_children_prepainted(self.page.control(focus.clone(), room))
                    .child(
                        div()
                            .id(("long-row", index))
                            .debug_selector(move || format!("long-row-{index}"))
                            .track_focus(&focus.tab_stop(true))
                            .h(px(ROW)),
                    )
            });
            div()
                .size_full()
                .child(
                    div()
                        .on_children_prepainted(self.page.observe())
                        .id("long-page")
                        .debug_selector(|| "long-page".into())
                        .h(px(VIEW))
                        .overflow_y_scroll()
                        .track_scroll(self.page.scroll())
                        .flex()
                        .flex_col()
                        .child(div().flex_shrink_0().h(px(first as f32 * ROW)))
                        .children(rows)
                        .child(div().flex_shrink_0().h(px((ROWS - end) as f32 * ROW))),
                )
                .into_any_element()
        }
    }

    /// A window drawing [`Long`] with `page`, and its switch that hides the
    /// list.
    fn long<'a>(
        cx: &'a mut TestAppContext,
        page: &FocusReveal,
    ) -> (&'a mut VisualTestContext, Rc<Cell<bool>>) {
        cx.update(gpui_kit::init);
        let hidden: Rc<Cell<bool>> = Rc::default();
        let (page, switch) = (page.clone(), hidden.clone());
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let view = cx.new(|_| Long {
                page,
                hidden: switch,
            });
            gpui_kit::component::Root::new(view, window, cx)
        });
        draw(cx);
        (cx, hidden)
    }

    /// Scrolls [`Long`]'s list so row `top` lies at the top of its view, and
    /// draws.
    fn scroll_long_to(cx: &mut VisualTestContext, page: &FocusReveal, top: usize) {
        cx.update(|window, _| {
            page.scroll()
                .set_offset(point(px(0.), -px(top as f32 * ROW)));
            window.refresh();
        });
        draw(cx);
    }

    /// Whether the window has focus, and the row of `page` that holds it.
    fn long_focus(cx: &mut VisualTestContext, page: &FocusReveal) -> (bool, Option<ElementId>) {
        let window = cx.update(|window, cx| window.focused(cx).is_some());
        (window, focused(cx, page))
    }

    /// A list walked from end to end keeps a handle only for the rows a
    /// frame draws and the one holding focus, which keeps its focus
    /// scrolled out: in a list of 500 rows, 200 px tall, that draws only
    /// what it shows and two rows past each end, row 3 is focused, and the
    /// list is scrolled seven rows at a time to its end and back to where
    /// row 3 is drawn above the view. After every step the reveal holds
    /// handles for at most the 15 rows a frame draws, those of the frame
    /// before, whose tab stops GPUI holds until the next frame, row 3,
    /// which holds focus throughout, and row 8, whose handle the test holds
    /// throughout, as a dialog that returns focus to it would; back where
    /// it is drawn, Tab moves on to row 4 and Shift+Tab back to row 3, each
    /// revealed with its ring, and row 8 is handed the same handle again.
    /// Kept for every row ever handed out, the walk left 500 handles.
    #[gpui::test]
    fn a_walked_list_keeps_handles_for_the_drawn_rows_and_the_focused_one(cx: &mut TestAppContext) {
        let page = FocusReveal::new(Trigger::Keyboard);
        let (cx, _) = long(cx, &page);
        let row = |index: usize| control("long-row", index);
        cx.update(|window, cx| page.focus(row(3).0, cx).focus(window, cx));
        draw(cx);
        assert_eq!(focused(cx, &page), Some(row(3).0), "row 3 holds focus");
        let elsewhere = cx.update(|_, cx| page.focus(row(8).0, cx));
        let last = ROWS - (VIEW / ROW) as usize;
        // Down to the end and back up to row 5, where row 3 is drawn above
        // the view.
        let walk = (0..last + 1)
            .step_by(7)
            .chain((5..last + 1).step_by(7).rev());
        for top in walk {
            scroll_long_to(cx, &page, top);
            let kept = page.kept();
            assert!(
                kept <= 2 * LONG_DRAWN + 2,
                "scrolled to row {top}, the reveal holds {kept} handles"
            );
            assert_eq!(
                focused(cx, &page),
                Some(row(3).0),
                "scrolled to row {top}, row 3 keeps focus"
            );
        }
        assert!(
            cx.debug_bounds("long-row-3").is_some(),
            "back at row 5, row 3 is drawn"
        );
        assert!(
            cx.update(|window, cx| page.holds_focus(window, cx)),
            "back at row 5, the list holds focus on row 3"
        );
        assert!(
            cx.update(|_, cx| page.focus(row(8).0, cx)) == elsewhere,
            "back at row 5, row 8 is handed the handle held throughout"
        );
        // A key no binding takes, so the window has switched to keyboard
        // input before the keys that move focus.
        cx.simulate_keystrokes("f13");
        draw(cx);
        cx.simulate_keystrokes("tab");
        draw(cx);
        assert_revealed(cx, &page, "long-page", &row(4), "tab from row 3");
        cx.simulate_keystrokes("shift-tab");
        draw(cx);
        assert_revealed(cx, &page, "long-page", &row(3), "shift-tab from row 4");
    }

    /// Focus moved onto a row while its list draws nothing stays on it
    /// when the list draws again without the row: row 3 is drawn, the list
    /// is hidden for a frame, as a page another page covers, and a handle
    /// held for row 3 focuses it and is let go; the list then draws
    /// scrolled to row 100. Row 3 keeps focus, held by the reveal alone.
    /// Given up because the frame does not draw it, the row's last handle
    /// went and the window had no focus.
    #[gpui::test]
    fn focus_moved_onto_a_row_while_its_list_is_hidden_stays_there(cx: &mut TestAppContext) {
        let page = FocusReveal::new(Trigger::Keyboard);
        let (cx, hidden) = long(cx, &page);
        let row = |index: usize| control("long-row", index);
        let held = cx.update(|_, cx| page.focus(row(3).0, cx));
        draw(cx);
        assert!(cx.debug_bounds("long-row-3").is_some(), "row 3 is drawn");
        hidden.set(true);
        cx.update(|window, _| window.refresh());
        draw(cx);
        cx.update(|window, cx| held.focus(window, cx));
        drop(held);
        draw(cx);
        assert_eq!(
            long_focus(cx, &page),
            (true, Some(row(3).0)),
            "hidden, the list's row 3 holds focus"
        );
        hidden.set(false);
        scroll_long_to(cx, &page, 100);
        assert!(
            cx.debug_bounds("long-row-3").is_none(),
            "row 3 is not drawn"
        );
        assert_eq!(
            long_focus(cx, &page),
            (true, Some(row(3).0)),
            "drawn without it, row 3 keeps focus"
        );
    }

    /// A row a dialog returns focus to keeps it after the row has scrolled
    /// out of its list while the dialog was open: row 3 holds focus, a
    /// dialog takes focus and holds row 3's handle to return it to, and the
    /// list scrolls to row 100 under it; the dialog closes, focusing row 3
    /// and letting its handle go at once. Row 3 holds focus, and once the
    /// list scrolls back it is drawn holding it. Held by the reveal only
    /// weakly while the dialog held it, its last handle went as the dialog
    /// closed and the window had no focus.
    #[gpui::test]
    fn a_row_a_dialog_returns_focus_to_keeps_it_after_scrolling_out(cx: &mut TestAppContext) {
        let page = FocusReveal::new(Trigger::Keyboard);
        let (cx, _) = long(cx, &page);
        let row = |index: usize| control("long-row", index);
        cx.update(|window, cx| page.focus(row(3).0, cx).focus(window, cx));
        draw(cx);
        let dialog = cx.update(|_, cx| cx.focus_handle());
        let returned = cx.update(|window, cx| {
            let returned = window.focused(cx).expect("row 3 holds focus");
            dialog.focus(window, cx);
            returned
        });
        draw(cx);
        scroll_long_to(cx, &page, 100);
        draw(cx);
        assert!(
            cx.debug_bounds("long-row-3").is_none(),
            "row 3 is not drawn"
        );
        // The test's record of painted frames holds the focus of each; let
        // it go, so the dialog alone holds row 3's handle, as in the app.
        page.take_painted();
        cx.update(|window, cx| {
            returned.focus(window, cx);
            drop(returned);
        });
        draw(cx);
        assert_eq!(
            long_focus(cx, &page),
            (true, Some(row(3).0)),
            "the dialog closed, row 3 holds focus"
        );
        scroll_long_to(cx, &page, 0);
        assert!(
            cx.update(|window, cx| page.holds_focus(window, cx)),
            "scrolled back, the list draws row 3 holding focus"
        );
    }
}
