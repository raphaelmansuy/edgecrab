//! Kanban board tools — Hermes `kanban_tools.py` Phase 1 parity.
//!
//! Durable task cards at `~/.edgecrab/kanban.db`. Gated by `kanban.enabled` config
//! or the `kanban` toolset.

use async_trait::async_trait;
use edgecrab_state::KanbanDb;
use edgecrab_types::{ToolError, ToolSchema};
use serde::Deserialize;
use serde_json::json;

use crate::kanban_gating;
use crate::registry::{ToolContext, ToolHandler};

fn kanban_check(ctx: &ToolContext) -> bool {
    ctx.config.kanban_enabled
}

fn kanban_orchestrator_check(ctx: &ToolContext) -> bool {
    ctx.config.kanban_enabled && kanban_gating::orchestrator_tool_visible(ctx)
}

fn open_board(ctx: &ToolContext) -> Result<std::sync::Arc<KanbanDb>, ToolError> {
    if !ctx.config.kanban_enabled {
        return Err(ToolError::PermissionDenied(
            "Kanban is disabled. Set kanban.enabled: true in config.yaml or enable the kanban toolset."
                .into(),
        ));
    }
    let db = KanbanDb::open_default(Some(&ctx.config.edgecrab_home)).map_err(|e| {
        ToolError::Other(format!("kanban db: {e}"))
    })?;
    let _ = db.reclaim_stale_claims();
    Ok(db)
}

fn worker_id(ctx: &ToolContext) -> String {
    ctx.session_key
        .clone()
        .unwrap_or_else(|| ctx.session_id.clone())
}

fn task_json(t: &edgecrab_state::KanbanTask) -> serde_json::Value {
    json!({
        "id": t.id,
        "title": t.title,
        "body": t.body,
        "status": t.status,
        "priority": t.priority,
        "worker_id": t.worker_id,
        "claim_expires": t.claim_expires,
        "result": t.result,
        "created_at": t.created_at,
        "updated_at": t.updated_at,
        "consecutive_failures": t.consecutive_failures,
        "last_failure_error": t.last_failure_error,
        "max_retries": t.max_retries,
        "current_run_id": t.current_run_id,
        "max_runtime_seconds": t.max_runtime_seconds,
        "assignee": t.assignee,
        "scheduled_at": t.scheduled_at,
    })
}

fn resolve_max_runtime(ctx: &ToolContext, arg: Option<i32>) -> Option<i32> {
    if let Some(v) = arg.filter(|v| *v > 0) {
        return Some(v);
    }
    let default = ctx.config.kanban_default_max_runtime_secs;
    if default > 0 {
        Some(default as i32)
    } else {
        None
    }
}

fn ttl_secs(ctx: &ToolContext) -> i64 {
    ctx.config.kanban_claim_ttl_secs.clamp(60, 86_400) as i64
}

// ─── kanban_create ─────────────────────────────────────────────────

pub struct KanbanCreateTool;

#[derive(Deserialize)]
struct CreateArgs {
    title: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    priority: i32,
    #[serde(default)]
    max_runtime_seconds: Option<i32>,
    #[serde(default)]
    assignee: Option<String>,
}

#[async_trait]
impl ToolHandler for KanbanCreateTool {
    fn name(&self) -> &'static str {
        "kanban_create"
    }
    fn toolset(&self) -> &'static str {
        "kanban"
    }
    fn emoji(&self) -> &'static str {
        "📋"
    }

    fn check_fn(&self, ctx: &ToolContext) -> bool {
        kanban_check(ctx)
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "kanban_create".into(),
            description: "Create a durable kanban task card (todo).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Short task title" },
                    "body": { "type": "string", "description": "Optional detailed description" },
                    "priority": { "type": "integer", "description": "Higher = sooner (default 0)" },
                    "max_runtime_seconds": {
                        "type": "integer",
                        "description": "Optional wall-clock limit for worker runs (seconds); uses kanban.default_max_runtime_secs when omitted"
                    },
                    "assignee": {
                        "type": "string",
                        "description": "Profile name to execute this task (must exist under ~/.edgecrab/profiles/)"
                    }
                },
                "required": ["title"]
            }),
            strict: None,
        }
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: CreateArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "kanban_create".into(),
            message: e.to_string(),
        })?;
        let db = open_board(ctx)?;
        let max_runtime = resolve_max_runtime(ctx, args.max_runtime_seconds);
        let assignee = args.assignee.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let task = db
            .create_task_with_assignee(
                &args.title,
                args.body.as_deref(),
                args.priority,
                max_runtime,
                assignee,
            )
            .map_err(|e| ToolError::Other(e.to_string()))?;
        Ok(json!({ "success": true, "task": task_json(&task) }).to_string())
    }
}

// ─── kanban_list ───────────────────────────────────────────────────

pub struct KanbanListTool;

#[derive(Deserialize)]
struct ListArgs {
    #[serde(default)]
    status: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    50
}

#[async_trait]
impl ToolHandler for KanbanListTool {
    fn name(&self) -> &'static str {
        "kanban_list"
    }
    fn toolset(&self) -> &'static str {
        "kanban"
    }
    fn emoji(&self) -> &'static str {
        "📋"
    }

    fn check_fn(&self, ctx: &ToolContext) -> bool {
        kanban_orchestrator_check(ctx)
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "kanban_list".into(),
            description: "List kanban task cards, optionally filtered by status.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string", "description": "todo|doing|done|blocked" },
                    "limit": { "type": "integer", "description": "Max cards (default 50, max 200)" }
                }
            }),
            strict: None,
        }
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: ListArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "kanban_list".into(),
            message: e.to_string(),
        })?;
        let status = args
            .status
            .as_deref()
            .and_then(edgecrab_state::KanbanStatus::parse);
        let db = open_board(ctx)?;
        let tasks = db
            .list_tasks(status, args.limit)
            .map_err(|e| ToolError::Other(e.to_string()))?;
        Ok(json!({
            "success": true,
            "count": tasks.len(),
            "tasks": tasks.iter().map(task_json).collect::<Vec<_>>()
        })
        .to_string())
    }
}

// ─── kanban_claim ──────────────────────────────────────────────────

pub struct KanbanClaimTool;

#[derive(Deserialize)]
struct ClaimArgs {
    task_id: String,
    #[serde(default)]
    worker_id: Option<String>,
}

#[async_trait]
impl ToolHandler for KanbanClaimTool {
    fn name(&self) -> &'static str {
        "kanban_claim"
    }
    fn toolset(&self) -> &'static str {
        "kanban"
    }
    fn emoji(&self) -> &'static str {
        "📋"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "kanban_claim".into(),
            description: "Claim a todo/blocked kanban card for this worker (sets status=doing).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" },
                    "worker_id": { "type": "string", "description": "Defaults to current session" }
                },
                "required": ["task_id"]
            }),
            strict: None,
        }
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: ClaimArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "kanban_claim".into(),
            message: e.to_string(),
        })?;
        let db = open_board(ctx)?;
        let worker = args.worker_id.unwrap_or_else(|| worker_id(ctx));
        let task = db
            .claim_task(&args.task_id, &worker, ttl_secs(ctx))
            .map_err(|e| ToolError::Other(e.to_string()))?;
        Ok(json!({ "success": true, "task": task_json(&task) }).to_string())
    }
}

// ─── kanban_complete ───────────────────────────────────────────────

pub struct KanbanCompleteTool;

#[derive(Deserialize)]
struct CompleteArgs {
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    result: Option<String>,
}

#[async_trait]
impl ToolHandler for KanbanCompleteTool {
    fn name(&self) -> &'static str {
        "kanban_complete"
    }
    fn toolset(&self) -> &'static str {
        "kanban"
    }
    fn emoji(&self) -> &'static str {
        "📋"
    }

    fn check_fn(&self, ctx: &ToolContext) -> bool {
        kanban_check(ctx)
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "kanban_complete".into(),
            description: "Mark a kanban card done with an optional result summary.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "string", "description": "Defaults to EDGECRAB_KANBAN_TASK when set" },
                    "result": { "type": "string", "description": "Completion summary" }
                }
            }),
            strict: None,
        }
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: CompleteArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "kanban_complete".into(),
            message: e.to_string(),
        })?;
        let db = open_board(ctx)?;
        let task_id = kanban_gating::resolve_task_id(ctx, args.task_id.as_deref()).ok_or_else(|| {
            ToolError::InvalidArgs {
                tool: "kanban_complete".into(),
                message: "task_id is required (or set EDGECRAB_KANBAN_TASK)".into(),
            }
        })?;
        let w = worker_id(ctx);
        let task = db
            .complete_task(&task_id, Some(&w), args.result.as_deref())
            .or_else(|_| db.complete_task(&task_id, None, args.result.as_deref()))
            .map_err(|e| ToolError::Other(e.to_string()))?;
        Ok(json!({ "success": true, "task": task_json(&task) }).to_string())
    }
}

// ─── kanban_release ────────────────────────────────────────────────

pub struct KanbanReleaseTool;

#[derive(Deserialize)]
struct ReleaseArgs {
    task_id: String,
}

#[async_trait]
impl ToolHandler for KanbanReleaseTool {
    fn name(&self) -> &'static str {
        "kanban_release"
    }
    fn toolset(&self) -> &'static str {
        "kanban"
    }
    fn emoji(&self) -> &'static str {
        "📋"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "kanban_release".into(),
            description: "Release a claimed kanban card back to todo.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" }
                },
                "required": ["task_id"]
            }),
            strict: None,
        }
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: ReleaseArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "kanban_release".into(),
            message: e.to_string(),
        })?;
        let db = open_board(ctx)?;
        let w = worker_id(ctx);
        let task = db
            .release_task(&args.task_id, Some(&w))
            .or_else(|_| db.release_task(&args.task_id, None))
            .map_err(|e| ToolError::Other(e.to_string()))?;
        Ok(json!({ "success": true, "task": task_json(&task) }).to_string())
    }
}

// ─── kanban_heartbeat ──────────────────────────────────────────────

pub struct KanbanHeartbeatTool;

#[derive(Deserialize)]
struct HeartbeatArgs {
    #[serde(default)]
    task_id: Option<String>,
}

#[async_trait]
impl ToolHandler for KanbanHeartbeatTool {
    fn name(&self) -> &'static str {
        "kanban_heartbeat"
    }
    fn toolset(&self) -> &'static str {
        "kanban"
    }
    fn emoji(&self) -> &'static str {
        "📋"
    }

    fn check_fn(&self, ctx: &ToolContext) -> bool {
        kanban_check(ctx)
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "kanban_heartbeat".into(),
            description: "Extend the claim lease on a doing kanban card.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "string", "description": "Defaults to EDGECRAB_KANBAN_TASK when set" }
                }
            }),
            strict: None,
        }
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: HeartbeatArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "kanban_heartbeat".into(),
            message: e.to_string(),
        })?;
        let db = open_board(ctx)?;
        let task_id = kanban_gating::resolve_task_id(ctx, args.task_id.as_deref()).ok_or_else(|| {
            ToolError::InvalidArgs {
                tool: "kanban_heartbeat".into(),
                message: "task_id is required (or set EDGECRAB_KANBAN_TASK)".into(),
            }
        })?;
        db.heartbeat_task(&task_id, &worker_id(ctx), ttl_secs(ctx))
            .map_err(|e| ToolError::Other(e.to_string()))?;
        Ok(json!({ "success": true, "task_id": task_id }).to_string())
    }
}

// ─── kanban_show ───────────────────────────────────────────────────

pub struct KanbanShowTool;

#[derive(Deserialize)]
struct ShowArgs {
    #[serde(default)]
    task_id: Option<String>,
}

#[async_trait]
impl ToolHandler for KanbanShowTool {
    fn name(&self) -> &'static str {
        "kanban_show"
    }
    fn toolset(&self) -> &'static str {
        "kanban"
    }
    fn emoji(&self) -> &'static str {
        "📋"
    }
    fn check_fn(&self, ctx: &ToolContext) -> bool {
        kanban_check(ctx)
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "kanban_show".into(),
            description: "Show a kanban task with comments (defaults to EDGECRAB_KANBAN_TASK).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" }
                }
            }),
            strict: None,
        }
    }
    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: ShowArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "kanban_show".into(),
            message: e.to_string(),
        })?;
        let task_id = kanban_gating::resolve_task_id(ctx, args.task_id.as_deref()).ok_or_else(|| {
            ToolError::InvalidArgs {
                tool: "kanban_show".into(),
                message: "task_id is required (or set EDGECRAB_KANBAN_TASK)".into(),
            }
        })?;
        let db = open_board(ctx)?;
        let task = db
            .get_task(&task_id)
            .map_err(|e| ToolError::Other(e.to_string()))?
            .ok_or_else(|| ToolError::Other(format!("task {task_id} not found")))?;
        let comments = db
            .list_comments(&task_id)
            .map_err(|e| ToolError::Other(e.to_string()))?;
        let parents = db
            .parent_ids(&task_id)
            .map_err(|e| ToolError::Other(e.to_string()))?;
        let children = db
            .child_ids(&task_id)
            .map_err(|e| ToolError::Other(e.to_string()))?;
        let runs = db
            .list_task_runs(&task_id, 10)
            .map_err(|e| ToolError::Other(e.to_string()))?;
        let worker_context = edgecrab_state::build_worker_context(&db, &task_id)
            .map_err(|e| ToolError::Other(e.to_string()))?;
        Ok(json!({
            "success": true,
            "task": task_json(&task),
            "comments": comments,
            "parents": parents,
            "children": children,
            "runs": runs,
            "worker_context": worker_context,
        })
        .to_string())
    }
}

// ─── kanban_block ──────────────────────────────────────────────────

pub struct KanbanBlockTool;

#[derive(Deserialize)]
struct BlockArgs {
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

#[async_trait]
impl ToolHandler for KanbanBlockTool {
    fn name(&self) -> &'static str {
        "kanban_block"
    }
    fn toolset(&self) -> &'static str {
        "kanban"
    }
    fn emoji(&self) -> &'static str {
        "📋"
    }
    fn check_fn(&self, ctx: &ToolContext) -> bool {
        kanban_check(ctx)
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "kanban_block".into(),
            description: "Block a kanban task (needs human input). Sets status=blocked.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "string", "description": "Defaults to EDGECRAB_KANBAN_TASK" },
                    "reason": { "type": "string", "description": "Why the task is blocked" }
                }
            }),
            strict: None,
        }
    }
    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: BlockArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "kanban_block".into(),
            message: e.to_string(),
        })?;
        let task_id = kanban_gating::resolve_task_id(ctx, args.task_id.as_deref()).ok_or_else(|| {
            ToolError::InvalidArgs {
                tool: "kanban_block".into(),
                message: "task_id is required (or set EDGECRAB_KANBAN_TASK)".into(),
            }
        })?;
        let db = open_board(ctx)?;
        let w = worker_id(ctx);
        let task = db
            .block_task(&task_id, Some(&w), args.reason.as_deref())
            .or_else(|_| db.block_task(&task_id, None, args.reason.as_deref()))
            .map_err(|e| ToolError::Other(e.to_string()))?;
        Ok(json!({ "success": true, "task": task_json(&task) }).to_string())
    }
}

// ─── kanban_unblock ────────────────────────────────────────────────

pub struct KanbanUnblockTool;

#[derive(Deserialize)]
struct UnblockArgs {
    task_id: String,
}

#[async_trait]
impl ToolHandler for KanbanUnblockTool {
    fn name(&self) -> &'static str {
        "kanban_unblock"
    }
    fn toolset(&self) -> &'static str {
        "kanban"
    }
    fn emoji(&self) -> &'static str {
        "📋"
    }
    fn check_fn(&self, ctx: &ToolContext) -> bool {
        kanban_orchestrator_check(ctx)
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "kanban_unblock".into(),
            description: "Unblock a kanban task (orchestrator-only). Returns it to todo.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" }
                },
                "required": ["task_id"]
            }),
            strict: None,
        }
    }
    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: UnblockArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "kanban_unblock".into(),
            message: e.to_string(),
        })?;
        let db = open_board(ctx)?;
        let task = db
            .unblock_task(&args.task_id)
            .map_err(|e| ToolError::Other(e.to_string()))?;
        Ok(json!({ "success": true, "task": task_json(&task) }).to_string())
    }
}

// ─── kanban_comment ──────────────────────────────────────────────

pub struct KanbanCommentTool;

#[derive(Deserialize)]
struct CommentArgs {
    #[serde(default)]
    task_id: Option<String>,
    body: String,
    #[serde(default)]
    author: Option<String>,
}

#[async_trait]
impl ToolHandler for KanbanCommentTool {
    fn name(&self) -> &'static str {
        "kanban_comment"
    }
    fn toolset(&self) -> &'static str {
        "kanban"
    }
    fn emoji(&self) -> &'static str {
        "📋"
    }
    fn check_fn(&self, ctx: &ToolContext) -> bool {
        kanban_check(ctx)
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "kanban_comment".into(),
            description: "Add a comment to a kanban task thread.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "string", "description": "Defaults to EDGECRAB_KANBAN_TASK" },
                    "body": { "type": "string", "description": "Comment text" },
                    "author": { "type": "string", "description": "Author label (defaults to session id)" }
                },
                "required": ["body"]
            }),
            strict: None,
        }
    }
    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: CommentArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "kanban_comment".into(),
            message: e.to_string(),
        })?;
        let task_id = kanban_gating::resolve_task_id(ctx, args.task_id.as_deref()).ok_or_else(|| {
            ToolError::InvalidArgs {
                tool: "kanban_comment".into(),
                message: "task_id is required (or set EDGECRAB_KANBAN_TASK)".into(),
            }
        })?;
        let author = args
            .author
            .unwrap_or_else(|| worker_id(ctx));
        let db = open_board(ctx)?;
        let comment = db
            .add_comment(&task_id, &author, &args.body)
            .map_err(|e| ToolError::Other(e.to_string()))?;
        Ok(json!({ "success": true, "comment": comment }).to_string())
    }
}

inventory::submit!(&KanbanShowTool as &dyn ToolHandler);
inventory::submit!(&KanbanBlockTool as &dyn ToolHandler);
inventory::submit!(&KanbanUnblockTool as &dyn ToolHandler);
inventory::submit!(&KanbanCommentTool as &dyn ToolHandler);

// ─── kanban_link ───────────────────────────────────────────────────

pub struct KanbanLinkTool;

#[derive(Deserialize)]
struct LinkArgs {
    parent_id: String,
    child_id: String,
}

#[async_trait]
impl ToolHandler for KanbanLinkTool {
    fn name(&self) -> &'static str {
        "kanban_link"
    }
    fn toolset(&self) -> &'static str {
        "kanban"
    }
    fn emoji(&self) -> &'static str {
        "📋"
    }
    fn check_fn(&self, ctx: &ToolContext) -> bool {
        kanban_check(ctx)
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "kanban_link".into(),
            description: "Add a parent→child dependency edge between kanban tasks.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "parent_id": { "type": "string", "description": "Blocking parent task id" },
                    "child_id": { "type": "string", "description": "Dependent child task id" }
                },
                "required": ["parent_id", "child_id"]
            }),
            strict: None,
        }
    }
    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: LinkArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "kanban_link".into(),
            message: e.to_string(),
        })?;
        let db = open_board(ctx)?;
        db.link_tasks(&args.parent_id, &args.child_id)
            .map_err(|e| ToolError::Other(e.to_string()))?;
        Ok(json!({
            "success": true,
            "parent_id": args.parent_id,
            "child_id": args.child_id,
        })
        .to_string())
    }
}

inventory::submit!(&KanbanLinkTool as &dyn ToolHandler);

inventory::submit!(&KanbanCreateTool as &dyn ToolHandler);
inventory::submit!(&KanbanListTool as &dyn ToolHandler);
inventory::submit!(&KanbanClaimTool as &dyn ToolHandler);
inventory::submit!(&KanbanCompleteTool as &dyn ToolHandler);
inventory::submit!(&KanbanReleaseTool as &dyn ToolHandler);
inventory::submit!(&KanbanHeartbeatTool as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ToolContext;
    use tempfile::TempDir;

    fn test_ctx(home: &TempDir) -> ToolContext {
        let mut ctx = ToolContext::test_context();
        ctx.config.edgecrab_home = home.path().to_path_buf();
        ctx.config.kanban_enabled = true;
        ctx.session_key = Some("sess-1".into());
        ctx
    }

    #[tokio::test]
    async fn create_and_list_via_tools() {
        let home = TempDir::new().expect("tmpdir");
        let ctx = test_ctx(&home);
        let create = KanbanCreateTool;
        let out = create
            .execute(
                json!({ "title": "Ship kanban MVP", "body": "phase 1" }),
                &ctx,
            )
            .await
            .expect("create");
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("json");
        let id = parsed["task"]["id"].as_str().expect("id");

        let list = KanbanListTool;
        let listed = list.execute(json!({}), &ctx).await.expect("list");
        assert!(listed.contains(id));
    }
}
