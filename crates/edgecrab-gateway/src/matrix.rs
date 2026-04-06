//! # Matrix adapter — Matrix homeserver via Client-Server API
//!
//! Connects to any Matrix homeserver via the Client-Server REST API.
//! Uses long-poll sync for receiving messages and REST for sending.
//!
//! ## Environment variables
//!
//! | Variable               | Required | Description                            |
//! |------------------------|----------|----------------------------------------|
//! | `MATRIX_HOMESERVER`    | Yes      | Homeserver URL (e.g. https://matrix.org)|
//! | `MATRIX_ACCESS_TOKEN`  | Yes*     | Access token (preferred auth)          |
//! | `MATRIX_USER_ID`       | No       | Full user ID (@bot:server)             |
//! | `MATRIX_ALLOWED_USERS` | No       | Comma-separated Matrix user IDs        |
//!
//! ## Limits
//!
//! - Max message length: **4000** characters

use std::env;
use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use edgecrab_types::Platform;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::platform::{IncomingMessage, MessageMetadata, OutgoingMessage, PlatformAdapter};

const MAX_MESSAGE_LENGTH: usize = 4000;
const SYNC_TIMEOUT_MS: u64 = 30000;
const RETRY_DELAY: Duration = crate::ADAPTER_RETRY_DELAY;

pub struct MatrixAdapter {
    homeserver: String,
    access_token: String,
    user_id: String,
    allowed_users: Vec<String>,
}

impl MatrixAdapter {
    pub fn from_env() -> Option<Self> {
        let homeserver = env::var("MATRIX_HOMESERVER")
            .ok()?
            .trim_end_matches('/')
            .to_string();
        let access_token = env::var("MATRIX_ACCESS_TOKEN").ok()?;
        let user_id = env::var("MATRIX_USER_ID").ok().unwrap_or_default();
        let allowed_users = env::var("MATRIX_ALLOWED_USERS")
            .ok()
            .map(|s| s.split(',').map(|u| u.trim().to_string()).collect())
            .unwrap_or_default();

        Some(Self {
            homeserver,
            access_token,
            user_id,
            allowed_users,
        })
    }

    pub fn is_available() -> bool {
        env::var("MATRIX_HOMESERVER").is_ok() && env::var("MATRIX_ACCESS_TOKEN").is_ok()
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.access_token)
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}/_matrix/client/v3{}", self.homeserver, path)
    }

    fn media_upload_url(&self, file_name: &str) -> String {
        format!(
            "{}/_matrix/media/v3/upload?filename={}",
            self.homeserver,
            urlencoding::encode(file_name)
        )
    }

    async fn upload_media(&self, path: &str) -> anyhow::Result<(String, String, u64)> {
        let file_name = Path::new(path)
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow::anyhow!("Matrix media path has no file name: {path}"))?
            .to_string();
        let bytes = std::fs::read(path)
            .map_err(|error| anyhow::anyhow!("Matrix cannot read {path}: {error}"))?;
        let content_type = matrix_content_type(&file_name);

        let client = reqwest::Client::new();
        let resp = client
            .post(self.media_upload_url(&file_name))
            .header("Authorization", self.auth_header())
            .header("Content-Type", content_type)
            .body(bytes.clone())
            .timeout(Duration::from_secs(60))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Matrix media upload error {status}: {text}");
        }

        let json: serde_json::Value = resp.json().await?;
        let content_uri = json["content_uri"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Matrix media upload response missing content_uri"))?;
        Ok((
            content_uri.to_string(),
            content_type.to_string(),
            bytes.len() as u64,
        ))
    }

    async fn send_media_message(
        &self,
        room_id: &str,
        path: &str,
        caption: Option<&str>,
        msgtype: &str,
    ) -> anyhow::Result<()> {
        let file_name = Path::new(path)
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow::anyhow!("Matrix media path has no file name: {path}"))?;
        let (content_uri, content_type, size) = self.upload_media(path).await?;
        let txn_id = uuid::Uuid::new_v4().to_string();
        let encoded_room = percent_encode_room_id(room_id);
        let url = format!(
            "{}/rooms/{}/send/m.room.message/{}",
            self.api_url(""),
            encoded_room,
            txn_id
        );

        let body = serde_json::json!({
            "msgtype": msgtype,
            "body": caption.unwrap_or(file_name),
            "filename": file_name,
            "url": content_uri,
            "info": {
                "mimetype": content_type,
                "size": size,
            }
        });

        let client = reqwest::Client::new();
        let resp = client
            .put(&url)
            .header("Authorization", self.auth_header())
            .json(&body)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Matrix media send error {status}: {text}");
        }

        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct SyncResponse {
    next_batch: Option<String>,
    rooms: Option<SyncRooms>,
}

#[derive(Debug, Deserialize)]
struct SyncRooms {
    join: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Serialize)]
struct MatrixMessageBody {
    msgtype: String,
    body: String,
}

fn percent_encode_room_id(room_id: &str) -> String {
    room_id
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                format!("{}", b as char)
            }
            _ => format!("%{:02X}", b),
        })
        .collect::<String>()
}

fn matrix_content_type(file_name: &str) -> &'static str {
    match Path::new(file_name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "json" => "application/json",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
}

#[async_trait]
impl PlatformAdapter for MatrixAdapter {
    fn platform(&self) -> Platform {
        Platform::Matrix
    }

    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
        info!("Matrix adapter starting — {}", self.homeserver);

        let client = reqwest::Client::new();
        let mut since: Option<String> = None;

        // Initial sync to get the since token (skip historical messages)
        let url = format!(
            "{}?timeout=0&filter={{\"room\":{{\"timeline\":{{\"limit\":0}}}}}}",
            self.api_url("/sync")
        );
        match client
            .get(&url)
            .header("Authorization", self.auth_header())
            .timeout(Duration::from_secs(30))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(sync) = resp.json::<SyncResponse>().await {
                    since = sync.next_batch;
                }
            }
            Ok(resp) => {
                warn!("Matrix initial sync failed: {}", resp.status());
            }
            Err(e) => {
                warn!("Matrix initial sync error: {e}");
            }
        }

        loop {
            let mut url = format!("{}?timeout={SYNC_TIMEOUT_MS}", self.api_url("/sync"));
            if let Some(ref token) = since {
                url.push_str(&format!("&since={token}"));
            }

            match client
                .get(&url)
                .header("Authorization", self.auth_header())
                .timeout(Duration::from_secs(60))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(sync) = resp.json::<SyncResponse>().await {
                        since = sync.next_batch.clone().or(since);

                        if let Some(rooms) = sync.rooms {
                            if let Some(joined) = rooms.join {
                                for (room_id, room_data) in joined {
                                    if let Some(timeline) = room_data.get("timeline") {
                                        if let Some(events) = timeline.get("events") {
                                            if let Some(events) = events.as_array() {
                                                for event in events {
                                                    self.handle_event(&tx, &room_id, event).await;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(resp) => {
                    warn!("Matrix sync error: {}", resp.status());
                    tokio::time::sleep(RETRY_DELAY).await;
                }
                Err(e) => {
                    warn!("Matrix sync request failed: {e}");
                    tokio::time::sleep(RETRY_DELAY).await;
                }
            }
        }
    }

    async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()> {
        let room_id = msg
            .metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("No Matrix room_id"))?;

        let txn_id = uuid::Uuid::new_v4().to_string();
        let encoded_room = percent_encode_room_id(room_id);
        let url = format!(
            "{}/rooms/{}/send/m.room.message/{}",
            self.api_url(""),
            encoded_room,
            txn_id
        );

        let body = MatrixMessageBody {
            msgtype: "m.text".into(),
            body: msg.text.clone(),
        };

        let client = reqwest::Client::new();
        let resp = client
            .put(&url)
            .header("Authorization", self.auth_header())
            .json(&body)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            error!("Matrix send error {}: {}", status, text);
            anyhow::bail!("Matrix send error {status}");
        }

        debug!("Matrix message sent to {}", room_id);
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
        let room_id = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("No Matrix room_id"))?;
        self.send_media_message(room_id, path, caption, "m.image")
            .await
    }

    async fn send_document(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        let room_id = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("No Matrix room_id"))?;
        self.send_media_message(room_id, path, caption, "m.file")
            .await
    }
}

impl MatrixAdapter {
    async fn handle_event(
        &self,
        tx: &mpsc::Sender<IncomingMessage>,
        room_id: &str,
        event: &serde_json::Value,
    ) {
        let event_type = event["type"].as_str().unwrap_or("");
        if event_type != "m.room.message" {
            return;
        }

        let sender = event["sender"].as_str().unwrap_or("");
        // Skip own messages
        if sender == self.user_id {
            return;
        }

        // Check allowed users
        if !self.allowed_users.is_empty() && !self.allowed_users.contains(&sender.to_string()) {
            debug!("Matrix message from unauthorized user: {}", sender);
            return;
        }

        let body = event["content"]["body"].as_str().unwrap_or("");
        if body.is_empty() {
            return;
        }

        let event_id = event["event_id"].as_str().unwrap_or("").to_string();

        let incoming = IncomingMessage {
            platform: Platform::Matrix,
            user_id: sender.to_string(),
            channel_id: Some(room_id.to_string()),
            text: body.to_string(),
            thread_id: None,
            metadata: MessageMetadata {
                message_id: Some(event_id),
                channel_id: Some(room_id.to_string()),
                user_display_name: event["content"]["displayname"].as_str().map(String::from),
                ..Default::default()
            },
        };

        if tx.send(incoming).await.is_err() {
            warn!("Matrix message channel closed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_max_length() {
        assert_eq!(MAX_MESSAGE_LENGTH, 4000);
    }

    #[test]
    fn matrix_no_markdown() {
        let adapter = MatrixAdapter {
            homeserver: "https://matrix.org".into(),
            access_token: "test".into(),
            user_id: "@bot:matrix.org".into(),
            allowed_users: vec![],
        };
        assert!(!adapter.supports_markdown());
    }
}
