# 🦀 Configuration and Paths

> **WHY**: A single binary that works on a laptop, a Raspberry Pi, and a cloud server needs a deterministic, layered config system. EdgeCrab uses a 4-tier merge stack so every default can be overridden at exactly the right scope — without touching unrelated settings.

**Source**: `crates/edgecrab-core/src/config.rs`, `crates/edgecrab-cli/src/profile.rs`

---

## The 4-Tier Load Order

```
┌─────────────────────────────────────────┐
│  Tier 1 — Compiled-in defaults          │  always present, never missing
└──────────────────────┬──────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────┐
│  Tier 2 — config.yaml                   │  $EDGECRAB_HOME/config.yaml
│            (or ~/.edgecrab/config.yaml) │  or profile home/config.yaml
└──────────────────────┬──────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────┐
│  Tier 3 — EDGECRAB_* env vars           │  EDGECRAB_MODEL, EDGECRAB_MAX_ITERATIONS…
└──────────────────────┬──────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────┐
│  Tier 4 — CLI flags                     │  --model, --iterations, --no-memory…
└─────────────────────────────────────────┘
                       │
                       ▼
                  AppConfig
             (single merged view)
```

**Rule**: later tier always wins. A CLI flag beats an env var; an env var beats config.yaml; config.yaml beats the compiled default.

---

## `AppConfig` Top-Level Sections

| Section | Purpose |
|---|---|
| `model` | Default model, temperature, context window |
| `agent` | Max iterations, reflection threshold |
| `tools` | Toolset allowlist, MCP server list |
| `gateway` | Platform adapter settings (Telegram token, Slack credentials…) |
| `mcp_servers` | MCP server definitions (name, command, args, env) |
| `memory` | File-backed memory enable/disable, max tokens |
| `skills` | Skills directory, auto-discovery |
| `security` | Approval mode, command-scan policy, path jail roots |
| `terminal` | Shell, PTY settings |
| `delegation` | Sub-agent concurrency, budget |
| `compression` | Trigger threshold, target ratio, summary model |
| `display` | Colour, TUI, streaming |
| `privacy` | Redaction patterns, telemetry opt-out |
| `browser` | Playwright/CDP settings |
| `checkpoints` | Frequency, storage path |
| `tts` / `stt` / `voice` | Audio I/O settings |
| `image_generation` | Default image-generation backend and settings |
| `honcho` | Honcho user-model memory service |
| `auxiliary` | Auxiliary model settings such as vision overrides |
| `moa` | Default Mixture-of-Agents aggregator and reference roster |

Top-level runtime flags (not nested in a section):

| Flag | Default | Meaning |
|---|---|---|
| `save_trajectories` | `false` | Write JSONL replay files after each session |
| `worktree` | `false` | Launch agent sessions in isolated git worktrees by default |
| `logging.level` | `"info"` | Default centralized log verbosity |
| `skip_context_files` | `false` | Skip `CLAUDE.md` / `AGENT.md` injection |
| `skip_memory` | `false` | Skip file-backed memory injection |
| `timezone` | system TZ | Overrides tz for cron and timestamps |
| `reasoning_effort` | `"medium"` | Passed to models that support it |

---

## Key Environment Variables

```bash
# Model override — fastest way to try a new model without editing config.yaml
EDGECRAB_MODEL="anthropic/claude-opus-4-6-20260219"

# Safety ceiling — refuse to loop more than N times per session
EDGECRAB_MAX_ITERATIONS=40

# Force UTC regardless of local machine timezone
EDGECRAB_TIMEZONE="UTC"

# Write JSONL trajectory files for every session
EDGECRAB_SAVE_TRAJECTORIES=true

# Launch agent sessions in isolated git worktrees by default
EDGECRAB_WORKTREE=true

# Override the default centralized logging level
EDGECRAB_LOG_LEVEL=debug

# Skip injecting CLAUDE.md / AGENT.md files entirely
EDGECRAB_SKIP_CONTEXT_FILES=true

# Disable file-backed memory injection
EDGECRAB_SKIP_MEMORY=true

# Control spill-to-artifact for oversized tool results
EDGECRAB_TOOL_RESULT_SPILL=true
EDGECRAB_TOOL_RESULT_SPILL_THRESHOLD=16384
EDGECRAB_TOOL_RESULT_SPILL_PREVIEW_LINES=80
```

Gateway-specific and terminal-specific variables follow the same `EDGECRAB_` prefix convention; see the gateway and security docs for the full list.

---

## Home Directory Layout

```
~/.edgecrab/              ← $EDGECRAB_HOME (default)
├── config.yaml           ← main config (Tier 2)
├── auth.json             ← structured provider-auth metadata and active provider
├── .env                  ← provider API keys and other local secrets
├── models.yaml           ← model catalog with cost metadata
├── SOUL.md               ← persistent personality / system-prompt addendum
├── state.db              ← SQLite session store (schema v6)
├── memories/             ← file-backed memory Markdown files
├── skills/               ← SKILL.md skill definitions
├── hooks/                ← script hook directories
│   └── my-hook/
│       ├── HOOK.yaml
│       └── handler.py
└── profiles/             ← named profile directories
    ├── work/
    │   ├── config.yaml
    │   ├── .env
    │   ├── SOUL.md
    │   └── state.db
    └── personal/
        └── …
```

> **Tip**: `EDGECRAB_HOME` is the single environment variable to move the entire home to a different path — useful for containers (`EDGECRAB_HOME=/data/.edgecrab`).

---

## Profiles

Each profile is an isolated runtime context. Profile switching changes the effective home directory for all subsequent commands.

```
~/.edgecrab/profiles/<name>/
├── config.yaml     ← profile-specific overrides
├── auth.json       ← profile-scoped provider auth metadata
├── .env            ← profile-specific secrets (loaded before env vars)
├── SOUL.md         ← profile-specific personality
├── memories/       ← profile-specific durable memory
├── skills/         ← profile-specific skills
├── plugins/        ← profile-specific plugins
├── hooks/          ← profile-specific hooks
└── state.db        ← profile-specific session store
```

EdgeCrab seeds bundled starter profiles on normal startup and profile commands:
`work`, `research`, and `homelab`. These are created once under
`~/.edgecrab/profiles/` and never overwrite existing user-edited profiles.

**What profiles share**: the `edgecrab` binary, the global sticky-profile marker
`~/.edgecrab/.active_profile`, and repo-local context files such as `AGENTS.md`.

**What profiles isolate**: conversation history, secrets, model selection,
memory, skills, plugins, hooks, MCP tokens, and personality.

```bash
# switch to the "work" profile for this session
edgecrab --profile work

# run a one-shot command under the "personal" profile
edgecrab --profile personal "summarise my notes"
```

---

## Minimal `config.yaml` Example

```yaml
model:
  default: "anthropic/claude-sonnet-4-20250514"
  temperature: 0.3
  smart_routing:
    enabled: true
    cheap_model: "anthropic/claude-haiku-4-5-20251001"

agent:
  max_iterations: 30

security:
  approval_mode: "on_risk"   # never | on_risk | always

compression:
  trigger_ratio: 0.80        # compress when context is 80% full
  target_ratio: 0.40         # shrink down to 40% of window

tools:
  result_spill: true
  result_spill_threshold: 16384
  result_spill_preview_lines: 80

memory:
  enabled: true
  max_inject_tokens: 4000

moa:
  enabled: true
  aggregator_model: "anthropic/claude-opus-4.6"
  reference_models:
    - "anthropic/claude-opus-4.6"
    - "openai/gpt-4.1"
```

---

## Tips

- **Don't store secrets in `config.yaml`** — use a profile `.env` file or real environment variables; secrets are redacted from logs via `edgecrab-security/src/redact.rs` but only if they contain a known pattern.
- **Provider auth now has two local layers** — `auth.json` tracks the active provider and metadata, while `.env` still carries the actual provider API key material used at runtime.
- **The `SOUL.md` file is the fastest way to give EdgeCrab a persistent personality** without modifying code. It is appended to the system prompt on every turn.
- **`models.yaml` controls cost tracking** — if you add a new model, add a cost entry so `/cost` and trajectory files report accurately.
- **Spilled tool artifacts are workspace-local, not home-local** — large successful tool results are written under `.edgecrab-artifacts/<session_id>/` in the active cwd so the agent can read them back through normal file tools.

---

## FAQ

**Q: Can I have per-project configs?**
A: Yes. Place a `config.yaml` in a project directory and launch EdgeCrab with `EDGECRAB_HOME=$(pwd)` — Tier 2 will pick it up.

**Q: Which config value wins when `EDGECRAB_MODEL` is set AND `config.yaml` has a model AND `--model` is passed on the CLI?**
A: The CLI flag (`--model`) wins. Tier 4 > Tier 3 > Tier 2.

**Q: Does changing `config.yaml` require restarting EdgeCrab?**
A: For CLI sessions, yes. For gateway long-running processes, a graceful restart is the safe path.

---

## Cross-References

- Memory injection details → [`007_memory_skills/001_memory_skills.md`](../007_memory_skills/001_memory_skills.md)
- Security gate settings → [`011_security/001_security.md`](../011_security/001_security.md)
- Session storage (`state.db`) → [`009_config_state/002_session_storage.md`](002_session_storage.md)
- Model routing config → [`003_agent_core/005_smart_model_routing.md`](../003_agent_core/005_smart_model_routing.md)
- Hooks discovery path → [`hooks.md`](../hooks.md)
