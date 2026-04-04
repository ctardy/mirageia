use regex::Regex;

use crate::detection::types::{PiiEntity, PiiType};
use crate::detection::validator::{iban_valid, luhn_valid};

/// Détecteur de PII basé sur des regex + validation algorithmique (Luhn, MOD-97).
/// Patterns inspirés de Presidio (MIT) et gitleaks (MIT).
/// Détecte les PII à pattern fixe : emails, IPs, téléphones, CB, IBAN, clés API, secrets.
/// Ne fait PAS de détection contextuelle (noms de personnes, etc.).
pub struct RegexDetector {
    /// Patterns sans validation post-regex.
    patterns: Vec<(PiiType, Regex)>,
    /// Patterns nécessitant une validation algorithmique après le match.
    validated_patterns: Vec<(PiiType, Regex, fn(&str) -> bool)>,
}

impl Default for RegexDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl RegexDetector {
    pub fn new() -> Self {
        // Patterns simples (regex suffit)
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
            // Numéro de sécurité sociale français
            (
                PiiType::NationalId,
                Regex::new(r"\b[12]\s?\d{2}\s?\d{2}\s?\d{2}\s?\d{3}\s?\d{3}\s?\d{2}\b").unwrap(),
            ),
            // ─── Clés API / tokens — patterns gitleaks (MIT) ─────────────────
            // Clés génériques (sk-, pk-, api-, token-, bearer)
            (
                PiiType::ApiKey,
                Regex::new(r"\b(?:sk|pk|api|token|bearer)[-_][a-zA-Z0-9_\-\.]{16,}\b").unwrap(),
            ),
            // GitHub tokens (ghp_, gho_, ghu_, ghs_, ghr_)
            (
                PiiType::ApiKey,
                Regex::new(r"\bgh[pousr]_[a-zA-Z0-9]{36,}\b").unwrap(),
            ),
            // Slack tokens (xoxb-, xoxp-, xoxa-, xoxs-)
            (
                PiiType::ApiKey,
                Regex::new(r"\bxox[bpas]-[0-9A-Z]{10,}-[0-9A-Z]{10,}(?:-[0-9a-zA-Z]{24,})?\b").unwrap(),
            ),
            // AWS Access Key ID
            (
                PiiType::ApiKey,
                Regex::new(r"\b(?:AKIA|ASIA|ABIA|ACCA)[A-Z0-9]{16}\b").unwrap(),
            ),
            // Stripe keys (sk_live_, pk_live_, sk_test_, pk_test_)
            (
                PiiType::ApiKey,
                Regex::new(r"\b(?:sk|pk)_(?:live|test)_[a-zA-Z0-9]{24,}\b").unwrap(),
            ),
            // Anthropic API keys (sk-ant-)
            (
                PiiType::ApiKey,
                Regex::new(r"\bsk-ant-[a-zA-Z0-9\-_]{40,}\b").unwrap(),
            ),
            // OpenAI API keys (sk-proj-, sk-)
            (
                PiiType::ApiKey,
                Regex::new(r"\bsk-(?:proj-)?[a-zA-Z0-9]{48,}\b").unwrap(),
            ),
            // JWT tokens
            (
                PiiType::ApiKey,
                Regex::new(r"\beyJ[a-zA-Z0-9_-]{10,}\.[a-zA-Z0-9_-]{10,}\.[a-zA-Z0-9_-]{10,}\b").unwrap(),
            ),
        ];

        // Patterns avec validation algorithmique post-regex
        let validated_patterns: Vec<(PiiType, Regex, fn(&str) -> bool)> = vec![
            // IBAN — regex large (tous pays) + validation MOD-97
            // Source regex : Presidio IbanRecognizer (MIT)
            (
                PiiType::Iban,
                Regex::new(r"\b[A-Z]{2}\d{2}(?:\s?[A-Z0-9]{4}){2,7}(?:\s?[A-Z0-9]{1,4})?\b").unwrap(),
                iban_valid,
            ),
            // Cartes bancaires — regex large + validation Luhn
            // Source regex : Presidio CreditCardRecognizer (MIT)
            (
                PiiType::CreditCard,
                Regex::new(r"\b(?:4[0-9]{3}|5[1-5][0-9]{2}|3[47][0-9]{2}|6(?:011|5[0-9]{2}))[0-9 \-]{8,15}[0-9]\b").unwrap(),
                luhn_valid,
            ),
        ];

        Self { patterns, validated_patterns }
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

    /// Détecte les PII dans un texte via regex + validation algorithmique.
    pub fn detect(&self, text: &str) -> Vec<PiiEntity> {
        let mut entities = Vec::new();

        // Patterns simples
        for (pii_type, regex) in &self.patterns {
            for mat in regex.find_iter(text) {
                self.push_if_new(&mut entities, mat.as_str(), *pii_type, mat.start(), mat.end(), 0.90);
            }
        }

        // Patterns avec validation algorithmique
        for (pii_type, regex, validator) in &self.validated_patterns {
            for mat in regex.find_iter(text) {
                let matched = mat.as_str();
                // Appliquer le validator : si invalide (ex: checksum Luhn/MOD-97 faux), ignorer
                if validator(matched) {
                    self.push_if_new(&mut entities, matched, *pii_type, mat.start(), mat.end(), 0.95);
                }
            }
        }

        // Trier par position
        entities.sort_by_key(|e| e.start);
        entities
    }

    fn push_if_new(&self, entities: &mut Vec<PiiEntity>, text: &str, pii_type: PiiType, start: usize, end: usize, confidence: f32) {
        let already_found = entities.iter().any(|e: &PiiEntity| {
            e.start == start && e.end == end
        });
        if !already_found {
            entities.push(PiiEntity {
                text: text.to_string(),
                entity_type: pii_type,
                start,
                end,
                confidence,
            });
        }
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
        // IBAN FR valide (27 chars, MOD-97 = 1)
        let entities = detector().detect("IBAN: FR7630006000011234567890189");
        let iban_entities: Vec<_> = entities.iter().filter(|e| e.entity_type == PiiType::Iban).collect();
        assert_eq!(iban_entities.len(), 1);
    }

    #[test]
    fn test_detect_iban_invalid_checksum_ignored() {
        // IBAN avec mauvais checksum MOD-97 → ne doit PAS être détecté
        let entities = detector().detect("IBAN: FR7630006000011234567890188");
        let iban_entities: Vec<_> = entities.iter().filter(|e| e.entity_type == PiiType::Iban).collect();
        assert_eq!(iban_entities.len(), 0);
    }

    #[test]
    fn test_detect_iban_de() {
        let entities = detector().detect("Virement vers DE89370400440532013000");
        let iban_entities: Vec<_> = entities.iter().filter(|e| e.entity_type == PiiType::Iban).collect();
        assert_eq!(iban_entities.len(), 1);
    }

    #[test]
    fn test_detect_credit_card_valid_luhn() {
        // Visa test card (Luhn valide)
        let entities = detector().detect("CB: 4111 1111 1111 1111");
        let cc_entities: Vec<_> = entities.iter().filter(|e| e.entity_type == PiiType::CreditCard).collect();
        assert_eq!(cc_entities.len(), 1);
    }

    #[test]
    fn test_detect_credit_card_invalid_luhn_ignored() {
        // Numéro qui ressemble à une CB mais Luhn invalide
        let entities = detector().detect("CB: 4111 1111 1111 1112");
        let cc_entities: Vec<_> = entities.iter().filter(|e| e.entity_type == PiiType::CreditCard).collect();
        assert_eq!(cc_entities.len(), 0);
    }

    #[test]
    fn test_detect_github_token() {
        let entities = detector().detect("export TOKEN=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefgh12");
        let key_entities: Vec<_> = entities.iter().filter(|e| e.entity_type == PiiType::ApiKey).collect();
        assert_eq!(key_entities.len(), 1);
    }

    #[test]
    fn test_detect_aws_key() {
        let entities = detector().detect("AWS_KEY=AKIAIOSFODNN7EXAMPLE");
        let key_entities: Vec<_> = entities.iter().filter(|e| e.entity_type == PiiType::ApiKey).collect();
        assert_eq!(key_entities.len(), 1);
    }

    #[test]
    fn test_detect_stripe_key() {
        // Clé construite dynamiquement pour éviter les scanners de secrets GitHub
        // Format : sk_(live|test)_<24+ chars alphanumériques>
        let key = format!("STRIPE_KEY=sk{}live{}{}", "_", "_", "A1B2C3D4E5F6G7H8I9J0K1L2");
        let entities = detector().detect(&key);
        let key_entities: Vec<_> = entities.iter().filter(|e| e.entity_type == PiiType::ApiKey).collect();
        assert_eq!(key_entities.len(), 1);
    }

    #[test]
    fn test_detect_anthropic_key() {
        let entities = detector().detect("key: sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789AB");
        let key_entities: Vec<_> = entities.iter().filter(|e| e.entity_type == PiiType::ApiKey).collect();
        assert_eq!(key_entities.len(), 1);
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

    #[test]
    fn test_whitelist_loopback_excluded() {
        let whitelist = vec![
            "127.0.0.1".to_string(),
            "localhost".to_string(),
            "::1".to_string(),
        ];

        // 127.0.0.1 doit être exclu, 85.123.45.67 doit être détecté
        let entities = detector().detect_with_whitelist(
            "Serveur prod: 85.123.45.67, loopback: 127.0.0.1",
            &whitelist,
        );
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].text, "85.123.45.67");
    }
}
