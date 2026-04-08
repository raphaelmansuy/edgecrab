# EdgeCrab MCP TUI and Operations

See [architecture.md](./architecture.md) for the layer model and [ADR-002](./adr-002-tui-first-mcp-operator-ux.md) for the decision rationale.

## 1. Operator Stories

### Story A: install a path-scoped server from the TUI

The user opens `/mcp`, selects `filesystem`, installs it, and expects the resulting config to contain the normalized workspace path for the current platform.

Requirements:

- The install flow must write `AppConfig.mcp_servers`.
- The rendered path must be normalized for Windows and Unix.
- The resulting TUI output must tell the user what to run next.

### Story B: diagnose a broken configured server

The user sees a configured server in the selector but the agent cannot use it.

Requirements:

- TUI must expose a doctor/check action for configured entries.
- TUI must expose an auth workflow that tells the operator the next action, not just the failure symptom.
- TUI must expose a login workflow for interactive OAuth servers so operators can finish auth without dropping to manual token surgery.
- Doctor output must distinguish:
  - bad config
  - missing command
  - invalid `cwd`
  - missing auth
  - broken OAuth setup
  - expired or missing cached OAuth token state
  - probe/connect failure
  - successful connection with zero tools

### Story C: pass a path with spaces

The user runs:

```text
/mcp install filesystem --path "C:\Users\Raphael\My Project"
```

or:

```text
/mcp install filesystem path="/Users/raphael/My Project"
```

Requirements:

- Quoted values must survive parsing.
- Backslashes in Windows paths must survive parsing.
- `name=` and `--name` should both work.
- `path=` and `--path` should both work.

## 2. TUI UX Rules

### `/mcp` without args

Must open the selector overlay and never dump a text list by default.

Reason:

- The selector is already the superior UX.
- TUI users should browse interactively by default.

### `/mcp search [query]`

Must open a dedicated remote MCP browser rather than reusing the local selector.

Reason:

- Remote discovery has different semantics from local operations.
- Source labels and per-source notices matter when aggregating official upstream sources plus the official registry.
- The browser must stay responsive while live registry results refresh in the background.

### Configured entries

Configured entries must expose:

- `Enter`: default action, normally `test`
- `v`: view config detail
- `c`: doctor/check
- `d` or `Delete`: remove

### Catalog entries

Catalog entries must expose:

- `Enter`: install if the entry is installable, otherwise view
- `i`: install
- `v`: view

### Remote official search entries

Remote official search entries must expose:

- `Enter`: install when EdgeCrab has a deterministic install plan, otherwise view
- `i`: install when supported
- `v`: view
- `l`: return to the local `/mcp` browser

Install support is intentionally narrower than search support:

- supported: bundled presets, streamable HTTP registry entries, npm stdio registry entries, PyPI stdio registry entries
- view-only: unsupported registry transports such as SSE or package types EdgeCrab cannot launch deterministically yet

## 3. CLI Rules

The CLI must remain scriptable and stable:

```bash
edgecrab mcp list
edgecrab mcp view github
edgecrab mcp install filesystem --path "/tmp/workspace"
edgecrab mcp test github
edgecrab mcp doctor
edgecrab mcp doctor github
edgecrab mcp auth github
edgecrab mcp login github
```

The CLI is not secondary. It is the automation surface. The TUI is the operator surface.

## 4. Output Rules

Doctor output should be compact, not verbose prose.

Auth output should be operational:

- active auth mode
- cache state
- next step
- explicit refresh-token guidance when that flow is configured

Interactive login output should be operator-safe:

- the verification or authorization URL
- the device code when the server uses device flow
- the effective loopback redirect URL when browser login is using a dynamic local port
- a success/failure summary that states whether refresh token caching succeeded

Browser-loopback login rules:

- `oauth.redirect_url` must be an `http` loopback URL (`localhost`, `127.0.0.1`, or `::1`).
- A redirect without an explicit port, or with port `0`, means EdgeCrab should allocate a free local callback port dynamically.
- A fixed-port redirect remains supported when the operator or provider requires it.

Good:

```text
github  warn
  transport: http https://example.com/mcp
  auth: none configured
  probe: fail 401 Unauthorized
```

Bad:

```text
The server called github appears to maybe be configured for HTTP, but there are some possible issues...
```

## 5. Why This Exceeds Claude Code and Hermes

Claude Code excels at coding workflows, but EdgeCrab can exceed it in MCP operations by exposing:

- controlled preset install
- config-backed server lifecycle
- hot reload
- probe plus doctor
- auth plus refresh-token guidance
- interactive OAuth login from the MCP control plane
- TUI-native browse/test/diagnose/remove flows

Hermes established the agent model. EdgeCrab should exceed it by giving MCP the same operational quality bar as model routing or gateway setup.
