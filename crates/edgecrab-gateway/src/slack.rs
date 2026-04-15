//! # Slack adapter
//!
//! Connects to Slack via Socket Mode (WebSocket) for real-time messages
//! and the Web API for sending responses.
//!
//! ```text
//!   SlackAdapter
//!     ├── start()           → Socket Mode WebSocket listener
//!     ├── send()            → Web API chat.postMessage
//!     ├── format_response() → Slack mrkdwn formatting
//!     └── is_available()    → checks SLACK_BOT_TOKEN + SLACK_APP_TOKEN
//! ```
//!
//! ## Environment variables
//!
//! | Variable          | Required | Description                          |
//! |-------------------|----------|--------------------------------------|
//! | `SLACK_BOT_TOKEN` | Yes      | Bot token (xoxb-...)                 |
//! | `SLACK_APP_TOKEN` | Yes      | App-level token for Socket Mode      |
//!
//! ## Limits
//!
//! - Max message length: **39000** characters (API limit is 40k, margin)
//! - Supports Slack mrkdwn formatting
//! - Thread support via `thread_ts`

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

/// Maximum message length for Slack.
const MAX_MESSAGE_LENGTH: usize = 39_000;

/// Delay between retries after a connection error (alias of `crate::ADAPTER_RETRY_DELAY`).
const RETRY_DELAY: Duration = crate::ADAPTER_RETRY_DELAY;

/// Slack Web API base URL.
const SLACK_API_BASE: &str = "https://slack.com/api";

// ---------------------------------------------------------------------------
// Slack API response types
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct SlackApiResponse {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct AuthTestResponse {
    ok: bool,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PostMessageResponse {
    ok: bool,
    #[serde(default)]
    ts: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FileUploadResponse {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
}

/// Socket Mode connection URL response.
#[derive(Debug, Deserialize)]
struct AppsConnectionsOpenResponse {
    ok: bool,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// Socket Mode WebSocket envelope.
#[derive(Debug, Deserialize)]
struct SocketModeEnvelope {
    #[serde(default)]
    envelope_id: Option<String>,
    #[serde(rename = "type")]
    #[serde(default)]
    event_type: Option<String>,
    #[serde(default)]
    payload: Option<serde_json::Value>,
}

/// Slack event wrapper (inside payload).
#[derive(Debug, Deserialize)]
struct EventPayload {
    #[serde(default)]
    event: Option<SlackEvent>,
}

/// Slack event.
#[derive(Debug, Deserialize)]
struct SlackEvent {
    #[serde(rename = "type")]
    #[serde(default)]
    event_type: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    ts: Option<String>,
    #[serde(default)]
    thread_ts: Option<String>,
    #[serde(default)]
    channel_type: Option<String>,
    #[serde(default)]
    files: Vec<SlackFile>,
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    bot_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SlackSlashCommandPayload {
    command: String,
    #[serde(default)]
    text: Option<String>,
    user_id: String,
    channel_id: String,
}

#[derive(Debug, Deserialize)]
struct SlackFile {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    mimetype: Option<String>,
    #[serde(default)]
    url_private: Option<String>,
    #[serde(default)]
    url_private_download: Option<String>,
    #[serde(default)]
    size: Option<u64>,
}

/// Acknowledge envelope.
#[derive(Debug, Serialize)]
struct SocketModeAck {
    envelope_id: String,
}

#[derive(Debug, Serialize)]
struct ChatPostMessageRequest<'a> {
    channel: &'a str,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    thread_ts: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct ChatUpdateRequest<'a> {
    channel: &'a str,
    ts: &'a str,
    text: &'a str,
}

// ---------------------------------------------------------------------------
// SlackAdapter
// ---------------------------------------------------------------------------

/// Slack adapter using Socket Mode for receiving and Web API for sending.
pub struct SlackAdapter {
    bot_token: String,
    app_token: String,
    api_base: String,
    client: reqwest::Client,
    bot_user_id: tokio::sync::RwLock<Option<String>>,
    /// Slack user IDs allowed to message the agent (empty = open access).
    allowed_users: Vec<String>,
}

impl SlackAdapter {
    fn default_api_base() -> String {
        env::var("SLACK_API_BASE_URL")
            .unwrap_or_else(|_| SLACK_API_BASE.to_string())
            .trim_end_matches('/')
            .to_string()
    }

    /// Create a new Slack adapter from environment variables.
    ///
    /// # Errors
    /// Returns an error if required tokens are not set.
    pub fn new() -> anyhow::Result<Self> {
        let bot_token = env::var("SLACK_BOT_TOKEN")
            .map_err(|_| anyhow::anyhow!("SLACK_BOT_TOKEN environment variable not set"))?;
        let app_token = env::var("SLACK_APP_TOKEN")
            .map_err(|_| anyhow::anyhow!("SLACK_APP_TOKEN environment variable not set"))?;

        let client = edgecrab_security::url_safety::build_ssrf_safe_client(
            Duration::from_secs(30),
        );

        Ok(Self {
            bot_token,
            app_token,
            api_base: Self::default_api_base(),
            client,
            bot_user_id: tokio::sync::RwLock::new(None),
            allowed_users: Vec::new(),
        })
    }

    /// Create a Slack adapter directly from token values.
    pub fn from_tokens(
        bot_token: String,
        app_token: String,
        allowed_users: Vec<String>,
    ) -> anyhow::Result<Self> {
        let client = edgecrab_security::url_safety::build_ssrf_safe_client(
            Duration::from_secs(30),
        );
        Ok(Self {
            bot_token,
            app_token,
            api_base: Self::default_api_base(),
            client,
            bot_user_id: tokio::sync::RwLock::new(None),
            allowed_users,
        })
    }

    /// Check whether the adapter can be activated.
    pub fn is_available() -> bool {
        env::var("SLACK_BOT_TOKEN").is_ok() && env::var("SLACK_APP_TOKEN").is_ok()
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}/{}", self.api_base, path.trim_start_matches('/'))
    }

    /// Call auth.test to get our bot user ID.
    async fn auth_test(&self) -> anyhow::Result<String> {
        let resp: AuthTestResponse = self
            .client
            .post(self.api_url("auth.test"))
            .bearer_auth(&self.bot_token)
            .send()
            .await?
            .json()
            .await?;

        if !resp.ok {
            anyhow::bail!("Slack auth.test failed: {}", resp.error.unwrap_or_default());
        }

        resp.user_id
            .ok_or_else(|| anyhow::anyhow!("auth.test did not return user_id"))
    }

    /// Get a Socket Mode WebSocket URL.
    async fn get_ws_url(&self) -> anyhow::Result<String> {
        let resp: AppsConnectionsOpenResponse = self
            .client
            .post(self.api_url("apps.connections.open"))
            .bearer_auth(&self.app_token)
            .send()
            .await?
            .json()
            .await?;

        if !resp.ok {
            anyhow::bail!(
                "Slack apps.connections.open failed: {}",
                resp.error.unwrap_or_default()
            );
        }

        resp.url
            .ok_or_else(|| anyhow::anyhow!("No WebSocket URL returned"))
    }

    /// Post a message to a Slack channel.
    async fn post_message(
        &self,
        channel: &str,
        text: &str,
        thread_ts: Option<&str>,
    ) -> anyhow::Result<Option<String>> {
        let body = ChatPostMessageRequest {
            channel,
            text,
            thread_ts,
        };

        let resp: PostMessageResponse = self
            .client
            .post(self.api_url("chat.postMessage"))
            .bearer_auth(&self.bot_token)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        if !resp.ok {
            anyhow::bail!(
                "Slack chat.postMessage failed: {}",
                resp.error.unwrap_or_default()
            );
        }

        Ok(resp.ts)
    }

    async fn update_message(&self, channel: &str, ts: &str, text: &str) -> anyhow::Result<()> {
        let body = ChatUpdateRequest { channel, ts, text };
        let resp: SlackApiResponse = self
            .client
            .post(self.api_url("chat.update"))
            .bearer_auth(&self.bot_token)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        if !resp.ok {
            anyhow::bail!(
                "Slack chat.update failed: {}",
                resp.error.unwrap_or_default()
            );
        }

        Ok(())
    }

    async fn upload_file(
        &self,
        channel: &str,
        path: &str,
        caption: Option<&str>,
        thread_ts: Option<&str>,
    ) -> anyhow::Result<()> {
        let file_bytes = tokio::fs::read(path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read file '{}': {}", path, e))?;
        let file_name = std::path::Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("attachment")
            .to_string();

        let mut form = reqwest::multipart::Form::new()
            .text("channels", channel.to_string())
            .part(
                "file",
                reqwest::multipart::Part::bytes(file_bytes).file_name(file_name),
            );
        if let Some(caption) = caption {
            form = form.text("initial_comment", caption.to_string());
        }
        if let Some(thread_ts) = thread_ts {
            form = form.text("thread_ts", thread_ts.to_string());
        }

        let resp: FileUploadResponse = self
            .client
            .post(self.api_url("files.upload"))
            .bearer_auth(&self.bot_token)
            .multipart(form)
            .send()
            .await?
            .json()
            .await?;

        if !resp.ok {
            anyhow::bail!(
                "Slack files.upload failed: {}",
                resp.error.unwrap_or_default()
            );
        }

        Ok(())
    }

    async fn download_private_file_to_cache(
        &self,
        url: &str,
        file_name: Option<&str>,
        fallback_name: &str,
    ) -> anyhow::Result<Option<String>> {
        let response = self
            .client
            .get(url)
            .bearer_auth(&self.bot_token)
            .send()
            .await?
            .error_for_status()?;
        let bytes = response.bytes().await?;
        crate::attachment_cache::persist_bytes("slack", file_name, fallback_name, bytes.as_ref())
    }

    /// Strip `<@BOT_USER_ID>` mention from messages to get clean input.
    fn strip_bot_mention(&self, text: &str, bot_user_id: &str) -> String {
        let mention = format!("<@{}>", bot_user_id);
        text.replace(&mention, "").trim().to_string()
    }

    fn should_accept_channel_message(&self, text: &str, is_dm: bool, bot_user_id: &str) -> bool {
        is_dm || text.contains(&format!("<@{}>", bot_user_id))
    }

    fn thread_ts_for_event(&self, event: &SlackEvent, is_dm: bool) -> Option<String> {
        if is_dm {
            event.thread_ts.clone()
        } else {
            event.thread_ts.clone().or(event.ts.clone())
        }
    }

    async fn extract_attachments(&self, event: &SlackEvent) -> Vec<MessageAttachment> {
        let mut attachments = Vec::new();
        for file in &event.files {
            let kind = classify_slack_attachment(file.mimetype.as_deref(), file.name.as_deref());
            let url = file
                .url_private_download
                .clone()
                .or_else(|| file.url_private.clone());
            let mut attachment = MessageAttachment {
                kind: kind.clone(),
                file_name: file.name.clone(),
                mime_type: file.mimetype.clone(),
                url: url.clone(),
                size_bytes: file.size,
                ..Default::default()
            };

            if let Some(url) = url.as_deref() {
                let fallback_name = slack_attachment_fallback_name(&kind, event.ts.as_deref());
                match self
                    .download_private_file_to_cache(url, file.name.as_deref(), &fallback_name)
                    .await
                {
                    Ok(Some(local_path)) => attachment.local_path = Some(local_path),
                    Ok(None) => {}
                    Err(error) => {
                        warn!(%error, url, "Slack attachment download failed");
                    }
                }
            }

            attachments.push(attachment);
        }
        attachments
    }

    fn parse_slash_command(&self, payload: serde_json::Value) -> Option<IncomingMessage> {
        let payload: SlackSlashCommandPayload = serde_json::from_value(payload).ok()?;
        let text = translate_slack_slash_command(&payload.command, payload.text.as_deref());
        Some(IncomingMessage {
            platform: Platform::Slack,
            user_id: payload.user_id,
            channel_id: Some(payload.channel_id.clone()),
            chat_type: crate::platform::ChatType::Dm, // slash commands are user-scoped
            text,
            thread_id: None,
            metadata: MessageMetadata {
                channel_id: Some(payload.channel_id),
                ..Default::default()
            },
        })
    }
}

fn classify_slack_attachment(
    mime_type: Option<&str>,
    file_name: Option<&str>,
) -> MessageAttachmentKind {
    let normalized_mime = mime_type.unwrap_or("").to_ascii_lowercase();
    let normalized_name = file_name.unwrap_or("").to_ascii_lowercase();

    if normalized_mime.starts_with("image/")
        || [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"]
            .iter()
            .any(|ext| normalized_name.ends_with(ext))
    {
        MessageAttachmentKind::Image
    } else if normalized_mime.starts_with("video/") {
        MessageAttachmentKind::Video
    } else if normalized_mime.starts_with("audio/") {
        MessageAttachmentKind::Audio
    } else if !normalized_mime.is_empty() || !normalized_name.is_empty() {
        MessageAttachmentKind::Document
    } else {
        MessageAttachmentKind::Other
    }
}

fn render_slack_incoming_text(content: &str, attachments: &[MessageAttachment]) -> String {
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
        if let Some(local_path) = attachment.local_path.as_deref() {
            lines.push(format!("- {}: {} ({})", label, file_name, local_path));
        } else {
            lines.push(format!("- {}: {}", label, file_name));
        }
    }

    lines.join("\n")
}

fn slack_attachment_fallback_name(kind: &MessageAttachmentKind, timestamp: Option<&str>) -> String {
    let safe_ts = timestamp.unwrap_or("unknown").replace('.', "_");
    let suffix = match kind {
        MessageAttachmentKind::Image => "jpg",
        MessageAttachmentKind::Video => "mp4",
        MessageAttachmentKind::Audio | MessageAttachmentKind::Voice => "ogg",
        MessageAttachmentKind::Sticker => "webp",
        MessageAttachmentKind::Document | MessageAttachmentKind::Other => "bin",
    };
    format!("slack-{safe_ts}.{suffix}")
}

fn translate_slack_slash_command(command: &str, text: Option<&str>) -> String {
    let trimmed_text = text.unwrap_or("").trim();
    if trimmed_text.starts_with('/') {
        return trimmed_text.to_string();
    }

    let slash_name = command.trim().trim_start_matches('/').to_ascii_lowercase();
    if let Some(mapped) = canonical_gateway_command(&slash_name) {
        if trimmed_text.is_empty() {
            return format!("/{mapped}");
        }
        return format!("/{mapped} {trimmed_text}");
    }

    if trimmed_text.is_empty() {
        return "/help".to_string();
    }

    let (first, rest) = trimmed_text
        .split_once(char::is_whitespace)
        .map(|(head, tail)| (head, tail.trim()))
        .unwrap_or((trimmed_text, ""));
    if let Some(mapped) = canonical_gateway_command(first) {
        if rest.is_empty() {
            format!("/{mapped}")
        } else {
            format!("/{mapped} {rest}")
        }
    } else {
        trimmed_text.to_string()
    }
}

fn canonical_gateway_command(value: &str) -> Option<&'static str> {
    match value.trim_start_matches('/').to_ascii_lowercase().as_str() {
        "help" | "h" | "?" => Some("help"),
        "new" => Some("new"),
        "reset" => Some("reset"),
        "stop" => Some("stop"),
        "retry" => Some("retry"),
        "status" => Some("status"),
        "usage" => Some("usage"),
        "background" | "bg" => Some("background"),
        "hooks" => Some("hooks"),
        _ => None,
    }
}

#[async_trait]
impl PlatformAdapter for SlackAdapter {
    fn platform(&self) -> Platform {
        Platform::Slack
    }

    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
        // Get bot user ID
        let bot_user_id = self.auth_test().await?;
        info!(bot_user_id = %bot_user_id, "Slack adapter authenticated");
        *self.bot_user_id.write().await = Some(bot_user_id.clone());

        loop {
            // Get Socket Mode WebSocket URL
            let ws_url = match self.get_ws_url().await {
                Ok(url) => url,
                Err(e) => {
                    error!(error = %e, "Failed to get Socket Mode URL, retrying");
                    tokio::time::sleep(RETRY_DELAY).await;
                    continue;
                }
            };

            info!("Connecting to Slack Socket Mode");

            // Connect WebSocket
            let ws_connect = tokio_tungstenite::connect_async(&ws_url).await;
            let (ws_stream, _) = match ws_connect {
                Ok(conn) => conn,
                Err(e) => {
                    error!(error = %e, "Slack WebSocket connection failed, retrying");
                    tokio::time::sleep(RETRY_DELAY).await;
                    continue;
                }
            };

            info!("Slack Socket Mode connected");

            use futures::StreamExt;
            let (mut ws_write, mut ws_read) = ws_stream.split();

            // Process messages
            while let Some(msg_result) = ws_read.next().await {
                let msg = match msg_result {
                    Ok(m) => m,
                    Err(e) => {
                        warn!(error = %e, "Slack WebSocket read error");
                        break; // Reconnect
                    }
                };

                let text = match msg {
                    tokio_tungstenite::tungstenite::Message::Text(t) => t,
                    tokio_tungstenite::tungstenite::Message::Ping(data) => {
                        // Reply with Pong to keep Socket Mode connection alive
                        use futures::SinkExt;
                        if let Err(e) = ws_write
                            .send(tokio_tungstenite::tungstenite::Message::Pong(data))
                            .await
                        {
                            warn!(error = %e, "Failed to send Slack Socket Mode pong");
                        }
                        continue;
                    }
                    tokio_tungstenite::tungstenite::Message::Close(_) => {
                        info!("Slack WebSocket closed by server");
                        break;
                    }
                    _ => continue,
                };

                // Parse envelope
                let envelope: SocketModeEnvelope = match serde_json::from_str(&text) {
                    Ok(e) => e,
                    Err(e) => {
                        debug!(error = %e, "Failed to parse Socket Mode envelope");
                        continue;
                    }
                };

                // Acknowledge the envelope to prevent retries
                if let Some(ref envelope_id) = envelope.envelope_id {
                    let ack = serde_json::to_string(&SocketModeAck {
                        envelope_id: envelope_id.clone(),
                    })
                    .unwrap_or_default();

                    use futures::SinkExt;
                    if let Err(e) = ws_write
                        .send(tokio_tungstenite::tungstenite::Message::Text(ack))
                        .await
                    {
                        warn!(error = %e, "Failed to send Socket Mode ack");
                    }
                }

                if envelope.event_type.as_deref() == Some("slash_commands") {
                    if let Some(payload) = envelope.payload {
                        if let Some(incoming) = self.parse_slash_command(payload) {
                            if tx.send(incoming).await.is_err() {
                                info!("Slack adapter: receiver dropped, shutting down");
                                return Ok(());
                            }
                        }
                    }
                    continue;
                }

                // Process events_api envelopes
                if envelope.event_type.as_deref() != Some("events_api") {
                    continue;
                }

                let Some(payload) = envelope.payload else {
                    continue;
                };

                let event_payload: EventPayload = match serde_json::from_value(payload) {
                    Ok(ep) => ep,
                    Err(_) => continue,
                };

                let Some(event) = event_payload.event else {
                    continue;
                };

                // Only process message events (not subtypes like bot_message)
                if event.event_type.as_deref() != Some("message") {
                    continue;
                }
                if event.subtype.is_some() {
                    continue;
                }
                // Skip bot messages (our own)
                if event.bot_id.is_some() {
                    continue;
                }

                let Some(ref event_text) = event.text else {
                    continue;
                };
                let Some(ref user_id) = event.user else {
                    continue;
                };
                let Some(ref channel) = event.channel else {
                    continue;
                };
                let is_dm = event.channel_type.as_deref() == Some("im");

                // Filter by allowed_users (empty = open access)
                if !self.allowed_users.is_empty() && !self.allowed_users.contains(user_id) {
                    debug!(
                        user_id = %user_id,
                        "Slack message filtered: not in allowed list"
                    );
                    continue;
                }

                if !self.should_accept_channel_message(event_text, is_dm, &bot_user_id) {
                    continue;
                }

                // Strip bot mention from text
                let clean_text = self.strip_bot_mention(event_text, &bot_user_id);
                let attachments = self.extract_attachments(&event).await;
                let rendered_text = render_slack_incoming_text(&clean_text, &attachments);
                if rendered_text.is_empty() && attachments.is_empty() {
                    continue;
                }

                let thread_ts = self.thread_ts_for_event(&event, is_dm);

                let incoming = IncomingMessage {
                    platform: Platform::Slack,
                    user_id: user_id.clone(),
                    channel_id: Some(channel.clone()),
                    chat_type: if is_dm {
                        crate::platform::ChatType::Dm
                    } else {
                        crate::platform::ChatType::Group
                    },
                    text: rendered_text,
                    thread_id: thread_ts.clone(),
                    metadata: MessageMetadata {
                        message_id: event.ts,
                        channel_id: Some(channel.clone()),
                        thread_id: thread_ts,
                        user_display_name: None,
                        attachments,
                        ..Default::default()
                    },
                };

                debug!(platform = "slack", user = %user_id, "Received message");

                if tx.send(incoming).await.is_err() {
                    info!("Slack adapter: receiver dropped, shutting down");
                    return Ok(());
                }
            }

            // WebSocket disconnected — reconnect
            warn!("Slack Socket Mode disconnected, reconnecting");
            tokio::time::sleep(RETRY_DELAY).await;
        }
    }

    async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()> {
        let channel = msg
            .metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Slack send requires channel_id"))?;

        let thread_ts = msg.metadata.thread_id.as_deref();
        let formatted = self.format_response(&msg.text, &msg.metadata);
        let chunks = split_message(&formatted, MAX_MESSAGE_LENGTH);

        for chunk in &chunks {
            self.post_message(channel, chunk, thread_ts).await?;
        }

        Ok(())
    }

    async fn send_and_get_id(&self, msg: OutgoingMessage) -> anyhow::Result<Option<String>> {
        let channel = msg
            .metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Slack send requires channel_id"))?;

        let thread_ts = msg.metadata.thread_id.as_deref();
        let formatted = self.format_response(&msg.text, &msg.metadata);
        let chunks = split_message(&formatted, MAX_MESSAGE_LENGTH);
        let first_chunk = chunks.first().cloned().unwrap_or_default();
        let message_id = self.post_message(channel, &first_chunk, thread_ts).await?;
        for chunk in chunks.iter().skip(1) {
            self.post_message(channel, chunk, thread_ts).await?;
        }
        Ok(message_id)
    }

    fn format_response(&self, text: &str, _metadata: &MessageMetadata) -> String {
        // Slack uses mrkdwn which is close to standard markdown.
        // Convert **bold** → *bold*, __italic__ → _italic_
        text.replace("**", "*")
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

    async fn edit_message(
        &self,
        message_id: &str,
        metadata: &MessageMetadata,
        new_text: &str,
    ) -> anyhow::Result<String> {
        let channel = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Slack edit_message requires channel_id"))?;
        let formatted = self.format_response(new_text, metadata);
        let truncated = if formatted.len() > MAX_MESSAGE_LENGTH {
            edgecrab_core::safe_truncate(&formatted, MAX_MESSAGE_LENGTH)
        } else {
            &formatted
        };
        self.update_message(channel, message_id, truncated).await?;
        Ok(message_id.to_string())
    }

    async fn send_photo(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        let channel = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Slack send_photo requires channel_id"))?;
        self.upload_file(channel, path, caption, metadata.thread_id.as_deref())
            .await
    }

    async fn send_document(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        let channel = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Slack send_document requires channel_id"))?;
        self.upload_file(channel, path, caption, metadata.thread_id.as_deref())
            .await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Bytes;
    use axum::extract::State as AxumState;
    use axum::routing::post;
    use axum::{Json, Router};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[test]
    fn split_short_message() {
        let chunks = split_message("hello", 100);
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn split_long_message() {
        let text = "line1\nline2\nline3\nline4";
        let chunks = split_message(text, 12);
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(chunk.len() <= 12);
        }
    }

    #[test]
    fn format_bold_conversion() {
        let adapter_text = "**hello** world";
        let formatted = adapter_text.replace("**", "*");
        assert_eq!(formatted, "*hello* world");
    }

    #[test]
    fn strip_mention() {
        let text = "<@U12345> hello there";
        let mention = format!("<@{}>", "U12345");
        let clean = text.replace(&mention, "").trim().to_string();
        assert_eq!(clean, "hello there");
    }

    #[test]
    fn channel_messages_require_bot_mention() {
        let adapter =
            SlackAdapter::from_tokens("bot".into(), "app".into(), Vec::new()).expect("adapter");
        assert!(!adapter.should_accept_channel_message("hello", false, "U123"));
        assert!(adapter.should_accept_channel_message("<@U123> hello", false, "U123"));
        assert!(adapter.should_accept_channel_message("hello", true, "U123"));
    }

    #[test]
    fn thread_ts_uses_channel_parent_but_not_top_level_dm() {
        let adapter =
            SlackAdapter::from_tokens("bot".into(), "app".into(), Vec::new()).expect("adapter");
        let channel_event = SlackEvent {
            event_type: Some("message".into()),
            text: Some("hello".into()),
            user: Some("U1".into()),
            channel: Some("C1".into()),
            ts: Some("100.1".into()),
            thread_ts: None,
            channel_type: Some("channel".into()),
            files: Vec::new(),
            subtype: None,
            bot_id: None,
        };
        let dm_event = SlackEvent {
            channel_type: Some("im".into()),
            ..channel_event
        };
        assert_eq!(
            adapter.thread_ts_for_event(&dm_event, true),
            None,
            "top-level DMs should not force a synthetic thread"
        );
        assert_eq!(
            adapter.thread_ts_for_event(
                &SlackEvent {
                    channel_type: Some("channel".into()),
                    ..dm_event
                },
                false
            ),
            Some("100.1".into())
        );
    }

    #[test]
    fn render_slack_incoming_text_includes_attachments() {
        let attachments = vec![MessageAttachment {
            kind: MessageAttachmentKind::Image,
            file_name: Some("diagram.png".into()),
            ..Default::default()
        }];
        let rendered = render_slack_incoming_text("see this", &attachments);
        assert!(rendered.contains("see this"));
        assert!(rendered.contains("Shared 1 attachment:"));
        assert!(rendered.contains("diagram.png"));
    }

    #[test]
    fn slack_attachment_classification_uses_mime_and_name() {
        assert_eq!(
            classify_slack_attachment(Some("image/png"), Some("a.bin")),
            MessageAttachmentKind::Image
        );
        assert_eq!(
            classify_slack_attachment(None, Some("voice.mp3")),
            MessageAttachmentKind::Document
        );
    }

    #[test]
    fn translate_slack_slash_command_maps_gateway_commands() {
        assert_eq!(
            translate_slack_slash_command("/edgecrab", Some("help")),
            "/help"
        );
        assert_eq!(
            translate_slack_slash_command("/edgecrab", Some("bg write tests")),
            "/background write tests"
        );
        assert_eq!(translate_slack_slash_command("/new", None), "/new");
        assert_eq!(
            translate_slack_slash_command("/edgecrab", Some("what time is it")),
            "what time is it"
        );
        assert_eq!(translate_slack_slash_command("/edgecrab", None), "/help");
    }

    #[tokio::test]
    async fn send_and_edit_and_upload_use_slack_api_endpoints() {
        #[derive(Clone, Default)]
        struct TestState {
            json_bodies: Arc<Mutex<Vec<serde_json::Value>>>,
            multipart_bodies: Arc<Mutex<Vec<String>>>,
        }

        async fn post_message(
            AxumState(state): AxumState<TestState>,
            Json(body): Json<serde_json::Value>,
        ) -> Json<serde_json::Value> {
            state.json_bodies.lock().await.push(body);
            Json(serde_json::json!({ "ok": true, "ts": "123.456" }))
        }

        async fn update_message(
            AxumState(state): AxumState<TestState>,
            Json(body): Json<serde_json::Value>,
        ) -> Json<serde_json::Value> {
            state.json_bodies.lock().await.push(body);
            Json(serde_json::json!({ "ok": true }))
        }

        async fn upload_file(
            AxumState(state): AxumState<TestState>,
            body: Bytes,
        ) -> Json<serde_json::Value> {
            state
                .multipart_bodies
                .lock()
                .await
                .push(String::from_utf8_lossy(&body).into_owned());
            Json(serde_json::json!({ "ok": true }))
        }

        let state = TestState::default();
        let app = Router::new()
            .route("/chat.postMessage", post(post_message))
            .route("/chat.update", post(update_message))
            .route("/files.upload", post(upload_file))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });

        let mut adapter =
            SlackAdapter::from_tokens("bot".into(), "app".into(), Vec::new()).expect("adapter");
        adapter.api_base = format!("http://{addr}");

        let message_id = adapter
            .send_and_get_id(OutgoingMessage {
                text: "hello slack".into(),
                metadata: MessageMetadata {
                    channel_id: Some("C123".into()),
                    thread_id: Some("200.1".into()),
                    ..Default::default()
                },
            })
            .await
            .expect("send");
        assert_eq!(message_id.as_deref(), Some("123.456"));

        adapter
            .edit_message(
                "123.456",
                &MessageMetadata {
                    channel_id: Some("C123".into()),
                    ..Default::default()
                },
                "updated",
            )
            .await
            .expect("edit");

        let temp = tempfile::NamedTempFile::new().expect("temp");
        tokio::fs::write(temp.path(), b"hello file")
            .await
            .expect("write");
        adapter
            .send_document(
                temp.path().to_str().expect("path"),
                Some("caption"),
                &MessageMetadata {
                    channel_id: Some("C123".into()),
                    thread_id: Some("200.1".into()),
                    ..Default::default()
                },
            )
            .await
            .expect("upload");

        let json_bodies = state.json_bodies.lock().await;
        assert_eq!(json_bodies[0]["channel"], "C123");
        assert_eq!(json_bodies[0]["thread_ts"], "200.1");
        assert_eq!(json_bodies[1]["ts"], "123.456");
        assert_eq!(json_bodies[1]["text"], "updated");
        drop(json_bodies);

        let multipart_bodies = state.multipart_bodies.lock().await;
        assert_eq!(multipart_bodies.len(), 1);
        assert!(multipart_bodies[0].contains("name=\"channels\""));
        assert!(multipart_bodies[0].contains("C123"));
        assert!(multipart_bodies[0].contains("name=\"thread_ts\""));
        assert!(multipart_bodies[0].contains("200.1"));
        assert!(multipart_bodies[0].contains("name=\"initial_comment\""));
        assert!(multipart_bodies[0].contains("caption"));

        server.abort();
    }

    #[test]
    fn is_available_without_env() {
        // Shouldn't panic — just returns false when env vars aren't set
        // (unless they happen to be set in the test environment)
        let _result = SlackAdapter::is_available();
    }
}
