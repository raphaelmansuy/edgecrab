# Dynamic Model Discovery Test Plan

## Unit tests

- Provider alias normalization
- Provider support detection
- Merge behavior between static and dynamic catalogs
- Cache read/write with per-provider timestamps
- Cache expiry per provider
- Corrupt cache fallback
- Parser coverage for:
  - Ollama `/api/tags`
  - OpenAI-compatible `/v1/models`
  - Gemini `/v1beta/models`
  - Copilot model list filtering
  - OpenRouter adapter conversion

## Integration tests

- `/models refresh all` reports only live-discovery providers
- `/models <provider>` reports correct source label
- model selector background refresh includes discovered providers
- `/provider` output reflects live discovery support and feature gating

## Bedrock tests

- Compile-only tests under feature gate
- Unit tests for Bedrock response conversion
- Optional ignored live test only when AWS credentials are present

## Failure-path tests

- Provider unavailable
- Timeout
- Invalid JSON
- Empty provider response
- Cache present after live failure
- No cache and no static models

## Quality gates

- `cargo fmt --all`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`

If Bedrock feature remains gated, run an additional verification pass with:

```bash
cargo test --features bedrock-model-discovery
cargo clippy --all-targets --features bedrock-model-discovery -- -D warnings
```
