//! # End-to-End Smoke Test
//!
//! Exercises the core Rust SDK API against local Ollama.
//! Validates the SDK surface with real model calls, session storage,
//! streaming, export, compression, and memory operations.
//!
//! Requires: Ollama running locally with `gemma4:latest` pulled.
//!
//! ```bash
//! cargo run --example e2e_smoke
//! ```

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use edgecrab_sdk::Message;
use edgecrab_sdk::prelude::*;
use edgecrab_sdk_core::{MemoryManager, SdkSession, StreamEvent};

const MODEL: &str = "ollama/gemma4:latest";
const CORE_FEATURES: &[&str] = &[
    "SdkModelCatalog.provider_ids",
    "SdkModelCatalog.models_for_provider",
    "SdkModelCatalog.context_window",
    "SdkModelCatalog.pricing",
    "SdkModelCatalog.flat_catalog",
    "SdkModelCatalog.default_model_for",
    "SdkModelCatalog.estimate_cost",
    "SdkConfig.default_config",
    "SdkConfig.mutation",
    "SdkAgent.chat",
    "SdkAgent.run",
    "SdkAgent.run_conversation",
    "SdkAgent.stream",
    "SdkAgent.session_id",
    "SdkAgent.chat_in_cwd",
    "SdkAgent.batch",
    "SdkAgent.model",
    "SdkAgent.set_model",
    "SdkAgent.fork",
    "SdkAgent.messages",
    "SdkAgent.session_snapshot",
    "SdkAgent.export",
    "SdkAgent.tool_names",
    "SdkAgent.toolset_summary",
    "SdkAgent.compress",
    "SdkAgent.new_session",
    "SdkToolRegistry.custom_tool",
    "SdkSession.list_sessions",
    "SdkSession.search_sessions",
    "SdkSession.get_messages",
    "SdkSession.rename_session",
    "SdkSession.prune_sessions",
    "SdkSession.stats",
    "MemoryManager.read_write",
];

#[derive(Deserialize)]
struct EchoArgs {
    message: String,
}

struct EchoTool;

#[async_trait]
impl ToolHandler for EchoTool {
    fn name(&self) -> &'static str {
        "echo"
    }
    fn toolset(&self) -> &'static str {
        "test"
    }
    fn emoji(&self) -> &'static str {
        "🔁"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "echo".into(),
            description: "Echo a message back to the caller verbatim.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string", "description": "Text to echo" }
                },
                "required": ["message"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let a: EchoArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "echo".into(),
            message: e.to_string(),
        })?;
        Ok(json!({ "echo": a.message }).to_string())
    }
}

fn ok(label: &str) {
    println!("  ✓ {label}");
}

fn section(title: &str) {
    println!("\n── {title} ──");
}

fn expect_contains(text: &str, expected: &str) {
    assert!(
        text.to_uppercase().contains(&expected.to_uppercase()),
        "Expected {:?} to include {:?}",
        text,
        expected
    );
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut covered = std::collections::BTreeSet::new();

    println!("EdgeCrab SDK — E2E Smoke Test");
    println!("Model: {MODEL}");
    println!("═══════════════════════════════════════");

    section("1. SdkModelCatalog");
    {
        use edgecrab_sdk::types::SdkModelCatalog;

        let providers = SdkModelCatalog::provider_ids();
        assert!(!providers.is_empty(), "No providers in catalog");
        assert!(providers.contains(&"ollama".to_string()), "ollama missing");
        covered.insert("SdkModelCatalog.provider_ids");
        ok(&format!("{} providers in catalog", providers.len()));

        let openai_models = SdkModelCatalog::models_for_provider("openai");
        assert!(
            !openai_models.is_empty(),
            "models_for_provider returned empty"
        );
        covered.insert("SdkModelCatalog.models_for_provider");
        ok(&format!("openai models → {} entries", openai_models.len()));

        let ctx = SdkModelCatalog::context_window("openai", "gpt-4o");
        assert!(ctx.is_some(), "context_window missing for openai/gpt-4o");
        covered.insert("SdkModelCatalog.context_window");
        ok(&format!("context_window → {:?}", ctx));

        let pricing = SdkModelCatalog::pricing("openai", "gpt-4o");
        assert!(pricing.is_some(), "pricing lookup failed");
        covered.insert("SdkModelCatalog.pricing");
        ok(&format!("pricing lookup → {:?}", pricing));

        let flat = SdkModelCatalog::flat_catalog();
        assert!(!flat.is_empty(), "flat_catalog empty");
        covered.insert("SdkModelCatalog.flat_catalog");
        ok(&format!("flat_catalog → {} models", flat.len()));

        let default_model = SdkModelCatalog::default_model_for("openai");
        assert!(default_model.is_some(), "default_model_for returned None");
        covered.insert("SdkModelCatalog.default_model_for");
        ok(&format!("default openai model → {:?}", default_model));

        let est = SdkModelCatalog::estimate_cost("openai", "gpt-4o", 1000, 200);
        assert!(est.is_some(), "estimate_cost returned None");
        covered.insert("SdkModelCatalog.estimate_cost");
        ok(&format!("cost estimation → {:?}", est));
    }

    section("2. SdkConfig");
    {
        let cfg = SdkConfig::default_config();
        let _model = cfg.default_model().to_string();
        covered.insert("SdkConfig.default_config");
        ok("default config loaded");

        let mut cfg2 = SdkConfig::default_config();
        cfg2.set_default_model(MODEL);
        cfg2.set_max_iterations(5);
        assert_eq!(cfg2.default_model(), MODEL);
        assert_eq!(cfg2.max_iterations(), 5);
        covered.insert("SdkConfig.mutation");
        ok("config mutation");
    }

    section("3. Core agent methods");
    let agent = SdkAgent::builder(MODEL)?
        .max_iterations(3)
        .quiet_mode(true)
        .skip_context_files(true)
        .skip_memory(true)
        .build()?;

    let reply = agent.chat("Reply with exactly: PONG").await?;
    expect_contains(&reply, "PONG");
    covered.insert("SdkAgent.chat");
    ok(&format!(
        "chat → {}",
        reply
            .chars()
            .take(60)
            .collect::<String>()
            .replace('\n', " ")
    ));

    let result = agent.run("Reply with exactly: OK").await?;
    expect_contains(&result.final_response, "OK");
    assert!(!result.session_id.is_empty(), "session_id empty");
    let run_session_id = result.session_id.clone();
    covered.insert("SdkAgent.run");
    ok(&format!(
        "run → {:?} | session_id={:.8} | cost=${:.6}",
        result.final_response.chars().take(40).collect::<String>(),
        result.session_id,
        result.cost.total_cost,
    ));

    let conv = agent
        .run_conversation(
            "Reply with exactly: CONV_OK",
            Some("You are terse. Obey exactly."),
            Some(vec![Message::user("Earlier note: keep replies short.")]),
        )
        .await?;
    expect_contains(&conv.final_response, "CONV_OK");
    covered.insert("SdkAgent.run_conversation");
    ok(&format!(
        "run_conversation → {:.40}",
        conv.final_response.replace('\n', " ")
    ));

    agent.set_reasoning_effort(Some("low".into())).await;
    agent.set_streaming(true).await;
    let mut rx = agent.stream("Reply with exactly: STREAM_OK").await?;
    let mut streamed = String::new();
    let mut saw_done = false;
    while let Some(event) = rx.recv().await {
        match event {
            StreamEvent::Token(text) => streamed.push_str(&text),
            StreamEvent::Done => {
                saw_done = true;
                break;
            }
            _ => {}
        }
    }
    assert!(saw_done, "expected stream termination");
    expect_contains(&streamed, "STREAM_OK");
    covered.insert("SdkAgent.stream");
    ok(&format!("stream → {:.40}", streamed.replace('\n', " ")));

    let sid = agent.session_id().await;
    assert!(sid.is_some(), "session_id getter returned None");
    covered.insert("SdkAgent.session_id");
    ok(&format!("session_id → {}", sid.unwrap_or_default()));

    let cwd = std::env::current_dir()?;
    let cwd_reply = agent
        .chat_in_cwd("Reply with exactly: CWD_OK", &cwd)
        .await?;
    expect_contains(&cwd_reply, "CWD_OK");
    covered.insert("SdkAgent.chat_in_cwd");
    ok(&format!(
        "chat_in_cwd → {:.40}",
        cwd_reply.replace('\n', " ")
    ));

    section("4. Parallelism and state");
    let prompts = ["Reply with exactly: YES", "Reply with exactly: NO"];
    let results = agent.batch(&prompts).await;
    assert_eq!(results.len(), 2, "batch returned wrong number of results");
    let yes = match &results[0] {
        Ok(text) => text.clone(),
        Err(err) => return Err(format!("batch[0] failed: {err}").into()),
    };
    let no = match &results[1] {
        Ok(text) => text.clone(),
        Err(err) => return Err(format!("batch[1] failed: {err}").into()),
    };
    expect_contains(&yes, "YES");
    expect_contains(&no, "NO");
    covered.insert("SdkAgent.batch");
    ok(&format!(
        "batch → {}, {}",
        yes.replace('\n', " "),
        no.replace('\n', " ")
    ));

    let before_model = agent.model().await;
    assert!(!before_model.is_empty(), "model getter empty");
    covered.insert("SdkAgent.model");
    ok(&format!("model getter → {before_model}"));

    agent.set_model(MODEL).await?;
    let after_model = agent.model().await;
    assert_eq!(after_model, MODEL);
    covered.insert("SdkAgent.set_model");
    ok(&format!("set_model → {after_model}"));

    let fork_a = agent.fork().await?;
    let fork_b = agent.fork().await?;
    let (ra, rb) = tokio::join!(
        fork_a.chat("Reply with exactly: FORK_A"),
        fork_b.chat("Reply with exactly: FORK_B"),
    );
    let ra = ra?;
    let rb = rb?;
    expect_contains(&ra, "FORK_A");
    expect_contains(&rb, "FORK_B");
    covered.insert("SdkAgent.fork");
    ok(&format!(
        "fork replies → {} / {}",
        ra.replace('\n', " "),
        rb.replace('\n', " ")
    ));

    let msgs = agent.messages().await;
    assert!(!msgs.is_empty(), "messages() returned empty");
    covered.insert("SdkAgent.messages");
    ok(&format!("messages → {} entries", msgs.len()));

    let snapshot = agent.session_snapshot().await;
    assert!(snapshot.message_count > 0, "session_snapshot empty");
    covered.insert("SdkAgent.session_snapshot");
    ok(&format!(
        "session_snapshot → {} messages",
        snapshot.message_count
    ));

    let exported = agent.export().await;
    assert!(!exported.messages.is_empty(), "export returned no messages");
    covered.insert("SdkAgent.export");
    ok(&format!("export → {} messages", exported.messages.len()));

    let tool_names = agent.tool_names().await;
    covered.insert("SdkAgent.tool_names");
    ok(&format!("tool_names → {} tools", tool_names.len()));

    let toolsets = agent.toolset_summary().await;
    covered.insert("SdkAgent.toolset_summary");
    ok(&format!("toolset_summary → {} groups", toolsets.len()));

    agent.compress().await;
    covered.insert("SdkAgent.compress");
    ok("compress completed");

    agent.new_session().await;
    assert!(
        agent.messages().await.is_empty(),
        "new_session did not clear history"
    );
    covered.insert("SdkAgent.new_session");
    ok("new_session cleared history");

    section("5. Custom tool (SdkToolRegistry)");
    {
        let mut registry = SdkToolRegistry::new();
        registry.register(Box::new(EchoTool));

        let tool_agent = SdkAgent::builder(MODEL)?
            .max_iterations(3)
            .quiet_mode(true)
            .skip_context_files(true)
            .skip_memory(true)
            .tools(Arc::new(registry.into_inner()))
            .build()?;

        let reply = tool_agent
            .chat("Use the echo tool once with message='hello_sdk'. Then reply with exactly: DONE")
            .await?;
        assert!(!reply.is_empty(), "tool-backed reply empty");
        covered.insert("SdkToolRegistry.custom_tool");
        ok(&format!("tool response → {:.60}", reply.replace('\n', " ")));
    }

    section("6. Session store and memory");
    {
        let db_path = edgecrab_home().join("sessions.db");
        let db = SdkSession::open(&db_path)?;

        let sessions = db.list_sessions(5)?;
        covered.insert("SdkSession.list_sessions");
        ok(&format!("list_sessions → {} sessions", sessions.len()));

        let hits = db.search_sessions("PONG", 5)?;
        covered.insert("SdkSession.search_sessions");
        ok(&format!("search_sessions → {} hits", hits.len()));

        let messages = db.get_messages(&run_session_id)?;
        covered.insert("SdkSession.get_messages");
        ok(&format!("get_messages → {} messages", messages.len()));

        db.rename_session(&run_session_id, "Rust SDK E2E")?;
        covered.insert("SdkSession.rename_session");
        ok("rename_session applied");

        let pruned = db.prune_sessions(36500, None)?;
        covered.insert("SdkSession.prune_sessions");
        ok(&format!("prune_sessions → {} deleted", pruned));

        let stats = db.stats()?;
        covered.insert("SdkSession.stats");
        ok(&format!(
            "session stats → {} sessions / {} messages",
            stats.total_sessions, stats.total_messages,
        ));

        let mem = MemoryManager::new(edgecrab_home());
        let token = format!(
            "rust-sdk-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis()
        );
        mem.write("memory", &token).await?;
        let content = mem.read("memory").await?;
        assert!(content.contains(&token), "memory write/read failed");
        let entries = mem.entries("memory").await?;
        let removed = mem.remove("memory", &token).await?;
        assert!(removed, "memory remove failed");
        covered.insert("MemoryManager.read_write");
        ok(&format!("memory round-trip → {} entries", entries.len()));
    }

    let pct = (covered.len() as f64 / CORE_FEATURES.len() as f64) * 100.0;
    println!(
        "\nCore API coverage: {}/{} ({pct:.1}%)",
        covered.len(),
        CORE_FEATURES.len()
    );
    assert!(pct >= 80.0, "Coverage below target: {pct:.1}%");

    println!("\n═══════════════════════════════════════");
    println!("E2E smoke test PASSED ✓");
    println!("Rust SDK coverage target PASSED ✓");
    Ok(())
}
