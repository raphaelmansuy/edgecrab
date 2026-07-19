# 010 — Engineering Quality Lens (Re-assessed)

**Authority:** [000](000-code-is-law.md)  
**Date:** 2026-07-19

---

## 1. Snapshot (law)

| Metric | EdgeCrab | Hermes |
|--------|----------|--------|
| Language | Rust 2024 workspace | Python 3.12 + TS apps |
| Loop center | conversation.rs 8051 | conversation_loop 5562 + run_agent 6247 |
| Tests | ~4.3k test attrs | ~2.1k test files |
| Lint | clippy `-D warnings` | ruff/eslint ecosystems |
| Type safety | compile-time | runtime + typing |

---

## 2. Maintainability

### EdgeCrab

| Good | Bad |
|------|-----|
| Crate DAG | conversation.rs still magnet |
| Real epilogue/dispatch_policy/failover/spill | prologue **stub** |
| Types force contract breaks | some thin re-exports |
| Replay/forensics culture | — |

### Hermes

| Good | Bad |
|------|-----|
| Real prologue/finalizer/executor | still large shells |
| Plugin isolation of long-tail | version skew |
| Huge regression suite | slow feedback |

---

## 3. Operability

| Capability | EC | H | Score |
|------------|----|---|-------|
| Doctor | ✅ | ✅ deep | H slight |
| Harness analyzer | ✅ | ❌ | **EC** |
| OTEL | ✅ modules | plugins | = |
| Trajectory | config | mature | H |
| Drain/restart guards | thinner | gateway modules | H |
| parent_session_id | **yes** | yes | = |

---

## 4. Engineering rules for EC (SOTA)

1. **No net-new logic in `conversation.rs`** without extracting owner first.  
2. One brain per concern (errors, spill, completion, compression).  
3. Tests never write `~/.edgecrab` — TempDir + `EDGECRAB_HOME`.  
4. Security crate mandatory for I/O tools.  
5. Prefer MCP/skills over core tools for long-tail.  
6. Keep replay CI / games* forensics.  
7. Prefer typed `RunOutcome` at every exit (AE3).

---

## 5. Scorecard

| Dimension | Score |
|-----------|-------|
| Type-driven correctness | **EC** |
| Regression volume | **H** |
| Module extraction | **H** (prologue) / **EC** (policy/spill) |
| Offline doctor | **EC** |
| Contributor speed | **H** |
| Safe-by-default culture | **EC** |
