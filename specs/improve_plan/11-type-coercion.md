# Spec 11: Argument Type Coercion

**Priority:** P1 — High
**Crate:** `edgecrab-tools` (registry.rs)
**Cross-ref:** [09-assessment-round2.md](09-assessment-round2.md) Gap 2

## Problem

LLMs frequently send `"42"` (string) when the schema says `integer`, or
`"true"` (string) when the schema says `boolean`. EdgeCrab passes raw args
to `serde_json::from_value::<MyArgs>()` which fails → `InvalidArgs` → wasted
turn.

```
+---------------------------------------------------------------+
|                   CURRENT (BROKEN)                            |
+---------------------------------------------------------------+
|                                                               |
|  Schema: { "line": { "type": "integer" } }                   |
|  LLM sends: { "line": "42" }                                 |
|       |                                                       |
|       v                                                       |
|  from_value::<Args>() → Err("expected integer, got string")  |
|       |                                                       |
|       v                                                       |
|  InvalidArgs → back to LLM → $0.10 wasted                    |
|                                                               |
+---------------------------------------------------------------+

+---------------------------------------------------------------+
|                   FIXED (TYPE COERCION)                       |
+---------------------------------------------------------------+
|                                                               |
|  Schema: { "line": { "type": "integer" } }                   |
|  LLM sends: { "line": "42" }                                 |
|       |                                                       |
|       v                                                       |
|  coerce_tool_args(args, schema) → { "line": 42 }             |
|       |                                                       |
|       v                                                       |
|  from_value::<Args>() → Ok → proceed normally                |
|                                                               |
+---------------------------------------------------------------+
```

## Coercion Rules (from Hermes Agent `coerce_tool_args`)

| Schema Type | Actual Value | Coercion |
|-------------|-------------|----------|
| `integer` | `"42"` (parseable string) | → `42` |
| `number` | `"3.14"` (parseable string) | → `3.14` |
| `boolean` | `"true"` / `"false"` | → `true` / `false` |
| `boolean` | `"1"` / `"0"` | → `true` / `false` |
| `array` | single non-array value | → `[value]` (wrap) |
| `string` | number/boolean | → `"42"` / `"true"` (stringify) |

## Implementation

**File:** `crates/edgecrab-tools/src/registry.rs`
**Function:** `coerce_tool_args(args: &mut Value, schema: &Value)`
**Call site:** `ToolRegistry::dispatch()` — after successful parse, before handler

```rust
pub fn coerce_tool_args(args: &mut Value, schema: &Value) {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return;
    };
    let Value::Object(ref mut map) = args else {
        return;
    };
    for (key, prop_schema) in properties {
        let Some(value) = map.get_mut(key) else { continue };
        let Some(expected_type) = prop_schema.get("type").and_then(Value::as_str) else {
            continue;
        };
        match expected_type {
            "integer" if value.is_string() => {
                if let Some(s) = value.as_str() {
                    if let Ok(n) = s.parse::<i64>() {
                        *value = Value::Number(n.into());
                    }
                }
            }
            "number" if value.is_string() => {
                if let Some(s) = value.as_str() {
                    if let Ok(n) = s.parse::<f64>() {
                        if let Some(n) = serde_json::Number::from_f64(n) {
                            *value = Value::Number(n);
                        }
                    }
                }
            }
            "boolean" if value.is_string() => {
                match value.as_str() {
                    Some("true" | "1") => *value = Value::Bool(true),
                    Some("false" | "0") => *value = Value::Bool(false),
                    _ => {}
                }
            }
            "string" if !value.is_string() => {
                *value = Value::String(value.to_string());
            }
            "array" if !value.is_array() => {
                let v = value.take();
                *value = Value::Array(vec![v]);
            }
            _ => {}
        }
    }
}
```

## Tests

1. `{"line": "42"}` + schema integer → `{"line": 42}`
2. `{"flag": "true"}` + schema boolean → `{"flag": true}`
3. `{"val": "3.14"}` + schema number → `{"val": 3.14}`
4. `{"name": 42}` + schema string → `{"name": "42"}`
5. `{"tags": "foo"}` + schema array → `{"tags": ["foo"]}`
6. Already correct type → unchanged

## SOLID / DRY

- **SRP:** Single function, single responsibility: type coercion.
- **OCP:** New type rules = new match arm, no existing code changed.
- **DRY:** Called once in `dispatch()` before handler invocation.
