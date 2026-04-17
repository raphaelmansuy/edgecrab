# WASM SDK — E2E Proof Guide

## WHY

The WASM SDK has a smaller surface, so the proof run is especially useful for making sure the browser-friendly agent features still behave as expected.

## WHAT IT COVERS

The current smoke check validates constructor/setup, tool registration, memory read-write, streaming, fork, model switching, and session reset against local Ollama.

## HOW TO RUN

```bash
cd sdks/wasm
wasm-pack build --target nodejs --out-dir pkg-node
OPENAI_BASE_URL=http://localhost:11434/v1 OPENAI_API_KEY=ollama \
  node examples/e2e_smoke.mjs
```

### What success looks like

You should see the coverage summary followed by:

- `E2E smoke test PASSED`
- `WASM SDK coverage target PASSED`

Primary proof file:

- [../e2e_smoke.mjs](../e2e_smoke.mjs)

Use this guide when you want a clear verification path instead of a generic example list.
