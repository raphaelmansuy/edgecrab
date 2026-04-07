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
- Doctor output must distinguish:
  - bad config
  - missing command
  - invalid `cwd`
  - missing auth
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

## 3. CLI Rules

The CLI must remain scriptable and stable:

```bash
edgecrab mcp list
edgecrab mcp view github
edgecrab mcp install filesystem --path "/tmp/workspace"
edgecrab mcp test github
edgecrab mcp doctor
edgecrab mcp doctor github
```

The CLI is not secondary. It is the automation surface. The TUI is the operator surface.

## 4. Output Rules

Doctor output should be compact, not verbose prose.

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
- TUI-native browse/test/diagnose/remove flows

Hermes established the agent model. EdgeCrab should exceed it by giving MCP the same operational quality bar as model routing or gateway setup.

