//! # E2E tests with VsCodeCopilot provider
//!
//! Validates that EdgeCrab works end-to-end using the VS Code Copilot
//! LLM provider (gpt-4.1-mini). These tests require a running VS Code
//! instance with the Copilot extension and the `VSCODE_COPILOT_TOKEN`
//! environment variable (or a valid VS Code IPC socket) to be set.
//!
//! WHY `#[ignore]`: E2E tests against live providers incur cost and
//! latency and cannot run in CI without credentials. They are opted-in
//! with `cargo test -- --ignored` or by setting the env var sentinel.
//!
//! To run:
//! ```bash
//! cargo test -p edgecrab-core --test e2e_copilot -- --include-ignored
//! ```

use std::sync::Arc;

use edgecrab_core::agent::{AgentBuilder, ApprovalChoice, StreamEvent};
use edgequake_llm::ProviderFactory;

/// Guard: skip test when VS Code Copilot is not available.
///
/// WHY env var check: We don't want to #[ignore] unconditionally
/// because that hides failures in environments where Copilot IS
/// available. Instead we check for the IPC socket path that
/// VS Code injects into every process it spawns. In CI without
/// VS Code, `VSCODE_IPC_HOOK_CLI` is absent and the test skips.
fn copilot_available() -> bool {
    // VS Code sets this env var in all child processes when the
    // Copilot extension is active.
    std::env::var("VSCODE_IPC_HOOK_CLI").is_ok() || std::env::var("VSCODE_COPILOT_TOKEN").is_ok()
}

/// Full chat round-trip: user → agent → LLM → response.
///
/// Uses gpt-4.1-mini (fast + cheap) via the vscode-copilot provider.
#[tokio::test]
#[ignore = "requires VS Code Copilot (VSCODE_IPC_HOOK_CLI or VSCODE_COPILOT_TOKEN)"]
async fn e2e_agent_chat_with_copilot_gpt4_mini() {
    if !copilot_available() {
        eprintln!("Skipping: VS Code Copilot not available");
        return;
    }

    let provider = ProviderFactory::create_llm_provider("vscode-copilot", "gpt-4.1-mini")
        .expect("should create VsCodeCopilot provider");

    let agent = AgentBuilder::new("vscode-copilot/gpt-4.1-mini")
        .provider(provider)
        .max_iterations(3)
        .build()
        .expect("agent build should succeed");

    let response = agent
        .chat("Reply with exactly: PONG")
        .await
        .expect("chat should succeed");

    assert!(!response.is_empty(), "Response should not be empty");
    assert!(
        response.to_uppercase().contains("PONG"),
        "Expected PONG in response, got: {response}"
    );
}

/// Streaming round-trip: tokens arrive via channel.
#[tokio::test]
#[ignore = "requires VS Code Copilot (VSCODE_IPC_HOOK_CLI or VSCODE_COPILOT_TOKEN)"]
async fn e2e_agent_streaming_with_copilot() {
    if !copilot_available() {
        eprintln!("Skipping: VS Code Copilot not available");
        return;
    }

    let provider = ProviderFactory::create_llm_provider("vscode-copilot", "gpt-4.1-mini")
        .expect("should create VsCodeCopilot provider");

    let agent = Arc::new(
        AgentBuilder::new("vscode-copilot/gpt-4.1-mini")
            .provider(provider)
            .max_iterations(2)
            .build()
            .expect("agent build should succeed"),
    );

    let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();

    let agent_clone = Arc::clone(&agent);
    let handle = tokio::spawn(async move {
        agent_clone
            .chat_streaming("Count from 1 to 3 with spaces.", chunk_tx)
            .await
            .expect("streaming should succeed");
    });

    let mut accumulated = String::new();
    let mut got_done = false;

    while let Some(event) = chunk_rx.recv().await {
        match event {
            StreamEvent::Token(text) => accumulated.push_str(&text),
            StreamEvent::ToolExec { .. } => {} // tool progress events — just ignore in this test
            StreamEvent::ToolDone { .. } => {} // tool completion events — just ignore in this test
            StreamEvent::SubAgentStart { .. } => {} // delegated progress — not relevant here
            StreamEvent::SubAgentReasoning { .. } => {} // delegated progress — not relevant here
            StreamEvent::SubAgentToolExec { .. } => {} // delegated progress — not relevant here
            StreamEvent::SubAgentFinish { .. } => {} // delegated progress — not relevant here
            StreamEvent::Done => {
                got_done = true;
                break;
            }
            StreamEvent::Clarify { .. } => {} // not expected in this test
            StreamEvent::Error(e) => panic!("Unexpected streaming error: {e}"),
            StreamEvent::Reasoning(_) => {} // extended thinking — ignore in this test
            StreamEvent::HookEvent { .. } => {} // hook events — not relevant in this test
            StreamEvent::ContextPressure { .. } => {} // pressure warnings — not relevant in this test
            // These interactive events are unexpected in an automated streaming test;
            // treat them as non-fatal by ignoring, preserving test determinism.
            StreamEvent::Approval { response_tx, .. } => {
                // Deny any unexpected approval request so the agent doesn't hang.
                let _ = response_tx.send(ApprovalChoice::Deny);
            }
            StreamEvent::SecretRequest { response_tx, .. } => {
                // Abort any unexpected secret request so the agent doesn't hang.
                let _ = response_tx.send(String::new());
            }
        }
    }

    handle.await.expect("spawn should complete");

    assert!(got_done, "Should receive Done event");
    assert!(!accumulated.is_empty(), "Should receive tokens");
    // Loose check: response mentions numbers 1–3
    let has_numbers = accumulated.contains('1') && accumulated.contains('3');
    assert!(has_numbers, "Expected 1..3 in response, got: {accumulated}");
}

/// Model hot-swap during session.
#[tokio::test]
#[ignore = "requires VS Code Copilot (VSCODE_IPC_HOOK_CLI or VSCODE_COPILOT_TOKEN)"]
async fn e2e_model_swap_with_copilot() {
    if !copilot_available() {
        eprintln!("Skipping: VS Code Copilot not available");
        return;
    }

    let provider = ProviderFactory::create_llm_provider("vscode-copilot", "gpt-4.1-mini")
        .expect("copilot provider");

    let agent = AgentBuilder::new("vscode-copilot/gpt-4.1-mini")
        .provider(Arc::clone(&provider))
        .max_iterations(2)
        .build()
        .expect("build");

    // First turn
    let r1 = agent.chat("Say ALPHA").await.expect("first chat");
    assert!(r1.to_uppercase().contains("ALPHA"), "got: {r1}");

    // Hot-swap to the same model (no-op but validates the plumbing)
    let provider2 = ProviderFactory::create_llm_provider("vscode-copilot", "gpt-4.1-mini")
        .expect("second provider");
    agent
        .swap_model("vscode-copilot/gpt-4.1-mini".into(), provider2)
        .await;

    // Second turn after swap
    let r2 = agent.chat("Say BETA").await.expect("second chat");
    assert!(r2.to_uppercase().contains("BETA"), "got: {r2}");
}
