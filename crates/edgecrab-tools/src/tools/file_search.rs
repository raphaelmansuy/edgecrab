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
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    include: Option<String>,
    #[serde(default = "default_max_results")]
    max_results: usize,
}

fn default_max_results() -> usize {
    50
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
            description: "Search for a pattern in files (regex supported). Returns matching lines with file paths and line numbers.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Search pattern (regex supported)"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search in (default: working directory)"
                    },
                    "include": {
                        "type": "string",
                        "description": "File glob pattern to include (e.g., '*.rs', '*.py')"
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default: 50)"
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

        let regex = regex::Regex::new(&args.pattern).map_err(|e| ToolError::InvalidArgs {
            tool: "search_files".into(),
            message: format!("Invalid regex: {}", e),
        })?;

        let max = args.max_results.min(200); // hard cap
        let mut results = Vec::new();

        // Walk directory tree (blocking — wrapped in spawn_blocking)
        let include_glob = args.include.clone();
        let cwd = ctx.cwd.clone();

        let matches = tokio::task::spawn_blocking(move || {
            let mut hits = Vec::new();
            walk_and_search(&search_root, &regex, &include_glob, &cwd, max, &mut hits);
            hits
        })
        .await
        .map_err(|e| ToolError::Other(format!("Search task failed: {}", e)))?;

        for (path, line_num, line) in matches {
            results.push(format!("{}:{}: {}", path, line_num, line));
        }

        let output = if results.is_empty() {
            "No matches found.".to_string()
        } else {
            let count = results.len();
            let truncated = if count >= max {
                format!(
                    "\n\n(showing first {} results, use max_results to see more)",
                    max
                )
            } else {
                String::new()
            };
            format!("{}{}", results.join("\n"), truncated)
        };

        // Consecutive re-search loop detection — mirrors hermes-agent file_tools.py.
        // Warn at 3 identical consecutive searches; hard-block at 4.
        let key = read_tracker::search_key(
            &args.pattern,
            args.path.as_deref(),
            args.include.as_deref(),
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
    max: usize,
    results: &mut Vec<(String, usize, String)>,
) {
    if results.len() >= max {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        if results.len() >= max {
            return;
        }

        let path = entry.path();

        // Skip hidden dirs and common large dirs
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.')
                || name == "node_modules"
                || name == "target"
                || name == "__pycache__"
            {
                continue;
            }
        }

        if path.is_dir() {
            walk_and_search(&path, regex, include_glob, cwd, max, results);
        } else if path.is_file() {
            // Check glob filter
            if let Some(glob) = include_glob {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if !simple_glob_match(glob, name) {
                        continue;
                    }
                }
            }

            // Read and search (skip binary files)
            if let Ok(content) = std::fs::read_to_string(&path) {
                let rel_path = path
                    .strip_prefix(cwd)
                    .unwrap_or(&path)
                    .display()
                    .to_string();

                for (i, line) in content.lines().enumerate() {
                    if results.len() >= max {
                        return;
                    }
                    if regex.is_match(line) {
                        let trimmed = if line.len() > 200 {
                            format!("{}...", crate::safe_truncate(line, 200))
                        } else {
                            line.to_string()
                        };
                        results.push((rel_path.clone(), i + 1, trimmed));
                    }
                }
            }
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
}
