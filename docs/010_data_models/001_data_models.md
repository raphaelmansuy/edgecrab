# 010.001 — Data Models

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 003.002 Conversation Loop](../003_agent_core/002_conversation_loop.md) | [→ 002.001 Architecture](../002_architecture/001_system_architecture.md)
> **Source**: `edgecrab-types/src/message.rs`, `edgecrab-types/src/tool.rs`, `edgecrab-types/src/usage.rs` — verified against real implementation

## 1. Message Types

```rust
// edgecrab-types/src/message.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Option<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,  // tool name for tool results
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrl },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,  // data:image/png;base64,... or https://...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,  // "low" | "high" | "auto"
}

impl Message {
    pub fn user(text: &str) -> Self { ... }
    pub fn assistant(text: &str) -> Self { ... }
    pub fn tool_result(tool_call_id: &str, content: &str) -> Self { ... }
    pub fn system(text: &str) -> Self { ... }
    pub fn system_summary(text: String) -> Self { ... }

    pub fn text_content(&self) -> String {
        match &self.content {
            Some(Content::Text(t)) => t.clone(),
            Some(Content::Parts(parts)) => parts.iter()
                .filter_map(|p| match p {
                    ContentPart::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
            None => String::new(),
        }
    }
}
```

## 2. Tool Call Types

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub r#type: String,  // always "function"
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,  // JSON string
}

impl ToolCall {
    pub fn parsed_args(&self) -> Result<serde_json::Value> {
        Ok(serde_json::from_str(&self.function.arguments)?)
    }
}
```

## 3. Usage & Pricing

```rust
// edgecrab-types/src/usage.rs

#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Cost {
    pub input_cost: f64,
    pub output_cost: f64,
    pub cache_read_cost: f64,
    pub cache_write_cost: f64,
    pub total_cost: f64,
}

/// Normalize usage across different API response formats
pub fn normalize_usage(raw: &serde_json::Value, api_mode: ApiMode) -> Usage {
    match api_mode {
        ApiMode::ChatCompletions => Usage {
            input_tokens: raw["prompt_tokens"].as_u64().unwrap_or(0),
            output_tokens: raw["completion_tokens"].as_u64().unwrap_or(0),
            // OpenRouter includes cache info in prompt_tokens_details
            cache_read_tokens: raw["prompt_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0),
            ..Default::default()
        },
        ApiMode::AnthropicMessages => Usage {
            input_tokens: raw["input_tokens"].as_u64().unwrap_or(0),
            output_tokens: raw["output_tokens"].as_u64().unwrap_or(0),
            cache_read_tokens: raw["cache_read_input_tokens"].as_u64().unwrap_or(0),
            cache_write_tokens: raw["cache_creation_input_tokens"].as_u64().unwrap_or(0),
            ..Default::default()
        },
        ApiMode::CodexResponses => Usage {
            input_tokens: raw["input_tokens"].as_u64().unwrap_or(0),
            output_tokens: raw["output_tokens"].as_u64().unwrap_or(0),
            ..Default::default()
        },
    }
}

/// Calculate cost using edgequake-llm's CostTracker
pub fn estimate_cost(usage: &Usage, model: &str) -> Cost {
    // Delegate to edgequake-llm cost tracking
    // Falls back to known pricing table
    todo!()
}
```

## 4. API Modes

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ApiMode {
    ChatCompletions,     // OpenAI / OpenRouter standard
    AnthropicMessages,   // Direct Anthropic API
    CodexResponses,      // OpenAI Codex Responses API
}

impl ApiMode {
    /// Auto-detect from base URL
    pub fn detect(base_url: &str, model: &str) -> Self {
        if base_url.contains("api.anthropic.com") {
            ApiMode::AnthropicMessages
        } else if base_url.contains("api.openai.com") && model.contains("codex") {
            ApiMode::CodexResponses
        } else {
            ApiMode::ChatCompletions
        }
    }
}
```

## 5. Trajectory Format

```rust
// edgecrab-types/src/trajectory.rs

#[derive(Serialize, Deserialize)]
pub struct Trajectory {
    pub session_id: String,
    pub model: String,
    pub timestamp: String,
    pub messages: Vec<Message>,
    pub metadata: TrajectoryMetadata,
}

#[derive(Serialize, Deserialize)]
pub struct TrajectoryMetadata {
    pub task_id: Option<String>,
    pub total_tokens: u64,
    pub total_cost: f64,
    pub api_calls: u32,
    pub tools_used: Vec<String>,
    pub completed: bool,
    pub duration_seconds: f64,
}

/// Save trajectory as JSONL (one JSON object per line)
pub fn save_trajectory(path: &Path, trajectory: &Trajectory) -> Result<()> {
    let json = serde_json::to_string(trajectory)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true).append(true).open(path)?;
    writeln!(file, "{}", json)?;
    Ok(())
}

/// Convert <REASONING_SCRATCHPAD> tags to <think> tags (legacy format compat)
pub fn convert_scratchpad_to_think(content: &str) -> String {
    content.replace("<REASONING_SCRATCHPAD>", "<think>")
           .replace("</REASONING_SCRATCHPAD>", "</think>")
}

/// Check if content has an opening <REASONING_SCRATCHPAD> without closing tag
pub fn has_incomplete_scratchpad(content: &str) -> bool {
    content.contains("<REASONING_SCRATCHPAD>") && !content.contains("</REASONING_SCRATCHPAD>")
}
```

## 6. Reasoning/Thinking Blocks

hermes-agent extracts reasoning from `<think>` tags and stores it in a separate field:

```rust
/// Extract thinking blocks from assistant content
pub fn extract_reasoning(content: &str) -> (String, Option<String>) {
    let re = regex::Regex::new(r"(?s)<think>(.*?)</think>").unwrap();
    let reasoning = re.captures(content).map(|c| c[1].trim().to_string());
    let cleaned = re.replace_all(content, "").trim().to_string();
    (cleaned, reasoning)
}

/// Check if content after think block is empty (thinking exhaustion)
pub fn has_content_after_think(content: &str) -> bool {
    let re = regex::Regex::new(r"(?s)<think>.*?</think>").unwrap();
    let after = re.replace_all(content, "").trim().to_string();
    !after.is_empty()
}
```

## 7. Anthropic Adapter Model (v0.4.0)

### 7.1 Thinking Budget Configuration

```rust
/// Thinking budget by reasoning effort level
const THINKING_BUDGET: &[(&str, u32)] = &[
    ("xhigh", 32000),
    ("high", 16000),
    ("medium", 8000),
    ("low", 4000),
];

/// Map reasoning effort to Anthropic's native adaptive thinking levels
const ADAPTIVE_EFFORT_MAP: &[(&str, &str)] = &[
    ("xhigh", "max"),
    ("high", "high"),
    ("medium", "medium"),
    ("low", "low"),
    ("minimal", "low"),
];

/// Only Claude 4.6+ supports adaptive thinking (not Claude 4.0/4.5)
pub fn supports_adaptive_thinking(model: &str) -> bool {
    let m = model.to_lowercase();
    m.contains("4-6") || m.contains("4.6")
}

/// Anthropic-specific beta headers (sent with ALL auth types)
const COMMON_BETAS: &[&str] = &[
    "interleaved-thinking-2025-05-14",
    "fine-grained-tool-streaming-2025-05-14",
];

/// Per-model max output token limits (thinking tokens count toward limit!)
/// Uses longest-prefix substring match so date-stamped IDs resolve correctly.
const ANTHROPIC_OUTPUT_LIMITS: &[(&str, u32)] = &[
    // Claude 4.6
    ("claude-opus-4-6",    128_000),
    ("claude-sonnet-4-6",   64_000),
    // Claude 4.5
    ("claude-opus-4-5",     64_000),
    ("claude-sonnet-4-5",   64_000),
    ("claude-haiku-4-5",    64_000),
    // Claude 4
    ("claude-opus-4",       32_000),
    ("claude-sonnet-4",     64_000),
    // Claude 3.7
    ("claude-3-7-sonnet",  128_000),
    // Claude 3.5
    ("claude-3-5-sonnet",    8_192),
    ("claude-3-5-haiku",     8_192),
    // Claude 3
    ("claude-3-opus",        4_096),
    ("claude-3-sonnet",      4_096),
    ("claude-3-haiku",       4_096),
];
/// Default for unknown future models
const ANTHROPIC_DEFAULT_OUTPUT_LIMIT: u32 = 128_000;

/// Get Anthropic max output tokens (longest-prefix substring match)
pub fn get_anthropic_max_output(model: &str) -> u32 {
    let m = model.to_lowercase();
    ANTHROPIC_OUTPUT_LIMITS.iter()
        .filter(|(key, _)| m.contains(key))
        .max_by_key(|(key, _)| key.len())
        .map_or(ANTHROPIC_DEFAULT_OUTPUT_LIMIT, |(_, v)| *v)
}
```

### 7.2 Claude Code OAuth Token Management

```rust
pub struct ClaudeCodeCredentials {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at_ms: u64,
}

/// Token resolution chain:
/// 1. ANTHROPIC_API_KEY env var
/// 2. Claude Code OAuth credentials (~/.claude/credentials.json)
/// 3. Claude managed key (~/.claude/.credentials.json)
pub fn resolve_anthropic_token() -> Option<String> {
    // Check env first
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        if !key.is_empty() { return Some(key); }
    }
    // Try Claude Code OAuth with refresh
    if let Some(creds) = read_claude_code_credentials() {
        if is_claude_code_token_valid(&creds) {
            return Some(creds.access_token);
        }
        // Attempt token refresh
        if let Some(refreshed) = refresh_oauth_token(&creds) {
            return Some(refreshed);
        }
    }
    // Try managed key
    read_claude_managed_key()
}

pub fn is_oauth_token(key: &str) -> bool {
    key.starts_with("sk-ant-oat-") // OAuth access token prefix
}
```

## 8. Billing Route & Cost Tracking (v0.4.0)

### 8.1 Canonical Usage

```rust
/// Normalized usage across all providers
pub struct CanonicalUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
}

impl CanonicalUsage {
    pub fn prompt_tokens(&self) -> u64 {
        self.input_tokens + self.cache_read_tokens + self.cache_write_tokens
    }
    pub fn total_tokens(&self) -> u64 {
        self.prompt_tokens() + self.output_tokens + self.reasoning_tokens
    }
}
```

### 8.2 Billing Route Resolution

```rust
pub struct BillingRoute {
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub billing_mode: String,  // matches CostSource labels
}
```

### 8.3 Pricing Entry & Cost Source

```rust
/// Per-model pricing snapshot (per million tokens)
pub struct PricingEntry {
    pub input_cost_per_million: Option<Decimal>,
    pub output_cost_per_million: Option<Decimal>,
    pub cache_read_cost_per_million: Option<Decimal>,
    pub cache_write_cost_per_million: Option<Decimal>,
    pub request_cost: Option<Decimal>,
    pub source: CostSource,
    pub source_url: Option<String>,
    pub pricing_version: Option<String>,
    pub fetched_at: Option<DateTime<Utc>>,
}

/// Where pricing data came from
pub enum CostSource {
    ProviderCostApi,          // provider reports actual cost
    ProviderGenerationApi,    // cost from generation metadata
    ProviderModelsApi,        // OpenRouter /models endpoint
    OfficialDocsSnapshot,     // hardcoded from provider docs
    UserOverride,             // user-configured pricing
    CustomContract,           // enterprise contract pricing
    None,
}

/// How confident we are in the cost figure
pub enum CostStatus {
    Actual,       // provider-reported actual cost
    Estimated,    // computed from known pricing table
    Included,     // subscription-included ($0)
    Unknown,      // no pricing data available
}
```

### 8.4 Cost Result

```rust
pub struct CostResult {
    pub amount_usd: Option<Decimal>,
    pub status: CostStatus,
    pub source: CostSource,
    pub label: String,
    pub fetched_at: Option<DateTime<Utc>>,
    pub pricing_version: Option<String>,
    pub notes: Vec<String>,
}
```
```
