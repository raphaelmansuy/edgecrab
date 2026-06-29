# 002 — Architecture & Runtime

Brutal comparison of how each agent is **built**, **shipped**, and **run**.

---

## Stack at a glance

| Dimension | Hermes | EdgeCrab |
|-----------|--------|----------|
| Language | Python 3.11+ | Rust (edition 2024, MSRV 1.95) |
| Package manager | uv / pip | cargo |
| LLM abstraction | In-tree + provider plugins | `edgequake-llm` crate |
| UI (modern) | React/Ink (`ui-tui/`) + WS gateway | ratatui + crossterm |
| UI (classic) | prompt_toolkit (`cli.py`) | Same TUI (no separate classic) |
| Agent ↔ UI coupling | **Out-of-process** JSON-RPC (`tui_gateway/`) | **In-process** `StreamEvent` channel |
| Desktop | Electron app | None |
| Default binary | `hermes` (Python entry) | `edgecrab` (static release) |

---

## Process model

### Hermes: multi-process, UI-survivable

```text
┌─────────────┐     JSON-RPC      ┌──────────────────┐
│  ui-tui     │ ◄──────────────► │ tui_gateway      │
│  (Node/React)│                  │ (Python server)  │
└─────────────┘                   └────────┬─────────┘
                                           │
                                           ▼
                                  ┌──────────────────┐
                                  │ AIAgent          │
                                  │ (run_agent.py)   │
                                  └──────────────────┘
```

**Pros**
- UI crash ≠ agent death (`gatewayRecovery.ts`)
- Same gateway serves TUI, desktop, web dashboard PTY
- React component testability (71 test files under `ui-tui/src/__tests__/`)

**Cons**
- Startup latency + Node memory footprint
- Documented OOM from verbose tool trails → Ink render explosion (`ui-tui/src/config/limits.ts`)
- RPC boundary complexity (`turnController.ts`, `STREAM_BATCH_MS`)

### EdgeCrab: single binary, direct dispatch

```text
┌─────────────────────────────────────────────┐
│ edgecrab-cli (one process)                  │
│  app.rs ──► StreamEvent ──► ratatui frame   │
│       └──► Agent::run_conversation (core)   │
└─────────────────────────────────────────────┘
```

**Pros**
- No JSON-RPC hop; lower latency
- Rust avoids Node-style render-tree OOM
- Simpler deploy (one artifact)

**Cons**
- **`app.rs` monolith ~34k lines** (June 2026; partial extraction to `app/` submodules)
- UI regression risk — hard to unit-test full render stack
- No "restart UI only" without killing agent

**Verdict:** **Different tradeoffs (≠)**. Hermes optimizes **surface area + resilience**. EdgeCrab optimizes **simplicity + resource use**.

---

## Module / crate map

### Hermes (Python monolith + packages)

| Area | Path | Lines (order of magnitude) |
|------|------|---------------------------|
| Agent core | `run_agent.py`, `agent/` | Central |
| Tools | `tools/*.py`, `tools/environments/` | 40+ modules |
| CLI | `hermes_cli/main.py`, `commands.py` | Large |
| Gateway | `gateway/run.py`, `gateway/platforms/` | 20+ adapters |
| Plugins | `plugins/**` | 100+ dirs |
| TUI | `ui-tui/`, `tui_gateway/server.py` | 10k+ in gateway alone |
| State | `hermes_state.py` | SQLite FTS5 |

### EdgeCrab (Rust workspace — 20 crates)

Dependency waist (from `AGENTS.md`):

```text
edgecrab-types → security → tools → state → core → {cli, gateway, acp, migrate, proxy}
```

| Crate | Responsibility |
|-------|----------------|
| `edgecrab-core` | Agent, loop, compression, goals, steering, routing |
| `edgecrab-tools` | 80+ tools, registry, toolsets |
| `edgecrab-gateway` | 17 platform adapters, delivery, pairing |
| `edgecrab-cli` | TUI + subcommands (largest crate) |
| `edgecrab-state` | SQLite WAL + FTS5 |
| `edgecrab-proxy` | OpenAI-compat subscription bridge |
| `edgecrab-acp` | VS Code JSON-RPC stdio |
| `edgecrab-lsp` | 25 LSP tools |
| `edgecrab-plugins` | Plugin discovery (subprocess JSON-RPC ADR) |
| `edgecrab-migrate` | Hermes + OpenClaw import |
| `edgecrab-sdk-core` + Python/Node SDKs | Programmatic API |

**Verdict:** **EdgeCrab leads internal boundaries** (explicit crates, `#![deny(clippy::unwrap_used)]` in prod). **Hermes leads horizontal extensibility** (plugins without recompile).

---

## Configuration & state layout

| Asset | Hermes | EdgeCrab |
|-------|--------|----------|
| Main config | `~/.hermes/config.yaml` | `~/.edgecrab/config.yaml` |
| Gateway config | `~/.hermes/gateway-config.yaml` | `gateway.*` section in main config |
| Secrets | `~/.hermes/.env`, `auth.json` | `~/.edgecrab/.env`, `auth.json` |
| Sessions DB | `hermes_state.py` → SQLite | `edgecrab-state` → `sessions.db` |
| Memories | `~/.hermes/memories/` | `~/.edgecrab/memories/` |
| Skills | `~/.hermes/skills/` + bundled | `~/.edgecrab/skills/` + sync |
| Checkpoints | `~/.hermes/checkpoints/` (shadow git) | `~/.edgecrab/checkpoints/` |
| Profiles | `~/.hermes/profiles/<name>/` | `~/.edgecrab/profiles/<name>/` (`profile.rs`) |
| Skin | `~/.hermes/skin.yaml` | `~/.edgecrab/skin.yaml` |

**Verdict:** **Parity** on layout — EdgeCrab deliberately mirrors Hermes for migration (`edgecrab-migrate`).

---

## Deployment profiles

| Profile | Hermes | EdgeCrab |
|---------|--------|----------|
| Developer laptop | `hermes` / `hermes --tui` | `edgecrab` / `cargo run` |
| Always-on gateway | `hermes gateway` | `edgecrab gateway` |
| IDE agent | `hermes acp` | `edgecrab acp` |
| Android/Termux | Supported | `termux` feature flag + compact UI |
| Docker | Documented | Less first-class |
| Windows native | First-class + WSL docs | Rust cross-compile possible; Hermes more mature |

**Verdict:** **Hermes leads platform docs & Windows story**. **EdgeCrab leads binary deploy & Termux**.

---

## Hot-swap & runtime mutation

| Capability | Hermes | EdgeCrab |
|------------|--------|----------|
| Model switch mid-session | `/model` | `/model` + instant hot-swap |
| Provider switch | `/model --provider` | `/provider`, `/model` |
| Tool enable/disable | `/tools disable` | `/tools`, toolset policy |
| MCP reload | `/reload-mcp` | `/reload-mcp` |
| Config reload | Partial (`/reload`) | Partial |

EdgeCrab `Agent` uses `Arc<RwLock<>>` for hot-swap without process restart.

**Verdict:** **Parity** with EdgeCrab slight edge on **model hot-swap UX** (spec 002-tui parity work).

---

## Technical debt (honest)

| Debt | Hermes | EdgeCrab |
|------|--------|----------|
| Largest file | `tui_gateway/server.py` (~10k) | `edgecrab-cli/src/app.rs` (~34k) |
| Plugin security | skills_guard + scanner | skills_guard + plugin guard |
| i18n | zh-Hans docs exist | Not started (gap 025) |
| Provider plugin API | Mature | Compile-time catalog (gap 009) |

**Grades**

| Dimension | Hermes | EdgeCrab |
|-----------|--------|----------|
| Modular boundaries | B | A− |
| Deploy simplicity | C (Python+Node) | A |
| UI architecture | A− (multi-surface) | B (monolith debt) |
| Extension without fork | A | C+ |

See also: [specs/002-tui-hemes-vs-edgecrab/001-architecture-and-stack.md](../002-tui-hemes-vs-edgecrab/001-architecture-and-stack.md)
