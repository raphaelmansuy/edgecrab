# Spec 10: Tool Call Argument Repair

**Priority:** P0 — Critical
**Crate:** `edgecrab-core` (conversation.rs)
**Cross-ref:** [09-assessment-round2.md](09-assessment-round2.md) Gap 1

## Problem

When an LLM emits malformed JSON for tool call arguments, EdgeCrab hard-fails
with `InvalidArgs`. This wastes an API turn ($0.10-0.50) for something a
trivial heuristic can fix locally.

```
+---------------------------------------------------------------+
|                   CURRENT (BROKEN)                            |
+---------------------------------------------------------------+
|                                                               |
|  LLM emits: {"path": "foo.rs", }  (trailing comma)           |
|       |                                                       |
|       v                                                       |
|  serde_json::from_str() → Err                                |
|       |                                                       |
|       v                                                       |
|  ToolError::InvalidArgs → back to LLM → $0.10 wasted         |
|                                                               |
+---------------------------------------------------------------+

+---------------------------------------------------------------+
|                   FIXED (SELF-HEALING)                        |
+---------------------------------------------------------------+
|                                                               |
|  LLM emits: {"path": "foo.rs", }  (trailing comma)           |
|       |                                                       |
|       v                                                       |
|  repair_tool_call_arguments(raw) → {"path": "foo.rs"}        |
|       |                                                       |
|       v                                                       |
|  serde_json::from_str() → Ok → proceed normally              |
|                                                               |
+---------------------------------------------------------------+
```

## Repair Rules (from Hermes Agent `_repair_tool_call_arguments`)

| Input Pattern | Repair | Priority |
|---------------|--------|----------|
| Empty string / null | `"{}"` | 1 |
| Python `None` | `null` | 2 |
| Python `True`/`False` | `true`/`false` | 2 |
| Trailing comma before `}` or `]` | Strip | 3 |
| Truncated JSON (unclosed brackets) | Close brackets | 4 |
| Single quotes → double quotes | Replace (outside strings) | 5 |

## Implementation

**File:** `crates/edgecrab-core/src/conversation.rs`
**Function:** `repair_tool_call_arguments(raw: &str) -> String`
**Call site:** `dispatch_single_tool()` — before `serde_json::from_str`

```rust
fn repair_tool_call_arguments(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "null" || trimmed == "None" {
        return "{}".to_string();
    }
    let mut s = trimmed.to_string();
    // Python booleans
    s = s.replace(": True", ": true")
         .replace(": False", ": false")
         .replace(": None", ": null");
    // Trailing commas
    // regex: ,\s*([}\]])
    static TRAILING_COMMA: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r",\s*([}\]])").unwrap());
    s = TRAILING_COMMA.replace_all(&s, "$1").to_string();
    // Unclosed brackets
    let opens = s.chars().filter(|c| *c == '{').count();
    let closes = s.chars().filter(|c| *c == '}').count();
    for _ in 0..(opens.saturating_sub(closes)) {
        s.push('}');
    }
    s
}
```

## Tests

1. Empty → `"{}"`
2. `"None"` → `"{}"`
3. `{"a": True}` → `{"a": true}`
4. `{"a": 1, }` → `{"a": 1}`
5. `{"a": {"b": 1}` → `{"a": {"b": 1}}`  (close unclosed)
6. Valid JSON → unchanged (passthrough)

## SOLID

- **SRP:** Pure function, single responsibility: repair args.
- **OCP:** New repair rules = new regex/replace lines, no existing code changed.
- **DRY:** Single call site in `dispatch_single_tool`.
