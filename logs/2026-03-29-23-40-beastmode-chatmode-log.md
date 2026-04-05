Actions: Reproduced Signal CLI link empty-output bug, patched local link flow to read URI from both stdout/stderr with timeout and diagnostics, rebuilt and validated configure flow shows QR URI reliably.
Decisions: Kept CLI backend path but hardened parser/stream handling; downgraded non-zero link exit to warning to avoid aborting setup wizard.
Next steps: User should rerun signal configure and scan QR promptly; if they prefer no-host-Java, choose docker-native backend.
Lessons/insights: signal-cli link output stream differs by environment/wrapper; robust dual-stream URI extraction is required.
