# 🦀 安全模型

> **为什么：** 能够运行 shell 命令、读取文件和获取 URL 的 AI 智能体是提示注入、路径遍历、SSRF 和秘密泄露的诱人目标。EdgeCrab 不是在 91 个核心工具中分散临时检查，而是将所有安全原语集中在 `edgecrab-security` — 一个每个工具在实际工作之前调用的单一 crate。

**来源：** `crates/edgecrab-security/src/`

---

## 威胁地图

| 威胁 | 模块 | 防护 |
|---|---|---|
| 路径遍历 (`../../etc/passwd`) | `path_jail` | 规范化并检查前缀 |
| 本地网络 SSRF (`http://192.168.x.x`) | `url_safety` | 阻止 RFC-1918 和环回 |
| 危险 shell 命令 (`rm -rf /`) | `command_scan` | Aho-Corasick + regex |
| 提示注入（隐藏 Unicode、指令） | `injection` | Unicode 规范化 + 模式检查 |
| 输出中的秘密泄露 | `redact` | 显示前的模式匹配脱敏 |
| 无限制的危险操作 | `approval` | 明确的用户确认门控 |
| 输入规范化边缘情况 | `normalize` | NFC + 剥离不可见字符 |
| 每路径权限策略 | `path_policy` | 路径前缀的允许/拒绝列表 |

---

## 模块描述

### `approval` — 显式风险门控

在工具执行高风险操作（shell 命令、项目外的文件写入、URL 获取）之前，它调用批准模块。批准模式在 `AppConfig::security.approval_mode` 中配置：

```
┌──────────────────────────────────────┐
│            approval_mode             │
├───────────┬───────────────┬──────────┤
│  "never"  │  "on_risk"    │ "always" │
│           │   (default)   │          │
│  skip     │  check risk   │  always  │
│  approval │  score; ask   │   ask    │
│           │  if risky     │          │
└───────────┴───────────────┴──────────┘
```

用户返回的 `ApprovalChoice`：

```rust
pub enum ApprovalChoice {
    Allow,          // run once
    AllowAlways,    // add to permanent allow list
    Deny,           // block this call
    DenyAlways,     // add to permanent deny list
}
```

---

### `command_scan` — Shell 命令安全

`CommandScanner` 使用 Aho-Corasick 多模式匹配已知危险模式（快速，O(n) 输入长度），然后应用 regex 二次扫描以进行上下文敏感模式：

```
raw shell command
      │
      ▼
┌─────────────────────┐
│   Aho-Corasick      │  first pass — O(n), pattern set compiled once
│   multi-pattern     │  matches: "rm -rf", ":(){ :|:& };:", "dd if=/dev/zero"…
└─────────┬───────────┘
          │ suspicious? → secondary scan
          ▼
┌─────────────────────┐
│   regex checks      │  context-sensitive: pipe chains, sudo escalation,
│                     │  network exfil patterns, /dev writes…
└─────────┬───────────┘
          │
          ▼
    RiskScore { level, reason }
          │
          ▼
    ┌─────┴──────┐
  safe       risky → approval gate
```

---

### `injection` — 提示注入检测

隐藏的 Unicode 和带外指令是 LLM 智能体的主要提示注入向量。`injection` 模块：

1. 将输入规范化为 NFC（捕获分解的不可见字符）
2. 剥离零宽连接符、RTLO/LTRO 覆盖字符、软连字符
3. 检查已知注入指令片段（`ignore previous instructions`, `disregard`, `system:`, `[INST]`...）
4. 返回 `InjectionRisk { detected: bool, reason: Option<String> }`

```rust
// Example call inside a tool handler
let risk = check_injection(&user_provided_filename)?;
if risk.detected {
    return Err(ToolError::安全Violation(risk.reason.unwrap_or_default()));
}
```

---

### `path_jail` — 系统隔离

```
requested path: "/home/user/project/../../etc/passwd"
      │
      ▼
canonicalise (resolve symlinks + .. segments)
      │
      ▼
"/etc/passwd"
      │
      ▼
check: does canonical path start with any allowed root?
  allowed roots: ["/home/user/project", "/tmp/edgecrab-*"]
      │
      ▼
NO → PathTraversalError
YES → proceed
```

允许的根目录在 `AppConfig::security` 中配置，并通过 `path_policy` 模块按会话扩展。

---

### `url_safety` — SSRF 预防

```rust
// Blocked address classes
- 127.0.0.0/8      (loopback)
- 10.0.0.0/8       (RFC-1918 private)
- 172.16.0.0/12    (RFC-1918 private)
- 192.168.0.0/16   (RFC-1918 private)
- 169.254.0.0/16   (link-local / AWS metadata endpoint)
- ::1              (IPv6 loopback)
- fd00::/8         (IPv6 ULA)
- file:// scheme
- unconventional ports (blocked list)
```

DNS 重绑定通过在发送请求之前解析主机名并检查解析后的地址与相同的阻止列表来缓解。

---

### `redact` — 输出净化

`redact` 在每个离开工具层回到模型或用户的字符串上运行。它针对以下内容进行模式匹配：

- AWS 密钥模式（`AKIA[A-Z0-9]{16}`）
- GitHub token（`ghp_`, `ghs_`, `github_pat_`）
- 环境变量中的通用高熵字符串
- 来自 `AppConfig::privacy.redact_patterns` 的自定义模式

匹配的秘密在显示或存储之前被替换为 `[REDACTED]`。

---

## 深度防御栈

```
model sends tool call
        │
        ▼
┌───────────────────┐
│  normalize input  │  NFC, strip invisible chars
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│  injection check  │  hidden Unicode, instruction fragments
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│  path / URL check │  traversal, SSRF, blocked schemes
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│  command scan     │  dangerous patterns (shell tools only)
└─────────┬─────────┘
          │
          ▼
┌───────────────────┐
│  approval gate    │  user confirmation for risky ops
└─────────┬─────────┘
          │
          ▼
     tool executes
          │
          ▼
┌───────────────────┐
│  redact output    │  secrets removed before model sees result
└─────────┬─────────┘
          │
          ▼
   result returned
```

---

## 代码质量约束

```rust
// crates/edgecrab-security/src/lib.rs
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
```

对意外输入 panic 的安全代码比返回错误的安全代码更糟。`edgecrab-security` 中的每个函数都返回 `Result`；panic 是编译时错误。

---

## 编写新工具：安全清单

如果你的工具触及以下任何一项，请使用相应的原语：

| 触点 | 调用的原语 |
|---|---|
| 系统路径 | `path_jail::check_path(path, &allowed_roots)` |
| URL/HTTP 请求 | `url_safety::check_url(url)` |
| Shell 命令 | `command_scan::scan(command)` |
| 注入到提示中的用户提供文本 | `injection::check_injection(text)` |
| 包含环境变量/凭据的输出 | `redact::redact(output)` |
| 任何高风险操作 | `approval::request(ctx, description)` |

---

## 提示

- **在 crate 根重新导出 —** `edgecrab-security/src/lib.rs` 重新导出最常见的函数。`use edgecrab_security::check_path` 在大多数工具中就足够了。
- **`#![deny(clippy::unwrap_used)]` 是你的朋友 —** 也将其应用于你自己的工具 crate。它在调用站点强制显式错误处理。
- **不要实现自己的注入检测 —** 字符级 Unicode 技巧很微妙。即使对于"简单"文本输入，也要使用 `injection` 模块。

---

## 常见问题

**问：EdgeCrab 在操作系统级别沙箱化工具执行吗？**
答：对于本地执行，默认情况下不应用内核沙箱。安全层是应用程序级别的。Docker 和 Singularity 后端提供操作系统级隔离 — 参见 [`008_environments/001_environments.md`](../008_environments/001_environments.md)。

**问：我可以添加自定义脱敏模式吗？**
答：可以。在 `config.yaml` 中的 `AppConfig::privacy.redact_patterns` 添加正则表达式模式。它们在启动时编译并与内置模式一起应用。

**问：如果 `command_scan` 对合法命令发出风险怎么办？**
答：批准门控触发（`on_risk` 模式）并向用户提示。`AllowAlways` 将其添加到该配置文件的永久允许列表。

---

## 交叉引用

- 运行时中的批准流程 → [`004_tools_system/004_tools_runtime.md`](../004_tools_system/004_tools_runtime.md)
- 执行后端（操作系统级隔离）→ [`008_environments/001_environments.md`](../008_environments/001_environments.md)
- `approval_mode` 和 `redact_patterns` 的配置 → [`009_config_state/001_config_state.md`](../009_config_state/001_config_state.md)
- 工具注册表（调用检查的地方）→ [`004_tools_system/001_tool_registry.md`](../004_tools_system/001_tool_registry.md)
