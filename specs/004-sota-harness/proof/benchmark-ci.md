# Benchmark CI (Wave 1-e)

GitHub Actions workflow: [`.github/workflows/harness-benchmark.yml`](../../../.github/workflows/harness-benchmark.yml)

## Jobs

| Step | Command |
|------|---------|
| Games003 replay | `cargo test -p edgecrab-core --test harness_games003_replay` |
| Local harness geometry | `cargo test -p edgecrab-core --test local_harness_geometry_e2e` |
| Context cache efficiency | `cargo test -p edgecrab-core --test context_cache_efficiency_e2e` |
| Threat patterns | `cargo test -p edgecrab-security --lib threat_patterns` |

Runs on every push/PR to `main`/`master` and on `workflow_dispatch`.

## Local reproduction

```bash
cargo test -p edgecrab-core --test harness_games003_replay
cargo test -p edgecrab-core --test local_harness_geometry_e2e
cargo test -p edgecrab-core --test context_cache_efficiency_e2e
cargo test -p edgecrab-security --lib threat_patterns
```
