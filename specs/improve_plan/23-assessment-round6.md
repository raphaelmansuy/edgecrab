# EdgeCrab Improvement Plan — Round 6 Assessment

**Date:** 2025-07
**Scope:** FP20 (dropped — Rust type system eliminates it) + FP21 (budget warning history purge)
**Method:** Code-Is-Law gap analysis against Hermes `run_agent.py` and Claude Code
**Status:** FP20 N/A | FP21 IMPLEMENTED

---

## Executive Summary

Round 5 closed FP18 (apply_patch error format) and FP19 (rate-limit retry-after parsing).
Round 6 research surfaced two candidate gaps. After First-Principles analysis:

- **FP20 (surrogate sanitization)**: Python-specific. Rust strings are valid UTF-8 by
  construction; lone surrogates cannot exist in a `String`. `serde_json` rejects invalid UTF-8
  at parse time. **No action required. Gap does not exist in Rust.**

- **FP21 (budget warning history purge)**: Real gap. EdgeCrab injects `_budget_warning` into
  tool-result messages but **never strips them from history** on subsequent turns. Hermes has a
  dedicated `_strip_budget_warnings_from_history()` call. This is implemented in this round.

---

## Gap Analysis Matrix

```
+-------+--------------------------------------------------------------+-----------+---------+
| ID    | Description                                                  | Hermes    | Status  |
+-------+--------------------------------------------------------------+-----------+---------+
| FP20  | Sanitize lone surrogate chars (U+D800-DFFF) before API call  | YES       | N/A     |
|       | WHY N/A: Rust String = valid UTF-8; surrogates impossible    |           | (Rust   |
|       |                                                              |           |  type   |
|       |                                                              |           |  safety)|
+-------+--------------------------------------------------------------+-----------+---------+
| FP21  | Strip _budget_warning fields + [BUDGET/URGENT] text from    | YES       | DONE    |
|       | tool-result and user messages BEFORE each API call          |           | (this   |
|       | WHY: Stale turn-N warnings persist into turns N+1..N+K,     |           |  round) |
|       | causing the LLM to prematurely truncate responses           |           |         |
+-------+--------------------------------------------------------------+-----------+---------+
```

---

## FP20 — Surrogate Sanitization (DROPPED — NOT APPLICABLE)

### Hermes source (run_agent.py)

```python
def _sanitize_messages_surrogates(self, messages):
    """Replace lone surrogate characters with Unicode replacement char."""
    SURROGATE_RE = re.compile(r'[\uD800-\uDFFF]')
    for msg in messages:
        if isinstance(msg.get('content'), str):
            msg['content'] = SURROGATE_RE.sub('\uFFFD', msg['content'])
```

### Why this exists in Python but not Rust

Python `str` objects can contain lone surrogates created by `str.encode('utf-8',
'surrogatepass')` on binary data. The Anthropic API rejects them, causing a 400 error.

In Rust, `String` is defined as valid UTF-8 (RFC 3629). Lone surrogates occupy U+D800–U+DFFF,
which are explicitly **excluded** from valid UTF-8 sequences. The Rust compiler and `serde_json`
both reject them at the type level. Tool output comes in as `String`, which is already clean.

**Verdict: FP20 is a Python-specific defensive measure. Rust's type system provides the
guarantee natively. No code change required.**

---

## FP21 — Budget Warning History Purge (IMPLEMENTED)

### First Principle

> "A budget warning is a turn-scoped signal, not a historical fact.
>  Once the next turn begins, the warning from the previous turn is stale data.
>  Stale signals create wrong behavior — the LLM prematurely wraps up even when
>  it has 40% of its budget remaining."

### The Bug (Code Walk)

```
conversation.rs execute_loop:

Turn N (e.g. api_call_count = 63, max = 90, 70%):
  process_response → LoopAction::Continue
  get_budget_warning(63, 90) → Some("[BUDGET: 70%...wrap up]")
  inject_budget_warning(&mut session.messages, &warning)  ← INJECTED
  publish_session_state
  continue

Turn N+1 (api_call_count = 64):
  sanitize_orphaned_tool_results(...)
  estimate_request_prompt_tokens(...)        ← sees [BUDGET: 70%] from turn 63
  build_chat_messages(...)                   ← sends [BUDGET: 70%] to LLM
  api_call(...)                              ← LLM reads "70% budget used — wrap up"
                                               but actual usage is 71%, not critical yet

Turn N+5 (api_call_count = 68):
  The [BUDGET: 70%] warning from turn 63 STILL in context
  PLUS a new [BUDGET: 75%] from turn 67 STILL in context
  LLM now sees TWO stale warnings AND the current one
  LLM behavior: over-commits to ending early, skips necessary tool calls
```

After compression, these warnings may be baked into the summary as if they're a permanent
attribute of the conversation, further corrupting the LLM's situational awareness.

### Hermes Reference (run_agent.py)

```python
def _strip_budget_warnings_from_history(self, messages):
    """Remove _budget_warning from tool result JSON; remove [BUDGET...] from text."""
    stripped = []
    for msg in messages:
        if msg.get('role') == 'tool':
            content = msg.get('content', '')
            try:
                obj = json.loads(content)
                obj.pop('_budget_warning', None)
                msg = {**msg, 'content': json.dumps(obj)}
            except (json.JSONDecodeError, TypeError):
                # Plain text fallback: strip [BUDGET...] patterns
                msg = {**msg, 'content': re.sub(
                    r'\n\n\[(?:BUDGET|URGENT):.*?\]', '', content
                )}
        stripped.append(msg)
    return stripped
```

Hermes calls this at the top of every loop iteration before building the API payload.

### EdgeCrab Implementation

```rust
/// Strip stale budget-warning annotations from message history.
///
/// WHY: `inject_budget_warning()` appends a `_budget_warning` key to the last
/// tool-result JSON (or plain text) at the END of each tool turn. That warning
/// is only valid for the current turn — it signals "you have N% of iterations
/// left RIGHT NOW". On subsequent turns the warning is stale data:
///   - The LLM sees "70% used — wrap up" even though it's now at 72% or 85%.
///   - If two turns both injected warnings, the LLM sees conflicting signals.
///   - Compression can bake stale warnings into summaries.
///
/// We strip ALL injected warnings at the top of each loop iteration, before
/// compression and before building the API payload. The only budget signal
/// the LLM ever sees is the one freshly injected at the END of the current turn.
///
/// Cross-ref: Hermes `_strip_budget_warnings_from_history()` in `run_agent.py`.
///
/// Handled cases:
///   1. Tool-role message with JSON content: remove `_budget_warning` key.
///   2. Tool-role message with plain text: strip `\n\n[BUDGET...]` / `\n\n[URGENT...]`.
///   3. User-role message that IS only a budget warning (injected as fallback
///      when there are no tool messages): remove the message entirely.
fn strip_budget_warnings_from_history(messages: &mut Vec<Message>) { ... }
```

Wire location: Immediately after `sanitize_orphaned_tool_results`, before compression:

```rust
// Line 1356 (existing)
sanitize_orphaned_tool_results(&mut session.messages);

// NEW — FP21: strip stale budget warnings before compression and API payload build
strip_budget_warnings_from_history(&mut session.messages);

// Line 1358 (existing)
let compression_params = ...
```

### DRY / SOLID Analysis

- **SRP**: `strip_budget_warnings_from_history()` does one thing — remove stale turn-scoped
  annotations. It does not touch other message fields.
- **OCP**: The strip function handles all three injection shapes (JSON key, plain text suffix,
  standalone user message). Adding a new injection shape only requires extending this function.
- **DRY**: The detection logic (look for `_budget_warning` JSON key or `[BUDGET/URGENT]` prefix)
  mirrors the injection logic in `inject_budget_warning()`, keeping them in sync.

### Edge Cases

| Case | Behavior |
|------|----------|
| No budget warnings in history | No-op (fast path: JSON parse skipped on clean messages) |
| Multiple stale warnings stacked | All removed |
| Warning injected this iteration | Stripped at TOP of NEXT iteration only; current iteration's API call sees it |
| Compression after strip | Compression summary never includes stale warnings |
| Plain text tool result with suffix | Regex strip removes `\n\n[BUDGET...]` suffix cleanly |
| User-only message that is pure budget warning | Message removed entirely from history |
| Non-budget user message | Untouched |

### Tests Added

```
test_strip_budget_warnings_strips_json_key
test_strip_budget_warnings_strips_plain_text_suffix
test_strip_budget_warnings_removes_standalone_user_message
test_strip_budget_warnings_noop_on_clean_history
test_strip_budget_warnings_multiple_stale_warnings
```

---

## Summary

| Round | Feature | File | Lines |
|-------|---------|------|-------|
| R6    | FP21: strip_budget_warnings_from_history | conversation.rs | ~20 |

FP20 not implemented (Rust type safety = free guarantee).

Cross-refs:
- [Hermes `run_agent.py` L200-400](../../../hermes-agent/run_agent.py)
- [EdgeCrab conversation.rs L1346-1360](../../../crates/edgecrab-core/src/conversation.rs)
- [specs/improve_plan/README.md](README.md)
