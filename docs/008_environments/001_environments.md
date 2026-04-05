# Execution Backends 🦀

> **Verified against:** `crates/edgecrab-tools/src/tools/backends/mod.rs` ·
> `crates/edgecrab-tools/src/tools/terminal.rs` ·
> `crates/edgecrab-tools/src/tools/process.rs`

---

## Why multiple backends exist

The `terminal`, `run_process`, and `execute_code` tools need to run shell
commands somewhere. "Somewhere" is not always the localhost:

- A security-conscious deployment wants isolated Docker containers
- A company workstation runs code in a remote dev environment via SSH
- A cloud agent uses Modal serverless sandboxes
- A research workflow needs Apptainer-isolated containers

The backend abstraction lets tool code remain identical regardless of where
the commands actually execute.

🦀 *`hermes-agent` (Python) defaulted to local execution only. OpenClaw supports
optional Docker sandboxing for tool isolation. EdgeCrab ships six execution backends —
local, Docker, SSH, Modal, Daytona, and Singularity — selectable per session.*

---

## Backend kinds

```rust
// AgentConfig::terminal_backend (BackendKind)
pub enum BackendKind {
    Local,
    Docker,
    Ssh,
    Modal,
    Daytona,
    Singularity,
}
```

---

## Backend comparison

```
  ┌────────────────┬────────────┬──────────────┬─────────────┬──────────────┐
  │ Backend        │ Isolation  │ Dependency   │ Persistent  │ Best for     │
  │                │ level      │ required     │ sessions    │              │
  ├────────────────┼────────────┼──────────────┼─────────────┼──────────────┤
  │ local          │ none       │ none         │ yes         │ dev, scripting│
  │ docker         │ container  │ Docker daemon│ per-run     │ code exec, CI │
  │ ssh            │ remote host│ SSH server   │ yes         │ remote dev   │
  │ modal          │ serverless │ Modal CLI    │ no          │ cloud sandbox │
  │ daytona        │ workspace  │ Daytona      │ yes         │ cloud dev env │
  │ singularity    │ container  │ Apptainer    │ per-run     │ HPC clusters  │
  └────────────────┴────────────┴──────────────┴─────────────┴──────────────┘
```

---

## Local backend (default)

The default. Commands run as subprocesses in the configured `cwd`:

```
  terminal tool call
    command: "cargo test --workspace"
    cwd: /Users/me/edgecrab
        │
        ▼
  std::process::Command::new("sh")
    .arg("-c").arg(command)
    .current_dir(cwd)
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
        │
        ▼
  stdout + stderr collected
  exit code checked
  output truncated + redacted
  returned to model
```

**Environment passthrough:** `AgentConfig::terminal_env_passthrough` controls
which environment variables propagate into tool subprocesses. Default: `PATH`,
`HOME`, `USER`, plus any explicitly listed vars.

**Persistent shell sessions:** The local backend reuses a shell process across
sequential tool calls in a session. `cd` in one `terminal` call is visible in
the next.

---

## Docker backend

```yaml
# ~/.edgecrab/config.yaml
terminal:
  backend: docker
  docker:
    image: "ubuntu:22.04"
    mounts:
      - host: /Users/me/project     # bind-mount project into container
        container: /workspace
    env:
      - CARGO_HOME=/workspace/.cargo
    working_dir: /workspace
```

Architecture:
```
  terminal tool call
        │
        ▼
  bollard::exec::CreateExecOptions {
    cmd: ["sh", "-c", command],
    working_dir: ...,
    env: [...],
    attach_stdout: true, attach_stderr: true,
  }
        │
        ▼
  docker exec into running container (or docker run for one-shot)
        │
        ▼
  stream stdout + stderr
  collect to string
  return
```

**Reference:** [`bollard` Docker API crate](https://docs.rs/bollard)

---

## SSH backend

```yaml
terminal:
  backend: ssh
  ssh:
    host: dev.mycompany.com
    port: 22
    user: raphaelmansuy
    key_path: ~/.ssh/id_ed25519
    working_dir: /home/raphaelmansuy/projects
```

```
  terminal tool call
        │
        ▼
  openssh::Session::connect(host, port, user)
  openssh::Session::command(["sh", "-c", command])
        │
        ▼
  stdout + stderr collected over SSH
  session reused within the agent session (no reconnect per call)
```

**Reference:** [`openssh` crate](https://docs.rs/openssh) (Unix only)

---

## Modal backend

```yaml
terminal:
  backend: modal
  modal:
    app: my-app
    stub: my-stub
    sandbox_path: /modal-sandbox    # fixed mount path inside Modal
```

Modal runs each command as a serverless Modal Function invocation. There is
no persistent shell — each `terminal` call is a fresh sandbox invocation.

---

## Daytona backend

```yaml
terminal:
  backend: daytona
  daytona:
    workspace_id: ws-abc123
    server_url: https://api.daytona.io
```

Daytona is a cloud dev environment service. Commands execute inside the
named Daytona workspace.

---

## Singularity backend

```yaml
terminal:
  backend: singularity
  singularity:
    image: /path/to/my.sif
    bind_mounts:
      - /data:/data:ro
      - /tmp/output:/output:rw
```

Used on HPC clusters where Docker is not available. Uses
[Apptainer/Singularity](https://apptainer.org) container format.

---

## Shared backend behaviour

Regardless of backend, these guarantees hold:

| Behaviour | Description |
|---|---|
| Cancellation | Each backend receives a `CancellationToken` and terminates on signal |
| Output truncation | Long output is truncated to a configurable max before entering the model context |
| Output redaction | API keys, secrets, and configured patterns are redacted |
| Exit code handling | Non-zero exit codes become error responses, not panics |
| Background processes | `run_process` starts processes tracked in `ProcessTable`; `kill_process` terminates them |

---

## Configuring the backend

```yaml
# ~/.edgecrab/config.yaml
terminal:
  backend: docker           # local | docker | ssh | modal | daytona | singularity

  # Per-backend config sections:
  docker:
    image: ubuntu:22.04
  ssh:
    host: myserver.example.com
  modal:
    app: my-app
  daytona:
    workspace_id: ws-abc
  singularity:
    image: /path/to/my.sif
```

Or via environment variable:
```sh
EDGECRAB_TERMINAL_BACKEND=docker edgecrab "run the integration tests"
```

---

## Tips

> **Tip: Use Docker backend for `execute_code` when running untrusted code.**
> The default `execute_code` tries Docker first. If Docker is running, code
> executes in an ephemeral container with no access to the host filesystem
> except explicit bind mounts.

> **Tip: SSH backend sessions are persistent within a session.**
> The SSH connection is established once and reused. `cd` in one shell call
> persists to the next. Background processes started via `run_process` over SSH
> are tracked in `ProcessTable` by PID.

> **Tip: Set `terminal.env_passthrough` to control secret leakage.**
> By default only `PATH` and `HOME` propagate. If tools need an API key,
> add it explicitly rather than passing all environment variables.

---

## FAQ

**Q: Can I switch backends mid-session?**
The backend is configured at `AgentConfig` level and read at session start.
Changing `config.yaml` and restarting is the only supported method.

**Q: What if Docker is not running?**
`execute_code` falls back to local execution with a sandbox warning. `terminal`
with `backend: docker` returns a `ToolError::Unavailable` if Docker is
unreachable.

**Q: Is there a way to run each tool call in a fresh container?**
Docker one-shot mode creates a new container per command (no persistent shell).
Set `docker.persistent_shell: false` in config to enable this mode.

---

## Cross-references

- `ToolContext` and backend references → [Tools Runtime](../004_tools_system/004_tools_runtime.md)
- Config fields for backends → [Config and State](../009_config_state/001_config_state.md)
- Security gate before backend execution → [Security](../011_security/001_security.md)
