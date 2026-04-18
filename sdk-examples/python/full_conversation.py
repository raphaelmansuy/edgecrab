"""Full Conversation Example — Get detailed results.

Usage:
    python full_conversation.py
"""

from edgecrab import Agent


def main():
    agent = Agent(
        "anthropic/claude-sonnet-4",
        max_iterations=10,
        temperature=0.5,
        quiet_mode=True,
    )

    # Run a full conversation to get detailed results
    result = agent.run_sync("What is 2 + 2? Answer briefly.")

    print(f"Reply: {result.reply}")
    print(f"Model: {result.model}")
    print(f"API calls: {result.api_calls}")
    print(f"Iterations: {result.iterations}")
    print(f"Elapsed: {result.elapsed_secs:.2f}s")


if __name__ == "__main__":
    main()
