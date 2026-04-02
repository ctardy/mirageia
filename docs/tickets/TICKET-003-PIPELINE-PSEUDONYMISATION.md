# TICKET-003 : Pipeline de pseudonymisation réversible

## Contexte
Implémenter le cœur de MirageIA : la pseudonymisation et la dé-pseudonymisation.

## Tâches
- [ ] Table de mapping en mémoire (HashMap chiffré AES-256-GCM)
- [ ] Générateur de pseudonymes par type (noms, IPs, emails, etc.)
- [ ] Dictionnaires intégrés (noms, prénoms par culture)
- [ ] Remplacement dans le texte (gestion des positions/offsets)
- [ ] Cohérence de session (même PII = même pseudonyme)
- [ ] Dé-pseudonymisation dans les réponses
- [ ] Gestion du streaming SSE (buffer + détection multi-tokens)
- [ ] Tests unitaires sur chaque type de PII
- [ ] Tests d'intégration (requête complète → pseudonymisation → dé-pseudonymisation)

## Critères de validation
- Round-trip parfait : texte → pseudonymiser → dé-pseudonymiser → texte original identique
- Streaming : les pseudonymes split entre tokens sont correctement détectés
- Le mapping est chiffré en mémoire et jamais écrit sur disque
