# Config and Paths

Verified against:
- `crates/edgecrab-core/src/config.rs`
- `crates/edgecrab-cli/src/profile.rs`

`AppConfig` is the single top-level config struct for the runtime.

## Load order

```text
+-----------------------------+
| defaults                    |
+-----------------------------+
               |
               v
+-----------------------------+
| config.yaml from            |
| EDGECRAB_HOME or ~/.edgecrab|
+-----------------------------+
               |
               v
+-----------------------------+
| EDGECRAB_* env overrides    |
+-----------------------------+
               |
               v
+-----------------------------+
| CLI overrides               |
+-----------------------------+
```

## Top-level config sections in code

- `model`
- `agent`
- `tools`
- `gateway`
- `mcp_servers`
- `memory`
- `skills`
- `security`
- `terminal`
- `delegation`
- `compression`
- `display`
- `privacy`
- `browser`
- `checkpoints`
- `tts`
- `stt`
- `voice`
- `honcho`
- `auxiliary`

There are also top-level runtime flags such as `save_trajectories`, `skip_context_files`, `skip_memory`, `timezone`, and `reasoning_effort`.

## Important environment overrides

- `EDGECRAB_MODEL`
- `EDGECRAB_MAX_ITERATIONS`
- `EDGECRAB_TIMEZONE`
- `EDGECRAB_SAVE_TRAJECTORIES`
- `EDGECRAB_SKIP_CONTEXT_FILES`
- `EDGECRAB_SKIP_MEMORY`
- terminal and gateway-specific `EDGECRAB_*` variables

## Home layout

The default home is `~/.edgecrab`, unless `EDGECRAB_HOME` is set.

Common files and directories:

- `config.yaml`
- `models.yaml`
- `SOUL.md`
- `memories/`
- `skills/`
- `state.db`
- `profiles/`

## Profiles

Named profiles live under `~/.edgecrab/profiles/<name>/` and carry their own:

- `config.yaml`
- `.env`
- `SOUL.md`
- `state.db`

Profile switching changes the effective home directory for runtime commands.
