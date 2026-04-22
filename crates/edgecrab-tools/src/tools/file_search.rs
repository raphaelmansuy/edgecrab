//! # search_files — Ripgrep-style recursive file search
//!
//! WHY regex search: Finding code references, function definitions, and
//! usage patterns is the #1 tool for code understanding. Using Rust's
//! regex crate gives near-ripgrep performance without spawning a subprocess.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use edgecrab_types::{ToolError, ToolSchema};

use crate::path_utils::jail_read_path;
use crate::read_tracker;
use crate::registry::{ToolContext, ToolHandler};

pub struct SearchFilesTool;

#[derive(Deserialize)]
struct Args {
    pattern: String,
    #[serde(default = "default_target")]
    target: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    include: Option<String>,
    #[serde(default)]
    file_glob: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default = "default_output_mode")]
    output_mode: String,
    #[serde(default)]
    context: Option<usize>,
    #[serde(default = "default_max_results")]
    max_results: usize,
}

fn default_max_results() -> usize {
    50
}

fn default_target() -> String {
    "content".into()
}

fn default_output_mode() -> String {
    "content".into()
}

#[async_trait]
impl ToolHandler for SearchFilesTool {
    fn name(&self) -> &'static str {
        "search_files"
    }

    fn toolset(&self) -> &'static str {
        "file"
    }

    fn parallel_safe(&self) -> bool {
        true
    }

    fn emoji(&self) -> &'static str {
        "🔍"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "search_files".into(),
            description: "Search file contents or find files by name. Use this instead of grep/rg/find/ls in terminal. \
                          Content search (target='content') supports regex, pagination, file filtering, and output modes. \
                          File search (target='files') finds files by glob-like pattern.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Regex pattern for content search, or file-name glob pattern for file search"
                    },
                    "target": {
                        "type": "string",
                        "enum": ["content", "files"],
                        "description": "content = search inside files, files = list matching file paths",
                        "default": "content"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search in (default: working directory)"
                    },
                    "include": {
                        "type": "string",
                        "description": "File glob pattern to include (e.g., '*.rs', '*.py')"
                    },
                    "file_glob": {
                        "type": "string",
                        "description": "Backward-compatible alias for include"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results to return (Hermes-compatible alias for `max_results`; takes precedence over `max_results` when both are set, max: 200)"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Skip the first N matches or file paths"
                    },
                    "output_mode": {
                        "type": "string",
                        "enum": ["content", "files_only", "count"],
                        "description": "For content search: full lines, matching file paths only, or per-file match counts",
                        "default": "content"
                    },
                    "context": {
                        "type": "integer",
                        "description": "Number of surrounding lines to include around each content match",
                        "default": 0
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default: 50, max: 200). Use `limit` as an alias."
                    }
                },
                "required": ["pattern"]
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
            tool: "search_files".into(),
            message: e.to_string(),
        })?;

        let path_policy = ctx.config.file_path_policy(&ctx.cwd);
        let search_root = match &args.path {
            Some(p) => jail_read_path(p, &path_policy)?,
            None => ctx.cwd.clone(),
        };
        let include_glob = args.include.clone().or(args.file_glob.clone());
        let include_glob_for_key = include_glob.clone();
        let pattern_for_key = args.pattern.clone();
        let max = args.limit.unwrap_or(args.max_results).min(200);
        let offset = args.offset.unwrap_or(0);
        let context = args.context.unwrap_or(0).min(20);
        let target = args.target.to_ascii_lowercase();
        let output_mode = args.output_mode.to_ascii_lowercase();
        let cwd = ctx.cwd.clone();

        let (raw_text, total_matches, returned_matches) = if target == "files" {
            let pattern = args.pattern.clone();
            let include_glob_for_walk = include_glob.clone();
            let matches = tokio::task::spawn_blocking(move || {
                let mut hits = Vec::new();
                walk_and_find_files(
                    &search_root,
                    &pattern,
                    &include_glob_for_walk,
                    &cwd,
                    &mut hits,
                );
                hits.sort();
                hits
            })
            .await
            .map_err(|e| ToolError::Other(format!("Search task failed: {}", e)))?;

            format_file_results(matches, offset, max)
        } else {
            let regex = regex::Regex::new(&args.pattern).map_err(|e| ToolError::InvalidArgs {
                tool: "search_files".into(),
                message: format!("Invalid regex: {}", e),
            })?;
            let include_glob_for_walk = include_glob.clone();

            let matches = tokio::task::spawn_blocking(move || {
                let mut hits = Vec::new();
                walk_and_search(
                    &search_root,
                    &regex,
                    &include_glob_for_walk,
                    &cwd,
                    context,
                    &mut hits,
                );
                hits
            })
            .await
            .map_err(|e| ToolError::Other(format!("Search task failed: {}", e)))?;

            format_content_results(matches, &output_mode, offset, max)
        };

        // Build a machine-readable pagination summary prepended to the output.
        // This prevents re-search loops: the agent can see `has_more=true` and
        // knows to use `offset` to paginate rather than blindly re-running.
        // Cost: ~10-15 tokens per call; benefit: avoids 50-200+ token re-searches.
        let has_more = offset + returned_matches < total_matches;
        let summary = if has_more {
            format!(
                "[search_result returned={returned_matches} total={total_matches} has_more=true next_offset={}]",
                offset + returned_matches
            )
        } else {
            format!(
                "[search_result returned={returned_matches} total={total_matches} has_more=false]"
            )
        };
        let output = format!("{summary}\n{raw_text}");

        // Consecutive re-search loop detection — mirrors hermes-agent file_tools.py.
        // Warn at 3 identical consecutive searches; hard-block at 4.
        let key = read_tracker::search_key(
            &pattern_for_key,
            args.path.as_deref(),
            include_glob_for_key.as_deref(),
            max,
        );
        let repeat = read_tracker::check_and_update(&ctx.session_id, key);

        if repeat >= 4 {
            return Err(ToolError::Other(format!(
                "BLOCKED: You have run this exact search {} times in a row. \
                 The results have NOT changed. You already have this information. \
                 Stop re-searching and proceed with your task.",
                repeat
            )));
        } else if repeat >= 3 {
            let warning = format!(
                "[WARNING: You have run this exact search {} times consecutively. \
                 The results have not changed. Use the information you already have.]\n",
                repeat
            );
            return Ok(warning + &output);
        }

        Ok(output)
    }
}

/// Recursively walk directories and search file contents.
fn walk_and_search(
    dir: &std::path::Path,
    regex: &regex::Regex,
    include_glob: &Option<String>,
    cwd: &std::path::Path,
    context: usize,
    results: &mut Vec<(String, usize, String, usize)>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();

        // Skip hidden dirs and common large dirs
        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && (name.starts_with('.')
                || name == "node_modules"
                || name == "target"
                || name == "__pycache__")
        {
            continue;
        }

        if path.is_dir() {
            walk_and_search(&path, regex, include_glob, cwd, context, results);
        } else if path.is_file() {
            // Check glob filter
            if let Some(glob) = include_glob
                && let Some(name) = path.file_name().and_then(|n| n.to_str())
                && !simple_glob_match(glob, name)
            {
                continue;
            }

            // Read and search (skip binary files)
            if let Ok(content) = std::fs::read_to_string(&path) {
                let lines: Vec<&str> = content.lines().collect();
                let rel_path = path
                    .strip_prefix(cwd)
                    .unwrap_or(&path)
                    .display()
                    .to_string();

                for (i, line) in lines.iter().enumerate() {
                    if regex.is_match(line) {
                        let snippet = if context == 0 {
                            if line.len() > 200 {
                                format!("{}...", crate::safe_truncate(line, 200))
                            } else {
                                (*line).to_string()
                            }
                        } else {
                            let start = i.saturating_sub(context);
                            let end = (i + context + 1).min(lines.len());
                            lines[start..end]
                                .iter()
                                .enumerate()
                                .map(|(delta, text)| {
                                    let line_no = start + delta + 1;
                                    format!("{line_no}: {}", text)
                                })
                                .collect::<Vec<_>>()
                                .join("\n")
                        };
                        results.push((rel_path.clone(), i + 1, snippet, 1));
                    }
                }
            }
        }
    }
}

fn walk_and_find_files(
    dir: &std::path::Path,
    pattern: &str,
    include_glob: &Option<String>,
    cwd: &std::path::Path,
    results: &mut Vec<String>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && (name.starts_with('.')
                || name == "node_modules"
                || name == "target"
                || name == "__pycache__")
        {
            continue;
        }

        if path.is_dir() {
            walk_and_find_files(&path, pattern, include_glob, cwd, results);
        } else if path.is_file() {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if let Some(glob) = include_glob
                && !simple_glob_match(glob, name)
            {
                continue;
            }
            if simple_glob_match(pattern, name) || name.contains(pattern) {
                results.push(
                    path.strip_prefix(cwd)
                        .unwrap_or(&path)
                        .display()
                        .to_string(),
                );
            }
        }
    }
}

/// Returns `(text, total_before_limit, returned_count)`.
///
/// `total_before_limit` is the number of raw matches collected before paging.
/// `returned_count` is the number of items actually present in `text` (after
/// applying `offset` + `limit`).  The caller uses these to build a machine-
/// readable `[search_result ...]` summary that lets the agent decide whether
/// to paginate rather than blindly re-running the same query.
fn format_file_results(
    matches: Vec<String>,
    offset: usize,
    limit: usize,
) -> (String, usize, usize) {
    let total = matches.len();
    let page: Vec<String> = matches.into_iter().skip(offset).take(limit).collect();
    let returned = page.len();
    let text = if page.is_empty() {
        "No matches found.".to_string()
    } else {
        page.join("\n")
    };
    (text, total, returned)
}

/// Returns `(text, total_before_limit, returned_count)`.
///
/// `total_before_limit` is the raw match count before paging.  For
/// `files_only` and `count` modes this is the raw hit count (not the
/// deduplicated file count) — useful as a relative signal; the agent cares
/// mainly about `has_more`, which is computed from total vs offset+returned.
/// `returned_count` is the number of entries in the output text.
fn format_content_results(
    matches: Vec<(String, usize, String, usize)>,
    output_mode: &str,
    offset: usize,
    limit: usize,
) -> (String, usize, usize) {
    if matches.is_empty() {
        return ("No matches found.".to_string(), 0, 0);
    }

    let total = matches.len();

    match output_mode {
        "files_only" => {
            let mut files = Vec::<String>::new();
            for (path, _, _, _) in matches {
                if !files.contains(&path) {
                    files.push(path);
                }
            }
            let page: Vec<String> = files.into_iter().skip(offset).take(limit).collect();
            let returned = page.len();
            let text = if page.is_empty() {
                "No matches found.".to_string()
            } else {
                page.join("\n")
            };
            (text, total, returned)
        }
        "count" => {
            let mut counts = std::collections::BTreeMap::<String, usize>::new();
            for (path, _, _, count) in matches {
                *counts.entry(path).or_default() += count;
            }
            let page: Vec<String> = counts
                .into_iter()
                .skip(offset)
                .take(limit)
                .map(|(path, count)| format!("{path}: {count}"))
                .collect();
            let returned = page.len();
            let text = if page.is_empty() {
                "No matches found.".to_string()
            } else {
                page.join("\n")
            };
            (text, total, returned)
        }
        _ => {
            let page: Vec<String> = matches
                .into_iter()
                .skip(offset)
                .take(limit)
                .map(|(path, line_num, line, _)| {
                    if line.contains('\n') {
                        format!("{path}:{line_num}:\n{line}")
                    } else {
                        format!("{path}:{line_num}: {line}")
                    }
                })
                .collect();
            let returned = page.len();
            let text = if page.is_empty() {
                "No matches found.".to_string()
            } else {
                page.join("\n")
            };
            (text, total, returned)
        }
    }
}

/// Simple glob matching for file name patterns like "*.rs", "*.py"
fn simple_glob_match(pattern: &str, name: &str) -> bool {
    if let Some(ext) = pattern.strip_prefix("*.") {
        name.ends_with(&format!(".{}", ext))
    } else {
        name == pattern
    }
}

inventory::submit!(&SearchFilesTool as &dyn ToolHandler);

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
    async fn search_basic() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("a.rs"), "fn hello() {}\nfn world() {}\n").expect("w");
        std::fs::write(dir.path().join("b.rs"), "fn other() {}\n").expect("w");

        let ctx = ctx_in(dir.path());
        let result = SearchFilesTool
            .execute(json!({"pattern": "hello"}), &ctx)
            .await
            .expect("search");

        assert!(result.contains("hello"));
        assert!(result.contains("a.rs"));
    }

    #[tokio::test]
    async fn search_with_glob_filter() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("code.rs"), "find_me\n").expect("w");
        std::fs::write(dir.path().join("code.py"), "find_me\n").expect("w");

        let ctx = ctx_in(dir.path());
        let result = SearchFilesTool
            .execute(json!({"pattern": "find_me", "include": "*.rs"}), &ctx)
            .await
            .expect("search");

        assert!(result.contains("code.rs"));
        assert!(!result.contains("code.py"));
    }

    #[tokio::test]
    async fn search_files_mode_lists_matching_paths() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").expect("w");
        std::fs::write(dir.path().join("lib.py"), "print('x')\n").expect("w");

        let ctx = ctx_in(dir.path());
        let result = SearchFilesTool
            .execute(json!({"pattern": "*.rs", "target": "files"}), &ctx)
            .await
            .expect("search");

        assert!(result.contains("main.rs"));
        assert!(!result.contains("lib.py"));
    }

    #[tokio::test]
    async fn search_count_mode_aggregates_matches() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("a.rs"), "needle\nneedle\n").expect("w");

        let ctx = ctx_in(dir.path());
        let result = SearchFilesTool
            .execute(json!({"pattern": "needle", "output_mode": "count"}), &ctx)
            .await
            .expect("search");

        assert!(result.contains("a.rs: 2"));
    }

    #[tokio::test]
    async fn search_no_matches() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("test.txt"), "nothing relevant").expect("w");

        let ctx = ctx_in(dir.path());
        let result = SearchFilesTool
            .execute(json!({"pattern": "zzzzz"}), &ctx)
            .await
            .expect("search");

        assert!(result.contains("No matches"));
    }

    #[tokio::test]
    async fn search_absolute_tmp_uses_edgecrab_temp_root() {
        let dir = TempDir::new().expect("workspace");
        let edgecrab_home = TempDir::new().expect("edgecrab_home");
        let mapped = edgecrab_home.path().join("tmp/files/logs/run.txt");
        std::fs::create_dir_all(mapped.parent().expect("tmp parent")).expect("create tmp parent");
        std::fs::write(&mapped, "needle\n").expect("write mapped tmp");

        let mut ctx = ctx_in(dir.path());
        ctx.config.edgecrab_home = edgecrab_home.path().to_path_buf();

        let result = SearchFilesTool
            .execute(json!({"pattern": "needle", "path": "/tmp"}), &ctx)
            .await
            .expect("search virtual tmp");

        assert!(result.contains("run.txt"));
        assert!(result.contains("needle"));
    }

    #[test]
    fn simple_glob_works() {
        assert!(simple_glob_match("*.rs", "main.rs"));
        assert!(!simple_glob_match("*.rs", "main.py"));
        assert!(simple_glob_match("Makefile", "Makefile"));
        assert!(!simple_glob_match("Makefile", "makefile"));
    }

    // ── Pagination summary header tests ──────────────────────────────────────
    // These tests verify the machine-readable [search_result ...] header that
    // the agent uses to decide whether to paginate or stop searching.

    #[tokio::test]
    async fn search_result_header_present_on_match() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("a.rs"), "needle\n").expect("w");

        let ctx = ctx_in(dir.path());
        let result = SearchFilesTool
            .execute(json!({"pattern": "needle"}), &ctx)
            .await
            .expect("search");

        assert!(
            result.starts_with("[search_result returned="),
            "must start with machine-readable summary header; got: {result}"
        );
        assert!(
            result.contains("has_more=false"),
            "single-page result must report has_more=false; got: {result}"
        );
    }

    #[tokio::test]
    async fn search_result_header_no_matches() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("test.txt"), "nothing here").expect("w");

        let ctx = ctx_in(dir.path());
        let result = SearchFilesTool
            .execute(json!({"pattern": "zzzzz_unique_zzzzz"}), &ctx)
            .await
            .expect("search");

        assert!(
            result.starts_with("[search_result returned=0 total=0 has_more=false]"),
            "zero-match result must report returned=0 total=0; got: {result}"
        );
    }

    #[tokio::test]
    async fn search_result_header_has_more_with_offset() {
        let dir = TempDir::new().expect("tmpdir");
        // Create 5 files that each match "needle"; limit to 2 per page.
        for i in 0..5 {
            std::fs::write(dir.path().join(format!("f{i}.rs")), "needle_unique_abc\n").expect("w");
        }

        let ctx = ctx_in(dir.path());
        // First page: offset=0, limit=2.
        let result = SearchFilesTool
            .execute(
                json!({"pattern": "needle_unique_abc", "limit": 2, "offset": 0}),
                &ctx,
            )
            .await
            .expect("search page1");

        assert!(
            result.contains("has_more=true"),
            "first page of 5 matches with limit=2 must report has_more=true; got: {result}"
        );
        assert!(
            result.contains("next_offset=2"),
            "must report next_offset=2; got: {result}"
        );

        // Second page: offset=2, limit=2 (should still have more).
        let result2 = SearchFilesTool
            .execute(
                json!({"pattern": "needle_unique_abc", "limit": 2, "offset": 2}),
                &ctx,
            )
            .await
            .expect("search page2");

        assert!(
            result2.contains("has_more=true"),
            "second page must still report has_more=true; got: {result2}"
        );
        assert!(
            result2.contains("next_offset=4"),
            "must report next_offset=4; got: {result2}"
        );

        // Last page: offset=4, limit=2 — only 1 item left.
        let result3 = SearchFilesTool
            .execute(
                json!({"pattern": "needle_unique_abc", "limit": 2, "offset": 4}),
                &ctx,
            )
            .await
            .expect("search page3");

        assert!(
            result3.contains("has_more=false"),
            "last page must report has_more=false; got: {result3}"
        );
        assert!(
            !result3.contains("next_offset"),
            "no next_offset on last page; got: {result3}"
        );
    }
}
