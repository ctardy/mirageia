use crate::detection::error::DetectionError;

/// Résultat de la tokenisation d'un texte.
#[derive(Debug, Clone)]
pub struct TokenizedInput {
    /// IDs des tokens.
    pub input_ids: Vec<i64>,
    /// Masque d'attention (1 = token réel, 0 = padding).
    pub attention_mask: Vec<i64>,
    /// Mapping token_idx → (start_byte, end_byte) dans le texte original.
    /// Les tokens spéciaux ([CLS], [SEP]) ont un offset (0, 0).
    pub offsets: Vec<(usize, usize)>,
}

/// Segment de texte pour le traitement de textes longs.
#[derive(Debug, Clone)]
pub struct TextSegment {
    pub text: String,
    pub global_offset: usize,
}

/// Wrapper autour du tokenizer HuggingFace.
pub struct PiiTokenizer {
    tokenizer: tokenizers::Tokenizer,
    max_length: usize,
}

impl PiiTokenizer {
    /// Charge un tokenizer depuis un fichier tokenizer.json.
    pub fn from_file(path: &std::path::Path) -> Result<Self, DetectionError> {
        let tokenizer = tokenizers::Tokenizer::from_file(path)
            .map_err(|e| DetectionError::Tokenizer(e.to_string()))?;

        Ok(Self {
            tokenizer,
            max_length: 512,
        })
    }

    /// Tokenise un texte et retourne les IDs, masques et offsets.
    pub fn encode(&self, text: &str) -> Result<TokenizedInput, DetectionError> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| DetectionError::Tokenizer(e.to_string()))?;

        let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let attention_mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&m| m as i64)
            .collect();
        let offsets: Vec<(usize, usize)> = encoding.get_offsets().to_vec();

        Ok(TokenizedInput {
            input_ids,
            attention_mask,
            offsets,
        })
    }

    /// Découpe un texte long en segments avec chevauchement.
    /// Chaque segment fait au maximum `max_tokens` tokens utiles.
    /// Le chevauchement est de `overlap` caractères.
    pub fn segment_text(&self, text: &str, overlap_chars: usize) -> Vec<TextSegment> {
        // Estimer la taille en caractères par segment
        // On utilise un ratio conservateur : ~4 chars par token en moyenne
        let max_chars = self.max_length * 3; // sous-estimation pour sécurité

        if text.len() <= max_chars {
            return vec![TextSegment {
                text: text.to_string(),
                global_offset: 0,
            }];
        }

        let mut segments = Vec::new();
        let mut start = 0;

        while start < text.len() {
            let end = (start + max_chars).min(text.len());

            let actual_end = if end < text.len() {
                text[start..end]
                    .rfind(char::is_whitespace)
                    .map(|pos| start + pos + 1)
                    .unwrap_or(end)
            } else {
                end
            };

            let actual_end = if actual_end <= start { end } else { actual_end };

            segments.push(TextSegment {
                text: text[start..actual_end].to_string(),
                global_offset: start,
            });

            if actual_end >= text.len() {
                break;
            }

            let new_start = if actual_end > overlap_chars {
                actual_end - overlap_chars
            } else {
                actual_end
            };
            start = new_start.max(start + 1);
        }

        segments
    }

    pub fn max_length(&self) -> usize {
        self.max_length
    }
}

/// Découpe un texte en segments sans tokenizer (version standalone pour les tests).
pub fn segment_text_simple(text: &str, max_chars: usize, overlap_chars: usize) -> Vec<TextSegment> {
    if text.len() <= max_chars {
        return vec![TextSegment {
            text: text.to_string(),
            global_offset: 0,
        }];
    }

    let mut segments = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let end = (start + max_chars).min(text.len());

        let actual_end = if end < text.len() {
            // Chercher le dernier espace APRÈS start pour ne pas reculer
            text[start..end]
                .rfind(char::is_whitespace)
                .map(|pos| start + pos + 1)
                .unwrap_or(end)
        } else {
            end
        };

        // Garde : si actual_end == start, forcer la progression
        let actual_end = if actual_end <= start {
            end
        } else {
            actual_end
        };

        segments.push(TextSegment {
            text: text[start..actual_end].to_string(),
            global_offset: start,
        });

        if actual_end >= text.len() {
            break;
        }

        let new_start = if actual_end > overlap_chars {
            actual_end - overlap_chars
        } else {
            actual_end
        };
        start = new_start.max(start + 1);
    }

    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_segment_short_text() {
        let segments = segment_text_simple("Hello world", 1000, 50);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "Hello world");
        assert_eq!(segments[0].global_offset, 0);
    }

    #[test]
    fn test_segment_long_text() {
        // Créer un texte de ~200 chars
        let text = "mot ".repeat(50); // 200 chars
        let segments = segment_text_simple(&text, 80, 20);

        assert!(segments.len() >= 2);

        // Premier segment commence à 0
        assert_eq!(segments[0].global_offset, 0);

        // Chaque segment fait au max 80 chars
        for seg in &segments {
            assert!(seg.text.len() <= 80);
        }

        // Le dernier segment couvre la fin du texte
        let last = segments.last().unwrap();
        assert_eq!(last.global_offset + last.text.len(), text.len());
    }

    #[test]
    fn test_segment_overlap() {
        let text = "aaa bbb ccc ddd eee fff ggg hhh iii jjj kkk lll mmm nnn ooo ppp";
        let segments = segment_text_simple(text, 30, 10);

        assert!(segments.len() >= 2);

        // Vérifier le chevauchement : le début du segment N+1 doit être avant la fin du segment N
        for i in 0..segments.len() - 1 {
            let end_of_current = segments[i].global_offset + segments[i].text.len();
            let start_of_next = segments[i + 1].global_offset;
            assert!(
                start_of_next < end_of_current,
                "Segment {} (end={}) et {} (start={}) devraient chevaucher",
                i, end_of_current, i + 1, start_of_next
            );
        }
    }

    #[test]
    fn test_segment_no_mid_word_cut() {
        let text = "abcdefghij klmnopqrst uvwxyz";
        let segments = segment_text_simple(text, 15, 5);

        // Aucun segment ne devrait couper au milieu d'un mot
        for seg in &segments {
            assert!(
                !seg.text.ends_with(|c: char| c.is_alphabetic())
                    || seg.global_offset + seg.text.len() == text.len(),
                "Segment '{}' semble coupé au milieu d'un mot",
                seg.text
            );
        }
    }

    #[test]
    fn test_segment_empty_text() {
        let segments = segment_text_simple("", 100, 20);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "");
    }

    #[test]
    fn test_segment_exact_max_chars() {
        let text = "a".repeat(100);
        let segments = segment_text_simple(&text, 100, 20);
        assert_eq!(segments.len(), 1);
    }
}
