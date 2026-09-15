# Contributing to GitTurtle

Thanks for helping make GitTurtle useful. Small bug fixes, clearer docs, reproducible reports and thoughtful interface improvements are welcome. For a substantial feature, [open a feature request](https://github.com/FernandoX7/GitTurtle/issues/new?template=feature_request.yml) first so we can agree on its scope.

GitTurtle is a native Rust/GPUI Git client for macOS and Linux. Keep changes focused on local history inspection and everyday Git work, with explicit user actions for writes and network access. The [design](DESIGN.md), [architecture](docs/architecture.md) and [project agreements](AGENTS.md) explain the boundaries. Please follow the [code of conduct](CODE_OF_CONDUCT.md).

Use your preferred editor and AI tools, if any. The same architecture, commit and validation requirements apply to every contribution. The [agent workflow](docs/development/README.md) includes optional tooling for coordinated development.

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

Choose the affected test package or a focused test name while iterating. Before submitting Rust code or dependency changes, run the combined checks:

```sh
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
```

For packaging or performance work, also run `cargo build --release --locked -p gitturtle`. Measure the affected path before claiming a speed improvement. For UI changes, exercise the changed workflow in the real native app and include readable screenshots where useful; follow the [validation guidance](docs/validation.md#current-validation-guidance). Identify the source/build, OS and session used, and distinguish actual desktop checks from virtual displays. Report unavailable platform checks plainly.

Docs-only changes need link, command and diff checks. Artwork changes need verification of their consumers and derived resources; see [asset conventions](assets/icons/README.md). Neither requires unrelated Rust tests.

Development-controller changes use `python3 scripts/check-agent-guidance.py` and `python3 -m unittest discover -s scripts/agent_loop -t scripts -p 'test_*.py'`. Follow the [development workflow](docs/development/README.md) for task contracts, isolated runs and evidence; product checks apply when those changes also affect the client.

CI changes also use `python3 -m unittest discover -s scripts/ci/tests -p 'test_*.py'`
and pinned actionlint 1.7.12 on both Quality and Website workflows. The
[CI guide](docs/ci.md#local-verification) records the download checksum and commands.

## Commit cohesive changes

Always use atomic [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/): each commit contains one cohesive change and its necessary code, tests and documentation. Use the form `type(optional-scope): description`, with `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore` or `revert`. Mark an incompatible change with `!` after the type/scope or a `BREAKING CHANGE:` footer. For example, `fix(history): retain selection after refresh` should include the correction and its relevant regression coverage and documentation.

Stage explicit intended paths and inspect the staged diff before each commit. Preserve unrelated staged and working changes. Never amend or rewrite published history without authorization. In coordinated agent work, the coordinator owns the index and commits; the [unattended controller](docs/development/README.md) owns commits inside its isolated checkout, and its workers return file changes without committing.

## Send a pull request

Describe the problem, resulting behavior and checks you ran. Keep the change reviewable and update the closest documentation when behavior changes. Include any remaining limitations instead of claiming checks you could not run. The PR template is intentionally short; remove sections that do not apply.

Contributions use pull requests with the macOS and Ubuntu Quality checks passing
and review conversations resolved. CodeQL scans must complete without unresolved
high/critical security findings or code-scanning errors. Workflows from external contributors need
maintainer approval before running. We squash-merge changes using the PR title
and description, so always use a Conventional Commit PR title and write the
description for a reader of the permanent Git history.
Merged branches in this repository are deleted automatically; your fork and
local branches remain yours.

Quality runs through PR events, main pushes and manual dispatch; a branch push
does not duplicate its PR run. PR checks exercise GitHub's test merge commit.
The [CI routing and gate policy](docs/ci.md#events-and-required-results) selects
inexpensive checks for docs/site/tooling-only changes and full platform coverage
for product or uncertain inputs. The current required Rust check names mirror
the complete Quality gate during the documented protection migration. CodeQL
remains separately required. Do not change repository rules or bypass a failed
requirement merely to obtain a green PR.

Sanitize screenshots, logs and fixtures before posting: remove credentials, private remote URLs, personal paths, identities and proprietary repository content. For a suspected vulnerability, use [private security reporting](SECURITY.md) rather than an issue or public PR.
