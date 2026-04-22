//! # Shadow Judge — Lightweight LLM Completion Oracle
//!
//! Fires AFTER the synchronous `DefaultCompletionPolicy` returns `Completed`.
//! Makes one isolated LLM classification call to verify that the original
//! user request is fully satisfied before the main loop breaks.
//!
//! ## Session isolation guarantee
//!
//! `run_shadow_judge()` borrows `messages` immutably and NEVER writes back.
//! The caller in `conversation.rs` is the ONLY place that may push a
//! steering-hint message — and only when `is_complete == false`.
//!
//! ## Prompt-cache behaviour
//!
//! No new `cache_control` markers are written for the judge call.
//! Existing markers on the cloned message slice — written by
//! `apply_cache_control` during the main loop — are preserved and will
//! produce cache HIT tokens on Anthropic, keeping per-call cost ≈ $0.003–0.005.

use std::sync::Arc;

use edgecrab_types::Message;
use edgequake_llm::LLMProvider;

use crate::config::ShadowJudgeConfig;
use crate::conversation::build_chat_messages;

// ─── System prompt ─────────────────────────────────────────────────────────────

const SHADOW_JUDGE_SYSTEM_PROMPT: &str = "\
You are a task-completion oracle. Your ONLY output is a JSON object — no prose outside the JSON.

Output schema:
{\"verdict\":\"complete\"|\"incomplete\",\"confidence\":0.0-1.0,\"reason\":\"<one sentence>\",\"steering_hint\":\"<specific next action if incomplete, otherwise null>\"}

Strict rules:
- \"complete\" means EVERY part of the user's original request is DONE with concrete evidence in the conversation.
- If the agent announced a future action but has not yet executed it, output \"incomplete\".
- If any explicitly requested sub-task is missing evidence of completion, output \"incomplete\".
- When uncertain, prefer \"incomplete\".
- Output ONLY the JSON object. No markdown fences. No commentary.";

/// Final user message appended to the judge's isolated message list.
/// Never added to the main `session.messages`.
const SHADOW_JUDGE_QUERY: &str = "\
[shadow-judge query]
Review the entire conversation above. Has the agent's most recent response fully \
completed the original user request? Check every sub-goal explicitly. If any sub-goal \
was promised or implied but not yet evidenced with tool output or concrete content, \
output \"incomplete\". Output the JSON verdict now.";

// ─── Public types ───────────────────────────────────────────────────────────────

/// Structured verdict from the shadow judge.
#[derive(Debug, Clone)]
pub struct ShadowVerdict {
    /// `true` if the judge considers the task fully complete.
    pub is_complete: bool,
    /// Judge confidence in the verdict. Range `[0.0, 1.0]`.
    pub confidence: f32,
    /// One-sentence reason for the verdict.
    pub reason: String,
    /// Specific next action the agent should take, if incomplete.
    pub steering_hint: Option<String>,
    /// Input tokens consumed by this judge call (for session cost accounting).
    pub input_tokens: u32,
    /// Output tokens consumed by this judge call.
    pub output_tokens: u32,
}

// ─── Public API ────────────────────────────────────────────────────────────────

/// Run a single shadow judge classification call.
///
/// Returns `None` on API failure or JSON parse failure — both are non-fatal.
/// The caller falls back to the synchronous assessor's verdict.
///
/// Returns `Some(ShadowVerdict)` on success. The caller checks
/// `verdict.is_complete` and `verdict.confidence` before acting.
///
/// # Session Isolation
///
/// This function takes a shared reference to `messages` and NEVER mutates it.
/// It constructs its own ephemeral `Vec<ChatMessage>` for the judge call and
/// discards that list on return.
pub async fn run_shadow_judge(
    provider: &Arc<dyn LLMProvider>,
    model: &str,
    messages: &[Message],
    config: &ShadowJudgeConfig,
) -> Option<ShadowVerdict> {
    // Minimum session length guard — skip trivial Q&A sessions.
    if messages.len() < config.min_messages_before_enable {
        tracing::debug!(
            msg_count = messages.len(),
            min = config.min_messages_before_enable,
            "shadow judge: skipping — session too short"
        );
        return None;
    }

    // Trim to the most-recent `context_messages` to bound token cost.
    // For very large sessions the tail is sufficient; older turns are already
    // cached so we lose little context but save significant prompt tokens.
    let context_slice = if config.context_messages > 0 && messages.len() > config.context_messages {
        &messages[messages.len() - config.context_messages..]
    } else {
        messages
    };

    // Build the judge's isolated message list.
    //   - `SHADOW_JUDGE_SYSTEM_PROMPT`: judge's own identity (not session prompt).
    //   - `cache_config = None`: do NOT write new cache_control markers; existing
    //     markers on the slice produce server-side cache HITs for free.
    let mut chat_messages =
        build_chat_messages(Some(SHADOW_JUDGE_SYSTEM_PROMPT), context_slice, None);

    // Append the judge query as the final user message.
    // This message is NEVER propagated to session.messages.
    chat_messages.push(edgequake_llm::ChatMessage::user(SHADOW_JUDGE_QUERY));

    // One-shot call with an empty tool list and no streaming.
    // The LLMProvider default for `chat_with_tools` with an empty slice is
    // equivalent to a plain `chat` call.
    let response = match provider
        .chat_with_tools(&chat_messages, &[], None, None)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(
                error = %e,
                model = model,
                "shadow judge: API call failed (non-fatal, falling through to loop break)"
            );
            return None;
        }
    };

    let raw_text = response.content.trim().to_string();
    let input_tokens = response.prompt_tokens as u32;
    let output_tokens = response.completion_tokens as u32;

    tracing::debug!(
        raw = %raw_text,
        input_tokens,
        output_tokens,
        "shadow judge: raw response"
    );

    parse_shadow_verdict(&raw_text, input_tokens, output_tokens)
}

/// Resolve the `(provider, model_string)` pair to use for the shadow judge.
///
/// Priority:
/// 1. `shadow_judge.model` (explicit override)
/// 2. `auxiliary_model` passed by caller (from `AgentConfig.auxiliary.model`)
/// 3. Fallback: `(main_provider.clone(), main_model.to_string())`
///
/// When the chosen model string contains `/`, the prefix is treated as the
/// provider family and a new provider is created via
/// `edgecrab_tools::create_provider_for_model`. On failure, falls back to
/// the main provider with the raw model string.
pub fn resolve_shadow_provider_and_model(
    shadow_cfg: &ShadowJudgeConfig,
    auxiliary_model: Option<&str>,
    main_provider: Arc<dyn LLMProvider>,
    main_model: &str,
) -> (Arc<dyn LLMProvider>, String) {
    let candidate = shadow_cfg
        .model
        .as_deref()
        .or(auxiliary_model)
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let Some(raw_model) = candidate else {
        return (main_provider, main_model.to_string());
    };

    if let Some((provider_name, model_name)) = raw_model.split_once('/') {
        let canonical = edgecrab_tools::vision_models::normalize_provider_name(provider_name);
        match edgecrab_tools::create_provider_for_model(&canonical, model_name) {
            Ok(p) => return (p, raw_model.to_string()),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    raw_model,
                    "shadow judge: failed to create configured provider, using main provider"
                );
            }
        }
    }

    // Bare model name — reuse main provider credentials.
    (main_provider, raw_model.to_string())
}

// ─── Private helpers ─────────────────────────────────────────────────────────

/// Parse the judge's JSON verdict, tolerating markdown fences and leading prose.
///
/// Returns `None` if JSON is malformed or `verdict` field is missing.
fn parse_shadow_verdict(
    text: &str,
    input_tokens: u32,
    output_tokens: u32,
) -> Option<ShadowVerdict> {
    // Strip markdown code fences that some models emit despite the prompt.
    let stripped = text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    // Find JSON object boundaries in case there is surrounding prose.
    let start = stripped.find('{')?;
    let end = stripped.rfind('}').map(|i| i + 1)?;
    let json_slice = &stripped[start..end];

    let v: serde_json::Value = serde_json::from_str(json_slice).ok()?;

    let verdict_str = v["verdict"].as_str()?;
    let is_complete = verdict_str == "complete";
    let confidence = v["confidence"].as_f64().unwrap_or(0.5) as f32;
    let reason = v["reason"]
        .as_str()
        .unwrap_or("no reason provided")
        .to_string();

    // Treat JSON `null`, the string "null", and empty string all as None.
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

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_complete_verdict_ok() {
        let json = r#"{"verdict":"complete","confidence":0.95,"reason":"All files created.","steering_hint":null}"#;
        let v =
            parse_shadow_verdict(json, 100, 20).expect("expected valid complete shadow verdict");
        assert!(v.is_complete);
        assert!((v.confidence - 0.95).abs() < 0.01);
        assert_eq!(v.reason, "All files created.");
        assert!(v.steering_hint.is_none());
        assert_eq!(v.input_tokens, 100);
        assert_eq!(v.output_tokens, 20);
    }

    #[test]
    fn parse_incomplete_verdict_with_hint() {
        let json = r#"{"verdict":"incomplete","confidence":0.88,"reason":"CSS file missing.","steering_hint":"Create style.css with the game styles."}"#;
        let v = parse_shadow_verdict(json, 200, 30)
            .expect("expected valid incomplete shadow verdict with hint");
        assert!(!v.is_complete);
        assert!((v.confidence - 0.88).abs() < 0.01);
        assert!(v.steering_hint.is_some());
        assert!(v
            .steering_hint
            .as_deref()
            .is_some_and(|hint| hint.contains("style.css")));
    }

    #[test]
    fn parse_strips_markdown_fences() {
        let json = "```json\n{\"verdict\":\"complete\",\"confidence\":0.9,\"reason\":\"done\",\"steering_hint\":null}\n```";
        let v =
            parse_shadow_verdict(json, 10, 5).expect("expected fenced JSON shadow verdict");
        assert!(v.is_complete);
    }

    #[test]
    fn parse_json_fence_no_lang_tag() {
        let json = "```\n{\"verdict\":\"incomplete\",\"confidence\":0.7,\"reason\":\"not done\",\"steering_hint\":\"keep going\"}\n```";
        let v = parse_shadow_verdict(json, 0, 0)
            .expect("expected fenced JSON shadow verdict without language tag");
        assert!(!v.is_complete);
    }

    #[test]
    fn parse_invalid_json_returns_none() {
        assert!(parse_shadow_verdict("This is not JSON at all.", 0, 0).is_none());
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
        let v = parse_shadow_verdict(json, 0, 0)
            .expect("expected parser to recover JSON shadow verdict after prose prefix");
        assert!(!v.is_complete);
        assert!(v.steering_hint.is_some());
    }

    #[test]
    fn parse_null_string_steering_hint_becomes_none() {
        let json = r#"{"verdict":"complete","confidence":0.99,"reason":"All done.","steering_hint":"null"}"#;
        let v = parse_shadow_verdict(json, 0, 0)
            .expect("expected valid shadow verdict with string null steering hint");
        assert!(v.steering_hint.is_none());
    }

    #[test]
    fn parse_empty_string_steering_hint_becomes_none() {
        let json =
            r#"{"verdict":"complete","confidence":0.99,"reason":"All done.","steering_hint":""}"#;
        let v = parse_shadow_verdict(json, 0, 0)
            .expect("expected valid shadow verdict with empty steering hint");
        assert!(v.steering_hint.is_none());
    }

    #[test]
    fn default_shadow_judge_config_is_disabled() {
        let cfg = ShadowJudgeConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.max_per_session, 5);
        assert!((cfg.confidence_threshold - 0.70).abs() < 0.001);
        assert_eq!(cfg.context_messages, 20);
        assert_eq!(cfg.min_messages_before_enable, 4);
    }

    #[test]
    fn resolve_no_override_returns_main_model() {
        // When no shadow model and no auxiliary model are configured,
        // resolve must return the main_model string unchanged and the
        // same provider pointer.
        use edgequake_llm::MockProvider;
        use std::sync::Arc;

        let cfg = ShadowJudgeConfig::default();
        assert!(cfg.model.is_none());

        let mock: Arc<dyn edgequake_llm::LLMProvider> = Arc::new(MockProvider::new());
        let (returned_provider, returned_model) = resolve_shadow_provider_and_model(
            &cfg,
            None,
            mock.clone(),
            "anthropic/claude-sonnet-4",
        );

        assert_eq!(returned_model, "anthropic/claude-sonnet-4");
        assert!(Arc::ptr_eq(&returned_provider, &mock));
    }

    #[test]
    fn resolve_auxiliary_model_overrides_when_no_shadow_model() {
        // When `shadow_cfg.model` is None but `auxiliary_model` is provided,
        // the auxiliary model string should be used as the fallback candidate.
        use edgequake_llm::MockProvider;
        use std::sync::Arc;

        let cfg = ShadowJudgeConfig::default(); // model: None
        let mock: Arc<dyn edgequake_llm::LLMProvider> = Arc::new(MockProvider::new());
        // "bare-model-name" has no '/' so resolve falls through to the
        // (main_provider, raw_model) branch rather than creating a new provider.
        let (returned_provider, returned_model) = resolve_shadow_provider_and_model(
            &cfg,
            Some("bare-model-name"),
            mock.clone(),
            "main/model",
        );

        assert_eq!(returned_model, "bare-model-name");
        // Provider should still be the main one (no '/' means no new provider is created).
        assert!(Arc::ptr_eq(&returned_provider, &mock));
    }
}
