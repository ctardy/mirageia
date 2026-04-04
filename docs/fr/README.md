# MirageIA

> **[Read in English](../../README.md)**

**Proxy de pseudonymisation intelligent pour API LLM.**

L'API ne voit jamais vos vraies données — elle voit un mirage.

```
Votre app  -->  MirageIA (proxy local :3100)  -->  API LLM (Anthropic / OpenAI)
                 │                                    │
                 ├─ Détecte les PII (regex + ONNX)    │
                 ├─ Pseudonymise avant envoi           │
                 └─ Restaure dans la réponse  <────────┘
```

## Le problème

Quand vous utilisez Claude, ChatGPT ou tout autre LLM via API, vos données transitent en clair vers des serveurs externes : noms, emails, adresses IP, clés API, numéros de téléphone... Ces données sensibles sont exposées sans que vous le sachiez.

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

### Installation

Téléchargez le binaire pour votre OS depuis [GitHub Releases](https://github.com/ctardy/mirageia/releases/latest) :

```bash
# Linux / macOS
curl -sSfL https://github.com/ctardy/mirageia/releases/latest/download/mirageia-linux-x86_64.tar.gz \
  | tar xz -C ~/.local/bin/

# Ou depuis les sources (nécessite Rust 1.75+)
git clone https://github.com/ctardy/mirageia.git && cd mirageia && cargo build --release
```

Sur Windows, téléchargez `mirageia-windows-x86_64.zip` depuis la page [Releases](https://github.com/ctardy/mirageia/releases/latest).

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

# Dashboard web
# Ouvrir http://localhost:3100/dashboard dans le navigateur
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

---

## Documentation

| Sujet | Lien |
|-------|------|
| **Installation** | [installation.md](installation.md) |
| **Déploiement ops (Docker + Apache)** | [deploiement-ops.md](deploiement-ops.md) |
| **Distribution & installeur** | [distribution.md](distribution.md) |
| **Contribution** | [contribution.md](contribution.md) |
| Architecture globale | [architecture/vue-ensemble.md](architecture/vue-ensemble.md) |
| Architecture détaillée | [architecture/architecture-detaillee.md](architecture/architecture-detaillee.md) |
| Flux de pseudonymisation | [architecture/flux-pseudonymisation.md](architecture/flux-pseudonymisation.md) |
| Modèle PII embarqué | [technique/modele-pii.md](technique/modele-pii.md) |
| Proxy HTTP | [technique/proxy-http.md](technique/proxy-http.md) |
| Analyse sécurité RSSI | [securite/analyse-rssi.md](securite/analyse-rssi.md) |

---

## Licence

MIT
