//! Startup / turn-1 context token budget estimation (CI + `/context budget`).
//!
//! Uses the same chars÷4 heuristic as `conversation.rs` so numbers match
//! compression gates and shelf displays.

use edgecrab_types::Message;
use edgequake_llm::ToolDefinition;

/// Rough token estimate from character count (mixed code + English).
pub fn estimate_chars_as_tokens(text: &str) -> usize {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return 0;
    }
    trimmed.chars().count().div_ceil(4)
}

pub fn estimate_json_tokens<T: serde::Serialize + ?Sized>(value: &T) -> usize {
    serde_json::to_string(value)
        .map(|s| estimate_chars_as_tokens(&s))
        .unwrap_or(0)
}

/// Token breakdown for doctor / slash-command display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextBudgetBreakdown {
    pub stable_tokens: usize,
    pub semi_stable_tokens: usize,
    pub dynamic_tokens: usize,
    pub tools_tokens: usize,
    pub tool_count: usize,
    /// Deferred tools not on the wire (indexed schema mode only).
    pub tools_deferred_count: Option<usize>,
    pub history_tokens: usize,
    pub total_tokens: usize,
    pub context_window: usize,
    /// Session-cumulative Anthropic/OpenRouter cache read tokens.
    pub cache_read_tokens: u64,
    /// Session-cumulative cache write tokens.
    pub cache_write_tokens: u64,
    /// Last-turn prompt mass (for hit-rate denominator when available).
    pub last_prompt_tokens: u64,
}

impl ContextBudgetBreakdown {
    pub fn pct_of_window(&self) -> f64 {
        if self.context_window == 0 {
            return 0.0;
        }
        (self.total_tokens as f64 / self.context_window as f64) * 100.0
    }

    /// Session cumulative cache hit rate: `cache_read / (input-ish + cache)`.
    pub fn cache_hit_rate_pct(&self) -> Option<f64> {
        let denom = self
            .last_prompt_tokens
            .max(self.cache_read_tokens.saturating_add(self.cache_write_tokens));
        if denom == 0 || self.cache_read_tokens == 0 {
            return None;
        }
        Some((self.cache_read_tokens as f64 / denom as f64) * 100.0)
    }

    pub fn with_deferred_tools(mut self, deferred: Option<usize>) -> Self {
        self.tools_deferred_count = deferred;
        self
    }

    pub fn with_cache_telemetry(
        mut self,
        cache_read_tokens: u64,
        cache_write_tokens: u64,
        last_prompt_tokens: u64,
    ) -> Self {
        self.cache_read_tokens = cache_read_tokens;
        self.cache_write_tokens = cache_write_tokens;
        self.last_prompt_tokens = last_prompt_tokens;
        self
    }

    pub fn format_report(&self) -> String {
        let semi_line = if self.semi_stable_tokens > 0 {
            format!("semi-stable: {:>6} tok\n", self.semi_stable_tokens)
        } else {
            String::new()
        };
        let tools_line = match self.tools_deferred_count {
            Some(deferred) => format!(
                "tools:     {:>6} tok ({} on wire, {} deferred)\n",
                self.tools_tokens, self.tool_count, deferred
            ),
            _ => format!(
                "tools:     {:>6} tok ({} tools)\n",
                self.tools_tokens, self.tool_count
            ),
        };
        let cache_line = if self.cache_read_tokens > 0 || self.cache_write_tokens > 0 {
            let hit = self
                .cache_hit_rate_pct()
                .map(|p| format!(" ({p:.0}% hit)"))
                .unwrap_or_default();
            format!(
                "cache:     read {:>6}  write {:>6}{hit}\n",
                self.cache_read_tokens, self.cache_write_tokens
            )
        } else {
            String::new()
        };
        format!(
            "Context budget (estimated):\n\
             \n\
             stable:    {:>6} tok\n\
             {semi_line}\
             dynamic:   {:>6} tok\n\
             {tools_line}\
             history:   {:>6} tok\n\
             {cache_line}\
             ─────────────────────\n\
             total:     {:>6} tok ({:.1}% of {}K)",
            self.stable_tokens,
            self.dynamic_tokens,
            self.history_tokens,
            self.total_tokens,
            self.pct_of_window(),
            self.context_window / 1000,
        )
    }
}

/// Estimate full request mass: system zones + tool schemas + conversation history.
pub fn estimate_context_budget(
    stable_prompt: Option<&str>,
    semi_stable_prompt: Option<&str>,
    dynamic_prompt: Option<&str>,
    combined_system: Option<&str>,
    messages: &[Message],
    tool_defs: &[ToolDefinition],
    context_window: usize,
) -> ContextBudgetBreakdown {
    let (stable_tokens, semi_stable_tokens, dynamic_tokens) = match (stable_prompt, dynamic_prompt)
    {
        (Some(stable), Some(dynamic)) => (
            estimate_chars_as_tokens(stable),
            estimate_chars_as_tokens(semi_stable_prompt.unwrap_or("")),
            estimate_chars_as_tokens(dynamic),
        ),
        _ => {
            let combined = combined_system.unwrap_or("");
            (0, 0, estimate_chars_as_tokens(combined))
        }
    };

    let tools_tokens = estimate_json_tokens(tool_defs);
    let tool_count = tool_defs.len();
    let history_tokens = crate::compression::estimate_tokens(messages);
    let total_tokens =
        stable_tokens + semi_stable_tokens + dynamic_tokens + tools_tokens + history_tokens;

    ContextBudgetBreakdown {
        stable_tokens,
        semi_stable_tokens,
        dynamic_tokens,
        tools_tokens,
        tool_count,
        tools_deferred_count: None,
        history_tokens,
        total_tokens,
        context_window,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        last_prompt_tokens: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_tools::{AppConfigRef, ToolContext, ToolRegistry};
    use edgecrab_types::Platform;

    fn schema_tokens_for_toolsets(alias: &str) -> usize {
        schema_tokens_for_toolsets_mode(alias, edgecrab_tools::ToolSchemaMode::Full)
    }

    fn schema_tokens_for_toolsets_mode(alias: &str, mode: edgecrab_tools::ToolSchemaMode) -> usize {
        let registry = ToolRegistry::new();
        let ctx = ToolContext {
            task_id: "budget-test".into(),
            cwd: std::env::temp_dir(),
            session_id: "budget-test".into(),
            user_task: None,
            cancel: tokio_util::sync::CancellationToken::new(),
            config: AppConfigRef::default(),
            state_db: None,
            platform: Platform::Cli,
            process_table: None,
            provider: None,
            tool_registry: None,
            delegate_depth: 0,
            delegate_agent_id: None,
            delegate_parent_id: None,
            sub_agent_runner: None,
            delegation_event_tx: None,
            clarify_tx: None,
            approval_tx: None,
            on_skills_changed: None,
            gateway_sender: None,
            origin_chat: None,
            session_key: None,
            todo_store: None,
            current_tool_call_id: None,
            current_tool_name: None,
            injected_messages: None,
            tool_progress_tx: None,
            watch_notification_tx: None,
            mutation_turn: None,
            lsp_gate: None,
            kanban_task_id: None,
            materialized_tools: None,
        };
        let enabled = edgecrab_tools::toolsets::expand_toolset_names(&[alias.to_string()]);
        let schemas = registry.get_definitions(Some(&enabled), None, &ctx);
        let llm = edgecrab_tools::to_llm_definitions_with_mode(&schemas, mode);
        estimate_json_tokens(&llm)
    }

    #[test]
    fn default_core_profile_under_18k_schema_tokens() {
        let tokens = schema_tokens_for_toolsets("core");
        assert!(
            tokens < 18_000,
            "core toolset schema budget exceeded: {tokens} tok (limit 18_000)"
        );
    }

    #[test]
    fn default_compact_core_profile_under_18k_schema_tokens() {
        use edgecrab_tools::ToolSchemaMode;
        let tokens = schema_tokens_for_toolsets_mode("core", ToolSchemaMode::Compact);
        assert!(
            tokens < 18_000,
            "compact core schema budget exceeded: {tokens} tok (limit 18_000)"
        );
    }

    #[test]
    fn minimal_profile_under_8k_schema_tokens() {
        let tokens = schema_tokens_for_toolsets("minimal");
        assert!(
            tokens < 8_000,
            "minimal toolset schema budget exceeded: {tokens} tok (limit 8_000)"
        );
    }

    #[test]
    fn compact_core_materially_smaller_than_full() {
        use edgecrab_tools::ToolSchemaMode;
        let full = schema_tokens_for_toolsets_mode("core", ToolSchemaMode::Full);
        let compact = schema_tokens_for_toolsets_mode("core", ToolSchemaMode::Compact);
        assert!(
            compact < full * 80 / 100,
            "compact core should be ≥20% smaller than full: full={full} compact={compact}"
        );
    }

    #[test]
    fn default_core_stable_guidance_under_3k_tokens() {
        use crate::prompt_builder::PromptBuilder;
        use edgecrab_types::Platform;

        let registry = ToolRegistry::new();
        let ctx = ToolContext {
            task_id: "budget-test".into(),
            cwd: std::env::temp_dir(),
            session_id: "budget-test".into(),
            user_task: None,
            cancel: tokio_util::sync::CancellationToken::new(),
            config: AppConfigRef::default(),
            state_db: None,
            platform: Platform::Cli,
            process_table: None,
            provider: None,
            tool_registry: None,
            delegate_depth: 0,
            delegate_agent_id: None,
            delegate_parent_id: None,
            sub_agent_runner: None,
            delegation_event_tx: None,
            clarify_tx: None,
            approval_tx: None,
            on_skills_changed: None,
            gateway_sender: None,
            origin_chat: None,
            session_key: None,
            todo_store: None,
            current_tool_call_id: None,
            current_tool_name: None,
            injected_messages: None,
            tool_progress_tx: None,
            watch_notification_tx: None,
            mutation_turn: None,
            lsp_gate: None,
            kanban_task_id: None,
            materialized_tools: None,
        };
        let enabled = edgecrab_tools::toolsets::expand_toolset_names(&["core".to_string()]);
        let tool_names: Vec<String> = registry
            .get_definitions(Some(&enabled), None, &ctx)
            .into_iter()
            .map(|s| s.name)
            .collect();
        let blocks = PromptBuilder::new(Platform::Cli)
            .skip_context_files(true)
            .model_name(Some("anthropic/claude-sonnet-4".to_string()))
            .available_tools(tool_names)
            .build_blocks(None, None, &[], None, None);
        let stable_tokens = estimate_chars_as_tokens(&blocks.stable);
        assert!(
            stable_tokens < 2_200,
            "default core stable guidance budget exceeded: {stable_tokens} tok (limit 2_200)"
        );
    }

    #[test]
    fn indexed_core_profile_under_8k_schema_tokens() {
        use edgecrab_tools::ToolSchemaMode;
        let tokens = schema_tokens_for_toolsets_mode("core", ToolSchemaMode::Indexed);
        assert!(
            tokens < 8_000,
            "indexed core schema budget exceeded: {tokens} tok (limit 8_000)"
        );
    }

    #[test]
    fn indexed_core_smaller_than_compact() {
        use edgecrab_tools::ToolSchemaMode;
        let compact = schema_tokens_for_toolsets_mode("core", ToolSchemaMode::Compact);
        let indexed = schema_tokens_for_toolsets_mode("core", ToolSchemaMode::Indexed);
        assert!(
            indexed < compact * 80 / 100,
            "indexed core should be ≥20% smaller than compact: compact={compact} indexed={indexed}"
        );
    }

    #[test]
    fn format_report_includes_sections() {
        let breakdown = ContextBudgetBreakdown {
            stable_tokens: 2847,
            semi_stable_tokens: 0,
            dynamic_tokens: 1203,
            tools_tokens: 14992,
            tool_count: 35,
            tools_deferred_count: None,
            history_tokens: 500,
            total_tokens: 19542,
            context_window: 128_000,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            last_prompt_tokens: 0,
        };
        let text = breakdown.format_report();
        assert!(text.contains("stable:"));
        assert!(text.contains("35 tools"));
        assert!(text.contains("128K"));
    }

    #[test]
    fn format_report_shows_deferred_when_indexed() {
        let breakdown = ContextBudgetBreakdown {
            stable_tokens: 2000,
            semi_stable_tokens: 0,
            dynamic_tokens: 800,
            tools_tokens: 4500,
            tool_count: 13,
            tools_deferred_count: Some(31),
            history_tokens: 0,
            total_tokens: 7300,
            context_window: 128_000,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            last_prompt_tokens: 0,
        };
        let text = breakdown.format_report();
        assert!(text.contains("13 on wire, 31 deferred"));
    }

    #[test]
    fn format_report_shows_zero_deferred_in_indexed_mode() {
        let breakdown = ContextBudgetBreakdown {
            stable_tokens: 2000,
            semi_stable_tokens: 0,
            dynamic_tokens: 800,
            tools_tokens: 4500,
            tool_count: 44,
            tools_deferred_count: Some(0),
            history_tokens: 0,
            total_tokens: 7300,
            context_window: 128_000,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            last_prompt_tokens: 0,
        };
        let text = breakdown.format_report();
        assert!(text.contains("44 on wire, 0 deferred"));
    }

    #[test]
    fn format_report_shows_cache_hit_rate() {
        let breakdown = ContextBudgetBreakdown {
            stable_tokens: 2000,
            semi_stable_tokens: 0,
            dynamic_tokens: 800,
            tools_tokens: 4500,
            tool_count: 13,
            tools_deferred_count: None,
            history_tokens: 0,
            total_tokens: 7300,
            context_window: 128_000,
            cache_read_tokens: 90_000,
            cache_write_tokens: 10_000,
            last_prompt_tokens: 100_000,
        };
        let text = breakdown.format_report();
        assert!(text.contains("cache:"));
        assert!(text.contains("90% hit"), "got: {text}");
    }
}
