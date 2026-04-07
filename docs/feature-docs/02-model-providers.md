
# Model Providers & Catalog (Deep Dive)

EdgeCrab supports **multi-provider LLM selection** via a single source-of-truth model catalog. All model selection, pricing, and routing is driven by the catalog, which is compiled-in but user-overrideable.

## Provider Enumeration

- **Anthropic**: Claude 4.6, 4.5, 3.x (reasoning, standard, fast tiers)
- **GitHub Copilot**: GPT-5.4, GPT-4.1, Gemini, etc. (Copilot API)
- **OpenAI**: GPT-4.1, GPT-4o, GPT-3.5, etc.
- **OpenRouter**: 600+ models (Hermes 3, Qwen, DeepSeek, etc.)
- **Ollama**: Local models (Llama 3, Phi-3, etc.)
- **LMStudio**: Local models (OpenAI-compatible)
- **Gemini**: Google Gemini 2.5/3.x
- **xAI**: Grok 3/4
- **DeepSeek**: DeepSeek V3, R1
- **HuggingFace**: Inference API
- **Z.AI**: GLM 4.5-5

See [`model_catalog_default.yaml`](../../crates/edgecrab-core/src/model_catalog_default.yaml) for the full, up-to-date list.

## Catalog Architecture

- [`model_catalog.rs`](../../crates/edgecrab-core/src/model_catalog.rs): Loads the catalog (compiled YAML, user override, future live discovery).
- **ProviderEntry**: Each provider has a label, default model, and list of models (with context, tier, pricing).
- **User override**: `~/.edgecrab/models.yaml` is merged on top of the default.
- **CLI setup**: [`setup.rs`](../../crates/edgecrab-cli/src/setup.rs) prompts for provider/model, checks env vars for API keys.
- **Config**: [`config.rs`](../../crates/edgecrab-core/src/config.rs) — provider/model selection, fallback config, CLI/env override.

## Provider Wiring & Routing

- [`model_router.rs`](../../crates/edgecrab-core/src/model_router.rs): Smart routing (simple vs complex messages), fallback on error, tier-based selection.
- **API mode**: Supports OpenAI-compatible, Copilot, Anthropic, Gemini, etc. (see `ApiMode` enum).
- **Extensibility**: Adding a new provider = add to catalog YAML, implement API mode if needed, add CLI setup prompt.

## Design Patterns & Extensibility

- **Single source of truth**: All model selection, pricing, and context window logic comes from the catalog.
- **Adapter pattern**: Each provider is mapped to an API mode; dynamic adapters (like in EdgeCode) are a TODO for live model discovery.
- **User override**: Users can add/override models/providers in their own YAML.
- **CLI/TUI**: Model selection is unified across CLI, TUI, and gateway.

## Limitations & TODOs

- **No live dynamic adapters yet**: Unlike EdgeCode, EdgeCrab does not (yet) fetch live model lists from OpenRouter/Ollama/LMStudio APIs; catalog is static at startup (user can override).
- **No per-model capability flags**: Vision, tool-calling, etc. are not yet first-class in the catalog.
- **No per-user access filtering**: All models in the catalog are shown; user-specific access is not yet detected.

## Key Code & Docs

- [model_catalog.rs](../../crates/edgecrab-core/src/model_catalog.rs)
- [model_catalog_default.yaml](../../crates/edgecrab-core/src/model_catalog_default.yaml)
- [model_router.rs](../../crates/edgecrab-core/src/model_router.rs)
- [config.rs](../../crates/edgecrab-core/src/config.rs)
- [setup.rs](../../crates/edgecrab-cli/src/setup.rs)

---
**TODOs:**
- Add dynamic adapters for live model discovery (OpenRouter, Ollama, LMStudio)
- Add per-model capability flags (vision, tool-calling, etc.)
- Add per-user access filtering for Copilot/OpenRouter
