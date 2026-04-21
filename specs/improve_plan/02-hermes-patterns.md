# 02 — Hermes Agent Patterns to Adopt

Cross-reference: patterns from hermes-agent that EdgeCrab should adopt.

## Pattern 1: Flat Core Tools List (~36 tools)

**Source**: `hermes-agent/toolsets.py` → `_HERMES_CORE_TOOLS`

```
Hermes _HERMES_CORE_TOOLS (36 tools):
+------------------+--------------------------------------+
| Category         | Tools                                |
+------------------+--------------------------------------+
| Web (2)          | web_search, web_extract              |
| Terminal (2)     | terminal, process                    |
| File (4)         | read_file, write_file, patch,        |
|                  | search_files                         |
| Vision (2)       | vision_analyze, image_generate       |
| Skills (3)       | skills_list, skill_view, skill_manage|
| Browser (10)     | browser_navigate...browser_console   |
| TTS (1)          | text_to_speech                       |
| Planning (2)     | todo, memory                         |
| Session (1)      | session_search                       |
| Clarify (1)      | clarify                              |
| Code (2)         | execute_code, delegate_task          |
| Cron (1)         | cronjob                              |
| Messaging (1)    | send_message                         |
| Home Asst (4)    | ha_list_entities...ha_call_service   |
+------------------+--------------------------------------+
| TOTAL            | ~36                                  |
+------------------+--------------------------------------+
```

**EdgeCrab equivalent**: Should reduce CORE_TOOLS from 173 to ~45
by moving specialized tools to on-demand toolsets.

```
EdgeCrab target CORE_TOOLS (~45 tools):
+------------------+--------------------------------------+
| Category         | Tools                                |
+------------------+--------------------------------------+
| Web (3)          | web_search, web_extract, web_crawl   |
| Terminal (2)     | terminal, run_process                |
| File (5)         | read_file, write_file, patch,        |
|                  | search_files, pdf_to_markdown        |
| Skills (3)       | skills_list, skill_view, skill_manage|
| Browser (7)      | navigate, snapshot, screenshot,      |
|                  | click, type, scroll, console         |
| TTS (1)          | text_to_speech                       |
| Vision (2)       | vision_analyze, transcribe_audio     |
| Planning (3)     | manage_todo_list, report_task_status,|
|                  | checkpoint                           |
| Memory (2)       | memory_read, memory_write            |
| Session (1)      | session_search                       |
| Clarify (1)      | clarify                              |
| Code (2)         | execute_code, delegate_task          |
| Cron (1)         | manage_cron_jobs                     |
| Messaging (1)    | send_message                         |
| HA (4)           | ha_list_entities...ha_call_service   |
| MCP (2)          | mcp_list_tools, mcp_call_tool        |
| Image (1)        | generate_image                       |
+------------------+--------------------------------------+
| TOTAL            | ~41                                  |
+------------------+--------------------------------------+

MOVED TO ON-DEMAND TOOLSETS:
+------------------+--------------------------------------+
| Toolset          | Tools (loaded on demand)             |
+------------------+--------------------------------------+
| lsp (25)         | All lsp_* tools                      |
| honcho (6)       | honcho_conclude...honcho_context     |
| process_mgmt (5) | list_processes, kill_process,        |
|                  | get_process_output, wait_for_process,|
|                  | write_stdin                          |
| browser_adv (5)  | browser_back, browser_press,         |
|                  | browser_close, browser_get_images,   |
|                  | browser_vision                       |
| browser_ext (3)  | browser_wait_for, browser_select,    |
|                  | browser_hover                        |
| mcp_ext (4)      | mcp_list_resources, mcp_read_resource|
|                  | mcp_list_prompts, mcp_get_prompt     |
| skills_ext (2)   | skills_categories, skills_hub        |
| moa (1)          | moa                                  |
+------------------+--------------------------------------+
```

## Pattern 2: content is Required String (no null)

**Source**: `hermes-agent/tools/file_tools.py` → `WRITE_FILE_SCHEMA`

```python
# Hermes: content is required string — no null option
"content": {"type": "string", "description": "Complete content to write"}
"required": ["path", "content"]
```

```rust
// EdgeCrab BEFORE:
"content": {"type": ["string", "null"], ...}

// EdgeCrab AFTER (adopt Hermes pattern):
"content": {"type": "string", ...}
```

**WHY**: Hermes Agent never creates empty scaffolds via write_file.
If the LLM needs an empty file, it must explicitly write `""`.

## Pattern 3: ACP Toolset as Explicit Exclusion

**Source**: `hermes-agent/toolsets.py` → `"hermes-acp"` toolset

Hermes defines the ACP (editor integration) toolset as an explicit
list that excludes clarify, send_message, TTS, image generation.

EdgeCrab should do the same — define ACP_TOOLS as CORE_TOOLS minus
an exclusion list (DRY principle).

```rust
// BEFORE: ACP_TOOLS is a separate 170-line list (90% copy of CORE_TOOLS)
pub const ACP_TOOLS: &[&str] = &[...170 lines...];

// AFTER: ACP_TOOLS derived from CORE_TOOLS minus exclusions
const ACP_EXCLUDED: &[&str] = &[
    "clarify", "send_message", "generate_image",
    "text_to_speech", "transcribe_audio", "vision_analyze",
];
pub fn acp_tools() -> Vec<&'static str> {
    CORE_TOOLS.iter()
        .filter(|t| !ACP_EXCLUDED.contains(t))
        .copied()
        .collect()
}
```

## Pattern 4: Dynamic Cross-Tool Reference Stripping

**Source**: `hermes-agent/model_tools.py` → `get_tool_definitions()`

When `web_search` is not available, Hermes strips the cross-reference
from `browser_navigate`'s description. This prevents the LLM from
hallucinating calls to unavailable tools.

EdgeCrab should adopt this pattern for any tool schema that references
other tools by name.

## Pattern 5: Argument Type Coercion

**Source**: `hermes-agent/model_tools.py` → `coerce_tool_args()`

LLMs frequently return numbers as strings ("42" instead of 42).
Hermes coerces arguments to match their schema types before dispatch.
EdgeCrab should adopt this to reduce InvalidArgs errors.
