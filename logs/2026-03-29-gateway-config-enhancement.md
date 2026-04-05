# 2026-03-29 Gateway Configuration Enhancement

## Actions
- Added per-platform gateway config structs: `TelegramGatewayConfig`, `DiscordGatewayConfig`, `SlackGatewayConfig`, `SignalGatewayConfig` to edgecrab-core
- Extended `GatewayConfig` to include all 5 platform configs (telegram, discord, slack, signal, whatsapp)
- Added comprehensive env var overrides for all platforms (allowed users, home channels, tokens)
- Created `gateway_setup.rs` — full interactive configuration wizard exceeding hermes-agent
- Added `edgecrab gateway configure [platform]` CLI subcommand with per-platform and full-wizard modes
- Updated `gateway_cmd.rs` to register Slack and Signal adapters alongside existing Discord/Telegram/WhatsApp
- Enhanced `setup.rs` gateway section with rich status display and cross-references to detailed wizard
- All env keys are saved to `~/.edgecrab/.env` with create-or-update logic

## Decisions
- Kept WhatsApp pairing flow in existing `whatsapp_cmd.rs` but also accessible via `gateway configure whatsapp`
- Used `unsafe { set_var }` block with safety comment (single-threaded setup context)
- Per-platform configs use `enabled` + `token_env` pattern (env var name, not secret value in YAML)

## Next steps
- Test interactive flows manually with `edgecrab gateway configure`
- Consider adding health checks after platform configuration
- Add `edgecrab doctor` checks for gateway platform readiness

## Lessons
- edgecrab already had a solid gateway architecture; the gap was interactive onboarding UX
- hermes-agent's setup_gateway() does ~160 lines per platform; edgecrab now matches and exceeds this
