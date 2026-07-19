# 002 — Approval flow

Grant negotiation happens **inside the tool** (same pattern as terminal `approval_runtime`), not via model-driven `clarify`.

```text
browser_navigate(url)
  → validate_browser_url
       fail SSRF loopback + RequestUserGrant in recovery
  → request_preview_loopback_grant(ctx, host, port, url)
       → ApprovalRequest { kind: PreviewLoopback, ... }
       → StreamEvent::Approval → TUI / gateway overlay
  → Once/Session/Always → apply SessionPreviewGrants / persist preview
  → re-validate + continue navigate
  → Deny → PermissionDenied (suppress_retry)
```

## Code anchors

- `edgecrab-types` — `RecoveryAction::RequestUserGrant`
- `edgecrab-security/url_safety.rs` — `SessionPreviewGrants`
- `edgecrab-tools/preview_grant.rs` — ask + apply
- `edgecrab-tools/recovery_catalog.rs` — grant suggestion on SSRF block
- `edgecrab-tools/tools/browser.rs` — call grant before hard-fail
- CLI approval overlay / status — preview copy
