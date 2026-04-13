//! # Platform adapter trait — unified interface for messaging platforms
//!
//! WHY a trait: Each messaging platform (Telegram, Discord, Slack, etc.)
//! has a wildly different API. This trait normalizes them into a single
//! interface so the gateway can dispatch messages to the agent without
//! knowing or caring which platform originated the message.
//!
//! ```text
//!   PlatformAdapter
//!     ├── start()         → spawn listener, push IncomingMessage to channel
//!     ├── send()          → deliver OutgoingMessage back to platform
//!     ├── format_response → platform-specific formatting (MarkdownV2, etc.)
//!     └── capabilities    → max length, markdown support, image support
//! ```

use async_trait::async_trait;
use edgecrab_types::Platform;
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use tokio::sync::mpsc;

/// Chat type: direct message vs. group/channel.
///
/// Used by the authorization layer to enforce group policies and by the
/// unauthorized-DM handler to decide whether to send pairing codes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ChatType {
    /// Direct / private message (1-on-1).
    #[default]
    Dm,
    /// Multi-user group or supergroup.
    Group,
    /// Broadcast channel (Telegram channels, Slack channels, etc.).
    Channel,
}

/// A normalized attachment kind across messaging platforms.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum MessageAttachmentKind {
    Image,
    Video,
    Audio,
    Voice,
    Document,
    Sticker,
    #[default]
    Other,
}

impl MessageAttachmentKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Voice => "voice",
            Self::Document => "document",
            Self::Sticker => "sticker",
            Self::Other => "attachment",
        }
    }
}

/// A normalized attachment extracted from an incoming platform event.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageAttachment {
    pub kind: MessageAttachmentKind,
    pub file_name: Option<String>,
    pub mime_type: Option<String>,
    pub url: Option<String>,
    pub local_path: Option<String>,
    pub size_bytes: Option<u64>,
}

impl MessageAttachment {
    /// Returns the best displayable source for humans: local path first, then URL.
    pub fn display_source(&self) -> Option<&str> {
        self.local_path
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| self.url.as_deref().filter(|value| !value.trim().is_empty()))
    }

    /// Returns a source that the vision pipeline can actually dereference.
    ///
    /// Contract:
    /// - local cached files are always preferred
    /// - remote sources must be explicit `http(s)` URLs
    /// - opaque transport placeholders like `signal://...` are rejected
    pub fn vision_source(&self) -> Option<&str> {
        self.local_path
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                self.url.as_deref().filter(|value| {
                    let trimmed = value.trim();
                    !trimmed.is_empty()
                        && (trimmed.starts_with("http://") || trimmed.starts_with("https://"))
                })
            })
    }
}

/// An incoming message from any platform.
#[derive(Debug, Clone)]
pub struct IncomingMessage {
    /// Which platform this came from
    pub platform: Platform,
    /// Platform-specific user identifier
    pub user_id: String,
    /// Optional channel/group identifier (DMs have None)
    pub channel_id: Option<String>,
    /// Whether this is a DM, group, or channel message.
    pub chat_type: ChatType,
    /// The message text
    pub text: String,
    /// Optional thread/topic ID for threaded conversations
    pub thread_id: Option<String>,
    /// Platform-specific metadata (sticker IDs, attachment URLs, etc.)
    pub metadata: MessageMetadata,
}

impl IncomingMessage {
    /// Returns `true` if this message is a slash command (starts with `/`).
    pub fn is_command(&self) -> bool {
        self.text.trim_start().starts_with('/')
    }

    /// Returns the command name without the leading `/`, lowercased.
    ///
    /// E.g. `/New` → `"new"`, `/Help me` → `"help"`.
    /// Returns `None` for non-command messages.
    pub fn get_command(&self) -> Option<&str> {
        if !self.is_command() {
            return None;
        }
        let trimmed = self.text.trim_start().trim_start_matches('/');
        // Command ends at the first whitespace
        let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
        // Strip optional @botname suffix (e.g. /new@MyBot)
        let end = trimmed[..end].find('@').map_or(end, |pos| pos);
        Some(&trimmed[..end])
    }

    /// Returns the text after the command name (the arguments).
    pub fn get_command_args(&self) -> &str {
        if !self.is_command() {
            return &self.text;
        }
        let trimmed = self.text.trim_start();
        // Skip past command word
        let after_slash = trimmed.trim_start_matches('/');
        let word_end = after_slash
            .find(char::is_whitespace)
            .unwrap_or(after_slash.len());
        after_slash[word_end..].trim_start()
    }
}

/// An outgoing response to deliver back to a platform.
#[derive(Debug, Clone)]
pub struct OutgoingMessage {
    /// The response text to send
    pub text: String,
    /// Metadata needed for delivery (reply_to, channel, etc.)
    pub metadata: MessageMetadata,
}

/// Platform-specific metadata that travels with messages.
#[derive(Debug, Clone, Default)]
pub struct MessageMetadata {
    /// Platform-native message ID (for reply threading)
    pub message_id: Option<String>,
    /// Channel/chat ID for delivery routing
    pub channel_id: Option<String>,
    /// Thread/topic ID
    pub thread_id: Option<String>,
    /// User display name (for logging)
    pub user_display_name: Option<String>,
    /// Structured media and file inputs attached to the message.
    pub attachments: Vec<MessageAttachment>,
    /// Hermes-style webhook delivery metadata used to route final responses.
    pub webhook_delivery: Option<WebhookDelivery>,
    /// Session-scoped skills to preload before the next turn executes.
    pub preloaded_skills: Vec<String>,
}

/// Extra routing metadata for webhook-originated sessions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WebhookDelivery {
    pub deliver: String,
    pub deliver_extra: BTreeMap<String, String>,
}

/// Unified interface for all messaging platform adapters.
///
/// Each platform implements this trait. The gateway starts all enabled
/// adapters, collects IncomingMessages via the mpsc channel, and routes
/// responses back through `send()`.
#[async_trait]
pub trait PlatformAdapter: Send + Sync + 'static {
    /// Platform identifier (matches the Platform enum)
    fn platform(&self) -> Platform;

    /// Start listening for incoming messages.
    ///
    /// The adapter should push IncomingMessages to `tx` and block until
    /// shutdown. Called inside a tokio::spawn, so it runs concurrently.
    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()>;

    /// Send a response back to the platform.
    async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()>;

    /// Platform-specific response formatting (e.g., MarkdownV2 for Telegram).
    fn format_response(&self, text: &str, metadata: &MessageMetadata) -> String;

    /// Maximum message length for this platform (e.g., 4096 for Telegram).
    fn max_message_length(&self) -> usize;

    /// Whether the platform supports Markdown formatting.
    fn supports_markdown(&self) -> bool;

    /// Whether the platform supports inline images.
    fn supports_images(&self) -> bool;

    /// Whether the platform supports file attachments.
    fn supports_files(&self) -> bool;

    /// Whether this adapter can edit an already-sent message in-place.
    ///
    /// WHY optional: Most platforms (WhatsApp, Signal, SMS, Email) do not
    /// support editing a sent message. Returning `false` here tells the
    /// stream consumer to skip intermediate edits and only send the final
    /// response as a new message, preventing a flood of partial updates.
    ///
    /// Platforms that return `true` MUST implement `edit_message()`.
    fn supports_editing(&self) -> bool {
        false
    }

    /// Edit an already-sent message in-place.
    ///
    /// `message_id`  — the platform-native ID returned by a prior `send()`.
    /// `metadata`    — routing metadata (channel, thread, etc.).
    /// `new_text`    — the full updated text to replace the existing message with.
    ///
    /// Returns the same (or updated) `message_id` on success, or an error if
    /// editing is not supported / the API call failed.
    ///
    /// Only called when `supports_editing()` returns `true`.  Platforms SHOULD
    /// implement this consistently: success iff the edit was acknowledged by
    /// the remote API.  The stream consumer treats any error as "editing not
    /// supported" and falls back to a plain final `send()`.
    async fn edit_message(
        &self,
        message_id: &str,
        metadata: &MessageMetadata,
        new_text: &str,
    ) -> anyhow::Result<String> {
        let _ = (message_id, metadata, new_text);
        anyhow::bail!("edit_message not supported by {:?}", self.platform())
    }

    /// Send a short status/progress notification.
    ///
    /// Used by the event processor to forward tool-execution progress to users.
    /// The default implementation delegates to `send()`.  Override only if the
    /// platform needs a different delivery path for ephemeral status messages
    /// (e.g. Discord ephemeral messages, Telegram typing indicators).
    async fn send_status(&self, text: &str, metadata: &MessageMetadata) -> anyhow::Result<()> {
        self.send(OutgoingMessage {
            text: text.to_string(),
            metadata: metadata.clone(),
        })
        .await
    }

    /// Send a typing indicator to signal that the agent is working.
    ///
    /// WHY: On most platforms, users see nothing for 10-30 seconds while the
    /// LLM generates a response. A typing indicator dramatically improves
    /// perceived latency. The indicator must be refreshed every ~5 seconds
    /// because most platforms expire it automatically.
    ///
    /// The default implementation is a no-op.  Override in adapters where the
    /// platform supports a typing/chat action API (e.g., Telegram `sendChatAction`
    /// with action=typing, Discord `channel.trigger_typing()`).
    async fn send_typing(&self, metadata: &MessageMetadata) -> anyhow::Result<()> {
        let _ = metadata;
        Ok(())
    }

    /// Send a message and return the platform-native message ID for later editing.
    ///
    /// WHY: The default `send()` method returns `Ok(())` (no message ID), which
    /// is sufficient for platforms that don't support editing.  This variant is
    /// used by the stream consumer on edit-capable platforms to obtain `message_id`
    /// so subsequent `edit_message()` calls can update the text in-place.
    ///
    /// The default implementation falls through to `send()` and returns `None`,
    /// meaning no in-place editing will be attempted.  Override alongside
    /// `supports_editing()` to enable progressive streaming for this adapter.
    async fn send_and_get_id(&self, msg: OutgoingMessage) -> anyhow::Result<Option<String>> {
        self.send(msg).await?;
        Ok(None)
    }

    /// Send a local image file as a photo attachment.
    ///
    /// WHY: When the agent generates or references a local image (e.g. a
    /// chart, screenshot, or tool output), the gateway can deliver it as a
    /// native platform photo rather than a raw file path string.  This
    /// provides a much better UX on image-capable platforms like Telegram
    /// and Discord.
    ///
    /// The default implementation falls back to `send()` with a text-only
    /// description.  Override in adapters that support binary photo uploads.
    ///
    /// `path`    — absolute path to the local image file.
    /// `caption` — optional caption shown below the image.
    async fn send_photo(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        // Fallback: mention the file as text.
        let text = match caption {
            Some(c) => format!("[Image: {}]\n{}", path, c),
            None => format!("[Image: {}]", path),
        };
        self.send(OutgoingMessage {
            text,
            metadata: metadata.clone(),
        })
        .await
    }

    /// Send a local file as a document attachment.
    ///
    /// Default implementation sends a text-only description.
    /// Override in adapters that support raw file uploads.
    ///
    /// `path`    — absolute path to the local file.
    /// `caption` — optional caption shown below the document.
    async fn send_document(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        let text = match caption {
            Some(c) => format!("[File: {}]\n{}", path, c),
            None => format!("[File: {}]", path),
        };
        self.send(OutgoingMessage {
            text,
            metadata: metadata.clone(),
        })
        .await
    }

    /// Send a local audio file as a voice or audio attachment.
    ///
    /// The default implementation falls back to `send_document()` so delivery
    /// stays reliable even on platforms without native audio uploads.
    async fn send_voice(
        &self,
        path: &str,
        caption: Option<&str>,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        self.send_document(path, caption, metadata).await
    }
}

/// Build the `*** ATTACHED IMAGES ***` injection block for image attachments.
///
/// WHY here: This is the single authoritative source for the image-attachment
/// prompt format used by EVERY platform adapter routed through the gateway.
/// `run.rs` calls this once per message at the dispatch point, so WhatsApp,
/// Telegram, Slack, Signal, WhatsApp — all benefit without any per-adapter
/// changes. Mirrors the identical block that `app.rs` injects for CLI clipboard
/// pastes, ensuring the VISION_GUIDANCE decision rules fire on every platform.
///
/// Returns `None` when no image attachments are present.
pub fn format_image_attachment_block(attachments: &[MessageAttachment]) -> Option<String> {
    let image_sources: Vec<&str> = attachments
        .iter()
        .filter(|a| a.kind == MessageAttachmentKind::Image)
        .filter_map(MessageAttachment::vision_source)
        .collect();

    if image_sources.is_empty() {
        return None;
    }

    let image_list = image_sources.join(", ");
    let count = image_sources.len();
    Some(format!(
        "*** ATTACHED IMAGES - ACTION REQUIRED ***\n\
         The user has attached {count} image source(s): {image_list}\n\
         You MUST call vision_analyze for EACH image before responding.\n\
         - Use tool: vision_analyze\n\
         - Parameter: image_source = <the source above>\n\
         - DO NOT use browser_vision (that captures web pages, not user image inputs)\n\
         - If the source is a local file, DO NOT use read_file on it (binary file)\n\
         *** END ATTACHED IMAGES ***"
    ))
}

/// A media reference parsed from an LLM response.
///
/// When the agent includes `[MEDIA:path]` or `[IMAGE:path]` tags in its reply,
/// the gateway extracts them, sends the files via platform-native APIs, and
/// strips the tags from the text delivered to the user.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaRef {
    /// Absolute or workspace-relative path to the file.
    pub path: String,
    /// True when the extension suggests an image (jpg/png/gif/webp/svg).
    pub is_image: bool,
}

impl MediaRef {
    /// Check whether the file extension belongs to a common image format.
    pub fn detect_image(path: &str) -> bool {
        let p = path.to_ascii_lowercase();
        p.ends_with(".jpg")
            || p.ends_with(".jpeg")
            || p.ends_with(".png")
            || p.ends_with(".gif")
            || p.ends_with(".webp")
            || p.ends_with(".svg")
            || p.ends_with(".bmp")
    }

    /// Check whether the file extension belongs to a common audio format.
    pub fn detect_audio(path: &str) -> bool {
        let p = path.to_ascii_lowercase();
        p.ends_with(".ogg")
            || p.ends_with(".opus")
            || p.ends_with(".mp3")
            || p.ends_with(".wav")
            || p.ends_with(".m4a")
            || p.ends_with(".aac")
            || p.ends_with(".flac")
    }
}

/// Parse media/file references embedded in a response string.
///
/// Returns `(cleaned_text, Vec<MediaRef>)` where `cleaned_text` is the response
/// with all media tags removed and `Vec<MediaRef>` contains the extracted refs.
///
/// WHY: Agents can only return text.  We use an inline tag protocol so the agent
/// can signal *"send this file"* without needing a separate tool call.  The
/// gateway is the single point that translates these tags into platform uploads.
///
/// Tag formats recognised:
/// - `[MEDIA:/path/to/file.pdf]`
/// - `[IMAGE:/path/to/photo.png]`
/// - `[FILE:/path/to/report.txt]`
/// - `MEDIA:/path/to/file.pdf`
/// - `IMAGE:"/path/with spaces/photo.png"`
pub fn extract_media_from_response(text: &str) -> (String, Vec<MediaRef>) {
    let (cleaned, mut refs) = extract_bracketed_media_from_response(text);
    let (cleaned, raw_refs) = extract_raw_media_from_response(&cleaned);
    refs.extend(raw_refs);
    let (cleaned, local_refs) = extract_local_files_from_response(&cleaned);
    refs.extend(local_refs);
    dedupe_media_refs(&mut refs);
    (normalize_media_cleaned_text(&cleaned), refs)
}

pub fn response_text_after_media_extraction(
    original: &str,
    cleaned: &str,
    media_refs: &[MediaRef],
) -> Option<String> {
    let cleaned = cleaned.trim();
    if !cleaned.is_empty() {
        return Some(cleaned.to_string());
    }
    if media_refs.is_empty() {
        let original = original.trim();
        if !original.is_empty() {
            return Some(original.to_string());
        }
    }
    None
}

fn extract_bracketed_media_from_response(text: &str) -> (String, Vec<MediaRef>) {
    let mut refs = Vec::new();
    let mut cleaned = String::with_capacity(text.len());
    let mut remaining = text;

    while let Some(open) = remaining.find('[') {
        let after_bracket = &remaining[open + 1..];
        let prefix_end = parse_media_prefix(after_bracket);

        if let Some((prefix_len, _tag)) = prefix_end {
            let rest = &after_bracket[prefix_len..];
            if let Some(close) = rest.find(']') {
                let path = rest[..close].trim().to_string();
                if !path.is_empty() {
                    refs.push(MediaRef {
                        is_image: MediaRef::detect_image(&path),
                        path,
                    });
                }
                cleaned.push_str(&remaining[..open]);
                let consumed = open + 1 + after_bracket.len() - rest.len() + close + 1;
                remaining = &remaining[consumed..];
                continue;
            }
        }

        cleaned.push_str(&remaining[..open + 1]);
        remaining = &remaining[open + 1..];
    }

    cleaned.push_str(remaining);
    (cleaned, refs)
}

fn extract_raw_media_from_response(text: &str) -> (String, Vec<MediaRef>) {
    let mut refs = Vec::new();
    let mut cleaned = String::with_capacity(text.len());
    let mut index = 0usize;

    while index < text.len() {
        let remaining = &text[index..];
        if let Some((prefix_len, tag)) = parse_media_prefix(remaining) {
            let prev_char = text[..index].chars().next_back();
            if is_media_boundary(prev_char) {
                let after_prefix = &remaining[prefix_len..];
                if let Some((path, consumed, suffix)) = parse_raw_media_path(after_prefix) {
                    refs.push(MediaRef {
                        is_image: tag == "IMAGE" || MediaRef::detect_image(&path),
                        path,
                    });
                    cleaned.push_str(&suffix);
                    index += prefix_len + consumed;
                    continue;
                }
            }
        }

        if let Some(ch) = remaining.chars().next() {
            cleaned.push(ch);
            index += ch.len_utf8();
        } else {
            break;
        }
    }

    (cleaned, refs)
}

fn parse_media_prefix(input: &str) -> Option<(usize, &'static str)> {
    for (prefix, tag) in [("MEDIA:", "MEDIA"), ("IMAGE:", "IMAGE"), ("FILE:", "FILE")] {
        if input
            .get(..prefix.len())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
        {
            return Some((prefix.len(), tag));
        }
    }
    None
}

fn is_media_boundary(prev_char: Option<char>) -> bool {
    prev_char
        .is_none_or(|ch| ch.is_whitespace() || matches!(ch, '(' | '[' | '{' | '<' | '"' | '\''))
}

fn parse_raw_media_path(input: &str) -> Option<(String, usize, String)> {
    let mut chars = input.char_indices();
    let (_, first) = chars.next()?;

    if matches!(first, '"' | '\'') {
        let quote = first;
        let mut closing = None;
        for (idx, ch) in chars {
            if ch == quote {
                closing = Some(idx);
                break;
            }
        }
        let closing = closing?;
        let path = input[1..closing].trim().to_string();
        if path.is_empty() {
            return None;
        }
        return Some((path, closing + quote.len_utf8(), String::new()));
    }

    let end = input
        .char_indices()
        .find(|(_, ch)| ch.is_whitespace())
        .map(|(idx, _)| idx)
        .unwrap_or(input.len());
    if end == 0 {
        return None;
    }

    let token = &input[..end];
    let trimmed_end = token.trim_end_matches(['.', ',', ';', '!', '?']).len();
    let path = token[..trimmed_end].trim().to_string();
    if path.is_empty() {
        return None;
    }

    Some((path, end, token[trimmed_end..].to_string()))
}

fn normalize_media_cleaned_text(text: &str) -> String {
    text.replace("\n\n\n", "\n\n").trim().to_string()
}

fn dedupe_media_refs(refs: &mut Vec<MediaRef>) {
    let mut unique = Vec::with_capacity(refs.len());
    for media_ref in refs.drain(..) {
        if unique
            .iter()
            .any(|existing: &MediaRef| existing.path == media_ref.path)
        {
            continue;
        }
        unique.push(media_ref);
    }
    *refs = unique;
}

fn extract_local_files_from_response(text: &str) -> (String, Vec<MediaRef>) {
    let mut refs = Vec::new();
    let mut cleaned = text.to_string();
    let mut raw_matches = Vec::new();

    for capture in local_file_regex().captures_iter(text) {
        let Some(matched) = capture.get(0) else {
            continue;
        };
        if !is_media_boundary(text[..matched.start()].chars().next_back()) {
            continue;
        }
        if span_inside_code_block(text, matched.start()) {
            continue;
        }

        let raw = matched.as_str();
        let expanded = if let Some(rest) = raw.strip_prefix("~/") {
            match dirs::home_dir() {
                Some(home) => home.join(rest).to_string_lossy().to_string(),
                None => continue,
            }
        } else {
            raw.to_string()
        };

        if !std::fs::metadata(&expanded).is_ok_and(|metadata| metadata.is_file()) {
            continue;
        }

        refs.push(MediaRef {
            is_image: MediaRef::detect_image(&expanded),
            path: expanded,
        });
        raw_matches.push(raw.to_string());
    }

    for raw in raw_matches {
        cleaned = cleaned.replace(&raw, "");
    }

    (normalize_media_cleaned_text(&cleaned), refs)
}

fn local_file_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r"(?ix)
            (?:~/|/)
            (?:[\w.\-]+/)*
            [\w.\-]+\.
            (?:png|jpe?g|gif|webp|svg|bmp|heic|heif|
               mp4|mov|avi|mkv|webm|3gp|
               ogg|opus|mp3|wav|m4a|aac|
               pdf|txt|md|csv|json|zip|docx?|xlsx?|pptx?)
            \b",
        )
        .expect("valid local file regex")
    })
}

fn span_inside_code_block(text: &str, position: usize) -> bool {
    fenced_code_spans(text)
        .into_iter()
        .chain(inline_code_spans(text))
        .any(|(start, end)| start <= position && position < end)
}

fn fenced_code_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut search_start = 0usize;
    while let Some(open_rel) = text[search_start..].find("```") {
        let open = search_start + open_rel;
        let after_open = open + 3;
        let Some(close_rel) = text[after_open..].find("```") else {
            break;
        };
        let close = after_open + close_rel + 3;
        spans.push((open, close));
        search_start = close;
    }
    spans
}

fn inline_code_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut current = None;
    for (index, ch) in text.char_indices() {
        if ch == '`' {
            if let Some(start) = current.take() {
                spans.push((start, index + ch.len_utf8()));
            } else {
                current = Some(index);
            }
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incoming_message_default_metadata() {
        let msg = IncomingMessage {
            platform: Platform::Cli,
            user_id: "user1".into(),
            channel_id: None,
            chat_type: ChatType::Dm,
            text: "hello".into(),
            thread_id: None,
            metadata: MessageMetadata::default(),
        };
        assert_eq!(msg.text, "hello");
        assert!(msg.metadata.message_id.is_none());
        assert!(msg.metadata.attachments.is_empty());
    }

    #[test]
    fn outgoing_message_construction() {
        let msg = OutgoingMessage {
            text: "response".into(),
            metadata: MessageMetadata {
                channel_id: Some("ch123".into()),
                ..Default::default()
            },
        };
        assert_eq!(msg.text, "response");
        assert_eq!(msg.metadata.channel_id.as_deref(), Some("ch123"));
    }

    #[test]
    fn attachment_kind_labels() {
        assert_eq!(MessageAttachmentKind::Image.label(), "image");
        assert_eq!(MessageAttachmentKind::Document.label(), "document");
        assert_eq!(MessageAttachmentKind::Other.label(), "attachment");
    }

    #[test]
    fn attachment_vision_source_prefers_local_path() {
        let attachment = MessageAttachment {
            local_path: Some("/tmp/cached.png".into()),
            url: Some("https://example.com/image.png".into()),
            ..Default::default()
        };
        assert_eq!(attachment.vision_source(), Some("/tmp/cached.png"));
    }

    #[test]
    fn attachment_vision_source_accepts_https_url() {
        let attachment = MessageAttachment {
            url: Some("https://example.com/image.png".into()),
            ..Default::default()
        };
        assert_eq!(
            attachment.vision_source(),
            Some("https://example.com/image.png")
        );
    }

    #[test]
    fn attachment_vision_source_rejects_opaque_scheme() {
        let attachment = MessageAttachment {
            url: Some("signal://attachment/abc123".into()),
            ..Default::default()
        };
        assert_eq!(attachment.vision_source(), None);
    }

    // ─── format_image_attachment_block ───────────────────────────────────

    fn make_image_attachment(path: &str) -> MessageAttachment {
        MessageAttachment {
            kind: MessageAttachmentKind::Image,
            local_path: Some(path.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn image_block_none_when_no_attachments() {
        assert!(format_image_attachment_block(&[]).is_none());
    }

    #[test]
    fn image_block_none_when_no_image_attachments() {
        let attachments = vec![MessageAttachment {
            kind: MessageAttachmentKind::Document,
            local_path: Some("/tmp/report.pdf".into()),
            ..Default::default()
        }];
        assert!(format_image_attachment_block(&attachments).is_none());
    }

    #[test]
    fn image_block_contains_path_and_instructions() {
        let attachments = vec![make_image_attachment("/home/user/.edgecrab/images/img.png")];
        let block = format_image_attachment_block(&attachments).expect("should return Some");
        assert!(block.contains("ATTACHED IMAGES"), "must contain marker");
        assert!(block.contains("vision_analyze"), "must name vision_analyze");
        assert!(
            block.contains("/home/user/.edgecrab/images/img.png"),
            "must contain path"
        );
        assert!(
            block.contains("DO NOT use browser_vision"),
            "must warn against browser_vision"
        );
    }

    #[test]
    fn image_block_rejects_opaque_transport_attachment_urls() {
        let attachments = vec![MessageAttachment {
            kind: MessageAttachmentKind::Image,
            local_path: None,
            url: Some("signal://attachment/abc123".into()),
            ..Default::default()
        }];
        assert!(format_image_attachment_block(&attachments).is_none());
    }

    #[test]
    fn image_block_accepts_https_image_urls() {
        let attachments = vec![MessageAttachment {
            kind: MessageAttachmentKind::Image,
            url: Some("https://example.com/img.png".into()),
            ..Default::default()
        }];
        let block = format_image_attachment_block(&attachments).expect("some");
        assert!(block.contains("https://example.com/img.png"));
    }

    #[test]
    fn image_block_lists_multiple_paths() {
        let attachments = vec![
            make_image_attachment("/tmp/a.png"),
            make_image_attachment("/tmp/b.jpg"),
        ];
        let block = format_image_attachment_block(&attachments).expect("some");
        assert!(block.contains("/tmp/a.png"));
        assert!(block.contains("/tmp/b.jpg"));
        assert!(block.contains("2 image source(s)"));
    }

    #[test]
    fn image_block_excludes_document_attachments_from_block() {
        // Mix of image + document: only the image path must appear in the block
        let attachments = vec![
            make_image_attachment("/tmp/photo.jpg"),
            MessageAttachment {
                kind: MessageAttachmentKind::Document,
                local_path: Some("/tmp/report.pdf".into()),
                ..Default::default()
            },
        ];
        let block = format_image_attachment_block(&attachments).expect("some");
        assert!(
            block.contains("/tmp/photo.jpg"),
            "image path must be in block"
        );
        assert!(
            !block.contains("/tmp/report.pdf"),
            "document must NOT appear in image block"
        );
        assert!(
            block.contains("1 image source(s)"),
            "count must be 1, not 2"
        );
    }

    #[test]
    fn image_block_handles_whatsapp_image_cache_path() {
        // WhatsApp Baileys bridge saves images to ~/.edgecrab/image_cache/
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let path = format!("{home}/.edgecrab/image_cache/img_001.jpg");
        let attachments = vec![make_image_attachment(&path)];
        let block = format_image_attachment_block(&attachments).expect("some");
        assert!(
            block.contains(&path),
            "WhatsApp image_cache path must appear in block"
        );
        assert!(
            block.contains("vision_analyze"),
            "must instruct vision_analyze"
        );
    }

    #[test]
    fn image_block_handles_telegram_gateway_media_path() {
        // Telegram adapter saves images to ~/.edgecrab/gateway_media/telegram/
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let path = format!("{home}/.edgecrab/gateway_media/telegram/photo_001.jpg");
        let attachments = vec![make_image_attachment(&path)];
        let block = format_image_attachment_block(&attachments).expect("some");
        assert!(
            block.contains(&path),
            "Telegram gateway_media path must appear in block"
        );
        assert!(
            block.contains("vision_analyze"),
            "must instruct vision_analyze"
        );
    }

    // ─── extract_media_from_response ─────────────────────────────────────

    #[test]
    fn extract_media_no_tags_returns_unchanged() {
        let text = "Hello, world!";
        let (cleaned, refs) = extract_media_from_response(text);
        assert_eq!(cleaned, text);
        assert!(refs.is_empty());
    }

    #[test]
    fn extract_media_single_image_tag() {
        let text = "Here is your chart: [IMAGE:/tmp/chart.png]\nEnjoy!";
        let (cleaned, refs) = extract_media_from_response(text);
        assert!(!cleaned.contains("[IMAGE:"), "tag should be stripped");
        assert!(cleaned.contains("Enjoy!"), "surrounding text must remain");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "/tmp/chart.png");
        assert!(refs[0].is_image);
    }

    #[test]
    fn extract_media_single_file_tag() {
        let text = "Report ready: [FILE:/tmp/report.pdf]";
        let (cleaned, refs) = extract_media_from_response(text);
        assert!(!cleaned.contains("[FILE:"), "tag should be stripped");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "/tmp/report.pdf");
        assert!(!refs[0].is_image, "pdf is not an image");
    }

    #[test]
    fn extract_media_multiple_tags() {
        let text = "[IMAGE:/tmp/a.png] and [MEDIA:/tmp/b.pdf]";
        let (cleaned, refs) = extract_media_from_response(text);
        assert_eq!(refs.len(), 2);
        assert!(refs[0].is_image);
        assert!(!refs[1].is_image);
        assert!(!cleaned.contains('['), "both tags stripped");
    }

    #[test]
    fn extract_media_case_insensitive_tags() {
        let text = "[image:/tmp/photo.jpg]";
        let (_, refs) = extract_media_from_response(text);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "/tmp/photo.jpg");
    }

    #[test]
    fn extract_media_raw_file_tag() {
        let text = "Send this now: MEDIA:/tmp/report.pdf";
        let (cleaned, refs) = extract_media_from_response(text);
        assert_eq!(cleaned, "Send this now:");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "/tmp/report.pdf");
        assert!(!refs[0].is_image);
    }

    #[test]
    fn extract_media_raw_quoted_image_tag_preserves_punctuation() {
        let text = "Attached IMAGE:\"/tmp/chart final.png\".";
        let (cleaned, refs) = extract_media_from_response(text);
        assert_eq!(cleaned, "Attached .");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "/tmp/chart final.png");
        assert!(refs[0].is_image);
    }

    #[test]
    fn response_text_after_media_extraction_omits_pure_media_messages() {
        let (cleaned, refs) = extract_media_from_response("MEDIA:/tmp/report.pdf");
        let text = response_text_after_media_extraction("MEDIA:/tmp/report.pdf", &cleaned, &refs);
        assert!(text.is_none());
        assert_eq!(refs.len(), 1);
    }

    // Local bare-file detection uses a /... or ~/... regex that only matches
    // Unix-style absolute paths. On Windows, TempDir paths use C:\... which
    // the regex does not cover; the MEDIA: tag (tested elsewhere) is the
    // cross-platform media dispatch mechanism.
    #[cfg(not(windows))]
    #[test]
    fn extract_media_detects_existing_bare_local_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("report.pdf");
        std::fs::write(&path, b"edgecrab").expect("write");
        let input = format!("Please send {}", path.display());

        let (cleaned, refs) = extract_media_from_response(&input);

        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, path.to_string_lossy());
        assert!(!refs[0].is_image);
        assert_eq!(cleaned, "Please send");
    }

    #[test]
    fn extract_media_handles_unicode_without_panicking() {
        let input = "Raphaël! (◕‿◕)★ I'm here and ready — how can I help?";
        let (cleaned, refs) = extract_media_from_response(input);
        assert_eq!(cleaned, input);
        assert!(refs.is_empty());
    }

    #[test]
    fn extract_media_handles_unicode_before_raw_tag() {
        let input = "Raphaël — here you go ★ MEDIA:/tmp/report.pdf";
        let (cleaned, refs) = extract_media_from_response(input);
        assert_eq!(cleaned, "Raphaël — here you go ★");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "/tmp/report.pdf");
    }

    #[test]
    fn extract_media_ignores_local_paths_inside_code_blocks() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("report.pdf");
        std::fs::write(&path, b"edgecrab").expect("write");
        let input = format!("```sh\ncat {}\n```", path.display());

        let (cleaned, refs) = extract_media_from_response(&input);

        assert!(refs.is_empty());
        assert_eq!(cleaned, input);
    }

    #[test]
    fn detect_image_extensions() {
        assert!(MediaRef::detect_image("/tmp/a.png"));
        assert!(MediaRef::detect_image("/tmp/b.JPG")); // upper-case
        assert!(MediaRef::detect_image("/tmp/c.webp"));
        assert!(!MediaRef::detect_image("/tmp/d.pdf"));
        assert!(!MediaRef::detect_image("/tmp/e.txt"));
    }

    #[test]
    fn detect_audio_extensions() {
        assert!(MediaRef::detect_audio("/tmp/reply.ogg"));
        assert!(MediaRef::detect_audio("/tmp/reply.mp3"));
        assert!(!MediaRef::detect_audio("/tmp/reply.png"));
    }
}
