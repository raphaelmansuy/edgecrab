# 022 — First-Principles Gap Analysis: EdgeCrab vs Hermes Agent

**Status:** Living cross-ref — **code is law**  
**Re-assessed:** 2026-07-20 (full dual-tree audit + July 2026 agent-engineering rubric + 027 Waves 0–5)  
**Hermes tree:** `/Users/raphaelmansuy/Github/03-working/hermes-agent`  
**EdgeCrab tree:** this repo

| Agent | Stack | Role (law) |
|-------|-------|------------|
| **EdgeCrab** | Rust, 17 crates | Typed harness, security-default, multi-SDK embed |
| **Hermes** | Python monorepo + TS apps | Agent OS: plugins, desktop/web, ecosystem velocity |
| **Grok Build** (TUI ref) | Rust monorepo | Code-edit display reference — [015](015-grok-build-tui-code-display.md) |

---

## Start here

1. **[000-code-is-law.md](000-code-is-law.md)** — evidence ledger (paths, sizes, errata)  
2. **[001-first-principles.md](001-first-principles.md)** — J1–J10 + **AE1–AE10** (July 2026 SOTA)  
3. **[011-master-gap-matrix.md](011-master-gap-matrix.md)** + **[012-borrow-reject-priority.md](012-borrow-reject-priority.md)**

Older folders (`003-ec-vs-hermes`, `017-…`) remain historical; **022 is the strategy cross-ref**.

---

## July 2026 AE rubric (one screen)

| # | Principle | EdgeCrab | Hermes |
|---|-----------|----------|--------|
| AE1 Bounded autonomy | **hard-stop ON** | hard-stop OFF default |
| AE2 Cross-window progress | goals + parent_session + learning reflection | parent_session + curator + bg review |
| AE3 Completion = evidence | RunOutcome + verify_on_stop true | string exit + nudge |
| AE4 Tool truth | artifact_spill 913 LOC + spill-blind block | tool_result_storage + concurrent exec |
| AE5 Prompt cache | stable/dynamic policy | system_and_3 caching |
| AE6 Classify → recover | failover 21 reasons | classifier 23 + credential_pool |
| AE7 Mediated I/O | security crate + preview grants | distributed; global private URL footgun |
| AE8 Human sovereignty | typed steer | interrupt + app UX |
| AE9 Observability | harness_analyzer + OTEL + TurnPhase | plugins + trajectory |
| AE10 Extend | **SDK embed** + MCP discovery/DCR | **plugin gravity** |

---

## Headline inventory (code-verified)

| Metric | EdgeCrab | Hermes |
|--------|----------|--------|
| Loop entry | `conversation.rs:364` `execute_loop` | `conversation_loop.run_conversation` |
| Loop LOC | **8194** | **5562** (+ run_agent 6247) |
| Prologue | **139** LOC (`turn_prologue.rs`) | **623** LOC real |
| Epilogue | 734 LOC | 546 LOC |
| FailoverReason | **21** / failover.rs 854 | **23** / classifier 1621 |
| Hard-stop default | **ON** | **OFF** |
| Spill | artifact_spill **913** LOC | tool_result_storage **254** LOC |
| CORE / LSP tools | 56 / **26** LSP | toolsets + plugins |
| Platforms | **17** adapters | **20** plugin platforms + specialties |
| Slash commands | **88** | **82** |
| parent_session_id | **yes** | **yes** |
| MCP OAuth | discovery + DCR + multi-grant OAuthConfig | OAuth manager depth |
| Client surfaces | ratatui + multi-SDK | CLI + Ink + desktop + web |

---

## Document map

| # | Doc | Lens |
|---|-----|------|
| [000](000-code-is-law.md) | Evidence ledger | **Law** |
| [001](001-first-principles.md) | Ontology + AE1–AE10 | First principles |
| [002](002-systems-architect-lens.md) | Structure | Architect |
| [003](003-ai-engineer-harness-lens.md) | Loop physics | AI engineer |
| [004](004-product-owner-lens.md) | Personas / wedge | Product |
| [005](005-security-trust-lens.md) | Trust boundary | Security |
| [006](006-tools-capabilities-lens.md) | Capability surface | Tools |
| [007](007-gateway-ecosystem-lens.md) | Messaging | Gateway |
| [008](008-tui-operator-lens.md) | Operator UX | TUI |
| [009](009-extensibility-sdk-lens.md) | Plugins / SDK | Platform |
| [010](010-engineering-quality-lens.md) | Maintainability | Eng |
| [011](011-master-gap-matrix.md) | All-lens matrix | Synthesis |
| [012](012-borrow-reject-priority.md) | P0–P3 plan | Action |
| [013](013-cross-ref-index.md) | Anchors | Nav |
| **[014](014-improvement-plan.md)** | **Improvement plan + assessment** | **✅ MCP URL/OAuth · multi-tool · harness wave-2** |
| [015](015-grok-build-tui-code-display.md) | Grok Build vs EC code-edit display | TUI ref |
| [016](016-tui-edit-display-plan.md) | Edit hunks plan (A–C landed) | TUI |
| [017](017-session-forensics-2026-07-19.md) | Day forensics (pinguin/hyperframe) | Evidence |
| [018](018-non-flaky-harness-best-practices-2026-07.md) | Non-flaky practices | Research |
| [019](019-non-flaky-harness-improvement-plan.md) | Waves A–D latches (shipped plumbing) | Plan |
| [020](020-grok-xai-oauth-agent-plan.md) | Grok/xAI OAuth agent plan | SuperGrok · edgequake-llm · TUI · e2e |
| [021](021-chess-verification-forensics-and-plan-2026-07-19.md) | SuperGrok 3D chess verify RCA | Evidence |
| [022](022-session-roadblock-4f94111e-harness-deadlock.md) | Harness deadlock RCA + Heal SM | Harness |
| [023](023-first-principles-browser-localhost-2026-07-19.md) | Browser localhost / CDP / proxy | Browser |
| [024](024-tui-stream-ux-from-grok-build.md) | TUI stream UX: thinking · tools · files | W1–W3 + light W4 landed |
| [025](025-harness-balance-reopen-cap-2026-07-20.md) | **Harness balance: reopen cap · prebuilt latch** | **Shipped 2026-07-20** |
| [026](026-tui-polish-density-follow-blocks.md) | **TUI polish: density · follow · typed blocks** | **Shipped** |
| **[027](027-agent-engineering-roadmap-2026-07.md)** | **Agent engineering roadmap (AE1–AE10)** | **✅ Waves 0–5 implemented** |

---

## Reading paths

| Audience | Path |
|----------|------|
| **Engineer** | 000 → 003 → **027** → **014** → 012 |
| **Implementer (MCP / multi-tool)** | **014** · [specs/mcp/](../mcp/) (Current State: URL OAuth + DCR shipped) |
| **TUI / code-edit UX** | **015** → **016** → **024** → **026** · `stream_presentation.rs` / `presentation/` |
| **Harness / visual_ux thrash** | **017** → **018** → **019** → **025** · `harness_gates.rs` · `completion_assessor.rs` |
| **PM / exec** | 000 skim → 001 → 004 → 011 → 014 |
| **Platform** | 000 → 002 → 007 → 009 → 014 WS-A |
| **Full** | 000…027 + [specs/mcp/](../mcp/) |

---

## One-screen verdict (re-assessed)

```text
  Hermes  = agent OS (plugins, desktop, pools, classifier depth, curator)
  EdgeCrab = typed runtime (safe defaults, completion types, spill quality,
             security crate, SDK embed, anti-theater pre-dispatch)

  Strategy = selective BORROW of residual gaps — not feature parity.
  Several prior “EC missing X” claims were FALSE (failover, spill maturity,
  parent_session_id, learning reflection, MCP OAuth grants, parallel tools).
  027 Waves 0–5 landed: CI tiers, MCP isolation/discovery, replay e2e,
  TurnPhase, tool contracts, gateway drain.
```

---

## Errata highlight (why re-assessment mattered)

| Old narrative | Law |
|---------------|-----|
| EC no unified error brain | `failover.rs` 21/23 |
| EC spill thin | 913 LOC + turn budget + blind-write block |
| EC no parent_session_id | column in session_db |
| EC no learning reflection | bg `run_learning_reflection` |
| EC MCP bearer-only | multi-grant OAuthConfig + discovery/DCR |
| EC no parallel tools | JoinSet path in process_response |

Full errata: [000 §13](000-code-is-law.md).
