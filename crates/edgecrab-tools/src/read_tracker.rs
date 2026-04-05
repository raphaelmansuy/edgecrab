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

use std::sync::LazyLock;

use dashmap::DashMap;

/// Per-session tracking state.
#[derive(Default)]
struct ReadTrackState {
    /// Canonical string key of the most recent read_file / search_files call.
    last_key: Option<String>,
    /// How many times that same key has been called consecutively.
    consecutive: u32,
}

/// Process-level tracker — persists across conversation turns within a session.
static TRACKER: LazyLock<DashMap<String, ReadTrackState>> = LazyLock::new(DashMap::new);

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
