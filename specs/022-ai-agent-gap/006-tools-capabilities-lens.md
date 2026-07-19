# 006 — Tools & Capabilities Lens (Re-assessed)

**Authority:** [000 §6, §11](000-code-is-law.md)  
**Date:** 2026-07-19

---

## 1. Philosophy

| | EdgeCrab | Hermes |
|-|----------|--------|
| Registration | compile-time `inventory::submit!` | runtime registry + plugins |
| Core set | `CORE_TOOLS` **56** | `TOOLSETS` composition |
| Quality bias | typed `ToolError`, spill-blind block | speed of adding tools |

---

## 2. Matrix (presence)

### Coding

| Capability | H | EC | Score |
|------------|---|----|-------|
| Files + terminal + process | ✅ | ✅ 8 process tools | = |
| Code execution | ✅ | ✅ | = |
| LSP | moderate | **26** `LSP_TOOLS` | **EC** |
| Computer use | ✅ | ✅ `tools/computer_use/*` | = |
| Checkpoints | ✅ | ✅ | = |

### Web / research

| | H | EC | Score |
|-|---|----|-------|
| search/extract | ✅ | ✅ | = |
| web_crawl | limited | ✅ research toolset | **EC** |
| Browser CDP | ✅ (+camofox) | ✅ 14 browser tools | = / H camofox |
| x_search | ✅ | ✅ opt-in | = |

### Media

Vision, TTS/STT, image gen, video — **parity** (EC video opt-in toolset).

### Memory / meta

| | H | EC | Score |
|-|---|----|-------|
| MEMORY/USER files | ✅ | ✅ | = |
| Memory providers | **8+ plugins** | Honcho toolset | **H** |
| Skills hub/guard/bundles | ✅ | ✅ | = |
| tool_search | ✅ | ✅ | = |
| MoA | ✅ | ✅ opt-in | = |
| Blueprints | ✅ | ❌ | **H** |
| Kanban tools | ✅ | ✅ (11 tools area) | = / H UI |

### Platform-specific tools

Discord admin, Feishu doc/drive, Yuanbao, Spotify — **H only** (S2–S3 demand filter).

---

## 3. Tool *quality* (not presence)

| Quality | EC (law) | H (law) |
|---------|----------|---------|
| Spill | artifact_spill 913 LOC | tool_result_storage 254 |
| Spill-blind write block | **yes** | no |
| Parallel batch | JoinSet | concurrent executor module |
| Typed errors | ToolError enum | mostly strings |
| Pre-dispatch theater blocks | yes | limited |

**Score quality-of-core: EC · long-tail presence: H.**

---

## 4. Gaps ranked

| ID | Gap | Sev | Action |
|----|-----|-----|--------|
| T-01 | Memory provider breadth | S1 | MCP bridge / 1–2 providers |
| T-02 | Cron blueprints | S2 | catalog concept |
| T-03 | Platform admin tools | S2–3 | demand-only |
| T-04 | Camofox-class browser | S2 | monitor reliability |

---

## 5. Scorecard

| Dimension | Score |
|-----------|-------|
| Core coding | = |
| LSP | **EC** |
| Spill quality | **EC** |
| Long-tail tools | **H** |
| Memory ecosystem | **H** |
| Extension speed | **H** |
