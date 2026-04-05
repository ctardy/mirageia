use rand::Rng;

use crate::detection::PiiType;
use crate::pseudonymization::dictionaries::Dictionaries;

/// Coherent pseudonym generator by PII type.
pub struct PseudonymGenerator {
    dictionaries: Dictionaries,
}

impl Default for PseudonymGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl PseudonymGenerator {
    pub fn new() -> Self {
        Self {
            dictionaries: Dictionaries::load(),
        }
    }

    /// Generates a pseudonym for the given type.
    pub fn generate(&self, pii_type: &PiiType, original: &str) -> String {
        let mut rng = rand::thread_rng();
        match pii_type {
            PiiType::GivenName | PiiType::PersonName => self.gen_firstname(&mut rng, original),
            PiiType::Surname => self.gen_lastname(&mut rng, original),
            PiiType::Email => self.gen_email(&mut rng, original),
            PiiType::IpAddress => self.gen_ip(&mut rng, original),
            PiiType::PhoneNumber => self.gen_phone(&mut rng, original),
            PiiType::CreditCard => self.gen_credit_card(&mut rng),
            PiiType::ApiKey | PiiType::Password => self.gen_api_key(&mut rng, original),
            PiiType::Username => self.gen_username(&mut rng),
            PiiType::Street => format!("{} rue de la Paix", rng.gen_range(1..200)),
            PiiType::City => "Villeneuve".to_string(),
            PiiType::ZipCode => format!("{:05}", rng.gen_range(10000..99999u32)),
            PiiType::AccountNumber | PiiType::Iban => self.gen_iban(&mut rng),
            PiiType::NationalId | PiiType::TaxNumber | PiiType::DriverLicense => {
                self.gen_national_id(&mut rng, original)
            }
            PiiType::DateOfBirth => format!("{:02}/{:02}/{}", rng.gen_range(1..28), rng.gen_range(1..12), rng.gen_range(1950..2005)),
            PiiType::InternalUrl | PiiType::ServerName => "https://internal.example.com/app".to_string(),
            PiiType::FilePath => "/home/user/documents/file.txt".to_string(),
            PiiType::PostalAddress => format!("{} rue de la Paix, 75001 Paris", rng.gen_range(1..200)),
            PiiType::Unknown => format!("[PSEUDO-{}]", rng.gen_range(1000..9999u32)),
        }
    }

    fn gen_firstname(&self, rng: &mut impl Rng, original: &str) -> String {
        let dict = &self.dictionaries.firstnames;
        loop {
            let candidate = dict[rng.gen_range(0..dict.len())].clone();
            if !candidate.eq_ignore_ascii_case(original) {
                return candidate;
            }
        }
    }

    fn gen_lastname(&self, rng: &mut impl Rng, original: &str) -> String {
        let dict = &self.dictionaries.lastnames;
        loop {
            let candidate = dict[rng.gen_range(0..dict.len())].clone();
            if !candidate.eq_ignore_ascii_case(original) {
                return candidate;
            }
        }
    }

    fn gen_email(&self, rng: &mut impl Rng, _original: &str) -> String {
        let first_idx = rng.gen_range(0..self.dictionaries.firstnames.len());
        let firstname = &self.dictionaries.firstnames[first_idx];
        // Normalize (strip basic accents) for the email
        let normalized: String = firstname
            .to_lowercase()
            .chars()
            .map(|c| match c {
                'é' | 'è' | 'ê' | 'ë' => 'e',
                'à' | 'â' | 'ä' => 'a',
                'ù' | 'û' | 'ü' => 'u',
                'ï' | 'î' => 'i',
                'ô' | 'ö' => 'o',
                'ç' => 'c',
                _ => c,
            })
            .collect();
        format!("{}@example.com", normalized)
    }

    fn gen_ip(&self, rng: &mut impl Rng, original: &str) -> String {
        if original.contains(':') {
            // IPv6
            format!(
                "fd00::{:x}:{:x}",
                rng.gen_range(1..0xFFFFu16),
                rng.gen_range(1..0xFFFFu16)
            )
        } else {
            // IPv4 in the 10.0.x.x range
            format!(
                "10.0.{}.{}",
                rng.gen_range(1..255u8),
                rng.gen_range(1..255u8)
            )
        }
    }

    fn gen_phone(&self, rng: &mut impl Rng, original: &str) -> String {
        // Preserve the format (length, separators)
        let digits: String = original
            .chars()
            .map(|c| {
                if c.is_ascii_digit() {
                    char::from_digit(rng.gen_range(0..10), 10).unwrap()
                } else {
                    c
                }
            })
            .collect();
        digits
    }

    fn gen_credit_card(&self, rng: &mut impl Rng) -> String {
        // Generate a card number with valid Luhn checksum
        let mut digits: Vec<u8> = (0..15).map(|_| rng.gen_range(0..10u8)).collect();
        // Prefix 4 (Visa-like)
        digits[0] = 4;
        // Compute the Luhn check digit
        let check = luhn_check_digit(&digits);
        digits.push(check);
        digits.iter().map(|d| char::from_digit(*d as u32, 10).unwrap()).collect()
    }

    fn gen_api_key(&self, rng: &mut impl Rng, original: &str) -> String {
        // Preserve the prefix (sk-, pk-, etc.) and the length
        let prefix_end = original.find('-').map(|i| i + 1).unwrap_or(0);
        let prefix = &original[..prefix_end];
        let rest_len = original.len() - prefix_end;

        let random_part: String = (0..rest_len)
            .map(|_| {
                let chars = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
                chars[rng.gen_range(0..chars.len())] as char
            })
            .collect();

        format!("{}{}", prefix, random_part)
    }

    fn gen_username(&self, rng: &mut impl Rng) -> String {
        let first_idx = rng.gen_range(0..self.dictionaries.firstnames.len());
        let num: u32 = rng.gen_range(10..99);
        format!("user_{}{}", self.dictionaries.firstnames[first_idx].to_lowercase(), num)
    }

    fn gen_iban(&self, rng: &mut impl Rng) -> String {
        // Fictitious IBAN in FR format
        format!(
            "FR{:02}{:05}{:05}{:011}{:02}",
            rng.gen_range(10..99u32),
            rng.gen_range(10000..99999u32),
            rng.gen_range(10000..99999u32),
            rng.gen_range(10000000000u64..99999999999u64),
            rng.gen_range(10..99u32)
        )
    }

    fn gen_national_id(&self, rng: &mut impl Rng, original: &str) -> String {
        // Preserve the length, replace the digits
        original
            .chars()
            .map(|c| {
                if c.is_ascii_digit() {
                    char::from_digit(rng.gen_range(0..10), 10).unwrap()
                } else {
                    c
                }
            })
            .collect()
    }
}

/// Computes the Luhn check digit for a partial number (without the last digit).
fn luhn_check_digit(digits: &[u8]) -> u8 {
    let mut sum: u32 = 0;
    for (i, &d) in digits.iter().rev().enumerate() {
        let mut val = d as u32;
        if i % 2 == 0 {
            val *= 2;
            if val > 9 {
                val -= 9;
            }
        }
        sum += val;
    }
    ((10 - (sum % 10)) % 10) as u8
}

/// Checks whether a number passes Luhn validation.
pub fn luhn_valid(number: &str) -> bool {
    let digits: Vec<u8> = number
        .chars()
        .filter(|c| c.is_ascii_digit())
        .map(|c| c.to_digit(10).unwrap() as u8)
        .collect();

    if digits.len() < 2 {
        return false;
    }

    let mut sum: u32 = 0;
    for (i, &d) in digits.iter().rev().enumerate() {
        let mut val = d as u32;
        if i % 2 == 1 {
            val *= 2;
            if val > 9 {
                val -= 9;
            }
        }
        sum += val;
    }
    sum.is_multiple_of(10)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generator() -> PseudonymGenerator {
        PseudonymGenerator::new()
    }

    #[test]
    fn test_gen_firstname_not_empty() {
        let gen = generator();
        let name = gen.generate(&PiiType::GivenName, "Jean");
        assert!(!name.is_empty());
    }

    #[test]
    fn test_gen_lastname_not_empty() {
        let gen = generator();
        let name = gen.generate(&PiiType::Surname, "Dupont");
        assert!(!name.is_empty());
    }

    #[test]
    fn test_gen_email_format() {
        let gen = generator();
        let email = gen.generate(&PiiType::Email, "jean@acme.fr");
        assert!(email.contains('@'));
        assert!(email.ends_with("@example.com"));
    }

    #[test]
    fn test_gen_ipv4_in_range() {
        let gen = generator();
        let ip = gen.generate(&PiiType::IpAddress, "192.168.1.50");
        assert!(ip.starts_with("10.0."));
        assert_eq!(ip.split('.').count(), 4);
    }

    #[test]
    fn test_gen_ipv6_format() {
        let gen = generator();
        let ip = gen.generate(&PiiType::IpAddress, "2001:db8::1");
        assert!(ip.starts_with("fd00::"));
    }

    #[test]
    fn test_gen_phone_preserves_format() {
        let gen = generator();
        let phone = gen.generate(&PiiType::PhoneNumber, "+33 6 12 34 56 78");
        assert_eq!(phone.len(), "+33 6 12 34 56 78".len());
        assert!(phone.starts_with('+'));
        // Spaces must be preserved at the same positions
        assert_eq!(phone.chars().nth(3), Some(' '));
        assert_eq!(phone.chars().nth(5), Some(' '));
    }

    #[test]
    fn test_gen_credit_card_luhn_valid() {
        let gen = generator();
        let cc = gen.generate(&PiiType::CreditCard, "4111111111111111");
        assert_eq!(cc.len(), 16);
        assert!(luhn_valid(&cc), "Numéro {} ne passe pas Luhn", cc);
    }

    #[test]
    fn test_gen_api_key_preserves_prefix() {
        let gen = generator();
        let key = gen.generate(&PiiType::ApiKey, "sk-abc123def456");
        assert!(key.starts_with("sk-"));
        assert_eq!(key.len(), "sk-abc123def456".len());
    }

    #[test]
    fn test_gen_api_key_no_prefix() {
        let gen = generator();
        let key = gen.generate(&PiiType::ApiKey, "abcdef123456");
        assert_eq!(key.len(), "abcdef123456".len());
    }

    #[test]
    fn test_gen_different_each_time() {
        let gen = generator();
        let mut results = std::collections::HashSet::new();
        for _ in 0..20 {
            results.insert(gen.generate(&PiiType::GivenName, "Jean"));
        }
        // With 50 first names, over 20 draws we should get at least 2 different results
        assert!(results.len() >= 2, "Le générateur produit toujours le même résultat");
    }

    #[test]
    fn test_luhn_valid_known_numbers() {
        assert!(luhn_valid("4111111111111111")); // Visa test
        assert!(luhn_valid("5500000000000004")); // MC test
        assert!(!luhn_valid("1234567890123456")); // Invalide
    }

    #[test]
    fn test_gen_iban_format() {
        let gen = generator();
        let iban = gen.generate(&PiiType::Iban, "FR7612345678901234567890123");
        assert!(iban.starts_with("FR"));
    }

    #[test]
    fn test_gen_national_id_preserves_length() {
        let gen = generator();
        let id = gen.generate(&PiiType::NationalId, "1 85 07 75 123 456 78");
        assert_eq!(id.len(), "1 85 07 75 123 456 78".len());
    }
}
