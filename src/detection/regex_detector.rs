use regex::Regex;

use crate::detection::types::{PiiEntity, PiiType};
use crate::detection::validator::{iban_valid, looks_like_secret, luhn_valid};

/// Regex-based PII detector with algorithmic validation (Luhn, MOD-97).
/// Patterns inspired by Presidio (MIT) and gitleaks (MIT).
/// Detects fixed-pattern PII: emails, IPs, phone numbers, credit cards, IBAN, API keys, secrets.
/// Does NOT perform contextual detection (person names, etc.).
#[allow(clippy::type_complexity)]
pub struct RegexDetector {
    /// Patterns without post-regex validation.
    patterns: Vec<(PiiType, Regex)>,
    /// Patterns requiring algorithmic validation after the match (full match validated).
    validated_patterns: Vec<(PiiType, Regex, fn(&str) -> bool)>,
    /// Patterns with capture group 1 = value to pseudonymize, validated by fn(&str) -> bool.
    /// Used for context-based secrets: `password = VALUE` → only VALUE is pseudonymized.
    capture_validated_patterns: Vec<(PiiType, Regex, fn(&str) -> bool)>,
}

impl Default for RegexDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl RegexDetector {
    pub fn new() -> Self {
        // Simple patterns (regex is sufficient)
        // ORDER MATTERS: specific patterns (API keys) first,
        // generic patterns (phone, etc.) last.
        // This way, if an API key contains digits, the phone pattern does not override the key.
        let patterns = vec![
            // ─── API keys / tokens — gitleaks patterns (MIT) ─────────────────
            // Anthropic API keys (sk-ant-) — specific, first
            (
                PiiType::ApiKey,
                Regex::new(r"\bsk-ant-[a-zA-Z0-9\-_]{40,}\b").unwrap(),
            ),
            // OpenAI API keys (sk-proj-, sk-)
            (
                PiiType::ApiKey,
                Regex::new(r"\bsk-(?:proj-)?[a-zA-Z0-9]{48,}\b").unwrap(),
            ),
            // Stripe keys (sk_live_, pk_live_, sk_test_, pk_test_)
            (
                PiiType::ApiKey,
                Regex::new(r"\b(?:sk|pk)_(?:live|test)_[a-zA-Z0-9]{24,}\b").unwrap(),
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
            // JWT tokens
            (
                PiiType::ApiKey,
                Regex::new(r"\beyJ[a-zA-Z0-9_-]{10,}\.[a-zA-Z0-9_-]{10,}\.[a-zA-Z0-9_-]{10,}\b").unwrap(),
            ),
            // Generic keys (sk-, pk-, api-, token-, bearer)
            (
                PiiType::ApiKey,
                Regex::new(r"\b(?:sk|pk|api|token|bearer)[-_][a-zA-Z0-9_\-\.]{16,}\b").unwrap(),
            ),
            // ─── Generic patterns (last, ignored if overlap) ──
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
            // IPv6 (simplified)
            (
                PiiType::IpAddress,
                Regex::new(r"\b(?:[0-9a-fA-F]{1,4}:){2,7}[0-9a-fA-F]{1,4}\b").unwrap(),
            ),
            // French phone numbers
            (
                PiiType::PhoneNumber,
                Regex::new(r"(?:\+33|0)\s?[1-9](?:[\s.-]?\d{2}){4}").unwrap(),
            ),
            // French social security number
            (
                PiiType::NationalId,
                Regex::new(r"\b[12]\s?\d{2}\s?\d{2}\s?\d{2}\s?\d{3}\s?\d{3}\s?\d{2}\b").unwrap(),
            ),
        ];

        // Patterns with post-regex algorithmic validation
        #[allow(clippy::type_complexity)]
        let validated_patterns: Vec<(PiiType, Regex, fn(&str) -> bool)> = vec![
            // IBAN — broad regex (all countries) + MOD-97 validation
            // Regex source: Presidio IbanRecognizer (MIT)
            (
                PiiType::Iban,
                Regex::new(r"\b[A-Z]{2}\d{2}(?:\s?[A-Z0-9]{4}){2,7}(?:\s?[A-Z0-9]{1,4})?\b").unwrap(),
                iban_valid,
            ),
            // Credit cards — broad regex + Luhn validation
            // Regex source: Presidio CreditCardRecognizer (MIT)
            (
                PiiType::CreditCard,
                Regex::new(r"\b(?:4[0-9]{3}|5[1-5][0-9]{2}|3[47][0-9]{2}|6(?:011|5[0-9]{2}))[0-9 \-]{8,15}[0-9]\b").unwrap(),
                luhn_valid,
            ),
        ];

        // Patterns with capture group 1 = value to pseudonymize + entropy validation
        // Regex matches `keyword = VALUE` or `keyword est VALUE` — only VALUE is pseudonymized
        // Source: detect-secrets HighEntropyString + gitleaks generic-api-key (MIT)
        #[allow(clippy::type_complexity)]
        let capture_validated_patterns: Vec<(PiiType, Regex, fn(&str) -> bool)> = vec![
            (
                PiiType::Password,
                Regex::new(
                    r"(?i)(?:password|passwd|pwd|mdp|secret|mot\s+de\s+passe)\s*(?:=|:|\s+est|\s+is)\s*(\S{8,})"
                ).unwrap(),
                looks_like_secret,
            ),
        ];

        Self { patterns, validated_patterns, capture_validated_patterns }
    }

    /// Detects PII in text via regex, excluding whitelisted terms.
    pub fn detect_with_whitelist(&self, text: &str, whitelist: &[String]) -> Vec<PiiEntity> {
        let mut entities = self.detect(text);
        if !whitelist.is_empty() {
            entities.retain(|e| {
                !whitelist.iter().any(|w| e.text.eq_ignore_ascii_case(w))
            });
        }
        entities
    }

    /// Detects PII in text via regex + algorithmic validation.
    pub fn detect(&self, text: &str) -> Vec<PiiEntity> {
        let mut entities = Vec::new();

        // Validated patterns first (confidence 0.95): IBAN, credit cards
        // They take priority over simple patterns in case of overlap
        for (pii_type, regex, validator) in &self.validated_patterns {
            for mat in regex.find_iter(text) {
                let matched = mat.as_str();
                if validator(matched) {
                    self.push_if_new(&mut entities, matched, *pii_type, mat.start(), mat.end(), 0.95);
                }
            }
        }

        // Capture-group patterns (confidence 0.95): keyword = VALUE — only VALUE pseudonymized
        for (pii_type, regex, validator) in &self.capture_validated_patterns {
            for caps in regex.captures_iter(text) {
                if let Some(value_match) = caps.get(1) {
                    let value = value_match.as_str();
                    if validator(value) {
                        self.push_if_new(
                            &mut entities,
                            value,
                            *pii_type,
                            value_match.start(),
                            value_match.end(),
                            0.95,
                        );
                    }
                }
            }
        }

        // Simple patterns (confidence 0.90): ignored if the range overlaps an already detected entity
        for (pii_type, regex) in &self.patterns {
            for mat in regex.find_iter(text) {
                let start = mat.start();
                let end = mat.end();
                // Skip if overlapping with an existing entity (e.g., PHONE inside an IBAN)
                let overlaps = entities.iter().any(|e| start < e.end && end > e.start);
                if !overlaps {
                    self.push_if_new(&mut entities, mat.as_str(), *pii_type, start, end, 0.90);
                }
            }
        }

        // Sort by position
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
        // "1.2.3" is not a valid IP (only 3 octets)
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
        // Valid FR IBAN (27 chars, MOD-97 = 1)
        let entities = detector().detect("IBAN: FR7630006000011234567890189");
        let iban_entities: Vec<_> = entities.iter().filter(|e| e.entity_type == PiiType::Iban).collect();
        assert_eq!(iban_entities.len(), 1);
    }

    #[test]
    fn test_iban_not_detected_as_phone() {
        // Digits in an IBAN must not be detected as a phone number
        let entities = detector().detect("IBAN : FR7630006000011234567890189");
        let phone_entities: Vec<_> = entities.iter().filter(|e| e.entity_type == PiiType::PhoneNumber).collect();
        let iban_entities: Vec<_> = entities.iter().filter(|e| e.entity_type == PiiType::Iban).collect();
        assert_eq!(iban_entities.len(), 1, "IBAN doit être détecté");
        assert_eq!(phone_entities.len(), 0, "IBAN ne doit pas être détecté comme téléphone");
    }

    #[test]
    fn test_detect_iban_invalid_checksum_ignored() {
        // IBAN with wrong MOD-97 checksum -> must NOT be detected
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
        // Number that looks like a credit card but has invalid Luhn
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
        // Key built dynamically to avoid GitHub secret scanners
        // Format: sk_(live|test)_<24+ alphanumeric chars>
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
    fn test_detect_password_in_context() {
        // Password after keyword "mot de passe est" — only VALUE pseudonymized
        let entities = detector().detect("Mon mot de passe est P@ssw0rd!Secure99");
        let pwd_entities: Vec<_> = entities.iter().filter(|e| e.entity_type == PiiType::Password).collect();
        assert_eq!(pwd_entities.len(), 1, "Le mot de passe doit être détecté");
        assert_eq!(pwd_entities[0].text, "P@ssw0rd!Secure99");
    }

    #[test]
    fn test_detect_password_with_equals() {
        let entities = detector().detect("password=aZ9!xK2@mP5#qR8$");
        let pwd_entities: Vec<_> = entities.iter().filter(|e| e.entity_type == PiiType::Password).collect();
        assert_eq!(pwd_entities.len(), 1);
        assert_eq!(pwd_entities[0].text, "aZ9!xK2@mP5#qR8$");
    }

    #[test]
    fn test_detect_simple_password_not_detected() {
        // Simple password without special chars / low entropy → NOT detected
        let entities = detector().detect("password=simple123");
        let pwd_entities: Vec<_> = entities.iter().filter(|e| e.entity_type == PiiType::Password).collect();
        assert_eq!(pwd_entities.len(), 0, "Mot de passe simple (faible entropie) ne doit pas être détecté");
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
        // 10.0.0.1 must be excluded, 192.168.1.50 remains
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

        // 127.0.0.1 must be excluded, 85.123.45.67 must be detected
        let entities = detector().detect_with_whitelist(
            "Serveur prod: 85.123.45.67, loopback: 127.0.0.1",
            &whitelist,
        );
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].text, "85.123.45.67");
    }
}
