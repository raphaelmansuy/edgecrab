# 22 — Round 5 Assessment: "Code Is Law" Live Evidence

> EdgeCrab vs Hermes Agent vs Claude Code.
> Evidence: live screenshot session, gpt-5-nano, 66.5k/128k context, turn 1.
> Date: 2026-04-21 | Branch: feat/agent-harness-next-release

---

## Live Evidence from Screenshot

```
+----------------------------------------------------------------------+
|  OBSERVED BEHAVIOUR (gpt-5-nano, turn 1, 52% context)               |
+----------------------------------------------------------------------+
|                                                                      |
|  1. patch      audit_quanta.md                                       |
|     x apply_patch failed and was rolled back. Errors:               |
|       [TUI shows nothing after "Errors:" — body on next line]        |
|                                                                      |
|     patch: 1 file(s) · +0 -0 hunks                                  |
|                                                                      |
|  2. result  apply_patch failed and was rolled back. Errors:          |
|     [EMPTY]  ← first-line-only TUI display truncation                |
|                                                                      |
|  3. Run failed                                                        |
|     LLM API error: API call failed after 3 retries:                  |
|     Rate limit exceeded: tokens: Rate limit reached for gpt-5-nano   |
|     TPM: Limit 200000, Used 172647, Requested 31344.                 |
|     Please try again in 1.197s.                                      |
|                                                                      |
+----------------------------------------------------------------------+
```

Two distinct bugs surfaced:

### Bug A: apply_patch Error Truncation in TUI

```
CURRENT format:
    "apply_patch failed and was rolled back. Errors:\n{}"
                                                    ^^^
    Body starts AFTER \n → TUI shows only first line → blank after colon

FIX:
    "apply_patch failed and was rolled back. {n} error(s): {first}; {rest...}"
    OR: join with "; " instead of "\n"
    => error fully visible on one line in TUI activity feed
```

Severity: **P1** — User sees empty error. Model sees empty error. Loop continues.

### Bug B: TPM Rate Limit → Opaque "API call failed" after 3 retries

```
CURRENT:
    Error bubbles up as ToolError::Other("LLM API error: API call failed ...")
    → no guidance on when to retry
    → no distinction between "rate limit" vs "auth failure" vs "server error"
    → 3 retries with BASE_BACKOFF=500ms, but TPM needs 1.197s
    → Run fails immediately

HERMES PATTERN (provider_fallback.py):
    - Detect rate limit HTTP 429 or RateLimitError subclass
    - Extract "please try again in X.Ys" from error text
    - Wait X.Y + 1s jitter, then retry same provider
    - After N rate-limit retries, fall back to fallback_model
    - Surface: "Rate limited. Waiting X.Ys before retry."

FIX:
    1. Parse "try again in X.Ys" from rate limit error message
    2. If wait_secs < 10s → sleep and retry (no provider switch)
    3. If wait_secs >= 10s OR on_repeat → use fallback_model
    4. Error message → "Rate limited (gpt-5-nano). Waiting 2s. Tokens: 31344 requested, 27353 available."
```

Severity: **P0** — Run fails completely on first rate-limit hit. Budget wasted.

---

## Round 5 Gap Matrix

```
+-----+--------------------------------------------+----------+---------+----------+
| #   | Pattern                                    | Hermes   | Claude  | EdgeCrab |
|     |                                            | Agent    | Code    |          |
+-----+--------------------------------------------+----------+---------+----------+
|     | === ALREADY IMPLEMENTED (Rounds 1-4) ===   |          |         |          |
+-----+--------------------------------------------+----------+---------+----------+
|  1  | FP1-FP12 all implemented                   | A-       | A-      | A        |
| 16  | FP16: Configurable write limit              | N/A      | N/A     | A        |
| 15  | FP15: Memory injection scan                 | A        | A       | A        |
+-----+--------------------------------------------+----------+---------+----------+
|     | === ROUND 5 BUGS ===                        |          |         |          |
+-----+--------------------------------------------+----------+---------+----------+
| B-A | apply_patch error body truncated in TUI    | N/A (py) | N/A     | FAIL     |
|     | (multi-line error, TUI shows first only)    |          |         |          |
+-----+--------------------------------------------+----------+---------+----------+
| B-B | TPM rate limit → opaque failure            | PASS     | PARTIAL | FAIL     |
|     | (parse wait_secs, retry, fallback)          | (full)   | (basic) | (none)   |
+-----+--------------------------------------------+----------+---------+----------+
|     | === ROUND 4 PENDING ===                     |          |         |          |
+-----+--------------------------------------------+----------+---------+----------+
| G11 | FP13: Read dedup cache                     | A        | B+      | PENDING  |
|     | (mtime dedup + consecutive loop block)      | (full)   | (hash)  | (partial)|
+-----+--------------------------------------------+----------+---------+----------+
| G16 | FP17: Compression dedup reset              | A        | N/A     | PENDING  |
|     | (clear read cache after compression)        |          |         |          |
+-----+--------------------------------------------+----------+---------+----------+
| G13 | FP14: Schema cross-ref filtering           | A        | N/A     | PENDING  |
|     | (strip refs to unavailable tools)            |          |         |          |
+-----+--------------------------------------------+----------+---------+----------+
```

---

## Bug B: Rate Limit — First Principles Analysis

```
+----------------------------------------------------------------------+
|  WHY RATE LIMITS HIT HARD                                            |
+----------------------------------------------------------------------+
|                                                                      |
|  GPT-5-nano TPM limit: 200,000                                       |
|  At turn 1: 66.5k tokens already loaded (system prompt + tools)      |
|  Tool call adds: 31,344 tokens                                        |
|  Total session: 172,647 tokens used in first few turns               |
|                                                                      |
|  RESULT: TPM exhausted in < 1 minute of real-time                    |
|                                                                      |
|  Provider says: "please try again in 1.197s"                         |
|  EdgeCrab does: 3 retries * 500ms backoff → FAIL after 1.5s          |
|  FIX: Parse "try again in X.Ys" → wait exactly that long             |
|                                                                      |
+----------------------------------------------------------------------+
|                                                                      |
|  FP19 (NEW): MATCH RETRY DELAY TO PROVIDER GUIDANCE                  |
|              "Providers tell you when to retry — listen to them"     |
|              => Parse "try again in X.Ys" from error message         |
|              => Use that delay instead of fixed 500ms backoff        |
|              => Cross-ref: OpenAI Ratelimit-Reset-Requests header    |
|                                                                      |
+----------------------------------------------------------------------+
```

---

## First Principles Stack (Round 5 Additions)

```
+----------------------------------------------------------------------+
|  FP18: Single-Line Error Surfaces Truth                              |
|        "A truncated error is a silent error"                         |
|        => Multi-line error bodies must be either:                    |
|           (a) first-line summary + detail body, OR                   |
|           (b) semicolon-joined single-line for TUI activity          |
|        => apply_patch: use "; " join not "\n" in summary             |
|                                                                      |
|  FP19: Retry Delay = Provider Guidance, Not Guess                   |
|        "If the provider says wait 1.197s, wait 1.2s not 500ms"      |
|        => Parse "try again in X.Ys" from rate-limit errors           |
|        => Use parsed wait + jitter as actual sleep duration          |
|        => Cross-ref: Hermes provider_fallback.py parse_wait_secs()   |
|                                                                      |
+----------------------------------------------------------------------+
```

---

## SOLID Audit: Round 5

| Principle | Violation | Fix |
|-----------|-----------|-----|
| **S**RP | `MAX_RETRIES` in conversation.rs handles both network errors AND rate limits | Split retry logic: network errors = 3× BASE_BACKOFF; rate limits = parse delay |
| **O**CP | Hard-coded `BASE_BACKOFF = 500ms` | Inject from error-parsed delay when available |
| **L**SP | `ToolError::Other` used for both "rate limit" and "file not found" | `ToolError` variants are already correct — issue is in API retry logic |
| **I**SP | N/A this round | — |
| **D**IP | `BASE_BACKOFF` constant coupled to retry loop | Rate limit delay should come from error payload |

---

## DRY Violations Found

| Location | Duplication | Fix |
|----------|------------|-----|
| `file_patch.rs` × 3 | `"apply_patch failed and was rolled back. Errors:\n{}"` at 3 call sites | Extract `format_patch_error(errors: &[String]) -> String` helper |
| `conversation.rs` | Same retry loop handles both TPM and server errors with fixed delay | Extract `parse_retry_after(err: &str) -> Option<Duration>` |

---

## Implementation Specs for Round 5

See:
- [Bug A fix](#) → file_patch.rs error format (in this doc, below)
- [Bug B fix — FP19](#) → conversation.rs retry logic  
- FP13 → [18-read-dedup-loop.md](18-read-dedup-loop.md) (pending)
- FP17 → coupled with FP13
- FP14 → [19-schema-cross-ref.md](19-schema-cross-ref.md) (pending)

---

## file_patch.rs Error Format Fix (Bug A)

```
BEFORE:
    Err(ToolError::Other(format!(
        "apply_patch failed and was rolled back. Errors:\n{}",
        errors.join("\n")
    )))

AFTER:
    Err(ToolError::Other(format_patch_errors(&errors)))

where:
    fn format_patch_errors(errors: &[String]) -> String {
        if errors.is_empty() {
            "apply_patch failed and was rolled back. (no error detail)".into()
        } else if errors.len() == 1 {
            format!("apply_patch failed and was rolled back: {}", errors[0])
        } else {
            format!(
                "apply_patch failed and was rolled back ({} errors): {}",
                errors.len(),
                errors.join("; ")
            )
        }
    }
```

WHY: TUI activity feed shows only the first line of a tool result.
Multi-line errors with `\n` mean the user and model see `"Errors:"` but no error details.
Semicolon-joined errors fit on one visible line in both TUI and LLM context.

---

## FP19: Rate-Limit Retry Delay (conversation.rs)

```rust
/// Parse "please try again in X.Ys" from rate-limit error messages.
fn parse_retry_after(err_str: &str) -> Option<Duration> {
    // OpenAI: "Please try again in 1.197s"
    // Anthropic: "retry after 2s"
    let re = regex::Regex::new(r"(?i)(?:try again|retry) (?:after |in )(\d+\.?\d*)s").ok()?;
    let caps = re.captures(err_str)?;
    let secs: f64 = caps[1].parse().ok()?;
    Some(Duration::from_millis((secs * 1000.0 + 200.0) as u64)) // +200ms jitter
}

// In retry loop:
let sleep_duration = if is_rate_limit(&err) {
    parse_retry_after(&err.to_string())
        .unwrap_or(BASE_BACKOFF * 2u32.pow(attempt))
} else {
    BASE_BACKOFF * 2u32.pow(attempt) // exponential for non-rate-limit
};
```
