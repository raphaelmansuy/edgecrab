"""EdgeCrab Python SDK — Basic usage examples.

Covers: sync/async chat, streaming, conversation results, memory,
forking, batch processing, model switching, and session management.

Usage:
    pip install edgecrab
    export OPENAI_API_KEY="sk-..."   # or set GITHUB_TOKEN for Copilot
    python basic_usage.py
"""

import asyncio
from edgecrab import Agent, AsyncAgent, ModelCatalog, Session


# ── 1. Sync Chat (simplest possible) ────────────────────────────────

def sync_example():
    """One-liner agent usage — blocking, great for scripts."""
    agent = Agent("copilot/claude-sonnet-4.6", quiet_mode=True)
    reply = agent.chat_sync("What is the capital of France?")
    print(f"Reply: {reply}")


# ── 2. Async Chat ───────────────────────────────────────────────────

async def async_example():
    """Non-blocking agent — ideal for web frameworks (FastAPI, etc.)."""
    agent = AsyncAgent("copilot/claude-sonnet-4.6", quiet_mode=True)
    reply = await agent.chat("Explain quantum computing in one sentence.")
    print(f"Reply: {reply}")


# ── 3. Streaming ────────────────────────────────────────────────────

async def streaming_example():
    """Watch tokens arrive in real-time."""
    agent = AsyncAgent("copilot/claude-sonnet-4.6", quiet_mode=True)
    async for event in agent.stream("Write a haiku about Rust programming"):
        if event.event_type == "token":
            print(event.data, end="", flush=True)
        elif event.event_type == "done":
            print()  # newline at end


# ── 4. Full Conversation Result ─────────────────────────────────────

async def conversation_result_example():
    """Get detailed results: token usage, cost, and metadata."""
    agent = AsyncAgent(
        "copilot/claude-sonnet-4.6",
        max_iterations=10,
        quiet_mode=True,
    )
    result = await agent.run("What are the SOLID principles? Be concise.")
    print(f"Response: {result.response[:200]}...")
    print(f"Model: {result.model}")
    print(f"API calls: {result.api_calls}")
    print(f"Input tokens: {result.input_tokens}")
    print(f"Output tokens: {result.output_tokens}")
    print(f"Total cost: ${result.total_cost:.4f}")
    print(f"Interrupted: {result.interrupted}")
    print(f"Budget exhausted: {result.budget_exhausted}")


# ── 5. Persistent Memory ────────────────────────────────────────────

async def memory_example():
    """Read, write, and manage the agent's persistent memory."""
    agent = AsyncAgent("copilot/claude-sonnet-4.6", quiet_mode=True)
    mem = agent.memory

    # Write a memory entry
    await mem.write("memory", "User prefers concise answers")

    # Read all memory
    content = await mem.read("memory")
    print(f"Memory content: {content}")

    # List entries
    entries = await mem.entries("memory")
    for entry in entries:
        print(f"  - {entry}")

    # Remove an entry
    removed = await mem.remove("memory", "concise answers")
    print(f"Removed: {removed}")


# ── 6. Fork — Branch Conversations ──────────────────────────────────

async def fork_example():
    """Fork creates an independent copy with shared history."""
    agent = AsyncAgent("copilot/claude-sonnet-4.6", quiet_mode=True)

    # Establish context
    await agent.chat("Remember: I'm building a Python web app with FastAPI.")

    # Fork for a sub-task — inherits full history
    forked = await agent.fork()
    reply = await forked.chat("What middleware should I add?")
    print(f"Forked agent: {reply[:200]}")

    # Original agent is unaffected
    original_reply = await agent.chat("What were we talking about?")
    print(f"Original agent: {original_reply[:200]}")


# ── 7. Batch Processing — Parallel Prompts ──────────────────────────

async def batch_example():
    """Send multiple prompts in parallel — each runs in an isolated fork."""
    agent = AsyncAgent("copilot/claude-sonnet-4.6", quiet_mode=True)

    prompts = [
        "Translate to French: 'Hello, world!'",
        "Translate to Spanish: 'Hello, world!'",
        "Translate to Japanese: 'Hello, world!'",
        "Translate to German: 'Hello, world!'",
    ]

    print(f"Sending {len(prompts)} prompts in parallel...")
    results = await agent.batch(prompts)

    for prompt, result in zip(prompts, results):
        lang = prompt.split(":")[0].replace("Translate to ", "")
        print(f"  {lang}: {result or 'ERROR'}")


# ── 8. Model Hot-Swap ───────────────────────────────────────────────

async def model_swap_example():
    """Switch models mid-conversation without losing context."""
    agent = AsyncAgent("copilot/claude-sonnet-4.6", quiet_mode=True)

    # Start with the powerful model
    await agent.chat("I'm designing a database schema for a blog.")
    print(f"Started with: {agent.model}")

    # Switch to cheaper model for simple follow-ups
    await agent.set_model("copilot/gpt-5-mini")
    print(f"Switched to: {agent.model}")

    reply = await agent.chat("List the tables we need.")
    print(f"Reply: {reply[:200]}")


# ── 9. Model Catalog — Offline Exploration ───────────────────────────

def catalog_example():
    """Explore providers, models, pricing — no API key needed."""
    catalog = ModelCatalog()

    providers = catalog.provider_ids()
    print(f"{len(providers)} providers: {', '.join(providers[:5])}...")

    # Pricing for common models
    for provider, model in [
        ("openai", "gpt-4o"),
        ("openai", "gpt-5-mini"),
        ("copilot", "gpt-5-mini"),
    ]:
        pricing = catalog.pricing(provider, model)
        if pricing:
            print(f"  {provider}/{model}: ${pricing[0]:.2f} in / ${pricing[1]:.2f} out per 1M tokens")

    # Pre-flight cost estimation
    cost = catalog.estimate_cost("openai", "gpt-5-mini", 10_000, 2_000)
    if cost:
        print(f"\n  Estimated cost (10K in / 2K out): ${cost[2]:.6f}")


# ── 10. Session Management ──────────────────────────────────────────

def session_management_example():
    """Browse, search, rename, and prune sessions."""
    agent = Agent("copilot/claude-sonnet-4.6", quiet_mode=True)

    # List recent sessions
    sessions = agent.list_sessions(limit=5)
    print(f"Recent sessions ({len(sessions)}):")
    for s in sessions:
        print(f"  {s.session_id[:8]}... | model={s.model} | msgs={s.message_count}")

    # Search across all sessions
    hits = agent.search_sessions("hello", limit=3)
    print(f"\nSearch hits ({len(hits)}):")
    for h in hits:
        print(f"  {h.session_id[:8]}... | score={h.score:.2f} | {h.snippet[:60]}")

    # Show available tools
    tools = agent.tool_names()
    print(f"\nTools ({len(tools)}): {', '.join(tools[:5])}...")

    summary = agent.toolset_summary()
    print("Toolsets:")
    for name, count in summary:
        print(f"  {name}: {count} tools")


# ── 11. Standalone Session DB ───────────────────────────────────────

def standalone_session_example():
    """Access session data without creating an agent."""
    session = Session()  # Uses default ~/.edgecrab/state.db

    # Stats
    stats = session.stats()
    print(f"Sessions: {stats.total_sessions}")
    print(f"Messages: {stats.total_messages}")
    print(f"DB size:  {stats.db_size_bytes / 1024:.1f} KB")

    # Browse
    sessions = session.list_sessions(limit=3)
    for s in sessions:
        print(f"\n  {s.session_id[:8]}... ({s.message_count} msgs)")
        messages = session.get_messages(s.session_id)
        for msg in messages[:2]:
            role = msg.get("role", "?")
            text = str(msg.get("content", ""))[:80]
            print(f"    [{role}] {text}")


# ── Run all examples ────────────────────────────────────────────────

if __name__ == "__main__":
    examples = [
        ("Sync Chat", sync_example, False),
        ("Async Chat", async_example, True),
        ("Streaming", streaming_example, True),
        ("Conversation Result", conversation_result_example, True),
        ("Memory", memory_example, True),
        ("Fork", fork_example, True),
        ("Batch Processing", batch_example, True),
        ("Model Swap", model_swap_example, True),
        ("Model Catalog", catalog_example, False),
        ("Session Management", session_management_example, False),
        ("Standalone Sessions", standalone_session_example, False),
    ]

    for name, fn, is_async in examples:
        print(f"\n{'='*60}")
        print(f"  {name}")
        print(f"{'='*60}")
        if is_async:
            asyncio.run(fn())
        else:
            fn()
