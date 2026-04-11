# 🦀 Security Model

> **WHY**: An AI agent that can run shell commands, read files, and fetch URLs is an attractive target for prompt injection, path traversal, SSRF, and secret leakage. Rather than scattering ad-hoc checks across 91 core tools, EdgeCrab centralises all security primitives in `edgecrab-security` — a single crate every tool calls before doing real work.

**Source**: `crates/edgecrab-security/src/`

---

## Threat Map

| Threat | Module | Guard |
|---|---|---|
| Path traversal (`../../etc/passwd`) | `path_jail` | Canonicalise and check prefix |
| Local-network SSRF (`http://192.168.x.x`) | `url_safety` | Block RFC-1918 and loopback |
| Dangerous shell commands (`rm -rf /`) | `command_scan` | Aho-Corasick + regex |
| Prompt injection (hidden Unicode, instructions) | `injection` | Unicode normalisation + pattern check |
| Secret leakage in output | `redact` | Pattern-matched redaction before display |
| Unrestricted risky operations | `approval` | Explicit user confirmation gate |
| Input normalisation edge cases | `normalize` | NFC + strip invisible chars |
| Per-path permission policy | `path_policy` | Allow/deny list for path prefixes |

---

## Module Descriptions

### `approval` — Explicit Risk Gate

Before a tool executes a high-risk operation (shell command, file write outside the project, URL fetch), it calls the approval module. The approval mode is configured in `AppConfig::security.approval_mode`:

```
┌──────────────────────────────────────┐
│            approval_mode             │
├───────────┬───────────────┬──────────┤
│  "never"  │  "on_risk"    │ "always" │
│           │   (default)   │          │
│  skip     │  check risk   │  always  │
│  approval │  score; ask   │   ask    │
│           │  if risky     │          │
└───────────┴───────────────┴──────────┘
```

`ApprovalChoice` returned by the user:

```rust
pub enum ApprovalChoice {
    Allow,          // run once
    AllowAlways,    // add to permanent allow list
    Deny,           // block this call
    DenyAlways,     // add to permanent deny list
}
```

---

### `command_scan` — Shell Command Safety

The `CommandScanner` uses Aho-Corasick multi-pattern matching for known dangerous patterns (fast, O(n) on input length), then applies regex secondary scans for context-sensitive patterns:

```
raw shell command
      │
      ▼
┌─────────────────────┐
│   Aho-Corasick      │  first pass — O(n), pattern set compiled once
│   multi-pattern     │  matches: "rm -rf", ":(){ :|:& };:", "dd if=/dev/zero"…
└─────────┬───────────┘
          │ suspicious? → secondary scan
          ▼
┌─────────────────────┐
│   regex checks      │  context-sensitive: pipe chains, sudo escalation,
│                     │  network exfil patterns, /dev writes…
└─────────┬───────────┘
          │
          ▼
    RiskScore { level, reason }
          │
          ▼
    ┌─────┴──────┐
  safe       risky → approval gate
```

---

### `injection` — Prompt Injection Detection

Hidden Unicode and out-of-band instructions are the primary prompt injection vectors for LLM agents. The `injection` module:

1. Normalises input to NFC (catches decomposed invisible characters)
2. Strips zero-width joiners, RTLO/LTRO override characters, soft-hyphens
3. Checks for known injection instruction fragments (`ignore previous instructions`, `disregard`, `system:`, `[INST]`…)
4. Returns `InjectionRisk { detected: bool, reason: Option<String> }`

```rust
// Example call inside a tool handler
let risk = check_injection(&user_provided_filename)?;
if risk.detected {
    return Err(ToolError::SecurityViolation(risk.reason.unwrap_or_default()));
}
```

---

### `path_jail` — Filesystem Confinement

```
requested path: "/home/user/project/../../etc/passwd"
      │
      ▼
canonicalise (resolve symlinks + .. segments)
      │
      ▼
"/etc/passwd"
      │
      ▼
check: does canonical path start with any allowed root?
  allowed roots: ["/home/user/project", "/tmp/edgecrab-*"]
      │
      ▼
NO → PathTraversalError
YES → proceed
```

Allowed roots are configured in `AppConfig::security` and extended per-session by the `path_policy` module.

---

### `url_safety` — SSRF Prevention

```rust
// Blocked address classes
- 127.0.0.0/8      (loopback)
- 10.0.0.0/8       (RFC-1918 private)
- 172.16.0.0/12    (RFC-1918 private)
- 192.168.0.0/16   (RFC-1918 private)
- 169.254.0.0/16   (link-local / AWS metadata endpoint)
- ::1              (IPv6 loopback)
- fd00::/8         (IPv6 ULA)
- file:// scheme
- unconventional ports (blocked list)
```

DNS rebinding is mitigated by resolving the hostname before sending the request and checking the resolved address against the same block list.

---

### `redact` — Output Sanitisation

`redact` runs on every string that leaves the tool layer back toward the model or the user. It pattern-matches against:

- AWS key patterns (`AKIA[A-Z0-9]{16}`)
- GitHub tokens (`ghp_`, `ghs_`, `github_pat_`)
- Generic high-entropy strings in environment variables
- Custom patterns from `AppConfig::privacy.redact_patterns`

Matched secrets are replaced with `[REDACTED]` before display or storage.

---

## Defence-in-Depth Stack

```
model sends tool call
        │
        ▼
┌───────────────────┐
│  normalize input  │  NFC, strip invisible chars
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│  injection check  │  hidden Unicode, instruction fragments
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│  path / URL check │  traversal, SSRF, blocked schemes
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│  command scan     │  dangerous patterns (shell tools only)
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│  approval gate    │  user confirmation for risky ops
└─────────┬─────────┘
          │
          ▼
     tool executes
          │
          ▼
┌───────────────────┐
│  redact output    │  secrets removed before model sees result
└─────────┬─────────┘
          │
          ▼
   result returned
```

---

## Code Quality Constraint

```rust
// crates/edgecrab-security/src/lib.rs
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
```

Security code that panics on unexpected input is worse than security code that returns an error. Every function in `edgecrab-security` returns a `Result`; panics are a compile-time error.

---

## Writing a New Tool: Security Checklist

If your tool touches any of the following, use the corresponding primitive:

| Touch-point | Primitive to call |
|---|---|
| Filesystem path | `path_jail::check_path(path, &allowed_roots)` |
| URL/HTTP request | `url_safety::check_url(url)` |
| Shell command | `command_scan::scan(command)` |
| User-supplied text injected into prompts | `injection::check_injection(text)` |
| Output containing env vars / credentials | `redact::redact(output)` |
| Any high-risk operation | `approval::request(ctx, description)` |

---

## Tips

- **Re-export at crate root** — `edgecrab-security/src/lib.rs` re-exports the most common functions. `use edgecrab_security::check_path` is enough in most tools.
- **`#![deny(clippy::unwrap_used)]` is your friend** — apply it to your own tool crates too. It forces explicit error handling at the call site.
- **Don't implement your own injection detection** — character-level Unicode tricks are subtle. Use the `injection` module even for "simple" text inputs.

---

## FAQ

**Q: Does EdgeCrab sandbox tool execution at the OS level?**
A: For local execution, no kernel sandbox is applied by default. The security layer is application-level. Docker and Singularity backends provide OS-level isolation — see [`008_environments/001_environments.md`](../008_environments/001_environments.md).

**Q: Can I add custom redaction patterns?**
A: Yes. Add regex patterns to `AppConfig::privacy.redact_patterns` in `config.yaml`. They are compiled at startup and applied alongside the built-in patterns.

**Q: What happens if `command_scan` raises a risk on a legitimate command?**
A: The approval gate fires (`on_risk` mode) and the user is prompted. `AllowAlways` adds it to the permanent allow list for that profile.

---

## Cross-References

- Approval flow in the runtime → [`004_tools_system/004_tools_runtime.md`](../004_tools_system/004_tools_runtime.md)
- Execution backends (OS-level isolation) → [`008_environments/001_environments.md`](../008_environments/001_environments.md)
- Config for `approval_mode` and `redact_patterns` → [`009_config_state/001_config_state.md`](../009_config_state/001_config_state.md)
- Tool registry (where checks are called) → [`004_tools_system/001_tool_registry.md`](../004_tools_system/001_tool_registry.md)
