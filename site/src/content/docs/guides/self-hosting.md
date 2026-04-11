---
title: Self-Hosting with Docker
description: Production-ready EdgeCrab gateway deployment with docker-compose, Nginx reverse proxy, TLS, health monitoring, and log management.
sidebar:
  order: 3
---

Deploy EdgeCrab as a persistent gateway service that your team or tooling can connect to over HTTP. This guide covers a production-ready setup with Nginx, TLS, health checks, and monitoring.

---

## Architecture

```
                    Internet / LAN
                         │
                    Nginx (TLS)
                         │ :443 → :8642
                    EdgeCrab Gateway
                         │
                    LLM Providers
                   (OpenAI, Anthropic, …)
```

---

## Prerequisites

- Docker and Docker Compose installed
- A domain name (for TLS) or IP address (for HTTP-only internal use)
- API keys for your LLM provider(s)

---

## Step 1 — Clone and Configure

```bash
git clone https://github.com/raphaelmansuy/edgecrab
cd edgecrab
cp .env.example .env
```

Edit `.env`:

```bash
# .env
OPENAI_API_KEY=sk-...
ANTHROPIC_API_KEY=sk-ant-...
EDGECRAB_PROVIDER=openai
EDGECRAB_MODEL=gpt-4o
EDGECRAB_LOG_LEVEL=info
```

---

## Step 2 — docker-compose.yml

```yaml
# docker-compose.yml
version: '3.9'

services:
  edgecrab:
    image: ghcr.io/raphaelmansuy/edgecrab:latest
    restart: unless-stopped
    env_file: .env
    ports:
      - "127.0.0.1:8642:8642"    # Bind to localhost only — Nginx handles external
    volumes:
      - edgecrab-data:/root/.edgecrab
    healthcheck:
      test: ["CMD", "curl", "-sf", "http://localhost:8642/health"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 10s
    logging:
      driver: json-file
      options:
        max-size: "10m"
        max-file: "5"

volumes:
  edgecrab-data:
    driver: local
```

Start:

```bash
docker compose up -d
docker compose logs -f edgecrab
```

Verify:

```bash
curl http://localhost:8642/health
# {"status":"ok","version":"<current-version>","provider":"openai","model":"gpt-4o"}
```

---

## Step 3 — Nginx Reverse Proxy (Optional)

For TLS termination and a clean hostname:

```nginx
# /etc/nginx/sites-available/edgecrab
server {
    listen 443 ssl http2;
    server_name agent.yourdomain.com;

    ssl_certificate     /etc/letsencrypt/live/agent.yourdomain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/agent.yourdomain.com/privkey.pem;
    ssl_protocols       TLSv1.2 TLSv1.3;
    ssl_session_cache   shared:SSL:10m;

    # Proxy to EdgeCrab
    location / {
        proxy_pass         http://127.0.0.1:8642;
        proxy_http_version 1.1;
        proxy_set_header   Host $host;
        proxy_set_header   X-Real-IP $remote_addr;
        proxy_set_header   Upgrade $http_upgrade;
        proxy_set_header   Connection "upgrade";
        proxy_read_timeout 300s;   # Allow long streaming responses
        proxy_buffering    off;    # Required for streaming
    }
}

server {
    listen 80;
    server_name agent.yourdomain.com;
    return 301 https://$host$request_uri;
}
```

Enable and test:

```bash
sudo ln -s /etc/nginx/sites-available/edgecrab /etc/nginx/sites-enabled/
sudo nginx -t
sudo systemctl reload nginx
```

---

## Step 4 — TLS with Let's Encrypt

```bash
sudo certbot --nginx -d agent.yourdomain.com
```

---

## Step 5 — Persistent Data

The `edgecrab-data` volume stores all persistent state. Back it up regularly:

```bash
# Backup volume to a tar file
docker run --rm \
  -v edgecrab-data:/root/.edgecrab \
  -v $(pwd):/backup \
  alpine tar czf /backup/edgecrab-backup-$(date +%Y%m%d).tar.gz /root/.edgecrab
```

---

## Step 6 — Health Monitoring

### Uptime checks

Configure your monitoring tool (UptimeRobot, Checkly, Grafana on-call) to hit:

```
GET https://agent.yourdomain.com/health
Expected: 200 OK
```

### Alerting on restarts

```bash
# Check how many times the container has restarted
docker inspect edgecrab --format '{{.RestartCount}}'
```

---

## Updating

```bash
docker compose pull
docker compose up -d
```

Rolling update with zero downtime requires a load balancer in front of multiple instances — outside the scope of this guide for single-node deployments.

---

## Securing the Deployment

- **Never expose port 8642 directly to the internet** — always use Nginx or another reverse proxy
- **Use `--bind 127.0.0.1:8642`** (the default) so the port is only accessible locally
- **Store API keys in `.env`** — never bake them into the Docker image
- **Rotate API keys regularly** and update `.env` + restart the container
- **Review `security.md`** for EdgeCrab's built-in security layers

---

## Connecting from EdgeCrab CLI

Point EdgeCrab's Python or Node.js SDK at your self-hosted instance:

```python
from edgecrab import Agent

agent = Agent(
    model="openai/gpt-4o",
    base_url="https://agent.yourdomain.com/v1",
    api_key="your-edgecrab-gateway-token",  # if you add auth middleware
)
```

---

## Pro Tips

- **Use `EDGECRAB_MANAGED=1`** in production to prevent the agent from writing to its own config. This ensures your container config is the source of truth.
- **Bind-mount `~/.edgecrab/skills/`** from a shared volume to give all gateway instances the same skills without rebuilding the image.
- **Separate data and config volumes**: Use one volume for `~/.edgecrab/state.db` (session state) and another for `~/.edgecrab/skills/` (skills) so you can restore them independently.
- **Health-check before rolling**: Wait for `/health` to return `200` before marking new containers healthy in your orchestrator.
- **Log to stdout**: EdgeCrab writes structured JSON logs to stdout when `EDGECRAB_LOG_LEVEL=info`. Use `docker compose logs -f` or ship to your logging stack.

---

## FAQ

**What's the recommended minimum server spec?**
For a team of 5–10: 2 vCPU, 2 GB RAM is sufficient for most workloads. EdgeCrab itself uses ~14 MB RSS; the rest is tool subprocess overhead.

**Does the Docker image include all dependencies (Node.js for WhatsApp, etc.)?**
Yes. The official `ghcr.io/raphaelmansuy/edgecrab` image bundles all optional dependencies including Node.js (for the WhatsApp bridge) and Chromium (for browser tools).

**How do I rotate an API key without downtime?**
Update `.env`, then run `docker compose up -d --no-deps edgecrab`. Docker Compose will restart only the EdgeCrab container with the new env, with minimal downtime.

**Can I run multiple gateway instances behind a load balancer?**
Yes, but session state is stored in `~/.edgecrab/state.db` (SQLite). For multi-instance setups, either use a shared NFS/EFS mount for the data directory, or use sticky sessions in your load balancer.

**How do I enable HTTPS without a custom domain?**
Use a self-signed cert with Nginx, or use Tailscale HTTPS which gives a `*.ts.net` certificate automatically.

---

## See Also

- [Docker Compose file](/docker-compose.yml) in the repository root
- [Environment Variables](/reference/environment-variables/) — all `EDGECRAB_*` deployment variables
- [Security Model](/user-guide/security/) — what the agent can and can't do on your server
