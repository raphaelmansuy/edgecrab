---
title: Configuration
description: Complete configuration reference for EdgeCrab — directory layout, config precedence, all config.yaml sections, and EDGECRAB_* environment variables. Grounded in crates/edgecrab-core/src/config.rs.
sidebar:
  order: 2
---

All EdgeCrab settings live in `~/.edgecrab/`. Configuration is layered — later entries override earlier ones, so you can set-and-forget in `config.yaml` and override per-invocation from the shell.

---

## Directory Structure

```
~/.edgecrab/
├── config.yaml       # All settings (model, terminal, compression, memory, etc.)
├── .env              # API keys and secrets
├── SOUL.md           # Primary agent identity (slot #1 in system prompt)
├── AGENTS.md         # Project-agnostic instructions for every session
├── memories/         # Persistent memory files (auto-managed by agent)
├── skills/           # Agent skills (directories with SKILL.md)
├── cron/             # Scheduled job storage
├── checkpoints/      # Shadow git repos for rollback (per working directory)
├── profiles/         # Named profiles with isolated configs
├── skin.yaml         # TUI color and kaomoji customization
├── state.db          # SQLite session database (WAL mode)
├── plugins/          # Optional plugin binaries
└── logs/             # Error and gateway logs
```

Override the home directory with `EDGECRAB_HOME`:

```bash
export EDGECRAB_HOME=/opt/edgecrab
```

---

## Managing Configuration

```bash
edgecrab config show         # print active config as YAML
edgecrab config edit         # open config.yaml in $EDITOR
edgecrab config set <key> <value>
edgecrab config path         # print path to config.yaml
edgecrab config env-path     # print path to .env
```

The `set` command routes automatically — non-secret values go to `config.yaml`, API keys and tokens go to `.env`.

---

## Configuration Precedence

Settings resolve from lowest to highest priority:

1. **Compiled defaults** — `AppConfig::default()` in `config.rs`
2. **`~/.edgecrab/config.yaml`** — your primary config file
3. **`EDGECRAB_*` environment variables** — override specific keys at runtime
4. **CLI flags** — `--model`, `--toolset`, etc. (highest priority, per-invocation)

:::tip
Secrets (API keys, bot tokens) go in `.env`. Everything else goes in `config.yaml`. When both are set, `config.yaml` wins.
:::

---

## Model Configuration

```yaml
# ~/.edgecrab/config.yaml
model:
  default: "anthropic/claude-sonnet-4-20250514"  # Default model
  max_iterations: 90           # Max tool call iterations per conversation
  streaming: true              # Stream tokens to terminal
  prompt_caching: true         # Enable OpenAI/Anthropic prompt caching
  cache_ttl: 300               # Cache TTL in seconds
  max_tokens: ~                # Max response tokens (null = model default)
  temperature: ~               # Sampling temperature (null = model default)
  api_key_env: "OPENROUTER_API_KEY"  # Env var name for the API key
  base_url: ~                  # Custom OpenAI-compatible base URL

  # Fallback model when primary fails
  fallback:
    model: "copilot/gpt-4.1-mini"
    provider: "copilot"        # Provider to use for auth

  # Smart routing: use a cheap model for simple messages
  smart_routing:
    enabled: false
    cheap_model: "copilot/gpt-4.1-mini"

# Mixture-of-Agents defaults
moa:
  enabled: true
  aggregator_model: "copilot/gpt-5-mini"
  reference_models:
    - "copilot/gpt-5-mini"
```

`/moa reset` now rewrites this block to a safe baseline for the current chat
model, and runtime execution will still auto-use the current chat model as a
last-chance expert and aggregator fallback if a saved MoA roster is stale.

**Defaults (from `ModelConfig::default()`):**

| Key | Default |
|-----|---------|
| `default` | `"anthropic/claude-sonnet-4-20250514"` |
| `max_iterations` | `90` |
| `streaming` | `true` |
| `prompt_caching` | `true` |
| `cache_ttl` | `300` |

Override model per-invocation:

```bash
edgecrab --model copilot/gpt-4.1-mini "quick question"
edgecrab -m ollama/llama3.3 "offline task"
```

---

## Tools Configuration

```yaml
tools:
  enabled_toolsets: ~          # null = all toolsets; or list like ["coding"]
  disabled_toolsets: ~         # toolsets to disable even if in enabled list
  custom_groups:               # define your own toolset aliases
    my-group:
      - read_file
      - write_file
      - terminal
  tool_delay: 1.0              # seconds between consecutive tool calls
  parallel_execution: true     # allow concurrent tool calls
  max_parallel_workers: 8      # concurrency limit
```

Override toolsets per-invocation:

```bash
edgecrab --toolset coding "implement the feature"
edgecrab --toolset file,terminal "run tests and fix"
edgecrab --toolset all "maximum capability"
```

---

## Terminal Configuration

```yaml
terminal:
  shell: ~                     # null = user's login shell
  timeout: 120                 # per-command timeout in seconds
  env_passthrough: []          # env var names to forward to subprocesses
```

---

## Memory Configuration

```yaml
memory:
  enabled: true                # master switch for persistent memory
  auto_flush: true             # auto-write memory after each session
```

Disable memory for a single session:

```bash
edgecrab --skip-memory "no memory this session"
```

---

## Skills Configuration

```yaml
skills:
  enabled: true
  hub_url: ~                   # override skills hub URL
  disabled: []                 # globally disabled skill names
  platform_disabled:           # platform-specific disable
    telegram:
      - heavy-skill
  external_dirs:               # additional skill directories (read-only)
    - ~/.agents/skills
    - ${TEAM_SKILLS_DIR}/skills
  preloaded: []                # skills loaded into every session
```

---

```yaml
tools:
  file:
    allowed_roots: []           # extra roots beyond the active workspace cwd
```

`tools.file.allowed_roots` extends the file-tool workspace boundary for
`read_file`, `write_file`, `patch`, `search_files`, `apply_patch`, local
vision image reads, and `@file` / `@folder` context refs. Relative paths
still resolve from the active workspace. Use absolute paths when targeting an
extra allowed root.

---

## Security Configuration

```yaml
security:
  approval_required: []        # command patterns requiring user approval
  blocked_commands: []         # commands that are always blocked
  path_restrictions: []        # deny-list roots overriding workspace + allowed_roots
  injection_scanning: true     # scan for prompt injection in tool results
  url_safety: true             # block private IPs and SSRF targets
```

`url_safety` blocks: private IPv4 ranges, private IPv6, `localhost`, `169.254.169.254`, `metadata.google.internal`, and non-HTTP(S) URLs.

Set `EDGECRAB_MANAGED=1` to enable managed mode — blocks config writes (useful for shared deployments).

---

## Gateway Configuration

Control the messaging gateway server:

```yaml
gateway:
  host: "127.0.0.1"
  port: 8080
  webhook_enabled: true
  session_timeout_minutes: 30
  enabled_platforms: []        # auto-detected from env vars

  telegram:
    enabled: false             # auto-set when TELEGRAM_BOT_TOKEN is present
    token_env: "TELEGRAM_BOT_TOKEN"
    allowed_users: []          # empty = all users
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
    http_url: ~                # URL of signal-cli HTTP daemon
    account: ~                 # Phone number registered with signal-cli
    allowed_users: []

  whatsapp:
    enabled: false
    bridge_port: 3000
    bridge_url: ~
    mode: "self-chat"
    allowed_users: []
    install_dependencies: true
```

Platform env vars auto-enable their section:

| Platform | Required Env Var |
|----------|-----------------|
| Telegram | `TELEGRAM_BOT_TOKEN` |
| Discord | `DISCORD_BOT_TOKEN` |
| Slack | `SLACK_BOT_TOKEN` + `SLACK_APP_TOKEN` |
| Signal | `SIGNAL_HTTP_URL` + `SIGNAL_ACCOUNT` |
| WhatsApp | `WHATSAPP_ENABLED=1` |

---

## MCP Server Configuration

```yaml
mcp_servers:
  github:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-github"]
    env:
      GITHUB_PERSONAL_ACCESS_TOKEN: "ghp_xxx"
    enabled: true
    timeout: 30                # per-call timeout (seconds)
    connect_timeout: 10        # connection timeout (seconds)
    tools:
      include: []              # if non-empty, only expose listed tools
      exclude: []              # tools to hide
      resources: true          # enable list/read resource wrappers
      prompts: true            # enable list/get prompt wrappers

  filesystem:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/workspace"]

  my-http-server:
    url: "http://localhost:9001/mcp"
    bearer_token: "my-static-token"
    headers:
      X-Custom-Header: "value"
```

Manage without editing YAML:

```bash
edgecrab mcp list
edgecrab mcp add github npx -y @modelcontextprotocol/server-github
edgecrab mcp remove github
```

---

## Context Compression

EdgeCrab automatically compresses long conversations to stay within the model's context window:

```yaml
compression:
  enabled: true
  threshold: 0.50              # compress when context exceeds 50% of window
  target_ratio: 0.20           # keep 20% of recent messages uncompressed
  protect_last_n: 20           # always keep the last 20 messages
  summary_model: ~             # null = use main model for summarization
```

Trigger manually:

```
/compress
```

---

## Delegation (Subagents)

Configure how the `delegate_task` tool spawns subagents:

```yaml
delegation:
  enabled: true
  model: ~                     # null = inherit parent model
  provider: ~                  # null = inherit parent provider
  base_url: ~                  # direct OpenAI-compatible endpoint
  max_subagents: 3             # max concurrent subagents
  max_iterations: 50           # max tool iterations per subagent
  shared_budget: false         # share parent's iteration budget
```

Example: use a cheap model for subtasks:

```yaml
delegation:
  model: "copilot/gpt-4.1-mini"
  provider: "copilot"
  max_subagents: 5
```

---

## Display & Appearance

```yaml
display:
  compact: false               # reduce whitespace in output
  personality: "helpful"       # default personality preset
  show_reasoning: false        # show model thinking tokens
  streaming: true              # stream response tokens
  tool_progress: verbose       # off | new | all | verbose (default: verbose)
  show_cost: true              # show cost in status bar
  show_status_bar: true        # show the bottom status bar
  check_for_updates: true      # show startup notice when a newer release exists
  update_check_interval_hours: 24  # refresh cadence for background update checks
  skin: "default"              # skin name from ~/.edgecrab/skin.yaml
```

Built-in personalities: `helpful`, `concise`, `technical`, `kawaii`, `pirate`, `philosopher`, `hype`, `shakespeare`, `noir`, `catgirl`, `creative`, `teacher`, `surfer`, `uwu`.

`tool_progress` controls how much tool activity appears in the transcript:

| Value | Behaviour |
|-------|----------|
| `off` | Silent — no tool lines in transcript (status bar still shows active work) |
| `new` | Show each distinct tool call once per turn |
| `all` | Show every tool call |
| `verbose` | Show every tool call plus curated detail lines for plan/result context (**default**) |

You can also toggle live in the TUI with `/verbose` (cycles) or `/verbose <mode>` (set directly).

---

## Privacy

```yaml
privacy:
  redact_pii: false            # strip PII (phone numbers, user IDs) from LLM context
```

When enabled, applies to gateway platforms (Telegram, WhatsApp, Signal). Hashes are deterministic — the same user always maps to the same hash.

---

## Checkpoints & Rollback

```yaml
checkpoints:
  enabled: true                # create shadow git commits before destructive ops
  max_snapshots: 50            # max checkpoints per working directory
```

See [Checkpoints & Rollback](/user-guide/checkpoints/) for the full guide.

---

## TTS Configuration

```yaml
tts:
  provider: "edge-tts"         # "edge-tts" | "openai" | "elevenlabs"
  voice: "en-US-AriaNeural"    # provider-specific voice name
  rate: ~                      # edge-tts rate modifier (e.g. "+10%")
  model: ~                     # openai TTS model (e.g. "tts-1-hd")
  auto_play: true              # auto-play in voice mode

  # ElevenLabs options
  elevenlabs_voice_id: ~
  elevenlabs_model_id: ~
  elevenlabs_api_key_env: "ELEVENLABS_API_KEY"
```

---

## STT Configuration

```yaml
stt:
  provider: "local"            # "local" (whisper) | "groq" | "openai"
  whisper_model: "base"        # local: tiny|base|small|medium|large-v3
  silence_threshold: -40.0     # dB for voice activity detection
  silence_duration_ms: 1500    # ms of silence before auto-stop
```

---

## Voice Mode

```yaml
voice:
  enabled: false               # enable voice mode components
  push_to_talk_key: "ctrl+b"  # push-to-talk key binding
  continuous: false            # continuous listening (no key press)
  hallucination_filter: true   # filter STT hallucinations
```

Enable voice in the TUI:

```
/voice on       # enable microphone input
/voice tts      # toggle spoken replies
```

---

## Honcho User Modeling

```yaml
honcho:
  enabled: true                # persistent cross-session user modeling
  cloud_sync: false            # sync to Honcho cloud (requires HONCHO_API_KEY)
  api_key_env: "HONCHO_API_KEY"
  api_url: "https://api.honcho.dev/v1"
  max_context_entries: 10      # entries injected into system prompt
  write_frequency: 0           # auto-conclude every N messages (0 = manual)
```

---

## Auxiliary Models

```yaml
auxiliary:
  model: ~                     # auxiliary model identifier
  provider: ~                  # provider for auxiliary tasks
  base_url: ~                  # custom OpenAI-compatible endpoint
  api_key_env: ~               # env var for API key
```

Auxiliary models are used for compression summaries and TTS prompts. Defaults to the main model.

---

## Reasoning Effort

```yaml
reasoning_effort: ""           # "" | "low" | "medium" | "high" | "xhigh"
```

Empty string = medium (default). Change mid-session:

```
/reasoning high      # increase reasoning depth
/reasoning off       # disable reasoning
/reasoning show      # display model thinking tokens
```

---

## Timezone

```yaml
timezone: ""                   # "" = server-local; or IANA string e.g. "America/New_York"
```

Affects timestamps in logs, cron scheduling, and the system prompt time injection.

---

## Browser Automation

```yaml
browser:
  command_timeout: 30          # CDP call timeout in seconds
  record_sessions: false       # auto-record sessions as WebM video
  recording_max_age_hours: 72  # auto-delete recordings older than this
```

---

## Environment Variable Overrides

Key `EDGECRAB_*` variables (applied via `apply_env_overrides` in `config.rs`):

| Variable | Config key | Description |
|----------|------------|-------------|
| `EDGECRAB_MODEL` | `model.default` | Override default model |
| `EDGECRAB_MAX_ITERATIONS` | `model.max_iterations` | Max agent iterations |
| `EDGECRAB_TIMEZONE` | `timezone` | IANA timezone string |
| `EDGECRAB_SAVE_TRAJECTORIES` | `save_trajectories` | Enable trajectory logging |
| `EDGECRAB_SKIP_CONTEXT_FILES` | `skip_context_files` | Skip auto-loading context files |
| `EDGECRAB_SKIP_MEMORY` | `skip_memory` | Disable memory for this session |
| `EDGECRAB_GATEWAY_HOST` | `gateway.host` | Gateway bind host |
| `EDGECRAB_GATEWAY_PORT` | `gateway.port` | Gateway bind port |
| `EDGECRAB_TTS_PROVIDER` | `tts.provider` | TTS provider override |
| `EDGECRAB_TTS_VOICE` | `tts.voice` | TTS voice override |
| `EDGECRAB_REASONING_EFFORT` | `reasoning_effort` | Reasoning effort level |
| `EDGECRAB_HOME` | — | Override `~/.edgecrab` home directory |
| `EDGECRAB_MANAGED` | `security.managed_mode` | Block config writes (`1` to enable) |
| `TELEGRAM_BOT_TOKEN` | `gateway.telegram.enabled` | Auto-enable Telegram |
| `DISCORD_BOT_TOKEN` | `gateway.discord.enabled` | Auto-enable Discord |
| `SLACK_BOT_TOKEN` | `gateway.slack.enabled` | Auto-enable Slack (with `SLACK_APP_TOKEN`) |
| `SIGNAL_HTTP_URL` | `gateway.signal.enabled` | Auto-enable Signal (with `SIGNAL_ACCOUNT`) |
| `HONCHO_API_KEY` | `honcho.cloud_sync` | Enable Honcho cloud sync |

See [Environment Variables Reference](/reference/environment-variables/) for the full list.
