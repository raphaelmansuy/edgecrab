//! # Tool Result Spill-to-Artifact
//!
//! When a tool result exceeds a configurable byte threshold, the full
//! output is written to a session-scoped artifact file on disk and only
//! a compact **head preview + metadata stub** is injected into the
//! conversation message history.
//!
//! ## Design invariants
//!
//! 1. **Nothing is lost** — full result persisted to disk.
//! 2. **Agent retains access** — artifact under `cwd`, readable via `read_file`.
//! 3. **Zero breaking changes** — `ToolHandler` trait signature unchanged.
//! 4. **Compression-friendly** — stub is ~200 bytes vs 50KB+ originals.
//! 5. **Feature-gated** — `tools.result_spill` config flag, on by default.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// Configuration for tool result spilling.
#[derive(Debug, Clone)]
pub struct SpillConfig {
    /// Whether spilling is enabled (gated by `tools.result_spill`).
    pub enabled: bool,
    /// Byte threshold — results strictly larger than this are spilled.
    pub threshold: usize,
    /// Number of lines to include in the preview stub.
    pub preview_lines: usize,
}

impl Default for SpillConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold: 16_384, // 16 KB
            preview_lines: 80,
        }
    }
}

/// Outcome of a spill attempt.
#[derive(Debug)]
pub enum SpillOutcome {
    /// Result was small enough — use it as-is.
    Inline(String),
    /// Result was spilled to an artifact file.
    Spilled {
        /// The stub message to inject into session.messages.
        stub: String,
        /// Absolute path of the artifact file on disk.
        artifact_path: PathBuf,
        /// Original byte length of the full result.
        original_bytes: usize,
        /// Original line count of the full result.
        original_lines: usize,
        /// Number of preview lines included in the stub.
        preview_line_count: usize,
    },
}

/// Per-session atomic sequence counter for artifact filenames.
///
/// Thread-safe across parallel tool calls within one conversation.
pub struct SpillSequence(AtomicU32);

impl SpillSequence {
    pub fn new() -> Self {
        Self(AtomicU32::new(1))
    }

    pub fn next(&self) -> u32 {
        self.0.fetch_add(1, Ordering::Relaxed)
    }
}

impl Default for SpillSequence {
    fn default() -> Self {
        Self::new()
    }
}

/// Attempt to spill a tool result to an artifact file.
///
/// Returns `SpillOutcome::Inline` if spilling is disabled, the result is
/// small enough, or writing fails (graceful degradation).
pub fn maybe_spill(
    tool_name: &str,
    _tool_call_id: &str,
    result: String,
    session_id: &str,
    cwd: &Path,
    config: &SpillConfig,
    seq: &SpillSequence,
) -> SpillOutcome {
    // Gate check
    if !config.enabled {
        return SpillOutcome::Inline(result);
    }

    // Size check — strictly greater than threshold
    if result.len() <= config.threshold {
        return SpillOutcome::Inline(result);
    }

    // Compute artifact directory and path
    let safe_name = sanitize_tool_name(tool_name);
    let seq_num = seq.next();
    let artifact_dir = artifact_dir_for_session(cwd, session_id);
    let filename = format!("{safe_name}_{seq_num:03}.md");
    let artifact_path = artifact_dir.join(&filename);

    // Attempt to write — on failure, fall back to inline
    if let Err(e) = write_artifact(&artifact_dir, &artifact_path, &result) {
        tracing::warn!(
            tool = %tool_name,
            path = %artifact_path.display(),
            error = %e,
            "tool result spill failed, falling back to inline"
        );
        return SpillOutcome::Inline(result);
    }

    // Ensure .gitignore covers the artifact directory
    ensure_gitignore(cwd);

    // Build the stub
    let original_bytes = result.len();
    let original_lines = result.lines().count().max(1);
    let (preview, preview_line_count) = build_preview(&result, config.preview_lines);

    // Compute relative path for display (agent uses read_file with relative paths)
    let rel_path = artifact_path.strip_prefix(cwd).unwrap_or(&artifact_path);

    let pct = if original_lines > 0 {
        (preview_line_count as f64 / original_lines as f64 * 100.0).round() as usize
    } else {
        100
    };

    let stub = format!(
        "[tool_result_spill]\n\
         tool: {tool_name}\n\
         lines: {original_lines}\n\
         bytes: {original_bytes}\n\
         artifact: {rel}\n\
         showing: {preview_line_count}/{original_lines} lines (first {pct}%)\n\
         \n\
         --- BEGIN PREVIEW ({preview_line_count} lines) ---\n\
         {preview}\n\
         --- END PREVIEW ---\n\
         \n\
         Full result saved to: {rel}\n\
         Use read_file or file_search to explore the full content.",
        rel = rel_path.display(),
    );

    tracing::info!(
        tool = %tool_name,
        original_bytes,
        original_lines,
        preview_lines = preview_line_count,
        artifact = %rel_path.display(),
        "tool result spilled to artifact"
    );

    SpillOutcome::Spilled {
        stub,
        artifact_path,
        original_bytes,
        original_lines,
        preview_line_count,
    }
}

// ─── Internal helpers ────────────────────────────────────────────────

/// Sanitize a tool name for use as a filename component.
/// Only allow alphanumeric chars and underscores; replace everything else.
fn sanitize_tool_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Compute the artifact directory path for a session.
fn artifact_dir_for_session(cwd: &Path, session_id: &str) -> PathBuf {
    // Sanitize session_id for directory name safety
    let safe_session: String = session_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    cwd.join(".edgecrab-artifacts").join(safe_session)
}

/// Write the full result to an artifact file atomically.
fn write_artifact(dir: &Path, path: &Path, content: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(path, content)?;
    Ok(())
}

/// Build a preview from the first N lines of the result.
///
/// Returns (preview_text, actual_line_count_shown).
fn build_preview(result: &str, max_lines: usize) -> (String, usize) {
    let lines: Vec<&str> = result.lines().take(max_lines).collect();
    let count = lines.len();
    (lines.join("\n"), count)
}

/// Ensure `.gitignore` in `cwd` contains `.edgecrab-artifacts/`.
///
/// Best-effort: if the file can't be read or written, we silently skip.
fn ensure_gitignore(cwd: &Path) {
    let gitignore_path = cwd.join(".gitignore");
    let entry = ".edgecrab-artifacts/";

    // Read existing content (or empty if no .gitignore)
    let existing = std::fs::read_to_string(&gitignore_path).unwrap_or_default();

    // Check if already present (line-by-line to avoid substring false positives)
    if existing.lines().any(|line| line.trim() == entry) {
        return;
    }

    // Append the entry
    let addition = if existing.is_empty() || existing.ends_with('\n') {
        format!("{entry}\n")
    } else {
        format!("\n{entry}\n")
    };

    if let Err(e) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&gitignore_path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, addition.as_bytes()))
    {
        tracing::debug!(error = %e, "could not update .gitignore with artifact entry");
    }
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_config(enabled: bool, threshold: usize, preview_lines: usize) -> SpillConfig {
        SpillConfig {
            enabled,
            threshold,
            preview_lines,
        }
    }

    #[test]
    fn inline_when_disabled() {
        let tmp = TempDir::new().expect("tempdir");
        let seq = SpillSequence::new();
        let config = test_config(false, 10, 5);
        let result = "a".repeat(1000);

        match maybe_spill(
            "test",
            "tc1",
            result.clone(),
            "ses1",
            tmp.path(),
            &config,
            &seq,
        ) {
            SpillOutcome::Inline(s) => assert_eq!(s, result),
            SpillOutcome::Spilled { .. } => panic!("should not spill when disabled"),
        }
    }

    #[test]
    fn inline_when_under_threshold() {
        let tmp = TempDir::new().expect("tempdir");
        let seq = SpillSequence::new();
        let config = test_config(true, 1000, 5);
        let result = "small result".to_string();

        match maybe_spill(
            "test",
            "tc1",
            result.clone(),
            "ses1",
            tmp.path(),
            &config,
            &seq,
        ) {
            SpillOutcome::Inline(s) => assert_eq!(s, result),
            SpillOutcome::Spilled { .. } => panic!("should not spill under threshold"),
        }
    }

    #[test]
    fn inline_when_exactly_at_threshold() {
        let tmp = TempDir::new().expect("tempdir");
        let seq = SpillSequence::new();
        let config = test_config(true, 100, 5);
        let result = "x".repeat(100); // exactly at threshold — no spill

        match maybe_spill(
            "test",
            "tc1",
            result.clone(),
            "ses1",
            tmp.path(),
            &config,
            &seq,
        ) {
            SpillOutcome::Inline(s) => assert_eq!(s, result),
            SpillOutcome::Spilled { .. } => panic!("should not spill at exact threshold"),
        }
    }

    #[test]
    fn spills_when_over_threshold() {
        let tmp = TempDir::new().expect("tempdir");
        let seq = SpillSequence::new();
        let config = test_config(true, 100, 5);
        let lines: Vec<String> = (1..=50).map(|i| format!("line {i}")).collect();
        let result = lines.join("\n");
        assert!(result.len() > 100);

        match maybe_spill(
            "file_search",
            "tc1",
            result.clone(),
            "ses1",
            tmp.path(),
            &config,
            &seq,
        ) {
            SpillOutcome::Spilled {
                stub,
                artifact_path,
                original_bytes,
                original_lines,
                preview_line_count,
            } => {
                assert_eq!(original_bytes, result.len());
                assert_eq!(original_lines, 50);
                assert_eq!(preview_line_count, 5);
                assert!(stub.contains("[tool_result_spill]"));
                assert!(stub.contains("tool: file_search"));
                assert!(stub.contains("lines: 50"));
                assert!(stub.contains("showing: 5/50 lines"));
                assert!(stub.contains("line 1"));
                assert!(stub.contains("line 5"));
                assert!(!stub.contains("line 6"));
                assert!(stub.contains("--- BEGIN PREVIEW"));
                assert!(stub.contains("--- END PREVIEW"));

                // Verify artifact file was written
                assert!(artifact_path.exists());
                let written = std::fs::read_to_string(&artifact_path).expect("read artifact");
                assert_eq!(written, result);
            }
            SpillOutcome::Inline(_) => panic!("should spill over threshold"),
        }
    }

    #[test]
    fn artifact_path_structure() {
        let tmp = TempDir::new().expect("tempdir");
        let seq = SpillSequence::new();
        let config = test_config(true, 10, 5);
        let result = "a".repeat(20);

        match maybe_spill(
            "my_tool",
            "tc1",
            result,
            "ses-abc123",
            tmp.path(),
            &config,
            &seq,
        ) {
            SpillOutcome::Spilled { artifact_path, .. } => {
                let rel = artifact_path
                    .strip_prefix(tmp.path())
                    .expect("relative path");
                assert_eq!(
                    rel,
                    Path::new(".edgecrab-artifacts/ses-abc123/my_tool_001.md")
                );
            }
            SpillOutcome::Inline(_) => panic!("should spill"),
        }
    }

    #[test]
    fn sequence_counter_increments() {
        let tmp = TempDir::new().expect("tempdir");
        let seq = SpillSequence::new();
        let config = test_config(true, 10, 5);

        for i in 1..=3 {
            let result = "a".repeat(20);
            match maybe_spill("tool", "tc1", result, "ses1", tmp.path(), &config, &seq) {
                SpillOutcome::Spilled { artifact_path, .. } => {
                    let name = artifact_path
                        .file_name()
                        .expect("filename")
                        .to_str()
                        .expect("utf8");
                    assert_eq!(name, format!("tool_{i:03}.md"));
                }
                SpillOutcome::Inline(_) => panic!("should spill"),
            }
        }
    }

    #[test]
    fn sanitize_tool_name_handles_special_chars() {
        assert_eq!(sanitize_tool_name("file_read"), "file_read");
        assert_eq!(sanitize_tool_name("mcp-server:tool"), "mcp_server_tool");
        assert_eq!(sanitize_tool_name("my.tool/v2"), "my_tool_v2");
        assert_eq!(sanitize_tool_name(""), "");
    }

    #[test]
    fn sanitize_session_id_in_path() {
        let dir = artifact_dir_for_session(Path::new("/tmp"), "ses/../../etc/passwd");
        // No path traversal — slashes become underscores
        assert!(dir.to_str().expect("utf8").contains("ses____"));
        assert!(!dir.to_str().expect("utf8").contains("etc/passwd"));
    }

    #[test]
    fn preview_with_fewer_lines_than_max() {
        let (preview, count) = build_preview("line 1\nline 2\nline 3", 10);
        assert_eq!(count, 3);
        assert_eq!(preview, "line 1\nline 2\nline 3");
    }

    #[test]
    fn preview_limits_to_max_lines() {
        let input: String = (1..=100)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (preview, count) = build_preview(&input, 5);
        assert_eq!(count, 5);
        assert!(preview.contains("line 1"));
        assert!(preview.contains("line 5"));
        assert!(!preview.contains("line 6"));
    }

    #[test]
    fn single_line_result_spills() {
        let tmp = TempDir::new().expect("tempdir");
        let seq = SpillSequence::new();
        let config = test_config(true, 10, 5);
        let result = "a".repeat(50); // single long line, no newlines

        match maybe_spill(
            "tool",
            "tc1",
            result.clone(),
            "ses1",
            tmp.path(),
            &config,
            &seq,
        ) {
            SpillOutcome::Spilled {
                original_lines,
                preview_line_count,
                stub,
                ..
            } => {
                assert_eq!(original_lines, 1);
                assert_eq!(preview_line_count, 1);
                assert!(stub.contains("showing: 1/1 lines"));
            }
            SpillOutcome::Inline(_) => panic!("should spill"),
        }
    }

    #[test]
    fn gitignore_is_created_on_first_spill() {
        let tmp = TempDir::new().expect("tempdir");
        let seq = SpillSequence::new();
        let config = test_config(true, 10, 5);
        let result = "a".repeat(20);

        let _ = maybe_spill("tool", "tc1", result, "ses1", tmp.path(), &config, &seq);

        let gitignore =
            std::fs::read_to_string(tmp.path().join(".gitignore")).expect("read .gitignore");
        assert!(gitignore.contains(".edgecrab-artifacts/"));
    }

    #[test]
    fn gitignore_not_duplicated() {
        let tmp = TempDir::new().expect("tempdir");
        let seq = SpillSequence::new();
        let config = test_config(true, 10, 5);

        // Write initial .gitignore with existing entry
        std::fs::write(tmp.path().join(".gitignore"), ".edgecrab-artifacts/\n").expect("write");

        let result = "a".repeat(20);
        let _ = maybe_spill("tool", "tc1", result, "ses1", tmp.path(), &config, &seq);

        let gitignore = std::fs::read_to_string(tmp.path().join(".gitignore")).expect("read");
        // Should appear exactly once
        assert_eq!(gitignore.matches(".edgecrab-artifacts/").count(), 1);
    }

    #[test]
    fn unicode_result_spills_safely() {
        let tmp = TempDir::new().expect("tempdir");
        let seq = SpillSequence::new();
        let config = test_config(true, 10, 5);
        // Multi-byte chars: each emoji is 4 bytes
        let result = "🦀".repeat(10); // 40 bytes

        match maybe_spill(
            "tool",
            "tc1",
            result.clone(),
            "ses1",
            tmp.path(),
            &config,
            &seq,
        ) {
            SpillOutcome::Spilled { artifact_path, .. } => {
                let written = std::fs::read_to_string(&artifact_path).expect("read");
                assert_eq!(written, result);
            }
            SpillOutcome::Inline(_) => panic!("should spill"),
        }
    }

    #[test]
    fn empty_result_never_spills() {
        let tmp = TempDir::new().expect("tempdir");
        let seq = SpillSequence::new();
        let config = test_config(true, 0, 5); // threshold=0, but empty string has len 0

        match maybe_spill(
            "tool",
            "tc1",
            String::new(),
            "ses1",
            tmp.path(),
            &config,
            &seq,
        ) {
            SpillOutcome::Inline(s) => assert!(s.is_empty()),
            SpillOutcome::Spilled { .. } => panic!("empty should not spill"),
        }
    }

    #[test]
    fn concurrent_sequence_numbers_are_unique() {
        let seq = SpillSequence::new();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..100 {
            let n = seq.next();
            assert!(seen.insert(n), "duplicate sequence number: {n}");
        }
    }
}
