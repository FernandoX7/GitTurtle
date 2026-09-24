//! Upstream palette values for the built-in themes adapted from Solarized, One, Rosé Pine,
//! Dracula and Kanagawa.
//!
//! Every value is copied from `docs/development/2026-09-17-theme-palettes.md` ("Source values for
//! the chosen palettes"), which records it from the upstream file at the pinned revision named in
//! each family module. Constants keep the upstream name so a reviewer can diff them against the
//! note and the source. Semantic mapping to GitTurtle's palette tokens, and any tuning needed to
//! reach the readability thresholds, belongs to the themes that use these values, not here.
//! License notices are preserved under `docs/licenses/assets/` and named in
//! `THIRD_PARTY_NOTICES.md`.

/// Solarized by Ethan Schoonover (MIT).
///
/// Upstream: <https://github.com/altercation/solarized>, file `README.md`, revision
/// `62f656a02f93c5190a8753159e34b385588d5ff3`
/// (<https://github.com/altercation/solarized/blob/62f656a02f93c5190a8753159e34b385588d5ff3/README.md>).
/// The sixteen-value table (`base03`…`base3` and eight accents) is shared by both variants; the
/// `dark` and `light` submodules record the upstream pairing rule. The reduced-lightness terminal
/// variants are not used.
pub mod solarized {
    /// `base03`
    pub const BASE03: u32 = 0x002b36;
    /// `base02`
    pub const BASE02: u32 = 0x073642;
    /// `base01`
    pub const BASE01: u32 = 0x586e75;
    /// `base00`
    pub const BASE00: u32 = 0x657b83;
    /// `base0`
    pub const BASE0: u32 = 0x839496;
    /// `base1`
    pub const BASE1: u32 = 0x93a1a1;
    /// `base2`
    pub const BASE2: u32 = 0xeee8d5;
    /// `base3`
    pub const BASE3: u32 = 0xfdf6e3;
    /// `yellow`
    pub const YELLOW: u32 = 0xb58900;
    /// `orange`
    pub const ORANGE: u32 = 0xcb4b16;
    /// `red`
    pub const RED: u32 = 0xdc322f;
    /// `magenta`
    pub const MAGENTA: u32 = 0xd33682;
    /// `violet`
    pub const VIOLET: u32 = 0x6c71c4;
    /// `blue`
    pub const BLUE: u32 = 0x268bd2;
    /// `cyan`
    pub const CYAN: u32 = 0x2aa198;
    /// `green`
    pub const GREEN: u32 = 0x859900;

    /// Solarized Dark: `base03`/`base02` surfaces with `base0` text and `base1` secondary text.
    pub mod dark {
        /// Canvas: `base03`.
        pub const CANVAS: u32 = super::BASE03;
        /// Panel: `base02`.
        pub const PANEL: u32 = super::BASE02;
        /// Text: `base0`.
        pub const TEXT: u32 = super::BASE0;
        /// Secondary text: `base1`.
        pub const MUTED: u32 = super::BASE1;
    }

    /// Solarized Light: `base3`/`base2` surfaces with `base00` text and `base01` secondary text.
    pub mod light {
        /// Canvas: `base3`.
        pub const CANVAS: u32 = super::BASE3;
        /// Panel: `base2`.
        pub const PANEL: u32 = super::BASE2;
        /// Text: `base00`.
        pub const TEXT: u32 = super::BASE00;
        /// Secondary text: `base01`.
        pub const MUTED: u32 = super::BASE01;
    }
}

/// One, from Atom's One Dark and One Light syntax themes (MIT, © 2016 GitHub Inc.).
///
/// Upstream defines the colors in HSL; the hex values were computed from those definitions
/// (`colorsys`, rounded) and recorded in the note. Both repositories were archived in 2018 and
/// their license texts are byte-identical, so one copy is preserved.
///
/// - Dark: <https://github.com/atom/one-dark-syntax>, file `styles/colors.less`, revision
///   `9c96f4454362267ac45322063e193ccf9d2debb1`
///   (<https://github.com/atom/one-dark-syntax/blob/9c96f4454362267ac45322063e193ccf9d2debb1/styles/colors.less>).
/// - Light: <https://github.com/atom/one-light-syntax>, file `styles/colors.less`, revision
///   `d84579027410c576086dfca14d934c4bd74b0438`
///   (<https://github.com/atom/one-light-syntax/blob/d84579027410c576086dfca14d934c4bd74b0438/styles/colors.less>).
pub mod one {
    /// One Dark (`atom/one-dark-syntax` at `9c96f44`).
    pub mod dark {
        /// `bg`
        pub const BG: u32 = 0x282c34;
        /// `mono-1`
        pub const MONO_1: u32 = 0xabb2bf;
        /// `mono-2`
        pub const MONO_2: u32 = 0x828997;
        /// `mono-3`
        pub const MONO_3: u32 = 0x5c6370;
        /// `cyan`
        pub const CYAN: u32 = 0x56b6c2;
        /// `blue`
        pub const BLUE: u32 = 0x61afef;
        /// `purple`
        pub const PURPLE: u32 = 0xc678dd;
        /// `green`
        pub const GREEN: u32 = 0x98c379;
        /// `red-1`
        pub const RED_1: u32 = 0xe06c75;
        /// `red-2`
        pub const RED_2: u32 = 0xbe5046;
        /// `orange-1`
        pub const ORANGE_1: u32 = 0xd19a66;
        /// `orange-2`
        pub const ORANGE_2: u32 = 0xe5c07b;
        /// `accent`
        pub const ACCENT: u32 = 0x528bff;
    }

    /// One Light (`atom/one-light-syntax` at `d845790`).
    pub mod light {
        /// `bg`
        pub const BG: u32 = 0xfafafa;
        /// `mono-1`
        pub const MONO_1: u32 = 0x383a42;
        /// `mono-2`
        pub const MONO_2: u32 = 0x696c77;
        /// `mono-3`
        pub const MONO_3: u32 = 0xa0a1a7;
        /// `cyan`
        pub const CYAN: u32 = 0x0184bc;
        /// `blue`
        pub const BLUE: u32 = 0x4078f2;
        /// `purple`
        pub const PURPLE: u32 = 0xa626a4;
        /// `green`
        pub const GREEN: u32 = 0x50a14f;
        /// `red-1`
        pub const RED_1: u32 = 0xe45649;
        /// `red-2`
        pub const RED_2: u32 = 0xca1243;
        /// `orange-1`
        pub const ORANGE_1: u32 = 0x986801;
        /// `orange-2`
        pub const ORANGE_2: u32 = 0xc18401;
        /// `accent`
        pub const ACCENT: u32 = 0x526fff;
    }
}

/// Rosé Pine by mvllow (MIT).
///
/// Upstream: <https://github.com/rose-pine/palette>, file `palette.json`, revision
/// `92af52b465ab6e47437aca223c9b8d3009a2023b`
/// (<https://github.com/rose-pine/palette/blob/92af52b465ab6e47437aca223c9b8d3009a2023b/palette.json>),
/// SHA-256 `8b71546357c65ee23715e77f144d914e5f415f44f3ef3b0179bbe1f17d69211d`. The Moon variant
/// and the `highlight` surfaces are not used.
pub mod rose_pine {
    /// Rosé Pine Main, the dark variant.
    pub mod main {
        /// `base`
        pub const BASE: u32 = 0x191724;
        /// `surface`
        pub const SURFACE: u32 = 0x1f1d2e;
        /// `overlay`
        pub const OVERLAY: u32 = 0x26233a;
        /// `muted`
        pub const MUTED: u32 = 0x6e6a86;
        /// `subtle`
        pub const SUBTLE: u32 = 0x908caa;
        /// `text`
        pub const TEXT: u32 = 0xe0def4;
        /// `love`
        pub const LOVE: u32 = 0xeb6f92;
        /// `gold`
        pub const GOLD: u32 = 0xf6c177;
        /// `rose`
        pub const ROSE: u32 = 0xebbcba;
        /// `pine`
        pub const PINE: u32 = 0x31748f;
        /// `foam`
        pub const FOAM: u32 = 0x9ccfd8;
        /// `iris`
        pub const IRIS: u32 = 0xc4a7e7;
    }

    /// Rosé Pine Dawn, the light variant.
    pub mod dawn {
        /// `base`
        pub const BASE: u32 = 0xfaf4ed;
        /// `surface`
        pub const SURFACE: u32 = 0xfffaf3;
        /// `overlay`
        pub const OVERLAY: u32 = 0xf2e9e1;
        /// `muted`
        pub const MUTED: u32 = 0x9893a5;
        /// `subtle`
        pub const SUBTLE: u32 = 0x797593;
        /// `text`
        pub const TEXT: u32 = 0x464261;
        /// `love`
        pub const LOVE: u32 = 0xb4637a;
        /// `gold`
        pub const GOLD: u32 = 0xea9d34;
        /// `rose`
        pub const ROSE: u32 = 0xd7827e;
        /// `pine`
        pub const PINE: u32 = 0x286983;
        /// `foam`
        pub const FOAM: u32 = 0x56949f;
        /// `iris`
        pub const IRIS: u32 = 0x907aa9;
    }
}

/// Dracula with its official Alucard light variant (MIT, © 2023 Dracula Theme).
///
/// Upstream: <https://github.com/dracula/dracula-theme>, file `README.md`, revision
/// `5962daae54e4608d281cb6f4eee2349e605d9e3c`
/// (<https://github.com/dracula/dracula-theme/blob/5962daae54e4608d281cb6f4eee2349e605d9e3c/README.md>):
/// the Color Palette table and the Alucard table in the same file.
pub mod dracula {
    /// Dracula, the dark variant. Upstream gives current line and selection one value.
    pub mod dark {
        /// `background`
        pub const BACKGROUND: u32 = 0x282a36;
        /// `current line and selection`
        pub const CURRENT_LINE: u32 = 0x44475a;
        /// `current line and selection`
        pub const SELECTION: u32 = 0x44475a;
        /// `foreground`
        pub const FOREGROUND: u32 = 0xf8f8f2;
        /// `comment`
        pub const COMMENT: u32 = 0x6272a4;
        /// `cyan`
        pub const CYAN: u32 = 0x8be9fd;
        /// `green`
        pub const GREEN: u32 = 0x50fa7b;
        /// `orange`
        pub const ORANGE: u32 = 0xffb86c;
        /// `pink`
        pub const PINK: u32 = 0xff79c6;
        /// `purple`
        pub const PURPLE: u32 = 0xbd93f9;
        /// `red`
        pub const RED: u32 = 0xff5555;
        /// `yellow`
        pub const YELLOW: u32 = 0xf1fa8c;
    }

    /// Alucard, the light variant.
    pub mod alucard {
        /// `background`
        pub const BACKGROUND: u32 = 0xfffbeb;
        /// `current line`
        pub const CURRENT_LINE: u32 = 0x6c664b;
        /// `selection`
        pub const SELECTION: u32 = 0xcfcfde;
        /// `foreground`
        pub const FOREGROUND: u32 = 0x1f1f1f;
        /// `comment`
        pub const COMMENT: u32 = 0x6c664b;
        /// `cyan`
        pub const CYAN: u32 = 0x036a96;
        /// `green`
        pub const GREEN: u32 = 0x14710a;
        /// `orange`
        pub const ORANGE: u32 = 0xa34d14;
        /// `pink`
        pub const PINK: u32 = 0xa3144d;
        /// `purple`
        pub const PURPLE: u32 = 0x644ac9;
        /// `red`
        pub const RED: u32 = 0xcb3a2a;
        /// `yellow`
        pub const YELLOW: u32 = 0x846e15;
    }
}

/// Kanagawa by Tommaso Laurenzi (MIT).
///
/// Upstream: <https://github.com/rebelot/kanagawa.nvim>, file `lua/kanagawa/colors.lua`, revision
/// `bb85e4bfc8d89b0e62c8fa53ccdd13d12e2f77b3`
/// (<https://github.com/rebelot/kanagawa.nvim/blob/bb85e4bfc8d89b0e62c8fa53ccdd13d12e2f77b3/lua/kanagawa/colors.lua>).
/// Upstream names become `SCREAMING_SNAKE_CASE` (`sumiInk0` is `SUMI_INK_0`). The Dragon variant
/// is not used.
pub mod kanagawa {
    /// Kanagawa Wave, the dark variant.
    pub mod wave {
        /// `sumiInk0`
        pub const SUMI_INK_0: u32 = 0x16161d;
        /// `sumiInk1`
        pub const SUMI_INK_1: u32 = 0x181820;
        /// `sumiInk2`
        pub const SUMI_INK_2: u32 = 0x1a1a22;
        /// `sumiInk3`
        pub const SUMI_INK_3: u32 = 0x1f1f28;
        /// `sumiInk4`
        pub const SUMI_INK_4: u32 = 0x2a2a37;
        /// `sumiInk5`
        pub const SUMI_INK_5: u32 = 0x363646;
        /// `sumiInk6`
        pub const SUMI_INK_6: u32 = 0x54546d;
        /// `waveBlue1`
        pub const WAVE_BLUE_1: u32 = 0x223249;
        /// `waveBlue2`
        pub const WAVE_BLUE_2: u32 = 0x2d4f67;
        /// `winterGreen`
        pub const WINTER_GREEN: u32 = 0x2b3328;
        /// `winterYellow`
        pub const WINTER_YELLOW: u32 = 0x49443c;
        /// `winterRed`
        pub const WINTER_RED: u32 = 0x43242b;
        /// `winterBlue`
        pub const WINTER_BLUE: u32 = 0x252535;
        /// `autumnGreen`
        pub const AUTUMN_GREEN: u32 = 0x76946a;
        /// `autumnRed`
        pub const AUTUMN_RED: u32 = 0xc34043;
        /// `autumnYellow`
        pub const AUTUMN_YELLOW: u32 = 0xdca561;
        /// `samuraiRed`
        pub const SAMURAI_RED: u32 = 0xe82424;
        /// `roninYellow`
        pub const RONIN_YELLOW: u32 = 0xff9e3b;
        /// `waveAqua1`
        pub const WAVE_AQUA_1: u32 = 0x6a9589;
        /// `dragonBlue`
        pub const DRAGON_BLUE: u32 = 0x658594;
        /// `oldWhite`
        pub const OLD_WHITE: u32 = 0xc8c093;
        /// `fujiWhite`
        pub const FUJI_WHITE: u32 = 0xdcd7ba;
        /// `fujiGray`
        pub const FUJI_GRAY: u32 = 0x727169;
        /// `oniViolet`
        pub const ONI_VIOLET: u32 = 0x957fb8;
        /// `crystalBlue`
        pub const CRYSTAL_BLUE: u32 = 0x7e9cd8;
        /// `springViolet1`
        pub const SPRING_VIOLET_1: u32 = 0x938aa9;
        /// `springViolet2`
        pub const SPRING_VIOLET_2: u32 = 0x9cabca;
        /// `springBlue`
        pub const SPRING_BLUE: u32 = 0x7fb4ca;
        /// `waveAqua2`
        pub const WAVE_AQUA_2: u32 = 0x7aa89f;
        /// `springGreen`
        pub const SPRING_GREEN: u32 = 0x98bb6c;
        /// `boatYellow2`
        pub const BOAT_YELLOW_2: u32 = 0xc0a36e;
        /// `carpYellow`
        pub const CARP_YELLOW: u32 = 0xe6c384;
        /// `sakuraPink`
        pub const SAKURA_PINK: u32 = 0xd27e99;
        /// `waveRed`
        pub const WAVE_RED: u32 = 0xe46876;
        /// `peachRed`
        pub const PEACH_RED: u32 = 0xff5d62;
        /// `surimiOrange`
        pub const SURIMI_ORANGE: u32 = 0xffa066;
        /// `katanaGray`
        pub const KATANA_GRAY: u32 = 0x717c7c;
    }

    /// Kanagawa Lotus, the light variant.
    pub mod lotus {
        /// `lotusInk1`
        pub const LOTUS_INK_1: u32 = 0x545464;
        /// `lotusInk2`
        pub const LOTUS_INK_2: u32 = 0x43436c;
        /// `lotusGray`
        pub const LOTUS_GRAY: u32 = 0xdcd7ba;
        /// `lotusGray2`
        pub const LOTUS_GRAY_2: u32 = 0x716e61;
        /// `lotusGray3`
        pub const LOTUS_GRAY_3: u32 = 0x8a8980;
        /// `lotusWhite0`
        pub const LOTUS_WHITE_0: u32 = 0xd5cea3;
        /// `lotusWhite1`
        pub const LOTUS_WHITE_1: u32 = 0xdcd5ac;
        /// `lotusWhite2`
        pub const LOTUS_WHITE_2: u32 = 0xe5ddb0;
        /// `lotusWhite3`
        pub const LOTUS_WHITE_3: u32 = 0xf2ecbc;
        /// `lotusWhite4`
        pub const LOTUS_WHITE_4: u32 = 0xe7dba0;
        /// `lotusWhite5`
        pub const LOTUS_WHITE_5: u32 = 0xe4d794;
        /// `lotusViolet1`
        pub const LOTUS_VIOLET_1: u32 = 0xa09cac;
        /// `lotusViolet2`
        pub const LOTUS_VIOLET_2: u32 = 0x766b90;
        /// `lotusViolet3`
        pub const LOTUS_VIOLET_3: u32 = 0xc9cbd1;
        /// `lotusViolet4`
        pub const LOTUS_VIOLET_4: u32 = 0x624c83;
        /// `lotusBlue1`
        pub const LOTUS_BLUE_1: u32 = 0xc7d7e0;
        /// `lotusBlue2`
        pub const LOTUS_BLUE_2: u32 = 0xb5cbd2;
        /// `lotusBlue3`
        pub const LOTUS_BLUE_3: u32 = 0x9fb5c9;
        /// `lotusBlue4`
        pub const LOTUS_BLUE_4: u32 = 0x4d699b;
        /// `lotusBlue5`
        pub const LOTUS_BLUE_5: u32 = 0x5d57a3;
        /// `lotusGreen`
        pub const LOTUS_GREEN: u32 = 0x6f894e;
        /// `lotusGreen2`
        pub const LOTUS_GREEN_2: u32 = 0x6e915f;
        /// `lotusGreen3`
        pub const LOTUS_GREEN_3: u32 = 0xb7d0ae;
        /// `lotusPink`
        pub const LOTUS_PINK: u32 = 0xb35b79;
        /// `lotusOrange`
        pub const LOTUS_ORANGE: u32 = 0xcc6d00;
        /// `lotusOrange2`
        pub const LOTUS_ORANGE_2: u32 = 0xe98a00;
        /// `lotusYellow`
        pub const LOTUS_YELLOW: u32 = 0x77713f;
        /// `lotusYellow2`
        pub const LOTUS_YELLOW_2: u32 = 0x836f4a;
        /// `lotusYellow3`
        pub const LOTUS_YELLOW_3: u32 = 0xde9800;
        /// `lotusYellow4`
        pub const LOTUS_YELLOW_4: u32 = 0xf9d791;
        /// `lotusRed`
        pub const LOTUS_RED: u32 = 0xc84053;
        /// `lotusRed2`
        pub const LOTUS_RED_2: u32 = 0xd7474b;
        /// `lotusRed3`
        pub const LOTUS_RED_3: u32 = 0xe82424;
        /// `lotusRed4`
        pub const LOTUS_RED_4: u32 = 0xd9a594;
        /// `lotusAqua`
        pub const LOTUS_AQUA: u32 = 0x597b75;
        /// `lotusAqua2`
        pub const LOTUS_AQUA_2: u32 = 0x5e857a;
        /// `lotusTeal1`
        pub const LOTUS_TEAL_1: u32 = 0x4e8ca2;
        /// `lotusTeal2`
        pub const LOTUS_TEAL_2: u32 = 0x6693bf;
        /// `lotusTeal3`
        pub const LOTUS_TEAL_3: u32 = 0x5a7785;
        /// `lotusCyan`
        pub const LOTUS_CYAN: u32 = 0xd7e3d8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTE: &str = include_str!("../../../../docs/development/2026-09-17-theme-palettes.md");

    /// Every constant, grouped by the note paragraph that records it, with its upstream name.
    const SOURCES: &[(&str, &[(&str, u32)])] = &[
        (
            "Solarized",
            &[
                ("base03", solarized::BASE03),
                ("base02", solarized::BASE02),
                ("base01", solarized::BASE01),
                ("base00", solarized::BASE00),
                ("base0", solarized::BASE0),
                ("base1", solarized::BASE1),
                ("base2", solarized::BASE2),
                ("base3", solarized::BASE3),
                ("yellow", solarized::YELLOW),
                ("orange", solarized::ORANGE),
                ("red", solarized::RED),
                ("magenta", solarized::MAGENTA),
                ("violet", solarized::VIOLET),
                ("blue", solarized::BLUE),
                ("cyan", solarized::CYAN),
                ("green", solarized::GREEN),
            ],
        ),
        (
            "One Dark",
            &[
                ("bg", one::dark::BG),
                ("mono-1", one::dark::MONO_1),
                ("mono-2", one::dark::MONO_2),
                ("mono-3", one::dark::MONO_3),
                ("cyan", one::dark::CYAN),
                ("blue", one::dark::BLUE),
                ("purple", one::dark::PURPLE),
                ("green", one::dark::GREEN),
                ("red-1", one::dark::RED_1),
                ("red-2", one::dark::RED_2),
                ("orange-1", one::dark::ORANGE_1),
                ("orange-2", one::dark::ORANGE_2),
                ("accent", one::dark::ACCENT),
            ],
        ),
        (
            "One Light",
            &[
                ("bg", one::light::BG),
                ("mono-1", one::light::MONO_1),
                ("mono-2", one::light::MONO_2),
                ("mono-3", one::light::MONO_3),
                ("cyan", one::light::CYAN),
                ("blue", one::light::BLUE),
                ("purple", one::light::PURPLE),
                ("green", one::light::GREEN),
                ("red-1", one::light::RED_1),
                ("red-2", one::light::RED_2),
                ("orange-1", one::light::ORANGE_1),
                ("orange-2", one::light::ORANGE_2),
                ("accent", one::light::ACCENT),
            ],
        ),
        (
            "Rosé Pine Main",
            &[
                ("base", rose_pine::main::BASE),
                ("surface", rose_pine::main::SURFACE),
                ("overlay", rose_pine::main::OVERLAY),
                ("muted", rose_pine::main::MUTED),
                ("subtle", rose_pine::main::SUBTLE),
                ("text", rose_pine::main::TEXT),
                ("love", rose_pine::main::LOVE),
                ("gold", rose_pine::main::GOLD),
                ("rose", rose_pine::main::ROSE),
                ("pine", rose_pine::main::PINE),
                ("foam", rose_pine::main::FOAM),
                ("iris", rose_pine::main::IRIS),
            ],
        ),
        (
            "Rosé Pine Dawn",
            &[
                ("base", rose_pine::dawn::BASE),
                ("surface", rose_pine::dawn::SURFACE),
                ("overlay", rose_pine::dawn::OVERLAY),
                ("muted", rose_pine::dawn::MUTED),
                ("subtle", rose_pine::dawn::SUBTLE),
                ("text", rose_pine::dawn::TEXT),
                ("love", rose_pine::dawn::LOVE),
                ("gold", rose_pine::dawn::GOLD),
                ("rose", rose_pine::dawn::ROSE),
                ("pine", rose_pine::dawn::PINE),
                ("foam", rose_pine::dawn::FOAM),
                ("iris", rose_pine::dawn::IRIS),
            ],
        ),
        (
            "Dracula",
            &[
                ("background", dracula::dark::BACKGROUND),
                ("current line and selection", dracula::dark::CURRENT_LINE),
                ("current line and selection", dracula::dark::SELECTION),
                ("foreground", dracula::dark::FOREGROUND),
                ("comment", dracula::dark::COMMENT),
                ("cyan", dracula::dark::CYAN),
                ("green", dracula::dark::GREEN),
                ("orange", dracula::dark::ORANGE),
                ("pink", dracula::dark::PINK),
                ("purple", dracula::dark::PURPLE),
                ("red", dracula::dark::RED),
                ("yellow", dracula::dark::YELLOW),
            ],
        ),
        (
            "Alucard",
            &[
                ("background", dracula::alucard::BACKGROUND),
                ("current line", dracula::alucard::CURRENT_LINE),
                ("selection", dracula::alucard::SELECTION),
                ("foreground", dracula::alucard::FOREGROUND),
                ("comment", dracula::alucard::COMMENT),
                ("cyan", dracula::alucard::CYAN),
                ("green", dracula::alucard::GREEN),
                ("orange", dracula::alucard::ORANGE),
                ("pink", dracula::alucard::PINK),
                ("purple", dracula::alucard::PURPLE),
                ("red", dracula::alucard::RED),
                ("yellow", dracula::alucard::YELLOW),
            ],
        ),
        (
            "Kanagawa Wave",
            &[
                ("sumiInk0", kanagawa::wave::SUMI_INK_0),
                ("sumiInk1", kanagawa::wave::SUMI_INK_1),
                ("sumiInk2", kanagawa::wave::SUMI_INK_2),
                ("sumiInk3", kanagawa::wave::SUMI_INK_3),
                ("sumiInk4", kanagawa::wave::SUMI_INK_4),
                ("sumiInk5", kanagawa::wave::SUMI_INK_5),
                ("sumiInk6", kanagawa::wave::SUMI_INK_6),
                ("waveBlue1", kanagawa::wave::WAVE_BLUE_1),
                ("waveBlue2", kanagawa::wave::WAVE_BLUE_2),
                ("winterGreen", kanagawa::wave::WINTER_GREEN),
                ("winterYellow", kanagawa::wave::WINTER_YELLOW),
                ("winterRed", kanagawa::wave::WINTER_RED),
                ("winterBlue", kanagawa::wave::WINTER_BLUE),
                ("autumnGreen", kanagawa::wave::AUTUMN_GREEN),
                ("autumnRed", kanagawa::wave::AUTUMN_RED),
                ("autumnYellow", kanagawa::wave::AUTUMN_YELLOW),
                ("samuraiRed", kanagawa::wave::SAMURAI_RED),
                ("roninYellow", kanagawa::wave::RONIN_YELLOW),
                ("waveAqua1", kanagawa::wave::WAVE_AQUA_1),
                ("dragonBlue", kanagawa::wave::DRAGON_BLUE),
                ("oldWhite", kanagawa::wave::OLD_WHITE),
                ("fujiWhite", kanagawa::wave::FUJI_WHITE),
                ("fujiGray", kanagawa::wave::FUJI_GRAY),
                ("oniViolet", kanagawa::wave::ONI_VIOLET),
                ("crystalBlue", kanagawa::wave::CRYSTAL_BLUE),
                ("springViolet1", kanagawa::wave::SPRING_VIOLET_1),
                ("springViolet2", kanagawa::wave::SPRING_VIOLET_2),
                ("springBlue", kanagawa::wave::SPRING_BLUE),
                ("waveAqua2", kanagawa::wave::WAVE_AQUA_2),
                ("springGreen", kanagawa::wave::SPRING_GREEN),
                ("boatYellow2", kanagawa::wave::BOAT_YELLOW_2),
                ("carpYellow", kanagawa::wave::CARP_YELLOW),
                ("sakuraPink", kanagawa::wave::SAKURA_PINK),
                ("waveRed", kanagawa::wave::WAVE_RED),
                ("peachRed", kanagawa::wave::PEACH_RED),
                ("surimiOrange", kanagawa::wave::SURIMI_ORANGE),
                ("katanaGray", kanagawa::wave::KATANA_GRAY),
            ],
        ),
        (
            "Kanagawa Lotus",
            &[
                ("lotusInk1", kanagawa::lotus::LOTUS_INK_1),
                ("lotusInk2", kanagawa::lotus::LOTUS_INK_2),
                ("lotusGray", kanagawa::lotus::LOTUS_GRAY),
                ("lotusGray2", kanagawa::lotus::LOTUS_GRAY_2),
                ("lotusGray3", kanagawa::lotus::LOTUS_GRAY_3),
                ("lotusWhite0", kanagawa::lotus::LOTUS_WHITE_0),
                ("lotusWhite1", kanagawa::lotus::LOTUS_WHITE_1),
                ("lotusWhite2", kanagawa::lotus::LOTUS_WHITE_2),
                ("lotusWhite3", kanagawa::lotus::LOTUS_WHITE_3),
                ("lotusWhite4", kanagawa::lotus::LOTUS_WHITE_4),
                ("lotusWhite5", kanagawa::lotus::LOTUS_WHITE_5),
                ("lotusViolet1", kanagawa::lotus::LOTUS_VIOLET_1),
                ("lotusViolet2", kanagawa::lotus::LOTUS_VIOLET_2),
                ("lotusViolet3", kanagawa::lotus::LOTUS_VIOLET_3),
                ("lotusViolet4", kanagawa::lotus::LOTUS_VIOLET_4),
                ("lotusBlue1", kanagawa::lotus::LOTUS_BLUE_1),
                ("lotusBlue2", kanagawa::lotus::LOTUS_BLUE_2),
                ("lotusBlue3", kanagawa::lotus::LOTUS_BLUE_3),
                ("lotusBlue4", kanagawa::lotus::LOTUS_BLUE_4),
                ("lotusBlue5", kanagawa::lotus::LOTUS_BLUE_5),
                ("lotusGreen", kanagawa::lotus::LOTUS_GREEN),
                ("lotusGreen2", kanagawa::lotus::LOTUS_GREEN_2),
                ("lotusGreen3", kanagawa::lotus::LOTUS_GREEN_3),
                ("lotusPink", kanagawa::lotus::LOTUS_PINK),
                ("lotusOrange", kanagawa::lotus::LOTUS_ORANGE),
                ("lotusOrange2", kanagawa::lotus::LOTUS_ORANGE_2),
                ("lotusYellow", kanagawa::lotus::LOTUS_YELLOW),
                ("lotusYellow2", kanagawa::lotus::LOTUS_YELLOW_2),
                ("lotusYellow3", kanagawa::lotus::LOTUS_YELLOW_3),
                ("lotusYellow4", kanagawa::lotus::LOTUS_YELLOW_4),
                ("lotusRed", kanagawa::lotus::LOTUS_RED),
                ("lotusRed2", kanagawa::lotus::LOTUS_RED_2),
                ("lotusRed3", kanagawa::lotus::LOTUS_RED_3),
                ("lotusRed4", kanagawa::lotus::LOTUS_RED_4),
                ("lotusAqua", kanagawa::lotus::LOTUS_AQUA),
                ("lotusAqua2", kanagawa::lotus::LOTUS_AQUA_2),
                ("lotusTeal1", kanagawa::lotus::LOTUS_TEAL_1),
                ("lotusTeal2", kanagawa::lotus::LOTUS_TEAL_2),
                ("lotusTeal3", kanagawa::lotus::LOTUS_TEAL_3),
                ("lotusCyan", kanagawa::lotus::LOTUS_CYAN),
            ],
        ),
    ];

    fn source_paragraph(title: &str) -> String {
        let section = NOTE
            .split_once("### Source values for the chosen palettes")
            .expect("the note keeps its source-values section")
            .1;
        let start = section
            .find(&format!("**{title}**"))
            .unwrap_or_else(|| panic!("the note records {title}"));
        let paragraph = &section[start..];
        paragraph[..paragraph.find("\n\n").unwrap_or(paragraph.len())].to_lowercase()
    }

    #[test]
    fn every_source_constant_matches_the_palette_note() {
        for (title, values) in SOURCES {
            let paragraph = source_paragraph(title);
            let mut names: Vec<&str> = values.iter().map(|(name, _)| *name).collect();
            names.dedup();
            assert_eq!(
                paragraph.matches("`#").count(),
                names.len(),
                "{title}: the note and the constants list different values"
            );
            for (name, value) in *values {
                assert!(*value <= 0xff_ff_ff, "{title} {name} is not a 24-bit color");
                let recorded = format!("{} `#{value:06x}`", name.to_lowercase());
                assert!(
                    paragraph.contains(&recorded),
                    "{title}: {name} is not recorded as #{value:06x}"
                );
            }
        }
    }

    #[test]
    fn solarized_variants_follow_the_upstream_pairing() {
        use solarized::{dark, light};
        assert_eq!(
            [dark::CANVAS, dark::PANEL, dark::TEXT, dark::MUTED],
            [
                solarized::BASE03,
                solarized::BASE02,
                solarized::BASE0,
                solarized::BASE1
            ]
        );
        assert_eq!(
            [light::CANVAS, light::PANEL, light::TEXT, light::MUTED],
            [
                solarized::BASE3,
                solarized::BASE2,
                solarized::BASE00,
                solarized::BASE01
            ]
        );
    }
}
