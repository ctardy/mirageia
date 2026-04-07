# Guide de déploiement — MirageIA

Guide opérationnel pour déployer MirageIA sur un serveur Linux avec Docker, derrière un reverse proxy Apache.

---

## Qu'est-ce que MirageIA ?

MirageIA est un **proxy local** qui s'intercale entre les développeurs et les API LLM (Claude, ChatGPT). Il détecte automatiquement les données sensibles (emails, IPs, numéros de téléphone, clés API...) dans les requêtes, les remplace par des valeurs fictives, et restaure les originaux dans les réponses.

**L'API LLM ne voit jamais les vraies données.**

```
Développeur → MirageIA (proxy :3100) → API LLM (Anthropic/OpenAI)
                │                         │
                ├─ Détecte les PII        │
                ├─ Pseudonymise           │
                └─ Restaure  ◄────────────┘
```

Le proxy expose un **dashboard web** pour surveiller les requêtes en temps réel.

---

## Prérequis serveur

| Élément | Version | Notes |
|---------|---------|-------|
| Linux | Ubuntu 22.04+ / Debian 12+ | x86_64 |
| Docker | 24+ | `docker --version` |
| Apache | 2.4+ | Avec `mod_proxy`, `mod_proxy_http`, `mod_ssl` |
| Port | 3100 | Port interne du proxy (non exposé directement) |
| Réseau | Sortant HTTPS | Vers `api.anthropic.com`, `api.openai.com` |

---

## 1. Déploiement Docker

### Récupérer le projet

```bash
git clone https://github.com/ctardy/mirageia.git /opt/mirageia
cd /opt/mirageia/docker
```

### Builder l'image

```bash
docker build -t mirageia .
```

L'image contient :
- Ubuntu 24.04
- MirageIA (binaire téléchargé depuis GitHub Releases **à chaque démarrage**)
- Node.js 22 + Claude Code
- ttyd (terminal web WebSocket)
- docker CLI (pour les tests Docker-in-Docker via socket)

Taille de l'image : ~500 Mo.

### Mise à jour du binaire MirageIA (sans rebuild)

L'entrypoint télécharge automatiquement la dernière release à chaque `docker restart` :

```bash
# Après un git tag + push, attendre que le CI publie la release (~3 min), puis :
docker restart mirageia
# Le nouveau binaire est automatiquement récupéré au redémarrage
```

Pour vérifier la version active :
```bash
docker exec mirageia mirageia --version
```

### Déploiement via Docker Compose (recommandé)

Fichier `docker-compose.yml` de référence :

```yaml
services:
  mirageia:
    build:
      context: /opt/projet/mirageia
      dockerfile: docker/Dockerfile
    image: mirageia:latest
    container_name: mirageia
    restart: unless-stopped
    env_file:
      - /opt/docker/conf/secrets.env   # ANTHROPIC_API_KEY
    environment:
      - TZ=Europe/Paris
    volumes:
      - ./home/.claude:/root/.claude        # tokens OAuth Claude Code persistés
      - ./home/.claude.json:/root/.claude.json
      - ./home/.local:/root/.local
      - /opt/projet/mirageia:/workspace     # source Rust montée pour cargo test
      - /var/run/docker.sock:/var/run/docker.sock  # Docker-in-Docker
    healthcheck:
      test: ["CMD", "curl", "-sf", "http://localhost:3100/health"]
      interval: 30s
      timeout: 10s
      retries: 3
    security_opt:
      - no-new-privileges:true
    cap_drop:
      - ALL
    cap_add:
      - NET_BIND_SERVICE
      - DAC_OVERRIDE   # nécessaire pour /var/run/docker.sock
    deploy:
      resources:
        limits:
          cpus: '2.0'
          memory: 3G        # 3G requis si le modèle ONNX est actif (VmPeak ~2,1 Go pendant le chargement)
        reservations:
          memory: 256M
```

> **Attention mémoire** : la limite mémoire doit être d'au moins **3 Go** si le modèle ONNX est actif. Le modèle atteint ~2,1 Go en VmPeak lors du chargement et se stabilise à ~946 Mo RSS. Sans ONNX (mode regex seul), 1 Go suffit.

### Activation du modèle ONNX

Le modèle ONNX est téléchargé une seule fois et persisté dans le volume `./home/.mirageia`. Il survit aux redémarrages du container.

```bash
# 1. Télécharger le modèle (une fois, dans le container en cours d'exécution)
docker exec mirageia mirageia model download iiiorg/piiranha-v1-detect-personal-information

# 2. Le définir comme actif
docker exec mirageia mirageia model use iiiorg/piiranha-v1-detect-personal-information

# 3. Rebuilder l'image (l'entrypoint est COPY lors du build — rebuild obligatoire)
cd /opt/docker/mirageia
docker compose build
docker compose up -d
```

> **Important** : `docker/entrypoint.sh` est intégré dans l'image via `COPY docker/entrypoint.sh /entrypoint.sh`. Modifier le fichier sur disque n'a **aucun effet** sans `docker compose build`.

Pour vérifier que l'ONNX est actif :
```bash
curl -s http://localhost:3100/health | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('onnx_model','inactif'))"
# → iiiorg/piiranha-v1-detect-personal-information
```

Le log de démarrage affichera :
```
✓ Modèle ONNX actif — détection contextuelle activée
```

### Lancer le container

```bash
# Mode production (daemon)
docker run -d \
  --name mirageia \
  --restart unless-stopped \
  -p 127.0.0.1:3100:3100 \
  -p 127.0.0.1:7681:7681 \
  -e ANTHROPIC_API_KEY=sk-ant-XXXXXXXXX \
  --memory=3g \
  mirageia

# Vérifier que ça tourne
docker logs mirageia
curl http://127.0.0.1:3100/health
```

Réponse attendue du health check :
```json
{"status": "ok", "passthrough": false, "pii_mappings": 0, "version": "0.5.9", "onnx_model": "iiiorg/piiranha-v1-detect-personal-information"}
```
(`"onnx_model"` vaut `null` en mode regex seul.)

### Terminal web (ttyd)

Le container expose un terminal web sur le port **7681** (chemin `/mirageia/`).
ttyd est lancé avec `--ping-interval 30` pour maintenir la connexion WebSocket.

Si déployé derrière Apache, configurer un timeout long sur le ProxyPass :

```apache
# IMPORTANT : timeout=3600 pour éviter la coupure WebSocket après 2 min
ProxyPass /mirageia/ http://mirageia:7681/mirageia/ timeout=3600
ProxyPassReverse /mirageia/ http://mirageia:7681/mirageia/
```

Commandes disponibles depuis le terminal web :
```
mirageia wrap -- claude    Lance Claude Code via le proxy
claude                     Lance Claude Code directement
mirageia console           Monitoring temps réel
curl localhost:3100/health Health check
```

### Lancer en mode interactif (tests)

```bash
docker run -it --rm \
  -p 127.0.0.1:3100:3100 \
  -e ANTHROPIC_API_KEY=sk-ant-XXXXXXXXX \
  mirageia
```

---

## 2. Configuration Apache (reverse proxy)

### Activer les modules

```bash
a2enmod proxy proxy_http ssl headers
systemctl restart apache2
```

### VirtualHost

Créer `/etc/apache2/sites-available/mirageia.conf` :

```apache
<VirtualHost *:443>
    ServerName mirageia.example.com

    # ─── Reverse proxy vers MirageIA ───

    ProxyPreserveHost On
    ProxyRequests Off

    # Dashboard web
    ProxyPass /dashboard http://127.0.0.1:3100/dashboard
    ProxyPassReverse /dashboard http://127.0.0.1:3100/dashboard

    # Health check
    ProxyPass /health http://127.0.0.1:3100/health
    ProxyPassReverse /health http://127.0.0.1:3100/health

    # Flux SSE temps réel (pour le dashboard)
    # IMPORTANT : désactiver le buffering sinon le SSE ne fonctionne pas
    <Location /events>
        ProxyPass http://127.0.0.1:3100/events
        ProxyPassReverse http://127.0.0.1:3100/events
        SetEnv proxy-sendchunked 1
        SetEnv proxy-initial-not-pooled 1
        SetEnv proxy-nokeepalive 1
    </Location>

    # Routes API (pour que les devs pointent dessus)
    ProxyPass /v1 http://127.0.0.1:3100/v1
    ProxyPassReverse /v1 http://127.0.0.1:3100/v1

    # ─── SSL (Let's Encrypt) ───

    SSLEngine on
    SSLCertificateFile /etc/letsencrypt/live/mirageia.example.com/fullchain.pem
    SSLCertificateKeyFile /etc/letsencrypt/live/mirageia.example.com/privkey.pem

    # ─── Sécurité ───

    # Restreindre l'accès au dashboard (optionnel)
    <Location /dashboard>
        Require ip 10.0.0.0/8 172.16.0.0/12 192.168.0.0/16
        # Ou par authentification :
        # AuthType Basic
        # AuthName "MirageIA Dashboard"
        # AuthUserFile /etc/apache2/.htpasswd-mirageia
        # Require valid-user
    </Location>

    # Headers de sécurité
    Header always set X-Content-Type-Options "nosniff"
    Header always set X-Frame-Options "DENY"
</VirtualHost>

# Redirection HTTP → HTTPS
<VirtualHost *:80>
    ServerName mirageia.example.com
    Redirect permanent / https://mirageia.example.com/
</VirtualHost>
```

### Activer et tester

```bash
a2ensite mirageia
apachectl configtest     # Vérifier la syntaxe
systemctl reload apache2

# Tester
curl https://mirageia.example.com/health
```

---

## 3. Utilisation par les développeurs

Une fois déployé, les développeurs ont deux options :

### Option A — Via le serveur (centralisé)

```bash
export ANTHROPIC_BASE_URL=https://mirageia.example.com
claude
```

Toutes les requêtes Claude passent par MirageIA sur le serveur.

### Option B — En local (décentralisé)

Chaque développeur installe MirageIA sur son poste :

```bash
# Télécharger le binaire
curl -sSfL https://github.com/ctardy/mirageia/releases/latest/download/mirageia-linux-x86_64.tar.gz | tar xz -C ~/.local/bin/

# Lancer le proxy localement
mirageia &

# Utiliser Claude via le proxy
mirageia wrap -- claude
```

---

## 4. Dashboard de monitoring

Accessible sur : `https://mirageia.example.com/dashboard`

Le dashboard affiche en temps réel :

| Information | Description |
|-------------|-------------|
| **Requêtes** | Compteur total de requêtes traitées |
| **PII détectées** | Nombre total de données sensibles interceptées |
| **Mappings actifs** | Nombre de pseudonymes en mémoire |
| **Mode** | PII (pseudonymisation active) ou PASS (passthrough) |
| **Flux live** | Chaque requête avec horodatage, provider, chemin, nombre de PII |

Le flux se met à jour en temps réel via Server-Sent Events (SSE).

---

## 5. Endpoints disponibles

| Endpoint | Méthode | Description | Accès |
|----------|---------|-------------|-------|
| `/health` | GET | État du proxy (JSON) | Monitoring / load balancer |
| `/dashboard` | GET | Dashboard web temps réel | Navigateur (protéger via Apache) |
| `/events` | GET | Flux SSE des événements | Dashboard / outils de monitoring |
| `/v1/messages` | POST | Proxy Anthropic (Claude) | Applications |
| `/v1/chat/completions` | POST | Proxy OpenAI (GPT) | Applications |

---

## 6. Mode passthrough (désactivation temporaire)

Pour désactiver la pseudonymisation sans arrêter le proxy :

```bash
# Relancer le container avec le flag
docker stop mirageia
docker run -d \
  --name mirageia \
  --restart unless-stopped \
  -p 127.0.0.1:3100:3100 \
  -e ANTHROPIC_API_KEY=sk-ant-XXXXXXXXX \
  -e MIRAGEIA_PASSTHROUGH=1 \
  mirageia
```

Le health check indiquera `"passthrough": true`. Le dashboard affichera "PASS" au lieu de "PII".

Pour réactiver : relancer sans `MIRAGEIA_PASSTHROUGH`.

---

## 7. Monitoring et alertes

### Health check (Nagios, Zabbix, etc.)

```bash
# Retourne HTTP 200 + JSON si OK
curl -sf http://127.0.0.1:3100/health || echo "CRITICAL: MirageIA down"
```

### Logs Docker

```bash
# Logs en temps réel
docker logs -f mirageia

# Logs avec horodatage
docker logs --timestamps mirageia

# Dernières 50 lignes
docker logs --tail 50 mirageia
```

Les logs affichent pour chaque requête :
```
INFO  PII détectées dans la requête pii_count=3
INFO  Requête pseudonymisée provider=Anthropic mappings=3
```

### Redémarrage automatique

Le flag `--restart unless-stopped` assure le redémarrage après un crash ou un reboot serveur.

---

## 8. Sécurité

### Ce qui transite

| Donnée | Où | En clair ? |
|--------|-----|-----------|
| Clés API (ANTHROPIC_API_KEY) | Variable d'env du container | Oui (variable d'env) |
| Requêtes originales (avec PII) | Entre le dev et MirageIA | Oui (HTTPS via Apache) |
| Requêtes pseudonymisées | Entre MirageIA et l'API LLM | Oui (HTTPS natif) |
| Table de mapping (PII ↔ pseudonymes) | En mémoire du container | Chiffré AES-256-GCM |

### Points importants

- La table de mapping **n'est jamais persistée sur disque** — un restart du container la vide
- Les clés API sont **transmises telles quelles** à l'API (MirageIA ne les stocke pas)
- Le binaire MirageIA n'a **aucune dépendance réseau** à part les API LLM cibles
- Aucune télémétrie, aucun appel vers des serveurs tiers

### Proxy d'entreprise (v0.5.25+)

Si MirageIA tourne dans un réseau d'entreprise qui fait transiter le trafic sortant par un proxy :

```toml
# config.toml
[proxy]
upstream_proxy = "http://proxy.corp:8080"
```

Ou via variable d'environnement :
```bash
MIRAGEIA_UPSTREAM_PROXY=http://proxy.corp:8080
```

#### Inspection SSL / proxy MITM

Certains proxies d'entreprise effectuent de l'inspection TLS et présentent leur propre certificat, que MirageIA rejette par défaut. Si vous obtenez des erreurs `502 Bad Gateway` avec un proxy corporate, activez l'acceptation des certificats :

```toml
# config.toml
[proxy]
upstream_proxy = "http://proxy.corp:8080"
danger_accept_invalid_certs = true
```

Ou :
```bash
MIRAGEIA_DANGER_ACCEPT_INVALID_CERTS=1
```

> **Attention** : `danger_accept_invalid_certs = true` désactive la validation TLS pour les appels sortants. À n'utiliser que si votre proxy d'entreprise est la cause. L'assistant `mirageia setup` pose automatiquement la question lors de la configuration d'un proxy (étape 5b).

### Recommandations

- Protéger le `/dashboard` par IP ou authentification (voir config Apache ci-dessus)
- Ne pas exposer le port 3100 directement — toujours passer par Apache/HTTPS
- Utiliser un réseau Docker dédié si d'autres containers tournent sur le serveur
- Stocker la clé API dans un secret manager (Docker secrets, Vault) plutôt qu'en variable d'env

---

## 9. Mise à jour

```bash
cd /opt/mirageia

# Récupérer la dernière version
git pull

# Rebuild l'image
docker build -t mirageia docker/

# Redémarrer le container
docker stop mirageia && docker rm mirageia
docker run -d \
  --name mirageia \
  --restart unless-stopped \
  -p 127.0.0.1:3100:3100 \
  -e ANTHROPIC_API_KEY=sk-ant-XXXXXXXXX \
  mirageia
```

Pour une version spécifique :

```bash
docker build --build-arg MIRAGEIA_VERSION=v0.2.0 -t mirageia docker/
```

---

## 10. Troubleshooting

### Le container ne démarre pas

```bash
docker logs mirageia
# Vérifier que ANTHROPIC_API_KEY est définie
```

### Le dashboard est vide (pas d'événements)

- Vérifier que le SSE passe à travers Apache (pas de buffering)
- Tester en direct : `curl http://127.0.0.1:3100/events` (doit rester ouvert)
- Vérifier les modules Apache : `apachectl -M | grep proxy`

### Les requêtes Claude échouent

```bash
# Tester le proxy directement
curl -X POST http://127.0.0.1:3100/v1/messages \
  -H "x-api-key: $ANTHROPIC_API_KEY" \
  -H "content-type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  -d '{"model":"claude-sonnet-4-20250514","max_tokens":10,"messages":[{"role":"user","content":"test"}]}'
```

### Le SSE ne fonctionne pas derrière Apache

Vérifier que ces directives sont bien présentes dans la `<Location /events>` :
```apache
SetEnv proxy-sendchunked 1
SetEnv proxy-initial-not-pooled 1
SetEnv proxy-nokeepalive 1
```

### Performance

| Mode | RAM au repos | Pic au démarrage |
|------|-------------|------------------|
| Regex seul | ~10 Mo | ~10 Mo |
| ONNX actif | ~946 Mo | ~2,1 Go |

- La pseudonymisation ajoute ~1–5 ms de latence par requête (couche regex)
- L'ONNX ajoute ~15–30 ms par requête
- Le container n'a pas besoin de GPU

---

## Résumé des commandes

```bash
# Build
docker build -t mirageia /opt/mirageia/docker/

# Lancer (production)
docker run -d --name mirageia --restart unless-stopped \
  -p 127.0.0.1:3100:3100 \
  -e ANTHROPIC_API_KEY=sk-ant-XXX \
  mirageia

# Lancer (test interactif)
docker run -it --rm -p 127.0.0.1:3100:3100 \
  -e ANTHROPIC_API_KEY=sk-ant-XXX mirageia

# Status
curl http://127.0.0.1:3100/health

# Logs
docker logs -f mirageia

# Restart
docker restart mirageia

# Stop
docker stop mirageia && docker rm mirageia
```

---

## Historique des versions

| Version | Date | Changements |
|---------|------|-------------|
| v0.5.27 | 07/04/2026 | Fix clippy : suppression du `use std::env` en double dans l'assistant de configuration. |
| v0.5.26 | 07/04/2026 | Fix panic UTF-8 dans le buffer de streaming (caractères multi-octets é/à/ç dans les réponses LLM). Ajout de `danger_accept_invalid_certs` dans l'assistant `mirageia setup` (étape 5b, affichée uniquement si un proxy est configuré). |
| v0.5.25 | 07/04/2026 | Ajout du proxy d'entreprise et de l'inspection SSL dans l'assistant `mirageia setup` (étapes 5 et 5b). |
| v0.5.24 | 07/04/2026 | Log des erreurs proxy via `tracing::error!` — les erreurs 502/connexion sont maintenant toujours visibles dans le terminal, même en mode `wrap`. |
| v0.5.23 | 07/04/2026 | Fix clippy : `.map().unwrap_or(false)` → `.map_or(false, ...)`. |
| v0.5.22 | 07/04/2026 | Empêche la boucle infinie d'auto-update sous Scoop/Homebrew (`is_managed_install`). Vérification de version dans le workflow de release. |
| v0.5.21 | 07/04/2026 | Démarrage automatique du proxy dans `wrap` et `console`. Affichage des erreurs API (4xx/5xx/429) dans la console. |
| v0.5.20 | 07/04/2026 | Ajout des options `upstream_proxy` et `danger_accept_invalid_certs` (proxy d'entreprise + inspection SSL). |
| v0.5.15 | 06/04/2026 | Authentification par bearer token, mode fail-open, protection SSRF. |
| v0.5.9 | 05/04/2026 | Nettoyage logs debug tokenizer. Image Docker à jour, ONNX actif en production. |
| v0.5.8 | 05/04/2026 | Fix entrypoint timeout 30s→120s. Limite RAM 1G→3G (ONNX : 946 Mo RSS + 2,1 Go VmPeak). Rebuild image Docker requis. |
| v0.5.7 | 05/04/2026 | Fix chemin modèle ONNX (`/` → `__` dans check_model_files). Fix health check entrypoint (boucle retry 30s). |
| v0.5.6 | 05/04/2026 | Auto-download modèle ONNX depuis GitHub Releases (tar.gz) avec fallback HuggingFace. Affichage modèle dans /health + console. |
| v0.5.5 | 05/04/2026 | Intégration ONNX (PiiDetector) dans le pipeline proxy — noms, dates, adresses via NER contextuel. Compilé avec `--features onnx`. Fail-open si modèle absent. |
| v0.5.0 | 04/04/2026 | Extraction texte depuis PDF (lopdf) et DOCX (zip+XML) avant pseudonymisation ; model manager CLI (`mirageia model list/download/use/delete/verify`) |
| v0.4.3 | 04/04/2026 | Fix faux positif PHONE_NUMBER sur les chiffres d'une clé API (réordonnancement des patterns : API keys avant téléphone) |
| v0.4.2 | 04/04/2026 | Ajout validateurs IBAN (MOD-97), Luhn (CB), entropie de Shannon + patterns secrets (GitHub, AWS, Stripe, Anthropic, OpenAI, JWT, Slack) |
| v0.4.1 | 04/04/2026 | Fix panic UTF-8 dans StreamBuffer (`rfind` → `char_indices().rev()`) ; fix champs SSE enrichis absents de la release v0.4.0 |
| v0.4.0 | 2026 | Version initiale déployée |
