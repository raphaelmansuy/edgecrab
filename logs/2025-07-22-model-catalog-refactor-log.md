# Task Log: SOLID/DRY Model Catalog Refactor

## Actions
- Created `model_catalog.rs` in edgecrab-core with `ModelCatalog` singleton (OnceLock + RwLock)
- Created `model_catalog_default.yaml` with 15+ providers, 130+ models, pricing data
- Wired app.rs (model selector + /models), setup.rs (default_model), pricing.rs (PRICING_TABLE) to ModelCatalog
- Added re-exports in lib.rs for public API
- Fixed Rust 2024 edition borrow pattern in persist_model_to_config
- Added fuzzy pricing lookup (date-suffix stripping) and zero-cost provider fallback
- Committed 6 files, +1039 -143 lines

## Decisions
- Embedded YAML via `include_str!` for zero-cost default (no file I/O at startup)
- User overrides merged from `~/.edgecrab/models.yaml` (additive, not destructive)
- Kept `persist_model_to_config()` in app.rs (writes to config.yaml, not models.yaml)
- Zero-cost fallback for copilot/ollama/lmstudio instead of requiring explicit pricing entries

## Next steps
- Consider live API discovery (OpenRouter /models) as optional cache layer
- Add `edgecrab models --refresh` CLI command to fetch live model lists

## Lessons
- Rust 2024 edition forbids explicit `ref mut` in implicitly-borrowing patterns
- `LazyLock<HashMap<String, _>>` works well when keys come from runtime data (catalog YAML)
