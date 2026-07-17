# 999 — Sequenced Execution Roadmap

This roadmap orders the 30 features into 4 execution phases, respecting
dependencies and maximising early value. Effort is given in ordinal
buckets (S = 1–3 days, M = 1–2 weeks, L = 2–4 weeks, XL = 1–3 months)
since wall-clock estimates are unreliable across team sizes.

---

## Phase 1 — Tier S "Must-Ship" (parallel-safe)

The six top-impact, foundational items. Run as parallel work streams.

| # | Folder | Effort | Stream | Depends on | Status |
|---|--------|--------|--------|------------|--------|
| 1 | [001-persistent-goals/](001-persistent-goals/) | M | Core | — | ✅ DONE |
| 2 | [002-file-mutation-verifier/](002-file-mutation-verifier/) | M | Tools | — | ✅ DONE |
| 3 | [003-lsp-write-diagnostics/](003-lsp-write-diagnostics/) | M | Tools | 002 | ✅ DONE |
| 4 | [004-prompt-prefix-cache/](004-prompt-prefix-cache/) | M | Core | — | ✅ DONE |
| 5 | [005-session-handoff/](005-session-handoff/) | L | Gateway | — | ✅ DONE |
| 6 | [006-checkpoints-v2/](006-checkpoints-v2/) | L | Core | — | ✅ DONE |

**✅ Phase 1 is fully shipped** — all six carry a `proof/` folder.

**Parallelisation**: 3 streams. Stream A (Core): 001 → 004 → 006.
Stream B (Tools): 002 → 003. Stream C (Gateway): 005.

**Exit criterion**: every Phase-1 feature has acceptance criteria
green.

---

## Phase 2 — Tier A "Strategic Capabilities"

| # | Folder | Effort | Depends on | Status |
|---|--------|--------|------------|--------|
| 7 | [007-multi-agent-kanban/](007-multi-agent-kanban/) | L | — | ✅ DONE |
| 8 | [008-openai-compat-proxy/](008-openai-compat-proxy/) | M | **024 (OAuth providers)** | ✅ DONE |
| 9 | [009-pluggable-providers-plugins/](009-pluggable-providers-plugins/) | L | — | 🟡 PARTIAL |
| 10 | [010-mcp-sse-oauth-parallel/](010-mcp-sse-oauth-parallel/) | L | — | ✅ DONE |
| 11 | [011-computer-use/](011-computer-use/) | XL | — | ✅ DONE |
| 12 | [012-video-tools/](012-video-tools/) | M | — | ✅ DONE |

**Critical dependency**: Folder 008 (proxy) cannot ship until
Folder 024 (OAuth) is done. Pull 024 forward to early Phase 2.

**Suggested order**:
- Sprint 1: 024 (pulled from Tier C) → unlocks 008
- Sprint 2 (parallel): 008, 009, 010
- Sprint 3 (parallel): 007, 012
- Sprint 4: 011 (computer use — long pole, separate stream)

---

## Phase 3 — Tier B "Quality / Hygiene"

| # | Folder | Effort | Depends on |
|---|--------|--------|------------|
| 13 | [013-cron-no-agent/](013-cron-no-agent/) | M | — |
| 14 | [014-web-search-backends/](014-web-search-backends/) | M | — |
| 15 | [015-native-clarify-buttons/](015-native-clarify-buttons/) | M | — |
| 16 | [016-discord-history-backfill/](016-discord-history-backfill/) | S | — | ✅ DONE |
| 17 | [017-tool-error-sanitization/](017-tool-error-sanitization/) | S | — |
| 18 | [018-osc8-clickable-urls/](018-osc8-clickable-urls/) | S | — |
| 19 | [019-sudo-bruteforce-defense/](019-sudo-bruteforce-defense/) | S | 015 (confirm UI) |
| 20 | [020-session-auto-resume/](020-session-auto-resume/) | S | — |

**Order**: ship all S-effort items first as quick wins (16, 17, 18,
20, 19), then medium items in parallel (13, 14, 15).

---

## Phase 4 — Tier C "Polish + Ecosystem"

| # | Folder | Effort | Depends on |
|---|--------|--------|------------|
| 21 | [021-curator-subsystem/](021-curator-subsystem/) | M | — |
| 22 | [022-cold-start-perf/](022-cold-start-perf/) | M | — |
| 23 | [023-persistent-cdp-ws/](023-persistent-cdp-ws/) | M | — |
| 24 | [024-oauth-providers/](024-oauth-providers/) | XL | *pulled to Phase 2* ✅ |
| 25 | [025-i18n/](025-i18n/) | L | — |
| 26 | [026-new-platforms/](026-new-platforms/) | L | 015 |
| 27 | [027-x-search-tool/](027-x-search-tool/) | S | — | ✅ DONE |
| 28 | [028-skills-hub-trusted-taps/](028-skills-hub-trusted-taps/) | M | existing skills system |
| 29 | [029-pareto-code-router/](029-pareto-code-router/) | M | — |
| 30 | [030-transform-llm-output-hook/](030-transform-llm-output-hook/) | M | 009 |

**Notes**: 024 ships in Phase 2 (prerequisite for 008). 030 depends on
the plugin loader from 009. 028 builds on EdgeCrab's existing skills
system.

---

## Dependency Graph (high-level)

```
        ┌─────────────┐
        │ 001 goals   │──┐
        │ 004 cache   │  │
        │ 006 chkpt   │  │
        └─────────────┘  │
                         ▼
                 ┌──────────────┐
                 │ Core ready   │
                 └──────────────┘
                         │
        ┌────────────────┼──────────────┐
        ▼                ▼              ▼
   ┌─────────┐    ┌──────────┐    ┌──────────┐
   │  002    │    │  009     │    │  010     │
   │ verify  │    │ plugins  │    │  MCP     │
   └─────────┘    └──────────┘    └──────────┘
        │              │                │
        ▼              ▼                ▼
   ┌─────────┐    ┌──────────┐    ┌──────────┐
   │  003    │    │  030     │    │  ...     │
   │  LSP    │    │ transf.  │    │          │
   └─────────┘    └──────────┘    └──────────┘

   ┌──────────┐    ┌──────────┐
   │  024     │───▶│  008     │
   │  OAuth   │    │  proxy   │
   └──────────┘    └──────────┘

   ┌──────────┐    ┌──────────┐    ┌──────────┐
   │  015     │───▶│  019     │    │  026     │
   │  buttons │    │  sudo    │───▶│ platforms│
   └──────────┘    └──────────┘    └──────────┘
```

---

## Parallelisable Streams

If you have a team of 4 engineers, this is how to split work:

| Engineer | Phase-1 | Phase-2 | Phase-3 | Phase-4 |
|----------|---------|---------|---------|---------|
| **A (Core)** | 001, 004, 006 | 009, 010 | 017 | 022, 030 |
| **B (Tools)** | 002, 003 | 007, 012 | 014, 018 | 023, 027, 028 |
| **C (Gateway)** | 005 | — | 013, 015, 016, 019 | 026 |
| **D (Adapters)** | — | 024, 008, 011 | 020 | 021, 025, 029 |

---

## Quick-Win Track (parallel always-on)

Engineer-D or an intern can pick up these S-effort items any time:

- 016 Discord history backfill
- 017 Tool-error sanitisation
- 018 OSC8 clickable URLs
- 020 Session auto-resume
- 027 `x_search` tool

Each is independently shippable in 1–3 days.

---

## Phase 5 — v0.15 Refresh (Tier D, new gaps)

| # | Folder | Effort | Depends on | Status |
|---|--------|--------|------------|--------|
| 31 | [031-promptware-brainworm-defense/](031-promptware-brainworm-defense/) | M | supersedes 017 | ○ OPEN |
| 32 | [032-secrets-manager/](032-secrets-manager/) | M | — | ✅ DONE |
| 33 | [033-skill-bundles/](033-skill-bundles/) | S | existing skills system | ✅ DONE |

**Priority**: 031 first — it is a security/trust item that also pays down
the four-way threat-pattern DRY debt and absorbs Phase-3 item 017. 033 is
a quick win on top of the existing skill loader. 032 layers a
`SecretResolver` seam under the provider factory without breaking `.env`.

**Sequencing note**: do 031 step 1 (consolidate the four threat-pattern
sources into `edgecrab-security`) as a pure refactor *before* any new
pattern work — it is the highest-leverage, lowest-risk move.

---

## Risk Concentration

| Risk | Owner Phase |
|------|-------------|
| OAuth TOS exposure | 024 (Phase 2) |
| Computer-use security | 011 (Phase 2) |
| Trusted-taps key UX | 028 (Phase 4) |
| Pareto router quality | 029 (Phase 4) |
| Bot Framework JWT edge cases | 026 (Phase 4) |
| Tool-output delimiter / cache interaction | 031 (Phase 5) |
| Secret backend availability fallback | 032 (Phase 5) |

---

## How to Use This Document

1. Open each folder's `001-overview.md` to read the gap.
2. Use `004-implementation-plan.md` for architecture + file map.
3. Verify against `005-acceptance-criteria.md` before merging.
4. Update the [README.md](README.md) status table as folders ship.

## Cross-References

- [README.md](README.md) — top-level index + tier ranking.
- [000-methodology.md](000-methodology.md) — analysis methodology.
