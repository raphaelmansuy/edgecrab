//! # Gateway runner — boots platform adapters and axum health endpoint
//!
//! WHY axum: Lightweight async HTTP framework that integrates seamlessly
//! with tokio. Used for the health endpoint, webhook inbound routes,
//! and the OpenAI-compatible API server.
//!
//! ```text
//!   Gateway::run()
//!     ├── boot health/API server (axum)
//!     ├── boot platform adapters (tokio::spawn each)
//!     ├── start session cleanup task
//!     └── run message dispatch loop (mpsc receiver)
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use edgecrab_tools::tools::vision::VisionAnalyzeTool;
use edgecrab_tools::{AppConfigRef, ToolContext, ToolHandler};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::config::GatewayConfig;
use crate::delivery::DeliveryRouter;
use crate::event_processor::GatewayEventProcessor;
use crate::hooks::{HookContext, HookRegistry};
use crate::interactions::{InteractionBroker, PendingInteractionKind};
use crate::platform::{IncomingMessage, PlatformAdapter};
use crate::session::{SessionKey, SessionManager};
use crate::webhook::WebhookPayload;
use edgecrab_core::{Agent, IsolatedAgentOptions};

/// Deterministic gateway-side image pre-analysis prompt.
///
/// WHY eager analysis: Hermes auto-enriches inbound images before the model
/// turn. Without this, EdgeCrab relies on the model noticing an attachment
/// block and choosing `vision_analyze` correctly, which is weaker and leaks
/// toolset/runtime misconfiguration back to the user. The gateway should
/// normalize image inputs into text context before dispatch.
const GATEWAY_IMAGE_ANALYSIS_PROMPT: &str = "\
Describe everything visible in this image in thorough detail. Include any text, \
numbers, code, UI elements, objects, people, colors, layout, and notable context.";

/// Hard cap on eager gateway image analyses per turn.
///
/// WHY bounded: a user can upload a large album. Analyzing every image before
/// the first token would create excessive latency and cost. We still surface
/// every attachment path in the injected context, but pre-analyze only the
/// first few images so single-image and small-batch UX stays reliable.
const MAX_GATEWAY_EAGER_IMAGE_ANALYSES: usize = 4;

/// Help text shown when a user sends /help to the gateway.
const HELP_TEXT: &str = "\
*Available commands:*

/help    - Show this help message
/new     - Start a fresh conversation (clears history)
/reset   - Alias for /new
/stop    - Stop the current agent response
/retry   - Retry your last message
/status  - Show whether an agent is currently running
/usage   - Show session stats
/background - Run a prompt in a separate background session
/hooks   - List loaded event hooks
/approve - Approve the oldest pending command request
/deny    - Deny the oldest pending approval or clarify request

Any other message is forwarded to the AI agent.

Tip: If you send a message while the agent is responding, it will be
queued and processed after the current response finishes. Use /stop
to cancel the current response and discard the queue.

When the agent asks for clarification, reply with plain text or a
choice number. When it asks for approval, reply `/approve`,
`/approve session`, `/approve always`, or `/deny`.";

fn parse_approval_reply(text: &str) -> Option<edgecrab_core::ApprovalChoice> {
    let normalized = text.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "/approve" | "approve" | "allow" | "yes" | "y" => Some(edgecrab_core::ApprovalChoice::Once),
        "/approve session" | "approve session" | "allow session" => {
            Some(edgecrab_core::ApprovalChoice::Session)
        }
        "/approve always" | "approve always" | "allow always" => {
            Some(edgecrab_core::ApprovalChoice::Always)
        }
        "/deny" | "deny" | "reject" | "no" | "n" => Some(edgecrab_core::ApprovalChoice::Deny),
        _ => None,
    }
}

fn parse_clarify_answer(text: &str, choices: &[String]) -> String {
    let trimmed = text.trim();
    if let Ok(index) = trimmed.parse::<usize>() {
        if (1..=choices.len()).contains(&index) {
            return choices[index - 1].clone();
        }
    }
    trimmed.to_string()
}

fn pending_interaction_hint(kind: &PendingInteractionKind) -> String {
    match kind {
        PendingInteractionKind::Approval { .. } => {
            "A command is waiting for approval. Reply `/approve`, `/approve session`, `/approve always`, or `/deny`.".into()
        }
        PendingInteractionKind::Clarify { choices, .. } => {
            if choices.as_ref().is_some_and(|list| !list.is_empty()) {
                "The agent is waiting for your answer. Reply with the choice number or plain text. Use `/deny` to cancel.".into()
            } else {
                "The agent is waiting for your answer. Reply with plain text. Use `/deny` to cancel.".into()
            }
        }
    }
}

fn image_attachment_sources(msg: &IncomingMessage) -> Vec<String> {
    msg.metadata
        .attachments
        .iter()
        .filter(|attachment| {
            matches!(
                attachment.kind,
                crate::platform::MessageAttachmentKind::Image
            )
        })
        .filter_map(|attachment| attachment.vision_source().map(str::to_string))
        .collect()
}

fn render_gateway_image_context(
    user_text: &str,
    image_sources: &[String],
    analyses: &[String],
    preanalysis_failures: &[String],
    skipped_count: usize,
) -> String {
    if image_sources.is_empty() {
        return user_text.to_string();
    }

    let mut lines = Vec::new();
    lines.push("*** ATTACHED IMAGES ***".to_string());
    lines.push(format!(
        "The user attached {} image(s).",
        image_sources.len()
    ));
    lines.push("Image sources:".to_string());
    for (idx, source) in image_sources.iter().enumerate() {
        lines.push(format!("- Image {}: {}", idx + 1, source));
    }
    if !analyses.is_empty() {
        lines.push(
            "Gateway pre-analysis already ran before this turn. Use it as primary context."
                .to_string(),
        );
        for (idx, analysis) in analyses.iter().enumerate() {
            lines.push(format!("Pre-analysis {}:\n{}", idx + 1, analysis));
        }
    }
    if !preanalysis_failures.is_empty() {
        lines.push(
            "Some images could not be pre-analyzed automatically. If `vision_analyze` is \
             available in this session and you need more detail, you may call it on the \
             image source(s) above."
                .to_string(),
        );
        for failure in preanalysis_failures {
            lines.push(format!("- {}", failure));
        }
    } else {
        lines.push(
            "If you need more detail than the gateway pre-analysis above, you may call \
             `vision_analyze` on one of the listed image sources if that tool is available \
             in this session."
                .to_string(),
        );
    }
    if skipped_count > 0 {
        lines.push(format!(
            "{skipped_count} additional image(s) were attached but not pre-analyzed to keep latency bounded."
        ));
    }
    lines.push("*** END ATTACHED IMAGES ***".to_string());

    let block = lines.join("\n");
    if user_text.trim().is_empty() {
        block
    } else {
        format!("{user_text}\n\n{block}")
    }
}

async fn build_effective_text(agent: &Agent, msg: &IncomingMessage) -> String {
    let image_sources = image_attachment_sources(msg);
    if image_sources.is_empty() {
        return msg.text.clone();
    }

    let provider = agent.provider_handle().await;
    let auxiliary = agent.auxiliary_config().await;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let session_key = format!(
        "gateway-image-preanalysis-{}",
        message_origin_recipient(msg)
    );
    let config = AppConfigRef {
        edgecrab_home: edgecrab_core::edgecrab_home(),
        auxiliary_provider: auxiliary.provider,
        auxiliary_model: auxiliary.model,
        auxiliary_base_url: auxiliary.base_url,
        auxiliary_api_key_env: auxiliary.api_key_env,
        ..Default::default()
    };

    let mut analyses = Vec::new();
    let mut failures = Vec::new();
    for (idx, source) in image_sources
        .iter()
        .take(MAX_GATEWAY_EAGER_IMAGE_ANALYSES)
        .enumerate()
    {
        let ctx = ToolContext {
            task_id: format!("gateway-image-preanalysis-{}", idx + 1),
            cwd: cwd.clone(),
            session_id: session_key.clone(),
            user_task: None,
            cancel: CancellationToken::new(),
            config: config.clone(),
            state_db: None,
            platform: msg.platform,
            process_table: None,
            provider: Some(provider.clone()),
            tool_registry: None,
            delegate_depth: 0,
            sub_agent_runner: None,
            clarify_tx: None,
            approval_tx: None,
            on_skills_changed: None,
            gateway_sender: None,
            origin_chat: None,
            session_key: Some(session_key.clone()),
            todo_store: None,
        };

        match VisionAnalyzeTool
            .execute(
                serde_json::json!({
                    "image_source": source,
                    "prompt": GATEWAY_IMAGE_ANALYSIS_PROMPT,
                    "detail": "high",
                }),
                &ctx,
            )
            .await
        {
            Ok(analysis) => analyses.push(analysis),
            Err(error) => failures.push(format!("Image {} ({source}): {error}", idx + 1)),
        }
    }

    render_gateway_image_context(
        &msg.text,
        &image_sources,
        &analyses,
        &failures,
        image_sources
            .len()
            .saturating_sub(MAX_GATEWAY_EAGER_IMAGE_ANALYSES),
    )
}

async fn deliver_text_and_media(
    delivery: &DeliveryRouter,
    adapter: Option<Arc<dyn PlatformAdapter>>,
    response: &str,
    platform: edgecrab_types::Platform,
    metadata: &crate::platform::MessageMetadata,
) -> anyhow::Result<usize> {
    let (cleaned, media_refs) = crate::platform::extract_media_from_response(response);

    let text_result = delivery
        .deliver(&cleaned, platform, metadata)
        .await
        .map(|_| cleaned.len())?;

    if !media_refs.is_empty() {
        if let Some(adapter) = adapter {
            for mref in &media_refs {
                let result = if mref.is_image {
                    adapter.send_photo(&mref.path, None, metadata).await
                } else {
                    adapter.send_document(&mref.path, None, metadata).await
                };
                if let Err(e) = result {
                    tracing::warn!(
                        path = %mref.path,
                        error = %e,
                        "media delivery failed"
                    );
                }
            }
        }
    }

    Ok(text_result)
}

fn delivery_recipient(chat_id: &str, thread_id: Option<&str>) -> String {
    match thread_id.filter(|thread_id| !thread_id.is_empty()) {
        Some(thread_id) => format!("{chat_id}:{thread_id}"),
        None => chat_id.to_string(),
    }
}

fn message_origin_recipient(msg: &IncomingMessage) -> String {
    let chat_id = msg
        .channel_id
        .as_deref()
        .or(msg.metadata.channel_id.as_deref())
        .unwrap_or(msg.user_id.as_str());
    let thread_id = msg
        .thread_id
        .as_deref()
        .or(msg.metadata.thread_id.as_deref());
    delivery_recipient(chat_id, thread_id)
}

/// Shared gateway state, passed to axum handlers via State extractor.
#[derive(Clone)]
pub struct GatewayState {
    pub session_manager: Arc<SessionManager>,
    pub hook_registry: Arc<HookRegistry>,
    pub message_tx: mpsc::Sender<IncomingMessage>,
    pub cancel: CancellationToken,
}

/// The main gateway service.
pub struct Gateway {
    config: GatewayConfig,
    adapters: Vec<Arc<dyn PlatformAdapter>>,
    session_manager: Arc<SessionManager>,
    delivery_router: Arc<DeliveryRouter>,
    hook_registry: Arc<HookRegistry>,
    cancel: CancellationToken,
    /// Agent for LLM dispatch — set via `set_agent()`.
    agent: Option<Arc<Agent>>,
    /// Per-session cancellation tokens.
    ///
    /// WHY: One slot per session. When a task finishes it removes its entry.
    /// Presence in this map means the task is still running.
    running_sessions: Arc<tokio::sync::Mutex<HashMap<String, CancellationToken>>>,
    /// One pending (queued) message per session.
    ///
    /// WHY: When a new user message arrives while an agent task is already
    /// running for that session, we queue it here instead of cancelling the
    /// running task (which would truncate the in-progress response). After the
    /// task finishes it re-dispatches the pending message into the main channel.
    /// Only the latest message is kept — older ones are replaced.
    pending_messages: Arc<tokio::sync::Mutex<HashMap<String, IncomingMessage>>>,
    /// Last user message text per session — enables the /retry command.
    last_messages: Arc<tokio::sync::Mutex<HashMap<String, String>>>,
    /// Pending approval / clarify interactions keyed by gateway session.
    interaction_broker: Arc<InteractionBroker>,
}

impl Gateway {
    pub fn new(config: GatewayConfig, cancel: CancellationToken) -> Self {
        let session_manager = Arc::new(SessionManager::new(config.idle_timeout()));

        // Discover file-based hooks from ~/.edgecrab/hooks/
        let mut hook_registry = HookRegistry::new();
        hook_registry.discover_and_load();

        Self {
            config,
            adapters: Vec::new(),
            session_manager,
            delivery_router: Arc::new(DeliveryRouter::new()),
            hook_registry: Arc::new(hook_registry),
            cancel,
            agent: None,
            running_sessions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            pending_messages: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            last_messages: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            interaction_broker: InteractionBroker::new(),
        }
    }

    /// Register a platform adapter.
    pub fn add_adapter(&mut self, adapter: Arc<dyn PlatformAdapter>) {
        self.adapters.push(adapter);
    }

    /// Set the agent for LLM dispatch.
    pub fn set_agent(&mut self, agent: Arc<Agent>) {
        self.agent = Some(agent);
    }

    /// Set the hook registry.
    pub fn set_hooks(&mut self, registry: HookRegistry) {
        self.hook_registry = Arc::new(registry);
    }

    /// Set the delivery router.
    pub fn set_delivery(&mut self, router: DeliveryRouter) {
        self.delivery_router = Arc::new(router);
    }

    /// Returns `true` if the user is authorized to use the gateway.
    ///
    /// Authorization rules (first match wins):
    /// 1. `GATEWAY_ALLOW_ALL_USERS=true|1|yes`  → allow everyone
    /// 2. `{PLATFORM}_ALLOW_ALL_USERS=true|1|yes` → allow everyone on that platform
    /// 3. `GATEWAY_ALLOWED_USERS=id1,id2` / `{PLATFORM}_ALLOWED_USERS=id1,id2`
    ///    → allow listed IDs only
    /// 4. If **no** allowlist env-var is configured at all → open gateway
    ///    (suitable for single-user / local deployments)
    ///
    /// Mirrors hermes-agent's `_is_user_authorized()` so operators can reuse
    /// the same env-var configuration across both gateways.
    fn is_user_authorized(&self, msg: &IncomingMessage) -> bool {
        // 1. Global allow-all override
        let allow_all = std::env::var("GATEWAY_ALLOW_ALL_USERS").unwrap_or_default();
        if matches!(allow_all.to_ascii_lowercase().trim(), "true" | "1" | "yes") {
            return true;
        }

        // 2. Per-platform allow-all override
        let platform_allow_all_var = match msg.platform {
            edgecrab_types::Platform::Telegram => "TELEGRAM_ALLOW_ALL_USERS",
            edgecrab_types::Platform::Discord => "DISCORD_ALLOW_ALL_USERS",
            _ => "",
        };
        if !platform_allow_all_var.is_empty() {
            let v = std::env::var(platform_allow_all_var).unwrap_or_default();
            if matches!(v.to_ascii_lowercase().trim(), "true" | "1" | "yes") {
                return true;
            }
        }

        // 3. Collect allowlists from env vars
        let global_list = std::env::var("GATEWAY_ALLOWED_USERS").unwrap_or_default();
        let platform_list_var = match msg.platform {
            edgecrab_types::Platform::Telegram => "TELEGRAM_ALLOWED_USERS",
            edgecrab_types::Platform::Discord => "DISCORD_ALLOWED_USERS",
            _ => "",
        };
        let platform_list = if platform_list_var.is_empty() {
            String::new()
        } else {
            std::env::var(platform_list_var).unwrap_or_default()
        };

        // 4. If no allowlist is configured → open gateway
        if global_list.trim().is_empty() && platform_list.trim().is_empty() {
            return true;
        }

        // 5. Check whether user_id is in either allowlist
        let user_id = msg.user_id.as_str();
        global_list
            .split(',')
            .chain(platform_list.split(','))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .any(|allowed| allowed == user_id)
    }

    ///
    /// WHY: The cron scheduler needs to deliver job output to external platforms
    /// without going through the full gateway message loop. This method snapshots
    /// the registered adapters into a standalone `DeliveryRouter` wrapped by a
    /// `GatewaySenderBridge` that implements the `GatewaySender` trait.
    ///
    /// Call this AFTER all `add_adapter()` calls and BEFORE spawning the cron tick.
    pub async fn build_sender(&self) -> Arc<dyn edgecrab_tools::registry::GatewaySender> {
        let mut router = DeliveryRouter::new();
        for adapter in &self.adapters {
            router.register(adapter.clone());
        }
        let state_db = match self.agent.as_ref() {
            Some(agent) => agent.state_db().await,
            None => None,
        };
        Arc::new(crate::sender::GatewaySenderBridge::new(
            Arc::new(router),
            state_db,
        ))
    }

    /// Run the gateway until cancellation.
    ///
    /// This starts the HTTP server, boots all platform adapters, and
    /// enters the message dispatch loop.
    pub async fn run(&self) -> anyhow::Result<()> {
        let (tx, mut rx) = mpsc::channel::<IncomingMessage>(256);
        let mut delivery_router = DeliveryRouter::new();

        // Emit startup hook
        let platform_names: Vec<String> = self
            .adapters
            .iter()
            .map(|a| format!("{:?}", a.platform()).to_lowercase())
            .collect();
        self.hook_registry
            .emit(
                "gateway:startup",
                &HookContext::new("gateway:startup")
                    .with_value("platforms", serde_json::json!(platform_names)),
            )
            .await;

        tracing::info!(
            adapters = self.adapters.len(),
            bind = %self.config.bind_addr(),
            "starting gateway"
        );

        if let Some(agent) = self.agent.as_ref() {
            if let Some(db) = agent.state_db().await {
                let _ = crate::channel_directory::build_from_sessions(&db);
            }
        }

        // Build axum router
        let state = GatewayState {
            session_manager: self.session_manager.clone(),
            hook_registry: self.hook_registry.clone(),
            message_tx: tx.clone(),
            cancel: self.cancel.clone(),
        };
        let app = build_router(state);

        // Start HTTP server
        let bind_addr = self.config.bind_addr();
        let cancel = self.cancel.clone();
        // Use a oneshot to detect immediate bind failures and propagate them
        // back to `run()` so the gateway shuts down cleanly instead of
        // continuing without an HTTP server.
        let (bind_ok_tx, bind_ok_rx) = tokio::sync::oneshot::channel::<anyhow::Result<()>>();
        tokio::spawn(async move {
            let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
                Ok(l) => {
                    let _ = bind_ok_tx.send(Ok(()));
                    l
                }
                Err(e) => {
                    tracing::error!(error = %e, addr = %bind_addr, "failed to bind");
                    let _ = bind_ok_tx.send(Err(anyhow::anyhow!(
                        "gateway failed to bind {bind_addr}: {e}"
                    )));
                    return;
                }
            };
            tracing::info!(addr = %bind_addr, "gateway HTTP server listening");
            let server = axum::serve(listener, app);
            tokio::select! {
                result = server => {
                    if let Err(e) = result {
                        tracing::error!(error = %e, "HTTP server error");
                    }
                }
                _ = cancel.cancelled() => {
                    tracing::info!("shutting down HTTP server");
                }
            }
        });

        // Propagate bind failure immediately — the gateway cannot function
        // without its HTTP surface (health + webhook endpoints).
        match bind_ok_rx.await {
            Ok(Ok(())) => {} // bound successfully
            Ok(Err(e)) => {
                self.cancel.cancel();
                return Err(e);
            }
            Err(_) => {
                // Sender dropped without sending — bind task panicked
                self.cancel.cancel();
                anyhow::bail!("gateway HTTP bind task exited unexpectedly");
            }
        }

        // Start platform adapters with automatic restart on unexpected exit
        for adapter in &self.adapters {
            delivery_router.register(adapter.clone());
            let adapter = adapter.clone();
            let tx = tx.clone();
            let cancel = self.cancel.clone();
            tokio::spawn(async move {
                let mut retry_delay = std::time::Duration::from_secs(5);
                loop {
                    tokio::select! {
                        result = adapter.start(tx.clone()) => {
                            match result {
                                Ok(()) => {
                                    // Clean exit (e.g. receiver dropped) — do not restart
                                    tracing::info!(platform = ?adapter.platform(), "adapter exited cleanly");
                                    return;
                                }
                                Err(e) => {
                                    tracing::error!(
                                        platform = ?adapter.platform(),
                                        error = %e,
                                        retry_secs = retry_delay.as_secs(),
                                        "platform adapter exited with error — restarting"
                                    );
                                }
                            }
                        }
                        _ = cancel.cancelled() => {
                            tracing::info!(platform = ?adapter.platform(), "adapter shutdown");
                            return;
                        }
                    }
                    // Back off before restart: 5s → 10s → 20s → … → 120s
                    tokio::select! {
                        _ = tokio::time::sleep(retry_delay) => {}
                        _ = cancel.cancelled() => return,
                    }
                    retry_delay = std::cmp::min(
                        std::time::Duration::from_secs(retry_delay.as_secs().saturating_mul(2)),
                        std::time::Duration::from_secs(120),
                    );
                }
            });
        }

        // Seal the delivery router — all adapters are registered; wrap in Arc so
        // the spawned dispatch tasks can share it without cloning the entire map.
        let delivery_router = Arc::new(delivery_router);

        // Start session cleanup task
        let sm = self.session_manager.clone();
        let interval = self.config.cleanup_interval();
        let cancel_cleanup = self.cancel.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        sm.cleanup_expired().await;
                    }
                    _ = cancel_cleanup.cancelled() => break,
                }
            }
        });

        // Message dispatch loop
        loop {
            tokio::select! {
                Some(msg) = rx.recv() => {
                    tracing::debug!(
                        platform = ?msg.platform,
                        user = %msg.user_id,
                        "received incoming message"
                    );

                    // ── Authorization guard ───────────────────────────────────
                    // Reject messages from unauthorized users before any command
                    // handling or agent dispatch.  Configuration is via env vars;
                    // see `is_user_authorized()` for the full rule set.
                    if !self.is_user_authorized(&msg) {
                        tracing::warn!(
                            platform = ?msg.platform,
                            user = %msg.user_id,
                            "unauthorized message rejected"
                        );
                        if let Some(adapter) = self
                            .adapters
                            .iter()
                            .find(|a| a.platform() == msg.platform)
                            .cloned()
                        {
                            let _ = adapter
                                .send(crate::platform::OutgoingMessage {
                                    text: "⛔ Unauthorized. Contact the bot administrator."
                                        .into(),
                                    metadata: msg.metadata.clone(),
                                })
                                .await;
                        }
                        continue;
                    }

                    // ── Command pre-dispatch ──────────────────────────────────
                    // Slash commands (/help, /new, /stop, /status) are intercepted
                    // here BEFORE the agent is invoked.  They are handled inline
                    // on the event loop (they are fast/non-blocking) and the result
                    // is delivered back via the delivery router.
                    if msg.is_command() {
                        let cmd = msg.get_command().unwrap_or("").to_ascii_lowercase();
                        let origin_chat_id = message_origin_recipient(&msg);
                        let session_key = format!("{}:{}", msg.platform, origin_chat_id);
                        let origin_adapter = self
                            .adapters
                            .iter()
                            .find(|a| a.platform() == msg.platform)
                            .cloned();

                        // Emit command:* hook before processing
                        let args_text = msg.text
                            .split_once(' ')
                            .map(|x| x.1)
                            .unwrap_or("")
                            .to_string();
                        self.hook_registry.emit(
                            &format!("command:{cmd}"),
                            &HookContext::new(format!("command:{cmd}"))
                                .with_user(&msg.user_id)
                                .with_platform(format!("{:?}", msg.platform).to_lowercase())
                                .with_str("command", &cmd)
                                .with_str("args", &args_text),
                        ).await;

                        let reply_text: Option<String> = match cmd.as_str() {
                            "help" => {
                                Some(HELP_TEXT.to_string())
                            }
                            "approve" => {
                                match self
                                    .interaction_broker
                                    .peek(&session_key)
                                    .await
                                {
                                    Some(view)
                                        if matches!(view.kind, PendingInteractionKind::Approval { .. }) =>
                                    {
                                        let choice = parse_approval_reply(msg.text.trim())
                                            .unwrap_or(edgecrab_core::ApprovalChoice::Once);
                                        let count = self
                                            .interaction_broker
                                            .resolve_oldest_approval(&session_key, choice)
                                            .await;
                                        if count > 0 {
                                            Some("✅ Approval recorded. Continuing…".into())
                                        } else {
                                            Some("No pending approval found.".into())
                                        }
                                    }
                                    Some(view) => Some(pending_interaction_hint(&view.kind)),
                                    None => Some("No pending approval found.".into()),
                                }
                            }
                            "deny" => {
                                match self
                                    .interaction_broker
                                    .peek(&session_key)
                                    .await
                                {
                                    Some(view)
                                        if matches!(view.kind, PendingInteractionKind::Approval { .. }) =>
                                    {
                                        let count = self
                                            .interaction_broker
                                            .resolve_oldest_approval(
                                                &session_key,
                                                edgecrab_core::ApprovalChoice::Deny,
                                            )
                                            .await;
                                        if count > 0 {
                                            Some("🛑 Pending approval denied.".into())
                                        } else {
                                            Some("No pending approval found.".into())
                                        }
                                    }
                                    Some(view)
                                        if matches!(view.kind, PendingInteractionKind::Clarify { .. }) =>
                                    {
                                        if self
                                            .interaction_broker
                                            .resolve_oldest_clarify(&session_key, String::new())
                                            .await
                                        {
                                            Some("🛑 Clarification cancelled.".into())
                                        } else {
                                            Some("No pending clarification found.".into())
                                        }
                                    }
                                    None => Some("No pending interaction found.".into()),
                                    Some(view) => Some(pending_interaction_hint(&view.kind)),
                                }
                            }
                            "stop" => {
                                let cancelled = {
                                    let mut guard = self.running_sessions.lock().await;
                                    if let Some(token) = guard.remove(&session_key) {
                                        token.cancel();
                                        true
                                    } else {
                                        false
                                    }
                                };
                                // Also discard any queued message for this session.
                                {
                                    let mut pending = self.pending_messages.lock().await;
                                    pending.remove(&session_key);
                                }
                                let _ = self.interaction_broker.cancel_session(&session_key).await;
                                if cancelled {
                                    Some("⚡ Stopped. Send a new message to continue.".into())
                                } else {
                                    Some("No active request to stop.".into())
                                }
                            }
                            "new" | "reset" => {
                                // Cancel any running agent for this session
                                {
                                    let mut guard = self.running_sessions.lock().await;
                                    if let Some(token) = guard.remove(&session_key) {
                                        token.cancel();
                                    }
                                }
                                // Clear queued + retry state for this session.
                                {
                                    let mut pending = self.pending_messages.lock().await;
                                    pending.remove(&session_key);
                                }
                                {
                                    let mut last = self.last_messages.lock().await;
                                    last.remove(&session_key);
                                }
                                let _ = self.interaction_broker.cancel_session(&session_key).await;
                                // Remove the LLM conversation history so the agent starts fresh.
                                let sk = SessionKey::new(
                                    msg.platform,
                                    &msg.user_id,
                                    msg.channel_id.as_deref()
                                        .or(msg.metadata.channel_id.as_deref()),
                                );
                                self.session_manager.remove(&sk);
                                // Emit session:reset hook
                                self.hook_registry.emit(
                                    "session:reset",
                                    &HookContext::new("session:reset")
                                        .with_user(&msg.user_id)
                                        .with_platform(format!("{:?}", msg.platform).to_lowercase())
                                        .with_str("session_key", &session_key),
                                ).await;
                                Some("✓ Session reset. Start a new conversation!".into())
                            }
                            "status" => {
                                let (is_running, has_pending) = {
                                    let running = self.running_sessions.lock().await;
                                    let pending = self.pending_messages.lock().await;
                                    (
                                        running.contains_key(&session_key),
                                        pending.contains_key(&session_key),
                                    )
                                };
                                let pending_interactions =
                                    self.interaction_broker.pending_count(&session_key).await;
                                let session_count = self.session_manager.session_count();
                                match (is_running, has_pending) {
                                    (true, true) => Some(format!(
                                        "🟡 Agent is running with 1 message queued. Pending interactions: {}. {} active session(s) total.",
                                        pending_interactions, session_count
                                    )),
                                    (true, false) => Some(format!(
                                        "🟡 Agent is running. Pending interactions: {}. {} active session(s) total.",
                                        pending_interactions, session_count
                                    )),
                                    _ => Some(format!(
                                        "✅ Ready. Pending interactions: {}. {} active session(s) total.",
                                        pending_interactions, session_count
                                    )),
                                }
                            }
                            "retry" => {
                                let last_text = {
                                    let last = self.last_messages.lock().await;
                                    last.get(&session_key).cloned()
                                };
                                match last_text {
                                    Some(text) => {
                                        // Re-inject the last user message into the dispatch loop.
                                        // The session guard will queue it if an agent is already
                                        // running, or dispatch it directly if idle.
                                        let retry_msg = IncomingMessage {
                                            platform: msg.platform,
                                            user_id: msg.user_id.clone(),
                                            channel_id: msg.channel_id.clone(),
                                            text,
                                            thread_id: msg.thread_id.clone(),
                                            metadata: msg.metadata.clone(),
                                        };
                                        let _ = tx.send(retry_msg).await;
                                        Some("🔄 Retrying last message...".into())
                                    }
                                    None => Some("No previous message to retry.".into()),
                                }
                            }
                            "usage" => {
                                let (is_running, has_pending, has_last) = {
                                    let running = self.running_sessions.lock().await;
                                    let pending = self.pending_messages.lock().await;
                                    let last = self.last_messages.lock().await;
                                    (
                                        running.contains_key(&session_key),
                                        pending.contains_key(&session_key),
                                        last.contains_key(&session_key),
                                    )
                                };
                                let pending_interactions =
                                    self.interaction_broker.pending_count(&session_key).await;
                                let total_sessions = self.session_manager.session_count();
                                let status = if is_running { "running" } else { "idle" };
                                let queued = if has_pending { "yes" } else { "no" };
                                let retryable = if has_last { "yes" } else { "no" };
                                Some(format!(
                                    "📊 *Session stats:*\n\
                                     • Status: {status}\n\
                                     • Message queued: {queued}\n\
                                     • Pending interactions: {pending_interactions}\n\
                                     • /retry available: {retryable}\n\
                                     • Total active sessions: {total_sessions}"
                                ))
                            }
                            "background" | "bg" => {
                                let prompt = msg.get_command_args().trim().to_string();
                                if prompt.is_empty() {
                                    Some(
                                        "Usage: /background <prompt>\nRuns the prompt in a separate session. You can keep chatting — the result will appear here when done.".into(),
                                    )
                                } else if let Some(agent) = self.agent.as_ref().cloned() {
                                    let delivery = delivery_router.clone();
                                    let adapter = origin_adapter.clone();
                                    let metadata = msg.metadata.clone();
                                    let platform = msg.platform;
                                    let platform_name = platform.to_string();
                                    let origin_chat_id_clone = origin_chat_id.clone();
                                    let preview = edgecrab_core::safe_truncate(&prompt, 60).to_string();
                                    let effective_text = build_effective_text(
                                        &agent,
                                        &IncomingMessage {
                                            text: prompt.clone(),
                                            ..msg.clone()
                                        },
                                    )
                                    .await;
                                    let task_id = format!(
                                        "bg_{}_{}",
                                        chrono::Local::now().format("%H%M%S"),
                                        uuid::Uuid::new_v4().simple()
                                    );
                                    let task_id_for_spawn = task_id.clone();
                                    let preview_for_spawn = preview.clone();

                                    tokio::spawn(async move {
                                        let background_agent = match agent
                                            .fork_isolated(IsolatedAgentOptions {
                                                session_id: Some(task_id_for_spawn.clone()),
                                                platform: Some(platform),
                                                quiet_mode: Some(true),
                                                origin_chat: Some((
                                                    platform_name.clone(),
                                                    origin_chat_id_clone.clone(),
                                                )),
                                            })
                                            .await
                                        {
                                            Ok(child) => child,
                                            Err(e) => {
                                                let _ = delivery
                                                    .deliver(
                                                        &format!(
                                                            "❌ Background task {task_id_for_spawn} failed: {e}"
                                                        ),
                                                        platform,
                                                        &metadata,
                                                    )
                                                    .await;
                                                return;
                                            }
                                        };

                                        let response = match background_agent
                                            .chat_with_origin(
                                                &effective_text,
                                                &platform_name,
                                                &origin_chat_id_clone,
                                            )
                                            .await
                                        {
                                            Ok(text) => {
                                                let body = if text.trim().is_empty() {
                                                    "(No response generated)".to_string()
                                                } else {
                                                    text
                                                };
                                                format!(
                                                    "✅ Background task complete\nPrompt: \"{preview_for_spawn}\"\n\n{body}"
                                                )
                                            }
                                            Err(e) => {
                                                format!(
                                                    "❌ Background task {task_id_for_spawn} failed: {e}"
                                                )
                                            }
                                        };

                                        let _ = deliver_text_and_media(
                                            &delivery,
                                            adapter,
                                            &response,
                                            platform,
                                            &metadata,
                                        )
                                        .await;
                                    });

                                    Some(format!(
                                        "🔄 Background task started: \"{preview}\"\nTask ID: {task_id}\nYou can keep chatting — results will appear when done."
                                    ))
                                } else {
                                    Some("No agent configured.".into())
                                }
                            }
                            "hooks" => {
                                // List all currently loaded file-based hooks.
                                let hooks = self.hook_registry.loaded_hooks();
                                if hooks.is_empty() {
                                    Some(
                                        "🪝 *No hooks loaded.*\n\
                                         Place hook directories under `~/.edgecrab/hooks/`.\n\
                                         Each directory needs `HOOK.yaml` + `handler.py` / `handler.ts`."
                                            .into(),
                                    )
                                } else {
                                    let mut lines = format!(
                                        "🪝 *Loaded hooks ({} total):*\n",
                                        hooks.len()
                                    );
                                    for h in hooks {
                                        let events = h.events.join(", ");
                                        lines.push_str(&format!(
                                            "\n• *{}* `[{lang}]` p={priority}\n  Events: `{events}`\n  {desc}",
                                            h.name,
                                            lang = h.language,
                                            priority = h.priority,
                                            desc = if h.description.is_empty() {
                                                String::new()
                                            } else {
                                                format!("_{}_", h.description)
                                            },
                                        ));
                                    }
                                    Some(lines)
                                }
                            }
                            _ => {
                                // Unknown command — fall through to agent dispatch
                                None
                            }
                        };

                        if let Some(text) = reply_text {
                            // Deliver the command reply directly via delivery router
                            // or the origin adapter's send() if available.
                            if let Some(adapter) = origin_adapter {
                                let out = crate::platform::OutgoingMessage {
                                    text,
                                    metadata: msg.metadata.clone(),
                                };
                                let _ = adapter.send(out).await;
                            } else {
                                let _ = delivery_router
                                    .deliver(&text, msg.platform, &msg.metadata)
                                    .await;
                            }
                            continue; // do not dispatch to agent
                        }
                        // Unknown command — fall through to normal agent dispatch below
                    }

                    let origin_chat_id_for_key = message_origin_recipient(&msg);
                    let session_key = format!("{}:{}", msg.platform, origin_chat_id_for_key);
                    if let Some(view) = self.interaction_broker.peek(&session_key).await {
                        let resolution_reply = match &view.kind {
                            PendingInteractionKind::Clarify { choices, .. } => {
                                let answer = choices
                                    .as_ref()
                                    .map(|list| parse_clarify_answer(&msg.text, list))
                                    .unwrap_or_else(|| msg.text.trim().to_string());
                                if answer.is_empty() {
                                    Some(pending_interaction_hint(&view.kind))
                                } else if self
                                    .interaction_broker
                                    .resolve_oldest_clarify(&session_key, answer)
                                    .await
                                {
                                    Some("✅ Answer received. Continuing…".into())
                                } else {
                                    Some("No pending clarification found.".into())
                                }
                            }
                            PendingInteractionKind::Approval { .. } => {
                                if let Some(choice) = parse_approval_reply(&msg.text) {
                                    let count = self
                                        .interaction_broker
                                        .resolve_oldest_approval(&session_key, choice)
                                        .await;
                                    if count > 0 {
                                        Some("✅ Approval recorded. Continuing…".into())
                                    } else {
                                        Some("No pending approval found.".into())
                                    }
                                } else {
                                    Some(pending_interaction_hint(&view.kind))
                                }
                            }
                        };

                        if let Some(text) = resolution_reply {
                            let origin_adapter = self
                                .adapters
                                .iter()
                                .find(|a| a.platform() == msg.platform)
                                .cloned();
                            if let Some(adapter) = origin_adapter {
                                let _ = adapter
                                    .send(crate::platform::OutgoingMessage {
                                        text,
                                        metadata: msg.metadata.clone(),
                                    })
                                    .await;
                            } else {
                                let _ = delivery_router
                                    .deliver(&text, msg.platform, &msg.metadata)
                                    .await;
                            }
                            continue;
                        }
                    }

                    // Emit hook
                    self.hook_registry.emit(
                        "agent:start",
                        &HookContext::new("agent:start")
                            .with_user(&msg.user_id)
                            .with_platform(format!("{:?}", msg.platform).to_lowercase())
                            .with_str("message", &msg.text),
                    ).await;

                    // Dispatch to Agent
                    if let Some(ref agent) = self.agent {
                        let agent = agent.clone();
                        let hooks = self.hook_registry.clone();
                        let delivery = delivery_router.clone();
                        let msg_clone = msg.clone();
                        let streaming_cfg = self.config.streaming.clone();
                        // Snapshot the adapter for the originating platform so the
                        // event processor can send status messages and the stream
                        // consumer can deliver progressive updates.
                        let origin_adapter: Option<Arc<dyn PlatformAdapter>> = self
                            .adapters
                            .iter()
                            .find(|a| a.platform() == msg.platform)
                            .cloned();

                        // ── Session guard (queue-based) ───────────────────────
                        // Compute the session key for this chat.
                        // If an agent task is ALREADY running for this session,
                        // queue the new message instead of cancelling the running
                        // one (which would truncate the in-progress response).
                        // Only the most-recent queued message is kept; older ones
                        // are silently replaced.
                        let origin_chat_id_for_key = message_origin_recipient(&msg);
                        let session_key = format!("{}:{}", msg.platform, origin_chat_id_for_key);

                        {
                            let running = self.running_sessions.lock().await;
                            if running.contains_key(&session_key) {
                                // Queue the message — don't cancel the running task.
                                let mut pending = self.pending_messages.lock().await;
                                pending.insert(session_key.clone(), msg.clone());
                                drop(pending);
                                drop(running);
                                // Notify the user so they know the message was received.
                                if let Some(ref adapter) = origin_adapter {
                                    let _ = adapter
                                        .send(crate::platform::OutgoingMessage {
                                            text: "⏳ Message queued. I'll respond after the current request finishes.".into(),
                                            metadata: msg.metadata.clone(),
                                        })
                                        .await;
                                }
                                continue; // Don't spawn a second task
                            }
                        }

                        // No running task — register this one and dispatch.
                        let task_cancel = CancellationToken::new();
                        {
                            let mut guard = self.running_sessions.lock().await;
                            guard.insert(session_key.clone(), task_cancel.clone());
                        }

                        // Persist message text so /retry can replay it.
                        {
                            let mut last = self.last_messages.lock().await;
                            last.insert(session_key.clone(), msg.text.clone());
                        }

                        let running_sessions = self.running_sessions.clone();
                        let pending_messages = self.pending_messages.clone();
                        let task_session_key = session_key.clone();
                        // Clone the sender so the task can re-dispatch the pending message.
                        let msg_tx = tx.clone();
                        let hook_registry_for_spawn = self.hook_registry.clone();
                        let interaction_broker = self.interaction_broker.clone();
                        // The token is registered in running_sessions; drop the local copy.
                        // /stop cancels the map-held token via running_sessions.remove().cancel().
                        drop(task_cancel);

                        tokio::spawn(async move {
                            // Resolve the origin chat_id: prefer channel_id (group/channel),
                            // fall back to user_id (DM).  This is what deliver='origin' uses
                            // to route cron job output back to the correct chat.
                            let origin_chat_id = message_origin_recipient(&msg_clone);
                            let platform_name = msg_clone.platform.to_string();

                            // Enrich the prompt with image attachment instructions.
                            // WHY here: This is the single gateway dispatch point covering
                            // ALL platforms (WhatsApp, Telegram, Slack, Signal, …). Injecting
                            // the *** ATTACHED IMAGES *** block here means every platform
                            // triggers the VISION_GUIDANCE rules in the system prompt,
                            // identical to the CLI pending_images path in app.rs.
                            let effective_text = build_effective_text(&agent, &msg_clone).await;

                            // NOTE: chat_streaming_with_origin() handles origin context
                            // internally, so we do NOT pre-set it here.

                            let response_result = match origin_adapter {
                                // ── Interactive gateway path ─────────────────────────────
                                // Use the event bridge whenever the originating platform
                                // has an adapter, even if progressive token delivery is
                                // disabled. This keeps approvals and clarify on one path.
                                Some(adapter_arc) => {
                                    dispatch_streaming_arc(
                                        &agent,
                                        &effective_text,
                                        &platform_name,
                                        &origin_chat_id,
                                        adapter_arc,
                                        msg_clone.metadata.clone(),
                                        streaming_cfg,
                                        hook_registry_for_spawn,
                                        interaction_broker.clone(),
                                        task_session_key.clone(),
                                    )
                                    .await
                                }
                                // ── Non-streaming fallback ────────────────────────────────
                                // No adapter is available, so we cannot surface pending
                                // interactions through the gateway event bridge.
                                None => {
                                    match agent
                                        .chat_with_origin(
                                            &effective_text,
                                            &platform_name,
                                            &origin_chat_id,
                                        )
                                        .await
                                    {
                                        Ok(response) => deliver_text_and_media(
                                            &delivery,
                                            None,
                                            &response,
                                            msg_clone.platform,
                                            &msg_clone.metadata,
                                        )
                                        .await,
                                        Err(e) => Err(anyhow::anyhow!("{}", e)),
                                    }
                                }
                            };

                            match response_result {
                                Ok(response_len) => {
                                    tracing::info!(
                                        platform = ?msg_clone.platform,
                                        user = %msg_clone.user_id,
                                        response_len,
                                        "agent response delivered"
                                    );
                                    hooks.emit(
                                        "agent:done",
                                        &HookContext::new("agent:done").with_user(&msg_clone.user_id),
                                    ).await;
                                }
                                Err(e) => {
                                    tracing::error!(
                                        error = %e,
                                        platform = ?msg_clone.platform,
                                        "agent dispatch failed"
                                    );
                                }
                            }

                            // Release the session guard.
                            // Unconditional remove is safe: we own this slot exclusively
                            // (queue-based guard prevents a second task from registering
                            // for the same session while we are running).
                            {
                                let mut guard = running_sessions.lock().await;
                                guard.remove(&task_session_key);
                            }
                            let _ = interaction_broker.cancel_session(&task_session_key).await;

                            // Re-dispatch any message that arrived while we were running.
                            let pending = {
                                let mut p = pending_messages.lock().await;
                                p.remove(&task_session_key)
                            };
                            if let Some(queued_msg) = pending {
                                tracing::debug!(
                                    session = %task_session_key,
                                    "re-dispatching queued message after task completion"
                                );
                                let _ = msg_tx.send(queued_msg).await;
                            }
                        });
                    } else {
                        tracing::warn!(text = %msg.text, "no agent configured, message dropped");
                    }
                }
                _ = self.cancel.cancelled() => {
                    tracing::info!("gateway shutting down");
                    break;
                }
            }
        }

        let _ = edgecrab_tools::tools::terminal::cleanup_all_backends().await;
        Ok(())
    }
}

/// Arc-based streaming dispatch — the production entry point called from
/// the gateway dispatch loop where adapters are already `Arc`.
///
/// WHY 8 arguments: each represents a distinct runtime concern
/// (agent, message text, routing identity, platform adapter, metadata,
/// streaming config, hook callbacks). Grouping them would
/// introduce an ad-hoc struct that is only used at this call site, hiding
/// the true dependencies rather than clarifying them.
#[allow(clippy::too_many_arguments)]
async fn dispatch_streaming_arc(
    agent: &Agent,
    message: &str,
    platform_name: &str,
    origin_chat_id: &str,
    adapter: Arc<dyn PlatformAdapter>,
    metadata: crate::platform::MessageMetadata,
    cfg: crate::config::GatewayStreamingConfig,
    hook_registry: Arc<HookRegistry>,
    interaction_broker: Arc<InteractionBroker>,
    session_key: String,
) -> anyhow::Result<usize> {
    let (processor, event_tx, consumer) = GatewayEventProcessor::new(
        adapter,
        metadata.clone(),
        cfg,
        hook_registry,
        interaction_broker,
        session_key,
    );

    let already_sent = consumer.already_sent_flag();
    let consumer_task = tokio::spawn(consumer.run());
    let processor_task = tokio::spawn(processor.run());

    let agent_result = agent
        .chat_streaming_with_origin(message, platform_name, origin_chat_id, event_tx)
        .await;

    let _ = processor_task.await;
    let _ = consumer_task.await;

    agent_result.map_err(|e| anyhow::anyhow!("{}", e))?;

    if already_sent.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok(0);
    }
    Ok(0)
}

/// Build the axum router with health and webhook endpoints.
fn build_router(state: GatewayState) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/webhook/incoming", post(webhook_incoming))
        .with_state(state)
}

/// Health check endpoint — returns 200 with session count.
async fn health_handler(State(state): State<GatewayState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "sessions": state.session_manager.session_count(),
    }))
}

/// Webhook inbound endpoint — accepts POST with WebhookPayload.
async fn webhook_incoming(
    State(state): State<GatewayState>,
    Json(payload): Json<WebhookPayload>,
) -> Json<serde_json::Value> {
    use crate::webhook::WebhookAdapter;

    let msg = WebhookAdapter::parse_incoming(&payload);
    match state.message_tx.send(msg).await {
        Ok(()) => Json(serde_json::json!({"status": "queued"})),
        Err(_) => Json(serde_json::json!({"status": "error", "message": "gateway channel full"})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_construction() {
        let config = GatewayConfig::default();
        let cancel = CancellationToken::new();
        let gw = Gateway::new(config, cancel);
        assert_eq!(gw.adapters.len(), 0);
        assert_eq!(gw.session_manager.session_count(), 0);
    }

    #[test]
    fn image_attachment_sources_rejects_opaque_transport_urls() {
        let msg = IncomingMessage {
            platform: edgecrab_types::Platform::Signal,
            user_id: "u1".into(),
            channel_id: None,
            text: "describe".into(),
            thread_id: None,
            metadata: crate::platform::MessageMetadata {
                attachments: vec![
                    crate::platform::MessageAttachment {
                        kind: crate::platform::MessageAttachmentKind::Image,
                        url: Some("signal://attachment/abc123".into()),
                        ..Default::default()
                    },
                    crate::platform::MessageAttachment {
                        kind: crate::platform::MessageAttachmentKind::Image,
                        url: Some("https://example.com/cat.png".into()),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        };

        let sources = image_attachment_sources(&msg);
        assert_eq!(sources, vec!["https://example.com/cat.png"]);
    }

    #[test]
    fn render_gateway_image_context_includes_preanalysis_without_forcing_tool_call() {
        let rendered = render_gateway_image_context(
            "Describe this",
            &[String::from("/tmp/cat.jpg")],
            &[String::from(
                "Image analysis (local file: /tmp/cat.jpg):\n\nA cat on a sofa.",
            )],
            &[],
            0,
        );

        assert!(rendered.contains("ATTACHED IMAGES"));
        assert!(rendered.contains("/tmp/cat.jpg"));
        assert!(rendered.contains("A cat on a sofa."));
        assert!(
            !rendered.contains("You MUST call vision_analyze"),
            "gateway context should not hard-require a tool that may be unavailable"
        );
    }

    #[test]
    fn render_gateway_image_context_handles_empty_user_text() {
        let rendered = render_gateway_image_context(
            "",
            &[String::from("/tmp/image.png")],
            &[],
            &[String::from("Image 1 (/tmp/image.png): unavailable")],
            0,
        );

        assert!(rendered.starts_with("*** ATTACHED IMAGES ***"));
        assert!(rendered.contains("unavailable"));
    }

    #[tokio::test]
    async fn health_endpoint() {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt; // for oneshot

        let (tx, _rx) = mpsc::channel(16);
        let state = GatewayState {
            session_manager: Arc::new(SessionManager::new(std::time::Duration::from_secs(3600))),
            hook_registry: Arc::new(HookRegistry::new()),
            message_tx: tx,
            cancel: CancellationToken::new(),
        };
        let app = build_router(state);

        let request = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .expect("request");

        let response = app.oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn webhook_endpoint() {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let (tx, mut rx) = mpsc::channel(16);
        let state = GatewayState {
            session_manager: Arc::new(SessionManager::new(std::time::Duration::from_secs(3600))),
            hook_registry: Arc::new(HookRegistry::new()),
            message_tx: tx,
            cancel: CancellationToken::new(),
        };
        let app = build_router(state);

        let payload = r#"{"text":"hello from webhook","user_id":"u1"}"#;
        let request = Request::builder()
            .method("POST")
            .uri("/webhook/incoming")
            .header("content-type", "application/json")
            .body(Body::from(payload))
            .expect("request");

        let response = app.oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        // Message should be in the channel
        let msg = rx.try_recv().expect("should receive message");
        assert_eq!(msg.text, "hello from webhook");
        assert_eq!(msg.user_id, "u1");
    }

    // ── Authorization tests ───────────────────────────────────────────────

    fn make_msg(user_id: &str, platform: edgecrab_types::Platform) -> IncomingMessage {
        IncomingMessage {
            platform,
            user_id: user_id.to_string(),
            channel_id: None,
            text: "hello".to_string(),
            thread_id: None,
            metadata: crate::platform::MessageMetadata::default(),
        }
    }

    fn make_gateway() -> Gateway {
        Gateway::new(GatewayConfig::default(), CancellationToken::new())
    }

    /// Serializes env-var tests so parallel test execution doesn't cause races.
    /// Env vars are global process state; reading and writing them concurrently
    /// is both unsafe (in the Rust sense) and logically incorrect for tests.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Clear all gateway auth env vars to put the environment in a known state.
    ///
    /// # Safety
    /// Must be called while holding `ENV_LOCK`.
    unsafe fn clear_auth_env() {
        unsafe {
            std::env::remove_var("GATEWAY_ALLOW_ALL_USERS");
            std::env::remove_var("GATEWAY_ALLOWED_USERS");
            std::env::remove_var("TELEGRAM_ALLOWED_USERS");
            std::env::remove_var("DISCORD_ALLOWED_USERS");
            std::env::remove_var("TELEGRAM_ALLOW_ALL_USERS");
            std::env::remove_var("DISCORD_ALLOW_ALL_USERS");
        }
    }

    #[test]
    fn auth_open_gateway_when_no_env_vars() {
        let _lock = ENV_LOCK.lock().unwrap();
        // SAFETY: single-threaded via ENV_LOCK; no other test holds the lock.
        unsafe {
            clear_auth_env();
        }

        let gw = make_gateway();
        let msg = make_msg("alice", edgecrab_types::Platform::Telegram);
        assert!(
            gw.is_user_authorized(&msg),
            "open gateway should allow everyone"
        );
    }

    #[test]
    fn auth_global_allow_all() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            clear_auth_env();
            std::env::set_var("GATEWAY_ALLOW_ALL_USERS", "true");
        }
        let gw = make_gateway();
        let msg = make_msg("anyone", edgecrab_types::Platform::Telegram);
        assert!(gw.is_user_authorized(&msg));
        unsafe {
            clear_auth_env();
        }
    }

    #[test]
    fn auth_global_allowlist_permits_listed_user() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            clear_auth_env();
            std::env::set_var("GATEWAY_ALLOWED_USERS", "alice,bob");
        }
        let gw = make_gateway();
        let allow = make_msg("alice", edgecrab_types::Platform::Telegram);
        let deny = make_msg("charlie", edgecrab_types::Platform::Telegram);
        assert!(gw.is_user_authorized(&allow));
        assert!(!gw.is_user_authorized(&deny));
        unsafe {
            clear_auth_env();
        }
    }

    #[test]
    fn auth_platform_allowlist_telegram() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            clear_auth_env();
            std::env::set_var("TELEGRAM_ALLOWED_USERS", "12345");
        }
        let gw = make_gateway();
        let allow = make_msg("12345", edgecrab_types::Platform::Telegram);
        let deny = make_msg("99999", edgecrab_types::Platform::Telegram);
        assert!(gw.is_user_authorized(&allow));
        assert!(!gw.is_user_authorized(&deny));
        unsafe {
            clear_auth_env();
        }
    }

    #[test]
    fn auth_platform_allow_all_discord() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            clear_auth_env();
            std::env::set_var("DISCORD_ALLOW_ALL_USERS", "1");
            // Telegram still needs explicit listing when a TELEGRAM list exists
            std::env::set_var("TELEGRAM_ALLOWED_USERS", "only-me");
        }
        let gw = make_gateway();
        let discord_msg = make_msg("anyone", edgecrab_types::Platform::Discord);
        let telegram_other = make_msg("stranger", edgecrab_types::Platform::Telegram);
        assert!(gw.is_user_authorized(&discord_msg));
        assert!(!gw.is_user_authorized(&telegram_other));
        unsafe {
            clear_auth_env();
        }
    }

    // ── HELP_TEXT tests ────────────────────────────────────────────────────

    #[test]
    fn help_text_contains_all_commands() {
        for cmd in &[
            "/help",
            "/new",
            "/reset",
            "/stop",
            "/retry",
            "/status",
            "/usage",
            "/background",
            "/approve",
            "/deny",
        ] {
            assert!(HELP_TEXT.contains(cmd), "HELP_TEXT missing {cmd}");
        }
    }

    #[test]
    fn parse_approval_reply_supports_risk_levels() {
        assert_eq!(
            parse_approval_reply("/approve"),
            Some(edgecrab_core::ApprovalChoice::Once)
        );
        assert_eq!(
            parse_approval_reply("approve session"),
            Some(edgecrab_core::ApprovalChoice::Session)
        );
        assert_eq!(
            parse_approval_reply("/approve always"),
            Some(edgecrab_core::ApprovalChoice::Always)
        );
        assert_eq!(
            parse_approval_reply("/deny"),
            Some(edgecrab_core::ApprovalChoice::Deny)
        );
        assert_eq!(parse_approval_reply("maybe"), None);
    }

    #[test]
    fn parse_clarify_answer_maps_numeric_choices() {
        let choices = vec!["red".to_string(), "blue".to_string()];
        assert_eq!(parse_clarify_answer("2", &choices), "blue");
        assert_eq!(parse_clarify_answer(" custom ", &choices), "custom");
    }
}
