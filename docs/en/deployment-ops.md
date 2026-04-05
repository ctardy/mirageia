# Deployment Guide -- MirageIA

Operational guide to deploy MirageIA on a Linux server with Docker, behind an Apache reverse proxy.

---

## What is MirageIA?

MirageIA is a **local proxy** that sits between developers and LLM APIs (Claude, ChatGPT). It automatically detects sensitive data (emails, IPs, phone numbers, API keys...) in requests, replaces them with fictitious values, and restores the originals in responses.

**The LLM API never sees the real data.**

```
Developer -> MirageIA (proxy :3100) -> LLM API (Anthropic/OpenAI)
                |                         |
                +-- Detects PII           |
                +-- Pseudonymizes         |
                +-- Restores  <-----------+
```

The proxy exposes a **web dashboard** to monitor requests in real time.

---

## Server prerequisites

| Item | Version | Notes |
|------|---------|-------|
| Linux | Ubuntu 22.04+ / Debian 12+ | x86_64 |
| Docker | 24+ | `docker --version` |
| Apache | 2.4+ | With `mod_proxy`, `mod_proxy_http`, `mod_ssl` |
| Port | 3100 | Internal proxy port (not directly exposed) |
| Network | Outbound HTTPS | To `api.anthropic.com`, `api.openai.com` |

---

## 1. Docker deployment

### Fetch the project

```bash
git clone https://github.com/ctardy/mirageia.git /opt/mirageia
cd /opt/mirageia/docker
```

### Build the image

```bash
docker build -t mirageia .
```

The image contains:
- Ubuntu 24.04
- MirageIA (binary downloaded from GitHub Releases **at each startup**)
- Node.js 22 + Claude Code
- ttyd (WebSocket web terminal)
- docker CLI (for Docker-in-Docker testing via socket)

Image size: ~500 MB.

### Updating the MirageIA binary (without rebuild)

The entrypoint automatically downloads the latest release on each `docker restart`:

```bash
# After a git tag + push, wait for CI to publish the release (~3 min), then:
docker restart mirageia
# The new binary is automatically fetched on restart
```

To check the active version:
```bash
docker exec mirageia mirageia --version
```

### Deployment via Docker Compose (recommended)

Reference `docker-compose.yml`:

```yaml
services:
  mirageia:
    build:
      context: /opt/projet/mirageia
      dockerfile: docker/Dockerfile
    image: mirageia:latest
    container_name: mirageia
    restart: unless-stopped
    env_file:
      - /opt/docker/conf/secrets.env   # ANTHROPIC_API_KEY
    environment:
      - TZ=Europe/Paris
    volumes:
      - ./home/.claude:/root/.claude        # persisted Claude Code OAuth tokens
      - ./home/.claude.json:/root/.claude.json
      - ./home/.local:/root/.local
      - /opt/projet/mirageia:/workspace     # Rust source for cargo test
      - /var/run/docker.sock:/var/run/docker.sock  # Docker-in-Docker
    healthcheck:
      test: ["CMD", "curl", "-sf", "http://localhost:3100/health"]
      interval: 30s
      timeout: 10s
      retries: 3
    security_opt:
      - no-new-privileges:true
    cap_drop:
      - ALL
    cap_add:
      - NET_BIND_SERVICE
      - DAC_OVERRIDE   # required for /var/run/docker.sock
    deploy:
      resources:
        limits:
          cpus: '2.0'
          memory: 1G        # minimum 1G — Claude Code with parallel agents ~500MB
        reservations:
          memory: 256M
```

> **Memory warning**: the memory limit must be at least **1 GB**. Claude Code with parallel agents uses ~500 MB. A 256 MB limit causes silent OOM kills (`Killed`).

### Start the container

```bash
# Production mode (daemon)
docker run -d \
  --name mirageia \
  --restart unless-stopped \
  -p 127.0.0.1:3100:3100 \
  -p 127.0.0.1:7681:7681 \
  -e ANTHROPIC_API_KEY=sk-ant-XXXXXXXXX \
  --memory=1g \
  mirageia

# Verify it is running
docker logs mirageia
curl http://127.0.0.1:3100/health
```

Expected health check response:
```json
{"status": "ok", "passthrough": false, "pii_mappings": 0, "version": "0.4.3"}
```

### Web terminal (ttyd)

The container exposes a web terminal on port **7681** (path `/mirageia/`).
ttyd is launched with `--ping-interval 30` to maintain the WebSocket connection.

If deployed behind Apache, configure a long timeout on the ProxyPass:

```apache
# IMPORTANT: timeout=3600 to prevent WebSocket disconnection after 2 min
ProxyPass /mirageia/ http://mirageia:7681/mirageia/ timeout=3600
ProxyPassReverse /mirageia/ http://mirageia:7681/mirageia/
```

Available commands from the web terminal:
```
mirageia wrap -- claude    Launch Claude Code through the proxy
claude                     Launch Claude Code directly
mirageia console           Real-time monitoring
curl localhost:3100/health Health check
```

### Start in interactive mode (testing)

```bash
docker run -it --rm \
  -p 127.0.0.1:3100:3100 \
  -e ANTHROPIC_API_KEY=sk-ant-XXXXXXXXX \
  mirageia
```

The container displays a menu with available commands:
```
  mirageia wrap -- claude    Launch Claude Code through the proxy
  claude                     Launch Claude Code directly (without proxy)
  mirageia console           Real-time monitoring
  curl localhost:3100/health Health check
```

---

## 2. Apache configuration (reverse proxy)

### Enable modules

```bash
a2enmod proxy proxy_http ssl headers
systemctl restart apache2
```

### VirtualHost

Create `/etc/apache2/sites-available/mirageia.conf`:

```apache
<VirtualHost *:443>
    ServerName mirageia.example.com

    # --- Reverse proxy to MirageIA ---

    ProxyPreserveHost On
    ProxyRequests Off

    # Web dashboard
    ProxyPass /dashboard http://127.0.0.1:3100/dashboard
    ProxyPassReverse /dashboard http://127.0.0.1:3100/dashboard

    # Health check
    ProxyPass /health http://127.0.0.1:3100/health
    ProxyPassReverse /health http://127.0.0.1:3100/health

    # Real-time SSE stream (for the dashboard)
    # IMPORTANT: disable buffering otherwise SSE will not work
    <Location /events>
        ProxyPass http://127.0.0.1:3100/events
        ProxyPassReverse http://127.0.0.1:3100/events
        SetEnv proxy-sendchunked 1
        SetEnv proxy-initial-not-pooled 1
        SetEnv proxy-nokeepalive 1
    </Location>

    # API routes (for developers to point to)
    ProxyPass /v1 http://127.0.0.1:3100/v1
    ProxyPassReverse /v1 http://127.0.0.1:3100/v1

    # --- SSL (Let's Encrypt) ---

    SSLEngine on
    SSLCertificateFile /etc/letsencrypt/live/mirageia.example.com/fullchain.pem
    SSLCertificateKeyFile /etc/letsencrypt/live/mirageia.example.com/privkey.pem

    # --- Security ---

    # Restrict dashboard access (optional)
    <Location /dashboard>
        Require ip 10.0.0.0/8 172.16.0.0/12 192.168.0.0/16
        # Or via authentication:
        # AuthType Basic
        # AuthName "MirageIA Dashboard"
        # AuthUserFile /etc/apache2/.htpasswd-mirageia
        # Require valid-user
    </Location>

    # Security headers
    Header always set X-Content-Type-Options "nosniff"
    Header always set X-Frame-Options "DENY"
</VirtualHost>

# HTTP -> HTTPS redirect
<VirtualHost *:80>
    ServerName mirageia.example.com
    Redirect permanent / https://mirageia.example.com/
</VirtualHost>
```

### Enable and test

```bash
a2ensite mirageia
apachectl configtest     # Verify syntax
systemctl reload apache2

# Test
curl https://mirageia.example.com/health
```

---

## 3. Usage by developers

Once deployed, developers have two options:

### Option A -- Via the server (centralized)

```bash
export ANTHROPIC_BASE_URL=https://mirageia.example.com
claude
```

All Claude requests go through MirageIA on the server.

### Option B -- Locally (decentralized)

Each developer installs MirageIA on their workstation:

```bash
# Download the binary
curl -sSfL https://github.com/ctardy/mirageia/releases/latest/download/mirageia-linux-x86_64.tar.gz | tar xz -C ~/.local/bin/

# Start the proxy locally
mirageia &

# Use Claude through the proxy
mirageia wrap -- claude
```

---

## 4. Monitoring dashboard

Accessible at: `https://mirageia.example.com/dashboard`

The dashboard displays in real time:

| Information | Description |
|-------------|-------------|
| **Requests** | Total count of processed requests |
| **PII detected** | Total number of intercepted sensitive data |
| **Active mappings** | Number of pseudonyms in memory |
| **Mode** | PII (pseudonymization active) or PASS (passthrough) |
| **Live feed** | Each request with timestamp, provider, path, PII count |

The feed updates in real time via Server-Sent Events (SSE).

---

## 5. Available endpoints

| Endpoint | Method | Description | Access |
|----------|--------|-------------|--------|
| `/health` | GET | Proxy status (JSON) | Monitoring / load balancer |
| `/dashboard` | GET | Real-time web dashboard | Browser (protect via Apache) |
| `/events` | GET | SSE event stream | Dashboard / monitoring tools |
| `/v1/messages` | POST | Anthropic proxy (Claude) | Applications |
| `/v1/chat/completions` | POST | OpenAI proxy (GPT) | Applications |

---

## 6. Passthrough mode (temporary deactivation)

To disable pseudonymization without stopping the proxy:

```bash
# Restart the container with the flag
docker stop mirageia
docker run -d \
  --name mirageia \
  --restart unless-stopped \
  -p 127.0.0.1:3100:3100 \
  -e ANTHROPIC_API_KEY=sk-ant-XXXXXXXXX \
  -e MIRAGEIA_PASSTHROUGH=1 \
  mirageia
```

The health check will indicate `"passthrough": true`. The dashboard will show "PASS" instead of "PII".

To re-enable: restart without `MIRAGEIA_PASSTHROUGH`.

---

## 7. Monitoring and alerts

### Health check (Nagios, Zabbix, etc.)

```bash
# Returns HTTP 200 + JSON if OK
curl -sf http://127.0.0.1:3100/health || echo "CRITICAL: MirageIA down"
```

### Docker logs

```bash
# Real-time logs
docker logs -f mirageia

# Logs with timestamps
docker logs --timestamps mirageia

# Last 50 lines
docker logs --tail 50 mirageia
```

The logs show for each request:
```
INFO  PII detected in request pii_count=3
INFO  Request pseudonymized provider=Anthropic mappings=3
```

### Automatic restart

The `--restart unless-stopped` flag ensures restart after a crash or server reboot.

---

## 8. Security

### What transits where

| Data | Where | In clear text? |
|------|-------|----------------|
| API keys (ANTHROPIC_API_KEY) | Container env variable | Yes (env variable) |
| Original requests (with PII) | Between the dev and MirageIA | Yes (HTTPS via Apache) |
| Pseudonymized requests | Between MirageIA and the LLM API | Yes (native HTTPS) |
| Mapping table (PII <-> pseudonyms) | In container memory | Encrypted AES-256-GCM |

### Important points

- The mapping table **is never persisted to disk** -- a container restart clears it
- API keys are **forwarded as-is** to the API (MirageIA does not store them)
- The MirageIA binary has **no network dependency** other than the target LLM APIs
- No telemetry, no calls to third-party servers

### Recommendations

- Protect `/dashboard` by IP or authentication (see Apache config above)
- Do not expose port 3100 directly -- always go through Apache/HTTPS
- Use a dedicated Docker network if other containers are running on the server
- Store the API key in a secret manager (Docker secrets, Vault) rather than as an env variable

---

## 9. Updating

```bash
cd /opt/mirageia

# Fetch the latest version
git pull

# Rebuild the image
docker build -t mirageia docker/

# Restart the container
docker stop mirageia && docker rm mirageia
docker run -d \
  --name mirageia \
  --restart unless-stopped \
  -p 127.0.0.1:3100:3100 \
  -e ANTHROPIC_API_KEY=sk-ant-XXXXXXXXX \
  mirageia
```

For a specific version:

```bash
docker build --build-arg MIRAGEIA_VERSION=v0.2.0 -t mirageia docker/
```

---

## 10. Troubleshooting

### The container does not start

```bash
docker logs mirageia
# Verify that ANTHROPIC_API_KEY is set
```

### The dashboard is empty (no events)

- Verify that SSE passes through Apache (no buffering)
- Test directly: `curl http://127.0.0.1:3100/events` (should stay open)
- Verify Apache modules: `apachectl -M | grep proxy`

### Claude requests fail

```bash
# Test the proxy directly
curl -X POST http://127.0.0.1:3100/v1/messages \
  -H "x-api-key: $ANTHROPIC_API_KEY" \
  -H "content-type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  -d '{"model":"claude-sonnet-4-20250514","max_tokens":10,"messages":[{"role":"user","content":"test"}]}'
```

### SSE does not work behind Apache

Verify that these directives are present in the `<Location /events>` block:
```apache
SetEnv proxy-sendchunked 1
SetEnv proxy-initial-not-pooled 1
SetEnv proxy-nokeepalive 1
```

### Performance

- MirageIA uses ~10 MB of RAM at rest
- Pseudonymization adds ~1-5 ms of latency per request
- The container does not require a GPU

---

## Command summary

```bash
# Build
docker build -t mirageia /opt/mirageia/docker/

# Start (production)
docker run -d --name mirageia --restart unless-stopped \
  -p 127.0.0.1:3100:3100 \
  -e ANTHROPIC_API_KEY=sk-ant-XXX \
  mirageia

# Start (interactive testing)
docker run -it --rm -p 127.0.0.1:3100:3100 \
  -e ANTHROPIC_API_KEY=sk-ant-XXX mirageia

# Status
curl http://127.0.0.1:3100/health

# Logs
docker logs -f mirageia

# Restart
docker restart mirageia

# Stop
docker stop mirageia && docker rm mirageia
```

---

## Version history

| Version | Date | Changes |
|---------|------|---------|
| v0.4.3 | 2026-04-04 | Fix PHONE_NUMBER false positive on API key digits (pattern reordering: API keys before phone) |
| v0.4.2 | 2026-04-04 | Added IBAN (MOD-97) and credit card (Luhn) validators, Shannon entropy, secret patterns (GitHub, AWS, Stripe, Anthropic, OpenAI, JWT, Slack) |
| v0.4.1 | 2026-04-04 | Fix UTF-8 panic in StreamBuffer (`rfind` → `char_indices().rev()`); fix missing enriched SSE fields in v0.4.0 release binary |
| v0.4.0 | 2026 | Initial deployed version |
| v0.5.0 | 2026-04-04 | Text extraction from PDF (lopdf) and DOCX (zip+XML) before pseudonymization; model manager CLI (`mirageia model list/download/use/delete/verify`) |
