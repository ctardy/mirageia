# Detailed Architecture -- MirageIA

> Reference document describing the precise operation of each component, their interactions, and the associated technical decisions.

---

## Table of Contents

1. [Overview](#1-overview)
2. [Request Lifecycle](#2-request-lifecycle)
3. [Component 1 -- HTTP Proxy](#3-component-1--http-proxy)
4. [Component 2 -- PII Detector](#4-component-2--pii-detector)
5. [Component 3 -- Pseudonymizer](#5-component-3--pseudonymizer)
6. [Component 4 -- Mapping Table](#6-component-4--mapping-table)
7. [Component 5 -- De-pseudonymizer](#7-component-5--de-pseudonymizer)
8. [Component 6 -- Dashboard](#8-component-6--dashboard)
9. [SSE Streaming Management](#9-sse-streaming-management)
10. [Security and Encryption](#10-security-and-encryption)
11. [Error Handling](#11-error-handling)
12. [Performance Constraints](#12-performance-constraints)
13. [Rust Module Structure](#13-rust-module-structure)

---

## 1. Overview

MirageIA is a **single process** that contains all of the following components:

```
┌──────────────────────────────────────────────────────────────────────────┐
│                        MirageIA (single process)                       │
│                                                                         │
│  ┌───────────┐   ┌─────────────┐   ┌────────────────┐   ┌───────────┐  │
│  │   HTTP    │──▶│    PII      │──▶│ Pseudonymizer  │──▶│  HTTP     │  │
│  │   Proxy   │   │  Detector   │   │                │   │  Client   │  │
│  │  (axum)   │◀──│  (ONNX)     │◀──│  Mapping table │◀──│ (reqwest) │  │
│  └───────────┘   └─────────────┘   └────────────────┘   └───────────┘  │
│       ▲  │            │                    │                   │  ▲     │
│       │  │       ┌────┘                    │                   │  │     │
│       │  │       ▼                         ▼                   │  │     │
│       │  │  ┌──────────┐          ┌──────────────┐             │  │     │
│       │  └─▶│ De-pseu- │          │   Events     │             │  │     │
│       │     │ donymizer│          │  (dashboard)  │             │  │     │
│       │     └──────────┘          └──────────────┘             │  │     │
│       │          │                       │                     │  │     │
│       │          ▼                       ▼                     │  │     │
│       │   ┌──────────┐          ┌──────────────┐              │  │     │
│       │   │ Streaming│          │  Dashboard   │              │  │     │
│       │   │ buffer   │          │  (Tauri)     │              │  │     │
│       │   └──────────┘          └──────────────┘              │  │     │
│       │                                                       │  │     │
└───────┼───────────────────────────────────────────────────────┼──┼─────┘
        │                                                       │  │
   original                                             cleaned │  │ raw
   request                                              request │  │ response
        ▲                                                       ▼  │
  ┌───────────┐                                        ┌──────────────┐
  │ Application│                                       │ Anthropic API│
  │ (Claude    │                                       │ / OpenAI     │
  │  Code, etc)│                                       └──────────────┘
  └───────────┘
```

### Fundamental Principle

MirageIA is a **benevolent man-in-the-middle proxy**: it interposes itself between the client application and the LLM API. The application does not know its data is being pseudonymized, and the API does not know the data is fictitious. Transparency is total on both sides.

---

## 2. Request Lifecycle

### 2.1 Outbound Flow (request)

```
 ① Reception           ② Extraction          ③ PII Detection
────────────────    ────────────────────   ─────────────────────
POST /v1/messages   Parse JSON body        ONNX model analyzes
Headers copied      Extract text fields    the text and returns
Body read in full   from messages          detected entities
                    (content, system)      [{text, type, pos}]

 ④ Pseudonymization    ⑤ Reconstruction     ⑥ Sending
─────────────────────  ──────────────────   ──────────────────
Each PII entity        Replace PIIs in      Cleaned request
→ pseudonym generated  the JSON body        sent to the API
→ stored in mapping    with pseudonyms      via reqwest
                       Recalculate offsets   Auth headers passed
                       + Content-Length     as-is
```

### 2.2 Return Flow (response)

```
 ⑦ Response reception   ⑧ De-pseudonymization   ⑨ Client return
─────────────────────  ──────────────────────  ──────────────────
Response from API      Scan text to find       Restored response
(complete or SSE)      known pseudonyms        sent back to app
                       in the mapping          The client receives
                       Replace with original   the real data
                       values
```

### 2.3 Sequence Diagram

```
Application          MirageIA                    LLM API
    │                    │                           │
    │─── POST /v1/msg ──▶│                           │
    │   "My name is      │                           │
    │    Tardy, IP       │                           │
    │    192.168.1.22"   │                           │
    │                    │                           │
    │                    │── PII Detection ────┐     │
    │                    │   ONNX Runtime      │     │
    │                    │◀───────────────────┘     │
    │                    │  [{Tardy, PERSON, 12-17}  │
    │                    │   {192.168.1.22, IP, ...}] │
    │                    │                           │
    │                    │── Pseudonymization ──┐    │
    │                    │   Tardy → Gerard      │    │
    │                    │   192.168.1.22        │    │
    │                    │     → 10.0.42.7       │    │
    │                    │◀────────────────────┘    │
    │                    │                           │
    │                    │──── POST /v1/messages ───▶│
    │                    │  "My name is Gerard,      │
    │                    │   IP 10.0.42.7"           │
    │                    │                           │
    │                    │◀──── Response ───────────│
    │                    │  "Hello Gerard,            │
    │                    │   your IP 10.0.42.7..."   │
    │                    │                           │
    │                    │── De-pseudonymization ─┐  │
    │                    │   Gerard → Tardy        │  │
    │                    │   10.0.42.7             │  │
    │                    │     → 192.168.1.22      │  │
    │                    │◀──────────────────────┘  │
    │                    │                           │
    │◀── Response ───────│                           │
    │  "Hello Tardy,     │                           │
    │   your IP          │                           │
    │   192.168.1.22..." │                           │
    │                    │                           │
```

---

## 3. Component 1 -- HTTP Proxy

### 3.1 Role

Single entry point. Listens on `localhost:3100` and intercepts requests destined for LLM APIs.

### 3.2 Routing by Provider

The proxy determines the target provider from the request path:

| Received Path | Target Provider | Upstream URL |
|---|---|---|
| `/v1/messages` | Anthropic | `https://api.anthropic.com/v1/messages` |
| `/v1/chat/completions` | OpenAI | `https://api.openai.com/v1/chat/completions` |
| Any other path | Passthrough | Direct forwarding without pseudonymization |

### 3.3 Header Handling

```
Incoming headers (application)
    │
    ├── x-api-key / Authorization: Bearer  →  Passed as-is to the API
    ├── Content-Type                        →  Preserved (application/json)
    ├── Content-Length                      →  Recalculated after pseudonymization
    ├── anthropic-version                  →  Passed as-is
    └── Accept: text/event-stream          →  Streaming mode indicator
```

The proxy optionally adds `X-MirageIA: active` to the response (can be disabled in configuration).

### 3.4 Textual Content Extraction

The proxy parses the JSON body and extracts the text fields to analyze based on the provider:

**Anthropic** (`/v1/messages`):
```json
{
  "system": "You are an assistant...",       ← analyzed
  "messages": [
    {
      "role": "user",
      "content": "My name is Tardy..."      ← analyzed
    },
    {
      "role": "assistant",
      "content": "Hello Tardy..."           ← analyzed
    }
  ]
}
```

**OpenAI** (`/v1/chat/completions`):
```json
{
  "messages": [
    {
      "role": "system",
      "content": "You are an assistant..."   ← analyzed
    },
    {
      "role": "user",
      "content": "My name is Tardy..."      ← analyzed
    }
  ]
}
```

Non-textual fields (`model`, `max_tokens`, `temperature`, `tools`, etc.) are **never** modified.

### 3.5 Multipart Content Handling

Anthropic messages support multipart content (text + images):

```json
{
  "content": [
    {"type": "text", "text": "Analyze this image..."},    ← analyzed
    {"type": "image", "source": {"type": "base64", ...}}  ← ignored
  ]
}
```

Only `{"type": "text"}` blocks are analyzed. Images, files, and other binary types are transmitted without modification.

### 3.6 Technical Stack

| Crate | Role |
|---|---|
| `axum` | Async HTTP server (routes, middleware) |
| `reqwest` | HTTP client for calling the upstream API |
| `tokio` | Async runtime (io, timers, channels) |
| `serde_json` | JSON parsing/serialization |
| `eventsource-stream` | SSE stream parsing (streaming responses) |

---

## 4. Component 2 -- PII Detector

### 4.1 Role

Analyzes the extracted text and returns a list of PII entities with their position, type, and confidence score.

### 4.2 Detection Pipeline

```
Raw text
    │
    ▼
┌────────────────────┐
│ Pre-processing     │  Unicode normalization, segmentation
└────────────────────┘  if text exceeds the model window (512 tokens)
    │
    ▼
┌────────────────────┐
│ Tokenization       │  HuggingFace tokenizer (`tokenizers` crate)
└────────────────────┘  Text → token IDs + attention mask
    │
    ▼
┌────────────────────┐
│ ONNX Inference     │  Model loaded via `ort` (ONNX Runtime Rust)
└────────────────────┘  Input: token IDs → Output: per-token logits
    │
    ▼
┌────────────────────┐
│ Post-processing    │  BIO/BILOU decoding → entities with positions
└────────────────────┘  Sub-token merging (##ard → Tardy)
    │                   Filtering by confidence score (configurable threshold)
    ▼
PII entity list
```

### 4.3 Detector Output Format

```rust
struct PiiEntity {
    text: String,           // "Tardy"
    entity_type: PiiType,   // PiiType::PersonName
    start: usize,           // start position in original text
    end: usize,             // end position in original text
    confidence: f32,        // 0.0 — 1.0
}

enum PiiType {
    PersonName,       // Names, first names, pseudonyms
    Email,            // Email addresses
    IpAddress,        // IPv4, IPv6
    PhoneNumber,      // Phone numbers
    PostalAddress,    // Postal addresses
    CreditCard,       // Credit card numbers
    Iban,             // IBAN numbers
    NationalId,       // Social security number, passport, etc.
    ApiKey,           // API keys, tokens, secrets
    InternalUrl,      // Internal URLs / private domains
    ServerName,       // Server names
    FilePath,         // Sensitive file paths
}
```

### 4.4 Handling Long Texts

The model has a limited context window (512 tokens for DistilBERT). For longer texts:

1. **Segmentation** with 64-token overlap
2. **Inference on each segment** independently
3. **Result merging**: deduplication of entities in overlap zones (keep the one with the highest confidence score)

```
Text of 1500 tokens:

Segment 1: tokens   0–511   ──▶ inference ──▶ entities
Segment 2: tokens 448–959   ──▶ inference ──▶ entities
Segment 3: tokens 896–1500  ──▶ inference ──▶ entities
                     ▲
               64-token
               overlap

Merge: deduplicate entities in zones 448–511 and 896–959
```

### 4.5 Confidence Threshold

- **Default threshold**: 0.75
- Entities below the threshold are ignored (not pseudonymized)
- The threshold is **configurable per PII type** to adjust sensitivity:
  - API keys, secrets: low threshold (0.5) → better a false positive
  - Person names: standard threshold (0.75) → avoid pseudonymizing "Thomas Edison"

### 4.6 ONNX Model -- Loading

```
MirageIA startup
    │
    ├── Check ~/.mirageia/models/{model}.onnx
    │   ├── Exists → load into memory via ort
    │   └── Does not exist → download from GitHub Release
    │                        → save to ~/.mirageia/models/
    │                        → load into memory
    │
    └── Model loaded → ONNX session ready
        (target loading time: < 3 seconds)
```

---

## 5. Component 3 -- Pseudonymizer

### 5.1 Role

Receives the list of detected PII entities and generates a consistent pseudonym for each one.

### 5.2 Replacement Strategies by Type

| PII Type | Strategy | Example |
|---|---|---|
| `PersonName` | Fictitious name from a built-in dictionary | Tardy → Gerard |
| `Email` | `{fictitious_firstname}@example.com` | chris@dom.fr → paul@example.com |
| `IpAddress` (v4) | IP in the 10.0.0.0/8 range | 192.168.1.22 → 10.0.42.7 |
| `IpAddress` (v6) | Fictitious IPv6 in fd00::/8 | fe80::1 → fd00::a1b2:c3d4 |
| `PhoneNumber` | Fictitious number, format preserved | 06 12 34 56 78 → 06 98 76 54 32 |
| `PostalAddress` | Fictitious address, same country | 12 rue X, Paris → 8 av Y, Lyon |
| `CreditCard` | Fictitious number (valid Luhn) | 4532... → 4111... |
| `Iban` | Fictitious IBAN (valid checksum) | FR76... → FR14... |
| `NationalId` | Fictitious ID, same format | 1 85 07... → 2 91 03... |
| `ApiKey` | Truncated random hash, same length | sk-abc123... → sk-xyz789... |
| `InternalUrl` | `https://internal.example.com/...` | srv.corp.local → internal.example.com |
| `FilePath` | Generic path | /home/chris/... → /home/user/... |

### 5.3 Session Consistency

Within the same session (conversation), **the same data always produces the same pseudonym**:

```
Message 1: "Contact Tardy at chris@dom.fr"
            → "Contact Gerard at paul@example.com"

Message 5: "Tardy confirmed by email"
            → "Gerard confirmed by email"
               ^^^^^^
               same pseudonym because same session
```

Consistency is ensured by a lookup in the mapping table **before** generating a new pseudonym.

### 5.4 Subnet Coherence for Grouped IPs

When multiple IPv4 addresses share the same /24 network prefix, MirageIA generates pseudonyms that **preserve this relationship**:

```
Originals:   10.0.1.10, 10.0.1.20, 10.0.1.30  (same /24: 10.0.1.0)
Pseudonyms:  142.87.53.10, 142.87.53.20, 142.87.53.30
             ^^^^^^^^^^                    ^^^^^^^^^^
             same pseudo prefix            host part preserved
```

This ensures the LLM reasons correctly about network relationships (same subnet, broadcast, etc.) even when working with pseudonyms.

**Algorithm**:
1. Before generation, group IPs by /24 prefix
2. For each group of ≥ 2 IPs, generate a shared pseudo prefix
3. Preserve the original host part (last octet)
4. Isolated IPs use standard generation (10.0.x.x)

### 5.5 Text Replacement

Replacements are performed in **descending position order** to preserve offsets:

```
Text: "Contact Tardy (chris@dom.fr) for the project"
               ^^^^^  ^^^^^^^^^^^^^^
               pos 8   pos 15

Replacement in descending order:
  1. pos 15–28: chris@dom.fr → paul@example.com
  2. pos 8–13:  Tardy → Gerard

Result: "Contact Gerard (paul@example.com) for the project"
```

If replacements were done in ascending order, replacing "Tardy" (5 → 6 chars) would shift the email's position.

### 5.6 Variable-Length Pseudonyms

When a pseudonym has a different length than the original, all offsets in the JSON are recalculated. The JSON body is reconstructed after all replacements, not modified in-place.

---

## 6. Component 4 -- Mapping Table

### 6.1 Role

Stores the bidirectional correspondence between original values and their pseudonyms. Enables de-pseudonymization in responses.

### 6.2 Structure

```rust
struct MappingEntry {
    id: u64,                // unique identifier
    original: String,       // "Tardy"
    pseudonym: String,      // "Gerard"
    pii_type: PiiType,      // PiiType::PersonName
    created_at: Instant,    // creation timestamp
}

struct MappingTable {
    // Fast lookup in both directions
    by_original: HashMap<String, MappingEntry>,   // original → entry
    by_pseudonym: HashMap<String, MappingEntry>,  // pseudonym → entry
    cipher: Aes256Gcm,                            // encryption key
}
```

### 6.3 Lifecycle

```
MirageIA startup
    │
    ├── Generate a random AES-256 key (in memory only)
    ├── Initialize empty table
    │
    │   For each request:
    ├── Lookup: does the PII already exist? → return existing pseudonym
    ├── Otherwise: generate a pseudonym, encrypt, store
    │
    │   For each response:
    ├── Reverse lookup: does the pseudonym exist? → return original
    │
    │   End of session:
    └── Table destroyed with the process memory
        (never persisted to disk)
```

### 6.4 Security Invariants

- The AES-256 key is randomly generated at each startup
- Original values are encrypted in memory (not stored in cleartext)
- The table is **never** written to disk (no file, no database)
- The table is **never** logged (no log contains an original value or mapping)
- When the process shuts down, memory is freed and data is lost

---

## 7. Component 5 -- De-pseudonymizer

### 7.1 Role

Scans LLM API responses to find known pseudonyms and replace them with original values.

### 7.2 Algorithm (complete response)

De-pseudonymization is performed in **two passes**:

```
API Response (text)
    │
    ▼
┌─────────────────────────────────────────┐
│ Pass 1 — Complete token replacement     │
│ (AhoCorasick)                           │
│                                         │
│ For each pseudonym in the mapping:      │
│   → Exact search in the text            │
│   → Replace with original value         │
│   → Longest first (priority)            │
└─────────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────────┐
│ Pass 2 — SPB (Sub-PII Binding)          │
│ Fragment restoration                    │
│                                         │
│ For each mapping of a decomposable type │
│ (IP, CC, SSN):                          │
│   → Extract structural fragments        │
│     (IP octets, CC groups, SSN segments)│
│   → Replace pseudo fragments with       │
│     original fragments                  │
│   → With false-positive guards          │
│     (word boundaries, context)          │
└─────────────────────────────────────────┘
    │
    ▼
Restored response
```

#### Why SPB is needed

When the LLM receives a pseudonym, it may decompose it in its response:

```
Request  : "Write this IP in decimal notation: 10.0.84.12"
                                                ^^^^^^^^^^
                                                (pseudonym of 172.16.254.3)

Response : "The address 10.0.84.12 has octets: 10, 0, 84, 12.
            The first octet 10 indicates a class A network."

After pass 1: "The address 172.16.254.3 has octets: 10, 0, 84, 12.
               The first octet 10 indicates a class A network."
                                       ^^^^^^^^^^^^^^^^^^^^^^
                                       pseudo fragments not restored!

After pass 2: "The address 172.16.254.3 has octets: 172, 16, 254, 3.
(SPB)          The first octet 172 indicates a class B network."
               ✓ Consistent
```

#### Decomposable types supported by SPB

| PII Type | Fragments | Example |
|----------|-----------|---------|
| `IpAddress` (v4) | Octets (separator `.`) | 10.0.84.12 → [10, 0, 84, 12] |
| `CreditCard` | Groups of 4 digits | 4832759104628371 → [4832, 7591, 0462, 8371] |
| `NationalId` | Segments (separator space) | 2 91 03 42 → [2, 91, 03, 42] |

#### False-positive guards

- **Fragments ≥ 2 characters**: replacement with word boundaries (`\b`)
- **Single-character fragments**: replacement only in analytical context (after `=`, `:`, or `,`)
- **Deduplication**: if the same pseudo fragment maps to multiple different originals (ambiguity), it is not replaced

### 7.3 De-pseudonymization Edge Cases

**Variants generated by the LLM**: the LLM may transform a pseudonym in unexpected ways:

| Pseudonym sent | What the LLM may respond | Strategy |
|---|---|---|
| `Gerard` | `Gerard` | Exact match → replacement |
| `Gerard` | `Mr. Gerard` | Partial match → replace "Gerard" |
| `Gerard` | `GERARD` | Case-insensitive match (optional) |
| `Gerard` | `Gérard` (with accent) | Unicode normalization before comparison |
| `paul@example.com` | `paul@example.com` | Exact match |
| `paul@example.com` | `paul` | No match (too ambiguous) |

### 7.4 Replacement Priority

The longest pseudonyms are replaced first to avoid conflicts:

```
Mapping: "Jean-Pierre Gerard" → "Jean-Pierre Tardy"
         "Gerard"             → "Tardy"

Text: "Jean-Pierre Gerard confirmed"

Order: first "Jean-Pierre Gerard" (19 chars), then "Gerard" (6 chars)
→ Avoids replacing "Gerard" alone within "Jean-Pierre Gerard"
```

---

## 8. Component 6 -- Dashboard

### 8.1 Role

Minimal local graphical interface (tray icon + Tauri webview) for monitoring MirageIA activity in real time.

### 8.2 Features

```
┌─────────────────────────────────────────────────────────┐
│  MirageIA Dashboard                          ─  □  ✕   │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  ● Proxy active — localhost:3100           [Pause] [■]  │
│                                                         │
│  ┌─ Current session ──────────────────────────────────┐ │
│  │ Requests processed : 42                            │ │
│  │ PII detected       : 127                           │ │
│  │ Types: 38 names, 22 emails, 15 IPs, 52 other      │ │
│  └────────────────────────────────────────────────────┘ │
│                                                         │
│  ┌─ Latest detections ────────────────────────────────┐ │
│  │ 14:32:01  PERSON     "████" → "Gerard"     ✓ 0.92 │ │
│  │ 14:32:01  EMAIL      "████" → "paul@ex…"  ✓ 0.98 │ │
│  │ 14:32:01  IP_ADDR    "████" → "10.0.42.7" ✓ 0.95 │ │
│  │ 14:31:58  PERSON     "████" → "Martin"    ✓ 0.88 │ │
│  │ 14:31:58  API_KEY    "████" → "sk-xyz…"   ✓ 0.97 │ │
│  └────────────────────────────────────────────────────┘ │
│                                                         │
│  ┌─ Configuration ────────────────────────────────────┐ │
│  │ Confidence threshold: [0.75] ◄──────────►         │ │
│  │ Active types: ☑ Names ☑ Emails ☑ IPs ☑ API Keys  │ │
│  │ Exclusions: [thomas edison, localhost, ...]        │ │
│  └────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

### 8.3 Dashboard Security

- Original values are **never** displayed in the dashboard (masked with `████`)
- Only pseudonyms, types, and scores are visible
- The dashboard is only accessible locally (no network exposure)

### 8.4 Proxy ↔ Dashboard Communication

The proxy emits events to the dashboard via an internal channel (Tauri events):

```rust
enum DashboardEvent {
    PiiDetected {
        pii_type: PiiType,
        pseudonym: String,      // not the original
        confidence: f32,
        timestamp: Instant,
    },
    RequestProcessed {
        provider: Provider,
        pii_count: usize,
        latency_ms: u64,
    },
    ProxyStatusChanged {
        status: ProxyStatus,    // Active, Paused, Error
    },
}
```

---

## 9. SSE Streaming Management

### 9.1 The Challenge

LLM APIs send responses token by token via Server-Sent Events. A pseudonym can be split across multiple tokens:

```
Tokens received from the API: "The" " name" " is" " Ger" "ard" "."
                                                    ^^^^  ^^^
                                              The pseudonym "Gerard" is
                                              split across two tokens
```

### 9.2 Streaming Buffer Architecture

```
Incoming SSE stream (token by token)
    │
    ▼
┌────────────────────────────────────┐
│         Circular buffer            │
│                                    │
│  Size = max length of pseudonyms   │
│  in the mapping                    │
│                                    │
│  Contents: "...is Ger"             │
│                   ^^^              │
│            not yet flushed         │
│            (potential prefix       │
│             of a pseudonym)        │
└────────────────────────────────────┘
    │
    ├── New token "ard" arrives
    │   Buffer = "...is Gerard"
    │                  ^^^^^^^
    │   "Gerard" recognized in the mapping!
    │
    ├── Replace "Gerard" → "Tardy"
    ├── Flush "...is Tardy" to the client
    └── Clear the buffer
```

### 9.3 Detailed Algorithm

```
For each SSE token received:
    1. Add the token to the buffer
    2. Check if the buffer contains a complete pseudonym
       → If yes: replace and flush
    3. Check if the buffer ends with a known pseudonym prefix
       → If yes: wait for the next token (the pseudonym may be in progress)
       → If no: flush the beginning of the buffer (not a pseudonym)
    4. If the buffer exceeds the max size: force flush the beginning
```

### 9.4 Buffer-Added Latency

- **Nominal case** (no pseudonym in the response): near-zero latency, tokens are flushed immediately
- **Pseudonym case**: latency = time to receive all tokens of the pseudonym (typically 2-4 tokens, i.e., 50-200ms)
- **Max buffer size**: configurable, default = length of the longest pseudonym + margin

### 9.5 SSE Format

The proxy reconstructs SSE events in the same format as the API:

```
data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"Tardy"}}

data: [DONE]
```

The `event:`, `id:`, and `retry:` fields are transmitted as-is.

---

## 10. Security and Encryption

### 10.1 Threat Model

MirageIA protects against:
- **Data leakage to LLM APIs**: PII never leaves the machine
- **In-memory mapping reads**: AES-256-GCM encryption

MirageIA does **not** protect against:
- An attacker with root access to the machine (memory dump possible)
- Interception between the app and the local proxy (localhost, negligible risk)

### 10.2 Mapping Encryption

```
Original value ("Tardy")
    │
    ▼
┌─────────────────────────────┐
│ AES-256-GCM                │
│ Key: randomly generated     │  ← 256 bits, never persisted
│      at each startup        │
│ Nonce: unique per entry     │  ← 96 bits, random
│ AAD: PII type + ID          │  ← authenticated additional data
└─────────────────────────────┘
    │
    ▼
Encrypted value (stored in the table)
```

### 10.3 What Is Never Exposed

| Data | In memory | On disk | In logs | In the dashboard |
|---|---|---|---|---|
| Original value | Encrypted (AES-256) | Never | Never | Never |
| Pseudonym | Cleartext | Never | Optional | Yes |
| AES key | Cleartext | Never | Never | Never |
| Complete mapping | Encrypted | Never | Never | Never |

---

## 11. Error Handling

### 11.1 Principle: fail-open vs fail-closed

MirageIA adopts a **fail-open** approach: in case of an error in the pseudonymization pipeline, the request is forwarded **as-is** to the API rather than blocking the user.

Rationale: MirageIA is an optional protection tool, not a firewall. Blocking the user's workflow is worse than occasionally letting a non-pseudonymized request through.

### 11.2 Error Scenarios

| Error | Behavior | Notification |
|---|---|---|
| ONNX model not loaded | Passthrough (request forwarded as-is) | Dashboard: warning |
| ONNX inference fails | Passthrough for this message | Dashboard: warning |
| Upstream API unreachable | Return the HTTP error to the client | Dashboard: error |
| Malformed JSON in request | Passthrough (no parsing) | Dashboard: warning |
| SSE buffer overflow | Forced flush without replacement | Dashboard: warning |
| Mapping decryption fails | Ignore the mapping entry | Internal error log |

### 11.3 Logs

- **No log ever contains original data** (PII)
- Logs contain: timestamps, detected PII types, scores, counters, errors
- Configurable log level (error, warn, info, debug)
- Logs are written to stderr (stdout is reserved for the proxy)

---

## 12. Performance Constraints

### 12.1 Objectives

| Metric | Target | Rationale |
|---|---|---|
| Added latency (non-streaming) | < 100ms | Imperceptible to the user |
| Added latency (streaming) | < 50ms per chunk | No visible stuttering |
| Startup time | < 5s | Including ~3s to load the ONNX model |
| Idle memory | < 200 MB | Model loaded, no request in progress |
| Memory under load | < 800 MB | Model + mapping + buffers |
| Binary size (without model) | < 30 MB | Fast download |
| Model size (DistilBERT INT8) | ~260 MB | Downloaded on first launch |

### 12.2 Planned Optimizations

- **Tokenizer cache**: tokens are reused if the same text is submitted
- **Batch inference**: if multiple messages in a request, analyze them in a single ONNX batch
- **O(1) mapping lookup**: HashMap for both directions (original → pseudo, pseudo → original)
- **Allocation-free replacement**: pre-allocate the output buffer to the estimated size

---

## 13. Rust Module Structure

```
src-tauri/
├── src/
│   ├── main.rs                  Entry point, Tauri + proxy initialization
│   │
│   ├── proxy/
│   │   ├── mod.rs               Public proxy module
│   │   ├── server.rs            axum server (routes, middleware)
│   │   ├── router.rs            Provider-based routing (Anthropic / OpenAI)
│   │   ├── extractor.rs         Textual content extraction from JSON
│   │   └── client.rs            reqwest HTTP client (upstream calls)
│   │
│   ├── detection/
│   │   ├── mod.rs               Public detection module
│   │   ├── model.rs             ONNX loading and inference (ort crate)
│   │   ├── tokenizer.rs         Tokenization (tokenizers crate)
│   │   ├── postprocess.rs       Post-processing (BIO → entities, sub-token merging)
│   │   └── types.rs             PiiEntity, PiiType, confidence thresholds
│   │
│   ├── pseudonymization/
│   │   ├── mod.rs               Public pseudonymization module
│   │   ├── generator.rs         Pseudonym generation by type
│   │   ├── replacer.rs          Text replacement (offset management)
│   │   ├── depseudonymizer.rs   Response de-pseudonymization (pass 1 + SPB)
│   │   ├── fragment_restorer.rs Fragment restoration (SPB — Sub-PII Binding)
│   │   └── dictionaries.rs      Built-in dictionaries (first names, last names)
│   │
│   ├── mapping/
│   │   ├── mod.rs               Public mapping module
│   │   ├── table.rs             Bidirectional mapping table
│   │   └── crypto.rs            AES-256-GCM encryption/decryption
│   │
│   ├── streaming/
│   │   ├── mod.rs               Public streaming module
│   │   ├── buffer.rs            Circular buffer for SSE
│   │   ├── sse_parser.rs        SSE event parsing
│   │   └── sse_writer.rs        SSE event reconstruction
│   │
│   ├── dashboard/
│   │   ├── mod.rs               Public dashboard module
│   │   ├── events.rs            Tauri events (PiiDetected, RequestProcessed)
│   │   └── state.rs             Shared state for the dashboard
│   │
│   └── config/
│       ├── mod.rs               Public configuration module
│       └── settings.rs          Settings (port, thresholds, exclusions, active types)
│
├── models/                      ONNX models (gitignored, downloaded at runtime)
├── dictionaries/                Pseudonym dictionaries (embedded in the binary)
│   ├── firstnames.json
│   ├── lastnames.json
│   └── addresses.json
├── Cargo.toml
└── tauri.conf.json
```

### 13.1 Cargo.toml Dependencies

| Crate | Target Version | Role |
|---|---|---|
| `tauri` | 2.x | Desktop framework (tray, webview, events) |
| `axum` | 0.7 | Async HTTP server |
| `reqwest` | 0.12 | HTTP client |
| `tokio` | 1.x | Async runtime |
| `serde` / `serde_json` | 1.x | JSON serialization |
| `ort` | 2.x | ONNX Runtime (Rust bindings) |
| `tokenizers` | 0.20 | HuggingFace tokenization |
| `aes-gcm` | 0.10 | AES-256-GCM encryption |
| `rand` | 0.8 | Random generation (keys, pseudonyms) |
| `tracing` | 0.1 | Structured logging |
| `eventsource-stream` | 0.2 | SSE parsing |
