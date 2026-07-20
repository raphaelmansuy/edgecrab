//! Call with exact `tool_names`, a `toolset` pack, or a BM25 `query` over deferred tools.

use std::collections::HashSet;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use edgecrab_types::{ToolError, ToolSchema};

use crate::registry::{ToolContext, ToolHandler};
use crate::tool_schema_index::{
    MAX_TOOLSET_MATERIALIZE, MaterializeSchemaStyle, TOOL_SEARCH_NAME, deferred_names_for_toolset,
    materialize_tool_names, read_materialized_set,
};
use crate::tool_search_bm25::{build_deferred_catalog, search_deferred_catalog};

pub struct ToolSearchTool;

#[derive(Deserialize)]
struct Args {
    /// Exact tool names to materialize (e.g. `["browser_navigate", "lsp_hover"]`).
    #[serde(default)]
    tool_names: Vec<String>,
    /// Materialize deferred tools in a registered toolset (pack load, max 8).
    #[serde(default)]
    toolset: Option<String>,
    /// BM25 search over deferred tools when exact names are unknown.
    #[serde(default)]
    query: Option<String>,
    /// Max results when using `query` (default 5, max 20).
    #[serde(default)]
    limit: Option<usize>,
}

/// Priority: `tool_names` > `toolset` > `query`.
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

    if let Some(toolset) = args
        .toolset
        .as_ref()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        && let Some(registry) = ctx.tool_registry.as_ref()
    {
        let known: HashSet<&str> = all_schemas.iter().map(|s| s.name.as_str()).collect();
        let members = registry.tools_in_toolset(toolset);
        return deferred_names_for_toolset(toolset, &members, &known, MAX_TOOLSET_MATERIALIZE);
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
            description: "Load schemas for deferred tools before calling them. \
                Use `tool_names` for exact activation, `toolset` to load a pack \
                (up to 8 deferred tools), or `query` for BM25 search over the catalog."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "tool_names": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Exact tool names to activate"
                    },
                    "toolset": {
                        "type": "string",
                        "description": "Registered toolset pack to materialize (e.g. browser, file, memory)"
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
                message: "provide tool_names, toolset, or a non-empty query".into(),
            });
        }

        let max = ctx.config.max_materialized_tools;
        let Some(materialized) = ctx.materialized_tools.as_ref() else {
            return Err(ToolError::ExecutionFailed {
                tool: TOOL_SEARCH_NAME.into(),
                message: "materialized tool set not available".into(),
            });
        };

        let outcome = materialize_tool_names(
            &names_to_activate,
            &all_schemas,
            materialized,
            max,
            MaterializeSchemaStyle::Compact,
        );

        Ok(json!({
            "activated": outcome.activated,
            "already_on_wire": outcome.already_wire,
            "not_found": outcome.not_found,
            "evicted": outcome.evicted,
            "schemas": outcome.schemas,
            "input_examples": outcome.input_examples,
            "query": args.query,
            "toolset": args.toolset,
            "max_materialized_tools": max,
            "hint": "Deferred tools are now on your tool list for subsequent turns. \
                     Use input_examples as argument templates when calling them."
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

    fn test_ctx(
        registry: Arc<ToolRegistry>,
        materialized: Arc<RwLock<crate::MaterializedToolSet>>,
    ) -> ToolContext {
        ToolContext {
            task_id: "t".into(),
            cwd: std::env::temp_dir(),
            session_id: "s".into(),
            user_task: None,
            cancel: CancellationToken::new(),
            config: AppConfigRef::default(),
            state_db: None,
            platform: Platform::Cli,
            capability_grants: None,
            process_table: None,
            provider: None,
            tool_registry: Some(registry),
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
            materialized_tools: Some(materialized),
        }
    }

    #[tokio::test]
    async fn bm25_query_materializes_deferred_tool() {
        let registry = Arc::new(ToolRegistry::new());
        let materialized = Arc::new(RwLock::new(crate::MaterializedToolSet::new()));
        let ctx = test_ctx(registry, materialized);

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
        let materialized = Arc::new(RwLock::new(crate::MaterializedToolSet::new()));
        let ctx = test_ctx(registry, materialized.clone());

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
        assert!(
            parsed["input_examples"]["browser_navigate"]
                .as_array()
                .is_some_and(|a| !a.is_empty()),
            "tool_search must return input_examples for curated tools: {out}"
        );
        assert!(
            parsed["hint"]
                .as_str()
                .is_some_and(|h| h.contains("input_examples")),
            "hint should point at input_examples"
        );
    }

    #[tokio::test]
    async fn toolset_bulk_materializes_pack() {
        let registry = Arc::new(ToolRegistry::new());
        let materialized = Arc::new(RwLock::new(crate::MaterializedToolSet::new()));
        let ctx = test_ctx(registry.clone(), materialized.clone());

        let tool = ToolSearchTool;
        let out = tool
            .execute(json!({ "toolset": "browser" }), &ctx)
            .await
            .expect("execute");
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("json");
        let activated = parsed["activated"].as_array().expect("activated array");
        assert!(
            !activated.is_empty(),
            "browser toolset should activate deferred tools: {out}"
        );
        assert!(activated.len() <= MAX_TOOLSET_MATERIALIZE);
        // tool_names wins over toolset
        let out2 = tool
            .execute(
                json!({
                    "tool_names": ["memory_write"],
                    "toolset": "browser",
                    "query": "browser"
                }),
                &ctx,
            )
            .await
            .expect("execute");
        let parsed2: serde_json::Value = serde_json::from_str(&out2).expect("json");
        assert!(
            parsed2["activated"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v == "memory_write"))
        );
    }

    #[test]
    fn resolve_priority_tool_names_over_toolset() {
        let registry = Arc::new(ToolRegistry::new());
        let materialized = Arc::new(RwLock::new(crate::MaterializedToolSet::new()));
        let ctx = test_ctx(registry.clone(), materialized);
        let schemas = registry.get_definitions(None, None, &ctx);
        let args = Args {
            tool_names: vec!["memory_write".into()],
            toolset: Some("browser".into()),
            query: Some("browser".into()),
            limit: None,
        };
        let names = resolve_tool_names(&args, &ctx, &schemas);
        assert_eq!(names, vec!["memory_write".to_string()]);
    }
}
