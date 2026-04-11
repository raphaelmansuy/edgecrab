//! # Signal messenger adapter
//!
//! Connects to a signal-cli daemon running in HTTP mode.
//! Inbound messages arrive via SSE (Server-Sent Events).
//! Outbound messages use JSON-RPC 2.0 over HTTP.
//!
//! ```text
//!   SignalAdapter
//!     ├── start()           → SSE listener for inbound messages
//!     ├── send()            → JSON-RPC send() call
//!     ├── format_response() → plain text (Signal doesn't support markdown)
//!     └── is_available()    → checks SIGNAL_HTTP_URL + SIGNAL_ACCOUNT
//! ```
//!
//! ## Environment variables
//!
//! | Variable           | Required | Description                                |
//! |--------------------|----------|--------------------------------------------|
//! | `SIGNAL_HTTP_URL`  | Yes      | URL of signal-cli HTTP daemon              |
//! | `SIGNAL_ACCOUNT`   | Yes      | Phone number registered with signal-cli    |
//!
//! ## Setup
//!
//! ```bash
//! signal-cli daemon --http 127.0.0.1:8080
//! ```
//!
//! ## Limits
//!
//! - Max message length: **8000** characters
//! - No markdown support (plain text only)

use std::env;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine as _;
use edgecrab_types::Platform;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::delivery::split_message;
use crate::platform::{
    IncomingMessage, MessageAttachment, MessageAttachmentKind, MessageMetadata, OutgoingMessage,
    PlatformAdapter,
};

/// Maximum message length for Signal.
const MAX_MESSAGE_LENGTH: usize = 8000;

/// Delay between SSE reconnection attempts (alias of `crate::ADAPTER_RETRY_DELAY`).
const SSE_RETRY_DELAY: Duration = crate::ADAPTER_RETRY_DELAY;

/// Maximum SSE retry delay (alias of `crate::ADAPTER_MAX_RETRY_DELAY`).
const SSE_RETRY_DELAY_MAX: Duration = crate::ADAPTER_MAX_RETRY_DELAY;

/// Monotonic JSON-RPC request ids for Signal daemon calls.
static JSON_RPC_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

// ---------------------------------------------------------------------------
// Signal API types
// ---------------------------------------------------------------------------

/// JSON-RPC 2.0 request.
#[derive(Debug, Serialize)]
struct JsonRpcRequest<'a> {
    jsonrpc: &'a str,
    method: &'a str,
    params: serde_json::Value,
    id: u64,
}

/// JSON-RPC 2.0 response.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    #[allow(dead_code)]
    id: Option<u64>,
    #[serde(default)]
    result: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    #[allow(dead_code)]
    code: Option<i64>,
    message: Option<String>,
}

/// Signal SSE event data envelope.
#[derive(Debug, Deserialize)]
struct SignalEnvelope {
    #[serde(default)]
    envelope: Option<SignalEnvelopeInner>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignalEnvelopeInner {
    #[serde(default)]
    source_number: Option<String>,
    #[serde(default)]
    source_uuid: Option<String>,
    #[serde(default)]
    source_device: Option<u32>,
    #[serde(default)]
    data_message: Option<SignalDataMessage>,
    #[serde(default)]
    sync_message: Option<SignalSyncMessage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignalDataMessage {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    group_info: Option<SignalGroupInfo>,
    #[serde(default)]
    timestamp: Option<u64>,
    #[serde(default)]
    attachments: Vec<SignalAttachment>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignalSyncMessage {
    #[serde(default)]
    sent_message: Option<SignalSentMessage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignalSentMessage {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    group_info: Option<SignalGroupInfo>,
    #[serde(default)]
    timestamp: Option<u64>,
    #[serde(default)]
    attachments: Vec<SignalAttachment>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignalAttachment {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    content_type: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    voice_note: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignalGroupInfo {
    #[serde(default)]
    group_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SignalTarget {
    Direct(String),
    Group(String),
}

// ---------------------------------------------------------------------------
// Helper: redact phone for logging
// ---------------------------------------------------------------------------

fn redact_phone(phone: &str) -> String {
    if phone.len() <= 8 {
        return "****".into();
    }
    let head = edgecrab_core::safe_truncate(phone, 4);
    let tail_start = edgecrab_core::safe_char_start(phone, phone.len().saturating_sub(4));
    format!("{head}****{}", &phone[tail_start..])
}

fn classify_signal_attachment(attachment: &SignalAttachment) -> MessageAttachmentKind {
    if attachment.voice_note.unwrap_or(false) {
        return MessageAttachmentKind::Voice;
    }

    let content_type = attachment
        .content_type
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if content_type.starts_with("image/") {
        return MessageAttachmentKind::Image;
    }
    if content_type.starts_with("video/") {
        return MessageAttachmentKind::Video;
    }
    if content_type.starts_with("audio/") {
        return MessageAttachmentKind::Audio;
    }

    if let Some(file_name) = attachment.filename.as_deref() {
        if let Some(extension) = file_name.rsplit('.').next() {
            return match extension.to_ascii_lowercase().as_str() {
                "jpg" | "jpeg" | "png" | "gif" | "webp" => MessageAttachmentKind::Image,
                "mp4" | "mov" | "avi" | "mkv" | "webm" => MessageAttachmentKind::Video,
                "ogg" | "opus" | "mp3" | "wav" | "m4a" | "aac" => MessageAttachmentKind::Audio,
                "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "csv"
                | "zip" => MessageAttachmentKind::Document,
                _ => MessageAttachmentKind::Other,
            };
        }
    }

    MessageAttachmentKind::Other
}

fn compact_phone_candidate(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_ascii_whitespace() && !matches!(c, '-' | '(' | ')'))
        .collect()
}

fn normalize_signal_phone(value: &str) -> Option<String> {
    let compact = compact_phone_candidate(value);
    if compact.is_empty() {
        return None;
    }

    if let Some(rest) = compact.strip_prefix('+') {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("+{rest}"));
        }
        return None;
    }

    if let Some(rest) = compact.strip_prefix("00") {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("+{rest}"));
        }
        return None;
    }

    if compact.chars().all(|c| c.is_ascii_digit()) {
        return Some(format!("+{compact}"));
    }

    None
}

fn normalize_signal_identity(value: &str) -> String {
    normalize_signal_phone(value).unwrap_or_else(|| value.trim().to_string())
}

fn normalize_signal_target(value: &str) -> anyhow::Result<SignalTarget> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        anyhow::bail!("Signal delivery requires a non-empty recipient");
    }

    if let Some(group_id) = trimmed.strip_prefix("group:") {
        let normalized_group_id = group_id.trim();
        if normalized_group_id.is_empty() {
            anyhow::bail!("Signal group target is missing its group id");
        }
        return Ok(SignalTarget::Group(normalized_group_id.to_string()));
    }

    if let Some(phone) = normalize_signal_phone(trimmed) {
        return Ok(SignalTarget::Direct(phone));
    }

    if trimmed.chars().any(|c| c.is_ascii_whitespace()) {
        anyhow::bail!(
            "Signal recipient '{}' is invalid; use an E.164 phone number or group:<group_id>",
            value
        );
    }

    Ok(SignalTarget::Group(trimmed.to_string()))
}

fn signal_attachment_to_metadata(attachment: &SignalAttachment) -> MessageAttachment {
    MessageAttachment {
        kind: classify_signal_attachment(attachment),
        file_name: attachment.filename.clone(),
        mime_type: attachment.content_type.clone(),
        url: attachment
            .id
            .as_ref()
            .map(|id| format!("signal://attachment/{id}")),
        size_bytes: attachment.size,
        ..Default::default()
    }
}

fn signal_extension_from_file_name(file_name: &str) -> Option<String> {
    let extension = std::path::Path::new(file_name)
        .extension()
        .and_then(|value| value.to_str())?
        .trim();
    if extension.is_empty() {
        None
    } else {
        Some(format!(".{}", extension.to_ascii_lowercase()))
    }
}

fn signal_extension_from_content_type(content_type: &str) -> Option<&'static str> {
    match content_type.trim().to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => Some(".jpg"),
        "image/png" => Some(".png"),
        "image/gif" => Some(".gif"),
        "image/webp" => Some(".webp"),
        "image/heic" => Some(".heic"),
        "image/heif" => Some(".heif"),
        "video/mp4" => Some(".mp4"),
        "video/webm" => Some(".webm"),
        "video/quicktime" => Some(".mov"),
        "audio/ogg" => Some(".ogg"),
        "audio/opus" => Some(".opus"),
        "audio/mpeg" => Some(".mp3"),
        "audio/mp4" | "audio/x-m4a" => Some(".m4a"),
        "audio/wav" | "audio/x-wav" => Some(".wav"),
        "application/pdf" => Some(".pdf"),
        "text/plain" => Some(".txt"),
        "text/csv" => Some(".csv"),
        "application/json" => Some(".json"),
        "application/zip" => Some(".zip"),
        _ => None,
    }
}

fn signal_extension_from_bytes(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        ".png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        ".jpg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        ".gif"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        ".webp"
    } else if bytes.starts_with(b"%PDF-") {
        ".pdf"
    } else if bytes.starts_with(b"ID3")
        || bytes
            .get(0..2)
            .is_some_and(|prefix| prefix[0] == 0xff && (prefix[1] & 0xe0) == 0xe0)
    {
        ".mp3"
    } else if bytes.starts_with(b"OggS") {
        ".ogg"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        ".wav"
    } else if bytes.get(4..8) == Some(b"ftyp") {
        ".mp4"
    } else if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        ".webm"
    } else {
        ".bin"
    }
}

fn signal_attachment_extension(attachment: &SignalAttachment, bytes: &[u8]) -> String {
    attachment
        .filename
        .as_deref()
        .and_then(signal_extension_from_file_name)
        .or_else(|| {
            attachment
                .content_type
                .as_deref()
                .and_then(signal_extension_from_content_type)
                .map(str::to_string)
        })
        .unwrap_or_else(|| signal_extension_from_bytes(bytes).to_string())
}

fn signal_mime_from_extension(extension: &str) -> Option<&'static str> {
    match extension {
        ".jpg" | ".jpeg" => Some("image/jpeg"),
        ".png" => Some("image/png"),
        ".gif" => Some("image/gif"),
        ".webp" => Some("image/webp"),
        ".heic" => Some("image/heic"),
        ".heif" => Some("image/heif"),
        ".mp4" => Some("video/mp4"),
        ".mov" => Some("video/quicktime"),
        ".webm" => Some("video/webm"),
        ".ogg" => Some("audio/ogg"),
        ".opus" => Some("audio/opus"),
        ".mp3" => Some("audio/mpeg"),
        ".m4a" => Some("audio/mp4"),
        ".wav" => Some("audio/wav"),
        ".pdf" => Some("application/pdf"),
        ".txt" => Some("text/plain"),
        ".csv" => Some("text/csv"),
        ".json" => Some("application/json"),
        ".zip" => Some("application/zip"),
        ".bin" => Some("application/octet-stream"),
        _ => None,
    }
}

fn signal_attachment_fallback_name(
    kind: &MessageAttachmentKind,
    attachment_id: &str,
    extension: &str,
) -> String {
    let label = match kind {
        MessageAttachmentKind::Image => "image",
        MessageAttachmentKind::Video => "video",
        MessageAttachmentKind::Audio => "audio",
        MessageAttachmentKind::Voice => "voice",
        MessageAttachmentKind::Document => "document",
        MessageAttachmentKind::Sticker => "sticker",
        MessageAttachmentKind::Other => "attachment",
    };
    let safe_id = attachment_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("signal-{label}-{safe_id}{extension}")
}

fn decode_signal_attachment_payload(result: serde_json::Value) -> anyhow::Result<Option<Vec<u8>>> {
    let encoded = match result {
        serde_json::Value::Null => return Ok(None),
        serde_json::Value::String(value) => value,
        serde_json::Value::Object(map) => match map.get("data").and_then(|value| value.as_str()) {
            Some(value) => value.to_string(),
            None => return Ok(None),
        },
        _ => anyhow::bail!("Signal getAttachment returned an unsupported payload shape"),
    };

    if encoded.trim().is_empty() {
        return Ok(None);
    }

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.as_bytes())
        .map_err(|error| anyhow::anyhow!("invalid Signal attachment payload: {error}"))?;
    if bytes.is_empty() {
        return Ok(None);
    }

    Ok(Some(bytes))
}

fn render_signal_incoming_text(text: &str, attachments: &[MessageAttachment]) -> String {
    let text = text.trim();
    if attachments.is_empty() {
        return text.to_string();
    }

    let mut lines = Vec::new();
    if !text.is_empty() {
        lines.push(text.to_string());
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

// ---------------------------------------------------------------------------
// SignalAdapter
// ---------------------------------------------------------------------------

/// Signal adapter using signal-cli HTTP daemon.
pub struct SignalAdapter {
    /// URL of the signal-cli HTTP daemon.
    http_url: String,
    /// Phone number registered with signal-cli.
    account: String,
    /// Pre-built HTTP client.
    client: reqwest::Client,
    /// Phone numbers allowed to message the agent (empty = open access).
    allowed_users: Vec<String>,
}

impl SignalAdapter {
    /// Create a new Signal adapter from environment variables.
    ///
    /// # Errors
    /// Returns an error if required environment variables are not set.
    pub fn new() -> anyhow::Result<Self> {
        let http_url = env::var("SIGNAL_HTTP_URL")
            .map_err(|_| anyhow::anyhow!("SIGNAL_HTTP_URL environment variable not set"))?;
        let account = env::var("SIGNAL_ACCOUNT")
            .map_err(|_| anyhow::anyhow!("SIGNAL_ACCOUNT environment variable not set"))?;

        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()?;

        Ok(Self {
            http_url: http_url.trim_end_matches('/').to_string(),
            account,
            client,
            allowed_users: Vec::new(),
        })
    }

    /// Create a Signal adapter directly from config values (no env vars required).
    pub fn from_config(
        http_url: String,
        account: String,
        allowed_users: Vec<String>,
    ) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()?;
        Ok(Self {
            http_url: http_url.trim_end_matches('/').to_string(),
            account,
            client,
            allowed_users: allowed_users
                .into_iter()
                .map(|entry| normalize_signal_identity(&entry))
                .collect(),
        })
    }

    /// Check whether the adapter can be activated.
    pub fn is_available() -> bool {
        env::var("SIGNAL_HTTP_URL").is_ok() && env::var("SIGNAL_ACCOUNT").is_ok()
    }

    /// Send a message via JSON-RPC.
    async fn send_message(
        &self,
        recipient: &str,
        text: &str,
        group_id: Option<&str>,
    ) -> anyhow::Result<()> {
        let params = if let Some(gid) = group_id {
            serde_json::json!({
                "account": self.account,
                "groupId": gid,
                "message": text,
            })
        } else {
            serde_json::json!({
                "account": self.account,
                "recipient": [recipient],
                "message": text,
            })
        };

        self.rpc("send", params).await?;

        Ok(())
    }

    async fn send_attachment(
        &self,
        target: &SignalTarget,
        path: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        if !std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
            anyhow::bail!("Signal attachment path does not exist: {path}");
        }

        let params = match target {
            SignalTarget::Group(group_id) => serde_json::json!({
                "account": self.account,
                "groupId": group_id,
                "message": caption.unwrap_or_default(),
                "attachments": [path],
            }),
            SignalTarget::Direct(recipient) => serde_json::json!({
                "account": self.account,
                "recipient": [recipient],
                "message": caption.unwrap_or_default(),
                "attachments": [path],
            }),
        };

        self.rpc("send", params).await?;
        Ok(())
    }

    async fn rpc(
        &self,
        method: &'static str,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let rpc_request = JsonRpcRequest {
            jsonrpc: "2.0",
            method,
            params,
            id: JSON_RPC_REQUEST_ID.fetch_add(1, Ordering::Relaxed),
        };

        let resp: JsonRpcResponse = self
            .client
            .post(format!("{}/api/v1/rpc", self.http_url))
            .json(&rpc_request)
            .send()
            .await?
            .json()
            .await?;

        if let Some(err) = resp.error {
            anyhow::bail!(
                "Signal RPC {} failed: {}",
                method,
                err.message.unwrap_or_else(|| "unknown".into())
            );
        }

        Ok(resp.result.unwrap_or(serde_json::Value::Null))
    }

    async fn materialize_attachment(
        &self,
        attachment: &SignalAttachment,
    ) -> anyhow::Result<MessageAttachment> {
        let mut metadata = signal_attachment_to_metadata(attachment);
        let Some(attachment_id) = attachment.id.as_deref().filter(|id| !id.trim().is_empty())
        else {
            return Ok(metadata);
        };

        let payload = self
            .rpc(
                "getAttachment",
                serde_json::json!({
                    "account": self.account,
                    "id": attachment_id,
                }),
            )
            .await?;
        let Some(bytes) = decode_signal_attachment_payload(payload)? else {
            return Ok(metadata);
        };

        let extension = signal_attachment_extension(attachment, &bytes);
        let fallback_name =
            signal_attachment_fallback_name(&metadata.kind, attachment_id, &extension);
        metadata.local_path = crate::attachment_cache::persist_bytes(
            "signal",
            attachment.filename.as_deref(),
            &fallback_name,
            &bytes,
        )?;
        metadata.size_bytes = Some(bytes.len() as u64);
        if metadata.mime_type.is_none() {
            metadata.mime_type = signal_mime_from_extension(&extension).map(str::to_string);
        }

        Ok(metadata)
    }

    async fn materialize_attachments(
        &self,
        attachments: &[SignalAttachment],
    ) -> Vec<MessageAttachment> {
        let mut materialized = Vec::with_capacity(attachments.len());
        for attachment in attachments {
            match self.materialize_attachment(attachment).await {
                Ok(metadata) => materialized.push(metadata),
                Err(error) => {
                    error!(
                        %error,
                        attachment_id = attachment.id.as_deref().unwrap_or("unknown"),
                        "Failed to fetch Signal attachment"
                    );
                    materialized.push(signal_attachment_to_metadata(attachment));
                }
            }
        }
        materialized
    }

    /// Connect to SSE stream and process incoming messages.
    async fn listen_sse(&self, tx: &mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
        // signal-cli HTTP daemon exposes a single SSE stream for ALL accounts.
        // The correct endpoint is GET /api/v1/events (not /api/v1/receive/{account}).
        // Reference: signal-cli-jsonrpc(5) man page, "With --http signal-cli exposes: GET /api/v1/events"
        let url = format!("{}/api/v1/events", self.http_url);

        let resp = self
            .client
            .get(&url)
            .header("Accept", "text/event-stream")
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let hint = if status == reqwest::StatusCode::NOT_FOUND {
                " — check that signal-cli daemon is running with --http flag \
                 and SIGNAL_HTTP_URL points to its port (not the EdgeCrab gateway port)"
            } else {
                ""
            };
            anyhow::bail!("Signal SSE connection failed: {}{}", status, hint);
        }

        info!("Signal SSE stream connected");

        use futures::StreamExt;
        let mut stream = resp.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let text = String::from_utf8_lossy(&chunk);
            buffer.push_str(&text);

            // Process complete SSE events (data: ...\n\n)
            while let Some(pos) = buffer.find("\n\n") {
                let event_text = buffer[..pos].to_string();
                buffer = buffer[pos + 2..].to_string();

                // Extract data field from SSE
                let data: String = event_text
                    .lines()
                    .filter_map(|line| {
                        line.strip_prefix("data:")
                            .or_else(|| line.strip_prefix("data: "))
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                if data.is_empty() {
                    continue;
                }

                // Parse Signal envelope
                let envelope: SignalEnvelope = match serde_json::from_str(&data) {
                    Ok(e) => e,
                    Err(e) => {
                        debug!(error = %e, "Failed to parse Signal envelope");
                        continue;
                    }
                };

                let Some(inner) = envelope.envelope else {
                    continue;
                };

                let data_msg = inner.data_message;
                let sync_sent_msg = inner.sync_message.and_then(|s| s.sent_message);

                let message_text = data_msg
                    .as_ref()
                    .and_then(|m| m.message.clone())
                    .or_else(|| sync_sent_msg.as_ref().and_then(|m| m.message.clone()));

                let attachments = if let Some(message) = data_msg.as_ref() {
                    self.materialize_attachments(&message.attachments).await
                } else if let Some(message) = sync_sent_msg.as_ref() {
                    self.materialize_attachments(&message.attachments).await
                } else {
                    Vec::new()
                };

                let rendered_text = render_signal_incoming_text(
                    message_text.as_deref().unwrap_or_default(),
                    &attachments,
                );

                if rendered_text.trim().is_empty() && attachments.is_empty() {
                    continue;
                }

                let source = inner
                    .source_number
                    .or(inner.source_uuid)
                    .unwrap_or_else(|| "unknown".into());
                let normalized_source = normalize_signal_identity(&source);

                // Skip our own messages by default.
                // Exception: for linked-device self-chat setups, user messages are synced
                // from the primary phone as sourceDevice=1. We allow those.
                if source == self.account && inner.source_device != Some(1) {
                    continue;
                }

                // Filter by allowed_users (empty list = open access)
                if !self.allowed_users.is_empty()
                    && !self.allowed_users.contains(&normalized_source)
                {
                    debug!(
                        source = %redact_phone(&source),
                        "Signal message filtered: sender not in allowed list"
                    );
                    continue;
                }

                let group_id = data_msg
                    .as_ref()
                    .and_then(|m| m.group_info.as_ref())
                    .and_then(|g| g.group_id.clone())
                    .or_else(|| {
                        sync_sent_msg
                            .as_ref()
                            .and_then(|m| m.group_info.as_ref())
                            .and_then(|g| g.group_id.clone())
                    });

                let channel_id = group_id.clone().or_else(|| Some(source.clone()));

                let incoming = IncomingMessage {
                    platform: Platform::Signal,
                    user_id: source.clone(),
                    channel_id: channel_id.clone(),
                    text: rendered_text,
                    thread_id: None, // Signal doesn't have threads
                    metadata: MessageMetadata {
                        message_id: data_msg
                            .as_ref()
                            .and_then(|m| m.timestamp)
                            .or_else(|| sync_sent_msg.as_ref().and_then(|m| m.timestamp))
                            .map(|ts| ts.to_string()),
                        channel_id,
                        thread_id: None,
                        user_display_name: None,
                        attachments,
                        ..Default::default()
                    },
                };

                debug!(
                    platform = "signal",
                    source = %redact_phone(&source),
                    "Received message"
                );

                if tx.send(incoming).await.is_err() {
                    info!("Signal adapter: receiver dropped, shutting down");
                    return Ok(());
                }
            }
        }

        anyhow::bail!("Signal SSE stream ended unexpectedly")
    }
}

#[async_trait]
impl PlatformAdapter for SignalAdapter {
    fn platform(&self) -> Platform {
        Platform::Signal
    }

    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
        info!(
            account = %redact_phone(&self.account),
            url = %self.http_url,
            "Signal adapter starting (SSE mode)"
        );

        let mut retry_delay = SSE_RETRY_DELAY;

        loop {
            match self.listen_sse(&tx).await {
                Ok(()) => {
                    // Stream ended normally — reconnect
                    retry_delay = SSE_RETRY_DELAY;
                }
                Err(e) => {
                    error!(error = %e, "Signal SSE error, reconnecting");
                    retry_delay = std::cmp::min(
                        Duration::from_secs(retry_delay.as_secs() * 2),
                        SSE_RETRY_DELAY_MAX,
                    );
                }
            }
            tokio::time::sleep(retry_delay).await;
        }
    }

    async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()> {
        let raw_target =
            msg.metadata.channel_id.as_deref().ok_or_else(|| {
                anyhow::anyhow!("Signal send requires channel_id (phone or group)")
            })?;
        let target = normalize_signal_target(raw_target)?;

        let text = &msg.text;
        let chunks = split_message(text, MAX_MESSAGE_LENGTH);

        for chunk in &chunks {
            match &target {
                SignalTarget::Group(group_id) => {
                    self.send_message(group_id, chunk, Some(group_id)).await?;
                }
                SignalTarget::Direct(recipient) => {
                    self.send_message(recipient, chunk, None).await?;
                }
            }
        }

        Ok(())
    }

    fn format_response(&self, text: &str, _metadata: &MessageMetadata) -> String {
        // Signal doesn't support markdown — return plain text
        text.to_string()
    }

    fn max_message_length(&self) -> usize {
        MAX_MESSAGE_LENGTH
    }

    fn supports_markdown(&self) -> bool {
        false
    }

    fn supports_images(&self) -> bool {
        true // Signal supports image attachments
    }

    fn supports_files(&self) -> bool {
        true // Signal supports file attachments
    }

    async fn send_photo(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        let raw_target = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Signal photo send requires channel_id"))?;
        let target = normalize_signal_target(raw_target)?;
        self.send_attachment(&target, path, caption).await
    }

    async fn send_document(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        let raw_target = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Signal document send requires channel_id"))?;
        let target = normalize_signal_target(raw_target)?;
        self.send_attachment(&target, path, caption).await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, routing::post};
    use serde_json::{Value, json};
    use std::sync::{Arc, Mutex};

    fn test_signal_adapter(http_url: String) -> SignalAdapter {
        SignalAdapter::from_config(http_url, "+15551234567".into(), Vec::new()).expect("adapter")
    }

    #[test]
    fn redact_phone_number() {
        assert_eq!(redact_phone("+15551234567"), "+155****4567");
    }

    #[test]
    fn redact_short_phone() {
        assert_eq!(redact_phone("+1234"), "****");
    }

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
    fn format_plain_text() {
        // Signal outputs plain text, no transformation
        let text = "**bold** and _italic_";
        assert_eq!(text, "**bold** and _italic_");
    }

    #[test]
    fn is_available_without_env() {
        let _result = SignalAdapter::is_available();
    }

    #[test]
    fn group_detection() {
        assert_eq!(
            normalize_signal_target("+15551234567").expect("direct"),
            SignalTarget::Direct("+15551234567".into())
        );
        assert_eq!(
            normalize_signal_target("15551234567").expect("direct"),
            SignalTarget::Direct("+15551234567".into())
        );
        assert_eq!(
            normalize_signal_target("group:abc123groupid==").expect("group"),
            SignalTarget::Group("abc123groupid==".into())
        );
        assert_eq!(
            normalize_signal_target("abc123groupid==").expect("group"),
            SignalTarget::Group("abc123groupid==".into())
        );
    }

    #[test]
    fn signal_phone_normalization_strips_formatting() {
        assert_eq!(
            normalize_signal_phone("(555) 123-4567"),
            Some("+5551234567".into())
        );
        assert_eq!(
            normalize_signal_phone("00447911123456"),
            Some("+447911123456".into())
        );
    }

    #[test]
    fn signal_target_rejects_whitespace_group_like_input() {
        let err = normalize_signal_target("group id").expect_err("invalid target");
        assert!(err.to_string().contains("invalid"));
    }

    #[test]
    fn signal_attachment_summary_for_media_only_message() {
        let attachments = vec![MessageAttachment {
            kind: MessageAttachmentKind::Voice,
            file_name: Some("voice-note.ogg".into()),
            ..Default::default()
        }];
        let rendered = render_signal_incoming_text("", &attachments);
        assert!(rendered.contains("Shared 1 attachment:"));
        assert!(rendered.contains("voice: voice-note.ogg"));
    }

    #[test]
    fn sse_url_uses_events_endpoint() {
        // The SSE endpoint must be /api/v1/events — NOT /api/v1/receive/{account}.
        // Regression test for the 404 bug caused by the wrong path.
        let base = "http://127.0.0.1:8090";
        let sse_url = format!("{base}/api/v1/events");
        assert!(
            sse_url.ends_with("/api/v1/events"),
            "SSE URL must end with /api/v1/events, got: {sse_url}"
        );
        assert!(
            !sse_url.contains("/receive/"),
            "SSE URL must not contain /receive/: {sse_url}"
        );
    }

    #[test]
    fn signal_attachment_summary_includes_local_path_when_available() {
        let attachments = vec![MessageAttachment {
            kind: MessageAttachmentKind::Image,
            file_name: Some("photo.png".into()),
            local_path: Some("/tmp/photo.png".into()),
            ..Default::default()
        }];
        let rendered = render_signal_incoming_text("see this", &attachments);
        assert!(rendered.contains("/tmp/photo.png"));
    }

    #[test]
    fn decode_signal_attachment_payload_accepts_dict_data_shape() {
        let payload = json!({
            "data": base64::engine::general_purpose::STANDARD.encode(b"\x89PNG\r\n\x1a\nrest")
        });

        let bytes = decode_signal_attachment_payload(payload)
            .expect("decode ok")
            .expect("has bytes");
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[tokio::test]
    async fn materialize_attachment_fetches_signal_bytes_to_local_cache() {
        let payload =
            base64::engine::general_purpose::STANDARD.encode(b"\x89PNG\r\n\x1a\nedgecrab-signal");
        let captured = Arc::new(Mutex::new(Vec::<Value>::new()));
        let captured_state = Arc::clone(&captured);

        let app = Router::new().route(
            "/api/v1/rpc",
            post(move |Json(body): Json<Value>| {
                let captured = Arc::clone(&captured_state);
                let payload = payload.clone();
                async move {
                    captured.lock().expect("lock").push(body);
                    Json(json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "result": { "data": payload }
                    }))
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });

        let adapter = test_signal_adapter(format!("http://{addr}"));
        let attachment = SignalAttachment {
            id: Some("attachment-123".into()),
            filename: Some("picture.png".into()),
            content_type: Some("image/png".into()),
            size: Some(21),
            voice_note: Some(false),
        };

        let materialized = adapter
            .materialize_attachment(&attachment)
            .await
            .expect("materialize ok");

        let requests = captured.lock().expect("lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["method"], "getAttachment");
        assert_eq!(requests[0]["params"]["account"], "+15551234567");
        assert_eq!(requests[0]["params"]["id"], "attachment-123");
        assert!(requests[0]["params"].get("attachmentId").is_none());

        let local_path = materialized.local_path.as_deref().expect("local path");
        assert!(std::path::Path::new(local_path).exists());
        let p = std::path::Path::new(local_path);
        assert!(
            p.components().any(|c| c.as_os_str() == "gateway_media")
                && p.components().any(|c| c.as_os_str() == "signal"),
            "expected gateway_media/signal in {local_path}"
        );
        assert_eq!(materialized.mime_type.as_deref(), Some("image/png"));

        server.abort();
    }

    #[tokio::test]
    async fn send_document_uses_signal_rpc_attachment_payload() {
        let captured = Arc::new(Mutex::new(Vec::<Value>::new()));
        let captured_state = Arc::clone(&captured);
        let app = Router::new().route(
            "/api/v1/rpc",
            post(move |Json(body): Json<Value>| {
                let captured = Arc::clone(&captured_state);
                async move {
                    captured.lock().expect("lock").push(body);
                    Json(json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "result": { "timestamp": 1 }
                    }))
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });

        let adapter = test_signal_adapter(format!("http://{addr}"));
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("report.pdf");
        std::fs::write(&path, b"edgecrab").expect("write");
        let metadata = MessageMetadata {
            channel_id: Some("+15557654321".into()),
            ..Default::default()
        };

        adapter
            .send_document(path.to_string_lossy().as_ref(), Some("report"), &metadata)
            .await
            .expect("send document");

        let requests = captured.lock().expect("lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["method"], "send");
        assert_eq!(requests[0]["params"]["message"], "report");
        assert_eq!(requests[0]["params"]["recipient"][0], "+15557654321");
        assert_eq!(
            requests[0]["params"]["attachments"][0],
            path.to_string_lossy().as_ref()
        );

        server.abort();
    }
}
