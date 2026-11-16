use gpui::{
    Anchor, App, Context, ElementId, Entity, FocusHandle, Focusable, Hsla, InteractiveElement as _,
    IntoElement, KeyBinding, MouseButton, NoAction, ParentElement, Pixels, RenderOnce, Role,
    SharedString, StatefulInteractiveElement as _, StyleRefinement, Styled, Subscription,
    TextAlign, Window, div, hsla, linear_color_stop, linear_gradient, prelude::FluentBuilder as _,
    px,
};
use rust_i18n::t;

use gpui_base::{ColorPicker as BaseColorPicker, ColorSwatch, color_channels, color_hex};
pub use gpui_base::{ColorPickerEvent, ColorPickerState};

use crate::{
    ActiveTheme as _, Colorize as _, Icon, Selectable, Sizable, Size, StyleSized,
    actions::{Confirm, SelectDown, SelectLeft, SelectRight, SelectUp},
    h_flex,
    input::Input,
    popover::Popover,
    separator::Separator,
    slider::{Slider, SliderAccessibleValue},
    styled::apply_focus_ring,
    tab::{Tab, TabBar},
    tooltip::{ManagedTooltipExt as _, Tooltip},
    v_flex,
};

/// GitTurtle patch: the key context around a palette's swatches, deeper than
/// the popover's, whose Enter and Space ([`Confirm`](crate::actions::Confirm))
/// close the picker without choosing anything.
const SWATCHES_CONTEXT: &str = "ColorPickerSwatches";

/// GitTurtle patch: the trigger's key context beside the base picker's
/// `ColorPicker`, whose Enter, bound to [`Confirm`], toggled the picker on
/// every key-down, a held key's repeats included. Bound to nothing here,
/// after it at the same depth, which outranks it, Enter reaches the
/// trigger's key-down listener ([`fresh_enter`]) instead.
const TRIGGER_CONTEXT: &str = "ColorPickerTrigger";

/// GitTurtle patch: the trigger's whole key context, the base picker's and
/// [`TRIGGER_CONTEXT`].
const TRIGGER_KEY_CONTEXT: &str = "ColorPicker ColorPickerTrigger";

/// GitTurtle patch: the swatches are one stop of the picker's tab order that
/// the arrow keys move inside. Enter and Space are bound to nothing here, so
/// they pass the popover's and the dialog's bindings by and reach GPUI's own
/// keyboard click on the focused swatch, which chooses it as the key comes
/// up: once however long the key is held, and only for a press that went down
/// on that swatch.
pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectUp, Some(SWATCHES_CONTEXT)),
        KeyBinding::new("down", SelectDown, Some(SWATCHES_CONTEXT)),
        KeyBinding::new("left", SelectLeft, Some(SWATCHES_CONTEXT)),
        KeyBinding::new("right", SelectRight, Some(SWATCHES_CONTEXT)),
        KeyBinding::new("enter", NoAction, Some(SWATCHES_CONTEXT)),
        KeyBinding::new("space", NoAction, Some(SWATCHES_CONTEXT)),
        KeyBinding::new("enter", NoAction, Some(TRIGGER_CONTEXT)),
    ]);
}

/// GitTurtle patch: Enter on the focused trigger opens or closes the picker
/// only on a fresh press, through the kit's held-Enter filter, as a Button in
/// a dialog presses: never on a held key's repeats, marked held or repeating
/// the recorded press ([`gpui_base::enter_press_is_repeat`]), nor for an
/// Enter whose press began on another control
/// ([`gpui_base::enter_press_began_on`]). Under X11 a held key's repeats
/// arrive as fresh presses, so Enter held after the hex field's commit, or a
/// repeat whose release and press a poll batch split after it chose a
/// swatch, reopened the picker it had just closed, and a pointer press that
/// closed a picker an Enter held on its trigger had opened let the next
/// repeat, begun on the trigger, open it again. Focus inside the popover
/// passes over this: there the popover's own contexts bind the key.
fn fresh_enter(
    trigger: &FocusHandle,
    event: &gpui::KeyDownEvent,
    window: &mut Window,
    cx: &mut App,
) {
    if event.keystroke.key != "enter"
        || event.keystroke.modifiers.modified()
        || !trigger.is_focused(window)
    {
        return;
    }
    if !event.is_held
        && !gpui_base::enter_press_is_repeat(window, cx)
        && gpui_base::enter_press_began_on(trigger, window, cx)
    {
        trigger.dispatch_action(&Confirm { secondary: false }, window, cx);
    }
    cx.stop_propagation();
}

/// GitTurtle patch: Enter inside the open popover, whose own binding
/// dispatches [`Confirm`] to close it, acts only on a fresh press, one that
/// began inside it, so a held Enter that opened the picker from its trigger
/// does nothing more until its key-up. That binding took every key-down, so
/// the first held repeat closed the picker its press had opened, and under
/// X11, whose repeats arrive as fresh presses the trigger takes again, each
/// repeat toggled it. A repeat is one by the kit's Enter tracker, marked
/// held or not ([`gpui_base::enter_press_is_repeat`]). The trigger's own
/// `Confirm` ([`fresh_enter`]) passes, and so does Space, which the popover
/// binds alike and which records no Enter press.
fn fresh_popover_confirm(trigger: &FocusHandle, window: &mut Window, cx: &mut App) {
    if !trigger.is_focused(window) && gpui_base::enter_press_is_repeat(window, cx) {
        cx.stop_propagation();
    }
}

/// GitTurtle patch: the gap between swatches, the kit's quarter rem, never
/// narrower than the focus ring and a pixel of surface beyond it, so a
/// focused swatch's ring lies whole in the gaps around it at every text
/// size, clear of the neighbours drawn after it and set off from one close
/// to the accent.
fn swatch_gap(window: &Window, cx: &App) -> Pixels {
    let ring = cx.theme().button_focus_ring;
    (window.rem_size() * 0.25).max(ring.width + ring.gap + px(1.))
}

/// GitTurtle patch: a color the palette draws and the name a screen reader
/// reads for it, which says the color and gives its value.
#[derive(Clone)]
struct PaletteColor {
    color: Hsla,
    name: SharedString,
}

impl PaletteColor {
    fn new(label: impl std::fmt::Display, color: Hsla) -> Self {
        Self {
            color,
            name: format!("{label}, {}", color_hex(color)).into(),
        }
    }

    /// A caller's color, for which the kit knows no name but its value.
    fn unnamed(color: Hsla) -> Self {
        Self {
            color,
            name: color_hex(color).into(),
        }
    }
}

/// The palette's rows of shades, each darkest first, as they are drawn.
fn color_palettes() -> Vec<Vec<PaletteColor>> {
    use crate::theme::DEFAULT_COLORS;
    use itertools::Itertools as _;

    macro_rules! c {
        ($color:tt, $name:literal) => {{
            let name = t!($name);
            DEFAULT_COLORS
                .$color
                .keys()
                .sorted()
                .rev()
                .map(|k| {
                    PaletteColor::new(
                        format!("{name} {k}"),
                        DEFAULT_COLORS.$color.get(k).map(|c| c.hsla).unwrap(),
                    )
                })
                .collect::<Vec<_>>()
        }};
    }

    vec![
        c!(stone, "ColorPicker.Stone"),
        c!(red, "ColorPicker.Red"),
        c!(orange, "ColorPicker.Orange"),
        c!(yellow, "ColorPicker.Yellow"),
        c!(green, "ColorPicker.Green"),
        c!(cyan, "ColorPicker.Cyan"),
        c!(blue, "ColorPicker.Blue"),
        c!(purple, "ColorPicker.Purple"),
        c!(pink, "ColorPicker.Pink"),
    ]
}

/// The featured row a picker draws when its caller gives none: the theme's
/// own colors, each and its light form.
fn theme_colors(cx: &App) -> Vec<PaletteColor> {
    let theme = cx.theme();
    let light = |color: std::borrow::Cow<'_, str>| t!("ColorPicker.Light", color = color);
    vec![
        PaletteColor::new(t!("ColorPicker.Red"), theme.red),
        PaletteColor::new(light(t!("ColorPicker.Red")), theme.red_light),
        PaletteColor::new(t!("ColorPicker.Blue"), theme.blue),
        PaletteColor::new(light(t!("ColorPicker.Blue")), theme.blue_light),
        PaletteColor::new(t!("ColorPicker.Green"), theme.green),
        PaletteColor::new(light(t!("ColorPicker.Green")), theme.green_light),
        PaletteColor::new(t!("ColorPicker.Yellow"), theme.yellow),
        PaletteColor::new(light(t!("ColorPicker.Yellow")), theme.yellow_light),
        PaletteColor::new(t!("ColorPicker.Cyan"), theme.cyan),
        PaletteColor::new(light(t!("ColorPicker.Cyan")), theme.cyan_light),
        PaletteColor::new(t!("ColorPicker.Magenta"), theme.magenta),
        PaletteColor::new(light(t!("ColorPicker.Magenta")), theme.magenta_light),
    ]
}

/// Whether two colors are the same one to the eye: a value kept as RGB comes
/// back through `Hsla` a rounding away from the palette's own.
fn same_color(a: Hsla, b: Hsla) -> bool {
    color_channels(a) == color_channels(b)
}

/// A swatch's element id, its row and its place along that row, unique
/// however long a caller's featured row is.
fn swatch_id(row: usize, ix: usize) -> ElementId {
    ElementId::NamedInteger("color".into(), ((row as u64) << 32) | ix as u64)
}

/// GitTurtle patch: watches an open picker, on the palette's tab and the
/// sliders' alike, for as long as its popover is drawn: the picker closing
/// by any route, a key's among them, puts the preview back on the committed
/// color, so one the arrows or a hover left does not outlast the picker, and
/// has the window draw before its next input
/// ([`crate::root::draw_after_popup_close`]).
///
/// A swatch's choice (its click or keyboard click) closes the picker through
/// its state, and the popover gave focus back to the trigger only as the
/// next frame built it, where GPUI asks for no frame after the one being
/// drawn: that frame drew the trigger as unfocused and none followed, so its
/// ring waited for an unrelated redraw. Focus inside the closing popover
/// goes back to the trigger here, before that frame, as Escape's close gives
/// it back, and the focus change asks for the frame. Enter in the hex field
/// closes it otherwise: the single-line Input passes the key on to the
/// popover's own Enter binding, whose `Confirm` closes it through
/// `PopoverState::toggle_open`, giving focus back as Escape does, so the
/// commit that follows finds focus on the trigger already.
struct ClosingWatch {
    _closing: Subscription,
}

impl ClosingWatch {
    fn new(state: &Entity<ColorPickerState>, window: &mut Window, cx: &mut App) -> Self {
        let closing = window.observe(state, cx, |state, window, cx| {
            if !state.read(cx).is_open() {
                let trigger = state.focus_handle(cx);
                if trigger.contains_focused(window, cx) && !trigger.is_focused(window) {
                    trigger.focus(window, cx);
                }
                state.update(cx, |state, cx| state.clear_preview(window, cx));
                crate::root::draw_after_popup_close(window, cx);
            }
        });
        Self { _closing: closing }
    }
}

/// The trigger's tooltip: the picker's value in the digits its field shows
/// and a save writes.
fn value_title(value: Option<Hsla>) -> SharedString {
    value.map(color_hex).unwrap_or_default().into()
}

/// GitTurtle patch: one of the sliders on the HSLA tab, by the channel it
/// sets. Upstream's sliders were unnamed: a screen reader read "slider" and
/// the state's value from 0 to 1, while the picker draws the channel's name
/// before it and its value in degrees or percent after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Channel {
    Hue,
    Saturation,
    Lightness,
    Alpha,
}

impl Channel {
    /// The name drawn before the slider, which also names it.
    fn name(self) -> SharedString {
        match self {
            Self::Hue => t!("ColorPicker.Hue"),
            Self::Saturation => t!("ColorPicker.Saturation"),
            Self::Lightness => t!("ColorPicker.Lightness"),
            Self::Alpha => t!("ColorPicker.Alpha"),
        }
        .to_string()
        .into()
    }

    /// The most the shown value reaches: hue in degrees, the rest in
    /// percent.
    fn most(self) -> f32 {
        match self {
            Self::Hue => 360.,
            Self::Saturation | Self::Lightness | Self::Alpha => 100.,
        }
    }

    /// The channel's value in `color` in the digits drawn after the slider.
    fn digits(self, color: Hsla) -> SharedString {
        let share = match self {
            Self::Hue => color.h,
            Self::Saturation => color.s,
            Self::Lightness => color.l,
            Self::Alpha => color.a,
        };
        format!("{:.0}", share * self.most()).into()
    }

    /// What the slider reports to assistive technology: the drawn digits,
    /// on a range and step in the same units, the step being what one key
    /// press moves its state, whose own `step` is a share of the channel's
    /// 0 to 1 (3.6 degrees for hue at the picker's 0.01). Reported as one,
    /// hue's step said a press moved it a degree. The step is kept to the
    /// hundredth, the precision of an `f32` share scaled to the drawn units.
    fn accessible_value(self, color: Hsla, step: f32) -> SliderAccessibleValue {
        let most = f64::from(self.most());
        SliderAccessibleValue {
            value: self.digits(color).parse().unwrap_or_default(),
            min: 0.,
            max: most,
            step: (f64::from(step) * most * 100.).round() / 100.,
        }
    }
}

/// An arrow key's direction in the palette.
#[derive(Clone, Copy, Debug)]
enum Step {
    Up,
    Down,
    Left,
    Right,
}

/// A place in the palette: its row and its swatch along that row.
type Place = (usize, usize);

/// Where `step` moves from `from` in rows of `lengths` swatches, or `None`
/// past the palette's edge, which keeps the cursor where it is. Up and Down
/// land on the swatch above or below, the one at the same share of its row:
/// a row's swatches share its width, and the featured row's twelve are
/// narrower than the eleven shades under them.
fn step_from(lengths: &[usize], (row, column): Place, step: Step) -> Option<Place> {
    let from = lengths[row];
    let to_row = |to: usize| (to, (2 * column + 1) * lengths[to] / (2 * from));
    match step {
        Step::Left => column.checked_sub(1).map(|column| (row, column)),
        Step::Right => (column + 1 < lengths[row]).then_some((row, column + 1)),
        Step::Up => (0..row).rev().find(|row| lengths[*row] > 0).map(to_row),
        Step::Down => (row + 1..lengths.len())
            .find(|row| lengths[*row] > 0)
            .map(to_row),
    }
}

/// GitTurtle patch: the keyboard's place in an open picker's palette, kept
/// for as long as the palette is drawn (`ColorPicker::render_palette_panel`
/// reads it only while the picker is open, so a picker opened again starts
/// over on its current color).
///
/// Every swatch has its own focus handle, which the arrow keys move focus
/// among, and only the swatch at `cursor` is a tab stop: the palette is one
/// stop of the popover's order and the hex field the other.
struct SwatchGrid {
    state: Entity<ColorPickerState>,
    rows: Vec<Vec<(FocusHandle, Hsla)>>,
    cursor: Option<Place>,
    /// The one swatch announced as checked: a color can be drawn in the
    /// featured row and among the shades, and a radio group has one checked
    /// member.
    checked: Option<Place>,
}

impl SwatchGrid {
    fn new(state: Entity<ColorPickerState>) -> Self {
        Self {
            state,
            rows: Vec::new(),
            cursor: None,
            checked: None,
        }
    }

    /// Fits the grid to the colors of this frame and returns each swatch's
    /// focus handle. The cursor follows whichever swatch holds focus, as a
    /// press focuses one; drawn first, it starts on the first swatch of the
    /// current color, or the palette's first where the color is not in it.
    fn sync(
        &mut self,
        colors: &[Vec<PaletteColor>],
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<Vec<FocusHandle>> {
        self.rows.resize_with(colors.len(), Vec::new);
        for (row, colors) in self.rows.iter_mut().zip(colors) {
            row.truncate(colors.len());
            for (ix, color) in colors.iter().enumerate() {
                match row.get_mut(ix) {
                    Some(swatch) => swatch.1 = color.color,
                    None => row.push((cx.focus_handle(), color.color)),
                }
            }
        }

        let value = self.state.read(cx).value();
        self.checked = self.place_of(value);
        let kept = self
            .cursor
            .filter(|(row, ix)| self.rows.get(*row).is_some_and(|row| *ix < row.len()));
        self.cursor = self
            .focused(window)
            .or(kept)
            .or(self.checked)
            .or_else(|| self.places().next().map(|(place, _)| place));

        self.rows
            .iter()
            .map(|row| row.iter().map(|(focus, _)| focus.clone()).collect())
            .collect()
    }

    /// Every swatch's place and color, in reading order.
    fn places(&self) -> impl Iterator<Item = (Place, Hsla)> + '_ {
        self.rows.iter().enumerate().flat_map(|(row, swatches)| {
            swatches
                .iter()
                .enumerate()
                .map(move |(ix, swatch)| ((row, ix), swatch.1))
        })
    }

    /// The first swatch of `value`, where the palette draws it.
    fn place_of(&self, value: Option<Hsla>) -> Option<Place> {
        let value = value?;
        self.places()
            .find(|(_, color)| same_color(*color, value))
            .map(|(place, _)| place)
    }

    fn focused(&self, window: &Window) -> Option<Place> {
        self.rows.iter().enumerate().find_map(|(row, swatches)| {
            swatches
                .iter()
                .position(|(focus, _)| focus.is_focused(window))
                .map(|ix| (row, ix))
        })
    }

    /// Moves focus one swatch by `step`, previewing the swatch it reaches as
    /// the pointer's hover does. At the palette's edge nothing moves.
    fn step(&mut self, step: Step, window: &mut Window, cx: &mut Context<Self>) {
        let lengths: Vec<usize> = self.rows.iter().map(Vec::len).collect();
        let Some(to) = self
            .focused(window)
            .and_then(|from| step_from(&lengths, from, step))
        else {
            return;
        };
        let (focus, color) = self.rows[to.0][to.1].clone();
        self.cursor = Some(to);
        focus.focus(window, cx);
        self.state
            .update(cx, |state, cx| state.preview_color(color, window, cx));
    }
}

/// A color picker element.
#[derive(IntoElement)]
pub struct ColorPicker {
    id: ElementId,
    style: StyleRefinement,
    state: Entity<ColorPickerState>,
    featured_colors: Option<Vec<Hsla>>,
    label: Option<SharedString>,
    /// The announced name, when the visible label is not it.
    accessibility_label: Option<SharedString>,
    /// GitTurtle patch: the open popover's announced name
    /// ([`ColorPicker::popover_label`]).
    popover_label: Option<SharedString>,
    icon: Option<Icon>,
    size: Size,
    anchor: Anchor,
}

impl ColorPicker {
    /// Create a new color picker element with the given [`ColorPickerState`].
    pub fn new(state: &Entity<ColorPickerState>) -> Self {
        Self {
            id: ("color-picker", state.entity_id()).into(),
            style: StyleRefinement::default(),
            state: state.clone(),
            featured_colors: None,
            size: Size::Medium,
            label: None,
            accessibility_label: None,
            popover_label: None,
            icon: None,
            anchor: Anchor::TopLeft,
        }
    }

    /// Set the featured colors to be displayed in the color picker.
    ///
    /// This is used to display a set of colors that the user can quickly select from,
    /// for example provided user's last used colors.
    pub fn featured_colors(mut self, colors: Vec<Hsla>) -> Self {
        self.featured_colors = Some(colors);
        self
    }

    /// Set the icon to the color picker button.
    ///
    /// If this is set the color picker button will display the icon.
    /// Else it will display the square color of the current value.
    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Set the label to be displayed above the color picker.
    ///
    /// Default is `None`.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set the name a screen reader announces, when the visible label is not
    /// it.
    ///
    /// A color picker's name comes from its [`label`](Self::label) by default.
    /// Setting this replaces the announced name without changing the visible
    /// label.
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    /// GitTurtle patch: the name a screen reader announces for the open
    /// popover, a dialog, such as the color the trigger chooses ("Removed
    /// color" for "Choose the Removed color"). Upstream left it unnamed, and
    /// Orca read it as "dialog".
    pub fn popover_label(mut self, label: impl Into<SharedString>) -> Self {
        self.popover_label = Some(label.into());
        self
    }

    /// Set the anchor corner of the color picker.
    ///
    /// Default is `Anchor::TopLeft`.
    pub fn anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = anchor;
        self
    }

    fn render_item(
        &self,
        id: ElementId,
        item: &PaletteColor,
        focus: &FocusHandle,
        (is_stop, checked): (bool, bool),
        window: &Window,
        cx: &mut App,
    ) -> ColorSwatch {
        let color = item.color;
        let hover_state = self.state.clone();
        let click_state = self.state.clone();

        ColorSwatch::new(id, color)
            .track_focus(focus)
            .tab_stop(is_stop)
            .accessibility_label(item.name.clone())
            .selected(checked)
            .h_5()
            .w_5()
            .bg(color)
            .border_1()
            .border_color(color.darken(0.1))
            .hover(|this| this.border_color(color.darken(0.3)).bg(color.lighten(0.1)))
            .active(|this| this.border_color(color.darken(0.5)).bg(color.darken(0.2)))
            .on_hover(move |color, entered, window, cx| {
                if entered {
                    hover_state.update(cx, |state, cx| state.preview_color(color, window, cx));
                }
            })
            .on_click(move |color, _, window, cx| {
                click_state.update(cx, |state, cx| state.select_color(color, window, cx));
            })
            // GitTurtle patch: the ring the kit's other controls draw, whole
            // in the gaps around the swatch ([`swatch_gap`]).
            .when(focus.is_focused(window), |this| {
                apply_focus_ring(this, cx.theme().button_focus_ring, window, cx)
            })
    }

    fn render_colors(&self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        window.use_keyed_state(("color-picker-closing", self.state.entity_id()), cx, {
            let state = self.state.clone();
            move |window, cx| ClosingWatch::new(&state, window, cx)
        });

        let active_tab = self.state.read(cx).active_tab();
        let (slider_color, hovered_color) = {
            let state = self.state.read(cx);
            let slider_color = state
                .displayed_color()
                .unwrap_or_else(|| hsla(0., 0., 0., 1.));
            (slider_color, state.preview())
        };
        let tab_state = self.state.clone();

        v_flex()
            .p_0p5()
            .gap_3()
            .child(
                TabBar::new("mode")
                    .segmented()
                    .selected_index(active_tab)
                    .on_click(move |ix: &usize, _, cx| {
                        tab_state.update(cx, |state, cx| state.set_active_tab(*ix, cx));
                    })
                    .child(Tab::new().flex_1().label(t!("ColorPicker.Palette")))
                    .child(Tab::new().flex_1().label(t!("ColorPicker.HSLA"))),
            )
            .child(match active_tab {
                0 => self.render_palette_panel(window, cx).into_any_element(),
                _ => self
                    .render_slider_tab_panel(slider_color, cx)
                    .into_any_element(),
            })
            .when_some(hovered_color, |this, hovered_color| {
                this.child(Separator::horizontal()).child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .bg(hovered_color)
                                .flex_shrink_0()
                                .border_1()
                                .border_color(hovered_color.darken(0.2))
                                .size_5()
                                .rounded(cx.theme().radius),
                        )
                        .child(
                            Input::new(self.state.read(cx).hex_input())
                                .small()
                                .px_2p5()
                                // GitTurtle patch: named by what it sets.
                                .aria_label(t!("ColorPicker.HexValue").to_string()),
                        ),
                )
            })
    }

    /// The swatches, the featured row above the palette's rows of shades,
    /// each with a place in [`SwatchGrid`] and an id of its own: the same
    /// color can be drawn in both, and swatches sharing an id share its
    /// element state.
    fn render_palette_panel(&self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let featured = match &self.featured_colors {
            Some(colors) => colors.iter().copied().map(PaletteColor::unnamed).collect(),
            None => theme_colors(cx),
        };
        let mut colors = vec![featured];
        colors.extend(color_palettes());

        let grid = window.use_keyed_state(("color-picker-swatches", self.state.entity_id()), cx, {
            let state = self.state.clone();
            move |_, _| SwatchGrid::new(state)
        });
        let handles = grid.update(cx, |grid, cx| grid.sync(&colors, window, cx));
        let (cursor, checked) = {
            let grid = grid.read(cx);
            (grid.cursor, grid.checked)
        };
        let gap = swatch_gap(window, cx);

        let mut rows = colors
            .iter()
            .zip(&handles)
            .enumerate()
            .map(|(row, (colors, handles))| {
                let swatches = colors
                    .iter()
                    .zip(handles)
                    .enumerate()
                    .map(|(ix, (item, focus))| {
                        let id = swatch_id(row, ix);
                        let place = Some((row, ix));
                        let (is_stop, is_checked) = (cursor == place, checked == place);
                        self.render_item(id, item, focus, (is_stop, is_checked), window, cx)
                    })
                    .collect::<Vec<_>>();
                h_flex().gap(gap).children(swatches)
            })
            .collect::<Vec<_>>()
            .into_iter();
        let featured_row = rows.next().expect("the featured row");

        v_flex()
            .id("swatches")
            .key_context(SWATCHES_CONTEXT)
            .role(Role::RadioGroup)
            .aria_label(SharedString::from(t!("ColorPicker.Palette").to_string()))
            .on_action(
                window.listener_for(&grid, |grid, _: &SelectUp, window, cx| {
                    grid.step(Step::Up, window, cx)
                }),
            )
            .on_action(
                window.listener_for(&grid, |grid, _: &SelectDown, window, cx| {
                    grid.step(Step::Down, window, cx)
                }),
            )
            .on_action(
                window.listener_for(&grid, |grid, _: &SelectLeft, window, cx| {
                    grid.step(Step::Left, window, cx)
                }),
            )
            .on_action(
                window.listener_for(&grid, |grid, _: &SelectRight, window, cx| {
                    grid.step(Step::Right, window, cx)
                }),
            )
            .gap_3()
            .child(featured_row)
            .child(Separator::horizontal())
            .child(v_flex().gap(gap).children(rows))
    }

    fn render_slider_tab_panel(&self, slider_color: Hsla, cx: &mut App) -> impl IntoElement {
        let sliders = self.state.read(cx).sliders().clone();
        let steps = 96usize;
        let hue_colors = (0..steps)
            .map(|ix| {
                let h = ix as f32 / (steps.saturating_sub(1)) as f32;
                hsla(h, 1.0, 0.5, 1.0)
            })
            .collect::<Vec<_>>();
        let saturation_start = hsla(slider_color.h, 0.0, slider_color.l, 1.0);
        let saturation_end = hsla(slider_color.h, 1.0, slider_color.l, 1.0);
        let lightness_colors = (0..steps)
            .map(|ix| {
                let l = ix as f32 / (steps.saturating_sub(1)) as f32;
                hsla(slider_color.h, 1.0, l, 1.0)
            })
            .collect::<Vec<_>>();
        let alpha_start = hsla(slider_color.h, slider_color.s, slider_color.l, 0.0);
        let alpha_end = hsla(slider_color.h, slider_color.s, slider_color.l, 1.0);

        let label_color = cx.theme().foreground.opacity(0.7);

        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .min_w_16()
                            .text_xs()
                            .text_color(label_color)
                            .child(Channel::Hue.name()),
                    )
                    .child(
                        div()
                            .relative()
                            .flex()
                            .items_center()
                            .flex_1()
                            .h_8()
                            .child(self.render_slider_track(hue_colors, cx))
                            .child(channel_slider(
                                sliders.hue(),
                                Channel::Hue,
                                slider_color,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .w_10()
                            .text_xs()
                            .text_color(label_color)
                            .text_align(TextAlign::Right)
                            .child(Channel::Hue.digits(slider_color)),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .min_w_16()
                            .text_xs()
                            .text_color(label_color)
                            .child(Channel::Saturation.name()),
                    )
                    .child(
                        div()
                            .relative()
                            .flex()
                            .items_center()
                            .flex_1()
                            .h_8()
                            .child(self.render_slider_track_gradient(
                                saturation_start,
                                saturation_end,
                                cx,
                            ))
                            .child(channel_slider(
                                sliders.saturation(),
                                Channel::Saturation,
                                slider_color,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .w_10()
                            .text_xs()
                            .text_color(label_color)
                            .text_align(TextAlign::Right)
                            .child(Channel::Saturation.digits(slider_color)),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .min_w_16()
                            .text_xs()
                            .text_color(label_color)
                            .child(Channel::Lightness.name()),
                    )
                    .child(
                        div()
                            .relative()
                            .flex()
                            .items_center()
                            .flex_1()
                            .h_8()
                            .child(self.render_slider_track(lightness_colors, cx))
                            .child(channel_slider(
                                sliders.lightness(),
                                Channel::Lightness,
                                slider_color,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .w_10()
                            .text_xs()
                            .text_color(label_color)
                            .text_align(TextAlign::Right)
                            .child(Channel::Lightness.digits(slider_color)),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .min_w_16()
                            .text_xs()
                            .text_color(label_color)
                            .child(Channel::Alpha.name()),
                    )
                    .child(
                        div()
                            .relative()
                            .flex()
                            .items_center()
                            .flex_1()
                            .h_8()
                            .child(self.render_slider_track_gradient(alpha_start, alpha_end, cx))
                            .child(channel_slider(
                                sliders.alpha(),
                                Channel::Alpha,
                                slider_color,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .w_10()
                            .text_xs()
                            .text_color(label_color)
                            .text_align(TextAlign::Right)
                            .child(Channel::Alpha.digits(slider_color)),
                    ),
            )
    }

    fn render_slider_track(&self, colors: Vec<Hsla>, _: &App) -> impl IntoElement {
        h_flex()
            .absolute()
            .left_0()
            .right_0()
            .h_2_5()
            .overflow_hidden()
            .children(
                colors
                    .into_iter()
                    .map(|color| div().flex_1().h_full().bg(color)),
            )
    }

    fn render_slider_track_gradient(&self, start: Hsla, end: Hsla, _: &App) -> impl IntoElement {
        div()
            .absolute()
            .left_0()
            .right_0()
            .h_2_5()
            .overflow_hidden()
            .bg(linear_gradient(
                90.,
                linear_color_stop(start, 0.),
                linear_color_stop(end, 1.),
            ))
    }
}

/// GitTurtle patch: the slider for `channel` of `color`, named by it and
/// reporting the digits drawn after it and the step one increment moves it
/// ([`channel_value`]).
fn channel_slider(
    state: &Entity<gpui_base::slider::SliderState>,
    channel: Channel,
    color: Hsla,
    cx: &App,
) -> Slider {
    Slider::new(state)
        .flex_1()
        .bg(cx.theme().transparent)
        .aria_label(channel.name())
        .accessible_value(channel_value(state, channel, color, cx))
}

/// GitTurtle patch: what the slider `state` for `channel` of `color`
/// reports to assistive technology ([`Channel::accessible_value`]), its step
/// read from the state, which its arrow keys move by.
fn channel_value(
    state: &Entity<gpui_base::slider::SliderState>,
    channel: Channel,
    color: Hsla,
    cx: &App,
) -> SliderAccessibleValue {
    channel.accessible_value(color, state.read(cx).step_value())
}

impl Sizable for ColorPicker {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl Focusable for ColorPicker {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.state.focus_handle(cx)
    }
}

impl Styled for ColorPicker {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ColorPicker {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.state.read(cx);
        let display_title = value_title(state.value());

        let open = state.is_open();
        let value = state.value();
        let focus_handle = self.state.focus_handle(cx);
        let open_state = self.state.clone();
        let popover_state = self.state.clone();
        self.state
            .update(cx, |state, cx| state.sync_pending_value(window, cx));

        BaseColorPicker::new(self.id.clone())
            .open(open)
            .key_context(TRIGGER_KEY_CONTEXT)
            .track_focus(&focus_handle)
            .capture_key_down({
                let focus_handle = focus_handle.clone();
                move |event, window, cx| fresh_enter(&focus_handle, event, window, cx)
            })
            .capture_action({
                let focus_handle = focus_handle.clone();
                move |_: &Confirm, window, cx| fresh_popover_confirm(&focus_handle, window, cx)
            })
            // GitTurtle patch: a press on the trigger focuses it before the
            // popover opens and records the focus it gives back as it
            // closes. The popover's own press listener stops the press, so
            // GPUI never focused the trigger and Escape gave focus back to
            // whichever control held it before the press.
            .capture_any_mouse_down({
                let focus_handle = focus_handle.clone();
                move |event, window, cx| {
                    if event.button == MouseButton::Left {
                        focus_handle.focus(window, cx);
                    }
                }
            })
            .when_some(
                self.accessibility_label
                    .clone()
                    .or_else(|| self.label.clone()),
                |this, label| this.accessibility_label(label),
            )
            .on_open_change(move |open, _, cx| {
                open_state.update(cx, |state, cx| state.set_open(open, cx));
            })
            .child(
                Popover::new("popover")
                    .when_some(self.popover_label.clone(), |this, label| {
                        this.aria_label(label)
                    })
                    .open(open)
                    .w_72()
                    .on_open_change(move |open: &bool, _, cx| {
                        popover_state.update(cx, |state, cx| state.set_open(*open, cx));
                    })
                    .trigger(ColorPickerButton {
                        id: "trigger".into(),
                        size: self.size,
                        label: self.label.clone(),
                        value,
                        tooltip: if display_title.is_empty() {
                            None
                        } else {
                            Some(display_title.clone())
                        },
                        icon: self.icon.clone(),
                        selected: false,
                    })
                    // GitTurtle patch: built only while it is drawn, so the
                    // swatches' focus handles and the cursor last no longer
                    // than the open popover ([`SwatchGrid`]).
                    .when(open, |this| this.child(self.render_colors(window, cx))),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui::{AppContext as _, TestAppContext, VisualTestContext, point, px};
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    use super::*;

    /// A page whose clicks are counted, holding a picker.
    struct Page {
        state: Entity<ColorPickerState>,
        clicks: Rc<Cell<usize>>,
    }

    impl gpui::Render for Page {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let clicks = self.clicks.clone();
            div()
                .id("page")
                .size_full()
                .on_click(move |_, _, _| clicks.set(clicks.get() + 1))
                .child(
                    div()
                        .debug_selector(|| "picker".into())
                        .w(px(40.))
                        .child(ColorPicker::new(&self.state)),
                )
        }
    }

    /// GitTurtle patch: a click given right after the key that closes a
    /// picker, before the window's next frame, lands on the page under
    /// where its popover was and never on the gone popover: Escape, before
    /// or after its release, or Enter on a swatch, which chooses it as the
    /// key comes up.
    #[gpui::test]
    fn a_click_after_the_key_that_closes_the_picker_lands_on_the_page(cx: &mut TestAppContext) {
        use crate::root::before_a_frame::{click, deliver, key};

        cx.update(crate::init);
        let clicks = Rc::new(Cell::new(0));
        let slot: Rc<RefCell<Option<Entity<ColorPickerState>>>> = Rc::default();
        let (_, cx) = cx.add_window_view({
            let (clicks, slot) = (clicks.clone(), slot.clone());
            move |window, cx| {
                let state = cx.new(|cx| {
                    ColorPickerState::new(window, cx).default_value(hsla(0.3, 0.2, 0.7, 1.))
                });
                *slot.borrow_mut() = Some(state.clone());
                let view = cx.new(|_| Page { state, clicks });
                crate::Root::new(view, window, cx).bordered(false)
            }
        });
        let state = slot.borrow().clone().expect("the picker's state");
        let settle = |cx: &mut VisualTestContext| {
            cx.run_until_parked();
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        };
        settle(cx);
        let picker = cx.debug_bounds("picker").expect("the picker");
        let inside = point(picker.left() + px(60.), picker.bottom() + px(80.));

        for (closing, key_up_first) in [("escape", false), ("escape", true), ("enter", true)] {
            let at = format!("{closing}, key up first: {key_up_first}");
            cx.simulate_click(picker.center(), Default::default());
            settle(cx);
            assert!(
                state.read_with(cx, |state, _| state.is_open()),
                "{at}: the click opened it"
            );
            // Into the swatches, which Enter chooses from.
            cx.update(|window, cx| window.focus_next(cx));
            settle(cx);

            let before = clicks.get();
            let [press, release] = click(inside);
            let (down, up) = (key(closing, true), key(closing, false));
            deliver(
                cx,
                if key_up_first {
                    vec![down, up, press, release]
                } else {
                    vec![down, press, release, up]
                },
            );
            settle(cx);

            assert!(
                !state.read_with(cx, |state, _| state.is_open()),
                "{at}: the key closed it"
            );
            assert_eq!(clicks.get(), before + 1, "{at}: the page took the click");
        }

        // On the sliders' tab, the click where the hue slider was: the gone
        // slider must not take the press.
        cx.update(|_, cx| state.update(cx, |state, cx| state.set_active_tab(1, cx)));
        let on_slider = point(picker.left() + px(160.), picker.bottom() + px(78.));
        for key_up_first in [false, true] {
            let at = format!("the sliders' tab, key up first: {key_up_first}");
            cx.simulate_click(picker.center(), Default::default());
            settle(cx);
            assert!(
                state.read_with(cx, |state, _| state.is_open()),
                "{at}: the click opened it"
            );
            let hue = state.read_with(cx, |state, cx| state.sliders().hue().read(cx).value());

            let before = clicks.get();
            let [press, release] = click(on_slider);
            let (down, up) = (key("escape", true), key("escape", false));
            deliver(
                cx,
                if key_up_first {
                    vec![down, up, press, release]
                } else {
                    vec![down, press, release, up]
                },
            );
            settle(cx);

            assert!(
                !state.read_with(cx, |state, _| state.is_open()),
                "{at}: the key closed it"
            );
            assert_eq!(clicks.get(), before + 1, "{at}: the page took the click");
            assert_eq!(
                state.read_with(cx, |state, cx| state.sliders().hue().read(cx).value()),
                hue,
                "{at}: the gone slider took no press"
            );
        }
    }

    /// GitTurtle patch: an Enter held on the trigger opens the picker once,
    /// and once a pointer press on the trigger has closed it mid-hold, the
    /// key's repeats as X11 sends them, fresh presses no flag marks and a
    /// release with its press, open nothing, as they repeat the press the
    /// base kit recorded (`gpui_base::enter_press_is_repeat`); after the
    /// key-up a fresh press opens it again. Begun on the trigger and not
    /// marked held, the first repeat after the pointer's close opened it.
    #[gpui::test]
    fn an_x11_enter_held_reopens_no_picker_a_pointer_closed(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let slot: Rc<RefCell<Option<Entity<ColorPickerState>>>> = Rc::default();
        let (_, cx) = cx.add_window_view({
            let slot = slot.clone();
            move |window, cx| {
                let state = cx.new(|cx| ColorPickerState::new(window, cx));
                *slot.borrow_mut() = Some(state.clone());
                let clicks = Rc::new(Cell::new(0));
                let view = cx.new(|_| Page { state, clicks });
                crate::Root::new(view, window, cx).bordered(false)
            }
        });
        let state = slot.borrow().clone().expect("the picker's state");
        let settle = |cx: &mut VisualTestContext| {
            cx.run_until_parked();
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        };
        // Enter as X11 sends it: a key-down, a press or a repeat alike, or
        // with `None` its key-up.
        let enter = |cx: &mut VisualTestContext, down: bool| {
            // Dispatched with no idle pass after it, as `simulate_event`
            // runs, so a release and its press arrive together.
            let keystroke = gpui::Keystroke::parse("enter").unwrap();
            let event = if down {
                gpui::PlatformInput::KeyDown(gpui::KeyDownEvent {
                    keystroke,
                    is_held: false,
                    prefer_character_input: false,
                })
            } else {
                gpui::PlatformInput::KeyUp(gpui::KeyUpEvent { keystroke })
            };
            cx.update(|window, cx| window.dispatch_event(event, cx));
        };
        let open = |cx: &mut VisualTestContext| state.read_with(cx, |state, _| state.is_open());
        settle(cx);
        let picker = cx.debug_bounds("picker").expect("the picker");
        // Opened and closed by the pointer and Escape, focus rests on the
        // trigger.
        cx.simulate_click(picker.center(), Default::default());
        settle(cx);
        cx.simulate_keystrokes("escape");
        settle(cx);
        assert!(!open(cx), "Escape closed it");

        enter(cx, true);
        settle(cx);
        assert!(open(cx), "Enter on the trigger opens it");
        cx.simulate_click(picker.center(), Default::default());
        settle(cx);
        assert!(!open(cx), "a press on the trigger closes it");
        for (repeat, events) in [
            ("an X11 repeat", &[true][..]),
            ("another X11 repeat", &[true]),
            ("a release and its press", &[false, true]),
        ] {
            for down in events {
                enter(cx, *down);
            }
            settle(cx);
            assert!(!open(cx), "{repeat}: the held Enter opens nothing");
        }
        enter(cx, false);
        settle(cx);
        enter(cx, true);
        settle(cx);
        assert!(open(cx), "a fresh press after the key-up opens it");
    }

    /// GitTurtle patch: each HSLA slider reports to assistive technology
    /// the step one increment (a screen reader's increment action) moves
    /// it, in the units drawn beside it and read from its state: at the
    /// picker's step of 0.01, 3.6 degrees for hue and 1 percent for
    /// saturation, lightness and alpha, on the range the drawn digits take.
    /// Reported as one for every slider, hue's step said an increment moved
    /// it a degree.
    #[gpui::test]
    fn each_slider_reports_the_step_one_increment_moves_it(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let cx = cx.add_empty_window();
        let state = cx.update(|window, cx| cx.new(|cx| ColorPickerState::new(window, cx)));
        let color = hsla(0.3, 0.2, 0.7, 1.);
        cx.update(|_, cx| {
            let sliders = state.read(cx).sliders().clone();
            for (channel, slider, most, step) in [
                (Channel::Hue, sliders.hue(), 360., 3.6),
                (Channel::Saturation, sliders.saturation(), 100., 1.),
                (Channel::Lightness, sliders.lightness(), 100., 1.),
                (Channel::Alpha, sliders.alpha(), 100., 1.),
            ] {
                let pressed = f64::from(slider.read(cx).step_value()) * most;
                let reported = channel_value(slider, channel, color, cx);
                assert_eq!(reported.step, step, "{channel:?}: the step it reports");
                assert!(
                    (reported.step - pressed).abs() < 1e-6,
                    "{channel:?}: {} is the step one press moves, {pressed}",
                    reported.step
                );
                assert_eq!(
                    (reported.min, reported.max),
                    (0., most),
                    "{channel:?}: the range of the drawn digits"
                );
            }
        });
    }

    /// GitTurtle patch: Up and Down land on the swatch above or below, the
    /// one at the same share of its row's width: the twelve featured
    /// swatches are narrower than the eleven shades under them, so from the
    /// middle on the matching index is not the swatch directly above. Going
    /// back returns, from every shade and from every featured swatch but the
    /// one that shares its shade with the next.
    #[test]
    fn up_and_down_land_on_the_swatch_above_or_below() {
        let lengths = [12, 11, 11];
        assert_eq!(step_from(&lengths, (1, 5), Step::Up), Some((0, 6)));
        assert_eq!(step_from(&lengths, (1, 6), Step::Up), Some((0, 7)));
        assert_eq!(step_from(&lengths, (1, 10), Step::Up), Some((0, 11)));
        assert_eq!(step_from(&lengths, (0, 0), Step::Down), Some((1, 0)));
        assert_eq!(step_from(&lengths, (1, 7), Step::Down), Some((2, 7)));
        for column in 0..11 {
            let up = step_from(&lengths, (1, column), Step::Up).expect("a row above");
            assert_eq!(
                step_from(&lengths, up, Step::Down),
                Some((1, column)),
                "Up then Down returns to shade {column}"
            );
        }
        for column in 0..12 {
            let down = step_from(&lengths, (0, column), Step::Down).expect("a row below");
            let back = step_from(&lengths, down, Step::Up).expect("a row above");
            assert!(
                back == (0, column) || (column == 5 && back == (0, 6)),
                "Down then Up from featured {column} came back at {back:?}"
            );
        }
    }

    /// GitTurtle patch: a palette color saved as a token comes back through
    /// RGB still the palette's color, so a picker opened again finds it.
    #[test]
    fn a_color_kept_as_rgb_is_still_the_palette_color() {
        for item in color_palettes().iter().flatten() {
            let rgb = item.color.to_rgb();
            let channel = |value: f32| (value.clamp(0., 1.) * 255.).round() as u32;
            let kept = (channel(rgb.r) << 16) | (channel(rgb.g) << 8) | channel(rgb.b);
            let back: Hsla = gpui::rgb(kept).into();
            assert!(same_color(item.color, back), "{} is lost", item.name);
        }
    }

    /// The value choosing a color commits, each channel rounded as the theme
    /// editor saves it (`hsla_rgb`).
    fn rounded_hex(color: Hsla) -> String {
        let rgb = color.to_rgb();
        let channel = |value: f32| (value.clamp(0., 1.) * 255.).round() as u32;
        format!(
            "#{:02X}{:02X}{:02X}",
            channel(rgb.r),
            channel(rgb.g),
            channel(rgb.b)
        )
    }

    /// GitTurtle patch: a color drawn in both the featured row and the
    /// palette checks only its first swatch, as a radio group has one checked
    /// member, and a color the palette does not draw checks none, though the
    /// cursor still starts on the palette's first swatch.
    #[gpui::test]
    fn only_the_first_swatch_of_a_color_drawn_twice_is_checked(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let cx = cx.add_empty_window();
        cx.update(|window, cx| {
            let blue = hsla(0.66, 1., 0.5, 1.);
            let rows = vec![
                vec![PaletteColor::unnamed(blue)],
                vec![
                    PaletteColor::unnamed(hsla(0., 1., 0.5, 1.)),
                    PaletteColor::unnamed(blue),
                ],
            ];
            let at = |value: Hsla, window: &mut Window, cx: &mut App| {
                let state = cx.new(|cx| ColorPickerState::new(window, cx).default_value(value));
                let grid = cx.new(|_| SwatchGrid::new(state));
                grid.update(cx, |grid, cx| {
                    grid.sync(&rows, window, cx);
                });
                grid
            };
            let drawn_twice = at(blue, window, cx);
            assert_eq!(drawn_twice.read(cx).checked, Some((0, 0)));
            let absent = at(hsla(0.3, 0.2, 0.7, 1.), window, cx);
            assert_eq!(absent.read(cx).checked, None);
            assert_eq!(absent.read(cx).cursor, Some((0, 0)));
        });
    }

    /// GitTurtle patch: the gap between swatches leaves a pixel of surface
    /// beyond the focus ring at every text size, before a neighbour that
    /// may be close to the accent.
    #[gpui::test]
    fn the_swatch_gap_leaves_surface_beyond_the_ring(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let cx = cx.add_empty_window();
        cx.update(|window, cx| {
            let ring = cx.theme().button_focus_ring;
            let footprint = ring.width + ring.gap;
            window.set_rem_size(px(11.));
            assert_eq!(swatch_gap(window, cx), footprint + px(1.));
            window.set_rem_size(px(40.));
            assert_eq!(swatch_gap(window, cx), px(10.), "a quarter rem above that");
        });
    }

    /// GitTurtle patch: swatch ids do not collide, however many featured
    /// colors a caller supplies.
    #[test]
    fn swatch_ids_are_unique_for_a_long_featured_row() {
        assert_ne!(swatch_id(0, 100), swatch_id(1, 0));
        assert_ne!(swatch_id(0, 1), swatch_id(1, 1));
    }

    /// One step of an arrow key: along the row, or to the swatch above or
    /// below; past the edge of the palette the cursor stays, and a row with
    /// no swatches is passed over.
    #[test]
    fn an_arrow_moves_one_swatch_and_stops_at_the_palettes_edge() {
        let lengths = [12, 11, 11];
        assert_eq!(step_from(&lengths, (0, 0), Step::Left), None);
        assert_eq!(step_from(&lengths, (0, 0), Step::Up), None);
        assert_eq!(step_from(&lengths, (0, 11), Step::Right), None);
        assert_eq!(step_from(&lengths, (2, 10), Step::Down), None);
        assert_eq!(step_from(&lengths, (1, 3), Step::Right), Some((1, 4)));
        assert_eq!(step_from(&lengths, (1, 3), Step::Left), Some((1, 2)));
        assert_eq!(step_from(&lengths, (1, 3), Step::Up), Some((0, 3)));
        assert_eq!(
            step_from(&lengths, (0, 11), Step::Down),
            Some((1, 10)),
            "the featured row is a swatch longer than the shades under it"
        );
        assert_eq!(step_from(&[0, 3], (1, 2), Step::Up), None);
        assert_eq!(step_from(&[0, 3], (1, 2), Step::Down), None);
    }

    /// Every palette swatch's name says its color, family and shade, and
    /// gives its value, and every default featured swatch's says its color
    /// and light form; a color a caller supplies is named by its value.
    #[gpui::test]
    fn every_swatch_says_its_color(cx: &mut TestAppContext) {
        cx.update(crate::init);
        cx.update(|cx| {
            let palette = color_palettes();
            assert_eq!(palette.len(), 9);
            let mut names: Vec<&SharedString> = palette
                .iter()
                .flatten()
                .inspect(|item| {
                    assert!(
                        item.name.ends_with(rounded_hex(item.color).as_str()),
                        "{} gives its value",
                        item.name
                    )
                })
                .map(|item| &item.name)
                .collect();
            assert_eq!(names.len(), 99);
            assert_eq!(palette[1][0].name.split(',').next(), Some("Red 950"));
            assert_eq!(palette[1][5].name.as_ref(), "Red 500, #EF4444");
            assert_eq!(palette[6][5].name.as_ref(), "Blue 500, #3B82F6");
            assert_eq!(value_title(Some(palette[1][5].color)).as_ref(), "#EF4444");
            assert_eq!(value_title(Some(palette[6][5].color)).as_ref(), "#3B82F6");
            assert_eq!(value_title(None).as_ref(), "");
            assert_eq!(palette[1][10].name.split(',').next(), Some("Red 50"));
            names.sort();
            names.dedup();
            assert_eq!(names.len(), 99, "no two shades share a name");

            let featured = theme_colors(cx);
            assert_eq!(featured.len(), 12);
            assert!(featured[0].name.starts_with("Red, #"));
            assert!(featured[1].name.starts_with("Light Red, #"));
            assert!(featured[11].name.starts_with("Light Magenta, #"));

            let caller = PaletteColor::unnamed(hsla(0., 1., 0.5, 1.));
            assert_eq!(caller.name.as_ref(), rounded_hex(hsla(0., 1., 0.5, 1.)));
        });
    }

    /// A palette's cursor starts on the first swatch of the picker's color,
    /// or the palette's first where that color is not in it, and from then
    /// on keeps the swatch focus last left, surviving a row that shrinks.
    #[gpui::test]
    fn the_cursor_starts_on_the_pickers_color_and_follows_focus(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let cx = cx.add_empty_window();
        cx.update(|window, cx| {
            let red = hsla(0., 1., 0.5, 1.);
            let blue = hsla(0.66, 1., 0.5, 1.);
            let other = hsla(0.3, 0.2, 0.7, 1.);
            let rows = vec![
                vec![PaletteColor::unnamed(blue), PaletteColor::unnamed(other)],
                vec![PaletteColor::unnamed(red), PaletteColor::unnamed(blue)],
            ];

            let at = |value: Hsla, window: &mut Window, cx: &mut App| {
                let state = cx.new(|cx| ColorPickerState::new(window, cx).default_value(value));
                let grid = cx.new(|_| SwatchGrid::new(state));
                grid.update(cx, |grid, cx| grid.sync(&rows, window, cx));
                grid
            };
            let on_red = at(red, window, cx);
            assert_eq!(on_red.read(cx).cursor, Some((1, 0)));
            let on_blue = at(blue, window, cx);
            assert_eq!(
                on_blue.read(cx).cursor,
                Some((0, 0)),
                "the first swatch of a color drawn twice"
            );
            let elsewhere = at(hsla(0.1, 0.3, 0.3, 1.), window, cx);
            assert_eq!(elsewhere.read(cx).cursor, Some((0, 0)));

            let focus = on_red.read(cx).rows[0][1].0.clone();
            focus.focus(window, cx);
            on_red.update(cx, |grid, cx| {
                grid.sync(&rows, window, cx);
            });
            assert_eq!(
                on_red.read(cx).cursor,
                Some((0, 1)),
                "the cursor follows focus"
            );

            window.blur(cx);
            on_red.update(cx, |grid, cx| {
                grid.sync(&rows[..1], window, cx);
            });
            assert_eq!(on_red.read(cx).cursor, Some((0, 1)), "kept once focus left");
        });
    }

    #[gpui::test]
    fn an_explicit_accessibility_label_replaces_the_visible_one(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let cx = cx.add_empty_window();
        cx.update(|window, cx| {
            let state = cx.new(|cx| ColorPickerState::new(window, cx));

            let plain = ColorPicker::new(&state).label("Color");
            assert_eq!(plain.accessibility_label, None);
            assert_eq!(plain.label.as_deref(), Some("Color"));

            let named = ColorPicker::new(&state)
                .label("Color")
                .accessibility_label("Text color");
            assert_eq!(
                named.accessibility_label.as_deref(),
                Some("Text color"),
                "an explicit name must win over the visible label"
            );
            assert_eq!(
                named.label.as_deref(),
                Some("Color"),
                "and must not change what is drawn"
            );
        });
    }
}

#[derive(IntoElement)]
struct ColorPickerButton {
    id: ElementId,
    selected: bool,
    icon: Option<Icon>,
    value: Option<Hsla>,
    size: Size,
    label: Option<SharedString>,
    tooltip: Option<SharedString>,
}

impl Selectable for ColorPickerButton {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl Sizable for ColorPickerButton {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl RenderOnce for ColorPickerButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let has_icon = self.icon.is_some();
        h_flex()
            .id(self.id)
            .gap_2()
            .children(self.icon)
            .when(!has_icon, |this| {
                this.child(
                    div()
                        .id("square")
                        .bg(cx.theme().tokens.background)
                        .border_1()
                        .border_color(cx.theme().input)
                        .rounded(cx.theme().radius)
                        .overflow_hidden()
                        .size_with(self.size)
                        .when_some(self.value, |this, value| {
                            this.bg(value)
                                .border_color(value.darken(0.3))
                                .when(self.selected, |this| this.border_2())
                        })
                        .when_some(self.tooltip, |this, tooltip| {
                            this.managed_tooltip(move |window, cx| {
                                Tooltip::new(tooltip.clone()).build(window, cx)
                            })
                        }),
                )
            })
            .when_some(self.label, |this, label| this.child(label))
    }
}
