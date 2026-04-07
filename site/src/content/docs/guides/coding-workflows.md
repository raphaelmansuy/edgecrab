---
title: Autonomous Coding Workflows
description: Practical patterns for using EdgeCrab as an autonomous coding agent — multi-step refactors, bug hunts, test generation, code reviews, and documentation runs.
sidebar:
  order: 2
---

This guide shows real-world EdgeCrab workflows that go beyond simple Q&A. Each pattern is a prompt strategy plus a set of skills that produces reliable results.

---

## Pattern 1 — Bug Hunt

**Goal**: Find and fix a regression without knowing which file it's in.

```
There's a regression where authenticated users are being logged out after 5 minutes
even though the session should last 24 hours. Find the root cause and fix it.
```

What EdgeCrab does:
1. Searches codebase for session-related code (`web_search`-free)
2. Reads session configuration files
3. Reads the authentication middleware
4. Reads recent commit history (`git log --oneline -20`)
5. Narrows to the likely commit with `git show`
6. Reads the changed lines
7. Proposes and implements a fix
8. Offers to run the test suite

**Tips**:
- Include a reproduction step in your prompt if you have one
- Name the feature area when you know it ("in the session expiry logic")

---

## Pattern 2 — Guided Refactor

**Goal**: Rename a type across the entire codebase safely.

```
Rename the Rust type `UserRecord` to `User` throughout the entire codebase.
Run cargo check after each file to confirm no new errors. If cargo check fails,
fix any type mismatches before moving on.
```

What EdgeCrab does:
1. Uses `lsp_find_references` or `lsp_workspace_symbols` to identify the symbol precisely
2. Uses `lsp_rename` for a semantic cross-file rename when a server is available
3. Falls back to `search_files` and file edits only if no LSP server covers the language
4. Runs `cargo check` after each batch
5. Fixes any resulting type errors
6. Reports a summary of all changed files

**With a skill**: Create a `rust-rename-type` skill with these steps pre-written so you can invoke it with a single line: `Rename UserRecord to User using the rust-rename-type skill.`

---

## Pattern 3 — Test Generation

**Goal**: Write a comprehensive test suite for an under-tested module.

```
The `edgecrab-security` crate has no tests for the SSRF guard. Write a comprehensive
test suite covering: private IP ranges (IPv4 and IPv6), DNS-based bypasses, valid
public URLs, and edge cases. Use Rust's built-in test framework.
```

What EdgeCrab does:
1. Reads the SSRF guard implementation (`edgecrab-security/src/ssrf.rs`)
2. Reads any existing tests for context
3. Identifies the main code paths and edge cases
4. Writes test functions covering all identified cases
5. Runs `cargo test -p edgecrab-security` to verify they pass

---

## Pattern 3A — Semantic Fix with LSP

**Goal**: Let the agent use language-server data before editing code.

```
Use the LSP tools first. Find the definition of `build_prompt`, inspect references,
pull diagnostics for the file, apply the best code action if one exists, and only then
patch code manually if needed. Run the relevant tests after the fix.
```

What EdgeCrab does:
1. Uses `lsp_goto_definition`, `lsp_find_references`, and `lsp_hover` for semantic context
2. Uses `lsp_diagnostics_pull` or `lsp_workspace_type_errors` before making claims about type errors
3. Tries `lsp_code_actions` / `lsp_select_and_apply_action` for server-suggested fixes
4. Uses `lsp_rename` or formatting tools when the change is semantic
5. Falls back to ordinary file tools only when the server lacks support or the task is textual

---

## Pattern 4 — Documentation Run

**Goal**: Add inline documentation to all public API items.

```
Add Rust doc comments (///) to every public function, struct, and enum in
edgecrab-core/src/agent.rs. Comments should explain what the item does,
not just restate the name. Add examples for the most important functions.
```

What EdgeCrab does:
1. Reads the file to understand the public API
2. Writes documentation for each item
3. Runs `cargo doc --no-deps` to verify it renders correctly
4. Checks for any `cargo clippy -- -D warnings` doc-comment issues

---

## Pattern 5 — Security Audit

**Goal**: Find security issues in a specific module.

```
Perform a security audit of the file tool implementation in
edgecrab-tools/src/tools/file.rs. Focus on path traversal, symlink attacks,
and TOCTOU races. Cite specific line numbers for any issues found.
```

**Even better with the `security-review` skill**:

```
Use the security-review skill to audit edgecrab-tools/src/tools/file.rs.
```

---

## Pattern 6 — Codebase Onboarding

**Goal**: Understand an unfamiliar codebase quickly.

```
I'm new to this codebase. Start with the top-level Cargo.toml and work
down: explain what each crate does, how they depend on each other, and
what the overall data flow is from a user prompt to a tool call. Draw an
ASCII architecture diagram at the end.
```

What EdgeCrab does:
1. Reads `Cargo.toml` (workspace manifest)
2. Reads each crate's `Cargo.toml` and `src/lib.rs` or `src/main.rs`
3. Understands the dependency graph
4. Summarizes the architecture
5. Produces an ASCII diagram

---

## Pattern 7 — CI Failure Triage

**Goal**: Diagnose and fix a failing CI build without running CI yourself.

```
The CI build is failing. Here's the error output:
[paste CI output]

Find the root cause, fix it, and make sure `cargo test` passes locally before
you're done.
```

What EdgeCrab does:
1. Parses the error output
2. Reads the relevant source files
3. Identifies the root cause
4. Implements a fix
5. Runs `cargo test` to confirm it passes

---

## Chaining Workflows

For complex multi-step projects, give EdgeCrab an explicit plan:

```
We're going to refactor the auth module to use JWT. Here's the plan:

1. Read all files in src/auth/ to understand the current implementation
2. Design the new JWT-based approach and explain it to me before touching code  
3. After my approval, implement the changes one file at a time
4. Write tests for the new implementation
5. Run cargo test and fix any failures
6. Summarize all changes in a PR description format

Start with step 1.
```

The explicit numbered plan gives EdgeCrab a clear loop termination condition and lets you review the design before any code is written.

---

## Prompt Tips for Best Results

| Do | Don't |
|----|-------|
| Specify the file or module name when you know it | Give open-ended "improve this codebase" |
| Ask for tests to be run after changes | Trust the agent to self-validate without running tests |
| Request a summary of changes at the end | Let the loop end without a completion report |
| Give constraints ("don't modify the test files") | Let constraints be implied |
| Provide context ("this broke after commit abc123") | Make the agent guess the context |

---

## Workflow Pattern Summary

| Pattern | Best For | Key Prompt Element |
|---------|----------|--------------------|
| Bug Hunt | Regressions, unknown root cause | Symptom + expected behaviour |
| Guided Refactor | Rename, extract, restructure | Old name → New name + "run check after each file" |
| Test Generation | Missing tests, edge-case coverage | Module path + coverage goals |
| Documentation Run | Public API doc comments | File path + quality bar |
| Security Audit | Module-level hardening | File path + threat categories |
| Codebase Onboarding | Unfamiliar repo | "Start with Cargo.toml, explain each crate" |
| CI Failure Triage | Broken CI, pasted error log | Paste full error + "fix and run tests" |

---

## Pro Tips

- **Give a termination condition**: "You're done when `cargo test --workspace` passes" prevents the agent from over-iterating.
- **Ask for a diff summary at the end**: "Summarize all changed files and why" produces useful commit message material.
- **Use a skill for repeat workflows**: Patterns 1–7 above can each be packaged as a skill so you invoke them with one line instead of a multi-sentence prompt.
- **Set `max_iterations` higher for large refactors**: A 50-file rename needs more loop iterations. Use `EDGECRAB_MAX_ITERATIONS=150` or `--max-iterations 150` for big tasks.
- **Gate on human review for destructive ops**: Prepend "Don't delete or overwrite any file without showing me the change and asking for confirmation first" to any large-scale workflow.

---

## FAQ

**What if the agent is stuck in a loop?**
Use `/stop` to cancel the current run. Then either refine your prompt or set a lower `max_iterations` to force an early summary.

**Can I chain skills with inline instructions?**
Yes: `Use the security-review skill on src/auth/, then use the test-generation skill to write tests for any issues you find.`

**How do I resume a workflow that was cancelled?**
Use `/retry` to resend the last message, or start a new message with "Continue where you left off" if the session history is intact.

**What is a session?**
A session is the conversation history for a single `edgecrab run` context. Sessions are stored in `~/.edgecrab/state.db`.

---

## See Also

- [Skills System](/features/skills/) — package any pattern as a reusable skill
- [Slash Commands](/reference/slash-commands/) — `/stop`, `/retry`, and live session controls
- [Sessions](/user-guide/sessions/) — session persistence and the `--session-id` flag
