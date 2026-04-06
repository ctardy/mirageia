# CLAUDE.md — Context for Claude Code

## Absolute rules (highest priority)

- **Language**: always write in English for code, commits, CLI messages, comments, and documentation. French translations are maintained in `docs/fr/` as secondary.
- **Shell paths**: always use absolute paths in proposed commands (e.g., `/c/dev/projects/mirageia`, never `./`)
- **Git pull with pending changes**: `git stash && git pull --rebase && git stash pop` — never run `git pull --rebase` directly
- **Git commit safety**: check `git status` and `git diff --cached` before committing. Never `git add .` or `git add -A`
- **No backward compatibility**: no `@Deprecated`, no useless fallback. The project is in construction phase
- **Never modify without request**: never edit files without explicit user request
- **Embedded LLM, no server**: the model runs via ONNX Runtime in-process — never depend on Ollama, LM Studio, or any external server
- **Zero cleartext sensitive data**: the pseudonymization mapping stays 100% local, never persisted in cleartext on disk, never logged

---

## Project description

**MirageIA** — Intelligent pseudonymization proxy for LLM APIs with embedded model.

Intercepts requests to LLM APIs (Anthropic, OpenAI), detects sensitive data (PII) via an embedded language model (ONNX Runtime), pseudonymizes them before sending, then re-injects original values in responses.

### How it works

```
Application (Claude Code, etc.)
  [configured to use MirageIA as HTTP proxy — explicit client-side setup required]
       | request
[MirageIA — single process]
  |-- PII detection (regex by default; optional ONNX model for contextual detection)
  |-- Pseudonymization (replacement with fake values + mapping ID)
  |-- In-memory mapping table (AES-256 encrypted)
       | cleaned request
LLM API (Anthropic / OpenAI)
       | response
[MirageIA]
  |-- De-pseudonymization (re-injection of original values via mapping ID)
       | restored response
Application
```

### Concrete example

| Original data | Sent to API | Mapping ID |
|---------------|-------------|------------|
| `192.168.1.22` | `192.168.1.223` | 458 |
| `Tardy` | `Gerard` | 253 |
| `chris@example.com` | `paul@example.com` | 254 |

The LLM API never sees the real data. The response contains `Gerard` -> MirageIA replaces it with `Tardy` before returning to the application.

### Differentiators

- **No external server**: no Ollama or external inference service — single autonomous binary; ONNX model is an optional feature requiring a separate ~337 MB download
- **Contextual detection** (optional): when the ONNX model is active, it understands context (won't mask "Thomas Edison" in a history lesson); default mode uses regex
- **Reversible pseudonymization**: bidirectional mapping with IDs, not simple `[REDACTED]` masking
- **SSE streaming**: compatible with LLM response streaming
- **Explicit proxy setup**: the client application must be configured to point to the proxy (e.g., `ANTHROPIC_BASE_URL=http://localhost:3100`); interception is not transparent

### Detected PII types

- Person names, first names, pseudonyms
- IP addresses (v4, v6)
- Email addresses
- Phone numbers
- Postal addresses
- Credit card numbers, IBAN
- Identifiers (social security, passport, etc.)
- API keys, tokens, secrets
- Internal URLs / private domain names
- Server names, sensitive file paths

---

## Technical stack (target)

| Component | Technology |
|-----------|-----------|
| Runtime | Rust (CLI binary, cross-platform) — Tauri removed |
| PII Model | ONNX Runtime (optional feature, ~337 MB model downloaded separately) — regex detection by default |
| HTTP Proxy | Man-in-the-middle interception (hyper / axum) |
| Mapping | In-memory, AES-256-GCM encrypted, not persisted |
| Interface | Integrated web dashboard (HTML/JS served by Axum at /dashboard) — no desktop interface |
| Tests | cargo test + PII fixtures |

---

## Build commands

```bash
# To be defined when the stack is finalized
cd /c/dev/projects/mirageia
```

---

## Documentation

| Topic | EN | FR |
|-------|----|----|
| Architecture overview | `docs/en/architecture/overview.md` | `docs/fr/architecture/vue-ensemble.md` |
| Pseudonymization flow | `docs/en/architecture/pseudonymization-flow.md` | `docs/fr/architecture/flux-pseudonymisation.md` |
| Embedded PII model | `docs/en/technical/pii-model.md` | `docs/fr/technique/modele-pii.md` |
| HTTP Proxy | `docs/en/technical/http-proxy.md` | `docs/fr/technique/proxy-http.md` |
| Deployment ops | `docs/en/deployment-ops.md` | `docs/fr/deploiement-ops.md` |
| Research & state of the art | `docs/recherche/etat-de-lart.md` | |
| Tickets | `docs/tickets/` | |
| Full index | `docs/README.md` |
