# GitTurtle patch to gpui-pre-linux 0.3.4

This directory contains the exact crates.io `gpui-pre-linux` 0.3.4 source under Apache-2.0, the version the matching GPUI Kit 0.6.0 stack locks. `LICENSE-APACHE` is byte-for-byte preserved. The registry archive (`gpui-pre-linux-0.3.4.crate`) has SHA-256 `8fd1d912c039641f0236b1701b7871bfddfb4926ffd1915b3958b73d03177250`, the checksum the workspace lockfile recorded for it; the package metadata names Zed's `crates/gpui_linux` at `69164008341295ad481bb11c0334a712ca8c23e3` (`https://github.com/zed-industries/zed`). `Cargo.toml` is the published normalized manifest and `Cargo.toml.orig` the original one; dependency requirements are unchanged and the workspace lockfile pins their versions. The registry-only `.cargo-ok` and the package lockfile are omitted. Apart from this note, no file differs from the archive.

Cargo stops checking the archive checksum once the crate comes from a path, so compare the tree with the archive it came from when reviewing a change here:

```sh
mkdir -p .local/gpui-pre-linux-archive
tar xzf ~/.cargo/registry/cache/index.crates.io-*/gpui-pre-linux-0.3.4.crate -C .local/gpui-pre-linux-archive
diff -r --exclude=Cargo.lock --exclude=GITTURTLE-PATCH.md .local/gpui-pre-linux-archive/gpui-pre-linux-0.3.4 vendor/gpui-pre-linux
```

The crate is the Linux and FreeBSD platform backend; its dependencies are all under `cfg(any(target_os = "linux", target_os = "freebsd"))`, so the macOS build graph does not reach it.
