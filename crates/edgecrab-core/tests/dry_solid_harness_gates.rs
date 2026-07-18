//! 018 dry-solid acceptance gates — fail CI if harness ownership laws regress.

use std::fs;
use std::path::PathBuf;

fn workspace_file(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
}

#[test]
fn conversation_has_no_default_harness_snapshot_at_assess() {
    let src = fs::read_to_string(workspace_file("crates/edgecrab-core/src/conversation.rs"))
        .expect("conversation.rs");
    // Assess sites must use build_turn_harness_snapshot / assess_turn_outcome.
    assert!(
        !src.contains("HarnessSnapshot::default()"),
        "conversation.rs must not use HarnessSnapshot::default() (HA-45 / 018)"
    );
}

#[test]
fn conversation_uses_pre_dispatch_decision_compositor() {
    let src = fs::read_to_string(workspace_file("crates/edgecrab-core/src/conversation.rs"))
        .expect("conversation.rs");
    assert!(
        src.contains("pre_dispatch_decision"),
        "conversation.rs must route pre-dispatch through turn_dispatch_policy"
    );
    assert!(
        !src.contains("guardrail_before_dispatch_checked_with_session("),
        "conversation.rs must not call guardrail_before_dispatch_checked_with_session directly"
    );
}

#[test]
fn turn_dispatch_policy_owns_body_not_facade() {
    let policy = fs::read_to_string(workspace_file(
        "crates/edgecrab-core/src/turn_dispatch_policy.rs",
    ))
    .expect("turn_dispatch_policy.rs");
    let dispatch = fs::read_to_string(workspace_file("crates/edgecrab-core/src/turn_dispatch.rs"))
        .expect("turn_dispatch.rs");
    let production = policy.split("#[cfg(test)]").next().unwrap_or(&policy);
    assert!(
        production.contains("visual_storm_block_result_with_args")
            && production.contains("spill_blind_write_block"),
        "turn_dispatch_policy must own storm + spill checks (018 P1)"
    );
    // Avoid matching the identifier inside string literals in tests/comments.
    let facade_call = concat!("guardrail_before_dispatch_checked", "_with_session(");
    assert!(
        !production.contains(facade_call),
        "policy must not call back into the turn_dispatch facade"
    );
    // Thin re-export only: deprecated wrappers must call pre_dispatch_decision.
    assert!(
        dispatch.contains("turn_dispatch_policy::pre_dispatch_decision"),
        "turn_dispatch re-exports must delegate to policy"
    );
}

#[test]
fn turn_epilogue_bans_default_snapshot_in_docs_comment() {
    let src = fs::read_to_string(workspace_file("crates/edgecrab-core/src/turn_epilogue.rs"))
        .expect("turn_epilogue.rs");
    assert!(
        src.contains("never use [`HarnessSnapshot::default`]")
            || src.contains("HarnessSnapshot::default"),
        "turn_epilogue must document the HA-45 ban"
    );
    assert!(
        src.contains("LifecycleEvent::PreVerify"),
        "turn_epilogue must emit PreVerify before assess"
    );
}

#[test]
fn indexed_hot_tools_stay_at_five() {
    let src = fs::read_to_string(workspace_file("crates/edgecrab-tools/src/toolsets.rs"))
        .expect("toolsets.rs");
    let Some(start) = src.find("pub const INDEXED_HOT_TOOLS") else {
        panic!("INDEXED_HOT_TOOLS missing");
    };
    let slice = &src[start..];
    let Some(eq) = slice.find("= &[") else {
        panic!("INDEXED_HOT_TOOLS array missing");
    };
    let rest = &slice[eq + 3..]; // starts at '['
    let Some(end) = rest.find(']') else {
        panic!("INDEXED_HOT_TOOLS array unclosed");
    };
    let body = &rest[1..end];
    let count = body.matches('"').count() / 2;
    assert_eq!(
        count, 5,
        "INDEXED_HOT_TOOLS must stay at 5 without new meter proof (found {count})"
    );
    assert!(
        body.contains("write_file"),
        "write_file must remain hot (create-path disclosure)"
    );
}

#[test]
fn materialize_path_does_not_mention_cached_system_prompt_assignment() {
    let src = fs::read_to_string(workspace_file("crates/edgecrab-tools/src/tool_schema_index.rs"))
        .expect("tool_schema_index.rs");
    assert!(
        !src.contains("cached_system_prompt"),
        "tool_schema_index must not touch cached_system_prompt (cache law)"
    );
}
