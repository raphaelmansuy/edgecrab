# 012 — Borrow / Reject / Priority (Re-assessed)

**Authority:** [000](000-code-is-law.md) · [011](011-master-gap-matrix.md) · AE1–AE10  
**Date:** 2026-07-19  
**Rule:** mechanism not file layout; one owner module; serve P1/P2/P3/P7.  

**Full implementation plan (MCP OAuth URL · multi-tool · e2e · SOLID/DRY):**  
→ **[014-improvement-plan.md](014-improvement-plan.md)**

---

## P0 — Invariant / SOTA

| ID | Action | Hermes anchor | EC owner | Done when |
|----|--------|---------------|----------|-----------|
| **P0.1** | Real turn prologue (not 39-line stub) | `turn_context.py` (623 LOC) | `turn_prologue.rs` | preflight, prompt restore, tracker reset leave conversation.rs |
| **P0.2** | Classifier residual only | `upstream_rate_limit`, `ssl_cert_verification` + guidance | `failover.rs` | two variants + rotate-vs-fallback semantics |
| **P0.3** | External VERIFY evidence bar | browser-as-user / verification_evidence patterns | `turn_epilogue` + `contract_verify` + computer_use/browser | coding/web task classes cannot self-declare done without evidence |
| **P0.4** | Extract tool batch dispatch + e2e multi-tool | `tool_executor.py` shape | **`tool_batch.rs`** (014 WS-B) | JoinSet path unit-testable; E2E-T1–T4 |
| **P0.5** | Wire credential rotate end-to-end | `credential_pool.py` | model_router + oauth | 429 rotates key when pool configured |
| **P0.6** | **MCP OAuth URL registration** | `hermes mcp add --url --auth oauth` | **`mcp_register.rs`** + cli/TUI (014 WS-A) | `mcp add --url` + login + SSRF e2e |

---

## P1 — High ROI

| ID | Action | Owner |
|----|--------|-------|
| P1.1 | Gateway circuit breaker + drain | edgecrab-gateway |
| P1.2 | MCP OAuth manager depth (401, disk watch) after URL register | mcp_client + mcp_oauth |
| P1.3 | Memory provider bridge (1–2 + MCP) | tools/plugins |
| P1.4 | Image shrink on ImageTooLarge | provider_call / compression |
| P1.5 | Billing/entitlement operator copy | CLI + failover hints |
| P1.6 | Tool streaming visibility (020) | edgecrab-cli |
| P1.7 | Compression lock for concurrent compress | compression.rs |

---

## P2 — Selective

| ID | Notes |
|----|-------|
| P2.1 | Curator LLM depth (optional) |
| P2.2 | Cron blueprints catalog |
| P2.3 | Profile routing gateway |
| P2.4 | i18n if growth geo |
| P2.5 | Kanban React depth if multi-agent marketed |
| P2.6 | Extra OAuth targets on demand |

---

## DEFER

Desktop app · Teams/LINE native without demand · full Python plugin host · Camofox unless browser reliability gap · Codex runtime unless users · learning graph productization

---

## REJECT

| Pattern | Why |
|---------|-----|
| Global `allow_private_urls` | SSRF class break |
| hard_stop default OFF | AE1 local thrash |
| Feature-parity KPI | kills wedge |
| Pet / achievements | brand noise |
| Auto-inject full spill | turn budget explosion |
| Second TUI stack (Ink) | maintain one well |

---

## KEEP / market (EC already has)

| Pattern | Path |
|---------|------|
| hard-stop ON | config + tool_loop_guardrails |
| Preview port/grant SSRF | url_safety PreviewPolicy |
| RunOutcome + assessor | edgecrab-types + completion_assessor |
| Shadow judge | shadow_judge.rs |
| Spill-blind write block | turn_dispatch_policy |
| Document done latch | task_class |
| Multi-SDK | sdks/* |
| harness_analyzer | core |
| LSP 26 | toolsets LSP_TOOLS |
| parent_session_id | session_db (use in UX) |
| Learning reflection bg | conversation.rs |
| MCP OAuth grants | mcp_client OAuthConfig |
| Typed steering | steering.rs |

---

## 90-day sequence

```text
  0–30d   P0.1 P0.2 P0.4     structure + classifier residual
  30–60d  P0.3 P0.5 P1.1 P1.6 verify + pools + gateway ops + TUI
  60–90d  P1.2 P1.3 P1.4 P1.5 P1.7  ecosystem bridges
```

---

## Success metrics (not feature counts)

| Metric | Direction |
|--------|-----------|
| False-complete rate | ↓ |
| Spill-blind mutation attempts blocked | observed in logs |
| 429 multi-key recovery success | ↑ when pool set |
| `conversation.rs` net lines | ↓ or flat |
| SDK hello-time | ↓ |
| Security review time per tool | ↓ |
