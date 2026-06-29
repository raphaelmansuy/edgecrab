//! End-to-end geometry + schema gates for the local inference harness (P6/P7/P8).
//!
//! Cross-crate wiring: config → policy → mutation budget → tool schema annotation.
//! Deterministic fixtures only — no live LM Studio server.

use std::sync::Arc;

use edgecrab_core::local_provider_policy::{
    effective_completion_options, local_tool_turn_absolute_max_tokens,
    local_tool_turn_plan, LocalToolTurnPlan,
};
use edgecrab_tools::{AppConfigRef, ToolContext, ToolRegistry};
use edgecrab_tools::mutation_turn_policy::{
    local_default_max_tool_argument_bytes, local_max_tool_argument_bytes_for_output_tokens,
    LOCAL_TOOL_TURN_ABS_MAX_TOKENS,
};
use edgecrab_tools::registry::{annotate_llm_definitions_for_local_turn, to_llm_definitions};
use edgecrab_types::{Platform, ToolError};
use edgequake_llm::{CompletionOptions, LLMProvider};
use tokio_util::sync::CancellationToken;

const LMSTUDIO_SYNCED_CTX: usize = 262_144;

fn minimal_tool_ctx() -> ToolContext {
    ToolContext {
        task_id: "lh-e2e".into(),
        cwd: std::env::temp_dir(),
        session_id: "lh-e2e-session".into(),
        user_task: None,
        cancel: CancellationToken::new(),
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
    }
}

struct NamedProvider {
    name: &'static str,
    context_length: usize,
    default_output: Option<usize>,
}

impl NamedProvider {
    fn lmstudio(context_length: usize, default_output: Option<usize>) -> Self {
        Self {
            name: "lmstudio",
            context_length,
            default_output,
        }
    }
}

#[async_trait::async_trait]
impl LLMProvider for NamedProvider {
    fn name(&self) -> &str {
        self.name
    }

    fn model(&self) -> &str {
        "qwen/qwen3.6-35b-a3b"
    }

    fn max_context_length(&self) -> usize {
        self.context_length
    }

    fn default_max_output_tokens(&self) -> Option<usize> {
        self.default_output
    }

    async fn complete(
        &self,
        prompt: &str,
    ) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
        Ok(edgequake_llm::LLMResponse::new(prompt, self.model()))
    }

    async fn complete_with_options(
        &self,
        prompt: &str,
        _options: &CompletionOptions,
    ) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
        self.complete(prompt).await
    }

    async fn chat(
        &self,
        _messages: &[edgequake_llm::ChatMessage],
        _options: Option<&CompletionOptions>,
    ) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
        Ok(edgequake_llm::LLMResponse::new("", self.model()))
    }
}

/// **LH-60** — config `max_tool_turn_tokens` resolves before compile-time default (8192).
#[test]
fn lh60_e2e_config_max_tokens_and_arg_geometry_chain() {
    assert_eq!(LOCAL_TOOL_TURN_ABS_MAX_TOKENS, 8192);
    assert_eq!(
        local_tool_turn_absolute_max_tokens(4096),
        4096,
        "yaml config value wins when env unset"
    );
    assert_eq!(
        local_default_max_tool_argument_bytes(),
        local_max_tool_argument_bytes_for_output_tokens(8192)
    );
    assert_eq!(local_default_max_tool_argument_bytes(), 27_852);
}

/// **LH-60b** — conversation shelf plan line cites live max_arg from geometry chain.
#[test]
fn lh60b_e2e_local_tool_turn_plan_geometry_matches_policy() {
    let provider: Arc<dyn LLMProvider> =
        Arc::new(NamedProvider::lmstudio(LMSTUDIO_SYNCED_CTX, Some(8192)));
    let config_abs_max = local_tool_turn_absolute_max_tokens(8192);
    let options = effective_completion_options(
        &CompletionOptions::default(),
        provider.as_ref(),
        true,
        32 * 1024,
        config_abs_max,
    );
    let plan = local_tool_turn_plan(
        provider.as_ref(),
        &options,
        57_000,
        32 * 1024,
        None,
        config_abs_max,
    )
    .expect("plan");

    assert_eq!(plan.max_tokens, 8192);
    assert_eq!(plan.max_tool_argument_bytes, 27_852);
    assert!(plan.log_line().contains("max_tokens=8192"));
    assert!(plan.log_line().contains("max_arg=27852B"));
    assert_eq!(plan.prompt_tokens_estimated, 57_000);
}

/// **LH-61** — registry → annotate path appends live budget to mutation tools only.
#[test]
fn lh61_e2e_registry_definitions_annotated_for_local_turn() {
    let registry = ToolRegistry::new();
    let ctx = minimal_tool_ctx();
    let schemas = registry.get_definitions(None, None, &ctx);
    let defs = to_llm_definitions(&schemas);

    let write_def = defs
        .iter()
        .find(|d| d.function.name == "write_file")
        .expect("write_file in core toolset");
    assert!(
        !write_def.function.description.contains("Local turn limit"),
        "baseline schema must not embed budget"
    );

    let annotated = annotate_llm_definitions_for_local_turn(
        defs,
        "lmstudio",
        Some((27_852, 8192)),
    );
    let write_annotated = annotated
        .iter()
        .find(|d| d.function.name == "write_file")
        .expect("write_file");
    assert!(write_annotated.function.description.contains("27852"));
    assert!(write_annotated.function.description.contains("8192"));

    let read_annotated = annotated
        .iter()
        .find(|d| d.function.name == "read_file")
        .expect("read_file");
    assert!(
        !read_annotated.function.description.contains("Local turn limit"),
        "non-mutation tools unchanged"
    );
}

/// **LH-62** — patch flat object schema + invalid-args enrichment respects `mode`.
#[test]
fn lh62_e2e_patch_flat_schema_invalid_args_enrichment_by_mode() {
    let registry = ToolRegistry::new();
    let ctx = minimal_tool_ctx();
    let patch_schema = registry
        .get_definitions(None, None, &ctx)
        .into_iter()
        .find(|s| s.name == "patch")
        .expect("patch tool registered");
    assert!(
        patch_schema.parameters.get("oneOf").is_none(),
        "P6 flat object schema for LM Studio"
    );
    assert_eq!(
        patch_schema.parameters.get("type").and_then(|v| v.as_str()),
        Some("object")
    );
    let replace_args = r#"{"mode":"replace","path":"tmp/outline.md"}"#;
    let replace_err = ToolError::InvalidArgs {
        tool: "patch".into(),
        message: "missing field `old_string`".into(),
    };
    let replace_payload = registry
        .enrich_invalid_args_error("patch", &replace_err, Some(replace_args))
        .expect("enriched");
    let replace_fields = replace_payload.required_fields.as_ref().expect("fields");
    assert!(replace_fields.iter().any(|f| f == "old_string"));
    assert!(replace_fields.iter().any(|f| f == "new_string"));

    let patch_args = r#"{"mode":"patch","path":"tmp/outline.md"}"#;
    let patch_err = ToolError::InvalidArgs {
        tool: "patch".into(),
        message: "missing field `patch`".into(),
    };
    let patch_payload = registry
        .enrich_invalid_args_error("patch", &patch_err, Some(patch_args))
        .expect("enriched");
    let patch_fields = patch_payload.required_fields.as_ref().expect("fields");
    assert!(patch_fields.iter().any(|f| f == "patch"));
    assert!(
        !patch_fields.iter().any(|f| f == "old_string"),
        "patch branch must not require replace-only fields"
    );
}

/// **LH-62b** — shelf plan type exposes geometry fields used by conversation preflight.
#[test]
fn lh62b_e2e_local_tool_turn_plan_fields_are_stable() {
    let plan = LocalToolTurnPlan {
        provider: "lmstudio".into(),
        model: "qwen".into(),
        max_tokens: 8192,
        reasoning_effort: "none".into(),
        reasoning_overridden: true,
        tool_choice_required: true,
        max_mutation_payload_bytes: 32 * 1024,
        absolute_max_tokens: 8192,
        http_timeout_secs: 600,
        context_length: LMSTUDIO_SYNCED_CTX,
        prompt_tokens_estimated: 57_000,
        max_tool_argument_bytes: 27_852,
        non_streaming: true,
    };
    let line = plan.log_line();
    assert!(line.contains("tool_choice=required"));
    assert!(line.contains("non-streaming"));
    assert!(line.contains("~57k/262k"));
}
