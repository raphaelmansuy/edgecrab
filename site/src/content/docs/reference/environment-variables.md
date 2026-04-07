---
title: Environment Variables
description: Complete reference for all EDGECRAB_* environment variables and platform-specific env vars. Grounded in crates/edgecrab-core/src/config.rs apply_env_overrides().
sidebar:
  order: 4
---

All `EDGECRAB_*` variables are applied via `apply_env_overrides()` in `config.rs`. They override the corresponding `config.yaml` values at runtime.

---

## Core Agent Variables

| Variable | Type | Default | Description |
|----------|------|---------|-------------|
| `EDGECRAB_HOME` | path | `~/.edgecrab` | Override the EdgeCrab home directory |
| `EDGECRAB_MODEL` | string | `ollama/gemma4:latest` | Default LLM model |
| `EDGECRAB_MAX_ITERATIONS` | integer | `90` | Max tool call iterations per session |
| `EDGECRAB_TIMEZONE` | string | (system) | IANA timezone (e.g. `America/New_York`) |
| `EDGECRAB_REASONING_EFFORT` | string | `""` | Reasoning budget: `low`, `medium`, `high`, `xhigh` |
| `EDGECRAB_SAVE_TRAJECTORIES` | bool | `false` | Log full trajectory to file |
| `EDGECRAB_SKIP_CONTEXT_FILES` | bool | `false` | Skip SOUL.md and AGENTS.md loading |
| `EDGECRAB_SKIP_MEMORY` | bool | `false` | Disable memory for this session |
| `EDGECRAB_MANAGED` | bool | `false` | Block all config writes (`1` to enable) |

---

## Gateway Variables

| Variable | Type | Description |
|----------|------|-------------|
| `EDGECRAB_GATEWAY_HOST` | string | Gateway bind host (default: `127.0.0.1`) |
| `EDGECRAB_GATEWAY_PORT` | integer | Gateway bind port (default: `8080`) |
| `EDGECRAB_GATEWAY_WEBHOOK` | bool | Enable/disable webhook endpoint |

---

## Telegram Variables

| Variable | Type | Description |
|----------|------|-------------|
| `TELEGRAM_BOT_TOKEN` | string | **Required.** Auto-enables Telegram platform |
| `TELEGRAM_ALLOWED_USERS` | csv | Comma-separated allowed Telegram usernames |
| `TELEGRAM_HOME_CHANNEL` | string | Chat ID for proactive messages |

---

## Discord Variables

| Variable | Type | Description |
|----------|------|-------------|
| `DISCORD_BOT_TOKEN` | string | **Required.** Auto-enables Discord platform |
| `DISCORD_ALLOWED_USERS` | csv | Comma-separated Discord user IDs |
| `DISCORD_HOME_CHANNEL` | string | Channel ID for proactive messages |

---

## Slack Variables

| Variable | Type | Description |
|----------|------|-------------|
| `SLACK_BOT_TOKEN` | string | **Required** (with `SLACK_APP_TOKEN`). Auto-enables Slack |
| `SLACK_APP_TOKEN` | string | **Required** (with `SLACK_BOT_TOKEN`). Socket Mode app token |
| `SLACK_ALLOWED_USERS` | csv | Comma-separated Slack user IDs |

---

## Signal Variables

| Variable | Type | Description |
|----------|------|-------------|
| `SIGNAL_HTTP_URL` | string | **Required** (with `SIGNAL_ACCOUNT`). signal-cli HTTP daemon URL |
| `SIGNAL_ACCOUNT` | string | **Required** (with `SIGNAL_HTTP_URL`). Registered phone number |

---

## WhatsApp Variables

| Variable | Type | Description |
|----------|------|-------------|
| `WHATSAPP_ENABLED` | bool | Enable WhatsApp (`1`, `true`, or `yes`) |
| `WHATSAPP_MODE` | string | Bridge mode: `self-chat` or `any-sender` |
| `WHATSAPP_ALLOWED_USERS` | csv | Comma-separated phone numbers |
| `WHATSAPP_BRIDGE_PORT` | integer | Bridge port (default: `3000`) |
| `WHATSAPP_BRIDGE_URL` | string | Override bridge URL |
| `WHATSAPP_SESSION_PATH` | path | Bridge session storage path |
| `WHATSAPP_REPLY_PREFIX` | string | Text prepended to all replies |

---

## Matrix Variables

| Variable | Type | Description |
|----------|------|-------------|
| `MATRIX_HOMESERVER` | string | **Required.** Homeserver URL (e.g. `https://matrix.org`) |
| `MATRIX_ACCESS_TOKEN` | string | **Required.** Long-lived access token |
| `MATRIX_USER_ID` | string | Full user ID (e.g. `@edgecrab:matrix.org`) |
| `MATRIX_ALLOWED_USERS` | csv | Comma-separated Matrix user IDs |

---

## Mattermost Variables

| Variable | Type | Description |
|----------|------|-------------|
| `MATTERMOST_URL` | string | **Required.** Server URL (e.g. `https://chat.example.com`) |
| `MATTERMOST_TOKEN` | string | **Required.** Bot or personal access token |
| `MATTERMOST_ALLOWED_USERS` | csv | Comma-separated Mattermost user IDs |

---

## DingTalk Variables

| Variable | Type | Description |
|----------|------|-------------|
| `DINGTALK_APP_KEY` | string | **Required.** DingTalk app AppKey |
| `DINGTALK_APP_SECRET` | string | **Required.** DingTalk app AppSecret |
| `DINGTALK_ROBOT_CODE` | string | Robot code if using multiple robots |
| `DINGTALK_WEBHOOK_PORT` | integer | Inbound webhook port |

---

## SMS (Twilio) Variables

| Variable | Type | Description |
|----------|------|-------------|
| `TWILIO_ACCOUNT_SID` | string | **Required.** Twilio Account SID |
| `TWILIO_AUTH_TOKEN` | string | **Required.** Twilio Auth Token |
| `TWILIO_PHONE_NUMBER` | string | **Required.** Your number in E.164 format (e.g. `+15551234567`) |
| `SMS_WEBHOOK_PORT` | integer | Local webhook port (default: `8082`) |
| `SMS_ALLOWED_USERS` | csv | Comma-separated allowed phone numbers (E.164) |

---

## Email Variables

| Variable | Type | Description |
|----------|------|-------------|
| `EMAIL_PROVIDER` | string | **Required.** One of: `sendgrid`, `mailgun`, `generic_smtp` |
| `EMAIL_API_KEY` | string | Required for SendGrid/Mailgun; optional for SMTP |
| `EMAIL_FROM` | string | **Required.** Sender address (e.g. `bot@example.com`) |
| `EMAIL_DOMAIN` | string | Required for Mailgun (e.g. `mg.example.com`) |
| `EMAIL_SMTP_HOST` | string | Required for `generic_smtp` |
| `EMAIL_SMTP_PORT` | integer | SMTP port (default: `587`) |
| `EMAIL_SMTP_USERNAME` | string | SMTP username (defaults to `EMAIL_FROM`) |
| `EMAIL_SMTP_PASSWORD` | string | SMTP password (fallback to `EMAIL_API_KEY`) |
| `EMAIL_WEBHOOK_PORT` | integer | Inbound webhook port (default: `8093`) |
| `EMAIL_ALLOWED` | csv | Comma-separated allowed sender addresses |

---

## Feishu / Lark Variables

| Variable | Type | Description |
|----------|------|-------------|
| `FEISHU_APP_ID` | string | **Required.** Feishu/Lark app ID |
| `FEISHU_APP_SECRET` | string | **Required.** Feishu/Lark app secret |
| `FEISHU_WEBHOOK_PORT` | integer | Inbound webhook port |
| `FEISHU_WEBHOOK_HOST` | string | Webhook bind host (default: `0.0.0.0`) |
| `FEISHU_WEBHOOK_PATH` | string | Webhook path (default: `/feishu/webhook`) |
| `FEISHU_BASE_URL` | string | Override Feishu API base URL |
| `FEISHU_VERIFICATION_TOKEN` | string | Webhook verification token |
| `FEISHU_ENCRYPT_KEY` | string | Webhook payload encryption key |
| `FEISHU_BOT_OPEN_ID` | string | Bot's open ID (used to filter out own messages) |
| `FEISHU_BOT_USER_ID` | string | Bot's user ID (alternative to `FEISHU_BOT_OPEN_ID`) |
| `FEISHU_BOT_NAME` | string | Bot's display name (alternative identity filter) |
| `FEISHU_GROUP_POLICY` | string | Group chat message policy |

---

## WeCom Variables

| Variable | Type | Description |
|----------|------|-------------|
| `WECOM_BOT_ID` | string | **Required.** WeCom bot corp ID |
| `WECOM_SECRET` | string | **Required.** WeCom bot secret |
| `WECOM_WEBSOCKET_URL` | string | Override WeCom WebSocket URL |

---

## TTS / STT Variables

| Variable | Type | Description |
|----------|------|-------------|
| `EDGECRAB_TTS_PROVIDER` | string | TTS provider: `edge-tts`, `openai`, `elevenlabs` |
| `EDGECRAB_TTS_VOICE` | string | TTS voice name |
| `ELEVENLABS_API_KEY` | string | ElevenLabs API key |

---

## Provider API Keys

These are not `EDGECRAB_*` variables — they are standard API key env vars detected by `setup.rs`:

| Variable | Provider |
|----------|----------|
| `OPENROUTER_API_KEY` | OpenRouter (default endpoint) |
| `ANTHROPIC_API_KEY` | Anthropic |
| `OPENAI_API_KEY` | OpenAI |
| `GOOGLE_API_KEY` | Google Gemini |
| `GOOGLE_APPLICATION_CREDENTIALS` | Vertex AI (path to service account JSON) |
| `XAI_API_KEY` | xAI Grok |
| `DEEPSEEK_API_KEY` | DeepSeek |
| `MISTRAL_API_KEY` | Mistral AI |
| `GROQ_API_KEY` | Groq (LPU inference) |
| `HUGGING_FACE_HUB_TOKEN` | HuggingFace |
| `ZAI_API_KEY` | Z.AI |
| `GITHUB_TOKEN` | GitHub Copilot |

Ollama and LM Studio require no API key (local inference only).

---

## Other Service Variables

| Variable | Service | Description |
|----------|---------|-------------|
| `HONCHO_API_KEY` | Honcho | Enables cloud sync (auto-sets `honcho.cloud_sync: true`) |
| `HA_URL` | Home Assistant | Base URL (enables Home Assistant tools) |
| `HA_TOKEN` | Home Assistant | Long-lived access token |
| `CDP_URL` | Browser | Chrome DevTools Protocol endpoint (instead of local Chrome) |

---

## Boolean Parsing

Variables marked as `bool` accept: `1`, `true`, `yes`, `on` (case-insensitive) to enable. Any other value (including empty) is treated as disabled.

## CSV Parsing

Variables marked as `csv` accept comma-separated values with optional whitespace: `"user1, user2, user3"`.

---

## Common Configurations (FAQ)

**Set a different model for one session without editing config.yaml:**
```bash
EDGECRAB_MODEL=anthropic/claude-opus-4-5 edgecrab run "big refactor task"
```

**Debug all environment overrides at startup:**
```bash
RUST_LOG=edgecrab_core=debug edgecrab run "test" 2>&1 | grep "env override"
```

**Disable all file-writing (read-only managed mode):**
```bash
EDGECRAB_MANAGED=1 edgecrab run "read and summarize src/"
```

**Run the gateway on a public interface:**
```bash
EDGECRAB_GATEWAY_HOST=0.0.0.0 EDGECRAB_GATEWAY_PORT=8443 edgecrab gateway start
```

**Combine multiple platforms at once:**
```bash
export TELEGRAM_BOT_TOKEN=...
export DISCORD_BOT_TOKEN=...
edgecrab gateway start  # both platforms start automatically
```

**What order do settings merge in?**
Config resolution: `defaults → ~/.edgecrab/config.yaml → EDGECRAB_* env vars → CLI flags`.
Environment variables always win over the config file, but CLI flags win over everything.

---

## See Also

- [Configuration Reference](/reference/configuration/) — full YAML schema with all defaults
- [CLI Commands](/reference/cli-commands/) — CLI flags that override env vars at a higher priority
- [Self-Hosting Guide](/guides/self-hosting/) — production deployment patterns using env vars
