"""Core message types for the EdgeCrab Python SDK.

These mirror the Rust types in edgecrab-types and provide a Pythonic
interface for constructing and inspecting conversation messages.

Usage:
    from edgecrab import Message, Role

    msg = Message.user("Hello!")
    print(msg.role)     # Role.USER
    print(msg.content)  # "Hello!"
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Optional

__all__ = ["Role", "Message", "ContentPart"]


class Role(str, Enum):
    """Message role in a conversation."""

    SYSTEM = "system"
    USER = "user"
    ASSISTANT = "assistant"
    TOOL = "tool"


@dataclass
class ContentPart:
    """A part of multimodal content."""

    type: str  # "text" or "image_url"
    text: Optional[str] = None
    image_url: Optional[str] = None
    detail: Optional[str] = None

    def to_dict(self) -> dict[str, Any]:
        if self.type == "text":
            return {"type": "text", "text": self.text or ""}
        return {
            "type": "image_url",
            "image_url": {"url": self.image_url or "", "detail": self.detail},
        }


@dataclass
class Message:
    """A conversation message.

    Mirrors the Rust `Message` type with convenience constructors.
    """

    role: Role
    content: Optional[str | list[ContentPart]] = None
    tool_calls: Optional[list[dict[str, Any]]] = None
    tool_call_id: Optional[str] = None
    name: Optional[str] = None
    reasoning: Optional[str] = None

    @staticmethod
    def user(text: str) -> Message:
        """Create a user message."""
        return Message(role=Role.USER, content=text)

    @staticmethod
    def system(text: str) -> Message:
        """Create a system message."""
        return Message(role=Role.SYSTEM, content=text)

    @staticmethod
    def assistant(text: str) -> Message:
        """Create an assistant message."""
        return Message(role=Role.ASSISTANT, content=text)

    @staticmethod
    def tool_result(tool_call_id: str, name: str, content: str) -> Message:
        """Create a tool result message."""
        return Message(
            role=Role.TOOL,
            content=content,
            tool_call_id=tool_call_id,
            name=name,
        )

    @property
    def text(self) -> Optional[str]:
        """Extract plain text content, joining multimodal parts if needed."""
        if self.content is None:
            return None
        if isinstance(self.content, str):
            return self.content
        # Multimodal: join text parts
        texts = [p.text for p in self.content if p.type == "text" and p.text]
        return "".join(texts) if texts else None

    def to_dict(self) -> dict[str, Any]:
        """Serialize to a dictionary compatible with the native layer."""
        d: dict[str, Any] = {"role": self.role.value}
        if self.content is not None:
            if isinstance(self.content, str):
                d["content"] = self.content
            else:
                d["content"] = [p.to_dict() for p in self.content]
        if self.tool_calls:
            d["tool_calls"] = self.tool_calls
        if self.tool_call_id:
            d["tool_call_id"] = self.tool_call_id
        if self.name:
            d["name"] = self.name
        if self.reasoning:
            d["reasoning"] = self.reasoning
        return d

    def __repr__(self) -> str:
        content_preview = ""
        if self.content:
            text = self.text or ""
            content_preview = text[:50] + ("..." if len(text) > 50 else "")
        return f"Message(role={self.role.value!r}, content={content_preview!r})"
