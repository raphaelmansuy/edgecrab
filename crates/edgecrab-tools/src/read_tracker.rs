//! # read_tracker — Consecutive re-read loop detection
//!
//! WHY: LLMs occasionally enter a loop where they re-read the exact same
//! file region (or run the exact same search) multiple times in a row
//! after a context compression boundary. Without a circuit-breaker the
//! agent burns tokens and tool-call budget getting nowhere.
//!
//! Mirrors hermes-agent's `_read_tracker` / `notify_other_tool_call()`
//! semantics exactly:
//!   - Warn at 3 consecutive identical reads/searches.
//!   - Hard-block at 4 (return error, no content).
//!   - Any intervening non-read/non-search tool call resets the counter.
//!
//! Design: module-level `LazyLock<DashMap>` keyed by `session_id` so the
//! state persists across multiple conversation turns within the same session,
//! matching hermes's module-level `_read_tracker` dict.  Uses `DashMap` for
//! lock-free concurrent reads from parallel tool dispatches.

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;
use std::time::SystemTime;

use dashmap::DashMap;
use edgecrab_types::ToolError;

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileSnapshot {
    exists: bool,
    len: u64,
    modified: Option<SystemTime>,
}

/// Key into the read dedup cache: (path_string, start_line, end_line).
/// `None` for start/end means "whole file".
type DedupKey = (String, Option<u64>, Option<u64>);

/// Per-session tracking state.
#[derive(Default)]
struct ReadTrackState {
    /// Canonical string key of the most recent read_file / search_files call.
    last_key: Option<String>,
    /// How many times that same key has been called consecutively.
    consecutive: u32,
    /// Last observed on-disk snapshot for files read in this session.
    file_snapshots: HashMap<String, FileSnapshot>,
    /// FP13: mtime-based read dedup cache.
    ///
    /// Maps (path, start_line, end_line) → mtime at read time.
    /// If the next read of the same range finds the same mtime, the file
    /// has not changed and the tokens would be wasted context.
    ///
    /// Cross-ref: Hermes `_read_tracker[task_id]["dedup"]`
    read_dedup: HashMap<DedupKey, SystemTime>,
}

/// Process-level tracker — persists across conversation turns within a session.
static TRACKER: LazyLock<DashMap<String, ReadTrackState>> = LazyLock::new(DashMap::new);

fn path_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn snapshot_for_path(path: &Path) -> Result<FileSnapshot, ToolError> {
    match std::fs::metadata(path) {
        Ok(meta) => Ok(FileSnapshot {
            exists: meta.is_file(),
            len: meta.len(),
            modified: meta.modified().ok(),
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(FileSnapshot {
            exists: false,
            len: 0,
            modified: None,
        }),
        Err(err) => Err(ToolError::Other(format!(
            "Cannot stat '{}': {}",
            path.display(),
            err
        ))),
    }
}

/// Record a read_file or search_files call.
///
/// Returns the current consecutive repeat count AFTER recording this call.
/// * 1 = first occurrence (new key or key changed)
/// * 2 = second consecutive identical call
/// * 3 = warn threshold
/// * ≥4 = block threshold
pub fn check_and_update(session_id: &str, key: String) -> u32 {
    let mut state = TRACKER.entry(session_id.to_string()).or_default();
    if state.last_key.as_deref() == Some(&key) {
        state.consecutive += 1;
    } else {
        state.last_key = Some(key);
        state.consecutive = 1;
    }
    state.consecutive
}

/// Reset the consecutive counter for a session.
///
/// Called by the tool dispatcher after every tool call that is NOT
/// `read_file` or `search_files`.  This ensures only truly back-to-back
/// identical calls trigger the warning/block — any other tool in between
/// is enough to reset.
pub fn notify_other_tool_call(session_id: &str) {
    if let Some(mut state) = TRACKER.get_mut(session_id) {
        state.last_key = None;
        state.consecutive = 0;
    }
    // If no entry exists yet there's nothing to reset — no-op is correct.
}

/// Build the canonical key for a `read_file` call.
pub fn read_key(path: &str, line_start: Option<usize>, line_end: Option<usize>) -> String {
    format!(
        "read:{}:{}:{}",
        path,
        line_start.unwrap_or(0),
        line_end.unwrap_or(0)
    )
}

/// Build the canonical key for a `search_files` call.
pub fn search_key(
    pattern: &str,
    path: Option<&str>,
    include: Option<&str>,
    max_results: usize,
) -> String {
    format!(
        "search:{}:{}:{}:{}",
        pattern,
        path.unwrap_or(""),
        include.unwrap_or(""),
        max_results
    )
}

pub fn record_file_snapshot(session_id: &str, path: &Path) -> Result<(), ToolError> {
    let snapshot = snapshot_for_path(path)?;
    let mut state = TRACKER.entry(session_id.to_string()).or_default();
    state.file_snapshots.insert(path_key(path), snapshot);
    Ok(())
}

pub fn clear_file_snapshot(session_id: &str, path: &Path) {
    if let Some(mut state) = TRACKER.get_mut(session_id) {
        state.file_snapshots.remove(&path_key(path));
    }
}

pub fn has_file_snapshot(session_id: &str, path: &Path) -> bool {
    TRACKER
        .get(session_id)
        .map(|state| state.file_snapshots.contains_key(&path_key(path)))
        .unwrap_or(false)
}

pub fn guard_file_freshness(
    session_id: &str,
    tool: &str,
    display_path: &str,
    path: &Path,
) -> Result<(), ToolError> {
    let Some(state) = TRACKER.get(session_id) else {
        return Ok(());
    };
    let Some(previous) = state.file_snapshots.get(&path_key(path)).cloned() else {
        return Ok(());
    };
    drop(state);

    let current = snapshot_for_path(path)?;
    if current == previous {
        return Ok(());
    }

    Err(ToolError::InvalidArgs {
        tool: tool.into(),
        message: format!(
            "'{display_path}' was modified since you last read it in this session. Re-run read_file on the current file before using {tool} so the model does not act on stale cached context."
        ),
    })
}

// ─── FP13: mtime-based read dedup cache ────────────────────────────────────
//
// WHY: If the file hasn't changed since the last read of the same range,
// those tokens are wasted context. Instead of injecting full content again,
// return a stub: "File unchanged since last read. Content already in context."
//
// Cross-ref: Hermes `_read_tracker[task_id]["dedup"]` dict.

/// Record a read in the dedup cache after the content has been served.
///
/// Called at the END of `read_file.execute()` so the cache entry only exists
/// for files that were successfully read (not for errors or image redirects).
pub fn record_read_dedup(
    session_id: &str,
    path: &Path,
    start_line: Option<u64>,
    end_line: Option<u64>,
) {
    let Ok(meta) = std::fs::metadata(path) else { return };
    let mtime = meta.modified().ok();
    // Only cache when we have a valid mtime — otherwise every re-read appears
    // to need dedup but we can't compare timestamps.
    let Some(mtime) = mtime else { return };
    let key: DedupKey = (path_key(path), start_line, end_line);
    let mut state = TRACKER.entry(session_id.to_string()).or_default();
    state.read_dedup.insert(key, mtime);
}

/// Check whether a read_file call would be a no-op (file unchanged).
///
/// Returns `Some(stub_message)` when the dedup cache contains an entry for
/// `(path, start_line, end_line)` and the file's current mtime matches what
/// was recorded when the file was last read.
///
/// Returns `None` when:
/// - No prior read of this range exists (first read → allow through)
/// - File mtime has changed (file was modified → allow through)
/// - File metadata is unreadable (safe fallback → allow through)
pub fn check_read_dedup(
    session_id: &str,
    path: &Path,
    start_line: Option<u64>,
    end_line: Option<u64>,
) -> Option<String> {
    let state = TRACKER.get(session_id)?;
    let key: DedupKey = (path_key(path), start_line, end_line);
    let cached_mtime = *state.read_dedup.get(&key)?;
    drop(state);

    // Check current mtime — if changed, dedup cache miss
    let current_mtime = std::fs::metadata(path).ok()?.modified().ok()?;
    if current_mtime != cached_mtime {
        return None;
    }

    let range_desc = match (start_line, end_line) {
        (Some(s), Some(e)) => format!(" (lines {s}–{e})"),
        (Some(s), None) => format!(" (from line {s})"),
        _ => String::new(),
    };
    Some(format!(
        "[File unchanged since last read] '{}'{range_desc} has not been modified. \
         The content is already in your context from a previous read_file call. \
         Proceed using the content you already have rather than re-reading.",
        path.display()
    ))
}

// ─── FP17: Reset read tracker state after compression ──────────────────────
//
// WHY: Context compression discards old messages, including earlier read_file
// results. After compression the model no longer has the file content, so the
// dedup cache would incorrectly suppress necessary re-reads.
//
// Cross-ref: Hermes `reset_file_dedup()` called from context_compressor.py.

/// Reset per-session dedup cache after context compression.
///
/// Does NOT reset `file_snapshots` (freshness guards should still apply) or
/// the `consecutive` counter (that is per-call, not per-context).
/// Only clears the dedup cache so the next `read_file` on any path is served
/// fresh — the model lost the content when the messages were pruned.
pub fn reset_read_dedup(session_id: &str) {
    if let Some(mut state) = TRACKER.get_mut(session_id) {
        state.read_dedup.clear();
    }
    // If no entry exists there is nothing to reset — no-op is correct.
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn first_call_returns_one() {
        let session = "rt-test-1";
        let key = read_key("foo.rs", None, None);
        assert_eq!(check_and_update(session, key), 1);
    }

    #[test]
    fn consecutive_same_key_increments() {
        let session = "rt-test-2";
        let key = || read_key("bar.rs", Some(1), Some(50));
        assert_eq!(check_and_update(session, key()), 1);
        assert_eq!(check_and_update(session, key()), 2);
        assert_eq!(check_and_update(session, key()), 3);
        assert_eq!(check_and_update(session, key()), 4);
    }

    #[test]
    fn different_key_resets_count() {
        let session = "rt-test-3";
        assert_eq!(check_and_update(session, read_key("a.rs", None, None)), 1);
        assert_eq!(check_and_update(session, read_key("a.rs", None, None)), 2);
        // Different file → reset
        assert_eq!(check_and_update(session, read_key("b.rs", None, None)), 1);
        // Back to a.rs → also reset (new key)
        assert_eq!(check_and_update(session, read_key("a.rs", None, None)), 1);
    }

    #[test]
    fn notify_resets_count() {
        let session = "rt-test-4";
        let key = || read_key("reset.rs", None, None);
        check_and_update(session, key());
        check_and_update(session, key());
        assert_eq!(check_and_update(session, key()), 3);
        // Some other tool fires — resets
        notify_other_tool_call(session);
        // Next identical read is count=1 again (fresh)
        assert_eq!(check_and_update(session, key()), 1);
    }

    #[test]
    fn freshness_guard_detects_external_modification_after_read() {
        let dir = TempDir::new().expect("tmpdir");
        let path = dir.path().join("freshness.txt");
        std::fs::write(&path, "v1").expect("seed");

        let session = "rt-test-5";
        record_file_snapshot(session, &path).expect("record snapshot");

        std::fs::write(&path, "v2").expect("modify");

        let err = guard_file_freshness(session, "write_file", "freshness.txt", &path)
            .expect_err("stale guard should fire");
        assert!(err.to_string().contains("modified since you last read it"));
    }

    #[test]
    fn freshness_guard_is_cleared_after_delete() {
        let dir = TempDir::new().expect("tmpdir");
        let path = dir.path().join("deleted.txt");
        std::fs::write(&path, "v1").expect("seed");

        let session = "rt-test-6";
        record_file_snapshot(session, &path).expect("record snapshot");
        clear_file_snapshot(session, &path);
        std::fs::remove_file(&path).expect("delete");

        guard_file_freshness(session, "write_file", "deleted.txt", &path)
            .expect("cleared snapshot should not block");
    }

    #[test]
    fn has_file_snapshot_reflects_record_and_clear() {
        let dir = TempDir::new().expect("tmpdir");
        let path = dir.path().join("snapshot.txt");
        std::fs::write(&path, "v1").expect("seed");

        let session = "rt-test-7";
        assert!(!has_file_snapshot(session, &path));

        record_file_snapshot(session, &path).expect("record snapshot");
        assert!(has_file_snapshot(session, &path));

        clear_file_snapshot(session, &path);
        assert!(!has_file_snapshot(session, &path));
    }

    // ─── FP13: read dedup cache tests ─────────────────────────────────────

    #[test]
    fn read_dedup_returns_none_on_first_read() {
        let dir = TempDir::new().expect("tmpdir");
        let path = dir.path().join("dedup_first.txt");
        std::fs::write(&path, "hello").expect("seed");

        let session = "rt-dedup-1";
        // No prior read → dedup returns None (allow through)
        assert!(
            check_read_dedup(session, &path, None, None).is_none(),
            "first read must not be deduped"
        );
    }

    #[test]
    fn read_dedup_returns_stub_on_unchanged_file() {
        let dir = TempDir::new().expect("tmpdir");
        let path = dir.path().join("dedup_unchanged.txt");
        std::fs::write(&path, "content").expect("seed");

        let session = "rt-dedup-2";
        // Simulate a prior read: record dedup entry
        record_read_dedup(session, &path, None, None);

        // File not modified → should return stub
        let result = check_read_dedup(session, &path, None, None);
        assert!(result.is_some(), "unchanged file must produce dedup stub");
        let stub = result.unwrap();
        assert!(stub.contains("unchanged since last read"), "stub must mention unchanged");
        assert!(stub.contains("already in your context"), "stub must guide model");
    }

    #[test]
    fn read_dedup_allows_read_after_file_modified() {
        let dir = TempDir::new().expect("tmpdir");
        let path = dir.path().join("dedup_modified.txt");
        std::fs::write(&path, "v1").expect("seed");

        let session = "rt-dedup-3";
        record_read_dedup(session, &path, None, None);

        // Modify the file (sleep 1 ms to guarantee mtime change on fast filesystems)
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&path, "v2").expect("modify");

        // File changed → dedup must return None (allow full read)
        assert!(
            check_read_dedup(session, &path, None, None).is_none(),
            "modified file must not be deduped"
        );
    }

    #[test]
    fn read_dedup_separate_ranges_are_independent() {
        let dir = TempDir::new().expect("tmpdir");
        let path = dir.path().join("dedup_ranges.txt");
        std::fs::write(&path, "line1\nline2\nline3").expect("seed");

        let session = "rt-dedup-4";
        // Record dedup for range 1-2 only
        record_read_dedup(session, &path, Some(1), Some(2));

        // Range 1-2: already read → stub
        assert!(check_read_dedup(session, &path, Some(1), Some(2)).is_some());
        // Range 1-3: NOT yet read → allow
        assert!(check_read_dedup(session, &path, Some(1), Some(3)).is_none());
        // Whole file: NOT yet read → allow
        assert!(check_read_dedup(session, &path, None, None).is_none());
    }

    // ─── FP17: reset_read_dedup tests ─────────────────────────────────────

    #[test]
    fn reset_read_dedup_clears_cache_after_compression() {
        let dir = TempDir::new().expect("tmpdir");
        let path = dir.path().join("dedup_reset.txt");
        std::fs::write(&path, "content").expect("seed");

        let session = "rt-dedup-5";
        record_read_dedup(session, &path, None, None);

        // Before reset: dedup should suppress re-read
        assert!(check_read_dedup(session, &path, None, None).is_some());

        // Simulate compression event: clear dedup cache
        reset_read_dedup(session);

        // After reset: next read must be allowed (context was pruned)
        assert!(
            check_read_dedup(session, &path, None, None).is_none(),
            "after compression reset, dedup cache must be cleared"
        );
    }

    #[test]
    fn reset_read_dedup_noop_for_unknown_session() {
        // Must not panic for sessions that have no tracker state
        reset_read_dedup("rt-unknown-session-xyz");
    }
}
