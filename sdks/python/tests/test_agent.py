"""Tests for the edgecrab Agent high-level API."""

from __future__ import annotations

import json
from unittest.mock import patch, MagicMock, call

import httpx
import pytest

from edgecrab.agent import Agent, AsyncAgent, AgentResult
from edgecrab.types import ChatMessage, UsageInfo

from conftest import MOCK_COMPLETION_RESPONSE, MOCK_MODELS_RESPONSE, MOCK_HEALTH_RESPONSE


class TestAgentInit:
    """Agent initialization tests."""

    def test_default_model(self):
        agent = Agent()
        assert agent.model == "anthropic/claude-sonnet-4-20250514"
        agent.close()

    def test_custom_model(self):
        agent = Agent(model="openai/gpt-4o")
        assert agent.model == "openai/gpt-4o"
        agent.close()

    def test_system_prompt_in_history(self):
        agent = Agent(system_prompt="You are helpful")
        assert len(agent.messages) == 1
        assert agent.messages[0].role == "system"
        assert agent.messages[0].content == "You are helpful"
        agent.close()

    def test_session_id_auto_generated(self):
        agent = Agent()
        assert agent.session_id is not None
        assert len(agent.session_id) > 0
        agent.close()

    def test_session_id_explicit(self):
        agent = Agent(session_id="my-session")
        assert agent.session_id == "my-session"
        agent.close()

    def test_context_manager(self):
        with Agent() as agent:
            assert isinstance(agent, Agent)


class TestAgentChat:
    """Tests for Agent.chat() method."""

    def test_chat_returns_reply(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_COMPLETION_RESPONSE,
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        with patch.object(httpx.Client, "post", return_value=mock_response):
            with Agent() as agent:
                reply = agent.chat("Hello")
                assert reply == "Hello! I'm EdgeCrab."

    def test_chat_maintains_history(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_COMPLETION_RESPONSE,
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        with patch.object(httpx.Client, "post", return_value=mock_response):
            with Agent() as agent:
                agent.chat("First message")
                assert agent.turn_count == 1
                assert len(agent.messages) == 2  # user + assistant

                agent.chat("Second message")
                assert agent.turn_count == 2
                assert len(agent.messages) == 4  # 2x (user + assistant)

    def test_chat_with_system_prompt(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_COMPLETION_RESPONSE,
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        with patch.object(httpx.Client, "post", return_value=mock_response):
            with Agent(system_prompt="Be concise") as agent:
                agent.chat("Hello")
                # system + user + assistant
                assert len(agent.messages) == 3
                assert agent.messages[0].role == "system"

    def test_on_turn_callback(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_COMPLETION_RESPONSE,
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        on_turn = MagicMock()
        with patch.object(httpx.Client, "post", return_value=mock_response):
            with Agent(on_turn=on_turn) as agent:
                agent.chat("Hello")
                on_turn.assert_called_once()
                turn_num, msg = on_turn.call_args[0]
                assert turn_num == 1
                assert msg.role == "assistant"


class TestAgentRun:
    """Tests for Agent.run() method."""

    def test_run_returns_agent_result(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_COMPLETION_RESPONSE,
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        with patch.object(httpx.Client, "post", return_value=mock_response):
            with Agent() as agent:
                result = agent.run("Hello")
                assert isinstance(result, AgentResult)
                assert result.response == "Hello! I'm EdgeCrab."
                assert result.turns_used == 1
                assert result.finished_naturally is True
                assert result.session_id == agent.session_id
                assert result.model == agent.model
                assert len(result.messages) == 2


class TestAgentReset:
    """Tests for Agent.reset() method."""

    def test_reset_clears_history(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_COMPLETION_RESPONSE,
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        with patch.object(httpx.Client, "post", return_value=mock_response):
            with Agent() as agent:
                agent.chat("Hello")
                assert agent.turn_count == 1

                old_session = agent.session_id
                agent.reset()
                assert agent.turn_count == 0
                assert len(agent.messages) == 0
                assert agent.session_id != old_session

    def test_reset_preserves_system_prompt(self):
        with Agent(system_prompt="Be helpful") as agent:
            agent.reset()
            assert len(agent.messages) == 1
            assert agent.messages[0].role == "system"


class TestAgentAddMessage:
    """Tests for Agent.add_message() method."""

    def test_add_message(self):
        with Agent() as agent:
            agent.add_message("user", "injected message")
            assert len(agent.messages) == 1
            assert agent.messages[0].content == "injected message"


class TestAgentUsage:
    """Tests for usage accumulation."""

    def test_usage_accumulates(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_COMPLETION_RESPONSE,
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        with patch.object(httpx.Client, "post", return_value=mock_response):
            with Agent() as agent:
                agent.chat("First")
                agent.chat("Second")
                assert agent.usage.total_tokens == 36  # 18 * 2


class TestAgentModelsHealth:
    """Tests for models/health passthrough."""

    def test_list_models(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_MODELS_RESPONSE,
            request=httpx.Request("GET", "http://test/v1/models"),
        )
        with patch.object(httpx.Client, "get", return_value=mock_response):
            with Agent() as agent:
                models = agent.list_models()
                assert len(models) == 2

    def test_health(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_HEALTH_RESPONSE,
            request=httpx.Request("GET", "http://test/v1/health"),
        )
        with patch.object(httpx.Client, "get", return_value=mock_response):
            with Agent() as agent:
                h = agent.health()
                assert h["status"] == "ok"


class TestAgentExports:
    """Test that Agent is properly exported."""

    def test_agent_in_all(self):
        import edgecrab
        assert "Agent" in edgecrab.__all__
        assert "AsyncAgent" in edgecrab.__all__
        assert "AgentResult" in edgecrab.__all__

    def test_agent_importable(self):
        from edgecrab import Agent, AsyncAgent, AgentResult
        assert Agent is not None
        assert AsyncAgent is not None
        assert AgentResult is not None
