# EdgeCrab SDK — Roadmap Status

> Generated from audit of real codebase vs spec suite (13 documents).
> Last updated: 2026-04-16
> Cross-ref: [12-CLI-SDK-PARITY.md](12-CLI-SDK-PARITY.md) for full gap analysis

---

## Phase 0: Foundation — `edgecrab-sdk-core` crate

| # | Task | Status | Notes |
|---|------|--------|-------|
| 0.1 | Create `crates/edgecrab-sdk-core/Cargo.toml` | [x] | Deps: edgecrab-core, tools, state, security, types |
| 0.2 | Define `SdkAgent` wrapping `Agent` with stable public API | [x] | 24 public methods |
| 0.3 | Define `SdkConfig` with builder pattern | [x] | load/load_from/load_profile/save/set_* (14 methods) |
| 0.4 | Define `SdkError` enum mirroring AgentError + ToolError | [x] | 6 variants |
| 0.5 | Define `StreamEvent` re-export (21 variants already in agent.rs) | [x] | Re-exported from edgecrab-core |
| 0.6 | Define `ConversationResult` re-export | [x] | Re-exported |
| 0.7 | Define `SdkToolRegistry` wrapping ToolRegistry | [x] | 7 methods |
| 0.8 | Implement From/Into conversions | [x] | SdkError ↔ AgentError, SdkConfig ↔ AppConfig |
| 0.9 | Write unit tests for type conversions | [x] | 13 unit tests |
| 0.10 | Write integration test: agent chat | [x] | Gated by EDGECRAB_E2E |
| 0.11 | Write integration test: streaming | [x] | Gated by EDGECRAB_E2E |
| 0.12 | Write integration test: custom tool | [x] | |
| 0.13 | Add to workspace Cargo.toml | [x] | |
| 0.14 | MemoryManager with security scanning | [x] | 7 unit tests, §-delimited entries |
| 0.15 | SdkModelCatalog (6 methods) | [x] | provider_ids, models_for, flat_catalog, context_window, pricing, default_model_for |
| 0.16 | SdkSession (5 methods) | [x] | open, list, search, get_messages, delete |

---

## Phase 1: Rust SDK — `edgecrab-sdk` crate

| # | Task | Status | Notes |
|---|------|--------|-------|
| 1.1 | Create `crates/edgecrab-sdk/Cargo.toml` | [x] | Re-exports edgecrab-sdk-core |
| 1.2 | Define `prelude` module | [x] | Agent, Tool, StreamEvent, ConversationResult, etc. |
| 1.3 | Create `edgecrab-sdk-macros` proc macro crate | [x] | `#[edgecrab_tool]` attribute macro |
| 1.4 | Implement `#[edgecrab::tool]` macro | [x] | Generates ToolHandler impl from fn signature |
| 1.5 | Write `examples/hello.rs` | [x] | basic_usage.rs |
| 1.6 | Write `examples/streaming.rs` | [ ] | |
| 1.7 | Write `examples/custom_tool.rs` | [ ] | |
| 1.8 | Add README.md | [x] | |

---

## Phase 2: Python SDK — `sdks/python/` (PyO3)

| # | Task | Status | Notes |
|---|------|--------|-------|
| 2.1 | Create `sdks/python/Cargo.toml` with PyO3 | [x] | |
| 2.2 | Create `pyproject.toml` with maturin | [x] | v0.6.0 |
| 2.3 | Implement `PyAgent` class | [x] | All core methods |
| 2.4 | Implement `@Tool` decorator | [ ] | Schema inference from type hints |
| 2.5 | Implement `StreamEvent` as Python enum | [x] | Via dict serialization |
| 2.6 | Implement `ConversationResult` dataclass | [x] | Via dict |
| 2.7 | Implement `PyConfig` with load()/load_from()/load_profile() | [x] | |
| 2.8 | Implement Python exception hierarchy | [x] | Via PyErr |
| 2.9 | Implement async iterator for stream() | [x] | AsyncAgent via asyncio.to_thread |
| 2.10 | Implement sync wrapper (chat_sync) | [x] | tokio::runtime::Runtime |
| 2.11 | Generate `.pyi` type stubs | [x] | |
| 2.12 | Write pytest suite | [x] | 12 passed, 6 skipped |
| 2.13 | Write examples | [x] | basic_usage.py |

---

## Phase 3: Node.js SDK — `sdks/nodejs-native/` (napi-rs)

| # | Task | Status | Notes |
|---|------|--------|-------|
| 3.1 | Create napi-rs crate | [x] | |
| 3.2 | Implement Agent class | [x] | All core methods + JS wrapper |
| 3.3 | Implement TypeScript types | [x] | index.d.ts |
| 3.4 | Write test suite | [x] | 10 passed, 4 skipped |

---

## Phase 4: Polish

| # | Task | Status | Notes |
|---|------|--------|-------|
| 4.1 | Documentation site | [x] | Astro docs (6 pages) |
| 4.2 | Migration guide | [ ] | From v1 HTTP SDK |
| 4.3 | Benchmarks | [ ] | |
| 4.4 | CI matrix | [x] | ci.yml, release-python.yml, release-node.yml |

---

## Phase 5: Binding Parity (NEW — from 12-CLI-SDK-PARITY.md)

Expose existing Rust SDK core methods to Python and Node.js bindings.

| # | Task | Status | Code Evidence |
|---|------|--------|---------------|
| 5.1 | Config.save() in Python/Node | [ ] | `SdkConfig::save()` at config.rs:66 |
| 5.2 | Config.set_default_model() in Python/Node | [ ] | `SdkConfig::set_default_model()` at config.rs:78 |
| 5.3 | Config.set_max_iterations() in Python/Node | [ ] | `SdkConfig::set_max_iterations()` at config.rs:88 |
| 5.4 | Config.set_temperature() in Python/Node | [ ] | `SdkConfig::set_temperature()` at config.rs:98 |
| 5.5 | ModelCatalog.pricing() in Python/Node | [ ] | `SdkModelCatalog::pricing()` at types.rs:48 |
| 5.6 | ModelCatalog.flat_catalog() in Python/Node | [ ] | `SdkModelCatalog::flat_catalog()` at types.rs:37 |
| 5.7 | ModelCatalog.default_model_for() in Python/Node | [ ] | `SdkModelCatalog::default_model_for()` at types.rs:53 |
| 5.8 | Session class in Python/Node | [ ] | `SdkSession` at session.rs (5 methods) |
| 5.9 | Agent.chat_in_cwd() in Python | [ ] | `SdkAgent::chat_in_cwd()` at agent.rs |
| 5.10 | Agent.session_snapshot in Python/Node | [ ] | `SdkAgent::session_snapshot()` at agent.rs |

---

## Phase 6: New SDK Core Capabilities (NEW)

Add capabilities from edgecrab-core that don't yet have SDK wrappers.

| # | Task | Status | Code Evidence |
|---|------|--------|---------------|
| 6.1 | Agent.compress() — manual compression trigger | [ ] | `Agent::force_compress()` at edgecrab-core/agent.rs:1049 |
| 6.2 | Approval workflow callback | [ ] | `StreamEvent::Approval` with `oneshot::Sender<ApprovalChoice>` |
| 6.3 | Session.prune() — prune old sessions | [ ] | `SessionDb::prune_sessions()` in edgecrab-state |
| 6.4 | Session.rename() — rename sessions | [ ] | `SessionDb::update_session_title()` in edgecrab-state |
| 6.5 | HealthCheck.run() — structured diagnostics | [ ] | `doctor.rs` checks in edgecrab-cli |
| 6.6 | ModelCatalog.estimate_cost() | [ ] | Trivial wrapper over pricing() |
| 6.7 | Context pressure documentation | [ ] | `StreamEvent::ContextPressure` already exists |

---

## Phase 7: Differentiation Features (NEW)

Competitive advantage features not in any competitor SDK.

| # | Task | Status | Notes |
|---|------|--------|-------|
| 7.1 | Agent.batch(messages) — parallel multi-prompt | [ ] | No edgecrab equivalent yet |
| 7.2 | Agent.clone_with(overrides) — config variation | [ ] | Like fork but clean slate |
| 7.3 | Agent.set_model() — hot-swap model | [ ] | Agent supports this internally |
| 7.4 | Agent.branch(name) — named fork | [ ] | Persist branch name in session |
| 7.5 | Retry with backoff on RateLimitedError | [ ] | Built into SDK error handling |

---

## Audit Findings (Real Code vs Spec) — Updated 2026-04-16

### CLI/TUI Surface: 46 slash commands + 30 subcommands + 100+ config keys
### SDK Surface: ~20% coverage → target ~60% after Phase 5-6

See [12-CLI-SDK-PARITY.md](12-CLI-SDK-PARITY.md) for the complete gap matrix.

### What the spec assumes vs what exists:

| Spec Assumption | Reality | Status |
|----------------|---------|--------|
| "SdkAgent wraps Agent" | ✅ 24 public methods | **Done** |
| "StreamEvent has 21 variants" | ✅ All 21 re-exported | **Done** |
| "Provider created from model string" | ✅ `create_provider_for_model()` | **Done** |
| "ToolRegistry.register()" | ✅ `register_dynamic(Box<dyn ToolHandler>)` | **Done** |
| "AppConfig has save()" | ✅ `save()` and `save_to()` exist | **Done** |
| "MemoryManager with security" | ✅ 7 unit tests, check_memory_content() | **Done** |
| "Config.save() in Python/Node" | ❌ Rust exists, bindings missing | **Phase 5.1** |
| "ModelCatalog full (6 methods)" | ✅ Rust has all 6 (pricing, not pricing_for); Python/Node have 3/6 | **Phase 5.5-5.7** |
| "Session standalone access" | ✅ Rust SdkSession exists; not in Python/Node | **Phase 5.8** |
| "Agent.compress()" | ❌ force_compress() in core, not in SDK | **Phase 6.1** |
| "Approval workflow" | ❌ StreamEvent::Approval exists; no callback bridge | **Phase 6.2** |
| "HealthCheck.run()" | ❌ doctor.rs logic exists; not in SDK core | **Phase 6.5** |

### Key architectural decisions:

1. **Interactive StreamEvent variants (Clarify/Approval/SecretRequest) contain `oneshot::Sender`** — NOT serializable, NOT Clone. SDK must bridge these for interactive callbacks.

2. **Agent fields are behind RwLock** — the Agent is designed for shared concurrent access. The SDK facade uses `&self` methods that acquire locks internally.

3. **Provider creation requires parsing "provider/model" strings** — `create_provider_for_model()` handles this. SDK wraps it as `Agent::new("anthropic/claude-sonnet-4")`.

4. **ToolRegistry uses inventory for compile-time tools** — SDK users can't use `inventory::submit!` from outside the crate. They use `register_dynamic()`. The proc macro generates code that calls `register_dynamic()`.

1. **Interactive StreamEvent variants (Clarify/Approval/SecretRequest) contain `oneshot::Sender`** — these are NOT serializable and NOT Clone. The SDK must handle this: either expose them for interactive UIs or filter them out for simple consumers.

2. **Agent fields are behind RwLock** — the Agent is designed for shared concurrent access. The SDK facade should use `&self` methods that acquire locks internally (which Agent already does).

3. **Provider creation requires parsing "provider/model" strings** — `create_provider_for_model()` in edgecrab-tools handles this. The SDK should expose a simple `Agent::new("anthropic/claude-sonnet-4")` that does provider resolution automatically.

4. **ToolRegistry uses inventory for compile-time tools** — SDK users can't use `inventory::submit!` from outside the crate. They must use `register_dynamic()`. The proc macro should generate code that calls `register_dynamic()`.
