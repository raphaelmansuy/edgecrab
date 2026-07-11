# 🦀 库选择

> **为什么：** 每个依赖都是信任关系、编译时成本和未来维护负担。这里列出的库被选中是因为它们提供了数千行正确 Rust 代码才能复制的功能 — 并且因为它们稳定、广泛使用且积极维护。

**来源：** workspace `Cargo.toml` 和各个 crate manifests。

此页面仅涵盖实质性塑造架构的库。传递依赖和次要工具被省略。

---

## 运行时和异步

### `tokio` — 异步基础

```toml
tokio = { version = "1", features = ["full"] }
```

**为什么：** 唯一生产级别的 Rust 异步运行时，具有工作窃取、`io-uring` 支持和丰富的生态系统。EdgeCrab 在各处使用多线程运行时：CLI、网关服务器、工具执行、MCP 客户端、cron 调度器。

**塑造架构：** EdgeCrab 中的每个 `async fn` 都针对 Tokio 的执行器模型编译。`edgecrab-cli` 中的 `#[tokio::main]` 入口点启动多线程调度器。

- 参考：[tokio.rs](https://tokio.rs) | [docs.rs/tokio](https://docs.rs/tokio)

### `tokio-util` — 取消和辅助程序

```toml
tokio-util = { version = "0.7", features = ["rt"] }
```

来自 `tokio-util` 的 `CancellationToken` 是智能体循环、网关处理器和工具执行中贯穿使用的协作取消原语。参见 [并发模型](../002_architecture/003_concurrency_model.md)。

### `futures` — 流和组合符工具

用于 LLM 输出流（`Stream<Item = StreamEvent>`）、异步迭代和组合符链。`futures::stream::StreamExt` trait 在网关和工具层无处不在。

---

## 序列化和配置

### `serde` + `serde_json` + `serde_yml`

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yml = "0.0.12"
```

**为什么：** `serde` 是事实上的 Rust 序列化标准。`edgecrab-types` 中的每个公共类型派生 `Serialize`/`Deserialize`。`serde_json` 处理 LLM API 负载和工具参数。`serde_yml` 处理 `config.yaml` 和技能 frontmatter。

- 参考：[serde.rs](https://serde.rs)

---

## 智能体和提供商层

### `edgequake-llm` — 提供商抽象

内部提供商 crate 在单个异步接口后面封装 OpenAI ChatCompletions、Anthropic Messages 和 Codex Responses API。EdgeCrab 调用 `edgequake-llm`；`edgequake-llm` 处理认证、重试、API 模式转换和流式传输。这使得多提供商支持成为可能，而无需在核心运行时中分散 `if anthropic { … } else { … }` 分支。

---

## 持久化和搜索

### `rusqlite` — 嵌入式 SQLite

```toml
rusqlite = { version = "0.31", features = ["bundled", "bundled-full"] }
```

**为什么：** 零外部依赖。`bundled` 功能将 SQLite 直接编译到二进制文件中 — 不需要主机上的 `libsqlite3`。`bundled-full` 添加 FTS5 支持以进行对话历史的全文搜索。

塑造架构：`edgecrab-state` crate 拥有所有数据库访问。WAL 模式 + 抖动重试写入使其对同一文件上的并发 CLI 和网关使用安全。

- 参考：[docs.rs/rusqlite](https://docs.rs/rusqlite)

---

## CLI 和 TUI

### `clap` — 参数解析

```toml
clap = { version = "4", features = ["derive"] }
```

EdgeCrab 的整个子命令树（`run`、`chat`、`gateway`、`version`、`skills`、`memory`...）使用 clap 的派生宏声明。完成、帮助文本和验证免费获得。

- 参考：[docs.rs/clap](https://docs.rs/clap)

### `ratatui` + `crossterm` — 终端 UI

```toml
ratatui = "0.27"
crossterm = "0.27"
```

交互式 TUI 模式（聊天面板、工具输出面板、状态栏）建立在 `ratatui` 之上。`crossterm` 提供跨平台终端后端（原始模式、事件循环、ANSI 序列）。

- 参考：[ratatui.rs](https://ratatui.rs)

### `tui-textarea` — 多行输入

提供 TUI 聊天模式中使用的多行文本输入小部件。处理 Unicode、多字节字符和 vim 风格的键绑定。

---

## HTTP 和服务器

### `reqwest` — HTTP 客户端

```toml
reqwest = { version = "0.12", features = ["json", "stream"] }
```

用于所有出站 HTTP：LLM 提供商调用（通过 `edgequake-llm`）、URL 获取工具、网络搜索工具。`stream` 功能启用大响应的异步流式传输。

### `axum` — HTTP 服务器

```toml
axum = "0.7"
```

为 ACP 服务器（`edgecrab-acp`）和 webhook/API-server 网关适配器提供动力。因其 Tokio 原生设计、tower 中间件兼容性和人体工学路由器 API 而被选中。

- 参考：[docs.rs/axum](https://docs.rs/axum)

### `tokio-tungstenite` — WebSocket

网关适配器（Discord gateway、Slack RTM、Matrix C-S API）和 ACP 流协议的 WebSocket 支持。

---

## 工具注册

### `inventory` — 编译时插件注册

```toml
inventory = "0.3"
```

**为什么这在架构上很重要：** `inventory` 使用链接器节将所有 crate 中的 `inventory::submit!` 项收集到一个全局注册表中 — 没有任何中央列表。每个工具自行注册：

```rust
inventory::submit! { &ReadFileTool as &dyn ToolHandler }
```

启动时，`ToolRegistry::collect()` 遍历所有提交的项目。添加新工具需要在工具自身文件之外进行零更改。这就是 EdgeCrab 如何在没有单体分发表的情况下达到 91 个核心工具的原因。

- 参考：[docs.rs/inventory](https://docs.rs/inventory) | [dtolnay/inventory](https://github.com/dtolnay/inventory)

---

## 并发工具

### `dashmap` — 并发哈希映射

```toml
dashmap = "6"
```

`DashMap` 提供细粒度锁的分片并发 `HashMap`。用于进程表（运行中的工具子进程）、MCP 客户端注册表和其他热路径并发映射。消除 `Mutex<HashMap<…>>` 反模式。

- 参考：[docs.rs/dashmap](https://docs.rs/dashmap)

---

## 执行后端

### `bollard` — Docker API 客户端

```toml
bollard = "0.17"
```

Docker 执行后端通过 `bollard` 与 Docker daemon 通信。用于创建工具执行的临时容器、挂载工作区并将 stdout/stderr 流式传输回智能体。

- 参考：[docs.rs/bollard](https://docs.rs/bollard)

### `openssh` — SSH 客户端（仅 Unix）

```toml
[target.'cfg(unix)'.dependencies]
openssh = "0.10"
```

SSH 执行后端使用 `openssh` 将工具执行转发到远程机器。限制为 `cfg(unix)` — Windows 构建不可用。

---

## 安全和文本处理

### `regex` + `aho-corasick` — 模式匹配

```toml
regex = "1"
aho-corasick = "1"
```

`aho-corasick` 是 `CommandScanner` 中的快速多模式引擎 — 无论模式数量如何，O(n) 输入长度。`regex` 处理上下文敏感的二次扫描和脱敏模式匹配。两者都在 `edgecrab-security` 中使用。

- 参考：[docs.rs/aho-corasick](https://docs.rs/aho-corasick) | [Aho-Corasick algorithm](https://en.wikipedia.org/wiki/Aho%E2%80%93Corasick_algorithm)

### `unicode-normalization` — NFC 规范化

注入检测模块需要它来在模式匹配之前规范化 Unicode。没有 NFC 规范化，同形字和分解字符注入攻击会绕过字符串相等检查。

### `strip-ansi-escapes` — 清理终端输出

在不支持颜色的上下文（日志、非 TUI 网关适配器）中存储或显示 shell 命令输出之前，剥离 ANSI 颜色/格式转义序列。

### `secrecy` — 秘密清零

```toml
secrecy = "0.8"
```

用具有 `Drop` 实现的 `Secret<String>` 包装敏感值，该实现在释放时将内存清零。用于会话期间保存在内存中的 API 密钥和凭据。

- 参考：[docs.rs/secrecy](https://docs.rs/secrecy)

---

## 库选择原则

| 原则 | 应用 |
|---|---|
| **捆绑而非系统** | `rusqlite bundled` — 不需要主机库 |
| **Tokio 原生** | `axum`、`reqwest`、`tokio-tungstenite` — 无阻塞线程 I/O |
| **零成本编译时** | `inventory`、`serde derive` — 无运行时反射 |
| **cfg 门控平台库** | `openssh` 仅在 Unix 上 — 干净的 Windows 构建 |
| **安全意识** | `secrecy` 用于凭据，`aho-corasick` 用于快速模式匹配 |

---

## 提示

- **不要为一个函数添加 crate —** 如果需要一个算法，请实现它。`inventory` 和 `dashmap` 是承重的；JSON 美化打印器不是。
- **在添加操作系统特定依赖之前检查 `cfg(unix)` —** `bollard` 在 Linux/macOS/Windows 上工作（Docker for Windows）；`openssh` 不行。遵循现有模式。
- **`serde_yml` 而不是 `serde_yaml` —** 工作区使用 `serde_yml` fork（维护中，没有 `unsafe` YAML 解析器）。不要引入旧的 `serde_yaml` crate。

---

## 交叉引用

- `inventory` 注册详情 → [`002_architecture/002_crate_dependency_graph.md`](../002_architecture/002_crate_dependency_graph.md)
- `DashMap` 在并发模型中 → [`002_architecture/003_concurrency_model.md`](../002_architecture/003_concurrency_model.md)
- `CancellationToken` 使用 → [`002_architecture/003_concurrency_model.md`](../002_architecture/003_concurrency_model.md)
- `rusqlite` WAL 详情 → [`009_config_state/002_session_storage.md`](../009_config_state/002_session_storage.md)
- 安全原语（`aho-corasick`、`regex`）→ [`011_security/001_security.md`](../011_security/001_security.md)
