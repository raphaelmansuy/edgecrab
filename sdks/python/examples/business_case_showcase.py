"""Business-oriented EdgeCrab Python SDK example.

Run with:
    python3 examples/business_case_showcase.py
"""

import asyncio

from edgecrab import AsyncAgent, ModelCatalog

MODEL = "ollama/gemma4:latest"


async def main() -> None:
    print("EdgeCrab Python SDK — Business Case Showcase")
    print(f"Model: {MODEL}")

    catalog = ModelCatalog()
    est = catalog.estimate_cost("openai", "gpt-5-mini", 2000, 600)
    if est:
        print(
            "Reference cost estimate for openai/gpt-5-mini: "
            f"input=${est[0]:.4f}, output=${est[1]:.4f}, total=${est[2]:.4f}"
        )

    agent = AsyncAgent(
        MODEL,
        max_iterations=4,
        quiet_mode=True,
        skip_context_files=True,
        instructions=(
            "You are an operations copilot for a growth-stage software company. "
            "Be concise, structured, and business-minded."
        ),
    )

    await agent.memory.write(
        "memory",
        "Company context: AcmeCloud sells workflow automation to mid-market SaaS teams. "
        "Priority metrics are churn, expansion revenue, and support resolution speed.",
    )

    scenarios = [
        (
            "Support triage",
            "A customer says: 'Our onboarding export is failing and we need a fix before tomorrow morning.' "
            "Reply with a severity, owner, and next action.",
        ),
        (
            "Executive brief",
            "Turn these notes into a short VP-ready update: churn stable, enterprise pipeline up 18%, "
            "support backlog down 12%, one release risk in payments.",
        ),
        (
            "Sales preparation",
            "Create a short account brief for a renewal call with a customer who wants better audit logs, "
            "faster support, and predictable pricing.",
        ),
    ]

    for title, prompt in scenarios:
        print(f"\n=== {title} ===")
        reply = await agent.chat(prompt)
        print(reply)

    print("\n=== Batch customer quote summaries ===")
    quotes = await agent.batch(
        [
            "Summarize this quote in one sentence: 'We love the workflow builder but permissions are still confusing.'",
            "Summarize this quote in one sentence: 'The rollout was smooth and our ops team saved hours every week.'",
            "Summarize this quote in one sentence: 'We need stronger reporting before we expand to more teams.'",
        ]
    )
    for idx, item in enumerate(quotes, start=1):
        print(f"{idx}. {str(item).replace(chr(10), ' ')}")


if __name__ == "__main__":
    asyncio.run(main())
