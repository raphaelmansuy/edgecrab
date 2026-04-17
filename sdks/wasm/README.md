# EdgeCrab WASM SDK

## WHY

The WASM SDK exists for browser and edge runtimes where native file, terminal, and OS-bound tools are not available.

Use it when you want to:

- embed an agent into a web app or edge function
- keep the runtime small and deployable
- use custom JavaScript tools safely
- verify browser-friendly behavior against local Ollama before shipping

## WHAT

The WASM SDK provides:

- `Agent` for chat and streaming
- `Tool` for JavaScript-native tool registration
- `MemoryManager` for in-memory persistence
- a Node-based E2E smoke proof in [examples/README.md](examples/README.md)

### Architecture

```text
Browser / Node / Edge runtime
            |
            v
        WASM Agent
            |
            +--> custom JS tools
            +--> in-memory state
            |
            v
     OpenAI-compatible API
     (local Ollama supported)
```

## HOW

### Build for Node-based local verification

```bash
cd sdks/wasm
wasm-pack build --target nodejs --out-dir pkg-node
```

### Run the local Ollama proof

```bash
OPENAI_BASE_URL=http://localhost:11434/v1 \
OPENAI_API_KEY=ollama \
node examples/e2e_smoke.mjs
```

### Shortcut

```bash
cd sdks/wasm
make e2e
```
