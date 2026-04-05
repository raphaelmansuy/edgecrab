//! Token usage and cost tracking types.
//!
//! Normalizes usage across different API response formats
//! (OpenAI, Anthropic, Codex) into a single structure.

use serde::{Deserialize, Serialize};

use crate::ApiMode;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Cost {
    pub input_cost: f64,
    pub output_cost: f64,
    pub cache_read_cost: f64,
    pub cache_write_cost: f64,
    pub total_cost: f64,
}

impl Usage {
    /// Compute total tokens from components.
    pub fn compute_total(&mut self) {
        self.total_tokens = self.input_tokens + self.output_tokens;
    }
}

/// Normalize raw API usage JSON into our unified Usage struct.
///
/// Each provider returns usage in a different shape — this function
/// maps them all into one representation.
pub fn normalize_usage(raw: &serde_json::Value, api_mode: ApiMode) -> Usage {
    /// Extract a u64 token count from a JSON field, defaulting to 0.
    fn tok(v: &serde_json::Value) -> u64 {
        v.as_u64().unwrap_or(0)
    }

    match api_mode {
        ApiMode::ChatCompletions => Usage {
            input_tokens: tok(&raw["prompt_tokens"]),
            output_tokens: tok(&raw["completion_tokens"]),
            cache_read_tokens: tok(&raw["prompt_tokens_details"]["cached_tokens"]),
            total_tokens: tok(&raw["total_tokens"]),
            ..Default::default()
        },
        ApiMode::AnthropicMessages => {
            let input = tok(&raw["input_tokens"]);
            let output = tok(&raw["output_tokens"]);
            Usage {
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: tok(&raw["cache_read_input_tokens"]),
                cache_write_tokens: tok(&raw["cache_creation_input_tokens"]),
                total_tokens: input + output,
                ..Default::default()
            }
        }
        ApiMode::CodexResponses => {
            let input = tok(&raw["input_tokens"]);
            let output = tok(&raw["output_tokens"]);
            Usage {
                input_tokens: input,
                output_tokens: output,
                total_tokens: input + output,
                ..Default::default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_chat_completions() {
        let raw = serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "total_tokens": 150,
            "prompt_tokens_details": { "cached_tokens": 20 }
        });
        let usage = normalize_usage(&raw, ApiMode::ChatCompletions);
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn normalize_anthropic() {
        let raw = serde_json::json!({
            "input_tokens": 200,
            "output_tokens": 80,
            "cache_read_input_tokens": 50,
            "cache_creation_input_tokens": 10
        });
        let usage = normalize_usage(&raw, ApiMode::AnthropicMessages);
        assert_eq!(usage.input_tokens, 200);
        assert_eq!(usage.output_tokens, 80);
        assert_eq!(usage.cache_read_tokens, 50);
        assert_eq!(usage.cache_write_tokens, 10);
        assert_eq!(usage.total_tokens, 280);
    }

    #[test]
    fn normalize_codex() {
        let raw = serde_json::json!({
            "input_tokens": 300,
            "output_tokens": 100
        });
        let usage = normalize_usage(&raw, ApiMode::CodexResponses);
        assert_eq!(usage.input_tokens, 300);
        assert_eq!(usage.output_tokens, 100);
        assert_eq!(usage.total_tokens, 400);
    }

    #[test]
    fn usage_roundtrip() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 20,
            cache_write_tokens: 5,
            reasoning_tokens: 10,
            total_tokens: 150,
        };
        let json = serde_json::to_string(&usage).expect("serialize");
        let deser: Usage = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(usage, deser);
    }

    #[test]
    fn cost_roundtrip() {
        let cost = Cost {
            input_cost: 0.001,
            output_cost: 0.003,
            total_cost: 0.004,
            ..Default::default()
        };
        let json = serde_json::to_string(&cost).expect("serialize");
        let deser: Cost = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cost, deser);
    }
}
