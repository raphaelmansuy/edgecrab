# Memory and Skills 🦀

> **Verified against:** `crates/edgecrab-core/src/prompt_builder.rs` ·
> `crates/edgecrab-tools/src/tools/memory.rs` ·
> `crates/edgecrab-tools/src/tools/skills.rs` ·
> `crates/edgecrab-tools/src/tools/honcho.rs`

---

## Why persistent memory matters

Without memory, every session starts fresh. EdgeCrab used the same model but knew
nothing about your project structure, code style preferences, or past decisions.
Memory gives the agent durable context that survives across sessions.

Without skills, every time you want the agent to "follow our release checklist",
you re-explain it. Skills encode reusable workflows in Markdown that the agent
reads and executes.

🦀 *`hermes-agent` (EdgeCrab's predecessor) reset memory on every session
unless you hand-edited its MEMORY.md before launch. OpenClaw ([TypeScript/Node.js](https://github.com/openclaw))
persists session transcripts but has no automatic cross-session memory injection
into the system prompt. EdgeCrab remembers — even when the crab goes to sleep.*

---

## Three memory systems

```
  ┌────────────────────────────────────────────────────────────────┐
  │  1. File-backed memory   (~/.edgecrab/memories/)               │
  │     ■ Plain Markdown files                                     │
  │     ■ Loaded into system prompt at session start               │
  │     ■ Read/write via memory_read / memory_write tools          │
  │     ■ Survives across all sessions                             │
  │     ■ Injection-checked before loading                         │
  └────────────────────────────────────────────────────────────────┘
  ┌────────────────────────────────────────────────────────────────┐
  │  2. Skills  (~/.edgecrab/skills/)                               │
  │     ■ Directories with SKILL.md files                           │
  │     ■ Listed in system prompt (summaries only)                  │
  │     ■ Invoked by name: "use the git-release skill"             │
  │     ■ Managed via skills_list / skill_manage / skills_hub      │
  └────────────────────────────────────────────────────────────────┘
  ┌────────────────────────────────────────────────────────────────┐
  │  3. Honcho  (external service)                                  │
  │     ■ User-level memory managed by Honcho API                  │
  │     ■ Semantic search over past sessions                       │
  │     ■ Requires HONCHO_APP_ID env var                           │
  │     ■ Tools: honcho_conclude, honcho_search, honcho_profile    │
  └────────────────────────────────────────────────────────────────┘
```

**Reference:** [Honcho docs](https://honcho.dev/docs)

---

## File-backed memory

### Layout

```
  ~/.edgecrab/memories/             (default; varies with profile)
    MEMORY.md                       ← primary memory file (always loaded)
    USER.md                         ← user profile facts
    <any-other>.md                  ← custom memory sections
```

All `.md` files in the memories directory are loaded. They are injected into the
system prompt in alphabetical order, each in its own section:

```
  [memory:MEMORY.md]
  (content of MEMORY.md)

  [memory:USER.md]
  (content of USER.md)
```

### Security gate

Every memory file passes through `check_memory_content()` before loading:

```
  check_memory_content(content)
    │
    ├── check_injection(content)
    │     ↳ blocks: "ignore previous", "you are now", etc.
    │
    ├── invisible unicode check
    │     ↳ blocks: zero-width spaces, directional overrides
    │
    └── exfiltration patterns
          ↳ blocks: curl with $SECRET, cat ~/.ssh/id_rsa, etc.
```

Files that fail the check are **skipped** with a warning — not loaded.

### Writing memory from the agent

```
  memory_write tool:
    path: "memories/my-project.md"
    content: "## Project facts\n- Uses SQLite for persistence\n..."

  → writes to ~/.edgecrab/memories/my-project.md
  → content passes security check before persisting
  → next session picks it up automatically
```

---

## Skills

### Skills vs plugins

From first principles:

- A `skill` is prompt-level procedural knowledge.
- A `plugin` is a runtime package EdgeCrab installs and manages.

Use a skill when you need reusable instructions or a skill-local bundle of
helper files and scripts that the agent uses through normal tools. Use a plugin
when you need executable extension behavior such as tools, hooks, subprocesses,
Python Hermes compatibility, readiness gating, or audited install/update lifecycle.

Important overlap:

- A plugin can bundle a `SKILL.md`.
- That does not make standalone skills and plugins the same thing.
- Standalone skills live under `~/.edgecrab/skills/` and are managed with
  `edgecrab skills ...`.
- Plugins live under `~/.edgecrab/plugins/` and are managed with
  `edgecrab plugins ...`.
- Standalone skills can also bundle helper files under directories such as
  `scripts/`, `references/`, `templates/`, and `assets/`.

Quick rule:

- If the artifact only needs to tell the agent what to do, make it a skill.
- If the artifact needs to make EdgeCrab do something new at runtime, make it a plugin.

### Layout

```
  ~/.edgecrab/skills/
    git-release/
      SKILL.md            ← required
      release-steps.md    ← referenced via read_files frontmatter
    python-test/
      SKILL.md
    my-custom-workflow/
      SKILL.md
```

External skill directories can be added in config:

```yaml
# ~/.edgecrab/config.yaml
skills:
  external_dirs:
    - /Users/me/shared-skills/
    - /work/team-skills/
```

### What loads at session start

The `PromptBuilder` includes a **summary** (not full content) of all installed
skills:

```
  Available skills:
  - git-release: Automated git tag, changelog, and crates.io publish workflow
  - python-test: Run pytest with coverage, lint, and type checking
  - my-custom-workflow: Deploy to staging and run smoke tests
```

Full skill content is loaded on demand when the model invokes the skill
(via `skill_view`) or when `preloaded_skills` config specifies it.

Standalone skills do not create a new tool, hook, process, or plugin runtime.
They can still carry helper files, and EdgeCrab now resolves Claude-style
`${CLAUDE_SKILL_DIR}` and `${CLAUDE_SESSION_ID}` placeholders when a skill is
loaded, but execution still happens through the normal tool surfaces.

### Viewing a skill

```sh
# List all skills
edgecrab skills list

# View a specific skill
edgecrab skills view git-release

# Search by keyword
edgecrab skills search "deploy"

# Install from hub
edgecrab skills install docker-build
```

---

## Skill runtime activation

When a skill is invoked, the agent:
1. Reads `SKILL.md` (full content)
2. Loads any files listed in `read_files` frontmatter
3. Follows the skill's instructions as part of its normal task loop

Conditional activation (from frontmatter):

```yaml
# SKILL.md frontmatter
requires_tools: [terminal, write_file]
requires_toolsets: [coding]
platforms: [linux, windows]
```

If the required tools are not in the active toolset, the skill is hidden from
the summary — it won't appear as a suggestion to the model.

Claude-style standalone skill bundles are partially compatible:

- EdgeCrab renders `${CLAUDE_SKILL_DIR}` and `${CLAUDE_SESSION_ID}`.
- EdgeCrab loads `read_files` and lists helper files from `references/`,
  `templates/`, `scripts/`, and `assets/`.
- EdgeCrab parses metadata such as `when_to_use`, `arguments`,
  `argument-hint`, `allowed-tools`, `user-invocable`,
  `disable-model-invocation`, `context`, and `shell`.
- EdgeCrab does not automatically execute Claude prompt-shell blocks or
  fork a dedicated sub-agent because those are Claude runtime semantics, not
  portable skill-bundle semantics.

---

## Honcho integration

Honcho is a separate cloud service providing user-level memory and personalisation:

```
  Session ends:
    honcho_conclude → sends session summary to Honcho API
                    → Honcho indexes it for future semantic search

  New session:
    honcho_context  → fetches relevant past experience from Honcho
                    → injects into the agent's current context

  Explicit search:
    honcho_search("how did I solve the auth problem?")
    → semantic search across all indexed past sessions
```

Honcho is completely optional. File-backed memory works without it.

---

## Tips

> **Tip: Keep `MEMORY.md` concise.** It loads on every session and contributes tokens.
> One-liners per fact are better than paragraphs:
> ```markdown
> - Project: edgecrab, Rust 2024, MSRV 1.85.0
> - Test command: cargo test --workspace
> - Deploy: cargo publish crates in dependency order
> ```

> **Tip: Use skills for multi-step workflows with failure modes.**
> A skill that documents both the happy path AND common failure cases is
> 10× more useful than one that only shows the success path.

> **Tip: `--skill git-release` pre-loads a skill for a session without `/slash` invocation.**
> ```sh
> edgecrab --skill git-release "prepare the next minor release"
> ```

---

## FAQ

**Q: Do memory files affect all profiles?**
No. Each profile has its own `memories/` directory. `~/.edgecrab/memories/` is
the default profile's memory. `~/.edgecrab/profiles/work/memories/` is the
`work` profile's memory.

**Q: What happens if a memory file is corrupted or injection-checked?**
It is skipped with a `tracing::warn!` log entry. The session continues normally
without that memory section.

**Q: Can the agent delete memory?**
Yes, via `memory_write` with empty content or `skill_manage` with `delete` action.
There is no auto-expiry.

---

## Cross-references

- How memory loads into the prompt → [Prompt Builder](../003_agent_core/003_prompt_builder.md)
- Security checks on memory content → [Security](../011_security/001_security.md)
- Skill file format → [Creating Skills](./002_creating_skills.md)
- Skills tools catalogue → [Tool Catalogue](../004_tools_system/002_tool_catalogue.md)
