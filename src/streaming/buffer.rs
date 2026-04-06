use crate::mapping::MappingTable;

/// Buffer for de-pseudonymization during SSE streaming.
/// Accumulates text tokens and detects pseudonyms that could
/// be split across multiple chunks.
pub struct StreamBuffer {
    /// Accumulated text waiting to be flushed.
    buffer: String,
    /// Maximum length of a pseudonym (to know how much to keep in the buffer).
    max_pseudonym_len: usize,
}

impl StreamBuffer {
    pub fn new(max_pseudonym_len: usize) -> Self {
        Self {
            buffer: String::new(),
            max_pseudonym_len,
        }
    }

    /// Appends text to the buffer and returns the text ready to be flushed
    /// (de-pseudonymized if needed).
    ///
    /// The buffer keeps the last `max_pseudonym_len` characters in reserve
    /// in case a pseudonym is split between two tokens.
    pub fn push(&mut self, text: &str, mapping: &MappingTable) -> String {
        self.buffer.push_str(text);

        if self.buffer.is_empty() {
            return String::new();
        }

        // If the buffer is shorter than the max pseudonym, we cannot flush yet
        if self.buffer.len() <= self.max_pseudonym_len {
            // But check if the buffer contains a complete pseudonym
            let pairs = mapping.all_pseudonyms_sorted();
            for (pseudo, _) in &pairs {
                if self.buffer.contains(pseudo.as_str()) {
                    // The pseudonym is complete, we can flush everything
                    return self.flush_all(mapping);
                }
            }
            // No complete pseudonym, keep waiting
            return String::new();
        }

        // We can flush the beginning of the buffer (keep the end in reserve)
        let flush_up_to = self.buffer.len() - self.max_pseudonym_len;
        let pairs = mapping.all_pseudonyms_sorted();

        // Find the safest cut point: last whitespace before flush_up_to that does NOT
        // land inside a known pseudonym occurrence. This prevents splitting phone numbers
        // or other PII values that contain spaces (e.g., "+33 6 12 34 56 78").
        let buf_snapshot = self.buffer.clone();
        let cut_point = buf_snapshot[..flush_up_to]
            .char_indices()
            .rev()
            .find(|(pos, c)| {
                if !c.is_whitespace() {
                    return false;
                }
                let cut = *pos + c.len_utf8();
                // Reject this cut if it falls inside any pseudonym occurrence
                !pairs.iter().any(|(pseudo, _)| {
                    let plen = pseudo.len();
                    if plen < 2 {
                        return false;
                    }
                    // Search only within the window around the cut point
                    let win_start = cut.saturating_sub(plen);
                    let win_end = (cut + plen).min(buf_snapshot.len());
                    let window = &buf_snapshot[win_start..win_end];
                    if let Some(rel) = window.find(pseudo.as_str()) {
                        let abs_start = win_start + rel;
                        let abs_end = abs_start + plen;
                        abs_start < cut && abs_end > cut
                    } else {
                        false
                    }
                })
            })
            .map(|(pos, c)| pos + c.len_utf8())
            .unwrap_or(flush_up_to);

        if cut_point == 0 {
            return String::new();
        }

        let to_flush = self.buffer[..cut_point].to_string();
        self.buffer = self.buffer[cut_point..].to_string();

        // De-pseudonymize the flushed portion
        crate::pseudonymization::depseudonymize_text(&to_flush, mapping)
    }

    /// Forces a flush of all remaining content.
    /// Called when the stream ends ([DONE]).
    pub fn flush_remaining(&mut self, mapping: &MappingTable) -> String {
        self.flush_all(mapping)
    }

    fn flush_all(&mut self, mapping: &MappingTable) -> String {
        let text = std::mem::take(&mut self.buffer);
        crate::pseudonymization::depseudonymize_text(&text, mapping)
    }

    /// Current size of the buffer.
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

        // Push a lot of text without pseudonyms
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

        // The complete pseudonym is in the buffer, should flush
        assert_eq!(flushed, "Jean");
    }

    #[test]
    fn test_buffer_split_pseudonym() {
        let mapping = MappingTable::new();
        mapping.insert("Jean", "Michel", PiiType::GivenName).unwrap();

        let mut buffer = StreamBuffer::new(20);

        // First token: beginning of the pseudonym
        let f1 = buffer.push("Mic", &mapping);
        assert_eq!(f1, ""); // Not yet complete

        // Second token: end of the pseudonym
        let f2 = buffer.push("hel", &mapping);
        assert_eq!(f2, "Jean"); // Now complete
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

        // At final flush, "Mic" is not a complete pseudonym -> stays as-is
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
    fn test_buffer_phone_with_spaces_not_split() {
        let mapping = MappingTable::new();
        // Phone pseudonym contains spaces — must not be cut in the middle
        mapping
            .insert("+33 6 12 34 56 78", "+64 8 41 49 48 34", crate::detection::PiiType::PhoneNumber)
            .unwrap();

        // Reserve = max(18, 18*4+1=73) = 73 chars
        // Use a buffer reserve matching what server.rs computes
        let max_len = "+64 8 41 49 48 34".chars().count() * 4 + 1;
        let mut buffer = StreamBuffer::new(max_len);

        // Simulate streaming: long prefix + phone split across chunks
        let mut total = String::new();
        let prefix = "Bonjour, votre numéro de téléphone est ".repeat(3); // ~120 chars, forces flush
        total.push_str(&buffer.push(&prefix, &mapping));
        total.push_str(&buffer.push("+64 8 41 ", &mapping));
        total.push_str(&buffer.push("49 48 34", &mapping));
        total.push_str(&buffer.push(" merci.", &mapping));
        total.push_str(&buffer.flush_remaining(&mapping));

        assert!(
            total.contains("+33 6 12 34 56 78"),
            "Le téléphone original doit être restauré. Reçu: {}",
            total
        );
        assert!(
            !total.contains("+64 8 41 49 48 34"),
            "Le pseudonyme ne doit plus apparaître. Reçu: {}",
            total
        );
    }

    #[test]
    fn test_buffer_long_text_with_pseudonym() {
        let mapping = MappingTable::new();
        mapping.insert("192.168.1.1", "10.0.0.42", PiiType::IpAddress).unwrap();

        let mut buffer = StreamBuffer::new(20);

        // Simulate streaming: text arrives in small chunks
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
