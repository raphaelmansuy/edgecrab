//! Incremental file-mutation policy — single source of truth (DRY / SOLID).
//!
//! First principle: a tool-call argument must fit in **one** provider completion.
//! The transport carries the entire JSON object; oversized args dominate latency on
//! local providers (LM Studio buffers server-side) and routinely hit HTTP timeouts.
//!
//! Budget derivation (not heuristics):
//!   max_arg_bytes = min(configured_mutation_limit, output_token_budget × chars/token × safety)

use edgequake_llm::LLMProvider;

use crate::edit_contract::{DEFAULT_MAX_MUTATION_PAYLOAD_BYTES, DEFAULT_MAX_MUTATION_PAYLOAD_KIB};

/// Conservative chars-per-token estimate for JSON tool arguments.
pub const TOOL_ARG_CHARS_PER_TOKEN: usize = 4;

/// Reserve headroom for JSON framing (`{"path":...}` keys, escaping).
pub const TOOL_ARG_BUDGET_SAFETY_RATIO: f32 = 0.85;

/// Minimum argument budget — below this, incremental edits are impractical.
pub const MIN_TOOL_ARGUMENT_BYTES: usize = 1024;

/// Default completion cap for one local tool turn (deterministic; not failure-adaptive).
///
/// 8192 tokens → ~27 KiB max tool-argument bytes (enough for typical single-file scaffolds).
/// Override at runtime via `EDGECRAB_LOCAL_TOOL_MAX_TOKENS` in edgecrab-core.
pub const LOCAL_TOOL_TURN_ABS_MAX_TOKENS: usize = 8192;

/// Max tool-argument bytes derivable from a fixed output-token budget (geometry formula).
pub fn local_max_tool_argument_bytes_for_output_tokens(output_tokens: usize) -> usize {
    let completion_cap = ((output_tokens * TOOL_ARG_CHARS_PER_TOKEN) as f32
        * TOOL_ARG_BUDGET_SAFETY_RATIO) as usize;
    DEFAULT_MAX_MUTATION_PAYLOAD_BYTES.min(completion_cap.max(MIN_TOOL_ARGUMENT_BYTES))
}

/// Default max tool-argument bytes when no live `LLMProvider` is available (prompt assembly).
pub fn local_default_max_tool_argument_bytes() -> usize {
    local_max_tool_argument_bytes_for_output_tokens(LOCAL_TOOL_TURN_ABS_MAX_TOKENS)
}

/// Fixed JSON envelope overhead (`id`, `name`, escaping) atop argument tokens.
const TOOL_CALL_ENVELOPE_TOKENS: usize = 64;

fn is_local_inference_provider(provider_name: &str) -> bool {
    matches!(provider_name, "lmstudio" | "ollama")
}

/// Provider-stated or context-derived output token ceiling (non-local default path).
pub fn provider_output_token_budget(provider: &dyn LLMProvider) -> usize {
    provider.default_max_output_tokens().unwrap_or_else(|| {
        let ctx = provider.max_context_length();
        if ctx == 0 {
            4096
        } else {
            (ctx / 4).clamp(1024, 8192)
        }
    })
}

/// Output tokens for **one** tool-turn completion — shared by API `max_tokens` and pre-dispatch guards.
///
/// Derivation (deterministic, not failure-adaptive):
///   min(provider_cap, tokens_for_mutation_payload + envelope, absolute_max)
pub fn output_token_budget_for_tool_turn(
    max_mutation_payload_bytes: usize,
    provider: &dyn LLMProvider,
    absolute_max_tokens: usize,
) -> usize {
    let provider_cap = provider_output_token_budget(provider);
    let tokens_for_mutation = max_mutation_payload_bytes
        .div_ceil(TOOL_ARG_CHARS_PER_TOKEN)
        + TOOL_CALL_ENVELOPE_TOKENS;
    provider_cap
        .min(tokens_for_mutation)
        .min(absolute_max_tokens)
}

/// Effective output budget for argument-size checks (local providers use the tool-turn formula).
pub fn effective_output_token_budget(
    max_mutation_payload_bytes: usize,
    provider: Option<&dyn LLMProvider>,
) -> usize {
    match provider {
        Some(p) if is_local_inference_provider(p.name()) => output_token_budget_for_tool_turn(
            max_mutation_payload_bytes,
            p,
            LOCAL_TOOL_TURN_ABS_MAX_TOKENS,
        ),
        Some(p) => provider_output_token_budget(p),
        None => 4096,
    }
}

/// Tools whose arguments routinely carry file bodies or large scripts.
pub fn is_large_payload_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "write_file" | "patch" | "apply_patch" | "execute_code"
    )
}

pub fn is_file_mutation_tool(tool_name: &str) -> bool {
    matches!(tool_name, "write_file" | "patch" | "apply_patch")
}

/// Derive the maximum tool-argument size allowed this turn.
///
/// Combines the configured mutation payload ceiling with the provider's completion
/// token budget (`default_max_output_tokens` or context-derived fallback).
pub fn max_tool_argument_bytes(
    max_mutation_payload_bytes: usize,
    provider: Option<&dyn LLMProvider>,
) -> usize {
    let mutation_cap = max_mutation_payload_bytes.max(MIN_TOOL_ARGUMENT_BYTES);
    let Some(provider) = provider else {
        return mutation_cap;
    };

    let output_tokens = effective_output_token_budget(max_mutation_payload_bytes, Some(provider));

    let completion_cap = ((output_tokens * TOOL_ARG_CHARS_PER_TOKEN) as f32 * TOOL_ARG_BUDGET_SAFETY_RATIO)
        as usize;
    mutation_cap.min(completion_cap.max(MIN_TOOL_ARGUMENT_BYTES))
}

pub fn estimate_argument_tokens(args_json: &str) -> usize {
    let trimmed = args_json.trim();
    if trimmed.is_empty() {
        return 0;
    }
    trimmed.chars().count().div_ceil(TOOL_ARG_CHARS_PER_TOKEN)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolArgumentBudgetViolation {
    pub tool_name: String,
    pub argument_bytes: usize,
    pub max_bytes: usize,
    pub estimated_tokens: usize,
    pub output_token_budget: usize,
}

/// Pre-dispatch guard: reject tool calls whose JSON args cannot fit one completion.
pub fn check_tool_argument_budget(
    tool_name: &str,
    args_json: &str,
    max_mutation_payload_bytes: usize,
    provider: Option<&dyn LLMProvider>,
) -> Result<(), ToolArgumentBudgetViolation> {
    if !is_large_payload_tool(tool_name) {
        return Ok(());
    }

    let argument_bytes = args_json.len();
    let max_bytes = max_tool_argument_bytes(max_mutation_payload_bytes, provider);
    if argument_bytes <= max_bytes {
        return Ok(());
    }

    let output_token_budget = effective_output_token_budget(max_mutation_payload_bytes, provider);

    Err(ToolArgumentBudgetViolation {
        tool_name: tool_name.to_string(),
        argument_bytes,
        max_bytes,
        estimated_tokens: estimate_argument_tokens(args_json),
        output_token_budget,
    })
}

/// Shared byte/token limits for recovery prompt text (DRY).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ToolTurnBudgetHint {
    max_bytes: usize,
    max_kib: usize,
    output_budget: usize,
}

fn tool_turn_budget_hint(
    max_mutation_payload_bytes: usize,
    provider: Option<&dyn LLMProvider>,
) -> ToolTurnBudgetHint {
    let max_bytes = max_tool_argument_bytes(max_mutation_payload_bytes, provider);
    ToolTurnBudgetHint {
        max_bytes,
        max_kib: max_bytes / 1024,
        output_budget: effective_output_token_budget(max_mutation_payload_bytes, provider),
    }
}

/// User-role recovery after `finish_reason=length` with no tool_calls on a tool turn.
pub fn length_without_tools_recovery_message(
    max_mutation_payload_bytes: usize,
    provider: Option<&dyn LLMProvider>,
) -> String {
    let hint = tool_turn_budget_hint(max_mutation_payload_bytes, provider);
    format!(
        "[System: Your previous completion hit max_tokens (finish_reason=length) without \
         emitting tool_calls. The output budget (~{} completion tokens, max {} bytes per tool \
         argument) cannot fit a large monolithic tool payload. Do NOT retry the same oversized \
         call. Use incremental edits: (1) minimal write_file scaffold (≤{} KiB), (2) extend with \
         patch/apply_patch. Each tool call must stay under {} bytes.]",
        hint.output_budget, hint.max_bytes, hint.max_kib, hint.max_bytes
    )
}

/// Suffix appended to mutation-tool descriptions on local provider tool turns.
pub fn local_tool_budget_schema_suffix(max_arg_bytes: usize, max_output_tokens: usize) -> String {
    format!(
        " Local turn limit: max argument ~{max_arg_bytes} bytes (~{max_output_tokens} completion tokens)."
    )
}

/// Stable dynamic-zone block: local provider output geometry (Layer 2).
///
/// WHY separate from cloud `code_editing_guidance`: cloud stable prompt cites the 32 KiB config
/// cap; local tool turns share one completion bounded by `max_output_tokens` × chars/token × safety.
pub fn local_inference_geometry_guidance(max_arg_bytes: usize, max_output_tokens: usize) -> String {
    let max_kib = max_arg_bytes.div_ceil(1024);
    format!(
        "\
## Local Inference Tool Geometry (binding)

You are on a local provider (LM Studio / Ollama). Each tool turn has a hard output ceiling:
  - max completion tokens: {max_output_tokens}
  - max tool-argument size: {max_arg_bytes} bytes (~{max_kib} KiB)

Rules:
  - A tool-call argument must fit in one completion — oversized write_file / execute_code payloads \
will hit max_tokens without parseable tool_calls.
  - For artifacts larger than ~{max_kib} KiB: minimal write_file scaffold, then patch/apply_patch chunks."
    )
}

/// User-role recovery block after stream stall / timeout on a large tool draft.
pub fn stream_interrupted_recovery_message(
    tool_names: &[String],
    max_mutation_payload_bytes: usize,
    provider: Option<&dyn LLMProvider>,
) -> String {
    let hint = tool_turn_budget_hint(max_mutation_payload_bytes, provider);

    if tool_names.is_empty() {
        return format!(
            "[System: Your previous tool-call draft was interrupted before it could complete \
             (common on LM Studio when arguments are large). Do NOT repeat the same oversized call. \
             Use incremental edits instead: (1) write a minimal scaffold with write_file (≤{} KiB), \
             then (2) extend with patch/apply_patch in focused steps. Keep each tool call under \
             {} bytes (~{} completion tokens).]",
            hint.max_kib, hint.max_bytes, hint.output_budget
        );
    }

    let tool_list: Vec<_> = tool_names.iter().take(3).cloned().collect();
    format!(
        "[System: Your previous tool call ({}) was too large or stalled before delivery. \
         Do NOT retry the same call with the same content. Break the work into multiple smaller \
         steps: scaffold with write_file, then patch/apply_patch sections. Each tool call must stay \
         under {} bytes ({} KiB, ~{} output tokens).]",
        tool_list.join(", "),
        hint.max_bytes,
        hint.max_kib,
        hint.output_budget
    )
}

/// Shelf notice when streaming tool-arg generation stalls mid-draft.
pub fn tool_draft_stall_recovery_notice(
    tool_name: &str,
    arg_bytes: usize,
    max_mutation_payload_bytes: usize,
    provider: Option<&dyn LLMProvider>,
) -> String {
    let max_bytes = max_tool_argument_bytes(max_mutation_payload_bytes, provider);
    format!(
        "↻ Tool draft aborted ({tool_name}, {arg_bytes} B args, limit {max_bytes} B) — \
         split into scaffold + patch steps instead of one giant payload"
    )
}

/// Stable prompt block for incremental editing (shared with prompt_builder).
pub fn code_editing_guidance(max_mutation_bytes: usize, max_mutation_kib: usize) -> String {
    format!(
        "\
## Code Editing Execution

When the user asks for a concrete code or file change and the necessary tools are available, inspect the relevant files and apply the edit in the same turn.

Rules:
  - Do not stop at a plan, draft diff, or 'ready for a patch?' unless the user explicitly asked for a plan/options or the requirements are materially ambiguous.
  - Use read/search/LSP tools to gather the minimum context needed, then mutate files with patch or apply_patch; prefer patch for localized edits (SWE-Edit: find-replace is token-efficient for small changes).
  - Create new files directly when the request requires them, but keep the first write small when the file will be substantial.
        - `write_file` overwrites the entire file — use `patch` for targeted edits.
        - Omit `content` only for a genuinely minimal scaffold you will extend immediately with patch/apply_patch.
        - For an existing non-empty file, call `read_file` in the current session before using `write_file`.
    - If a file may have changed after your last read, call `read_file` again before mutating.
  - The file-mutation contract is hard-bounded: each write_file / patch / apply_patch payload must stay at or under {max_mutation_bytes} bytes ({max_mutation_kib} KiB) per call.
  - For large artifacts (presentations, long scripts, game code), NEVER emit one giant write_file or execute_code payload. Strategy: (1) minimal scaffold write_file, (2) sequential patch/apply_patch chunks, (3) verify with read_file.
  - Do not call execute_code as a placeholder — only when you have concrete code to run; keep scripts small and prefer patch for file content.
  - Once the requested edit is complete, stop expanding scope.
  - After editing, report what changed and any verification you ran.
  - Ask before destructive changes outside the user's stated scope."
    )
}

pub fn default_code_editing_guidance() -> String {
    code_editing_guidance(
        DEFAULT_MAX_MUTATION_PAYLOAD_BYTES,
        DEFAULT_MAX_MUTATION_PAYLOAD_KIB,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use edgequake_llm::{ChatMessage, CompletionOptions};
    use std::sync::Arc;

    struct StubProvider {
        context: usize,
        default_output: Option<usize>,
    }

    #[async_trait]
    impl LLMProvider for StubProvider {
        fn name(&self) -> &str {
            "stub"
        }

        fn model(&self) -> &str {
            "stub-model"
        }

        fn max_context_length(&self) -> usize {
            self.context
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
            messages: &[ChatMessage],
            _options: Option<&CompletionOptions>,
        ) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
            Ok(edgequake_llm::LLMResponse::new(
                messages
                    .last()
                    .map(|m| m.content.as_str())
                    .unwrap_or(""),
                self.model(),
            ))
        }
    }

    #[test]
    fn local_tool_turn_output_budget_respects_absolute_cap() {
        struct LocalStub {
            default_output: Option<usize>,
        }

        #[async_trait]
        impl LLMProvider for LocalStub {
            fn name(&self) -> &str {
                "lmstudio"
            }

            fn model(&self) -> &str {
                "qwen-test"
            }

            fn max_context_length(&self) -> usize {
                262_144
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
                _prompt: &str,
                _options: &CompletionOptions,
            ) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
                Ok(edgequake_llm::LLMResponse::new("", self.model()))
            }

            async fn chat(
                &self,
                _messages: &[ChatMessage],
                _options: Option<&CompletionOptions>,
            ) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
                Ok(edgequake_llm::LLMResponse::new("", self.model()))
            }
        }

        let provider: Arc<dyn LLMProvider> = Arc::new(LocalStub {
            default_output: Some(8192),
        });
        let budget = output_token_budget_for_tool_turn(
            32 * 1024,
            provider.as_ref(),
            LOCAL_TOOL_TURN_ABS_MAX_TOKENS,
        );
        assert_eq!(budget, LOCAL_TOOL_TURN_ABS_MAX_TOKENS);
        let max_bytes = max_tool_argument_bytes(32 * 1024, Some(provider.as_ref()));
        assert_eq!(
            max_bytes,
            ((LOCAL_TOOL_TURN_ABS_MAX_TOKENS * TOOL_ARG_CHARS_PER_TOKEN) as f32
                * TOOL_ARG_BUDGET_SAFETY_RATIO) as usize
        );
    }

    #[test]
    fn budget_is_min_of_mutation_and_completion_caps() {
        let provider: Arc<dyn LLMProvider> = Arc::new(StubProvider {
            context: 65_536,
            default_output: Some(2048),
        });
        let max = max_tool_argument_bytes(32 * 1024, Some(provider.as_ref()));
        // 2048 * 4 * 0.85 = 6963
        assert_eq!(max, 6963);
    }

    #[test]
    fn rejects_oversized_write_file_args() {
        let provider: Arc<dyn LLMProvider> = Arc::new(StubProvider {
            context: 65_536,
            default_output: Some(4096),
        });
        let big = "x".repeat(20_000);
        let args = format!(r#"{{"path":"a.md","content":{big:?}}}"#);
        let err = check_tool_argument_budget(
            "write_file",
            &args,
            32 * 1024,
            Some(provider.as_ref()),
        )
        .expect_err("oversized");
        assert_eq!(err.tool_name, "write_file");
        assert!(err.argument_bytes > err.max_bytes);
    }

    #[test]
    fn ignores_non_mutation_tools() {
        assert!(
            check_tool_argument_budget("read_file", &"x".repeat(50_000), 32 * 1024, None).is_ok()
        );
    }

    #[test]
    fn recovery_message_names_tools_and_limits() {
        let msg = stream_interrupted_recovery_message(
            &["write_file".into()],
            32 * 1024,
            None,
        );
        assert!(msg.contains("write_file"));
        assert!(msg.contains("KiB"));
        assert!(msg.contains("scaffold"));
    }

    #[test]
    fn lh20_length_without_tools_recovery_message_cites_exact_max_bytes() {
        let max_bytes = max_tool_argument_bytes(32 * 1024, None);
        let msg = length_without_tools_recovery_message(32 * 1024, None);
        assert!(msg.contains("finish_reason=length"));
        assert!(msg.contains("without emitting tool_calls"));
        assert!(msg.contains(&max_bytes.to_string()));
        assert!(msg.contains("scaffold"));
        assert!(!msg.contains("interrupted"));
        assert!(!msg.contains("draft was interrupted"));
        assert!(!msg.contains("manage_todo_list"));
    }

    #[test]
    fn lh52_local_default_max_tool_argument_bytes_matches_abs_max_geometry() {
        assert_eq!(
            local_default_max_tool_argument_bytes(),
            local_max_tool_argument_bytes_for_output_tokens(LOCAL_TOOL_TURN_ABS_MAX_TOKENS)
        );
        assert_eq!(local_default_max_tool_argument_bytes(), 27_852);
    }

    #[test]
    fn lh54_local_inference_geometry_guidance_cites_arg_cap_not_config_kib() {
        let block = local_inference_geometry_guidance(27_852, LOCAL_TOOL_TURN_ABS_MAX_TOKENS);
        assert!(block.contains("27852"));
        assert!(block.contains("8192"));
        assert!(!block.contains("32768"));
        assert!(!block.contains("manage_todo_list"));
    }
}
