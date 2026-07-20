//! Deterministic e2e for context-size / cache-efficiency contracts (Sprints A–G).
//!
//! First principles under test:
//! 1. Gateway hygiene gates (85% tokens OR hard message limit).
//! 2. First-compression note mutates **combined** only; stable/semi prefixes peel cleanly.
//! 3. Defer-preflight arms after compaction; anti-thrashing blocks after repeated fallbacks.
//! 4. Materialized-tool LRU respects `max_materialized_tools`.
//! 5. Subdirectory AGENTS.md hints append to tool results (never system prompt).
//! 6. Envelope breakpoint planner skips empty assistant/tool carriers.
//! 7. Kimi/Moonshot OpenRouter enables envelope cache.
//! 8. SUMMARY_PREFIX anti-hijack lives in summary messages only.

use edgecrab_core::compression::{
    CompressionRuntimeState, FIRST_COMPRESSION_NOTE, GATEWAY_HYGIENE_THRESHOLD, SUMMARY_PREFIX,
    apply_first_compression_system_note, automatic_compression_blocked, effective_threshold,
    record_completed_compaction, should_defer_preflight_to_real_usage, should_run_session_hygiene,
};
use edgecrab_core::conversation::split_dynamic_after_cache_prefixes;
use edgecrab_core::prompt_cache_policy::{
    PromptCacheDecision, apply_prompt_cache_breakpoints, decide_prompt_cache,
};
use edgecrab_core::subdirectory_hints::SubdirectoryHintTracker;
use edgecrab_tools::{DEFAULT_MAX_MATERIALIZED_TOOLS, MaterializedToolSet};
use edgequake_llm::{CachePromptConfig, ChatMessage};

#[test]
fn hygiene_gate_token_and_hard_message_contracts() {
    let ctx = 128_000;
    let at_85 = ((ctx as f32) * GATEWAY_HYGIENE_THRESHOLD) as usize;
    assert_eq!(GATEWAY_HYGIENE_THRESHOLD, 0.85);

    assert!(!should_run_session_hygiene(3, at_85, ctx, true, 5000));
    assert!(!should_run_session_hygiene(10, at_85 - 1, ctx, true, 5000));
    assert!(should_run_session_hygiene(10, at_85, ctx, true, 5000));
    assert!(should_run_session_hygiene(5000, 100, ctx, true, 5000));
    assert!(!should_run_session_hygiene(10, at_85, ctx, false, 5000));
}

#[test]
fn small_context_raises_in_loop_threshold_floor() {
    assert!((effective_threshold(200_000, 0.50) - 0.75).abs() < f32::EPSILON);
    assert!((effective_threshold(600_000, 0.50) - 0.50).abs() < f32::EPSILON);
}

#[test]
fn first_compression_note_survives_three_tier_peel() {
    let stable = "You are EdgeCrab.";
    let semi = "## Skills\n- foo";
    let dynamic = "Today is Friday.\nMEMORY.md content";
    let mut combined = Some(format!("{stable}\n\n{semi}\n\n{dynamic}"));
    let mut done = false;

    apply_first_compression_system_note(&mut done, &mut combined);
    assert!(done);
    apply_first_compression_system_note(&mut done, &mut combined);

    let combined = combined.expect("combined");
    assert!(combined.starts_with(stable));
    let peeled = split_dynamic_after_cache_prefixes(&combined, stable, semi);
    assert!(
        peeled.contains("Earlier conversation turns have been compacted"),
        "note must be in dynamic zone: {peeled}"
    );
    assert!(FIRST_COMPRESSION_NOTE.contains("compacted"));
}

#[test]
fn compaction_runtime_defer_and_anti_thrash_contracts() {
    let mut state = CompressionRuntimeState::default();
    record_completed_compaction(&mut state, 8_000, false);
    assert_eq!(state.compression_count, 1);
    assert!(state.awaiting_real_usage_after_compression);
    assert!(should_defer_preflight_to_real_usage(&state, 50_000, 40_000));

    record_completed_compaction(&mut state, 9_000, true);
    record_completed_compaction(&mut state, 9_500, true);
    assert!(automatic_compression_blocked(&state));
}

#[test]
fn materialized_tool_lru_evicts_coldest_at_default_cap() {
    assert_eq!(DEFAULT_MAX_MATERIALIZED_TOOLS, 12);
    let mut set = MaterializedToolSet::new();
    for i in 0..14 {
        set.insert(format!("deferred_tool_{i}"), DEFAULT_MAX_MATERIALIZED_TOOLS);
    }
    assert_eq!(set.len(), DEFAULT_MAX_MATERIALIZED_TOOLS);
    assert!(!set.contains("deferred_tool_0"));
    assert!(set.contains("deferred_tool_13"));
}

#[test]
fn subdirectory_hint_appends_once_per_directory() {
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let root = dir.path();
    let sub = root.join("svc");
    std::fs::create_dir_all(&sub).expect("mkdir");
    std::fs::write(sub.join("AGENTS.md"), "# Service\nPrefer gRPC.").expect("write");
    std::fs::write(sub.join("lib.rs"), "// lib").expect("write lib");

    let mut tracker = SubdirectoryHintTracker::new(root);
    let args = serde_json::json!({"path": "svc/lib.rs"});
    let hint = tracker
        .check_tool_call("read_file", &args)
        .expect("first visit yields hint");
    assert!(hint.contains("Prefer gRPC"));
    assert!(tracker.check_tool_call("read_file", &args).is_none());
}

#[test]
fn kimi_openrouter_enables_envelope_cache() {
    let d = decide_prompt_cache(
        "openrouter",
        "moonshotai/kimi-k2.6",
        Some("https://openrouter.ai/api/v1"),
    );
    assert!(d.should_cache);
    assert!(!d.native_inner_layout);
}

#[test]
fn envelope_breakpoint_planner_skips_empty_carriers() {
    let mut msgs = vec![
        ChatMessage::system("sys"),
        ChatMessage::user("u1"),
        ChatMessage::assistant_with_tools("", vec![]),
        ChatMessage::tool_result("t1", ""),
        ChatMessage::user("u2"),
        ChatMessage::assistant("visible assistant"),
        ChatMessage::tool_result("t2", "ok"),
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
    assert!(msgs[0].cache_control.is_some());
    assert!(msgs[2].cache_control.is_none());
    assert!(msgs[3].cache_control.is_none());
    let marked: Vec<_> = msgs
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, m)| m.cache_control.is_some())
        .map(|(i, _)| i)
        .collect();
    assert!(marked.contains(&5) || marked.contains(&6) || marked.contains(&4));
    assert!(!marked.contains(&2));
    assert!(!marked.contains(&3));
}

#[test]
fn summary_prefix_is_anti_hijack_reference_only() {
    assert!(SUMMARY_PREFIX.contains("REFERENCE ONLY"));
    assert!(SUMMARY_PREFIX.contains("latest user message WINS"));
    assert!(!SUMMARY_PREFIX.contains("You are EdgeCrab"));
}

#[test]
fn session_db_persists_stable_and_semi_tiers() {
    use edgecrab_state::{SessionDb, SessionRecord};
    let tmp = tempfile::tempdir().expect("tmpdir");
    let db = SessionDb::open(&tmp.path().join("s.db")).expect("db");
    let record = SessionRecord {
        id: "tier-persist".into(),
        source: "cli".into(),
        user_id: None,
        model: Some("mock/m".into()),
        system_prompt: Some("STABLE\n\nSEMI\n\ndyn".into()),
        stable_system_prompt: Some("STABLE".into()),
        semi_stable_system_prompt: Some("SEMI".into()),
        parent_session_id: None,
        started_at: 1.0,
        ended_at: None,
        end_reason: None,
        message_count: 0,
        tool_call_count: 0,
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        last_prompt_tokens: 0,
        estimated_cost_usd: None,
        title: None,
    };
    db.save_session(&record).expect("save");
    let loaded = db.get_session("tier-persist").expect("get").expect("found");
    assert_eq!(loaded.stable_system_prompt.as_deref(), Some("STABLE"));
    assert_eq!(loaded.semi_stable_system_prompt.as_deref(), Some("SEMI"));
}
