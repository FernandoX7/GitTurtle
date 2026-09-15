# macOS package validation

Read this for a package, app-icon, or bundled-resource task. Follow the [package script](../../../../scripts/package-macos.sh) and [icon pipeline](../../../../assets/icons/README.md); building a package is unnecessary for source-only interaction checks.

If the task asks to verify an existing release, inspect that exact artifact without replacing it. For a requested fresh package, prefer a new `.app` output path until it is verified; the script replaces a matching GitTurtle bundle in place and does not provide rollback. Finish active Git operations and quit the old copy before any authorized replacement. When compiled inputs changed, run from the repository root:

```sh
cargo build --release --locked -p gitturtle --target-dir target
./scripts/package-macos.sh --no-build /absolute/path/to/new/GitTurtle.app
```

For bundle-resource-only changes, the packaging command may reuse an existing release executable when its source revision and working-tree state are known and its Rust, dependency, and embedded-asset inputs remain unchanged. Packaging compiles `assets/AppIcon.icon` even with `--no-build`. Check `main.rs::Assets` and `EmbeddedAssets`: control SVGs and the branding PNG are embedded; the bundle ships compiled macOS icon resources. An executable timestamp or UUID alone does not establish source freshness; rebuild when provenance is unknown or compiled inputs changed. Repackaging alone does not require repeating clean Rust gates.

The packager copies `target/release/gitturtle` (or `target/debug/gitturtle` with `--debug`). Confirm the actual Cargo output and the executable's `--build-info` identify the intended source, target and profile before packaging. A configured nondefault Cargo target can put a fresh build elsewhere; resolve that mismatch instead of packaging an older file left at the expected path.

For packaged checks, select the intended app by its absolute path; multiple local bundles can share an identifier. For timing a package, start that package's executable with the environment and repository argument before attaching UI automation, for example when validating the default output:

```sh
GITTURTLE_TRACE=1 dist/GitTurtle.app/Contents/MacOS/gitturtle /path/to/repository
```

The script verifies the local ad-hoc signature. Compare `--build-info` and Mach-O UUIDs (`dwarfdump --uuid`) between the intended executable and bundle; signing may change raw executable bytes. Retain the final signed artifact hash separately, then close old processes and launch the verified path to establish which build is running.

Check `Contents/Info.plist`, `Resources/Assets.car`, fallback `AppIcon.icns` and generated icon-name keys against the [icon pipeline](../../../../assets/icons/README.md). For icon changes inspect small-size appearance in the app, Finder and Dock, including the requested native appearance variants. Verify embedded branding/control artwork in the executable; repackaging cannot update it. Check `Contents/Resources/licenses/` and any `REVIEW_REQUIRED.md`; local packaging reports notice gaps and does not clear the [public binary release requirements](../../../../docs/public-launch.md#before-a-public-binary-release).

Record the revision, signed artifact and actual macOS/architecture exercised. An ad-hoc signature does not establish Developer ID signing, notarization, Gatekeeper acceptance on another machine, universal architecture support or Linux compatibility.
