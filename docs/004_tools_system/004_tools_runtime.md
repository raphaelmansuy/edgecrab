# Tools Runtime 🦀

> **Verified against:** `crates/edgecrab-tools/src/registry.rs` ·
> `crates/edgecrab-tools/src/process_table.rs` ·
> `crates/edgecrab-tools/src/tools/terminal.rs` ·
> `crates/edgecrab-tools/src/tools/backends/mod.rs`

---

## Why the runtime layer exists

A tool is more than a function call. A terminal command needs a working directory,
a backend (local shell? Docker? SSH tunnel?), a process table entry, and a
cancellation token. An image analysis tool needs a vision model provider
reference. A sub-agent delegation tool needs an agent runner factory.

The *tools runtime* is the set of shared objects and infrastructure that tools
operate against — distinct from the tool logic itself.

---

## `ToolContext` — the per-call environment

```rust
// Every tool receives this at execute() time
pub struct ToolContext {
    pub task_id:          String,         // unique per tool call
    pub cwd:              PathBuf,        // working directory for this call
    pub session_id:       String,
    pub user_task:        Option<String>, // original user message (for delegation)
    pub cancel:           CancellationToken,
    pub config:           AppConfigRef,   // read-only snapshot of AppConfig
    pub state_db:         Option<Arc<SessionDb>>,
    pub platform:         Platform,
    pub process_table:    Option<Arc<ProcessTable>>,
    pub provider:         Option<Arc<dyn LLMProvider>>,   // used by vision, image-gen
    pub tool_registry:    Option<Arc<ToolRegistry>>,       // used by MoA
    pub delegate_depth:   u32,            // max 2; blocks runaway recursion
    pub sub_agent_runner: Option<Arc<dyn SubAgentRunner>>,
    pub clarify_tx:       Option<UnboundedSender<ClarifyRequest>>,
    pub approval_tx:      Option<UnboundedSender<ApprovalRequest>>,
    pub on_skills_changed: Option<Arc<dyn Fn() + Send + Sync>>,
    pub gateway_sender:   Option<Arc<dyn GatewaySender>>,
    pub origin_chat:      Option<(String, String)>,
    pub session_key:      Option<String>,
    pub todo_store:       Option<Arc<TodoStore>>,
}
```

Most fields are `Option` because they are not available in all execution contexts
(tests, headless cron, ACP). Tools must handle `None` gracefully.

---

## `ProcessTable` — background process registry

Background processes (started via `run_process`) are tracked here for the
duration of an agent session:

```
  ┌──────────────────────────────────────────────────────────────┐
  │  ProcessTable                                                │
  │                                                              │
  │  DashMap<task_id, ProcessRecord>                             │
  │                                                              │
  │  ProcessRecord                                               │
  │    pid: u32                                                  │
  │    command: String                                           │
  │    cwd: PathBuf                                              │
  │    started_at: Instant                                       │
  │    status: ProcessStatus  (Running | Exited(code) | Killed)  │
  │    stdout_buf: Arc<Mutex<RingBuffer>>  (last N lines)        │
  │    stderr_buf: Arc<Mutex<RingBuffer>>                        │
  │    stdin_tx: Option<UnboundedSender<String>>                 │
  └──────────────────────────────────────────────────────────────┘
```

`DashMap` allows concurrent reads (e.g., `list_processes` while a tool is
appending output) without blocking. Inner mutexes protect per-record mutable
state.

A background GC task (cancelled when `Agent` drops) removes `Exited` and
`Killed` entries after a configurable TTL.

Tool interaction with `ProcessTable`:
```
  run_process  → insert new ProcessRecord
  list_processes → read all records
  get_process_output → read stdout_buf / stderr_buf
  write_stdin  → send to stdin_tx
  kill_process → send SIGTERM → update status
  wait_for_process → poll status with timeout
```

---

## Execution backends

The `terminal` and `execute_code` tools delegate actual shell execution to a
backend abstraction. This is what makes EdgeCrab Docker/SSH/Modal-aware
without the tool code knowing which environment it's in:

```
  BackendKind (from AgentConfig::terminal_backend)
        │
        ├── Local       → subprocess::Command with cwd
        │                 (default; no extra dependencies)
        │
        ├── Docker      → docker exec / docker run
        │                 DockerBackendConfig: image, mounts, env
        │
        ├── SSH         → ssh HostName command
        │                 SshBackendConfig: host, port, user, key_path
        │
        ├── Modal       → modal run (Python serverless sandbox)
        │                 ModalBackendConfig: app, stub, sandbox_path="/modal-sandbox"
        │
        ├── Daytona     → daytona workspace execute
        │                 DaytonaBackendConfig: workspace_id, server_url
        │
        └── Singularity → singularity exec image command
                          SingularityBackendConfig: image, bind_mounts
```

Backend configuration lives in `AppConfig::terminal_*` fields and is passed
into `ToolContext::config` at execution time.

---

## Backend selection and caching

```
  First terminal tool call in a session:
    read AgentConfig::terminal_backend  →  connect to backend
    store connection in per-session backend cache

  All subsequent terminal calls:
    retrieve cached backend connection
    execute command
    no reconnect overhead
```

The cache is keyed on `(session_id, BackendKind)`. Session isolation prevents
cross-session leakage.

---

## Security gates in the tool execution path

Before a tool's `execute()` runs, two gates may fire:

```
  ToolRegistry::dispatch()
        │
        ▼  Gate 1: command scan (terminal tool only)
  if tool_name == "terminal" || tool_name == "run_process":
    CommandScanner::scan(command)
    if is_dangerous && approval_mode != Off:
      send ApprovalRequest via approval_tx
      await user decision:
        Deny    → ToolError::PermissionDenied
        Once    → proceed once
        Session → add to session allowlist, proceed
        Always  → add to permanent allowlist, proceed

        ▼  Gate 2: path jail (file tools)
  if tool_name in ["read_file", "write_file", "patch"]:
    resolve_safe_path(path, jail_root)
    if path escapes jail:
      Err(AgentError::Security(...))
      → mapped to ToolError::PermissionDenied
```

---

## Output redaction

Sensitive values are redacted from tool outputs before they enter the conversation
history. This runs at the backend/runtime layer:

- API keys matching `sk-*`, `Bearer *` patterns
- Environment variable values from `ctx.config.security.redact_env_vars` list
- File content from paths in `ctx.config.security.redact_file_patterns`

The model never sees the raw secret values — only `[REDACTED]` placeholders.

---

## Approval flow (interactive mode)

```
  model: "Run: rm -rf ./build"
        │
        ▼
  CommandScanner detects: "rm -r" pattern → DangerCategory::DestructiveFileOps
        │
        ▼
  approval_tx.send(ApprovalRequest {
    command: "rm -rf ./build",
    full_command: "rm -rf ./build",
    reasons: ["matches destructive file operation pattern: rm -r"],
    response_tx: oneshot::Sender<ApprovalResponse>,
  })
        │
        ▼
  TUI renders:
    ┌─────────────────────────────────────────────────┐
    │ ⚠️  EdgeCrab wants to run a dangerous command    │
    │ rm -rf ./build                                   │
    │ Reason: destructive file operation               │
    │                                                  │
    │  [O]nce  [S]ession  [A]lways  [D]eny             │
    └─────────────────────────────────────────────────┘
        │
  user selects → response_tx.send(ApprovalResponse::Once)
        │
        ▼
  tool executes
```

Without a TUI (headless / gateway), `approval_tx` is `None`. Dangerous
commands **fail closed** — they return `ToolError::PermissionDenied` rather
than executing unguarded.

---

## Tips

> **Tip: `ctx.cancel.is_cancelled()` must be polled in loops.**
> Any tool that uses a loop (web crawl, process poller, directory walker) must
> check `ctx.cancel.is_cancelled()` at the top of each iteration. Ctrl-C will
> not stop the tool otherwise.

> **Tip: Use `BackendKind::Docker` for untrusted code execution.**
> `execute_code` defaults to Docker when available. The model's code runs in
> an isolated container with no access to the host filesystem by default.

> **Tip: `ToolContext::test_context()` in unit tests.**
> Provides a fully populated context with temporary paths and no-op channels.
> Avoid building `ToolContext` manually in tests.

---

## FAQ

**Q: How does the terminal tool handle interactive commands (e.g. `vim`)?**
It doesn't — interactive TTY commands are blocked or return immediately with
a timeout. Use `run_process` + `write_stdin` for programs that require input.

**Q: Can tools share state within a session?**
Yes, through `ToolContext` fields shared via `Arc`: `process_table`, `todo_store`,
`state_db`. A file written by `write_file` in iteration 3 is readable by
`read_file` in iteration 7 because they share the same filesystem.

**Q: What is `delegate_depth`?**
Recursive sub-agent invocations increment `delegate_depth`. When it reaches 2,
`delegate_task` returns a `CapabilityDenied` error with `suppression_key = "delegate:max_depth"`.
This prevents runaway sub-agent chains.

---

## Cross-references

- Execution backends configuration → [Config and State](../009_config_state/001_config_state.md)
- `CommandScanner` internals → [Security](../011_security/001_security.md)
- Backend environments (Docker, SSH, Modal) → [Execution Backends](../008_environments/001_environments.md)
- `ProcessTable` concurrency model → [Concurrency Model](../002_architecture/003_concurrency_model.md)
