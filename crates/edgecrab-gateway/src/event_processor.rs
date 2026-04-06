//! # Gateway event processor — bridges Agent StreamEvents to platform delivery
//!
//! ## Design rationale
//!
//! The agent's `chat_streaming()` produces a stream of typed events:
//!
//! ```text
//!   StreamEvent::Reasoning(text)          LLM thinking block
//!   StreamEvent::ToolExec { name, args }  A tool is starting
//!   StreamEvent::ToolDone { name, .. }    A tool finished
//!   StreamEvent::Token(text)              A response token
//!   StreamEvent::Done                     Full response complete
//!   StreamEvent::Error(msg)               Agent failed
//!   StreamEvent::Clarify { .. }           Agent needs user input
//!   StreamEvent::Approval { .. }          Agent needs command approval
//! ```
//!
//! The processor maps each event type to the right delivery action, per
//! platform and per configuration, without the caller needing to know anything
//! about platform capabilities:
//!
//! ```text
//!   GatewayEventProcessor::run()
//!     ├── Reasoning  → send_status("🧠 Thinking…")  [if show_reasoning=true]
//!     ├── ToolExec   → send_status("🔧 {name}…")    [if tool_progress=true]
//!     ├── ToolDone   → (logged, not sent by default)
//!     ├── Token      → forwarded to GatewayStreamConsumer via delta channel
//!     ├── Done       → GatewayStreamConsumer::finish()
//!     ├── Error      → send_status("⚠️ {msg}")
//!     ├── Clarify    → queue in broker + send reply instructions
//!     └── Approval   → queue in broker + send approval instructions
//! ```
//!
//! ## DRY / SOLID compliance
//!
//! - **Single Responsibility**: this file owns *only* the event→delivery mapping.
//!   Token buffering/editing lives in `stream_consumer.rs`.
//!   Platform HTTP calls live in each adapter.
//! - **Open/Closed**: adding a new `StreamEvent` variant only requires a new
//!   match arm here — no other files change.
//! - **Dependency Inversion**: depends on the `PlatformAdapter` trait, not any
//!   concrete adapter.

use std::collections::HashMap;
use std::sync::Arc;

use edgecrab_core::StreamEvent;
use tokio::sync::mpsc::{self, UnboundedReceiver};
use tokio_util::sync::CancellationToken;

use crate::config::GatewayStreamingConfig;
use crate::hooks::{HookContext, HookRegistry};
use crate::interactions::{InteractionBroker, PendingInteractionKind, PendingInteractionView};
use crate::platform::{MessageMetadata, PlatformAdapter};
use crate::stream_consumer::{GatewayStreamConsumer, StreamConsumerConfig, StreamItem};

fn format_context_pressure_status(estimated_tokens: usize, threshold_tokens: usize) -> String {
    let ratio = if threshold_tokens == 0 {
        0.0
    } else {
        (estimated_tokens as f32 / threshold_tokens as f32).clamp(0.0, 1.0)
    };
    let percent = (ratio * 100.0).round() as usize;
    let width = 12usize;
    let filled = ((ratio * width as f32).round() as usize).min(width);
    let bar = format!("{}{}", "▰".repeat(filled), "▱".repeat(width - filled));
    format!(
        "⚠️ Context {bar} {percent}% to compression ({estimated_tokens}/{threshold_tokens} tokens)."
    )
}

fn format_pending_interaction(view: &PendingInteractionView) -> String {
    match &view.kind {
        PendingInteractionKind::Approval {
            command,
            full_command,
            reasons,
        } => {
            let reason_text = if reasons.is_empty() {
                "Flagged by the command safety policy.".to_string()
            } else {
                reasons.join("; ")
            };
            format!(
                "⚠️ Approval required [#{}]\nCommand: `{}`\nReason: {}\n\nReply `/approve`, `/approve session`, `/approve always`, or `/deny`.\nYou can also reply with plain text like `approve session`.\n\nFull command:\n```sh\n{}\n```",
                view.id, command, reason_text, full_command
            )
        }
        PendingInteractionKind::Clarify { question, choices } => {
            let mut text = format!("❓ Clarification needed [#{}]\n{}", view.id, question);
            if let Some(choices) = choices {
                for (idx, choice) in choices.iter().enumerate() {
                    text.push_str(&format!("\n{}. {}", idx + 1, choice));
                }
            }
            text.push_str("\n\nReply with your answer. Use `/deny` to cancel.");
            text
        }
    }
}

// ─── Processor ────────────────────────────────────────────────────────────

/// Translates `StreamEvent`s from the agent into platform-appropriate messages.
///
/// One processor is created per incoming gateway message and driven by
/// `GatewayEventProcessor::run()` until the agent emits `Done` or `Error`.
pub struct GatewayEventProcessor {
    adapter: Arc<dyn PlatformAdapter>,
    metadata: MessageMetadata,
    cfg: GatewayStreamingConfig,
    /// Hook registry for forwarding `HookEvent` stream events.
    hook_registry: Arc<HookRegistry>,
    // Receiver for agent events
    event_rx: UnboundedReceiver<StreamEvent>,
    // Sender into the stream consumer's token channel
    delta_tx: mpsc::Sender<StreamItem>,
    // Whether the stream consumer has already delivered the response.
    // Returned to the caller so it can skip a duplicate final `deliver()`.
    already_sent: Arc<std::sync::atomic::AtomicBool>,
    interaction_broker: Arc<InteractionBroker>,
    session_key: String,
}

impl GatewayEventProcessor {
    /// Create a new processor together with:
    /// - the `UnboundedSender` to pass to `agent.chat_streaming()`
    /// - the `GatewayStreamConsumer` task to `tokio::spawn()`
    /// - `self` to `tokio::spawn()` / `await`
    ///
    /// Caller pattern:
    /// ```ignore
    /// let (processor, event_tx, consumer) =
    ///     GatewayEventProcessor::new(adapter, metadata, cfg);
    /// let already_sent = consumer.already_sent_flag();
    /// let consumer_task = tokio::spawn(consumer.run());
    /// let processor_task = tokio::spawn(processor.run());
    ///
    /// agent.chat_streaming(message, event_tx).await?;
    /// consumer_task.await?;
    /// processor_task.await?;
    /// ```
    pub fn new(
        adapter: Arc<dyn PlatformAdapter>,
        metadata: MessageMetadata,
        cfg: GatewayStreamingConfig,
        hook_registry: Arc<HookRegistry>,
        interaction_broker: Arc<InteractionBroker>,
        session_key: String,
    ) -> (
        Self,
        tokio::sync::mpsc::UnboundedSender<StreamEvent>,
        GatewayStreamConsumer,
    ) {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        let consumer_cfg = StreamConsumerConfig {
            edit_interval: cfg.edit_interval(),
            buffer_threshold: cfg.buffer_threshold,
            cursor: cfg.cursor.clone(),
            prefer_editing: cfg.enabled,
        };
        let consumer = GatewayStreamConsumer::new(adapter.clone(), metadata.clone(), consumer_cfg);
        let delta_tx = consumer.delta_sender();
        let already_sent = consumer.already_sent_flag();

        (
            Self {
                adapter,
                metadata,
                cfg,
                hook_registry,
                event_rx,
                delta_tx,
                already_sent,
                interaction_broker,
                session_key,
            },
            event_tx,
            consumer,
        )
    }

    /// Whether the stream consumer has already delivered the response.
    ///
    /// Only valid AFTER `run()` has returned.  The caller uses this to skip
    /// an extra `DeliveryRouter::deliver()` that would duplicate the output on
    /// edit-capable platforms.
    pub fn already_sent(&self) -> bool {
        self.already_sent.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Process events until `Done` or `Error`, driving the stream consumer.
    ///
    /// Must be `tokio::spawn`ed concurrently with the stream consumer task.
    ///
    /// ## Typing indicators
    ///
    /// A background keepalive task sends `send_typing()` to the platform every
    /// 4 seconds while the agent is generating. This is essential for platforms
    /// like Telegram where the "typing…" indicator expires after ~5 seconds.
    /// The keepalive is cancelled immediately when the first token arrives (the
    /// stream consumer's live edit takes over as the visual progress indicator).
    pub async fn run(mut self) {
        const SUBAGENT_BATCH_SIZE: usize = 5;
        // ── Typing indicator keepalive ────────────────────────────────────
        // Spawn a background task that refreshes the typing indicator every
        // 4s while the agent is thinking (before the first token).
        let typing_adapter = self.adapter.clone();
        let typing_metadata = self.metadata.clone();
        let typing_cancel = CancellationToken::new();
        let typing_cancel_child = typing_cancel.clone();

        let typing_task = tokio::spawn(async move {
            // Initial indicator — fire immediately so there's no dead gap.
            let _ = typing_adapter.send_typing(&typing_metadata).await;
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(4));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let _ = typing_adapter.send_typing(&typing_metadata).await;
                    }
                    _ = typing_cancel_child.cancelled() => break,
                }
            }
        });

        /// Cancel the typing keepalive — `CancellationToken::cancel()` is already idempotent.
        macro_rules! cancel_typing {
            () => {
                typing_cancel.cancel();
            };
        }

        let mut subagent_batches: HashMap<usize, Vec<String>> = HashMap::new();

        while let Some(event) = self.event_rx.recv().await {
            match event {
                StreamEvent::Reasoning(text) => {
                    if self.cfg.show_reasoning && !text.trim().is_empty() {
                        let summary =
                            format!("🧠 _{}_", text.chars().take(280).collect::<String>());
                        self.send_status(&summary).await;
                    }
                }

                StreamEvent::ToolExec { name, .. } => {
                    if self.cfg.tool_progress {
                        let status = format!("🔧 {}…", name);
                        self.send_status(&status).await;
                    }
                }

                StreamEvent::SubAgentReasoning {
                    task_index,
                    task_count,
                    text,
                } => {
                    if self.cfg.show_reasoning && !text.trim().is_empty() {
                        let status = format!(
                            "💭 [{}/{}] {}",
                            task_index + 1,
                            task_count,
                            text.chars().take(180).collect::<String>()
                        );
                        self.send_status(&status).await;
                    }
                }

                StreamEvent::ToolDone {
                    name,
                    result_preview,
                    duration_ms,
                    is_error,
                    ..
                } => {
                    if self.cfg.tool_progress
                        && !is_error
                        && result_preview
                            .as_deref()
                            .is_some_and(|preview| should_surface_tool_completion(&name, preview))
                    {
                        self.send_status(&format!(
                            "✅ {} {}",
                            name,
                            result_preview.as_deref().unwrap_or_default()
                        ))
                        .await;
                    }
                    if self.cfg.tool_progress && is_error {
                        self.send_status(&format!(
                            "❌ {} failed in {:.1}s{}",
                            name,
                            duration_ms as f64 / 1000.0,
                            result_preview
                                .as_deref()
                                .filter(|preview| !preview.trim().is_empty())
                                .map(|preview| format!(": {preview}"))
                                .unwrap_or_default()
                        ))
                        .await;
                    }
                    // Successful tool completions are logged but not surfaced
                    // by default — they would be too noisy.
                    tracing::debug!(
                        tool = %name,
                        duration_ms,
                        is_error,
                        "tool done"
                    );
                }

                StreamEvent::SubAgentStart {
                    task_index,
                    task_count,
                    goal,
                } => {
                    let goal = goal.chars().take(72).collect::<String>();
                    let status = format!(
                        "🔀 [{}/{}] Starting delegated task: {}",
                        task_index + 1,
                        task_count,
                        goal
                    );
                    self.send_status(&status).await;
                }

                StreamEvent::SubAgentToolExec {
                    task_index,
                    task_count,
                    name,
                    ..
                } => {
                    let batch = subagent_batches.entry(task_index).or_default();
                    batch.push(name);
                    if batch.len() >= SUBAGENT_BATCH_SIZE {
                        let summary = batch.join(", ");
                        batch.clear();
                        let status = format!("🔀 [{}/{}] {}", task_index + 1, task_count, summary);
                        self.send_status(&status).await;
                    }
                }

                StreamEvent::SubAgentFinish {
                    task_index,
                    task_count,
                    status,
                    duration_ms,
                    summary,
                    ..
                } => {
                    if let Some(batch) = subagent_batches.get_mut(&task_index) {
                        if !batch.is_empty() {
                            let buffered = batch.join(", ");
                            batch.clear();
                            let status_line =
                                format!("🔀 [{}/{}] {}", task_index + 1, task_count, buffered);
                            self.send_status(&status_line).await;
                        }
                    }
                    let summary = summary
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .chars()
                        .take(96)
                        .collect::<String>();
                    let status_text = if summary.trim().is_empty() {
                        format!(
                            "{} [{}/{}] {} in {:.1}s",
                            if status == "completed" { "✅" } else { "❌" },
                            task_index + 1,
                            task_count,
                            status,
                            duration_ms as f64 / 1000.0
                        )
                    } else {
                        format!(
                            "{} [{}/{}] {} in {:.1}s: {}",
                            if status == "completed" { "✅" } else { "❌" },
                            task_index + 1,
                            task_count,
                            status,
                            duration_ms as f64 / 1000.0,
                            summary
                        )
                    };
                    self.send_status(&status_text).await;
                }

                StreamEvent::Token(text) => {
                    // Cancel typing indicator: the stream consumer's live edit
                    // becomes the progress indicator once tokens start flowing.
                    cancel_typing!();
                    // Forward to the stream consumer's accumulator.
                    let _ = self.delta_tx.send(StreamItem::Delta(text)).await;
                }

                StreamEvent::Done => {
                    cancel_typing!();
                    subagent_batches.clear();
                    // Signal the consumer to flush and exit.
                    let _ = self.delta_tx.send(StreamItem::Done).await;
                    break;
                }

                StreamEvent::Error(msg) => {
                    cancel_typing!();
                    subagent_batches.clear();
                    tracing::error!(error = %msg, "agent streaming error");
                    // Send an error message to the user.
                    let err_text = format!("⚠️ An error occurred: {}", msg);
                    self.send_status(&err_text).await;
                    // Terminate the consumer — do not send a partial response.
                    let _ = self.delta_tx.send(StreamItem::Done).await;
                    break;
                }

                StreamEvent::Clarify {
                    question,
                    choices,
                    response_tx,
                } => {
                    let view = self
                        .interaction_broker
                        .enqueue_clarify(&self.session_key, question, choices, response_tx)
                        .await;
                    self.send_status(&format_pending_interaction(&view)).await;
                }

                StreamEvent::HookEvent {
                    event,
                    context_json,
                } => {
                    // Forward tool:pre/post, llm:pre/post, and any other hook
                    // events from the conversation loop to the file-based hooks.
                    // Fire-and-forget: errors are logged inside emit().
                    match serde_json::from_str::<HookContext>(&context_json) {
                        Ok(ctx) => {
                            self.hook_registry.emit(&event, &ctx).await;
                        }
                        Err(e) => {
                            tracing::debug!(
                                event = %event,
                                error = %e,
                                "HookEvent context_json parse failed"
                            );
                        }
                    }
                }

                StreamEvent::ContextPressure {
                    estimated_tokens,
                    threshold_tokens,
                } => {
                    tracing::warn!(
                        estimated_tokens,
                        threshold_tokens,
                        "context pressure: approaching compression threshold"
                    );
                    self.send_status(&format_context_pressure_status(
                        estimated_tokens,
                        threshold_tokens,
                    ))
                    .await;
                }

                StreamEvent::Approval {
                    command,
                    full_command,
                    reasons,
                    response_tx,
                } => {
                    let view = self
                        .interaction_broker
                        .enqueue_approval(
                            &self.session_key,
                            command,
                            full_command,
                            reasons,
                            response_tx,
                        )
                        .await;
                    self.send_status(&format_pending_interaction(&view)).await;
                }

                StreamEvent::SecretRequest {
                    var_name,
                    response_tx,
                    ..
                } => {
                    // Gateway context — no interactive masked-input overlay available.
                    // Try to read from the process environment; if not set, send empty
                    // string (which the agent treats as abort).
                    let value = std::env::var(&var_name).unwrap_or_default();
                    if value.is_empty() {
                        tracing::warn!(
                            var_name = %var_name,
                            "gateway: secret request for unset env var — aborting"
                        );
                    }
                    let _ = response_tx.send(value);
                }
            }
        }

        // Loop exited — either via `break` (Done/Error path, which already sent
        // StreamItem::Done) or because the channel was closed unexpectedly.
        // In the latter case we must still signal the consumer.
        // The cancel_typing! macro is idempotent, so calling it here is safe.
        cancel_typing!();
        let _ = self.delta_tx.send(StreamItem::Done).await;
        let _ = typing_task.await;
    }

    // ── Helpers ───────────────────────────────────────────────────────────

    async fn send_status(&self, text: &str) {
        if let Err(e) = self.adapter.send_status(text, &self.metadata).await {
            tracing::debug!(error = %e, "gateway event processor: send_status failed");
        }
    }
}

pub(crate) fn should_surface_tool_completion(name: &str, preview: &str) -> bool {
    let n = name.to_ascii_lowercase();
    let p = preview.to_ascii_lowercase();
    n.contains("write")
        || n.contains("patch")
        || n.contains("delete")
        || n.contains("move")
        || n.contains("rename")
        || n.contains("create")
        || p.contains("wrote ")
        || p.contains("patched ")
        || p.contains("deleted ")
        || p.contains("moved ")
}

// ─── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interactions::InteractionBroker;
    use crate::platform::{IncomingMessage, MessageMetadata, OutgoingMessage, PlatformAdapter};
    use edgecrab_types::Platform;
    use tokio::sync::mpsc;

    struct DumbAdapter {
        sent: tokio::sync::Mutex<Vec<String>>,
    }

    impl DumbAdapter {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                sent: tokio::sync::Mutex::new(Vec::new()),
            })
        }
        async fn drain(&self) -> Vec<String> {
            self.sent.lock().await.drain(..).collect()
        }
    }

    async fn wait_for_pending(
        broker: &Arc<InteractionBroker>,
        session_key: &str,
    ) -> PendingInteractionView {
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if let Some(view) = broker.peek(session_key).await {
                    return view;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("pending interaction timeout")
    }

    #[async_trait::async_trait]
    impl PlatformAdapter for DumbAdapter {
        fn platform(&self) -> Platform {
            Platform::Webhook
        }
        async fn start(&self, _tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
            Ok(())
        }
        async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()> {
            self.sent.lock().await.push(msg.text);
            Ok(())
        }
        fn format_response(&self, text: &str, _m: &MessageMetadata) -> String {
            text.to_string()
        }
        fn max_message_length(&self) -> usize {
            4096
        }
        fn supports_markdown(&self) -> bool {
            false
        }
        fn supports_images(&self) -> bool {
            false
        }
        fn supports_files(&self) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn processor_forwards_tokens_and_done() {
        let adapter = DumbAdapter::new();
        let metadata = MessageMetadata::default();
        let cfg = GatewayStreamingConfig {
            tool_progress: false,
            show_reasoning: false,
            ..Default::default()
        };
        let hooks = std::sync::Arc::new(crate::hooks::HookRegistry::new());
        let broker = InteractionBroker::new();

        let (processor, event_tx, consumer) = GatewayEventProcessor::new(
            adapter.clone(),
            metadata,
            cfg,
            hooks,
            broker,
            "webhook:test".into(),
        );

        let consumer_task = tokio::spawn(consumer.run());
        let processor_task = tokio::spawn(processor.run());

        // Send a few tokens then Done
        event_tx.send(StreamEvent::Token("Hello".into())).unwrap();
        event_tx.send(StreamEvent::Token(" world".into())).unwrap();
        event_tx.send(StreamEvent::Done).unwrap();
        drop(event_tx);

        consumer_task.await.unwrap();
        processor_task.await.unwrap();

        let sent = adapter.drain().await;
        // The consumer (batch mode, DumbAdapter doesn't support editing)
        // should deliver one message containing both tokens.
        assert!(!sent.is_empty(), "expected at least one sent message");
        let full = sent.join("");
        assert!(full.contains("Hello"), "expected 'Hello' in output: {full}");
        assert!(full.contains("world"), "expected 'world' in output: {full}");
    }

    #[tokio::test]
    async fn processor_sends_tool_status_when_enabled() {
        let adapter = DumbAdapter::new();
        let metadata = MessageMetadata::default();
        let cfg = GatewayStreamingConfig {
            tool_progress: true,
            show_reasoning: true,
            ..Default::default()
        };
        let hooks = std::sync::Arc::new(crate::hooks::HookRegistry::new());
        let broker = InteractionBroker::new();

        let (processor, event_tx, consumer) = GatewayEventProcessor::new(
            adapter.clone(),
            metadata,
            cfg,
            hooks,
            broker,
            "webhook:test".into(),
        );
        let consumer_task = tokio::spawn(consumer.run());
        let processor_task = tokio::spawn(processor.run());

        event_tx
            .send(StreamEvent::ToolExec {
                name: "web_search".into(),
                args_json: "{}".into(),
            })
            .unwrap();
        event_tx.send(StreamEvent::Token("answer".into())).unwrap();
        event_tx.send(StreamEvent::Done).unwrap();
        drop(event_tx);

        consumer_task.await.unwrap();
        processor_task.await.unwrap();

        let sent = adapter.drain().await;
        let joined = sent.join(" ");
        assert!(
            joined.contains("web_search"),
            "expected tool name in status: {joined}"
        );
    }

    #[tokio::test]
    async fn processor_suppresses_tool_status_when_disabled() {
        let adapter = DumbAdapter::new();
        let metadata = MessageMetadata::default();
        let cfg = GatewayStreamingConfig {
            tool_progress: false,
            show_reasoning: false,
            ..Default::default()
        };
        let hooks = std::sync::Arc::new(crate::hooks::HookRegistry::new());
        let broker = InteractionBroker::new();

        let (processor, event_tx, consumer) = GatewayEventProcessor::new(
            adapter.clone(),
            metadata,
            cfg,
            hooks,
            broker,
            "webhook:test".into(),
        );
        let consumer_task = tokio::spawn(consumer.run());
        let processor_task = tokio::spawn(processor.run());

        event_tx
            .send(StreamEvent::ToolExec {
                name: "file_read".into(),
                args_json: "{}".into(),
            })
            .unwrap();
        event_tx.send(StreamEvent::Token("done".into())).unwrap();
        event_tx.send(StreamEvent::Done).unwrap();
        drop(event_tx);

        consumer_task.await.unwrap();
        processor_task.await.unwrap();

        let sent = adapter.drain().await;
        // Only the final answer should appear — no tool status messages.
        for msg in &sent {
            assert!(
                !msg.contains("file_read"),
                "unexpected tool status in output: {msg}"
            );
        }
    }

    #[tokio::test]
    async fn processor_reports_subagent_progress_and_completion() {
        let adapter = DumbAdapter::new();
        let metadata = MessageMetadata::default();
        let cfg = GatewayStreamingConfig {
            tool_progress: true,
            show_reasoning: false,
            ..Default::default()
        };
        let hooks = std::sync::Arc::new(crate::hooks::HookRegistry::new());
        let broker = InteractionBroker::new();

        let (processor, event_tx, consumer) = GatewayEventProcessor::new(
            adapter.clone(),
            metadata,
            cfg,
            hooks,
            broker,
            "webhook:test".into(),
        );
        let consumer_task = tokio::spawn(consumer.run());
        let processor_task = tokio::spawn(processor.run());

        event_tx
            .send(StreamEvent::SubAgentStart {
                task_index: 0,
                task_count: 2,
                goal: "inspect delegation".into(),
            })
            .unwrap();
        event_tx
            .send(StreamEvent::SubAgentReasoning {
                task_index: 0,
                task_count: 2,
                text: "scoping the repo".into(),
            })
            .unwrap();
        for tool_name in [
            "file_search",
            "terminal",
            "read_file",
            "terminal",
            "terminal",
        ] {
            event_tx
                .send(StreamEvent::SubAgentToolExec {
                    task_index: 0,
                    task_count: 2,
                    name: tool_name.into(),
                    args_json: "{}".into(),
                })
                .unwrap();
        }
        event_tx
            .send(StreamEvent::SubAgentFinish {
                task_index: 0,
                task_count: 2,
                status: "completed".into(),
                duration_ms: 2_300,
                summary: "delegation audited".into(),
                api_calls: 2,
                model: Some("mock/model".into()),
            })
            .unwrap();
        event_tx.send(StreamEvent::Done).unwrap();
        drop(event_tx);

        consumer_task.await.unwrap();
        processor_task.await.unwrap();

        let joined = adapter.drain().await.join("\n");
        assert!(joined.contains("Starting delegated task"));
        assert!(joined.contains("file_search, terminal, read_file, terminal, terminal"));
        assert!(joined.contains("completed in 2.3s"));
        assert!(joined.contains("delegation audited"));
    }

    #[tokio::test]
    async fn processor_surfaces_subagent_reasoning_when_enabled() {
        let adapter = DumbAdapter::new();
        let metadata = MessageMetadata::default();
        let cfg = GatewayStreamingConfig {
            tool_progress: true,
            show_reasoning: true,
            ..Default::default()
        };
        let hooks = std::sync::Arc::new(crate::hooks::HookRegistry::new());
        let broker = InteractionBroker::new();

        let (processor, event_tx, consumer) = GatewayEventProcessor::new(
            adapter.clone(),
            metadata,
            cfg,
            hooks,
            broker,
            "webhook:test".into(),
        );
        let consumer_task = tokio::spawn(consumer.run());
        let processor_task = tokio::spawn(processor.run());

        event_tx
            .send(StreamEvent::SubAgentReasoning {
                task_index: 1,
                task_count: 3,
                text: "scoping the repo".into(),
            })
            .unwrap();
        event_tx.send(StreamEvent::Done).unwrap();
        drop(event_tx);

        consumer_task.await.unwrap();
        processor_task.await.unwrap();

        let joined = adapter.drain().await.join("\n");
        assert!(joined.contains("[2/3]"));
        assert!(joined.contains("scoping the repo"));
    }

    #[tokio::test]
    async fn processor_reports_tool_errors_when_progress_enabled() {
        let adapter = DumbAdapter::new();
        let metadata = MessageMetadata::default();
        let cfg = GatewayStreamingConfig {
            tool_progress: true,
            show_reasoning: false,
            ..Default::default()
        };
        let hooks = std::sync::Arc::new(crate::hooks::HookRegistry::new());
        let broker = InteractionBroker::new();

        let (processor, event_tx, consumer) = GatewayEventProcessor::new(
            adapter.clone(),
            metadata,
            cfg,
            hooks,
            broker,
            "webhook:test".into(),
        );
        let consumer_task = tokio::spawn(consumer.run());
        let processor_task = tokio::spawn(processor.run());

        event_tx
            .send(StreamEvent::ToolDone {
                name: "terminal".into(),
                args_json: "{}".into(),
                result_preview: Some("permission denied".into()),
                duration_ms: 1_500,
                is_error: true,
            })
            .unwrap();
        event_tx.send(StreamEvent::Done).unwrap();
        drop(event_tx);

        consumer_task.await.unwrap();
        processor_task.await.unwrap();

        let joined = adapter.drain().await.join("\n");
        assert!(joined.contains("terminal failed in 1.5s: permission denied"));
    }

    #[tokio::test]
    async fn processor_surfaces_context_pressure_as_status() {
        let adapter = DumbAdapter::new();
        let metadata = MessageMetadata::default();
        let cfg = GatewayStreamingConfig::default();
        let hooks = std::sync::Arc::new(crate::hooks::HookRegistry::new());
        let broker = InteractionBroker::new();

        let (processor, event_tx, consumer) = GatewayEventProcessor::new(
            adapter.clone(),
            metadata,
            cfg,
            hooks,
            broker,
            "webhook:test".into(),
        );
        let consumer_task = tokio::spawn(consumer.run());
        let processor_task = tokio::spawn(processor.run());

        event_tx
            .send(StreamEvent::ContextPressure {
                estimated_tokens: 27_000,
                threshold_tokens: 32_000,
            })
            .unwrap();
        event_tx.send(StreamEvent::Token("done".into())).unwrap();
        event_tx.send(StreamEvent::Done).unwrap();
        drop(event_tx);

        consumer_task.await.unwrap();
        processor_task.await.unwrap();

        let joined = adapter.drain().await.join("\n");
        assert!(joined.contains("Context"));
        assert!(joined.contains("compression"));
        assert!(joined.contains("27000/32000"));
    }

    #[test]
    fn surfaces_file_edit_completions_but_not_generic_searches() {
        assert!(should_surface_tool_completion(
            "write_file",
            "Wrote 42 bytes to 'src/main.rs'"
        ));
        assert!(should_surface_tool_completion(
            "apply_patch",
            "Patched 'src/lib.rs': 2 replacement(s)"
        ));
        assert!(!should_surface_tool_completion(
            "web_search",
            "Found 10 results"
        ));
    }

    #[tokio::test]
    async fn processor_registers_approval_instead_of_auto_approving() {
        let adapter = DumbAdapter::new();
        let metadata = MessageMetadata::default();
        let cfg = GatewayStreamingConfig::default();
        let hooks = std::sync::Arc::new(crate::hooks::HookRegistry::new());
        let broker = InteractionBroker::new();

        let (processor, event_tx, consumer) = GatewayEventProcessor::new(
            adapter.clone(),
            metadata,
            cfg,
            hooks,
            broker.clone(),
            "webhook:test".into(),
        );
        let consumer_task = tokio::spawn(consumer.run());
        let processor_task = tokio::spawn(processor.run());

        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        event_tx
            .send(StreamEvent::Approval {
                command: "rm -rf /tmp/demo".into(),
                full_command: "rm -rf /tmp/demo".into(),
                reasons: vec!["destructive-file-ops".into()],
                response_tx,
            })
            .unwrap();

        let pending = wait_for_pending(&broker, "webhook:test").await;
        assert!(matches!(
            pending.kind,
            PendingInteractionKind::Approval { .. }
        ));

        let sent = adapter.drain().await.join("\n");
        assert!(sent.contains("Approval required"));
        assert!(sent.contains("destructive-file-ops"));

        let count = broker
            .resolve_oldest_approval("webhook:test", edgecrab_core::ApprovalChoice::Session)
            .await;
        assert_eq!(count, 1);
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), response_rx)
                .await
                .expect("approval resolution timeout")
                .expect("approval resolution channel"),
            edgecrab_core::ApprovalChoice::Session
        );

        drop(event_tx);
        tokio::time::timeout(std::time::Duration::from_secs(1), processor_task)
            .await
            .expect("processor timeout")
            .unwrap();
        consumer_task.abort();
    }

    #[tokio::test]
    async fn processor_registers_clarify_request_for_gateway_reply() {
        let adapter = DumbAdapter::new();
        let metadata = MessageMetadata::default();
        let cfg = GatewayStreamingConfig::default();
        let hooks = std::sync::Arc::new(crate::hooks::HookRegistry::new());
        let broker = InteractionBroker::new();

        let (processor, event_tx, consumer) = GatewayEventProcessor::new(
            adapter.clone(),
            metadata,
            cfg,
            hooks,
            broker.clone(),
            "webhook:test".into(),
        );
        let consumer_task = tokio::spawn(consumer.run());
        let processor_task = tokio::spawn(processor.run());

        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        event_tx
            .send(StreamEvent::Clarify {
                question: "Which folder?".into(),
                choices: Some(vec!["Work".into(), "Personal".into()]),
                response_tx,
            })
            .unwrap();

        let pending = wait_for_pending(&broker, "webhook:test").await;
        assert!(matches!(
            pending.kind,
            PendingInteractionKind::Clarify { .. }
        ));

        let sent = adapter.drain().await.join("\n");
        assert!(sent.contains("Clarification needed"));
        assert!(sent.contains("Which folder?"));

        assert!(
            broker
                .resolve_oldest_clarify("webhook:test", "Personal".into())
                .await
        );
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), response_rx)
                .await
                .expect("clarify resolution timeout")
                .expect("clarify resolution channel"),
            "Personal"
        );

        drop(event_tx);
        tokio::time::timeout(std::time::Duration::from_secs(1), processor_task)
            .await
            .expect("processor timeout")
            .unwrap();
        consumer_task.abort();
    }
}
