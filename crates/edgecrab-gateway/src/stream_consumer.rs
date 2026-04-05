//! # Gateway stream consumer — progressive message delivery with streamed tokens
//!
//! ## Why this exists
//!
//! Without streaming, users stare at a blank screen for 10-30 s while the LLM
//! generates a response and runs tool calls.  Progressive delivery dramatically
//! improves perceived latency.
//!
//! ## Two delivery modes
//!
//! ```text
//!   ┌─────────────────────────────────────────────────────┐
//!   │  Edit mode  (Telegram, Discord, Slack)              │
//!   │                                                     │
//!   │  send_and_get_id() → message_id                     │
//!   │  edit_message(message_id, ...)  (every 300 ms)  ──► │  user sees live tokens
//!   │  edit_message(message_id, final text)           ──► │  cursor removed
//!   └─────────────────────────────────────────────────────┘
//!
//!   ┌─────────────────────────────────────────────────────┐
//!   │  Batch mode  (WhatsApp, Signal, SMS, Email, …)      │
//!   │                                                     │
//!   │  [accumulate all tokens silently]                   │
//!   │  send(final text)                               ──► │  single response
//!   └─────────────────────────────────────────────────────┘
//! ```
//!
//! The choice of mode is driven by adapter capability plus per-request
//! configuration. This lets the gateway preserve a single event bridge for
//! approvals/clarify even when progressive token delivery is disabled.
//!
//! Mirrors hermes-agent's `gateway/stream_consumer.py`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use crate::platform::{MessageMetadata, OutgoingMessage, PlatformAdapter};

// ─── Config ───────────────────────────────────────────────────────────────

/// Configuration for a single stream consumer instance.
#[derive(Debug, Clone)]
pub struct StreamConsumerConfig {
    /// Minimum interval between consecutive edits (edit mode only).
    pub edit_interval: Duration,
    /// Minimum accumulated chars before triggering an intermediate edit.
    pub buffer_threshold: usize,
    /// Cursor appended to partial messages to signal "still typing".
    pub cursor: String,
    /// Whether live message editing is allowed for this dispatch.
    pub prefer_editing: bool,
}

impl Default for StreamConsumerConfig {
    fn default() -> Self {
        Self {
            edit_interval: Duration::from_millis(300),
            buffer_threshold: 40,
            cursor: " ▉".into(),
            prefer_editing: true,
        }
    }
}

// ─── Wire protocol ────────────────────────────────────────────────────────

/// Sentinel message signalling the stream is complete.
pub enum StreamItem {
    Delta(String),
    Done,
}

// ─── Consumer ─────────────────────────────────────────────────────────────

/// Async consumer that progressively delivers streamed tokens to a platform.
///
/// Create one consumer per incoming message, spawn `consumer.run()` as a
/// `tokio::task`, then feed deltas through the sender returned by
/// `delta_sender()` / `on_delta_callback()`.
pub struct GatewayStreamConsumer {
    adapter: Arc<dyn PlatformAdapter>,
    metadata: MessageMetadata,
    config: StreamConsumerConfig,
    rx: mpsc::Receiver<StreamItem>,
    tx: mpsc::Sender<StreamItem>,
    /// `true` once at least one message has been sent or edited.
    ///
    /// The gateway dispatcher checks this flag after the consumer exits: if
    /// `already_sent == true`, the full response has already been delivered
    /// (edit mode) and a duplicate `deliver()` call must be skipped.
    already_sent: Arc<AtomicBool>,
    /// Last text delivered to the platform — used to skip redundant edits.
    ///
    /// WHY: Token deltas can be tiny (1-2 chars). If two consecutive flush
    /// cycles produce the same display string (e.g. a timer tick with no new
    /// tokens), we skip the edit API call to save quota and avoid throttling.
    last_sent_text: String,
}

impl GatewayStreamConsumer {
    pub fn new(
        adapter: Arc<dyn PlatformAdapter>,
        metadata: MessageMetadata,
        config: StreamConsumerConfig,
    ) -> Self {
        let (tx, rx) = mpsc::channel(512);
        Self {
            adapter,
            metadata,
            config,
            rx,
            tx,
            already_sent: Arc::new(AtomicBool::new(false)),
            last_sent_text: String::new(),
        }
    }

    /// Returns a sender handle for pushing deltas (clone-able, `Send + Sync`).
    pub fn delta_sender(&self) -> mpsc::Sender<StreamItem> {
        self.tx.clone()
    }

    /// Returns a closure suitable for passing as a sync delta callback.
    pub fn on_delta_callback(&self) -> Box<dyn Fn(String) + Send + Sync> {
        let tx = self.tx.clone();
        Box::new(move |text: String| {
            let _ = tx.try_send(StreamItem::Delta(text));
        })
    }

    /// Signal that the stream is complete.
    pub fn finish(&self) {
        let _ = self.tx.try_send(StreamItem::Done);
    }

    /// Returns `true` if at least one message was sent or edited.
    pub fn already_sent(&self) -> bool {
        self.already_sent.load(Ordering::Relaxed)
    }

    /// Returns an `Arc` to the `already_sent` flag for external inspection.
    pub fn already_sent_flag(&self) -> Arc<AtomicBool> {
        self.already_sent.clone()
    }

    /// Run the consumer until the stream is done.
    ///
    /// Selects the delivery mode based on `adapter.supports_editing()` plus
    /// the per-dispatch `prefer_editing` flag:
    /// - **Edit mode**: sends an initial placeholder, then edits as tokens arrive.
    /// - **Batch mode**: accumulates silently, sends a single message at the end.
    pub async fn run(self) {
        if self.config.prefer_editing && self.adapter.supports_editing() {
            self.run_edit_mode().await;
        } else {
            self.run_batch_mode().await;
        }
    }

    // ── Edit mode ─────────────────────────────────────────────────────────

    async fn run_edit_mode(mut self) {
        let safe_limit = self
            .adapter
            .max_message_length()
            .saturating_sub(self.config.cursor.len() + 100)
            .max(500);

        let mut accumulated = String::new();
        let mut last_edit_time = Instant::now();
        let mut message_id: Option<String> = None;
        // When false, editing failed — fall back to batch delivery at the end.
        let mut edit_supported = true;

        loop {
            let (new_tokens, got_done) = self.drain_channel().await;
            accumulated.push_str(&new_tokens);

            let elapsed = last_edit_time.elapsed();
            let should_flush = got_done
                || (elapsed >= self.config.edit_interval && !accumulated.is_empty())
                || (accumulated.len() >= self.config.buffer_threshold);

            if should_flush && !accumulated.is_empty() && edit_supported {
                // Overflow: accumulated text exceeds the platform limit —
                // finalise the current bubble and open a new one.
                while accumulated.len() > safe_limit {
                    let split_at = accumulated[..safe_limit]
                        .rfind('\n')
                        .filter(|&p| p > safe_limit / 2)
                        .unwrap_or(safe_limit);
                    let chunk: String = accumulated.drain(..split_at).collect();

                    if let Some(ref mid) = message_id {
                        if let Err(e) = self.adapter.edit_message(mid, &self.metadata, &chunk).await
                        {
                            tracing::debug!(error = %e, "edit overflow chunk failed; disabling editing");
                            edit_supported = false;
                            break;
                        }
                    } else {
                        match self
                            .adapter
                            .send_and_get_id(OutgoingMessage {
                                text: chunk,
                                metadata: self.metadata.clone(),
                            })
                            .await
                        {
                            Ok(_id) => {
                                // Chunk delivered as its own bubble; continue loop.
                            }
                            Err(e) => {
                                tracing::debug!(error = %e, "initial send for overflow failed");
                                edit_supported = false;
                                break;
                            }
                        }
                    }
                    // After this overflow chunk, start fresh (new bubble next flush).
                    message_id = None;
                    self.already_sent.store(false, Ordering::Relaxed);
                }

                if !edit_supported {
                    break;
                }

                let display = if got_done {
                    accumulated.clone()
                } else {
                    format!("{}{}", accumulated, self.config.cursor)
                };

                if let Some(ref mid) = message_id {
                    // Skip the API call when text hasn't changed (saves quota).
                    if display == self.last_sent_text {
                        // nothing to do — identical content
                    } else {
                        match self
                            .adapter
                            .edit_message(mid, &self.metadata, &display)
                            .await
                        {
                            Ok(_) => {
                                self.already_sent.store(true, Ordering::Relaxed);
                                self.last_sent_text = display;
                            }
                            Err(e) => {
                                tracing::debug!(error = %e, "editMessageText failed; disabling editing");
                                edit_supported = false;
                            }
                        }
                    }
                } else {
                    match self
                        .adapter
                        .send_and_get_id(OutgoingMessage {
                            text: display.clone(),
                            metadata: self.metadata.clone(),
                        })
                        .await
                    {
                        Ok(id) => {
                            self.already_sent.store(true, Ordering::Relaxed);
                            self.last_sent_text = display;
                            message_id = id;
                        }
                        Err(e) => {
                            tracing::debug!(error = %e, "initial send failed; disabling editing");
                            edit_supported = false;
                        }
                    }
                }

                last_edit_time = Instant::now();
            }

            if got_done {
                // Final edit: remove cursor from the live message bubble.
                // Extract media tags before the last edit so the delivered text
                // is clean and any referenced files are sent as attachments.
                if edit_supported {
                    let (cleaned, media_refs) =
                        crate::platform::extract_media_from_response(&accumulated);
                    let final_text = if cleaned.is_empty() {
                        accumulated.clone()
                    } else {
                        cleaned
                    };
                    if let (Some(mid), false) = (&message_id, final_text.is_empty()) {
                        if final_text != self.last_sent_text {
                            let _ = self
                                .adapter
                                .edit_message(mid, &self.metadata, &final_text)
                                .await;
                        }
                    } else if message_id.is_none() && !final_text.is_empty() {
                        // Nothing was sent yet (stream completed in one shot)
                        self.send_final(&final_text).await;
                    }
                    self.send_media_attachments(&media_refs).await;
                    return; // delivered
                }
                break; // fall through to batch fallback
            }
        }

        // Batch fallback: editing was disabled after partial streaming.
        // Extract and deliver media before sending the final text.
        if !accumulated.is_empty() {
            let (cleaned, media_refs) = crate::platform::extract_media_from_response(&accumulated);
            let final_text = if cleaned.is_empty() {
                &accumulated
            } else {
                &cleaned
            };
            self.send_final(final_text).await;
            self.send_media_attachments(&media_refs).await;
        }
    }

    // ── Batch mode ────────────────────────────────────────────────────────

    /// Silently accumulate all tokens, then deliver as one message.
    ///
    /// WHY: Platforms like WhatsApp and Signal don't support editing a sent
    /// message.  Sending a new message per token delta would flood the chat.
    async fn run_batch_mode(mut self) {
        let safe_limit = self
            .adapter
            .max_message_length()
            .saturating_sub(100)
            .max(500);
        let mut accumulated = String::new();

        loop {
            let (new_tokens, got_done) = self.drain_channel().await;
            accumulated.push_str(&new_tokens);

            if got_done {
                if !accumulated.is_empty() {
                    // Extract and strip media tags before delivery.
                    let (cleaned, media_refs) =
                        crate::platform::extract_media_from_response(&accumulated);
                    let final_text = if cleaned.is_empty() {
                        &accumulated
                    } else {
                        &cleaned
                    };
                    self.send_chunks(final_text, safe_limit).await;
                    self.send_media_attachments(&media_refs).await;
                }
                return;
            }
        }
    }

    // ── Shared helpers ────────────────────────────────────────────────────

    /// Drain the mpsc channel; block up to 50 ms for the first item, then
    /// non-blocking drain everything else that's already queued.
    async fn drain_channel(&mut self) -> (String, bool) {
        let mut buf = String::new();
        let mut got_done = false;

        match tokio::time::timeout(Duration::from_millis(50), self.rx.recv()).await {
            Ok(Some(StreamItem::Delta(text))) => buf.push_str(&text),
            Ok(Some(StreamItem::Done)) => got_done = true,
            Ok(None) => got_done = true, // channel closed
            Err(_) => {}                 // timeout
        }

        if !got_done {
            while let Ok(item) = self.rx.try_recv() {
                match item {
                    StreamItem::Delta(t) => buf.push_str(&t),
                    StreamItem::Done => {
                        got_done = true;
                        break;
                    }
                }
            }
        }

        (buf, got_done)
    }

    /// Send any media files (images / documents) extracted from the stream.
    ///
    /// Called after the final text has been delivered so media attachments
    /// appear AFTER the primary response bubble.
    async fn send_media_attachments(&self, media_refs: &[crate::platform::MediaRef]) {
        for mref in media_refs {
            let result = if mref.is_image {
                self.adapter
                    .send_photo(&mref.path, None, &self.metadata)
                    .await
            } else {
                self.adapter
                    .send_document(&mref.path, None, &self.metadata)
                    .await
            };
            if let Err(e) = result {
                tracing::warn!(
                    path = %mref.path,
                    error = %e,
                    "streaming media attachment delivery failed"
                );
            }
        }
    }

    /// Send `text` as a single outgoing message.
    async fn send_final(&self, text: &str) {
        if let Err(e) = self.send_with_retry(text, 3).await {
            tracing::debug!(error = %e, "stream consumer batch send failed after retries");
        }
    }

    /// Send `text` with exponential back-off retry for transient failures.
    ///
    /// WHY: Telegram and other platforms occasionally return 429 (Too Many
    /// Requests) or 5xx errors.  Retrying with back-off recovers silently
    /// instead of dropping the user's response.
    ///
    /// Strategy: up to `max_retries` attempts, doubling delay each time
    /// (2 s → 4 s → 8 s), capped at 30 s.  Non-retryable errors (4xx except
    /// 429) fail immediately.
    async fn send_with_retry(&self, text: &str, max_retries: u32) -> anyhow::Result<()> {
        let mut delay = Duration::from_secs(2);
        let mut last_err: Option<anyhow::Error> = None;

        for attempt in 0..=max_retries {
            let msg = OutgoingMessage {
                text: text.to_string(),
                metadata: self.metadata.clone(),
            };
            match self.adapter.send(msg).await {
                Ok(()) => {
                    self.already_sent.store(true, Ordering::Relaxed);
                    return Ok(());
                }
                Err(e) => {
                    let err_str = e.to_string().to_ascii_lowercase();
                    // Non-retryable client errors (rate-limit 429 IS retryable).
                    let is_retryable = err_str.contains("429")
                        || err_str.contains("too many requests")
                        || err_str.contains("timed out")
                        || err_str.contains("connection reset")
                        || err_str.contains("broken pipe")
                        || err_str.contains("503")
                        || err_str.contains("502")
                        || err_str.contains("500");

                    if attempt < max_retries && is_retryable {
                        tracing::debug!(
                            error = %e,
                            attempt,
                            delay_ms = delay.as_millis(),
                            "transient send error, retrying"
                        );
                        tokio::time::sleep(delay).await;
                        delay = std::cmp::min(delay * 2, Duration::from_secs(30));
                    } else {
                        last_err = Some(e);
                        break;
                    }
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("send failed after retries")))
    }

    /// Split `text` at natural boundaries and send each chunk with retry.
    async fn send_chunks(&self, text: &str, safe_limit: usize) {
        use crate::delivery::split_message;
        let chunks = split_message(text, safe_limit);
        for (i, chunk) in chunks.iter().enumerate() {
            if let Err(e) = self.send_with_retry(chunk, 3).await {
                tracing::debug!(error = %e, chunk = i, "stream consumer chunk send failed after retries");
            }
            if i + 1 < chunks.len() {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{IncomingMessage, MessageMetadata, OutgoingMessage, PlatformAdapter};
    use async_trait::async_trait;
    use edgecrab_types::Platform;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[test]
    fn default_config_values() {
        let cfg = StreamConsumerConfig::default();
        assert_eq!(cfg.buffer_threshold, 40);
        assert_eq!(cfg.cursor, " ▉");
        assert_eq!(cfg.edit_interval, Duration::from_millis(300));
        assert!(cfg.prefer_editing);
    }

    #[test]
    fn stream_item_done_sentinel() {
        let item = StreamItem::Done;
        match item {
            StreamItem::Done => {}
            StreamItem::Delta(_) => panic!("unexpected variant"),
        }
    }

    struct RecordingAdapter {
        sends: Mutex<Vec<String>>,
        edits: Mutex<Vec<String>>,
        send_ids: Mutex<Vec<String>>,
    }

    impl RecordingAdapter {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                sends: Mutex::new(Vec::new()),
                edits: Mutex::new(Vec::new()),
                send_ids: Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait]
    impl PlatformAdapter for RecordingAdapter {
        fn platform(&self) -> Platform {
            Platform::Webhook
        }

        async fn start(
            &self,
            _tx: tokio::sync::mpsc::Sender<IncomingMessage>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()> {
            self.sends.lock().await.push(msg.text);
            Ok(())
        }

        fn format_response(&self, text: &str, _metadata: &MessageMetadata) -> String {
            text.to_string()
        }

        fn max_message_length(&self) -> usize {
            4096
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

        fn supports_editing(&self) -> bool {
            true
        }

        async fn edit_message(
            &self,
            _message_id: &str,
            _metadata: &MessageMetadata,
            new_text: &str,
        ) -> anyhow::Result<String> {
            self.edits.lock().await.push(new_text.to_string());
            Ok("edited".into())
        }

        async fn send_and_get_id(&self, msg: OutgoingMessage) -> anyhow::Result<Option<String>> {
            self.send_ids.lock().await.push(msg.text);
            Ok(Some("msg-1".into()))
        }
    }

    #[tokio::test]
    async fn forced_batch_mode_skips_editing_on_edit_capable_adapter() {
        let adapter = RecordingAdapter::new();
        let mut cfg = StreamConsumerConfig::default();
        cfg.prefer_editing = false;

        let consumer = GatewayStreamConsumer::new(adapter.clone(), MessageMetadata::default(), cfg);
        let delta_tx = consumer.delta_sender();
        let task = tokio::spawn(consumer.run());

        delta_tx
            .send(StreamItem::Delta("hello world".into()))
            .await
            .expect("delta");
        delta_tx.send(StreamItem::Done).await.expect("done");

        task.await.expect("consumer task");

        assert_eq!(adapter.sends.lock().await.as_slice(), ["hello world"]);
        assert!(adapter.send_ids.lock().await.is_empty());
        assert!(adapter.edits.lock().await.is_empty());
    }
}
