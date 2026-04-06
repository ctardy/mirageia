# MirageIA Multi-Dimensional Project Audit

**Date**: 2026-04-06
**Version analyzed**: v0.5.14
**Method**: 5 parallel AI agents, each specialized on one dimension, cross-validated

---

## Executive Summary

| Dimension | Score | Verdict |
|-----------|-------|---------|
| **Business Positioning** | 6.8/10 | Promising, real market, unique positioning |
| **Architecture & Implementation** | 8/10 | Production-ready, ~7800 LOC well-structured |
| **Security** | B+ (single-user) / D (multi-user) | Solid crypto, but critical auth gaps |
| **Code Quality** | 7.5/10 | 233 tests, 0 warnings, but 134 unwrap() |
| **Technical Debt** | 60+ items identified | Functional but accumulated debt |

---

## 1. Business Positioning & Market Viability

### Score: 6.8/10 - Promising but Pre-Launch

### Competitive Landscape

MirageIA operates in a fragmented market with competitors across three tiers:

| Competitor | Type | Strength | Weakness vs MirageIA |
|-----------|------|----------|----------------------|
| **Presidio** (7.5k stars) | Python library | PII detection standard | Not a proxy, not reversible |
| **CloakPipe** | Rust proxy | Similar architecture | No contextual ONNX detection |
| **PasteGuard** (570 stars) | TypeScript proxy | Browser extension | Depends on Node.js, not a single binary |
| **LLM Guard** (2.8k stars) | Python pipeline | 15+ scanners | Not transparent, requires Docker |
| **Lakera Guard** | Commercial SaaS | <50ms latency, mature | Cloud = data leaves infrastructure |
| **LiteLLM** (20k stars) | Multi-provider proxy | Presidio integration | No native detection |

### Unique Value Proposition

No competitor offers all of these simultaneously:
- Embedded ONNX model (contextual PII detection)
- Transparent HTTP proxy
- Single binary, zero dependencies
- Reversible pseudonymization with AES-256-GCM mapping
- Zero configuration

### Target Markets

1. **Enterprises (500+ employees)** - Viability: Excellent - GDPR/NIS2 pressure, sovereignty needs
2. **Regulated industries** (finance, healthcare, government) - Viability: Very Good
3. **Individual developers** - Viability: Good - Viral adoption possible but low monetization

### Recommended Commercial Model

- **Phase 1** (now - 6 months): Open-source MIT, community building
- **Phase 2** (6-12 months): Open-core + SaaS console beta
- **Phase 3** (12+ months): Enterprise tier + support contracts
- **Potential ARR**: $500k-$5M by end of 2027

### Key Risks

- ONNX model accuracy (~85-92% vs 95%+ for cloud solutions)
- False positives that break LLM responses
- OpenAI/Anthropic could add native masking
- Market consolidation (LiteLLM could integrate PII detection)

### Distance to MVP

**MVP is considered complete (v0.5.14)**: functional proxy, 8 regex PII types, reversible pseudonymization, SSE streaming, dashboard, 233 tests. Missing mainly ONNX integration (blocked by MSVC toolchain) and Tauri desktop app.

---

## 2. Architecture & Implementation

### Score: 8/10 - Production-Ready

### Strengths

- **Exemplary modular organization**: `proxy/`, `detection/`, `pseudonymization/`, `mapping/`, `streaming/`, `extraction/` - clear separation of concerns
- **Sophisticated pseudonymization pipeline**:
  - PII detection (regex + optional ONNX)
  - Replacement with bidirectional AES-256-GCM mapping
  - De-pseudonymization with Aho-Corasick (O(n+m))
  - SSE streaming support with intelligent buffer
- **Notable innovations**:
  - **IP subnet coherence**: IPs in the same /24 share a pseudo-prefix
  - **Fragment restoration (SPB)**: Recovers data even when LLM decomposes a pseudonym
  - **Streaming buffer**: Never cuts a pseudonym in the middle of an SSE chunk
- **233 tests** (207 unit + 26 e2e), zero `unsafe` blocks

### Areas of Concern

- **ONNX bottleneck**: `session.lock().unwrap()` serializes all inferences - problem at high concurrency
- **No provider abstraction**: Adding a 3rd LLM (Llama, etc.) requires code changes, not config
- **`proxy_handler` is 600+ lines** - needs refactoring into smaller functions
- **No batching**: Each entity detection = a separate pass

### Estimated Latency

- Regex: <5ms | ONNX: 15-30ms | Pseudonymization: O(n) | Total: +5-15ms/request

### Dependency Analysis

| Crate | Version | Role | Risk |
|-------|---------|------|------|
| axum | 0.7 | HTTP framework | Low - actively maintained |
| tokio | 1 | Async runtime | Low - stable |
| reqwest | 0.12 | HTTP client | Low - actively maintained |
| aes-gcm | 0.10 | Encryption | Low - RustCrypto, well-maintained |
| ort | 2.0.0-rc.12 | ONNX inference | Medium - release candidate |
| aho-corasick | 1 | String matching | Low - stable |

---

## 3. Security Audit

### Grade: B+ (single-user) / D (multi-user)

### Critical Findings

| # | Severity | Issue | File |
|---|----------|-------|------|
| C1 | **CRITICAL** | No authentication on proxy | `server.rs` |
| C2 | **CRITICAL** | Cross-user mapping leakage in shared environments | `table.rs` |
| H1 | **HIGH** | Single key, never rotated | `crypto.rs` |
| H2 | **HIGH** | SSRF risk - no upstream URL validation | `router.rs` |
| H3 | **HIGH** | PII temporarily in plaintext memory, not zeroized | `server.rs` |
| M1 | **MEDIUM** | Temporary plaintext exposure during decryption | `table.rs` |
| M2 | **MEDIUM** | Malicious ONNX model risk (no SHA-256 verification) | `model_manager.rs` |
| M3 | **MEDIUM** | Potential ReDoS vulnerability in regex patterns | `regex_detector.rs` |

### Positive Security Findings

- **Zero `unsafe` blocks** in the entire codebase
- **AES-256-GCM correctly implemented**: random nonces via `OsRng`, never reused
- **Crypto key zeroization** via `zeroize` crate on `Drop`
- **No secrets in logs** - no PII data logged in plaintext
- **TLS enabled by default** to upstream (rustls-tls, not OpenSSL)
- **Binds to 127.0.0.1** only (not exposed to network)
- **Hash-based lookup** prevents plaintext original value storage

### Key Recommendation

> The product is safe for **single-user use on personal workstations**. It is **NOT safe** for multi-user or enterprise deployments without adding authentication and per-user isolation.

### Deployment Security Checklist

- [ ] Run proxy as unprivileged user
- [ ] Bind to 127.0.0.1 only (not 0.0.0.0)
- [ ] Set `MIRAGEIA_LOG_LEVEL=info` (NOT debug)
- [ ] Rotate proxy process daily (if possible) to clear memory/mappings
- [ ] Do NOT store API keys in config file - use environment variables
- [ ] Monitor for unusual proxy activity (spike in PII detections)

---

## 4. Code Quality

### Score: 7.5/10

### Summary Table

| Aspect | Rating | Detail |
|--------|--------|--------|
| **Code Style** | 4/5 | Consistent, minor language mixing |
| **Error Handling** | 3/5 | **134 unwrap() + 6 expect()** - RwLock poisoning risk |
| **Test Coverage** | 4/5 | 233 tests, gaps in streaming & fuzzing |
| **Documentation** | 4/5 | Good architecture, missing lifecycle docs |
| **Code Smells** | 4/5 | Minimal, mostly minor issues |
| **Rust Idioms** | 4/5 | Strong ownership usage, some cloning |
| **Build Quality** | 5/5 | Zero warnings, Clippy `-D warnings` in CI |

### Critical Issues

1. **134 `unwrap()` in production code** - Most dangerous:
   - `mapping/table.rs:51,75,89` - RwLock `.read().unwrap()` - panic if lock poisoned
   - `detection/model.rs:54` - `session.lock().unwrap()` - panic under concurrency
   - 30+ `Regex::new(...).unwrap()` in `regex_detector.rs` - technically safe but anti-idiomatic

2. **Mixed FR/EN error messages** - `"Erreur de chiffrement"` vs English code comments

3. **No configuration validation** - `confidence_threshold = 5.0` accepted without error

4. **Test gaps**:
   - Streaming with fragmented pseudonyms: barely tested
   - ONNX inference: only file existence checked, no actual inference
   - No fuzzing for a security product
   - No performance benchmarks

---

## 5. Technical Debt

### Inventory: 60+ items identified

### Top Priorities

| Category | Items | Effort | Risk |
|----------|-------|--------|--------|
| **Silent fail-open** | Proxy forwards without masked PII on error, no notification | Medium | **CRITICAL** |
| **ONNX Windows** | Feature compiled but unusable, silent degradation | High | High |
| **Refactoring server.rs** | 600+ line handler, strong coupling | High | Medium |
| **Mutex generator** | Serializes all pseudonymization | Medium | Medium |
| **Documentation vs reality** | 5 major discrepancies | Medium | Medium |

### Documentation vs Reality Gaps

| Promise | Reality |
|---------|---------|
| "Tauri single binary" (CLAUDE.md) | CLI Rust-only, no Tauri |
| "Embedded ONNX model" (README) | Optional feature, manual 337MB download |
| "Tauri webview dashboard" | Hardcoded HTML in an Axum route |
| "Transparent HTTP interception" | Requires explicit proxy configuration |
| "Contextual detection v2" | Code ready, blocked by MSVC toolchain |

### Missing Infrastructure

- No `cargo-audit` or `cargo-deny` in CI
- No MSVC testing in CI (only GNU on Windows)
- No code coverage (LLVM/tarpaulin)
- No SBOM or release signing
- No Prometheus metrics for monitoring
- No Kubernetes manifests/Helm charts

---

## 6. Prioritized Action Plan

### Immediate (before v1.0)

1. **SECURITY**: Convert silent fail-open to configurable fail-safe (or at minimum notify user)
2. **SECURITY**: Add bearer token authentication on proxy
3. **SECURITY**: Validate upstream URLs against whitelist (anti-SSRF)
4. **QUALITY**: Replace `RwLock.unwrap()` with proper error handling in `mapping/table.rs`
5. **DOCS**: Update README/CLAUDE.md to reflect reality (no Tauri, ONNX optional)

### Short-term (v1.1)

6. **ARCH**: Refactor `proxy_handler` into sub-functions
7. **PERF**: Replace `Mutex<PseudonymGenerator>` with stateless function or thread-local RNG
8. **TESTS**: Add fuzzing on streaming buffer and regex patterns
9. **TESTS**: Add performance benchmarks with `criterion`
10. **ONNX**: Runtime detection of ONNX availability with user notification

### Medium-term (v2.0)

11. **ARCH**: Trait interface for PII detectors (`pub trait PiiDetector`)
12. **ARCH**: Trait interface for LLM providers (`pub trait LlmProvider`)
13. **SECURITY**: Periodic crypto key rotation
14. **SECURITY**: SHA-256 verification of downloaded ONNX models
15. **BUSINESS**: Define commercial model (open-core recommended)

---

## 7. Final Verdict

**MirageIA is a technically solid project with a unique market positioning.** The combination of transparent proxy + embedded ONNX detection + reversible AES-256-GCM pseudonymization + single binary exists in no competitor.

**Strengths**: Clean architecture, correct crypto, 233 tests, innovative pseudonymization pipeline (IP subnet coherence, fragment restoration, streaming-aware)

**Weaknesses**: Security gaps for multi-user use, 134 risky unwrap(), documentation out of sync with reality, ONNX blocked on Windows

**The project is MVP-complete for single-user use.** To reach enterprise-readiness, plan 3-6 months of hardening (security, tests, documentation, ONNX Windows).
