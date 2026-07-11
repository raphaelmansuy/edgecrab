# 对话循环 🦀

> **已验证来源：** `crates/edgecrab-core/src/conversation.rs`

---

## 为什么循环很重要

ReAct（推理+行动）模式是概念基础，但理解实际代码循环才能调试 agent 行为。当 EdgeCrab 忽略工具结果、意外循环或预算耗尽时，答案总是在 `execute_loop` 中。

**参考：** [ReAct: Synergizing Reasoning and Acting in Language Models](https://arxiv.org/abs/2210.03629)

---

## 循环常量 (来自源码)

```rust
// conversation.rs
const MAX_RETRIES: u32 = 3;
const BASE_BACKOFF: Duration = Duration::from_millis(500);
const SKILL_REFLECTION_THRESHOLD: u32 = 5;  // 学习反思在一轮中≥5次工具调用时触发
```

---

## 完整注释循环

```
  execute_loop(user_message, ...)
  ══════════════════════════════════════════════════════════════

  [设置]
    快照配置 + provider (RwLock 读取，await 前释放保护)
    解析 cwd 和启用的工具集
    重置本轮的 CancellationToken

  ──────────────────────────────────────────────────────────────

  [扩展]
    expand_context_refs(user_message)
      "@./src/lib.rs" → 内联文件内容
      "@http://..."   → 获取页面
      "@session:id"   → 会话搜索结果

  ──────────────────────────────────────────────────────────────

  [第一轮：构建系统提示]
    if cached_system_prompt is None:
      PromptBuilder::build()
        → SOUL.md / EDGECRAB.md / AGENTS.md / CLAUDE.md
        → 内存文件部分 (如果 skip_memory=false)
        → 技能摘要 (如果存在技能)
        → 工具特定指导块
        → 注入检查所有外部内容
      存储到 SessionState::cached_system_prompt

  ──────────────────────────────────────────────────────────────

  循环 (最多 max_iterations = 90):
  ┌──────────────────────────────────────────────────────────┐
  │                                                          │
  │  [预算检查]                                              │
  │    budget.try_consume() → false → BudgetExhausted       │
  │                                                          │
  │  [取消检查]                                              │
  │    is_cancelled() → true → break with interrupted=true   │
  │                                                          │
  │  [路由]  (如果启用智能路由)                              │
  │    classify_message(last_user_msg)                       │
  │    TurnRoute::Cheap  → 切换到 cheap_model               │
  │    TurnRoute::Primary → 保持 primary model              │
  │                                                          │
  │  [压缩]                                                  │
  │    check_compression_status(messages, params, ctx_len)   │
  │    PressureWarning → 发送 StreamEvent::ContextPressure   │
  │    Compressed      → compress_with_llm() → 5阶段管道    │
  │                                                          │
  │  [提供者调用]   (最多 MAX_RETRIES=3，退避 500ms)         │
  │    provider.chat(messages, tools, streaming)             │
  │                                                          │
  │    RateLimited → sleep(retry_after_ms) → 重试           │
  │    ContextLimit → 触发压缩 → 重试                        │
  │    其他错误  → AgentError 传播给调用者                    │
  │                                                          │
  │  ┌─────────────────────────────────────────────┐         │
  │  │ 响应：tool_calls?               │ text? │         │
  │  └────────────────┬──────────────────────┬──────┘         │
  │                   │ YES                  │ NO             │
  │  ┌────────────────▼──────┐   ┌───────────▼──────────┐    │
  │  │ 工具分发              │   │ 最终响应              │    │
  │  │                       │   │                      │    │
  │  │ 对每个 tool_call:     │   │  发送 Token 事件      │    │
  │  │   1. 安全检查         │   │  提取推理             │    │
  │  │   2. 审批门控         │   │  修剪 <think> 标签         │    │
  │  │   3. 解析工具集       │   │                      │    │
  │  │   4. 发送 ToolExec    │   │  持久化会话           │    │
  │  │   5. execute()        │   │  到 SQLite            │    │
  │  │   6. 发送 ToolDone    │   │                      │    │
  │  │   7. 追加结果         │   │  返回                │    │
  │  │     到消息            │   │  ConversationResult   │    │
  │  └────────────────┬──────┘   └──────────────────────┘    │
  │                   │ loop ◄─────────────────────────────   │
  │                   │                                       │
  └──────────────────-┼───────────────────────────────────────┘

  [循环后]
    if tool_call_count >= SKILL_REFLECTION_THRESHOLD (5):
      learning_reflection()  ← 闭合学习循环
    persist to SQLite (if state_db present)
    return ConversationResult
```

---

## 工具分发详解

默认情况下，响应中的每个工具按顺序分发。`parallel_safe=true` 的工具可能并发分发（见[并发模型](../002_architecture/003_concurrency_model.md)）。

对于每次工具调用：

```
  1. 安全门控
       edgecrab-security::command_scan (用于终端工具)
       edgecrab-security::path_jail    (用于文件工具)

  2. 审批门控
       ApprovalPolicy::check(tool_name, args, session_id)
       if needs_approval:
         发送 StreamEvent::Approval { command, tx }
         等待用户通过 oneshot 通道响应
         Once / Session / Always / Deny

  3. ToolRegistry::dispatch(name, args, ctx)
       精确匹配 → handler.check_fn(&ctx) → handler.execute(args, &ctx)
       无匹配    → 模糊匹配 (Levenshtein ≤ 3) → ToolError::NotFound

  4. 结果处理
       Ok(string)   → maybe_spill()
                    → 内联原始结果
                      OR 预览存根 + 工件路径在
                         .edgecrab-artifacts/<session_id>/...
                    → Message::tool_result(id, name, result) → 追加
       Err(ToolError) → 序列化为 ToolErrorResponse JSON → 追加
                        (错误保持内联以便模型可以自我纠正)
```

---

## 终止条件

| 条件 | 如何退出 | `ConversationResult` 字段 |
|---|---|---|
| 模型返回文本（无工具调用） | `break` 带响应 | `final_response` |
| 迭代预算耗尽 | `break` 从预算检查 | `budget_exhausted = true` |
| 用户取消 | `break` 从取消检查 | `interrupted = true` |
| 最大重试次数超限 | `Err(AgentError::Llm)` 传播 | — |
| 压缩失败 3 次 | `Err(AgentError::CompressionFailed)` | — |

---

## 消息历史不变量

对话历史始终使用 OpenAI 兼容的消息格式：

```
  system     (由 PromptBuilder 构建，缓存)
  user       (原始用户消息)
  assistant  (模型响应，可能包含 tool_calls)
  tool       (工具结果，每个 tool_call 一个)
  tool       (...)
  assistant  (下一个模型响应)
  ...
```

这种形状是压缩、持久化和恢复所依赖的。破坏它——例如，在另一个 `assistant` 之后立即追加一个 `assistant`——会产生提供者 API 错误。

大型成功的工具结果可能显示为 `[tool_result_spill]` 存根而不是原始输出。这仍然是一个有效的 `tool` 消息；完整负载位于引用的工件文件中，并通过 `read_file` 保持可访问。

---

## `ConversationResult`

```rust
pub struct ConversationResult {
    pub final_response:   String,
    pub messages:         Vec<Message>,   // 完整回合历史
    pub session_id:       String,
    pub api_calls:        u32,
    pub interrupted:      bool,
    pub budget_exhausted: bool,
    pub model:            String,
    pub usage:            Usage,           // input/output/cache/reasoning tokens
    pub cost:             Cost,            // USD 估算成本
    pub tool_errors:      Vec<ToolErrorRecord>,  // 本回合所有失败
}
```

---

## 学习反思 (≥5 次工具调用)

当一轮使用 5 次或更多工具调用时，`learning_reflection()` 运行：

```
  轮次以 tool_call_count >= 5 结束
        │
        ▼
  learning_reflection(messages, model, provider)
        │
        ▼
  LLM 调用："什么模式应该影响未来的技能创建？"
        │
        ▼
  可选：将学习写入 ~/.edgecrab/memories/session_insights.md
```

这实现了一个闭合学习循环：长而复杂的回合教会 EdgeCrab 有用模式，而不需要明确的用户指令。

🦀 *`hermes-agent` 和 OpenClaw 从未这样做过。EdgeCrab 从每场战斗会话中自动变得更聪明。*

---

## 调试技巧

> **Tip: 启用 `RUST_LOG=edgecrab_core=debug` 跟踪每次迭代。**
> 每次预算检查、路由决策、压缩触发和工具分发都发出结构化日志条目。

> **Tip: `ConversationResult::tool_errors` 是你的事后分析日志。**
> 每次失败的工具调用都记录了它被调用的确切参数。如果 agent 似乎"放弃"或做了意想不到的事情，先检查这个。

> **Tip: `budget_exhausted = true` 意味着模型需要超过 90 轮迭代。**
> 要么任务确实复杂（在配置中提高 `max_iterations`），要么模型陷入循环（检查 `tool_errors` 中重复相同的调用）。

---

## 常见问题

**Q: 为什么循环将提供者重试最多 3 次？**
瞬态 API 错误（速率限制、网络抖动）在生产中很常见。指数退避（基础 500ms，加倍）处理其中大部分而不打扰用户。3 次失败后错误传播。

**Q: 我可以在迭代之间添加自定义钩子吗？**
可以。`StreamEvent::HookEvent { event, context_json }` 在关键点发出。在 `edgecrab-gateway/src/hooks.rs` 中实现原生钩子或在 `~/.edgecrab/hooks/` 中实现基于文件的脚本钩子。见[钩子](../hooks.md)。

**Q: 循环支持多步骤工具链吗？例如 search → read → write？**
是的。每次迭代追加工具结果并重新调用模型。模型自然地链接："我搜索并找到了 X，现在我将读取 Y，现在我将写入 Z"跨多个迭代。每一步消耗一轮预算。

---

## 交叉引用

- `execute_loop()` 实现的对话循环 → [Agent 结构](./001_agent_struct.md)
- 循环中触发的上下文压缩 → [上下文压缩](./004_context_compression.md)
- 从循环调用的智能模型路由 → [智能模型路由](./005_smart_model_routing.md)
- 工具分发实现 → [工具注册表](../004_tools_system/001_tool_registry.md)
- 循环中处理的错误变体 → [错误处理](../002_architecture/004_error_handling.md)
