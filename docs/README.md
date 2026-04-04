# Documentation MirageIA

Proxy de pseudonymisation intelligent pour API LLM.

## Structure

```
docs/
├── officiel/           Documentation technique validée
│   ├── architecture/   Architecture globale, flux, décisions
│   ├── technique/      Modèle PII, proxy HTTP
│   ├── securite/       Analyse RSSI, conformité RGPD
│   ├── installation.md Guide d'installation complet
│   └── contribution.md Guide de contribution et mise à jour
├── recherche/          État de l'art, benchmarks, projets similaires
├── tickets/            Tâches, backlog, décisions
└── README.md           Cet index
```

---

## Guides pratiques

- **[Installation](officiel/installation.md)** — Prérequis, installation Rust/GCC, build, premier lancement, résolution de problèmes
- **[Distribution & installeur](officiel/distribution.md)** — Binaires précompilés, script d'install, installeur Tauri, CI/CD, mise à jour
- **[Contribution & mise à jour](officiel/contribution.md)** — Workflow dev, comment ajouter un type PII, modifier le pipeline, process de release

---

## Officiel

### Architecture
- [Vue d'ensemble](officiel/architecture/vue-ensemble.md) — Schéma global, composants, flux de données
- [Architecture détaillée](officiel/architecture/architecture-detaillee.md) — Fonctionnement de chaque composant, interactions, modules Rust
- [Flux de pseudonymisation](officiel/architecture/flux-pseudonymisation.md) — Pipeline détection → remplacement → mapping → restauration

### Sécurité
- [Analyse RSSI](officiel/securite/analyse-rssi.md) — Risques, données qui transitent, protections, conformité RGPD/NIS2

### Technique
- [Modèle PII embarqué](officiel/technique/modele-pii.md) — Choix du modèle, ONNX Runtime, quantification, performances
- [Proxy HTTP](officiel/technique/proxy-http.md) — Interception, streaming SSE, compatibilité providers

---

## Recherche

- [État de l'art](recherche/etat-de-lart.md) — Projets existants, comparatifs, inspirations
- [Extension SaaS](recherche/extension-saas.md) — Analyse d'une extension vers un modèle SaaS hybride

---

## Tickets

- [Backlog & décisions](tickets/README.md) — État des tickets, écarts par rapport au plan initial

---

## Code source

Le code est organisé en 7 modules Rust :

| Module | Fichier principal | Rôle |
|---|---|---|
| `config` | `src/config/settings.rs` | Configuration TOML + env vars |
| `proxy` | `src/proxy/server.rs` | Handler HTTP axum, pipeline complet |
| `detection` | `src/detection/regex_detector.rs` | Détection PII par regex (v1) |
| `mapping` | `src/mapping/table.rs` | Table bidirectionnelle chiffrée AES-256-GCM |
| `pseudonymization` | `src/pseudonymization/generator.rs` | Génération de pseudonymes, remplacement, restauration |
| `streaming` | `src/streaming/buffer.rs` | Buffer SSE pour pseudonymes split entre tokens |
| `detection` (ONNX) | `src/detection/model.rs` | Modèle ONNX (feature-gated, v2) |

133 tests couvrent l'ensemble du pipeline (voir [README principal](../README.md#tests)).
