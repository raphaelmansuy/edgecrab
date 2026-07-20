# 🐧 Pinguin Slide Party - Makefile

.PHONY: start stop dev serve open help clean

# Default values
PORT ?= 8081
HOST ?= localhost

## start - Start the game server
start:
	@echo "🐧 Starting Pinguin Slide Party on http://$(HOST):$(PORT) ..."
	@python3 -m http.server $(PORT)

## dev - Start with auto-reload (using browser-sync if available)
dev:
	@if command -v browser-sync >/dev/null 2>&1; then \
		echo "🚀 Starting with live-reload..."; \
		browser-sync start --server --files="*.html,*.css,js/*.js" --port $(PORT); \
	else \
		echo "⚠️  browser-sync not found, falling back to basic server"; \
		python3 -m http.server $(PORT); \
	fi

## serve - Start server in background, show URL
serve:
	@echo "🐧 Server running at http://$(HOST):$(PORT)"
	@python3 -m http.server $(PORT)

## open - Start server and open browser
open:
	@echo "🐧 Starting server and opening browser..."
	@open http://$(HOST):$(PORT) &
	@python3 -m http.server $(PORT)

## stop - Stop any background server on the port
stop:
	@lsof -ti:$(PORT) | xargs kill -9 2>/dev/null || echo "No process on port $(PORT)"

## clean - Remove any temp files
clean:
	@echo "🧹 Clean complete"

## help - Show this help
help:
	@echo "🐧 Pinguin Slide Party - Available commands:"
	@echo "  make start   - Start the game server (default port: 8081)"
	@echo "  make dev     - Start with live-reload (requires browser-sync)"
	@echo "  make serve   - Start server in background"
	@echo "  make open    - Start server and open browser automatically"
	@echo "  make stop    - Stop any background server on the port"
	@echo "  make clean   - Remove temp files"
	@echo ""
	@echo "Environment variables:"
	@echo "  PORT - Server port (default: 8081)"
	@echo "  HOST - Server host (default: localhost)"
