# 工具注册表 🦀

> **已验证来源：** `crates/edgecrab-tools/src/registry.rs` ·
> `crates/edgecrab-tools/src/tools/mod.rs`

---

## 为什么需要注册表

没有注册表，agent 循环需要知道每个工具：导入它、调用它、处理其错误。添加一个工具意味着编辑循环。

注册表反转了这一点：工具在编译时通过 `inventory::submit!` 声明自己。循环调用 `ToolRegistry::dispatch(name, args, ctx)` 并获得结果 — 它不知道哪个工具运行，工具如何工作，或工具位于哪个 crate 中。

🦀 *`hermes-agent` (Python) 通过中央 handler dict 分发工具 — 添加工具意味着编辑 dispatch map。EdgeCrab 的注册表意味着新工具本质上是新文件中的新结构。螃蟹在不需要手术的情况下长出新爪子。*

---

## 注册：通过 `inventory` 进行编译时注册

```rust
// edgecrab-tools/src/tools/ 中的任何文件

struct ReadFileTool;

#[async_trait]
impl ToolHandler for ReadFileTool {
    fn name(&self)    -> &'static str { "read_file" }
    fn toolset(&self) -> &'static str { "file" }
    fn schema(&self)  -> ToolSchema   { /* JSON schema */ }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext)
        -> Result<String, ToolError>
    {
        let path = args["path"].as_str()
            .ok_or_else(|| ToolError::InvalidArgs { .. })?;
        // ... 读取文件 ...
        Ok(content)
    }
}

// 此行在二进制启动时注册 ReadFileTool — 无需维护列表
inventory::submit! { &ReadFileTool as &dyn ToolHandler }
```

`ToolRegistry::new()` 迭代 `inventory::iter::<&dyn ToolHandler>` 并自动构建内部 `HashMap<name, handler>`。

**参考：** [`inventory` crate](https://docs.rs/inventory/latest/inventory/)

---

## `ToolHandler` trait

```rust
#[async_trait]
pub trait ToolHandler: Send + Sync + 'static {
    // 必填
    fn name(&self)    -> &'static str;
    fn toolset(&self) -> &'static str;
    fn schema(&self)  -> ToolSchema;
    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext)
        -> Result<String, ToolError>;

    // 可选 — 显示默认值
    fn is_available(&self) -> bool { true }        // 启动时：docker 存在？API 配置？
    fn check_fn(&self, _ctx: &ToolContext) -> bool { true }  // 每次请求：平台允许？
    fn parallel_safe(&self) -> bool { false }       // 可以与同级工具并发运行？
    fn emoji(&self) -> &'static str { "⚡" }         // TUI 显示
}
```

`is_available()` 在注册表构建时调用一次。未通过检查的工具仍然被注册但从发送给 LLM 的 schema 列表中排除。

`check_fn()` 在每次分发时调用。用于每请求条件（例如，`ha_*` 工具检查 `HA_URL` 在调用时是否配置）。

---

## 分发路径

```
  ToolRegistry::dispatch(name, args, ctx)
        │
        ├─ 静态工具中的精确匹配？
        │       │
        │       ├─ toolset 在 active_toolsets 中？（否 → CapabilityDenied）
        │       ├─ check_fn(&ctx)?             （否 → CapabilityDenied）
        │       └─ handler.execute(args, ctx)
        │
        ├─ 动态工具中的精确匹配？（MCP、插件）
        │       └─ 与静态相同的门控
        │
        └─ 无精确匹配
                │
                ▼
          fuzzy_match(name)   [莱文斯坦距离 ≤ 3]
                │
                ├─ 找到：  ToolError::NotFound("Did you mean: <suggestion>?")
                └─ 未找到：ToolError::NotFound(name)
```

**参考：** [莱文斯坦距离](https://en.wikipedia.org/wiki/Levenshtein_distance)

---

## `ToolContext` — 执行环境

每个工具接收一个 `ToolContext` 引用。这是工具可以访问的内容的完整图景：

```rust
pub struct ToolContext {
    pub task_id:          String,
    pub cwd:              PathBuf,          // 当前工作目录
    pub session_id:       String,
    pub user_task:        Option<String>,   // 原始用户请求（用于委托）
    pub cancel:           CancellationToken,
    pub config:           AppConfigRef,     // 只读配置快照
    pub state_db:         Option<Arc<SessionDb>>,
    pub platform:         Platform,
    pub process_table:    Option<Arc<ProcessTable>>,
    pub provider:         Option<Arc<dyn LLMProvider>>,  // 用于 generate_image 等
    pub tool_registry:    Option<Arc<ToolRegistry>>,     // 用于 moa
    pub delegate_depth:   u32,              // max=2；防止失控递归
    pub sub_agent_runner: Option<Arc<dyn SubAgentRunner>>,
    pub clarify_tx:       Option<UnboundedSender<ClarifyRequest>>,   // 询问用户
    pub approval_tx:      Option<UnboundedSender<ApprovalRequest>>,  // 门控危险操作
    pub on_skills_changed: Option<Arc<dyn Fn() + Send + Sync>>,
    pub gateway_sender:   Option<Arc<dyn GatewaySender>>,
    pub origin_chat:      Option<(String, String)>,  // (platform, chat_id)
    pub session_key:      Option<String>,
    pub todo_store:       Option<Arc<TodoStore>>,
}
```

测试使用 `ToolContext::test_context()`（仅在 `#[cfg(test)]` 时编译）。

---

## 动态工具 (MCP + 插件)

静态工具使用 `inventory` 并在编译时包含。动态工具在运行时注册：

```rust
impl ToolRegistry {
    pub fn register_dynamic(&mut self, handler: Box<dyn ToolHandler>)
}
```

这由以下使用：
- **MCP 服务器** — `mcp_list_tools` 将远程工具代理为动态 `ToolHandler` 实例
- **插件** — 在启动时从 `~/.edgecrab/plugins/` 加载

动态工具参与与静态工具相同的所有分发逻辑（工具集过滤、审批门控、模糊匹配）。

---

## `GatewaySender` 和 `SubAgentRunner` traits

这两个 trait 打破了循环依赖（见[Crate 依赖图](../002_architecture/002_crate_dependency_graph.md)）：

```rust
// 定义于 edgecrab-tools/src/registry.rs
// 分别在 edgecrab-gateway 和 edgecrab-core 中实现

#[async_trait]
pub trait GatewaySender: Send + Sync + 'static {
    async fn send_message(&self, platform, recipient, message) -> Result<(), String>;
    async fn list_targets(&self) -> Result<Vec<String>, String>;
}

#[async_trait]
pub trait SubAgentRunner: Send + Sync {
    async fn run_task(
        &self,
        goal: String,
        system_prompt: Option<String>,
        enabled_toolsets: Vec<String>,
        max_iterations: u32,
        model_override: Option<String>,
        parent_cancel: CancellationToken,
    ) -> Result<SubAgentResult, String>;
}
```

---

## 编写新工具 — 逐步说明

```sh
# 1. 创建文件
touch crates/edgecrab-tools/src/tools/my_tool.rs

# 2. 实现 ToolHandler（见下方模板）

# 3. 添加模块声明
echo 'pub mod my_tool;' >> crates/edgecrab-tools/src/tools/mod.rs

# 4. 添加到 toolsets.rs 中的工具集（或创建新的工具集条目）

# 5. 如果适用，将工具名称添加到 toolsets.rs 中的 CORE_TOOLS 或 ACP_TOOLS

# 6. cargo build -- 验证编译并通过工具列表显示
edgecrab tools list | grep my_tool
```

最小工具模板：

```rust
use async_trait::async_trait;
use serde_json::Value;
use edgecrab_types::{ToolSchema, ToolError};
use crate::registry::{ToolContext, ToolHandler};

pub struct MyTool;

#[async_trait]
impl ToolHandler for MyTool {
    fn name(&self)    -> &'static str { "my_tool" }
    fn toolset(&self) -> &'static str { "file" }       // 或新的工具集名称

    fn schema(&self)  -> ToolSchema {
        ToolSchema {
            name: "my_tool".into(),
            description: "What this tool does and when to use it.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path" }
                },
                "required": ["path"]
            }),
            strict: None,
        }
    }

    async fn execute(&self, args: Value, ctx: &ToolContext)
        -> Result<String, ToolError>
    {
        let path = args["path"].as_str()
            .ok_or_else(|| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: "path is required".into(),
            })?;

        // 如果可能很慢，定期检查取消
        if ctx.cancel.is_cancelled() {
            return Err(ToolError::ExecutionFailed {
                tool: self.name().into(),
                message: "cancelled".into(),
            });
        }

        Ok(format!("processed {path}"))
    }
}

inventory::submit! { &MyTool as &dyn ToolHandler }
```

---

## 提示

> **Tip: 为模型编写工具描述，而不是为人。**
> `ToolSchema` 中的 `description` 字段是 LLM 阅读以决定是否调用你的工具的内容。明确说明*何时*使用它以及它返回什么。

> **Tip: 包含完整的输入 schema 和 `"required"` 字段。**
> 接受可选参数的工具应该用默认值优雅地处理缺失的键。模型可能会省略可选字段。

> **Tip: 使用 `check_fn()` 进行环境相关的可用性。**
> 如果你的工具需要 API 密钥或运行中的服务，在 `check_fn()` 中检查它。这返回带有有帮助消息的 capability-denied 错误，而不是静默产生晦涩的执行失败。

---

## 常见问题

**Q: 模型如何知道哪些工具可用？**
`ToolRegistry::get_definitions(enabled, disabled, ctx)` 返回一个 `Vec<ToolSchema>`，按活动工具集和 `check_fn()` 过滤。此列表作为 `tools` 参数传递给 LLM 提供者，用于每次 API 调用。

**Q: 工具可以调用另一个工具吗？**
不能直接 — 工具不应该导入彼此或调用 `ToolRegistry::dispatch` 它们自己。对于子任务，使用 `ctx.sub_agent_runner.run_task(...)`（`delegate_task` 工具包装了这个）或通过 `fork_isolated` 启动隔离的 agent。

**Q: `MAX_CLARIFY_CHOICES = 4` 是什么？**
`clarify` 工具向用户发送带有最多 4 个选项的澄清请求。`Clarify` 流事件携带一个 `oneshot::Sender<String>`；前端渲染选项并将用户的选择发回。

---

## 交叉引用

- 工具目录（全部 65 个名称）→ [工具目录](./002_tool_catalogue.md)
- 工具集组成和别名 → [工具集组成](./003_toolset_composition.md)
- `ToolContext` 和后端 → [工具运行时](./004_tools_runtime.md)
- 发送回模型的错误载荷 → [错误处理](../002_architecture/004_error_handling.md)
