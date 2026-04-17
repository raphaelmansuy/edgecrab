"""Multi-Agent Pipeline — Orchestrate specialist agents.

Demonstrates how to fork agents, assign different models, and compose
results from multiple specialists — a pattern for supervisor/worker
architectures.

Usage:
    python multi_agent_pipeline.py
"""

import asyncio
from edgecrab import AsyncAgent


async def main():
    # ── 1. Coordinator establishes context ───────────────────────────
    coordinator = AsyncAgent(
        "copilot/claude-sonnet-4.6",
        max_iterations=5,
        quiet_mode=True,
        instructions="You are a technical lead coordinating an API design.",
    )

    await coordinator.chat(
        "We're designing a REST API for a bookmark manager. "
        "Users can create, list, tag, and share bookmarks."
    )

    # ── 2. Fork specialists — each inherits the context ─────────────
    api_designer = await coordinator.fork()
    example_writer = await coordinator.fork()
    test_writer = await coordinator.fork()

    # Use a cheaper model for the simpler tasks
    await example_writer.set_model("copilot/gpt-5-mini")
    await test_writer.set_model("copilot/gpt-5-mini")

    # ── 3. Run all specialists in parallel ───────────────────────────
    endpoints_task = api_designer.chat(
        "Design the REST endpoints. Return a markdown table: "
        "Method | Path | Description | Request Body | Response"
    )
    examples_task = example_writer.chat(
        "Write 5 curl examples covering CRUD operations and tagging."
    )
    tests_task = test_writer.chat(
        "Write 5 pytest test cases using httpx for the bookmarks API."
    )

    endpoints, examples, tests = await asyncio.gather(
        endpoints_task, examples_task, tests_task
    )

    print("=== API Endpoints ===")
    print(endpoints[:800])
    print("\n=== Curl Examples ===")
    print(examples[:800])
    print("\n=== Test Cases ===")
    print(tests[:800])

    # ── 4. Coordinator synthesizes the final doc ─────────────────────
    summary = await coordinator.chat(
        f"Here is our API design:\n\n{endpoints[:500]}\n\n"
        "Write a one-paragraph executive summary and list 3 next steps."
    )
    print("\n=== Executive Summary ===")
    print(summary)


if __name__ == "__main__":
    asyncio.run(main())
