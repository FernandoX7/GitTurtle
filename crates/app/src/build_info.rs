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

pub fn diagnostics(
    scale: f32,
    theme: crate::appearance::ThemeChoice,
    density: crate::appearance::Density,
    interface_size: u8,
    code_size: u8,
) -> String {
    let mut report = identity();
    report["display"] = json!({
        "backend": session_backend(),
        "scale_factor": scale,
        "theme": theme,
        "density": density,
        "interface_text_size": interface_size,
        "code_text_size": code_size,
    });
    pretty(&report)
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).expect("build identity contains serializable values")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_include_compiled_identity_and_only_allowlisted_display_settings() {
        let report: Value = serde_json::from_str(&diagnostics(
            1.25,
            crate::appearance::ThemeChoice::Midnight,
            crate::appearance::Density::Compact,
            13,
            15,
        ))
        .unwrap();
        assert_eq!(report["source_revision"], REVISION);
        assert_eq!(report["target"], TARGET);
        assert_eq!(report["profile"], PROFILE);
        assert_eq!(report["display"]["scale_factor"], 1.25);
        assert_eq!(report["display"].as_object().unwrap().len(), 6);
        assert_eq!(report.as_object().unwrap().len(), 9);
        assert!(!summary().contains('/'));
    }
}
