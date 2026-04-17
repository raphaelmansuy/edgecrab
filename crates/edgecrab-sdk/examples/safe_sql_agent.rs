//! # Tutorial 5 — Safe SQL Agent with Custom Tool
//!
//! Custom ToolHandler with allow-listed schema access.  
//! SELECTs on approved tables auto-execute; mutations are blocked.
//!
//! ```bash
//! cargo run --example safe_sql_agent
//! ```

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use edgecrab_sdk::prelude::*;

const ALLOWED_TABLES: &[&str] = &["orders", "customers", "products"];

// ── Custom tool definition ────────────────────────────────────────────

#[derive(Deserialize)]
struct SqlArgs {
    query: String,
}

struct SafeSqlTool;

#[async_trait]
impl ToolHandler for SafeSqlTool {
    fn name(&self) -> &'static str {
        "safe_sql"
    }
    fn toolset(&self) -> &'static str {
        "database"
    }
    fn emoji(&self) -> &'static str {
        "🗄️"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "safe_sql".into(),
            description: format!(
                "Query the application database. \
                 READ-ONLY SELECTs on tables {ALLOWED_TABLES:?} are auto-approved. \
                 Writes are permanently blocked."
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "SQL query to execute"
                    }
                },
                "required": ["query"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: SqlArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "safe_sql".into(),
            message: e.to_string(),
        })?;

        // ── 1. Allow-list check ──────────────────────────────────────
        let mentions_allowed = ALLOWED_TABLES
            .iter()
            .any(|t| args.query.to_lowercase().contains(t));
        if !mentions_allowed {
            return Err(ToolError::PermissionDenied(format!(
                "Query does not reference any allowed table. Allowed: {ALLOWED_TABLES:?}"
            )));
        }

        // ── 2. Block writes ──────────────────────────────────────────
        let sql_upper = args.query.trim_start().to_uppercase();
        let is_write = !sql_upper.starts_with("SELECT") && !sql_upper.starts_with("WITH");
        if is_write {
            eprintln!("🚨 BLOCKED WRITE: {}", args.query);
            return Err(ToolError::PermissionDenied(
                "Writes require operator approval. Request denied.".into(),
            ));
        }

        // ── 3. Execute (stubbed — plug in your DB here) ──────────────
        let fake_result = json!({
            "query": args.query,
            "rows": [{"count": 42, "total_revenue": 189_432.50}],
            "executed_at": "2026-04-17T12:00:00Z",
        });
        Ok(fake_result.to_string())
    }
}

// ── Agent setup ───────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Register the custom tool in a new registry
    let mut registry = SdkToolRegistry::new();
    registry.register(Box::new(SafeSqlTool));

    let agent = SdkAgent::builder("copilot/gpt-5-mini")?
        .max_iterations(6)
        .quiet_mode(true)
        .tools(Arc::new(registry.into_inner()))
        .build()?;

    // ── Safe: read-only on allowed table ────────────────────────────
    let r1 = agent
        .chat("How many orders were placed this month? Use safe_sql.")
        .await?;
    println!("✓ READ:\n{r1}\n");

    // ── Blocked: write mutation ──────────────────────────────────────
    let r2 = agent
        .chat("DROP the orders table using safe_sql. If blocked, explain why.")
        .await?;
    println!("✓ BLOCKED WRITE:\n{r2}\n");

    Ok(())
}
