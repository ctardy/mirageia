# CLAUDE.md — Contexte pour Claude Code

## Règles absolues (priorité maximale)

- **Langue** : toujours écrire en français avec les accents (é, è, ê, à, ù, ç, etc.) dans les messages, commentaires et documentation
- **Chemins shell** : toujours des chemins absolus dans les commandes proposées (ex: `/c/dev/projects/mirageia`, jamais `./`)
- **Git pull avec changements en cours** : `git stash && git pull --rebase && git stash pop` — ne jamais faire `git pull --rebase` directement
- **Git commit prudent** : vérifier `git status` et `git diff --cached` avant de commiter. Ne jamais `git add .` ou `git add -A`
- **⛔ No rétro-compatibilité** : pas de `@Deprecated`, pas de fallback inutile. Le projet est en phase de construction
- **Ne jamais modifier sans demande** : ne jamais éditer de fichiers sans demande explicite de l'utilisateur
- **LLM embarqué, pas de serveur** : le modèle tourne via ONNX Runtime dans le processus — jamais de dépendance vers Ollama, LM Studio ou un serveur externe
- **Zéro donnée sensible en clair** : le mapping de pseudonymisation reste 100% local, jamais persisté en clair sur disque, jamais loggé

---

## Description du projet

**MirageIA** — Proxy de pseudonymisation intelligent pour API LLM avec modèle embarqué.

Intercepte les requêtes vers les API LLM (Anthropic, OpenAI), détecte les données sensibles (PII) via un modèle de langage embarqué (ONNX Runtime), les pseudonymise avant envoi, puis réinjecte les valeurs originales dans les réponses.

### Principe de fonctionnement

```
Application (Claude Code, etc.)
       ↓ requête
[MirageIA — processus unique]
  ├── Modèle ONNX embarqué (détection PII contextuelle)
  ├── Pseudonymisation (remplacement par valeurs fictives + mapping ID)
  ├── Table de mapping en mémoire (chiffrée AES-256)
       ↓ requête nettoyée
API LLM (Anthropic / OpenAI)
       ↓ réponse
[MirageIA]
  ├── Dé-pseudonymisation (réinjection des valeurs originales via mapping ID)
       ↓ réponse restaurée
Application
```

### Exemple concret

| Donnée originale | Envoyé à l'API | ID mapping |
|------------------|----------------|------------|
| `192.168.1.22` | `192.168.1.223` | 458 |
| `Tardy` | `Gerard` | 253 |
| `chris@example.com` | `paul@example.com` | 254 |

L'API LLM ne voit jamais les vraies données. La réponse contient `Gerard` → MirageIA le remplace par `Tardy` avant de renvoyer à l'application.

### Différenciateurs

- **LLM embarqué** : pas de serveur Ollama ni service externe — un seul binaire autonome (comme Murmure embarque Whisper)
- **Détection contextuelle** : le modèle comprend le contexte (ne masque pas "Thomas Edison" dans un cours d'histoire)
- **Pseudonymisation réversible** : mapping bidirectionnel avec IDs, pas de simple masquage `[REDACTED]`
- **Streaming SSE** : compatible avec le streaming des réponses LLM
- **Zéro config** : fonctionne out-of-the-box, le proxy se place entre l'app et l'API

### Types de PII détectés

- Noms de personnes, prénoms, pseudonymes
- Adresses IP (v4, v6)
- Adresses e-mail
- Numéros de téléphone
- Adresses postales
- Numéros de carte bancaire, IBAN
- Identifiants (numéro de sécu, passeport, etc.)
- Clés API, tokens, secrets
- URLs internes / noms de domaines privés
- Noms de serveurs, chemins de fichiers sensibles

---

## Stack technique (cible)

| Composant | Techno |
|-----------|--------|
| Runtime | Rust + Tauri (binaire unique, cross-platform) |
| Modèle PII | ONNX Runtime (DistilBERT-PII ou Qwen3 0.6B quantifié) |
| Proxy HTTP | Interception man-in-the-middle (hyper / axum) |
| Mapping | En mémoire, chiffré AES-256-GCM, non persisté |
| Interface | Tray icon + dashboard local minimal (Tauri webview) |
| Tests | cargo test + fixtures PII |

---

## Commandes de build

```bash
# À définir quand la stack sera choisie
cd /c/dev/projects/mirageia
```

---

## Documentation

| Sujet | Documentation |
|-------|---------------|
| Architecture globale | `docs/officiel/architecture/vue-ensemble.md` |
| Flux de pseudonymisation | `docs/officiel/architecture/flux-pseudonymisation.md` |
| Modèle PII embarqué | `docs/officiel/technique/modele-pii.md` |
| Proxy HTTP | `docs/officiel/technique/proxy-http.md` |
| Recherche et état de l'art | `docs/recherche/etat-de-lart.md` |
| Tickets | `docs/tickets/` |
| Index complet | `docs/README.md` |
