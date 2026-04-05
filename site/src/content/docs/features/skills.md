---
title: Skills System
description: Agent-created procedural memory — how EdgeCrab discovers, loads, and installs skills from directories containing SKILL.md. Grounded in crates/edgecrab-tools/src/tools/skills.rs.
sidebar:
  order: 4
---

Skills are **portable, reusable procedural instructions** that EdgeCrab loads into its system prompt at session start. Think of them as "how-to guides" the agent can follow: a `security-audit` skill might walk it through OWASP Top 10 checks; a `deploy-k8s` skill might outline a safe Kubernetes rollout sequence.

Skills are compatible with [agentskills.io](https://agentskills.io) — the shared open-source skills registry used by all Nous Research agents.

---

## Directory Structure

Each skill is a **directory** containing a `SKILL.md` file — not a flat `.md` file:

```
~/.edgecrab/skills/
├── rust-test-fixer/
│   └── SKILL.md
├── security-audit/
│   └── SKILL.md
│   └── checklist.md        # optional extra context file
└── deploy-k8s/
    └── SKILL.md
    └── examples/
        └── deployment.yaml
```

EdgeCrab resolves skills in this order:

1. `~/.edgecrab/skills/` — primary user skills (highest priority)
2. Directories in `skills.external_dirs` in `config.yaml`
3. Skills bundled with the binary (read-only)

Local user skills always win when a name conflicts with external or bundled skills.

---

## SKILL.md Format

`SKILL.md` uses a YAML frontmatter block followed by Markdown instructions:

```markdown
---
name: security-audit
description: Systematic OWASP Top 10 security audit for web applications.
category: security
platforms:
  - cli
  - telegram
read_files:
  - checklist.md
---

# Security Audit Workflow

You are performing a security audit. Follow these steps:

1. Check authentication mechanisms for common weaknesses...
2. Test for SQL injection entry points...
3. Review session management...
```

### Frontmatter Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | `string` | yes | Unique identifier (becomes a slash command) |
| `description` | `string` | yes | Short summary shown in `/skills` listing |
| `category` | `string` | no | Hub category (e.g. `security`, `devops`, `coding`) |
| `platforms` | `string[]` | no | Limit to platforms (e.g. `["cli", "telegram"]`). Omit = all. |
| `read_files` | `string[]` | no | Extra files in the skill dir to inject as context |

---

## Installing Skills

### From the Hub

```bash
edgecrab skills list                               # list installed skills
edgecrab skills view security-audit               # read a skill
edgecrab skills install official/security-audit   # install from agentskills.io
edgecrab skills install raphaelmansuy/rust-fixer  # install from GitHub
edgecrab skills remove security-audit             # uninstall
```

From inside the TUI:

```
/skills browse         # browse hub
/skills install react  # install a skill
```

### Manual Installation

```bash
mkdir -p ~/.edgecrab/skills/my-skill
cat > ~/.edgecrab/skills/my-skill/SKILL.md << 'EOF'
---
name: my-skill
description: My custom workflow
category: custom
---

# My Skill

When this skill is active, follow these instructions...
EOF
```

No restart needed — EdgeCrab picks up new skills automatically.

---

## Loading Skills

### At Launch

```bash
edgecrab -S security-audit "audit the payment service"
edgecrab -S "security-audit,code-review" "full review"
edgecrab --skill rust-test-fixer --skill code-review
```

### Inside the TUI

```
/security-audit        # load skill, it prompts for input
```

Every installed skill is auto-registered as a slash command. Typing `/security-audit some context` loads the skill and sends `some context` as the first message.

### Permanently in Config

```yaml
# ~/.edgecrab/config.yaml
skills:
  preloaded:
    - security-audit
    - code-review
```

---

## Disabling Skills

Globally disable without uninstalling:

```yaml
skills:
  disabled:
    - heavy-skill
```

Platform-specific disable:

```yaml
skills:
  platform_disabled:
    telegram:
      - heavy-skill   # disabled in Telegram, active in CLI
```

---

## External Skill Directories

Share skills across projects or teams:

```yaml
# ~/.edgecrab/config.yaml
skills:
  external_dirs:
    - ~/.agents/skills           # another agent's directory
    - /shared/team/skills        # team skills
    - ${SKILLS_REPO}/skills      # env-var reference
```

Supports `~` expansion and `${VAR}` substitution. External directories are read-only.

---

## Skills vs Memory vs Context Files

| Concept | What it is | How it's populated |
|---------|------------|---------------------|
| Skills | Procedural workflow instructions | Written by you or hub-installed |
| Memory | Persistent facts about you and your projects | Auto-written by the agent |
| Context files | Project-level instructions (AGENTS.md, etc.) | You write; auto-discovered |

Skills are loaded on demand. Memory is always loaded (unless `--skip-memory`). Context files are project-scoped.

---

## Example: Writing a Security Audit Skill

```markdown
---
name: security-audit
description: Run an OWASP-aligned security review of Rust code
category: security
---

# Security Audit

When this skill is active, perform a structured security review:

## Step 1 — Identify entry points
Find all public HTTP handlers, CLI entry points, and IPC endpoints.
List them with file:line references.

## Step 2 — Check input validation
For each entry point, verify:
- All user-supplied data is validated before use
- Path inputs are sanitized (no traversal)
- Integer inputs are range-checked

## Step 3 — Check secrets handling
Search for hardcoded secrets, passwords, or API keys.
Verify secrets are loaded from environment, not source code.

## Step 4 — Generate report
Output a prioritized list: CRITICAL / HIGH / MEDIUM / LOW.
Include file:line for each finding and a one-line fix suggestion.
```

Save as `~/.edgecrab/skills/security-audit/SKILL.md` and invoke with:
```bash
edgecrab -S security-audit "audit crates/api/"
```

---

## Pro Tips

**Keep skills focused.** A skill that does one thing well (e.g., "write migration tests") is more reliable than a monolithic "do everything" skill. The agent follows procedures better when they're specific.

**Use skills for consistent team workflows.** Put shared skills in a git repo, then point everyone's config to it via `skills.external_dirs`. Now every developer follows the same PR-review checklist.

**Layer skills.** You can load multiple skills in one session:
```bash
edgecrab -S code-review,security-audit "review PR #42"
```
The skills are concatenated in the system prompt. Keep their combined length manageable (< 2000 tokens).

**Check what's loaded.** Inside a session, run `/skills` to see which skills are active and their descriptions.

---

## Frequently Asked Questions

**Q: My skill is a `.md` file, not a directory. Why doesn't it load?**

Skills must be **directories** containing a `SKILL.md` file. A bare `security-audit.md` is not recognized. Rename it:
```bash
mkdir -p ~/.edgecrab/skills/security-audit
mv security-audit.md ~/.edgecrab/skills/security-audit/SKILL.md
```

**Q: How long can a skill be?**

There's no hard limit, but skills longer than ~2000 tokens may push other context out of the window. Keep the core procedure concise; reference external files using `[see](./checklist.md)` inside the skill directory.

**Q: Can skills call other skills?**

Not directly, but you can reference another skill's content by putting it in the same directory:
```
my-complex-skill/
  SKILL.md           # main instructions (can say "follow checklist.md")
  checklist.md       # sub-procedure loaded as additional context
```

**Q: Can I share skills between EdgeCrab and Hermes Agent?**

Yes. Skills are compatible across all `agentskills.io` agents. Point `skills.external_dirs` at your Hermes skills directory:
```yaml
skills:
  external_dirs:
    - ~/.hermes/skills
```

**Q: How do I know if a skill from agentskills.io is safe?**

Skills are Markdown files — they cannot execute code. Review the `SKILL.md` before installing. The edgecrab hub shows source links and contributor info.

---

## See Also

- [Building Your First Skill](/guides/first-skill/) — Step-by-step guide with testing
- [Context Files](/features/context-files/) — How SOUL.md and AGENTS.md interact with skills
- [Memory](/features/memory/) — Persistent facts versus procedural skills
- [CLI Commands](/reference/cli-commands/) — `edgecrab skills` subcommand
