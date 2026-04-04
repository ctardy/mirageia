# Environnement de développement Linux — MirageIA

Guide pour installer un poste de dev Linux (natif ou WSL) capable de compiler, tester et lancer MirageIA localement.

---

## Prérequis systeme

```bash
# Debian / Ubuntu
sudo apt update && sudo apt install -y build-essential pkg-config libssl-dev git curl

# Fedora
sudo dnf groupinstall -y "Development Tools" && sudo dnf install -y openssl-devel git curl

# Arch
sudo pacman -Syu --noconfirm base-devel openssl git curl
```

---

## 1. Installer Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# Verifier
rustc --version    # 1.75+ requis
cargo --version
```

Ajouter dans `~/.bashrc` ou `~/.zshrc` si ce n'est pas deja fait :

```bash
. "$HOME/.cargo/env"
```

---

## 2. Cloner le depot

```bash
git clone https://github.com/ctardy/mirageia.git
cd mirageia
```

---

## 3. Compiler

```bash
# Build dev (rapide, non optimise)
cargo build

# Build release (optimise, pour tester les perfs)
cargo build --release
```

Le binaire est dans :
- `target/debug/mirageia` (dev)
- `target/release/mirageia` (release)

---

## 4. Lancer les tests

```bash
# Tous les tests (unitaires + e2e)
cargo test

# Unitaires seulement
cargo test --lib

# E2E seulement
cargo test --test e2e_proxy

# Un module specifique
cargo test -- detection::regex

# Un test precis
cargo test -- test_detect_email

# Avec sortie verbose
cargo test -- --nocapture
```

---

## 5. Verifications avant commit (reproduit la CI)

La CI execute deux jobs : clippy et tests. Pour reproduire localement :

```bash
# 1. Clippy (linter strict, -D warnings = erreur sur tout warning)
rustup component add clippy   # une seule fois
cargo clippy -- -D warnings

# 2. Tests
cargo test

# 3. Check rapide (sans linker, plus rapide que build)
cargo check
```

**Patterns clippy courants a eviter :**

| Pattern | Erreur clippy | Correction |
|---------|---------------|------------|
| `x >= 200 && x < 300` | `manual_range_contains` | `(200..300).contains(&x)` |
| `v.sort_by(\|a,b\| b.len().cmp(&a.len()))` | `unnecessary_sort_by` | `v.sort_by_key(\|b\| Reverse(b.len()))` |
| `let _ = x;` sur un Result | `let_underscore_drop` | `drop(x);` explicite |

---

## 6. Lancer le proxy en dev

```bash
# Mode normal
cargo run

# Avec logs debug
MIRAGEIA_LOG_LEVEL=debug cargo run

# Mode passthrough (sans pseudonymisation)
cargo run -- proxy --passthrough

# Avec une commande (ex: curl de test)
cargo run -- wrap -- curl -s http://localhost:3100/health
```

---

## 7. Commandes utiles au quotidien

```bash
# Formatter le code (optionnel, pas dans la CI pour l'instant)
cargo fmt

# Voir les dependances obsoletes
cargo install cargo-outdated   # une seule fois
cargo outdated

# Nettoyer le cache de build (si problemes bizarres)
cargo clean

# Recompiler tout
cargo clean && cargo build
```

---

## 8. Structure du projet

```
mirageia/
├── src/
│   ├── main.rs                    # CLI (clap) : proxy, console, setup, stop, update, wrap
│   ├── config/settings.rs         # AppConfig, chargement config.toml + env
│   ├── detection/
│   │   ├── regex_detector.rs      # Detection PII par regex
│   │   ├── types.rs               # Enum PiiType + PiiEntity
│   │   ├── postprocess.rs         # Post-traitement des entites
│   │   └── mod.rs                 # Re-exports
│   ├── mapping/
│   │   ├── table.rs               # MappingTable (bidirectionnel, en memoire)
│   │   └── crypto.rs              # Chiffrement AES-256-GCM
│   ├── pseudonymization/
│   │   ├── generator.rs           # Generation de pseudonymes realistes
│   │   ├── replacer.rs            # Remplacement PII -> pseudonymes
│   │   ├── depseudonymizer.rs     # Remplacement pseudonymes -> originaux
│   │   ├── fragment_restorer.rs   # Restauration des fragments dans les reponses
│   │   └── dictionaries.rs        # Dictionnaires de noms, prenoms, etc.
│   ├── proxy/
│   │   ├── server.rs              # Handler axum, ProxyEvent, graceful shutdown
│   │   ├── client.rs              # Client HTTP upstream (reqwest)
│   │   ├── router.rs              # Routage provider (Anthropic, OpenAI)
│   │   ├── extractor.rs           # Extraction champs texte du JSON
│   │   └── error.rs               # Types d'erreur proxy
│   └── streaming/
│       ├── sse_parser.rs          # Parser SSE (Anthropic + OpenAI)
│       └── buffer.rs              # Buffer de de-pseudonymisation streaming
├── tests/
│   └── e2e_proxy.rs               # Tests d'integration (mock upstream + proxy)
├── docs/                          # Documentation FR/EN
├── .github/workflows/
│   ├── ci.yml                     # CI : clippy + tests (Linux + Windows)
│   └── release.yml                # Release : build + upload binaires
├── Cargo.toml                     # Dependances et metadonnees
└── CLAUDE.md                      # Instructions pour Claude Code
```

---

## 9. Workflow de dev recommande

```bash
# 1. Creer une branche
git checkout -b feat/ma-feature

# 2. Coder...

# 3. Verifier avant commit
cargo clippy -- -D warnings && cargo test

# 4. Commiter
git add <fichiers>
git commit -m "feat: description"

# 5. Pousser
git push -u origin feat/ma-feature
```

---

## 10. WSL (Windows Subsystem for Linux)

Si vous developpez sur Windows avec WSL :

```powershell
# Depuis PowerShell (admin)
wsl --install -d Ubuntu
```

Puis dans le terminal WSL, suivre ce guide depuis l'etape 1.

Le code source peut etre :
- **Dans WSL** (`/home/user/mirageia`) : meilleures performances de compilation
- **Sur Windows monte** (`/mnt/c/dev/projects/mirageia`) : plus lent a compiler mais accessible des deux cotes

Recommandation : cloner dans WSL pour la compilation, utiliser VS Code avec l'extension "Remote - WSL" pour editer.

---

## Troubleshooting

### `linker 'cc' not found`

```bash
sudo apt install build-essential
```

### `failed to run custom build command for openssl-sys`

```bash
sudo apt install pkg-config libssl-dev
```

### Les tests e2e timeout

Les tests e2e lancent des serveurs sur des ports aleatoires. Si un pare-feu bloque les connexions localhost :

```bash
# Verifier que localhost fonctionne
curl http://127.0.0.1:3100 2>&1 | head -1
# Doit retourner "connection refused" (pas "timeout")
```

### `cargo test` est lent

```bash
# Premier build long (compile toutes les deps), les suivants sont incrementaux
# Pour accelerer, utiliser le linker mold :
sudo apt install mold
```

Ajouter dans `~/.cargo/config.toml` :

```toml
[target.x86_64-unknown-linux-gnu]
linker = "clang"
rustflags = ["-C", "link-arg=-fuse-ld=mold"]
```
