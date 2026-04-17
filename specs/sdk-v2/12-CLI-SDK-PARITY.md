# EdgeCrab SDK — CLI/SDK Parity Analysis

> **Cross-references:** [SPEC](02-SPEC.md) | [IMPL](06-IMPLEMENTATION.md) | [ROADMAP](ROADMAP-STATUS.md) | [ASSESSMENT](ASSESSMENT.md)
> **Date:** 2026-04-16
> **Methodology:** Every claim verified against code. Code is law.

---

## 1. Motivation

The EdgeCrab CLI/TUI exposes **46 slash commands**, **30+ subcommands**, and **100+ configuration keys**. The SDK currently surfaces approximately 20% of the underlying capabilities. This document identifies every feature gap, applies First Principles / DRY / SOLID analysis to determine which gaps add value to the SDK, and produces an actionable design for closing them.

### 1.1 Design Filter (What Belongs in an SDK?)

**First Principles:** SDK users are developers embedding EdgeCrab in their applications. They need programmatic access to agent capabilities — not TUI rendering, system administration, or device-specific features.

**SOLID applied:**
- **S** — Each SDK module owns one concern (Agent, Config, Session, Catalog, Tools)
- **O** — Extensible via ToolHandler trait, not by modifying SDK core
- **I** — Don't force users to import things they don't need; keep `Agent` simple
- **D** — Depend on abstractions (`ToolHandler`, `SdkConfig`), not on CLI internals

**DRY:** Don't duplicate CLI command logic — expose the underlying _capabilities_ that CLI commands call.

---

## 2. Complete Feature Matrix

### 2.1 Features Already in SDK

| Feature | Rust Core | Python | Node.js | WASM | Code Evidence |
|---------|-----------|--------|---------|------|---------------|
| Agent chat/stream/run | ✅ | ✅ | ✅ | ✅ | `SdkAgent::chat()`, `stream()`, `run()` |
| Agent run_conversation | ✅ | ✅ | ✅ | ❌ | `SdkAgent::run_conversation()` |
| Agent fork/interrupt | ✅ | ✅ | ✅ | ✅/❌ | `SdkAgent::fork()`, `interrupt()` |
| Session new/id/history | ✅ | ✅ | ✅ | ✅/❌ | `new_session()`, `session_id()`, `messages()` |
| Session list/search/export | ✅ | ✅ | ✅ | — | `list_sessions()`, `search_sessions()`, `export()` |
| Memory read/write/remove | ✅ | ✅ | ✅ | ✅ | `MemoryManager` (7 unit tests) |
| Config load/load_from/load_profile | ✅ | ✅ | ✅ | — | `SdkConfig::load()`, `load_profile()` |
| Model catalog (partial) | ✅ | ✅ (3/6) | ✅ (3/6) | — | `provider_ids()`, `models_for_provider()`, `context_window()` |
| Tool registration | ✅ | ✅ | ✅ | ✅ | `SdkToolRegistry`, `Tool.create()` |
| Streaming events | ✅ | ✅ | ✅ | ✅ | `StreamEvent` (21 variants) |
| Reasoning effort control | ✅ | ✅ | ✅ | — | `set_reasoning_effort()` |
| Streaming toggle | ✅ | ✅ | ✅ | — | `set_streaming()` |
| Toolset summary | ✅ | ✅ | ✅ | — | `tool_names()`, `toolset_summary()` |
| Context controls | ✅ | ✅ | ✅ | — | `skip_context_files`, `skip_memory` |
| Delegation config | ✅ | — | — | — | Via `AppConfig` |
| MCP config | ✅ | — | — | — | Via `AppConfig.mcp_servers` |
| Compression config | ✅ | — | — | — | Via `AppConfig` |

### 2.2 Features in Rust Core But NOT Exposed to Python/Node.js

These exist in `edgecrab-sdk-core` but are missing from the FFI bindings:

| Feature | Rust Core Method | Python | Node.js | Impact |
|---------|-----------------|--------|---------|--------|
| Config save | `SdkConfig::save()` | ❌ | ❌ | Can't persist config changes |
| Config set_default_model | `SdkConfig::set_default_model()` | ❌ | ❌ | Can't change model programmatically |
| Config set_max_iterations | `SdkConfig::set_max_iterations()` | ❌ | ❌ | Can't tune iteration limit |
| Config set_temperature | `SdkConfig::set_temperature()` | ❌ | ❌ | Can't tune temperature |
| Session delete | `SdkSession::delete_session()` | ❌ | ❌ | Can't clean up sessions |
| Session standalone access | `SdkSession::open()` | ❌ | ❌ | Can't query sessions without agent |
| ModelCatalog pricing | `SdkModelCatalog::pricing()` | ❌ | ❌ | Can't build model selection UIs |
| ModelCatalog flat_catalog | `SdkModelCatalog::flat_catalog()` | ❌ | ❌ | Can't enumerate all models |
| ModelCatalog default_model_for | `SdkModelCatalog::default_model_for()` | ❌ | ❌ | Can't resolve defaults |
| chat_in_cwd | `SdkAgent::chat_in_cwd()` | ❌ | ✅ | Python can't set CWD per-chat |
| session_snapshot | `SdkAgent::session_snapshot()` | ❌ | ❌ | No raw snapshot access |

### 2.3 Features in edgecrab-core But NOT in SDK Core

These exist in the main runtime but have no SDK wrapper:

| Feature | Core Method | Use Case | Value |
|---------|------------|----------|-------|
| Force compress | `Agent::force_compress()` | Manual context compression trigger | **HIGH** — long conversations |
| Session prune | `SessionDb::prune_sessions()` | Clean old sessions | **MEDIUM** — maintenance |
| Session rename | `SessionDb::update_session_title()` | Rename sessions | **LOW** — convenience |
| Session insights | *(not in SessionDb)* | Token/cost analytics | **HIGH** — dashboards |
| Health check | `doctor.rs` checks | Provider connectivity, config validity | **MEDIUM** — monitoring |
| MCP reload | `reload_mcp_connections()` | Refresh MCP server connections | **LOW** — rare |

### 2.4 Features in CLI Only — NOT Appropriate for SDK

These are TUI-specific or system-administration concerns. **Do NOT surface in SDK.**

| Feature | Reason Not in SDK |
|---------|-------------------|
| Skin/theme engine | TUI rendering concern |
| Slash command dispatch | CLI interaction pattern |
| Mouse/scroll capture | TUI input concern |
| Status bar | TUI display concern |
| Banner/branding | TUI display concern |
| Paste clipboard | TUI input concern |
| `/btw` ephemeral question | CLI conversation pattern (use `fork()` instead) |
| `/queue` prompt queueing | CLI workflow (use `chat()` in loop instead) |
| `/background` execution | CLI workflow (use async/spawn instead) |
| Gateway platform adapters | Separate process (17 adapters) |
| Gateway pairing/approval | Gateway-specific auth flow |
| Webhook management | Gateway runtime concern |
| Voice/TTS/STT | Device-specific, use tools instead |
| Browser CDP management | Specialized, available as built-in tools |
| Cron job management | Separate scheduler process |
| Profile create/delete/list | CLI admin commands (SDK has `load_profile()` — sufficient) |
| Shell completion | CLI-only |
| Uninstall | CLI-only |
| Migration (hermes) | CLI-only one-time operation |
| Log management | Use standard logging instead |
| Permissions diagnostics | macOS CLI-only |
| Auth management | CLI interactive flow |

---

## 3. Proposed SDK Additions

Applying the design filter, these are the features worth adding, grouped by phase:

### Phase A — Binding Parity (Rust core → Python/Node.js)

**Principle:** Every public method on `SdkConfig`, `SdkSession`, and `SdkModelCatalog` should be accessible from Python and Node.js. Zero-cost to implement — the Rust code already exists.

| Item | Rust Method | Python Binding | Node.js Binding | LOC Est. |
|------|------------|----------------|-----------------|----------|
| A.1 | `SdkConfig::save()` | `Config.save()` | `config.save()` | ~5 |
| A.2 | `SdkConfig::set_default_model()` | `Config.set_default_model()` | `config.setDefaultModel()` | ~5 |
| A.3 | `SdkConfig::set_max_iterations()` | `Config.set_max_iterations()` | `config.setMaxIterations()` | ~5 |
| A.4 | `SdkConfig::set_temperature()` | `Config.set_temperature()` | `config.setTemperature()` | ~5 |
| A.5 | `SdkModelCatalog::pricing()` | `ModelCatalog.pricing()` | `ModelCatalog.pricing()` | ~10 |
| A.6 | `SdkModelCatalog::flat_catalog()` | `ModelCatalog.flat_catalog()` | `ModelCatalog.flatCatalog()` | ~10 |
| A.7 | `SdkModelCatalog::default_model_for()` | `ModelCatalog.default_model_for()` | `ModelCatalog.defaultModelFor()` | ~5 |
| A.8 | `SdkSession::open()` | `Session.open()` | `Session.open()` | ~15 |
| A.9 | `SdkSession::delete_session()` | `Session.delete_session()` | `Session.deleteSession()` | ~5 |
| A.10 | `SdkSession::get_messages()` | `Session.get_messages()` | `Session.getMessages()` | ~10 |
| A.11 | `SdkAgent::chat_in_cwd()` | `Agent.chat_in_cwd()` | *(already exists)* | ~5 |
| A.12 | `SdkAgent::session_snapshot()` | `Agent.session_snapshot` | `agent.sessionSnapshot` | ~10 |

**Total Phase A: ~90 LOC of binding code. All backed by working Rust.**

### Phase B — New SDK Core Capabilities

**Principle:** Add capabilities that exist in `edgecrab-core` but not yet in `edgecrab-sdk-core`.

| Item | Description | Core Method | Design |
|------|------------|-------------|--------|
| B.1 | **Force compress** | `Agent::force_compress()` | `SdkAgent::compress()` — triggers manual context compression |
| B.2 | **Approval workflow** | `StreamEvent::Approval` + `oneshot::Sender` | Callback-based: `approval_handler: Fn(ApprovalRequest) -> ApprovalChoice` on builder |
| B.3 | **Session prune** | `SessionDb::prune_sessions()` | `SdkSession::prune(older_than_days, source?)` |
| B.4 | **Session rename** | `SessionDb::update_session_title()` | `SdkSession::rename(id, title)` |
| B.5 | **Session stats** | `SessionDb::get_session_stats()` | `SdkSession::stats() -> SessionStats` (count, total tokens, date range) |
| B.6 | **Health check** | `doctor.rs` logic | `SdkHealthCheck::run() -> Vec<HealthResult>` — config, providers, connectivity |
| B.7 | **Context pressure** | `StreamEvent::ContextPressure` | Already in StreamEvent; document and expose in Python/Node.js event types |

### Phase C — Differentiation Features

**Principle:** Features that make EdgeCrab SDK uniquely powerful vs. competitors.

| Item | Description | Design |
|------|------------|--------|
| C.1 | **Batch execution** | `SdkAgent::batch(messages) -> Vec<ConversationResult>` — parallel multi-prompt |
| C.2 | **Agent cloning** | `SdkAgent::clone_with(overrides) -> SdkAgent` — new agent with same config but different model/params |
| C.3 | **Model hot-swap** | `SdkAgent::set_model(model)` — change model mid-conversation |
| C.4 | **Conversation branching** | `SdkAgent::branch(name?) -> SdkAgent` — named fork with history copy |
| C.5 | **Cost estimation** | `SdkModelCatalog::estimate_cost(model, input_tokens, output_tokens) -> Cost` |
| C.6 | **Retry with backoff** | Built-in retry on `RateLimitedError` with exponential backoff |

---

## 4. Design Details

### 4.1 Config Mutation (Phase A)

The `SdkConfig` wrapper already has `save()`, `set_default_model()`, `set_max_iterations()`, `set_temperature()` in Rust. The bindings just need thin wrappers.

**Python:**
```python
config = Config.load()
config.set_default_model("openai/gpt-4.1")
config.set_max_iterations(120)
config.set_temperature(0.7)
config.save()  # Writes to ~/.edgecrab/config.yaml
```

**Node.js:**
```typescript
const config = Config.load();
config.setDefaultModel("openai/gpt-4.1");
config.setMaxIterations(120);
config.setTemperature(0.7);
config.save();
```

**Code evidence:** `SdkConfig` in `crates/edgecrab-sdk-core/src/config.rs` lines 66-98.

### 4.2 Session Management (Phase A + B)

Standalone `Session` class for querying session DB without an agent:

**Python:**
```python
from edgecrab import Session

db = Session.open()  # Default path
# or: db = Session.open("~/.edgecrab/sessions.db")

sessions = db.list_sessions(limit=20)
hits = db.search_sessions("auth refactor", limit=10)
messages = db.get_messages("ses_abc123")
db.delete_session("ses_abc123")
db.rename("ses_abc123", "Auth Refactor Sprint")
count = db.prune(older_than_days=90)
```

**Code evidence:** `SdkSession` in `crates/edgecrab-sdk-core/src/session.rs` already has `open()`, `list_sessions()`, `search_sessions()`, `get_messages()`, `delete_session()`. Only `prune()` and `rename()` need core additions.

### 4.3 ModelCatalog Completeness (Phase A)

**Python:**
```python
from edgecrab import ModelCatalog

catalog = ModelCatalog()

# Already exposed:
providers = catalog.provider_ids()
models = catalog.models_for_provider("anthropic")
window = catalog.context_window("anthropic", "claude-sonnet-4")

# NEW — Phase A additions:
pricing = catalog.pricing("anthropic", "claude-sonnet-4")
# -> {"input": 3.0, "output": 15.0, "cache_read": 0.3, "cache_write": 3.75}

all_models = catalog.flat_catalog()
# -> [("anthropic", "claude-sonnet-4", "Claude Sonnet 4"), ...]

default = catalog.default_model_for("anthropic")
# -> "claude-sonnet-4"
```

**Code evidence:** `SdkModelCatalog` in `crates/edgecrab-sdk-core/src/types.rs` lines 37-53.

### 4.4 Force Compress (Phase B)

**Python:**
```python
agent = Agent("anthropic/claude-sonnet-4")

# After many turns, manually trigger compression
for i in range(50):
    await agent.chat(f"Process item {i}")

await agent.compress()  # Triggers structural + LLM-based compression
```

**Implementation:** Add `compress()` to `SdkAgent` that calls `self.inner.force_compress().await`.

**Code evidence:** `Agent::force_compress()` in `crates/edgecrab-core/src/agent.rs` line 1049.

### 4.5 Approval Workflow (Phase B)

The most impactful missing feature. Without it, embedded agents can execute dangerous commands uncontrolled.

**Python:**
```python
from edgecrab import Agent, ApprovalRequest, ApprovalChoice

def my_handler(request: ApprovalRequest) -> ApprovalChoice:
    print(f"Command: {request.command}")
    print(f"Reasons: {request.reasons}")
    if "rm -rf" in request.command:
        return ApprovalChoice.DENY
    return ApprovalChoice.ONCE

agent = Agent(
    "anthropic/claude-sonnet-4",
    approval_handler=my_handler,
)
```

**Implementation complexity:** HIGH. The current `StreamEvent::Approval` uses `oneshot::Sender<ApprovalChoice>` which is not serializable and requires bridging the tokio channel to a Python/JS callback across the FFI boundary. Requires:
1. Add `approval_handler` field to `SdkAgentBuilder`
2. In `build()`, install a handler that intercepts `Approval` events
3. Bridge the `oneshot::Sender` to a blocking call to the foreign function

**Code evidence:** `StreamEvent::Approval` in `crates/edgecrab-core/src/agent.rs`, `ApprovalChoice` already exported from `edgecrab-sdk-core`.

### 4.6 Health Check (Phase B)

**Python:**
```python
from edgecrab import HealthCheck

results = HealthCheck.run()
for r in results:
    print(f"{r.name}: {'✅' if r.ok else '❌'} {r.message}")
    # "config_exists: ✅ ~/.edgecrab/config.yaml found"
    # "anthropic_key: ✅ ANTHROPIC_API_KEY set"
    # "anthropic_ping: ✅ 142ms latency"
```

**Implementation:** Extract the check logic from `crates/edgecrab-cli/src/doctor.rs` into a reusable `edgecrab-sdk-core` module. The doctor checks are currently CLI-coupled (they print Rich-style output); the SDK version returns structured data.

---

## 5. Priority Matrix

| Priority | Items | Effort | Value | Rationale |
|----------|-------|--------|-------|-----------|
| **P0** | A.1–A.12 | ~90 LOC | HIGH | Binding parity — code exists, just needs thin wrappers |
| **P1** | B.1 (compress), B.7 (context pressure) | ~30 LOC | HIGH | Production conversations need compression control |
| **P1** | B.2 (approval workflow) | ~200 LOC | CRITICAL | Security requirement for production embedding |
| **P2** | B.3–B.5 (session ops) | ~60 LOC | MEDIUM | Session lifecycle management |
| **P2** | B.6 (health check) | ~150 LOC | MEDIUM | Monitoring/observability |
| **P3** | C.1–C.6 (differentiation) | ~400 LOC | MEDIUM | Competitive advantage features |

---

## 6. What NOT to Add (Anti-Targets)

These features were explicitly evaluated and rejected:

| Feature | Rejection Reason (SOLID principle violated) |
|---------|----------------------------------------------|
| Skin/theme engine | **S** — TUI rendering is not SDK's responsibility |
| Slash command router | **S** — CLI interaction pattern, not programmatic API |
| Gateway platform adapters | **S** — Separate bounded context (17 adapters, 10K+ LOC) |
| Cron scheduler | **S** — Separate process concern; use OS cron + SDK `chat()` |
| Voice/TTS/STT | **I** — Device-specific; available as built-in tools |
| Browser CDP control | **I** — Too specialized; available as built-in tools |
| Profile CRUD | **I** — Admin operation; `load_profile()` sufficient for SDK consumers |
| Auth management | **S** — Interactive CLI flow; SDK users set env vars directly |
| Log management | **D** — Use `tracing` subscriber, don't re-implement |
| Plugin WASM/Lua runtime | **O** — Plugins are runtime concern, not SDK surface |

---

## 7. Appendix: Full CLI → SDK Mapping

| CLI Command | SDK Equivalent | Status |
|-------------|---------------|--------|
| `edgecrab chat <prompt>` | `agent.chat(prompt)` | ✅ Implemented |
| `edgecrab -q <prompt>` | `agent.chat(prompt)` | ✅ Implemented |
| `edgecrab setup` | `Config.load()` + env vars | ✅ Implemented (different UX) |
| `edgecrab doctor` | `HealthCheck.run()` | ❌ **Phase B.6** |
| `edgecrab version` | `edgecrab::VERSION` | ✅ Available as const |
| `edgecrab sessions list` | `Session.list_sessions()` | ⚠️ Rust only (Phase A.8) |
| `edgecrab sessions browse` | `Session.search_sessions()` | ⚠️ Rust only (Phase A.8) |
| `edgecrab sessions export` | `agent.export()` | ✅ Implemented |
| `edgecrab sessions delete` | `Session.delete_session()` | ⚠️ Rust only (Phase A.9) |
| `edgecrab sessions prune` | `Session.prune()` | ❌ **Phase B.3** |
| `edgecrab sessions stats` | `Session.stats()` | ❌ **Phase B.5** |
| `edgecrab model` | `agent.set_model()` | ❌ **Phase C.3** |
| `edgecrab config show` | `Config.load()` | ✅ Implemented |
| `edgecrab config set` | `Config.set_*()` | ⚠️ Rust only (Phase A.2-A.4) |
| `edgecrab tools list` | `agent.tool_names()` | ✅ Implemented |
| `edgecrab mcp list` | `Config.mcp_servers` | ✅ Via config |
| `edgecrab plugins list` | — | ❌ Not planned (anti-target) |
| `edgecrab skills list` | — | ❌ Not planned (anti-target) |
| `edgecrab profile use` | `Config.load_profile()` | ✅ Implemented |
| `edgecrab insights` | `Session.stats()` | ❌ **Phase B.5** |
| `/model` | Constructor param | ✅ Implemented |
| `/new` | `agent.new_session()` | ✅ Implemented |
| `/retry` | `agent.chat(last_message)` | ✅ User-space |
| `/undo` | — | ❌ Not planned (history mutation) |
| `/compress` | `agent.compress()` | ❌ **Phase B.1** |
| `/cost` | `result.cost` | ✅ In ConversationResult |
| `/fork` | `agent.fork()` | ✅ Implemented |
| `/stop` | `agent.interrupt()` | ✅ Implemented |
| `/memory` | `agent.memory` | ✅ Implemented |
| `/reasoning` | `agent.set_reasoning_effort()` | ✅ Implemented |
| `/stream` | `agent.set_streaming()` | ✅ Implemented |
| `/export` | `agent.export()` | ✅ Implemented |
| `/approve` | `approval_handler` callback | ❌ **Phase B.2** |
