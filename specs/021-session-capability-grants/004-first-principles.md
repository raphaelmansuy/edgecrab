# 004 — First-Principles Analysis (Failed Preview Session + 021 Fix)

**Status:** Canonical analysis  
**Companion:** [000-overview.md](000-overview.md), [003-acceptance.md](003-acceptance.md)

## 1. What the system is for

EdgeCrab’s job in the reference session was not “answer a question.” It was to **complete a visual preview loop inside the product**:

```text
build/serve local page → browser_navigate(loopback) → observe → iterate
```

Two independent capabilities were required:

| Capability | Nature | Who can lift it |
|---|---|---|
| Correct Node engine (≥22 for hyperframes) | Environment | User (via shell / toolchain) |
| Browser access to `127.0.0.1:PORT` | Security policy | User (preview grant / config) |

Neither is a “missing fact” the model can invent. Both are **external constraints**.

## 2. What actually happened (facts)

```text
Visual preview task
  ├─→ Node engine mismatch
  └─→ browser_navigate localhost
         → SSRF / preview deny
         → Model retries / port shops
         → Harness suppressed_retry
         → Agent dumps bash recipe
```

Orthogonal failures stacked:

1. **Environment** — host Node v20 vs required ≥22 (`EBADENGINE`-class).
2. **Policy** — loopback navigate blocked; recovery pointed at `/config preview on` (operator action), not consent.
3. **Harness symptom** — identical `PermissionDenied` retries → `suppressed_retry_response` → transcript noise + late manual recipe.

The agent’s final answer was honest but late: it externalized work the product should have negotiated at the first policy wall.

## 3. Laws that were violated

### Law A — Grantable vs hard denial

If the validator already knows a denial is **user-revocable**, the control loop must **interrupt for consent**, not return a naked failure hoping the model will ask.

- **Violated:** browser loopback used denial text without `Once/Session/Always/Deny`.
- **Prompt bias amplified it:** `<act_dont_ask>` + clarify “not for dangerous-command confirmation.”

### Law B — Capability negotiation is not clarify

`clarify` = preference / free text.  
Policy grants need: binding security state, auditability, shared TUI/gateway chrome (`/approve session`).

### Law C — No heuristic “ask after N failures”

Counting failures then maybe asking is flaky and teaches the model to spam.

### Law D — Environment blockers need structured preference, not silent mutation

Node mismatch → one preference question or one approval-gated install. Never silent `nvm use`.

### Law E — Transcript is a trust surface

Identical policy fails must not dominate the UI.

## 4. Product metric

**JTBD:** finish the visual preview without leaving EdgeCrab for a bash recipe, when the only blocker is a policy the user can lift.

**Metric:** time from first `PreviewLoopback` denial → successful navigate (or explicit Deny) ≤ **1 user decision**.

## 5. What 021 restored

| Law | Ship location | Status |
|---|---|---|
| A — interrupt for consent | `preview_grant.rs` + `browser.rs` grant-then-revalidate | Done (tool-boundary) |
| B — typed grant | `RecoveryAction::RequestUserGrant` + `SessionPreviewGrants` | Done |
| C — no N-failure heuristic | First SSRF loopback → one approval ask | Done by design |
| D — Node preference | `terminal_node_engine_mismatch` → clarify schema | Done |
| E — transcript calm | `tool_failure_fingerprint` collapse in `response_dispatch` | Done |
| TUI/gateway parity | `ApprovalKind::PreviewLoopback` | Done |

Grant negotiation happens **inside the tool** (same pattern as terminal `approval_runtime`), not via model-driven `clarify`. See [002-approval-flow.md](002-approval-flow.md).

## 6. Residual gaps

- Live TUI dogfood of G1–G4 remains the strongest proof (unit tests ≠ one real Session → green navigate).
- Wave E still relies on the model calling `clarify` after structured recovery — intentional (preference ≠ security grant).
- Acceptance unit-test names are wired in [003-acceptance.md](003-acceptance.md); see [proof/g1-g6-session-capability.md](proof/g1-g6-session-capability.md).

## 7. Verdict

**Root cause of the failed session:** the product lacked a **capability negotiation primitive** at the tool boundary for user-revocable policy walls. The model behaved as designed under bad affordances.

**First-principles conclusion:** 021 restores the correct law — *grantable denials interrupt for consent* — for preview loopback. Remaining work is live proof (G1–G4 dogfood), not another architecture rewrite.
