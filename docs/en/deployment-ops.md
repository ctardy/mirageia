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
          memory: 3G        # 3G required when ONNX model is active (VmPeak ~2.1 GB during loading)
        reservations:
          memory: 256M
```

> **Memory warning**: the memory limit must be at least **3 GB** when the ONNX model is active. The model peaks at ~2.1 GB during loading and stabilizes at ~946 MB RSS. Without ONNX (regex-only mode), 1 GB is sufficient.

### ONNX model activation

The ONNX model is downloaded once and persisted in the `./home/.mirageia` volume. It survives container restarts.

```bash
# 1. Download the model (run once, inside the running container)
docker exec mirageia mirageia model download iiiorg/piiranha-v1-detect-personal-information

# 2. Set it as active
docker exec mirageia mirageia model use iiiorg/piiranha-v1-detect-personal-information

# 3. Rebuild the image (the entrypoint is COPY'd at build time — rebuild required)
cd /opt/docker/mirageia
docker compose build
docker compose up -d
```

> **Important**: `docker/entrypoint.sh` is baked into the image via `COPY docker/entrypoint.sh /entrypoint.sh`. Modifying the file on disk has **no effect** until `docker compose build` is run.

To verify ONNX is active:
```bash
curl -s http://localhost:3100/health | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('onnx_model','not active'))"
# → iiiorg/piiranha-v1-detect-personal-information
```

The startup log will show:
```
✓ Modèle ONNX actif — détection contextuelle activée
```

### Start the container

```bash
# Production mode (daemon)
docker run -d \
  --name mirageia \
  --restart unless-stopped \
  -p 127.0.0.1:3100:3100 \
  -p 127.0.0.1:7681:7681 \
  -e ANTHROPIC_API_KEY=sk-ant-XXXXXXXXX \
  --memory=3g \
  mirageia

# Verify it is running
docker logs mirageia
curl http://127.0.0.1:3100/health
```

Expected health check response:
```json
{"status": "ok", "passthrough": false, "pii_mappings": 0, "version": "0.5.9", "onnx_model": "iiiorg/piiranha-v1-detect-personal-information"}
```
(`"onnx_model"` is `null` when running in regex-only mode.)

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

### Corporate proxy support (v0.5.25+)

If MirageIA runs inside a corporate network that routes outbound traffic through a proxy:

```toml
# config.toml
[proxy]
upstream_proxy = "http://proxy.corp:8080"
```

Or via environment variable:
```bash
MIRAGEIA_UPSTREAM_PROXY=http://proxy.corp:8080
```

MirageIA passes all outbound LLM API calls through this proxy.

#### SSL inspection / MITM proxies

Some corporate proxies perform TLS inspection and present their own certificate, which MirageIA rejects by default. If you get `502 Bad Gateway` errors with a corporate proxy, enable certificate acceptance:

```toml
# config.toml
[proxy]
upstream_proxy = "http://proxy.corp:8080"
danger_accept_invalid_certs = true
```

Or:
```bash
MIRAGEIA_DANGER_ACCEPT_INVALID_CERTS=1
```

> **Warning**: `danger_accept_invalid_certs = true` disables TLS validation for upstream calls. Only use it when your corporate proxy is the cause. The `mirageia setup` wizard will ask about this automatically when you configure a proxy.

---

### Bearer token authentication (v0.5.15+)

Protect the proxy against unauthorized use with an optional bearer token:

```bash
# Via environment variable
MIRAGEIA_PROXY_TOKEN=your-secret-token

# Via config.toml
[proxy]
proxy_token = "your-secret-token"
```

When set, all LLM requests (`/v1/messages`, `/v1/chat/completions`, etc.) must include:
```
Authorization: Bearer your-secret-token
```

Requests without a valid token receive `HTTP 401 {"error": "unauthorized"}`.

`/health`, `/dashboard`, `/events`, and `/shutdown` are exempt from authentication.

### Fail-safe mode (v0.5.15+)

By default, if pseudonymization fails (e.g. internal error), the proxy forwards the request unmasked (`fail_open = true`) with a `[SECURITY WARNING]` log entry.

To block instead of forward on error:

```toml
# config.toml
[proxy]
fail_open = false
```

With `fail_open = false`, a pseudonymization error returns `HTTP 502 {"error": "pseudonymization_failed"}` — the request is never sent to the LLM API.

### SSRF protection (v0.5.15+)

The proxy validates `anthropic_base_url` and `openai_base_url` at startup and rejects:
- Loopback addresses: `localhost`, `127.0.0.1`, `::1`, `0.0.0.0`
- Private IPv4 ranges: `10.x`, `192.168.x`, `172.16-31.x`
- Cloud metadata: `169.254.x`
- Non-HTTP/HTTPS schemes

Invalid configuration causes an immediate startup error.

### Recommendations

- Protect `/dashboard` by IP or authentication (see Apache config above)
- Do not expose port 3100 directly -- always go through Apache/HTTPS
- Use a dedicated Docker network if other containers are running on the server
- Store the API key in a secret manager (Docker secrets, Vault) rather than as an env variable
- Set `MIRAGEIA_PROXY_TOKEN` in shared or multi-user environments
- Set `fail_open = false` if unmasked forwarding is unacceptable for your use case

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

| Mode | RAM at rest | Startup peak |
|------|-------------|--------------|
| Regex only | ~10 MB | ~10 MB |
| ONNX active | ~946 MB | ~2.1 GB |

- Pseudonymization adds ~1–5 ms of latency per request (regex layer)
- ONNX adds ~15–30 ms per request
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
| v0.5.27 | 2026-04-07 | Fix clippy: remove duplicate `use std::env` in setup wizard. |
| v0.5.26 | 2026-04-07 | Fix UTF-8 panic in streaming buffer (multi-byte chars like é/à/ç in LLM responses). Add `danger_accept_invalid_certs` to setup wizard (Step 5b, shown only when a corporate proxy is configured). |
| v0.5.25 | 2026-04-07 | Add `upstream_proxy` and `danger_accept_invalid_certs` to setup wizard (Steps 5 and 5b). Corporate proxy + SSL inspection fully configurable via `mirageia setup`. |
| v0.5.24 | 2026-04-07 | Log proxy errors via `tracing::error!` so 502/connection errors are always visible in terminal even when using `wrap`. |
| v0.5.23 | 2026-04-07 | Fix clippy: `.map().unwrap_or(false)` → `.map_or(false, ...)` in `ensure_proxy_running`. |
| v0.5.22 | 2026-04-07 | Prevent auto-update infinite loop under Scoop/Homebrew (`is_managed_install` check). Add version verification step in release workflow. |
| v0.5.21 | 2026-04-07 | Auto-start proxy in `wrap` and `console` commands instead of exiting when not running. Display API errors (4xx/5xx/429) in console output. |
| v0.5.20 | 2026-04-07 | Add `upstream_proxy` and `danger_accept_invalid_certs` config options (corporate proxy + SSL inspection support). |
| v0.5.15 | 2026-04-06 | Bearer token auth, fail-open mode, SSRF protection. |
| v0.5.9 | 2026-04-05 | Clean up debug tokenizer logs. Docker image updated, ONNX active in production. |
| v0.5.8 | 2026-04-05 | Fix entrypoint timeout 30s→120s. Memory limit 1G→3G (ONNX: 946 MB RSS + 2.1 GB VmPeak during loading). Docker image rebuild required. |
| v0.5.7 | 2026-04-05 | Fix ONNX model path (`/` → `__` in check_model_files). Fix entrypoint health check timeout (1s→30s retry loop). |
| v0.5.6 | 2026-04-05 | Auto-download ONNX model from GitHub Releases (tar.gz) with HuggingFace fallback. ONNX model displayed in /health + console. |
| v0.5.5 | 2026-04-05 | ONNX integration (PiiDetector) in proxy pipeline — names, dates, addresses via contextual NER. Compiled with `--features onnx` in release.yml. Fail-open if model absent. |
| v0.5.0 | 2026-04-04 | Text extraction from PDF (lopdf) and DOCX (zip+XML) before pseudonymization; model manager CLI (`mirageia model list/download/use/delete/verify`) |
| v0.4.3 | 2026-04-04 | Fix PHONE_NUMBER false positive on API key digits (pattern reordering: API keys before phone) |
| v0.4.2 | 2026-04-04 | Added IBAN (MOD-97) and credit card (Luhn) validators, Shannon entropy, secret patterns (GitHub, AWS, Stripe, Anthropic, OpenAI, JWT, Slack) |
| v0.4.1 | 2026-04-04 | Fix UTF-8 panic in StreamBuffer (`rfind` → `char_indices().rev()`); fix missing enriched SSE fields in v0.4.0 release binary |
| v0.4.0 | 2026 | Initial deployed version |
