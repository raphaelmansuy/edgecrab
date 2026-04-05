# Tool Catalogue

Verified against:
- `crates/edgecrab-tools/src/toolsets.rs`
- `crates/edgecrab-tools/src/tools/`

This is the current core tool surface grouped by behavior, not by source file.

## Web

- `web_search`
- `web_extract`
- `web_crawl`

## Terminal and process control

- `terminal`
- `run_process`
- `list_processes`
- `kill_process`
- `get_process_output`
- `wait_for_process`
- `write_stdin`

## Files

- `read_file`
- `write_file`
- `patch`
- `search_files`

## Skills

- `skills_list`
- `skills_categories`
- `skill_view`
- `skill_manage`
- `skills_hub`

## Browser

- `browser_navigate`
- `browser_snapshot`
- `browser_screenshot`
- `browser_click`
- `browser_type`
- `browser_scroll`
- `browser_console`
- `browser_back`
- `browser_press`
- `browser_close`
- `browser_get_images`
- `browser_vision`
- `browser_wait_for`
- `browser_select`
- `browser_hover`

## Media

- `text_to_speech`
- `vision_analyze`
- `transcribe_audio`
- `generate_image`

## Planning, memory, and history

- `manage_todo_list`
- `memory_read`
- `memory_write`
- `session_search`
- `checkpoint`
- `clarify`

## Honcho

- `honcho_conclude`
- `honcho_search`
- `honcho_list`
- `honcho_remove`
- `honcho_profile`
- `honcho_context`

## Home Assistant

- `ha_list_entities`
- `ha_get_state`
- `ha_list_services`
- `ha_call_service`

## Execution and delegation

- `execute_code`
- `delegate_task`
- `mixture_of_agents`

## Scheduling

- `manage_cron_jobs`

## MCP

- `mcp_list_tools`
- `mcp_call_tool`
- `mcp_list_resources`
- `mcp_read_resource`
- `mcp_list_prompts`
- `mcp_get_prompt`

## Messaging

- `send_message`

## Notes on visibility

- Some tools are always compiled in but runtime-gated by environment, platform, or attached services.
- ACP intentionally excludes interactive and delivery-specific tools such as `clarify`, `send_message`, `generate_image`, and `text_to_speech`.
