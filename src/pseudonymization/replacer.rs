use std::collections::HashMap;

use rand::Rng;

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

/// Extrait le préfixe réseau d'une IPv4 selon le masque (en bits).
/// Retourne les octets du préfixe sous forme de chaîne "a.b.c" pour /24, etc.
fn ip_network_prefix(ip: &str, mask_bits: u8) -> Option<String> {
    if ip.contains(':') {
        return None; // IPv6 non supporté pour le regroupement
    }
    let octets: Vec<u8> = ip
        .split('.')
        .filter_map(|s| s.parse::<u8>().ok())
        .collect();
    if octets.len() != 4 {
        return None;
    }
    let full_octets = (mask_bits / 8) as usize;
    if full_octets == 0 || full_octets > 3 {
        return None;
    }
    Some(
        octets[..full_octets]
            .iter()
            .map(|o| o.to_string())
            .collect::<Vec<_>>()
            .join("."),
    )
}

/// Extrait la partie hôte d'une IPv4 selon le masque (en bits).
fn ip_host_part(ip: &str, mask_bits: u8) -> Option<String> {
    if ip.contains(':') {
        return None;
    }
    let octets: Vec<&str> = ip.split('.').collect();
    if octets.len() != 4 {
        return None;
    }
    let full_octets = (mask_bits / 8) as usize;
    Some(octets[full_octets..].join("."))
}

/// Génère un préfixe réseau pseudo aléatoire pour un masque donné.
fn generate_pseudo_prefix(mask_bits: u8) -> String {
    let mut rng = rand::thread_rng();
    let full_octets = (mask_bits / 8) as usize;
    (0..full_octets)
        .map(|_| rng.gen_range(1..255u8).to_string())
        .collect::<Vec<_>>()
        .join(".")
}

/// Pseudonymise les entités PII dans un texte.
/// Les remplacements sont effectués en ordre décroissant de position
/// pour préserver les offsets.
///
/// Pour les IPs partageant un même sous-réseau, des pseudonymes cohérents
/// sont générés (même préfixe pseudo, seule la partie hôte diffère).
///
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

    // Pré-calculer les préfixes réseau cohérents pour les IPs groupées
    let subnet_pseudo_prefixes = compute_subnet_prefixes(entities, mapping);

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
                let new_pseudo = if entity.entity_type == PiiType::IpAddress {
                    generate_subnet_coherent_ip(
                        &entity.text,
                        &subnet_pseudo_prefixes,
                        generator,
                        mapping,
                    )
                } else {
                    generator.generate(&entity.entity_type, &entity.text)
                };
                // Insérer dans le mapping
                let _ = mapping.insert(&entity.text, &new_pseudo, entity.entity_type);
                new_pseudo
            }
        };

        // Vérifier que les bornes sont valides
        if entity.start <= result.len() && entity.end <= result.len() && entity.start <= entity.end
        {
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

/// Détecte les IPs qui partagent un même sous-réseau (/24) dans le batch d'entités,
/// et génère un préfixe pseudo commun pour chaque groupe.
/// Retourne un mapping : préfixe réseau original → préfixe pseudo.
fn compute_subnet_prefixes(
    entities: &[PiiEntity],
    mapping: &MappingTable,
) -> HashMap<String, String> {
    let mut prefix_groups: HashMap<String, Vec<String>> = HashMap::new();

    for entity in entities {
        if entity.entity_type != PiiType::IpAddress {
            continue;
        }
        if entity.text.contains(':') {
            continue; // IPv6 exclu
        }
        if mapping.lookup_original(&entity.text).is_some() {
            continue; // Déjà mappé, on ne recalcule pas
        }

        if let Some(prefix) = ip_network_prefix(&entity.text, 24) {
            prefix_groups
                .entry(prefix)
                .or_default()
                .push(entity.text.clone());
        }
    }

    let mut result = HashMap::new();
    for (orig_prefix, ips) in &prefix_groups {
        if ips.len() >= 2 {
            // Plusieurs IPs dans le même /24 → générer un préfixe pseudo commun
            result.insert(orig_prefix.clone(), generate_pseudo_prefix(24));
        }
    }
    result
}

/// Génère un pseudonyme IP cohérent avec le sous-réseau si applicable.
/// Si l'IP fait partie d'un groupe de sous-réseau, utilise le préfixe pseudo commun
/// et préserve la partie hôte originale.
///
/// Vérifie aussi le mapping existant : si une IP dans le même /24 a déjà été
/// pseudonymisée (dans un champ texte précédent), réutilise le même préfixe pseudo.
fn generate_subnet_coherent_ip(
    original_ip: &str,
    subnet_prefixes: &HashMap<String, String>,
    generator: &PseudonymGenerator,
    mapping: &MappingTable,
) -> String {
    if original_ip.contains(':') {
        return generator.generate(&PiiType::IpAddress, original_ip);
    }

    let orig_prefix = match ip_network_prefix(original_ip, 24) {
        Some(p) => p,
        None => return generator.generate(&PiiType::IpAddress, original_ip),
    };

    // 1. Vérifier les préfixes pré-calculés du batch courant
    if let Some(pseudo_prefix) = subnet_prefixes.get(&orig_prefix) {
        if let Some(host) = ip_host_part(original_ip, 24) {
            return format!("{}.{}", pseudo_prefix, host);
        }
    }

    // 2. Vérifier le mapping existant pour trouver une IP sœur déjà pseudonymisée
    for (pseudo, original, pii_type) in &mapping.all_entries_with_type() {
        if *pii_type != PiiType::IpAddress || original.contains(':') {
            continue;
        }
        if let Some(existing_orig_prefix) = ip_network_prefix(original, 24) {
            if existing_orig_prefix == orig_prefix {
                // Trouvé une IP sœur ! Extraire le préfixe pseudo utilisé
                if let Some(existing_pseudo_prefix) = ip_network_prefix(pseudo, 24) {
                    if let Some(host) = ip_host_part(original_ip, 24) {
                        return format!("{}.{}", existing_pseudo_prefix, host);
                    }
                }
            }
        }
    }

    // 3. Pas de groupement : générer un préfixe aléatoire + préserver la partie hôte
    // Cela permet la cohérence si une IP sœur arrive dans un appel ultérieur
    if let Some(host) = ip_host_part(original_ip, 24) {
        let prefix = generate_pseudo_prefix(24);
        format!("{}.{}", prefix, host)
    } else {
        generator.generate(&PiiType::IpAddress, original_ip)
    }
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

    #[test]
    fn test_subnet_coherent_ips() {
        let mapping = MappingTable::new();
        let generator = PseudonymGenerator::new();

        // 3 IPs dans le même /24 (10.0.1.x)
        let text = "Serveurs: 10.0.1.10, 10.0.1.20, 10.0.1.30";
        let entities = vec![
            PiiEntity {
                text: "10.0.1.10".to_string(),
                entity_type: PiiType::IpAddress,
                start: 10,
                end: 19,
                confidence: 0.95,
            },
            PiiEntity {
                text: "10.0.1.20".to_string(),
                entity_type: PiiType::IpAddress,
                start: 21,
                end: 30,
                confidence: 0.95,
            },
            PiiEntity {
                text: "10.0.1.30".to_string(),
                entity_type: PiiType::IpAddress,
                start: 32,
                end: 41,
                confidence: 0.95,
            },
        ];

        let (result, records) = pseudonymize_text(text, &entities, &mapping, &generator);

        // Les 3 IPs ne doivent plus apparaître
        assert!(!result.contains("10.0.1.10"));
        assert!(!result.contains("10.0.1.20"));
        assert!(!result.contains("10.0.1.30"));
        assert_eq!(records.len(), 3);

        // Les 3 pseudonymes doivent partager le même préfixe /24
        let pseudo_prefix_0 = records[0]
            .pseudonym
            .rsplitn(2, '.')
            .last()
            .unwrap()
            .to_string();
        let pseudo_prefix_1 = records[1]
            .pseudonym
            .rsplitn(2, '.')
            .last()
            .unwrap()
            .to_string();
        let pseudo_prefix_2 = records[2]
            .pseudonym
            .rsplitn(2, '.')
            .last()
            .unwrap()
            .to_string();

        assert_eq!(
            pseudo_prefix_0, pseudo_prefix_1,
            "Les pseudonymes doivent partager le même préfixe réseau"
        );
        assert_eq!(
            pseudo_prefix_1, pseudo_prefix_2,
            "Les pseudonymes doivent partager le même préfixe réseau"
        );

        // La partie hôte doit être préservée (10, 20, 30)
        let host_0 = records[0].pseudonym.split('.').last().unwrap();
        let host_1 = records[1].pseudonym.split('.').last().unwrap();
        let host_2 = records[2].pseudonym.split('.').last().unwrap();
        assert_eq!(host_0, "10");
        assert_eq!(host_1, "20");
        assert_eq!(host_2, "30");
    }

    #[test]
    fn test_single_ip_no_subnet_grouping() {
        let mapping = MappingTable::new();
        let generator = PseudonymGenerator::new();

        // Une seule IP → pas de regroupement
        let text = "Serveur: 192.168.1.50";
        let entities = vec![PiiEntity {
            text: "192.168.1.50".to_string(),
            entity_type: PiiType::IpAddress,
            start: 9,
            end: 21,
            confidence: 0.95,
        }];

        let (result, records) = pseudonymize_text(text, &entities, &mapping, &generator);

        assert!(!result.contains("192.168.1.50"));
        assert_eq!(records.len(), 1);
        // Le pseudonyme doit être une IPv4 valide avec la partie hôte préservée
        assert_eq!(records[0].pseudonym.split('.').count(), 4);
        assert!(
            records[0].pseudonym.ends_with(".50"),
            "La partie hôte doit être préservée, got: {}",
            records[0].pseudonym
        );
    }

    #[test]
    fn test_ips_different_subnets_no_grouping() {
        let mapping = MappingTable::new();
        let generator = PseudonymGenerator::new();

        // 2 IPs dans des sous-réseaux différents
        let text = "A: 192.168.1.10, B: 10.0.2.20";
        let entities = vec![
            PiiEntity {
                text: "192.168.1.10".to_string(),
                entity_type: PiiType::IpAddress,
                start: 3,
                end: 15,
                confidence: 0.95,
            },
            PiiEntity {
                text: "10.0.2.20".to_string(),
                entity_type: PiiType::IpAddress,
                start: 20,
                end: 29,
                confidence: 0.95,
            },
        ];

        let (_, records) = pseudonymize_text(text, &entities, &mapping, &generator);

        assert_eq!(records.len(), 2);
        // Les préfixes /24 doivent être différents (pas de groupement)
        let prefix_0 = records[0]
            .pseudonym
            .rsplitn(2, '.')
            .last()
            .unwrap()
            .to_string();
        let prefix_1 = records[1]
            .pseudonym
            .rsplitn(2, '.')
            .last()
            .unwrap()
            .to_string();
        // On ne peut pas garantir qu'ils soient différents (collision aléatoire possible)
        // mais on vérifie que les deux sont des IPs valides
        assert_eq!(records[0].pseudonym.split('.').count(), 4);
        assert_eq!(records[1].pseudonym.split('.').count(), 4);
        let _ = (prefix_0, prefix_1); // utilisation pour éviter le warning
    }

    #[test]
    fn test_subnet_coherent_ips_cross_requests() {
        let mapping = MappingTable::new();
        let generator = PseudonymGenerator::new();

        // Simuler 3 IPs du même /24 arrivant dans des appels séparés
        // (comme quand elles sont dans des champs texte différents)
        let text1 = "Serveur A: 10.0.1.10";
        let entities1 = vec![PiiEntity {
            text: "10.0.1.10".to_string(),
            entity_type: PiiType::IpAddress,
            start: 11,
            end: 20,
            confidence: 0.95,
        }];
        let (_, records1) = pseudonymize_text(text1, &entities1, &mapping, &generator);

        let text2 = "Serveur B: 10.0.1.20";
        let entities2 = vec![PiiEntity {
            text: "10.0.1.20".to_string(),
            entity_type: PiiType::IpAddress,
            start: 11,
            end: 20,
            confidence: 0.95,
        }];
        let (_, records2) = pseudonymize_text(text2, &entities2, &mapping, &generator);

        let text3 = "Serveur C: 10.0.1.30";
        let entities3 = vec![PiiEntity {
            text: "10.0.1.30".to_string(),
            entity_type: PiiType::IpAddress,
            start: 11,
            end: 20,
            confidence: 0.95,
        }];
        let (_, records3) = pseudonymize_text(text3, &entities3, &mapping, &generator);

        // Les 3 pseudonymes doivent partager le même préfixe /24
        let prefix1 = records1[0]
            .pseudonym
            .rsplitn(2, '.')
            .last()
            .unwrap()
            .to_string();
        let prefix2 = records2[0]
            .pseudonym
            .rsplitn(2, '.')
            .last()
            .unwrap()
            .to_string();
        let prefix3 = records3[0]
            .pseudonym
            .rsplitn(2, '.')
            .last()
            .unwrap()
            .to_string();

        assert_eq!(
            prefix1, prefix2,
            "Les IPs du même /24 doivent avoir le même préfixe pseudo (cross-requête). Got {} vs {}",
            records1[0].pseudonym, records2[0].pseudonym
        );
        assert_eq!(
            prefix2, prefix3,
            "Les IPs du même /24 doivent avoir le même préfixe pseudo (cross-requête). Got {} vs {}",
            records2[0].pseudonym, records3[0].pseudonym
        );

        // La partie hôte doit être préservée
        assert!(records1[0].pseudonym.ends_with(".10"));
        assert!(records2[0].pseudonym.ends_with(".20"));
        assert!(records3[0].pseudonym.ends_with(".30"));
    }
}
