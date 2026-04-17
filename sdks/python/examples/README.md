# EdgeCrab Python Examples

## WHY

These examples are for people who want the Python SDK to feel friendly, practical, and immediately useful.

They help you go from **"I wonder how this works"** to **"I just ran a real agent locally"** with as little friction as possible.

Use them when you want to:

- start from a tiny async example
- explore a more realistic workflow without a lot of setup
- test against local Ollama before spending money on provider calls
- borrow patterns for sessions, memory, and orchestration

## WHAT

### Best places to start

- Want the easiest entry point? Run `basic_usage.py`
- Want a business-ready workflow fast? Run `business_case_showcase.py`
- Want something closer to an app? Read `session_aware_support.py`
- Want the proof-oriented check? Run `e2e_smoke.py`

### Prefer a guided path?

- [getting-started/README.md](getting-started/README.md) — the shortest route to a working Python agent
- [business/README.md](business/README.md) — practical examples you can show to a team or customer
- [e2e/README.md](e2e/README.md) — what the proof run covers and how to verify it locally

### Example map

| Example | Best for | What you’ll learn |
| --- | --- | --- |
| `basic_usage.py` | First success | A tiny async starter with one chat call |
| `business_case_showcase.py` | Business workflows | Support triage, sales prep, and executive updates |
| `cost_aware_review.py` | Budget-sensitive work | Cost-aware model choice |
| `parallel_research.py` | Broader investigation | Concurrent research tasks |
| `multi_agent_pipeline.py` | Workflow design | Multi-agent orchestration |
| `session_aware_support.py` | Helpful assistants | Session and memory reuse |
| `safe_sql_agent.py` | Safer tools | Guarded tool interaction patterns |
| `e2e_smoke.py` | Proof | Core Python SDK verification against local Ollama |

## HOW

### What happens under the hood

```text
Your Python code
      |
      v
   AsyncAgent
      |
      +--> chat / run / stream
      +--> memory / sessions
      +--> fork / batch
      |
      v
Local Ollama or your chosen provider
```

### Setup once

```bash
cd sdks/python
python3 -m pip install -U maturin
maturin develop
ollama pull gemma4:latest
```

### First run

```bash
python3 examples/basic_usage.py
```

### Next, try a richer workflow

```bash
python3 examples/session_aware_support.py
python3 examples/parallel_research.py
```

### Want the proof-oriented validation?

```bash
cd sdks/python
make e2e
```

### Suggested path

1. `basic_usage.py`
2. `session_aware_support.py`
3. `multi_agent_pipeline.py`
4. `e2e_smoke.py`
