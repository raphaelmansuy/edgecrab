# ADR-0602: WeChat (Weixin) & WeCom Callback Mode

| Field       | Value                                              |
|-------------|----------------------------------------------------|
| Status      | Proposed                                           |
| Date        | 2026-04-14                                         |
| Depends on  | ADR-0610 (Security Hardening), edgecrab-gateway    |
| Implements  | hermes-agent PR #7166, #7943, #7928, #8665         |
| Crate       | `edgecrab-gateway`                                 |
| Files       | `crates/edgecrab-gateway/src/weixin.rs` (NEW)      |
|             | `crates/edgecrab-gateway/src/wecom.rs` (EXISTS)    |

---

## 1. Context

Two separate adapters serve the Chinese messaging ecosystem:

| Platform     | Transport          | API                              | Status in EdgeCrab          |
|--------------|--------------------|----------------------------------|-----------------------------|
| **Weixin**   | HTTP long-poll     | iLink Bot API (Tencent)          | **NOT YET IMPLEMENTED**     |
| **WeCom**    | WebSocket          | AI Bot WS Gateway               | **EXISTS** (`wecom.rs`)     |

hermes-agent ships a 1669-line Weixin adapter and a 1435-line WeCom adapter.
EdgeCrab's `wecom.rs` already implements the WS protocol. This ADR specifies
the new Weixin adapter and enhancements to the existing WeCom adapter.

---

## 2. First Principles

| Principle       | Application                                                     |
|-----------------|-----------------------------------------------------------------|
| **SRP**         | Weixin adapter owns iLink protocol; WeCom owns WS protocol     |
| **OCP**         | New `weixin.rs`, minimal changes to existing `wecom.rs`         |
| **DRY**         | Shared crypto utilities extracted to a common module            |
| **ISP**         | Each adapter implements only its transport-specific methods     |
| **Code is Law** | All behavior derived from hermes-agent `weixin.py` / `wecom.py`|

---

## 3. Architecture

### 3.1 Weixin (Personal WeChat via iLink Bot API)

```
+-------------------------------------------------------------------+
|                     WeixinAdapter                                  |
|                                                                    |
|  +-------------------+   +-------------------+   +--------------+ |
|  | Long-Poll Loop    |   | iLink REST Client |   | AES-128-ECB  | |
|  | GET /getupdates   |   | POST /sendmessage |   | CDN Crypto   | |
|  | (35s timeout)     |   | POST /getuploadurl|   | encrypt/     | |
|  |                   |   | POST /sendtyping  |   | decrypt      | |
|  +--------+----------+   +--------+----------+   +------+-------+ |
|           |                        |                     |         |
|           v                        v                     v         |
|  +-------------------+   +-------------------+   +--------------+ |
|  | ContextTokenStore |   | TypingTicketCache |   | QR Login     | |
|  | (disk-backed JSON)|   | (in-memory TTL)   |   | Flow (setup) | |
|  +-------------------+   +-------------------+   +--------------+ |
+-------------------------------------------------------------------+
           |                        |
           v                        v
   Inbound WeChat msgs      Outbound responses
   (personal accounts)      (text + encrypted media)
```

### 3.2 WeCom (Enterprise WeChat — existing + enhancements)

```
+-------------------------------------------------------------------+
|                     WeComAdapter (existing wecom.rs)               |
|                                                                    |
|  +-----------------------+   +----------------------------+        |
|  | WebSocket Connection  |   | Chunked Media Upload       |       |
|  | wss://openws.work...  |   | init -> chunks -> finish   |       |
|  |                       |   | (512KB base64 chunks)      |       |
|  | Commands:             |   +----------------------------+        |
|  |  aibot_subscribe      |                                         |
|  |  aibot_msg_callback   |   +----------------------------+        |
|  |  aibot_respond_msg    |   | Text Batching              |       |
|  |  aibot_send_msg       |   | aggregate split msgs       |       |
|  |  ping (30s heartbeat) |   | quiet window: 0.6s / 2.0s  |       |
|  +-----------------------+   +----------------------------+        |
+-------------------------------------------------------------------+
```

---

## 4. Data Model — Weixin Adapter

### 4.1 Platform Enum Extension

```rust
// crates/edgecrab-types/src/config.rs
pub enum Platform {
    // ... existing variants ...
    Weixin,   // NEW — personal WeChat via iLink Bot API
    // Wecom already exists
}
```

### 4.2 Adapter Struct

```rust
pub struct WeixinAdapter {
    token: String,                            // iLink Bot bearer token
    account_id: String,                       // Bot account identifier
    base_url: String,                         // default: https://ilinkai.weixin.qq.com
    cdn_base_url: String,                     // default: https://novac2c.cdn.weixin.qq.com/c2c
    client: reqwest::Client,
    context_tokens: Arc<Mutex<ContextTokenStore>>,  // per-peer disk-backed
    typing_cache: Arc<Mutex<TypingTicketCache>>,     // in-memory TTL 600s
    seen_messages: Arc<Mutex<DeduplicationMap>>,      // 300s TTL, max 1000
    dm_policy: AccessPolicy,
    group_policy: AccessPolicy,
    allowed_users: HashSet<String>,
    shutdown: CancellationToken,
}

/// Disk-backed per-peer context token store
struct ContextTokenStore {
    path: PathBuf,       // ~/.edgecrab/weixin/accounts/<id>.context-tokens.json
    tokens: HashMap<String, String>,
}

/// In-memory typing ticket cache with TTL
struct TypingTicketCache {
    cache: HashMap<String, (String, Instant)>,  // peer -> (ticket, fetched_at)
    ttl: Duration,                               // 600s
}
```

### 4.3 Env Vars — Weixin

| Variable                  | Required | Default                                  | Source                    |
|---------------------------|----------|------------------------------------------|---------------------------|
| `WEIXIN_TOKEN`            | Yes      | —                                        | `weixin.py:__init__`      |
| `WEIXIN_ACCOUNT_ID`       | Yes      | —                                        | `weixin.py:__init__`      |
| `WEIXIN_BASE_URL`         | No       | `https://ilinkai.weixin.qq.com`          | `weixin.py:__init__`      |
| `WEIXIN_CDN_BASE_URL`     | No       | `https://novac2c.cdn.weixin.qq.com/c2c`  | `weixin.py:__init__`      |
| `WEIXIN_DM_POLICY`        | No       | `open`                                   | `weixin.py:__init__`      |
| `WEIXIN_GROUP_POLICY`     | No       | `disabled`                               | `weixin.py:__init__`      |
| `WEIXIN_ALLOWED_USERS`    | No       | —                                        | `weixin.py:__init__`      |
| `WEIXIN_HOME_CHANNEL`     | No       | —                                        | `gateway/config.py`       |

---

## 5. Protocol Flows — Weixin

### 5.1 iLink Authentication Headers

Every request to iLink Bot API requires:

```
AuthorizationType: ilink_bot_token
Authorization: Bearer <token>
X-WECHAT-UIN: <random base64-encoded 4-byte uint>
iLink-App-Id: bot
iLink-App-ClientVersion: 131584
```

### 5.2 Long-Poll Inbound Loop

```
_poll_loop()
  |
  +-- loop {
  |     GET /ilink/bot/getupdates (timeout=35s)
  |     for msg in response.messages:
  |       +-- dedup check (msg_id, 300s window, max 1000)
  |       +-- parse text from item_list (type=1)
  |       +-- parse referenced messages (引用)
  |       +-- download+decrypt media (images, voice, video, files)
  |       +-- resolve chat type (DM vs group)
  |       +-- apply dm_policy / group_policy
  |       +-- build IncomingMessage, send via tx
  |   }
```

### 5.3 AES-128-ECB Media Crypto

```
Outbound upload:
  plaintext -> PKCS7 pad -> AES-128-ECB encrypt (random 16-byte key)
     -> POST getuploadurl (filekey, rawsize, md5, filesize, aeskey_hex)
     -> POST ciphertext to CDN URL
     -> extract x-encrypted-param header
     -> sendmessage with encrypt_query_param + aes_key (base64)

Inbound download:
  GET media URL -> ciphertext
     -> AES-128-ECB decrypt (key from metadata)
     -> PKCS7 unpad -> plaintext file
```

### 5.4 Markdown Reformatting

iMessage-like personal WeChat does not render markdown natively:

```
# Title        ->  【Title】
## Subtitle    ->  **Subtitle**
| col | col |  ->  - Label: Value (key-value per row)
```

### 5.5 Context Token Echo Protocol

Every outbound reply must echo the latest `context_token` received from that
peer. Tokens are persisted to disk so they survive restarts.

```
Inbound msg from user_123: { context_token: "abc123" }
  -> ContextTokenStore.set("user_123", "abc123")
  -> persist to ~/.edgecrab/weixin/accounts/<id>.context-tokens.json

Outbound reply to user_123:
  -> ContextTokenStore.get("user_123") -> "abc123"
  -> include context_token: "abc123" in sendmessage payload
```

---

## 6. WeCom Enhancements (existing wecom.rs)

### 6.1 Current Coverage

EdgeCrab's `wecom.rs` already implements:
- WebSocket subscribe handshake
- Inbound text callback parsing with dedup
- Correlated reply delivery (aibot_respond_msg)
- Proactive outbound send (aibot_send_msg)
- Ping heartbeat (30s)
- Reconnection with exponential backoff

### 6.2 Missing Features (from hermes-agent #7943, #7928)

| Feature                 | Status      | hermes-agent Source             |
|-------------------------|-------------|----------------------------------|
| Chunked media upload    | **MISSING** | `wecom.py:_upload_media()`       |
| Media download + AES    | **MISSING** | `wecom.py:_cache_media()`        |
| Stream reply mode       | **MISSING** | `wecom.py:_send_reply_stream()`  |
| Text batching           | **MISSING** | `wecom.py:_enqueue_text_event()` |
| DM/group policy config  | **MISSING** | `wecom.py:_entry_matches()`      |
| Image size downgrade    | **MISSING** | `wecom.py:send_image()`          |

### 6.3 Chunked Media Upload Protocol (WeCom WS)

```
Phase 1 - Init:
  aibot_upload_media_init {
    body: { type, filename, total_size, total_chunks, md5 }
  } -> { upload_id }

Phase 2 - Chunks (512KB each, base64):
  aibot_upload_media_chunk {
    body: { upload_id, chunk_index, data: <base64> }
  } -> ack per chunk

Phase 3 - Finish:
  aibot_upload_media_finish {
    body: { upload_id }
  } -> { media_id }

Phase 4 - Send:
  aibot_send_msg {
    body: { msgtype: "image"|"file"|"voice"|"video",
            <type>: { media_id } }
  }
```

### 6.4 Media Size Limits with Auto-Downgrade

```
+----------+--------+------------------------------------+
| Type     | Limit  | Downgrade                          |
+----------+--------+------------------------------------+
| Image    | 10 MB  | -> "file" type if exceeded         |
| Video    | 10 MB  | -> "file" type if exceeded         |
| Voice    |  2 MB  | AMR only; -> "file" if wrong fmt   |
| File     | 20 MB  | Reject with error                  |
+----------+--------+------------------------------------+
```

### 6.5 Text Batching (Client-Side Split Aggregation)

```
WeCom client splits long messages at ~4000 chars.
Adapter aggregates rapid successive messages:

  msg_1 (3900 chars) -> buffer, start quiet timer (2.0s)
  msg_2 (2100 chars) -> append to buffer, restart timer (0.6s)
  [0.6s quiet]       -> flush merged text as single IncomingMessage
```

### 6.6 Inbound Media AES Decryption (WeCom)

```
Ciphertext from WeCom -> AES-256-CBC decrypt
Key: from media metadata
IV:  key[0..16]
Padding: PKCS#7
```

---

## 7. Edge Cases & Roadblocks

| #  | Edge Case                              | Remediation                                    | Source                   |
|----|----------------------------------------|------------------------------------------------|--------------------------|
| 1  | iLink token expiry                     | Re-auth via QR login flow (setup wizard)       | `weixin.py:qr_login()`  |
| 2  | CDN upload returns non-200             | Retry once, then return ToolError              | `weixin.py:send_document`|
| 3  | AES key format variance (hex vs raw)   | `_parse_aes_key()` handles both 16B and 32B    | `weixin.py:_parse_aes_key`|
| 4  | Context token missing for new peer     | Send without context_token (iLink accepts it)  | `weixin.py:send()`       |
| 5  | Dedup map memory leak                  | Cap at 1000 entries, 300s TTL eviction         | `weixin.py:_poll_loop()` |
| 6  | Two profiles polling same account      | `acquire_scoped_lock()` on token               | `weixin.py:connect()`    |
| 7  | WeCom WS disconnect mid-upload         | Fail pending futures, retry on reconnect       | `wecom.py:disconnect()`  |
| 8  | WeCom voice > 2MB                      | Auto-downgrade to file attachment              | `wecom.py:send_voice()`  |
| 9  | WeCom text batching false merge        | Separate quiet windows: 0.6s normal, 2.0s long| `wecom.py:_flush_text`   |
| 10 | iLink base URL redirect on QR login    | Follow `redirect_host` from scan response      | `weixin.py:qr_login()`  |
| 11 | Group messages require @mention        | Parse `at_user_list` in msg metadata           | `weixin.py:_process_msg` |
| 12 | Referenced message contains media      | Prepend `[引用媒体: ...]` text prefix          | `weixin.py:_process_msg` |

---

## 8. Implementation Plan

### 8.1 Files to Create

| File                                      | Purpose                               |
|-------------------------------------------|---------------------------------------|
| `crates/edgecrab-gateway/src/weixin.rs`   | Weixin adapter (iLink Bot API)        |
| `crates/edgecrab-gateway/src/weixin_crypto.rs` | AES-128-ECB CDN crypto helpers   |

### 8.2 Files to Modify

| File                                        | Change                                       |
|---------------------------------------------|----------------------------------------------|
| `crates/edgecrab-types/src/config.rs`       | Add `Platform::Weixin`                       |
| `crates/edgecrab-gateway/src/lib.rs`        | Add `pub mod weixin; pub mod weixin_crypto;`  |
| `crates/edgecrab-gateway/src/run.rs`        | Register WeixinAdapter in boot sequence      |
| `crates/edgecrab-gateway/src/wecom.rs`      | Add chunked upload, text batching, media DL  |
| `crates/edgecrab-cli/src/gateway_catalog.rs`| Add Weixin setup wizard entries              |
| `Cargo.toml` (edgecrab-gateway)            | Add `aes` + `cbc` crate deps                |

### 8.3 Dependencies

```toml
[dependencies]
aes = "0.8"           # AES-128-ECB for Weixin CDN, AES-256-CBC for WeCom
cbc = "0.1"           # CBC mode for WeCom media decryption
# ecb mode via aes crate directly
```

### 8.4 Test Matrix

| Test                              | Adapter | Validates                              |
|-----------------------------------|---------|----------------------------------------|
| `test_weixin_auth_headers`        | Weixin  | Correct iLink headers generated        |
| `test_context_token_persist`      | Weixin  | Disk-backed store survives restart     |
| `test_aes128_ecb_roundtrip`       | Weixin  | Encrypt/decrypt identity               |
| `test_aes_key_parse_hex_vs_raw`   | Weixin  | Both 16B raw and 32B hex work          |
| `test_markdown_reformat`          | Weixin  | `# Title` -> `【Title】`               |
| `test_dedup_eviction`             | Weixin  | Max 1000, 300s TTL                     |
| `test_wecom_chunked_upload`       | WeCom   | 3-phase upload protocol                |
| `test_wecom_text_batching`        | WeCom   | Merge rapid messages, respect quiet    |
| `test_wecom_aes256_cbc_decrypt`   | WeCom   | Media decryption correctness           |
| `test_wecom_size_downgrade`       | WeCom   | Image >10MB -> file type               |

---

## 9. Security Considerations

- **Credential isolation**: Weixin token stored with chmod 0o600 per account
- **Token lock**: `acquire_scoped_lock()` prevents two profiles polling same account
- **SSRF on WeCom media URLs**: `is_safe_url()` check before download
- **AES key handling**: Keys kept in memory only, never logged
- **Dedup map bounded**: Prevents DoS via message replay
- **CDN upload path traversal**: File paths validated before upload

---

## 10. Acceptance Criteria

### Weixin
- [ ] `WeixinAdapter` implements `PlatformAdapter` trait
- [ ] Long-poll loop receives messages from iLink Bot API
- [ ] AES-128-ECB media crypto (upload + download)
- [ ] Context token echo protocol (disk-persisted)
- [ ] Markdown reformatting for WeChat display
- [ ] Dedup with bounded map (1000 entries, 300s TTL)
- [ ] QR login flow in setup wizard

### WeCom Enhancements
- [ ] Chunked media upload (init→chunks→finish) over WebSocket
- [ ] Media download with AES-256-CBC decryption
- [ ] Stream reply mode (aibot_respond_msg with stream type)
- [ ] Text batching with quiet window (0.6s / 2.0s)
- [ ] Media size limits with auto-downgrade to file type
- [ ] DM/group policy configuration
