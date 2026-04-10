# Plugin Spec Completion Checklist

Repository-visible sources:

- `specs/spec_plugins/spec/015_hermes_compatibility.md`
- `specs/spec_plugins/spec/016_implementation_plan.md`

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
- [x] Hermes-compatible `on_session_end` hook execution path
- [x] Hermes-compatible `pre_tool_call` and `post_tool_call`
- [x] Hermes-compatible `post_llm_call`
- [x] Hermes-compatible `pre_api_request` and `post_api_request`
- [x] Hermes-compatible `on_session_finalize` and `on_session_reset` in CLI sessions

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
- [x] Hermes hub indexing for upstream `plugins/...` directories
- [x] Hermes hub indexing for repo-root Hermes plugin directories (`42-evey/hermes-plugins`)
- [x] deterministic shared-support-file resolution for curated GitHub Hermes sources with repo-root helper modules

## Hermes Compatibility Proof

- [x] guide-style Hermes plugin E2E coverage (`calculator` from the upstream build guide contract)
- [x] real upstream Hermes plugin E2E coverage (`holographic`)
- [x] real upstream Hermes package-import compatibility coverage (`honcho`)
- [x] real upstream Hermes skill install coverage (`1password`)
- [x] real `42-evey/hermes-plugins` E2E coverage (`evey-telemetry`, `evey-status`)
- [x] raw local Hermes bundle install without handwritten `plugin.toml`
- [x] bundled `SKILL.md` loading inside Hermes plugin roots
- [x] Hermes path translation verified on real upstream skill content
- [x] `metadata.hermes.related_skills` surfaced in plugin info rendering
- [x] pip entry-point plugin loading parity
- [x] Hermes CLI subcommand registration parity
- [x] Hermes memory-provider `cli.py register_cli(subparser)` convention
- [x] gateway-specific session-boundary parity proof

## Residual Non-Goal From Older Study Notes

- [ ] literal WASM/TypeScript plugin SDK flow from the older study doc

Verified non-gaps:

- The Python Hermes plugin system used by upstream `hermes-agent` and curated community repos is covered by real install/runtime tests.
- `cargo test -- --include-ignored` remains unchecked because the ignored suite includes external-network or credentialed scenarios and is not a stable compatibility proof for Hermes plugins.

## Documentation and Verification

- [x] README updated
- [x] plugin docs updated
- [x] site docs updated
- [x] Hermes-style plugin authoring tutorial added to repo docs
- [x] Hermes-style plugin authoring tutorial added to site docs
- [x] changelog updated
- [x] `cargo test -p edgecrab-plugins hermes_plugin_loads_bundled_skill_metadata -- --nocapture`
- [x] `cargo test -p edgecrab-plugins cached_hermes_repo_index_includes_python_plugin_directories -- --nocapture`
- [x] `cargo test -p edgecrab-core api_call_with_retry_invokes_hermes_api_hooks -- --nocapture`
- [x] `cargo test -p edgecrab-core session_boundary_hooks_fire_on_new_and_finalize -- --nocapture`
- [x] `cargo test -p edgecrab-gateway --lib run::tests::gateway_keeps_agent_history_isolated_per_chat_session -- --nocapture`
- [x] `cargo test -p edgecrab-gateway --lib run::tests::gateway_session_hooks_fire_across_chat_reset_and_shutdown -- --nocapture`
- [x] `cargo test -p edgecrab-cli --test plugins_e2e real_hermes_honcho_memory_cli_is_invocable_end_to_end -- --nocapture`
- [x] `cargo test -p edgecrab-cli --test plugins_e2e -- --nocapture`
- [x] `cargo test -p edgecrab-plugins live_official_hermes_search_returns_real_plugins -- --ignored --nocapture`
- [x] `cargo test -p edgecrab-plugins --lib`
- [x] `cargo test -p edgecrab-core --lib`
- [x] `cargo clippy -p edgecrab-plugins -p edgecrab-core -p edgecrab-cli -p edgecrab-gateway --tests -- -D warnings`
- [x] `cargo test`
- [ ] `cargo test -- --include-ignored`
- [x] `pnpm build` in `site/`
