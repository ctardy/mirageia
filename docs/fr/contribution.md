# Guide de contribution et mise à jour — MirageIA

## Workflow de développement

### Cycle de travail

```
1. Créer une branche    git checkout -b feat/ma-feature
2. Coder + tester       cargo test (à chaque module)
3. Vérifier             cargo test (suite complète)
4. Commiter             git commit (messages en français)
5. Pull request         Revue + merge dans main
```

### Conventions

- **Langue** : tout en français (code, commentaires, commits, docs) sauf les identifiants Rust
- **Commits** : `type: description` — ex: `feat: ajout détection numéros SIRET`, `fix: buffer SSE overflow`, `docs: guide installation`
- **Tests** : chaque module inclut un bloc `#[cfg(test)] mod tests` — pas de code sans test
- **Pas de rétro-compatibilité** : le projet est en construction, pas de `@Deprecated`

### Commandes essentielles

```bash
# Build
cargo build
cargo build --release

# Tests
cargo test                          # Tous (133+)
cargo test --lib                    # Unitaires seulement
cargo test --test e2e_proxy         # E2E seulement
cargo test -- detection::regex      # Un module
cargo test -- test_detect_email     # Un test précis

# Vérification rapide (sans linker)
cargo check

# Lancer le proxy
cargo run

# Lancer avec logs debug
MIRAGEIA_LOG_LEVEL=debug cargo run
```

---

## Comment ajouter un nouveau type de PII

Exemple : ajouter la détection des numéros SIRET.

### Étape 1 — Ajouter le type dans `detection/types.rs`

```rust
pub enum PiiType {
    // ... existants ...
    Siret,  // ← nouveau
}
```

Mettre à jour `Display`, `label_to_pii_type()` et `default_threshold()`.

### Étape 2 — Ajouter la regex dans `detection/regex_detector.rs`

```rust
// Dans RegexDetector::new()
patterns.push((
    PiiType::Siret,
    Regex::new(r"\b\d{3}\s?\d{3}\s?\d{3}\s?\d{5}\b").unwrap(),
));
```

### Étape 3 — Ajouter le générateur dans `pseudonymization/generator.rs`

```rust
// Dans PseudonymGenerator::generate()
PiiType::Siret => self.gen_siret(&mut rng, original),

// Nouvelle méthode
fn gen_siret(&self, rng: &mut impl Rng, original: &str) -> String {
    // Préserver le format, remplacer les chiffres
    original.chars().map(|c| {
        if c.is_ascii_digit() { char::from_digit(rng.gen_range(0..10), 10).unwrap() }
        else { c }
    }).collect()
}
```

### Étape 4 — Ajouter les tests

Dans `regex_detector.rs` :
```rust
#[test]
fn test_detect_siret() {
    let entities = detector().detect("SIRET: 123 456 789 00012");
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].entity_type, PiiType::Siret);
}
```

Dans `generator.rs` :
```rust
#[test]
fn test_gen_siret_preserves_format() {
    let gen = generator();
    let siret = gen.generate(&PiiType::Siret, "123 456 789 00012");
    assert_eq!(siret.len(), "123 456 789 00012".len());
}
```

### Étape 5 — Vérifier

```bash
cargo test
# Tous les tests doivent passer, y compris les nouveaux
```

---

## Comment modifier le pipeline de pseudonymisation

### Flux des données

```
server.rs::proxy_handler()
    → pseudonymize_request()
        → extractor::extract_text_fields()     # JSON → champs texte
        → regex_detector::detect_with_whitelist()  # texte → entités PII
        → replacer::pseudonymize_text()         # entités → texte pseudonymisé
            → generator::generate()             # type PII → pseudonyme
            → mapping::table::insert()          # stockage chiffré
        → extractor::rebuild_body()             # champs → JSON reconstruit
    → client::forward()                         # envoi upstream
    → build_depseudonymized_response()
        → depseudonymizer::depseudonymize_text()  # pseudonymes → originaux
        ou streaming::buffer::push()               # SSE avec buffer
```

### Points d'extension

| Pour... | Modifier |
|---|---|
| Ajouter un type de PII | `types.rs` + `regex_detector.rs` + `generator.rs` |
| Changer la stratégie de pseudonymisation | `generator.rs` |
| Supporter un nouveau provider LLM | `router.rs` + `extractor.rs` + `sse_parser.rs` |
| Modifier le chiffrement | `mapping/crypto.rs` |
| Ajouter un champ JSON à analyser | `extractor.rs` |

---

## Comment ajouter un nouveau provider LLM

Exemple : ajouter Mistral (`/v1/chat/completions` avec un format légèrement différent).

### Étape 1 — `proxy/router.rs`

```rust
pub enum Provider {
    Anthropic,
    OpenAI,
    Mistral,  // ← nouveau
}

pub fn resolve_provider(path: &str) -> Option<Provider> {
    if path.starts_with("/v1/messages") {
        Some(Provider::Anthropic)
    } else if path.starts_with("/v1/chat/completions") {
        Some(Provider::OpenAI)  // ou Mistral selon un header ?
    }
    // ...
}
```

### Étape 2 — `proxy/extractor.rs`

Ajouter `extract_mistral_fields()` si le format JSON diffère.

### Étape 3 — `streaming/sse_parser.rs`

Ajouter le parsing SSE si le format de streaming diffère.

### Étape 4 — `config/settings.rs`

Ajouter `mistral_base_url` dans `AppConfig`.

---

## Process de release

### Vérifications avant release

```bash
# 1. Tous les tests passent
cargo test

# 2. Pas de warnings
cargo check 2>&1 | grep warning

# 3. Build release fonctionne
cargo build --release

# 4. Le binaire fonctionne
./target/release/mirageia --help
```

### Versionning

Le projet suit [SemVer](https://semver.org/) :
- `0.x.y` — phase de développement (breaking changes possibles)
- `MAJOR.MINOR.PATCH` à partir de 1.0

Version dans `Cargo.toml` :
```toml
[package]
version = "0.1.0"
```

### Process de mise à jour

1. **Mettre à jour le code** sur une branche dédiée
2. **Lancer les tests** : `cargo test` — tous doivent passer
3. **Mettre à jour la version** dans `Cargo.toml`
4. **Mettre à jour la documentation** si le comportement change :
   - `README.md` — si nouvelles features visibles par l'utilisateur
   - `docs/officiel/installation.md` — si les prérequis changent
   - `docs/tickets/` — marquer les tickets complétés
5. **Commiter et merger** dans `main`
6. **Tagger** : `git tag v0.2.0`

### Mise à jour des dépendances

```bash
# Voir les mises à jour disponibles
cargo outdated    # (nécessite cargo-outdated)

# Mettre à jour Cargo.lock
cargo update

# Re-tester après mise à jour
cargo test
```

---

## Structure des tests

### Organisation

```
src/
├── config/settings.rs          # tests inline (#[cfg(test)])
├── detection/
│   ├── regex_detector.rs       # tests inline
│   ├── types.rs                # tests inline
│   ├── postprocess.rs          # tests inline
│   ├── tokenizer.rs            # tests inline
│   ├── model.rs                # tests inline
│   └── mod.rs                  # tests inline (label_map)
├── mapping/
│   ├── crypto.rs               # tests inline
│   └── table.rs                # tests inline
├── pseudonymization/
│   ├── dictionaries.rs         # tests inline
│   ├── generator.rs            # tests inline
│   ├── replacer.rs             # tests inline
│   └── depseudonymizer.rs      # tests inline
├── proxy/
│   ├── router.rs               # tests inline
│   └── extractor.rs            # tests inline
├── streaming/
│   ├── sse_parser.rs           # tests inline
│   └── buffer.rs               # tests inline
└── tests/
    └── e2e_proxy.rs            # tests d'intégration (mock upstream)
```

### Convention de nommage des tests

```rust
#[test]
fn test_<action>_<cas>() {
    // test_detect_email          → cas nominal
    // test_detect_no_pii         → cas vide
    // test_decrypt_wrong_key     → cas d'erreur
    // test_whitelist_excludes    → cas avec config
    // test_concurrent_access     → cas concurrent
}
```

### Ajouter un test e2e

Les tests e2e dans `tests/e2e_proxy.rs` lancent un mock upstream et le proxy :

```rust
#[tokio::test]
async fn test_mon_scenario() {
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
            "messages": [{"role": "user", "content": "Mon texte avec PII"}]
        }))
        .send()
        .await
        .unwrap();

    // Vérifier la requête envoyée au mock
    let captured = captured.lock().await;
    let sent = captured.as_ref().unwrap()["messages"][0]["content"].as_str().unwrap();
    assert!(!sent.contains("PII originale"));

    // Vérifier la réponse reçue
    assert_eq!(resp.status(), 200);
}
```
