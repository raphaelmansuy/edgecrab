# Discovery & Hub Protocol

**Status:** PROPOSED  
**Version:** 0.1.0  
**Date:** 2026-04-09  
**Cross-refs:** [000_overview], [003_manifest], [005_lifecycle], [006_security], [007_registry]

---

## 1. Overview

The discovery system lets users find and install plugins without knowing their exact source URLs.

It has three layers:

```
  User types: /plugins hub search "github issue"
                         │
  ┌────────────────────────────────────────────────┐
  │  1. CURATED SOURCES                            │
  │     Hardcoded trusted repos (Official level)   │
  │     edgecrab-plugins, hermes-plugins, ...      │
  └──────────────┬─────────────────────────────────┘
                 │
  ┌──────────────▼─────────────────────────────────┐
  │  2. COMMUNITY TAPS                             │
  │     User-added hub.sources in config.yaml      │
  │     Community level trust                      │
  └──────────────┬─────────────────────────────────┘
                 │
  ┌──────────────▼─────────────────────────────────┐
  │  3. DIRECT INSTALL                             │
  │     /plugins install owner/repo/path           │
  │     or local file path                         │
  │     Unverified level trust                     │
  └────────────────────────────────────────────────┘
```

---

## 2. Curated Sources

Official plugin search uses live GitHub-backed sources that already contain
installable plugins. User-configured community taps may still use JSON
indices via `plugins.hub.sources`.

```rust
// In edgecrab-plugins/src/hub.rs

pub const CURATED_PLUGIN_SOURCES: &[CuratedSource] = &[
    CuratedSource {
        name:        "edgecrab-official",
        url:         "https://github.com/raphaelmansuy/edgecrab",
        trust_level: TrustLevel::Official,
        description: "Official EdgeCrab plugin registry",
    },
    CuratedSource {
        name:        "hermes-plugins",
        url:         "https://github.com/NousResearch/hermes-agent",
        trust_level: TrustLevel::Official,
        description: "Hermes Agent compatible plugins",
    },
];

pub struct CuratedSource {
    pub name:        &'static str,
    pub url:         &'static str,
    pub trust_level: TrustLevel,
    pub description: &'static str,
}
```

---

For repo-backed official plugin sources, discovery exposes plugin-capable roots
only. Standalone skills live in the remote skills browser and are not mixed into
remote plugin search results.

---

## 3. Hub Index Format

Community and user-configured plugin taps may serve a JSON index file. This format
remains supported for non-official sources and custom registries.

```json
{
  "version":       "1",
  "generated_at":  "2026-04-09T00:00:00Z",
  "source_name":   "edgecrab-official",
  "plugins": [
    {
      "name":        "github-tools",
      "version":     "1.2.0",
      "description": "Create issues, PRs, and search GitHub repositories",
      "kind":        "tool-server",
      "tags":        ["github", "vcs", "productivity"],
      "author":      "EdgeCrab Team",
      "license":     "MIT",
      "homepage":    "https://github.com/edgecrab/plugins/tree/main/github-tools",
      "install_url": "github:edgecrab/plugins/github-tools",
      "checksum":    "sha256:abc123...",
      "tools":       ["create_github_issue", "search_github", "create_pr"],
      "requires_env":["GITHUB_TOKEN"],
      "capabilities":{"tool_delegate": false, "memory_read": false}
    }
  ]
}
```

### 3.1 Index Field Definitions

| Field | Type | Required | Description |
|---|---|---|---|
| `version` | string | yes | Schema version (currently `"1"`) |
| `generated_at` | ISO-8601 | yes | Timestamp of index generation |
| `source_name` | string | yes | The source that produced this index |
| `plugins[].name` | string | yes | Unique plugin name (slug format) |
| `plugins[].version` | semver | yes | Latest published version |
| `plugins[].kind` | string | yes | `"skill"`, `"tool-server"`, or `"script"` |
| `plugins[].install_url` | string | yes | How to install: `github:owner/repo/path`, `https://...`, or `local:/abs/path` |
| `plugins[].checksum` | string | yes | `sha256:<hex>` of the downloaded archive |
| `plugins[].tools` | [string] | no | Tool names provided (tool-server only) |
| `plugins[].requires_env` | [string] | no | Environment variables the plugin needs |
| `plugins[].capabilities` | object | no | Non-standard capabilities required |
| `plugins[].tags` | [string] | no | Search tags |

---

## 4. Trust Level Determination

Trust level is assigned at install time based on the source:

```
  Install source                 Assigned trust level
  ─────────────────────────────  ────────────────────
  CURATED_PLUGIN_SOURCES (official)   Official
  CURATED_PLUGIN_SOURCES (community)  Community
  User-added hub.sources               Community
  Direct GitHub (owner/repo/path)      Unverified
  Local file path                      Unverified
  Agent-generated (ScriptPlugin)       Verified (agent signed)
```

Trust level is persisted in the `plugins` DB table.
It is displayed in `/plugins list` and `/plugins info`.

Trust propagation invariant (INV-6): **Trust level can only increase via explicit user promotion,
never automatically.** Installing from a community hub with `--trust-override=official`
is allowed only with a user confirmation prompt.

---

## 5. Hub Discovery Client

```rust
pub struct HubClient {
    http:    reqwest::Client,
    cache:   HashMap<String, CachedIndex>,
    config:  PluginsHubConfig,
}

pub struct CachedIndex {
    fetched_at: Instant,
    index:      HubIndex,
}

const CACHE_TTL: Duration = Duration::from_secs(3_600); // 1 hour — matches Hermes INDEX_CACHE_TTL
const SOURCE_TIMEOUT: Duration = Duration::from_secs(12);
const MAX_INDEX_SIZE: usize = 1 * 1024 * 1024; // 1 MiB

impl HubClient {
    /// Fetch (or return cached) the index for a single source.
    pub async fn fetch_source(&mut self, source: &str) -> Result<HubIndex, HubError>;

    /// Aggregate search across all configured sources.
    pub async fn search(&mut self, query: &str) -> Vec<PluginSearchResult>;

    /// Resolve "github:owner/repo/path" to a download URL.
    pub async fn resolve_github(path: &str) -> Result<ResolvedPlugin, HubError>;

    /// Download a skill/plugin file-by-file (GitHub Contents API) into dest.
    /// IMPORTANT: No ZIP archives are used — individual files are fetched.
    pub async fn download(&self, install_url: &str, dest: &Path) -> Result<SkillBundle, HubError>;
}
```

### 5.1 Search Algorithm

```
search("github issue"):
  1. Fetch all source indices (parallel, CACHE_TTL respected)
  2. For each plugin in each index:
       score = title_match(query) * 2.0
              + tag_match(query)  * 1.5
              + desc_match(query) * 1.0
              + is_official(source) * 0.5  // boost official results
  3. Sort descending by score, deduplicate by name (first-seen wins)
  4. Return top-20 results with source attribution
```

### 5.2 TUI Search Report Contract

The interactive plugin browser does not consume the flat ranked list directly.
It consumes a grouped search report so the TUI can remain DRY with the skills
browser while still exposing plugin-specific source behavior.

```rust
pub struct PluginHubSourceInfo {
    pub name: String,
    pub label: String,
    pub trust_level: TrustLevel,
    pub description: String,
    pub url: String,
}

pub struct PluginMeta {
    pub name: String,
    pub identifier: String,   // install/update identifier, usually install_url
    pub description: String,
    pub version: String,
    pub kind: PluginKind,
    pub origin: String,
    pub trust_level: String,
    pub tags: Vec<String>,
    pub install_url: String,
    pub requires_env: Vec<String>,
}

pub struct PluginSearchGroup {
    pub source: PluginHubSourceInfo,
    pub results: Vec<PluginMeta>,
    pub notice: Option<String>,
}

pub struct PluginSearchReport {
    pub groups: Vec<PluginSearchGroup>,
}

pub async fn search_hub_report(
    config: &PluginsConfig,
    query: &str,
    source_filter: Option<&str>,
    limit: usize,
) -> Result<PluginSearchReport, PluginError>;
```

Rules:

1. Results MUST remain grouped by source so the TUI can render source labels and
   preserve partial-failure context.
2. `source_filter` MUST limit the report to matching configured sources without
   changing the query semantics.
3. A fetch failure for one source MUST become `PluginSearchGroup.notice` for that
   source, not a global hard error.
4. The legacy flat `search_hub(...) -> Vec<PluginSearchResult>` API remains valid
   for text-mode CLI output and scripting.

### 5.3 Official Source Resolution

Official sources do not require a remote `index.json`.

Resolution flow:

1. Fetch `git/trees/main?recursive=1` for the published repository.
2. Collect plugin-capable roots only, for example Hermes `plugins/...` directories
   and repo-root Hermes plugin directories.
3. Represent each plugin directory as a plugin result with:
   `identifier = hub:<source>/<root>/<relative_path>`
   `install_url = github:<repo>/<root>/<relative_path>`
4. Lazily fetch the matching `plugin.yaml` or bundled metadata for top-ranked
   results to populate the user-facing description shown in the CLI and TUI.

Standalone skills from `skills/` and `optional-skills/` are handled by the
remote skills browser instead of the remote plugin browser.

---

## 6. Install URL Schemes

The `install_url` field (and the argument to `/plugins install`) supports these schemes:

| Scheme | Example | Notes |
|---|---|---|
| `github:` | `github:edgecrab/plugins/github-tools` | Resolves via GitHub Contents API |
| `https://` | `https://example.com/my-plugin.zip` | Direct archive download |
| `local:` | `local:/path/to/plugin-dir` | Symlink-safe local copy |
| bare path | `./my-plugin` | Resolved relative to cwd |

### 6.1 GitHub Resolution — File-by-File Protocol

**There are no ZIP archives.** Hermes and EdgeCrab both use the GitHub Contents API to
download individual files. EdgeCrab MUST implement the same protocol.

```
github:edgecrab/plugins/github-tools
         │
         ▼
GET https://api.github.com/repos/edgecrab/plugins/contents/github-tools
Header: Accept: application/vnd.github+json
Header: Authorization: Bearer $GITHUB_TOKEN   (if set)
         │
         ▼
JSON array of file/dir entries:
  [
    { "type": "file", "name": "SKILL.md",   "download_url": "https://..." },
    { "type": "dir",  "name": "templates",  "url": "https://api.github.com/..." },
    ...
  ]
         │
   for each entry:
     if type == "file":  GET download_url  → bytes
     if type == "dir":   recurse one level (max depth 2 from skill root)
         │
         ▼
SkillBundle {
    name:       "github-tools",
    files:      BTreeMap { "SKILL.md" => bytes, "templates/issue.md" => bytes, ... },
    source:     "github",
    identifier: "edgecrab/plugins/github-tools",
    trust_level: TrustLevel::Community,
    metadata:   serde_json::Value::Null,
}
```

`GITHUB_TOKEN` env var is used for auth if set, enabling higher rate limits (5000 req/h).
Without it, requests are unauthenticated (60 req/h per IP; sufficient for single installs).

**Full GitHub authentication priority order** (source: `skills_hub.py GitHubAuth._resolve_token()`):

```
Priority 1: GITHUB_TOKEN or GH_TOKEN env var         (PAT method, 5000 req/h)
Priority 2: `gh auth token` subprocess               (gh-cli method, uses gh CLI)
Priority 3: GitHub App JWT                           (github-app method)
             Needs: GITHUB_APP_ID,
                    GITHUB_APP_PRIVATE_KEY_PATH,
                    GITHUB_APP_INSTALLATION_ID
             Token cached 3500 seconds (~58 min)
Priority 4: Unauthenticated                          (60 req/h, public repos only)
```

EdgeCrab MUST implement the same 4-level priority resolution. The `gh` CLI method is
especially valuable in developer environments where `gh auth login` has already been run.

**Excluded during download:** Any entry whose name is in
`[".", "..", ".git", ".github", ".hub"]` MUST be skipped.

### 6.2 Default Tap Registry

Source-verified from `skills_hub.py GitHubSource.DEFAULT_TAPS`:

```python
DEFAULT_TAPS = [
    {"repo": "openai/skills",                  "path": "skills/"},  # trusted
    {"repo": "anthropics/skills",              "path": "skills/"},  # trusted
    {"repo": "VoltAgent/awesome-agent-skills", "path": "skills/"},  # community
    {"repo": "garrytan/gstack",               "path": ""},          # community
]
```

Trust level is determined by `TRUSTED_REPOS = {"openai/skills", "anthropics/skills"}`.
The VoltAgent and garrytan repos are community-level despite being default taps.

EdgeCrab MUST seed its hub with the same 4 default taps for Hermes compatibility.

### 6.3 `inspect()` — Metadata-Only Preview

The hub source interface exposes an `inspect()` method distinct from `fetch()`:
- `inspect(identifier)` downloads **only `SKILL.md`** from the remote skill directory.
  This enables fast browsing without downloading the full bundle.
- `fetch(identifier)` downloads **all files** in the skill directory.

```rust
pub trait SkillSource {
    fn source_id(&self) -> &str;
    fn trust_level_for(&self, identifier: &str) -> TrustLevel;
    fn search(&self, query: &str, limit: usize) -> Vec<SkillMeta>;
    fn fetch(&self, identifier: &str) -> Option<SkillBundle>;   // full download
    fn inspect(&self, identifier: &str) -> Option<SkillMeta>;   // SKILL.md only
}
```

`/plugins hub info <identifier>` uses `inspect()`. `/plugins install <identifier>` uses `fetch()`.

---

## 7. User-Added Taps

Users can add additional hub sources in `config.yaml`:

```yaml
plugins:
  hub:
    sources:
      - url:  "https://mycompany.com/edgecrab-plugins/index.json"
        name: "corp-internal"
        trust_override: "community"   # max: community (cannot self-promote to official)
```

The `trust_override` field may only be omitted (defaults to `unverified`) or
set to `community`. Setting it to `official` or `verified` is rejected at config load time.

---

## 8. Index Validation

Before using any hub index, the client validates:

1. `version == "1"` — reject unknown schema versions
2. `generated_at` not more than 7 days old — reject stale indices
3. Each plugin entry has `name`, `version`, `kind`, `install_url`, `checksum`
4. `checksum` is in `sha256:<hex>` format
5. Total number of plugins ≤ 10 000 (reject oversized indices)
6. Index JSON size ≤ 1 MiB

Validation failures cause the source to be skipped (logged as warning) rather than
crashing or showing partial results.

---

## 9. Checksum Verification

After downloading a plugin archive:

```rust
fn verify_checksum(data: &[u8], expected: &str) -> Result<(), HubError> {
    let expected_hex = expected.strip_prefix("sha256:")
        .ok_or(HubError::BadChecksum("not sha256 format".into()))?;
    let actual = sha2::Sha256::digest(data);
    let actual_hex = hex::encode(actual);
    if actual_hex != expected_hex {
        return Err(HubError::ChecksumMismatch { expected: expected_hex.into(), actual: actual_hex });
    }
    Ok(())
}
```

This is identical to Homebrew's checksum verification pattern.
The checksum is also stored in the `plugins` DB table and re-verified on every `load_all()`.

---

## 10. Hub State Files (Hermes-Compatible)

EdgeCrab persists hub state in a directory that mirrors Hermes exactly:

```
~/.edgecrab/skills/.hub/        (for skill-type plugins)
~/.edgecrab/plugins/.hub/       (for tool-server / script plugins)
├── lock.json                   # Installed plugin provenance
├── taps.json                   # User-added tap registry sources
├── quarantine/                 # Staging during install verification
├── audit.log                   # Append-only JSONL install/remove log
└── index-cache/                # Disk-persisted remote indices
    ├── <source-name>.json
    └── <source-name>.json.etag # ETag for conditional GET
```

The `.hub/` directory is excluded from skill scanning (see §10.3).

### 10.1 lock.json (Hermes-Compatible)

```json
{
  "lock_version": 1,
  "generated_at": "2025-01-15T10:30:00Z",
  "entries": {
    "github-issues": {
      "source":      "github",
      "identifier":  "openai/skills/github-issues",
      "trust_level": "trusted",
      "installed_at":"2025-01-15T10:29:58Z",
      "content_hash":"sha256:abcdef...",
      "files": ["SKILL.md", "templates/issue-template.md"]
    }
  }
}
```

EdgeCrab MUST write `trust_level` values using Hermes alias names (`"trusted"`,
`"community"`, etc.) so that Hermes can read the lock file without modification.

### 10.2 taps.json (Hermes-Compatible)

```json
{
  "taps": [
    { "name": "official",    "url": "https://raw.githubusercontent.com/openai/skills/main/index.json", "enabled": true },
    { "name": "clawhub",    "url": "https://clawhub.io/index.json", "enabled": true },
    { "name": "corp-tap",   "url": "https://mycompany.com/skills/index.json", "enabled": false }
  ]
}
```

EdgeCrab reads Hermes-written `taps.json` without modification.
Entries with `enabled: false` are excluded from search and install.

### 10.3 Excluded Directories

```rust
const EXCLUDED_SKILL_DIRS: &[&str] = &[".git", ".github", ".hub"];
```

These are skipped at all levels during skill scanning.

### 10.4 audit.log (Append-Only JSONL)

```jsonc
{"ts":"2025-01-15T10:29:58Z","event":"install","name":"github-issues","source":"official","trust":"trusted","verdict":"allow"}
{"ts":"2025-01-15T10:31:00Z","event":"remove","name":"old-skill","by":"user"}
```

### 10.5 Index Cache on Disk (ETag Conditional GET)

Unlike earlier specs (which only used in-memory cache), EdgeCrab persists index files
to `index-cache/` for offline resilience:

1. On first fetch: write `<source>.json` + store `ETag` response header in `<source>.json.etag`.
2. On subsequent fetches (within TTL=3600 s): return disk cache without network.
3. After TTL: send `If-None-Match: <etag>` conditional GET.
   - `304 Not Modified` → use disk cache, reset TTL.
   - `200 OK` → update disk cache.

Cache invalidation: `/plugins hub refresh` removes all `index-cache/*.json` files
and forces unconditional fetches.

---

## 11. Hub Index Caching (Summary)

TTL: **3600 seconds (1 hour)** — this is the Hermes-identical value.
Indices are cached both in memory (during session) and on disk (across sessions).
See §10.5 for the disk caching protocol.

---

## 12. Offline Mode

If the host has no network access:
- Cached index (if within TTL): used as-is
- No cache: search returns empty result with message
  `"Hub unreachable — showing installed plugins only"`
- Direct Github install URLs: fail fast with helpful error
  `"Cannot download plugin: network unavailable. Install from a local path instead."`

---

## 13. Future: Plugin Signing (Post-MVP)

In the current design, trust is based on source (hub curators vet plugins).
A future extension may add cryptographic signing:

```
  plugin author signs manifest with Ed25519 key
       ↓
  Curated hub verifies signature + adds its own countersignature
       ↓
  Client verifies both signatures on install
```

This is deferred until the hub directory is operational and adoption is sufficient
to justify the key management overhead. See [001_adr_architecture] §3 for the full
rationale for deferring cryptographic signing to Phase 2.
