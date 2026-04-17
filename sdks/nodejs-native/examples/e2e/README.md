# Node.js SDK — E2E Proof Guide

## WHY

Use this folder when you want high-confidence verification that the Node SDK still behaves correctly against a real model backend.

## WHAT IT COVERS

The Node proof run checks catalog helpers, chat, runConversation, streaming, history, batch, session snapshot/export, session store operations, compression, and memory round-trips.

## HOW TO RUN

```bash
cd sdks/nodejs-native
node examples/e2e_smoke.mjs
```

### What success looks like

You should see the coverage summary followed by:

- `E2E smoke test PASSED`
- `Node.js SDK coverage target PASSED`

Primary proof file:

- [../e2e_smoke.mjs](../e2e_smoke.mjs)

Run this before shipping SDK changes or updating the examples.
