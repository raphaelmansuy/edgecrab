# Makefile — SimCity Builder dev server & tooling
# Usage:
#   make serve      # start a local static server on http://127.0.0.1:8000
#   make open       # print the URL to open in a browser
#   make stop       # stop the running server
#   make smoke      # run the headless simulation smoke test (node)
#   make clean      # remove the server pid file

PORT ?= 8000
HOST ?= 127.0.0.1
DIR  := $(CURDIR)
PIDFILE := $(DIR)/.simcity-server.pid

.PHONY: serve open stop smoke clean

serve:
	@echo "Starting SimCity Builder on http://$(HOST):$(PORT)/"
	@(python3 -m http.server $(PORT) --bind $(HOST) --directory "$(DIR)" >/tmp/simcity-server.log 2>&1 & echo $$! > $(PIDFILE))
	@sleep 1
	@curl -sS -o /dev/null -w "index.html -> HTTP %{http_code}\n" http://$(HOST):$(PORT)/ || echo "server did not respond"
	@echo "Open: http://$(HOST):$(PORT)/"

open:
	@echo "http://$(HOST):$(PORT)/"

stop:
	@if [ -f "$(PIDFILE)" ]; then kill "$$(cat $(PIDFILE))" 2>/dev/null && echo "server stopped"; rm -f $(PIDFILE); else echo "no server pid file found"; fi

smoke:
	@node smoketest.js

clean:
	@rm -f $(PIDFILE)
	@echo "cleaned"
