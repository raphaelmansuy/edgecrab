# EdgeCrab SDK — Implementation Plan

> **Cross-references:** [SPEC](02-SPEC.md) | [ADR](05-ADR.md) | [ROADBLOCKS](07-ROADBLOCKS.md)

---

## WHY a Phased Plan

Shipping a tri-language SDK with native bindings, 90+ tools, and full API parity is a multi-month effort. Phasing reduces risk — each phase delivers a usable SDK that gets developer feedback before committing to the next layer.

---

## Phase Overview

```
+------------------------------------------------------------------------+
|                      Implementation Phases                              |
+------------------------------------------------------------------------+
|                                                                        |
|  Phase 0       Phase 1        Phase 2       Phase 3       Phase 4      |
|  Foundation    Rust SDK       Python SDK    Node.js SDK   Polish       |
|  (2 weeks)     (3 weeks)      (4 weeks)     (3 weeks)     (2 weeks)    |
|                                                                        |
|  +----------+ +------------+ +-----------+ +-----------+ +----------+ |
|  | sdk-core | | Rust pub   | | PyO3      | | napi-rs   | | Docs     | |
|  | crate    | | API        | | bindings  | | bindings  | | Examples | |
|  | Types    | | Streaming  | | Wheels    | | npm pkg   | | Tests    | |
|  | Facade   | | Tools API  | | Decorator | | TypeScript| | Bench    | |
|  | Tests    | | Sessions   | | Types     | | Types     | | CI/CD    | |
|  +----------+ +------------+ +-----------+ +-----------+ +----------+ |
|                                                                        |
|  Total estimated: 14 weeks                                              |
+------------------------------------------------------------------------+
```

---

## Phase 0: Foundation (Weeks 1–2)

### Goal
Create `edgecrab-sdk-core` crate — the stable facade between the runtime and all bindings.

### Deliverables

```
crates/edgecrab-sdk-core/
  Cargo.toml
  src/
    lib.rs           # Re-exports
    agent.rs         # SdkAgent — wraps Agent with stable API
    config.rs        # SdkConfig — wraps AppConfig
    types.rs         # SDK-specific types (StreamEvent enum, Result, Error)
    tools.rs         # SdkToolRegistry — stable tool registration
    session.rs       # SdkSession — wraps SessionDb
    catalog.rs       # SdkModelCatalog — wraps ModelCatalog
    error.rs         # SdkError hierarchy
    convert.rs       # From/Into conversions between SDK and internal types
```

### Tasks

- [ ] Create `crates/edgecrab-sdk-core/Cargo.toml` with deps on edgecrab-core, tools, state, security
- [ ] Define `SdkAgent` struct wrapping `Agent` with stable public methods
- [ ] Define `SdkConfig` with builder pattern for all configuration
- [ ] Define `SdkError` enum mirroring `AgentError` + `ToolError` variants
- [ ] Define `StreamEvent` enum (Token, Reasoning, ToolExec, ToolResult, Done, Error)
- [ ] Define `ConversationResult` struct with response, usage, cost, tool_errors
- [ ] Define `SdkToolRegistry` wrapping `ToolRegistry` with `register()` method
- [ ] Implement `From<AgentError> for SdkError` and other conversions
- [ ] Write unit tests for all type conversions
- [ ] Write integration test: create agent → chat → verify result
- [ ] Write integration test: create agent → stream → collect events
- [ ] Write integration test: register custom tool → chat → tool executes
- [ ] Add `edgecrab-sdk-core` to workspace `Cargo.toml`

### Exit Criteria
- `cargo test -p edgecrab-sdk-core` passes
- All public types have `#[doc]` comments
- `cargo doc -p edgecrab-sdk-core --no-deps` renders clean

---

## Phase 1: Rust SDK (Weeks 3–5)

### Goal
Publish `edgecrab-sdk` crate to crates.io — the Rust-native SDK.

### Deliverables

```
crates/edgecrab-sdk/
  Cargo.toml
  src/
    lib.rs           # Public API re-exports
    prelude.rs       # use edgecrab_sdk::prelude::*;
    macros.rs        # #[edgecrab::tool] proc macro (separate crate)

crates/edgecrab-sdk-macros/
  Cargo.toml
  src/
    lib.rs           # Proc macro for @tool decorator
```

### Tasks

- [ ] Create `crates/edgecrab-sdk/Cargo.toml` re-exporting edgecrab-sdk-core
- [ ] Define `prelude` module with common imports
- [ ] Create `edgecrab-sdk-macros` proc macro crate
- [ ] Implement `#[edgecrab::tool]` macro that generates ToolHandler impl
- [ ] Write `examples/hello.rs` — minimal agent
- [ ] Write `examples/streaming.rs` — streaming agent
- [ ] Write `examples/custom_tool.rs` — custom tool with proc macro
- [ ] Write `examples/multi_agent.rs` — delegation pattern
- [ ] Write `examples/mcp_server.rs` — MCP integration
- [ ] Add README.md with quick start
- [ ] Add CHANGELOG.md
- [ ] Configure CI: test + clippy + doc + publish (dry-run)
- [ ] Publish v0.1.0 to crates.io

### Exit Criteria
- `cargo add edgecrab-sdk` works
- All 5 examples compile and run
- `cargo doc` shows clean API documentation
- CI pipeline green

---

## Phase 2: Python SDK (Weeks 6–9)

### Goal
Publish `edgecrab` Python package to PyPI with native bindings.

### Deliverables

```
sdks/python/
  Cargo.toml          # PyO3 crate
  pyproject.toml      # maturin build config
  src/
    lib.rs            # PyO3 module definition
    agent.rs          # PyAgent wrapping SdkAgent
    tools.rs          # @Tool decorator, PyToolHandler
    types.rs          # Python type conversions
    stream.rs         # AsyncIterator for StreamEvent
    config.rs         # PyConfig wrapping SdkConfig
    session.rs        # PySessionDb wrapping SdkSession
    catalog.rs        # PyModelCatalog
    error.rs          # Python exception classes
  python/
    edgecrab/
      __init__.py     # Top-level exports
      _native.pyi     # Type stubs for native module
      py.typed        # PEP 561 marker
```

### Tasks

- [ ] Create `sdks/python/Cargo.toml` with PyO3 deps
- [ ] Create `pyproject.toml` with maturin config
- [ ] Implement `PyAgent` class with `chat()`, `chat_sync()`, `stream()`, `run()`
- [ ] Implement `@Tool` decorator with schema inference from type hints
- [ ] Implement `StreamEvent` as Python enum (token, reasoning, tool_exec, etc.)
- [ ] Implement `ConversationResult` as Python dataclass
- [ ] Implement `PyConfig` with `load()` and `load_from()`
- [ ] Implement `PySessionDb` with `search_sessions()`, `list_sessions()`
- [ ] Implement `PyModelCatalog` with `provider_ids()`, `models_for()`
- [ ] Implement Python exception hierarchy (AgentError, ToolError, etc.)
- [ ] Implement async iterator for `agent.stream()`
- [ ] Implement sync wrapper (`chat_sync`) using `tokio::runtime::Runtime`
- [ ] Generate `.pyi` type stubs
- [ ] Write pytest suite: test_agent.py, test_tools.py, test_streaming.py
- [ ] Write `examples/hello.py`
- [ ] Write `examples/streaming.py`
- [ ] Write `examples/custom_tools.py`
- [ ] Write `examples/cost_tracking.py`
- [ ] Write `examples/mcp_integration.py`
- [ ] Configure maturin CI for wheel building (linux x86+arm, macOS arm+x86, Windows)
- [ ] Test wheel install on clean Python 3.10, 3.11, 3.12, 3.13
- [ ] Add README.md with quick start
- [ ] Publish v0.1.0 to PyPI

### Exit Criteria
- `pip install edgecrab` works on all target platforms
- All examples run
- Type stubs provide full autocomplete in VS Code
- pytest suite passes
- Wheel size < 35MB

---

## Phase 3: Node.js SDK (Weeks 10–12)

### Goal
Publish `edgecrab` npm package with native bindings.

### Deliverables

```
sdks/node-v2/
  Cargo.toml          # napi-rs crate
  package.json        # npm config
  src/
    lib.rs            # napi-rs module definition
    agent.rs          # JsAgent wrapping SdkAgent
    tools.rs          # Tool.create() factory
    types.rs          # JS/TS type conversions
    stream.rs         # AsyncIterator for StreamEvent
    config.rs         # JsConfig
    session.rs        # JsSessionDb
    catalog.rs        # JsModelCatalog
    error.rs          # JS error classes
  index.d.ts          # TypeScript declarations
  index.js            # CommonJS entry
  index.mjs           # ESM entry
```

### Tasks

- [ ] Create `sdks/node-v2/Cargo.toml` with napi-rs deps
- [ ] Create `package.json` with napi-rs build config
- [ ] Implement `Agent` class with `chat()`, `stream()`, `run()`
- [ ] Implement `Tool.create()` factory function
- [ ] Implement `StreamEvent` as TypeScript discriminated union
- [ ] Implement `ConversationResult` interface
- [ ] Implement `Config` class with `load()` and `loadFrom()`
- [ ] Implement `SessionDb` class with `searchSessions()`, `listSessions()`
- [ ] Implement `ModelCatalog` class
- [ ] Implement Error classes (AgentError, ToolError, etc.)
- [ ] Implement async iterator for `agent.stream()`
- [ ] Generate TypeScript declarations (`index.d.ts`)
- [ ] Write test suite (vitest/jest): agent, tools, streaming
- [ ] Write `examples/hello.ts`
- [ ] Write `examples/streaming.ts`
- [ ] Write `examples/custom-tools.ts`
- [ ] Write `examples/cost-tracking.ts`
- [ ] Configure napi-rs CI for platform builds
- [ ] Test npm install on Node 18, 20, 22
- [ ] Add README.md with quick start
- [ ] Publish v0.1.0 to npm

### Exit Criteria
- `npm install edgecrab` works on all target platforms
- All examples run
- TypeScript types provide full autocomplete
- Test suite passes
- Package size < 35MB

---

## Phase 4: Polish (Weeks 13–14)

### Goal
Documentation, examples, benchmarks, and CI hardening.

### Tasks

- [ ] Write comprehensive documentation site (mdbook or similar)
- [ ] Write migration guide from edgecrab-sdk v1 (HTTP client)
- [ ] Write migration guide from competing SDKs (Claude, OpenAI, Pydantic AI)
- [ ] Write cookbook with 10 real-world recipes
- [ ] Run benchmarks: latency, memory, binary size vs competitors
- [ ] Set up CI matrix for all platforms + Python/Node versions
- [ ] Set up automated PyPI + npm + crates.io publishing
- [ ] Write CONTRIBUTING.md for SDK development
- [ ] Security audit of FFI boundary
- [ ] Load testing with concurrent agents
- [ ] Final review of all public API surfaces
- [ ] Tag v0.1.0 release across all packages

### Exit Criteria
- All 3 SDKs published and installable
- Documentation site live
- CI green on all platforms
- No known P0 bugs

---

## Dependency Graph

```
+--------------------------------------------------------------------+
|                    Build Dependency Graph                            |
+--------------------------------------------------------------------+
|                                                                    |
|  Week 1-2:                                                         |
|  edgecrab-sdk-core ─────────────────────────────────┐              |
|       |                                              |              |
|  Week 3-5:                                           |              |
|  edgecrab-sdk (Rust) ──────┐                         |              |
|  edgecrab-sdk-macros ──────┤                         |              |
|       |                    |                         |              |
|  Week 6-9:                 |                         |              |
|  sdks/python ───────────┤ (depends on sdk-core)   |              |
|       |                    |                         |              |
|  Week 10-12:               |                         |              |
|  sdks/node-v2 ─────────────┘ (depends on sdk-core)   |              |
|       |                                              |              |
|  Week 13-14:                                         |              |
|  Docs + CI + Release ───────────────────────────────┘              |
+--------------------------------------------------------------------+
|                                                                    |
|  PARALLEL TRACKS (can start in Phase 1):                           |
|  - HTTP fallback client (edgecrab-client / @edgecrab/client)       |
|  - Documentation site scaffolding                                  |
|  - CI pipeline for wheel/npm builds                                |
+--------------------------------------------------------------------+
```

---

## Risk Mitigation

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| PyO3 manylinux build fails | HIGH | BLOCKS Phase 2 | Start CI early in Phase 0; have Docker build containers ready |
| napi-rs Windows build issues | MEDIUM | Delays Phase 3 | Windows is lowest priority; ship Linux+macOS first |
| Binary size > 50MB | MEDIUM | Adoption friction | Profile with `cargo bloat`; strip debug symbols; consider feature gates |
| Tokio runtime conflicts in Python | LOW | Breaks sync wrappers | Test with asyncio, uvloop, and trio; document limitations |
| API design wrong after launch | HIGH | Technical debt | Ship as v0.1.0; collect feedback for 4 weeks before v1.0 |
| Model catalog changes break SDK | LOW | Maintenance burden | Catalog is runtime data, not compiled into SDK; loaded at init |

---

## Success Metrics

| Metric | Target (3 months post-launch) |
|--------|-------------------------------|
| PyPI downloads/month | 5,000+ |
| npm downloads/month | 3,000+ |
| crates.io downloads/month | 1,000+ |
| GitHub stars | 500+ |
| Open issues | < 30 |
| Time to first agent (measured) | < 30 seconds |
| API parity across languages | 100% |

---

## Brutal Honest Assessment

### Strengths of This Plan
- Phased delivery — each phase is independently valuable
- Foundation-first approach — sdk-core crate prevents rework
- Explicit exit criteria — no ambiguity about "done"
- Risk mitigation table is actionable

### Weaknesses
- **14 weeks is optimistic** — assumes one experienced Rust developer full-time
- **No user research phase** — shipping API based on assumptions, not developer interviews
- **Python wheel building is consistently the hardest part** and gets only 4 weeks
- **No beta program** — going straight from internal testing to public release
- **Missing: HTTP fallback client timeline** — mentioned in ADR but not scheduled

### Improvements Made After Assessment
- Added parallel tracks note (HTTP client, docs, CI can start early)
- Added explicit Python/Node version testing requirements
- Added success metrics with concrete targets
- Noted v0.1.0 as explicit beta — expect breaking changes before v1.0
- Added security audit task in Phase 4
