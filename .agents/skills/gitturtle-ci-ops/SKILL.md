---
name: gitturtle-ci-ops
description: Free-tier GitHub Actions CI operations for GitTurtle — $0 budget, 5 macOS slots, 10 GB cache headroom, change routing, zero retries, and hosted run diagnostics. Use when changing workflows, CI scripts, caches, dependencies, or investigating hosted CI failures.
---

# Free-tier CI operations

GitTurtle runs strictly on **$0/month** GitHub public-repo runners: 4 vCPU Linux (`ubuntu-24.04`, `ubuntu-24.04-arm`, `archlinux` container), 3 vCPU macOS (`macos-15`, `macos-26`), **max 5 concurrent macOS jobs**, and **10 GB total cache storage**.

## Non-negotiable CI rules

1. **Full test suite before `main`:** Every product PR runs the full workspace test suite on Ubuntu x64 (2 shards), Arch Linux (2 shards), and macOS 15 (3 shards) with shard test-count reconciliation, plus Clippy (Ubuntu + macOS 15), Release builds (Ubuntu x64, Ubuntu ARM64, macOS 26), and package transfer verification.
2. **Zero retries, zero wider timeouts, zero skipped tests:** Never rerun a real failure away, widen a timeout to hide a slowdown, or skip a flaky test. Inspect the full job log (`gh run view --job=<id> --log`), root-cause the failure, and fix it. Only genuine third-party mirror/network outages (e.g., `apt-get` 503/DNS) justify a rerun.
3. **Serialize product PRs against `main`:** Open one macOS-heavy product PR at a time, wait for `Quality` to finish and squash-merge, and let `main`'s post-merge run clear macOS slots before launching the next product PR.
4. **Never open separate follow-up `docs(dev):` PRs:** Fold concise doc and record updates into the same atomic PR.

## Cache & slot discipline

- **10 GB cache cap:** Keep `[profile.dev.package."*"] debug = false` (with `debug = 1` on workspace crates) so two full cache generations fit inside 10 GB without LRU thrashing. Check usage with `gh cache list --limit 100`.
- **Cache key hygiene:** Pure doc/note edits under `vendor/` (`AGENTS.md`, `CLAUDE.md`, `GITTURTLE-PATCH.md`, `CHANGELOG.md`) are excluded from Rust cache hashing in `.github/actions/setup-rust/cache.py`. Batch `.github/actions/setup-rust/` edits into a single PR so caches only go cold once.
- **5-slot macOS budget:** Pure `crates/`/`vendor/` PRs run `product` only (3 `macos-15` test shards + 1 `macos-15` Clippy + 1 `macos-26` Release = 5 macOS jobs) without blocking on `agent-tooling · macos-15`.
- **Smart `push: main` routing:** Pure docs/tooling/website merges skip the product Rust rebuild on `push: main`; a weekly `schedule` trigger refreshes caches if `main` has no recent code push.
- **Note `workflow_dispatch` blind spot:** `Image privacy` has no PR diff on `workflow_dispatch`; PR runs exercise image-diff scanning.
- **Before pushing CI edits:** Always run `actionlint`, `python3 -m unittest discover -s scripts/ci/tests -t scripts/ci/tests -p 'test_*.py'`, and `python3 scripts/ci/tooling_suites.py run --directory /tmp/ci-check`.
