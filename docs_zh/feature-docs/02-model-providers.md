# 模型提供程序系统 🦀

EdgeCrab 支持多种 LLM 模型提供程序，通过统一的 `LLMProvider` trait 抽象。

## 支持的提供程序

### 1. OpenAI 兼容提供程序

```yaml
model:
  provider: openai
  api_key: env:OPENAI_API_KEY
  base_url: https://api.openai.com/v1
  model: gpt-4o-mini
  temperature: 0.7
  max_tokens: 4096
```

### 2. Anthropic

```yaml
model:
  provider: anthropic
  api_key: env:ANTHROPIC_API_KEY
  model: claude-3-5-sonnet-20241022
  temperature: 0.7
  max_tokens: 8192
```

### 3. Google Gemini

```yaml
model:
  provider: gemini
  api_key: env:GOOGLE_API_KEY
  model: gemini-1.5-pro
  temperature: 0.7
  max_output_tokens: 8192
```

### 4. LocalAI

```yaml
model:
  provider: openai
  api_key: env:LOCALAI_API_KEY
  base_url: http://localhost:8080/v1
  model: mistral
  temperature: 0.7
  max_tokens: 4096
```

### 5. Ollama

```yaml
model:
  provider: ollama
  base_url: http://localhost:11434/api
  model: llama3
  temperature: 0.7
  max_tokens: 4096
```

## Provider Trait

所有提供程序都实现了统一的 trait：

```rust
pub trait LLMProvider: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn model_name(&self) -> &str;
    fn context_window(&self) -> usize;
    fn send_message(&self, messages: &[Message]) -> Result<Completion>;
    fn send_streaming(&self, messages: &[Message]) -> Result<impl Stream<Item = Chunk>>;
    fn token_count(&self, text: &str) -> usize;
}
```

## 模型目录

EdgeCrab 使用 `ModelCatalog` 来管理模型元数据：

```rust
pub struct ModelCatalog {
    models: HashMap<String, ModelInfo>,
}

pub struct ModelInfo {
    pub provider:   String,
    pub name:       String,
    pub context:    usize,
    pub price_per_1k_input:  f64,
    pub price_per_1k_output: f64,
    pub capabilities:        Vec<Capability>,
}
```

## 自动模型选择

EdgeCrab 可以根据任务自动选择最佳模型：

```yaml
model:
  auto_select: true
  preferred:
    - gpt-4o-mini
    - claude-3-5-sonnet
    - gemini-1.5-flash
```

### 选择策略

1. **上下文需求**：选择能容纳所需上下文窗口的最小模型
2. **成本优化**：在满足需求的前提下选择最便宜的模型
3. **能力匹配**：选择具有所需能力（如代码、工具使用）的模型

## 回退机制

当主提供程序失败时，EdgeCrab 可以自动回退到备用提供程序：

```yaml
model:
  provider: openai
  model: gpt-4o-mini
  fallbacks:
    - provider: anthropic
      model: claude-3-5-sonnet
    - provider: gemini
      model: gemini-1.5-pro
  max_retries: 3
  retry_delay: 5s
```

## 流式支持

所有提供程序都支持流式输出：

```rust
let stream = provider.send_streaming(&messages).await?;
tokio::pin!(stream);

while let Some(chunk) = stream.next().await {
    let text = chunk.content.as_deref().unwrap_or("");
    print!("{}", text);
}
```

## 工具调用支持

提供程序支持结构化工具调用：

```json
{
  "tool_calls": [
    {
      "id": "call_123",
      "type": "function",
      "function": {
        "name": "get_weather",
        "arguments": {
          "location": "Beijing"
        }
      }
    }
  ]
}
```

### 提供程序特定的工具格式

| 提供程序 | 工具格式 |
|----------|----------|
| OpenAI | JSON mode + function calling |
| Anthropic | Tool use block |
| Gemini | Function calling |
| Ollama | OpenAI-compatible |

## 配置示例

### 完整配置

```yaml
model:
  provider: openai
  api_key: env:OPENAI_API_KEY
  base_url: https://api.openai.com/v1
  model: gpt-4o-mini
  temperature: 0.7
  max_tokens: 4096
  top_p: 0.9
  frequency_penalty: 0.0
  presence_penalty: 0.0
  auto_select: false
  fallbacks:
    - provider: anthropic
      api_key: env:ANTHROPIC_API_KEY
      model: claude-3-5-sonnet
  max_retries: 3
  retry_delay: 5s
  streaming: true
  timeout: 60s
```

### 环境变量配置

```bash
export EDGECRAB_MODEL_PROVIDER=openai
export EDGECRAB_MODEL_API_KEY=sk-xxx
export EDGECRAB_MODEL_NAME=gpt-4o-mini
```

## 性能优化

### 连接池

```yaml
model:
  connection_pool:
    size: 10
    idle_timeout: 30s
    max_lifetime: 1h
```

### 缓存

```yaml
model:
  caching:
    enabled: true
    ttl: 1h
    max_entries: 1000
```

### 请求批处理

```yaml
model:
  batching:
    enabled: true
    max_batch_size: 10
    batch_timeout: 100ms
```

## 监控

### 请求统计

```yaml
model:
  metrics:
    enabled: true
    collect_token_usage: true
    collect_latency: true
    collect_cost: true
```

### 日志

```yaml
model:
  logging:
    enabled: true
    log_requests: false
    log_responses: false
    log_token_usage: true
```

## 验证

### 测试提供程序连接

```bash
edgecrab model test
```

### 测试特定提供程序

```bash
edgecrab model test --provider anthropic
```

### 验证配置

```bash
edgecrab model validate
```

## 自定义提供程序

可以通过实现 `LLMProvider` trait 添加自定义提供程序：

```rust
pub struct MyProvider {
    client: reqwest::Client,
    api_key: String,
    model:   String,
}

impl LLMProvider for MyProvider {
    fn name(&self) -> &str {
        "my-provider"
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn context_window(&self) -> usize {
        4096
    }

    async fn send_message(&self, messages: &[Message]) -> Result<Completion> {
        // 实现 API 调用
    }
}
```

然后在配置中注册：

```yaml
model:
  provider: my-provider
  api_key: env:MY_API_KEY
  model: my-model
```

## 未来计划

- 更多提供程序支持（Cohere, Mistral, Groq）
- 模型微调集成
- 本地模型优化（CUDA, Metal）
- 多模型协作
- 基于性能的动态模型切换