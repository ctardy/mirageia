# TICKET-001 : Setup projet Rust + Tauri

## Contexte
Initialiser la structure du projet MirageIA avec Rust et Tauri.

## Tâches
- [ ] Initialiser le projet Tauri (`cargo create-tauri-app`)
- [ ] Configurer la structure de dossiers (src-tauri, src pour le front webview)
- [ ] Ajouter les dépendances de base dans `Cargo.toml` :
  - `axum` ou `hyper` (proxy HTTP)
  - `ort` (ONNX Runtime)
  - `tokenizers` (HuggingFace)
  - `reqwest` (client HTTP)
  - `tokio` (runtime async)
  - `aes-gcm` (chiffrement mapping)
- [ ] Créer un proxy HTTP minimal (hello world qui forwarde les requêtes)
- [ ] Tray icon basique
- [ ] CI : `cargo build` + `cargo test` sur GitHub Actions

## Critères de validation
- Le binaire se lance et affiche un tray icon
- Le proxy écoute sur `localhost:3100` et forwarde vers `api.anthropic.com`
- `cargo test` passe
