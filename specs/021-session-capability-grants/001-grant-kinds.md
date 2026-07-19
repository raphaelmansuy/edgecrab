# 001 — Grant kinds

## Implemented: `preview_loopback`

| Field | Meaning |
|-------|---------|
| `grant_kind` | `"preview_loopback"` |
| `host` | `127.0.0.1` / `localhost` / `::1` |
| `port` | Target port |
| `url` | Full navigate URL |

Recovery attaches `RecoveryAction::RequestUserGrant` with that payload when SSRF blocks loopback HTTP(S).

## User decisions

| Choice | Effect |
|--------|--------|
| Once | Allow this host:port for a single redispatch |
| Session | Allow host:port until process exit (`SessionPreviewGrants`) |
| Always | Persist `security.preview.enabled = true` (+ ensure port allowlist covers port) |
| Deny | Structured denial; `suppress_retry` |

## Future kinds (not in MVP)

- `host_command_install` — dangerous package installs (use terminal approval today)
- `private_network` — non-loopback LAN (explicitly out of scope)
