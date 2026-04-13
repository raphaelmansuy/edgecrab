//! # Mattermost adapter — REST API + WebSocket
//!
//! Connects to a self-hosted (or cloud) Mattermost instance via its REST
//! API (v4) and WebSocket for real-time events.
//!
//! ## Environment variables
//!
//! | Variable                  | Required | Description                     |
//! |---------------------------|----------|---------------------------------|
//! | `MATTERMOST_URL`          | Yes      | Server URL (e.g. https://mm.co) |
//! | `MATTERMOST_TOKEN`        | Yes      | Bot token or PAT                |
//! | `MATTERMOST_ALLOWED_USERS`| No       | Comma-separated user IDs        |
//!
//! ## Limits
//!
//! - Max message length: **4000** characters

use std::env;
use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use edgecrab_types::Platform;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tracing::{debug, error, info, warn};

use crate::platform::{IncomingMessage, MessageMetadata, OutgoingMessage, PlatformAdapter};

const MAX_MESSAGE_LENGTH: usize = 4000;
const RETRY_DELAY: Duration = crate::ADAPTER_RETRY_DELAY;
const MAX_RETRY_DELAY: Duration = crate::ADAPTER_MAX_RETRY_DELAY;

pub struct MattermostAdapter {
    base_url: String,
    token: String,
    allowed_users: Vec<String>,
    bot_user_id: std::sync::Mutex<String>,
}

impl MattermostAdapter {
    pub fn from_env() -> Option<Self> {
        let base_url = env::var("MATTERMOST_URL")
            .ok()?
            .trim_end_matches('/')
            .to_string();
        let token = env::var("MATTERMOST_TOKEN").ok()?;
        let allowed_users = env::var("MATTERMOST_ALLOWED_USERS")
            .ok()
            .map(|s| s.split(',').map(|u| u.trim().to_string()).collect())
            .unwrap_or_default();

        Some(Self {
            base_url,
            token,
            allowed_users,
            bot_user_id: std::sync::Mutex::new(String::new()),
        })
    }

    pub fn is_available() -> bool {
        env::var("MATTERMOST_URL").is_ok() && env::var("MATTERMOST_TOKEN").is_ok()
    }

    #[allow(dead_code)]
    fn headers(&self) -> Vec<(&str, String)> {
        vec![("Authorization", format!("Bearer {}", self.token))]
    }

    fn ws_url(&self) -> String {
        let url = self
            .base_url
            .replace("https://", "wss://")
            .replace("http://", "ws://");
        format!("{url}/api/v4/websocket")
    }

    async fn get_me(&self) -> anyhow::Result<String> {
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{}/api/v4/users/me", self.base_url))
            .header("Authorization", format!("Bearer {}", self.token))
            .timeout(Duration::from_secs(15))
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await?;
        Ok(json["id"].as_str().unwrap_or("").to_string())
    }

    async fn upload_file(&self, channel_id: &str, path: &str) -> anyhow::Result<String> {
        let file_name = Path::new(path)
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow::anyhow!("Mattermost media path has no file name: {path}"))?;
        let bytes = std::fs::read(path)
            .map_err(|error| anyhow::anyhow!("Mattermost cannot read {path}: {error}"))?;
        let part = reqwest::multipart::Part::bytes(bytes).file_name(file_name.to_string());
        let form = reqwest::multipart::Form::new()
            .text("channel_id", channel_id.to_string())
            .part("files", part);

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/api/v4/files", self.base_url))
            .header("Authorization", format!("Bearer {}", self.token))
            .multipart(form)
            .timeout(Duration::from_secs(60))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Mattermost file upload error {status}: {text}");
        }

        let json: serde_json::Value = resp.json().await?;
        let file_id = json["file_infos"]
            .as_array()
            .and_then(|infos| infos.first())
            .and_then(|info| info["id"].as_str())
            .ok_or_else(|| anyhow::anyhow!("Mattermost upload response missing file id"))?;
        Ok(file_id.to_string())
    }

    async fn send_file_post(
        &self,
        channel_id: &str,
        path: &str,
        caption: Option<&str>,
        thread_id: Option<&str>,
    ) -> anyhow::Result<()> {
        let file_id = self.upload_file(channel_id, path).await?;
        let mut body = serde_json::json!({
            "channel_id": channel_id,
            "message": caption.unwrap_or_default(),
            "file_ids": [file_id],
        });
        if let Some(root_id) = thread_id {
            body["root_id"] = serde_json::json!(root_id);
        }

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/api/v4/posts", self.base_url))
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&body)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Mattermost media post error {status}: {text}");
        }

        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct MmWsEvent {
    event: Option<String>,
    data: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct MmWsAuth {
    seq: u32,
    action: String,
    data: serde_json::Value,
}

#[async_trait]
impl PlatformAdapter for MattermostAdapter {
    fn platform(&self) -> Platform {
        Platform::Mattermost
    }

    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
        info!("Mattermost adapter starting — {}", self.base_url);

        // Get bot user ID
        match self.get_me().await {
            Ok(id) => {
                if let Ok(mut bot_id) = self.bot_user_id.lock() {
                    *bot_id = id.clone();
                }
                debug!("Mattermost bot user ID: {}", id);
            }
            Err(e) => warn!("Failed to get Mattermost bot user ID: {e}"),
        }

        let mut retry_delay = RETRY_DELAY;

        loop {
            let ws_url = self.ws_url();
            debug!("Connecting to Mattermost WebSocket: {}", ws_url);

            match tokio_tungstenite::connect_async(&ws_url).await {
                Ok((ws_stream, _)) => {
                    retry_delay = RETRY_DELAY;
                    let (mut write, mut read) = ws_stream.split();

                    // Authenticate
                    let auth = MmWsAuth {
                        seq: 1,
                        action: "authentication_challenge".into(),
                        data: serde_json::json!({ "token": self.token }),
                    };
                    if let Ok(json) = serde_json::to_string(&auth) {
                        let _ = write.send(WsMessage::Text(json)).await;
                    }

                    while let Some(msg) = read.next().await {
                        match msg {
                            Ok(WsMessage::Text(text)) => {
                                if let Ok(event) = serde_json::from_str::<MmWsEvent>(&text) {
                                    if event.event.as_deref() == Some("posted") {
                                        if let Some(data) = event.data {
                                            self.handle_posted(&tx, &data).await;
                                        }
                                    }
                                }
                            }
                            Ok(WsMessage::Close(_)) => {
                                info!("Mattermost WebSocket closed");
                                break;
                            }
                            Err(e) => {
                                warn!("Mattermost WebSocket error: {e}");
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    warn!("Mattermost WebSocket connect failed: {e}");
                }
            }

            warn!("Mattermost reconnecting in {:?}...", retry_delay);
            tokio::time::sleep(retry_delay).await;
            retry_delay = (retry_delay * 2).min(MAX_RETRY_DELAY);
        }
    }

    async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()> {
        let channel_id = msg
            .metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("No Mattermost channel_id"))?;

        let mut body = serde_json::json!({
            "channel_id": channel_id,
            "message": msg.text,
        });

        // Thread support
        if let Some(ref root_id) = msg.metadata.thread_id {
            body["root_id"] = serde_json::json!(root_id);
        }

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/api/v4/posts", self.base_url))
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&body)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            error!("Mattermost send error {}: {}", status, text);
            anyhow::bail!("Mattermost send error {status}");
        }

        debug!("Mattermost message sent to {}", channel_id);
        Ok(())
    }

    fn format_response(&self, text: &str, _metadata: &MessageMetadata) -> String {
        // Mattermost supports standard Markdown
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
        let channel_id = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("No Mattermost channel_id"))?;
        self.send_file_post(channel_id, path, caption, metadata.thread_id.as_deref())
            .await
    }

    async fn send_document(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        let channel_id = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("No Mattermost channel_id"))?;
        self.send_file_post(channel_id, path, caption, metadata.thread_id.as_deref())
            .await
    }
}

impl MattermostAdapter {
    async fn handle_posted(&self, tx: &mpsc::Sender<IncomingMessage>, data: &serde_json::Value) {
        let post_str = data["post"].as_str().unwrap_or("{}");
        let post: serde_json::Value = match serde_json::from_str(post_str) {
            Ok(p) => p,
            Err(_) => return,
        };

        let user_id = post["user_id"].as_str().unwrap_or("");
        let bot_id = self
            .bot_user_id
            .lock()
            .map(|id| id.clone())
            .unwrap_or_default();
        if user_id == bot_id || user_id.is_empty() {
            return;
        }

        if !self.allowed_users.is_empty() && !self.allowed_users.contains(&user_id.to_string()) {
            debug!("Mattermost message from unauthorized user: {}", user_id);
            return;
        }

        let message = post["message"].as_str().unwrap_or("");
        if message.is_empty() {
            return;
        }

        let channel_id = post["channel_id"].as_str().unwrap_or("").to_string();
        let post_id = post["id"].as_str().unwrap_or("").to_string();
        let root_id = post["root_id"]
            .as_str()
            .map(String::from)
            .filter(|s| !s.is_empty());

        let incoming = IncomingMessage {
            platform: Platform::Mattermost,
            user_id: user_id.to_string(),
            channel_id: Some(channel_id.clone()),
            chat_type: crate::platform::ChatType::Group,
            text: message.to_string(),
            thread_id: root_id.clone(),
            metadata: MessageMetadata {
                message_id: Some(post_id),
                channel_id: Some(channel_id),
                thread_id: root_id,
                user_display_name: data["sender_name"].as_str().map(String::from),
                attachments: Vec::new(),
                ..Default::default()
            },
        };

        if tx.send(incoming).await.is_err() {
            warn!("Mattermost message channel closed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mattermost_max_length() {
        assert_eq!(MAX_MESSAGE_LENGTH, 4000);
    }
}
