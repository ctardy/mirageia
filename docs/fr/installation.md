# Guide d'installation — MirageIA

## Installation rapide

### Option A — Binaire précompilé (recommandé)

Téléchargez le binaire pour votre OS depuis [GitHub Releases](https://github.com/ctardy/mirageia/releases/latest) :

```bash
# Linux / macOS
curl -sSfL https://github.com/ctardy/mirageia/releases/latest/download/mirageia-linux-x86_64.tar.gz \
  | tar xz -C ~/.local/bin/

# Vérifier
mirageia --version
```

**Windows — via Scoop (recommandé)**

```powershell
# 1. Ajouter le bucket MirageIA (obligatoire avant l'installation)
scoop bucket add mirageia https://github.com/ctardy/scoop-mirageia
# 2. Installer
scoop install mirageia
# 3. Vérifier
mirageia --version
```

> **`couldn't find manifest for 'mirageia'` ?** L'étape 1 a été ignorée. Lancez d'abord `scoop bucket add mirageia https://github.com/ctardy/scoop-mirageia`, puis réessayez.

Scoop installe le binaire et configure le PATH automatiquement. Pas de blocage Windows SmartScreen.

**Windows — manuel** (sans Scoop) : téléchargez `mirageia-windows-x86_64.zip` depuis la page [Releases](https://github.com/ctardy/mirageia/releases/latest) et extrayez `mirageia.exe` dans un dossier de votre PATH.

### Option B — Depuis les sources

Si vous avez Rust installé :

```bash
git clone https://github.com/ctardy/mirageia.git
cd mirageia
cargo build --release
# Le binaire est dans target/release/mirageia(.exe)
```

### Premiers pas

```bash
# 1. Lancer l'assistant de configuration
mirageia setup

# 2. Démarrer le proxy
mirageia
```

L'assistant `mirageia setup` vous guide pas à pas : choix du port, sélection des providers LLM, whitelist, configuration automatique du shell. Voir la section [Configuration guidée](#configuration-guidée) ci-dessous.

---

## Prérequis

| Outil | Version | Obligatoire | Notes |
|---|---|---|---|
| **Rust** | 1.75+ | Oui | Installé via [rustup](https://rustup.rs/) |
| **GCC** (Windows) | 15+ | Oui (Windows GNU) | Via MSYS2 (`mingw-w64-x86_64-gcc`) |
| **Git** | 2.x | Oui | Pour cloner le dépôt |

### Systèmes supportés

| OS | Toolchain | Status |
|---|---|---|
| Windows 11 | `stable-x86_64-pc-windows-gnu` + MSYS2 | ✅ Testé |
| Windows 11 | `stable-x86_64-pc-windows-msvc` | ⚠️ Nécessite VS Build Tools complet |
| macOS | `stable-aarch64-apple-darwin` | 📋 Non testé |
| Linux | `stable-x86_64-unknown-linux-gnu` | 📋 Non testé |

---

## Installation pas à pas (Windows)

### 1. Installer Rust

```bash
# Depuis un terminal (Git Bash, PowerShell, etc.)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

# Vérifier l'installation
rustc --version    # rustc 1.94.1 ou plus récent
cargo --version
```

### 2. Installer MSYS2 et GCC

MirageIA utilise la toolchain GNU sur Windows. Il faut installer GCC via MSYS2.

```bash
# Installer MSYS2 via winget
winget install -e --id MSYS2.MSYS2

# Installer GCC dans MSYS2
/c/msys64/usr/bin/bash.exe -lc "pacman -S --noconfirm mingw-w64-x86_64-gcc"
```

### 3. Configurer la toolchain Rust

```bash
rustup default stable-x86_64-pc-windows-gnu
```

### 4. Configurer le PATH

Ajoutez ces chemins à votre PATH (dans `.bashrc` ou `.bash_profile` pour Git Bash) :

```bash
export PATH="/c/msys64/mingw64/bin:$HOME/.cargo/bin:$PATH"
```

Vérification :

```bash
gcc --version     # gcc.exe (Rev8, Built by MSYS2 project) 15.x
cargo --version   # cargo 1.94.x
```

### 5. Cloner et compiler

```bash
git clone <repo-url>
cd mirageia

# Build en mode développement
cargo build

# Build en mode release (optimisé)
cargo build --release
```

Le binaire se trouve dans `target/release/mirageia.exe` (ou `target/debug/mirageia.exe`).

### 6. Vérifier que tout fonctionne

```bash
# Lancer les tests
cargo test

# Résultat attendu : 144 tests passent, 0 échec
```

---

## Configuration guidée

### L'assistant `mirageia setup`

Au lieu de configurer manuellement, lancez l'assistant interactif :

```bash
mirageia setup
```

L'assistant vous guide à travers 6 étapes :

| Étape | Question | Ce qui est fait |
|---|---|---|
| 1 | — (automatique) | Détecte l'OS (Windows/macOS/Linux) et le shell (bash/zsh/PowerShell) |
| 2 | Port d'écoute ? | Défaut `3100`, modifiable |
| 3 | Quels providers LLM ? | Multi-sélection : Anthropic, OpenAI, Gemini, Mistral. Auto-détecte les clés API déjà configurées |
| 4 | Whitelist ? | Termes à ne jamais pseudonymiser (optionnel) |
| 5 | — (automatique) | Génère `~/.mirageia/config.toml` |
| 6 | Configurer le shell ? | Propose d'ajouter les `export` dans `.bashrc` / `.zshrc` |

Exemple de session :

```
  Système détecté : Windows (Git Bash)

? Port d'écoute du proxy [3100] : 3100

? Quels providers LLM utilisez-vous ?
  >[x] Anthropic (Claude) ✓ clé API détectée
   [ ] OpenAI (GPT)
   [ ] Google Gemini
   [ ] Mistral AI

? Ajouter des termes à ne jamais pseudonymiser ? [o/N] : o
  Whitelist : Thomas Edison, Martin Fowler

  ✓ Configuration écrite dans ~/.mirageia/config.toml

? Ajouter automatiquement à ~/.bashrc ? [O/n] : O
  ✓ ~/.bashrc mis à jour

  Configuration terminée !
    Proxy     : http://127.0.0.1:3100
    Providers : Anthropic (Claude)
    Shell     : ✓ configuré

  Pour démarrer : mirageia
```

---

## Premier lancement

```bash
mirageia
```

Ce qui se passe :
1. Charge `~/.mirageia/config.toml` (créé par `setup` ou manuellement)
2. Démarre le proxy sur le port configuré
3. Affiche dans le terminal :

```
INFO  MirageIA v0.1.0
INFO  MirageIA proxy écoute sur 127.0.0.1:3100
```

Si c'est la première utilisation sans avoir lancé `setup` :
```
Première utilisation ? Lancez `mirageia setup` pour la configuration guidée.
```

Le proxy démarre quand même avec les défauts — le setup n'est pas obligatoire.

### Activation par session (recommandé)

Plutôt que de configurer le proxy globalement dans votre shell, utilisez `mirageia wrap` pour activer le proxy uniquement sur une session donnée :

```bash
# Terminal 1 — Lancer le proxy
mirageia

# Terminal 2 — Lancer Claude Code via le proxy
mirageia wrap -- claude
```

`wrap` vérifie que le proxy tourne, puis lance la commande avec `ANTHROPIC_BASE_URL` et `OPENAI_BASE_URL` pointant vers le proxy. Quand la commande se termine, les variables d'environnement disparaissent.

Avantage : si le proxy est arrêté, les apps lancées normalement (`claude`) fonctionnent toujours directement vers l'API.

### Monitoring en temps réel

```bash
# Dans un terminal séparé
mirageia console
```

Affiche chaque requête qui transite par le proxy, avec le nombre de PII détectées :
```
  [14:32:01] → PII  Anthropic  /v1/messages (3 PII détectées)
  [14:32:02] ← PII  Anthropic  /v1/messages
```

### Mode passthrough (désactivation temporaire)

Pour relayer les requêtes **sans pseudonymiser** (debug, test de performance) :

```bash
# Par flag CLI
mirageia proxy --passthrough

# Par variable d'environnement
MIRAGEIA_PASSTHROUGH=1 mirageia

# Par config.toml
# [proxy]
# passthrough = true
```

Le health check indique le mode actif :
```bash
curl http://localhost:3100/health
# → {"status":"ok","passthrough":true,"pii_mappings":0}
```

### Vérification

```bash
# Health check
curl http://localhost:3100/health
# → {"status":"ok","passthrough":false,"pii_mappings":0}

# Test avec des PII (nécessite une clé API)
curl -X POST http://localhost:3100/v1/messages \
  -H "x-api-key: $ANTHROPIC_API_KEY" \
  -H "content-type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  -d '{
    "model": "claude-sonnet-4-20250514",
    "max_tokens": 100,
    "messages": [{"role":"user","content":"Mon email est jean@acme.fr et l IP est 192.168.1.50"}]
  }'
```

Dans les logs MirageIA :
```
INFO  PII détectées dans la requête pii_count=2
INFO  Requête pseudonymisée provider=Anthropic mappings=2
```

---

## Configuration manuelle (alternative au setup)

Si vous préférez configurer à la main :

```bash
mkdir -p ~/.mirageia
cp config.example.toml ~/.mirageia/config.toml
# Éditer le fichier selon vos besoins
```

Puis configurez votre shell :

```bash
# Pour Anthropic (Claude Code, SDK)
export ANTHROPIC_BASE_URL=http://localhost:3100

# Pour OpenAI
export OPENAI_BASE_URL=http://localhost:3100
```

MirageIA route automatiquement :
- `/v1/messages` → Anthropic
- `/v1/chat/completions` → OpenAI

Voir le [README principal](../../README.md) pour la liste complète des options.

---

## Résolution de problèmes

### `cargo: command not found`

Le PATH de Rust n'est pas configuré. Ajoutez :
```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

### `gcc.exe: program not found` (Windows)

MSYS2/GCC n'est pas dans le PATH :
```bash
export PATH="/c/msys64/mingw64/bin:$PATH"
```

### `ort does not provide prebuilt binaries for x86_64-pc-windows-gnu`

La feature ONNX n'est pas supportée avec la toolchain GNU. Deux solutions :
1. Utiliser le détecteur regex (par défaut, pas besoin d'ONNX)
2. Basculer vers la toolchain MSVC (`rustup default stable-x86_64-pc-windows-msvc`) avec Visual Studio Build Tools complet

### `LINK : fatal error LNK1104: cannot open file 'msvcrt.lib'`

Visual Studio n'a pas les composants C++ desktop installés. Installer via le VS Installer :
- Composant : "MSVC v143 - VS 2022 C++ x64/x86 build tools"
- Composant : "Windows 11 SDK"

### Les tests échouent

```bash
# Nettoyer le build et réessayer
cargo clean
cargo test
```

---

## Désinstallation

```bash
# Supprimer le binaire
cargo clean

# Supprimer la config
rm -rf ~/.mirageia

# Supprimer Rust (si souhaité)
rustup self uninstall
```
