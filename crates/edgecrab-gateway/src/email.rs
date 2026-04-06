//! # Email adapter — HTTP-based email gateway
//!
//! Sends email via HTTP relay APIs (SendGrid, Mailgun, or generic SMTP
//! relay) and receives incoming mail via an axum webhook endpoint.
//!
//! This adapter supports multiple providers and is more cloud-native than
//! raw IMAP/SMTP. Most email services (SendGrid, Mailgun, Postmark, etc.)
//! support inbound email parsing to webhooks.
//!
//! ## Environment variables
//!
//! | Variable              | Required | Description                                        |
//! |-----------------------|----------|----------------------------------------------------|
//! | `EMAIL_PROVIDER`      | Yes      | One of: `sendgrid`, `mailgun`, `generic_smtp`      |
//! | `EMAIL_API_KEY`       | Yes      | API key for the provider                            |
//! | `EMAIL_FROM`          | Yes      | Sender address (e.g. bot@example.com)               |
//! | `EMAIL_DOMAIN`        | No       | Domain for Mailgun (e.g. mg.example.com)            |
//! | `EMAIL_WEBHOOK_PORT`  | No       | Webhook port (default: 8093)                        |
//! | `EMAIL_ALLOWED`       | No       | Comma-separated allowed sender addresses            |
//!
//! ## Limits
//!
//! - Max message length: **50000** characters

use std::env;
use std::time::Duration;

use async_trait::async_trait;
use axum::Router;
use axum::extract::Json;
use axum::routing::post;
use edgecrab_types::Platform;
use lettre::message::Mailbox;
use lettre::message::{Attachment, MultiPart, SinglePart, header};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::platform::{IncomingMessage, MessageMetadata, OutgoingMessage, PlatformAdapter};

const MAX_MESSAGE_LENGTH: usize = 50_000;

#[derive(Debug, Clone)]
enum EmailProvider {
    SendGrid,
    Mailgun,
    GenericSmtp,
}

pub struct EmailAdapter {
    provider: EmailProvider,
    api_key: String,
    from_address: String,
    domain: Option<String>,
    smtp_host: Option<String>,
    smtp_port: u16,
    smtp_username: Option<String>,
    smtp_password: Option<String>,
    webhook_port: u16,
    allowed_senders: Vec<String>,
}

impl EmailAdapter {
    pub fn from_env() -> Option<Self> {
        let provider_str = env::var("EMAIL_PROVIDER").ok()?;
        let from_address = env::var("EMAIL_FROM").ok()?;

        let provider = match provider_str.to_lowercase().as_str() {
            "sendgrid" => EmailProvider::SendGrid,
            "mailgun" => EmailProvider::Mailgun,
            "generic_smtp" | "smtp" => EmailProvider::GenericSmtp,
            _ => {
                warn!("Unknown EMAIL_PROVIDER: {}", provider_str);
                return None;
            }
        };

        let domain = env::var("EMAIL_DOMAIN").ok();
        let smtp_host = env::var("EMAIL_SMTP_HOST")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let smtp_port: u16 = env::var("EMAIL_SMTP_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(587);
        let smtp_username = env::var("EMAIL_SMTP_USERNAME")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let smtp_password = env::var("EMAIL_SMTP_PASSWORD")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let api_key = env::var("EMAIL_API_KEY")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_default();
        let webhook_port: u16 = env::var("EMAIL_WEBHOOK_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8093);
        let allowed_senders = env::var("EMAIL_ALLOWED")
            .ok()
            .map(|s| s.split(',').map(|a| a.trim().to_lowercase()).collect())
            .unwrap_or_default();

        match provider {
            EmailProvider::SendGrid | EmailProvider::Mailgun if api_key.is_empty() => return None,
            EmailProvider::GenericSmtp if smtp_host.is_none() => return None,
            EmailProvider::GenericSmtp if smtp_password.is_none() && api_key.is_empty() => {
                return None;
            }
            _ => {}
        }

        Some(Self {
            provider,
            api_key,
            from_address,
            domain,
            smtp_host,
            smtp_port,
            smtp_username,
            smtp_password,
            webhook_port,
            allowed_senders,
        })
    }

    pub fn is_available() -> bool {
        let provider = env::var("EMAIL_PROVIDER").unwrap_or_default();
        let has_from = env::var("EMAIL_FROM")
            .ok()
            .is_some_and(|value| !value.trim().is_empty());
        let has_api_key = env::var("EMAIL_API_KEY")
            .ok()
            .is_some_and(|value| !value.trim().is_empty());
        let has_smtp_host = env::var("EMAIL_SMTP_HOST")
            .ok()
            .is_some_and(|value| !value.trim().is_empty());
        let has_smtp_password = env::var("EMAIL_SMTP_PASSWORD")
            .ok()
            .is_some_and(|value| !value.trim().is_empty());

        match provider.trim().to_ascii_lowercase().as_str() {
            "sendgrid" | "mailgun" => has_from && has_api_key,
            "generic_smtp" | "smtp" => {
                has_from && has_smtp_host && (has_smtp_password || has_api_key)
            }
            _ => false,
        }
    }

    async fn send_sendgrid(&self, to: &str, subject: &str, body: &str) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let payload = serde_json::json!({
            "personalizations": [{
                "to": [{"email": to}],
                "subject": subject,
            }],
            "from": {"email": self.from_address},
            "content": [{
                "type": "text/plain",
                "value": body,
            }],
        });

        let resp = client
            .post("https://api.sendgrid.com/v3/mail/send")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("SendGrid error {status}: {text}");
        }

        Ok(())
    }

    async fn send_mailgun(&self, to: &str, subject: &str, body: &str) -> anyhow::Result<()> {
        let domain = self
            .domain
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("EMAIL_DOMAIN required for Mailgun"))?;

        let client = reqwest::Client::new();
        let form = [
            ("from", self.from_address.as_str()),
            ("to", to),
            ("subject", subject),
            ("text", body),
        ];

        let resp = client
            .post(format!("https://api.mailgun.net/v3/{domain}/messages"))
            .basic_auth("api", Some(&self.api_key))
            .form(&form)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Mailgun error {status}: {text}");
        }

        Ok(())
    }

    async fn send_generic_smtp(&self, to: &str, subject: &str, body: &str) -> anyhow::Result<()> {
        let smtp_host = self
            .smtp_host
            .clone()
            .ok_or_else(|| anyhow::anyhow!("EMAIL_SMTP_HOST required for generic_smtp"))?;
        let smtp_port = self.smtp_port;
        let smtp_username = self
            .smtp_username
            .clone()
            .unwrap_or_else(|| self.from_address.clone());
        let smtp_password = self
            .smtp_password
            .clone()
            .unwrap_or_else(|| self.api_key.clone());
        let from_address = normalize_email_address(&self.from_address)?;
        let to_address = normalize_email_address(to)?;
        let sanitized_subject = sanitize_email_subject(subject);
        let body = body.to_string();

        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let from_mailbox: Mailbox = from_address.parse()?;
            let to_mailbox: Mailbox = to_address.parse()?;
            let message = Message::builder()
                .from(from_mailbox)
                .to(to_mailbox)
                .subject(sanitized_subject)
                .body(body)?;

            let credentials = Credentials::new(smtp_username, smtp_password);
            let mailer = SmtpTransport::builder_dangerous(&smtp_host)
                .port(smtp_port)
                .credentials(credentials)
                .build();
            mailer.send(&message)?;
            Ok(())
        })
        .await
        .map_err(|error| anyhow::anyhow!("generic SMTP send task failed: {error}"))??;

        Ok(())
    }

    async fn send_generic_smtp_with_attachment(
        &self,
        to: &str,
        subject: &str,
        body: &str,
        file_path: &str,
    ) -> anyhow::Result<()> {
        let smtp_host = self
            .smtp_host
            .clone()
            .ok_or_else(|| anyhow::anyhow!("EMAIL_SMTP_HOST required for generic_smtp"))?;
        let smtp_port = self.smtp_port;
        let smtp_username = self
            .smtp_username
            .clone()
            .unwrap_or_else(|| self.from_address.clone());
        let smtp_password = self
            .smtp_password
            .clone()
            .unwrap_or_else(|| self.api_key.clone());
        let from_address = normalize_email_address(&self.from_address)?;
        let to_address = normalize_email_address(to)?;
        let sanitized_subject = sanitize_email_subject(subject);
        let body = body.to_string();
        let file_path = file_path.to_string();

        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let path = std::path::Path::new(&file_path);
            let file_name = path
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| anyhow::anyhow!("attachment path has no file name: {file_path}"))?
                .to_string();
            let bytes = std::fs::read(path)?;
            let content_type = email_attachment_content_type(&file_name);
            let from_mailbox: Mailbox = from_address.parse()?;
            let to_mailbox: Mailbox = to_address.parse()?;
            let attachment = Attachment::new(file_name).body(bytes, content_type);
            let multipart = MultiPart::mixed()
                .singlepart(SinglePart::plain(body))
                .singlepart(attachment);
            let message = Message::builder()
                .from(from_mailbox)
                .to(to_mailbox)
                .subject(sanitized_subject)
                .multipart(multipart)?;

            let credentials = Credentials::new(smtp_username, smtp_password);
            let mailer = SmtpTransport::builder_dangerous(&smtp_host)
                .port(smtp_port)
                .credentials(credentials)
                .build();
            mailer.send(&message)?;
            Ok(())
        })
        .await
        .map_err(|error| anyhow::anyhow!("generic SMTP send task failed: {error}"))??;

        Ok(())
    }
}

fn email_attachment_content_type(file_name: &str) -> header::ContentType {
    let mime = match std::path::Path::new(file_name)
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
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "json" => "application/json",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    };
    mime.parse().expect("valid content type")
}

fn normalize_email_address(value: &str) -> anyhow::Result<String> {
    let normalized = value.trim();
    if normalized.is_empty() {
        anyhow::bail!("email address is empty");
    }
    let mailbox: Mailbox = normalized
        .parse()
        .map_err(|error| anyhow::anyhow!("invalid email address '{}': {error}", value))?;
    Ok(mailbox.email.to_string())
}

fn sanitize_email_subject(subject: &str) -> String {
    let collapsed = subject
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.is_empty() {
        "EdgeCrab Response".into()
    } else {
        collapsed
    }
}

/// Inbound email webhook payload (common fields across providers)
#[derive(Debug, Deserialize)]
struct InboundEmail {
    from: Option<String>,
    #[allow(dead_code)]
    to: Option<String>,
    subject: Option<String>,
    text: Option<String>,
    #[serde(alias = "body-plain")]
    body_plain: Option<String>,
    #[serde(alias = "sender")]
    sender_email: Option<String>,
}

impl InboundEmail {
    fn sender(&self) -> String {
        self.from
            .clone()
            .or_else(|| self.sender_email.clone())
            .unwrap_or_default()
    }

    fn body(&self) -> String {
        self.text
            .clone()
            .or_else(|| self.body_plain.clone())
            .unwrap_or_default()
    }
}

#[async_trait]
impl PlatformAdapter for EmailAdapter {
    fn platform(&self) -> Platform {
        Platform::Email
    }

    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
        info!(
            "Email adapter starting (provider: {:?}, webhook port: {})",
            self.provider, self.webhook_port
        );

        let allowed = self.allowed_senders.clone();
        let tx = std::sync::Arc::new(tx);

        let app = Router::new().route(
            "/email/inbound",
            post({
                let tx = tx.clone();
                let allowed = allowed.clone();
                move |Json(email): Json<InboundEmail>| {
                    let tx = tx.clone();
                    let allowed = allowed.clone();
                    async move {
                        let sender = email.sender();
                        let body = email.body();

                        if body.is_empty() {
                            return "ok";
                        }

                        if !allowed.is_empty() && !allowed.contains(&sender.to_lowercase()) {
                            debug!("Email from unauthorized sender: {}", sender);
                            return "ok";
                        }

                        let incoming = IncomingMessage {
                            platform: Platform::Email,
                            user_id: sender.clone(),
                            channel_id: Some(sender.clone()),
                            text: body,
                            thread_id: email.subject.clone(),
                            metadata: MessageMetadata {
                                message_id: None,
                                channel_id: Some(sender),
                                thread_id: email.subject,
                                user_display_name: email.from,
                                attachments: Vec::new(),
                            },
                        };

                        let _ = tx.send(incoming).await;
                        "ok"
                    }
                }
            }),
        );

        let addr = format!("0.0.0.0:{}", self.webhook_port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        info!("Email webhook listening on {}", addr);
        axum::serve(listener, app).await?;
        Ok(())
    }

    async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()> {
        let to = msg
            .metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("No recipient email address"))?;
        let to = normalize_email_address(to)?;

        let subject = msg
            .metadata
            .thread_id
            .as_deref()
            .unwrap_or("EdgeCrab Response");

        match self.provider {
            EmailProvider::SendGrid => self.send_sendgrid(&to, subject, &msg.text).await?,
            EmailProvider::Mailgun => self.send_mailgun(&to, subject, &msg.text).await?,
            EmailProvider::GenericSmtp => {
                self.send_generic_smtp(&to, subject, &msg.text).await?;
            }
        }

        debug!("Email sent to {}", to);
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
        matches!(self.provider, EmailProvider::GenericSmtp)
    }

    async fn send_document(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        let to = metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("No recipient email address"))?;
        let to = normalize_email_address(to)?;
        let subject = metadata.thread_id.as_deref().unwrap_or("EdgeCrab Response");

        match self.provider {
            EmailProvider::GenericSmtp => {
                self.send_generic_smtp_with_attachment(
                    &to,
                    subject,
                    caption.unwrap_or_default(),
                    path,
                )
                .await?
            }
            EmailProvider::SendGrid | EmailProvider::Mailgun => {
                let file_name = std::path::Path::new(path)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or(path);
                let body = match caption {
                    Some(caption) if !caption.trim().is_empty() => {
                        format!("{caption}\n\nAttachment: {file_name} ({path})")
                    }
                    _ => format!("Attachment: {file_name} ({path})"),
                };
                match self.provider {
                    EmailProvider::SendGrid => self.send_sendgrid(&to, subject, &body).await?,
                    EmailProvider::Mailgun => self.send_mailgun(&to, subject, &body).await?,
                    EmailProvider::GenericSmtp => unreachable!(),
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_max_length() {
        assert_eq!(MAX_MESSAGE_LENGTH, 50_000);
    }

    #[test]
    fn email_addresses_are_trimmed_and_validated() {
        assert_eq!(
            normalize_email_address("  User@example.com ").expect("email"),
            "User@example.com"
        );
    }

    #[test]
    fn invalid_email_addresses_are_rejected() {
        assert!(normalize_email_address("not-an-email").is_err());
    }

    #[test]
    fn empty_subject_falls_back_to_default() {
        assert_eq!(sanitize_email_subject(" \n "), "EdgeCrab Response");
        assert_eq!(sanitize_email_subject("Status\nUpdate"), "Status Update");
    }

    #[test]
    fn generic_smtp_is_available_with_smtp_password_without_api_key() {
        unsafe {
            std::env::set_var("EMAIL_PROVIDER", "generic_smtp");
            std::env::set_var("EMAIL_FROM", "bot@example.com");
            std::env::set_var("EMAIL_SMTP_HOST", "smtp.example.com");
            std::env::set_var("EMAIL_SMTP_PASSWORD", "secret");
            std::env::remove_var("EMAIL_API_KEY");
        }
        assert!(EmailAdapter::is_available());
        assert!(EmailAdapter::from_env().is_some());
        unsafe {
            std::env::remove_var("EMAIL_PROVIDER");
            std::env::remove_var("EMAIL_FROM");
            std::env::remove_var("EMAIL_SMTP_HOST");
            std::env::remove_var("EMAIL_SMTP_PASSWORD");
        }
    }

    #[test]
    fn only_generic_smtp_claims_native_file_support() {
        let smtp = EmailAdapter {
            provider: EmailProvider::GenericSmtp,
            api_key: String::new(),
            from_address: "bot@example.com".into(),
            domain: None,
            smtp_host: Some("smtp.example.com".into()),
            smtp_port: 587,
            smtp_username: None,
            smtp_password: Some("secret".into()),
            webhook_port: 8093,
            allowed_senders: Vec::new(),
        };
        let sendgrid = EmailAdapter {
            provider: EmailProvider::SendGrid,
            api_key: "secret".into(),
            from_address: "bot@example.com".into(),
            domain: None,
            smtp_host: None,
            smtp_port: 587,
            smtp_username: None,
            smtp_password: None,
            webhook_port: 8093,
            allowed_senders: Vec::new(),
        };

        assert!(smtp.supports_files());
        assert!(!sendgrid.supports_files());
    }
}
