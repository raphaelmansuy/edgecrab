//! Wave-2 Hermes gap close e2e (spec 022-014): prologue, VERIFY ordering, credential pool.

use edgecrab_core::{
    CredentialPool, compression_made_progress, plan_tool_batch, should_run_preflight_estimate,
};
use edgecrab_tools::ToolRegistry;
use edgecrab_types::{FunctionCall, ToolCall};

#[test]
fn e2e_preflight_few_but_huge_gate() {
    // Few messages, huge rough tokens → preflight should run
    assert!(should_run_preflight_estimate(3, 0, 20, 90_000, 50_000));
    // Few messages, small tokens → skip
    assert!(!should_run_preflight_estimate(3, 0, 20, 100, 50_000));
    // Many messages → count gate
    assert!(should_run_preflight_estimate(40, 0, 20, 100, 50_000));
}

#[test]
fn e2e_compression_progress_material_token_cut() {
    assert!(compression_made_progress(20, 20, 200_000, 150_000));
    assert!(!compression_made_progress(20, 20, 200_000, 195_000));
}

#[test]
fn e2e_credential_pool_rotate_sequence() {
    let mut pool = CredentialPool::new();
    pool.install_keys(
        "openai",
        ["sk-aaa".into(), "sk-bbb".into(), "sk-ccc".into()],
    );
    assert_eq!(pool.active_token("openai"), Some("sk-aaa"));
    assert_eq!(
        pool.mark_exhausted_and_rotate("openai").as_deref(),
        Some("sk-bbb")
    );
    assert_eq!(
        pool.mark_exhausted_and_rotate("openai").as_deref(),
        Some("sk-ccc")
    );
    assert!(pool.mark_exhausted_and_rotate("openai").is_none());
}

#[test]
fn e2e_multi_tool_plan_with_registry() {
    let reg = ToolRegistry::new();
    let calls = vec![
        ToolCall {
            id: "1".into(),
            r#type: "function".into(),
            function: FunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path":"a.rs"}"#.into(),
            },
            thought_signature: None,
        },
        ToolCall {
            id: "2".into(),
            r#type: "function".into(),
            function: FunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path":"b.rs"}"#.into(),
            },
            thought_signature: None,
        },
    ];
    let plan = plan_tool_batch(Some(&reg), &calls);
    assert_eq!(plan.total(), 2);
}
