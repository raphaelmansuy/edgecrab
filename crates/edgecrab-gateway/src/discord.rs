//! # Discord adapter
//!
//! Connects to the Discord REST API and Gateway WebSocket to receive and
//! send messages in Discord servers and DMs.
//!
//! ```text
//!   DiscordAdapter
//!     ├── start()           → Gateway WebSocket connect, IDENTIFY, listen
//!     ├── send()            → REST API POST /channels/{id}/messages
//!     ├── format_response() → Discord-flavored markdown
//!     └── is_available()    → checks DISCORD_BOT_TOKEN
//! ```
//!
//! ## Environment variables
//!
//! | Variable            | Required | Description                       |
//! |---------------------|----------|-----------------------------------|
//! | `DISCORD_BOT_TOKEN` | Yes      | Bot token from Discord Dev Portal |
//!
//! ## Limits
//!
//! - Max message length: **2000** characters
//! - Supports Discord markdown (bold, italic, code blocks, etc.)
//! - Thread persistence via `message_reference`
//! - Typing indicator before responses

use std::env;
use std::time::Duration;

use async_trait::async_trait;
use edgecrab_types::Platform;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::delivery::split_message;
use crate::platform::{
    IncomingMessage, MessageAttachment, MessageAttachmentKind, MessageMetadata, OutgoingMessage,
    PlatformAdapter,
};
use crate::voice_delivery::prepare_voice_attachment;

/// Maximum message length for Discord messages.
const MAX_MESSAGE_LENGTH: usize = 2000;

/// Delay between retries after a network error (alias of `crate::ADAPTER_RETRY_DELAY`).
const RETRY_DELAY: Duration = crate::ADAPTER_RETRY_DELAY;

/// Discord API base URL.
const DISCORD_API_BASE: &str = "https://discord.com/api/v10";

/// Discord Gateway WebSocket URL.
#[allow(dead_code)]
const DISCORD_GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";

// ---------------------------------------------------------------------------
// Discord API types (minimal subset)
// ---------------------------------------------------------------------------

/// Gateway event payload (simplified — we only care about MESSAGE_CREATE).
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GatewayEvent {
    op: u8,
    #[serde(default)]
    t: Option<String>,
    #[serde(default)]
    s: Option<u64>,
    d: serde_json::Value,
}

/// Gateway Hello payload (op 10).
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct HelloPayload {
    heartbeat_interval: u64,
}

/// Gateway Identify payload (op 2).
#[allow(dead_code)]
#[derive(Debug, Serialize)]
struct IdentifyPayload {
    token: String,
    intents: u64,
    properties: IdentifyProperties,
}

#[allow(dead_code)]
#[derive(Debug, Serialize)]
struct IdentifyProperties {
    os: String,
    browser: String,
    device: String,
}

/// Discord message object (subset).
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct DiscordMessage {
    id: String,
    channel_id: String,
    author: DiscordUser,
    content: String,
    #[serde(default)]
    thread: Option<DiscordThread>,
    #[serde(default)]
    guild_id: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct DiscordUser {
    id: String,
    username: String,
    #[serde(default)]
    bot: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct DiscordThread {
    id: String,
}

/// Create message request body.
#[derive(Debug, Serialize)]
struct CreateMessageRequest<'a> {
    content: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_reference: Option<MessageReference<'a>>,
}

#[derive(Debug, Serialize)]
struct MessageReference<'a> {
    message_id: &'a str,
}

#[derive(Debug, Deserialize)]
struct CreateMessageResponse {
    id: String,
}

#[derive(Debug, Serialize)]
struct UpdateMessageRequest<'a> {
    content: &'a str,
}

// Trigger typing indicator request (empty body, POST).
// No request body fields are needed; the POST URL is enough.

// ---------------------------------------------------------------------------
// DiscordAdapter
// ---------------------------------------------------------------------------

/// Discord Gateway opcode constants.
mod op {
    pub const DISPATCH: u8 = 0;
    pub const HEARTBEAT: u8 = 1;
    pub const IDENTIFY: u8 = 2;
    pub const RECONNECT: u8 = 7;
    pub const INVALID_SESSION: u8 = 9;
    pub const HELLO: u8 = 10;
    pub const HEARTBEAT_ACK: u8 = 11;
}

/// Discord Gateway intents:
/// GUILDS | GUILD_MESSAGES | DIRECT_MESSAGES | MESSAGE_CONTENT (privileged)
const GATEWAY_INTENTS: u64 = (1 << 0) | (1 << 9) | (1 << 12) | (1 << 15);

/// Discord adapter using REST API + Gateway WebSocket.
///
/// Listens for `MESSAGE_CREATE` events on the Gateway and delivers
/// responses via the REST API. Sends a typing indicator before replying.
pub struct DiscordAdapter {
    /// Bot token from Discord Developer Portal
    token: String,
    /// Pre-built HTTP client
    client: reqwest::Client,
    /// Base URL for the Discord REST API.
    api_base: String,
    /// User IDs allowed to message the agent (empty = open access).
    allowed_users: Vec<String>,
}

impl DiscordAdapter {
    /// Create a new Discord adapter from environment variables.
    ///
    /// # Errors
    /// Returns an error if `DISCORD_BOT_TOKEN` is not set.
    pub fn new() -> anyhow::Result<Self> {
        let token = env::var("DISCORD_BOT_TOKEN")
            .map_err(|_| anyhow::anyhow!("DISCORD_BOT_TOKEN environment variable not set"))?;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self {
            token,
            client,
            api_base: DISCORD_API_BASE.into(),
            allowed_users: Vec::new(),
        })
    }

    /// Create a Discord adapter directly from config values.
    pub fn from_token(token: String, allowed_users: Vec<String>) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            token,
            client,
            api_base: DISCORD_API_BASE.into(),
            allowed_users,
        })
    }

    /// Check whether the adapter can be activated.
    pub fn is_available() -> bool {
        env::var("DISCORD_BOT_TOKEN").is_ok()
    }

    async fn cache_attachment(
        &self,
        url: &str,
        file_name: Option<&str>,
        fallback_name: &str,
    ) -> anyhow::Result<Option<String>> {
        let response = self.client.get(url).send().await?.error_for_status()?;
        let bytes = response.bytes().await?;
        if bytes.is_empty() {
            return Ok(None);
        }

        crate::attachment_cache::persist_bytes("discord", file_name, fallback_name, bytes.as_ref())
    }

    async fn extract_attachments(
        &self,
        value: &serde_json::Value,
        message_id: Option<&str>,
    ) -> Vec<MessageAttachment> {
        let mut attachments = Vec::new();
        for attachment in value.as_array().into_iter().flatten() {
            let file_name = attachment["filename"].as_str().map(ToString::to_string);
            let mime_type = attachment["content_type"].as_str().map(ToString::to_string);
            let url = attachment["url"].as_str().map(ToString::to_string);
            let size_bytes = attachment["size"].as_u64();
            let kind = classify_discord_attachment(mime_type.as_deref(), file_name.as_deref());

            let mut normalized = MessageAttachment {
                kind: kind.clone(),
                file_name: file_name.clone(),
                mime_type,
                url: url.clone(),
                size_bytes,
                ..Default::default()
            };

            if let Some(url) = url.as_deref() {
                let fallback_name =
                    discord_attachment_fallback_name(&kind, message_id, file_name.as_deref());
                match self
                    .cache_attachment(url, file_name.as_deref(), &fallback_name)
                    .await
                {
                    Ok(Some(local_path)) => normalized.local_path = Some(local_path),
                    Ok(None) => {}
                    Err(error) => warn!(%error, url, "Discord attachment download failed"),
                }
            }

            attachments.push(normalized);
        }

        attachments
    }

    /// Trigger the typing indicator in a channel.
    async fn trigger_typing(&self, channel_id: &str) -> anyhow::Result<()> {
        let url = format!("{}/channels/{}/typing", self.api_base, channel_id);
        self.client
            .post(&url)
            .header("Authorization", format!("Bot {}", self.token))
            .send()
            .await?;
        Ok(())
    }

    /// Send a message to a Discord channel via REST.
    async fn send_rest_message_with_id(
        &self,
        channel_id: &str,
        text: &str,
        reply_to: Option<&str>,
    ) -> anyhow::Result<String> {
        let url = format!("{}/channels/{}/messages", self.api_base, channel_id);
        let body = CreateMessageRequest {
            content: text,
            message_reference: reply_to.map(|id| MessageReference { message_id: id }),
        };

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bot {}", self.token))
            .json(&body)
            .send()
            .await?;

        if resp.status().is_success() {
            let created: CreateMessageResponse = resp.json().await?;
            return Ok(created.id);
        }

        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        let should_retry_without_reply =
            reply_to.is_some() && body_text.contains("Cannot reply to a system message");
        if should_retry_without_reply {
            let fallback = CreateMessageRequest {
                content: text,
                message_reference: None,
            };
            let retry = self
                .client
                .post(&url)
                .header("Authorization", format!("Bot {}", self.token))
                .json(&fallback)
                .send()
                .await?;
            if retry.status().is_success() {
                let created: CreateMessageResponse = retry.json().await?;
                return Ok(created.id);
            }
            let retry_status = retry.status();
            let retry_body = retry.text().await.unwrap_or_default();
            anyhow::bail!(
                "Discord sendMessage failed after reply fallback ({}): {}",
                retry_status,
                retry_body
            );
        }

        anyhow::bail!("Discord sendMessage failed ({}): {}", status, body_text)
    }

    async fn send_rest_message(
        &self,
        channel_id: &str,
        text: &str,
        reply_to: Option<&str>,
    ) -> anyhow::Result<()> {
        self.send_rest_message_with_id(channel_id, text, reply_to)
            .await
            .map(|_| ())
    }

    async fn edit_rest_message(
        &self,
        channel_id: &str,
        message_id: &str,
        text: &str,
    ) -> anyhow::Result<()> {
        let url = format!(
            "{}/channels/{}/messages/{}",
            self.api_base, channel_id, message_id
        );
        let body = UpdateMessageRequest { content: text };
        let resp = self
            .client
            .patch(&url)
            .header("Authorization", format!("Bot {}", self.token))
            .json(&body)
            .send()
            .await?;

        if resp.status().is_success() {
            return Ok(());
        }

        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Discord editMessage failed ({}): {}", status, body_text)
    }

    /// Upload a local file as a Discord message attachment using multipart POST.
    ///
    /// Protocol: `POST /channels/{id}/messages` with `multipart/form-data`:
    /// - `files[0]`   — binary file content
    /// - `payload_json` — JSON `{content: caption}` (optional caption text)
    ///
    /// WHY: Discord natively renders images and documents uploaded as
    /// form-data attachments. Sending a local path as plain text would expose
    /// the server filesystem path to users without rendering anything useful.
    async fn send_file_attachment(
        &self,
        channel_id: &str,
        path: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let file_bytes = tokio::fs::read(path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read file '{}': {}", path, e))?;

        let file_name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment");

        // Guess MIME type from extension for the Content-Type header.
        let mime = match std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref()
        {
            Some("png") => "image/png",
            Some("jpg" | "jpeg") => "image/jpeg",
            Some("gif") => "image/gif",
            Some("webp") => "image/webp",
            Some("ogg" | "opus") => "audio/ogg",
            Some("mp3") => "audio/mpeg",
            Some("wav") => "audio/wav",
            Some("m4a") => "audio/mp4",
            Some("aac") => "audio/aac",
            Some("pdf") => "application/pdf",
            _ => "application/octet-stream",
        };

        let file_part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(file_name.to_string())
            .mime_str(mime)?;

        let payload_json = serde_json::json!({
            "content": caption.unwrap_or("")
        });
        let payload_part = reqwest::multipart::Part::text(payload_json.to_string())
            .mime_str("application/json")?;

        let form = reqwest::multipart::Form::new()
            .part("files[0]", file_part)
            .part("payload_json", payload_part);

        let url = format!("{}/channels/{}/messages", self.api_base, channel_id);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bot {}", self.token))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Discord file upload failed ({}): {}", status, body_text);
        }

        Ok(())
    }

    /// Connect to the Discord Gateway WebSocket and process MESSAGE_CREATE events.    ///
    /// Protocol:
    /// 1. Connect to `wss://gateway.discord.gg/?v=10&encoding=json`
    /// 2. Receive HELLO (op=10) → extract `heartbeat_interval`
    /// 3. Send IDENTIFY (op=2) with token + intents
    /// 4. Start heartbeat: send op=1 every `heartbeat_interval` ms
    /// 5. Receive DISPATCH (op=0) events; handle MESSAGE_CREATE
    /// 6. On RECONNECT (op=7) or INVALID_SESSION (op=9) → break to reconnect
    async fn connect_gateway(&self, tx: &mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
        use futures::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message as WsMsg;

        let (ws_stream, _) = tokio_tungstenite::connect_async(DISCORD_GATEWAY_URL).await?;
        let (ws_write, mut ws_read) = ws_stream.split();

        // Channel for sending frames: heartbeat task and main loop both write here
        let (frame_tx, mut frame_rx) = tokio::sync::mpsc::unbounded_channel::<serde_json::Value>();

        // Writer task — drains frame_rx and sends to WebSocket write half
        let frame_tx_clone = frame_tx.clone();
        tokio::spawn(async move {
            let mut writer = ws_write;
            while let Some(payload) = frame_rx.recv().await {
                let text = match serde_json::to_string(&payload) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                if writer.send(WsMsg::Text(text)).await.is_err() {
                    break;
                }
            }
            drop(frame_tx_clone); // Keep sender alive until writer exits
        });

        let mut heartbeat_interval: Option<tokio::time::Interval> = None;
        let mut last_sequence: Option<u64> = None;
        let mut heartbeat_ack = true; // tracks whether we received the previous ack

        info!("Discord Gateway connected, waiting for HELLO");

        loop {
            let ws_msg = if let Some(ref mut ticker) = heartbeat_interval {
                tokio::select! {
                    msg = ws_read.next() => msg,
                    _ = ticker.tick() => {
                        // Send heartbeat
                        if !heartbeat_ack {
                            warn!("Discord: no heartbeat ACK received — connection may be zombie, reconnecting");
                            anyhow::bail!("heartbeat ACK timeout");
                        }
                        heartbeat_ack = false;
                        let hb = serde_json::json!({
                            "op": op::HEARTBEAT,
                            "d": last_sequence
                        });
                        let _ = frame_tx.send(hb);
                        continue;
                    }
                }
            } else {
                ws_read.next().await
            };

            let ws_msg = match ws_msg {
                Some(Ok(m)) => m,
                Some(Err(e)) => anyhow::bail!("Discord WebSocket error: {e}"),
                None => anyhow::bail!("Discord WebSocket stream ended"),
            };

            let text = match ws_msg {
                WsMsg::Text(t) => t,
                WsMsg::Ping(d) => {
                    let _ = frame_tx.send(serde_json::json!(null)); // unused but keeps writer alive
                    // tungstenite handles Pong internally; just continue
                    let _ = d;
                    continue;
                }
                WsMsg::Close(_) => anyhow::bail!("Discord WebSocket closed by server"),
                _ => continue,
            };

            let event: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    debug!(error = %e, "Failed to parse Discord Gateway event");
                    continue;
                }
            };

            let op = event["op"].as_u64().unwrap_or(255) as u8;
            if let Some(s) = event["s"].as_u64() {
                last_sequence = Some(s);
            }

            match op {
                op::HELLO => {
                    let interval_ms = event["d"]["heartbeat_interval"].as_u64().unwrap_or(41250);
                    info!(interval_ms, "Discord HELLO received, sending IDENTIFY");

                    // Start heartbeat interval (first tick is after ~interval_ms)
                    let mut ticker = tokio::time::interval(Duration::from_millis(interval_ms));
                    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    ticker.tick().await; // consume the immediate first tick
                    heartbeat_interval = Some(ticker);

                    // Send IDENTIFY
                    let identify = serde_json::json!({
                        "op": op::IDENTIFY,
                        "d": {
                            "token": self.token,
                            "intents": GATEWAY_INTENTS,
                            "properties": {
                                "os": "linux",
                                "browser": "edgecrab",
                                "device": "edgecrab"
                            }
                        }
                    });
                    let _ = frame_tx.send(identify);
                }

                op::DISPATCH => {
                    let event_name = event["t"].as_str().unwrap_or("");
                    heartbeat_ack = true; // reset on any dispatch

                    match event_name {
                        "READY" => {
                            info!("Discord Gateway READY — bot is connected");
                        }
                        "MESSAGE_CREATE" => {
                            let d = &event["d"];
                            // Skip bot messages (including our own)
                            if d["author"]["bot"].as_bool().unwrap_or(false) {
                                continue;
                            }
                            let attachments = self
                                .extract_attachments(&d["attachments"], d["id"].as_str())
                                .await;
                            let content = d["content"].as_str().unwrap_or("");
                            let rendered_text = render_discord_incoming_text(content, &attachments);
                            if rendered_text.trim().is_empty() && attachments.is_empty() {
                                continue;
                            }
                            let user_id =
                                d["author"]["id"].as_str().unwrap_or("unknown").to_string();
                            let username = d["author"]["username"]
                                .as_str()
                                .unwrap_or("unknown")
                                .to_string();
                            let channel_id = d["channel_id"].as_str().unwrap_or("").to_string();
                            let message_id = d["id"].as_str().unwrap_or("").to_string();
                            let thread_id = d["thread"]["id"].as_str().map(ToString::to_string);

                            // Filter by allowed_users
                            if !self.allowed_users.is_empty()
                                && !self.allowed_users.contains(&user_id)
                                && !self.allowed_users.contains(&username)
                            {
                                debug!(
                                    user_id = %user_id,
                                    "Discord message filtered: not in allowed list"
                                );
                                continue;
                            }

                            let chat_type = if d["guild_id"].as_str().is_some() {
                                crate::platform::ChatType::Group
                            } else {
                                crate::platform::ChatType::Dm
                            };

                            let incoming = IncomingMessage {
                                platform: Platform::Discord,
                                user_id: user_id.clone(),
                                channel_id: Some(channel_id.clone()),
                                chat_type,
                                text: rendered_text,
                                thread_id: thread_id.clone(),
                                metadata: MessageMetadata {
                                    message_id: Some(message_id),
                                    channel_id: Some(channel_id),
                                    thread_id,
                                    user_display_name: Some(username),
                                    attachments,
                                    ..Default::default()
                                },
                            };

                            debug!(platform = "discord", user = %user_id, "Received message");

                            if tx.send(incoming).await.is_err() {
                                info!("Discord adapter: receiver dropped, shutting down");
                                return Ok(());
                            }
                        }
                        _ => {}
                    }
                }

                op::HEARTBEAT => {
                    // Server requesting an immediate heartbeat
                    let hb = serde_json::json!({
                        "op": op::HEARTBEAT,
                        "d": last_sequence
                    });
                    let _ = frame_tx.send(hb);
                }

                op::HEARTBEAT_ACK => {
                    heartbeat_ack = true;
                }

                op::RECONNECT => {
                    info!("Discord requested RECONNECT");
                    anyhow::bail!("Discord RECONNECT requested");
                }

                op::INVALID_SESSION => {
                    let resumable = event["d"].as_bool().unwrap_or(false);
                    warn!(resumable, "Discord INVALID_SESSION — reconnecting");
                    anyhow::bail!("Discord INVALID_SESSION");
                }

                other => {
                    debug!(op = other, "Discord unknown opcode");
                }
            }
        }
    }
}

/// Format text for Discord markdown.
///
/// Discord uses a subset of Markdown: **bold**, *italic*, `code`,
/// ```code blocks```, > quotes, etc. We leave the text mostly as-is
/// since Discord natively supports standard Markdown.
fn format_discord_markdown(text: &str) -> String {
    // Discord markdown is close to standard Markdown — no heavy escaping
    // needed. We just ensure the output is within limits.
    text.to_string()
}

fn classify_discord_attachment(
    content_type: Option<&str>,
    file_name: Option<&str>,
) -> MessageAttachmentKind {
    let content_type = content_type.unwrap_or_default().to_ascii_lowercase();
    if content_type.starts_with("image/") {
        return MessageAttachmentKind::Image;
    }
    if content_type.starts_with("video/") {
        return MessageAttachmentKind::Video;
    }
    if content_type.starts_with("audio/") {
        return MessageAttachmentKind::Audio;
    }

    let extension = file_name
        .and_then(|name| name.rsplit('.').next())
        .map(|ext| ext.to_ascii_lowercase());
    match extension.as_deref() {
        Some("jpg" | "jpeg" | "png" | "gif" | "webp") => MessageAttachmentKind::Image,
        Some("mp4" | "mov" | "avi" | "mkv" | "webm") => MessageAttachmentKind::Video,
        Some("ogg" | "opus" | "mp3" | "wav" | "m4a" | "aac") => MessageAttachmentKind::Audio,
        Some("pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "csv" | "zip") => {
            MessageAttachmentKind::Document
        }
        _ => MessageAttachmentKind::Other,
    }
}

fn discord_attachment_fallback_name(
    kind: &MessageAttachmentKind,
    message_id: Option<&str>,
    file_name: Option<&str>,
) -> String {
    let suffix = match kind {
        MessageAttachmentKind::Image => "jpg",
        MessageAttachmentKind::Video => "mp4",
        MessageAttachmentKind::Audio | MessageAttachmentKind::Voice => "ogg",
        MessageAttachmentKind::Sticker => "webp",
        MessageAttachmentKind::Document => file_name
            .and_then(|name| {
                std::path::Path::new(name)
                    .extension()
                    .and_then(|ext| ext.to_str())
            })
            .unwrap_or("bin"),
        MessageAttachmentKind::Other => "bin",
    };
    let safe_message_id = message_id.unwrap_or("unknown");
    format!("discord-{safe_message_id}.{suffix}")
}

fn render_discord_incoming_text(content: &str, attachments: &[MessageAttachment]) -> String {
    let content = content.trim();
    if attachments.is_empty() {
        return content.to_string();
    }

    let mut lines = Vec::new();
    if !content.is_empty() {
        lines.push(content.to_string());
        lines.push(String::new());
    }

    let noun = if attachments.len() == 1 {
        "attachment"
    } else {
        "attachments"
    };
    lines.push(format!("Shared {} {}:", attachments.len(), noun));

    for attachment in attachments {
        let label = attachment.kind.label();
        let file_name = attachment.file_name.as_deref().unwrap_or(label);
        if let Some(source) = attachment.display_source() {
            lines.push(format!("- {}: {} ({})", label, file_name, source));
        } else {
            lines.push(format!("- {}: {}", label, file_name));
        }
    }

    lines.join("\n")
}

#[async_trait]
impl PlatformAdapter for DiscordAdapter {
    fn platform(&self) -> Platform {
        Platform::Discord
    }

    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
        info!("Discord adapter starting (Gateway WebSocket mode)");

        let mut retry_delay = RETRY_DELAY;
        loop {
            match self.connect_gateway(&tx).await {
                Ok(()) => {
                    info!("Discord Gateway disconnected cleanly");
                    retry_delay = RETRY_DELAY;
                }
                Err(e) => {
                    error!(error = %e, "Discord Gateway error, reconnecting");
                    retry_delay = std::cmp::min(
                        Duration::from_secs(retry_delay.as_secs().saturating_mul(2)),
                        Duration::from_secs(120),
                    );
                }
            }
            tokio::time::sleep(retry_delay).await;
        }
    }

    async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()> {
        let channel_id = msg
            .metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Discord send requires channel_id"))?;

        let reply_to = msg.metadata.message_id.as_deref();

        // Typing indicator
        if let Err(e) = self.trigger_typing(channel_id).await {
            debug!(error = %e, "Failed to trigger typing indicator");
        }

        let formatted = self.format_response(&msg.text, &msg.metadata);
        let chunks = split_message(&formatted, MAX_MESSAGE_LENGTH);

        for (index, chunk) in chunks.iter().enumerate() {
            let chunk_reply_to = if index == 0 { reply_to } else { None };
            self.send_rest_message(channel_id, chunk, chunk_reply_to)
                .await?;
        }

        Ok(())
    }

    fn format_response(&self, text: &str, _metadata: &MessageMetadata) -> String {
        format_discord_markdown(text)
    }

    fn max_message_length(&self) -> usize {
        MAX_MESSAGE_LENGTH
    }

    fn supports_markdown(&self) -> bool {
        true
    }

    fn supports_images(&self) -> bool {
        true
    }

    fn supports_files(&self) -> bool {
        true
    }

    fn supports_editing(&self) -> bool {
        true
    }

    async fn send_typing(&self, metadata: &MessageMetadata) -> anyhow::Result<()> {
        let channel_id = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Discord send_typing requires channel_id"))?;
        self.trigger_typing(channel_id).await
    }

    async fn send_and_get_id(&self, msg: OutgoingMessage) -> anyhow::Result<Option<String>> {
        let channel_id = msg
            .metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Discord send_and_get_id requires channel_id"))?;
        let reply_to = msg.metadata.message_id.as_deref();
        let formatted = self.format_response(&msg.text, &msg.metadata);
        let chunks = split_message(&formatted, MAX_MESSAGE_LENGTH);
        let first_chunk = chunks.first().cloned().unwrap_or_default();
        let message_id = self
            .send_rest_message_with_id(channel_id, &first_chunk, reply_to)
            .await?;
        for chunk in chunks.iter().skip(1) {
            self.send_rest_message(channel_id, chunk, None).await?;
        }
        Ok(Some(message_id))
    }

    async fn edit_message(
        &self,
        message_id: &str,
        metadata: &MessageMetadata,
        new_text: &str,
    ) -> anyhow::Result<String> {
        let channel_id = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Discord edit_message requires channel_id"))?;
        let formatted = self.format_response(new_text, metadata);
        let truncated = if formatted.len() > MAX_MESSAGE_LENGTH {
            edgecrab_core::safe_truncate(&formatted, MAX_MESSAGE_LENGTH)
        } else {
            &formatted
        };
        self.edit_rest_message(channel_id, message_id, truncated)
            .await?;
        Ok(message_id.to_string())
    }

    /// Send a local image file as a Discord attachment.
    ///
    /// Uses the Discord REST multipart upload (`files[0]` field) so the image
    /// renders inline in Discord rather than as a plain-text URL.
    async fn send_photo(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &crate::platform::MessageMetadata,
    ) -> anyhow::Result<()> {
        let channel_id = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Discord send_photo requires channel_id"))?;

        self.send_file_attachment(channel_id, path, caption).await
    }

    /// Send a local file as a Discord attachment.
    ///
    /// Uses the Discord REST multipart upload so the file appears as a
    /// native Discord attachment rather than as a plain-text path.
    async fn send_document(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &crate::platform::MessageMetadata,
    ) -> anyhow::Result<()> {
        let channel_id = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Discord send_document requires channel_id"))?;

        self.send_file_attachment(channel_id, path, caption).await
    }

    async fn send_voice(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &crate::platform::MessageMetadata,
    ) -> anyhow::Result<()> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};

        let channel_id = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Discord send_voice requires channel_id"))?;

        let prepared = prepare_voice_attachment(path, Platform::Discord).await?;
        let result = async {
            let file_bytes = tokio::fs::read(&prepared.path).await.map_err(|e| {
                anyhow::anyhow!(
                    "Discord send_voice: cannot read {}: {}",
                    prepared.path.display(),
                    e
                )
            })?;

            if prepared.is_native_voice_note {
                let waveform_b64 = STANDARD.encode([128u8; 256]);
                let payload_json = serde_json::json!({
                    "content": caption.unwrap_or(""),
                    "flags": 8192,
                    "attachments": [{
                        "id": "0",
                        "filename": "voice-message.ogg",
                        "duration_secs": prepared.duration_secs.unwrap_or(5.0),
                        "waveform": waveform_b64,
                    }]
                });
                let form = reqwest::multipart::Form::new()
                    .part(
                        "files[0]",
                        reqwest::multipart::Part::bytes(file_bytes)
                            .file_name("voice-message.ogg")
                            .mime_str(prepared.mime)?,
                    )
                    .part(
                        "payload_json",
                        reqwest::multipart::Part::text(payload_json.to_string())
                            .mime_str("application/json")?,
                    );
                let url = format!("{}/channels/{}/messages", DISCORD_API_BASE, channel_id);
                let resp = self
                    .client
                    .post(&url)
                    .header("Authorization", format!("Bot {}", self.token))
                    .multipart(form)
                    .send()
                    .await?;
                if resp.status().is_success() {
                    Ok(())
                } else {
                    let status = resp.status();
                    let body_text = resp.text().await.unwrap_or_default();
                    debug!(
                        status = %status,
                        body = body_text,
                        "Discord native voice-message upload failed; falling back to regular attachment"
                    );
                    self.send_file_attachment(
                        channel_id,
                        prepared.path.to_string_lossy().as_ref(),
                        caption,
                    )
                    .await
                }
            } else {
                self.send_file_attachment(
                    channel_id,
                    prepared.path.to_string_lossy().as_ref(),
                    caption,
                )
                .await
            }
        }
        .await;

        prepared.cleanup().await;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::get};

    fn test_adapter() -> DiscordAdapter {
        DiscordAdapter {
            token: "test".into(),
            client: reqwest::Client::new(),
            api_base: "http://localhost".into(),
            allowed_users: Vec::new(),
        }
    }

    #[test]
    fn test_format_discord_markdown() {
        let text = "**bold** and *italic* and `code`";
        assert_eq!(format_discord_markdown(text), text);
    }

    #[test]
    fn test_split_message_short() {
        let chunks = split_message("short", 2000);
        assert_eq!(chunks, vec!["short"]);
    }

    #[test]
    fn test_split_message_long() {
        let long = "a".repeat(3000);
        let chunks = split_message(&long, 2000);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 2000);
        assert_eq!(chunks[1].len(), 1000);
    }

    #[test]
    fn test_is_available_without_env() {
        let _ = DiscordAdapter::is_available();
    }

    #[test]
    fn test_capabilities() {
        let adapter = test_adapter();
        assert_eq!(adapter.max_message_length(), 2000);
        assert!(adapter.supports_markdown());
        assert!(adapter.supports_images());
        assert!(adapter.supports_files());
        assert!(adapter.supports_editing());
        assert_eq!(adapter.platform(), Platform::Discord);
    }

    #[test]
    fn test_format_response() {
        let adapter = test_adapter();
        let formatted = adapter.format_response("**hello**", &MessageMetadata::default());
        assert_eq!(formatted, "**hello**");
    }

    #[test]
    fn test_render_discord_incoming_text_with_attachments() {
        let attachments = vec![MessageAttachment {
            kind: MessageAttachmentKind::Image,
            file_name: Some("preview.png".into()),
            url: Some("https://cdn.discordapp.com/preview.png".into()),
            ..Default::default()
        }];
        let rendered = render_discord_incoming_text("", &attachments);
        assert!(rendered.contains("Shared 1 attachment:"));
        assert!(rendered.contains("image: preview.png"));
    }

    #[tokio::test]
    async fn test_extract_attachments_downloads_discord_image_to_local_cache() {
        async fn image_bytes() -> ([(&'static str, &'static str); 1], &'static [u8]) {
            (
                [("content-type", "image/png")],
                b"\x89PNG\r\n\x1a\nedgecrab",
            )
        }

        let app = Router::new().route("/cdn/image.png", get(image_bytes));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });

        let adapter = test_adapter();
        let image_url = format!("http://{addr}/cdn/image.png");
        let attachments = adapter
            .extract_attachments(
                &serde_json::json!([
                    {
                        "filename": "preview.png",
                        "content_type": "image/png",
                        "url": image_url,
                        "size": 16
                    }
                ]),
                Some("msg123"),
            )
            .await;

        assert_eq!(attachments.len(), 1);
        let attachment = &attachments[0];
        let local_path = attachment.local_path.as_deref().expect("local path");
        assert!(std::path::Path::new(local_path).exists());
        let p = std::path::Path::new(local_path);
        assert!(
            p.components().any(|c| c.as_os_str() == "gateway_media")
                && p.components().any(|c| c.as_os_str() == "discord"),
            "expected gateway_media/discord in {local_path}"
        );
        assert_eq!(attachment.url.as_deref(), Some(image_url.as_str()));

        server.abort();
    }

    #[tokio::test]
    async fn discord_streaming_paths_support_editing_and_typing() {
        use axum::{
            Json, Router,
            extract::{Request, State},
            http::StatusCode,
            routing::any,
        };
        use std::sync::{Arc, Mutex};

        #[derive(Clone, Default)]
        struct RequestLog {
            paths: Arc<Mutex<Vec<String>>>,
        }

        async fn record_request(
            State(log): State<RequestLog>,
            request: Request,
        ) -> (StatusCode, Json<serde_json::Value>) {
            let path = request.uri().path().to_string();
            let method = request.method().as_str().to_string();
            log.paths
                .lock()
                .expect("paths")
                .push(format!("{method} {path}"));

            let response = if method == "POST" && path.ends_with("/messages") {
                serde_json::json!({ "id": "msg_123" })
            } else if method == "PATCH" && path.contains("/messages/") {
                let message_id = path.rsplit('/').next().unwrap_or("unknown");
                serde_json::json!({ "id": message_id })
            } else if method == "POST" && path.ends_with("/typing") {
                serde_json::json!({})
            } else {
                return (StatusCode::NOT_FOUND, Json(serde_json::json!({})));
            };

            (StatusCode::OK, Json(response))
        }

        let log = RequestLog::default();
        let app = Router::new()
            .fallback(any(record_request))
            .with_state(log.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });

        let adapter = DiscordAdapter {
            token: "test".into(),
            client: reqwest::Client::new(),
            api_base: format!("http://{addr}"),
            allowed_users: Vec::new(),
        };
        let metadata = MessageMetadata {
            channel_id: Some("chan_1".into()),
            ..Default::default()
        };

        let message_id = adapter
            .send_and_get_id(OutgoingMessage {
                text: "hello".into(),
                metadata: metadata.clone(),
            })
            .await
            .expect("send and get id");
        assert_eq!(message_id.as_deref(), Some("msg_123"));

        adapter
            .edit_message("msg_123", &metadata, "updated")
            .await
            .expect("edit message");
        adapter.send_typing(&metadata).await.expect("send typing");

        let paths = log.paths.lock().expect("paths").clone();
        assert!(paths.contains(&"POST /channels/chan_1/messages".to_string()));
        assert!(paths.contains(&"PATCH /channels/chan_1/messages/msg_123".to_string()));
        assert!(paths.contains(&"POST /channels/chan_1/typing".to_string()));

        server.abort();
    }
}
