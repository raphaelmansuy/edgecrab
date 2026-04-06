//! # Context Compression — prevents context-window overflow
//!
//! WHY: Long conversations accumulate tokens until they exceed the
//! model's context window. Rather than hard-truncating (which loses
//! important early context), we summarize old messages while preserving
//! the most recent ones verbatim.
//!
//! ```text
//!   [system] [msg1] [msg2] ... [msgN-20] [msgN-19] ... [msgN]
//!    ↑ keep    └──── prune tools ───────┘
//!               └─── llm_summarize ─────┘  └── keep last 20 ──┘
//! ```
//!
//! Pipeline (v0.4.0 — matching hermes-agent 0.4.x):
//!
//! 1. **Tool output pruning** — replace gigantic tool results in old
//!    messages with `PRUNED_TOOL_PLACEHOLDER` (cheap, no LLM needed).
//!    This alone often halves the prompt size.
//!
//! 2. **Boundary determination** — tail is token-budget based (walks
//!    backward accumulating token estimates until `threshold × target_ratio`
//!    budget is exhausted), with `protect_last_n` as a floor. Boundaries
//!    are aligned backward to avoid splitting tool_call/tool_result groups.
//!
//! 3. **LLM-powered summary** — calls the provider with a structured
//!    8-section template: Goal / Constraints & Preferences / Progress
//!    (Done / In Progress / Blocked) / Key Decisions / Relevant Files /
//!    Next Steps / Critical Context. Output is prefixed with `SUMMARY_PREFIX`
//!    so the next compression pass can locate and update it (iterative
//!    updates). Summary token budget = content_tokens × 0.20, min 2 000,
//!    max min(context_length × 0.05, 12 000).
//!
//! 4. **Structural fallback** — if the LLM call fails, a structured
//!    stat-based summary is built instead (message counts, excerpts).
//!
//! 5. **Orphan sanitization** — after assembling head + summary + tail,
//!    orphaned tool_result messages (no matching tool_call in history)
//!    are removed and orphaned tool_calls get a stub result injected.
//!
//! ## Context pressure warnings
//!
//! When estimated tokens exceed 85 % of the compression threshold the
//! function returns `CompressionStatus::PressureWarning`. After a
//! successful compression that brings usage below 85 % of threshold the
//! status reverts to `CompressionStatus::Ok`.
//!
//! ```text
//!   compress_with_llm(messages, params, provider)
//!       │
//!       ├── prune_tool_outputs(old_messages)      ← step 1 (cheap)
//!       ├── find prior SUMMARY_PREFIX block?       ← iterative update
//!       │       yes → prepend to transcript
//!       │       no  → fresh summary
//!       ├── llm_summarize(pruned_old) → Ok(text) OR Err
//!       │       ↓ on Err
//!       │   build_summary() [structural fallback]
//!       │
//!       └── [Message::system_summary(SUMMARY_PREFIX + text), ...recent]
//! ```

use std::sync::Arc;

use edgecrab_types::Message;
use edgequake_llm::LLMProvider;

use crate::config::CompressionConfig;
use crate::model_catalog::ModelCatalog;

// ─── Constants ────────────────────────────────────────────────────────

/// Prefix for LLM-generated compaction summaries.
///
/// WHY a recognisable prefix: The next compression pass can locate this
/// message and feed it back to the LLM as "prior summary" context so the
/// model produces an *update* rather than starting from scratch. This
/// means summaries improve with each subsequent compaction.
pub const SUMMARY_PREFIX: &str =
    "[CONTEXT COMPACTION] Earlier turns were summarised to reclaim context window space.\n\n";

/// Replacement text for pruned tool output blocks.
///
/// WHY prune first: Tool results (file contents, shell output) can be
/// thousands of tokens each. Replacing them before the LLM call keeps
/// the summarisation prompt itself small — no recursion risk.
pub const PRUNED_TOOL_PLACEHOLDER: &str = "[tool output pruned — reclaimed context window space]";

/// Number of head messages (system prompt + first exchange) always preserved.
/// Matches hermes-agent's `protect_first_n = 3` constant.
const PROTECT_FIRST_N: usize = 3;

/// Minimum tokens for the LLM summary budget.
const MIN_SUMMARY_TOKENS: usize = 2_000;

/// Summary token budget as a fraction of compressed content tokens.
const SUMMARY_RATIO: f32 = 0.20;

/// Hard ceiling on summary tokens (absolute maximum).
const SUMMARY_TOKENS_CEILING: usize = 12_000;

/// Approximate characters per token for rough estimation without a tokenizer.
const CHARS_PER_TOKEN: usize = 4;

/// Stub text injected for orphaned tool_calls after compression.
const STUB_TOOL_RESULT: &str = "[Result from earlier conversation — see context summary above]";

/// 8-section structured summary template (hermes-agent 0.4.x format).
const SUMMARY_TEMPLATE: &str = "\
## Goal
[What the user is trying to accomplish]

## Constraints & Preferences
[User preferences, coding style, constraints, important decisions]

## Progress
### Done
[Completed work — include specific file paths, commands run, results obtained]
### In Progress
[Work currently underway]
### Blocked
[Any blockers or issues encountered]

## Key Decisions
[Important technical decisions and why they were made]

## Relevant Files
[Files read, modified, or created — with brief note on each]

## Next Steps
[What needs to happen next to continue the work]

## Critical Context
[Any specific values, error messages, configuration details, or data that would be lost without explicit preservation]";

/// Configuration for context compression.
#[derive(Debug, Clone)]
pub struct CompressionParams {
    /// Estimated context window size for the target model.
    pub context_window: usize,
    /// Compress when estimated tokens exceed this fraction of the window.
    /// Default 0.50 (50 %). Threshold tokens = context_window × threshold.
    pub threshold: f32,
    /// Tail budget ratio: tail_token_budget = threshold_tokens × target_ratio.
    /// Controls how many tokens the "protected recent messages" tail may use.
    /// Default 0.20. Falls back to protect_last_n when the budget would keep
    /// fewer than protect_last_n messages.
    pub target_ratio: f32,
    /// Minimum number of recent messages always kept uncompressed.
    /// Default 20. Acts as a floor when token-budget tail selection would
    /// protect fewer messages.
    pub protect_last_n: usize,
}

const DEFAULT_CONTEXT_WINDOW: usize = 128_000;

impl Default for CompressionParams {
    fn default() -> Self {
        Self {
            context_window: DEFAULT_CONTEXT_WINDOW,
            threshold: 0.50,
            target_ratio: 0.20,
            protect_last_n: 20,
        }
    }
}

impl CompressionParams {
    /// Resolve compression parameters for the active model/configuration.
    pub fn from_model_config(model: &str, cfg: &CompressionConfig) -> Self {
        let context_window = model
            .split_once('/')
            .and_then(|(provider, name)| ModelCatalog::context_window(provider, name))
            .map(|tokens| tokens as usize)
            .unwrap_or(DEFAULT_CONTEXT_WINDOW);

        Self {
            context_window,
            threshold: cfg.threshold.clamp(0.01, 1.0),
            target_ratio: cfg.target_ratio.clamp(0.01, 1.0),
            protect_last_n: cfg.protect_last_n.max(1),
        }
    }
}

/// Result of a compression trigger check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionStatus {
    /// Token usage is below the warning threshold.
    Ok,
    /// Approaching compaction: tokens > 85 % of threshold.
    /// Emitted as a UI warning before compression fires.
    PressureWarning,
    /// Compression should fire: tokens ≥ threshold.
    NeedsCompression,
}

/// Estimate token count for a message list.
///
/// WHY ~4 chars/token: This is a rough heuristic that works well
/// for English text across GPT/Claude tokenizers. It's fast (no
/// tokenizer dependency) and good enough for the compression
/// threshold check. Exact counts come from the API response.
pub fn estimate_tokens(messages: &[Message]) -> usize {
    messages
        .iter()
        .map(|m| {
            let text_len = m.text_content().len();
            // ~4 chars per token + overhead per message
            (text_len / 4) + 4
        })
        .sum()
}

/// Check if compression is needed.
pub fn needs_compression(messages: &[Message], params: &CompressionParams) -> bool {
    matches!(
        check_compression_status(messages, params),
        CompressionStatus::NeedsCompression
    )
}

/// Full compression status check with pressure warning.
///
/// Returns:
/// - `Ok` — below 85 % of threshold
/// - `PressureWarning` — between 85 % and 100 % of threshold (UI warning)
/// - `NeedsCompression` — at or above threshold (compression should fire)
pub fn check_compression_status(
    messages: &[Message],
    params: &CompressionParams,
) -> CompressionStatus {
    let estimated = estimate_tokens(messages);
    check_compression_status_for_estimate(estimated, params)
}

/// Classify compression pressure from a precomputed token estimate.
pub fn check_compression_status_for_estimate(
    estimated: usize,
    params: &CompressionParams,
) -> CompressionStatus {
    let threshold_tokens = (params.context_window as f32 * params.threshold) as usize;
    let warning_tokens = (threshold_tokens as f32 * 0.85) as usize;

    if estimated >= threshold_tokens {
        CompressionStatus::NeedsCompression
    } else if estimated >= warning_tokens {
        CompressionStatus::PressureWarning
    } else {
        CompressionStatus::Ok
    }
}

/// Perform simple compression: summarize old messages into a single
/// system-level summary, keeping the last N messages intact.
///
/// Returns the compressed message list. The summary message is a
/// placeholder — in production, this would call a cheaper LLM to
/// generate a real summary.
///
/// WHY simple truncation for Phase 1: Full LLM-based summarization
/// requires an async call to a summary model and careful chunking.
/// This is deferred to Phase 2. For now, we produce a structured
/// summary stub that preserves the message structure.
pub fn compress_messages(messages: &[Message], params: &CompressionParams) -> Vec<Message> {
    if messages.len() <= params.protect_last_n {
        return messages.to_vec();
    }

    let split_point = messages.len().saturating_sub(params.protect_last_n);
    let old_messages = &messages[..split_point];
    let recent_messages = &messages[split_point..];

    // Build a structured summary of the old messages
    let summary = build_summary(old_messages);

    let mut compressed = Vec::with_capacity(1 + recent_messages.len());
    compressed.push(Message::system_summary(summary));
    compressed.extend_from_slice(recent_messages);

    compressed
}

/// Build a text summary of messages (simple extraction, no LLM).
///
/// Extracts key information: user questions, assistant conclusions,
/// tool calls made. This is a structural summary — the LLM-based
/// summary (Phase 2) will produce a more coherent narrative.
fn build_summary(messages: &[Message]) -> String {
    let mut parts = Vec::new();
    parts.push("[Context Summary — earlier messages compressed]".to_string());

    let mut user_count = 0u32;
    let mut assistant_count = 0u32;
    let mut tool_count = 0u32;

    for m in messages {
        match m.role {
            edgecrab_types::Role::User => user_count += 1,
            edgecrab_types::Role::Assistant => assistant_count += 1,
            edgecrab_types::Role::Tool => tool_count += 1,
            edgecrab_types::Role::System => {}
        }
    }

    parts.push(format!(
        "Compressed {user_count} user messages, {assistant_count} assistant \
         responses, and {tool_count} tool results."
    ));

    // Include the first user message for context
    if let Some(first_user) = messages
        .iter()
        .find(|m| m.role == edgecrab_types::Role::User)
    {
        let preview = first_user.text_content();
        let truncated = if preview.len() > 200 {
            format!("{}...", crate::safe_truncate(&preview, 200))
        } else {
            preview
        };
        parts.push(format!("First user message: {truncated}"));
    }

    parts.join("\n")
}

// ─── LLM-powered compression ──────────────────────────────────────────

/// LLM-powered context compression (v0.4.0 — hermes-agent parity).
///
/// WHY LLM summarization > structural: A structural summary preserves
/// message counts but loses semantic meaning. An LLM summary produces a
/// coherent narrative the model can use to reason about earlier state.
///
/// Pipeline (6 phases — mirrors hermes-agent `context_compressor.py`):
/// 1. **Prune** — replace large tool outputs with placeholders (cheap, no LLM).
/// 2. **Boundary** — determine head/tail by token-budget walk; align both
///    boundaries to avoid splitting tool_call/tool_result groups.
/// 3. **Prior** — extract any existing `SUMMARY_PREFIX` block for iterative update.
/// 4. **Summarise** — call LLM with 8-section template; fall back to structural
///    summary on LLM failure (never silently drops context).
/// 5. **Assemble** — head messages + summary message + tail messages.
/// 6. **Sanitize** — remove orphaned tool results; inject stub results for
///    orphaned tool_calls so the assembled list is always API-compliant.
pub async fn compress_with_llm(
    messages: &[Message],
    params: &CompressionParams,
    provider: &Arc<dyn LLMProvider>,
) -> Vec<Message> {
    let n = messages.len();
    // Need at least: protected head + 1 message to summarise + protected tail.
    if n <= PROTECT_FIRST_N + params.protect_last_n {
        return messages.to_vec();
    }

    // Phase 1: prune tool outputs (cheap, no LLM).
    let pruned = prune_tool_outputs(messages);

    // Phase 2: determine compression boundaries.
    // Head: always keep system prompt + first exchange (PROTECT_FIRST_N messages).
    let head_end = align_boundary_forward(&pruned, PROTECT_FIRST_N);
    // Tail: walk backward until token budget exhausted.
    let threshold_tokens = (params.context_window as f32 * params.threshold) as usize;
    let tail_token_budget = (threshold_tokens as f32 * params.target_ratio) as usize;
    let tail_start =
        find_tail_cut_by_tokens(&pruned, head_end, tail_token_budget, params.protect_last_n);

    if head_end >= tail_start {
        // Nothing in the middle — history is too short to compress.
        return messages.to_vec();
    }

    let turns_to_summarize = &pruned[head_end..tail_start];

    // Phase 3: extract prior summary for iterative update.
    let prior_summary = extract_prior_summary(messages);

    // Phase 4: LLM summarization with 8-section template.
    let summary_text = llm_summarize(
        turns_to_summarize,
        params.context_window,
        provider,
        prior_summary.as_deref(),
    )
    .await
    .unwrap_or_else(|e: edgequake_llm::LlmError| {
        tracing::warn!(error = %e, "LLM compression failed, using structural fallback");
        build_summary(turns_to_summarize)
    });

    // Phase 5: assemble head + summary + tail.
    let prefixed = format!("{SUMMARY_PREFIX}{summary_text}");
    let mut result = Vec::with_capacity(head_end + 1 + (n - tail_start));
    result.extend_from_slice(&pruned[..head_end]);
    result.push(Message::system_summary(prefixed));
    result.extend_from_slice(&pruned[tail_start..]);

    // Phase 6: fix orphaned tool pairs.
    sanitize_orphan_pairs(result)
}

/// Replace large tool-result messages with a placeholder.
///
/// WHY: Tool outputs (file contents, grep results, command output) are
/// often thousands of tokens. Replacing them with a 10-token placeholder
/// before summarisation halves the LLM input cost at no semantic loss —
/// the summary will describe *what* the tool found, not dump raw bytes.
///
/// Threshold: tool results over 200 chars are pruned. This preserves
/// short "ok" / "error" responses that carry semantic meaning.
pub fn prune_tool_outputs(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .map(|m| {
            if m.role == edgecrab_types::Role::Tool && m.text_content().len() > 200 {
                // Keep the tool_call_id / tool_name metadata, replace body.
                Message::tool_result(
                    m.tool_call_id.as_deref().unwrap_or("unknown"),
                    m.name.as_deref().unwrap_or("tool"),
                    PRUNED_TOOL_PLACEHOLDER,
                )
            } else {
                m.clone()
            }
        })
        .collect()
}

/// Extract the text of the most recent SUMMARY_PREFIX block, if any.
///
/// WHY: Iterative update means the second compression pass feeds the
/// prior summary back to the LLM as existing context. The LLM can then
/// produce an *incremental update* rather than re-summarising everything
/// from scratch, which is both cheaper and more coherent.
fn extract_prior_summary(messages: &[Message]) -> Option<String> {
    messages
        .iter()
        .find(|m| {
            m.role == edgecrab_types::Role::System && m.text_content().starts_with(SUMMARY_PREFIX)
        })
        .map(|m| {
            m.text_content()
                .strip_prefix(SUMMARY_PREFIX)
                .unwrap_or(&m.text_content())
                .to_string()
        })
}

// ─── Boundary alignment helpers ──────────────────────────────────────

/// Slide `idx` forward past any leading tool-result messages.
///
/// WHY: If the head boundary lands on a tool result, the preceding
/// assistant tool_call has been preserved but the result would fall into
/// the middle (summarized) region, splitting the pair. Moving forward
/// ensures we start at a clean message boundary.
fn align_boundary_forward(messages: &[Message], idx: usize) -> usize {
    let mut i = idx;
    while i < messages.len() && messages[i].role == edgecrab_types::Role::Tool {
        i += 1;
    }
    i
}

/// Pull `idx` backward past any trailing tool results to the parent assistant.
///
/// WHY: If the tail-start boundary falls inside a tool_call/result group,
/// dropping the parent assistant message would create orphaned tool results
/// that the API rejects. Walking backward to the parent assistant ensures
/// the whole group is either kept or summarized together.
fn align_boundary_backward(messages: &[Message], idx: usize) -> usize {
    if idx == 0 || idx >= messages.len() {
        return idx;
    }
    // Walk backward past consecutive tool results.
    let mut check = idx.saturating_sub(1);
    while check > 0 && messages[check].role == edgecrab_types::Role::Tool {
        check -= 1;
    }
    // If the parent is an assistant with tool_calls, pull boundary before it.
    if messages[check].role == edgecrab_types::Role::Assistant && messages[check].has_tool_calls() {
        check
    } else {
        idx
    }
}

// ─── Token-budget tail selection ─────────────────────────────────────

/// Walk backward from the end of `messages`, accumulating token estimates,
/// and return the index where the protected tail starts.
///
/// WHY token-budget tail instead of fixed `protect_last_n`: A fixed count
/// fails on large models (20 short messages ≪ 20 K tokens) and on small
/// ones (20 long tool outputs may fill the context window). A budget-scaled
/// tail self-adjusts to model context size and message density.
///
/// Falls back to `protect_last_n` if the budget would protect the entire
/// history (small conversation) or fewer than `protect_last_n` messages.
fn find_tail_cut_by_tokens(
    messages: &[Message],
    head_end: usize,
    token_budget: usize,
    protect_last_n: usize,
) -> usize {
    let n = messages.len();
    let mut accumulated: usize = 0;
    let mut cut_idx = n;

    for i in (head_end..n).rev() {
        let msg_tokens = messages[i].text_content().len() / CHARS_PER_TOKEN + 10;
        let protected_count = n - i;
        if accumulated + msg_tokens > token_budget && protected_count >= protect_last_n {
            break;
        }
        accumulated += msg_tokens;
        cut_idx = i;
    }

    // Enforce minimum tail of `protect_last_n` messages.
    let fallback = n.saturating_sub(protect_last_n);
    let cut_idx = cut_idx.min(fallback);

    // If budget swallowed everything (small history), use fixed fallback.
    let cut_idx = if cut_idx <= head_end {
        fallback
    } else {
        cut_idx
    };

    // Align: never split a tool_call/tool_result group at the tail boundary.
    let cut_idx = align_boundary_backward(messages, cut_idx);

    // Always leave at least one message in the middle to compress.
    cut_idx.max(head_end + 1)
}

// ─── Orphan pair sanitization ─────────────────────────────────────────

/// Fix orphaned tool_call / tool_result pairs after assembling the compressed list.
///
/// Two failure modes that this resolves:
/// 1. A tool *result* references a call_id whose parent assistant `tool_call`
///    was summarized away → API rejects "No tool_call found for call_id …".
/// 2. An assistant message has `tool_calls` whose results were dropped →
///    API rejects because every tool_call must have a matching result message.
///
/// Removes orphaned results (case 1) and injects one-line stub results for
/// orphaned calls (case 2) so the assembled list is always API-compliant.
fn sanitize_orphan_pairs(messages: Vec<Message>) -> Vec<Message> {
    use std::collections::HashSet;

    // Surviving call IDs present in assistant messages.
    let call_ids: HashSet<String> = messages
        .iter()
        .filter(|m| m.role == edgecrab_types::Role::Assistant)
        .flat_map(|m| m.tool_calls.iter().flatten().map(|tc| tc.id.clone()))
        .collect();

    // Call IDs referenced by existing tool result messages.
    let result_ids: HashSet<String> = messages
        .iter()
        .filter(|m| m.role == edgecrab_types::Role::Tool)
        .filter_map(|m| m.tool_call_id.clone())
        .collect();

    // Phase 1: drop orphaned tool results (result references a missing call).
    let orphaned_results: HashSet<String> = result_ids.difference(&call_ids).cloned().collect();
    let messages: Vec<Message> = if orphaned_results.is_empty() {
        messages
    } else {
        tracing::debug!(
            count = orphaned_results.len(),
            "sanitizer: dropped orphaned tool results"
        );
        messages
            .into_iter()
            .filter(|m| {
                m.role != edgecrab_types::Role::Tool
                    || m.tool_call_id
                        .as_ref()
                        .map(|id| !orphaned_results.contains(id))
                        .unwrap_or(true)
            })
            .collect()
    };

    // Rebuild remaining result IDs after phase-1 filtering.
    let result_ids_after: HashSet<String> = messages
        .iter()
        .filter(|m| m.role == edgecrab_types::Role::Tool)
        .filter_map(|m| m.tool_call_id.clone())
        .collect();

    // Phase 2: inject stub results for tool_calls that lost their result.
    let missing_results: HashSet<String> =
        call_ids.difference(&result_ids_after).cloned().collect();
    if missing_results.is_empty() {
        return messages;
    }

    tracing::debug!(
        count = missing_results.len(),
        "sanitizer: injected stub tool results"
    );
    let mut patched = Vec::with_capacity(messages.len() + missing_results.len());
    for m in messages {
        let is_assistant = m.role == edgecrab_types::Role::Assistant;
        let tool_calls = m.tool_calls.clone();
        patched.push(m);
        if is_assistant {
            if let Some(tcs) = tool_calls {
                for tc in tcs {
                    if missing_results.contains(&tc.id) {
                        patched.push(Message::tool_result(
                            &tc.id,
                            &tc.function.name,
                            STUB_TOOL_RESULT,
                        ));
                    }
                }
            }
        }
    }
    patched
}

// ─── Summary budget & serialization ──────────────────────────────────

/// Scale the LLM summary token budget with content size and model context window.
///
/// Formula: `content_tokens × SUMMARY_RATIO`, clamped to
/// `[MIN_SUMMARY_TOKENS, min(context_window × 0.05, SUMMARY_TOKENS_CEILING)]`.
///
/// WHY scaled not fixed: Small conversations need small summaries; large-context
/// models (200 K+ tokens) deserve richer summaries. The ceiling prevents cost runaway.
fn compute_summary_budget(content_tokens: usize, context_window: usize) -> usize {
    let budget = (content_tokens as f32 * SUMMARY_RATIO) as usize;
    let ceiling = ((context_window as f32 * 0.05) as usize).min(SUMMARY_TOKENS_CEILING);
    budget.max(MIN_SUMMARY_TOKENS).min(ceiling)
}

/// Serialize conversation turns into labeled text for the summarizer LLM.
///
/// Includes tool call arguments and result content (truncated to 3 000 chars
/// per message) so the summarizer can capture file paths, commands, outputs.
/// System messages are excluded because they are not conversation history.
fn serialize_for_summary(messages: &[Message]) -> String {
    const MAX_MSG_CHARS: usize = 3_000;
    const HEAD_CHARS: usize = 2_000;
    const TAIL_CHARS: usize = 800;

    messages
        .iter()
        .filter(|m| m.role != edgecrab_types::Role::System)
        .map(|m| {
            let text = m.text_content();
            let content = if text.len() > MAX_MSG_CHARS {
                let head = crate::safe_truncate(&text, HEAD_CHARS.min(text.len()));
                let tail_start =
                    crate::safe_char_start(&text, text.len().saturating_sub(TAIL_CHARS));
                format!("{}…[truncated]…{}", head, &text[tail_start..])
            } else {
                text
            };
            match m.role {
                edgecrab_types::Role::Tool => {
                    let id = m.tool_call_id.as_deref().unwrap_or("");
                    format!("[TOOL RESULT {id}]: {content}")
                }
                edgecrab_types::Role::Assistant => {
                    let mut line = format!("[ASSISTANT]: {content}");
                    if let Some(tcs) = &m.tool_calls {
                        let calls: Vec<String> = tcs
                            .iter()
                            .map(|tc| {
                                let args = if tc.function.arguments.len() > 500 {
                                    format!(
                                        "{}…",
                                        crate::safe_truncate(&tc.function.arguments, 400)
                                    )
                                } else {
                                    tc.function.arguments.clone()
                                };
                                format!("  {}({})", tc.function.name, args)
                            })
                            .collect();
                        line.push_str("\n[Tool calls:\n");
                        line.push_str(&calls.join("\n"));
                        line.push(']');
                    }
                    line
                }
                edgecrab_types::Role::User => format!("[USER]: {content}"),
                edgecrab_types::Role::System => unreachable!("filtered above"),
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

// ─── LLM summarization ────────────────────────────────────────────────

/// Call the provider to produce a structured 8-section summary of old messages.
///
/// Sections: Goal / Constraints & Preferences / Progress (Done / In Progress /
/// Blocked) / Key Decisions / Relevant Files / Next Steps / Critical Context.
///
/// When `prior_summary` is `Some`, the prompt asks for an *iterative update*
/// rather than a fresh summary — cheaper, more coherent across repeated passes.
///
/// `max_tokens` = `compute_summary_budget(content_tokens, context_window) × 2`
/// to give the model headroom; the provider truncates the response if needed.
async fn llm_summarize(
    messages: &[Message],
    context_window: usize,
    provider: &Arc<dyn LLMProvider>,
    prior_summary: Option<&str>,
) -> Result<String, edgequake_llm::LlmError> {
    let content = serialize_for_summary(messages);
    let content_tokens = estimate_tokens(messages);
    let summary_budget = compute_summary_budget(content_tokens, context_window);

    let prompt = match prior_summary {
        Some(prior) => format!(
            "You are updating a context compaction summary. A previous compaction produced \
             the summary below. New conversation turns have occurred since then and need to \
             be incorporated.\n\n\
             PREVIOUS SUMMARY:\n{prior}\n\n\
             NEW TURNS TO INCORPORATE:\n{content}\n\n\
             Update the summary using this exact structure. PRESERVE all existing information \
             that is still relevant. ADD new progress. Move items from \"In Progress\" to \
             \"Done\" when completed. Remove information only if it is clearly obsolete.\n\n\
             {SUMMARY_TEMPLATE}\n\n\
             Target ~{summary_budget} tokens. Be specific — include file paths, command \
             outputs, error messages, and concrete values rather than vague descriptions.\n\n\
             Write only the summary body. Do not include any preamble or prefix."
        ),
        None => format!(
            "Create a structured handoff summary for a later assistant that will continue \
             this conversation after earlier turns are compacted.\n\n\
             TURNS TO SUMMARIZE:\n{content}\n\n\
             Use this exact structure:\n\n\
             {SUMMARY_TEMPLATE}\n\n\
             Target ~{summary_budget} tokens. Be specific — include file paths, command \
             outputs, error messages, and concrete values rather than vague descriptions. \
             The goal is to prevent the next assistant from repeating work or losing \
             important details.\n\n\
             Write only the summary body. Do not include any preamble or prefix."
        ),
    };

    let options = edgequake_llm::CompletionOptions {
        max_tokens: Some(summary_budget * 2),
        temperature: Some(0.3),
        ..Default::default()
    };
    let llm_messages = vec![edgequake_llm::ChatMessage::user(&prompt)];
    let response = provider.chat(&llm_messages, Some(&options)).await?;
    Ok(response.content.trim().to_string())
}

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_messages(n: usize) -> Vec<Message> {
        (0..n)
            .map(|i| {
                if i % 2 == 0 {
                    Message::user(&format!("question {i}"))
                } else {
                    Message::assistant(&format!("answer {i}"))
                }
            })
            .collect()
    }

    #[test]
    fn estimate_tokens_basic() {
        let msgs = vec![Message::user("hello world")]; // 11 chars → ~2 tokens + 4 overhead
        let tokens = estimate_tokens(&msgs);
        assert!(tokens > 0);
        assert!(tokens < 20);
    }

    #[test]
    fn needs_compression_under_threshold() {
        let msgs = make_messages(5);
        let params = CompressionParams {
            context_window: 128_000,
            threshold: 0.50,
            target_ratio: 0.20,
            protect_last_n: 20,
        };
        assert!(!needs_compression(&msgs, &params));
    }

    #[test]
    fn needs_compression_over_threshold() {
        let msgs: Vec<Message> = (0..1000)
            .map(|i| Message::user(&format!("{}{}", "a".repeat(500), i)))
            .collect();
        let params = CompressionParams {
            context_window: 1000, // small window
            threshold: 0.10,
            target_ratio: 0.20,
            protect_last_n: 5,
        };
        assert!(needs_compression(&msgs, &params));
    }

    #[test]
    fn check_status_pressure_warning() {
        // threshold_tokens = 1000 * 0.50 = 500; warning_tokens = 500 * 0.85 = 425.
        // We need estimate > 425 and < 500.
        // estimate_tokens for one 1700-char message = 1700/4 + 4 = 429. ✓
        let msgs = vec![Message::user(&"x".repeat(1_700))];
        let params = CompressionParams {
            context_window: 1_000,
            threshold: 0.50,
            target_ratio: 0.20,
            protect_last_n: 5,
        };
        assert_eq!(
            check_compression_status(&msgs, &params),
            CompressionStatus::PressureWarning
        );
    }

    #[test]
    fn check_status_needs_compression() {
        let msgs: Vec<Message> = (0..1000)
            .map(|i| Message::user(&"a".repeat(500 + i)))
            .collect();
        let params = CompressionParams {
            context_window: 1_000,
            threshold: 0.10,
            target_ratio: 0.20,
            protect_last_n: 5,
        };
        assert_eq!(
            check_compression_status(&msgs, &params),
            CompressionStatus::NeedsCompression
        );
    }

    #[test]
    fn check_status_ok_below_warning() {
        let msgs = make_messages(2);
        let params = CompressionParams::default();
        assert_eq!(
            check_compression_status(&msgs, &params),
            CompressionStatus::Ok
        );
    }

    #[test]
    fn check_status_for_estimate_reuses_threshold_logic() {
        let params = CompressionParams {
            context_window: 1_000,
            threshold: 0.50,
            target_ratio: 0.20,
            protect_last_n: 5,
        };
        assert_eq!(
            check_compression_status_for_estimate(430, &params),
            CompressionStatus::PressureWarning
        );
        assert_eq!(
            check_compression_status_for_estimate(500, &params),
            CompressionStatus::NeedsCompression
        );
    }

    #[test]
    fn compression_params_from_model_config_uses_runtime_values() {
        let cfg = CompressionConfig {
            enabled: true,
            threshold: 0.75,
            target_ratio: 0.33,
            protect_last_n: 12,
            summary_model: None,
        };
        let params = CompressionParams::from_model_config("anthropic/claude-opus-4.6", &cfg);
        assert_eq!(params.threshold, 0.75);
        assert_eq!(params.target_ratio, 0.33);
        assert_eq!(params.protect_last_n, 12);
        assert_eq!(
            params.context_window,
            ModelCatalog::context_window("anthropic", "claude-opus-4.6").expect("catalog context")
                as usize
        );
    }

    #[test]
    fn compress_preserves_recent() {
        let msgs = make_messages(30);
        let params = CompressionParams {
            protect_last_n: 10,
            ..Default::default()
        };

        let compressed = compress_messages(&msgs, &params);
        // 1 summary + 10 recent = 11
        assert_eq!(compressed.len(), 11);

        // First message should be the summary
        assert_eq!(compressed[0].role, edgecrab_types::Role::System);
        assert!(compressed[0].text_content().contains("Context Summary"));

        // Last message should be the last original message
        assert_eq!(
            compressed.last().expect("last").text_content(),
            msgs.last().expect("last").text_content()
        );
    }

    #[test]
    fn compress_small_history_is_noop() {
        let msgs = make_messages(5);
        let params = CompressionParams {
            protect_last_n: 20,
            ..Default::default()
        };
        let compressed = compress_messages(&msgs, &params);
        assert_eq!(compressed.len(), msgs.len());
    }

    #[test]
    fn summary_contains_counts() {
        let msgs = make_messages(10);
        let summary = build_summary(&msgs);
        assert!(summary.contains("5 user messages"));
        assert!(summary.contains("5 assistant responses"));
    }

    // ── Boundary helpers ──────────────────────────────────────────────

    #[test]
    fn align_forward_skips_leading_tool_messages() {
        let msgs = vec![
            Message::user("q"),
            Message::tool_result("c1", "t", "r1"),
            Message::tool_result("c2", "t", "r2"),
            Message::user("follow-up"),
        ];
        assert_eq!(align_boundary_forward(&msgs, 1), 3);
        assert_eq!(align_boundary_forward(&msgs, 0), 0);
        assert_eq!(align_boundary_forward(&msgs, 4), 4); // past end
    }

    #[test]
    fn align_backward_pulls_before_assistant_with_tool_calls() {
        let tc = edgecrab_types::ToolCall {
            id: "c1".into(),
            r#type: "function".into(),
            function: edgecrab_types::FunctionCall {
                name: "my_tool".into(),
                arguments: "{}".into(),
            },
            thought_signature: None,
        };
        let msgs = vec![
            Message::user("q"),
            Message::assistant_with_tool_calls("", vec![tc]),
            Message::tool_result("c1", "my_tool", "result"),
            Message::user("next"),
        ];
        // Boundary at index 3 should pull before the assistant (index 1).
        assert_eq!(align_boundary_backward(&msgs, 3), 1);
        // Edge cases: 0 and past-end stay unchanged.
        assert_eq!(align_boundary_backward(&msgs, 0), 0);
    }

    #[test]
    fn align_backward_noop_without_tool_calls() {
        // Assistant without tool_calls — boundary should not move.
        let msgs = vec![
            Message::user("q"),
            Message::assistant("a"),
            Message::user("next"),
        ];
        assert_eq!(align_boundary_backward(&msgs, 2), 2);
    }

    #[test]
    fn find_tail_cut_returns_more_than_head_end() {
        let msgs = make_messages(10);
        let cut = find_tail_cut_by_tokens(&msgs, 2, 0, 2);
        assert!(cut > 2, "cut={cut} must be > head_end=2");
        assert!(cut <= msgs.len());
    }

    #[test]
    fn find_tail_cut_respects_protect_last_n() {
        let msgs = make_messages(20);
        // With a huge budget, fallback to protect_last_n=5.
        let cut = find_tail_cut_by_tokens(&msgs, 0, usize::MAX, 5);
        // cut should be at most n - protect_last_n = 15
        assert!(cut <= 15, "cut={cut}");
    }

    // ── Orphan sanitization ───────────────────────────────────────────

    #[test]
    fn sanitize_removes_orphaned_tool_result() {
        // Tool result with no matching assistant tool_call → removed.
        let messages = vec![
            Message::user("do something"),
            Message::tool_result("call_999", "some_tool", "output"),
        ];
        let sanitized = sanitize_orphan_pairs(messages);
        assert_eq!(sanitized.len(), 1);
        assert_eq!(sanitized[0].role, edgecrab_types::Role::User);
    }

    #[test]
    fn sanitize_injects_stub_for_missing_tool_result() {
        // Assistant with tool_call but no matching result → stub injected.
        let tc = edgecrab_types::ToolCall {
            id: "call_1".into(),
            r#type: "function".into(),
            function: edgecrab_types::FunctionCall {
                name: "my_tool".into(),
                arguments: "{}".into(),
            },
            thought_signature: None,
        };
        let messages = vec![
            Message::user("do something"),
            Message::assistant_with_tool_calls("", vec![tc]),
        ];
        let sanitized = sanitize_orphan_pairs(messages);
        // user + assistant + stub tool result
        assert_eq!(sanitized.len(), 3);
        assert_eq!(sanitized[2].role, edgecrab_types::Role::Tool);
        assert_eq!(sanitized[2].tool_call_id.as_deref(), Some("call_1"));
        assert!(sanitized[2].text_content().contains("earlier conversation"));
    }

    #[test]
    fn sanitize_noop_on_well_formed_pairs() {
        // Well-formed assistant + result → unchanged.
        let tc = edgecrab_types::ToolCall {
            id: "call_x".into(),
            r#type: "function".into(),
            function: edgecrab_types::FunctionCall {
                name: "search".into(),
                arguments: "{}".into(),
            },
            thought_signature: None,
        };
        let messages = vec![
            Message::user("query"),
            Message::assistant_with_tool_calls("", vec![tc]),
            Message::tool_result("call_x", "search", "results"),
        ];
        let len = messages.len();
        let sanitized = sanitize_orphan_pairs(messages);
        assert_eq!(sanitized.len(), len);
    }

    #[test]
    fn sanitize_empty_input_is_noop() {
        let sanitized = sanitize_orphan_pairs(vec![]);
        assert!(sanitized.is_empty());
    }

    // ── Summary budget ─────────────────────────────────────────────────

    #[test]
    fn budget_clamps_to_minimum() {
        // Tiny content → floor at MIN_SUMMARY_TOKENS.
        assert_eq!(compute_summary_budget(10, 128_000), MIN_SUMMARY_TOKENS);
    }

    #[test]
    fn budget_clamps_to_ceiling_from_context() {
        // ceiling = min(128_000 * 0.05, 12_000) = min(6_400, 12_000) = 6_400
        let budget = compute_summary_budget(1_000_000, 128_000);
        assert_eq!(budget, 6_400);
    }

    #[test]
    fn budget_hard_cap_limits_huge_windows() {
        // With a very large context window the 12_000 hard cap must kick in.
        let budget = compute_summary_budget(1_000_000, 4_000_000);
        assert!(budget <= SUMMARY_TOKENS_CEILING, "budget={budget}");
    }

    // ── Serialize for summary ──────────────────────────────────────────

    #[test]
    fn serialize_labels_user_and_assistant() {
        let msgs = vec![Message::user("hello"), Message::assistant("world")];
        let text = serialize_for_summary(&msgs);
        assert!(text.contains("[USER]: hello"), "text={text}");
        assert!(text.contains("[ASSISTANT]: world"), "text={text}");
    }

    #[test]
    fn serialize_skips_system_messages() {
        let msgs = vec![Message::system("You are an AI"), Message::user("hi")];
        let text = serialize_for_summary(&msgs);
        assert!(!text.contains("You are an AI"));
        assert!(text.contains("[USER]: hi"));
    }

    #[test]
    fn serialize_truncates_long_content() {
        let long_content = "z".repeat(5_000);
        let msgs = vec![Message::user(&long_content)];
        let text = serialize_for_summary(&msgs);
        assert!(
            text.contains("[truncated]"),
            "should truncate long messages"
        );
    }

    #[test]
    fn serialize_truncates_long_unicode_content_without_panicking() {
        let prefix = "z".repeat(1_999);
        let long_content = format!("{prefix}étail{}", "y".repeat(5_000));
        let msgs = vec![Message::user(&long_content)];
        let text = serialize_for_summary(&msgs);
        assert!(text.contains("[truncated]"));
        assert!(!text.contains('�'));
    }

    #[test]
    fn summary_includes_first_user_message() {
        let msgs = vec![
            Message::user("What is the meaning of life?"),
            Message::assistant("42"),
        ];
        let summary = build_summary(&msgs);
        assert!(summary.contains("What is the meaning of life?"));
    }

    #[test]
    fn summary_truncates_long_first_message() {
        let long_msg = "x".repeat(500);
        let msgs = vec![Message::user(&long_msg)];
        let summary = build_summary(&msgs);
        assert!(summary.contains("..."));
        assert!(summary.len() < 600);
    }

    // ── New v0.4.0 tests ──────────────────────────────────────────────

    #[test]
    fn summary_prefix_constant_starts_correctly() {
        assert!(SUMMARY_PREFIX.starts_with("[CONTEXT COMPACTION]"));
    }

    #[test]
    fn pruned_tool_placeholder_is_short() {
        // Must fit in a single token budget line
        assert!(PRUNED_TOOL_PLACEHOLDER.len() < 100);
    }

    #[test]
    fn prune_tool_outputs_replaces_long_results() {
        let messages = vec![
            Message::user("run a command"),
            Message::tool_result("id1", "shell_exec", &"x".repeat(500)),
        ];
        let pruned = prune_tool_outputs(&messages);
        assert_eq!(pruned.len(), 2);
        // User message unchanged
        assert_eq!(pruned[0].text_content(), "run a command");
        // Tool result replaced with placeholder
        assert_eq!(pruned[1].text_content(), PRUNED_TOOL_PLACEHOLDER);
    }

    #[test]
    fn prune_tool_outputs_keeps_short_results() {
        let messages = vec![Message::tool_result("id1", "shell_exec", "ok")];
        let pruned = prune_tool_outputs(&messages);
        assert_eq!(pruned[0].text_content(), "ok");
    }

    #[test]
    fn extract_prior_summary_finds_prefixed_block() {
        let summary_text = "Prior summary content";
        let messages = vec![
            Message::system_summary(format!("{SUMMARY_PREFIX}{summary_text}")),
            Message::user("hello"),
        ];
        let extracted = extract_prior_summary(&messages);
        assert_eq!(extracted.as_deref(), Some(summary_text));
    }

    #[test]
    fn extract_prior_summary_returns_none_without_prefix() {
        let messages = vec![
            Message::system_summary("Regular context summary".to_string()),
            Message::user("hello"),
        ];
        let extracted = extract_prior_summary(&messages);
        assert!(extracted.is_none());
    }
}
