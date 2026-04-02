# Analyse de sécurité — Point de vue RSSI

> Ce document s'adresse aux Responsables de la Sécurité des Systèmes d'Information (RSSI) qui évaluent les risques liés à l'utilisation d'assistants IA basés sur des LLM (Claude Code, GitHub Copilot, ChatGPT, etc.) et les protections que MirageIA peut apporter.

---

## Table des matières

1. [Scénario de référence](#1-scénario-de-référence)
2. [Ce qui transite vers l'API LLM](#2-ce-qui-transite-vers-lapi-llm)
3. [Cartographie des risques pour le RSSI](#3-cartographie-des-risques-pour-le-rssi)
4. [Protections apportées par MirageIA](#4-protections-apportées-par-mirageia)
5. [Matrice risques × protections](#5-matrice-risques--protections)
6. [Limites et périmètre non couvert](#6-limites-et-périmètre-non-couvert)
7. [Conformité réglementaire](#7-conformité-réglementaire)
8. [Recommandations pour le RSSI](#8-recommandations-pour-le-rssi)

---

## 1. Scénario de référence

Prenons le cas concret le plus courant : un développeur installe **Claude Code** (CLI d'Anthropic) sur son poste de travail pour l'assister dans le développement logiciel.

### 1.1 Installation et accès local

```
Poste de travail du développeur
│
├── /c/dev/projects/mon-projet/        ← dossier de travail
│   ├── src/                           ← code source
│   ├── .env                           ← variables d'environnement (secrets !)
│   ├── config/database.yml            ← credentials base de données
│   ├── docker-compose.yml             ← infrastructure
│   ├── tests/fixtures/users.json      ← données de test (PII potentielles)
│   └── ...
│
├── Claude Code installé (CLI)
│   ├── Accès en lecture à TOUS les fichiers du projet
│   ├── Accès en écriture (avec confirmation utilisateur)
│   ├── Accès au terminal (exécution de commandes)
│   └── Accès à git (historique, diff, blame)
│
└── Clé API Anthropic configurée
    └── ANTHROPIC_API_KEY=sk-ant-...
```

**Point critique** : dès l'installation, Claude Code a potentiellement accès à **l'intégralité des fichiers** du projet, y compris ceux contenant des données sensibles.

### 1.2 Comment Claude Code interagit avec le développeur

```
Développeur                          Claude Code (local)
    │                                      │
    │── "Refactore la fonction login" ────▶│
    │                                      │
    │                                      │── Lit src/auth/login.ts
    │                                      │── Lit src/auth/middleware.ts
    │                                      │── Lit .env (pour comprendre le contexte)
    │                                      │── Lit config/database.yml
    │                                      │── Exécute git log (historique récent)
    │                                      │
    │                                      │── Construit un prompt contenant :
    │                                      │   • Le contenu des fichiers lus
    │                                      │   • Le contexte du projet
    │                                      │   • L'instruction de l'utilisateur
    │                                      │   • Les résultats de commandes
    │                                      │
    │                                      │══════════════════════════════════╗
    │                                      │  ENVOI À L'API ANTHROPIC        ║
    │                                      │  (via HTTPS vers le cloud)      ║
    │                                      │══════════════════════════════════╝
    │                                      │
```

---

## 2. Ce qui transite vers l'API LLM

### 2.1 Anatomie d'un appel API

Chaque interaction avec Claude Code génère un appel HTTP POST vers `api.anthropic.com/v1/messages`. Voici ce qui est envoyé :

```json
{
  "model": "claude-sonnet-4-20250514",
  "max_tokens": 8096,
  "system": "Tu es Claude Code, un assistant de développement...",
  "messages": [
    {
      "role": "user",
      "content": [
        {
          "type": "text",
          "text": "Voici le contenu du fichier src/auth/login.ts :\n\n
                   import { db } from '../config';\n
                   const DB_PASSWORD = 'P@ssw0rd_Pr0d!';\n
                   const API_SECRET = 'sk-secret-xyz123';\n
                   ...\n\n
                   Refactore cette fonction."
        }
      ]
    }
  ]
}
```

### 2.2 Types de données qui transitent

| Type de donnée | Comment ça arrive dans l'API | Fréquence | Risque |
|---|---|---|---|
| **Contenu de fichiers source** | Claude Code lit les fichiers et les inclut dans le prompt | Très fréquent | Moyen |
| **Fichiers de configuration** | `.env`, `database.yml`, `docker-compose.yml` | Fréquent | **Critique** |
| **Credentials en dur** | Mots de passe, clés API, tokens dans le code | Fréquent | **Critique** |
| **Données de test / fixtures** | `users.json`, dumps SQL, CSV avec des vraies données | Occasionnel | **Élevé** |
| **Historique git** | Commits, diffs, blames (peuvent contenir des PII) | Fréquent | Moyen |
| **Résultats de commandes** | Output de `npm test`, `docker ps`, logs d'erreur | Fréquent | Moyen |
| **Noms de personnes** | Dans le code, commentaires, git blame, données | Fréquent | Élevé |
| **Adresses email** | Dans le code, configs, données de test | Fréquent | Élevé |
| **Adresses IP / noms de serveurs** | Dans les configs, logs, scripts de déploiement | Fréquent | **Élevé** |
| **URLs internes** | Intranet, APIs internes, dashboards | Fréquent | Élevé |
| **Schémas de base de données** | Migrations, modèles ORM, requêtes SQL | Occasionnel | Moyen |
| **Clés SSH / certificats** | Si présents dans le dossier projet | Rare | **Critique** |
| **Images / captures d'écran** | Screenshots d'erreurs, maquettes UI | Occasionnel | Moyen |
| **Documents PDF / Office** | Documentation interne, specs | Rare | Élevé |
| **Binaires** | Non envoyés directement (Claude Code ne lit pas les binaires) | Jamais | — |

### 2.3 Ce que Claude Code envoie concrètement

#### A. Le prompt système (system)

Le prompt système contient les instructions globales de comportement, le contenu des fichiers `CLAUDE.md` du projet (qui peuvent contenir des informations sur l'architecture interne), et les instructions de l'utilisateur.

```
┌─ System prompt ──────────────────────────────────────────────┐
│ Instructions de comportement de Claude Code                   │
│ + Contenu de CLAUDE.md (conventions du projet)                │
│ + Contexte de l'environnement (OS, shell, git status)         │
│                                                               │
│ ⚠ Peut contenir : noms de serveurs, conventions internes,     │
│   noms de base de données, patterns d'architecture            │
└───────────────────────────────────────────────────────────────┘
```

#### B. Les messages utilisateur (content)

Chaque message contient l'instruction de l'utilisateur **plus** le contenu des fichiers que Claude Code a décidé de lire pour répondre :

```
┌─ Message utilisateur ────────────────────────────────────────┐
│                                                               │
│ "Refactore la fonction d'authentification"                     │
│                                                               │
│ + Résultat des outils utilisés par Claude Code :               │
│                                                               │
│   ┌─ Read(src/auth/login.ts) ──────────────────────────────┐ │
│   │ const DB_HOST = '192.168.1.50';                         │ │
│   │ const DB_PASSWORD = 'P@ssw0rd_Pr0d!';                   │ │
│   │ // Author: jean.dupont@entreprise.fr                     │ │
│   │ function login(email: string, password: string) { ... }  │ │
│   └─────────────────────────────────────────────────────────┘ │
│                                                               │
│   ┌─ Read(.env) ───────────────────────────────────────────┐ │
│   │ DATABASE_URL=postgres://admin:s3cret@db.internal:5432   │ │
│   │ STRIPE_SECRET_KEY=sk_live_4eC39HqLyjWDarjtT1zdp7dc     │ │
│   │ AWS_ACCESS_KEY_ID=AKIA...                                │ │
│   └─────────────────────────────────────────────────────────┘ │
│                                                               │
│   ┌─ Bash(git log --oneline -5) ───────────────────────────┐ │
│   │ a1b2c3d fix: login Pierre Martin (pierre@corp.fr)       │ │
│   │ d4e5f6g feat: ajout endpoint /api/users                  │ │
│   └─────────────────────────────────────────────────────────┘ │
│                                                               │
└───────────────────────────────────────────────────────────────┘
```

**Tout cela transite en HTTPS vers les serveurs d'Anthropic.**

#### C. L'historique de conversation

La conversation complète est renvoyée à chaque appel API. Si le développeur a 20 échanges dans une session, le 20ᵉ appel contient les 19 échanges précédents + le nouveau message. Le volume de données transmises **croît au fil de la conversation**.

```
Appel 1 : system + message_1                          →  ~5 Ko
Appel 2 : system + message_1 + réponse_1 + message_2  →  ~15 Ko
Appel 5 : system + tout l'historique + message_5       →  ~80 Ko
Appel 20 : system + tout l'historique + message_20     →  ~500 Ko+

Chaque appel contient TOUT ce qui a été échangé précédemment,
y compris les fichiers lus et les secrets exposés dans les messages antérieurs.
```

#### D. Images et captures d'écran

Claude Code supporte l'envoi d'images (screenshots d'erreurs, maquettes UI) encodées en base64 :

```json
{
  "type": "image",
  "source": {
    "type": "base64",
    "media_type": "image/png",
    "data": "/9j/4AAQSkZJRgABAQ..."
  }
}
```

Les images peuvent contenir des PII visibles (noms, emails, adresses dans des captures d'écran d'applications).

#### E. Ce qui ne transite PAS

| Donnée | Transmise ? | Raison |
|---|---|---|
| Fichiers binaires (`.exe`, `.dll`, `.so`) | Non | Claude Code ne lit pas les binaires |
| Fichiers hors du projet | Non (sauf demande explicite) | Scope limité au répertoire de travail |
| Keychain / gestionnaire de mots de passe | Non | Pas d'accès |
| Fichiers système (`/etc/shadow`, registre) | Non | Pas d'accès (sauf si root) |
| Trafic réseau local | Non | Pas de capture réseau |

### 2.4 Politique de rétention d'Anthropic

Selon la documentation d'Anthropic (à vérifier — les politiques évoluent) :

| Aspect | Politique API |
|---|---|
| Entraînement sur les données API | Non (les données API ne sont pas utilisées pour l'entraînement) |
| Rétention des requêtes | 30 jours (pour abus/sécurité), puis suppression |
| Localisation des serveurs | États-Unis (principalement) |
| Chiffrement en transit | TLS 1.2+ |
| Chiffrement au repos | Oui (infrastructure cloud) |
| Accès par les employés Anthropic | Limité, pour investigation de sécurité/abus |

**Point d'attention RSSI** : même si Anthropic n'utilise pas les données API pour l'entraînement, elles **transitent et sont stockées temporairement** sur des serveurs aux États-Unis. Pour une entreprise soumise au RGPD, c'est un transfert de données hors UE.

---

## 3. Cartographie des risques pour le RSSI

### 3.1 Risques identifiés

| # | Risque | Impact | Probabilité | Criticité |
|---|---|---|---|---|
| R1 | **Fuite de credentials** (clés API, mots de passe, tokens) dans les prompts | Critique | Élevée | 🔴 Critique |
| R2 | **Fuite de PII** (noms, emails, téléphones de clients/employés) | Élevé | Élevée | 🔴 Critique |
| R3 | **Exposition d'architecture interne** (IPs, noms de serveurs, schémas DB) | Élevé | Élevée | 🟠 Élevé |
| R4 | **Fuite de propriété intellectuelle** (algorithmes, logique métier) | Élevé | Moyenne | 🟠 Élevé |
| R5 | **Non-conformité RGPD** (transfert de données personnelles hors UE) | Élevé | Élevée | 🟠 Élevé |
| R6 | **Exposition de données de production** (fixtures, dumps avec vraies données) | Élevé | Moyenne | 🟠 Élevé |
| R7 | **Accumulation de contexte** (l'historique croissant multiplie l'exposition) | Moyen | Élevée | 🟡 Moyen |
| R8 | **PII dans les images** (screenshots contenant des données visibles) | Moyen | Faible | 🟡 Moyen |
| R9 | **Exposition de la topologie réseau** (configs réseau, firewalls, VPN) | Moyen | Moyenne | 🟡 Moyen |
| R10 | **Prompt injection** (un fichier malveillant manipule le comportement du LLM) | Moyen | Faible | 🟡 Moyen |

### 3.2 Attentes typiques d'un RSSI

Un RSSI évaluant l'adoption d'outils LLM dans son organisation attend :

#### Visibilité et contrôle

- **Savoir ce qui sort** : quelles données quittent le périmètre de l'entreprise ?
- **Logs d'audit** : tracer qui a envoyé quoi, quand, à quel provider
- **Politique d'accès** : contrôler quels fichiers/dossiers sont accessibles au LLM
- **Kill switch** : pouvoir couper l'accès instantanément

#### Protection des données

- **Aucun secret en clair** vers l'extérieur (credentials, clés API, tokens)
- **Aucune PII** non maîtrisée (noms, emails, téléphones, adresses)
- **Aucune donnée d'infrastructure** exploitable (IPs, noms de serveurs, schémas réseau)
- **Classification des données** : ne pas envoyer les données classifiées "Confidentiel" ou supérieur

#### Conformité

- **RGPD** : pas de transfert de données personnelles hors UE sans base légale
- **NIS2** : mesures de sécurité proportionnées pour les entités essentielles/importantes
- **Politique interne** : respect de la charte de sécurité de l'entreprise
- **Traçabilité** : prouver aux auditeurs que les données sont protégées

#### Réversibilité et souveraineté

- **Pas de vendor lock-in** : pouvoir changer de provider LLM sans perte
- **Contrôle local** : les mécanismes de protection tournent sur l'infrastructure interne
- **Indépendance** : pas de dépendance à un service tiers pour la protection

---

## 4. Protections apportées par MirageIA

### 4.1 Protection par type de donnée

#### A. Credentials et secrets

| Donnée | Exemple | Protection MirageIA |
|---|---|---|
| Mot de passe en dur | `password = "P@ssw0rd!"` | Détection → remplacement par `password = "Tr0ub4dor!"` |
| Clé API | `sk-live-4eC39HqLyjW...` | Détection → remplacement par `sk-live-a1B2c3D4e5F...` (même format) |
| Token JWT | `eyJhbGci...` | Détection → remplacement par un JWT fictif |
| Connection string | `postgres://admin:s3cret@db:5432` | Détection de chaque composant sensible (user, password, host) |
| Clé SSH privée | `-----BEGIN RSA PRIVATE KEY-----` | Détection du bloc complet → remplacement par une clé fictive |
| Variable d'environnement | `AWS_SECRET_ACCESS_KEY=AKIA...` | Détection du pattern `VARIABLE=valeur` → valeur pseudonymisée |

#### B. Données personnelles (PII)

| Donnée | Exemple dans le code | Protection MirageIA |
|---|---|---|
| Nom de personne | `// Author: Jean Dupont` | → `// Author: Michel Martin` |
| Email | `admin@entreprise.fr` | → `paul@example.com` |
| Téléphone | `+33 6 12 34 56 78` | → `+33 6 98 76 54 32` |
| Adresse IP | `192.168.1.50` | → `10.0.42.7` |
| IBAN | `FR76 3000 6000...` | → `FR14 2004 1010...` (checksum valide) |
| Numéro de sécu | `1 85 07 75 123 456 78` | → `2 91 03 13 987 654 32` |
| Adresse postale | `12 rue de la Paix, Paris` | → `8 avenue Victor Hugo, Lyon` |

#### C. Données d'infrastructure

| Donnée | Exemple | Protection MirageIA |
|---|---|---|
| IP serveur interne | `db.internal:5432` | → `db.example.local:5432` |
| Nom de domaine interne | `jira.corp.entreprise.fr` | → `jira.internal.example.com` |
| URL interne | `https://gitlab.corp/projet/repo` | → `https://gitlab.example.com/projet/repo` |
| Nom de serveur | `srv-prod-db-01` | → `srv-app-01` |
| Chemin de fichier | `/opt/entreprise/data/clients.db` | → `/opt/app/data/database.db` |
| Plage réseau | `10.42.0.0/16` | → `10.0.0.0/16` |

#### D. Données métier

| Donnée | Exemple | Protection MirageIA |
|---|---|---|
| Noms de clients dans les fixtures | `{"name": "Société Durand"}` | → `{"name": "Société Example"}` |
| Montants financiers | `total: 1_547_892.50€` | Non pseudonymisé (pas une PII directe) |
| Numéro de contrat | `CTR-2024-00547` | Détection contextuelle → pseudonymisation si pattern identifié |

### 4.2 Protection de l'historique conversationnel

MirageIA pseudonymise **chaque appel API**, y compris l'historique cumulé :

```
Appel 1 :
  Utilisateur : "Refactore login.ts" + contenu du fichier
  → MirageIA pseudonymise le contenu du fichier

Appel 5 :
  Historique (appels 1-4 inclus) + nouveau message
  → Les pseudonymes sont COHÉRENTS : "Tardy" est toujours "Gerard"
  → L'historique contient les versions déjà pseudonymisées
  → Le nouveau message est aussi pseudonymisé
```

### 4.3 Ce que l'API voit vs ce qui est réel

```
┌─ Ce que le développeur écrit ──────────────────────────────────┐
│ "Le serveur db-prod-01 (192.168.1.50) est en panne.           │
│  Contacte jean.dupont@entreprise.fr pour le mot de passe       │
│  admin du dashboard https://grafana.corp.entreprise.fr"         │
└────────────────────────────────────────────────────────────────┘
                              │
                         MirageIA
                              │
                              ▼
┌─ Ce que l'API Anthropic reçoit ────────────────────────────────┐
│ "Le serveur srv-app-01 (10.0.42.7) est en panne.              │
│  Contacte paul.martin@example.com pour le mot de passe         │
│  admin du dashboard https://grafana.internal.example.com"       │
└────────────────────────────────────────────────────────────────┘
                              │
                      Réponse de l'API
                              │
                              ▼
┌─ Ce que l'API répond ──────────────────────────────────────────┐
│ "Pour le serveur srv-app-01, je suggère de vérifier les logs   │
│  à /var/log/... Demandez à paul.martin@example.com de..."      │
└────────────────────────────────────────────────────────────────┘
                              │
                         MirageIA
                              │
                              ▼
┌─ Ce que le développeur reçoit ─────────────────────────────────┐
│ "Pour le serveur db-prod-01, je suggère de vérifier les logs   │
│  à /var/log/... Demandez à jean.dupont@entreprise.fr de..."    │
└────────────────────────────────────────────────────────────────┘
```

---

## 5. Matrice risques × protections

| Risque | Sans MirageIA | Avec MirageIA | Protection |
|---|---|---|---|
| R1 — Fuite de credentials | 🔴 Exposé en clair | 🟢 Pseudonymisé | Clés, tokens et mots de passe remplacés par des valeurs fictives de même format |
| R2 — Fuite de PII | 🔴 Exposé en clair | 🟢 Pseudonymisé | Noms, emails, téléphones, adresses remplacés par des valeurs fictives cohérentes |
| R3 — Exposition architecture | 🟠 Exposé en clair | 🟢 Pseudonymisé | IPs, noms de serveurs, URLs internes remplacés |
| R4 — Fuite de propriété intellectuelle | 🟠 Exposé en clair | 🟡 Partiellement protégé | La logique métier et les algorithmes ne sont pas pseudonymisés (seulement les données) |
| R5 — Non-conformité RGPD | 🟠 Transfert hors UE | 🟢 Données anonymisées | Les PII sont pseudonymisées avant le transfert — les données envoyées ne sont plus des données personnelles au sens du RGPD |
| R6 — Données de production | 🟠 Exposé en clair | 🟢 Pseudonymisé | Les fixtures et dumps contenant des vraies données sont pseudonymisés |
| R7 — Accumulation de contexte | 🟡 Historique croissant | 🟢 Pseudonymisé | Chaque appel est pseudonymisé, l'historique ne contient que des pseudonymes |
| R8 — PII dans les images | 🟡 Envoyées telles quelles | 🔴 Non protégé | MirageIA v1 ne traite pas les images (limitation connue) |
| R9 — Topologie réseau | 🟡 Exposée | 🟢 Pseudonymisé | IPs, plages réseau, noms de serveurs remplacés |
| R10 — Prompt injection | 🟡 Risque existant | 🟡 Risque inchangé | MirageIA ne protège pas contre le prompt injection (hors périmètre) |

---

## 6. Limites et périmètre non couvert

### 6.1 Ce que MirageIA ne protège PAS

| Limitation | Explication | Mitigation possible |
|---|---|---|
| **Propriété intellectuelle** | Les algorithmes, la logique métier, l'architecture du code sont transmis en clair. MirageIA protège les *données*, pas la *logique*. | Politique d'entreprise limitant les types de projets autorisés avec un LLM |
| **Images et captures d'écran** | MirageIA v1 analyse le texte uniquement, pas les images. Les PII visibles dans les screenshots ne sont pas détectées. | Sensibilisation des développeurs, extension future avec OCR |
| **Prompt injection** | Si un fichier malveillant contient des instructions qui manipulent le LLM, MirageIA ne détecte pas cette menace. | Compléter avec un outil de détection de prompt injection (LLM Guard, NeMo Guardrails) |
| **Métadonnées de fichiers** | Noms de fichiers, chemins, timestamps ne sont pas pseudonymisés s'ils apparaissent dans les métadonnées de la requête (hors contenu textuel). | Extension future |
| **Volume et patterns d'usage** | Le nombre de requêtes, leur fréquence et leur taille restent visibles pour le provider. | VPN / proxy réseau si nécessaire |
| **Données structurées complexes** | Un schéma SQL complet ou un fichier de migration peut révéler la structure métier même avec les données pseudonymisées. | Politique d'entreprise, classification des fichiers |

### 6.2 Faux positifs et faux négatifs

| Cas | Impact | Fréquence estimée |
|---|---|---|
| **Faux positif** : une variable `edison_voltage` est pseudonymisée | La réponse du LLM peut être incorrecte (nom de variable modifié) | Faible (détection contextuelle) |
| **Faux négatif** : un identifiant client non standard `CLI-847293` n'est pas détecté | Fuite de données | Moyen (formats non standards) |

Le seuil de confiance est ajustable : un seuil bas réduit les faux négatifs mais augmente les faux positifs.

---

## 7. Conformité réglementaire

### 7.1 RGPD (Règlement Général sur la Protection des Données)

| Exigence RGPD | Sans MirageIA | Avec MirageIA |
|---|---|---|
| **Art. 5 — Minimisation** : ne collecter que les données nécessaires | ❌ Toutes les données du projet sont envoyées | ✅ Les PII sont pseudonymisées, seules des données fictives sont envoyées |
| **Art. 25 — Privacy by design** | ❌ Aucune protection intégrée | ✅ Protection automatique par défaut, sans action du développeur |
| **Art. 44-49 — Transfert hors UE** | ❌ Données personnelles envoyées aux USA | ✅ Les données envoyées ne sont plus des données personnelles (pseudonymisées de manière réversible localement uniquement) |
| **Art. 32 — Sécurité du traitement** | ❌ Données en clair dans les requêtes API | ✅ Chiffrement du mapping AES-256-GCM, données pseudonymisées |
| **Art. 30 — Registre des traitements** | Le dashboard MirageIA peut servir de trace des données traitées | ✅ Logs des types de PII détectées (sans les valeurs originales) |

### 7.2 NIS2 (Directive sur la sécurité des réseaux et des systèmes d'information)

| Exigence NIS2 | Contribution de MirageIA |
|---|---|
| Gestion des risques liés à la chaîne d'approvisionnement | Réduit le risque de fuite de données via des fournisseurs tiers (providers LLM) |
| Mesures techniques de sécurité | Chiffrement du mapping, pseudonymisation automatique |
| Notification des incidents | Le dashboard peut détecter des patterns anormaux (volume inhabituel de PII) |

### 7.3 SOC 2 / ISO 27001

MirageIA contribue aux contrôles suivants :
- **Contrôle d'accès aux données** : les données sensibles ne quittent pas le périmètre
- **Chiffrement** : AES-256-GCM pour le mapping en mémoire
- **Journalisation** : logs d'audit des détections (sans données originales)
- **Gestion des tiers** : réduction du risque lié aux fournisseurs de services IA

---

## 8. Recommandations pour le RSSI

### 8.1 Déploiement recommandé

```
┌─ Périmètre de l'entreprise ─────────────────────────────────────┐
│                                                                   │
│  Poste développeur                                                │
│  ┌──────────────────────────────────────────────────────────────┐ │
│  │                                                              │ │
│  │  Application (Claude Code)                                   │ │
│  │       │                                                      │ │
│  │       ▼                                                      │ │
│  │  ┌──────────┐                                                │ │
│  │  │ MirageIA │ ← tourne en local, aucune donnée ne sort      │ │
│  │  │          │   sans pseudonymisation                        │ │
│  │  └──────────┘                                                │ │
│  │       │                                                      │ │
│  └───────┼──────────────────────────────────────────────────────┘ │
│          │ données pseudonymisées uniquement                      │
│          │ (HTTPS)                                                │
└──────────┼────────────────────────────────────────────────────────┘
           │
           ▼
    ┌──────────────┐
    │ API Anthropic │  ← ne voit que des données fictives
    │ (cloud US)    │
    └──────────────┘
```

### 8.2 Mesures complémentaires recommandées

| Mesure | Objectif | Priorité |
|---|---|---|
| **`.gitignore` strict** | Empêcher les fichiers sensibles d'être dans le repo (`.env`, clés) | Haute |
| **`.claudeignore`** | Exclure des fichiers de la lecture par Claude Code | Haute |
| **Formation développeurs** | Sensibiliser aux risques de fuite de données via les LLM | Haute |
| **Classification des projets** | Interdire l'usage de LLM sur les projets "Confidentiel" et supérieur | Haute |
| **Monitoring réseau** | Surveiller le volume de données envoyées aux API LLM | Moyenne |
| **Rotation des clés API** | Limiter l'impact d'une clé API exposée dans un prompt | Moyenne |
| **Revue des fixtures** | Remplacer les vraies données dans les fichiers de test par des données fictives | Moyenne |
| **Audit périodique** | Vérifier que MirageIA détecte correctement les PII du contexte métier | Moyenne |

### 8.3 Indicateurs de suivi (KPI)

| KPI | Description | Cible |
|---|---|---|
| Taux de couverture PII | % de PII détectées vs PII réelles (mesuré par audit) | > 95% |
| Taux de faux positifs | % de détections incorrectes | < 5% |
| Nombre de credentials interceptés | Credentials qui auraient été envoyés sans MirageIA | Suivi mensuel |
| Latence ajoutée | Impact sur la productivité du développeur | < 100ms |
| Adoption | % de développeurs utilisant MirageIA | 100% (obligatoire) |

### 8.4 Argumentaire pour la direction

> **Sans MirageIA** : chaque développeur utilisant Claude Code, Copilot ou ChatGPT envoie potentiellement des mots de passe, des données clients, des adresses IP de serveurs de production et des clés API vers des serveurs cloud aux États-Unis. L'entreprise n'a aucune visibilité ni contrôle sur ces fuites.
>
> **Avec MirageIA** : un proxy local transparent pseudonymise automatiquement toutes les données sensibles avant qu'elles ne quittent le poste. Les développeurs conservent la productivité offerte par les LLM, la DSI conserve le contrôle des données. Le tout dans un binaire unique, sans dépendance cloud, sans coût récurrent, et conforme au RGPD.
