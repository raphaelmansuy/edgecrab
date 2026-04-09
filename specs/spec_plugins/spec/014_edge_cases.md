# Edge Cases — Plugin & Skill System

**Status:** PROPOSED  
**Version:** 0.1.0  
**Date:** 2026-04-09  
**Cross-refs:** [003_manifest], [005_lifecycle], [006_security], [009_discovery_hub],
               [013_plugins_skills_relation], [015_hermes_compatibility]

---

## 1. SKILL.md Parsing Edge Cases

### EC-01 — Empty body after frontmatter

```yaml
---
name: my-skill
description: A skill.
---

```

**Expected:** REJECTED at install time.  
**Error:** `"SKILL.md must have content after the frontmatter."`  
**Source:** `skill_manager_tool.py _validate_frontmatter()`  
**EdgeCrab impl:** `body.trim().is_empty()` → `Err(ManifestError::EmptyBody)`

---

### EC-02 — Missing opening `---`

```
name: my-skill
description: A skill.
---
Some content here.
```

**Expected:** Treated as a skill with EMPTY frontmatter and full text as body.  
Frontmatter `{}`, body = entire content.  
**Source:** `skill_utils.py parse_frontmatter()` — returns `({}, content)` if no leading `---`.

---

### EC-03 — Malformed YAML in frontmatter (YAML parse fails)

```yaml
---
name: my: skill: with-colons
description: "unclosed string
---
Body content.
```

**Expected:** Fallback to simple key:value line splitting.  
Each line `key: value` is split on first `:`, values are bare strings.  
**Source:** `skill_utils.py parse_frontmatter()` `except Exception` block.

---

### EC-04 — Frontmatter with no closing `---`

```
---
name: my-skill
description: A skill.
Body content here.
```

**Expected:** Treated as skill with empty frontmatter, full content as body.  
`re.search(r"\n---\s*\n", content[3:])` fails → returns `({}, content)`.

---

### EC-05 — `name` with uppercase letters

```yaml
name: MySkill
```

**Expected:** REJECTED. `name` must match `^[a-z0-9][a-z0-9._-]*$`.  
**Error:** `"name 'MySkill' does not match pattern ^[a-z0-9][a-z0-9._-]*$"`  
**Note:** Env var names like `MY_VAR` use a different regex (`^[A-Za-z_][A-Za-z0-9_]*$`)
and uppercase IS allowed there.

---

### EC-06 — `name` is 65 characters (exceeds MAX_NAME_LENGTH)

**Expected:** REJECTED. `MAX_NAME_LENGTH = 64`.  
**EdgeCrab impl:** `if name.len() > 64 { return Err(...) }`

---

### EC-07 — `description` over 1024 characters

**Expected:** REJECTED. `MAX_DESCRIPTION_LENGTH = 1024`.

---

### EC-08 — `read_files` with path traversal

```yaml
read_files:
  - ../../../etc/passwd
  - /absolute/path.md
```

**Expected:** REJECTED at load time. Paths must be relative and within skill dir.  
No `..` components, no absolute paths.  
**Source:** `skill_manager_tool.py _resolve_skill_dir()` / path validation.

---

### EC-09 — Skill content exceeds 100,000 characters

Body + all `read_files` combined exceeds `MAX_SKILL_CONTENT_CHARS = 100_000`.  
**Expected:** Installation allowed, but content is truncated or a warning issued.  
(Implementation choice; Hermes warns and truncates.)

---

### EC-10 — `category` value containing slash or spaces

```yaml
category: "my/category with spaces"
```

**Expected:** REJECTED. `category` uses same regex as `name`: `^[a-z0-9][a-z0-9._-]*$`.

---

## 2. Platform Matching Edge Cases

### EC-11 — `platforms` is a string, not a list

```yaml
platforms: macos
```

**Expected:** Treated as single-element list: `["macos"]`.  
**Source:** `skill_utils.py skill_matches_platform()`: `if not isinstance(platforms, list): platforms = [platforms]`

---

### EC-12 — Unknown platform string

```yaml
platforms: [haiku]
```

**Expected:** Passed through as-is to `sys.platform.startswith("haiku")`.  
If current platform doesn't start with "haiku", skill is excluded.  
No error; unknown strings are silently ignored.

---

### EC-13 — `platforms: []` (empty list)

**Expected:** Treated as "all platforms" (backward-compat default). Skill IS loaded.  
**Source:** `skill_utils.py skill_matches_platform()`: `if not platforms: return True`

---

## 3. Credential-Collection Edge Cases

### EC-14 — `env_var` not `name` key in `required_environment_variables`

```yaml
required_environment_variables:
  - env_var: MY_API_KEY     # uses alias
    description: "My API key"
```

**Expected:** ACCEPTED. Both `name` and `env_var` are valid.  
**Source:** `skills_tool.py`: `env_name = str(entry.get("name") or entry.get("env_var") or "").strip()`

---

### EC-15 — `url` not `provider_url` in `collect_secrets`

```yaml
setup:
  collect_secrets:
    - env_var: MY_API_KEY
      url: "https://example.com/api"    # uses alias
```

**Expected:** ACCEPTED. Both `provider_url` and `url` are valid.  
**Source:** `skills_tool.py`: `provider_url = str(item.get("provider_url") or item.get("url") or "").strip()`

---

### EC-16 — `required_environment_variables` entry with no `help`, no `provider_url`, no `url`

```yaml
setup:
  help: "See https://example.com for setup instructions."
  required_environment_variables:
    - name: MY_TOKEN    # no help field on this entry
```

**Expected:** `setup.help` is used as fallback help text.  
**Source:** `skills_tool.py`: `help_text = entry.get("help") or ... or setup.get("help")`

---

### EC-17 — Skills requiring setup in remote backend environment

Skill has `required_environment_variables` but agent is running in a Docker/Modal/SSH backend.

**Expected:** Status = `UNSUPPORTED` (not `SETUP_NEEDED`).  
Skill is NOT injected. User sees: "This skill requires interactive setup unavailable in
this environment. Set `MY_TOKEN` manually before starting."  
**Source:** `skills_tool.py _REMOTE_ENV_BACKENDS = frozenset({"docker", "singularity", "modal", "ssh", "daytona"})`

---

### EC-18 — Env var name with invalid characters

```yaml
required_environment_variables:
  - name: "my-api-key"    # hyphens NOT allowed in env var names
```

**Expected:** REJECTED at parse. `_ENV_VAR_NAME_RE = ^[A-Za-z_][A-Za-z0-9_]*$`  
Hyphens are not allowed in env var names (they are not valid POSIX variable names).

---

## 4. Security Scanner Edge Cases

### EC-19 — Pattern match spans across lines

The scanner applies regex per-line. A pattern split across two lines is NOT detected.  
**Implication:** Obfuscation via line breaking can evade single-line regex scanning.  
**Mitigation (Phase 2):** Multi-line scan mode for high-risk file types.

---

### EC-20 — Agent-created skill with `dangerous` verdict

```python
should_allow_install(result)  # returns (None, "some reason")
```

**Expected:** `None` = "ask" → install proceeds, findings surface as warning.  
This is intentional: the agent wrote the skill and has context.  
**Contrast:** `community` + `dangerous` → `(False, reason)` → BLOCKED (no override).  
**Source:** `skills_guard.py INSTALL_POLICY["agent-created"] = ("allow", "allow", "ask")`

---

### EC-21 — `skills_guard` module unavailable (optional import)

```python
_GUARD_AVAILABLE = False  # guard failed to import
```

**Expected:** Graceful degradation. Installation proceeds with a warning:
"Security scanning unavailable — guard module not installed. All plugins treated as unscanned."  
Trust policy still applies; only pattern scanning is skipped.  
**Source:** `skill_manager_tool.py _GUARD_AVAILABLE` flag.

---

### EC-22 — Skill with a file in a non-allowed subdirectory

```
my-skill/
  SKILL.md
  src/                   ← NOT in ALLOWED_SUBDIRS
    tool.py
```

**Expected:** REJECTED at install. Only `references/`, `templates/`, `scripts/`, `assets/`
are allowed subdirectories.  
**Source:** `skill_manager_tool.py ALLOWED_SUBDIRS = {"references", "templates", "scripts", "assets"}`

---

### EC-23 — Skill directory with more than 50 files

**Expected:** Structural limit triggered: `MAX_FILE_COUNT = 50`.  
Scanner flags as suspicious (`"skills shouldn't have 50+ files"`). Verdict: at least `caution`.

---

### EC-24 — Binary file in skill directory

```
my-skill/
  SKILL.md
  helper.exe             ← suspicious binary extension
```

**Expected:** Flagged as high-severity. `SUSPICIOUS_BINARY_EXTENSIONS = {".exe", ".dll", ".so", ...}`  
Verdict: at minimum `caution`; likely `dangerous` for `.exe`/`.dll`.

---

### EC-25 — Zero-width Unicode in SKILL.md body

Content contains invisible characters: `\u200b`, `\u200c`, `\u202e` (RTL override), etc.  
**Expected:** Flagged as injection attempt. Verdict: `caution` or `dangerous` depending on
character set.  
**Source:** `skills_guard.py INVISIBLE_CHARS` set with 17 invisible Unicode code points.

---

## 5. Hub Discovery Edge Cases

### EC-26 — GitHub API returns truncated tree

When `_download_directory_via_tree()` gets `truncated: true` in the response:  
**Expected:** Falls back to recursive Contents API.  
**Source:** `skills_hub.py GitHubSource._download_directory()`:
```python
if tree_data.get("truncated"):
    logger.debug("Git tree truncated, falling back to Contents API")
    return None  # triggers fallback
```

---

### EC-27 — Skill appears in multiple taps (deduplication)

Same skill name found in `openai/skills` (trusted) and `VoltAgent/awesome-agent-skills` (community).  
**Expected:** Higher trust-level entry wins.  
**Source:** `skills_hub.py GitHubSource.search()`:
```python
_trust_rank = {"builtin": 2, "trusted": 1, "community": 0}
# prefer higher rank on duplicate name
```

---

### EC-28 — Hub search with empty results from all taps

**Expected:** Returns empty list (not an error). CLI shows "No skills found for query."

---

### EC-29 — `taps.json` is missing or corrupted

**Expected:** Treated as empty tap list. Default taps still apply (they are hardcoded,
not loaded from `taps.json`). Warning logged for corrupted file.

---

### EC-30 — Index cache expired (TTL = 3600 seconds)

**Expected:** On next hub operation, cache is invalidated and re-fetched from GitHub.  
Cache key format: `{repo}_{path}` with `/` replaced by `_`.

---

### EC-31 — GitHub App token expires during a batch operation

Token cached for 3500 seconds. If a large batch operation spans >58 minutes:  
**Expected:** The cached token expires. Next API call triggers re-generation.  
`self._app_token_expiry = time.time() + 3500` — must check `time.time() > expiry`.

---

## 6. Platform-Specific Edge Cases

### EC-32 — `/config skills platform_disabled` overrides global `disabled`

```yaml
skills:
  disabled: [heavy-skill]
  platform_disabled:
    telegram: []          # empty list for telegram → nothing extra disabled
```

**Expected:** `heavy-skill` is disabled globally but NOT disabled for Telegram (because
`platform_disabled.telegram` is present and is an empty list → it overrides the global).  
**Source:** `skill_utils.py get_disabled_skill_names()` returns `platform_disabled[platform]`
when that key is present, even if empty.

---

### EC-33 — `HERMES_PLATFORM` vs `HERMES_SESSION_PLATFORM`

Both env vars set to different values:  
**Expected:** `HERMES_PLATFORM` takes precedence (checked first).  
**Source:** `skill_utils.py`: `resolved_platform = (platform or os.getenv("HERMES_PLATFORM") or os.getenv("HERMES_SESSION_PLATFORM"))`

---

## 7. External Directories Edge Cases

### EC-34 — External dir path with `~` and env vars

```yaml
plugins:
  external_dirs:
    - ~/my-skills
    - ${TEAM_SKILL_PATH}
```

**Expected:** Both expanded (`expanduser` + `expandvars`) then resolved to absolute paths.  
`${TEAM_SKILL_PATH}` with unset env var → expansion leaves `$TEAM_SKILL_PATH` literally → not a real dir → skipped silently.

---

### EC-35 — External dir resolves to same path as main plugins dir

**Expected:** Silently de-duplicated. Main `~/.edgecrab/plugins/` is never in the external dirs list.

---

### EC-36 — External dir does not exist at startup

**Expected:** Silently skipped. Only dirs that `is_dir()` at startup are included.  
**Implication:** Dirs that appear later (e.g., mounted network share) require agent restart.

---

## 8. Sync / Bundled Skills Edge Cases

### EC-37 — User has customized a bundled skill

Origin hash in `.bundled_manifest` differs from current user copy hash.  
**Expected:** Bundled update is SKIPPED. User's customization is preserved.  
**Source:** `skills_sync.py` update logic.

---

### EC-38 — User deleted a bundled skill

Skill in manifest, absent from user `~/.edgecrab/plugins/`.  
**Expected:** Deletion is RESPECTED. Skill is NOT re-synced.  
**Source:** `skills_sync.py`: `DELETED by user (in manifest, absent from user dir): respected, not re-added.`

---

### EC-39 — Bundled skill removed from repo

Skill in manifest but no longer present in the repo's `skills/` directory.  
**Expected:** Removed from manifest. User's copy untouched (may remain on disk).  
**Source:** `skills_sync.py`: `REMOVED from bundled: cleaned from manifest.`

---

### EC-40 — `EDGECRAB_BUNDLED_PLUGINS` env var set

**Expected:** Uses the specified directory as the bundled skills source instead of the
repo-relative `skills/` directory.  
**Source:** Mirrors Hermes `HERMES_BUNDLED_SKILLS` env var.
