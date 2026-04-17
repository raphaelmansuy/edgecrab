MODEL ?= ollama/gemma4:latest
OLLAMA_BASE ?= http://localhost:11434

.PHONY: help install build basic e2e

help:
	@echo "Targets: install, build, basic, e2e"

install:
	npm install

build:
	npm run build

basic:
	node examples/basic_usage.mjs

e2e:
	ANTHROPIC_BASE_URL=$(OLLAMA_BASE) ANTHROPIC_AUTH_TOKEN=ollama node examples/e2e_smoke.mjs
