
# Model Providers & Catalog (Deep Dive)

EdgeCrab supports multi-provider LLM selection via a single source-of-truth model catalog plus provider-scoped live discovery where that can be implemented safely. Static catalog data remains the fallback for pricing, defaults, and offline operation.

## Provider Enumeration

- **Anthropic**: Claude 4.6, 4.5, 3.x (reasoning, standard, fast tiers)
- **GitHub Copilot**: GPT-5.4, GPT-4.1, Gemini, etc. (Copilot API)
- **OpenAI**: GPT-4.1, GPT-4o, GPT-3.5, etc.
- **OpenRouter**: 600+ models (Hermes 3, Qwen, DeepSeek, etc.)
- **Ollama**: Local models (Llama 3, Phi-3, etc.)
- **LMStudio**: Local models (OpenAI-compatible)
- **oMLX**: Local MLX on Apple Silicon (OpenAI-compatible, default `:9050`, settings-aware)
- **MTPLX**: Local MTP speculative decode on Apple Silicon (OpenAI-compatible, settings.port / default `:8000`)
- **llama-server** (`llamacpp`): llama.cpp Metal GGUF (OpenAI-compatible, default `:8080`)
- **vLLM-MLX** (`vllm-mlx`): MLX continuous batching server (default `:8000`; may share port with MTPLX — use `/endpoint`)
- **mlx-lm** (`mlx-lm`): Official `mlx_lm.server` (default `:8080`; may share port with llama-server — use `/endpoint`)
- **Gemini**: Google Gemini 2.5/3.x
- **AWS Bedrock**: Amazon Nova, Anthropic Claude on Bedrock, Meta Llama, Mistral, Cohere, DeepSeek, and Qwen model IDs
- **xAI**: Grok 3/4
- **DeepSeek**: DeepSeek V3, R1
- **HuggingFace**: Inference API
- **Z.AI**: GLM 4.5-5

See [`model_catalog_default.yaml`](../../crates/edgecrab-core/src/model_catalog_default.yaml) for the full, up-to-date list.

## Catalog Architecture

- [`model_catalog.rs`](../../crates/edgecrab-core/src/model_catalog.rs): Loads the catalog (compiled YAML + user override).
- [`model_discovery.rs`](../../crates/edgecrab-core/src/model_discovery.rs): Provider-normalized live discovery, cache, and static fallback.
- **ProviderEntry**: Each provider has a label, default model, and list of models (with context, tier, pricing).
- **User override**: `~/.edgecrab/models.yaml` is merged on top of the default.
- **Discovery cache**: `~/.edgecrab/model_discovery_cache.json` stores per-provider discoveries with TTLs.
- **CLI setup**: [`setup.rs`](../../crates/edgecrab-cli/src/setup.rs) prompts for provider/model, checks env vars for API keys.
- **Config**: [`config.rs`](../../crates/edgecrab-core/src/config.rs) — provider/model selection, fallback config, CLI/env override.

## Provider Wiring & Routing

- [`model_router.rs`](../../crates/edgecrab-core/src/model_router.rs): Smart routing (simple vs complex messages), fallback on error, tier-based selection.
- **API mode**: Supports OpenAI-compatible, Copilot, Anthropic, Gemini, Bedrock, etc. (see `ApiMode` enum).
- **Extensibility**: Adding a new provider = add to catalog YAML, implement API mode if needed, add CLI setup prompt.

## Design Patterns & Extensibility

- **Single source of truth**: All model selection, pricing, and context window logic comes from the catalog.
- **Adapter pattern**: Live discovery is provider-specific and adapter-based, following the same first-principles direction as EdgeCode while remaining stricter about unsafe generic `/v1/models` endpoints.
- **User override**: Users can add/override models/providers in their own YAML.
- **CLI/TUI**: `/model` opens immediately from the static catalog, then refreshes live inventories in place. `/models` shows provider inventory, source, and discovery status.

## Limitations & TODOs

- **Live discovery is intentionally scoped**: EdgeCrab supports provider-specific discovery for OpenRouter, Ollama, LM Studio, oMLX, MTPLX, llama-server (`llamacpp`), vLLM-MLX, mlx-lm, Google Gemini, GitHub Copilot, and AWS Bedrock. Providers like OpenAI remain static because a raw `/v1/models` list is not a reliable chat-model inventory.
- **Base URL overrides**: `/endpoint` (aliases `/endpoints`, `/provider-url`) opens a TUI dialog to set per-provider base URLs persisted under `provider_endpoints` in `config.yaml`.
- **No per-model capability flags**: Vision, tool-calling, etc. are not yet first-class in the catalog.
- **Bedrock discovery can still be disabled**: the default build enables `bedrock-model-discovery`, but builds made with `--no-default-features` fall back to the embedded Bedrock catalog.
- **No per-user access filtering**: Static inventory is broader than any one account's entitlements. Live discovery narrows only the providers where the backend exposes a trustworthy scoped list.

## Local Mac / desktop family (023)

| Id | Default port | Env host keys | edgequake-llm |
|----|--------------|---------------|---------------|
| `ollama` | 11434 | `OLLAMA_HOST` | `OllamaProvider` |
| `lmstudio` | 1234 | `LMSTUDIO_HOST` | `LMStudioProvider` |
| `omlx` | 9050 | `OMLX_HOST` + `~/.omlx/settings.json` | `OmlxProvider` |
| `mtplx` | 8000 / settings | `MTPLX_HOST` + app settings | `MtplxProvider` |
| `llamacpp` | 8080 | `LLAMACPP_HOST`, `LLAMA_SERVER_HOST` | `LocalOpenAi` + `LLAMACPP_IDENTITY` |
| `vllm-mlx` | 8000 | `VLLM_MLX_HOST` | `LocalOpenAi` + `VLLM_MLX_IDENTITY` |
| `mlx-lm` | 8080 | `MLX_LM_HOST` | `LocalOpenAi` + `MLX_LM_IDENTITY` |

**Shared:** `local_provider_policy`, `local_prefix_cache`, `provider_endpoints`, discovery adapters, zero-cost pricing, setup/doctor.

**Tests:** `crates/edgecrab-core/tests/{omlx,mtplx}_provider_citizenship.rs`, `local_mac_providers_citizenship.rs` (optional `LLAMACPP_E2E=1`, `VLLM_MLX_E2E=1`, `MLX_LM_E2E=1`).

**Website:** [site providers/local](../../site/src/content/docs/providers/local.md), [overview](../../site/src/content/docs/providers/overview.md).

**Specs:** [specs/023-omlx/](../../specs/023-omlx/) — especially [014 landscape](../../specs/023-omlx/014-apple-silicon-local-landscape.md).

## Key Code & Docs

- [model_catalog.rs](../../crates/edgecrab-core/src/model_catalog.rs)
- [model_discovery.rs](../../crates/edgecrab-core/src/model_discovery.rs)
- [model_catalog_default.yaml](../../crates/edgecrab-core/src/model_catalog_default.yaml)
- [local_provider_policy.rs](../../crates/edgecrab-core/src/local_provider_policy.rs)
- [provider_endpoints.rs](../../crates/edgecrab-core/src/provider_endpoints.rs)
- [local_prefix_cache.rs](../../crates/edgecrab-core/src/local_prefix_cache.rs)
- [model_router.rs](../../crates/edgecrab-core/src/model_router.rs)
- [config.rs](../../crates/edgecrab-core/src/config.rs)
- [setup.rs](../../crates/edgecrab-cli/src/setup.rs)
- edgequake-llm: `providers/local_openai_common.rs`, `llamacpp.rs`, `vllm_mlx.rs`, `mlx_lm.rs`, `omlx.rs`, `mtplx.rs`

---
**TODOs:**
- Add per-model capability flags (vision, tool-calling, etc.)
- Add richer entitlement-aware filtering for Copilot/OpenRouter/Bedrock
- P2: multi-port doctor fingerprint (response headers), watchlist Maic/exo
