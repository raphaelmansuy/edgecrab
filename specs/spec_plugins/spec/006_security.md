# Security Model — Threat Model, Scanning, Trust Policy

**Status:** PROPOSED  
**Version:** 0.1.0  
**Date:** 2026-04-09  
**Cross-refs:** [000_overview], [003_manifest], [005_lifecycle], [007_registry], [008_host_api]

---

## 1. Threat Model

### 1.1 Threat Actors

| Actor | Capability | Goal |
|---|---|---|
| Malicious community plugin | Arbitrary code in subprocess | Exfiltrate API keys, destroy files |
| Compromised trusted plugin | Code with fewer restrictions | Same + harder to detect |
| Prompt-injected plugin content | Control agent system prompt | Redirect agent behavior |
| Rhai script with hidden logic | In-process code | Bypass host restrictions |
| Hub index tampering (MitM) | Serve modified plugin files | Install backdoor |
| Plugin after social engineering | User-approved dangerous plugin | Same as malicious |

### 1.2 Trust Boundary

```
 ┌───────────────────────────────────────────────────────────────────┐
 │  TRUSTED ZONE (edgecrab agent process)                            │
 │                                                                   │
 │   ~/.edgecrab/config.yaml    ← not writable by plugins           │
 │   ~/.edgecrab/memories/      ← writable via host:memory_write    │
 │   ~/.edgecrab/sessions.db    ← read-only via host:session_search │
 │   Agent secrets / env vars   ← readable via host:secret_get ONLY │
 │                               ← subprocess env never includes    │
 │                                  ANTHROPIC_API_KEY etc.          │
 │                                                                   │
 └──────────────────────┬────────────────────────────────────────────┘
                        │  strictly controlled host API
                        │  (capabilities declared in plugin.toml)
 ┌──────────────────────▼────────────────────────────────────────────┐
 │  UNTRUSTED ZONE (plugin subprocess)                               │
 │                                                                   │
 │   Can only:                                                       │
 │    • read/write files listed in allowed_paths                     │
 │    • call hosts in allowed_hosts (SSRF-checked)                   │
 │    • call host API functions declared in capabilities.host        │
 │    • spawn its own sub-subprocesses (NOT restricted — see §2.3)  │
 │                                                                   │
 │   Cannot:                                                         │
 │    • access ANTHROPIC_API_KEY or other agent secrets              │
 │    • read ~/.edgecrab/config.yaml                                 │
 │    • write outside allowed_paths                                  │
 │    • call undeclared host API methods (PermissionDenied)          │
 └───────────────────────────────────────────────────────────────────┘
```

### 1.3 Known Limitations (Accepted Risks)

- **Subprocess cannot be fully sandboxed without OS-level jails** (macOS Seatbelt / Linux
  Landlock / namespaces). Phase 1 uses *static analysis* (scanning) + *policy enforcement*
  (SSRF guard on host-API calls) as a defense-in-depth layer. OS sandboxing is Phase 2.
- **Rhai scripts run in-process** — a malicious Rhai script could theoretically call
  `host:secret_get` for a secret the plugin does not need, if the user approved the wrong
  capabilities. Mitigation: `secret_get` is logged and auditable; capability list is shown
  at install time.
- **Plugins can spawn sub-processes** — the subprocess policy cannot prevent a Python plugin
  from `import subprocess; subprocess.run("rm -rf /", shell=True)`. Mitigation: static
  scanning detects common destructive patterns; Phase 2 adds OS-level containment.

---

## 2. Static Analysis Scanner

### 2.1 Architecture

The plugin scanner is an extension of `edgecrab-tools/src/tools/skills_guard.rs`.
It performs **multi-file, multi-language static analysis** using regex pattern matching.

```
PluginSecurityScanner::scan(plugin_dir, source)
     │
     ├── walk plugin_dir recursively
     ├── for each text file (by extension):
     │     ├── read content (skip files > 1 MiB)
     │     ├── scan line-by-line against THREAT_PATTERNS
     │     └── collect Finding { pattern_id, severity, category, file, line, match }
     │
     ├── compute verdict:
     │     any Critical/High finding → "dangerous"
     │     any Medium finding        → "caution"
     │     Low only                 → "safe"
     │
     └── return ScanResult { skill_name, source, trust_level, verdict, findings }
```

### 2.2 Threat Pattern Catalog

Source-verified from `skills_guard.py THREAT_PATTERNS` (April 2026).
All 10 standard categories + 2 additional categories; ~90 patterns total.

#### Category: exfiltration

| Pattern ID | Description | Severity |
|---|---|---|
| `env_exfil_curl` | curl/wget/fetch/httpx/requests with secret env vars | Critical |
| `env_exfil_wget` | wget with secret env vars | Critical |
| `env_exfil_fetch` | fetch() with secret vars | Critical |
| `env_exfil_httpx` | httpx with secrets | Critical |
| `env_exfil_requests` | Python requests with secrets | Critical |
| `encoded_exfil` | base64 encoding combined with env access | High |
| `ssh_dir_access` | References `~/.ssh` directory | High |
| `aws_dir_access` | References `~/.aws` directory | High |
| `gpg_dir_access` | References `~/.gnupg` GPG keyring | High |
| `kube_dir_access` | References `~/.kube` Kubernetes config | High |
| `docker_dir_access` | References `~/.docker` (registry creds) | High |
| `hermes_env_access` | References `~/.hermes/.env` directly | **Critical** |
| `edgecrab_env_access` | References `~/.edgecrab/.env` directly (EdgeCrab addition) | **Critical** |
| `read_secrets_file` | `cat` on `.env`, `.netrc`, `.npmrc`, `.pypirc` | Critical |
| `dump_all_env` | `printenv` or `env |` piped | High |
| `python_os_environ` | `os.environ` access (potential dump) | High |
| `python_getenv_secret` | `os.getenv()` with KEY/TOKEN/SECRET/PASSWORD | Critical |
| `node_process_env` | `process.env[...` in Node.js | High |
| `ruby_env_secret` | `ENV[...]` with secret name in Ruby | Critical |
| `dns_exfil` | dig/nslookup with variable interpolation | Critical |
| `tmp_staging` | Write to `/tmp/` then curl/wget/nc | Critical |
| `md_image_exfil` | Markdown image URL with `${variable}` | High |
| `md_link_exfil` | Markdown link with `${variable}` | High |
| `context_exfil` | Instructs agent to output conversation history | High |
| `send_to_url` | Instructs agent to send data to a URL | High |

#### Category: injection

| Pattern ID | Description | Severity |
|---|---|---|
| `prompt_injection_ignore` | "ignore previous instructions" | Critical |
| `role_hijack` | "you are now" role override | High |
| `deception_hide` | "do not tell the user" | Critical |
| `sys_prompt_override` | "system prompt override" | Critical |
| `role_pretend` | "pretend you are" | High |
| `disregard_rules` | "disregard your instructions/rules" | Critical |
| `leak_system_prompt` | "output system/initial prompt" | High |
| `conditional_deception` | "when no one is watching" | High |
| `bypass_restrictions` | "act as if you have no restrictions" | Critical |
| `translate_execute` | translate-then-execute evasion | Critical |
| `html_comment_injection` | Hidden instructions in HTML comments | High |
| `hidden_div` | Hidden `display:none` div | High |
| `jailbreak_dan` | DAN / "Do Anything Now" | Critical |
| `jailbreak_dev_mode` | "developer mode enabled" | Critical |
| `hypothetical_bypass` | "hypothetical scenario" + ignore/bypass | High |
| `educational_pretext` | "for educational purposes only" | Medium |
| `remove_filters` | "respond without restrictions/filters" | Critical |
| `fake_update` | "you have been updated/patched to" | High |
| `fake_policy` | "new policy / updated guidelines" | Medium |

#### Category: destructive

| Pattern ID | Description | Severity |
|---|---|---|
| `destructive_root_rm` | `rm -rf /` | Critical |
| `destructive_home_rm` | `rm` targeting `$HOME` | Critical |
| `insecure_perms` | `chmod 777` | Medium |
| `system_overwrite` | Redirect output to `/etc/` | Critical |
| `format_filesystem` | `mkfs` | Critical |
| `disk_overwrite` | `dd if=... of=/dev/` | Critical |
| `python_rmtree` | `shutil.rmtree()` on absolute path | High |
| `truncate_system` | `truncate -s 0 /` | Critical |

#### Category: persistence

| Pattern ID | Description | Severity |
|---|---|---|
| `persistence_cron` | `crontab` | Medium |
| `shell_rc_mod` | `.bashrc`, `.zshrc`, `.profile` etc. | Medium |
| `ssh_backdoor` | `authorized_keys` | Critical |
| `ssh_keygen` | `ssh-keygen` | Medium |
| `systemd_service` | `systemd` service or `systemctl enable` | Medium |
| `init_script` | `/etc/init.d/` | Medium |
| `macos_launchd` | `launchctl load`, `LaunchAgents`, `LaunchDaemons` | Medium |
| `sudoers_mod` | `/etc/sudoers` or `visudo` | Critical |
| `git_config_global` | `git config --global` | Medium |
| `agent_config_mod` | Writes `AGENTS.md`, `CLAUDE.md`, `.cursorrules` | Critical |
| `hermes_config_mod` | References `.hermes/config.yaml` or `.hermes/SOUL.md` | Critical |
| `other_agent_config` | References `.claude/settings`, `.codex/config` | High |

#### Category: network

| Pattern ID | Description | Severity |
|---|---|---|
| `reverse_shell` | `nc -lp`, `ncat`, `socat` listener | Critical |
| `tunnel_service` | ngrok, localtunnel, serveo, cloudflared | High |
| `hardcoded_ip_port` | Hardcoded `1.2.3.4:PORT` | Medium |
| `bind_all_interfaces` | `0.0.0.0:PORT` or `INADDR_ANY` | High |
| `bash_reverse_shell` | `/bin/sh -i` via `/dev/tcp/` | Critical |
| `python_socket_oneliner` | `python -c 'import socket'` one-liner | Critical |
| `python_socket_connect` | `socket.connect((` | High |
| `exfil_service` | webhook.site, requestbin, pipedream | High |
| `paste_service` | pastebin, hastebin, ghostbin | Medium |

#### Category: obfuscation

| Pattern ID | Description | Severity |
|---|---|---|
| `base64_decode_pipe` | `base64 -d \|` piped to execution | High |
| `hex_encoded_string` | Multiple `\xNN` hex sequences | Medium |
| `eval_string` | `eval("...")` | High |
| `exec_string` | `exec("...")` | High |
| `echo_pipe_exec` | `echo ... \| bash/sh/python` | Critical |
| `python_compile_exec` | `compile(..., 'exec')` | High |
| `python_getattr_builtins` | `getattr(__builtins__,...)` | High |
| `python_import_os` | `__import__('os')` | High |
| `python_codecs_decode` | `codecs.decode("...")` | Medium |
| `js_char_code` | `String.fromCharCode` / `charCodeAt` | Medium |
| `js_base64` | `atob()` / `btoa()` | Medium |
| `string_reversal` | `[::-1]` slice | Low |
| `chr_building` | `chr(N) + chr(N)` chain | High |
| `unicode_escape_chain` | Chain of `\uNNNN` sequences | Medium |

#### Category: execution *(source-verified, was missing from earlier spec)*

| Pattern ID | Description | Severity |
|---|---|---|
| `python_subprocess` | `subprocess.run/call/Popen/check_output(` | Medium |
| `python_os_system` | `os.system(` — unguarded shell | High |
| `python_os_popen` | `os.popen(` — shell pipe | High |
| `node_child_process` | `child_process.exec/spawn/fork(` | High |
| `java_runtime_exec` | `Runtime.getRuntime().exec(` | High |
| `backtick_subshell` | Backtick with `$(...)` inside | Medium |

#### Category: traversal *(source-verified, was missing from earlier spec)*

| Pattern ID | Description | Severity |
|---|---|---|
| `path_traversal_deep` | `../../..` (3+ levels) | High |
| `path_traversal` | `../..` (2 levels) | Medium |
| `system_passwd_access` | `/etc/passwd` or `/etc/shadow` | Critical |
| `proc_access` | `/proc/self` or `/proc/PID/` | High |
| `dev_shm` | `/dev/shm/` (shared memory staging) | Medium |

#### Category: mining *(source-verified, was missing from earlier spec)*

| Pattern ID | Description | Severity |
|---|---|---|
| `crypto_mining` | xmrig, `stratum+tcp`, monero, coinhive, cryptonight | Critical |
| `mining_indicators` | hashrate, nonce+difficulty | Medium |

#### Category: supply_chain *(source-verified, was missing from earlier spec)*

| Pattern ID | Description | Severity |
|---|---|---|
| `curl_pipe_shell` | `curl ... \| bash` | Critical |
| `wget_pipe_shell` | `wget -O - \| bash` | Critical |
| `curl_pipe_python` | `curl ... \| python` | Critical |
| `pep723_inline_deps` | PEP 723 `# /// script` with dependencies | Medium |
| `unpinned_pip_install` | `pip install` without `==` version pin | Medium |
| `unpinned_npm_install` | `npm install` without `@version` | Medium |
| `uv_run` | `uv run` (auto-installs unpinned deps) | Medium |
| `remote_fetch` | curl/wget/requests.get/fetch of `https://` URL | Medium |
| `git_clone` | `git clone` at runtime | Medium |
| `docker_pull` | `docker pull` at runtime | Medium |

#### Category: privilege_escalation

| Pattern ID | Description | Severity |
|---|---|---|
| `allowed_tools_field` | Skill declares `allowed-tools:` field (pre-approves access) | High |
| `sudo_usage` | `sudo` | High |
| `setuid_setgid` | `setuid`, `setgid`, `cap_setuid` | Critical |
| `nopasswd_sudo` | `NOPASSWD` sudoers entry | Critical |
| `suid_bit` | `chmod [u+]s` (SUID/SGID bit) | Critical |

#### Category: credential_exposure

| Pattern ID | Description | Severity |
|---|---|---|
| `hardcoded_secret` | `api_key = "XXXX"` pattern (20+ chars) | Critical |
| `embedded_private_key` | `-----BEGIN RSA PRIVATE KEY-----` | Critical |
| `github_token_leaked` | `ghp_...` or `github_pat_...` pattern | Critical |
| `openai_key_leaked` | `sk-...` pattern | Critical |
| `anthropic_key_leaked` | `sk-ant-...` pattern | Critical |
| `aws_access_key_leaked` | `AKIA...` (16 uppercase chars) | Critical |

### 2.3 Extension Points (DRY with skills_guard.rs)

The plugin scanner is implemented as a SUPERSET of skills_guard.rs:

```rust
// In edgecrab-plugins crate:
use edgecrab_tools::tools::skills_guard::{
    THREAT_PATTERNS, Finding, Verdict, ScanResult, ThreatCategory, Severity
};

pub struct PluginSecurityScanner {
    // Inherits all THREAT_PATTERNS from skills_guard
    extra_patterns: Vec<ThreatPattern>,  // plugin-specific patterns
}

impl PluginSecurityScanner {
    pub fn new() -> Self {
        Self {
            extra_patterns: PLUGIN_EXTRA_PATTERNS.to_vec(),
        }
    }

    pub fn scan(&self, plugin_dir: &Path, source: &str) -> ScanResult {
        // Combines skills_guard::scan() patterns + extra_patterns
    }
}
```

This satisfies DRY: skills_guard.rs is NOT duplicated; plugin scanning ADDS to it.

---

## 3. Trust Policy

### 3.1 Trust Levels

| Level | Source Examples | Assigned By |
|---|---|---|
| `builtin` | Ships with edgecrab binary | Hardcoded in installer |
| `trusted` | raphaelmansuy/edgecrab, NousResearch/hermes-agent | Hub source definition |
| `community` | skills.sh, any GitHub repo not in trusted list | Default |
| `agent-created` | Generated by `plugin_manage` tool | Installer |

### 3.2 Install Policy Matrix

Source-verified from `skills_guard.py INSTALL_POLICY`:

```python
INSTALL_POLICY = {
    "builtin":       ("allow",  "allow",  "allow"),
    "trusted":       ("allow",  "allow",  "block"),
    "community":     ("allow",  "block",  "block"),
    "agent-created": ("allow",  "allow",  "ask"),
}
```

| Trust Level | Safe Verdict | Caution Verdict | Dangerous Verdict |
|---|---|---|---|
| `builtin` | AUTO-ALLOW | AUTO-ALLOW | AUTO-ALLOW |
| `trusted` | AUTO-ALLOW | AUTO-ALLOW | BLOCKED |
| `community` | AUTO-ALLOW | BLOCKED | BLOCKED |
| `agent-created` | AUTO-ALLOW | AUTO-ALLOW | `ask` → allow + warn |

**`ask` semantics (agent-created + dangerous):** `should_allow_install()` returns
`Option<bool>` where `None` means "ask". For agent-created plugins, "ask" does NOT
block the install — it allows installation but surfaces the scan findings to the user
as a visible warning. This is intentional: the agent wrote the skill, so it has context.

Gateway installs (Telegram/Discord): community+safe is AUTO-ALLOW (same as CLI).
Blocking only applies to caution/dangerous verdicts regardless of channel.

### 3.3 Hermes Name Aliases

The trust level names used in EdgeCrab are identical to the Hermes names (ground truth:
`tools/skills_guard.py` → `INSTALL_POLICY`). For cross-reference with earlier EdgeCrab
design documents that used different names, the mapping is:

```
+------------------+------------------+
| EdgeCrab / Hermes| Earlier EdC docs |
+------------------+------------------+
| builtin          | Official         |
| trusted          | Verified         |
| community        | Community        |
| agent-created    | (new in Hermes)  |
| (none)           | Unverified       |
+------------------+------------------+
```

**Unverified** remains valid for locally-installed plugins with no hub provenance.
It is not present in Hermes because Hermes always has hub provenance. EdgeCrab adds it
for the case of `/plugins install ./local-path`.

### 3.4 Trusted Repository Whitelist

The following repositories automatically receive `trusted` level (mirrors Hermes):

```rust
/// Matches Hermes TRUSTED_REPOS = {"openai/skills", "anthropics/skills"}
const HERMES_TRUSTED_REPOS: &[&str] = &[
    "openai/skills",
    "anthropics/skills",
];

/// EdgeCrab's own trusted repos (added on top, never replacing Hermes list)
const EDGECRAB_TRUSTED_REPOS: &[&str] = &[
    "raphaelmansuy/edgecrab",
    "NousResearch/hermes-agent",
];
```

### 3.5 Trust Elevation

Trust elevation (community → trusted) is NOT possible at runtime.
It requires a code change to add the source to `CURATED_SOURCES` or the trusted-repos list.
This is intentional — trust escalation must go through review.

---

## 4. Runtime Enforcement

### 4.1 Capability Enforcement (Host API)

Every host API function checks the plugin's declared capabilities before executing:

```rust
// In HostApiRouter::handle(method, plugin_name, params):
let manifest = registry.get_manifest(plugin_name)?;
let required_cap = method_to_capability(method);  // e.g. "host:memory_read"

if !manifest.capabilities.host.contains(&required_cap) {
    return Err(PluginError::CapabilityDenied {
        plugin: plugin_name.into(),
        capability: required_cap.into(),
    });
}
```

### 4.2 Network SSRF Guard

All HTTP calls from host API functions (e.g., `host:http_get`) are pre-checked:

```rust
// Checks allowed_hosts from manifest
if !manifest.capabilities.allowed_hosts.contains(&url.host()) {
    return Err(PluginError::NetworkBlocked { host: url.host().into() });
}
// THEN: standard edgecrab_security::ssrf::is_safe_url() check
edgecrab_security::ssrf::is_safe_url(&url)?;
```

### 4.3 File Path Safety

All file operations via host API check against `capabilities.allowed_paths`:

```rust
let path = resolve_allowed_path(&params.path, &manifest.capabilities.allowed_paths)?;
edgecrab_security::path_safety::validate_path(&path, &allowed_roots)?;
```

### 4.4 Secret Isolation

Plugin subprocess environment variables NEVER include agent secrets:

```rust
// When spawning plugin subprocess:
let mut cmd = Command::new(&exec.command);
// Only pass explicitly declared env vars from [exec.env]
for (k, v) in &manifest.exec.env {
    cmd.env(k, v);
}
// NEVER inherit agent process env (especially not ANTHROPIC_API_KEY etc.)
cmd.env_clear();
cmd.env("PATH", std::env::var("PATH").unwrap_or_default());
// ... minimal env only
```

Secrets are provided ON DEMAND via the `host:secret_get` host API, which:
1. Checks plugin has the `host:secret_get` capability
2. Checks the requested secret name is in an optional allowlist
3. Logs the access for auditing

### 4.5 Tool Name Collision (INV-4)

```rust
// At plugin enable time, before adding tools to dispatch table:
for tool in plugin.list_tools().await {
    if let Some(existing) = inventory_tools.get(&tool.name) {
        return Err(PluginError::ToolNameConflict {
            tool_name: tool.name.clone(),
            conflict_with: "compile-time inventory tool".into(),
        });
    }
    if let Some(other_plugin) = runtime_tools.get(&tool.name) {
        return Err(PluginError::ToolNameConflict {
            tool_name: tool.name.clone(),
            conflict_with: other_plugin.plugin_name().into(),
        });
    }
}
```

### 4.6 Integrity Verification

On every plugin load (agent restart):

```rust
let installed_checksum = db.get_checksum(plugin_name)?;
let actual_checksum = compute_dir_checksum(&plugin_dir)?;

if installed_checksum != actual_checksum {
    return Err(PluginError::IntegrityViolation {
        plugin: plugin_name.into(),
        expected: installed_checksum,
        actual: actual_checksum,
    });
}
```

This detects plugin files modified after install (e.g., malicious modification while agent was stopped).

---

## 5. Audit Log

All security-relevant plugin events are written to `~/.edgecrab/logs/plugin-audit.log`:

```json
{"ts":"2026-04-09T10:00:00Z","event":"plugin_installed","name":"github-tools","trust":"community","verdict":"safe","source":"github:..."}
{"ts":"2026-04-09T10:05:00Z","event":"secret_accessed","plugin":"github-tools","secret":"GITHUB_TOKEN","by":"tool_call"}
{"ts":"2026-04-09T10:06:00Z","event":"network_attempt","plugin":"github-tools","host":"api.github.com","allowed":true}
{"ts":"2026-04-09T10:07:00Z","event":"plugin_crashed","name":"github-tools","reason":"exit code 1","restart_count":1}
```

Audit log is append-only (write mode, no truncation). Rotated at 10 MiB.
