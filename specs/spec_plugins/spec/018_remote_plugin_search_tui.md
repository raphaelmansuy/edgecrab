# Remote Plugin Search TUI

**Status:** PROPOSED  
**Version:** 0.1.0  
**Date:** 2026-04-09  
**Cross-refs:** [009_discovery_hub], [010_cli_commands], [017_plugin_tui]

**EdgeCrab source files (CODE IS LAW):**
- `crates/edgecrab-cli/src/app.rs` — `RemotePluginEntry`, `RemotePluginAction`, `RemoteBrowserState<T>`, `open_remote_plugin_selector()`, `schedule_remote_plugin_search()`, `apply_remote_plugin_search_result()`, `run_remote_plugin_action()`, `render_remote_plugin_selector()`
- `crates/edgecrab-cli/src/plugins_cmd.rs` — `/plugins search`, `/plugins browse`, `install_plugin_capture()`, `update_plugin_capture()`
- `crates/edgecrab-cli/src/plugins.rs` — local discovery, including persisted `install_source`
- `crates/edgecrab-plugins/src/hub.rs` — `search_hub_report()`, `PluginSearchReport`, `PluginSearchGroup`, `PluginMeta`, `hub_source_summaries()`
- `crates/edgecrab-plugins/src/discovery.rs` — `DiscoveredPlugin.install_source`

## 1. Purpose

The remote plugin browser gives plugins the same level of official-search TUI support
already available for remote skills.

Goals:

1. Search official and configured plugin registries from the TUI.
2. Surface install, update, and replace actions directly from search results.
3. Preserve source-level context and partial-failure notices.
4. Reuse shared selector/browser infrastructure instead of duplicating the skills browser.

For the built-in official sources, "registry" means live GitHub-backed discovery over
plugin-capable roots only. Standalone skills stay in the remote skills browser.

## 2. Entry Points

The browser opens from:

```text
/plugins search <query>
/plugins search --source <source> <query>
/plugins browse
/plugins hub search <query>   # alias
/plugins hub browse           # alias
R inside the local plugin toggle overlay
```

Behavior:

1. `/plugins search <query>` opens the browser with the query prefilled and starts a debounced search.
2. `/plugins search --source <source> <query>` also applies a source filter.
3. `/plugins browse` opens the browser with an empty query and shows source summaries until the user types.

## 3. Shared Architecture

The browser MUST reuse the generic remote-browser state used by other TUI discovery flows:

```rust
struct RemoteBrowserState<T>
where
    T: Clone + FuzzyItem,
{
    selector: FuzzySelector<T>,
    notices: Vec<String>,
    last_completed_query: Option<String>,
    search_due_at: Option<Instant>,
    inflight_request_id: Option<u64>,
    next_request_id: u64,
    loading_query: Option<String>,
    action_in_flight: Option<String>,
    source_filter: Option<String>,
}
```

This keeps the design DRY and follows SOLID:

1. `RemoteBrowserState<T>` owns generic browser mechanics only.
2. `RemotePluginEntry` carries plugin-specific detail and action state.
3. Search, rendering, and action execution stay separated.

## 4. Search Model

Remote plugin search is backed by `search_hub_report(...)`, not only the legacy flat
search list.

```rust
pub struct PluginSearchReport {
    pub groups: Vec<PluginSearchGroup>,
}

pub struct PluginSearchGroup {
    pub source: PluginHubSourceInfo,
    pub results: Vec<PluginMeta>,
    pub notice: Option<String>,
}
```

Rules:

1. Results remain grouped by source in the backend report.
2. The TUI flattens groups into rows but preserves `source.label` in each row.
3. Source fetch failures become non-fatal notices shown in the detail pane.
4. An empty query clears live results instead of preloading every remote plugin.

## 5. Default Actions

Each remote plugin result is mapped to one default action:

| Condition | Action |
|---|---|
| No local install with matching source or name | `install` |
| Local plugin has matching persisted `install_source` | `update` |
| Local plugin name collides but source differs or is absent | `replace` |

This decision uses local discovery data, including `install_source` populated from
`plugin.toml` `trust.source`.

## 6. Update Semantics

Remote browser updates MUST be real updates, not a UI-only affordance.

Rules:

1. If the installed plugin has a stamped remote `trust.source`, update MUST re-materialize
   from that source, re-scan, verify checksum, and atomically replace the installed directory.
2. If no stamped remote source exists but the plugin directory is a git checkout, update may
   fall back to `git pull --ff-only`.
3. If neither condition holds, update is skipped with a clear message.

## 7. Key Bindings

| Key | Action |
|---|---|
| `↑` / `k` | Move selection up |
| `↓` / `j` | Move selection down |
| `PgUp` / `PgDn` | Page navigation |
| Type / `Backspace` | Update fuzzy query and re-run debounced search |
| `Enter` | Run default action for selected result |
| `I` | Run default action for selected result |
| `U` | Force update only when the selected result maps to an installed remote plugin |
| `R` | Refresh the current remote search |
| `L` | Return to the local plugin toggle overlay |
| `Z` | Toggle fullscreen detail pane |
| `Esc` | Close the browser or exit fullscreen detail |

## 8. Detail Pane Requirements

The detail pane MUST show:

1. Source label and trust level.
2. Install identifier.
3. Description.
4. Kind and origin.
5. Default action explanation.
6. Required environment variables when present.
7. Tags when present.
8. Replace warning when the selected result would overwrite a local name collision.
9. Source notes when one or more registries failed or returned notices.

When the query is empty, the detail pane SHOULD list configured registry sources using
`hub_source_summaries(...)`.

## 9. Validation

Minimum validation for this feature set:

1. TUI command routing tests for `/plugins search`, `/plugins search --source`, and `/plugins browse`.
2. Unit coverage for action derivation: install vs update vs replace.
3. Plugin update coverage ensuring hub-installed plugins can update from persisted source metadata.
