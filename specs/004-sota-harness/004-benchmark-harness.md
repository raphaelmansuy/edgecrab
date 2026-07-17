# 004 — Terminal-Bench style harness (local)

EdgeCrab does not vendor [Terminal-Bench](https://github.com/laude-institute/terminal-bench); this doc describes how to run comparable harness checks locally.

## Quick harness suite (CI parity)

```bash
cargo test -p edgecrab-core --test harness_games003_replay
cargo test -p edgecrab-core --test local_harness_geometry_e2e
cargo test -p edgecrab-core --test context_cache_efficiency_e2e
cargo test -p edgecrab-security --lib threat_patterns
```

## Headless agent smoke (NDJSON)

```bash
cargo build --release -p edgecrab-cli
edgecrab -q --json-stream "Reply with exactly: ok"
```

Emits one JSON object per line (`token`, `tool_exec`, `done`, `error`).

## Terminal-Bench style stub

1. Point `EDGECRAB_HOME` at a temp dir for isolation.
2. Run quiet headless tasks with `--json-stream` and parse NDJSON for `done`.
3. Gate on exit code + expected substring in final token stream.

Example config stub (`~/.edgecrab/config.yaml`):

```yaml
model:
  default_model: anthropic/claude-sonnet-4-20250514
agent:
  max_iterations: 30
harness:
  verification_strict: false
  guardrails_hard_stop: true
```

Full Terminal-Bench integration (task YAML import, scoring harness) is out of scope for Wave 1; use the replay tests above as the public benchmark surface.
