//! E2E-style tests for multi-tool batch planning (spec 022/014 WS-B).
//!
//! These lock parallel/sequential partition policy without needing a live LLM.

use edgecrab_core::{parallel_max_workers, plan_tool_batch};
use edgecrab_tools::ToolRegistry;
use edgecrab_types::{FunctionCall, ToolCall};

fn tc(id: &str, name: &str, args: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        r#type: "function".into(),
        function: FunctionCall {
            name: name.into(),
            arguments: args.into(),
        },
        thought_signature: None,
    }
}

#[test]
fn e2e_t1_independent_reads_plan_parallel_when_safe() {
    let reg = ToolRegistry::new();
    let calls = vec![
        tc("1", "read_file", r#"{"path":"a.rs"}"#),
        tc("2", "read_file", r#"{"path":"b.rs"}"#),
    ];
    let plan = plan_tool_batch(Some(&reg), &calls);
    assert_eq!(plan.total(), 2);
    if reg.is_parallel_safe("read_file") {
        assert_eq!(
            plan.parallel.len(),
            2,
            "two independent read_file calls should be parallel"
        );
        assert!(plan.sequential.is_empty());
    }
}

#[test]
fn e2e_t2_overlapping_paths_serialize() {
    let reg = ToolRegistry::new();
    // path_arguments claim only applies when tool is not parallel_safe
    let tool = "write_file";
    if reg.path_arguments(tool).is_empty() {
        return;
    }
    let calls = vec![
        tc("1", tool, r#"{"path":"src/lib.rs","content":"a"}"#),
        tc("2", tool, r#"{"path":"src/lib.rs","content":"b"}"#),
    ];
    let plan = plan_tool_batch(Some(&reg), &calls);
    assert_eq!(plan.total(), 2);
    if !reg.is_parallel_safe(tool) {
        assert!(
            plan.sequential.len() >= 1,
            "overlapping write paths must serialize: {plan:?}"
        );
    }
}

#[test]
fn e2e_t3_one_batch_plan_preserves_all_ids() {
    let reg = ToolRegistry::new();
    let calls = vec![
        tc("a", "read_file", r#"{"path":"x"}"#),
        tc("b", "terminal", r#"{"command":"echo hi"}"#),
        tc("c", "search_files", r#"{"pattern":"foo"}"#),
    ];
    let plan = plan_tool_batch(Some(&reg), &calls);
    assert_eq!(plan.total(), 3);
    let mut ids: Vec<_> = plan
        .parallel
        .iter()
        .chain(plan.sequential.iter())
        .map(|p| p.id.as_str())
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, vec!["a", "b", "c"]);
}

#[test]
fn e2e_t4_mutating_tools_not_blindly_parallel() {
    let reg = ToolRegistry::new();
    // write_file / patch should typically be sequential with each other on same path
    let calls = vec![
        tc("1", "write_file", r#"{"path":"out.txt","content":"a"}"#),
        tc("2", "write_file", r#"{"path":"out.txt","content":"b"}"#),
    ];
    let plan = plan_tool_batch(Some(&reg), &calls);
    assert_eq!(plan.total(), 2);
    // If write_file is path-aware, second should not both be free-parallel without claim.
    if !reg.is_parallel_safe("write_file") {
        // Expected: sequential path (or first parallel if path_arguments allow non-overlap only)
        assert!(
            plan.sequential.len() >= 1 || plan.parallel.len() <= 1,
            "mutating same file should serialize: {plan:?}"
        );
    }
}

#[test]
fn e2e_parallel_max_workers_env_contract() {
    assert_eq!(parallel_max_workers(Some(8), 100), 8);
    assert_eq!(parallel_max_workers(None, 2), 2);
}
