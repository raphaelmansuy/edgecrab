# Python SDK — Getting Started

## WHY

This path is designed for the shortest route to a working Python agent.

## WHAT TO RUN

1. [../basic_usage.py](../basic_usage.py) — first successful agent call
2. [../session_aware_support.py](../session_aware_support.py) — memory and sessions in practice
3. [../parallel_research.py](../parallel_research.py) — concurrent work patterns

## HOW

```bash
cd sdks/python
maturin develop
ollama pull gemma4:latest
python3 examples/basic_usage.py
```

Once you have a first win, continue with [../business/README.md](../business/README.md).
