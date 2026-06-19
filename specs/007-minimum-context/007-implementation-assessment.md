# 007 — Post-Implementation Assessment vs Hermes (Brutal)

**Date:** 2026-06-15 (re-assessed)  
**Branch:** `feat/minimum-context-007` (unmerged)  
**Scope:** Sprint S1–S6 + cache/skills/context-file polish

---

## First-principles design choices

| Principle | Choice | Why |
|-----------|--------|-----|
| **Agent must act** | `core` alias includes `terminal` + `file` | Shell execution is non-negotiable; a schema without `terminal` is a chatbot, not an agent |
| **Agent must perceive** | `core` includes `web` (search + extract, not crawl) | Research is default; `web_crawl` demoted to `research` toolset (heavy schema + rare turn-1 need) |
| **Agent must remember** | `core` includes `memory` (read/write only) | Honcho split to opt-in `honcho` toolset — 6 schemas saved from default |
| **Schema is physics** | CI gates: `core` < 18K, `minimal` < 8K, stable < 2.2K | If CI doesn't measure it, the list grows forever |
| **Cache ≠ minimum** | Stable / semi-stable / dynamic split + skills index | Guidance 1h; skills names 5m; descriptions dynamic |
| **Default is product** | `enabled_toolsets: ["core"]` not `None` | **Leapfrogs Hermes** — Hermes still ships `enabled_toolsets=None` |
| **Lists must lie honestly** | `CORE_TOOLS` = default core surface (56 names), not every registered tool | Runtime law is `resolve_alias("core")`, not a stale superset const |

---

## What shipped (code is law)

| ID | Change | Law |
|----|--------|-----|
| L0.1 | `ToolsConfig::default()` → `enabled_toolsets: Some(["core"])` | `config.rs` |
| L0.2 | Subagents default `minimal` when no toolsets requested | `delegate_task.rs` |
| L1.1 | Honcho → `honcho` toolset; `web_crawl` + `pdf_to_markdown` → `research` | `honcho.rs`, `extract_crawl.rs`, `pdf_to_markdown.rs` |
| L1.1b | `core` alias: `web`, `terminal`, `memory`, `skills` | `toolsets.rs` |
| L1.1c | `CORE_TOOLS` trimmed to 56 names; `HONCHO_TOOLS`, `RESEARCH_EXTRA_TOOLS` | `toolsets.rs` |
| L1.2 | `tools.schema_mode: compact\|full\|indexed` — default **indexed** | `schema_mode.rs`, `tool_schema_index.rs`, `tool_search.rs` |
| L1.2b | `research` alias expands literal `research` toolset | `toolsets.rs` |
| L1.3 | ACP: `acp_tools()` derived from `CORE_TOOLS − exclusions` (no duplicate const) | `toolsets.rs` |
| L2.3 | Trimmed MEMORY, VISION, LSP guidance; merged TASK_STATUS + PROGRESSION | `prompt_builder.rs` |
| L2.1 | `TASK_COMPLETION_GUIDANCE` + trimmed scheduling block | `prompt_builder.rs` |
| L0.3 | `enabled_toolsets: null` → `["core"]` at load; empty list guard | `normalize_tools_policy_keys`, `sanitize_tools_policy` |
| L2.2 | SCHEDULING + SKILLS + MOA guidance collapsed | `prompt_builder.rs` |
| L3.1 | Skills: name index in semi-stable (5m), descriptions in dynamic | `SkillPromptParts`, `load_skill_prompt_parts()` |
| L4.5 | Three-block cache wire: stable (1h) + semi-stable skills (5m) + dynamic | `cached_semi_stable_prompt`, `build_chat_messages_blocks()` |
| L3.2 | Context-files mtime cache | `discover_context_files_cached()`, `file_manifest.rs` |
| L3.3 | `tool_search` + deferred index + dispatch wire gate | `tool_schema_index.rs`, `tools/tool_search.rs`, `dispatch_single_tool` |
| L4.1 | CI schema + stable guidance budgets | `context_budget.rs` |
| L4.2 | `/context budget` slash command | `commands.rs`, `app.rs` |
| L4.3b | OpenRouter `cache_control` on content blocks | `edgequake-llm` `openai_wire.rs` v0.6.26 |
| L4.4 | Local KV normalization | `local_provider_policy.rs` |
| DRY | `file_manifest.rs` + `evict_oldest_cache_entry` shared by skills + context caches | `file_manifest.rs`, `prompt_builder.rs` |

---

## Verdict table (honest, post-polish)

| Dimension | Hermes | EdgeCrab (branch) | Winner |
|-----------|--------|-------------------|--------|
| **Default tool surface** | `None` = all (~35 tools) | `core` default + indexed wire <8K | **EdgeCrab** |
| **Turn-1 minimum (M1)** | ~15K tools + ~1.2K guidance | ~4–6K indexed wire + ~1.2–1.8K stable guidance | **EdgeCrab** |
| **Stable/dynamic architecture** | Single cached system string | Three-tier: stable + semi-stable (skills) + dynamic | **EdgeCrab** |
| **Cache provider breadth** | Mature `anthropic_prompt_cache_policy()` | `resolve_prompt_cache()` + layout wire | **≈ parity** |
| **OpenRouter cache wire** | Envelope inner markers in Python | `openai_wire` propagates `cache_control` on content blocks (v0.6.26) | **≈ parity** |
| **Local KV reuse** | Full message normalize pass | Tool-arg canonicalize + trim on local only | **Hermes slightly** |
| **ACP editor bloat** | N/A | LSP opt-in via `lsp` toolset | **EdgeCrab fixed** |
| **CORE_TOOLS honesty** | `_HERMES_CORE_TOOLS` ~49 names | `CORE_TOOLS` 56 names aligned with default core | **≈ parity** |
| **Lazy schema / tool search** | `model_tools.py` tiers | **indexed default** + `tool_search` + CI <8K wire | **EdgeCrab** |
| **Skills index** | Full index in stable string | Names in semi-stable (5m cache); descriptions dynamic | **EdgeCrab** (skill install no longer busts 1h stable) |
| **Cold-start I/O** | Context file cache | mtime cache for AGENTS.md walk | **≈ parity** |
| **Observability** | TUI debug paths | `/context budget` + doctor warn | **EdgeCrab** |

---

## Brutal truths

### EdgeCrab wins (real)

1. **Default `core` leapfrogs Hermes** — Hermes still ships `enabled_toolsets=None`.
2. **CI enforces schema + stable guidance** — Hermes relies on manual measurement.
3. **Three-tier cache architecture** — Stable guidance (1h) survives skill installs; skills index gets its own 5m breakpoint.
4. **`CORE_TOOLS` no longer lies** — Was 64 names including opt-in honcho/crawl; now 56 matching policy.
5. **Context-files cache** — Monorepo AGENTS.md walks no longer hit disk every turn.
6. **`ACP_TOOLS` const eliminated** — Single derivation: `CORE_TOOLS − ACP_EXCLUDED`; `is_acp_tool()` for membership checks.
7. **Legacy config migration** — `null`/empty `enabled_toolsets` → `core` at load; no silent full-tool surface.

### EdgeCrab still loses (don't spin)

1. **Stable guidance ~1.2–1.8K tok** — near Hermes ~1.2K but still slightly heavier on local-model enforcement blocks.
2. **OpenRouter cache unverified in production** — wire fix shipped in edgequake-llm 0.6.26; needs telemetry proof on real OpenRouter Claude sessions.
3. **Branch unmerged** — PR #47 required for `main`.

### Tie / workload-dependent

| Workload | Better default |
|----------|----------------|
| CLI coding, no LSP | **EdgeCrab** |
| VS Code ACP + LSP | **EdgeCrab** (`core` + `lsp` explicit) |
| OpenRouter Claude daily driver | **≈ tie** (cache_control wire fixed in 0.6.26) |
| Ollama/LM Studio long sessions | **≈ tie** |
| Research-heavy + write_file on local Qwen | **EdgeCrab** (more enforcement guidance — costs tokens, buys completion) |

---

## Measured anchors (branch)

| Profile | CI assertion | Notes |
|---------|--------------|-------|
| `core` schema | < 18,000 tok | `context_budget::default_core_profile` |
| `minimal` schema | < 8,000 tok | `context_budget::minimal_profile` |
| `core` stable guidance | < 2,200 tok | `default_core_stable_guidance` |
| `core` indexed schema | < 8,000 tok | `indexed_core_profile` |
| `core` indexed vs compact | indexed ≥20% smaller | `indexed_core_smaller_than_compact` |
| Dispatch wire gate | deferred blocked until `tool_search` | `dispatch_single_tool_blocks_deferred_until_materialized` |
| `/context budget` indexed | shows `N on wire, M deferred` | `format_report_shows_deferred_when_indexed` |
| `CORE_TOOLS` names | ≤ 57 | `core_tools_count_is_honest` |

Run locally: `/context budget` after first turn, or `cargo test -p edgecrab-core context_budget`.

---

## What's next (highest ROI)

1. **edgequake-llm 0.6.27 publish** — repin edgecrab `Cargo.toml` from path to crates.io after release.
2. **Production cache telemetry** — surface `cache_read_input_tokens` in doctor or `/context budget`.
3. **Merge PR #47** — Ship branch to `main`.

**Closed this pass:** dispatch wire gate test; `/context budget` wire vs deferred counts; stable `INDEXED_TOOL_GUIDANCE` when `schema_mode: indexed`.

---

## Cross-refs

- [005-leverage-plan.md](005-leverage-plan.md) — full backlog
- [006-cache-preservation.md](006-cache-preservation.md) — cache architecture
- [004-comparison-matrix.md](004-comparison-matrix.md) — pre-implementation numbers
