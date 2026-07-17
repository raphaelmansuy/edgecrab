# 005 — First-Principles SOTA Harness Assessment (July 2026)

Post-Wave assessment of EdgeCrab against the July 2026 agent-harness field.
Scoring axis: the six physics meters in [001-meters-and-5-whys.md](001-meters-and-5-whys.md).
Living grades: [002-peer-scorecard.md](002-peer-scorecard.md). Wave closure: [003-wave-status.md](003-wave-status.md).

**Date:** 2026-07-17  
**Scope:** Assessment + scoreboard refresh only (no product code in this pass).

---

## 1. Scope and method

### Binding Constraint

For long-horizon coding tasks, the harness (context construction, tools,
orchestration, verification) often moves scores more than swapping frontier
models — the Binding Constraint Thesis
([arXiv:2605.23950](https://arxiv.org/html/2605.23950)). Public 2026 evidence
(Terminal-Bench harness swaps, Endor Labs Cursor vs Claude Code) shows
same-model swings of several to tens of points.

EdgeCrab is therefore judged on whether it maximizes the six meters for its
**open, multi-provider, multi-surface** thesis — not whether it clones Claude
Code’s UX or Codex Cloud.

```text
observe → plan → act → observe
         ↑ harness scaffolding (hooks, skills, subagents, sandbox, sessions, surfaces)
         ↑ LLM as stochastic policy
```

### Meters (only scoring axis)

| Meter | Question |
|-------|----------|
| Task success | Does the change land green? |
| Horizon | Does fidelity survive hour-long runs? |
| Cost / turn | Useful work per $ and per token |
| TTFT / prefill | Time to first useful action |
| Trust | Can side-effects be bounded / audited? |
| Surface continuity | Same agent across CLI / IDE / chat / CI? |

### Peers in scope

Claude Code · Cursor · Codex (CLI + cloud) · OpenHands · Pi · Hermes · Grok Build

---

## 2. Field map (July 2026) — who optimizes which meter

| Peer | Primary basin | Meter they own |
|------|---------------|----------------|
| **Claude Code** | Claude-native terminal orchestration | Hooks economy, long-horizon terminal task success (reported SWE/T-Bench) |
| **Cursor** | IDE inline pair + multi-provider | Surface (visual loop), task success in-editor, multi-provider routing |
| **Codex** | CLI + cloud sandbox continuity | Trust-by-default (kernel sandbox), async cloud / CI fire-and-forget |
| **OpenHands** | Open SDK + Docker multi-agent | Trust (container), parallel agents, embeddable REST |
| **Pi** | Minimal extensible TS harness | Package / context-engineering culture, programmable hooks |
| **Hermes** | Open gateway + plugin breadth | Messaging gateway, plugin economy |
| **Grok Build** | Open Rust TUI | Binary deploy narrative, parallel-subagent story |
| **EdgeCrab** | Integrity + cache + gateway + binary | See §3–4 |

Market narrative still ranks sealed Claude/Codex/Cursor products; open harnesses
win when they publish measured reliability and own a clear basin.

---

## 3. EdgeCrab physics audit (shipped evidence)

Wave 1 mostly ✅; W2-a/c and W3-b/c still 🟡. Verification pass green:
[proof/verification.md](proof/verification.md).

| Meter | State | Evidence (code / CI) |
|-------|-------|----------------------|
| **Task success** | Strong local integrity loop | `conversation.rs` `execute_loop`; LSP/mutation footers; shadow judge; harness advisory; `isolated_worktree` + `delegate_task` |
| **Horizon** | Strong | Ralph goals (`goals/`); mission steering at tool boundaries (`steering.rs`); compression without system-prompt rebuild (`compression.rs`) |
| **Cost / turn** | Lead candidate | 3-tier prompt cache (`prompt_cache_policy.rs`, `prompt_builder.rs`); `SmartRoutingStats`; doctor ≥70% cache SLO; `context_budget.rs` |
| **TTFT / prefill** | Strong locally | Stable / semi-stable / dynamic zones; hooks/goals/steers inject into **messages** only — never mutate `cached_system_prompt` mid-turn |
| **Trust** | Parity → strong (not Codex-kernel) | `threat_patterns` SoT; `prepare_tool_result_body`; recalled-memory quarantine; optional Seatbelt/bwrap (`os_sandbox.rs`) — allow-default debt remains |
| **Surface continuity** | Lead gateway+binary; IDE parity | TUI + ~18 gateway adapters + ACP + MCP in/out + `--json-stream` + proxy/cron in one binary (`edgecrab-cli`) |

### Cache law (invariant)

Lifecycle hooks only `emit_global(...)` at turn/tool/compress boundaries
([`lifecycle_hooks.rs`](../../crates/edgecrab-core/src/lifecycle_hooks.rs)).
Goals, steers, and footers append to `messages`. Compression reshapes history,
not the cached system prompt.

---

## 4. Updated peer matrix

Legend: **L** lead · **P** parity · **B** behind · **N** niche / N/A · **P−** parity-minus

| Dimension | Claude Code | Cursor | Codex | OpenHands | Pi | Hermes | Grok Build | **EdgeCrab (post-Wave)** |
|-----------|-------------|--------|-------|-----------|-----|--------|------------|---------------------------|
| Programmable hooks | **L** | P | P | P | **L** | P | P | **P** (core events; economy shallower than Claude/Pi) |
| Skills / install UX | L | P | P | P | **L** | **L** | P | **P** (`edgecrab install` + hub; not Pi package culture) |
| Parallel / worktrees | L | P | **L** | **L** | N | L | **L** | **P** (`isolated_worktree`; not fleet orchestration) |
| Coding integrity | P | P | P | P | B | B | P | **L** |
| Context / cache eng. | P | P | P | P | P | P | P | **L** |
| Multi-provider / local | B | **L** | P | **L** | **L** | **L** | P | **P→L** (catalog+OAuth+routing; plugin providers 🟡) |
| Sandbox / approvals | L | P | **L** | **L** (Docker) | B | P | P | **P** (OS sandbox optional; not kernel/Docker default) |
| IDE surface | L | **L** | L | P | RPC | ACP | ACP | **P** (ACP; no Cursor-class IDE) |
| Messaging gateway | N | N | N | N | N | **L** | N | **L** |
| Async cloud / CI | L | P | **L** | L | P | P | P | **P** (json-stream + smoke; no cloud VM product) |
| Single binary / deploy | P | B | P | B | B | B | **L** | **L** |
| Public harness proof | L | L | L | L | B | B | Rising | **P−** (deterministic CI; no Terminal-Bench/SWE public score) |

### Delta from pre-Wave [002](002-peer-scorecard.md) (aspirational arrows)

| Dimension | Pre-Wave | Post-Wave | Notes |
|-----------|----------|-----------|-------|
| Hooks / install / worktrees | B→P | **P** | Wave 1 shipped |
| Sandbox | P→L | **P** | Optional OS sandbox landed; default harden still open → not L |
| Async / headless | B→P | **P** | `--json-stream` + headless smoke |
| Public benchmarks | B→P | **P−** | CI replay suites yes; public leaderboard scores no |
| Multi-provider | P→L | **P→L** | Still blocked on 009 plugin wiring 🟡 |

---

## 5. Verdict

**EdgeCrab is SOTA in a specific basin, not the global basin.**

1. **Where EC is SOTA** — coding integrity + prompt-cache economics + messaging
   gateway breadth + single-binary deployability. No peer simultaneously leads
   those four win conditions.
2. **Where EC is SOTA-adjacent / parity** — lifecycle hooks, skill install,
   worktree subagents, multi-provider routing, headless NDJSON. Waves 1–3 closed
   the prior B→P cliffs in code.
3. **Where EC is not SOTA (do not pretend)**
   - **Task-success theater:** no public Terminal-Bench / SWE-bench Pro
     disclosure; peers publish harness+model scores as product.
   - **IDE / inline pair surface:** Cursor owns the visual loop; ACP is
     continuity, not that UX.
   - **Default hard sandbox + async cloud:** Codex / OpenHands win
     trust-by-default and fire-and-forget cloud; Seatbelt/bwrap is opt-in and
     allow-default.
   - **Plugin / package economy depth:** Pi + Hermes still denser; gap 009 🟡.
4. **Binding constraint for perception** — not missing long-tail tools.
   Missing: **measured reliability narrative**, **fleet-scale orchestration
   depth**, and **default sandbox**.

### Root-cause update (vs Five WHYs #5)

The thesis and scoreboard now exist ([001](001-meters-and-5-whys.md), this tree).
The new root cause of “not perceived as SOTA” is:

> **Proof gap** (no public harness leaderboard scores) **+** incomplete Wave
> items (**009** pluggable providers, **hardened sandbox defaults**) relative
> to sealed Claude/Codex products.

---

## 6. Cliffs ranked by meter ROI

Force-multiply only; do not reopen out-of-scope Hermes breadth.

| Rank | Cliff | Meters unlocked | Pointer |
|------|-------|-----------------|---------|
| 1 | Public harness score disclosure (Terminal-Bench-style tasks; publish scores) | Task success (perception + regression) | Extend [004-benchmark-harness.md](004-benchmark-harness.md); full vendor import still optional |
| 2 | Harden OS sandbox default (deny-default Seatbelt / tighter bwrap) | Trust | `os_sandbox.rs`; close allow-default debt in [verification.md](proof/verification.md) |
| 3 | Finish 009 pluggable providers wiring | Cost / turn, multi-provider freedom | W2-a 🟡 — beyond `list_plugin_provider_aliases` stub |
| 4 | Token-efficiency UX polish | Cost / turn | W2-c 🟡 — doctor SLO + `/context budget` → product polish |

Lower priority (do not dilute): kanban React SPA depth (W3-b), circuit-breaker
hardening (W3-c), Electron desktop, Codex app-server clone, full Hermes 100+
Python plugin port, Spotify/Feishu doc tools.

---

## 7. Explicit non-goals

Unchanged from [003-wave-status.md](003-wave-status.md):

- Spotify / Feishu doc tools
- Electron desktop
- Codex app-server clone
- Full Hermes 100+ Python plugin port

---

## Related proofs

- [proof/verification.md](proof/verification.md) — DRY/SOLID/e2e/first-principles pass
- [.github/workflows/harness-benchmark.yml](../../.github/workflows/harness-benchmark.yml) — deterministic harness CI
- [003-ec-vs-hermes/012-master-gap-matrix.md](../003-ec-vs-hermes/012-master-gap-matrix.md) — Hermes depth (orthogonal to this peer basin)
