# Architecture -- Overview

## Global Diagram

```
┌─────────────────────────────────────────────────────────┐
│                    MirageIA (single process)             │
│                                                         │
│  ┌──────────┐    ┌──────────────┐    ┌───────────────┐  │
│  │  HTTP    │───▶│  PII         │───▶│ Pseudonymizer │  │
│  │  Proxy   │    │  Detector    │    │               │  │
│  │          │◀───│  (ONNX)      │◀───│ Mapping table │  │
│  └──────────┘    └──────────────┘    └───────────────┘  │
│       ▲                                     │           │
│       │              ┌──────────┐           │           │
│       └──────────────│ Dashboard│───────────┘           │
│                      │ (Tauri)  │                       │
│                      └──────────┘                       │
└─────────────────────────────────────────────────────────┘
        ▲                                     │
        │ original request          cleaned request
        │                                     ▼
  ┌───────────┐                      ┌──────────────┐
  │ Claude    │                      │ Anthropic API│
  │ Code, etc.│                      │ / OpenAI     │
  └───────────┘                      └──────────────┘
```

## Main Components

### 1. HTTP Proxy
- Listens on a local port (e.g., `localhost:3100`)
- Intercepts requests to `api.anthropic.com` and `api.openai.com`
- Supports SSE (Server-Sent Events) streaming for streamed responses
- The client application is configured to point to the proxy instead of the API directly
- Transparent handling of authentication headers (API keys passed as-is)

### 2. PII Detector (embedded ONNX model)
- Language model embedded directly in the binary via ONNX Runtime
- No external server (Ollama, etc.) -- everything runs in-process
- Contextual detection: understands semantics, not just pattern matching
- Target model: DistilBERT-PII (~260 MB) or Qwen3 0.6B quantized (~400 MB)
- Target latency: < 50ms per request

### 3. Pseudonymizer + Mapping Table
- Replaces each detected PII with a consistent fictitious value (same data type)
- Assigns a unique ID to each replacement
- In-memory mapping table, encrypted with AES-256-GCM
- Deterministic mapping per session: same input = same pseudonym throughout the conversation
- De-pseudonymization in responses: searches for pseudonyms and re-injects the originals

### 4. Dashboard (Tauri webview)
- Discreet tray icon (taskbar)
- Real-time view of detected and pseudonymized PII
- Session statistics (number of replacements, PII types)
- Configuration (supported providers, PII types to detect, exclusions)

## Detailed Data Flow

1. **Incoming request**: the application sends a request to `localhost:3100/v1/messages`
2. **Content extraction**: the proxy extracts text from messages (user, system, assistant)
3. **PII detection**: the ONNX model analyzes the text and returns detected entities with their positions
4. **Pseudonymization**: each entity is replaced by a fictitious value, the mapping is stored
5. **Sending**: the cleaned request is forwarded to the real API
6. **Response**: the API response is intercepted
7. **De-pseudonymization**: pseudonyms found in the response are replaced with the originals
8. **Return**: the restored response is sent back to the application

## Technical Constraints

- **Single binary**: no external dependency installation required
- **Cross-platform**: Windows, macOS, Linux
- **Performance**: added latency < 100ms (detection + replacement)
- **Memory**: footprint < 1 GB (model + runtime + mapping)
- **Security**: mapping never persisted to disk, never logged, encrypted in memory
