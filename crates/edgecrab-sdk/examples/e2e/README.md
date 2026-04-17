# Rust SDK — E2E Proof Guide

## WHY

This folder is for users who want evidence, not just examples.

The Rust smoke run exercises real SDK behavior against a live local model and checks the public surface that matters most.

## WHAT IT COVERS

The current proof run validates chat, full conversations, streaming, model hot-swap, session export, session DB access, memory operations, tool registration, compression, and reset behavior.

## HOW TO RUN

From the repository root:

```bash
cargo run --example e2e_smoke -p edgecrab-sdk
```

### What success looks like

You should see a coverage summary followed by:

- `E2E smoke test PASSED`
- `Rust SDK coverage target PASSED`

Primary proof file:

- [../e2e_smoke.rs](../e2e_smoke.rs)

If this passes, you have strong evidence that the Rust SDK surface is working end to end.
