# 🦀 钩子

> **为什么：** 操作员和平台适配器经常需要在特定的生命周期点运行自定义逻辑 — 审计日志、策略执行、自动恢复门控、完成审查或消息/会话控制 — 而无需修改核心运行时。钩子是 CLI/TUI 和网关之间的扩展点。

**来源：** `crates/edgecrab-gateway/src/hooks.rs`

---

## 两种钩子类型

```text
┌───────────────────────────────────────────────┐
│                  HookRegistry                  │
│                                                │
│  ┌──────────────────┐  ┌─────────────────────┐ │
│  │  Native hooks    │  │  Script hooks        │ │
│  │  (Rust structs)  │  │  (.py / .js / .ts)  │ │
│  │  impl GatewayHook│  │  discovered from     │ │
│  │                  │  │  ~/.edgecrab/hooks/  │ │
│  └──────────────────┘  └─────────────────────┘ │
└───────────────────────────────────────────────┘
          │                         │
          ▼                         ▼
    HookResult::Continue    HookResult::Cancel
    HookResult::Cancel { reason }
```

**原生钩子**是编译到二进制文件中的 Rust 结构体 — 最低延迟，类型安全，可以访问所有内部类型。

**脚本钩子**在启动时从磁盘加载 — 零重新编译要求，可以用 Python、JavaScript 或 TypeScript 编写。

---

## 核心类型

```rust
/// Passed to every hook invocation
pub struct HookContext {
    pub event: String,           // e.g. "session:start", "tool:pre"
    pub session_id: Option<String>,
    pub user_id: Option<String>,
    pub platform: Option<String>,
    pub fields: serde_json::Map<String, Value>, // event-specific payload
}

/// Hook return value — controls whether processing continues
pub enum HookResult {
    Continue,                    // let the event proceed
    Cancel { reason: String },   // abort the event with a reason
}

/// Trait all native hooks implement
pub trait GatewayHook: Send + Sync {
    fn events(&self) -> &[&str];             // which events this hook handles
    async fn handle(&self, ctx: HookContext) -> HookResult;
}

/// Parsed HOOK.yaml manifest
pub struct HookManifest {
    pub name: String,
    pub events: Vec<String>,     // event patterns this hook subscribes to
    pub language: String,        // "python" | "javascript" | "typescript"
    pub handler: String,         // filename: "handler.py", "handler.js"…
}
```

---

## 脚本钩子布局

```text
~/.edgecrab/hooks/
└── my-audit-hook/
    ├── HOOK.yaml         ← manifest
    └── handler.py        ← or handler.js / handler.ts
```

### `HOOK.yaml` 格式

```yaml
name: my-audit-hook
events:
  - session:start
  - session:end
  - tool:pre
language: python
handler: handler.py
```

### `handler.py` 契约

```python
import json, sys

def handle(ctx: dict) -> dict:
    """
    ctx keys: event, session_id, user_id, platform, fields
    Return: {"action": "continue"} or {"action": "cancel", "reason": "..."}
    """
    event = ctx["event"]

    if event == "session:start":
        # audit log
        with open("/var/log/edgecrab-audit.log", "a") as f:
            f.write(json.dumps(ctx) + "\n")

    return {"action": "continue"}


if __name__ == "__main__":
    ctx = json.loads(sys.stdin.read())
    result = handle(ctx)
    print(json.dumps(result))
```

Python 钩子通过 `python3` 运行。JavaScript/TypeScript 钩子通过 `bun` 运行。

在 TUI 中使用 `/hooks` 检查已加载的钩子，查看它们订阅了哪些事件，并在编辑后重新加载注册表。

### `handler.ts` 契约（通过 Bun 的 TypeScript）

```typescript
import { readFileSync } from "fs";

interface HookContext {
  event: string;
  session_id?: string;
  user_id?: string;
  platform?: string;
  fields: Record<string, unknown>;
}

function handle(ctx: HookContext): { action: "continue" | "cancel"; reason?: string } {
  if (ctx.event === "tool:pre" && ctx.fields.tool_name === "bash") {
    const cmd = ctx.fields.command as string;
    if (cmd.includes("rm -rf /")) {
      return { action: "cancel", reason: "Destructive command blocked by hook" };
    }
  }
  return { action: "continue" };
}

const ctx = JSON.parse(readFileSync("/dev/stdin", "utf8")) as HookContext;
console.log(JSON.stringify(handle(ctx)));
```

---

## 事件目录

### 网关生命周期

| 事件 | 何时触发 | 关键 `fields` |
|---|---|---|
| `gateway:startup` | 网关进程启动 | `platform`, `adapter_version` |
| `session:start` | 新会话创建 | `source`, `user_id` |
| `session:end` | 会话正常结束 | `turn_count`, `total_tokens` |
| `session:reset` | `/reset` 斜杠命令 | `session_id` |

### Agent 生命周期

| 事件 | 何时触发 | 关键 `fields` |
|---|---|---|
| `agent:start` | Agent 开始处理一轮 | `model`, `toolset`, `message` |
| `agent:step` | 每次 ReAct 循环迭代 | `iteration`, `tool_name` (if tool call) |
| `agent:end` | Agent 完成一轮 | `iterations`, `tokens` |
| `agent:run_finished` | 运行达到终端 harness 结果 | `completion_state`, `exit_reason`, `summary` |
| `agent:done` | 运行结束后的最终生命周期通知 | `completion_state`, `exit_reason`, `summary` |
| `agent:stop` | 运行被接受前的最终停止审查门控 | `completion_state`, `exit_reason`, `summary`, `active_tasks`, `blocked_tasks` |
| `agent:task_completed` | 运行成功完成 | `summary` |
| `agent:task_blocked` | 运行因阻塞或等待用户输入而结束 | `completion_state`, `summary` |
| `agent:needs_input` | 运行需要用户澄清 | `completion_state`, `summary` |
| `agent:needs_verification` | 运行缺少完成的新鲜证据 | `summary`, `evidence_count` |
| `agent:task_incomplete` | 运行停止且仍有待处理工作 | `summary`, `active_tasks`, `blocked_tasks` |

### 工具事件

| 事件 | 何时触发 | 关键 `fields` |
|---|---|---|
| `tool:pre` | 工具执行前 | `tool_name`, `arguments` |
| `tool:post` | 工具执行后 | `tool_name`, `success`, `output_bytes` |

### LLM 事件

| 事件 | 何时触发 | 关键 `fields` |
|---|---|---|
| `llm:pre` | 发送请求给提供商前 | `model`, `message_count`, `prompt_tokens_est` |
| `llm:post` | 接收响应后 | `model`, `finish_reason`, `tokens` |

### CLI 事件

| 事件 | 何时触发 | 关键 `fields` |
|---|---|---|
| `cli:start` | CLI 进程启动 | `args` |
| `cli:end` | CLI 进程退出 | `exit_code` |

### 命令事件

| 模式 | 何时触发 |
|---|---|
| `command:*` | 任何斜杠命令（`/reset`, `/memory`, `/skills`...） |
| `command:reset` |  specifically `/reset` |

---

## 事件匹配

`HookRegistry` 支持三种匹配模式：

```text
Exact match:    "session:start"  → only that event
Prefix wildcard: "command:*"     → any event starting with "command:"
Global wildcard: "*"             → every event (use sparingly)
```

钩子可以订阅多个模式：

```yaml
events:
  - session:start
  - session:end
  - command:*
```

---

## 钩子执行顺序

当事件发生时：

```text
event fires
     │
     ▼
collect all matching hooks (native + script)
     │
     ▼
execute in registration order
     │
     ├── HookResult::Continue → next hook
     │
     └── HookResult::Cancel { reason }
              │
              ▼
         event aborted
         reason returned to caller
```

单个 `Cancel` 会短路钩子链的其余部分。

在实际行为中，`command:*`、`agent:start` 和 `agent:stop` 特别有用，因为它们可以用人类可读的原因阻止或推迟执行。

---

## 原生钩子示例

```rust
use edgecrab_gateway::hooks::{GatewayHook, HookContext, HookResult};

pub struct RateLimitHook {
    max_sessions_per_minute: u32,
}

impl GatewayHook for RateLimitHook {
    fn events(&self) -> &[&str] {
        &["session:start"]
    }

    async fn handle(&self, ctx: HookContext) -> HookResult {
        let user_id = ctx.user_id.as_deref().unwrap_or("anonymous");
        if self.over_limit(user_id) {
            return HookResult::Cancel {
                reason: format!("Rate limit exceeded for user {user_id}"),
            };
        }
        HookResult::Continue
    }
}
```

在构建网关时注册它：

```rust
gateway_builder.register_hook(Box::new(RateLimitHook { max_sessions_per_minute: 10 }));
```

---

## 重要注意事项

钩子系统是**网关拥有的**。它会为通过平台适配器（Telegram、Discord、CLI-as-gateway 等）到达的会话触发。它**不**提供通用的核心运行时扩展点。对于编译时工具注册，参见 [`inventory::submit!`](004_tools_system/001_tool_registry.md)。

---

## 提示

- **谨慎使用 `Cancel` —** 每次迭代都取消 `agent:step` 的钩子会静默阻止所有工具使用。彻底测试钩子取消。
- **脚本钩子作为子进程运行 —** 存在 I/O 序列化开销。对于热路径（`agent:step`、`llm:pre`），优先使用原生 Rust 钩子。
- **`fields` 是无模式的 —** 确切的键取决于事件。在开发中记录 `ctx` 以发现给定事件的完整负载。
- **Python 钩子需要 `PATH` 上的 `python3` —** 如果网关在最小容器中运行，确保 `python3` 可用或改用基于 `bun` 的 TypeScript 钩子。

---

## 常见问题

**问：钩子可以在消息到达 agent 之前修改它吗？**
答：不能直接通过 `HookResult` — 当前 API 只有 Continue/Cancel。对于消息转换，使用原生 Rust `PlatformAdapter` 中间件或网关级拦截器。

**问：钩子并行运行吗？**
答：不。给定事件的钩子按注册顺序顺序运行。这保持 Cancel 语义的确定性。

**问：我可以将钩子作为技能的一部分发布吗？**
答：目前不行。钩子位于 `~/.edgecrab/hooks/` 中，是网关级别的。技能位于 `~/.edgecrab/skills/` 中，是 agent 级别的。

---

## 交叉引用

- 网关架构（钩子触发的地方）→ [`006_gateway/001_gateway_architecture.md`](006_gateway/001_gateway_architecture.md)
- 平台适配器（网关事件的来源）→ [`006_gateway/001_gateway_architecture.md`](006_gateway/001_gateway_architecture.md)
- 技能（agent 级扩展，不是网关级）→ [`007_memory_skills/002_creating_skills.md`](007_memory_skills/002_creating_skills.md)
- 钩子发现路径配置 → [`009_config_state/001_config_state.md`](009_config_state/001_config_state.md)
