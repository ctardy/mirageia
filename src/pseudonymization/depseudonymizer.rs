use aho_corasick::AhoCorasick;

use crate::mapping::MappingTable;
use crate::pseudonymization::fragment_restorer::restore_fragments;

/// Dé-pseudonymise un texte en remplaçant les pseudonymes par les valeurs originales.
///
/// Deux passes :
/// 1. **Remplacement principal** (AhoCorasick) : remplace les tokens complets
///    (pseudonymes entiers → valeurs originales).
/// 2. **Restauration de fragments** (SPB — Sub-PII Binding) : détecte et remplace
///    les sous-parties de pseudonymes que le LLM a extraites dans son analyse
///    (octets d'IP, groupes de chiffres CC, segments NSS, etc.).
pub fn depseudonymize_text(text: &str, mapping: &MappingTable) -> String {
    let pairs = mapping.all_pseudonyms_sorted(); // triés par longueur décroissante

    if pairs.is_empty() {
        return text.to_string();
    }

    let patterns: Vec<&str> = pairs.iter().map(|(pseudo, _)| pseudo.as_str()).collect();
    let replacements: Vec<&str> = pairs.iter().map(|(_, orig)| orig.as_str()).collect();

    let ac = AhoCorasick::builder()
        .match_kind(aho_corasick::MatchKind::LeftmostLongest)
        .build(&patterns)
        .expect("Erreur AhoCorasick");

    // Passe 1 : remplacement des tokens complets
    let result = ac.replace_all(text, &replacements);

    // Passe 2 : restauration des fragments (SPB)
    restore_fragments(&result, mapping)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detection::PiiType;

    #[test]
    fn test_depseudonymize_simple() {
        let mapping = MappingTable::new();
        mapping.insert("Jean", "Michel", PiiType::GivenName).unwrap();
        mapping.insert("jean@acme.fr", "michel@example.com", PiiType::Email).unwrap();

        let text = "Bonjour Michel, votre email est michel@example.com";
        let result = depseudonymize_text(text, &mapping);

        assert_eq!(result, "Bonjour Jean, votre email est jean@acme.fr");
    }

    #[test]
    fn test_depseudonymize_no_match() {
        let mapping = MappingTable::new();
        mapping.insert("Jean", "Michel", PiiType::GivenName).unwrap();

        let text = "Aucun pseudonyme ici";
        let result = depseudonymize_text(text, &mapping);

        assert_eq!(result, "Aucun pseudonyme ici");
    }

    #[test]
    fn test_depseudonymize_empty_mapping() {
        let mapping = MappingTable::new();
        let text = "Texte normal";
        let result = depseudonymize_text(text, &mapping);
        assert_eq!(result, "Texte normal");
    }

    #[test]
    fn test_depseudonymize_longest_first() {
        let mapping = MappingTable::new();
        // "Michel Martin" et "Michel" sont tous les deux des pseudonymes
        mapping.insert("Jean-Pierre Dupont", "Michel Martin", PiiType::PersonName).unwrap();
        mapping.insert("Jean", "Michel", PiiType::GivenName).unwrap();

        // "Michel Martin" doit être remplacé en entier, pas juste "Michel"
        let text = "Contact: Michel Martin";
        let result = depseudonymize_text(text, &mapping);

        assert_eq!(result, "Contact: Jean-Pierre Dupont");
    }

    #[test]
    fn test_depseudonymize_multiple_occurrences() {
        let mapping = MappingTable::new();
        mapping.insert("192.168.1.1", "10.0.0.42", PiiType::IpAddress).unwrap();

        let text = "Serveur 10.0.0.42 et backup 10.0.0.42";
        let result = depseudonymize_text(text, &mapping);

        assert_eq!(result, "Serveur 192.168.1.1 et backup 192.168.1.1");
    }

    #[test]
    fn test_roundtrip_pseudonymize_depseudonymize() {
        use crate::pseudonymization::generator::PseudonymGenerator;
        use crate::pseudonymization::replacer::pseudonymize_text;
        use crate::detection::PiiEntity;

        let mapping = MappingTable::new();
        let generator = PseudonymGenerator::new();

        let original = "Contactez jean@acme.fr pour plus d'infos";
        let entities = vec![PiiEntity {
            text: "jean@acme.fr".to_string(),
            entity_type: PiiType::Email,
            start: 10,
            end: 22,
            confidence: 0.95,
        }];

        let (pseudonymized, _) = pseudonymize_text(original, &entities, &mapping, &generator);

        // Le texte pseudonymisé ne contient plus l'email original
        assert!(!pseudonymized.contains("jean@acme.fr"));

        // La dé-pseudonymisation restaure l'original
        let restored = depseudonymize_text(&pseudonymized, &mapping);
        assert_eq!(restored, original);
    }
}
