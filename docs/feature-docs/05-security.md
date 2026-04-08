
# Security & Defenses (Deep Dive)

EdgeCrab implements **multi-layer security** to protect the host, user data, and agent integrity. Key layers:

## 1. Path Policy

- [`path_policy.rs`](../../crates/edgecrab-security/src/path_policy.rs): Enforces workspace root, allowed/denied roots, virtual tmp roots. All file access (read/write) is mediated by this policy.
- Prevents path traversal, symlink escape, and unauthorized file access.

## 2. SSRF Guard

- [`ssrf.rs`](../../crates/edgecrab-security/src/ssrf.rs): Blocks requests to private IPs (10.x, 192.168.x, 127.x, ::1, etc.) in all web tools.
- Prevents server-side request forgery and local network attacks.

## 3. Command Scanner

- [`command_scan.rs`](../../crates/edgecrab-security/src/command_scan.rs): Scans all shell commands for dangerous patterns (destructive ops, privilege escalation, remote code exec, SQL destruction, process killing, file overwrite, etc.).
- Uses Aho-Corasick + regex, normalizes input to catch obfuscation.

## 4. Prompt Injection Detection

- All context files (AGENTS.md, SOUL.md, skills, etc.) are scanned for prompt injection patterns before injection. Blocked files are replaced with `[BLOCKED: ...]` placeholder.
- Tool/memory writes are scanned for injection before persisting.

## 5. Skill Threat Scanner

- [`skills_guard.rs`](../../crates/edgecrab-tools/src/tools/skills_guard.rs): All external skills are scanned before installation (regex-based static analysis for exfiltration, injection, destructive, persistence, network, obfuscation).
- Verdict: safe / caution / dangerous, with detailed findings and severity.

## 6. Output Redaction

- [`redact.rs`](../../crates/edgecrab-security/src/redact.rs): Strips API keys, tokens, secrets from all tool/terminal output and logs. Covers 20+ key patterns (OpenAI, Anthropic, GitHub, Slack, Google, etc.).

## 7. Container/Cloud Isolation

- Sandboxing via Docker, Modal, Daytona, Singularity, SSH (see [sandboxing.md](01-sandboxing.md)).
- Local backend is protected by static guards; container/cloud backends provide OS-level isolation.

## 8. Memory & State Isolation

- Each agent/session has its own process table and tool context; no cross-session state sharing.
- State DB uses SQLite WAL + integrity checks.

## Policy Notes & Limitations

- **No macOS Seatbelt/App Sandbox**: Unlike EdgeCode, EdgeCrab does not use OS-level policy on macOS; all isolation is via containers/cloud or static guards.
- **No network proxy enforcement**: SSRF guard blocks private IPs, but there is no HTTP proxy for fine-grained network filtering (EdgeCode has this for network sandbox mode).
- **No seccomp**: No syscall filtering on local backend; rely on command scanner and path policy.

## Key Code & Docs

- [redact.rs](../../crates/edgecrab-security/src/redact.rs)
- [path_policy.rs](../../crates/edgecrab-security/src/path_policy.rs)
- [ssrf.rs](../../crates/edgecrab-security/src/ssrf.rs)
- [command_scan.rs](../../crates/edgecrab-security/src/command_scan.rs)
- [skills_guard.rs](../../crates/edgecrab-tools/src/tools/skills_guard.rs)

---
**TODOs:**
- Evaluate adding macOS Seatbelt/App Sandbox for parity with EdgeCode
- Consider HTTP proxy for network sandboxing
- Add seccomp or syscall filtering for local backend
