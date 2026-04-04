use crate::mapping::MappingTable;

/// Buffer pour la dé-pseudonymisation en streaming SSE.
/// Accumule les tokens texte et détecte les pseudonymes qui pourraient
/// être coupés entre plusieurs chunks.
pub struct StreamBuffer {
    /// Texte accumulé en attente de flush.
    buffer: String,
    /// Longueur maximale d'un pseudonyme (pour savoir combien garder en buffer).
    max_pseudonym_len: usize,
}

impl StreamBuffer {
    pub fn new(max_pseudonym_len: usize) -> Self {
        Self {
            buffer: String::new(),
            max_pseudonym_len,
        }
    }

    /// Ajoute du texte au buffer et retourne le texte prêt à être flushed
    /// (dé-pseudonymisé si nécessaire).
    ///
    /// Le buffer garde les derniers `max_pseudonym_len` caractères en réserve
    /// au cas où un pseudonyme serait coupé entre deux tokens.
    pub fn push(&mut self, text: &str, mapping: &MappingTable) -> String {
        self.buffer.push_str(text);

        if self.buffer.is_empty() {
            return String::new();
        }

        // Si le buffer est plus court que le max pseudonyme, on ne peut pas encore flusher
        if self.buffer.len() <= self.max_pseudonym_len {
            // Mais vérifier si le buffer contient un pseudonyme complet
            let pairs = mapping.all_pseudonyms_sorted();
            for (pseudo, _) in &pairs {
                if self.buffer.contains(pseudo.as_str()) {
                    // Le pseudonyme est complet, on peut tout flusher
                    return self.flush_all(mapping);
                }
            }
            // Pas de pseudonyme complet, on attend
            return String::new();
        }

        // On peut flusher le début du buffer (garder la fin en réserve)
        let flush_up_to = self.buffer.len() - self.max_pseudonym_len;

        // Trouver la coupure la plus sûre (fin de mot), en respectant les frontières UTF-8
        let cut_point = self.buffer[..flush_up_to]
            .char_indices()
            .rev()
            .find(|(_, c)| c.is_whitespace())
            .map(|(pos, c)| pos + c.len_utf8())
            .unwrap_or(flush_up_to);

        if cut_point == 0 {
            return String::new();
        }

        let to_flush = self.buffer[..cut_point].to_string();
        self.buffer = self.buffer[cut_point..].to_string();

        // Dé-pseudonymiser la partie flushée
        crate::pseudonymization::depseudonymize_text(&to_flush, mapping)
    }

    /// Force le flush de tout le contenu restant.
    /// Appelé quand le stream se termine ([DONE]).
    pub fn flush_remaining(&mut self, mapping: &MappingTable) -> String {
        self.flush_all(mapping)
    }

    fn flush_all(&mut self, mapping: &MappingTable) -> String {
        let text = std::mem::take(&mut self.buffer);
        crate::pseudonymization::depseudonymize_text(&text, mapping)
    }

    /// Taille actuelle du buffer.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detection::PiiType;

    #[test]
    fn test_buffer_no_pseudonym_flushes_progressively() {
        let mapping = MappingTable::new();
        let mut buffer = StreamBuffer::new(20);

        // Pousser beaucoup de texte sans pseudonymes
        let flushed = buffer.push("Ceci est un long texte sans aucune donnée sensible. ", &mapping);
        assert!(!flushed.is_empty());
        assert!(flushed.contains("Ceci est un long texte"));
    }

    #[test]
    fn test_buffer_detects_complete_pseudonym() {
        let mapping = MappingTable::new();
        mapping.insert("Jean", "Michel", PiiType::GivenName).unwrap();

        let mut buffer = StreamBuffer::new(20);
        let flushed = buffer.push("Michel", &mapping);

        // Le pseudonyme complet est dans le buffer, devrait flusher
        assert_eq!(flushed, "Jean");
    }

    #[test]
    fn test_buffer_split_pseudonym() {
        let mapping = MappingTable::new();
        mapping.insert("Jean", "Michel", PiiType::GivenName).unwrap();

        let mut buffer = StreamBuffer::new(20);

        // Premier token : début du pseudonyme
        let f1 = buffer.push("Mic", &mapping);
        assert_eq!(f1, ""); // Pas encore complet

        // Deuxième token : fin du pseudonyme
        let f2 = buffer.push("hel", &mapping);
        assert_eq!(f2, "Jean"); // Maintenant complet
    }

    #[test]
    fn test_buffer_flush_remaining() {
        let mapping = MappingTable::new();
        mapping.insert("test", "xxxx", PiiType::Unknown).unwrap();

        let mut buffer = StreamBuffer::new(20);
        buffer.push("quelque ", &mapping);

        let remaining = buffer.flush_remaining(&mapping);
        assert_eq!(remaining, "quelque ");
    }

    #[test]
    fn test_buffer_flush_remaining_with_pseudonym() {
        let mapping = MappingTable::new();
        mapping.insert("Jean", "Michel", PiiType::GivenName).unwrap();

        let mut buffer = StreamBuffer::new(20);
        buffer.push("Bonjour Mic", &mapping);

        // Au flush final, "Mic" n'est pas un pseudonyme complet → reste tel quel
        let remaining = buffer.flush_remaining(&mapping);
        assert_eq!(remaining, "Bonjour Mic");
    }

    #[test]
    fn test_buffer_empty() {
        let mapping = MappingTable::new();
        let mut buffer = StreamBuffer::new(20);

        let flushed = buffer.push("", &mapping);
        assert_eq!(flushed, "");
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_buffer_long_text_with_pseudonym() {
        let mapping = MappingTable::new();
        mapping.insert("192.168.1.1", "10.0.0.42", PiiType::IpAddress).unwrap();

        let mut buffer = StreamBuffer::new(20);

        // Simuler un streaming : le texte arrive par petits morceaux
        let mut total_output = String::new();
        total_output.push_str(&buffer.push("Le serveur ", &mapping));
        total_output.push_str(&buffer.push("10.0.", &mapping));
        total_output.push_str(&buffer.push("0.42", &mapping));
        total_output.push_str(&buffer.push(" est en ligne", &mapping));
        total_output.push_str(&buffer.flush_remaining(&mapping));

        assert!(total_output.contains("192.168.1.1"));
        assert!(!total_output.contains("10.0.0.42"));
    }
}
