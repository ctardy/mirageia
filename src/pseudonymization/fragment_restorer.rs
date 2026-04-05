use std::collections::HashMap;

use regex::Regex;

use crate::detection::PiiType;
use crate::mapping::MappingTable;

/// Fragment pair: a fragment of the pseudonym and its corresponding original.
#[derive(Debug, Clone)]
struct FragmentPair {
    pseudo_fragment: String,
    original_fragment: String,
}

/// Extracts structural fragments from a mapping based on the PII type.
/// Only decomposable types (IP, CC, NationalId) produce fragments.
fn decompose_fragments(pseudonym: &str, original: &str, pii_type: PiiType) -> Vec<FragmentPair> {
    match pii_type {
        PiiType::IpAddress => decompose_ip(pseudonym, original),
        PiiType::CreditCard => decompose_credit_card(pseudonym, original),
        PiiType::NationalId | PiiType::TaxNumber | PiiType::DriverLicense => {
            decompose_segmented(pseudonym, original)
        }
        _ => vec![],
    }
}

/// Decomposes an IPv4 address into octets.
fn decompose_ip(pseudo: &str, original: &str) -> Vec<FragmentPair> {
    if pseudo.contains(':') || original.contains(':') {
        return vec![];
    }
    let pseudo_octets: Vec<&str> = pseudo.split('.').collect();
    let orig_octets: Vec<&str> = original.split('.').collect();
    if pseudo_octets.len() != 4 || orig_octets.len() != 4 {
        return vec![];
    }
    pseudo_octets
        .iter()
        .zip(orig_octets.iter())
        .filter(|(p, o)| p != o)
        .map(|(p, o)| FragmentPair {
            pseudo_fragment: p.to_string(),
            original_fragment: o.to_string(),
        })
        .collect()
}

/// Decomposes a credit card number into groups of 4 digits.
fn decompose_credit_card(pseudo: &str, original: &str) -> Vec<FragmentPair> {
    let pseudo_digits: String = pseudo.chars().filter(|c| c.is_ascii_digit()).collect();
    let orig_digits: String = original.chars().filter(|c| c.is_ascii_digit()).collect();
    if pseudo_digits.len() != orig_digits.len() || pseudo_digits.len() < 8 {
        return vec![];
    }
    let mut pairs = vec![];
    for i in (0..pseudo_digits.len()).step_by(4) {
        let end = (i + 4).min(pseudo_digits.len());
        let p = &pseudo_digits[i..end];
        let o = &orig_digits[i..end];
        if p != o {
            pairs.push(FragmentPair {
                pseudo_fragment: p.to_string(),
                original_fragment: o.to_string(),
            });
        }
    }
    pairs
}

/// Decomposes a segmented identifier (NSS, etc.) into parts separated by spaces.
fn decompose_segmented(pseudo: &str, original: &str) -> Vec<FragmentPair> {
    let pseudo_parts: Vec<&str> = pseudo.split_whitespace().collect();
    let orig_parts: Vec<&str> = original.split_whitespace().collect();
    if pseudo_parts.len() != orig_parts.len() || pseudo_parts.len() < 2 {
        return vec![];
    }
    pseudo_parts
        .iter()
        .zip(orig_parts.iter())
        .filter(|(p, o)| p != o)
        .map(|(p, o)| FragmentPair {
            pseudo_fragment: p.to_string(),
            original_fragment: o.to_string(),
        })
        .collect()
}

/// Restores pseudonym fragments decomposed by the LLM in the text.
///
/// Called AFTER the main de-pseudonymization (complete token replacement).
/// Detects when the LLM has extracted sub-parts of a pseudonym (IP octets,
/// CC digit groups, NSS segments) and replaces them with the original sub-parts.
///
/// False-positive prevention strategy:
/// - Fragments >= 2 characters: replacement with word boundaries (\b)
/// - Single-character fragments: replacement only in analytical context
///   (after `=`, `:`, or at the start of a structured value)
/// - Deduplication: if the same pseudo fragment maps to multiple different
///   originals, it is skipped (ambiguity)
/// - Protection of already restored original values: zones containing restored
///   PII are masked during fragment replacement to prevent
///   any corruption (e.g., `o'brien` corrupted by an IP fragment `o`)
pub fn restore_fragments(text: &str, mapping: &MappingTable) -> String {
    let entries = mapping.all_entries_with_type();

    let mut all_fragments: Vec<FragmentPair> = Vec::new();

    // Collect original values to protect them
    let mut originals_to_protect: Vec<String> = Vec::new();

    for (pseudo, original, pii_type) in &entries {
        originals_to_protect.push(original.clone());
        let fragments = decompose_fragments(pseudo, original, *pii_type);
        all_fragments.extend(fragments);
    }

    if all_fragments.is_empty() {
        return text.to_string();
    }

    // Deduplication: if a pseudo_fragment maps to multiple different originals,
    // it is ambiguous -> we exclude it
    let mut fragment_map: HashMap<String, Vec<String>> = HashMap::new();
    for pair in &all_fragments {
        fragment_map
            .entry(pair.pseudo_fragment.clone())
            .or_default()
            .push(pair.original_fragment.clone());
    }

    // Keep only non-ambiguous fragments
    let mut safe_fragments: Vec<FragmentPair> = Vec::new();
    for pair in &all_fragments {
        let targets = &fragment_map[&pair.pseudo_fragment];
        // Verify that all targets are identical
        if targets.iter().all(|t| t == &pair.original_fragment) {
            // Check that we haven't already added this fragment
            if !safe_fragments
                .iter()
                .any(|f| f.pseudo_fragment == pair.pseudo_fragment)
            {
                safe_fragments.push(pair.clone());
            }
        }
    }

    if safe_fragments.is_empty() {
        return text.to_string();
    }

    // Sort by descending length of the pseudo fragment (longest first)
    safe_fragments.sort_by(|a, b| b.pseudo_fragment.len().cmp(&a.pseudo_fragment.len()));

    // --- Protection of original values ---
    // Temporarily replace already restored values with placeholders
    // to prevent fragment replacement from corrupting them.
    let mut result = text.to_string();
    let mut placeholders: Vec<(String, String)> = Vec::new();

    // Sort by descending length to avoid partial replacements
    let mut sorted_originals = originals_to_protect;
    sorted_originals.sort_by_key(|b| std::cmp::Reverse(b.len()));
    sorted_originals.dedup();

    for (i, original) in sorted_originals.iter().enumerate() {
        let placeholder = format!("\x00PROTECT_{}\x00", i);
        result = result.replace(original, &placeholder);
        placeholders.push((placeholder, original.clone()));
    }

    // --- Fragment replacement ---
    for pair in &safe_fragments {
        if pair.pseudo_fragment.len() < 2 {
            // Short fragments: contextual replacement only
            // Context: after = or : (with optional space), or in a numeric list
            let escaped = regex::escape(&pair.pseudo_fragment);
            let pattern = format!(r"(?<=[=:,]\s?){escaped}(?:\b|(?=[,\s\)\]}}]))", escaped = escaped);
            if let Ok(re) = Regex::new(&pattern) {
                result = re
                    .replace_all(&result, pair.original_fragment.as_str())
                    .to_string();
            }
        } else {
            // Fragments >= 2 chars: replacement with word boundaries
            let pattern = format!(r"\b{}\b", regex::escape(&pair.pseudo_fragment));
            if let Ok(re) = Regex::new(&pattern) {
                result = re
                    .replace_all(&result, pair.original_fragment.as_str())
                    .to_string();
            }
        }
    }

    // --- Restoration of protected values ---
    for (placeholder, original) in &placeholders {
        result = result.replace(placeholder, original);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decompose_ip_fragments() {
        let fragments = decompose_ip("10.0.84.12", "172.16.254.3");
        assert_eq!(fragments.len(), 4);
        assert_eq!(fragments[0].pseudo_fragment, "10");
        assert_eq!(fragments[0].original_fragment, "172");
        assert_eq!(fragments[1].pseudo_fragment, "0");
        assert_eq!(fragments[1].original_fragment, "16");
        assert_eq!(fragments[2].pseudo_fragment, "84");
        assert_eq!(fragments[2].original_fragment, "254");
        assert_eq!(fragments[3].pseudo_fragment, "12");
        assert_eq!(fragments[3].original_fragment, "3");
    }

    #[test]
    fn test_decompose_ip_identical_octets_excluded() {
        // If an octet is identical, it is not included
        let fragments = decompose_ip("172.0.84.3", "172.16.254.3");
        assert_eq!(fragments.len(), 2); // only octets 2 and 3 differ
        assert_eq!(fragments[0].pseudo_fragment, "0");
        assert_eq!(fragments[1].pseudo_fragment, "84");
    }

    #[test]
    fn test_decompose_ipv6_returns_empty() {
        let fragments = decompose_ip("fd00::1234:5678", "2001:db8::1");
        assert!(fragments.is_empty());
    }

    #[test]
    fn test_decompose_credit_card() {
        let fragments = decompose_credit_card("4832759104628371", "4111111111111111");
        assert!(!fragments.is_empty());
        // Premier groupe : 4832 vs 4111
        assert_eq!(fragments[0].pseudo_fragment, "4832");
        assert_eq!(fragments[0].original_fragment, "4111");
    }

    #[test]
    fn test_decompose_national_id() {
        let fragments = decompose_segmented("2 91 03 42 876 219 35", "1 85 07 75 123 456 78");
        assert_eq!(fragments.len(), 7);
        assert_eq!(fragments[0].pseudo_fragment, "2");
        assert_eq!(fragments[0].original_fragment, "1");
        assert_eq!(fragments[1].pseudo_fragment, "91");
        assert_eq!(fragments[1].original_fragment, "85");
    }

    #[test]
    fn test_restore_ip_fragments() {
        let mapping = MappingTable::new();
        mapping
            .insert("172.16.254.3", "10.0.84.12", PiiType::IpAddress)
            .unwrap();

        // Text after main de-pseudonymization (complete IP already restored)
        // but analytical fragments still contain the pseudonym values
        let text = "L'adresse IP est 172.16.254.3. Premier octet: 10, deuxième: 84, classe du réseau: A";
        let result = restore_fragments(text, &mapping);

        assert!(result.contains("Premier octet: 172"));
        assert!(result.contains("deuxième: 254"));
        // "10" in "172.16.254.3" must not be touched (not isolated as a word)
        assert!(result.contains("172.16.254.3"));
    }

    #[test]
    fn test_restore_nss_fragments() {
        let mapping = MappingTable::new();
        mapping
            .insert(
                "1 85 07 75 123 456 78",
                "2 91 03 42 876 219 35",
                PiiType::NationalId,
            )
            .unwrap();

        let text = "Le NSS est 1 85 07 75 123 456 78. Décomposition: sexe=2, année=91, mois=03, département=42";
        let result = restore_fragments(text, &mapping);

        assert!(result.contains("année=85"));
        assert!(result.contains("mois=07"));
        assert!(result.contains("département=75"));
    }

    #[test]
    fn test_restore_cc_fragments() {
        let mapping = MappingTable::new();
        mapping
            .insert("4111111111111111", "4832759104628371", PiiType::CreditCard)
            .unwrap();

        let text = "Le numéro est 4111111111111111. Groupes: 4832, 7591, 0462, 8371";
        let result = restore_fragments(text, &mapping);

        assert!(result.contains("4111"));
        assert!(result.contains("1111"));
    }

    #[test]
    fn test_restore_no_fragments_for_email() {
        let mapping = MappingTable::new();
        mapping
            .insert("jean@acme.fr", "paul@example.com", PiiType::Email)
            .unwrap();

        let text = "L'email est jean@acme.fr et paul est mentionné";
        let result = restore_fragments(text, &mapping);

        // No fragment replacement for emails
        assert_eq!(result, text);
    }

    #[test]
    fn test_restore_empty_mapping() {
        let mapping = MappingTable::new();
        let text = "Texte sans PII";
        let result = restore_fragments(text, &mapping);
        assert_eq!(result, text);
    }

    #[test]
    fn test_ambiguous_fragments_skipped() {
        let mapping = MappingTable::new();
        // Two IPs with the same pseudo octet "10" but different originals
        mapping
            .insert("192.168.1.1", "10.0.50.20", PiiType::IpAddress)
            .unwrap();
        mapping
            .insert("172.16.0.1", "10.0.60.30", PiiType::IpAddress)
            .unwrap();

        // The fragment "10" maps to "192" AND "172" -> ambiguous, do not touch
        let text = "Octet: 10";
        let result = restore_fragments(text, &mapping);

        // "10" must NOT be replaced because it is ambiguous
        assert!(result.contains("10"));
    }

    #[test]
    fn test_restored_email_not_corrupted_by_fragments() {
        let mapping = MappingTable::new();
        // An email with special chars + an IP in the same mapping
        mapping
            .insert(
                "o'brien+newsletter@hyphen-domain.co.uk",
                "julie@example.com",
                PiiType::Email,
            )
            .unwrap();
        mapping
            .insert("85.123.45.67", "10.0.84.12", PiiType::IpAddress)
            .unwrap();

        // After main de-pseudonymization, both values are restored
        // The fragment restorer must NOT corrupt the restored email
        let text = "Contact: o'brien+newsletter@hyphen-domain.co.uk, IP: 85.123.45.67, octet: 10";
        let result = restore_fragments(text, &mapping);

        // The email must be intact
        assert!(
            result.contains("o'brien+newsletter@hyphen-domain.co.uk"),
            "L'email restauré a été corrompu : {}",
            result
        );
        // The IP must be intact
        assert!(result.contains("85.123.45.67"));
        // The isolated fragment must be replaced
        assert!(result.contains("octet: 85"));
    }

    #[test]
    fn test_restored_ip_not_corrupted_by_fragments() {
        let mapping = MappingTable::new();
        mapping
            .insert("172.16.254.3", "10.0.84.12", PiiType::IpAddress)
            .unwrap();

        // The fully restored IP must not be corrupted by the replacement
        // of its own pseudo fragments
        let text = "IP: 172.16.254.3, analyse: premier=10, troisième=84";
        let result = restore_fragments(text, &mapping);

        assert!(
            result.contains("172.16.254.3"),
            "L'IP restaurée a été corrompue : {}",
            result
        );
        assert!(result.contains("premier=172"));
        assert!(result.contains("troisième=254"));
    }
}
