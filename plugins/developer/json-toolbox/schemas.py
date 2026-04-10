JSON_VALIDATE = {
    "name": "json_validate",
    "description": "Validate JSON text and optionally return pretty-printed output.",
    "parameters": {
        "type": "object",
        "properties": {
            "content": {
                "type": "string",
                "description": "Raw JSON document to validate.",
            },
            "pretty": {
                "type": "boolean",
                "description": "When true, return normalized pretty JSON.",
            },
        },
        "required": ["content"],
        "additionalProperties": False,
    },
}

JSON_POINTER_GET = {
    "name": "json_pointer_get",
    "description": "Resolve a RFC 6901 JSON Pointer against a JSON document.",
    "parameters": {
        "type": "object",
        "properties": {
            "content": {
                "type": "string",
                "description": "Raw JSON document to inspect.",
            },
            "pointer": {
                "type": "string",
                "description": "Pointer such as /meta/name or /items/0/id.",
            },
        },
        "required": ["content", "pointer"],
        "additionalProperties": False,
    },
}
