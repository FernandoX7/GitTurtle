# Commits and branches

Atomic Conventional Commits, one cohesive change with its code, tests, and documentation (`feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`). Stage explicit intended paths, inspect `git diff --cached`, preserve unrelated changes, never bypass hooks (`--no-verify`), and never amend or rewrite published history. PR titles use Conventional Commit subjects because merges are squashed.

Never mention AI tools or include `Co-Authored-By` AI trailers in commits, PR titles, PR bodies, or docs. Enable repository hooks once per clone with `git config core.hooksPath .githooks`.
