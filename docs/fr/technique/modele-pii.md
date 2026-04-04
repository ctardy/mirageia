# Détection PII — Implémentation actuelle et feuille de route

## Phase actuelle : Regex + Validation algorithmique

La détection PII de MirageIA repose sur deux couches complémentaires, sans serveur externe ni GPU requis.

### Couche 1 — Patterns validés algorithmiquement (confiance 0.95)

Ces patterns combinent une regex large avec un validateur de checksum :

| Type PII | Regex | Validateur | Algorithme |
|----------|-------|-----------|------------|
| IBAN | `\b[A-Z]{2}\d{2}(?:\s?[A-Z0-9]{4}){2,7}…\b` | `iban_valid()` | MOD-97 (ISO 13616) |
| Carte bancaire | Visa / MC / Amex / Discover | `luhn_valid()` | Luhn (ISO/IEC 7812) |

**Principe MOD-97 (IBAN)** : déplacer les 4 premiers caractères à la fin, remplacer les lettres (A=10…Z=35), effectuer le modulo 97 par blocs de 9 chiffres. Résultat attendu : 1.

**Principe Luhn (CB)** : doubler chaque 2ème chiffre depuis la droite (si résultat ≥ 10, soustraire 9), sommer tous les chiffres, le total doit être divisible par 10.

### Couche 2 — Patterns simples (confiance 0.90)

Patterns regex sans validation post-matching. **Ordre d'exécution critique** (voir section ci-dessous) :

| Priorité | Type | Exemples détectés |
|----------|------|--------------------|
| 1 | Clé Anthropic | `sk-ant-api03-…` |
| 2 | Clé OpenAI | `sk-proj-…`, `sk-…` (48+ chars) |
| 3 | Clé Stripe | `sk_live_…`, `pk_test_…` |
| 4 | Token GitHub | `ghp_…`, `gho_…`, `ghu_…` |
| 5 | Token Slack | `xoxb-…`, `xoxp-…` |
| 6 | AWS Access Key | `AKIA…`, `ASIA…` |
| 7 | JWT | `eyJ…` |
| 8 | Clé générique | `sk-…`, `api-…`, `token-…` |
| 9 | Email | `user@domain.tld` |
| 10 | IPv4 | `192.168.1.22` |
| 11 | IPv6 | `2001:db8::1` |
| 12 | Téléphone FR | `06 12 34 56 78`, `+33 6…` |
| 13 | NSS | `1 85 12 75 123 456 78` |

Sources : patterns inspirés de [Presidio (MIT)](https://github.com/microsoft/presidio) et [gitleaks (MIT)](https://github.com/gitleaks/gitleaks).

### Couche 3 — Entropie de Shannon (secrets génériques)

Pour les chaînes à haute entropie sans préfixe connu :

```rust
pub fn looks_like_secret(s: &str) -> bool {
    shannon_entropy(s) > 3.5
        && s.len() >= 12
        && char_class_count(s) >= 3  // minuscules + majuscules + chiffres + spéciaux
}
```

### Ordre d'exécution et détection de chevauchement

```
validated_patterns (IBAN, CB)
      ↓ confiance 0.95, priorité absolue
patterns spécifiques (API keys)
      ↓ confiance 0.90, tournent en premier
patterns génériques (email, IP, téléphone)
      ↓ ignorés si chevauchement avec entité déjà enregistrée
```

**Règle critique** : les patterns API keys doivent impérativement se trouver **avant** les patterns génériques (téléphone, IP) dans la liste. Sinon, le pattern téléphone peut matcher des chiffres contenus dans une clé API (`0123456789` dans `sk-ant-api03-…0123456789AB`), s'enregistrer en premier, et le filtre de chevauchement bloque ensuite la détection de la clé entière.

## Implémentation Rust

```
src/detection/
├── mod.rs               — module racine
├── types.rs             — PiiEntity, PiiType
├── regex_detector.rs    — RegexDetector (patterns + validated_patterns)
└── validator.rs         — iban_valid(), luhn_valid(), shannon_entropy()
```

### `RegexDetector::detect()` — algorithme

```rust
pub fn detect(&self, text: &str) -> Vec<PiiEntity> {
    let mut entities = Vec::new();

    // 1. Patterns validés (IBAN, CB) — confiance 0.95
    for (pii_type, regex, validator) in &self.validated_patterns {
        for mat in regex.find_iter(text) {
            if validator(mat.as_str()) {
                push_if_new(&mut entities, …, 0.95);
            }
        }
    }

    // 2. Patterns simples — confiance 0.90, API keys d'abord
    for (pii_type, regex) in &self.patterns {
        for mat in regex.find_iter(text) {
            let overlaps = entities.iter().any(|e| start < e.end && end > e.start);
            if !overlaps {
                push_if_new(&mut entities, …, 0.90);
            }
        }
    }

    entities.sort_by_key(|e| e.start);
    entities
}
```

## Tests

```bash
# Lancer tous les tests (199 au total)
docker run --rm -v /opt/projet/mirageia:/workspace -w /workspace rust:latest cargo test

# Tests clés détection PII
cargo test test_detect_iban
cargo test test_iban_not_detected_as_phone
cargo test test_detect_credit_card
cargo test test_detect_anthropic_key
cargo test test_detect_secret_high_entropy
```

## Phase suivante : modèle ONNX embarqué

La détection par regex couvre les PII à pattern fixe. Pour la détection contextuelle (noms de personnes, organisations, adresses), la feuille de route prévoit un modèle ONNX embarqué.

### Modèles candidats

| Modèle | Taille | Capacités | Latence |
|--------|--------|-----------|---------|
| DistilBERT-PII | ~260 Mo (INT8) | 33 types PII | 5–15ms |
| AnonymizerSLM Qwen3 0.6B | ~400 Mo (Q4) | Contextuel (≠ "Thomas Edison") | 50–200ms |
| Qwen3 1.7B | ~1.2 Go (Q4) | Précision maximale, multilingue | 100–500ms |

### Intégration prévue

```
[Texte brut]
     ↓
[Tokenizer HuggingFace (crate tokenizers)]
     ↓
[ONNX Runtime (crate ort)]
     ↓
[Post-processing] → positions, types, scores de confiance
     ↓
[Liste d'entités PII]
```

Le modèle `.onnx` sera téléchargé au premier lancement depuis GitHub Releases et mis en cache dans `~/.mirageia/models/`. Le feature flag `onnx` est déjà présent dans `Cargo.toml` :

```toml
[features]
default = []
onnx = ["ort"]
```

### Benchmarks cibles (ONNX)

| Métrique | Objectif |
|----------|----------|
| Précision (vrais positifs) | > 90% |
| Rappel (PII non manquées) | > 95% |
| Latence par requête | < 100ms |
| Mémoire | < 800 Mo |
| Taille binaire (sans modèle) | < 30 Mo |

## Références

- [Presidio (Microsoft, MIT)](https://github.com/microsoft/presidio) — patterns IBAN et CB
- [gitleaks (MIT)](https://github.com/gitleaks/gitleaks) — patterns clés API
- [ONNX Runtime](https://onnxruntime.ai/) — runtime d'inférence cross-platform
- [ort (crate Rust)](https://github.com/pykeio/ort) — bindings Rust pour ONNX Runtime
- [AnonymizerSLM](https://huggingface.co/blog/pratyushrt/anonymizerslm) — modèles PII spécialisés
- [CloakPipe](https://github.com/rohansx/cloakpipe) — proxy Rust avec NER ONNX
