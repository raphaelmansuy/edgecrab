# Proof: W4 Trust + surface honesty

**Date:** 2026-07-18  
**Honesty note:** `OsSandboxConfig.deny_default` was previously a dead field.
Close-lies **P0** threads it into `wrap_command` and doctor soft-sandbox warn.
Do **not** claim sandbox meter **L** until operators opt into
`deny_default: true` and measure breakage — default remains soft.

## Trust

- Keep port-scoped `PreviewConfig` (no Hermes `allow_private_urls`)
- `deny_default: false` (default) → Seatbelt `(allow default)` / soft bwrap
- `deny_default: true` → Seatbelt `(deny default)` + explicit allows; bwrap
  adds `--unshare-user/pid/ipc` (+ `--unshare-net` when network denied)
- Doctor warns when `mode != off && deny_default == false` (“soft sandbox”)

## Surface

- `--json-stream` emits `RunFinished` as `kind=done` with `completion_state`,
  `exit_reason`, `summary`
- TUI continues to use `format_operator_notice` / `RunOutcome`

## Verify

```bash
cargo test -p edgecrab-security --lib os_sandbox
cargo test -p edgecrab-cli --bin edgecrab -- os_sandbox_soft_mode_warns
cargo test -p edgecrab-cli --bin edgecrab -- json_stream_run_finished_includes_exit_reason
```
