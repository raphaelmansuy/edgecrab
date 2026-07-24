# 006 — edgequake-llm Implementation Plan

**Repo:** `/Users/raphaelmansuy/Github/03-working/edgequake-llm`  
**Principle:** Thin OpenAI-compatible wrapper; factory/catalog/discovery registration; no agent policy.

---

## 1. Why this repo first

EdgeCrab’s `create_provider_for_model` is a thin pass-through to `ProviderFactory`. Without `omlx` in edgequake-llm, EdgeCrab cannot create a provider instance. Catalog-only changes would show models that cannot run.

---

## 2. Module design (DRY)

### 2.1 Reuse

| Reuse | From |
|-------|------|
| HTTP chat/tools/stream/embed | `OpenAICompatibleProvider` |
| Error types | `LlmError` |
| Discovery trait | `ModelDiscoveryProvider` |
| Descriptor registry | `ProviderCatalog` |
| Factory pattern | `create_lmstudio` / `create_ollama` twins |

### 2.2 oMLX-specific only

| Concern | Implementation |
|---------|----------------|
| Default host `:8000` | constants |
| Env prefix `OMLX_*` | from_env / builder |
| Optional bearer | pass into OpenAICompatible config |
| Health / list_models | GET `/v1/models` |
| Display name | `"omlx"` |

### 2.3 Do **not** port from LM Studio

| LM Studio feature | Port? |
|-------------------|-------|
| `lms load` auto-load | **No** |
| Native `/api/v1/chat` reasoning split | **No** (P0) |
| `/api/v1/models` preferred over `/v1/models` | Prefer `/v1/models` first for oMLX |
| Embedding dim defaults for nomic | Only if oMLX embedding defaults known (P1) |

---

## 3. File-level task list

| Step | File | Work |
|------|------|------|
| EQL-1 | `src/providers/omlx.rs` | **New** provider + builder + tests |
| EQL-2 | `src/providers/mod.rs` | mod + pub use |
| EQL-3 | `src/lib.rs` | re-export `OmlxProvider` |
| EQL-4 | `src/factory.rs` | enum + parse + create + docs strings |
| EQL-5 | `src/provider_catalog.rs` | descriptor |
| EQL-6 | `src/model_config.rs` | if `ProviderType` / config enum lists locals |
| EQL-7 | `src/discovery/providers/omlx.rs` | **New** dynamic discovery |
| EQL-8 | `src/discovery/providers/mod.rs` | register |
| EQL-9 | `src/discovery/service.rs` or registry | ensure omlx in discovery service map |
| EQL-10 | `docs/providers.md` | table row + section |
| EQL-11 | `docs/provider-families.md` | mention under local OpenAI family |
| EQL-12 | `tests/e2e_omlx_openai_compatible.rs` | live gated |
| EQL-13 | `CHANGELOG.md` | version note |
| EQL-14 | Cargo.toml version bump | semver minor/patch per project rules |

---

## 4. Constructor API (public)

```rust
impl OmlxProvider {
    pub fn new(host: impl Into<String>, model: impl Into<String>) -> Result<Self>;
    pub fn from_env() -> Result<Self>;
    pub fn builder() -> OmlxProviderBuilder;
    pub fn with_model(self, model: impl Into<String>) -> Self;
    pub fn with_api_key(self, key: impl Into<String>) -> Self;
    pub fn with_timeout(self, timeout: Duration) -> Self;
    pub async fn health_check(&self) -> Result<()>;
    pub async fn list_models(&self) -> Result<Vec<String>>;
}
```

Builder fields: `host`, `model`, `api_key`, `timeout`, `embedding_model` (optional).

---

## 5. OpenAICompatible wiring

Suggested inner config:

```text
base_url:  {host}/v1     (after normalize)
api_key:   OMLX_API_KEY or placeholder if server allows empty
model:     OMLX_MODEL
timeout:   OMLX_TIMEOUT_SECONDS
```

Some OpenAI-compatible clients require a non-empty API key string even when ignored — follow whatever `OpenAICompatibleProvider` already does for keyless local (LM Studio pattern).

---

## 6. Discovery module

```rust
pub struct OmlxDiscovery { host: String }

impl ModelDiscoveryProvider for OmlxDiscovery {
    fn provider_id(&self) -> &str { "omlx" }
    fn discovery_strategy(&self) -> DiscoveryStrategy { Dynamic }
    async fn discover_models(&self) -> Result<Vec<DiscoveredModel>> {
        // GET {host}/v1/models → parse OpenAI list; empty if down
    }
}
```

Parse fixture tests with sample payload:

```json
{
  "object": "list",
  "data": [
    { "id": "qwen3-8b", "object": "model" },
    { "id": "qwen3-8b:thinking", "object": "model" }
  ]
}
```

---

## 7. Unit tests (must pass offline)

| Test | Assert |
|------|--------|
| `from_str_omlx_aliases` | omlx / o-mlx / o_mlx |
| `canonical_id` | `"omlx"` |
| `create_omlx_name` | provider.name() == "omlx" |
| `host_normalize_strips_v1` | base composition correct |
| `discovery_parse_profiles` | ids with `:` kept |
| `catalog_resolve_id` | ProviderCatalog::resolve_id |

---

## 8. E2E tests (opt-in)

```bash
# prerequisites: omlx serve on :8000 with a chat model
OMLX_E2E=1 cargo test -p edgequake-llm --test e2e_omlx_openai_compatible -- --ignored
```

Cases:

1. list_models non-empty  
2. chat hello  
3. chat_with_tools (if model supports)  
4. stream tokens  
5. optional embeddings  

---

## 9. SOLID review gates

| Gate | Check |
|------|-------|
| S | omlx.rs has no catalog/TUI knowledge |
| O | new provider via enum arm + descriptor, not rewriting OpenAICompatible |
| L | substitutable in multi_provider example |
| I | embeddings optional trait impl |
| D | no dependency on edgecrab crates |

---

## 10. Release handoff to EdgeCrab

After merge + publish (or path patch for local dev):

```toml
# EdgeCrab workspace — update edgequake-llm version
edgequake-llm = "X.Y.Z"
```

Document path-dep workflow for maintainers only:

```toml
# [patch.crates-io]  # local dev only
# edgequake-llm = { path = "../edgequake-llm" }
```

---

## 11. Estimate

| Scope | Effort (experienced) |
|-------|----------------------|
| Provider + factory + catalog | 0.5–1 day |
| Discovery + tests + docs | 0.5 day |
| Live e2e on Mac | 0.5 day (env-dependent) |
