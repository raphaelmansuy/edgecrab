# ADR 003: Cache and Fallback Policy

## Status

Accepted

## Context

Live discovery should not block every model-listing interaction on a network
call, but cached data must not silently hide local changes forever.

The current cache is global and timestamped once for all providers. That is too
coarse.

## Decision

Use a per-provider cache entry:

```text
model_discovery_cache.json
  providers:
    ollama:
      updated_at: 1712486400
      models: [...]
    openrouter:
      updated_at: 1712482800
      models: [...]
```

Resolution order:

1. live discovery
2. non-expired provider cache
3. static catalog

TTL is provider-specific:

- local providers (`ollama`, `lmstudio`): short TTL
- remote providers (`openrouter`, `google`, `copilot`): longer TTL
- `bedrock`: medium TTL when enabled

## Edge cases

- Empty live result does not overwrite a healthy non-empty cache unless the
  provider explicitly returned an empty inventory successfully.
- Corrupt cache is ignored, not fatal.
- Unknown provider returns static-only result with empty model list if the
  catalog has nothing for it.
- Provider aliases map to one canonical cache key.

## Consequences

- Better correctness for frequently changing local providers.
- Better latency for remote providers.
- No cross-provider cache poisoning.
