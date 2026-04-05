# ONNX Integration — Implementation Reference

> **Status**: Implemented (v0.5.5+)
> **Feature flag**: `--features onnx` (enabled by default in CI release builds)
> **Model**: `iiiorg/piiranha-v1-detect-personal-information` (pre-exported ONNX, hosted on GitHub Releases)

---

## Overview

MirageIA embeds a contextual NER model via ONNX Runtime to detect person names, organizations, and addresses — entities that regex cannot reliably detect without context.

The two layers are complementary:
- **Regex layer** handles fixed-format PII (IBAN, API keys, emails, IPs, credit cards) with higher priority
- **ONNX layer** adds contextual entities that did not overlap with regex results

---

## Active Model

**[`iiiorg/piiranha-v1-detect-personal-information`](https://huggingface.co/iiiorg/piiranha-v1-detect-personal-information)**

| Criterion | Value |
|-----------|-------|
| Size on disk | ~337 MB (ONNX INT8 quantized) |
| Memory (RSS at runtime) | ~946 MB |
| Memory peak (during loading) | ~2.1 GB |
| Specialization | PII detection (not generic NER) |
| Languages | Multilingual including FR |
| CPU latency | ~15–30ms / request |

> **Note**: HuggingFace only distributes this model in SafeTensors format. MirageIA hosts a pre-exported ONNX version at `github.com/ctardy/mirageia/releases/download/models-v1/`.

---

## Detection Pipeline (Implemented)

```
Raw text
   ↓
[Regex layer]  validated_patterns (IBAN/MOD-97, CB/Luhn) → confidence 0.95
               + capture_validated_patterns (password + entropy)
               + patterns (API keys first, then email/IP/phone/NSS) → confidence 0.90
   ↓
[ONNX layer]  tokenizers::Tokenizer (WordPiece/BPE, HuggingFace crate)
               → token_ids + attention_mask + character offsets
               ↓
              ort::Session::run() → logits [n_tokens × n_labels]
               ↓
              argmax per token → BIO labels (B-PER, I-PER, B-ORG, O…)
               → BIO merging → Vec<PiiEntity> with character positions
   ↓
[Merge]       ONNX entities added only if no overlap with regex results
               Unknown-type entities skipped
   ↓
Vec<PiiEntity>  →  pseudonymization pipeline
```

### Merge implementation (`server.rs`)

```rust
let entities = state.detector.detect_with_whitelist(&field.text, &state.config.whitelist);
#[cfg(feature = "onnx")]
let entities = {
    let mut combined = entities;
    if let Some(onnx) = &state.onnx_detector {
        if let Ok(onnx_entities) = onnx.detect(&field.text) {
            for onnx_entity in onnx_entities {
                if onnx_entity.entity_type == PiiType::Unknown { continue; }
                let overlaps = combined.iter().any(|e|
                    onnx_entity.start < e.end && onnx_entity.end > e.start
                );
                if !overlaps { combined.push(onnx_entity); }
            }
        }
    }
    combined
};
```

**Graceful degradation**: if the model is missing or fails to load, MirageIA starts in regex-only mode — no crash.

---

## Rust Source Structure

```
src/detection/
├── mod.rs               — PiiDetector struct (model + tokenizer + label_map)
│                           from_model_name(), detect(), load_label_map()
├── types.rs             — PiiEntity, PiiType
├── regex_detector.rs    — RegexDetector (validated_patterns + patterns)
├── validator.rs         — iban_valid(), luhn_valid(), shannon_entropy()
├── tokenizer.rs         — PiiTokenizer (HuggingFace tokenizers crate wrapper)
├── model.rs             — PiiModel (ort Session, infer())
└── model_manager.rs     — download/cache/verify, get_active_model(), set_active_model()
```

### Key types

```rust
pub struct PiiDetector {
    model: PiiModel,                      // ort::Session wrapped in Mutex<>
    tokenizer: PiiTokenizer,              // tokenizers::Tokenizer
    label_map: Vec<String>,               // e.g. ["O", "B-PER", "I-PER", …]
    thresholds: HashMap<PiiType, f32>,
    overlap_chars: usize,                 // text segmentation overlap (200 chars)
}

pub struct PiiModel {
    session: std::sync::Mutex<ort::session::Session>,
    // Mutex required: ort 2.0-rc.12 Session::run() takes &mut self
}
```

### ort 2.0.0-rc.12 API specifics

```rust
// Session creation
let session = ort::session::Session::builder()
    .map_err(|e| ...)?
    .commit_from_file(model_path)
    .map_err(|e| ...)?;

// Inference
let ids_tensor = ort::value::Tensor::<i64>::from_array(input_ids_ndarray)?;
let mask_tensor = ort::value::Tensor::<i64>::from_array(attention_mask_ndarray)?;
let outputs = session.run(ort::inputs![
    "input_ids" => ids_tensor,
    "attention_mask" => mask_tensor
])?;
let (shape, data) = outputs[0].try_extract_tensor::<f32>()?;
// shape: &[i64], data: &[f32] — index: data[token_idx * num_labels + label_idx]
```

Dependencies: `ort = { version = "2.0.0-rc.12", features = ["download-binaries", "ndarray"] }`, `ndarray = "0.17"` (must match ort's internal version).

---

## Model Management CLI

```bash
mirageia model list                        # list cached models, mark active
mirageia model download <hf-repo>          # download from GitHub Releases (then HF fallback)
mirageia model use <name>                  # set active model (writes ~/.mirageia/active_model)
mirageia model delete <name>               # remove from cache
mirageia model verify                      # SHA-256 integrity check
```

### On-disk structure

```
~/.mirageia/
├── active_model          — one line: model name (e.g. iiiorg/piiranha-v1-detect-personal-information)
└── models/
    └── iiiorg__piiranha-v1-detect-personal-information/
        ├── model.onnx        (~337 MB)
        ├── tokenizer.json    (~16 MB — vocab + BPE rules)
        ├── config.json       (id2label map)
        └── meta.json         (source URL, downloaded_at, version)
```

Directory naming: `/` → `__` (e.g. `iiiorg/piiranha-v1-detect-personal-information` → `iiiorg__piiranha-v1-detect-personal-information`).

### Download strategy

`ensure_model()` tries in order:
1. **GitHub Releases** — `https://github.com/ctardy/mirageia/releases/download/models-v1/{safe_name}.tar.gz` (pre-exported ONNX bundle, no Python/optimum required)
2. **HuggingFace** fallback — individual file download if GitHub asset is not available

---

## Startup Behavior

| Situation | Behavior |
|-----------|----------|
| `active_model` set, model files present | Load ONNX detector, log "détection contextuelle active" |
| `active_model` set, model files missing | Log warn, start in regex-only mode (fail-open) |
| No `active_model` file | Start in regex-only mode |
| Inference error | Log error, that request uses regex results only |

The active model name is exposed in:
- `GET /health` → `"onnx_model": "iiiorg/piiranha-v1-detect-personal-information"` (or `null`)
- `mirageia console` → `Detection  : regex + ONNX (iiiorg/piiranha-v1-detect-personal-information)`

---

## Activating the ONNX Model

### Server/Docker deployment

```bash
# 1. Download the model (inside the running container)
docker exec mirageia mirageia model download iiiorg/piiranha-v1-detect-personal-information

# 2. Set it as active
docker exec mirageia mirageia model use iiiorg/piiranha-v1-detect-personal-information

# 3. Rebuild the Docker image (entrypoint is COPY'd at build time)
cd /opt/docker/mirageia
docker compose build
docker compose up -d
```

> The model is persisted in the `./home/.mirageia` volume and survives container restarts.

### Local installation

```bash
mirageia model download iiiorg/piiranha-v1-detect-personal-information
mirageia model use iiiorg/piiranha-v1-detect-personal-information
mirageia  # restart the proxy
```

---

## Memory Requirements

| Mode | RSS | VmPeak (loading) |
|------|-----|------------------|
| Regex only | ~10 MB | ~10 MB |
| ONNX active | ~946 MB | ~2.1 GB |

For Docker deployments with ONNX enabled, set the memory limit to **at least 3 GB**:

```yaml
deploy:
  resources:
    limits:
      memory: 3G
```

---

## Implementation Notes

### Text segmentation

Long texts are split into overlapping segments (200-char overlap) to avoid truncating entities at segment boundaries. Results from all segments are merged with deduplication.

### Token → character mapping

The `tokenizers` crate returns `encoding.get_offsets()`: `(char_start, char_end)` per token. These are used to reconstruct correct `PiiEntity { start, end }` positions in the original text.

### BIO merging

Labels follow the BIO scheme. Adjacent `I-*` tokens with the same type as the preceding `B-*` are merged into a single entity spanning their combined character range.

```
Token:  "Jean"  "Du"   "##pont"  "works"
Label:  B-PER   I-PER  I-PER     O
→ entity: "Jean Dupont" (merged, char positions from offsets)
```

### False positives

Expected cases: historical figures ("Thomas Edison"), generic variable names. Mitigated by the whitelist in `config.toml` and entity overlap with regex results.

---

## Adding a New Model

Any HuggingFace token-classification model compatible with ONNX Runtime can be used:

```bash
# Export to ONNX (requires optimum)
pip install optimum[onnxruntime]
optimum-cli export onnx \
  --model <hf-repo> \
  --task token-classification \
  ~/.mirageia/models/<safe_name>/

# Activate
mirageia model use <hf-repo>
```

To distribute pre-exported ONNX bundles, create a GitHub release with a `{safe_name}.tar.gz` asset containing `model.onnx`, `tokenizer.json`, `config.json` at the archive root.
