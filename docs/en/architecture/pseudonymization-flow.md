# Pseudonymization Flow

## Complete Pipeline

```
Incoming request
       │
       ▼
┌──────────────────┐
│ 1. Parse request │  Extract textual content (messages, system prompt)
└──────────────────┘
       │
       ▼
┌──────────────────┐
│ 2. Tokenization  │  Prepare text for the ONNX model
└──────────────────┘
       │
       ▼
┌──────────────────┐
│ 3. PII Detection │  ONNX model → list of entities with positions and types
└──────────────────┘  E.g.: [{text: "Tardy", type: "PERSON", start: 42, end: 47}]
       │
       ▼
┌──────────────────┐
│ 4. Pseudonym     │  For each entity, generate a consistent pseudonym:
│    generation    │  - Name → another name (same cultural origin if possible)
└──────────────────┘  - IP → another IP (same fictitious subnet)
       │              - Email → another email (same fictitious domain)
       ▼
┌──────────────────┐
│ 5. Mapping       │  Store {id, original, pseudonym, type, session}
└──────────────────┘  Encrypted with AES-256-GCM in memory
       │
       ▼
┌──────────────────┐
│ 6. Replacement   │  Substitute in text (descending positions to preserve offsets)
└──────────────────┘
       │
       ▼
  Cleaned request → LLM API
```

## De-pseudonymization (response)

```
LLM API Response
       │
       ▼
┌──────────────────┐
│ 1. Scan response │  Search for all known pseudonyms in the text
└──────────────────┘
       │
       ▼
┌──────────────────┐
│ 2. Reverse       │  Replace each pseudonym with the original value
│    replacement   │  via the mapping table
└──────────────────┘
       │
       ▼
  Restored response → Application
```

## Edge Cases

### SSE Streaming
- LLM responses arrive token by token
- The de-pseudonymizer maintains a buffer to detect multi-token pseudonyms
- E.g.: if the pseudonym is "Gerard", it may arrive as "Ger" + "ard" → buffer needed

### Multi-word Pseudonyms
- "Jean-Pierre Dupont" → "Michel Martin" (the mapping covers the complete entity)
- De-pseudonymization must handle variants (initials, truncations by the LLM)

### Session Consistency
- Same data = same pseudonym throughout the entire conversation
- "Tardy" will always be replaced by "Gerard" within the same session
- Between sessions, pseudonyms change (no persistence)

### False Positives
- The model may detect a false positive (e.g., a variable name that looks like a person's name)
- The user can configure exclusions (whitelist)
- The dashboard displays detections for manual review

## Pseudonym Generation by Type

| PII Type | Replacement Strategy |
|----------|----------------------|
| Person name | Fictitious name (built-in dictionary) |
| IPv4 address | IP in a fictitious range (e.g., 10.0.x.x) |
| IPv6 address | Fictitious IPv6 address |
| Email | `{firstname}@example.com` |
| Phone number | Fictitious number (format preserved) |
| IBAN | Fictitious IBAN (valid checksum) |
| Credit card | Fictitious number (valid Luhn) |
| Postal address | Fictitious address (same country) |
| API key / token | Truncated random hash |
| Internal URL | `https://internal.example.com/...` |
| File path | Generic path (`/home/user/...`) |
