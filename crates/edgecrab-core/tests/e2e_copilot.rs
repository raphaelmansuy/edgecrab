//! # E2E tests with VsCodeCopilot provider
//!
//! Validates that EdgeCrab works end-to-end using the VS Code Copilot
//! LLM provider with the live supported GPT-5 mini path. These tests require a running VS Code
//! instance with the Copilot extension and the `VSCODE_COPILOT_TOKEN`
//! environment variable (or a valid VS Code IPC socket) to be set.
//!
//! WHY `#[ignore]`: E2E tests against live providers incur cost and
//! latency and cannot run in CI without credentials. They are opted-in
//! with `cargo test -- --ignored` or by setting the env var sentinel.
//!
//! To run:
//! ```bash
//! cargo test -p edgecrab-core --test e2e_copilot -- --include-ignored --nocapture
//! ```
//!
//! If a fresh GitHub device login is required, the test prints the official
//! authentication link and code to the terminal output.

use std::sync::Arc;

use edgecrab_core::agent::{AgentBuilder, ApprovalChoice, StreamEvent};
use edgequake_llm::providers::vscode::{auth::GitHubAuth, token::TokenManager};

const COPILOT_TEST_MODEL: &str = "gpt-5-mini";
const COPILOT_TEST_SPEC: &str = "vscode-copilot/gpt-5-mini";
const COPILOT_AUTO_MODEL: &str = "auto";
const COPILOT_AUTO_SPEC: &str = "vscode-copilot/auto";

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
    if std::env::var("VSCODE_IPC_HOOK_CLI").is_ok() || std::env::var("VSCODE_COPILOT_TOKEN").is_ok()
    {
        return true;
    }

    if let Some(home) = dirs::home_dir() {
        let candidates = [
            home.join(".config/github-copilot/hosts.json"),
            home.join("Library/Application Support/github-copilot/hosts.json"),
            home.join(".config/edgequake/copilot/github_token.json"),
            home.join("Library/Application Support/edgequake/copilot/github_token.json"),
        ];
        return candidates.iter().any(|path| path.exists());
    }

    false
}

fn is_verified_global_rate_limit(message: &str) -> bool {
    let msg = message.to_ascii_lowercase();
    msg.contains("user_weekly_rate_limited")
        || msg.contains("user_global_rate_limited")
        || msg.contains("global-chat:global-cogs-7-day-key")
}

fn needs_copilot_device_login(message: &str) -> bool {
    let msg = message.to_ascii_lowercase();
    msg.contains("no github copilot oauth session found")
        || msg.contains("run `edgecrab auth login copilot`")
        || msg.contains("bad credentials")
        || msg.contains("rejected by github")
        || msg.contains("401 unauthorized")
}

#[test]
fn detects_when_e2e_should_trigger_device_login() {
    assert!(needs_copilot_device_login(
        "No GitHub Copilot OAuth session found. Run `edgecrab auth login copilot`."
    ));
    assert!(needs_copilot_device_login(
        "All available GitHub Copilot credentials were rejected by GitHub"
    ));
    assert!(!needs_copilot_device_login(
        "user_weekly_rate_limited: retry after 1d 21h"
    ));
}

async fn ensure_copilot_authenticated_for_e2e() {
    let manager = TokenManager::new().expect("token manager should initialize");

    match manager.get_valid_copilot_token().await {
        Ok(_) => return,
        Err(err) => {
            let msg = err.to_string();
            if !needs_copilot_device_login(&msg) {
                eprintln!(
                    "Continuing Copilot E2E without interactive re-login; the remaining issue is not an auth prompt: {msg}"
                );
                return;
            }

            eprintln!(
                "Copilot E2E needs an official GitHub device login before the live request can continue: {msg}"
            );
        }
    }

    let auth = GitHubAuth::new().expect("GitHub auth client should initialize");
    let access_token = auth
        .device_code_flow(|code| {
            let url = code
                .verification_uri_complete
                .as_deref()
                .unwrap_or(&code.verification_uri);
            eprintln!("\nOpen this link to authenticate GitHub Copilot for the E2E test:\n{url}");
            eprintln!("Code: {}\n", code.user_code);
            eprintln!("Waiting for GitHub approval...\n");
        })
        .await
        .expect("device login should complete for the opted-in E2E run");

    manager
        .save_github_token(access_token)
        .await
        .expect("fresh GitHub token should be cached after device login");
    let _ = manager
        .get_valid_copilot_token()
        .await
        .expect("Copilot token refresh should succeed after device login");
}

/// Full chat round-trip: user → agent → LLM → response.
///
/// Uses the real supported GPT-5 mini Copilot model via the vscode-copilot provider.
#[tokio::test]
#[ignore = "requires VS Code Copilot (VSCODE_IPC_HOOK_CLI or VSCODE_COPILOT_TOKEN)"]
async fn e2e_agent_chat_with_copilot_gpt4_mini() {
    if !copilot_available() {
        eprintln!("Skipping: VS Code Copilot not available");
        return;
    }
    ensure_copilot_authenticated_for_e2e().await;

    let provider = edgecrab_tools::create_provider_for_model("vscode-copilot", COPILOT_TEST_MODEL)
        .expect("should create VsCodeCopilot provider");

    let agent = AgentBuilder::new(COPILOT_TEST_SPEC)
        .provider(provider)
        .max_iterations(3)
        .build()
        .expect("agent build should succeed");

    match agent.chat("Reply with exactly: PONG").await {
        Ok(response) => {
            assert!(!response.is_empty(), "Response should not be empty");
            assert!(
                response.to_uppercase().contains("PONG"),
                "Expected PONG in response, got: {response}"
            );
        }
        Err(err) => {
            let msg = err.to_string();
            if is_verified_global_rate_limit(&msg) {
                eprintln!(
                    "Skipping live Copilot round-trip because GitHub auth succeeded but the account is currently under an upstream Copilot weekly/global rate limit: {msg}"
                );
                return;
            }
            panic!("chat should succeed: {msg}");
        }
    }
}

/// Full terminal E2E proof for EdgeCrab with VS Code Copilot using GPT-5 mini.
#[tokio::test]
#[ignore = "requires VS Code Copilot (VSCODE_IPC_HOOK_CLI or VSCODE_COPILOT_TOKEN)"]
async fn e2e_agent_chat_with_copilot_gpt5_mini() {
    if !copilot_available() {
        eprintln!("Skipping: VS Code Copilot not available");
        return;
    }
    ensure_copilot_authenticated_for_e2e().await;

    let provider = edgecrab_tools::create_provider_for_model("vscode-copilot", COPILOT_TEST_MODEL)
        .expect("should create VsCodeCopilot provider");

    let agent = AgentBuilder::new(COPILOT_TEST_SPEC)
        .provider(provider)
        .max_iterations(3)
        .build()
        .expect("agent build should succeed");

    match agent
        .chat("Reply with exactly: EDGECRAB_GPT5_MINI_OK")
        .await
    {
        Ok(response) => {
            println!("GPT-5 mini response: {response}");
            assert!(!response.is_empty(), "Response should not be empty");
            assert!(
                response.to_uppercase().contains("EDGECRAB_GPT5_MINI_OK"),
                "Expected EDGECRAB_GPT5_MINI_OK in response, got: {response}"
            );
        }
        Err(err) => {
            let msg = err.to_string();
            if is_verified_global_rate_limit(&msg) {
                eprintln!(
                    "Skipping gpt-5-mini due to upstream Copilot weekly/global rate limit: {msg}"
                );
                return;
            }
            panic!("gpt-5-mini chat should succeed: {msg}");
        }
    }
}

/// Full terminal E2E proof for EdgeCrab with VS Code Copilot Auto mode.
#[tokio::test]
#[ignore = "requires VS Code Copilot (VSCODE_IPC_HOOK_CLI or VSCODE_COPILOT_TOKEN)"]
async fn e2e_agent_chat_with_copilot_auto() {
    if !copilot_available() {
        eprintln!("Skipping: VS Code Copilot not available");
        return;
    }
    ensure_copilot_authenticated_for_e2e().await;

    let provider = edgecrab_tools::create_provider_for_model("vscode-copilot", COPILOT_AUTO_MODEL)
        .expect("should create VsCodeCopilot provider for Auto");

    let agent = AgentBuilder::new(COPILOT_AUTO_SPEC)
        .provider(provider)
        .max_iterations(3)
        .build()
        .expect("agent build should succeed");

    match agent.chat("Reply with exactly: EDGECRAB_AUTO_OK").await {
        Ok(response) => {
            println!("Auto response: {response}");
            assert!(!response.is_empty(), "Response should not be empty");
            assert!(
                response.to_uppercase().contains("EDGECRAB_AUTO_OK"),
                "Expected EDGECRAB_AUTO_OK in response, got: {response}"
            );
        }
        Err(err) => {
            let msg = err.to_string();
            if is_verified_global_rate_limit(&msg) {
                eprintln!(
                    "Skipping auto-mode due to upstream Copilot weekly/global rate limit: {msg}"
                );
                return;
            }
            panic!("auto-mode chat should succeed: {msg}");
        }
    }
}

/// Streaming round-trip: tokens arrive via channel.
#[tokio::test]
#[ignore = "requires VS Code Copilot (VSCODE_IPC_HOOK_CLI or VSCODE_COPILOT_TOKEN)"]
async fn e2e_agent_streaming_with_copilot() {
    if !copilot_available() {
        eprintln!("Skipping: VS Code Copilot not available");
        return;
    }
    ensure_copilot_authenticated_for_e2e().await;

    let provider = edgecrab_tools::create_provider_for_model("vscode-copilot", COPILOT_TEST_MODEL)
        .expect("should create VsCodeCopilot provider");

    let agent = Arc::new(
        AgentBuilder::new(COPILOT_TEST_SPEC)
            .provider(provider)
            .max_iterations(2)
            .build()
            .expect("agent build should succeed"),
    );

    let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();

    let agent_clone = Arc::clone(&agent);
    let handle = tokio::spawn(async move {
        match agent_clone
            .chat_streaming("Count from 1 to 3 with spaces.", chunk_tx)
            .await
        {
            Ok(()) => {}
            Err(err) => {
                let msg = err.to_string();
                if is_verified_global_rate_limit(&msg) {
                    eprintln!(
                        "Skipping streaming due to upstream Copilot weekly/global rate limit: {msg}"
                    );
                    return;
                }
                panic!("streaming should succeed: {msg}");
            }
        }
    });

    let mut accumulated = String::new();
    let mut got_done = false;

    while let Some(event) = chunk_rx.recv().await {
        match event {
            StreamEvent::Token(text) => accumulated.push_str(&text),
            StreamEvent::ToolExec { .. } => {} // tool execution events — just ignore in this test
            StreamEvent::ToolProgress { .. } => {} // tool progress events — just ignore in this test
            StreamEvent::ToolDone { .. } => {} // tool completion events — just ignore in this test
            StreamEvent::SubAgentStart { .. } => {} // delegated progress — not relevant here
            StreamEvent::SubAgentReasoning { .. } => {} // delegated progress — not relevant here
            StreamEvent::SubAgentToolExec { .. } => {} // delegated progress — not relevant here
            StreamEvent::SubAgentFinish { .. } => {} // delegated progress — not relevant here
            StreamEvent::RunFinished { .. } => {} // terminal outcome is surfaced separately from transport done
            StreamEvent::Done => {
                got_done = true;
                break;
            }
            StreamEvent::Clarify { .. } => {} // not expected in this test
            StreamEvent::Error(e) => {
                if is_verified_global_rate_limit(&e) {
                    eprintln!(
                        "Skipping streaming due to upstream Copilot weekly/global rate limit: {e}"
                    );
                    return;
                }
                panic!("Unexpected streaming error: {e}")
            }
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
    ensure_copilot_authenticated_for_e2e().await;

    let provider = edgecrab_tools::create_provider_for_model("vscode-copilot", COPILOT_TEST_MODEL)
        .expect("copilot provider");

    let agent = AgentBuilder::new(COPILOT_TEST_SPEC)
        .provider(Arc::clone(&provider))
        .max_iterations(2)
        .build()
        .expect("build");

    // First turn
    let r1 = match agent.chat("Say ALPHA").await {
        Ok(res) => res,
        Err(err) => {
            let msg = err.to_string();
            if is_verified_global_rate_limit(&msg) {
                eprintln!(
                    "Skipping model swap due to upstream Copilot weekly/global rate limit: {msg}"
                );
                return;
            }
            panic!("first chat: {msg}");
        }
    };
    assert!(r1.to_uppercase().contains("ALPHA"), "got: {r1}");

    // Hot-swap to the same model (no-op but validates the plumbing)
    let provider2 = edgecrab_tools::create_provider_for_model("vscode-copilot", COPILOT_TEST_MODEL)
        .expect("second provider");
    agent.swap_model(COPILOT_TEST_SPEC.into(), provider2).await;

    // Second turn after swap
    let r2 = match agent.chat("Say BETA").await {
        Ok(res) => res,
        Err(err) => {
            let msg = err.to_string();
            if is_verified_global_rate_limit(&msg) {
                eprintln!(
                    "Skipping model swap due to upstream Copilot weekly/global rate limit: {msg}"
                );
                return;
            }
            panic!("second chat: {msg}");
        }
    };
    assert!(r2.to_uppercase().contains("BETA"), "got: {r2}");
}
