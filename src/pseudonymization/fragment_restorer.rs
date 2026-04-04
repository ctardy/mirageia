use std::collections::HashMap;

use regex::Regex;

use crate::detection::PiiType;
use crate::mapping::MappingTable;

/// Paire de fragments : un fragment du pseudonyme et son correspondant original.
#[derive(Debug, Clone)]
struct FragmentPair {
    pseudo_fragment: String,
    original_fragment: String,
}

/// Extrait les fragments structurels d'un mapping selon le type de PII.
/// Seuls les types décomposables (IP, CC, NationalId) produisent des fragments.
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

/// Décompose une IP v4 en octets.
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

/// Décompose un numéro de carte de crédit en groupes de 4 chiffres.
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

/// Décompose un identifiant segmenté (NSS, etc.) en parties séparées par des espaces.
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

/// Restaure les fragments de pseudonymes décomposés par le LLM dans le texte.
///
/// Appelé APRÈS la dé-pseudonymisation principale (remplacement des tokens complets).
/// Détecte quand le LLM a extrait des sous-parties d'un pseudonyme (octets d'IP,
/// groupes de chiffres CC, segments NSS) et les remplace par les sous-parties originales.
///
/// Stratégie anti-faux-positifs :
/// - Fragments ≥ 2 caractères : remplacement avec limites de mot (\b)
/// - Fragments de 1 caractère : remplacement uniquement en contexte analytique
///   (après `=`, `:`, ou en début de valeur structurée)
/// - Dédoublonnage : si un même fragment pseudo mappe vers plusieurs originaux
///   différents, il est ignoré (ambiguïté)
/// - Protection des valeurs originales déjà restaurées : les zones contenant des PII
///   restaurées sont masquées pendant le remplacement de fragments pour éviter
///   toute corruption (ex: `o'brien` corrompu par un fragment IP `o`)
pub fn restore_fragments(text: &str, mapping: &MappingTable) -> String {
    let entries = mapping.all_entries_with_type();

    let mut all_fragments: Vec<FragmentPair> = Vec::new();

    // Collecter les valeurs originales pour les protéger
    let mut originals_to_protect: Vec<String> = Vec::new();

    for (pseudo, original, pii_type) in &entries {
        originals_to_protect.push(original.clone());
        let fragments = decompose_fragments(pseudo, original, *pii_type);
        all_fragments.extend(fragments);
    }

    if all_fragments.is_empty() {
        return text.to_string();
    }

    // Dédoublonnage : si un pseudo_fragment mappe vers plusieurs originaux différents,
    // c'est ambigu → on l'exclut
    let mut fragment_map: HashMap<String, Vec<String>> = HashMap::new();
    for pair in &all_fragments {
        fragment_map
            .entry(pair.pseudo_fragment.clone())
            .or_default()
            .push(pair.original_fragment.clone());
    }

    // Garder seulement les fragments non ambigus
    let mut safe_fragments: Vec<FragmentPair> = Vec::new();
    for pair in &all_fragments {
        let targets = &fragment_map[&pair.pseudo_fragment];
        // Vérifier que toutes les cibles sont identiques
        if targets.iter().all(|t| t == &pair.original_fragment) {
            // Vérifier qu'on n'a pas déjà ajouté ce fragment
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

    // Trier par longueur décroissante du fragment pseudo (plus longs d'abord)
    safe_fragments.sort_by(|a, b| b.pseudo_fragment.len().cmp(&a.pseudo_fragment.len()));

    // --- Protection des valeurs originales ---
    // Remplacer temporairement les valeurs déjà restaurées par des placeholders
    // pour empêcher le remplacement de fragments de les corrompre.
    let mut result = text.to_string();
    let mut placeholders: Vec<(String, String)> = Vec::new();

    // Trier par longueur décroissante pour éviter les remplacements partiels
    let mut sorted_originals = originals_to_protect;
    sorted_originals.sort_by_key(|b| std::cmp::Reverse(b.len()));
    sorted_originals.dedup();

    for (i, original) in sorted_originals.iter().enumerate() {
        let placeholder = format!("\x00PROTECT_{}\x00", i);
        result = result.replace(original, &placeholder);
        placeholders.push((placeholder, original.clone()));
    }

    // --- Remplacement des fragments ---
    for pair in &safe_fragments {
        if pair.pseudo_fragment.len() < 2 {
            // Fragments courts : remplacement contextuel uniquement
            // Contexte : après = ou : (avec espace optionnel), ou dans une liste numérique
            let escaped = regex::escape(&pair.pseudo_fragment);
            let pattern = format!(r"(?<=[=:,]\s?){escaped}(?:\b|(?=[,\s\)\]}}]))", escaped = escaped);
            if let Ok(re) = Regex::new(&pattern) {
                result = re
                    .replace_all(&result, pair.original_fragment.as_str())
                    .to_string();
            }
        } else {
            // Fragments ≥ 2 chars : remplacement avec limites de mot
            let pattern = format!(r"\b{}\b", regex::escape(&pair.pseudo_fragment));
            if let Ok(re) = Regex::new(&pattern) {
                result = re
                    .replace_all(&result, pair.original_fragment.as_str())
                    .to_string();
            }
        }
    }

    // --- Restauration des valeurs protégées ---
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
        // Si un octet est identique, il n'est pas inclus
        let fragments = decompose_ip("172.0.84.3", "172.16.254.3");
        assert_eq!(fragments.len(), 2); // seuls octets 2 et 3 diffèrent
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

        // Texte après dé-pseudonymisation principale (IP complète déjà restaurée)
        // mais les fragments analytiques contiennent encore les valeurs du pseudonyme
        let text = "L'adresse IP est 172.16.254.3. Premier octet: 10, deuxième: 84, classe du réseau: A";
        let result = restore_fragments(text, &mapping);

        assert!(result.contains("Premier octet: 172"));
        assert!(result.contains("deuxième: 254"));
        // "10" dans "172.16.254.3" ne doit pas être touché (pas isolé comme mot)
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

        // Aucun remplacement de fragments pour les emails
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
        // Deux IPs avec le même octet pseudo "10" mais des originaux différents
        mapping
            .insert("192.168.1.1", "10.0.50.20", PiiType::IpAddress)
            .unwrap();
        mapping
            .insert("172.16.0.1", "10.0.60.30", PiiType::IpAddress)
            .unwrap();

        // Le fragment "10" mappe vers "192" ET "172" → ambigu, on ne touche pas
        let text = "Octet: 10";
        let result = restore_fragments(text, &mapping);

        // "10" ne doit PAS être remplacé car ambigu
        assert!(result.contains("10"));
    }

    #[test]
    fn test_restored_email_not_corrupted_by_fragments() {
        let mapping = MappingTable::new();
        // Un email avec chars spéciaux + une IP dans le même mapping
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

        // Après dé-pseudonymisation principale, les deux valeurs sont restaurées
        // Le fragment restorer ne doit PAS corrompre l'email restauré
        let text = "Contact: o'brien+newsletter@hyphen-domain.co.uk, IP: 85.123.45.67, octet: 10";
        let result = restore_fragments(text, &mapping);

        // L'email doit être intact
        assert!(
            result.contains("o'brien+newsletter@hyphen-domain.co.uk"),
            "L'email restauré a été corrompu : {}",
            result
        );
        // L'IP doit être intacte
        assert!(result.contains("85.123.45.67"));
        // Le fragment isolé doit être remplacé
        assert!(result.contains("octet: 85"));
    }

    #[test]
    fn test_restored_ip_not_corrupted_by_fragments() {
        let mapping = MappingTable::new();
        mapping
            .insert("172.16.254.3", "10.0.84.12", PiiType::IpAddress)
            .unwrap();

        // L'IP complète restaurée ne doit pas être corrompue par le remplacement
        // de ses propres fragments pseudo
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
