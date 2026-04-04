# MirageIA

**Proxy de pseudonymisation intelligent pour API LLM.**

L'API ne voit jamais vos vraies données — elle voit un mirage.

```
Votre app  ──►  MirageIA (proxy local :3100)  ──►  API LLM (Anthropic / OpenAI)
                 │                                    │
                 ├─ Détecte les PII (regex + ONNX)    │
                 ├─ Pseudonymise avant envoi           │
                 └─ Restaure dans la réponse  ◄────────┘
```

## Le problème

Quand vous utilisez Claude, ChatGPT ou tout autre LLM via API, vos données transitent en clair vers des serveurs externes : noms, emails, adresses IP, clés API, numéros de téléphone… Ces données sensibles sont exposées sans que vous le sachiez.

## La solution

MirageIA s'intercale entre votre application et l'API LLM. Il détecte automatiquement les données sensibles, les remplace par des valeurs fictives cohérentes, et restaure les originaux dans la réponse.

| Donnée originale | Ce que l'API reçoit | Ce que vous recevez |
|---|---|---|
| `jean.dupont@acme.fr` | `alice@example.com` | `jean.dupont@acme.fr` (restauré) |
| `192.168.1.22` | `10.0.84.12` | `192.168.1.22` (restauré) |
| `06 12 34 56 78` | `06 47 91 28 53` | `06 12 34 56 78` (restauré) |
| `sk-abc123def456...` | `sk-xR9mK2pL7wQ4...` | `sk-abc123def456...` (restauré) |

Le LLM travaille avec des données fictives mais cohérentes — sa réponse est tout aussi pertinente, et vos données n'ont jamais quitté votre machine.

---

## Démarrage rapide

### Prérequis

- [Rust](https://rustup.rs/) (1.75+)
- GCC (via MSYS2 sur Windows) ou toolchain MSVC

### Installation

```bash
git clone <repo-url>
cd mirageia

# Sur Windows avec MSYS2 :
export PATH="/c/msys64/mingw64/bin:$HOME/.cargo/bin:$PATH"

cargo build --release
```

### Configuration guidée

```bash
# L'assistant vous guide : port, providers LLM, whitelist, shell
mirageia setup
```

### Utilisation

```bash
# Lancer le proxy
mirageia

# Utiliser Claude Code via le proxy (juste cette session)
mirageia wrap -- claude

# Surveiller les requêtes en temps réel (dans un autre terminal)
mirageia console
```

**Activation par session** — `mirageia wrap` lance votre commande avec le proxy activé, sans modifier votre shell. Quand la commande se termine, le proxy n'est plus utilisé :

```bash
mirageia wrap -- claude          # Claude Code protégé
mirageia wrap -- python app.py   # Script Python protégé
claude                           # Claude Code direct (sans proxy)
```

### Désactiver temporairement

```bash
# Option 1 : Mode passthrough (le proxy relaie sans pseudonymiser)
mirageia proxy --passthrough

# Option 2 : Arrêter le proxy — n'affecte PAS les apps lancées normalement
# Seules celles lancées via `mirageia wrap` passent par le proxy
```

### Vérification

```bash
# Health check
curl http://localhost:3100/health
# → {"status":"ok","passthrough":false,"pii_mappings":0}

# Requête test (nécessite une clé API Anthropic)
curl -X POST http://localhost:3100/v1/messages \
  -H "x-api-key: $ANTHROPIC_API_KEY" \
  -H "content-type: application/json" \
  -d '{
    "model": "claude-sonnet-4-20250514",
    "max_tokens": 100,
    "messages": [{"role": "user", "content": "Mon email est jean@acme.fr et mon IP est 192.168.1.50"}]
  }'
```

Dans les logs de MirageIA, vous verrez les PII détectées et pseudonymisées. La requête envoyée à Anthropic ne contiendra ni l'email ni l'IP originale.

---

## Configuration

MirageIA fonctionne sans configuration (zéro config). Pour personnaliser, créez `~/.mirageia/config.toml` :

```toml
[proxy]
listen_addr = "127.0.0.1:3100"  # Adresse d'écoute
log_level = "info"               # debug, info, warn, error
add_header = false               # Ajouter X-MirageIA: active aux requêtes
fail_open = true                 # Transmettre la requête si la pseudonymisation échoue
passthrough = false              # Mode passthrough : relayer sans pseudonymiser

[detection]
confidence_threshold = 0.75      # Seuil de confiance (0.0–1.0)
whitelist = [                    # Termes à ne jamais pseudonymiser
    "localhost",
    "127.0.0.1",
    "Thomas Edison",
]
```

Les variables d'environnement prennent le dessus sur le fichier :

| Variable | Description |
|---|---|
| `MIRAGEIA_LISTEN_ADDR` | Adresse d'écoute (ex: `0.0.0.0:3100`) |
| `MIRAGEIA_ANTHROPIC_URL` | URL de base Anthropic |
| `MIRAGEIA_OPENAI_URL` | URL de base OpenAI |
| `MIRAGEIA_LOG_LEVEL` | Niveau de log |
| `MIRAGEIA_PASSTHROUGH` | Activer le mode passthrough (toute valeur = activé) |

---

## Types de PII détectés

Le détecteur regex (v1) couvre les PII à pattern fixe :

| Type | Exemples | Pseudonyme généré |
|---|---|---|
| Email | `jean@acme.fr` | `alice@example.com` |
| IPv4 | `192.168.1.50` | `10.0.84.12` |
| IPv6 | `2001:db8::1` | `fd00::a1b2:c3d4` |
| Téléphone | `06 12 34 56 78` | `06 47 91 28 53` (format préservé) |
| Carte bancaire | `4111 1111 1111 1111` | `4892 7631 0458 2173` (Luhn valide) |
| IBAN | `FR7612345678901234567890` | `FR8398765432109876543210` |
| Clé API / token | `sk-abc123def456...` | `sk-xR9mK2pL7wQ4...` (préfixe préservé) |
| N° sécurité sociale | `1 85 07 75 123 456 78` | `2 91 03 69 847 215 34` |

Le détecteur ONNX contextuel (v2, en cours) ajoutera la détection de noms de personnes, adresses postales, et comprendra le contexte ("Thomas Edison" dans un cours d'histoire → pas masqué).

---

## Architecture

```
src/
├── main.rs                  CLI (proxy / setup / detect / wrap / console)
├── lib.rs                   Modules publics
├── config/
│   └── settings.rs          AppConfig, chargement TOML + env
├── proxy/
│   ├── server.rs            Handler axum, pipeline complet
│   ├── router.rs            Routage Anthropic / OpenAI par path
│   ├── client.rs            Client HTTP upstream (reqwest)
│   ├── extractor.rs         Extraction/rebuild JSON par provider
│   └── error.rs             Types d'erreurs proxy
├── detection/
│   ├── regex_detector.rs    Détecteur PII par regex (v1)
│   ├── types.rs             PiiType, PiiEntity, label_to_pii_type
│   ├── model.rs             Modèle ONNX (feature-gated, v2)
│   ├── tokenizer.rs         Tokenizer HuggingFace, segmentation
│   ├── postprocess.rs       Softmax, BIO decode, fusion entités
│   └── error.rs             Erreurs de détection
├── pseudonymization/
│   ├── generator.rs         Générateur de pseudonymes par type
│   ├── replacer.rs          Remplacement dans le texte (offsets)
│   ├── depseudonymizer.rs   Dé-pseudonymisation (AhoCorasick)
│   └── dictionaries.rs      Prénoms/noms embarqués
├── mapping/
│   ├── table.rs             Table bidirectionnelle (SHA-256 + AES-256-GCM)
│   ├── crypto.rs            Chiffrement/déchiffrement, zéroisation
│   └── error.rs             Erreurs de mapping
└── streaming/
    ├── sse_parser.rs        Parse/rebuild SSE Anthropic/OpenAI
    └── buffer.rs            Buffer pour pseudonymes split entre tokens
```

### Pipeline de traitement

```
REQUÊTE ENTRANTE
    │
    ▼
[Extraction JSON]  ← extractor.rs (champs textuels Anthropic/OpenAI)
    │
    ▼
[Détection PII]    ← regex_detector.rs (emails, IPs, tels, CB, IBAN, clés)
    │                 + whitelist filtering
    ▼
[Pseudonymisation] ← replacer.rs (positions décroissantes)
    │                 + generator.rs (pseudonymes cohérents par type)
    │                 + mapping/table.rs (AES-256-GCM en mémoire)
    ▼
[Reconstruction]   ← extractor.rs (rebuild JSON)
    │
    ▼
[Forward]          ← client.rs → API upstream
    │
    ▼
RÉPONSE UPSTREAM
    │
    ▼
[Dé-pseudo]        ← depseudonymizer.rs (AhoCorasick, longest-first)
    │                 ou buffer.rs (streaming SSE, split entre tokens)
    ▼
RÉPONSE CLIENT (données originales restaurées)
```

---

## Tests

```bash
# Tous les tests (144)
cargo test

# Tests unitaires uniquement
cargo test --lib

# Tests e2e (proxy + mock upstream)
cargo test --test e2e_proxy

# Tests d'un module spécifique
cargo test -- detection::regex_detector
cargo test -- mapping::crypto
cargo test -- pseudonymization
```

### Couverture des tests

| Module | Tests | Couverture |
|---|---:|---|
| config | 6 | Config par défaut, parsing TOML, partiel, vide, passthrough |
| proxy/router | 7 | Routage Anthropic/OpenAI, URLs |
| proxy/extractor | 9 | Extraction/rebuild JSON, content string/array, system |
| detection/types | 7 | Labels, seuils, aliases, display |
| detection/postprocess | 11 | Softmax, extraction, fusion, multi-token, seuils |
| detection/tokenizer | 5 | Segmentation, overlap, progression |
| detection/regex_detector | 16 | Email, IP, phone, CB, IBAN, API key, whitelist |
| detection/model | 2 | Répertoire modèles, fichiers manquants |
| detection/mod | 4 | Chargement label_map |
| mapping/crypto | 6 | AES-256-GCM roundtrip, nonces, unicode |
| mapping/table | 8 | Bidirectionnel, concurrent, IDs uniques |
| pseudonymization/generator | 13 | Tous les types PII, Luhn, format |
| pseudonymization/replacer | 5 | Positions, cohérence session |
| pseudonymization/depseudonymizer | 6 | Roundtrip, longest-first |
| streaming/sse_parser | 7 | Anthropic, OpenAI, DONE, rebuild |
| streaming/buffer | 7 | Split pseudonyme, flush |
| **e2e** | **12** | **Pipeline complet, passthrough, events SSE, dashboard** |
| **Total** | **145** | |

---

## Statut du projet

| Composant | Statut | Notes |
|---|---|---|
| Proxy HTTP transparent | ✅ Terminé | axum, routage Anthropic/OpenAI |
| Détection PII regex | ✅ Terminé | 8 types de PII |
| Pseudonymisation réversible | ✅ Terminé | Mapping AES-256-GCM |
| Dé-pseudonymisation réponses | ✅ Terminé | Non-streaming + SSE buffer |
| Configuration TOML + whitelist | ✅ Terminé | ~/.mirageia/config.toml |
| Fail-open | ✅ Terminé | Passthrough si erreur |
| Mode passthrough | ✅ Terminé | `--passthrough` / config / env var |
| Activation par session | ✅ Terminé | `mirageia wrap -- claude` |
| Console de monitoring | ✅ Terminé | `mirageia console` (SSE temps réel) |
| Dashboard web | ✅ Terminé | `/dashboard` embarqué dans le binaire |
| Docker + déploiement | ✅ Terminé | Dockerfile, guide ops, Apache reverse proxy |
| Tests e2e | ✅ Terminé | 145 tests |
| Détection ONNX contextuelle | 🔧 Structuré | Code prêt, ONNX Runtime bloqué par toolchain MSVC |
| Dashboard Tauri | 📋 Planifié | Phase 4 |

---

## Documentation

| Sujet | Lien |
|-------|------|
| | FR | EN |
|---|---|---|
| **Installation** | [`docs/fr/installation.md`](docs/fr/installation.md) | [`docs/en/installation.md`](docs/en/installation.md) |
| **Déploiement ops** | [`docs/fr/deploiement-ops.md`](docs/fr/deploiement-ops.md) | [`docs/en/deployment-ops.md`](docs/en/deployment-ops.md) |
| **Distribution** | [`docs/fr/distribution.md`](docs/fr/distribution.md) | [`docs/en/distribution.md`](docs/en/distribution.md) |
| **Contribution** | [`docs/fr/contribution.md`](docs/fr/contribution.md) | [`docs/en/contributing.md`](docs/en/contributing.md) |
| Architecture | [`docs/fr/architecture/`](docs/fr/architecture/) | [`docs/en/architecture/`](docs/en/architecture/) |
| Sécurité / Security | [`docs/fr/securite/`](docs/fr/securite/) | [`docs/en/security/`](docs/en/security/) |
| Technique / Technical | [`docs/fr/technique/`](docs/fr/technique/) | [`docs/en/technical/`](docs/en/technical/) |
| Recherche | [`docs/recherche/`](docs/recherche/) | |
| Tickets | [`docs/tickets/`](docs/tickets/) | |

## Licence

MIT
