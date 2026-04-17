# EdgeCrab SDK — Roadblocks, Edge Cases & Risk Analysis

> **Cross-references:** [ADR](05-ADR.md) | [IMPL](06-IMPLEMENTATION.md) | [SPEC](02-SPEC.md)

---

## WHY This Document Exists

Every SDK spec document shows the happy path. This document shows the unhappy paths — the edge cases that will bite users, the platform gotchas that will waste engineering weeks, and the fundamental tensions that have no perfect solution. Ignoring these doesn't make them go away; it makes them surface in production.

---

## 1. FFI Boundary Hazards

### 1.1 PyO3 + Tokio Runtime Conflicts

```
+----------------------------------------------------------------+
|                    The Runtime Conflict                         |
+----------------------------------------------------------------+
|                                                                |
|  User code (Python):                                           |
|  +----------------------------------------------------------+ |
|  | import asyncio                                            | |
|  | async def main():                                         | |
|  |     agent = Agent()                                       | |
|  |     # Agent internally creates tokio runtime              | |
|  |     result = await agent.chat("hello")                    | |
|  |     # PyO3 bridges asyncio <-> tokio                      | |
|  | asyncio.run(main())                                       | |
|  +----------------------------------------------------------+ |
|                                                                |
|  PROBLEM: What if user already has a tokio runtime?            |
|  +----------------------------------------------------------+ |
|  | # User using pyo3-asyncio in their own Rust extension     | |
|  | # Two tokio runtimes = undefined behavior or panic        | |
|  +----------------------------------------------------------+ |
|                                                                |
|  PROBLEM: Sync wrapper in async context                        |
|  +----------------------------------------------------------+ |
|  | async def main():                                         | |
|  |     result = agent.chat_sync("hello")  # DEADLOCK         | |
|  |     # chat_sync creates new runtime, but we're already    | |
|  |     # inside asyncio event loop                           | |
|  +----------------------------------------------------------+ |
+----------------------------------------------------------------+
```

**Mitigation:**
- Detect existing tokio runtime before creating new one
- `chat_sync()` must detect nested event loop and raise clear error
- Document: "Do not call `chat_sync()` from inside an async context"
- Consider `nest_asyncio` as optional dependency for Jupyter notebooks

### 1.2 Memory Leaks Across FFI

```
Rust side:   Arc<Agent> created per SDK instance
Python side: Agent.__del__ must ensure Rust Arc is dropped
Edge case:   Circular references prevent __del__ from running

Risk: Each Agent holds:
  - Arc<ToolRegistry> (~500KB with 90+ tool schemas)
  - Arc<SessionDb> (SQLite connection)
  - Vec<Message> (conversation history, potentially MB)
  - tokio::Runtime (thread pool)
```

**Mitigation:**
- Implement `Agent.close()` / `Agent.__aenter__` + `__aexit__` for explicit cleanup
- Use weak references for back-pointers
- Add `sys.getrefcount()` integration tests
- Document context manager pattern as recommended

### 1.3 Signal Handling

```
PROBLEM: User presses Ctrl+C during agent.run()

Python:  KeyboardInterrupt raised in main thread
Rust:    tokio task still running in background
Result:  Rust task completes, writes to session DB,
         but Python has already unwound the stack

Worse:   Ctrl+C during FFI call = potential segfault
```

**Mitigation:**
- Register signal handler that calls `agent.interrupt()` before re-raising
- `interrupt()` sets `CancellationToken` in the Rust runtime
- Agent loop checks token at each iteration boundary
- `Agent.__del__` sends cancellation on drop

---

## 2. Platform-Specific Nightmares

### 2.1 manylinux Wheel Building

```
+----------------------------------------------------------------+
|              The manylinux Compatibility Matrix                 |
+----------------------------------------------------------------+
|                                                                |
|  Target           | GLIBC | OpenSSL | Challenges              |
|  -----------------+-------+---------+------------------------ |
|  manylinux2014    | 2.17  | 1.0.2   | Oldest supported,      |
|  (RHEL 7)         |       |         | many deps unavailable   |
|                    |       |         |                         |
|  manylinux_2_28   | 2.28  | 1.1.1+  | Better, but still      |
|  (RHEL 8)         |       |         | needs static linking    |
|                    |       |         |                         |
|  musllinux_1_2    | musl  | 3.x     | Alpine, static linked,  |
|                    |       |         | but slower              |
+----------------------------------------------------------------+
|                                                                |
|  EdgeCrab links:                                               |
|  - openssl-sys (for HTTPS in web tools)                        |
|  - sqlite3 (for session storage)                               |
|  - ring (for crypto in security crate)                         |
|                                                                |
|  Each of these has manylinux compatibility issues.             |
+----------------------------------------------------------------+
```

**Mitigation:**
- Use `vendored` feature for openssl-sys and libsqlite3-sys
- Use `ring` with static linking
- Build in Docker containers matching each manylinux target
- Test wheels in clean Docker containers before publishing
- Have fallback: pure HTTP client (`edgecrab-client`) for environments where native fails

### 2.2 macOS Universal Binary

```
PROBLEM: Apple Silicon (arm64) vs Intel (x86_64)

maturin can build universal2 wheels, but:
- Cross-compilation requires both toolchains
- Some C dependencies don't cross-compile cleanly
- Fat binaries double the wheel size (~60MB)

Alternative: Ship separate arm64 and x86_64 wheels
- pip resolves correct wheel automatically
- Smaller individual downloads
- But doubles CI build matrix
```

**Mitigation:**
- Ship separate wheels (arm64 + x86_64)
- Use `MACOSX_DEPLOYMENT_TARGET=11.0` for arm64
- Use `MACOSX_DEPLOYMENT_TARGET=10.12` for x86_64

### 2.3 Windows Challenges

```
PROBLEM: Windows has unique issues

1. Path separators: \ vs /
   - All path validation in edgecrab-security uses /
   - Windows paths may use \ or mixed
   - Must normalize before validation

2. File locking:
   - Windows locks files more aggressively
   - SQLite WAL mode can fail if another process has the file
   - Tool file writes may fail if editor has file open

3. Terminal:
   - ANSI escape codes not always supported
   - PowerShell vs cmd.exe vs Git Bash — different behaviors
   - Terminal tool must detect and adapt

4. Long paths:
   - MAX_PATH (260 chars) limit
   - Must opt-in to long path support via manifest
```

**Mitigation:**
- Normalize all paths at FFI boundary
- Use `\\?\` prefix for long paths on Windows
- Test on Windows CI with both cmd and PowerShell
- Document Windows-specific limitations

---

## 3. Concurrency Edge Cases

### 3.1 Multiple Agents, Shared SessionDb

```
PROBLEM: Two Agent instances writing to same sessions.db

Agent A ──write──> sessions.db <──write── Agent B
                   (WAL mode)

WAL mode handles concurrent reads well,
but concurrent writes can cause SQLITE_BUSY.

Worse: Two agents with same session_id = corrupted conversation.
```

**Mitigation:**
- `SessionDb` uses connection pooling with WAL mode
- Each `Agent` gets unique session_id by default
- Document: "Do not share session_id across concurrent agents"
- Add `SQLITE_BUSY` retry with exponential backoff

### 3.2 Tool Execution During Cancellation

```
PROBLEM: agent.interrupt() called while tool is executing

Timeline:
  t=0:  Agent calls terminal("rm -rf /tmp/build")
  t=1:  User calls agent.interrupt()
  t=2:  CancellationToken set
  t=3:  terminal() still running — rm is a system process
  t=4:  Agent loop exits
  t=5:  terminal() completes — but result is discarded
  t=6:  Session saved without the terminal result

Risk: Side effects (file deletion) occurred but aren't recorded.
```

**Mitigation:**
- Tools should check cancellation token at checkpoints
- Terminal tool should kill subprocess on cancellation
- Record partial results: "Tool interrupted: terminal (rm -rf /tmp/build)"
- Agent loop waits for in-flight tool with timeout before exiting

### 3.3 Streaming + Tool Calls Interleaving

```
PROBLEM: During streaming, tool calls arrive mid-token

Stream:  Token("Let me ") → Token("check ") → ToolExec("file_read") →
         [tool runs, returns result] → Token("The file contains ") → ...

In Python async iterator:
  async for event in agent.stream("read the config"):
      if event.is_token:
          print(event.text, end="")
      elif event.is_tool_exec:
          print(f"\n[Using tool: {event.name}]")
          # BUT: user might not handle this case
          # Result: tool calls silently dropped
```

**Mitigation:**
- Document that `stream()` yields ALL event types, not just tokens
- Provide `agent.stream_text_only()` that buffers tool calls silently
- Provide `agent.stream_verbose()` that includes tool call details
- Default `stream()` includes all events — force users to handle them

---

## 4. Schema Inference Limitations

### 4.1 Python Type Hint Edge Cases

```python
# WORKS: Simple types
@Tool("tool1")
async def tool1(name: str, count: int) -> dict: ...
# Schema: {"name": {"type": "string"}, "count": {"type": "integer"}}

# WORKS: Optional
@Tool("tool2")
async def tool2(name: str, count: int = 5) -> dict: ...
# Schema: count not in "required"

# BROKEN: Complex types
@Tool("tool3")
async def tool3(items: list[dict[str, Any]]) -> dict: ...
# Schema: ??? — can't infer nested structure from type hint

# BROKEN: Union types
@Tool("tool4")
async def tool4(value: str | int) -> dict: ...
# Schema: ??? — JSON Schema "oneOf" is model-dependent

# BROKEN: Pydantic models
@Tool("tool5")
async def tool5(config: MyPydanticModel) -> dict: ...
# Schema: Could use model_json_schema(), but adds Pydantic dependency
```

**Mitigation:**
- Support simple types: str, int, float, bool, list[str], Optional[T]
- For complex types, require explicit schema parameter:
  ```python
  @Tool("tool3", schema={"items": {"type": "array", "items": {...}}})
  async def tool3(items: list) -> dict: ...
  ```
- Optionally detect and use Pydantic models if pydantic is installed
- Raise clear error at registration time, not at runtime

---

## 5. Security at the FFI Boundary

### 5.1 Python Object Injection

```
PROBLEM: User passes malicious Python objects through FFI

# Normal usage
result = await agent.run("hello")

# Attack: override __str__ to execute code
class Evil:
    def __str__(self):
        import os; os.system("rm -rf /")
        return "hello"

result = await agent.run(Evil())
# If FFI calls str() on the argument, code executes
```

**Mitigation:**
- FFI boundary accepts only `str` — type-check before conversion
- Use `isinstance(message, str)` guard at entry point
- PyO3 `#[pyfunction]` with explicit `String` parameter does this automatically

### 5.2 Tool Result Data Exfiltration

```
PROBLEM: Custom tool returns data the user didn't intend to expose

@Tool("read_env")
async def read_env(name: str) -> str:
    return os.environ.get(name, "")

# Agent could call: read_env("ANTHROPIC_API_KEY")
# And include the key in its response
```

**Mitigation:**
- Redaction pipeline in edgecrab-core already catches API key patterns
- Document: "Tool results pass through the LLM — do not return secrets"
- Consider opt-in tool result redaction: `@Tool("read_env", redact=True)`

---

## 6. Versioning and Compatibility

### 6.1 SDK Version vs Runtime Version

```
PROBLEM: SDK v0.2.0 built against edgecrab-core v0.6.0
         User has edgecrab CLI v0.7.0 installed
         Are sessions compatible? Are tool schemas compatible?

Scenario:
  SDK creates session with v0.6.0 schema
  CLI opens session with v0.7.0 — new fields added
  SDK opens session again — unknown fields cause panic
```

**Mitigation:**
- Session schema versioning in SQLite (migration on open)
- SDK embeds the runtime — no version mismatch possible in embedded mode
- HTTP client mode: version negotiation in handshake
- Semver: breaking changes only in major versions

### 6.2 Model Catalog Staleness

```
PROBLEM: New model released by Anthropic
         SDK users on version v0.1.0 can't use it
         Model catalog is compiled into the binary

Timeline:
  Day 0:  SDK v0.1.0 released with model_catalog_default.yaml
  Day 15: Anthropic releases claude-4-turbo
  Day 16: User tries Agent("anthropic/claude-4-turbo") → Error: unknown model
```

**Mitigation:**
- Allow custom model strings that bypass catalog validation
- Support `~/.edgecrab/models.yaml` for user-added models (already works in CLI)
- Publish SDK patch releases when new models drop (automated CI)
- Catalog validation is a warning, not an error — unknown models still work

---

## 7. Performance Cliffs

### 7.1 Large Conversation History

```
PROBLEM: Conversation with 500+ messages

Memory:   500 messages × ~2KB avg = ~1MB in memory (fine)
SQLite:   FTS5 index grows with message count (fine up to ~10K)
LLM call: 500 messages → exceeds context window
           → compression trigger → LLM summarize call
           → additional latency + cost on compression turn

User perception: "Why did response take 10 seconds this time?"
```

**Mitigation:**
- Document compression behavior
- Expose `agent.compress()` for manual compression
- Provide `agent.context_usage` → percentage of context used
- Warn when approaching threshold: `StreamEvent.Warning("Context 80% full")`

### 7.2 Tool Execution Timeouts

```
PROBLEM: User's custom tool hangs forever

@Tool("slow_api")
async def slow_api(query: str) -> dict:
    async with aiohttp.ClientSession() as session:
        resp = await session.get(f"https://slow-api.com/{query}")  # Hangs
        return await resp.json()

Agent loop blocks indefinitely waiting for tool result.
```

**Mitigation:**
- Default tool timeout: 120 seconds (configurable per tool)
- `@Tool("slow_api", timeout=30)` for custom timeouts
- ToolError.Timeout raised on expiry — agent loop continues
- Background tool execution with `@Tool("slow_api", background=True)`

---

## 8. Ecosystem Integration Pitfalls

### 8.1 Jupyter Notebook Compatibility

```
PROBLEM: Jupyter already runs an event loop

In Jupyter:
  import asyncio
  asyncio.get_event_loop()  # Already running!

  from edgecrab import Agent
  agent = Agent()
  await agent.chat("hello")
  # Works! (top-level await in IPython)

  agent.chat_sync("hello")
  # DEADLOCK! Can't create nested runtime
```

**Mitigation:**
- Detect Jupyter environment (check `IPython.get_ipython()`)
- In Jupyter, `chat_sync()` uses `nest_asyncio` if available
- Document: "In Jupyter, use `await agent.chat()` — not `agent.chat_sync()`"

### 8.2 Docker / Container Deployment

```
PROBLEM: EdgeCrab writes to ~/.edgecrab/ by default
         Containers are ephemeral — state lost on restart

Also:
  - SQLite WAL mode requires shared memory (not available in some container runtimes)
  - Rust binary may not match container's libc (Alpine = musl, Ubuntu = glibc)
```

**Mitigation:**
- Support `EDGECRAB_HOME` env var for custom state directory
- Document: mount volume at `/app/.edgecrab` or set `EDGECRAB_HOME`
- Provide official Docker image with SDK pre-installed
- Provide `Agent(state_dir="/app/data")` constructor parameter

---

## Summary: Top 5 Risks by Severity

```
+--------------------------------------------------------------------+
|  #  | Risk                          | Severity | Likelihood | ETA  |
+-----+-------------------------------+----------+------------+------+
|  1  | PyO3 manylinux build breaks   |   HIGH   |    HIGH    | Ph 2 |
|  2  | Tokio runtime conflict        |   HIGH   |   MEDIUM   | Ph 2 |
|  3  | Binary size > 50MB            |  MEDIUM  |   MEDIUM   | Ph 2 |
|  4  | Ctrl+C during FFI = segfault  |   HIGH   |    LOW     | Ph 0 |
|  5  | Schema inference fails        |  MEDIUM  |    HIGH    | Ph 2 |
+--------------------------------------------------------------------+
```

---

## Brutal Honest Assessment

### What This Document Gets Right
- Covers real failure modes, not hypothetical ones
- Code examples show the actual failure scenario, not just description
- Mitigations are specific and actionable
- Signal handling / cancellation is a real FFI hazard most docs ignore

### What's Missing
- **No load testing data** — how many concurrent agents can one process handle?
- **No memory profiling** — actual memory footprint of Agent + 90 tools is unknown
- **No benchmarks vs subprocess approach** — is PyO3 actually faster in practice?
- **No accessibility considerations** — screen readers, high contrast, etc. for CLI output

### Improvements Made After Assessment
- Added Docker/container section — production deployments always containerize
- Added Jupyter section — half of Python AI work happens in notebooks
- Added top 5 risks summary table for executive-level scanning
- Added tool timeout edge case — custom tools hanging is common
