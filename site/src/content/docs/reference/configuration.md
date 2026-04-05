---
title: Configuration Reference
description: Complete config.yaml reference for EdgeCrab — all sections, fields, types, and defaults. Grounded in crates/edgecrab-core/src/config.rs AppConfig struct.
sidebar:
  order: 2
---

This is the complete configuration reference. All fields are optional — unset fields use their compiled defaults. The file lives at `~/.edgecrab/config.yaml`.

:::tip
Run `edgecrab config show` to print your active resolved configuration (after env override merging).
:::

---

## Full Annotated config.yaml

```yaml
# ~/.edgecrab/config.yaml

# ── Model ──────────────────────────────────────────────────────────────
model:
  default: "anthropic/claude-sonnet-4-20250514"  # Default LLM model
  max_iterations: 90             # Max tool calls per session
  streaming: true                # Stream tokens to TUI
  prompt_caching: true           # Prompt caching (Anthropic / OpenAI)
  cache_ttl: 300                 # Cache TTL in seconds
  max_tokens: ~                  # null = model default
  temperature: ~                 # null = model default
  api_key_env: "OPENROUTER_API_KEY"  # API key env name
  base_url: ~                    # Custom OpenAI-compatible URL

  fallback:
    model: "copilot/gpt-4.1-mini"  # null = no fallback
    provider: "copilot"

  smart_routing:
    enabled: false               # Route simple messages to cheap_model
    cheap_model: ""

# ── Tools ──────────────────────────────────────────────────────────────
tools:
  enabled_toolsets: ~            # null = all; list = whitelist
  disabled_toolsets: ~           # toolsets to always remove
  custom_groups: {}              # custom alias → tool list
  file:
    allowed_roots: []            # extra readable/writable roots beyond the workspace cwd
  tool_delay: 1.0                # seconds between tool calls
  parallel_execution: true       # allow concurrent tool calls
  max_parallel_workers: 8        # concurrency limit

# ── Memory ─────────────────────────────────────────────────────────────
memory:
  enabled: true
  auto_flush: true               # auto-save memory at session end

# ── Skills ─────────────────────────────────────────────────────────────
skills:
  enabled: true
  hub_url: ~                     # null = default hub
  disabled: []                   # globally disabled skill names
  platform_disabled: {}          # platform: [skill-name, ...]
  external_dirs: []              # extra skill directories
  preloaded: []                  # skills loaded every session

# ── Security ───────────────────────────────────────────────────────────
security:
  approval_required: []          # command patterns requiring approval
  blocked_commands: []           # patterns always blocked
  path_restrictions: []          # deny-list roots overriding workspace + allowed_roots
  injection_scanning: true       # scan tool results for prompt injection
  url_safety: true               # block private IPs and SSRF targets
  managed_mode: false            # block config writes

# ── Terminal ───────────────────────────────────────────────────────────
terminal:
  shell: ~                       # null = login shell
  timeout: 120                   # seconds per command
  env_passthrough: []            # env var names to forward

# ── Gateway ────────────────────────────────────────────────────────────
gateway:
  host: "127.0.0.1"
  port: 8080
  webhook_enabled: true
  session_timeout_minutes: 30
  enabled_platforms: []

  telegram:
    enabled: false
    token_env: "TELEGRAM_BOT_TOKEN"
    allowed_users: []
    home_channel: ~

  discord:
    enabled: false
    token_env: "DISCORD_BOT_TOKEN"
    allowed_users: []
    home_channel: ~

  slack:
    enabled: false
    bot_token_env: "SLACK_BOT_TOKEN"
    app_token_env: "SLACK_APP_TOKEN"
    allowed_users: []
    home_channel: ~

  signal:
    enabled: false
    http_url: ~
    account: ~
    allowed_users: []

  whatsapp:
    enabled: false
    bridge_port: 3000
    bridge_url: ~
    mode: "self-chat"
    allowed_users: []
    install_dependencies: true

# ── MCP Servers ────────────────────────────────────────────────────────
mcp_servers: {}
# Example:
# mcp_servers:
#   github:
#     command: npx
#     args: ["-y", "@modelcontextprotocol/server-github"]
#     env:
#       GITHUB_PERSONAL_ACCESS_TOKEN: "ghp_xxx"
#     enabled: true
#     timeout: 30
#     tools:
#       include: []
#       exclude: []

# ── Delegation ─────────────────────────────────────────────────────────
delegation:
  enabled: true
  model: ~
  provider: ~
  base_url: ~
  max_subagents: 3
  max_iterations: 50
  shared_budget: false

# ── Compression ────────────────────────────────────────────────────────
compression:
  enabled: true
  threshold: 0.50                # compress when context > 50% of window
  target_ratio: 0.20             # keep 20% uncompressed
  protect_last_n: 20             # always keep last N messages
  summary_model: ~               # null = use main model

# ── Display ────────────────────────────────────────────────────────────
display:
  compact: false
  personality: "helpful"
  show_reasoning: false
  streaming: true
  show_cost: true
  skin: "default"

# ── Privacy ────────────────────────────────────────────────────────────
privacy:
  redact_pii: false

# ── Browser ────────────────────────────────────────────────────────────
browser:
  command_timeout: 30
  record_sessions: false
  recording_max_age_hours: 72

# ── Checkpoints ────────────────────────────────────────────────────────
checkpoints:
  enabled: true
  max_snapshots: 50

# ── TTS ────────────────────────────────────────────────────────────────
tts:
  provider: "edge-tts"           # "edge-tts" | "openai" | "elevenlabs"
  voice: "en-US-AriaNeural"
  rate: ~
  model: ~
  auto_play: true
  elevenlabs_voice_id: ~
  elevenlabs_model_id: ~
  elevenlabs_api_key_env: "ELEVENLABS_API_KEY"

# ── STT ────────────────────────────────────────────────────────────────
stt:
  provider: "local"              # "local" | "groq" | "openai"
  whisper_model: "base"          # tiny|base|small|medium|large-v3
  silence_threshold: -40.0       # dB
  silence_duration_ms: 1500

# ── Voice ──────────────────────────────────────────────────────────────
voice:
  enabled: false
  push_to_talk_key: "ctrl+b"
  continuous: false
  hallucination_filter: true

# ── Honcho ─────────────────────────────────────────────────────────────
honcho:
  enabled: true
  cloud_sync: false
  api_key_env: "HONCHO_API_KEY"
  api_url: "https://api.honcho.dev/v1"
  max_context_entries: 10
  write_frequency: 0             # 0 = manual conclude only

# ── Auxiliary Model ────────────────────────────────────────────────────
auxiliary:
  model: ~
  provider: ~
  base_url: ~
  api_key_env: ~

# ── Reasoning ──────────────────────────────────────────────────────────
reasoning_effort: ""             # "" | "low" | "medium" | "high" | "xhigh"

# ── Timezone ───────────────────────────────────────────────────────────
timezone: ""                     # "" = system timezone; IANA format
```

---

## Field Index

| Config Key | Type | Default | Env Override |
|------------|------|---------|-------------|
| `model.default` | string | `anthropic/claude-sonnet-4-20250514` | `EDGECRAB_MODEL` |
| `model.max_iterations` | integer | `90` | `EDGECRAB_MAX_ITERATIONS` |
| `model.streaming` | bool | `true` | — |
| `model.prompt_caching` | bool | `true` | — |
| `model.cache_ttl` | integer | `300` | — |
| `tools.tool_delay` | float | `1.0` | — |
| `tools.parallel_execution` | bool | `true` | — |
| `tools.max_parallel_workers` | integer | `8` | — |
| `terminal.timeout` | integer | `120` | — |
| `delegation.max_subagents` | integer | `3` | — |
| `delegation.max_iterations` | integer | `50` | — |
| `compression.threshold` | float | `0.50` | — |
| `compression.protect_last_n` | integer | `20` | — |
| `checkpoints.max_snapshots` | integer | `50` | — |
| `tts.provider` | string | `edge-tts` | `EDGECRAB_TTS_PROVIDER` |
| `tts.voice` | string | `en-US-AriaNeural` | `EDGECRAB_TTS_VOICE` |
| `stt.whisper_model` | string | `base` | — |
| `voice.push_to_talk_key` | string | `ctrl+b` | — |
| `honcho.max_context_entries` | integer | `10` | — |
| `gateway.port` | integer | `8080` | `EDGECRAB_GATEWAY_PORT` |
| `gateway.host` | string | `127.0.0.1` | `EDGECRAB_GATEWAY_HOST` |
| `timezone` | string | `""` | `EDGECRAB_TIMEZONE` |
| `reasoning_effort` | string | `""` | `EDGECRAB_REASONING_EFFORT` |

---

## Pro Tips

- **Use `edgecrab config show`** to see the fully-merged config (YAML + env overrides) without editing files.
- **Use `edgecrab config set model.max_iterations 150`** for large refactors instead of editing the YAML manually.
- **`EDGECRAB_*` env vars always override `config.yaml`**: perfect for CI — set env vars in your pipeline and never touch the baked-in config file.
- **`compression.threshold: 0.35`** triggers summarisation earlier so context windows stay comfortable for models with smaller windows (32K-class local models).
- **`security.managed_mode: true`** locks config writes at runtime — the agent cannot change its own settings (useful in production gateways).

---

## FAQ

**Where does the config file go?**
`~/.edgecrab/config.yaml`. Override with `--config <path>` or `EDGECRAB_HOME=<dir>`.

**What happens if I have a typo in config.yaml?**
EdgeCrab reports a YAML parse error and exits. Run `edgecrab doctor` to diagnose. Use `edgecrab config show` to preview the merged config before a long session.

**Can I have multiple config files (e.g. work vs. personal)?**
Yes — use Profiles (`edgecrab profile create work`) or pass `--config ~/work-config.yaml` per invocation.

**How do I set a custom `base_url` for a non-OpenAI provider?**
In `config.yaml`:
```yaml
model:
  default: "my-provider/my-model"
  base_url: "https://api.myprovider.com/v1"
  api_key_env: "MY_PROVIDER_API_KEY"
```

**Does changing `model.max_iterations` affect memory or cost?**
Higher `max_iterations` allows longer tool-call chains, which can increase cost and latency on large tasks. It does not affect memory. The agent still stops early if the task completes before the limit.

---

## See Also

- [Environment Variables](/reference/environment-variables/) — env override reference
- [CLI Commands](/reference/cli-commands/) — CLI flags that override config at runtime
- [Security Model](/features/security/) — `security.*` config keys explained
