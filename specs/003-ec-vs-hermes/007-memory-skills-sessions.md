# 007 — Memory, Skills & Sessions

Persistent knowledge, procedural skills, conversation history.

**Status (2026-06-13):** Skills production-complete (hub + guard TUI wired). Memory write approval shipped. Remaining: LLM curator (021), external memory plugins.

---

## Built-in memory (files)

| Asset | Hermes | EdgeCrab |
|-------|--------|----------|
| MEMORY.md | `~/.hermes/memories/` | `~/.edgecrab/memories/` |
| USER.md | Yes | Yes |
| Tool API | `memory` (unified) | `memory_read`, `memory_write` |
| **Write approval gate** | **`/memory pending\|approve\|reject`** | **Yes** — `/memory pending\|approve\|reject\|approval` (CLI + gateway) |
| Prompt injection in context files | `sanitize_context` | `injection.rs` block |

**Verdict:** **Hermes leads governance** (staging approvals); **parity on storage model**.

---

## External memory providers

Hermes `plugins/memory/` — **8 pluggable backends** (single-select via `memory.provider`):

| Provider | Hermes | EdgeCrab |
|----------|--------|----------|
| Honcho | Plugin + 5–6 tools | 6 built-in tools |
| OpenViking | Plugin | No |
| Mem0 | Plugin | No |
| Hindsight | Plugin | No |
| Holographic | Plugin | No |
| RetainDB | Plugin | No |
| ByteRover | Plugin | No |
| Supermemory | Plugin | No |

**Verdict:** **Hermes leads choice**; EdgeCrab **bakes in Honcho** without plugin swap.

---

## Skills system

| Feature | Hermes | EdgeCrab |
|---------|--------|----------|
| Bundled skills | `skills/` + `optional-skills/` | Bundled + `skills_sync.rs` |
| User skills dir | `~/.hermes/skills/` | `~/.edgecrab/skills/` |
| Agent-created skills | `skill_manage` | `skill_manage` |
| Dynamic `/<skill>` slash | Yes | **Yes** — single dispatch path |
| Skills Hub remote install | GitHub, ClawHub, LobeHub, Claude Marketplace, agentskills.io, **custom taps** | **Yes** — GitHub + **ClawHub + LobeHub + Claude Marketplace + browse.sh + agentskills.io federation** + skills.sh install + **`/skills tap add`** |
| Security scanner | `skills_guard.py` (23+ patterns) | `skills_guard.rs` + **`.skillignore` / `.clawhubignore`** |
| **Dangerous skill approval** | **`--force` blocked on dangerous** | **Exceeds:** `/skills trust` + hash-bound store + `--trust` + **`/skills review`** TUI (CLI intercepts `review`, `trust`, `inspect --scan`) + gateway text `/skills inspect --scan` |
| Trust tiers | builtin/trusted/community | Guard severity scoring |
| Skill bundles | `skill-bundles/*.yaml` | **Yes** + `/bundles create\|delete` |
| **Curator** (staleness/review) | **`hermes curator`, `/curator`** (LLM + archive) | **Yes** — stale + **archive/restore/prune --dry-run** (deterministic; no LLM merge) |
| Write approval gate | Yes | **Yes** — persists via `/skills approval on\|off` |
| `/reload-skills`, `/bundles` | Yes | **Yes** (CLI + gateway) |
| **Hub audit / lock** | **`/skills audit`, lock file** | **Yes** — `/skills audit`, `/skills audit --deep`, `/skills audit log`, `/skills lock` |
| **Hub update check** | **`/skills check`** | **Yes** — dry-run upstream hash compare before `/skills update` |
| **Bundled skill reset** | **`/skills reset <name> [--restore]`** | **Yes** — clears stuck `user_modified` manifest; `--restore` re-copies bundled |
| **Bundled opt-out/in** | **`/skills opt-out [--remove]`, `opt-in [--sync]`** | **Yes** — `.no-bundled-skills` marker + pristine removal |
| **Hub snapshot** | **`/skills snapshot export\|import`** | **Yes** — versioned JSON + Hermes import + content hashes |
| Usage telemetry | `.usage.json` (curator-only UX) | **Yes** + **`/skills usage`** (visible now) |
| Platform-disabled skills | Yes | **Yes** — wired in slash scan |
| OS platform filter (`platforms: [macos]`) | Yes | **Yes** — shared `filters.rs` (macos↔darwin alias) |
| Environment filter (`environments: [kanban]`) | Yes | **Yes** — offer-time only; explicit load bypasses |
| Slash invocation extras | Setup notes + supporting files w/ absolute paths | **Yes** — env + **`required_credential_files`** in `invocation_extras.rs` / `skill_view` |
| Skill preprocessing | `${HERMES_SKILL_DIR}`, inline shell, config inject | **Yes** — `${HERMES,EDGECRAB,CLAUDE}_*` + config block; **`!`cmd``** opt-in via `/skills inline-shell on` (command-scanned) |
| Pin skills | `.usage.json` pinned flag | **Yes** — `/skills pin\|unpin`, curator skips pinned |
| Skill config | `hermes config migrate` | **`/skills config migrate`** + set |
| Curator backups | Pre-run tar.gz + rollback | **`/curator backups`** + **`/curator rollback <id>`** (+ **cron skill-links** restore) |
| Bulk approve/reject | `/skills approve all` | **Yes** — `/skills approve\|reject all` |
| Protected built-ins | `plan` never archived | **`plan`** (+ extensible list in `protected.rs`) |
| Scheduled auto-prune | Gateway tick + LLM review | **Gateway tick** (opt-in `curator.enabled`) — deterministic prune |
| `prune_builtins` | Default on | **Opt-in** (`curator.prune_builtins`, default off) |
| Self-improvement nudges | After complex tasks | Skills guidance in prompt |

> **Status:** Skills production-complete for deterministic ops + **multi-registry hub**. Optional: LLM merge (021), memory approval.

**Implementation:** `crates/edgecrab-tools/src/skills/` — discovery, **filters**, bundles, invocation, write_approval, usage, curator, preprocess, archive, config_settings, scheduler, **backup**, **protected**, **credential_files**.

**First-principles wins over Hermes:**
- Single Rust module (no Python split across cli/gateway/tools)
- `/skills usage` + `/curator stale` **without** background LLM agent
- `/curator prune --dry-run` — preview archival with zero mutations (explicit opt-in to apply)
- Scheduled curator **opt-in** (`curator.enabled: false` default) vs Hermes auto-on
- Persisted write-approval toggle from slash command
- Never auto-deletes — archive to `.archive/` only; hub skills always protected
- Inline shell **opt-in** (`skills.inline_shell: false` default) with `CommandScanner` gate — Hermes runs by default when enabled in config
- `${EDGECRAB_*}` aliases alongside Hermes/Claude tokens
- Activity telemetry: use + view + patch (Hermes-parity signals)
- Pre-run **tar.gz backups** before mutating prune (rollback without LLM; **cron `skills` links** reconciled on rollback)
- `/skills approve all` / `/skills reject all` (Hermes parity)
- Shared **offer-time filters** (`platforms`, `environments`, `user-invocable`) — one module for slash + skills_list (fixes macos/darwin alias bug)
- Slash messages include **setup notes** + **absolute-path supporting file hints** (Hermes `_build_skill_message` parity; credential file paths under `~/.edgecrab/`)
- `/reload-skills` invalidates bundle + prompt caches immediately (no 60s stale window)
- **Hub exceeds Hermes:** parallel multi-registry search (ClawHub + **LobeHub** + **Claude Marketplace** + browse.sh + agentskills.io federation + skills.sh install path + **custom GitHub taps**), **unified local index** (instant search + self-improving merge + bootstrap from local hub + marketplace + **bundled repo trees** caches), real `inspect`, URL install, ClawHub community-trust warnings, path-safe ZIP extraction, **shared `hub_slash.rs` dispatch** (CLI + gateway DRY), **auto cache invalidation** after hub install/update/remove (tool + slash + `on_skills_changed`), **versioned snapshot export** with content hashes + Hermes snapshot import compatibility, **hash-bound dangerous-skill trust** (`/skills trust` + `--trust`) with audit trail + **TUI guard overlay with inline file inspector** (Findings/Files tabs, line highlights, f-to-jump, proactive scan-on-select in remote browser) — Hermes blocks dangerous entirely; agent tool **`skills_hub scan`** for guard + file listing

**Verdict:** **Skills parity exceeded on deterministic ops and hub UX.** Hermes still leads on **LLM curator merge/review** only; **Claude Marketplace parity closed**. Hub catalog latency: EdgeCrab unified index + local-cache bootstrap exceeds Hermes when remote index CDN is down. Gateway `/skills` shares hub slash handler with CLI.

---

## Sessions & search

| Feature | Hermes | EdgeCrab |
|---------|--------|----------|
| Storage | SQLite WAL | SQLite WAL |
| FTS5 search | Yes | Yes |
| Tool | `session_search` | `session_search` |
| Session lineage | `parent_session_id` | Fork/branch support |
| Source tagging | cli, telegram, … | `Platform` enum |
| Branch | `/branch` | `/branch` |
| Background session | `/background` | `/background` |
| Handoff preserves ID | `/handoff` | Handoff module |
| Profiles | `hermes -p name` | `edgecrab profile` (`profile.rs`) |
| Schema version | Migrated in config | v10 (`session_db.rs`) |
| Multi-process DB contention | Yes | Jitter-retry writes |

**Verdict:** **Parity (A)** — both production-grade session stores.

---

## Personalities / profiles

| | Hermes | EdgeCrab |
|---|--------|----------|
| `/personality` | Bundled presets | `bundled_profiles.rs` |
| Named profiles (isolated home) | `hermes profile` | `edgecrab profile` |
| Per-profile secrets/SOUL/skills | Yes | Yes |

**Verdict:** **Parity** — earlier docs claiming EC lacks profiles were **wrong** (`profile.rs` mirrors Hermes).

---

## Grades

| Dimension | Hermes | EdgeCrab |
|-----------|--------|----------|
| File memory | A | A |
| External memory plugins | A | C (Honcho only) |
| Skills library | A | **A** (full deterministic curator; LLM merge optional 021) |
| Skills hygiene | A (curator) | **A** (guard + approval + usage + archive + pin) |
| Skills architecture | B+ (Python sprawl) | **A (unified module)** |
| Session FTS | A | A |
| Profiles | A | A |

Cross-ref: [001-gap-analysis 021/028](../001-gap-analysis-v14/999-roadmap.md)
