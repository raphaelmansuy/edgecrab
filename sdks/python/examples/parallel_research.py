"""Tutorial 2 — Parallel Research Pipeline.

Wall-clock time becomes the slowest call, not the sum. ~5× throughput.
See: site/src/content/docs/tutorials/02-parallel-research.md
"""
from __future__ import annotations
import asyncio
import time
from edgecrab import AsyncAgent

QUERIES = [
    "Summarize the Rust borrow checker in one sentence.",
    "Summarize Python's GIL in one sentence.",
    "Summarize Go's goroutines in one sentence.",
    "Summarize Erlang's actor model in one sentence.",
    "Summarize Haskell's laziness in one sentence.",
]


async def main() -> None:
    agent = AsyncAgent(
        "copilot/gpt-5-mini", max_iterations=3, quiet_mode=True
    )

    t0 = time.perf_counter()
    results = await agent.batch(QUERIES)
    elapsed = time.perf_counter() - t0

    for q, r in zip(QUERIES, results):
        print(f"Q: {q}\n  → {r or 'ERROR'}\n")

    print(f"── wall-clock: {elapsed:.2f}s ──")


if __name__ == "__main__":
    asyncio.run(main())
