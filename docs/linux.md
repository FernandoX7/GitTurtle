# Build and install locally on Linux

GitTurtle uses the native GPUI Linux backend. Run it inside a Wayland or X11
desktop session with working GPU drivers. The repository picker requires the
desktop's `xdg-desktop-portal` service and a compatible FileChooser backend;
opening a repository by command-line path does not require the picker.

These instructions describe a source build and a user-local installation, not a
distribution package. Build-specific results belong in the [validation notes](validation.md).

## Build and run

On Ubuntu 24.04 or Pop!_OS 24.04, install the native build dependencies from the
[quality workflow](../.github/workflows/quality.yml), plus Git and desktop utilities:

```sh
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential clang cmake pkg-config git openssh-client \
  libfontconfig-dev libwayland-dev libxkbcommon-x11-dev libx11-xcb-dev \
  libssl-dev libzstd-dev libvulkan-dev xdg-utils desktop-file-utils
```

With `rustup` installed, run the following from the repository root. Selecting
the toolchain on each command leaves other Rust projects' defaults alone.

```sh
rustup toolchain install 1.98.0 --profile minimal --component rustfmt --component clippy
cargo +1.98.0 build --release --locked -p gitturtle
./target/release/gitturtle /path/to/repository
```

Omit the repository argument to open Projects or restore the saved session,
according to the startup setting. A build without a desktop session does not
verify window rendering or interaction. If the picker fails, check the desktop's
portal setup and try a repository path directly.

## Install for the current user

After a successful release build, close any running GitTurtle instance and run
these commands from the repository root. They install or replace this user's
GitTurtle executable, icon and application launcher; no root privileges are needed.

```sh
install -Dm755 target/release/gitturtle "$HOME/.local/bin/gitturtle"
install -Dm644 assets/app-icon.png \
  "$HOME/.local/share/icons/com.gitturtle.desktop.png"
mkdir -p "$HOME/.local/share/applications"
cat > "$HOME/.local/share/applications/com.gitturtle.desktop.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=GitTurtle
Comment=Browse Git history and manage local repositories
Exec="$HOME/.local/bin/gitturtle" %f
Icon=$HOME/.local/share/icons/com.gitturtle.desktop.png
Terminal=false
Categories=Development;RevisionControl;
StartupWMClass=com.gitturtle.desktop
EOF
desktop-file-validate "$HOME/.local/share/applications/com.gitturtle.desktop.desktop"
update-desktop-database "$HOME/.local/share/applications"
```

The existing PNG is 1024 × 1024. The launcher references it by absolute path:
the standard hicolor theme does not include a `1024x1024/apps` directory, so
installing it there with only a theme icon name can leave the launcher blank.
The desktop filename matches the application's `com.gitturtle.desktop` ID.
Open **GitTurtle** from the application launcher or run
`"$HOME/.local/bin/gitturtle" /path/to/repository`. The launcher uses an absolute
executable path, so it does not depend on `~/.local/bin` being in the desktop's
`PATH`. Rebuild and repeat the installation commands after source changes.

Preferences, repository sessions and local drafts live under
`$XDG_CONFIG_HOME/gitturtle`, or `~/.config/gitturtle` when that variable is unset.
Keyboard shortcuts use Control in place of macOS Command.

## Window controls and quitting

Close the window with its **×** control, or use **Ctrl+Q** to quit GitTurtle.
Linux desktops that supply window decorations retain their own title bar.
On desktops such as GNOME Wayland, the tab strip supplies the window controls
in the position and order configured by the desktop, including minimize and
maximize when enabled. Drag its blank space to move the window, double-click
to maximize or restore, and right-click for the window menu when supported.
Closing the final window uses the normal application shutdown and draft saver.

This follows GNOME's [header bar](https://developer.gnome.org/hig/patterns/containers/header-bars.html)
and [keyboard](https://developer.gnome.org/hig/reference/keyboard.html) conventions.
On macOS, GitTurtle retains the system traffic-light controls and
**GitTurtle → Quit GitTurtle / Cmd+Q** in the native application menu, following
Apple's [window guidance](https://developer.apple.com/design/human-interface-guidelines/windows).

## Platform limits

- GitHub account connection cannot persist credentials on Linux yet; the app
  refuses storage instead of writing plaintext credentials. This is separate
  from Git's configured authentication for explicit fetch, pull and push.
- Native PDF page rendering/text extraction and ImageIO-based AVIF, HEIC and
  JPEG 2000 previews require macOS. See the [preview support matrix](file-previews.md)
  for format-specific behavior.
- System Preview uses macOS Quick Look and has no Linux launcher yet.
- The app's automatic reading of OS reduced-motion, contrast and transparency
  accessibility preferences is currently macOS-only.

Compilation and automated tests alone do not establish native desktop coverage.
For a local smoke check, open a disposable repository, select a commit, activate
a file to enter Compare, return to History, and exercise text input, Control
shortcuts, an image preview and Settings. Use disposable fixtures for staging or
other Git writes; [create-demo-repo.py](../scripts/create-demo-repo.py) accepts
`--output` and refuses an existing nonempty destination.
