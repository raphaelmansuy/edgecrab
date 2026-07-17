# 001 — Meters and Five WHYs

## Six physics meters

| Meter | Question | Dominant driver |
|-------|----------|-----------------|
| **Task success** | Does the change land green? | Tools + verification + loop policy |
| **Horizon** | Does fidelity survive hour-long runs? | Context budget, goals, memory, compression |
| **Cost / turn** | Useful work per $ and per token | Cache hit rate, tool schema floor, routing |
| **TTFT / prefill** | Time to first useful action | Prefix stability (cloud) + KV reuse (local) |
| **Trust** | Can side-effects be bounded / audited? | Sandbox, approvals, injection defense, secrets |
| **Surface continuity** | Same agent across CLI / IDE / chat / CI? | Single orchestrator + adapters |

**Invariant:** observe → plan → act → observe. Differentiation is scaffolding
(hooks, skills, subagents, sandbox, sessions, surfaces).

## Five WHYs — why EdgeCrab is not yet perceived as SOTA

1. Market narratives optimize for Claude Code / Codex / Cursor / Grok Build
   (hooks, cloud async, parallel subagents, IDE polish), not gateway + integrity + binary.
2. Users maximize useful completed PRs per dollar with least friction; sealed
   products co-design model + harness.
3. EdgeCrab optimized Hermes-parity coding core + gateway; still thin on
   lifecycle hook economy, Pi-class install UX, parallel worktree fleets,
   public harness benchmarks, headless CI culture.
4. Hermes is breadth-max; SOTA coding agents won on orchestration depth +
   measured reliability + distribution. EC already leads Hermes on integrity
   and deployability — next cliff is vs Claude Code / Codex / Grok Build / Pi.
5. **Root cause:** no single SOTA thesis + scoreboard. Closing 30 Hermes gaps
   dilutes force. Force-multiply the five win conditions; close only peer gaps
   that block the meters.

## Win conditions (own these)

1. Coding integrity — mutation footer, LSP write gate, shadow judge, typed steering
2. Context / cache economics — 3-tier prompt cache, compression hygiene, local prefill
3. Model freedom — API + OAuth + local without lock-in
4. Multi-surface — TUI + gateway + ACP + cron + proxy in one binary
5. Deployability — static binary, Termux, migrate, SDK
