Actions: Researched non-Java Signal options via upstream docs (signal-cli, binary distributions, presage, libsignal-service-rs, signald).
Decisions: Recommend staying on signal-cli for production stability in EdgeCrab; Rust alternatives are possible but require custom integration and have maintenance/licensing tradeoffs.
Next steps: If user wants, provide either a no-Java signal-cli-native path or a presage-based prototype branch plan.
Lessons/insights: signald is deprecated; signal-cli remains maintained and receives frequent updates; presage/libsignal-service-rs are the main Rust path.
