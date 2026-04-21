# 04 — P1: Actionable Error Guidance

**Priority**: P1 (second highest leverage)
**Impact**: Self-healing loop — LLM corrects itself on first retry
**Risk**: Low — additive change, no behavior removed
**Cross-ref**: [01-diagnosis.md](01-diagnosis.md) RC-3, [02-hermes-patterns.md](02-hermes-patterns.md) Pattern 5

## WHY This Is P1

When a tool receives invalid arguments, the current error is:

```
InvalidArgs { tool: "terminal", message: "missing field `command`" }
```

This tells the LLM WHAT failed, but not HOW to fix it. The LLM must
reconstruct the schema from memory (unreliable with 77+ schemas).

```
BEFORE:
+---------------------------+
| Error: missing "command"  | --> LLM guesses schema
+---------------------------+     --> wrong guess 40% of time
                                  --> 2-3 retries wasted

AFTER:
+---------------------------+
| Error: missing "command"  |
| Required: ["command"]     | --> LLM sees exact fields needed
| Example: {"command":"ls"} | --> copy-paste correct structure
+---------------------------+     --> fixed on first retry
```

## Implementation: Enhanced ToolError::InvalidArgs

### File: `crates/edgecrab-types/src/error.rs`

Add `required_fields` and `usage_hint` to InvalidArgs:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolError {
    InvalidArgs {
        tool: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        required_fields: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage_hint: Option<String>,
    },
    // ... other variants unchanged
}
```

### File: `crates/edgecrab-types/src/error.rs` — suggested_action

```rust
fn suggested_action(&self) -> Option<String> {
    match self {
        ToolError::InvalidArgs { tool, message, required_fields, usage_hint } => {
            let mut parts = vec![format!("Fix the arguments for '{tool}': {message}")];
            if let Some(fields) = required_fields {
                parts.push(format!("Required fields: {:?}", fields));
            }
            if let Some(hint) = usage_hint {
                parts.push(format!("Example: {hint}"));
            }
            Some(parts.join(". "))
        },
        // ... other variants as before
    }
}
```

### File: `crates/edgecrab-tools/src/registry.rs` — dispatch enrichment

When dispatch catches a deserialization error, extract the schema's
`required` fields and build a usage hint from the schema:

```rust
// In ToolRegistry::dispatch() error handling:
Err(e) => {
    let schema = self.get_schema(name);
    let required = schema
        .and_then(|s| s.parameters.get("required"))
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());
    let usage_hint = schema.map(|s| build_usage_hint(&s.parameters));
    Err(ToolError::InvalidArgs {
        tool: name.into(),
        message: e.to_string(),
        required_fields: required,
        usage_hint,
    })
}
```

## Edge Cases

1. **Schema without "required"**: `required_fields` is None → no extra info
2. **Complex nested schemas**: `usage_hint` only shows top-level properties
3. **Tool not in registry**: Schema lookup returns None → basic error only
4. **Existing callers of InvalidArgs**: Must update all call sites to add
   `required_fields: None, usage_hint: None` (backward compat)
5. **Serialization size**: Limited to top-level fields only — no deep nesting
