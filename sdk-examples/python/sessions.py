"""Session Management Example — List and search sessions.

Usage:
    python sessions.py
"""

from edgecrab import Agent, Config, edgecrab_home


def main():
    print(f"EdgeCrab home: {edgecrab_home()}")

    # Load config
    config = Config.load()
    print(f"Default model: {config.default_model}")

    # Create an agent
    agent = Agent("anthropic/claude-sonnet-4", quiet_mode=True)

    # List recent sessions
    sessions = agent.list_sessions(limit=5)
    print(f"\nRecent sessions ({len(sessions)}):")
    for s in sessions:
        print(f"  - {s.id[:8]}... | model={s.model} | messages={s.message_count}")

    # Search sessions
    hits = agent.search_sessions("hello", limit=3)
    print(f"\nSearch results ({len(hits)}):")
    for h in hits:
        print(f"  - session={h.session_id[:8]}... | score={h.score:.2f}")
        print(f"    snippet: {h.snippet[:80]}...")

    # Show tool info
    tools = agent.tool_names()
    print(f"\nAvailable tools ({len(tools)}): {', '.join(tools[:5])}...")

    summary = agent.toolset_summary()
    print("Toolsets:")
    for name, count in summary:
        print(f"  - {name}: {count} tools")


if __name__ == "__main__":
    main()
