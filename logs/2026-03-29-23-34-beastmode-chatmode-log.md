Actions: Implemented docker-native Signal backend alternative in gateway setup wizard, added backend selector (cli/docker-native), docker-native daemon auto-start/update, JSON-RPC account detection, and QR link via startLink/finishLink.
Decisions: Kept existing Signal adapter unchanged by preserving SIGNAL_HTTP_URL contract; implemented no-host-Java mode as managed docker-native signal-cli service.
Next steps: Run `edgecrab gateway configure signal`, select docker-native, start container, and complete QR link; then start/restart gateway.
Lessons/insights: A backend-mode switch in setup provides a practical migration path without changing runtime adapter APIs.
