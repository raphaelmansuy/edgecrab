Actions: Researched Node.js Signal alternatives and validated upstream JSON-RPC and package ecosystem status.
Decisions: Recommend Node.js integration via signal-cli JSON-RPC/HTTP endpoints, not signald.
Next steps: If requested, implement a small Node bridge service for send/receive using signal-cli daemon endpoints.
Lessons/insights: @signalapp/libsignal-client is low-level crypto bindings; most npm Signal messenger packages are wrappers and vary in reliability.
