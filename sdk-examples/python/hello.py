"""Hello World — Minimal EdgeCrab Python SDK Example.

Usage:
    python hello.py
"""

from edgecrab import Agent


def main():
    # Create an agent with a model string
    agent = Agent("anthropic/claude-sonnet-4", quiet_mode=True)

    # Send a message (sync)
    reply = agent.chat_sync("What is EdgeCrab? Answer in one sentence.")
    print(f"Agent: {reply}")


if __name__ == "__main__":
    main()
