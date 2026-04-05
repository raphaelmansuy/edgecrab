//! # Model Router — smart routing & provider fallback
//!
//! WHY: Not every turn needs a $10/M-token frontier model. Simple
//! messages ("thanks", "what time is it?") can be routed to a cheaper
//! model, saving cost while preserving quality for complex turns.
//!
//! ```text
//!   user_message
//!       │
//!       ▼
//!   ┌──────────────────────┐
//!   │   classify(message)  │
//!   │   simple? complex?   │
//!   └─────────┬────────────┘
//!        simple│       │complex
//!             ▼        ▼
//!       cheap_model  primary_model
//!             │        │
//!             ▼        ▼
//!       ┌──────────────────┐
//!       │  try provider    │
//!       │  on error →      │
//!       │  try fallback    │
//!       └──────────────────┘
//! ```

use std::collections::HashSet;

use crate::config::{FallbackConfig, ModelConfig};
use edgecrab_types::ApiMode;

// ─── Complexity classification ────────────────────────────────────────

/// Keywords that signal the message involves code, debugging, or tool use.
/// If any of these appear as a word in the message, we keep the primary model.
const COMPLEX_KEYWORDS: &[&str] = &[
    "debug",
    "debugging",
    "implement",
    "implementation",
    "refactor",
    "patch",
    "traceback",
    "stacktrace",
    "exception",
    "error",
    "analyze",
    "analysis",
    "investigate",
    "architecture",
    "design",
    "compare",
    "benchmark",
    "optimize",
    "optimise",
    "review",
    "terminal",
    "shell",
    "tool",
    "tools",
    "pytest",
    "test",
    "tests",
    "plan",
    "planning",
    "delegate",
    "subagent",
    "cron",
    "docker",
    "kubernetes",
    "code",
    "function",
    "class",
    "struct",
    "enum",
    "compile",
    "build",
    "deploy",
    "fix",
    "bug",
];

/// Thresholds for classifying a message as "simple".
#[derive(Debug, Clone)]
pub struct RoutingThresholds {
    pub max_chars: usize,
    pub max_words: usize,
    pub max_newlines: usize,
}

impl Default for RoutingThresholds {
    fn default() -> Self {
        Self {
            max_chars: 160,
            max_words: 28,
            max_newlines: 1,
        }
    }
}

/// WHY the message looks complex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComplexityReason {
    TooLong,
    TooManyWords,
    MultiLine,
    ContainsCodeFence,
    ContainsInlineCode,
    ContainsUrl,
    ContainsComplexKeyword(String),
    Empty,
}

/// Classify a user message as simple or complex.
///
/// Conservative: if in doubt, keep the primary (strong) model.
/// Only short, keyword-free, single-line plain-text messages qualify
/// as "simple".
pub fn classify_message(text: &str, thresholds: &RoutingThresholds) -> Option<ComplexityReason> {
    let text = text.trim();

    if text.is_empty() {
        return Some(ComplexityReason::Empty);
    }
    if text.len() > thresholds.max_chars {
        return Some(ComplexityReason::TooLong);
    }
    if text.split_whitespace().count() > thresholds.max_words {
        return Some(ComplexityReason::TooManyWords);
    }
    if text.chars().filter(|&c| c == '\n').count() > thresholds.max_newlines {
        return Some(ComplexityReason::MultiLine);
    }
    if text.contains("```") {
        return Some(ComplexityReason::ContainsCodeFence);
    }
    if text.contains('`') {
        return Some(ComplexityReason::ContainsInlineCode);
    }
    // URL detection: "http://" or "https://" or "www."
    if text.contains("http://") || text.contains("https://") || text.contains("www.") {
        return Some(ComplexityReason::ContainsUrl);
    }

    let lowered = text.to_lowercase();
    let words: HashSet<&str> = lowered
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| c.is_ascii_punctuation()))
        .collect();

    for &kw in COMPLEX_KEYWORDS {
        if words.contains(kw) {
            return Some(ComplexityReason::ContainsComplexKeyword(kw.to_string()));
        }
    }

    None // simple!
}

// ─── Route resolution ─────────────────────────────────────────────────

/// Describes which model + provider to use for a given turn.
#[derive(Debug, Clone)]
pub struct TurnRoute {
    pub model: String,
    pub api_mode: ApiMode,
    pub base_url: Option<String>,
    pub api_key_env: String,
    /// Human-readable label, e.g. "smart route → gpt-4.1-mini (openai)"
    pub label: Option<String>,
    /// Whether this is the primary model or a routed/fallback model.
    pub is_primary: bool,
}

/// Configuration for smart/cheap model routing.
#[derive(Debug, Clone, Default)]
pub struct SmartRoutingConfig {
    pub enabled: bool,
    pub cheap_model: String,
    pub cheap_base_url: Option<String>,
    pub cheap_api_key_env: Option<String>,
    pub thresholds: RoutingThresholds,
}

/// Resolve which model to use for a given user message.
///
/// Steps:
///   1. If smart routing is enabled and the message is simple → cheap model
///   2. Otherwise → primary model
pub fn resolve_turn_route(
    user_message: &str,
    model_config: &ModelConfig,
    smart_routing: &SmartRoutingConfig,
) -> TurnRoute {
    // Try cheap model for simple messages
    if smart_routing.enabled
        && !smart_routing.cheap_model.is_empty()
        && classify_message(user_message, &smart_routing.thresholds).is_none()
    {
        let base_url = smart_routing
            .cheap_base_url
            .clone()
            .or_else(|| model_config.base_url.clone());
        let api_key_env = smart_routing
            .cheap_api_key_env
            .clone()
            .unwrap_or_else(|| model_config.api_key_env.clone());

        let api_mode = ApiMode::detect(
            base_url.as_deref().unwrap_or(""),
            &smart_routing.cheap_model,
        );

        return TurnRoute {
            model: smart_routing.cheap_model.clone(),
            api_mode,
            base_url,
            api_key_env,
            label: Some(format!("smart route → {}", smart_routing.cheap_model)),
            is_primary: false,
        };
    }

    // Primary model
    let api_mode = ApiMode::detect(
        model_config.base_url.as_deref().unwrap_or(""),
        &model_config.default_model,
    );

    TurnRoute {
        model: model_config.default_model.clone(),
        api_mode,
        base_url: model_config.base_url.clone(),
        api_key_env: model_config.api_key_env.clone(),
        label: None,
        is_primary: true,
    }
}

/// Build a fallback route from config, for use when the primary provider errors.
pub fn fallback_route(fallback: &FallbackConfig) -> TurnRoute {
    let api_mode = ApiMode::detect(fallback.base_url.as_deref().unwrap_or(""), &fallback.model);

    TurnRoute {
        model: fallback.model.clone(),
        api_mode,
        base_url: fallback.base_url.clone(),
        api_key_env: fallback
            .api_key_env
            .clone()
            .unwrap_or_else(|| "OPENROUTER_API_KEY".into()),
        label: Some(format!("fallback → {}", fallback.model)),
        is_primary: false,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_message_classified_as_simple() {
        let thresholds = RoutingThresholds::default();
        assert!(classify_message("hello there!", &thresholds).is_none());
        assert!(classify_message("what's 2+2?", &thresholds).is_none());
        assert!(classify_message("thanks!", &thresholds).is_none());
    }

    #[test]
    fn complex_keyword_detected() {
        let thresholds = RoutingThresholds::default();
        let result = classify_message("please debug this", &thresholds);
        assert_eq!(
            result,
            Some(ComplexityReason::ContainsComplexKeyword("debug".into()))
        );
    }

    #[test]
    fn long_message_is_complex() {
        let thresholds = RoutingThresholds::default();
        let long = "a ".repeat(100); // 200 chars
        assert_eq!(
            classify_message(&long, &thresholds),
            Some(ComplexityReason::TooLong)
        );
    }

    #[test]
    fn code_fence_is_complex() {
        let thresholds = RoutingThresholds::default();
        assert_eq!(
            classify_message("look at this ```code```", &thresholds),
            Some(ComplexityReason::ContainsCodeFence)
        );
    }

    #[test]
    fn inline_code_is_complex() {
        let thresholds = RoutingThresholds::default();
        assert_eq!(
            classify_message("what does `foo()` do?", &thresholds),
            Some(ComplexityReason::ContainsInlineCode)
        );
    }

    #[test]
    fn url_is_complex() {
        let thresholds = RoutingThresholds::default();
        assert!(matches!(
            classify_message("check https://example.com", &thresholds),
            Some(ComplexityReason::ContainsUrl)
        ));
    }

    #[test]
    fn multiline_is_complex() {
        let thresholds = RoutingThresholds::default();
        assert_eq!(
            classify_message("line1\nline2\nline3", &thresholds),
            Some(ComplexityReason::MultiLine)
        );
    }

    #[test]
    fn empty_message_is_complex() {
        let thresholds = RoutingThresholds::default();
        assert_eq!(
            classify_message("", &thresholds),
            Some(ComplexityReason::Empty)
        );
    }

    #[test]
    fn smart_routing_disabled_uses_primary() {
        let model_config = ModelConfig::default();
        let smart = SmartRoutingConfig::default(); // disabled
        let route = resolve_turn_route("hi", &model_config, &smart);
        assert!(route.is_primary);
        assert_eq!(route.model, model_config.default_model);
    }

    #[test]
    fn smart_routing_enabled_simple_message() {
        let model_config = ModelConfig::default();
        let smart = SmartRoutingConfig {
            enabled: true,
            cheap_model: "gpt-4.1-mini".into(),
            cheap_base_url: Some("https://api.openai.com/v1".into()),
            cheap_api_key_env: Some("OPENAI_API_KEY".into()),
            thresholds: RoutingThresholds::default(),
        };
        let route = resolve_turn_route("hi there!", &model_config, &smart);
        assert!(!route.is_primary);
        assert_eq!(route.model, "gpt-4.1-mini");
        assert!(route.label.is_some());
    }

    #[test]
    fn smart_routing_enabled_complex_message_uses_primary() {
        let model_config = ModelConfig::default();
        let smart = SmartRoutingConfig {
            enabled: true,
            cheap_model: "gpt-4.1-mini".into(),
            cheap_base_url: None,
            cheap_api_key_env: None,
            thresholds: RoutingThresholds::default(),
        };
        let route = resolve_turn_route("please debug this error", &model_config, &smart);
        assert!(route.is_primary);
    }

    #[test]
    fn fallback_route_construction() {
        let fb = FallbackConfig {
            model: "anthropic/claude-haiku".into(),
            provider: "anthropic".into(),
            base_url: Some("https://api.anthropic.com/v1".into()),
            api_key_env: Some("ANTHROPIC_API_KEY".into()),
        };
        let route = fallback_route(&fb);
        assert!(!route.is_primary);
        assert_eq!(route.model, "anthropic/claude-haiku");
        assert_eq!(route.api_key_env, "ANTHROPIC_API_KEY");
        assert!(route.label.as_ref().is_some_and(|l| l.contains("fallback")));
    }
}
