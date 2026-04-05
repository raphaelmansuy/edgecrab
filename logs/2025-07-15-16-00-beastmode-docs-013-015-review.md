# Task Log: EdgeClaw Docs 013-015 Review

## Actions
- Fixed edgequake-llm description "17+ providers" → "13 native providers" in 013
- Replaced deprecated `serde_yaml` with `serde_yml` 0.0.12 in 013
- Updated `tree-sitter` 0.24 → 0.26 in 013
- Replaced `headless_chrome` with `chromiumoxide` 0.9 (async, tokio-native) in 013
- Added missing crates: `unicode-normalization`, `rand`, `rust_decimal` in 013
- Added `serde_yaml` and `headless_chrome` to rejected crates table in 013
- Fixed "17+ providers" → "13 native providers" in 014
- Added WAL write contention tuning details (jitter retry, FTS5 triggers) to 014
- Added command normalization details (NFKC, ~30 patterns) to 014
- Fixed `memories/` directory path (not MEMORY.md) in 014
- Updated browser crate to `chromiumoxide` 0.9 in 014
- Expanded Phase 4.2 Environments with Atropos, 11 parsers, PersistentShell, benchmarks in 014
- Updated browser section to reflect `chromiumoxide` decision in 015
- Added Roadblock 17: Tool call parser porting (11 model-specific parsers) in 015
- Added Roadblock 18: PersistentShell file-based IPC in 015
- Updated risk matrix with new roadblocks in 015
- Fixed section ordering (17/18 before risk matrix) in 015

## Decisions
- `serde_yml` chosen over `serde_yaml` (deprecated) despite lower version (0.0.12)
- `chromiumoxide` chosen over `headless_chrome` (async/tokio-native, actively maintained)
- `tree-sitter` 0.26 is current (doc had 0.24)
- `chrono-tz` rejection kept (chrono built-in tz sufficient for our needs)

## Next steps
- All 15 document folders reviewed and updated
- Full OODA review cycle complete

## Lessons/insights
- Always verify crate versions against crates.io — training data versions can be stale
- serde_yaml deprecation is a significant ecosystem event to track
