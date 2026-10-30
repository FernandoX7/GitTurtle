use std::{cell::Cell, rc::Rc};

use gpui::{
    Anchor, AnyElement, App, Bounds, Div, ElementId, InteractiveElement, Interactivity,
    IntoElement, ParentElement, Pixels, Point, RenderOnce, StatefulInteractiveElement,
    StyleRefinement, Styled, Window, deferred, div, px,
};

use crate::{ElementExt as _, Positioner, StyledExt as _};

/// Distance kept between a popup and the window edge.
const WINDOW_MARGIN: Pixels = px(8.);

/// Deferred paint priority for interactive surfaces that must appear above dialogs.
pub const POPUP_PRIORITY: usize = 100;

/// GitTurtle patch: the room an application keeps clear of popups at a
/// window's foot, from the viewport's bottom edge ([`set_popup_bottom_inset`]).
struct PopupBottomInset(Rc<dyn Fn(&Window) -> Pixels>);

impl gpui::Global for PopupBottomInset {}

/// GitTurtle patch: keeps every popup menu and popover a window's [`Popup`]
/// host or a context menu places clear of the room `inset` gives for that
/// window, measured up from the viewport's bottom edge, such as a status bar
/// and the margin kept above it. The host's own margin still applies where it
/// is larger. Unset, a popup keeps only that margin, as upstream does.
pub fn set_popup_bottom_inset(cx: &mut App, inset: impl Fn(&Window) -> Pixels + 'static) {
    cx.set_global(PopupBottomInset(Rc::new(inset)));
}

/// GitTurtle patch: the room [`set_popup_bottom_inset`] keeps clear of popups
/// at `window`'s foot, or nothing where no application set it.
pub fn popup_bottom_inset(window: &Window, cx: &App) -> Pixels {
    cx.try_global::<PopupBottomInset>()
        .map_or(Pixels::ZERO, |inset| (inset.0)(window))
}

#[derive(Default)]
struct PopupAnchorState {
    bounds: Bounds<Pixels>,
    captured: bool,
}

/// An unstyled trigger and anchored popup host.
///
/// `Popup` owns trigger measurement, anchor-point calculation, first-frame
/// synchronization, deferred rendering, and window-edge snapping. Callers own
/// open state, interaction, popup content, appearance, and motion.
#[derive(IntoElement)]
pub struct Popup {
    id: ElementId,
    base: gpui::Stateful<Div>,
    style: StyleRefinement,
    anchor: Anchor,
    trigger: AnyElement,
    content: Option<AnyElement>,
}

impl Popup {
    pub fn new(id: impl Into<ElementId>, trigger: impl IntoElement) -> Self {
        let id = id.into();
        Self {
            base: div().id(id.clone()),
            id,
            style: StyleRefinement::default(),
            anchor: Anchor::TopLeft,
            trigger: trigger.into_any_element(),
            content: None,
        }
    }

    pub fn anchor(mut self, anchor: impl Into<Anchor>) -> Self {
        self.anchor = anchor.into();
        self
    }

    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }

    pub fn resolved_corner(anchor: Anchor, trigger_bounds: Bounds<Pixels>) -> Point<Pixels> {
        match anchor {
            Anchor::TopLeft => trigger_bounds.origin,
            Anchor::TopCenter => trigger_bounds.top_center(),
            Anchor::TopRight => trigger_bounds.top_right(),
            Anchor::BottomLeft => Point {
                x: trigger_bounds.origin.x,
                y: trigger_bounds.origin.y - trigger_bounds.size.height,
            },
            Anchor::BottomCenter => Point {
                x: trigger_bounds.top_center().x,
                y: trigger_bounds.origin.y - trigger_bounds.size.height,
            },
            Anchor::BottomRight => Point {
                x: trigger_bounds.top_right().x,
                y: trigger_bounds.origin.y - trigger_bounds.size.height,
            },
            Anchor::LeftCenter | Anchor::RightCenter => trigger_bounds.origin,
        }
    }
}

impl Styled for Popup {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl InteractiveElement for Popup {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Popup {}

impl RenderOnce for Popup {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state =
            window.use_keyed_state((self.id, "anchor"), cx, |_, _| PopupAnchorState::default());
        let anchor = self.anchor;
        let position = Rc::new(Cell::new(Self::resolved_corner(
            anchor,
            state.read(cx).bounds,
        )));
        // GitTurtle patch: only content held back for the trigger's bounds
        // waits for the next frame. A closed popup has none, and a trigger
        // first drawn closed, as review options that fold into their menu at
        // a new width, asked for a frame that drew nothing new.
        let waiting = self.content.is_some() && !state.read(cx).captured;

        let root = self
            .base
            .child(self.trigger)
            .on_prepaint({
                let state = state.clone();
                let position = position.clone();
                move |bounds, window, cx| {
                    position.set(Self::resolved_corner(anchor, bounds));
                    state.update(cx, |state, _| {
                        state.bounds = bounds;
                        state.captured = true;
                    });
                    if waiting {
                        window.request_animation_frame();
                    }
                }
            })
            .refine_style(&self.style);

        let Some(content) = self.content else {
            return root;
        };
        if !state.read(cx).captured {
            return root;
        }

        root.child(
            deferred(
                // GitTurtle patch: placed at the trigger's corner as this
                // frame lays the trigger out, which its prepaint records
                // before deferred content is prepainted. Taken from the frame
                // before, an open popup stayed where a width step moved its
                // trigger from until some other change drew a frame.
                Positioner::corner_following(anchor, position)
                    .margin(WINDOW_MARGIN)
                    .bottom_inset(popup_bottom_inset(window, cx))
                    // The host blocks the mouse, so no caller has to remember:
                    // what a popup covers belongs to the popup.
                    .occlude()
                    .child(content),
            )
            .with_priority(POPUP_PRIORITY),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Context, Render, px};

    #[test]
    fn resolved_corner_preserves_existing_anchor_math() {
        let bounds = Bounds {
            origin: Point::new(px(100.), px(100.)),
            size: gpui::Size::new(px(200.), px(50.)),
        };
        assert_eq!(
            Popup::resolved_corner(Anchor::TopCenter, bounds),
            Point::new(px(200.), px(100.))
        );
        assert_eq!(
            Popup::resolved_corner(Anchor::BottomRight, bounds),
            Point::new(px(300.), px(50.))
        );
    }

    struct Harness;

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            Popup::new(
                "popup",
                div()
                    .debug_selector(|| "popup-trigger".into())
                    .size(px(100.)),
            )
            .content(
                div()
                    .debug_selector(|| "popup-content".into())
                    .size(px(20.)),
            )
        }
    }

    /// A caller that styles its own surface — a hover card, a dropdown — does
    /// not have to remember to block the mouse. The host does it, so the panel
    /// a popup covers stops reacting to a pointer that is over the popup.
    struct OcclusionHarness {
        background_hovered: Rc<Cell<bool>>,
        content_hovered: Rc<Cell<bool>>,
    }

    impl Render for OcclusionHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let background = self.background_hovered.clone();
            let content = self.content_hovered.clone();
            div()
                .relative()
                .size(px(200.))
                .child(
                    div()
                        .id("background")
                        .absolute()
                        .size_full()
                        .on_mouse_move(move |_, _, _| background.set(true)),
                )
                .child(
                    Popup::new("popup", div().size(px(100.))).content(
                        div()
                            .id("content")
                            .size(px(40.))
                            .on_mouse_move(move |_, _, _| content.set(true)),
                    ),
                )
        }
    }

    #[gpui::test]
    fn the_popup_surface_blocks_the_panel_it_covers(cx: &mut gpui::TestAppContext) {
        let background_hovered = Rc::new(Cell::new(false));
        let content_hovered = Rc::new(Cell::new(false));
        let (_, window) = cx.add_window_view({
            let background_hovered = background_hovered.clone();
            let content_hovered = content_hovered.clone();
            move |_, _| OcclusionHarness {
                background_hovered,
                content_hovered,
            }
        });
        window.update(|window, cx| window.draw(cx).clear(cx));
        window.update(|window, cx| window.draw(cx).clear(cx));

        window.simulate_mouse_move(
            gpui::point(px(20.), px(110.)),
            None,
            gpui::Modifiers::default(),
        );
        assert!(!background_hovered.get());
        // The surface blocks what is behind it, not its own content: the
        // hitbox goes in ahead of the children, never over them.
        assert!(content_hovered.get());

        // The same pointer outside the surface still reaches the panel, so the
        // assertion above is about occlusion and not a dead listener.
        window.simulate_mouse_move(
            gpui::point(px(150.), px(180.)),
            None,
            gpui::Modifiers::default(),
        );
        assert!(background_hovered.get());
    }

    #[gpui::test]
    fn trigger_capture_enables_deferred_content_on_the_next_frame(cx: &mut gpui::TestAppContext) {
        let (_, window) = cx.add_window_view(|_, _| Harness);
        window.update(|window, cx| window.draw(cx).clear(cx));
        window.update(|window, cx| window.draw(cx).clear(cx));

        assert_eq!(
            window.debug_bounds("popup-trigger").unwrap().size,
            gpui::Size::new(px(100.), px(100.))
        );
        assert_eq!(
            window.debug_bounds("popup-content").unwrap().size,
            gpui::Size::new(px(20.), px(20.))
        );
    }

    /// GitTurtle patch: a popup 50 px in from the window's top and trailing
    /// edges, where a narrower window moves its trigger, keyed by `id` and
    /// open while `open` holds.
    struct Trailing {
        id: Rc<Cell<&'static str>>,
        open: Rc<Cell<bool>>,
    }

    impl Render for Trailing {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let popup = Popup::new(
                self.id.get(),
                div()
                    .debug_selector(|| "popup-trigger".into())
                    .size(px(100.)),
            );
            let popup = if self.open.get() {
                popup.content(
                    div()
                        .debug_selector(|| "popup-content".into())
                        .size(px(20.)),
                )
            } else {
                popup
            };
            div()
                .size_full()
                .flex()
                .justify_end()
                .pt(px(50.))
                .pr(px(50.))
                .child(popup)
        }
    }

    /// GitTurtle patch: a popup asks for the next frame only while it holds
    /// content back for its trigger's bounds. Upstream it asked the first
    /// time its trigger was drawn, so a menu's trigger first drawn closed,
    /// as GitTurtle's text review options are when they fold into their
    /// menu at a new width, asked for a frame that drew nothing new. A
    /// trigger first drawn open still asks for the frame that draws its
    /// content, and that frame asks for no other.
    #[gpui::test]
    fn a_popup_asks_for_a_frame_only_for_content_it_holds_back(cx: &mut gpui::TestAppContext) {
        let id = Rc::new(Cell::new("closed"));
        let open = Rc::new(Cell::new(false));
        let (view, window) = cx.add_window_view({
            let (id, open) = (id.clone(), open.clone());
            move |_, _| Trailing { id, open }
        });
        window.update(|window, cx| window.draw(cx).clear(cx));
        assert!(window.debug_bounds("popup-trigger").is_some());
        assert_eq!(
            window.update(|window, cx| window.simulate_next_frame(cx)),
            0,
            "a trigger first drawn closed asks for no frame"
        );

        // A popup of its own, first drawn open.
        id.set("open");
        open.set(true);
        view.update(window, |_, cx| cx.notify());
        window.run_until_parked();
        assert!(window.debug_bounds("popup-trigger").is_some());
        assert!(
            window.debug_bounds("popup-content").is_none(),
            "the content waits for its trigger's bounds"
        );
        assert_eq!(
            window.update(|window, cx| window.simulate_next_frame(cx)),
            1,
            "a trigger first drawn open asks for the frame that draws its content"
        );
        window.run_until_parked();
        assert!(
            window.debug_bounds("popup-content").is_some(),
            "the frame it asked for draws the content"
        );
        assert_eq!(
            window.update(|window, cx| window.simulate_next_frame(cx)),
            0,
            "and that frame asks for no other"
        );
    }

    /// GitTurtle patch: an open popup follows its trigger in the frame that
    /// moves it, and asks for no frame to do so. Placed at the corner its
    /// trigger had in the frame before, the popup stayed where a narrower
    /// window moved the trigger from until something else drew a frame.
    #[gpui::test]
    fn an_open_popup_follows_its_trigger_in_the_frame_that_moves_it(cx: &mut gpui::TestAppContext) {
        let (_, window) = cx.add_window_view(|_, _| Trailing {
            id: Rc::new(Cell::new("popup")),
            open: Rc::new(Cell::new(true)),
        });
        window.simulate_resize(gpui::size(px(800.), px(400.)));
        window.update(|window, cx| window.draw(cx).clear(cx));
        window.update(|window, cx| window.draw(cx).clear(cx));
        let drawn = |window: &mut gpui::VisualTestContext| {
            (
                window.debug_bounds("popup-trigger").unwrap(),
                window.debug_bounds("popup-content").unwrap(),
            )
        };
        let (trigger, content) = drawn(window);
        let beside = content.origin - trigger.origin;
        window.update(|window, cx| window.simulate_next_frame(cx));
        window.run_until_parked();

        window.simulate_resize(gpui::size(px(600.), px(400.)));
        window.run_until_parked();
        let (moved, followed) = drawn(window);
        assert_eq!(
            moved.origin,
            trigger.origin - Point::new(px(200.), px(0.)),
            "the narrower window moves the trigger"
        );
        assert_eq!(
            followed.origin - moved.origin,
            beside,
            "the popup keeps its place beside its trigger in the frame that moves it"
        );
        assert_eq!(
            window.update(|window, cx| window.simulate_next_frame(cx)),
            0,
            "and asks for no other frame"
        );
    }
}
