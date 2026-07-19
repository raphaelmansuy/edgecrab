# 005 — Security & Trust Lens (Re-assessed)

**Authority:** [000 §8 AE7](000-code-is-law.md)  
**Date:** 2026-07-19

---

## 1. Threat model (agent-amplified)

| Threat | Severity | EC mediation | H mediation |
|--------|----------|--------------|-------------|
| Path escape | Critical | `path_jail` / `path_policy` | `path_security` |
| SSRF | Critical | `is_safe_url` + preview policy | `url_safety` + optional global allow |
| Command injection | Critical | `command_scan` | approval + patterns |
| Prompt injection (context/skills) | High | `injection`, `threat_patterns`, skills_guard | threat_patterns, skills_guard |
| Tool-output injection | High | `wrap_tool_result`, brainworm patterns | patterns |
| Secret leak | High | `redact`, `secrets` backends | `secret_scope`, secret_sources |
| Unbounded loop | High | hard-stop ON | hard-stop OFF default |
| Gateway confused deputy | High | pairing | pairing |

---

## 2. Architecture (law)

**EC:** single crate `edgecrab-security` — audit one boundary.  
**H:** distributed modules — more features, consistency is discipline.

### EC public surface (selected)

- `resolve_safe_path`, `is_safe_url`, `build_ssrf_safe_client`  
- `PreviewPolicy`, `grant_session_preview_loopback`, `grant_once_preview_loopback`  
- `CommandScanner`, `ApprovalPolicy` / `ApprovalMode`  
- `scan` / `ThreatFinding`, `prepare_tool_result_body`  
- `SecretResolver`, `OsSandboxMode` / `wrap_command`  
- `check_website_access`, `SecretRedactor`

### H distinctive

- `security.allow_private_urls` global toggle (`_global_allow_private_urls`)  
- `tirith_security`, write_approval UX maturity  
- `credential_files`, richer secret_sources  

---

## 3. Design disagreements (do not parity)

| Issue | Law | EC decision |
|-------|-----|-------------|
| Global private URLs | H footgun | **REJECT** — keep port/grant scoped preview |
| Hard-stop default | H OFF / EC ON | **KEEP ON** — local models thrash |
| Profile isolates all security keys | historical hole | **merge unset from global** |

---

## 4. Pre-dispatch as security (often missed)

EC `turn_dispatch_policy` blocks:

- verification theater  
- port shopping on loopback  
- spill-blind writes  
- document GUI thrash  

This is **security-of-correctness** (prevents destructive thrash), not only SSRF. SOTA-aligned.

---

## 5. Gaps

| ID | Gap | Sev | Action |
|----|-----|-----|--------|
| S-01 | Credential pool depth | S1 | BORROW pool mechanics |
| S-02 | Secret sources breadth | S2 | extend `secrets.rs` |
| S-03 | Egress isolation docs/mode | S2 | optional proxy-only |
| S-04 | Continuous threat pattern sync | S2 | skills_guard parity process |

---

## 6. Scorecard

| Dimension | Score |
|-----------|-------|
| Structural mediation | **EC** |
| Safe defaults | **EC** |
| Vault/pool depth | **H** |
| Operator flexibility | **H** |
| Anti-theater mediation | **EC** |
| Skills supply chain | = |
