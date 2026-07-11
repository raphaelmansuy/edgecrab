# 智能模型路由 🦀

> **已验证来源：** `crates/edgecrab-core/src/model_router.rs` ·
> `crates/edgecrab-core/src/model_catalog.rs`

---

## 为什么需要智能路由

在"现在几点了？"上运行 Claude Opus 的成本是 Claude Haiku 的 5 倍，耗时 2–3 倍。但在"用正确的错误处理重构这个 1,000 行模块"上运行 Haiku 会产生明显更差的结果。

智能路由对每个用户回合进行分类并选择最具成本效益的模型。决策是保守的：只有明显简单的消息才会路由到更便宜的模型。当有疑问时，使用主要模型。

🦀 *`hermes-agent` 和 OpenClaw 对所有事情使用一个模型 — 每个"现在几点了？"的成本与完整重构相同。EdgeCrab 为战斗选择正确的武器。*

---

## 路由类型

```rust
// model_router.rs
pub enum TurnRoute {
    Primary,   // 使用配置的主要模型
    Cheap,     // 使用配置的 cheap_model
    Fallback,  // 使用回退模型（如果主要失败）
}
```

---

## 分类算法

`classify_message(msg: &str, thresholds: &RoutingThresholds) -> TurnRoute`

```
  输入：用户消息字符串

  第 1 步 — 长度检查
    chars > 160  → Primary
    words > 28   → Primary
    newlines > 1 → Primary (多行 → 复杂)

  第 2 步 — 结构检查
    contains code fences (```)  → Primary
    contains inline code (`)    → Primary
    contains URL (http://)      → Primary

  第 3 步 — 关键词扫描
    message (lowercased) contains any COMPLEX_KEYWORD → Primary

  第 4 步 — 默认
    以上均未触发 → Cheap
```

---

## 复杂关键词 (来自源码)

```rust
const COMPLEX_KEYWORDS: &[&str] = &[
    // 调试和修复
    "debug", "fix", "bug", "traceback", "exception", "error",
    // 编码
    "implement", "refactor", "patch", "code", "function", "class",
    "struct", "enum", "compile", "build",
    // 分析
    "analyze", "analyse", "architecture", "design", "compare",
    "benchmark", "optimize", "optimise", "review",
    // 工具和执行
    "terminal", "shell", "tool", "docker", "kubernetes",
    "pytest", "test", "deploy", "ci", "pipeline",
    // 规划
    "plan", "delegate", "subagent", "cron",
    // 更多技术关键词...
];
```

---

## 路由决策流程

```
  用户输入消息
        │
        ▼
  SmartRoutingConfig::enabled?
        │
        ├─ NO  → 始终使用 Primary 模型
        │
        └─ YES
              │
              ▼
        classify_message()
              │
              ├─ TurnRoute::Primary → 使用 config.model
              │
              └─ TurnRoute::Cheap   → 使用 config.smart_routing.cheap_model
                                        如果未配置则回退到 Primary
```

---

## 配置

在 `~/.edgecrab/config.yaml` 中：

```yaml
model:
  name: anthropic/claude-opus-4-20250514   # Primary 模型
  smart_routing:
    enabled: true
    cheap_model: anthropic/claude-haiku-4-5-20251001
    # 可选回退：如果主要失败
    fallback_model: anthropic/claude-sonnet-4-20250514
```

或通过 CLI：
```sh
edgecrab --model anthropic/claude-opus-4-20250514 "refactor auth.rs"
```

`--model` 标志覆盖整个会话的智能路由。

在 TUI 中：

```sh
/cheap_model                  # 打开与 /model 相同的选择器式 UI
/cheap_model status           # 检查当前智能路由状态
/cheap_model off              # 禁用 cheap_model 路由并清除其覆盖
/config cheap                 # 从配置中心跳转
```

cheap_model 选择器将 `model.smart_routing.enabled` 和 `model.smart_routing.cheap_model` 持久化回 `config.yaml`。

## 相关的多模型默认值

EdgeCrab 还为 `moa` 工具暴露单独的顶层 `moa` 块（遗留别名：`mixture_of_agents`）：

```yaml
moa:
  enabled: true
  aggregator_model: anthropic/claude-opus-4.6
  reference_models:
    - anthropic/claude-opus-4.6
    - google/gemini-2.5-pro
    - openai/gpt-4.1
    - deepseek/deepseek-r1
```

这些默认值在 MoA 工具调用省略显式 `aggregator_model` 或 `reference_models` 参数时使用。当 `moa.enabled` 为 `false` 时，该工具对模型隐藏，直接调用被拒绝。MoA 也依赖工具集策略：`tools.enabled_toolsets` / `tools.disabled_toolsets` 仍然暴露 `moa` 工具集。`/moa on` 在可能时修复字面白名单和黑名单条目并报告何时更广泛的别名仍然阻止该工具。TUI 暴露：

```sh
/moa status
/moa on
/moa off
/moa aggregator
/moa experts
/moa add
/moa remove
/config moa
```

编辑聚合器或参考阵容会规范化提供者别名、去重阵容并为未来回合重新启用 MoA。执行期间，活动聊天模型也被用作安全网：必要时追加为隐式最后机会专家，聚合在完全失败工具之前回退到当前聊天模型。`/moa reset` 现在写入当前聊天模型的安全基线而不是盲目恢复脆弱的跨提供者阵容。

---

## 模型目录

`ModelCatalog` 是可用模型和提供者的单一真实来源。它从两个来源加载：

```
  1. 嵌入式默认值：model_catalog_default.yaml（编译到二进制文件中）
  2. 用户覆盖：    ~/.edgecrab/models.yaml     (合并在顶部)

  存储在：OnceLock<RwLock<CatalogData>>
  可通过以下方式刷新：ModelCatalog::reload()
```

关键类型：

```rust
pub struct ModelEntry {
    pub id: String,           // "anthropic/claude-opus-4-20250514"
    pub name: String,         // "Claude Opus 4"
    pub provider: String,     // "anthropic"
    pub tier: ModelTier,      // Fast | Balanced | Powerful
    pub context_window: usize,
    pub pricing: PricingPair, // input_per_million, output_per_million (USD)
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_reasoning: bool,
}

pub enum ModelTier { Fast, Balanced, Powerful }
```

---

## 路由阈值

```rust
pub struct RoutingThresholds {
    pub max_chars:    usize, // 默认 160
    pub max_words:    usize, // 默认 28
    pub max_newlines: usize, // 默认 1
}
```

这些在设计上是保守的。160 字符的消息大约是两个中等句子 — 任何更长的内容都可能足够细致以值得使用主要模型。

---

## 路由决策示例

| 消息 | 路由 | 原因 |
|---|---|---|
| `"what time is it?"` | Cheap | 20 字符，无关键词 |
| `"list files in current dir"` | Cheap | 26 字符，无复杂关键词 |
| `"fix the bug in auth.rs line 42"` | Primary | 包含 `fix` 和 `bug` |
| `"refactor the auth module"` | Primary | 包含 `refactor` |
| `"hello"` | Cheap | 5 字符，无关键词 |
| `"implement a redis cache for sessions"` | Primary | 包含 `implement` |
| `"explain this code:\n```rust\n...`"` | Primary | 有换行 + 代码围栏 |

---

## 回退路由

如果主要模型失败（例如速率限制、配额超限）：

```
  primary 因 AgentError::RateLimited 或 AgentError::Llm 失败
        │
        ▼
  fallback_route(config)
        │
        ├─ fallback_model 已配置？→ 使用回退
        └─ 无回退？               → 向调用者传播错误
```

---

## 提示

> **Tip: 将 `smart_routing.cheap_model` 设置为同一提供者的快速模型。**
> 跨提供者路由（例如 Anthropic 用于主要，Together.ai 用于廉价）如果两者都使用 OpenRouter 作为代理则可以工作。同提供者路由避免凭据设置。

> **Tip: 为可重现基准测试禁用智能路由。**
> `smart_routing.enabled: false` 强制每轮通过主要模型，为 A/B 测试提供一致的输出质量。

> **Tip: 添加特定领域的关键词以触发主要路由。**
> 如果项目使用 `"query"` 作为复杂操作关键词，将其添加到配置中的自定义 `routing_keywords` 列表。默认列表涵盖通用软件工程但不是每个领域。

---

## 常见问题

**Q: 智能路由影响对话历史吗？**
不。模型名称记录在 `ConversationResult::model` 中用于本轮，但消息历史使用相同的格式无论哪個模型处理本轮。

**Q: 我可以看到每轮由哪个模型处理吗？**
可以。TUI 状态栏显示当前模型。`ConversationResult::model` 记录每轮的名称。SQLite 数据库中的会话分析包括每轮模型细分。

**Q: 如果廉价模型缺少工具支持怎么办？**
在路由前检查 `ModelEntry::supports_tools`。如果廉价模型不支持工具且本轮需要它们，路由自动回退到 Primary。

---

## 交叉引用

- 循环中集成的路由 → [对话循环](./002_conversation_loop.md)
- `AppConfig` 中的模型配置 → [配置和状态](../009_config_state/001_config_state.md)
- 模型定价和成本跟踪 → [数据模型](../010_data_models/001_data_models.md)
- MoA 工具行为 → [工具目录](../004_tools_system/002_tool_catalogue.md)
