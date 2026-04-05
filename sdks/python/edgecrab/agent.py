"""High-level Agent abstraction for the EdgeCrab SDK.

Inspired by hermes-agent's AIAgent — provides a batteries-included Agent
object that manages conversation state, streaming, tool metadata, and
session lifecycle over the EdgeCrab OpenAI-compatible API.
"""

from __future__ import annotations

import copy
import os
import threading
import uuid
from dataclasses import dataclass, field
from typing import Any, Callable, Generator, Optional

from edgecrab.client import (
    AsyncEdgeCrabClient,
    EdgeCrabClient,
    EdgeCrabError,
    InterruptedError,
    MaxTurnsExceededError,
)
from edgecrab.types import (
    ChatChoice,
    ChatCompletionResponse,
    ChatMessage,
    ModelInfo,
    StreamChunk,
    UsageInfo,
)


@dataclass
class AgentResult:
    """Result of a conversation run — mirrors hermes-agent's AgentResult."""

    response: str
    messages: list[ChatMessage] = field(default_factory=list)
    session_id: str = ""
    model: str = ""
    turns_used: int = 0
    finished_naturally: bool = True
    interrupted: bool = False
    max_turns_exceeded: bool = False
    usage: Optional[UsageInfo] = None


class Agent:
    """High-level agent interface for EdgeCrab.

    Manages conversation history, session state, and provides both simple
    and advanced APIs for interacting with the EdgeCrab agent.

    Parameters
    ----------
    model:
        Model identifier (default: ``anthropic/claude-sonnet-4-20250514``).
    base_url:
        EdgeCrab API server URL (env: ``EDGECRAB_BASE_URL``).
    api_key:
        Bearer token (env: ``EDGECRAB_API_KEY``).
    system_prompt:
        System prompt prepended to every conversation.
    max_turns:
        Maximum conversation turns before stopping. Default: 50.
    temperature:
        Sampling temperature.
    max_tokens:
        Maximum tokens per response.
    timeout:
        HTTP timeout in seconds. Default: 120.
    session_id:
        Explicit session ID; auto-generated if not provided.
    streaming:
        Whether to use streaming by default. Default: False.
    max_retries:
        Maximum retries on transient errors (passed to client). Default: 3.
    on_token:
        Callback ``(str) -> None`` fired for each streaming token.
    on_tool_call:
        Callback ``(str, dict) -> None`` fired when a tool call is detected.
    on_turn:
        Callback ``(int, ChatMessage) -> None`` fired after each turn.
    on_error:
        Callback ``(Exception) -> None`` fired on recoverable errors.
    """

    def __init__(
        self,
        model: str = "anthropic/claude-sonnet-4-20250514",
        *,
        base_url: str | None = None,
        api_key: str | None = None,
        system_prompt: str | None = None,
        max_turns: int = 50,
        temperature: float | None = None,
        max_tokens: int | None = None,
        timeout: float = 120.0,
        session_id: str | None = None,
        streaming: bool = False,
        max_retries: int = 3,
        on_token: Callable[[str], None] | None = None,
        on_tool_call: Callable[[str, dict], None] | None = None,
        on_turn: Callable[[int, ChatMessage], None] | None = None,
        on_error: Callable[[Exception], None] | None = None,
    ) -> None:
        self.model = model
        self.system_prompt = system_prompt
        self.max_turns = max_turns
        self.temperature = temperature
        self.max_tokens = max_tokens
        self.streaming = streaming
        self.session_id = session_id or str(uuid.uuid4())

        # Callbacks
        self.on_token = on_token
        self.on_tool_call = on_tool_call
        self.on_turn = on_turn
        self.on_error = on_error

        # Conversation state
        self._messages: list[ChatMessage] = []
        self._turn_count: int = 0
        self._total_usage = UsageInfo()

        # Interrupt flag (thread-safe)
        self._interrupted = threading.Event()

        resolved_url = base_url or os.environ.get("EDGECRAB_BASE_URL", "http://127.0.0.1:8642")
        resolved_key = api_key or os.environ.get("EDGECRAB_API_KEY")

        self._client = EdgeCrabClient(
            base_url=resolved_url,
            api_key=resolved_key,
            timeout=timeout,
            max_retries=max_retries,
        )

        if self.system_prompt:
            self._messages.append(ChatMessage(role="system", content=self.system_prompt))

    # ── Lifecycle ───────────────────────────────────────────────────

    def close(self) -> None:
        """Close the underlying HTTP client."""
        self._client.close()

    def __enter__(self) -> "Agent":
        return self

    def __exit__(self, *args: Any) -> None:
        self.close()

    # ── Interrupt ───────────────────────────────────────────────────

    def interrupt(self) -> None:
        """Signal the agent to stop after the current turn.

        Thread-safe — can be called from another thread or a signal handler.
        """
        self._interrupted.set()

    def clear_interrupt(self) -> None:
        """Clear the interrupt flag so the agent can continue."""
        self._interrupted.clear()

    @property
    def is_interrupted(self) -> bool:
        """Whether the agent has been interrupted."""
        return self._interrupted.is_set()

    # ── Simple API ──────────────────────────────────────────────────

    def chat(self, message: str) -> str:
        """Send a message and return the assistant's text reply.

        Maintains conversation history across calls.
        Raises ``MaxTurnsExceededError`` if ``max_turns`` is reached.
        Raises ``InterruptedError`` if ``interrupt()`` was called.
        """
        if self._interrupted.is_set():
            raise InterruptedError()

        if self._turn_count >= self.max_turns:
            raise MaxTurnsExceededError(self.max_turns)

        self._messages.append(ChatMessage(role="user", content=message))
        self._turn_count += 1

        if self.streaming and self.on_token:
            return self._chat_streaming()

        resp = self._client.create_completion(
            messages=self._messages,
            model=self.model,
            temperature=self.temperature,
            max_tokens=self.max_tokens,
        )
        assistant_msg = self._extract_response(resp)
        self._accumulate_usage(resp.usage)

        if self.on_turn:
            self.on_turn(self._turn_count, assistant_msg)

        return assistant_msg.content

    def _chat_streaming(self) -> str:
        """Internal streaming chat — fires on_token callback for each delta."""
        collected: list[str] = []
        for chunk in self._client.stream_completion(
            messages=self._messages,
            model=self.model,
            temperature=self.temperature,
            max_tokens=self.max_tokens,
        ):
            if self._interrupted.is_set():
                break
            for choice in chunk.choices:
                if choice.delta.content:
                    collected.append(choice.delta.content)
                    if self.on_token:
                        self.on_token(choice.delta.content)

        full_text = "".join(collected)
        assistant_msg = ChatMessage(role="assistant", content=full_text)
        self._messages.append(assistant_msg)

        if self.on_turn:
            self.on_turn(self._turn_count, assistant_msg)

        return full_text

    # ── Full conversation run ───────────────────────────────────────

    def run(
        self,
        message: str,
        *,
        conversation_history: list[ChatMessage] | None = None,
    ) -> AgentResult:
        """Run a full agent conversation — the agent will run tool loops
        autonomously up to ``max_turns``.

        Parameters
        ----------
        message:
            The user message to send.
        conversation_history:
            Optional prior conversation history to inject before running.
            This is analogous to hermes-agent's ``conversation_history`` param.

        Returns a structured ``AgentResult``.
        """
        if conversation_history:
            for msg in conversation_history:
                self._messages.append(msg)

        interrupted = False
        max_turns_exceeded = False
        try:
            reply = self.chat(message)
        except InterruptedError:
            reply = self._messages[-1].content if self._messages else ""
            interrupted = True
        except MaxTurnsExceededError:
            reply = self._messages[-1].content if self._messages else ""
            max_turns_exceeded = True

        return AgentResult(
            response=reply,
            messages=list(self._messages),
            session_id=self.session_id,
            model=self.model,
            turns_used=self._turn_count,
            finished_naturally=not interrupted and not max_turns_exceeded,
            interrupted=interrupted,
            max_turns_exceeded=max_turns_exceeded,
            usage=self._total_usage,
        )

    # ── Stream API ──────────────────────────────────────────────────

    def stream(self, message: str) -> Generator[str, None, None]:
        """Send a message and yield response tokens as they arrive.

        Maintains conversation history. Unlike ``chat()``, this returns a
        generator instead of a complete string.
        """
        if self._interrupted.is_set():
            raise InterruptedError()
        if self._turn_count >= self.max_turns:
            raise MaxTurnsExceededError(self.max_turns)

        self._messages.append(ChatMessage(role="user", content=message))
        self._turn_count += 1
        collected: list[str] = []

        for chunk in self._client.stream_completion(
            messages=self._messages,
            model=self.model,
            temperature=self.temperature,
            max_tokens=self.max_tokens,
        ):
            if self._interrupted.is_set():
                break
            for choice in chunk.choices:
                if choice.delta.content:
                    collected.append(choice.delta.content)
                    yield choice.delta.content

        full_text = "".join(collected)
        self._messages.append(ChatMessage(role="assistant", content=full_text))

    # ── Multi-turn conversation ─────────────────────────────────────

    def add_message(self, role: str, content: str) -> None:
        """Manually inject a message into the conversation history."""
        self._messages.append(ChatMessage(role=role, content=content))  # type: ignore[arg-type]

    def reset(self) -> None:
        """Reset conversation state for a new session."""
        self._messages.clear()
        self._turn_count = 0
        self._total_usage = UsageInfo()
        self.session_id = str(uuid.uuid4())
        self._interrupted.clear()
        if self.system_prompt:
            self._messages.append(ChatMessage(role="system", content=self.system_prompt))

    # ── Conversation persistence ────────────────────────────────────

    def export_conversation(self) -> dict[str, Any]:
        """Export the current conversation state as a serializable dict.

        Can be saved to disk and later restored with ``import_conversation()``.
        """
        return {
            "session_id": self.session_id,
            "model": self.model,
            "messages": [m.model_dump() for m in self._messages],
            "turn_count": self._turn_count,
            "usage": self._total_usage.model_dump(),
        }

    def import_conversation(self, data: dict[str, Any]) -> None:
        """Restore a conversation state from a previously exported dict."""
        self.session_id = data.get("session_id", self.session_id)
        self._messages = [ChatMessage.model_validate(m) for m in data.get("messages", [])]
        self._turn_count = data.get("turn_count", 0)
        if data.get("usage"):
            self._total_usage = UsageInfo.model_validate(data["usage"])

    def clone(self) -> "Agent":
        """Create a fork of this agent with an independent copy of the conversation.

        Useful for exploring alternate conversation branches without
        affecting the original agent's state.
        """
        new_agent = Agent(
            model=self.model,
            system_prompt=self.system_prompt,
            max_turns=self.max_turns,
            temperature=self.temperature,
            max_tokens=self.max_tokens,
            streaming=self.streaming,
            on_token=self.on_token,
            on_tool_call=self.on_tool_call,
            on_turn=self.on_turn,
            on_error=self.on_error,
        )
        # Deep-copy mutable state
        new_agent._messages = [m.model_copy() for m in self._messages]
        new_agent._turn_count = self._turn_count
        new_agent._total_usage = self._total_usage.model_copy()
        new_agent.session_id = str(uuid.uuid4())
        return new_agent

    # ── Introspection ───────────────────────────────────────────────

    @property
    def messages(self) -> list[ChatMessage]:
        """Current conversation history (read-only copy)."""
        return list(self._messages)

    @property
    def turn_count(self) -> int:
        """Number of user turns completed."""
        return self._turn_count

    @property
    def usage(self) -> UsageInfo:
        """Accumulated token usage."""
        return self._total_usage

    def list_models(self) -> list[ModelInfo]:
        """List available models from the server."""
        return self._client.list_models()

    def health(self) -> dict:
        """Check server health."""
        return self._client.health().model_dump()

    # ── Internal ────────────────────────────────────────────────────

    def _extract_response(self, resp: ChatCompletionResponse) -> ChatMessage:
        """Extract the assistant message from a completion response and append it."""
        if not resp.choices:
            msg = ChatMessage(role="assistant", content="")
        else:
            msg = resp.choices[0].message
        self._messages.append(msg)
        return msg

    def _accumulate_usage(self, usage: UsageInfo | None) -> None:
        if usage:
            self._total_usage.prompt_tokens += usage.prompt_tokens
            self._total_usage.completion_tokens += usage.completion_tokens
            self._total_usage.total_tokens += usage.total_tokens


class AsyncAgent:
    """Asynchronous high-level agent interface for EdgeCrab.

    Same API as :class:`Agent` but all IO methods are ``async``.
    Includes interrupt support, conversation persistence, and max_turns enforcement.
    """

    def __init__(
        self,
        model: str = "anthropic/claude-sonnet-4-20250514",
        *,
        base_url: str | None = None,
        api_key: str | None = None,
        system_prompt: str | None = None,
        max_turns: int = 50,
        temperature: float | None = None,
        max_tokens: int | None = None,
        timeout: float = 120.0,
        session_id: str | None = None,
        streaming: bool = False,
        max_retries: int = 3,
        on_token: Callable[[str], None] | None = None,
        on_tool_call: Callable[[str, dict], None] | None = None,
        on_turn: Callable[[int, ChatMessage], None] | None = None,
        on_error: Callable[[Exception], None] | None = None,
    ) -> None:
        self.model = model
        self.system_prompt = system_prompt
        self.max_turns = max_turns
        self.temperature = temperature
        self.max_tokens = max_tokens
        self.streaming = streaming
        self.session_id = session_id or str(uuid.uuid4())

        self.on_token = on_token
        self.on_tool_call = on_tool_call
        self.on_turn = on_turn
        self.on_error = on_error

        self._messages: list[ChatMessage] = []
        self._turn_count: int = 0
        self._total_usage = UsageInfo()
        self._interrupted = threading.Event()

        resolved_url = base_url or os.environ.get("EDGECRAB_BASE_URL", "http://127.0.0.1:8642")
        resolved_key = api_key or os.environ.get("EDGECRAB_API_KEY")

        self._client = AsyncEdgeCrabClient(
            base_url=resolved_url,
            api_key=resolved_key,
            timeout=timeout,
            max_retries=max_retries,
        )

        if self.system_prompt:
            self._messages.append(ChatMessage(role="system", content=self.system_prompt))

    async def close(self) -> None:
        await self._client.close()

    async def __aenter__(self) -> "AsyncAgent":
        return self

    async def __aexit__(self, *args: Any) -> None:
        await self.close()

    # ── Interrupt ───────────────────────────────────────────────────

    def interrupt(self) -> None:
        """Signal the agent to stop after the current turn. Thread-safe."""
        self._interrupted.set()

    def clear_interrupt(self) -> None:
        """Clear the interrupt flag."""
        self._interrupted.clear()

    @property
    def is_interrupted(self) -> bool:
        return self._interrupted.is_set()

    # ── Chat ────────────────────────────────────────────────────────

    async def chat(self, message: str) -> str:
        """Send a message and return the assistant's text reply."""
        if self._interrupted.is_set():
            raise InterruptedError()
        if self._turn_count >= self.max_turns:
            raise MaxTurnsExceededError(self.max_turns)

        self._messages.append(ChatMessage(role="user", content=message))
        self._turn_count += 1

        if self.streaming and self.on_token:
            return await self._chat_streaming()

        resp = await self._client.create_completion(
            messages=self._messages,
            model=self.model,
            temperature=self.temperature,
            max_tokens=self.max_tokens,
        )
        assistant_msg = self._extract_response(resp)
        self._accumulate_usage(resp.usage)

        if self.on_turn:
            self.on_turn(self._turn_count, assistant_msg)

        return assistant_msg.content

    async def _chat_streaming(self) -> str:
        collected: list[str] = []
        async for chunk in self._client.stream_completion(
            messages=self._messages,
            model=self.model,
            temperature=self.temperature,
            max_tokens=self.max_tokens,
        ):
            if self._interrupted.is_set():
                break
            for choice in chunk.choices:
                if choice.delta.content:
                    collected.append(choice.delta.content)
                    if self.on_token:
                        self.on_token(choice.delta.content)

        full_text = "".join(collected)
        self._messages.append(ChatMessage(role="assistant", content=full_text))

        if self.on_turn:
            self.on_turn(self._turn_count, ChatMessage(role="assistant", content=full_text))

        return full_text

    async def run(
        self,
        message: str,
        *,
        conversation_history: list[ChatMessage] | None = None,
    ) -> AgentResult:
        """Run a full agent conversation."""
        if conversation_history:
            for msg in conversation_history:
                self._messages.append(msg)

        interrupted = False
        max_turns_exceeded = False
        try:
            reply = await self.chat(message)
        except InterruptedError:
            reply = self._messages[-1].content if self._messages else ""
            interrupted = True
        except MaxTurnsExceededError:
            reply = self._messages[-1].content if self._messages else ""
            max_turns_exceeded = True

        return AgentResult(
            response=reply,
            messages=list(self._messages),
            session_id=self.session_id,
            model=self.model,
            turns_used=self._turn_count,
            finished_naturally=not interrupted and not max_turns_exceeded,
            interrupted=interrupted,
            max_turns_exceeded=max_turns_exceeded,
            usage=self._total_usage,
        )

    async def stream(self, message: str):
        """Send a message and yield response tokens as they arrive."""
        if self._interrupted.is_set():
            raise InterruptedError()
        if self._turn_count >= self.max_turns:
            raise MaxTurnsExceededError(self.max_turns)

        self._messages.append(ChatMessage(role="user", content=message))
        self._turn_count += 1
        collected: list[str] = []

        async for chunk in self._client.stream_completion(
            messages=self._messages,
            model=self.model,
            temperature=self.temperature,
            max_tokens=self.max_tokens,
        ):
            if self._interrupted.is_set():
                break
            for choice in chunk.choices:
                if choice.delta.content:
                    collected.append(choice.delta.content)
                    yield choice.delta.content

        full_text = "".join(collected)
        self._messages.append(ChatMessage(role="assistant", content=full_text))

    def add_message(self, role: str, content: str) -> None:
        self._messages.append(ChatMessage(role=role, content=content))  # type: ignore[arg-type]

    def reset(self) -> None:
        self._messages.clear()
        self._turn_count = 0
        self._total_usage = UsageInfo()
        self.session_id = str(uuid.uuid4())
        self._interrupted.clear()
        if self.system_prompt:
            self._messages.append(ChatMessage(role="system", content=self.system_prompt))

    # ── Conversation persistence ────────────────────────────────────

    def export_conversation(self) -> dict[str, Any]:
        """Export the current conversation state as a serializable dict."""
        return {
            "session_id": self.session_id,
            "model": self.model,
            "messages": [m.model_dump() for m in self._messages],
            "turn_count": self._turn_count,
            "usage": self._total_usage.model_dump(),
        }

    def import_conversation(self, data: dict[str, Any]) -> None:
        """Restore a conversation state from a previously exported dict."""
        self.session_id = data.get("session_id", self.session_id)
        self._messages = [ChatMessage.model_validate(m) for m in data.get("messages", [])]
        self._turn_count = data.get("turn_count", 0)
        if data.get("usage"):
            self._total_usage = UsageInfo.model_validate(data["usage"])

    def clone(self) -> "AsyncAgent":
        """Create a fork with an independent copy of the conversation."""
        new_agent = AsyncAgent(
            model=self.model,
            system_prompt=self.system_prompt,
            max_turns=self.max_turns,
            temperature=self.temperature,
            max_tokens=self.max_tokens,
            streaming=self.streaming,
            on_token=self.on_token,
            on_tool_call=self.on_tool_call,
            on_turn=self.on_turn,
            on_error=self.on_error,
        )
        new_agent._messages = [m.model_copy() for m in self._messages]
        new_agent._turn_count = self._turn_count
        new_agent._total_usage = self._total_usage.model_copy()
        new_agent.session_id = str(uuid.uuid4())
        return new_agent

    @property
    def messages(self) -> list[ChatMessage]:
        return list(self._messages)

    @property
    def turn_count(self) -> int:
        return self._turn_count

    @property
    def usage(self) -> UsageInfo:
        return self._total_usage

    async def list_models(self) -> list[ModelInfo]:
        return await self._client.list_models()

    async def health(self) -> dict:
        h = await self._client.health()
        return h.model_dump()

    def _extract_response(self, resp: ChatCompletionResponse) -> ChatMessage:
        if not resp.choices:
            msg = ChatMessage(role="assistant", content="")
        else:
            msg = resp.choices[0].message
        self._messages.append(msg)
        return msg

    def _accumulate_usage(self, usage: UsageInfo | None) -> None:
        if usage:
            self._total_usage.prompt_tokens += usage.prompt_tokens
            self._total_usage.completion_tokens += usage.completion_tokens
            self._total_usage.total_tokens += usage.total_tokens
