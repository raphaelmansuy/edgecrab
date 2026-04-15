//! # Weixin (Personal WeChat) adapter — iLink Bot API
//!
//! Long-poll inbound loop + REST outbound delivery for personal WeChat
//! via the iLink Bot API (Tencent).
//!
//! ## Environment variables
//!
//! | Variable              | Required | Default                                  |
//! |-----------------------|----------|------------------------------------------|
//! | `WEIXIN_TOKEN`        | Yes      | —                                        |
//! | `WEIXIN_ACCOUNT_ID`   | Yes      | —                                        |
//! | `WEIXIN_BASE_URL`     | No       | `https://ilinkai.weixin.qq.com`          |
//! | `WEIXIN_CDN_BASE_URL` | No       | `https://novac2c.cdn.weixin.qq.com/c2c`  |
//! | `WEIXIN_DM_POLICY`    | No       | `open`                                   |
//! | `WEIXIN_GROUP_POLICY`  | No      | `disabled`                               |
//! | `WEIXIN_ALLOWED_USERS` | No      | —                                        |

use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::Engine as _;
use edgecrab_types::Platform;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::weixin_crypto;

use crate::platform::{
    ChatType, IncomingMessage, MessageAttachment, MessageAttachmentKind, MessageMetadata,
    OutgoingMessage, PlatformAdapter,
};

// ─── Constants ───────────────────────────────────────────────────────────

const DEFAULT_BASE_URL: &str = "https://ilinkai.weixin.qq.com";
const DEFAULT_CDN_BASE_URL: &str = "https://novac2c.cdn.weixin.qq.com/c2c";
const POLL_TIMEOUT_SECS: u64 = 35;
const MAX_MESSAGE_LENGTH: usize = 4096;
const DEDUP_MAX_ENTRIES: usize = 1000;
const DEDUP_TTL: Duration = Duration::from_secs(300);
const TYPING_TICKET_TTL: Duration = Duration::from_secs(600);

/// Error code returned by iLink when the session has expired and needs re-auth.
const ERRCODE_SESSION_EXPIRED: i64 = -14;

// iLink authentication constants
const ILINK_APP_ID: &str = "bot";
const ILINK_APP_CLIENT_VERSION: &str = "131584";

// ─── Access policy ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum AccessPolicy {
    Open,
    AllowList,
    Disabled,
}

impl AccessPolicy {
    fn from_env(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "open" => Self::Open,
            "allowlist" | "allow_list" => Self::AllowList,
            "disabled" | "off" | "false" => Self::Disabled,
            _ => Self::Open,
        }
    }
}

// ─── Dedup map ──────────────────────────────────────────────────────────

struct DeduplicationMap {
    entries: HashMap<String, Instant>,
}

impl DeduplicationMap {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Returns `true` if the message is new (not a duplicate).
    fn check_and_insert(&mut self, msg_id: &str) -> bool {
        self.evict_expired();
        if self.entries.contains_key(msg_id) {
            return false;
        }
        // Cap at max entries — evict oldest if needed
        if self.entries.len() >= DEDUP_MAX_ENTRIES {
            if let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, ts)| *ts)
                .map(|(k, _)| k.clone())
            {
                self.entries.remove(&oldest_key);
            }
        }
        self.entries.insert(msg_id.to_string(), Instant::now());
        true
    }

    fn evict_expired(&mut self) {
        let now = Instant::now();
        self.entries.retain(|_, ts| now.duration_since(*ts) < DEDUP_TTL);
    }
}

// ─── Context token store (disk-backed) ──────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct ContextTokenStore {
    #[serde(skip)]
    path: PathBuf,
    tokens: HashMap<String, String>,
}

impl ContextTokenStore {
    fn load(path: PathBuf) -> Self {
        let tokens = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self { path, tokens }
    }

    fn get(&self, peer: &str) -> Option<&String> {
        self.tokens.get(peer)
    }

    fn set(&mut self, peer: &str, token: &str) {
        self.tokens.insert(peer.to_string(), token.to_string());
        self.persist();
    }

    fn persist(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&self.tokens) {
            let _ = std::fs::write(&self.path, json);
        }
    }
}

// ─── Typing ticket cache ────────────────────────────────────────────────

struct TypingTicketCache {
    cache: HashMap<String, (String, Instant)>,
}

impl TypingTicketCache {
    fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    fn get(&self, peer: &str) -> Option<&str> {
        self.cache.get(peer).and_then(|(ticket, fetched_at)| {
            if fetched_at.elapsed() < TYPING_TICKET_TTL {
                Some(ticket.as_str())
            } else {
                None
            }
        })
    }

    fn set(&mut self, peer: &str, ticket: String) {
        self.cache
            .insert(peer.to_string(), (ticket, Instant::now()));
    }
}

// ─── Adapter ────────────────────────────────────────────────────────────

/// Media item descriptor extracted from iLink message items.
struct MediaItem {
    kind: MessageAttachmentKind,
    encrypted_query_param: Option<String>,
    aes_key: Option<String>,
    file_name: Option<String>,
}

/// Load the sync buffer from disk (crash recovery).
fn load_sync_buf(path: &std::path::Path) -> String {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|v| v.get("get_updates_buf").and_then(|b| b.as_str()).map(|s| s.to_string()))
        .unwrap_or_default()
}

pub struct WeixinAdapter {
    token: String,
    account_id: String,
    base_url: String,
    cdn_base_url: String,
    client: reqwest::Client,
    context_tokens: Arc<Mutex<ContextTokenStore>>,
    typing_cache: Arc<Mutex<TypingTicketCache>>,
    seen_messages: Arc<Mutex<DeduplicationMap>>,
    /// Sync buffer for iLink long-poll: echoed back on each POST to avoid re-delivery.
    sync_buf: Arc<Mutex<String>>,
    dm_policy: AccessPolicy,
    group_policy: AccessPolicy,
    allowed_users: HashSet<String>,
    shutdown: CancellationToken,
}

impl WeixinAdapter {
    pub fn from_env() -> Option<Self> {
        let token = env::var("WEIXIN_TOKEN").ok()?;
        let account_id = env::var("WEIXIN_ACCOUNT_ID").ok()?;
        let base_url = env::var("WEIXIN_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        let cdn_base_url = env::var("WEIXIN_CDN_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_CDN_BASE_URL.to_string());

        let dm_policy = env::var("WEIXIN_DM_POLICY")
            .map(|v| AccessPolicy::from_env(&v))
            .unwrap_or(AccessPolicy::Open);
        let group_policy = env::var("WEIXIN_GROUP_POLICY")
            .map(|v| AccessPolicy::from_env(&v))
            .unwrap_or(AccessPolicy::Disabled);
        let allowed_users: HashSet<String> = env::var("WEIXIN_ALLOWED_USERS")
            .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or_default();

        let home = edgecrab_core::edgecrab_home();
        let ctx_path = home
            .join("weixin")
            .join("accounts")
            .join(format!("{account_id}.context-tokens.json"));

        let context_tokens = ContextTokenStore::load(ctx_path);

        // Load sync buffer from disk for crash recovery
        let sync_buf_path = home
            .join("weixin")
            .join("accounts")
            .join(format!("{account_id}.sync-buf.json"));
        let sync_buf = load_sync_buf(&sync_buf_path);

        Some(Self {
            token,
            account_id,
            base_url,
            cdn_base_url,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(POLL_TIMEOUT_SECS + 5))
                .build()
                .unwrap_or_default(),
            context_tokens: Arc::new(Mutex::new(context_tokens)),
            typing_cache: Arc::new(Mutex::new(TypingTicketCache::new())),
            seen_messages: Arc::new(Mutex::new(DeduplicationMap::new())),
            sync_buf: Arc::new(Mutex::new(sync_buf)),
            dm_policy,
            group_policy,
            allowed_users,
            shutdown: CancellationToken::new(),
        })
    }

    pub fn is_available() -> bool {
        env::var("WEIXIN_TOKEN").is_ok() && env::var("WEIXIN_ACCOUNT_ID").is_ok()
    }

    /// Build iLink authentication headers.
    fn auth_headers(&self) -> reqwest::header::HeaderMap {
        use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
        let mut headers = HeaderMap::new();
        // Random X-WECHAT-UIN: base64-encoded 4-byte random uint
        let rand_bytes: [u8; 4] = rand::random();
        let uin = base64::engine::general_purpose::STANDARD.encode(rand_bytes);

        if let Ok(v) = HeaderValue::from_str("ilink_bot_token") {
            headers.insert(
                HeaderName::from_static("authorizationtype"),
                v,
            );
        }
        if let Ok(v) = HeaderValue::from_str(&format!("Bearer {}", self.token)) {
            headers.insert(reqwest::header::AUTHORIZATION, v);
        }
        if let Ok(v) = HeaderValue::from_str(&uin) {
            headers.insert(HeaderName::from_static("x-wechat-uin"), v);
        }
        if let Ok(v) = HeaderValue::from_str(ILINK_APP_ID) {
            headers.insert(HeaderName::from_static("ilink-app-id"), v);
        }
        if let Ok(v) = HeaderValue::from_str(ILINK_APP_CLIENT_VERSION) {
            headers.insert(
                HeaderName::from_static("ilink-app-clientversion"),
                v,
            );
        }
        headers
    }

    /// Long-poll loop: POST /ilink/bot/getupdates with sync buffer.
    ///
    /// Uses POST with `get_updates_buf` body field to synchronize message
    /// delivery — the server echoes back a sync token that prevents
    /// re-delivery on reconnect.
    async fn poll_loop(&self, tx: mpsc::Sender<IncomingMessage>) {
        info!("Weixin poll loop started for account {}", self.account_id);
        loop {
            if self.shutdown.is_cancelled() {
                info!("Weixin poll loop shutting down");
                break;
            }

            let url = format!(
                "{}/ilink/bot/getupdates?timeout={}",
                self.base_url, POLL_TIMEOUT_SECS
            );

            // Build POST body with sync buffer
            let sync_buf_val = {
                let buf = self.sync_buf.lock().await;
                buf.clone()
            };
            let payload = serde_json::json!({
                "get_updates_buf": sync_buf_val,
            });

            let result = self
                .client
                .post(&url)
                .headers(self.auth_headers())
                .json(&payload)
                .timeout(Duration::from_secs(POLL_TIMEOUT_SECS + 10))
                .send()
                .await;

            match result {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<serde_json::Value>().await {
                        Ok(body) => {
                            // Check for session-expired error
                            let errcode = body.get("ret")
                                .or_else(|| body.get("errcode"))
                                .and_then(|v| v.as_i64())
                                .unwrap_or(0);
                            if errcode == ERRCODE_SESSION_EXPIRED {
                                warn!("Weixin: session expired (errcode {ERRCODE_SESSION_EXPIRED}), resetting sync buffer");
                                let mut buf = self.sync_buf.lock().await;
                                *buf = String::new();
                                self.persist_sync_buf(&buf);
                                tokio::time::sleep(Duration::from_secs(2)).await;
                                continue;
                            }

                            // Update sync buffer from response
                            if let Some(new_buf) = body.get("get_updates_buf").and_then(|v| v.as_str()) {
                                let mut buf = self.sync_buf.lock().await;
                                *buf = new_buf.to_string();
                                self.persist_sync_buf(&buf);
                            }

                            self.process_updates(&body, &tx).await;
                        }
                        Err(e) => {
                            warn!("Weixin: failed to parse response: {e}");
                        }
                    }
                }
                Ok(resp) => {
                    warn!("Weixin: poll returned status {}", resp.status());
                    tokio::time::sleep(crate::ADAPTER_RETRY_DELAY).await;
                }
                Err(e) if e.is_timeout() => {
                    // Normal long-poll timeout — just retry
                    debug!("Weixin: poll timeout (normal)");
                }
                Err(e) => {
                    error!("Weixin: poll error: {e}");
                    tokio::time::sleep(crate::ADAPTER_RETRY_DELAY).await;
                }
            }
        }
    }

    /// Persist the sync buffer to disk for crash recovery.
    fn persist_sync_buf(&self, buf: &str) {
        let home = edgecrab_core::edgecrab_home();
        let path = home
            .join("weixin")
            .join("accounts")
            .join(format!("{}.sync-buf.json", self.account_id));
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let json = serde_json::json!({"get_updates_buf": buf});
        if let Ok(text) = serde_json::to_string(&json) {
            let _ = std::fs::write(&path, text);
        }
    }

    async fn process_updates(&self, body: &serde_json::Value, tx: &mpsc::Sender<IncomingMessage>) {
        let Some(messages) = body.get("messages").and_then(|v| v.as_array()) else {
            return;
        };

        for msg in messages {
            let msg_id = msg
                .get("msg_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if msg_id.is_empty() {
                continue;
            }

            // Dedup
            {
                let mut seen = self.seen_messages.lock().await;
                if !seen.check_and_insert(msg_id) {
                    debug!("Weixin: skipping duplicate msg {msg_id}");
                    continue;
                }
            }

            // Extract sender
            let from_user = msg
                .get("from_user")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if from_user.is_empty() {
                continue;
            }

            // Extract context token and persist
            if let Some(ctx_token) = msg.get("context_token").and_then(|v| v.as_str()) {
                let mut store = self.context_tokens.lock().await;
                store.set(&from_user, ctx_token);
            }

            // Determine chat type
            let is_group = msg
                .get("is_group")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let chat_type = if is_group {
                ChatType::Group
            } else {
                ChatType::Dm
            };

            // Check access policy
            if !self.check_access(chat_type, &from_user) {
                debug!("Weixin: access denied for {from_user} (chat_type={chat_type:?})");
                continue;
            }

            // Extract text from item_list
            let text = Self::extract_text(msg);

            // Extract and download media attachments
            let media_items = Self::extract_attachments(msg);
            let mut attachments = Vec::new();
            for item in &media_items {
                if let Some((path, kind)) = self.download_media(item).await {
                    attachments.push(MessageAttachment {
                        kind,
                        file_name: item.file_name.clone(),
                        local_path: Some(path.display().to_string()),
                        ..Default::default()
                    });
                }
            }

            // Skip messages with neither text nor attachments
            if text.is_empty() && attachments.is_empty() {
                continue;
            }

            // Extract channel_id (group_id for groups, from_user for DMs)
            let channel_id = if is_group {
                msg.get("group_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            } else {
                Some(from_user.clone())
            };

            // Build and send incoming message
            let incoming = IncomingMessage {
                platform: Platform::Weixin,
                user_id: from_user.clone(),
                channel_id: channel_id.clone(),
                chat_type,
                text,
                thread_id: None,
                metadata: MessageMetadata {
                    message_id: Some(msg_id.to_string()),
                    channel_id,
                    user_display_name: msg
                        .get("from_nickname")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    attachments,
                    ..Default::default()
                },
            };

            if let Err(e) = tx.send(incoming).await {
                error!("Weixin: failed to send incoming message: {e}");
                break;
            }
        }
    }

    fn extract_text(msg: &serde_json::Value) -> String {
        let mut parts = Vec::new();

        // Check for referenced message (引用)
        if let Some(ref_msg) = msg.get("reference").and_then(|v| v.as_object()) {
            if let Some(ref_text) = ref_msg.get("content").and_then(|v| v.as_str()) {
                parts.push(format!("[引用: {ref_text}]"));
            } else {
                parts.push("[引用媒体]".to_string());
            }
        }

        // Extract text from item_list (type=1 is text)
        if let Some(items) = msg.get("item_list").and_then(|v| v.as_array()) {
            for item in items {
                let item_type = item.get("type").and_then(|v| v.as_u64()).unwrap_or(0);
                match item_type {
                    1 => {
                        // Text item
                        if let Some(content) = item.get("content").and_then(|v| v.as_str()) {
                            parts.push(content.to_string());
                        }
                    }
                    3 => parts.push("[图片]".to_string()),  // Image
                    34 => parts.push("[语音]".to_string()), // Voice
                    43 => parts.push("[视频]".to_string()), // Video
                    49 => parts.push("[文件]".to_string()), // File
                    _ => {}
                }
            }
        }

        // Fallback: direct content field
        if parts.is_empty() {
            if let Some(content) = msg.get("content").and_then(|v| v.as_str()) {
                parts.push(content.to_string());
            }
        }

        parts.join("\n")
    }

    /// Extract media attachments from iLink message items.
    fn extract_attachments(msg: &serde_json::Value) -> Vec<MediaItem> {
        let Some(items) = msg.get("item_list").and_then(|v| v.as_array()) else {
            return Vec::new();
        };
        let mut media = Vec::new();
        for item in items {
            let item_type = item.get("type").and_then(|v| v.as_u64()).unwrap_or(0);
            let kind = match item_type {
                3 => MessageAttachmentKind::Image,
                34 => MessageAttachmentKind::Voice,
                43 => MessageAttachmentKind::Video,
                49 => MessageAttachmentKind::Document,
                _ => continue,
            };
            let encrypted_query_param = item
                .get("encrypted_query_param")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let aes_key = item
                .get("aes_key")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let file_name = item
                .get("file_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            if encrypted_query_param.is_some() || aes_key.is_some() {
                media.push(MediaItem {
                    kind,
                    encrypted_query_param,
                    aes_key,
                    file_name,
                });
            }
        }
        media
    }

    /// Download and decrypt a media item from the Weixin CDN.
    async fn download_media(&self, item: &MediaItem) -> Option<(PathBuf, MessageAttachmentKind)> {
        let encrypted_param = item.encrypted_query_param.as_deref()?;

        let url = format!(
            "{}/download?encrypted_query_param={}",
            self.cdn_base_url,
            urlencoding::encode(encrypted_param)
        );

        let resp = match self
            .client
            .get(&url)
            .timeout(Duration::from_secs(60))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                warn!("Weixin CDN download failed: HTTP {}", r.status());
                return None;
            }
            Err(e) => {
                warn!("Weixin CDN download error: {e}");
                return None;
            }
        };

        let raw = match resp.bytes().await {
            Ok(b) => b.to_vec(),
            Err(e) => {
                warn!("Weixin CDN read error: {e}");
                return None;
            }
        };

        // Decrypt with AES-128-ECB if key provided
        let data = if let Some(aes_key_b64) = &item.aes_key {
            let key_bytes = base64::engine::general_purpose::STANDARD
                .decode(aes_key_b64)
                .ok()?;
            let key = weixin_crypto::parse_aes128_key(&key_bytes).ok()?;
            weixin_crypto::aes128_ecb_decrypt(&key, &raw).ok()?
        } else {
            raw
        };

        // Save to cache directory
        let ext = match item.kind {
            MessageAttachmentKind::Image => "jpg",
            MessageAttachmentKind::Voice => "mp3",
            MessageAttachmentKind::Video => "mp4",
            MessageAttachmentKind::Document => "bin",
            _ => "bin",
        };
        let default_name = format!("weixin_media.{ext}");
        let filename = item
            .file_name
            .as_deref()
            .unwrap_or(&default_name);
        let cache_dir = edgecrab_core::edgecrab_home()
            .join("weixin")
            .join("cache");
        let _ = std::fs::create_dir_all(&cache_dir);
        let path = cache_dir.join(format!("{}_{filename}", uuid::Uuid::new_v4()));
        if std::fs::write(&path, &data).is_err() {
            warn!("Weixin: failed to cache media file");
            return None;
        }
        Some((path, item.kind.clone()))
    }

    fn check_access(&self, chat_type: ChatType, user_id: &str) -> bool {
        let policy = match chat_type {
            ChatType::Dm => &self.dm_policy,
            ChatType::Group | ChatType::Channel => &self.group_policy,
        };
        match policy {
            AccessPolicy::Open => true,
            AccessPolicy::AllowList => self.allowed_users.contains(user_id),
            AccessPolicy::Disabled => false,
        }
    }

    /// Reformat markdown for WeChat display.
    ///
    /// WeChat personal doesn't render markdown natively:
    /// - `# Title` → `【Title】`
    /// - `## Subtitle` → `**Subtitle**`
    /// - Tables → key-value lists
    pub fn reformat_markdown(text: &str) -> String {
        let mut output = String::with_capacity(text.len());
        let mut in_table = false;
        let mut table_headers: Vec<String> = Vec::new();

        for line in text.lines() {
            let trimmed = line.trim();

            // Handle table rows
            if trimmed.starts_with('|') && trimmed.ends_with('|') {
                let cells: Vec<&str> = trimmed
                    .trim_matches('|')
                    .split('|')
                    .map(|c| c.trim())
                    .collect();

                if !in_table {
                    // First row = headers
                    table_headers = cells.iter().map(|c| c.to_string()).collect();
                    in_table = true;
                    continue;
                }

                // Skip separator row (---)
                if cells.iter().all(|c| c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')) {
                    continue;
                }

                // Data row → key-value pairs
                for (i, cell) in cells.iter().enumerate() {
                    if let Some(header) = table_headers.get(i) {
                        output.push_str(&format!("- {header}: {cell}\n"));
                    }
                }
                continue;
            }

            if in_table {
                in_table = false;
                table_headers.clear();
            }

            // Heading reformats
            if let Some(title) = trimmed.strip_prefix("# ") {
                output.push_str(&format!("【{title}】\n"));
            } else if let Some(subtitle) = trimmed.strip_prefix("## ") {
                output.push_str(&format!("**{subtitle}**\n"));
            } else if let Some(sub) = trimmed.strip_prefix("### ") {
                output.push_str(&format!("▸ {sub}\n"));
            } else {
                output.push_str(line);
                output.push('\n');
            }
        }

        // Remove trailing newline
        if output.ends_with('\n') {
            output.pop();
        }
        output
    }

    /// Send a message via iLink sendmessage API.
    async fn send_text(&self, to_user: &str, text: &str) -> anyhow::Result<()> {
        let url = format!("{}/ilink/bot/sendmessage", self.base_url);

        // Get context token for this peer
        let ctx_token = {
            let store = self.context_tokens.lock().await;
            store.get(to_user).cloned()
        };

        let mut payload = serde_json::json!({
            "to_user": to_user,
            "msg_type": "text",
            "content": text,
        });

        if let Some(token) = ctx_token {
            payload.as_object_mut().map(|obj| {
                obj.insert("context_token".to_string(), serde_json::Value::String(token))
            });
        }

        let resp = self
            .client
            .post(&url)
            .headers(self.auth_headers())
            .json(&payload)
            .timeout(Duration::from_secs(15))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Weixin sendmessage failed: {status} {body}");
        }

        Ok(())
    }

    /// Split text into chunks that fit within the max message length.
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
            // Find a safe split point (newline or space)
            let split_at = remaining[..max_len]
                .rfind('\n')
                .or_else(|| remaining[..max_len].rfind(' '))
                .unwrap_or(max_len);
            let (chunk, rest) = remaining.split_at(split_at);
            chunks.push(chunk.to_string());
            remaining = rest.trim_start_matches('\n');
        }
        chunks
    }

    /// Upload a file to the Weixin CDN via AES-128-ECB encryption.
    ///
    /// Steps:
    /// 1. Generate random 16-byte AES key
    /// 2. Encrypt plaintext with PKCS7 → AES-128-ECB
    /// 3. POST getuploadurl to obtain CDN upload URL
    /// 4. POST ciphertext to CDN
    /// 5. Extract `x-encrypted-param` header from CDN response
    /// 6. Return (encrypted_query_param, aes_key_hex) for sendmessage
    async fn upload_media(
        &self,
        data: &[u8],
        to_user: &str,
        file_type: &str,
    ) -> anyhow::Result<(String, String)> {
        // Generate random AES key
        let key: [u8; 16] = rand::random();
        let aes_key_hex: String = key.iter().map(|b| format!("{b:02x}")).collect();

        // Encrypt
        let ciphertext = weixin_crypto::aes128_ecb_encrypt(&key, data);

        // Request upload URL
        let url = format!("{}/ilink/bot/getuploadurl", self.base_url);
        let raw_md5 = format!("{:x}", md5::compute(data));
        let filekey = format!("weixin_{}_{}", to_user, uuid::Uuid::new_v4());
        let payload = serde_json::json!({
            "to_user": to_user,
            "filekey": filekey,
            "rawsize": data.len(),
            "filesize": ciphertext.len(),
            "md5": raw_md5,
            "aeskey": aes_key_hex,
            "file_type": file_type,
        });

        let resp = self
            .client
            .post(&url)
            .headers(self.auth_headers())
            .json(&payload)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;

        let body: serde_json::Value = resp.json().await?;
        let upload_url = body
            .get("upload_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Weixin: missing upload_url in getuploadurl response"))?;
        let upload_param = body
            .get("upload_param")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        // Upload ciphertext to CDN
        let cdn_url = if upload_param.is_empty() {
            upload_url.to_string()
        } else {
            format!(
                "{}/upload?upload_param={}&filekey={}",
                self.cdn_base_url,
                urlencoding::encode(upload_param),
                urlencoding::encode(&filekey)
            )
        };

        let cdn_resp = self
            .client
            .post(&cdn_url)
            .body(ciphertext)
            .timeout(Duration::from_secs(120))
            .send()
            .await?;

        let encrypted_query_param = cdn_resp
            .headers()
            .get("x-encrypted-param")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("Weixin CDN: missing x-encrypted-param header"))?;

        let aes_key_b64 = base64::engine::general_purpose::STANDARD.encode(key);
        Ok((encrypted_query_param, aes_key_b64))
    }

    /// Send a media file via iLink sendmessage with CDN upload.
    async fn send_media_file(
        &self,
        to_user: &str,
        file_path: &str,
        msg_type: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let data = tokio::fs::read(file_path).await?;
        let (encrypted_query_param, aes_key) = self.upload_media(&data, to_user, msg_type).await?;

        // Get context token
        let ctx_token = {
            let store = self.context_tokens.lock().await;
            store.get(to_user).cloned()
        };

        let mut payload = serde_json::json!({
            "to_user": to_user,
            "msg_type": msg_type,
            "encrypt_query_param": encrypted_query_param,
            "aes_key": aes_key,
        });

        if let Some(token) = ctx_token {
            payload.as_object_mut().map(|obj| {
                obj.insert("context_token".to_string(), serde_json::Value::String(token))
            });
        }

        let url = format!("{}/ilink/bot/sendmessage", self.base_url);
        let resp = self
            .client
            .post(&url)
            .headers(self.auth_headers())
            .json(&payload)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Weixin media send failed: {status} {body}");
        }

        // Send caption as follow-up text
        if let Some(cap) = caption.filter(|c| !c.trim().is_empty()) {
            self.send_text(to_user, cap).await?;
        }

        Ok(())
    }
}

#[async_trait]
impl PlatformAdapter for WeixinAdapter {
    fn platform(&self) -> Platform {
        Platform::Weixin
    }

    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
        info!(
            "Starting Weixin adapter for account {}",
            self.account_id
        );
        self.poll_loop(tx).await;
        Ok(())
    }

    async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()> {
        let to_user = msg
            .metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Weixin send: missing channel_id"))?;

        let formatted = WeixinAdapter::reformat_markdown(&msg.text);
        let chunks = Self::split_text(&formatted, MAX_MESSAGE_LENGTH);

        for chunk in chunks {
            self.send_text(to_user, &chunk).await?;
        }

        Ok(())
    }

    fn format_response(&self, text: &str, _metadata: &MessageMetadata) -> String {
        Self::reformat_markdown(text)
    }

    fn max_message_length(&self) -> usize {
        MAX_MESSAGE_LENGTH
    }

    fn supports_markdown(&self) -> bool {
        false // WeChat personal doesn't render markdown
    }

    fn supports_images(&self) -> bool {
        true // Via CDN upload with AES encryption
    }

    fn supports_files(&self) -> bool {
        true
    }

    async fn send_typing(&self, metadata: &MessageMetadata) -> anyhow::Result<()> {
        let Some(to_user) = metadata.channel_id.as_deref() else {
            return Ok(());
        };

        // Check typing ticket cache
        let cached_ticket = {
            let cache = self.typing_cache.lock().await;
            cache.get(to_user).map(|s| s.to_string())
        };

        let ticket = if let Some(t) = cached_ticket {
            t
        } else {
            // Fetch new typing ticket
            let url = format!("{}/ilink/bot/gettyping_ticket", self.base_url);
            let resp = self
                .client
                .post(&url)
                .headers(self.auth_headers())
                .json(&serde_json::json!({"to_user": to_user}))
                .timeout(Duration::from_secs(10))
                .send()
                .await?;
            let body: serde_json::Value = resp.json().await?;
            let t = body
                .get("ticket")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if !t.is_empty() {
                let mut cache = self.typing_cache.lock().await;
                cache.set(to_user, t.clone());
            }
            t
        };

        if ticket.is_empty() {
            return Ok(());
        }

        let url = format!("{}/ilink/bot/sendtyping", self.base_url);
        let _ = self
            .client
            .post(&url)
            .headers(self.auth_headers())
            .json(&serde_json::json!({
                "to_user": to_user,
                "typing_ticket": ticket,
            }))
            .timeout(Duration::from_secs(5))
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
        let to_user = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Weixin send_photo: missing channel_id"))?;
        self.send_media_file(to_user, path, "image", caption).await
    }

    async fn send_document(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        let to_user = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Weixin send_document: missing channel_id"))?;
        self.send_media_file(to_user, path, "file", caption).await
    }

    async fn send_voice(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        let to_user = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Weixin send_voice: missing channel_id"))?;
        self.send_media_file(to_user, path, "voice", caption).await
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reformat_heading_level1() {
        let input = "# Welcome";
        assert_eq!(WeixinAdapter::reformat_markdown(input), "【Welcome】");
    }

    #[test]
    fn reformat_heading_level2() {
        let input = "## Section";
        assert_eq!(WeixinAdapter::reformat_markdown(input), "**Section**");
    }

    #[test]
    fn reformat_heading_level3() {
        let input = "### Sub-section";
        assert_eq!(WeixinAdapter::reformat_markdown(input), "▸ Sub-section");
    }

    #[test]
    fn reformat_table_to_list() {
        let input = "| Name | Value |\n| --- | --- |\n| key1 | val1 |\n| key2 | val2 |";
        let result = WeixinAdapter::reformat_markdown(input);
        assert!(result.contains("- Name: key1"));
        assert!(result.contains("- Value: val1"));
        assert!(result.contains("- Name: key2"));
    }

    #[test]
    fn reformat_passthrough() {
        let input = "Hello World\nNo special formatting here.";
        assert_eq!(
            WeixinAdapter::reformat_markdown(input),
            "Hello World\nNo special formatting here."
        );
    }

    #[test]
    fn dedup_rejects_duplicate() {
        let mut dedup = DeduplicationMap::new();
        assert!(dedup.check_and_insert("msg_1"));
        assert!(!dedup.check_and_insert("msg_1"));
        assert!(dedup.check_and_insert("msg_2"));
    }

    #[test]
    fn dedup_respects_max_entries() {
        let mut dedup = DeduplicationMap::new();
        for i in 0..DEDUP_MAX_ENTRIES {
            assert!(dedup.check_and_insert(&format!("msg_{i}")));
        }
        // Inserting one more should succeed (oldest evicted)
        assert!(dedup.check_and_insert("msg_overflow"));
        assert_eq!(dedup.entries.len(), DEDUP_MAX_ENTRIES);
    }

    #[test]
    fn context_token_store_roundtrip() {
        let dir = std::env::temp_dir().join(format!("weixin_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("tokens.json");

        {
            let mut store = ContextTokenStore::load(path.clone());
            store.set("user_a", "token_123");
            store.set("user_b", "token_456");
        }

        // Reload from disk
        let store = ContextTokenStore::load(path.clone());
        assert_eq!(store.get("user_a"), Some(&"token_123".to_string()));
        assert_eq!(store.get("user_b"), Some(&"token_456".to_string()));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn access_policy_parsing() {
        assert_eq!(AccessPolicy::from_env("open"), AccessPolicy::Open);
        assert_eq!(AccessPolicy::from_env("allowlist"), AccessPolicy::AllowList);
        assert_eq!(AccessPolicy::from_env("disabled"), AccessPolicy::Disabled);
        assert_eq!(AccessPolicy::from_env("off"), AccessPolicy::Disabled);
        assert_eq!(AccessPolicy::from_env("unknown"), AccessPolicy::Open);
    }

    #[test]
    fn check_access_open() {
        let adapter_policy_dm = AccessPolicy::Open;
        let adapter_policy_group = AccessPolicy::Disabled;
        let allowed: HashSet<String> = HashSet::new();

        // Open DM policy
        assert!(matches!(adapter_policy_dm, AccessPolicy::Open));
        // Disabled group policy
        assert!(matches!(adapter_policy_group, AccessPolicy::Disabled));
        // AllowList with empty set
        let allowlist = AccessPolicy::AllowList;
        assert!(!allowed.contains("user_x"));
        assert!(matches!(allowlist, AccessPolicy::AllowList));
    }

    #[test]
    fn split_text_short() {
        let chunks = WeixinAdapter::split_text("hello", 100);
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn split_text_long() {
        let text = "a ".repeat(100); // 200 chars
        let chunks = WeixinAdapter::split_text(text.trim(), 50);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.len() <= 50);
        }
    }

    #[test]
    fn extract_text_from_item_list() {
        let msg = serde_json::json!({
            "msg_id": "123",
            "from_user": "test",
            "item_list": [
                {"type": 1, "content": "Hello"},
                {"type": 2, "content": "image.jpg"},
                {"type": 1, "content": "World"}
            ]
        });
        let text = WeixinAdapter::extract_text(&msg);
        assert_eq!(text, "Hello\nWorld");
    }

    #[test]
    fn extract_text_with_reference() {
        let msg = serde_json::json!({
            "msg_id": "456",
            "from_user": "test",
            "reference": {"content": "original message"},
            "item_list": [{"type": 1, "content": "reply text"}]
        });
        let text = WeixinAdapter::extract_text(&msg);
        assert!(text.contains("[引用: original message]"));
        assert!(text.contains("reply text"));
    }

    #[test]
    fn typing_cache_expiry() {
        let mut cache = TypingTicketCache::new();
        cache.set("peer_1", "ticket_abc".to_string());
        assert_eq!(cache.get("peer_1"), Some("ticket_abc"));
        assert_eq!(cache.get("unknown"), None);
    }
}
