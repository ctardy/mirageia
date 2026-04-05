# PII Detection — Current Implementation and Roadmap

## Current Phase: Regex + Algorithmic Validation

MirageIA's PII detection relies on two complementary layers, requiring no external server or GPU.

### Layer 1 — Algorithmically Validated Patterns (confidence 0.95)

These patterns combine a broad regex with a checksum validator:

| PII Type | Regex | Validator | Algorithm |
|----------|-------|-----------|-----------|
| IBAN | `\b[A-Z]{2}\d{2}(?:\s?[A-Z0-9]{4}){2,7}…\b` | `iban_valid()` | MOD-97 (ISO 13616) |
| Credit card | Visa / MC / Amex / Discover | `luhn_valid()` | Luhn (ISO/IEC 7812) |

**MOD-97 principle (IBAN)**: move the first 4 characters to the end, replace letters (A=10…Z=35), compute modulo 97 in 9-digit blocks. Expected result: 1.

**Luhn principle (credit cards)**: double every 2nd digit from the right (if result ≥ 10, subtract 9), sum all digits, the total must be divisible by 10.

### Layer 2 — Simple Patterns (confidence 0.90)

Regex patterns without post-match validation. **Execution order is critical** (see section below):

| Priority | Type | Examples detected |
|----------|------|-------------------|
| 1 | Anthropic key | `sk-ant-api03-…` |
| 2 | OpenAI key | `sk-proj-…`, `sk-…` (48+ chars) |
| 3 | Stripe key | `sk_live_…`, `pk_test_…` |
| 4 | GitHub token | `ghp_…`, `gho_…`, `ghu_…` |
| 5 | Slack token | `xoxb-…`, `xoxp-…` |
| 6 | AWS Access Key | `AKIA…`, `ASIA…` |
| 7 | JWT | `eyJ…` |
| 8 | Generic key | `sk-…`, `api-…`, `token-…` |
| 9 | Email | `user@domain.tld` |
| 10 | IPv4 | `192.168.1.22` |
| 11 | IPv6 | `2001:db8::1` |
| 12 | French phone | `06 12 34 56 78`, `+33 6…` |
| 13 | National ID | `1 85 12 75 123 456 78` |

Sources: patterns inspired by [Presidio (MIT)](https://github.com/microsoft/presidio) and [gitleaks (MIT)](https://github.com/gitleaks/gitleaks).

### Layer 3 — Shannon Entropy (generic secrets)

For high-entropy strings without a known prefix:

```rust
pub fn looks_like_secret(s: &str) -> bool {
    shannon_entropy(s) > 3.5
        && s.len() >= 12
        && char_class_count(s) >= 3  // lowercase + uppercase + digits + specials
}
```

### Execution Order and Overlap Detection

```
validated_patterns (IBAN, credit cards)
      ↓ confidence 0.95, absolute priority
specific patterns (API keys)
      ↓ confidence 0.90, run first
generic patterns (email, IP, phone)
      ↓ skipped if overlapping with an already-registered entity
```

**Critical rule**: API key patterns must be placed **before** generic patterns (phone, IP) in the list. Otherwise, the phone pattern can match digits contained within an API key (e.g., `0123456789` inside `sk-ant-api03-…0123456789AB`), register first, and the overlap filter then blocks detection of the entire key.

### Layer 4 — ONNX Contextual Model (confidence 0.80–0.95)

An embedded NER model detects entities that regex cannot reliably handle without context: person names, organizations, addresses.

Model: `iiiorg/piiranha-v1-detect-personal-information` (337 MB ONNX INT8, ~946 MB RSS, multilingual including FR). See [ONNX integration reference](onnx-integration.md) for full details.

ONNX entities are added only if they do not overlap with regex results. If the model is missing, MirageIA starts in regex-only mode (fail-open).

## Rust Implementation

```
src/detection/
├── mod.rs               — PiiDetector struct (ONNX), load_label_map()
├── types.rs             — PiiEntity, PiiType
├── regex_detector.rs    — RegexDetector (patterns + validated_patterns)
├── validator.rs         — iban_valid(), luhn_valid(), shannon_entropy()
├── tokenizer.rs         — PiiTokenizer (HuggingFace tokenizers crate)
├── model.rs             — PiiModel (ort Session, infer())
├── model_manager.rs     — download/cache/verify, get_active_model()
├── postprocess.rs       — logits_to_predictions(), extract_entities(), BIO merge
└── error.rs             — DetectionError
```

### `RegexDetector::detect()` — algorithm

```rust
pub fn detect(&self, text: &str) -> Vec<PiiEntity> {
    let mut entities = Vec::new();

    // 1. Validated patterns (IBAN, credit cards) — confidence 0.95
    for (pii_type, regex, validator) in &self.validated_patterns {
        for mat in regex.find_iter(text) {
            if validator(mat.as_str()) {
                push_if_new(&mut entities, …, 0.95);
            }
        }
    }

    // 2. Simple patterns — confidence 0.90, API keys first
    for (pii_type, regex) in &self.patterns {
        for mat in regex.find_iter(text) {
            let overlaps = entities.iter().any(|e| start < e.end && end > e.start);
            if !overlaps {
                push_if_new(&mut entities, …, 0.90);
            }
        }
    }

    entities.sort_by_key(|e| e.start);
    entities
}
```

## Tests

```bash
# Run all tests (206 unit + 25 e2e)
docker run --rm -v /opt/projet/mirageia:/workspace -w /workspace rust:latest cargo test

# Key PII detection tests
cargo test test_detect_iban
cargo test test_iban_not_detected_as_phone
cargo test test_detect_credit_card
cargo test test_detect_anthropic_key
cargo test test_detect_secret_high_entropy
```

## References

- [Presidio (Microsoft, MIT)](https://github.com/microsoft/presidio) — IBAN and credit card patterns
- [gitleaks (MIT)](https://github.com/gitleaks/gitleaks) — API key patterns
- [ONNX Runtime](https://onnxruntime.ai/) — cross-platform inference runtime
- [ort (Rust crate)](https://github.com/pykeio/ort) — Rust bindings for ONNX Runtime
- [AnonymizerSLM](https://huggingface.co/blog/pratyushrt/anonymizerslm) — specialized PII models
- [CloakPipe](https://github.com/rohansx/cloakpipe) — Rust proxy with ONNX NER
