//! Built-in themes, semantic palette tokens, readability fitting, and layout density.

use crate::highlights::{
    Chosen, EditorHighlights, SyntaxRoles, keeps_colour_blind_gaps, lightness_beyond, line_moves,
    mix, stands_beyond,
};
use crate::{custom, sources, ui_scale};
use serde::{Deserialize, Serialize};

/// Pure RGBA color in linear 0.0..=1.0 float components matching GPUI's `Rgba`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RgbaColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

/// Convert packed `0xRRGGBB` into an opaque [`RgbaColor`].
pub fn rgb_color(hex: u32) -> RgbaColor {
    RgbaColor {
        r: ((hex >> 16) & 0xff) as f32 / 255.,
        g: ((hex >> 8) & 0xff) as f32 / 255.,
        b: (hex & 0xff) as f32 / 255.,
        a: 1.0,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    Graphite,
    Daylight,
    TokyoNight,
    CatppuccinMocha,
    Nord,
    Porcelain,
    Sandstone,
    DeepSea,
    Ember,
    SolarizedDark,
    SolarizedLight,
    OneDark,
    OneLight,
    RosePine,
    RosePineDawn,
    Dracula,
    Alucard,
    KanagawaWave,
    KanagawaLotus,
    #[default]
    #[serde(other)]
    Midnight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub canvas: u32,
    pub panel: u32,
    pub subtle: u32,
    pub hover: u32,
    pub border: u32,
    pub text: u32,
    pub muted: u32,
    pub accent: u32,
    pub accent_foreground: u32,
    pub accent_hover: u32,
    pub accent_active: u32,
    pub selected: u32,
    pub added: u32,
    pub removed: u32,
    pub modified: u32,
    pub renamed: u32,
    pub warning: u32,
    pub added_background: u32,
    pub removed_background: u32,
    pub hunk: u32,
    pub line_number: u32,
}

impl Palette {
    /// Preserve the selection while giving a selected, clickable row feedback.
    pub fn row_hover(self, selected: bool) -> u32 {
        if !selected {
            return self.hover;
        }
        let mut color = 0;
        for shift in [0, 8, 16] {
            let base = (self.selected >> shift) & 0xff;
            let accent = (self.accent >> shift) & 0xff;
            color |= ((base * 93 + accent * 7) / 100) << shift;
        }
        color
    }

    /// The shared button's fill for a state surface (`hover` or `selected`): a
    /// translucent layer that GPUI composites over whatever the button sits on.
    ///
    /// Over `panel`, the surface the readability rules measure both states
    /// against, the layer composites to exactly `state`. Elsewhere it repeats
    /// that step, so a button in a hovered or selected row lifts again, where an
    /// opaque fill would vanish into a row of the same surface; and hover and
    /// pressed stay as far apart on every surface as the palette's own rows. The
    /// opacity is the smallest that still reaches `state` from `panel` with a
    /// displayable color, which keeps the step nearly the same everywhere: the
    /// layer moves a surface `s` by `(state - panel) + alpha * (panel - s)`.
    pub fn control_fill(self, state: u32) -> RgbaColor {
        let (panel, state) = (rgb_color(self.panel), rgb_color(state));
        let steps = [(panel.r, state.r), (panel.g, state.g), (panel.b, state.b)];
        // Below one 8-bit step the layer could not change a pixel.
        let alpha = steps.iter().fold(1. / 255., |alpha: f32, &(from, to)| {
            // Past this opacity the channel's layer color leaves 0..=1.
            alpha.max(if to > from {
                (to - from) / (1. - from)
            } else if to < from {
                (from - to) / from
            } else {
                0.
            })
        });
        let [r, g, b] = steps.map(|(from, to)| (from + (to - from) / alpha).clamp(0., 1.));
        RgbaColor { r, g, b, a: alpha }
    }

    /// What the shared button ([`control_button_variant`]) shows over an
    /// opaque `surface` at rest, hovered and pressed, selected where
    /// `selected`, as GPUI blends its fill: for content drawn on the button
    /// that cuts itself out of it, as the compact Changes count's ring does.
    /// Unselected, it is transparent at rest and lays its
    /// [`Self::control_fill`] layers over the surface; selected, it is
    /// opaque `selected`, and [`control_selected_hover`] while hovered or
    /// pressed, since the kit gives a selected control no pressed fill.
    pub fn control_fills(self, selected: bool, surface: u32) -> [u32; 3] {
        if selected {
            let hovered = self.row_hover(true);
            [self.selected, hovered, hovered]
        } else {
            [
                surface,
                composite(self.control_fill(self.hover), surface),
                composite(self.control_fill(self.selected), surface),
            ]
        }
    }

    /// An enabled, unchecked Switch's track under the pointer: its resting
    /// `border` fill moved by the step a row takes from `panel` to `hover`, so
    /// the track lifts as the rows around it do. The shared button's
    /// [`Self::control_fill`] layer would shrink that step over a `border`
    /// lighter than `panel`, below a visible change in One Dark.
    pub fn switch_hover(self) -> u32 {
        [16, 8, 0].into_iter().fold(0, |color, shift| {
            let channel = |value: u32| ((value >> shift) & 0xff) as i32;
            let lifted = channel(self.border) + channel(self.hover) - channel(self.panel);
            color | ((lifted.clamp(0, 255) as u32) << shift)
        })
    }

    /// The shared button's label: `text`, unless `text` falls below the text
    /// rule over one of the fills the button composites. Then it moves toward
    /// black in a light palette and white in a dark one until it clears the rule
    /// with the rasterization margin tuned tokens keep.
    ///
    /// A fill that lifts a surface toward the label lowers the label's contrast
    /// by exactly that lift, so where `text` has little room on a row band (the
    /// hovered selected row in Kanagawa Lotus and One Dark) no opacity keeps
    /// both the lift and the rule; the label takes the difference instead. The
    /// kit paints one foreground for every state, so the resting label is the
    /// same color.
    pub fn control_label(self) -> u32 {
        let fills = [
            self.control_fill(self.hover),
            self.control_fill(self.selected),
        ];
        let surfaces = [
            self.panel,
            self.subtle,
            self.canvas,
            self.hover,
            self.selected,
            self.row_hover(true),
        ];
        let readable = |label, minimum| {
            fills.iter().all(|&fill| {
                surfaces
                    .iter()
                    .all(|&surface| custom::contrast(label, composite(fill, surface)) >= minimum)
            })
        };
        if readable(self.text, LABEL_RULE) {
            return self.text;
        }
        let extreme = if custom::luminance(self.text) < custom::luminance(self.panel) {
            0x000000
        } else {
            0xffffff
        };
        toward_extreme(self.text, extreme, |label| {
            readable(label, LABEL_RULE + LABEL_MARGIN)
        })
    }

    /// The surface of menus, popovers, selects and tooltips: the lightest of
    /// the page's surfaces (subtle, canvas and panel, the panel on a tie), so
    /// a floating surface sits raised above the page, lighter in dark and
    /// light palettes alike, as GNOME and macOS draw menus. Its edge is the
    /// `border`. Every surface rule already holds `text` and `muted` on it,
    /// and the menu's hover row keeps its step off it.
    pub fn raised(self) -> u32 {
        [self.subtle, self.canvas, self.panel]
            .into_iter()
            .max_by(|&a, &b| custom::luminance(a).total_cmp(&custom::luminance(b)))
            .unwrap_or(self.panel)
    }

    /// The edge of menus, popovers and selects: `border`, which the border
    /// rule holds only against the panel, unless it misses [`POPUP_EDGE`]
    /// against the raised surface or a page surface the popup may lie on. Then
    /// it takes the first of 256 even steps toward black in a light palette or
    /// white in a dark one that clears it on all of them, which lands it just
    /// above the floor rather than a coarse step past it.
    pub fn raised_edge(self) -> u32 {
        let surfaces = [self.raised(), self.subtle, self.canvas, self.panel];
        let clears = |edge| {
            surfaces
                .iter()
                .all(|&surface| custom::contrast(edge, surface) >= POPUP_EDGE)
        };
        if clears(self.border) {
            return self.border;
        }
        let extreme = if self.is_light() { 0x000000 } else { 0xffffff };
        toward_extreme_in(256, self.border, extreme, clears)
    }

    /// The opacity of the black a dialog's backdrop lays over the page: the
    /// least, from the kit's own (20% in a dark palette, 5% in a light one)
    /// in steps of 1%, that leaves the dialog's surface, `canvas`, at least
    /// [`DIALOG_LIFT`] lighter than every page surface it dims (canvas, panel
    /// and subtle), so the page recedes beneath the dialog. With the kit's
    /// 20%, Midnight's dialog stood 1.03:1 over the dimmed canvas, 1.00:1
    /// over the dimmed subtle and darker than History's dimmed panel
    /// (1.03:1 the other way), so it read as a hole in the page with only
    /// its edge to mark it. Where no backdrop up to [`DIALOG_BACKDROP_MAX_PERCENT`]
    /// lifts a near-black canvas that far over a lighter panel (Omarchy
    /// ethereal and hackerman), the least that leaves the dialog no darker
    /// than any dimmed surface; where none does that either (vantablack's
    /// black canvas), the kit's own, since no more black would help. The
    /// backdrop is blended over the page's sRGB values, as it is drawn.
    pub fn dialog_backdrop(self) -> f64 {
        let lifts = |alpha: f64, lift: f64| {
            [self.canvas, self.panel, self.subtle]
                .iter()
                .all(|&surface| {
                    let dimmed = backdrop_over(surface, alpha);
                    custom::luminance(self.canvas) >= custom::luminance(dimmed)
                        && custom::contrast(self.canvas, dimmed) >= lift
                })
        };
        let base = if self.is_light() { 5 } else { 20 };
        let steps =
            || (base..=DIALOG_BACKDROP_MAX_PERCENT).map(|percent| f64::from(percent) / 100.);
        steps()
            .find(|&alpha| lifts(alpha, DIALOG_LIFT))
            .or_else(|| steps().find(|&alpha| lifts(alpha, 1.)))
            .unwrap_or(f64::from(base) / 100.)
    }

    /// The 1 px edge of a dialog's panel (the kit's `Theme::dialog_edge`) and
    /// of the bordered Cancel on it (the kit's `Theme::secondary_edge`):
    /// `border`, unless it misses [`DIALOG_EDGE`] against the dialog's
    /// surface, `canvas`, or a page surface its backdrop dims
    /// ([`Self::dialog_backdrop`]), as every palette's does: Braden's stood
    /// 1.28:1 against its dialog and 1.08:1 against the dimmed page. Then it
    /// takes the first of 256 even steps toward black in a light palette or
    /// white in a dark one, the text's side as for the popup edge, that
    /// clears it on all of them.
    pub fn dialog_edge(self) -> u32 {
        let alpha = self.dialog_backdrop();
        let surfaces = [
            self.canvas,
            backdrop_over(self.canvas, alpha),
            backdrop_over(self.panel, alpha),
            backdrop_over(self.subtle, alpha),
        ];
        let clears = |edge| {
            surfaces
                .iter()
                .all(|&surface| custom::contrast(edge, surface) >= DIALOG_EDGE)
        };
        if clears(self.border) {
            return self.border;
        }
        let extreme = if self.is_light() { 0x000000 } else { 0xffffff };
        toward_extreme_in(256, self.border, extreme, clears)
    }

    /// The thumb of every scrollbar the app draws (the kit's
    /// `scrollbar_thumb`): `tags::ScrollCue`'s lists, the dialogs' and
    /// pickers' bodies and every other scrollbar the kit draws. It is `border`,
    /// unless that misses [`SCROLL_THUMB`] against a surface a track lies
    /// on, the canvas, the panel, `subtle` and the raised surface the
    /// pickers' tracks rest on, either as fitted or as drawn a channel step
    /// nearer that surface ([`drawn_nearer`]). Then it takes the first of 256
    /// even steps toward black in a light palette or white in a dark one, the
    /// text's side, that clears it on all of them. A thumb is the only cue
    /// that more content lies past a list's edge, and in `border` it stood
    /// 1.52:1 off Midnight's canvas, 1.39:1 off its panel and 1.28:1 off
    /// Braden's canvas. Fitted without the step, Midnight's #444f5f held
    /// 2.01:1 on its raised panel and drew #434e5e, 1.98:1, on Keyboard
    /// Shortcuts' track. Hovered, it takes `muted`, secondary text, which
    /// reads 4.5:1.
    pub fn scrollbar_thumb(self) -> u32 {
        let surfaces = [self.canvas, self.panel, self.subtle, self.raised()];
        let clears = |thumb| {
            surfaces.iter().all(|&surface| {
                custom::contrast(thumb, surface) >= SCROLL_THUMB
                    && custom::contrast(drawn_nearer(thumb, surface), surface) >= SCROLL_THUMB
            })
        };
        if clears(self.border) {
            return self.border;
        }
        let extreme = if self.is_light() { 0x000000 } else { 0xffffff };
        toward_extreme_in(256, self.border, extreme, clears)
    }

    /// The rows a thumb drawn over them rests on ([`Self::over_rows_thumb`]):
    /// the panel, a hovered row, a selected row and a hovered selected one.
    pub fn thumb_rows(self) -> [(&'static str, u32); 4] {
        [
            ("panel", self.panel),
            ("hovered row", self.row_hover(false)),
            ("selected row", self.selected),
            ("hovered selected row", self.row_hover(true)),
        ]
    }

    /// The thumb of a scrollbar whose track lies over rows and takes no fill,
    /// the Working inspector's short body's (`tags::ScrollCue::over_rows`),
    /// so the thumb rests on the rows beneath it ([`Self::thumb_rows`]) rather
    /// than on a track's canvas: the first of 256 even steps from
    /// [`Self::scrollbar_thumb`] toward `text`, that thumb included, that
    /// holds [`SCROLL_THUMB`] against each of them by WCAG's luminance ratio,
    /// as fitted and as drawn a channel step nearer the row
    /// ([`drawn_nearer`]), the closest deciding. The palette-wide thumb
    /// misses on a hovered selected row in every shipped palette, and stood
    /// 1.46:1 on Midnight's selected row (#45505f on #223b3b), where this
    /// one is #66707d, 2.38:1. Where no step toward the text holds it, as
    /// where the text itself reads under 2:1 on a row, it takes the first of
    /// 256 even steps toward black in a light palette or white in a dark one
    /// that does, as [`Self::scrollbar_thumb`] does, and never the text. The
    /// rule is the luminance ratio, never an OKLab distance alone, which a
    /// hue's saturation can meet while the thumb's lightness sits on the
    /// row's.
    pub fn over_rows_thumb(self) -> u32 {
        self.thumb_resting_on(&self.thumb_rows().map(|(_, row)| row))
    }

    /// The thumb of a scrollbar on a failed page's notice, whose track rests
    /// on the notice's `removed_background` (`tags::Surface::Removed`), as
    /// the pickers' tracks rest on their raised surface: fitted to that
    /// surface as [`Self::over_rows_thumb`] is to its rows, the first of 256
    /// even steps from [`Self::scrollbar_thumb`] toward `text`, that thumb
    /// included, that holds [`SCROLL_THUMB`] there by WCAG's luminance
    /// ratio, as fitted and as drawn a channel step nearer
    /// ([`drawn_nearer`]), else the first toward white in a dark palette or
    /// black in a light one that does. The palette's thumb, fitted to the
    /// track's other surfaces, stood 1.66:1 on Ember's removed background
    /// (#5f4f60 on #472b37) and 1.74:1 on Midnight's.
    pub fn removed_thumb(self) -> u32 {
        self.thumb_resting_on(&[self.removed_background])
    }

    /// The thumb fitted to `surfaces` ([`Self::over_rows_thumb`]): the
    /// least step from [`Self::scrollbar_thumb`] toward `text`, that thumb
    /// included, that holds [`SCROLL_THUMB`] on each of them as fitted and
    /// as drawn a channel step nearer, or, where no step toward the text
    /// does, the least toward the extreme on the text's side; never the text.
    fn thumb_resting_on(self, surfaces: &[u32]) -> u32 {
        let clears = |thumb| {
            surfaces.iter().all(|&surface| {
                custom::contrast(thumb, surface) >= SCROLL_THUMB
                    && custom::contrast(drawn_nearer(thumb, surface), surface) >= SCROLL_THUMB
            })
        };
        let thumb = self.scrollbar_thumb();
        let toward_text = toward_extreme_in(256, thumb, self.text, clears);
        if clears(toward_text) {
            return toward_text;
        }
        let extreme = if self.is_light() { 0x000000 } else { 0xffffff };
        toward_extreme_in(256, thumb, extreme, clears)
    }

    /// The edge of an outline drawn on a picker's selected row, such as a
    /// Keyboard Shortcuts key chip: [`Self::raised_edge`], the edge it takes
    /// on a resting row, unless that misses [`POPUP_EDGE`] against `selected`,
    /// as Midnight's #2c384a did on #223b3b (1.01:1) and Braden's #c4ced9 on
    /// #d6e7e2 (1.24:1). Then it takes the first of 256 even steps toward black
    /// in a light palette or white in a dark one, the text's side as for the
    /// raised edge, that clears it there.
    pub fn selected_edge(self) -> u32 {
        let clears = |edge| custom::contrast(edge, self.selected) >= POPUP_EDGE;
        let edge = self.raised_edge();
        if clears(edge) {
            return edge;
        }
        let extreme = if self.is_light() { 0x000000 } else { 0xffffff };
        toward_extreme_in(256, edge, extreme, clears)
    }

    /// The 1 px edge of a disabled Button that rests on the neutral `subtle`
    /// fill, a primary action, a secondary control or a selected shared
    /// helper (the kit's `Theme::disabled_edge`): `border`, unless it misses
    /// [`DISABLED_EDGE`] against a surface such a Button rests on, the page's
    /// surfaces and the raised menu surface, the fill itself among them. Then
    /// it takes the first of 256 even steps toward black in a light palette or
    /// white in a dark one, the text's side as for the popup edge, that clears
    /// it on all of them. The fill stands no more than 1.34:1 off any of them,
    /// and 1.03 to 1.05:1 off a dialog in Rosé Pine, Graphite, Midnight,
    /// Braden and Ember, where a disabled default action drew only its dim
    /// label. A fill 1.5:1 off its surface would read stronger than an
    /// enabled secondary control and pull the disabled label below 3:1.
    /// Disabled filled Buttons stand on page surfaces only, so the hovered
    /// and selected rows are left out: fitting against `selected` made every
    /// edge heavier (2 to 2.4:1 on dark pages) for a case that does not
    /// occur. A future one on a row surface gets its own fit then.
    pub fn disabled_edge(self) -> u32 {
        let surfaces = [self.canvas, self.panel, self.subtle, self.raised()];
        let clears = |edge| {
            surfaces
                .iter()
                .all(|&surface| custom::contrast(edge, surface) >= DISABLED_EDGE)
        };
        if clears(self.border) {
            return self.border;
        }
        let extreme = if self.is_light() { 0x000000 } else { 0xffffff };
        toward_extreme_in(256, self.border, extreme, clears)
    }

    /// The label of a control that cannot be used, the one dim disabled
    /// Buttons, Switch labels, menu items with their shortcuts and command
    /// palette rows share (the kit's `Theme::disabled_foreground`): a layer of
    /// `muted` at half strength, or at the first of 16 even steps toward three
    /// quarters that reads [`DISABLED_LABEL`] on every surface such a label
    /// rests on: the page's surfaces and the raised menu surface, and the
    /// translucent fill the kit's default Button (Settings' Save) takes on
    /// each of them while disabled ([`Self::default_disabled_fill`]). It keeps
    /// [`DISABLED_LABEL_MARGIN`] of room above that for 1x rendering, unless
    /// the step that does would take more than [`DISABLED_SHARE`] of enabled
    /// secondary text's contrast on one of them or on the highlighted row
    /// (`selected`), as in Kanagawa Lotus. Half of a dark secondary text reads
    /// about 2:1 on a light surface, where most dark palettes already read 3:1
    /// at half. A translucent layer still dims with whatever it lies on, and
    /// the three-quarter bound keeps it fainter than enabled secondary text
    /// even where `muted` itself is weak.
    pub fn disabled_label(self) -> RgbaColor {
        self.fit_disabled_label(
            DISABLED_LABEL,
            DISABLED_SHARE,
            16,
            self.default_disabled_fill(),
        )
    }

    /// [`Self::disabled_label`] under More contrast, fitted on the adjusted
    /// palette ([`Self::with_more_contrast`]): the same layer of `muted`,
    /// from half strength in up to 28 steps of 1/64 to fifteen sixteenths,
    /// that reads [`MORE_CONTRAST_DISABLED`] on the same surfaces with the
    /// same margin while keeping at most [`MORE_CONTRAST_DISABLED_SHARE`] of
    /// secondary text's contrast, so it stays fainter than the secondary
    /// text beside it. Half of a 7:1 `muted` reads about 3:1 on a dark
    /// surface, and the three quarters [`Self::disabled_label`] stops at
    /// about 4:1. The default Button's disabled fill it is measured on is
    /// `chosen`'s, the palette before the adjustment, since a fill keeps its
    /// value under More contrast ([`Self::configure`]).
    pub fn more_contrast_disabled_label(self, chosen: Palette) -> RgbaColor {
        self.fit_disabled_label(
            MORE_CONTRAST_DISABLED,
            MORE_CONTRAST_DISABLED_SHARE,
            28,
            chosen.default_disabled_fill(),
        )
    }

    /// The disabled label's fit: the first layer of `muted`, from half
    /// strength in up to `steps` steps of 1/64, that reads `floor` with
    /// [`DISABLED_LABEL_MARGIN`] on every surface a disabled label rests on
    /// (the page's, the raised one and the default Button's disabled `fill`
    /// over each) while keeping at most `share` of secondary text's contrast
    /// there and on `selected`; else the first that reads `floor`; else the
    /// last step.
    fn fit_disabled_label(self, floor: f64, share: f64, steps: u8, fill: RgbaColor) -> RgbaColor {
        let page = [self.canvas, self.panel, self.subtle, self.raised()];
        let surfaces = page
            .into_iter()
            .chain(page.map(|surface| composite(fill, surface)));
        let reading = |label, surface| custom::contrast(composite(label, surface), surface);
        let reads = |label, floor| {
            surfaces
                .clone()
                .all(|surface| reading(label, surface) >= floor)
        };
        let fainter = |label| {
            surfaces.clone().chain([self.selected]).all(|surface| {
                reading(label, surface) <= custom::contrast(self.muted, surface) * share
            })
        };
        let layer = |step: u8| RgbaColor {
            a: 0.5 + f32::from(step) / 64.,
            ..rgb_color(self.muted)
        };
        let layers = || (0..=steps).map(layer);
        layers()
            .find(|&label| reads(label, floor + DISABLED_LABEL_MARGIN) && fainter(label))
            .or_else(|| layers().find(|&label| reads(label, floor)))
            .unwrap_or(layer(steps))
    }

    /// The fill the kit's default Button (no variant: Settings' Save)
    /// paints over its surface while disabled: half of the
    /// kit's input background, which is the `input` color (the app's `border`)
    /// at 30% in a dark palette and the `background` (`canvas`) in a light
    /// one. Over Settings' panel card it is Midnight's #1a222f, where the dim
    /// fitted without it read 3.002:1. Under More contrast the kit derives
    /// the fill from the palette as chosen (`Theme::input_fill_base`,
    /// [`Self::configure`]), so the fill drawn is the unadjusted palette's.
    pub fn default_disabled_fill(self) -> RgbaColor {
        if self.is_light() {
            RgbaColor {
                a: 0.5,
                ..rgb_color(self.canvas)
            }
        } else {
            RgbaColor {
                a: 0.15,
                ..rgb_color(self.border)
            }
        }
    }

    /// This palette as More contrast draws it, the one adjustment
    /// [`Self::apply`] makes while [`MoreContrast`] is on, before it derives
    /// anything else, derived on the fly and never saved: the added and
    /// removed line tints [`Self::more_contrast_fit`] decides, and secondary
    /// text, gutter numbers and `border` raised on them
    /// ([`Self::with_raised_floors`]). Every other token stays.
    pub fn with_more_contrast(self) -> Self {
        self.more_contrast_fit().0
    }

    /// [`Self::with_more_contrast`] with the changed-word tints decided beside its
    /// line tints: the one place that decides the diff's tints under More
    /// contrast. Each word tint drawn with More contrast off that lies beyond
    /// its line from the canvas ([`lightness_beyond`]) is lifted from its line
    /// in OKLab lightness where it stands faintest
    /// ([`EditorHighlights::lifted_word_tint`]), measured on the line tints as
    /// chosen with the syntax raised on them. Where such a word tint still
    /// stands nearer its line than [`WORD_LIGHTNESS_GAP`], that side's line
    /// tint moves away from it ([`line_moves`]), the added side first, through
    /// the steps on which every color drawn on the diff's lines and words reads
    /// at least as it does with More contrast off, with secondary text, gutter
    /// numbers and syntax raised on the moved line ([`Self::diff_reads_as_off`]),
    /// the selection and Find's matches stand apart from it as they do off
    /// ([`EditorHighlights::stay_apart_from`]), and the word stands from it in
    /// colour-blind vision at least as far as from the line drawn off
    /// ([`keeps_colour_blind_gaps`]): to the step before the first that would
    /// not, never past it. Every line of that side draws the moved
    /// tint: whole added or removed lines, changed lines around their words,
    /// in Diff and Split, and the gutter beside them. Last, a word tint that
    /// does not stand beyond its line from the canvas ([`stands_beyond`]),
    /// one drawn off on the line's canvas side, which keeps its line, or one
    /// the lift and the line's move leave nearer its line than
    /// [`HIGHLIGHT_DISTANCE`], steps to the line's far side
    /// ([`EditorHighlights::crossed_word_tint`]).
    ///
    /// Why the line: a word tint between its line and the text on it can only
    /// lift toward that text, which lowers the text's contrast, so where the
    /// word has no room left the line may still have some. Why a word on the
    /// canvas side keeps its line: the line would move away from it toward the
    /// text, lowering the text's contrast, and leave the word stepping across
    /// further. Why the step across: the order canvas, line, word is what makes
    /// a changed word read as emphasis of its line, and a word lifted toward
    /// the canvas reads as a hole in it (Omarchy flexoki-light's added word
    /// `#f7ffdf` at 1.005:1 against its canvas, Solarized Dark's `#0c2f00` at
    /// 1.017:1, apart by hue alone; decision, 2026-11-05: strictly, in every
    /// palette, where with More contrast off 29 palettes drew a word on its
    /// line's canvas side, Braden's removed word `#fcf6f7` on its line
    /// `#f9e5eb`).
    pub fn more_contrast_fit(self) -> (Self, Chosen) {
        let own = self.syntax_roles(None);
        let raised = self.with_raised_floors();
        let roles = raised.syntax_roles(Some(self));
        let sides = [
            (self.added, self.added_background),
            (self.removed, self.removed_background),
        ];
        let highlights = EditorHighlights::fit(self, own, None);
        let off = [highlights.added_word, highlights.removed_word];
        // A word drawn off on its line's canvas side neither lifts nor moves its line: it steps
        // across the line last.
        let beyond = [0, 1].map(|side| lightness_beyond(off[side], sides[side].1, self) > 0.);
        let words = [0, 1].map(|side| {
            let (diff, line) = sides[side];
            if beyond[side] {
                EditorHighlights::lifted_word_tint(roles, own, off[side], diff, line)
            } else {
                off[side]
            }
        });
        let with_lines = |[added_background, removed_background]: [u32; 2]| {
            Self {
                added_background,
                removed_background,
                ..self
            }
            .with_raised_floors()
        };
        let mut lines = sides.map(|(_, line)| line);
        for side in (0..2).filter(|&side| beyond[side]) {
            let off_pair = (off[side], sides[side].1);
            let moved = line_moves(lines[side], words[side], off_pair, self.canvas)
                .into_iter()
                .take_while(|&line| {
                    let mut moved = lines;
                    moved[side] = line;
                    highlights.stay_apart_from(line, sides[side].1)
                        && keeps_colour_blind_gaps(words[side], line, sides[side].1)
                        && self.diff_reads_as_off(own, with_lines(moved), off, words)
                })
                .last();
            if let Some(line) = moved {
                lines[side] = line;
            }
        }
        let palette = if lines == sides.map(|(_, line)| line) {
            raised
        } else {
            with_lines(lines)
        };
        let roles = palette.syntax_roles(Some(self));
        let patch = [roles.string, roles.removed];
        let words = [0, 1].map(|side| {
            let (diff, line) = (sides[side].0, lines[side]);
            if stands_beyond(words[side], line, self) {
                words[side]
            } else {
                EditorHighlights::crossed_word_tint(
                    roles,
                    [diff, patch[side]],
                    (line, off[side]),
                    self,
                )
            }
        });
        let chosen = Chosen {
            palette: self,
            off: highlights,
            words,
        };
        (palette, chosen)
    }

    /// Whether every color drawn on the diff's line and changed-word tints in
    /// `adjusted`, this palette under More contrast with `words` its word tints,
    /// reads there at least as the same color reads with More contrast off, on
    /// this palette's line tints and `off`, the word tints drawn off, with
    /// `own` this palette's syntax: on each line tint `text`, every syntax role,
    /// the side's diff color, secondary text and gutter numbers; on each word
    /// tint `text`, every syntax role but `removed` and the side's diff color
    /// ([`SyntaxRoles::word_colors`]).
    pub fn diff_reads_as_off(
        self,
        own: SyntaxRoles,
        adjusted: Self,
        off: [u32; 2],
        words: [u32; 2],
    ) -> bool {
        fn reads(mut drawn: impl Iterator<Item = (u32, u32)>, on: u32, off_on: u32) -> bool {
            drawn.all(|(is, was)| custom::contrast(is, on) >= custom::contrast(was, off_on))
        }
        let roles = adjusted.syntax_roles(Some(self));
        let on_line = |palette: Self, roles: SyntaxRoles, diff: u32| {
            roles
                .colors()
                .into_iter()
                .chain([diff, palette.muted, palette.line_number])
        };
        let on_word = |roles: SyntaxRoles, diff: u32| roles.word_colors().into_iter().chain([diff]);
        [
            (
                self.added,
                (adjusted.added_background, self.added_background),
                (words[0], off[0]),
            ),
            (
                self.removed,
                (adjusted.removed_background, self.removed_background),
                (words[1], off[1]),
            ),
        ]
        .into_iter()
        .all(|(diff, (line, off_line), (word, off_word))| {
            reads(
                on_line(adjusted, roles, diff).zip(on_line(self, own, diff)),
                line,
                off_line,
            ) && reads(on_word(roles, diff).zip(on_word(own, diff)), word, off_word)
        })
    }

    /// This palette with secondary text (`muted`) and gutter numbers
    /// (`line_number`) moved toward `text` until each reads
    /// [`MORE_CONTRAST_TEXT`] on the six surfaces the readability rules
    /// judge, measured as they rest: on canvas, panel, subtle and the
    /// selected row, and on the raised surface and the added and removed
    /// line tints. On the momentary surfaces, hover and the hovered selected
    /// row, each needs only the text rule ([`LABEL_RULE`]), so a hover alone
    /// never flattens secondary text into body text. On a surface where
    /// `text` itself reads below the floor, the floor is `text`'s own
    /// contrast, and since neither token reads above `text` on any of these
    /// surfaces ([`Self::toward_text`]), it becomes `text`, or a shade of
    /// `text`'s own luminance, where such a surface rests. `border` takes
    /// [`Self::resting_edge`], so dividers, fields' outlines and the popup
    /// edge fitted from it ([`Self::raised_edge`]) hold
    /// [`MORE_CONTRAST_EDGE`]; the disabled edge,
    /// the unchecked Switch track and a dark field's fill keep the unadjusted
    /// palette's ([`Self::configure`]). Every other token stays: the hues,
    /// the accent with its foreground, hover and pressed fills, the
    /// selection, the status and diff colours and their line tints. A token
    /// already past its floor keeps its value, and nothing the floors are
    /// measured on changes, so raising them again changes nothing.
    pub fn with_raised_floors(self) -> Self {
        let resting = [
            self.canvas,
            self.panel,
            self.subtle,
            self.raised(),
            self.selected,
            self.added_background,
            self.removed_background,
        ];
        let momentary = [self.hover, self.row_hover(true)];
        let floors: Vec<(u32, f64)> = resting
            .map(|surface| (surface, MORE_CONTRAST_TEXT))
            .into_iter()
            .chain(momentary.map(|surface| (surface, LABEL_RULE)))
            .collect();
        Self {
            muted: self.toward_text(self.muted, &floors),
            line_number: self.toward_text(self.line_number, &floors),
            border: self.resting_edge(),
            ..self
        }
    }

    /// `color` moved toward `text` in 256ths of each sRGB channel ([`mix`]),
    /// to the first step that reads its floor on every surface of `floors`,
    /// or `text`'s own contrast on one where that is lower, and reads above
    /// `text` on none of them: secondary text never reads above body text.
    /// Mixed in sRGB, a step can pass `text`'s luminance before it reaches
    /// `text`, so the first step to reach `text`'s own contrast read above
    /// it: Kanagawa Lotus's secondary text stopped at `#504F59`, 4.894:1
    /// against its text's 4.871:1 on the hovered selected row, and One
    /// Dark's at `#B0B5BD`, a thousandth above its text on every surface.
    /// `text` reaches every floor and reads as itself, so the walk ends
    /// there at the latest.
    fn toward_text(self, color: u32, floors: &[(u32, f64)]) -> u32 {
        let text = custom::luminance(self.text);
        let floors: Vec<(f64, f64, f64)> = floors
            .iter()
            .map(|&(surface, floor)| {
                let surface = custom::luminance(surface);
                let reachable = custom::luminance_contrast(text, surface);
                (surface, floor.min(reachable), reachable)
            })
            .collect();
        let reads = |color: u32| {
            let color = custom::luminance(color);
            floors.iter().all(|&(surface, floor, text)| {
                let ratio = custom::luminance_contrast(color, surface);
                ratio >= floor && ratio <= text
            })
        };
        (0..=256)
            .map(|step| mix(color, self.text, step))
            .find(|&step| reads(step))
            .unwrap_or(self.text)
    }

    /// The 1 px edge an enabled control that rests on a fill of its own
    /// draws over its bounds while More contrast is on, and the `border`
    /// [`Self::with_more_contrast`] gives: `border`, unless it misses
    /// [`MORE_CONTRAST_EDGE`] against the canvas, the panel, `subtle` or the
    /// raised surface. Then it takes the first of 256 even steps toward black
    /// in a light palette or white in a dark one, the text's side as for the
    /// popup edge, that clears it on all of them. On an adjusted palette it
    /// is that palette's `border`.
    pub fn resting_edge(self) -> u32 {
        let surfaces = [self.canvas, self.panel, self.subtle, self.raised()];
        let clears = |edge| {
            surfaces
                .iter()
                .all(|&surface| custom::contrast(edge, surface) >= MORE_CONTRAST_EDGE)
        };
        if clears(self.border) {
            return self.border;
        }
        let extreme = if self.is_light() { 0x000000 } else { 0xffffff };
        toward_extreme_in(256, self.border, extreme, clears)
    }

    /// A sidebar row's label and icon colors. The label stays `text`, which
    /// reads at the text rule on every row state, and selection marks the icon
    /// in `accent`, a graphic at 3:1, beside the selected surface: `accent` is
    /// fitted only to 3:1, which left a selected label at 3.0:1 in Omarchy
    /// Solitude.
    pub fn sidebar_row(self, active: bool) -> (u32, u32) {
        (self.text, if active { self.accent } else { self.muted })
    }

    /// `color` as a small label on every one of `surfaces`: badges, chips and
    /// gutter numbers at 10 to 12 px render about 0.2 below their token, so
    /// `color` stays only where it clears the text rule with the margin tuned
    /// tokens keep. Otherwise it takes the first step toward black in a light
    /// palette and white in a dark one that does, which keeps its hue.
    pub fn small_label(self, color: u32, surfaces: &[u32]) -> u32 {
        let readable = |label| {
            surfaces
                .iter()
                .all(|&surface| custom::contrast(label, surface) >= LABEL_RULE + LABEL_MARGIN)
        };
        if readable(color) {
            return color;
        }
        let extreme = if self.is_light() { 0x000000 } else { 0xffffff };
        toward_extreme(color, extreme, readable)
    }
}

/// The text rule, and the margin tuned tokens keep above it because small text
/// at 1x renders about 0.2 lower (DESIGN.md, semantic palette ownership).
pub const LABEL_RULE: f64 = 4.5;
pub const LABEL_MARGIN: f64 = 0.25;

/// The contrast a disabled label keeps on the surfaces it rests on
/// ([`Palette::disabled_label`]): readable, and well under the text rule
/// enabled labels keep, so it still reads as unavailable.
pub const DISABLED_LABEL: f64 = 3.0;
/// The room [`Palette::disabled_label`] keeps above [`DISABLED_LABEL`] on its
/// closest surface: a label at 1x renders below its token, so one fitted to
/// exactly 3:1 (Midnight's disabled Save, 3.002:1) read under it natively.
pub const DISABLED_LABEL_MARGIN: f64 = 0.05;
/// The most of enabled secondary text's contrast a disabled label keeps on a
/// surface, so disabled stays clearly fainter than enabled text.
pub const DISABLED_SHARE: f64 = 0.6;

/// The contrast a scrollbar's thumb keeps against every surface its track
/// lies on ([`Palette::scrollbar_thumb`]): the thumb is the only cue that
/// more content exists, so it reads plainly, and it stays well under the
/// 4.5:1 of secondary text, which its hovered state takes, so it never
/// competes with the content it scrolls.
pub const SCROLL_THUMB: f64 = 2.0;

/// The contrast a disabled Button's edge keeps against every surface it
/// rests on and its own fill ([`Palette::disabled_edge`]): enough that it
/// reads as a control's shape, and well under the 3:1 of the bordered Cancel
/// beside a dialog's default action, so with its dim label it still reads as
/// one that cannot act.
pub const DISABLED_EDGE: f64 = 1.5;

/// The contrast secondary text, gutter numbers and syntax reach under More
/// contrast ([`Palette::with_more_contrast`], [`Palette::apply`]): WCAG's
/// enhanced contrast for text (1.4.6), up from the text rule's 4.5:1 and
/// syntax's 4.75:1.
pub const MORE_CONTRAST_TEXT: f64 = 7.0;

/// The contrast dividers, fields' outlines and the popup, dialog, bordered
/// secondary, disabled and resting edges keep under More contrast: WCAG's
/// non-text contrast (1.4.11), up from the border rule's 1.3:1, the popup
/// edge's 1.4:1 and the disabled edge's 1.5:1.
pub const MORE_CONTRAST_EDGE: f64 = 3.0;

/// The contrast a disabled label reaches under More contrast
/// ([`Palette::more_contrast_disabled_label`]): the text rule, up from
/// [`DISABLED_LABEL`].
pub const MORE_CONTRAST_DISABLED: f64 = 4.5;
/// The most of the adjusted secondary text's contrast that label keeps on a
/// surface. Secondary text reads 7:1 there, so 4.5:1 is nearly two thirds of
/// it: the [`DISABLED_SHARE`] kept beside 4.5:1 secondary text cannot hold.
pub const MORE_CONTRAST_DISABLED_SHARE: f64 = 0.75;

/// The contrast a popup's 1 px edge keeps against its surface and the page
/// beneath it ([`Palette::raised_edge`]). Antialiasing and fractional scales
/// draw a hairline below its token: an edge fitted to the border rule's 1.3
/// drew 1.21:1 to 1.26:1 natively, so the token keeps room above that rule.
pub const POPUP_EDGE: f64 = 1.4;

/// The contrast a dialog's 1 px edge keeps against the dialog and every page
/// surface its backdrop dims, which its bordered Cancel keeps against the
/// dialog too ([`Palette::dialog_edge`]): the 3:1 a control's or a
/// container's boundary keeps (WCAG's non-text contrast). The backdrop lifts
/// a dialog as little as 1.05:1 off the page it dims, so the edge is what sets
/// the dialog apart; at the popup edge's 1.4:1 Braden's read 1.41:1 against
/// its dimmed page, and its Cancel, in `border`, 1.28:1 against the dialog.
pub const DIALOG_EDGE: f64 = 3.0;

/// How much lighter a dialog's surface stands than any page surface its
/// backdrop dims ([`Palette::dialog_backdrop`]): the step at which the page
/// reads as receding beneath the dialog rather than level with it.
pub const DIALOG_LIFT: f64 = 1.05;

/// The darkest a dialog's backdrop grows, so the page stays legible beneath
/// it.
pub const DIALOG_BACKDROP_MAX_PERCENT: u32 = 60;

/// `surface` under a backdrop of black at `alpha`, blended per sRGB channel.
pub fn backdrop_over(surface: u32, alpha: f64) -> u32 {
    [16, 8, 0]
        .map(|shift| {
            let channel = f64::from((surface >> shift) & 0xff) * (1. - alpha);
            (channel.round() as u32) << shift
        })
        .into_iter()
        .fold(0, |color, channel| color | channel)
}

/// The first of 32 even steps from `from` to `extreme`, `from` included, that
/// `accepts`, or `extreme`.
fn toward_extreme(from: u32, extreme: u32, accepts: impl Fn(u32) -> bool) -> u32 {
    toward_extreme_in(32, from, extreme, accepts)
}

/// `color` as the renderer may draw it over `surface`: each 8-bit channel
/// one step nearer the surface's, where they differ. A colour passes through
/// the toolkit's floating-point HSLA and the GPU's rounding on its way to the
/// screen, and Midnight's scrollbar thumb, #444f5f, drew #434e5e.
pub fn drawn_nearer(color: u32, surface: u32) -> u32 {
    [16, 8, 0].into_iter().fold(0, |drawn, shift| {
        let (channel, toward) = ((color >> shift) & 0xff, (surface >> shift) & 0xff);
        let channel = match channel.cmp(&toward) {
            std::cmp::Ordering::Greater => channel - 1,
            std::cmp::Ordering::Less => channel + 1,
            std::cmp::Ordering::Equal => channel,
        };
        drawn | (channel << shift)
    })
}

/// The first of `steps` even steps from `from` to `extreme`, `from` included,
/// that `accepts`, or `extreme`.
pub(crate) fn toward_extreme_in(
    steps: u32,
    from: u32,
    extreme: u32,
    accepts: impl Fn(u32) -> bool,
) -> u32 {
    (0..=steps)
        .map(|step| {
            [16, 8, 0].into_iter().fold(0, |color, shift| {
                let from = (from >> shift) & 0xff;
                let to = (extreme >> shift) & 0xff;
                color | (((from * (steps - step) + to * step) / steps) << shift)
            })
        })
        .find(|&label| accepts(label))
        .unwrap_or(extreme)
}

/// The alpha of a History reference badge's tint: its lane color over the row.
pub const REF_TINT_ALPHA: u32 = 0x1e;

/// Small labels fitted once per palette, so the rows and gutters that draw them
/// read a color instead of fitting one every frame. [`Palette::apply`]
/// installs them; [`labels`] refits for a palette applied some other way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Labels {
    pub palette: Palette,
    /// A History reference badge's label for each graph lane, on the lane's
    /// tint over every History row surface: the canvas, hovered, selected and
    /// hovered selected rows.
    pub refs: [u32; 6],
    /// The Unstaged badge's label: `modified` on `subtle`.
    pub unstaged: u32,
    /// `added` on its diff tile, `added_background`: the gutter numbers of
    /// added lines and the Staged and "N staged" badges.
    pub added: u32,
    /// `removed` on its diff tile: the gutter numbers of removed lines.
    pub removed: u32,
    /// A count chip's number: `text` on its `hover` fill.
    pub count: u32,
    /// A failure message beside the control that failed, such as a refused
    /// Open in default app: `warning` on the canvas, panel and raised
    /// surfaces a comparison and its dialogs rest on.
    pub warning: u32,
    /// The thumb a scrollbar draws over rows ([`Palette::over_rows_thumb`]),
    /// fitted here so the frames that draw it read it.
    pub over_rows_thumb: u32,
    /// The thumb a scrollbar draws on a failed page's notice
    /// ([`Palette::removed_thumb`]), fitted here alike.
    pub removed_thumb: u32,
}

impl Labels {
    pub fn fit(palette: Palette) -> Self {
        let rows = [
            palette.canvas,
            palette.hover,
            palette.selected,
            palette.row_hover(true),
        ];
        let refs = crate::palette_colors(palette).map(|lane| {
            let tint = RgbaColor {
                a: REF_TINT_ALPHA as f32 / 255.,
                ..rgb_color(lane)
            };
            palette.small_label(lane, &rows.map(|row| composite(tint, row)))
        });
        Self {
            palette,
            refs,
            unstaged: palette.small_label(palette.modified, &[palette.subtle]),
            added: palette.small_label(palette.added, &[palette.added_background]),
            removed: palette.small_label(palette.removed, &[palette.removed_background]),
            count: palette.small_label(palette.text, &[palette.hover]),
            warning: palette.small_label(
                palette.warning,
                &[palette.canvas, palette.panel, palette.raised()],
            ),
            over_rows_thumb: palette.over_rows_thumb(),
            removed_thumb: palette.removed_thumb(),
        }
    }
}

/// Composite a translucent layer over an opaque surface as GPUI's renderers
/// blend a quad: on the gamma-encoded values of their non-sRGB targets.
pub fn composite(layer: RgbaColor, surface: u32) -> u32 {
    let beneath = rgb_color(surface);
    [
        (layer.r, beneath.r, 16),
        (layer.g, beneath.g, 8),
        (layer.b, beneath.b, 0),
    ]
    .into_iter()
    .fold(0, |color, (top, bottom, shift)| {
        let value = (layer.a * top + (1. - layer.a) * bottom) * 255.;
        color | ((value.round() as u32) << shift)
    })
}

impl ThemeChoice {
    pub const ALL: [Self; 20] = [
        Self::Midnight,
        Self::Daylight,
        Self::Graphite,
        Self::TokyoNight,
        Self::CatppuccinMocha,
        Self::Nord,
        Self::Porcelain,
        Self::Sandstone,
        Self::DeepSea,
        Self::Ember,
        Self::SolarizedDark,
        Self::SolarizedLight,
        Self::OneDark,
        Self::OneLight,
        Self::RosePine,
        Self::RosePineDawn,
        Self::Dracula,
        Self::Alucard,
        Self::KanagawaWave,
        Self::KanagawaLotus,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Midnight => "Midnight",
            Self::Graphite => "Graphite",
            Self::Daylight => "Braden",
            Self::TokyoNight => "Tokyo Night",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::Nord => "Nord",
            Self::Porcelain => "Porcelain",
            Self::Sandstone => "Sandstone",
            Self::DeepSea => "Deep Sea",
            Self::Ember => "Ember",
            Self::SolarizedDark => "Solarized Dark",
            Self::SolarizedLight => "Solarized Light",
            Self::OneDark => "One Dark",
            Self::OneLight => "One Light",
            Self::RosePine => "Rosé Pine",
            Self::RosePineDawn => "Rosé Pine Dawn",
            Self::Dracula => "Dracula",
            Self::Alucard => "Alucard",
            Self::KanagawaWave => "Kanagawa Wave",
            Self::KanagawaLotus => "Kanagawa Lotus",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Midnight => "Deep slate · mint",
            Self::Daylight => "Soft white · evergreen",
            Self::Graphite => "Warm charcoal · lilac",
            Self::TokyoNight => "City blues · neon",
            Self::CatppuccinMocha => "Cozy pastels · mauve",
            Self::Nord => "Arctic blue · frost",
            Self::Porcelain => "Cool ivory · sapphire",
            Self::Sandstone => "Warm paper · terracotta",
            Self::DeepSea => "Ocean ink · turquoise",
            Self::Ember => "Smoked plum · apricot",
            Self::SolarizedDark => "Deep teal · azure",
            Self::SolarizedLight => "Warm cream · azure",
            Self::OneDark => "Soft charcoal · sky",
            Self::OneLight => "Clean paper · cobalt",
            Self::RosePine => "Dusky violet · rose",
            Self::RosePineDawn => "Blush paper · pine",
            Self::Dracula => "Night charcoal · purple",
            Self::Alucard => "Pale parchment · violet",
            Self::KanagawaWave => "Inky dusk · cornflower",
            Self::KanagawaLotus => "Rice paper · denim",
        }
    }

    pub fn is_light(self) -> bool {
        matches!(
            self,
            Self::Daylight
                | Self::Porcelain
                | Self::Sandstone
                | Self::SolarizedLight
                | Self::OneLight
                | Self::RosePineDawn
                | Self::Alucard
                | Self::KanagawaLotus
        )
    }

    pub fn palette(self) -> Palette {
        match self {
            // Original GitTurtle palettes, complete semantic surface sets.
            Self::Porcelain => Palette {
                canvas: 0xf6f7fc,
                panel: 0xffffff,
                subtle: 0xeff1f8,
                // Tuned: 1.08:1 off subtle, as the Omarchy fitter holds hover.
                hover: 0xe4e8f3,
                border: 0xcbd3e4,
                text: 0x242e49,
                muted: 0x4d5b78,
                accent: 0x3455a6,
                accent_foreground: 0xffffff,
                accent_hover: 0x294790,
                accent_active: 0x203978,
                // Leans toward the sapphire accent so a pressed button stands apart from
                // its hover on every surface (the pressed-step readability rule).
                selected: 0xdce6f6,
                added: 0x246448,
                removed: 0xa92d4e,
                modified: 0x795314,
                renamed: 0x6c459a,
                warning: 0x795314,
                added_background: 0xe3f0e9,
                removed_background: 0xf8e5ed,
                hunk: 0x3455a6,
                line_number: 0x5f6c86,
            },
            Self::Sandstone => Palette {
                canvas: 0xf8f3ea,
                panel: 0xfffcf6,
                subtle: 0xf0eade,
                hover: 0xeae1d3,
                border: 0xd4c6b5,
                text: 0x3b302b,
                muted: 0x635446,
                accent: 0x965034,
                accent_foreground: 0xffffff,
                accent_hover: 0x82432b,
                accent_active: 0x6e3723,
                // Leans toward the terracotta accent so a pressed button stands apart from
                // its hover on every surface (the pressed-step readability rule).
                selected: 0xf2dcd0,
                added: 0x396241,
                removed: 0xa13243,
                modified: 0x755012,
                renamed: 0x794a84,
                warning: 0x755012,
                added_background: 0xe7efdc,
                removed_background: 0xf6e3dd,
                hunk: 0x365e8b,
                line_number: 0x70614f,
            },
            Self::DeepSea => Palette {
                canvas: 0x0d1c27,
                panel: 0x132735,
                subtle: 0x10222f,
                hover: 0x213b4b,
                border: 0x365366,
                text: 0xe4f2f7,
                muted: 0xb2c9d6,
                accent: 0x68dccb,
                accent_foreground: 0x072d2c,
                accent_hover: 0x91e9dc,
                accent_active: 0x58c9b9,
                selected: 0x21434c,
                added: 0x83d8ae,
                removed: 0xf7a0ad,
                modified: 0xe9ca8a,
                renamed: 0xc5b2f0,
                warning: 0xe9ca8a,
                added_background: 0x183c35,
                removed_background: 0x3b2c3b,
                hunk: 0x94c9f4,
                line_number: 0x9bb7c9,
            },
            Self::Ember => Palette {
                canvas: 0x201a22,
                panel: 0x2a222c,
                subtle: 0x251e27,
                hover: 0x3b303d,
                border: 0x514052,
                text: 0xf7ece5,
                muted: 0xd0bfc7,
                accent: 0xf2b38c,
                accent_foreground: 0x382119,
                accent_hover: 0xffcba6,
                accent_active: 0xe3a27a,
                selected: 0x48343d,
                added: 0xadd3a5,
                removed: 0xf2a2b2,
                modified: 0xe8c88b,
                renamed: 0xd1b0ef,
                warning: 0xe8c88b,
                added_background: 0x303b2e,
                removed_background: 0x472b37,
                hunk: 0xb4c7ee,
                line_number: 0xbda6b6,
            },
            Self::Midnight => Palette {
                canvas: 0x10151f,
                panel: 0x171e2b,
                subtle: 0x131a25,
                hover: 0x222c3c,
                border: 0x2b3749,
                text: 0xe8eef7,
                muted: 0xa4b1c5,
                accent: 0x75e0bb,
                accent_foreground: 0x0a241d,
                accent_hover: 0x99edcf,
                accent_active: 0x5ccca6,
                selected: 0x223b3b,
                added: 0x75e0bb,
                removed: 0xff95a8,
                modified: 0xe8bc79,
                renamed: 0xc1a5f5,
                warning: 0xe8bc79,
                added_background: 0x19322d,
                removed_background: 0x382531,
                hunk: 0x95baff,
                line_number: 0x899bb5,
            },
            Self::Graphite => Palette {
                canvas: 0x18191d,
                panel: 0x202126,
                subtle: 0x1c1d22,
                hover: 0x2b2c33,
                border: 0x3b3d47,
                text: 0xefeff4,
                muted: 0xa8a9b8,
                accent: 0xb7a5ff,
                accent_foreground: 0x211936,
                accent_hover: 0xcabaff,
                accent_active: 0xa994f0,
                selected: 0x38324e,
                added: 0x91ddb3,
                removed: 0xf5a0ac,
                modified: 0xe8c58c,
                renamed: 0xb7a5ff,
                warning: 0xe8c58c,
                added_background: 0x22352c,
                removed_background: 0x3e2830,
                hunk: 0xa8bfff,
                line_number: 0x9b9daa,
            },
            Self::Daylight => Palette {
                canvas: 0xf4f6fa,
                panel: 0xffffff,
                subtle: 0xedf1f7,
                // Tuned: 1.08:1 off subtle, as the Omarchy fitter holds hover.
                hover: 0xe3e8ef,
                border: 0xd2dce8,
                text: 0x253247,
                // Tuned: 5 darker per channel for 4.75:1 on the darker selected rows.
                muted: 0x4d5d74,
                accent: 0x08755d,
                accent_foreground: 0xffffff,
                accent_hover: 0x06654f,
                accent_active: 0x055440,
                // Tuned: darkened in linear light, keeping its hue, so a pressed
                // control reads darker than its hover, at least 10 from it.
                selected: 0xd6e7e2,
                added: 0x146744,
                removed: 0xb53351,
                modified: 0x875a14,
                renamed: 0x7850b4,
                warning: 0x875a14,
                added_background: 0xe2f1e9,
                removed_background: 0xf9e5eb,
                hunk: 0x3764ae,
                line_number: 0x5b6c84,
            },
            // Base hues follow the original palette; secondary text and status
            // surfaces are tuned for readable, small native UI labels.
            // https://github.com/tokyo-night/tokyo-night-vscode-theme
            Self::TokyoNight => Palette {
                canvas: 0x1a1b26,
                panel: 0x202231,
                subtle: 0x16161e,
                hover: 0x292e42,
                border: 0x373e59,
                text: 0xc0caf5,
                muted: 0xa9b1d6,
                accent: 0x7aa2f7,
                accent_foreground: 0x151c30,
                accent_hover: 0x99b9ff,
                accent_active: 0x7299e8,
                selected: 0x2c3552,
                added: 0x9ece6a,
                removed: 0xf7768e,
                modified: 0xe0af68,
                renamed: 0xbb9af7,
                warning: 0xe0af68,
                added_background: 0x26362c,
                removed_background: 0x3c2638,
                hunk: 0x7dcfff,
                line_number: 0x8997bd,
            },
            // https://catppuccin.com/palette/#mocha
            Self::CatppuccinMocha => Palette {
                canvas: 0x1e1e2e,
                panel: 0x242436,
                subtle: 0x181825,
                hover: 0x313244,
                border: 0x45475a,
                text: 0xcdd6f4,
                muted: 0xa6adc8,
                accent: 0xcba6f7,
                accent_foreground: 0x251b33,
                accent_hover: 0xddc0ff,
                accent_active: 0xba97e8,
                selected: 0x36324b,
                added: 0xa6e3a1,
                removed: 0xf38ba8,
                modified: 0xf9e2af,
                renamed: 0xcba6f7,
                warning: 0xf9e2af,
                added_background: 0x28392f,
                removed_background: 0x3e293d,
                hunk: 0x89b4fa,
                line_number: 0x9399b2,
            },
            // https://www.nordtheme.com/docs/colors-and-palettes
            Self::Nord => Palette {
                canvas: 0x2e3440,
                panel: 0x343c4b,
                subtle: 0x292f3b,
                hover: 0x3e4555,
                border: 0x4c566a,
                text: 0xeceff4,
                muted: 0xc0c9d8,
                accent: 0x88c0d0,
                accent_foreground: 0x243039,
                accent_hover: 0xa1d3df,
                accent_active: 0x80b5c7,
                selected: 0x3b485c,
                added: 0xa3be8c,
                removed: 0xe89aa3,
                modified: 0xebcb8b,
                renamed: 0xd0acd0,
                warning: 0xebcb8b,
                added_background: 0x303e39,
                removed_background: 0x45333f,
                hunk: 0x88c0d0,
                line_number: 0xa7b4c9,
            },
            // Families adapted from `sources`. Named constants are upstream
            // values used unchanged; hex literals marked "tuned" depart from
            // the upstream value for a readability rule, with the margin 1x
            // rasterization needs, and are listed in DESIGN.md. Unmarked
            // literals are derived surfaces the upstream palette does not
            // define (subtle, hover, selected, diff tiles, accent states).
            Self::SolarizedDark => {
                use sources::solarized::{self as s, dark};
                Palette {
                    canvas: dark::CANVAS,
                    panel: dark::PANEL,
                    subtle: 0x01313d,
                    hover: 0x103c48,
                    selected: 0x0b4154,
                    border: s::BASE01,
                    // Tuned: base0 and base1 lightened for 4.5:1 on selected rows.
                    text: 0xb2bdbe,
                    muted: 0xaab5b5,
                    // Tuned: blue lightened for 3:1 on selected and hovered rows.
                    accent: 0x3499df,
                    accent_foreground: 0x001e26,
                    accent_hover: 0x48a0de,
                    accent_active: 0x278ed6,
                    // Tuned: green, red and violet lightened for 3:1 on row
                    // surfaces, 4.5:1 in diff tiles and the canvas label; yellow
                    // for 4.5:1 as warning text on subtle surfaces, and modified
                    // keeps warning's value.
                    added: 0x92a802,
                    removed: 0xea706e,
                    modified: 0xbe9209,
                    renamed: 0x898dd2,
                    warning: 0xbe9209,
                    added_background: 0x103830,
                    removed_background: 0x1a2c35,
                    // Tuned: cyan lightened for 4.5:1 on panels.
                    hunk: 0x30aea5,
                    // Tuned: base01 lightened for 4.5:1 in the gutter.
                    line_number: 0x8aa0a8,
                }
            }
            Self::SolarizedLight => {
                use sources::solarized::{self as s, light};
                Palette {
                    canvas: light::CANVAS,
                    panel: light::PANEL,
                    subtle: 0xf6efdc,
                    hover: 0xe3dfcf,
                    selected: 0xd0dad5,
                    border: s::BASE1,
                    // Tuned: base00 and base01 darkened for 4.5:1 on selected rows.
                    text: 0x394549,
                    muted: 0x44565c,
                    // Tuned: blue darkened for a 4.5:1 white label and 3:1 on rows.
                    accent: 0x1c73b1,
                    accent_foreground: 0xffffff,
                    accent_hover: 0x1d6aa0,
                    accent_active: 0x1a5e8f,
                    // Tuned: every accent darkened for 3:1 on row surfaces,
                    // 4.5:1 in diff tiles, the canvas label on fills and warning
                    // text on subtle surfaces.
                    added: 0x5b6900,
                    removed: 0xc2201e,
                    modified: 0x846200,
                    renamed: 0x6166bd,
                    warning: 0x846200,
                    added_background: 0xece9c3,
                    removed_background: 0xfae2d1,
                    // Tuned: blue darkened for 4.5:1 on resting surfaces and 3:1 on rows.
                    hunk: 0x19689e,
                    // Tuned: base01 darkened for 4.5:1 on panels.
                    line_number: 0x51666d,
                }
            }
            Self::OneDark => {
                use sources::one::dark as o;
                Palette {
                    canvas: o::BG,
                    panel: 0x2e333d,
                    subtle: 0x21252b,
                    hover: 0x333943,
                    selected: 0x323d52,
                    border: 0x4b5263,
                    // Tuned: mono-1 and mono-2 lightened for 4.5:1 on the hovered selected
                    // row at 1x; text is kept above secondary text.
                    text: 0xaeb5c2,
                    muted: 0xb0b5bc,
                    accent: o::BLUE,
                    accent_foreground: 0x1b2533,
                    accent_hover: 0x7dbdf2,
                    accent_active: 0x53a8ee,
                    added: o::GREEN,
                    // Tuned: red-1 lightened for 4.5:1 in its diff tile and under the canvas label.
                    removed: 0xe98991,
                    modified: o::ORANGE_2,
                    renamed: o::PURPLE,
                    warning: o::ORANGE_2,
                    added_background: 0x353e3c,
                    removed_background: 0x3e343c,
                    hunk: o::CYAN,
                    // Tuned: mono-2 lightened for 4.5:1 in the gutter.
                    line_number: 0x9aa0ab,
                }
            }
            Self::OneLight => {
                use sources::one::light as o;
                Palette {
                    canvas: o::BG,
                    panel: 0xffffff,
                    subtle: 0xf0f0f1,
                    // Tuned: 1.08:1 off subtle, as the Omarchy fitter holds hover.
                    hover: 0xe7e7e9,
                    // Tuned: stepped darker from #e9edff until hover reads 0.005 lighter in OKLab,
                    // so a hovered row never reads heavier than the selected one.
                    selected: 0xe0e4f6,
                    border: 0xd3d3d6,
                    text: o::MONO_1,
                    // Tuned: mono-2 darkened for 4.5:1 on selected rows.
                    muted: 0x5a5d67,
                    // Tuned: blue darkened for a 4.5:1 white label.
                    accent: 0x2d6aef,
                    accent_foreground: 0xffffff,
                    accent_hover: 0x175bef,
                    accent_active: 0x0f52e3,
                    // Tuned: green darkened for 3:1 on rows and 4.5:1 in diff tiles.
                    added: 0x377236,
                    removed: o::RED_2,
                    // Tuned: orange-1 darkened for 4.5:1 as warning text on subtle
                    // surfaces; modified keeps warning's value.
                    modified: 0x8e5e00,
                    renamed: o::PURPLE,
                    warning: 0x8e5e00,
                    added_background: 0xe6efe5,
                    removed_background: 0xf6e7eb,
                    // Tuned: cyan darkened for 4.5:1 on subtle surfaces.
                    hunk: 0x0070a1,
                    line_number: o::MONO_2,
                }
            }
            Self::RosePine => {
                use sources::rose_pine::main as r;
                Palette {
                    canvas: r::BASE,
                    panel: r::SURFACE,
                    subtle: 0x16141f,
                    hover: r::OVERLAY,
                    selected: 0x2d2a45,
                    border: 0x403d52,
                    text: r::TEXT,
                    // Tuned: subtle lightened for 4.5:1 on selected rows and diff tiles.
                    muted: 0xa6a2bc,
                    accent: r::ROSE,
                    accent_foreground: 0x2a1d25,
                    accent_hover: 0xf3cfcd,
                    accent_active: 0xe2aeac,
                    added: r::FOAM,
                    removed: r::LOVE,
                    modified: r::GOLD,
                    renamed: r::IRIS,
                    warning: r::GOLD,
                    added_background: 0x1f2e36,
                    removed_background: 0x351f30,
                    hunk: r::IRIS,
                    line_number: r::SUBTLE,
                }
            }
            Self::RosePineDawn => {
                use sources::rose_pine::dawn as r;
                Palette {
                    canvas: r::BASE,
                    panel: r::SURFACE,
                    subtle: 0xf4ede4,
                    // Tuned: overlay darkened for 1.08:1 off subtle, as the Omarchy fitter holds hover.
                    hover: 0xece3dc,
                    // Tuned: a step darker and toward iris, so a pressed control
                    // stands at least 10 from its hover, darker than it.
                    selected: 0xeadde7,
                    border: 0xdfdad9,
                    text: r::TEXT,
                    // Tuned: subtle darkened for 4.5:1 on every row surface and diff tile.
                    muted: 0x59566e,
                    accent: r::PINE,
                    accent_foreground: 0xffffff,
                    accent_hover: 0x225a70,
                    accent_active: 0x1d4d60,
                    // Tuned: foam, love, gold and iris darkened for 3:1 on row
                    // surfaces, 4.5:1 in diff tiles, the canvas label on fills and
                    // warning text on subtle surfaces.
                    added: 0x3c6b74,
                    removed: 0x934e62,
                    modified: 0x8e5c18,
                    renamed: 0x806b97,
                    warning: 0x8e5c18,
                    added_background: 0xe4ecea,
                    removed_background: 0xf6e3e3,
                    hunk: r::PINE,
                    // Tuned: subtle darkened for 4.5:1 on canvas.
                    line_number: 0x6b6783,
                }
            }
            Self::Dracula => {
                use sources::dracula::dark as d;
                Palette {
                    canvas: d::BACKGROUND,
                    panel: 0x2e303e,
                    subtle: 0x21222c,
                    hover: 0x383a4a,
                    selected: d::SELECTION,
                    border: 0x4a4d62,
                    text: d::FOREGROUND,
                    // Tuned: comment lightened for 4.5:1 on every row surface and diff tile.
                    muted: 0xbdc4db,
                    accent: d::PURPLE,
                    accent_foreground: d::BACKGROUND,
                    accent_hover: 0xcfaefb,
                    accent_active: 0xb083f7,
                    added: d::GREEN,
                    // Tuned: red lightened for 3:1 on selected rows and 4.5:1 in its diff tile.
                    removed: 0xff7979,
                    modified: d::ORANGE,
                    renamed: d::PURPLE,
                    warning: d::ORANGE,
                    added_background: 0x2b4136,
                    removed_background: 0x472e3a,
                    hunk: d::CYAN,
                    // Tuned: comment lightened for 4.5:1 in the gutter.
                    line_number: 0x909cc1,
                }
            }
            Self::Alucard => {
                use sources::dracula::alucard as a;
                Palette {
                    canvas: a::BACKGROUND,
                    panel: 0xfffdf5,
                    subtle: 0xf5f1e1,
                    // Tuned: 1.08:1 off subtle, as the Omarchy fitter holds hover.
                    hover: 0xebe8d8,
                    selected: a::SELECTION,
                    border: 0xd9d4bf,
                    text: a::FOREGROUND,
                    // Tuned: comment darkened for 4.5:1 on selected and hovered selected rows.
                    muted: 0x534e38,
                    accent: a::PURPLE,
                    accent_foreground: 0xffffff,
                    accent_hover: 0x563cb8,
                    accent_active: 0x4a32a2,
                    added: a::GREEN,
                    // Tuned: red darkened for 3:1 on hovered selected rows and 4.5:1 in its diff tile.
                    removed: 0xba3223,
                    modified: a::ORANGE,
                    renamed: a::PURPLE,
                    warning: a::ORANGE,
                    added_background: 0xe3f0da,
                    removed_background: 0xfbe3dc,
                    hunk: a::CYAN,
                    line_number: a::COMMENT,
                }
            }
            Self::KanagawaWave => {
                use sources::kanagawa::wave as k;
                Palette {
                    canvas: k::SUMI_INK_3,
                    panel: k::SUMI_INK_4,
                    subtle: k::SUMI_INK_2,
                    // Tuned: sumiInk5 darkened below the selected row's lightness, so a hovered
                    // row never reads heavier than the selected one.
                    hover: 0x323242,
                    // Tuned: waveBlue1 lightened to stay 1.15:1 apart from the panel.
                    selected: 0x24364e,
                    border: k::SUMI_INK_6,
                    text: k::FUJI_WHITE,
                    muted: k::OLD_WHITE,
                    accent: k::CRYSTAL_BLUE,
                    accent_foreground: k::SUMI_INK_3,
                    accent_hover: 0x94aee0,
                    accent_active: 0x6d8dce,
                    // Tuned: autumnGreen lightened for 4.5:1 in its diff tile.
                    added: 0x89a47e,
                    removed: k::PEACH_RED,
                    modified: k::AUTUMN_YELLOW,
                    renamed: k::ONI_VIOLET,
                    warning: k::RONIN_YELLOW,
                    added_background: k::WINTER_GREEN,
                    removed_background: k::WINTER_RED,
                    hunk: k::SPRING_BLUE,
                    // Tuned: sumiInk6 lightened for 4.5:1 in the gutter.
                    line_number: 0x9494ad,
                }
            }
            Self::KanagawaLotus => {
                use sources::kanagawa::lotus as k;
                Palette {
                    canvas: k::LOTUS_WHITE_3,
                    panel: 0xf7f3d1,
                    subtle: k::LOTUS_WHITE_2,
                    hover: k::LOTUS_WHITE_1,
                    // Tuned: lotusBlue1 stepped darker until hover reads 0.005 lighter in OKLab
                    // on the canvas, panel and subtle, so a hovered row never reads heavier than
                    // the selected one.
                    selected: 0xc4d4dd,
                    border: k::LOTUS_WHITE_0,
                    // Tuned: lotusInk1 darkened to stay above secondary text, which keeps its
                    // margin on the darker hovered selected row.
                    text: 0x4f4f5f,
                    // Tuned: lotusGray2 darkened for 4.5:1 on every row surface and diff tile,
                    // with the margin on the hovered selected row.
                    muted: 0x545147,
                    accent: k::LOTUS_BLUE_4,
                    accent_foreground: k::LOTUS_WHITE_3,
                    accent_hover: 0x435c89,
                    accent_active: 0x3a5077,
                    // Tuned: lotusGreen2, lotusRed2, lotusYellow3 and lotusOrange2 darkened for
                    // 3:1 on row surfaces, 4.5:1 in diff tiles, the canvas label on fills and
                    // warning text on subtle surfaces.
                    added: 0x49613e,
                    removed: 0xa72428,
                    modified: 0x916100,
                    renamed: k::LOTUS_VIOLET_4,
                    warning: 0x8b4c00,
                    // Tuned: lotusGreen3 blended halfway to the canvas and lotusRed4 a little
                    // past halfway, so text keeps 4.5:1 inside diff tiles.
                    added_background: 0xd4deb5,
                    removed_background: 0xeed0b0,
                    // Tuned: lotusBlue4 darkened for 4.5:1 on subtle surfaces.
                    hunk: 0x425c8b,
                    // Tuned: lotusGray2 darkened for 4.5:1 on the canvas.
                    line_number: 0x676458,
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    Compact,
    #[default]
    #[serde(other)]
    Comfortable,
}

impl Density {
    pub const ALL: [Self; 2] = [Self::Comfortable, Self::Compact];

    pub fn label(self) -> &'static str {
        match self {
            Self::Comfortable => "Comfortable",
            Self::Compact => "Compact",
        }
    }

    pub fn history_row_height(self) -> f32 {
        self.history_row_height_at_scale(ui_scale())
    }
    pub fn history_row_height_at_scale(self, scale: f32) -> f32 {
        scale
            * match self {
                Self::Comfortable => 34.,
                Self::Compact => 28.,
            }
    }

    pub fn file_row_height(self) -> f32 {
        self.file_row_height_at_scale(ui_scale())
    }
    pub fn file_row_height_at_scale(self, scale: f32) -> f32 {
        scale
            * match self {
                Self::Comfortable => 44.,
                Self::Compact => 34.,
            }
    }
}
