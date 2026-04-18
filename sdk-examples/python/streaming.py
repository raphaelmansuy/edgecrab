"""Streaming Example — Watch tokens arrive.

Usage:
    python streaming.py
"""

from edgecrab import Agent


def main():
    agent = Agent("anthropic/claude-sonnet-4", streaming=True, quiet_mode=True)

    events = agent.stream_sync("Write a haiku about Python programming.")
    for event in events:
        if event.kind == "token":
            print(event.data, end="", flush=True)
        elif event.kind == "done":
            print("\n--- Done ---")
        else:
            print(f"[{event.kind}]: {event.data}")


if __name__ == "__main__":
    main()
