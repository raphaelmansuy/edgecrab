"""
EdgeCrab Python SDK — E2E Smoke Test

Exercises the core Python SDK API against local Ollama.
Run with:
    python e2e_smoke.py
"""
import asyncio
import sys
from pathlib import Path

MODEL = "ollama/gemma4:latest"
CORE_FEATURES = [
    "ModelCatalog.provider_ids",
    "ModelCatalog.pricing",
    "ModelCatalog.flat_catalog",
    "ModelCatalog.default_model_for",
    "ModelCatalog.estimate_cost",
    "AsyncAgent.chat",
    "AsyncAgent.run",
    "AsyncAgent.run_conversation",
    "AsyncAgent.stream",
    "AsyncAgent.history",
    "AsyncAgent.session_id",
    "AsyncAgent.chat_in_cwd",
    "AsyncAgent.fork",
    "AsyncAgent.batch",
    "AsyncAgent.set_model",
    "AsyncAgent.session_snapshot",
    "AsyncAgent.export",
    "AsyncAgent.tool_names",
    "AsyncAgent.toolset_summary",
    "AsyncAgent.compress",
    "AsyncAgent.new_session",
    "AsyncAgent.search_sessions",
    "AsyncAgent.list_sessions",
    "Session.get_messages",
    "Session.rename_session",
    "Session.prune_sessions",
    "Session.stats",
    "AsyncMemoryManager.read_write",
]
covered: set[str] = set()


def ok(label: str) -> None:
    print(f"  ✓ {label}")


def section(title: str) -> None:
    print(f"\n── {title} ──")


def mark(feature: str, label: str) -> None:
    covered.add(feature)
    ok(label)


def expect_contains(text: str | None, expected: str) -> None:
    actual = str(text or "").strip().upper()
    if expected.upper() not in actual:
        raise AssertionError(f"Expected {text!r} to include {expected!r}")


async def main() -> None:
    print("EdgeCrab Python SDK — E2E Smoke Test")
    print(f"Model: {MODEL}")
    print("═══════════════════════════════════════")

    try:
        from edgecrab import AsyncAgent, Session
        from edgecrab._native import ModelCatalog
    except ImportError as e:
        print(f"FATAL: cannot import edgecrab: {e}")
        sys.exit(1)

    section("1. ModelCatalog")
    catalog = ModelCatalog()
    providers = catalog.provider_ids()
    assert providers, "No providers in catalog"
    assert "ollama" in providers, f"ollama missing from {providers}"
    mark("ModelCatalog.provider_ids", f"{len(providers)} providers in catalog")

    pricing = catalog.pricing("openai", "gpt-4o")
    mark("ModelCatalog.pricing", f"pricing lookup → {pricing}")

    flat = catalog.flat_catalog()
    assert flat, "flat_catalog empty"
    mark("ModelCatalog.flat_catalog", f"flat_catalog → {len(flat)} models")

    default_model = catalog.default_model_for("openai")
    assert default_model, "default_model_for returned None"
    mark("ModelCatalog.default_model_for", f"default_model_for('openai') → {default_model}")

    est = catalog.estimate_cost("openai", "gpt-4o", 1000, 200)
    mark("ModelCatalog.estimate_cost", f"cost estimate → {est}")

    section("2. Core agent methods")
    agent = AsyncAgent(MODEL, max_iterations=3, quiet_mode=True,
                       skip_context_files=True, skip_memory=True)
    reply = await agent.chat("Reply with exactly: PONG")
    expect_contains(reply, "PONG")
    mark("AsyncAgent.chat", f"chat → {reply[:60].replace(chr(10), ' ')}")

    result = await agent.run("Reply with exactly: OK")
    assert result.response, "response empty"
    expect_contains(result.response, "OK")
    mark("AsyncAgent.run", f"run → {result.response[:40].replace(chr(10), ' ')}")

    conv = await agent.run_conversation(
        "Reply with exactly: CONV_OK",
        system="You are terse. Obey exactly.",
        history=["Earlier note: keep replies short."],
    )
    expect_contains(conv.response, "CONV_OK")
    mark("AsyncAgent.run_conversation", f"run_conversation → {conv.response[:40].replace(chr(10), ' ')}")

    await agent.set_reasoning_effort("low")
    await agent.set_streaming(True)
    stream_parts: list[str] = []
    async for event in agent.stream("Reply with exactly: STREAM_OK"):
        if event.event_type == "token":
            stream_parts.append(event.data)
    streamed = "".join(stream_parts)
    expect_contains(streamed or str(stream_parts), "STREAM_OK")
    mark("AsyncAgent.stream", f"stream → {streamed[:40].replace(chr(10), ' ')}")

    msgs = agent.history
    assert msgs, "history empty after run()"
    mark("AsyncAgent.history", f"{len(msgs)} messages in history")

    sid = agent.session_id
    assert sid, "session_id is None"
    mark("AsyncAgent.session_id", f"session_id = {sid[:12]}…")

    cwd_reply = await agent.chat_in_cwd("Reply with exactly: CWD_OK", str(Path.cwd()))
    expect_contains(cwd_reply, "CWD_OK")
    mark("AsyncAgent.chat_in_cwd", f"chat_in_cwd → {cwd_reply[:40].replace(chr(10), ' ')}")

    section("3. Batch, fork, and state")
    fork_a = await agent.fork()
    fork_b = await agent.fork()
    ra, rb = await asyncio.gather(
        fork_a.chat("Reply with exactly: FORK_A"),
        fork_b.chat("Reply with exactly: FORK_B"),
    )
    expect_contains(ra, "FORK_A")
    expect_contains(rb, "FORK_B")
    mark("AsyncAgent.fork", f"fork replies → {ra[:10]} / {rb[:10]}")

    results = await agent.batch(["Reply with exactly: YES", "Reply with exactly: NO"])
    assert len(results) == 2, f"batch returned {len(results)} items"
    expect_contains(results[0], "YES")
    expect_contains(results[1], "NO")
    mark("AsyncAgent.batch", f"batch → {[str(r).strip() for r in results]}")

    snapshot = await agent.session_snapshot()
    assert isinstance(snapshot, dict), "session_snapshot invalid"
    mark("AsyncAgent.session_snapshot", f"session_snapshot → {snapshot.get('message_count', 'ok')} messages")

    exported = await agent.export()
    assert isinstance(exported, dict) and isinstance(exported.get("messages"), list), "export invalid"
    mark("AsyncAgent.export", f"export → {len(exported['messages'])} messages")

    section("4. Metadata and persistence")
    await agent.set_model(MODEL)
    mark("AsyncAgent.set_model", f"set_model → {agent.model}")

    tool_names = await agent.tool_names()
    assert isinstance(tool_names, list), "tool_names returned a non-list value"
    mark("AsyncAgent.tool_names", f"tool_names → {len(tool_names)} tools")

    toolset_summary = await agent.toolset_summary()
    assert isinstance(toolset_summary, list), "toolset_summary returned a non-list value"
    mark("AsyncAgent.toolset_summary", f"toolset_summary → {len(toolset_summary)} groups")

    hits = agent.search_sessions("PONG", 5)
    mark("AsyncAgent.search_sessions", f"search_sessions → {len(hits)} hits")

    sessions = agent.list_sessions(5)
    mark("AsyncAgent.list_sessions", f"list_sessions → {len(sessions)} sessions")

    db = Session()
    messages = db.get_messages(sid)
    assert isinstance(messages, list), "Session.get_messages failed"
    mark("Session.get_messages", f"Session.get_messages → {len(messages)} messages")

    db.rename_session(sid, "Python SDK E2E")
    mark("Session.rename_session", "rename_session applied")

    pruned = db.prune_sessions(36500)
    mark("Session.prune_sessions", f"prune_sessions → {pruned} deleted")

    stats = db.stats()
    mark("Session.stats", f"session stats → {stats.total_sessions} sessions / {stats.total_messages} messages")

    mem = agent.memory
    token = f"python-sdk-{int(asyncio.get_running_loop().time() * 1000)}"
    await mem.write("memory", token)
    content = await mem.read("memory")
    assert token in content, "memory write/read failed"
    entries = await mem.entries("memory")
    removed = await mem.remove("memory", token)
    assert removed, "memory remove failed"
    mark("AsyncMemoryManager.read_write", f"memory round-trip → {len(entries)} entries")

    await agent.compress()
    mark("AsyncAgent.compress", "compress completed")

    await agent.new_session()
    assert agent.history == [], "new_session did not clear history"
    mark("AsyncAgent.new_session", "new_session cleared history")

    pct = (len(covered) / len(CORE_FEATURES)) * 100
    print(f"\nCore API coverage: {len(covered)}/{len(CORE_FEATURES)} ({pct:.1f}%)")
    if pct < 80:
        raise AssertionError(f"Coverage below target: {pct:.1f}%")

    print("\n═══════════════════════════════════════")
    print("E2E smoke test PASSED ✓")
    print("Python SDK coverage target PASSED ✓")


if __name__ == "__main__":
    asyncio.run(main())
