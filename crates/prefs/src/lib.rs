//! Pure, UI-framework-independent persistence for preferences, history columns,
//! project libraries, Git identity profiles, recovery drafts, and activity journals.
#![cfg_attr(not(any(test, feature = "test-support")), forbid(unsafe_code))]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

pub mod activity;
pub mod columns;
pub mod preferences;
pub mod profiles;
pub mod project_library;
pub mod recovery_drafts;

pub use preferences::*;
