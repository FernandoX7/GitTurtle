//! Pure, UI-framework-independent diff, graph-layout, partial-staging, split-alignment,
//! text-review, and path-filter models.
#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

pub mod graph;
pub mod gutter;
pub mod partial_view;
pub mod patch;
pub mod path_filter;
pub mod split_diff;
pub mod text_review;
