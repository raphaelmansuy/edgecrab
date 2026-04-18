//! # MemoryManager — programmatic access to agent memory files.
//!
//! Provides `read(key)` / `write(key, value)` / `remove(key, old)` for the
//! `MEMORY.md` and `USER.md` files stored under `~/.edgecrab/memories/`.
//!
//! This is the SDK counterpart to the `memory_read` / `memory_write` tools
//! that the agent uses during conversation. It allows SDK consumers to
//! inspect or seed memory programmatically without running an agent loop.

use std::path::{Path, PathBuf};

use crate::error::SdkError;
use edgecrab_security::check_memory_content;

const ENTRY_DELIMITER: &str = "\n§\n";

/// Maximum characters for MEMORY.md (agent's curated notes).
const MEMORY_MAX_CHARS: usize = 2200;
/// Maximum characters for USER.md (user profile).
const USER_MAX_CHARS: usize = 1375;

/// Target memory file selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryTarget {
    /// Agent memory (`MEMORY.md`).
    Memory,
    /// User profile (`USER.md`).
    User,
}

impl MemoryTarget {
    fn filename(self) -> &'static str {
        match self {
            Self::Memory => "MEMORY.md",
            Self::User => "USER.md",
        }
    }

    fn max_chars(self) -> usize {
        match self {
            Self::Memory => MEMORY_MAX_CHARS,
            Self::User => USER_MAX_CHARS,
        }
    }
}

impl std::fmt::Display for MemoryTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Memory => write!(f, "memory"),
            Self::User => write!(f, "user"),
        }
    }
}

/// Resolve a key string to a [`MemoryTarget`].
fn resolve_target(key: &str) -> MemoryTarget {
    match key {
        "user" => MemoryTarget::User,
        _ => MemoryTarget::Memory,
    }
}

/// Programmatic access to the agent's persistent memory files.
///
/// # Usage
///
/// ```rust,no_run
/// # use edgecrab_sdk_core::MemoryManager;
/// # async fn example() -> Result<(), edgecrab_sdk_core::SdkError> {
/// let mem = MemoryManager::new("/path/to/.edgecrab");
/// let content = mem.read("memory").await?;
/// mem.write("memory", "Important fact about the user").await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct MemoryManager {
    memories_dir: PathBuf,
}

impl MemoryManager {
    /// Create a new `MemoryManager` rooted at the given edgecrab home directory.
    pub fn new(edgecrab_home: impl AsRef<Path>) -> Self {
        Self {
            memories_dir: edgecrab_home.as_ref().join("memories"),
        }
    }

    /// Read the contents of a memory file.
    ///
    /// `key` is `"memory"` (default, MEMORY.md) or `"user"` (USER.md).
    /// Returns the file content or an empty string if the file doesn't exist yet.
    pub async fn read(&self, key: &str) -> Result<String, SdkError> {
        let target = resolve_target(key);
        let path = self.memories_dir.join(target.filename());

        if !path.is_file() {
            return Ok(String::new());
        }

        tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| SdkError::Config(format!("Cannot read {}: {}", target.filename(), e)))
    }

    /// Write (append) a new entry to a memory file.
    ///
    /// `key` is `"memory"` or `"user"`.
    /// `value` is the content to append as a new `§`-delimited entry.
    ///
    /// The content is scanned for prompt injection before persisting.
    pub async fn write(&self, key: &str, value: &str) -> Result<(), SdkError> {
        let target = resolve_target(key);
        let content = value.trim();

        if content.is_empty() {
            return Err(SdkError::Config("Content cannot be empty".into()));
        }

        // Security: scan for prompt injection
        if let Err(msg) = check_memory_content(content) {
            return Err(SdkError::Config(format!("Memory content blocked: {msg}")));
        }

        tokio::fs::create_dir_all(&self.memories_dir)
            .await
            .map_err(|e| SdkError::Config(format!("Cannot create memories dir: {e}")))?;

        let path = self.memories_dir.join(target.filename());
        let existing = tokio::fs::read_to_string(&path).await.unwrap_or_default();

        // Duplicate detection
        let existing_entries: Vec<&str> = existing
            .split('§')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if existing_entries.contains(&content) {
            return Ok(()); // Already exists — no-op
        }

        let mut result = existing.clone();
        if !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        if !result.is_empty() {
            result.push_str(ENTRY_DELIMITER.trim_start_matches('\n'));
        }
        result.push_str(content);
        result.push('\n');

        if result.len() > target.max_chars() {
            return Err(SdkError::Config(format!(
                "{} would exceed {}-char limit ({} chars). Remove old entries first.",
                target.filename(),
                target.max_chars(),
                result.len()
            )));
        }

        // Atomic write: temp file + rename
        let tmp_path = path.with_extension("tmp");
        tokio::fs::write(&tmp_path, &result)
            .await
            .map_err(|e| SdkError::Config(format!("Cannot write {}: {e}", target.filename())))?;
        tokio::fs::rename(&tmp_path, &path)
            .await
            .map_err(|e| SdkError::Config(format!("Cannot rename {}: {e}", target.filename())))?;

        Ok(())
    }

    /// Remove an entry from a memory file by substring match.
    ///
    /// Finds the first `§`-delimited entry containing `old_content` and removes it.
    pub async fn remove(&self, key: &str, old_content: &str) -> Result<bool, SdkError> {
        let target = resolve_target(key);
        let path = self.memories_dir.join(target.filename());

        if !path.is_file() {
            return Ok(false);
        }

        let existing = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| SdkError::Config(format!("Cannot read {}: {e}", target.filename())))?;

        let entries: Vec<String> = existing
            .split('§')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let old = old_content.trim();
        let idx = entries.iter().position(|e| e.contains(old));

        match idx {
            Some(i) => {
                let mut new_entries = entries;
                new_entries.remove(i);
                let result = if new_entries.is_empty() {
                    String::new()
                } else {
                    new_entries.join(ENTRY_DELIMITER) + "\n"
                };

                let tmp_path = path.with_extension("tmp");
                tokio::fs::write(&tmp_path, &result).await.map_err(|e| {
                    SdkError::Config(format!("Cannot write {}: {e}", target.filename()))
                })?;
                tokio::fs::rename(&tmp_path, &path).await.map_err(|e| {
                    SdkError::Config(format!("Cannot rename {}: {e}", target.filename()))
                })?;

                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// List all entries from a memory file as separate strings.
    pub async fn entries(&self, key: &str) -> Result<Vec<String>, SdkError> {
        let content = self.read(key).await?;
        Ok(content
            .split('§')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_read_nonexistent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr = MemoryManager::new(tmp.path());
        let content = mgr.read("memory").await.unwrap();
        assert!(content.is_empty());
    }

    #[tokio::test]
    async fn test_memory_write_and_read() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr = MemoryManager::new(tmp.path());

        mgr.write("memory", "First fact").await.unwrap();
        let content = mgr.read("memory").await.unwrap();
        assert!(content.contains("First fact"));

        mgr.write("memory", "Second fact").await.unwrap();
        let content = mgr.read("memory").await.unwrap();
        assert!(content.contains("First fact"));
        assert!(content.contains("Second fact"));
    }

    #[tokio::test]
    async fn test_memory_duplicate_detection() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr = MemoryManager::new(tmp.path());

        mgr.write("memory", "Unique fact").await.unwrap();
        mgr.write("memory", "Unique fact").await.unwrap(); // No-op
        let entries = mgr.entries("memory").await.unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[tokio::test]
    async fn test_memory_remove() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr = MemoryManager::new(tmp.path());

        mgr.write("memory", "Keep this").await.unwrap();
        mgr.write("memory", "Remove this").await.unwrap();

        let removed = mgr.remove("memory", "Remove").await.unwrap();
        assert!(removed);

        let entries = mgr.entries("memory").await.unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].contains("Keep this"));
    }

    #[tokio::test]
    async fn test_memory_user_target() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr = MemoryManager::new(tmp.path());

        mgr.write("user", "User preference").await.unwrap();
        let content = mgr.read("user").await.unwrap();
        assert!(content.contains("User preference"));
    }

    #[tokio::test]
    async fn test_memory_entries() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr = MemoryManager::new(tmp.path());

        mgr.write("memory", "Entry one").await.unwrap();
        mgr.write("memory", "Entry two").await.unwrap();
        mgr.write("memory", "Entry three").await.unwrap();

        let entries = mgr.entries("memory").await.unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[tokio::test]
    async fn test_memory_empty_content_rejected() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mgr = MemoryManager::new(tmp.path());

        let result = mgr.write("memory", "   ").await;
        assert!(result.is_err());
    }
}
