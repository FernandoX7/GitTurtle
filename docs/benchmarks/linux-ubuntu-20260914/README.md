# Ubuntu readiness — September 14, 2026

The Ubuntu 24.04 x86-64 source build and a separate runtime-only installation
passed. Native GPUI interactions were exercised on virtual X11 and nested
Wayland displays with Ubuntu userspace. **An actual Ubuntu GNOME login was
not available.** The teammate procedure and remaining acceptance checklist
are in the [Linux runbook](../../linux.md).

## Source and environment

App source is `6824c7d`; installer/toolchain/runbook source starts at `d482a3f`,
with the archive-ownership and explicit target-directory corrections below.
The Ubuntu release executable SHA-256 is
`3c39630d691de172ee8302ab0e8bf30976dbc9cd80c957919f357ff230c1af75`.
The same bytes were installed for the runtime-only, virtual-display and
physical-host probes. All 872 recorded source/manifest/asset inputs matched
the workspace after the Rust checks; later edits affect packaging and
documentation/evidence only, not the executable.
[Machine-readable identities](evidence.json) include the input digest,
[per-file inputs](build-inputs.json), check results and screenshot hashes.

Two separate roots came from Canonical's Ubuntu Base 24.04.5 amd64 image
(SHA-256 `e77b6f10c2590cef872b33ee9f635a0e3fd1f57fb074c0e52b5c7f56147a0c86`).
The published SHA256SUMS signature verified against Ubuntu CD Image Automatic
Signing Key (2012), fingerprint
`843938DF228D22F7B3742BC0D94AA3F0EFE21092`.
Rootless bubblewrap exposed Ubuntu's own filesystem, `/proc`, a private `/dev`,
read-only `/sys`, and a local evidence directory. No host `/usr`, `/lib`, Rust
installation, Cargo cache or compiled output was mounted into the build.
The roots shared the Pop!_OS kernel `7.1.5-76070105-generic`; this was not an
Ubuntu kernel/boot/VM check.

The host has no passwordless sudo or installed Docker/Podman/QEMU. Unprivileged
user/mount namespaces worked. Apt initially encountered unmapped package
ownership with the single-UID/GID mapping; Ubuntu `fakeroot` with
`FAKEROOTDONTTRYCHOWN=1` repaired package provisioning only. Cargo and runtime
commands used cleared environments without `LD_PRELOAD`, `LIBRARY_PATH`,
`LD_LIBRARY_PATH`, `RUSTFLAGS` or the previous local link directory.

## Build and installation results

| Check | Result |
| --- | --- |
| Fresh locked release build | Passed with Rust 1.98.0 / Cargo 1.98.0 / Ubuntu GCC 13.3.0. No missing-library failure or workaround. |
| Formatting and `cargo check --locked -p gitturtle` | Passed. |
| `cargo test --locked --workspace` | 694 passed, zero failed, five intentionally ignored. Includes both new picker tests and disposable Git workflow/preview fixtures. |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | Passed. |
| ELF / linked libraries | Every dependency resolved; no RPATH/RUNPATH. [Dynamic entries](release-elf-dynamic.txt), [ldd](release-ldd.txt). |
| Documented build package completeness | Nested-desktop tooling added no extra `-dev` packages beyond the documented build set; [comparison](build-dependency-comparison.txt). |
| Separate pristine runtime root | Only the runbook's runtime package set and its dependencies were installed for the runtime check. No Cargo/GCC, native build libraries or unversioned `libxkbcommon-x11.so` link. `systemd-dev` is a transitive runtime dependency, so this is not a claim of zero packages ending in `-dev`. |
| Relocated archive / install | Passed from `/`, without a checkout, with only HOME, XDG_DATA_HOME and `/usr/bin:/bin` PATH. HOME/data paths contained spaces. Reinstall after moving the bundle passed. |
| Launcher / icons / permissions | Gio discovered the installed app by ID with `should_show=true` and decoded absolute Exec/Icon paths and `Terminal=false`. Headless GTK hicolor lookup and native loading resolved all eight exact-size PNGs through normal XDG search paths. Desktop syntax, executable mode 0755 and unchanged bytes passed; [lookup record](runtime-desktop-lookup.json). No actual Ubuntu application-menu or dock appearance check. |
| Negative installation checks | Corrupt content, empty/truncated manifest, unsupported executable-path characters, relative HOME/data directory and missing development-package preflight produced useful errors. Empty/truncated manifests and relative HOME failed before destination writes. |
| No-display launch | Installed binary exited 1 with the new desktop-session message, without a panic or preference writes. This is a headless diagnostic check, not a GUI pass. |

The ignored tests require explicitly isolated gh, OpenPGP or loopback sshd
contexts, or run the release review benchmark/SVG mutation probe. Their
absence is not covered by the passing count. This run did not execute hosted
CI, a macOS build, live-provider authentication or network Git workflows.
The authored CI now packages, extracts, installs and checks the no-display
failure after its Ubuntu release build.

A final archive created on the host initially failed ordinary `tar` extraction
in the single-UID runtime namespace because it recorded the builder's UID/GID
1000. The packager now stores neutral numeric owner/group 0. Ordinary extraction,
checksum verification, installation from `/` with a cleared environment,
executable identity/mode and the no-display diagnostic then passed in the same
runtime root; [targeted record](archive-owner-check.json). Normal non-root
teammates do not need extra extraction flags. The manual build command also
pins `--target-dir target`, matching the packager and preventing custom Cargo
target-directory settings from making later commands use an older executable.

## Virtual native interaction

The installed release ran from `/` with isolated application state and the
normal display/session variables, without custom linker/GPU-selection
variables. The fixture came from `scripts/create-demo-repo.py` at
`/qa-fixtures/demo`; all writes stayed in this disposable repository. Native
screenshots were captured from the virtual X server with Pillow ImageGrab;
input used XTest/xdotool and the app's AT-SPI actions. These are actual GPUI
windows, not browser recreations.

**X11:** Ubuntu Xvfb 21.1.11, 1920×1200 at normal scale, Openbox decorations.
The environment provided Mesa 25.2.8 / llvmpipe (LLVM 20.1.2); no host GPU
device was passed through. Vulkan enumerated llvmpipe. The exact renderer
backend selected by the app was not instrumented.

- Commit selection stayed in History; explicit file activation entered text
  Compare; Back retained the selected `e96f692…` commit and changed-file list.
- Visually inspected syntax/gutters, before/after PNG with transparency,
  Overlay, SVG and rendered Markdown (heading, emphasis, table, code).
- Working Changes listed two edited files and a new Markdown file. Native
  **Stage README.md** put only README in the index; **Unstage README.md** left
  the index empty. Independent `git diff --cached` / `git diff` checks confirmed
  the unrelated CSS edit and working bytes were retained. No commit was made.
- Title `Ubuntu QA draft` and description `Persist exact draft text.` were
  entered, saved in preferences and visibly restored after Ctrl+Q/relaunch.
  The selected history commit also survived. The X11 window-manager Close
  button terminated a later instance normally.
- Missing portal service produced the new visible recovery dialog. With an
  isolated GTK portal, Open Folder and cancellation worked. A first attempt
  using different PID namespaces failed with `/proc/0/root`; sharing the GUI
  processes' PID namespace fixed that environment error. Directory selection
  still did not complete during the isolated GTK probe, so successful picking
  remains unverified. No app change is attributed to that unproven outcome.

**Wayland:** Weston 13.0.0 with its X11 backend and Pixman compositor, first
1800×1120 at scale 1, then 3600×2240 at scale 2 on a 3840×2400 virtual X server.
The app connected to the Wayland socket, not X11. Screenshots verified readable
history/Compare, fonts and artwork, retained drafts and a 12-triangle GLB
`BoxInterleaved` preview with an absent Before side. History → Compare → Back
and the app's **Close window** action passed. Weston exposed only Close in
this session's button configuration; GNOME minimize/maximize/drag are not
established by this run. Integer scale 2 was a separate launch, not a live
monitor transition or fractional-scale check.

Software graphics emitted EGL/DRI3/Zink adapter-probing warnings; the windows
continued rendering and interacting. Isolated portals emitted missing FUSE,
RealtimeKit/PipeWire and some AT-SPI/GTK warnings. Those infrastructure warnings
are recorded separately from app failures; no GPU performance claim is made.

Selected original screenshots:

| Workflow | Evidence |
| --- | --- |
| X11 history / text | [History](screenshots/x11-history.png), [Compare](screenshots/x11-text-compare.png) |
| Portable previews | [PNG](screenshots/x11-image-compare.png), [SVG](screenshots/x11-svg.png), [Markdown](screenshots/x11-markdown-rendered.png) |
| Working draft / failure message | [Draft](screenshots/x11-draft.png), [portal error](screenshots/x11-picker.png) |
| Wayland / integer scale | [History at 1×](screenshots/wayland-history.png), [restored draft](screenshots/wayland-draft.png), [GLB at 2×](screenshots/wayland-scale2-glb.png) |

## Physical host and remaining desktop coverage

The identical Ubuntu-built executable also opened Projects on the existing
Pop!_OS 24.04 GNOME/Wayland desktop, from a temporary path with isolated app
preferences. The native accessibility tree exposed Minimize, Maximize, Close
and the real GNOME Open Folder dialog. The computer-use provider offered no
screenshots and rejected keyboard delivery for lack of verifiable focus;
maximize/restore and directory selection were not established. After the
accessibility bridge was toggled back, the tree became too shallow for a
verified Close action, so the isolated empty Projects instance was terminated
by its checked executable/PID. This is startup/semantic evidence only, not a
new host visual or graceful-shutdown pass.

The prior user instance was left running unchanged. The accessibility bridge
and GNOME toolkit-accessibility setting were restored to false; no screen
reader was started. Fixture application state stayed isolated throughout.

Still required: actual Ubuntu GNOME application-menu/dock/Alt-Tab integration;
portal directory selection/cancellation with a normal login; GNOME control
and keyboard behavior; real Wayland and Xorg sessions on teammate drivers;
fractional/mixed-monitor scaling; IME/screen-reader behavior; hunk/line staging
and broader preview interactions in that desktop. The short
[acceptance checklist](../../linux.md#ubuntu-desktop-acceptance-checklist)
covers these without claiming the container establishes them.
