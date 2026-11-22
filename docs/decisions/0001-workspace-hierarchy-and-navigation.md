# ADR 0001: Native GPUI Workspace Hierarchy and Navigation

- **Date:** 2026-09-07 (consolidated 2026-11-21)
- **Status:** Accepted

## Context
GitTurtle needed a fast, dense desktop workspace comparable in ergonomics to mature native Git clients without adopting Electron, webviews, or decorative dashboards that waste vertical space.

## Decision
- Render a native Rust/GPUI workspace with four full-height surfaces: History (left rail, virtualized commit graph/table, right changed-files inspector), Compare (full-width comparison with collapsed left rail and persistent file inspector), Working Changes (staged/unstaged groups + anchored commit composer), and grouped Projects/Settings pages.
- Selecting a commit loads metadata and changed files only; entering Compare requires explicit file activation. Returning via Back retains History state and never allows late previews to reopen Compare or construct hidden editors on Projects/Settings.
- Keep one primary return button in the upper-left slot (`Projects` at root History; Back arrow with destination name elsewhere).

## Consequences
- First useful frames never block on file previews, syntax highlighting, or rename detection.
- Navigation state transitions stay predictable and testable via `#[gpui::test]` without leaking hidden focus or memory across pages.
