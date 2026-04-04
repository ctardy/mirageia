# Proxy HTTP

## Rôle

Le proxy HTTP est le point d'entrée de MirageIA. Il intercepte toutes les requêtes destinées aux API LLM, les fait passer par le pipeline de pseudonymisation, puis transmet la requête nettoyée à l'API réelle.

## Configuration côté client

L'application (Claude Code, etc.) doit être configurée pour utiliser le proxy :

```bash
# Claude Code — variable d'environnement
export ANTHROPIC_BASE_URL=http://localhost:3100

# OpenAI SDK
export OPENAI_BASE_URL=http://localhost:3100
```

Le proxy détermine le provider cible à partir du path :
- `/v1/messages` → Anthropic (`api.anthropic.com`)
- `/v1/chat/completions` → OpenAI (`api.openai.com`)

## Endpoints interceptés

### Anthropic
| Endpoint | Méthode | Streaming |
|----------|---------|-----------|
| `/v1/messages` | POST | Oui (SSE) |

### OpenAI
| Endpoint | Méthode | Streaming |
|----------|---------|-----------|
| `/v1/chat/completions` | POST | Oui (SSE) |

### Endpoints internes MirageIA
| Endpoint | Méthode | Description |
|----------|---------|-------------|
| `/health` | GET | État du proxy : `{"status":"ok","passthrough":false,"pii_mappings":0}` |
| `/events` | GET | Flux SSE temps réel des requêtes (pour `mirageia console`) |

## Mode passthrough

Le proxy peut relayer les requêtes **sans pseudonymiser**, utile pour le debug ou la désactivation temporaire :

```bash
mirageia proxy --passthrough        # Flag CLI
MIRAGEIA_PASSTHROUGH=1 mirageia     # Variable d'environnement
```

Ou dans `config.toml` :
```toml
[proxy]
passthrough = true
```

En mode passthrough, les requêtes sont transmises telles quelles à l'API. Les événements sont quand même émis sur `/events` (marqués `passthrough: true`).

## Gestion du streaming SSE

Les API LLM utilisent le Server-Sent Events pour streamer les réponses token par token. Le proxy doit :

1. **Requête** : pseudonymiser le body complet avant envoi (pas de streaming sur la requête)
2. **Réponse** : 
   - Bufferiser les tokens entrants
   - Détecter quand un pseudonyme complet a été reçu
   - Remplacer et transmettre au client
   - Flush le buffer régulièrement pour ne pas introduire trop de latence

### Stratégie de buffer (réponse streaming)

```
Tokens reçus:  "Le" "nom" " de" " l'" "util" "isat" "eur" " est" " Ger" "ard"
                                                                    ^^^^^^^^^^^
Buffer:        accumule "Ger" → "Gerard" reconnu → remplace par "Tardy" → flush
```

- Le buffer maintient les N derniers tokens (N = longueur max d'un pseudonyme)
- Quand un pseudonyme est reconnu, il est remplacé et flushé
- Les tokens non-ambigus sont flushés immédiatement

## Headers

- Les headers d'authentification (`x-api-key`, `Authorization: Bearer`) sont transmis tels quels au provider
- MirageIA ajoute un header `X-MirageIA: active` pour traçabilité (optionnel, désactivable)
- Le `Content-Length` est recalculé après pseudonymisation

## Commandes CLI

| Commande | Description |
|----------|-------------|
| `mirageia` | Lancer le proxy (comportement par défaut) |
| `mirageia proxy --passthrough` | Lancer en mode passthrough |
| `mirageia setup` | Assistant de configuration interactif |
| `mirageia wrap -- <cmd>` | Lancer une commande avec le proxy activé (activation par session) |
| `mirageia console` | Afficher les requêtes en temps réel (se connecte au flux `/events`) |
| `mirageia detect <texte>` | Détecter les PII dans un texte (nécessite `--features onnx`) |

### `mirageia wrap`

Lance un processus enfant avec `ANTHROPIC_BASE_URL` et `OPENAI_BASE_URL` pointant vers le proxy. Vérifie d'abord que le proxy est actif via `/health`.

```bash
# Lancer Claude Code protégé par MirageIA
mirageia wrap -- claude

# Lancer un script Python protégé
mirageia wrap -- python app.py

# Spécifier un port différent
mirageia wrap --port 4200 -- claude
```

### `mirageia console`

Se connecte au endpoint `/events` SSE du proxy et affiche les événements formatés :

```
  [14:32:01] → PII  Anthropic  /v1/messages (3 PII détectées)
  [14:32:02] ← PII  Anthropic  /v1/messages
  [14:35:10] → PASS OpenAI     /v1/chat/completions
  [14:35:11] ← PASS OpenAI     /v1/chat/completions
```

## Stack technique

- **Rust** : `axum` pour le serveur HTTP
- **reqwest** : client HTTP pour appeler les API en amont
- **tokio** : runtime async + broadcast channel pour les événements
- **async-stream** : génération de flux SSE pour `/events`
- **chrono** : horodatage des événements
