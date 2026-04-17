# EdgeCrab Rust SDK

## WHY

This crate gives Rust applications direct access to the EdgeCrab agent runtime without going through a CLI or external bridge.

Use it when you want:

- a strongly typed builder API
- direct control over tools, memory, and sessions
- first-class async Rust ergonomics
- repeatable local Ollama validation before production rollout

## WHAT

The Rust SDK includes:

- `SdkAgent` for chat, run, stream, batch, and fork workflows
- `SdkConfig` for profile-aware configuration
- `SdkSession` and `MemoryManager` for persistence
- `SdkToolRegistry` for custom tools
- runnable examples in [examples/README.md](examples/README.md)

### Start here if you want business value fast

| Goal | Best example |
| --- | --- |
| Show a practical operations copilot | `business_case_showcase.rs` |
| Review work without overspending | `cost_aware_review.rs` |
| Build a support assistant with memory | `session_aware_support.rs` |
| Validate the core SDK surface locally | `e2e_smoke.rs` |

### Verified proof status

The SDK examples are backed by fresh local Ollama verification across all targets:

| Target | Proof result |
| --- | --- |
| Rust | 34/34 passed |
| Node.js | 30/30 passed |
| Python | 28/28 passed |
| WASM | 17/17 passed |

### Architecture

```text
Rust application
      |
      v
  SdkAgent builder
      |
      +--> config
      +--> tools
      +--> sessions
      +--> memory
      |
      v
Agent runtime and provider calls
```

## HOW

### Quick start

```bash
cargo run --example basic_usage -p edgecrab-sdk
cargo run --example business_case_showcase -p edgecrab-sdk
```

### Local proof with Ollama

```bash
cd crates/edgecrab-sdk
make e2e
```

### Examples

The full example guide lives in [examples/README.md](examples/README.md).
