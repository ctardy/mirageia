# État de l'art — Projets similaires

## Projets existants analysés (avril 2026)

### PasteGuard — Le plus complet (proxy + dashboard)
- **GitHub** : `sgasser/pasteguard` — 570 stars, actif
- **Stack** : TypeScript, proxy HTTP local (port 3000)
- **Détection** : Microsoft Presidio (NER + regex) — 30+ types de PII, 24 langues
- **Réversibilité** : ✅ Mapping bidirectionnel
- **Providers** : OpenAI, Anthropic
- **Bonus** : Dashboard, extension navigateur, "Route Mode" Ollama
- **Limite** : détection regex/NER, pas de LLM embarqué

### CloakPipe — Le plus sophistiqué techniquement
- **GitHub** : `rohansx/cloakpipe` — Rust, binaire unique
- **Détection** : Pipeline 5 couches (regex → checksums → NER ONNX DistilBERT → résolution floue → règles TOML)
- **Réversibilité** : ✅ Vault AES-256-GCM, streaming SSE
- **Latence** : < 20ms
- **Limite** : NER classique (pas de compréhension contextuelle profonde)

### LiteLLM + Microsoft Presidio — Solution entreprise
- **GitHub** : `BerriAI/litellm` — 20k+ stars
- **Approche** : Presidio comme guardrail dans un proxy OpenAI-compatible
- **Réversibilité** : ✅ (`output_parse_pii = True`)
- **Providers** : 100+
- **Limite** : lourd (Docker, Python, YAML), pas embarqué

### AnonymizerSLM — Approche LLM local (la plus intelligente)
- **Source** : Hugging Face (Eternis AI) — modèles Qwen3 0.6B / 1.7B / 4B
- **Approche** : Petit LLM entraîné spécifiquement pour détecter et pseudonymiser les PII
- **Intelligence** : comprend le contexte (ne masque pas "Thomas Edison")
- **Score** : 9.55/10 (GPT-4.1 = 9.77/10)
- **Limite** : pas de proxy intégré, pas d'app standalone

## Positionnement de MirageIA

| Critère | PasteGuard | CloakPipe | LiteLLM | AnonymizerSLM | **MirageIA** |
|---------|------------|-----------|---------|---------------|-------------|
| LLM embarqué | ❌ | ❌ (NER) | ❌ | ✅ (modèle seul) | ✅ |
| Zéro dépendance | ❌ (Node) | ✅ | ❌ (Docker) | ❌ | ✅ |
| Proxy intégré | ✅ | ✅ | ✅ | ❌ | ✅ |
| Détection contextuelle | ❌ | Partielle | ❌ | ✅ | ✅ |
| Streaming SSE | ✅ | ✅ | ✅ | ❌ | ✅ |
| Dashboard | ✅ | ❌ | ✅ | ❌ | ✅ |
| Réversibilité | ✅ | ✅ | ✅ | ✅ | ✅ |

**MirageIA est le premier projet à combiner** : LLM embarqué (ONNX) + proxy HTTP + pseudonymisation réversible + streaming SSE + dashboard — dans un seul binaire sans dépendances.

## Inspirations techniques

- **Murmure** (`Kieirra/murmure`) : modèle Whisper embarqué via ONNX dans Tauri/Rust — même approche pour embarquer le modèle PII
- **AI Dev Bridge** (`anas-zahouri/ai-dev-bridge`) : extension Claude Code qui intercepte les appels API — inspiration pour le mécanisme d'interception
- **CloakPipe** : architecture Rust du proxy, gestion du streaming SSE, vault chiffré
