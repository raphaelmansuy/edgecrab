//! Materialize-time tool input examples (July 2026 progressive disclosure).
//!
//! Compact wire schemas strip property prose. Examples expand **with**
//! `tool_search` / materialize hits — not on every turn-1 hot tool.
//!
//! Provider-agnostic: emitted in tool_search result JSON (message path),
//! not Anthropic-only `input_examples` / `defer_loading` wire fields.

use serde_json::{Value, json};
use std::collections::HashMap;

/// Max examples returned per tool (Anthropic-style 1–3 budget).
pub const MAX_INPUT_EXAMPLES_PER_TOOL: usize = 3;

/// Look up 1–3 concrete argument examples for a tool name (if curated).
pub fn input_examples_for_tool(name: &str) -> Vec<Value> {
    let examples = curated_examples(name);
    examples
        .into_iter()
        .take(MAX_INPUT_EXAMPLES_PER_TOOL)
        .collect()
}

/// Build `tool_name → examples` for a set of activated names (skips empty).
pub fn input_examples_for_names(names: &[String]) -> HashMap<String, Vec<Value>> {
    let mut out = HashMap::new();
    for name in names {
        let examples = input_examples_for_tool(name);
        if !examples.is_empty() {
            out.insert(name.clone(), examples);
        }
    }
    out
}

fn curated_examples(name: &str) -> Vec<Value> {
    match name {
        "write_file" => vec![
            json!({"path": "src/hello.rs", "content": "fn main() {\n    println!(\"hi\");\n}\n"}),
            json!({"path": "README.md", "content": "# Project\n\nGetting started.\n"}),
        ],
        "web_extract" => vec![
            json!({"url": "https://example.com/article"}),
            json!({"url": "https://docs.example.com/api", "max_chars": 8000}),
        ],
        "web_crawl" => vec![json!({"url": "https://example.com", "max_pages": 3})],
        "browser_navigate" => vec![
            json!({"url": "https://example.com"}),
            json!({"url": "http://127.0.0.1:8000/"}),
        ],
        "browser_click" => vec![json!({"ref": "e12"}), json!({"selector": "button.submit"})],
        "browser_snapshot" => vec![json!({}), json!({"full_page": true})],
        "browser_type" => vec![json!({"ref": "e5", "text": "search query"})],
        "read_file" => vec![
            json!({"path": "src/lib.rs"}),
            json!({"path": "README.md", "offset": 1, "limit": 80}),
        ],
        "apply_patch" => vec![json!({
            "path": "src/lib.rs",
            "old_string": "fn foo() {}",
            "new_string": "fn foo() -> i32 { 0 }"
        })],
        "tool_search" => vec![
            json!({"query": "browser snapshot localhost"}),
            json!({"query": "write file create"}),
        ],
        "memory_write" => vec![json!({
            "target": "MEMORY",
            "content": "User prefers Rust over Python for CLI tools."
        })],
        "memory_read" => vec![json!({"target": "MEMORY"}), json!({"target": "USER"})],
        "skill_view" => vec![json!({"name": "deploy-checklist"})],
        "skill_manage" => vec![
            json!({
                "action": "create",
                "name": "pr-review",
                "content": "# PR review\n1. Read diff\n2. Run tests\n"
            }),
            json!({
                "action": "edit",
                "name": "pr-review",
                "content": "# PR review\n1. Read diff\n2. Run tests\n3. Note pitfalls\n"
            }),
            json!({
                "action": "write_skill_file",
                "name": "pr-review",
                "file_path": "references/checklist.md",
                "file_content": "- [ ] Tests green\n"
            }),
        ],
        "delegate_task" => vec![json!({
            "task": "Find all TODO comments in crates/edgecrab-core",
            "toolsets": ["file", "terminal"]
        })],
        "manage_todo_list" => vec![json!({
            "todos": [
                {"id": "1", "content": "Implement feature", "status": "in_progress"},
                {"id": "2", "content": "Add tests", "status": "pending"}
            ]
        })],
        "patch" => vec![json!({
            "path": "src/lib.rs",
            "old_string": "fn foo() {}",
            "new_string": "fn foo() -> i32 { 0 }"
        })],
        "search_files" => vec![
            json!({"pattern": "TODO|FIXME", "path": "crates"}),
            json!({"pattern": "fn main", "glob": "*.rs"}),
        ],
        "terminal" => vec![
            json!({"command": "cargo test -p edgecrab-core --lib"}),
            json!({"command": "ls -la", "workdir": "."}),
        ],
        "ha_call_service" => vec![json!({
            "domain": "light",
            "service": "turn_on",
            "entity_id": "light.living_room"
        })],
        "x_search" => vec![json!({"query": "EdgeCrab AI agent", "max_results": 5})],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_file_has_examples_capped() {
        let ex = input_examples_for_tool("write_file");
        assert!(!ex.is_empty());
        assert!(ex.len() <= MAX_INPUT_EXAMPLES_PER_TOOL);
        assert!(ex[0].get("path").is_some());
    }

    #[test]
    fn unknown_tool_has_no_examples() {
        assert!(input_examples_for_tool("quick_stock_quote").is_empty());
    }

    #[test]
    fn names_map_skips_empty() {
        let map = input_examples_for_names(&[
            "write_file".into(),
            "quick_stock_quote".into(),
            "browser_navigate".into(),
        ]);
        assert!(map.contains_key("write_file"));
        assert!(map.contains_key("browser_navigate"));
        assert!(!map.contains_key("quick_stock_quote"));
    }
}
