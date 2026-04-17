MODEL ?= ollama/gemma4:latest
OLLAMA_BASE ?= http://localhost:11434

.PHONY: help basic full batch e2e

help:
	@echo "Targets: basic, full, batch, e2e"

basic:
	cargo run --example basic_usage -p edgecrab-sdk

full:
	cargo run --example full_conversation -p edgecrab-sdk

batch:
	cargo run --example batch_processing -p edgecrab-sdk

e2e:
	ANTHROPIC_BASE_URL=$(OLLAMA_BASE) ANTHROPIC_AUTH_TOKEN=ollama cargo run --example e2e_smoke -p edgecrab-sdk
