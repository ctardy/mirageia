# Architecture détaillée — MirageIA

> Document de référence décrivant le fonctionnement précis de chaque composant, leurs interactions, et les décisions techniques associées.

---

## Table des matières

1. [Vue d'ensemble](#1-vue-densemble)
2. [Cycle de vie d'une requête](#2-cycle-de-vie-dune-requête)
3. [Composant 1 — Proxy HTTP](#3-composant-1--proxy-http)
4. [Composant 2 — Détecteur PII](#4-composant-2--détecteur-pii)
5. [Composant 3 — Pseudonymiseur](#5-composant-3--pseudonymiseur)
6. [Composant 4 — Table de mapping](#6-composant-4--table-de-mapping)
7. [Composant 5 — Dé-pseudonymiseur](#7-composant-5--dé-pseudonymiseur)
8. [Composant 6 — Dashboard](#8-composant-6--dashboard)
9. [Gestion du streaming SSE](#9-gestion-du-streaming-sse)
10. [Sécurité et chiffrement](#10-sécurité-et-chiffrement)
11. [Gestion des erreurs](#11-gestion-des-erreurs)
12. [Contraintes de performance](#12-contraintes-de-performance)
13. [Structure des modules Rust](#13-structure-des-modules-rust)

---

## 1. Vue d'ensemble

MirageIA est un **processus unique** qui contient tous les composants suivants :

```
┌──────────────────────────────────────────────────────────────────────────┐
│                        MirageIA (processus unique)                      │
│                                                                         │
│  ┌───────────┐   ┌─────────────┐   ┌────────────────┐   ┌───────────┐  │
│  │   Proxy   │──▶│  Détecteur  │──▶│ Pseudonymiseur │──▶│  Client   │  │
│  │   HTTP    │   │  PII (ONNX) │   │                │   │  HTTP     │  │
│  │  (axum)   │◀──│             │◀──│  Mapping table │◀──│ (reqwest) │  │
│  └───────────┘   └─────────────┘   └────────────────┘   └───────────┘  │
│       ▲  │            │                    │                   │  ▲     │
│       │  │       ┌────┘                    │                   │  │     │
│       │  │       ▼                         ▼                   │  │     │
│       │  │  ┌──────────┐          ┌──────────────┐             │  │     │
│       │  └─▶│ Dé-pseu- │          │  Événements  │             │  │     │
│       │     │ donymiseur│         │  (dashboard)  │             │  │     │
│       │     └──────────┘          └──────────────┘             │  │     │
│       │          │                       │                     │  │     │
│       │          ▼                       ▼                     │  │     │
│       │   ┌──────────┐          ┌──────────────┐              │  │     │
│       │   │ Buffer   │          │  Dashboard   │              │  │     │
│       │   │ streaming│          │  (Tauri)     │              │  │     │
│       │   └──────────┘          └──────────────┘              │  │     │
│       │                                                       │  │     │
└───────┼───────────────────────────────────────────────────────┼──┼─────┘
        │                                                       │  │
   requête                                              requête │  │ réponse
   originale                                           nettoyée │  │ brute
        ▲                                                       ▼  │
  ┌───────────┐                                        ┌──────────────┐
  │ Application│                                       │ API Anthropic│
  │ (Claude    │                                       │ / OpenAI     │
  │  Code, etc)│                                       └──────────────┘
  └───────────┘
```

### Principe fondamental

MirageIA est un **proxy man-in-the-middle bienveillant** : il s'interpose entre l'application cliente et l'API LLM. L'application ne sait pas que ses données sont pseudonymisées, l'API ne sait pas que les données sont fictives. La transparence est totale des deux côtés.

---

## 2. Cycle de vie d'une requête

### 2.1 Flux aller (requête)

```
 ① Réception          ② Extraction         ③ Détection PII
────────────────    ────────────────────   ─────────────────────
POST /v1/messages   Parse JSON body        Modèle ONNX analyse
Headers copiés      Extraire les champs    le texte et retourne
Body lu en entier   textuels des messages  les entités détectées
                    (content, system)      [{text, type, pos}]

 ④ Pseudonymisation   ⑤ Reconstruction     ⑥ Envoi
─────────────────────  ──────────────────   ──────────────────
Chaque entité PII      Remplacer dans le    Requête nettoyée
→ pseudonyme généré    body JSON les PII    envoyée à l'API
→ stocké dans mapping  par les pseudonymes  via reqwest
                       Recalculer offsets   Headers auth passés
                       + Content-Length     tels quels
```

### 2.2 Flux retour (réponse)

```
 ⑦ Réception réponse   ⑧ Dé-pseudonymisation  ⑨ Retour client
─────────────────────  ──────────────────────  ──────────────────
Réponse de l'API       Scanner le texte pour   Réponse restaurée
(complète ou SSE)      trouver les pseudos     renvoyée à l'app
                       connus dans le mapping  Le client reçoit
                       Remplacer par les       les vraies données
                       valeurs originales
```

### 2.3 Diagramme de séquence

```
Application          MirageIA                    API LLM
    │                    │                           │
    │─── POST /v1/msg ──▶│                           │
    │   "Mon nom est     │                           │
    │    Tardy, IP       │                           │
    │    192.168.1.22"   │                           │
    │                    │                           │
    │                    │── Détection PII ──┐       │
    │                    │   ONNX Runtime    │       │
    │                    │◀─────────────────┘       │
    │                    │  [{Tardy, PERSON, 12-17}  │
    │                    │   {192.168.1.22, IP, ...}] │
    │                    │                           │
    │                    │── Pseudonymisation ─┐     │
    │                    │   Tardy → Gerard     │     │
    │                    │   192.168.1.22       │     │
    │                    │     → 10.0.42.7      │     │
    │                    │◀────────────────────┘     │
    │                    │                           │
    │                    │──── POST /v1/messages ───▶│
    │                    │  "Mon nom est Gerard,     │
    │                    │   IP 10.0.42.7"           │
    │                    │                           │
    │                    │◀──── Réponse ────────────│
    │                    │  "Bonjour Gerard,         │
    │                    │   votre IP 10.0.42.7..."  │
    │                    │                           │
    │                    │── Dé-pseudonymisation ─┐  │
    │                    │   Gerard → Tardy        │  │
    │                    │   10.0.42.7             │  │
    │                    │     → 192.168.1.22      │  │
    │                    │◀──────────────────────┘  │
    │                    │                           │
    │◀── Réponse ───────│                           │
    │  "Bonjour Tardy,   │                           │
    │   votre IP         │                           │
    │   192.168.1.22..." │                           │
    │                    │                           │
```

---

## 3. Composant 1 — Proxy HTTP

### 3.1 Rôle

Point d'entrée unique. Écoute sur `localhost:3100` et intercepte les requêtes destinées aux API LLM.

### 3.2 Routage par provider

Le proxy détermine le provider cible à partir du path de la requête :

| Path reçu | Provider cible | URL upstream |
|---|---|---|
| `/v1/messages` | Anthropic | `https://api.anthropic.com/v1/messages` |
| `/v1/chat/completions` | OpenAI | `https://api.openai.com/v1/chat/completions` |
| Tout autre path | Passthrough | Forwarding direct sans pseudonymisation |

### 3.3 Traitement des headers

```
Headers entrants (application)
    │
    ├── x-api-key / Authorization: Bearer  →  Transmis tels quels à l'API
    ├── Content-Type                        →  Conservé (application/json)
    ├── Content-Length                      →  Recalculé après pseudonymisation
    ├── anthropic-version                  →  Transmis tel quel
    └── Accept: text/event-stream          →  Indicateur de mode streaming
```

Le proxy ajoute optionnellement `X-MirageIA: active` à la réponse (désactivable en configuration).

### 3.4 Extraction du contenu textuel

Le proxy parse le body JSON et extrait les champs textuels à analyser selon le provider :

**Anthropic** (`/v1/messages`) :
```json
{
  "system": "Tu es un assistant...",        ← analysé
  "messages": [
    {
      "role": "user",
      "content": "Mon nom est Tardy..."     ← analysé
    },
    {
      "role": "assistant",
      "content": "Bonjour Tardy..."         ← analysé
    }
  ]
}
```

**OpenAI** (`/v1/chat/completions`) :
```json
{
  "messages": [
    {
      "role": "system",
      "content": "Tu es un assistant..."    ← analysé
    },
    {
      "role": "user",
      "content": "Mon nom est Tardy..."     ← analysé
    }
  ]
}
```

Les champs non-textuels (`model`, `max_tokens`, `temperature`, `tools`, etc.) ne sont **jamais** modifiés.

### 3.5 Gestion du content multipart

Les messages Anthropic supportent le contenu multipart (texte + images) :

```json
{
  "content": [
    {"type": "text", "text": "Analyse cette image..."},    ← analysé
    {"type": "image", "source": {"type": "base64", ...}}   ← ignoré
  ]
}
```

Seuls les blocs `{"type": "text"}` sont analysés. Les images, fichiers et autres types binaires sont transmis sans modification.

### 3.6 Stack technique

| Crate | Rôle |
|---|---|
| `axum` | Serveur HTTP async (routes, middleware) |
| `reqwest` | Client HTTP pour appeler l'API upstream |
| `tokio` | Runtime async (io, timers, channels) |
| `serde_json` | Parsing/sérialisation JSON |
| `eventsource-stream` | Parsing du flux SSE (réponses streaming) |

---

## 4. Composant 2 — Détecteur PII

### 4.1 Rôle

Analyse le texte extrait et retourne une liste d'entités PII avec leur position, type et score de confiance.

### 4.2 Pipeline de détection

```
Texte brut
    │
    ▼
┌────────────────────┐
│ Pré-traitement     │  Normalisation Unicode, découpage en segments
└────────────────────┘  si le texte dépasse la fenêtre du modèle (512 tokens)
    │
    ▼
┌────────────────────┐
│ Tokenisation       │  Tokenizer HuggingFace (crate `tokenizers`)
└────────────────────┘  Texte → token IDs + attention mask
    │
    ▼
┌────────────────────┐
│ Inférence ONNX     │  Modèle chargé via `ort` (ONNX Runtime Rust)
└────────────────────┘  Entrée : token IDs → Sortie : logits par token
    │
    ▼
┌────────────────────┐
│ Post-traitement    │  Décodage BIO/BILOU → entités avec positions
└────────────────────┘  Fusion des sous-tokens (##ard → Tardy)
    │                   Filtrage par score de confiance (seuil configurable)
    ▼
Liste d'entités PII
```

### 4.3 Format de sortie du détecteur

```rust
struct PiiEntity {
    text: String,           // "Tardy"
    entity_type: PiiType,   // PiiType::PersonName
    start: usize,           // position début dans le texte original
    end: usize,             // position fin dans le texte original
    confidence: f32,        // 0.0 — 1.0
}

enum PiiType {
    PersonName,       // Noms, prénoms, pseudonymes
    Email,            // Adresses email
    IpAddress,        // IPv4, IPv6
    PhoneNumber,      // Numéros de téléphone
    PostalAddress,    // Adresses postales
    CreditCard,       // Numéros de carte bancaire
    Iban,             // Numéros IBAN
    NationalId,       // Numéro de sécu, passeport, etc.
    ApiKey,           // Clés API, tokens, secrets
    InternalUrl,      // URLs internes / domaines privés
    ServerName,       // Noms de serveurs
    FilePath,         // Chemins de fichiers sensibles
}
```

### 4.4 Gestion des textes longs

Le modèle a une fenêtre de contexte limitée (512 tokens pour DistilBERT). Pour les textes plus longs :

1. **Découpage en segments** avec chevauchement de 64 tokens
2. **Inférence sur chaque segment** indépendamment
3. **Fusion des résultats** : dédoublonnage des entités dans les zones de chevauchement (garder celle avec le meilleur score de confiance)

```
Texte de 1500 tokens :

Segment 1 : tokens   0–511   ──▶ inférence ──▶ entités
Segment 2 : tokens 448–959   ──▶ inférence ──▶ entités
Segment 3 : tokens 896–1500  ──▶ inférence ──▶ entités
                     ▲
               chevauchement
               de 64 tokens

Fusion : dédoublonner les entités dans les zones 448–511 et 896–959
```

### 4.5 Seuil de confiance

- **Seuil par défaut** : 0.75
- Les entités sous le seuil sont ignorées (pas pseudonymisées)
- Le seuil est **configurable par type de PII** pour ajuster la sensibilité :
  - Clés API, secrets : seuil bas (0.5) → mieux vaut un faux positif
  - Noms de personnes : seuil standard (0.75) → éviter de pseudonymiser "Thomas Edison"

### 4.6 Modèle ONNX — chargement

```
Démarrage de MirageIA
    │
    ├── Vérifier ~/.mirageia/models/{modèle}.onnx
    │   ├── Existe → charger en mémoire via ort
    │   └── N'existe pas → télécharger depuis GitHub Release
    │                       → sauvegarder dans ~/.mirageia/models/
    │                       → charger en mémoire
    │
    └── Modèle chargé → session ONNX prête
        (temps de chargement cible : < 3 secondes)
```

---

## 5. Composant 3 — Pseudonymiseur

### 5.1 Rôle

Reçoit la liste des entités PII détectées et génère un pseudonyme cohérent pour chacune.

### 5.2 Stratégies de remplacement par type

| Type PII | Stratégie | Exemple |
|---|---|---|
| `PersonName` | Nom fictif depuis un dictionnaire intégré | Tardy → Gerard |
| `Email` | `{prénom_fictif}@example.com` | chris@dom.fr → paul@example.com |
| `IpAddress` (v4) | IP dans le range 10.0.0.0/8 | 192.168.1.22 → 10.0.42.7 |
| `IpAddress` (v6) | IPv6 fictive dans fd00::/8 | fe80::1 → fd00::a1b2:c3d4 |
| `PhoneNumber` | Numéro fictif, format préservé | 06 12 34 56 78 → 06 98 76 54 32 |
| `PostalAddress` | Adresse fictive, même pays | 12 rue X, Paris → 8 av Y, Lyon |
| `CreditCard` | Numéro fictif (Luhn valide) | 4532... → 4111... |
| `Iban` | IBAN fictif (checksum valide) | FR76... → FR14... |
| `NationalId` | ID fictif, même format | 1 85 07... → 2 91 03... |
| `ApiKey` | Hash aléatoire tronqué, même longueur | sk-abc123... → sk-xyz789... |
| `InternalUrl` | `https://internal.example.com/...` | srv.corp.local → internal.example.com |
| `FilePath` | Chemin générique | /home/chris/... → /home/user/... |

### 5.3 Cohérence de session

Dans une même session (conversation), **la même donnée produit toujours le même pseudonyme** :

```
Message 1 : "Contactez Tardy à chris@dom.fr"
             → "Contactez Gerard à paul@example.com"

Message 5 : "Tardy a confirmé par email"
             → "Gerard a confirmé par email"
                ^^^^^^
                même pseudonyme car même session
```

La cohérence est assurée par un lookup dans la table de mapping **avant** de générer un nouveau pseudonyme.

### 5.4 Remplacement dans le texte

Les remplacements sont effectués en **ordre décroissant de position** pour préserver les offsets :

```
Texte : "Contactez Tardy (chris@dom.fr) pour le projet"
                   ^^^^^  ^^^^^^^^^^^^^^
                   pos 10  pos 17

Remplacement en ordre décroissant :
  1. pos 17–30 : chris@dom.fr → paul@example.com
  2. pos 10–15 : Tardy → Gerard

Résultat : "Contactez Gerard (paul@example.com) pour le projet"
```

Si on remplaçait dans l'ordre croissant, le remplacement de "Tardy" (5 → 6 chars) décalerait la position de l'email.

### 5.5 Pseudonymes de longueur variable

Quand un pseudonyme a une longueur différente de l'original, tous les offsets dans le JSON sont recalculés. Le body JSON est reconstruit après tous les remplacements, pas modifié in-place.

---

## 6. Composant 4 — Table de mapping

### 6.1 Rôle

Stocke la correspondance bidirectionnelle entre les valeurs originales et leurs pseudonymes. Permet la dé-pseudonymisation dans les réponses.

### 6.2 Structure

```rust
struct MappingEntry {
    id: u64,                // identifiant unique
    original: String,       // "Tardy"
    pseudonym: String,      // "Gerard"
    pii_type: PiiType,      // PiiType::PersonName
    created_at: Instant,    // timestamp de création
}

struct MappingTable {
    // Lookup rapide dans les deux sens
    by_original: HashMap<String, MappingEntry>,   // original → entry
    by_pseudonym: HashMap<String, MappingEntry>,  // pseudonyme → entry
    cipher: Aes256Gcm,                            // clé de chiffrement
}
```

### 6.3 Cycle de vie

```
Démarrage MirageIA
    │
    ├── Génération d'une clé AES-256 aléatoire (en mémoire uniquement)
    ├── Initialisation de la table vide
    │
    │   Pour chaque requête :
    ├── Lookup : la PII existe déjà ? → retourner le pseudonyme existant
    ├── Sinon : générer un pseudonyme, chiffrer, stocker
    │
    │   Pour chaque réponse :
    ├── Lookup inverse : le pseudonyme existe ? → retourner l'original
    │
    │   Fin de session :
    └── Table détruite avec la mémoire du processus
        (jamais persistée sur disque)
```

### 6.4 Invariants de sécurité

- La clé AES-256 est générée aléatoirement à chaque démarrage
- Les valeurs originales sont chiffrées en mémoire (pas stockées en clair)
- La table n'est **jamais** écrite sur disque (pas de fichier, pas de base de données)
- La table n'est **jamais** loggée (aucun log ne contient de valeur originale ni de mapping)
- À l'arrêt du processus, la mémoire est libérée et les données sont perdues

---

## 7. Composant 5 — Dé-pseudonymiseur

### 7.1 Rôle

Scanne les réponses de l'API LLM pour trouver les pseudonymes connus et les remplacer par les valeurs originales.

### 7.2 Algorithme (réponse complète)

```
Réponse API (texte)
    │
    ▼
Pour chaque entrée dans mapping.by_pseudonym :
    │
    ├── Rechercher le pseudonyme dans le texte de la réponse
    │   (recherche exacte, case-sensitive)
    │
    ├── Si trouvé :
    │   └── Remplacer par la valeur originale (déchiffrée)
    │
    └── Si non trouvé : continuer
    │
    ▼
Réponse restaurée
```

### 7.3 Cas particuliers de dé-pseudonymisation

**Variantes générées par le LLM** : le LLM peut transformer un pseudonyme de manière inattendue :

| Pseudonyme envoyé | Ce que le LLM peut répondre | Stratégie |
|---|---|---|
| `Gerard` | `Gerard` | Match exact → remplacement |
| `Gerard` | `M. Gerard` | Match partiel → remplacement de "Gerard" |
| `Gerard` | `GERARD` | Match case-insensitive (optionnel) |
| `Gerard` | `Gérard` (avec accent) | Normalisation Unicode avant comparaison |
| `paul@example.com` | `paul@example.com` | Match exact |
| `paul@example.com` | `paul` | Pas de match (trop ambigu) |

### 7.4 Priorité de remplacement

Les pseudonymes les plus longs sont remplacés en premier pour éviter les conflits :

```
Mapping : "Jean-Pierre Gerard" → "Jean-Pierre Tardy"
          "Gerard"             → "Tardy"

Texte : "Jean-Pierre Gerard a confirmé"

Ordre : d'abord "Jean-Pierre Gerard" (19 chars), puis "Gerard" (6 chars)
→ Évite de remplacer "Gerard" seul dans "Jean-Pierre Gerard"
```

---

## 8. Composant 6 — Dashboard

### 8.1 Rôle

Interface graphique locale minimale (tray icon + webview Tauri) pour superviser l'activité de MirageIA en temps réel.

### 8.2 Fonctionnalités

```
┌─────────────────────────────────────────────────────────┐
│  MirageIA Dashboard                          ─  □  ✕   │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  ● Proxy actif — localhost:3100            [Pause] [■]  │
│                                                         │
│  ┌─ Session en cours ─────────────────────────────────┐ │
│  │ Requêtes traitées : 42                             │ │
│  │ PII détectées     : 127                            │ │
│  │ Types : 38 noms, 22 emails, 15 IPs, 52 autres     │ │
│  └────────────────────────────────────────────────────┘ │
│                                                         │
│  ┌─ Dernières détections ─────────────────────────────┐ │
│  │ 14:32:01  PERSON     "████" → "Gerard"     ✓ 0.92 │ │
│  │ 14:32:01  EMAIL      "████" → "paul@ex…"  ✓ 0.98 │ │
│  │ 14:32:01  IP_ADDR    "████" → "10.0.42.7" ✓ 0.95 │ │
│  │ 14:31:58  PERSON     "████" → "Martin"    ✓ 0.88 │ │
│  │ 14:31:58  API_KEY    "████" → "sk-xyz…"   ✓ 0.97 │ │
│  └────────────────────────────────────────────────────┘ │
│                                                         │
│  ┌─ Configuration ────────────────────────────────────┐ │
│  │ Seuil de confiance : [0.75] ◄──────────►          │ │
│  │ Types actifs : ☑ Noms ☑ Emails ☑ IPs ☑ Clés API  │ │
│  │ Exclusions : [thomas edison, localhost, ...]       │ │
│  └────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

### 8.3 Sécurité du dashboard

- Les valeurs originales ne sont **jamais** affichées dans le dashboard (masquées par `████`)
- Seuls les pseudonymes, types et scores sont visibles
- Le dashboard est accessible uniquement en local (pas d'exposition réseau)

### 8.4 Communication proxy ↔ dashboard

Le proxy émet des événements vers le dashboard via un canal interne (Tauri events) :

```rust
enum DashboardEvent {
    PiiDetected {
        pii_type: PiiType,
        pseudonym: String,      // pas l'original
        confidence: f32,
        timestamp: Instant,
    },
    RequestProcessed {
        provider: Provider,
        pii_count: usize,
        latency_ms: u64,
    },
    ProxyStatusChanged {
        status: ProxyStatus,    // Active, Paused, Error
    },
}
```

---

## 9. Gestion du streaming SSE

### 9.1 Le défi

Les API LLM envoient les réponses token par token via Server-Sent Events. Un pseudonyme peut être découpé entre plusieurs tokens :

```
Tokens reçus par l'API : "Le" " nom" " est" " Ger" "ard" "."
                                              ^^^^  ^^^
                                        Le pseudonyme "Gerard" est
                                        coupé entre deux tokens
```

### 9.2 Architecture du buffer streaming

```
Flux SSE entrant (token par token)
    │
    ▼
┌────────────────────────────────────┐
│         Buffer circulaire          │
│                                    │
│  Taille = longueur max des        │
│  pseudonymes dans le mapping       │
│                                    │
│  Contenu : "...est Ger"           │
│                     ^^^            │
│              pas encore flushé     │
│              (préfixe potentiel    │
│               d'un pseudonyme)     │
└────────────────────────────────────┘
    │
    ├── Nouveau token "ard" arrive
    │   Buffer = "...est Gerard"
    │                   ^^^^^^^
    │   "Gerard" reconnu dans le mapping !
    │
    ├── Remplacer "Gerard" → "Tardy"
    ├── Flusher "...est Tardy" vers le client
    └── Vider le buffer
```

### 9.3 Algorithme détaillé

```
Pour chaque token SSE reçu :
    1. Ajouter le token au buffer
    2. Vérifier si le buffer contient un pseudonyme complet
       → Si oui : remplacer et flusher
    3. Vérifier si le buffer se termine par un préfixe de pseudonyme connu
       → Si oui : attendre le prochain token (le pseudonyme est peut-être en cours)
       → Si non : flusher le début du buffer (pas un pseudonyme)
    4. Si le buffer dépasse la taille max : forcer le flush du début
```

### 9.4 Latence ajoutée par le buffer

- **Cas nominal** (pas de pseudonyme dans la réponse) : latence quasi-nulle, les tokens sont flushés immédiatement
- **Cas pseudonyme** : latence = temps de recevoir tous les tokens du pseudonyme (typiquement 2-4 tokens, soit 50-200ms)
- **Taille max du buffer** : configurable, par défaut = longueur du plus long pseudonyme + marge

### 9.5 Format SSE

Le proxy reconstruit les événements SSE dans le même format que l'API :

```
data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"Tardy"}}

data: [DONE]
```

Les champs `event:`, `id:` et `retry:` sont transmis tels quels.

---

## 10. Sécurité et chiffrement

### 10.1 Modèle de menace

MirageIA protège contre :
- **Fuite de données vers les API LLM** : les PII ne quittent jamais la machine
- **Lecture du mapping en mémoire** : chiffrement AES-256-GCM

MirageIA ne protège **pas** contre :
- Un attaquant avec accès root à la machine (dump mémoire possible)
- L'interception entre l'app et le proxy local (localhost, risque négligeable)

### 10.2 Chiffrement du mapping

```
Valeur originale ("Tardy")
    │
    ▼
┌─────────────────────────────┐
│ AES-256-GCM                │
│ Clé : générée aléatoirement │  ← 256 bits, jamais persistée
│       à chaque démarrage    │
│ Nonce : unique par entrée   │  ← 96 bits, aléatoire
│ AAD : type PII + ID         │  ← données additionnelles authentifiées
└─────────────────────────────┘
    │
    ▼
Valeur chiffrée (stockée dans la table)
```

### 10.3 Ce qui n'est jamais exposé

| Donnée | En mémoire | Sur disque | Dans les logs | Dans le dashboard |
|---|---|---|---|---|
| Valeur originale | Chiffrée (AES-256) | Jamais | Jamais | Jamais |
| Pseudonyme | En clair | Jamais | Optionnel | Oui |
| Clé AES | En clair | Jamais | Jamais | Jamais |
| Mapping complet | Chiffré | Jamais | Jamais | Jamais |

---

## 11. Gestion des erreurs

### 11.1 Principe : fail-open vs fail-closed

MirageIA adopte une approche **fail-open** : en cas d'erreur dans le pipeline de pseudonymisation, la requête est transmise **telle quelle** à l'API plutôt que de bloquer l'utilisateur.

Justification : MirageIA est un outil de protection optionnel, pas un pare-feu. Bloquer le flux de travail de l'utilisateur est pire que de laisser passer une requête non pseudonymisée ponctuellement.

### 11.2 Scénarios d'erreur

| Erreur | Comportement | Notification |
|---|---|---|
| Modèle ONNX non chargé | Passthrough (requête transmise telle quelle) | Dashboard : avertissement |
| Inférence ONNX échoue | Passthrough pour ce message | Dashboard : avertissement |
| API upstream injoignable | Retourner l'erreur HTTP au client | Dashboard : erreur |
| JSON malformé dans la requête | Passthrough (pas de parsing) | Dashboard : avertissement |
| Buffer SSE overflow | Flush forcé sans remplacement | Dashboard : avertissement |
| Déchiffrement mapping échoue | Ignorer l'entrée de mapping | Log d'erreur interne |

### 11.3 Logs

- **Aucun log ne contient de données originales** (PII)
- Les logs contiennent : timestamps, types de PII détectées, scores, compteurs, erreurs
- Niveau de log configurable (error, warn, info, debug)
- Les logs sont écrits sur stderr (stdout est réservé au proxy)

---

## 12. Contraintes de performance

### 12.1 Objectifs

| Métrique | Cible | Justification |
|---|---|---|
| Latence ajoutée (non-streaming) | < 100ms | Imperceptible pour l'utilisateur |
| Latence ajoutée (streaming) | < 50ms par chunk | Pas de saccade visible |
| Temps de démarrage | < 5s | Dont ~3s pour charger le modèle ONNX |
| Mémoire au repos | < 200 Mo | Modèle chargé, pas de requête en cours |
| Mémoire en charge | < 800 Mo | Modèle + mapping + buffers |
| Taille binaire (sans modèle) | < 30 Mo | Téléchargement rapide |
| Taille modèle (DistilBERT INT8) | ~260 Mo | Téléchargé au 1er lancement |

### 12.2 Optimisations prévues

- **Cache du tokenizer** : les tokens sont réutilisés si le même texte est soumis
- **Inférence batch** : si plusieurs messages dans une requête, les analyser en un seul batch ONNX
- **Lookup de mapping O(1)** : HashMap pour les deux sens (original → pseudo, pseudo → original)
- **Remplacement sans allocation** : pré-allouer le buffer de sortie à la taille estimée

---

## 13. Structure des modules Rust

```
src-tauri/
├── src/
│   ├── main.rs                  Point d'entrée, initialisation Tauri + proxy
│   │
│   ├── proxy/
│   │   ├── mod.rs               Module proxy public
│   │   ├── server.rs            Serveur axum (routes, middleware)
│   │   ├── router.rs            Routage par provider (Anthropic / OpenAI)
│   │   ├── extractor.rs         Extraction du contenu textuel depuis le JSON
│   │   └── client.rs            Client HTTP reqwest (appels upstream)
│   │
│   ├── detection/
│   │   ├── mod.rs               Module détection public
│   │   ├── model.rs             Chargement et inférence ONNX (crate ort)
│   │   ├── tokenizer.rs         Tokenisation (crate tokenizers)
│   │   ├── postprocess.rs       Post-traitement (BIO → entités, fusion sous-tokens)
│   │   └── types.rs             PiiEntity, PiiType, seuils de confiance
│   │
│   ├── pseudonymization/
│   │   ├── mod.rs               Module pseudonymisation public
│   │   ├── generator.rs         Génération de pseudonymes par type
│   │   ├── replacer.rs          Remplacement dans le texte (gestion des offsets)
│   │   ├── depseudonymizer.rs   Dé-pseudonymisation des réponses
│   │   └── dictionaries.rs      Dictionnaires intégrés (noms, prénoms)
│   │
│   ├── mapping/
│   │   ├── mod.rs               Module mapping public
│   │   ├── table.rs             Table de mapping bidirectionnelle
│   │   └── crypto.rs            Chiffrement/déchiffrement AES-256-GCM
│   │
│   ├── streaming/
│   │   ├── mod.rs               Module streaming public
│   │   ├── buffer.rs            Buffer circulaire pour SSE
│   │   ├── sse_parser.rs        Parsing des événements SSE
│   │   └── sse_writer.rs        Reconstruction des événements SSE
│   │
│   ├── dashboard/
│   │   ├── mod.rs               Module dashboard public
│   │   ├── events.rs            Événements Tauri (PiiDetected, RequestProcessed)
│   │   └── state.rs             État partagé pour le dashboard
│   │
│   └── config/
│       ├── mod.rs               Module configuration public
│       └── settings.rs          Paramètres (port, seuils, exclusions, types actifs)
│
├── models/                      Modèles ONNX (gitignored, téléchargés au runtime)
├── dictionaries/                Dictionnaires de pseudonymes (embarqués dans le binaire)
│   ├── firstnames.json
│   ├── lastnames.json
│   └── addresses.json
├── Cargo.toml
└── tauri.conf.json
```

### 13.1 Dépendances Cargo.toml

| Crate | Version cible | Rôle |
|---|---|---|
| `tauri` | 2.x | Framework desktop (tray, webview, events) |
| `axum` | 0.7 | Serveur HTTP async |
| `reqwest` | 0.12 | Client HTTP |
| `tokio` | 1.x | Runtime async |
| `serde` / `serde_json` | 1.x | Sérialisation JSON |
| `ort` | 2.x | ONNX Runtime (bindings Rust) |
| `tokenizers` | 0.20 | Tokenisation HuggingFace |
| `aes-gcm` | 0.10 | Chiffrement AES-256-GCM |
| `rand` | 0.8 | Génération aléatoire (clés, pseudonymes) |
| `tracing` | 0.1 | Logging structuré |
| `eventsource-stream` | 0.2 | Parsing SSE |
