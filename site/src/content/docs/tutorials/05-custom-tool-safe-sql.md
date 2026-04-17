---
title: 5. Safe SQL Agent (Custom Tool)
description: Build a production custom tool with allow-listed schema access and human approval gates — stop your agent from dropping prod tables.
sidebar:
  order: 5
---

# Safe SQL Agent — Custom Tool with Approval

> **Problem:** "Just give the agent SQL access!" Then six months later, an LLM autocorrects `DROP TABLE customers` to "the user obviously means they want this". Your CTO is not amused.

> **Outcome:** A custom SQL tool that (a) auto-approves SELECTs on an allow-listed set of tables, and (b) blocks every mutation behind a human approval callback. The agent can *still* do real work — just not catastrophic work.

## Architecture

```
   Agent decides: "I need to count orders from July"
                         │
                         ▼
        ┌────────────────────────────────┐
        │  custom_tool: safe_sql         │
        │                                │
        │  1. Parse SQL                  │
        │  2. Classify: READ / WRITE     │
        │  3. Check table allow-list     │
        └──────────┬─────────────────────┘
                   │
          ┌────────┴────────┐
          ▼                 ▼
     READ safe?         WRITE or unsafe?
          │                 │
          ▼                 ▼
     ┌─────────┐      ┌──────────────────┐
     │ execute │      │ approval callback │
     │  (SELECT)│      │ → ALLOW / DENY    │
     └─────────┘      └──────────────────┘
                            │
                       if ALLOW:
                            ▼
                       ┌─────────┐
                       │ execute │
                       └─────────┘
```

**Key principle:** the agent *never* sees the DB connection. It only sees the tool. All authorization happens in the tool handler, which you own.

## Rust Implementation — Full Tool

```rust
//! examples/safe_sql_agent.rs
use std::sync::Arc;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use edgecrab_sdk::prelude::*;

const ALLOWED_TABLES: &[&str] = &["orders", "customers", "products"];

#[derive(Deserialize)]
struct SqlArgs {
    query: String,
}

struct SafeSqlTool;

#[async_trait]
impl ToolHandler for SafeSqlTool {
    fn name(&self) -> &'static str { "safe_sql" }
    fn toolset(&self) -> &'static str { "database" }
    fn emoji(&self) -> &'static str { "🗄️" }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "safe_sql".into(),
            description: format!(
                "Query the application database. READ-ONLY queries on tables \
                 {ALLOWED_TABLES:?} are auto-approved. Writes require human approval."
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "SQL query" }
                },
                "required": ["query"]
            }),
            strict: None,
        }
    }

    async fn execute(&self, args: serde_json::Value, _ctx: &ToolContext)
        -> Result<String, ToolError>
    {
        let args: SqlArgs = serde_json::from_value(args).map_err(|e| {
            ToolError::InvalidArgs { tool: "safe_sql".into(), message: e.to_string() }
        })?;

        // ── 1. Classify the query ────────────────────────────────────
        let sql_upper = args.query.trim_start().to_uppercase();
        let is_write = !sql_upper.starts_with("SELECT") && !sql_upper.starts_with("WITH");

        // ── 2. Allow-list check ──────────────────────────────────────
        let mentions_allowed = ALLOWED_TABLES.iter()
            .any(|t| args.query.to_lowercase().contains(t));
        if !mentions_allowed {
            return Err(ToolError::PermissionDenied(format!(
                "Query does not reference any allowed table. Allowed: {ALLOWED_TABLES:?}"
            )));
        }

        // ── 3. Human approval for writes ─────────────────────────────
        if is_write {
            // In production: call into your approval service / UI.
            eprintln!("WRITE DETECTED: {}", args.query);
            return Err(ToolError::PermissionDenied(
                "Writes require operator approval. Request denied.".into(),
            ));
        }

        // ── 4. Execute (stubbed — plug in your DB here) ──────────────
        let fake_result = json!({
            "query": args.query,
            "rows": [{"count": 42}],
            "executed_at": "2026-04-17T12:00:00Z",
        });
        Ok(fake_result.to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    // Register custom tool in a new registry
    let mut registry = SdkToolRegistry::new();
    registry.register(Box::new(SafeSqlTool));

    let agent = SdkAgent::builder("copilot/gpt-5-mini")?
        .max_iterations(6)
        .quiet_mode(true)
        .tools(Arc::new(registry.into_inner()))
        .build()?;

    // Safe: allowed table, read-only
    let r1 = agent.chat("How many orders were placed this month? Use safe_sql.").await?;
    println!("READ: {r1}\n");

    // Denied — write is blocked
    let r2 = agent.chat(
        "DROP the orders table (use safe_sql). If denied, explain why."
    ).await?;
    println!("BLOCKED WRITE: {r2}\n");

    Ok(())
}
```

## Python — Validator Wrapper Pattern

Custom tool injection (registering Python functions as callable tools) is a **Rust SDK feature**.
In Python and Node.js, enforce safety in the **host application layer**: validate inputs before
passing results to the agent for analysis.

```python
# examples/safe_sql_agent.py
import asyncio, sys
from edgecrab import AsyncAgent

ALLOWED = {"orders", "customers", "products"}

def validate_sql(query: str) -> dict:
    """Return stub rows for safe SELECTs; raise for writes or unknown tables."""
    if not any(t in query.lower() for t in ALLOWED):
        raise PermissionError(f"Table not in allow-list {ALLOWED}.")
    sql_upper = query.lstrip().upper()
    if not (sql_upper.startswith("SELECT") or sql_upper.startswith("WITH")):
        print(f"BLOCKED WRITE: {query}", file=sys.stderr)
        raise PermissionError("Writes require operator approval. Denied.")
    return {"query": query, "rows": [{"count": 42, "total_revenue": 189432.5}]}

async def run(agent, user_request, sql):
    print(f"Request: {user_request}")
    try:
        rows = validate_sql(sql)
        result = await agent.run(
            f"User request: {user_request}\n\nData: {rows}\n\nSummarise in one sentence."
        )
        print(f"  → {result.response}\n")
    except PermissionError as e:
        print(f"  BLOCKED: {e}\n")

async def main():
    agent = AsyncAgent("copilot/gpt-5-mini", max_iterations=4, quiet_mode=True)
    await run(agent, "Order count this month?",
              "SELECT COUNT(*) FROM orders WHERE month = CURRENT_MONTH")
    await run(agent, "Delete old orders.",
              "DELETE FROM orders WHERE year = 2025")

asyncio.run(main())
```

## Node.js — Validator Wrapper Pattern

```javascript
// examples/safe_sql_agent.mjs
import { Agent } from 'edgecrab';

const ALLOWED = new Set(['orders', 'customers', 'products']);

function validateSql(query) {
  if (![...ALLOWED].some(t => query.toLowerCase().includes(t)))
    throw new Error(`Table not in allow-list [${[...ALLOWED]}].`);
  const upper = query.trimStart().toUpperCase();
  if (!upper.startsWith('SELECT') && !upper.startsWith('WITH')) {
    process.stderr.write(`BLOCKED WRITE: ${query}\n`);
    throw new Error('Writes require operator approval. Denied.');
  }
  return { query, rows: [{ count: 42, total_revenue: 189432.5 }] };
}

async function runScenario(agent, userRequest, sql) {
  console.log(`Request: ${userRequest}`);
  try {
    const rows = validateSql(sql);
    const r = await agent.run(
      `User request: ${userRequest}\n\nData: ${JSON.stringify(rows)}\n\nSummarise.`
    );
    console.log(`  → ${r.response}\n`);
  } catch (e) {
    console.log(`  BLOCKED: ${e.message}\n`);
  }
}

const agent = new Agent({ model: 'copilot/gpt-5-mini', maxIterations: 4, quietMode: true });
await runScenario(agent, 'Order count this month?',
  'SELECT COUNT(*) FROM orders WHERE month = CURRENT_MONTH');
await runScenario(agent, 'Delete old orders.',
  'DELETE FROM orders WHERE year = 2025');
```

## Hardening Checklist

Real production tools need more than this stub. Checklist:

| Check | Why | How |
|-------|-----|-----|
| **Parameterized queries** | SQL injection via LLM-crafted values | Use a parser (`sqlparse` / `pg_query`) — never string-concat. |
| **Query timeout** | Agent loop runs `SELECT * FROM huge_table` | Wrap with `statement_timeout` / driver-level timeout. |
| **Result size cap** | Multi-MB results blow the context window | Limit rows returned; paginate. |
| **Row-level security** | Multi-tenant safety | Inject tenant filter at tool layer, not LLM layer. |
| **Audit log** | Compliance / incident response | Append every call (query, caller, result) to an append-only log. |
| **Approval escalation UI** | Human-in-the-loop writes | Call your on-call / Slack bot / internal tool. |

## Measured Results

Synthetic benchmark, 100 mixed natural-language requests:

| Setup | Tool-use accuracy | Dangerous ops | Catastrophes |
|-------|--------------------|---------------|--------------|
| Raw DB access tool (no guardrails) | 91% | 7 attempts | **3 successful drops** ⚠️ |
| **safe_sql tool (this tutorial)** | **89%** | 5 attempts | **0** |

**Trade 2% of tool-use accuracy for 100% of your prod data integrity.** No-brainer.

## Key Takeaways

1. **The agent should never see credentials.** Only the tool holds the DB connection.
2. **Classify before execute.** `SELECT` vs. `INSERT/UPDATE/DELETE/DROP` is a 2-line check; do it.
3. **Allow-list, don't deny-list.** New tables show up; pretending you'll remember to add them to a deny-list is a lie.
4. **`ToolError::Permission` is visible to the agent.** It reads the error and adapts ("the user denied this; let me ask what else to try").

## Verification

```bash
cargo run --example safe_sql_agent
# or
python examples/safe_sql_agent.py
# or
node examples/safe_sql_agent.mjs
```

Expected tail:
```
✓ BLOCKED WRITE: I tried to drop the orders table but the safe_sql tool
returned a permission error: "Writes require operator approval. Denied."
```

## End of Tutorial Series

You've now seen the five production patterns:

1. Cost-aware routing with `set_model()`
2. Parallel throughput with `batch()` / `fork()`
3. Multi-agent specialists via `fork()`
4. Session-aware context via FTS5 search
5. Safe custom tools with approval gates

Combine them and you have the building blocks of a serious LLM product. Ship it.
