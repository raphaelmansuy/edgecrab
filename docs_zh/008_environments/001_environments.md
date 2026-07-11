# 执行后端 🦀

> **已验证来源：** `crates/edgecrab-tools/src/tools/backends/mod.rs` ·
> `crates/edgecrab-tools/src/tools/terminal.rs` ·
> `crates/edgecrab-tools/src/tools/process.rs`

---

## 为什么存在多个后端

`terminal`、`run_process` 和 `execute_code` 工具需要在某处运行 shell 命令。"某处"并不总是本地主机：

- 注重安全的部署希望使用隔离的 Docker 容器
- 公司工作站通过 SSH 在远程开发环境中运行代码
- 云智能体使用 Modal 无服务器沙盒
- 研究工作流需要 Apptainer 隔离容器

后端抽象使工具代码保持相同，无论命令实际在哪里执行。

🦀 *`hermes-agent`（Python）默认仅本地执行。OpenClaw 支持可选的 Docker 沙箱用于工具隔离。EdgeCrab 提供六个执行后端 — local、Docker、SSH、Modal、Daytona 和 Singularity — 可按会话选择。*

---

## 后端类型

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

## 后端比较

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

## 本地后端（默认）

默认配置。命令在配置的 `cwd` 中作为子进程运行：

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

**环境变量传递：** `AgentConfig::terminal_env_passthrough` 控制哪些环境变量传播到工具子进程中。默认：`PATH`、`HOME`、`USER`，加上任何明确列出的变量。

**持久化 shell 会话：** 本地后端在会话中的连续工具调用之间重用 shell 进程。一次 `terminal` 调用中的 `cd` 在下一次可见。

---

## Docker 后端

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

架构：
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

**参考：** [`bollard` Docker API crate](https://docs.rs/bollard)

---

## SSH 后端

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

**参考：** [`openssh` crate](https://docs.rs/openssh) (Unix only)

---

## Modal 后端

```yaml
terminal:
  backend: modal
  modal:
    app: my-app
    stub: my-stub
    sandbox_path: /modal-sandbox    # fixed mount path inside Modal
```

Modal 将每个命令作为无服务器 Modal Function 调用运行。没有持久化 shell — 每次 `terminal` 调用都是新的沙盒调用。

---

## Daytona 后端

```yaml
terminal:
  backend: daytona
  daytona:
    workspace_id: ws-abc123
    server_url: https://api.daytona.io
```

Daytona 是一个云开发环境服务。命令在命名的 Daytona 工作区中执行。

---

## Singularity 后端

```yaml
terminal:
  backend: singularity
  singularity:
    image: /path/to/my.sif
    bind_mounts:
      - /data:/data:ro
      - /tmp/output:/output:rw
```

用于 Docker 不可用的 HPC 集群。使用 [Apptainer/Singularity](https://apptainer.org) 容器格式。

---

## 共享后端行为

无论后端如何，这些保证都成立：

| 行为 | 描述 |
|---|---|
| 取消 | 每个后端接收一个 `CancellationToken` 并在信号时终止 |
| 输出截断 | 长输出在进入模型上下文之前被截断为可配置的最大值 |
| 输出脱敏 | API 密钥、秘密和配置的模式会被脱敏 |
| 退出码处理 | 非零退出码变为错误响应，而不是 panic |
| 后台进程 | `run_process` 启动的进程在 `ProcessTable` 中跟踪；`kill_process` 终止它们 |

---

## 配置后端

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

或通过环境变量：
```sh
EDGECRAB_TERMINAL_BACKEND=docker edgecrab "run the integration tests"
```

---

## 提示

> **提示：对不受信任的代码使用 `execute_code` 时使用 Docker 后端。**
> 默认的 `execute_code` 首先尝试 Docker。如果 Docker 正在运行，代码
> 在临时容器中执行，除了显式绑定挂载外无法访问主机文件系统。

> **提示：SSH 后端会话在会话内是持久的。**
> SSH 连接建立一次并重用。一次 shell 调用中的 `cd` 持续到下一次。通过 SSH 使用 `run_process` 启动的后台进程由 PID 在 `ProcessTable` 中跟踪。

> **提示：设置 `terminal.env_passthrough` 以控制秘密泄露。**
> 默认情况下只有 `PATH` 和 `HOME` 传播。如果工具需要 API 密钥，请显式添加它，而不是传递所有环境变量。

---

## 常见问题

**问：我可以在会话中途切换后端吗？**
后端在 `AgentConfig` 级别配置并在会话开始时读取。更改 `config.yaml` 并重启是唯一的支持方法。

**问：如果 Docker 未运行会怎样？**
`execute_code` 回退到本地执行并发出沙箱警告。如果 Docker 不可达，`terminal` 带有 `backend: docker` 返回 `ToolError::Unavailable`。

**问：有没有办法让每次工具调用都在新容器中运行？**
Docker 一次性模式为每个命令创建新容器（无持久化 shell）。在配置中设置 `docker.persistent_shell: false` 以启用此模式。

---

## 交叉引用

- `ToolContext` 和后端引用 → [工具运行时](../004_tools_system/004_tools_runtime.md)
- 后端的配置字段 → [配置和状态](../009_config_state/001_config_state.md)
- 后端执行前的安全门控 → [安全](../011_security/001_security.md)
