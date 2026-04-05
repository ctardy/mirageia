# Text Extraction — Images, PDFs, DOCX

> **Status**: To be implemented
> **Priority**: Text PDF first (lopdf, zero system dependency)
> **Target**: v0.6.0

---

## Principle

Images and documents attached to a Claude request are converted to plain text, then processed by the existing pseudonymization pipeline. The original block (image/document) is **replaced** in the request by a pseudonymized `text` block.

```
Incoming request
   ↓
For each "image" or "document" block in content[]:
   → Extract plain text
   → Pseudonymize (existing pipeline)
   → Replace block with { "type": "text", "text": "..." pseudonymized }
   ↓
Outgoing request: text blocks only
```

Claude receives pseudonymized text. De-pseudonymization in the response works normally — no change to the existing pipeline.

---

## API Request Transformation

**Original request:**
```json
{
  "content": [
    { "type": "text", "text": "here is my contract" },
    { "type": "document", "source": { "media_type": "application/pdf", "data": "JVBERi0..." } }
  ]
}
```

**After extraction + pseudonymization:**
```json
{
  "content": [
    { "type": "text", "text": "here is my contract" },
    { "type": "text", "text": "[document extract]\n\nContract between [PERS_1] and [PERS_2].\nIBAN: [IBAN_1]\nSigned on [DATE_1]..." }
  ]
}
```

---

## Extraction by Content Type

```rust
pub fn extract_text(block: &ContentBlock) -> Option<String> {
    match block {
        ContentBlock::Document(data, "application/pdf")  => pdf::extract(data),
        ContentBlock::Document(data, mime) if mime.contains("wordprocessingml") => docx::extract(data),
        ContentBlock::Image(data, _media_type)           => ocr::extract(data),
        _ => None,  // unsupported type → block passed through with warning
    }
}
```

| Type | Rust Lib | Notes |
|------|----------|-------|
| Text PDF (generated) | `lopdf` | Pure Rust, no system dependency |
| Scanned PDF | `ocrs` (ONNX) | Same model as image detection |
| DOCX / Word | `zip` + `quick-xml` | DOCX = ZIP + XML, no dedicated lib needed |
| Image / screenshot | `ocrs` (ONNX) | OCR on PNG, JPEG, WebP |

---

## Rust Modules to Create

```
src/extraction/
├── mod.rs          — extract_text(block) → Option<String>
├── pdf.rs          — lopdf: body + metadata (author, company, path)
├── docx.rs         — zip + quick-xml: document.xml + comments.xml + core.xml
└── ocr.rs          — ocrs: images and scanned PDFs (requires "onnx" feature)
```

### `pdf.rs` — extracts text and metadata

```rust
pub fn extract(data: &[u8]) -> Option<String> {
    let doc = lopdf::Document::load_mem(data).ok()?;

    let mut parts = Vec::new();

    // Metadata (often rich in PII: author, company, file path)
    if let Some(meta) = extract_metadata(&doc) {
        parts.push(format!("[metadata]\n{}", meta));
    }

    // Text page by page
    for page_id in doc.page_iter() {
        if let Ok(text) = doc.extract_text(&[page_id]) {
            parts.push(text);
        }
    }

    Some(parts.join("\n\n"))
}
```

### `docx.rs` — extracts content + comments + metadata

```rust
pub fn extract(data: &[u8]) -> Option<String> {
    let cursor = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor).ok()?;
    let mut parts = Vec::new();

    // Main content
    if let Ok(text) = read_xml(&mut archive, "word/document.xml") {
        parts.push(text);
    }
    // Comments (often forgotten, may contain sensitive notes)
    if let Ok(text) = read_xml(&mut archive, "word/comments.xml") {
        parts.push(format!("[comments]\n{}", text));
    }
    // Metadata: author, company, creation date
    if let Ok(text) = read_xml(&mut archive, "docProps/core.xml") {
        parts.push(format!("[metadata]\n{}", text));
    }

    Some(parts.join("\n\n"))
}
```

---

## What We Lose (Acceptable)

- **Layout**: tables, columns, headers/footers — Claude receives linear text
- **Embedded images** in PDF/DOCX: charts, logos, photos — ignored (not relevant for pseudonymization)
- **Visual structure**: indentation, heading styles

For the target use cases (contracts, configs, logs, terminal screenshots, emails) plain text is more than sufficient for Claude to understand the content.

---

## Behavior When Extraction Fails

```
Unrecognized type or failed extraction
   ↓
Log: "⚠ [type] block not extracted — sent as-is"
   ↓
Block passed through unchanged (current behavior)
```

Never crashes, never silently blocks.

---

## Dependencies to Add in Cargo.toml

```toml
# PDF extraction (text)
lopdf = "0.34"

# DOCX extraction
zip = { version = "2", default-features = false, features = ["deflate"] }  # already present
quick-xml = "0.36"

# OCR for images + scanned PDFs (optional feature, same model as ONNX PII)
# ocrs = "0.8"   ← enable with "onnx" feature
```

`lopdf` and `quick-xml` are lightweight, pure Rust, no model download required.
`ocrs` shares the ONNX runtime with contextual PII detection — single download.

---

## Implementation Order

1. **`pdf.rs`** with `lopdf` — covers most cases (computer-generated PDFs)
2. **`docx.rs`** with `zip` + `quick-xml` — Word is common in enterprise
3. **`ocr.rs`** with `ocrs` — images and scanned PDFs (depends on `onnx` feature)
4. Hook into the request interceptor (`src/proxy/request_handler.rs`)
5. Tests on corpus: PDF contract, Word file with comments, terminal screenshot
6. Release `v0.6.0`

---

## Implementation Status (v0.5.0)

**Implemented ✅**

| File | Content |
|------|---------|
| `src/extraction/mod.rs` | `preprocess_media_blocks()` — main hook, fail-open |
| `src/extraction/pdf.rs` | `extract()` via lopdf: text page by page + metadata (author, title, company) |
| `src/extraction/docx.rs` | `extract()` via zip + quick-xml: document.xml + comments.xml + core.xml |
| `src/detection/model_manager.rs` | `ensure_model()`, `list_models()`, `delete_model()`, `verify_model()`, `get/set_active_model()` |

**`mirageia model` CLI subcommand:**
```bash
mirageia model list               # list cached models
mirageia model download <name>    # download from HuggingFace
mirageia model use <name>         # set active model
mirageia model delete <name>      # remove from cache
mirageia model verify             # verify SHA-256 of active model
```

**Dependencies added to Cargo.toml:**
```toml
lopdf = "0.34"
quick-xml = "0.36"
base64 = "0.22"
reqwest = { ..., features = ["...", "blocking"] }
```

**Tests: 219 total (203 unit + 16 integration)**
