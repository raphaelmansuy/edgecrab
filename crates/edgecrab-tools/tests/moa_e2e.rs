use std::path::PathBuf;
use std::sync::Arc;

use edgecrab_tools::{AppConfigRef, ToolContext, ToolRegistry};
use edgecrab_types::Platform;
use edgequake_llm::traits::{ChatMessage, CompletionOptions};
use serde_json::json;
use tokio_util::sync::CancellationToken;

fn copilot_available() -> bool {
    std::env::var("VSCODE_IPC_HOOK_CLI").is_ok()
        || std::env::var("VSCODE_IPC_HOOK").is_ok()
        || std::env::var("VSCODE_COPILOT_TOKEN").is_ok()
        || dirs::home_dir()
            .map(|home| home.join(".config/github-copilot/hosts.json").exists())
            .unwrap_or(false)
}

fn live_tool_context(provider: Arc<dyn edgequake_llm::LLMProvider>) -> ToolContext {
    ToolContext {
        task_id: "moa-e2e".into(),
        cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        session_id: "moa-e2e-session".into(),
        user_task: Some("exercise moa end to end".into()),
        cancel: CancellationToken::new(),
        config: AppConfigRef {
            moa_enabled: true,
            parent_active_toolsets: vec!["moa".into()],
            ..Default::default()
        },
        state_db: None,
        platform: Platform::Cli,
        process_table: None,
        provider: Some(provider),
        tool_registry: None,
        delegate_depth: 0,
        sub_agent_runner: None,
        delegation_event_tx: None,
        clarify_tx: None,
        approval_tx: None,
        on_skills_changed: None,
        gateway_sender: None,
        origin_chat: None,
        session_key: None,
        todo_store: None,
        current_tool_call_id: None,
        current_tool_name: None,
        injected_messages: None,
        tool_progress_tx: None,
    }
}

async fn probe_copilot_model(model: &str) -> Result<String, String> {
    let provider = edgecrab_tools::create_provider_for_model("vscode-copilot", model)?;
    let response = provider
        .chat(
            &[ChatMessage::user(
                "Reply with exactly the single word PONG.",
            )],
            Some(&CompletionOptions {
                temperature: Some(0.2),
                max_tokens: Some(64),
                ..Default::default()
            }),
        )
        .await
        .map_err(|err| err.to_string())?;
    Ok(response.content)
}

async fn discover_supported_copilot_models(candidates: &[&str]) -> Vec<(String, String)> {
    let mut supported = Vec::new();
    for model in candidates {
        match probe_copilot_model(model).await {
            Ok(content) => supported.push(((*model).to_string(), content)),
            Err(err) => eprintln!("Skipping unsupported Copilot model {model}: {err}"),
        }
    }
    supported
}

async fn find_unsupported_copilot_model(candidates: &[&str]) -> Option<String> {
    for model in candidates {
        if probe_copilot_model(model).await.is_err() {
            return Some((*model).to_string());
        }
    }
    None
}

#[tokio::test]
#[ignore = "requires live Copilot credentials and network access"]
async fn e2e_moa_dispatch_with_secondary_copilot_model() {
    if !copilot_available() {
        eprintln!("Skipping: Copilot credentials are not available");
        return;
    }

    let supported =
        discover_supported_copilot_models(&["gpt-5-mini", "gpt-4.1", "gpt-4.1-mini"]).await;

    if supported.len() < 2 {
        eprintln!("Skipping: need at least two supported Copilot models for MoA E2E");
        return;
    }

    let active_model = supported[0].0.clone();
    let secondary_model = supported[1].0.clone();
    let direct_probe = supported[1].1.clone();
    assert!(
        direct_probe.to_uppercase().contains("PONG"),
        "secondary provider probe failed: {direct_probe}"
    );

    let active_provider =
        edgecrab_tools::create_provider_for_model("vscode-copilot", &active_model)
            .expect("active copilot provider");
    let ctx = live_tool_context(active_provider);
    let registry = ToolRegistry::new();

    let response = registry
        .dispatch(
            "moa",
            json!({
                "user_prompt": "Reply with exactly the single word PONG.",
                "reference_models": [format!("vscode-copilot/{secondary_model}")],
                "aggregator_model": format!("vscode-copilot/{secondary_model}")
            }),
            &ctx,
        )
        .await
        .expect("moa dispatch should succeed");

    assert!(
        response.contains(&format!("Aggregator: vscode-copilot/{secondary_model}")),
        "unexpected response header: {response}"
    );
    assert!(
        response.to_uppercase().contains("PONG"),
        "expected synthesized content to contain PONG, got: {response}"
    );
}

#[tokio::test]
#[ignore = "requires live Copilot credentials and network access"]
async fn e2e_moa_falls_back_when_configured_aggregator_is_unsupported() {
    if !copilot_available() {
        eprintln!("Skipping: Copilot credentials are not available");
        return;
    }

    let supported = discover_supported_copilot_models(&["gpt-5-mini", "gpt-4.1"]).await;
    if supported.is_empty() {
        eprintln!("Skipping: need at least one supported Copilot model");
        return;
    }

    let Some(unsupported_model) =
        find_unsupported_copilot_model(&["gpt-4.1-mini", "gpt-4.5-preview", "not-a-real-model"])
            .await
    else {
        eprintln!("Skipping: could not find an unsupported Copilot model to exercise fallback");
        return;
    };

    let active_model = supported[0].0.clone();
    let active_provider =
        edgecrab_tools::create_provider_for_model("vscode-copilot", &active_model)
            .expect("active copilot provider");
    let ctx = live_tool_context(active_provider);
    let registry = ToolRegistry::new();

    let response = registry
        .dispatch(
            "moa",
            json!({
                "user_prompt": "Reply with exactly the single word PONG.",
                "reference_models": [format!("vscode-copilot/{active_model}")],
                "aggregator_model": format!("vscode-copilot/{unsupported_model}")
            }),
            &ctx,
        )
        .await
        .expect("moa dispatch should succeed via aggregator fallback");

    assert!(
        response.contains(&format!(
            "Requested aggregator: vscode-copilot/{unsupported_model}"
        )),
        "unexpected response header: {response}"
    );
    assert!(
        response.contains(&format!("Aggregator: vscode-copilot/{active_model}")),
        "expected fallback aggregator to be the active model, got: {response}"
    );
    assert!(
        response.to_uppercase().contains("PONG"),
        "expected synthesized content to contain PONG, got: {response}"
    );
}

#[tokio::test]
#[ignore = "requires live Copilot credentials and network access"]
async fn e2e_moa_uses_active_model_safety_expert_when_configured_references_fail() {
    if !copilot_available() {
        eprintln!("Skipping: Copilot credentials are not available");
        return;
    }

    let supported = discover_supported_copilot_models(&["gpt-5-mini", "gpt-4.1"]).await;
    if supported.is_empty() {
        eprintln!("Skipping: need at least one supported Copilot model");
        return;
    }

    let Some(unsupported_model) =
        find_unsupported_copilot_model(&["gpt-4.1-mini", "gpt-4.5-preview", "not-a-real-model"])
            .await
    else {
        eprintln!("Skipping: could not find an unsupported Copilot model to exercise fallback");
        return;
    };

    let active_model = supported[0].0.clone();
    let active_provider =
        edgecrab_tools::create_provider_for_model("vscode-copilot", &active_model)
            .expect("active copilot provider");
    let ctx = live_tool_context(active_provider);
    let registry = ToolRegistry::new();

    let response = registry
        .dispatch(
            "moa",
            json!({
                "user_prompt": "Reply with exactly the single word PONG.",
                "reference_models": [format!("vscode-copilot/{unsupported_model}")],
                "aggregator_model": format!("vscode-copilot/{active_model}")
            }),
            &ctx,
        )
        .await
        .expect("moa dispatch should succeed via active-model safety expert");

    assert!(
        response.contains(&format!(
            "Active-model safety expert: vscode-copilot/{active_model}"
        )),
        "expected response to mention the implicit safety expert, got: {response}"
    );
    assert!(
        response.contains(&format!("Reference models: vscode-copilot/{active_model}")),
        "expected active model to provide the successful reference response, got: {response}"
    );
    assert!(
        response.to_uppercase().contains("PONG"),
        "expected synthesized content to contain PONG, got: {response}"
    );
}
