//! # Webhook adapter — HTTP inbound/outbound messaging
//!
//! WHY webhook is always-on: Unlike platform-specific adapters that
//! require API credentials, the webhook adapter accepts HTTP POST
//! requests and sends responses back to a callback URL. This is the
//! simplest integration path and is always available.
//!
//! ```text
//!   POST /webhook/incoming  → IncomingMessage → agent → response
//!   POST callback_url       ← OutgoingMessage (if callback configured)
//! ```

use std::sync::Arc;

use async_trait::async_trait;
use edgecrab_types::Platform;
use serde::Deserialize;
use tokio::sync::{Mutex, mpsc};

use crate::platform::{IncomingMessage, MessageMetadata, OutgoingMessage, PlatformAdapter};

/// Webhook adapter for HTTP-based messaging.
///
/// Inbound messages arrive via axum routes (handled in run.rs).
/// Outbound messages are collected in a buffer for retrieval.
pub struct WebhookAdapter {
    /// Buffer for outbound messages (polled by delivery or callback)
    outbox: Arc<Mutex<Vec<OutgoingMessage>>>,
}

impl WebhookAdapter {
    pub fn new() -> Self {
        Self {
            outbox: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Parse an incoming webhook payload into an IncomingMessage.
    pub fn parse_incoming(payload: &WebhookPayload) -> IncomingMessage {
        IncomingMessage {
            platform: Platform::Webhook,
            user_id: payload.user_id.clone().unwrap_or_else(|| "webhook".into()),
            channel_id: payload.channel_id.clone(),
            chat_type: crate::platform::ChatType::Channel,
            text: payload.text.clone(),
            thread_id: None,
            metadata: MessageMetadata {
                message_id: payload.message_id.clone(),
                channel_id: payload.channel_id.clone(),
                ..Default::default()
            },
        }
    }

    /// Drain outbound messages (for polling-based integrations).
    pub async fn drain_outbox(&self) -> Vec<OutgoingMessage> {
        let mut outbox = self.outbox.lock().await;
        std::mem::take(&mut *outbox)
    }

    /// Get reference to outbox for sharing.
    pub fn outbox(&self) -> Arc<Mutex<Vec<OutgoingMessage>>> {
        self.outbox.clone()
    }
}

impl Default for WebhookAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Webhook inbound payload.
#[derive(Debug, Clone, Deserialize)]
pub struct WebhookPayload {
    pub text: String,
    pub user_id: Option<String>,
    pub channel_id: Option<String>,
    pub message_id: Option<String>,
}

#[async_trait]
impl PlatformAdapter for WebhookAdapter {
    fn platform(&self) -> Platform {
        Platform::Webhook
    }

    async fn start(&self, _tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
        // Webhook adapter doesn't poll — messages arrive via HTTP routes.
        // This method blocks forever (or until the gateway shuts down).
        // The actual HTTP handler is in run.rs.
        futures::future::pending::<()>().await;
        Ok(())
    }

    async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()> {
        let mut outbox = self.outbox.lock().await;
        outbox.push(msg);
        Ok(())
    }

    fn format_response(&self, text: &str, _metadata: &MessageMetadata) -> String {
        text.to_string() // No formatting needed for webhooks
    }

    fn max_message_length(&self) -> usize {
        65_536 // 64KB — generous for webhooks
    }

    fn supports_markdown(&self) -> bool {
        true
    }

    fn supports_images(&self) -> bool {
        false
    }

    fn supports_files(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_incoming_basic() {
        let payload = WebhookPayload {
            text: "hello".into(),
            user_id: Some("user1".into()),
            channel_id: None,
            message_id: None,
        };
        let msg = WebhookAdapter::parse_incoming(&payload);
        assert_eq!(msg.text, "hello");
        assert_eq!(msg.user_id, "user1");
    }

    #[test]
    fn parse_incoming_defaults() {
        let payload = WebhookPayload {
            text: "test".into(),
            user_id: None,
            channel_id: None,
            message_id: None,
        };
        let msg = WebhookAdapter::parse_incoming(&payload);
        assert_eq!(msg.user_id, "webhook");
    }

    #[tokio::test]
    async fn send_collects_in_outbox() {
        let adapter = WebhookAdapter::new();
        let msg = OutgoingMessage {
            text: "response".into(),
            metadata: MessageMetadata::default(),
        };
        adapter.send(msg).await.expect("send");

        let out = adapter.drain_outbox().await;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "response");
    }

    #[tokio::test]
    async fn drain_clears_outbox() {
        let adapter = WebhookAdapter::new();
        adapter
            .send(OutgoingMessage {
                text: "msg1".into(),
                metadata: MessageMetadata::default(),
            })
            .await
            .expect("send");

        let out1 = adapter.drain_outbox().await;
        assert_eq!(out1.len(), 1);

        let out2 = adapter.drain_outbox().await;
        assert_eq!(out2.len(), 0); // drained
    }

    #[test]
    fn webhook_capabilities() {
        let adapter = WebhookAdapter::new();
        assert_eq!(adapter.max_message_length(), 65_536);
        assert!(adapter.supports_markdown());
        assert!(!adapter.supports_images());
    }
}
