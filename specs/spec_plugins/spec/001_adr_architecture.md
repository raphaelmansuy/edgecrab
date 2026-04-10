# ADR-001: Plugin Architecture — Subprocess JSON-RPC over WASM / Native Libraries

**Status:** ACCEPTED  
**Date:** 2026-04-09  
**Deciders:** Engineering Team  
**Cross-refs:** [000_overview], [002_adr_transport], [004_plugin_types], [006_security]

---

## Context

EdgeCrab needs a plugin system that:
1. Allows runtime-extensible tools without recompiling the binary
2. Isolates plugin failures so the agent cannot crash
3. Is buildable without wrestling C FFI / LLVM backends on all target platforms
4. Enables community contributions with minimal friction

We evaluated four architectural alternatives.

---

## Decision Drivers

| Driver | Weight | Notes |
|---|---|---|
| Process isolation (crash safety) | CRITICAL | Plugin crash must not kill agent |
| Cross-platform portability | HIGH | macOS, Linux, Windows (future) |
| Security sandboxing | HIGH | Plugins cannot exfiltrate secrets |
| Build complexity | MEDIUM | Team should be able to ship quickly |
| Performance overhead | MEDIUM | <50ms first call, <5ms steady state |
| Developer experience | MEDIUM | Plugin authors should not need Rust |
| Community ecosystem | LOW | Nice to have familiar tooling |

---

## Options Considered

### Option A — Native Shared Libraries (`.so` / `.dylib`)

```
edgecrab binary
  └── dlopen("my-plugin.so")
        └── fn init_plugin(registry: *mut ToolRegistry)
```

**Pros:** Zero serialization overhead, full Rust API access.  
**Cons:**
- A single `unwrap()` in plugin code crashes the WHOLE agent process. **CRITICAL FAIL.**
- Shared library ABI is not stable in Rust (no stable `#[repr(C)]` for async traits).
- Plugin authors MUST use identical Rust version + feature flags. Impossible for community.
- No meaningful sandboxing — plugin has full process memory access.

**Verdict: REJECTED** — INV-1 (crash isolation) cannot be satisfied.

---

### Option B — WebAssembly / WASI

```
edgecrab binary
  └── wasmtime::Engine
        └── Module::from_file("plugin.wasm")
              └── host imports (read_file, http_get, ...)
```

**Pros:** Strong sandbox, language-agnostic, memory-isolated.  
**Cons:**
- `wasmtime` crate adds ~20 MB to binary size, 3-8s initial JIT compile per plugin.
- Async support in WASM is 2024-era nightly only; `component-model` async stabilization
  is expected late 2026 — too uncertain for Phase 1.
- Plugin authors need WASM toolchain (`wasm-pack`, `wasm-bindgen`) — steep learning curve.
- WASI filesystem + network access requires careful host shim — ~3000 lines of glue code.
- WASM64 for large memory plugins is still experimental.

**Verdict: DEFERRED** to Phase 2. Architecture chosen today must accommodate WASM as a
future `PluginKind::Wasm` variant without breaking changes (see [004_plugin_types]).

---

### Option C — Embedded Scripting (Rhai only)

```
edgecrab binary
  └── rhai::Engine
        └── eval_file("plugin.rhai")
              └── scope with exposed host functions
```

**Pros:** Zero subprocess overhead, Rhai is safe (no unsafe), tiny ~800KB dep.  
**Cons:**
- Rhai is synchronous; wrapping async host calls requires `block_on()` bridges.
- Cannot spawn background threads, do HTTP streaming, or shell out.
- Scripts run IN-PROCESS — a tight infinite loop can still block the agent.
- No stdlib for complex operations (file I/O, HTTP) without explicit host exposure.

**Verdict: ACCEPTED as one of three PluginKind variants** (Script), but NOT as the
primary architecture for tool-server plugins that need I/O.

---

### Option D — Subprocess JSON-RPC 2.0 (CHOSEN)

```
edgecrab binary                   plugin binary / script
  │                                      │
  │    stdin ──[ JSON-RPC request ] ──▶  │
  │                                      │  execute tool
  │    stdout ◀─[ JSON-RPC response ]──  │
  │                                      │
  │    (on plugin crash → EOF on pipe)   │
  │    (edgecrab catches BrokenPipe →    │
  │     returns ToolError::PluginCrashed)│
```

**Pros:**
- **Process isolation**: plugin crash = EOF/BrokenPipe, NOT agent panic. Satisfies INV-1.
- **Language agnostic**: plugins can be Python, Node, Go, Bash — any language.
- **Protocol established**: JSON-RPC 2.0 is the MCP protocol. We already have mcp_client.rs.
- **Restart possible**: if plugin dies, agent can restart it transparently.
- **Partial sandboxing**: OS ulimits, Landlock (Linux), Seatbelt (macOS) around subprocess.
- **Portable**: works identically on Linux, macOS, Windows (stdin/stdout are universal).

**Cons:**
- Serialization overhead: ~0.1-1ms per call (acceptable per performance budget).
- Requires process management (PID tracking, cleanup on agent exit).
- Plugin startup latency: 50-200ms for first tool call (mitigated by eager startup).

**Verdict: ACCEPTED as the primary PluginKind::ToolServer architecture.**

---

## Decision

We adopt a **hybrid three-tier architecture**:

```
┌─────────────────────────────────────────────────────┐
│              PluginKind enum                        │
│                                                     │
│  Skill      ──▶ reads SKILL.md, injects into prompt │
│             ──▶ NO subprocess, NO code execution    │
│             ──▶ existing skills_hub.rs path         │
│                                                     │
│  ToolServer ──▶ spawns subprocess                   │
│             ──▶ communicates via JSON-RPC 2.0 stdio │
│             ──▶ MCP-compatible protocol             │
│             ──▶ process-isolated (crash-safe)       │
│                                                     │
│  Script     ──▶ embedded Rhai engine (in-proc)      │
│             ──▶ synchronous, sandboxed eval         │
│             ──▶ no network/fs unless explicitly     │
│                declared in capabilities             │
│                                                     │
│  [Wasm]     ──▶ RESERVED for Phase 2 (wasmtime)    │
└─────────────────────────────────────────────────────┘
```

### Rationale Summary

The subprocess model (Option D) provides the best balance of isolation, language
freedom, and implementation speed. It reuses the JSON-RPC 2.0 framing already proven
in `mcp_client.rs`. Rhai (Option C) is a natural fit for lightweight single-tool plugins
that the agent itself generates (`skill_manage` equivalent). WASM (Option B) is the
right long-term answer but technology is not yet stable enough.

Compile-time `inventory!` tools are UNCHANGED — plugin tools layer ON TOP, not instead.

---

## Consequences

### Positive
- Plugin authors can write in any language (Python, Node, Go, Bash, Ruby).
- Plugin crash is gracefully recovered — the agent logs a warning and continues.
- The MCP transport code is reused, reducing surface area.
- Rhai scripts can be generated by the agent itself (agent-managed plugins).

### Negative
- Process management complexity: need PID table, graceful shutdown, restart policy.
- Windows named-pipe transport may differ slightly from POSIX stdin/stdout (future work).
- ToolServer plugins have ~100ms startup latency (first tool call per session).

### Neutral
- WASM plugins will become `PluginKind::Wasm` in Phase 2 — the enum is designed open.
- Existing `skills` system is unchanged, skill plugins are a superset.

---

## Compliance with SOLID/DRY

| Principle | How satisfied |
|---|---|
| SRP | `edgecrab-plugins` owns plugin lifecycle; `edgecrab-tools` owns built-in dispatch |
| OCP | New kinds added via new `PluginKind` variant + impl; no existing match arms change |
| LSP | Every `PluginKind` variant implements the same `Plugin` trait fully |
| ISP | `PluginRegistry` trait exposes only the needed surface to each consumer |
| DIP | `edgecrab-core` depends on `PluginRegistry` trait, not the concrete impl |
| DRY | Reuses `skills_guard.rs` scanning, `mcp_client.rs` transport, `skills_hub.rs` discovery |
