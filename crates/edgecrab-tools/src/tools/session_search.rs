//! # session_search — Full-text search across past sessions
//!
//! WHY session search: Lets the agent recall past conversations using
//! the FTS5 index already maintained by edgecrab-state.
//!
//! When `query` is omitted or empty, the tool falls back to listing the
//! N most-recent sessions ordered by start time (no FTS involved).  This
//! means callers asking "list my last 10 sessions" don't need a keyword.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use edgecrab_types::{ToolError, ToolSchema};

use crate::registry::{ToolContext, ToolHandler};

pub struct SessionSearchTool;

#[derive(Deserialize)]
struct Args {
    /// Optional FTS5 query. Absent or empty → list recent sessions.
    #[serde(default)]
    query: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    10
}

/// Format a Unix timestamp (seconds since epoch) as a human-readable UTC date-time.
fn fmt_ts(ts: f64) -> String {
    // Avoid pulling in chrono for a simple display — produce ISO-8601 manually
    // via UNIX arithmetic.  Good enough for TUI display to the nearest minute.
    let secs = ts as u64;
    let minutes = secs / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    let h = hours % 24;
    let m = minutes % 60;

    // Rough Gregorian year/month/day from days-since-epoch (1970-01-01)
    let (y, mo, d) = gregorian_from_days(days);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02} UTC")
}

/// Minimal Gregorian calendar conversion from days since 1970-01-01.
/// Accurate for dates in a reasonable range (no timezone, no leap-seconds).
#[allow(clippy::manual_div_ceil)] // Integer floor-division from Fliegel&vanFlandern; not a div_ceil
fn gregorian_from_days(days: u64) -> (u64, u64, u64) {
    // Algorithm: Fliegel & van Flandern (CACM, 1968)
    // Adapted for 0-based days since 1970-01-01 (JDN 2440588).
    let jdn = days + 2_440_588;
    let l = jdn + 68_569;
    let n = 4 * l / 146_097;
    let l = l - (146_097 * n + 3) / 4;
    let i = 4_000 * (l + 1) / 1_461_001;
    let l = l - 1_461 * i / 4 + 31;
    let j = 80 * l / 2_447;
    let day = l - 2_447 * j / 80;
    let l = j / 11;
    let month = j + 2 - 12 * l;
    let year = 100 * (n - 49) + i + l;
    (year, month, day)
}

#[async_trait]
impl ToolHandler for SessionSearchTool {
    fn name(&self) -> &'static str {
        "session_search"
    }

    fn toolset(&self) -> &'static str {
        "session"
    }

    fn emoji(&self) -> &'static str {
        "🔎"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "session_search".into(),
            description: concat!(
                "Search past sessions using full-text search (FTS5), or list the most recent ",
                "sessions when no query is given. Use this proactively when the user refers to ",
                "previous work or asks what happened in a past session. ",
                "Omit `query` (or pass an empty string) to get the latest N sessions ordered ",
                "by start time. Provide a keyword `query` to find sessions whose messages ",
                "contain that text. Prefer OR between independent keywords for broader recall."
            )
            .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "FTS5 search query. Omit or leave empty to list recent sessions."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results (default: 10, max: 50)"
                    }
                },
                "required": []
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: Args = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "session_search".into(),
            message: e.to_string(),
        })?;

        let limit = args.limit.clamp(1, 50);

        let db = ctx.state_db.clone().ok_or_else(|| ToolError::Unavailable {
            tool: "session_search".into(),
            reason: "No session database available".into(),
        })?;

        // Determine mode: FTS search vs. chronological list
        let query_str = args.query.as_deref().unwrap_or("").trim().to_string();

        if query_str.is_empty() {
            // ── List mode: return most-recent sessions ─────────────────
            let results = tokio::task::spawn_blocking(move || db.list_sessions(limit))
                .await
                .map_err(|e| ToolError::Other(format!("Task join error: {}", e)))?
                .map_err(|e| ToolError::Other(format!("List sessions error: {}", e)))?;

            if results.is_empty() {
                return Ok("No sessions found.".to_string());
            }

            let mut output = format!("Latest {} session(s):\n\n", results.len());
            for (i, s) in results.iter().enumerate() {
                let title = s.title.as_deref().unwrap_or("(untitled)");
                let model = s.model.as_deref().unwrap_or("unknown");
                output.push_str(&format!(
                    "{}. [{}] {} — {} | {} messages | model: {}\n",
                    i + 1,
                    &s.id[..8.min(s.id.len())],
                    fmt_ts(s.started_at),
                    title,
                    s.message_count,
                    model,
                ));
            }
            Ok(output)
        } else {
            // ── Search mode: FTS5 full-text search ─────────────────────
            // Clone before move: query_str is consumed by spawn_blocking's closure,
            // but we need it again for the result header (DRY — one source of truth).
            let display_query = query_str.clone();
            let results = tokio::task::spawn_blocking(move || db.search(&query_str, limit))
                .await
                .map_err(|e| ToolError::Other(format!("Task join error: {}", e)))?
                .map_err(|e| ToolError::Other(format!("Search error: {}", e)))?;

            if results.is_empty() {
                return Ok(format!("No results found for '{display_query}'"));
            }

            let mut output = format!(
                "Found {} result(s) for '{display_query}':\n\n",
                results.len()
            );
            for (i, result) in results.iter().enumerate() {
                output.push_str(&format!(
                    "{}. [session: {}] ({})\n   {}\n\n",
                    i + 1,
                    &result.session_id[..8.min(result.session_id.len())],
                    result.role,
                    result.snippet
                ));
            }
            Ok(output)
        }
    }
}

inventory::submit!(&SessionSearchTool as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;

    // ── fmt_ts unit tests ──────────────────────────────────────────────

    #[test]
    fn fmt_ts_epoch() {
        // 1970-01-01 00:00 UTC
        assert_eq!(fmt_ts(0.0), "1970-01-01 00:00 UTC");
    }

    #[test]
    fn fmt_ts_known_date() {
        // 2024-01-15 14:30 UTC  →  1705329000 seconds (approx)
        // Let's use 2024-01-01 00:00 UTC = 1704067200
        let ts = 1_704_067_200.0_f64;
        let s = fmt_ts(ts);
        assert!(s.starts_with("2024-01-01"), "got: {s}");
    }

    // ── Tool behaviour tests ───────────────────────────────────────────

    /// Empty query (no DB) → Unavailable error (not InvalidArgs).
    #[tokio::test]
    async fn empty_query_without_db_returns_unavailable() {
        let ctx = ToolContext::test_context();
        let result = SessionSearchTool.execute(json!({"query": ""}), &ctx).await;
        assert!(
            matches!(result, Err(ToolError::Unavailable { .. })),
            "expected Unavailable, got: {result:?}"
        );
    }

    /// Omitted query (no DB) → Unavailable error (not InvalidArgs).
    #[tokio::test]
    async fn omitted_query_without_db_returns_unavailable() {
        let ctx = ToolContext::test_context();
        let result = SessionSearchTool.execute(json!({}), &ctx).await;
        assert!(
            matches!(result, Err(ToolError::Unavailable { .. })),
            "expected Unavailable, got: {result:?}"
        );
    }

    /// Whitespace-only query (no DB) → Unavailable error (not InvalidArgs).
    #[tokio::test]
    async fn whitespace_query_without_db_returns_unavailable() {
        let ctx = ToolContext::test_context();
        let result = SessionSearchTool
            .execute(json!({"query": "   "}), &ctx)
            .await;
        assert!(
            matches!(result, Err(ToolError::Unavailable { .. })),
            "expected Unavailable, got: {result:?}"
        );
    }

    /// Non-empty query with no DB → Unavailable error.
    #[tokio::test]
    async fn search_query_without_db_returns_unavailable() {
        let ctx = ToolContext::test_context();
        let result = SessionSearchTool
            .execute(json!({"query": "hello"}), &ctx)
            .await;
        assert!(
            matches!(result, Err(ToolError::Unavailable { .. })),
            "expected Unavailable, got: {result:?}"
        );
    }

    /// Limit clamping: 0 → 1, 999 → 50.
    #[tokio::test]
    async fn limit_clamped_zero_and_overflow() {
        let ctx = ToolContext::test_context();
        // Both should fail with Unavailable (no DB), not InvalidArgs
        for limit in [0_u64, 999] {
            let result = SessionSearchTool
                .execute(json!({"limit": limit}), &ctx)
                .await;
            assert!(
                matches!(result, Err(ToolError::Unavailable { .. })),
                "limit={limit}: expected Unavailable, got: {result:?}"
            );
        }
    }

    // ── Integration tests: real in-memory SessionDb ────────────────────

    /// Build a ToolContext backed by an on-disk SessionDb in a temp dir, seeded with sessions.
    fn ctx_with_db(sessions: &[(&str, &str)]) -> (ToolContext, tempfile::TempDir) {
        use edgecrab_state::{SessionDb, SessionRecord};
        use edgecrab_types::Message;
        use std::sync::Arc;

        let tmp = tempfile::TempDir::new().expect("temp dir");
        let db_path = tmp.path().join("state.db");
        let db = SessionDb::open(&db_path).expect("open DB");

        for (sid, content) in sessions {
            let record = SessionRecord {
                id: sid.to_string(),
                source: "cli".to_string(),
                user_id: None,
                model: Some("mock/test".to_string()),
                system_prompt: None,
                parent_session_id: None,
                started_at: 1_720_000_000.0 + sid.len() as f64,
                ended_at: None,
                end_reason: None,
                message_count: 0,
                tool_call_count: 0,
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                reasoning_tokens: 0,
                estimated_cost_usd: None,
                title: Some(format!("Session {sid}")),
            };
            db.save_session(&record).expect("save session");
            db.save_message(sid, &Message::user(content), 1_720_000_001.0)
                .expect("save message");
        }
        let mut ctx = ToolContext::test_context();
        ctx.state_db = Some(Arc::new(db));
        // Return TempDir so it outlives the test (dropped at end of test)
        (ctx, tmp)
    }

    /// Empty query with a real DB → list mode, shows recent sessions.
    #[tokio::test]
    async fn empty_query_with_db_lists_sessions() {
        let (ctx, _tmp) = ctx_with_db(&[
            ("sess-a", "Rust ownership and borrowing"),
            ("sess-b", "Python asyncio patterns"),
        ]);
        let result = SessionSearchTool
            .execute(json!({}), &ctx)
            .await
            .expect("should succeed in list mode");
        assert!(
            result.contains("Latest"),
            "expected 'Latest' header, got: {result}"
        );
        assert!(
            result.contains("sess-"),
            "expected session IDs, got: {result}"
        );
    }

    /// FTS search with a keyword finds the matching session.
    #[tokio::test]
    async fn fts5_search_with_db_finds_matches() {
        let (ctx, _tmp) = ctx_with_db(&[
            ("sess-rust", "Rust ownership and borrowing concepts"),
            ("sess-py", "Python asyncio event loop patterns"),
        ]);
        let result = SessionSearchTool
            .execute(json!({"query": "Rust"}), &ctx)
            .await
            .expect("FTS search should succeed");
        assert!(
            result.contains("Found"),
            "expected 'Found' header, got: {result}"
        );
        // The snippet or session id should appear
        assert!(
            result.to_lowercase().contains("rust") || result.contains("sess-rust"),
            "expected Rust-related content, got: {result}"
        );
    }

    /// FTS query with no matching content → "No results found" message.
    #[tokio::test]
    async fn fts5_search_no_match_returns_no_results_message() {
        let (ctx, _tmp) = ctx_with_db(&[("sess-x", "completely unrelated content")]);
        let result = SessionSearchTool
            .execute(json!({"query": "xyzzy_never_matches"}), &ctx)
            .await
            .expect("should return Ok, not Err, when no results");
        assert!(
            result.contains("No results"),
            "expected no-results message, got: {result}"
        );
    }

    /// List mode with limit=1 returns at most 1 session.
    #[tokio::test]
    async fn list_mode_respects_limit() {
        let (ctx, _tmp) = ctx_with_db(&[
            ("s1", "first session"),
            ("s2", "second session"),
            ("s3", "third session"),
        ]);
        let result = SessionSearchTool
            .execute(json!({"limit": 1}), &ctx)
            .await
            .expect("should succeed");
        assert!(result.contains("1. "), "should have item 1, got: {result}");
        assert!(
            !result.contains("2. "),
            "should not have item 2 with limit=1, got: {result}"
        );
    }

    /// Whitespace-only query with a real DB → falls back to list mode.
    #[tokio::test]
    async fn whitespace_query_with_db_falls_back_to_list() {
        let (ctx, _tmp) = ctx_with_db(&[("s1", "some content")]);
        let result = SessionSearchTool
            .execute(json!({"query": "   "}), &ctx)
            .await
            .expect("should succeed in list mode");
        assert!(
            result.contains("Latest") || result.contains("s1"),
            "got: {result}"
        );
    }

    /// Empty DB with no sessions → "No sessions found." message.
    #[tokio::test]
    async fn empty_db_list_returns_no_sessions_message() {
        let (ctx, _tmp) = ctx_with_db(&[]);
        let result = SessionSearchTool
            .execute(json!({}), &ctx)
            .await
            .expect("should succeed on empty DB");
        assert_eq!(result, "No sessions found.");
    }
}
