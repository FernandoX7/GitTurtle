---
paths:
  - "crates/git-core/**"
---
# Git core conventions

The crate guide beside this code owns the operation routing and command policies; this rule adds what Claude needs to test and review here.

- Passive reads use the fixed-argument builders in `src/lib.rs` (`git_command`, `bounded_output`, `passive_status_command`) with `--end-of-options` and `--` before paths, no helpers, no filters, no lazy fetch and no optional locks. Explicit writes use `normal_command`, which keeps identity, hooks, signing, filters and credentials. Never reuse one builder for the other or add unsigned or hook-bypassing fallbacks.
- Pick the fixture from the guide's table: `cargo test --locked -p gitturtle-core --test <fixture> [filter]` for integration fixtures, `--lib [filter]` for module unit tests. Mutation checks use disposable repositories and local bare remotes, and assert HEAD, refs, index and working bytes, including the unrelated work that must survive.
- A nonzero exit, timeout, overflow or lost reply never proves nothing changed; preserve that uncertainty in errors and never replay automatically.
- On hosts with Git 2.43 one authentication test fails while passing in CI (`work::authentication::tests::controlled_git_preserves_configured_askpass_and_helper_interaction`); list it in `.local/gate/known-failures.txt` rather than changing the test, and treat any other failure as real.
