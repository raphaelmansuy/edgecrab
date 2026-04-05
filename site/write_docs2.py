#!/usr/bin/env python3
"""Write docs batch 2: configuration.md, worktrees.md, checkpoints.md, profiles.md"""
import os

BASE = "src/content/docs"

# ─── user-guide/configuration.md ─────────────────────────────────────
configuration = r"""---
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
```

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

## Security Configuration

```yaml
security:
  approval_required: []        # command patterns requiring user approval
  blocked_commands: []         # commands that are always blocked
  path_restrictions: []        # paths the agent cannot access
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
  show_cost: true              # show cost in status bar
  skin: "default"              # skin name from ~/.edgecrab/skin.yaml
```

Built-in personalities: `helpful`, `concise`, `technical`, `kawaii`, `pirate`, `philosopher`, `hype`, `shakespeare`, `noir`, `catgirl`, `creative`, `teacher`, `surfer`, `uwu`.

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
"""

with open(f"{BASE}/user-guide/configuration.md", "w") as f:
    f.write(configuration)
print("configuration.md written")

# ─── user-guide/worktrees.md ──────────────────────────────────────────
worktrees = r"""---
title: Git Worktrees
description: Run multiple EdgeCrab agents in parallel on the same repository using isolated git worktrees. Grounded in crates/edgecrab-cli/src/cli_args.rs.
sidebar:
  order: 7
---

Git worktrees let you run multiple EdgeCrab sessions in parallel on the same repository without agents interfering with each other. Each session gets its own branch and working directory.

---

## When to Use Worktrees

Use worktrees when you want to:

- Run two agents simultaneously on the same repo
- Try an approach in isolation before deciding to keep it
- Work on a feature while an agent handles an unrelated bug fix
- Evaluate different prompting strategies in parallel

---

## Enabling Worktrees

### Per-invocation

```bash
edgecrab -w "refactor the authentication module"
edgecrab --worktree "add rate limiting to the API"
```

Each `-w` invocation creates a new branch and worktree under `.worktrees/` in your current git repository.

### Always-on

```yaml
# ~/.edgecrab/config.yaml
# Not directly supported in config — use -w flag per session
```

### One-shot (quiet mode)

```bash
edgecrab -w -q "write tests for the parser module" | tee output.txt
```

---

## How It Works

When you run `edgecrab -w`:

1. EdgeCrab creates a new branch: `edgecrab/<timestamp>-<short-hash>`
2. Creates a worktree at `.worktrees/<branch-name>/`
3. Starts the agent session from that worktree directory
4. **On exit:** If the worktree has no uncommitted changes, it is removed automatically. If changes exist, the worktree is preserved for manual recovery.

```
my-project/
├── src/              # main branch
├── .worktrees/
│   ├── edgecrab-1714832400-a1b2c3/   # agent session 1
│   └── edgecrab-1714832450-d4e5f6/   # agent session 2
```

---

## Parallel Workflow Example

```bash
# Terminal 1 — refactor auth
edgecrab -w "refactor authentication to use JWT with refresh tokens"

# Terminal 2 — add tests
edgecrab -w "write comprehensive unit tests for the user module"

# Terminal 3 — fix a bug
edgecrab -w "fix the race condition in the session manager"
```

All three agents work in isolation. When done, review each branch, cherry-pick what you want, and clean up:

```bash
git branch -a                  # list all edgecrab branches
git diff main edgecrab/...     # review changes
git merge edgecrab/...         # merge good work
git branch -D edgecrab/...     # clean up
```

---

## Including Gitignored Files

By default, worktrees don't inherit gitignored files (`.env`, `node_modules/`, `.venv/`, etc.). Create a `.worktreeinclude` file in your repo root to copy specified patterns into each worktree:

```
# .worktreeinclude
.env
.venv/
node_modules/
.cargo/
```

Files matching these patterns are copied (not symlinked) into new worktrees before the agent starts.

---

## Worktrees in Config (Global Toggle)

To always use worktrees without the `-w` flag, there's no direct config key — but you can create a shell alias:

```bash
alias ec='edgecrab -w'
```

Or set your default workflow in a profile:

```bash
edgecrab profile create isolated
# edit ~/.edgecrab/profiles/isolated/config.yaml
edgecrab -p isolated "task requiring isolation"
```

---

## Cleaning Up

Stale worktrees that weren't cleaned automatically (e.g. the agent crashed):

```bash
# List all worktrees
git worktree list

# Remove a stale worktree
git worktree remove .worktrees/edgecrab-1714832400-a1b2c3
git branch -D edgecrab/1714832400-a1b2c3
```

Or prune all worktrees whose directories no longer exist:

```bash
git worktree prune
```
"""

with open(f"{BASE}/user-guide/worktrees.md", "w") as f:
    f.write(worktrees)
print("worktrees.md written")

# ─── user-guide/checkpoints.md ────────────────────────────────────────
checkpoints = r"""---
title: Checkpoints & Rollback
description: Automatic filesystem snapshots via shadow git — how EdgeCrab checkpoints before destructive operations and how to roll back. Grounded in crates/edgecrab-tools/src/tools/checkpoint.rs.
sidebar:
  order: 8
---

EdgeCrab automatically creates filesystem checkpoints before any destructive file operation — `write_file`, `patch`, and certain terminal commands. Checkpoints are stored as commits in a shadow git repository scoped to your working directory. If something goes wrong, you can roll back to any prior checkpoint.

---

## How Checkpoints Work

Before every destructive operation, EdgeCrab:

1. Checks if `checkpoints.enabled` is `true` (default)
2. Resolves the shadow git repository path: `~/.edgecrab/checkpoints/<sha256_of_cwd>/`
3. Stages all tracked files in the working directory
4. Creates a commit with a label like `checkpoint: before write_file src/main.rs`

The shadow repo is fully isolated from your project's git history — it tracks files for rollback purposes only, never interfering with your commits or branches.

---

## Configuration

```yaml
# ~/.edgecrab/config.yaml
checkpoints:
  enabled: true          # master switch (default: true)
  max_snapshots: 50      # max checkpoints per working directory
```

Enable/disable per-session:

```bash
edgecrab --checkpoints    # (flag not yet in v0.x; use config)
```

Disable globally:

```yaml
checkpoints:
  enabled: false
```

---

## Viewing Checkpoints

Inside the TUI:

```
/rollback           # opens the rollback UI showing recent checkpoints
```

This shows a numbered list of available checkpoints with their labels and timestamps.

---

## Rolling Back

From the TUI, after `/rollback`, select the checkpoint number to restore to. EdgeCrab:

1. Shows a diff of what will change
2. Asks for confirmation
3. Restores the files from the selected checkpoint

From the command line, use the tools via the agent:

```
> Show me all checkpoints for this project
> Roll back to checkpoint 3
> What changed between checkpoints 2 and 4?
> Restore just the file src/main.rs from checkpoint 5
```

The agent uses these internal operations (exposed through `manage_checkpoints` tool internally):

| Operation | Description |
|-----------|-------------|
| `checkpoint create "label"` | Manually create a checkpoint |
| `checkpoint list` | List all checkpoints for the CWD |
| `checkpoint restore N` | Restore all files to checkpoint N |
| `checkpoint diff N` | Show diff between current state and checkpoint N |
| `checkpoint restore_file N <file>` | Restore a single file from checkpoint N |

---

## Manual Checkpoints

Create a checkpoint before a risky operation:

```
❯ Before we do the big refactor, create a checkpoint
```

The agent calls `checkpoint create "before refactor"`.

Or use the slash command:

```
/rollback               # opens interactive rollback, also shows create option
```

---

## Checkpoint Storage

Checkpoints are stored in `~/.edgecrab/checkpoints/<sha256_of_cwd>/` — one shadow git repo per working directory. This means:

- Working on `~/project-a` keeps its checkpoints separate from `~/project-b`
- The shadow repos are never visible in your project's git log
- Old checkpoints are pruned when `max_snapshots` is exceeded (oldest first)

---

## What Gets Checkpointed

| Operation | Checkpoint created? |
|-----------|---------------------|
| `write_file` | ✅ Before writing |
| `patch` | ✅ Before patching |
| Terminal commands flagged as destructive | ✅ Before execution |
| `read_file` | ❌ Read-only |
| `web_search` | ❌ No file changes |

---

## Disabling for a Session

```bash
# Currently disable globally in config.yaml
checkpoints:
  enabled: false
```

When disabled, no shadow git repos are created or written. Previously created checkpoints remain on disk until pruned.
"""

with open(f"{BASE}/user-guide/checkpoints.md", "w") as f:
    f.write(checkpoints)
print("checkpoints.md written")

# ─── user-guide/profiles.md ───────────────────────────────────────────
profiles = r"""---
title: Profiles
description: Named profiles with isolated home directories — separate config, env, SOUL.md, memory, skills, and state per profile. Grounded in crates/edgecrab-cli/src/profile.rs.
sidebar:
  order: 9
---

Profiles let you run EdgeCrab with completely isolated configurations. Each profile has its own `config.yaml`, `.env`, `SOUL.md`, memories, skills, and state database — making it easy to maintain separate agent identities for work, personal use, different clients, or different projects.

---

## Profile Directory Structure

Each profile lives under `~/.edgecrab/profiles/<name>/`:

```
~/.edgecrab/profiles/
├── work/
│   ├── config.yaml       # Work-specific model, toolsets, etc.
│   ├── .env              # Work API keys
│   ├── SOUL.md           # Work agent identity
│   ├── memories/         # Work-specific memory
│   ├── skills/           # Work-specific skills
│   └── state.db          # Work sessions database
├── personal/
│   ├── config.yaml
│   ├── .env
│   ├── SOUL.md
│   └── ...
└── client-acme/
    └── ...
```

The active profile is tracked in `~/.edgecrab/.active_profile`. Shell aliases are created at `~/.local/bin/<profile_name>` as thin wrappers for `edgecrab -p <name>`.

---

## Managing Profiles

### List Profiles

```bash
edgecrab profile list
```

Output:
```
  default   (built-in)
* work      ~/.edgecrab/profiles/work/
  personal  ~/.edgecrab/profiles/personal/
```

`*` marks the active profile.

### Create a Profile

```bash
edgecrab profile create work
edgecrab profile create client-acme --clone work   # clone from an existing profile
```

This creates the profile directory with default config files. Edit them to customize:

```bash
edgecrab -p work config edit    # edit work profile config
```

### Switch Active Profile

```bash
edgecrab profile use work
```

All subsequent `edgecrab` invocations use the work profile until you switch again.

### Delete a Profile

```bash
edgecrab profile delete client-acme
```

This removes the entire `~/.edgecrab/profiles/client-acme/` directory permanently.

### Show Profile Info

```bash
edgecrab profile show            # show active profile
edgecrab profile show work       # show a specific profile
edgecrab profile path            # print active profile home path
edgecrab profile path work       # print a specific profile's path
```

---

## Running Under a Profile

Use `-p` / `--profile` to run EdgeCrab under a specific profile without switching the active profile:

```bash
edgecrab -p work "open a PR for the auth refactor"
edgecrab -p personal "help me plan my vacation"
edgecrab -p client-acme -S deploy-aws "deploy the staging environment"
```

---

## Shell Aliases

When you create a profile, EdgeCrab registers a shell alias at `~/.local/bin/<name>` (if that directory is in `$PATH`). This lets you invoke profiles directly:

```bash
# After: edgecrab profile create work
work "open a PR for the auth refactor"

# After: edgecrab profile create personal
personal "what should I make for dinner?"
```

The alias is a thin wrapper:

```bash
#!/bin/bash
exec edgecrab -p work "$@"
```

---

## Profile-Specific SOUL.md

Each profile can have a different agent identity. Edit `~/.edgecrab/profiles/<name>/SOUL.md` to define the persona:

```markdown
# Work Profile Agent

You are a professional software engineering assistant. You work on production Rust
systems. Be concise, precise, and always reference actual code. Never mock
implementations — only write or suggest code that actually compiles and works.
```

```markdown
# Personal Profile Agent

You are a helpful personal assistant. Help with task planning, research,
writing, cooking, and life organization. Be warm and conversational.
```

---

## Profile Isolation

Profiles are fully isolated:

| Resource | Isolated per profile? |
|----------|-----------------------|
| `config.yaml` | ✅ |
| `.env` (API keys) | ✅ |
| `SOUL.md` (identity) | ✅ |
| `memories/` | ✅ |
| `skills/` | ✅ |
| `state.db` (sessions) | ✅ |
| Binary / version | ❌ (shared) |
| Cron jobs | ❌ (shared `~/.edgecrab/cron/`) |

---

## Example: Work vs Personal

```bash
# Create work profile with high reasoning model
edgecrab profile create work
cat > ~/.edgecrab/profiles/work/config.yaml << 'EOF'
model:
  default: "anthropic/claude-opus-4"
  max_iterations: 90
tools:
  enabled_toolsets: ["coding"]
reasoning_effort: "high"
EOF

# Create personal profile with fast cheap model
edgecrab profile create personal
cat > ~/.edgecrab/profiles/personal/config.yaml << 'EOF'
model:
  default: "copilot/gpt-4.1-mini"
  max_iterations: 30
display:
  personality: "helpful"
EOF
```

Now just run:

```bash
work "refactor the auth module"     # uses claude-opus-4, reasoning=high
personal "plan my weekend"          # uses gpt-4.1-mini, friendly tone
```
"""

with open(f"{BASE}/user-guide/profiles.md", "w") as f:
    f.write(profiles)
print("profiles.md written")

print("Batch 2 complete")
