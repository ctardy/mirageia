use std::collections::HashMap;

use rand::Rng;

use crate::detection::{PiiEntity, PiiType};
use crate::mapping::MappingTable;
use crate::pseudonymization::generator::PseudonymGenerator;

/// Record of a replacement that was performed.
#[derive(Debug, Clone)]
pub struct ReplacementRecord {
    pub original: String,
    pub pseudonym: String,
    pub pii_type: PiiType,
    pub start: usize,
    pub end: usize,
}

/// Extracts the network prefix of an IPv4 address based on the mask (in bits).
/// Returns the prefix octets as a string "a.b.c" for /24, etc.
fn ip_network_prefix(ip: &str, mask_bits: u8) -> Option<String> {
    if ip.contains(':') {
        return None; // IPv6 not supported for grouping
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

/// Extracts the host part of an IPv4 address based on the mask (in bits).
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

/// Generates a pseudo-random network prefix for a given mask.
fn generate_pseudo_prefix(mask_bits: u8) -> String {
    let mut rng = rand::thread_rng();
    let full_octets = (mask_bits / 8) as usize;
    (0..full_octets)
        .map(|_| rng.gen_range(1..255u8).to_string())
        .collect::<Vec<_>>()
        .join(".")
}

/// Pseudonymizes PII entities in a text.
/// Replacements are performed in descending order of position
/// to preserve offsets.
///
/// For IPs sharing the same subnet, coherent pseudonyms
/// are generated (same pseudo prefix, only the host part differs).
///
/// Returns the modified text and the list of replacements.
pub fn pseudonymize_text(
    text: &str,
    entities: &[PiiEntity],
    mapping: &MappingTable,
    generator: &PseudonymGenerator,
) -> (String, Vec<ReplacementRecord>) {
    if entities.is_empty() {
        return (text.to_string(), vec![]);
    }

    // Pre-compute coherent network prefixes for grouped IPs
    let subnet_pseudo_prefixes = compute_subnet_prefixes(entities, mapping);

    // Sort by descending position
    let mut sorted_entities: Vec<&PiiEntity> = entities.iter().collect();
    sorted_entities.sort_by(|a, b| b.start.cmp(&a.start));

    let mut result = text.to_string();
    let mut records = Vec::new();

    for entity in sorted_entities {
        // Look up an existing pseudonym in the mapping (session coherence)
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
                // Insert into the mapping
                let _ = mapping.insert(&entity.text, &new_pseudo, entity.entity_type);
                new_pseudo
            }
        };

        // Check that the bounds are valid
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

    // Reverse the records to get text order (first occurrence first)
    records.reverse();

    (result, records)
}

/// Detects IPs that share the same subnet (/24) in the entity batch,
/// and generates a common pseudo prefix for each group.
/// Returns a mapping: original network prefix -> pseudo prefix.
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
            continue; // IPv6 excluded
        }
        if mapping.lookup_original(&entity.text).is_some() {
            continue; // Already mapped, no need to recompute
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
            // Multiple IPs in the same /24 -> generate a common pseudo prefix
            result.insert(orig_prefix.clone(), generate_pseudo_prefix(24));
        }
    }
    result
}

/// Generates a subnet-coherent IP pseudonym if applicable.
/// If the IP belongs to a subnet group, uses the common pseudo prefix
/// and preserves the original host part.
///
/// Also checks the existing mapping: if an IP in the same /24 was already
/// pseudonymized (in a previous text field), reuses the same pseudo prefix.
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

    // 1. Check the pre-computed prefixes from the current batch
    if let Some(pseudo_prefix) = subnet_prefixes.get(&orig_prefix) {
        if let Some(host) = ip_host_part(original_ip, 24) {
            return format!("{}.{}", pseudo_prefix, host);
        }
    }

    // 2. Check existing mapping to find a sibling IP already pseudonymized
    for (pseudo, original, pii_type) in &mapping.all_entries_with_type() {
        if *pii_type != PiiType::IpAddress || original.contains(':') {
            continue;
        }
        if let Some(existing_orig_prefix) = ip_network_prefix(original, 24) {
            if existing_orig_prefix == orig_prefix {
                // Found a sibling IP! Extract the pseudo prefix used
                if let Some(existing_pseudo_prefix) = ip_network_prefix(pseudo, 24) {
                    if let Some(host) = ip_host_part(original_ip, 24) {
                        return format!("{}.{}", existing_pseudo_prefix, host);
                    }
                }
            }
        }
    }

    // 3. No grouping: generate a random prefix + preserve the host part
    // This enables coherence if a sibling IP arrives in a later call
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

        // First occurrence
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

        // Second occurrence of the same name
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

        // The same pseudonym must be used (session coherence)
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
        // Records sorted by ascending position
        assert!(records[0].start < records[1].start);
    }

    #[test]
    fn test_subnet_coherent_ips() {
        let mapping = MappingTable::new();
        let generator = PseudonymGenerator::new();

        // 3 IPs in the same /24 (10.0.1.x)
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

        // The 3 IPs must no longer appear
        assert!(!result.contains("10.0.1.10"));
        assert!(!result.contains("10.0.1.20"));
        assert!(!result.contains("10.0.1.30"));
        assert_eq!(records.len(), 3);

        // The 3 pseudonyms must share the same /24 prefix
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

        // The host part must be preserved (10, 20, 30)
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

        // Single IP -> no grouping
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
        // The pseudonym must be a valid IPv4 with the host part preserved
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

        // 2 IPs in different subnets
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
        // The /24 prefixes should be different (no grouping)
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
        // We cannot guarantee they are different (random collision possible)
        // but we verify that both are valid IPs
        assert_eq!(records[0].pseudonym.split('.').count(), 4);
        assert_eq!(records[1].pseudonym.split('.').count(), 4);
        let _ = (prefix_0, prefix_1); // use to avoid the warning
    }

    #[test]
    fn test_subnet_coherent_ips_cross_requests() {
        let mapping = MappingTable::new();
        let generator = PseudonymGenerator::new();

        // Simulate 3 IPs from the same /24 arriving in separate calls
        // (as when they are in different text fields)
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

        // The 3 pseudonyms must share the same /24 prefix
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

        // The host part must be preserved
        assert!(records1[0].pseudonym.ends_with(".10"));
        assert!(records2[0].pseudonym.ends_with(".20"));
        assert!(records3[0].pseudonym.ends_with(".30"));
    }
}
