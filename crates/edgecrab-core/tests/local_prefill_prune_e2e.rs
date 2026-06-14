//! End-to-end (deterministic) tests for local structural prefill prune policy.
//!
//! Exercises the homelab failure signature: ~46k prompt tokens after web research,
//! below the 50% LLM-compression threshold on synced LM Studio context, but above
//! the deterministic prefill prune budget — without requiring a live LM Studio server.

use edgecrab_core::compression::{
    apply_structural_tool_output_prune, count_long_tool_outputs, estimate_tokens,
    structural_prefill_prune,
};
use edgecrab_core::local_provider_policy::{
    gate_local_structural_prune, local_prefill_prune_token_budget,
    should_structural_prefill_prune, try_apply_structural_tool_output_prune,
    LocalStructuralPrunePhase, LOCAL_PREFILL_CONTEXT_DIVISOR, LOCAL_PREFILL_PRUNE_TOKEN_BUDGET,
};
use edgecrab_tools::mutation_turn_policy::length_without_tools_recovery_message;
use edgecrab_types::Message;

const LMSTUDIO_SYNCED_CTX: usize = 262_144;
const COMPRESSION_THRESHOLD_AT_50PCT: usize = LMSTUDIO_SYNCED_CTX / 2;

fn research_heavy_messages(tool_count: usize, body_chars: usize) -> Vec<Message> {
    let mut messages = vec![Message::user("Build a PPT about market trends")];
    for i in 0..tool_count {
        messages.push(Message::tool_result(
            &format!("web_{i}"),
            if i % 2 == 0 { "web_search" } else { "web_extract" },
            &format!("result {i}\n{}", "x".repeat(body_chars)),
        ));
    }
    messages.push(Message::user("Now write the scaffold file"));
    messages
}

/// ~48k+ message tokens — above prefill budget (32k @ 262k ctx), below 50% compress (131k).
const RESEARCH_TOOL_COUNT: usize = 8;
const RESEARCH_BODY_CHARS: usize = 25_000;

/// Homelab length-failure band: ~34–40k message tokens — above 32k preflight budget (B1).
const MID_BAND_TOOL_COUNT: usize = 8;
const MID_BAND_BODY_CHARS: usize = 18_000;

fn length_failure_loop_messages(recovery_rounds: usize) -> Vec<Message> {
    let mut messages = research_heavy_messages(MID_BAND_TOOL_COUNT, MID_BAND_BODY_CHARS);
    let recovery = length_without_tools_recovery_message(32 * 1024, None);
    for _ in 0..recovery_rounds {
        messages.push(Message::user(&recovery));
    }
    messages
}

#[test]
fn homelab_research_prompt_triggers_prefill_prune_not_llm_compression() {
    let messages = research_heavy_messages(RESEARCH_TOOL_COUNT, RESEARCH_BODY_CHARS);
    let prompt_tokens = estimate_tokens(&messages);
    assert!(
        prompt_tokens > local_prefill_prune_token_budget(LMSTUDIO_SYNCED_CTX),
        "fixture must exceed prefill budget (got {prompt_tokens})"
    );
    assert!(
        prompt_tokens < COMPRESSION_THRESHOLD_AT_50PCT,
        "fixture must stay below 50% compress threshold (got {prompt_tokens})"
    );
    assert!(should_structural_prefill_prune(
        prompt_tokens,
        LMSTUDIO_SYNCED_CTX
    ));
}

#[test]
fn structural_prefill_prune_breaks_length_failure_loop_signature() {
    let messages = research_heavy_messages(RESEARCH_TOOL_COUNT, RESEARCH_BODY_CHARS);
    let before = estimate_tokens(&messages);
    assert!(should_structural_prefill_prune(before, LMSTUDIO_SYNCED_CTX));

    let (pruned, replaced) = structural_prefill_prune(&messages, None);
    assert!(replaced >= RESEARCH_TOOL_COUNT, "expected all research tool outputs pruned");
    let after = estimate_tokens(&pruned);
    assert!(
        after < before / 3,
        "prune must reclaim most tool mass: before={before} after={after}"
    );
    assert!(
        !should_structural_prefill_prune(after, LMSTUDIO_SYNCED_CTX),
        "after prune, preflight should not re-fire immediately"
    );
}

#[test]
fn prefill_budget_formula_matches_first_principles_cap() {
    assert_eq!(
        local_prefill_prune_token_budget(LMSTUDIO_SYNCED_CTX),
        LOCAL_PREFILL_PRUNE_TOKEN_BUDGET
            .min(LMSTUDIO_SYNCED_CTX / LOCAL_PREFILL_CONTEXT_DIVISOR)
    );
    assert_eq!(
        local_prefill_prune_token_budget(8192),
        8192 / LOCAL_PREFILL_CONTEXT_DIVISOR
    );
}

/// **LH-30** — homelab mid-band (34–40k) triggers preflight at lowered threshold (32k).
#[test]
fn lh30_mid_band_triggers_preflight_prune() {
    let messages = research_heavy_messages(MID_BAND_TOOL_COUNT, MID_BAND_BODY_CHARS);
    let before = estimate_tokens(&messages);
    let budget = local_prefill_prune_token_budget(LMSTUDIO_SYNCED_CTX);

    assert_eq!(budget, 32_000);
    assert!(
        before >= 34_000 && before <= 40_000,
        "fixture must match homelab mid band (got {before})"
    );
    assert!(
        before > budget,
        "mid band must exceed preflight budget (before={before}, budget={budget})"
    );
    assert!(gate_local_structural_prune(
        LocalStructuralPrunePhase::Preflight,
        before,
        LMSTUDIO_SYNCED_CTX,
    ));
}

/// **LH-31** — preflight prune on mid-band fixture drops estimate below budget.
#[test]
fn lh31_mid_band_preflight_prune_drops_below_budget() {
    let messages = research_heavy_messages(MID_BAND_TOOL_COUNT, MID_BAND_BODY_CHARS);
    let before = estimate_tokens(&messages);
    let budget = local_prefill_prune_token_budget(LMSTUDIO_SYNCED_CTX);
    assert!(before > budget);

    let (pruned, outcome) = try_apply_structural_tool_output_prune(
        LocalStructuralPrunePhase::Preflight,
        before,
        LMSTUDIO_SYNCED_CTX,
        &messages,
        None,
    )
    .expect("preflight must prune mid-band fixture");

    assert_eq!(count_long_tool_outputs(&pruned), 0);
    assert_eq!(outcome.long_tool_outputs_remaining, 0);
    let after = estimate_tokens(&pruned);
    assert!(
        after <= budget,
        "post-prune must sit at or below preflight budget: after={after} budget={budget}"
    );
}

/// **LH-11** — length-recovery structural prune still reclaims fat tool outputs.
#[test]
fn lh11_length_recovery_prune_drops_tokens_in_mid_band() {
    let messages = length_failure_loop_messages(3);
    let before = estimate_tokens(&messages);

    let (pruned, outcome) = try_apply_structural_tool_output_prune(
        LocalStructuralPrunePhase::LengthRecovery,
        before,
        LMSTUDIO_SYNCED_CTX,
        &messages,
        None,
    )
    .expect("length recovery must prune fat tool outputs");

    assert_eq!(count_long_tool_outputs(&pruned), 0);
    assert_eq!(outcome.long_tool_outputs_remaining, 0);
    assert_eq!(outcome.tools_pruned, MID_BAND_TOOL_COUNT);
    assert!(
        outcome.message_tokens_after < outcome.message_tokens_before,
        "prune must shrink message tokens: before={} after={}",
        outcome.message_tokens_before,
        outcome.message_tokens_after
    );
    assert!(
        estimate_tokens(&pruned) < before / 3,
        "mid-band prune must reclaim most tool mass: before={before} after={after}",
        after = estimate_tokens(&pruned)
    );

    let (_, direct_outcome) = apply_structural_tool_output_prune(&messages, None)
        .expect("direct apply matches try_apply outcome");
    assert_eq!(direct_outcome.tools_pruned, outcome.tools_pruned);
}

/// High-band fixture: ~57k+ tokens, above 20% structural compress, below 50% LLM compress.
const HIGH_BAND_TOOL_ROUNDS: usize = 12;
const HIGH_BAND_BODY_CHARS: usize = 20_000;

fn high_band_messages() -> Vec<Message> {
    let mut messages = vec![Message::user("Build PPT — extended research phase")];
    for i in 0..HIGH_BAND_TOOL_ROUNDS {
        messages.push(Message::tool_result(
            &format!("web_{i}"),
            if i % 2 == 0 { "web_search" } else { "web_extract" },
            &format!("chunk {i}\n{}", "y".repeat(HIGH_BAND_BODY_CHARS)),
        ));
        messages.push(Message::user(&format!("Summarize chunk {i} and continue")));
    }
    messages.push(Message::user("Now write the scaffold file"));
    messages
}

fn lmstudio_compression_params() -> edgecrab_core::compression::CompressionParams {
    edgecrab_core::compression::CompressionParams {
        context_window: LMSTUDIO_SYNCED_CTX,
        threshold: 0.5,
        target_ratio: 0.20,
        protect_last_n: 20,
    }
}

/// **LH-32** — ~57k band triggers local structural compress, not LLM compress @ 50%.
#[test]
fn lh32_high_band_triggers_local_structural_compress_not_llm() {
    use edgecrab_core::compression::{
        check_compression_status_for_estimate, CompressionStatus,
    };
    use edgecrab_core::local_provider_policy::{
        local_structural_compress_token_threshold, should_local_structural_compress,
    };

    let messages = high_band_messages();
    let before = estimate_tokens(&messages);
    let mid = local_structural_compress_token_threshold(LMSTUDIO_SYNCED_CTX);
    let params = lmstudio_compression_params();

    assert!(
        before >= 55_000 && before <= 65_000,
        "fixture must sit in high band (got {before})"
    );
    assert!(before > mid, "before={before} mid={mid}");
    assert!(before < COMPRESSION_THRESHOLD_AT_50PCT);
    assert!(should_local_structural_compress(
        before,
        LMSTUDIO_SYNCED_CTX,
        COMPRESSION_THRESHOLD_AT_50PCT,
    ));
    assert_ne!(
        check_compression_status_for_estimate(before, &params),
        CompressionStatus::NeedsCompression
    );
}

/// **LH-33** — mid-band structural compress shrinks high-band fixture without LLM.
#[test]
fn lh33_local_structural_compress_reduces_high_band_tokens() {
    use edgecrab_core::local_provider_policy::try_local_midband_structural_compress;

    let messages = high_band_messages();
    let before = estimate_tokens(&messages);
    let params = lmstudio_compression_params();

    let (compressed, tokens_before, tokens_after) = try_local_midband_structural_compress(
        &messages,
        &params,
        LMSTUDIO_SYNCED_CTX,
        before,
        None,
    )
    .expect("high band must structural compress");

    assert_eq!(tokens_before, before);
    assert!(tokens_after < tokens_before / 2);
    assert!(compressed.len() < messages.len());
}

/// **LH-63** — homelab ~57k signature must exceed 20% mid-band threshold (was gap @ 0.22).
#[test]
fn lh63_homelab_57k_band_triggers_structural_compress_at_20pct() {
    use edgecrab_core::local_provider_policy::{
        local_structural_compress_token_threshold, should_local_structural_compress,
    };

    let messages = high_band_messages();
    let before = estimate_tokens(&messages);
    let mid = local_structural_compress_token_threshold(LMSTUDIO_SYNCED_CTX);

    assert_eq!(mid, 52_428, "262144 × 0.20");
    assert!(
        before > mid,
        "fixture must exceed mid-band threshold (before={before} mid={mid})"
    );
    assert!(
        57_000 > mid,
        "homelab-reported ~57k must exceed 20% threshold (mid={mid})"
    );
    assert!(should_local_structural_compress(
        before,
        LMSTUDIO_SYNCED_CTX,
        COMPRESSION_THRESHOLD_AT_50PCT,
    ));
    assert!(should_local_structural_compress(
        57_000,
        LMSTUDIO_SYNCED_CTX,
        COMPRESSION_THRESHOLD_AT_50PCT,
    ));
}
