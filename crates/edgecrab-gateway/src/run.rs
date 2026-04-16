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

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use edgecrab_command_catalog::{SlashSurface, slash_commands_for_surface};
use edgecrab_tools::tools::transcribe::TranscribeAudioTool;
use edgecrab_tools::tools::tts::TextToSpeechTool;
use edgecrab_tools::tools::vision::VisionAnalyzeTool;
use edgecrab_tools::{AppConfigRef, ToolContext, ToolHandler};
use edgecrab_types::OriginChat;
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::auth;
use crate::config::GatewayConfig;
use crate::delivery::DeliveryRouter;
use crate::event_processor::GatewayEventProcessor;
use crate::hooks::{HookContext, HookRegistry};
use crate::interactions::{InteractionBroker, PendingInteractionKind};
use crate::pairing::PairingStore;
use crate::platform::{IncomingMessage, PlatformAdapter, WebhookDelivery};
use crate::sender::{parse_platform, resolve_target};
use crate::session::{SessionKey, SessionManager};
use crate::voice_delivery::voice_delivery_doctor;
use crate::webhook::WebhookPayload;
use crate::webhook_subscriptions::{
    default_max_body_bytes, default_rate_limit_per_minute, load_subscriptions, verify_signature,
};
use edgecrab_core::{Agent, IsolatedAgentOptions};
use edgecrab_core::{config::resolve_personality, model_catalog::ModelCatalog};
use edgecrab_tools::create_provider_for_model;
use edgecrab_types::Role;

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
const MAX_GATEWAY_EAGER_AUDIO_TRANSCRIPTS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GatewayVoiceMode {
    Off,
    VoiceOnly,
    All,
}

impl GatewayVoiceMode {
    fn label(self) -> &'static str {
        match self {
            Self::Off => "Off (text only)",
            Self::VoiceOnly => "On (voice reply to voice messages)",
            Self::All => "TTS (voice reply to all messages)",
        }
    }
}

fn gateway_voice_mode_path() -> PathBuf {
    edgecrab_core::edgecrab_home().join("gateway_voice_mode.json")
}

fn load_gateway_voice_modes_from(path: &Path) -> HashMap<String, GatewayVoiceMode> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(raw) = serde_json::from_str::<HashMap<String, String>>(&content) else {
        return HashMap::new();
    };

    raw.into_iter()
        .filter_map(|(chat_id, mode)| {
            let parsed = match mode.as_str() {
                "off" => GatewayVoiceMode::Off,
                "voice_only" => GatewayVoiceMode::VoiceOnly,
                "all" => GatewayVoiceMode::All,
                _ => return None,
            };
            Some((chat_id, parsed))
        })
        .collect()
}

fn save_gateway_voice_modes_to(
    path: &Path,
    modes: &HashMap<String, GatewayVoiceMode>,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw: HashMap<&str, &str> = modes
        .iter()
        .map(|(chat_id, mode)| {
            let mode = match mode {
                GatewayVoiceMode::Off => "off",
                GatewayVoiceMode::VoiceOnly => "voice_only",
                GatewayVoiceMode::All => "all",
            };
            (chat_id.as_str(), mode)
        })
        .collect();
    std::fs::write(path, serde_json::to_vec_pretty(&raw)?)?;
    Ok(())
}

fn incoming_message_is_voice_origin(msg: &IncomingMessage) -> bool {
    let has_voice_note = msg
        .metadata
        .attachments
        .iter()
        .any(|attachment| attachment.kind == crate::platform::MessageAttachmentKind::Voice);
    if has_voice_note {
        return true;
    }

    // Some bridges only classify note-style inbound audio as `Audio`.
    let has_audio = msg.metadata.attachments.iter().any(|attachment| {
        matches!(
            attachment.kind,
            crate::platform::MessageAttachmentKind::Audio
                | crate::platform::MessageAttachmentKind::Voice
        )
    });
    has_audio && msg.text.trim().is_empty()
}

fn gateway_builtin_commands() -> Vec<(String, String)> {
    let mut commands = Vec::new();
    for command in slash_commands_for_surface(SlashSurface::Gateway) {
        commands.push((
            format!("/{}", command.name),
            command.description.to_string(),
        ));
        for alias in command.aliases {
            commands.push((format!("/{alias}"), format!("Alias for /{}", command.name)));
        }
    }
    commands.sort_by(|a, b| a.0.cmp(&b.0));
    commands
}

fn installed_skill_commands() -> Vec<String> {
    let skills_dir = edgecrab_core::edgecrab_home().join("skills");
    let Ok(entries) = std::fs::read_dir(skills_dir) else {
        return Vec::new();
    };

    let mut names = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(raw_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if path.is_dir() {
            names.push(raw_name.to_string());
        } else if path.extension().is_some_and(|ext| ext == "md") {
            names.push(raw_name.trim_end_matches(".md").to_string());
        }
    }
    names.sort();
    names.dedup();
    names
}

fn gateway_commands_page(page: usize) -> String {
    const PAGE_SIZE: usize = 12;

    let mut entries = gateway_builtin_commands();
    entries.extend(
        installed_skill_commands()
            .into_iter()
            .map(|name| (format!("/{name}"), "Installed skill command".to_string())),
    );
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let total_pages = entries.len().max(1).div_ceil(PAGE_SIZE);
    let page = page.clamp(1, total_pages);
    let start = (page - 1) * PAGE_SIZE;
    let end = (start + PAGE_SIZE).min(entries.len());

    let mut text = format!("Available commands page {page}/{total_pages}\n\n");
    for (name, description) in &entries[start..end] {
        text.push_str(&format!("{name:<18} {description}\n"));
    }
    if total_pages > 1 {
        text.push_str("\nUse /commands <page> to browse more.");
    }
    text
}

/// Help text shown when a user sends /help to the gateway.
fn gateway_help_text() -> String {
    let mut text = String::from("*Available commands:*\n\n");
    for (name, description) in gateway_builtin_commands() {
        text.push_str(&format!("{name} - {description}\n"));
    }
    text.push_str(
        "\nAny other message is forwarded to the AI agent.\n\n\
         Tip: if you send a message while the agent is responding, it will be\n\
         queued and processed after the current response finishes. Use /stop\n\
         to cancel the current response and discard the queue.\n\n\
         When the agent asks for clarification, reply with plain text or a\n\
         choice number. When it asks for approval, reply `/approve`,\n\
         `/approve session`, `/approve always`, or `/deny`.",
    );
    text
}

fn format_gateway_insights(
    snapshot: &edgecrab_core::SessionSnapshot,
    historical: Option<&edgecrab_state::InsightsReport>,
) -> String {
    let cost = edgecrab_core::pricing::estimate_cost(
        &edgecrab_core::pricing::CanonicalUsage {
            input_tokens: snapshot.input_tokens,
            output_tokens: snapshot.output_tokens,
            cache_read_tokens: snapshot.cache_read_tokens,
            cache_write_tokens: snapshot.cache_write_tokens,
            reasoning_tokens: snapshot.reasoning_tokens,
        },
        &snapshot.model,
    );

    let mut text = String::from("Current chat session\n");
    text.push_str(&format!("• Model: {}\n", snapshot.model));
    text.push_str(&format!("• User turns: {}\n", snapshot.user_turn_count));
    text.push_str(&format!("• Messages: {}\n", snapshot.message_count));
    text.push_str(&format!("• API calls: {}\n", snapshot.api_call_count));
    text.push_str(&format!(
        "• Tokens: {}\n",
        snapshot.input_tokens
            + snapshot.output_tokens
            + snapshot.cache_read_tokens
            + snapshot.cache_write_tokens
            + snapshot.reasoning_tokens
    ));
    text.push_str(&format!(
        "• Estimated cost: ${:.4}\n",
        cost.amount_usd.unwrap_or(0.0)
    ));

    if let Some(report) = historical {
        let ov = &report.overview;
        text.push_str(&format!("\nLast {} days\n", report.days));
        text.push_str(&format!("• Sessions: {}\n", ov.total_sessions));
        text.push_str(&format!("• Messages: {}\n", ov.total_messages));
        text.push_str(&format!("• Tool calls: {}\n", ov.total_tool_calls));
        text.push_str(&format!(
            "• Estimated cost: ${:.2}\n",
            ov.estimated_total_cost_usd
        ));
        if !report.models.is_empty() {
            text.push_str("\nTop models:\n");
            for model in report.models.iter().take(5) {
                text.push_str(&format!(
                    "• {}: {} sessions, ${:.2}\n",
                    model.model, model.sessions, model.estimated_cost_usd
                ));
            }
        }
        if !report.top_tools.is_empty() {
            text.push_str("\nTop tools:\n");
            for tool in report.top_tools.iter().take(5) {
                text.push_str(&format!("• {}: {}\n", tool.name, tool.count));
            }
        }
    }

    text
}

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

fn audio_attachment_sources(msg: &IncomingMessage) -> Vec<String> {
    msg.metadata
        .attachments
        .iter()
        .filter(|attachment| {
            matches!(
                attachment.kind,
                crate::platform::MessageAttachmentKind::Audio
                    | crate::platform::MessageAttachmentKind::Voice
            )
        })
        .filter_map(|attachment| attachment.local_path.as_deref())
        .filter(|path| !path.trim().is_empty())
        .map(str::to_string)
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
    let audio_sources = audio_attachment_sources(msg);
    if image_sources.is_empty() && audio_sources.is_empty() {
        return msg.text.clone();
    }

    let provider = agent.provider_handle().await;
    let auxiliary = agent.auxiliary_config().await;
    let (_, stt, _) = agent.media_config().await;
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
        stt_provider: Some(stt.provider),
        stt_whisper_model: Some(stt.whisper_model),
        ..Default::default()
    };

    let mut effective_text = msg.text.clone();
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
            delegation_event_tx: None,
            clarify_tx: None,
            approval_tx: None,
            on_skills_changed: None,
            gateway_sender: None,
            origin_chat: None,
            session_key: Some(session_key.clone()),
            todo_store: None,
            current_tool_call_id: None,
            current_tool_name: None,
            injected_messages: None,
            tool_progress_tx: None,
            watch_notification_tx: None,
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

    if !image_sources.is_empty() {
        effective_text = render_gateway_image_context(
            &effective_text,
            &image_sources,
            &analyses,
            &failures,
            image_sources
                .len()
                .saturating_sub(MAX_GATEWAY_EAGER_IMAGE_ANALYSES),
        );
    }

    let mut transcripts = Vec::new();
    let mut transcription_failures = Vec::new();
    for (idx, source) in audio_sources
        .iter()
        .take(MAX_GATEWAY_EAGER_AUDIO_TRANSCRIPTS)
        .enumerate()
    {
        let ctx = ToolContext {
            task_id: format!("gateway-audio-pretranscribe-{}", idx + 1),
            cwd: cwd.clone(),
            session_id: session_key.clone(),
            user_task: None,
            cancel: CancellationToken::new(),
            config: config.clone(),
            state_db: None,
            platform: msg.platform,
            process_table: None,
            provider: None,
            tool_registry: None,
            delegate_depth: 0,
            sub_agent_runner: None,
            delegation_event_tx: None,
            clarify_tx: None,
            approval_tx: None,
            on_skills_changed: None,
            gateway_sender: None,
            origin_chat: None,
            session_key: Some(session_key.clone()),
            todo_store: None,
            current_tool_call_id: None,
            current_tool_name: None,
            injected_messages: None,
            tool_progress_tx: None,
            watch_notification_tx: None,
        };

        match TranscribeAudioTool
            .execute(serde_json::json!({ "file_path": source }), &ctx)
            .await
        {
            Ok(transcript) => transcripts.push(transcript),
            Err(error) => {
                transcription_failures.push(format!("Audio {} ({source}): {error}", idx + 1))
            }
        }
    }

    if !audio_sources.is_empty() {
        effective_text = render_gateway_audio_context(
            &effective_text,
            &audio_sources,
            &transcripts,
            &transcription_failures,
            audio_sources
                .len()
                .saturating_sub(MAX_GATEWAY_EAGER_AUDIO_TRANSCRIPTS),
        );
    }

    effective_text
}

fn render_gateway_audio_context(
    user_text: &str,
    audio_sources: &[String],
    transcripts: &[String],
    transcription_failures: &[String],
    skipped_count: usize,
) -> String {
    if audio_sources.is_empty() {
        return user_text.to_string();
    }

    let mut lines = Vec::new();
    lines.push("*** ATTACHED AUDIO ***".to_string());
    lines.push(format!(
        "The user attached {} audio file(s) or voice note(s).",
        audio_sources.len()
    ));
    lines.push("Audio sources:".to_string());
    for (idx, source) in audio_sources.iter().enumerate() {
        lines.push(format!("- Audio {}: {}", idx + 1, source));
    }
    if !transcripts.is_empty() {
        lines.push(
            "Gateway transcription already ran before this turn. Use it as primary context."
                .to_string(),
        );
        for (idx, transcript) in transcripts.iter().enumerate() {
            lines.push(format!("Transcript {}:\n{}", idx + 1, transcript));
        }
    }
    if !transcription_failures.is_empty() {
        lines.push(
            "Some audio attachments could not be transcribed automatically. If \
             `transcribe_audio` is available in this session and you need more detail, \
             you may call it on one of the local audio paths above."
                .to_string(),
        );
        for failure in transcription_failures {
            lines.push(format!("- {}", failure));
        }
    }
    if skipped_count > 0 {
        lines.push(format!(
            "{skipped_count} additional audio attachment(s) were not pre-transcribed to keep latency bounded."
        ));
    }
    lines.push("*** END ATTACHED AUDIO ***".to_string());

    let block = lines.join("\n");
    if user_text.trim().is_empty() {
        block
    } else {
        format!("{user_text}\n\n{block}")
    }
}

fn background_gateway_arg_preview(args_json: &str) -> String {
    const PRIORITY: &[&str] = &[
        "query", "url", "path", "command", "goal", "prompt", "text", "content",
    ];

    let Ok(value) = serde_json::from_str::<serde_json::Value>(args_json) else {
        return String::new();
    };
    let Some(obj) = value.as_object() else {
        return String::new();
    };
    for key in PRIORITY {
        if let Some(text) = obj.get(*key).and_then(|v| v.as_str()) {
            return edgecrab_core::safe_truncate(text, 48).to_string();
        }
    }
    String::new()
}

fn background_gateway_tool_label(name: &str, args_json: &str) -> String {
    let preview = background_gateway_arg_preview(args_json);
    if preview.is_empty() {
        name.to_string()
    } else {
        format!("{name}: {preview}")
    }
}

async fn deliver_text_and_media(
    delivery: &DeliveryRouter,
    adapter: Option<Arc<dyn PlatformAdapter>>,
    response: &str,
    platform: edgecrab_types::Platform,
    metadata: &crate::platform::MessageMetadata,
) -> anyhow::Result<usize> {
    if let Some(webhook_delivery) = metadata.webhook_delivery.as_ref() {
        if should_use_custom_webhook_delivery(platform, webhook_delivery) {
            return deliver_webhook_response(delivery, response, webhook_delivery).await;
        }
    }

    let (cleaned, media_refs) = crate::platform::extract_media_from_response(response);
    let text_result = if let Some(text) =
        crate::platform::response_text_after_media_extraction(response, &cleaned, &media_refs)
    {
        delivery
            .deliver(&text, platform, metadata)
            .await
            .map(|_| text.len())?
    } else {
        0
    };

    if !media_refs.is_empty() {
        if let Some(adapter) = adapter {
            for mref in &media_refs {
                let result = if mref.is_image {
                    adapter.send_photo(&mref.path, None, metadata).await
                } else if crate::platform::MediaRef::detect_audio(&mref.path) {
                    adapter.send_voice(&mref.path, None, metadata).await
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

fn should_use_custom_webhook_delivery(
    platform: edgecrab_types::Platform,
    webhook_delivery: &WebhookDelivery,
) -> bool {
    if platform != edgecrab_types::Platform::Webhook {
        return false;
    }
    let deliver = webhook_delivery.deliver.trim();
    !deliver.is_empty()
        && !deliver.eq_ignore_ascii_case("log")
        && !deliver.eq_ignore_ascii_case("origin")
        && !deliver.eq_ignore_ascii_case("webhook")
}

async fn deliver_webhook_response(
    delivery: &DeliveryRouter,
    response: &str,
    webhook_delivery: &WebhookDelivery,
) -> anyhow::Result<usize> {
    let deliver = webhook_delivery.deliver.trim();
    if deliver.eq_ignore_ascii_case("github_comment") {
        let text = crate::platform::extract_media_from_response(response).0;
        return deliver_github_comment(text.trim(), &webhook_delivery.deliver_extra);
    }

    let platform = parse_platform(deliver)
        .ok_or_else(|| anyhow::anyhow!("unknown webhook deliver target '{deliver}'"))?;
    let recipient = webhook_delivery
        .deliver_extra
        .get("chat_id")
        .or_else(|| webhook_delivery.deliver_extra.get("recipient"))
        .map(String::as_str)
        .unwrap_or("");
    let target = resolve_target(platform, deliver, recipient)
        .map_err(|err| anyhow::anyhow!("webhook delivery target error: {err}"))?;
    let thread_id = webhook_delivery
        .deliver_extra
        .get("thread_id")
        .or_else(|| webhook_delivery.deliver_extra.get("message_thread_id"))
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .or(target.thread_id.clone());
    let metadata = crate::platform::MessageMetadata {
        channel_id: Some(target.channel_id),
        thread_id,
        ..Default::default()
    };
    delivery.deliver(response, platform, &metadata).await?;
    Ok(response.len())
}

fn deliver_github_comment(
    content: &str,
    extra: &BTreeMap<String, String>,
) -> anyhow::Result<usize> {
    let repo = extra
        .get("repo")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("github_comment delivery requires deliver_extra.repo"))?;
    let pr_number = extra
        .get("pr_number")
        .or_else(|| extra.get("issue_number"))
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("github_comment delivery requires deliver_extra.pr_number")
        })?;

    let result = Command::new("gh")
        .args([
            "pr", "comment", pr_number, "--repo", repo, "--body", content,
        ])
        .output()
        .map_err(|error| {
            anyhow::anyhow!("failed to run gh for github_comment delivery: {error}")
        })?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr).trim().to_string();
        anyhow::bail!(
            "github_comment delivery failed: {}",
            if stderr.is_empty() {
                format!("gh exited with {}", result.status)
            } else {
                stderr
            }
        );
    }
    Ok(content.len())
}

async fn maybe_send_voice_reply(
    agent: &Agent,
    adapter: Option<Arc<dyn PlatformAdapter>>,
    msg: &IncomingMessage,
    response: &str,
    mode: GatewayVoiceMode,
) {
    let Some(adapter) = adapter else {
        return;
    };
    if mode == GatewayVoiceMode::Off {
        return;
    }
    if mode == GatewayVoiceMode::VoiceOnly && !incoming_message_is_voice_origin(msg) {
        return;
    }

    let (cleaned, media_refs) = crate::platform::extract_media_from_response(response);
    if media_refs
        .iter()
        .any(|media_ref| crate::platform::MediaRef::detect_audio(&media_ref.path))
    {
        return;
    }
    let Some(text) =
        crate::platform::response_text_after_media_extraction(response, &cleaned, &media_refs)
    else {
        return;
    };
    let Some(text) = edgecrab_tools::tools::tts::sanitize_text_for_tts(text.trim(), 4000) else {
        return;
    };

    let (tts, _, _) = agent.media_config().await;
    let ctx = ToolContext {
        task_id: "gateway-auto-tts".into(),
        cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        session_id: format!("gateway-auto-tts-{}", message_origin_recipient(msg)),
        user_task: None,
        cancel: CancellationToken::new(),
        config: AppConfigRef {
            edgecrab_home: edgecrab_core::edgecrab_home(),
            tts_provider: Some(tts.provider),
            tts_voice: Some(tts.voice),
            tts_rate: tts.rate,
            tts_model: tts.model,
            tts_elevenlabs_voice_id: tts.elevenlabs_voice_id,
            tts_elevenlabs_model_id: tts.elevenlabs_model_id,
            tts_elevenlabs_api_key_env: Some(tts.elevenlabs_api_key_env),
            ..Default::default()
        },
        state_db: None,
        platform: msg.platform,
        process_table: None,
        provider: None,
        tool_registry: None,
        delegate_depth: 0,
        sub_agent_runner: None,
        delegation_event_tx: None,
        clarify_tx: None,
        approval_tx: None,
        on_skills_changed: None,
        gateway_sender: None,
        origin_chat: None,
        session_key: Some(message_origin_recipient(msg)),
        todo_store: None,
        current_tool_call_id: None,
        current_tool_name: None,
        injected_messages: None,
        tool_progress_tx: None,
        watch_notification_tx: None,
    };

    let result = match TextToSpeechTool
        .execute(serde_json::json!({ "text": text }), &ctx)
        .await
    {
        Ok(output) => output,
        Err(error) => {
            tracing::warn!(platform = ?msg.platform, error = %error, "gateway auto TTS failed");
            return;
        }
    };

    let Some(audio_path) = edgecrab_tools::tools::tts::extract_audio_path_from_tts_output(&result)
    else {
        tracing::warn!("gateway auto TTS returned no usable audio path");
        return;
    };

    if let Err(error) = adapter.send_voice(&audio_path, None, &msg.metadata).await {
        tracing::warn!(
            platform = ?msg.platform,
            path = %audio_path,
            error = %error,
            "gateway auto voice delivery failed"
        );
    }

    let temp_tts_dir = std::env::temp_dir().join("edgecrab_tts");
    if Path::new(&audio_path).starts_with(&temp_tts_dir) {
        let _ = tokio::fs::remove_file(&audio_path).await;
    }
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

fn extract_stream_fallback_response(
    messages: &[edgecrab_types::Message],
    baseline_len: usize,
) -> Option<String> {
    messages
        .iter()
        .skip(baseline_len)
        .rev()
        .find(|message| message.role == Role::Assistant)
        .or_else(|| {
            messages
                .iter()
                .rev()
                .find(|message| message.role == Role::Assistant)
        })
        .map(|message| message.text_content())
        .filter(|text| !text.trim().is_empty())
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.as_str()
    } else {
        "unknown panic payload"
    }
}

/// Shared gateway state, passed to axum handlers via State extractor.
#[derive(Clone)]
pub struct GatewayState {
    pub session_manager: Arc<SessionManager>,
    pub hook_registry: Arc<HookRegistry>,
    pub message_tx: mpsc::Sender<IncomingMessage>,
    pub cancel: CancellationToken,
    webhook_ingress: Arc<tokio::sync::Mutex<WebhookIngressState>>,
}

#[derive(Default)]
struct WebhookIngressState {
    seen_deliveries: HashMap<String, Instant>,
    route_windows: HashMap<String, Vec<Instant>>,
}

impl WebhookIngressState {
    fn prune(&mut self, now: Instant, ttl: Duration) {
        self.seen_deliveries
            .retain(|_, seen_at| now.duration_since(*seen_at) <= ttl);
        for timestamps in self.route_windows.values_mut() {
            timestamps.retain(|seen_at| now.duration_since(*seen_at) <= Duration::from_secs(60));
        }
    }

    fn is_duplicate(&mut self, key: &str, now: Instant, ttl: Duration) -> bool {
        if let Some(previous) = self.seen_deliveries.get(key).copied()
            && now.duration_since(previous) <= ttl
        {
            return true;
        }
        self.seen_deliveries.insert(key.to_string(), now);
        false
    }

    fn exceeds_rate_limit(&mut self, route: &str, limit_per_minute: u32, now: Instant) -> bool {
        let bucket = self.route_windows.entry(route.to_string()).or_default();
        bucket.retain(|seen_at| now.duration_since(*seen_at) <= Duration::from_secs(60));
        if limit_per_minute > 0 && bucket.len() >= limit_per_minute as usize {
            return true;
        }
        bucket.push(now);
        false
    }
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
    /// Reset requests deferred until the current session task exits.
    pending_session_resets: Arc<tokio::sync::Mutex<HashSet<String>>>,
    /// Pending approval / clarify interactions keyed by gateway session.
    interaction_broker: Arc<InteractionBroker>,
    /// Per-chat persisted voice reply mode, mirroring Hermes gateway semantics.
    voice_modes: Arc<tokio::sync::Mutex<HashMap<String, GatewayVoiceMode>>>,
    voice_mode_path: PathBuf,
    /// Pairing store for DM code-based approval.
    pairing_store: Arc<PairingStore>,
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
            pending_session_resets: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            interaction_broker: InteractionBroker::new(),
            voice_modes: Arc::new(tokio::sync::Mutex::new(load_gateway_voice_modes_from(
                &gateway_voice_mode_path(),
            ))),
            voice_mode_path: gateway_voice_mode_path(),
            pairing_store: Arc::new(PairingStore::new()),
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

    async fn set_voice_mode(&self, chat_key: &str, mode: GatewayVoiceMode) -> anyhow::Result<()> {
        let snapshot = {
            let mut voice_modes = self.voice_modes.lock().await;
            voice_modes.insert(chat_key.to_string(), mode);
            voice_modes.clone()
        };
        save_gateway_voice_modes_to(&self.voice_mode_path, &snapshot)
    }

    async fn voice_mode_for(&self, chat_key: &str) -> GatewayVoiceMode {
        self.voice_modes
            .lock()
            .await
            .get(chat_key)
            .copied()
            .unwrap_or(GatewayVoiceMode::Off)
    }

    async fn handle_voice_command(&self, msg: &IncomingMessage, chat_key: &str) -> String {
        let args = msg.get_command_args().trim().to_ascii_lowercase();
        match args.as_str() {
            "on" | "enable" => match self.set_voice_mode(chat_key, GatewayVoiceMode::VoiceOnly).await {
                Ok(()) => "Voice mode enabled.\nI'll reply with audio when you send a voice message.\nUse /voice tts to get spoken replies for every message.".into(),
                Err(error) => format!("Failed to persist voice mode: {error}"),
            },
            "tts" => match self.set_voice_mode(chat_key, GatewayVoiceMode::All).await {
                Ok(()) => "Auto-TTS enabled.\nAll replies in this chat will include an audio response when TTS is available.".into(),
                Err(error) => format!("Failed to persist voice mode: {error}"),
            },
            "off" | "disable" => match self.set_voice_mode(chat_key, GatewayVoiceMode::Off).await {
                Ok(()) => "Voice mode disabled. Text-only replies.".into(),
                Err(error) => format!("Failed to persist voice mode: {error}"),
            },
            "status" => {
                let mode = self.voice_mode_for(chat_key).await;
                format!(
                    "Voice mode: {}\n\
                     Platform delivery: {}\n\
                     /voice on   - speak replies only for voice-originating messages\n\
                     /voice tts  - speak replies for all messages\n\
                     /voice off  - disable spoken replies\n\
                     /voice doctor - show platform voice delivery diagnostics",
                    mode.label(),
                    msg.platform
                )
            }
            "doctor" => voice_delivery_doctor(msg.platform),
            "" => {
                let current = self.voice_mode_for(chat_key).await;
                let next = if current == GatewayVoiceMode::Off {
                    GatewayVoiceMode::VoiceOnly
                } else {
                    GatewayVoiceMode::Off
                };
                match self.set_voice_mode(chat_key, next).await {
                    Ok(()) => format!("Voice mode: {}", next.label()),
                    Err(error) => format!("Failed to persist voice mode: {error}"),
                }
            }
            _ => {
                "Usage: /voice [on|off|tts|status|doctor]\n\
                 `on` speaks replies to voice messages only. `tts` speaks every reply."
                    .into()
            }
        }
    }

    async fn session_is_running(&self, session_key: &str) -> bool {
        self.running_sessions.lock().await.contains_key(session_key)
    }

    async fn resolve_command_session_agent(
        &self,
        msg: &IncomingMessage,
        origin_chat_id: &str,
    ) -> Result<Arc<Agent>, String> {
        let Some(base_agent) = self.agent.as_ref().cloned() else {
            return Err("No agent configured.".into());
        };
        let session_lookup = SessionKey::new(
            msg.platform,
            &msg.user_id,
            msg.channel_id
                .as_deref()
                .or(msg.metadata.channel_id.as_deref()),
        );
        let session = self
            .session_manager
            .resolve(
                &session_lookup,
                &base_agent,
                OriginChat::new(msg.platform.to_string(), origin_chat_id.to_string()),
            )
            .await
            .map_err(|error| error.to_string())?;
        let mut guard = session.write().await;
        guard.touch();
        Ok(guard.agent.clone())
    }

    async fn handle_model_command(&self, msg: &IncomingMessage, origin_chat_id: &str) -> String {
        let Ok(agent) = self
            .resolve_command_session_agent(msg, origin_chat_id)
            .await
        else {
            return "No agent configured.".into();
        };

        let target = msg.get_command_args().trim();
        if target.is_empty() || target.eq_ignore_ascii_case("status") {
            let current = agent.model().await;
            return format!(
                "Current model: {current}\nUsage: /model <provider>/<model>\nThis is session-scoped in gateway mode."
            );
        }

        let Some((provider, model)) = target.split_once('/') else {
            return format!("Invalid model target '{target}'. Use /model <provider>/<model>.");
        };

        match create_provider_for_model(provider, model) {
            Ok(provider_impl) => {
                agent.swap_model(target.to_string(), provider_impl).await;
                format!("Model switched to {target} for this chat session.")
            }
            Err(error) => format!("Failed to switch model to {target}: {error}"),
        }
    }

    async fn handle_provider_command(&self, msg: &IncomingMessage, origin_chat_id: &str) -> String {
        let Ok(agent) = self
            .resolve_command_session_agent(msg, origin_chat_id)
            .await
        else {
            return "No agent configured.".into();
        };
        let current_model = agent.model().await;
        let current_provider = current_model.split('/').next().unwrap_or("unknown");
        let catalog = ModelCatalog::get();
        let providers = catalog
            .providers
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        format!("Current provider: {current_provider}\nAvailable providers: {providers}")
    }

    async fn handle_reasoning_command(
        &self,
        msg: &IncomingMessage,
        origin_chat_id: &str,
    ) -> String {
        let Ok(agent) = self
            .resolve_command_session_agent(msg, origin_chat_id)
            .await
        else {
            return "No agent configured.".into();
        };
        let level = msg.get_command_args().trim().to_ascii_lowercase();
        match level.as_str() {
            "" | "status" => {
                "Usage: /reasoning <low|medium|high|status>\nGateway mode supports effort selection; reasoning-visibility toggles remain TUI-only.".into()
            }
            "low" | "medium" | "high" => {
                agent.set_reasoning_effort(Some(level.clone())).await;
                format!("Reasoning effort set to {level} for this chat session.")
            }
            _ => "Unknown reasoning option. Use: low, medium, high, status".into(),
        }
    }

    async fn handle_personality_command(
        &self,
        msg: &IncomingMessage,
        origin_chat_id: &str,
    ) -> String {
        let Ok(agent) = self
            .resolve_command_session_agent(msg, origin_chat_id)
            .await
        else {
            return "No agent configured.".into();
        };
        let config = edgecrab_core::AppConfig::load().unwrap_or_default();
        let name = msg.get_command_args().trim().to_ascii_lowercase();

        if name.is_empty() || name == "status" {
            let catalog = edgecrab_core::config::personality_catalog(&config)
                .into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>()
                .join(", ");
            return format!("Usage: /personality <name|clear>\nAvailable presets: {catalog}");
        }

        if name == "clear" {
            let configured = resolve_personality(&config, &config.display.personality);
            agent.set_personality_addon(configured).await;
            return "Cleared the temporary personality overlay for this chat session.".into();
        }

        match resolve_personality(&config, &name) {
            Some(addon) => {
                agent.set_personality_addon(Some(addon)).await;
                format!("Personality switched to '{name}' for this chat session.")
            }
            None => {
                let catalog = edgecrab_core::config::personality_catalog(&config)
                    .into_iter()
                    .map(|(name, _)| name)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("Unknown personality '{name}'. Available: {catalog}")
            }
        }
    }

    async fn handle_resume_command(&self, msg: &IncomingMessage, origin_chat_id: &str) -> String {
        let query = msg.get_command_args().trim();
        if query.is_empty() {
            return "Usage: /resume <session-id-or-title>".into();
        }

        let Ok(agent) = self
            .resolve_command_session_agent(msg, origin_chat_id)
            .await
        else {
            return "No agent configured.".into();
        };
        let Some(db) = agent.state_db().await else {
            return "Session database is not configured.".into();
        };

        match db.resolve_session(query) {
            Ok(Some(id)) => match agent.restore_session(&id).await {
                Ok(count) => format!("Resumed session {id} ({count} message(s) restored)."),
                Err(error) => format!("Failed to restore session {id}: {error}"),
            },
            Ok(None) => format!("No session found matching '{query}'."),
            Err(error) => format!("Failed to resolve session '{query}': {error}"),
        }
    }

    async fn handle_title_command(&self, msg: &IncomingMessage, origin_chat_id: &str) -> String {
        let title = msg.get_command_args().trim();
        if title.is_empty() {
            return "Usage: /title <name>".into();
        }
        let Ok(agent) = self
            .resolve_command_session_agent(msg, origin_chat_id)
            .await
        else {
            return "No agent configured.".into();
        };
        agent.set_session_title(title.to_string()).await;
        format!("Session title set to '{title}'.")
    }

    async fn handle_sethome_command(&self, msg: &IncomingMessage) -> String {
        let trimmed = msg.get_command_args().trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("status") {
            let config = edgecrab_core::AppConfig::load().unwrap_or_default();
            let mut lines = Vec::new();
            if let Some(value) = config.gateway.telegram.home_channel.as_deref() {
                lines.push(format!("telegram: {value}"));
            }
            if let Some(value) = config.gateway.discord.home_channel.as_deref() {
                lines.push(format!("discord: {value}"));
            }
            if let Some(value) = config.gateway.slack.home_channel.as_deref() {
                lines.push(format!("slack: {value}"));
            }
            if lines.is_empty() {
                return "No home channels configured.\nUsage: /sethome <telegram|discord|slack> <channel|clear>".into();
            }
            return format!("Configured home channels:\n{}", lines.join("\n"));
        }

        let mut parts = trimmed.split_whitespace();
        let platform = parts.next().unwrap_or_default().to_ascii_lowercase();
        let value = parts.collect::<Vec<_>>().join(" ");
        if value.is_empty() {
            return "Usage: /sethome <telegram|discord|slack> <channel|clear>".into();
        }
        let channel = (!value.eq_ignore_ascii_case("clear")).then_some(value);

        let mut config = edgecrab_core::AppConfig::load().unwrap_or_default();
        match platform.as_str() {
            "telegram" => {
                config.gateway.telegram.enabled = true;
                config.gateway.enable_platform("telegram");
                config.gateway.telegram.home_channel = channel.clone();
            }
            "discord" => {
                config.gateway.discord.enabled = true;
                config.gateway.enable_platform("discord");
                config.gateway.discord.home_channel = channel.clone();
            }
            "slack" => {
                config.gateway.slack.enabled = true;
                config.gateway.enable_platform("slack");
                config.gateway.slack.home_channel = channel.clone();
            }
            _ => {
                return "Unsupported platform. Use one of: telegram, discord, slack".into();
            }
        }

        match config.save() {
            Ok(()) => match channel {
                Some(value) => format!("Home channel for {platform} set to {value}."),
                None => format!("Home channel for {platform} cleared."),
            },
            Err(error) => format!("Failed to save home channel for {platform}: {error}"),
        }
    }

    async fn handle_insights_command(&self, msg: &IncomingMessage, origin_chat_id: &str) -> String {
        let Ok(agent) = self
            .resolve_command_session_agent(msg, origin_chat_id)
            .await
        else {
            return "No agent configured.".into();
        };
        let days = msg
            .get_command_args()
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|days| *days > 0)
            .unwrap_or(30);
        let snapshot = agent.session_snapshot().await;
        let historical = match agent.state_db().await {
            Some(db) => db.query_insights(days).ok(),
            None => None,
        };
        format_gateway_insights(&snapshot, historical.as_ref())
    }

    /// Returns `true` if the user is authorized to use the gateway.
    ///
    /// Delegates to `auth::check_authorization()` which is the single source
    /// of truth for authorization decisions. See that module for the full rule set.
    ///
    /// Key difference from the old implementation: when no allowlists are configured,
    /// access is **denied** by default. Operators must explicitly set
    /// `GATEWAY_ALLOW_ALL_USERS=true` for open access.
    fn is_user_authorized(&self, msg: &IncomingMessage) -> auth::AuthResult {
        auth::check_authorization(
            msg.platform,
            &msg.user_id,
            msg.chat_type,
            self.config.group_policy,
            Some(&self.pairing_store),
        )
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
            group_policy = %self.config.group_policy,
            unauthorized_dm = ?self.config.unauthorized_dm_behavior,
            "starting gateway"
        );

        // ── Startup security posture audit log ───────────────────────────
        // Log per-platform security posture so operators can verify config
        // at startup.  This satisfies ADR-004 Finding 5 (visibility).
        for adapter in &self.adapters {
            let platform_id = format!("{:?}", adapter.platform()).to_lowercase();
            let posture = auth::analyze_platform_security(
                &platform_id,
                self.config.group_policy,
                Some(&self.pairing_store),
            );
            if posture.access_mode.is_secure() {
                tracing::info!(
                    platform = %posture.platform,
                    access = %posture.access_mode.label(),
                    groups = %posture.group_policy,
                    allowlisted = posture.allowlisted_count,
                    paired = posture.paired_count,
                    "platform security posture"
                );
            } else {
                tracing::warn!(
                    platform = %posture.platform,
                    access = %posture.access_mode.label(),
                    groups = %posture.group_policy,
                    allowlisted = posture.allowlisted_count,
                    paired = posture.paired_count,
                    "platform security posture — OPEN ACCESS"
                );
            }
            for w in &posture.warnings {
                tracing::warn!(platform = %posture.platform, "{w}");
            }
        }

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
            webhook_ingress: Arc::new(tokio::sync::Mutex::new(WebhookIngressState::default())),
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
                    // handling or agent dispatch.  Uses `auth::check_authorization()`
                    // for the full rule set — secure-by-default (denies when no
                    // allowlist configured).
                    let auth_result = self.is_user_authorized(&msg);
                    if auth_result.is_allowed() {
                        tracing::debug!(
                            platform = ?msg.platform,
                            user = %msg.user_id,
                            chat_type = ?msg.chat_type,
                            reason = ?auth_result,
                            "auth: access granted"
                        );
                    } else {
                        tracing::warn!(
                            platform = ?msg.platform,
                            user = %msg.user_id,
                            chat_type = ?msg.chat_type,
                            reason = ?auth_result,
                            "auth: access denied"
                        );
                        // Determine response based on unauthorized_dm_behavior config
                        let reply = auth::unauthorized_dm_response(
                            self.config.unauthorized_dm_behavior,
                            msg.chat_type,
                            msg.platform,
                            &msg.user_id,
                            &msg.user_id,
                            &self.pairing_store,
                        );
                        if let Some(text) = reply {
                            if let Some(adapter) = self
                                .adapters
                                .iter()
                                .find(|a| a.platform() == msg.platform)
                                .cloned()
                            {
                                let _ = adapter
                                    .send(crate::platform::OutgoingMessage {
                                        text,
                                        metadata: msg.metadata.clone(),
                                    })
                                    .await;
                            }
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
                                Some(gateway_help_text())
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
                                    let guard = self.running_sessions.lock().await;
                                    if let Some(token) = guard.get(&session_key) {
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
                                let cancelled = {
                                    let guard = self.running_sessions.lock().await;
                                    if let Some(token) = guard.get(&session_key) {
                                        token.cancel();
                                        true
                                    } else {
                                        false
                                    }
                                };
                                // Clear queued + retry state for this session.
                                {
                                    let mut pending = self.pending_messages.lock().await;
                                    pending.remove(&session_key);
                                }
                                {
                                    let mut last = self.last_messages.lock().await;
                                    last.remove(&session_key);
                                }
                                let sk = SessionKey::new(
                                    msg.platform,
                                    &msg.user_id,
                                    msg.channel_id.as_deref()
                                        .or(msg.metadata.channel_id.as_deref()),
                                );
                                let mut reset_deferred = false;
                                if cancelled {
                                    let deadline = tokio::time::Instant::now()
                                        + std::time::Duration::from_secs(2);
                                    while tokio::time::Instant::now() < deadline {
                                        let still_running = {
                                            let guard = self.running_sessions.lock().await;
                                            guard.contains_key(&session_key)
                                        };
                                        if !still_running {
                                            break;
                                        }
                                        tokio::time::sleep(std::time::Duration::from_millis(25))
                                            .await;
                                    }
                                    reset_deferred = {
                                        let guard = self.running_sessions.lock().await;
                                        guard.contains_key(&session_key)
                                    };
                                }

                                if reset_deferred {
                                    let mut pending_resets =
                                        self.pending_session_resets.lock().await;
                                    pending_resets.insert(session_key.clone());
                                } else if let Some(session) = self.session_manager.get(&sk) {
                                    let agent = {
                                        let mut guard = session.write().await;
                                        guard.touch();
                                        guard.agent.clone()
                                    };
                                    agent.new_session().await;
                                }
                                let _ = self.interaction_broker.cancel_session(&session_key).await;
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
                                            chat_type: msg.chat_type,
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
                            "undo" => {
                                if self.session_is_running(&session_key).await {
                                    Some("Stop the current response before using /undo.".into())
                                } else {
                                    match self
                                        .resolve_command_session_agent(&msg, &origin_chat_id)
                                        .await
                                    {
                                        Ok(agent) => {
                                            let removed = agent.undo_last_turn().await;
                                            Some(if removed == 0 {
                                                "Nothing to undo.".into()
                                            } else {
                                                format!("Undid the last turn ({removed} message(s) removed).")
                                            })
                                        }
                                        Err(error) => Some(error),
                                    }
                                }
                            }
                            "compress" => {
                                if self.session_is_running(&session_key).await {
                                    Some("Stop the current response before using /compress.".into())
                                } else {
                                    match self
                                        .resolve_command_session_agent(&msg, &origin_chat_id)
                                        .await
                                    {
                                        Ok(agent) => {
                                            let before = agent.session_snapshot().await.message_count;
                                            agent.force_compress().await;
                                            let after = agent.session_snapshot().await.message_count;
                                            Some(format!(
                                                "Compressed the current chat context ({before} -> {after} messages in live history)."
                                            ))
                                        }
                                        Err(error) => Some(error),
                                    }
                                }
                            }
                            "model" => Some(self.handle_model_command(&msg, &origin_chat_id).await),
                            "provider" => {
                                Some(self.handle_provider_command(&msg, &origin_chat_id).await)
                            }
                            "reasoning" => {
                                if self.session_is_running(&session_key).await {
                                    Some(
                                        "Stop the current response before changing reasoning effort."
                                            .into(),
                                    )
                                } else {
                                    Some(
                                        self.handle_reasoning_command(&msg, &origin_chat_id).await,
                                    )
                                }
                            }
                            "personality" => {
                                if self.session_is_running(&session_key).await {
                                    Some(
                                        "Stop the current response before changing personality."
                                            .into(),
                                    )
                                } else {
                                    Some(
                                        self.handle_personality_command(&msg, &origin_chat_id)
                                            .await,
                                    )
                                }
                            }
                            "title" => {
                                if self.session_is_running(&session_key).await {
                                    Some("Stop the current response before setting a title.".into())
                                } else {
                                    Some(self.handle_title_command(&msg, &origin_chat_id).await)
                                }
                            }
                            "resume" => {
                                if self.session_is_running(&session_key).await {
                                    Some("Stop the current response before resuming another session.".into())
                                } else {
                                    Some(self.handle_resume_command(&msg, &origin_chat_id).await)
                                }
                            }
                            "reload-mcp" | "reload_mcp" => {
                                edgecrab_tools::tools::mcp_client::reload_mcp_connections();
                                Some("MCP server connections cleared. They will reconnect on the next MCP tool call.".into())
                            }
                            "sethome" | "set-home" => {
                                Some(self.handle_sethome_command(&msg).await)
                            }
                            "insights" => {
                                Some(self.handle_insights_command(&msg, &origin_chat_id).await)
                            }
                            "voice" => {
                                Some(self.handle_voice_command(&msg, &origin_chat_id).await)
                            }
                            "commands" => {
                                let page = msg
                                    .get_command_args()
                                    .trim()
                                    .parse::<usize>()
                                    .ok()
                                    .filter(|page| *page > 0)
                                    .unwrap_or(1);
                                Some(gateway_commands_page(page))
                            }
                            "yolo" => {
                                let currently_enabled =
                                    edgecrab_tools::approval_runtime::yolo_enabled_for_session(
                                        &session_key,
                                    );
                                match msg
                                    .get_command_args()
                                    .trim()
                                    .to_ascii_lowercase()
                                    .as_str()
                                {
                                    "status" => Some(format!(
                                        "YOLO mode is {} for this chat session.",
                                        if currently_enabled { "ON" } else { "OFF" }
                                    )),
                                    "on" | "enable" | "enabled" => {
                                        edgecrab_tools::approval_runtime::set_yolo_for_session(
                                            &session_key,
                                            true,
                                        );
                                        Some("YOLO mode enabled for this chat session.".into())
                                    }
                                    "off" | "disable" | "disabled" => {
                                        edgecrab_tools::approval_runtime::set_yolo_for_session(
                                            &session_key,
                                            false,
                                        );
                                        Some("YOLO mode disabled for this chat session.".into())
                                    }
                                    "" | "toggle" => {
                                        edgecrab_tools::approval_runtime::set_yolo_for_session(
                                            &session_key,
                                            !currently_enabled,
                                        );
                                        Some(format!(
                                            "YOLO mode {} for this chat session.",
                                            if currently_enabled {
                                                "disabled"
                                            } else {
                                                "enabled"
                                            }
                                        ))
                                    }
                                    other => Some(format!(
                                        "Unknown yolo mode '{other}'. Use: /yolo [on|off|toggle|status]"
                                    )),
                                }
                            }
                            "rollback" => {
                                let args = msg.get_command_args().trim();
                                let prompt = match args {
                                    "" | "list" => {
                                        "Please list all available checkpoints by calling the checkpoint tool with action='list'.".to_string()
                                    }
                                    name => format!(
                                        "Please restore the checkpoint named '{}' by calling the checkpoint tool with action='restore', name='{}'.",
                                        name, name
                                    ),
                                };
                                let rollback_msg = IncomingMessage {
                                    platform: msg.platform,
                                    user_id: msg.user_id.clone(),
                                    channel_id: msg.channel_id.clone(),
                                    chat_type: msg.chat_type,
                                    text: prompt,
                                    thread_id: msg.thread_id.clone(),
                                    metadata: msg.metadata.clone(),
                                };
                                let _ = tx.send(rollback_msg).await;
                                Some(if args.is_empty() || args == "list" {
                                    "Listing checkpoints...".into()
                                } else {
                                    format!("Attempting to restore checkpoint '{args}'...")
                                })
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
                                        const BG_SUBAGENT_BATCH_SIZE: usize = 5;
                                        let background_agent = match agent
                                            .fork_isolated(IsolatedAgentOptions {
                                                session_id: Some(task_id_for_spawn.clone()),
                                                platform: Some(platform),
                                                quiet_mode: Some(true),
                                                origin_chat: Some(OriginChat::new(
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

                                        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
                                        let stream_task = tokio::spawn({
                                            let effective_text = effective_text.clone();
                                            let platform_name = platform_name.clone();
                                            let origin_chat_id_clone = origin_chat_id_clone.clone();
                                            async move {
                                                background_agent
                                                    .chat_streaming_with_origin(
                                                        &effective_text,
                                                        &platform_name,
                                                        &origin_chat_id_clone,
                                                        event_tx,
                                                    )
                                                    .await
                                            }
                                        });

                                        let mut response = String::new();
                                        let mut stream_error: Option<String> = None;
                                        let mut last_tool: Option<String> = None;
                                        let mut subagent_batches: HashMap<usize, Vec<String>> = HashMap::new();

                                        while let Some(event) = event_rx.recv().await {
                                            match event {
                                                edgecrab_core::StreamEvent::Token(text) => {
                                                    response.push_str(&text);
                                                }
                                                edgecrab_core::StreamEvent::ToolExec { name, args_json, .. } => {
                                                    let label = background_gateway_tool_label(&name, &args_json);
                                                    if last_tool.as_deref() != Some(label.as_str()) {
                                                        last_tool = Some(label.clone());
                                                        if let Some(adapter) = adapter.as_ref() {
                                                            let _ = adapter
                                                                .send_status(
                                                                    &format!(
                                                                        "🔧 {} {}",
                                                                        task_id_for_spawn, label
                                                                    ),
                                                                    &metadata,
                                                                )
                                                                .await;
                                                        }
                                                    }
                                                }
                                                edgecrab_core::StreamEvent::ToolProgress { name, message, .. } => {
                                                    let label = format!("{}: {}", name, message);
                                                    if last_tool.as_deref() != Some(label.as_str()) {
                                                        last_tool = Some(label.clone());
                                                        if let Some(adapter) = adapter.as_ref() {
                                                            let _ = adapter
                                                                .send_status(
                                                                    &format!(
                                                                        "🔧 {} {}",
                                                                        task_id_for_spawn, label
                                                                    ),
                                                                    &metadata,
                                                                )
                                                                .await;
                                                        }
                                                    }
                                                }
                                                edgecrab_core::StreamEvent::ToolDone {
                                                    name,
                                                    result_preview,
                                                    duration_ms,
                                                    is_error,
                                                    ..
                                                } => {
                                                    if is_error {
                                                        if let Some(adapter) = adapter.as_ref() {
                                                            let _ = adapter
                                                                .send_status(
                                                                    &format!(
                                                                        "❌ {} {} failed in {:.1}s",
                                                                        task_id_for_spawn,
                                                                        name,
                                                                        duration_ms as f64 / 1000.0
                                                                    ),
                                                                    &metadata,
                                                                )
                                                                .await;
                                                        }
                                                    } else if result_preview.as_deref().is_some_and(|preview| {
                                                        crate::event_processor::should_surface_tool_completion(
                                                            &name, preview,
                                                        )
                                                    }) {
                                                        if let Some(adapter) = adapter.as_ref() {
                                                            let _ = adapter
                                                                .send_status(
                                                                    &format!(
                                                                        "✅ {} {}",
                                                                        task_id_for_spawn,
                                                                        result_preview
                                                                            .as_deref()
                                                                            .unwrap_or_default()
                                                                    ),
                                                                    &metadata,
                                                                )
                                                                .await;
                                                        }
                                                    }
                                                }
                                                edgecrab_core::StreamEvent::SubAgentStart {
                                                    task_index,
                                                    task_count,
                                                    goal,
                                                } => {
                                                    if let Some(adapter) = adapter.as_ref() {
                                                        let _ = adapter
                                                            .send_status(
                                                                &format!(
                                                                    "🔀 {} [{}/{}] {}",
                                                                    task_id_for_spawn,
                                                                    task_index + 1,
                                                                    task_count,
                                                                    edgecrab_core::safe_truncate(&goal, 72)
                                                                ),
                                                                &metadata,
                                                            )
                                                            .await;
                                                    }
                                                }
                                                edgecrab_core::StreamEvent::SubAgentReasoning {
                                                    task_index,
                                                    task_count,
                                                    text,
                                                } => {
                                                    if let Some(adapter) = adapter.as_ref() {
                                                        let _ = adapter
                                                            .send_status(
                                                                &format!(
                                                                    "💭 {} [{}/{}] {}",
                                                                    task_id_for_spawn,
                                                                    task_index + 1,
                                                                    task_count,
                                                                    edgecrab_core::safe_truncate(&text, 72)
                                                                ),
                                                                &metadata,
                                                            )
                                                            .await;
                                                    }
                                                }
                                                edgecrab_core::StreamEvent::SubAgentToolExec {
                                                    task_index,
                                                    task_count,
                                                    name,
                                                    ..
                                                } => {
                                                    let batch = subagent_batches.entry(task_index).or_default();
                                                    batch.push(name);
                                                    if batch.len() >= BG_SUBAGENT_BATCH_SIZE {
                                                        let summary = batch.join(", ");
                                                        batch.clear();
                                                        if let Some(adapter) = adapter.as_ref() {
                                                            let _ = adapter
                                                                .send_status(
                                                                    &format!(
                                                                        "🔀 {} [{}/{}] {}",
                                                                        task_id_for_spawn,
                                                                        task_index + 1,
                                                                        task_count,
                                                                        summary
                                                                    ),
                                                                    &metadata,
                                                                )
                                                                .await;
                                                        }
                                                    }
                                                }
                                                edgecrab_core::StreamEvent::SubAgentFinish {
                                                    task_index,
                                                    task_count,
                                                    status,
                                                    duration_ms,
                                                    ..
                                                } => {
                                                    if let Some(batch) = subagent_batches.get_mut(&task_index) {
                                                        if !batch.is_empty() {
                                                            let summary = batch.join(", ");
                                                            batch.clear();
                                                            if let Some(adapter) = adapter.as_ref() {
                                                                let _ = adapter
                                                                    .send_status(
                                                                        &format!(
                                                                            "🔀 {} [{}/{}] {}",
                                                                            task_id_for_spawn,
                                                                            task_index + 1,
                                                                            task_count,
                                                                            summary
                                                                        ),
                                                                        &metadata,
                                                                    )
                                                                    .await;
                                                            }
                                                        }
                                                    }
                                                    if let Some(adapter) = adapter.as_ref() {
                                                        let _ = adapter
                                                            .send_status(
                                                                &format!(
                                                                    "{} {} [{}/{}] {} in {:.1}s",
                                                                    if status == "completed" { "✅" } else { "❌" },
                                                                    task_id_for_spawn,
                                                                    task_index + 1,
                                                                    task_count,
                                                                    status,
                                                                    duration_ms as f64 / 1000.0
                                                                ),
                                                                &metadata,
                                                            )
                                                            .await;
                                                    }
                                                }
                                                edgecrab_core::StreamEvent::Approval { response_tx, .. } => {
                                                    let _ = response_tx.send(edgecrab_core::ApprovalChoice::Deny);
                                                }
                                                edgecrab_core::StreamEvent::Clarify { response_tx, .. } => {
                                                    let _ = response_tx.send(String::new());
                                                }
                                                edgecrab_core::StreamEvent::SecretRequest { response_tx, .. } => {
                                                    let _ = response_tx.send(String::new());
                                                }
                                                edgecrab_core::StreamEvent::Error(err) => {
                                                    stream_error = Some(err);
                                                }
                                                edgecrab_core::StreamEvent::Done => break,
                                                _ => {}
                                            }
                                        }

                                        let response = match stream_task.await {
                                            Ok(Ok(())) if stream_error.is_none() => {
                                                let body = if response.trim().is_empty() {
                                                    "(No response generated)".to_string()
                                                } else {
                                                    response
                                                };
                                                format!(
                                                    "✅ Background task complete\nPrompt: \"{preview_for_spawn}\"\n\n{body}"
                                                )
                                            }
                                            Ok(Ok(())) => {
                                                format!(
                                                    "❌ Background task {task_id_for_spawn} failed: {}",
                                                    stream_error.unwrap_or_else(|| "background task failed".into())
                                                )
                                            }
                                            Ok(Err(e)) => {
                                                format!(
                                                    "❌ Background task {task_id_for_spawn} failed: {e}"
                                                )
                                            }
                                            Err(e) => {
                                                format!(
                                                    "❌ Background task {task_id_for_spawn} failed: background join error: {e}"
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
                    if let Some(ref base_agent) = self.agent {
                        let base_agent = base_agent.clone();
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

                        let session_lookup = SessionKey::new(
                            msg.platform,
                            &msg.user_id,
                            msg.channel_id.as_deref().or(msg.metadata.channel_id.as_deref()),
                        );
                        let session = self
                            .session_manager
                            .resolve(
                                &session_lookup,
                                &base_agent,
                                OriginChat::new(
                                    msg.platform.to_string(),
                                    origin_chat_id_for_key.clone(),
                                ),
                            )
                            .await;
                        let session: Arc<tokio::sync::RwLock<crate::session::GatewaySession>> =
                            match session {
                            Ok(session) => session,
                            Err(error) => {
                                let mut running = self.running_sessions.lock().await;
                                running.remove(&session_key);
                                return Err(anyhow::anyhow!("{error}"));
                            }
                        };
                        let session_agent = {
                            let mut guard = session.write().await;
                            guard.touch();
                            guard.agent.clone()
                        };

                        let running_sessions = self.running_sessions.clone();
                        let pending_messages = self.pending_messages.clone();
                        let pending_session_resets = self.pending_session_resets.clone();
                        let task_session_key = session_key.clone();
                        let gateway_voice_mode = self.voice_mode_for(&origin_chat_id_for_key).await;
                        // Clone the sender so the task can re-dispatch the pending message.
                        let msg_tx = tx.clone();
                        let hook_registry_for_spawn = self.hook_registry.clone();
                        let interaction_broker = self.interaction_broker.clone();
                        // The token is registered in running_sessions; drop the local copy.
                        // /stop cancels the map-held token via running_sessions.remove().cancel().
                        drop(task_cancel);

                        tokio::spawn(async move {
                            let task_outcome = AssertUnwindSafe(async {
                                // Resolve the origin chat_id: prefer channel_id (group/channel),
                                // fall back to user_id (DM).  This is what deliver='origin' uses
                                // to route cron job output back to the correct chat.
                                let origin_chat_id = message_origin_recipient(&msg_clone);
                                let platform_name = msg_clone.platform.to_string();

                                if !msg_clone.metadata.preloaded_skills.is_empty() {
                                    session_agent
                                        .set_preloaded_skills(msg_clone.metadata.preloaded_skills.clone())
                                        .await;
                                }

                                // Enrich the prompt with image attachment instructions.
                                // WHY here: This is the single gateway dispatch point covering
                                // ALL platforms (WhatsApp, Telegram, Slack, Signal, …). Injecting
                                // the *** ATTACHED IMAGES *** block here means every platform
                                // triggers the VISION_GUIDANCE rules in the system prompt,
                                // identical to the CLI pending_images path in app.rs.
                                let effective_text =
                                    build_effective_text(&session_agent, &msg_clone).await;

                                // NOTE: chat_streaming_with_origin() handles origin context
                                // internally, so we do NOT pre-set it here.
                                let voice_adapter = origin_adapter.clone();
                                let response_result = match origin_adapter {
                                    Some(adapter_arc) => {
                                        dispatch_streaming_arc(
                                            &session_agent,
                                            &effective_text,
                                            msg_clone.platform,
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
                                    None => match session_agent
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
                                    },
                                };

                                match response_result {
                                    Ok(response_len) => {
                                        let response_text: Option<String> = session_agent
                                            .messages()
                                            .await
                                            .iter()
                                            .rev()
                                            .find(|message| message.role == Role::Assistant)
                                            .map(|message| message.text_content())
                                            .filter(|text| !text.trim().is_empty());
                                        if let Some(response_text) = response_text {
                                            maybe_send_voice_reply(
                                                &session_agent,
                                                voice_adapter,
                                                &msg_clone,
                                                &response_text,
                                                gateway_voice_mode,
                                            )
                                            .await;
                                        }
                                        tracing::info!(
                                            platform = ?msg_clone.platform,
                                            user = %msg_clone.user_id,
                                            response_len,
                                            "agent response delivered"
                                        );
                                        hooks.emit(
                                            "agent:done",
                                            &HookContext::new("agent:done")
                                                .with_user(&msg_clone.user_id),
                                        )
                                        .await;
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            error = %e,
                                            platform = ?msg_clone.platform,
                                            "agent dispatch failed"
                                        );
                                    }
                                }
                            })
                            .catch_unwind()
                            .await;

                            if let Err(panic_payload) = task_outcome {
                                tracing::error!(
                                    session = %task_session_key,
                                    panic = %panic_payload_message(&*panic_payload),
                                    "gateway session task panicked"
                                );
                            }

                            let reset_requested = {
                                let mut pending = pending_session_resets.lock().await;
                                pending.remove(&task_session_key)
                            };
                            if reset_requested {
                                session_agent.new_session().await;
                            }
                            let _ = interaction_broker.cancel_session(&task_session_key).await;

                            // Keep the session marked as running until all
                            // deferred cleanup is complete so shutdown cannot
                            // finalize the session while a reset is in flight.
                            {
                                let mut guard = running_sessions.lock().await;
                                guard.remove(&task_session_key);
                            }

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

        let session_cancels = {
            let running = self.running_sessions.lock().await;
            running.values().cloned().collect::<Vec<_>>()
        };
        for token in session_cancels {
            token.cancel();
        }
        let wait_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        while tokio::time::Instant::now() < wait_deadline {
            if self.running_sessions.lock().await.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        if !self.running_sessions.lock().await.is_empty() {
            tracing::warn!("gateway shutdown timed out waiting for running sessions to drain");
        }
        self.session_manager.finalize_all().await;
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
    platform: edgecrab_types::Platform,
    platform_name: &str,
    origin_chat_id: &str,
    adapter: Arc<dyn PlatformAdapter>,
    metadata: crate::platform::MessageMetadata,
    cfg: crate::config::GatewayStreamingConfig,
    hook_registry: Arc<HookRegistry>,
    interaction_broker: Arc<InteractionBroker>,
    session_key: String,
) -> anyhow::Result<usize> {
    let baseline_len = agent.messages().await.len();
    let fallback_adapter = Arc::clone(&adapter);
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

    let final_messages = agent.messages().await;
    if let Some(response) = extract_stream_fallback_response(&final_messages, baseline_len) {
        let mut delivery = DeliveryRouter::new();
        delivery.register(Arc::clone(&fallback_adapter));
        return deliver_text_and_media(
            &delivery,
            Some(fallback_adapter),
            &response,
            platform,
            &metadata,
        )
        .await;
    }

    anyhow::bail!("gateway streaming completed without delivering a response")
}

/// Build the axum router with health and webhook endpoints.
fn build_router(state: GatewayState) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/webhook/incoming", post(webhook_incoming))
        .route("/webhooks/:name", post(webhook_subscription_incoming))
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

/// Dynamic webhook endpoint backed by ~/.edgecrab/webhook_subscriptions.json.
async fn webhook_subscription_incoming(
    State(state): State<GatewayState>,
    AxumPath(name): AxumPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<serde_json::Value>) {
    let subscriptions = match load_subscriptions() {
        Ok(subscriptions) => subscriptions,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"status": "error", "message": err.to_string()})),
            );
        }
    };
    let Some(subscription) = subscriptions.get(&name) else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"status": "error", "message": "subscription not found"})),
        );
    };

    let body_limit = if subscription.max_body_bytes == 0 {
        default_max_body_bytes()
    } else {
        subscription.max_body_bytes
    };
    if body.len() > body_limit {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({
                "status": "error",
                "message": "payload exceeds configured body limit",
                "route": name,
            })),
        );
    }

    let payload = String::from_utf8_lossy(&body).to_string();
    let provided_signature = headers
        .get("X-Hub-Signature-256")
        .or_else(|| headers.get("X-Webhook-Signature"))
        .or_else(|| headers.get("X-Gitlab-Token"))
        .or_else(|| headers.get("X-Webhook-Secret"))
        .and_then(|value| value.to_str().ok());
    if !verify_signature(&subscription.secret, &payload, provided_signature) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"status": "error", "message": "invalid webhook signature"})),
        );
    }

    let event = headers
        .get("X-Event-Type")
        .or_else(|| headers.get("X-GitHub-Event"))
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            serde_json::from_str::<serde_json::Value>(&payload)
                .ok()
                .and_then(|value| {
                    value
                        .get("event_type")
                        .and_then(|value| value.as_str())
                        .map(str::to_string)
                })
        });

    if !subscription.accepts_event(event.as_deref()) {
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "ignored",
                "route": name,
                "reason": "event filtered",
            })),
        );
    }

    let delivery_id = headers
        .get("X-GitHub-Delivery")
        .or_else(|| headers.get("X-Delivery-Id"))
        .or_else(|| headers.get("X-Request-Id"))
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    {
        let now = Instant::now();
        let mut ingress = state.webhook_ingress.lock().await;
        let ttl = Duration::from_secs(3600);
        ingress.prune(now, ttl);
        let delivery_key = format!("{name}:{delivery_id}");
        if ingress.is_duplicate(&delivery_key, now, ttl) {
            return (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "duplicate",
                    "route": name,
                    "delivery_id": delivery_id,
                })),
            );
        }
        let rate_limit = if subscription.rate_limit_per_minute == 0 {
            default_rate_limit_per_minute()
        } else {
            subscription.rate_limit_per_minute
        };
        if ingress.exceeds_rate_limit(&name, rate_limit, now) {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({
                    "status": "error",
                    "message": "rate limit exceeded",
                    "route": name,
                })),
            );
        }
    }

    let text = render_dynamic_webhook_prompt(
        &name,
        subscription.prompt.as_str(),
        &payload,
        event.as_deref(),
    );
    let deliver_extra = render_delivery_extra(&subscription.deliver_extra, &payload);
    let message = IncomingMessage {
        platform: edgecrab_types::Platform::Webhook,
        user_id: format!("webhook:{name}:{delivery_id}"),
        channel_id: Some(name.clone()),
        chat_type: crate::platform::ChatType::Channel,
        text,
        thread_id: None,
        metadata: crate::platform::MessageMetadata {
            webhook_delivery: Some(WebhookDelivery {
                deliver: subscription.deliver.clone(),
                deliver_extra,
            }),
            preloaded_skills: subscription.skills.clone(),
            ..Default::default()
        },
    };

    match state.message_tx.send(message).await {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "status": "accepted",
                "route": name,
                "event": event,
                "delivery_id": delivery_id,
            })),
        ),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"status": "error", "message": "gateway channel full"})),
        ),
    }
}

fn render_dynamic_webhook_prompt(
    name: &str,
    prompt: &str,
    payload: &str,
    event: Option<&str>,
) -> String {
    let payload_json = serde_json::from_str::<serde_json::Value>(payload).ok();
    let rendered_prompt =
        render_webhook_template(prompt.trim(), payload_json.as_ref(), event, name);
    if !rendered_prompt.trim().is_empty() {
        return rendered_prompt;
    }

    let pretty_payload = payload_json
        .as_ref()
        .and_then(|value| serde_json::to_string_pretty(value).ok())
        .unwrap_or_else(|| payload.to_string());

    let mut text = String::new();
    text.push_str(&format!("Webhook subscription: {name}\n"));
    if let Some(event) = event {
        text.push_str(&format!("Event: {event}\n"));
    }
    text.push_str("Payload:\n");
    text.push_str(&pretty_payload);
    text
}

fn render_delivery_extra(
    extra: &BTreeMap<String, String>,
    payload: &str,
) -> BTreeMap<String, String> {
    let payload_json = serde_json::from_str::<serde_json::Value>(payload).ok();
    extra
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                render_webhook_template(value, payload_json.as_ref(), None, "")
                    .trim()
                    .to_string(),
            )
        })
        .collect()
}

fn render_webhook_template(
    template: &str,
    payload: Option<&serde_json::Value>,
    event: Option<&str>,
    route: &str,
) -> String {
    if template.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let key = after[..end].trim();
        if let Some(value) = resolve_webhook_template_value(key, payload, event, route) {
            out.push_str(&value);
        } else {
            out.push('{');
            out.push_str(key);
            out.push('}');
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn resolve_webhook_template_value(
    key: &str,
    payload: Option<&serde_json::Value>,
    event: Option<&str>,
    route: &str,
) -> Option<String> {
    match key {
        "__raw__" => payload
            .and_then(|value| serde_json::to_string_pretty(value).ok())
            .map(|raw| edgecrab_core::safe_truncate(&raw, 4000).to_string()),
        "event" => event.map(str::to_string),
        "route" => Some(route.to_string()),
        _ => payload
            .and_then(|value| lookup_webhook_template_path(value, key))
            .map(webhook_value_to_string),
    }
}

fn lookup_webhook_template_path<'a>(
    value: &'a serde_json::Value,
    path: &str,
) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for segment in path.split('.') {
        current = match current {
            serde_json::Value::Object(map) => map.get(segment)?,
            serde_json::Value::Array(items) => items.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

fn webhook_value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".into(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => value.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use edgecrab_core::{AgentBuilder, AppConfig};
    use edgequake_llm::traits::{
        ChatMessage, ChatRole, CompletionOptions, ToolChoice, ToolDefinition,
    };
    use std::sync::Mutex;
    use tempfile::TempDir;
    use tokio::sync::Notify;
    use tokio::sync::mpsc;

    #[derive(Default)]
    struct RecordingAdapter {
        sent: Mutex<Vec<String>>,
        photos: Mutex<Vec<String>>,
        documents: Mutex<Vec<String>>,
        voices: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl PlatformAdapter for RecordingAdapter {
        fn platform(&self) -> edgecrab_types::Platform {
            edgecrab_types::Platform::Webhook
        }

        async fn start(&self, _tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
            Ok(())
        }

        async fn send(&self, msg: crate::platform::OutgoingMessage) -> anyhow::Result<()> {
            self.sent.lock().expect("sent lock").push(msg.text);
            Ok(())
        }

        fn format_response(
            &self,
            text: &str,
            _metadata: &crate::platform::MessageMetadata,
        ) -> String {
            text.to_string()
        }

        fn max_message_length(&self) -> usize {
            4096
        }

        fn supports_markdown(&self) -> bool {
            false
        }

        fn supports_images(&self) -> bool {
            true
        }

        fn supports_files(&self) -> bool {
            true
        }

        async fn send_photo(
            &self,
            path: &str,
            _caption: Option<&str>,
            _metadata: &crate::platform::MessageMetadata,
        ) -> anyhow::Result<()> {
            self.photos
                .lock()
                .expect("photos lock")
                .push(path.to_string());
            Ok(())
        }

        async fn send_document(
            &self,
            path: &str,
            _caption: Option<&str>,
            _metadata: &crate::platform::MessageMetadata,
        ) -> anyhow::Result<()> {
            self.documents
                .lock()
                .expect("documents lock")
                .push(path.to_string());
            Ok(())
        }

        async fn send_voice(
            &self,
            path: &str,
            _caption: Option<&str>,
            _metadata: &crate::platform::MessageMetadata,
        ) -> anyhow::Result<()> {
            self.voices
                .lock()
                .expect("voices lock")
                .push(path.to_string());
            Ok(())
        }
    }

    struct ScriptedAdapter {
        incoming: Mutex<Vec<IncomingMessage>>,
        sent: Mutex<Vec<String>>,
        notify: Notify,
        delay: std::time::Duration,
    }

    impl ScriptedAdapter {
        fn with_delay(incoming: Vec<IncomingMessage>, delay: std::time::Duration) -> Self {
            Self {
                incoming: Mutex::new(incoming),
                sent: Mutex::new(Vec::new()),
                notify: Notify::new(),
                delay,
            }
        }

        async fn wait_for_sent(&self, count: usize) -> Vec<String> {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                let current = self.sent.lock().expect("sent lock").clone();
                if current.len() >= count {
                    return current;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "timed out waiting for {count} sent messages, got {}",
                    current.len()
                );
                self.notify.notified().await;
            }
        }
    }

    #[async_trait]
    impl PlatformAdapter for ScriptedAdapter {
        fn platform(&self) -> edgecrab_types::Platform {
            edgecrab_types::Platform::Webhook
        }

        async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
            let scripted = self
                .incoming
                .lock()
                .expect("incoming lock")
                .drain(..)
                .collect::<Vec<_>>();
            let scripted_len = scripted.len();
            for (index, message) in scripted.into_iter().enumerate() {
                tx.send(message).await.expect("send scripted message");
                if !self.delay.is_zero() && index + 1 < scripted_len {
                    tokio::time::sleep(self.delay).await;
                }
            }
            futures::future::pending::<()>().await;
            Ok(())
        }

        async fn send(&self, msg: crate::platform::OutgoingMessage) -> anyhow::Result<()> {
            self.sent.lock().expect("sent lock").push(msg.text);
            self.notify.notify_waiters();
            Ok(())
        }

        fn format_response(
            &self,
            text: &str,
            _metadata: &crate::platform::MessageMetadata,
        ) -> String {
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

    struct HistoryEchoProvider;

    #[async_trait]
    impl edgequake_llm::LLMProvider for HistoryEchoProvider {
        fn name(&self) -> &str {
            "history-echo"
        }

        fn model(&self) -> &str {
            "history-echo/mock"
        }

        fn max_context_length(&self) -> usize {
            8192
        }

        async fn complete(
            &self,
            prompt: &str,
        ) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
            Ok(edgequake_llm::LLMResponse::new(prompt, self.model()))
        }

        async fn complete_with_options(
            &self,
            prompt: &str,
            _options: &CompletionOptions,
        ) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
            self.complete(prompt).await
        }

        async fn chat(
            &self,
            messages: &[ChatMessage],
            options: Option<&CompletionOptions>,
        ) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
            self.chat_with_tools(messages, &[], None, options).await
        }

        async fn chat_with_tools(
            &self,
            messages: &[ChatMessage],
            _tools: &[ToolDefinition],
            _tool_choice: Option<ToolChoice>,
            _options: Option<&CompletionOptions>,
        ) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
            let user_messages = messages
                .iter()
                .filter(|message| message.role == ChatRole::User)
                .map(|message| message.content.clone())
                .collect::<Vec<_>>();
            let current = user_messages.last().cloned().unwrap_or_default();
            let prior = user_messages.len().saturating_sub(1);
            let first_prior = user_messages
                .first()
                .filter(|_| prior > 0)
                .cloned()
                .unwrap_or_else(|| "-".into());
            Ok(edgequake_llm::LLMResponse::new(
                format!("current={current};prior_users={prior};first_prior={first_prior}"),
                self.model(),
            ))
        }
    }

    fn write_gateway_session_hook_plugin(dir: &std::path::Path) {
        std::fs::write(
            dir.join("plugin.yaml"),
            r#"
name: gateway-session-hooks
version: "1.0.0"
description: Gateway session hook recorder
provides_hooks:
  - on_session_start
  - on_session_end
  - on_session_finalize
  - on_session_reset
"#,
        )
        .expect("write plugin manifest");
        std::fs::write(
            dir.join("__init__.py"),
            r#"
import json
from pathlib import Path

def _append(event_name, session_id, platform, **kwargs):
    target = Path(__file__).with_name("gateway-session-hooks.jsonl")
    with target.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps({"event": event_name, "session_id": session_id, "platform": platform}) + "\n")

def register(ctx):
    ctx.register_hook("on_session_start", lambda **kwargs: _append("on_session_start", **kwargs))
    ctx.register_hook("on_session_end", lambda **kwargs: _append("on_session_end", **kwargs))
    ctx.register_hook("on_session_finalize", lambda **kwargs: _append("on_session_finalize", **kwargs))
    ctx.register_hook("on_session_reset", lambda **kwargs: _append("on_session_reset", **kwargs))
"#,
        )
        .expect("write plugin");
    }

    fn webhook_message(user_id: &str, text: &str) -> IncomingMessage {
        IncomingMessage {
            platform: edgecrab_types::Platform::Webhook,
            user_id: user_id.to_string(),
            channel_id: None,
            chat_type: crate::platform::ChatType::Dm,
            text: text.to_string(),
            thread_id: None,
            metadata: crate::platform::MessageMetadata::default(),
        }
    }

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
            chat_type: crate::platform::ChatType::Dm,
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
    fn audio_attachment_sources_only_return_local_audio_and_voice_paths() {
        let msg = IncomingMessage {
            platform: edgecrab_types::Platform::Signal,
            user_id: "u1".into(),
            channel_id: None,
            chat_type: crate::platform::ChatType::Dm,
            text: "transcribe".into(),
            thread_id: None,
            metadata: crate::platform::MessageMetadata {
                attachments: vec![
                    crate::platform::MessageAttachment {
                        kind: crate::platform::MessageAttachmentKind::Voice,
                        local_path: Some("/tmp/voice.ogg".into()),
                        ..Default::default()
                    },
                    crate::platform::MessageAttachment {
                        kind: crate::platform::MessageAttachmentKind::Audio,
                        local_path: Some("/tmp/audio.mp3".into()),
                        ..Default::default()
                    },
                    crate::platform::MessageAttachment {
                        kind: crate::platform::MessageAttachmentKind::Document,
                        local_path: Some("/tmp/report.pdf".into()),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        };

        let sources = audio_attachment_sources(&msg);
        assert_eq!(sources, vec!["/tmp/voice.ogg", "/tmp/audio.mp3"]);
    }

    #[test]
    fn incoming_message_is_voice_origin_for_voice_note() {
        let msg = IncomingMessage {
            platform: edgecrab_types::Platform::Telegram,
            user_id: "u1".into(),
            channel_id: Some("chat".into()),
            chat_type: crate::platform::ChatType::Dm,
            text: String::new(),
            thread_id: None,
            metadata: crate::platform::MessageMetadata {
                attachments: vec![crate::platform::MessageAttachment {
                    kind: crate::platform::MessageAttachmentKind::Voice,
                    local_path: Some("/tmp/voice.ogg".into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
        };

        assert!(incoming_message_is_voice_origin(&msg));
    }

    #[test]
    fn incoming_message_is_voice_origin_for_empty_text_audio() {
        let msg = IncomingMessage {
            platform: edgecrab_types::Platform::Whatsapp,
            user_id: "u1".into(),
            channel_id: Some("chat".into()),
            chat_type: crate::platform::ChatType::Dm,
            text: String::new(),
            thread_id: None,
            metadata: crate::platform::MessageMetadata {
                attachments: vec![crate::platform::MessageAttachment {
                    kind: crate::platform::MessageAttachmentKind::Audio,
                    local_path: Some("/tmp/audio.ogg".into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
        };

        assert!(incoming_message_is_voice_origin(&msg));
    }

    #[test]
    fn load_gateway_voice_modes_filters_invalid_values() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("gateway_voice_mode.json");
        std::fs::write(
            &path,
            r#"{"chat-a":"voice_only","chat-b":"all","chat-c":"broken"}"#,
        )
        .expect("write");

        let loaded = load_gateway_voice_modes_from(&path);
        assert_eq!(loaded.get("chat-a"), Some(&GatewayVoiceMode::VoiceOnly));
        assert_eq!(loaded.get("chat-b"), Some(&GatewayVoiceMode::All));
        assert!(!loaded.contains_key("chat-c"));
    }

    #[test]
    fn save_gateway_voice_modes_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("gateway_voice_mode.json");
        let modes = HashMap::from([
            ("chat-a".to_string(), GatewayVoiceMode::Off),
            ("chat-b".to_string(), GatewayVoiceMode::All),
        ]);

        save_gateway_voice_modes_to(&path, &modes).expect("save");
        let loaded = load_gateway_voice_modes_from(&path);

        assert_eq!(loaded, modes);
    }

    #[test]
    fn extract_audio_path_from_tts_output_supports_media_and_legacy_output() {
        assert_eq!(
            edgecrab_tools::tools::tts::extract_audio_path_from_tts_output(
                "Generated audio.\nMEDIA:/tmp/reply.mp3"
            ),
            Some("/tmp/reply.mp3".into())
        );
        assert_eq!(
            edgecrab_tools::tools::tts::extract_audio_path_from_tts_output(
                "Audio saved to: /tmp/reply.mp3"
            ),
            Some("/tmp/reply.mp3".into())
        );
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

    #[test]
    fn render_gateway_audio_context_includes_transcripts() {
        let rendered = render_gateway_audio_context(
            "Reply to this",
            &[String::from("/tmp/voice.ogg")],
            &[String::from("Transcript (via local):\nhello from voice")],
            &[],
            0,
        );

        assert!(rendered.contains("ATTACHED AUDIO"));
        assert!(rendered.contains("/tmp/voice.ogg"));
        assert!(rendered.contains("hello from voice"));
    }

    #[test]
    fn background_gateway_tool_label_prefers_meaningful_arg_preview() {
        let label = background_gateway_tool_label(
            "terminal",
            r#"{"command":"cargo test -p edgecrab-core"}"#,
        );
        assert!(label.contains("terminal:"));
        assert!(label.contains("cargo test"));
    }

    #[test]
    fn background_gateway_arg_preview_handles_invalid_json() {
        assert!(background_gateway_arg_preview("{not-json").is_empty());
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
            webhook_ingress: Arc::new(tokio::sync::Mutex::new(WebhookIngressState::default())),
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
            webhook_ingress: Arc::new(tokio::sync::Mutex::new(WebhookIngressState::default())),
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

    #[tokio::test]
    #[serial_test::serial(edgecrab_gateway_env)]
    async fn dynamic_webhook_duplicate_delivery_returns_duplicate() {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join(".edgecrab");
        unsafe {
            std::env::set_var("EDGECRAB_HOME", &home);
        }
        let mut subscriptions = std::collections::BTreeMap::new();
        let subscription = crate::webhook_subscriptions::create_subscription(
            crate::webhook_subscriptions::CreateSubscriptionParams {
                name: "github",
                description: None,
                events: &[],
                prompt: Some("PR {action}"),
                skills: &[],
                secret: crate::webhook_subscriptions::insecure_no_auth_secret().to_string(),
                deliver: None,
                deliver_extra: std::collections::BTreeMap::new(),
                rate_limit_per_minute: Some(30),
                max_body_bytes: Some(1_048_576),
            },
        )
        .expect("subscription");
        subscriptions.insert(subscription.name.clone(), subscription);
        crate::webhook_subscriptions::save_subscriptions(&subscriptions)
            .expect("save subscriptions");
        assert!(
            crate::webhook_subscriptions::load_subscriptions()
                .expect("load subscriptions")
                .contains_key("github")
        );

        let (tx, mut rx) = mpsc::channel(16);
        let state = GatewayState {
            session_manager: Arc::new(SessionManager::new(std::time::Duration::from_secs(3600))),
            hook_registry: Arc::new(HookRegistry::new()),
            message_tx: tx,
            cancel: CancellationToken::new(),
            webhook_ingress: Arc::new(tokio::sync::Mutex::new(WebhookIngressState::default())),
        };
        let app = build_router(state);

        let request = || {
            Request::builder()
                .method("POST")
                .uri("/webhooks/github")
                .header("content-type", "application/json")
                .header("X-GitHub-Event", "pull_request")
                .header("X-GitHub-Delivery", "delivery-123")
                .body(Body::from(r#"{"action":"opened"}"#))
                .expect("request")
        };

        let first = app
            .clone()
            .oneshot(request())
            .await
            .expect("first response");
        assert_eq!(first.status(), StatusCode::ACCEPTED);
        let msg = rx.try_recv().expect("queued message");
        assert_eq!(msg.user_id, "webhook:github:delivery-123");

        let second = app.oneshot(request()).await.expect("second response");
        assert_eq!(second.status(), StatusCode::OK);

        unsafe {
            std::env::remove_var("EDGECRAB_HOME");
        }
    }

    #[tokio::test]
    #[serial_test::serial(edgecrab_gateway_env)]
    async fn dynamic_webhook_rate_limit_and_body_limit_are_enforced() {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join(".edgecrab");
        unsafe {
            std::env::set_var("EDGECRAB_HOME", &home);
        }
        let mut subscriptions = std::collections::BTreeMap::new();
        let subscription = crate::webhook_subscriptions::create_subscription(
            crate::webhook_subscriptions::CreateSubscriptionParams {
                name: "limited",
                description: None,
                events: &[],
                prompt: Some("Event {event}"),
                skills: &[],
                secret: crate::webhook_subscriptions::insecure_no_auth_secret().to_string(),
                deliver: None,
                deliver_extra: std::collections::BTreeMap::new(),
                rate_limit_per_minute: Some(1),
                max_body_bytes: Some(32),
            },
        )
        .expect("subscription");
        subscriptions.insert(subscription.name.clone(), subscription);
        crate::webhook_subscriptions::save_subscriptions(&subscriptions)
            .expect("save subscriptions");
        assert!(
            crate::webhook_subscriptions::load_subscriptions()
                .expect("load subscriptions")
                .contains_key("limited")
        );

        let (tx, _rx) = mpsc::channel(16);
        let state = GatewayState {
            session_manager: Arc::new(SessionManager::new(std::time::Duration::from_secs(3600))),
            hook_registry: Arc::new(HookRegistry::new()),
            message_tx: tx,
            cancel: CancellationToken::new(),
            webhook_ingress: Arc::new(tokio::sync::Mutex::new(WebhookIngressState::default())),
        };
        let app = build_router(state);

        let accepted = Request::builder()
            .method("POST")
            .uri("/webhooks/limited")
            .header("content-type", "application/json")
            .header("X-GitHub-Delivery", "delivery-a")
            .body(Body::from(r#"{"ok":true}"#))
            .expect("request");
        assert_eq!(
            app.clone()
                .oneshot(accepted)
                .await
                .expect("accepted")
                .status(),
            StatusCode::ACCEPTED
        );

        let rate_limited = Request::builder()
            .method("POST")
            .uri("/webhooks/limited")
            .header("content-type", "application/json")
            .header("X-GitHub-Delivery", "delivery-b")
            .body(Body::from(r#"{"ok":true}"#))
            .expect("request");
        assert_eq!(
            app.clone()
                .oneshot(rate_limited)
                .await
                .expect("rate limited")
                .status(),
            StatusCode::TOO_MANY_REQUESTS
        );

        let oversized = Request::builder()
            .method("POST")
            .uri("/webhooks/limited")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"data":"this payload is longer than thirty-two bytes"}"#,
            ))
            .expect("request");
        assert_eq!(
            app.oneshot(oversized).await.expect("oversized").status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );

        unsafe {
            std::env::remove_var("EDGECRAB_HOME");
        }
    }

    #[test]
    fn render_dynamic_webhook_prompt_supports_dot_paths_and_raw_payload() {
        let payload = r#"{"action":"opened","pull_request":{"number":42,"title":"Fix auth"}}"#;
        let rendered = render_dynamic_webhook_prompt(
            "github",
            "PR #{pull_request.number}: {pull_request.title}\nEvent={event}\nRaw={__raw__}",
            payload,
            Some("pull_request"),
        );
        assert!(rendered.contains("PR #42: Fix auth"));
        assert!(rendered.contains("Event=pull_request"));
        assert!(rendered.contains("\"title\": \"Fix auth\""));
    }

    #[test]
    fn render_delivery_extra_supports_payload_templates() {
        let payload = r#"{"repository":{"full_name":"org/repo"},"pull_request":{"number":42}}"#;
        let extra = BTreeMap::from([
            ("repo".to_string(), "{repository.full_name}".to_string()),
            ("pr_number".to_string(), "{pull_request.number}".to_string()),
        ]);
        let rendered = render_delivery_extra(&extra, payload);
        assert_eq!(rendered.get("repo").map(String::as_str), Some("org/repo"));
        assert_eq!(rendered.get("pr_number").map(String::as_str), Some("42"));
    }

    #[tokio::test]
    #[serial_test::serial(edgecrab_gateway_env)]
    async fn dynamic_webhook_attaches_skills_and_delivery_metadata() {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join(".edgecrab");
        unsafe {
            std::env::set_var("EDGECRAB_HOME", &home);
        }

        let mut subscriptions = std::collections::BTreeMap::new();
        let deliver_extra = BTreeMap::from([
            ("repo".to_string(), "{repository.full_name}".to_string()),
            ("pr_number".to_string(), "{pull_request.number}".to_string()),
        ]);
        let subscription = crate::webhook_subscriptions::create_subscription(
            crate::webhook_subscriptions::CreateSubscriptionParams {
                name: "github-comment",
                description: None,
                events: &[],
                prompt: Some("Review PR #{pull_request.number}"),
                skills: &["code-review".to_string()],
                secret: crate::webhook_subscriptions::insecure_no_auth_secret().to_string(),
                deliver: Some("github_comment"),
                deliver_extra,
                rate_limit_per_minute: Some(30),
                max_body_bytes: Some(1_048_576),
            },
        )
        .expect("subscription");
        subscriptions.insert(subscription.name.clone(), subscription);
        crate::webhook_subscriptions::save_subscriptions(&subscriptions)
            .expect("save subscriptions");

        let (tx, mut rx) = mpsc::channel(16);
        let state = GatewayState {
            session_manager: Arc::new(SessionManager::new(std::time::Duration::from_secs(3600))),
            hook_registry: Arc::new(HookRegistry::new()),
            message_tx: tx,
            cancel: CancellationToken::new(),
            webhook_ingress: Arc::new(tokio::sync::Mutex::new(WebhookIngressState::default())),
        };
        let app = build_router(state);

        let request = Request::builder()
            .method("POST")
            .uri("/webhooks/github-comment")
            .header("content-type", "application/json")
            .header("X-GitHub-Delivery", "delivery-456")
            .body(Body::from(
                r#"{"repository":{"full_name":"org/repo"},"pull_request":{"number":42}}"#,
            ))
            .expect("request");

        let response = app.oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::ACCEPTED);

        let msg = rx.try_recv().expect("queued message");
        assert_eq!(msg.metadata.preloaded_skills, vec!["code-review"]);
        let delivery = msg
            .metadata
            .webhook_delivery
            .as_ref()
            .expect("webhook delivery metadata");
        assert_eq!(delivery.deliver, "github_comment");
        assert_eq!(
            delivery.deliver_extra.get("repo").map(String::as_str),
            Some("org/repo")
        );
        assert_eq!(
            delivery.deliver_extra.get("pr_number").map(String::as_str),
            Some("42")
        );

        unsafe {
            std::env::remove_var("EDGECRAB_HOME");
        }
    }

    // ── Authorization tests ───────────────────────────────────────────────

    fn make_msg(user_id: &str, platform: edgecrab_types::Platform) -> IncomingMessage {
        IncomingMessage {
            platform,
            user_id: user_id.to_string(),
            channel_id: None,
            chat_type: crate::platform::ChatType::Dm,
            text: "hello".to_string(),
            thread_id: None,
            metadata: crate::platform::MessageMetadata::default(),
        }
    }

    fn make_gateway() -> Gateway {
        Gateway::new(GatewayConfig::default(), CancellationToken::new())
    }

    /// Clear all gateway auth env vars to put the environment in a known state.
    ///
    /// # Safety
    /// Must be called from a serialized test because env vars are global state.
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
    #[serial_test::serial(edgecrab_gateway_env)]
    fn auth_deny_by_default_when_no_env_vars() {
        unsafe {
            clear_auth_env();
        }

        let gw = make_gateway();
        let msg = make_msg("alice", edgecrab_types::Platform::Telegram);
        assert!(
            !gw.is_user_authorized(&msg).is_allowed(),
            "secure-by-default: should deny when no allowlist configured"
        );
    }

    #[test]
    #[serial_test::serial(edgecrab_gateway_env)]
    fn auth_global_allow_all() {
        unsafe {
            clear_auth_env();
            std::env::set_var("GATEWAY_ALLOW_ALL_USERS", "true");
        }
        let gw = make_gateway();
        let msg = make_msg("anyone", edgecrab_types::Platform::Telegram);
        assert!(gw.is_user_authorized(&msg).is_allowed());
        unsafe {
            clear_auth_env();
        }
    }

    #[test]
    #[serial_test::serial(edgecrab_gateway_env)]
    fn auth_global_allowlist_permits_listed_user() {
        unsafe {
            clear_auth_env();
            std::env::set_var("GATEWAY_ALLOWED_USERS", "alice,bob");
        }
        let gw = make_gateway();
        let allow = make_msg("alice", edgecrab_types::Platform::Telegram);
        let deny = make_msg("charlie", edgecrab_types::Platform::Telegram);
        assert!(gw.is_user_authorized(&allow).is_allowed());
        assert!(!gw.is_user_authorized(&deny).is_allowed());
        unsafe {
            clear_auth_env();
        }
    }

    #[test]
    #[serial_test::serial(edgecrab_gateway_env)]
    fn auth_platform_allowlist_telegram() {
        unsafe {
            clear_auth_env();
            std::env::set_var("TELEGRAM_ALLOWED_USERS", "12345");
        }
        let gw = make_gateway();
        let allow = make_msg("12345", edgecrab_types::Platform::Telegram);
        let deny = make_msg("99999", edgecrab_types::Platform::Telegram);
        assert!(gw.is_user_authorized(&allow).is_allowed());
        assert!(!gw.is_user_authorized(&deny).is_allowed());
        unsafe {
            clear_auth_env();
        }
    }

    #[test]
    #[serial_test::serial(edgecrab_gateway_env)]
    fn auth_platform_allow_all_discord() {
        unsafe {
            clear_auth_env();
            std::env::set_var("DISCORD_ALLOW_ALL_USERS", "1");
            // Telegram still needs explicit listing when a TELEGRAM list exists
            std::env::set_var("TELEGRAM_ALLOWED_USERS", "only-me");
        }
        let gw = make_gateway();
        let discord_msg = make_msg("anyone", edgecrab_types::Platform::Discord);
        let telegram_other = make_msg("stranger", edgecrab_types::Platform::Telegram);
        assert!(gw.is_user_authorized(&discord_msg).is_allowed());
        assert!(!gw.is_user_authorized(&telegram_other).is_allowed());
        unsafe {
            clear_auth_env();
        }
    }

    // ── help-text tests ────────────────────────────────────────────────────

    #[test]
    fn help_text_contains_all_commands() {
        let help_text = gateway_help_text();
        for cmd in gateway_builtin_commands().into_iter().map(|(name, _)| name) {
            assert!(help_text.contains(&cmd), "gateway help missing {cmd}");
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

    #[test]
    fn extract_stream_fallback_response_prefers_new_assistant_turn() {
        let messages = vec![
            edgecrab_types::Message::user("old user"),
            edgecrab_types::Message::assistant("old answer"),
            edgecrab_types::Message::user("new user"),
            edgecrab_types::Message::assistant("new answer"),
        ];

        let response = extract_stream_fallback_response(&messages, 2).expect("response");
        assert_eq!(response, "new answer");
    }

    #[test]
    fn extract_stream_fallback_response_falls_back_to_last_assistant() {
        let messages = vec![
            edgecrab_types::Message::user("old user"),
            edgecrab_types::Message::assistant("old answer"),
        ];

        let response = extract_stream_fallback_response(&messages, 99).expect("response");
        assert_eq!(response, "old answer");
    }

    #[tokio::test]
    async fn deliver_text_and_media_sends_media_only_responses_when_adapter_available() {
        let adapter = Arc::new(RecordingAdapter::default());
        let mut delivery = DeliveryRouter::new();
        delivery.register(adapter.clone());

        let sent = deliver_text_and_media(
            &delivery,
            Some(adapter.clone()),
            "MEDIA:/tmp/report.pdf",
            edgecrab_types::Platform::Webhook,
            &crate::platform::MessageMetadata::default(),
        )
        .await
        .expect("delivery succeeds");

        assert_eq!(sent, 0);
        assert!(adapter.sent.lock().expect("sent lock").is_empty());
        assert_eq!(
            adapter.documents.lock().expect("documents lock").as_slice(),
            &["/tmp/report.pdf".to_string()]
        );
    }

    #[tokio::test]
    async fn deliver_text_and_media_routes_audio_to_send_voice() {
        let adapter = Arc::new(RecordingAdapter::default());
        let mut delivery = DeliveryRouter::new();
        delivery.register(adapter.clone());

        let sent = deliver_text_and_media(
            &delivery,
            Some(adapter.clone()),
            "MEDIA:/tmp/reply.mp3",
            edgecrab_types::Platform::Webhook,
            &crate::platform::MessageMetadata::default(),
        )
        .await
        .expect("delivery succeeds");

        assert_eq!(sent, 0);
        assert!(adapter.sent.lock().expect("sent lock").is_empty());
        assert_eq!(
            adapter.voices.lock().expect("voices lock").as_slice(),
            &["/tmp/reply.mp3".to_string()]
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn gateway_keeps_agent_history_isolated_per_chat_session() {
        let cancel = CancellationToken::new();
        let mut gateway = Gateway::new(
            GatewayConfig {
                port: 0,
                streaming: crate::config::GatewayStreamingConfig {
                    enabled: false,
                    ..crate::config::GatewayStreamingConfig::default()
                },
                ..GatewayConfig::default()
            },
            cancel.clone(),
        );
        let adapter = Arc::new(ScriptedAdapter::with_delay(
            vec![
                webhook_message("alice", "alpha"),
                webhook_message("bob", "bravo"),
                webhook_message("alice", "again"),
            ],
            std::time::Duration::from_millis(10),
        ));
        gateway.add_adapter(adapter.clone());
        gateway.set_agent(Arc::new(
            AgentBuilder::new("history-echo/mock")
                .provider(Arc::new(HistoryEchoProvider))
                .build()
                .expect("build agent"),
        ));

        let task = tokio::spawn(async move { gateway.run().await });
        let sent = adapter.wait_for_sent(4).await;
        cancel.cancel();
        tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .expect("gateway shutdown timeout")
            .expect("gateway join")
            .expect("gateway run");

        assert!(
            sent.iter()
                .any(|msg| msg.contains("current=alpha;prior_users=0;first_prior=-")),
            "sent: {sent:?}"
        );
        assert!(
            sent.iter()
                .any(|msg| msg.contains("current=bravo;prior_users=0;first_prior=-")),
            "sent: {sent:?}"
        );
        assert!(
            sent.iter()
                .any(|msg| msg.contains("current=again;prior_users=1;first_prior=alpha")),
            "sent: {sent:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn gateway_session_hooks_fire_across_chat_reset_and_shutdown() {
        let temp = TempDir::new().expect("tempdir");
        let plugin_dir = temp.path().join("gateway-session-hooks");
        std::fs::create_dir_all(&plugin_dir).expect("plugin dir");
        write_gateway_session_hook_plugin(&plugin_dir);

        let mut config = AppConfig::default();
        config.plugins.external_dirs = vec![temp.path().to_string_lossy().to_string()];
        let agent = Arc::new(
            AgentBuilder::from_config(&config)
                .provider(Arc::new(edgequake_llm::MockProvider::new()))
                .build()
                .expect("build agent"),
        );

        let cancel = CancellationToken::new();
        let mut gateway = Gateway::new(
            GatewayConfig {
                port: 0,
                streaming: crate::config::GatewayStreamingConfig {
                    enabled: false,
                    ..crate::config::GatewayStreamingConfig::default()
                },
                ..GatewayConfig::default()
            },
            cancel.clone(),
        );
        let adapter = Arc::new(ScriptedAdapter::with_delay(
            vec![
                webhook_message("alice", "hello"),
                webhook_message("alice", "/new"),
            ],
            std::time::Duration::from_millis(10),
        ));
        gateway.add_adapter(adapter.clone());
        gateway.set_agent(agent);

        let task = tokio::spawn(async move { gateway.run().await });
        let _ = adapter.wait_for_sent(2).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        cancel.cancel();
        tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .expect("gateway shutdown timeout")
            .expect("gateway join")
            .expect("gateway run");

        let log =
            std::fs::read_to_string(plugin_dir.join("gateway-session-hooks.jsonl")).expect("log");
        assert!(log.contains("on_session_start"), "log:\n{log}");
        assert!(log.contains("on_session_end"), "log:\n{log}");
        assert!(log.contains("on_session_reset"), "log:\n{log}");
        assert!(log.contains("on_session_finalize"), "log:\n{log}");
    }
}
