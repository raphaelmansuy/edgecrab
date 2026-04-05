# 003.002 — Conversation Loop

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 003.001 Agent Struct](001_agent_struct.md) | [→ 003.003 Prompt Builder](003_prompt_builder.md) | [→ 003.004 Context Compression](004_context_compression.md)
> **Source**: `edgecrab-core/src/conversation.rs` — verified against real implementation

## 1. Entry Point

The public API is `Agent::run_conversation()` (see [003.001](001_agent_struct.md#6-public-api)). Internally it delegates to `execute_loop()`, which contains the full ReAct cycle:

```rust
// edgecrab-core/src/conversation.rs

impl Agent {
    pub(crate) async fn execute_loop(
        &self,
        user_message: &str,
        system_message: Option<&str>,
        history: Option<Vec<Message>>,
        event_tx: Option<&UnboundedSender<StreamEvent>>,
    ) -> Result<ConversationResult, AgentError>
}
```

## 2. High-Level Algorithm

```
execute_loop(user_message, system_message?, history?, event_tx?)
│
├── 1. Reset iteration budget
├── 2. Reset cancel token (fresh CancellationToken if prior was cancelled)
├── 3. Snapshot config + provider (immune to /model hot-swap mid-loop)
├── 4. Seed session.messages from history (gateway: fresh Agent per msg)
├── 5. Resolve cwd, expand toolset configuration
├── 6. Expand @context references (@file, @url, @diff, @staged, @folder, @git)
│       ├── Parse REFERENCE_PATTERN from user message
│       ├── Security check (block sensitive paths: .ssh, .aws, etc.)
│       └── Inject content, track injected tokens
├── 7. Classify message complexity (→ model routing hint)
├── 8. Build/restore cached system prompt (first turn only)
│       └── PromptBuilder: identity + platform + memory + skills + SOUL.md
├── 9. Preflight context compression (if tokens > threshold)
│
├── 10. MAIN LOOP (while budget.try_consume() succeeds):
│       ├── a. Check cancel token → break if cancelled
│       ├── b. Prepare api_messages:
│       │       ├── Copy reasoning → reasoning_content for API
│       │       ├── Strip trajectory-only fields
│       │       ├── Sanitize for strict APIs (Mistral)
│       │       ├── Prepend system message
│       │       ├── Apply Anthropic prompt caching
│       │       └── Sanitize orphaned tool results
│       ├── c. Resolve turn route (primary vs cheap model)
│       ├── d. API call (streaming path):
│       │       ├── Retry loop (3 retries, exponential backoff 500ms base)
│       │       ├── Fallback model activation on repeated failures
│       │       └── Handle finish_reason: stop | tool_calls | length
│       ├── e. Process response → LoopAction:
│       │       ├── tool_calls → dispatch → append results → Continue
│       │       └── text only → Done(response_text)
│       ├── f. Context compression (if tokens exceed threshold mid-loop)
│       └── g. Post-iteration: track usage, update session state
│
├── 11. [if ≥5 tool calls] Learning reflection (closed learning loop)
│       └── Agent may call skill_manage / memory_write
│
├── 12. Post-loop:
│       ├── Persist session to SQLite
│       ├── Save trajectory (if enabled)
│       └── Return ConversationResult
```

## 3. LoopAction

```rust
/// What happened after processing one API response.
enum LoopAction {
    /// Tool calls were dispatched — loop again for the next LLM response.
    Continue,
    /// LLM produced a final text response — exit the loop.
    Done(String),
}
```

> **Note**: There is no `CompressAndRetry` variant. Context compression is handled as a separate check within the loop body — if needed, compression runs and then the loop naturally continues with `Continue`.

## 4. DispatchContext

Groups shared dispatch parameters to avoid `clippy::too_many_arguments`:

```rust
struct DispatchContext<'a> {
    registry: Option<&'a Arc<ToolRegistry>>,
    cancel: &'a CancellationToken,
    state_db: &'a Option<Arc<SessionDb>>,
    platform: Platform,
    process_table: &'a Arc<ProcessTable>,
    provider: Option<Arc<dyn LLMProvider>>,
    tool_registry_arc: Option<Arc<ToolRegistry>>,
    sub_agent_runner: Option<Arc<dyn SubAgentRunner>>,
    clarify_tx: Option<UnboundedSender<ClarifyRequest>>,
    origin_chat: Option<(String, String)>,
    config_ref: AppConfigRef,
    conversation_session_id: String,
}
```

## 5. Tool Dispatch

### Parallel vs Sequential

```rust
fn should_parallelize(calls: &[ToolCall], registry: &ToolRegistry) -> bool {
    if calls.len() <= 1 { return false; }

    // Check parallel_safe() on each tool handler
    // Check for path overlap on file-scoped tools
    // Reject if any tool is in NEVER_PARALLEL set
}
```

### Parallel Execution (via JoinSet)

```rust
async fn dispatch_parallel(
    calls: Vec<ToolCall>,
    registry: &ToolRegistry,
    ctx: &ToolContext,
) -> Vec<ToolResult> {
    let mut set = JoinSet::new();
    for call in calls {
        set.spawn(async move { registry.dispatch(&call, &ctx).await });
    }
    // Collect results as they complete
}
```

## 6. Retry & Fallback

| Scenario | Strategy |
|----------|----------|
| Empty response | 3 retries, exponential backoff (500ms → 1s → 2s) |
| Rate limit (429) | Activate fallback model immediately |
| Service unavailable (503) | Activate fallback model immediately |
| 3× consecutive errors | Activate fallback model |
| Context overflow | Compress + continue loop (up to 3 passes) |
| Length truncation | Continue loop for continuation |
| Auth failure | Re-auth via edgequake-llm token refresh |

Constants:

```rust
const MAX_RETRIES: u32 = 3;
const BASE_BACKOFF: Duration = Duration::from_millis(500);
```

## 7. Closed Learning Loop

After sessions with 5+ tool calls, `execute_loop` appends a reflection prompt. The agent can then:
- Save a reusable workflow via `skill_manage(action='create', ...)`
- Patch an outdated skill via `skill_manage(action='patch', ...)`
- Record project/user facts via `memory_write`

```rust
/// Minimum tool-call count before end-of-session learning reflection fires.
const SKILL_REFLECTION_THRESHOLD: u32 = 5;
```

This mirrors hermes-agent's self-improvement architecture. The SKILLS_GUIDANCE constant in `prompt_builder.rs` provides a proactive nudge during the session; the explicit reflection step provides a reliable second trigger.

## 8. Interrupt Handling

Interrupts are cooperative via `CancellationToken` (see [002.003](../002_architecture/003_concurrency_model.md)):

```rust
// Checked at:
// 1. Top of each loop iteration
// 2. Inside API call retry loop (between retries)
// 3. Inside streaming response consumption (between chunks)

// CancellationToken is RESET at execute_loop start:
let cancel = {
    let mut guard = self.cancel.lock().expect("cancel mutex not poisoned");
    if guard.is_cancelled() {
        *guard = CancellationToken::new();  // fresh token for this turn
    }
    guard.clone()
};
```

## 9. Checkpoint Manager

After each tool dispatch iteration, EdgeCrab can snapshot the working directory for rollback:

```rust
// Config-driven: checkpoints_enabled (default true), max 50 snapshots per dir
// Triggered after tool results are appended to messages
// Rollback via /rollback slash command
```
