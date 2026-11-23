# Flatpak Packaging Prep (`com.gitturtle.GitTurtle`)

This directory contains the prepared Flatpak manifest and AppStream metainfo for GitTurtle. **It is not published yet**; archive URLs and `sha256` entries hold `not-released` placeholders until `v0.1.0` release archives are published.

## Required live verification before publishing

Before submitting or publishing a Flatpak build, verify the following on a live Flatpak host (both `x86_64` and `aarch64` if targeting both):

1. **Nested seccomp inside Bubblewrap:** GitTurtle's Linux PDF, AVIF/JPEG 2000, and 3D `ufbx`/`meshopt` helper invokes `prctl(PR_SET_NO_NEW_PRIVS, 1)` and installs its own seccomp-bpf allow-list (`crates/app/src/pdf_helper.rs`). Confirm that `flatpak run com.gitturtle.GitTurtle` allows nested seccomp installation without `EPERM`/`SIGSYS` on PDF, AVIF, JP2, OBJ/FBX, and meshopt GLB previews.
2. **Wayland + X11 fallback & GPU:** Confirm hardware Vulkan and fallback OpenGL (`WGPU_BACKEND=gl`) rendering under `--socket=wayland`, `--socket=fallback-x11`, and `--device=dri`.
3. **Secret Service & Portals:** Confirm file chooser, Open in default app, desktop appearance/font portals, and `org.freedesktop.secrets` GitHub authorization work cleanly inside the sandbox.
4. **Runtime tool & codec availability:** `org.freedesktop.Platform//24.08` does not bundle `git`, `gh`, or `libdav1d.so.7`, and `finish-args` omits `--socket=ssh-auth` by default. Before publishing, bundle `git` (and optional `libdav1d.so.7` with its BSD-2-Clause notice, plus `--socket=ssh-auth` if SSH remotes are supported inside Flatpak) as Flatpak modules or extensions.
5. **Update release checksums:** Replace the `000000000000` commit stem and `not-released` SHA-256 placeholders in `com.gitturtle.GitTurtle.yml` with the exact published release `SHA256SUMS` values and validate `com.gitturtle.GitTurtle.metainfo.xml` with `appstreamcli validate --pedantic`.
