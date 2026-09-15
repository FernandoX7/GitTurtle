# macOS package validation

Read this for a package, app-icon, or bundled-resource task. Follow the [package script](../../../../scripts/package-macos.sh) and [icon pipeline](../../../../assets/icons/README.md); building a package is unnecessary for source-only interaction checks.

If the task asks to verify an existing release, inspect that exact artifact without replacing it. For a requested fresh package, prefer a new `.app` output path until it is verified; the script validates a matching GitTurtle bundle and stages its replacement with rollback on publication failure. This is not a retained user-facing previous-version backup. Finish active Git operations and quit the old copy before any authorized replacement. When compiled inputs changed, run from the repository root:

```sh
cargo build --release --locked -p gitturtle --target aarch64-apple-darwin --target-dir target
./scripts/package-macos.sh --no-build /absolute/path/to/new/GitTurtle.app
```

For bundle-resource-only changes, the packaging command may reuse an existing release executable when its source revision and working-tree state are known and its Rust, dependency, and embedded-asset inputs remain unchanged. Packaging compiles `assets/AppIcon.icon` even with `--no-build`. Check `main.rs::Assets` and `EmbeddedAssets`: control SVGs and the branding PNG are embedded; the bundle ships compiled macOS icon resources. An executable timestamp or UUID alone does not establish source freshness; rebuild when provenance is unknown or compiled inputs changed. Repackaging alone does not require repeating clean Rust gates.

The packager copies `target/aarch64-apple-darwin/release/gitturtle` (or the corresponding `debug` path with `--debug`). For an established binary elsewhere, pass `--no-build --binary /absolute/path/to/gitturtle`. Confirm its `--build-info`, expected source/version and SHA-256 before packaging. Reusing a modified build or packaging checkout requires `--expected-sha256` to pin the reviewed input; a timestamp is insufficient. The current package supports arm64 only and requires Xcode 26+, Metal and the declared icon-tool prerequisites.

Use `--archive-dir /absolute/new/directory` to produce the version/source-labelled ZIP, checksum and archive manifest. The detached `.app.build-info.json` records pre-sign and final executable digests; ZIP `build-info.json` remains outside the app's sealed resources. Extract the actual archive and validate its manifest, signature and launch path. `--distribution` requires complete notices and a clean release identity. Preserve failed-stage diagnostics if publication detects concurrent output changes.

For packaged checks, select the intended app by its absolute path; multiple local bundles can share an identifier. For timing a package, start that package's executable with the environment and repository argument before attaching UI automation, for example when validating the default output:

```sh
GITTURTLE_TRACE=1 dist/GitTurtle.app/Contents/MacOS/gitturtle /path/to/repository
```

The script verifies the local ad-hoc signature. Compare `--build-info` and Mach-O UUIDs (`dwarfdump --uuid`) between the intended executable and bundle; signing may change raw executable bytes. Retain the final signed artifact hash separately, then close old processes and launch the verified path to establish which build is running.

Check `Contents/Info.plist`, `Resources/Assets.car`, fallback `AppIcon.icns` and generated icon-name keys against the [icon pipeline](../../../../assets/icons/README.md). For icon changes inspect small-size appearance in the app, Finder and Dock, including the requested native appearance variants. Verify embedded branding/control artwork in the executable; repackaging cannot update it. Check `Contents/Resources/licenses/` and any `REVIEW_REQUIRED.md`; local packaging reports notice gaps and does not clear the [public binary release requirements](../../../../docs/public-launch.md#before-a-public-binary-release).

Record the revision, signed artifact and actual macOS/architecture exercised. An ad-hoc signature does not establish Developer ID signing, notarization, Gatekeeper acceptance on another machine, universal architecture support or Linux compatibility.
