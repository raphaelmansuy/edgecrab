# Hooks

Verified against `crates/edgecrab-gateway/src/hooks.rs`.

The current hook system lives in the gateway crate. It supports both native Rust hooks and file-based script hooks.

## Two hook types

- native hooks implementing `GatewayHook`
- script hooks discovered from `~/.edgecrab/hooks/<name>/`

## Script hook layout

```text
+------------------------------+
| ~/.edgecrab/hooks/my-hook/   |
+------------------------------+
| HOOK.yaml                    |
| handler.py                   |
| handler.js                   |
| handler.ts                   |
+------------------------------+
```

Python hooks run through `python3`. JavaScript and TypeScript hooks run through `bun`.

## Core types

- `HookContext`: event name plus session, user, platform, and arbitrary JSON fields
- `HookResult`: `Continue` or `Cancel { reason }`
- `HookManifest`: parsed `HOOK.yaml`
- `HookRegistry`: discovery, matching, and dispatch

## Event families currently documented in code

- `gateway:startup`
- `session:start`
- `session:end`
- `session:reset`
- `agent:start`
- `agent:step`
- `agent:end`
- `command:*`
- `tool:pre`
- `tool:post`
- `llm:pre`
- `llm:post`
- `cli:start`
- `cli:end`

## Matching behavior

- exact names are supported
- prefix wildcards such as `command:*` are supported
- global wildcard `*` is supported

## Important caveat

This hook system is gateway-owned. If you are looking for a generic core-runtime extension system, this is not it.
