# ADR-001: Gateway Path Policy — Tolerate Non-Existent Trusted Roots

**Status**: Accepted  
**Date**: 2026-04-13  
**Deciders**: EdgeCrab core team  
**Technical Area**: edgecrab-security / edgecrab-tools / edgecrab-gateway  

---

## Context

### Observed Failure

When an EdgeCrab Gateway instance receives an image (e.g., via WhatsApp) and the
agent calls `vision_analyze` on the downloaded file, the tool fails immediately
with:

```
vision_analyze failed in 0.0s: Execution failed in vision_analyze:
Cannot resolve allowed root '/Users/user/.edgecrab/images': No such file or directory
```

The same class of failure was previously seen in `pdf_to_markdown` tests (fixed
in 0.4.0 by guarding the extra root with `.exists()` at the call site).

### Root Cause (First Principles)

`PathPolicy::canonical_allowed_roots` calls `std::fs::canonicalize()` on
**every** root — workspace root, virtual-tmp root, configured allowed roots, and
caller-provided *extra_roots*. `canonicalize` fails with OS error 2 when the
target path does not exist.

The three optional "extra" roots passed by `vision_analyze` are:
- `~/.edgecrab/images/`         — TUI clipboard images
- `~/.edgecrab/image_cache/`    — WhatsApp Baileys bridge cache
- `~/.edgecrab/gateway_media/`  — Rust-native gateway adapters (Telegram, Discord…)

These directories are **lazily created**: `ensure_edgecrab_home()` does not
create them; they only appear when the first image is downloaded. On a fresh
install, or before any gateway image has arrived, they do not exist.

### Hermes Approach (Comparison)

Hermes (`vision_tools.py`) does **not** apply path jailing. It reads local
files directly, trusting that the LLM selected a sensible path. This trades
security for simplicity.

EdgeCrab's path-jailing approach is correct and more secure. The bug is not the
existence of the jail — it is the rigidity of the canonicalization step, which
treats "root does not exist" as an unrecoverable error rather than "root cannot
contain any files, so skip it."

### Gateway vs CLI Isolation

| Aspect | CLI | Gateway |
|---|---|---|
| Working directory | User CWD | `std::env::current_dir()` at launch |
| Allowed file access | Workspace + optionals | Same policy, wider optional roots |
| Image sources | Local clipboard saves | Platform-downloaded to `gateway_media/` |
| Typical optional roots | `~/.edgecrab/images/` | + `image_cache/`, `gateway_media/` |

Hermes solves this by using `MESSAGING_CWD` (defaults to `PATH.home()`) as the
working directory, giving the gateway broad implicit access. EdgeCrab's design
is more restrictive by default, but must tolerate optional roots that haven't
been created yet.

---

## Decision

### Primary Fix: Tolerate non-existent optional roots in `canonical_allowed_roots`

Modify `path_policy.rs` to apply different handling per root category:

| Category | On non-existent | Rationale |
|---|---|---|
| `workspace_root` | Fail (`InvalidRoot`) | Required invariant — the agent's CWD must exist |
| `virtual_tmp_root` | Already pre-canonicalized; n/a | Caller ensures it exists |
| `self.allowed_roots` (config) | Log `warn!`, skip | User misconfiguration; don't crash the tool |
| `extra_roots` (caller-provided) | Log `debug!`, skip | Lazily-created optional dirs; safe to omit |

**Security invariant preserved**: A non-existent directory contains no files.
Skipping it means "no path under this root is trusted yet" — which is exactly
correct. The caller cannot use a non-existent trusted root to access files that
don't exist anyway.

### Why NOT "create the directories at startup"

Adding `gateway_media_dir` and `image_cache_dir` to `ensure_edgecrab_home()`
would create empty directories that are only meaningful when the gateway is
running. CLI-only users would get unnecessary noise. The laziness is
intentional.

### Why NOT "fix at every call site"

That was done as a workaround in `pdf_to_markdown` (v0.4.0). It is not
scalable — any future tool that passes optional extra roots would need the same
boilerplate guard. Fixing the root cause in the security layer benefits all
current and future callers.

### Why NOT "mirror Hermes and remove path jailing in gateway mode"

Path jailing is a meaningful security boundary. In gateway mode, the agent
receives instructions from remote users over messaging platforms. Without path
jailing, a malicious gateway user could prompt the agent to read
`/etc/passwd`, SSH private keys, or other files outside the intended scope.
Hermes accepts this risk for simplicity; EdgeCrab does not.

---

## Consequences

### Positive

- `vision_analyze` works correctly in all gateway contexts on first run
- `pdf_to_markdown` can be simplified (the `.exists()` guard at the call site can stay as defense-in-depth, but is no longer load-bearing)
- All future tools that pass optional extra roots via `jail_read_path_multi` benefit automatically
- CLI behavior is unchanged (its extra roots are also created lazily and benefit from the same graceful handling)

### Negative / Trade-offs

- A misconfigured `allowed_roots` entry in config silently becomes a no-op (mitigated by the `warn!` log)
- Slightly more complex logic in `canonical_allowed_roots`

### Security Assessment

The change makes the allow-list **more permissive when a root doesn't exist**
(skip) versus **erroring out**. This is strictly safer from a security
perspective: if the root doesn't exist, no files can be under it, so skipping
it is equivalent to "allow nothing under this root" — which was the effective
behavior before the root was created anyway.

---

## Implementation

See commit: `fix(security): skip non-existent extra_roots in canonical_allowed_roots`

Changed files:
- `crates/edgecrab-security/src/path_policy.rs` — split `canonical_allowed_roots` iterator to handle extra_roots gracefully
- `crates/edgecrab-tools/src/tools/vision.rs` — no change required (fix is in the layer below)
- `crates/edgecrab-tools/src/tools/pdf_to_markdown.rs` — optional: simplify the `.exists()` guard that was added as a v0.4.0 workaround

---

## Alternatives Considered and Rejected

| Alternative | Why Rejected |
|---|---|
| Create dirs at startup (`ensure_edgecrab_home`) | Creates unnecessary dirs for CLI-only users |
| Fix at each call site (`.exists()` guard) | Recurring boilerplate; doesn't fix root cause |
| Remove path jailing for gateway | Degrades security for remote message senders |
| Use a separate `GatewayPathPolicy` subtype | Over-engineering; the laziness problem is general, not gateway-specific |
| Catch `InvalidRoot` in `execute()` and retry without extra roots | Changes tool semantics; hides the real failure mode |

---

## Appendix: Hermes Gateway Isolation Comparison

Hermes uses `MESSAGING_CWD` (env var defaulting to `Path.home()`) as the
terminal working directory in gateway mode. File tools (`file_read`, etc.) use
the CWD as their implicit root. This gives the gateway agent broad access to
the home directory by default.

EdgeCrab uses the process CWD (`std::env::current_dir()`) and applies
`PathPolicy` independently of platform. The gateway receives the same security
policy as CLI but with additional trusted roots for media directories.

The EdgeCrab model is strictly more restrictive. The fix in this ADR removes
the unnecessary fragility (extra_roots failing when non-existent) while
preserving the restriction.
