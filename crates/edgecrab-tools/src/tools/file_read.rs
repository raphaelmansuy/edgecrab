//! # read_file — Read file contents with optional line ranges
//!
//! WHY line ranges: LLMs have limited context. Reading entire large files
//! wastes tokens. Line ranges let the agent focus on relevant sections,
//! matching how hermes-agent's read_file works.
//!
//! WHY line numbers: Adds column-1 line numbers (`  42|content`) by default,
//! matching hermes-agent's `_add_line_numbers()` in `file_operations.py`.
//! Line numbers let the LLM reference specific locations in follow-up
//! `patch` calls without guessing offsets.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use edgecrab_types::{ToolError, ToolSchema};

use crate::path_utils::jail_read_path;
use crate::read_tracker;
use crate::registry::{ToolContext, ToolHandler};

pub struct ReadFileTool;

#[derive(Deserialize)]
struct Args {
    path: String,
    #[serde(default)]
    line_start: Option<usize>,
    #[serde(default)]
    line_end: Option<usize>,
    /// Add `  N|` line-number prefix to every output line.
    /// Default true — matches hermes-agent parity and makes patch calls easier.
    #[serde(default = "default_line_numbers")]
    line_numbers: bool,
}

fn default_line_numbers() -> bool {
    true
}

/// Add `  N|` line-number prefixes to `content`, starting at `first_line`.
///
/// Format mirrors hermes-agent's `_add_line_numbers()`:
/// ```text
///     1| line one content
///    42| def foo():
/// ```
/// Long lines are NOT truncated here — the LLM needs the full content for `patch`.
fn add_line_numbers(content: &str, first_line: usize) -> String {
    // Determine width needed (e.g. 3 chars for files ≤ 999 lines)
    let total = first_line + content.lines().count().saturating_sub(1);
    let width = total.to_string().len().max(4);

    content
        .lines()
        .enumerate()
        .map(|(i, line)| format!("{:>width$}| {line}", first_line + i, width = width))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Suggest similar file names when the requested file is not found.
///
/// WHY: LLMs frequently typo or guess file names. Providing nearby candidates
/// (difflib-style, shared 50%+ chars) avoids a wasted round-trip.
/// Mirrors hermes-agent's `_suggest_similar_files()`.
fn suggest_similar_files(path: &str, cwd: &std::path::Path) -> Vec<String> {
    let dir = std::path::Path::new(path)
        .parent()
        .map(|p| {
            if p.components().count() == 0 {
                cwd.to_path_buf()
            } else {
                cwd.join(p)
            }
        })
        .unwrap_or_else(|| cwd.to_path_buf());
    let basename = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
        .to_lowercase();

    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut candidates: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let fname = e.file_name();
            let name = fname.to_str()?;
            let name_lower = name.to_lowercase();
            // Shared character-set overlap ≥ 50% by unique char count.
            let b_chars: std::collections::HashSet<char> = basename.chars().collect();
            let n_chars: std::collections::HashSet<char> = name_lower.chars().collect();
            let common = b_chars.intersection(&n_chars).count();
            if common * 2 >= basename.len().min(name_lower.len()) {
                Some(e.path().display().to_string())
            } else {
                None
            }
        })
        .take(5)
        .collect();

    candidates.sort();
    candidates
}

#[async_trait]
impl ToolHandler for ReadFileTool {
    fn name(&self) -> &'static str {
        "read_file"
    }

    fn toolset(&self) -> &'static str {
        "file"
    }

    fn parallel_safe(&self) -> bool {
        true
    }

    fn emoji(&self) -> &'static str {
        "📄"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "read_file".into(),
            description: "Read the contents of a file. Optionally specify line range. \
                          Returns content with line numbers by default (set line_numbers=false to disable)."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to working directory"
                    },
                    "line_start": {
                        "type": "integer",
                        "description": "Start line (1-indexed, inclusive)"
                    },
                    "line_end": {
                        "type": "integer",
                        "description": "End line (1-indexed, inclusive)"
                    },
                    "line_numbers": {
                        "type": "boolean",
                        "description": "Prefix each line with its line number (default: true)"
                    }
                },
                "required": ["path"]
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
            tool: "read_file".into(),
            message: e.to_string(),
        })?;

        // Image redirect: if the path points to an image file, redirect to vision_analyze.
        // This mirrors hermes-agent file_operations.py:501-509 — prevents the agent from
        // trying to read binary image bytes as text, which always fails.
        {
            let ext = std::path::Path::new(&args.path)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase());
            if matches!(
                ext.as_deref(),
                Some(
                    "png"
                        | "jpg"
                        | "jpeg"
                        | "gif"
                        | "webp"
                        | "bmp"
                        | "tiff"
                        | "tif"
                        | "avif"
                        | "ico"
                )
            ) {
                return Ok(format!(
                    "[IMAGE FILE DETECTED] '{}' is an image — use vision_analyze instead. \
                     Call vision_analyze with image_source='{}' to inspect its contents.",
                    args.path, args.path
                ));
            }
        }

        let path_policy = ctx.config.file_path_policy(&ctx.cwd);

        // Resolve and jail path using the shared path helper (SRP — security in one place)
        let resolved = match jail_read_path(&args.path, &path_policy) {
            Ok(p) => p,
            Err(e) => {
                // File not found → suggest similar file names, mirrors hermes-agent.
                if matches!(&e, ToolError::NotFound(_)) {
                    let suggestions = suggest_similar_files(&args.path, &ctx.cwd);
                    if !suggestions.is_empty() {
                        return Err(ToolError::NotFound(format!(
                            "{e}\nSimilar files found:\n{}",
                            suggestions.join("\n")
                        )));
                    }
                }
                return Err(e);
            }
        };

        // Size check
        let metadata = tokio::fs::metadata(&resolved)
            .await
            .map_err(|e| ToolError::Other(format!("Cannot stat '{}': {}", args.path, e)))?;

        if metadata.len() as usize > ctx.config.max_file_read_bytes {
            return Err(ToolError::Other(format!(
                "File too large ({} bytes, max {}). Use line_start/line_end to read a section.",
                metadata.len(),
                ctx.config.max_file_read_bytes
            )));
        }

        let content = tokio::fs::read_to_string(&resolved)
            .await
            .map_err(|e| ToolError::Other(format!("Cannot read '{}': {}", args.path, e)))?;

        // Apply line range filter.
        // `first_line` tracks the 1-based line number of the first output line
        // so that add_line_numbers() produces correct absolute numbers even for
        // partial reads (e.g. line_start=100 → output starts at "  100|").
        let (output, first_line) = match (args.line_start, args.line_end) {
            (Some(start), Some(end)) => {
                let lines: Vec<&str> = content.lines().collect();
                let start_idx = start.saturating_sub(1); // 1-indexed to 0-indexed
                let end_idx = end.min(lines.len());
                if start_idx >= lines.len() {
                    return Ok(format!("(empty — file has {} lines)", lines.len()));
                }
                (lines[start_idx..end_idx].join("\n"), start)
            }
            (Some(start), None) => {
                let lines: Vec<&str> = content.lines().collect();
                let start_idx = start.saturating_sub(1);
                if start_idx >= lines.len() {
                    return Ok(format!("(empty — file has {} lines)", lines.len()));
                }
                (lines[start_idx..].join("\n"), start)
            }
            _ => (content, 1usize),
        };

        // Apply line-number prefixes when requested (default: true).
        let output = if args.line_numbers && !output.is_empty() {
            add_line_numbers(&output, first_line)
        } else {
            output
        };

        // Consecutive re-read loop detection — mirrors hermes-agent file_tools.py.
        // Warn at 3 identical consecutive reads; hard-block at 4.
        let key = read_tracker::read_key(&args.path, args.line_start, args.line_end);
        let count = read_tracker::check_and_update(&ctx.session_id, key);

        if count >= 4 {
            return Err(ToolError::Other(format!(
                "BLOCKED: You have read '{}' (lines {:?}–{:?}) {} times in a row. \
                 The content has NOT changed. You already have this information. \
                 Stop re-reading and proceed with your task.",
                args.path, args.line_start, args.line_end, count
            )));
        } else if count >= 3 {
            let warning = format!(
                "[WARNING: You have read this exact region {} times consecutively. \
                 The content has not changed since your last read. \
                 If you are stuck in a loop, stop reading and proceed.]\n",
                count
            );
            return Ok(warning + &output);
        }

        Ok(output)
    }
}

// Compile-time registration
inventory::submit!(&ReadFileTool as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn ctx_in(dir: &std::path::Path) -> ToolContext {
        let mut ctx = ToolContext::test_context();
        ctx.cwd = dir.to_path_buf();
        ctx
    }

    #[tokio::test]
    async fn read_file_basic() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("test.txt"), "line1\nline2\nline3\n").expect("write");

        let ctx = ctx_in(dir.path());
        let result = ReadFileTool
            .execute(json!({"path": "test.txt"}), &ctx)
            .await
            .expect("read");

        assert!(result.contains("line1"));
        assert!(result.contains("line3"));
    }

    #[tokio::test]
    async fn read_file_line_range() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("test.txt"), "a\nb\nc\nd\ne\n").expect("write");

        let ctx = ctx_in(dir.path());
        // Disable line numbers for an exact content equality check
        let result = ReadFileTool
            .execute(
                json!({"path": "test.txt", "line_start": 2, "line_end": 4, "line_numbers": false}),
                &ctx,
            )
            .await
            .expect("read");

        assert_eq!(result, "b\nc\nd");
    }

    #[tokio::test]
    async fn read_file_line_numbers_are_on_by_default() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("nums.txt"), "first\nsecond\nthird\n").expect("write");

        let ctx = ctx_in(dir.path());
        let result = ReadFileTool
            .execute(json!({"path": "nums.txt"}), &ctx)
            .await
            .expect("read");

        // Default output should contain `1|` prefix
        assert!(
            result.contains("1|"),
            "expected line number prefix, got: {result}"
        );
        assert!(result.contains("first"), "expected content");
    }

    #[tokio::test]
    async fn read_file_line_range_numbers_are_absolute() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("abs.txt"), "l1\nl2\nl3\nl4\nl5\n").expect("write");

        let ctx = ctx_in(dir.path());
        // Read starting at line 3 — the prefix should say "3|" not "1|"
        let result = ReadFileTool
            .execute(
                json!({"path": "abs.txt", "line_start": 3, "line_end": 5}),
                &ctx,
            )
            .await
            .expect("read");

        assert!(
            result.contains("3|"),
            "expected absolute line number 3, got: {result}"
        );
        assert!(
            result.contains("l3"),
            "expected line content l3, got: {result}"
        );
    }

    #[tokio::test]
    async fn read_file_path_traversal_blocked() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        let result = ReadFileTool
            .execute(json!({"path": "../../../etc/passwd"}), &ctx)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn read_file_missing_file() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        let result = ReadFileTool
            .execute(json!({"path": "nonexistent.txt"}), &ctx)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn read_file_image_redirects_to_vision_analyze() {
        let dir = TempDir::new().expect("tmpdir");
        // Write a fake PNG (doesn't need valid bytes — redirect happens before read)
        std::fs::write(dir.path().join("screenshot.png"), b"\x89PNG\r\n").expect("write");
        let ctx = ctx_in(dir.path());

        let result = ReadFileTool
            .execute(json!({"path": "screenshot.png"}), &ctx)
            .await
            .expect("should return redirect message, not error");

        assert!(
            result.contains("vision_analyze"),
            "expected redirect to vision_analyze, got: {result}"
        );
        assert!(
            result.contains("screenshot.png"),
            "expected path in message"
        );
    }

    #[tokio::test]
    async fn read_file_image_redirect_covers_common_extensions() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        for ext in ["jpg", "jpeg", "gif", "webp", "bmp", "tiff", "avif", "ico"] {
            let fname = format!("img.{ext}");
            std::fs::write(dir.path().join(&fname), b"fake").expect("write");
            let result = ReadFileTool
                .execute(json!({"path": fname}), &ctx)
                .await
                .expect("redirect expected");
            assert!(
                result.contains("vision_analyze"),
                "ext={ext} should redirect"
            );
        }
    }

    #[tokio::test]
    async fn read_file_warns_on_third_consecutive_read() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("loop.txt"), "some content").expect("write");

        // Each test needs a unique session_id to avoid cross-test interference
        let mut ctx = ctx_in(dir.path());
        ctx.session_id = format!("test-warn-{}", uuid::Uuid::new_v4());

        let args = json!({"path": "loop.txt"});

        // Reads 1 and 2 — normal result, no warning
        let r1 = ReadFileTool.execute(args.clone(), &ctx).await.expect("r1");
        assert!(!r1.contains("WARNING"), "read 1 should not warn");
        let r2 = ReadFileTool.execute(args.clone(), &ctx).await.expect("r2");
        assert!(!r2.contains("WARNING"), "read 2 should not warn");

        // Read 3 — warning prepended
        let r3 = ReadFileTool.execute(args.clone(), &ctx).await.expect("r3");
        assert!(
            r3.contains("WARNING"),
            "read 3 should contain warning, got: {r3}"
        );
        assert!(
            r3.contains("some content"),
            "content still present with warning"
        );
    }

    #[tokio::test]
    async fn read_file_blocks_on_fourth_consecutive_read() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("blocked.txt"), "content").expect("write");

        let mut ctx = ctx_in(dir.path());
        ctx.session_id = format!("test-block-{}", uuid::Uuid::new_v4());

        let args = json!({"path": "blocked.txt"});

        // Reads 1-3 allowed
        for _ in 0..3 {
            let _ = ReadFileTool.execute(args.clone(), &ctx).await;
        }

        // Read 4 must return an error
        let r4 = ReadFileTool.execute(args.clone(), &ctx).await;
        assert!(r4.is_err(), "read 4 should be BLOCKED");
        let msg = r4.unwrap_err().to_string();
        assert!(
            msg.contains("BLOCKED"),
            "error should say BLOCKED, got: {msg}"
        );
    }

    #[tokio::test]
    async fn read_file_other_tool_resets_counter() {
        // Simulate: read 3x → warning; then "other tool" fires → counter resets;
        // next read should be count=1 again (no warning).
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("reset.txt"), "data").expect("write");

        let mut ctx = ctx_in(dir.path());
        ctx.session_id = format!("test-reset-{}", uuid::Uuid::new_v4());

        let args = json!({"path": "reset.txt"});

        // 3 reads (third triggers warning)
        for _ in 0..3 {
            let _ = ReadFileTool.execute(args.clone(), &ctx).await;
        }
        // Simulate another tool calling notify (e.g., write_file)
        crate::read_tracker::notify_other_tool_call(&ctx.session_id);

        // Next read should be count=1, no warning
        let r = ReadFileTool
            .execute(args.clone(), &ctx)
            .await
            .expect("after reset");
        assert!(
            !r.contains("WARNING"),
            "after reset read should not warn, got: {r}"
        );
    }

    #[tokio::test]
    async fn read_file_allows_absolute_path_in_configured_allowed_root() {
        let dir = TempDir::new().expect("workspace");
        let extra = TempDir::new().expect("extra");
        let extra_file = extra.path().join("shared.txt");
        std::fs::write(&extra_file, "shared").expect("write");

        let mut ctx = ctx_in(dir.path());
        ctx.config.file_allowed_roots = vec![extra.path().to_path_buf()];

        let result = ReadFileTool
            .execute(
                json!({"path": extra_file.to_string_lossy(), "line_numbers": false}),
                &ctx,
            )
            .await
            .expect("read");

        assert_eq!(result, "shared");
    }

    #[tokio::test]
    async fn read_file_maps_absolute_tmp_into_edgecrab_temp_root() {
        let dir = TempDir::new().expect("workspace");
        let edgecrab_home = TempDir::new().expect("edgecrab_home");
        let mapped = edgecrab_home.path().join("tmp/files/summary.md");
        std::fs::create_dir_all(mapped.parent().expect("tmp parent")).expect("create tmp parent");
        std::fs::write(&mapped, "tmp contents").expect("write mapped tmp");

        let mut ctx = ctx_in(dir.path());
        ctx.config.edgecrab_home = edgecrab_home.path().to_path_buf();

        let result = ReadFileTool
            .execute(
                json!({"path": "/tmp/summary.md", "line_numbers": false}),
                &ctx,
            )
            .await
            .expect("read virtual tmp");

        assert_eq!(result, "tmp contents");
    }
}
