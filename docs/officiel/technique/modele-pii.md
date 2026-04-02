# Modèle PII embarqué

## Approche : LLM local via ONNX Runtime

Contrairement aux solutions basées sur regex ou sur un serveur LLM externe (Ollama), MirageIA embarque directement le modèle dans le binaire via **ONNX Runtime**.

Référence : le projet [Murmure](https://github.com/Kieirra/murmure) utilise la même approche pour embarquer Whisper (speech-to-text) directement dans une application Tauri/Rust.

## Modèles candidats

### Option 1 : DistilBERT-PII (recommandé pour v1)
- **Taille** : ~260 Mo (quantifié INT8)
- **Capacités** : 33 types d'entités PII
- **Latence** : 5–15ms par inférence
- **Avantage** : léger, rapide, spécialisé
- **Inconvénient** : moins bon sur le contexte subtil

### Option 2 : AnonymizerSLM Qwen3 0.6B
- **Taille** : ~400 Mo (quantifié Q4)
- **Capacités** : détection contextuelle avancée (comprend "Thomas Edison" ≠ PII)
- **Latence** : 50–200ms
- **Avantage** : intelligence contextuelle supérieure
- **Inconvénient** : plus lourd, nécessite plus de RAM

### Option 3 : Qwen3 1.7B (cible v2)
- **Taille** : ~1.2 Go (quantifié Q4)
- **Capacités** : meilleure précision, meilleure compréhension multilingue
- **Latence** : 100–500ms
- **Avantage** : score 9.55/10 (proche GPT-4.1)
- **Inconvénient** : empreinte mémoire significative

## Intégration ONNX Runtime

```
[Texte brut]
     │
     ▼
[Tokenizer (embarqué)]  ← vocabulaire du modèle
     │
     ▼
[ONNX Runtime]          ← modèle .onnx embarqué ou téléchargé au 1er lancement
     │
     ▼
[Post-processing]        ← extraction des entités, positions, types, scores de confiance
     │
     ▼
[Liste d'entités PII]
```

### Dépendances Rust
- `ort` (crate Rust pour ONNX Runtime) — binding natif, pas de FFI Python
- `tokenizers` (crate HuggingFace) — tokenisation rapide en Rust pur

### Distribution du modèle
- **Option A** : modèle embarqué dans le binaire (taille binaire ~300+ Mo mais zéro téléchargement)
- **Option B** : téléchargement au premier lancement depuis un CDN/GitHub Release (binaire léger, ~20 Mo)
- **Recommandation** : Option B avec cache local dans `~/.mirageia/models/`

## Benchmarks cibles

| Métrique | Objectif |
|----------|----------|
| Précision (vrais positifs) | > 90% |
| Rappel (PII non manquées) | > 95% (mieux vaut un faux positif qu'une fuite) |
| Latence par requête | < 100ms |
| Mémoire | < 800 Mo |
| Taille binaire (sans modèle) | < 30 Mo |

## Références

- [ONNX Runtime](https://onnxruntime.ai/) — Runtime d'inférence cross-platform
- [ort (crate Rust)](https://github.com/pykeio/ort) — Bindings Rust pour ONNX Runtime
- [Murmure](https://github.com/Kieirra/murmure) — Exemple d'app Tauri avec modèle ONNX embarqué
- [AnonymizerSLM](https://huggingface.co/blog/pratyushrt/anonymizerslm) — Modèles spécialisés détection PII
- [CloakPipe](https://github.com/rohansx/cloakpipe) — Proxy Rust avec NER ONNX (DistilBERT-PII)
