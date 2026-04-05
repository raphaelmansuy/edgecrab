"""Synchronous and asynchronous HTTP clients for the EdgeCrab API."""

from __future__ import annotations

import json
import time
from typing import Any, Generator, Optional

import httpx

from edgecrab.types import (
    ChatCompletionRequest,
    ChatCompletionResponse,
    ChatMessage,
    HealthResponse,
    ModelInfo,
    StreamChunk,
)

_DEFAULT_BASE_URL = "http://127.0.0.1:8642"
_DEFAULT_TIMEOUT = 120.0
_DEFAULT_MAX_RETRIES = 3
_DEFAULT_RETRY_BASE_DELAY = 1.0


# ── Structured Error Hierarchy ──────────────────────────────────


class EdgeCrabError(Exception):
    """Base exception for all EdgeCrab API errors."""

    def __init__(self, message: str, status_code: int | None = None) -> None:
        super().__init__(message)
        self.status_code = status_code


class AuthenticationError(EdgeCrabError):
    """Raised on 401/403 — invalid or missing API key."""

    pass


class RateLimitError(EdgeCrabError):
    """Raised on 429 — too many requests."""

    def __init__(self, message: str, retry_after: float | None = None) -> None:
        super().__init__(message, status_code=429)
        self.retry_after = retry_after


class ServerError(EdgeCrabError):
    """Raised on 5xx — server-side errors (retryable)."""

    pass


class TimeoutError(EdgeCrabError):
    """Raised when a request times out."""

    def __init__(self, message: str = "Request timed out") -> None:
        super().__init__(message, status_code=None)


class ConnectionError(EdgeCrabError):
    """Raised when the server is unreachable."""

    def __init__(self, message: str = "Could not connect to EdgeCrab server") -> None:
        super().__init__(message, status_code=None)


class MaxTurnsExceededError(EdgeCrabError):
    """Raised when the agent exceeds its configured max_turns."""

    def __init__(self, max_turns: int) -> None:
        super().__init__(f"Agent exceeded maximum turns ({max_turns})", status_code=None)
        self.max_turns = max_turns


class InterruptedError(EdgeCrabError):
    """Raised when an agent conversation is interrupted."""

    def __init__(self) -> None:
        super().__init__("Agent conversation was interrupted", status_code=None)


def _classify_error(status_code: int, detail: str, headers: dict[str, str] | None = None) -> EdgeCrabError:
    """Create the appropriate error subclass based on HTTP status code."""
    msg = f"API error {status_code}: {detail}"
    if status_code in (401, 403):
        return AuthenticationError(msg, status_code=status_code)
    if status_code == 429:
        retry_after = None
        if headers:
            ra = headers.get("retry-after")
            if ra:
                try:
                    retry_after = float(ra)
                except ValueError:
                    pass
        return RateLimitError(msg, retry_after=retry_after)
    if status_code >= 500:
        return ServerError(msg, status_code=status_code)
    return EdgeCrabError(msg, status_code=status_code)


def _is_retryable(exc: Exception) -> bool:
    """Return True if the error is transient and worth retrying."""
    if isinstance(exc, (ServerError, TimeoutError, ConnectionError)):
        return True
    if isinstance(exc, RateLimitError):
        return True
    if isinstance(exc, httpx.TimeoutException):
        return True
    if isinstance(exc, httpx.ConnectError):
        return True
    return False


def _build_headers(api_key: str | None) -> dict[str, str]:
    headers: dict[str, str] = {"Content-Type": "application/json"}
    if api_key:
        headers["Authorization"] = f"Bearer {api_key}"
    return headers


def _parse_sse_line(line: str) -> StreamChunk | None:
    """Parse a single SSE data line into a StreamChunk."""
    if not line.startswith("data: "):
        return None
    payload = line[6:].strip()
    if payload == "[DONE]":
        return None
    return StreamChunk.model_validate_json(payload)


class EdgeCrabClient:
    """Synchronous client for the EdgeCrab OpenAI-compatible API.

    Parameters
    ----------
    base_url:
        Base URL of the EdgeCrab API server. Default: ``http://127.0.0.1:8642``.
    api_key:
        Optional bearer token for authentication.
    timeout:
        HTTP request timeout in seconds. Default: 120.
    max_retries:
        Maximum number of retries on transient errors. Default: 3.
    retry_base_delay:
        Base delay in seconds for exponential backoff. Default: 1.0.
    """

    def __init__(
        self,
        base_url: str = _DEFAULT_BASE_URL,
        api_key: str | None = None,
        timeout: float = _DEFAULT_TIMEOUT,
        max_retries: int = _DEFAULT_MAX_RETRIES,
        retry_base_delay: float = _DEFAULT_RETRY_BASE_DELAY,
    ) -> None:
        self._base_url = base_url.rstrip("/")
        self._api_key = api_key
        self._max_retries = max_retries
        self._retry_base_delay = retry_base_delay
        self._client = httpx.Client(
            base_url=self._base_url,
            headers=_build_headers(api_key),
            timeout=timeout,
        )

    def close(self) -> None:
        """Close the underlying HTTP client."""
        self._client.close()

    def __enter__(self) -> "EdgeCrabClient":
        return self

    def __exit__(self, *args: Any) -> None:
        self.close()

    # ── Chat completions ────────────────────────────────────────────

    def chat(
        self,
        message: str,
        *,
        model: str = "anthropic/claude-sonnet-4-20250514",
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
    ) -> str:
        """Send a single message and return the assistant's reply.

        This is the simplest interface — for full control, use
        :meth:`create_completion`.
        """
        messages: list[ChatMessage] = []
        if system:
            messages.append(ChatMessage(role="system", content=system))
        messages.append(ChatMessage(role="user", content=message))
        resp = self.create_completion(
            messages=messages,
            model=model,
            temperature=temperature,
            max_tokens=max_tokens,
        )
        if resp.choices:
            return resp.choices[0].message.content
        return ""

    def create_completion(
        self,
        messages: list[ChatMessage],
        *,
        model: str = "anthropic/claude-sonnet-4-20250514",
        temperature: float | None = None,
        max_tokens: int | None = None,
        stream: bool = False,
        tools: list[dict[str, Any]] | None = None,
    ) -> ChatCompletionResponse:
        """Create a chat completion (non-streaming).

        Parameters
        ----------
        messages:
            Conversation messages.
        model:
            Model identifier.
        temperature:
            Sampling temperature.
        max_tokens:
            Maximum tokens to generate.
        stream:
            Must be False for this method. Use :meth:`stream_completion` for streaming.
        tools:
            Optional tool definitions.
        """
        if stream:
            raise ValueError("Use stream_completion() for streaming requests")
        req = ChatCompletionRequest(
            model=model,
            messages=messages,
            temperature=temperature,
            max_tokens=max_tokens,
            stream=False,
            tools=tools,
        )
        return self._post_with_retry(req)

    def _post_with_retry(self, req: ChatCompletionRequest) -> ChatCompletionResponse:
        """POST with exponential backoff on transient errors."""
        last_exc: Exception | None = None
        for attempt in range(self._max_retries + 1):
            try:
                response = self._client.post(
                    "/v1/chat/completions",
                    content=req.model_dump_json(exclude_none=True),
                )
                self._check_response(response)
                return ChatCompletionResponse.model_validate_json(response.content)
            except (httpx.TimeoutException,) as exc:
                last_exc = TimeoutError(str(exc))
            except (httpx.ConnectError,) as exc:
                last_exc = ConnectionError(str(exc))
            except EdgeCrabError as exc:
                if not _is_retryable(exc) or attempt == self._max_retries:
                    raise
                last_exc = exc

            if attempt < self._max_retries:
                delay = self._retry_base_delay * (2 ** attempt)
                if isinstance(last_exc, RateLimitError) and last_exc.retry_after:
                    delay = max(delay, last_exc.retry_after)
                time.sleep(delay)

        raise last_exc  # type: ignore[misc]

    def stream_completion(
        self,
        messages: list[ChatMessage],
        *,
        model: str = "anthropic/claude-sonnet-4-20250514",
        temperature: float | None = None,
        max_tokens: int | None = None,
        tools: list[dict[str, Any]] | None = None,
    ) -> Generator[StreamChunk, None, None]:
        """Create a streaming chat completion and yield chunks.

        Parameters
        ----------
        messages:
            Conversation messages.
        model:
            Model identifier.
        temperature:
            Sampling temperature.
        max_tokens:
            Maximum tokens to generate.
        tools:
            Optional tool definitions.

        Yields
        ------
        StreamChunk
            Individual streaming chunks as they arrive.
        """
        req = ChatCompletionRequest(
            model=model,
            messages=messages,
            temperature=temperature,
            max_tokens=max_tokens,
            stream=True,
            tools=tools,
        )
        with self._client.stream(
            "POST",
            "/v1/chat/completions",
            content=req.model_dump_json(exclude_none=True),
        ) as response:
            self._check_response(response)
            for line in response.iter_lines():
                chunk = _parse_sse_line(line)
                if chunk is not None:
                    yield chunk

    # ── Models ──────────────────────────────────────────────────────

    def list_models(self) -> list[ModelInfo]:
        """List available models."""
        response = self._client.get("/v1/models")
        self._check_response(response)
        data = response.json()
        models_data = data.get("data", data) if isinstance(data, dict) else data
        return [ModelInfo.model_validate(m) for m in models_data]

    # ── Health ──────────────────────────────────────────────────────

    def health(self) -> HealthResponse:
        """Check API server health."""
        response = self._client.get("/v1/health")
        self._check_response(response)
        return HealthResponse.model_validate_json(response.content)

    # ── Internal ────────────────────────────────────────────────────

    @staticmethod
    def _check_response(response: httpx.Response) -> None:
        if response.status_code >= 400:
            try:
                detail = response.json().get("error", {}).get("message", response.text)
            except Exception:
                detail = response.text
            headers = dict(response.headers) if hasattr(response, "headers") else {}
            raise _classify_error(response.status_code, detail, headers)


class AsyncEdgeCrabClient:
    """Asynchronous client for the EdgeCrab OpenAI-compatible API.

    Parameters
    ----------
    base_url:
        Base URL of the EdgeCrab API server. Default: ``http://127.0.0.1:8642``.
    api_key:
        Optional bearer token for authentication.
    timeout:
        HTTP request timeout in seconds. Default: 120.
    max_retries:
        Maximum number of retries on transient errors. Default: 3.
    retry_base_delay:
        Base delay in seconds for exponential backoff. Default: 1.0.
    """

    def __init__(
        self,
        base_url: str = _DEFAULT_BASE_URL,
        api_key: str | None = None,
        timeout: float = _DEFAULT_TIMEOUT,
        max_retries: int = _DEFAULT_MAX_RETRIES,
        retry_base_delay: float = _DEFAULT_RETRY_BASE_DELAY,
    ) -> None:
        self._base_url = base_url.rstrip("/")
        self._api_key = api_key
        self._max_retries = max_retries
        self._retry_base_delay = retry_base_delay
        self._client = httpx.AsyncClient(
            base_url=self._base_url,
            headers=_build_headers(api_key),
            timeout=timeout,
        )

    async def close(self) -> None:
        """Close the underlying HTTP client."""
        await self._client.aclose()

    async def __aenter__(self) -> "AsyncEdgeCrabClient":
        return self

    async def __aexit__(self, *args: Any) -> None:
        await self.close()

    # ── Chat completions ────────────────────────────────────────────

    async def chat(
        self,
        message: str,
        *,
        model: str = "anthropic/claude-sonnet-4-20250514",
        system: str | None = None,
        temperature: float | None = None,
        max_tokens: int | None = None,
    ) -> str:
        """Send a single message and return the assistant's reply."""
        messages: list[ChatMessage] = []
        if system:
            messages.append(ChatMessage(role="system", content=system))
        messages.append(ChatMessage(role="user", content=message))
        resp = await self.create_completion(
            messages=messages,
            model=model,
            temperature=temperature,
            max_tokens=max_tokens,
        )
        if resp.choices:
            return resp.choices[0].message.content
        return ""

    async def create_completion(
        self,
        messages: list[ChatMessage],
        *,
        model: str = "anthropic/claude-sonnet-4-20250514",
        temperature: float | None = None,
        max_tokens: int | None = None,
        stream: bool = False,
        tools: list[dict[str, Any]] | None = None,
    ) -> ChatCompletionResponse:
        """Create a chat completion (non-streaming)."""
        if stream:
            raise ValueError("Use stream_completion() for streaming requests")
        req = ChatCompletionRequest(
            model=model,
            messages=messages,
            temperature=temperature,
            max_tokens=max_tokens,
            stream=False,
            tools=tools,
        )
        return await self._post_with_retry(req)

    async def _post_with_retry(self, req: ChatCompletionRequest) -> ChatCompletionResponse:
        """POST with exponential backoff on transient errors."""
        import asyncio

        last_exc: Exception | None = None
        for attempt in range(self._max_retries + 1):
            try:
                response = await self._client.post(
                    "/v1/chat/completions",
                    content=req.model_dump_json(exclude_none=True),
                )
                self._check_response(response)
                return ChatCompletionResponse.model_validate_json(response.content)
            except (httpx.TimeoutException,) as exc:
                last_exc = TimeoutError(str(exc))
            except (httpx.ConnectError,) as exc:
                last_exc = ConnectionError(str(exc))
            except EdgeCrabError as exc:
                if not _is_retryable(exc) or attempt == self._max_retries:
                    raise
                last_exc = exc

            if attempt < self._max_retries:
                delay = self._retry_base_delay * (2 ** attempt)
                if isinstance(last_exc, RateLimitError) and last_exc.retry_after:
                    delay = max(delay, last_exc.retry_after)
                await asyncio.sleep(delay)

        raise last_exc  # type: ignore[misc]

    async def stream_completion(
        self,
        messages: list[ChatMessage],
        *,
        model: str = "anthropic/claude-sonnet-4-20250514",
        temperature: float | None = None,
        max_tokens: int | None = None,
        tools: list[dict[str, Any]] | None = None,
    ):
        """Create a streaming chat completion and yield chunks asynchronously."""
        req = ChatCompletionRequest(
            model=model,
            messages=messages,
            temperature=temperature,
            max_tokens=max_tokens,
            stream=True,
            tools=tools,
        )
        async with self._client.stream(
            "POST",
            "/v1/chat/completions",
            content=req.model_dump_json(exclude_none=True),
        ) as response:
            self._check_response(response)
            async for line in response.aiter_lines():
                chunk = _parse_sse_line(line)
                if chunk is not None:
                    yield chunk

    # ── Models ──────────────────────────────────────────────────────

    async def list_models(self) -> list[ModelInfo]:
        """List available models."""
        response = await self._client.get("/v1/models")
        self._check_response(response)
        data = response.json()
        models_data = data.get("data", data) if isinstance(data, dict) else data
        return [ModelInfo.model_validate(m) for m in models_data]

    # ── Health ──────────────────────────────────────────────────────

    async def health(self) -> HealthResponse:
        """Check API server health."""
        response = await self._client.get("/v1/health")
        self._check_response(response)
        return HealthResponse.model_validate_json(response.content)

    # ── Internal ────────────────────────────────────────────────────

    @staticmethod
    def _check_response(response: httpx.Response) -> None:
        if response.status_code >= 400:
            try:
                detail = response.json().get("error", {}).get("message", response.text)
            except Exception:
                detail = response.text
            headers = dict(response.headers) if hasattr(response, "headers") else {}
            raise _classify_error(response.status_code, detail, headers)
