# 004 — Tools & Toolsets

Feature-by-feature capability matrix. Counts are **built-in** unless noted.

---

## Toolset model

| | Hermes (`toolsets.py`) | EdgeCrab (`toolsets.rs`) |
|---|------------------------|--------------------------|
| Toolset count | ~28 | ~20 literals + aliases |
| Aliases | Platform presets | `core`, `coding`, `research`, `safe`, `minimal`, … |
| Platform-specific sets | Gateway presets | `CORE_TOOLS` shared CLI+gateway |
| Opt-in sets | video, moa, x_search, spotify, … | `lsp`, `moa`, `computer_use`, `mcp_extended` |

---

## Core tools matrix

| Tool / area | Hermes | EdgeCrab | Verdict |
|-------------|--------|----------|---------|
| **read_file** | Yes | Yes | Parity |
| **write_file** | Yes | Yes | Parity |
| **patch** | Yes | Yes | Parity |
| **search_files** | Yes | Yes | Parity |
| **pdf_to_markdown** | ? | Yes | EC only |
| **terminal** | Yes | Yes | Parity |
| **process** (bg) | Yes | 8 process tools | EC more granular |
| **web_search** | Yes | Yes + backend chain | EC leads failover |
| **web_extract** | Yes | Yes | Parity |
| **web_crawl** | No | Yes | EC only |
| **browser_*** (CDP) | 10+2 gated | 14 tools | Parity (EC slightly broader) |
| **vision_analyze** | Yes | Yes | Parity |
| **image_generate** | Yes | `generate_image` | Parity |
| **text_to_speech** | Yes | Yes | Parity |
| **transcribe_audio** | Yes | Yes | Parity |
| **memory** | Single `memory` tool | `memory_read` + `memory_write` | Parity |
| **session_search** | FTS5 | FTS5 | Parity |
| **todo** | `todo` | `manage_todo_list` + `report_task_status` | EC more structured |
| **clarify** | Yes | Yes | Parity |
| **execute_code** | Yes | Yes | Parity |
| **delegate_task** | Yes | Yes | Parity |
| **cronjob** | `cronjob` | `manage_cron_jobs` | Parity |
| **send_message** | Gateway | Gateway | Parity |
| **checkpoint** | No (slash only) | Tool + slash | EC leads |
| **skills_*** | 3 tools | 5 tools (+ hub) | EC leads surface |
| **MCP** | Dynamic `mcp_*` | 6 explicit MCP tools | Different (≠) |
| **mixture_of_agents** | `mixture_of_agents` | `moa` (opt-in) | Parity |
| **computer_use** | macOS cua-driver | macOS cua-driver | Parity (both shipped) |
| **x_search** | Gated | Gap 027 | Hermes only |
| **video_analyze** | Opt-in | Gap 012 | Hermes only |
| **video_generate** | Plugin | Gap 012 | Hermes only |
| **kanban_*** (9) | Yes | **11 tools + runs + failure circuit** 🟡 | Hermes (notifier, dashboard) |
| **discord** / **discord_admin** | Platform-gated | No | Hermes only |
| **feishu_doc** / **drive** | 5 tools | No | Hermes only |
| **spotify_*** (7) | Plugin | No | Hermes only |
| **ha_*** | 4 tools | 4 tools | Parity |
| **honcho_*** | 5–6 via memory plugin | 6 built-in | Parity (different packaging) |
| **LSP** (25) | `hermes lsp` integration | 25 tools + write gate | **EC leads depth** |
| **blueprints** | `tools/blueprints.py` | No | Hermes only |

---

## Terminal execution backends

Both support:

| Backend | Hermes | EdgeCrab |
|---------|--------|----------|
| local | `tools/environments/local.py` | `tools/backends/local.rs` |
| docker | Yes | Yes |
| ssh | Yes | Yes |
| modal | Yes | Yes |
| daytona | Yes | Yes |
| singularity | Yes | Yes |

**Verdict:** **Parity (A)**.

---

## Web search backends

| Backend | Hermes | EdgeCrab |
|---------|--------|----------|
| DDGS | plugin | built-in chain |
| Brave | plugin | built-in |
| Exa | plugin | built-in |
| Firecrawl | plugin | built-in |
| Tavily | plugin | built-in |
| SearXNG | plugin | built-in |
| Parallel | plugin | built-in |
| xAI | plugin | built-in |
| Failover chain | Config | `web/search/chain.rs` |

**Verdict:** **EdgeCrab leads integration** (compiled chain); **Hermes leads plug-in new backends without release**.

---

## Browser backends

| Backend | Hermes | EdgeCrab |
|---------|--------|----------|
| Local CDP | Yes | Yes |
| Browserbase | plugin | Configurable |
| Browser Use | plugin | Partial |
| Camofox | plugin | No |
| Firecrawl | plugin | Via web stack |

**Verdict:** **Hermes leads** optional cloud browsers; **parity** on local CDP.

---

## Tool security (dispatch layer)

| Guard | Hermes | EdgeCrab |
|-------|--------|----------|
| Path jail | Yes | `edgecrab-security` |
| SSRF | `url_safety.py` | `url_safety.rs` |
| Command scan | `approval.py` | ~30 patterns |
| Smart approval (LLM) | Yes | Yes (`approvals.mode=smart`) |
| Edit contract limits | Partial | `edit_contract.rs` |
| LSP post-write gate | Yes | Yes (spec 003) |

**Verdict:** **Hermes leads approval UX**; **EdgeCrab leads static enforcement + LSP gate**.

---

## Brutal summary

| Category | Winner |
|----------|--------|
| Coding agent core | **Parity** |
| LSP / IDE coding | **EdgeCrab** |
| Web crawl + search chain | **EdgeCrab** |
| Video / x_search / Spotify | **Hermes** |
| Multi-agent kanban | **Hermes** (dispatcher); EC MVP shipped |
| Media (image/TTS/STT) | **Parity** |
| Computer use | **Parity** |
| MCP surface | **≠** (dynamic vs explicit) |

**EdgeCrab open gaps:** 007 kanban Phase 4 (multi-board, task_runs, failure circuit), 012 video, 027 x-search — see [012-master-gap-matrix.md](012-master-gap-matrix.md).
