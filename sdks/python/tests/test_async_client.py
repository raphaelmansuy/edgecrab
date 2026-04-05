"""Tests for the edgecrab Python SDK — async client."""

from __future__ import annotations

import json
from unittest.mock import AsyncMock, patch, MagicMock

import httpx
import pytest

from edgecrab.client import AsyncEdgeCrabClient, EdgeCrabError
from edgecrab.types import ChatMessage

from conftest import (
    MOCK_COMPLETION_RESPONSE,
    MOCK_HEALTH_RESPONSE,
    MOCK_MODELS_RESPONSE,
)


class TestAsyncClientInit:
    """Async client initialization tests."""

    @pytest.mark.asyncio
    async def test_async_context_manager(self):
        async with AsyncEdgeCrabClient() as client:
            assert isinstance(client, AsyncEdgeCrabClient)

    def test_default_base_url(self):
        client = AsyncEdgeCrabClient()
        assert client._base_url == "http://127.0.0.1:8642"

    def test_api_key_header(self):
        client = AsyncEdgeCrabClient(api_key="async-key")
        assert client._client.headers["authorization"] == "Bearer async-key"


class TestAsyncChat:
    """Tests for async chat completion (mocked HTTP)."""

    @pytest.mark.asyncio
    async def test_chat_returns_string(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_COMPLETION_RESPONSE,
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        with patch.object(httpx.AsyncClient, "post", return_value=mock_response):
            async with AsyncEdgeCrabClient() as client:
                reply = await client.chat("Hello")
                assert reply == "Hello! I'm EdgeCrab."

    @pytest.mark.asyncio
    async def test_chat_with_system(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_COMPLETION_RESPONSE,
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        with patch.object(httpx.AsyncClient, "post", return_value=mock_response) as mock_post:
            async with AsyncEdgeCrabClient() as client:
                await client.chat("Hello", system="Be concise")
                call_args = mock_post.call_args
                body = json.loads(call_args.kwargs.get("content", call_args[1].get("content", "")))
                assert body["messages"][0]["role"] == "system"

    @pytest.mark.asyncio
    async def test_create_completion(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_COMPLETION_RESPONSE,
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        with patch.object(httpx.AsyncClient, "post", return_value=mock_response):
            async with AsyncEdgeCrabClient() as client:
                resp = await client.create_completion(
                    messages=[ChatMessage(role="user", content="Hi")]
                )
                assert resp.id == "chatcmpl-test123"
                assert len(resp.choices) == 1

    @pytest.mark.asyncio
    async def test_stream_raises_on_non_streaming(self):
        async with AsyncEdgeCrabClient() as client:
            with pytest.raises(ValueError, match="stream_completion"):
                await client.create_completion(
                    messages=[ChatMessage(role="user", content="Hi")],
                    stream=True,
                )


class TestAsyncModels:
    """Tests for async model listing."""

    @pytest.mark.asyncio
    async def test_list_models(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_MODELS_RESPONSE,
            request=httpx.Request("GET", "http://test/v1/models"),
        )
        with patch.object(httpx.AsyncClient, "get", return_value=mock_response):
            async with AsyncEdgeCrabClient() as client:
                models = await client.list_models()
                assert len(models) == 2


class TestAsyncHealth:
    """Tests for async health check."""

    @pytest.mark.asyncio
    async def test_health(self):
        mock_response = httpx.Response(
            200,
            json=MOCK_HEALTH_RESPONSE,
            request=httpx.Request("GET", "http://test/v1/health"),
        )
        with patch.object(httpx.AsyncClient, "get", return_value=mock_response):
            async with AsyncEdgeCrabClient() as client:
                h = await client.health()
                assert h.status == "ok"


class TestAsyncErrorHandling:
    """Tests for async error handling."""

    @pytest.mark.asyncio
    async def test_api_error(self):
        mock_response = httpx.Response(
            403,
            json={"error": {"message": "Forbidden"}},
            request=httpx.Request("POST", "http://test/v1/chat/completions"),
        )
        with patch.object(httpx.AsyncClient, "post", return_value=mock_response):
            async with AsyncEdgeCrabClient() as client:
                with pytest.raises(EdgeCrabError) as exc_info:
                    await client.chat("Hello")
                assert exc_info.value.status_code == 403
