---
title: Checkpoints & Rollback
description: Automatic filesystem snapshots via shadow git — how EdgeCrab checkpoints before destructive operations and how to roll back. Grounded in crates/edgecrab-tools/src/tools/checkpoint.rs.
sidebar:
  order: 8
---

EdgeCrab automatically creates filesystem checkpoints before any destructive file operation — `write_file`, `patch`, and certain terminal commands. Checkpoints are stored as commits in a shadow git repository scoped to your working directory. If something goes wrong, you can roll back to any prior checkpoint.

---

## How Checkpoints Work

Before every destructive operation, EdgeCrab:

1. Checks if `checkpoints.enabled` is `true` (default)
2. Resolves the shadow git repository path: `~/.edgecrab/checkpoints/<sha256_of_cwd>/`
3. Stages all tracked files in the working directory
4. Creates a commit with a label like `checkpoint: before write_file src/main.rs`

The shadow repo is fully isolated from your project's git history — it tracks files for rollback purposes only, never interfering with your commits or branches.

---

## Configuration

```yaml
# ~/.edgecrab/config.yaml
checkpoints:
  enabled: true          # master switch (default: true)
  max_snapshots: 50      # max checkpoints per working directory
```

Enable/disable per-session:

```bash
edgecrab --checkpoints    # (flag not yet in v0.x; use config)
```

Disable globally:

```yaml
checkpoints:
  enabled: false
```

---

## Viewing Checkpoints

Inside the TUI:

```
/rollback           # opens the rollback UI showing recent checkpoints
```

This shows a numbered list of available checkpoints with their labels and timestamps.

---

## Rolling Back

From the TUI, after `/rollback`, select the checkpoint number to restore to. EdgeCrab:

1. Shows a diff of what will change
2. Asks for confirmation
3. Restores the files from the selected checkpoint

From the command line, use the tools via the agent:

```
> Show me all checkpoints for this project
> Roll back to checkpoint 3
> What changed between checkpoints 2 and 4?
> Restore just the file src/main.rs from checkpoint 5
```

The agent uses these internal operations (exposed through `manage_checkpoints` tool internally):

| Operation | Description |
|-----------|-------------|
| `checkpoint create "label"` | Manually create a checkpoint |
| `checkpoint list` | List all checkpoints for the CWD |
| `checkpoint restore N` | Restore all files to checkpoint N |
| `checkpoint diff N` | Show diff between current state and checkpoint N |
| `checkpoint restore_file N <file>` | Restore a single file from checkpoint N |

---

## Manual Checkpoints

Create a checkpoint before a risky operation:

```
❯ Before we do the big refactor, create a checkpoint
```

The agent calls `checkpoint create "before refactor"`.

Or use the slash command:

```
/rollback               # opens interactive rollback, also shows create option
```

---

## Checkpoint Storage

Checkpoints are stored in `~/.edgecrab/checkpoints/<sha256_of_cwd>/` — one shadow git repo per working directory. This means:

- Working on `~/project-a` keeps its checkpoints separate from `~/project-b`
- The shadow repos are never visible in your project's git log
- Old checkpoints are pruned when `max_snapshots` is exceeded (oldest first)

---

## What Gets Checkpointed

| Operation | Checkpoint created? |
|-----------|---------------------|
| `write_file` | ✅ Before writing |
| `patch` | ✅ Before patching |
| Terminal commands flagged as destructive | ✅ Before execution |
| `read_file` | ❌ Read-only |
| `web_search` | ❌ No file changes |

---

## Disabling for a Session

```bash
# Currently disable globally in config.yaml
checkpoints:
  enabled: false
```

When disabled, no shadow git repos are created or written. Previously created checkpoints remain on disk until pruned.

---

## Pro Tips

**Create a manual checkpoint before anything destructive.** Before a major refactoring, ask the agent: "Create a checkpoint labeled 'before-refactor'." This gives you a named recovery point you can come back to.

**Use `/rollback` interactively.** Don't try to remember checkpoint IDs. `/rollback` in the TUI shows a list of checkpoints with timestamps and labels — pick the one you want.

**Checkpoints are per-directory.** They track the directory where EdgeCrab was running, not a project name. Always start EdgeCrab from your project root for consistent checkpoint scoping.

---

## Frequently Asked Questions

**Q: How is this different from `git stash` or git branches?**

Checkpoints are completely transparent to your git history. They live in a shadow git repo in `~/.edgecrab/checkpoints/` and never appear in your project's `git log`. You can use checkpoints and git branches simultaneously.

**Q: Will checkpoints save large binary files?**

Yes, but they'll be large. Checkpoints use git under the hood. If your project has large binary files, exclude them or disable checkpoints for those sessions.

**Q: How do I know if a checkpoint was created?**

Watch the TUI — a `⚙ checkpoint` line appears before every file write. The checkpoint ID is shown in the message.

**Q: Can I create a checkpoint via CLI (not TUI)?**

Yes, the agent can create one: `edgecrab "create a checkpoint before we start"`. There's no dedicated `edgecrab checkpoint` CLI subcommand — it's managed through the agent loop.

**Q: Checkpoints are taking up a lot of disk space. How do I clean them?**

```yaml
checkpoints:
  max_snapshots: 20   # keep only the last 20 (default: 50)
```
Or manually delete the shadow repos:
```bash
rm -rf ~/.edgecrab/checkpoints/
```
This deletes all checkpoint history. Existing project files are unaffected.

---

## See Also

- [Worktrees](/user-guide/worktrees/) — Git worktree isolation for full branch-level safety
- [Security Model](/user-guide/security/) — How checkpoints interact with the approval policy
- [Configuration](/user-guide/configuration/) — `checkpoints.*` config options
