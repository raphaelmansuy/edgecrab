# Task Log — edgecrab Feature Parity with hermes-agent

## Actions
- Fixed 5 test compile errors in `prompt_builder.rs` (load_skill_summary signature change)
- Added `auto_title_session` async function to `conversation.rs` — fires on first exchange as background tokio task
- Added `InsightsReport`, `InsightsOverview`, `ModelBreakdown`, `PlatformBreakdown`, `ToolUsage`, `DailyActivity` structs to `edgecrab-state`
- Added `query_insights(days: u32)` method to `SessionDb`
- Updated `handle_show_insights()` in `app.rs` to show historical 30-day stats + daily sparkline
- Added `state_db()` accessor to `Agent` struct
- Added path-keyed `SKILLS_CACHE: Mutex<HashMap<PathBuf, SkillsCacheEntry>>` with 60-second TTL + `invalidate_skills_cache()`
- Wrapped existing `load_skill_summary` scan in `load_skill_summary_inn# Task Log — edgecrab Feature Parity with hermes-agent

## Actions
- Fixed 5 test compile errors in `prompt_builder.rs` (load_skill_summary signature change)
- Added `auto_title_session` async functio`M
## Actions
- Fixed 5 test compile errors in `prompt_bus +- Fixed 5is- Added `auto_title_session` async function to `conversation.rs` — fires on first exchaut- Added `InsightsReport`, `InsightsOverview`, `ModelBreakdown`, `PlatformBreakdown`, `ToolUsage`, `DailyActivity` st` - Added `query_insights(days: u32)` method to `SessionDb`
- Updated `handle_show_insights()` in `app.rs` to show historical 30-day stats + dol- Updated `handle_show_insights()` in `app.rs` to show h N- Added `state_db()` accessor to `Agent` struct
- Added path-keyed `SKILLS_CACHE: Mutex<HashMap c- Added path-keyed `SKILLS_CACHE: Mutex<HashMay - Wrapped existing `load_skill_summary` scan in `load_skill_summary_inn# Task Log — edgecrab Feature Parity with hermes-ag F
## Actions
- Fixed 5 test compile errors in `prompt_builder.rs` (load_skill_summary signature change)
- Added `auto_title_ses Mu- Fixed 5s - Added `auto_title_session` async functio`M
## Actions
- Fixed 5 test compile errors in _v## Actions
- Fixed 5 test compile errors inon- Fixed 5lo- Updated `handle_show_insights()` in `app.rs` to show his cargo build 2>&1 | tail -3 | cat
 cargo build 2>&1 | grep -E "Finished|error\[" | cat
 cargo build 2>&1; echo "EXIT: $?"
 cargo check 2>&1 | grep -c "^error" ; echo done
 cargo check 2>&1 | tail -20
 cargo test --no-run 2>&1 | tail -30
 ls /Users/raphaelmansuy/Github/temp/nous_hermes/edgecrab/Cargo.toml && echo "EXISTS"
 pwd && ls /Users/raphaelmansuy/Github/temp/nous_hermes/edgecrab/ | head -10
 echo "hello"
 pushd /Users/raphaelmansuy/Github/temp/nous_hermes/edgecrab
 cargo check 2>&1 | head -20
 cargo test -p edgecrab-security --manifest-path /Users/raphaelmansuy/Github/temp/nous_hermes/edgecrab/Cargo.toml 2>&1 | cat
 cargo test -p edgecrab-security 2>&1 | tail -30 | cat
 cargo test -p edgecrab-security --manifest-path /Users/raphaelmansuy/Github/temp/nous_hermes/edgecrab/Cargo.toml 2>&1 | grep -E "^(test |FAILED|error|test result)" | cat
 ls /Users/raphaelmansuy/Github/temp/nous_hermes/edgecrab/ && echo "OK"
 echo "hello"
 sleep 120 && echo "WAIT_DONE"
 cargo test --manifest-path /Users/raphaelmansuy/Github/temp/nous_hermes/edgecrab/Cargo.toml 2>&1 | tail -30
 cargo test --manifest-path /Users/raphaelmansuy/Github/temp/nous_hermes/edgecrab/Cargo.toml --no-fail-fast 2>&1 | tee /tmp/test_output.txt | tail -5
 cat /tmp/test_output.txt 2>/dev/null | grep -E "test result|FAILED" | head -20
 mkdir -p /Users/raphaelmansuy/Github/temp/nous_hermes/edgecrab/logs && cat > /Users/raphaelmansuy/Github/temp/nous_hermes/edgecrab/logs/2025-07-15-session-feature-parity.md << 'EOF'
# Task Log: 2025-07-15 — Feature Parity Session

## Actions
- Fixed 2 failing GitHub PAT redaction tests by: (1) adding `_` to `ghp_[A-Za-z0-9_]` pattern for fine-grained PAT token bodies; (2) restricting ENV assignment pattern value portion to `[A-Za-z0-9_\-.+/]` chars so already-redacted `[REDACTED_...]` tokens are not re-matched and overwritten

## Decisions
- Used character-class restriction instead of lookahead (Rust `regex` crate has no lookahead support) to prevent over-matching of already-redacted tokens

## Next Steps
- None; all 635 tests pass, 0 failed

## Lessons/Insights
- When applying multiple regex patterns sequentially, downstream patterns can re-match already-replaced tokens — restrict value character classes to raw secret characters (not `[`, `]`) to prevent this
