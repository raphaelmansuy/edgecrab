# 012 — Master Gap Matrix

Single cross-reference table. Status from code inspection + [001-gap-analysis-v14/999-roadmap.md](../001-gap-analysis-v14/999-roadmap.md).

**Legend:** ✅ parity/shipped · 🟡 partial · ❌ missing · ≠ different design

---

## Core harness

| Feature | Hermes | EdgeCrab | Status | Leader |
|---------|--------|----------|--------|--------|
| ReAct loop | ✅ | ✅ | Parity | — |
| Streaming | ✅ | ✅ | Parity | — |
| Interrupt | ✅ | ✅ | Parity | — |
| Compression LLM+structural | ✅ | ✅ | Parity | — |
| Prompt prefix cache | ✅ | ✅ | Parity | — |
| Persistent goals (Ralph) | ✅ | ✅ | Parity | — |
| `/done` subgoal marker | ❌ | ✅ | EC only | EC |
| Mission steering (typed) | 🟡 `/steer` | ✅ HINT/REDIRECT/STOP | EC ahead | EC |
| Shadow judge | ❌ | ✅ | EC only | EC |
| File mutation footer | ✅ | ✅ | Parity | — |
| Kanban multi-agent | ✅ 9 tools + dispatcher + React dashboard + decomposer + profile routing + PATCH + drag-drop + parent blockers + respawn guard + scheduled_at + build_worker_context | 🟡 11 tools + decomposer + profile routing + static UI + PATCH + drag-drop + parent blockers (409) + respawn guard + scheduled_at + build_worker_context | Partial | Hermes (React SPA depth) |
| Delegate + `/agents` | ✅ | ✅ | Parity | EC (TUI control) |
| Checkpoints v2 | ✅ | ✅ | Parity | — |
| Config `/snapshot` | ✅ | ✅ | Parity | — |
| Codex app-server runtime | ✅ | ❌ | Hermes | Hermes |

---

## Tools

| Feature | Hermes | EdgeCrab | Status | Leader |
|---------|--------|----------|--------|--------|
| File/terminal/web/browser core | ✅ | ✅ | Parity | — |
| web_crawl | ❌ | ✅ | EC only | EC |
| LSP 25 tools + write gate | 🟡 | ✅ | EC ahead | EC |
| video_analyze/generate | ✅ | ❌ gap 012 | Hermes | Hermes |
| x_search | ✅ | ❌ gap 027 | Hermes | Hermes |
| computer_use | ✅ | ✅ | Parity | — |
| mixture_of_agents | ✅ | ✅ opt-in | Parity | — |
| Spotify (7 tools) | ✅ plugin | ❌ | Hermes | Hermes |
| Feishu doc/drive tools | ✅ | ❌ | Hermes | Hermes |
| Discord admin tools | ✅ | ❌ | Hermes | Hermes |
| Blueprints | ✅ | ❌ | Hermes | Hermes |
| Web search backend chain | plugins | ✅ compiled | ≠ | EC integrated / H extensible |
| Terminal 6 backends | ✅ | ✅ | Parity | — |

---

## Gateway

| Feature | Hermes | EdgeCrab | Status | Leader |
|---------|--------|----------|--------|--------|
| Telegram/Discord/Slack/… core | ✅ | ✅ | Parity | — |
| Teams/Google Chat/LINE | ✅ plugin | ❌ | Hermes | Hermes |
| ntfy/SimpleX/IRC/Photon | ✅ plugin | ❌ | Hermes | Hermes |
| Yuanbao/QQ | ✅ | ❌ | Hermes | Hermes |
| Session handoff | ✅ `/handoff` | ✅ | Parity | — |
| Stream message edit | ✅ | ✅ | Parity | — |
| MEDIA:// delivery | ✅ | ✅ | Parity | — |
| DM pairing | ✅ | ✅ | Parity | — |
| Platform circuit breaker | ✅ | 🟡 | Hermes | Hermes |
| Discord history backfill | ✅ | ❌ gap 016 | Hermes | Hermes |
| Native clarify buttons | 🟡 | 🟡 Telegram + Discord + WhatsApp (buttons ≤3, list 4–10) | EC ahead (3 platforms) |

---

## Models & auth

| Feature | Hermes | EdgeCrab | Status | Leader |
|---------|--------|----------|--------|--------|
| Provider count | 30+ | 19 | Hermes | Hermes |
| OAuth subscriptions (big 4) | ✅ | ✅ | Parity | — |
| Qwen/Gemini CLI OAuth | ✅ | ❌ | Hermes | Hermes |
| Credential pools | ✅ | 🟡 | Hermes | Hermes |
| Fallback chain | ✅ | ✅ | Parity | — |
| `/fast` priority tier | ✅ | 🟡 | Hermes | Hermes |
| OpenRouter Pareto router | ✅ | ❌ gap 029 | Hermes | Hermes |
| OpenAI-compat proxy | ✅ | ✅ | Parity | — |
| Nous Portal tool gateway | ✅ | ≠ proxy | Hermes | Hermes |

---

## Memory & skills

| Feature | Hermes | EdgeCrab | Status | Leader |
|---------|--------|----------|--------|--------|
| MEMORY.md / USER.md | ✅ | ✅ | Parity | — |
| Memory write approval | ✅ | ✅ | Parity | — |
| 8 memory provider plugins | ✅ | Honcho only | Hermes | Hermes |
| Skills hub + guard | ✅ | ✅ | Parity | — |
| Curator subsystem | ✅ | ❌ gap 021 | Hermes | Hermes |
| Skill bundles | ✅ | 🟡 | Hermes | Hermes |
| Profiles | ✅ | ✅ | Parity | — |

---

## Security

| Feature | Hermes | EdgeCrab | Status | Leader |
|---------|--------|----------|--------|--------|
| Path/SSRF/command guards | ✅ | ✅ | Parity | — |
| Smart approval (LLM) | ✅ | ✅ | Parity | — |
| Skills guard | ✅ | ✅ | Parity | — |
| OSV supply-chain audit | ✅ | 🟡 | Hermes | Hermes |
| Bitwarden secrets | ✅ | ❌ gap 032 | Hermes | Hermes |
| LSP write gate | ✅ | ✅ | Parity | — |

---

## UX & surfaces

| Feature | Hermes | EdgeCrab | Status | Leader |
|---------|--------|----------|--------|--------|
| Modern TUI | ✅ React | ✅ ratatui | ≠ | H component / EC perf |
| Desktop Electron | ✅ | ❌ | Hermes | Hermes |
| Web dashboard | ✅ | ❌ | Hermes | Hermes |
| Slash commands | 75 | 84 | ≠ | EC count / H depth |
| Spawn tree replay disk | 🟡 | ✅ | EC ahead | EC |
| i18n | ✅ zh-Hans | ❌ gap 025 | Hermes | Hermes |
| Git worktree mode | ✅ | ✅ | Parity | — |
| Voice mode | ✅ | ✅ | Parity | — |

---

## Extensibility

| Feature | Hermes | EdgeCrab | Status | Leader |
|---------|--------|----------|--------|--------|
| Python plugins | ✅ 100+ | ❌ | Hermes | Hermes |
| Provider plugins | ✅ | ❌ gap 009 | Hermes | Hermes |
| MCP client | ✅ | ✅ | Parity | — |
| Hermes as MCP server | ✅ | ❌ | Hermes | Hermes |
| ACP | ✅ | ✅ | Parity | — |
| SDK Python/Node | 🟡 | ✅ | EC ahead | EC |
| WASM plugins | ❌ | deferred | Both weak | — |

---

## Engineering

| Feature | Hermes | EdgeCrab | Status | Leader |
|---------|--------|----------|--------|--------|
| Test count | ~25k | ~650+ | Hermes | Hermes |
| Clippy/type safety | N/A | ✅ strict | EC | EC |
| `edgecrab migrate` | N/A | ✅ | EC | EC |
| Monolith hotspot | tui_gateway | app.rs | Both debt | — |

---

## Score summary (rough)

| Category | Hermes wins | EdgeCrab wins | Parity |
|----------|-------------|---------------|--------|
| Core harness | 1 | 4 | 9 |
| Tools | 8 | 4 | 6 |
| Gateway | 5 | 0 | 8 |
| Models | 6 | 0 | 4 |
| Memory/skills | 3 | 0 | 5 |
| Security | 3 | 1 | 4 |
| UX | 4 | 2 | 4 |
| Extensibility | 4 | 2 | 2 |
| Engineering | 1 | 3 | 1 |

**Interpretation:** Hermes wins **breadth** (plugins, platforms, providers, ops features). EdgeCrab wins **depth** on coding-agent integrity (LSP, shadow judge, steering, mutation footer) and **deployability** (binary, SDK, migration).

---

## Open EdgeCrab gaps (from roadmap)

Priority items still **not** at Hermes parity:

| ID | Feature | Tier |
|----|---------|------|
| 007 | Multi-agent kanban (React dashboard) | A 🟡 partial — orchestration API + profile routing + static settings UI |
| 009 | Pluggable providers/plugins | A |
| 012 | Video tools | A |
| 015 | Native clarify buttons (WhatsApp Cloud list mode) | B 🟡 partial — Baileys list 4–10 shipped |
| 016 | Discord history backfill | B |
| 021 | Curator subsystem | C |
| 025 | i18n | C |
| 027 | x-search tool | C |
| 029 | Pareto code router | C |
| 032 | Secrets manager | C |

Shipped notable gaps: 001 goals, 002 mutation verifier, 003 LSP diagnostics, 004 cache, 005 handoff, 006 checkpoints, 010 MCP OAuth, 011 computer use, 024 OAuth providers, **memory write approval**, **config `/snapshot`**, **pre-update auto-snapshot**, **smart approval**, **kanban Phase 2–6** (comments, block/unblock, deps, dispatch, task_runs, failure_limit, multi-board, gateway notifier, max_runtime, worker interrupt, `/kanban subscribe`, read API + events WS, **decomposer + triage + static UI + API auth + orchestration settings + profile routing + PATCH/DELETE tasks + describe-auto + drag-drop + parent blockers 409 + archive + per-profile cap + scheduled_at + respawn guard + rate-limit requeue + `/kanban schedule|archive|delete` + build_worker_context handoff**), **clarify buttons (Telegram + Discord + WhatsApp buttons/list)**, **kanban_create max_runtime wired**.
