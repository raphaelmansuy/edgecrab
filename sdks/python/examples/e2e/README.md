# Python SDK — E2E Proof Guide

## WHY

This path is for validation, regression checks, and release confidence.

## WHAT IT COVERS

The proof run checks model catalog helpers, chat, run, run_conversation, streaming, fork, batch, session snapshot/export, session DB operations, compression, new_session, and memory round-trips.

## HOW TO RUN

```bash
cd sdks/python
python3 examples/e2e_smoke.py
```

### What success looks like

You should see the coverage summary followed by:

- `E2E smoke test PASSED`
- `Python SDK coverage target PASSED`

Primary proof file:

- [../e2e_smoke.py](../e2e_smoke.py)

This is the best starting point when you need to verify that the Python wrapper still matches the real Rust core.
