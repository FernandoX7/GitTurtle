---
paths:
  - "crates/app/src/preferences*.rs"
  - "crates/app/src/profiles*.rs"
  - "crates/app/src/profiles/**"
  - "crates/app/src/projects*.rs"
  - "crates/app/src/project_library.rs"
  - "crates/app/src/settings*.rs"
---
# Persistence conventions

There is no database. Preferences, project lists and named profiles are versioned JSON stores in the application data directory, written through the serialized preference executor off the UI thread; credentials use the macOS Keychain and are never written to disk on Linux. Read the writes-and-persistence contract in `crates/app/docs/` before changing a store.

A schema change bumps the store version, keeps older files readable, and carries a test that loads the previous version and the new one. Saves reread the store before merging so a concurrent writer is not overwritten, respect the documented size bounds, and never block a page render. Local repository paths and build outputs stay out of versioned defaults.
