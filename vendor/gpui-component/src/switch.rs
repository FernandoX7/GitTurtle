use crate::{
    ActiveTheme, Disableable, Side, Sizable, Size, StyledExt, styled::apply_focus_ring,
    text::Text, tooltip::ComponentTooltip,
};
use gpui::{
    AnyElement, App, Background, ElementId, FocusHandle, Hsla, InteractiveElement, IntoElement,
    ParentElement as _, RenderOnce, SharedString, StatefulInteractiveElement as _,
    StyleRefinement, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_base::{Switch as BaseSwitch, SwitchThumb, SwitchTrack, spring};
use std::rc::Rc;

/// The group a Switch's root forms, so hovering its label hovers its track.
const SWITCH_GROUP: &str = "gpui-component-switch";

/// A Switch element that can be toggled on or off.
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    style: StyleRefinement,
    checked: bool,
    disabled: bool,
    label: Option<Text>,
    /// The announced name, when the visible label is not it.
    accessibility_label: Option<SharedString>,
    label_side: Side,
    on_click: Option<Rc<dyn Fn(&bool, &mut Window, &mut App)>>,
    size: Size,
    color: Option<Hsla>,
    tooltip: ComponentTooltip,
    /// GitTurtle patch: a caller-owned focus handle, used instead of keyed
    /// state.
    focus_handle: Option<FocusHandle>,
    /// GitTurtle patch: content drawn before the track, inside the Switch.
    leading: Option<AnyElement>,
    /// GitTurtle patch: what assistive technology reads after the name.
    accessibility_description: Option<SharedString>,
}

impl Switch {
    /// Create a new Switch element.
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id: ElementId = id.into();
        Self {
            id: id.clone(),
            style: StyleRefinement::default(),
            checked: false,
            disabled: false,
            label: None,
            accessibility_label: None,
            on_click: None,
            label_side: Side::Right,
            size: Size::Medium,
            color: None,
            tooltip: ComponentTooltip::default(),
            focus_handle: None,
            leading: None,
            accessibility_description: None,
        }
    }

    /// Uses a caller-owned focus handle instead of creating keyed state, as
    /// `Button::track_focus` does, so the caller can tell whether the Switch
    /// holds focus.
    pub fn track_focus(mut self, focus_handle: &FocusHandle) -> Self {
        self.focus_handle = Some(focus_handle.clone());
        self
    }

    /// Set the checked state of the switch.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Set the label of the switch.
    pub fn label(mut self, label: impl Into<Text>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set the name a screen reader announces, when the visible label is not
    /// it.
    ///
    /// A switch's name comes from its [`label`](Self::label) by default.
    /// Setting this replaces the announced name without changing what is
    /// displayed.
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    /// Add a click handler for the switch.
    pub fn on_click<F>(mut self, handler: F) -> Self
    where
        F: Fn(&bool, &mut Window, &mut App) + 'static,
    {
        self.on_click = Some(Rc::new(handler));
        self
    }

    /// Set the background color of the switch when checked.
    /// Defaults to `cx.theme().primary`.
    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Set tooltip text for the switch.
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip.text = Some((tooltip.into(), None));
        self
    }

    /// Draw `leading` before the track, inside the Switch, which then fills
    /// its width with the track at its trailing edge: a settings row whose
    /// title and description are the control. A press anywhere on the row
    /// toggles it, hovering it lifts the track as hovering the label does,
    /// and its text takes the disabled label colour wherever it sets none of
    /// its own. Its text is drawn, not read, while no element in it has an
    /// ID: one with an ID writes a node of its own, which is read apart from
    /// the Switch. Name the Switch with
    /// [`accessibility_label`](Self::accessibility_label) and describe it
    /// with [`accessibility_description`](Self::accessibility_description).
    /// It leads only with the label on its default side, the right: a label
    /// on the left reverses the row and puts `leading` after the track.
    ///
    /// GitTurtle patch: upstream draws only a label after or before the
    /// track, at the kit's text size.
    pub fn leading(mut self, leading: impl IntoElement) -> Self {
        self.leading = Some(leading.into_any_element());
        self
    }

    /// Set what assistive technology reads after the Switch's name and
    /// state, such as a settings row's description.
    ///
    /// GitTurtle patch: upstream sets no description.
    pub fn accessibility_description(mut self, description: impl Into<SharedString>) -> Self {
        self.accessibility_description = Some(description.into());
        self
    }
}

impl Styled for Switch {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        &mut self.style
    }
}

impl Sizable for Switch {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl Disableable for Switch {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// The thumb's fill. Unchecked, the thumb is `switch_thumb` on the `switch`
/// track; checked, it sits on the accent, so it takes the colour primary
/// Buttons put on the accent, `primary_foreground`. `switch_thumb` is a
/// foreground in GitTurtle's palettes, and on the accent it can all but vanish:
/// a theme whose accent is its text colour drew a 1:1 knob.
pub fn thumb_color(checked: bool, cx: &App) -> Hsla {
    if checked {
        cx.theme().primary_foreground
    } else {
        cx.theme().switch_thumb
    }
}

/// A disabled Switch's label: the dim disabled Buttons and menu items draw
/// their labels in, [`crate::Theme::disabled_foreground`], so one dim marks
/// every unavailable control. Plain `muted_foreground` cannot mark it, because
/// a theme may set its secondary text as strong as its text.
pub fn disabled_label_color(cx: &App) -> Hsla {
    cx.theme().disabled_foreground()
}

impl RenderOnce for Switch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let style = self.style.clone();
        div().refine_style(&style).child(self.base(window, cx))
    }
}

impl Switch {
    /// The base Switch this Switch draws inside its own styled box: the
    /// element whose accessibility node names and describes it, which an
    /// application's tests read by rendering it.
    ///
    /// GitTurtle patch: upstream builds it inside `render`.
    pub fn base(self, window: &mut Window, cx: &mut App) -> BaseSwitch {
        let checked = self.checked;
        let on_click = self.on_click.clone();
        let accessibility_label = self
            .accessibility_label
            .clone()
            .or_else(|| self.label.as_ref().map(|label| label.get_text(cx)));

        let checked_bg = self
            .color
            .map(Background::from)
            .unwrap_or(cx.theme().tokens.primary.into());
        let unchecked_bg: Background = cx.theme().tokens.switch.into();
        // A disabled Switch fades its track and its thumb by the same factor.
        // The thumb is `switch_thumb`, not the surface behind the track, so a
        // thumb left at full strength on a faded track read as live. GPUI
        // multiplies each primitive's alpha instead of compositing the control
        // as one group, so the faded track shows through the faded thumb,
        // which is accepted.
        let disabled_opacity = 0.5;
        let disabled_bg = if checked { checked_bg } else { unchecked_bg }.opacity(disabled_opacity);
        let toggle_bg: Background = thumb_color(checked, cx).into();
        let disabled_toggle_bg = toggle_bg.opacity(disabled_opacity);
        let disabled_label_color = disabled_label_color(cx);

        let (bg_width, bg_height) = match self.size {
            Size::XSmall | Size::Small => (px(28.), px(16.)),
            _ => (px(36.), px(20.)),
        };
        let bar_width = match self.size {
            Size::XSmall | Size::Small => px(12.),
            _ => px(16.),
        };
        let inset = px(2.);
        let radius = if cx.theme().radius >= px(4.) {
            bg_height
        } else {
            cx.theme().radius
        };

        // The thumb's position is geometry, not a semantic state style: a
        // `checked` style setting `left` outranks the instance style by the
        // documented precedence, which left the travel visible in one direction
        // only. The spring owns it end to end and reverses from wherever the
        // thumb is when the switch is toggled again mid-travel.
        let thumb_x = spring(
            (self.id.clone(), "thumb"),
            if checked {
                bg_width - bar_width - inset * 2
            } else {
                px(0.)
            },
            cx.theme().motion_tokens().spring_move,
            window,
            cx,
        );

        // GitTurtle patch: the wrapper owns the base Switch's focus handle,
        // so a focused enabled Switch can draw the Button focus ring around its
        // track; an enabled unchecked one lifts its track on hover.
        let focus_handle = self.focus_handle.clone().unwrap_or_else(|| {
            window
                .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
                .read(cx)
                .clone()
        });
        let focused = !self.disabled && focus_handle.is_focused(window);
        let hover_bg = cx
            .theme()
            .switch_hover
            .filter(|_| !checked && !self.disabled);
        // GitTurtle patch: an enabled, unchecked track draws the theme's
        // resting edge from a layer over its bounds, which moves nothing; a
        // checked track, on the accent, and a disabled one draw none.
        let resting_edge = cx
            .theme()
            .resting_edge
            .filter(|_| !checked && !self.disabled);

        BaseSwitch::new(self.id.clone())
            .track_focus(&focus_handle)
            .group(SWITCH_GROUP)
            .checked(checked)
            .disabled(self.disabled)
            .styles(|styles| {
                styles.disabled(|style| {
                    style.text_color(disabled_label_color).cursor_not_allowed()
                })
            })
            .when_some(accessibility_label, |this, label| {
                this.accessibility_label(label)
            })
            .when_some(self.accessibility_description, |this, description| {
                this.aria_description(description)
            })
            .when_some(on_click, |this, on_click| {
                this.on_change(move |next, _, window, cx| on_click(&next, window, cx))
            })
            .h_flex()
            .gap_2()
            .map(|this| {
                if self.leading.is_some() {
                    this.w_full().items_center()
                } else {
                    this.items_start()
                }
            })
            .when(self.label_side.is_left(), |this| this.flex_row_reverse())
            .when_some(self.leading, |this, leading| {
                this.child(div().flex_1().min_w_0().child(leading))
            })
            .child(
                // Switch Bar
                SwitchTrack::new((self.id.clone(), "track"))
                    .checked(checked)
                    .disabled(self.disabled)
                    .when(cfg!(test), |this| {
                        this.debug_selector(|| "switch-bar".into())
                    })
                    .w(bg_width)
                    .h(bg_height)
                    .rounded(radius)
                    .flex()
                    .items_center()
                    .border(inset)
                    .border_color(cx.theme().transparent)
                    .when(!checked, |this| this.bg(unchecked_bg))
                    .styles(|styles| {
                        styles
                            .checked(|style| style.bg(checked_bg))
                            .disabled(|style| style.bg(disabled_bg))
                    })
                    .when_some(hover_bg, |this, hover| {
                        this.group_hover(SWITCH_GROUP, |style| style.bg(hover))
                    })
                    // The ring is the Button's; the border it would tint
                    // is the thumb's transparent inset, so it stays clear.
                    .when(focused, |this| {
                        apply_focus_ring(this, cx.theme().button_focus_ring, window, cx)
                            .border_color(cx.theme().transparent)
                    })
                    .map(|this| self.tooltip.apply(this))
                    // Under the thumb, and reaching back across the inset that
                    // places the track's children, so its edge is the track's.
                    .when_some(resting_edge, |this, edge| {
                        this.child(
                            div()
                                .absolute()
                                .top(-inset)
                                .left(-inset)
                                .right(-inset)
                                .bottom(-inset)
                                .border_1()
                                .border_color(edge)
                                .rounded(radius),
                        )
                    })
                    .child(
                        // Switch Toggle
                        SwitchThumb::new(checked)
                            .disabled(self.disabled)
                            .rounded(radius)
                            .size(bar_width)
                            .left(thumb_x)
                            .bg(toggle_bg)
                            .styles(|styles| {
                                styles.disabled(|style| style.bg(disabled_toggle_bg))
                            }),
                    ),
            )
            .when_some(self.label, |this, label| {
                this.child(
                    div()
                        .when(cfg!(test), |this| {
                            this.debug_selector(|| "switch-label".into())
                        })
                        .line_height(bg_height)
                        .child(label)
                        .map(|this| match self.size {
                            Size::XSmall | Size::Small => this.text_sm(),
                            _ => this.text_base(),
                        }),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use gpui::{
        Context, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, Render, TestAppContext,
        VisualTestContext, point,
    };

    use super::*;

    #[test]
    fn an_explicit_accessibility_label_replaces_the_visible_one() {
        let plain = Switch::new("wifi").label("Wi-Fi");
        assert_eq!(plain.accessibility_label, None);
        assert!(matches!(
            &plain.label,
            Some(Text::String(label)) if label.as_ref() == "Wi-Fi"
        ));

        let named = Switch::new("wifi")
            .label("Wi-Fi")
            .accessibility_label("Toggle Wi-Fi");
        assert_eq!(
            named.accessibility_label.as_deref(),
            Some("Toggle Wi-Fi"),
            "an explicit name must win over the visible label"
        );
        assert!(
            matches!(
                &named.label,
                Some(Text::String(label)) if label.as_ref() == "Wi-Fi"
            ),
            "and must not change what is drawn"
        );
    }

    struct SwitchHarness {
        disabled: bool,
        toggles: Rc<Cell<usize>>,
        parent_clicks: Rc<Cell<usize>>,
    }

    impl Render for SwitchHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let toggles = self.toggles.clone();
            let parent_clicks = self.parent_clicks.clone();
            div()
                .id("switch-parent")
                .tab_group()
                .size(px(100.))
                .on_click(move |_, _, _| parent_clicks.set(parent_clicks.get() + 1))
                .child(Switch::new("switch").disabled(self.disabled).on_click(
                    move |checked, _, _| {
                        assert!(*checked);
                        toggles.set(toggles.get() + 1);
                    },
                ))
        }
    }

    fn harness(
        cx: &mut TestAppContext,
        disabled: bool,
    ) -> (&mut VisualTestContext, Rc<Cell<usize>>, Rc<Cell<usize>>) {
        cx.update(crate::init);
        let toggles = Rc::new(Cell::new(0));
        let parent_clicks = Rc::new(Cell::new(0));
        let (_, cx) = cx.add_window_view({
            let toggles = toggles.clone();
            let parent_clicks = parent_clicks.clone();
            move |_, _| SwitchHarness {
                disabled,
                toggles,
                parent_clicks,
            }
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        (cx, toggles, parent_clicks)
    }

    fn activate_key(cx: &mut VisualTestContext, key: &str) {
        let keystroke = Keystroke::parse(key).unwrap();
        cx.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(KeyUpEvent { keystroke });
    }

    #[gpui::test]
    fn canonical_pointer_activation_fires_once_and_focuses(cx: &mut TestAppContext) {
        let (cx, toggles, _) = harness(cx, false);
        cx.simulate_click(point(px(10.), px(10.)), Modifiers::default());

        assert_eq!(toggles.get(), 1);
        cx.update(|window, cx| assert!(window.focused(cx).is_some()));
    }

    #[gpui::test]
    fn canonical_switch_supports_tab_enter_and_space(cx: &mut TestAppContext) {
        let (cx, toggles, _) = harness(cx, false);
        cx.update(|window, cx| window.focus_next(cx));
        cx.update(|window, cx| assert!(window.focused(cx).is_some()));

        activate_key(cx, "enter");
        activate_key(cx, "space");

        assert_eq!(toggles.get(), 2);
    }

    #[gpui::test]
    fn canonical_disabled_switch_is_inert_and_blocks_parent(cx: &mut TestAppContext) {
        let (cx, toggles, parent_clicks) = harness(cx, true);
        cx.simulate_click(point(px(10.), px(10.)), Modifiers::default());

        assert_eq!(toggles.get(), 0);
        assert_eq!(parent_clicks.get(), 0);
        cx.update(|window, cx| assert!(window.focused(cx).is_none()));
    }

    #[gpui::test]
    fn label_prepaints_with_the_base_switch_content(cx: &mut TestAppContext) {
        struct LabelHarness;

        impl Render for LabelHarness {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div()
                    .debug_selector(|| "labeled-switch".into())
                    .child(Switch::new("switch").label("Airplane mode"))
            }
        }

        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|_, _| LabelHarness);
        cx.update(|window, cx| window.draw(cx).clear(cx));

        let bounds = cx
            .debug_bounds("labeled-switch")
            .expect("the complete labeled Switch must participate in prepaint");
        assert!(bounds.size.width > px(36.));
        let bar = cx
            .debug_bounds("switch-bar")
            .expect("the Switch bar must participate in prepaint");
        let label = cx
            .debug_bounds("switch-label")
            .expect("the Switch label must participate in prepaint");
        assert_eq!(bar.origin.y, label.origin.y);
    }

    /// Switches in each state, each alone in a box that takes its track's
    /// bounds.
    struct RestingEdgeHarness;

    impl Render for RestingEdgeHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let switch = |id: &'static str, checked: bool, disabled: bool| {
                div()
                    .flex()
                    .debug_selector(move || id.into())
                    .child(Switch::new(id).checked(checked).disabled(disabled))
            };
            div()
                .flex()
                .items_start()
                .gap_2()
                .p_2()
                .child(switch("unchecked", false, false))
                .child(switch("checked", true, false))
                .child(switch("disabled", false, true))
                .child(switch("disabled-checked", true, true))
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum Pointer {
        Away,
        Over,
        Pressing,
    }

    type Fill = (
        gpui::Bounds<gpui::ScaledPixels>,
        Background,
        gpui::Corners<gpui::ScaledPixels>,
    );
    type Edge = (
        Hsla,
        gpui::Edges<gpui::ScaledPixels>,
        gpui::Corners<gpui::ScaledPixels>,
    );

    /// What one frame painted for a Switch.
    #[derive(Debug)]
    struct PaintedSwitch {
        /// Its box's bounds, its track's.
        bounds: gpui::Bounds<gpui::Pixels>,
        /// Each filled quad inside them, the track's and the thumb's.
        fills: Vec<Fill>,
        /// The fill over the whole track.
        track: Option<Fill>,
        /// Each bordered quad over the whole track, once: GPUI paints a
        /// border over a transparent fill as up to four strips of one quad.
        edges: Vec<Edge>,
    }

    /// Draws the Switch at `selector` with the pointer away from it, over it
    /// or pressing it, and reads what the frame painted for it.
    fn painted_switch(
        cx: &mut VisualTestContext,
        selector: &'static str,
        pointer: Pointer,
    ) -> PaintedSwitch {
        use gpui::MouseButton;

        let draw = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        };
        draw(cx);
        let center = cx.debug_bounds(selector).expect(selector).center();
        let away = point(px(1.), px(1.));
        match pointer {
            Pointer::Away => cx.simulate_mouse_move(away, None, Modifiers::default()),
            Pointer::Over => cx.simulate_mouse_move(center, None, Modifiers::default()),
            Pointer::Pressing => {
                cx.simulate_mouse_move(center, None, Modifiers::default());
                cx.simulate_mouse_down(center, MouseButton::Left, Modifiers::default());
            }
        }
        draw(cx);
        let bounds = cx.debug_bounds(selector).expect(selector);
        let painted = cx.update(|window, _| {
            let scale = window.scale_factor();
            let device = |logical: gpui::Pixels| logical.as_f32() * scale;
            let mut painted = PaintedSwitch {
                bounds,
                fills: Vec::new(),
                track: None,
                edges: Vec::new(),
            };
            for quad in window.painted_quads() {
                let drawn = quad.bounds;
                let sides = [
                    (drawn.left().as_f32(), device(bounds.left())),
                    (drawn.top().as_f32(), device(bounds.top())),
                    (drawn.right().as_f32(), device(bounds.right())),
                    (drawn.bottom().as_f32(), device(bounds.bottom())),
                ];
                let whole = sides.iter().all(|(drawn, own)| (drawn - own).abs() < 0.5);
                let [left, top, right, bottom] = sides;
                let inside = left.0 > left.1 - 0.5
                    && top.0 > top.1 - 0.5
                    && right.0 < right.1 + 0.5
                    && bottom.0 < bottom.1 + 0.5;
                let widths = quad.border_widths;
                if [widths.top, widths.right, widths.bottom, widths.left]
                    .into_iter()
                    .any(|width| width.as_f32() > 0.)
                {
                    let edge = (quad.border_color, widths, quad.corner_radii);
                    if whole && !painted.edges.contains(&edge) {
                        painted.edges.push(edge);
                    }
                } else if inside && !quad.background.is_transparent() {
                    let fill = (drawn, quad.background, quad.corner_radii);
                    painted.fills.push(fill);
                    if whole {
                        painted.track = Some(fill);
                    }
                }
            }
            painted
        });
        if let Pointer::Pressing = pointer {
            cx.simulate_mouse_up(center, MouseButton::Left, Modifiers::default());
        }
        cx.simulate_mouse_move(away, None, Modifiers::default());
        painted
    }

    fn resting_edge_harness(cx: &mut TestAppContext) -> &mut VisualTestContext {
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|_, _| RestingEdgeHarness);
        cx
    }

    fn set_resting_edge(cx: &mut VisualTestContext, edge: Option<Hsla>) {
        cx.update(|_, cx| crate::Theme::global_mut(cx).resting_edge = edge);
    }

    /// GitTurtle patch: with the theme's `resting_edge` set, an enabled,
    /// unchecked Switch paints one 1 px edge of that colour over its track,
    /// with the track's corners, at rest, hovered and pressed, while its
    /// track still lifts to `switch_hover` under the pointer; its bounds, its
    /// track and its thumb are those it paints with the edge unset, when it
    /// paints none.
    #[gpui::test]
    fn an_unchecked_switch_draws_the_themes_resting_edge_in_every_pointer_state(
        cx: &mut TestAppContext,
    ) {
        let cx = resting_edge_harness(cx);
        let edge = gpui::hsla(0.75, 0.4, 0.4, 1.);
        let hover = gpui::hsla(0.6, 0.3, 0.5, 1.);
        cx.update(|_, cx| crate::Theme::global_mut(cx).switch_hover = Some(hover));
        let scale = cx.update(|window, _| window.scale_factor());
        for pointer in [Pointer::Away, Pointer::Over, Pointer::Pressing] {
            let unset = painted_switch(cx, "unchecked", pointer);
            set_resting_edge(cx, Some(edge));
            let set = painted_switch(cx, "unchecked", pointer);
            set_resting_edge(cx, None);

            assert!(unset.edges.is_empty(), "{pointer:?} unset: {unset:?}");
            let Some((_, track, corners)) = unset.track else {
                panic!("{pointer:?}: the track's fill: {unset:?}");
            };
            if !matches!(pointer, Pointer::Away) {
                assert_eq!(track, hover.into(), "{pointer:?}: the hover lift");
            }
            let [(color, widths, drawn)] = set.edges[..] else {
                panic!("{pointer:?} set: one edge: {set:?}");
            };
            assert_eq!(color, edge, "{pointer:?}");
            assert!(
                [widths.top, widths.right, widths.bottom, widths.left]
                    .into_iter()
                    .all(|width| (width.as_f32() - scale).abs() < 0.5),
                "{pointer:?}: 1 px on every side, {widths:?}"
            );
            assert_eq!(drawn, corners, "{pointer:?}: the track's corners");
            assert_eq!(set.bounds, unset.bounds, "{pointer:?}: it moves nothing");
            assert_eq!(set.fills, unset.fills, "{pointer:?}: track and thumb");
        }
    }

    /// GitTurtle patch: a checked Switch, on the accent, and a disabled one,
    /// checked or not, draw no resting edge, the theme's set or not, and
    /// paint the same bounds and fills either way.
    #[gpui::test]
    fn a_checked_or_disabled_switch_draws_no_resting_edge(cx: &mut TestAppContext) {
        let cx = resting_edge_harness(cx);
        let edge = gpui::hsla(0.75, 0.4, 0.4, 1.);
        for id in ["checked", "disabled", "disabled-checked"] {
            for pointer in [Pointer::Away, Pointer::Over, Pointer::Pressing] {
                let unset = painted_switch(cx, id, pointer);
                set_resting_edge(cx, Some(edge));
                let set = painted_switch(cx, id, pointer);
                set_resting_edge(cx, None);

                assert!(
                    unset.track.is_some(),
                    "{id} {pointer:?}: its track: {unset:?}"
                );
                assert!(unset.edges.is_empty(), "{id} {pointer:?} unset: {unset:?}");
                assert!(set.edges.is_empty(), "{id} {pointer:?} set: {set:?}");
                assert_eq!(set.bounds, unset.bounds, "{id} {pointer:?}");
                assert_eq!(set.fills, unset.fills, "{id} {pointer:?}");
            }
        }
    }
}
