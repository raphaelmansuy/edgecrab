# 028 — Skills Hub Trusted Taps

**Tier:** C | **Impact:** 3 | **Value-per-Effort:** 3 | **Risk:** 3
**Primitive moved:** Trust + Ecosystem (curated extensibility)

> **Implementation status (2026-07-18):** Crypto signed taps are implemented under
> [`specs/019-skills-install/`](../../019-skills-install/) **W4** —
> [`signed_taps.rs`](../../../crates/edgecrab-tools/src/tools/skills_hub/signed_taps.rs)
> + [proof/w4-signed-taps.md](../../019-skills-install/proof/w4-signed-taps.md).
> This 028 pack remains historical context; do not duplicate implementation plans here.
> Note: Hermes “signed taps” in older gap text were **overstated** vs Hermes code (unsigned GitHub taps + regex guard).

## Why It Matters (First Principles)

Skills are arbitrary text injected into the system prompt. An attacker
publishing a malicious skill can exfiltrate, manipulate, or sabotage.
Hermes v0.14 introduced "trusted taps" — cryptographically-signed
skill registries with a publisher trust model. Users opt-in to a tap;
the tap publisher's GPG/Ed25519 key signs every manifest entry.

## The Gap

EdgeCrab's skills hub (`skills_hub.rs`) installs from any GitHub URL
after a content scan (`skills_guard.rs`). There is no concept of
*publisher* identity or signature verification.

## What EdgeCrab Gets Wrong Today

A typosquatted GitHub repo with a malicious skill that *passes* the
23-pattern scanner can ship to users. The scanner catches obvious
exfil patterns, not subtle behavioural attacks (e.g. a skill
instructing the agent to always email a copy of conversations to an
attacker domain).

## Cross-References

- [002-hermes-reference.md](002-hermes-reference.md)
- [003-edgecrab-current-state.md](003-edgecrab-current-state.md)
- [004-implementation-plan.md](004-implementation-plan.md)
- [005-acceptance-criteria.md](005-acceptance-criteria.md)
