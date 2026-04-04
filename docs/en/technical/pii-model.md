# Embedded PII Model

## Approach: Local LLM via ONNX Runtime

Unlike solutions based on regex or an external LLM server (Ollama), MirageIA embeds the model directly into the binary via **ONNX Runtime**.

Reference: the [Murmure](https://github.com/Kieirra/murmure) project uses the same approach to embed Whisper (speech-to-text) directly into a Tauri/Rust application.

## Candidate models

### Option 1: DistilBERT-PII (recommended for v1)
- **Size**: ~260 MB (INT8 quantized)
- **Capabilities**: 33 PII entity types
- **Latency**: 5-15ms per inference
- **Advantage**: lightweight, fast, specialized
- **Disadvantage**: less accurate on subtle context

### Option 2: AnonymizerSLM Qwen3 0.6B
- **Size**: ~400 MB (Q4 quantized)
- **Capabilities**: advanced contextual detection (understands "Thomas Edison" is not PII)
- **Latency**: 50-200ms
- **Advantage**: superior contextual intelligence
- **Disadvantage**: heavier, requires more RAM

### Option 3: Qwen3 1.7B (target for v2)
- **Size**: ~1.2 GB (Q4 quantized)
- **Capabilities**: better accuracy, better multilingual understanding
- **Latency**: 100-500ms
- **Advantage**: score 9.55/10 (close to GPT-4.1)
- **Disadvantage**: significant memory footprint

## ONNX Runtime integration

```
[Raw text]
     |
     v
[Tokenizer (embedded)]  <- model vocabulary
     |
     v
[ONNX Runtime]          <- .onnx model embedded or downloaded on first launch
     |
     v
[Post-processing]        <- entity extraction, positions, types, confidence scores
     |
     v
[PII entity list]
```

### Rust dependencies
- `ort` (Rust crate for ONNX Runtime) -- native binding, no Python FFI
- `tokenizers` (HuggingFace crate) -- fast tokenization in pure Rust

### Model distribution
- **Option A**: model embedded in the binary (binary size ~300+ MB but zero download)
- **Option B**: download on first launch from a CDN/GitHub Release (lightweight binary, ~20 MB)
- **Recommendation**: Option B with local cache in `~/.mirageia/models/`

## Target benchmarks

| Metric | Objective |
|--------|-----------|
| Precision (true positives) | > 90% |
| Recall (PII not missed) | > 95% (a false positive is better than a leak) |
| Latency per request | < 100ms |
| Memory | < 800 MB |
| Binary size (without model) | < 30 MB |

## References

- [ONNX Runtime](https://onnxruntime.ai/) -- Cross-platform inference runtime
- [ort (Rust crate)](https://github.com/pykeio/ort) -- Rust bindings for ONNX Runtime
- [Murmure](https://github.com/Kieirra/murmure) -- Example Tauri app with embedded ONNX model
- [AnonymizerSLM](https://huggingface.co/blog/pratyushrt/anonymizerslm) -- Specialized PII detection models
- [CloakPipe](https://github.com/rohansx/cloakpipe) -- Rust proxy with ONNX NER (DistilBERT-PII)
