"""Tutorial 4 — Session-Aware Support Bot.

FTS5 search finds relevant prior tickets in <10ms, injects them as
context so the agent resolves issues faster using org memory.
See: site/src/content/docs/tutorials/04-session-aware-support.md
"""
from __future__ import annotations
import asyncio
from edgecrab import AsyncAgent


async def resolve_ticket(user_message: str) -> None:
    # ── 1. Search prior sessions for similar issues ─────────────────
    prior_context = ""
    try:
        # Use a lightweight agent instance solely for session DB access.
        # search_sessions() does NOT make LLM calls — it queries the local DB.
        searcher = AsyncAgent(
            "copilot/gpt-5-mini",
            max_iterations=1,
            quiet_mode=True,
            skip_memory=True,
        )
        hits = await asyncio.to_thread(searcher.search_sessions, user_message, 3)
        if hits:
            prior_context = "\n\n--- RELEVANT PRIOR TICKETS ---\n"
            for h in hits:
                prior_context += (
                    f"[session={h.session_id[:8]}, score={h.score:.2f}]\n"
                    f"{h.snippet}\n\n"
                )
            print(f"[search] found {len(hits)} prior session(s)")
        else:
            print("[search] no matching prior sessions — starting cold")
    except Exception as e:
        print(f"[search] session DB unavailable ({e}), starting cold")

    # ── 2. Spin up agent with prior context pre-loaded ──────────────
    agent = AsyncAgent(
        "copilot/gpt-5-mini",
        max_iterations=6,
        quiet_mode=True,
        instructions=(
            "You are a senior support engineer. If prior tickets contain "
            "the answer, reference them by session ID. Always state the "
            "ROOT CAUSE before the FIX."
        ),
    )

    result = await agent.run(f"User ticket: {user_message}{prior_context}")

    print(f"Resolution:\n{result.response}")
    print(f"\nSession:  {result.session_id}")
    print(f"Cost:     ${result.total_cost:.6f}")


if __name__ == "__main__":
    asyncio.run(resolve_ticket("My K8s pods keep OOMKilling under load."))
