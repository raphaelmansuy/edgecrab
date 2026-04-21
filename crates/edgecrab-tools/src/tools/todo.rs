//! # todo — Task checklist management
//!
//! WHY todo: Lets the agent track multi-step plans visibly. Mirrors
//! hermes-agent's `manage_todo_list` tool.
//!
//! ## Stateful design (TodoStore)
//!
//! A `TodoStore` instance is created once per conversation and shared
//! (via `Arc`) into every tool execution via `ToolContext.todo_store`.
//! This means the list survives context compression — after compression,
//! `format_for_injection()` re-injects active items into the conversation
//! so the model never loses its task plan.
//!
//! WHY server-side state over relying on the LLM: After compression the
//! LLM's view of the todo list disappears. Hoisting the list out of the
//! conversation context makes it reliably available on every turn.

use std::sync::Mutex;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use edgecrab_types::{ToolError, ToolSchema};

use crate::registry::{ToolContext, ToolHandler};

pub struct TodoTool;

// ─── Domain types ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct TodoItem {
    pub id: u32,
    pub title: String,
    pub status: String, // "not-started" | "in-progress" | "completed" | "blocked" | "cancelled"
}

#[derive(Deserialize)]
struct Args {
    /// Full todo list to write. Omit to read the current list.
    #[serde(default)]
    items: Option<Vec<TodoItem>>,
    /// Hermes-compatible todo payload shape.
    #[serde(default)]
    todos: Option<Vec<HermesTodoItem>>,
    /// true → update existing items by id, append new ones.
    /// false (default) → replace the entire list.
    #[serde(default)]
    merge: Option<bool>,
}

#[derive(Deserialize)]
struct HermesTodoItem {
    id: String,
    content: String,
    status: String,
}

fn status_from_hermes(status: &str) -> &str {
    match status {
        "pending" => "not-started",
        "in_progress" => "in-progress",
        "completed" => "completed",
        "cancelled" => "cancelled",
        _ => "not-started",
    }
}

fn stable_todo_id(raw: &str) -> u32 {
    if let Ok(parsed) = raw.parse::<u32>() {
        return parsed;
    }

    let mut hash: u32 = 2166136261;
    for byte in raw.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(16777619);
    }
    hash.max(1)
}

fn normalize_items(args: &Args) -> Option<Vec<TodoItem>> {
    args.items.clone().or_else(|| {
        args.todos.as_ref().map(|todos| {
            todos
                .iter()
                .map(|item| TodoItem {
                    id: stable_todo_id(&item.id),
                    title: item.content.clone(),
                    status: status_from_hermes(&item.status).to_string(),
                })
                .collect()
        })
    })
}

// ─── TodoStore ────────────────────────────────────────────────────────

/// Per-session in-memory task list, safe for concurrent access.
///
/// WHY Arc<TodoStore> not Arc<Mutex<Vec>>: The Mutex lives inside the
/// store, so callers can share `Arc<TodoStore>` without a separate
/// wrapping `Mutex`. Each method handles its own locking.
pub struct TodoStore {
    items: Mutex<Vec<TodoItem>>,
}

impl Default for TodoStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TodoStore {
    pub fn new() -> Self {
        Self {
            items: Mutex::new(Vec::new()),
        }
    }

    /// Replace the entire list with `new_items`.
    pub fn write(&self, new_items: Vec<TodoItem>) {
        *self.items.lock().expect("todo store mutex poisoned") = new_items;
    }

    /// Update existing items by id, append items with unknown ids.
    ///
    /// WHY merge mode: Allows the agent to update a single status without
    /// re-passing the full list, reducing token usage.
    pub fn merge_items(&self, updates: Vec<TodoItem>) {
        let mut guard = self.items.lock().expect("todo store mutex poisoned");
        for update in updates {
            if let Some(existing) = guard.iter_mut().find(|i| i.id == update.id) {
                *existing = update;
            } else {
                guard.push(update);
            }
        }
    }

    /// Return a snapshot of the current list.
    pub fn read(&self) -> Vec<TodoItem> {
        self.items
            .lock()
            .expect("todo store mutex poisoned")
            .clone()
    }

    /// True if the store contains any items.
    pub fn has_items(&self) -> bool {
        !self
            .items
            .lock()
            .expect("todo store mutex poisoned")
            .is_empty()
    }

    /// Produce active items for re-injection after context compression.
    ///
    /// WHY only active items: Re-injecting completed/cancelled items
    /// causes the model to re-do finished work. Narrowing to "not-started"
    /// and "in-progress" gives the model exactly what it needs to continue.
    /// Blocked items are also injected so the model sees the unresolved dependency.
    ///
    /// Returns None when there are no active items (nothing to inject).
    pub fn format_for_injection(&self) -> Option<String> {
        let guard = self.items.lock().expect("todo store mutex poisoned");
        let active: Vec<&TodoItem> = guard
            .iter()
            .filter(|i| {
                i.status == "not-started" || i.status == "in-progress" || i.status == "blocked"
            })
            .collect();
        if active.is_empty() {
            return None;
        }
        let mut lines =
            vec!["[Your active task list was preserved across context compression]".to_string()];
        for item in active {
            let marker = if item.status == "in-progress" {
                "[>]"
            } else if item.status == "blocked" {
                "[!]"
            } else {
                "[ ]"
            };
            lines.push(format!(
                "- {} {}. {} ({})",
                marker, item.id, item.title, item.status
            ));
        }
        Some(lines.join("\n"))
    }
}

#[async_trait]
impl ToolHandler for TodoTool {
    fn name(&self) -> &'static str {
        "manage_todo_list"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["todo"]
    }

    fn toolset(&self) -> &'static str {
        "meta"
    }

    fn emoji(&self) -> &'static str {
        "📝"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "manage_todo_list".into(),
            description: "Manage your task list for the current session. Use for complex tasks \
                          with 3+ steps or when the user provides multiple tasks. \
                          Call with no parameters (omit 'items') to read the current list.\n\n\
                          Writing:\n\
                          - Provide 'items' array to create/update items\n\
                          - merge=false (default): replace the entire list with a fresh plan\n\
                          - merge=true: update existing items by id, add any new ones\n\n\
                          Each item: {id: integer, title: string, \
                          status: not-started|in-progress|completed|blocked|cancelled}\n\
                          Use `blocked` when a dependency, approval, or user input is still required.\n\
                          Hermes-compatible calls using `todo` with `todos`, `content`, and \
                          pending|in_progress statuses are also accepted.\n\
                          Only ONE item in-progress at a time. \
                          Mark items completed immediately when done.\n\n\
                          Always returns the full current list."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "integer" },
                                "title": { "type": "string" },
                                "status": {
                                    "type": "string",
                                    "enum": ["not-started", "in-progress", "completed", "blocked", "cancelled"]
                                }
                            },
                            "required": ["id", "title", "status"]
                        },
                        "description": "Complete array of todo items to write. Omit to read the current list."
                    },
                    "merge": {
                        "type": "boolean",
                        "description": "true: update existing items by id, add new ones. false (default): replace entire list.",
                        "default": false
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
            tool: "manage_todo_list".into(),
            message: e.to_string(),
        })?;
        let normalized_items = normalize_items(&args);

        if let Some(store) = &ctx.todo_store {
            // ── Stateful path: use per-session TodoStore ──────────────────
            if let Some(mut raw_items) = normalized_items {
                // Normalize statuses before writing.
                for item in &mut raw_items {
                    normalize_status(item);
                }
                if args.merge.unwrap_or(false) {
                    store.merge_items(raw_items);
                } else {
                    store.write(raw_items);
                }
            }
            // Read back full list (whether we just wrote or not)
            let items = store.read();
            format_response(items)
        } else {
            // ── Stateless fallback (tests, minimal contexts without a store) ──
            let raw_items = normalized_items.unwrap_or_default();
            let items: Vec<TodoItem> = raw_items
                .into_iter()
                .map(|mut i| {
                    normalize_status(&mut i);
                    i
                })
                .collect();
            format_response(items)
        }
    }

    fn parallel_safe(&self) -> bool {
        false // state mutation
    }
}

/// Build the JSON response for a todo list snapshot — single source of truth
/// used by both the stateful and stateless paths.
///
/// WHY extracted: DRY — both the store path and the stateless fallback
/// need identical output formatting.
fn format_response(items: Vec<TodoItem>) -> Result<String, ToolError> {
    let total = items.len();
    let not_started = items.iter().filter(|i| i.status == "not-started").count();
    let in_progress = items.iter().filter(|i| i.status == "in-progress").count();
    let completed = items.iter().filter(|i| i.status == "completed").count();
    let blocked = items.iter().filter(|i| i.status == "blocked").count();
    let cancelled = items.iter().filter(|i| i.status == "cancelled").count();

    let todos: Vec<serde_json::Value> = items
        .iter()
        .map(|i| json!({"id": i.id, "title": i.title, "status": i.status}))
        .collect();

    let result = json!({
        "todos": todos,
        "summary": {
            "total": total,
            "not_started": not_started,
            "in_progress": in_progress,
            "completed": completed,
            "blocked": blocked,
            "cancelled": cancelled
        }
    });

    Ok(serde_json::to_string(&result).expect("json serialization is infallible"))
}

fn normalize_status(item: &mut TodoItem) {
    // Accept "pending" as a hermes-agent compatibility alias for "not-started".
    if item.status == "pending"
        || !matches!(
            item.status.as_str(),
            "not-started" | "in-progress" | "completed" | "blocked" | "cancelled"
        )
    {
        item.status = "not-started".to_string();
    }
}

inventory::submit!(&TodoTool as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn make_item(id: u32, title: &str, status: &str) -> TodoItem {
        TodoItem {
            id,
            title: title.to_string(),
            status: status.to_string(),
        }
    }

    // ── TodoStore unit tests ───────────────────────────────────────────

    #[test]
    fn store_write_and_read() {
        let store = TodoStore::new();
        store.write(vec![
            make_item(1, "Task A", "not-started"),
            make_item(2, "Task B", "in-progress"),
        ]);
        let items = store.read();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Task A");
        assert_eq!(items[1].status, "in-progress");
    }

    #[test]
    fn store_write_replaces_all() {
        let store = TodoStore::new();
        store.write(vec![make_item(1, "Old", "not-started")]);
        store.write(vec![make_item(2, "New", "completed")]);
        let items = store.read();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "New");
    }

    #[test]
    fn store_merge_updates_existing_appends_new() {
        let store = TodoStore::new();
        store.write(vec![
            make_item(1, "Task A", "not-started"),
            make_item(2, "Task B", "not-started"),
        ]);
        store.merge_items(vec![
            make_item(1, "Task A", "completed"),   // update existing
            make_item(3, "Task C", "in-progress"), // append new
        ]);
        let items = store.read();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].status, "completed", "Task A should be updated");
        assert_eq!(items[1].status, "not-started", "Task B unchanged");
        assert_eq!(items[2].title, "Task C", "Task C appended");
    }

    #[test]
    fn normalize_items_accepts_hermes_shape() {
        let args = Args {
            items: None,
            todos: Some(vec![HermesTodoItem {
                id: "plan-step-1".into(),
                content: "Inspect the bug".into(),
                status: "in_progress".into(),
            }]),
            merge: Some(true),
        };
        let items = normalize_items(&args).expect("compat items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Inspect the bug");
        assert_eq!(items[0].status, "in-progress");
        assert_ne!(items[0].id, 0);
    }

    #[test]
    fn store_format_for_injection_active_only() {
        let store = TodoStore::new();
        store.write(vec![
            make_item(1, "Done task", "completed"),
            make_item(2, "Active task", "in-progress"),
            make_item(3, "Pending task", "not-started"),
            make_item(4, "Cancelled", "cancelled"),
        ]);
        let injection = store
            .format_for_injection()
            .expect("should have active items");
        assert!(injection.contains("[>] 2. Active task"));
        assert!(injection.contains("[ ] 3. Pending task"));
        assert!(
            !injection.contains("Done task"),
            "completed items must not be injected"
        );
        assert!(
            !injection.contains("Cancelled"),
            "cancelled items must not be injected"
        );
    }

    #[test]
    fn store_format_for_injection_none_when_all_done() {
        let store = TodoStore::new();
        store.write(vec![
            make_item(1, "Done", "completed"),
            make_item(2, "Also done", "cancelled"),
        ]);
        assert!(store.format_for_injection().is_none());
    }

    #[test]
    fn store_format_for_injection_none_when_empty() {
        let store = TodoStore::new();
        assert!(store.format_for_injection().is_none());
    }

    #[test]
    fn store_has_items() {
        let store = TodoStore::new();
        assert!(!store.has_items());
        store.write(vec![make_item(1, "T", "not-started")]);
        assert!(store.has_items());
    }

    // ── Tool integration tests ─────────────────────────────────────────

    #[tokio::test]
    async fn todo_renders_list() {
        let ctx = ToolContext::test_context();
        let result = TodoTool
            .execute(
                json!({
                    "items": [
                        {"id": 1, "title": "First task", "status": "completed"},
                        {"id": 2, "title": "Second task", "status": "in-progress"},
                        {"id": 3, "title": "Third task", "status": "not-started"}
                    ]
                }),
                &ctx,
            )
            .await
            .expect("ok");

        let v: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        assert_eq!(v["summary"]["total"], 3);
        assert_eq!(v["summary"]["completed"], 1);
        assert_eq!(v["summary"]["in_progress"], 1);
        assert_eq!(v["summary"]["not_started"], 1);
        assert_eq!(v["summary"]["cancelled"], 0);
        let todos = v["todos"].as_array().expect("array");
        assert_eq!(todos[0]["status"], "completed");
        assert_eq!(todos[1]["status"], "in-progress");
        assert_eq!(todos[2]["status"], "not-started");
    }

    #[tokio::test]
    async fn todo_cancelled_status() {
        let ctx = ToolContext::test_context();
        let result = TodoTool
            .execute(
                json!({
                    "items": [
                        {"id": 1, "title": "Done", "status": "completed"},
                        {"id": 2, "title": "Skipped", "status": "cancelled"}
                    ]
                }),
                &ctx,
            )
            .await
            .expect("ok");

        let v: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        assert_eq!(v["summary"]["cancelled"], 1);
        assert_eq!(v["summary"]["completed"], 1);
        assert_eq!(v["summary"]["total"], 2);
    }

    #[tokio::test]
    async fn todo_unknown_status_normalized() {
        let ctx = ToolContext::test_context();
        let result = TodoTool
            .execute(
                json!({
                    "items": [
                        {"id": 1, "title": "Task A", "status": "weird-status"}
                    ]
                }),
                &ctx,
            )
            .await
            .expect("ok");

        let v: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        let todos = v["todos"].as_array().expect("array");
        assert_eq!(
            todos[0]["status"], "not-started",
            "Unknown status should be coerced to not-started"
        );
        assert_eq!(v["summary"]["not_started"], 1);
    }

    #[tokio::test]
    async fn todo_read_only_with_store() {
        let mut ctx = ToolContext::test_context();
        let store = Arc::new(TodoStore::new());
        store.write(vec![make_item(1, "Prepopulated", "in-progress")]);
        ctx.todo_store = Some(store);

        // No "items" key — read-only call
        let result = TodoTool.execute(json!({}), &ctx).await.expect("ok");

        let v: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        assert_eq!(v["summary"]["total"], 1);
        assert_eq!(v["todos"][0]["title"], "Prepopulated");
    }

    #[tokio::test]
    async fn todo_merge_mode_with_store() {
        let mut ctx = ToolContext::test_context();
        let store = Arc::new(TodoStore::new());
        store.write(vec![
            make_item(1, "Task A", "not-started"),
            make_item(2, "Task B", "not-started"),
        ]);
        ctx.todo_store = Some(store);

        // Merge: only update id=1, leave id=2 untouched
        let result = TodoTool
            .execute(
                json!({
                    "items": [{"id": 1, "title": "Task A", "status": "completed"}],
                    "merge": true
                }),
                &ctx,
            )
            .await
            .expect("ok");

        let v: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        assert_eq!(v["summary"]["total"], 2, "merge should not remove Task B");
        assert_eq!(v["todos"][0]["status"], "completed");
        assert_eq!(v["todos"][1]["status"], "not-started");
    }
}
