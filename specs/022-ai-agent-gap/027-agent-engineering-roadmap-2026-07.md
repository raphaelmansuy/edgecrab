# 027 — Agent Engineering Roadmap (July 2026)

**Status:** Waves 0–5 implemented (2026-07-20)  
**Date:** 2026-07-20  
**Authority:** [001-first-principles.md](001-first-principles.md) · AE1–AE10 · [019](019-non-flaky-harness-improvement-plan.md)  
**Principles:** First principles · no flaky heuristics · DRY · SOLID · e2e-first · code is law

---

## Implementation status

| Wave | Status | Evidence |
|------|--------|----------|
| **0** | ✅ | `027` spec · CI offline tier · `harness_nonflaky_e2e` in `harness-benchmark.yml` · product README restored |
| **1** | ✅ | MCP pool key `{home}::{server}::{isolation}` · atomic tokens · stale reconnect · isolation e2e |
| **2** | ✅ | `harness_replay_e2e` NF-E10–E12 · TUI semantic TestBackend snapshot |
| **3** | ✅ | `tool_batch` JoinSet dispatch · `turn_phase` · prologue ownership · `conversation.rs` LOC ↓ |
| **4** | ✅ | schema validate · tool deadlines · `SideEffect` metadata · credential hot-swap boundary |
| **5** | ✅ | gateway breaker/drain · `CapabilityGrants` · ImageTooLarge shrink · compression lock |

---

## Mission (one screen)

EdgeCrab is a **typed, security-default agent runtime**. The harness—not the model—owns completion:

```text
INTENT → PLAN → ACT → VERIFY → DONE
```

**Keep:** hard-stop ON, typed `RunOutcome` / evidence latches, security-mediated I/O, artifact spill, SDK embed, typed steering, MCP control plane.  
**Reject:** Hermes feature parity, soft “probably done” scores, sleep-based e2e, process-global MCP session reuse.

Strategy cross-ref: [README.md](README.md) · [014-improvement-plan.md](014-improvement-plan.md).

---

## Waves

| Wave | Theme | Owner modules | E2E gate |
|------|-------|---------------|----------|
| **0** | Spec + CI tiers | `.github/workflows/*`, this doc | offline suite + heuristic checker required |
| **1** | MCP session isolation | `mcp_client.rs`, token store | distinct `Mcp-Session-Id` per EdgeCrab session; 401→refresh |
| **2** | Evidence replay + TUI e2e | `harness_replay/`, `tui_stream_ux_e2e` | NF-E10+ fixtures; TestBackend snapshots |
| **3** | Loop modularization | `tool_batch`, `turn_prologue`, turn phases | phase golden + existing harness green |
| **4** | Tool contracts | `registry.rs`, provider hot-swap | schema reject; deadline kill; rotate at boundary |
| **5** | Platform seeds | gateway breaker, capability grants | drain e2e; scoped ToolContext |

---

## Design laws (non-negotiable)

From [019 §1](019-non-flaky-harness-improvement-plan.md):

- Model proposes; harness latches decide DONE.
- Forbidden: soft scores, sleep-and-retry without state change, path-regex oracles alone, LLM self-grade as sole gate, wall-clock correctness asserts.
- Required: boolean latches, exit codes / HTTP status / content class, exact fingerprints, readiness channels, `TempDir` + mocks, committed replay fixtures.
- CI: `scripts/check-no-flaky-heuristics.sh` must stay green.

---

## CI tiers

| Tier | Workflow / job | Required for merge |
|------|----------------|--------------------|
| **offline** | `ci.yml` rust tests + `harness-benchmark.yml` (incl. `harness_nonflaky_e2e`) + heuristic checker | **yes** |
| **mocked protocol** | MCP Axum loopback e2e (`mcp_*_e2e`) inside offline | **yes** |
| **live** | `headless-smoke.yml` (secrets); ignored tests `--ignored` | **no** — skip when secrets absent |

Live suites must never be required for green main.

---

## Success metrics

| Metric | Target |
|--------|--------|
| False-complete / reopen on fixture corpus | zero on committed fixtures |
| MCP cross-session session-id leakage | zero under Wave-1 e2e |
| `conversation.rs` net LOC after Wave 3 | flat or ↓ |
| Offline CI flake rate | ≈0 |
| Streamable HTTP MCP (`mcp test`) | remains green |
| Workspace clippy `-D warnings` | required gate |

---

## Explicit rejects

Feature-parity KPI · soft confidence completion · sleep-based e2e · global private-URL allow · second UI stack · process-global MCP session reuse as optimization.

---

## Reading path

001 → 019 → **027** → implement Wave N → update [000-code-is-law.md](000-code-is-law.md) when code lands.
