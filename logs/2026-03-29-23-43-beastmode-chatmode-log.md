Actions: Diagnosed Java mismatch, patched Signal wizard to auto-set compatible JAVA_HOME for signal-cli commands, rebuilt and validated configure flow no longer errors with --enable-native-access JVM option.
Decisions: Added macOS Java detection priority (JAVA_HOME, /usr/libexec/java_home versions 25..21, Homebrew OpenJDK fallback) and routed all signal-cli invocations through one helper.
Next steps: User reruns signal configure and scans QR from phone during link flow; then restart gateway.
Lessons/insights: signal-cli wrapper can inherit incompatible Java; explicit runtime selection in command invocation eliminates host Java drift issues.
