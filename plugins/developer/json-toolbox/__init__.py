from . import schemas, tools
from .cli import handle_json_toolbox, setup_json_toolbox_cli


def register(ctx):
    ctx.register_tool("json_validate", schemas.JSON_VALIDATE, tools.json_validate)
    ctx.register_tool("json_pointer_get", schemas.JSON_POINTER_GET, tools.json_pointer_get)
    ctx.register_cli_command(
        name="json-toolbox",
        help="Validate and pretty-print JSON files",
        setup_fn=setup_json_toolbox_cli,
        handler_fn=handle_json_toolbox,
    )
