# Plugin Spec Completion Checklist

Tracked against `specs/spec_plugins/spec/`.

## Core Runtime

- [x] `plugin.toml` parsing and validation for `skill`, `tool-server`, and `script`
- [x] plugin config surface under `plugins:` in `config.yaml`
- [x] discovery across user, project, and system plugin roots
- [x] runtime registration of tool-server and script plugin tools
- [x] prompt injection of enabled skill plugins

## Transport and Host API

- [x] MCP-style stdio handshake: `initialize`, `notifications/initialized`
- [x] tool-server dispatch for `tools/list` and `tools/call`
- [x] reverse `host:*` request handling during plugin calls
- [x] host APIs for `platform_info`, `log`, `memory_read`, `memory_write`, `session_search`, `secret_get`, and delegated `tool_call`

## Install, Hub, and Security

- [x] quarantine install flow
- [x] static guard scan before activation
- [x] install/remove audit log
- [x] curated and configured hub index search
- [x] `hub:<source>/<plugin>` source resolution
- [x] direct `https://...zip` archive installs
- [x] GitHub Contents API download with `GITHUB_TOKEN`/`GH_TOKEN` and `gh auth token` fallback
- [x] manifest trust/source/checksum stamping on install
- [x] checksum verification when hub metadata provides an expected digest

## CLI and TUI Surface

- [x] `/plugins list|info|status|install|enable|disable|toggle|audit`
- [x] `/plugins hub search|browse|refresh`
- [x] `edgecrab plugins ...` CLI coverage for list/info/status/install/enable/disable/toggle/audit/hub/update/remove
- [x] plugin toggle overlay for persisted enable/disable state

## Documentation and Verification

- [x] README updated
- [x] plugin docs updated
- [x] site docs updated
- [x] changelog updated
- [x] `cargo check -p edgecrab-plugins -p edgecrab-cli`
- [x] `cargo test -p edgecrab-plugins -p edgecrab-cli`
- [x] `cargo clippy -p edgecrab-plugins -p edgecrab-cli -- -D warnings`
- [x] `cargo test`
- [x] `cargo test -- --include-ignored`
- [x] `pnpm build` in `site/`
