//! Compiled identity and a small, explicit allowlist of bug-report diagnostics.
//! No repository, user-directory, credential, process or network reads occur.

use serde_json::{Value, json};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REVISION: &str = env!("GITTURTLE_BUILD_REVISION");
pub const TREE: &str = env!("GITTURTLE_BUILD_TREE");
pub const TARGET: &str = env!("GITTURTLE_BUILD_TARGET");
pub const PROFILE: &str = env!("GITTURTLE_BUILD_PROFILE");

pub fn summary() -> String {
    format!(
        "GitTurtle {VERSION} ({}; {TREE}; {TARGET}; {PROFILE})",
        &REVISION[..REVISION.len().min(12)],
    )
}

pub fn identity() -> Value {
    json!({
        "application": "GitTurtle",
        "version": VERSION,
        "source_revision": REVISION,
        "source_tree": TREE,
        "target": TARGET,
        "profile": PROFILE,
        "rustc": env!("GITTURTLE_BUILD_RUSTC"),
        "build_unix_seconds": env!("GITTURTLE_BUILD_TIME"),
    })
}

/// Handles only an exact information request, before loading app state or GPUI.
/// A repository named `--version` can still be opened using its absolute path.
pub fn handle_cli() -> bool {
    let mut args = std::env::args_os().skip(1);
    let first = args.next();
    if args.next().is_some() {
        return false;
    }
    match first.as_deref().and_then(|arg| arg.to_str()) {
        Some("--version" | "-V") => println!("{}", summary()),
        Some("--build-info") => println!("{}", pretty(&identity())),
        _ => return false,
    }
    true
}

fn session_backend() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        gpui_kit::guess_compositor()
    }
    #[cfg(target_os = "macos")]
    {
        "macOS"
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        "unknown"
    }
}

/// The display settings a bug report carries beside the compiled identity. The report is
/// pasted into public issues, so it holds only enumerated choices, booleans and numbers that
/// change how the window draws: never free text, paths, names, commands, URLs, branch names
/// or the widths of panes the person sized (so not the external editor, the default branch,
/// recent repositories, profiles or a custom theme's id or name). A choice explains a
/// rendering bug; free text can identify a person.
#[derive(Clone, Copy, Debug)]
pub struct DisplaySettings {
    pub scale_factor: f32,
    /// A custom theme is passed as its base: an id alone names nothing.
    pub theme: crate::appearance::custom::ThemeSelection,
    pub follow_system: bool,
    pub density: crate::appearance::Density,
    pub interface_text_size: u8,
    pub code_text_size: u8,
    /// Settings' Use the desktop's monospace font, reported only where Settings offers it
    /// ([`REPORTS_SYSTEM_CODE_FONT`]).
    pub system_code_font: bool,
    /// Settings' Reduce motion, and whether motion is reduced now.
    pub reduce_motion: (crate::preferences::DisplayChoice, bool),
    /// Settings' Increase contrast, and whether More contrast is on now.
    pub increase_contrast: (crate::preferences::DisplayChoice, bool),
}

/// Whether the report carries `system_code_font`: only on Linux, the one platform whose
/// Settings offers Use the desktop's monospace font (fontconfig's `monospace` family).
/// Elsewhere the switch does not exist and the stored value changes nothing drawn, so
/// reporting it misled whoever read the report, as macOS reports once did.
const REPORTS_SYSTEM_CODE_FONT: bool = cfg!(target_os = "linux");

/// Settings' choice for a display preference, `system` where it follows the desktop's (or
/// macOS's) answer, with what the app does now.
fn choice((choice, effective): (crate::preferences::DisplayChoice, bool)) -> Value {
    use crate::preferences::DisplayChoice;
    let choice = match choice {
        DisplayChoice::Follow => "system",
        DisplayChoice::On => "on",
        DisplayChoice::Off => "off",
    };
    json!({ "choice": choice, "effective": effective })
}

pub fn diagnostics(display: &DisplaySettings) -> String {
    use crate::appearance::custom::ThemeSelection;
    let theme = match display.theme {
        ThemeSelection::BuiltIn(choice) => json!(choice),
        ThemeSelection::Omarchy => json!("omarchy"),
        // The caller reports a custom theme by its base; an id alone names nothing.
        ThemeSelection::Custom(_) => json!("custom"),
    };
    let mut report = identity();
    report["display"] = json!({
        "backend": session_backend(),
        "scale_factor": display.scale_factor,
        "theme": theme,
        "follow_system": display.follow_system,
        "density": display.density,
        "interface_text_size": display.interface_text_size,
        "code_text_size": display.code_text_size,
        "reduce_motion": choice(display.reduce_motion),
        "increase_contrast": choice(display.increase_contrast),
    });
    if REPORTS_SYSTEM_CODE_FONT {
        report["display"]["system_code_font"] = json!(display.system_code_font);
    }
    pretty(&report)
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).expect("build identity contains serializable values")
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::appearance::custom::ThemeSelection;
    use crate::preferences::DisplayChoice;

    /// The report is pasted into public issues: its display object holds exactly these keys,
    /// each an enumerated choice, a boolean or a number, so a new one fails here until it is
    /// added deliberately under the rule [`DisplaySettings`] states. Linux adds
    /// [`LINUX_DISPLAY_KEYS`].
    const DISPLAY_KEYS: [&str; 9] = [
        "backend",
        "scale_factor",
        "theme",
        "follow_system",
        "density",
        "interface_text_size",
        "code_text_size",
        "reduce_motion",
        "increase_contrast",
    ];
    /// The display keys only Linux reports: settings only its Settings offers.
    const LINUX_DISPLAY_KEYS: [&str; 1] = ["system_code_font"];

    /// The display object's keys on the platform the test runs on, sorted.
    fn display_keys() -> Vec<String> {
        let linux: &[&str] = if cfg!(target_os = "linux") {
            &LINUX_DISPLAY_KEYS
        } else {
            &[]
        };
        let mut keys: Vec<_> = DISPLAY_KEYS
            .iter()
            .chain(linux)
            .map(|key| key.to_string())
            .collect();
        keys.sort();
        keys
    }

    /// The report for `theme`, `reduce_motion` and `increase_contrast`, the rest fixed.
    fn report(
        theme: ThemeSelection,
        reduce_motion: (DisplayChoice, bool),
        increase_contrast: (DisplayChoice, bool),
    ) -> Value {
        serde_json::from_str(&diagnostics(&DisplaySettings {
            scale_factor: 1.25,
            theme,
            follow_system: true,
            density: crate::appearance::Density::Compact,
            interface_text_size: 13,
            code_text_size: 15,
            system_code_font: true,
            reduce_motion,
            increase_contrast,
        }))
        .unwrap()
    }

    #[test]
    fn diagnostics_include_compiled_identity_and_only_allowlisted_display_settings() {
        let midnight = ThemeSelection::BuiltIn(crate::appearance::ThemeChoice::Midnight);
        let follow = (DisplayChoice::Follow, false);
        let midnight_report = report(midnight, follow, follow);
        assert_eq!(midnight_report["display"]["theme"], "midnight");
        // The Omarchy theme is a fixed key, never the desktop theme's name.
        let omarchy = report(ThemeSelection::Omarchy, follow, follow);
        assert_eq!(omarchy["display"]["theme"], "omarchy");
        assert_eq!(midnight_report["source_revision"], REVISION);
        assert_eq!(midnight_report["target"], TARGET);
        assert_eq!(midnight_report["profile"], PROFILE);
        let display = &midnight_report["display"];
        assert_eq!(display["scale_factor"], 1.25);
        assert_eq!(display["density"], "compact");
        assert_eq!(display["interface_text_size"], 13);
        assert_eq!(display["code_text_size"], 15);
        assert_eq!(display["follow_system"], true);
        // Use the desktop's monospace font exists only in Linux's Settings: reported there,
        // and not at all on macOS, where it changes nothing drawn.
        if cfg!(target_os = "linux") {
            assert_eq!(display["system_code_font"], true);
        } else {
            assert_eq!(display.get("system_code_font"), None);
        }
        let mut keys: Vec<_> = display.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            display_keys(),
            "the display object's keys are the allowlist"
        );
        assert_eq!(keys.len(), if cfg!(target_os = "linux") { 10 } else { 9 });
        assert_eq!(midnight_report.as_object().unwrap().len(), 9);
        assert!(!summary().contains('/'));
        // Each display choice reports what Settings chose, `system` for following the
        // desktop, and what the app does now: following the desktop, either answer; On and
        // Off, theirs.
        for (stored, effective, key) in [
            (DisplayChoice::Follow, true, "system"),
            (DisplayChoice::Follow, false, "system"),
            (DisplayChoice::On, true, "on"),
            (DisplayChoice::Off, false, "off"),
        ] {
            let off = (DisplayChoice::Off, false);
            let expected = json!({ "choice": key, "effective": effective });
            let motion = report(midnight, (stored, effective), off);
            assert_eq!(motion["display"]["reduce_motion"], expected);
            assert_eq!(
                motion["display"]["increase_contrast"],
                json!({ "choice": "off", "effective": false })
            );
            let contrast = report(midnight, off, (stored, effective));
            assert_eq!(contrast["display"]["increase_contrast"], expected);
            assert_eq!(
                contrast["display"]["reduce_motion"],
                json!({ "choice": "off", "effective": false })
            );
        }
    }
}
