---
title: Security Model
description: EdgeCrab's defense-in-depth security architecture. Path traversal prevention, SSRF guards, Aho-Corasick command scanning, output redaction, and safe defaults. Grounded in crates/edgecrab-security/.
sidebar:
  order: 5
---

Security is compiled into EdgeCrab — not runtime configuration switches.
Every tool execution passes through a 6-layer defense stack before any
file is read, any command is run, or any URL is fetched.

---

## Threat Model

EdgeCrab is an autonomous agent: it reads files, runs shell commands, and
makes HTTP requests on your behalf. The primary risks are:

```
Risk                  Vector
-------------------------------------------------------
Path traversal        LLM reads /etc/passwd or ~/.ssh/id_rsa
SSRF                  Web tool fetches 169.254.169.254 (cloud IMDS)
Command injection     Malicious file content escapes to shell args
Secret exfiltration   LLM output contains API keys from read files
Prompt injection      Web page / file contains hidden LLM instructions
Unsafe approval       Agent runs destructive op without user review
```

---

## Layer 1 — Path Safety (`path_jail.rs`)

Every file read / write / list / search passes through `path_jail`:

1. Resolve all symlinks and collapse `..` (canonicalize)
2. Verify the canonical path starts with one of the allowed roots
3. Reject the call if it falls outside allowed roots

The active workspace root is always allowed. Add extra roots explicitly in `config.yaml`:

```yaml
tools:
  file:
    allowed_roots:
      - /Users/you/projects/myapp   # only this project
```

This policy is enforced at runtime by the shared path resolver used by
file tools, local vision image reads, and `@file` / `@folder` context refs.
It is not a general terminal sandbox: shell commands can still access any
path the selected execution backend can reach.

---

## Layer 2 — SSRF Guard (`url_safety.rs`)

All outbound HTTP calls (web search, web extract, web crawl, browser
navigation) resolve the hostname to IPs and check *those resolved IPs*
against a blocklist — preventing DNS-rebinding bypasses.

Blocked address space:

| Range | Category |
|-------|---------|
| `10.0.0.0/8` | RFC 1918 private |
| `172.16.0.0/12` | RFC 1918 private |
| `192.168.0.0/16` | RFC 1918 private |
| `127.0.0.0/8` | Loopback |
| `::1/128` | IPv6 loopback |
| `169.254.0.0/16` | Link-local / AWS / GCP / Azure IMDS |
| `fd00::/8` | IPv6 ULA |
| `100.64.0.0/10` | Carrier-grade NAT |
| `*.internal`, `metadata.google.internal` | Cloud-provider metadata FQDNs |

`SafeUrl` is a distinct Rust type. No HTTP call is made without first
constructing a `SafeUrl` — building one panics at compile time when
bypassed, and returns an error at runtime on blocked addresses.

**Cannot be disabled** — the SSRF guard is compiled unconditionally.

---

## Layer 3 — Command Scanner (`command_scan.rs`)

Before any shell command executes via the terminal tool, the
`CommandScanner` runs two passes over the **normalized** command string
(ANSI-stripped, NFKC-normalized, lowercased):

**Pass 1 — Aho-Corasick O(n) literal scan**

38 literal patterns across 8 danger categories:

| Category | Example patterns |
|----------|-----------------|
| `DestructiveFileOps` | `rm -r`, `rm -f`, `rmdir`, `shred`, `find -delete`, `dd if=`, `-exec rm` |
| `PermissionEscalation` | `chmod 777`, `chown -R root` |
| `SystemDamage` | `mkfs`, `> /dev/sd`, `> /etc/`, `systemctl stop`, `:(){ |` (fork bomb) |
| `SqlDestruction` | `drop table`, `drop database`, `truncate table` |
| `RemoteCodeExecution` | `| bash`, `|sh`, `bash -c `, `python -c `, `node -e `, `perl -e ` |
| `ProcessKilling` | `kill -9 -1`, `pkill -9` |
| `GatewayProtection` | `gateway run &`, `nohup ... gateway` |
| `FileOverwrite` | `tee /etc/`, `tee .ssh/` |

**Pass 2 — Regex scan for non-contiguous patterns**

Patterns that require lookahead or non-adjacent keywords:

- `DELETE FROM` without a `WHERE` clause (SQL)
- `find PATH -exec rm` (non-adjacent)
- `bash <(curl ...)` / `sh < <(wget ...)` (process substitution)
- `bash -lc`, `sh -ic` (combined -c flag forms)
- Gateway run outside systemd (`nohup ... disown`)

Tool-generated commands containing flagged patterns are **blocked** and
the LLM receives a rejection message. User-typed commands in the TUI are
not scanned — the user is trusted.

---

## Layer 4 — Output Redaction (`redact.rs`)

All LLM text — before display and before logging — passes through the
redaction pipeline. Matched patterns are replaced with `[REDACTED]`:

| Pattern | Matches |
|---------|---------|
| `sk-[A-Za-z0-9]{20,}` | OpenAI API keys |
| `ghp_[A-Za-z0-9]{36}` | GitHub personal access tokens |
| `github_pat_[A-Za-z0-9_]{80,}` | Fine-grained GitHub PATs |
| `AKIA[A-Z0-9]{16}` | AWS access key IDs |
| `-----BEGIN .* PRIVATE KEY-----` | PEM private keys |
| High-entropy strings > 40 chars | Generic secrets (base64-like) |

The raw API response is never written to disk. The SQLite session
database stores only the redacted form.

---

## Layer 5 — Prompt Injection Detection (`injection.rs`)

Tool results (file contents, web pages, process output) are scanned for
prompt-injection markers before being injected into the LLM context:

- `Ignore (all|previous|above) instructions`
- `You are now` / `Act as` override attempts
- Hidden Unicode direction overrides (BiDi)
- `<|im_start|>` / `<|system|>` token injections

Detected injections are:
1. Flagged with a warning banner in the TUI
2. Wrapped in a sandbox comment so the LLM knows the content is
   untrusted user data, not system instructions

---

## Layer 6 — Approval Policy (`approval.rs`)

Destructive operations can require explicit user approval before
execution. Three modes:

| Mode | Behavior |
|------|---------|
| `off` | No approval required (default in interactive TUI) |
| `smart` | Approve when risk score exceeds threshold |
| `manual` | All tool calls require `/approve` or inline button |

In gateway platforms (Telegram, Discord), approval appears as inline
buttons. In the TUI, `/approve` and `/deny` slash commands confirm or
reject pending actions.

---

## State Integrity

The SQLite session database uses WAL (Write-Ahead Logging) mode:
- Concurrent reads never block writes
- Crash-safe: power failure mid-write cannot corrupt the database
- Integrity verified on startup via `PRAGMA integrity_check`

---

## Principle of Least Privilege — Config Template

```yaml
# ~/.edgecrab/config.yaml
tools:
  file:
    allowed_roots:
      - /path/to/your/project   # never broader than needed
  terminal:
    timeout_seconds: 10          # short timeout = smaller blast radius
    allowed_commands:            # optional allowlist
      - cargo
      - git
      - grep
  web:
    max_results: 5

security:
  path_restrictions: []          # optional deny-list inside the workspace/allowed roots
  ssrf_guard: true               # cannot be false — compile-time guard
  command_scan: true
  output_redaction: true
  approval_required: []          # add destructive command patterns here
```

---

## Reporting Security Issues

Report vulnerabilities privately via [GitHub Security Advisories](https://github.com/raphaelmansuy/edgecrab/security/advisories/new). Do not open a public issue for security bugs.

---

## Security in Practice: Common Scenarios

**Scenario 1: Constraining a refactoring task**
```yaml
# Only allow reading and writing inside the project
tools:
  file:
    allowed_roots:
      - /home/you/project
```
If the agent tries to read `/etc/hosts` or any file outside the workspace plus `allowed_roots`, the call is blocked by the shared path policy.

**Scenario 2: Review before any shell command**
```yaml
security:
  approval_required:
    - "rm"
    - "git push"
```
Matching terminal commands require approval before running. Non-matching commands follow the normal terminal policy.

**Scenario 3: Block all network access**
```yaml
tools:
  enabled_toolsets:
    - file
    - terminal
    - memory
```
No web/browser toolsets → no HTTP calls from agent tools. The SSRF guard still protects terminal commands that happen to make HTTP requests.

**Scenario 4: Team deployment — lock down messaging**
```yaml
gateway:
  telegram:
    allowed_users:
      - alice
      - bob
    home_channel: "-100123456789"
  slack:
    allowed_users:
      - U012ABCDE
      - U098FGHIJ
```
Only explicitly listed users can interact with the agent. All others receive an "access denied" response.

---

## Pro Tips

**Prefer `allowed_roots` over relying on agent judgment.** The agent is smart, but the path policy is deterministic runtime enforcement. Always set the smallest roots you actually need.

**Run `edgecrab doctor` after changing security config.** It checks that your settings are valid before you rely on them.

**Log everything in production.** Set `save_trajectories: true` in config.yaml. All tool calls and results are saved as JSON trajectory files in `~/.edgecrab/trajectories/`. Invaluable for post-incident review.

---

## Frequently Asked Questions

**Q: Can the agent read my SSH keys or credentials?**

Not if the workspace root and `allowed_roots` exclude them. By default, file tools can read only inside the active workspace. To keep credentials inaccessible even inside a broad workspace, also add `security.path_restrictions` entries for sensitive subtrees.
```yaml
tools:
  file:
    allowed_roots:
      - /home/you/your-project
```

**Q: Can a malicious web page take control of the agent?**

EdgeCrab implements prompt injection detection (Layer 5). Content containing common injection markers is wrapped in a sandbox comment and flagged. This is defense-in-depth — do not rely on it as the only control. Never point the agent at untrusted web pages without an explicit approval policy.

**Q: Can the SSRF guard be disabled?**

No. It's compiled unconditionally. `SafeUrl` is a Rust type — constructing an HTTP request without first constructing a `SafeUrl` is a compile error. Runtime configuration can't disable this.

**Q: What happens if a command scanner match is a false positive?**

User-typed commands in the TUI are never scanned — only agent-generated commands. If the agent is blocked from a legitimate command:
1. Type the command yourself in the TUI instead
2. Or temporarily add it to `terminal.allowed_commands`

**Q: Does EdgeCrab send any data to external servers by default?**

Only to the configured LLM API. No telemetry, no crash reporting, no analytics. Network calls are limited to: LLM provider API, web tools (if enabled), gateway platforms (if configured), and Honcho (if cloud sync is enabled).

---

## See Also

- [Configuration](/user-guide/configuration/) — Full `security.*` config reference
- [Docker Deployment](/user-guide/docker/) — Additional container security
- [Architecture](/developer/architecture/) — How security crates are structured in the codebase
