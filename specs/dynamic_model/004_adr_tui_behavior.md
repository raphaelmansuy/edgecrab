# ADR 004: TUI and CLI Behavior

## Status

Accepted

## Decision

The TUI and slash commands must expose discovery truthfully.

### `/models`

- `/models` without arguments shows provider inventory with counts, discovery
  status, and current-provider markers instead of dumping every model inline.
- `/models <provider>` uses live discovery only when the provider supports it.
- Output includes the source: `live API`, `cache`, or `static catalog`.
- `/models refresh [provider|all]` only refreshes providers that actually
  support live discovery.
- Feature-gated providers such as `bedrock` remain visible even when runtime
  discovery is disabled in the current build.

### `/provider`

- Show which providers support live discovery.
- Show Bedrock as gated if the feature is not enabled.

### Model selector

- Background refresh should include all live-discovery providers plus the
  current provider if different.
- `/model` should open immediately from the static catalog and keep keyboard
  navigation responsive while live discovery runs.
- Discovery refresh should update selector rows in place instead of replacing
  the entire TUI with a blocking spinner overlay.
- Selector labels stay simple: `provider/model`.
- The selector should not imply that every provider is dynamically refreshed.

## UX requirements

- No silent downgrade in messaging.
- No fake "refresh" for static-only providers.
- No provider alias confusion (`copilot` vs `vscode-copilot`, `google` vs
  `gemini`, `lm-studio` vs `lmstudio`).

## Consequences

- Users can understand whether they are seeing a live inventory or a fallback.
- Support burden drops because the TUI no longer overclaims discovery behavior.
