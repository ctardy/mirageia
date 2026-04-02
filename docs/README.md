# Documentation MirageIA

Proxy de pseudonymisation intelligent pour API LLM avec modèle embarqué.

## Structure

```
docs/
├── officiel/       Documentation technique validée
│   ├── architecture/   Architecture globale, flux, décisions
│   └── technique/      Modèle PII, proxy HTTP, mapping, chiffrement
├── recherche/      État de l'art, benchmarks, projets similaires
├── specs/          Spécifications complexes
├── tickets/        Améliorations ciblées, tâches à faire
└── errors/         Problèmes rencontrés et solutions
```

---

## Officiel

### Architecture
- [Vue d'ensemble](officiel/architecture/vue-ensemble.md) — Schéma global, composants, flux de données
- [Architecture détaillée](officiel/architecture/architecture-detaillee.md) — Fonctionnement précis de chaque composant, interactions, décisions techniques
- [Flux de pseudonymisation](officiel/architecture/flux-pseudonymisation.md) — Pipeline détection → remplacement → mapping → restauration

### Sécurité
- [Analyse RSSI](officiel/securite/analyse-rssi.md) — Risques, données qui transitent, protections, conformité RGPD/NIS2

### Technique
- [Modèle PII embarqué](officiel/technique/modele-pii.md) — Choix du modèle, ONNX Runtime, quantification, performances
- [Proxy HTTP](officiel/technique/proxy-http.md) — Interception, streaming SSE, compatibilité providers

---

## Recherche

- [État de l'art](recherche/etat-de-lart.md) — Projets existants, comparatifs, inspirations

---

## Specs

Spécifications complexes (fonctionnalités multi-couches).

---

## Tickets

Tâches ciblées, améliorations, correctifs.
