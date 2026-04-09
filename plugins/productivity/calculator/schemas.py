CALCULATE = {
    "name": "calculate",
    "description": "Evaluate a safe arithmetic expression.",
    "parameters": {
        "type": "object",
        "properties": {
            "expression": {
                "type": "string",
                "description": "Arithmetic expression using numbers and + - * / // % **",
            }
        },
        "required": ["expression"],
        "additionalProperties": False,
    },
}

UNIT_CONVERT = {
    "name": "unit_convert",
    "description": "Convert a numeric value between supported units.",
    "parameters": {
        "type": "object",
        "properties": {
            "value": {
                "type": "number",
                "description": "Numeric value to convert.",
            },
            "from_unit": {
                "type": "string",
                "description": "Source unit such as km, m, cm, mm, or mi.",
            },
            "to_unit": {
                "type": "string",
                "description": "Destination unit such as km, m, cm, mm, or mi.",
            },
        },
        "required": ["value", "from_unit", "to_unit"],
        "additionalProperties": False,
    },
}
