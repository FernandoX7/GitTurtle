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

There is no database. Preferences, project lists and named profiles are versioned JSON stores in the application data directory, written through the serialized preference executor off the UI thread. GitTurtle never writes credentials to disk or to any plaintext file: macOS keeps them in the Keychain and Linux in the desktop keyring through the Secret Service. Read the writes-and-persistence contract in `crates/app/docs/` before changing a store.

An added field with a default that older builds ignore does not bump the store version, as `system_code_font` did in `cea2012b`; a change older builds would misread does, keeps older files readable, and carries a test that loads the previous version and the new one. Saves reread the store before merging so a concurrent writer is not overwritten, respect the documented size bounds, and never block a page render. Local repository paths and build outputs stay out of versioned defaults.
