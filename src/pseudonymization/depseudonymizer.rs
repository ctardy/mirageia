use aho_corasick::AhoCorasick;

use crate::mapping::MappingTable;
use crate::pseudonymization::fragment_restorer::restore_fragments;

/// De-pseudonymizes a text by replacing pseudonyms with the original values.
///
/// Two passes:
/// 1. **Main replacement** (AhoCorasick): replaces complete tokens AND char-array
///    decompositions (e.g., `"V","Q","W",...` produced when the LLM breaks down
///    a pseudonymized value character by character).
/// 2. **Fragment restoration** (SPB -- Sub-PII Binding): detects and replaces
///    sub-parts of pseudonyms that the LLM extracted in its analysis
///    (IP octets, CC digit groups, NSS segments, etc.).
pub fn depseudonymize_text(text: &str, mapping: &MappingTable) -> String {
    let pairs = mapping.all_pseudonyms_sorted(); // sorted by descending length

    if pairs.is_empty() {
        return text.to_string();
    }

    let mut patterns: Vec<String> = Vec::new();
    let mut replacements: Vec<String> = Vec::new();

    for (pseudo, orig) in &pairs {
        // Main pattern: the pseudonym itself
        patterns.push(pseudo.clone());
        replacements.push(orig.clone());

        // Char-array patterns: the LLM may decompose a pseudonym letter by letter.
        // Two variants are needed:
        // - Unescaped (`"c","h","a","r"`)  : SSE streaming (text delta already JSON-decoded)
        // - JSON-escaped (`\"c\",\"h\",...`): non-streaming (depseudonymizer runs on raw JSON body)
        if pseudo.chars().count() >= 2 {
            let pseudo_arr = char_array_repr(pseudo);
            let orig_arr = char_array_repr(orig);
            if pseudo_arr != orig_arr {
                patterns.push(pseudo_arr);
                replacements.push(orig_arr);
            }

            let pseudo_arr_json = char_array_repr_json_escaped(pseudo);
            let orig_arr_json = char_array_repr_json_escaped(orig);
            if pseudo_arr_json != orig_arr_json {
                patterns.push(pseudo_arr_json);
                replacements.push(orig_arr_json);
            }
        }
    }

    let pattern_refs: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();
    let replacement_refs: Vec<&str> = replacements.iter().map(|s| s.as_str()).collect();

    let ac = AhoCorasick::builder()
        .match_kind(aho_corasick::MatchKind::LeftmostLongest)
        .build(&pattern_refs)
        .expect("Erreur AhoCorasick");

    // Pass 1: replacement of complete tokens (+ char arrays)
    let result = ac.replace_all(text, &replacement_refs);

    // Pass 2: fragment restoration (SPB)
    restore_fragments(&result, mapping)
}

/// Builds the char-array representation of a string as it would appear in a JSON array.
/// e.g., "abc" → `"a","b","c"`
/// e.g., "+33 6" → `"+","3","3"," ","6"`
/// Used for SSE streaming (text deltas are already JSON-decoded).
pub fn char_array_repr(s: &str) -> String {
    s.chars()
        .map(|c| format!("\"{}\"", c))
        .collect::<Vec<_>>()
        .join(",")
}

/// Builds the char-array representation with JSON-escaped quotes.
/// e.g., "abc" → `\"a\",\"b\",\"c\"`
/// Used for non-streaming responses where depseudonymization runs on the raw JSON body.
pub fn char_array_repr_json_escaped(s: &str) -> String {
    s.chars()
        .map(|c| format!("\\\"{}\\\"", c))
        .collect::<Vec<_>>()
        .join(",")
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
        // "Michel Martin" and "Michel" are both pseudonyms
        mapping.insert("Jean-Pierre Dupont", "Michel Martin", PiiType::PersonName).unwrap();
        mapping.insert("Jean", "Michel", PiiType::GivenName).unwrap();

        // "Michel Martin" must be replaced entirely, not just "Michel"
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
    fn test_depseudonymize_char_array_password() {
        let mapping = MappingTable::new();
        mapping
            .insert("MyS3cr3tP@ssw0rd!", "VQWoiUHG0O8aBwleP", PiiType::Password)
            .unwrap();

        // SSE streaming form (unescaped quotes)
        let text = r#""password": ["V","Q","W","o","i","U","H","G","0","O","8","a","B","w","l","e","P"]"#;
        let result = depseudonymize_text(text, &mapping);
        assert!(
            result.contains(r#""M","y","S","3","c","r","3","t","P","@","s","s","w","0","r","d","!""#),
            "Forme SSE : les chars du mot de passe original doivent être restaurés. Reçu: {}",
            result
        );

        // Non-streaming form (JSON-escaped quotes, as they appear in the raw JSON body)
        let text_json = r#"\"password\": [\"V\",\"Q\",\"W\",\"o\",\"i\",\"U\",\"H\",\"G\",\"0\",\"O\",\"8\",\"a\",\"B\",\"w\",\"l\",\"e\",\"P\"]"#;
        let result_json = depseudonymize_text(text_json, &mapping);
        assert!(
            result_json.contains(r#"\"M\",\"y\",\"S\",\"3\",\"c\",\"r\",\"3\",\"t\",\"P\",\"@\",\"s\",\"s\",\"w\",\"0\",\"r\",\"d\",\"!\""#),
            "Forme JSON-échappée : les chars doivent être restaurés. Reçu: {}",
            result_json
        );
    }

    #[test]
    fn test_depseudonymize_char_array_email() {
        let mapping = MappingTable::new();
        mapping
            .insert("jean.dupont@gmail.com", "sophie@example.com", PiiType::Email)
            .unwrap();

        // SSE streaming form
        let text = r#""email": ["s","o","p","h","i","e","@","e","x","a","m","p","l","e",".","c","o","m"]"#;
        let result = depseudonymize_text(text, &mapping);
        assert!(
            result.contains(r#""j","e","a","n",".","d","u","p","o","n","t","@","g","m","a","i","l",".","c","o","m""#),
            "Les chars de l'email original doivent être restaurés. Reçu: {}",
            result
        );
    }

    #[test]
    fn test_depseudonymize_char_array_phone_with_spaces() {
        let mapping = MappingTable::new();
        mapping
            .insert("+33 6 12 34 56 78", "+64 8 41 49 48 34", PiiType::PhoneNumber)
            .unwrap();

        // SSE streaming form
        let text = r#""phone": ["+","6","4"," ","8"," ","4","1"," ","4","9"," ","4","8"," ","3","4"]"#;
        let result = depseudonymize_text(text, &mapping);
        assert!(
            result.contains(r#""+","3","3"," ","6"," ","1","2"," ","3","4"," ","5","6"," ","7","8""#),
            "Les chars du téléphone original doivent être restaurés. Reçu: {}",
            result
        );
    }

    #[test]
    fn test_depseudonymize_char_array_and_full_token_together() {
        let mapping = MappingTable::new();
        mapping
            .insert("jean@corp.fr", "paul@example.com", PiiType::Email)
            .unwrap();

        // Response has both a full token AND a char array in the same text
        let text = r#"Email: paul@example.com. Chars: ["p","a","u","l","@","e","x","a","m","p","l","e",".","c","o","m"]"#;
        let result = depseudonymize_text(text, &mapping);
        assert!(
            result.contains("jean@corp.fr"),
            "L'email complet doit être restauré. Reçu: {}",
            result
        );
        assert!(
            result.contains(r#""j","e","a","n","@","c","o","r","p",".","f","r""#),
            "Les chars de l'email doivent être restaurés. Reçu: {}",
            result
        );
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

        // The pseudonymized text no longer contains the original email
        assert!(!pseudonymized.contains("jean@acme.fr"));

        // De-pseudonymization restores the original
        let restored = depseudonymize_text(&pseudonymized, &mapping);
        assert_eq!(restored, original);
    }
}
