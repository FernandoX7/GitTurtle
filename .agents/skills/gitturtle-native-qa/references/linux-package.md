# Linux package and installation validation

Read this for a Linux bundle, installer, launcher/icon or upgrade/recovery task. The [Linux runbook](../../../../docs/linux.md) owns supported targets, dependencies, commands and desktop limits; do not reproduce its installation checklist here. The local bundle targets Ubuntu 24.04 x86-64. A build on another userspace or architecture needs its own evidence.

## Establish the payload

Verify an existing bundle without rebuilding or replacing it. A fresh [package](../../../../scripts/package-linux.sh) uses the explicit `x86_64-unknown-linux-gnu` target and refuses existing output paths. With `--no-build`, establish the reused executable's source identity separately; `--binary PATH` selects an alternate release path. The packaging revision alone does not identify that binary.

Compare the payload's `--build-info`, `build-info.json` compiled identity and binary SHA-256 with the source/patch and target actually built. Check the archive checksum and extracted `SHA256SUMS`, executable, installer, launcher resources, icons and retained license inventory. Report unresolved notice entries against the [public release checklist](../../../../docs/public-launch.md#before-a-public-binary-release); a locally installable archive is not distribution clearance.

## Exercise installation and recovery

For changed installer behavior, use its existing disposable fixtures. When the current native bundle is available on the target system, include it:

```sh
python3 scripts/test-install-linux.py
python3 scripts/test-install-linux.py --bundle /absolute/path/to/verified/bundle
```

The first form skips real-bundle tests. The second adds actual payload installation, relocation/removal and rollback checks in disposable homes/data directories, with a synthetic prior executable. Neither establishes desktop launch or an older release's compatibility with newer app state. For a claimed upgrade/downgrade path, preserve the prior real payload and test relevant state copies, including supported-version refusal, before replacing the user's installation.

Follow the runbook for an authorized real installation: finish active Git operations, quit, install without sudo, and retain the recovery point. Check active-process refusal, checksum/ownership validation, stale extra-file cleanup and preservation of unrelated configuration when those paths change. Installation replaces individual files atomically; forced interruption is recovered from the retained backup, not a whole-installation transaction. A rollback must preserve preferences and drafts even if the older app cannot interpret their version.

## Separate package checks from desktop checks

Validate the installed executable hash, retained metadata/licenses, absolute desktop `Exec`/`Icon` paths and launch after the extracted bundle is moved. Run the [actual desktop acceptance checklist](../../../../docs/linux.md#ubuntu-desktop-acceptance-checklist) for the affected platform behavior, using disposable repositories and app state. Record whether the session is physical Wayland, Xorg, XWayland or nested/virtual, plus compositor, GPU/driver, display and text scales.

ELF/library checks and a useful no-display error do not verify graphics, portals, accessibility, menus or real input. Confirm a picker actually returns the chosen directory, and distinguish Cancel from portal failure. For text changes, keep Wayland portal text scaling separate from X11 toolkit DPI; inspect source rows, split alignment, Find/selection and retained tabs through live changes. Use the runbook's capability limits for Linux credentials, macOS-only formats and system integrations rather than recording their expected unavailability as a rendering pass.
