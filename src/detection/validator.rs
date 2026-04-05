/// PII validation algorithms ported from Presidio and detect-secrets.
/// Pure Rust, zero external dependencies.
/// IBAN validation using MOD-97 algorithm (ISO 13616).
/// Source: Presidio IbanRecognizer + Wikipedia MOD-97-10
pub fn iban_valid(iban: &str) -> bool {
    let iban = iban.replace([' ', '-'], "").to_uppercase();
    if iban.len() < 15 || iban.len() > 34 {
        return false;
    }

    // Verify that the first 2 chars are letters and the next 2 are digits
    let chars: Vec<char> = iban.chars().collect();
    if !chars[0].is_ascii_alphabetic() || !chars[1].is_ascii_alphabetic() {
        return false;
    }
    if !chars[2].is_ascii_digit() || !chars[3].is_ascii_digit() {
        return false;
    }

    // Move the first 4 chars to the end
    let rearranged = format!("{}{}", &iban[4..], &iban[..4]);

    // Replace letters with digits (A=10, B=11, ..., Z=35)
    let numeric: String = rearranged
        .chars()
        .map(|c| {
            if c.is_ascii_alphabetic() {
                (c as u32 - 'A' as u32 + 10).to_string()
            } else {
                c.to_string()
            }
        })
        .collect();

    // MOD-97 computation in blocks of 9 digits to avoid u64 overflow
    let mut remainder: u64 = 0;
    for c in numeric.chars() {
        if let Some(d) = c.to_digit(10) {
            remainder = remainder * 10 + d as u64;
            remainder %= 97;
        } else {
            return false;
        }
    }

    remainder == 1
}

/// Credit card validation using Luhn algorithm (ISO/IEC 7812).
/// Source: Presidio CreditCardRecognizer
pub fn luhn_valid(s: &str) -> bool {
    let digits: Vec<u32> = s
        .chars()
        .filter(|c| c.is_ascii_digit())
        .filter_map(|c| c.to_digit(10))
        .collect();

    if digits.len() < 13 || digits.len() > 19 {
        return false;
    }

    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let doubled = d * 2;
                if doubled > 9 { doubled - 9 } else { doubled }
            } else {
                d
            }
        })
        .sum();

    sum.is_multiple_of(10)
}

/// Shannon entropy of a string (bits per character).
/// Used to detect passwords and secrets with high entropy.
/// A strong password typically has entropy > 3.5 bits.
pub fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let len = s.len() as f64;
    let mut freq = std::collections::HashMap::new();
    for c in s.chars() {
        *freq.entry(c).or_insert(0u32) += 1;
    }
    -freq
        .values()
        .map(|&count| {
            let p = count as f64 / len;
            p * p.log2()
        })
        .sum::<f64>()
}

/// Detects whether a string looks like a password or secret:
/// high entropy + sufficient length + mixed characters.
pub fn looks_like_secret(s: &str) -> bool {
    if s.len() < 12 {
        return false;
    }
    let entropy = shannon_entropy(s);
    let has_upper = s.chars().any(|c| c.is_ascii_uppercase());
    let has_lower = s.chars().any(|c| c.is_ascii_lowercase());
    let has_digit = s.chars().any(|c| c.is_ascii_digit());
    let has_special = s.chars().any(|c| !c.is_alphanumeric());

    let char_classes = [has_upper, has_lower, has_digit, has_special]
        .iter()
        .filter(|&&b| b)
        .count();

    entropy > 3.5 && char_classes >= 3
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── IBAN ────────────────────────────────────────────────────

    #[test]
    fn test_iban_valid_fr() {
        assert!(iban_valid("FR7630006000011234567890189"));
    }

    #[test]
    fn test_iban_valid_with_spaces() {
        assert!(iban_valid("FR76 3000 6000 0112 3456 7890 189"));
    }

    #[test]
    fn test_iban_valid_de() {
        assert!(iban_valid("DE89370400440532013000"));
    }

    #[test]
    fn test_iban_valid_gb() {
        assert!(iban_valid("GB29NWBK60161331926819"));
    }

    #[test]
    fn test_iban_invalid_checksum() {
        assert!(!iban_valid("FR7630006000011234567890188")); // last digit modified
    }

    #[test]
    fn test_iban_too_short() {
        assert!(!iban_valid("FR76300"));
    }

    #[test]
    fn test_iban_invalid_format() {
        assert!(!iban_valid("1234567890"));
    }

    // ─── Luhn ────────────────────────────────────────────────────

    #[test]
    fn test_luhn_visa_valid() {
        assert!(luhn_valid("4111111111111111"));
    }

    #[test]
    fn test_luhn_mastercard_valid() {
        assert!(luhn_valid("5500005555555559"));
    }

    #[test]
    fn test_luhn_amex_valid() {
        assert!(luhn_valid("378282246310005"));
    }

    #[test]
    fn test_luhn_with_spaces() {
        assert!(luhn_valid("4111 1111 1111 1111"));
    }

    #[test]
    fn test_luhn_invalid() {
        assert!(!luhn_valid("4111111111111112")); // last digit modified
    }

    #[test]
    fn test_luhn_too_short() {
        assert!(!luhn_valid("411111111"));
    }

    // ─── Entropy / secrets ───────────────────────────────────────

    #[test]
    fn test_entropy_low_for_simple() {
        assert!(shannon_entropy("aaaaaaa") < 1.0);
        assert!(shannon_entropy("abcabc") < 2.5);
    }

    #[test]
    fn test_entropy_high_for_random() {
        assert!(shannon_entropy("aZ9!xK2@mP5#qR") > 3.5);
    }

    #[test]
    fn test_looks_like_secret_true() {
        assert!(looks_like_secret("P@ssw0rd!Secure99"));
        assert!(looks_like_secret("aZ9!xK2@mP5#qR8$"));
    }

    #[test]
    fn test_looks_like_secret_false_simple() {
        assert!(!looks_like_secret("password"));
        assert!(!looks_like_secret("bonjour"));
        assert!(!looks_like_secret("12345678"));
    }

    #[test]
    fn test_looks_like_secret_false_short() {
        assert!(!looks_like_secret("aZ9!xK"));
    }
}
