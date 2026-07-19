# 004 — Product Owner Lens (Re-assessed)

**Authority:** [000](000-code-is-law.md) · AE10 product strategy  
**Date:** 2026-07-19

---

## 1. Product definitions (law-backed)

| | Hermes | EdgeCrab |
|-|--------|----------|
| One-liner | **Agent OS** — plugins, desktop, multi-chat, personal memory | **Typed agent runtime** — safe defaults, embed SDKs, coding harness |
| Proof | `apps/desktop`, `plugins/*`, locales, curator | `edgecrab-security`, `RunOutcome`, `sdks/*`, hard-stop ON |
| Risk if confused | Feature sprawl | Parity treadmill kills wedge |

---

## 2. Personas × code reality

| Persona | Needs | Leader (law) |
|---------|-------|--------------|
| P1 Local coder | hard-stop, LSP 26, spill truth, verify | **EC** |
| P2 Multi-chat ops | 17 EC adapters vs H long-tail plugins | **= core · H tail** |
| P3 Embedder | multi-lang SDKs | **EC** |
| P4 Plugin tinkerer | Python plugins overnight | **H** |
| P5 Desktop consumer | Electron app | **H** |
| P6 Personal memory agent | multi memory providers + curator | **H** |
| P7 Security-conscious | security crate + grants + no global private URL | **EC** |
| P8 IDE/ACP | both have ACP | **=** |

---

## 3. Wedges (double down vs refuse)

### EdgeCrab double-down

1. Safe-by-default coding harness (AE1/AE3/AE7) — **already in code**.  
2. Multi-SDK embed (AE10) — `sdks/*` + crates.  
3. Operator forensics — `harness_analyzer`, session forensics culture.  
4. Migration — `edgecrab-migrate` hermes + openclaw.

### Hermes double-down (do not fight)

1. Plugin marketplace gravity.  
2. Desktop + web.  
3. Credential pools + billing UX.  
4. Lifestyle (pet, achievements, i18n).

### Roadmap filter

```text
Serves P1/P2/P3/P7 AND (AE1–AE10 or revenue)?
  no  → REJECT/DEFER
  yes → invariant? → P0/P1
        workaround via MCP/skill? → prefer extension
```

---

## 4. Positioning (external)

**True:** Rust-native runtime; security-first defaults; Hermes-class core loop; embed SDKs; migrate path.  
**False:** 100% Hermes parity; “safer only because Rust” without naming mediation; broader provider plugins than Hermes.

---

## 5. Scorecard

| Dimension | Score |
|-----------|-------|
| Wedge clarity if disciplined | **EC** |
| End-user surface | **H** |
| Builder embed | **EC** |
| Messaging core | = |
| Messaging long-tail | **H** |
| Personal agent lifestyle | **H** |
| Coding reliability defaults | **EC** |
