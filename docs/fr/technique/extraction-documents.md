# Extraction de texte — Images, PDF, DOCX

> **Statut** : À implémenter
> **Priorité** : PDF texte en premier (lopdf, zéro dépendance système)
> **Cible** : v0.6.0

---

## Principe

Les images et documents joints à une requête Claude sont convertis en texte brut, puis traités par le pipeline de pseudonymisation existant. Le bloc original (image/document) est **remplacé** dans la requête par un bloc `text` pseudonymisé.

```
Requête entrante
   ↓
Pour chaque bloc "image" ou "document" dans content[] :
   → Extraire le texte brut
   → Pseudonymiser (pipeline existant)
   → Remplacer le bloc par { "type": "text", "text": "..." pseudonymisé }
   ↓
Requête sortante : uniquement des blocs texte
```

Claude reçoit du texte pseudonymisé. La dé-pseudonymisation dans la réponse fonctionne normalement — aucun changement au pipeline existant.

---

## Transformation de la requête API

**Requête originale :**
```json
{
  "content": [
    { "type": "text", "text": "voici mon contrat" },
    { "type": "document", "source": { "media_type": "application/pdf", "data": "JVBERi0..." } }
  ]
}
```

**Après extraction + pseudonymisation :**
```json
{
  "content": [
    { "type": "text", "text": "voici mon contrat" },
    { "type": "text", "text": "[extrait du document]\n\nContrat entre [PERS_1] et [PERS_2].\nIBAN : [IBAN_1]\nSigné le [DATE_1]..." }
  ]
}
```

---

## Extraction par type de contenu

```rust
pub fn extract_text(block: &ContentBlock) -> Option<String> {
    match block {
        ContentBlock::Document(data, "application/pdf")  => pdf::extract(data),
        ContentBlock::Document(data, mime) if mime.contains("wordprocessingml") => docx::extract(data),
        ContentBlock::Image(data, _media_type)           => ocr::extract(data),
        _ => None,  // type non supporté → bloc passé en clair avec avertissement
    }
}
```

| Type | Lib Rust | Notes |
|------|----------|-------|
| PDF texte (généré) | `lopdf` | Pure Rust, pas de dépendance système |
| PDF scanné | `ocrs` (ONNX) | Même modèle que détection image |
| DOCX / Word | `zip` + `quick-xml` | DOCX = ZIP + XML, pas de lib dédiée nécessaire |
| Image / screenshot | `ocrs` (ONNX) | OCR sur PNG, JPEG, WebP |

---

## Modules Rust à créer

```
src/extraction/
├── mod.rs          — extract_text(block) → Option<String>
├── pdf.rs          — lopdf : body + métadonnées (auteur, société, chemin)
├── docx.rs         — zip + quick-xml : document.xml + comments.xml + core.xml
└── ocr.rs          — ocrs : images et PDF scannés (feature "onnx" requis)
```

### `pdf.rs` — extrait le texte et les métadonnées

```rust
pub fn extract(data: &[u8]) -> Option<String> {
    let doc = lopdf::Document::load_mem(data).ok()?;

    let mut parts = Vec::new();

    // Métadonnées (souvent riches en PII : auteur, société, chemin)
    if let Some(meta) = extract_metadata(&doc) {
        parts.push(format!("[métadonnées]\n{}", meta));
    }

    // Texte page par page
    for page_id in doc.page_iter() {
        if let Ok(text) = doc.extract_text(&[page_id]) {
            parts.push(text);
        }
    }

    Some(parts.join("\n\n"))
}
```

### `docx.rs` — extrait contenu + commentaires + métadonnées

```rust
pub fn extract(data: &[u8]) -> Option<String> {
    let cursor = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor).ok()?;
    let mut parts = Vec::new();

    // Contenu principal
    if let Ok(text) = read_xml(&mut archive, "word/document.xml") {
        parts.push(text);
    }
    // Commentaires (souvent oubliés, peuvent contenir des notes sensibles)
    if let Ok(text) = read_xml(&mut archive, "word/comments.xml") {
        parts.push(format!("[commentaires]\n{}", text));
    }
    // Métadonnées : auteur, société, date de création
    if let Ok(text) = read_xml(&mut archive, "docProps/core.xml") {
        parts.push(format!("[métadonnées]\n{}", text));
    }

    Some(parts.join("\n\n"))
}
```

---

## Ce qu'on perd (acceptable)

- **Mise en page** : tableaux, colonnes, en-têtes/pieds de page — Claude reçoit du texte linéaire
- **Images embarquées** dans le PDF/DOCX : graphiques, logos, photos — ignorés (non pertinents pour la pseudonymisation)
- **Structure visuelle** : indentation, style des titres

Pour les cas d'usage cibles (contrats, configs, logs, screenshots de terminal, emails) le texte brut est largement suffisant pour que Claude comprenne le contenu.

---

## Comportement si extraction impossible

```
Type non reconnu ou extraction échouée
   ↓
Log : "⚠ bloc [type] non extrait — envoyé en clair"
   ↓
Bloc passé tel quel dans la requête (comportement actuel)
```

Jamais de crash, jamais de blocage silencieux.

---

## Dépendances à ajouter dans Cargo.toml

```toml
# Extraction PDF (texte)
lopdf = "0.34"

# Extraction DOCX
zip = { version = "2", default-features = false, features = ["deflate"] }  # déjà présent
quick-xml = "0.36"

# OCR images + PDF scannés (feature optionnelle, même modèle que ONNX PII)
# ocrs = "0.8"   ← à activer avec feature "onnx"
```

`lopdf` et `quick-xml` sont des dépendances légères, pure Rust, sans modèle à télécharger.
`ocrs` partage le runtime ONNX avec la détection PII contextuelle — un seul téléchargement.

---

## Ordre d'implémentation

1. **`pdf.rs`** avec `lopdf` — couvre la majorité des cas (PDFs générés par ordinateur)
2. **`docx.rs`** avec `zip` + `quick-xml` — Word est fréquent en entreprise
3. **`ocr.rs`** avec `ocrs` — images et PDFs scannés (dépend de la feature `onnx`)
4. Brancher dans l'intercepteur de requêtes (`src/proxy/request_handler.rs`)
5. Tests sur corpus : contrat PDF, fichier Word avec commentaires, screenshot terminal
6. Release `v0.6.0`

---

## État d'implémentation (v0.5.0)

**Implémenté ✅**

| Fichier | Contenu |
|---------|---------|
| `src/extraction/mod.rs` | `preprocess_media_blocks()` — hook principal, fail-open |
| `src/extraction/pdf.rs` | `extract()` via lopdf : texte page par page + métadonnées (auteur, titre, société) |
| `src/extraction/docx.rs` | `extract()` via zip + quick-xml : document.xml + comments.xml + core.xml |
| `src/detection/model_manager.rs` | `ensure_model()`, `list_models()`, `delete_model()`, `verify_model()`, `get/set_active_model()` |

**Subcommand CLI `mirageia model` :**
```bash
mirageia model list               # liste les modèles en cache
mirageia model download <name>    # télécharge depuis HuggingFace
mirageia model use <name>         # définit le modèle actif
mirageia model delete <name>      # supprime du cache
mirageia model verify             # vérifie le SHA-256 du modèle actif
```

**Dépendances ajoutées dans Cargo.toml :**
```toml
lopdf = "0.34"
quick-xml = "0.36"
base64 = "0.22"
reqwest = { ..., features = ["...", "blocking"] }
```

**Tests : 219 au total (203 unit + 16 integration)**
