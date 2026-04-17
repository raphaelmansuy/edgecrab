# WASM SDK — Getting Started

## WHY

This path helps you get a browser-friendly or edge-friendly agent running quickly.

## WHAT TO RUN

1. [../basic_usage.mjs](../basic_usage.mjs) — chat, streaming, tools, and memory
2. [../business_case_showcase.mjs](../business_case_showcase.mjs) — practical operations demo

## HOW

```bash
cd sdks/wasm
wasm-pack build --target nodejs --out-dir pkg-node
OPENAI_BASE_URL=http://localhost:11434/v1 OPENAI_API_KEY=ollama \
  node examples/basic_usage.mjs
```

If you want proof-style validation next, jump to [../e2e/README.md](../e2e/README.md).
