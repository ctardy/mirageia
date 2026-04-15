# Distribution et installation — MirageIA

## Stratégie de distribution

### Principe

MirageIA est un **binaire unique** sans dépendance externe. L'installation consiste à :
1. Télécharger le binaire pour son OS
2. Le placer dans le PATH
3. Le lancer

Pas de Docker, pas de Python, pas de Node, pas de runtime. Un seul fichier exécutable.

---

## Phase actuelle — Binaire précompilé

### GitHub Releases

Chaque release publie des binaires pour 3 plateformes :

```
mirageia-v0.1.0-windows-x86_64.zip      (~5 Mo)
mirageia-v0.1.0-macos-aarch64.tar.gz    (~5 Mo)
mirageia-v0.1.0-linux-x86_64.tar.gz     (~5 Mo)
```

Contenu de chaque archive :
```
mirageia(.exe)       Le binaire
config.example.toml  Exemple de configuration
README.md            Instructions rapides
```

### Installation manuelle

#### Windows

```powershell
# Télécharger
Invoke-WebRequest -Uri "https://github.com/ctardy/mirageia/releases/latest/download/mirageia-windows-x86_64.zip" -OutFile mirageia.zip

# Extraire
Expand-Archive mirageia.zip -DestinationPath "$env:LOCALAPPDATA\MirageIA"

# Ajouter au PATH (PowerShell profile)
$env:PATH += ";$env:LOCALAPPDATA\MirageIA"
```

#### macOS / Linux

```bash
# Télécharger et installer
curl -sSfL https://github.com/ctardy/mirageia/releases/latest/download/mirageia-$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m).tar.gz | tar xz -C /usr/local/bin/
```

### Signature numérique (Windows)

Le binaire `mirageia.exe` est **signé numériquement** avec Microsoft Trusted Signing. L'éditeur affiché par Windows est `UITguard`.

Cela signifie :
- Pas d'avertissement SmartScreen au téléchargement ou à l'exécution
- Vérification d'intégrité : toute modification du binaire invalide la signature
- Horodatage RFC 3161 : la signature reste valide après expiration du certificat

Pour vérifier la signature :
```powershell
Get-AuthenticodeSignature mirageia.exe
```

Les binaires macOS et Linux ne sont pas signés (hors périmètre pour cette phase).

### Script d'installation automatique

Un script `install.sh` détecte l'OS et l'architecture, télécharge le bon binaire :

```bash
curl -sSf https://raw.githubusercontent.com/ctardy/mirageia/main/install.sh | sh
```

Le script :
1. Détecte OS (Windows/macOS/Linux) et architecture (x86_64/aarch64)
2. Télécharge le binaire depuis GitHub Releases
3. Le place dans `~/.local/bin/` (Linux/macOS) ou `%LOCALAPPDATA%\MirageIA` (Windows)
4. Vérifie l'installation (`mirageia --version`)
5. Crée le répertoire `~/.mirageia/` s'il n'existe pas

---

## Premier lancement — Expérience utilisateur

### Configuration guidée : `mirageia setup`

L'assistant interactif guide le développeur à travers toute la configuration :

```bash
mirageia setup
```

L'assistant :
1. **Détecte l'OS** (Windows/macOS/Linux) et le shell (bash/zsh/PowerShell/Git Bash)
2. **Demande le port** d'écoute (défaut : 3100)
3. **Propose les providers LLM** (Anthropic, OpenAI, Gemini, Mistral) — auto-détecte les clés API existantes
4. **Propose une whitelist** de termes à ne jamais pseudonymiser
5. **Génère `~/.mirageia/config.toml`**
6. **Configure le shell** automatiquement (ajoute les `export` dans `.bashrc` / `.zshrc`)

Au premier lancement sans setup, MirageIA affiche :
```
Première utilisation ? Lancez `mirageia setup` pour la configuration guidée.
```

Le proxy démarre quand même avec les défauts — le setup n'est pas obligatoire.

---

## Phase 4 — Installeur Tauri

Quand le dashboard Tauri sera implémenté, on bénéficiera des installeurs natifs générés par Tauri :

### Windows — MSI + NSIS

```
MirageIA-0.2.0-x86_64-setup.exe     (NSIS, ~15 Mo)
MirageIA-0.2.0-x86_64.msi           (MSI, ~15 Mo)
```

L'installeur :
- Installe dans `C:\Program Files\MirageIA\`
- Crée un raccourci dans le menu démarrer
- Ajoute le tray icon au démarrage automatique
- Configure le PATH

### macOS — DMG + .app

```
MirageIA-0.2.0-aarch64.dmg          (~15 Mo)
```

L'installeur :
- Bundle `.app` glisser-déposer dans Applications
- Tray icon dans la barre de menus
- Auto-start configurable via les préférences système

### Linux — AppImage + .deb

```
mirageia-0.2.0-amd64.AppImage       (~15 Mo)
mirageia-0.2.0-amd64.deb            (~10 Mo)
```

---

## Gestionnaires de paquets (futur)

### winget (Windows)

```powershell
winget install MirageIA
```

Nécessite un manifest dans le [winget-pkgs](https://github.com/microsoft/winget-pkgs) repo.

### Homebrew (macOS)

```bash
brew install mirageia
```

Nécessite un Homebrew tap ou une formule dans homebrew-core.

### cargo install (multi-plateforme)

```bash
cargo install mirageia
```

Publie le crate sur [crates.io](https://crates.io). Nécessite Rust installé côté utilisateur — moins pratique mais familier pour les développeurs Rust.

---

## CI/CD — Build automatisé

### GitHub Actions pour les releases

```yaml
# .github/workflows/release.yml
name: Release
on:
  push:
    tags: ['v*']

jobs:
  build:
    strategy:
      matrix:
        include:
          - os: windows-latest
            target: x86_64-pc-windows-msvc
            artifact: mirageia-windows-x86_64.zip
          - os: macos-latest
            target: aarch64-apple-darwin
            artifact: mirageia-macos-aarch64.tar.gz
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            artifact: mirageia-linux-x86_64.tar.gz

    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - run: cargo build --release --target ${{ matrix.target }}
      - name: Package
        run: |
          # Créer l'archive avec le binaire + config.example.toml
          ...
      - uses: softprops/action-gh-release@v2
        with:
          files: ${{ matrix.artifact }}
```

### Matrice de build

| Plateforme | Target | Testée | Notes |
|---|---|---|---|
| Windows x86_64 | `x86_64-pc-windows-msvc` | ⚠️ | Nécessite MSVC complet en CI |
| Windows x86_64 | `x86_64-pc-windows-gnu` | ✅ | Build actuel (pas d'ONNX) |
| macOS ARM | `aarch64-apple-darwin` | 📋 | À tester |
| macOS Intel | `x86_64-apple-darwin` | 📋 | À tester |
| Linux x86_64 | `x86_64-unknown-linux-gnu` | 📋 | À tester |

---

## Mise à jour

### Mise à jour automatique (intégrée)

MirageIA intègre un système de mise à jour automatique transparent :

```
Démarrage du proxy
       │
       ├── 1. Vérifier s'il y a un binaire stagé dans ~/.mirageia/staging/
       │      → Si oui : swap avec le binaire courant, message à l'utilisateur
       │
       ├── 2. Démarrer le proxy normalement
       │
       └── 3. En arrière-plan (après 5s) :
              → Vérifier la dernière version sur GitHub Releases
              → Si nouvelle version : télécharger dans ~/.mirageia/staging/
              → Sera appliquée au prochain démarrage
```

**L'utilisateur ne voit rien** — au prochain redémarrage, la nouvelle version est déjà là.

### Commande manuelle

```bash
# Vérifier si une mise à jour est disponible
mirageia update --check

# Vérifier, télécharger et appliquer immédiatement
mirageia update
```

### Mécanisme de swap

1. Le nouveau binaire est téléchargé dans `~/.mirageia/staging/`
2. Au démarrage, le binaire courant est renommé en `.old`
3. Le binaire stagé est copié à l'emplacement courant
4. `.old` et staging/ sont nettoyés
5. En cas d'erreur : rollback automatique vers `.old`

### Script d'installation (première installation ou forcer la dernière version)

```bash
curl -sSf https://raw.githubusercontent.com/ctardy/mirageia/main/install.sh | sh
```

### cargo install

```bash
cargo install mirageia --force
```

---

## Taille des binaires (estimations)

| Configuration | Taille estimée |
|---|---|
| CLI seul (sans ONNX) | ~5–8 Mo |
| CLI + ONNX Runtime | ~30–40 Mo |
| Tauri (avec webview, sans ONNX) | ~15–20 Mo |
| Tauri + ONNX | ~50–60 Mo |
| Modèle PII (DistilBERT INT8) | ~260 Mo (téléchargé séparément) |

Le modèle ONNX est téléchargé au premier lancement dans `~/.mirageia/models/`, pas inclus dans le binaire.

---

## Fichier de config exemple

À inclure dans chaque release :

```toml
# ~/.mirageia/config.toml
# Configuration MirageIA — toutes les options sont optionnelles

[proxy]
# listen_addr = "127.0.0.1:3100"    # Adresse d'écoute du proxy
# anthropic_base_url = "https://api.anthropic.com"
# openai_base_url = "https://api.openai.com"
# log_level = "info"                 # debug, info, warn, error
# add_header = false                 # Ajouter X-MirageIA: active
# fail_open = true                   # Passthrough si erreur

[detection]
# confidence_threshold = 0.75        # Seuil de confiance (0.0–1.0)
# whitelist = [                      # Termes à ne jamais pseudonymiser
#     "localhost",
#     "127.0.0.1",
#     "example.com",
# ]
```
