# EdgeCrab Plugin System — Overview & Motivation

**Status:** PROPOSED  
**Version:** 0.1.0  
**Date:** 2026-04-09  
**Authors:** Engineering Team  
**Cross-refs:** [001_adr_architecture], [003_manifest], [005_lifecycle], [012_crate_structure], [014_implementation_plan], [017_plugin_tui], [018_remote_plugin_search_tui]

---

## 1. Executive Summary

EdgeCrab is a production-grade Rust agent with a compile-time tool registry (`inventory!`).
This is fast and safe, but **cannot be extended at runtime without recompiling the binary**.

The Plugin System bridges that gap: a **runtime-extensible layer** that follows the same
security-first, layered approach already present in edgecrab-tools, while matching the
community experience that hermes-agent delivers with its Python registry pattern.

```
Before (static)                    After (static + runtime plugins)
─────────────────                  ──────────────────────────────────
edgecrab binary                    edgecrab binary
  └── inventory! tools (fixed)       ├── inventory! tools (fixed, fast)
                                     └── PluginRegistry (runtime, extensible)
                                           ├── skill plugins  (markdown)
                                           ├── tool-server plugins (subprocess/JSON-RPC)
                                           └── script plugins  (Rhai, future WASM)
```

---

## 2. Problem Statement

### 2.1 What hermes-agent has that edgecrab lacks

| Capability | hermes-agent | edgecrab (today) | Gap |
|---|---|---|---|
| Skill injection into prompt | ✅ SKILL.md | ✅ SKILL.md | None |
| Runtime tool registration | ✅ Python `registry.register()` | ❌ compile-time only | **CRITICAL** |
| Community plugin hub | ✅ skills.sh + GitHub | ⚠️ skills hub (markdown only) | **HIGH** |
| Executable plugin sandboxing | ✅ subprocess isolation | ❌ no runtime exec plugins | **HIGH** |
| Plugin enable/disable per session | ✅ via config | ⚠️ skills only | **MEDIUM** |
| Agent-creatable plugins | ✅ `skill_manage` tool | ⚠️ skills only | **MEDIUM** |
| Plugin versioning / conflict detection | ❌ | ❌ | **LOW** |

### 2.2 First-Principles Constraints

1. **Safety first** — A plugin crash must NEVER take down the agent process.
2. **Zero compile-time regression** — All existing `inventory!` tools must work unchanged.
3. **DRY** — Plugin discovery, security scanning, and hub code MUST NOT duplicate the
   existing `skills_hub.rs`, `skills_guard.rs`, `skills_sync.rs` logic. It extends them.
4. **SOLID**
   - _Single Responsibility_: each crate has one job.
   - _Open/Closed_: add new plugin kinds via a trait, not new match arms everywhere.
   - _Liskov_: Every `PluginKind` is a valid `Plugin` — no "unsupported operation" panics.
   - _Interface Segregation_: Consumers that only need tool dispatch don't depend on
     install/security code.
   - _Dependency Inversion_: `edgecrab-core` depends on traits in `edgecrab-plugins`,
     not concrete implementations.

---

## 3. Scope

### 3.1 In-Scope (Phase 1 — this spec series)

- Plugin manifest format (`plugin.toml`)
- Three plugin kinds: **Skill**, **ToolServer**, **Script (Rhai)**
- PluginRegistry trait + in-process implementation
- Security scanning (extending `skills_guard.rs`)
- Lifecycle: discover → quarantine → scan → install → enable → disable → uninstall
- CLI commands: `/plugins` (list, install, remove, enable, disable, info, search, browse, hub)
- Interactive remote plugin browser with parity to the remote skills browser
- Config schema (`plugins:` section in `config.yaml`)
- New crate: `edgecrab-plugins`
- Integration: `edgecrab-core` prompt injection + tool dispatch routing

### 3.2 Explicitly Out-of-Scope (Phase 2+)

- WASM/WASI plugins (complex host ABI, deferred)
- Native `.so`/`.dylib` plugins (security concerns, deferred)
- Plugin marketplace payments / licencing
- Hot-reload without restart (nice-to-have, requires unsafe)
- Multi-agent plugin sharing across network

---

## 4. System Context Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          User / Gateway                                     │
└──────────────────────────────────┬──────────────────────────────────────────┘
                                   │ /plugins install, /plugins list, etc.
                                   ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  edgecrab-cli / edgecrab-gateway (UI layer)                                 │
│                                                                             │
│   PluginCommands  ──────────────── /plugins (46 slash commands)             │
└──────────────────────────────────┬──────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  edgecrab-plugins (NEW CRATE)                                               │
│                                                                             │
│   ┌────────────────┐  ┌──────────────────┐  ┌──────────────────────────┐   │
│   │ PluginRegistry │  │ PluginLifecycle  │  │    PluginSecurityScanner │   │
│   │ (trait)        │  │ (install, scan,  │  │    (extends skills_guard) │   │
│   │                │  │  quarantine, ...) │  │                          │   │
│   └───────┬────────┘  └──────────────────┘  └──────────────────────────┘   │
│           │                                                                 │
│   ┌───────▼────────────────────────────────────────────────────────────┐    │
│   │ kinds/                                                             │    │
│   │  ├── SkillPlugin      (reads SKILL.md → prompt injection)         │    │
│   │  ├── ToolServerPlugin (subprocess JSON-RPC → dynamic ToolHandler) │    │
│   │  └── ScriptPlugin     (Rhai eval → ToolHandler wrapper)           │    │
│   └────────────────────────────────────────────────────────────────────┘    │
└────────────────────┬───────────────────────────────────────────────────────┘
                     │ integrates with
          ┌──────────┴──────────┐
          │                     │
          ▼                     ▼
  edgecrab-core           edgecrab-tools
  (PromptBuilder)         (ToolRegistry +
  (Agent loop)             inventory! tools)
```

---

## 5. Key Invariants (Code is Law)

These invariants MUST be preserved by every PR touching the plugin system.

```
INV-1: Plugin subprocess death MUST NOT kill the agent process.
INV-2: A plugin tool call timeout MUST NOT block other tool calls.
INV-3: A quarantined plugin MUST NOT execute any code.
INV-4: compile-time inventory! tools take priority over plugin tools with the same name.
INV-5: All file paths within plugins MUST pass edgecrab_security path validation.
INV-6: All network requests from plugins MUST pass edgecrab_security SSRF check.
INV-7: Plugin manifests failing SHA-256 integrity check MUST NOT be installed.
INV-8: Disabled plugins MUST NOT appear in the tool list sent to the LLM.
INV-9: Plugin install MUST be idempotent (re-install same version = no-op).
INV-10: Plugin state MUST persist across agent restarts.
```

---

## 6. Related Documents

| Document | Topic |
|---|---|
| [001_adr_architecture.md] | Why subprocess+JSON-RPC over WASM/native |
| [002_adr_transport.md] | Why newline-delimited JSON over raw protobuf |
| [003_manifest.md] | Full `plugin.toml` schema with examples |
| [004_plugin_types.md] | Skill, ToolServer, Script plugin specifications |
| [005_lifecycle.md] | State machine: install → quarantine → scan → enable |
| [006_security.md] | Threat model, scanning rules, trust levels |
| [007_registry.md] | PluginRegistry trait, RwLock internals, dispatch |
| [008_host_api.md] | Host functions exposed to plugins, capability model |
| [009_discovery_hub.md] | Hub protocol, index format, trust propagation |
| [010_cli_commands.md] | `/plugins` slash command spec |
| [011_config_schema.md] | `plugins:` section in `config.yaml` |
| [012_crate_structure.md] | `edgecrab-plugins` crate module layout |
| [013_edge_cases.md] | Failure modes, race conditions, security edge cases |
| [014_implementation_plan.md] | Phased rollout, milestones, test strategy |
| [017_plugin_tui.md] | Local installed-plugin toggle overlay |
| [018_remote_plugin_search_tui.md] | Remote official/configured plugin search browser |

---

## 7. Glossary

| Term | Definition |
|---|---|
| **Plugin** | A self-contained unit of functionality installable at runtime |
| **Skill Plugin** | A markdown-based plugin that injects text into the system prompt |
| **ToolServer Plugin** | A subprocess that exposes tools via JSON-RPC 2.0 |
| **Script Plugin** | An embedded Rhai script that implements one or more tool handlers |
| **PluginManifest** | The `plugin.toml` file declaring plugin metadata and capabilities |
| **Quarantine** | Temporary staging directory where downloaded plugins await scan |
| **Trust Level** | `builtin` > `trusted` > `community` — affects install policy |
| **Capability** | A declared permission a plugin needs from the host (e.g. `host:memory`) |
| **Host API** | Functions the edgecrab runtime exposes to plugin processes |
| **PluginRegistry** | Runtime table of active plugins and their exposed tools |
