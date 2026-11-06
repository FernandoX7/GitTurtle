use gpui::{
    Anchor, Animation, AnimationExt as _, AnyElement, App, Bounds, Context, Div, ElementId,
    FocusHandle, InteractiveElement as _, IntoElement, MouseButton, ParentElement, Pixels,
    RenderOnce, Stateful, StyleRefinement, Styled, Window, prelude::FluentBuilder as _, px,
};
use std::{rc::Rc, time::Duration};

use crate::ThemeStyled as _;
use crate::{
    ClearOfOpener, Opener, Selectable, StyledExt as _,
    animation::ease_out_cubic,
    styled::{popover_ring, popover_shadow},
    v_flex,
};
use gpui_base::Popover as BasePopover;
pub use gpui_base::PopoverState;

pub(crate) fn init(_: &mut App) {}

/// How long a dropdown takes to settle into place after it opens.
///
/// This is shadcn/ui's figure: its popup surfaces carry `animate-in`, whose
/// duration is 150ms.
const DROPDOWN_ENTER_DURATION: Duration = Duration::from_millis(150);

/// Where a dropdown starts out, relative to where it comes to rest.
///
/// Negative is above, so the surface slides *down* out of the trigger's edge —
/// what shadcn/ui expresses as `data-[side=bottom]:slide-in-from-top-2`. Its
/// `2` is `0.5rem`, which is 8px at the default root size.
const DROPDOWN_ENTER_OFFSET: Pixels = px(-8.);

/// GitTurtle patch: its drawn edge, not only its fill, stays the popup
/// margin inside the window ([`crate::popup_fill_margin`]).
fn dropdown_positioner(bounds: Bounds<Pixels>) -> gpui_base::Positioner {
    gpui_base::Positioner::side(bounds)
        .placement(gpui_base::Placement::Bottom)
        .align(gpui_base::Align::Start)
        .offset(px(6.))
        .margin(crate::popup_fill_margin())
}

/// Positions a dropdown surface under its trigger and animates it in.
///
/// This is the shared open motion for Select, Combobox and DatePicker, modelled
/// on shadcn/ui: over 150ms the surface fades up from nothing while sliding the
/// last 8px out of the trigger's edge, on an ease-out curve so it decelerates
/// into place.
///
/// `surface` must be the panel itself — the element carrying
/// [`ThemeStyled::popover_style`] — and not a wrapper around it. GPUI takes a
/// shadow's shape from the element it is set on, so a wrapper of a different
/// size would throw the shadow out of register with the panel.
///
/// # Why the shadow is animated too
///
/// GPUI has no group compositing: `opacity` multiplies into each primitive's
/// alpha separately rather than fading a composited subtree. A drop shadow is
/// painted as a full blurred rect *under* the element — the shader only cuts the
/// element out of `inset` shadows — so a translucent panel does not hide its own
/// shadow, and mid-fade the shadow shows straight through the panel as a dark
/// slab. Ramping the ink by the cube of the fade keeps it out of sight until the
/// panel is opaque enough to cover it, and still lands on the resting shadow
/// [`popover_shadow`] gives every other popup.
///
/// # Departures from shadcn
///
/// - shadcn also scales the surface up from 95% (`zoom-in-95`). GPUI has no
///   element transform — only images and SVGs take a `TransformationMatrix` —
///   so there is nothing to scale a subtree with, and the fade and slide carry
///   the motion on their own.
/// - There is no exit motion. A closing dropdown stops being rendered in the
///   same frame its state flips, so playing one would mean keeping the surface
///   mounted past the close, which is a change to how each of these components
///   tracks `open`.
/// - The slide always comes from above. [`gpui_base::Positioner`] resolves the
///   side the surface actually lands on during layout and does not report it
///   back, so a dropdown that flips above its trigger for want of room below
///   slides the opposite way — 8px over 150ms, in the rare case where it
///   happens.
///
/// Reduced motion needs no handling here: GPUI's animation element adopts the
/// final value on the first frame when the system asks for it.
pub(crate) fn dropdown_popup(
    id: impl Into<ElementId>,
    bounds: Bounds<Pixels>,
    surface: impl IntoElement + Styled + 'static,
    cx: &App,
) -> gpui_base::Positioner {
    let travel: f32 = DROPDOWN_ENTER_OFFSET.into();
    // Read out here: the animation runs long after `cx` is gone.
    let ring = popover_ring(cx);

    dropdown_positioner(bounds).child(surface.with_animation(
        id,
        Animation::new(DROPDOWN_ENTER_DURATION).with_easing(ease_out_cubic),
        move |surface, delta| {
            surface
                .top(px(travel * (1. - delta)))
                .opacity(delta)
                .shadow(popover_shadow(ring, delta * delta * delta))
        },
    ))
}

/// A popover element that can be triggered by a button or any other element.
#[derive(IntoElement)]
pub struct Popover {
    id: ElementId,
    style: StyleRefinement,
    anchor: Anchor,
    default_open: bool,
    open: Option<bool>,
    tracked_focus_handle: Option<FocusHandle>,
    trigger: Option<Box<dyn FnOnce(bool, &Window, &App) -> AnyElement + 'static>>,
    content: Option<
        Rc<
            dyn Fn(&mut PopoverState, &mut Window, &mut Context<PopoverState>) -> AnyElement
                + 'static,
        >,
    >,
    children: Vec<AnyElement>,
    /// Style for trigger element.
    /// This is used for hotfix the trigger element style to support w_full.
    ///
    /// GitTurtle patch: applied to the box the popover lays the trigger out
    /// in, which upstream never did.
    trigger_style: Option<StyleRefinement>,
    mouse_button: MouseButton,
    appearance: bool,
    overlay_closable: bool,
    on_open_change: Option<Rc<dyn Fn(&bool, &mut Window, &mut App)>>,
}

impl Popover {
    /// Create a new Popover with `view` mode.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            style: StyleRefinement::default(),
            anchor: Anchor::TopLeft,
            trigger: None,
            trigger_style: None,
            content: None,
            tracked_focus_handle: None,
            children: vec![],
            mouse_button: MouseButton::Left,
            appearance: true,
            overlay_closable: true,
            default_open: false,
            open: None,
            on_open_change: None,
        }
    }

    /// Set the anchor corner of the popover, default is [`Anchor::TopLeft`].
    ///
    /// Imagine the popover has a pointer tip (like a speech bubble's tail). The
    /// anchor is where that tip sits relative to the trigger: `Anchor::TopLeft`
    /// places it at the trigger's top-left corner, `Anchor::BottomRight` at the
    /// bottom-right, and so on. The popover then hangs off that point.
    pub fn anchor(mut self, anchor: impl Into<Anchor>) -> Self {
        self.anchor = anchor.into();
        self
    }

    /// Set the mouse button to trigger the popover, default is `MouseButton::Left`.
    pub fn mouse_button(mut self, mouse_button: MouseButton) -> Self {
        self.mouse_button = mouse_button;
        self
    }

    /// Set the trigger element of the popover.
    pub fn trigger<T>(mut self, trigger: T) -> Self
    where
        T: Selectable + IntoElement + 'static,
    {
        self.trigger = Some(Box::new(|is_open, _, _| {
            let selected = trigger.is_selected();
            trigger.selected(selected || is_open).into_any_element()
        }));
        self
    }

    /// Set the default open state of the popover, default is `false`.
    ///
    /// This is only used to initialize the open state of the popover.
    ///
    /// And please note that if you use the `open` method, this value will be ignored.
    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    /// Force set the open state of the popover.
    ///
    /// If this is set, the popover will be controlled by this value.
    ///
    /// NOTE: You must be used in conjunction with `on_open_change` to handle state changes.
    pub fn open(mut self, open: bool) -> Self {
        self.open = Some(open);
        self
    }

    /// Add a callback to be called when the open state changes.
    ///
    /// The first `&bool` parameter is the **new open state**.
    ///
    /// This is useful when using the `open` method to control the popover state.
    pub fn on_open_change<F>(mut self, callback: F) -> Self
    where
        F: Fn(&bool, &mut Window, &mut App) + 'static,
    {
        self.on_open_change = Some(Rc::new(callback));
        self
    }

    /// Set the style for the trigger element.
    ///
    /// GitTurtle patch: it styles the box the popover lays the trigger out
    /// in, the box the trigger's siblings see, so it suits the trigger's
    /// placement (margins, flex-item properties) rather than its looks, which
    /// the trigger draws itself.
    pub fn trigger_style(mut self, style: StyleRefinement) -> Self {
        self.trigger_style = Some(style);
        self
    }

    /// Set whether clicking outside the popover will dismiss it, default is `true`.
    pub fn overlay_closable(mut self, closable: bool) -> Self {
        self.overlay_closable = closable;
        self
    }

    /// Set the content builder for content of the Popover.
    ///
    /// This callback will called every time on render the popover.
    /// So, you should avoid creating new elements or entities in the content closure.
    pub fn content<F, E>(mut self, content: F) -> Self
    where
        E: IntoElement,
        F: Fn(&mut PopoverState, &mut Window, &mut Context<PopoverState>) -> E + 'static,
    {
        self.content = Some(Rc::new(move |state, window, cx| {
            content(state, window, cx).into_any_element()
        }));
        self
    }

    /// Set whether the popover no style, default is `false`.
    ///
    /// If no style:
    ///
    /// - The popover will not have a bg, border, shadow, or padding.
    /// - The click out of the popover will not dismiss it.
    pub fn appearance(mut self, appearance: bool) -> Self {
        self.appearance = appearance;
        self
    }

    /// Bind the focus handle to receive focus when the popover is opened.
    /// If you not set this, a new focus handle will be created for the popover to
    ///
    /// If popover is opened, the focus will be moved to the focus handle.
    pub fn track_focus(mut self, handle: &FocusHandle) -> Self {
        self.tracked_focus_handle = Some(handle.clone());
        self
    }
}

impl ParentElement for Popover {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Popover {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Popover {
    pub(crate) fn render_popover_content(
        anchor: Anchor,
        appearance: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Stateful<Div> {
        // GitTurtle patch: the theme's popup gap past the drawn edge, or
        // upstream's quarter rem to the fill ([`crate::popup_offset`]).
        let offset = crate::popup_offset(window, cx);
        v_flex()
            .id("content")
            .occlude()
            .tab_group()
            .when(appearance, |this| this.popover_style(cx).p_3())
            // GitTurtle patch: the gap from the trigger is a margin, which the
            // popup host measures and keeps inside the window and above the
            // application's bottom inset with the surface. Upstream offset
            // the surface by its relative `top`, which the host never saw,
            // so a surface it moved to the inset ended a quarter rem past it.
            .map(|this| match anchor {
                Anchor::TopLeft | Anchor::TopCenter | Anchor::TopRight => this.mt(offset),
                Anchor::BottomLeft | Anchor::BottomCenter | Anchor::BottomRight => this.mb(offset),
                Anchor::LeftCenter | Anchor::RightCenter => this.mt(offset), // Fallback for centered
            })
    }
}

impl RenderOnce for Popover {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let anchor = self.anchor;
        let appearance = self.appearance;
        let style = self.style;
        let children = self.children;
        let content = self.content;
        let trigger_style = self.trigger_style;

        let popover = BasePopover::new(self.id)
            .anchor(self.anchor)
            // GitTurtle patch: the host keeps the drawn edge, the ring
            // outside the surface's fill, the popup margin inside the
            // window, where it kept the fill there and the ring 1 px nearer
            // ([`crate::POPUP_WINDOW_MARGIN`]).
            .margin(crate::popup_fill_margin())
            .mouse_button(self.mouse_button)
            .default_open(self.default_open)
            .overlay_closable(self.overlay_closable)
            .content(move |state, window, cx| {
                // GitTurtle patch: nothing the popover draws, its shadow
                // included, reaches the trigger on the side its anchor
                // opens it from ([`ClearOfOpener`]).
                let opener = match anchor {
                    Anchor::TopLeft | Anchor::TopCenter | Anchor::TopRight => {
                        Some(Opener::Above(crate::popup_offset(window, cx)))
                    }
                    Anchor::BottomLeft | Anchor::BottomCenter | Anchor::BottomRight => {
                        Some(Opener::Below(crate::popup_offset(window, cx)))
                    }
                    Anchor::LeftCenter | Anchor::RightCenter => None,
                };
                let content = Self::render_popover_content(anchor, appearance, window, cx)
                    .when_some(content, |this, content| {
                        this.child((content)(state, window, cx))
                    })
                    .children(children)
                    .refine_style(&style);
                match opener {
                    Some(opener) => ClearOfOpener::new(content, opener).into_any_element(),
                    None => content.into_any_element(),
                }
            })
            .when_some(self.trigger, |this, trigger| this.trigger_with(trigger))
            .when_some(self.open, |this, open| this.open(open))
            .when_some(self.tracked_focus_handle, |this, handle| {
                this.track_focus(&handle)
            })
            .when_some(self.on_open_change, |this, callback| {
                this.on_open_change(move |open, window, cx| callback(open, window, cx))
            })
            .into_any_element();
        // GitTurtle patch: the trigger's box takes the trigger style, so the
        // trigger's placement among its siblings, flex-item properties
        // included, holds as it would without the popover.
        match trigger_style {
            Some(trigger_style) => gpui::div()
                .refine_style(&trigger_style)
                .child(popover)
                .into_any_element(),
            None => popover,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{button::Button, theme::Theme};
    use gpui::{Bounds, Context, MouseButton, Point, Render, div, point, px, size};
    use gpui_base::Popup as BasePopup;
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn test_popover_builder_chaining() {
        let popover = Popover::new("test")
            .anchor(Anchor::BottomCenter)
            .mouse_button(MouseButton::Right)
            .default_open(true)
            .appearance(false)
            .overlay_closable(false);

        assert_eq!(popover.anchor, Anchor::BottomCenter);
        assert_eq!(popover.mouse_button, MouseButton::Right);
        assert!(popover.default_open);
        assert!(!popover.appearance);
        assert!(!popover.overlay_closable);
    }

    #[test]
    fn test_resolved_corner_top_positions() {
        use gpui::px;

        let bounds = Bounds {
            origin: Point {
                x: px(100.),
                y: px(100.),
            },
            size: gpui::Size {
                width: px(200.),
                height: px(50.),
            },
        };

        let pos = BasePopup::resolved_corner(Anchor::TopLeft, bounds);
        assert_eq!(pos.x, px(100.));
        assert_eq!(pos.y, px(100.));

        let pos = BasePopup::resolved_corner(Anchor::TopCenter, bounds);
        assert_eq!(pos.x, px(200.));
        assert_eq!(pos.y, px(100.));

        let pos = BasePopup::resolved_corner(Anchor::TopRight, bounds);
        assert_eq!(pos.x, px(300.));
        assert_eq!(pos.y, px(100.));

        let pos = BasePopup::resolved_corner(Anchor::BottomLeft, bounds);
        assert_eq!(pos.x, px(100.));
        assert_eq!(pos.y, px(50.));

        let pos = BasePopup::resolved_corner(Anchor::BottomCenter, bounds);
        assert_eq!(pos.x, px(200.));
        assert_eq!(pos.y, px(50.));

        let pos = BasePopup::resolved_corner(Anchor::BottomRight, bounds);
        assert_eq!(pos.x, px(300.));
        assert_eq!(pos.y, px(50.));
    }

    struct PopoverHarness {
        changes: Rc<RefCell<Vec<bool>>>,
    }

    impl Render for PopoverHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let changes = self.changes.clone();
            Popover::new("runtime-popover")
                .trigger(Button::new("runtime-trigger").label("Open").size(px(100.)))
                .content(|_, _, _| {
                    div()
                        .debug_selector(|| "runtime-popover-content".into())
                        .size(px(40.))
                })
                .on_open_change(move |open, _, _| changes.borrow_mut().push(*open))
        }
    }

    #[gpui::test]
    fn pointer_open_and_outside_dismiss_use_the_base_popup_host(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_base::GlobalState::init(cx);
            cx.set_global(Theme::default());
            init(cx);
        });

        let changes = Rc::new(RefCell::new(Vec::new()));
        let (_, cx) = cx.add_window_view({
            let changes = changes.clone();
            move |_, _| PopoverHarness { changes }
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));

        cx.simulate_click(point(px(20.), px(20.)), Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(cx.debug_bounds("runtime-popover-content").is_some());

        cx.simulate_click(point(px(300.), px(300.)), Default::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(cx.debug_bounds("runtime-popover-content").is_none());
        // A change callback reports state transitions, not redundant dismissal
        // requests. The base host may see both paths, but only the first closes.
        assert_eq!(&*changes.borrow(), &[true, false]);
    }

    struct DefaultOpenHarness;

    impl Render for DefaultOpenHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            Popover::new("default-open-popover")
                .default_open(true)
                .trigger(Button::new("default-open-trigger").label("Open"))
                .child(
                    div()
                        .debug_selector(|| "default-open-content".into())
                        .size(px(40.)),
                )
        }
    }

    #[gpui::test]
    fn default_open_is_forwarded_to_the_base_popover(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_base::GlobalState::init(cx);
            cx.set_global(Theme::default());
            init(cx);
        });
        let (_, cx) = cx.add_window_view(|_, _| DefaultOpenHarness);
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(cx.debug_bounds("default-open-content").is_some());
    }

    struct Harness {
        open: bool,
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().when(self.open, |this| {
                this.child(dropdown_popup(
                    "dropdown",
                    Bounds::new(point(px(0.), px(100.)), size(px(120.), px(30.))),
                    div().debug_selector(|| "surface".into()).size(px(50.)),
                    cx,
                ))
            })
        }
    }

    /// A dropdown that reused one animation key across opens would play its
    /// enter motion the first time and then appear already settled on every
    /// open after that. That is invisible in any single frame and easy to
    /// reintroduce by giving the animation a constant id, so it is pinned here.
    #[gpui::test]
    fn the_enter_motion_starts_over_every_time_the_dropdown_opens(cx: &mut gpui::TestAppContext) {
        cx.update(crate::init);
        let (view, window) = cx.add_window_view(|_, _| Harness { open: true });

        window.update(|window, cx| window.draw(cx).clear(cx));
        let opening = window.debug_bounds("surface").unwrap().origin;

        // The animation runs off the wall clock, so settling is waited out
        // rather than stepped. Several times the duration leaves room for a
        // loaded machine.
        std::thread::sleep(DROPDOWN_ENTER_DURATION * 4);
        window.update(|window, cx| window.draw(cx).clear(cx));
        let settled = window.debug_bounds("surface").unwrap().origin;

        assert!(
            opening.y < settled.y,
            "the surface should slide down into place, from {opening:?} to {settled:?}",
        );

        for open in [false, true] {
            window.update(|window, cx| {
                view.update(cx, |this, cx| {
                    this.open = open;
                    cx.notify();
                });
                window.draw(cx).clear(cx);
            });
        }

        let reopening = window.debug_bounds("surface").unwrap().origin;
        assert!(
            reopening.y < settled.y,
            "reopening should start the motion over at {opening:?} rather than showing a \
             settled surface, but the first frame was already at {reopening:?}",
        );
    }

    /// A Button that opens and closes a dropdown surface below it on each
    /// click, as Select's trigger does.
    struct ClickedDropdown {
        open: bool,
    }

    impl Render for ClickedDropdown {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(
                    Button::new("dropdown-trigger")
                        .label("Open")
                        .w(px(100.))
                        .h(px(30.))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.open = !this.open;
                            cx.notify();
                        })),
                )
                .when(self.open, |this| {
                    this.child(dropdown_popup(
                        "dropdown",
                        Bounds::new(point(px(0.), px(0.)), size(px(100.), px(30.))),
                        div()
                            .debug_selector(|| "dropdown-surface".into())
                            .size(px(50.)),
                        cx,
                    ))
                })
        }
    }

    /// GitTurtle patch (test only): with reduce motion on, a popover a click
    /// opens is drawn in the frame it opens and asks for no frame after it,
    /// and a click takes it away in the next frame, as a dialog opens and
    /// closes. The kit's Popover has no motion of its own; a dropdown surface
    /// (`dropdown_popup`) slides and fades in over 150 ms through GPUI's
    /// animation element, which draws its last frame at once under reduce
    /// motion whether a key or a click opened it. Without reduce motion the
    /// dropdown asks for its next frame.
    #[gpui::test]
    fn reduce_motion_shows_and_hides_a_click_opened_popover_at_rest(cx: &mut gpui::TestAppContext) {
        cx.update(crate::init);
        cx.update(|cx| cx.set_reduce_motion(true));
        let draw = |cx: &mut gpui::VisualTestContext| {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        };
        let frames_asked = |cx: &mut gpui::VisualTestContext| {
            cx.update(|window, cx| window.simulate_next_frame(cx))
        };

        let changes = Rc::new(RefCell::new(Vec::new()));
        let (_, popover) = cx.add_window_view({
            let changes = changes.clone();
            move |_, _| PopoverHarness { changes }
        });
        draw(popover);
        frames_asked(popover);
        popover.simulate_click(point(px(20.), px(20.)), Default::default());
        draw(popover);
        let opened = popover.debug_bounds("runtime-popover-content");
        assert!(opened.is_some(), "the popover shows in the frame it opens");
        assert_eq!(frames_asked(popover), 0, "the popover asks for no frame");
        draw(popover);
        assert_eq!(popover.debug_bounds("runtime-popover-content"), opened);
        popover.simulate_click(point(px(300.), px(300.)), Default::default());
        draw(popover);
        assert!(popover.debug_bounds("runtime-popover-content").is_none());
        assert_eq!(&*changes.borrow(), &[true, false]);

        let (_, dropdown) = cx.add_window_view(|_, _| ClickedDropdown { open: false });
        draw(dropdown);
        frames_asked(dropdown);
        dropdown.simulate_click(point(px(20.), px(15.)), Default::default());
        draw(dropdown);
        let opened = dropdown
            .debug_bounds("dropdown-surface")
            .expect("the dropdown shows in the frame it opens");
        assert_eq!(
            frames_asked(dropdown),
            0,
            "the dropdown's slide and fade ask for no frame"
        );
        draw(dropdown);
        assert_eq!(
            dropdown.debug_bounds("dropdown-surface"),
            Some(opened),
            "the dropdown opened at rest"
        );
        dropdown.simulate_click(point(px(20.), px(15.)), Default::default());
        draw(dropdown);
        assert!(dropdown.debug_bounds("dropdown-surface").is_none());

        // Without reduce motion the same click starts the slide.
        dropdown.update(|_, cx| cx.set_reduce_motion(false));
        dropdown.simulate_click(point(px(20.), px(15.)), Default::default());
        draw(dropdown);
        assert!(dropdown.debug_bounds("dropdown-surface").is_some());
        assert!(
            frames_asked(dropdown) > 0,
            "the slide asks for its next frame"
        );
    }

    /// How far from a window's edge [`SidesHarness`]'s triggers start and
    /// end: 6.5 px, as the Linux Main Menu's button starts at 13 pt, and
    /// 2 px, three device pixels at scale 1.5.
    const TRIGGER_INSETS: [f32; 2] = [6.5, 2.];

    /// GitTurtle patch: two open popovers, each a surface 120 px wide too
    /// wide for its side of the window from a 40 px trigger `inset` from
    /// that side: one opening from its trigger's leading corner at the
    /// left, one from its trailing corner at the right, as the Main Menu
    /// and the Settings profile menu open.
    struct SidesHarness {
        inset: Pixels,
    }

    impl Render for SidesHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let side = |id: &'static str, anchor: Anchor| {
                Popover::new(id)
                    .anchor(anchor)
                    .default_open(true)
                    .appearance(false)
                    .trigger(Button::new(id).label("Open").w(px(40.)))
                    .content(move |_, _, cx| {
                        div()
                            .debug_selector(move || id.into())
                            .popover_style(cx)
                            .w(px(120.))
                            .h(px(60.))
                            .into_any_element()
                    })
            };
            div()
                .size_full()
                .flex()
                .justify_between()
                .pl(self.inset)
                .pr(self.inset)
                .child(side("left-surface", Anchor::TopLeft))
                .child(side("right-surface", Anchor::TopRight))
        }
    }

    /// GitTurtle patch: a popover moved in from either side of the window
    /// draws its edge, the ring outside its fill, 8 px from that side, on
    /// the device pixel, at scale 1, 1.5 and 2: 8, 12 and 16 device pixels.
    /// Each fill is painted where it is laid out, wholly on device pixels,
    /// and its ring is read as GPUI paints a shadow, over every device pixel
    /// the band outside the fill touches (`paint_drop_shadows`). The popup
    /// host moved a popover by whole pixels from where it was laid out,
    /// beside a trigger half a pixel into one, so at scale 2 its fill lay
    /// 9.5 px in and its edge 8.5 px, 17 device pixels, from either side;
    /// and GPUI places an element's offset on the device pixel with halves
    /// toward zero, so at 1.5 beside a trigger 2 px in the old 7 px offset,
    /// 10.5 device pixels, put the fill 13 device pixels in and its edge 11
    /// from the side.
    #[gpui::test]
    fn a_popover_moved_in_from_a_side_draws_its_edge_8_px_from_it(cx: &mut gpui::TestAppContext) {
        cx.update(crate::init);
        // 460 px is a whole number of device pixels at each scale.
        let width = px(460.);
        for (scale, inset) in [1., 1.5, 2.]
            .into_iter()
            .flat_map(|scale| TRIGGER_INSETS.map(|inset| (scale, inset)))
        {
            let (_, cx) = cx.add_window_view(move |_, _| SidesHarness { inset: px(inset) });
            cx.simulate_resize(size(width, px(300.)));
            cx.update(|window, cx| {
                window.set_scale_factor(scale);
                window.draw(cx).clear(cx);
                window.draw(cx).clear(cx);
            });
            let margin = f32::from(crate::POPUP_WINDOW_MARGIN) * scale;
            for (id, left) in [("left-surface", true), ("right-surface", false)] {
                let at = format!("scale {scale}, {id} from a trigger {inset} px in");
                let fill = cx.debug_bounds(id).unwrap_or_else(|| panic!("{at}: open"));
                let painted = cx.update(|window, cx| {
                    let popover = gpui::Background::from(crate::ActiveTheme::theme(cx).popover);
                    window
                        .painted_quads()
                        .into_iter()
                        .filter(|quad| quad.background == popover)
                        .map(|quad| quad.bounds)
                        .find(|quad| {
                            let center = fill.center();
                            quad.contains(&point(
                                gpui::ScaledPixels(f32::from(center.x) * scale),
                                gpui::ScaledPixels(f32::from(center.y) * scale),
                            ))
                        })
                        .unwrap_or_else(|| panic!("{at}: the fill {fill:?} is painted"))
                });
                for (laid, drawn) in [
                    (fill.left(), painted.left()),
                    (fill.right(), painted.right()),
                ] {
                    assert!(
                        (f32::from(laid) * scale - drawn.as_f32()).abs() < 1e-3,
                        "{at}: the fill {fill:?} is painted at {painted:?}, between device pixels"
                    );
                }
                let ring = fill.dilate(crate::POPOVER_EDGE_WIDTH);
                let from_side = if left {
                    (f32::from(ring.left()) * scale).floor()
                } else {
                    f32::from(width) * scale - (f32::from(ring.right()) * scale).ceil()
                };
                assert_eq!(
                    from_side, margin,
                    "{at}: the fill {fill:?} draws its edge {from_side} device pixels from the \
                     side, not {margin}"
                );
            }
        }
    }
}
