# Flux de pseudonymisation

## Pipeline complet

```
Requête entrante
       │
       ▼
┌──────────────────┐
│ 1. Parse requête │  Extraire le contenu textuel (messages, system prompt)
└──────────────────┘
       │
       ▼
┌──────────────────┐
│ 2. Tokenisation  │  Préparer le texte pour le modèle ONNX
└──────────────────┘
       │
       ▼
┌──────────────────┐
│ 3. Détection PII │  Modèle ONNX → liste d'entités avec positions et types
└──────────────────┘  Ex: [{text: "Tardy", type: "PERSON", start: 42, end: 47}]
       │
       ▼
┌──────────────────┐
│ 4. Génération    │  Pour chaque entité, générer un pseudonyme cohérent :
│    pseudonymes   │  - Nom → autre nom (même origine culturelle si possible)
└──────────────────┘  - IP → autre IP (même sous-réseau fictif)
       │              - Email → autre email (même domaine fictif)
       ▼
┌──────────────────┐
│ 5. Mapping       │  Stocker {id, original, pseudonyme, type, session}
└──────────────────┘  Chiffré AES-256-GCM en mémoire
       │
       ▼
┌──────────────────┐
│ 6. Remplacement  │  Substituer dans le texte (positions décroissantes pour garder les offsets)
└──────────────────┘
       │
       ▼
  Requête nettoyée → API LLM
```

## Dé-pseudonymisation (réponse)

```
Réponse API LLM
       │
       ▼
┌──────────────────┐
│ 1. Scan réponse  │  Rechercher tous les pseudonymes connus dans le texte
└──────────────────┘
       │
       ▼
┌──────────────────┐
│ 2. Remplacement  │  Remplacer chaque pseudonyme par la valeur originale
│    inverse       │  via la table de mapping
└──────────────────┘
       │
       ▼
  Réponse restaurée → Application
```

## Cas particuliers

### Streaming SSE
- Les réponses LLM arrivent token par token
- Le dé-pseudonymiseur maintient un buffer pour détecter les pseudonymes multi-tokens
- Ex: si le pseudonyme est "Gerard", il peut arriver en "Ger" + "ard" → buffer nécessaire

### Pseudonymes multi-mots
- "Jean-Pierre Dupont" → "Michel Martin" (le mapping porte sur l'entité complète)
- La dé-pseudonymisation doit gérer les variantes (initiales, troncatures par le LLM)

### Cohérence de session
- Même donnée = même pseudonyme dans toute la conversation
- "Tardy" sera toujours remplacé par "Gerard" dans la même session
- Entre sessions, les pseudonymes changent (pas de persistance)

### Faux positifs
- Le modèle peut détecter un faux positif (ex: un nom de variable qui ressemble à un nom)
- L'utilisateur peut configurer des exclusions (whitelist)
- Le dashboard affiche les détections pour vérification manuelle

## Génération de pseudonymes par type

| Type PII | Stratégie de remplacement |
|----------|---------------------------|
| Nom de personne | Nom fictif (dictionnaire intégré) |
| Adresse IP v4 | IP dans un range fictif (ex: 10.0.x.x) |
| Adresse IP v6 | IP v6 fictive |
| Email | `{prenom}@example.com` |
| Téléphone | Numéro fictif (format préservé) |
| IBAN | IBAN fictif (checksum valide) |
| Carte bancaire | Numéro fictif (Luhn valide) |
| Adresse postale | Adresse fictive (même pays) |
| Clé API / token | Hash tronqué aléatoire |
| URL interne | `https://internal.example.com/...` |
| Chemin de fichier | Chemin générique (`/home/user/...`) |
