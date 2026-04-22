# Shadow Judge — Concrete Implementation Plan

Cross-ref: [07-adr-shadow-judge.md](07-adr-shadow-judge.md),
[06-shadow-judge-critique-and-concept.md](06-shadow-judge-critique-and-concept.md)

---

## Overview

This document specifies every code change required to implement the shadow judge.
All design constraints from ADR 007 are enforced by the implementation.

| Change | File | Status |
|--------|------|--------|
| New config struct `ShadowJudgeConfig` | `config.rs` | Not started |
| `shadow_judge` field in `AppConfig` | `config.rs` | Not started |
| `shadow_judge` field in `AgentConfig` | `agent.rs` | Not started |
| `to_agent_config` projection | `agent.rs` | Not started |
| New module `shadow_judge.rs` | `shadow_judge.rs` | Not started |
| `mod shadow_judge` declaration | `lib.rs` | Not started |
| Integration in `execute_loop` | `conversation.rs` | Not started |
| Unit tests | `shadow_judge.rs` | Not started |

---

## 1. Config Changes (`crates/edgecrab-core/src/config.rs`)

### 1.1 Add `ShadowJudgeConfig` struct

Insert after the existing `AuxiliaryConfig` struct (≈ line 2001):

```rust
/// Shadow judge configuration — lightweight LLM completion oracle.
///
/// When enabled, the shadow judge fires after the synchronous
/// `DefaultCompletionPolicy` returns `Completed`. It makes a single
/// LLM classification call to verify that the original user request is
/// actually satisfied before allowing the loop to break.
///
/// Default: all fields produce a safe disabled state.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ShadowJudgeConfig {
    /// Enable shadow judge. Default: false (opt-in).
    pub enabled: bool,
    /// Judge model (e.g. "anthropic/claude-haiku-4-20250514").
    /// null → use auxiliary.model → use main model.
    pub model: Option<String>,
    /// Judge provider name override.
    /// null → inferred from model prefix.
    pub provider: Option<String>,
    /// Hard cap on shadow judge invocations per session.
    /// Prevents infinite correction loops. Default: 5.
    pub max_per_session: u32,
    /// If judge confidence < this threshold, treat verdict as "complete".
    /// Range [0.0, 1.0]. Default: 0.70.
    pub confidence_threshold: f32,
    /// Number of most-recent messages to pass to the judge.
    /// 0 = send all messages (caution: more tokens). Default: 20.
    pub context_messages: usize,
    /// Minimum conversation length before judge is eligible to fire.
    /// Prevents single-turn Q&A sessions from being judged. Default: 4.
    pub min_messages_before_enable: usize,
}

impl Default for ShadowJudgeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: None,
            provider: None,
            max_per_session: 5,
            confidence_threshold: 0.70,
            context_messages: 20,
            min_messages_before_enable: 4,
        }
    }
}
```

### 1.2 Add `shadow_judge` field to `AppConfig`

In the `AppConfig` struct (≈ line 63), add after `auxiliary`:

```rust
    pub shadow_judge: ShadowJudgeConfig,
```

The `#[serde(default)]` on `AppConfig` ensures backward compatibility — existing
`config.yaml` files without a `shadow_judge` section load the `Default` (disabled).

---

## 2. Agent Config Changes (`crates/edgecrab-core/src/agent.rs`)

### 2.1 Add `shadow_judge` field to `AgentConfig`

In `AgentConfig` struct (≈ line 132), add after the `auxiliary` field:

```rust
    /// Shadow judge configuration projected from AppConfig.
    pub shadow_judge: crate::config::ShadowJudgeConfig,
```

### 2.2 Add default for `shadow_judge` in `AgentConfig::default()`

In the `Default` impl for `AgentConfig`:

```rust
    shadow_judge: crate::config::ShadowJudgeConfig::default(),
```

### 2.3 Project `shadow_judge` in `AppConfig::to_agent_config()`

In the `to_agent_config` method (the large projection block, ≈ line 370), add:

```rust
            shadow_judge: self.shadow_judge.clone(),
```

---

## 3. New Module: `shadow_judge.rs`

Create `crates/edgecrab-core/src/shadow_judge.rs`:

```rust
//! # Shadow Judge — Lightweight LLM Completion Oracle
//!
//! Fires after the synchronous `DefaultCompletionPolicy` returns `Completed`.
//! Makes a single isolated LLM classification call to verify that the original
//! user request is fully satisfied before the main loop breaks.
//!
//! ## Session isolation guarantee
//!
//! The shadow judge:
//! 1. CLONES `session.messages` into a read-only snapshot.
//! 2. Builds an independent `Vec<ChatMessage>` for the judge call.
//! 3. Appends the judge query to that independent list only.
//! 4. Makes one `provider.chat_with_tools()` call with an EMPTY tool list.
//! 5. Parses the structured JSON verdict.
//!
//! The main `session.messages` is NEVER mutated by `run_shadow_judge`.
//! Only the CALLER in `conversation.rs` mutates `session.messages` when it
//! injects the steering hint message — a deliberate, controlled write.
//!
//! ## Prompt cache behaviour
//!
//! No `cache_control` markers are written on the judge's message list.
//! Because the conversation history in `session.messages` was already marked
//! with `cache_control: ephemeral` breakpoints by `apply_cache_control` during
//! the main loop, those same breakpoints exist inside the judge's cloned slice.
//! The Anthropic server-side cache will hit those entries without any extra
//! `cache_control` from the judge call. Writing new breakpoints would be both
//! redundant and wasteful (cache-write tokens are 12× more expensive than
//! cache-read tokens on Anthropic).

use std::sync::Arc;

use edgecrab_types::Message;
use edgequake_llm::LLMProvider;

use crate::config::ShadowJudgeConfig;
use crate::conversation::build_chat_messages;

// ─── System prompt ────────────────────────────────────────────────────────────

const SHADOW_JUDGE_SYSTEM_PROMPT: &str = "\
You are a task-completion oracle. Your ONLY output is a JSON object. No prose outside the JSON.

Output schema:
{\"verdict\":\"complete\"|\"incomplete\",\"confidence\":0.0-1.0,\"reason\":\"<one sentence>\",\"steering_hint\":\"<specific next action if incomplete, else null>\"}

Strict rules:
- \"complete\" means EVERY part of the user's original request is DONE with concrete evidence in the conversation.
- If the agent announced a future action but has not yet executed it, output \"incomplete\".
- If any explicitly requested sub-task is missing evidence of completion, output \"incomplete\".
- When uncertain, prefer \"incomplete\".
- Output ONLY the JSON object. No markdown fences. No commentary.";

/// Final user message appended to the judge's isolated message list.
/// Never added to session.messages — used only inside `run_shadow_judge`.
const SHADOW_JUDGE_QUERY: &str = "\
[shadow-judge query]
Review the entire conversation above. Has the agent's most recent response fully \
completed the original user request? Check every sub-goal explicitly. If any sub-goal \
was promised or implied but not yet evidenced with tool output or concrete file content, \
output \"incomplete\". Output JSON verdict now.";

// ─── Public types ─────────────────────────────────────────────────────────────

/// Structured verdict returned by the shadow judge.
#[derive(Debug, Clone)]
pub struct ShadowVerdict {
    /// True if the judge considers the task fully complete.
    pub is_complete: bool,
    /// Judge's confidence in its verdict. Range [0.0, 1.0].
    pub confidence: f32,
    /// One-sentence reason for the verdict.
    pub reason: String,
    /// Specific next action the agent should take, if incomplete.
    pub steering_hint: Option<String>,
    /// Input tokens consumed by the judge call (for session cost tracking).
    pub input_tokens: u32,
    /// Output tokens consumed by the judge call (for session cost tracking).
    pub output_tokens: u32,
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Run the shadow judge classification call.
///
/// Returns `None` on API failure or JSON parse failure (non-fatal; caller
/// falls back to the synchronous assessor's verdict).
///
/// Returns `Some(ShadowVerdict)` on success. The caller is responsible for
/// checking `verdict.is_complete` and `verdict.confidence` before acting.
///
/// # Session Isolation
///
/// This function NEVER writes to `messages`. It builds its own isolated
/// `Vec<ChatMessage>` from a slice of `messages` and the judge prompts.
/// The original `messages` slice is borrowed immutably for the duration of
/// this call and is unchanged on return.
pub async fn run_shadow_judge(
    provider: &Arc<dyn LLMProvider>,
    model: &str,
    messages: &[Message],
    config: &ShadowJudgeConfig,
) -> Option<ShadowVerdict> {
    // Respect minimum session length guard.
    if messages.len() < config.min_messages_before_enable {
        tracing::debug!(
            msg_count = messages.len(),
            min = config.min_messages_before_enable,
            "shadow judge: skipping — session too short"
        );
        return None;
    }

    // Trim to last `context_messages` to control token cost.
    // WHY: Long sessions have mostly-cached history; sending all is cheap.
    // But for very long sessions (100+ messages) we bound to the recent tail.
    let context_slice = if config.context_messages > 0
        && messages.len() > config.context_messages
    {
        &messages[messages.len() - config.context_messages..]
    } else {
        messages
    };

    // Build the judge's isolated message list.
    // WHY `build_chat_messages` with `cache_config = None`:
    //   - We pass the judge's own system prompt (not the main session's).
    //   - `None` for cache_config → no `cache_control` annotations written.
    //   - Existing `cache_control` markers from the main session's
    //     `apply_cache_control` are preserved inside the slice because
    //     they are stored on the `Message` structs themselves. Anthropic
    //     will hit those existing cache entries.
    let mut chat_messages = build_chat_messages(
        Some(SHADOW_JUDGE_SYSTEM_PROMPT),
        context_slice,
        None, // No new cache breakpoints on the judge call
    );

    // Append judge query as the final user message.
    // This is NEVER added to session.messages.
    chat_messages.push(edgequake_llm::ChatMessage::user(SHADOW_JUDGE_QUERY));

    // Make the LLM call — no tools, non-streaming, minimal output tokens.
    //
    // WHY `chat_with_tools` with empty tool list: The main provider API
    // uses `chat_with_tools` uniformly. An empty tool list produces the
    // same result as a plain `chat` call but reuses the existing call site.
    let response = match provider
        .chat_with_tools(&chat_messages, &[], Some(model), None)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(
                error = %e,
                model = model,
                "shadow judge: API call failed (non-fatal, continuing with sync assessor verdict)"
            );
            return None;
        }
    };

    let raw_text = response.content.trim().to_string();
    let input_tokens = response.usage.as_ref().map_or(0, |u| u.input_tokens as u32);
    let output_tokens = response.usage.as_ref().map_or(0, |u| u.output_tokens as u32);

    tracing::debug!(
        raw = %raw_text,
        input_tokens,
        output_tokens,
        "shadow judge: raw response"
    );

    parse_shadow_verdict(&raw_text, input_tokens, output_tokens)
}

/// Resolve the provider and model to use for the shadow judge.
///
/// Priority:
/// 1. `shadow_judge.model` (if set) — split on '/' for provider+model
/// 2. `auxiliary.model` (if set) — split on '/' for provider+model
/// 3. Fallback: caller-supplied `(main_provider, main_model)`.
///
/// Returns `(provider, model_string)`.
pub fn resolve_shadow_provider_and_model(
    shadow_cfg: &ShadowJudgeConfig,
    auxiliary_model: Option<&str>,
    main_provider: Arc<dyn LLMProvider>,
    main_model: &str,
) -> (Arc<dyn LLMProvider>, String) {
    // Try shadow_judge.model first, then auxiliary.model.
    let candidate = shadow_cfg
        .model
        .as_deref()
        .or(auxiliary_model)
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let Some(raw_model) = candidate else {
        // No override — use main provider + model.
        return (main_provider, main_model.to_string());
    };

    // "provider/model" → create provider for that family.
    if let Some((provider_name, _model_name)) = raw_model.split_once('/') {
        let canonical =
            edgecrab_tools::vision_models::normalize_provider_name(provider_name);
        match edgecrab_tools::create_provider_for_model(&canonical, _model_name) {
            Ok(p) => return (p, raw_model.to_string()),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    raw_model,
                    "shadow judge: failed to build configured provider, falling back to main provider"
                );
            }
        }
    }

    // Bare model name — reuse main provider credentials.
    (main_provider, raw_model.to_string())
}

// ─── Private helpers ─────────────────────────────────────────────────────────

/// Parse the judge's JSON verdict from its response text.
///
/// Returns `None` if the JSON is malformed or missing required fields.
/// WHY permissive parsing: the judge may wrap JSON in markdown fences despite
/// the system prompt forbidding it; strip them before parsing.
fn parse_shadow_verdict(text: &str, input_tokens: u32, output_tokens: u32) -> Option<ShadowVerdict> {
    // Strip optional markdown code fences (e.g. ```json ... ```)
    let cleaned = text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    // Find the JSON object boundaries in case there is leading/trailing text.
    let start = cleaned.find('{')?;
    let end = cleaned.rfind('}').map(|i| i + 1)?;
    let json_slice = &cleaned[start..end];

    let v: serde_json::Value = serde_json::from_str(json_slice).ok()?;

    let verdict_str = v["verdict"].as_str()?;
    let is_complete = verdict_str == "complete";
    let confidence = v["confidence"].as_f64().unwrap_or(0.5) as f32;
    let reason = v["reason"]
        .as_str()
        .unwrap_or("no reason provided")
        .to_string();
    let steering_hint = v["steering_hint"]
        .as_str()
        .filter(|s| !s.is_empty() && *s != "null")
        .map(str::to_string);

    Some(ShadowVerdict {
        is_complete,
        confidence,
        reason,
        steering_hint,
        input_tokens,
        output_tokens,
    })
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ─── parse_shadow_verdict tests ───────────────────────────────────────────

    #[test]
    fn parse_complete_verdict_ok() {
        let json = r#"{"verdict":"complete","confidence":0.95,"reason":"All files created.","steering_hint":null}"#;
        let v = parse_shadow_verdict(json, 100, 20).unwrap();
        assert!(v.is_complete);
        assert!((v.confidence - 0.95).abs() < 0.01);
        assert_eq!(v.reason, "All files created.");
        assert!(v.steering_hint.is_none());
    }

    #[test]
    fn parse_incomplete_verdict_with_hint() {
        let json = r#"{"verdict":"incomplete","confidence":0.88,"reason":"CSS file missing.","steering_hint":"Create style.css with the game styles."}"#;
        let v = parse_shadow_verdict(json, 200, 30).unwrap();
        assert!(!v.is_complete);
        assert!((v.confidence - 0.88).abs() < 0.01);
        assert!(v.steering_hint.is_some());
        assert!(v.steering_hint.unwrap().contains("style.css"));
    }

    #[test]
    fn parse_strips_markdown_fences() {
        let json = "```json\n{\"verdict\":\"complete\",\"confidence\":0.9,\"reason\":\"done\",\"steering_hint\":null}\n```";
        let v = parse_shadow_verdict(json, 10, 5).unwrap();
        assert!(v.is_complete);
    }

    #[test]
    fn parse_invalid_json_returns_none() {
        let bad = "This is not JSON at all.";
        assert!(parse_shadow_verdict(bad, 0, 0).is_none());
    }

    #[test]
    fn parse_missing_verdict_field_returns_none() {
        let json = r#"{"confidence":0.9,"reason":"done","steering_hint":null}"#;
        assert!(parse_shadow_verdict(json, 0, 0).is_none());
    }

    #[test]
    fn parse_json_with_leading_prose() {
        // Some models prepend a sentence despite the system prompt.
        let json = r#"Here is my verdict: {"verdict":"incomplete","confidence":0.75,"reason":"JS missing.","steering_hint":"Write game.js."}"#;
        let v = parse_shadow_verdict(json, 0, 0).unwrap();
        assert!(!v.is_complete);
    }

    #[test]
    fn parse_null_steering_hint_becomes_none() {
        let json = r#"{"verdict":"complete","confidence":0.99,"reason":"All done.","steering_hint":"null"}"#;
        let v = parse_shadow_verdict(json, 0, 0).unwrap();
        // "null" string should map to None
        assert!(v.steering_hint.is_none());
    }

    #[test]
    fn resolve_shadow_provider_and_model_no_override_uses_main() {
        // When no shadow model and no auxiliary model are configured, the main
        // provider and model are returned unchanged.
        // (This test requires a mock provider; placeholder — integrate with
        //  MockLLMProvider when it is available in the test harness.)
        let cfg = ShadowJudgeConfig::default();
        // cfg.model = None, no auxiliary_model
        // Assertion: (provider, model) == (main_provider, main_model)
        // Full integration test is in conversation.rs integration tests.
        let _ = cfg; // suppress unused warning
    }
}
```

---

## 4. Register Module (`crates/edgecrab-core/src/lib.rs`)

Add the new module declaration after `pub mod sub_agent_runner;` (≈ line 23):

```rust
pub mod shadow_judge;
```

---

## 5. Integration in `conversation.rs`

### 5.1 Declare shadow judge counter before the loop

In `execute_loop`, near the top of the function body (after `let config = ...`), add:

```rust
    // Shadow judge invocation counter — bounded by config.shadow_judge.max_per_session.
    // WHY track here: The counter must survive across `continue` iterations.
    // It is reset to 0 at loop start (each execute_loop call = one user turn).
    let mut shadow_judge_invocations: u32 = 0;
    let shadow_judge_cfg = config.shadow_judge.clone();
```

### 5.2 Resolve shadow judge provider once before the loop

After the `let effective_provider = ...` snapshot at loop start, add:

```rust
    // Resolve the shadow judge provider/model once per session.
    // WHY pre-resolution: provider construction may involve env lookups and Arc
    // allocation. We do it once and reuse inside the loop.
    let (shadow_judge_provider, shadow_judge_model) = if shadow_judge_cfg.enabled {
        crate::shadow_judge::resolve_shadow_provider_and_model(
            &shadow_judge_cfg,
            config.auxiliary.model.as_deref(),
            Arc::clone(&effective_provider),
            &config.model,
        )
    } else {
        // Placeholder — unused when shadow judge is disabled.
        (Arc::clone(&effective_provider), config.model.clone())
    };
```

### 5.3 Add shadow judge veto at `LoopAction::Done` branch

The current code at the `LoopAction::Done(text)` branch (≈ line 1913):

```rust
                    // (existing code) ...
                    if should_continue_after_model_text(&provisional_outcome) {
                        // ... inject follow-up and continue ...
                    }

                    final_response = text;
                    break;
```

Replace the `final_response = text; break;` block with:

```rust
                    // ── Shadow Judge veto ──────────────────────────────────────────
                    // Only fires when:
                    //   1. Shadow judge is enabled in config
                    //   2. The synchronous assessor says "Completed"
                    //      (we are past the should_continue_after_model_text gate above)
                    //   3. Session is long enough (min_messages_before_enable)
                    //   4. We haven't hit the per-session invocation cap
                    //
                    // Constraint SJ-1: run_shadow_judge() NEVER mutates session.messages.
                    // Constraint SJ-3: only downgrade verdict (Completed → Incomplete).
                    if shadow_judge_cfg.enabled
                        && shadow_judge_invocations < shadow_judge_cfg.max_per_session
                        && session.messages.len() >= shadow_judge_cfg.min_messages_before_enable
                    {
                        if let Some(verdict) = crate::shadow_judge::run_shadow_judge(
                            &shadow_judge_provider,
                            &shadow_judge_model,
                            &session.messages,
                            &shadow_judge_cfg,
                        )
                        .await
                        {
                            // Accumulate shadow judge tokens into session totals for
                            // cost tracking and usage display (SJ-10).
                            session.session_input_tokens += verdict.input_tokens as u64;
                            session.session_output_tokens += verdict.output_tokens as u64;

                            if !verdict.is_complete
                                && verdict.confidence >= shadow_judge_cfg.confidence_threshold
                            {
                                shadow_judge_invocations += 1;
                                tracing::info!(
                                    invocation = shadow_judge_invocations,
                                    confidence = verdict.confidence,
                                    reason = %verdict.reason,
                                    has_hint = verdict.steering_hint.is_some(),
                                    "shadow judge: task incomplete — continuing loop"
                                );
                                session.messages.push(Message::user(
                                    &build_shadow_judge_message(&verdict),
                                ));
                                self.publish_session_state(&session).await;
                                continue;
                            } else {
                                tracing::debug!(
                                    confidence = verdict.confidence,
                                    is_complete = verdict.is_complete,
                                    threshold = shadow_judge_cfg.confidence_threshold,
                                    "shadow judge: verdict is complete or below confidence threshold — proceeding to break"
                                );
                            }
                        }
                    }

                    final_response = text;
                    break;
```

### 5.4 Add `build_shadow_judge_message` helper function

Add near `build_completion_follow_up_message` (≈ line 2600):

```rust
/// Format a user-visible continuation message from a shadow judge verdict.
///
/// WHY a separate function: mirrors `build_completion_follow_up_message` in
/// style and purpose. The shadow judge message is more specific — it carries
/// the judge's precise reason and steering hint — which is more useful to
/// the agent than the generic "do not stop yet" text.
fn build_shadow_judge_message(verdict: &crate::shadow_judge::ShadowVerdict) -> String {
    let hint = verdict
        .steering_hint
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("Continue working until the original request is fully complete.");

    format!(
        "[shadow-judge: {}. {}]",
        verdict.reason.trim_end_matches('.'),
        hint
    )
}
```

---

## 6. Provider Resolution Contract

The shadow judge provider is resolved via `resolve_shadow_provider_and_model()`:

```
Priority order:
  shadow_judge.model (e.g. "anthropic/claude-haiku-4-20250514")
  → split on '/' → canonical provider name + model name
  → create_provider_for_model(canonical, model_name)

  If not set: auxiliary.model (same split logic)

  If not set: (main_provider, main_model)
```

This matches the pattern in `sub_agent_runner.rs::resolve_child_provider_and_model`.
The helper is `edgecrab_tools::create_provider_for_model` (already used by the
delegation runner).

---

## 7. Token Accounting

Session token usage is tracked in `SessionState`:
- `session.session_input_tokens: u64`
- `session.session_output_tokens: u64`

The shadow judge increments these with `verdict.input_tokens` and
`verdict.output_tokens` (extracted from `response.usage`). This ensures:
- `/cost` and `/usage` slash commands reflect shadow judge overhead.
- The final `ConversationResult::usage` is accurate.

---

## 8. Configuration YAML Example

```yaml
# ~/.edgecrab/config.yaml

# Route side-task calls to a cheap model
auxiliary:
  model: "anthropic/claude-haiku-4-20250514"
  provider: "anthropic"

# Enable shadow judge for Nova-lite sessions
shadow_judge:
  enabled: true
  # model: null → inherits auxiliary.model = claude-haiku-4
  max_per_session: 5
  confidence_threshold: 0.70
  context_messages: 20
  min_messages_before_enable: 4
```

---

## 9. Test Plan

### Unit Tests (in `shadow_judge.rs`)

| # | Test | Coverage |
|---|------|----------|
| T1 | `parse_complete_verdict_ok` | Happy path complete |
| T2 | `parse_incomplete_verdict_with_hint` | Happy path incomplete + hint |
| T3 | `parse_strips_markdown_fences` | JSON fence tolerance |
| T4 | `parse_invalid_json_returns_none` | Malformed input → None |
| T5 | `parse_missing_verdict_field_returns_none` | Partial JSON → None |
| T6 | `parse_json_with_leading_prose` | Prose-prefixed JSON extraction |
| T7 | `parse_null_steering_hint_becomes_none` | "null" string → Option::None |

### Integration Tests (add to `conversation.rs` test section)

| # | Test | Coverage |
|---|------|----------|
| I1 | `shadow_judge_disabled_loop_breaks_normally` | disabled=false → no extra turns |
| I2 | `shadow_judge_incomplete_verdict_continues_loop` | judge says incomplete → loop continues |
| I3 | `shadow_judge_complete_verdict_allows_break` | judge says complete → loop breaks |
| I4 | `shadow_judge_max_per_session_enforced` | after N invocations → judge skipped |
| I5 | `shadow_judge_api_error_is_nonfatal` | provider error → loop breaks normally |
| I6 | `shadow_judge_below_confidence_threshold_allows_break` | low confidence → treated as complete |
| I7 | `shadow_judge_short_session_skipped` | len < min_messages → judge not invoked |
| I8 | `shadow_judge_tokens_added_to_session_usage` | tokens accumulated correctly |

### Token Cost Validation (manual)

Run a Nova-lite session with shadow judge enabled. After the session, inspect `/cost` and
`/usage` output. Shadow judge overhead should appear in total tokens and be itemized in
session logs at `tracing::debug!` level.

---

## 10. Migration and Rollout

### Phase 1 — Implementation (this plan)

- Implement all 8 changes above.
- Deploy with `shadow_judge.enabled: false` default.
- All existing sessions unaffected.

### Phase 2 — Auto-Suggest for Weak Models

Update `model_catalog_default.yaml` to add `suggest_shadow_judge: true` on:
- `amazon.nova-lite-v1:0`
- `amazon.nova-micro-v1:0`
- Any model with `context_window < 32768` (proxy for "capacity-limited model")

The setup wizard reads this flag and suggests enabling shadow judge in `config.yaml`
when the user selects a flagged model.

### Phase 3 — Heuristic Retirement (Long Term)

Once shadow judge has accumulated 90 days of production use without regressions,
evaluate retiring the ADR 003 deferred-work heuristic in `completion_assessor.rs`.
The shadow judge's semantic coverage fully subsumes it.

---

## 11. Design Constraints Verification

| Constraint | How Satisfied |
|------------|--------------|
| C1: Shadow call MUST NOT mutate `session.messages` | `run_shadow_judge()` takes `&[Message]` (immutable borrow). Only the caller writes the steering hint. |
| C2: MUST NOT rebuild main session system prompt | The judge uses its own `SHADOW_JUDGE_SYSTEM_PROMPT`. The main `session.cached_system_prompt` is not touched. |
| C3: MUST produce structured JSON without LLM retry | `parse_shadow_verdict()` attempts JSON extraction with fence-stripping and object boundary search. Returns `None` on failure — caller falls back. |
| C4: MUST be skippable via config | `shadow_judge_cfg.enabled = false` short-circuits before any provider call. |
| C5: MUST bound invocations per session | `shadow_judge_invocations < shadow_judge_cfg.max_per_session` check before every call. |
| C6: MUST be non-fatal | All `provider.chat_with_tools()` errors return `None`; caller proceeds to `break` normally. |
| C7: MUST account tokens in session usage | `session.session_input_tokens` and `session.session_output_tokens` incremented with `verdict.input_tokens` and `verdict.output_tokens`. |
