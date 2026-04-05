---
title: Git Worktrees
description: Run multiple EdgeCrab agents in parallel on the same repository using isolated git worktrees. Grounded in crates/edgecrab-cli/src/cli_args.rs.
sidebar:
  order: 7
---

Git worktrees let you run multiple EdgeCrab sessions in parallel on the same repository without agents interfering with each other. Each session gets its own branch and working directory.

---

## When to Use Worktrees

Use worktrees when you want to:

- Run two agents simultaneously on the same repo
- Try an approach in isolation before deciding to keep it
- Work on a feature while an agent handles an unrelated bug fix
- Evaluate different prompting strategies in parallel

---

## Enabling Worktrees

### Per-invocation

```bash
edgecrab -w "refactor the authentication module"
edgecrab --worktree "add rate limiting to the API"
```

Each `-w` invocation creates a new branch and worktree under `.worktrees/` in your current git repository.

### Always-on

```yaml
# ~/.edgecrab/config.yaml
# Not directly supported in config — use -w flag per session
```

### One-shot (quiet mode)

```bash
edgecrab -w -q "write tests for the parser module" | tee output.txt
```

---

## How It Works

When you run `edgecrab -w`:

1. EdgeCrab creates a new branch: `edgecrab/<timestamp>-<short-hash>`
2. Creates a worktree at `.worktrees/<branch-name>/`
3. Starts the agent session from that worktree directory
4. **On exit:** If the worktree has no uncommitted changes, it is removed automatically. If changes exist, the worktree is preserved for manual recovery.

```
my-project/
├── src/              # main branch
├── .worktrees/
│   ├── edgecrab-1714832400-a1b2c3/   # agent session 1
│   └── edgecrab-1714832450-d4e5f6/   # agent session 2
```

---

## Parallel Workflow Example

```bash
# Terminal 1 — refactor auth
edgecrab -w "refactor authentication to use JWT with refresh tokens"

# Terminal 2 — add tests
edgecrab -w "write comprehensive unit tests for the user module"

# Terminal 3 — fix a bug
edgecrab -w "fix the race condition in the session manager"
```

All three agents work in isolation. When done, review each branch, cherry-pick what you want, and clean up:

```bash
git branch -a                  # list all edgecrab branches
git diff main edgecrab/...     # review changes
git merge edgecrab/...         # merge good work
git branch -D edgecrab/...     # clean up
```

---

## Including Gitignored Files

By default, worktrees don't inherit gitignored files (`.env`, `node_modules/`, `.venv/`, etc.). Create a `.worktreeinclude` file in your repo root to copy specified patterns into each worktree:

```
# .worktreeinclude
.env
.venv/
node_modules/
.cargo/
```

Files matching these patterns are copied (not symlinked) into new worktrees before the agent starts.

---

## Worktrees in Config (Global Toggle)

To always use worktrees without the `-w` flag, there's no direct config key — but you can create a shell alias:

```bash
alias ec='edgecrab -w'
```

Or set your default workflow in a profile:

```bash
edgecrab profile create isolated
# edit ~/.edgecrab/profiles/isolated/config.yaml
edgecrab -p isolated "task requiring isolation"
```

---

## Cleaning Up

Stale worktrees that weren't cleaned automatically (e.g. the agent crashed):

```bash
# List all worktrees
git worktree list

# Remove a stale worktree
git worktree remove .worktrees/edgecrab-1714832400-a1b2c3
git branch -D edgecrab/1714832400-a1b2c3
```

Or prune all worktrees whose directories no longer exist:

```bash
git worktree prune
```

---

## Pro Tips

**Use worktrees for every risky task.** Make `-w` your default via an alias:
```bash
alias ec='edgecrab -w'   # always isolated
alias ece='edgecrab'     # explicit non-isolated
```

**Review the branch diff before merging.** The agent may have made changes you want to cherry-pick rather than merge wholesale:
```bash
git diff main edgecrab/1714832400-a1b2c3 -- src/auth.rs
git show edgecrab/1714832400-a1b2c3:src/auth.rs
```

**Name sessions when using worktrees.** `edgecrab -w --session auth-refactor` makes it easy to correlate the branch (`edgecrab/auth-refactor`) with the session.

---

## Frequently Asked Questions

**Q: Do worktrees work in repos with submodules?**

Submodules are not automatically initialized in new worktrees. Run `git submodule update --init` in the worktree directory after EdgeCrab creates it.

**Q: The agent created changes in the worktree but I want them in main. How do I merge?**

```bash
git merge edgecrab/1714832400-a1b2c3        # merge all changes
git cherry-pick <sha>                        # take specific commits
git diff main edgecrab/... | git apply       # apply as uncommitted changes
```

**Q: I ran `edgecrab -w` but my `.env` file isn't in the worktree. How do I fix this?**

Add `.env` to `.worktreeinclude` in your repo root:
```
# .worktreeinclude
.env
.venv/
node_modules/
```
EdgeCrab copies (not symlinks) these files into new worktrees so secrets are available.

**Q: Can I run multiple agents in parallel worktrees at the same time?**

Yes — that's one of the main use cases. Run three terminals:
```bash
edgecrab -w "explore approach A"
edgecrab -w "explore approach B"
edgecrab -w "explore approach C"
```
Each gets its own branch and worktree directory. They don't interfere.

**Q: A worktree directory still exists but `git worktree list` doesn't show it.**

Run `git worktree prune` to clean up stale references. Then manually `rm -rf .worktrees/<stale-dir>`.

---

## See Also

- [Quick Start](/getting-started/quick-start/) — `-w` flag in the getting started context
- [Sessions](/user-guide/sessions/) — Session management across worktree sessions
- [CLI Commands](/reference/cli-commands/) — `--worktree` flag details
