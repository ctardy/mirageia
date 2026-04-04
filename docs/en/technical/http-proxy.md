# HTTP Proxy

## Role

The HTTP proxy is MirageIA's entry point. It intercepts all requests destined for LLM APIs, passes them through the pseudonymization pipeline, then forwards the cleaned request to the actual API.

## Client-side configuration

The application (Claude Code, etc.) must be configured to use the proxy:

```bash
# Claude Code — environment variable
export ANTHROPIC_BASE_URL=http://localhost:3100

# OpenAI SDK
export OPENAI_BASE_URL=http://localhost:3100
```

The proxy determines the target provider from the path:
- `/v1/messages` → Anthropic (`api.anthropic.com`)
- `/v1/chat/completions` → OpenAI (`api.openai.com`)

## Intercepted endpoints

### Anthropic
| Endpoint | Method | Streaming |
|----------|--------|-----------|
| `/v1/messages` | POST | Yes (SSE) |

### OpenAI
| Endpoint | Method | Streaming |
|----------|--------|-----------|
| `/v1/chat/completions` | POST | Yes (SSE) |

### MirageIA internal endpoints
| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Proxy status: `{"status":"ok","passthrough":false,"pii_mappings":0}` |
| `/events` | GET | Real-time SSE stream of requests (for `mirageia console`) |

## Passthrough mode

The proxy can relay requests **without pseudonymizing**, useful for debugging or temporary deactivation:

```bash
mirageia proxy --passthrough        # CLI flag
MIRAGEIA_PASSTHROUGH=1 mirageia     # Environment variable
```

Or in `config.toml`:
```toml
[proxy]
passthrough = true
```

In passthrough mode, requests are forwarded as-is to the API. Events are still emitted on `/events` (marked `passthrough: true`).

## SSE streaming handling

LLM APIs use Server-Sent Events to stream responses token by token. The proxy must:

1. **Request**: pseudonymize the entire body before sending (no streaming on the request)
2. **Response**: 
   - Buffer incoming tokens
   - Detect when a complete pseudonym has been received
   - Replace and forward to the client
   - Flush the buffer regularly to avoid introducing too much latency

### Buffer strategy (streaming response)

```
Tokens received:  "The" " user" "'s" " name" " is" " Ger" "ard"
                                                       ^^^^^^^^^^^
Buffer:           accumulates "Ger" → "Gerard" recognized → replaced by "Tardy" → flush
```

- The buffer retains the last N tokens (N = max length of a pseudonym)
- When a pseudonym is recognized, it is replaced and flushed
- Non-ambiguous tokens are flushed immediately

## Headers

- Authentication headers (`x-api-key`, `Authorization: Bearer`) are forwarded as-is to the provider
- MirageIA adds an `X-MirageIA: active` header for traceability (optional, can be disabled)
- `Content-Length` is recalculated after pseudonymization

## CLI commands

| Command | Description |
|---------|-------------|
| `mirageia` | Start the proxy (default behavior) |
| `mirageia proxy --passthrough` | Start in passthrough mode |
| `mirageia setup` | Interactive configuration wizard |
| `mirageia wrap -- <cmd>` | Run a command with the proxy enabled (per-session activation) |
| `mirageia console` | Display requests in real time (connects to the `/events` stream) |
| `mirageia detect <text>` | Detect PII in a text (requires `--features onnx`) |

### `mirageia wrap`

Launches a child process with `ANTHROPIC_BASE_URL` and `OPENAI_BASE_URL` pointing to the proxy. First checks that the proxy is active via `/health`.

```bash
# Run Claude Code protected by MirageIA
mirageia wrap -- claude

# Run a Python script with protection
mirageia wrap -- python app.py

# Specify a different port
mirageia wrap --port 4200 -- claude
```

### `mirageia console`

Connects to the proxy's `/events` SSE endpoint and displays formatted events:

```
  [14:32:01] → PII  Anthropic  /v1/messages (3 PII detected)
  [14:32:02] ← PII  Anthropic  /v1/messages
  [14:35:10] → PASS OpenAI     /v1/chat/completions
  [14:35:11] ← PASS OpenAI     /v1/chat/completions
```

## Technical stack

- **Rust**: `axum` for the HTTP server
- **reqwest**: HTTP client for calling upstream APIs
- **tokio**: async runtime + broadcast channel for events
- **async-stream**: SSE stream generation for `/events`
- **chrono**: event timestamping
