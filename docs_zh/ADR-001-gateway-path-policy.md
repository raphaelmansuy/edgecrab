# ADR-001: 网关路径策略 — 容忍不存在的可信根目录

**状态**: 已接受
**日期**: 2026-04-13
**决策者**: EdgeCrab 核心团队
**技术领域**: edgecrab-security / edgecrab-tools / edgecrab-gateway

---

## 背景

### 观察到的故障

当 EdgeCrab 网关实例接收到图像（例如通过 WhatsApp）并且代理对下载的文件调用 `vision_analyze` 时，工具立即失败，错误如下：

```
vision_analyze failed in 0.0s: Execution failed in vision_analyze:
Cannot resolve allowed root '/Users/user/.edgecrab/images': No such file or directory
```

之前在 `pdf_to_markdown` 测试中也出现过同类故障（在 0.4.0 中通过在调用点使用 `.exists()` 保护额外根目录修复）。

### 根本原因（第一性原理）

`PathPolicy::canonical_allowed_roots` 对**每个**根目录调用 `std::fs::canonicalize()` — 工作区根目录、虚拟临时根目录、配置的允许根目录，以及调用者提供的 *extra_roots*。当目标路径不存在时，`canonicalize` 返回 OS 错误 2。

`vision_analyze` 传递的三个可选"额外"根目录是：
- `~/.edgecrab/images/`         — TUI 剪贴板图像
- `~/.edgecrab/image_cache/`    — WhatsApp Baileys 桥缓存
- `~/.edgecrab/gateway_media/`  — Rust 原生网关适配器（Telegram、Discord…）

这些目录是**延迟创建**的：`ensure_edgecrab_home()` 不会创建它们；它们仅在下载第一张图像时才出现。在新安装时，或在任何网关图像到达之前，它们不存在。

### Hermes 方案（对比）

Hermes (`vision_tools.py`) **不**应用路径限制。它直接读取本地文件，信任 LLM 选择了合理的路径。这是以安全性换取简单性。

EdgeCrab 的路径限制方案是正确且更安全的。问题不在于限制的存在 — 而在于规范化步骤的僵化，它将"根目录不存在"视为不可恢复的错误，而不是"根目录不包含任何文件，因此跳过它"。

### 网关 vs CLI 隔离

| 方面 | CLI | 网关 |
|---|---|---|
| 工作目录 | 用户 CWD | 启动时的 `std::env::current_dir()` |
| 允许的文件访问 | 工作区 + 可选根目录 | 相同策略，更宽的可选根目录 |
| 图像来源 | 本地剪贴板保存 | 平台下载到 `gateway_media/` |
| 典型可选根目录 | `~/.edgecrab/images/` | + `image_cache/`, `gateway_media/` |

Hermes 通过使用 `MESSAGING_CWD`（默认为 `PATH.home()`）作为工作目录来解决此问题，为网关提供广泛的隐式访问权限。EdgeCrab 的设计默认更加严格，但必须容忍尚未创建的可选根目录。

---

## 决策

### 主要修复：在 `canonical_allowed_roots` 中容忍不存在的可选根目录

修改 `path_policy.rs` 以根据根目录类别应用不同的处理方式：

| 类别 | 不存在时的处理 | 理由 |
|---|---|---|
| `workspace_root` | 失败 (`InvalidRoot`) | 必需的不变量 — 代理的 CWD 必须存在 |
| `virtual_tmp_root` | 已预先规范化；不适用 | 调用者确保它存在 |
| `self.allowed_roots` (配置) | 记录 `warn!`，跳过 | 用户配置错误；不要使工具崩溃 |
| `extra_roots` (调用者提供) | 记录 `debug!`，跳过 | 延迟创建的可选目录；安全地省略 |

**安全不变量保持**: 不存在的目录不包含任何文件。跳过它意味着"此根目录下没有路径被信任" — 这恰好是正确的。调用者无法使用不存在的可信根目录来访问本不存在的文件。

### 为什么不"在启动时创建目录"

将 `gateway_media_dir` 和 `image_cache_dir` 添加到 `ensure_edgecrab_home()` 会创建仅在网关运行时有意义的空目录。仅使用 CLI 的用户会得到不必要的冗余。延迟创建是有意的设计。

### 为什么不"在每个调用点修复"

这在 `pdf_to_markdown`（v0.4.0）中作为临时解决方案完成了。它不可扩展 — 任何传递可选额外根目录的未来工具都需要相同的样板保护。在安全层修复根本原因使所有当前和未来的调用者都受益。

### 为什么不"镜像 Hermes 并在网关模式下移除路径限制"

路径限制是有意义的安全边界。在网关模式下，代理通过消息平台接收来自远程用户的指令。没有路径限制，恶意网关用户可以提示代理读取 `/etc/passwd`、SSH 私钥或超出预期范围的其他文件。Hermes 为了简单性接受此风险；EdgeCrab 不接受。

---

## 后果

### 正面

- `vision_analyze` 在首次运行时在所有网关上下文中正常工作
- `pdf_to_markdown` 可以简化（调用点的 `.exists()` 保护可以保留作为纵深防御，但不再是必需的）
- 通过 `jail_read_path_multi` 传递可选额外根目录的所有未来工具自动受益
- CLI 行为不变（其额外根目录也延迟创建，并受益于相同的优雅处理）

### 负面 / 权衡

- 配置中错误配置的 `allowed_roots` 条目静默地变成无操作（通过 `warn!` 日志缓解）
- `canonical_allowed_roots` 中的逻辑稍微复杂一些

### 安全评估

此更改使允许列表在根目录不存在时**更宽松**（跳过）而不是**出错**。从安全角度来看，这严格更安全：如果根目录不存在，则其下没有任何文件，因此跳过它等同于"允许此根目录下的无内容" — 这是根目录创建之前的有效行为。

---

## 实现

参见提交：`fix(security): skip non-existent extra_roots in canonical_allowed_roots`

修改的文件：
- `crates/edgecrab-security/src/path_policy.rs` — 拆分 `canonical_allowed_roots` 迭代器以优雅地处理 extra_roots
- `crates/edgecrab-tools/src/tools/vision.rs` — 不需要更改（修复在底层）
- `crates/edgecrab-tools/src/tools/pdf_to_markdown.rs` — 可选：简化作为 v0.4.0 临时解决方案添加的 `.exists()` 保护

---

## 考虑并拒绝的替代方案

| 替代方案 | 拒绝原因 |
|---|---|
| 在启动时创建目录 (`ensure_edgecrab_home`) | 为仅 CLI 用户创建不必要的目录 |
| 在每个调用点修复（`.exists()` 保护） | 重复的样板代码；不修复根本原因 |
| 为网关移除路径限制 | 降低远程消息发送者的安全性 |
| 使用单独的 `GatewayPathPolicy` 子类型 | 过度设计；延迟问题是通用的，不是网关特定的 |
| 在 `execute()` 中捕获 `InvalidRoot` 并在不使用 extra_roots 的情况下重试 | 更改工具语义；隐藏真正的故障模式 |

---

## 附录：Hermes 网关隔离对比

Hermes 在网关模式下使用 `MESSAGING_CWD`（默认为 `Path.home()` 的环境变量）作为终端工作目录。文件工具（`file_read` 等）使用 CWD 作为它们的隐式根目录。这默认赋予网关代理对主目录的广泛访问权限。

EdgeCrab 使用进程 CWD（`std::env::current_dir()`）并独立于平台应用 `PathPolicy`。网关接收与 CLI 相同的安全策略，但具有额外的媒体目录可信根目录。

EdgeCrab 模型严格更具限制性。此 ADR 中的修复消除了不必要的脆弱性（extra_roots 在不存在时失败），同时保持了限制。