# EdgeCrab Python SDK

## WHY

Use this SDK when you want Python productivity with the Rust runtime doing the heavy lifting.

This is the canonical EdgeCrab Python SDK publication path.

It is designed for:

- async or sync agent workflows
- local Ollama validation before paid-provider rollout
- session-aware applications
- memory-backed assistants and research tools

## WHAT

Core capabilities:

- native Rust-backed runtime via PyO3
- sync and async agents
- streaming callbacks
- persistent sessions and memory
- profile-aware configuration via `Config.load_profile()`

### Architecture

```text
Python app
   |
   v
PyO3 bindings
   |
   v
EdgeCrab Rust runtime
   |
   +--> sessions
   +--> memory
   +--> tools
   |
   v
LLM provider or local Ollama
```

## HOW

### Quick start

```bash
python3 -m pip install -U edgecrab
```

```python
from edgecrab import Agent

agent = Agent(model="copilot/gpt-5-mini")
reply = agent.chat_sync("Hello")
print(reply)
```

### Development and examples

See the full examples guide in `examples/README.md`.

```bash
cd sdks/python
python3 -m pip install -U maturin
maturin develop
make e2e
```

## SDK Tutorials

All tutorials ship with working Python examples in `examples/`.

| Tutorial | Example | Model |
| --- | --- | --- |
| [1. Cost-Aware Code Review](../../site/src/content/docs/tutorials/01-cost-aware-review.md) | `examples/cost_aware_review.py` | openai/gpt-5 + copilot/gpt-5-mini |
| [2. Parallel Research Pipeline](../../site/src/content/docs/tutorials/02-parallel-research.md) | `examples/parallel_research.py` | copilot/gpt-5-mini |
| [3. Multi-Agent Pipeline](../../site/src/content/docs/tutorials/03-multi-agent-pipeline.md) | `examples/multi_agent_pipeline.py` | openai/gpt-4o + copilot/gpt-5-mini |
| [4. Session-Aware Support Bot](../../site/src/content/docs/tutorials/04-session-aware-support.md) | `examples/session_aware_support.py` | copilot/gpt-5-mini |
| [5. Safe SQL Agent](../../site/src/content/docs/tutorials/05-custom-tool-safe-sql.md) | `examples/safe_sql_agent.py` | copilot/gpt-5-mini |
