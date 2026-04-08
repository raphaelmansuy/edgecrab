# EdgeCrab LSP — Roadblocks and Mitigations

A frank analysis of every significant obstacle in implementing the LSP spec. Each blocker
includes: severity, root cause, concrete mitigation, and residual risk.

---

## 1. Language Server Availability

**Severity**: HIGH — blocks ALL LSP operations for a language

**Problem**: Language servers are not universally installed. A Rust project needs
`rust-analyzer`, a TypeScript project needs `typescript-language-server`, etc. If the
binary is missing from PATH, `tokio::process::Command::spawn()` returns
`ErrorKind::NotFound`.

**Current Claude Code behaviour**: Returns `"Found no language servers"` and falls back to
regex-based symbol search. Functional degradation, not a crash.

**Mitigation**:
```rust
pub async fn check_server_available(command: &str) -> Result<(), LspError> {
    which::which(command).map_err(|_| LspError::ServerNotFound {
        command: command.to_string(),
        hint: format!(
            "Install with: {}",
            install_hint(command).unwrap_or("see language server docs")
        ),
    })?;
    Ok(())
}

fn install_hint(cmd: &str) -> Option<&'static str> {
    match cmd {
        "rust-analyzer"             => Some("rustup component add rust-analyzer"),
        "typescript-language-server"=> Some("npm install -g typescript-language-server typescript"),
        "clangd"                    => Some("brew install llvm  OR  apt install clangd"),
        "pylsp"                     => Some("pip install python-lsp-server"),
        "gopls"                     => Some("go install golang.org/x/tools/gopls@latest"),
        _                           => None,
    }
}
```

When `ServerNotFound`, tools return structured JSON the agent can present to the user,
not a panic.

**Residual risk**: User may have the binary installed but not in EdgeCrab's PATH (e.g.,
installed in a virtual env or via asdf). Mitigation: accept `command` as full absolute path
in config.

---

## 2. Async Tokio Deadlock Risk

**Severity**: MEDIUM — can cause hangs in specific task topologies

**Problem**: `async-lsp`'s `MainLoop` runs in a tokio task. The `ServerSocket::request()`
method is also async. If we hold a `DashMap` entry lock while awaiting an LSP request, and
the response handler also needs to acquire the same lock → deadlock.

**Root cause**: `DashMap::get()` and `DashMap::entry()` hold a shard lock. Awaiting across
a lock boundary is safe from Rust's `Send + Sync` perspective, but deadlock is still
possible if the receiver needs the same shard.

**Mitigation**: Never hold a `DashMap` entry across an `.await` point. Pattern:

```rust
// WRONG — shard lock held across await
let server = self.servers.get(&lang);       // ← lock acquired
let result = server.request(...).await?;    // ← await while locked → potential deadlock

// CORRECT — clone what we need, release lock, then await
let socket = {
    let state = self.servers.get(&lang)?;   // ← lock acquired
    state.server_socket.clone()?            // ← clone Arc<ServerSocket>
};                                          // ← lock released here
let result = socket.request(...).await?;   // ← await without lock
```

`ServerSocket` is `Clone + Send` by design in async-lsp, enabling this pattern.

**Testing**: Write integration tests that send concurrent requests to the same server to
exercise the locking path.

---

## 3. LSP Protocol Version Negotiation

**Severity**: MEDIUM — causes subtle feature unavailability

**Problem**: We target LSP 3.17 for inlay hints, semantic tokens (as proposed features), and
pull diagnostics. Older servers (e.g., clangd 13, old pyright versions) may not support these.

**Problem surface**: `lsp-types` crate models LSP 3.17 features behind the `proposed` feature
flag. At the library level this is fine. At runtime, the SERVER may return `null` or a
method-not-found error for 3.17 requests.

**Mitigation**:
1. Always check `server_capabilities` before sending a 3.17 request (handled by `CapabilityRouter`)
2. Handle `ResponseError { code: MethodNotFound (-32601) }` gracefully — return
   `{ "supported": false }` instead of propagating as `ToolError`
3. Degrade silently: Tier 2 operations that lack capability simply return
   `{ "supported": false, "reason": "..." }` — the model can work around this

```rust
match socket.request::<InlayHintRequest>(params).await {
    Ok(result) => Ok(render_hints(result)),
    Err(async_lsp::Error::Response(e)) if e.code == -32601 => {
        Ok(json!({ "supported": false, "reason": "Server does not support inlay hints" }))
    }
    Err(e) => Err(ToolError::Internal(e.to_string())),
}
```

---

## 4. Position Encoding Mismatch (UTF-16 vs UTF-8)

**Severity**: MEDIUM — incorrect results without a runtime error

**Problem**: LSP 3.16 and earlier specify positions as UTF-16 code-unit offsets. Rust strings
are UTF-8. A naive translation of byte offset to LSP character position is incorrect for any
file containing non-ASCII characters (e.g., Unicode identifiers, Chinese comments, emoji
in strings).

**LSP 3.17 mitigation**: LSP 3.17 introduces `positionEncodingKind` negotiation, allowing
clients to request UTF-8 positions. Clients advertise:

```rust
ClientCapabilities {
    general: Some(GeneralClientCapabilities {
        position_encodings: Some(vec![
            PositionEncodingKind::UTF8,   // preferred — avoids the conversion entirely
            PositionEncodingKind::UTF16,  // fallback for old servers
        ]),
        ..Default::default()
    }),
    ..Default::default()
}
```

If the server accepts UTF-8 (confirmed in `InitializeResult.capabilities.position_encoding`),
no conversion is needed. Otherwise, `PositionEncoder::to_position` handles UTF-16 conversion.

**`PositionEncoder` implementation**:

```rust
pub fn utf8_col_to_utf16(line: &str, utf8_col: usize) -> usize {
    line.char_indices()
        .take_while(|(i, _)| *i < utf8_col)
        .map(|(_, c)| c.len_utf16())
        .sum()
}

pub fn utf16_col_to_utf8_byte(line: &str, utf16_col: u32) -> usize {
    let mut remaining = utf16_col as usize;
    for (byte_idx, c) in line.char_indices() {
        if remaining == 0 { return byte_idx; }
        remaining = remaining.saturating_sub(c.len_utf16());
    }
    line.len()
}
```

**Residual risk**: Line endings (`\r\n` vs `\n`) also affect offsets. Normalize all text
to `\n` before computing positions, matching the `didOpen` content sent to the server.

---

## 5. Version Counter Monotonicity

**Severity**: LOW (but causes hard-to-debug silent failures if violated)

**Problem**: LSP requires that `didChange` notifications include a strictly monotonically
increasing `version` integer. If the same file is closed and re-opened with version 1,
some servers reject subsequent changes. Others silently discard them.

**Mitigation**: `DocumentSyncLayer` uses a `DashMap<Url, AtomicI32>` for version counters.
The counter is NEVER reset to zero — it persists for the `edgecrab` session lifetime.
Even if a file is closed and re-opened, the version continues from where it left off.

```rust
fn next_version(&self, uri: &Url) -> i32 {
    self.version_counters
        .entry(uri.clone())
        .or_insert_with(|| AtomicI32::new(0))
        .fetch_add(1, Ordering::Relaxed)
}
```

---

## 6. Remote Backends (Docker, SSH, Modal, Daytona)

**Severity**: HIGH for users on remote backends

**Problem**: EdgeCrab supports Docker, SSH, Modal, and Daytona as execution backends
(via `edgecrab-tools/src/tools/terminal.rs` backend dispatch). Language servers run on
the LOCAL machine and access the LOCAL filesystem. When files exist only in a container
or on a remote SSH host, the local language server cannot read them.

**Options**:

| Option | Feasibility | Notes |
|--------|-------------|-------|
| Remote LSP over stdin/SSH tunnel | Medium | `ssh host -- rust-analyzer` via tokio process + port forward |
| Container LSP via `docker exec` | Medium | `docker exec -i container rust-analyzer` as the command |
| LSP over TCP (remote language server) | Hard | Requires server-side setup |
| Disable LSP for remote backends | Easy | Return `{ "supported": false, "reason": "remote backend" }` |

**Recommended path**:
1. Phase 1: Disable LSP for non-local backends with clear message
2. Phase 2: Support Docker via `docker exec` as the process command
3. Phase 3: SSH tunneling via openssh (already in workspace deps)

**Detection**:
```rust
fn is_local_backend(ctx: &ToolContext) -> bool {
    matches!(ctx.config.execution_backend(), BackendKind::Local)
}
```

---

## 7. Unsaved Buffer State

**Severity**: MEDIUM for tools invoked mid-edit

**Problem**: Claude Code has an in-memory buffer model: the IDE knows the unsaved state of
every open file. EdgeCrab does not — all file access is via filesystem reads. If the agent
writes to a file and immediately requests hover at a position in the new content, the
position may not match if the file write is not yet flushed.

**Mitigation**:
1. `file_write` tool already flushes synchronously to disk
2. `DocumentSyncLayer::ensure_open()` reads from disk (which is current after flush)
3. For tools that modify content (format, apply_code_action), re-read the file content from
   disk after applying edits before any subsequent LSP request

**Residual risk**: If the OS file cache is stale (extremely rare with tokio's async writes),
a `tokio::time::sleep(Duration::from_millis(50))` after write resolves it. This is a last
resort; do not add it speculatively.

---

## 8. Security: LSP Servers Execute Arbitrary Code

**Severity**: HIGH if server binaries are attacker-controlled

**Problem**: Language servers run as child processes with the same permissions as EdgeCrab.
If the model is tricked into specifying a malicious binary as the server command, it runs
with full user access.

**Mitigation** (see Architecture §7):
1. Server commands are read from `~/.edgecrab/config.yaml` under `lsp.servers` only
2. Model output (tool arguments) never specifies the server command — tools only receive
   `file`, `line`, `column`, etc.
3. `edgecrab-security::path_safety::validate_path()` is called on all file arguments
4. The config file itself is scanned for prompt injection patterns (existing mechanism)

**Residual risk**: A compromised config file (e.g., social-engineered edit) could inject
a malicious binary. Mitigated by: (a) only accepting absolute paths in server config,
(b) `which::which` validation that the binary exists in a known location.

---

## 9. Large Workspace Symbol Search Performance

**Severity**: LOW (UX issue, not a correctness issue)

**Problem**: `workspace/symbol` with an empty query string requests ALL symbols. For large
workspaces (rust-analyzer in a monorepo), this can be slow (1-5 seconds) and return
thousands of results.

**Mitigation**:
- Require `query` to be non-empty (enforced in schema with `minLength: 1`)
- Add `max_results: Option<usize>` parameter (default: 50) and truncate with a note
- Rely on server-side fuzzy filtering; never send empty queries

---

## 10. `$/progress` Partial Results

**Severity**: LOW (quality issue for streaming responses)

**Problem**: Some operations (`workspace/symbol`, `textDocument/semanticTokens`) may stream
partial results via `$/progress` tokens. `async-lsp` handles this at the MainLoop level
via `window.workDoneProgress`, but our current design waits for the final response only.

**Mitigation**: For Phase 1, wait for final response (simpler, covers all cases). If a
server only responds via partial results with no final result, add a handler:

```rust
// In EdgeCrabClientHandler notifications
fn work_done_progress_create(&mut self, params: WorkDoneProgressCreateParams) {
    self.in_progress.insert(params.token, vec![]);
}
fn progress(&mut self, params: ProgressParams) {
    if let ProgressParamsValue::WorkDone(WorkDoneProgress::Report(r)) = params.value {
        self.in_progress.entry(params.token).or_default().push(r.message.unwrap_or_default());
    }
}
```

This is Phase 2 work.

---

## Summary Risk Matrix

| Blocker | Severity | Phase 1 | Phase 2 |
|---------|----------|---------|---------|
| Server not installed | HIGH | Helpful error + install hint | Auto-install wizard |
| Deadlock on DashMap | MEDIUM | Never hold lock across await | Tests |
| Protocol version mismatch | MEDIUM | Capability check + degrade | positionEncodingKind negotiation |
| UTF-16/UTF-8 positions | MEDIUM | PositionEncoder | Negotiate UTF-8 (LSP 3.17) |
| Version counter reset | LOW | Persistent AtomicI32 | — |
| Remote backends | HIGH | Disable + message | Docker exec |
| Unsaved buffer state | MEDIUM | Flush before open | — |
| Security: server binary | HIGH | Config allowlist + validate_path | — |
| Large symbol search | LOW | min query length | Server filtering |
| Partial results | LOW | Await final only | $/progress handler |
