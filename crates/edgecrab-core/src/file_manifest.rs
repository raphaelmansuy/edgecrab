//! File modification manifests for in-process prompt caches.
//!
//! Stores `(mtime_secs, size_bytes)` per path so caches can invalidate without
//! re-reading file contents. Shared by skills and context-file caches.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Per-file snapshot used for mtime+size invalidation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileManifestEntry {
    pub mtime_nanos: u128,
    pub size_bytes: u64,
}

/// Snapshot of one or more files on disk.
#[derive(Debug, Clone, Default)]
pub struct FileManifest {
    entries: HashMap<PathBuf, FileManifestEntry>,
}

impl FileManifest {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains_path(&self, path: &Path) -> bool {
        self.entries.contains_key(path)
    }

    /// Stat `path` and record it in the manifest. Returns false when the file is absent.
    pub fn record_path(&mut self, path: &Path) -> bool {
        let Ok(meta) = std::fs::metadata(path) else {
            return false;
        };
        self.entries.insert(
            path.to_path_buf(),
            FileManifestEntry {
                mtime_nanos: mtime_nanos(&meta),
                size_bytes: meta.len(),
            },
        );
        true
    }

    /// Returns true when every recorded path still matches disk state.
    pub fn is_valid(&self) -> bool {
        for (path, expected) in &self.entries {
            match std::fs::metadata(path) {
                Err(_) => {
                    tracing::trace!(
                        path = %path.display(),
                        "file manifest: file deleted — cache invalidated"
                    );
                    return false;
                }
                Ok(meta) => {
                    if mtime_nanos(&meta) != expected.mtime_nanos
                        || meta.len() != expected.size_bytes
                    {
                        tracing::trace!(
                            path = %path.display(),
                            "file manifest: file changed — cache invalidated"
                        );
                        return false;
                    }
                }
            }
        }
        true
    }
}

/// Evict the oldest entry when an in-process cache map exceeds `max_entries`.
pub fn evict_oldest_cache_entry<K, V, F>(
    map: &mut std::collections::HashMap<K, V>,
    max_entries: usize,
    built_at: F,
) where
    K: Eq + std::hash::Hash + Clone,
    F: Fn(&V) -> std::time::Instant,
{
    if map.len() >= max_entries
        && let Some(oldest_key) = map
            .iter()
            .min_by_key(|(_, v)| built_at(v))
            .map(|(k, _)| k.clone())
    {
        map.remove(&oldest_key);
    }
}

fn mtime_nanos(meta: &std::fs::Metadata) -> u128 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn manifest_detects_content_change() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("AGENTS.md");
        std::fs::write(&path, "v1").unwrap();

        let mut manifest = FileManifest::new();
        assert!(manifest.record_path(&path));

        std::fs::write(&path, "v2-longer").unwrap();
        assert!(!manifest.is_valid());
    }

    #[test]
    fn manifest_detects_deletion() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("AGENTS.md");
        std::fs::write(&path, "x").unwrap();

        let mut manifest = FileManifest::new();
        manifest.record_path(&path);
        std::fs::remove_file(&path).unwrap();
        assert!(!manifest.is_valid());
    }

    #[test]
    fn manifest_detects_same_size_content_change() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("AGENTS.md");
        std::fs::write(&path, "aaaaaaaa").unwrap();

        let mut manifest = FileManifest::new();
        manifest.record_path(&path);

        std::thread::sleep(std::time::Duration::from_millis(5));
        std::fs::write(&path, "bbbbbbbb").unwrap();
        assert!(!manifest.is_valid());
    }
}
