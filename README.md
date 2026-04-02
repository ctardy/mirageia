# MirageIA

**Proxy de pseudonymisation intelligent pour API LLM avec modèle embarqué.**

L'API ne voit jamais vos vraies données — elle voit un mirage.

```
Votre app  ──►  MirageIA (proxy local)  ──►  API LLM (Anthropic / OpenAI)
                 │                              │
                 ├─ Détecte les PII (LLM ONNX)  │
                 ├─ Pseudonymise avant envoi     │
                 └─ Restaure dans la réponse  ◄──┘
```

## Le problème

Quand vous utilisez Claude, ChatGPT ou tout autre LLM via API, vos données transitent en clair vers des serveurs externes : noms, emails, adresses IP, clés API, numéros de téléphone… Ces données sensibles sont exposées sans que vous le sachiez.

## La solution

MirageIA s'intercale entre votre application et l'API LLM. Il détecte automatiquement les données sensibles, les remplace par des valeurs fictives cohérentes, et restaure les originaux dans la réponse.

| Donnée originale | Ce que l'API reçoit | Ce que vous recevez |
|---|---|---|
| `user.lastName = "Tardy"` | `user.lastName = "Gerard"` | `"Tardy"` (restauré) |
| `192.168.1.22` | `10.0.42.7` | `192.168.1.22` (restauré) |
| `chris@mondomaine.fr` | `paul@example.com` | `chris@mondomaine.fr` (restauré) |

Le LLM travaille avec des données fictives mais cohérentes — sa réponse est tout aussi pertinente, et vos données n'ont jamais quitté votre machine.

## Différenciateurs

| | MirageIA | Concurrents |
|---|---|---|
| **Détection** | LLM embarqué (ONNX) — comprend le contexte | Regex / NER classique |
| **Intelligence** | Ne masque pas "Thomas Edison" dans un cours d'histoire | Masque tout aveuglément |
| **Architecture** | Binaire unique, zéro dépendance | Python + Docker + serveur externe |
| **Réversibilité** | Pseudonymes cohérents avec mapping chiffré | `[REDACTED]` — information perdue |
| **Streaming** | Compatible SSE natif | Rarement supporté |
| **Sécurité** | Mapping AES-256-GCM en mémoire, jamais persisté | Souvent en clair |

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                 MirageIA (processus unique)              │
│                                                         │
│  ┌──────────┐    ┌──────────────┐    ┌───────────────┐  │
│  │  Proxy   │───▶│  Détecteur   │───▶│ Pseudonymiseur│  │
│  │  HTTP    │    │  PII (ONNX)  │    │               │  │
│  │  (axum)  │◀───│              │◀───│  Mapping table│  │
│  └──────────┘    └──────────────┘    └───────────────┘  │
│       ▲                                     │           │
│       │              ┌──────────┐           │           │
│       └──────────────│ Dashboard│───────────┘           │
│                      │ (Tauri)  │                       │
│                      └──────────┘                       │
└─────────────────────────────────────────────────────────┘
        ▲                                     │
        │ requête originale        requête nettoyée
        │                                     ▼
  ┌───────────┐                      ┌──────────────┐
  │ Claude    │                      │ API Anthropic│
  │ Code, etc.│                      │ / OpenAI     │
  └───────────┘                      └──────────────┘
```

## Fonctionnement

1. Votre application envoie une requête à `localhost:3100` au lieu de l'API directe
2. MirageIA extrait le contenu textuel des messages
3. Le modèle ONNX embarqué détecte les PII avec compréhension du contexte
4. Chaque PII est remplacée par un pseudonyme cohérent (même type de donnée)
5. Le mapping `{original ↔ pseudonyme}` est stocké chiffré en mémoire (AES-256-GCM)
6. La requête nettoyée est envoyée à l'API LLM
7. La réponse est interceptée et les pseudonymes sont remplacés par les originaux
8. Votre application reçoit la réponse restaurée — transparence totale

## Types de PII détectés

- Noms de personnes, prénoms, pseudonymes
- Adresses email
- Adresses IP (v4, v6)
- Numéros de téléphone
- Adresses postales
- Numéros de carte bancaire, IBAN
- Identifiants (numéro de sécu, passeport, etc.)
- Clés API, tokens, secrets
- URLs internes / noms de domaines privés
- Noms de serveurs, chemins de fichiers sensibles

## Stack technique

| Composant | Technologie |
|-----------|-------------|
| Runtime | Rust + Tauri (binaire unique, cross-platform) |
| Modèle PII | ONNX Runtime (DistilBERT-PII ou Qwen3 0.6B quantifié) |
| Proxy HTTP | axum / hyper |
| Mapping | En mémoire, chiffré AES-256-GCM, non persisté |
| Interface | Tray icon + dashboard local (Tauri webview) |
| Tests | cargo test + fixtures PII |

## Statut du projet

> En cours de développement — phase de conception et prototypage.

## Documentation

| Sujet | Lien |
|-------|------|
| Architecture globale | [`docs/officiel/architecture/vue-ensemble.md`](docs/officiel/architecture/vue-ensemble.md) |
| Flux de pseudonymisation | [`docs/officiel/architecture/flux-pseudonymisation.md`](docs/officiel/architecture/flux-pseudonymisation.md) |
| Modèle PII embarqué | [`docs/officiel/technique/modele-pii.md`](docs/officiel/technique/modele-pii.md) |
| Proxy HTTP | [`docs/officiel/technique/proxy-http.md`](docs/officiel/technique/proxy-http.md) |
| État de l'art / concurrents | [`docs/recherche/etat-de-lart.md`](docs/recherche/etat-de-lart.md) |
| Tickets | [`docs/tickets/`](docs/tickets/) |
| Index documentation | [`docs/README.md`](docs/README.md) |

## Licence

À définir.
