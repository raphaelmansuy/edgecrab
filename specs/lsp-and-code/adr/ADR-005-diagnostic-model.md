# ADR-005 — Diagnostic Model: Push vs Pull

**Status**: Accepted  
**Date**: 2025

---

## Context

LSP diagnostics (errors, warnings, hints) can reach the client in two ways:

1. **Push** (`textDocument/publishDiagnostics` notification) — server sends whenever it has new results
2. **Pull** (`textDocument/diagnostic` request, LSP 3.17) — client requests on demand

Claude Code uses push-only. We can do both.

---

## Push Model

Server sends `publishDiagnostics` at its own cadence (e.g., after file save, after compilation).
These are stored in `DiagnosticCache`.

Pros:
- Works with all servers (LSP 3.16 baseline)
- No polling required

Cons:
- Diagnostics may be stale by the time the agent reads them
- Cache must be invalidated on file write (handled by notify integration)
- Race condition: agent reads cache before server has finished processing

### EdgeCrab handler

```rust
// In EdgeCrabClientHandler (the notification listener built into the MainLoop)
impl LanguageClient for EdgeCrabClientHandler {
    async fn publish_diagnostics(&mut self, params: PublishDiagnosticsParams)
        -> ControlFlow<async_lsp::Result<()>>
    {
        self.diag_cache.update(params.uri, params.diagnostics);
        ControlFlow::Continue(())
    }
}
```

## Pull Model (LSP 3.17)

Agent explicitly requests diagnostics for a file/workspace.

Pros:
- Fresh, synchronous: server computes on request
- No stale data risk
- Can be batched: request diagnostics for N files in parallel

Cons:
- Only supported by servers that advertise `diagnosticProvider` (rust-analyzer ≥ 2023,
  typescript-language-server ≥ 4.1, clangd ≥ 17)
- Adds a round-trip per file

---

## Decision

**Both, with pull preferred when available**

```
Tool call lsp_diagnostics_pull:
  1. Check server_capabilities.diagnostic_provider
  2. If Some(_) → send textDocument/diagnostic request (fresh)
  3. If None    → fall back to DiagnosticCache (push cache)
```

This gives the agent the freshest possible diagnostics without requiring LSP 3.17
server support.

```rust
pub async fn get_diagnostics(
    socket: &ServerSocket,
    caps:   &ServerCapabilities,
    uri:    &Url,
    cache:  &DiagnosticCache,
) -> Result<Vec<Diagnostic>, LspError> {
    if caps.diagnostic_provider.is_some() {
        // Pull: ask server directly
        let params = DocumentDiagnosticParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            identifier: None,
            previous_result_id: None,
            work_done_progress_params: Default::default(),
        };
        let result = socket.request::<DocumentDiagnosticRequest>(params).await?;
        Ok(match result {
            DocumentDiagnosticReport::Full(r) => r.result.items,
            DocumentDiagnosticReport::Unchanged(_) => {
                // Server says no change — use cache
                cache.get(uri).unwrap_or_default()
            }
        })
    } else {
        // Push cache fallback
        Ok(cache.get(uri).unwrap_or_default())
    }
}
```

---

## Consequences

- `DiagnosticCache` must remain as the push-model store even when pull is available,
  because background errors (not from explicit tool calls) still arrive via push
- The `lsp_workspace_type_errors` Tier 3 tool uses push cache only (no workspace-pull support
  in most servers yet; workspace/diagnostic support is rare)
- `cache.clear_file(uri)` must be called before a pull request to avoid stale-then-fresh confusion
