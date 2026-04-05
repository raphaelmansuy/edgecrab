Task logs

Actions: Implemented concrete skills_hub actions (search, browse, inspect, install, update, uninstall), added GitHub direct install path, integrated optional skill install flow, and wired external skill directories scan in skills_list.
Decisions: Reused existing skills_hub.rs install pipeline (quarantine->scan->install->lock), used GitHub Contents API for owner/repo/path installs, and kept update as lock/status + reinstall guidance for safe incremental Phase 2 completion.
Next steps: Review diff, run runtime smoke checks (skills_hub search/install against a known repo), and commit Phase 2 changes.
Lessons/insights: Existing hub core was strong; main gap was tool surface integration and end-to-end wiring in skills.rs.
