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
        "../git-core/src",
        "../git-core/Cargo.toml",
        "../preview/src",
        "../preview/Cargo.toml",
        "../../Cargo.toml",
        "../../Cargo.lock",
        "../../assets",
        "../../vendor/gpui-base",
        "../../vendor/gpui-component",
        "../../vendor/gpui-pre-macos",
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
    emit("GITTURTLE_BUILD_REVISION", &revision);
    emit("GITTURTLE_BUILD_TREE", tree);
    emit(
        "GITTURTLE_BUILD_TARGET",
        &env::var("TARGET").expect("Cargo target"),
    );
    emit(
        "GITTURTLE_BUILD_PROFILE",
        &env::var("PROFILE").expect("Cargo profile"),
    );
    emit("GITTURTLE_BUILD_RUSTC", &rustc);
    emit("GITTURTLE_BUILD_TIME", &built_at.to_string());
}
