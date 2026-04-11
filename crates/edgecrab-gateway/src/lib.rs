//! # edgecrab-gateway
//!
//! Multi-platform messaging gateway (Discord, Slack, Telegram, etc.).
//!
//! ```text
//!   edgecrab-gateway
//!     ├── platform.rs   — PlatformAdapter trait, IncomingMessage, OutgoingMessage
//!     ├── session.rs    — SessionManager with DashMap, session lifecycle
//!     ├── delivery.rs   — DeliveryRouter, message splitting
//!     ├── hooks.rs      — GatewayHook trait, HookRegistry, event matching
//!     ├── config.rs     — GatewayConfig (serde YAML)
//!     ├── webhook.rs    — WebhookAdapter (always-on HTTP adapter)
//!     └── run.rs        — Gateway runner, axum HTTP server, dispatch loop
//! ```

#![deny(clippy::unwrap_used)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::useless_conversion,
        clippy::field_reassign_with_default
    )
)]

pub mod api_server;
pub mod attachment_cache;
pub mod channel_directory;
pub mod config;
pub mod delivery;
pub mod dingtalk;
pub mod discord;
pub mod email;
pub mod event_processor;
pub mod feishu;
pub mod homeassistant;
pub mod hooks;
pub mod interactions;
pub mod matrix;
pub mod mattermost;
pub mod mirror;
pub mod pairing;
pub mod platform;
pub mod run;
pub mod sender;
pub mod session;
pub mod signal;
pub mod slack;
pub mod sms;
pub mod stream_consumer;
pub mod telegram;
pub mod voice_delivery;
pub mod webhook;
pub mod webhook_subscriptions;
pub mod wecom;
pub mod whatsapp;

// ─── Shared adapter constants ─────────────────────────────────────────

/// Base retry delay shared by all platform adapters after a network error.
///
/// WHY a shared constant: every adapter (Telegram, Discord, Slack, Signal,
/// Matrix, Mattermost, HomeAssistant) previously defined `RETRY_DELAY =
/// Duration::from_secs(5)` independently. One source of truth avoids
/// divergence if the value needs tuning.
pub(crate) const ADAPTER_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(5);

/// Maximum retry delay for exponential-backoff loops in platform adapters.
pub(crate) const ADAPTER_MAX_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(60);
