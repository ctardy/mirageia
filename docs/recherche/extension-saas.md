# Extension du périmètre — Modèle SaaS et routage multi-provider

> Analyse exploratoire suite à une proposition d'élargir MirageIA vers un modèle SaaS avec routage multi-provider.

---

## Contexte

Proposition d'évolution : transformer MirageIA en une plateforme SaaS offrant plusieurs services, notamment la sécurité PII et la redirection transparente vers n'importe quel fournisseur LLM (au choix du client).

---

## Analyse des deux approches

### Option A — SaaS pur (proxy cloud)

Le client pointe ses applications vers un proxy hébergé par MirageIA. Le proxy détecte les PII, pseudonymise, puis route vers le provider LLM choisi par le client.

```
Client (app)  ──▶  MirageIA (cloud)  ──▶  Anthropic / OpenAI / Mistral / ...
```

**Avantages** :
- Déploiement instantané pour le client (rien à installer)
- Facturation à l'usage (SaaS classique)
- Centralisation des logs d'audit

**Inconvénients majeurs** :
- **Les données sensibles transitent par nos serveurs** → on devient un vecteur de risque supplémentaire, ce qui contredit le positionnement sécurité de MirageIA
- **Marché déjà occupé** : LiteLLM (open-source, 20k+ stars, 100+ providers) fait le routage multi-provider gratuitement ; Lakera Guard et LLM Guard font la sécurité
- **Argument RGPD affaibli** : les données passent par un tiers (nous) avant le provider → deux transferts au lieu d'un
- **Confiance** : le RSSI doit nous faire confiance en plus du provider LLM

### Option B — Hybride (moteur local + console SaaS) ← recommandée

Le moteur de sécurité et le routage restent **sur le poste ou l'infrastructure du client**. Seule la console d'administration (policies, dashboard, audit) est en SaaS.

```
Entreprise cliente (on-prem)              Cloud MirageIA (SaaS)
┌────────────────────────────────┐       ┌─────────────────────┐
│ App → MirageIA (binaire local) │──────▶│ Console admin       │
│      ├─ Détection PII (ONNX)   │ config│ ├─ Policies         │
│      ├─ Pseudonymisation       │◀──────│ ├─ Audit logs       │
│      ├─ Routage multi-provider │       │ ├─ Dashboard        │
│      │   ├─▶ Anthropic         │       │ ├─ Alertes          │
│      │   ├─▶ OpenAI            │       │ └─ Gestion licences │
│      │   └─▶ Mistral / autre   │       └─────────────────────┘
│      └─ Logs (sans PII)  ──────┼──────▶ (logs anonymisés)
└────────────────────────────────┘
    ⚠ Aucune donnée sensible ne
      quitte le périmètre client
```

**Avantages** :
- Conserve le différenciateur clé : "les données ne sortent jamais"
- Le RSSI valide : le moteur tourne sur son infra, seuls les logs anonymisés partent
- Justifie un abonnement (console SaaS, policies centralisées, support)
- Le routage multi-provider est un bonus facile à ajouter (forwarding HTTP)
- Compatible avec les exigences de souveraineté (SecNumCloud, HDS, etc.)

**Inconvénients** :
- Plus complexe à déployer pour le client (installation du binaire)
- Nécessite un mécanisme de synchronisation config cloud ↔ agent local
- Coût de développement de la console SaaS

---

## Fonctionnalités du modèle hybride

### Côté local (binaire MirageIA — déjà dans le périmètre)

- Détection PII via modèle ONNX embarqué
- Pseudonymisation / dé-pseudonymisation réversible
- Routage multi-provider (Anthropic, OpenAI, Mistral, Azure, Bedrock, etc.)
- Streaming SSE
- Mapping chiffré AES-256-GCM en mémoire

### Côté SaaS (console — extension)

| Fonctionnalité | Description |
|---|---|
| **Policies centralisées** | Définir les règles de détection par projet/équipe (seuils, types de PII, exclusions) |
| **Dashboard multi-postes** | Vue agrégée des détections sur tous les postes de l'équipe |
| **Audit logs** | Historique des détections (types, compteurs, timestamps — jamais les données originales) |
| **Alertes** | Notification si volume anormal de PII détectées, tentative de contournement, etc. |
| **Gestion des licences** | Attribution, révocation, suivi des postes actifs |
| **Routage provider** | Configuration centralisée des providers autorisés et des clés API |
| **Rapports de conformité** | Export pour les auditeurs (RGPD, NIS2, SOC 2) |

---

## Concurrents sur ce créneau

| Concurrent | Modèle | Différence avec l'option B |
|---|---|---|
| LiteLLM | Proxy multi-provider (self-hosted ou cloud) | Pas de LLM embarqué pour la détection PII, sécurité basique (Presidio) |
| Lakera Guard | SaaS pur (détection prompt injection + PII) | Données transitent par leurs serveurs |
| Protecto AI | SaaS pur (tokenisation sémantique) | Données transitent par leurs serveurs |
| Granica Screen | Cloud (AWS/GCP/Azure) | Dépendance cloud, pas de binaire local |
| Private AI | API + on-premise | Détection PII avancée mais pas de routage multi-provider |

**Aucun concurrent ne propose le modèle hybride** (moteur local + console SaaS).

---

## Modèle de revenus potentiel

| Offre | Contenu | Prix indicatif |
|---|---|---|
| **Community** | Binaire MirageIA (local, open-source), détection PII, 1 provider | Gratuit |
| **Pro** | + Console SaaS, multi-provider, policies, dashboard, alertes | ~30-50€/poste/mois |
| **Enterprise** | + SSO, audit logs avancés, rapports conformité, support prioritaire, SLA | Sur devis |

---

## Décision

À discuter. L'option B (hybride) est recommandée car elle :
- Préserve le positionnement sécurité de MirageIA
- Crée une valeur récurrente (abonnement SaaS)
- Se différencie de tous les concurrents existants
- Répond aux attentes des RSSI (données locales, contrôle centralisé)

L'option A (SaaS pur) est déconseillée : elle affaiblit le différenciateur principal et entre en concurrence frontale avec LiteLLM (gratuit, établi, 20k+ stars).
