use crate::detection::{PiiEntity, PiiType};
use crate::mapping::MappingTable;
use crate::pseudonymization::generator::PseudonymGenerator;

/// Enregistrement d'un remplacement effectué.
#[derive(Debug, Clone)]
pub struct ReplacementRecord {
    pub original: String,
    pub pseudonym: String,
    pub pii_type: PiiType,
    pub start: usize,
    pub end: usize,
}

/// Pseudonymise les entités PII dans un texte.
/// Les remplacements sont effectués en ordre décroissant de position
/// pour préserver les offsets.
/// Retourne le texte modifié et la liste des remplacements.
pub fn pseudonymize_text(
    text: &str,
    entities: &[PiiEntity],
    mapping: &MappingTable,
    generator: &PseudonymGenerator,
) -> (String, Vec<ReplacementRecord>) {
    if entities.is_empty() {
        return (text.to_string(), vec![]);
    }

    // Trier par position décroissante
    let mut sorted_entities: Vec<&PiiEntity> = entities.iter().collect();
    sorted_entities.sort_by(|a, b| b.start.cmp(&a.start));

    let mut result = text.to_string();
    let mut records = Vec::new();

    for entity in sorted_entities {
        // Chercher un pseudonyme existant dans le mapping (cohérence de session)
        let pseudonym = match mapping.lookup_original(&entity.text) {
            Some(existing) => existing,
            None => {
                let new_pseudo = generator.generate(&entity.entity_type, &entity.text);
                // Insérer dans le mapping
                let _ = mapping.insert(&entity.text, &new_pseudo, entity.entity_type);
                new_pseudo
            }
        };

        // Vérifier que les bornes sont valides
        if entity.start <= result.len() && entity.end <= result.len() && entity.start <= entity.end {
            result.replace_range(entity.start..entity.end, &pseudonym);

            records.push(ReplacementRecord {
                original: entity.text.clone(),
                pseudonym,
                pii_type: entity.entity_type,
                start: entity.start,
                end: entity.end,
            });
        }
    }

    // Inverser les records pour avoir l'ordre du texte (premier en premier)
    records.reverse();

    (result, records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pseudonymize_single_entity() {
        let mapping = MappingTable::new();
        let generator = PseudonymGenerator::new();

        let text = "Mon email est jean@acme.fr merci";
        let entities = vec![PiiEntity {
            text: "jean@acme.fr".to_string(),
            entity_type: PiiType::Email,
            start: 14,
            end: 26,
            confidence: 0.95,
        }];

        let (result, records) = pseudonymize_text(text, &entities, &mapping, &generator);

        assert!(!result.contains("jean@acme.fr"));
        assert!(result.contains("@example.com"));
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].original, "jean@acme.fr");
        assert!(records[0].pseudonym.contains("@example.com"));
    }

    #[test]
    fn test_pseudonymize_multiple_entities() {
        let mapping = MappingTable::new();
        let generator = PseudonymGenerator::new();

        let text = "Jean Dupont, IP: 192.168.1.50";
        let entities = vec![
            PiiEntity {
                text: "Jean".to_string(),
                entity_type: PiiType::GivenName,
                start: 0,
                end: 4,
                confidence: 0.9,
            },
            PiiEntity {
                text: "Dupont".to_string(),
                entity_type: PiiType::Surname,
                start: 5,
                end: 11,
                confidence: 0.9,
            },
            PiiEntity {
                text: "192.168.1.50".to_string(),
                entity_type: PiiType::IpAddress,
                start: 17,
                end: 29,
                confidence: 0.95,
            },
        ];

        let (result, records) = pseudonymize_text(text, &entities, &mapping, &generator);

        assert!(!result.contains("Jean"));
        assert!(!result.contains("Dupont"));
        assert!(!result.contains("192.168.1.50"));
        assert_eq!(records.len(), 3);
    }

    #[test]
    fn test_session_coherence() {
        let mapping = MappingTable::new();
        let generator = PseudonymGenerator::new();

        // Première occurrence
        let text1 = "Contact: Jean";
        let entities1 = vec![PiiEntity {
            text: "Jean".to_string(),
            entity_type: PiiType::GivenName,
            start: 9,
            end: 13,
            confidence: 0.9,
        }];
        let (result1, _) = pseudonymize_text(text1, &entities1, &mapping, &generator);
        let pseudo1 = &result1[9..];

        // Deuxième occurrence du même nom
        let text2 = "Bonjour Jean";
        let entities2 = vec![PiiEntity {
            text: "Jean".to_string(),
            entity_type: PiiType::GivenName,
            start: 8,
            end: 12,
            confidence: 0.9,
        }];
        let (result2, _) = pseudonymize_text(text2, &entities2, &mapping, &generator);
        let pseudo2 = &result2[8..];

        // Le même pseudonyme doit être utilisé (cohérence de session)
        assert_eq!(pseudo1, pseudo2);
    }

    #[test]
    fn test_pseudonymize_empty_entities() {
        let mapping = MappingTable::new();
        let generator = PseudonymGenerator::new();

        let text = "Texte sans PII";
        let (result, records) = pseudonymize_text(text, &[], &mapping, &generator);

        assert_eq!(result, text);
        assert!(records.is_empty());
    }

    #[test]
    fn test_records_ordered_by_position() {
        let mapping = MappingTable::new();
        let generator = PseudonymGenerator::new();

        let text = "A Jean B Dupont C";
        let entities = vec![
            PiiEntity {
                text: "Jean".to_string(),
                entity_type: PiiType::GivenName,
                start: 2,
                end: 6,
                confidence: 0.9,
            },
            PiiEntity {
                text: "Dupont".to_string(),
                entity_type: PiiType::Surname,
                start: 9,
                end: 15,
                confidence: 0.9,
            },
        ];

        let (_, records) = pseudonymize_text(text, &entities, &mapping, &generator);
        assert_eq!(records.len(), 2);
        // Records triés par position croissante
        assert!(records[0].start < records[1].start);
    }
}
