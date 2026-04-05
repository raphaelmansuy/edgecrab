Task logs

Actions: Audited wiring paths for skills and skills_hub across core/tools/acp; fixed missing propagation of external skill dirs into ToolContext; added skills_hub exposure in core and ACP tool allowlists; aligned ACP permission tests; reran strict clippy and targeted core/acp/skills tests.
Decisions: Treated “all wired” as end-to-end path validation (config -> context -> tool exposure -> permission); patched only concrete gaps found; kept existing TODO about available_tools context plumbing as known follow-up.
Next steps: Commit current changes and run full workspace tests if desired; optional follow-up is wiring available_tools into conditional activation checks.
Lessons/insights: Main wiring gap was integration, not implementation logic: features existed but were not fully exposed/propagated in all runtime paths.
