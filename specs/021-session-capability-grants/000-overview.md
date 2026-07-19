# 021 — Session Capability Grants

**Status:** Implemented (MVP)  
**Trigger:** Failed visual-preview session — loopback `browser_navigate` denied, Node engine mismatch, retry spam, manual bash recipe.

## Law

> **Grantable denials interrupt for consent.**  
> When a tool hits a user-revocable policy wall, ask Once / Session / Always / Deny on the approval overlay. Do not hope the model asks via `clarify`, and do not count failures before asking.

## Dual blockers in the reference session

1. **Security** — `browser_navigate` to `127.0.0.1` blocked by SSRF / `security.preview`
2. **Environment** — Node &lt; engine requirement (structured recovery → clarify preference)

## Non-goals

- Silent VisualUx auto-preview expansion
- Heuristic “ask after N failures”
- Auto Node install without terminal approval
- Broadening SSRF for non-loopback private IPs

## Docs

| Doc | Role |
|-----|------|
| [001-grant-kinds.md](001-grant-kinds.md) | PreviewLoopback + future kinds |
| [002-approval-flow.md](002-approval-flow.md) | Tool-boundary grant + redispatch |
| [003-acceptance.md](003-acceptance.md) | G1–G6 scenarios |
| [004-first-principles.md](004-first-principles.md) | Failed-session analysis + laws |
| [proof/g1-g6-session-capability.md](proof/g1-g6-session-capability.md) | Automated + dogfood proof |
