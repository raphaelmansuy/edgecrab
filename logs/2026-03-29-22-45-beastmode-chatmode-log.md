# Task Log — 2026-03-29-22-45

## Actions
- Rewrote `configure_signal()` in `crates/edgecrab-cli/src/gateway_setup.rs`
- Added `check_signal_cli_installed()` and `signal_cli_version()` helpers
- Added `try_install_signal_cli()` + per-OS platform helpers (macOS/brew, Linux/apt, fallback)
- Added `test_signal_cli_connection()` (curl probe → TCP fallback) against `/api/v1/check`
- Added `tcp_probe_signal_cli()` fallback when curl is absent
- Fixed Signal HTTP URL default: now consistently `http://127.0.0.1:8090` (was silently using 8080 = the gateway port)
- Added step-by-step account registration/link guidance in the wizard
- Added post-config daemon launch command block
- Refactored `try_install_signal_cli` into per-OS `try_install_signal_cli_platform()` fns to satisfy clippy needless-return lint

## Decisions
- Used `curl --write-out %{http_code}` as primary connectivity probe (available on macOS + most Linux without extra deps)
- TCP fallback used only when curl is absent
- Platform-specific install helpers defined as separate `#[cfg]` functions, not blocks-in-one-fn, to avoid clippy `needless_return`
- Did not touch non-Signal platforms (Telegram/Discord/Slack/WhatsApp) — only Signal was broken

## Next Steps
- Test `edgecrab gateway configure signal` end-to-end with signal-cli installed/not installed
- Optionally: wrap the "start daemon" step with `launchd`/`systemd` service helper

## Lessons / Insights
- `#[cfg]`-gated `return` at the end of a block inside a function triggers clippy `needless_return`; splitting into per-OS functions avoids this cleanly
- signal-cli HTTP check endpoint is `GET /api/v1/check` → 200 OK when running
