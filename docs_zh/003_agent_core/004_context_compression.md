# 上下文压缩 🦀

> **已验证来源：** `crates/edgecrab-core/src/compression.rs`

---

## 为什么需要压缩

在一个大型代码库上工作的 90 轮迭代 agent 可以生成 50,000+ 令牌的对话历史。大多数 LLM 的上下文窗口为 128,000–200,000 令牌。如果没有干预，长会话要么达到提供者的上下文限制（硬错误），要么静默丢弃早期消息（丢失意图和先前决策）。

EdgeCrab 使用 5 阶段管道在不丢失重要信息的情况下保持会话存活。

🦀 *`hermes-agent` (Python) 在提供者上下文限制时抛出未处理异常。OpenClaw 静默切片早期令牌。EdgeCrab 智能压缩并继续战斗。*

---

## 默认值来自源码

```rust
// compression.rs
const DEFAULT_CONTEXT_WINDOW: usize = 128_000;
const DEFAULT_THRESHOLD:      f64   = 0.50;    // 在 50% 满时压缩
const DEFAULT_TARGET_RATIO:   f64   = 0.20;    // 目标压缩后为窗口的 20%
const DEFAULT_PROTECT_LAST_N: usize = 20;      // 始终保留最后 20 条消息
const PROTECT_FIRST_N:        usize = 3;       // 始终保留前 3 条消息
const PRESSURE_WARNING_PCT:   f64   = 0.85;    // 在阈值的 85% 时警告
```

导出的关键常量：
```rust
pub const SUMMARY_PREFIX: &str =
    "[CONTEXT COMPACTION] Earlier turns were summarised to reclaim context window space.\n\n";

pub const PRUNED_TOOL_PLACEHOLDER: &str =
    "[tool output pruned — reclaimed context window space]";
```

---

## 压缩触发时机

```
  每次 LLM 响应后：
  check_compression_status(messages, params, context_window)
        │
        ├── Ok                         → 无需操作
        │
        ├── PressureWarning             → 发送 StreamEvent::ContextPressure
        │    估计 > 阈值×窗口的 85%     用于警告用户（尚未压缩）
        │
        └── Compressed                 → 运行 compress_with_llm() 管道
             估计 > 阈值×窗口            (当前: > 128K 的 50% = 64K)
```

令牌估计使用快速字符计数近似（~4 字符/令牌）而不是完整分词器 — 足够快用于热路径检查，足够准确用于触发决策。

---

## 5 阶段压缩管道

```
  输入：完整消息历史

  ┌────────────────────────────────────────────────────────────────┐
  │  第 1 阶段 — 工具输出修剪 / 溢出（无 LLM，廉价）              │
  │                                                                │
  │  对每个 tool_result 消息：                                      │
  │    if content.len() > LARGE_OUTPUT_THRESHOLD:                  │
  │      with spill context: 将完整结果写入工件文件                 │
  │                         在历史中保留预览存根                    │
  │      without spill context: 替换为 PRUNED_TOOL_PLACEHOLDER    │
  │      "[tool output pruned — reclaimed context window space]"   │
  │                                                                │
  │  通常在长会话中移除 60-80% 的令牌                               │
  └────────────────────────────────────────────────────────────────┘
                              │
                              ▼
  ┌────────────────────────────────────────────────────────────────┐
  │  第 2 阶段 — 边界确定                                         │
  │                                                                │
  │  识别：                                                       │
  │    protected_head   = messages[0..PROTECT_FIRST_N]   (3)       │
  │    protected_tail   = messages[-protect_last_n..]    (20)      │
  │    compression_zone = messages[3..-20]                         │
  └────────────────────────────────────────────────────────────────┘
                              │
                              ▼
  ┌────────────────────────────────────────────────────────────────┐
  │  第 3 阶段 — LLM 总结 compression_zone                         │
  │                                                                │
  │  系统提示：                                                    │
  │    "Summarise the following conversation into 8 sections:      │
  │     1. Goal   2. Constraints   3. Progress   4. Decisions      │
  │     5. Files   6. Next Steps    7. Critical Context  8. Errors" │
  │                                                                │
  │  结果：一条带有 SUMMARY_PREFIX 前置的系统消息                  │
  └────────────────────────────────────────────────────────────────┘
                              │
                    ┌─────────┴──────────┐
                    │ LLM 失败？          │
                    ▼ Yes                ▼ No (正常)
  ┌──────────────────────┐   ┌───────────────────────────────────┐
  │  第 4 阶段 — 结构回退总结  │   │  插入 SUMMARY_PREFIX 消息 +     │
  │                        │   │  重新组装：head + summary +      │
  │  从元数据生成总结      │   │  tail                              │
  │  + 消息类型（无需 LLM） │   └───────────────────────────────────┘
  └──────────────────────┘
                              │
                              ▼
  ┌────────────────────────────────────────────────────────────────┐
  │  第 5 阶段 — 孤儿清理                                  │
  │                                                                │
  │  遍历最终消息列表：                                            │
  │    │                                                           │
  │    ├── orphaned tool_result (无匹配的 tool_call)              │
  │    │     → 删除（会导致 API 错误）                             │
  │    │                                                           │
  │    └── orphaned tool_call in assistant message                 │
  │          → 注入存根 tool_result                                │
  │            "[result not available after context compression]"  │
  └────────────────────────────────────────────────────────────────┘
```

---

## 8 部分总结格式

LLM 被指示在第 3 阶段产生此确切结构：

| 部分 | 内容 |
|---|---|
| **Goal** | 用户的原始整体目标 |
| **Constraints** | 会话期间建立的规则和限制 |
| **Progress** | 已完成或确认的内容 |
| **Decisions** | 做出的关键决策及其背后的原因 |
| **Files touched** | 读取、写入或修改的文件 |
| **Next steps** | 压缩前计划的内容 |
| **Critical context** | 不能丢失的任何事实 |
| **Errors encountered** | 遇到的故障及如何解决 |

---

## 示例：前后对比

**压缩前**（简化）：
```
  system:    "你是 EdgeCrab..."  [5,000 tokens]
  user:      "Refactor the auth module"
  assistant: "I'll start by reading the files"
  tool:      [read_file content: 8,000 tokens of source]
  tool:      [read_file content: 6,000 tokens of source]
  assistant: "I've read both files. Here's my plan..."
  user:      "Proceed"
  assistant: "Writing the refactored version..."
  tool:      [write_file result]
  ...  [60 more messages]
  Total: ~95,000 tokens
```

**压缩后**（第 1 + 3 阶段）：
```
  system:    "你是 EdgeCrab..."  [5,000 tokens]
  system:    "[CONTEXT COMPACTION] Goal: Refactor auth module.
              Progress: Read auth.rs and session.rs. Wrote
              new auth.rs with JWT support.
              Files touched: src/auth.rs, src/session.rs
              ..."  [~800 tokens]
  user:      [最后 20 条消息保留]
  ...
  Total: ~15,000 tokens
```

当压缩器有溢出上下文时，修剪的工具消息可能变为：

```text
[tool_result_spill]
tool: file_search
lines: 2847
bytes: 98304
artifact: .edgecrab-artifacts/ses_abc123/file_search_001.md
showing: 80/2847 lines (first 3%)
```

---

## 提示

> **Tip: `StreamEvent::ContextPressure` 是早期警告信号。**
> 当你在 TUI 中看到"上下文压力"时，接下来的几次迭代将触发压缩。如果你正在执行任务，考虑在压缩触发前完成当前子任务 — 它可能会丢失一些工具输出细节。

> **Tip: 在 `Agent` 上调用 `force_compress()` 立即触发压缩。**
> 在测试中或想要在移交给子 agent 之前检查点长会话时很有用。

> **Tip: `protect_last_n` 默认值为 20，意味着最近 20 条消息始终逐字保留。** 压缩永远不会截断你的即时上下文 — 只有旧历史被总结。

---

## 常见问题

**Q: 压缩会丢失信息吗？**
摘要中会丢失一些细节，但关键事实通过 8 部分格式保留。LLM 被指示保留所有决策、错误和文件路径 — 这比 `hermes-agent` 或 OpenClaw 的简单截断更有条理。

**Q: 如果 LLM 无法生成摘要怎么办？**
第 4 阶段介入：从消息类型元数据生成结构摘要（无内容）。质量较低但从不会崩溃会话。

**Q: 压缩如何影响工具调用历史完整性？**
第 5 阶段（孤儿净化）确保最终消息列表始终有效。孤立的 `tool_result` 消息（其 `tool_call` 被修剪）被删除。孤立的 `tool_call` 引用获得插入的存根结果。

**Q: 如果工具结果被溢出到磁盘，agent 在压缩后仍能使用它吗？**
可以。存根保留相对工件路径，工件位于活动工作空间下，因此现有文件工具稍后可以检查它。

**Q: 我可以禁用压缩吗？**
在 `~/.edgecrab/config.yaml` 中设置 `compression.enabled = false`。Agent 将改为达到提供者的上下文限制并获得硬性 `ContextLimit` 错误。不建议用于长会话。

---

## 交叉引用

- 循环中触发压缩的位置 → [对话循环](./002_conversation_loop.md)
- 令牌估计和成本跟踪 → [数据模型](../010_data_models/001_data_models.md)
- 通过压缩保留的会话消息格式 → [会话存储](../009_config_state/002_session_storage.md)
