"""Tests for the edgecrab Python SDK — synchronous client."""

from __future__ import annotations

import json
from unittest.mock import MagicMock, patch

import httpx
import pytest

import edgecrab
from edgecrab.client import EdgeCrabClient, EdgeCrabError
from edgecrab.types import ChatMessage

from conftest import (
    MOCK_COMPLETION_RESPONSE,
    MOCK_HEALTH_RESPONSE,
    MOCK_MODELS_RESPONSE,
)


class TestVersion:
    """Version metadata tests."""

    def test_version_string(self):
        assert isinstance(edgecrab.__version__, str)
        assert len(edgecrab.__version__) > 0

    def test_version_semver(self):
        parts = edgecrab.__version__.split(".")
        assert len(parts) == 3
        for p in parts:
            assert p.isdigit()


class TestClientInit:
    """Client initialization tests."""

    def test_default_base_url(self):
        client = EdgeCrabClient()
        assert client._base_url == "http://127.0.0.1:8642"
        client.close()

    def test_custom_base_url(self):
        client = EdgeCrabClient(base_url="http://localhost:9999/")
        assert client._base_url == "http://localhost:9999"
        client.close()

    def test_context_manager(self):
        with EdgeCrabClient() as client:
            assert isinstance(client, EdgeCrabClient)

    def test_api_key_header(self):
        client = EdgeCrabClient(api_key="test-key-123")
        assert client._client.headers["authorization"] == "Bearer test-key-123"
        client.close()


class TestChat:
    """Tests for chat completion (mocked HTTP)."""

    def test_chat_returns_string(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_COMPLETION_RESPONSE,
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        with patch.object(httpx.Client, "post", return_value=mock_response):
            with EdgeCrabClient() as client:
                reply = client.chat("Hello")
                assert reply == "Hello! I'm EdgeCrab."

    def test_chat_with_system_prompt(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_COMPLETION_RESPONSE,
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        with patch.object(httpx.Client, "post", return_value=mock_response) as mock_post:
            with EdgeCrabClient() as client:
                client.chat("Hello", system="You are helpful")
                call_args = mock_post.call_args
                body = json.loads(call_args.kwargs.get("content", call_args[1].get("content", "")))
                assert body["messages"][0]["role"] == "system"
                assert body["messages"][1]["role"] == "user"

    def test_create_completion(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_COMPLETION_RESPONSE,
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        with patch.object(httpx.Client, "post", return_value=mock_response):
            with EdgeCrabClient() as client:
                resp = client.create_completion(
                    messages=[ChatMessage(role="user", content="Hi")]
                )
                assert resp.id == "chatcmpl-test123"
                assert len(resp.choices) == 1
                assert resp.choices[0].message.content == "Hello! I'm EdgeCrab."
                assert resp.usage is not None
                assert resp.usage.total_tokens == 18

    def test_create_completion_stream_raises(self):
        with EdgeCrabClient() as client:
            with pytest.raises(ValueError, match="stream_completion"):
                client.create_completion(
                    messages=[ChatMessage(role="user", content="Hi")],
                    stream=True,
                )


class TestModels:
    """Tests for model listing."""

    def test_list_models(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_MODELS_RESPONSE,
            request=httpx.Request("GET", "http://test/v1/models"),
        )
        with patch.object(httpx.Client, "get", return_value=mock_response):
            with EdgeCrabClient() as client:
                models = client.list_models()
                assert len(models) == 2
                assert models[0].id == "anthropic/claude-sonnet-4-20250514"
                assert models[1].owned_by == "openai"


class TestHealth:
    """Tests for health check."""

    def test_health_ok(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_HEALTH_RESPONSE,
            request=httpx.Request("GET", "http://test/v1/health"),
        )
        with patch.object(httpx.Client, "get", return_value=mock_response):
            with EdgeCrabClient() as client:
                h = client.health()
                assert h.status == "ok"
                assert h.version == edgecrab.__version__


class TestErrorHandling:
    """Tests for error handling."""

    def test_api_error_raises(self):
        mock_response = httpx.Response(
            401,
            json={"error": {"message": "Unauthorized"}},
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        with patch.object(httpx.Client, "post", return_value=mock_response):
            with EdgeCrabClient() as client:
                with pytest.raises(EdgeCrabError) as exc_info:
                    client.chat("Hello")
                assert exc_info.value.status_code == 401
                assert "Unauthorized" in str(exc_info.value)

    def test_api_error_non_json(self):
        mock_response = httpx.Response(
            500,
            text="Internal Server Error",
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        with patch.object(httpx.Client, "post", return_value=mock_response):
            with EdgeCrabClient() as client:
                with pytest.raises(EdgeCrabError) as exc_info:
                    client.chat("Hello")
                assert exc_info.value.status_code == 500


class TestTypes:
    """Tests for Pydantic type models."""

    def test_chat_message_defaults(self):
        msg = ChatMessage(content="hello")
        assert msg.role == "user"
        assert msg.content == "hello"

    def test_chat_message_serialization(self):
        msg = ChatMessage(role="system", content="You are helpful")
        data = msg.model_dump()
        assert data["role"] == "system"
        assert data["content"] == "You are helpful"

    def test_completion_response_parsing(self):
        from edgecrab.types import ChatCompletionResponse

        resp = ChatCompletionResponse.model_validate(MOCK_COMPLETION_RESPONSE)
        assert resp.model == "anthropic/claude-sonnet-4-20250514"
        assert resp.choices[0].finish_reason == "stop"


class TestAllExports:
    """Test that all public exports are present."""

    def test_core_exports(self):
        assert "EdgeCrabClient" in edgecrab.__all__
        assert "AsyncEdgeCrabClient" in edgecrab.__all__
        assert "__version__" in edgecrab.__all__

    def test_type_exports(self):
        assert "ChatMessage" in edgecrab.__all__
        assert "ChatCompletionResponse" in edgecrab.__all__
        assert "StreamChunk" in edgecrab.__all__
