# 沙盒系统 🦀

EdgeCrab 使用多层沙盒来确保安全执行。

## 沙盒层次

### 1. 进程沙盒

每个工具执行都在独立的子进程中运行：

```rust
pub struct SandboxedProcess {
    pid:            u32,
    stdin:          ChildStdin,
    stdout:         ChildStdout,
    stderr:         ChildStderr,
    kill_on_drop:   bool,
    timeout:        Option<Duration>,
}
```

### 2. 网络沙盒

默认情况下，工具进程没有网络访问权限：

```yaml
sandbox:
  network: blocked
  allowed_hosts: []
  allow_localhost: false
```

### 3. 文件系统沙盒

文件系统访问受到限制：

```yaml
sandbox:
  fs:
    read_only: true
    allowed_paths:
      - ~/.edgecrab
      - ./
    blocked_paths:
      - /etc
      - /home
```

### 4. 环境沙盒

环境变量被清理，只保留白名单：

```yaml
sandbox:
  env:
    allowed:
      - PATH
      - HOME
      - LANG
      - EDGECRAB_*
    blocked:
      - AWS_*
      - GCP_*
      - AZURE_*
```

## 沙盒配置

### 全局配置

```yaml
sandbox:
  enabled: true
  default_profile: strict
  profiles:
    strict:
      network: blocked
      fs:
        read_only: true
        allowed_paths: []
    relaxed:
      network: allowed
      fs:
        read_only: false
        allowed_paths:
          - ./
          - ~/.edgecrab
    development:
      network: allowed
      fs:
        read_only: false
        allowed_paths: []
```

### 工具特定配置

```toml
[tool.sandbox]
profile = "relaxed"
network = "allowed"
allowed_hosts = ["api.example.com"]
```

## 沙盒执行流程

```text
用户请求
    ↓
解析工具调用
    ↓
检查沙盒策略
    ↓
创建受限进程
    ↓
设置网络隔离
    ↓
设置文件系统限制
    ↓
设置环境变量白名单
    ↓
执行工具
    ↓
收集输出
    ↓
终止进程
    ↓
返回结果
```

## 安全特性

### 进程隔离

- 每个工具调用都在独立进程中运行
- 进程在完成后立即终止
- 使用 `kill_on_drop` 确保资源清理

### 网络控制

- 默认阻止所有网络访问
- 可以配置允许的主机列表
- 支持本地主机例外

### 文件系统保护

- 默认只读模式
- 可以配置允许的路径列表
- 支持工作目录限制

### 环境清理

- 清理敏感环境变量
- 只允许白名单中的变量
- 支持前缀匹配（如 `EDGECRAB_*`）

### 资源限制

- CPU 时间限制
- 内存限制
- 执行超时

## Linux 特定沙盒

在 Linux 上，EdgeCrab 使用更强大的沙盒机制：

```rust
use nix::sys::ptrace;
use nix::sys::signal;
use nix::sys::wait::waitpid;

pub struct LinuxSandbox {
    seccomp_filter: Option<SeccompFilter>,
    namespace_flags: CloneFlags,
}
```

### seccomp

使用 seccomp 过滤系统调用：

```rust
let filter = SeccompFilter::new(
    Action::KillProcess,
    vec![
        Syscall::open,
        Syscall::read,
        Syscall::write,
        Syscall::close,
        Syscall::exit,
    ],
);
```

### 命名空间

使用 Linux 命名空间隔离：

```rust
let flags = CloneFlags::CLONE_NEWUTS
    | CloneFlags::CLONE_NEWIPC
    | CloneFlags::CLONE_NEWPID
    | CloneFlags::CLONE_NEWNS;
```

## macOS 特定沙盒

在 macOS 上，EdgeCrab 使用 App Sandbox：

```xml
<dict>
    <key>com.apple.security.app-sandbox</key>
    <true/>
    <key>com.apple.security.network.client</key>
    <false/>
    <key>com.apple.security.files.user-selected.read-only</key>
    <true/>
</dict>
```

## Windows 特定沙盒

在 Windows 上，EdgeCrab 使用 Job Objects：

```rust
use winapi::um::jobapi2::CreateJobObjectW;
use winapi::um::jobapi2::SetInformationJobObject;
use winapi::um::winnt::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
```

## 验证

### 测试网络隔离

```bash
edgecrab tools call network_test
```

预期结果：网络访问被拒绝。

### 测试文件系统隔离

```bash
edgecrab tools call file_read /etc/passwd
```

预期结果：读取被拒绝。

### 测试环境清理

```bash
export SECRET_KEY=super_secret
edgecrab tools call env_dump
```

预期结果：`SECRET_KEY` 不在输出中。

## 性能考虑

沙盒增加了执行开销：

| 操作 | 开销 |
|------|------|
| 进程创建 | ~5ms |
| seccomp 设置 | ~1ms |
| 命名空间设置 | ~2ms |
| 环境清理 | <1ms |

### 优化策略

- 重用进程池（可选）
- 延迟沙盒设置
- 使用轻量级隔离

## 最佳实践

1. 始终使用默认的严格沙盒配置
2. 只为需要的工具放宽限制
3. 定期审查沙盒配置
4. 记录所有沙盒违规
5. 使用审计日志监控异常行为

## 未来改进

- WebAssembly 沙盒支持
- GPU 隔离
- 更细粒度的系统调用过滤
- 沙盒逃逸检测
- 实时监控和告警