import ast
import json
from pathlib import Path

_BIN_OPS = {
    ast.Add: lambda left, right: left + right,
    ast.Sub: lambda left, right: left - right,
    ast.Mult: lambda left, right: left * right,
    ast.Div: lambda left, right: left / right,
    ast.FloorDiv: lambda left, right: left // right,
    ast.Mod: lambda left, right: left % right,
    ast.Pow: lambda left, right: left**right,
}

_UNARY_OPS = {
    ast.UAdd: lambda value: value,
    ast.USub: lambda value: -value,
}


def _is_number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def _normalize_result(value):
    if isinstance(value, float) and value.is_integer():
        return int(value)
    return value


def _evaluate(node):
    if isinstance(node, ast.Expression):
        return _evaluate(node.body)
    if isinstance(node, ast.Constant) and _is_number(node.value):
        return node.value
    if isinstance(node, ast.Num) and _is_number(node.n):
        return node.n
    if isinstance(node, ast.BinOp) and type(node.op) in _BIN_OPS:
        left = _evaluate(node.left)
        right = _evaluate(node.right)
        return _BIN_OPS[type(node.op)](left, right)
    if isinstance(node, ast.UnaryOp) and type(node.op) in _UNARY_OPS:
        return _UNARY_OPS[type(node.op)](_evaluate(node.operand))
    raise ValueError(f"Unsupported expression element: {type(node).__name__}")


def _units():
    path = Path(__file__).with_name("data") / "units.json"
    return json.loads(path.read_text(encoding="utf-8"))


def calculate(args, **kwargs):
    expression = str(args.get("expression", "")).strip()
    if not expression:
        return json.dumps({"ok": False, "error": "expression is required"})

    try:
        tree = ast.parse(expression, mode="eval")
        result = _normalize_result(_evaluate(tree))
        return json.dumps({"ok": True, "expression": expression, "result": result})
    except ZeroDivisionError:
        return json.dumps({"ok": False, "expression": expression, "error": "division by zero"})
    except Exception as exc:
        return json.dumps({"ok": False, "expression": expression, "error": str(exc)})


def unit_convert(args, **kwargs):
    if args.get("value") is None:
        return json.dumps({"ok": False, "error": "value is required"})

    from_unit = str(args.get("from_unit", "")).strip().lower()
    to_unit = str(args.get("to_unit", "")).strip().lower()
    if not from_unit or not to_unit:
        return json.dumps({"ok": False, "error": "from_unit and to_unit are required"})

    units = _units()
    if from_unit not in units or to_unit not in units:
        return json.dumps(
            {
                "ok": False,
                "error": f"unsupported conversion: {from_unit} -> {to_unit}",
                "supported_units": sorted(units.keys()),
            }
        )

    value = float(args["value"])
    base_value = value * float(units[from_unit])
    converted = base_value / float(units[to_unit])
    normalized = _normalize_result(round(converted, 6))
    return json.dumps(
        {
            "ok": True,
            "input": {"value": value, "unit": from_unit},
            "output": {"value": normalized, "unit": to_unit},
        }
    )
