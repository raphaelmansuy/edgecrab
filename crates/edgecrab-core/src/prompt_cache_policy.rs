//! Prompt prefix cache eligibility + breakpoint placement (Hermes parity).
//!
//! Eligibility: `decide_prompt_cache` — single source of truth for whether caching
//! is active and which wire layout (`native_inner` vs envelope).
//!
//! Placement: [`apply_prompt_cache_breakpoints`] — Hermes `_can_carry_marker` /
//! `system_and_3` for envelope; last-N user breakpoints for native inner layout
//! (stable/semi system markers are applied by the 3-tier builder).

use edgequake_llm::{CacheControl, CachePromptConfig, ChatMessage, ChatRole};

/// Anthropic allows at most four `cache_control` breakpoints per request.
pub const MAX_CACHE_BREAKPOINTS: usize = 4;

/// Whether prompt caching should be active for this provider/model pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PromptCacheDecision {
    pub should_cache: bool,
    /// When true, markers belong on inner content blocks (native Anthropic).
    /// When false, envelope layout (OpenRouter / Qwen / Kimi on OpenAI wire).
    pub native_inner_layout: bool,
}

/// Host suffix match — `host` ends with `.suffix` or equals `suffix`.
fn host_matches(base_url: Option<&str>, suffix: &str) -> bool {
    let Some(url) = base_url else {
        return false;
    };
    let host = url
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    host == suffix || host.ends_with(&format!(".{suffix}"))
}

/// Hermes `_model_name_is_kimi_family` / moonshot OpenRouter gate.
fn model_is_kimi_family(model_lower: &str) -> bool {
    model_lower.contains("kimi")
        || model_lower.contains("moonshot")
        || model_lower.contains("moonshotai/")
}

/// Decide whether EdgeCrab should attach prompt cache breakpoints.
pub fn decide_prompt_cache(
    provider_name: &str,
    model: &str,
    base_url: Option<&str>,
) -> PromptCacheDecision {
    let provider_lower = provider_name.to_ascii_lowercase();
    let model_lower = model.to_ascii_lowercase();
    let is_claude = model_lower.contains("claude");
    let is_kimi = model_is_kimi_family(&model_lower);
    let is_openrouter = host_matches(base_url, "openrouter.ai");
    let is_nous_portal = base_url
        .map(|u| u.to_ascii_lowercase().contains("nousresearch"))
        .unwrap_or(false);
    let is_native_anthropic =
        provider_lower == "anthropic" || host_matches(base_url, "api.anthropic.com");

    if is_native_anthropic {
        return PromptCacheDecision {
            should_cache: true,
            native_inner_layout: true,
        };
    }
    // OpenRouter / Nous: Claude + Kimi use envelope `cache_control` (Hermes #25970).
    if (is_openrouter || is_nous_portal) && (is_claude || is_kimi) {
        return PromptCacheDecision {
            should_cache: true,
            native_inner_layout: false,
        };
    }
    if is_nous_portal && model_lower.contains("qwen") {
        return PromptCacheDecision {
            should_cache: true,
            native_inner_layout: false,
        };
    }
    if is_claude && provider_lower.contains("anthropic") {
        return PromptCacheDecision {
            should_cache: true,
            native_inner_layout: true,
        };
    }

    let is_minimax = matches!(provider_lower.as_str(), "minimax" | "minimax-cn")
        || host_matches(base_url, "api.minimax.io")
        || host_matches(base_url, "api.minimaxi.com");
    if is_claude && is_minimax {
        return PromptCacheDecision {
            should_cache: true,
            native_inner_layout: true,
        };
    }

    let model_is_qwen = model_lower.contains("qwen");
    let provider_is_alibaba_family = matches!(
        provider_lower.as_str(),
        "opencode" | "opencode-zen" | "opencode-go" | "alibaba"
    );
    if provider_is_alibaba_family && model_is_qwen {
        return PromptCacheDecision {
            should_cache: true,
            native_inner_layout: false,
        };
    }

    // Direct Moonshot / Kimi provider ids (envelope layout).
    if matches!(provider_lower.as_str(), "moonshot" | "kimi" | "moonshotai")
        || host_matches(base_url, "api.moonshot.cn")
        || host_matches(base_url, "api.moonshot.ai")
    {
        return PromptCacheDecision {
            should_cache: true,
            native_inner_layout: false,
        };
    }

    // Legacy gate: direct Anthropic provider id without URL (tests, mocks).
    if provider_lower == "anthropic" {
        return PromptCacheDecision {
            should_cache: true,
            native_inner_layout: true,
        };
    }

    PromptCacheDecision::default()
}

/// Fast check used by the conversation loop before building cache config.
pub fn provider_supports_prompt_caching(
    provider_name: &str,
    model: &str,
    base_url: Option<&str>,
) -> bool {
    decide_prompt_cache(provider_name, model, base_url).should_cache
}

/// Resolved prompt-cache settings for a provider turn.
#[derive(Debug, Clone)]
pub struct ResolvedPromptCache {
    pub decision: PromptCacheDecision,
    pub config: CachePromptConfig,
}

/// Build cache config when user settings and provider policy both allow caching.
pub fn resolve_prompt_cache(
    provider_name: &str,
    model: &str,
    base_url: Option<&str>,
    prompt_caching_enabled: bool,
    prefix_cfg: &crate::config::PromptPrefixCacheConfig,
) -> Option<ResolvedPromptCache> {
    if !(prompt_caching_enabled && prefix_cfg.enabled) {
        return None;
    }
    let decision = decide_prompt_cache(provider_name, model, base_url);
    if !decision.should_cache {
        return None;
    }
    Some(ResolvedPromptCache {
        decision,
        config: CachePromptConfig {
            cache_ttl: Some(prefix_cfg.normalized_ttl().to_string()),
            ..Default::default()
        },
    })
}

/// Cache marker from config TTL (explicit `1h` / `5m` — July 2026 default is 5m).
pub fn cache_marker_from_config(config: &CachePromptConfig) -> CacheControl {
    match config.cache_ttl.as_deref() {
        Some("1h") => CacheControl::ephemeral_ttl("1h"),
        Some("5m") => CacheControl::ephemeral_ttl("5m"),
        _ => CacheControl::ephemeral(),
    }
}

/// Hermes `_can_carry_marker`: envelope layout ignores top-level markers on
/// empty-content assistant/tool messages — those would waste a breakpoint.
pub fn message_can_carry_marker(msg: &ChatMessage, native_inner_layout: bool) -> bool {
    if native_inner_layout {
        return true;
    }
    // Envelope: only non-empty text content can host a content-part marker.
    // Images alone are rare on tool results; treat non-empty content as carrier.
    !msg.content.trim().is_empty()
}

/// Apply prompt-cache breakpoints (eligibility already decided by caller).
///
/// * **Native inner** (`system_already_marked`): leave system markers alone;
///   mark last-N **user** messages (stable-law 3-tier path).
/// * **Native inner** (single system block): mark system + last-N users.
/// * **Envelope**: Hermes `system_and_3` — first system + last carryable
///   non-system messages (skip empty assistant/tool), up to
///   [`MAX_CACHE_BREAKPOINTS`].
pub fn apply_prompt_cache_breakpoints(
    messages: &mut [ChatMessage],
    decision: PromptCacheDecision,
    config: &CachePromptConfig,
    system_already_marked: bool,
) {
    if !config.enabled || !decision.should_cache || messages.is_empty() {
        return;
    }
    let marker = cache_marker_from_config(config);

    if decision.native_inner_layout {
        apply_native_breakpoints(messages, config, &marker, system_already_marked);
    } else {
        apply_envelope_system_and_3(messages, &marker);
    }
}

fn apply_native_breakpoints(
    messages: &mut [ChatMessage],
    config: &CachePromptConfig,
    marker: &CacheControl,
    system_already_marked: bool,
) {
    if !system_already_marked && config.cache_system_prompt {
        for msg in messages.iter_mut() {
            if matches!(msg.role, ChatRole::System) && msg.cache_control.is_none() {
                msg.cache_control = Some(marker.clone());
                break; // first system only for single-block path
            }
        }
    }

    let user_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| matches!(m.role, ChatRole::User))
        .map(|(i, _)| i)
        .collect();
    let last_n_start = user_indices
        .len()
        .saturating_sub(config.cache_last_n_messages);
    let last_n: std::collections::HashSet<usize> =
        user_indices.into_iter().skip(last_n_start).collect();

    let used = messages
        .iter()
        .filter(|m| m.cache_control.is_some())
        .count();
    let mut remaining = MAX_CACHE_BREAKPOINTS.saturating_sub(used);

    for (i, msg) in messages.iter_mut().enumerate() {
        if remaining == 0 {
            break;
        }
        if msg.cache_control.is_some() {
            continue;
        }
        let should = matches!(msg.role, ChatRole::User)
            && (msg.content.len() >= config.min_content_length || last_n.contains(&i));
        if should {
            msg.cache_control = Some(marker.clone());
            remaining -= 1;
        }
    }
}

/// Hermes `system_and_3` for envelope providers.
fn apply_envelope_system_and_3(messages: &mut [ChatMessage], marker: &CacheControl) {
    let mut used = 0usize;

    if matches!(messages[0].role, ChatRole::System) {
        messages[0].cache_control = Some(marker.clone());
        used += 1;
    }

    let remaining = MAX_CACHE_BREAKPOINTS.saturating_sub(used);
    let carryable: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| !matches!(m.role, ChatRole::System) && message_can_carry_marker(m, false))
        .map(|(i, _)| i)
        .collect();

    let start = carryable.len().saturating_sub(remaining);
    for &idx in &carryable[start..] {
        if messages[idx].cache_control.is_none() {
            messages[idx].cache_control = Some(marker.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_anthropic_enables_cache() {
        let d = decide_prompt_cache(
            "anthropic",
            "claude-sonnet-4-20250514",
            Some("https://api.anthropic.com"),
        );
        assert!(d.should_cache);
        assert!(d.native_inner_layout);
    }

    #[test]
    fn openrouter_claude_uses_envelope_layout() {
        let d = decide_prompt_cache(
            "openrouter",
            "anthropic/claude-sonnet-4",
            Some("https://openrouter.ai/api/v1"),
        );
        assert!(d.should_cache);
        assert!(!d.native_inner_layout);
    }

    #[test]
    fn openrouter_kimi_enables_envelope_cache() {
        let d = decide_prompt_cache(
            "openrouter",
            "moonshotai/kimi-k2.6",
            Some("https://openrouter.ai/api/v1"),
        );
        assert!(d.should_cache);
        assert!(!d.native_inner_layout);
    }

    #[test]
    fn nous_portal_kimi_enables_cache() {
        let d = decide_prompt_cache("nous", "kimi-k2", Some("https://api.nousresearch.com/v1"));
        assert!(d.should_cache);
        assert!(!d.native_inner_layout);
    }

    #[test]
    fn nous_portal_qwen_enables_cache() {
        let d = decide_prompt_cache(
            "nous",
            "qwen3.6-plus",
            Some("https://api.nousresearch.com/v1"),
        );
        assert!(d.should_cache);
        assert!(!d.native_inner_layout);
    }

    #[test]
    fn opencode_qwen_enables_cache() {
        let d = decide_prompt_cache("opencode-go", "qwen3-coder", None);
        assert!(d.should_cache);
    }

    #[test]
    fn gpt4o_disables_cache() {
        let d = decide_prompt_cache("openai", "gpt-4o", None);
        assert!(!d.should_cache);
    }

    #[test]
    fn resolve_prompt_cache_respects_user_toggle() {
        let prefix = crate::config::PromptPrefixCacheConfig::default();
        assert!(
            resolve_prompt_cache("anthropic", "claude-sonnet-4", None, false, &prefix).is_none()
        );
        let resolved = resolve_prompt_cache(
            "anthropic",
            "claude-sonnet-4",
            Some("https://api.anthropic.com"),
            true,
            &prefix,
        )
        .expect("anthropic should cache");
        assert!(resolved.decision.native_inner_layout);
        assert_eq!(resolved.config.cache_ttl.as_deref(), Some("1h"));
    }

    #[test]
    fn envelope_skips_empty_assistant_tool_carriers() {
        let mut msgs = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("u1"),
            ChatMessage::assistant_with_tools("", vec![]), // empty content
            ChatMessage::tool_result("t1", ""),            // empty tool
            ChatMessage::user("u2"),
            ChatMessage::assistant("a1 with text"),
            ChatMessage::tool_result("t2", "tool body"),
        ];
        let decision = PromptCacheDecision {
            should_cache: true,
            native_inner_layout: false,
        };
        let cfg = CachePromptConfig {
            enabled: true,
            cache_ttl: Some("1h".into()),
            ..Default::default()
        };
        apply_prompt_cache_breakpoints(&mut msgs, decision, &cfg, false);

        assert!(msgs[0].cache_control.is_some(), "system marked");
        assert!(
            msgs[2].cache_control.is_none(),
            "empty assistant must not waste BP"
        );
        assert!(
            msgs[3].cache_control.is_none(),
            "empty tool must not waste BP"
        );
        // Last 3 carryable non-system: u2, a1, t2 (and possibly u1 if room)
        let marked_non_sys: Vec<usize> = msgs
            .iter()
            .enumerate()
            .skip(1)
            .filter(|(_, m)| m.cache_control.is_some())
            .map(|(i, _)| i)
            .collect();
        assert!(
            marked_non_sys.contains(&4)
                || marked_non_sys.contains(&5)
                || marked_non_sys.contains(&6),
            "carryable tail must be marked: {marked_non_sys:?}"
        );
        assert!(!marked_non_sys.contains(&2));
        assert!(!marked_non_sys.contains(&3));
        assert!(marked_non_sys.len() <= 3);
    }

    #[test]
    fn native_respects_system_already_marked() {
        let mut msgs = vec![
            {
                let mut s = ChatMessage::system("stable");
                s.cache_control = Some(CacheControl::ephemeral_ttl("1h"));
                s
            },
            ChatMessage::system("dynamic"), // no marker
            ChatMessage::user("u1"),
            ChatMessage::user("u2"),
            ChatMessage::user("u3"),
        ];
        let decision = PromptCacheDecision {
            should_cache: true,
            native_inner_layout: true,
        };
        let cfg = CachePromptConfig {
            enabled: true,
            cache_system_prompt: true,
            cache_last_n_messages: 2,
            cache_ttl: Some("1h".into()),
            ..Default::default()
        };
        apply_prompt_cache_breakpoints(&mut msgs, decision, &cfg, true);
        assert!(
            msgs[1].cache_control.is_none(),
            "dynamic must stay unmarked"
        );
        assert!(msgs[3].cache_control.is_some());
        assert!(msgs[4].cache_control.is_some());
        assert!(msgs[2].cache_control.is_none());
    }
}
