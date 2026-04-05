---
title: Docker Deployment
description: Run EdgeCrab as a containerized gateway server. Full docker-compose setup, volume mounting, environment variables, and production deployment guide.
sidebar:
  order: 4
---

EdgeCrab ships a multi-stage Docker image that runs the HTTP gateway (`edgecrab-gateway`). This is the recommended deployment method for team use, CI/CD pipelines, and server environments.

---

## Quick Start

```bash
docker pull ghcr.io/raphaelmansuy/edgecrab:latest
docker run --rm -it \
  -p 8642:8642 \
  -e OPENAI_API_KEY="$OPENAI_API_KEY" \
  -v ~/.edgecrab:/root/.edgecrab \
  ghcr.io/raphaelmansuy/edgecrab:latest
```

Visit `http://localhost:8642/health` to verify the gateway is running.

---

## docker-compose (recommended)

The repository includes a `docker-compose.yml`:

```yaml
# docker-compose.yml
version: '3.9'
services:
  edgecrab:
    image: ghcr.io/raphaelmansuy/edgecrab:latest
    restart: unless-stopped
    ports:
      - "8642:8642"
    environment:
      - OPENAI_API_KEY=${OPENAI_API_KEY}
      - ANTHROPIC_API_KEY=${ANTHROPIC_API_KEY}
      - EDGECRAB_PROVIDER=openai
      - EDGECRAB_MODEL=gpt-4o
    volumes:
      - edgecrab-data:/root/.edgecrab
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8642/health"]
      interval: 30s
      timeout: 10s
      retries: 3

volumes:
  edgecrab-data:
```

```bash
# Start
docker compose up -d

# View logs
docker compose logs -f edgecrab

# Stop
docker compose down
```

---

## Building the Image Locally

```bash
git clone https://github.com/raphaelmansuy/edgecrab
cd edgecrab
docker build -t edgecrab:local .
docker run -p 8642:8642 -e OPENAI_API_KEY="$OPENAI_API_KEY" edgecrab:local
```

The `Dockerfile` uses a multi-stage build:
1. **Builder stage**: `rust:1.85-slim` — compiles all crates
2. **Runtime stage**: `debian:bookworm-slim` — adds only the binary and runtime libs (~25 MB final image)

---

## Environment Variables

All EdgeCrab configuration can be driven by environment variables inside Docker:

| Variable | Description |
|----------|-------------|
| `EDGECRAB_PROVIDER` | Active LLM provider (e.g. `openai`) |
| `EDGECRAB_MODEL` | Model name (e.g. `gpt-4o`) |
| `EDGECRAB_LOG_LEVEL` | Log verbosity: `trace`/`debug`/`info`/`warn`/`error` |
| `OPENAI_API_KEY` | OpenAI API key |
| `ANTHROPIC_API_KEY` | Anthropic API key |
| `GITHUB_TOKEN` | GitHub Copilot token |
| `GOOGLE_API_KEY` | Google Gemini API key |
| `XAI_API_KEY` | xAI Grok API key |
| `DEEPSEEK_API_KEY` | DeepSeek API key |
| `HUGGING_FACE_HUB_TOKEN` | Hugging Face API token |
| `ZAI_API_KEY` | Z.AI API key |

Pass them via `--env-file`:

```bash
docker run --env-file ~/.edgecrab/.env -p 8642:8642 ghcr.io/raphaelmansuy/edgecrab:latest
```

---

## Volume Mounts

| Container path | Purpose |
|---------------|---------|
| `/root/.edgecrab` | All state: config, memories, skills, SQLite DB |

Mount this to a named volume or host path to persist data across container restarts:

```bash
docker run \
  -v /data/edgecrab:/root/.edgecrab \
  -p 8642:8642 \
  ghcr.io/raphaelmansuy/edgecrab:latest
```

---

## Gateway API Endpoints

The `edgecrab-gateway` exposes an OpenAI-compatible HTTP API:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check |
| `/v1/chat/completions` | POST | OpenAI-compatible chat |
| `/v1/models` | GET | List available models |

This means any tool that supports the OpenAI API can connect to EdgeCrab as a backend.

---

## Running the Interactive TUI via Docker

For one-off interactive sessions inside a container:

```bash
docker run --rm -it \
  -e OPENAI_API_KEY="$OPENAI_API_KEY" \
  -v ~/.edgecrab:/root/.edgecrab \
  ghcr.io/raphaelmansuy/edgecrab:latest \
  edgecrab
```

:::tip
When running the TUI in Docker, ensure your terminal emulator passes through UTF-8 and supports 256-color or true-color output for the best ratatui rendering.
:::

---

## Production Checklist

- [ ] Mount `/root/.edgecrab` to a persistent volume
- [ ] Pass API keys via `--env-file` (never bake them into the image)
- [ ] Set `restart: unless-stopped` or `restart: always`
- [ ] Expose port 8642 only on `127.0.0.1` if behind a reverse proxy
- [ ] Add a health check (`/health`)
- [ ] Set `EDGECRAB_LOG_LEVEL=warn` to reduce log noise in production

---

## Pro Tips

**Use `--env-file`, never `--env`.** Passing secrets via `--env VAR=value` leaks them to `docker ps` and process listings. `--env-file ~/.edgecrab/.env` is safe.

**Check memory usage.** EdgeCrab is lightweight (~15 MB resident), so you can run multiple instances on a single host without resource pressure. Set a memory limit anyway for hygiene:
```yaml
services:
  edgecrab:
    deploy:
      resources:
        limits:
          memory: 256M
```

**Run `edgecrab doctor` inside the container after first deploy:**
```bash
docker compose exec edgecrab edgecrab doctor
```
This confirms API keys are visible and the provider ping succeeds.

---

## Frequently Asked Questions

**Q: The container starts but the gateway doesn't receive messages.**

Check that:
1. The platform tokens are set in the env file
2. Port 8642 is exposed and not blocked by firewall
3. `edgecrab gateway status` (run inside container) shows the platform as active

**Q: Can I run EdgeCrab in Kubernetes?**

Yes. Use a Deployment with one replica, a PersistentVolumeClaim for `/root/.edgecrab`, and a Secret for the API keys. EdgeCrab has no clustering or leader-election requirements — it's a stateful single-process app.

**Q: I updated the Docker image but my data was lost.**

Ensure the volume mount is set up correctly before the first run. The container's internal `/root/.edgecrab` must be mounted to a persistent location. If no volume is mounted, all data is lost when the container stops.

**Q: How do I run the TUI inside a running container?**

```bash
docker exec -it edgecrab-container edgecrab
```
The TUI works inside Docker as long as the container has a TTY (`-it` or `tty: true` in compose).

**Q: Can I use Ollama inside Docker with EdgeCrab?**

Yes. Run Ollama in a separate container and set:
```yaml
environment:
  EDGECRAB_MODEL: "ollama/llama3.3"
  OLLAMA_HOST: "http://ollama:11434"
```
With a `depends_on: [ollama]` in your compose file.

---

## See Also

- [Self-Hosting Guide](/guides/self-hosting/) — Reverse proxy, SSL, and monitoring
- [Configuration](/user-guide/configuration/) — All config options
- [Security Model](/user-guide/security/) — Security considerations for server deployment
- [Messaging Gateway](/user-guide/messaging/) — Gateway platform configuration
