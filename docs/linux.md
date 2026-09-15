# Linux preview installation

Initial target: **Ubuntu 24.04 LTS, x86-64**, in a Wayland or X11 desktop
session. GitTurtle uses native GPUI; it needs a working graphics driver and
system fonts. The [validation record](validation.md) separates clean Ubuntu
userspace checks, virtual-display checks and actual desktop interaction.
A container build does not establish Ubuntu GNOME desktop compatibility.

## Install a local preview bundle

Use a bundle built on Ubuntu 24.04 with the procedure below. It contains the
release executable, installer, icon resources, license notices and checksums; neither Cargo nor
this checkout is needed on the receiving machine. This is a user-local build
bundle, not a signed distribution package, AppImage, Flatpak or `.deb`.
Public binary availability is announced through the project's
[GitHub releases](https://github.com/FernandoX7/GitTurtle/releases); a source
checkout or locally prepared bundle is not a published release.

On the receiving Ubuntu machine, install the runtime dependencies:

```sh
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  git openssh-client ca-certificates python3 desktop-file-utils xdg-utils \
  libxcb1 libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libwayland-cursor0 \
  libwayland-egl1 libfontconfig1 fontconfig fonts-dejavu-core fonts-dejavu-mono \
  libvulkan1 mesa-vulkan-drivers libegl1 libgl1 libgl1-mesa-dri \
  xdg-desktop-portal xdg-desktop-portal-gnome xdg-desktop-portal-gtk
```

Ubuntu GNOME normally supplies much of this. Keep the appropriate vendor GPU
driver installed, especially on NVIDIA systems; `libvulkan1` is only the loader.
Mesa supplies its drivers and a software implementation, not the proprietary
NVIDIA driver. Other desktops need their matching portal backend instead of
assuming GNOME's backend works there. Git LFS is optional (`git-lfs`) for the
explicit LFS download feature; ordinary local inspection does not download.

Transfer the `.tar.gz` and `.tar.gz.sha256` together from a trusted teammate.
With the default archive name, run from their receiving directory:

```sh
sha256sum -c gitturtle-linux-x86_64.tar.gz.sha256
tar -xzf gitturtle-linux-x86_64.tar.gz
python3 gitturtle-linux-x86_64/install.py
```

Run the installer **without sudo**. It checks bundle hashes, linked libraries
and desktop-entry syntax before installation, then atomically replaces the
executable at `~/.local/bin/gitturtle`. Icons and the application entry go under
`$XDG_DATA_HOME`, or `~/.local/share` when unset. The installer supports spaces
in these paths; desktop `Exec` restrictions require the executable path to
exclude newlines, tabs, `=` and `%`. `XDG_DATA_HOME` must be absolute.

Open **GitTurtle** in the application menu. Its desktop file is
`applications/com.gitturtle.desktop.desktop`, matching the application ID.
The entry uses an absolute executable and icon path, with `Terminal=false`;
no shell initialization, checkout working directory, `LIBRARY_PATH`,
`LD_LIBRARY_PATH`, or Cargo installation is needed. The original 1024-pixel
PNG is used by the entry, with derived 16, 24, 32, 48, 64, 128, 256 and 512-pixel
hicolor icons for desktop lookup by application ID. It does not rely on an
unregistered `hicolor/1024x1024` directory.

Let active Git operations finish and quit GitTurtle before upgrading, then
extract the new archive and rerun its installer. The installer checks the
installed executable against accessible processes in `/proc` and refuses an
upgrade while a matching process is running; it never stops a process. Existing
settings and drafts stay in place. The extracted bundle
and source directory may be moved or removed after successful installation.
Checksums detect corruption; they do not authenticate an untrusted sender.

The bundle includes the project license and resolved dependency/font notices in
`licenses/`. Installation retains them under `$XDG_DATA_HOME/gitturtle/licenses`
(or `~/.local/share/gitturtle/licenses`) after you remove the extracted bundle.
The same directory's parent retains `build-info.json` and `install.py` for
build identification and recovery.

Before replacing an existing installation, the installer saves its executable,
launcher, icons, build metadata and license files under
`$XDG_DATA_HOME/gitturtle/install-backups/`. It verifies backup checksums before
restoring anything. To return to the previous installation after quitting the app:

```sh
python3 "${XDG_DATA_HOME:-$HOME/.local/share}/gitturtle/install.py" --rollback
```

Rollback preserves settings and drafts. Running it again returns to the build
just replaced. Backups remain available under `install-backups/`; remove older
backups only when you no longer need them. Installation and rollback are
serialized, and an ordinary file-copy failure restores the saved files. If the
installer is forcibly interrupted, the saved backup remains available for
recovery; individual file replacements are atomic, not the whole installation.

## Build from source on Ubuntu 24.04

Install the runtime packages above, then the build/packaging prerequisites:

```sh
sudo apt-get install -y --no-install-recommends \
  build-essential clang cmake pkg-config curl \
  libfontconfig-dev libwayland-dev libxkbcommon-x11-dev libx11-xcb-dev \
  libssl-dev libzstd-dev libvulkan-dev python3-pil
```

Install Rust through [rustup](https://rustup.rs/) if needed. The checked-in
`rust-toolchain.toml` selects **Rust 1.98.0** with rustfmt and Clippy only for
this checkout; other projects' default toolchains are unchanged. The workspace
minimum is Rust 1.98. `Cargo.lock` pins the matching GPUI Kit 0.6.0 / GPUI 0.3.4
stack and local patches. Do not independently upgrade toolkit components or
use Ubuntu's older packaged Rust in place of the selected toolchain.

From the repository root:

```sh
rustup show active-toolchain
cargo fmt --all -- --check
cargo build --release --locked -p gitturtle --target x86_64-unknown-linux-gnu --target-dir target
./target/x86_64-unknown-linux-gnu/release/gitturtle /absolute/path/to/repository
./scripts/package-linux.sh --no-build
```

Alternatively, `./scripts/package-linux.sh` checks native development packages
and builds the locked release itself. Both forms produce
`dist/gitturtle-linux-x86_64/`, its `.tar.gz` and archive checksum.
Choose another output directory on subsequent runs, for example
`./scripts/package-linux.sh --no-build dist/ubuntu-test-2`; the packager refuses
to overwrite existing outputs. Both modes use the explicit x86-64 target path,
regardless of your other Cargo target settings. `--no-build` requires an already
built release from the intended source; to reuse a separately built artifact,
pass its path explicitly, for example `--no-build --binary target/release/gitturtle`.
Packaging also needs Cargo and the resolved locked sources to collect license
texts, including with `--no-build`. Review any generated `REVIEW_REQUIRED.md`
before redistribution: unresolved upstream notices block a public binary release.
The collector's `--require-complete` mode enforces that release gate; local
development packaging reports the gaps without claiming distribution clearance.
`build-info.json` records the packaging revision,
working-tree status, executable hash and whether the packager rebuilt it;
it does not certify the source identity of an arbitrary reused binary.
Current executables also embed their own source revision, source-tree state,
target, build profile, compiler version and build time. These belong to the
executable and remain available after the source checkout moves or changes.
`unknown` identifies a source archive without Git metadata; `modified` records
local source changes and is not a reproducible commit identity. Keep the
executable SHA-256 with reports or measurement records involving such a build.

The packaged executable embeds UI assets. Build on the oldest supported target
(Ubuntu 24.04 here) rather than transferring a binary built against newer glibc.
No local link workaround is needed with the development packages installed:
`libxkbcommon-x11-dev` supplies the linker file `libxkbcommon-x11.so`; the
runtime package supplies the versioned `libxkbcommon-x11.so.0`. A manually made
symlink in `target/` is not part of the supported installation.

## Launch, state and troubleshooting

```sh
"$HOME/.local/bin/gitturtle" /absolute/path/to/repository
"$HOME/.local/bin/gitturtle" --version
"$HOME/.local/bin/gitturtle" --build-info
```

The information flags work without a graphical session and do not open a
repository or load saved app state. In the app, **Main Menu → About GitTurtle**
(or **Command Palette → About GitTurtle**) shows the version, source revision,
target and build profile. **Copy bug diagnostics** copies build information,
display backend/scale, theme, density and text sizes. It includes no repository
paths, source text, branch names, account details or credentials. Paste this
with reproduction steps and the observed/expected behavior in a
[GitHub issue](https://github.com/FernandoX7/GitTurtle/issues).

With no repository argument, the startup setting opens Projects or restores
the saved session. Application preferences, repository sessions and local
drafts live under `$XDG_CONFIG_HOME/gitturtle`, falling back to
`~/.config/gitturtle`; repository data stays in the repository. Back up that
configuration directory before destructive cleanup. Avoid concurrent app
instances sharing one configuration directory during testing.

The repository picker uses the session D-Bus service `xdg-desktop-portal` and
its FileChooser backend. Cancel leaves the current context intact. A failed
request now shows the underlying error and recovery guidance; a command-line
repository path remains available when the picker cannot run. After installing
portal packages, log out and back in if the session has stale service state.

| Symptom | Check / recovery |
| --- | --- |
| Build reports missing `-lxkbcommon-x11` | Install `libxkbcommon-x11-dev`, then rebuild without custom `LIBRARY_PATH`. `pkg-config --libs xkbcommon-x11` must succeed. |
| Installer says a shared library is missing | Install the runtime list; inspect `ldd` on the trusted bundled executable. A clean `ldd` alone does not check dynamically loaded graphics drivers. |
| Installer says GitTurtle is still running | Let its Git operation finish, quit the app normally, then rerun the installer. The message lists matching process IDs; the installer never terminates them. |
| Upgrade causes a regression | Quit the app, run the retained installer's `--rollback` command above, then include build diagnostics for both versions in your report. |
| No window when launched over SSH, from a console, or in CI | A normal launch needs a graphical session. GitTurtle exits with a specific no-display message; do not invent display variables or set `ZED_HEADLESS` to claim a desktop test. |
| Window/graphics initialization fails | Launch from a desktop terminal to capture stderr; check `vulkaninfo --summary` (`vulkan-tools`) and your GPU driver. An invalid display connection can still fail inside the toolkit before window creation. |
| Picker does nothing / reports a portal failure | Check `systemctl --user status xdg-desktop-portal xdg-desktop-portal-gnome`; inspect `journalctl --user -b -u xdg-desktop-portal`. Check the matching backend, then reopen the app. |
| Blank launcher icon / menu entry absent | Rerun the installer and `desktop-file-validate` on the installed entry; confirm the entry's absolute `Icon` path exists. Refresh the app menu or log out/in if its cache remains stale. |
| Missing or cramped text | Check `fc-match sans-serif` and `fc-match 'DejaVu Sans Mono'`; install both DejaVu packages above. Settings has separate interface/code text sizes. |
| Text smaller than in other apps | On Wayland, GitTurtle multiplies its text sizes by the desktop text scaling factor (GNOME Settings › Accessibility › Large Text, `org.gnome.desktop.interface text-scaling-factor`). On X11 the toolkit scales the whole window through `Xft.dpi` instead. The app's own interface/code sizes apply on top. |
| Colored fringes or soft text, typically on an OLED or rotated panel | GitTurtle follows the desktop antialiasing preference. GNOME's default `font-rendering` "automatic" renders grayscale like GTK 4; "manual" follows `font-antialiasing` (Tweaks › Fonts), where `rgba` selects subpixel rendering. Other desktops are read through fontconfig: `fc-match --format '%{antialias}\|%{rgba}\n' sans-serif`. Grayscale is the safe choice; changes apply without a restart. |
| Need an X11 comparison in a session that provides XWayland | Launch once with `env -u WAYLAND_DISPLAY "$HOME/.local/bin/gitturtle" /path/to/fixture`. This tests XWayland, not a full Xorg session. |

No `DISPLAY`, `WAYLAND_DISPLAY`, GPU-selection or library variable belongs in
the installed desktop entry. It inherits the normal desktop session.

## Window controls and quitting

Close with **×**, or quit with **Ctrl+Q**. Linux desktops supplying server
window decorations retain their title bar. For client decorations, including
GNOME Wayland, the tab strip supplies controls in the desktop's configured
position/order, including minimize/maximize when enabled. Drag blank tab-strip
space to move; double-click to maximize/restore; right-click for the window
menu when supported. Closing the final window uses normal shutdown and draft
saving. macOS retains system traffic lights and **Cmd+Q** / the native Quit
menu item. Linux keyboard shortcuts use Control instead of Command.

## Platform limits

- GitHub account connection cannot persist credentials on Linux yet; it refuses
  storage instead of writing plaintext credentials. Git's own configured
  authentication for explicit fetch/pull/push is separate.
- Native PDF page rendering/text extraction and ImageIO-based AVIF, HEIC and
  JPEG 2000 decoding require macOS. PNG/JPEG/GIF/WebP, supported SVG, rendered
  Markdown and finite mesh/GLB/STEP previews have portable decoders; the
  [format matrix](file-previews.md) defines bounds and unsupported variants.
  Recognizing metadata is not the same as rendering a format.
- System Preview uses macOS Quick Look and has no Linux launcher.
- Automatic refresh skips ignored working-tree directories while preserving
  coverage for tracked files inside them and essential Git metadata. Linux
  coverage is bounded to 16,384 directory registrations and a scan budget;
  reaching a limit keeps existing watches and shows a quiet status with
  Refresh and copyable Details. Ignore-rule edits reconsider affected coverage,
  including shared and global excludes. Refresh or returning focus retries
  degraded coverage. See the [local refresh contract](../crates/app/docs/navigation-and-refresh.md#local-refresh).
- Automatic reading of OS reduced-motion, contrast and transparency
  accessibility preferences is macOS-only. Manual app preferences remain
  available; Linux screen-reader and IME coverage is not established.
- Glyph antialiasing reads `font-rendering` and `font-antialiasing` from the
  XDG settings portal (`org.gnome.desktop.interface`). GNOME Automatic mode
  uses grayscale, matching [GTK 4](https://docs.gtk.org/gtk4/property.Settings.gtk-xft-rgba.html);
  Manual mode follows the antialiasing key. Missing or invalid desktop values
  fall back to fontconfig's resolved `antialias`/`rgba` defaults, then grayscale.
  Hinting and horizontal stripe order remain toolkit decisions; vertical
  fontconfig stripes and "none" antialiasing are rendered grayscale.
- Portal changes apply while the app runs. Returning focus to a window rereads
  the snapshot and fontconfig fallback, and retries an unavailable portal.
  Fontconfig-only changes therefore apply on focus return, without idle polling.
  Unresolved antialiasing uses grayscale. Portal setup/read work has
  a 250 ms deadline; `fc-match` has a 100 ms execution deadline and 128-byte
  output limit. Launch waits at most 400 ms for the initial applied snapshot;
  outstanding portal futures are cancelled and failed child queries are reaped.
- The portal's `text-scaling-factor` multiplies both saved app text sizes on
  Wayland, within 0.5–3.0; nonpositive or nonfinite values use 1.0. This does
  not implement KDE-specific font DPI preferences. The toolkit's X11 backend
  already reads `Xft.dpi`/RandR scaling (but not XSettings DPI), so GitTurtle
  does not apply the factor a second time on X11. Fractional scaling and live
  monitor changes still need checks on the target desktop.
- This bundle does not provide automatic updates, desktop file associations,
  distribution signing or support guarantees for other Ubuntu releases/CPUs.

## Ubuntu desktop acceptance checklist

Run these on an actual teammate Ubuntu 24.04 GNOME desktop; virtual X servers
and nested compositors do not verify GNOME shell, drivers or portal integration.
Use `python3 scripts/create-demo-repo.py --output /tmp/gitturtle-demo-NEW`
from a source copy to create a disposable fixture; it refuses a nonempty
output and also creates a sibling linked worktree. Do not use a work repository
for staging or mutation tests. Keep the executable hash, Ubuntu/GNOME version,
GPU/driver, session type and scale setting with the result.

- [ ] Install/extract from Downloads, then move the extracted bundle and source.
  Launch from Applications after login; check the menu, dock and Alt-Tab icon.
- [ ] On Wayland and, where offered, a separate Ubuntu on Xorg login: minimize,
  maximize/restore, move/resize, close, Ctrl+Q and reopen. Confirm session,
  selected commit/file and an uncommitted Title/Description draft survive.
- [ ] Check 100%, 200% and an available fractional display scale; readable UI/code,
  icons and fonts, text entry, shortcuts and mixed-DPI monitor movement.
- [ ] With the app open, toggle Settings › Accessibility › Large Text and switch
  Tweaks › Fonts › Antialiasing between Subpixel and Grayscale (with rendering
  set to Manual): text size and glyph edges follow without a restart.
- [ ] Open and cancel the repository picker from Projects and Ctrl+O, including
  a path containing spaces. Confirm errors are useful if the portal is absent.
- [ ] Select a commit (stays in History), activate text/PNG/Markdown files, Back,
  then rapid activation/Back; inspect image zoom/overlay and a supported model.
- [ ] In Working Changes, stage and unstage fixture files/hunks. Verify with
  `git diff` and `git diff --cached`; ensure unrelated fixture edits survive.
  Enter a draft, navigate away, quit and reopen, then verify exact draft text.

To uninstall the user-local app, remove `~/.local/bin/gitturtle`, the installed
`applications/com.gitturtle.desktop.desktop`, `icons/com.gitturtle.desktop.png`
and the eight `icons/hicolor/SIZExSIZE/apps/com.gitturtle.desktop.png` files
and `gitturtle/licenses/`, `gitturtle/build-info.json`, `gitturtle/install.py`,
`gitturtle/install.lock`, `gitturtle/previous-installation.json` and any unwanted
`gitturtle/install-backups/` under the data directory used at installation. Leave other icons and the
configuration directory intact unless you explicitly want to discard state.
