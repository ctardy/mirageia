use crate::detection::error::DetectionError;

/// Result of text tokenization.
#[derive(Debug, Clone)]
pub struct TokenizedInput {
    /// Token IDs.
    pub input_ids: Vec<i64>,
    /// Attention mask (1 = real token, 0 = padding).
    pub attention_mask: Vec<i64>,
    /// Mapping token_idx -> (start_byte, end_byte) in the original text.
    /// Special tokens ([CLS], [SEP]) have offset (0, 0).
    pub offsets: Vec<(usize, usize)>,
}

/// Text segment for processing long texts.
#[derive(Debug, Clone)]
pub struct TextSegment {
    pub text: String,
    pub global_offset: usize,
}

/// Wrapper around the HuggingFace tokenizer.
pub struct PiiTokenizer {
    tokenizer: tokenizers::Tokenizer,
    max_length: usize,
}

impl PiiTokenizer {
    /// Loads a tokenizer from a tokenizer.json file.
    pub fn from_file(path: &std::path::Path) -> Result<Self, DetectionError> {
        let tokenizer = tokenizers::Tokenizer::from_file(path)
            .map_err(|e| DetectionError::Tokenizer(e.to_string()))?;

        Ok(Self {
            tokenizer,
            max_length: 512,
        })
    }

    /// Tokenizes a text and returns IDs, masks, and offsets.
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

    /// Splits a long text into segments with overlap.
    /// Each segment has at most `max_tokens` useful tokens.
    /// The overlap is `overlap` characters.
    pub fn segment_text(&self, text: &str, overlap_chars: usize) -> Vec<TextSegment> {
        // Estimate size in characters per segment
        // Using a conservative ratio: ~4 chars per token on average
        let max_chars = self.max_length * 3; // underestimate for safety

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

/// Splits text into segments without a tokenizer (standalone version for tests).
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
            // Find the last space AFTER start to avoid going backwards
            text[start..end]
                .rfind(char::is_whitespace)
                .map(|pos| start + pos + 1)
                .unwrap_or(end)
        } else {
            end
        };

        // Guard: if actual_end == start, force progress
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
        // Create a ~200 char text
        let text = "mot ".repeat(50); // 200 chars
        let segments = segment_text_simple(&text, 80, 20);

        assert!(segments.len() >= 2);

        // First segment starts at 0
        assert_eq!(segments[0].global_offset, 0);

        // Each segment is at most 80 chars
        for seg in &segments {
            assert!(seg.text.len() <= 80);
        }

        // The last segment covers the end of the text
        let last = segments.last().unwrap();
        assert_eq!(last.global_offset + last.text.len(), text.len());
    }

    #[test]
    fn test_segment_overlap() {
        let text = "aaa bbb ccc ddd eee fff ggg hhh iii jjj kkk lll mmm nnn ooo ppp";
        let segments = segment_text_simple(text, 30, 10);

        assert!(segments.len() >= 2);

        // Verify overlap: the start of segment N+1 must be before the end of segment N
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

        // No segment should cut in the middle of a word
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
