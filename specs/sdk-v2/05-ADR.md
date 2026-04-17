# EdgeCrab SDK — Architecture Decision Records

> **Cross-references:** [SPEC](02-SPEC.md) | [IMPL](06-IMPLEMENTATION.md) | [ROADBLOCKS](07-ROADBLOCKS.md)

---

## WHY ADRs Matter for This SDK

The SDK makes irreversible choices that affect every user. These decisions should be documented, debated, and justified — not discovered through source archaeology. Each ADR follows: Context → Decision → Consequences → Alternatives Considered.

---

## ADR-001: PyO3 Native Bindings vs HTTP Client

### Status: ACCEPTED

### Context

The Python SDK needs to call the EdgeCrab Rust runtime. Two approaches exist:

```
+--------------------------------------------------+
|        Option A: PyO3 Native Bindings            |
+--------------------------------------------------+
|                                                  |
|  Python process                                  |
|  +--------------------------------------------+ |
|  | PyO3 module (.so/.pyd)                     | |
|  |  +--------------------------------------+  | |
|  |  | edgecrab-sdk-core (Rust)             |  | |
|  |  | edgecrab-core, tools, state, etc.    |  | |
|  |  | tokio runtime (in-process)           |  | |
|  |  +--------------------------------------+  | |
|  +--------------------------------------------+ |
|                                                  |
|  Latency: ~0.01ms per call                       |
|  Binary size: ~30MB shared library               |
|  Build: maturin + wheel per platform             |
+--------------------------------------------------+

+--------------------------------------------------+
|        Option B: HTTP Client                     |
+--------------------------------------------------+
|                                                  |
|  Python process        Separate EdgeCrab process |
|  +----------------+   +----------------------+  |
|  | HTTP client    |   | edgecrab serve       |  |
|  | (requests/     |-->| (Actix/Axum server) |  |
|  |  httpx)        |   | port 8080            |  |
|  +----------------+   +----------------------+  |
|                                                  |
|  Latency: ~1-5ms per call                        |
|  Binary size: ~50KB Python package               |
|  Build: pure Python, no compilation              |
+--------------------------------------------------+

+--------------------------------------------------+
|        Option C: Subprocess (Claude SDK style)   |
+--------------------------------------------------+
|                                                  |
|  Python process        CLI subprocess            |
|  +----------------+   +----------------------+  |
|  | subprocess.run |   | edgecrab chat        |  |
|  | stdin/stdout   |-->| JSON-RPC over stdio  |  |
|  +----------------+   +----------------------+  |
|                                                  |
|  Latency: ~50-200ms per call (spawn overhead)    |
|  Binary size: ~50KB Python package + CLI binary  |
|  Build: pure Python, requires CLI install        |
+--------------------------------------------------+
```

### Decision

**Primary: PyO3 native bindings (Option A)**
**Fallback: HTTP client (Option B) as separate `edgecrab-client` package**

### Consequences

**Positive:**
- Zero IPC overhead — agent loop runs in-process
- No separate server to start — `pip install edgecrab` is all you need
- Streaming events arrive without HTTP chunking latency
- Session state is shared in-memory, not serialized over the wire
- Token cost is exactly what the LLM API returns — no proxy overhead

**Negative:**
- Must build wheels for every platform (manylinux, macOS arm64/x86, Windows)
- Binary size ~30MB — large for a Python package
- Build requires Rust toolchain — no contribution without rustup
- Debugging segfaults requires Rust knowledge
- Cannot hot-reload the Rust core — must rebuild wheel

**Why not subprocess (Option C):**
- Claude Agent SDK uses this and it works, but cold start ~2s per query is unacceptable for production agents
- No shared session state — each subprocess invocation is stateless
- Serialization overhead for every tool call result

### Alternatives Considered

| Alternative | Why Rejected |
|-------------|-------------|
| Pure Python reimplementation | Would lose performance, security guarantees, and tool parity |
| gRPC | Over-engineered for single-process use case |
| Unix domain sockets | Same IPC overhead as HTTP, less portable |
| ctypes/cffi | Less ergonomic than PyO3, manual memory management |

---

## ADR-002: Napi-RS vs N-API vs Subprocess for Node.js

### Status: ACCEPTED

### Context

Similar to Python, the Node.js SDK needs to call the Rust runtime.

### Decision

**Primary: napi-rs native bindings**
**Fallback: HTTP client as `@edgecrab/client` package**

### Consequences

Same trade-offs as ADR-001 (PyO3), with Node-specific notes:
- napi-rs generates `.node` files per platform
- `npm install` triggers optional dependency download (platform-specific binary)
- Node.js async model maps cleanly to Rust's tokio via napi-rs async functions
- Binary size is similar (~30MB)

### Alternatives Considered

| Alternative | Why Rejected (for full Node.js SDK) |
|-------------|-------------|
| WebAssembly (WASM) | No filesystem/network access — incompatible with full tool execution. **However, a separate WASM SDK target is planned as a "lite" variant** (see ADR-006) |
| Edge runtime (Deno Deploy, CF Workers) | Viable via the WASM SDK lite variant — see ADR-006 |
| Native Node addon (C++) | Extra C++ bridge layer adds complexity |

---

## ADR-003: Single `edgecrab-sdk-core` Crate vs Direct Bindings

### Status: ACCEPTED

### Context

PyO3 and napi-rs bindings could either:
1. Bind directly to `edgecrab-core` + `edgecrab-tools` + `edgecrab-state`
2. Bind to a new `edgecrab-sdk-core` crate that provides a stable facade

```
+--------------------------------------------+
|  Option A: Direct bindings                 |
+--------------------------------------------+
|  PyO3/Napi --> edgecrab-core               |
|            --> edgecrab-tools              |
|            --> edgecrab-state              |
|  Problem: Any internal API change breaks   |
|  the bindings layer                        |
+--------------------------------------------+

+--------------------------------------------+
|  Option B: SDK core facade                 |
+--------------------------------------------+
|  PyO3/Napi --> edgecrab-sdk-core           |
|                  |-> edgecrab-core          |
|                  |-> edgecrab-tools         |
|                  |-> edgecrab-state         |
|  Benefit: Stable public API surface        |
+--------------------------------------------+
```

### Decision

**Option B: New `edgecrab-sdk-core` crate** that provides:
- `SdkAgent` — wraps `Agent` with stable public API
- `SdkConfig` — wraps `AppConfig` with stable accessors
- `SdkRegistry` — wraps `ToolRegistry` with stable tool registration
- Conversion traits between SDK types and internal types

### Consequences

**Positive:**
- Internal refactors don't break the SDK — only `edgecrab-sdk-core` needs updating
- Clear semver boundary — SDK version can differ from runtime version
- Bindings code is simpler — one dependency, not five

**Negative:**
- Extra crate to maintain
- Potential for facade to lag behind new features
- Slight indirection cost (trivial — all inlined)

---

## ADR-004: Tool Registration Pattern

### Status: ACCEPTED

### Context

How should SDK users register custom tools?

| Pattern | Example SDK | Ergonomics | Type Safety |
|---------|-------------|------------|-------------|
| Decorator | Pydantic AI, Claude SDK | Excellent | Good (via hints) |
| Class/Trait | edgecrab-tools (current) | Verbose | Excellent |
| Object literal | OpenAI Agents SDK | Good | Moderate |
| Proc macro | Rust ecosystem | Excellent | Excellent |

### Decision

**All patterns supported, decorator preferred for Python/Node.js, proc macro for Rust:**

```python
# Python: decorator (preferred)
@Tool("name", description="...")
async def my_tool(arg: str) -> dict: ...

# Python: class (advanced)
class MyTool(ToolHandler):
    name = "name"
    async def execute(self, args, ctx): ...
```

```rust
// Rust: proc macro (preferred)
#[edgecrab::tool(name = "name", description = "...")]
async fn my_tool(arg: String) -> Result<String, ToolError> { ... }

// Rust: trait (advanced)
impl ToolHandler for MyTool { ... }
```

### Consequences

**Positive:**
- Decorator pattern matches developer expectations from FastAPI, Flask, pytest
- Schema auto-generated from type hints (Python) / function signature (Rust)
- Class pattern available for tools that need state (database connections, etc.)

**Negative:**
- Decorator magic can be confusing for debugging
- Schema inference from Python type hints is imperfect for complex types
- Two patterns to document and test

---

## ADR-005: Async-First vs Sync-First API

### Status: ACCEPTED

### Context

LLM calls are inherently async (network I/O). Should the SDK API be async-first or sync-first?

| SDK | Default | Sync wrapper |
|-----|---------|-------------|
| Claude Agent SDK | Async only | None |
| OpenAI Agents SDK | Async (Runner.run) | Runner.run_sync |
| Pydantic AI | Async | agent.run_sync() |
| Google ADK | Async | Runner.run_sync |

### Decision

**Async-first with sync wrappers for every method:**

```python
# Async (preferred)
result = await agent.chat("hello")

# Sync wrapper (convenience)
result = agent.chat_sync("hello")
```

### Consequences

**Positive:**
- Matches natural model of LLM I/O
- Sync wrappers make scripting/notebooks easy
- All streaming APIs are async-only (correct — you can't sync-stream)

**Negative:**
- Sync wrappers create a tokio runtime internally — can't nest in existing async contexts
- Two code paths to test

---

## ADR-006: Error Handling Strategy

### Status: ACCEPTED

### Context

How should errors propagate from the Rust runtime through FFI to Python/Node.js?

### Decision

**Typed exception hierarchy mirroring Rust error types:**

```
Rust:    AgentError::RateLimited { provider, retry_after }
           ↓ PyO3 conversion
Python:  raise AgentError.RateLimited(provider="anthropic", retry_after_ms=5000)

Rust:    ToolError::InvalidArgs { tool, message }
           ↓ napi-rs conversion
Node.js: throw new ToolError.InvalidArgs({ tool: "...", message: "..." })
```

### Consequences

**Positive:**
- Users can catch specific error types
- Error messages include actionable context (retry_after, tool name, etc.)
- Stack traces originate from the correct layer (not buried in FFI)

**Negative:**
- Must maintain error type mappings across 3 languages
- Some Rust error details may be lost in translation (lifetimes, internal state)

---

## ADR-007: Configuration Loading Priority

### Status: ACCEPTED

### Context

Configuration can come from multiple sources. What's the priority?

### Decision

```
+----------------------------------------------------------+
|           Configuration Resolution Order                  |
+----------------------------------------------------------+
|                                                          |
|  1. Constructor kwargs        (highest priority)          |
|     Agent(model="...", max_iterations=50)                |
|                                                          |
|  2. Environment variables                                 |
|     EDGECRAB_MODEL, EDGECRAB_MAX_ITERATIONS              |
|                                                          |
|  3. Config file (explicit path)                           |
|     Config.load_from("./my-config.yaml")                 |
|                                                          |
|  4. Config file (default)                                 |
|     ~/.edgecrab/config.yaml                              |
|                                                          |
|  5. Compiled defaults          (lowest priority)          |
|     DEFAULT_CONFIG in edgecrab-core                      |
+----------------------------------------------------------+
```

### Consequences

**Positive:**
- Familiar pattern (12-factor app compliance)
- Constructor overrides are obvious and testable
- Config file provides persistent defaults without env var pollution

**Negative:**
- 5 layers of config resolution can be hard to debug ("where did this value come from?")
- Must document resolution order clearly

---

## ADR-008: Session Storage — SQLite vs File-per-session

### Status: ACCEPTED (inherited from edgecrab-state)

### Context

EdgeCrab already uses SQLite WAL + FTS5 for session storage. The SDK exposes this.

### Decision

**Keep SQLite. Expose via SessionDb type in SDK.**

### Consequences

**Positive:**
- FTS5 full-text search across all sessions — unique among competitors
- WAL mode allows concurrent reads during writes
- Single file (`sessions.db`) — easy to backup, migrate, inspect

**Negative:**
- SQLite native library must be bundled or linked
- Cannot use in serverless environments that don't support file I/O
- No built-in remote/distributed session storage (must be external)

---

## ADR-009: Streaming Protocol — Server-Sent Events vs WebSocket vs Callback

### Status: ACCEPTED

### Context

For the HTTP fallback client, how should streaming responses be delivered?

### Decision

**SSE for HTTP client. Callbacks + async iterators for native bindings.**

```
Native (PyO3/Napi):
  async for event in agent.stream("..."): ...
  - Zero serialization
  - Direct memory sharing via Arc<Mutex<...>>

HTTP fallback:
  GET /v1/chat/completions (stream=true)
  - SSE format (text/event-stream)
  - Compatible with existing OpenAI client libraries
```

### Consequences

**Positive:**
- SSE is widely supported, auto-reconnects, works through proxies
- Native streaming has zero overhead
- OpenAI compatibility means existing tools (Langfuse, etc.) work out of the box

**Negative:**
- SSE is unidirectional — cannot send interrupts through the stream
- Interrupts via separate HTTP DELETE /v1/chat/completions/{id} endpoint

---

## ADR-010: Wheel Size and Tree-Shaking

### Status: PROPOSED

### Context

The full EdgeCrab runtime is ~30MB compiled. This is large for a Python/Node.js package.

### Decision

**Ship full runtime in v2.0. Investigate modular builds in v2.1.**

Possible future approach:
```
edgecrab[full]     — 30MB, all tools (default)
edgecrab[core]     — 8MB, agent + custom tools only (no built-in tools)
edgecrab[web]      — 12MB, core + web tools
edgecrab[browser]  — 20MB, core + web + browser (includes CDP)
```

### Consequences

**Positive:**
- v2.0 ships faster without feature-gating complexity
- Users get the full experience out of the box
- No confusion about which features are available

**Negative:**
- 30MB download for simple use cases
- CI/CD pipelines download unnecessary tools
- Cold start in Lambda/Cloud Functions is slower

---

## ADR-006: WASM SDK as Lite Compilation Target

### Status: PROPOSED

### Context

EdgeCrab's core is Rust, and Rust compiles to WebAssembly. This enables running the agent loop directly in browsers and edge compute platforms (Cloudflare Workers, Deno Deploy, Vercel Edge Functions) — without a server. However, WASM environments lack filesystem access, subprocess spawning, and native networking (only `fetch` is available).

ADR-002 rejected WASM for the Node.js SDK because full tool execution requires filesystem/network/subprocess access. This ADR proposes a separate "lite" WASM SDK that accepts these limitations and exposes only the WASM-compatible subset.

### Decision

**Ship `@edgecrab/wasm` as a fourth SDK target alongside Rust, Python, and Node.js.**

The WASM SDK includes:
- Agent core (ReAct loop, context compression, model routing)
- Model catalog (15 providers, 200+ models)
- Cost tracking and token counting
- Streaming via JS callbacks (Rust `async fn` → JS `Promise` via `wasm-bindgen-futures`)
- Custom tools registered in JavaScript (via `WasmToolHandler` — JS functions called from WASM)
- Session persistence via adapter pattern (IndexedDB, KV stores, external APIs)

The WASM SDK excludes:
- All 100+ built-in tools (file I/O, terminal, browser automation, MCP stdio, etc.)
- SQLite session store (replaced by JS adapter)
- `tokio` runtime (replaced by `wasm-bindgen-futures` / `spawn_local`)
- SSRF guard (`std::net` unavailable — security delegated to browser/edge runtime fetch policies)

### Compilation Toolchain

```
Rust source → wasm-pack → wasm-bindgen → .wasm + .js glue + .d.ts types
                                                 ↓
                                            @edgecrab/wasm (npm)
```

Targets:
- `wasm-pack build --target web` — browser ESM
- `wasm-pack build --target bundler` — Webpack/Vite
- `wasm-pack build --target nodejs` — Node.js (for testing/SSR)

### Consequences

**Positive:**
- Browser-native AI agents with no backend needed
- Cloudflare Workers / Vercel Edge Functions with sub-10ms cold starts
- Same Rust agent loop code runs everywhere — no reimplementation
- Auto-generated TypeScript definitions from `#[wasm_bindgen]`
- Custom tools in JS have full access to browser/edge APIs (IndexedDB, Web Audio, Canvas, etc.)

**Negative:**
- No built-in tools — users must implement all tools as JS callbacks
- Binary size (~3-5MB gzipped) is large for a browser library
- `reqwest` WASM mode has limitations (no connection pooling, no HTTP/2)
- CORS restrictions limit which LLM APIs can be called from browser (proxy may be needed)
- Testing requires headless browser (`wasm-pack test --headless --chrome`)
- Single-threaded — no `Send + Sync`, no `tokio::spawn`

### Alternatives Considered

| Alternative | Why Not Chosen |
|-------------|---------------|
| Transpile Rust to JS | No mature Rust→JS transpiler; would lose type safety |
| Rewrite agent core in TypeScript | Duplication; diverges from Rust codebase; performance loss |
| Thin HTTP client only (like current Node.js SDK) | Requires a server; defeats the purpose of edge/browser compute |
| WASI Preview 2 only | Browser support is nascent; Cloudflare Workers WASI support is experimental |

### Prerequisites Before Shipping

1. `edgecrab-core` must compile with `#[cfg(target_arch = "wasm32")]` gates
2. A `no-fs` Cargo feature flag must strip filesystem-dependent code
3. `reqwest` WASM feature must be validated for all 15 provider HTTP flows
4. Session persistence must use trait-based adapter (not hardcoded rusqlite)
5. Binary size must be verified <5MB gzipped

---

## Brutal Honest Assessment

### Strengths of These ADRs
- Cover the genuinely hard decisions (FFI strategy, error propagation, config resolution)
- Each alternative is honestly evaluated, not strawman-dismissed
- Consequences include negatives — not just positive spin

### Weaknesses
- **ADR-001 assumes PyO3 wheel building works reliably** — manylinux builds for Rust+Python are notoriously fragile; should have a "if wheels fail" contingency
- **ADR-010 punts on binary size** — 30MB is a real barrier for serverless deployments; competitors (Claude SDK aside) are <1MB packages
- **Missing ADR for versioning strategy** — how does SDK version relate to runtime version?
- **Missing ADR for backward compatibility guarantees** — when can we break the API?

### Improvements Made After Assessment
- Added explicit "Alternatives Considered" tables to each ADR
- Added ADR-010 to acknowledge binary size issue even if deferred
- Noted that subprocess approach (Option C) is what Claude SDK uses — honest comparison
- Added clear status (ACCEPTED/PROPOSED) to each ADR
