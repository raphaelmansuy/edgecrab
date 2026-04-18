//! # Feishu adapter — webhook ingress + REST egress
//!
//! Core path covered by this adapter:
//! - inbound webhook verification and event ingestion
//! - allowlist filtering and duplicate-event suppression
//! - outbound tenant token refresh and text replies

use std::collections::{HashMap, HashSet};
use std::env;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock as StdRwLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use edgecrab_core::edgecrab_home;
use edgecrab_types::Platform;
use regex::Regex;
use reqwest::multipart::{Form, Part};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, RwLock, mpsc};
use uuid::Uuid;

use crate::delivery::split_message;
use crate::platform::{
    IncomingMessage, MessageAttachment, MessageAttachmentKind, MessageMetadata, OutgoingMessage,
    PlatformAdapter,
};

const DEFAULT_BASE_URL: &str = "https://open.feishu.cn";
const DEFAULT_WEBHOOK_HOST: &str = "127.0.0.1";
const DEFAULT_WEBHOOK_PATH: &str = "/feishu/webhook";
const DEFAULT_WEBHOOK_PORT: u16 = 8765;
const MAX_MESSAGE_LENGTH: usize = 8000;
const DEDUP_TTL: Duration = Duration::from_secs(60 * 60 * 24);
const FEISHU_DEDUP_CACHE_SIZE: usize = 2048;
const FEISHU_WEBHOOK_MAX_BODY_BYTES: usize = 1024 * 1024;
const FEISHU_WEBHOOK_RATE_LIMIT_MAX: usize = 120;
const FEISHU_WEBHOOK_RATE_MAX_KEYS: usize = 4096;
const FEISHU_WEBHOOK_RATE_WINDOW: Duration = Duration::from_secs(60);
const FEISHU_WEBHOOK_ANOMALY_THRESHOLD: u32 = 25;
const FEISHU_WEBHOOK_ANOMALY_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const TOKEN_REFRESH_SKEW: Duration = Duration::from_secs(60);
const FEISHU_IMAGE_UPLOAD_TYPE: &str = "message";
const FEISHU_FILE_UPLOAD_TYPE: &str = "stream";
const FEISHU_REPLY_FALLBACK_CODES: [i64; 2] = [230011, 231003];
const FEISHU_CARD_ACTION_DEDUP_TTL: Duration = Duration::from_secs(15 * 60);
const FALLBACK_POST_TEXT: &str = "[Post]";
const FALLBACK_IMAGE_TEXT: &str = "[Image]";
const FALLBACK_ATTACHMENT_TEXT: &str = "[Attachment]";
const FALLBACK_FORWARD_TEXT: &str = "[Forwarded messages]";
const FALLBACK_SHARE_CHAT_TEXT: &str = "[Shared chat]";
const FALLBACK_INTERACTIVE_TEXT: &str = "[Interactive card]";

#[derive(Clone)]
struct FeishuWebhookState {
    tx: mpsc::Sender<IncomingMessage>,
    adapter: FeishuAdapter,
}

#[derive(Clone)]
struct CachedToken {
    value: String,
    expires_at: Instant,
}

#[derive(Clone)]
struct WebhookAnomalyState {
    count: u32,
    last_status: String,
    first_seen: Instant,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct FeishuNormalizedMessage {
    text: String,
    attachments: Vec<MessageAttachment>,
    mentioned_ids: Vec<String>,
    resources: Vec<FeishuResourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FeishuResourceRef {
    attachment_index: usize,
    file_key: String,
    resource_type: &'static str,
}

#[derive(Clone)]
pub struct FeishuAdapter {
    app_id: String,
    app_secret: String,
    base_url: String,
    webhook_host: String,
    webhook_port: u16,
    webhook_path: String,
    verification_token: Option<String>,
    encrypt_key: Option<String>,
    allowed_users: Arc<HashSet<String>>,
    seen_event_ids: Arc<Mutex<HashMap<String, Instant>>>,
    dedup_state_path: Option<PathBuf>,
    card_action_tokens: Arc<Mutex<HashMap<String, Instant>>>,
    webhook_rate_limits: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
    webhook_anomalies: Arc<Mutex<HashMap<String, WebhookAnomalyState>>>,
    tenant_token: Arc<RwLock<Option<CachedToken>>>,
    bot_name: Arc<StdRwLock<Option<String>>>,
    bot_identity_lookup_attempted: Arc<Mutex<bool>>,
    http: reqwest::Client,
}

impl FeishuAdapter {
    pub fn from_env() -> Option<Self> {
        let app_id = env::var("FEISHU_APP_ID").ok()?.trim().to_string();
        let app_secret = env::var("FEISHU_APP_SECRET").ok()?.trim().to_string();
        if app_id.is_empty() || app_secret.is_empty() {
            return None;
        }

        let webhook_port = env::var("FEISHU_WEBHOOK_PORT")
            .ok()
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(DEFAULT_WEBHOOK_PORT);
        let dedup_state_path = edgecrab_home().join("feishu_seen_event_ids.json");

        Some(Self {
            app_id,
            app_secret,
            base_url: env::var("FEISHU_BASE_URL")
                .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string())
                .trim_end_matches('/')
                .to_string(),
            webhook_host: env::var("FEISHU_WEBHOOK_HOST")
                .unwrap_or_else(|_| DEFAULT_WEBHOOK_HOST.to_string()),
            webhook_port,
            webhook_path: normalize_path(
                &env::var("FEISHU_WEBHOOK_PATH")
                    .unwrap_or_else(|_| DEFAULT_WEBHOOK_PATH.to_string()),
            ),
            verification_token: env::var("FEISHU_VERIFICATION_TOKEN")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            encrypt_key: env::var("FEISHU_ENCRYPT_KEY")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            allowed_users: Arc::new(parse_csv_set("FEISHU_ALLOWED_USERS")),
            seen_event_ids: Arc::new(Mutex::new(load_seen_event_ids(&dedup_state_path))),
            dedup_state_path: Some(dedup_state_path),
            card_action_tokens: Arc::new(Mutex::new(HashMap::new())),
            webhook_rate_limits: Arc::new(Mutex::new(HashMap::new())),
            webhook_anomalies: Arc::new(Mutex::new(HashMap::new())),
            tenant_token: Arc::new(RwLock::new(None)),
            bot_name: Arc::new(StdRwLock::new(None)),
            bot_identity_lookup_attempted: Arc::new(Mutex::new(false)),
            http: reqwest::Client::new(),
        })
    }

    pub fn is_available() -> bool {
        env::var("FEISHU_APP_ID").is_ok() && env::var("FEISHU_APP_SECRET").is_ok()
    }

    fn router(&self, tx: mpsc::Sender<IncomingMessage>) -> Router {
        let state = FeishuWebhookState {
            tx,
            adapter: self.clone(),
        };
        Router::new()
            .route(&self.webhook_path, post(handle_webhook))
            .with_state(state)
    }

    async fn ensure_bot_identity(&self) {
        if env::var("FEISHU_BOT_OPEN_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .is_some()
            || env::var("FEISHU_BOT_USER_ID")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .is_some()
            || env::var("FEISHU_BOT_NAME")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .is_some()
            || self
                .bot_name
                .read()
                .ok()
                .and_then(|value| value.clone())
                .is_some()
        {
            return;
        }

        let mut attempted = self.bot_identity_lookup_attempted.lock().await;
        if *attempted {
            return;
        }
        *attempted = true;
        drop(attempted);

        let token = match self.tenant_access_token().await {
            Ok(token) => token,
            Err(error) => {
                tracing::debug!(%error, "Feishu bot identity hydration skipped");
                return;
            }
        };

        let response = match self
            .http
            .get(format!(
                "{}/open-apis/application/v6/applications/{}",
                self.base_url, self.app_id
            ))
            .bearer_auth(token)
            .query(&[("lang", "en_us")])
            .timeout(Duration::from_secs(15))
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                tracing::debug!(%error, "Feishu bot identity request failed");
                return;
            }
        };

        let Ok((status, payload)) = read_feishu_json(response, "bot identity").await else {
            return;
        };
        if !status.is_success() {
            tracing::debug!(status = %status, "Feishu bot identity request returned non-success");
            return;
        }

        let code = payload
            .get("code")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        if code != 0 {
            if code == 99991672 {
                tracing::warn!(
                    "Feishu bot identity hydration requires app self-manage permission for precise mention gating"
                );
            } else {
                tracing::debug!(code, payload = %payload, "Feishu bot identity hydration rejected");
            }
            return;
        }

        let bot_name = payload
            .get("data")
            .and_then(|value| value.get("app"))
            .and_then(|value| value.get("app_name"))
            .and_then(Value::as_str)
            .or_else(|| {
                payload
                    .get("data")
                    .and_then(|value| value.get("app_name"))
                    .and_then(Value::as_str)
            })
            .or_else(|| {
                payload
                    .get("data")
                    .and_then(|value| value.get("name"))
                    .and_then(Value::as_str)
            })
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        if let Some(bot_name) = bot_name {
            if let Ok(mut slot) = self.bot_name.write() {
                *slot = Some(bot_name);
            }
        }
    }

    async fn is_duplicate_card_action(&self, token: &str) -> bool {
        if token.trim().is_empty() {
            return false;
        }

        let mut guard = self.card_action_tokens.lock().await;
        let now = Instant::now();
        guard.retain(|_, seen_at| now.duration_since(*seen_at) <= FEISHU_CARD_ACTION_DEDUP_TTL);
        if guard.contains_key(token) {
            return true;
        }
        guard.insert(token.to_string(), now);
        false
    }

    async fn is_duplicate_event(&self, event_id: &str) -> bool {
        let mut guard = self.seen_event_ids.lock().await;
        let now = Instant::now();
        guard.retain(|_, seen_at| now.duration_since(*seen_at) <= DEDUP_TTL);
        if guard.contains_key(event_id) {
            return true;
        }
        guard.insert(event_id.to_string(), now);
        persist_seen_event_ids(self.dedup_state_path.as_deref(), &guard);
        false
    }

    async fn check_webhook_rate_limit(&self, remote_ip: &str) -> bool {
        let key = format!("{}:{}:{remote_ip}", self.app_id, self.webhook_path);
        let mut guard = self.webhook_rate_limits.lock().await;
        let now = Instant::now();
        guard.retain(|_, timestamps| {
            timestamps.retain(|ts| now.duration_since(*ts) <= FEISHU_WEBHOOK_RATE_WINDOW);
            !timestamps.is_empty()
        });

        if guard.len() >= FEISHU_WEBHOOK_RATE_MAX_KEYS && !guard.contains_key(&key) {
            return false;
        }

        let timestamps = guard.entry(key).or_default();
        timestamps.retain(|ts| now.duration_since(*ts) <= FEISHU_WEBHOOK_RATE_WINDOW);
        if timestamps.len() >= FEISHU_WEBHOOK_RATE_LIMIT_MAX {
            return false;
        }
        timestamps.push(now);
        true
    }

    async fn record_webhook_anomaly(&self, remote_ip: &str, status: &str) {
        let mut guard = self.webhook_anomalies.lock().await;
        let now = Instant::now();
        guard.retain(|_, state| now.duration_since(state.first_seen) <= FEISHU_WEBHOOK_ANOMALY_TTL);

        let entry = guard
            .entry(remote_ip.to_string())
            .or_insert(WebhookAnomalyState {
                count: 0,
                last_status: status.to_string(),
                first_seen: now,
            });
        if now.duration_since(entry.first_seen) > FEISHU_WEBHOOK_ANOMALY_TTL {
            *entry = WebhookAnomalyState {
                count: 1,
                last_status: status.to_string(),
                first_seen: now,
            };
            return;
        }

        entry.count = entry.count.saturating_add(1);
        entry.last_status = status.to_string();
        if entry.count % FEISHU_WEBHOOK_ANOMALY_THRESHOLD == 0 {
            tracing::warn!(
                remote_ip,
                status,
                count = entry.count,
                elapsed_secs = now.duration_since(entry.first_seen).as_secs_f32(),
                "Feishu webhook anomaly threshold reached"
            );
        }
    }

    async fn clear_webhook_anomaly(&self, remote_ip: &str) {
        self.webhook_anomalies.lock().await.remove(remote_ip);
    }

    fn is_webhook_signature_valid(&self, headers: &HeaderMap, body: &[u8]) -> bool {
        let Some(encrypt_key) = self.encrypt_key.as_deref() else {
            return true;
        };

        let timestamp = header_value(headers, "x-lark-request-timestamp");
        let nonce = header_value(headers, "x-lark-request-nonce");
        let signature = header_value(headers, "x-lark-signature");
        let (Some(timestamp), Some(nonce), Some(signature)) = (timestamp, nonce, signature) else {
            return false;
        };

        let body_str = String::from_utf8_lossy(body);
        let mut hasher = Sha256::new();
        hasher.update(timestamp.as_bytes());
        hasher.update(nonce.as_bytes());
        hasher.update(encrypt_key.as_bytes());
        hasher.update(body_str.as_bytes());
        let computed = format!("{:x}", hasher.finalize());
        computed == signature
    }

    async fn tenant_access_token(&self) -> anyhow::Result<String> {
        {
            let guard = self.tenant_token.read().await;
            if let Some(token) = guard.as_ref() {
                if token.expires_at > Instant::now() + TOKEN_REFRESH_SKEW {
                    return Ok(token.value.clone());
                }
            }
        }

        let response = self
            .http
            .post(format!(
                "{}/open-apis/auth/v3/tenant_access_token/internal",
                self.base_url
            ))
            .json(&json!({
                "app_id": self.app_id,
                "app_secret": self.app_secret,
            }))
            .timeout(Duration::from_secs(15))
            .send()
            .await?;

        let (status, payload) = read_feishu_json(response, "token request").await?;
        if !status.is_success() {
            anyhow::bail!("Feishu token request failed with HTTP {}", status);
        }
        let code = payload.get("code").and_then(Value::as_i64).unwrap_or(0);
        if code != 0 {
            let msg = payload
                .get("msg")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            anyhow::bail!("Feishu token request failed: code {code}, {msg}");
        }

        let token = payload
            .get("tenant_access_token")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("Feishu token response missing tenant_access_token"))?
            .to_string();
        let expires_in = payload
            .get("expire")
            .and_then(Value::as_u64)
            .unwrap_or(7200);
        let cached = CachedToken {
            value: token.clone(),
            expires_at: Instant::now() + Duration::from_secs(expires_in),
        };
        *self.tenant_token.write().await = Some(cached);
        Ok(token)
    }

    async fn send_text(
        &self,
        receive_id: &str,
        text: &str,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<Option<String>> {
        self.send_message_payload(
            receive_id,
            metadata.message_id.as_deref(),
            metadata.thread_id.as_deref(),
            "text",
            json!({ "text": text }).to_string(),
        )
        .await
    }

    async fn send_message_payload(
        &self,
        receive_id: &str,
        reply_to: Option<&str>,
        thread_id: Option<&str>,
        msg_type: &str,
        content: String,
    ) -> anyhow::Result<Option<String>> {
        let token = self.tenant_access_token().await?;
        let active_reply_to = match reply_to {
            Some(reply_to) if !reply_to.trim().is_empty() => Some(reply_to.trim()),
            _ => None,
        };
        if let Some(reply_to) = active_reply_to {
            let response = self
                .http
                .post(format!(
                    "{}/open-apis/im/v1/messages/{reply_to}/reply",
                    self.base_url
                ))
                .bearer_auth(&token)
                .json(&json!({
                    "content": content,
                    "msg_type": msg_type,
                    "reply_in_thread": thread_id.is_some(),
                    "uuid": Uuid::new_v4().to_string(),
                }))
                .timeout(Duration::from_secs(20))
                .send()
                .await?;

            let (status, payload) = read_feishu_json(response, "reply").await?;
            match validate_feishu_response(status, &payload, "reply") {
                Ok(()) => return Ok(extract_message_id(&payload)),
                Err(err) => {
                    let code = payload.get("code").and_then(Value::as_i64);
                    if !code.is_some_and(|value| FEISHU_REPLY_FALLBACK_CODES.contains(&value)) {
                        return Err(err);
                    }
                }
            }
        }

        let receive_id_type = infer_feishu_receive_id_type(receive_id);
        let response = self
            .http
            .post(format!(
                "{}/open-apis/im/v1/messages?receive_id_type={receive_id_type}",
                self.base_url,
            ))
            .bearer_auth(token)
            .json(&json!({
                "receive_id": receive_id,
                "msg_type": msg_type,
                "content": content,
                "uuid": Uuid::new_v4().to_string(),
            }))
            .timeout(Duration::from_secs(20))
            .send()
            .await?;

        let (status, payload) = read_feishu_json(response, "send").await?;
        validate_feishu_response(status, &payload, "send")?;
        Ok(extract_message_id(&payload))
    }

    async fn send_formatted_chunks(
        &self,
        receive_id: &str,
        chunks: Vec<String>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<Option<String>> {
        let mut last_message_id = None;
        for chunk in chunks {
            last_message_id = self.send_text(receive_id, &chunk, metadata).await?;
        }
        Ok(last_message_id)
    }

    async fn upload_image(&self, path: &str) -> anyhow::Result<String> {
        let file_name = Path::new(path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("image");
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|err| anyhow::anyhow!("Feishu send_photo: cannot read {path}: {err}"))?;
        let token = self.tenant_access_token().await?;
        let form = Form::new()
            .text("image_type", FEISHU_IMAGE_UPLOAD_TYPE)
            .part("image", Part::bytes(bytes).file_name(file_name.to_string()));
        let response = self
            .http
            .post(format!("{}/open-apis/im/v1/images", self.base_url))
            .bearer_auth(token)
            .multipart(form)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;
        let (status, payload) = read_feishu_json(response, "image upload").await?;
        validate_feishu_response(status, &payload, "image upload")?;
        payload
            .get("data")
            .and_then(|value| value.get("image_key"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("Feishu image upload missing image_key"))
    }

    async fn upload_file(&self, path: &str) -> anyhow::Result<(String, String)> {
        let file_name = Path::new(path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("attachment")
            .to_string();
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|err| anyhow::anyhow!("Feishu send_document: cannot read {path}: {err}"))?;
        let token = self.tenant_access_token().await?;
        let form = Form::new()
            .text("file_type", infer_feishu_file_upload_type(&file_name))
            .text("file_name", file_name.clone())
            .part("file", Part::bytes(bytes).file_name(file_name.clone()));
        let response = self
            .http
            .post(format!("{}/open-apis/im/v1/files", self.base_url))
            .bearer_auth(token)
            .multipart(form)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;
        let (status, payload) = read_feishu_json(response, "file upload").await?;
        validate_feishu_response(status, &payload, "file upload")?;
        let file_key = payload
            .get("data")
            .and_then(|value| value.get("file_key"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("Feishu file upload missing file_key"))?;
        Ok((file_key, file_name))
    }

    async fn send_image(
        &self,
        receive_id: &str,
        image_key: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        let trimmed_caption = caption.map(str::trim).filter(|value| !value.is_empty());
        let _ = match trimmed_caption {
            Some(caption) => {
                self.send_message_payload(
                    receive_id,
                    metadata.message_id.as_deref(),
                    metadata.thread_id.as_deref(),
                    "post",
                    build_media_post_payload(
                        caption,
                        json!({
                            "tag": "img",
                            "image_key": image_key,
                        }),
                    ),
                )
                .await?
            }
            None => {
                self.send_message_payload(
                    receive_id,
                    metadata.message_id.as_deref(),
                    metadata.thread_id.as_deref(),
                    "image",
                    json!({ "image_key": image_key }).to_string(),
                )
                .await?
            }
        };
        Ok(())
    }

    async fn send_file(
        &self,
        receive_id: &str,
        file_key: &str,
        file_name: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        let trimmed_caption = caption.map(str::trim).filter(|value| !value.is_empty());
        let _ = match trimmed_caption {
            Some(caption) => {
                self.send_message_payload(
                    receive_id,
                    metadata.message_id.as_deref(),
                    metadata.thread_id.as_deref(),
                    "post",
                    build_media_post_payload(
                        caption,
                        json!({
                            "tag": "media",
                            "file_key": file_key,
                            "file_name": file_name,
                        }),
                    ),
                )
                .await?
            }
            None => {
                self.send_message_payload(
                    receive_id,
                    metadata.message_id.as_deref(),
                    metadata.thread_id.as_deref(),
                    "file",
                    json!({ "file_key": file_key }).to_string(),
                )
                .await?
            }
        };
        Ok(())
    }

    async fn update_text_message(&self, message_id: &str, text: &str) -> anyhow::Result<()> {
        let token = self.tenant_access_token().await?;
        let response = self
            .http
            .put(format!(
                "{}/open-apis/im/v1/messages/{message_id}",
                self.base_url
            ))
            .bearer_auth(token)
            .json(&json!({
                "msg_type": "text",
                "content": json!({ "text": text }).to_string(),
            }))
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        let (status, payload) = read_feishu_json(response, "update").await?;
        validate_feishu_response(status, &payload, "update")
    }

    async fn get_message(&self, message_id: &str) -> anyhow::Result<Value> {
        let token = self.tenant_access_token().await?;
        let response = self
            .http
            .get(format!(
                "{}/open-apis/im/v1/messages/{message_id}",
                self.base_url
            ))
            .bearer_auth(token)
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        let (status, payload) = read_feishu_json(response, "get message").await?;
        validate_feishu_response(status, &payload, "get message")?;
        payload
            .get("data")
            .and_then(|value| value.get("items"))
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Feishu get message returned no items"))
    }

    async fn download_message_resource(
        &self,
        message_id: &str,
        file_key: &str,
        resource_type: &str,
        attachment: &MessageAttachment,
    ) -> anyhow::Result<Option<MessageAttachment>> {
        let mut request_types = vec![resource_type];
        if matches!(resource_type, "audio" | "media") {
            request_types.push("file");
        }

        for request_type in request_types {
            let token = self.tenant_access_token().await?;
            let response = self
                .http
                .get(format!(
                    "{}/open-apis/im/v1/messages/{message_id}/resources/{file_key}",
                    self.base_url
                ))
                .bearer_auth(&token)
                .query(&[("type", request_type)])
                .timeout(Duration::from_secs(30))
                .send()
                .await?;
            let status = response.status();
            let headers = response.headers().clone();

            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                if body.trim_start().starts_with('{') {
                    continue;
                }
                anyhow::bail!(
                    "Feishu resource download failed with HTTP {} for type {}",
                    status,
                    request_type
                );
            }

            let bytes = response.bytes().await?;
            if bytes.is_empty() {
                continue;
            }

            let content_type =
                reqwest_header_value(&headers, "content-type").map(normalize_content_type);
            let file_name = parse_content_disposition_file_name(&headers)
                .or_else(|| attachment.file_name.clone())
                .filter(|value| !value.trim().is_empty());
            let fallback_name =
                fallback_feishu_attachment_name(attachment.kind.clone(), file_key, request_type);
            let local_path = crate::attachment_cache::persist_bytes(
                "feishu",
                file_name.as_deref(),
                &fallback_name,
                bytes.as_ref(),
            )?;

            let mut enriched = attachment.clone();
            enriched.file_name = file_name.or(enriched.file_name.clone());
            enriched.mime_type = content_type;
            enriched.local_path = local_path;
            enriched.size_bytes = Some(bytes.len() as u64);
            return Ok(Some(enriched));
        }

        Ok(None)
    }

    async fn enrich_incoming_resources(
        &self,
        message_id: &str,
        normalized: &mut FeishuNormalizedMessage,
    ) {
        for resource in normalized.resources.clone() {
            let Some(attachment) = normalized
                .attachments
                .get(resource.attachment_index)
                .cloned()
            else {
                continue;
            };
            match self
                .download_message_resource(
                    message_id,
                    &resource.file_key,
                    resource.resource_type,
                    &attachment,
                )
                .await
            {
                Ok(Some(enriched)) => normalized.attachments[resource.attachment_index] = enriched,
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(
                        %error,
                        message_id,
                        file_key = %resource.file_key,
                        resource_type = resource.resource_type,
                        "Feishu attachment download failed"
                    );
                }
            }
        }
    }
}

#[async_trait]
impl PlatformAdapter for FeishuAdapter {
    fn platform(&self) -> Platform {
        Platform::Feishu
    }

    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
        self.ensure_bot_identity().await;
        let router = self.router(tx);
        let addr = format!("{}:{}", self.webhook_host, self.webhook_port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        tracing::info!(addr = %addr, path = %self.webhook_path, "Feishu webhook listening");
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await?;
        Ok(())
    }

    async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()> {
        let receive_id = msg
            .metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Feishu delivery requires channel_id"))?;
        let formatted = self.format_response(&msg.text, &msg.metadata);
        let chunks = split_message(&formatted, MAX_MESSAGE_LENGTH);
        let _ = self
            .send_formatted_chunks(receive_id, chunks, &msg.metadata)
            .await?;
        Ok(())
    }

    async fn send_and_get_id(&self, msg: OutgoingMessage) -> anyhow::Result<Option<String>> {
        let receive_id = msg
            .metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Feishu delivery requires channel_id"))?;
        let formatted = self.format_response(&msg.text, &msg.metadata);
        let chunks = split_message(&formatted, MAX_MESSAGE_LENGTH);
        self.send_formatted_chunks(receive_id, chunks, &msg.metadata)
            .await
    }

    fn format_response(&self, text: &str, _metadata: &MessageMetadata) -> String {
        text.to_string()
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
        true
    }

    async fn edit_message(
        &self,
        message_id: &str,
        metadata: &MessageMetadata,
        new_text: &str,
    ) -> anyhow::Result<String> {
        let formatted = self.format_response(new_text, metadata);
        let truncated = truncate_for_edit(&formatted, MAX_MESSAGE_LENGTH);
        self.update_text_message(message_id, &truncated).await?;
        Ok(message_id.to_string())
    }

    async fn send_photo(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        let receive_id = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Feishu send_photo requires channel_id"))?;
        let image_key = self.upload_image(path).await?;
        self.send_image(receive_id, &image_key, caption, metadata)
            .await
    }

    async fn send_document(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        let receive_id = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Feishu send_document requires channel_id"))?;
        let (file_key, file_name) = self.upload_file(path).await?;
        self.send_file(receive_id, &file_key, &file_name, caption, metadata)
            .await
    }
}

async fn handle_webhook(
    State(state): State<FeishuWebhookState>,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let remote_ip = webhook_remote_ip(connect_info, &headers);

    if !state.adapter.check_webhook_rate_limit(&remote_ip).await {
        state
            .adapter
            .record_webhook_anomaly(&remote_ip, "429")
            .await;
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "code": 1, "msg": "rate limit exceeded" })),
        );
    }

    if let Some(content_type) = header_value(&headers, "content-type") {
        let normalized = content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if !normalized.is_empty() && normalized != "application/json" {
            state
                .adapter
                .record_webhook_anomaly(&remote_ip, "415")
                .await;
            return (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                Json(json!({ "code": 1, "msg": "unsupported content type" })),
            );
        }
    }

    if body.len() > FEISHU_WEBHOOK_MAX_BODY_BYTES {
        state
            .adapter
            .record_webhook_anomaly(&remote_ip, "413")
            .await;
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({ "code": 1, "msg": "request body too large" })),
        );
    }

    let payload = match serde_json::from_slice::<Value>(&body) {
        Ok(payload) => payload,
        Err(error) => {
            state
                .adapter
                .record_webhook_anomaly(&remote_ip, "400")
                .await;
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "code": 1, "msg": format!("invalid json: {error}") })),
            );
        }
    };

    if let Some(challenge) = payload.get("challenge").and_then(Value::as_str) {
        return (StatusCode::OK, Json(json!({ "challenge": challenge })));
    }

    if !token_is_valid(&state.adapter.verification_token, &payload) {
        state
            .adapter
            .record_webhook_anomaly(&remote_ip, "401-token")
            .await;
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "code": 1, "msg": "invalid verification token" })),
        );
    }

    if !state.adapter.is_webhook_signature_valid(&headers, &body) {
        state
            .adapter
            .record_webhook_anomaly(&remote_ip, "401-sig")
            .await;
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "code": 1, "msg": "invalid signature" })),
        );
    }

    if payload.get("encrypt").is_some() {
        state
            .adapter
            .record_webhook_anomaly(&remote_ip, "400-encrypted")
            .await;
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "code": 1, "msg": "encrypted webhook payloads are not supported" })),
        );
    }

    let event_id = payload
        .get("header")
        .and_then(|value| value.get("event_id"))
        .and_then(Value::as_str)
        .or_else(|| payload.get("event_id").and_then(Value::as_str))
        .or_else(|| {
            payload
                .get("event")
                .and_then(|value| value.get("message"))
                .and_then(|value| value.get("message_id"))
                .and_then(Value::as_str)
        })
        .map(str::to_string);

    if let Some(id) = event_id {
        if state.adapter.is_duplicate_event(&id).await {
            return (StatusCode::OK, Json(json!({ "code": 0 })));
        }
    }

    match parse_webhook_event(&state.adapter, &payload).await {
        Ok(Some(message)) => {
            if state.tx.send(message).await.is_err() {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "code": 1, "msg": "receiver dropped" })),
                );
            }
        }
        Ok(None) => {}
        Err(err) => {
            state
                .adapter
                .record_webhook_anomaly(&remote_ip, "400")
                .await;
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "code": 1, "msg": err.to_string() })),
            );
        }
    }
    state.adapter.clear_webhook_anomaly(&remote_ip).await;

    let event_type = payload
        .get("header")
        .and_then(|value| value.get("event_type"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    if event_type == "card.action.trigger" {
        (StatusCode::OK, Json(json!({})))
    } else {
        (StatusCode::OK, Json(json!({ "code": 0 })))
    }
}

fn token_is_valid(verification_token: &Option<String>, payload: &Value) -> bool {
    match verification_token {
        Some(expected) => payload
            .get("header")
            .and_then(|value| value.get("token"))
            .and_then(Value::as_str)
            .or_else(|| payload.get("token").and_then(Value::as_str))
            .is_some_and(|actual| actual == expected),
        None => true,
    }
}

fn header_value(headers: &HeaderMap, key: &str) -> Option<String> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn webhook_remote_ip(connect_info: Option<ConnectInfo<SocketAddr>>, headers: &HeaderMap) -> String {
    if let Some(forwarded) = header_value(headers, "x-forwarded-for")
        .and_then(|value| value.split(',').next().map(str::trim).map(str::to_string))
        .filter(|value| !value.is_empty())
    {
        return forwarded;
    }
    connect_info
        .map(|ConnectInfo(addr)| addr.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

async fn parse_webhook_event(
    adapter: &FeishuAdapter,
    payload: &Value,
) -> anyhow::Result<Option<IncomingMessage>> {
    let event_type = payload
        .get("header")
        .and_then(|value| value.get("event_type"))
        .and_then(Value::as_str)
        .unwrap_or("im.message.receive_v1");

    match event_type {
        "im.message.receive_v1" => parse_event(adapter, payload).await,
        "im.message.reaction.created_v1" => parse_reaction_event(adapter, payload, true).await,
        "im.message.reaction.deleted_v1" => parse_reaction_event(adapter, payload, false).await,
        "card.action.trigger" => parse_card_action_event(adapter, payload).await,
        _ => Ok(None),
    }
}

async fn parse_event(
    adapter: &FeishuAdapter,
    payload: &Value,
) -> anyhow::Result<Option<IncomingMessage>> {
    let event = match payload.get("event") {
        Some(value) => value,
        None => return Ok(None),
    };
    let message = event
        .get("message")
        .ok_or_else(|| anyhow::anyhow!("Feishu event missing message payload"))?;

    let user_id = event
        .get("sender")
        .and_then(|value| value.get("sender_id"))
        .and_then(|value| {
            value
                .get("open_id")
                .or_else(|| value.get("user_id"))
                .or_else(|| value.get("union_id"))
        })
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();

    if user_id.is_empty() {
        return Ok(None);
    }
    if !adapter.allowed_users.is_empty() && !adapter.allowed_users.contains(&user_id) {
        return Ok(None);
    }

    let chat_id = message
        .get("chat_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if chat_id.is_empty() {
        return Ok(None);
    }

    let message_id = message
        .get("message_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let message_type = message
        .get("message_type")
        .and_then(Value::as_str)
        .unwrap_or("text");
    let raw_content = message.get("content").and_then(Value::as_str).unwrap_or("");
    let mut normalized = normalize_message_content(message_type, raw_content);
    if !should_accept_feishu_message(adapter, event, message, raw_content, &normalized, &user_id) {
        return Ok(None);
    }

    if let Some(message_id) = message_id.as_deref() {
        if !normalized.resources.is_empty() {
            adapter
                .enrich_incoming_resources(message_id, &mut normalized)
                .await;
        }
    }

    let thread_id = message
        .get("thread_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let text = render_feishu_incoming_text(&normalized.text, &normalized.attachments);
    if text.trim().is_empty() {
        return Ok(None);
    }

    let feishu_chat_type = message
        .get("chat_type")
        .and_then(Value::as_str)
        .unwrap_or("p2p");
    let chat_type = if feishu_chat_type.eq_ignore_ascii_case("p2p") {
        crate::platform::ChatType::Dm
    } else {
        crate::platform::ChatType::Group
    };

    Ok(Some(IncomingMessage {
        platform: Platform::Feishu,
        user_id,
        channel_id: Some(chat_id.clone()),
        chat_type,
        text,
        thread_id: thread_id.clone(),
        metadata: MessageMetadata {
            message_id,
            channel_id: Some(chat_id),
            thread_id,
            user_display_name: event
                .get("sender")
                .and_then(|value| value.get("sender_id"))
                .and_then(|value| value.get("open_id"))
                .and_then(Value::as_str)
                .map(str::to_string),
            attachments: normalized.attachments,
            ..Default::default()
        },
    }))
}

async fn parse_reaction_event(
    adapter: &FeishuAdapter,
    payload: &Value,
    created: bool,
) -> anyhow::Result<Option<IncomingMessage>> {
    let event = match payload.get("event") {
        Some(value) => value,
        None => return Ok(None),
    };

    let operator_type = event
        .get("operator_type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(operator_type.as_str(), "bot" | "app") {
        return Ok(None);
    }

    let target_message_id = event
        .get("message_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);
    let Some(target_message_id) = target_message_id else {
        return Ok(None);
    };

    let target = adapter.get_message(&target_message_id).await?;
    let sender_type = target
        .get("sender")
        .and_then(|value| value.get("sender_type"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if sender_type != "app" {
        return Ok(None);
    }

    let user_id = event
        .get("user_id")
        .and_then(extract_event_user_id)
        .unwrap_or_default();
    if user_id.is_empty() {
        return Ok(None);
    }

    let chat_id = target
        .get("chat_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if chat_id.is_empty() {
        return Ok(None);
    }

    let emoji = event
        .get("reaction_type")
        .and_then(|value| value.get("emoji_type"))
        .and_then(Value::as_str)
        .unwrap_or("UNKNOWN");
    let action = if created { "added" } else { "removed" };
    let thread_id = target
        .get("thread_id")
        .and_then(Value::as_str)
        .map(str::to_string);

    Ok(Some(IncomingMessage {
        platform: Platform::Feishu,
        user_id,
        channel_id: Some(chat_id.clone()),
        chat_type: crate::platform::ChatType::Dm, // reactions don't carry chat_type
        text: format!("reaction:{action}:{emoji}"),
        thread_id: thread_id.clone(),
        metadata: MessageMetadata {
            message_id: Some(target_message_id),
            channel_id: Some(chat_id),
            thread_id,
            ..Default::default()
        },
    }))
}

async fn parse_card_action_event(
    adapter: &FeishuAdapter,
    payload: &Value,
) -> anyhow::Result<Option<IncomingMessage>> {
    let event = match payload.get("event") {
        Some(value) => value,
        None => return Ok(None),
    };
    let token = event
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if adapter.is_duplicate_card_action(&token).await {
        return Ok(None);
    }
    let chat_id = event
        .get("context")
        .and_then(|value| value.get("open_chat_id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let user_id = event
        .get("operator")
        .and_then(extract_event_user_id)
        .unwrap_or_default();
    if chat_id.is_empty() || user_id.is_empty() {
        return Ok(None);
    }

    let action_tag = event
        .get("action")
        .and_then(|value| value.get("tag"))
        .and_then(Value::as_str)
        .unwrap_or("button");
    let value = event
        .get("action")
        .and_then(|value| value.get("value"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut text = format!("/card {action_tag}");
    if value.as_object().is_some_and(|value| !value.is_empty()) {
        text.push(' ');
        text.push_str(&value.to_string());
    }

    Ok(Some(IncomingMessage {
        platform: Platform::Feishu,
        user_id,
        channel_id: Some(chat_id.clone()),
        chat_type: crate::platform::ChatType::Dm, // card actions don't carry chat_type
        text,
        thread_id: None,
        metadata: MessageMetadata {
            message_id: (!token.is_empty()).then_some(token),
            channel_id: Some(chat_id),
            ..Default::default()
        },
    }))
}

fn normalize_message_content(message_type: &str, raw_content: &str) -> FeishuNormalizedMessage {
    match message_type {
        "text" => FeishuNormalizedMessage {
            text: load_feishu_payload(raw_content)
                .get("text")
                .and_then(Value::as_str)
                .map(normalize_feishu_text)
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| normalize_feishu_text(raw_content)),
            ..Default::default()
        },
        "post" => parse_post_content(raw_content),
        "image" => normalize_image_message(raw_content),
        "file" => normalize_file_like_message(raw_content, MessageAttachmentKind::Document),
        "audio" => normalize_file_like_message(raw_content, MessageAttachmentKind::Audio),
        "media" => normalize_file_like_message(raw_content, MessageAttachmentKind::Video),
        "share_chat" => normalize_share_chat_message(raw_content),
        "merge_forward" => normalize_merge_forward_message(raw_content),
        "interactive" | "card" => normalize_interactive_message(raw_content),
        "sticker" => FeishuNormalizedMessage {
            text: "[Sticker]".to_string(),
            ..Default::default()
        },
        _ => FeishuNormalizedMessage {
            text: normalize_feishu_text(raw_content),
            ..Default::default()
        },
    }
}

fn parse_post_content(raw_content: &str) -> FeishuNormalizedMessage {
    parse_post_payload(&load_feishu_payload(raw_content))
}

fn parse_post_payload(payload: &Value) -> FeishuNormalizedMessage {
    let locale_payload = resolve_post_payload(payload).unwrap_or(payload);

    let mut lines = Vec::new();
    let mut attachments = Vec::new();
    let mut mentioned_ids = Vec::new();
    let mut resources = Vec::new();
    if let Some(title) = locale_payload.get("title").and_then(Value::as_str) {
        let title = normalize_feishu_text(title);
        if !title.is_empty() {
            lines.push(title);
        }
    }

    if let Some(rows) = locale_payload.get("content").and_then(Value::as_array) {
        for row in rows {
            let mut fragments = Vec::new();
            if let Some(items) = row.as_array() {
                for item in items {
                    let rendered = render_post_element(
                        item,
                        &mut attachments,
                        &mut mentioned_ids,
                        &mut resources,
                    );
                    if !rendered.is_empty() {
                        fragments.push(rendered);
                    }
                }
            }
            if !fragments.is_empty() {
                lines.push(normalize_feishu_text(&fragments.join(" ")));
            }
        }
    }

    let text = lines
        .into_iter()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    FeishuNormalizedMessage {
        text: if text.is_empty() {
            FALLBACK_POST_TEXT.to_string()
        } else {
            text
        },
        attachments,
        mentioned_ids,
        resources,
    }
}

fn normalize_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        DEFAULT_WEBHOOK_PATH.to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

fn infer_feishu_receive_id_type(receive_id: &str) -> &'static str {
    let trimmed = receive_id.trim();
    if trimmed.starts_with("oc_") {
        "open_chat_id"
    } else if trimmed.starts_with("ou_") || trimmed.starts_with("open_") {
        "open_id"
    } else if trimmed.starts_with("on_") {
        "union_id"
    } else {
        "chat_id"
    }
}

fn should_accept_feishu_message(
    adapter: &FeishuAdapter,
    event: &Value,
    message: &Value,
    raw_content: &str,
    normalized: &FeishuNormalizedMessage,
    user_id: &str,
) -> bool {
    let chat_type = message
        .get("chat_type")
        .and_then(Value::as_str)
        .unwrap_or("p2p");
    if chat_type.eq_ignore_ascii_case("p2p") {
        return true;
    }

    let group_policy = env::var("FEISHU_GROUP_POLICY")
        .unwrap_or_else(|_| "mentioned".to_string())
        .to_ascii_lowercase();
    if group_policy == "disabled" {
        return false;
    }

    let allowed_group_users = parse_csv_set("FEISHU_ALLOWED_GROUP_USERS");
    if !allowed_group_users.is_empty() && !allowed_group_users.contains(user_id) {
        return false;
    }

    if group_policy == "open" {
        return true;
    }

    if raw_content.contains("@_all") {
        return true;
    }

    if mentions_target_bot(adapter, message.get("mentions")) {
        return true;
    }

    if normalized
        .mentioned_ids
        .iter()
        .any(|mentioned_id| is_bot_identity(adapter, mentioned_id))
    {
        return true;
    }

    let mention_count = message
        .get("mentions")
        .and_then(Value::as_array)
        .map(|mentions| mentions.len())
        .unwrap_or_default();
    let identity_configured = env::var("FEISHU_BOT_OPEN_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .is_some()
        || env::var("FEISHU_BOT_USER_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .is_some()
        || env::var("FEISHU_BOT_NAME")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .is_some()
        || adapter
            .bot_name
            .read()
            .ok()
            .and_then(|value| value.clone())
            .is_some();

    if !identity_configured {
        return mention_count > 0 || !normalized.mentioned_ids.is_empty();
    }

    let _ = event;
    false
}

fn mentions_target_bot(adapter: &FeishuAdapter, mentions: Option<&Value>) -> bool {
    mentions
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|mention| {
            let name = mention
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let Some(id) = mention.get("id") else {
                return !name.trim().is_empty() && is_bot_identity(adapter, name);
            };
            if let Some(id_str) = id.as_str() {
                return is_bot_identity(adapter, id_str);
            }
            if let Some(mentioned_id) = extract_event_user_id(id) {
                return is_bot_identity(adapter, &mentioned_id);
            }
            !name.trim().is_empty() && is_bot_identity(adapter, name)
        })
}

fn is_bot_identity(adapter: &FeishuAdapter, candidate: &str) -> bool {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return false;
    }

    let configured = [
        env::var("FEISHU_BOT_OPEN_ID").ok(),
        env::var("FEISHU_BOT_USER_ID").ok(),
        env::var("FEISHU_BOT_NAME").ok(),
        adapter.bot_name.read().ok().and_then(|value| value.clone()),
        Some(adapter.app_id.clone()),
    ];

    configured
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .any(|value| value == trimmed)
}

fn load_feishu_payload(raw_content: &str) -> Value {
    serde_json::from_str::<Value>(raw_content).unwrap_or_else(|_| json!({ "text": raw_content }))
}

fn resolve_post_payload(payload: &Value) -> Option<&Value> {
    if payload.get("content").is_some() {
        return Some(payload);
    }
    payload
        .get("post")
        .and_then(resolve_post_payload)
        .or_else(|| payload.get("zh_cn").and_then(resolve_post_payload))
        .or_else(|| payload.get("en_us").and_then(resolve_post_payload))
        .or_else(|| {
            payload
                .as_object()
                .and_then(|values| values.values().find(|value| value.get("content").is_some()))
        })
}

fn normalize_image_message(raw_content: &str) -> FeishuNormalizedMessage {
    let payload = load_feishu_payload(raw_content);
    let text = payload
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| payload.get("alt").and_then(Value::as_str))
        .map(normalize_feishu_text)
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    let image_key = payload
        .get("image_key")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let file_name = image_key.map(|value| format!("image-{value}.jpg"));
    let mut resources = Vec::new();
    if let Some(image_key) = image_key {
        resources.push(FeishuResourceRef {
            attachment_index: 0,
            file_key: image_key.to_string(),
            resource_type: "image",
        });
    }
    FeishuNormalizedMessage {
        text,
        attachments: vec![MessageAttachment {
            kind: MessageAttachmentKind::Image,
            file_name,
            ..Default::default()
        }],
        resources,
        ..Default::default()
    }
}

fn normalize_file_like_message(
    raw_content: &str,
    kind: MessageAttachmentKind,
) -> FeishuNormalizedMessage {
    let payload = load_feishu_payload(raw_content);
    let file_name = first_non_empty_text(&[
        payload.get("file_name"),
        payload.get("title"),
        payload.get("text"),
    ]);
    let text = first_non_empty_text(&[
        payload.get("text"),
        payload.get("title"),
        payload.get("summary"),
        payload.get("preview"),
    ]);
    let fallback_name = file_name
        .clone()
        .unwrap_or_else(|| FALLBACK_ATTACHMENT_TEXT.to_string());
    let file_key = payload
        .get("file_key")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let mut resources = Vec::new();
    if let Some(file_key) = file_key {
        resources.push(FeishuResourceRef {
            attachment_index: 0,
            file_key: file_key.to_string(),
            resource_type: match kind {
                MessageAttachmentKind::Audio => "audio",
                MessageAttachmentKind::Video => "media",
                _ => "file",
            },
        });
    }

    FeishuNormalizedMessage {
        text: text.unwrap_or_default(),
        attachments: vec![MessageAttachment {
            kind,
            file_name: Some(fallback_name),
            ..Default::default()
        }],
        resources,
        ..Default::default()
    }
}

fn normalize_share_chat_message(raw_content: &str) -> FeishuNormalizedMessage {
    let payload = load_feishu_payload(raw_content);
    let chat_name = first_non_empty_text(&[
        payload.get("chat_name"),
        payload.get("name"),
        payload.get("title"),
    ]);
    let share_id = first_non_empty_text(&[
        payload.get("chat_id"),
        payload.get("open_chat_id"),
        payload.get("share_chat_id"),
    ]);
    let mut lines = Vec::new();
    lines.push(chat_name.unwrap_or_else(|| FALLBACK_SHARE_CHAT_TEXT.to_string()));
    if let Some(share_id) = share_id {
        lines.push(format!("Chat ID: {share_id}"));
    }
    FeishuNormalizedMessage {
        text: lines.join("\n"),
        ..Default::default()
    }
}

fn normalize_merge_forward_message(raw_content: &str) -> FeishuNormalizedMessage {
    let payload = load_feishu_payload(raw_content);
    let title = first_non_empty_text(&[
        payload.get("title"),
        payload.get("summary"),
        payload.get("preview"),
    ]);
    let mut lines = Vec::new();
    if let Some(title) = title {
        lines.push(title);
    }

    for key in ["messages", "items", "message_list", "records", "content"] {
        if let Some(items) = payload.get(key).and_then(Value::as_array) {
            for item in items.iter().take(8) {
                let sender = first_non_empty_text(&[
                    item.get("sender_name"),
                    item.get("user_name"),
                    item.get("sender"),
                    item.get("name"),
                ]);
                let nested_type = item
                    .get("message_type")
                    .or_else(|| item.get("msg_type"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let body = if nested_type.eq_ignore_ascii_case("post") {
                    parse_post_payload(item).text
                } else {
                    first_non_empty_text(&[
                        item.get("text"),
                        item.get("summary"),
                        item.get("preview"),
                        item.get("content"),
                    ])
                    .unwrap_or_default()
                };
                if body.is_empty() {
                    continue;
                }
                match sender {
                    Some(sender) => lines.push(format!("- {sender}: {body}")),
                    None => lines.push(format!("- {body}")),
                }
            }
            break;
        }
    }

    FeishuNormalizedMessage {
        text: if lines.is_empty() {
            FALLBACK_FORWARD_TEXT.to_string()
        } else {
            lines.join("\n")
        },
        ..Default::default()
    }
}

fn normalize_interactive_message(raw_content: &str) -> FeishuNormalizedMessage {
    let payload = load_feishu_payload(raw_content);
    let card_payload = payload.get("card").unwrap_or(&payload);
    let mut lines = collect_text_segments(card_payload);
    if lines.is_empty() {
        lines.push(FALLBACK_INTERACTIVE_TEXT.to_string());
    }
    FeishuNormalizedMessage {
        text: lines.join("\n"),
        ..Default::default()
    }
}

fn render_post_element(
    element: &Value,
    attachments: &mut Vec<MessageAttachment>,
    mentioned_ids: &mut Vec<String>,
    resources: &mut Vec<FeishuResourceRef>,
) -> String {
    if let Some(text) = element.as_str() {
        return normalize_feishu_text(text);
    }

    let Some(object) = element.as_object() else {
        return String::new();
    };
    let tag = object
        .get("tag")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    match tag.as_str() {
        "text" => object
            .get("text")
            .and_then(Value::as_str)
            .map(normalize_feishu_text)
            .unwrap_or_default(),
        "a" => first_non_empty_text(&[object.get("text"), object.get("href")]).unwrap_or_default(),
        "at" => {
            let mentioned_id =
                first_non_empty_text(&[object.get("open_id"), object.get("user_id")]);
            if let Some(mentioned_id) = mentioned_id {
                if !mentioned_ids.contains(&mentioned_id) {
                    mentioned_ids.push(mentioned_id);
                }
            }
            let display_name = first_non_empty_text(&[
                object.get("user_name"),
                object.get("name"),
                object.get("text"),
            ])
            .unwrap_or_else(|| "user".to_string());
            format!("@{display_name}")
        }
        "img" | "image" => {
            let image_key = object
                .get("image_key")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty());
            let attachment_index = attachments.len();
            attachments.push(MessageAttachment {
                kind: MessageAttachmentKind::Image,
                file_name: image_key.map(|value| format!("image-{value}.jpg")),
                ..Default::default()
            });
            if let Some(image_key) = image_key {
                resources.push(FeishuResourceRef {
                    attachment_index,
                    file_key: image_key.to_string(),
                    resource_type: "image",
                });
            }
            first_non_empty_text(&[object.get("text"), object.get("alt")])
                .map(|text| format!("[Image: {text}]"))
                .unwrap_or_else(|| FALLBACK_IMAGE_TEXT.to_string())
        }
        "media" | "file" | "audio" | "video" => {
            let file_name = first_non_empty_text(&[
                object.get("file_name"),
                object.get("title"),
                object.get("text"),
            ])
            .unwrap_or_else(|| FALLBACK_ATTACHMENT_TEXT.to_string());
            let attachment_index = attachments.len();
            attachments.push(MessageAttachment {
                kind: match tag.as_str() {
                    "audio" => MessageAttachmentKind::Audio,
                    "video" | "media" => MessageAttachmentKind::Video,
                    _ => MessageAttachmentKind::Document,
                },
                file_name: Some(file_name.clone()),
                ..Default::default()
            });
            if let Some(file_key) = object
                .get("file_key")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
            {
                resources.push(FeishuResourceRef {
                    attachment_index,
                    file_key: file_key.to_string(),
                    resource_type: match tag.as_str() {
                        "audio" => "audio",
                        "video" | "media" => "media",
                        _ => "file",
                    },
                });
            }
            format!("[Attachment: {file_name}]")
        }
        "br" => "\n".to_string(),
        _ => object
            .values()
            .map(|value| render_post_element(value, attachments, mentioned_ids, resources))
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn extract_event_user_id(value: &Value) -> Option<String> {
    value
        .get("open_id")
        .or_else(|| value.get("user_id"))
        .or_else(|| value.get("union_id"))
        .or_else(|| value.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn reqwest_header_value(headers: &reqwest::header::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn normalize_content_type(content_type: String) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

fn parse_content_disposition_file_name(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let content_disposition = reqwest_header_value(headers, "content-disposition")?;
    for part in content_disposition.split(';') {
        let trimmed = part.trim();
        if let Some(value) = trimmed.strip_prefix("filename=") {
            return Some(value.trim_matches('"').to_string());
        }
        if let Some(value) = trimmed.strip_prefix("filename*=") {
            return value.split("''").nth(1).map(|value| value.to_string());
        }
    }
    None
}

fn fallback_feishu_attachment_name(
    kind: MessageAttachmentKind,
    resource_key: &str,
    request_type: &str,
) -> String {
    let extension = match kind {
        MessageAttachmentKind::Image => "jpg",
        MessageAttachmentKind::Video => "mp4",
        MessageAttachmentKind::Audio | MessageAttachmentKind::Voice => "ogg",
        MessageAttachmentKind::Document | MessageAttachmentKind::Other => match request_type {
            "audio" => "ogg",
            "media" => "mp4",
            _ => "bin",
        },
        MessageAttachmentKind::Sticker => "webp",
    };
    format!("feishu-{resource_key}.{extension}")
}

fn first_non_empty_text(values: &[Option<&Value>]) -> Option<String> {
    values.iter().flatten().find_map(|value| match value {
        Value::String(text) => {
            let normalized = normalize_feishu_text(text);
            if normalized.is_empty() {
                None
            } else {
                Some(normalized)
            }
        }
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(boolean) => Some(boolean.to_string()),
        _ => None,
    })
}

fn collect_text_segments(value: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    collect_text_segments_into(value, &mut lines);
    let mut unique = Vec::new();
    for line in lines {
        if !line.is_empty() && !unique.contains(&line) {
            unique.push(line);
        }
    }
    unique
}

fn collect_text_segments_into(value: &Value, lines: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            let normalized = normalize_feishu_text(text);
            if !normalized.is_empty() {
                lines.push(normalized);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_text_segments_into(item, lines);
            }
        }
        Value::Object(object) => {
            for key in ["text", "content", "title", "name", "value"] {
                if let Some(item) = object.get(key) {
                    collect_text_segments_into(item, lines);
                }
            }
            for (key, item) in object {
                if ["text", "content", "title", "name", "value"].contains(&key.as_str()) {
                    continue;
                }
                collect_text_segments_into(item, lines);
            }
        }
        _ => {}
    }
}

fn normalize_feishu_text(text: &str) -> String {
    static MENTION_PLACEHOLDER_RE: OnceLock<Regex> = OnceLock::new();
    static WHITESPACE_RE: OnceLock<Regex> = OnceLock::new();

    let mention_placeholder = MENTION_PLACEHOLDER_RE
        .get_or_init(|| Regex::new(r"@_user_\d+").expect("valid mention placeholder regex"));
    let whitespace =
        WHITESPACE_RE.get_or_init(|| Regex::new(r"[^\S\n]+").expect("valid whitespace regex"));

    let cleaned = mention_placeholder.replace_all(text, " ");
    let cleaned = cleaned.replace("\r\n", "\n").replace('\r', "\n");
    cleaned
        .split('\n')
        .filter_map(|line| {
            let normalized = whitespace.replace_all(line, " ");
            let trimmed = normalized.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_feishu_incoming_text(content: &str, attachments: &[MessageAttachment]) -> String {
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

fn truncate_for_edit(text: &str, max_len: usize) -> String {
    if text.chars().count() <= max_len {
        return text.to_string();
    }
    text.chars().take(max_len).collect()
}

fn infer_feishu_file_upload_type(file_name: &str) -> &'static str {
    match Path::new(file_name)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("pdf") => "pdf",
        Some("doc") | Some("docx") => "doc",
        Some("xls") | Some("xlsx") => "xls",
        Some("ppt") | Some("pptx") => "ppt",
        _ => FEISHU_FILE_UPLOAD_TYPE,
    }
}

fn build_media_post_payload(caption: &str, media_tag: Value) -> String {
    let mut rows = Vec::new();
    let trimmed = caption.trim();
    if !trimmed.is_empty() {
        rows.push(vec![json!({
            "tag": "text",
            "text": trimmed,
        })]);
    }
    rows.push(vec![media_tag]);
    json!({
        "zh_cn": {
            "content": rows,
        }
    })
    .to_string()
}

fn validate_feishu_response(
    status: StatusCode,
    payload: &Value,
    action: &str,
) -> anyhow::Result<()> {
    if !status.is_success() {
        anyhow::bail!("Feishu {action} failed with HTTP {}", status);
    }
    let code = payload.get("code").and_then(Value::as_i64).unwrap_or(0);
    if code != 0 {
        let msg = payload
            .get("msg")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        anyhow::bail!("Feishu {action} failed: code {code}, {msg}");
    }
    Ok(())
}

fn extract_message_id(payload: &Value) -> Option<String> {
    payload
        .get("data")
        .and_then(|value| value.get("message_id"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

async fn read_feishu_json(
    response: reqwest::Response,
    action: &str,
) -> anyhow::Result<(StatusCode, Value)> {
    let status = response.status();
    let body = response.text().await?;
    let payload = serde_json::from_str::<Value>(&body).map_err(|err| {
        anyhow::anyhow!(
            "Feishu {action} returned non-JSON body (HTTP {}): {} ({err})",
            status,
            body.trim()
        )
    })?;
    Ok((status, payload))
}

fn parse_csv_set(key: &str) -> HashSet<String> {
    env::var(key)
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn load_seen_event_ids(path: &Path) -> HashMap<String, Instant> {
    let Ok(payload) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(entries) = serde_json::from_str::<HashMap<String, u64>>(&payload) else {
        tracing::warn!(path = %path.display(), "Feishu dedup state unreadable; starting fresh");
        return HashMap::new();
    };

    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    entries
        .into_iter()
        .filter_map(|(event_id, seen_unix)| {
            let age = now_unix.saturating_sub(seen_unix);
            if age > DEDUP_TTL.as_secs() {
                return None;
            }
            Some((
                event_id,
                Instant::now()
                    .checked_sub(Duration::from_secs(age))
                    .unwrap_or_else(Instant::now),
            ))
        })
        .collect()
}

fn persist_seen_event_ids(path: Option<&Path>, cache: &HashMap<String, Instant>) {
    let Some(path) = path else {
        return;
    };

    let now = Instant::now();
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut recent: Vec<_> = cache
        .iter()
        .filter_map(|(event_id, seen_at)| {
            let age = now.duration_since(*seen_at);
            if age > DEDUP_TTL {
                return None;
            }
            Some((event_id.clone(), now_unix.saturating_sub(age.as_secs())))
        })
        .collect();
    recent.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    recent.truncate(FEISHU_DEDUP_CACHE_SIZE);

    let payload: HashMap<String, u64> = recent.into_iter().collect();
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            tracing::warn!(%error, path = %path.display(), "Feishu dedup state dir create failed");
            return;
        }
    }
    let body = match serde_json::to_vec(&payload) {
        Ok(body) => body,
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "Feishu dedup state serialize failed");
            return;
        }
    };
    if let Err(error) = std::fs::write(path, body) {
        tracing::warn!(%error, path = %path.display(), "Feishu dedup state persist failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::body::Bytes;
    use axum::extract::OriginalUri;
    use axum::extract::State as AxumState;
    use axum::http::Request;
    use axum::routing::put;
    use tower::util::ServiceExt;

    fn test_adapter(base_url: impl Into<String>) -> FeishuAdapter {
        FeishuAdapter {
            app_id: "app".into(),
            app_secret: "secret".into(),
            base_url: base_url.into(),
            webhook_host: DEFAULT_WEBHOOK_HOST.into(),
            webhook_port: DEFAULT_WEBHOOK_PORT,
            webhook_path: DEFAULT_WEBHOOK_PATH.into(),
            verification_token: None,
            encrypt_key: None,
            allowed_users: Arc::new(HashSet::new()),
            seen_event_ids: Arc::new(Mutex::new(HashMap::new())),
            dedup_state_path: None,
            card_action_tokens: Arc::new(Mutex::new(HashMap::new())),
            webhook_rate_limits: Arc::new(Mutex::new(HashMap::new())),
            webhook_anomalies: Arc::new(Mutex::new(HashMap::new())),
            tenant_token: Arc::new(RwLock::new(None)),
            bot_name: Arc::new(StdRwLock::new(None)),
            bot_identity_lookup_attempted: Arc::new(Mutex::new(false)),
            http: reqwest::Client::new(),
        }
    }

    #[test]
    fn dedup_state_round_trip_persists_recent_ids() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("seen.json");
        let mut cache = HashMap::new();
        cache.insert(
            "evt_recent".to_string(),
            Instant::now() - Duration::from_secs(5),
        );

        persist_seen_event_ids(Some(&path), &cache);
        let loaded = load_seen_event_ids(&path);

        assert!(loaded.contains_key("evt_recent"));
    }

    #[test]
    fn dedup_state_drops_expired_ids_on_load() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("seen.json");
        let expired = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("unix time")
            .as_secs()
            .saturating_sub(DEDUP_TTL.as_secs() + 5);
        std::fs::write(
            &path,
            serde_json::to_vec(&HashMap::from([(String::from("evt_old"), expired)])).expect("json"),
        )
        .expect("write");

        let loaded = load_seen_event_ids(&path);
        assert!(!loaded.contains_key("evt_old"));
    }

    #[test]
    fn normalize_post_content_collects_title_and_rows() {
        let text = parse_post_content(
            r#"{"zh_cn":{"title":"Spec","content":[[{"tag":"text","text":"Line one"}],[{"tag":"text","text":"Line two"},{"tag":"text","text":"tail"}]]}}"#,
        )
        .text;
        assert_eq!(text, "Spec\nLine one\nLine two tail");
    }

    #[test]
    fn infer_receive_id_type_uses_prefixes() {
        assert_eq!(infer_feishu_receive_id_type("oc_chat"), "open_chat_id");
        assert_eq!(infer_feishu_receive_id_type("ou_user"), "open_id");
        assert_eq!(infer_feishu_receive_id_type("open_user"), "open_id");
        assert_eq!(infer_feishu_receive_id_type("on_union"), "union_id");
        assert_eq!(infer_feishu_receive_id_type("chat_id"), "chat_id");
    }

    #[test]
    fn normalize_text_message_strips_feishu_mentions() {
        let normalized = normalize_message_content("text", r#"{"text":"hi @_user_1 there"}"#);
        assert_eq!(normalized.text, "hi there");
        assert!(normalized.attachments.is_empty());
    }

    #[test]
    fn normalize_post_extracts_text_mentions_and_attachments() {
        let normalized = normalize_message_content(
            "post",
            r#"{"zh_cn":{"title":"Release","content":[[{"tag":"text","text":"see"},{"tag":"at","open_id":"ou_bot","user_name":"EdgeCrab"},{"tag":"img","image_key":"img_1","text":"diagram"}],[{"tag":"file","file_key":"file_1","file_name":"report.pdf"}]]}}"#,
        );
        assert_eq!(
            normalized.text,
            "Release\nsee @EdgeCrab [Image: diagram]\n[Attachment: report.pdf]"
        );
        assert_eq!(normalized.mentioned_ids, vec!["ou_bot"]);
        assert_eq!(normalized.attachments.len(), 2);
        assert_eq!(normalized.attachments[0].kind, MessageAttachmentKind::Image);
        assert_eq!(
            normalized.attachments[1].file_name.as_deref(),
            Some("report.pdf")
        );
    }

    #[test]
    fn parse_event_preserves_attachment_metadata() {
        let payload = json!({
            "event": {
                "sender": {"sender_id": {"open_id": "ou_user"}},
                "message": {
                    "message_id": "om_1",
                    "chat_id": "oc_chat",
                    "message_type": "file",
                    "content": "{\"file_key\":\"file_1\",\"file_name\":\"notes.txt\"}"
                }
            }
        });

        let adapter = test_adapter("http://unused");

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let message = runtime
            .block_on(parse_event(&adapter, &payload))
            .expect("parse ok")
            .expect("message");

        assert_eq!(message.text, "Shared 1 attachment:\n- document: notes.txt");
        assert_eq!(message.metadata.attachments.len(), 1);
        assert_eq!(
            message.metadata.attachments[0].kind,
            MessageAttachmentKind::Document
        );
    }

    #[tokio::test]
    async fn parse_event_downloads_feishu_attachment_to_local_cache() {
        async fn issue_token() -> Json<Value> {
            Json(json!({
                "code": 0,
                "tenant_access_token": "tenant-token",
                "expire": 7200
            }))
        }

        async fn download_resource()
        -> (StatusCode, [(&'static str, &'static str); 2], &'static [u8]) {
            (
                StatusCode::OK,
                [
                    ("content-type", "text/plain"),
                    ("content-disposition", "attachment; filename=\"notes.txt\""),
                ],
                b"hello from feishu",
            )
        }

        let app = Router::new()
            .route(
                "/open-apis/auth/v3/tenant_access_token/internal",
                post(issue_token),
            )
            .route(
                "/open-apis/im/v1/messages/om_1/resources/file_1",
                axum::routing::get(download_resource),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });

        let adapter = test_adapter(format!("http://{addr}"));

        let payload = json!({
            "event": {
                "sender": {"sender_id": {"open_id": "ou_user"}},
                "message": {
                    "message_id": "om_1",
                    "chat_id": "oc_chat",
                    "chat_type": "p2p",
                    "message_type": "file",
                    "content": "{\"file_key\":\"file_1\",\"file_name\":\"notes.txt\"}"
                }
            }
        });

        let message = parse_event(&adapter, &payload)
            .await
            .expect("parse ok")
            .expect("message");

        let attachment = &message.metadata.attachments[0];
        let local_path = attachment.local_path.as_deref().expect("local path");
        assert!(std::path::Path::new(local_path).exists());
        assert_eq!(attachment.mime_type.as_deref(), Some("text/plain"));
        assert!(message.text.contains("notes.txt"));
        assert!(message.text.contains(local_path));

        server.abort();
    }

    #[tokio::test]
    async fn parse_webhook_event_routes_reaction_on_bot_message() {
        async fn issue_token() -> Json<Value> {
            Json(json!({
                "code": 0,
                "tenant_access_token": "tenant-token",
                "expire": 7200
            }))
        }

        async fn get_message() -> Json<Value> {
            Json(json!({
                "code": 0,
                "data": {
                    "items": [{
                        "message_id": "om_bot",
                        "chat_id": "oc_chat",
                        "thread_id": "omt_thread",
                        "sender": {"sender_type": "app"}
                    }]
                }
            }))
        }

        let app = Router::new()
            .route(
                "/open-apis/auth/v3/tenant_access_token/internal",
                post(issue_token),
            )
            .route(
                "/open-apis/im/v1/messages/om_bot",
                axum::routing::get(get_message),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });

        let adapter = test_adapter(format!("http://{addr}"));

        let payload = json!({
            "header": { "event_type": "im.message.reaction.created_v1" },
            "event": {
                "message_id": "om_bot",
                "operator_type": "user",
                "user_id": { "open_id": "ou_user" },
                "reaction_type": { "emoji_type": "THUMBSUP" }
            }
        });

        let message = parse_webhook_event(&adapter, &payload)
            .await
            .expect("parse ok")
            .expect("message");

        assert_eq!(message.text, "reaction:added:THUMBSUP");
        assert_eq!(message.channel_id.as_deref(), Some("oc_chat"));
        assert_eq!(message.thread_id.as_deref(), Some("omt_thread"));

        server.abort();
    }

    #[tokio::test]
    async fn parse_webhook_event_routes_card_action() {
        let adapter = test_adapter("http://unused");
        let payload = json!({
            "header": { "event_type": "card.action.trigger" },
            "event": {
                "token": "tok_1",
                "operator": { "open_id": "ou_user" },
                "context": { "open_chat_id": "oc_chat" },
                "action": {
                    "tag": "button",
                    "value": { "command": "approve" }
                }
            }
        });

        let message = parse_card_action_event(&adapter, &payload)
            .await
            .expect("parse ok")
            .expect("message");

        assert_eq!(message.text, "/card button {\"command\":\"approve\"}");
        assert_eq!(message.channel_id.as_deref(), Some("oc_chat"));
        assert_eq!(message.user_id, "ou_user");
    }

    #[tokio::test]
    async fn duplicate_card_actions_are_dropped_by_token() {
        let adapter = test_adapter("http://unused");
        let payload = json!({
            "header": { "event_type": "card.action.trigger" },
            "event": {
                "token": "tok_dup",
                "operator": { "open_id": "ou_user" },
                "context": { "open_chat_id": "oc_chat" },
                "action": { "tag": "button" }
            }
        });

        let first = parse_card_action_event(&adapter, &payload)
            .await
            .expect("first parse");
        let second = parse_card_action_event(&adapter, &payload)
            .await
            .expect("second parse");

        assert!(first.is_some());
        assert!(second.is_none());
    }

    #[tokio::test]
    async fn ensure_bot_identity_hydrates_bot_name() {
        async fn issue_token() -> Json<Value> {
            Json(json!({
                "code": 0,
                "tenant_access_token": "tenant-token",
                "expire": 7200
            }))
        }

        async fn get_application() -> Json<Value> {
            Json(json!({
                "code": 0,
                "data": {
                    "app": {
                        "app_name": "EdgeCrab Bot"
                    }
                }
            }))
        }

        let app = Router::new()
            .route(
                "/open-apis/auth/v3/tenant_access_token/internal",
                post(issue_token),
            )
            .route(
                "/open-apis/application/v6/applications/app",
                axum::routing::get(get_application),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });

        let adapter = test_adapter(format!("http://{addr}"));
        adapter.ensure_bot_identity().await;

        assert!(is_bot_identity(&adapter, "EdgeCrab Bot"));
        server.abort();
    }

    #[tokio::test]
    async fn group_messages_without_mentions_are_dropped_by_default() {
        let adapter = test_adapter("http://unused");

        let payload = json!({
            "event": {
                "sender": {"sender_id": {"open_id": "ou_user"}},
                "message": {
                    "message_id": "om_1",
                    "chat_id": "oc_chat",
                    "chat_type": "group",
                    "message_type": "text",
                    "content": "{\"text\":\"hello group\"}"
                }
            }
        });

        let message = parse_event(&adapter, &payload).await.expect("parse");
        assert!(message.is_none());
    }

    #[tokio::test]
    async fn webhook_returns_challenge_when_token_matches() {
        let (tx, _rx) = mpsc::channel(1);
        let mut adapter = test_adapter("http://unused");
        adapter.verification_token = Some("verify-me".into());

        let response = adapter
            .router(tx)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(DEFAULT_WEBHOOK_PATH)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"challenge":"abc123","token":"verify-me","type":"url_verification"}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn webhook_rejects_invalid_signature_when_encrypt_key_is_set() {
        let (tx, _rx) = mpsc::channel(1);
        let mut adapter = test_adapter("http://unused");
        adapter.encrypt_key = Some("enc-secret".into());

        let response = adapter
            .router(tx)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(DEFAULT_WEBHOOK_PATH)
                    .header("content-type", "application/json")
                    .header("x-lark-request-timestamp", "123")
                    .header("x-lark-request-nonce", "nonce")
                    .header("x-lark-signature", "bad-signature")
                    .body(Body::from(
                        r#"{"header":{"event_id":"evt-1","event_type":"im.message.receive_v1"},"event":{"sender":{"sender_id":{"open_id":"ou_user"}},"message":{"message_id":"om_1","chat_id":"oc_chat","chat_type":"p2p","message_type":"text","content":"{\"text\":\"hello\"}"}}}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn webhook_rejects_non_json_content_type() {
        let (tx, _rx) = mpsc::channel(1);
        let adapter = test_adapter("http://unused");

        let response = adapter
            .router(tx)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(DEFAULT_WEBHOOK_PATH)
                    .header("content-type", "text/plain")
                    .body(Body::from("hello"))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[tokio::test]
    async fn webhook_filters_duplicate_events_and_allowlist() {
        let (tx, mut rx) = mpsc::channel(2);
        let mut adapter = test_adapter("http://unused");
        adapter.allowed_users = Arc::new(HashSet::from([String::from("ou_allowed")]));
        let router = adapter.router(tx);

        let allowed = r#"{
            "header":{"event_id":"evt-1"},
            "event":{
                "sender":{"sender_id":{"open_id":"ou_allowed"}},
                "message":{
                    "message_id":"om_1",
                    "chat_id":"oc_chat",
                    "message_type":"text",
                    "content":"{\"text\":\"hello\"}"
                }
            }
        }"#;
        let blocked = r#"{
            "header":{"event_id":"evt-2"},
            "event":{
                "sender":{"sender_id":{"open_id":"ou_blocked"}},
                "message":{
                    "message_id":"om_2",
                    "chat_id":"oc_chat",
                    "message_type":"text",
                    "content":"{\"text\":\"ignore me\"}"
                }
            }
        }"#;

        let _ = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(DEFAULT_WEBHOOK_PATH)
                    .header("content-type", "application/json")
                    .body(Body::from(allowed))
                    .expect("request"),
            )
            .await
            .expect("allowed response");

        let _ = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(DEFAULT_WEBHOOK_PATH)
                    .header("content-type", "application/json")
                    .body(Body::from(allowed))
                    .expect("duplicate request"),
            )
            .await
            .expect("duplicate response");

        let _ = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(DEFAULT_WEBHOOK_PATH)
                    .header("content-type", "application/json")
                    .body(Body::from(blocked))
                    .expect("blocked request"),
            )
            .await
            .expect("blocked response");

        let message = rx.recv().await.expect("allowed message");
        assert_eq!(message.text, "hello");
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn send_uses_token_endpoint_and_message_endpoint() {
        use axum::extract::State as AxumState;

        #[derive(Clone, Default)]
        struct TestState {
            bodies: Arc<Mutex<Vec<Value>>>,
            uris: Arc<Mutex<Vec<String>>>,
        }

        async fn issue_token() -> Json<Value> {
            Json(json!({
                "code": 0,
                "tenant_access_token": "tenant-token",
                "expire": 7200
            }))
        }

        async fn accept_message(
            AxumState(state): AxumState<TestState>,
            OriginalUri(uri): OriginalUri,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            let mut bodies = state.bodies.lock().await;
            bodies.push(body);
            let message_id = format!("om_sent_{}", bodies.len());
            drop(bodies);
            state.uris.lock().await.push(uri.to_string());
            Json(json!({
                "code": 0,
                "data": { "message_id": message_id }
            }))
        }

        let state = TestState::default();
        let app = Router::new()
            .route(
                "/open-apis/auth/v3/tenant_access_token/internal",
                post(issue_token),
            )
            .route("/open-apis/im/v1/messages", post(accept_message))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });

        let adapter = test_adapter(format!("http://{addr}"));

        let message_id = adapter
            .send_and_get_id(OutgoingMessage {
                text: "hello feishu".into(),
                metadata: MessageMetadata {
                    channel_id: Some("oc_chat".into()),
                    ..Default::default()
                },
            })
            .await
            .expect("send");

        assert_eq!(message_id.as_deref(), Some("om_sent_1"));
        let bodies = state.bodies.lock().await;
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0]["receive_id"], "oc_chat");
        drop(bodies);
        let uris = state.uris.lock().await;
        assert_eq!(
            uris[0],
            "/open-apis/im/v1/messages?receive_id_type=open_chat_id"
        );

        server.abort();
    }

    #[tokio::test]
    async fn send_and_get_id_splits_long_messages_and_returns_last_id() {
        #[derive(Clone, Default)]
        struct TestState {
            bodies: Arc<Mutex<Vec<Value>>>,
        }

        async fn issue_token() -> Json<Value> {
            Json(json!({
                "code": 0,
                "tenant_access_token": "tenant-token",
                "expire": 7200
            }))
        }

        async fn accept_message(
            AxumState(state): AxumState<TestState>,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            let mut bodies = state.bodies.lock().await;
            bodies.push(body);
            let message_id = format!("om_sent_{}", bodies.len());
            Json(json!({
                "code": 0,
                "data": { "message_id": message_id }
            }))
        }

        let state = TestState::default();
        let app = Router::new()
            .route(
                "/open-apis/auth/v3/tenant_access_token/internal",
                post(issue_token),
            )
            .route("/open-apis/im/v1/messages", post(accept_message))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });

        let adapter = test_adapter(format!("http://{addr}"));

        let message_id = adapter
            .send_and_get_id(OutgoingMessage {
                text: "a".repeat(MAX_MESSAGE_LENGTH + 32),
                metadata: MessageMetadata {
                    channel_id: Some("chat_id".into()),
                    ..Default::default()
                },
            })
            .await
            .expect("send");

        assert_eq!(message_id.as_deref(), Some("om_sent_2"));
        assert_eq!(state.bodies.lock().await.len(), 2);

        server.abort();
    }

    #[tokio::test]
    async fn edit_message_uses_update_endpoint() {
        #[derive(Clone, Default)]
        struct TestState {
            updates: Arc<Mutex<Vec<Value>>>,
        }

        async fn issue_token() -> Json<Value> {
            Json(json!({
                "code": 0,
                "tenant_access_token": "tenant-token",
                "expire": 7200
            }))
        }

        async fn accept_update(
            AxumState(state): AxumState<TestState>,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            state.updates.lock().await.push(body);
            Json(json!({
                "code": 0,
                "data": { "message_id": "om_existing" }
            }))
        }

        let state = TestState::default();
        let app = Router::new()
            .route(
                "/open-apis/auth/v3/tenant_access_token/internal",
                post(issue_token),
            )
            .route("/open-apis/im/v1/messages/om_existing", put(accept_update))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });

        let adapter = test_adapter(format!("http://{addr}"));

        let returned = adapter
            .edit_message("om_existing", &MessageMetadata::default(), "updated")
            .await
            .expect("edit");

        assert_eq!(returned, "om_existing");
        let updates = state.updates.lock().await;
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0]["msg_type"], "text");
        assert_eq!(updates[0]["content"], "{\"text\":\"updated\"}");

        server.abort();
    }

    #[tokio::test]
    async fn send_photo_uploads_image_and_sends_native_image_message() {
        #[derive(Clone, Default)]
        struct TestState {
            image_uploads: Arc<Mutex<Vec<String>>>,
            messages: Arc<Mutex<Vec<Value>>>,
            uris: Arc<Mutex<Vec<String>>>,
        }

        async fn issue_token() -> Json<Value> {
            Json(json!({
                "code": 0,
                "tenant_access_token": "tenant-token",
                "expire": 7200
            }))
        }

        async fn upload_image(AxumState(state): AxumState<TestState>, bytes: Bytes) -> Json<Value> {
            state
                .image_uploads
                .lock()
                .await
                .push(String::from_utf8_lossy(&bytes).into_owned());
            Json(json!({
                "code": 0,
                "data": { "image_key": "img_123" }
            }))
        }

        async fn accept_message(
            AxumState(state): AxumState<TestState>,
            OriginalUri(uri): OriginalUri,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            state.messages.lock().await.push(body);
            state.uris.lock().await.push(uri.to_string());
            Json(json!({
                "code": 0,
                "data": { "message_id": "om_sent_1" }
            }))
        }

        let state = TestState::default();
        let app = Router::new()
            .route(
                "/open-apis/auth/v3/tenant_access_token/internal",
                post(issue_token),
            )
            .route("/open-apis/im/v1/images", post(upload_image))
            .route("/open-apis/im/v1/messages", post(accept_message))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });

        let temp = tempfile::tempdir().expect("tempdir");
        let image_path = temp.path().join("sample.png");
        std::fs::write(&image_path, b"fake-image").expect("write image");

        let adapter = test_adapter(format!("http://{addr}"));

        adapter
            .send_photo(
                image_path.to_str().expect("utf8 path"),
                None,
                &MessageMetadata {
                    channel_id: Some("oc_chat".into()),
                    ..Default::default()
                },
            )
            .await
            .expect("send photo");

        let uploads = state.image_uploads.lock().await;
        assert_eq!(uploads.len(), 1);
        assert!(uploads[0].contains("name=\"image_type\""));
        assert!(uploads[0].contains(FEISHU_IMAGE_UPLOAD_TYPE));
        assert!(uploads[0].contains("filename=\"sample.png\""));
        drop(uploads);

        let messages = state.messages.lock().await;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["msg_type"], "image");
        assert_eq!(messages[0]["content"], "{\"image_key\":\"img_123\"}");
        drop(messages);

        let uris = state.uris.lock().await;
        assert_eq!(
            uris[0],
            "/open-apis/im/v1/messages?receive_id_type=open_chat_id"
        );

        server.abort();
    }

    #[tokio::test]
    async fn send_document_uploads_file_and_sends_caption_post() {
        #[derive(Clone, Default)]
        struct TestState {
            file_uploads: Arc<Mutex<Vec<String>>>,
            messages: Arc<Mutex<Vec<Value>>>,
        }

        async fn issue_token() -> Json<Value> {
            Json(json!({
                "code": 0,
                "tenant_access_token": "tenant-token",
                "expire": 7200
            }))
        }

        async fn upload_file(AxumState(state): AxumState<TestState>, bytes: Bytes) -> Json<Value> {
            state
                .file_uploads
                .lock()
                .await
                .push(String::from_utf8_lossy(&bytes).into_owned());
            Json(json!({
                "code": 0,
                "data": { "file_key": "file_123" }
            }))
        }

        async fn accept_message(
            AxumState(state): AxumState<TestState>,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            state.messages.lock().await.push(body);
            Json(json!({
                "code": 0,
                "data": { "message_id": "om_sent_1" }
            }))
        }

        let state = TestState::default();
        let app = Router::new()
            .route(
                "/open-apis/auth/v3/tenant_access_token/internal",
                post(issue_token),
            )
            .route("/open-apis/im/v1/files", post(upload_file))
            .route("/open-apis/im/v1/messages", post(accept_message))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });

        let temp = tempfile::tempdir().expect("tempdir");
        let file_path = temp.path().join("notes.pdf");
        std::fs::write(&file_path, b"fake-pdf").expect("write file");

        let adapter = test_adapter(format!("http://{addr}"));

        adapter
            .send_document(
                file_path.to_str().expect("utf8 path"),
                Some("Report ready"),
                &MessageMetadata {
                    channel_id: Some("oc_chat".into()),
                    ..Default::default()
                },
            )
            .await
            .expect("send document");

        let uploads = state.file_uploads.lock().await;
        assert_eq!(uploads.len(), 1);
        assert!(uploads[0].contains("name=\"file_type\""));
        assert!(uploads[0].contains("pdf"));
        assert!(uploads[0].contains("filename=\"notes.pdf\""));
        drop(uploads);

        let messages = state.messages.lock().await;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["msg_type"], "post");
        let content = messages[0]["content"].as_str().expect("post content");
        assert!(content.contains("Report ready"));
        assert!(content.contains("\"file_key\":\"file_123\""));
        assert!(content.contains("\"file_name\":\"notes.pdf\""));

        server.abort();
    }

    #[tokio::test]
    async fn send_replies_and_falls_back_to_chat_when_reply_target_is_missing() {
        #[derive(Clone, Default)]
        struct TestState {
            reply_requests: Arc<Mutex<Vec<String>>>,
            message_requests: Arc<Mutex<Vec<Value>>>,
        }

        async fn issue_token() -> Json<Value> {
            Json(json!({
                "code": 0,
                "tenant_access_token": "tenant-token",
                "expire": 7200
            }))
        }

        async fn reject_reply(AxumState(state): AxumState<TestState>, bytes: Bytes) -> Json<Value> {
            state
                .reply_requests
                .lock()
                .await
                .push(format!("om_missing:{}", String::from_utf8_lossy(&bytes)));
            Json(json!({
                "code": 230011,
                "msg": "message not found"
            }))
        }

        async fn accept_message(
            AxumState(state): AxumState<TestState>,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            state.message_requests.lock().await.push(body);
            Json(json!({
                "code": 0,
                "data": { "message_id": "om_fallback" }
            }))
        }

        let state = TestState::default();
        let app = Router::new()
            .route(
                "/open-apis/auth/v3/tenant_access_token/internal",
                post(issue_token),
            )
            .route(
                "/open-apis/im/v1/messages/om_missing/reply",
                post(reject_reply),
            )
            .route("/open-apis/im/v1/messages", post(accept_message))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });

        let adapter = test_adapter(format!("http://{addr}"));

        let message_id = adapter
            .send_and_get_id(OutgoingMessage {
                text: "reply me".into(),
                metadata: MessageMetadata {
                    channel_id: Some("oc_chat".into()),
                    message_id: Some("om_missing".into()),
                    ..Default::default()
                },
            })
            .await
            .expect("send");

        assert_eq!(message_id.as_deref(), Some("om_fallback"));
        assert_eq!(state.reply_requests.lock().await.len(), 1);
        assert_eq!(state.message_requests.lock().await.len(), 1);

        server.abort();
    }
}
