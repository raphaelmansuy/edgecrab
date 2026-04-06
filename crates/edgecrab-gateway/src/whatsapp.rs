//! # WhatsApp adapter via the Hermes Baileys bridge
//!
//! Hermes already ships a production-tested Node bridge that handles the
//! difficult parts of WhatsApp connectivity. EdgeCrab reuses that bridge
//! rather than reimplementing the protocol stack in Rust.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::Context;
use async_trait::async_trait;
use edgecrab_core::{config::WhatsAppGatewayConfig, edgecrab_home};
use edgecrab_types::Platform;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::platform::{
    IncomingMessage, MessageAttachment, MessageAttachmentKind, MessageMetadata, OutgoingMessage,
    PlatformAdapter,
};

const MAX_MESSAGE_LENGTH: usize = 65_536;
const DEFAULT_BRIDGE_PORT: u16 = 3000;
const DEFAULT_POLL_INTERVAL_MS: u64 = 1_000;
const DEFAULT_STARTUP_TIMEOUT_SECS: u64 = 20;
const PERSONAL_JID_SUFFIX: &str = "@s.whatsapp.net";

#[derive(Debug, Clone)]
pub struct BridgeAssets {
    pub bridge_dir: PathBuf,
    pub bridge_script: PathBuf,
}

#[derive(Debug, Clone)]
pub struct WhatsappAdapterConfig {
    pub bridge_port: u16,
    pub bridge_url: Option<String>,
    pub bridge_dir: Option<PathBuf>,
    pub bridge_script: Option<PathBuf>,
    pub session_path: PathBuf,
    pub mode: String,
    pub allowed_users: Vec<String>,
    pub reply_prefix: Option<String>,
    pub install_dependencies: bool,
    pub startup_timeout: Duration,
    pub poll_interval: Duration,
    pub log_path: PathBuf,
}

impl Default for WhatsappAdapterConfig {
    fn default() -> Self {
        let home = edgecrab_home();
        Self {
            bridge_port: DEFAULT_BRIDGE_PORT,
            bridge_url: None,
            bridge_dir: None,
            bridge_script: None,
            session_path: home.join("whatsapp").join("session"),
            mode: "self-chat".into(),
            allowed_users: Vec::new(),
            reply_prefix: Some("\u{2695} *EdgeCrab Agent*\n------------\n".into()),
            install_dependencies: true,
            startup_timeout: Duration::from_secs(DEFAULT_STARTUP_TIMEOUT_SECS),
            poll_interval: Duration::from_millis(DEFAULT_POLL_INTERVAL_MS),
            log_path: home.join("logs").join("whatsapp-bridge.log"),
        }
    }
}

impl From<&WhatsAppGatewayConfig> for WhatsappAdapterConfig {
    fn from(value: &WhatsAppGatewayConfig) -> Self {
        Self {
            bridge_port: value.bridge_port,
            bridge_url: value.bridge_url.clone(),
            bridge_dir: value.bridge_dir.clone(),
            bridge_script: value.bridge_script.clone(),
            session_path: value
                .session_path
                .clone()
                .unwrap_or_else(|| edgecrab_home().join("whatsapp").join("session")),
            mode: value.mode.clone(),
            // Normalize: strip '+' prefix so numbers match WhatsApp JIDs
            allowed_users: value
                .allowed_users
                .iter()
                .map(|u| u.trim_start_matches('+').to_string())
                .collect(),
            reply_prefix: value.reply_prefix.clone(),
            install_dependencies: value.install_dependencies,
            ..Self::default()
        }
    }
}

impl WhatsappAdapterConfig {
    pub fn bridge_url(&self) -> String {
        self.bridge_url
            .clone()
            .unwrap_or_else(|| format!("http://127.0.0.1:{}", self.bridge_port))
    }

    pub fn health_url(&self) -> String {
        format!("{}/health", self.bridge_url())
    }

    pub fn messages_url(&self) -> String {
        format!("{}/messages", self.bridge_url())
    }

    pub fn send_url(&self) -> String {
        format!("{}/send", self.bridge_url())
    }

    pub fn send_media_url(&self) -> String {
        format!("{}/send-media", self.bridge_url())
    }
}

#[derive(Debug, Deserialize)]
struct WhatsAppHealth {
    status: String,
}

#[derive(Debug, Deserialize)]
struct WhatsAppInboundEvent {
    #[serde(rename = "messageId")]
    message_id: Option<String>,
    #[serde(rename = "chatId")]
    chat_id: String,
    #[serde(rename = "senderId")]
    sender_id: String,
    #[serde(rename = "senderName")]
    sender_name: Option<String>,
    body: String,
    #[serde(rename = "mediaUrls", default)]
    media_urls: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SendRequest<'a> {
    #[serde(rename = "chatId")]
    chat_id: &'a str,
    message: &'a str,
}

#[derive(Debug, Serialize)]
struct SendMediaRequest<'a> {
    #[serde(rename = "chatId")]
    chat_id: &'a str,
    #[serde(rename = "filePath")]
    file_path: &'a str,
    #[serde(rename = "mediaType")]
    media_type: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    caption: Option<&'a str>,
    #[serde(rename = "fileName", skip_serializing_if = "Option::is_none")]
    file_name: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct SendResponse {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug)]
struct BridgeSendFailure {
    detail: String,
}

impl std::fmt::Display for BridgeSendFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.detail)
    }
}

impl std::error::Error for BridgeSendFailure {}

pub struct WhatsAppAdapter {
    config: WhatsappAdapterConfig,
    client: reqwest::Client,
    managed_bridge: Mutex<Option<Child>>,
}

impl WhatsAppAdapter {
    pub fn new(config: WhatsappAdapterConfig) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;
        Ok(Self {
            config,
            client,
            managed_bridge: Mutex::new(None),
        })
    }

    pub fn is_available(config: &WhatsappAdapterConfig) -> bool {
        if config.bridge_url.is_some() {
            true
        } else {
            has_node() && resolve_bridge_assets(config).is_ok()
        }
    }

    pub fn default_session_path() -> PathBuf {
        edgecrab_home().join("whatsapp").join("session")
    }

    pub fn default_log_path() -> PathBuf {
        edgecrab_home().join("logs").join("whatsapp-bridge.log")
    }

    pub fn resolve_bridge_assets(config: &WhatsappAdapterConfig) -> anyhow::Result<BridgeAssets> {
        resolve_bridge_assets(config)
    }

    pub fn pair(config: &WhatsappAdapterConfig) -> anyhow::Result<()> {
        ensure_node_available()?;
        let assets = resolve_bridge_assets(config)?;
        ensure_dependencies(config, &assets)?;
        std::fs::create_dir_all(&config.session_path)?;

        let status = Command::new("node")
            .arg(&assets.bridge_script)
            .arg("--pair-only")
            .arg("--session")
            .arg(&config.session_path)
            .arg("--mode")
            .arg(&config.mode)
            .current_dir(&assets.bridge_dir)
            .status()
            .context("failed to start WhatsApp pairing bridge")?;

        if !status.success() {
            anyhow::bail!("WhatsApp pairing exited with status {}", status);
        }
        Ok(())
    }

    pub async fn health(config: &WhatsappAdapterConfig) -> anyhow::Result<String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()?;
        let health = client
            .get(config.health_url())
            .send()
            .await?
            .error_for_status()?
            .json::<WhatsAppHealth>()
            .await?;
        Ok(health.status)
    }

    async fn ensure_bridge_ready(&self) -> anyhow::Result<()> {
        if let Ok(status) = Self::health(&self.config).await {
            if status == "connected" {
                return Ok(());
            }
        }

        self.start_managed_bridge()?;

        let deadline = Instant::now() + self.config.startup_timeout;
        while Instant::now() < deadline {
            match Self::health(&self.config).await {
                Ok(status) if status == "connected" => return Ok(()),
                Ok(status) => {
                    debug!(status, "waiting for WhatsApp bridge");
                }
                Err(error) => {
                    debug!(%error, "waiting for WhatsApp bridge health endpoint");
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        anyhow::bail!(
            "WhatsApp bridge did not become ready; check {}",
            self.config.log_path.display()
        );
    }

    fn start_managed_bridge(&self) -> anyhow::Result<()> {
        if self.config.bridge_url.is_some() {
            anyhow::bail!("remote WhatsApp bridge is unavailable");
        }

        if self
            .managed_bridge
            .lock()
            .expect("managed_bridge mutex poisoned")
            .is_some()
        {
            return Ok(());
        }

        ensure_node_available()?;
        let assets = resolve_bridge_assets(&self.config)?;
        ensure_dependencies(&self.config, &assets)?;
        std::fs::create_dir_all(
            self.config
                .log_path
                .parent()
                .unwrap_or_else(|| Path::new(".")),
        )?;
        std::fs::create_dir_all(&self.config.session_path)?;

        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.config.log_path)
            .with_context(|| format!("failed to open {}", self.config.log_path.display()))?;
        let stderr_file = log_file.try_clone()?;

        let mut cmd = Command::new("node");
        cmd.arg(&assets.bridge_script)
            .arg("--port")
            .arg(self.config.bridge_port.to_string())
            .arg("--session")
            .arg(&self.config.session_path)
            .arg("--mode")
            .arg(&self.config.mode)
            .current_dir(&assets.bridge_dir)
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(stderr_file))
            .stdin(Stdio::null());
        if !self.config.allowed_users.is_empty() {
            // Normalize: strip leading '+' from phone numbers so the bridge can
            // compare against WhatsApp JIDs which never include '+'.
            let normalized: Vec<String> = self
                .config
                .allowed_users
                .iter()
                .map(|u| u.trim_start_matches('+').to_string())
                .collect();
            cmd.env("WHATSAPP_ALLOWED_USERS", normalized.join(","));
        }
        if let Some(prefix) = &self.config.reply_prefix {
            cmd.env("WHATSAPP_REPLY_PREFIX", prefix);
        }
        cmd.env("WHATSAPP_MODE", &self.config.mode);

        let child = cmd.spawn().context("failed to spawn WhatsApp bridge")?;
        info!(
            pid = child.id(),
            log = %self.config.log_path.display(),
            "started WhatsApp bridge"
        );
        *self
            .managed_bridge
            .lock()
            .expect("managed_bridge mutex poisoned") = Some(child);
        Ok(())
    }

    async fn poll_messages(&self) -> anyhow::Result<Vec<WhatsAppInboundEvent>> {
        let response = self
            .client
            .get(self.config.messages_url())
            .send()
            .await?
            .error_for_status()?;
        Ok(response.json::<Vec<WhatsAppInboundEvent>>().await?)
    }

    fn event_to_message(event: WhatsAppInboundEvent) -> IncomingMessage {
        let attachments = event
            .media_urls
            .iter()
            .map(|path| whatsapp_attachment(path))
            .collect::<Vec<_>>();
        let text = render_incoming_text(&event.body, &attachments);

        IncomingMessage {
            platform: Platform::Whatsapp,
            user_id: event.sender_id,
            channel_id: Some(event.chat_id.clone()),
            text,
            thread_id: None,
            metadata: MessageMetadata {
                message_id: event.message_id,
                channel_id: Some(event.chat_id),
                user_display_name: event.sender_name,
                attachments,
                ..Default::default()
            },
        }
    }

    async fn send_bridge_message(
        &self,
        chat_id: &str,
        message: &str,
    ) -> Result<(), BridgeSendFailure> {
        let body = SendRequest { chat_id, message };
        let response = self
            .client
            .post(self.config.send_url())
            .json(&body)
            .send()
            .await
            .map_err(|error| BridgeSendFailure {
                detail: format!(
                    "failed to contact the local message relay ({}): {error}",
                    self.config.send_url()
                ),
            })?;

        let status = response.status();
        let raw_body = response.text().await.map_err(|error| BridgeSendFailure {
            detail: format!(
                "failed to read the local message relay response ({}): {error}",
                self.config.send_url()
            ),
        })?;

        if !status.is_success() {
            let detail = parse_bridge_error(&raw_body).unwrap_or_else(|| {
                status
                    .canonical_reason()
                    .unwrap_or("unknown error")
                    .to_string()
            });
            return Err(BridgeSendFailure { detail });
        }

        let payload =
            serde_json::from_str::<SendResponse>(&raw_body).map_err(|error| BridgeSendFailure {
                detail: format!(
                    "invalid response from the local message relay ({}): {error}",
                    self.config.send_url()
                ),
            })?;
        if !payload.success {
            return Err(BridgeSendFailure {
                detail: payload.error.unwrap_or_else(|| "unknown error".into()),
            });
        }
        Ok(())
    }

    async fn send_bridge_media(
        &self,
        chat_id: &str,
        file_path: &str,
        media_type: &str,
        caption: Option<&str>,
        file_name: Option<&str>,
    ) -> Result<(), BridgeSendFailure> {
        let body = SendMediaRequest {
            chat_id,
            file_path,
            media_type,
            caption,
            file_name,
        };
        let response = self
            .client
            .post(self.config.send_media_url())
            .json(&body)
            .send()
            .await
            .map_err(|error| BridgeSendFailure {
                detail: format!(
                    "failed to contact the local media relay ({}): {error}",
                    self.config.send_media_url()
                ),
            })?;

        let status = response.status();
        let raw_body = response.text().await.map_err(|error| BridgeSendFailure {
            detail: format!(
                "failed to read the local media relay response ({}): {error}",
                self.config.send_media_url()
            ),
        })?;

        if !status.is_success() {
            let detail = parse_bridge_error(&raw_body).unwrap_or_else(|| {
                status
                    .canonical_reason()
                    .unwrap_or("unknown error")
                    .to_string()
            });
            return Err(BridgeSendFailure { detail });
        }

        let payload =
            serde_json::from_str::<SendResponse>(&raw_body).map_err(|error| BridgeSendFailure {
                detail: format!(
                    "invalid response from the local media relay ({}): {error}",
                    self.config.send_media_url()
                ),
            })?;
        if !payload.success {
            return Err(BridgeSendFailure {
                detail: payload.error.unwrap_or_else(|| "unknown error".into()),
            });
        }
        Ok(())
    }
}

fn normalize_outbound_chat_id(chat_id: &str) -> String {
    let trimmed = chat_id.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if let Some((local_part, suffix)) = split_supported_jid(trimmed) {
        let normalized_local = normalize_jid_local_part(local_part, suffix);
        if !normalized_local.is_empty() {
            return format!("{normalized_local}@{suffix}");
        }
    }

    let compact = compact_phone_candidate(trimmed);
    if !compact.is_empty() && compact.chars().all(|c| c.is_ascii_digit()) {
        return format!("{compact}{PERSONAL_JID_SUFFIX}");
    }

    trimmed.to_string()
}

fn split_supported_jid(value: &str) -> Option<(&str, &str)> {
    if let Some((local_part, suffix)) = value.split_once('@') {
        let suffix = suffix.trim();
        if is_supported_jid_suffix(suffix) {
            return Some((local_part, suffix));
        }
    }

    if let Some((local_part, suffix)) = value.split_once(':') {
        let suffix = suffix.trim();
        if is_supported_jid_suffix(suffix) {
            return Some((local_part, suffix));
        }
    }

    None
}

fn is_supported_jid_suffix(suffix: &str) -> bool {
    matches!(suffix, "s.whatsapp.net" | "g.us" | "lid")
}

fn normalize_jid_local_part(local_part: &str, suffix: &str) -> String {
    let device_stripped = local_part.split(':').next().unwrap_or(local_part).trim();
    if suffix == "g.us" {
        return device_stripped
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .collect();
    }

    compact_phone_candidate(device_stripped)
}

fn compact_phone_candidate(value: &str) -> String {
    let compact: String = value
        .chars()
        .filter(|c| !c.is_ascii_whitespace() && !matches!(c, '-' | '(' | ')'))
        .collect();
    compact.trim_start_matches('+').to_string()
}

fn parse_bridge_error(raw_body: &str) -> Option<String> {
    let trimmed = raw_body.trim();
    if trimmed.is_empty() {
        return None;
    }

    serde_json::from_str::<SendResponse>(trimmed)
        .ok()
        .and_then(|payload| payload.error)
        .or_else(|| Some(trimmed.to_string()))
}

fn whatsapp_attachment(path: &str) -> MessageAttachment {
    let file_name = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(ToString::to_string);
    MessageAttachment {
        kind: classify_whatsapp_attachment(path),
        file_name,
        local_path: Some(path.to_string()),
        ..Default::default()
    }
}

fn classify_whatsapp_attachment(path: &str) -> MessageAttachmentKind {
    let extension = Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());

    match extension.as_deref() {
        Some("jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "heic") => {
            MessageAttachmentKind::Image
        }
        Some("mp4" | "mov" | "avi" | "mkv" | "webm") => MessageAttachmentKind::Video,
        Some("ogg" | "opus" | "mp3" | "wav" | "m4a" | "aac") => MessageAttachmentKind::Audio,
        Some("pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "csv" | "zip") => {
            MessageAttachmentKind::Document
        }
        _ => MessageAttachmentKind::Other,
    }
}

fn render_incoming_text(body: &str, attachments: &[MessageAttachment]) -> String {
    let body = body.trim();
    if attachments.is_empty() {
        return body.to_string();
    }

    let mut lines = Vec::new();
    if !body.is_empty() {
        lines.push(body.to_string());
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
        if let Some(path) = attachment.local_path.as_deref() {
            lines.push(format!("- {}: {} ({})", label, file_name, path));
        } else {
            lines.push(format!("- {}: {}", label, file_name));
        }
    }

    lines.join("\n")
}

#[async_trait]
impl PlatformAdapter for WhatsAppAdapter {
    fn platform(&self) -> Platform {
        Platform::Whatsapp
    }

    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
        self.ensure_bridge_ready().await?;
        info!("WhatsApp adapter polling bridge");

        loop {
            match self.poll_messages().await {
                Ok(events) => {
                    for event in events {
                        let message = Self::event_to_message(event);
                        tx.send(message)
                            .await
                            .context("failed to forward WhatsApp message")?;
                    }
                }
                Err(error) => {
                    warn!(%error, "WhatsApp bridge poll failed");
                }
            }
            tokio::time::sleep(self.config.poll_interval).await;
        }
    }

    async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()> {
        let original_chat_id = msg
            .metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("WhatsApp delivery requires channel_id"))?;
        let normalized_chat_id = normalize_outbound_chat_id(original_chat_id);
        if normalized_chat_id.is_empty() {
            anyhow::bail!("WhatsApp delivery requires a non-empty channel_id");
        }

        self.send_bridge_message(&normalized_chat_id, &msg.text)
            .await
            .map_err(|error| {
                anyhow::anyhow!(
                    "WhatsApp bridge send failed for '{}' (normalized to '{}'): {}",
                    original_chat_id,
                    normalized_chat_id,
                    error
                )
            })
    }

    fn format_response(&self, text: &str, _metadata: &MessageMetadata) -> String {
        text.to_string()
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

    async fn send_photo(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        if !std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
            anyhow::bail!("WhatsApp photo path does not exist: {path}");
        }

        let original_chat_id = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("WhatsApp photo delivery requires channel_id"))?;
        let normalized_chat_id = normalize_outbound_chat_id(original_chat_id);
        if normalized_chat_id.is_empty() {
            anyhow::bail!("WhatsApp photo delivery requires a non-empty channel_id");
        }

        self.send_bridge_media(&normalized_chat_id, path, "image", caption, None)
            .await
            .map_err(|error| {
                anyhow::anyhow!(
                    "WhatsApp bridge media send failed for '{}' (normalized to '{}'): {}",
                    original_chat_id,
                    normalized_chat_id,
                    error
                )
            })
    }

    async fn send_document(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        if !std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
            anyhow::bail!("WhatsApp document path does not exist: {path}");
        }

        let original_chat_id = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("WhatsApp document delivery requires channel_id"))?;
        let normalized_chat_id = normalize_outbound_chat_id(original_chat_id);
        if normalized_chat_id.is_empty() {
            anyhow::bail!("WhatsApp document delivery requires a non-empty channel_id");
        }

        let file_name = std::path::Path::new(path)
            .file_name()
            .and_then(|value| value.to_str());
        self.send_bridge_media(&normalized_chat_id, path, "document", caption, file_name)
            .await
            .map_err(|error| {
                anyhow::anyhow!(
                    "WhatsApp bridge media send failed for '{}' (normalized to '{}'): {}",
                    original_chat_id,
                    normalized_chat_id,
                    error
                )
            })
    }

    async fn send_voice(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        if !std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
            anyhow::bail!("WhatsApp audio path does not exist: {path}");
        }

        let original_chat_id = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("WhatsApp audio delivery requires channel_id"))?;
        let normalized_chat_id = normalize_outbound_chat_id(original_chat_id);
        if normalized_chat_id.is_empty() {
            anyhow::bail!("WhatsApp audio delivery requires a non-empty channel_id");
        }

        let file_name = std::path::Path::new(path)
            .file_name()
            .and_then(|value| value.to_str());
        self.send_bridge_media(&normalized_chat_id, path, "audio", caption, file_name)
            .await
            .map_err(|error| {
                anyhow::anyhow!(
                    "WhatsApp bridge media send failed for '{}' (normalized to '{}'): {}",
                    original_chat_id,
                    normalized_chat_id,
                    error
                )
            })
    }
}

fn has_node() -> bool {
    Command::new("node")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn ensure_node_available() -> anyhow::Result<()> {
    if has_node() {
        Ok(())
    } else {
        anyhow::bail!("Node.js is required for WhatsApp support")
    }
}

fn resolve_bridge_assets(config: &WhatsappAdapterConfig) -> anyhow::Result<BridgeAssets> {
    if let Some(script) = &config.bridge_script {
        let bridge_dir = config.bridge_dir.clone().unwrap_or_else(|| {
            script
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf()
        });
        if script.exists() {
            return Ok(BridgeAssets {
                bridge_dir,
                bridge_script: script.clone(),
            });
        }
    }

    if let Some(dir) = &config.bridge_dir {
        let script = dir.join("bridge.js");
        if script.exists() {
            return Ok(BridgeAssets {
                bridge_dir: dir.clone(),
                bridge_script: script,
            });
        }
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // Look relative to the gateway crate: crates/edgecrab-gateway -> edgecrab root
    let crate_root = manifest_dir
        .join("../..")
        .canonicalize()
        .unwrap_or(manifest_dir.clone());
    let repo_root = manifest_dir
        .join("../../..")
        .canonicalize()
        .unwrap_or(manifest_dir.clone());
    let candidates = [
        // Prefer edgecrab's own bridge first (sibling to crates/)
        crate_root.join("scripts").join("whatsapp-bridge"),
        // Then try mono-repo layout
        repo_root
            .join("edgecrab")
            .join("scripts")
            .join("whatsapp-bridge"),
        // Fallback to hermes-agent bridge
        repo_root
            .join("hermes-agent")
            .join("scripts")
            .join("whatsapp-bridge"),
    ];

    for bridge_dir in candidates {
        let bridge_script = bridge_dir.join("bridge.js");
        if bridge_script.exists() {
            return Ok(BridgeAssets {
                bridge_dir,
                bridge_script,
            });
        }
    }

    anyhow::bail!("could not find a WhatsApp bridge directory")
}

fn ensure_dependencies(
    config: &WhatsappAdapterConfig,
    assets: &BridgeAssets,
) -> anyhow::Result<()> {
    if !config.install_dependencies || assets.bridge_dir.join("node_modules").exists() {
        return Ok(());
    }

    info!(dir = %assets.bridge_dir.display(), "Installing WhatsApp bridge dependencies");

    // Step 1: Install with --ignore-scripts to avoid sharp's native build
    // which commonly fails when platform-specific prebuilt binaries aren't
    // resolved automatically (node-gyp + node-addon-api issues).
    let status = Command::new("npm")
        .args(["install", "--ignore-scripts"])
        .current_dir(&assets.bridge_dir)
        .status()
        .context("failed to run npm install for WhatsApp bridge")?;
    if !status.success() {
        anyhow::bail!("npm install failed in {}", assets.bridge_dir.display());
    }

    // Step 2: Run the postinstall script which installs the correct
    // platform-specific prebuilt sharp binary (e.g. @img/sharp-darwin-arm64).
    let postinstall = assets.bridge_dir.join("install-sharp-prebuilt.js");
    if postinstall.exists() {
        let post_status = Command::new("node")
            .arg(&postinstall)
            .current_dir(&assets.bridge_dir)
            .status();
        match post_status {
            Ok(s) if s.success() => {
                info!("Sharp prebuilt installed successfully");
            }
            Ok(s) => {
                // Non-fatal: sharp is only needed for media thumbnails
                warn!(
                    code = s.code(),
                    "sharp prebuilt install exited with error (non-fatal)"
                );
            }
            Err(e) => {
                warn!(%e, "could not run sharp prebuilt installer (non-fatal)");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, routing::post};
    use serde_json::{Value, json};
    use std::sync::{Arc, Mutex};

    #[test]
    fn config_from_gateway_config() {
        let source = WhatsAppGatewayConfig {
            enabled: true,
            bridge_port: 3111,
            mode: "bot".into(),
            allowed_users: vec!["12345".into()],
            ..Default::default()
        };
        let config = WhatsappAdapterConfig::from(&source);
        assert_eq!(config.bridge_port, 3111);
        assert_eq!(config.mode, "bot");
        assert_eq!(config.allowed_users, vec!["12345"]);
    }

    #[test]
    fn incoming_media_paths_are_appended() {
        let incoming = WhatsAppInboundEvent {
            message_id: Some("msg-1".into()),
            chat_id: "123@s.whatsapp.net".into(),
            sender_id: "123@s.whatsapp.net".into(),
            sender_name: Some("Raphael".into()),
            body: "look".into(),
            media_urls: vec!["/tmp/test.png".into(), "/tmp/test.pdf".into()],
        };
        let message = WhatsAppAdapter::event_to_message(incoming);
        assert!(message.text.contains("Shared 2 attachments:"));
        assert!(message.text.contains("image: test.png"));
        assert!(message.text.contains("document: test.pdf"));
        assert_eq!(
            message.metadata.channel_id.as_deref(),
            Some("123@s.whatsapp.net")
        );
        assert_eq!(message.metadata.attachments.len(), 2);
    }

    #[test]
    fn bridge_assets_use_explicit_script() {
        let temp = tempfile::tempdir().expect("tempdir");
        let script = temp.path().join("bridge.js");
        std::fs::write(&script, "console.log('ok');").expect("write bridge");

        let config = WhatsappAdapterConfig {
            bridge_script: Some(script.clone()),
            ..Default::default()
        };
        let assets = resolve_bridge_assets(&config).expect("assets");
        assert_eq!(assets.bridge_script, script);
        assert_eq!(assets.bridge_dir, temp.path());
    }

    // ─── attachment classification edge cases ──────────────────────────────

    #[test]
    fn heic_classified_as_image() {
        // iPhone photos sent via WhatsApp arrive as .heic
        let att = whatsapp_attachment("/tmp/photo.heic");
        assert_eq!(att.kind, MessageAttachmentKind::Image);
    }

    #[test]
    fn jpg_and_png_classified_as_image() {
        for ext in &["jpg", "jpeg", "png", "gif", "webp", "bmp"] {
            let path = format!("/tmp/file.{ext}");
            let att = whatsapp_attachment(&path);
            assert_eq!(
                att.kind,
                MessageAttachmentKind::Image,
                ".{ext} must be classified as Image"
            );
        }
    }

    #[test]
    fn mp4_classified_as_video() {
        let att = whatsapp_attachment("/cache/video.mp4");
        assert_eq!(att.kind, MessageAttachmentKind::Video);
    }

    #[test]
    fn ogg_classified_as_audio() {
        let att = whatsapp_attachment("/cache/voice.ogg");
        assert_eq!(att.kind, MessageAttachmentKind::Audio);
    }

    #[test]
    fn unknown_extension_classified_as_other() {
        let att = whatsapp_attachment("/cache/mystery.xyz");
        assert_eq!(att.kind, MessageAttachmentKind::Other);
    }

    #[test]
    fn local_path_is_stored_in_attachment() {
        let path = "/home/user/.edgecrab/image_cache/img_abc123.jpg";
        let att = whatsapp_attachment(path);
        assert_eq!(att.local_path.as_deref(), Some(path));
    }

    #[test]
    fn file_name_extracted_from_path() {
        let att = whatsapp_attachment("/cache/img_abc123.jpg");
        assert_eq!(att.file_name.as_deref(), Some("img_abc123.jpg"));
    }

    // ─── render_incoming_text edge cases ──────────────────────────────────

    #[test]
    fn empty_body_with_image_still_shows_attachment_summary() {
        // Edge case: user sends ONLY an image with no caption
        let incoming = WhatsAppInboundEvent {
            message_id: Some("msg-2".into()),
            chat_id: "42@s.whatsapp.net".into(),
            sender_id: "42@s.whatsapp.net".into(),
            sender_name: None,
            body: "".into(), // no caption
            media_urls: vec!["/home/.edgecrab/image_cache/img_aabbcc.jpg".into()],
        };
        let message = WhatsAppAdapter::event_to_message(incoming);
        // Text must still include the attachment summary for the agent to see the image
        assert!(
            message.text.contains("Shared 1 attachment:"),
            "empty-body image message must produce attachment summary, got: {:?}",
            message.text
        );
        assert!(message.text.contains("image_cache/img_aabbcc.jpg"));
        assert_eq!(message.metadata.attachments.len(), 1);
    }

    #[test]
    fn no_media_returns_plain_body() {
        let incoming = WhatsAppInboundEvent {
            message_id: None,
            chat_id: "99@s.whatsapp.net".into(),
            sender_id: "99@s.whatsapp.net".into(),
            sender_name: None,
            body: "Hello, world!".into(),
            media_urls: vec![],
        };
        let message = WhatsAppAdapter::event_to_message(incoming);
        assert_eq!(message.text, "Hello, world!");
        assert!(message.metadata.attachments.is_empty());
    }

    #[test]
    fn allowed_users_plus_prefix_is_stripped() {
        // '+' prefix in phone numbers must be removed so they match WhatsApp JIDs
        let source = WhatsAppGatewayConfig {
            enabled: true,
            allowed_users: vec!["+33612345678".into(), "0044123456".into()],
            ..Default::default()
        };
        let config = WhatsappAdapterConfig::from(&source);
        assert_eq!(config.allowed_users, vec!["33612345678", "0044123456"]);
    }

    #[test]
    fn outbound_phone_number_is_normalized_to_personal_jid() {
        assert_eq!(
            normalize_outbound_chat_id("+33614251689"),
            "33614251689@s.whatsapp.net"
        );
        assert_eq!(
            normalize_outbound_chat_id("336 14 25 16 89"),
            "33614251689@s.whatsapp.net"
        );
    }

    #[test]
    fn outbound_existing_jids_are_preserved() {
        assert_eq!(
            normalize_outbound_chat_id("120363048792887850@g.us"),
            "120363048792887850@g.us"
        );
        assert_eq!(
            normalize_outbound_chat_id("131211435503789@lid"),
            "131211435503789@lid"
        );
    }

    #[test]
    fn outbound_device_scoped_jids_are_reduced_to_chat_jids() {
        assert_eq!(
            normalize_outbound_chat_id("33614251689:10@s.whatsapp.net"),
            "33614251689@s.whatsapp.net"
        );
        assert_eq!(
            normalize_outbound_chat_id("131211435503789:10@lid"),
            "131211435503789@lid"
        );
    }

    #[test]
    fn outbound_suffix_shorthand_is_supported() {
        assert_eq!(
            normalize_outbound_chat_id("131211435503789:lid"),
            "131211435503789@lid"
        );
        assert_eq!(
            normalize_outbound_chat_id("33614251689:s.whatsapp.net"),
            "33614251689@s.whatsapp.net"
        );
    }

    #[tokio::test]
    async fn send_document_uses_bridge_media_endpoint() {
        let captured = Arc::new(Mutex::new(Vec::<Value>::new()));
        let captured_state = Arc::clone(&captured);
        let app = Router::new().route(
            "/send-media",
            post(move |Json(body): Json<Value>| {
                let captured = Arc::clone(&captured_state);
                async move {
                    captured.lock().expect("lock").push(body);
                    Json(json!({ "success": true }))
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

        let adapter = WhatsAppAdapter::new(WhatsappAdapterConfig {
            bridge_url: Some(format!("http://{addr}")),
            ..Default::default()
        })
        .expect("adapter");
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("report.pdf");
        std::fs::write(&path, b"edgecrab").expect("write");
        let metadata = MessageMetadata {
            channel_id: Some("+33614251689".into()),
            ..Default::default()
        };

        adapter
            .send_document(path.to_string_lossy().as_ref(), Some("report"), &metadata)
            .await
            .expect("send document");

        let requests = captured.lock().expect("lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["chatId"], "33614251689@s.whatsapp.net");
        assert_eq!(requests[0]["mediaType"], "document");
        assert_eq!(requests[0]["caption"], "report");
        assert_eq!(requests[0]["filePath"], path.to_string_lossy().as_ref());

        server.abort();
    }
}
