"""Async wrapper for the EdgeCrab Agent.

Provides ``async def`` versions of all Agent methods using
``asyncio.to_thread()`` to keep the event loop non-blocking while
the native Rust runtime does the heavy lifting.

Usage:
    from edgecrab import AsyncAgent

    agent = AsyncAgent("anthropic/claude-sonnet-4")
    reply = await agent.chat("Hello!")

    async for event in agent.stream("Explain X"):
        if event.event_type == "token":
            print(event.data, end="")
"""

from __future__ import annotations

import asyncio
from typing import Any, AsyncIterator, Callable, Optional

from edgecrab._native import (
    Agent as _NativeAgent,
    ConversationResult,
    MemoryManager as _NativeMemoryManager,
    SessionSearchHit,
    SessionSummary,
    StreamEvent,
)

__all__ = ["AsyncAgent", "AsyncMemoryManager"]


class AsyncMemoryManager:
    """Async wrapper around the native MemoryManager."""

    __slots__ = ("_inner",)

    def __init__(self, inner: _NativeMemoryManager) -> None:
        self._inner = inner

    async def read(self, key: str = "memory") -> str:
        """Read the contents of a memory file."""
        return await asyncio.to_thread(self._inner.read, key)

    async def write(self, key: str, value: str) -> None:
        """Write (append) a new entry to a memory file."""
        await asyncio.to_thread(self._inner.write, key, value)

    async def remove(self, key: str, old_content: str) -> bool:
        """Remove an entry by substring match."""
        return await asyncio.to_thread(self._inner.remove, key, old_content)

    async def entries(self, key: str = "memory") -> list[str]:
        """List all entries from a memory file."""
        return await asyncio.to_thread(self._inner.entries, key)


class AsyncAgent:
    """Async wrapper around the native EdgeCrab Agent.

    All blocking Rust FFI calls are dispatched to a thread via
    ``asyncio.to_thread()`` so that the Python event loop stays
    responsive.

    Constructor arguments are identical to :class:`Agent`.
    """

    __slots__ = ("_agent", "_memory", "_on_stream", "_on_tool_call", "_on_tool_result")

    def __init__(
        self,
        model: str = "anthropic/claude-sonnet-4",
        *,
        max_iterations: Optional[int] = None,
        temperature: Optional[float] = None,
        streaming: Optional[bool] = None,
        session_id: Optional[str] = None,
        quiet_mode: Optional[bool] = None,
        instructions: Optional[str] = None,
        toolsets: Optional[list[str]] = None,
        disabled_toolsets: Optional[list[str]] = None,
        disabled_tools: Optional[list[str]] = None,
        skip_context_files: Optional[bool] = None,
        skip_memory: Optional[bool] = None,
        config=None,
        on_stream: Optional[Callable[[StreamEvent], None]] = None,
        on_tool_call: Optional[Callable[..., None]] = None,
        on_tool_result: Optional[Callable[..., None]] = None,
    ) -> None:
        if config is not None:
            self._agent = _NativeAgent.from_config(config)
        else:
            self._agent = _NativeAgent(
                model,
                max_iterations=max_iterations,
                temperature=temperature,
                streaming=streaming,
                session_id=session_id,
                quiet_mode=quiet_mode,
                instructions=instructions,
                toolsets=toolsets,
                disabled_toolsets=disabled_toolsets,
                disabled_tools=disabled_tools,
                skip_context_files=skip_context_files,
                skip_memory=skip_memory,
            )
        self._memory: Optional[AsyncMemoryManager] = None
        self._on_stream = on_stream
        self._on_tool_call = on_tool_call
        self._on_tool_result = on_tool_result

    # ── Simple API ────────────────────────────────────────────────────

    async def chat(self, message: str) -> str:
        """Send a message and get the response (async).

        If ``on_stream`` was provided at construction, streaming events are
        dispatched to the callback as they arrive and the final response text
        is returned.
        """
        if self._on_stream is not None:
            # Use streaming mode and fire callback per-event
            events = await asyncio.to_thread(self._agent.stream_sync, message)
            final_text = ""
            for event in events:
                self._on_stream(event)
                if event.event_type == "token":
                    final_text += event.data
            return final_text
        return await asyncio.to_thread(self._agent.chat_sync, message)

    def chat_sync(self, message: str) -> str:
        """Send a message and get the response (sync/blocking)."""
        return self._agent.chat_sync(message)

    # ── Streaming API ─────────────────────────────────────────────────

    async def stream(self, message: str) -> AsyncIterator[StreamEvent]:
        """Stream events from the agent as an async iterator.

        If ``on_stream``, ``on_tool_call``, or ``on_tool_result`` callbacks
        were provided at construction, they are also invoked for each
        matching event.
        """
        events = await asyncio.to_thread(self._agent.stream_sync, message)
        for event in events:
            if self._on_stream is not None:
                self._on_stream(event)
            if self._on_tool_call is not None and event.event_type == "tool_exec":
                self._on_tool_call(event)
            if self._on_tool_result is not None and event.event_type == "tool_result":
                self._on_tool_result(event)
            yield event

    # ── Full API ──────────────────────────────────────────────────────

    async def run(self, message: str) -> ConversationResult:
        """Run a full conversation and get detailed results."""
        return await asyncio.to_thread(self._agent.run_sync, message)

    async def run_conversation(
        self,
        message: str,
        *,
        system: Optional[str] = None,
        history: Optional[list[str]] = None,
    ) -> ConversationResult:
        """Run a conversation with optional system prompt and history."""
        return await asyncio.to_thread(
            self._agent.run_conversation,
            message,
            system=system,
            history=history,
        )

    # ── Session Management ────────────────────────────────────────────

    async def fork(self) -> "AsyncAgent":
        """Fork the agent into an isolated copy."""
        forked_native = await asyncio.to_thread(self._agent.fork)
        wrapper = AsyncAgent.__new__(AsyncAgent)
        wrapper._agent = forked_native
        wrapper._memory = None
        wrapper._on_stream = self._on_stream
        wrapper._on_tool_call = self._on_tool_call
        wrapper._on_tool_result = self._on_tool_result
        return wrapper

    def interrupt(self) -> None:
        """Interrupt the current agent run."""
        self._agent.interrupt()

    @property
    def is_cancelled(self) -> bool:
        """Check if the agent has been cancelled."""
        return self._agent.is_cancelled()

    async def new_session(self) -> None:
        """Start a new session (reset conversation)."""
        await asyncio.to_thread(self._agent.new_session)

    @property
    def session_id(self) -> Optional[str]:
        """Get the current session ID."""
        return self._agent.session_id

    @property
    def history(self) -> list[dict[str, Any]]:
        """Get the conversation history."""
        return self._agent.history

    @property
    def model(self) -> str:
        """Get the current model name."""
        return self._agent.model

    async def export(self) -> dict[str, Any]:
        """Export the current session."""
        return await asyncio.to_thread(self._agent.export)

    # ── Memory ────────────────────────────────────────────────────────

    @property
    def memory(self) -> AsyncMemoryManager:
        """Get an async MemoryManager for reading/writing agent memory."""
        if self._memory is None:
            self._memory = AsyncMemoryManager(self._agent.memory)
        return self._memory

    # ── Configuration ─────────────────────────────────────────────────

    async def tool_names(self) -> list[str]:
        """List available tool names."""
        return await asyncio.to_thread(self._agent.tool_names)

    async def toolset_summary(self) -> list[tuple[str, int]]:
        """Get a summary of toolsets and their tool counts."""
        return await asyncio.to_thread(self._agent.toolset_summary)

    def list_sessions(self, limit: int = 20) -> list[SessionSummary]:
        """List recent sessions."""
        return self._agent.list_sessions(limit)

    def search_sessions(self, query: str, limit: int = 10) -> list[SessionSearchHit]:
        """Search sessions by text."""
        return self._agent.search_sessions(query, limit)

    async def set_reasoning_effort(self, effort: Optional[str] = None) -> None:
        """Set the reasoning effort level."""
        await asyncio.to_thread(self._agent.set_reasoning_effort, effort)

    async def set_streaming(self, enabled: bool) -> None:
        """Enable or disable streaming mode."""
        await asyncio.to_thread(self._agent.set_streaming, enabled)

    async def chat_in_cwd(self, message: str, cwd: str) -> str:
        """Send a message with a specific working directory (async)."""
        return await asyncio.to_thread(self._agent.chat_in_cwd, message, cwd)

    async def session_snapshot(self) -> dict[str, Any]:
        """Get a snapshot of the current session state."""
        return await asyncio.to_thread(self._agent.session_snapshot)

    async def compress(self) -> None:
        """Force context compression — summarise conversation history."""
        await asyncio.to_thread(self._agent.compress)

    async def set_model(self, model: str) -> None:
        """Hot-swap the model at runtime. Takes 'provider/model' string."""
        await asyncio.to_thread(self._agent.set_model, model)

    async def batch(self, messages: list[str]) -> list[str | None]:
        """Run multiple prompts in parallel, returning results in order."""
        return await asyncio.to_thread(self._agent.batch, messages)

    def __repr__(self) -> str:
        return f"AsyncAgent(model='{self.model}')"
