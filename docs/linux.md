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
  libwayland-egl1 libfontconfig1 fontconfig fonts-dejavu-core libdav1d7 \
  libvulkan1 mesa-vulkan-drivers libegl1 libgl1 libgl1-mesa-dri \
  xdg-desktop-portal xdg-desktop-portal-gnome xdg-desktop-portal-gtk
```

Ubuntu GNOME normally supplies much of this. `libdav1d7` (from universe) is
optional at startup: GitTurtle loads `libdav1d.so.7` dynamically inside its
confined Linux preview helper when decoding AVIF images, so GitTurtle starts
cleanly without it and reports AVIF unavailable only if an AVIF preview is
opened (see [Platform limits](#platform-limits)). Keep the appropriate vendor GPU
driver installed, especially on NVIDIA systems; `libvulkan1` is only the loader.
Mesa supplies its drivers and a software implementation, not the proprietary
NVIDIA driver. Other desktops need their matching portal backend instead of
assuming GNOME's backend works there. Git LFS is optional (`git-lfs`) for the
explicit LFS download feature; ordinary local inspection does not download.

Transfer the `.tar.gz`, `.tar.gz.sha256` and `.tar.gz.manifest.json` together
from a trusted teammate. The external manifest identifies the archived package,
its source/version/target and both the archive and package-manifest hashes.
With the default archive name, run from their receiving directory:

```sh
sha256sum -c gitturtle-linux-x86_64.tar.gz.sha256
tar -xzf gitturtle-linux-x86_64.tar.gz
python3 gitturtle-linux-x86_64/install.py
```

A bundle packaged on an AArch64 machine is named `gitturtle-linux-aarch64`
instead. The installer refuses a bundle built for another architecture.

An archive built by CI (a Quality run's artifact, or a GitHub release once one is
published) is named
`GitTurtle-<version>-<source commit, 12 characters>-linux-x86_64-release.tar.gz`,
or the same with `linux-aarch64`, and unpacks to a folder of that name without
`.tar.gz`: use those names in the three commands above.

Run the installer **without sudo**. It checks the required payload hashes,
package/compiled identity, license inventory, linked libraries and desktop-entry
syntax before installation, then atomically replaces the
executable at `~/.local/bin/gitturtle`. Icons and the application entry go under
`$XDG_DATA_HOME`, or `~/.local/share` when unset. The installer supports spaces
in these paths; desktop `Exec` restrictions require the executable path to
exclude newlines, tabs, `=` and `%`. `XDG_DATA_HOME` must be absolute. A user-selected home/data root may itself be
a symlink, but managed descendants, payload paths, backups and the installation
lock must not redirect through symlinks.

Open **GitTurtle** in the application menu. Its desktop file is
`applications/com.gitturtle.GitTurtle.desktop`, named after the application ID,
so `gtk-launch com.gitturtle.GitTurtle` also opens it. Installing over a bundle
from before 2026-10-26 removes that bundle's `com.gitturtle.desktop` entry and
icons, and `--rollback` restores them.
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
  libssl-dev libzstd-dev libvulkan-dev libdav1d-dev python3-pil
```

The build uses `libdav1d-dev` only for C ABI layout verification tests; the
packaged executable does not hard-link `libdav1d.so.7` (`scripts/package-linux.sh`
verifies `NEEDED` has no `libdav1d` entry and no `dav1d_*` symbol is defined or
statically imported). Leave every `SYSTEM_DEPS_*` variable unset:
`scripts/package-linux.sh` refuses to run while one is set.

Install Rust through [rustup](https://rustup.rs/) if needed. The checked-in
`rust-toolchain.toml` selects **Rust 1.100.0** with rustfmt and Clippy only for
this checkout; other projects' default toolchains are unchanged. The workspace
minimum is Rust 1.100. `Cargo.lock` pins the matching GPUI Kit 0.10.0 / GPUI 0.3.12
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

On an AArch64 machine, use `aarch64-unknown-linux-gnu` in these target paths.

Alternatively, `./scripts/package-linux.sh` checks native development packages
and builds the locked release itself. Both forms produce four outputs:
`dist/gitturtle-linux-x86_64/` (`dist/gitturtle-linux-aarch64/` on AArch64),
its `.tar.gz`, `.tar.gz.sha256` and `.tar.gz.manifest.json`. The tarball uses neutral numeric owner/group IDs.
Choose another output directory on subsequent runs, for example
`./scripts/package-linux.sh --no-build dist/ubuntu-test-2`; the packager refuses
existing outputs, including dangling symlinks. Payload preparation and archive
creation finish in a private staging directory before the final outputs appear.
A failed license/identity check leaves no completed bundle or archive.

Both modes use the packaging machine's explicit target path
(`x86_64-unknown-linux-gnu` or `aarch64-unknown-linux-gnu`), regardless of
other Cargo target settings. `--no-build` requires a release from the packaging checkout's current
commit and workspace version. To reuse a separately built artifact, pass its path
explicitly, for example `--no-build --binary target/release/gitturtle`.
The packager checks the ELF architecture and embedded source revision, version,
profile and target, then records the executable digest. It rejects unavailable or
mismatched compiled identity. `--expected-revision`, `--expected-version` and
`--expected-sha256` can pin the intended inputs explicitly:

```sh
./scripts/package-linux.sh --no-build \
  --expected-revision "$(git rev-parse HEAD)" \
  --expected-version 0.1.0 \
  --expected-sha256 "$(sha256sum target/x86_64-unknown-linux-gnu/release/gitturtle | cut -d ' ' -f 1)" \
  dist/verified-local-preview
```

Use the version of the intended checkout in place of `0.1.0` when it changes.
For `--no-build`, a modified packaging checkout or modified compiled source also
requires `--expected-sha256`: a commit ID alone cannot distinguish two local
builds with different uncommitted changes. Keep the expected digest from the
identified build; these checks do not authenticate a binary from an untrusted
sender. The bounded `--build-info` probe runs without a display or saved app state.

Packaging needs Cargo and the resolved locked sources to collect license texts,
including with `--no-build`. Local development bundles retain unresolved notices
in `licenses/REVIEW_REQUIRED.md` and record `distribution: development`.
Use `--distribution` for downloadable CI/release artifacts. It invokes the
collector's `--require-complete` gate and requires a clean release executable and
clean packaging checkout; unresolved upstream notices refuse distribution.
Reviewing the inventory alone does not clear a missing authoritative notice.

The bundle's format-2 `build-info.json` records compiled identity, packaging
revision/tree status, executable and lockfile hashes, license-inventory hash,
notice gaps, signing status (`unsigned` on Linux), distribution status and whether
the packager rebuilt the executable. `SHA256SUMS` covers this manifest, the shared
identity helper, installer and payload resources. The external format-1
`.tar.gz.manifest.json` adds archive name, size and SHA-256 plus the package-manifest
hash and contents. Before installation, the installer checks the bundled
manifest against the executable and license inventory; the external manifest
records archive identity for download verification. It is not a signature.

Current executables embed their own source revision, source-tree state, target,
build profile, compiler version and build time. These remain available after the
source checkout moves. `unknown` identifies a source archive without Git metadata
and cannot satisfy the verified package contract. `modified` records local source
changes and needs the executable SHA-256 in evidence; it is not a reproducible
commit identity.

### Validate a package change

Run focused source fixtures before packaging:

```sh
python3 -m unittest discover -s scripts/ci/tests -p 'test_package_*.py'
python3 scripts/test-install-linux.py
bash -n scripts/package-linux.sh
```

The source fixtures use synthetic payloads/tool doubles and skip real-bundle
checks unless supplied a bundle. The packager fixtures run the real `readelf`
from binutils: without it they skip on a developer host, and they fail where
`CI` is set, so a hosted job never skips them. They establish refusal and
recovery behavior, not a native/package acceptance result. For that result,
verify the produced archive checksum, extract into a new directory, and pass
the extracted bundle to:

```sh
python3 scripts/test-install-linux.py --bundle /absolute/path/to/extracted/gitturtle-linux-x86_64
```

This uses disposable homes to check actual payload installation, corruption,
upgrade and rollback after removing the extracted bundle. Record hashes for the
archive, compiled executable and installed executable, then perform a real smoke
launch from the installed path. Identify the actual desktop/session used; the
[desktop checklist](#ubuntu-desktop-acceptance-checklist) still applies.

The packaged executable embeds UI assets. Build on the oldest supported target
(Ubuntu 24.04 here) rather than transferring a binary built against newer glibc.
No local link workaround is needed with the development packages installed:
`libxkbcommon-x11-dev` supplies the linker file `libxkbcommon-x11.so`; the
runtime package supplies the versioned `libxkbcommon-x11.so.0`. A manually made
symlink in `target/` is not part of the supported installation.

## Arch Linux and Omarchy

Checked on 2026-09-27 on Omarchy 4.0.4 (Arch; Hyprland 0.56.2 on native
Wayland; AMD Radeon Vega with Mesa 26.2.2 and `vulkan-radeon`; one 1366 × 768
display at scale 1). Other Arch-based desktops share the build requirements but
have no recorded check. On Omarchy itself, Git writes and network actions were
checked on 2026-09-30, on a headless 1480 × 800 output: staging a file and a single hunk, unstaging, committing,
creating, switching and deleting a branch, and fetch, pull and push over
`file://` and over HTTP with GitTurtle's authentication prompt; see the
[Git writes record](validation/2026-09.md#september-30-git-writes-and-network-actions-on-omarchy).
More than one monitor and a physical HiDPI panel have no recorded check yet; see
the [validation record](validation/2026-09.md#september-27-omarchy-and-hyprland).
Omarchy 4.5 on x86-64 was checked on 2026-11-17 with the CI-built archive, on
an AMD laptop at 1,366 × 768: installation, the `gitturtle-bin` package build,
a first launch, 23 stock themes live, the desktop font, tiled, floating and
half-width windows, scale 2, and by hand Super+C, Super+V and the folder
picker; see the
[x86-64 record](validation/2026-11.md#november-17-the-ci-built-x86-64-archive-passes-the-omarchy-check-on-an-x86-64-omarchy-45-laptop).
That panel takes no fractional scale, so 1.25 and 1.5 have no record on
Omarchy 4.5 for x86-64.
[Omarchy on Apple Silicon](#omarchy-on-apple-silicon-aarch64) says what has
run on Omarchy 4.5 there, and
[Check a build on an Omarchy machine](#check-a-build-on-an-omarchy-machine)
is the one command that makes that check.

Under XWayland (launched with `env -u WAYLAND_DISPLAY`), History, Compare,
Settings with the Omarchy theme, the command palette, shortcuts and typing
behave as on native Wayland, and Copy source pastes unchanged, though the
clipboard offers different type names. Pointer hover was not checked there. The
difference to expect is size. With Omarchy's `xwayland:force_zero_scaling` on,
Hyprland does not scale an X11 window, and GitTurtle takes its scale from
`GPUI_X11_SCALE_FACTOR`, then `Xft.dpi`, then the monitor's physical size. On a
1366 × 768 panel that measures 310 × 170 mm, it drew about 17% larger than
native at scale 1, and about 7% smaller than native on an output at scale 1.25.
`GPUI_X11_SCALE_FACTOR` overrides that choice; see the
[XWayland record](validation/2026-09.md#september-29-gitturtle-under-xwayland-on-omarchy).

Omarchy already ships the native libraries, `clang`, `desktop-file-utils` and
both portal backends. Install the three missing build tools, then use the same
packaging script and installer as Ubuntu:

```sh
sudo pacman -S --needed rustup python-pillow cmake
./scripts/package-linux.sh
python3 dist/gitturtle-linux-$(uname -m)/install.py
```

`rustup` needs no default toolchain: the checkout's `rust-toolchain.toml`
installs Rust 1.100.0 on the first build. On another Arch system also install
`base-devel clang pkgconf desktop-file-utils fontconfig wayland libxkbcommon-x11
libx11 libxcb openssl zstd vulkan-icd-loader`, the Vulkan driver for your GPU,
`xdg-desktop-portal` and a FileChooser backend, plus optional `dav1d` if you
want AVIF image previews (`libdav1d.so.7` is loaded dynamically inside the
confined preview helper, so GitTurtle starts cleanly without it). No font
package is needed for interface text, which uses the embedded IBM Plex Sans, or
for code text, which uses the embedded DejaVu Sans Mono unless **Use the
desktop's monospace font** is turned on in Settings, which follows Omarchy's
font (JetBrainsMono Nerd Font by default), and an open window changes with
`omarchy font set` as soon as it is applied (including newly installed
fontconfig faces, up to 8 faces / 16 MiB). A first launch on Omarchy turns it
on.

Omarchy theme live reload resolves `$XDG_STATE_HOME/omarchy/current` first and
falls back to `$XDG_CONFIG_HOME/omarchy/current` (Omarchy 3.x), watching the
parent `omarchy/` directory so atomic symlink swaps reattach without a restart.
Launch tool lookup also includes `$XDG_DATA_HOME/omarchy/bin`.

### Hyprland, single-instance handoff & optional Linux packages

- **Minimum window size & Hyprland tiling:** GitTurtle supports narrow tiled
  splits down to `460 × 480` px. If you want an optional floating/tiling
  Hyprland window rule, match on the Wayland `app_id`:

  ```ini
  windowrule = match:class ^com\.gitturtle\.GitTurtle$, float on
  ```

- **Single-instance handoff (`SingleMainWindow=true`):** Launching `gitturtle
  /path/to/repo` from a terminal or app launcher while GitTurtle is already
  running forwards the repository path and `XDG_ACTIVATION_TOKEN` over a
  `0600` user-only Unix socket (`$XDG_RUNTIME_DIR/gitturtle.sock`, validated
  with `SO_PEERCRED` and config-directory tag) and opens it in a new tab in the
  existing window. Set `GITTURTLE_NO_SINGLE_INSTANCE=1` to force an independent
  instance.
- **Linux clipboard, selection & IME:** Selecting non-masked text in inputs,
  editors, or split diffs populates Wayland/X11 `PRIMARY` selection on left
  mouse release, and middle-click pastes `PRIMARY` into editable inputs. Masked
  and password fields never write to `PRIMARY`, map masked candidate bounds to
  bullet offsets, set Wayland `zwp_text_input_v3` `ContentPurpose::Password` +
  `ContentHint::SensitiveData | ContentHint::HiddenText`, and suspend X11 XIM
  forwarding while active. Wayland `zwp_text_input_v3` buffers `PreeditString`,
  `CommitString`, and `DeleteSurroundingText` until `Done`, validates commit
  serials, clamps cursor rectangles to surface bounds, resets cleanly across
  direct editable-field focus switches, and stays disabled outside editable
  inputs (including read-only diffs). X11 XIM reuses a single negotiated input
  context, supports incremental `PreeditDraw`/`PreeditCaret`/`PreeditDone`
  updates and clamped fractional-scale `SpotLocation`, forwards keys only on
  focused editable non-password fields, and tracks held keycodes for key
  auto-repeat.
- **Packaging preparation (local/unpublished):** Alongside the portable `.tar.gz`
  bundle (`scripts/package-linux.sh`) and Arch AUR recipe
  (`packaging/aur/gitturtle-bin/`), the repository includes local Debian/Ubuntu
  `.deb` packaging (`scripts/package-deb.sh`) and Flatpak manifest preparation
  (`packaging/flatpak/com.gitturtle.GitTurtle.yml`). None are published yet.

**Menu → Open repository in editor** opens the repository in your Omarchy editor
(what `omarchy launch editor` opens; Neovim and other terminal editors run in your
terminal) unless Settings → Projects names another editor, and **Open repository
in terminal** opens your default terminal (`xdg-terminal-exec`, as Super+Return
does) in the repository.

**Settings** offers an **Omarchy** theme in a **Desktop** group above the other
themes, and a first launch on Omarchy (no saved preferences yet) selects it. It follows your current Omarchy theme, including each switch while it
is selected, and turns **Follow system appearance** off while in use. GitTurtle
reads `$XDG_STATE_HOME/omarchy/current/theme/colors.toml` (defaulting to
`~/.local/state/omarchy/current/…` and falling back to
`$XDG_CONFIG_HOME/omarchy/current/…` on Omarchy 3.x), the `theme.name` file
beside `theme/`, and whether the theme has a `light.mode` file. It reads them at
launch, whenever its window regains focus and when you choose the card, watches
both `current/` and its parent `omarchy/` directory while the Omarchy theme is
selected or Settings is open, and never writes under Omarchy's directories. Colors resolve the way Omarchy's own
`omarchy-theme-color` resolves them. That includes themes whose `colors.toml`
Omarchy generated from an `alacritty.toml`, which name only `color0` to
`color15`, and the older short names such as `bg` and `fg`. The card appears
only when that `colors.toml` exists. A file that cannot be used keeps the last
colors read (or Midnight): the card says it is unavailable and which colors it
keeps, and a line under it gives the reason, such as a missing color or a
permission problem. The [user guide](user-guide.md) describes the
theme.

Quality CI also builds the workspace and runs its tests in an `archlinux:base-devel`
container against current Arch packages; see
[Arch Linux container job](ci.md#arch-linux-container-job). That job has no display,
so it does not replace the desktop check above.

On Hyprland:

- The compositor supplies server-side decorations and draws no title bar, so
  GitTurtle shows no window buttons. Quit with **Ctrl+Q** or close the window
  with the compositor's binding.
- GitTurtle sets a `460 × 480` window minimum so half-width and tiled splits
  stay usable. Below 800 px History collapses its navigation to the rail and the
  header keeps only icons for Projects and the profile; the inspector stays
  whole down to about 600 px. Float the window (Omarchy: **Super+T**) or widen
  its split for the full layout.
- The window class and Wayland app ID are `com.gitturtle.GitTurtle`, for window
  rules and the launcher entry's `StartupWMClass`; bundles before 2026-10-26 used
  `com.gitturtle.desktop`. GitTurtle keeps Omarchy's default window opacity
  (0.985 focused, 0.96 unfocused), as terminals and editors do. For a fully
  opaque window, add this line below the personal configuration comment in
  `~/.config/hypr/hyprland.lua`:
  `o.window("^com\\.gitturtle\\.GitTurtle$", { tag = "-default-opacity", opacity = "1 1" })`.
- `xdg-desktop-portal-hyprland` has no file chooser; the repository picker is
  `xdg-desktop-portal-gtk`'s dialog, which Omarchy's portal configuration
  selects. The first open after login took about four seconds while that
  backend started.

### Check a build on an Omarchy machine

[`scripts/omarchy-verify.sh`](../scripts/omarchy-verify.sh) checks a CI-built
archive on an Omarchy 4.5 machine, x86-64 or AArch64, in one run, and packs
what it found as one file to send back. It runs from a source checkout and
needs nothing that a stock Omarchy lacks.

Download the machine's archive from the green Quality run on `main` for the
commit being released, never from a pull request's run
([CI package downloads](ci-artifacts.md#find-and-verify-a-download)), into one
folder with its `.sha256`, and with its `.manifest.json` and
`ci-provenance.json` when you have them. The checksum beside an archive shows
that the download is whole, not who built it: the trusted value is that
commit's SHA, taken from the run's page and given to `--expect-revision`. Then
run the script from a directory outside your home: GitTurtle's header shows
the path of the demo repository the run makes, and the pictures should not
carry your name.

```sh
cd /var/tmp
/path/to/GitTurtle/scripts/omarchy-verify.sh \
  ~/Downloads/GitTurtle-<version>-<commit>-linux-$(uname -m)-release.tar.gz \
  --expect-revision <the commit's SHA>
```

It prints what it is about to do and waits for Enter. First it changes
nothing: it records the host, verifies the archive (its `.sha256`, the
`SHA256SUMS` inside, its source revision, `--build-info`, `ldd`, the
manifest), installs it with the archive's own `install.py` into a disposable
home, and builds the [`gitturtle-bin`](../packaging/aur/README.md) package
from it. The source revision is read from `build-info.json` in the archive
before anything from the archive runs; when `--expect-revision`, the manifest
or `ci-provenance.json` names another one, the executable, the installer, the
package build and the whole desktop part are skipped as "identity not
established". Then comes the desktop part, in which the keyboard and pointer
are left alone. It takes about three minutes on the machine it was first run
on, the Apple Silicon laptop described below with Omarchy 4.5's 24 themes:
about five seconds a theme, of which the switch itself took 0.3 to 1.8
seconds, and under a minute and a half for everything else. The plan gives the
sum for the machine's own themes, and the run times every switch and writes it
in the report. The script moves to a workspace that holds no window, opens the
installed build there on a demo repository and takes a picture of GitTurtle's
own window, never of the screen, at each step: the first launch, every theme
`omarchy-theme-list` names, one other font, the window floating, its monitor
at the scales 1, 1.25, 1.5 and 2 that its mode allows, the window inactive
beside a terminal the script opens, and 30 seconds at rest before it is
closed. Floating, the window is given 1,480 × 973 logical pixels, or three
quarters of the monitor's usable area where that does not fit, centred in that
area, which is the monitor less the edges it reserves for Omarchy's bar or a
dock; the report records what Hyprland gave. A picture is refused, and its
check fails, while another window or another monitor overlaps the window's
rectangle, while the rectangle leaves its monitor or reaches into one of those
reserved edges, where a bar is drawn over any window, or while Omarchy's shell
does not confirm that notifications are silenced. Where Omarchy's rules make
the window translucent, the wallpaper shows through it faintly; the report
gives the opacity Hyprland reports beside each picture.

Only code text follows `omarchy-font-set`, and GitTurtle opens on History,
which shows none. So the font check first sends three keys to GitTurtle's own
window, named by its address and only while it also has the focus: Down, Down
and Return. Down twice selects the third commit from the top, whose one
changed file is a text file, and Return opens that file's comparison. They are
the app's own list keys and they only read: the report says that the demo
repository and the settings GitTurtle saved are as they were before the keys.
The check then is honest about what it saw. It takes a picture before the
keys and one after them, and where the two files are the same it says that
the comparison was not reached. The picture after the keys is the one before
the font change; another is taken after the change and a last one after the
starting font is back, and the files are compared by size and checksum. The
check ends `ok` only when the picture changed with the font, and `partial:
the picture did not change with the font`, with the view it believes it was
on, when it did not; it also says whether the last picture is the same file
as the one before the change. The checks after the font check picture the
window on the view that check left, and each says which.

What it changes, it puts back when the step ends and on every exit, Ctrl+C
included, and also when its output is piped into a program that has ended: the
workspace shown, the theme and wallpaper, Omarchy's
`skip-<editor>-theme-changes` toggles, the monospace font and the terminal and
fontconfig files `omarchy-font-set` rewrites (through a symbolic link too: the
bytes of the file a link leads to are saved and put back, and the link stays a
link), the monitor's scale, the window's floating state with its size and
place, the focused window, and two settings it turns on for the run,
notification silencing and stay-awake. A file is put back by a copy beside it
and a rename, so it is never missing in between. If anything is not as it was found, the restore
check fails and the saved copies stay in `work/restore` under the results
directory, with a `manifest.txt` that says which copy belongs to which file;
the last lines of the run say where.

A kill the script cannot catch (`kill -9`) or a power loss leaves the desktop
as the step in progress had it. Before its first change the script prints
where the starting state is written (`records/desktop-start.txt`, and
`work/restore` with the saved copies) and how to put back by hand what that
run changes. The text has a step only for a change the plan lets the run make,
each step gives the value the run found, and a step is taken only if the
desktop differs from that value, because a step taken where nothing was
changed would itself change something. The steps come in the order the script
itself uses: the workspace (`SUPER` and its number); the theme
(`omarchy-theme-set`, only when the current theme is another one: setting the
current theme again moves on to its next wallpaper); the wallpaper
(`omarchy-theme-bg-set` with the picture's path, which `manifest.txt` keeps
and the packed records do not); the skip toggles the run made (remove the
files `manifest.txt` lists under `~/.local/state/omarchy/toggles`, and that
directory too when `manifest.txt` says the run made it); the font
(`omarchy-font-set`, only when the current font is another one: it rewrites
the font files and replaces a link at three of them whatever the font was) and
only then the font files, because `omarchy-font-set` rewrites them (copy each
saved `font-N` over the file `manifest.txt` names); the monitor's scale
(`hyprctl reload`); and, where the run found them off, notification silencing
(`omarchy-shell notifications setDnd off`) and stay-awake
(`omarchy-toggle-idle allow-idle`).

A theme switch is Omarchy's own `omarchy-theme-set`, and it does what Omarchy
does on every switch: it retints the terminals and the programs in them,
Hyprland's configuration, GTK's colour scheme and icons, the themes Omarchy
keeps for the coding agents it supports, T3 Code, Obsidian and fcitx5, the
keyboard backlight where Omarchy drives one, and the browsers' colour, a file under
`/etc` that Omarchy writes through its own passwordless sudo rule before it
tells a running browser to read it again. The script keeps a switch from doing
more. It turns Omarchy's skip toggles on for the run, so that no editor's
settings are rewritten and no extension is installed (VS Code, VS Code
Insiders, VSCodium and Cursor each have one in Omarchy 4.5). And it skips the
themes check, with the reason, where a switch would do more than that. Two
reasons stand whatever is asked, because a switch would reach the network or
raise a dialog: Omarchy's theme sync being on (each switch would push the
theme to your herdr machines over SSH; `omarchy-toggle-theme-sync` turns it
off), and Omarchy's sudo rule for the browser colour missing (each switch
would raise a password dialog).

Two more reasons are about what the script cannot vouch for, and the plan
names each one before Enter. The first is a `theme-set` hook under
`~/.config/omarchy/hooks` that is not byte for byte the one Omarchy packages
for its wallpaper daemon: the plan gives the hook's file name and says whether
it differs from the packaged `/usr/share/owe/10-owe-sync` (the installed hook
is a copy made when that package was installed, so it differs on a stock
machine once the package has been updated) or is not Omarchy's at all. The
second is a step of `omarchy-theme-set` that is not among the 19 read for
Omarchy 4.5.0, which a newer Omarchy may add. Read what the plan names, then
either move a hook aside for the run or pass `--themes-anyway`, which runs the
themes check despite those two reasons and nothing else: it never overrides
theme sync or a missing sudo rule, and it covers only what the plan named, not
a hook or step that appears afterwards. A `font-set` hook skips the font check
in the same way, and no option overrides that.

The plan is decided once, before Enter: a part it says will not run is not
started later in the run. It refuses the desktop part when Hyprland is not the
compositor, when the session is locked or Omarchy cannot tell whether it is,
on battery below 30 %, when the desktop is on a named workspace or shows a
special one (the run starts from a numbered workspace and returns to it), or
when the monitor has no workspace without a window. The desktop part also
ends at its start, before GitTurtle opens, when Omarchy's shell does not
confirm that notifications are silenced once the script has asked for that:
no picture could be taken, so silencing and the workspace are put back as they
were, nothing else on the desktop is changed, and every check of the desktop
part reports `skipped` with that reason. With the desktop part
asked for, a results directory under your home stops the run at the start,
before the archive is checked. It never runs as root and makes no network
request. Its one use of sudo is the question Omarchy's own command asks,
`sudo -n -l`, which elevates nothing, runs no command and never prompts, to
learn whether that sudo rule exists; the plan says when it was asked. Besides
the changes above and, with `--hands`, the clipboard, it writes only under its
results directory, and it leaves an
installed GitTurtle and its settings alone: the installer gets a disposable
home and the app a new `XDG_CONFIG_HOME`. It ends only processes it started:
the app it started is ended before another is started, and all of them are
ended on every exit.

What comes back is `gitturtle-omarchy-check-<UTC date and time>.tar.gz`, which
the last line names after "send this file back": `report.txt`, with one
section per check, the records it quotes, and the pictures. A section ends in
`RESULT <check> ok`, `partial: <what was not covered and why>` when the check
ran but could not cover a part of it (a scale the monitor's mode does not
allow, a manifest that was not downloaded, a font change that the picture did
not show), `FAILED: <why>` or
`skipped: <why>`. They name no host, no user and no home directory, and the
file is packed only after every record was read as text and none of them does.
The exit status is 0 when no check failed and the file was packed, 1 when a
check failed or nothing was packed, and 2 when the script refused to start.

`--no-desktop` runs only the part that changes nothing. `--skip LIST` leaves
out the checks it names, separated by commas, from `package`, `themes`,
`font`, `floating`, `scales`, `inactive`, `rest` and `hands`: each reports
`skipped: by --skip` and changes nothing, the plan shows what will run, and
any other name stops the run before anything runs (exit 2). It is for a run in
stages, or a second run of only what failed. `--hands` adds
four prompts at the end for Omarchy's copy and paste bindings and the folder
picker; that part runs beside your terminal, on the workspace you started
from. It replaces what your clipboard holds and does not put it back: the
script first puts a line of its own there, offered as sensitive so that
Omarchy's clipboard history leaves it out; the first prompt asks no question
because the clipboard answers it; the hash you copy is in that history like
any copy of yours; and a line still there at the end is cleared. Each prompt
takes a picture when you press Enter, except the third, which takes none
because the folder picker lists your folders, and the second, which is
pictured only while the clipboard holds a demo commit's hash or the script's
own line, as the last then is too, because the search field still shows the
paste; a prompt left unanswered, or without its picture, is reported as not
covered. `--help` has
the rest. The first x86-64 run is the
[November 17 record](validation/2026-11.md#november-17-the-ci-built-x86-64-archive-passes-the-omarchy-check-on-an-x86-64-omarchy-45-laptop).

### Omarchy on Apple Silicon (AArch64)

Since version 4.5, Omarchy supports Apple Silicon MacBooks natively;
[omarchy.org](https://omarchy.org/) lists the models and covers the
installation. The Omarchy steps above apply unchanged: the packager builds
`aarch64-unknown-linux-gnu` there and writes `dist/gitturtle-linux-aarch64/`.

GitTurtle's Linux work is done and checked on one: an Apple MacBook Pro
(16-inch, M2 Pro, 2023) that has run Omarchy 4.5 since 2026-11-08. It reports
Arch Linux ARM in `/etc/os-release`, and GitTurtle runs there as a native
Wayland client under Hyprland 0.56.2, with Mesa's Asahi Vulkan driver
(`vulkan-asahi`), on a 3456 × 2234 panel at scale 2.

On Omarchy 4.5, the AArch64 archive built by Quality's release job was
downloaded, verified, installed with its own `install.py` and run on this
MacBook on 2026-11-15: a first launch that took the Omarchy theme, History, a
commit from Changes, and PNG, AVIF and rendered Markdown previews. The
[validation record](validation.md) has that run under November 15.

The dated AArch64 records there were made on this MacBook before that, when it
ran Omarchy Mac 4.0.4 on Asahi Linux, and they name that system. The first, on
2026-10-25, covered the native release build, packaging, installation, launch
from the installed executable, History and image comparison, on a temporary
1480 × 800 headless output at scale 1; see the
[AArch64 record](validation/2026-10.md#october-25-linux-aarch64-on-omarchy-on-asahi-linux).
The system already had every native library and `clang`. That check installed
Rust and cmake through mise and Pillow into mise's Python instead of the pacman
packages; either works, as long as the `python3` on `PATH` can import Pillow.

That first check left open the folder picker, Git writes and network actions,
fractional scaling, the Omarchy theme and the command palette. Later records on
the same MacBook cover a
[first launch that matches the desktop](validation/2026-10.md#october-26-a-first-launch-on-omarchy-matches-the-desktop),
[`omarchy font set`](validation/2026-10.md#october-26-omarchy-font-set-reaches-code-text-at-once),
the [editor and terminal handoffs](validation/2026-10.md#october-26-omarchy-editor-default-terminal-and-a-launch-or-focus-binding),
[every stock Omarchy theme](validation/2026-10.md#october-27-every-stock-omarchy-theme-after-the-contrast-fixes)
in History, Compare, Changes, Settings, the command palette and the menu, and
[Stage all and Fetch](validation/2026-10.md#october-29-keyboard-menus-clear-the-status-bar-capped-menus-keep-whole-rows-and-fetch-says-when-nothing-is-new),
among many checks of single changes. The rest of that list (the folder picker,
pull and push, and fractional scaling) has no AArch64 record of its own.

Quality's `Rust release · ubuntu-24.04-arm` job builds, packages and checks the
AArch64 release bundle on a hosted Ubuntu 24.04 ARM64 runner and keeps the
archive as a seven-day artifact ([CI package downloads](ci-artifacts.md)). That
runner has no display, so the job is a build and package check, not a desktop
check. Its first hosted run passed on 2026-11-15, in
[quality run 43738958820](https://github.com/FernandoX7/GitTurtle/actions/runs/43738958820)
at `7782c581`. Other ARM hardware and distributions have no recorded check, and
a bundle built on AArch64 installs only on AArch64.

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
display backend/scale, theme and whether it follows the system appearance,
density, text sizes and code font choice, and Reduce motion and Increase
contrast as chosen and as in effect. It includes no
repository paths, source text, branch names, account details or credentials.
Paste this with reproduction steps and the observed/expected behavior in a
[GitHub issue](https://github.com/FernandoX7/GitTurtle/issues).

With no repository argument, the startup setting opens Projects or restores
the saved session. Application preferences, repository sessions and local
drafts live under `$XDG_CONFIG_HOME/gitturtle`, falling back to
`~/.config/gitturtle`; repository data stays in the repository. Back up that
configuration directory before destructive cleanup. Avoid concurrent app
instances sharing one configuration directory during testing.

The repository picker uses the session D-Bus service `xdg-desktop-portal` and
its FileChooser backend. Cancel leaves the current context intact. A failed
request shows the recovery guidance first and the underlying error, shortened,
on a line after it; a command-line
repository path remains available when the picker cannot run. After installing
portal packages, log out and back in if the session has stale service state.

| Symptom | Check / recovery |
| --- | --- |
| Build reports missing `-lxkbcommon-x11` | Install `libxkbcommon-x11-dev`, then rebuild without custom `LIBRARY_PATH`. `pkg-config --libs xkbcommon-x11` must succeed. |
| Installer says a shared library is missing | Install the runtime list; inspect `ldd` on the trusted bundled executable. A clean `ldd` alone does not check dynamically loaded graphics drivers. |
| AVIF preview says `libdav1d` is unavailable | Install `libdav1d7` (Ubuntu) or `dav1d` (Arch). GitTurtle starts cleanly without `libdav1d` and loads `libdav1d.so.7` dynamically inside the confined preview helper only for AVIF images. |
| Installer says GitTurtle is still running | Let its Git operation finish, quit the app normally, then rerun the installer. The message lists matching process IDs; the installer never terminates them. |
| Upgrade causes a regression | Quit the app, run the retained installer's `--rollback` command above, then include build diagnostics for both versions in your report. |
| No window when launched over SSH, from a console, or in CI | A normal launch needs a graphical session. GitTurtle exits with a specific no-display message; do not invent display variables or set `ZED_HEADLESS` to claim a desktop test. |
| Window/graphics initialization fails | Launch from a desktop terminal to capture stderr; check `vulkaninfo --summary` (`vulkan-tools`) and your GPU driver. Try `WGPU_BACKEND=gl` on Mesa/OpenGL or check Vulkan ICDs; if a Wayland connection fails with `DISPLAY` set, GitTurtle falls back to X11 automatically, and hardware GPU loss retries up to 3 times before falling back to software rendering. |
| Picker does nothing / reports a portal failure | Check `systemctl --user status xdg-desktop-portal xdg-desktop-portal-gnome`; inspect `journalctl --user -b -u xdg-desktop-portal`. Check the matching backend, then reopen the app. |
| Blank launcher icon / menu entry absent | Rerun the installer and `desktop-file-validate` on the installed entry; confirm the entry's absolute `Icon` path exists. Refresh the app menu or log out/in if its cache remains stale. |
| Missing or cramped text | Interface text uses the IBM Plex Sans embedded in the executable, in its own regular, medium, semibold and bold faces. Characters it lacks, such as CJK and emoji, come from installed fonts: check `fc-match sans-serif` and install `fonts-dejavu-core` above (or a CJK family) if no sans family resolves. Code text uses the DejaVu Sans Mono embedded in the executable, so it needs no font package. With **Use the desktop's monospace font** on, code uses `fc-match monospace`'s family (loading newly installed faces live, up to 8 faces / 16 MiB) when it is monospace; otherwise the setting shows why and keeps the embedded font. Settings has separate interface/code text sizes. |
| Text smaller than in other apps | On Wayland, GitTurtle multiplies its text sizes by the desktop text scaling factor (GNOME Settings › Accessibility › Large Text, `org.gnome.desktop.interface text-scaling-factor`, falling back to KDE `kcmfonts` `forceFontDPI`). On X11 the toolkit scales the whole window through `Xft.dpi`, KDE `kcmfonts`, or RandR scaling instead. The app's own interface/code sizes apply on top. |
| Colored fringes or soft text, typically on an OLED or rotated panel | GitTurtle follows the desktop antialiasing preference. GNOME's default `font-rendering` "automatic" renders grayscale like GTK 4; "manual" follows `font-antialiasing` (Tweaks › Fonts), where `rgba` selects subpixel rendering. When no portal answers, or its GNOME keys are missing or invalid, GitTurtle uses fontconfig's resolved defaults, distribution defaults included: `fc-match --format '%{antialias}\|%{rgba}\n' sans-serif`. Grayscale is the safe choice; changes apply without a restart. |
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

- **The GitHub account lives in your desktop keyring.** **Connect GitHub CLI
  account** stores GitTurtle's GitHub authorization in your default desktop
  keyring (gnome-keyring on Omarchy) through the Secret Service, and nowhere
  else: GitTurtle writes no credential file and has no fallback, so with no
  keyring installed or able to start (D-Bus starts an installed gnome-keyring
  when it is asked for), Connect fails and says so. Refresh PRs and opening a
  pull request read it without any prompt; a locked keyring is never unlocked
  by them, and GitTurtle says it is locked. Only Connect and **Disconnect
  GitTurtle** may show the keyring's own prompt. Disconnect removes it from
  your default keyring; if you changed which keyring is the default since you
  connected, the item stays in the old one, where Seahorse shows it.
  [GitHub collaboration](github-collaboration.md#linux-desktop-keyring) has
  the item's attributes and the bounds.
- **A keyring without a password is not protected on disk.** Omarchy creates
  its Default keyring with no password, and gnome-keyring keeps such a keyring
  unencrypted in `~/.local/share/keyrings`, readable by anything that runs as
  you. To protect it, install Seahorse (`sudo pacman -S seahorse`), open
  **Passwords and Keys**, choose **Change Password** on the Default keyring and
  set one. A keyring with a password that your login does not unlock stays
  locked until you unlock it.
- **GitHub CLI discovery.** GitTurtle runs GitHub CLI from `/usr/local/bin/gh`,
  `/usr/bin/gh`, `/run/current-system/sw/bin/gh` (NixOS), or `$HOME/.local/bin/gh`
  (`sudo pacman -S github-cli` on Arch and Omarchy); the first verified path wins.
  Connect/Disconnect passes the active Wayland/X11 parent `WindowIdentifier` to
  the Secret Service portal so unlock prompts attach to GitTurtle's window, and
  shows a calm one-time dismissible notice if a passwordless
  `Default_keyring.keyring` file is detected.
- Live GitHub sign-in, disconnect/reconnect via the desktop keyring with zero
  token leaks, and end-to-end PR creation were verified on Omarchy 4.5
  ([November 16 check](validation/2026-11.md#november-16-a-live-github-sign-in-writes-no-token-and-the-app-creates-a-pull-request-end-to-end));
  live commenting, replying, and review submission remain covered by tests and
  the offline fixture. Git's own configured authentication for explicit
  fetch/pull/push is separate.
- PDF pages and their text, AVIF images, JPEG 2000 images (JP2 files and
  raw J2K codestreams), and native C/C++ 3D model decoders (`.obj`/`.fbx` via
  `ufbx` and GLB `EXT_meshopt_compression` views via `meshopt`) run on x86-64
  and AArch64 in GitTurtle's own helper process: the app starts its own
  executable per request, the helper preloads optional `libdav1d.so.7` before
  confining itself (resource limits, one thread, `no_new_privs` and a seccomp
  filter that allows no file, network or process calls) and decodes with
  [hayro](https://github.com/LaurenzV/hayro) for PDF, its `hayro-jpeg2000` for
  JPEG 2000, `mp4parse` with optional system `libdav1d` for AVIF, and
  `ufbx`/`meshopt` for 3D native geometry, and the app kills it after 10
  seconds, on Back, another file or page, Retry page or a tab switch, which it
  checks for every 25 ms, and when the app exits. Helpers share at most two
  slots at a time, and an image comes back at most 1,600 pixels on its longer
  edge. **It needs a kernel with seccomp filters** (`CONFIG_SECCOMP_FILTER`, on
  in every mainstream distribution kernel, including Omarchy's on Apple
  Silicon): without them the helper refuses to start, every page says “The PDF
  renderer could not start.”, every AVIF or JPEG 2000 image says “The image
  decoder could not start.”, and `.obj`/`.fbx`/meshopt-GLB previews fail closed
  without running native decoders in-process. A running helper is named
  `gitturtle-pdf` across pages, images, and 3D jobs. Other architectures keep
  PDF metadata and an unavailable result, and decode neither AVIF nor JPEG
  2000. The confinement's tests ran on AArch64 (the Apple Silicon MacBook
  above) and on x86-64 in hosted runs: for PDF pages in
  [quality run 41480657885](https://github.com/FernandoX7/GitTurtle/actions/runs/41480657885)
  ("Rust tests and Clippy · ubuntu-24.04", all 21 helper tests at `66add2e6`),
  and for images in
  [quality run 41757534359](https://github.com/FernandoX7/GitTurtle/actions/runs/41757534359)
  (the Ubuntu 24.04 tests job and the Arch container, the 17 tests of
  `tests/image_helper.rs` at `36d977aa`; see the
  [record](validation/2026-11.md#november-3-night-the-linux-avif-and-jpeg-2000-view-passes-its-x86-64-run-and-a-slow-macos-runner-reaches-the-jobs-time-limit)).
- **libdav1d is an optional runtime dependency.** The executable does not
  hard-link `libdav1d.so.7`, so GitTurtle starts cleanly even when `dav1d` /
  `libdav1d7` is not installed. When decoding an AVIF image, the confined
  helper loads the distribution's `libdav1d.so.7` via `dlopen` before enabling
  seccomp and checks **libdav1d 1.4.1 or later** (API major 7): when absent or
  older it shows “AVIF decoding needs libdav1d 1.4.1 or later” in place of the
  image. JPEG 2000 does not use `libdav1d`. Ubuntu 24.04's `libdav1d7` is 1.4.1. The floor cannot tell
  whether your distribution has backported a fix; `libdav1d` is patched at its
  pace.
- AVIF shows its first image only: an animated AVIF shows its first frame,
  captioned “first frame of an animation”, and does not play. AVIF image grids,
  as some cameras and phones write, are not decoded yet: they say “AVIF image
  grids are not decoded on Linux yet” and offer **Open in default app**. A
  crop (`clap`) is not applied, HDR is not tone-mapped, and an ICC profile
  over 4 MiB, malformed or not RGB is not applied; the caption says so
  (“crop not applied”, “HDR not tone-mapped”, “color profile not applied”).
  A JPX file decodes only when it is JP2-compatible.
- HEIF and HEIC images are not decoded on Linux: they say “HEIF / HEIC images
  are not decoded on Linux” and offer **Open in default app**. macOS decodes
  them through ImageIO. PNG/JPEG/GIF/WebP, supported SVG, rendered Markdown
  and finite mesh/GLB/STEP previews have portable decoders, and AVIF and
  JPEG 2000 decode in the helper above; the [format matrix](file-previews.md)
  defines bounds and unsupported variants. Recognizing metadata is not the
  same as rendering a format.
- System preview is **Open in default app** on Linux: an explicit click hands a
  private read-only copy of the captured bytes to the desktop's default
  application through the OpenURI portal, or `xdg-open` where no portal
  answers, once GIO (and, for `xdg-open`, `xdg-mime`) types the copy as
  what its content is. Only passive types decided from content offer it:
  raster images and PDF, and plain text where a view presents a file's
  captured text as a document, which is the Diagrams view of a Mermaid file
  or of Markdown with Mermaid diagrams and a text 3D model's header (OBJ,
  ASCII STL, STEP); a source diff, the source tabs and Markdown's Rendered
  view offer nothing. Audio and video, office documents, SVG, HTML,
  scripts, desktop entries, executables, archives, fonts and unrecognized
  bytes do not. See the
  [handoff contract](file-previews.md#background-work-and-resource-lifetime).
- Automatic refresh skips ignored working-tree directories while preserving
  coverage for tracked files inside them and essential Git metadata. Linux
  coverage is bounded to 16,384 directory registrations and a scan budget;
  reaching a limit keeps existing watches and shows a quiet status with
  Refresh and copyable Details. Ignore-rule edits reconsider affected coverage,
  including shared and global excludes. Refresh or returning focus retries
  degraded coverage. See the [local refresh contract](../crates/app/docs/navigation-and-refresh.md#local-refresh).
- Dark-desktop launch, accent color, reduced motion, and KDE/non-GNOME parity:
  on startup GitTurtle waits concurrently (bounded 250 ms) for the initial XDG
  portal appearance before opening the main window so dark desktops do not
  flash light; when no portal answers, it falls back to `GTK_THEME`, GTK 4/3
  `settings.ini`, and KDE `kdeglobals` for dark mode, plus `kwinrc` for CSD
  window-button layout. When the settings portal publishes
  `org.freedesktop.appearance accent-color`, Settings offers **Use desktop
  accent color** (fitted through OKLab/WCAG contrast checks). Reduced motion
  and increased contrast follow XDG appearance `reduced-motion`, GNOME's
  `reduced-motion`/`enable-animations`, Hyprland's `animations:enabled` IPC
  option (`omarchy toggle animations`), appearance `contrast`, and GNOME's
  `high-contrast`. Text scaling follows the portal's `text-scaling-factor` on
  Wayland and falls back to KDE `kcmfonts` `forceFontDPI` when portal/X11 DPI
  is absent. Linux screen-reader and live desktop IME coverage still require
  interactive target-desktop verification.
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
  Wayland, within 0.5–3.0; nonpositive or nonfinite values use 1.0, falling
  back to KDE `kcmfonts` `forceFontDPI` (`dpi / 96.0`) when the portal does not
  report a scale. The toolkit's X11 backend reads `Xft.dpi`, KDE `kcmfonts`
  `forceFontDPI`, or RandR scaling, so GitTurtle does not apply the factor a
  second time on X11. Fractional scaling and live monitor changes still need
  checks on the target desktop.
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
- [ ] Select a commit (stays in History), activate text/PNG/AVIF/JPEG 2000/Markdown files, Back,
  then rapid activation/Back; inspect image zoom/overlay and a supported model.
- [ ] In Working Changes, stage and unstage fixture files/hunks. Verify with
  `git diff` and `git diff --cached`; ensure unrelated fixture edits survive.
  Enter a draft, navigate away, quit and reopen, then verify exact draft text.

To uninstall the user-local app, remove `~/.local/bin/gitturtle`, the installed
`applications/com.gitturtle.GitTurtle.desktop`, `icons/com.gitturtle.GitTurtle.png`
and the eight `icons/hicolor/SIZExSIZE/apps/com.gitturtle.GitTurtle.png` files
and `gitturtle/licenses/`, `gitturtle/build-info.json`, `gitturtle/install.py`,
`gitturtle/install.lock`, `gitturtle/previous-installation.json` and any unwanted
`gitturtle/install-backups/` under the data directory used at installation. Leave other icons and the
configuration directory intact unless you explicitly want to discard state.
