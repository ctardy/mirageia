# MirageIA

Proxy de pseudonymisation intelligent pour API LLM avec modèle embarqué.

## Concept

MirageIA intercepte les requêtes vers les API LLM (Anthropic, OpenAI), détecte les données sensibles (PII) via un modèle de langage **embarqué** (ONNX Runtime), les pseudonymise avant envoi, puis réinjecte les valeurs originales dans les réponses.

**L'API ne voit jamais vos vraies données — elle voit un mirage.**

```
Votre code :  user.lastName = "Tardy"        →  API reçoit :  user.lastName = "Gerard"
Votre IP :    192.168.1.22                    →  API reçoit :  192.168.1.223
Votre email : chris@mondomaine.fr             →  API reçoit :  paul@example.com
```

La réponse de l'API est automatiquement restaurée avec les vraies valeurs.

## Différenciateurs

- **LLM embarqué** : le modèle tourne dans le processus via ONNX Runtime — pas de serveur Ollama ni service externe
- **Détection contextuelle** : comprend la sémantique (ne masque pas "Thomas Edison" dans un cours d'histoire)
- **Pseudonymisation réversible** : mapping bidirectionnel avec IDs, pas de simple `[REDACTED]`
- **Un seul binaire** : zéro dépendance, cross-platform (Windows, macOS, Linux)
- **Streaming** : compatible SSE (Server-Sent Events) pour les réponses en flux

## Stack

- **Rust + Tauri** : binaire unique, cross-platform
- **ONNX Runtime** : inférence du modèle PII embarqué
- **axum/hyper** : proxy HTTP
- **AES-256-GCM** : chiffrement du mapping en mémoire

## Documentation

Voir [docs/README.md](docs/README.md) pour la documentation complète.

## Licence

À définir.
