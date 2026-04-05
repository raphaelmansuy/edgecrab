"""Shared test fixtures for edgecrab Python SDK tests."""

from __future__ import annotations

import json

import pytest
import httpx

MOCK_COMPLETION_RESPONSE = {
    "id": "chatcmpl-test123",
    "object": "chat.completion",
    "created": 1700000000,
    "model": "anthropic/claude-sonnet-4-20250514",
    "choices": [
        {
            "index": 0,
            "message": {"role": "assistant", "content": "Hello! I'm EdgeCrab."},
            "finish_reason": "stop",
        }
    ],
    "usage": {"prompt_tokens": 10, "completion_tokens": 8, "total_tokens": 18},
}

MOCK_MODELS_RESPONSE = {
    "data": [
        {"id": "anthropic/claude-sonnet-4-20250514", "object": "model", "owned_by": "anthropic"},
        {"id": "openai/gpt-4", "object": "model", "owned_by": "openai"},
    ]
}

MOCK_HEALTH_RESPONSE = {"status": "ok", "version": "0.1.0"}


MOCK_STREAM_LINES = [
    'data: {"id":"chatcmpl-s1","object":"chat.completion.chunk","created":1700000000,"model":"test","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello"},"finish_reason":null}]}',
    'data: {"id":"chatcmpl-s1","object":"chat.completion.chunk","created":1700000000,"model":"test","choices":[{"index":0,"delta":{"content":" world"},"finish_reason":null}]}',
    'data: {"id":"chatcmpl-s1","object":"chat.completion.chunk","created":1700000000,"model":"test","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}',
    "data: [DONE]",
]
