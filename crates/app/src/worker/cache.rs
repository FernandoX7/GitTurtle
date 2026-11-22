//! Bounded LRU preview cache shared across worker requests.
use super::types::{Content, Job};
use super::{CACHE_BYTE_LIMIT, CACHE_ENTRY_LIMIT, PREVIEW_EDGE};
use gitturtle_core::FileChange;
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
};

/// Both filenames affect text patch headers and the image decoder hint. Store
/// byte-safe paths as structured fields instead of concatenating display strings.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(crate) struct PreviewKey {
    pub(super) repository: PathBuf,
    pub(super) old_oid: Option<String>,
    pub(super) new_oid: Option<String>,
    pub(super) old_path: Option<PathBuf>,
    pub(super) new_path: Option<PathBuf>,
    pub(super) old_mode: String,
    pub(super) new_mode: String,
    pub(super) preview_edge: u32,
    pub(super) origins: crate::markdown_view::Origins,
}

impl PreviewKey {
    pub(super) fn new(repository: &Path, file: &FileChange) -> Self {
        Self {
            repository: repository.into(),
            old_oid: file.old_oid.clone(),
            new_oid: file.new_oid.clone(),
            old_path: file.old_path.clone(),
            new_path: file.new_path.clone(),
            old_mode: file.old_mode.clone(),
            new_mode: file.new_mode.clone(),
            preview_edge: PREVIEW_EDGE,
            origins: Default::default(),
        }
    }

    pub(crate) fn with_origins(
        repository: &Path,
        file: &FileChange,
        origins: crate::markdown_view::Origins,
    ) -> Self {
        Self {
            origins,
            ..Self::new(repository, file)
        }
    }

    /// The key of a comparison preview (`Job::Preview`), the whole of what
    /// it reads, as its cache entry is keyed.
    pub(super) fn of(job: &Job) -> Option<Self> {
        let Job::Preview {
            repo,
            file,
            origins,
            ..
        } = job
        else {
            return None;
        };
        Some(Self::with_origins(repo.path(), file, origins.clone()))
    }
}

pub(super) struct PreviewCache {
    pub(super) entries: HashMap<PreviewKey, Arc<Content>>,
    pub(super) lru: VecDeque<PreviewKey>,
    pub(super) bytes: usize,
    byte_limit: usize,
    entry_limit: usize,
}

impl Default for PreviewCache {
    fn default() -> Self {
        Self::with_limits(CACHE_BYTE_LIMIT, CACHE_ENTRY_LIMIT)
    }
}

impl PreviewCache {
    pub(super) fn with_limits(byte_limit: usize, entry_limit: usize) -> Self {
        Self {
            entries: HashMap::new(),
            lru: VecDeque::new(),
            bytes: 0,
            byte_limit,
            entry_limit,
        }
    }

    pub(super) fn get(&mut self, key: &PreviewKey) -> Option<Arc<Content>> {
        let value = Arc::clone(self.entries.get(key)?);
        self.lru.retain(|existing| existing != key);
        self.lru.push_back(key.clone());
        Some(value)
    }

    pub(super) fn remove(&mut self, key: &PreviewKey) {
        if let Some(value) = self.entries.remove(key) {
            self.bytes -= value.bytes();
        }
        self.lru.retain(|existing| existing != key);
    }

    pub(super) fn insert(&mut self, key: PreviewKey, content: Arc<Content>) {
        self.remove(&key);
        let bytes = content.bytes();
        if bytes > self.byte_limit || self.entry_limit == 0 {
            return;
        }
        while self.bytes + bytes > self.byte_limit || self.entries.len() >= self.entry_limit {
            let Some(old) = self.lru.front().cloned() else {
                break;
            };
            self.remove(&old);
        }
        self.bytes += bytes;
        self.lru.push_back(key.clone());
        self.entries.insert(key, content);
    }
}
