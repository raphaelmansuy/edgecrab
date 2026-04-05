# 011.001 — Security

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 004.001 Tool Registry](../004_tools_system/001_tool_registry.md) | [→ 009 Config](../009_config_state/001_config_state.md)
> **Source**: `edgecrab-security/src/` — verified against real implementation (7 modules)

## 1. Threat Model

| Threat | Vector | Mitigation |
|--------|--------|-----------|
| Prompt injection | User input, tool results | Input scanning, output filtering |
| Command injection | Terminal tool | Command deny-list, regex scanning |
| Path traversal | File tools | Canonicalize + jail check |
| SSRF | Web tools, browser | URL allowlist, private IP block |
| Credential leak | Env vars, terminal output | Env passthrough allowlist, redaction |
| Destructive ops | rm -rf, git reset | Approval flow, destructive pattern detection |
| DoS via iteration | Infinite tool loops | Iteration budget (hard limit) |
| Supply chain | MCP servers, plugins | Sandboxing, permission model |

## 2. Approval System

Three approval modes (config `approvals.mode`):
- **manual** — always prompt the user (default)
- **smart** — use auxiliary LLM to auto-approve low-risk commands, prompt for high-risk
- **off** — skip all approval prompts (equivalent to `--yolo`)

Permanent allowlist: patterns approved with "always" are persisted to
`config.yaml` → `command_allowlist` and survive across sessions.

```rust
// edgecrab-security/src/approval.rs

pub enum ApprovalDecision {
    Approved,
    Denied,
    AlwaysApprove,     // persist to command_allowlist
    ApproveForSession, // session-scoped only
}

#[async_trait]
pub trait ApprovalHandler: Send + Sync {
    async fn request_approval(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
        reason: &str,
    ) -> Result<ApprovalDecision>;
}

/// Per-session approval state (thread-safe, keyed by session_key)
pub struct ApprovalPolicy {
    /// Shell command patterns that require approval (~30 patterns)
    pub destructive_patterns: Vec<(Regex, String)>, // (pattern, description)
    /// Session-level auto-approvals (thread-safe)
    pub session_approved: RwLock<HashMap<String, HashSet<String>>>,
    /// Permanent allowlist from config.yaml
    pub permanent_approved: RwLock<HashSet<String>>,
    /// Pattern key aliases for backward compat (legacy regex key ↔ description)
    pub key_aliases: HashMap<String, HashSet<String>>,
}

impl ApprovalPolicy {
    pub fn needs_approval(&self, tool_name: &str, args: &Value) -> bool {
        if tool_name == "terminal" {
            if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
                return self.is_destructive_command(cmd);
            }
        }
        // ... file path checks, etc.
        false
    }
}
```

### 2.1 Dangerous Command Patterns (~30 patterns)

hermes-agent detects dangerous commands across these categories:

| Category | Example patterns |
|----------|-----------------|
| Destructive file ops | `rm -r`, `rmdir`, `truncate`, `dd if=`, `shred`, `find -delete`, `xargs rm` |
| Permission escalation | `chmod 777`, `chown -R root` |
| System damage | `mkfs`, `> /dev/sd`, `> /etc/`, `systemctl stop`, `fork bomb` |
| SQL destruction | `DROP TABLE/DATABASE`, `DELETE FROM` (without WHERE), `TRUNCATE` |
| Remote code execution | `curl|sh`, `wget|bash`, `bash -c`, `python -e`, process substitution |
| Process killing | `kill -9 -1`, `pkill -9` |
| Gateway protection | `gateway run &`, `nohup gateway run` (must use systemd) |
| File overwrite via tee | `tee /etc/`, `tee .ssh/`, `tee .hermes/.env` |

### 2.2 Command Normalization (Bypass Prevention)

Before pattern matching, commands are normalized to prevent obfuscation:

```rust
/// Normalize command before dangerous-pattern matching
pub fn normalize_command_for_detection(command: &str) -> String {
    let mut cmd = strip_ansi(command);          // Strip ALL ANSI escape sequences
    cmd = cmd.replace('\x00', "");              // Strip null bytes
    cmd = unicode_normalization::nfkc(&cmd);    // Normalize fullwidth Unicode → ASCII
    cmd
}
```

This prevents bypasses via:
- ANSI escape sequence insertion (CSI, OSC, DCS, 8-bit C1)
- Null byte injection
- Unicode fullwidth Latin characters (`ｒｍ` → `rm`)

## 3. Tirith Pre-Exec Security Scanning

External binary (`tirith`) for content-level threat scanning. Exit code is
the verdict source of truth: 0 = allow, 1 = block, 2 = warn.

**Auto-install**: If tirith is not found on PATH, it is automatically
downloaded from GitHub releases to `$EDGECRAB_HOME/bin/tirith` in a
background thread (startup never blocks). Downloads always verify
**SHA-256 checksums**. When `cosign` is available, supply chain provenance
(GitHub Actions workflow signature) is also verified.

Disk-persistent failure marker (`$EDGECRAB_HOME/.tirith-install-failed`)
prevents retry across process restarts (24h TTL). If the failure was
`cosign_missing` and cosign is now on PATH, marker is auto-cleared.

Config keys: `security.tirith_enabled`, `security.tirith_path`,
`security.tirith_timeout` (default 5s), `security.tirith_fail_open` (default true).

```rust
// edgecrab-security/src/tirith.rs
pub fn scan_command(cmd: &str) -> TirithVerdict {
    // Spawn tirith subprocess → parse exit code → parse JSON findings
    ...
}
```

## 4. Path Traversal Prevention

```rust
// edgecrab-security/src/path_jail.rs

/// Resolve and validate a path against a jail directory
pub fn resolve_safe_path(path: &str, jail: &Path) -> Result<PathBuf, SecurityError> {
    let resolved = jail.join(path).canonicalize()
        .map_err(|_| SecurityError::PathTraversal(path.to_string()))?;
    if !resolved.starts_with(jail) {
        return Err(SecurityError::PathTraversal(path.to_string()));
    }
    Ok(resolved)
}
```

## 5. URL Safety (SSRF Prevention)

```rust
// edgecrab-security/src/url_safety.rs

use std::net::IpAddr;

pub fn is_safe_url(url: &str) -> Result<bool, SecurityError> {
    let parsed = url::Url::parse(url)
        .map_err(|_| SecurityError::InvalidUrl(url.to_string()))?;

    // Block non-HTTP(S) schemes
    match parsed.scheme() {
        "http" | "https" => {},
        _ => return Ok(false),
    }

    // Resolve hostname
    let host = parsed.host_str()
        .ok_or(SecurityError::InvalidUrl(url.to_string()))?;

    // Block private/reserved IPs (SSRF prevention)
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_ip(&ip) || is_loopback(&ip) || is_link_local(&ip) {
            return Ok(false);
        }
    }

    // Block known dangerous hostnames
    let blocked = ["localhost", "metadata.google.internal", "169.254.169.254"];
    if blocked.contains(&host) {
        return Ok(false);
    }

    Ok(true)
}
```

## 6. Environment Variable Passthrough

```rust
// edgecrab-security/src/env_passthrough.rs

/// Allowlist of env vars that can be forwarded to sandboxed environments
pub struct EnvPassthrough {
    allowlist: HashSet<String>,
    redact_patterns: Vec<Regex>,
}

impl EnvPassthrough {
    pub fn default_allowlist() -> Self {
        Self {
            allowlist: [
                "PATH", "HOME", "USER", "LANG", "LC_ALL",
                "TERM", "COLORTERM", "SHELL",
                // Project-specific
                "NODE_ENV", "PYTHONPATH", "RUST_LOG",
            ].iter().map(|s| s.to_string()).collect(),
            redact_patterns: vec![
                Regex::new(r"(?i)(api_key|secret|token|password|auth)").unwrap(),
            ],
        }
    }

    pub fn filter(&self, env: &HashMap<String, String>) -> HashMap<String, String> {
        env.iter()
            .filter(|(k, _)| self.allowlist.contains(k.as_str()))
            .filter(|(k, _)| !self.redact_patterns.iter().any(|r| r.is_match(k)))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}
```

## 7. Output Redaction

Comprehensive regex-based secret redaction for logs and tool output.
Short tokens (<18 chars) are fully masked. Longer tokens preserve
first 6 and last 4 characters for debuggability.

```rust
// edgecrab-security/src/redact.rs

/// 20+ API key prefix patterns
static PREFIX_PATTERNS: &[&str] = &[
    r"sk-[A-Za-z0-9_-]{10,}",         // OpenAI / OpenRouter / Anthropic
    r"ghp_[A-Za-z0-9]{10,}",          // GitHub PAT (classic)
    r"github_pat_[A-Za-z0-9_]{10,}",  // GitHub PAT (fine-grained)
    r"xox[baprs]-[A-Za-z0-9-]{10,}",  // Slack tokens
    r"AIza[A-Za-z0-9_-]{30,}",        // Google API keys
    r"pplx-[A-Za-z0-9]{10,}",         // Perplexity
    r"fal_[A-Za-z0-9_-]{10,}",        // Fal.ai
    r"fc-[A-Za-z0-9]{10,}",           // Firecrawl
    r"bb_live_[A-Za-z0-9_-]{10,}",    // BrowserBase
    r"gAAAA[A-Za-z0-9_=-]{20,}",      // Codex encrypted tokens
    r"AKIA[A-Z0-9]{16}",              // AWS Access Key ID
    r"sk_live_[A-Za-z0-9]{10,}",      // Stripe (live)
    r"sk_test_[A-Za-z0-9]{10,}",      // Stripe (test)
    r"SG\.[A-Za-z0-9_-]{10,}",        // SendGrid
    r"hf_[A-Za-z0-9]{10,}",           // HuggingFace
    r"npm_[A-Za-z0-9]{10,}",          // npm
    r"pypi-[A-Za-z0-9_-]{10,}",       // PyPI
    r"am_[A-Za-z0-9_-]{10,}",         // AgentMail
    // ... + more
];

/// Additional redaction layers beyond prefix patterns:
/// - ENV assignments: KEY_WITH_SECRET=value
/// - JSON fields: "apiKey": "value", "token": "value"
/// - Authorization headers: Bearer <token>
/// - Telegram bot tokens: bot<digits>:<token>
/// - Private key blocks: -----BEGIN RSA PRIVATE KEY-----
/// - Database connection strings: postgres://user:PASSWORD@host
/// - E.164 phone numbers (when privacy.redact_pii is enabled)
pub fn redact_sensitive(text: &str) -> String { ... }

/// Custom log formatter that applies redaction to all log records
pub struct RedactingFormatter;
```

## 8. Website Blocklist (Domain Policy)

Configurable domain-level access control for web tools. Supports:
- Inline domain list in config.yaml (`security.website_blocklist.domains`)
- Shared blocklist files (`security.website_blocklist.shared_files`)
- Wildcard pattern matching (`*.example.com`)
- Host normalization (strip `www.`, lowercase)
- TTL-based cache with `invalidate_cache()` for hot reload

```rust
// edgecrab-security/src/website_policy.rs

pub struct WebsitePolicy {
    domains: HashSet<String>,
    shared_files: Vec<PathBuf>,
    enabled: bool,
}

impl WebsitePolicy {
    /// Check if a URL/domain is blocked. Returns None if allowed.
    pub fn check_access(&self, url: &str) -> Option<BlockReason> {
        if !self.enabled { return None; }
        let host = extract_host(url);
        let normalized = normalize_host(&host);
        // Check exact match, then wildcard sub-patterns
        ...
    }
}
```

## 9. Rust Advantage — Security

| Aspect | hermes-agent (Python) | EdgeCrab (Rust) |
|--------|----------------------|-----------------|
| Memory safety | GC-managed, but C extension vulnerabilities | Guaranteed by borrow checker |
| Command injection | Regex-based, bypassable | Regex + structured command builder |
| Path traversal | `os.path.realpath` (race condition) | `canonicalize` + jail check |
| Type confusion | Runtime type checks | Compile-time type safety |
| Dependency audit | pip audit (manual) | `cargo audit` (automated in CI) |
| Binary hardening | N/A (interpreted) | ASLR, stack canaries, `-C opt-level=3` |

## 10. Provider-Scoped Credential Isolation (v0.4.0)

Prevent API keys from leaking across provider boundaries:

```rust
// edgecrab-security/src/credentials.rs

pub struct ProviderCredential {
    provider: String,
    key: SecretString,        // zeroize on drop
    allowed_base_urls: Vec<String>,
}

impl ProviderCredential {
    /// Only allow credential use against permitted base URLs
    pub fn authorize_request(&self, base_url: &str) -> Result<&str, SecurityError> {
        if self.allowed_base_urls.iter().any(|u| base_url.starts_with(u)) {
            Ok(self.key.expose_secret())
        } else {
            Err(SecurityError::CredentialScopeMismatch {
                provider: self.provider.clone(),
                attempted_url: base_url.to_string(),
            })
        }
    }
}
```

## 11. @ Context Reference Security

Block injection of sensitive paths via `@file` references:

```rust
/// Paths blocked from @ context reference injection
const BLOCKED_CONTEXT_PATTERNS: &[&str] = &[
    ".env", ".env.local", ".env.production",
    ".ssh/", ".gnupg/", ".aws/credentials",
    ".netrc", ".pgpass", ".npmrc", ".pypirc",
    "credentials.json", "token.json",
];

pub fn is_context_ref_allowed(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    !BLOCKED_CONTEXT_PATTERNS.iter().any(|p| path_str.contains(p))
}
```

## 12. MCP Server Sandboxing

```rust
// edgecrab-security/src/mcp_sandbox.rs

pub struct McpPermissions {
    pub allowed_tools: Option<HashSet<String>>,   // None = all allowed
    pub blocked_tools: HashSet<String>,
    pub max_calls_per_minute: u32,
    pub allow_network: bool,
    pub allow_filesystem: bool,
}

/// Validate MCP tool call against permissions before dispatch
pub fn check_mcp_permission(
    server: &str,
    tool: &str,
    perms: &McpPermissions,
) -> Result<(), SecurityError> {
    if perms.blocked_tools.contains(tool) {
        return Err(SecurityError::McpToolBlocked { server: server.into(), tool: tool.into() });
    }
    if let Some(allowed) = &perms.allowed_tools {
        if !allowed.contains(tool) {
            return Err(SecurityError::McpToolNotAllowed { server: server.into(), tool: tool.into() });
        }
    }
    Ok(())
}
```

## 13. Gateway Session Worktree Isolation

Each gateway session gets an isolated working directory to prevent
cross-session file interference:

```rust
pub fn create_session_worktree(session_id: &str) -> Result<PathBuf> {
    let base = edgecrab_home().join("worktrees");
    let worktree = base.join(session_id);
    std::fs::create_dir_all(&worktree)?;
    Ok(worktree)
}

/// Cleanup stale worktrees older than max_age
pub fn cleanup_stale_worktrees(max_age: Duration) -> Result<usize> {
    let base = edgecrab_home().join("worktrees");
    let mut cleaned = 0;
    for entry in std::fs::read_dir(&base)? {
        let entry = entry?;
        if let Ok(meta) = entry.metadata() {
            if meta.modified()?.elapsed()? > max_age {
                std::fs::remove_dir_all(entry.path())?;
                cleaned += 1;
            }
        }
    }
    Ok(cleaned)
}
```
