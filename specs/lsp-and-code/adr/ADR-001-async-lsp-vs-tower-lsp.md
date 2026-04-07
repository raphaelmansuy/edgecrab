# ADR-001 — LSP Client Library: async-lsp vs tower-lsp vs raw JSON-RPC

**Status**: Accepted  
**Date**: 2025  
**Deciders**: EdgeCrab architecture team

---

## Context

EdgeCrab needs an LSP **client** library (not a server — it connects *to* language servers).
Three realistic choices exist:

1. `async-lsp` v0.2.3 (oxalica, MIT/Apache-2.0)
2. `tower-lsp` v0.20 (ebkalderon, MIT)
3. Custom JSON-RPC over `tokio::io` + `lsp-types`

---

## Options Evaluated

### Option A: `async-lsp`

```
                        tower Layer composition
                           ┌──────────────┐
                           │ CatchUnwind  │
                           ├──────────────┤
                           │  Tracing     │
                           ├──────────────┤
  ServerSocket ──►        │ Concurrency  │   ──► handler (EdgeCrabClientHandler)
  (send requests)         ├──────────────┤
                           │  Lifecycle   │
                           └──────────────┘
       ▲
  supports CLIENT role (ServerSocket)
  AND server role (ClientSocket)
```

Key properties:
- Works as CLIENT (sends requests to language servers) — this is what we need
- `&mut self` in handlers — state mutation is safe, no Arc<Mutex<>> fight
- Notifications processed synchronously within the loop → correct ordering guaranteed
- Custom tower `Layer`s can be added (tracing, metrics, circuit-breaker)
- 460K downloads, actively maintained

### Option B: `tower-lsp`

- Primarily designed to BUILD language servers, not to be a client
- Does not expose a `ServerSocket` or client-side API for sending arbitrary requests
- Notifications are spawned as tasks → ordering not guaranteed
- Handler trait forces `Arc<RwLock<State>>` → ergonomic pain
- **Not viable** for our client use-case

### Option C: Raw JSON-RPC

- Maximum control
- Requires building: request ID tracking, response correlation, notification routing,
  cancellation (`$/cancelRequest`), progress (`$/progress`), initialization state machine manually
- Estimated 2,000–3,000 lines of boilerplate duplicating what async-lsp already solves
- High risk of subtle correctness bugs (duplicate IDs, unhandled edge cases in LSP lifecycle)

---

## Decision

**Option A: async-lsp**

Rationale:
1. Only library with correct client-side API for sending arbitrary LSP requests
2. Tower layer model aligns with EdgeCrab's principle of composable middleware
3. `&mut self` eliminates the Arc<Mutex<>> cognitive overhead seen in tower-lsp
4. Correct notification ordering is critical for document sync (didChange before hover)
5. 460K downloads indicate real-world validation

---

## Consequences

**Positive**:
- Correct protocol implementation out of the box
- Composable error handling (CatchUnwind prevents server crashes from killing our process)
- Clean tracing integration
- Cancellation support via `$/cancelRequest` (async-lsp handles request ID lifecycle)

**Negative**:
- `async-lsp` v0.2 is relatively young (< 1M downloads) — check for API stability
- `omni-trait` feature flag required for `LanguageServer` / `LanguageClient` omnibus traits
- Must be comfortable with tower service builder pattern

**Migration path**: If async-lsp is abandoned, the `ServerSocket` / `ClientSocket` abstraction
means we can swap the underlying transport without changing tool code.

---

## Cargo addition

```toml
# workspace dependencies
async-lsp = { version = "0.2.3", features = ["omni-trait", "tokio"] }
lsp-types = { version = "0.97.0", features = ["proposed"] }
```
