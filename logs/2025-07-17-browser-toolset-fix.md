# 2025-07-17 — Browser toolset root cause fix

## Root Cause
`edgecrab setup` generates `enabled_toolsets: [core, web, terminal, memory, skills]`.
`resolve_alias("core")` expanded to `[core, file, meta, scheduling, delegation, code_execution, session, mcp]` — **no "browser"**.
Browser tools have toolset label `"browser"`. Since "browser" was not in the expanded enabled list, `get_definitions()` filtered them out.
LLM schema contained `mcp_call_tool` but not `browser_navigate`, so model called `mcp_call_tool(tool_name="browser_navigate")` → immediate failure.

## Fix
1. `crates/edgecrab-tools/src/toolsets.rs` — added `"browser"` to `resolve_alias("core")` expansion.
   Browser tools are runtime-gated by `browser_is_available()` so safe: absent on machines without Chrome/CDP.
2. `crates/edgecrab-cli/src/setup.rs` — updated generated config template comment to reflect new behavior.
3. Added regression test `resolve_alias_core_includes_browser_toolset`.

## Prior Session Work (Still Active)
- `browser_is_available()` checks: Chrome binary OnceLock OR CDP_OVERRIDE mutex OR BROWSER_CDP_URL env var.
- Loop `tool_defs` recomputed each iteration (added this session) + debug logging for schema.
- All 18 toolset tests pass. Release build clean.
