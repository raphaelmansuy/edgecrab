# 007.002 — Creating Skills

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 007.001 Memory & Skills](./001_memory_skills.md) | [→ 003.003 Prompt Builder](../003_agent_core/003_prompt_builder.md) | [→ 009.001 Config & State](../009_config_state/001_config_state.md)
> **Source**: `edgecrab-tools/src/tools/skills.rs`, `edgecrab-tools/src/tools/skills_hub.rs`
> **Parity**: mirrors hermes-agent skill system

---

## 1. What Skills Are

Skills are Markdown files that inject domain knowledge into the agent's context. Unlike memory (project-specific notes) or the system prompt (personality), skills are **loadable modules** that the agent explicitly activates.

```
~/.edgecrab/skills/
├── git-workflow/
│   └── SKILL.md         ← active — flat layout
│
├── mlops/               ← category directory
│   └── training/        ← nested category
│       └── axolotl/     ← skill (nested up to N levels)
│           ├── SKILL.md
│           ├── references/
│           │   └── hparams.md
│           └── templates/
│               └── config.yaml
└── ...
```

Skills are discovered by leaf directory name. `skill_view name: "axolotl"` finds `mlops/training/axolotl/SKILL.md` automatically.

---

## 2. SKILL.md File Format

```markdown
---
name: Git Workflow
description: Advanced git branching, bisect, and worktree workflows
category: development
version: 1.0.0
license: MIT

# Platform restriction (omit to allow all platforms)
platforms:
  - darwin
  - linux

# Additional files to auto-load with the skill body
read_files:
  - references/commands.md
  - templates/pr-template.md

# Required credentials / env vars
required_environment_variables:
  - name: GITHUB_TOKEN
    prompt: "Enter your GitHub personal access token"
    help: "Create one at https://github.com/settings/tokens"
    required_for: "full functionality"

# Conditional activation (hide/show based on enabled toolsets)
conditional_activation:
  fallback_for_toolsets:
    - git_native         # Hide this skill when the native git toolset is active
  requires_tools:
    - terminal           # Only show when terminal tool is available
---

# Git Workflow

This skill provides advanced git patterns including worktrees, bisect, and
interactive rebase workflows optimized for large codebases.

## Branching Strategy
...
```

The YAML frontmatter is **optional** — a plain Markdown file works as a skill.

---

## 3. Frontmatter Reference

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | directory name | Display name |
| `description` | string | — | One-line description for `skills_list` |
| `category` | string | `"uncategorized"` | Groups skills in `skills_list` output |
| `version` | string | — | Semantic version |
| `license` | string | — | License identifier |
| `platforms` | list | all | Restrict to `darwin` / `linux` / `windows` |
| `read_files` | list | — | Relative paths to auto-load with the skill |
| `required_environment_variables` | list | — | Credential requirements checked on load |
| `conditional_activation` | object | — | Fallback / requires rules (see §4) |

### `required_environment_variables` items

| Sub-field | Required | Description |
|-----------|----------|-------------|
| `name` | yes | Environment variable name |
| `prompt` | no | Human-readable prompt for credential setup |
| `help` | no | URL or extra instructions |
| `required_for` | no | `"full functionality"` or `"operation"` |

When `required_for: "operation"`, a missing env var causes a load-time warning in the skill content and a prompt to the user to configure it.

---

## 4. Conditional Activation

Skills can hide themselves based on what toolsets and tools are enabled, so fallback skills only appear when their primary alternative is absent.

```yaml
conditional_activation:
  fallback_for_toolsets:
    - web_native    # Hide this skill when web_native toolset is active
  requires_toolsets:
    - web           # Only show when the web toolset is enabled
  fallback_for_tools:
    - browser       # Hide when browser tool is available
  requires_tools:
    - terminal      # Only show when terminal is available
```

| Rule | Behaviour |
|------|-----------|
| `fallback_for_toolsets` | **Hide** if any listed toolset is active |
| `requires_toolsets` | **Hide** if none of the listed toolsets are active |
| `fallback_for_tools` | **Hide** if any listed tool is available |
| `requires_tools` | **Hide** if none of the listed tools are available |

Conditions are OR within each group. All groups are AND with each other.

---

## 5. Progressive Disclosure

Skills use a three-tier loading model so large reference libraries don't flood the context on every load:

```
Tier 0: skills_categories               → category names + counts
Tier 1: skills_list [category: "..."]   → skill names + descriptions
Tier 2: skill_view name: "..."          → SKILL.md body + read_files
Tier 3: skill_view name: "..." file_path: "references/api.md"  → one linked file
```

Supporting files in these standard subdirectories are automatically listed at tier 2 but not loaded:

- `references/` — API docs, spec files, lookup tables
- `templates/` — Output templates the agent can fill in
- `scripts/` — Helper scripts the agent can run
- `assets/` — Static assets (images, data files)

The agent can load any tier-3 file on demand without re-loading the whole skill.

---

## 6. Skill Management Tools

### 6.1 `skill_manage` — create / edit / delete

```json
{
  "action": "create",
  "name": "docker-patterns",
  "content": "# Docker Patterns\n\nMulti-stage builds and compose patterns...",
  "category": "devops"
}
```

| Action | Required fields | Effect |
|--------|----------------|--------|
| `create` | `name`, `content` | Creates `skills/<name>/SKILL.md`; fails if directory already exists |
| `edit` | `name`, `content` | Overwrites existing `SKILL.md`; finds nested skills by leaf name |
| `delete` | `name` | Removes `skills/<name>/` directory |
| `list` | — | Lists all skill names (same as `skills_list`) |
| `view` | `name` | Returns skill content (same as `skill_view`) |

After any mutation (`create`, `edit`, `delete`), the skills prompt cache is automatically invalidated so the next `skills_list` call reflects the change.

### 6.2 `skills_list` — discover available skills

```json
{
  "category": "development"   // optional
}
```

Returns a grouped list by category, filtered by:
- Current platform (`darwin` / `linux` / `windows`)
- `disabled_skills` from config
- `conditional_activation` rules
- External skill directories (merged, local takes precedence on duplicates)

### 6.3 `skill_view` — load a skill

```json
{
  "name": "git-workflow",
  "file_path": "references/commands.md"   // optional — tier 3 access
}
```

Loads `SKILL.md` body + all `read_files` at tier 2. Loads a single linked file when `file_path` is given.

### 6.4 `skills_categories` — browse categories

Returns category names with skill counts. Useful before calling `skills_list` with a category filter.

---

## 7. Skill Discovery Algorithm

```text
discover_skills(roots: [local_dir, ...external_dirs])
  │
  └─ for each root:
       walk directory tree
         ├─ if leaf_dir/SKILL.md exists → it's a skill
         │    parse_skill_frontmatter()
         │    filter: platform, disabled_list, conditional_activation
         │    deduplicate: first root wins on name collision
         └─ else → recurse into subdirectory
```

Search priority: local skills directory (`~/.edgecrab/skills/`) beats external dirs. Within a root, a flat `skills/my-skill/SKILL.md` beats a nested `skills/category/my-skill/SKILL.md` if both exist (direct lookup is tried first).

External directories are configured in `config.yaml`:

```yaml
skills:
  external_dirs:
    - ~/work/company-skills
    - /opt/shared/edgecrab-skills
```

---

## 8. Preloaded Skills

Skills can be auto-loaded at session start (injected into the system prompt as cached blocks) without the agent needing to call `skill_view`:

**CLI flag**:
```bash
edgecrab -S git-workflow -S docker-patterns
```

**Config file** (`~/.edgecrab/config.yaml`):
```yaml
skills:
  preloaded:
    - git-workflow
    - docker-patterns
```

Preloaded skills are injected into the cached system prompt layer (slot 1) during `PromptBuilder::build()`. Because they sit in the cached portion of the prompt, they benefit from provider prompt caching (see [003.003 Prompt Builder](../003_agent_core/003_prompt_builder.md)).

---

## 9. Example: Creating a Skill from Scratch

```bash
# Option 1: via the agent
> skill_manage action=create name=docker-patterns \
    content="# Docker Patterns\n..."

# Option 2: manual file creation
mkdir -p ~/.edgecrab/skills/docker-patterns
cat > ~/.edgecrab/skills/docker-patterns/SKILL.md << 'EOF'
---
name: Docker Patterns
description: Multi-stage builds, compose orchestration, and debugging
category: devops
platforms:
  - darwin
  - linux
read_files:
  - references/compose-snippets.md
---

# Docker Patterns

## Multi-Stage Builds
...
EOF

mkdir -p ~/.edgecrab/skills/docker-patterns/references
echo "# Compose Snippets" > ~/.edgecrab/skills/docker-patterns/references/compose-snippets.md
```

Verify it's discoverable:

```
> skills_list category=devops
→ ### devops
  - **Docker Patterns**: Multi-stage builds, compose orchestration, and debugging
```

Load it:

```
> skill_view name=docker-patterns
→ ## Skill: docker-patterns
  ...
```

---

## 10. Disabled Skills

Prevent a skill from appearing or loading without deleting it:

```yaml
# ~/.edgecrab/config.yaml
skills:
  disabled:
    - old-skill-name
    - another-skill
```

Disabled skills are excluded from `skills_list` and will return a `NotFound` error from `skill_view`.

---

## 11. Security Considerations

| Risk | Mitigation |
|------|-----------|
| Path traversal via skill name | `..` in name → `PermissionDenied` |
| Path traversal via `file_path` | canonical path check verifies file stays within skill dir |
| Absolute paths in `read_files` | filtered out silently |
| Injection via frontmatter content | All skill content is read-only by the model — `skill_manage` mutations are gated by tool availability (see `edgecrab_security::check_injection()`) |

---

## 12. Testing

```bash
# Unit tests (skills tools)
cargo test -p edgecrab-tools skill

# Specific test
cargo test -p edgecrab-tools skill_view_strips_frontmatter
cargo test -p edgecrab-tools find_skill_dir_nested
```

All tests use `tempfile::TempDir` — no `~/.edgecrab` directory is touched during tests.
