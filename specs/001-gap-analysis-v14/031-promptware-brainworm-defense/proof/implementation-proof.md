# Implementation proof — gap 031 Promptware / Brainworm Defense

## Status: ✅ DONE (Wave 1-d)

## Single source of truth

| Item | Location |
|------|----------|
| Pattern catalogue | [`crates/edgecrab-security/src/threat_patterns.rs`](../../../../crates/edgecrab-security/src/threat_patterns.rs) |
| `scan(text, ScanContext) -> ScanResult` | same |
| Brainworm needles (≥15) | `BRAINWORM_NEEDLES` + `brainworm_pattern_count()` |
| Tool-result delimiters | `wrap_tool_result()` → `⟦EDGECRAB:TOOL_RESULT id=…⟧` |

## Consumers (no local needle lists)

| Call site | Delegation |
|-----------|------------|
| `injection.rs` | `check_injection` / `check_memory_content` → `scan` |
| `skills_guard.rs` | per-line `scan(…, Install)` |
| `plugins/guard.rs` | per-line `scan(…, Install)` |
| `prompt_builder.rs` | `scan_for_injection` + recalled memory load scan |
| `conversation.rs` | `wrap_tool_result` in `append_tool_result_to_session` |

## Config

```yaml
security:
  injection_scanning: true          # existing
  tool_output_delimiters: true      # default
  scan_recalled_memory: true        # default
```

Opt-out for delimiters in tests: `EDGECRAB_DISABLE_TOOL_DELIMITERS=1`.

## Tests

```bash
cargo test -p edgecrab-security --lib threat_patterns
cargo test -p edgecrab-security --lib injection
```

Acceptance highlights covered: ≥15 brainworm patterns with Block/Quarantine;
delimiter envelope keeps forged `</tool_result>` as literal content;
recalled MEMORY.md quarantined at load with `tracing::warn!`.
