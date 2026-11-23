# AUR package: gitturtle-bin

`gitturtle-bin/` is the package for the [Arch User Repository](https://aur.archlinux.org/):
GitTurtle's release executable for `x86_64` and `aarch64`, repackaged from the
GitHub Release's Linux archive. It compiles nothing.

**The package is not published yet.** No GitHub Release exists, so the
`PKGBUILD` carries placeholders: `_commit` is forty zeros and both checksums are
`not-released`. `makepkg` cannot build it as checked in, because that archive
does not exist and `not-released` matches no file. Publishing is the owner's
step, after the first release; [the steps](#at-release-time) are below.

## What it installs

Everything comes from the archive's own files. `package()` downloads nothing and
there is no `.install` scriptlet.

| Installed path | From the archive |
| --- | --- |
| `/usr/bin/gitturtle` | `bin/gitturtle`, byte for byte (`options=(!strip)`) |
| `/usr/share/applications/com.gitturtle.GitTurtle.desktop` | Written by `package()`: the entry `install.py` writes, with `Exec=gitturtle %f` and `Icon=com.gitturtle.GitTurtle` in place of its per-user paths |
| `/usr/share/icons/hicolor/<size>x<size>/apps/com.gitturtle.GitTurtle.png` | `icons/hicolor/`, in the installer's sizes: 16, 24, 32, 48, 64, 128, 256 and 512 |
| `/usr/share/licenses/gitturtle-bin/` | The whole `licenses/` tree: GitTurtle's `LICENSE`, `THIRD_PARTY_NOTICES.md`, `dependencies.json` and every dependency's notices |
| `/usr/share/doc/gitturtle-bin/README.md` | `README.md`, the [Linux guide](../../docs/linux.md) |

The desktop entry and the icons are named after the application ID,
`com.gitturtle.GitTurtle`, which is also the window's Wayland app ID: a launcher
finds the icon through that name. It is what [`install.py`](../../scripts/install-linux.py)
installs for one user, system-wide. The installer's own files are left out
(`install.py`, `package_identity.py`, `build-info.json`, `SHA256SUMS` and the
1024-pixel `icons/app-icon.png`): pacman owns the installed files, and
`gitturtle --build-info` prints the build's identity. A per-user installation
made by `install.py` stays where it is, and its desktop entry under
`~/.local/share` takes precedence over the package's in the launcher.

## Dependencies

`depends` is derived from the release executable, not from a guess:

| Package | Why |
| --- | --- |
| `glibc`, `libgcc`, `libxcb`, `libxkbcommon`, `libxkbcommon-x11` | The owners (`pacman -Qo`) of the executable's `NEEDED` libraries (`readelf -d`): `libc.so.6`, `libm.so.6` and the loader, `libgcc_s.so.1`, `libxcb.so.1`, `libxkbcommon.so.0`, `libxkbcommon-x11.so.0` (`libdav1d.so.7` is loaded dynamically via `dlopen` in the confined helper, so `dav1d` is an optional dependency for AVIF previews) |
| `wayland`, `vulkan-icd-loader`, `libglvnd` | The owners of the libraries the executable loads by name at run time, which `NEEDED` does not list: `libwayland-client.so.0` and `libwayland-egl.so.1`, `libvulkan.so.1`, `libEGL.so.1` |
| `vulkan-driver` | The Vulkan loader draws nothing without a driver; each driver package provides this name |
| `git` | Run for every repository |
| `hicolor-icon-theme` | Owns the icon theme the icons are installed into |

`optdepends` holds what one feature needs, each with its reason: `dav1d` (AVIF
image previews via dynamic `dlopen`), `git-lfs`, `github-cli`, a Secret Service
keyring, `xdg-desktop-portal` and a file chooser backend, `xdg-utils`, `glib2`,
`fontconfig`, `openssh` and `xdg-terminal-exec`.
When a release's executable links or loads something else, change `depends` with it.

## Test it locally

On Arch (either architecture), with `makepkg` and the packaging prerequisites
of the [Linux guide](../../docs/linux.md). From a clean, committed checkout,
build the release executable and package it under the release's asset name:

```sh
target="$(uname -m)-unknown-linux-gnu"
cargo build --locked --release -p gitturtle --target "$target"
stem="GitTurtle-0.1.0-$(git rev-parse HEAD | cut -c1-12)-linux-$(uname -m)-release"
scripts/package-linux.sh --no-build --binary "target/$target/release/gitturtle" --distribution "dist/$stem"
packaging/aur/test-local.sh "dist/$stem.tar.gz"
```

`0.1.0` is the workspace version in `Cargo.toml`, which `pkgver` repeats; the
test refuses an archive of another version. That output directory gives the
archive the name a release's has, and the one directory inside it the same
name, which `package()` enters. An archive under another name works too, such
as `dist/gitturtle-linux-<arch>.tar.gz` from the packager's default output:
the test then adds a `prepare()` to its copy of the `PKGBUILD` that renames the
extracted directory, and `package()` runs as written.

Without a second argument the test works in a new directory under `/tmp` and
keeps it for inspection: remove it when done, or name a work directory as the
second argument.

[`test-local.sh`](test-local.sh) installs nothing, downloads nothing and never
runs as root. It copies the `PKGBUILD` to a work directory, points this
architecture's source at the local archive with its real checksum, runs
`makepkg` there, and checks the built package:

- `.SRCINFO` is what `makepkg --printsrcinfo` prints, the source URL names this
  build's archive in the `v<version>` release, and `makepkg` refuses the
  placeholder checksum while one is checked in;
- the file list (`bsdtar -tf`) is exactly the one above, with the notices and
  icons the archive lists, modes 755 and 644, and owner root;
- the packaged executable is the archive's, and its `--build-info` reports the
  same source revision;
- `desktop-file-validate` passes;
- the archive's `install.py`, run with `HOME` under the work directory, writes
  the same desktop entry apart from `Exec`'s program and `Icon`, and the same
  icons, notices and executable.

It does not start the app: a launch from the installed package, and the
launcher's icon, are checked on a desktop. `namcap` runs when it is installed.
`python3 -B -m unittest discover -s scripts/ci/tests -p 'test_aur_package.py'`
compares the `PKGBUILD`, the `.SRCINFO` and the release's asset names without
`makepkg`, and runs with the CI helper tests.

## At release time

After the GitHub Release `v<version>` is published
([Releases](../../docs/releases.md)), in `gitturtle-bin/PKGBUILD` (the tag must be
`v<version>`: the release workflow also accepts a bare version as its tag, and
this package would not find that release's assets):

1. Set `pkgver` to the release's version, `pkgrel` to 1, and `_commit` to the
   release's full source commit, which the release notes give as `Source:`.
   The asset names carry its first 12 digits.
2. Set `sha256sums_x86_64` and `sha256sums_aarch64` from the release's
   `SHA256SUMS`: the lines of the two archives.
   `grep -E 'linux-(x86_64|aarch64)-release\.tar\.gz$' SHA256SUMS` prints them.
   Take them from that file rather than from `updpkgsums`, which would record
   whatever was downloaded. Never use `SKIP`.
3. Regenerate `.SRCINFO`, never by hand:
   `(cd packaging/aur/gitturtle-bin && makepkg --printsrcinfo > .SRCINFO)`.
4. Run the unit test above, then `packaging/aur/test-local.sh` on the release's
   downloaded archive, on a machine of each architecture you can. With real
   values in the `PKGBUILD`, the test fails unless `_commit` and the checksum
   are that archive's.
5. Commit the two files here. Then, in a clone of the AUR repository
   (`git clone ssh://aur@aur.archlinux.org/gitturtle-bin.git`; the first push
   creates the package), copy `PKGBUILD` and `.SRCINFO` to its root, commit them
   and push. The push is the owner's.

A later packaging-only change keeps `pkgver` and raises `pkgrel`.
