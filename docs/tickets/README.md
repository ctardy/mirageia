# Tickets MirageIA

## Backlog

| # | Titre | Priorité | Statut | Notes |
|---|-------|----------|--------|-------|
| 001 | [Setup projet Rust + Tauri](TICKET-001-SETUP-PROJET-RUST-TAURI.md) | Haute | ✅ Terminé (partiel) | CLI Rust pur, Tauri reporté Phase 4 |
| 002 | [Intégration modèle ONNX](TICKET-002-INTEGRATION-MODELE-ONNX.md) | Haute | 🔧 Structuré | Code prêt, bloqué MSVC. Détecteur regex en fallback |
| 003 | [Pipeline pseudonymisation](TICKET-003-PIPELINE-PSEUDONYMISATION.md) | Haute | ✅ Terminé | Mapping AES-256-GCM, générateurs, SSE buffer |

## Tickets ajoutés en cours de développement

| # | Titre | Priorité | Statut |
|---|-------|----------|--------|
| 004 | Config TOML + whitelist | Moyenne | ✅ Terminé |
| 005 | Tests e2e avec mock server | Moyenne | ✅ Terminé |
| 006 | Dashboard Tauri (Phase 4) | Basse | 📋 Planifié |
| 007 | Détection contextuelle ONNX (Phase 2 v2) | Haute | 📋 Planifié |

## Écarts par rapport au plan initial

### CLI au lieu de Tauri (TICKET-001)

**Décision** : démarrer en CLI Rust pur pour accélérer le prototypage. La migration vers Tauri est reportée en Phase 4.

**Raison** : Tauri ajoute de la complexité sans valeur ajoutée pour valider le pipeline.

### Détecteur regex au lieu d'ONNX (TICKET-002)

**Décision** : détecteur regex comme fallback. Le code ONNX est structuré et feature-gated (`--features onnx`).

**Raison** : ONNX Runtime ne fournit pas de binaires pour `x86_64-pc-windows-gnu`. Le détecteur regex couvre les PII à pattern fixe et permet un proxy 100% fonctionnel.

**Manque** : détection de noms de personnes, compréhension du contexte.

### Modèle PII : piiranha (DeBERTa-v2)

**Découverte** : le meilleur candidat est `iiiorg/piiranha-v1-detect-personal-information` (DeBERTa-v2, 1.1 Go, 18 labels). Licence `cc-by-nc-nd-4.0` (non-commercial) — à évaluer.
