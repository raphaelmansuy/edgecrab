"""Pydantic models for the EdgeCrab OpenAI-compatible API."""

from __future__ import annotations

from typing import Any, Literal, Optional

from pydantic import BaseModel, Field


class ChatMessage(BaseModel):
    """A single message in a conversation."""

    role: Literal["system", "user", "assistant", "tool"] = "user"
    content: str
    name: Optional[str] = None
    tool_call_id: Optional[str] = None


class ChatCompletionRequest(BaseModel):
    """Request body for POST /v1/chat/completions."""

    model: str = "anthropic/claude-sonnet-4-20250514"
    messages: list[ChatMessage]
    temperature: Optional[float] = None
    max_tokens: Optional[int] = None
    stream: bool = False
    tools: Optional[list[dict[str, Any]]] = None


class UsageInfo(BaseModel):
    """Token usage statistics."""

    prompt_tokens: int = 0
    completion_tokens: int = 0
    total_tokens: int = 0


class ChatChoice(BaseModel):
    """A single choice in a chat completion response."""

    index: int = 0
    message: ChatMessage
    finish_reason: Optional[str] = None


class ChatCompletionResponse(BaseModel):
    """Response body from POST /v1/chat/completions (non-streaming)."""

    id: str = ""
    object: str = "chat.completion"
    created: int = 0
    model: str = ""
    choices: list[ChatChoice] = Field(default_factory=list)
    usage: Optional[UsageInfo] = None


class StreamDelta(BaseModel):
    """Delta payload in a streaming chunk."""

    role: Optional[str] = None
    content: Optional[str] = None


class StreamChoice(BaseModel):
    """A single choice in a streaming chunk."""

    index: int = 0
    delta: StreamDelta = Field(default_factory=StreamDelta)
    finish_reason: Optional[str] = None


class StreamChunk(BaseModel):
    """A single SSE chunk from a streaming completion."""

    id: str = ""
    object: str = "chat.completion.chunk"
    created: int = 0
    model: str = ""
    choices: list[StreamChoice] = Field(default_factory=list)


class ModelInfo(BaseModel):
    """Model metadata from GET /v1/models."""

    id: str
    object: str = "model"
    created: Optional[int] = None
    owned_by: Optional[str] = None


class HealthResponse(BaseModel):
    """Response from GET /v1/health."""

    status: str = "ok"
    version: Optional[str] = None
