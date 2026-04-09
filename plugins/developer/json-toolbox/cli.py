import json
import sys
from pathlib import Path


def _read_text(path):
    if path == "-":
        return sys.stdin.read()
    return Path(path).read_text(encoding="utf-8")


def _handle_pretty(args):
    content = _read_text(args.path)
    document = json.loads(content)
    print(json.dumps(document, indent=2, sort_keys=True))
    return 0


def _handle_validate(args):
    try:
        json.loads(_read_text(args.path))
        print("valid")
        return 0
    except Exception as exc:
        print(f"invalid: {exc}")
        return 1


def setup_json_toolbox_cli(parser):
    subcommands = parser.add_subparsers(dest="subcommand")

    pretty = subcommands.add_parser("pretty", help="Pretty-print JSON from a file or stdin")
    pretty.add_argument("path", help="Path to a JSON file, or '-' for stdin")
    pretty.set_defaults(func=_handle_pretty)

    validate = subcommands.add_parser("validate", help="Validate JSON from a file or stdin")
    validate.add_argument("path", help="Path to a JSON file, or '-' for stdin")
    validate.set_defaults(func=_handle_validate)


def handle_json_toolbox(args):
    handler = getattr(args, "func", None)
    if handler is None:
        print("usage: edgecrab json-toolbox <pretty|validate> <path|- >")
        return 1
    return handler(args)
