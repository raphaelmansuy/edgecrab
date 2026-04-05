# Task Log — skill_view not-found hints + sub_agent_runner optimization

## Actions
- Added `collect_available_skill_names()` helper to `skills.rs` — scans skill roots, returns up to N skill names
- Modified `skill_view` not-found path to include available skill names in the error message (matching hermes-agent behavior)
- Previously (prior turn): added `skip_memory = true` and `skip_context_files = true` to `sub_agent_runner.rs`
- Added `skill_view_not_found_lists_available` test — verifies hint includes available skill names

## Decisions
- Used sync `std::fs::read_dir` for `collect_available_skill_names` (matching `find_skill_dir` pattern) since it's only called on error path
- Limited to 20 skill names max (matching hermes-agent's limit)
- Kept error message format simple: "Skill 'X' not found. Available skills: a, b, c. Use skills_list for the full list."

## Next Steps
- Consider adding disabled skill check to skill_view (hermes has `_is_skill_disabled()`)
- Consider structured JSON output for skill_view (hermes returns JSON with success, linked_files, etc.)

## Lessons/Insights
- The delegate_task "stuck" issue was just slow LLM API calls, not a bug — but skip_memory/skip_context_files eliminates unnecessary I/O
- skill_view returning bare "not found" without hints causes the model to hallucinate skill names repeatedly — hermes's approach of listing available skills in the error helps self-correction
