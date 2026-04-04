# Architecture — Vue d'ensemble

## Schéma global

```
┌─────────────────────────────────────────────────────────┐
│                    MirageIA (processus unique)           │
│                                                         │
│  ┌──────────┐    ┌──────────────┐    ┌───────────────┐  │
│  │  Proxy   │───▶│  Détecteur   │───▶│ Pseudonymiseur│  │
│  │  HTTP    │    │  PII (ONNX)  │    │               │  │
│  │          │◀───│              │◀───│  Mapping table │  │
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

## Composants principaux

### 1. Proxy HTTP
- Écoute sur un port local (ex: `localhost:3100`)
- Intercepte les requêtes vers `api.anthropic.com` et `api.openai.com`
- Supporte le streaming SSE (Server-Sent Events) pour les réponses en flux
- L'application cliente est configurée pour pointer vers le proxy au lieu de l'API directe
- Gestion transparente des headers d'authentification (API keys passées telles quelles)

### 2. Détecteur PII (modèle ONNX embarqué)
- Modèle de langage embarqué directement dans le binaire via ONNX Runtime
- Pas de serveur externe (Ollama, etc.) — tout tourne dans le processus
- Détection contextuelle : comprend la sémantique, pas juste du pattern matching
- Modèle cible : DistilBERT-PII (~260 Mo) ou Qwen3 0.6B quantifié (~400 Mo)
- Latence cible : < 50ms par requête

### 3. Pseudonymiseur + Table de mapping
- Remplace chaque PII détectée par une valeur fictive cohérente (même type de donnée)
- Attribue un ID unique à chaque remplacement
- Table de mapping en mémoire, chiffrée AES-256-GCM
- Mapping déterministe par session : même entrée = même pseudonyme dans la conversation
- Dé-pseudonymisation dans les réponses : recherche des pseudonymes et réinjection des originaux

### 4. Dashboard (Tauri webview)
- Tray icon discret (barre des tâches)
- Vue en temps réel des PII détectées et pseudonymisées
- Statistiques de session (nombre de remplacements, types de PII)
- Configuration (providers supportés, types de PII à détecter, exclusions)

## Flux de données détaillé

1. **Requête entrante** : l'application envoie une requête à `localhost:3100/v1/messages`
2. **Extraction du contenu** : le proxy extrait le texte des messages (user, system, assistant)
3. **Détection PII** : le modèle ONNX analyse le texte et retourne les entités détectées avec leur position
4. **Pseudonymisation** : chaque entité est remplacée par une valeur fictive, le mapping est stocké
5. **Envoi** : la requête nettoyée est transmise à l'API réelle
6. **Réponse** : la réponse de l'API est interceptée
7. **Dé-pseudonymisation** : les pseudonymes présents dans la réponse sont remplacés par les originaux
8. **Retour** : la réponse restaurée est renvoyée à l'application

## Contraintes techniques

- **Un seul binaire** : pas d'installation de dépendances externes
- **Cross-platform** : Windows, macOS, Linux
- **Performance** : latence ajoutée < 100ms (détection + remplacement)
- **Mémoire** : empreinte < 1 Go (modèle + runtime + mapping)
- **Sécurité** : mapping jamais persisté sur disque, jamais loggé, chiffré en mémoire
