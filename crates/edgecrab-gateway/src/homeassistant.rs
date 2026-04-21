//! # Home Assistant adapter — WebSocket + REST API
//!
//! Listens for `conversation_chat` events over the HA WebSocket API and
//! responds via the REST API.
//!
//! ## Environment variables
//!
//! | Variable           | Required | Description                                 |
//! |--------------------|----------|---------------------------------------------|
//! | `HA_URL`           | Yes      | Home Assistant URL (e.g. http://ha:8123)    |
//! | `HA_TOKEN`         | Yes      | Long-lived access token                      |
//! | `HA_ALLOWED_USERS` | No       | Comma-separated user IDs                     |
//!
//! ## Limits
//!
//! - Max message length: **10000** characters (HA has no strict limit)

use std::env;
use std::time::Duration;

use async_trait::async_trait;
use edgecrab_types::Platform;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tracing::{debug, error, info, warn};

use crate::platform::{IncomingMessage, MessageMetadata, OutgoingMessage, PlatformAdapter};

const MAX_MESSAGE_LENGTH: usize = 10000;
const RETRY_DELAY: Duration = crate::ADAPTER_RETRY_DELAY;
const MAX_RETRY_DELAY: Duration = crate::ADAPTER_MAX_RETRY_DELAY;

pub struct HomeAssistantAdapter {
    base_url: String,
    token: String,
    allowed_users: Vec<String>,
}

impl HomeAssistantAdapter {
    pub fn from_env() -> Option<Self> {
        let base_url = env::var("HA_URL").ok()?.trim_end_matches('/').to_string();
        let token = env::var("HA_TOKEN").ok()?;
        let allowed_users = env::var("HA_ALLOWED_USERS")
            .ok()
            .map(|s| s.split(',').map(|u| u.trim().to_string()).collect())
            .unwrap_or_default();

        Some(Self {
            base_url,
            token,
            allowed_users,
        })
    }

    pub fn is_available() -> bool {
        env::var("HA_URL").is_ok() && env::var("HA_TOKEN").is_ok()
    }

    fn ws_url(&self) -> String {
        let url = self
            .base_url
            .replace("https://", "wss://")
            .replace("http://", "ws://");
        format!("{url}/api/websocket")
    }
}

#[derive(Debug, Serialize)]
struct HaWsMsg {
    id: u64,
    #[serde(rename = "type")]
    msg_type: String,
    #[serde(flatten)]
    extra: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct HaWsResponse {
    #[serde(rename = "type")]
    msg_type: Option<String>,
    #[allow(dead_code)]
    id: Option<u64>,
    event: Option<serde_json::Value>,
}

#[async_trait]
impl PlatformAdapter for HomeAssistantAdapter {
    fn platform(&self) -> Platform {
        Platform::HomeAssistant
    }

    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
        info!("Home Assistant adapter starting — {}", self.base_url);

        let mut retry_delay = RETRY_DELAY;

        loop {
            let ws_url = self.ws_url();
            debug!("Connecting to HA WebSocket: {}", ws_url);

            match tokio_tungstenite::connect_async(&ws_url).await {
                Ok((ws_stream, _)) => {
                    retry_delay = RETRY_DELAY;
                    let (mut write, mut read) = ws_stream.split();
                    let mut msg_id: u64 = 0;

                    // Wait for auth_required
                    while let Some(Ok(msg)) = read.next().await {
                        if let WsMessage::Text(text) = msg
                            && text.contains("auth_required")
                        {
                            break;
                        }
                    }

                    // Authenticate
                    let auth = serde_json::json!({
                        "type": "auth",
                        "access_token": self.token,
                    });
                    if let Ok(json) = serde_json::to_string(&auth) {
                        let _ = write.send(WsMessage::Text(json)).await;
                    }

                    // Wait for auth_ok
                    let mut authenticated = false;
                    while let Some(Ok(msg)) = read.next().await {
                        if let WsMessage::Text(text) = msg {
                            if text.contains("auth_ok") {
                                authenticated = true;
                                info!("Home Assistant authenticated");
                                break;
                            } else if text.contains("auth_invalid") {
                                error!("Home Assistant auth failed");
                                break;
                            }
                        }
                    }

                    if !authenticated {
                        warn!("HA auth failed, retrying...");
                        tokio::time::sleep(retry_delay).await;
                        retry_delay = (retry_delay * 2).min(MAX_RETRY_DELAY);
                        continue;
                    }

                    // Subscribe to conversation events
                    msg_id += 1;
                    let sub = HaWsMsg {
                        id: msg_id,
                        msg_type: "subscribe_events".into(),
                        extra: serde_json::json!({
                            "event_type": "conversation_chat",
                        }),
                    };
                    if let Ok(json) = serde_json::to_string(&sub) {
                        let _ = write.send(WsMessage::Text(json)).await;
                    }

                    // Process events
                    while let Some(msg) = read.next().await {
                        match msg {
                            Ok(WsMessage::Text(text)) => {
                                if let Ok(resp) = serde_json::from_str::<HaWsResponse>(&text)
                                    && resp.msg_type.as_deref() == Some("event")
                                    && let Some(event) = resp.event
                                {
                                    self.handle_event(&tx, &event).await;
                                }
                            }
                            Ok(WsMessage::Close(_)) => {
                                info!("HA WebSocket closed");
                                break;
                            }
                            Err(e) => {
                                warn!("HA WebSocket error: {e}");
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    warn!("HA WebSocket connect failed: {e}");
                }
            }

            warn!("Home Assistant reconnecting in {:?}...", retry_delay);
            tokio::time::sleep(retry_delay).await;
            retry_delay = (retry_delay * 2).min(MAX_RETRY_DELAY);
        }
    }

    async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()> {
        let client = reqwest::Client::new();

        // Use the HA REST API to send a response
        let body = serde_json::json!({
            "message": msg.text,
        });

        // If we have a conversation_id, use the conversation API
        if let Some(ref conv_id) = msg.metadata.channel_id {
            let resp = client
                .post(format!("{}/api/conversation/process", self.base_url))
                .header("Authorization", format!("Bearer {}", self.token))
                .header("Content-Type", "application/json")
                .json(&serde_json::json!({
                    "text": msg.text,
                    "conversation_id": conv_id,
                }))
                .timeout(Duration::from_secs(30))
                .send()
                .await?;

            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                error!("HA send error {}: {}", status, text);
                anyhow::bail!("HA send error {status}");
            }
        } else {
            // Fire a custom event so HA automations can pick it up
            let resp = client
                .post(format!("{}/api/events/edgecrab_response", self.base_url))
                .header("Authorization", format!("Bearer {}", self.token))
                .header("Content-Type", "application/json")
                .json(&body)
                .timeout(Duration::from_secs(15))
                .send()
                .await?;

            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                error!("HA event fire error {}: {}", status, text);
                anyhow::bail!("HA event fire error {status}");
            }
        }

        debug!("Home Assistant message sent");
        Ok(())
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
        false
    }

    fn supports_files(&self) -> bool {
        false
    }
}

impl HomeAssistantAdapter {
    async fn handle_event(&self, tx: &mpsc::Sender<IncomingMessage>, event: &serde_json::Value) {
        let data = &event["data"];
        let user_id = data["user_id"].as_str().unwrap_or("");
        let text = data["text"].as_str().unwrap_or("");
        let conversation_id = data["conversation_id"].as_str().map(String::from);

        if text.is_empty() {
            return;
        }

        if !self.allowed_users.is_empty() && !self.allowed_users.contains(&user_id.to_string()) {
            debug!("HA: ignoring message from unauthorized user: {}", user_id);
            return;
        }

        let incoming = IncomingMessage {
            platform: Platform::HomeAssistant,
            user_id: user_id.to_string(),
            channel_id: conversation_id.clone(),
            chat_type: crate::platform::ChatType::Dm,
            text: text.to_string(),
            thread_id: None,
            metadata: MessageMetadata {
                message_id: None,
                channel_id: conversation_id,
                thread_id: None,
                user_display_name: data["user_name"].as_str().map(String::from),
                attachments: Vec::new(),
                ..Default::default()
            },
        };

        if tx.send(incoming).await.is_err() {
            warn!("HA message channel closed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ha_max_length() {
        assert_eq!(MAX_MESSAGE_LENGTH, 10000);
    }
}
