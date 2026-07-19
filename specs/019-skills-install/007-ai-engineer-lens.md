# 007 — AI Engineer Lens

**Cross-ref:** [002](./002-first-principles.md) · [004](./004-edgecrab-current-state.md) · [010](./010-solid-dry-ownership.md)

## Role of skills in the agent loop

```text
skills_list  →  skill_view (SKILL.md)  →  nested files
                     │
                     ▼
           progressive disclosure
           (token-cheap index in prompt;
            full body on demand)
```

Install is **not** a ReAct turn concern — it mutates the skills filesystem + hub lock, then invalidates discovery caches so the **next** prompt assembly / `/reload-skills` sees the skill.

## Agent tool surface (keep; harden)

| Tool | Purpose | Install relevance |
|------|---------|-------------------|
| `skills_list` / `skills_categories` | Discovery | Read-only |
| `skill_view` | Progressive load | Read-only |
| `skill_manage` | Create/edit/patch/delete | Optional write-approval + guard |
| `skills_hub` | search/install/update/… | **Must call same façade as CLI** |

### Rules for agent install

1. Agent may propose install; policy still applies (`InstallGate`).  
2. Dangerous without prior hash approval → structured error telling human to `/skills trust` or TUI Trust — do not silently `--trust`.  
3. Never inject skill body into `cached_system_prompt` from the tool handler.  
4. After successful install, call `notify_hub_skills_mutated` (already) so list tools refresh.

## Façade contract (target)

Logical API (names locked in implementation; intent fixed):

```text
search(query, opts) -> SearchReport
inspect(id, {scan}) -> InspectReport | InstallScanPreview
preview_install(id) -> InstallScanPreview
install(id, InstallGate) -> InstallOutcome
trust(id) / untrust(id) / list_trusted()
check(name?) / update(name?)
audit(name?, deep)
tap_{list,add,remove}
snapshot_{export,import}
uninstall(name)
```

Adapters:

| Adapter | Crate |
|---------|-------|
| clap | `edgecrab-cli` |
| slash | `hub_slash.rs` |
| TUI | CLI overlays → façade async |
| gateway | same slash |
| agent tool | `skills.rs` / hub tool |

## Progressive disclosure vs hub

| Layer | Content | Cache |
|-------|---------|-------|
| Prompt skills summary | Compact index | Session cache; invalidate on hub mutate |
| `skill_view` | Full SKILL.md | Per call |
| Nested assets | references/, scripts/ | Per call |
| Hub lock | Provenance | Disk |

Do not conflate **bundled sync** (`skills_sync`) with **hub install**. Sync seeds official skills; hub installs community/curated/local identifiers.

## Error model (agent-friendly)

Return structured, actionable strings (or JSON in `--json` / tool JSON):

| Code-ish | Meaning | Next action |
|----------|---------|-------------|
| `RATE_LIMITED` | GitHub API | Set `GITHUB_TOKEN` / `GH_TOKEN` |
| `AMBIGUOUS` | Short name collision | Show candidate table |
| `SCAN_CAUTION` | Needs `--force` | Preview findings |
| `SCAN_DANGEROUS` | Needs `--trust` | Human trust path |
| `SIG_FAIL` | W4 signature | Refuse install |
| `NOT_FOUND` | Identifier miss | Suggest search |
| `PATH_TRAVERSAL` | Bad bundle paths | Refuse |

## Cache / prompt interaction

```text
  hub mutate
      → invalidate discovery
      → invalidate skills summary cache
      → NOT rebuild cached_system_prompt mid-conversation

  next turn / reload_skills / new session
      → PromptBuilder picks up new summary in dynamic zone
```

Stable zone remains free of skill bodies (Anthropic prefix cache law — see AGENTS.md / 018).

## SOLID mapping for AI systems

| Principle | Application |
|-----------|-------------|
| S | `skills_hub` owns install; `skills_guard` owns scan; CLI owns render |
| O | New registry = new `SkillSource` impl, not fork of `install_skill` |
| L | All sources produce `SkillBundle` |
| I | Agent tool interface ⊆ façade; no secret install APIs |
| D | UI/tools depend on façade traits/fns, not GitHub HTTP details |

## Test strategy (engineer)

| Layer | What |
|-------|------|
| Unit | policy matrix, path traversal, hash trust invalidation |
| Hub | install local fixture skill end-to-end in TempDir + `EDGECRAB_HOME` |
| Slash/CLI | clap parse + hub_slash action table parity test |
| Agent | tool args → façade mock; dangerous blocked without trust |
| TUI | pure keymap + render snapshot tests (no network) |

## Explicit non-goals (engineer)

- MoA / multi-model skill ranking  
- Embedding-based skill search as default (BM25/index first)  
- Auto-scheduling blueprints without user confirmation
