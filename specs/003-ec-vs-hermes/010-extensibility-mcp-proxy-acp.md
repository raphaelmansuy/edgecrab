# 010 — Extensibility: MCP, Proxy, ACP, Plugins

How third parties extend each runtime without forking core.

---

## MCP (Model Context Protocol)

| Feature | Hermes | EdgeCrab |
|---------|--------|----------|
| Client transports | stdio + HTTP | stdio + HTTP |
| Tool naming | Dynamic `mcp_<server>_<tool>` | Explicit `mcp_list_tools`, `mcp_call_tool`, … |
| Resource/prompt tools | Via MCP | `mcp_list/read_resources`, `mcp_list/get_prompts` |
| OAuth refresh (HTTP) | Yes | Yes (spec 010 shipped) |
| Parallel MCP calls | Yes | Yes |
| Token store | Config + files | `~/.edgecrab/mcp-tokens/` |
| Catalog/install | `optional-mcps/`, `hermes mcp install` | `mcp_catalog.rs`, config yaml |
| Reload | `/reload-mcp` | `/reload-mcp` |
| **Hermes as MCP server** | `hermes mcp` exposes tools | No first-class equivalent |
| Codex callback MCP | Yes | No |

**Verdict:** **Hermes leads ecosystem** (server mode + dynamic tools); **EdgeCrab leads explicit tool surface** for LLM schema stability.

---

## ACP (Agent Communication Protocol)

| | Hermes | EdgeCrab |
|---|--------|----------|
| Transport | JSON-RPC stdio | JSON-RPC stdio |
| Crate/module | `acp_adapter/` | `edgecrab-acp/` |
| Launch | `hermes acp` | `edgecrab acp` |
| Workspace init | Manual | `edgecrab acp init` |
| Tool subset | Coding-focused | `ACP_TOOLS` (core − media − clarify + LSP) |
| VS Code integration | Yes | Yes |

**Verdict:** **Parity (A)** — EdgeCrab adds init automation.

---

## Subscription proxy

See [006-models-providers-routing.md](006-models-providers-routing.md).

Both: local OpenAI-compatible server for driving external clients with subscription OAuth credentials.

**Verdict:** **Parity**.

---

## Plugin systems (largest divergence)

### Hermes: Python plugin economy

| Aspect | Detail |
|--------|--------|
| Discovery | `~/.hermes/plugins/`, `.hermes/plugins/`, pip entry points |
| Manifest | `plugin.yaml` |
| Categories | model-providers, web, browser, memory, platforms, video_gen, image_gen, observability, … |
| Install | `hermes plugins install` |
| Count in repo | 100+ directories under `plugins/` |
| Hot reload | Partial (`/reload`) |

**Strength:** Add provider/platform/tool **without recompiling Hermes**.

### EdgeCrab: compile-time + ADR subprocess

| Aspect | Detail |
|--------|--------|
| Crate | `edgecrab-plugins/` |
| ADR | Subprocess JSON-RPC (WASM deferred per spec_plugins) |
| Kinds | Skill, ToolServer, Script, **Hermes** compat |
| Hermes hooks | `invoke_hermes_hook`, `extract_pre_llm_context` |
| Context engines | `discover_context_engines()` |
| Security | `guard.rs` bundle scanner |

**Strength:** Type-safe core tools; Hermes hook compat for migration.

**Weakness:** Provider plugins (gap **009**) not at Hermes parity — new providers need Rust/catalog edits or proxy forward.

**Verdict:** **Hermes A vs EdgeCrab C+** on extensibility today.

---

## Context engine plugins

| | Hermes | EdgeCrab |
|---|--------|----------|
| Default | `"compressor"` | Built-in compression |
| Alternatives | `"lcm"` lossless plugin | `ContextEngine` trait (pluggable) |
| Config | `context.engine` | `context.engine` in config |

**Verdict:** **Hermes leads shipped alternatives**; EdgeCrab has hook point.

---

## Hooks (lifecycle)

Both: config-defined shell/Python/JS hooks on gateway lifecycle events.

| | Hermes | EdgeCrab |
|---|--------|----------|
| Module | `gateway/hooks.py` | `gateway/hooks.rs` |
| User dir | config paths | `~/.edgecrab/hooks/` |
| CLI | `hermes hooks` | `/hooks` |

**Verdict:** **Parity**.

---

## SDK / programmatic use

| | Hermes | EdgeCrab |
|---|--------|----------|
| Python API | Import `AIAgent` | `edgecrab-sdk` + PyO3 (`sdks/python/`) |
| Node API | Limited | `sdks/nodejs-native/` |
| Stable facade | Informal | `edgecrab-sdk-core` |

**Verdict:** **EdgeCrab leads** first-class SDK story.

---

## Grades

| Dimension | Hermes | EdgeCrab |
|-----------|--------|----------|
| MCP client | A | A |
| MCP server | A | D |
| ACP | A | A |
| Proxy | B+ | B+ |
| Plugins | A | C+ |
| SDK | C | B+ |

Cross-ref: [001-gap-analysis 009/010/030](../001-gap-analysis-v14/999-roadmap.md), [spec_plugins](../spec_plugins/spec/001_adr_architecture.md)
