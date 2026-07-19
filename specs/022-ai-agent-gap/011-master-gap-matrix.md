# 011 — Master Gap Matrix (Re-assessed, Code Is Law)

**Date:** 2026-07-19  
**Legend:** **EC** · **H** · **=** · **≠** · **both−**  
**Sev:** S0 invariant · S1 primary job · S2 competitive · S3 long-tail  
**Source of truth:** [000-code-is-law.md](000-code-is-law.md)

---

## A. July 2026 AE principles

| AE | Principle | Score | Sev residual | Notes |
|----|-----------|-------|--------------|-------|
| AE1 | Bounded autonomy | **EC** | — | hard-stop ON vs OFF |
| AE2 | Cross-window progress | **≠** | S2 | both have parent_session + goals; H curator deeper |
| AE3 | Completion = evidence | **EC** | S1 both− E2E | types + verify_on_stop true |
| AE4 | Tool truth / spill | **EC** | — | 913 LOC + spill-blind block |
| AE5 | Prompt cache | **=** | — | both |
| AE6 | Classify → recover | **= / H slight** | S1 narrowed | 21 vs 23; pools |
| AE7 | Mediated I/O | **EC** | — | crate + preview grants |
| AE8 | Human sovereignty | **= / EC steer** | — | |
| AE9 | Observability | **≠** | S2 | analyzer vs plugins |
| AE10 | Extend without bloat | **≠** | S1 ecosystem | SDK vs plugins |

---

## B. Harness

| Item | Score | Action |
|------|-------|--------|
| ReAct loop | = | maintain |
| Prologue extract | **H** | P0 real prologue |
| Epilogue | = / EC verify | keep |
| Pre-dispatch theater | **EC** | keep |
| Parallel tools | = | H better module boundary |
| Spill stack | **EC** | keep |
| Failover taxonomy | = / H +2 | P0.2 narrow |
| Credential pool | **H** | P1 |
| Typed RunOutcome | **EC** | keep |
| Shadow judge | **EC** | keep |
| Document done latch | **EC** | keep |
| Learning reflection | = / H depth | H curator/bg_review deeper |
| Compression | = / H volume | P1 image shrink |
| Offline analyzer | **EC** | keep |

---

## C. Tools

| Item | Score | Action |
|------|-------|--------|
| Core coding | = | |
| LSP (26) | **EC** | market |
| web_crawl | **EC** | market |
| Memory providers | **H** | bridge |
| Long-tail platform tools | **H** | demand |
| Tool error quality | **EC** | keep |

---

## D. Gateway

| Item | Score | Action |
|------|-------|--------|
| 17 core adapters | = | |
| Long-tail plugins | **H** | MCP/webhook |
| Ops (breaker/drain) | **H** | BORROW |
| Clarify multi-platform | **EC** | keep |

---

## E. Models / auth

| Item | Score | Action |
|------|-------|--------|
| OAuth big-4 | = | |
| Credential pools | **H** | P1 |
| MCP OAuth presence | = | manager depth H |
| Proxy clarity | **EC** | keep |

---

## F. Operator / product

| Item | Score | Action |
|------|-------|--------|
| Desktop/web | **H** | DEFER |
| Embed SDK | **EC** | double down |
| i18n | **H** | optional |
| Steer UX | **EC** | keep |

---

## G. Security

| Item | Score | Action |
|------|-------|--------|
| Crate mediation | **EC** | keep |
| Global private URL | H footgun | **REJECT** |
| Hard-stop default | **EC** | **KEEP** |

---

## Summary

```text
  EC leads:  AE1 defaults, AE3 types, AE4 spill+blocks, AE7 structure,
             LSP, SDK embed, steer, analyzer, document latch, shadow judge

  H leads:   prologue modularity, classifier depth (+2), credential pools,
             plugins, desktop/web, curator, memory providers, long-tail
             platforms, concurrent executor isolation, billing UX copy

  Near-parity: core ReAct, spill existence, parent_session_id, MCP OAuth
               grants present, compression core, gateway core chat, ACP

  Shared both−: E2E browser-as-user VERIFY, initializer-agent productization,
                loop file mass, partial stream brittleness

  Intentional ≠: hard-stop, preview SSRF model, no desktop, no pet
```
