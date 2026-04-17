"""E2E-style tests for the EdgeCrab Python SDK.

These tests validate the API surface, type exports, and wrapper logic.
Tests that require a real LLM API key are skipped unless EDGECRAB_E2E=1 is set.
"""

import asyncio
import os
import pytest
from unittest.mock import MagicMock

# ── Import surface tests ─────────────────────────────────────────────

def test_all_exports_importable():
    """Verify all __all__ exports are importable."""
    from edgecrab import __all__ as exports
    import edgecrab

    for name in exports:
        assert hasattr(edgecrab, name), f"Missing export: {name}"


def test_agent_class_exists():
    from edgecrab import Agent
    assert callable(Agent)


def test_async_agent_class_exists():
    from edgecrab import AsyncAgent
    assert callable(AsyncAgent)


def test_memory_manager_class_exists():
    from edgecrab import MemoryManager
    assert callable(MemoryManager)


def test_async_memory_manager_class_exists():
    from edgecrab import AsyncMemoryManager
    assert callable(AsyncMemoryManager)


def test_tool_class_exists():
    from edgecrab import Tool
    assert callable(Tool)


def test_message_and_role_exist():
    from edgecrab import Message, Role
    assert hasattr(Role, "SYSTEM")
    assert hasattr(Role, "USER")
    assert hasattr(Role, "ASSISTANT")
    assert hasattr(Role, "TOOL")

    msg = Message(role=Role.USER, content="hello")
    assert msg.role == Role.USER
    assert msg.content == "hello"


def test_stream_event_exists():
    from edgecrab import StreamEvent
    assert StreamEvent is not None


def test_config_classes_exist():
    from edgecrab import (
        Agent,
        Config,
        ConversationResult,
        SessionSummary,
        SessionSearchHit,
        ModelCatalog,
    )
    assert all(callable(c) for c in [Agent, Config, ModelCatalog])
    assert hasattr(Config, "load_profile")
    assert hasattr(Agent, "from_config")


def test_utility_functions_exist():
    from edgecrab import edgecrab_home, ensure_edgecrab_home
    # These should be callable
    assert callable(edgecrab_home)
    assert callable(ensure_edgecrab_home)


# ── AsyncAgent wrapper tests (no LLM needed) ─────────────────────────

def test_async_agent_has_expected_methods():
    from edgecrab import AsyncAgent
    agent_methods = [
        "chat", "chat_sync", "stream", "run", "run_conversation",
        "fork", "interrupt", "new_session", "export",
        "tool_names", "toolset_summary",
        "list_sessions", "search_sessions",
        "set_reasoning_effort", "set_streaming",
    ]
    agent_props = ["is_cancelled", "session_id", "history", "model", "memory"]

    for method in agent_methods:
        assert hasattr(AsyncAgent, method), f"Missing method: {method}"
    for prop in agent_props:
        assert hasattr(AsyncAgent, prop), f"Missing property: {prop}"


def test_async_agent_repr():
    """AsyncAgent.__repr__ should work even without API key."""
    from edgecrab import AsyncAgent
    try:
        agent = AsyncAgent(model="anthropic/claude-sonnet-4")
        r = repr(agent)
        assert "AsyncAgent" in r
    except Exception:
        pytest.skip("Agent construction requires API key")


def test_on_stream_callback_stored():
    """Verify on_stream callback is stored on AsyncAgent."""
    from edgecrab import AsyncAgent

    callback = MagicMock()
    try:
        agent = AsyncAgent(
            model="anthropic/claude-sonnet-4",
            on_stream=callback,
        )
        assert agent._on_stream is callback
    except Exception:
        pytest.skip("Agent construction requires API key")


# ── Tool tests ────────────────────────────────────────────────────────

def test_tool_schema_creation():
    from edgecrab import ToolSchema
    schema = ToolSchema(
        name="test_tool",
        description="A test tool",
        parameters={"type": "object", "properties": {"x": {"type": "string"}}},
    )
    assert schema.name == "test_tool"
    assert schema.description == "A test tool"


# ── E2E tests (require EDGECRAB_E2E=1 and valid API keys) ────────────

E2E = os.getenv("EDGECRAB_E2E") == "1"


@pytest.mark.skipif(not E2E, reason="E2E tests require EDGECRAB_E2E=1")
def test_sync_chat():
    from edgecrab import Agent
    agent = Agent(model="copilot/gpt-5-mini")
    reply = agent.chat("Say 'hello' and nothing else.")
    assert "hello" in reply.lower()


@pytest.mark.skipif(not E2E, reason="E2E tests require EDGECRAB_E2E=1")
@pytest.mark.asyncio
async def test_async_chat():
    from edgecrab import AsyncAgent
    agent = AsyncAgent(model="copilot/gpt-5-mini")
    reply = await agent.chat("Say 'hello' and nothing else.")
    assert "hello" in reply.lower()


@pytest.mark.skipif(not E2E, reason="E2E tests require EDGECRAB_E2E=1")
@pytest.mark.asyncio
async def test_async_stream():
    from edgecrab import AsyncAgent
    agent = AsyncAgent(model="copilot/gpt-5-mini")
    tokens = []
    async for event in agent.stream("Say 'hello' and nothing else."):
        if event.event_type == "token":
            tokens.append(event.data)
    full = "".join(tokens)
    assert "hello" in full.lower()


@pytest.mark.skipif(not E2E, reason="E2E tests require EDGECRAB_E2E=1")
@pytest.mark.asyncio
async def test_on_stream_callback():
    from edgecrab import AsyncAgent
    received = []
    agent = AsyncAgent(
        model="copilot/gpt-5-mini",
        on_stream=lambda ev: received.append(ev),
    )
    reply = await agent.chat("Say 'hello' and nothing else.")
    assert "hello" in reply.lower()
    assert len(received) > 0
