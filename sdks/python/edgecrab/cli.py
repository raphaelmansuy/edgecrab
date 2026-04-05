"""CLI entry point for the EdgeCrab Python SDK.

Usage
-----
  edgecrab chat "Hello, what can you do?"
  edgecrab chat --model gpt-4 --system "You are a Rust expert" "How to use traits?"
  edgecrab models
  edgecrab health
"""

from __future__ import annotations

import argparse
import json
import os
import sys

from edgecrab._version import __version__
from edgecrab.client import EdgeCrabClient, EdgeCrabError


def _get_client(args: argparse.Namespace) -> EdgeCrabClient:
    base_url = args.base_url or os.environ.get("EDGECRAB_BASE_URL", "http://127.0.0.1:8642")
    api_key = args.api_key or os.environ.get("EDGECRAB_API_KEY")
    return EdgeCrabClient(base_url=base_url, api_key=api_key)


def _cmd_chat(args: argparse.Namespace) -> None:
    message = " ".join(args.message)
    if not message:
        print("Error: no message provided", file=sys.stderr)
        sys.exit(1)
    with _get_client(args) as client:
        if args.stream:
            from edgecrab.types import ChatMessage

            messages = []
            if args.system:
                messages.append(ChatMessage(role="system", content=args.system))
            messages.append(ChatMessage(role="user", content=message))
            for chunk in client.stream_completion(
                messages=messages,
                model=args.model,
                temperature=args.temperature,
            ):
                for choice in chunk.choices:
                    if choice.delta.content:
                        sys.stdout.write(choice.delta.content)
                        sys.stdout.flush()
            print()
        else:
            reply = client.chat(
                message,
                model=args.model,
                system=args.system,
                temperature=args.temperature,
            )
            print(reply)


def _cmd_models(args: argparse.Namespace) -> None:
    with _get_client(args) as client:
        models = client.list_models()
        for m in models:
            print(f"  {m.id}" + (f"  (by {m.owned_by})" if m.owned_by else ""))


def _cmd_health(args: argparse.Namespace) -> None:
    with _get_client(args) as client:
        h = client.health()
        print(json.dumps(h.model_dump(), indent=2))


def main() -> None:
    parser = argparse.ArgumentParser(
        prog="edgecrab",
        description="CLI for the EdgeCrab autonomous coding agent",
    )
    parser.add_argument("-V", "--version", action="version", version=f"edgecrab-sdk {__version__}")
    parser.add_argument("--base-url", help="API server URL (env: EDGECRAB_BASE_URL)")
    parser.add_argument("--api-key", help="Bearer token (env: EDGECRAB_API_KEY)")

    sub = parser.add_subparsers(dest="command")

    # ── chat ────────────────────────────────────────────────────────
    chat_p = sub.add_parser("chat", help="Send a message to the agent")
    chat_p.add_argument("message", nargs="+", help="Message text")
    chat_p.add_argument(
        "-m", "--model", default="anthropic/claude-sonnet-4-20250514", help="Model to use"
    )
    chat_p.add_argument("-s", "--system", help="System prompt")
    chat_p.add_argument("-t", "--temperature", type=float, help="Sampling temperature")
    chat_p.add_argument("--stream", action="store_true", help="Stream the response")

    # ── models ──────────────────────────────────────────────────────
    sub.add_parser("models", help="List available models")

    # ── health ──────────────────────────────────────────────────────
    sub.add_parser("health", help="Check API health")

    args = parser.parse_args()
    if not args.command:
        parser.print_help()
        sys.exit(1)

    try:
        {"chat": _cmd_chat, "models": _cmd_models, "health": _cmd_health}[args.command](args)
    except EdgeCrabError as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)
    except KeyboardInterrupt:
        sys.exit(130)


if __name__ == "__main__":
    main()
