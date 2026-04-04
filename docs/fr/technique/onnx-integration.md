# Intégration ONNX — Spécification d'implémentation

> **Statut** : À implémenter (phase suivante après v0.4.x)
> **Prérequis** : régex + validation algorithmique stable (v0.4.3 ✅)
> **Feature flag** : `--features onnx` (déjà déclaré dans `Cargo.toml`)

---

## Objectif

Compléter la détection regex (PII à format fixe) par une détection **contextuelle** via un modèle NER embarqué. Cible principale : noms de personnes, organisations, adresses — entités sans pattern prévisible.

La détection regex conserve la **priorité** sur les PII à format fixe (IBAN, clés API, CB). ONNX complète ce que le regex ne peut pas voir.

---

## Modèle recommandé

**[`iiiorg/piiranha-v1-detect-personal-information`](https://huggingface.co/iiiorg/piiranha-v1-detect-personal-information)**

| Critère | Valeur |
|---------|--------|
| Taille | ~280 Mo (INT8 quantifié) |
| Spécialisation | PII direct (pas NER générique) |
| Langues | Multilingue dont FR |
| Format | Export ONNX disponible sur HuggingFace |
| Latence CPU | ~15ms / 500 tokens |

Alternatives documentées :

| Modèle | Taille | Points forts | Limite |
|--------|--------|--------------|--------|
| `dslim/bert-base-NER` | ~170 Mo | Léger, rapide | Labels génériques (PER/ORG/LOC), pas PII |
| `lakshyakh93/deberta_finetuned_pii` | ~350 Mo | 12 types PII | Principalement EN |
| `Qwen3 0.6B` | ~400 Mo (Q4) | Contextuel avancé | Plus lourd, latence 50–200ms |

---

## Architecture cible

### Pipeline de détection

```
Texte brut
   ↓
Tokenizer HuggingFace (WordPiece/BPE)
   → token_ids + attention_mask + offsets caractères
   ↓
ONNX Runtime (crate ort)
   → logits : [n_tokens × n_labels]
   ↓
Post-processing
   → argmax par token → label BIO (B-PER, I-PER, B-ORG, O…)
   → BIO merging : reconstituer les entités multi-tokens
   → mapper offsets tokens → positions caractères (via encoding.get_offsets())
   ↓
Vec<PiiEntity> — fusionné avec résultats regex (sans chevauchement)
```

### Fusion regex + ONNX

```rust
pub fn detect_all(text: &str) -> Vec<PiiEntity> {
    // Regex toujours actif (PII à format fixe, prioritaire)
    let mut entities = regex_detector.detect(text);

    #[cfg(feature = "onnx")]
    {
        let onnx_entities = onnx_detector.detect(text);
        for e in onnx_entities {
            // Ne pas écraser ce que le regex a déjà détecté
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

**Dégradation gracieuse** : si le feature `onnx` n'est pas compilé ou si le modèle est absent, MirageIA démarre en mode regex seul — pas de crash.

---

## Structure des fichiers Rust à créer

```
src/detection/
├── mod.rs                  — exposer OnnxDetector si feature onnx
├── onnx_detector.rs        — (NOUVEAU) inférence + BIO merge
└── model_manager.rs        — (NOUVEAU) download + cache + vérification hash
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
        // 1. Tokeniser avec offsets
        let encoding = self.tokenizer.encode(text, false)?;
        let offsets = encoding.get_offsets(); // [(char_start, char_end), …]

        // 2. Inférence
        let logits = self.session.run(inputs![
            encoding.get_ids(),
            encoding.get_attention_mask()
        ])?;

        // 3. Argmax + BIO merging + conversion offsets → PiiEntity
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

    // Télécharger depuis HuggingFace
    println!("  → Téléchargement du modèle {} (~280 Mo)…", config.model_name);
    download_hf_model(&config.model_name, &model_dir)?;
    write_meta(&meta_path, &config.model_name)?;

    Ok(model_dir)
}

fn meta_valid(meta_path: &Path, model_path: &Path) -> bool {
    // Vérifier que le hash SHA-256 du .onnx correspond à model.json
    // + vérification hebdomadaire optionnelle contre l'API HuggingFace
    …
}
```

---

## Cache et gestion des modèles

### Structure sur disque

```
~/.mirageia/
└── models/
    ├── piiranha-v1/
    │   ├── model.onnx          (~280 Mo)
    │   ├── tokenizer.json      (vocab + règles BPE)
    │   └── model.json          (hash SHA-256 + version + source + date)
    └── bert-base-NER/          (modèle alternatif, peut coexister)
        ├── model.onnx
        ├── tokenizer.json
        └── model.json
```

### Format `model.json`

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
model = "iiiorg/piiranha-v1-detect-personal-information"  # défaut
model_cache_dir = "~/.mirageia/models"
check_updates = "weekly"   # never | startup | daily | weekly
confidence_threshold = 0.85
```

---

## CLI de gestion des modèles

```bash
mirageia model list                        # liste les modèles en cache + actif
mirageia model download <hf-repo>          # télécharger sans activer
mirageia model use <nom>                   # changer le modèle actif
mirageia model update                      # vérifier et appliquer les mises à jour
mirageia model delete <nom>                # supprimer du cache
mirageia model verify                      # vérifier le hash SHA-256 du modèle actif
```

---

## Comportement au démarrage

| Situation | Comportement |
|-----------|-------------|
| Premier lancement | Télécharge le modèle par défaut, affiche la progression |
| Lancements suivants | Charge depuis le cache, démarrage instantané |
| Modèle corrompu (hash invalide) | Re-télécharge automatiquement |
| Mise à jour disponible | Notifie, **ne met pas à jour sans confirmation** |
| Changement de modèle | `mirageia model use <nom>` ou éditer `config.toml` |
| Pas de réseau + cache absent | Démarre en **mode regex seul** (pas de crash) |
| Pas de réseau + cache présent | Utilise le cache existant normalement |

**Principe** : les mises à jour de modèle ne sont **jamais silencieuses** — un modèle qui change peut modifier le comportement de détection en production.

---

## Points d'attention implémentation

### Mapping token → caractère
Point le plus délicat. Un tokenizer BPE/WordPiece fragmente les mots :
- `"Dupont"` → `["Du", "##pont"]`
- `"jean.dupont@acme.fr"` → plusieurs tokens

La crate `tokenizers` retourne `encoding.get_offsets()` : tableau de `(char_start, char_end)` par token. Utiliser ces offsets pour reconstruire les `PiiEntity { start, end }` corrects dans le texte original.

### BIO merging
Les labels NER suivent le schéma BIO :
- `B-PER` = début d'une entité personne
- `I-PER` = continuation
- `O` = hors entité

```
Token:  "Jean"  "Du"   "##pont"  "travaille"
Label:  B-PER   I-PER  I-PER     O
→ entité : "Jean Dupont" (positions fusionnées)
```

### Seuil de confiance
Appliquer un seuil sur le score softmax avant de valider une entité. Valeur recommandée : 0.85. Configurable par l'utilisateur pour ajuster le ratio précision/rappel.

### Faux positifs contextuels
Exemples attendus :
- `"Thomas Edison a inventé l'ampoule"` → détecté comme PER malgré le contexte historique
- `"Le champ `username` contient…"` → `username` potentiellement détecté

Mitigation : whitelist utilisateur dans `config.toml` + seuil de confiance élevé.

---

## Ordre d'implémentation suggéré

1. `model_manager.rs` — download HuggingFace + cache + hash SHA-256
2. `onnx_detector.rs` — tokenize + inférence + BIO merge basique
3. Brancher dans `detect_all()` derrière `#[cfg(feature = "onnx")]`
4. Tests : comparer rappel regex seul vs regex + ONNX sur corpus PII FR/EN
5. CLI `mirageia model` (list, use, update, delete)
6. Config `~/.mirageia/config.toml`
7. Release `v0.5.0` avec `--features onnx` activé par défaut dans le CI
