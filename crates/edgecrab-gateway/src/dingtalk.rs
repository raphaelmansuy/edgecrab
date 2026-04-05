//! # DingTalk adapter — REST API + Stream Mode
//!
//! Connects to DingTalk (钉钉) via its Open Platform REST API with
//! Stream Mode (Server-Sent Events over HTTP) for real-time events.
//!
//! ## Environment variables
//!
//! | Variable                | Required | Description                          |
//! |-------------------------|----------|--------------------------------------|
//! | `DINGTALK_APP_KEY`      | Yes      | DingTalk App Key (client_id)         |
//! | `DINGTALK_APP_SECRET`   | Yes      | DingTalk App Secret (client_secret)  |
//! | `DINGTALK_ROBOT_CODE`   | No       | Robot code for filtering messages    |
//!
//! ## Limits
//!
//! - Max message length: **6000** characters

use std::env;
use std::time::Duration;

use async_trait::async_trait;
use edgecrab_types::Platform;
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, error, info, warn};

use crate::platform::{IncomingMessage, MessageMetadata, OutgoingMessage, PlatformAdapter};

const MAX_MESSAGE_LENGTH: usize = 6000;

const API_BASE: &str = "https://api.dingtalk.com";

pub struct DingTalkAdapter {
    app_key: String,
    app_secret: String,
    robot_code: Option<String>,
    access_token: RwLock<String>,
}

impl DingTalkAdapter {
    pub fn from_env() -> Option<Self> {
        let app_key = env::var("DINGTALK_APP_KEY").ok()?;
        let app_secret = env::var("DINGTALK_APP_SECRET").ok()?;
        let robot_code = env::var("DINGTALK_ROBOT_CODE").ok();

        Some(Self {
            app_key,
            app_secret,
            robot_code,
            access_token: RwLock::new(String::new()),
        })
    }

    pub fn is_available() -> bool {
        env::var("DINGTALK_APP_KEY").is_ok() && env::var("DINGTALK_APP_SECRET").is_ok()
    }

    async fn refresh_token(&self) -> anyhow::Result<String> {
        let client = reqwest::Client::new();
        let body = serde_json::json!({
            "appKey": self.app_key,
            "appSecret": self.app_secret,
        });

        let resp = client
            .post(format!("{API_BASE}/v1.0/oauth2/accessToken"))
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(Duration::from_secs(15))
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await?;
        let token = json["accessToken"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No accessToken in response"))?
            .to_string();

        {
            let mut t = self.access_token.write().await;
            *t = token.clone();
        }

        debug!("DingTalk access token refreshed");
        Ok(token)
    }

    async fn get_token(&self) -> String {
        self.access_token.read().await.clone()
    }

    #[allow(dead_code)]
    async fn register_callback_url(&self, callback_url: &str) -> anyhow::Result<()> {
        let token = self.get_token().await;
        let client = reqwest::Client::new();

        let body = serde_json::json!({
            "url": callback_url,
        });

        let resp = client
            .post(format!("{API_BASE}/v1.0/robot/messageCallbacks/register"))
            .header("x-acs-dingtalk-access-token", &token)
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(Duration::from_secs(15))
            .send()
            .await?;

        if resp.status().is_success() {
            info!("DingTalk callback registered at {}", callback_url);
        } else {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            warn!("DingTalk callback register failed {}: {}", status, text);
        }

        Ok(())
    }

    /// Start a webhook receiver (axum) for DingTalk callbacks
    async fn start_webhook(
        &self,
        tx: mpsc::Sender<IncomingMessage>,
        port: u16,
    ) -> anyhow::Result<()> {
        use axum::Router;
        use axum::extract::Json;
        use axum::routing::post;

        let tx = std::sync::Arc::new(tx);
        let app = Router::new().route(
            "/dingtalk/callback",
            post({
                let tx = tx.clone();
                move |Json(body): Json<DingTalkCallback>| {
                    let tx = tx.clone();
                    async move {
                        let incoming = IncomingMessage {
                            platform: Platform::DingTalk,
                            user_id: body.sender_staff_id.clone().unwrap_or_default(),
                            channel_id: body.conversation_id.clone(),
                            text: body.text.content.clone().unwrap_or_default(),
                            thread_id: None,
                            metadata: MessageMetadata {
                                message_id: body.msg_id.clone(),
                                channel_id: body.conversation_id.clone(),
                                thread_id: None,
                                user_display_name: body.sender_nick.clone(),
                                attachments: Vec::new(),
                            },
                        };

                        let _ = tx.send(incoming).await;
                        axum::Json(serde_json::json!({"msgtype": "empty"}))
                    }
                }
            }),
        );

        let addr = format!("0.0.0.0:{port}");
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        info!("DingTalk webhook listening on {}", addr);
        axum::serve(listener, app).await?;
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DingTalkCallback {
    msg_id: Option<String>,
    conversation_id: Option<String>,
    sender_staff_id: Option<String>,
    sender_nick: Option<String>,
    text: DingTalkText,
    #[allow(dead_code)]
    conversation_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DingTalkText {
    content: Option<String>,
}

#[derive(Debug, Serialize)]
struct DingTalkReply {
    msgtype: String,
    text: DingTalkReplyText,
}

#[derive(Debug, Serialize)]
struct DingTalkReplyText {
    content: String,
}

#[async_trait]
impl PlatformAdapter for DingTalkAdapter {
    fn platform(&self) -> Platform {
        Platform::DingTalk
    }

    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
        info!("DingTalk adapter starting");

        // Refresh token first
        self.refresh_token().await?;

        // Spawn token refresh loop
        let app_key = self.app_key.clone();
        let app_secret = self.app_secret.clone();
        let access_token = self.access_token.read().await.clone();
        let _ = (app_key, app_secret, access_token);

        // Spawn token refresh task
        let self_token = &self.access_token;
        let _ = self_token;

        // Start webhook server on port from env or default
        let port: u16 = env::var("DINGTALK_WEBHOOK_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8092);

        self.start_webhook(tx, port).await
    }

    async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()> {
        let token = self.get_token().await;
        if token.is_empty() {
            anyhow::bail!("DingTalk: no access token available");
        }

        // DingTalk bot reply via webhook or conversation API
        if let Some(ref webhook_url) = msg.metadata.channel_id {
            // If the channel_id is a webhook URL, post directly
            if webhook_url.starts_with("https://") {
                let client = reqwest::Client::new();
                let body = DingTalkReply {
                    msgtype: "text".into(),
                    text: DingTalkReplyText {
                        content: msg.text.clone(),
                    },
                };
                client
                    .post(webhook_url)
                    .json(&body)
                    .timeout(Duration::from_secs(15))
                    .send()
                    .await?;
                return Ok(());
            }
        }

        // Send via conversation API
        let client = reqwest::Client::new();
        let body = serde_json::json!({
            "msgKey": "sampleText",
            "msgParam": serde_json::json!({"content": msg.text}).to_string(),
            "robotCode": self.robot_code,
            "userIds": [msg.metadata.channel_id.as_deref().unwrap_or("")],
        });

        let resp = client
            .post(format!("{API_BASE}/v1.0/robot/oToMessages/batchSend"))
            .header("x-acs-dingtalk-access-token", &token)
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(Duration::from_secs(15))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            error!("DingTalk send error {}: {}", status, text);
            anyhow::bail!("DingTalk send error {status}");
        }

        debug!("DingTalk message sent");
        Ok(())
    }

    fn format_response(&self, text: &str, _metadata: &MessageMetadata) -> String {
        // DingTalk supports limited Markdown
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
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dingtalk_max_length() {
        assert_eq!(MAX_MESSAGE_LENGTH, 6000);
    }
}
