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
       | request
[MirageIA — single process]
  |-- Embedded ONNX model (contextual PII detection)
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

- **Embedded LLM**: no Ollama server or external service — a single autonomous binary (like Murmure embeds Whisper)
- **Contextual detection**: the model understands context (won't mask "Thomas Edison" in a history lesson)
- **Reversible pseudonymization**: bidirectional mapping with IDs, not simple `[REDACTED]` masking
- **SSE streaming**: compatible with LLM response streaming
- **Zero config**: works out-of-the-box, the proxy sits between the app and the API

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
| Runtime | Rust + Tauri (single binary, cross-platform) |
| PII Model | ONNX Runtime (DistilBERT-PII or Qwen3 0.6B quantized) |
| HTTP Proxy | Man-in-the-middle interception (hyper / axum) |
| Mapping | In-memory, AES-256-GCM encrypted, not persisted |
| Interface | Tray icon + minimal local dashboard (Tauri webview) |
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
