# État de l'art — Projets similaires et concurrents

> Dernière mise à jour : 2 avril 2026

---

## Concurrents open-source

### PasteGuard — Proxy + dashboard + extension navigateur
- **GitHub** : `sgasser/pasteguard` — ~570 stars, très actif
- **Stack** : TypeScript, proxy HTTP local (port 3000)
- **Détection** : Microsoft Presidio (NER + regex) — 30+ types de PII, 24 langues
- **Réversibilité** : ✅ Mapping bidirectionnel
- **Providers** : OpenAI, Anthropic, Open WebUI
- **Bonus** : Dashboard web, extension navigateur, "Route Mode" Ollama
- **Limite** : détection regex/NER seulement, pas de LLM embarqué, nécessite Node.js

### CloakPipe — Le plus proche techniquement de MirageIA
- **GitHub** : `rohansx/cloakpipe` — Rust, binaire unique
- **Détection** : Pipeline 5 couches (regex → checksums → NER ONNX DistilBERT → résolution floue → règles TOML)
- **Réversibilité** : ✅ Vault AES-256-GCM, streaming SSE
- **Latence** : < 5-20ms
- **Providers** : tout provider compatible OpenAI
- **Limite** : NER classique (pas de compréhension contextuelle profonde via LLM)

### Microsoft Presidio — La référence de détection PII
- **GitHub** : `microsoft/presidio` — ~7 500 stars, très actif
- **Stack** : Python, spaCy, Transformers
- **Architecture** : Bibliothèque modulaire (Analyzer + Anonymizer), déployable comme microservice
- **Détection** : NLP (spaCy, Transformers) + pattern matching (regex)
- **Types PII** : noms, emails, téléphones, cartes bancaires, IBAN, adresses, SSN, etc.
- **Réversibilité** : possible via opérateurs personnalisés
- **Limite** : bibliothèque Python, pas un proxy, pas de streaming natif

### LLM Guard (Protect AI) — Sécurité complète pour LLM
- **GitHub** : `protectai/llm-guard` — ~2 800 stars, très actif
- **Stack** : Python, Transformers, Presidio
- **Architecture** : Pipeline de scanners modulaires, API REST disponible (Docker)
- **Détection** : 15 scanners d'entrée + 20 de sortie (PII, prompt injection, toxicité, secrets, URLs malveillantes)
- **Réversibilité** : ✅ Vault (mapping réversible)
- **Limite** : Python, pas de proxy transparent, nécessite intégration dans le code

### LiteLLM + Microsoft Presidio — Solution proxy entreprise
- **GitHub** : `BerriAI/litellm` — 20k+ stars
- **Stack** : Python, Docker
- **Approche** : Presidio comme guardrail dans un proxy OpenAI-compatible, configuration YAML
- **Réversibilité** : ✅ (`output_parse_pii = True`)
- **Providers** : 100+ (OpenAI, Anthropic, Azure, Bedrock, Vertex, etc.)
- **Limite** : lourd (Docker, Python, YAML), pas de modèle embarqué, pas autonome

### NeMo Guardrails (NVIDIA) — Guardrails programmables
- **GitHub** : `NVIDIA-NeMo/Guardrails` — ~5 900 stars, très actif
- **Stack** : Python
- **Architecture** : Middleware Python avec langage de configuration Colang
- **Détection PII** : déléguée à Presidio
- **Bonus** : filtrage de contenu, détection de jailbreak, contrôle de topique
- **Limite** : orienté sécurité conversationnelle, détection PII basique et déléguée

### AnonymizerSLM (Eternis AI) — L'approche LLM contextuel
- **Source** : Hugging Face (`pratyushrt/anonymizerslm`) — modèles Qwen3 0.6B / 1.7B / 4B
- **Approche** : petit LLM entraîné spécifiquement pour détecter et pseudonymiser les PII
- **Intelligence** : comprend le contexte (ne masque pas "Thomas Edison" dans un cours d'histoire)
- **Score** : 9.55/10 (GPT-4.1 = 9.77/10)
- **Limite** : modèle seul, pas de proxy intégré, pas d'application standalone, nécessite intégration manuelle

### PII Masker (HydroXai) — Modèle DeBERTa fine-tuné
- **GitHub** : `HydroXai/pii-masker` — ~160 stars, actif
- **Stack** : Python, PyTorch, DeBERTa-v3
- **Approche** : modèle NER Transformer fine-tuné pour la détection PII
- **Limite** : masquage uniquement (pas de pseudonymisation réversible), orienté batch, pas streaming

### anonLLM — SDK minimaliste
- **GitHub** : `fsndzomga/anonLLM` — Python, PyPI
- **Détection** : regex + heuristiques (noms, emails, téléphones)
- **Réversibilité** : ✅ anonymisation réversible
- **Providers** : OpenAI (GPT-3/4)
- **Limite** : très limité en types de PII, pas de proxy, pas de streaming

### PII Codex (EdyVision) — Analyse et scoring de sévérité
- **GitHub** : `EdyVision/pii-codex` — ~98 stars, actif
- **Stack** : Python, Poetry
- **Approche** : surcouche analytique au-dessus de Presidio, scoring de risque par type de PII
- **Limite** : orienté recherche, pas production

### Guardrails AI detect_pii — Validateur binaire
- **GitHub** : `guardrails-ai/detect_pii` — ~15 stars
- **Approche** : validateur binaire (contient PII ou non) dans l'écosystème Guardrails AI
- **Limite** : trop basique, simple détection binaire, pas de pseudonymisation

---

## Concurrents commerciaux (SaaS / API)

### Lakera Guard
- **URL** : lakera.ai
- **Modèle** : SaaS + self-hosted (~500$/mois)
- **Détection** : prompt injection + PII (CB, SSN, IBAN), contenu toxique, liens malveillants
- **Latence** : < 50ms, intégration en 1 ligne de code
- **Différenciateur** : faux positifs réduits de 90% vs concurrents
- **Limite** : commercial, pas de pseudonymisation réversible native

### Protecto AI
- **URL** : protecto.ai
- **Modèle** : SaaS / SDK
- **Détection** : moteur DeepSight (modèles transformer), comprend la sémantique du texte non structuré
- **Bonus** : tokenisation préservant le sens sémantique, RBAC pour agents AI, conformité HIPAA/GDPR/PDPL
- **Providers** : OpenAI, Anthropic
- **Limite** : commercial, SaaS (dépendance externe)

### Granica Screen
- **URL** : docs.granica.ai
- **Modèle** : service cloud (AWS, GCP, Azure)
- **Détection** : algorithme propriétaire, multi-langues, multi-formats
- **Bonus** : couvre aussi les données d'entraînement (pas seulement l'inférence), conformité GDPR/CCPA/HIPAA
- **Différenciateur** : 5-10x plus efficace en coût de scan que les concurrents
- **Limite** : cloud uniquement, pas de binaire local

### Private AI (Limina)
- **URL** : private-ai.com
- **Modèle** : API SaaS + on-premise
- **Détection** : 50+ types d'entités (PII, PHI, PCI), 52 langues, multi-formats (texte, PDF, images, audio)
- **Bonus** : détermination d'expert HIPAA, IA contextuelle (noms ambigus, entités multi-phrases)
- **Limite** : commercial, installation on-premise lourde

### DataHawk (LLM Shield)
- **URL** : datahawk.io
- **Modèle** : SaaS Gateway / SDK (Python, Java, Node.js)
- **Détection** : propriétaire, 10 000+ docs/sec
- **Modes** : MASK, REPLACE, HASH, TOKEN
- **Latence** : 12ms
- **Bonus** : tracking de session, audit trail complet, conformité PCI-DSS + GDPR
- **Limite** : commercial, SaaS

---

## Tableau comparatif — Fonctionnalités clés

| Critère | PasteGuard | CloakPipe | Presidio | LLM Guard | LiteLLM | AnonymizerSLM | **MirageIA** |
|---------|------------|-----------|----------|-----------|---------|---------------|-------------|
| LLM embarqué | ❌ | ❌ (NER) | ❌ | ❌ | ❌ | ✅ (modèle seul) | **✅** |
| Détection contextuelle | ❌ | Partielle | ❌ | ❌ | ❌ | **✅** | **✅** |
| Proxy HTTP transparent | ✅ | ✅ | ❌ | ❌ | ✅ | ❌ | **✅** |
| Zéro dépendance | ❌ (Node) | ✅ | ❌ (Python) | ❌ (Python) | ❌ (Docker) | ❌ | **✅** |
| Binaire unique | ❌ | ✅ | ❌ | ❌ | ❌ | ❌ | **✅** |
| Streaming SSE | ✅ | ✅ | ❌ | ❌ | ✅ | ❌ | **✅** |
| Réversibilité | ✅ | ✅ | Partielle | ✅ | ✅ | ✅ | **✅** |
| Dashboard | ✅ | ❌ | ❌ | ❌ | ✅ | ❌ | **✅** |
| Chiffrement mapping | ❌ | ✅ (AES-256) | ❌ | ❌ | ❌ | ❌ | **✅** (AES-256) |

---

## Positionnement unique de MirageIA

**Aucun concurrent ne combine l'ensemble de ces caractéristiques :**

1. **LLM embarqué (ONNX)** pour une détection contextuelle intelligente — seul AnonymizerSLM offre cette capacité, mais sans proxy ni application
2. **Proxy HTTP transparent** — CloakPipe et PasteGuard l'offrent, mais avec une détection NER/regex classique
3. **Binaire unique Rust sans dépendances** — seul CloakPipe s'en approche (Rust), mais sans LLM embarqué
4. **Mapping chiffré AES-256 en mémoire** — seul CloakPipe fait du chiffrement, mais sans LLM contextuel
5. **Zéro config** — fonctionne out-of-the-box entre l'app et l'API

**MirageIA = AnonymizerSLM (intelligence) + CloakPipe (architecture) + PasteGuard (UX)**

---

## Inspirations techniques

- **Murmure** (`Kieirra/murmure`) : modèle Whisper embarqué via ONNX dans Tauri/Rust — même approche pour embarquer le modèle PII
- **AI Dev Bridge** (`anas-zahouri/ai-dev-bridge`) : extension Claude Code qui intercepte les appels API — inspiration pour le mécanisme d'interception
- **CloakPipe** : architecture Rust du proxy, gestion du streaming SSE, vault chiffré
- **AnonymizerSLM** : approche de détection PII par LLM contextuel, modèles quantifiés (0.6B-4B)
- **LLM Guard** : concept de vault pour la pseudonymisation réversible, pipeline de scanners modulaires
