# 05 — P2: Consecutive Failure Escalation

**Priority**: P2
**Impact**: Prevents budget-burning error spirals; forces user feedback loop
**Risk**: Medium — must not trigger on benign single failures
**Cross-ref**: [01-diagnosis.md](01-diagnosis.md) RC-5

## WHY This Is P2

```
CURRENT BEHAVIOR (no escalation):

    Tool fails -> error in messages -> LLM retries
         |                                   |
         v                                   v
    Tool fails again -> suppression fires -> LLM tries workaround
         |                                   |
         v                                   v
    Workaround fails -> LLM tries another -> ... (loop continues)
         |                                   |
         v                                   v
    Budget exhausted (90 iterations) -> "I was unable to complete..."

    Total cost: $5-15 in API calls for no result
```

```
TARGET BEHAVIOR (with escalation):

    Tool fails -> error with guidance -> LLM retries
         |
         v
    Tool fails again (consecutive=2) -> LLM tries different approach
         |
         v
    Third failure (consecutive=3) -> ESCALATE TO USER
         |
         v
    "I'm having trouble with X. The error is Y.
     Would you like me to try a different approach?"
         |
         v
    User provides guidance -> agent resumes with new context
    
    Total cost: $0.30-0.50 in API calls
```

## Implementation

### File: `crates/edgecrab-core/src/conversation.rs`

Add a `ConsecutiveFailureTracker` struct:

```rust
/// Tracks consecutive tool failures to detect stuck loops.
/// Reset on any successful tool call.
struct ConsecutiveFailureTracker {
    count: u32,
    max_before_escalation: u32,
    last_errors: Vec<String>,
}

impl ConsecutiveFailureTracker {
    fn new(max: u32) -> Self {
        Self { count: 0, max_before_escalation: max, last_errors: Vec::new() }
    }

    /// Record a tool error. Returns true if escalation threshold reached.
    fn record_failure(&mut self, error_summary: &str) -> bool {
        self.count += 1;
        self.last_errors.push(error_summary.to_string());
        if self.last_errors.len() > 5 {
            self.last_errors.remove(0);
        }
        self.count >= self.max_before_escalation
    }

    /// Reset on successful tool call.
    fn record_success(&mut self) {
        self.count = 0;
        self.last_errors.clear();
    }

    fn should_escalate(&self) -> bool {
        self.count >= self.max_before_escalation
    }

    fn escalation_message(&self) -> String {
        let errors = self.last_errors.join("\n  - ");
        format!(
            "I've encountered {} consecutive tool errors and need your guidance.\n\
             Recent errors:\n  - {}\n\
             Would you like me to try a different approach, or can you help resolve the issue?",
            self.count, errors
        )
    }
}
```

### Integration with execute_loop

```rust
// In execute_loop(), after process_response dispatches tools:
let mut failure_tracker = ConsecutiveFailureTracker::new(3);

// After each tool dispatch result:
if result.is_error {
    if failure_tracker.record_failure(&result.error_summary) {
        // Force the assistant to ask the user
        messages.push(Message::system(failure_tracker.escalation_message()));
        failure_tracker.record_success(); // reset after escalation
    }
} else {
    failure_tracker.record_success();
}
```

## Edge Cases

1. **Mixed success/failure batches**: If 3 tools run in parallel and 2 fail
   but 1 succeeds, the success resets the counter. This is intentional —
   the agent is making partial progress.

2. **Suppressed retries**: Suppressed retries count as failures (they indicate
   the agent is repeating the same mistake).

3. **Benign single failures**: A single InvalidArgs followed by a correct
   retry does NOT trigger escalation (count resets to 0 on success).

4. **Subagent isolation**: Each subagent gets its own tracker. Parent agent's
   tracker is not affected by child failures.

5. **Escalation message injection**: The escalation message is injected as a
   system message, not a user message. This prevents the LLM from thinking
   the user said it.

6. **Config override**: `max_consecutive_failures` in config.yaml (default: 3).
   Set to 0 to disable escalation.
