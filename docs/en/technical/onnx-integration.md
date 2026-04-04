# ONNX Integration — Implementation Specification

> **Status**: To be implemented (next phase after v0.4.x)
> **Prerequisites**: regex + algorithmic validation stable (v0.4.3 ✅)
> **Feature flag**: `--features onnx` (already declared in `Cargo.toml`)

---

## Objective

Complement regex detection (fixed-format PII) with **contextual detection** via an embedded NER model. Primary target: person names, organizations, addresses — entities without predictable patterns.

Regex detection keeps **priority** over fixed-format PII (IBAN, API keys, credit cards). ONNX fills in what regex cannot see.

---

## Recommended Model

**[`iiiorg/piiranha-v1-detect-personal-information`](https://huggingface.co/iiiorg/piiranha-v1-detect-personal-information)**

| Criterion | Value |
|-----------|-------|
| Size | ~280 MB (INT8 quantized) |
| Specialization | PII direct (not generic NER) |
| Languages | Multilingual including FR |
| Format | ONNX export available on HuggingFace |
| CPU latency | ~15ms / 500 tokens |

Documented alternatives:

| Model | Size | Strengths | Limitation |
|-------|------|-----------|------------|
| `dslim/bert-base-NER` | ~170 MB | Lightweight, fast | Generic labels (PER/ORG/LOC), not PII |
| `lakshyakh93/deberta_finetuned_pii` | ~350 MB | 12 PII types | Mainly EN |
| `Qwen3 0.6B` | ~400 MB (Q4) | Advanced contextual | Heavier, 50–200ms latency |

---

## Target Architecture

### Detection pipeline

```
Raw text
   ↓
HuggingFace Tokenizer (WordPiece/BPE)
   → token_ids + attention_mask + character offsets
   ↓
ONNX Runtime (ort crate)
   → logits: [n_tokens × n_labels]
   ↓
Post-processing
   → argmax per token → BIO label (B-PER, I-PER, B-ORG, O…)
   → BIO merging: reconstruct multi-token entities
   → map token offsets → character positions (via encoding.get_offsets())
   ↓
Vec<PiiEntity> — merged with regex results (no overlap)
```

### Merging regex + ONNX

```rust
pub fn detect_all(text: &str) -> Vec<PiiEntity> {
    // Regex always active (fixed-format PII, higher priority)
    let mut entities = regex_detector.detect(text);

    #[cfg(feature = "onnx")]
    {
        let onnx_entities = onnx_detector.detect(text);
        for e in onnx_entities {
            // Do not overwrite what regex already detected
            let overlaps = entities.iter().any(|r| r.start < e.end && r.end > e.start);
            if !overlaps {
                entities.push(e);
            }
        }
    }

    entities.sort_by_key(|e| e.start);
    entities
}
```

**Graceful degradation**: if the `onnx` feature is not compiled or the model is missing, MirageIA starts in regex-only mode — no crash.

---

## Rust Files to Create

```
src/detection/
├── mod.rs                  — expose OnnxDetector if feature onnx
├── onnx_detector.rs        — (NEW) inference + BIO merge
└── model_manager.rs        — (NEW) download + cache + hash verification
```

### `onnx_detector.rs`

```rust
#[cfg(feature = "onnx")]
pub struct OnnxDetector {
    session: ort::Session,
    tokenizer: tokenizers::Tokenizer,
}

#[cfg(feature = "onnx")]
impl OnnxDetector {
    pub fn load(model_dir: &Path) -> Result<Self> {
        let session = ort::Session::builder()?
            .with_optimization_level(ort::GraphOptimizationLevel::Level3)?
            .commit_from_file(model_dir.join("model.onnx"))?;

        let tokenizer = tokenizers::Tokenizer::from_file(
            model_dir.join("tokenizer.json")
        )?;

        Ok(Self { session, tokenizer })
    }

    pub fn detect(&self, text: &str) -> Vec<PiiEntity> {
        // 1. Tokenize with character offsets
        let encoding = self.tokenizer.encode(text, false)?;
        let offsets = encoding.get_offsets(); // [(char_start, char_end), …]

        // 2. Inference
        let logits = self.session.run(inputs![
            encoding.get_ids(),
            encoding.get_attention_mask()
        ])?;

        // 3. Argmax + BIO merging + offset → PiiEntity conversion
        bio_merge(logits, offsets, text)
    }
}
```

### `model_manager.rs`

```rust
pub struct ModelMeta {
    pub model: String,
    pub version: String,
    pub sha256: String,
    pub downloaded_at: DateTime<Utc>,
    pub source: String,
}

pub fn ensure_model(config: &Config) -> Result<PathBuf> {
    let model_dir = config.model_cache_dir
        .join(&config.model_name);
    let model_path = model_dir.join("model.onnx");
    let meta_path = model_dir.join("model.json");

    if model_path.exists() && meta_valid(&meta_path, &model_path) {
        return Ok(model_dir);
    }

    // Download from HuggingFace
    println!("  → Downloading model {} (~280 MB)…", config.model_name);
    download_hf_model(&config.model_name, &model_dir)?;
    write_meta(&meta_path, &config.model_name)?;

    Ok(model_dir)
}

fn meta_valid(meta_path: &Path, model_path: &Path) -> bool {
    // Check that the SHA-256 hash of the .onnx matches model.json
    // + optional weekly check against the HuggingFace API
    …
}
```

---

## Cache and Model Management

### On-disk structure

```
~/.mirageia/
└── models/
    ├── piiranha-v1/
    │   ├── model.onnx          (~280 MB)
    │   ├── tokenizer.json      (vocab + BPE rules)
    │   └── model.json          (SHA-256 hash + version + source + date)
    └── bert-base-NER/          (alternative model, can coexist)
        ├── model.onnx
        ├── tokenizer.json
        └── model.json
```

### `model.json` format

```json
{
  "model": "iiiorg/piiranha-v1-detect-personal-information",
  "version": "1.0",
  "sha256": "a3f9c2…",
  "downloaded_at": "2026-04-04T22:00:00Z",
  "source": "https://huggingface.co/iiiorg/piiranha-v1-detect-personal-information/resolve/main/model.onnx"
}
```

### Configuration (`~/.mirageia/config.toml`)

```toml
[detection]
model = "iiiorg/piiranha-v1-detect-personal-information"  # default
model_cache_dir = "~/.mirageia/models"
check_updates = "weekly"   # never | startup | daily | weekly
confidence_threshold = 0.85
```

---

## Model Management CLI

```bash
mirageia model list                        # list cached models + active one
mirageia model download <hf-repo>          # download without activating
mirageia model use <name>                  # switch active model
mirageia model update                      # check and apply updates
mirageia model delete <name>               # remove from cache
mirageia model verify                      # verify SHA-256 hash of active model
```

---

## Startup Behavior

| Situation | Behavior |
|-----------|----------|
| First launch | Download default model, show progress |
| Subsequent launches | Load from cache, instant startup |
| Corrupted model (invalid hash) | Re-download automatically |
| Update available | Notify, **do not update without confirmation** |
| Model change | `mirageia model use <name>` or edit `config.toml` |
| No network + no cache | Start in **regex-only mode** (no crash) |
| No network + cache present | Use existing cache normally |

**Principle**: model updates are **never silent** — a model change can alter detection behavior in production.

---

## Implementation Notes

### Token → character mapping
The trickiest part. A BPE/WordPiece tokenizer fragments words:
- `"Dupont"` → `["Du", "##pont"]`
- `"jean.dupont@acme.fr"` → several tokens

The `tokenizers` crate returns `encoding.get_offsets()`: an array of `(char_start, char_end)` per token. Use these offsets to reconstruct correct `PiiEntity { start, end }` positions in the original text.

### BIO merging
NER labels follow the BIO scheme:
- `B-PER` = beginning of a person entity
- `I-PER` = continuation
- `O` = outside entity

```
Token:  "Jean"  "Du"   "##pont"  "works"
Label:  B-PER   I-PER  I-PER     O
→ entity: "Jean Dupont" (merged positions)
```

### Confidence threshold
Apply a softmax score threshold before validating an entity. Recommended value: 0.85. User-configurable to adjust the precision/recall tradeoff.

### Contextual false positives
Expected examples:
- `"Thomas Edison invented the light bulb"` → detected as PER despite historical context
- `"The `username` field contains…"` → `username` potentially detected

Mitigation: user whitelist in `config.toml` + high confidence threshold.

---

## Suggested Implementation Order

1. `model_manager.rs` — HuggingFace download + cache + SHA-256 hash
2. `onnx_detector.rs` — tokenize + inference + basic BIO merge
3. Hook into `detect_all()` behind `#[cfg(feature = "onnx")]`
4. Tests: compare recall regex-only vs regex+ONNX on FR/EN PII corpus
5. `mirageia model` CLI (list, use, update, delete)
6. `~/.mirageia/config.toml` support
7. Release `v0.5.0` with `--features onnx` enabled by default in CI
