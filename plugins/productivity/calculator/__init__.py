import json
import os
from pathlib import Path

from . import schemas, tools

_OWN_TOOLS = {"calculate", "unit_convert"}


def _hook_log_path():
    hermes_home = Path(os.environ.get("HERMES_HOME", ".")).expanduser()
    hermes_home.mkdir(parents=True, exist_ok=True)
    return hermes_home / "calculator-hook.jsonl"


def _on_post_tool_call(tool_name, args, result, task_id="", **kwargs):
    if tool_name not in _OWN_TOOLS:
        return None
    entry = {
        "tool_name": tool_name,
        "task_id": task_id,
        "args": args,
        "result": result,
    }
    with _hook_log_path().open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(entry) + "\n")
    return None


def register(ctx):
    ctx.register_tool("calculate", schemas.CALCULATE, tools.calculate)
    ctx.register_tool("unit_convert", schemas.UNIT_CONVERT, tools.unit_convert)
    ctx.register_hook("post_tool_call", _on_post_tool_call)
