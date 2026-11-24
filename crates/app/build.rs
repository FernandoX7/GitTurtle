use std::{env, path::Path, process::Command, time::SystemTime};

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("--no-optional-locks")
        .args(args)
        .current_dir(root)
        .env("GIT_NO_LAZY_FETCH", "1")
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn emit(name: &str, value: &str) {
    // Build metadata contains no paths, account names or arbitrary Git text.
    println!("cargo:rustc-env={name}={value}");
}

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("Cargo manifest directory");
    let root = Path::new(&manifest).join("../..");
    // Include native dependencies/assets, not only this package's Rust files.
    // Watching .git itself would rebuild after unrelated object/lock activity.
    for path in [
        "src",
        "build.rs",
        "Cargo.toml",
        "../diff-model/src",
        "../diff-model/Cargo.toml",
        "../git-core/src",
        "../git-core/Cargo.toml",
        "../github/src",
        "../github/Cargo.toml",
        "../prefs/src",
        "../prefs/Cargo.toml",
        "../preview/src",
        "../preview/Cargo.toml",
        "../theme/src",
        "../theme/Cargo.toml",
        "../../Cargo.toml",
        "../../Cargo.lock",
        "../../assets",
        "../../vendor/accesskit_atspi_common",
        "../../vendor/accesskit_unix",
        "../../vendor/gpui-base",
        "../../vendor/gpui-component",
        "../../vendor/gpui-pre-linux",
        "../../vendor/gpui-pre-macos",
        "../../vendor/gpui-pre-macros",
        "../../vendor/mermaid-rs-renderer",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    for name in ["HEAD", "index", "packed-refs"] {
        if let Some(path) = git(&root, &["rev-parse", "--git-path", name]) {
            println!("cargo:rerun-if-changed={}", root.join(path).display());
        }
    }
    if let Some(reference) = git(&root, &["symbolic-ref", "-q", "HEAD"])
        && let Some(path) = git(&root, &["rev-parse", "--git-path", &reference])
    {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }
    let revision = git(&root, &["rev-parse", "--verify", "HEAD"])
        .filter(|value| value.len() >= 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .unwrap_or_else(|| "unknown".into());
    let tree = match git(
        &root,
        &["status", "--porcelain", "--untracked-files=normal"],
    ) {
        Some(status) if status.is_empty() => "clean",
        Some(_) => "modified",
        None => "unknown",
    };
    let rustc = Command::new(env::var_os("RUSTC").expect("Cargo Rust compiler"))
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    let built_at = env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        });
    let profile = env::var("PROFILE").expect("Cargo profile");
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if profile == "release" && matches!(target_os.as_str(), "linux" | "macos") {
        // Strip input local (`STB_LOCAL`) symbols on ELF (including AArch64
        // `$d`/`$x` mapping symbols, `GCC_except_table*`, and `static` helpers)
        // and non-global symbols on Mach-O while keeping `.symtab` and all
        // global/weak symbols intact for `scripts/package-linux.sh`'s
        // static-dav1d audit and release crash diagnostics.
        println!("cargo:rustc-link-arg-bin=gitturtle=-Wl,-x");
    }
    emit("GITTURTLE_BUILD_REVISION", &revision);
    emit("GITTURTLE_BUILD_TREE", tree);
    emit(
        "GITTURTLE_BUILD_TARGET",
        &env::var("TARGET").expect("Cargo target"),
    );
    emit("GITTURTLE_BUILD_PROFILE", &profile);
    emit("GITTURTLE_BUILD_RUSTC", &rustc);
    emit("GITTURTLE_BUILD_TIME", &built_at.to_string());
}
