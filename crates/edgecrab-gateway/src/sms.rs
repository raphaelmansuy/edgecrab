//! # SMS adapter — Twilio REST API + webhook
//!
//! Sends outbound SMS via Twilio REST API. Receives inbound messages
//! via an axum webhook endpoint.
//!
//! ## Environment variables
//!
//! | Variable              | Required | Description                          |
//! |-----------------------|----------|--------------------------------------|
//! | `TWILIO_ACCOUNT_SID`  | Yes      | Twilio Account SID                   |
//! | `TWILIO_AUTH_TOKEN`    | Yes      | Twilio Auth Token                    |
//! | `TWILIO_PHONE_NUMBER`  | Yes      | From-number in E.164 format          |
//! | `SMS_WEBHOOK_PORT`    | No       | Webhook port (default: 8082)         |
//! | `SMS_ALLOWED_USERS`   | No       | Comma-separated allowed phone numbers|
//!
//! ## Limits
//!
//! - Max message length: **1600** characters (~10 SMS segments)

use std::env;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Form, Router};
use base64::Engine;
use edgecrab_types::Platform;
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::platform::{IncomingMessage, MessageMetadata, OutgoingMessage, PlatformAdapter};

const MAX_MESSAGE_LENGTH: usize = 1600;
const DEFAULT_WEBHOOK_PORT: u16 = 8082;
const TWILIO_API_BASE: &str = "https://api.twilio.com/2010-04-01/Accounts";

pub struct SmsAdapter {
    account_sid: String,
    auth_token: String,
    from_number: String,
    webhook_port: u16,
    allowed_users: Vec<String>,
}

impl SmsAdapter {
    pub fn from_env() -> Option<Self> {
        let account_sid = env::var("TWILIO_ACCOUNT_SID").ok()?;
        let auth_token = env::var("TWILIO_AUTH_TOKEN").ok()?;
        let from_number = env::var("TWILIO_PHONE_NUMBER").ok()?;
        let webhook_port = env::var("SMS_WEBHOOK_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(DEFAULT_WEBHOOK_PORT);
        let allowed_users = env::var("SMS_ALLOWED_USERS")
            .ok()
            .map(|s| {
                s.split(',')
                    .filter_map(normalize_phone_number)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Some(Self {
            account_sid,
            auth_token,
            from_number: normalize_phone_number(&from_number).unwrap_or(from_number),
            webhook_port,
            allowed_users,
        })
    }

    pub fn is_available() -> bool {
        env::var("TWILIO_ACCOUNT_SID").is_ok() && env::var("TWILIO_AUTH_TOKEN").is_ok()
    }

    fn basic_auth_header(&self) -> String {
        let creds = format!("{}:{}", self.account_sid, self.auth_token);
        let encoded = base64::engine::general_purpose::STANDARD.encode(creds.as_bytes());
        format!("Basic {encoded}")
    }
}

fn normalize_phone_number(value: &str) -> Option<String> {
    let compact: String = value
        .chars()
        .filter(|c| !c.is_ascii_whitespace() && !matches!(c, '-' | '(' | ')'))
        .collect();
    if compact.is_empty() {
        return None;
    }

    if let Some(rest) = compact.strip_prefix('+') {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("+{rest}"));
        }
        return None;
    }

    if let Some(rest) = compact.strip_prefix("00") {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("+{rest}"));
        }
        return None;
    }

    if compact.chars().all(|c| c.is_ascii_digit()) {
        return Some(format!("+{compact}"));
    }

    None
}

/// Twilio webhook form data for inbound SMS.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TwilioWebhookForm {
    #[serde(rename = "From")]
    from: Option<String>,
    #[serde(rename = "To")]
    to: Option<String>,
    #[serde(rename = "Body")]
    body: Option<String>,
    #[serde(rename = "MessageSid")]
    message_sid: Option<String>,
}

/// Validate Twilio `X-Twilio-Signature` header using HMAC-SHA1.
///
/// Algorithm (Twilio spec):
///   1. Concatenate webhook URL + sorted POST params as `key=value`
///   2. HMAC-SHA1 with the auth token as key
///   3. base64-encode and compare with the header value
///
/// Uses constant-time comparison to prevent timing side-channel attacks.
fn validate_twilio_signature(
    auth_token: &str,
    url: &str,
    params: &std::collections::BTreeMap<String, String>,
    signature_header: &str,
) -> bool {
    use hmac::{Hmac, Mac};
    use sha1::Sha1;
    use subtle::ConstantTimeEq;

    let mut data = url.to_string();
    for (key, value) in params {
        data.push_str(key);
        data.push_str(value);
    }

    let Ok(mut mac) = Hmac::<Sha1>::new_from_slice(auth_token.as_bytes()) else {
        return false;
    };
    mac.update(data.as_bytes());
    let expected = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());

    bool::from(expected.as_bytes().ct_eq(signature_header.as_bytes()))
}

/// Shared state for the webhook handler.
struct WebhookState {
    tx: mpsc::Sender<IncomingMessage>,
    allowed_users: Vec<String>,
    auth_token: String,
    webhook_url: String,
}

async fn handle_twilio_webhook(
    State(state): State<Arc<WebhookState>>,
    headers: axum::http::HeaderMap,
    Form(form): Form<TwilioWebhookForm>,
) -> &'static str {
    // ── Twilio signature validation ──────────────────────────────────
    if !state.auth_token.is_empty() {
        let sig = headers
            .get("X-Twilio-Signature")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let mut params = std::collections::BTreeMap::new();
        if let Some(ref v) = form.from {
            params.insert("From".to_string(), v.clone());
        }
        if let Some(ref v) = form.to {
            params.insert("To".to_string(), v.clone());
        }
        if let Some(ref v) = form.body {
            params.insert("Body".to_string(), v.clone());
        }
        if let Some(ref v) = form.message_sid {
            params.insert("MessageSid".to_string(), v.clone());
        }
        if !validate_twilio_signature(&state.auth_token, &state.webhook_url, &params, sig) {
            warn!("Twilio signature validation failed — rejecting webhook");
            return "<Response></Response>";
        }
    }

    let from = normalize_phone_number(&form.from.unwrap_or_default()).unwrap_or_default();
    let body = form.body.unwrap_or_default();
    let message_sid = form.message_sid.unwrap_or_default();

    if body.trim().is_empty() {
        return "<Response></Response>";
    }

    if !state.allowed_users.is_empty() && !state.allowed_users.contains(&from) {
        debug!("SMS from unauthorized number: {}", from);
        return "<Response></Response>";
    }

    let incoming = IncomingMessage {
        platform: Platform::Sms,
        user_id: from.clone(),
        channel_id: Some(from.clone()),
        chat_type: crate::platform::ChatType::Dm,
        text: body,
        thread_id: None,
        metadata: MessageMetadata {
            message_id: Some(message_sid),
            channel_id: Some(from),
            ..Default::default()
        },
    };

    if state.tx.send(incoming).await.is_err() {
        warn!("SMS message channel closed");
    }

    // TwiML empty response (no auto-reply)
    "<Response></Response>"
}

#[async_trait]
impl PlatformAdapter for SmsAdapter {
    fn platform(&self) -> Platform {
        Platform::Sms
    }

    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
        info!(
            "SMS adapter starting — webhook on port {}",
            self.webhook_port
        );

        let webhook_url = env::var("SMS_WEBHOOK_URL")
            .unwrap_or_else(|_| format!("http://localhost:{}/webhooks/twilio", self.webhook_port));

        let state = Arc::new(WebhookState {
            tx,
            allowed_users: self.allowed_users.clone(),
            auth_token: self.auth_token.clone(),
            webhook_url,
        });

        let app = Router::new()
            .route("/webhooks/twilio", post(handle_twilio_webhook))
            .route("/health", get(|| async { "ok" }))
            .with_state(state);

        let listener =
            tokio::net::TcpListener::bind(format!("0.0.0.0:{}", self.webhook_port)).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }

    async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()> {
        let raw_to = msg
            .metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("No recipient phone number"))?;
        let to = normalize_phone_number(raw_to).ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid SMS recipient '{}'; use an E.164 phone number or digits only",
                raw_to
            )
        })?;

        let url = format!("{}/{}/Messages.json", TWILIO_API_BASE, self.account_sid);
        let client = reqwest::Client::new();

        let resp = client
            .post(&url)
            .header("Authorization", self.basic_auth_header())
            .form(&[
                ("From", self.from_number.as_str()),
                ("To", to.as_str()),
                ("Body", &msg.text),
            ])
            .timeout(Duration::from_secs(30))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            error!("Twilio API error {}: {}", status, text);
            anyhow::bail!("Twilio API error {status}");
        }

        debug!("SMS sent to {}", to);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sms_max_length() {
        assert_eq!(MAX_MESSAGE_LENGTH, 1600);
    }

    #[test]
    fn phone_numbers_are_normalized_to_e164() {
        assert_eq!(
            normalize_phone_number("+1 (555) 123-4567"),
            Some("+15551234567".into())
        );
        assert_eq!(
            normalize_phone_number("00447911123456"),
            Some("+447911123456".into())
        );
        assert_eq!(
            normalize_phone_number("15551234567"),
            Some("+15551234567".into())
        );
    }

    #[test]
    fn invalid_phone_number_is_rejected() {
        assert_eq!(normalize_phone_number("abc123"), None);
        assert_eq!(normalize_phone_number(""), None);
    }

    #[test]
    fn twilio_signature_valid() {
        use std::collections::BTreeMap;
        let auth_token = "12345";
        let url = "https://mycompany.com/myapp.php?foo=1&bar=2";
        let mut params = BTreeMap::new();
        params.insert("CallSid".to_string(), "CA1234567890ABCDE".to_string());
        params.insert("Caller".to_string(), "+14158675310".to_string());
        params.insert("Digits".to_string(), "1234".to_string());
        params.insert("From".to_string(), "+14158675310".to_string());
        params.insert("To".to_string(), "+18005551212".to_string());

        // Compute expected signature for this test data
        use hmac::{Hmac, Mac};
        use sha1::Sha1;
        let mut data = url.to_string();
        for (key, value) in &params {
            data.push_str(key);
            data.push_str(value);
        }
        let mut mac = Hmac::<Sha1>::new_from_slice(auth_token.as_bytes()).expect("hmac");
        mac.update(data.as_bytes());
        let expected_sig =
            base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());

        assert!(validate_twilio_signature(
            auth_token,
            url,
            &params,
            &expected_sig
        ));
    }

    #[test]
    fn twilio_signature_tampered() {
        use std::collections::BTreeMap;
        let params = BTreeMap::new();
        assert!(!validate_twilio_signature(
            "secret",
            "https://example.com/webhook",
            &params,
            "invalid-signature"
        ));
    }
}
