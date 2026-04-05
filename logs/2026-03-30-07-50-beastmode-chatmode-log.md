# Task logs

Actions: Investigated Signal end-to-end path; found daemon connectivity failures, config corruption in allowed_users, self-message filtering gap, and SSE decode instability; patched gateway startup + signal adapter.
Decisions: Kept Signal on HTTP/SSE path; fixed health check endpoint to /api/v1/check; removed global reqwest timeout to avoid SSE body decode failures; allowed synced primary-phone self-chat messages only when sourceDevice=1 to avoid loops.
Next steps: User sends a fresh Signal message from phone to self chat and confirms agent reply; if no reply, collect last 100 lines of ~/.edgecrab/logs/gateway.log and ~/.edgecrab/logs/signal-cli.log.
Lessons/insights: signal-cli SSE keepalive can be delayed; global HTTP client timeouts break SSE; self-chat on linked devices arrives as sync sent messages and must be parsed explicitly.
