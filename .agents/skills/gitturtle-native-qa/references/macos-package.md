# macOS package validation

Read this for a package, app-icon, or bundled-resource task. Follow the [package script](../../../../scripts/package-macos.sh) and [icon pipeline](../../../../assets/icons/README.md); building a package is unnecessary for source-only interaction checks.

If the task asks to verify an existing release, inspect that exact artifact without replacing it. For a requested fresh package, quit any running copy before replacing its bundle. When compiled inputs changed, run from the repository root:

```sh
cargo build --release --locked -p gitturtle --target-dir target
./scripts/package-macos.sh --no-build
```

For bundle-resource-only changes, the packaging command may reuse an existing release executable when its source revision and working-tree state are known and its Rust, dependency, and embedded-asset inputs remain unchanged. Packaging compiles `assets/AppIcon.icon` even with `--no-build`. Check `main.rs::Assets` and `EmbeddedAssets`: control SVGs and the branding PNG are embedded; the bundle ships compiled macOS icon resources. An executable timestamp or UUID alone does not establish source freshness; rebuild when provenance is unknown or compiled inputs changed. Repackaging alone does not require repeating clean Rust gates.

For packaged checks, select the intended app using the absolute path to `dist/GitTurtle.app`; multiple local bundles can share an identifier. For timing a package, start its executable with the environment and repository argument before attaching UI automation:

```sh
GITTURTLE_TRACE=1 dist/GitTurtle.app/Contents/MacOS/gitturtle /path/to/repository
```

The script verifies the local ad-hoc signature. To check that the packaged executable matches the release build, compare build identity (on macOS, `dwarfdump --uuid`); signing may change raw executable bytes. Close old processes and launch the verified path to establish which build is running. Record the revision and package actually exercised. Local packaging does not establish notarization, universal architecture support, or Linux compatibility.
