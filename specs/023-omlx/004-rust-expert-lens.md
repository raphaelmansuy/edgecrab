# 004 — Rust Expert Lens (DRY / SOLID / Types)

**Focus:** How to land oMLX in **edgequake-llm** and **EdgeCrab** without trait soup, match-arm sprawl, or cross-crate duplication.

---

## 1. Dependency direction (hard rule)

```text
edgequake-llm  (LLMProvider, ProviderFactory, discovery)
       ▲
edgecrab-tools (create_provider_for_model)
       ▲
edgecrab-core  (catalog, discovery adapters, local_provider_policy, conversation)
       ▲
edgecrab-cli / gateway / acp
```

- **Never** put oMLX HTTP code in EdgeCrab.  
- **Never** put catalog YAML parsing in edgequake-llm.  
- Shared **names** only: canonical id string `"omlx"` and env var conventions.

---

## 2. edgequake-llm design

### 2.1 Preferred structure (compose, don’t clone)

```rust
// src/providers/omlx.rs  (sketch — implementation plan detail in 006)

/// Thin oMLX adapter: OpenAI-compatible local server (default :8000).
pub struct OmlxProvider {
    inner: OpenAICompatibleProvider,
    host: String,
}

impl OmlxProvider {
    pub fn from_env() -> Result<Self> { /* OMLX_HOST, OMLX_MODEL, OMLX_API_KEY, timeout */ }
    pub fn builder() -> OmlxProviderBuilder { … }
    pub async fn health_check(&self) -> Result<()> { /* GET /v1/models or /health if any */ }
    pub async fn list_models(&self) -> Result<Vec<String>> { /* GET /v1/models */ }
}

#[async_trait]
impl LLMProvider for OmlxProvider {
    fn name(&self) -> &str { "omlx" }
    // delegate chat / chat_with_tools / stream to inner
}

// Optional:
#[async_trait]
impl EmbeddingProvider for OmlxProvider { /* delegate if configured */ }
```

**Explicitly omit (unless product later needs):** LM Studio’s `lms load` CLI auto-load and native `/api/v1/chat` reasoning split — oMLX is not LM Studio.

### 2.2 Registration surfaces (complete set)

| File | Change |
|------|--------|
| `src/providers/mod.rs` | `pub mod omlx;` re-export |
| `src/lib.rs` | `pub use providers::omlx::OmlxProvider;` |
| `src/factory.rs` | `ProviderType::Omlx`, `from_str`, `all()`, `canonical_id`, create paths |
| `src/provider_catalog.rs` | `ProviderDescriptor { id: "omlx", aliases, features: CHAT_EMBED_DISCOVERY, … }` |
| `src/model_config.rs` | `ProviderType` / config enum if separate from factory |
| `src/discovery/providers/omlx.rs` | Dynamic discovery |
| `src/discovery/providers/mod.rs` | register |
| `docs/providers.md` | feature table row |
| `tests/e2e_omlx_*.rs` | gated live tests |

### 2.3 Factory match arms — keep mechanical

```rust
"omlx" | "o-mlx" | "o_mlx" => Some(Self::Omlx),
// …
Self::Omlx => "omlx",
// …
ProviderType::Omlx => Self::create_omlx[_with_model](…),
```

Do **not** special-case oMLX in application attribution beyond local passthrough (same family as Ollama/LM Studio as appropriate).

### 2.4 Env contract (single table)

| Variable | Default | Notes |
|----------|---------|-------|
| `OMLX_HOST` | `http://127.0.0.1:8000` | Also accept `OMLX_BASE_URL`; strip trailing `/v1` when composing |
| `OMLX_MODEL` | `default` or empty | Empty → server default / first model if discoverable |
| `OMLX_API_KEY` | unset | `Authorization: Bearer` when set |
| `OMLX_TIMEOUT_SECONDS` | `600` | Local inference |
| `OMLX_EMBEDDING_MODEL` | optional | P1 |

Normalize host:

```text
http://127.0.0.1:8000     → base for /v1/...
http://127.0.0.1:8000/v1  → strip /v1 before join
```

(Same helper as LM Studio — **extract shared `normalize_openai_base_url` if not already**.)

---

## 3. EdgeCrab design

### 3.1 Local family DRY refactor (do this once)

**Problem:** `matches!(name, "lmstudio" | "ollama" | …)` appears in:

- `local_provider_policy.rs`
- `mutation_turn_policy.rs`
- `registry.rs`
- `tool_progress_tail.rs`
- CLI filters
- pricing

**Solution (recommended):**

```rust
// edgecrab-core/src/local_provider_policy.rs  (or edgecrab-types if truly shared)

/// Canonical local inference provider ids (runtime names).
pub const LOCAL_INFERENCE_PROVIDERS: &[&str] = &[
    "ollama",
    "lmstudio",
    "omlx",
    "vllm",
    "llamacpp",
];

pub fn is_local_inference_provider(provider_name: &str) -> bool {
    LOCAL_INFERENCE_PROVIDERS
        .iter()
        .any(|&p| p == provider_name)
}
```

Then:

- `mutation_turn_policy` / registry call **this** function (or re-export), not a second match.  
- Provider-specific **copy** (stall messages) stays as `match` for strings only.  
- Timeouts: `match` with fallback `_ if is_local… => DEFAULT`.

**Alternative (stricter SOLID):** small `LocalProviderKind` enum — higher churn; not required for P0 if const slice + function is consistent.

### 3.2 Catalog

```yaml
  omlx:
    label: "oMLX (local MLX)"
    default_model: "default"
    models:
      - model: "default"
        context: 128000
        tier: standard
```

Aliases in `model_catalog.rs` normalize:

```rust
"o-mlx" | "o_mlx" => "omlx".to_string(),
```

### 3.3 Discovery adapter

Mirror `LMStudioDiscovery`:

```rust
impl ModelDiscoveryAdapter for OmlxDiscovery {
    fn canonical_name(&self) -> &'static str { "omlx" }
    fn aliases(&self) -> &'static [&'static str] { &["o-mlx", "o_mlx"] }
    fn cache_ttl(&self) -> Duration { Duration::from_secs(LOCAL_CACHE_TTL_SECS) }
    async fn fetch_models(&self) -> anyhow::Result<Vec<String>> {
        let base = std::env::var("OMLX_HOST")
            .or_else(|_| std::env::var("OMLX_BASE_URL"))
            .unwrap_or_else(|_| "http://127.0.0.1:8000".into());
        fetch_openai_compatible_models(&base, api_key_opt).await
    }
}
```

Register in discovery adapter list / inventory.

### 3.4 Identity parsing for multi-segment models

```text
omlx/mlx-community/Qwen3-8B-4bit     → provider=omlx, model=mlx-community/Qwen3-8B-4bit
omlx/qwen3-8b:thinking               → provider=omlx, model=qwen3-8b:thinking
```

Reuse lenient resolve; add tests **specifically** for `:` profiles and multi-`/`.

### 3.5 Avoid unwrap / panics

Follow AGENTS.md: factory returns `Result`; discovery returns empty on down; no byte-sliced model ids.

---

## 4. Type & trait checklist

| Trait / type | oMLX |
|--------------|------|
| `LLMProvider` | Required |
| `EmbeddingProvider` | P1 optional |
| `ProviderType` (factory) | Required |
| `ProviderConfig` / catalog model config | Required if other locals use it |
| `ModelDiscoveryProvider` (eq-llm) | Required |
| `ModelDiscoveryAdapter` (edgecrab) | Required |
| Image gen traits | **Out of scope** |

---

## 5. Error mapping

| HTTP / condition | `LlmError` variant | User suffix |
|------------------|--------------------|-------------|
| Connect refused | `NetworkError` | Start oMLX / check host |
| Timeout | `Timeout` | local stall notice (omlx copy) |
| 401 | Auth error | Set `OMLX_API_KEY` |
| 404 model | Invalid model / not found | List via `/models omlx` |
| 5xx | Provider error | Server log `~/.omlx/logs` |

Map through existing `OpenAICompatibleProvider` paths; only add oMLX-specific messages at EdgeCrab policy layer.

---

## 6. Test architecture (Rust)

| Layer | Location | Style |
|-------|----------|-------|
| Unit: factory from_str | edgequake-llm | no network |
| Unit: host normalize | edgequake-llm | table-driven |
| Unit: local policy includes omlx | edgecrab-core | no network |
| Unit: catalog resolve | edgecrab-core | lenient multi-segment |
| Unit: discovery parse payload | edgequake-llm / edgecrab | fixture JSON |
| Integration: create provider | edgequake-llm | mockittp or wiremock |
| E2E live | both | `#[ignore]` + env |

**Flake rules:** never hit real oMLX in default CI; live tests opt-in.

---

## 7. Versioning / release coupling

| Step | Repo |
|------|------|
| 1 | Land oMLX in edgequake-llm; bump version |
| 2 | EdgeCrab Cargo.toml depends on new edgequake-llm |
| 3 | EdgeCrab catalog + policy + CLI |

Local path during development: user may use `[patch]` or path dep to `/Users/raphaelmansuy/Github/03-working/edgequake-llm` — do not leave permanent path patches in published trees (CHANGELOG already notes prior removal of local path patch).

---

## 8. Rust Expert anti-goals

- God-object `LocalProvider` that switches on enum for all HTTP differences  
- `macro_rules!` generating three near-identical providers without shared inner  
- `unsafe` for nothing  
- Blocking `reqwest` in async traits  
- Holding `Mutex<Client>` unnecessarily — share `reqwest::Client` like peers  

---

## 9. Definition of clean code complete

- [ ] `OmlxProvider` ≤ ~300 LOC excluding tests (thin)  
- [ ] No new hardcoded local pairs outside message copy / timeout env names  
- [ ] `cargo test -p edgequake-llm --lib` green  
- [ ] `cargo test -p edgecrab-core --lib` green  
- [ ] `cargo clippy --workspace -- -D warnings` clean in both repos after change  
