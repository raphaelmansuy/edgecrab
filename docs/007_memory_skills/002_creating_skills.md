# Creating Skills 🦀

> **Verified against:** `crates/edgecrab-tools/src/tools/skills.rs`

---

## Why write a skill

A skill is the cheapest form of agent customisation. Instead of modifying source
code, you write a Markdown file that the agent reads at runtime. The agent then
follows the skill's instructions as part of its normal response loop.

A well-written skill encodes the difference between a generic agent and one that
knows exactly how *your* project works.

🦀 *Skills are EdgeCrab's muscle memory. The crab learns the right moves and
executes them without you having to teach it every time.*

---

## Minimal structure

```
  ~/.edgecrab/skills/my-skill/
    SKILL.md
```

One directory, one file. That is the complete requirement. The directory name
becomes the default skill name if no `name:` frontmatter is present.

You can also bundle helper files alongside `SKILL.md`, for example:

```text
~/.edgecrab/skills/my-skill/
  SKILL.md
  scripts/
    helper.py
  references/
    api.md
  templates/
    output.md
```

---

## SKILL.md format

```markdown
---
name: my-skill               # Display name (optional; defaults to dir name)
description: One-line summary for the skills list prompt injection
category: devops             # Groups skills in skills_categories output
version: 1.0.0
license: MIT
platforms:                   # Omit to show on all supported operating systems
  - linux
  - windows
read_files:                  # Additional files loaded when skill is invoked
  - references/release.yml
requires_tools:              # Skill hidden if these tools are absent
  - terminal
  - write_file
requires_toolsets:           # Skill hidden if these toolsets aren't active
  - coding
required_environment_variables:
  - name: GITHUB_TOKEN
    prompt: GitHub token
    help: https://github.com/settings/tokens
when_to_use: Use when preparing a release or validating release state.
arguments:
  - version
  - channel
argument-hint: <version> <channel>
allowed-tools:
  - read_file
  - run_terminal
user-invocable: true
disable-model-invocation: false
context: fork
shell: bash
---

# My Skill

## When to use this skill
(tell the model exactly when this skill is appropriate)

## Prerequisites
(what must be true before starting)

## Workflow
1. Step one
2. Step two
   - important note
3. Step three

## Common failures
- **If X happens**: do Y instead
- **Error "Z not found"**: check that W is configured

## Example
(a concrete example of the workflow in action)
```

---

## Frontmatter fields reference

| Field | Type | Effect |
|---|---|---|
| `name` | string | Display name in lists; defaults to directory name |
| `description` | string | Injected into system prompt summary |
| `category` | string | Groups in `skills_categories` output |
| `version` | string | Displayed in skill view; no version enforcement |
| `license` | string | Metadata only |
| `platforms` | list | If set, skill hidden on other operating systems (`darwin`, `linux`, `windows`) |
| `read_files` | list | Relative paths loaded alongside SKILL.md on invocation |
| `requires_tools` | list | Skill hidden if all listed tools are not available |
| `requires_toolsets` | list | Skill hidden if all listed toolsets are not active |
| `required_environment_variables` | list of objects | Env passthrough + guidance for missing credentials |
| `when_to_use` | string | Claude-style fallback summary when `description` is absent |
| `arguments` | list | Claude-style declared argument names; displayed in `skill_view` |
| `argument-hint` | string | Claude-style invocation hint; displayed in `skill_view` |
| `allowed-tools` | list | Claude-style advisory metadata; displayed in `skill_view` |
| `user-invocable` | bool | Hidden from `skills_list` when `false` |
| `disable-model-invocation` | bool | Parsed and displayed; not enforced by EdgeCrab |
| `context` | string | Parsed and displayed; `fork` is not auto-executed |
| `shell` | string | Parsed and displayed; prompt-shell blocks are not auto-executed |

Frontmatter is **optional**. A `SKILL.md` with no frontmatter and just body
text is a valid skill.

---

## Writing effective skill content

### Do: state the trigger condition explicitly

```markdown
## When to use
Use this skill when asked to create a new release, bump the version,
publish to crates.io, or update the CHANGELOG.
```

Without this, the model may not activate the skill even when it's appropriate.

### Do: show the exact commands

```markdown
## Steps
1. `cargo test --workspace` — verify all tests pass
2. `git tag v$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')`
3. `git push --tags`
4. Run `cargo publish` for each crate in dependency order
```

### Do: document failure paths

```markdown
## Failures
- If `cargo publish` returns "crate already exists": increment patch version
- If tests fail on integration tests: check `docker ps` — the test DB may be down
```

### Don't: write aspirational steps

If a step requires a tool the model doesn't have or a service that isn't
configured, note the prerequisite. Aspirational steps confuse the agent.

---

## Invoking a skill

```sh
# From CLI flag (pre-loads before the session starts)
edgecrab --skill git-release "prepare the 1.2.0 release"

# From within a session (slash command)
/skills list
/skills view git-release

# The model can invoke a skill itself
# (after reading the summary in the system prompt):
"Use the git-release skill to create the patch release"
```

---

## `read_files` — linked documents

Skills can reference additional files loaded alongside the `SKILL.md`:

```yaml
read_files:
  - ../shared/release-checklist.md   # relative to SKILL.md
  - /absolute/path/to/runbook.md
```

These files are loaded when the skill is invoked (via `skill_view`) and their
content is included in the skill body. Use this to keep the `SKILL.md` brief
while referencing detailed runbooks.

For Claude-style helper scripts, EdgeCrab also supports:

- `${CLAUDE_SKILL_DIR}` → substituted to the concrete skill directory
- `${CLAUDE_SESSION_ID}` → substituted to the active session id

That means a skill can safely refer to bundled CLI helpers, for example:

```markdown
Run `${CLAUDE_SKILL_DIR}/scripts/helper.py --session ${CLAUDE_SESSION_ID}` with the terminal tool.
```

Claude compatibility boundary:

- Supported: skill-directory layout, `SKILL.md`, `read_files`, helper-file
  discovery, `when_to_use` fallback, `${CLAUDE_SKILL_DIR}`, and
  `${CLAUDE_SESSION_ID}`.
- Not automatically executed: Claude inline prompt-shell expansion and
  forked-skill runtime semantics.

---

## Installing from the skills hub

```sh
# Browse available skills
edgecrab skills hub

# Install a skill by name
edgecrab skills install docker-build

# Installed to ~/.edgecrab/skills/docker-build/
```

The hub is a curated collection of community-contributed skills.
Local skills always take precedence over hub skills of the same name.

---

## Managing skills

```sh
# List all installed skills
edgecrab skills list

# View full content of a skill
edgecrab skills view git-release

# Search by keyword
edgecrab skills search "deploy"

# Remove a skill
edgecrab skills remove old-skill

# Install from a local directory path
edgecrab skills install /path/to/my-skill
```

---

## Example: complete skill

```markdown
---
name: rust-release
description: Publish Rust workspace crates to crates.io in dependency order
category: release
version: 1.0.0
requires_tools: [terminal]
required_environment_variables:
  - name: CARGO_REGISTRY_TOKEN
    prompt: crates.io token
---

# Rust Release

## When to use
When asked to publish, release, or bump the version of any crate in
the edgecrab workspace.

## Prerequisites
- All tests pass: `cargo test --workspace`
- `CARGO_REGISTRY_TOKEN` environment variable is set
- Working directory is the workspace root

## Publish order
Respect the dependency graph. Publish leaf crates first:

1. edgecrab-types
2. edgecrab-security
3. edgecrab-state
4. edgecrab-cron
5. edgecrab-tools
6. edgecrab-core
7. edgecrab-gateway
8. edgecrab-acp
9. edgecrab-migrate
10. edgecrab-cli

Wait 30 seconds between each publish for crates.io to index.

## Commands
```sh
cargo publish -p edgecrab-types
sleep 30
cargo publish -p edgecrab-security
# ... continue
```

## Failures
- `crate already uploaded` → version already exists; bump the version in Cargo.toml
- `401 Unauthorized` → check CARGO_REGISTRY_TOKEN is valid and not expired
```

---

## Tips

> **Tip: One skill per workflow, not one skill per project.**
> A `git-release` skill is reusable across projects. A `my-specific-project`
> skill that embeds project-specific details is harder to maintain and share.

> **Tip: Test the skill interactively before saving.**
> Run the workflow manually in a session, note every edge case, then write the
> skill based on what actually happened — not what you hoped would happen.

> **Tip: Use `requires_tools` to prevent the model from reading the skill
> when the right tools aren't available.** A skill that requires `terminal`
> but is shown in a `--toolset safe` session wastes prompt tokens and
> confuses the model.

---

## FAQ

**Q: Can a skill call another skill?**
Not directly — there is no skill-calling syntax. But the model can read a skill
(via `skill_view`) and follow its instructions, which may include "follow the
X workflow" referring to another skill. The model will then request that skill.

**Q: How should I version my skills?**
`version` in frontmatter is purely informational. There is no enforcement.
Use it for your own tracking; the runtime ignores it for activation purposes.

**Q: Can skills be shared across team members?**
Yes. Add the shared directory to `skills.external_dirs` in each team member's
`~/.edgecrab/config.yaml`. Or publish to the skills hub.

---

## Cross-references

- Memory system overview → [Memory and Skills](./001_memory_skills.md)
- Skills in the system prompt → [Prompt Builder](../003_agent_core/003_prompt_builder.md)
- Skills tools (`skill_manage`, `skills_hub`) → [Tool Catalogue](../004_tools_system/002_tool_catalogue.md)
