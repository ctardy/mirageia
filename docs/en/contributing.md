# Contribution and Update Guide -- MirageIA

## Development Workflow

### Work Cycle

```
1. Create a branch       git checkout -b feat/my-feature
2. Code + test           cargo test (for each module)
3. Verify                cargo test (full suite)
4. Commit                git commit (commit messages in French)
5. Pull request          Review + merge into main
```

### Conventions

- **Language**: everything in French (code, comments, commits, docs) except Rust identifiers
- **Commits**: `type: description` -- e.g., `feat: ajout detection numeros SIRET`, `fix: buffer SSE overflow`, `docs: guide installation`
- **Tests**: each module includes a `#[cfg(test)] mod tests` block -- no code without tests
- **No backward compatibility**: the project is under construction, no `@Deprecated`

### Essential Commands

```bash
# Build
cargo build
cargo build --release

# Tests
cargo test                          # All (133+)
cargo test --lib                    # Unit tests only
cargo test --test e2e_proxy         # E2E only
cargo test -- detection::regex      # A specific module
cargo test -- test_detect_email     # A specific test

# Quick check (no linker)
cargo check

# Run the proxy
cargo run

# Run with debug logs
MIRAGEIA_LOG_LEVEL=debug cargo run
```

---

## How to Add a New PII Type

Example: adding SIRET number detection.

### Step 1 -- Add the type in `detection/types.rs`

```rust
pub enum PiiType {
    // ... existing variants ...
    Siret,  // <- new
}
```

Update `Display`, `label_to_pii_type()` and `default_threshold()`.

### Step 2 -- Add the regex in `detection/regex_detector.rs`

```rust
// In RegexDetector::new()
patterns.push((
    PiiType::Siret,
    Regex::new(r"\b\d{3}\s?\d{3}\s?\d{3}\s?\d{5}\b").unwrap(),
));
```

### Step 3 -- Add the generator in `pseudonymization/generator.rs`

```rust
// In PseudonymGenerator::generate()
PiiType::Siret => self.gen_siret(&mut rng, original),

// New method
fn gen_siret(&self, rng: &mut impl Rng, original: &str) -> String {
    // Preserve the format, replace digits
    original.chars().map(|c| {
        if c.is_ascii_digit() { char::from_digit(rng.gen_range(0..10), 10).unwrap() }
        else { c }
    }).collect()
}
```

### Step 4 -- Add the tests

In `regex_detector.rs`:
```rust
#[test]
fn test_detect_siret() {
    let entities = detector().detect("SIRET: 123 456 789 00012");
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].entity_type, PiiType::Siret);
}
```

In `generator.rs`:
```rust
#[test]
fn test_gen_siret_preserves_format() {
    let gen = generator();
    let siret = gen.generate(&PiiType::Siret, "123 456 789 00012");
    assert_eq!(siret.len(), "123 456 789 00012".len());
}
```

### Step 5 -- Verify

```bash
cargo test
# All tests must pass, including the new ones
```

---

## How to Modify the Pseudonymization Pipeline

### Data Flow

```
server.rs::proxy_handler()
    -> pseudonymize_request()
        -> extractor::extract_text_fields()     # JSON -> text fields
        -> regex_detector::detect_with_whitelist()  # text -> PII entities
        -> replacer::pseudonymize_text()         # entities -> pseudonymized text
            -> generator::generate()             # PII type -> pseudonym
            -> mapping::table::insert()          # encrypted storage
        -> extractor::rebuild_body()             # fields -> reconstructed JSON
    -> client::forward()                         # send upstream
    -> build_depseudonymized_response()
        -> depseudonymizer::depseudonymize_text()  # pseudonyms -> originals
        or streaming::buffer::push()               # SSE with buffer
```

### Extension Points

| To... | Modify |
|---|---|
| Add a PII type | `types.rs` + `regex_detector.rs` + `generator.rs` |
| Change the pseudonymization strategy | `generator.rs` |
| Support a new LLM provider | `router.rs` + `extractor.rs` + `sse_parser.rs` |
| Modify the encryption | `mapping/crypto.rs` |
| Add a JSON field to analyze | `extractor.rs` |

---

## How to Add a New LLM Provider

Example: adding Mistral (`/v1/chat/completions` with a slightly different format).

### Step 1 -- `proxy/router.rs`

```rust
pub enum Provider {
    Anthropic,
    OpenAI,
    Mistral,  // <- new
}

pub fn resolve_provider(path: &str) -> Option<Provider> {
    if path.starts_with("/v1/messages") {
        Some(Provider::Anthropic)
    } else if path.starts_with("/v1/chat/completions") {
        Some(Provider::OpenAI)  // or Mistral depending on a header?
    }
    // ...
}
```

### Step 2 -- `proxy/extractor.rs`

Add `extract_mistral_fields()` if the JSON format differs.

### Step 3 -- `streaming/sse_parser.rs`

Add SSE parsing if the streaming format differs.

### Step 4 -- `config/settings.rs`

Add `mistral_base_url` in `AppConfig`.

---

## Release Process

### Pre-release Checks

```bash
# 1. All tests pass
cargo test

# 2. No warnings
cargo check 2>&1 | grep warning

# 3. Release build works
cargo build --release

# 4. The binary works
./target/release/mirageia --help
```

### Versioning

The project follows [SemVer](https://semver.org/):
- `0.x.y` -- development phase (breaking changes possible)
- `MAJOR.MINOR.PATCH` starting from 1.0

Version in `Cargo.toml`:
```toml
[package]
version = "0.1.0"
```

### Update Process

1. **Update the code** on a dedicated branch
2. **Run the tests**: `cargo test` -- all must pass
3. **Update the version** in `Cargo.toml`
4. **Update the documentation** if behavior changes:
   - `README.md` -- if new user-facing features
   - `docs/officiel/installation.md` -- if prerequisites change
   - `docs/tickets/` -- mark completed tickets
5. **Commit and merge** into `main`
6. **Tag**: `git tag v0.2.0`

### Updating Dependencies

```bash
# See available updates
cargo outdated    # (requires cargo-outdated)

# Update Cargo.lock
cargo update

# Re-test after update
cargo test
```

---

## Test Structure

### Organization

```
src/
├── config/settings.rs          # inline tests (#[cfg(test)])
├── detection/
│   ├── regex_detector.rs       # inline tests
│   ├── types.rs                # inline tests
│   ├── postprocess.rs          # inline tests
│   ├── tokenizer.rs            # inline tests
│   ├── model.rs                # inline tests
│   └── mod.rs                  # inline tests (label_map)
├── mapping/
│   ├── crypto.rs               # inline tests
│   └── table.rs                # inline tests
├── pseudonymization/
│   ├── dictionaries.rs         # inline tests
│   ├── generator.rs            # inline tests
│   ├── replacer.rs             # inline tests
│   └── depseudonymizer.rs      # inline tests
├── proxy/
│   ├── router.rs               # inline tests
│   └── extractor.rs            # inline tests
├── streaming/
│   ├── sse_parser.rs           # inline tests
│   └── buffer.rs               # inline tests
└── tests/
    └── e2e_proxy.rs            # integration tests (mock upstream)
```

### Test Naming Convention

```rust
#[test]
fn test_<action>_<case>() {
    // test_detect_email          -> nominal case
    // test_detect_no_pii         -> empty case
    // test_decrypt_wrong_key     -> error case
    // test_whitelist_excludes    -> case with config
    // test_concurrent_access     -> concurrent case
}
```

### Adding an E2E Test

E2E tests in `tests/e2e_proxy.rs` launch a mock upstream and the proxy:

```rust
#[tokio::test]
async fn test_my_scenario() {
    let (upstream_addr, captured) = start_mock_upstream().await;
    let proxy_addr = start_proxy(upstream_addr).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/v1/messages", proxy_addr))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .json(&serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "My text with PII"}]
        }))
        .send()
        .await
        .unwrap();

    // Verify the request sent to the mock
    let captured = captured.lock().await;
    let sent = captured.as_ref().unwrap()["messages"][0]["content"].as_str().unwrap();
    assert!(!sent.contains("original PII"));

    // Verify the received response
    assert_eq!(resp.status(), 200);
}
```
