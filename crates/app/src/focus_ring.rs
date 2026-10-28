//! A list's or a pane's keyboard focus ring, kept while the pointer moves.
//!
//! GPUI's `focus_visible` styles a focused element only while the last input
//! was a key, so merely moving the pointer took the ring off a list that kept
//! keyboard focus. Here a key press shows the ring of whatever holds focus,
//! and the ring stays whatever the pointer does until a pointer press, as GTK
//! and browsers' `:focus-visible` behave: a click still focuses a list without
//! marking it, and the next key marks it. Every element that draws its own
//! focus asks [`FocusRing::shows`] rather than `focus_visible` or
//! `last_input_was_keyboard`; the [`sweep`] tabs through every page and
//! dialog and fails a stop that shows no whole ring.
use crate::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

/// One window's keyboard focus ring, owned by its [`GitTurtle`]. A clone
/// shares it, so a dialog built while the window draws, which cannot read its
/// owner then, keeps one to ask.
#[derive(Clone, Default)]
pub(crate) struct FocusRing {
    /// A key was pressed after the last pointer press. Shared with the
    /// window's pointer-press listener, [`FocusRing::pointer_listener`].
    keyboard: Rc<Cell<bool>>,
    /// The focus whose ring the frame being built draws.
    ringed: Rc<RefCell<Option<FocusHandle>>>,
}

impl FocusRing {
    /// Reads, before a frame builds its lists and dialogs, which focus draws
    /// its ring.
    pub(crate) fn begin_frame(&self, window: &Window, cx: &App) {
        // GPUI marks every key press as keyboard input and redraws the
        // window when input turns from the pointer to keys, so a frame
        // follows each press that shows the ring.
        if window.last_input_was_keyboard() {
            self.keyboard.set(true);
        }
        *self.ringed.borrow_mut() = self.keyboard.get().then(|| window.focused(cx)).flatten();
    }

    /// Whether `focus` draws its keyboard ring this frame.
    pub(crate) fn shows(&self, focus: &FocusHandle) -> bool {
        self.ringed.borrow().as_ref() == Some(focus)
    }

    /// The ring a pane draws on its own edge while `focus`, inside it, shows
    /// its ring: a 1 px outline in `color`, laid over the pane's content as
    /// the pane's last child, so nothing in the pane moves for it and the
    /// pane's own clip, if it has one, keeps it whole. The pane is
    /// `relative`. A text pane whose editor draws no edge of its own, such as
    /// Compare's, shows its focus this way.
    pub(crate) fn edge(&self, focus: &FocusHandle, color: impl Into<Hsla>) -> Option<Div> {
        self.shows(focus).then(|| {
            div()
                .absolute()
                .inset_0()
                .border_1()
                .border_color(color.into())
        })
    }

    /// An empty element whose paint ends the ring at any pointer press in the
    /// window, over a dialog's backdrop included: GPUI calls a window's mouse
    /// listeners for every press, whatever lies under the pointer.
    pub(crate) fn pointer_listener(&self) -> impl IntoElement {
        let keyboard = self.keyboard.clone();
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let keyboard = keyboard.clone();
                window.on_mouse_event(move |_: &MouseDownEvent, phase, window, _| {
                    if phase == DispatchPhase::Capture && keyboard.replace(false) {
                        window.refresh();
                    }
                });
            },
        )
        .absolute()
        .size_0()
    }
}

#[cfg(test)]
pub(crate) mod sweep;

/// An outline the last frame painted, read back for tests: its bounds and
/// width in logical pixels, its colour, which of its sides (top, right,
/// bottom, left) are drawn whole, whether any of it shows, and whether all
/// four sides are whole.
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct PaintedOutline {
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) width: Pixels,
    pub(crate) color: Hsla,
    pub(crate) sides: [bool; 4],
    pub(crate) drawn: bool,
    pub(crate) whole: bool,
}

/// The outlines of `color`, borders with no fill, that the last frame
/// painted. GPUI paints such a border as one strip per side, each masked to
/// its side and to every clip around it, and drops a strip a clip hides
/// whole. A side is whole when the strips' masks together cover its band,
/// its border's width along its whole length.
#[cfg(test)]
pub(crate) fn painted_outlines(
    cx: &mut gpui_kit::VisualTestContext,
    color: Hsla,
) -> Vec<PaintedOutline> {
    painted_outlines_where(cx, |painted| painted == color)
}

/// The outlines, as [`painted_outlines`] reads them, whose colour `matches`.
#[cfg(test)]
pub(crate) fn painted_outlines_where(
    cx: &mut gpui_kit::VisualTestContext,
    matches: impl Fn(Hsla) -> bool,
) -> Vec<PaintedOutline> {
    use gpui_kit::ScaledPixels;
    /// Whether `masks` together cover `band`: the masks that span its
    /// thickness, laid end to end along its length, reach both its ends.
    fn covers(masks: &[Bounds<ScaledPixels>], band: Bounds<ScaledPixels>, across: bool) -> bool {
        let (start, end) = if across {
            (band.left(), band.right())
        } else {
            (band.top(), band.bottom())
        };
        let mut spans: Vec<_> = masks
            .iter()
            .filter(|mask| {
                if across {
                    mask.top() <= band.top() && mask.bottom() >= band.bottom()
                } else {
                    mask.left() <= band.left() && mask.right() >= band.right()
                }
            })
            .map(|mask| {
                if across {
                    (mask.left(), mask.right())
                } else {
                    (mask.top(), mask.bottom())
                }
            })
            .collect();
        spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let mut reach = start;
        for (from, to) in spans {
            if from > reach {
                break;
            }
            reach = reach.max(to);
        }
        band.size.width > ScaledPixels(0.) && band.size.height > ScaledPixels(0.) && reach >= end
    }
    cx.update(|window, _| {
        let scale = window.scale_factor();
        let logical = |value: ScaledPixels| px(value.as_f32() / scale);
        let strips: Vec<_> = window
            .painted_quads()
            .into_iter()
            .filter(|quad| quad.background.is_transparent() && matches(quad.border_color))
            .map(|quad| {
                (
                    (quad.bounds, quad.border_color),
                    quad.border_widths,
                    quad.content_mask.bounds,
                )
            })
            .collect();
        let mut outlines: Vec<PaintedOutline> = Vec::new();
        let mut seen = Vec::new();
        for ((bounds, color), widths, _) in &strips {
            if seen.contains(&(*bounds, *color)) {
                continue;
            }
            seen.push((*bounds, *color));
            let masks: Vec<_> = strips
                .iter()
                .filter(|(other, _, _)| *other == (*bounds, *color))
                .map(|(_, _, mask)| *mask)
                .collect();
            let band = |left, top, right, bottom| {
                Bounds::from_corners(point(left, top), point(right, bottom))
            };
            let (b, w) = (bounds, widths);
            let bands = [
                (band(b.left(), b.top(), b.right(), b.top() + w.top), true),
                (
                    band(b.right() - w.right, b.top(), b.right(), b.bottom()),
                    false,
                ),
                (
                    band(b.left(), b.bottom() - w.bottom, b.right(), b.bottom()),
                    true,
                ),
                (
                    band(b.left(), b.top(), b.left() + w.left, b.bottom()),
                    false,
                ),
            ];
            let sides = bands.map(|(band, across)| covers(&masks, band, across));
            // A rounded outline's strips reach inside its bands, so a clip
            // can keep part of a strip while hiding every band.
            let drawn = bands.iter().any(|(band, _)| {
                masks.iter().any(|mask| {
                    let inside = mask.intersect(band);
                    inside.size.width > ScaledPixels(0.) && inside.size.height > ScaledPixels(0.)
                })
            });
            outlines.push(PaintedOutline {
                bounds: Bounds::from_corners(
                    point(logical(bounds.left()), logical(bounds.top())),
                    point(logical(bounds.right()), logical(bounds.bottom())),
                ),
                width: logical(widths.top),
                color: *color,
                sides,
                drawn,
                whole: sides.iter().all(|side| *side),
            });
        }
        outlines
    })
}

/// The focused kit Input whose text lies in `text` draws its focus whole in
/// the last frame: its own 1 px edge in the theme's ring colour, the accent,
/// and the kit's ring outside that edge, every side of both inside the clips
/// around the field.
#[cfg(test)]
pub(crate) fn assert_input_ringed(
    cx: &mut gpui_kit::VisualTestContext,
    text: Bounds<Pixels>,
    field: &str,
) {
    use gpui_kit::component::{FocusRing, Theme};
    let kit = FocusRing::default();
    let (edge_color, ring_color) = cx.read(|cx| {
        let ring = Theme::global(cx).ring;
        (ring, ring.alpha(kit.opacity))
    });
    let edges: Vec<_> = painted_outlines(cx, edge_color)
        .into_iter()
        .filter(|edge| edge.bounds.contains(&text.center()))
        .collect();
    let [edge] = edges[..] else {
        panic!("{field}: one accent edge around the field, {text:?}: {edges:?}");
    };
    assert_eq!(edge.width, px(1.), "{field}: {edge:?}");
    assert!(
        edge.whole,
        "{field}: a clip cuts the field's edge: {edge:?}"
    );
    let around = edge.bounds.dilate(kit.gap + kit.width);
    let slack = px(1.);
    let rings: Vec<_> = painted_outlines(cx, ring_color)
        .into_iter()
        .filter(|ring| {
            [
                (ring.bounds.left(), around.left()),
                (ring.bounds.top(), around.top()),
                (ring.bounds.right(), around.right()),
                (ring.bounds.bottom(), around.bottom()),
            ]
            .into_iter()
            .all(|(drawn, expected)| (drawn - expected).abs() <= slack)
        })
        .collect();
    let [ring] = rings[..] else {
        panic!("{field}: one ring around the field's edge, {around:?}: {rings:?}");
    };
    assert!(
        ring.whole,
        "{field}: a clip cuts the field's ring: {ring:?}"
    );
}
