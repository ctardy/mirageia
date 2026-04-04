# Documentation MirageIA

Proxy de pseudonymisation intelligent pour API LLM.
Intelligent pseudonymization proxy for LLM APIs.

---

## Langues / Languages

| Francais | English |
|----------|---------|
| [Documentation complete](fr/) | [Full documentation](en/) |

---

## Structure

```
docs/
├── fr/                 Documentation francaise
│   ├── architecture/   Architecture globale, flux, décisions
│   ├── technique/      Modèle PII, proxy HTTP
│   ├── securite/       Analyse RSSI, conformité RGPD
│   ├── installation.md
│   ├── deploiement-ops.md
│   ├── distribution.md
│   └── contribution.md
├── en/                 English documentation
│   ├── architecture/   System architecture, flows, decisions
│   ├── technical/      PII model, HTTP proxy
│   ├── security/       Security analysis, GDPR compliance
│   ├── installation.md
│   ├── deployment-ops.md
│   ├── distribution.md
│   └── contributing.md
├── recherche/          Recherche interne / Internal research
├── tickets/            Backlog, tickets
└── README.md           Cet index / This index
```

---

## Francais

### Guides pratiques
- **[Installation](fr/installation.md)** — Prérequis, installation Rust/GCC, build, premier lancement, résolution de problèmes
- **[Déploiement ops (Docker + Apache)](fr/deploiement-ops.md)** — Docker, reverse proxy Apache, dashboard web, monitoring, sécurité
- **[Distribution & installeur](fr/distribution.md)** — Binaires précompilés, script d'install, installeur Tauri, CI/CD, mise à jour
- **[Contribution](fr/contribution.md)** — Workflow dev, comment ajouter un type PII, modifier le pipeline, process de release
- **[Dev Linux](fr/dev-linux.md)** — Setup complet d'un poste de dev Linux/WSL : Rust, build, tests, clippy, workflow

### Architecture
- [Vue d'ensemble](fr/architecture/vue-ensemble.md) — Schéma global, composants, flux de données
- [Architecture détaillée](fr/architecture/architecture-detaillee.md) — Fonctionnement de chaque composant, interactions, modules Rust
- [Flux de pseudonymisation](fr/architecture/flux-pseudonymisation.md) — Pipeline détection → remplacement → mapping → restauration

### Sécurité
- [Analyse RSSI](fr/securite/analyse-rssi.md) — Risques, données qui transitent, protections, conformité RGPD/NIS2

### Technique
- [Modèle PII embarqué](fr/technique/modele-pii.md) — Choix du modèle, ONNX Runtime, quantification, performances
- [Proxy HTTP](fr/technique/proxy-http.md) — Interception, streaming SSE, compatibilité providers

---

## English

### Practical Guides
- **[Installation](en/installation.md)** — Prerequisites, Rust/GCC setup, build, first launch, troubleshooting
- **[Ops Deployment (Docker + Apache)](en/deployment-ops.md)** — Docker, Apache reverse proxy, web dashboard, monitoring, security
- **[Distribution & Installer](en/distribution.md)** — Precompiled binaries, install script, Tauri installer, CI/CD, updates
- **[Contributing](en/contributing.md)** — Dev workflow, adding PII types, modifying the pipeline, release process
- **[Dev Linux](en/dev-linux.md)** — Full Linux/WSL dev setup: Rust, build, tests, clippy, workflow

### Architecture
- [Overview](en/architecture/overview.md) — System diagram, components, data flow
- [Detailed Architecture](en/architecture/detailed-architecture.md) — Component internals, interactions, Rust modules
- [Pseudonymization Flow](en/architecture/pseudonymization-flow.md) — Pipeline: detection → replacement → mapping → restoration

### Security
- [Security Analysis](en/security/security-analysis.md) — Risks, data in transit, protections, GDPR/NIS2 compliance

### Technical
- [Embedded PII Model](en/technical/pii-model.md) — Model choice, ONNX Runtime, quantization, performance
- [HTTP Proxy](en/technical/http-proxy.md) — Interception, SSE streaming, provider compatibility

---

## Research / Recherche

- [État de l'art](recherche/etat-de-lart.md) — Projets existants, comparatifs, inspirations
- [Extension SaaS](recherche/extension-saas.md) — Analyse d'une extension vers un modèle SaaS hybride

---

## Tickets

- [Backlog & decisions](tickets/README.md)

---

## Code source

7 Rust modules / 145 tests:

| Module | Main file | Role |
|---|---|---|
| `config` | `src/config/settings.rs` | TOML + env vars configuration |
| `proxy` | `src/proxy/server.rs` | HTTP handler (axum), full pipeline, dashboard |
| `detection` | `src/detection/regex_detector.rs` | PII detection via regex (v1) |
| `mapping` | `src/mapping/table.rs` | Bidirectional encrypted table (AES-256-GCM) |
| `pseudonymization` | `src/pseudonymization/generator.rs` | Pseudonym generation, replacement, restoration |
| `streaming` | `src/streaming/buffer.rs` | SSE buffer for split pseudonyms between tokens |
| `detection` (ONNX) | `src/detection/model.rs` | ONNX model (feature-gated, v2) |
