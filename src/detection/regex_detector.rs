use regex::Regex;

use crate::detection::types::{PiiEntity, PiiType};

/// Détecteur de PII basé sur des regex.
/// Utilisé comme fallback quand le modèle ONNX n'est pas disponible.
/// Détecte les PII à pattern fixe : emails, IPs, téléphones, CB, IBAN, clés API.
/// Ne fait PAS de détection contextuelle (noms de personnes, etc.).
pub struct RegexDetector {
    patterns: Vec<(PiiType, Regex)>,
}

impl RegexDetector {
    pub fn new() -> Self {
        let patterns = vec![
            // Emails
            (
                PiiType::Email,
                Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap(),
            ),
            // IPv4
            (
                PiiType::IpAddress,
                Regex::new(r"\b(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\b").unwrap(),
            ),
            // IPv6 (simplifié)
            (
                PiiType::IpAddress,
                Regex::new(r"\b(?:[0-9a-fA-F]{1,4}:){2,7}[0-9a-fA-F]{1,4}\b").unwrap(),
            ),
            // Téléphones français
            (
                PiiType::PhoneNumber,
                Regex::new(r"(?:\+33|0)\s?[1-9](?:[\s.-]?\d{2}){4}").unwrap(),
            ),
            // Cartes bancaires (16 chiffres, avec ou sans espaces/tirets)
            (
                PiiType::CreditCard,
                Regex::new(r"\b\d{4}[\s-]?\d{4}[\s-]?\d{4}[\s-]?\d{4}\b").unwrap(),
            ),
            // IBAN (FR + 2 chiffres + 23 alphanum)
            (
                PiiType::Iban,
                Regex::new(r"\b[A-Z]{2}\d{2}\s?\d{4}\s?\d{4}\s?\d{4}\s?\d{4}\s?\d{2,4}\b").unwrap(),
            ),
            // Clés API / tokens (sk-, pk-, ghp_, xoxb-, etc.)
            (
                PiiType::ApiKey,
                Regex::new(r"\b(?:sk|pk|api|token|ghp|gho|xoxb|xoxp|AKIA|bearer)[-_][a-zA-Z0-9_-]{16,}\b").unwrap(),
            ),
            // Numéro de sécurité sociale français
            (
                PiiType::NationalId,
                Regex::new(r"\b[12]\s?\d{2}\s?\d{2}\s?\d{2}\s?\d{3}\s?\d{3}\s?\d{2}\b").unwrap(),
            ),
        ];

        Self { patterns }
    }

    /// Détecte les PII dans un texte via regex, en excluant les termes de la whitelist.
    pub fn detect_with_whitelist(&self, text: &str, whitelist: &[String]) -> Vec<PiiEntity> {
        let mut entities = self.detect(text);
        if !whitelist.is_empty() {
            entities.retain(|e| {
                !whitelist.iter().any(|w| e.text.eq_ignore_ascii_case(w))
            });
        }
        entities
    }

    /// Détecte les PII dans un texte via regex.
    pub fn detect(&self, text: &str) -> Vec<PiiEntity> {
        let mut entities = Vec::new();

        for (pii_type, regex) in &self.patterns {
            for mat in regex.find_iter(text) {
                // Éviter les doublons (même position)
                let start = mat.start();
                let end = mat.end();
                let already_found = entities.iter().any(|e: &PiiEntity| {
                    e.start == start && e.end == end
                });

                if !already_found {
                    entities.push(PiiEntity {
                        text: mat.as_str().to_string(),
                        entity_type: *pii_type,
                        start,
                        end,
                        confidence: 0.90, // confiance fixe pour les regex
                    });
                }
            }
        }

        // Trier par position
        entities.sort_by_key(|e| e.start);
        entities
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detector() -> RegexDetector {
        RegexDetector::new()
    }

    #[test]
    fn test_detect_email() {
        let entities = detector().detect("Contactez jean.dupont@acme.fr pour info");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].entity_type, PiiType::Email);
        assert_eq!(entities[0].text, "jean.dupont@acme.fr");
    }

    #[test]
    fn test_detect_multiple_emails() {
        let entities = detector().detect("alice@test.com et bob@corp.org");
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0].text, "alice@test.com");
        assert_eq!(entities[1].text, "bob@corp.org");
    }

    #[test]
    fn test_detect_ipv4() {
        let entities = detector().detect("Serveur sur 192.168.1.50 port 8080");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].entity_type, PiiType::IpAddress);
        assert_eq!(entities[0].text, "192.168.1.50");
    }

    #[test]
    fn test_detect_ipv4_not_invalid() {
        let entities = detector().detect("Version 1.2.3");
        // "1.2.3" n'est pas une IP valide (3 octets seulement)
        assert!(entities.is_empty());
    }

    #[test]
    fn test_detect_phone_french() {
        let entities = detector().detect("Tel: 06 12 34 56 78");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].entity_type, PiiType::PhoneNumber);
    }

    #[test]
    fn test_detect_phone_international() {
        let entities = detector().detect("Tel: +33 6 12 34 56 78");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].entity_type, PiiType::PhoneNumber);
    }

    #[test]
    fn test_detect_credit_card() {
        let entities = detector().detect("CB: 4111 1111 1111 1111");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].entity_type, PiiType::CreditCard);
    }

    #[test]
    fn test_detect_api_key() {
        let entities = detector().detect("key: sk-abc123def456ghi789jkl012");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].entity_type, PiiType::ApiKey);
    }

    #[test]
    fn test_detect_iban() {
        let entities = detector().detect("IBAN: FR7612345678901234567890");
        let iban_entities: Vec<_> = entities.iter().filter(|e| e.entity_type == PiiType::Iban).collect();
        assert_eq!(iban_entities.len(), 1);
    }

    #[test]
    fn test_detect_no_pii() {
        let entities = detector().detect("Ceci est un texte sans données sensibles.");
        assert!(entities.is_empty());
    }

    #[test]
    fn test_detect_mixed() {
        let text = "Email: jean@acme.fr, IP: 10.0.0.1, Tel: 06 12 34 56 78";
        let entities = detector().detect(text);
        assert_eq!(entities.len(), 3);

        let types: Vec<PiiType> = entities.iter().map(|e| e.entity_type).collect();
        assert!(types.contains(&PiiType::Email));
        assert!(types.contains(&PiiType::IpAddress));
        assert!(types.contains(&PiiType::PhoneNumber));
    }

    #[test]
    fn test_detect_positions_correct() {
        let text = "xxx jean@test.fr yyy";
        let entities = detector().detect(text);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].start, 4);
        assert_eq!(entities[0].end, 18 - 2); // "jean@test.fr" starts at 4
        assert_eq!(&text[entities[0].start..entities[0].end], "jean@test.fr");
    }

    #[test]
    fn test_detect_sorted_by_position() {
        let text = "IP: 10.0.0.1, email: z@b.com";
        let entities = detector().detect(text);
        assert!(entities.len() >= 2);
        for i in 1..entities.len() {
            assert!(entities[i].start >= entities[i - 1].start);
        }
    }

    #[test]
    fn test_whitelist_excludes_term() {
        let whitelist = vec!["10.0.0.1".to_string()];
        let entities = detector().detect_with_whitelist(
            "Serveurs: 10.0.0.1 et 192.168.1.50",
            &whitelist,
        );
        // 10.0.0.1 doit être exclu, 192.168.1.50 reste
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].text, "192.168.1.50");
    }

    #[test]
    fn test_whitelist_case_insensitive() {
        let whitelist = vec!["JEAN@ACME.FR".to_string()];
        let entities = detector().detect_with_whitelist(
            "Contact: jean@acme.fr",
            &whitelist,
        );
        assert!(entities.is_empty());
    }

    #[test]
    fn test_whitelist_empty_no_effect() {
        let entities = detector().detect_with_whitelist(
            "Email: jean@test.fr",
            &[],
        );
        assert_eq!(entities.len(), 1);
    }
}
