//! # Delivery router — format and deliver responses to platforms
//!
//! WHY message splitting: Platforms have strict message length limits
//! (Telegram: 4096 chars, Discord: 2000 chars). The delivery router
//! splits long responses at natural boundaries (paragraph, sentence)
//! to avoid mid-word breaks.
//!
//! ```text
//!   DeliveryRouter
//!     ├── deliver()       → format + split + send via adapter
//!     ├── split_message() → break at paragraph/sentence boundaries
//!     └── adapters        → HashMap<Platform, Arc<dyn PlatformAdapter>>
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use edgecrab_types::Platform;

use crate::platform::{
    MediaRef, MessageMetadata, OutgoingMessage, PlatformAdapter, extract_media_from_response,
    response_text_after_media_extraction,
};

/// Routes formatted responses to the correct platform adapter.
pub struct DeliveryRouter {
    adapters: HashMap<Platform, Arc<dyn PlatformAdapter>>,
}

impl DeliveryRouter {
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
        }
    }

    /// Register a platform adapter for delivery.
    pub fn register(&mut self, adapter: Arc<dyn PlatformAdapter>) {
        self.adapters.insert(adapter.platform(), adapter);
    }

    /// Deliver a response to the specified platform.
    ///
    /// Extracts any `MEDIA:/path` references first: image paths are sent via
    /// `send_photo`, audio via `send_voice`, other files via `send_document`.
    /// The remaining text (if any) is formatted, split to fit the platform's
    /// message length limit, and sent as plain text chunks.
    ///
    /// WHY media-aware here: the DeliveryRouter is the single dispatch point for
    /// all outbound messages — both gateway agent replies (run.rs) and the
    /// `send_message` tool (via GatewaySenderBridge). Centralising MEDIA:
    /// extraction here ensures neither path silently sends a raw file path as
    /// text.
    pub async fn deliver(
        &self,
        response: &str,
        platform: Platform,
        metadata: &MessageMetadata,
    ) -> anyhow::Result<()> {
        let adapter = self
            .adapters
            .get(&platform)
            .ok_or_else(|| anyhow::anyhow!("No adapter registered for {:?}", platform))?;

        let (cleaned, media_refs) = extract_media_from_response(response);

        // Send text portion (skip if the response was entirely media)
        if let Some(text) = response_text_after_media_extraction(response, &cleaned, &media_refs) {
            let formatted = adapter.format_response(&text, metadata);
            let max_len = adapter.max_message_length();

            if formatted.len() > max_len {
                let chunks = split_message(&formatted, max_len);
                for (i, chunk) in chunks.iter().enumerate() {
                    adapter
                        .send(OutgoingMessage {
                            text: chunk.clone(),
                            metadata: metadata.clone(),
                        })
                        .await?;
                    if i < chunks.len() - 1 {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            } else {
                adapter
                    .send(OutgoingMessage {
                        text: formatted,
                        metadata: metadata.clone(),
                    })
                    .await?;
            }
        }

        // Send media attachments
        for mref in &media_refs {
            let result = if mref.is_image {
                adapter.send_photo(&mref.path, None, metadata).await
            } else if MediaRef::detect_audio(&mref.path) {
                adapter.send_voice(&mref.path, None, metadata).await
            } else {
                adapter.send_document(&mref.path, None, metadata).await
            };
            if let Err(e) = result {
                tracing::warn!(
                    path = %mref.path,
                    error = %e,
                    "media delivery failed — falling back to path in text"
                );
            }
        }

        Ok(())
    }

    /// Check if a platform adapter is registered.
    pub fn has_adapter(&self, platform: &Platform) -> bool {
        self.adapters.contains_key(platform)
    }

    /// Return the list of platforms that have registered adapters.
    pub fn list_platforms(&self) -> Vec<Platform> {
        self.adapters.keys().cloned().collect()
    }
}

impl Default for DeliveryRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// Split a message into chunks that fit within `max_len`.
///
/// WHY paragraph-first: Splitting at paragraph breaks (double newline)
/// preserves formatting better than arbitrary cuts. Falls back to
/// single newline, then space, then hard cut as last resort.
pub fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            chunks.push(remaining.to_string());
            break;
        }

        // Try to split at a natural boundary within max_len
        let cut_at = find_split_point(remaining, max_len);
        let (chunk, rest) = remaining.split_at(cut_at);
        chunks.push(chunk.trim_end().to_string());
        remaining = rest.trim_start();
    }

    chunks
}

/// Find the best split point within `max_len` bytes.
///
/// Priority: paragraph break > newline > space > hard cut.
fn find_split_point(text: &str, max_len: usize) -> usize {
    let safe_max = (0..=max_len)
        .rev()
        .find(|&i| text.is_char_boundary(i))
        .unwrap_or(0);
    let search_window = &text[..safe_max];

    // Try paragraph break (double newline)
    if let Some(pos) = search_window.rfind("\n\n") {
        if pos > 0 {
            return pos + 1; // Include one newline
        }
    }

    // Try single newline
    if let Some(pos) = search_window.rfind('\n') {
        if pos > 0 {
            return pos + 1;
        }
    }

    // Try space
    if let Some(pos) = search_window.rfind(' ') {
        if pos > 0 {
            return pos + 1;
        }
    }

    // Hard cut at max_len (ensure valid UTF-8 boundary)
    let mut cut = max_len;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_short_message_no_split() {
        let chunks = split_message("hello world", 100);
        assert_eq!(chunks, vec!["hello world"]);
    }

    #[test]
    fn split_at_paragraph_boundary() {
        let text = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.";
        let chunks = split_message(text, 30);
        assert!(chunks.len() >= 2);
        assert!(chunks[0].contains("First"));
    }

    #[test]
    fn split_at_newline() {
        let text = "Line one\nLine two\nLine three\nLine four";
        let chunks = split_message(text, 20);
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(chunk.len() <= 20, "chunk too long: {}", chunk.len());
        }
    }

    #[test]
    fn split_at_space() {
        let text = "word1 word2 word3 word4 word5 word6 word7 word8";
        let chunks = split_message(text, 15);
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn split_hard_cut_on_long_word() {
        let text = "a".repeat(100);
        let chunks = split_message(&text, 30);
        assert!(chunks.len() >= 4);
        // First chunk should be exactly 30
        assert_eq!(chunks[0].len(), 30);
    }

    #[test]
    fn delivery_router_default_empty() {
        let router = DeliveryRouter::new();
        assert!(!router.has_adapter(&Platform::Telegram));
    }
}
