# Task Log: Browser Feature Parity Implementation

- **Actions**: Added CDP override system (CdpEndpoint, parse/set/clear/status), expanded Chrome discovery (Brave, Edge), `/browser` slash command with connect/disconnect/status, annotate mode in browser_vision, fixed compile errors (pub visibility).
- **Decisions**: Deferred session recording (requires video encoder) and cloud providers (Browserbase/BrowserUse — needs new infrastructure). Used `push_output` instead of non-existent `inject_system_note`.
- **Next steps**: Consider adding session recording via CDP Page.startScreencast if demand arises. Cloud providers can be added as a separate feature.
- **Lessons/insights**: Hermes delegates recording to agent-browser CLI subprocess; edgecrab's direct CDP approach means some features require different implementation strategies.
