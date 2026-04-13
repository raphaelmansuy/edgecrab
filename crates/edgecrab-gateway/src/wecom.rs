//! # WeCom adapter — AI bot websocket ingress + egress
//!
//! Core path covered by this adapter:
//! - websocket subscribe handshake
//! - inbound text callback parsing with dedup and allowlist checks
//! - correlated reply delivery using callback request IDs
//! - proactive outbound send for non-reply delivery paths

use std::collections::{HashMap, HashSet};
use std::env;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::Engine;
use edgecrab_types::Platform;
use futures::{Sink, SinkExt, Stream, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use crate::delivery::split_message;
use crate::platform::{IncomingMessage, MessageMetadata, OutgoingMessage, PlatformAdapter};

const DEFAULT_WS_URL: &str = "wss://openws.work.weixin.qq.com";
const APP_CMD_SUBSCRIBE: &str = "aibot_subscribe";
const APP_CMD_CALLBACK: &str = "aibot_msg_callback";
const APP_CMD_LEGACY_CALLBACK: &str = "aibot_callback";
const APP_CMD_SEND: &str = "aibot_send_msg";
const APP_CMD_RESPONSE: &str = "aibot_respond_msg";
const APP_CMD_UPLOAD_MEDIA_INIT: &str = "aibot_upload_media_init";
const APP_CMD_UPLOAD_MEDIA_CHUNK: &str = "aibot_upload_media_chunk";
const APP_CMD_UPLOAD_MEDIA_FINISH: &str = "aibot_upload_media_finish";
const APP_CMD_PING: &str = "ping";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const DEDUP_TTL: Duration = Duration::from_secs(60 * 5);
const MAX_MESSAGE_LENGTH: usize = 4000;
const BACKOFF_STEPS: &[u64] = &[2, 5, 10, 30];
const UPLOAD_CHUNK_SIZE: usize = 512 * 1024;
const MAX_UPLOAD_CHUNKS: usize = 100;

pub struct WeComAdapter {
    bot_id: String,
    secret: String,
    ws_url: String,
    allowed_users: Arc<HashSet<String>>,
    outbound_tx: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>,
    seen_messages: Arc<Mutex<HashMap<String, Instant>>>,
    reply_req_ids: Arc<Mutex<HashMap<String, String>>>,
}

impl WeComAdapter {
    pub fn from_env() -> Option<Self> {
        let bot_id = env::var("WECOM_BOT_ID").ok()?.trim().to_string();
        let secret = env::var("WECOM_SECRET").ok()?.trim().to_string();
        if bot_id.is_empty() || secret.is_empty() {
            return None;
        }

        Some(Self {
            bot_id,
            secret,
            ws_url: env::var("WECOM_WEBSOCKET_URL")
                .unwrap_or_else(|_| DEFAULT_WS_URL.to_string())
                .trim()
                .to_string(),
            allowed_users: Arc::new(parse_csv_set("WECOM_ALLOWED_USERS")),
            outbound_tx: Arc::new(Mutex::new(None)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            seen_messages: Arc::new(Mutex::new(HashMap::new())),
            reply_req_ids: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn is_available() -> bool {
        env::var("WECOM_BOT_ID").is_ok() && env::var("WECOM_SECRET").is_ok()
    }

    async fn current_sender(&self) -> anyhow::Result<mpsc::UnboundedSender<String>> {
        self.outbound_tx
            .lock()
            .await
            .clone()
            .ok_or_else(|| anyhow::anyhow!("WeCom websocket is not connected"))
    }

    async fn send_request(&self, cmd: &str, body: Value) -> anyhow::Result<Value> {
        let req_id = format!("{cmd}-{}", Uuid::new_v4());
        self.send_request_with_req_id(cmd, req_id, body).await
    }

    async fn send_request_with_req_id(
        &self,
        cmd: &str,
        req_id: String,
        body: Value,
    ) -> anyhow::Result<Value> {
        let sender = self.current_sender().await?;
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(req_id.clone(), tx);

        let payload = json!({
            "cmd": cmd,
            "headers": { "req_id": req_id },
            "body": body,
        });
        if sender.send(payload.to_string()).is_err() {
            self.pending
                .lock()
                .await
                .remove(payload_req_id(&payload).as_str());
            anyhow::bail!("WeCom websocket writer is unavailable");
        }

        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(response)) => {
                self.pending
                    .lock()
                    .await
                    .remove(payload_req_id(&payload).as_str());
                raise_if_wecom_error(&response)?;
                Ok(response)
            }
            Ok(Err(_)) => {
                self.pending
                    .lock()
                    .await
                    .remove(payload_req_id(&payload).as_str());
                anyhow::bail!("WeCom response channel closed");
            }
            Err(_) => {
                self.pending
                    .lock()
                    .await
                    .remove(payload_req_id(&payload).as_str());
                anyhow::bail!("Timed out waiting for WeCom {cmd} response");
            }
        }
    }

    async fn send_reply_request(&self, reply_req_id: &str, body: Value) -> anyhow::Result<Value> {
        self.send_request_with_req_id(APP_CMD_RESPONSE, reply_req_id.to_string(), body)
            .await
    }

    async fn reply_req_id_for_message(&self, message_id: Option<&str>) -> Option<String> {
        let message_id = message_id?;
        self.reply_req_ids.lock().await.get(message_id).cloned()
    }

    async fn send_markdown_chunks(
        &self,
        channel_id: &str,
        text: &str,
        reply_req_id: Option<&str>,
    ) -> anyhow::Result<()> {
        let chunks = split_message(text, MAX_MESSAGE_LENGTH);
        for chunk in chunks {
            let body = json!({
                "chatid": channel_id,
                "msgtype": "markdown",
                "markdown": { "content": chunk },
            });
            if let Some(reply_req_id) = reply_req_id {
                let _ = self.send_reply_request(reply_req_id, body).await?;
            } else {
                let _ = self.send_request(APP_CMD_SEND, body).await?;
            }
        }
        Ok(())
    }

    async fn upload_media_bytes(
        &self,
        data: &[u8],
        media_type: &str,
        filename: &str,
    ) -> anyhow::Result<String> {
        if data.is_empty() {
            anyhow::bail!("WeCom media upload requires a non-empty file");
        }

        let total_chunks = data.len().div_ceil(UPLOAD_CHUNK_SIZE);
        if total_chunks > MAX_UPLOAD_CHUNKS {
            anyhow::bail!(
                "WeCom media upload exceeds maximum chunk count ({total_chunks} > {MAX_UPLOAD_CHUNKS})"
            );
        }

        let init = self
            .send_request(
                APP_CMD_UPLOAD_MEDIA_INIT,
                json!({
                    "type": media_type,
                    "filename": filename,
                    "total_size": data.len(),
                    "total_chunks": total_chunks,
                    "md5": format!("{:x}", md5::compute(data)),
                }),
            )
            .await?;
        let upload_id = init
            .get("body")
            .and_then(|value| value.get("upload_id"))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("WeCom media upload init missing upload_id"))?;

        for (chunk_index, chunk) in data.chunks(UPLOAD_CHUNK_SIZE).enumerate() {
            let _ = self
                .send_request(
                    APP_CMD_UPLOAD_MEDIA_CHUNK,
                    json!({
                        "upload_id": upload_id,
                        "chunk_index": chunk_index,
                        "base64_data": base64::engine::general_purpose::STANDARD.encode(chunk),
                    }),
                )
                .await?;
        }

        let finish = self
            .send_request(
                APP_CMD_UPLOAD_MEDIA_FINISH,
                json!({ "upload_id": upload_id }),
            )
            .await?;
        finish
            .get("body")
            .and_then(|value| value.get("media_id"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("WeCom media upload finish missing media_id"))
    }

    async fn send_media_path(
        &self,
        path: &str,
        media_type: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        let channel_id = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("WeCom media delivery requires channel_id"))?;
        let file_bytes = tokio::fs::read(path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read WeCom media '{}': {}", path, e))?;
        let filename = Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("attachment.bin");
        let reply_req_id = self
            .reply_req_id_for_message(metadata.message_id.as_deref())
            .await;

        let media_id = self
            .upload_media_bytes(&file_bytes, media_type, filename)
            .await?;
        let body = json!({
            "chatid": channel_id,
            "msgtype": media_type,
            media_type: { "media_id": media_id },
        });
        if let Some(reply_req_id) = reply_req_id.as_deref() {
            let _ = self.send_reply_request(reply_req_id, body).await?;
        } else {
            let _ = self.send_request(APP_CMD_SEND, body).await?;
        }

        if let Some(caption) = caption.map(str::trim).filter(|caption| !caption.is_empty()) {
            self.send_markdown_chunks(channel_id, caption, reply_req_id.as_deref())
                .await?;
        }

        Ok(())
    }
}

#[async_trait]
impl PlatformAdapter for WeComAdapter {
    fn platform(&self) -> Platform {
        Platform::Wecom
    }

    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
        let mut backoff_index = 0usize;

        loop {
            let connect = tokio::time::timeout(CONNECT_TIMEOUT, connect_async(&self.ws_url)).await;
            let (stream, _) = match connect {
                Ok(Ok(parts)) => parts,
                Ok(Err(err)) => {
                    tracing::warn!(error = %err, url = %self.ws_url, "WeCom connect failed");
                    tokio::time::sleep(Duration::from_secs(BACKOFF_STEPS[backoff_index])).await;
                    backoff_index = (backoff_index + 1).min(BACKOFF_STEPS.len() - 1);
                    continue;
                }
                Err(_) => {
                    tracing::warn!(url = %self.ws_url, "WeCom connect timed out");
                    tokio::time::sleep(Duration::from_secs(BACKOFF_STEPS[backoff_index])).await;
                    backoff_index = (backoff_index + 1).min(BACKOFF_STEPS.len() - 1);
                    continue;
                }
            };

            backoff_index = 0;
            let (write, read) = stream.split();
            let (out_tx, out_rx) = mpsc::unbounded_channel();
            *self.outbound_tx.lock().await = Some(out_tx);

            let pending = self.pending.clone();
            let reader = tokio::spawn(connection_reader(
                read,
                tx.clone(),
                self.allowed_users.clone(),
                self.pending.clone(),
                self.seen_messages.clone(),
                self.reply_req_ids.clone(),
            ));
            let writer = tokio::spawn(connection_writer(write, out_rx));

            let auth_result = self
                .send_request(
                    APP_CMD_SUBSCRIBE,
                    json!({
                        "bot_id": self.bot_id,
                        "secret": self.secret,
                    }),
                )
                .await;

            if let Err(err) = auth_result {
                clear_connection(&self.outbound_tx, &pending).await;
                reader.abort();
                writer.abort();
                return Err(err);
            }

            match reader.await {
                Ok(Ok(())) => {
                    clear_connection(&self.outbound_tx, &pending).await;
                    writer.abort();
                    anyhow::bail!("WeCom reader exited unexpectedly");
                }
                Ok(Err(err)) => {
                    tracing::warn!(error = %err, "WeCom reader stopped");
                }
                Err(err) => {
                    tracing::warn!(error = %err, "WeCom reader task panicked");
                }
            }

            clear_connection(&self.outbound_tx, &pending).await;
            writer.abort();
            tokio::time::sleep(Duration::from_secs(BACKOFF_STEPS[backoff_index])).await;
            backoff_index = (backoff_index + 1).min(BACKOFF_STEPS.len() - 1);
        }
    }

    async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()> {
        let channel_id = msg
            .metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("WeCom delivery requires channel_id"))?;
        let reply_req_id = self
            .reply_req_id_for_message(msg.metadata.message_id.as_deref())
            .await;
        self.send_markdown_chunks(channel_id, &msg.text, reply_req_id.as_deref())
            .await
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
        self.send_media_path(path, "image", caption, metadata).await
    }

    async fn send_document(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        self.send_media_path(path, "file", caption, metadata).await
    }
}

async fn clear_connection(
    outbound_tx: &Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
    pending: &Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>,
) {
    *outbound_tx.lock().await = None;
    pending.lock().await.clear();
}

async fn connection_writer<S>(
    mut write: S,
    mut rx: mpsc::UnboundedReceiver<String>,
) -> anyhow::Result<()>
where
    S: Sink<Message> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    while let Some(frame) = rx.recv().await {
        write.send(Message::Text(frame)).await?;
    }
    Ok(())
}

async fn connection_reader<S>(
    mut read: S,
    tx: mpsc::Sender<IncomingMessage>,
    allowed_users: Arc<HashSet<String>>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>,
    seen_messages: Arc<Mutex<HashMap<String, Instant>>>,
    reply_req_ids: Arc<Mutex<HashMap<String, String>>>,
) -> anyhow::Result<()>
where
    S: Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    while let Some(frame) = read.next().await {
        let frame = frame?;
        match frame {
            Message::Text(text) => {
                let payload: Value = serde_json::from_str(&text)?;
                let req_id = payload_req_id(&payload);
                let cmd = payload
                    .get("cmd")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();

                if !req_id.is_empty() && !is_callback_command(&cmd) && cmd != APP_CMD_PING {
                    if let Some(waiter) = pending.lock().await.remove(&req_id) {
                        let _ = waiter.send(payload);
                        continue;
                    }
                }

                if is_callback_command(&cmd) {
                    if let Some(message) =
                        parse_callback(&payload, &allowed_users, &seen_messages).await?
                    {
                        if let Some(message_id) = message.metadata.message_id.clone() {
                            reply_req_ids.lock().await.insert(message_id, req_id);
                        }
                        let _ = tx.send(message).await;
                    }
                }
            }
            Message::Close(_) => anyhow::bail!("WeCom websocket closed"),
            _ => {}
        }
    }

    anyhow::bail!("WeCom websocket stream ended")
}

async fn parse_callback(
    payload: &Value,
    allowed_users: &HashSet<String>,
    seen_messages: &Arc<Mutex<HashMap<String, Instant>>>,
) -> anyhow::Result<Option<IncomingMessage>> {
    let body = match payload.get("body") {
        Some(value) => value,
        None => return Ok(None),
    };

    let fallback_req_id = payload_req_id(payload);
    let message_id = body
        .get("msgid")
        .and_then(Value::as_str)
        .or_else(|| payload.get("msgid").and_then(Value::as_str))
        .map(str::to_string)
        .unwrap_or(fallback_req_id)
        .trim()
        .to_string();
    if message_id.is_empty() || is_duplicate(seen_messages, &message_id).await {
        return Ok(None);
    }

    let user_id = body
        .get("from")
        .and_then(|value| value.get("userid"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if user_id.is_empty() {
        return Ok(None);
    }
    if !user_is_allowed(allowed_users, &user_id) {
        return Ok(None);
    }

    let channel_id = body
        .get("chatid")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| user_id.clone())
        .trim()
        .to_string();
    if channel_id.is_empty() {
        return Ok(None);
    }

    let text = extract_text(body);
    if text.trim().is_empty() {
        return Ok(None);
    }

    Ok(Some(IncomingMessage {
        platform: Platform::Wecom,
        user_id,
        channel_id: Some(channel_id.clone()),
        chat_type: crate::platform::ChatType::Dm,
        text,
        thread_id: None,
        metadata: MessageMetadata {
            message_id: Some(message_id),
            channel_id: Some(channel_id),
            thread_id: None,
            user_display_name: None,
            attachments: Vec::new(),
            ..Default::default()
        },
    }))
}

fn extract_text(body: &Value) -> String {
    let msgtype = body
        .get("msgtype")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();

    match msgtype.as_str() {
        "mixed" => body
            .get("mixed")
            .and_then(|value| value.get("msg_item"))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        if item.get("msgtype").and_then(Value::as_str) == Some("text") {
                            item.get("text")
                                .and_then(|value| value.get("content"))
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(str::to_string)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default(),
        "text" => body
            .get("text")
            .and_then(|value| value.get("content"))
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .to_string(),
        "voice" => body
            .get("voice")
            .and_then(|value| value.get("content"))
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .to_string(),
        "image" => "[Image]".to_string(),
        "file" => "[File]".to_string(),
        _ => String::new(),
    }
}

async fn is_duplicate(cache: &Arc<Mutex<HashMap<String, Instant>>>, message_id: &str) -> bool {
    let mut guard = cache.lock().await;
    let now = Instant::now();
    guard.retain(|_, seen_at| now.duration_since(*seen_at) <= DEDUP_TTL);
    if guard.contains_key(message_id) {
        return true;
    }
    guard.insert(message_id.to_string(), now);
    false
}

fn payload_req_id(payload: &Value) -> String {
    payload
        .get("headers")
        .and_then(|value| value.get("req_id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn is_callback_command(cmd: &str) -> bool {
    matches!(cmd, APP_CMD_CALLBACK | APP_CMD_LEGACY_CALLBACK)
}

fn raise_if_wecom_error(payload: &Value) -> anyhow::Result<()> {
    let errcode = payload.get("errcode").and_then(Value::as_i64).unwrap_or(0);
    if errcode == 0 {
        return Ok(());
    }
    let errmsg = payload
        .get("errmsg")
        .and_then(Value::as_str)
        .unwrap_or("unknown error");
    anyhow::bail!("WeCom returned errcode {errcode}: {errmsg}");
}

fn parse_csv_set(key: &str) -> HashSet<String> {
    env::var(key)
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(normalize_wecom_identity)
                .collect()
        })
        .unwrap_or_default()
}

fn user_is_allowed(allowed_users: &HashSet<String>, user_id: &str) -> bool {
    if allowed_users.is_empty() {
        return true;
    }

    let normalized_user_id = normalize_wecom_identity(user_id);
    allowed_users.iter().any(|entry| {
        let normalized_entry = normalize_wecom_identity(entry);
        normalized_entry == "*" || normalized_entry == normalized_user_id
    })
}

fn normalize_wecom_identity(raw: &str) -> String {
    let mut value = raw.trim();
    if let Some(stripped) = strip_ascii_prefix(value, "wecom:") {
        value = stripped;
    }
    if let Some(stripped) = strip_ascii_prefix(value, "user:") {
        value = stripped;
    } else if let Some(stripped) = strip_ascii_prefix(value, "group:") {
        value = stripped;
    }
    value.trim().to_ascii_lowercase()
}

fn strip_ascii_prefix<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    value
        .get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .map(|_| &value[prefix.len()..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    #[test]
    fn extract_text_supports_mixed_payloads() {
        let body = json!({
            "msgtype": "mixed",
            "mixed": {
                "msg_item": [
                    {"msgtype": "text", "text": {"content": "hello"}},
                    {"msgtype": "text", "text": {"content": "world"}}
                ]
            }
        });
        assert_eq!(extract_text(&body), "hello\nworld");
    }

    #[test]
    fn normalize_wecom_identity_strips_prefixes_case_insensitively() {
        assert_eq!(normalize_wecom_identity("wecom:user:Alice"), "alice");
        assert_eq!(normalize_wecom_identity("GROUP:Chat-1"), "chat-1");
        assert_eq!(normalize_wecom_identity("*"), "*");
    }

    #[tokio::test]
    async fn parse_callback_respects_allowlist_and_dedup() {
        let payload = json!({
            "cmd": APP_CMD_CALLBACK,
            "headers": { "req_id": "incoming-1" },
            "body": {
                "msgid": "msg-1",
                "chatid": "chat-1",
                "msgtype": "text",
                "from": { "userid": "alice" },
                "text": { "content": "hello" }
            }
        });
        let allowed = HashSet::from([String::from("WECOM:USER:ALICE")]);
        let seen = Arc::new(Mutex::new(HashMap::new()));

        let first = parse_callback(&payload, &allowed, &seen)
            .await
            .expect("first parse");
        let second = parse_callback(&payload, &allowed, &seen)
            .await
            .expect("second parse");
        let blocked = parse_callback(
            &payload,
            &HashSet::from([String::from("bob")]),
            &Arc::new(Mutex::new(HashMap::new())),
        )
        .await
        .expect("blocked parse");

        assert_eq!(first.expect("message").text, "hello");
        assert!(second.is_none());
        assert!(blocked.is_none());
    }

    #[tokio::test]
    async fn start_receives_callback_and_send_reuses_reply_req_id() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let addr = listener.local_addr().expect("addr");

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut ws = accept_async(stream).await.expect("handshake");

            let subscribe = ws
                .next()
                .await
                .expect("subscribe frame")
                .expect("subscribe");
            let subscribe_payload: Value =
                serde_json::from_str(subscribe.into_text().expect("text").as_str())
                    .expect("subscribe payload");
            let subscribe_req_id = payload_req_id(&subscribe_payload);
            ws.send(Message::Text(
                json!({
                    "cmd": APP_CMD_SUBSCRIBE,
                    "headers": { "req_id": subscribe_req_id },
                    "errcode": 0
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("subscribe ack");

            ws.send(Message::Text(
                json!({
                    "cmd": APP_CMD_CALLBACK,
                    "headers": { "req_id": "incoming-req-1" },
                    "body": {
                        "msgid": "msg-1",
                        "chatid": "chat-1",
                        "msgtype": "text",
                        "from": { "userid": "alice" },
                        "text": { "content": "hello wecom" }
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("callback");

            let reply = ws.next().await.expect("reply frame").expect("reply");
            let reply_payload: Value =
                serde_json::from_str(reply.into_text().expect("text").as_str())
                    .expect("reply payload");
            assert_eq!(reply_payload["cmd"], APP_CMD_RESPONSE);
            assert_eq!(payload_req_id(&reply_payload), "incoming-req-1");
            assert_eq!(reply_payload["body"]["chatid"], "chat-1");

            ws.send(Message::Text(
                json!({
                    "cmd": APP_CMD_RESPONSE,
                    "headers": { "req_id": "incoming-req-1" },
                    "errcode": 0
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("reply ack");
        });

        let adapter = Arc::new(WeComAdapter {
            bot_id: "bot".into(),
            secret: "secret".into(),
            ws_url: format!("ws://{addr}"),
            allowed_users: Arc::new(HashSet::new()),
            outbound_tx: Arc::new(Mutex::new(None)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            seen_messages: Arc::new(Mutex::new(HashMap::new())),
            reply_req_ids: Arc::new(Mutex::new(HashMap::new())),
        });

        let (tx, mut rx) = mpsc::channel(4);
        let runner = adapter.clone();
        let run_task = tokio::spawn(async move { runner.start(tx).await });

        let incoming = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("receive timeout")
            .expect("incoming");
        assert_eq!(incoming.text, "hello wecom");

        adapter
            .send(OutgoingMessage {
                text: "reply body".into(),
                metadata: MessageMetadata {
                    channel_id: Some("chat-1".into()),
                    message_id: Some("msg-1".into()),
                    ..Default::default()
                },
            })
            .await
            .expect("send reply");

        run_task.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn send_photo_uploads_media_and_sends_caption() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let addr = listener.local_addr().expect("addr");

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut ws = accept_async(stream).await.expect("handshake");

            let subscribe = ws
                .next()
                .await
                .expect("subscribe frame")
                .expect("subscribe");
            let subscribe_payload: Value =
                serde_json::from_str(subscribe.into_text().expect("text").as_str())
                    .expect("subscribe payload");
            let subscribe_req_id = payload_req_id(&subscribe_payload);
            ws.send(Message::Text(
                json!({
                    "cmd": APP_CMD_SUBSCRIBE,
                    "headers": { "req_id": subscribe_req_id },
                    "errcode": 0
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("subscribe ack");

            let init_frame = ws.next().await.expect("init frame").expect("init");
            let init_payload: Value =
                serde_json::from_str(init_frame.into_text().expect("text").as_str())
                    .expect("init payload");
            assert_eq!(init_payload["cmd"], APP_CMD_UPLOAD_MEDIA_INIT);
            assert_eq!(init_payload["body"]["type"], "image");
            let init_req_id = payload_req_id(&init_payload);
            ws.send(Message::Text(
                json!({
                    "cmd": APP_CMD_UPLOAD_MEDIA_INIT,
                    "headers": { "req_id": init_req_id },
                    "errcode": 0,
                    "body": { "upload_id": "upload-1" }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("init ack");

            let chunk_frame = ws.next().await.expect("chunk frame").expect("chunk");
            let chunk_payload: Value =
                serde_json::from_str(chunk_frame.into_text().expect("text").as_str())
                    .expect("chunk payload");
            assert_eq!(chunk_payload["cmd"], APP_CMD_UPLOAD_MEDIA_CHUNK);
            assert_eq!(chunk_payload["body"]["upload_id"], "upload-1");
            let chunk_req_id = payload_req_id(&chunk_payload);
            ws.send(Message::Text(
                json!({
                    "cmd": APP_CMD_UPLOAD_MEDIA_CHUNK,
                    "headers": { "req_id": chunk_req_id },
                    "errcode": 0
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("chunk ack");

            let finish_frame = ws.next().await.expect("finish frame").expect("finish");
            let finish_payload: Value =
                serde_json::from_str(finish_frame.into_text().expect("text").as_str())
                    .expect("finish payload");
            assert_eq!(finish_payload["cmd"], APP_CMD_UPLOAD_MEDIA_FINISH);
            let finish_req_id = payload_req_id(&finish_payload);
            ws.send(Message::Text(
                json!({
                    "cmd": APP_CMD_UPLOAD_MEDIA_FINISH,
                    "headers": { "req_id": finish_req_id },
                    "errcode": 0,
                    "body": { "media_id": "media-1", "type": "image" }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("finish ack");

            let media_frame = ws.next().await.expect("media frame").expect("media");
            let media_payload: Value =
                serde_json::from_str(media_frame.into_text().expect("text").as_str())
                    .expect("media payload");
            assert_eq!(media_payload["cmd"], APP_CMD_SEND);
            assert_eq!(media_payload["body"]["msgtype"], "image");
            assert_eq!(media_payload["body"]["image"]["media_id"], "media-1");
            let media_req_id = payload_req_id(&media_payload);
            ws.send(Message::Text(
                json!({
                    "cmd": APP_CMD_SEND,
                    "headers": { "req_id": media_req_id },
                    "errcode": 0
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("media ack");

            let caption_frame = ws.next().await.expect("caption frame").expect("caption");
            let caption_payload: Value =
                serde_json::from_str(caption_frame.into_text().expect("text").as_str())
                    .expect("caption payload");
            assert_eq!(caption_payload["cmd"], APP_CMD_SEND);
            assert_eq!(caption_payload["body"]["msgtype"], "markdown");
            assert_eq!(caption_payload["body"]["markdown"]["content"], "caption");
            let caption_req_id = payload_req_id(&caption_payload);
            ws.send(Message::Text(
                json!({
                    "cmd": APP_CMD_SEND,
                    "headers": { "req_id": caption_req_id },
                    "errcode": 0
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("caption ack");
        });

        let adapter = Arc::new(WeComAdapter {
            bot_id: "bot".into(),
            secret: "secret".into(),
            ws_url: format!("ws://{addr}"),
            allowed_users: Arc::new(HashSet::new()),
            outbound_tx: Arc::new(Mutex::new(None)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            seen_messages: Arc::new(Mutex::new(HashMap::new())),
            reply_req_ids: Arc::new(Mutex::new(HashMap::new())),
        });

        let temp = tempfile::NamedTempFile::new().expect("temp");
        tokio::fs::write(temp.path(), b"image-bytes")
            .await
            .expect("write image");

        let (tx, _rx) = mpsc::channel(4);
        let runner = adapter.clone();
        let run_task = tokio::spawn(async move { runner.start(tx).await });

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if adapter.outbound_tx.lock().await.is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("connection ready");

        adapter
            .send_photo(
                temp.path().to_str().expect("path"),
                Some("caption"),
                &MessageMetadata {
                    channel_id: Some("chat-1".into()),
                    ..Default::default()
                },
            )
            .await
            .expect("send photo");

        run_task.abort();
        let _ = server.await;
    }
}
