# Config Schema — `plugins:` Section

**Status:** PROPOSED  
**Version:** 0.1.0  
**Date:** 2026-04-09  
**Cross-refs:** [003_manifest], [005_lifecycle], [009_discovery_hub], [010_cli_commands]

---

## 1. Overview

The plugin system adds a new top-level `plugins:` key to `~/.edgecrab/config.yaml`.

This document specifies:
1. The full YAML schema for `plugins:`
2. The corresponding Rust struct `PluginsConfig`
3. How it integrates into the existing `AppConfig`
4. Environment variable overrides
5. Migration notes (compatibility with existing `skills:` section)

---

## 2. Full YAML Schema

```yaml
plugins:
  # ── Global switches ─────────────────────────────────────────
  enabled: true                # Master switch — disable all plugins at once
  auto_enable: true            # Start plugins marked as Approved on agent startup
  call_timeout_secs: 60        # Timeout for a single tool call to a plugin (0 = no limit)
  startup_timeout_secs: 10     # Timeout for plugin subprocess startup handshake

  # ── Disabled plugins ────────────────────────────────────────
  disabled: []                 # List of plugin names to disable on startup
                               # (These override auto_enable)
  # Example:
  # disabled:
  #   - old-helper
  #   - experimental-plugin

  # ── Installation directory ──────────────────────────────────
  install_dir: "~/.edgecrab/plugins"    # Where plugins are stored
  quarantine_dir: "~/.edgecrab/plugins/.quarantine"  # Staging during install

  # ── Security settings ───────────────────────────────────────
  security:
    min_trust_level: "unverified"   # Minimum trust to install without --force
                                    # Options: official | community | unverified
    allow_caution: false            # If false, Caution verdict requires --force
    max_tool_count: 100             # Max tools a single plugin may register
    max_skill_size_kb: 512          # Max size of a SKILL.md (kilobytes)
    scan_on_load: true              # Re-scan plugins on every agent startup
                                    # (not just on install)

  # ── Hub configuration ───────────────────────────────────────
  hub:
    enabled: true
    cache_ttl_secs: 900             # 15 minutes
    sources: []                     # Additional hub sources (beyond built-ins)
    # Example:
    # sources:
    #   - url:            "https://mycompany.com/edgecrab-plugins/index.json"
    #     name:           "corp-internal"
    #     trust_override: "community"   # unverified | community (community is max)

  # ── Host API limits ─────────────────────────────────────────
  host_api:
    max_memory_write_per_min: 60
    max_secret_get_per_min: 20
    max_inject_per_min: 5
    max_tool_delegate_per_min: 30
    max_log_per_min: 200

  # ── Per-plugin overrides ─────────────────────────────────────
  overrides: {}
  # Example:
  # overrides:
  #   github-tools:
  #     call_timeout_secs: 30
  #     disabled: false
  #   slow-plugin:
  #     call_timeout_secs: 120
```

---

## 3. Rust Structs

```rust
// In edgecrab-plugins/src/config.rs

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct PluginsConfig {
    pub enabled:               bool,
    pub auto_enable:           bool,
    pub call_timeout_secs:     u64,
    pub startup_timeout_secs:  u64,
    pub disabled:              Vec<String>,
    pub install_dir:           PathBuf,
    pub quarantine_dir:        PathBuf,
    pub security:              PluginsSecurityConfig,
    pub hub:                   PluginsHubConfig,
    pub host_api:              HostApiLimitsConfig,
    pub overrides:             HashMap<String, PluginOverrideConfig>,
}

impl Default for PluginsConfig {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        Self {
            enabled:              true,
            auto_enable:          true,
            call_timeout_secs:    60,
            startup_timeout_secs: 10,
            disabled:             vec![],
            install_dir:          home.join(".edgecrab/plugins"),
            quarantine_dir:       home.join(".edgecrab/plugins/.quarantine"),
            security:             PluginsSecurityConfig::default(),
            hub:                  PluginsHubConfig::default(),
            host_api:             HostApiLimitsConfig::default(),
            overrides:            HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct PluginsSecurityConfig {
    pub min_trust_level:    TrustLevel,     // unverified
    pub allow_caution:      bool,           // false
    pub max_tool_count:     usize,          // 100
    pub max_skill_size_kb:  usize,          // 512
    pub scan_on_load:       bool,           // true
}

impl Default for PluginsSecurityConfig {
    fn default() -> Self {
        Self {
            min_trust_level:   TrustLevel::Unverified,
            allow_caution:     false,
            max_tool_count:    100,
            max_skill_size_kb: 512,
            scan_on_load:      true,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct PluginsHubConfig {
    pub enabled:        bool,             // true
    pub cache_ttl_secs: u64,             // 900
    pub sources:        Vec<HubSource>,  // []
}

impl Default for PluginsHubConfig {
    fn default() -> Self {
        Self {
            enabled:        true,
            cache_ttl_secs: 900,
            sources:        vec![],
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct HubSource {
    pub url:            String,
    pub name:           String,
    pub trust_override: Option<TrustLevel>,   // max: community
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct HostApiLimitsConfig {
    pub max_memory_write_per_min: u32,   // 60
    pub max_secret_get_per_min:   u32,   // 20
    pub max_inject_per_min:       u32,   // 5
    pub max_tool_delegate_per_min:u32,   // 30
    pub max_log_per_min:          u32,   // 200
}

impl Default for HostApiLimitsConfig {
    fn default() -> Self {
        Self {
            max_memory_write_per_min:  60,
            max_secret_get_per_min:    20,
            max_inject_per_min:        5,
            max_tool_delegate_per_min: 30,
            max_log_per_min:           200,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct PluginOverrideConfig {
    pub call_timeout_secs: Option<u64>,
    pub disabled:          Option<bool>,
}
```

---

## 4. Integration into AppConfig

```rust
// In edgecrab-core/src/config.rs — add one field

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
pub struct AppConfig {
    // ... existing fields ...
    #[serde(default)]
    pub plugins: PluginsConfig,
}
```

The `PluginsConfig` type lives in `edgecrab-plugins` and is re-exported from `edgecrab-core` to avoid circular dependencies:

```rust
// edgecrab-core/src/config.rs
pub use edgecrab_plugins::config::PluginsConfig;
```

---

## 5. Environment Variable Overrides

| Variable | Config key | Values |
|---|---|---|
| `EDGECRAB_PLUGINS_ENABLED` | `plugins.enabled` | `true` / `false` |
| `EDGECRAB_PLUGINS_AUTO_ENABLE` | `plugins.auto_enable` | `true` / `false` |
| `EDGECRAB_PLUGINS_CALL_TIMEOUT` | `plugins.call_timeout_secs` | integer seconds |
| `EDGECRAB_PLUGINS_HUB_ENABLED` | `plugins.hub.enabled` | `true` / `false` |
| `EDGECRAB_PLUGINS_SCAN_ON_LOAD` | `plugins.security.scan_on_load` | `true` / `false` |

Environment variables override the config YAML value per the existing resolution order:
**defaults → config.yaml → env vars → CLI args**

---

## 6. Config Validation Rules

These are checked at agent startup, not deferred to first use:

```
1. install_dir must exist or be creatable
2. quarantine_dir must exist or be creatable
3. call_timeout_secs must be >= 0
4. startup_timeout_secs must be 5..=60
5. security.max_tool_count must be 1..=1000
6. security.max_skill_size_kb must be 1..=10240
7. hub.sources[].trust_override, if set, must be "unverified" or "community"
   (not "official" or "verified" — INV-6)
8. hub.cache_ttl_secs must be >= 60
9. overrides keys must match installed plugin names (warn-only, not error)
10. disabled list entries that don't match any installed plugin: warn-only
```

Validation failures for items 1-8 cause a fatal startup error with a clear message.
Items 9-10 emit `tracing::warn!` but do not stop the agent.

---

## 7. Effective Config Computation

When the registry resolves the effective timeout for a tool call, per-plugin overrides win:

```rust
fn effective_call_timeout(config: &PluginsConfig, plugin_name: &str) -> Duration {
    let secs = config.overrides
        .get(plugin_name)
        .and_then(|o| o.call_timeout_secs)
        .unwrap_or(config.call_timeout_secs);
    if secs == 0 {
        Duration::MAX
    } else {
        Duration::from_secs(secs)
    }
}
```

And for the `disabled` list:

```rust
fn is_disabled(config: &PluginsConfig, plugin_name: &str) -> bool {
    config.disabled.contains(&plugin_name.to_string())
    || config.overrides
         .get(plugin_name)
         .and_then(|o| o.disabled)
         .unwrap_or(false)
}
```

---

## 8. Relation to Existing `skills:` Section

The existing `skills:` section in `config.yaml` (managing SKILL.md files) is NOT removed.
It remains the configuration for the legacy Skill files that are not plugins.

The new `plugins:` section covers the plugin system from this spec.
The `SkillPlugin` type internally converts SKILL.md files to plugins, but they are
discovered from `~/.edgecrab/plugins/<name>/SKILL.md` rather than `~/.edgecrab/skills/`.

Migration note: Users who have existing SKILL.md files in `~/.edgecrab/skills/` will
continue to use those via the existing `skills:` mechanism. They can optionally
wrap them as SkillPlugins by creating a `plugin.toml` alongside the SKILL.md.
There is NO automatic migration of existing skill files to plugins.

---

## 9. Minimal Config Example

A user who wants plugins enabled but no hub (air-gapped environment):

```yaml
plugins:
  enabled: true
  hub:
    enabled: false
```

A user who wants stricter security:

```yaml
plugins:
  security:
    min_trust_level: "official"
    allow_caution: false
    scan_on_load: true
```

A developer testing a local plugin with a longer timeout:

```yaml
plugins:
  overrides:
    my-dev-plugin:
      call_timeout_secs: 300
```
