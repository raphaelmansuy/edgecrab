"""edgecrab — Python SDK for the EdgeCrab autonomous coding agent."""

from __future__ import annotations

from edgecrab._version import __version__
from edgecrab.agent import Agent, AsyncAgent, AgentResult
from edgecrab.client import (
    EdgeCrabClient,
    AsyncEdgeCrabClient,
    EdgeCrabError,
    AuthenticationError,
    RateLimitError,
    ServerError,
    TimeoutError,
    ConnectionError,
    MaxTurnsExceededError,
    InterruptedError,
)
from edgecrab.types import (
    ChatMessage,
    ChatCompletionRequest,
    ChatCompletionResponse,
    ChatChoice,
    UsageInfo,
    ModelInfo,
    HealthResponse,
    StreamDelta,
    StreamChoice,
    StreamChunk,
)

__all__ = [
    "__version__",
    # High-level Agent API
    "Agent",
    "AsyncAgent",
    "AgentResult",
    # Low-level HTTP clients
    "EdgeCrabClient",
    "AsyncEdgeCrabClient",
    # Error hierarchy
    "EdgeCrabError",
    "AuthenticationError",
    "RateLimitError",
    "ServerError",
    "TimeoutError",
    "ConnectionError",
    "MaxTurnsExceededError",
    "InterruptedError",
    # Types
    "ChatMessage",
    "ChatCompletionRequest",
    "ChatCompletionResponse",
    "ChatChoice",
    "UsageInfo",
    "ModelInfo",
    "HealthResponse",
    "StreamDelta",
    "StreamChoice",
    "StreamChunk",
]
