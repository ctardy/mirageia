# Intégration ONNX — Référence d'implémentation

> **Statut** : Implémenté (v0.5.5+)
> **Feature flag** : `--features onnx` (activé par défaut dans les builds CI release)
> **Modèle** : `iiiorg/piiranha-v1-detect-personal-information` (pré-exporté ONNX, hébergé sur GitHub Releases)

---

## Vue d'ensemble

MirageIA embarque un modèle NER contextuel via ONNX Runtime pour détecter les noms de personnes, organisations et adresses — des entités que le regex ne peut pas détecter de manière fiable sans contexte.

Les deux couches sont complémentaires :
- La **couche regex** gère les PII à format fixe (IBAN, clés API, emails, IPs, cartes bancaires) avec une priorité plus élevée
- La **couche ONNX** ajoute les entités contextuelles qui ne chevauchent pas les résultats regex

---

## Modèle actif

**[`iiiorg/piiranha-v1-detect-personal-information`](https://huggingface.co/iiiorg/piiranha-v1-detect-personal-information)**

| Critère | Valeur |
|---------|--------|
| Taille sur disque | ~337 Mo (ONNX INT8 quantifié) |
| Mémoire (RSS stable) | ~946 Mo |
| Pic mémoire (chargement) | ~2,1 Go |
| Spécialisation | Détection PII (pas NER générique) |
| Langues | Multilingue dont FR |
| Latence CPU | ~15–30 ms / requête |

> **Note** : HuggingFace ne distribue ce modèle qu'en format SafeTensors. MirageIA héberge une version pré-exportée ONNX sur `github.com/ctardy/mirageia/releases/download/models-v1/`.

---

## Pipeline de détection (implémenté)

```
Texte brut
   ↓
[Couche regex]  validated_patterns (IBAN/MOD-97, CB/Luhn) → confiance 0.95
               + capture_validated_patterns (password + entropie)
               + patterns (API keys en premier, puis email/IP/téléphone/NSS) → confiance 0.90
   ↓
[Couche ONNX]  tokenizers::Tokenizer (WordPiece/BPE, crate HuggingFace)
               → token_ids + attention_mask + offsets caractères
               ↓
              ort::Session::run() → logits [n_tokens × n_labels]
               ↓
              argmax par token → labels BIO (B-PER, I-PER, B-ORG, O…)
               → BIO merging → Vec<PiiEntity> avec positions caractères
   ↓
[Fusion]       Entités ONNX ajoutées seulement sans chevauchement avec le regex
               Entités de type Unknown ignorées
   ↓
Vec<PiiEntity>  →  pipeline de pseudonymisation
```

### Implémentation de la fusion (`server.rs`)

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

**Dégradation gracieuse** : si le modèle est absent ou échoue au chargement, MirageIA démarre en mode regex seul — pas de crash.

---

## Structure des sources Rust

```
src/detection/
├── mod.rs               — struct PiiDetector (model + tokenizer + label_map)
│                           from_model_name(), detect(), load_label_map()
├── types.rs             — PiiEntity, PiiType
├── regex_detector.rs    — RegexDetector (validated_patterns + patterns)
├── validator.rs         — iban_valid(), luhn_valid(), shannon_entropy()
├── tokenizer.rs         — PiiTokenizer (wrapper crate tokenizers HuggingFace)
├── model.rs             — PiiModel (ort Session, infer())
└── model_manager.rs     — download/cache/vérification, get_active_model(), set_active_model()
```

### Types clés

```rust
pub struct PiiDetector {
    model: PiiModel,                      // ort::Session wrappé dans Mutex<>
    tokenizer: PiiTokenizer,              // tokenizers::Tokenizer
    label_map: Vec<String>,               // ex. ["O", "B-PER", "I-PER", …]
    thresholds: HashMap<PiiType, f32>,
    overlap_chars: usize,                 // chevauchement segmentation texte (200 chars)
}

pub struct PiiModel {
    session: std::sync::Mutex<ort::session::Session>,
    // Mutex requis : ort 2.0-rc.12 Session::run() prend &mut self
}
```

### Spécificités de l'API ort 2.0.0-rc.12

```rust
// Création de session
let session = ort::session::Session::builder()
    .map_err(|e| ...)?
    .commit_from_file(model_path)
    .map_err(|e| ...)?;

// Inférence
let ids_tensor = ort::value::Tensor::<i64>::from_array(input_ids_ndarray)?;
let mask_tensor = ort::value::Tensor::<i64>::from_array(attention_mask_ndarray)?;
let outputs = session.run(ort::inputs![
    "input_ids" => ids_tensor,
    "attention_mask" => mask_tensor
])?;
let (shape, data) = outputs[0].try_extract_tensor::<f32>()?;
// shape: &[i64], data: &[f32] — index: data[token_idx * num_labels + label_idx]
```

Dépendances : `ort = { version = "2.0.0-rc.12", features = ["download-binaries", "ndarray"] }`, `ndarray = "0.17"` (doit correspondre à la version interne d'ort).

---

## CLI de gestion des modèles

```bash
mirageia model list                        # liste les modèles en cache, marque l'actif
mirageia model download <hf-repo>          # téléchargement depuis GitHub Releases (puis HF en fallback)
mirageia model use <nom>                   # définir le modèle actif (écrit ~/.mirageia/active_model)
mirageia model delete <nom>                # supprimer du cache
mirageia model verify                      # vérification intégrité SHA-256
```

### Structure sur disque

```
~/.mirageia/
├── active_model          — une ligne : nom du modèle (ex. iiiorg/piiranha-v1-detect-personal-information)
└── models/
    └── iiiorg__piiranha-v1-detect-personal-information/
        ├── model.onnx        (~337 Mo)
        ├── tokenizer.json    (~16 Mo — vocab + règles BPE)
        ├── config.json       (map id2label)
        └── meta.json         (URL source, downloaded_at, version)
```

Nommage des répertoires : `/` → `__` (ex. `iiiorg/piiranha-v1-detect-personal-information` → `iiiorg__piiranha-v1-detect-personal-information`).

### Stratégie de téléchargement

`ensure_model()` essaie dans l'ordre :
1. **GitHub Releases** — `https://github.com/ctardy/mirageia/releases/download/models-v1/{safe_name}.tar.gz` (bundle ONNX pré-exporté, sans Python/optimum requis)
2. **Fallback HuggingFace** — téléchargement fichier par fichier si l'asset GitHub n'est pas disponible

---

## Comportement au démarrage

| Situation | Comportement |
|-----------|-------------|
| `active_model` défini, fichiers modèle présents | Charge le détecteur ONNX, log "détection contextuelle active" |
| `active_model` défini, fichiers manquants | Log warn, démarre en mode regex seul (fail-open) |
| Pas de fichier `active_model` | Démarre en mode regex seul |
| Erreur d'inférence | Log error, la requête concernée utilise uniquement le regex |

Le nom du modèle actif est exposé dans :
- `GET /health` → `"onnx_model": "iiiorg/piiranha-v1-detect-personal-information"` (ou `null`)
- `mirageia console` → `Detection  : regex + ONNX (iiiorg/piiranha-v1-detect-personal-information)`

---

## Activation du modèle ONNX

### Déploiement serveur/Docker

```bash
# 1. Télécharger le modèle (dans le container en cours d'exécution)
docker exec mirageia mirageia model download iiiorg/piiranha-v1-detect-personal-information

# 2. Le définir comme actif
docker exec mirageia mirageia model use iiiorg/piiranha-v1-detect-personal-information

# 3. Rebuilder l'image Docker (l'entrypoint est COPY lors du build)
cd /opt/docker/mirageia
docker compose build
docker compose up -d
```

> Le modèle est persisté dans le volume `./home/.mirageia` et survit aux redémarrages du container.

### Installation locale

```bash
mirageia model download iiiorg/piiranha-v1-detect-personal-information
mirageia model use iiiorg/piiranha-v1-detect-personal-information
mirageia  # redémarrer le proxy
```

---

## Besoins en mémoire

| Mode | RSS | VmPeak (chargement) |
|------|-----|---------------------|
| Regex seul | ~10 Mo | ~10 Mo |
| ONNX actif | ~946 Mo | ~2,1 Go |

Pour les déploiements Docker avec ONNX activé, définir la limite mémoire à **au moins 3 Go** :

```yaml
deploy:
  resources:
    limits:
      memory: 3G
```

---

## Notes d'implémentation

### Segmentation du texte

Les textes longs sont découpés en segments chevauchants (chevauchement 200 caractères) pour éviter de tronquer des entités aux frontières de segment. Les résultats de tous les segments sont fusionnés avec déduplication.

### Mapping token → caractère

La crate `tokenizers` retourne `encoding.get_offsets()` : `(char_start, char_end)` par token. Ces offsets sont utilisés pour reconstruire les positions correctes `PiiEntity { start, end }` dans le texte original.

### BIO merging

Les labels suivent le schéma BIO. Les tokens `I-*` adjacents avec le même type que le `B-*` précédent sont fusionnés en une seule entité couvrant leur plage de caractères combinée.

```
Token:  "Jean"  "Du"   "##pont"  "travaille"
Label:  B-PER   I-PER  I-PER     O
→ entité : "Jean Dupont" (fusionnée, positions caractères depuis offsets)
```

### Faux positifs

Cas attendus : personnages historiques ("Thomas Edison"), noms de variables génériques. Atténués par la whitelist dans `config.toml` et le chevauchement d'entités avec les résultats regex.

---

## Ajouter un nouveau modèle

Tout modèle HuggingFace de classification de tokens compatible ONNX Runtime peut être utilisé :

```bash
# Exporter en ONNX (nécessite optimum)
pip install optimum[onnxruntime]
optimum-cli export onnx \
  --model <hf-repo> \
  --task token-classification \
  ~/.mirageia/models/<safe_name>/

# Activer
mirageia model use <hf-repo>
```

Pour distribuer des bundles ONNX pré-exportés, créer une release GitHub avec un asset `{safe_name}.tar.gz` contenant `model.onnx`, `tokenizer.json`, `config.json` à la racine de l'archive.
