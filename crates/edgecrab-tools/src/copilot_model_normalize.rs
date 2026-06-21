//! Copilot model ID normalization (Hermes `normalize_copilot_model_id` parity).

const ALIASES: &[(&str, &str)] = &[
    ("anthropic/claude-opus-4.6", "claude-opus-4.6"),
    ("anthropic/claude-sonnet-4.6", "claude-sonnet-4.6"),
    ("anthropic/claude-sonnet-4", "claude-sonnet-4"),
    ("anthropic/claude-sonnet-4.5", "claude-sonnet-4.5"),
    ("anthropic/claude-haiku-4.5", "claude-haiku-4.5"),
    ("claude-opus-4-6", "claude-opus-4.6"),
    ("claude-sonnet-4-6", "claude-sonnet-4.6"),
    ("claude-sonnet-4-0", "claude-sonnet-4"),
    ("claude-sonnet-4-5", "claude-sonnet-4.5"),
    ("claude-haiku-4-5", "claude-haiku-4.5"),
    ("anthropic/claude-opus-4-6", "claude-opus-4.6"),
    ("anthropic/claude-sonnet-4-6", "claude-sonnet-4.6"),
    ("anthropic/claude-sonnet-4-0", "claude-sonnet-4"),
    ("anthropic/claude-sonnet-4-5", "claude-sonnet-4.5"),
    ("anthropic/claude-haiku-4-5", "claude-haiku-4.5"),
    ("openai/gpt-4.1", "gpt-4.1"),
    ("openai/gpt-4.1-mini", "gpt-4.1-mini"),
];

/// Normalize user-facing Copilot model aliases to API model slugs.
pub fn normalize_copilot_model_id(model: &str) -> String {
    let raw = model
        .trim()
        .strip_prefix("copilot/")
        .unwrap_or(model.trim())
        .trim()
        .to_string();
    if raw.is_empty() {
        return raw;
    }
    if raw.eq_ignore_ascii_case("auto") {
        return "auto".to_string();
    }

    for (alias, target) in ALIASES {
        if raw.eq_ignore_ascii_case(alias) {
            return (*target).to_string();
        }
    }

    if let Some((_, bare)) = raw.split_once('/') {
        let bare = bare.trim();
        for (alias, target) in ALIASES {
            if bare.eq_ignore_ascii_case(alias) {
                return (*target).to_string();
            }
        }
        return bare.to_string();
    }

    raw
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_dash_notation_haiku() {
        assert_eq!(
            normalize_copilot_model_id("claude-haiku-4-5"),
            "claude-haiku-4.5"
        );
        assert_eq!(
            normalize_copilot_model_id("copilot/claude-haiku-4.5"),
            "claude-haiku-4.5"
        );
    }

    #[test]
    fn preserves_auto() {
        assert_eq!(normalize_copilot_model_id("copilot/auto"), "auto");
    }
}
