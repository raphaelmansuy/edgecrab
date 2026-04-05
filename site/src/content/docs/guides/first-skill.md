---
title: Building Your First Skill
description: Step-by-step tutorial for creating a reusable EdgeCrab skill. From writing the Markdown file to testing, iterating, and sharing your skill.
sidebar:
  order: 1
---

Skills are the most powerful way to extend EdgeCrab. This tutorial walks you through creating a skill from scratch — a practical `git-pr-review` skill that performs a thorough pull request review.

---

## What We'll Build

A skill that:
1. Lists the files changed in the current branch vs. `main`
2. Reads each changed file
3. Reviews the code for correctness, security, and style
4. Produces a structured review report

**Total time: ~15 minutes**

---

## Step 1 — Create the Skill File

```bash
mkdir -p ~/.edgecrab/skills
cat > ~/.edgecrab/skills/git-pr-review.md << 'EOF'
---
name: git-pr-review
description: Perform a thorough code review of the current branch's changes vs. main. Produces a structured review with blocking issues, suggestions, and praise.
capabilities:
  - code review
  - pull request
  - PR review
  - git diff
  - review changes
version: 1.0.0
---

# Git PR Review Skill

## When to Use

Use this skill when asked to:
- Review a pull request or branch
- Check what's changed vs. main
- Identify issues in the current diff

## Review Process

### Step 1 — Discover Changes

Run:
```bash
git diff main...HEAD --name-only
```

This lists all files changed in the current branch vs. the `main` branch.

### Step 2 — Get the Diff

Run:
```bash
git diff main...HEAD --stat
```

Then for a focused view of the full diff:
```bash
git diff main...HEAD
```

### Step 3 — Read Each Changed File

For each changed file, use `file_read` to read the **full current version** of the file, not just the diff.

### Step 4 — Review Each File

For each file, check:

**Correctness**
- Does the logic do what the commit message says?
- Are there off-by-one errors, null pointer risks, or race conditions?
- Are errors handled appropriately?

**Security**
- No hardcoded secrets or credentials
- Input is validated before use
- No SQL injection, path traversal, or SSRF risks

**Code Quality**
- Functions are focused and testable
- Naming is clear and consistent
- No dead code or commented-out code left in

**Tests**
- Are new behaviors covered by tests?
- Are existing tests still passing? (Run `cargo test` or relevant test command)

### Step 5 — Produce the Review Report

Format the review as:

```
## PR Review: [branch name] → main

### Summary
[2–3 sentence overview]

### 🚨 Blocking Issues
[list — things that MUST be fixed before merge]

### ⚠️ Suggestions
[list — things that should be addressed but are not blockers]

### ✅ Looks Good
[list — call out good patterns and well-written code]

### Next Steps
[clear list of actions for the author]
```

## Important Rules

- Be specific: cite file names and line numbers
- Be constructive: every criticism should have a suggestion
- Be thorough: don't skip files or sections
- Run tests before reporting test-related issues
EOF
```

---

## Step 2 — Test the Skill

Start EdgeCrab and use the skill:

```bash
edgecrab
```

Then type:

```
Please use the git-pr-review skill to review the current branch.
```

Or more directly:

```
Review the PR for this branch using the git-pr-review skill.
```

---

## Step 3 — Observe and Iterate

Watch what the agent does:
- Does it run `git diff main...HEAD --name-only`? ✓
- Does it read the relevant files? ✓
- Does the review report follow the structure? ✓

If something is missing, edit `~/.edgecrab/skills/git-pr-review.md` and:

```
/theme    # Reload (skills are reloaded with the theme)
```

Or exit and restart EdgeCrab.

---

## Step 4 — Add Real-World Tests to the Skill

Edit the skill to add examples of what good output looks like. This "few-shot" context improves LLM behavior:

```markdown
## Example Output

### 🚨 Blocking Issues
- **src/auth.rs L42**: Password is compared with `==` instead of a constant-time comparison. Use `subtle::ConstantTimeEq`. CVE risk: timing attack.
- **src/api/users.rs L78**: User ID from URL parameter is interpolated directly into SQL query. Parameterize it.
```

---

## Step 5 — Share the Skill

Skills are plain Markdown files — share them however you like:
- Copy to another machine's `~/.edgecrab/skills/`
- Commit to your team's shared config repository
- Submit to the [EdgeCrab community skills repository](https://github.com/raphaelmansuy/edgecrab/tree/main/skills)

---

## Skill Design Tips

1. **Be explicit about steps**: Number them. The agent follows numbered lists reliably.
2. **Name tools you need**: Say "use `file_read`" rather than "read the file" — it reduces ambiguity.
3. **Add examples**: Few-shot examples in the skill body dramatically improve output quality.
4. **Define rules**: A "Rules" or "Important" section at the end acts as a checklist the agent follows.
5. **Keep it focused**: One skill = one clearly-defined workflow. Better to have 10 focused skills than 1 sprawling one.
6. **Version it**: Use a `version` frontmatter field so you can track improvements.

---

## Pro Tips

- **Use `capabilities` keywords that match natural prompts**: The skill is triggered by fuzzy match on `capabilities`. Include synonyms like `"code review"`, `"PR review"`, `"review changes"` to improve hit rate.
- **Test with `/skills`**: The TUI command `/skills` shows the current list of loaded skills and their descriptions. If your skill doesn't appear, check frontmatter YAML syntax.
- **Add a `## When NOT to Use` section**: Prevents the agent from applying the skill in the wrong context — e.g. a `git-pr-review` skill should not trigger when the user says "review my writing".
- **Start skill instructions with a verb**: "Run `git diff...`" is clearer than "The agent should run `git diff...`". Command form reduces ambiguity.
- **Skills are versioned by embedding `version` in frontmatter**: Increment when you make behaviour-changing edits so you can tell which version a trajectory used.

---

## FAQ

**Where should I store skills?**
Global skills (work in any project): `~/.edgecrab/skills/`. Project skills (checked into the repo): `./skills/` in your project root. Both locations are loaded automatically.

**How does EdgeCrab know when to use a skill?**
It runs a fuzzy match between your message and the `capabilities` list in the skill frontmatter. Be explicit and include synonyms.

**Can a skill call another skill?**
Yes. Include `Use the [skill-name] skill for step X.` in the skill body. The agent resolves the referenced skill and executes its steps.

**Can I disable a skill without deleting it?**
Rename the file to `skill-name.md.disabled`. The runtime ignores files that don't end in `.md`.

**How do I make a skill run on every session automatically?**
Include it in `~/.edgecrab/config.yaml` under `skills.autoload: ["skill-name"]` — the skill's instructions are prepended to the system prompt on every session.

---

## See Also

- [Skills System](/features/skills/) — full runtime reference and frontmatter schema
- [Autonomous Coding Workflows](/guides/coding-workflows/) — chain multiple skills into longer workflows
- [Configuration Reference](/reference/configuration/) — `skills.autoload` and `skills.dir` config keys
