"""Tutorial 5 — Safe SQL Agent.

Python-layer enforcement: allow-listed tables, read-only guard.
The host application pre-validates every SQL query before the agent
processes it. Writes and unknown tables are blocked at the Python layer
— no LLM can override it.

Usage:
    python safe_sql_agent.py

Note: Custom tool injection (registering Python functions as callable
tools) is a Rust SDK feature. Python/Node.js enforce safety via the
validator wrapper pattern shown here.
See: site/src/content/docs/tutorials/05-custom-tool-safe-sql.md
"""
from __future__ import annotations

import asyncio
import sys
from typing import Any

from edgecrab import AsyncAgent

ALLOWED = {"orders", "customers", "products"}

# ── Validation layer (runs in Python, before the agent sees results) ──────────

def validate_sql(query: str) -> dict[str, Any]:
    """Return mock rows for allowed SELECTs, raise for everything else."""
    # 1. Allow-list check
    if not any(t in query.lower() for t in ALLOWED):
        raise PermissionError(
            f"Query must reference one of {ALLOWED}. Query rejected."
        )
    # 2. Block writes
    sql_upper = query.lstrip().upper()
    if not (sql_upper.startswith("SELECT") or sql_upper.startswith("WITH")):
        print(f"BLOCKED WRITE: {query}", file=sys.stderr)
        raise PermissionError("Writes require operator approval. Request denied.")
    # 3. Stub result — plug in your DB pool here
    return {
        "query": query,
        "rows": [{"count": 42, "total_revenue": 189_432.50}],
        "executed_at": "2026-04-17T12:00:00Z",
    }


# ── Helper: run one scenario with pre-validation ──────────────────────────────

async def run_scenario(agent: AsyncAgent, user_request: str, sql_to_validate: str) -> None:
    """Validate SQL, then ask the agent to interpret the result."""
    print(f"Request: {user_request}")
    try:
        rows = validate_sql(sql_to_validate)
        prompt = (
            f"User request: {user_request}\n\n"
            f"Query result: {rows}\n\n"
            "Summarise the data in one sentence."
        )
        result = await agent.run(prompt)
        print(f"  Response: {result.response}")
        print(f"  Cost:     ${result.total_cost:.6f}\n")
    except PermissionError as exc:
        print(f"  BLOCKED: {exc}\n")


# ── Main ──────────────────────────────────────────────────────────────────────

async def main() -> None:
    agent = AsyncAgent(
        "copilot/gpt-5-mini",
        max_iterations=4,
        quiet_mode=True,
        instructions=(
            "You are a data analyst. You receive pre-validated SQL results. "
            "Summarise in plain English. Never suggest running additional queries."
        ),
    )

    # Safe: read-only on allowed table
    await run_scenario(
        agent,
        "How many orders were placed this month?",
        "SELECT COUNT(*) AS count FROM orders WHERE month = CURRENT_MONTH",
    )

    # Blocked: write mutation
    await run_scenario(
        agent,
        "Delete all orders from last year.",
        "DELETE FROM orders WHERE year = 2025",
    )

    # Blocked: unknown table
    await run_scenario(
        agent,
        "Show me all users.",
        "SELECT * FROM users",
    )


if __name__ == "__main__":
    asyncio.run(main())
