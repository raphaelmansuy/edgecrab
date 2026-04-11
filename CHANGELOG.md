# Changelog

All notable changes to EdgeCrab are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

---

## [Unreleased]

### Added

- **Profile starter bundles and code-true profile docs** — EdgeCrab now seeds bundled `work`, `research`, and `homelab` profiles with starter `config.yaml`, `SOUL.md`, and memory files, and the README, internal docs, and site docs now document the real profile YAML format and command surface.
- **First-class TUI profile browser and live runtime switching** — `/profiles` now opens a searchable profile browser, `/profile` exposes full inline management, and `/profile use <name>` rebuilds the running runtime immediately instead of deferring the switch to the next launch.
- **Native in-browser profile admin controls** — the profile browser now performs create, rename, import, export, alias, and delete flows through direct TUI capture overlays instead of command prefills, with `Home`/`End` navigation and inspection for `config.yaml`, `SOUL.md`, memory, and tool policy.
- **`/profile` now routes through the same overlay surface** — profile read subcommands now open the browser on the relevant profile and detail tab instead of dropping large reports into scrollback, and the browser now supports view cycling with `Tab`, `Shift-Tab`, and `Left`/`Right`, plus an inline help tab.
- **Hermes plugin install and runtime E2E coverage** — new CLI integration tests now install and execute a guide-style Hermes plugin bundle, a real upstream `holographic` Hermes plugin, and a real upstream `1password` Hermes skill directly through `edgecrab plugins install`.
- **Hermes entry-point and plugin CLI E2E coverage** — a new integration test now installs a pip-distributed Hermes entry-point plugin into an isolated virtualenv, verifies discovery through `EDGECRAB_PLUGIN_PYTHON`, and executes a top-level Hermes CLI command registered through `ctx.register_cli_command()`.
- **Community Hermes repo coverage and authoring guides** — new docs and site guides now show how to build Hermes-style plugins for EdgeCrab, and the CLI E2E suite now covers real `42-evey/hermes-plugins` plugins (`evey-telemetry`, `evey-status`).
- **Official repo Hermes example plugins** — the repository now ships `plugins/productivity/calculator` and `plugins/developer/json-toolbox` as dependency-free Hermes-format examples, both covered by CLI E2E tests and usable as documentation-grade templates.
- **Installed plugin browser now has first-class TUI UX** — `/plugins` now opens the local plugin browser overlay by default, with split-pane details, fuzzy filtering across plugin/runtime metadata, staged enable-disable changes, scope switching, and direct handoff to remote plugin search.
- **Gateway parity proof for Hermes session hooks** — new gateway tests now prove per-chat agent isolation plus `on_session_start`, `on_session_end`, `on_session_finalize`, and `on_session_reset` behavior through real gateway message flow, reset, and shutdown paths.
- **Claude-style standalone skill bundle support for helper scripts** — preloaded and viewed skills now render a deterministic base-directory header, substitute `${CLAUDE_SKILL_DIR}` and `${CLAUDE_SESSION_ID}`, surface bundled helper files from `references/`, `templates/`, `scripts/`, and `assets/`, and parse Claude frontmatter metadata including `when_to_use`, `arguments`, `argument-hint`, `allowed-tools`, `user-invocable`, `disable-model-invocation`, `context`, and `shell`.
- **Compatibility claims narrowed to code-proven behavior** — docs now distinguish supported Claude-style skill-bundle features from still-missing Claude runtime semantics such as inline prompt-shell execution and automatic forked skill-agent invocation.
- **Remote plugin browser now stays plugin-only** — curated repo-backed plugin search no longer mixes standalone skills into the plugin overlay, skill-only sources are hidden from plugin-browser source summaries, and per-source reporting no longer starves Hermes plugin results behind earlier sources.

### Changed

- **Profile management now exceeds Hermes on backend safety** — profile export/import no longer shells out to `tar`, active-profile persistence is atomic, alias collisions are validated, clone flows preserve the right identity files, and profile deletion now attempts to stop a running gateway before removing the profile directory.
- **Release documentation now follows measured reality** — README, internal docs, and the Astro site now report the current stripped macOS arm64 binary size at about 49 MB and use source-derived provider, tool, and gateway counts instead of stale marketing numbers.
- **Hermes local bundle installs are now source-compatible** — `edgecrab plugins install ./path` now accepts raw Hermes plugin directories and raw `SKILL.md` skill bundles without requiring authors to add a handwritten `plugin.toml`.
- **Installed Hermes plugins retain their Hermes runtime identity** — rediscovery now preserves hook introspection, bundled `SKILL.md` metadata, and the persisted `HERMES_HOME` mapping instead of degrading installed Hermes bundles into generic manifest-only plugins.
- **Installed Hermes memory plugins now retain their upstream package identity** — flattened installs are now aliased back into `plugins.memory.<name>` so real upstream plugins such as `honcho` keep working across fresh-process rediscovery, runtime hooks, and CLI loading.
- **Hermes hook parity now matches upstream `VALID_HOOKS` in the CLI runtime** — EdgeCrab now drives `pre_tool_call`, `post_tool_call`, `pre_llm_call`, `post_llm_call`, `pre_api_request`, `post_api_request`, `on_session_start`, `on_session_end`, `on_session_finalize`, and `on_session_reset`.
- **Hermes hub search now indexes upstream Python plugin directories** — the curated `hermes-plugins` source now returns installable results from `plugins/...` as well as `skills/...` and `optional-skills/...`.
- **Hermes hub search now indexes repo-root Hermes plugin directories too** — the curated `hermes-evey` source maps `42-evey/hermes-plugins` directly, including declared shared support files for curated GitHub installs.
- **`edgecrab-official` now indexes repo-local Hermes plugins too** — the official source now scans the repository `plugins/` tree in addition to official skills, and repo-backed caches are keyed by mapping so skill and plugin indexes do not trample each other.
- **Rust release automation now publishes `edgecrab-plugins` explicitly** — crates.io publish order and local publish helpers now include the shared plugin crate as a first-class release artifact instead of relying on workspace-only coverage.

### Fixed

- **Full workspace release validation is green again** — the flaky profile-delete TUI test now tolerates macOS cwd races in the parallel suite, the LSP integration test no longer deadlocks by spawning `cargo run` from inside `cargo test`, and release verification now passes under `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo build --release`.
- **OpenClaw/Hermes migration helpers now preserve heading context and character-budget merges correctly** — markdown memory extraction now drops generic top-level file headings while retaining useful subheading context, and merged memory entry limits now count characters instead of UTF-8 bytes.
- **Hermes memory-provider plugins without static `provides_tools` declarations no longer fail local install** — EdgeCrab now permits Hermes manifest bundles whose runtime tools are discovered dynamically.
- **Installer-stamped trusted manifests now rediscover cleanly** — plugin manifests stamped with install-time trust/source/checksum metadata no longer fail validation as if the bundle had self-assigned trust.
- **Manifest-backed skill installs now surface missing credentials correctly** — installed Hermes skills such as `1password` now keep their `setup-needed` state and missing environment variable list in `/plugins info`.
- **Pip-installed Hermes plugins are now discovered through the selected Python runtime** — EdgeCrab now reads the `hermes_agent.plugins` entry-point group from the configured plugin Python interpreter instead of only scanning directory bundles.
- **`ctx.register_cli_command()` and `cli.py register_cli(subparser)` now map to executable EdgeCrab CLI trees** — Hermes plugins can expose `edgecrab <plugin-command> ...` trees through either upstream registration path instead of being silently ignored.
- **The TUI Tool Manager now tracks plugin state live** — installing, enabling, disabling, updating, or removing plugins rebuilds the active agent registry immediately so `/tools` always reflects the real set of exposed plugin tools without requiring a restart.
- **Hermes and tool-server plugin schemas now preserve real `inputSchema` definitions** — dynamic plugin tools no longer downgrade to a generic object schema before provider submission, and outgoing tool schemas are normalized so strict OpenAI-compatible validators accept object tools that rely on `additionalProperties`.
- **Plugin search now uses layered hub caches** — source indexes, repo-backed plugin trees, and repo-entry descriptions are cached separately with TTL-based freshness checks, stale-cache fallback on refresh failure, and shared cache primitives instead of duplicated per-source logic.

---

## [0.2.4] — 2026-04-09

### Added

- **Shared plugin runtime foundation** — new `edgecrab-plugins` workspace crate now centralizes plugin config, manifest parsing, Hermes-compatible `SKILL.md` loading, platform/readiness filtering, tool-server JSON-RPC clients, and Rhai script runtime support.
- **Prompt-integrated skill plugins** — prompt assembly now appends enabled plugin-provided `SKILL.md` content from `~/.edgecrab/plugins/` so plugin skills behave like first-class session guidance instead of a CLI-only discovery feature.
- **Runtime plugin tool registration** — CLI runtime registry construction now discovers installed tool-server and script plugins and registers their tools dynamically so plugin tools participate in the normal `ToolRegistry` flow.
- **Persisted plugin config and CLI controls** — `config.yaml` now has a top-level `plugins:` section and the CLI now supports `edgecrab plugins info|enable|disable` in addition to listing/install/update/remove.
- **Plugin hub search, quarantine installs, and audit log** — plugin installs can now stage from GitHub or local directories into quarantine, run a static scan before activation, search configured hub indices, and append install/remove actions to `~/.edgecrab/plugins/.hub/audit.log`.
- **Plugin host API and transport parity** — tool-server plugins now use an MCP-style stdio handshake (`initialize`, `notifications/initialized`, `tools/list`, `tools/call`) and can issue reverse `host:*` calls for platform info, logging, memory/session access, secret reads, safe conversation message injection, and delegated host tool execution.
- **Hub/source resolution and manifest integrity stamping** — plugin installs now resolve `hub:<source>/<plugin>` entries against cached hub indices, accept direct `https://...zip` archives, support `gh auth token` as a GitHub auth fallback, verify expected checksums when hub metadata provides them, and stamp installed manifests with trust/source/checksum metadata.
- **Hermes plugin compatibility bridge** — EdgeCrab now discovers Hermes-style `plugin.yaml` + `__init__.py` directory plugins, honors Hermes `requires_env` readiness gating, exposes their registered tools through a Python host bridge, discovers legacy `~/.hermes/plugins/` roots, and runs Hermes-compatible `on_session_start` and `pre_llm_call` hooks with user-message context injection semantics.

### Changed

- **Wrapper release orchestration on `main` now follows the binaries workflow** — future npm/PyPI CLI wrapper publishes trigger from the successful completion of `Release — Native Binaries`, and the Node SDK workflow only creates a draft fallback release if it races ahead of binary publication. That avoids `GITHUB_TOKEN`-suppressed release-event fan-out and premature public releases before binary assets exist.
- **Node SDK package entrypoints now match the built tarball** — `sdks/node/package.json` now points CommonJS consumers at `dist/index.js` and ESM consumers at `dist/index.mjs`, with packaging tests that fail if the manifest references artifacts that were not actually built.
- **Plugin documentation and command references now match the implementation** — README, docs, and site pages now document `hub:` installs, archive installs, audit/checksum behavior, and the expanded `/plugins hub ...` surface.
- **Plugin runtime exposure now matches plugin state** — disabled or setup-needed runtime plugins are no longer advertised to the model as callable tools.
- **Plugin search UX now matches the rest of the platform** — the CLI now exposes first-class `plugins search|browse|refresh` commands with backward-compatible `hub-*` aliases, optional source targeting for Hermes registries, and install-ready `hub:<source>/<plugin>` search output.

---

## [0.2.3] — 2026-04-09

### Fixed

- **Wrapper release orchestration on `main` now follows the binaries workflow** — future npm/PyPI CLI wrapper publishes trigger from the successful completion of `Release — Native Binaries`, and the Node SDK workflow only creates a draft fallback release if it races ahead of binary publication. That avoids `GITHUB_TOKEN`-suppressed release-event fan-out and premature public releases before binary assets exist.
- **Node SDK package entrypoints now match the built tarball** — `sdks/node/package.json` now points CommonJS consumers at `dist/index.js` and ESM consumers at `dist/index.mjs`, with packaging tests that fail if the manifest references artifacts that were not actually built.

---

## [0.2.2] — 2026-04-09

### Fixed

- **Python SDK runtime version drift** — `edgecrab-sdk` now derives its published package metadata and runtime `edgecrab.__version__` from `sdks/python/edgecrab/_version.py`, and release automation syncs that file directly so PyPI metadata and the installed SDK cannot diverge.
- **Rust release publish order** — `release-rust.yml` now publishes `edgecrab-cron` before `edgecrab-tools` and keeps the workspace crates in dependency order, preventing crates.io resolution failures during tagged releases.
- **CLI wrapper publication timing** — `release-binaries.yml` now publishes the GitHub Release once native assets and checksums are attached, and the npm/PyPI CLI wrapper workflows now trigger from the `published` release event so users do not get wrapper packages before their GitHub binary assets are public.
- **Release operator documentation** — the CI/CD publication guides now document the corrected trigger topology, the Python SDK version source, the Rust crate order including `edgecrab-lsp`, and the post-release Homebrew tap update flow.

---

## [0.2.1] — 2026-04-09

### Fixed

- **Rust workspace release reproducibility** — internal crate dependencies now inherit from root `workspace.dependencies` instead of carrying stale per-crate `0.1.0` constraints, so crates.io, native binary, and Docker release builds resolve the same workspace graph as local development.
- **Release version synchronization** — `scripts/release-version.sh` now syncs and validates the internal Rust workspace dependency versions alongside the package versions, preventing future partial releases caused by Cargo manifest drift.

---

## [0.2.0] — 2026-04-09

### Added

- **Channel-aware `edgecrab update`** — EdgeCrab now detects whether it was installed via npm, PyPI/pipx, cargo, Homebrew, source checkout, or manual binary, then applies the correct upgrade path or prints safe guidance for non-package-managed installs.
- **Non-blocking release notices** — the TUI now surfaces cached or background-fetched release notices without blocking startup, and CLI subcommands can show cached upgrade hints when a newer release exists.
- **ADR/spec set for updates** — new documents under `specs/update_command/` define the updater architecture, edge cases, roadblocks, and verification matrix.

### Changed

- **Version derivation is more centralized** — the npm CLI wrapper now derives its binary release tag from `sdks/npm-cli/package.json`, and the PyPI CLI wrapper now derives both package metadata and binary release tag from `sdks/pypi-cli/edgecrab_cli/_version.py`.
- **Workspace version is now the single release authority** — `Cargo.toml` `[workspace.package].version` is the canonical release version, `scripts/release-version.sh` syncs the Node SDK, npm CLI wrapper, PyPI CLI wrapper, and Python SDK from it, and CI now rejects drift before release automation can publish mismatched artifacts.
- **Generated PyPI build artifacts removed from source control** — `sdks/pypi-cli/build/lib/...` no longer participates in releases, reducing duplicate version-bearing files and eliminating stale generated metadata from the repo.
- **Release binaries now publish checksums** — `release-binaries.yml` uploads an `edgecrab-checksums.txt` manifest alongside binary archives to simplify Homebrew tap updates and manual verification.

### Fixed

- **Wrapper/binary release drift** — the npm and PyPI CLI wrappers no longer depend on separately-maintained binary-version constants that could fall out of sync with the published wrapper version.
- **Cross-channel version drift** — release automation and CI now catch mismatches like stale wrapper package versions before npm/PyPI releases can diverge from the Rust workspace release.

---
## [0.1.4] — 2026-04-09

### Added

#### TUI Picker Overlays — Settings Now Have First-Class UX

Four commands that previously printed plain text now open rich interactive picker overlays when invoked with no arguments. Non-empty arguments still apply directly for scripting and power users.

- **`/reasoning` → full picker** — five-option picker (Low / Medium / High effort plus Show / Hide reasoning trace). Pre-selects the last-set effort level. Displays a detail panel with description, cost/speed tradeoffs, and current status badge. Args still work: `/reasoning high`, `/reasoning show`.
- **`/personality` → searchable picker** — scrolling list of all available personality presets drawn from config at open time, with a live preview pane showing the persona description. Pre-selects the currently active personality. Args still work: `/personality kawaii`, `/personality clear`.
- **`/stream` → binary picker** — two-option picker (ON / OFF) pre-selected to the current streaming state with a one-line description of each mode. Args still work: `/stream off`.
- **`/statusbar` → binary picker** — two-option picker (Visible / Hidden) pre-selected to the current bar state. Args still work: `/statusbar hidden`.

All four pickers share the same keyboard contract as `/verbose`: ↑↓ or Tab to navigate, Enter to apply, Esc to cancel, and any left-click outside the popup to dismiss.

#### Documentation
- **Config reference** — `display.tool_progress`, `display.show_status_bar`, `display.show_cost`, `display.compact`, `display.streaming`, `display.show_reasoning`, and `display.skin` are now documented in `reference/configuration.md` with types and defaults.
- **User guide** — `user-guide/configuration.md` now documents the `tool_progress` table (`off | new | all | verbose`) and explains live TUI toggling with `/verbose`.

### Changed

- **Tool progress defaults to `verbose`** — the out-of-the-box experience now shows every tool call plus curated plan/result detail lines. Previously the default was `all`. Existing configs are unaffected; only new installations change behaviour. Live-toggle with `/verbose` or `/verbose <off|new|all|verbose>`.
- **`/verbose` command description updated** — now reads "Open tool-progress picker (TUI); or set directly: /verbose [off|new|all|verbose]" to reflect picker-first UX.

### Fixed

- **`edgecrab-lsp` crates.io publish** — the `description` field was missing from the crate manifest, blocking the Rust release workflow. Added description and re-released.

### Internal

- **DRY picker helpers** — four module-level free functions (`popup_rect`, `picker_three_layout`, `picker_two_cols`, `picker_help_line`) replace ~140 lines of repeated boilerplate across five picker render methods, satisfying the Open/Closed Principle: adding a new picker touches only its own render function.
- **`reasoning_effort_hint` caching** — `handle_set_reasoning` now records the last-applied low/medium/high effort so the `/reasoning` picker can pre-select it on re-open.

---

## [0.1.3] — 2026-04-09

### Added

#### CLI & TUI
- **ADR-backed CLI/TUI audit spec set** — new documents under `specs/cli_improve/` cross-reference the clap subcommand tree, slash-command registry, TUI handlers, config UX, and verification plan.
- **ADR-backed binary CLI audit spec set** — new documents under `specs/cli_command_improve/` audit the full terminal command surface, from clap entry/help/version behavior through nested subcommand wiring and runtime-home semantics.
- **TUI config center** — `/config` now opens a searchable control surface that summarizes runtime configuration, exposes important paths, and routes directly into model, vision, image, display, skin, voice, gateway-home, and update actions.
- **Selector-grade cheap-model routing UX** — `/cheap_model` now matches `/model` with a fast model picker, persisted smart-routing state, `status`, and `off` flows instead of relying on manual YAML edits.
- **First-class MoA configuration UX** — `/moa` now exposes live status, aggregator selection, explicit expert add/remove flows, searchable full-roster editing, reset behavior, and config-center entry points; the canonical callable tool is now `moa` while keeping `mixture_of_agents` as a legacy alias, and persisted `moa` defaults are consumed when per-call overrides are omitted.
- **MoA runtime hardening and operator controls** — `moa.enabled` now gates `moa` cleanly, `/moa on|off` mirrors the cheap-model enable/disable workflow, model specs are normalized and deduplicated before persistence, and MoA no longer silently falls back to the active model while claiming a different roster.
- **MoA activation is now end-to-end correct** — the default `core` toolset alias now expands to `moa`, `/moa on` repairs literal toolset whitelist/blacklist entries before enabling the feature, `/moa status` reports config-vs-toolset effective availability, and the system prompt now tells models to call `moa` directly when the user asks for multi-model consensus.
- **MoA secondary Copilot routing now uses the real provider path** — when MoA needs a second `vscode-copilot/...` model for a reference or aggregator step, it now reuses the same direct Copilot construction path as the CLI instead of falling back to the broken localhost proxy factory path; live MoA Copilot coverage is now exercised by an ignored end-to-end test.
- **MoA now degrades safely instead of dying on stale rosters** — the active chat model is appended as a last-chance safety expert at runtime, aggregation now falls back through an ordered candidate list when the configured aggregator is unsupported, `/moa status` documents the safety behavior, `/moa reset` writes a safe current-session baseline, and live Copilot E2E coverage now exercises both unsupported-aggregator and unsupported-reference fallback paths.
- **Binary command contract hardening** — `--config` now rebases the effective runtime home for binary commands, `config edit` supports argument-bearing `$EDITOR` values such as `code --wait`, `config env-path` respects the active config root, and `version` now reflects the model catalog provider inventory instead of a hardcoded subset.
- **Browser launch tests are explicit opt-in** — real-Chrome launch paths are now suppressed under Rust test harnesses unless `EDGECRAB_RUN_BROWSER_LAUNCH_TESTS=1` is set, so `cargo test -- --include-ignored` no longer opens Chrome by side effect.

#### Model Discovery
- **Dynamic provider discovery with principled fallback** — `edgecrab-core` now supports provider-scoped live model discovery for OpenRouter, Ollama, LM Studio, Google Gemini, GitHub Copilot, and AWS Bedrock, with per-provider cache plus static catalog fallback instead of generic `/v1/models` heuristics.
- **AWS Bedrock as a first-class provider** — the embedded model catalog now includes Bedrock model IDs, the runtime provider is enabled in normal builds, and Bedrock discovery is documented and spec'd under `specs/dynamic_model/`.
- **ADR-backed model discovery specification** — new design and test documents under `specs/dynamic_model/` cover architecture, provider support matrix, cache/fallback rules, TUI behavior, Bedrock constraints, and verification strategy.

#### MCP
- **`edgecrab mcp doctor` and `/mcp doctor`** — configured MCP servers can now be diagnosed with static config checks plus live probe output, including command resolution, `cwd` validation, auth source hints, filter summaries, and sample discovered tools.
- **Exceptional TUI MCP control flow** — the MCP browser now has a dedicated `check` action for configured servers, making install, view, test, diagnose, and remove available from one overlay.
- **Native multi-source MCP search in the TUI** — `/mcp search [query]` now opens a dedicated remote browser that searches steering-group reference servers, official integrations, archived upstream entries, and the official MCP Registry with per-source provenance and background refresh.
- **Deterministic install plans for searchable official MCP entries** — remote MCP search results can now be installed directly when EdgeCrab can launch them safely: bundled presets, streamable HTTP registry entries, npm stdio packages, and PyPI stdio packages. Unsupported registry transports remain visible but view-only.
- **OAuth-style HTTP MCP auth is now env-friendly** — `bearer_token` and HTTP header values from `mcp_servers` expand `${ENV_VAR}` placeholders at load time, so short-lived access tokens can be injected safely from the environment instead of hardcoding secrets.
- **OAuth-aware MCP operator UX** — configured HTTP MCP servers now surface OAuth mode, token endpoint, grant type, token-cache freshness, and missing refresh/client credential signals through `/mcp view`, `/mcp doctor`, and the TUI MCP browser instead of collapsing everything into a generic auth warning.
- **Actionable MCP OAuth workflow guidance** — `edgecrab mcp auth <server>` and `/mcp auth <server>` now tell the operator which auth path is active, what is missing, and what to do next.
- **Refresh-token-aware MCP token storage** — `/mcp-token` now supports refresh-token caching and per-server token status, so refresh-token OAuth servers are operable from the TUI instead of only diagnosable.
- **Interactive MCP OAuth login** — `edgecrab mcp login <server>` and `/mcp login <server>` now run real interactive OAuth flows for HTTP MCP servers, including device-code login and browser-based authorization-code login with loopback callback support.
- **Browser OAuth gap closed further** — browser-based MCP login now supports dynamic loopback redirect ports for busy local environments, validates redirect safety in doctor/auth output, and has end-to-end authorization-code coverage in the CLI test suite.
- **Code-grounded MCP specification set** — new ADR-style documents under `specs/mcp/` define the MCP transport plane, TUI operator UX, path parsing model, and edge-case roadblocks from the current implementation outward.

#### Skills Hub
- **Curated remote skill discovery** — `edgecrab skills search`, `/skills search`, and the `skills_hub` tool now search live skill catalogs from Hermes Agent, EdgeCrab, OpenAI Skills, Anthropic Skills, and `skills.sh`, with per-source origin and trust labels.
- **Interactive remote skill browser in TUI** — `/skills search [query]` and `/skills hub [query]` now open a searchable full-screen browser with live background refresh, source/error notes, install-or-update actions, and local/remote browser switching instead of dumping raw text into the transcript.
- **Cached parallel remote indexing** — remote GitHub-backed skill sources are searched in parallel, indexed through the GitHub tree API, cached under `~/.edgecrab/skills/.hub/index-cache/`, and reused on slow or failed refreshes for better long-search UX.
- **Curated install aliases** — remote skills can now be installed with short identifiers like `edgecrab:<path>` and `hermes-agent:<path>` in addition to raw `owner/repo/path`.
- **Remote skill updates** — `edgecrab skills update`, `/skills update`, and `skills_hub update` now refresh hub-installed skills from their recorded source identifier and pull the latest remote bundle.

#### Language Server Protocol
- **New `edgecrab-lsp` crate** — dedicated workspace crate for language-server lifecycle, document sync, diagnostics caching, position encoding, edit application, and LLM-facing rendering.
- **25 LSP tools exposed to the agent and ACP/editor surface** — navigation parity with Claude Code plus `lsp_code_actions`, `lsp_apply_code_action`, `lsp_rename`, `lsp_format_document`, `lsp_format_range`, `lsp_inlay_hints`, `lsp_semantic_tokens`, `lsp_signature_help`, `lsp_type_hierarchy_prepare`, `lsp_supertypes`, `lsp_subtypes`, `lsp_diagnostics_pull`, `lsp_linked_editing_range`, `lsp_enrich_diagnostics`, `lsp_select_and_apply_action`, and `lsp_workspace_type_errors`.
- **LSP-aware prompt guidance** — the system prompt now teaches the agent to prefer semantic LSP navigation, diagnostics, rename, formatting, and code actions over grep-style heuristics when a server is available.
- **Configurable language servers** — new top-level `lsp` config with default Rust, TypeScript, JavaScript, Python, Go, C, and C++ server definitions plus per-server command, args, env, root markers, and initialization options.

#### Tests, Automation, and Docs
- **LSP integration and surface regressions** — integration coverage exercises the mock LSP server end to end, and surface tests now prove EdgeCrab exposes more than Claude Code's 9 documented LSP operations on both core and ACP surfaces.
- **Build and release automation updated for `edgecrab-lsp`** — Makefile and crates.io release workflow now include the new crate in publish order, and CI runs the dedicated `edgecrab-lsp` package tests explicitly.
- **Documentation refresh for semantic coding** — README, docs, site pages, configuration reference, and changelog now document LSP setup, capabilities, and the expanded coding toolset.
- **CLI/model-routing documentation refresh** — the CLI architecture, config/TUI deep dive, smart-routing docs, tool catalogue, and config reference now document `/cheap_model`, `/moa`, and the new `moa:` config block consistently.
- **MoA correctness regression coverage** — new tests now cover config sanitization, command dispatch, enable/disable persistence, selector save flows, and the provider-routing logic that prevents fake multi-model runs.

---

## [0.1.0] — 2026-04-06

_First public release. Phase 5: Integration & Polish._

### Added

#### Voice & TUI
- **Voice capture** — microphone input with real-time waveform display in the TUI. Dependency alerts guide users through system requirements (portaudio, sox, etc.).
- **Voice playback** — gateway voice delivery with cross-platform audio. `/voice` command toggles voice mode.
- **TUI MCP management** — `/mcp` workflow for searching, installing, and testing MCP presets directly from the TUI without leaving the session.
- **Improved TUI waiting UX** — animated spinner with contextual status messages during long operations.
- **Streaming presence indicators** — token-level streaming display keeps the cursor live while the model responds.

#### Media & Documents
- **EdgeParse PDF conversion** (`edgecrab-tools`) — `EdgeParseTool` extracts text from PDF files; powered by `pdf-extract` crate.
- **Media UX improvements** — image display in TUI via ratatui image widgets; `browser_get_images` results render inline.

#### MCP Integration
- **Curated MCP catalog** — 50+ pre-vetted MCP presets bundled. `edgecrab mcp search <query>` filters by capability. `edgecrab mcp install` clones and wires presets in one command.
- **EdgeCrab ACP identity** (`edgecrab-acp`) — Agent Communication Protocol implementation enabling multi-agent coordination over HTTP/WebSocket.

#### Agent & Core
- **Agent observability** — structured span events for each ReAct iteration, tool call, and provider exchange. Compatible with OpenTelemetry collectors.
- **Skill provisioning** — guarded installs verify checksum and sandboxed execution before adding a new skill to the agent's tool inventory.
- **Prompt pipeline pressure relief** — adaptive token budget management prevents context overflow in long sessions without triggering LLM compression unnecessarily.
- **File execution hardening** — `RunFileTool` validates script shebangs, enforces 5-minute timeout, caps stdout at 50 KB, strips API keys before spawn.

#### SDKs & CI/CD
- **Python SDK** (`sdks/python/`) — `edgecrab-sdk` on PyPI. Sync/async clients, Agent/AsyncAgent with conversation history, streaming, CLI (`edgecrab chat/models/health`). 85 tests.
- **Node.js SDK** (`sdks/node/`) — `edgecrab-sdk` on npm. TypeScript-first with Agent, streaming (async generator), CLI. CJS + ESM dual build. 39 tests.
- **CI workflow** (`.github/workflows/ci.yml`) — Rust build/test/clippy/fmt on ubuntu/macos/windows matrix, Python SDK tests, Node.js SDK tests, cargo-audit security scan.
- **Release workflows** — `release-rust.yml` (10 crates to crates.io in dependency order), `release-python.yml` (wheels + sdist to PyPI), `release-node.yml` (npm publish with pack + GitHub Release upload), `release-docker.yml` (multi-arch Docker image to GHCR).
- **Makefile** — `make build`, `make ci`, `make test-sdks`, `make publish-all`, colored help output.
- **CONTRIBUTING.md** — Bug reports, PRs, dev setup, SDK development, coding guidelines.
- **Docker support** — Multi-stage `Dockerfile` and `docker-compose.yml` for gateway server deployment.

#### `edgecrab-core`
- **`Agent::chat_streaming()`** — new async method that delivers tokens via `UnboundedSender<StreamEvent>` as they arrive from the provider. Uses `chat_with_tools_stream()` when supported; falls back to chunked replay of buffered response for providers that don't support streaming.
- **`StreamEvent` enum** — `Token(String)`, `Done`, `Error(String)` events sent from streaming agent to TUI event loop.
- **`compress_with_llm()`** in `compression.rs` — LLM-powered context compression that summarizes old messages using the active provider. Falls back to structural summary on LLM failure.
- **`build_chat_messages_pub()`** in `conversation.rs` — public alias of `build_chat_messages()` so agent.rs streaming path can reuse role-mapping logic without duplication.
- **LLM compression wired into conversation loop** — `execute_loop()` now checks `needs_compression()` before each API call and runs `compress_with_llm()` when the threshold is exceeded.

#### `edgecrab-tools`
- **`RunProcessTool`** in `process.rs` — real background process spawning via `tokio::process::Command`. Drains stdout/stderr asynchronously via `drain_reader()` background tasks, feeding lines into the shared `ProcessTable` ring buffer. Security: scanned by `CommandScanner` before spawning.
- **`WebSearchTool`** now hits the real DuckDuckGo Instant Answer API (`api.duckduckgo.com`). Returns `RelatedTopics` as a formatted list. No API key required.
- **`WebExtractTool`** now fetches URLs via `reqwest`, strips HTML tags using a lazy-compiled regex, and decodes common HTML entities. SSRF protection via `is_safe_url()` before every request.
- **8 new tests** for web tools: `strip_html_basic`, `strip_html_whitespace_collapsed`, `urlencoding_spaces`, `urlencoding_special_chars`, `web_search_available`, `web_extract_available`, `validate_url_blocks_private`, `validate_url_allows_public`.

#### `edgecrab-cli`
- **YAML skin engine** — `SkinConfig` reads `~/.edgecrab/skin.yaml` at startup (falls back silently to defaults). `Theme::from_skin()` converts hex color strings (`#RRGGBB`) to ratatui `Color::Rgb` styles. Customizable: all 7 semantic colors + `prompt_symbol` + `tool_prefix`.
- **Model hot-swap** — `/model provider/model-name` now creates a new provider via `ProviderFactory::create_llm_provider()` and calls `agent.swap_model()` to atomically replace the provider. Live-updates status bar.
- **Session management** — `/session new` and `/new` call `agent.new_session()` and clear the output area.
- **Theme reload** — `/theme` reloads `~/.edgecrab/skin.yaml` without restart.
- **Streaming display** — TUI background task uses `Agent::chat_streaming()`. Tokens arrive via `AgentResponse::Token(text)` and are appended to the current streaming line in `check_responses()`. `AgentResponse::Done` finalizes the line.
- **6 new theme tests**: `parse_hex_valid`, `parse_hex_no_hash`, `parse_hex_invalid`, `skin_config_color_override`, `skin_config_custom_symbols`, `theme_load_falls_back_on_missing_file`.

#### `edgecrab-types`
- **`ToolError::ExecutionFailed { tool: String, message: String }`** — new error variant for runtime tool failures (e.g. process spawn errors).

#### E2E Tests
- **3 new E2E tests** in `crates/edgecrab-core/tests/e2e_copilot.rs`:
  - `e2e_agent_chat_with_copilot_gpt4_mini` — full round-trip via VS Code Copilot
  - `e2e_agent_streaming_with_copilot` — streaming tokens via `chat_streaming()`
  - `e2e_model_swap_with_copilot` — hot-swap between two provider instances
  - All `#[ignore]`d unless `VSCODE_IPC_HOOK_CLI` or `VSCODE_COPILOT_TOKEN` is set.

### Changed

- **Slash command honesty pass** — `/theme` now opens the skin browser by default and reserves `reload` for explicit `skin.yaml` reloads, matching the documented UX.
- **Status bar is now a real persisted preference** — `/statusbar [on|off|toggle|status]` updates runtime state and stores the choice in `display.show_status_bar`.
- **Approval and gateway utility commands now operate on real state** — `/approve`, `/deny`, `/sethome`, and `/update` now resolve active approval flow, supported home-channel config, and local git-based update status instead of placeholder output.

- **`/model` waiting UX is no longer blocking** — the TUI model selector now opens immediately from the embedded catalog and refreshes live inventories in place, instead of replacing the full screen with a loading overlay.
- **`/models` is now TUI-friendly** — provider inventory output is summarized by provider with counts and discovery status, exact-provider reports show source/fallback truthfully, and Bedrock remains visible even when live discovery is feature-gated out of the build.

- **TUI `/mcp` parsing is now quote-aware and cross-platform** — `/mcp install ...` supports quoted `--path` / `--name` and `key=value` forms without breaking on spaces or Windows-style backslash paths.
- **Default model switched to local Ollama** — fresh config defaults and the agent fallback model now use `ollama/gemma4:latest`.
- **Remote GitHub skill install is now recursive** — installing a distant skill bundle preserves nested templates, references, and support files instead of downloading only top-level files.
- `Theme::prompt_symbol` and `Theme::tool_prefix` changed from `&'static str` to `String` to support runtime overrides from `skin.yaml`.
- `App::check_responses()` now handles the `AgentResponse` enum instead of a plain struct — dispatches `Token`, `Done`, and `Error` variants.
- `compression.rs` doc comment updated to describe both structural and LLM strategies.

### Fixed
- Web tool docs: fixed doc list continuation without blank line (clippy `rustdoc::invalid_html_in_doc_comments`).
- `AgentResponse::Text` variant removed (was never constructed — replaced by streaming `Token` + `Done`).
- Removed unused `config` variable in `Agent::chat_streaming()`.
- `block_on(async move { unit_expr })` changed to drop spurious `let _ =` to satisfy `clippy::let_unit_value`.

### Stats
- Rust tests: **1629 passing**, 0 failed, 8 ignored — `cargo test --workspace`
- Python SDK: **85 passing** — `pytest sdks/python/tests/`
- Node.js SDK: **39 passing** — `npm test` in `sdks/node/`
- **Total: 1753 tests across all packages**
- Clippy: **0 warnings** with `-D warnings`
- Build time: ~5s (debug), clean cache
- Binary size: **~49 MB** for current stripped macOS arm64 release builds; startup time depends on provider and environment
