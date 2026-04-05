# Task Log — 2026-03-30-20-52 — OODA SOLID/DRY Audit

## Actions
- Committed 38 pending files (1316 insertions) as baseline before audit
- OBSERVE: scanned 53k lines across 80+ Rust files; identified top-40 by size
- ORIENT: found 3 major DRY violations, 0 OCP issues (inventory used), no ISP issues
- ACT #1: DRY — removed 4 private `split_message` copies in telegram, discord, slack, signal; delegated to `crate::delivery::split_message` (already `pub fn`, smarter paragraph-break splitting)
- ACT #2: DRY — consolidated `check_injection` from memory.rs (7 patterns) + honcho.rs (8 patterns incl. "disregard") into new `edgecrab-security/src/injection.rs`; re-exported at crate root
- ACT #3: DRY — removed private `fn edgecrab_home()` from whatsapp.rs and doctor.rs that both silently ignored `$EDGECRAB_HOME`; use `edgecrab_core::edgecrab_home()` instead; also fixed unused PathBuf import in doctor.rs
- Ran full build (all crates), clippy (zero warnings), all 700+ tests (zero failures)
- Committed refactors: -165 lines, +73 lines net

## Decisions
- Kept `copy_dir_recursive` duplicate (app.rs vs hermes.rs) — different crates, no shared util crate, would require over-engineering
- Kept inline `dirs::home_dir().join(".edgecrab")` in tools — cannot import edgecrab-core from edgecrab-tools (circular dep); fix requires ToolContext change or moving edgecrab_home to edgecrab-security
- Did not extract `install_skill_from_github` from app.rs — single-use private fn, no appropriate target module without creating one-function module
- The timeout constant `30` (HTTP) repeated across adapters — deferred, acceptable as each adapter owns its timeout independently

## Next Steps
- Consider adding `edgecrab_home: PathBuf` to ToolContext to allow tools to respect EDGECRAB_HOME
- Consider extracting `install_skill_from_github` to `crates/edgecrab-cli/src/github_skills.rs`

## Lessons / Insights
- `delivery.rs::split_message` was already `pub` but unused by adapters — always check if a canonical already exists before writing private copies
- `check_injection` in honcho.rs was strictly better (had "disregard") — consolidation also improved coverage
- Local `edgecrab_home()` copies silently broke `$EDGECRAB_HOME` override — DRY violations can introduce silent functional regressions, not just code bloat
