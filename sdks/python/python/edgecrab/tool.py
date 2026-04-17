"""Python @Tool decorator for registering custom tools with an EdgeCrab agent.

Usage:
    from edgecrab import Tool

    @Tool("get_weather", description="Get weather for a city")
    def get_weather(city: str, units: str = "celsius") -> dict:
        '''Get current weather for a city.

        Args:
            city: City name
            units: Temperature units (celsius or fahrenheit)
        '''
        return {"city": city, "temp": 22, "units": units}

    agent = Agent("anthropic/claude-sonnet-4", tools=[get_weather])
"""

from __future__ import annotations

import inspect
import json
from dataclasses import dataclass, field
from typing import Any, Callable, Optional, get_type_hints

__all__ = ["Tool", "ToolSchema"]

# Python type → JSON Schema type mapping
_TYPE_MAP: dict[type, str] = {
    str: "string",
    int: "integer",
    float: "number",
    bool: "boolean",
    list: "array",
    dict: "object",
}


@dataclass
class ToolSchema:
    """JSON Schema for a tool's parameters."""

    name: str
    description: str
    parameters: dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        return {
            "name": self.name,
            "description": self.description,
            "parameters": self.parameters,
        }


def _python_type_to_json_schema(py_type: type) -> dict[str, str]:
    """Convert a Python type annotation to a JSON Schema type descriptor."""
    origin = getattr(py_type, "__origin__", None)

    # Handle Optional[X] (Union[X, None])
    if origin is type(None):
        return {"type": "string"}

    # Check basic type map
    json_type = _TYPE_MAP.get(py_type)
    if json_type:
        return {"type": json_type}

    # Fallback
    return {"type": "string"}


def _extract_param_descriptions(func: Callable) -> dict[str, str]:
    """Extract parameter descriptions from Google/Sphinx-style docstrings."""
    doc = inspect.getdoc(func) or ""
    descriptions: dict[str, str] = {}

    in_args = False
    for line in doc.split("\n"):
        stripped = line.strip()

        if stripped.lower().startswith("args:") or stripped.lower().startswith("parameters:"):
            in_args = True
            continue

        if in_args:
            if stripped and not stripped.startswith("-") and ":" in stripped:
                # "param_name: description" or "param_name (type): description"
                parts = stripped.split(":", 1)
                if len(parts) == 2:
                    param = parts[0].strip().split("(")[0].strip()
                    desc = parts[1].strip()
                    descriptions[param] = desc
            elif stripped == "" or (not stripped.startswith(" ") and stripped.endswith(":")):
                in_args = False

    return descriptions


def _build_schema(
    func: Callable,
    name: str,
    description: Optional[str],
) -> ToolSchema:
    """Build a ToolSchema from a Python function's signature and type hints."""
    sig = inspect.signature(func)
    hints = get_type_hints(func)
    param_docs = _extract_param_descriptions(func)

    # Use the function's docstring first line as description if not provided
    if description is None:
        doc = inspect.getdoc(func) or ""
        description = doc.split("\n")[0] if doc else f"Tool: {name}"

    properties: dict[str, Any] = {}
    required: list[str] = []

    for param_name, param in sig.parameters.items():
        if param_name in ("self", "cls", "ctx", "context"):
            continue

        py_type = hints.get(param_name, str)

        # Check if Optional
        is_optional = False
        origin = getattr(py_type, "__origin__", None)
        if origin is not None:
            args = getattr(py_type, "__args__", ())
            if type(None) in args:
                is_optional = True
                # Get the non-None type
                non_none = [a for a in args if a is not type(None)]
                py_type = non_none[0] if non_none else str

        prop: dict[str, Any] = _python_type_to_json_schema(py_type)

        if param_name in param_docs:
            prop["description"] = param_docs[param_name]

        properties[param_name] = prop

        if param.default is inspect.Parameter.empty and not is_optional:
            required.append(param_name)

    parameters = {
        "type": "object",
        "properties": properties,
    }
    if required:
        parameters["required"] = required

    return ToolSchema(name=name, description=description, parameters=parameters)


class Tool:
    """Decorator for registering Python functions as EdgeCrab tools.

    Can be used as:
        @Tool("tool_name")
        def my_tool(arg: str) -> str: ...

        @Tool("tool_name", description="Does X")
        def my_tool(arg: str) -> str: ...
    """

    def __init__(
        self,
        name: str,
        *,
        description: Optional[str] = None,
        toolset: str = "custom",
        emoji: str = "🔧",
    ):
        self.name = name
        self.description = description
        self.toolset = toolset
        self.emoji = emoji
        self._func: Optional[Callable] = None
        self._schema: Optional[ToolSchema] = None

    def __call__(self, func: Callable) -> "Tool":
        """When used as @Tool("name"), this is called with the decorated function."""
        self._func = func
        self._schema = _build_schema(func, self.name, self.description)
        self.description = self._schema.description
        return self

    @property
    def schema(self) -> ToolSchema:
        """Get the tool's JSON Schema."""
        if self._schema is None:
            raise RuntimeError("Tool decorator was not applied to a function")
        return self._schema

    def execute(self, args: dict[str, Any]) -> Any:
        """Execute the tool function with the given arguments."""
        if self._func is None:
            raise RuntimeError("Tool decorator was not applied to a function")

        # Handle both sync and async functions
        if inspect.iscoroutinefunction(self._func):
            import asyncio
            return asyncio.get_event_loop().run_until_complete(self._func(**args))
        return self._func(**args)

    def to_dict(self) -> dict[str, Any]:
        """Serialize the tool definition for passing to the native layer."""
        return {
            "name": self.name,
            "description": self.description or "",
            "toolset": self.toolset,
            "emoji": self.emoji,
            "schema": self.schema.to_dict(),
        }

    def __repr__(self) -> str:
        return f"Tool(name='{self.name}', toolset='{self.toolset}')"
