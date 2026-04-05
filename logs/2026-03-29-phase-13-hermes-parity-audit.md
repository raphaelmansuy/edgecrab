# Phase 13: Hermes Parity Audit — Implementation Log

**Date:** 2026-03-29
**Baseline:** 526 tests passing, 36 tool registrations
**Final:** 556 tests passing, 39 tool registrations, 2 new gateway adapters

## New Tools Implemented

### 1. `vision_analyze` (crates/edgecrab-tools/src/tools/vision.rs)
- **Matches:** hermes `tools/vision_tools.py`
- **Features:** Multimodal image analysis via LLM, URL + local file input, SSRF protection via `is_safe_url`, base64 encoding, MIME auto-detection, 10MB limit, 120s timeout
- **Tests:** 7 unit tests

### 2. `manage_cron_jobs` (crates/edgecrab-tools/src/tools/cron.rs)
- **Matches:** hermes `tools/cronjob_tools.py` + `cron/`
- **Features:** LLM-callable cron CRUD (create/list/pause/resume/remove/status), shared store with CLI `cron` subcommand, prompt injection scanning (10 regex patterns + invisible unicode), 5-field cron auto-conversion to 7-field format
- **Tests:** 8 unit tests

### 3. `transcribe_audio` (crates/edgecrab-tools/src/tools/transcribe.rs)
- **Matches:** hermes `tools/transcription_tools.py`
- **Features:** Full local parity — local whisper CLI (default, free), Groq API, OpenAI API. Binary discovery in /opt/homebrew/bin + /usr/local/bin + PATH. ffmpeg audio conversion for non-WAV formats. Configurable command template (EDGECRAB_LOCAL_STT_COMMAND), model, language. Model auto-correction across providers.
- **Tests:** 5 unit tests

## New Gateway Adapters

### 4. Slack (crates/edgecrab-gateway/src/slack.rs)
- **Matches:** hermes `gateway/platforms/slack.py`
- **Features:** Socket Mode (WebSocket) for receiving, Web API for sending. Bot mention stripping, thread support, message splitting at 39k chars, mrkdwn formatting.
- **Env:** `SLACK_BOT_TOKEN` + `SLACK_APP_TOKEN`
- **Tests:** 5 unit tests

### 5. Signal (crates/edgecrab-gateway/src/signal.rs)
- **Matches:** hermes `gateway/platforms/signal.py`
- **Features:** SSE listener for inbound, JSON-RPC 2.0 for outbound via signal-cli HTTP daemon. Group + DM support, phone number redaction, exponential backoff reconnection, 8k char limit.
- **Env:** `SIGNAL_HTTP_URL` + `SIGNAL_ACCOUNT`
- **Tests:** 7 unit tests

## Feature Parity Matrix

| Feature Area | Hermes | Edgecrab | Status |
|---|---|---|---|
| File tools | read/write/search/patch | read_file/write_file/search_files/patch | ✅ Parity |
| Terminal | terminal + background=True | terminal + run_process/list_processes/kill_process | ✅ Better separation |
| Web | web_search/web_extract | web_search/web_extract | ✅ Parity |
| Browser | 6 tools (browserbase) | 11 tools (chromiumoxide native) | ✅ Exceeds |
| Memory | memory_read/memory_write | memory_read/memory_write | ✅ Parity |
| Skills | skills_list/skill_view/install | skills_list/skill_view/skill_manage | ✅ Parity |
| MCP | mcp_tool (1050 lines) | mcp_list_tools/mcp_call_tool | ✅ Parity |
| Planning | todo/clarify | manage_todo_list/clarify | ✅ Parity |
| Code exec | execute_code/delegate | execute_code/delegate_task | ✅ Parity |
| Session | session search/FTS5 | session_search/checkpoint | ✅ Parity |
| TTS | tts_tool (3 providers) | text_to_speech | ✅ Parity |
| Vision | vision_tools | vision_analyze | ✅ NEW |
| STT | transcription_tools (3 backends) | transcribe_audio (3 backends) | ✅ NEW |
| Cron | cronjob_tools + scheduler | manage_cron_jobs (shared w/CLI) | ✅ NEW |
| Telegram | ✅ | ✅ | ✅ Parity |
| Discord | ✅ | ✅ | ✅ Parity |
| Slack | ✅ | ✅ | ✅ NEW |
| Signal | ✅ | ✅ | ✅ NEW |
| WhatsApp | ✅ | ✅ | ✅ Parity |
| Webhook | ✅ | ✅ | ✅ Parity |
| Prompt builder | ~12 sources | ~12 sources w/ injection scanning | ✅ Parity |
| Model routing | simple/complex | simple/complex w/ fallback | ✅ Parity |
| Compression | context compressor | trajectory_compressor | ✅ Parity |
| Skin engine | skin_engine.py | TUI skin w/ ratatui | ✅ Parity |
| Security | url_safety + approval | edgecrab-security (SSRF, path jail, injection) | ✅ Exceeds |

## Niche Hermes Features (Not Implemented — Low Priority)

- `homeassistant_tool.py` — Home Assistant integration (Platform enum exists, adapter not wired)
- `honcho_tools.py` — Honcho session management
- `mixture_of_agents_tool.py` — Multi-model mixture
- `rl_training_tool.py` — RL training environments
- `email.py`, `matrix.py`, `mattermost.py`, `dingtalk.py`, `sms.py` — Niche platform adapters

## Files Modified

- `crates/edgecrab-tools/src/tools/vision.rs` — NEW
- `crates/edgecrab-tools/src/tools/cron.rs` — NEW
- `crates/edgecrab-tools/src/tools/transcribe.rs` — NEW
- `crates/edgecrab-tools/src/tools/mod.rs` — Added vision, cron, transcribe modules
- `crates/edgecrab-tools/src/toolsets.rs` — Added to CORE_TOOLS
- `crates/edgecrab-tools/Cargo.toml` — Added base64, cron, chrono, which deps
- `crates/edgecrab-gateway/src/slack.rs` — NEW
- `crates/edgecrab-gateway/src/signal.rs` — NEW
- `crates/edgecrab-gateway/src/lib.rs` — Added slack, signal modules
- `crates/edgecrab-gateway/Cargo.toml` — Added tokio-tungstenite dep
- `crates/edgecrab-cli/Cargo.toml` — Changed cron to workspace dep
- `Cargo.toml` (workspace) — Added base64, cron, which to workspace deps
