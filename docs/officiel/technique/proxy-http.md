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

## Stack technique

- **Rust** : `axum` ou `hyper` pour le serveur HTTP
- **reqwest** : client HTTP pour appeler les API en amont
- **tokio** : runtime async
- **eventsource-stream** : parsing SSE pour le streaming
