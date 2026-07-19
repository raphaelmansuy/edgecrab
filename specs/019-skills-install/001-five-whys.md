# 001 — Five WHYs

**Cross-ref:** [000](./000-overview.md) · [002](./002-first-principles.md) · [006](./006-product-owner-lens.md)

## Chain

### WHY 1 — Why do users install skills?

Agents need **reusable, progressive-disclosure expertise** (domain playbooks, tool recipes, review checklists) without stuffing every instruction into the system prompt on every turn.

**Signal:** `skills_list` → `skill_view` → nested files; slash `/skill-name`; bundles.

### WHY 2 — Why a hub (not only copy a folder)?

Discovery + provenance beat “paste a directory and hope.” Users need search across registries, short-name resolution, updates, uninstall, and an audit trail of *where* a skill came from.

**Signal:** multi-source search, lockfile, check/update, taps, snapshot.

### WHY 3 — Why quarantine + scan before commit?

A skill is **untrusted text that becomes agent policy**. It can instruct exfiltration, destructive shell, persistence, or subtle behavioral sabotage. Installing straight into `~/.edgecrab/skills/` is a trust boundary violation.

**Signal:** quarantine dir → `skills_guard` → `InstallGate` → rename into skills root → audit log.

### WHY 4 — Why do users still fail, hesitate, or install blindly?

| Failure mode | Root |
|--------------|------|
| “Works in TUI, missing in CLI” | Surface asymmetry (`SkillsCommand` thin vs rich `/skills`) |
| “What does Caution mean?” | Verdict without theatre (Hermes Rich panels help; EdgeCrab TUI strong, CLI weak) |
| Rate-limit / empty search | No doctor guidance for `GITHUB_TOKEN`; index freshness opaque |
| False confidence | Regex guard passes; no publisher identity → typosquat risk |
| Force culture | Community skills trip Caution → users learn `--force` as habit |

### WHY 5 — Why must EdgeCrab exceed Hermes (not merely match)?

Matching Hermes CLI breadth alone is table stakes. EdgeCrab already owns **typed dangerous trust** (`--trust` + hash-bound approvals) and a **Skill Guard overlay**. The product win is:

1. **Parity** — same façade on every surface (Hermes CLI lead closed).
2. **Theatre** — install stages + evidence before commit (Hermes Rich storytelling closed; TUI lead widened).
3. **Publisher truth** — real signed taps (neither product ships this in code today; gap 028 overstated Hermes).
4. **Doctor** — hub health as a first-class readiness check.

Without (1)–(3), users bounce between surfaces, override scans blindly, and cannot trust community taps at scale.

---

## Problem statement

> EdgeCrab’s Skills Hub pipeline is production-grade, but **capability is uneven across surfaces**, **trust is pattern-only for publishers**, and **install storytelling is incomplete outside the TUI overlay** — so users under-discover skills, over-force installs, and cannot prove publisher integrity.

## Success metrics (product)

| Metric | Target |
|--------|--------|
| Surface parity | Every slash hub action has a CLI equivalent (W1) |
| Trust legibility | Install path always surfaces verdict + top findings before commit (W2) |
| Blind force rate | Documented decline via `--trust` + preview (tracked in proof) |
| Doctor signal | `/doctor` reports hub lock, taps, token, quarantine orphans (W3) |
| Publisher verify | Signed tap install fails closed on bad sig (W4) |
| Time-to-first-skill | New user: search → install → `/skill-name` usable in ≤3 minutes |

## Anti-metrics (do not optimize)

- Number of registries for its own sake
- Pattern count in `threat_patterns` without precision evidence
- Desktop/web UI clones that fork policy from the Rust façade
