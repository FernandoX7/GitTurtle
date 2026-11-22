//! Pure bounded in-memory path normalization and substring matching.

use anyhow::{Result, ensure};
use gitturtle_core::FileChange;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

pub const MAX_PATHS: usize = 100_000;
pub const MAX_INDEX_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_QUERY_BYTES: usize = 4096;

pub type Index = Vec<[String; 2]>;

pub fn checkpoint(cancellation: &AtomicU64, ticket: u64) -> Result<()> {
    ensure!(
        cancellation.load(Ordering::Relaxed) == ticket,
        "Path filter superseded"
    );
    Ok(())
}
pub fn prepare_index(files: &[FileChange], cancellation: &AtomicU64, ticket: u64) -> Result<Index> {
    ensure!(
        files.len() <= MAX_PATHS,
        "Path filtering supports up to 100,000 changed files. Clear the filter to browse every file."
    );
    prepare_paths(
        files
            .iter()
            .map(|file| [file.old_path.as_deref(), file.new_path.as_deref()]),
        cancellation,
        ticket,
    )
}
pub fn prepare_paths<'a>(
    paths: impl Iterator<Item = [Option<&'a std::path::Path>; 2]>,
    cancellation: &AtomicU64,
    ticket: u64,
) -> Result<Index> {
    let mut bytes = 0;
    let mut index = Vec::new();
    for (i, paths) in paths.enumerate() {
        ensure!(
            i < MAX_PATHS,
            "Path filtering supports up to 100,000 changed files. Clear the filter to browse every file."
        );
        if i % 128 == 0 {
            checkpoint(cancellation, ticket)?;
        }
        let mut entry = [String::new(), String::new()];
        for (side, path) in paths.into_iter().enumerate() {
            if let Some(path) = path {
                let display = path.to_string_lossy();
                ensure!(
                    display.len() <= 64 * 1024,
                    "A changed path exceeds the 64 KiB filter limit. Clear the filter to browse every file."
                );
                ensure!(
                    display.len() <= MAX_INDEX_BYTES - bytes,
                    "Changed paths exceed the 16 MiB filter limit. Clear the filter to browse every file."
                );
                entry[side] = display.to_lowercase();
                bytes += entry[side].len();
                ensure!(
                    bytes <= MAX_INDEX_BYTES,
                    "Changed paths exceed the 16 MiB filter limit. Clear the filter to browse every file."
                );
            }
        }
        index.push(entry);
    }
    Ok(index)
}
pub fn matching(
    index: &Index,
    query: &str,
    cancellation: &AtomicU64,
    ticket: u64,
) -> Result<Arc<[usize]>> {
    ensure!(
        query.len() <= MAX_QUERY_BYTES,
        "Path filter queries must be at most 4,096 bytes."
    );
    let query = query.to_lowercase();
    let mut rows = Vec::new();
    for (i, paths) in index.iter().enumerate() {
        if i % 128 == 0 {
            checkpoint(cancellation, ticket)?;
        }
        if paths.iter().any(|path| path.contains(&query)) {
            rows.push(i);
        }
    }
    checkpoint(cancellation, ticket)?;
    Ok(rows.into())
}
