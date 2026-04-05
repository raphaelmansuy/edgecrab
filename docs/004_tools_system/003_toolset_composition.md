# Toolset Composition

Verified against `crates/edgecrab-tools/src/toolsets.rs`.

Toolsets are the policy layer above the raw tool registry. They decide which tools are shown to the model.

## Current aliases

- `core`
- `coding`
- `research`
- `debugging`
- `safe`
- `all`
- `minimal`
- `data_gen`

## How alias expansion works

```text
user config or CLI override
  -> alias expansion
  -> optional enabled whitelist
  -> disabled subtraction
  -> final toolset names
  -> registry filters tool schemas
```

## Canonical toolset names used by the resolver

- `file`
- `terminal`
- `web`
- `browser`
- `memory`
- `skills`
- `meta`
- `scheduling`
- `delegation`
- `code_execution`
- `session`
- `mcp`
- `media`
- `messaging`
- `core`

## Notable behavior in current code

- `core` is a meta-alias, not just the single `checkpoint` toolset.
- `all` is a sentinel, not a literal toolset.
- Browser and messaging toolsets can be included by policy but still vanish at runtime when the capability is unavailable.
- The conversation loop expands enabled and disabled toolsets once at turn start and reuses the result.
