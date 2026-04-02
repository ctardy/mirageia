# TICKET-002 : Intégration modèle ONNX pour détection PII

## Contexte
Embarquer un modèle de détection PII via ONNX Runtime dans le binaire MirageIA.

## Tâches
- [ ] Choisir le modèle initial (DistilBERT-PII recommandé pour v1)
- [ ] Convertir/télécharger le modèle au format ONNX
- [ ] Quantifier en INT8 pour réduire la taille
- [ ] Intégrer `ort` (ONNX Runtime Rust) dans le projet
- [ ] Implémenter le tokenizer (crate `tokenizers`)
- [ ] Créer le module de détection : texte → liste d'entités PII
- [ ] Tester sur un jeu de données de référence (noms, IPs, emails, etc.)
- [ ] Benchmark : latence < 50ms, précision > 90%

## Critères de validation
- Le modèle charge en < 3 secondes au démarrage
- Détecte correctement noms, emails, IPs, téléphones dans du texte libre
- Ne détecte PAS les noms célèbres dans un contexte historique/technique
- Latence d'inférence < 50ms pour un texte de 500 tokens
