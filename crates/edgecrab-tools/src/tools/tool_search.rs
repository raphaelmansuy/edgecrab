//! Call with exact `tool_names` or a BM25 `query` over deferred tools (Hermes parity).

use std::collections::HashSet;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use edgecrab_types::{ToolError, ToolSchema};

use crate::registry::{ToolContext, ToolHandler};
use crate::schema_mode::compact_tool_schema;
use crate::tool_schema_index::{self, TOOL_SEARCH_NAME, read_materialized_set};
use crate::tool_search_bm25::{build_deferred_catalog, search_deferred_catalog};

pub struct ToolSearchTool;

#[derive(Deserialize)]
struct Args {
    /// Exact tool names to materialize (e.g. `["browser_navigate", "lsp_hover"]`).
    #[serde(default)]
    tool_names: Vec<String>,
    /// BM25 search over deferred tools when exact names are unknown.
    #[serde(default)]
    query: Option<String>,
    /// Max results when using `query` (default 5, max 20).
    #[serde(default)]
    limit: Option<usize>,
}

fn resolve_tool_names(args: &Args, ctx: &ToolContext, all_schemas: &[ToolSchema]) -> Vec<String> {
    let explicit: Vec<String> = args
        .tool_names
        .iter()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .collect();
    if !explicit.is_empty() {
        return explicit;
    }
    let Some(query) = args
        .query
        .as_ref()
        .map(|q| q.trim())
        .filter(|q| !q.is_empty())
    else {
        return Vec::new();
    };
    let limit = args.limit.unwrap_or(5).clamp(1, 20);
    let materialized = read_materialized_set(ctx.materialized_tools.as_ref());
    let catalog = build_deferred_catalog(all_schemas, &materialized);
    search_deferred_catalog(&catalog, query, limit)
}

#[async_trait]
impl ToolHandler for ToolSearchTool {
    fn name(&self) -> &'static str {
        TOOL_SEARCH_NAME
    }

    fn toolset(&self) -> &'static str {
        "core"
    }

    fn emoji(&self) -> &'static str {
        "🔍"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: TOOL_SEARCH_NAME.into(),
            description: "Load full schemas for deferred tools before calling them. \
                Use `tool_names` for exact activation or `query` for BM25 search over \
                the deferred catalog."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "tool_names": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Exact tool names to activate"
                    },
                    "query": {
                        "type": "string",
                        "description": "Natural-language search over deferred tools (BM25)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max tools to return for query search (default 5)"
                    }
                }
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
            tool: TOOL_SEARCH_NAME.into(),
            message: e.to_string(),
        })?;
        let registry = ctx
            .tool_registry
            .as_ref()
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool: TOOL_SEARCH_NAME.into(),
                message: "tool registry not available".into(),
            })?;

        let all_schemas = registry.get_definitions(None, None, ctx);
        let names_to_activate = resolve_tool_names(&args, ctx, &all_schemas);
        if names_to_activate.is_empty() {
            return Err(ToolError::InvalidArgs {
                tool: TOOL_SEARCH_NAME.into(),
                message: "provide tool_names or a non-empty query".into(),
            });
        }

        let known: HashSet<&str> = all_schemas.iter().map(|s| s.name.as_str()).collect();

        let mut activated = Vec::new();
        let mut already_wire = Vec::new();
        let mut not_found = Vec::new();
        let mut schemas_out = Vec::new();

        let materialized = ctx.materialized_tools.as_ref();

        for name in names_to_activate {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                continue;
            }
            if !known.contains(trimmed) {
                not_found.push(trimmed.to_string());
                continue;
            }
            if tool_schema_index::is_hot_tool(trimmed) {
                already_wire.push(trimmed.to_string());
                continue;
            }
            if let Some(set) = materialized
                && let Ok(mut guard) = set.write()
            {
                guard.insert(trimmed.to_string());
            }
            if let Some(schema) = all_schemas.iter().find(|s| s.name == trimmed) {
                schemas_out.push(compact_tool_schema(schema));
            }
            activated.push(trimmed.to_string());
        }

        Ok(json!({
            "activated": activated,
            "already_on_wire": already_wire,
            "not_found": not_found,
            "schemas": schemas_out,
            "query": args.query,
            "hint": "Deferred tools are now on your tool list for subsequent turns."
        })
        .to_string())
    }
}

inventory::submit!(&ToolSearchTool as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, RwLock};

    use edgecrab_types::Platform;
    use tokio_util::sync::CancellationToken;

    use crate::config_ref::AppConfigRef;
    use crate::registry::ToolRegistry;

    #[tokio::test]
    async fn bm25_query_materializes_deferred_tool() {
        let registry = Arc::new(ToolRegistry::new());
        let materialized = Arc::new(RwLock::new(HashSet::new()));
        let ctx = ToolContext {
            task_id: "t".into(),
            cwd: std::env::temp_dir(),
            session_id: "s".into(),
            user_task: None,
            cancel: CancellationToken::new(),
            config: AppConfigRef::default(),
            state_db: None,
            platform: Platform::Cli,
            process_table: None,
            provider: None,
            tool_registry: Some(registry.clone()),
            delegate_depth: 0,
            delegate_agent_id: None,
            delegate_parent_id: None,
            sub_agent_runner: None,
            delegation_event_tx: None,
            clarify_tx: None,
            approval_tx: None,
            on_skills_changed: None,
            gateway_sender: None,
            origin_chat: None,
            session_key: None,
            todo_store: None,
            current_tool_call_id: None,
            current_tool_name: None,
            injected_messages: None,
            tool_progress_tx: None,
            watch_notification_tx: None,
            mutation_turn: None,
            lsp_gate: None,
            kanban_task_id: None,
            materialized_tools: Some(materialized.clone()),
        };

        let tool = ToolSearchTool;
        let out = tool
            .execute(json!({ "query": "browser navigate url" }), &ctx)
            .await
            .expect("execute");
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("json");
        assert!(
            parsed["activated"]
                .as_array()
                .is_some_and(|a| !a.is_empty()),
            "BM25 query should activate at least one tool: {out}"
        );
    }

    #[tokio::test]
    async fn materializes_deferred_tool() {
        let registry = Arc::new(ToolRegistry::new());
        let materialized = Arc::new(RwLock::new(HashSet::new()));
        let ctx = ToolContext {
            task_id: "t".into(),
            cwd: std::env::temp_dir(),
            session_id: "s".into(),
            user_task: None,
            cancel: CancellationToken::new(),
            config: AppConfigRef::default(),
            state_db: None,
            platform: Platform::Cli,
            process_table: None,
            provider: None,
            tool_registry: Some(registry.clone()),
            delegate_depth: 0,
            delegate_agent_id: None,
            delegate_parent_id: None,
            sub_agent_runner: None,
            delegation_event_tx: None,
            clarify_tx: None,
            approval_tx: None,
            on_skills_changed: None,
            gateway_sender: None,
            origin_chat: None,
            session_key: None,
            todo_store: None,
            current_tool_call_id: None,
            current_tool_name: None,
            injected_messages: None,
            tool_progress_tx: None,
            watch_notification_tx: None,
            mutation_turn: None,
            lsp_gate: None,
            kanban_task_id: None,
            materialized_tools: Some(materialized.clone()),
        };

        let tool = ToolSearchTool;
        let out = tool
            .execute(json!({ "tool_names": ["browser_navigate"] }), &ctx)
            .await
            .expect("execute");
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("json");
        assert!(
            parsed["activated"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v == "browser_navigate"))
        );
        assert!(materialized.read().unwrap().contains("browser_navigate"));
    }
}
