# GitTurtle patch to gpui-pre-wgpu 0.3.12

This directory contains the exact crates.io `gpui-pre-wgpu` 0.3.12 source under Apache-2.0, the version locked by the matching GPUI Kit 0.10.0 stack. `LICENSE-APACHE` is byte-for-byte preserved. The registry archive (`gpui-pre-wgpu-0.3.12.crate`) has SHA-256 `dc76608d385f6d0dcbad44504b1bf669dac3b7d7ffa2885acdb7c1c358caf852`. Dependency requirements are unchanged and the workspace lockfile pins their versions. The registry-only `.cargo-ok`, `.cargo-checksum.json`, and package lockfile are omitted. Apart from this note, `src/wgpu_renderer.rs` and `src/wgpu_context.rs` are the only files that differ from the archive.

## Changes

### 1. Software-renderer device-loss recovery (`src/wgpu_renderer.rs`)

Upstream `WgpuRenderer::recover` (`src/wgpu_renderer.rs`) unconditionally calls `WgpuContext::new_rejecting_software(...)` after dropping `self.state` to `RendererState::Released`. On virtual machines or older systems that start on a software Vulkan/OpenGL adapter (`llvmpipe` / `lavapipe`, `wgpu::DeviceType::Cpu`), or when a hardware GPU does not return after suspend/reset, recovery permanently fails.

`WgpuRenderer` now persists `was_software: bool` (captured from `context.adapter.get_info().device_type == wgpu::DeviceType::Cpu` when the renderer is initialized) and `hardware_recovery_attempts: u32`:

- Sessions that were already running on a software adapter recover via `WgpuContext::new(...)` immediately.
- Hardware-backed sessions still prefer hardware recovery via `WgpuContext::new_rejecting_software(...)` for `MAX_HARDWARE_RECOVERY_ATTEMPTS` (3) attempts so suspend/resume gives the hardware driver time to reinitialize, then fall back to `WgpuContext::new(...)` instead of looping indefinitely.

### 2. Honor `WGPU_BACKEND` within supported backends (`src/wgpu_context.rs`)

Upstream `WgpuContext::instance()` hardcodes `wgpu::Backends::VULKAN | wgpu::Backends::GL` without consulting `WGPU_BACKEND`, so users troubleshooting broken Vulkan ICDs cannot force Mesa OpenGL via `WGPU_BACKEND=gl`. `WgpuContext::instance()` now reads `wgpu::Backends::from_env()` intersected with `VULKAN | GL`, falling back to `VULKAN | GL` when unset or empty.

Note: Upstream `gpui-pre-wgpu`'s published crate includes `#[cfg(test)]` modules referencing unbundled Zed workspace fonts (`../../../assets/fonts/lilex/...`) and an undeclared `naga` dev-dependency, so standalone crate-test runs cannot compile upstream's tests; workspace builds and Clippy verify the patched library code directly.

## Removing the patch

Remove this patch, the `[patch.crates-io]` entry, the workspace exclusion, the `THIRD_PARTY_NOTICES.md` row, and the `vendor/AGENTS.md` row once upstream `gpui-pre-wgpu` allows software-backed sessions (and bounded hardware fallback) to recover from device loss and respects `WGPU_BACKEND`.
