# Task Log: MCP Parity with hermes-agent

## Actions
- Expanded `McpServerConfig` in config.rs: added `url`, `headers`, `bearer_token`, `timeout`, `connect_timeout`, `tools: McpToolsFilterConfig`
- Added `McpToolsFilterConfig` struct with `include`, `exclude`, `resources`, `prompts` fields
- Added `YamlMcpToolsFilter` in mcp_client.rs; expanded `YamlMcpServer` and local `McpServerConfig` with same fields
- Added `HttpMcpConnection` custom headers + separate `connect_timeout` via `reqwest::ClientBuilder::connect_timeout()`
- Added `apply_tool_filter()` + `extract_tool_filter()` helpers: include wins over exclude
- Updated `mcp_list_tools` execute to apply filtering
- Updated `mcp_call_tool` execute to check filter before invoking (returns error for excluded tools)
- Added `McpDynamicTool` struct + `ToolHandler` impl using `Box::leak` for static name/toolset strings
- Added `discover_and_register_mcp_tools()` pub async fn: connects servers, fetches tool lists, applies filters, registers `mcp_<server>_<tool>` dynamic tools + capability-aware resource/prompt wrappers
- Added static utility tools: `McpListResourcesTool`, `McpReadResourceTool`, `McpListPromptsTool`, `McpGetPromptTool`
- Updated `CORE_TOOLS` and `ACP_TOOLS` in toolsets.rs with 4 new utility tools
- Added `build_tool_registry_with_mcp_discovery()` async fn in runtime.rs
- Updated agent startup in main.rs, gateway_cmd.rs, cron_cmd.rs to use MCP discovery registry

## Decisions
- Used `Box::leak` for dynamic tool names/toolsets (bounded by startup, acceptable)
- Kept `build_tool_registry()` sync for non-agent commands (status, tools subcommands)
- Filter precedence: include (whitelist) wins over exclude (blacklist)

## Next Steps
- `notifications/tools/list_changed` for dynamic refresh
- MCP sampling support (`sampling/createMessage`)
- `/reload-mcp` dynamic tool re-registration

## Lessons/Insights
- `ToolHandler` trait requires `&'static str` → `Box::leak` needed for runtime-generated dynamic tool names
