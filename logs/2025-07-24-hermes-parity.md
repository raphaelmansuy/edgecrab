# Task Log — 2025-07-24 — Hermes-Agent Feature Parity

## Actions
- Added 4 missing personality presets to `config.rs`: `creative`, `teacher`, `surfer`, `uwu` (now 14 total, matching hermes-agent)
- Added `SwitchPersonality(String)` and `SwitchSkin(String)` variants to `CommandResult` enum in `commands.rs`
- Upgraded `/personality [name|clear]` command to switch presets at runtime; no-arg shows list + current overlay
- Upgraded `/skin [name]` command to switch named skins at runtime; no-arg shows available skins
- Added `session_personality: Option<String>` and `session_skin: Option<String>` fields to `App` struct
- Implemented `handle_switch_personality()`: resolves preset, appends overlay to cached system prompt via `append_to_system_prompt()`
- Implemented `handle_switch_skin()`: applies Skin colors to active Theme fields live
- Added `load_global_soul(edgecrab_home)` to `prompt_builder.rs`: loads `~/.edgecrab/SOUL.md` with injection scanning + truncation
- Added `seed_global_soul(edgecrab_home)`: auto-seeds `~/.edgecrab/SOUL.md` with default content if missing (never overwrites)
- Updated `conversation.rs`: passes global SOUL.md as `override_identity` (slot #1) to `PromptBuilder::build()`
- Added 6 hermes-parity skins to `skin_engine.rs`: `ares`, `mono`, `slate`, `poseidon`, `sisyphus`, `charizard` (now 12 total)

## Decisions
- Kept project SOUL.md walk-up from CWD as a bonus context file section (hermes drops it; we stack both)
- Used `append_to_system_prompt()` for session personality overlay to avoid frozen-cache invalidation
- Skin switching updates live Theme struct fields; not persisted across sessions (session-only like hermes)
- Did NOT implement first-match-wins context file exclusivity — edgecrab loading all files is more powerful

## Next Steps
- Test SOUL.md auto-seeding on a fresh install (delete `~/.edgecrab/SOUL.md` and run)
- Consider `/skin` no-arg to show current active skin name alongside list

## Lessons/Insights
- hermes-agent has 14 personality presets; edgecrab was missing `creative`, `teacher`, `surfer`, `uwu`  
- Theme struct field names differ from Skin color names — always check `theme.rs` before writing Theme fields
- `load_global_soul` must be pure sync (not async) since it's called in a sync context during session initialization
