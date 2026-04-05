# Task Log — EdgeCrab First-Principle Audit

## Actions
- Audited 53 tool registrations (all CORE_TOOLS verified via inventory::submit!)
- FIX #1: Renamed workspace_root->edgecrab_home in AppConfigRef; default from current_dir() to ~/.edgecrab; wired through memory.rs, skills.rs, conversation.rs, app.rs, config_ref.rs
- FIX #2: Skills prompt -> `## Skills (mandatory)` with XML available_skills tags + mandatory scan instruction (hermes-agent parity)
- FIX #3: Context files wrapped in `# Project Context` block with instruction header
- FIX #4: Added available_tools builder field + has_tool() to PromptBuilder; moved tool_defs computation before prompt build in execute_loop so tool names available; MEMORY/SESSION_SEARCH/SKILLS/SCHEDULING guidance gated on tool availability
- FIX #6: Memory block headers now show ═══ MEMORY [42% - 924/2,200 chars] ═══ format
- FIX #7: memory_write response now returns "XX% used (N/max chars)" instead of "N bytes total"

## Decisions
- Moved tool_defs computation BEFORE system prompt build; reuses computed schemas for both prompt and LLM API call
- Used Option<Vec<String>> for available_tools with None = all tools assumed present (backward compat for tests)
- Did NOT add timestamp model/session info (Fix #5) - requires plumbing model name through AgentConfig into PromptBuilder, deferred

## Next Steps
- FIX #5 (deferred): Add model name + session ID to timestamp line in PromptBuilder::build()
- Consider extracting duplicate MEMORY_MAX_CHARS/USER_MAX_CHARS into edgecrab-types

## Lessons/Insights
- With inventory crate, all tools registered at compile time - tool count is deterministic and auditable
- Moving expensive compute (tool schema resolution) before prompt build avoids duplicate work and enables tool-aware prompt assembly
