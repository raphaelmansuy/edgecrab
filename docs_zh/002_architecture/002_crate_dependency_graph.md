# Crate 依赖图 🦀

> **已验证来源：** `Cargo.toml` (workspace) · each crate's `Cargo.toml`

---

## 为什么依赖图很重要

依赖图不仅仅是记录——它是一个强制执行的架构约束。如果 `edgecrab-tools` 可以自由导入 `edgecrab-core`，工具可以生成代理，代理又生成更多代理，导致无限递归，而且对代理循环的任何更改都需要重新构建 *所有* 10 个 crate。下面的 DAG 显示了实际结构：每个箭头都是有意的，违反它会导致编译错误。

理解这个图可以回答：*"新代码应该放在哪里？"*

---

## 完整依赖图

```
  edgecrab-cli ──────────────────────────────────────────────┐
  (binary entry point — depends on everything)               │
        │                                                    │
        ├──► edgecrab-gateway ──────────────────────┐        │
        │         │                                 │        │
        ├──► edgecrab-acp ──────────────────┐       │        │
        │                                   │       │        │
        │                                  └──►  edgecrab-core ◄─┘
        │                                              │
        │                    ┌─────────────────────────┼──────────────┐
        │                    │                         │              │
        │                    ▼                         ▼              ▼
        │            edgecrab-tools           edgecrab-state  edgecrab-security
        │            (ToolRegistry, 91 tools) (SQLite WAL)    (CommandScanner)
        │                    │                         │              │
        │                    └─────────────────────────┼──────────────┘
        │                                              │
        │                                              ▼
        │                                      edgecrab-types
        │                                      (leaf — no internal deps)
        │
        ├──► edgecrab-cron ──────────────────────────► edgecrab-types
        │    (also used by edgecrab-tools)
        │
        └──► edgecrab-migrate ─────────────────────────────────────────►
             edgecrab-types, edgecrab-state
```

---

## 依赖表

| Crate | 内部依赖 | 备注 |
|---|---|---|
| `edgecrab-types` | _(none)_ | 叶子节点。每个 crate 都导入这个。`#![deny(clippy::unwrap_used)]` |
| `edgecrab-security` | `edgecrab-types` | 无异步，无 LLM 调用。无状态检查。 |
| `edgecrab-state` | `edgecrab-types` | 唯一拥有原始 SQL 的 crate。 |
| `edgecrab-cron` | `edgecrab-types` | 独立的调度库。 |
| `edgecrab-tools` | `edgecrab-types`、`edgecrab-state`、`edgecrab-security` | 定义 `SubAgentRunner` trait 以避免导入 core。 |
| `edgecrab-core` | `edgecrab-types`、`edgecrab-tools`、`edgecrab-state`、`edgecrab-security` | 实现 `SubAgentRunner`。拥有代理循环。 |
| `edgecrab-acp` | `edgecrab-core`、`edgecrab-types` | 薄薄的 JSON-RPC 2.0 stdio 包装器。 |
| `edgecrab-gateway` | `edgecrab-core`、`edgecrab-tools`、`edgecrab-types`、`edgecrab-state`、`edgecrab-security`、`edgecrab-cron` | 导入集最广；构建完整的消息传递栈。 |
| `edgecrab-cli` | all crates | 二进制入口点；拉入所有依赖。 |
| `edgecrab-migrate` | `edgecrab-types`、`edgecrab-state` | 一次性迁移助手。 |

---

## 解决工具↔核心循环依赖

工具需要生成子代理。子代理存在于 `edgecrab-core` 中。直接导入会产生循环：

```
  edgecrab-core  ──► edgecrab-tools  ──►  edgecrab-core   ✗ CYCLE
```

解决方案是 **trait 对象反转**：

```
  Step 1: edgecrab-tools 定义契约

      pub trait SubAgentRunner: Send + Sync {
          async fn run_task(&self, goal, ...) -> Result<SubAgentResult, String>;
      }

  Step 2: edgecrab-core 实现它

      impl SubAgentRunner for CoreSubAgentRunner { ... }

  Step 3: Agent 将 Arc<dyn SubAgentRunner> 传入 ToolContext

      ctx.sub_agent_runner.run_task("do X")  // 工具从不导入 core

  Result:

      edgecrab-tools ─► edgecrab-types   (SubAgentRunner trait)
      edgecrab-core  ─► edgecrab-tools   (ToolRegistry, ToolHandler)
                     implements SubAgentRunner
                                       ✓ no cycle
```

相同模式适用于 `GatewaySender`：

```
  edgecrab-tools   defines   GatewaySender trait
  edgecrab-gateway implements GatewaySender
  edgecrab-core    holds     RwLock<Option<Arc<dyn GatewaySender>>>
```

🦀 *把螃蟹的钳子（工具）想象成通过肌腱（trait 对象）连接到身体（核心）。钳子可以独立移动——它不需要导入整个神经系统来完成工作。*

---

## 编译时工具注册

工具不会出现在手工维护的列表中。[`inventory`](https://docs.rs/inventory) crate 支持编译时插件收集：

```rust
// In any tool file inside edgecrab-tools:
inventory::submit! {
    &ReadFileTool as &dyn ToolHandler
}

// ToolRegistry::new() in registry.rs:
for handler in inventory::iter::<&dyn ToolHandler> {
    tools.insert(handler.name(), *handler);
}
```

**添加新工具：** 实现 `ToolHandler` + 调用 `inventory::submit!` + 重新编译。无需列表、无需 match 分支、无需更新注册函数。

**参考：** [`inventory` crate docs](https://docs.rs/inventory/latest/inventory/)

---

## 新代码放在哪里

| 场景 | 目标 crate |
|---|---|
| 新的共享类型或枚举 | `edgecrab-types` |
| 新的路径 / URL / 命令 / 注入检查 | `edgecrab-security` |
| 新的 SQL 查询或架构迁移 | `edgecrab-state` |
| 新的 cron 调度格式 | `edgecrab-cron` |
| 新工具 | `edgecrab-tools` |
| 循环行为、提示策略、压缩 | `edgecrab-core` |
| 新的 CLI 子命令或 TUI 功能 | `edgecrab-cli` |
| 新的消息平台 | `edgecrab-gateway` |
| 新的编辑器协议 | `edgecrab-acp` |

---

## 提示

> **提示：使用 `cargo tree` 验证 core 没有被导入到 tools 中。**
> ```sh
> cargo tree -p edgecrab-tools | grep edgecrab-core
> # Must print nothing
> ```

> **提示：保持 `edgecrab-types` 尽可能精简。**
> 您在此处添加的任何依赖都会传播到所有 10 个 crate。撰写本文时，它唯一允许的内部依赖是 `edgequake-llm` 用于类型桥接。

> **提示：`edgecrab-state` 是唯一允许运行 SQL 的 crate。**
> 如果您需要新的查询，请向 `SessionDb` 添加方法——不要向另一个 crate 的 `Cargo.toml` 添加 `rusqlite`。

---

## 常见问题

**问：为什么 `edgecrab-gateway` 依赖于 `edgecrab-cron`？**
网关将 cron 触发的消息作为交付目标。`edgecrab-cron` 中的 `Deliver::Platform` 变体命名了一个网关通道。网关将该名称解析为实际的适配器并发送 cron 输出。

**问：我可以创建一个同时依赖 `edgecrab-core` 和 `edgecrab-gateway` 的 crate 吗？**
可以——`edgecrab-cli` 已经这样做了。它们不是冲突的对等体；它们是可以组合的层次。

---

## 交叉引用

- 系统层概述 → [系统架构](./001_system_architecture.md)
- 并发详情 → [并发模型](./003_concurrency_model.md)
- 工具中的 trait 对象 → [工具注册表](../004_tools_system/001_tool_registry.md)