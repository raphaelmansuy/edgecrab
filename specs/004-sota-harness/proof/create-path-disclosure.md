# Proof: Create-path progressive disclosure (game001)

**Date:** 2026-07-17  
**Meter:** Task success (create-file coding tasks under Indexed mode)  
**Evidence session:** `8d74ce9c-6733-4fbd-9054-f40804e0bc68` (homelab / vscode-copilot / kimi-k2.7-code)

## Failure signature

User: `Write a complete html5 and javascript 3D game in ./demo/game001`

Observed thrash:

1. `skill_manage` InvalidArgs ×3 (skills API ≠ workspace write)
2. `terminal` heredoc → HA-18 deny → SwitchTool `write_file` (still deferred)
3. `tool_search` → `write_file` succeeds
4. Shelf showed `[write_file] wrote to ?`

Wire fact: prefetch/materialize had promoted `skill_manage` while `write_file` stayed deferred.

## Fixes

| Fix | Anchor |
|-----|--------|
| Hot set: `web_search` → `write_file` | `INDEXED_HOT_TOOLS` |
| Create-intent prefetch bias | `looks_like_create_file_intent` / `prefetch_tools_for_user_message` |
| Recovery auto-materialize | `tools_to_materialize_from_error_json` + dispatch hook |
| SKILLS_GUIDANCE Indexed gating | `PromptBuilder` wire_tools + `SKILLS_GUIDANCE_INDEXED` |
| `skill_manage` action rename | `write_skill_file` (+ `write_file` alias) |
| Action-conditional InvalidArgs | `skill_manage_required_fields` |
| Shelf path from result JSON | `tool_result_summary.rs` |

## Verification

```bash
cargo test -p edgecrab-tools --lib indexed_hot_tools
cargo test -p edgecrab-tools --lib create_intent
cargo test -p edgecrab-tools --lib ha18_heredoc
cargo test -p edgecrab-tools --lib skill_manage_enrichment
cargo test -p edgecrab-core --lib write_file_preview
cargo test -p edgecrab-core --test indexed_tool_disclosure_e2e
```

## Related

- [progressive-disclosure-decisions.md](./progressive-disclosure-decisions.md)
- [indexed-schema-disclosure.md](./indexed-schema-disclosure.md)
