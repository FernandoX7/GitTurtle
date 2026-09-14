# Contributing to GitTurtle

Thanks for helping make GitTurtle useful. Small bug fixes, clearer docs, reproducible reports and thoughtful interface improvements are welcome. For a substantial feature, [open a feature request](https://github.com/FernandoX7/GitTurtle/issues/new?template=feature_request.yml) first so we can agree on its scope.

GitTurtle is a native Rust/GPUI Git client for macOS and Linux. Keep changes focused on local history inspection and everyday Git work, with explicit user actions for writes and network access. The [design](DESIGN.md), [architecture](docs/architecture.md) and [project agreements](AGENTS.md) explain the boundaries. Please follow the [code of conduct](CODE_OF_CONDUCT.md).

## Set up a checkout

Fork the repository, clone your fork, and create a branch for your change. Install Git and [rustup](https://rustup.rs/). `rust-toolchain.toml` selects Rust 1.98.0, rustfmt and Clippy; keep `Cargo.lock` in use.

- **macOS:** install Xcode 26 or later and its Metal toolchain. The [packaging guide](docs/user-guide.md#package-locally-on-macos) describes local ad-hoc-signed builds.
- **Linux:** follow the [Ubuntu 24.04 runtime and build prerequisites](docs/linux.md#build-from-source-on-ubuntu-2404). A desktop session and graphics drivers are required for native interaction; headless compilation is a separate check.

From the repository root:

```sh
rustup show active-toolchain
cargo check --locked -p gitturtle
cargo run --locked -p gitturtle -- /absolute/path/to/repository
```

Use a disposable repository when exercising writes. The fixture generator creates history, preview files and a sibling linked worktree, and refuses a nonempty destination:

```sh
python3 scripts/create-demo-repo.py --output /tmp/gitturtle-contribution-demo
cargo run --release --locked -p gitturtle -- /tmp/gitturtle-contribution-demo
```

Choose a fresh fixture path for later runs. Keep test repositories and generated build outputs out of your contribution.

## Find the right layer

| Area | Start here |
| --- | --- |
| Native UI, focus, state, scheduling and GitHub collaboration | [`crates/app`](crates/app/AGENTS.md) |
| Git reads, typed operations, paths and authentication | [`crates/git-core`](crates/git-core/AGENTS.md) |
| Bounded decoding of supplied preview bytes | [`crates/preview`](crates/preview/AGENTS.md) |
| Dependency patches | [`vendor`](vendor/AGENTS.md) |

Read the affected area's instructions before editing. Reuse existing workflows and command metadata. Repository reads and decoding belong off the UI thread; passive browsing must not change the repository or fetch objects. Preserve unrelated staged changes, files, drafts and repository context. Dependency or asset changes must retain their licenses and attribution.

## Validate your change

During Rust iteration, run formatting, the app check and the tests relevant to the behavior:

```sh
cargo fmt --all -- --check
cargo check --locked -p gitturtle
cargo test --locked -p gitturtle
cargo test --locked -p gitturtle-core
cargo test --locked -p gitturtle-preview
```

Choose the affected test package or a focused test name while iterating. Before submitting code or dependency changes, run the combined checks:

```sh
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
```

For packaging or performance work, also run `cargo build --release --locked -p gitturtle`. Measure the affected path before claiming a speed improvement. For UI changes, exercise the changed workflow in the real native app and include readable screenshots where useful; follow the [validation guidance](docs/validation.md#current-validation-guidance). Identify the source/build, OS and session used, and distinguish actual desktop checks from virtual displays. Report unavailable platform checks plainly.

Docs-only changes need link, command and diff checks. Artwork changes need verification of their consumers and derived resources; see [asset conventions](assets/icons/README.md). Neither requires unrelated Rust tests.

## Send a pull request

Describe the problem, resulting behavior and checks you ran. Keep the change reviewable and update the closest documentation when behavior changes. Include any remaining limitations instead of claiming checks you could not run. The PR template is intentionally short; remove sections that do not apply.

Contributions use pull requests with the macOS and Ubuntu Quality checks passing
and review conversations resolved. CodeQL scans must complete without unresolved
high/critical security findings or code-scanning errors. Workflows from external contributors need
maintainer approval before running. We squash-merge changes using the PR title
and description, so write those for a reader of the permanent Git history.
Merged branches in this repository are deleted automatically; your fork and
local branches remain yours.

Sanitize screenshots, logs and fixtures before posting: remove credentials, private remote URLs, personal paths, identities and proprietary repository content. For a suspected vulnerability, use [private security reporting](SECURITY.md) rather than an issue or public PR.
