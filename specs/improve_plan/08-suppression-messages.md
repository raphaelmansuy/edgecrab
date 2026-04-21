# 08 — P3: Actionable Suppression Feedback

**Priority**: P3
**Impact**: LLM self-corrects faster after suppressed retries
**Risk**: Low — changes message text only, no logic changes
**Cross-ref**: [01-diagnosis.md](01-diagnosis.md) RC-4

## WHY

```
CURRENT SUPPRESSION MESSAGE:
    "Tool call '{name}' was suppressed to avoid an identical retry that
     would likely fail again. Correct the JSON arguments or use a
     different approach."

    Problem: LLM doesn't know WHAT to correct or WHAT approach to try.
    Result: LLM makes another wrong guess -> another suppression.

TARGET SUPPRESSION MESSAGE:
    "Tool call '{name}' was suppressed (identical to a previous failed call).
     Previous error: {original_error}
     To fix: {suggested_action}
     Alternative tools: {alternatives}"

    Result: LLM has specific guidance to self-correct.
```

## Implementation

### File: `crates/edgecrab-core/src/conversation.rs`

#### Change 1: Store original error in suppression state

```rust
// Current suppression state stores only fingerprint + key
// Add original_error field:
struct ToolSuppression {
    fingerprint: String,
    suppression_key: String,
    original_error: String,       // NEW
    tool_name: String,            // NEW
    suggested_action: Option<String>, // NEW
}
```

#### Change 2: remember_tool_suppression takes error context

```rust
fn remember_tool_suppression(
    state: &mut SuppressedToolState,
    fingerprint: String,
    suppression_key: String,
    original_error: &str,         // NEW
    tool_name: &str,              // NEW
    suggested_action: Option<&str>, // NEW
) {
    state.insert(fingerprint.clone(), ToolSuppression {
        fingerprint,
        suppression_key: suppression_key.clone(),
        original_error: original_error.to_string(),
        tool_name: tool_name.to_string(),
        suggested_action: suggested_action.map(String::from),
    });
    state.insert(suppression_key, ToolSuppression { /* broader key */ });
}
```

#### Change 3: Enriched suppression response

```rust
fn suppressed_retry_response(
    tool_name: &str,
    suppression: &ToolSuppression,
) -> String {
    let mut msg = format!(
        "Tool call '{}' was suppressed (identical to a previously failed call).\n",
        tool_name,
    );
    if !suppression.original_error.is_empty() {
        msg.push_str(&format!("Previous error: {}\n", suppression.original_error));
    }
    if let Some(action) = &suppression.suggested_action {
        msg.push_str(&format!("Suggested fix: {}\n", action));
    }
    msg.push_str("Please change the arguments or use a different tool.");
    msg
}
```

## Edge Cases

1. **Suppression without prior error**: When suppressed by broader key (not
   exact fingerprint), `original_error` may not match exactly. The message
   still provides useful context.

2. **Long error messages**: Truncate `original_error` to 500 chars to avoid
   bloating the message context.

3. **suggested_action is None**: Falls back to generic "change arguments"
   message (graceful degradation).

4. **Multiple suppressions**: Each suppression entry stores its own error
   context. No cross-contamination between different tools.

5. **Memory overhead**: ToolSuppression is short-lived (cleared between turns
   or after successful calls). No memory leak risk.
