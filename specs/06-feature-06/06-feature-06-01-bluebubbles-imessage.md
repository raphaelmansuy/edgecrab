# ADR-0601: iMessage Gateway via BlueBubbles

| Field       | Value                                              |
|-------------|----------------------------------------------------|
| Status      | Proposed                                           |
| Date        | 2026-04-14                                         |
| Depends on  | ADR-0610 (Security Hardening), edgecrab-gateway    |
| Implements  | hermes-agent PR #6437, #6460, #6494, #7107         |
| Crate       | `edgecrab-gateway`                                 |
| File        | `crates/edgecrab-gateway/src/bluebubbles.rs`       |

---

## 1. Context

BlueBubbles is a self-hosted macOS server that exposes Apple iMessage over a
REST + webhook API. hermes-agent ships a ~920-line Python adapter
(`gateway/platforms/bluebubbles.py`) providing full iMessage integration.
EdgeCrab's gateway already has the `PlatformAdapter` trait, `DeliveryRouter`,
and 13 adapters. This ADR specifies the Rust port.

---

## 2. First Principles

| Principle       | Application                                                |
|-----------------|------------------------------------------------------------|
| **SRP**         | Adapter owns only BlueBubbles protocol; no business logic  |
| **OCP**         | New adapter, zero changes to `PlatformAdapter` trait       |
| **DIP**         | Adapter depends on `PlatformAdapter` abstraction           |
| **DRY**         | Reuse `split_message()`, `DeliveryRouter`, SSRF guards     |
| **Code is Law** | All behavior derived from hermes-agent source truth        |

---

## 3. Architecture

```
+----------------------------------------------------------------------+
|                        edgecrab-gateway                              |
|                                                                      |
|  +---------------------+     +----------------------------------+    |
|  |   DeliveryRouter    |     |     BlueBubblesAdapter           |    |
|  |   (existing)        |     |                                  |    |
|  |   .register(adapter)+---->| platform() -> Platform::BlueBub  |    |
|  |   .deliver(resp)    |     | start(tx) -> spawn webhook srv   |    |
|  +---------------------+     | send(msg) -> POST /sendmessage   |    |
|                               | send_photo/voice/document       |    |
|                               +--------+------------------------+    |
|                                        |                             |
|                                        v                             |
|       +---------------------------+    +---------------------------+  |
|       |   Axum Webhook Server     |    |   reqwest HTTP Client     |  |
|       |   POST /bb-webhook        |    |   -> BlueBubbles Server   |  |
|       |   GET  /health            |    |   REST API v1             |  |
|       +---------------------------+    +---------------------------+  |
+----------------------------------------------------------------------+
                    |                              |
                    v                              v
          Incoming iMessages              Outgoing iMessages
          (webhook push)                  (REST POST)
```

---

## 4. Data Model

### 4.1 Platform Enum Extension

```rust
// crates/edgecrab-types/src/config.rs
pub enum Platform {
    // ... existing 18 variants ...
    BlueBubbles,   // NEW
}
```

### 4.2 Adapter Struct

```rust
pub struct BlueBubblesAdapter {
    server_url: String,                       // BlueBubbles server URL
    password: String,                         // API password (URL-encoded in queries)
    webhook_host: String,                     // bind host (default 127.0.0.1)
    webhook_port: u16,                        // bind port (default 8645)
    webhook_path: String,                     // path   (default /bb-webhook)
    send_read_receipts: bool,                 // default true
    private_api_enabled: bool,                // detected at connect()
    client: reqwest::Client,                  // reused across calls
    guid_cache: DashMap<String, String>,      // target -> chat GUID
    webhook_shutdown: Option<CancellationToken>,
}
```

### 4.3 Env Vars

| Variable                        | Required | Default             | Source (hermes-agent)        |
|---------------------------------|----------|---------------------|------------------------------|
| `BLUEBUBBLES_SERVER_URL`        | Yes      | —                   | `bluebubbles.py:__init__`    |
| `BLUEBUBBLES_PASSWORD`          | Yes      | —                   | `bluebubbles.py:__init__`    |
| `BLUEBUBBLES_WEBHOOK_HOST`      | No       | `127.0.0.1`         | `bluebubbles.py:__init__`    |
| `BLUEBUBBLES_WEBHOOK_PORT`      | No       | `8645`              | `bluebubbles.py:__init__`    |
| `BLUEBUBBLES_WEBHOOK_PATH`      | No       | `/bb-webhook`       | `bluebubbles.py:__init__`    |
| `BLUEBUBBLES_SEND_READ_RECEIPTS`| No       | `true`              | `bluebubbles.py:__init__`    |
| `BLUEBUBBLES_ALLOWED_USERS`     | No       | —                   | `gateway/run.py:L6197`       |
| `BLUEBUBBLES_HOME_CHANNEL`      | No       | —                   | `gateway/config.py`          |

---

## 5. Protocol Flow

### 5.1 Startup (connect)

```
BlueBubblesAdapter::start(tx)
  |
  +-- 1. Validate server_url + password are set
  +-- 2. Create reqwest::Client (timeout=30s)
  +-- 3. GET /api/v1/ping                     -> verify connectivity
  +-- 4. GET /api/v1/server/info              -> detect private_api, helper_connected
  +-- 5. Spawn axum server on webhook_host:webhook_port
  |       GET  /health    -> "ok"
  |       POST /bb-webhook -> handle_webhook()
  +-- 6. Register webhook via POST /api/v1/webhook
  |       - First check existing webhooks (crash recovery)
  |       - Events: ["new-message", "updated-message", "message"]
  +-- 7. Send IncomingMessage via tx channel
```

### 5.2 Inbound Message (webhook)

```
POST /bb-webhook
  |
  +-- 1. Authenticate: check password in query/headers
  |       (X-Password, X-BlueBubbles-GUID, query ?password=)
  |       -> 401 on mismatch
  +-- 2. Parse JSON payload, extract message record
  +-- 3. Filter: skip isFromMe, skip tapback reactions (2000-3005)
  +-- 4. Download attachments via GET /api/v1/attachment/{guid}/download
  |       - HEIC/HEIF -> jpg conversion
  |       - CAF -> mp3 conversion
  +-- 5. Resolve sender: handle.address -> from -> chatIdentifier
  +-- 6. Build IncomingMessage, send via tx channel
```

### 5.3 Outbound Message

```
send(chat_id, text)
  |
  +-- 1. Strip markdown (no native markdown in iMessage)
  +-- 2. Split via truncate_message(4000)
  +-- 3. For each chunk:
  |       +-- Resolve chat GUID (cache hit or API query)
  |       +-- If no GUID + private_api: create chat via _create_chat_for_handle
  |       +-- POST /api/v1/message/text { chatGuid, message, ... }
  +-- 4. Reply threading: selectedMessageGuid (requires Private API)
```

---

## 6. Edge Cases & Roadblocks

| # | Edge Case                        | Remediation                                            | Source                |
|---|----------------------------------|--------------------------------------------------------|-----------------------|
| 1 | Server crash → dangling webhooks | `start()` queries existing webhooks before registering | `_register_webhook()` |
| 2 | Duplicate webhook registrations  | `disconnect()` deletes ALL matching webhooks           | `_unregister_webhook`|
| 3 | HEIC image attachments           | Convert to JPEG before caching                         | `_download_attachment`|
| 4 | CAF audio from Apple devices     | Convert to MP3 before caching                          | `_download_attachment`|
| 5 | Private API unavailable          | Graceful skip: typing, read receipts, reactions no-op  | `send_typing()`      |
| 6 | Chat GUID not resolvable         | Create new chat if private_api + valid handle          | `send()`             |
| 7 | Large media uploads              | 120s timeout on multipart POST                         | `_send_attachment()`  |
| 8 | Tapback reaction messages        | Filter by associatedMessageType range 2000-3005        | `_handle_webhook()`  |
| 9 | Password leakage in logs         | URL-encode password in query params only               | `_api_url()`          |
| 10| Phone/email in error logs        | Redact via regex before logging                        | `_PHONE_RE/_EMAIL_RE` |
| 11| Webhook payload format variance  | Try JSON first, fallback to form-encoded parse_qs      | `_handle_webhook()`  |
| 12| iMessage has no message editing  | Platform in `no-message-editing` list                  | `run.py:L6197`       |

---

## 7. Implementation Plan

### 7.1 Files to Create

| File                                          | Purpose                          |
|-----------------------------------------------|----------------------------------|
| `crates/edgecrab-gateway/src/bluebubbles.rs`  | Adapter implementation           |

### 7.2 Files to Modify

| File                                           | Change                              |
|------------------------------------------------|--------------------------------------|
| `crates/edgecrab-types/src/config.rs`          | Add `Platform::BlueBubbles`          |
| `crates/edgecrab-gateway/src/lib.rs`           | Add `pub mod bluebubbles;`           |
| `crates/edgecrab-gateway/src/run.rs`           | Register adapter in boot sequence    |
| `crates/edgecrab-cli/src/gateway_catalog.rs`   | Add setup wizard entries             |

### 7.3 Dependencies

```toml
# No new crate deps — reuse existing:
# reqwest (already in edgecrab-gateway)
# axum   (already in edgecrab-gateway)
# dashmap (already in workspace)
```

### 7.4 Test Matrix

| Test                              | Type        | Validates                          |
|-----------------------------------|-------------|-------------------------------------|
| `test_from_env_missing`           | Unit        | Returns None when creds missing     |
| `test_from_env_complete`          | Unit        | Constructs adapter with defaults    |
| `test_webhook_auth_reject`        | Unit        | 401 on wrong password               |
| `test_webhook_auth_accept`        | Unit        | 200 on correct password             |
| `test_skip_from_me`               | Unit        | Filters own messages                |
| `test_skip_tapback`               | Unit        | Filters reaction messages           |
| `test_guid_cache`                 | Unit        | Cache hit avoids API call           |
| `test_markdown_strip`             | Unit        | `**bold**` → `bold`                 |
| `test_message_split`              | Unit        | Long text split at 4000 chars       |
| `test_crash_recovery_webhook`     | Integration | Reuses existing webhook on restart  |
| `test_disconnect_cleanup`         | Integration | Unregisters webhook on shutdown     |

---

## 8. Security Considerations

- **Webhook auth**: Password validated on every POST via constant-time comparison
- **SSRF**: Attachment download URLs are internal BlueBubbles server; no user-controlled URLs
- **PII redaction**: Phone numbers and emails stripped from log output
- **Credential storage**: Password never logged; only used in URL query params over local network
- **No message editing**: Platform correctly flagged, prevents edit-based confusion attacks

---

## 9. Acceptance Criteria

- [ ] `BlueBubblesAdapter` implements `PlatformAdapter` trait
- [ ] Auto-webhook registration with crash-recovery dedup
- [ ] Inbound: text, image, voice, video, document attachments
- [ ] Outbound: text (markdown stripped), photo, voice, document
- [ ] Private API: typing indicators, read receipts (graceful skip if unavailable)
- [ ] Tapback reactions filtered from inbound
- [ ] HEIC→JPEG, CAF→MP3 media conversion
- [ ] All tests pass: `cargo test -p edgecrab-gateway -- bluebubbles`
- [ ] Setup wizard entry in gateway catalog
