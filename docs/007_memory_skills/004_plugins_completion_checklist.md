# Plugin Spec Completion Checklist

Repository-visible sources:

- `specs/spec_plugins/00_study.md`
- `specs/spec_plugins/01_hermes_compat.md`

The Hermes compatibility doc is now the precise contract. The older study doc remains useful as examples, but not as the implementation baseline.

## Current Runtime

- [x] `plugin.toml` parsing and validation for `skill`, `tool-server`, and `script`
- [x] plugin config surface under `plugins:` in `config.yaml`
- [x] discovery across user, project, and system plugin roots
- [x] runtime registration of tool-server and script plugin tools
- [x] prompt injection of enabled skill plugins
- [x] discovery of Hermes-style `plugin.yaml` + `__init__.py` directory plugins
- [x] legacy Hermes user/project plugin root discovery
- [x] Hermes `requires_env` readiness gating to `setup-needed`
- [x] non-available plugins excluded from runtime tool exposure

## Transport and Host API

- [x] MCP-style stdio handshake: `initialize`, `notifications/initialized`
- [x] tool-server dispatch for `tools/list` and `tools/call`
- [x] reverse `host:*` request handling during plugin calls
- [x] host APIs for `platform_info`, `log`, `memory_read`, `memory_write`, `session_search`, `secret_get`, `inject_message`, and delegated `tool_call`
- [x] Hermes-compatible `pre_llm_call` hook execution with user-message context injection
- [x] Hermes-compatible `on_session_start` hook execution

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
- [x] first-class `plugins search|browse|refresh` UX with backward-compatible `hub-*` aliases
- [x] source-aware plugin search output with install-ready `hub:<source>/<plugin>` targets

## Remaining Hermes Gaps

- [ ] pip entry-point plugin loading parity
- [ ] Hermes CLI subcommand registration parity
- [ ] full Hermes hook surface beyond `on_session_start` and `pre_llm_call`
- [ ] literal WASM/TypeScript plugin SDK flow from the older study doc

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
