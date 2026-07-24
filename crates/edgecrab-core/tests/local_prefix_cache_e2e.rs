//! E2E: local tool wire schemas stay frozen across budget changes (prefix stability).
//!
//! Spec: `specs/023-omlx/013-local-prefix-cache-july-2026.md`
//!
//! Why this matters: MTPLX reports `prefix_divergence_at_token` when tool
//! description suffixes change every API iteration as context/max_tokens move.

use edgecrab_core::local_prefix_cache::{
    LocalToolFreeze, clear_local_tool_freeze, resolve_frozen_local_api_tools,
    tool_defs_content_fingerprint, tool_set_fingerprint,
};
use edgequake_llm::ToolDefinition;
use serde_json::json;

fn tool(name: &str, desc: &str) -> ToolDefinition {
    ToolDefinition::function(
        name,
        desc,
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "content": { "type": "string" }
            }
        }),
    )
}

/// Multi-round simulation: three local API "iterations" with growing context budgets.
#[test]
fn e2e_mtplx_tool_wire_stable_across_three_iterations() {
    let active = vec![
        tool("read_file", "Read a file from disk."),
        tool("write_file", "Write a file to disk."),
        tool("execute_code", "Run code in a sandbox."),
        tool("terminal", "Run a shell command."),
    ];
    let mut freeze: Option<LocalToolFreeze> = None;

    let budgets = [(12_000, 2048), (18_000, 4096), (27_852, 8192)];
    let mut fingerprints = Vec::new();

    for (i, budget) in budgets.iter().enumerate() {
        let wire = resolve_frozen_local_api_tools(&mut freeze, "mtplx", &active, Some(*budget));
        assert_eq!(wire.len(), active.len(), "iter {i}");
        fingerprints.push(tool_defs_content_fingerprint(&wire));
        // Tip-only growth would happen in messages; tools must not change.
        if i > 0 {
            assert_eq!(
                fingerprints[i], fingerprints[0],
                "iter {i}: tool wire diverged (prefix killer)"
            );
        }
    }

    // write_file must still carry local limit text from first freeze
    let write = freeze
        .as_ref()
        .unwrap()
        .tools
        .iter()
        .find(|d| d.function.name == "write_file")
        .unwrap();
    assert!(
        write.function.description.contains("Local turn limit"),
        "missing local budget suffix"
    );
    assert!(
        write.function.description.contains("12000"),
        "should retain first-iteration budget, got: {}",
        write.function.description
    );
}

#[test]
fn e2e_omlx_and_mtplx_share_freeze_policy() {
    let active = vec![tool("write_file", "Write.")];
    for provider in ["omlx", "mtplx", "ollama", "lmstudio"] {
        let mut freeze = None;
        let a = resolve_frozen_local_api_tools(&mut freeze, provider, &active, Some((5000, 1024)));
        let b = resolve_frozen_local_api_tools(&mut freeze, provider, &active, Some((9000, 4096)));
        assert_eq!(
            tool_defs_content_fingerprint(&a),
            tool_defs_content_fingerprint(&b),
            "provider={provider}"
        );
    }
}

#[test]
fn e2e_tool_set_growth_refreezes_cleanly() {
    let mut freeze = None;
    let small = vec![tool("read_file", "Read.")];
    let _ = resolve_frozen_local_api_tools(&mut freeze, "mtplx", &small, Some((1000, 512)));
    let fp1 = freeze.as_ref().unwrap().fingerprint.clone();

    let grown = vec![tool("read_file", "Read."), tool("write_file", "Write.")];
    let wire = resolve_frozen_local_api_tools(&mut freeze, "mtplx", &grown, Some((1000, 512)));
    assert_eq!(wire.len(), 2);
    assert_ne!(freeze.as_ref().unwrap().fingerprint, fp1);
    assert_eq!(
        freeze.as_ref().unwrap().fingerprint,
        tool_set_fingerprint(&grown)
    );
}

#[test]
fn e2e_clear_freeze_on_explicit_reset() {
    let mut freeze = Some(LocalToolFreeze {
        fingerprint: "x".into(),
        tools: vec![tool("a", "b")],
    });
    clear_local_tool_freeze(&mut freeze);
    assert!(freeze.is_none());
}

#[test]
fn e2e_cloud_provider_never_freezes() {
    let active = vec![tool("write_file", "Write.")];
    let mut freeze = None;
    let _ = resolve_frozen_local_api_tools(&mut freeze, "anthropic", &active, Some((12_000, 2048)));
    assert!(freeze.is_none());
}
