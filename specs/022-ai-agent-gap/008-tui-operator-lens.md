# 008 — TUI & Operator Lens (Re-assessed)

**Authority:** [000 §9 AE8–AE9](000-code-is-law.md)  
**Date:** 2026-07-19

---

## 1. Surfaces (law)

| Surface | EdgeCrab | Hermes |
|---------|----------|--------|
| Terminal UI | ratatui `edgecrab-cli` | Ink `ui-tui` (~369 TS/TSX) |
| UI/compute split | in-process | `tui_gateway` host |
| Desktop | ❌ | `apps/desktop` (~877 TS/TSX) |
| Web | kanban partial | `web/` |
| Slash catalog | **88** | **82** |

---

## 2. Operator jobs

| Job | EC | H | Score |
|-----|----|---|-------|
| Stream tokens | ✅ | ✅ | = |
| Live tools | activity shelf + focus pane | rich events + labels | = |
| Mid-flight steer | Ctrl+S HINT/REDIRECT/STOP | steer/interrupt | **EC** typed |
| Subagent monitor | `/agents` | multi-agent + desktop | = / H depth |
| Cost/billing narrative | `/cost` `/usage` | billing_view + credits | **H** |
| Doctor/onboarding | doctor + setup | deeper + journey | **H** slight |
| i18n | weak | many `locales/*.yaml` | **H** |
| Harness post-mortem | harness_analyzer | — | **EC** |

---

## 3. Non-goals for EC

| Hermes pattern | Decision |
|----------------|----------|
| Pet / achievements | REJECT |
| Second Ink TUI | REJECT |
| Desktop clone | DEFER unless P5 funded |

---

## 4. Gaps

| ID | Gap | Sev | Action |
|----|-----|-----|--------|
| U-01 | Billing/entitlement in-band copy | S1 | from classifier guidance |
| U-02 | Tool streaming polish | S1 | continue 020 |
| U-03 | Onboarding journey | S2 | lightweight checklist |
| U-04 | i18n | S2 | if geo growth |

---

## 5. Scorecard

| Dimension | Score |
|-----------|-------|
| Terminal power user | **EC** slight |
| Consumer desktop | **H** |
| Steering | **EC** |
| Internationalization | **H** |
| Crash isolation UI/compute | **H** |
| Forensics path | **EC** |
