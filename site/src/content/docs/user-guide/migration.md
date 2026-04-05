---
title: Migrating from Hermes Agent
description: Import your entire Hermes Agent configuration, memories, skills, and session data into EdgeCrab with a single command. Zero manual work.
sidebar:
  order: 6
---

If you're moving from [Hermes Agent](https://github.com/NousResearch/hermes-agent), EdgeCrab includes a first-class migration tool that imports everything in one step.

---

## What Gets Migrated

| Asset | Source | Destination |
|-------|--------|-------------|
| Configuration | `~/.hermes/config.yaml` | `~/.edgecrab/config.yaml` |
| Memories | `~/.hermes/memories/` | `~/.edgecrab/memories/` |
| Skills | `~/.hermes/skills/` | `~/.edgecrab/skills/` |
| Environment file | `~/.hermes/.env` | `~/.edgecrab/.env` |

Session history stored in Hermes's PostgreSQL or SQLite backend is **not** migrated (format is incompatible). Only the structured state above is imported.

---

## Step 1 — Dry Run

Always run the dry-run first to see exactly what will happen:

```bash
edgecrab migrate --dry-run
```

```
EdgeCrab Migration (dry-run) — no files will be written
────────────────────────────────────────────────────────────────
Source: ~/.hermes/

  Config:   ~/.hermes/config.yaml  →  ~/.edgecrab/config.yaml
            (provider: openai, model: gpt-4o)
  Memories: 7 files                →  ~/.edgecrab/memories/
  Skills:   12 files               →  ~/.edgecrab/skills/
  Env:      ~/.hermes/.env         →  ~/.edgecrab/.env
              (4 keys detected, will be merged)

No conflicts detected.

Run `edgecrab migrate` (without --dry-run) to execute.
────────────────────────────────────────────────────────────────
```

---

## Step 2 — Execute Migration

```bash
edgecrab migrate
```

```
EdgeCrab Migration
────────────────────────────────────────────────────────────────
✓  Config written         ~/.edgecrab/config.yaml
✓  Memories copied        7/7 files
✓  Skills copied          12/12 files
✓  Env merged             ~/.edgecrab/.env
────────────────────────────────────────────────────────────────
Migration complete. Run `edgecrab doctor` to verify.
```

---

## Step 3 — Verify

```bash
edgecrab doctor
```

If everything is configured correctly, all checks should pass.

---

## Handling Conflicts

If `~/.edgecrab/` already exists (e.g., you ran `edgecrab setup` first), the migration will skip files that already exist:

```
  Skills:   12 files               →  ~/.edgecrab/skills/
              ⚠ 3 files already exist — skipping (use --overwrite to replace)
```

To force overwrite:

```bash
edgecrab migrate --overwrite
```

Or merge manually:

```bash
edgecrab migrate --dry-run > /tmp/migration-plan.txt
# Review and selectively copy files
```

---

## Configuration Mapping

EdgeCrab uses the same provider names as Hermes. Your existing `config.yaml` provider settings will work directly. A few key differences:

| Hermes config key | EdgeCrab config key |
|-------------------|---------------------|
| `llm.provider` | `provider` |
| `llm.model` | `model` |
| `tools.allowed_paths` | `tools.file.allowed_roots` |
| `memory.path` | `memory.dir` |
| `skills.path` | `skills.dir` |

---

## Skills Compatibility

EdgeCrab skills use the same Markdown format as Hermes Agent skills. All skills migrated from Hermes will work as-is.

The only difference: EdgeCrab's skills system adds a `capabilities` frontmatter field for faster lookup. EdgeCrab will automatically upgrade skills to the new format the first time they are used.

---

## Keeping Both Agents

You can run Hermes Agent and EdgeCrab side-by-side without conflict — they use different state directories (`~/.hermes` vs `~/.edgecrab`). If you update memories in one, run the migration again to sync to the other.

---

## Reverting

If you want to undo the migration, simply delete `~/.edgecrab/` (back it up first):

```bash
cp -r ~/.edgecrab ~/.edgecrab.pre-migration
rm -rf ~/.edgecrab
```

Your Hermes Agent installation is never modified by `edgecrab migrate`.

---

## Pro Tips

**Always run `--dry-run` first.** It shows exactly what will be copied and flags any conflicts without making changes. Review it before committing.

**Migrate incrementally.** If you make improvements to your Hermes memories or skills after migration, re-run `edgecrab migrate` — it skips files that already exist (unless you use `--overwrite`).

**After migration, use EdgeCrab for a week before removing Hermes.** This ensures you're comfortable before losing the fallback. Both can coexist indefinitely.

---

## Frequently Asked Questions

**Q: My Hermes session history isn't migrated. Is that expected?**

Yes — session history uses different SQLite schemas and cannot be auto-migrated. Only config, memories, skills, and `.env` are imported. If critical conversations exist, export them from Hermes first: `hermes sessions export <id> > important-session.md`.

**Q: Some of my Hermes config keys aren't recognized. What do I do?**

EdgeCrab has a superset of Hermes config but some keys changed names (see the mapping table above). Run `edgecrab doctor` after migration — it highlights unrecognized config keys. Then update them using `edgecrab config set <new-key> <value>`.

**Q: I use Hermes profiles. Are those migrated?**

Profiles from `~/.hermes/profiles/` are migrated to `~/.edgecrab/profiles/`. Each profile's config, memories, and skills are migrated independently.

**Q: Will `edgecrab migrate` break my Hermes installation?**

Never. `edgecrab migrate` is read-only on the Hermes side — it only reads from `~/.hermes/` and writes to `~/.edgecrab/`. Your Hermes setup is completely untouched.

**Q: I want to keep Hermes as a fallback. Is that safe?**

Yes. Both agents coexist. Just be aware that changes to `~/.edgecrab/memories/` are not automatically synced back to `~/.hermes/memories/`. Run `edgecrab migrate --overwrite` periodically to freshen your EdgeCrab data from Hermes, or manage them independently.

---

## See Also

- [Installation](/getting-started/installation/) — Install EdgeCrab first
- [Quick Start](/getting-started/quick-start/) — Get running after migration
- [Configuration](/user-guide/configuration/) — Review migrated config
- [CLI Commands](/reference/cli-commands/) — `edgecrab migrate` flags
