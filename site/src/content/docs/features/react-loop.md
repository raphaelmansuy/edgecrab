---
title: ReAct Tool Loop
description: Deep dive into EdgeCrab's autonomous ReAct (Reason + Act) loop — how the agent plans, executes tools, observes results, and adapts until a task is complete.
sidebar:
  order: 2
---

EdgeCrab's ReAct loop is the intelligence engine that makes it autonomous. Understanding it helps you write better prompts and interpret agent behavior.

---

## What is ReAct?

ReAct (Reason + Act) is an agent architecture that interleaves:

1. **Reasoning** — The LLM thinks through the goal and decides what to do next
2. **Acting** — The agent executes a tool (file read, shell command, web search, etc.)
3. **Observing** — The result is fed back to the LLM
4. **Loop** — Repeat until the task is complete or no more actions are needed

This is fundamentally different from a chatbot: EdgeCrab doesn't just generate text — it takes actions and adapts based on real-world feedback.

---

## The Loop in Detail

```
User prompt
     │
     ▼
┌────────────────────────────────────────────────┐
│                  LLM Reasoning                 │
│  "I need to read the Cargo.toml to understand  │
│   what dependencies this project has."         │
└────────────────────┬───────────────────────────┘
                     │ tool_call: file_read("Cargo.toml")
                     ▼
┌────────────────────────────────────────────────┐
│               Tool Execution                   │
│  edgecrab-tools::file::read("Cargo.toml")      │
│  → security check (allowed_roots)              │
│  → read file                                   │
│  → return content                              │
└────────────────────┬───────────────────────────┘
                     │ tool_result: "[package]\nname = ..."
                     ▼
┌────────────────────────────────────────────────┐
│                  LLM Reasoning                 │
│  "The project uses tokio 1.x. Let me check     │
│   if there are any known issues with the       │
│   version they're using."                      │
└────────────────────┬───────────────────────────┘
                     │ tool_call: web_search("tokio 1.x breaking changes 2025")
                     ▼
        ... (continues until task complete)
```

---

## How Tool Calls Work

The LLM communicates tool calls as structured JSON in its response. EdgeCrab parses these and dispatches them to the appropriate tool implementation in `edgecrab-tools`.

Each tool has:
- A **name** (e.g. `file_read`)
- A **JSON schema** describing its arguments
- A **synchronous or async handler** in Rust
- A **security wrapper** that validates before execution

The tool result is inserted into the conversation as a `tool` role message.

---

## Recursion Safety

The loop has a configurable maximum depth to prevent infinite loops or runaway LLM behavior:

```yaml
# config.yaml
tools:
  max_loop_depth: 20   # Default: 20 iterations per user turn
```

If the loop hits the limit, EdgeCrab reports to the user instead of silently stopping:

```
⚠  Max loop depth (20) reached. The agent may not have completed the task.
   Try increasing `tools.max_loop_depth` or breaking the task into smaller steps.
```

---

## Tool Call Transparency

Every tool call and its result is shown in the TUI in real time:

```
⚙  file_read  {"path": "src/main.rs", "start_line": 1, "end_line": 50}
   → 50 lines read (1.2 KB)

⚙  terminal_exec  {"command": "cargo test -p edgecrab-core 2>&1 | head -50"}
   → exit code: 0, 12 tests passed
```

The `⚙` prefix marks tool calls. The indented `→` line shows the result summary. Full results are stored in the session history.

---

## Interrupting the Loop

Press `Ctrl-C` at any time to:
- Cancel the current tool execution (if running)
- Cancel the current LLM generation
- Return control to the input bar (session history is preserved)

This is safe even during file writes — EdgeCrab uses atomic write operations (write to temp, then rename).

---

## When Does the Loop End?

The loop ends when:
1. The LLM generates a final response with **no tool calls** (task complete)
2. The user presses `Ctrl-C`
3. The max loop depth is reached
4. A tool returns a terminal error (e.g. file permission denied after 3 retries)

---

## Customizing Tool Availability

You can restrict which tools are available for a session:

```bash
# Enable only file and terminal tools
edgecrab --toolset file,terminal "refactor this module"

# Disable web search entirely
edgecrab --toolset file,terminal,memory,skills,session "work offline"
```

Inside a session:

```
/tools                # Show currently active tools
```

---

## System Prompt and Tool Injection

At the start of every session, EdgeCrab builds a system prompt that includes:
1. The user's name and preferences (from memory)
2. The active skills (as context)
3. Tool descriptions (so the LLM knows what's available)
4. Current date and working directory

This means the agent always has awareness of what it can do and who it's talking to.

---

## Parallel Tool Calls

The LLM can request multiple tool calls in a single response. EdgeCrab executes independent calls in parallel when safe:

- Multiple `file_read` calls on different files → executed concurrently
- A `web_search` and a `memory_read` → executed concurrently
- A `file_write` followed by `terminal_exec` → always sequential (order matters)

Parallelism is safe by default: each tool call runs in its own tokio task with its own security context.

---

## Common Prompt Patterns

These patterns give the agent the context it needs to complete tasks with minimal back-and-forth.

**Pattern: Explicit scope**
```
Review src/auth/*.rs for security issues. Check for: SQL injection, insecure deserialization,
missing input validation. Output a prioritized list with file:line references.
```

**Pattern: Step-by-step constraint**
```
Add a health check endpoint to crates/api/src/main.rs.
Steps:
1. Read the existing route structure
2. Add GET /health returning {"status":"ok","version":"<cargo version>"}
3. Add a test
4. Run cargo test -p api to verify
Do not modify other files.
```

**Pattern: Iterative refinement**
```
Start with the simplest implementation. I'll review and ask for improvements.
```

---

## Frequently Asked Questions

**Q: The agent is looping and not making progress. What's happening?**

Usually this means the model is unsure what to do next and keeps reading the same files. Try being more explicit: name the file to modify, the exact change to make, or the command to run. You can also interrupt with `Ctrl+C` and restart with a more targeted prompt.

**Q: How many iterations does the agent typically use for a real task?**

Simple tasks (fix a typo, explain a function): 1-3 iterations.
Medium tasks (add a feature, write tests): 5-15 iterations.
Complex tasks (refactor a module, debug a subtle bug): 15-30 iterations.

If the default `max_loop_depth: 20` feels too low, increase it:
```yaml
tools:
  max_loop_depth: 40
```

**Q: Can the agent run multiple tasks in parallel?**

Not within a single session. But you can run multiple `edgecrab` processes in different terminals (or use worktrees). Each session has its own context and tool dispatch.

**Q: The agent keeps asking for confirmation. How do I reduce interruptions?**

Adjust `security.approval_required`. By default only destructive operations require approval. You can tune which patterns trigger approval:
```yaml
security:
  approval_required:
    - pattern: "rm -rf"
    - pattern: "drop table"
```
Remove entries to reduce prompts. Add entries for patterns you want reviewed.

**Q: Does the agent read my entire codebase at once?**

No. It reads files on demand using `file_read` and `file_search` tools. This makes it efficient for large repos — it only loads what's relevant. For project-wide context, use `AGENTS.md` to describe the architecture. The agent reads this at startup.

---

## See Also

- [Tools](/features/tools/) — Complete list of all available tools
- [Security Model](/user-guide/security/) — How tool calls are checked before execution
- [Configuration](/user-guide/configuration/) — `tools.max_loop_depth` and other loop settings
- [TUI Interface](/features/tui/) — How tool calls are displayed in the terminal UI
- [Skills](/features/skills/) — Pre-loaded procedures that guide the ReAct loop
