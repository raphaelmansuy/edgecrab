"""Tests for enhanced SDK features: error hierarchy, interrupt, export/import, clone, max_turns."""

from __future__ import annotations

import json
from unittest.mock import patch, MagicMock

import httpx
import pytest

from edgecrab.agent import Agent, AsyncAgent, AgentResult
from edgecrab.client import (
    EdgeCrabError,
    AuthenticationError,
    RateLimitError,
    ServerError,
    TimeoutError,
    ConnectionError,
    MaxTurnsExceededError,
    InterruptedError,
    _classify_error,
    _is_retryable,
)
from edgecrab.types import ChatMessage, UsageInfo

from conftest import MOCK_COMPLETION_RESPONSE


# ── Error Hierarchy ────────────────────────────────────────────────


class TestErrorHierarchy:
    """Test structured error classification."""

    def test_401_raises_authentication_error(self):
        err = _classify_error(401, "Unauthorized")
        assert isinstance(err, AuthenticationError)
        assert isinstance(err, EdgeCrabError)
        assert err.status_code == 401

    def test_403_raises_authentication_error(self):
        err = _classify_error(403, "Forbidden")
        assert isinstance(err, AuthenticationError)
        assert err.status_code == 403

    def test_429_raises_rate_limit_error(self):
        err = _classify_error(429, "Too Many Requests")
        assert isinstance(err, RateLimitError)
        assert err.status_code == 429

    def test_429_with_retry_after(self):
        err = _classify_error(429, "Too Many Requests", {"retry-after": "30"})
        assert isinstance(err, RateLimitError)
        assert err.retry_after == 30.0

    def test_500_raises_server_error(self):
        err = _classify_error(500, "Internal Server Error")
        assert isinstance(err, ServerError)
        assert err.status_code == 500

    def test_502_raises_server_error(self):
        err = _classify_error(502, "Bad Gateway")
        assert isinstance(err, ServerError)

    def test_400_raises_base_error(self):
        err = _classify_error(400, "Bad Request")
        assert isinstance(err, EdgeCrabError)
        assert not isinstance(err, AuthenticationError)
        assert not isinstance(err, ServerError)

    def test_all_errors_inherit_from_base(self):
        """All error types must inherit from EdgeCrabError."""
        assert issubclass(AuthenticationError, EdgeCrabError)
        assert issubclass(RateLimitError, EdgeCrabError)
        assert issubclass(ServerError, EdgeCrabError)
        assert issubclass(TimeoutError, EdgeCrabError)
        assert issubclass(ConnectionError, EdgeCrabError)
        assert issubclass(MaxTurnsExceededError, EdgeCrabError)
        assert issubclass(InterruptedError, EdgeCrabError)


class TestRetryableClassification:
    """Test _is_retryable helper."""

    def test_server_error_is_retryable(self):
        assert _is_retryable(ServerError("err", 500))

    def test_timeout_is_retryable(self):
        assert _is_retryable(TimeoutError())

    def test_connection_error_is_retryable(self):
        assert _is_retryable(ConnectionError())

    def test_rate_limit_is_retryable(self):
        assert _is_retryable(RateLimitError("err"))

    def test_auth_error_is_not_retryable(self):
        assert not _is_retryable(AuthenticationError("err", 401))

    def test_generic_error_is_not_retryable(self):
        assert not _is_retryable(EdgeCrabError("err", 400))


# ── Interrupt ──────────────────────────────────────────────────────


class TestAgentInterrupt:
    """Test interrupt mechanism."""

    def test_interrupt_flag(self):
        agent = Agent()
        assert not agent.is_interrupted
        agent.interrupt()
        assert agent.is_interrupted
        agent.clear_interrupt()
        assert not agent.is_interrupted
        agent.close()

    def test_chat_raises_when_interrupted(self):
        with Agent() as agent:
            agent.interrupt()
            with pytest.raises(InterruptedError):
                agent.chat("Hello")

    def test_stream_raises_when_interrupted(self):
        with Agent() as agent:
            agent.interrupt()
            with pytest.raises(InterruptedError):
                list(agent.stream("Hello"))

    def test_run_captures_interrupt(self):
        """run() should catch InterruptedError and return result."""
        with Agent() as agent:
            agent.interrupt()
            result = agent.run("Hello")
            assert isinstance(result, AgentResult)
            assert result.interrupted is True
            assert result.finished_naturally is False

    def test_reset_clears_interrupt(self):
        with Agent() as agent:
            agent.interrupt()
            agent.reset()
            assert not agent.is_interrupted


# ── Max Turns ──────────────────────────────────────────────────────


class TestMaxTurns:
    """Test max_turns enforcement."""

    def test_chat_raises_when_max_turns_exceeded(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_COMPLETION_RESPONSE,
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        with patch.object(httpx.Client, "post", return_value=mock_response):
            with Agent(max_turns=1) as agent:
                agent.chat("First")  # turn 1 — OK
                with pytest.raises(MaxTurnsExceededError):
                    agent.chat("Second")  # turn 2 — exceeds max

    def test_run_captures_max_turns_exceeded(self):
        with Agent(max_turns=0) as agent:
            result = agent.run("Hello")
            assert result.max_turns_exceeded is True
            assert result.finished_naturally is False


# ── Conversation Export / Import ───────────────────────────────────


class TestConversationPersistence:
    """Test export/import conversation."""

    def test_export_empty(self):
        with Agent() as agent:
            exported = agent.export_conversation()
            assert exported["session_id"] == agent.session_id
            assert exported["messages"] == []
            assert exported["turn_count"] == 0

    def test_export_with_history(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_COMPLETION_RESPONSE,
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        with patch.object(httpx.Client, "post", return_value=mock_response):
            with Agent(system_prompt="Be helpful") as agent:
                agent.chat("Hello")
                exported = agent.export_conversation()
                assert len(exported["messages"]) == 3  # system + user + assistant
                assert exported["turn_count"] == 1
                assert exported["usage"]["total_tokens"] == 18

    def test_import_restores_state(self):
        data = {
            "session_id": "restored-session",
            "model": "test-model",
            "messages": [
                {"role": "user", "content": "Hi"},
                {"role": "assistant", "content": "Hello!"},
            ],
            "turn_count": 1,
            "usage": {"prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8},
        }
        with Agent() as agent:
            agent.import_conversation(data)
            assert agent.session_id == "restored-session"
            assert agent.turn_count == 1
            assert len(agent.messages) == 2
            assert agent.usage.total_tokens == 8

    def test_roundtrip(self):
        """Export → import should preserve state."""
        mock_response = httpx.Response(
            200,
            json=MOCK_COMPLETION_RESPONSE,
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        with patch.object(httpx.Client, "post", return_value=mock_response):
            with Agent() as agent1:
                agent1.chat("Hello")
                exported = agent1.export_conversation()

            with Agent() as agent2:
                agent2.import_conversation(exported)
                assert agent2.turn_count == agent1.turn_count
                assert len(agent2.messages) == len(agent1.messages)
                assert agent2.usage.total_tokens == agent1.usage.total_tokens


# ── Clone ──────────────────────────────────────────────────────────


class TestAgentClone:
    """Test agent cloning / conversation forking."""

    def test_clone_creates_independent_copy(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_COMPLETION_RESPONSE,
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        with patch.object(httpx.Client, "post", return_value=mock_response):
            with Agent(system_prompt="Be brief") as agent:
                agent.chat("Hello")
                clone = agent.clone()

                # Same content
                assert clone.turn_count == agent.turn_count
                assert len(clone.messages) == len(agent.messages)
                assert clone.model == agent.model

                # Different session
                assert clone.session_id != agent.session_id

                # Independent — modifying clone doesn't affect original
                clone.add_message("user", "extra")
                assert len(clone.messages) == len(agent.messages) + 1

                clone.close()

    def test_clone_preserves_system_prompt(self):
        with Agent(system_prompt="Be helpful") as agent:
            clone = agent.clone()
            assert clone.messages[0].content == "Be helpful"
            clone.close()


# ── Conversation History Injection ─────────────────────────────────


class TestConversationHistoryInjection:
    """Test injecting conversation history via run()."""

    def test_run_with_conversation_history(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_COMPLETION_RESPONSE,
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        prior_history = [
            ChatMessage(role="user", content="Context message 1"),
            ChatMessage(role="assistant", content="Got it"),
        ]
        with patch.object(httpx.Client, "post", return_value=mock_response):
            with Agent() as agent:
                result = agent.run("Follow up", conversation_history=prior_history)
                # Should have: 2 prior + 1 user + 1 assistant = 4
                assert len(result.messages) == 4
                assert result.messages[0].content == "Context message 1"


# ── AgentResult New Fields ─────────────────────────────────────────


class TestAgentResultFields:
    """Test new AgentResult fields."""

    def test_result_has_interrupted_field(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_COMPLETION_RESPONSE,
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        with patch.object(httpx.Client, "post", return_value=mock_response):
            with Agent() as agent:
                result = agent.run("Hello")
                assert result.interrupted is False
                assert result.max_turns_exceeded is False
                assert result.finished_naturally is True


# ── Error Exports ──────────────────────────────────────────────────


class TestErrorExports:
    """Test that all error types are properly exported from the package."""

    def test_errors_in_all(self):
        import edgecrab
        for name in [
            "EdgeCrabError",
            "AuthenticationError",
            "RateLimitError",
            "ServerError",
            "TimeoutError",
            "ConnectionError",
            "MaxTurnsExceededError",
            "InterruptedError",
        ]:
            assert name in edgecrab.__all__, f"{name} not in __all__"

    def test_errors_importable(self):
        from edgecrab import (
            EdgeCrabError,
            AuthenticationError,
            RateLimitError,
            ServerError,
            TimeoutError,
            ConnectionError,
            MaxTurnsExceededError,
            InterruptedError,
        )
        assert EdgeCrabError is not None
