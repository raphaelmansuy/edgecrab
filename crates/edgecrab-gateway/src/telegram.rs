//! # Telegram Bot API adapter
//!
//! Connects to the Telegram Bot API using long-polling (`getUpdates`) to
//! receive messages and the `sendMessage` endpoint to respond.
//!
//! ```text
//!   TelegramAdapter
//!     ├── start()           → getUpdates long-poll loop
//!     ├── send()            → sendMessage with MarkdownV2
//!     ├── format_response() → escape for MarkdownV2
//!     └── is_available()    → checks TELEGRAM_BOT_TOKEN
//! ```
//!
//! ## Environment variables
//!
//! | Variable             | Required | Description                 |
//! |----------------------|----------|-----------------------------|
//! | `TELEGRAM_BOT_TOKEN` | Yes      | Bot token from @BotFather   |
//!
//! ## Limits
//!
//! - Max message length: **4096** characters
//! - Supports MarkdownV2 formatting
//! - Handles groups, threads/topics via `message_thread_id`

use std::env;
use std::time::Duration;

use async_trait::async_trait;
use edgecrab_types::Platform;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::delivery::split_message;
use crate::platform::{
    IncomingMessage, MessageAttachment, MessageAttachmentKind, MessageMetadata, OutgoingMessage,
    PlatformAdapter,
};

/// Maximum message length for Telegram messages.
const MAX_MESSAGE_LENGTH: usize = 4096;

/// Default long-poll timeout in seconds.
const LONG_POLL_TIMEOUT: u64 = 30;

/// Delay between retries after a network error (alias of `crate::ADAPTER_RETRY_DELAY`).
const RETRY_DELAY: Duration = crate::ADAPTER_RETRY_DELAY;

/// Telegram Bot API base URL.
const TELEGRAM_API_BASE: &str = "https://api.telegram.org";

// ---------------------------------------------------------------------------
// API types (subset of the Telegram Bot API)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct TelegramResponse<T> {
    ok: bool,
    #[serde(default)]
    description: Option<String>,
    result: Option<T>,
}

fn is_thread_not_found(description: &str) -> bool {
    let normalized = description.trim().to_ascii_lowercase();
    normalized.contains("thread not found") || normalized.contains("message thread not found")
}

#[derive(Debug, Deserialize)]
struct Update {
    update_id: i64,
    message: Option<TelegramMessage>,
}

#[derive(Debug, Deserialize)]
struct TelegramMessage {
    message_id: i64,
    from: Option<TelegramUser>,
    chat: TelegramChat,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    caption: Option<String>,
    #[serde(default)]
    photo: Vec<TelegramPhotoSize>,
    #[serde(default)]
    document: Option<TelegramFileHandle>,
    #[serde(default)]
    audio: Option<TelegramFileHandle>,
    #[serde(default)]
    video: Option<TelegramFileHandle>,
    #[serde(default)]
    animation: Option<TelegramFileHandle>,
    #[serde(default)]
    voice: Option<TelegramFileHandle>,
    #[serde(default)]
    sticker: Option<TelegramSticker>,
    #[serde(default)]
    message_thread_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TelegramPhotoSize {
    file_id: String,
    #[serde(default)]
    file_size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct TelegramFileHandle {
    file_id: String,
    #[serde(default)]
    file_name: Option<String>,
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default)]
    file_size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct TelegramSticker {
    file_id: String,
    #[serde(default)]
    emoji: Option<String>,
    #[serde(default)]
    is_animated: Option<bool>,
    #[serde(default)]
    is_video: Option<bool>,
    #[serde(default)]
    file_size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct TelegramFileInfo {
    #[serde(default)]
    file_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramUser {
    id: i64,
    first_name: String,
    #[serde(default)]
    last_name: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    username: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramChat {
    id: i64,
    #[allow(dead_code)]
    #[serde(rename = "type")]
    chat_type: String,
}

#[derive(Debug, Serialize)]
struct SendMessageRequest<'a> {
    chat_id: &'a str,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_mode: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to_message_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_thread_id: Option<i64>,
}

// ---------------------------------------------------------------------------
// TelegramAdapter
// ---------------------------------------------------------------------------

/// Telegram Bot API adapter.
///
/// Uses long-polling (`getUpdates`) to receive messages and `sendMessage`
/// to deliver responses. Supports MarkdownV2 formatting, group chats,
/// and threaded topics.
pub struct TelegramAdapter {
    /// Bot token from @BotFather
    token: String,
    /// Pre-built HTTP client (reused across requests)
    client: reqwest::Client,
    /// Telegram user IDs allowed to message the agent (empty = open access).
    allowed_users: Vec<String>,
}

impl TelegramAdapter {
    /// Create a new Telegram adapter from environment variables.
    ///
    /// # Errors
    /// Returns an error if `TELEGRAM_BOT_TOKEN` is not set.
    pub fn new() -> anyhow::Result<Self> {
        let token = env::var("TELEGRAM_BOT_TOKEN")
            .map_err(|_| anyhow::anyhow!("TELEGRAM_BOT_TOKEN environment variable not set"))?;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(LONG_POLL_TIMEOUT + 10))
            .build()?;

        Ok(Self {
            token,
            client,
            allowed_users: Vec::new(),
        })
    }

    /// Create a Telegram adapter directly from a token + allowed user list.
    pub fn from_token(token: String, allowed_users: Vec<String>) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(LONG_POLL_TIMEOUT + 10))
            .build()?;
        Ok(Self {
            token,
            client,
            allowed_users,
        })
    }

    /// Check whether the adapter can be activated.
    pub fn is_available() -> bool {
        env::var("TELEGRAM_BOT_TOKEN").is_ok()
    }

    /// Build the full API URL for a given method.
    fn api_url(&self, method: &str) -> String {
        format!("{}/bot{}/{}", TELEGRAM_API_BASE, self.token, method)
    }

    /// Fetch updates starting from `offset`.
    async fn get_updates(&self, offset: i64) -> anyhow::Result<Vec<Update>> {
        let url = self.api_url("getUpdates");
        let resp: TelegramResponse<Vec<Update>> = self
            .client
            .get(&url)
            .query(&[
                ("offset", offset.to_string()),
                ("timeout", LONG_POLL_TIMEOUT.to_string()),
            ])
            .send()
            .await?
            .json()
            .await?;

        if !resp.ok {
            anyhow::bail!(
                "Telegram getUpdates failed: {}",
                resp.description.unwrap_or_default()
            );
        }

        Ok(resp.result.unwrap_or_default())
    }

    /// Send a text message to a chat.
    async fn send_message(
        &self,
        chat_id: &str,
        text: &str,
        reply_to: Option<i64>,
        thread_id: Option<i64>,
        use_markdown: bool,
    ) -> anyhow::Result<()> {
        let url = self.api_url("sendMessage");
        for candidate_thread_id in [thread_id, None] {
            if candidate_thread_id.is_none() && thread_id.is_none() {
                continue;
            }

            let body = SendMessageRequest {
                chat_id,
                text,
                parse_mode: if use_markdown {
                    Some("MarkdownV2")
                } else {
                    None
                },
                reply_to_message_id: reply_to,
                message_thread_id: candidate_thread_id,
            };

            let resp: TelegramResponse<serde_json::Value> = self
                .client
                .post(&url)
                .json(&body)
                .send()
                .await?
                .json()
                .await?;

            if resp.ok {
                return Ok(());
            }

            let description = resp.description.unwrap_or_default();
            if candidate_thread_id.is_some() && is_thread_not_found(&description) {
                warn!(
                    thread_id = ?candidate_thread_id,
                    "Telegram thread not found, retrying without message_thread_id"
                );
                continue;
            }

            if use_markdown {
                warn!("MarkdownV2 send failed, retrying as plain text");
                let plain = SendMessageRequest {
                    chat_id,
                    text,
                    parse_mode: None,
                    reply_to_message_id: reply_to,
                    message_thread_id: candidate_thread_id,
                };
                let retry: TelegramResponse<serde_json::Value> = self
                    .client
                    .post(&url)
                    .json(&plain)
                    .send()
                    .await?
                    .json()
                    .await?;
                if retry.ok {
                    return Ok(());
                }

                let retry_description = retry.description.unwrap_or_default();
                if candidate_thread_id.is_some() && is_thread_not_found(&retry_description) {
                    warn!(
                        thread_id = ?candidate_thread_id,
                        "Telegram plain-text retry failed because thread is missing, retrying without message_thread_id"
                    );
                    continue;
                }

                anyhow::bail!("Telegram sendMessage failed: {}", retry_description);
            }

            anyhow::bail!("Telegram sendMessage failed: {}", description);
        }

        anyhow::bail!("Telegram sendMessage failed: thread not found")
    }

    /// Send a text message to a chat and return the sent message_id.
    ///
    /// WHY: Used by the streaming path to obtain the message ID for subsequent
    /// `editMessageText` calls.  The standard `send_message()` discards the ID
    /// because most delivery paths don't need it.
    async fn send_message_with_id(
        &self,
        chat_id: &str,
        text: &str,
        reply_to: Option<i64>,
        thread_id: Option<i64>,
        use_markdown: bool,
    ) -> anyhow::Result<i64> {
        let url = self.api_url("sendMessage");
        for candidate_thread_id in [thread_id, None] {
            if candidate_thread_id.is_none() && thread_id.is_none() {
                continue;
            }

            let body = SendMessageRequest {
                chat_id,
                text,
                parse_mode: if use_markdown {
                    Some("MarkdownV2")
                } else {
                    None
                },
                reply_to_message_id: reply_to,
                message_thread_id: candidate_thread_id,
            };

            let resp: TelegramResponse<TelegramMessage> = self
                .client
                .post(&url)
                .json(&body)
                .send()
                .await?
                .json()
                .await?;

            if resp.ok {
                return resp.result.map(|m| m.message_id).ok_or_else(|| {
                    anyhow::anyhow!("Telegram sendMessage: response missing result")
                });
            }

            let description = resp.description.unwrap_or_default();
            if candidate_thread_id.is_some() && is_thread_not_found(&description) {
                warn!(
                    thread_id = ?candidate_thread_id,
                    "Telegram thread not found, retrying without message_thread_id"
                );
                continue;
            }

            if use_markdown {
                warn!("MarkdownV2 sendMessage failed, retrying as plain text");
                let plain = SendMessageRequest {
                    chat_id,
                    text,
                    parse_mode: None,
                    reply_to_message_id: reply_to,
                    message_thread_id: candidate_thread_id,
                };
                let retry: TelegramResponse<TelegramMessage> = self
                    .client
                    .post(&url)
                    .json(&plain)
                    .send()
                    .await?
                    .json()
                    .await?;
                if retry.ok {
                    return retry
                        .result
                        .map(|m| m.message_id)
                        .ok_or_else(|| anyhow::anyhow!("Telegram sendMessage: no result"));
                }

                let retry_description = retry.description.unwrap_or_default();
                if candidate_thread_id.is_some() && is_thread_not_found(&retry_description) {
                    warn!(
                        thread_id = ?candidate_thread_id,
                        "Telegram plain-text retry failed because thread is missing, retrying without message_thread_id"
                    );
                    continue;
                }

                anyhow::bail!("Telegram sendMessage failed: {}", retry_description);
            }

            anyhow::bail!("Telegram sendMessage failed: {}", description);
        }

        anyhow::bail!("Telegram sendMessage failed: thread not found")
    }

    /// Edit an already-sent Telegram message using `editMessageText`.
    async fn edit_message_text(
        &self,
        chat_id: &str,
        message_id: i64,
        text: &str,
    ) -> anyhow::Result<()> {
        #[derive(Serialize)]
        struct EditMessageTextRequest<'a> {
            chat_id: &'a str,
            message_id: i64,
            text: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            parse_mode: Option<&'a str>,
        }

        let url = self.api_url("editMessageText");
        let body = EditMessageTextRequest {
            chat_id,
            message_id,
            text,
            parse_mode: Some("MarkdownV2"),
        };

        let resp: TelegramResponse<serde_json::Value> = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        if !resp.ok {
            // Retry without Markdown
            let plain_body = EditMessageTextRequest {
                chat_id,
                message_id,
                text,
                parse_mode: None,
            };
            let retry: TelegramResponse<serde_json::Value> = self
                .client
                .post(&url)
                .json(&plain_body)
                .send()
                .await?
                .json()
                .await?;
            if !retry.ok {
                anyhow::bail!(
                    "Telegram editMessageText failed: {}",
                    retry.description.unwrap_or_default()
                );
            }
        }

        Ok(())
    }

    /// Send a `sendChatAction` API call to show a typing indicator.
    ///
    /// WHY: Telegram clears the "typing…" indicator after ~5 seconds. The
    /// caller is responsible for refreshing it periodically (every 4 s) until
    /// the response is ready.
    async fn send_chat_action(&self, chat_id: &str, thread_id: Option<i64>) -> anyhow::Result<()> {
        #[derive(serde::Serialize)]
        struct ChatActionRequest<'a> {
            chat_id: &'a str,
            action: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            message_thread_id: Option<i64>,
        }

        let url = self.api_url("sendChatAction");
        let body = ChatActionRequest {
            chat_id,
            action: "typing",
            message_thread_id: thread_id,
        };
        let response: TelegramResponse<serde_json::Value> = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;
        if response.ok {
            return Ok(());
        }

        let description = response.description.unwrap_or_default();
        if thread_id.is_some() && is_thread_not_found(&description) {
            let fallback = ChatActionRequest {
                chat_id,
                action: "typing",
                message_thread_id: None,
            };
            let retry: TelegramResponse<serde_json::Value> = self
                .client
                .post(&url)
                .json(&fallback)
                .send()
                .await?
                .json()
                .await?;
            if retry.ok {
                return Ok(());
            }
            anyhow::bail!(
                "Telegram sendChatAction failed: {}",
                retry.description.unwrap_or_default()
            );
        }
        anyhow::bail!("Telegram sendChatAction failed: {}", description)
    }

    async fn get_file_info(&self, file_id: &str) -> anyhow::Result<TelegramFileInfo> {
        let url = self.api_url("getFile");
        let response: TelegramResponse<TelegramFileInfo> = self
            .client
            .get(&url)
            .query(&[("file_id", file_id)])
            .send()
            .await?
            .json()
            .await?;

        if !response.ok {
            anyhow::bail!(
                "Telegram getFile failed: {}",
                response.description.unwrap_or_default()
            );
        }

        response
            .result
            .ok_or_else(|| anyhow::anyhow!("Telegram getFile returned no file info"))
    }

    async fn materialize_attachment(
        &self,
        file_id: &str,
        kind: MessageAttachmentKind,
        file_name: Option<String>,
        mime_type: Option<String>,
        size_bytes: Option<u64>,
        fallback_name: &str,
    ) -> MessageAttachment {
        let mut attachment = MessageAttachment {
            kind,
            file_name: file_name
                .clone()
                .or_else(|| Some(fallback_name.to_string())),
            mime_type,
            size_bytes,
            ..Default::default()
        };

        match self.get_file_info(file_id).await {
            Ok(file_info) => {
                if let Some(file_path) = file_info.file_path {
                    let download_url =
                        format!("{}/file/bot{}/{}", TELEGRAM_API_BASE, self.token, file_path);
                    attachment.url = Some(download_url.clone());

                    match self
                        .download_attachment(&download_url, attachment.file_name.as_deref())
                        .await
                    {
                        Ok(Some(local_path)) => {
                            attachment.local_path = Some(local_path);
                        }
                        Ok(None) => {}
                        Err(error) => {
                            warn!(%error, file_id, "Telegram attachment download failed");
                        }
                    }
                }
            }
            Err(error) => {
                warn!(%error, file_id, "Telegram attachment metadata lookup failed");
            }
        }

        attachment
    }

    async fn download_attachment(
        &self,
        url: &str,
        file_name: Option<&str>,
    ) -> anyhow::Result<Option<String>> {
        let response = self.client.get(url).send().await?.error_for_status()?;
        let bytes = response.bytes().await?;
        if bytes.is_empty() {
            return Ok(None);
        }

        crate::attachment_cache::persist_bytes(
            "telegram",
            file_name,
            "telegram-attachment.bin",
            bytes.as_ref(),
        )
    }

    async fn extract_attachments(&self, message: &TelegramMessage) -> Vec<MessageAttachment> {
        let mut attachments = Vec::new();

        if let Some(photo) = message
            .photo
            .iter()
            .max_by_key(|photo| photo.file_size.unwrap_or(0))
        {
            attachments.push(
                self.materialize_attachment(
                    &photo.file_id,
                    MessageAttachmentKind::Image,
                    Some(format!("telegram-photo-{}.jpg", message.message_id)),
                    Some("image/jpeg".into()),
                    photo.file_size,
                    "telegram-photo.jpg",
                )
                .await,
            );
        }

        if let Some(document) = &message.document {
            attachments.push(
                self.materialize_attachment(
                    &document.file_id,
                    MessageAttachmentKind::Document,
                    document.file_name.clone(),
                    document.mime_type.clone(),
                    document.file_size,
                    "telegram-document.bin",
                )
                .await,
            );
        }

        if let Some(audio) = &message.audio {
            attachments.push(
                self.materialize_attachment(
                    &audio.file_id,
                    MessageAttachmentKind::Audio,
                    audio.file_name.clone(),
                    audio.mime_type.clone(),
                    audio.file_size,
                    "telegram-audio.bin",
                )
                .await,
            );
        }

        if let Some(video) = &message.video {
            attachments.push(
                self.materialize_attachment(
                    &video.file_id,
                    MessageAttachmentKind::Video,
                    video.file_name.clone(),
                    video.mime_type.clone(),
                    video.file_size,
                    "telegram-video.bin",
                )
                .await,
            );
        }

        if let Some(animation) = &message.animation {
            attachments.push(
                self.materialize_attachment(
                    &animation.file_id,
                    MessageAttachmentKind::Video,
                    animation.file_name.clone(),
                    animation.mime_type.clone(),
                    animation.file_size,
                    "telegram-animation.bin",
                )
                .await,
            );
        }

        if let Some(voice) = &message.voice {
            attachments.push(
                self.materialize_attachment(
                    &voice.file_id,
                    MessageAttachmentKind::Voice,
                    voice
                        .file_name
                        .clone()
                        .or_else(|| Some(format!("telegram-voice-{}.ogg", message.message_id))),
                    voice.mime_type.clone().or_else(|| Some("audio/ogg".into())),
                    voice.file_size,
                    "telegram-voice.ogg",
                )
                .await,
            );
        }

        if let Some(sticker) = &message.sticker {
            let sticker_name = if sticker.is_video.unwrap_or(false) {
                format!("telegram-sticker-{}.webm", message.message_id)
            } else if sticker.is_animated.unwrap_or(false) {
                format!("telegram-sticker-{}.tgs", message.message_id)
            } else {
                format!("telegram-sticker-{}.webp", message.message_id)
            };
            let mut attachment = self
                .materialize_attachment(
                    &sticker.file_id,
                    MessageAttachmentKind::Sticker,
                    Some(sticker_name),
                    None,
                    sticker.file_size,
                    "telegram-sticker.bin",
                )
                .await;
            if let Some(emoji) = sticker.emoji.as_deref() {
                attachment.file_name = attachment
                    .file_name
                    .take()
                    .map(|file_name| format!("{} {}", emoji, file_name));
            }
            attachments.push(attachment);
        }

        attachments
    }
}

fn render_telegram_incoming_text(
    text: Option<&str>,
    caption: Option<&str>,
    attachments: &[MessageAttachment],
) -> String {
    let base = text
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .or_else(|| caption.map(str::trim).filter(|caption| !caption.is_empty()));

    if attachments.is_empty() {
        return base.unwrap_or_default().to_string();
    }

    let mut lines = Vec::new();
    if let Some(base) = base {
        lines.push(base.to_string());
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

/// Escape special characters for Telegram MarkdownV2 format.
///
/// See <https://core.telegram.org/bots/api#markdownv2-style>
fn escape_markdown_v2(text: &str) -> String {
    let special = [
        '_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!',
    ];
    let mut out = String::with_capacity(text.len() * 2);
    for ch in text.chars() {
        if special.contains(&ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

#[async_trait]
impl PlatformAdapter for TelegramAdapter {
    fn platform(&self) -> Platform {
        Platform::Telegram
    }

    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
        info!("Telegram adapter starting (long-poll mode)");
        let mut offset: i64 = 0;

        loop {
            match self.get_updates(offset).await {
                Ok(updates) => {
                    for update in updates {
                        offset = update.update_id + 1;

                        let Some(msg) = update.message else {
                            continue;
                        };

                        let user_id = msg
                            .from
                            .as_ref()
                            .map(|u| u.id.to_string())
                            .unwrap_or_else(|| "unknown".into());

                        // Filter by allowed_users (empty = open access)
                        if !self.allowed_users.is_empty() && !self.allowed_users.contains(&user_id)
                        {
                            debug!(
                                user_id = %user_id,
                                "Telegram message filtered: not in allowed list"
                            );
                            continue;
                        }

                        let display_name = msg.from.as_ref().map(|u| {
                            let mut name = u.first_name.clone();
                            if let Some(ref last) = u.last_name {
                                name.push(' ');
                                name.push_str(last);
                            }
                            name
                        });

                        let attachments = self.extract_attachments(&msg).await;
                        let rendered_text = render_telegram_incoming_text(
                            msg.text.as_deref(),
                            msg.caption.as_deref(),
                            &attachments,
                        );
                        if rendered_text.trim().is_empty() && attachments.is_empty() {
                            continue;
                        }

                        let incoming = IncomingMessage {
                            platform: Platform::Telegram,
                            user_id,
                            channel_id: Some(msg.chat.id.to_string()),
                            text: rendered_text,
                            thread_id: msg.message_thread_id.map(|id| id.to_string()),
                            metadata: MessageMetadata {
                                message_id: Some(msg.message_id.to_string()),
                                channel_id: Some(msg.chat.id.to_string()),
                                thread_id: msg.message_thread_id.map(|id| id.to_string()),
                                user_display_name: display_name,
                                attachments,
                            },
                        };

                        debug!(
                            platform = "telegram",
                            chat_id = msg.chat.id,
                            "Received message"
                        );

                        if tx.send(incoming).await.is_err() {
                            info!("Telegram adapter: receiver dropped, shutting down");
                            return Ok(());
                        }
                    }
                }
                Err(e) => {
                    error!(error = %e, "Telegram getUpdates failed, retrying");
                    tokio::time::sleep(RETRY_DELAY).await;
                }
            }
        }
    }

    async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()> {
        let chat_id = msg
            .metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Telegram send requires channel_id"))?;

        let reply_to = msg
            .metadata
            .message_id
            .as_deref()
            .and_then(|id| id.parse::<i64>().ok());

        let thread_id = msg
            .metadata
            .thread_id
            .as_deref()
            .and_then(|id| id.parse::<i64>().ok());

        // Split long messages
        let formatted = self.format_response(&msg.text, &msg.metadata);
        let chunks = split_message(&formatted, MAX_MESSAGE_LENGTH);

        for chunk in &chunks {
            self.send_message(chat_id, chunk, reply_to, thread_id, true)
                .await?;
        }

        Ok(())
    }

    fn format_response(&self, text: &str, _metadata: &MessageMetadata) -> String {
        escape_markdown_v2(text)
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

    fn supports_editing(&self) -> bool {
        true
    }

    async fn send_typing(&self, metadata: &MessageMetadata) -> anyhow::Result<()> {
        let chat_id = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("send_typing: no channel_id in metadata"))?;
        let thread_id = metadata
            .thread_id
            .as_deref()
            .and_then(|id| id.parse::<i64>().ok());
        self.send_chat_action(chat_id, thread_id).await
    }

    async fn send_and_get_id(&self, msg: OutgoingMessage) -> anyhow::Result<Option<String>> {
        let chat_id = msg
            .metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Telegram send_and_get_id requires channel_id"))?;
        let reply_to = msg
            .metadata
            .message_id
            .as_deref()
            .and_then(|id| id.parse::<i64>().ok());
        let thread_id = msg
            .metadata
            .thread_id
            .as_deref()
            .and_then(|id| id.parse::<i64>().ok());
        let formatted = self.format_response(&msg.text, &msg.metadata);
        // Use the first chunk's ID; edit will update in place.
        let chunks = split_message(&formatted, MAX_MESSAGE_LENGTH);
        let first = chunks.first().cloned().unwrap_or_default();
        let message_id = self
            .send_message_with_id(chat_id, &first, reply_to, thread_id, true)
            .await?;
        // Send remaining chunks (overflow) without tracking their IDs.
        for chunk in chunks.iter().skip(1) {
            self.send_message(chat_id, chunk, reply_to, thread_id, true)
                .await?;
        }
        Ok(Some(message_id.to_string()))
    }

    async fn edit_message(
        &self,
        message_id: &str,
        metadata: &MessageMetadata,
        new_text: &str,
    ) -> anyhow::Result<String> {
        let chat_id = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Telegram edit_message requires channel_id"))?;
        let msg_id: i64 = message_id
            .parse()
            .map_err(|_| anyhow::anyhow!("Telegram edit_message: invalid message_id"))?;

        // If the text exceeds limit, just edit with a truncated version —
        // the final send() will deliver the full text.
        let text = if new_text.len() > MAX_MESSAGE_LENGTH {
            edgecrab_core::safe_truncate(new_text, MAX_MESSAGE_LENGTH)
        } else {
            new_text
        };

        let formatted = escape_markdown_v2(text);
        self.edit_message_text(chat_id, msg_id, &formatted).await?;
        Ok(message_id.to_string())
    }

    async fn send_photo(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        let chat_id = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Telegram send_photo requires channel_id"))?;
        let thread_id = metadata
            .thread_id
            .as_deref()
            .and_then(|t| t.parse::<i64>().ok());

        let file_bytes = tokio::fs::read(path)
            .await
            .map_err(|e| anyhow::anyhow!("send_photo: cannot read {}: {}", path, e))?;

        let file_name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("photo.jpg")
            .to_string();

        let url = self.api_url("sendPhoto");
        let mut form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part(
                "photo",
                reqwest::multipart::Part::bytes(file_bytes).file_name(file_name),
            );
        if let Some(c) = caption {
            form = form.text("caption", c.to_string());
        }
        if let Some(tid) = thread_id {
            form = form.text("message_thread_id", tid.to_string());
        }

        let resp = self.client.post(&url).multipart(form).send().await?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Telegram sendPhoto failed: {}", body);
        }
        Ok(())
    }

    async fn send_document(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        let chat_id = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Telegram send_document requires channel_id"))?;
        let thread_id = metadata
            .thread_id
            .as_deref()
            .and_then(|t| t.parse::<i64>().ok());

        let file_bytes = tokio::fs::read(path)
            .await
            .map_err(|e| anyhow::anyhow!("send_document: cannot read {}: {}", path, e))?;

        let file_name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("document")
            .to_string();

        let url = self.api_url("sendDocument");
        let mut form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part(
                "document",
                reqwest::multipart::Part::bytes(file_bytes).file_name(file_name),
            );
        if let Some(c) = caption {
            form = form.text("caption", c.to_string());
        }
        if let Some(tid) = thread_id {
            form = form.text("message_thread_id", tid.to_string());
        }

        let resp = self.client.post(&url).multipart(form).send().await?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Telegram sendDocument failed: {}", body);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_markdown_v2() {
        assert_eq!(escape_markdown_v2("hello"), "hello");
        assert_eq!(escape_markdown_v2("hello_world"), "hello\\_world");
        assert_eq!(escape_markdown_v2("a*b*c"), "a\\*b\\*c");
        assert_eq!(escape_markdown_v2("1.2.3"), "1\\.2\\.3");
    }

    #[test]
    fn test_split_message_short() {
        let chunks = split_message("short msg", 4096);
        assert_eq!(chunks, vec!["short msg"]);
    }

    #[test]
    fn test_split_message_long() {
        let long = "a".repeat(5000);
        let chunks = split_message(&long, 4096);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 4096);
        assert_eq!(chunks[1].len(), 904);
    }

    #[test]
    fn test_split_message_newline_boundary() {
        let text = format!("{}\n{}", "a".repeat(100), "b".repeat(100));
        let chunks = split_message(&text, 110);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], "a".repeat(100));
        assert_eq!(chunks[1], "b".repeat(100));
    }

    #[test]
    fn test_is_available_without_env() {
        // In test environment, the token is typically not set
        // This just exercises the code path
        let _ = TelegramAdapter::is_available();
    }

    #[test]
    fn test_format_response() {
        // Create adapter with explicit fields for testing
        let adapter = TelegramAdapter {
            token: "test".into(),
            client: reqwest::Client::new(),
            allowed_users: Vec::new(),
        };
        let formatted = adapter.format_response("hello_world", &MessageMetadata::default());
        assert_eq!(formatted, "hello\\_world");
    }

    #[test]
    fn test_capabilities() {
        let adapter = TelegramAdapter {
            token: "test".into(),
            client: reqwest::Client::new(),
            allowed_users: Vec::new(),
        };
        assert_eq!(adapter.max_message_length(), 4096);
        assert!(adapter.supports_markdown());
        assert!(adapter.supports_images());
        assert!(adapter.supports_files());
        assert_eq!(adapter.platform(), Platform::Telegram);
    }

    #[test]
    fn test_render_telegram_incoming_text_with_attachment() {
        let attachments = vec![MessageAttachment {
            kind: MessageAttachmentKind::Image,
            file_name: Some("telegram-photo-1.jpg".into()),
            local_path: Some("/tmp/telegram-photo-1.jpg".into()),
            ..Default::default()
        }];
        let rendered = render_telegram_incoming_text(None, Some("Look at this"), &attachments);
        assert!(rendered.contains("Look at this"));
        assert!(rendered.contains("Shared 1 attachment:"));
        assert!(rendered.contains("image: telegram-photo-1.jpg"));
    }

    #[test]
    fn test_thread_not_found_detection() {
        assert!(is_thread_not_found("Bad Request: message thread not found"));
        assert!(is_thread_not_found("thread not found"));
        assert!(!is_thread_not_found("chat not found"));
    }
}
