# Data Models

Verified against:
- `crates/edgecrab-types/src/message.rs`
- `crates/edgecrab-types/src/tool.rs`
- `crates/edgecrab-types/src/usage.rs`
- `crates/edgecrab-types/src/config.rs`

The shared types crate is the contract between the core runtime, tools, gateway, ACP server, and state layer.

## Core message model

`Message` contains:

- `role`
- `content`
- optional `tool_calls`
- optional `tool_call_id`
- optional `name`
- optional `reasoning`
- optional `finish_reason`

## Roles

- `system`
- `user`
- `assistant`
- `tool`

## Content forms

```text
Content
  -> Text(String)
  -> Parts(Vec<ContentPart>)

ContentPart
  -> text
  -> image_url
```

That lets the same type handle plain chat and multimodal turns.

## Tool-calling types

- `ToolCall`: id, type, function call, optional Gemini thought signature
- `FunctionCall`: tool name and JSON-encoded arguments
- `ToolSchema`: OpenAI-style function schema plus optional `strict`

## Usage and cost

`Usage` normalizes token accounting across:

- chat completions
- Anthropic messages
- Codex responses

`Cost` stores the corresponding cost breakdown.

## Runtime enums worth remembering

- `ApiMode`: `ChatCompletions`, `AnthropicMessages`, `CodexResponses`
- `Platform`: 18 variants including CLI, gateway platforms, ACP, and cron

## Practical rule

If you need a type in more than one crate, it probably belongs in `edgecrab-types` rather than in an app-specific module.
