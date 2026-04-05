# Gateway / Channels Gap Analysis

## Bottom line

EdgeCrab exceeds Hermes on gateway onboarding, operator diagnostics, runtime cohesion, and operator-path validation depth.

EdgeCrab ships 15 gateway adapters in source.

Hermes ships 15 gateway adapters in source.

If the comparison is narrowed to messaging channels only, EdgeCrab ships 14 and Hermes ships 14. Both projects also ship an embedded OpenAI-compatible API server adapter.

EdgeCrab's messaging adapters are:

- Telegram
- Discord
- Slack
- Feishu
- WeCom
- Signal
- WhatsApp
- Webhook
- Email
- SMS
- Matrix
- Mattermost
- DingTalk
- Home Assistant

EdgeCrab's non-messaging gateway adapter is:

- API Server

EdgeCrab now ships one catalog-driven operator surface across all 15 EdgeCrab gateway adapters.

From first principles, raw channel count is not the decisive variable by itself. The decisive variable is whether a shipped adapter can be discovered, configured, validated, and debugged through one coherent control path.

## Code-backed EdgeCrab advantages

### 1. Cleaner gateway decomposition

EdgeCrab's gateway is split into explicit runtime seams:

- `platform.rs`
- `delivery.rs`
- `sender.rs`
- one Rust module per adapter

From first principles, this is the right shape because transport-specific behavior, routing, and outbound delivery are separate responsibilities. That improves extension safety and reduces cross-adapter coupling.

### 2. Better runtime cohesion with the rest of EdgeCrab

EdgeCrab's gateway code lives inside the same async Rust runtime as the agent, tools, session handling, and CLI-adjacent control flow.

That reduces impedance mismatch in three ways:

- fewer language and process boundaries
- fewer serialization seams between gateway and core agent logic
- clearer ownership of cancellation, concurrency, and backpressure

### 3. One operator surface for all shipped adapters

`crates/edgecrab-cli/src/gateway_catalog.rs` is now the single source of truth for:

- shipped gateway channels
- required environment variables
- setup instructions
- completeness diagnostics

That catalog is consumed by both:

- `crates/edgecrab-cli/src/gateway_setup.rs`
- `crates/edgecrab-cli/src/gateway_cmd.rs`

That matters because it removes the previous drift where runtime supported more adapters than setup and status exposed.

In concrete terms, guided setup now covers:

- Telegram
- Discord
- Slack
- Feishu
- WeCom
- Signal
- WhatsApp
- Webhook
- Email
- SMS
- Matrix
- Mattermost
- DingTalk
- Home Assistant
- API Server

This is the stronger shape from a SOLID and DRY perspective:

- one place defines adapter onboarding facts
- one place defines required credentials
- one place defines "ready" versus "incomplete"
- setup and status consume the same law of the system

### 4. Partial configuration now becomes diagnosable instead of silent

`edgecrab gateway status` now derives its platform report from the same catalog-backed completeness logic as setup.

That improves the operator path in two ways:

- env-backed adapters such as SMS, Matrix, Mattermost, DingTalk, Home Assistant, and Email now appear in status when configured
- partial states now surface actionable missing keys instead of degrading into implicit runtime behavior

From first principles, this is what good onboarding means. A system is not "easy" because its happy path is short. It is easy because failure states are made explicit before runtime surprises.

### 5. Stronger regression gates on the operator path

EdgeCrab now regression-tests the catalog, CLI status surface, and gap analysis together:

- source-backed gap audits verify the documented gateway inventory against both repositories
- CLI end-to-end tests verify ready, incomplete, disabled, and provider-specific edge cases across the catalog-driven surface
- `api_server` now uses one consistent identifier across status, routing, and audit paths

From first principles, this matters because an operator surface is only as trustworthy as the tests that keep names, counts, and enablement semantics aligned. EdgeCrab now exceeds Hermes on operator-path validation depth for the shipped surface it exposes.

## Where Hermes still has residual advantage

### 1. Historical runtime exercise

Hermes has more accumulated runtime exercise across the same broad messaging layer. That matters, but it is different from having a cleaner and more tightly regression-gated operator surface.

## Gap verdict

For gateway channels, EdgeCrab now matches Hermes on raw adapter breadth and exceeds Hermes on operator completeness and operator-path validation across that same shipped surface. EdgeCrab exceeds it on:

- gateway decomposition
- same-runtime cohesion
- shared onboarding/status law
- explicit partial-config diagnostics
- operator-path regression depth

Hermes still carries more historical runtime exercise, but the former breadth gap is now closed. The remaining comparison is operational maturity versus a cleaner, more unified, and more tightly audited EdgeCrab surface.

## Sources audited

- `edgecrab/crates/edgecrab-gateway/src/lib.rs`
- `edgecrab/crates/edgecrab-cli/src/gateway_catalog.rs`
- `edgecrab/crates/edgecrab-cli/src/gateway_setup.rs`
- `edgecrab/crates/edgecrab-cli/src/gateway_cmd.rs`
- `edgecrab/crates/edgecrab-cli/tests/gateway_status_e2e.rs`
- `edgecrab/crates/edgecrab-cli/tests/gap_audit.rs`
- `hermes-agent/gateway/platforms/`
- `hermes-agent/hermes_cli/gateway.py`
- `hermes-agent/tests/gateway/`
