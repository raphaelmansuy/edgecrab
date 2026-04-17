"""Tutorial 1 — Cost-Aware Code Review.

Two-tier triage: cheap model flags risk, expensive model does deep review.
See: site/src/content/docs/tutorials/01-cost-aware-review.md
"""
from __future__ import annotations
import asyncio
import re
from edgecrab import AsyncAgent

DIFFS = [
    "Renamed `count` to `total_count` in stats.rs",
    "Added new auth middleware with JWT + refresh token logic",
    "Updated README typo: 'recieve' -> 'receive'",
]

RISK_RE = re.compile(r"RISK=(\d+)")


async def main() -> None:
    agent = AsyncAgent(
        "copilot/gpt-5-mini",
        max_iterations=4,
        quiet_mode=True,
        instructions=(
            "You are a strict code reviewer. For each diff, output ONE line: "
            "RISK=<1-10> SUMMARY=<10 words max>."
        ),
    )

    total_cost = 0.0
    escalated = 0

    for diff in DIFFS:
        # Tier 1 — cheap triage
        tri = await agent.run(f"Diff:\n{diff}")
        total_cost += tri.total_cost
        m = RISK_RE.search(tri.response)
        risk = int(m.group(1)) if m else 5

        head = tri.response.splitlines()[0] if tri.response else ""
        print(f"[tier1] risk={risk} | {head}")

        if risk < 7:
            print("  → APPROVED\n")
            continue

        # Tier 2 — hot-swap flagship model, preserve session
        escalated += 1
        await agent.set_model("copilot/claude-sonnet-4.6")
        deep = await agent.run(
            "The diff was flagged high-risk. "
            "Give a detailed review and 2 concrete fix suggestions."
        )
        total_cost += deep.total_cost
        head = deep.response.splitlines()[0] if deep.response else ""
        print(f"  [tier2] → {head}\n")

        await agent.set_model("copilot/gpt-5-mini")

    print("─" * 40)
    print(f"Total diffs:   {len(DIFFS)}")
    print(f"Escalated:     {escalated} ({100*escalated/len(DIFFS):.0f}%)")
    print(f"Total cost:    ${total_cost:.6f}")
    print(f"Per diff:      ${total_cost/len(DIFFS):.6f}")


if __name__ == "__main__":
    asyncio.run(main())
