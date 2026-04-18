MODEL ?= ollama/gemma4:latest
OLLAMA_BASE ?= http://localhost:11434
PYTHON ?= python3

.PHONY: help build basic e2e

help:
	@echo "Targets: build, basic, e2e"

build:
	$(PYTHON) -m pip install -U maturin
	maturin develop

basic:
	$(PYTHON) examples/basic_usage.py

e2e:
	ANTHROPIC_BASE_URL=$(OLLAMA_BASE) ANTHROPIC_AUTH_TOKEN=ollama $(PYTHON) examples/e2e_smoke.py
