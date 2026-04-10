import json


def _load_document(content):
    return json.loads(str(content))


def _json_kind(value):
    if isinstance(value, dict):
        return "object"
    if isinstance(value, list):
        return "array"
    if isinstance(value, str):
        return "string"
    if isinstance(value, bool):
        return "boolean"
    if value is None:
        return "null"
    return "number"


def _decode_pointer(pointer):
    if pointer == "":
        return []
    if not pointer.startswith("/"):
        raise ValueError("pointer must be empty or start with '/'")
    return [
        token.replace("~1", "/").replace("~0", "~")
        for token in pointer.lstrip("/").split("/")
    ]


def _resolve_pointer(document, pointer):
    current = document
    for token in _decode_pointer(pointer):
        if isinstance(current, list):
            try:
                index = int(token)
            except ValueError as exc:
                raise ValueError(f"pointer token '{token}' is not a list index") from exc
            try:
                current = current[index]
            except IndexError as exc:
                raise ValueError(f"list index out of bounds: {index}") from exc
            continue
        if isinstance(current, dict):
            if token not in current:
                raise ValueError(f"missing object key: {token}")
            current = current[token]
            continue
        raise ValueError(f"cannot descend into {_json_kind(current)} with token '{token}'")
    return current


def json_validate(args, **kwargs):
    content = args.get("content", "")
    pretty = bool(args.get("pretty", False))
    try:
        document = _load_document(content)
        payload = {
            "ok": True,
            "kind": _json_kind(document),
            "valid": True,
        }
        if pretty:
            payload["pretty"] = json.dumps(document, indent=2, sort_keys=True)
        if isinstance(document, dict):
            payload["keys"] = sorted(document.keys())
        return json.dumps(payload)
    except Exception as exc:
        return json.dumps({"ok": False, "valid": False, "error": str(exc)})


def json_pointer_get(args, **kwargs):
    content = args.get("content", "")
    pointer = str(args.get("pointer", ""))
    try:
        document = _load_document(content)
        value = _resolve_pointer(document, pointer)
        return json.dumps({"ok": True, "pointer": pointer, "value": value})
    except Exception as exc:
        return json.dumps({"ok": False, "pointer": pointer, "error": str(exc)})
