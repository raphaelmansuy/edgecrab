//! # BlueBubbles adapter — iMessage gateway
//!
//! Connects to a self-hosted [BlueBubbles](https://bluebubbles.app/) server
//! to send and receive iMessages via REST API + webhook notifications.
//!
//! ## Environment variables
//!
//! | Variable                          | Required | Default        |
//! |-----------------------------------|----------|----------------|
//! | `BLUEBUBBLES_SERVER_URL`          | Yes      | —              |
//! | `BLUEBUBBLES_PASSWORD`            | Yes      | —              |
//! | `BLUEBUBBLES_WEBHOOK_HOST`        | No       | `127.0.0.1`    |
//! | `BLUEBUBBLES_WEBHOOK_PORT`        | No       | `8645`         |
//! | `BLUEBUBBLES_WEBHOOK_PATH`        | No       | `/bb-webhook`  |
//! | `BLUEBUBBLES_SEND_READ_RECEIPTS`  | No       | `true`         |
//! | `BLUEBUBBLES_ALLOWED_USERS`       | No       | —              |
//!
//! ## Limits
//!
//! - Max message length: **4000** characters (iMessage practical limit)

use std::env;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::{
    Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use dashmap::DashMap;
use edgecrab_types::Platform;
use serde::Deserialize;
use subtle::ConstantTimeEq;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::platform::{
    IncomingMessage, MessageAttachment, MessageAttachmentKind, MessageMetadata, OutgoingMessage,
    PlatformAdapter,
};

const MAX_MESSAGE_LENGTH: usize = 4000;

/// Tapback reaction subtypes: 2000..3005 are Apple tapback reactions.
const TAPBACK_RANGE: std::ops::RangeInclusive<i64> = 2000..=3005;

pub struct BlueBubblesAdapter {
    server_url: String,
    password: String,
    webhook_host: String,
    webhook_port: u16,
    webhook_path: String,
    send_read_receipts: bool,
    private_api_enabled: Arc<std::sync::atomic::AtomicBool>,
    client: reqwest::Client,
    guid_cache: DashMap<String, String>,
    shutdown: CancellationToken,
}

impl BlueBubblesAdapter {
    pub fn from_env() -> Option<Self> {
        let server_url = env::var("BLUEBUBBLES_SERVER_URL").ok()?;
        let password = env::var("BLUEBUBBLES_PASSWORD").ok()?;

        let webhook_host =
            env::var("BLUEBUBBLES_WEBHOOK_HOST").unwrap_or_else(|_| "127.0.0.1".into());
        let webhook_port: u16 = env::var("BLUEBUBBLES_WEBHOOK_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8645);
        let webhook_path =
            env::var("BLUEBUBBLES_WEBHOOK_PATH").unwrap_or_else(|_| "/bb-webhook".into());
        let send_read_receipts = env::var("BLUEBUBBLES_SEND_READ_RECEIPTS")
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .ok()?;

        Some(Self {
            server_url: server_url.trim_end_matches('/').to_string(),
            password,
            webhook_host,
            webhook_port,
            webhook_path,
            send_read_receipts,
            private_api_enabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            client,
            guid_cache: DashMap::new(),
            shutdown: CancellationToken::new(),
        })
    }

    pub fn is_available() -> bool {
        env::var("BLUEBUBBLES_SERVER_URL").is_ok() && env::var("BLUEBUBBLES_PASSWORD").is_ok()
    }

    /// Build API URL with password in query params.
    fn api_url(&self, path: &str) -> String {
        let sep = if path.contains('?') { '&' } else { '?' };
        format!(
            "{}/api/v1{path}{sep}password={}",
            self.server_url,
            urlencoding::encode(&self.password)
        )
    }

    /// Ping the BlueBubbles server to verify connectivity.
    async fn ping(&self) -> anyhow::Result<()> {
        let resp = self.client.get(self.api_url("/ping")).send().await?;

        if !resp.status().is_success() {
            anyhow::bail!("BlueBubbles ping failed: HTTP {}", resp.status());
        }
        debug!("BlueBubbles server ping OK");
        Ok(())
    }

    /// Detect Private API availability from server info.
    async fn detect_private_api(&self) -> anyhow::Result<bool> {
        let resp = self.client.get(self.api_url("/server/info")).send().await?;

        let info: serde_json::Value = resp.json().await?;
        let private_api = info["data"]["private_api_enabled"]
            .as_bool()
            .unwrap_or(false);
        let helper = info["data"]["helper_connected"].as_bool().unwrap_or(false);

        let enabled = private_api && helper;
        self.private_api_enabled
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
        info!(private_api = enabled, "BlueBubbles Private API status");
        Ok(enabled)
    }

    /// Register our webhook URL with the BlueBubbles server.
    async fn register_webhook(&self) -> anyhow::Result<()> {
        let callback_url = format!(
            "http://{}:{}{}",
            self.webhook_host, self.webhook_port, self.webhook_path
        );

        // Clean up existing webhooks first (crash recovery).
        let list_resp = self.client.get(self.api_url("/webhook")).send().await?;

        if list_resp.status().is_success() {
            let list: serde_json::Value = list_resp.json().await?;
            if let Some(webhooks) = list["data"].as_array() {
                for wh in webhooks {
                    if let Some(url) = wh["url"].as_str()
                        && url == callback_url
                        && let Some(id) = wh["id"].as_i64()
                    {
                        let _ = self
                            .client
                            .delete(self.api_url(&format!("/webhook/{id}")))
                            .send()
                            .await;
                        debug!(id, "Removed stale webhook");
                    }
                }
            }
        }

        // Register new webhook.
        let body = serde_json::json!({
            "url": callback_url,
            "events": ["new-message", "updated-message", "message"],
        });

        let resp = self
            .client
            .post(self.api_url("/webhook"))
            .json(&body)
            .send()
            .await?;

        if resp.status().is_success() {
            info!(url = callback_url, "BlueBubbles webhook registered");
        } else {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            warn!("BlueBubbles webhook registration failed {status}: {text}");
        }

        Ok(())
    }

    /// Unregister all matching webhooks on shutdown.
    async fn unregister_webhooks(&self) {
        let callback_url = format!(
            "http://{}:{}{}",
            self.webhook_host, self.webhook_port, self.webhook_path
        );

        let Ok(resp) = self.client.get(self.api_url("/webhook")).send().await else {
            return;
        };
        let Ok(list): Result<serde_json::Value, _> = resp.json().await else {
            return;
        };

        if let Some(webhooks) = list["data"].as_array() {
            for wh in webhooks {
                if wh["url"].as_str() == Some(&callback_url)
                    && let Some(id) = wh["id"].as_i64()
                {
                    let _ = self
                        .client
                        .delete(self.api_url(&format!("/webhook/{id}")))
                        .send()
                        .await;
                    debug!(id, "Unregistered webhook on shutdown");
                }
            }
        }
    }

    /// Resolve a target address to a chat GUID.
    async fn resolve_chat_guid(&self, target: &str) -> anyhow::Result<String> {
        // Check cache first.
        if let Some(guid) = self.guid_cache.get(target) {
            return Ok(guid.clone());
        }

        // Search existing chats.
        let resp = self
            .client
            .get(self.api_url(&format!(
                "/chat?limit=50&with=participants&chatIdentifier={target}"
            )))
            .send()
            .await?;

        if resp.status().is_success() {
            let data: serde_json::Value = resp.json().await?;
            if let Some(chats) = data["data"].as_array() {
                for chat in chats {
                    if let Some(guid) = chat["guid"].as_str() {
                        self.guid_cache.insert(target.to_string(), guid.to_string());
                        return Ok(guid.to_string());
                    }
                }
            }
        }

        // Fallback: create new chat if Private API is available.
        if self
            .private_api_enabled
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            let body = serde_json::json!({
                "participants": [target],
            });
            let resp = self
                .client
                .post(self.api_url("/chat/new"))
                .json(&body)
                .send()
                .await?;

            if resp.status().is_success() {
                let data: serde_json::Value = resp.json().await?;
                if let Some(guid) = data["data"]["guid"].as_str() {
                    self.guid_cache.insert(target.to_string(), guid.to_string());
                    return Ok(guid.to_string());
                }
            }
        }

        anyhow::bail!("Could not resolve chat GUID for {target}")
    }

    /// Send a text message via the BlueBubbles REST API.
    async fn send_text(&self, chat_guid: &str, text: &str) -> anyhow::Result<()> {
        let body = serde_json::json!({
            "chatGuid": chat_guid,
            "message": text,
            "method": if self.private_api_enabled.load(std::sync::atomic::Ordering::Relaxed) { "private-api" } else { "apple-script" },
        });

        let resp = self
            .client
            .post(self.api_url("/message/text"))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("BlueBubbles send failed {status}: {text}");
        }
        Ok(())
    }

    /// Send a file as an attachment.
    async fn send_attachment(
        &self,
        chat_guid: &str,
        file_path: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let file_bytes = tokio::fs::read(file_path).await?;
        let file_name = std::path::Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        let part = reqwest::multipart::Part::bytes(file_bytes).file_name(file_name.to_string());

        let form = reqwest::multipart::Form::new()
            .text("chatGuid", chat_guid.to_string())
            .text("name", file_name.to_string())
            .part("attachment", part);

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;

        let resp = client
            .post(self.api_url("/message/attachment"))
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("BlueBubbles attachment send failed {status}: {text}");
        }

        if let Some(cap) = caption {
            self.send_text(chat_guid, cap).await?;
        }

        Ok(())
    }

    /// Mark a message as read (send read receipt).
    async fn mark_read(&self, chat_guid: &str) {
        if !self.send_read_receipts
            || !self
                .private_api_enabled
                .load(std::sync::atomic::Ordering::Relaxed)
        {
            return;
        }
        let body = serde_json::json!({
            "chatGuid": chat_guid,
        });
        let _ = self
            .client
            .post(self.api_url("/chat/read"))
            .json(&body)
            .send()
            .await;
    }

    /// Strip markdown formatting (iMessage doesn't support it).
    fn strip_markdown(text: &str) -> String {
        let mut result = String::with_capacity(text.len());
        let mut in_code_fence = false;

        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                in_code_fence = !in_code_fence;
                // Keep code content but remove fence markers
                continue;
            }
            if in_code_fence {
                result.push_str(line);
                result.push('\n');
                continue;
            }

            let mut processed = line.to_string();
            // Headers: ### → nothing, ## → nothing, # → nothing
            for prefix in &["### ", "## ", "# "] {
                if processed.starts_with(prefix) {
                    processed = processed[prefix.len()..].to_string();
                    break;
                }
            }
            // Links: [text](url) → text
            while let Some(start) = processed.find('[') {
                if let Some(mid) = processed[start..].find("](")
                    && let Some(end) = processed[start + mid..].find(')')
                {
                    let link_text = &processed[start + 1..start + mid];
                    let before = &processed[..start];
                    let after = &processed[start + mid + end + 1..];
                    processed = format!("{before}{link_text}{after}");
                    continue;
                }
                break;
            }
            // Bold/italic: **text** → text, __text__ → text
            processed = processed.replace("**", "");
            processed = processed.replace("__", "");
            // Strikethrough: ~~text~~ → text
            processed = processed.replace("~~", "");
            // Italic: *text* and _text_ — only single markers not already removed
            // Use simple replace for remaining single markers
            processed = processed.replace('`', "");
            result.push_str(&processed);
            result.push('\n');
        }

        // Remove trailing newline
        if result.ends_with('\n') {
            result.pop();
        }
        result
    }
}

/// Shared state for the webhook handler.
#[derive(Clone)]
#[allow(dead_code)]
struct WebhookState {
    password: String,
    server_url: String,
    client: reqwest::Client,
    tx: mpsc::Sender<IncomingMessage>,
    private_api: bool,
}

/// Webhook JSON payload from BlueBubbles.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct WebhookPayload {
    #[serde(rename = "type")]
    event_type: Option<String>,
    data: Option<serde_json::Value>,
}

/// A BlueBubbles message record.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct BBMessage {
    guid: Option<String>,
    text: Option<String>,
    #[serde(rename = "isFromMe")]
    is_from_me: Option<bool>,
    #[serde(rename = "associatedMessageType")]
    associated_message_type: Option<i64>,
    #[serde(rename = "chatIdentifier")]
    chat_identifier: Option<String>,
    #[serde(rename = "chatGuid")]
    chat_guid: Option<String>,
    handle: Option<BBHandle>,
    attachments: Option<Vec<BBAttachment>>,
}

#[derive(Debug, Deserialize)]
struct BBHandle {
    address: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct BBAttachment {
    guid: Option<String>,
    #[serde(rename = "mimeType")]
    mime_type: Option<String>,
    #[serde(rename = "transferName")]
    transfer_name: Option<String>,
}

async fn health_handler() -> &'static str {
    "ok"
}

async fn webhook_handler(
    State(state): State<Arc<WebhookState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
    headers: axum::http::HeaderMap,
    body: axum::extract::Json<WebhookPayload>,
) -> StatusCode {
    // Authenticate: check password from query, header, or X-BlueBubbles-GUID.
    let provided_pw = params
        .get("password")
        .cloned()
        .or_else(|| {
            headers
                .get("X-Password")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();

    if !bool::from(provided_pw.as_bytes().ct_eq(state.password.as_bytes())) {
        return StatusCode::UNAUTHORIZED;
    }

    let Some(data) = &body.data else {
        return StatusCode::OK;
    };

    // Parse message.
    let msg: BBMessage = match serde_json::from_value(data.clone()) {
        Ok(m) => m,
        Err(e) => {
            debug!("BlueBubbles webhook: failed to parse message: {e}");
            return StatusCode::OK;
        }
    };

    // Skip own messages.
    if msg.is_from_me.unwrap_or(false) {
        return StatusCode::OK;
    }

    // Skip tapback reactions.
    if let Some(assoc_type) = msg.associated_message_type
        && TAPBACK_RANGE.contains(&assoc_type)
    {
        debug!("Skipping tapback reaction type {assoc_type}");
        return StatusCode::OK;
    }

    let text = msg.text.unwrap_or_default();
    if text.is_empty() && msg.attachments.as_ref().is_none_or(|a| a.is_empty()) {
        return StatusCode::OK;
    }

    let sender = msg
        .handle
        .as_ref()
        .and_then(|h| h.address.clone())
        .or(msg.chat_identifier.clone())
        .unwrap_or_else(|| "unknown".to_string());

    // Redact PII from logs.
    let safe_sender = redact_pii(&sender);
    info!(from = safe_sender, "BlueBubbles message received");

    let chat_id = msg
        .chat_guid
        .or(msg.chat_identifier)
        .unwrap_or_else(|| sender.clone());

    // Download inbound attachments
    let mut attachments = Vec::new();
    if let Some(att_list) = &msg.attachments {
        for att in att_list {
            if let Some(guid) = &att.guid {
                let dl_url = format!(
                    "{}/api/v1/attachment/{}/download?password={}",
                    state.server_url,
                    urlencoding::encode(guid),
                    urlencoding::encode(&state.password),
                );
                match state.client.get(&dl_url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let fname = att.transfer_name.clone().unwrap_or_else(|| guid.clone());
                        let dir = std::env::temp_dir().join("edgecrab_bb_attachments");
                        let _ = tokio::fs::create_dir_all(&dir).await;
                        let dest = dir.join(&fname);
                        if let Ok(bytes) = resp.bytes().await
                            && tokio::fs::write(&dest, &bytes).await.is_ok()
                        {
                            let kind = match att.mime_type.as_deref() {
                                Some(m) if m.starts_with("image/") => MessageAttachmentKind::Image,
                                Some(m) if m.starts_with("audio/") => MessageAttachmentKind::Audio,
                                Some(m) if m.starts_with("video/") => MessageAttachmentKind::Video,
                                _ => MessageAttachmentKind::Document,
                            };
                            attachments.push(MessageAttachment {
                                kind,
                                file_name: Some(fname),
                                local_path: Some(dest.display().to_string()),
                                ..Default::default()
                            });
                        }
                    }
                    Ok(resp) => {
                        debug!(
                            "BlueBubbles attachment download failed: HTTP {}",
                            resp.status()
                        );
                    }
                    Err(e) => {
                        debug!("BlueBubbles attachment download error: {e}");
                    }
                }
            }
        }
    }

    let incoming = IncomingMessage {
        platform: Platform::BlueBubbles,
        user_id: sender,
        channel_id: Some(chat_id.clone()),
        chat_type: crate::platform::ChatType::Dm,
        text,
        thread_id: None,
        metadata: MessageMetadata {
            channel_id: Some(chat_id),
            attachments,
            ..Default::default()
        },
    };

    if let Err(e) = state.tx.send(incoming).await {
        error!("Failed to forward BlueBubbles message: {e}");
    }

    StatusCode::OK
}

/// Redact phone numbers and emails from log output.
fn redact_pii(s: &str) -> String {
    // Phone: +1234567890 or similar
    let phone_re = regex::Regex::new(r"\+?\d{7,15}").expect("valid regex");
    let email_re =
        regex::Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").expect("valid regex");
    let redacted = phone_re.replace_all(s, "[REDACTED_PHONE]");
    email_re
        .replace_all(&redacted, "[REDACTED_EMAIL]")
        .to_string()
}

#[async_trait]
impl PlatformAdapter for BlueBubblesAdapter {
    fn platform(&self) -> Platform {
        Platform::BlueBubbles
    }

    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
        // 1. Verify server connectivity.
        self.ping().await?;

        // 2. Detect Private API support.
        if let Err(e) = self.detect_private_api().await {
            warn!("BlueBubbles: failed to detect private API: {e}");
        }

        // 3. Register webhook (with crash recovery cleanup).
        self.register_webhook().await?;

        // 4. Start webhook server.
        let state = Arc::new(WebhookState {
            password: self.password.clone(),
            server_url: self.server_url.clone(),
            client: self.client.clone(),
            tx,
            private_api: self
                .private_api_enabled
                .load(std::sync::atomic::Ordering::Relaxed),
        });

        let app = Router::new()
            .route("/health", get(health_handler))
            .route(&self.webhook_path, post(webhook_handler))
            .with_state(state);

        let addr = format!("{}:{}", self.webhook_host, self.webhook_port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        info!(addr = addr, "BlueBubbles webhook server started");

        let shutdown = self.shutdown.clone();
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                shutdown.cancelled().await;
            })
            .await?;

        // Cleanup on shutdown.
        self.unregister_webhooks().await;

        Ok(())
    }

    async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()> {
        let chat_guid = self
            .resolve_chat_guid(msg.metadata.channel_id.as_deref().unwrap_or(""))
            .await?;
        let text = Self::strip_markdown(&msg.text);

        // Split long messages.
        let chunks = split_text(&text, MAX_MESSAGE_LENGTH);
        for chunk in chunks {
            self.send_text(&chat_guid, &chunk).await?;
        }

        // Mark as read.
        self.mark_read(&chat_guid).await;

        Ok(())
    }

    fn format_response(&self, text: &str, _metadata: &MessageMetadata) -> String {
        Self::strip_markdown(text)
    }

    fn max_message_length(&self) -> usize {
        MAX_MESSAGE_LENGTH
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

    fn supports_editing(&self) -> bool {
        false // iMessage does not support message editing
    }

    async fn send_typing(&self, metadata: &MessageMetadata) -> anyhow::Result<()> {
        if !self
            .private_api_enabled
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return Ok(());
        }
        let chat_guid = self
            .resolve_chat_guid(metadata.channel_id.as_deref().unwrap_or(""))
            .await?;
        let body = serde_json::json!({
            "chatGuid": chat_guid,
        });
        let _ = self
            .client
            .post(self.api_url("/chat/typing"))
            .json(&body)
            .send()
            .await;
        Ok(())
    }

    async fn send_photo(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        let chat_guid = self
            .resolve_chat_guid(metadata.channel_id.as_deref().unwrap_or(""))
            .await?;
        self.send_attachment(&chat_guid, path, caption).await
    }

    async fn send_document(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        let chat_guid = self
            .resolve_chat_guid(metadata.channel_id.as_deref().unwrap_or(""))
            .await?;
        self.send_attachment(&chat_guid, path, caption).await
    }

    async fn send_voice(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        let chat_guid = self
            .resolve_chat_guid(metadata.channel_id.as_deref().unwrap_or(""))
            .await?;
        self.send_attachment(&chat_guid, path, caption).await
    }
}

/// Split text into chunks respecting word boundaries.
fn split_text(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            chunks.push(remaining.to_string());
            break;
        }

        // Find last space before max_len.
        let split_at = remaining[..max_len].rfind(' ').unwrap_or(max_len);

        chunks.push(remaining[..split_at].to_string());
        remaining = remaining[split_at..].trim_start();
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_env_missing() {
        // Clear vars to ensure None.
        unsafe { std::env::remove_var("BLUEBUBBLES_SERVER_URL") };
        unsafe { std::env::remove_var("BLUEBUBBLES_PASSWORD") };
        assert!(BlueBubblesAdapter::from_env().is_none());
    }

    #[test]
    fn test_strip_markdown() {
        assert_eq!(
            BlueBubblesAdapter::strip_markdown("**bold** and `code`"),
            "bold and code"
        );
        assert_eq!(BlueBubblesAdapter::strip_markdown("# Header"), "Header");
    }

    #[test]
    fn test_skip_tapback() {
        assert!(TAPBACK_RANGE.contains(&2000));
        assert!(TAPBACK_RANGE.contains(&3005));
        assert!(!TAPBACK_RANGE.contains(&1999));
        assert!(!TAPBACK_RANGE.contains(&3006));
    }

    #[test]
    fn test_message_split() {
        let text = "a ".repeat(2500);
        let chunks = split_text(&text, 4000);
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(chunk.len() <= 4000);
        }
    }

    #[test]
    fn test_message_no_split_needed() {
        let chunks = split_text("hello world", 4000);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "hello world");
    }

    #[test]
    fn test_redact_pii_phone() {
        assert_eq!(redact_pii("+14155551234"), "[REDACTED_PHONE]");
    }

    #[test]
    fn test_redact_pii_email() {
        assert_eq!(redact_pii("user@example.com"), "[REDACTED_EMAIL]");
    }

    #[test]
    fn test_redact_pii_mixed() {
        let input = "Message from +14155551234 and user@example.com";
        let result = redact_pii(input);
        assert!(!result.contains("+14155551234"));
        assert!(!result.contains("user@example.com"));
    }
}
