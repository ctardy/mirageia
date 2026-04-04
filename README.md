# MirageIA

> **[Lire en francais](docs/fr/README.md)**

**Intelligent pseudonymization proxy for LLM APIs.**

The API never sees your real data — it sees a mirage.

```
Your app  -->  MirageIA (local proxy :3100)  -->  LLM API (Anthropic / OpenAI)
                │                                    │
                ├─ Detects PII (regex + ONNX)        │
                ├─ Pseudonymizes before sending       │
                └─ Restores in the response  <────────┘
```

## The Problem

When you use Claude, ChatGPT, or any other LLM via API, your data travels in plain text to external servers: names, emails, IP addresses, API keys, phone numbers... This sensitive data is exposed without you knowing.

## The Solution

MirageIA sits between your application and the LLM API. It automatically detects sensitive data, replaces it with consistent fake values, and restores the originals in the response.

| Original data | What the API receives | What you get back |
|---|---|---|
| `jean.dupont@acme.fr` | `alice@example.com` | `jean.dupont@acme.fr` (restored) |
| `192.168.1.22` | `10.0.84.12` | `192.168.1.22` (restored) |
| `06 12 34 56 78` | `06 47 91 28 53` | `06 12 34 56 78` (restored) |
| `sk-abc123def456...` | `sk-xR9mK2pL7wQ4...` | `sk-abc123def456...` (restored) |

The LLM works with fake but consistent data — its response is just as relevant, and your data never left your machine.

---

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (1.75+)
- GCC (via MSYS2 on Windows) or MSVC toolchain

### Installation

```bash
git clone https://github.com/ctardy/mirageia.git
cd mirageia

# On Windows with MSYS2:
export PATH="/c/msys64/mingw64/bin:$HOME/.cargo/bin:$PATH"

cargo build --release
```

### Guided Setup

```bash
# The wizard guides you: port, LLM providers, whitelist, shell
mirageia setup
```

### Usage

```bash
# Start the proxy
mirageia

# Use Claude Code via the proxy (this session only)
mirageia wrap -- claude

# Monitor requests in real time (in another terminal)
mirageia console

# Web dashboard
# Open http://localhost:3100/dashboard in your browser
```

**Per-session activation** — `mirageia wrap` launches your command with the proxy enabled, without modifying your shell. When the command exits, the proxy is no longer used:

```bash
mirageia wrap -- claude          # Claude Code protected
mirageia wrap -- python app.py   # Python script protected
claude                           # Claude Code direct (no proxy)
```

### Temporarily Disable

```bash
# Option 1: Passthrough mode (proxy relays without pseudonymizing)
mirageia proxy --passthrough

# Option 2: Stop the proxy — does NOT affect apps launched normally
# Only those launched via `mirageia wrap` go through the proxy
```

### Verification

```bash
# Health check
curl http://localhost:3100/health
# -> {"status":"ok","passthrough":false,"pii_mappings":0}

# Test request (requires an Anthropic API key)
curl -X POST http://localhost:3100/v1/messages \
  -H "x-api-key: $ANTHROPIC_API_KEY" \
  -H "content-type: application/json" \
  -d '{
    "model": "claude-sonnet-4-20250514",
    "max_tokens": 100,
    "messages": [{"role": "user", "content": "My email is jean@acme.fr and my IP is 192.168.1.50"}]
  }'
```

In the MirageIA logs, you will see the detected and pseudonymized PII. The request sent to Anthropic will contain neither the original email nor the IP.

---

## Configuration

MirageIA works without configuration (zero config). To customize, create `~/.mirageia/config.toml`:

```toml
[proxy]
listen_addr = "127.0.0.1:3100"  # Listening address
log_level = "info"               # debug, info, warn, error
add_header = false               # Add X-MirageIA: active header to requests
fail_open = true                 # Forward request if pseudonymization fails
passthrough = false              # Passthrough mode: relay without pseudonymizing

[detection]
confidence_threshold = 0.75      # Confidence threshold (0.0-1.0)
whitelist = [                    # Terms to never pseudonymize
    "localhost",
    "127.0.0.1",
    "Thomas Edison",
]
```

Environment variables take precedence over the file:

| Variable | Description |
|---|---|
| `MIRAGEIA_LISTEN_ADDR` | Listening address (e.g., `0.0.0.0:3100`) |
| `MIRAGEIA_ANTHROPIC_URL` | Anthropic base URL |
| `MIRAGEIA_OPENAI_URL` | OpenAI base URL |
| `MIRAGEIA_LOG_LEVEL` | Log level |
| `MIRAGEIA_PASSTHROUGH` | Enable passthrough mode (any value = enabled) |

---

## Detected PII Types

The regex detector (v1) covers fixed-pattern PII:

| Type | Examples | Generated pseudonym |
|---|---|---|
| Email | `jean@acme.fr` | `alice@example.com` |
| IPv4 | `192.168.1.50` | `10.0.84.12` |
| IPv6 | `2001:db8::1` | `fd00::a1b2:c3d4` |
| Phone | `06 12 34 56 78` | `06 47 91 28 53` (format preserved) |
| Credit card | `4111 1111 1111 1111` | `4892 7631 0458 2173` (Luhn valid) |
| IBAN | `FR7612345678901234567890` | `FR8398765432109876543210` |
| API key / token | `sk-abc123def456...` | `sk-xR9mK2pL7wQ4...` (prefix preserved) |
| Social security # | `1 85 07 75 123 456 78` | `2 91 03 69 847 215 34` |

The contextual ONNX detector (v2, in progress) will add detection of person names, postal addresses, and will understand context ("Thomas Edison" in a history lesson = not masked).

---

## Architecture

```
src/
├── main.rs                  CLI (proxy / setup / detect / wrap / console)
├── lib.rs                   Public modules
├── config/
│   └── settings.rs          AppConfig, TOML + env loading
├── proxy/
│   ├── server.rs            axum handler, full pipeline, dashboard
│   ├── router.rs            Anthropic / OpenAI routing by path
│   ├── client.rs            Upstream HTTP client (reqwest)
│   ├── extractor.rs         JSON extraction/rebuild per provider
│   └── error.rs             Proxy error types
├── detection/
│   ├── regex_detector.rs    PII detector via regex (v1)
│   ├── types.rs             PiiType, PiiEntity, label_to_pii_type
│   ├── model.rs             ONNX model (feature-gated, v2)
│   ├── tokenizer.rs         HuggingFace tokenizer, segmentation
│   ├── postprocess.rs       Softmax, BIO decode, entity merging
│   └── error.rs             Detection errors
├── pseudonymization/
│   ├── generator.rs         Pseudonym generator by type
│   ├── replacer.rs          Text replacement (offsets)
│   ├── depseudonymizer.rs   De-pseudonymization (AhoCorasick)
│   └── dictionaries.rs      Embedded first/last names
├── mapping/
│   ├── table.rs             Bidirectional table (SHA-256 + AES-256-GCM)
│   ├── crypto.rs            Encryption/decryption, zeroization
│   └── error.rs             Mapping errors
└── streaming/
    ├── sse_parser.rs        Parse/rebuild SSE Anthropic/OpenAI
    └── buffer.rs            Buffer for pseudonyms split between tokens
```

### Processing Pipeline

```
INCOMING REQUEST
    │
    ▼
[JSON Extraction]    <- extractor.rs (Anthropic/OpenAI text fields)
    │
    ▼
[PII Detection]      <- regex_detector.rs (emails, IPs, phones, CC, IBAN, keys)
    │                   + whitelist filtering
    ▼
[Pseudonymization]   <- replacer.rs (descending positions)
    │                   + generator.rs (consistent pseudonyms by type)
    │                   + mapping/table.rs (AES-256-GCM in memory)
    ▼
[Reconstruction]     <- extractor.rs (rebuild JSON)
    │
    ▼
[Forward]            <- client.rs -> upstream API
    │
    ▼
UPSTREAM RESPONSE
    │
    ▼
[De-pseudonymize]    <- depseudonymizer.rs (AhoCorasick, longest-first)
    │                   or buffer.rs (streaming SSE, split between tokens)
    ▼
CLIENT RESPONSE (original data restored)
```

---

## Tests

```bash
# All tests (145)
cargo test

# Unit tests only
cargo test --lib

# E2E tests (proxy + mock upstream)
cargo test --test e2e_proxy

# Specific module tests
cargo test -- detection::regex_detector
cargo test -- mapping::crypto
cargo test -- pseudonymization
```

### Test Coverage

| Module | Tests | Coverage |
|---|---:|---|
| config | 6 | Default config, TOML parsing, partial, empty, passthrough |
| proxy/router | 7 | Anthropic/OpenAI routing, URLs |
| proxy/extractor | 9 | JSON extraction/rebuild, content string/array, system |
| detection/types | 7 | Labels, thresholds, aliases, display |
| detection/postprocess | 11 | Softmax, extraction, merging, multi-token, thresholds |
| detection/tokenizer | 5 | Segmentation, overlap, progression |
| detection/regex_detector | 16 | Email, IP, phone, CC, IBAN, API key, whitelist |
| detection/model | 2 | Models directory, missing files |
| detection/mod | 4 | Label map loading |
| mapping/crypto | 6 | AES-256-GCM roundtrip, nonces, unicode |
| mapping/table | 8 | Bidirectional, concurrent, unique IDs |
| pseudonymization/generator | 13 | All PII types, Luhn, format |
| pseudonymization/replacer | 5 | Positions, session coherence |
| pseudonymization/depseudonymizer | 6 | Roundtrip, longest-first |
| streaming/sse_parser | 7 | Anthropic, OpenAI, DONE, rebuild |
| streaming/buffer | 7 | Split pseudonym, flush |
| **e2e** | **12** | **Full pipeline, passthrough, SSE events, dashboard** |
| **Total** | **145** | |

---

## Project Status

| Component | Status | Notes |
|---|---|---|
| Transparent HTTP proxy | Done | axum, Anthropic/OpenAI routing |
| Regex PII detection | Done | 8 PII types |
| Reversible pseudonymization | Done | AES-256-GCM mapping |
| Response de-pseudonymization | Done | Non-streaming + SSE buffer |
| TOML config + whitelist | Done | ~/.mirageia/config.toml |
| Fail-open | Done | Passthrough on error |
| Passthrough mode | Done | `--passthrough` / config / env var |
| Per-session activation | Done | `mirageia wrap -- claude` |
| Monitoring console | Done | `mirageia console` (real-time SSE) |
| Web dashboard | Done | `/dashboard` embedded in binary |
| Docker + deployment | Done | Dockerfile, ops guide, Apache reverse proxy |
| E2E tests | Done | 145 tests |
| Contextual ONNX detection | Structured | Code ready, ONNX Runtime blocked by MSVC toolchain |
| Tauri dashboard | Planned | Phase 4 |

---

## Documentation

| | FR | EN |
|---|---|---|
| **Installation** | [`docs/fr/installation.md`](docs/fr/installation.md) | [`docs/en/installation.md`](docs/en/installation.md) |
| **Ops Deployment** | [`docs/fr/deploiement-ops.md`](docs/fr/deploiement-ops.md) | [`docs/en/deployment-ops.md`](docs/en/deployment-ops.md) |
| **Distribution** | [`docs/fr/distribution.md`](docs/fr/distribution.md) | [`docs/en/distribution.md`](docs/en/distribution.md) |
| **Contributing** | [`docs/fr/contribution.md`](docs/fr/contribution.md) | [`docs/en/contributing.md`](docs/en/contributing.md) |
| Architecture | [`docs/fr/architecture/`](docs/fr/architecture/) | [`docs/en/architecture/`](docs/en/architecture/) |
| Security | [`docs/fr/securite/`](docs/fr/securite/) | [`docs/en/security/`](docs/en/security/) |
| Technical | [`docs/fr/technique/`](docs/fr/technique/) | [`docs/en/technical/`](docs/en/technical/) |
| Research | [`docs/recherche/`](docs/recherche/) | |
| Tickets | [`docs/tickets/`](docs/tickets/) | |

## License

MIT
