# edgecrab-security

> **Why this crate?** An agent that can run shell commands, fetch URLs, and write files is only  
> as safe as the checks that guard those operations. `edgecrab-security` centralises every  
> security primitive so every tool and every layer of EdgeCrab runs the same vetted checks —  
> no re-implementation, no forgotten edge cases.

Part of [EdgeCrab](https://www.edgecrab.com) — the Rust SuperAgent.

---

## What's inside

| Module | Protection | Key function |
|--------|-----------|--------------|
| `path_safety` | Directory traversal | `validate_path(path, allowed_root)` |
| `ssrf` | Server-Side Request Forgery | `is_safe_url(url)` |
| `command_scan` | Shell-injection | `scan_command(args)` |
| `injection` | Prompt injection in context files | `scan_for_injection(text)` |
| `redaction` | Secret leakage | `redact_secrets(text)` |

## Add to your crate

```toml
[dependencies]
edgecrab-security = { path = "../edgecrab-security" }
```

## Usage

```rust
use edgecrab_security::{path_safety, ssrf, command_scan};

// Guard a file write
path_safety::validate_path(&user_path, &workspace_root)?;

// Guard a web fetch
if !ssrf::is_safe_url(&url) {
    return Err(ToolError::Blocked { reason: "private IP".into() });
}

// Guard a shell argument
command_scan::scan_command(&["git", "commit", "-m", user_input])?;
```

## Security model

- **Path safety** — canonicalises both the target path and the allowed root, then asserts the former is a descendant of the latter. Symlink-safe.
- **SSRF guard** — rejects loopback, private RFC-1918 / RFC-4193 ranges, and link-local addresses (`169.254.x.x`, `fe80::/10`).
- **Injection scan** — regex + invisible-unicode + homoglyph detection on context files (AGENTS.md, SOUL.md, etc.) before they are injected into the system prompt.
- **Redaction** — strips `sk-*`, `ghp_*`, `Bearer …` patterns from LLM output before display or logging.

---

> Full docs, guides, and release notes → [edgecrab.com](https://www.edgecrab.com)
