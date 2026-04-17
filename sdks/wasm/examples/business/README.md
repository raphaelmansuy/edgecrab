# WASM SDK — Business Examples

## WHY

This path highlights examples that are easy to imagine inside a browser app, edge runtime, or lightweight front-end integration.

## BEST FITS

- [../business_case_showcase.mjs](../business_case_showcase.mjs) — customer-support and executive-response patterns
- [../basic_usage.mjs](../basic_usage.mjs) — the core chat and tool mechanics behind those flows

## QUICK DEMO

```bash
cd sdks/wasm
wasm-pack build --target nodejs --out-dir pkg-node
OPENAI_BASE_URL=http://localhost:11434/v1 OPENAI_API_KEY=ollama \
  node examples/business_case_showcase.mjs
```

Use this track when you want a small, deployable example with clear business value.
