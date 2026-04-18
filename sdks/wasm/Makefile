OLLAMA_BASE ?= http://localhost:11434/v1

.PHONY: help build-node e2e

help:
	@echo "Targets: build-node, e2e"

build-node:
	wasm-pack build --target nodejs --out-dir pkg-node

e2e: build-node
	ANTHROPIC_BASE_URL=$(OLLAMA_BASE) ANTHROPIC_AUTH_TOKEN=ollama node examples/e2e_smoke.mjs
