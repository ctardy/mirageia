use std::collections::HashMap;

use crate::detection::types::{label_to_pii_type, default_threshold, PiiEntity, PiiType};

/// Raw result per token: label ID and confidence score.
#[derive(Debug, Clone)]
pub struct TokenPrediction {
    pub label_id: usize,
    pub confidence: f32,
}

/// Text segment with its global offset in the original text.
#[derive(Debug, Clone)]
pub struct TextSegment {
    pub text: String,
    pub global_offset: usize,
}

/// Extracts PII entities from per-token predictions.
///
/// - `predictions`: one prediction per token (label_id + confidence)
/// - `offsets`: mapping token -> (start_byte, end_byte) in the segment text
/// - `original_text`: the segment text
/// - `label_map`: mapping label_id -> label name (e.g., "I-EMAIL")
/// - `thresholds`: confidence thresholds per PII type (optional, uses defaults otherwise)
/// - `global_offset`: offset of the segment in the full text
pub fn extract_entities(
    predictions: &[TokenPrediction],
    offsets: &[(usize, usize)],
    original_text: &str,
    label_map: &[String],
    thresholds: &HashMap<PiiType, f32>,
    global_offset: usize,
) -> Vec<PiiEntity> {
    let mut entities: Vec<PiiEntity> = Vec::new();

    // Current state for merging consecutive tokens of the same type
    let mut current_type: Option<PiiType> = None;
    let mut current_start: usize = 0;
    let mut current_end: usize = 0;
    let mut current_confidence_sum: f32 = 0.0;
    let mut current_token_count: usize = 0;

    for (i, pred) in predictions.iter().enumerate() {
        // Skip tokens without offset (special tokens [CLS], [SEP], etc.)
        if i >= offsets.len() {
            break;
        }
        let (token_start, token_end) = offsets[i];
        if token_start == token_end {
            // Special token, no associated text
            flush_entity(
                &mut entities,
                &current_type,
                current_start,
                current_end,
                current_confidence_sum,
                current_token_count,
                original_text,
                thresholds,
                global_offset,
            );
            current_type = None;
            continue;
        }

        let label = label_map.get(pred.label_id).map(|s| s.as_str()).unwrap_or("O");
        let pii_type = label_to_pii_type(label);

        match (pii_type, &current_type) {
            // Same type as current -> extend the entity
            (Some(ptype), Some(ctype)) if ptype == *ctype => {
                current_end = token_end;
                current_confidence_sum += pred.confidence;
                current_token_count += 1;
            }
            // New PII type -> flush the previous one, start a new one
            (Some(ptype), _) => {
                flush_entity(
                    &mut entities,
                    &current_type,
                    current_start,
                    current_end,
                    current_confidence_sum,
                    current_token_count,
                    original_text,
                    thresholds,
                    global_offset,
                );
                current_type = Some(ptype);
                current_start = token_start;
                current_end = token_end;
                current_confidence_sum = pred.confidence;
                current_token_count = 1;
            }
            // Not a PII (label O) -> flush the previous one
            (None, _) => {
                flush_entity(
                    &mut entities,
                    &current_type,
                    current_start,
                    current_end,
                    current_confidence_sum,
                    current_token_count,
                    original_text,
                    thresholds,
                    global_offset,
                );
                current_type = None;
            }
        }
    }

    // Flush the last entity in progress
    flush_entity(
        &mut entities,
        &current_type,
        current_start,
        current_end,
        current_confidence_sum,
        current_token_count,
        original_text,
        thresholds,
        global_offset,
    );

    entities
}

/// Adds the accumulated entity if it exceeds the confidence threshold.
#[allow(clippy::too_many_arguments)]
fn flush_entity(
    entities: &mut Vec<PiiEntity>,
    current_type: &Option<PiiType>,
    start: usize,
    end: usize,
    confidence_sum: f32,
    token_count: usize,
    original_text: &str,
    thresholds: &HashMap<PiiType, f32>,
    global_offset: usize,
) {
    if let Some(pii_type) = current_type {
        if token_count == 0 {
            return;
        }

        let avg_confidence = confidence_sum / token_count as f32;
        let threshold = thresholds
            .get(pii_type)
            .copied()
            .unwrap_or_else(|| default_threshold(pii_type));

        if avg_confidence >= threshold {
            let text = if end <= original_text.len() {
                original_text[start..end].to_string()
            } else {
                return;
            };

            // Skip empty entities or whitespace-only entities
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return;
            }

            // Adjust start/end for the trimmed text
            let trim_left = text.len() - text.trim_start().len();
            let trim_right = text.len() - text.trim_end().len();

            entities.push(PiiEntity {
                text: trimmed.to_string(),
                entity_type: *pii_type,
                start: global_offset + start + trim_left,
                end: global_offset + end - trim_right,
                confidence: avg_confidence,
            });
        }
    }
}

/// Merges entities from overlapping segments.
/// In case of duplicates (same approximate position), keeps the one with the best score.
pub fn merge_segment_entities(segments: Vec<Vec<PiiEntity>>) -> Vec<PiiEntity> {
    let mut all_entities: Vec<PiiEntity> = segments.into_iter().flatten().collect();

    if all_entities.is_empty() {
        return all_entities;
    }

    // Sort by start position
    all_entities.sort_by_key(|e| e.start);

    let mut merged: Vec<PiiEntity> = Vec::new();

    for entity in all_entities {
        if let Some(last) = merged.last() {
            // Check for overlap
            if entity.start < last.end && entity.entity_type == last.entity_type {
                // Overlap with same type -> keep the one with the best score
                if entity.confidence > last.confidence {
                    merged.pop();
                    merged.push(entity);
                }
                // Otherwise, keep the previous one (already in merged)
            } else if entity.start < last.end {
                // Overlap with a different type -> keep the one with the best score
                if entity.confidence > last.confidence {
                    merged.pop();
                    merged.push(entity);
                }
            } else {
                merged.push(entity);
            }
        } else {
            merged.push(entity);
        }
    }

    merged
}

/// Applies softmax on a logits vector and returns the probabilities.
pub fn softmax(logits: &[f32]) -> Vec<f32> {
    let max = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let exp_sum: f32 = logits.iter().map(|&x| (x - max).exp()).sum();
    logits.iter().map(|&x| (x - max).exp() / exp_sum).collect()
}

/// Converts raw logits (per token) into predictions.
/// `logits_per_token`: for each token, a vector of logits (one per label).
pub fn logits_to_predictions(logits_per_token: &[Vec<f32>]) -> Vec<TokenPrediction> {
    logits_per_token
        .iter()
        .map(|token_logits| {
            let probs = softmax(token_logits);
            let (label_id, confidence) = probs
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(id, &conf)| (id, conf))
                .unwrap_or((0, 0.0));
            TokenPrediction {
                label_id,
                confidence,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_label_map() -> Vec<String> {
        vec![
            "I-ACCOUNTNUM".to_string(),     // 0
            "I-BUILDINGNUM".to_string(),     // 1
            "I-CITY".to_string(),            // 2
            "I-CREDITCARDNUMBER".to_string(),// 3
            "I-DATEOFBIRTH".to_string(),     // 4
            "I-DRIVERLICENSENUM".to_string(),// 5
            "I-EMAIL".to_string(),           // 6
            "I-GIVENNAME".to_string(),       // 7
            "I-IDCARDNUM".to_string(),       // 8
            "I-PASSWORD".to_string(),        // 9
            "I-SOCIALNUM".to_string(),       // 10
            "I-STREET".to_string(),          // 11
            "I-SURNAME".to_string(),         // 12
            "I-TAXNUM".to_string(),          // 13
            "I-TELEPHONENUM".to_string(),    // 14
            "I-USERNAME".to_string(),        // 15
            "I-ZIPCODE".to_string(),         // 16
            "O".to_string(),                 // 17
        ]
    }

    #[test]
    fn test_extract_single_entity() {
        let label_map = make_label_map();
        // Text: "email jean@test.fr ok"
        // Simulated tokens: ["email", " ", "jean@test.fr", " ", "ok"]
        let predictions = vec![
            TokenPrediction { label_id: 17, confidence: 0.99 }, // O
            TokenPrediction { label_id: 6, confidence: 0.95 },  // I-EMAIL
            TokenPrediction { label_id: 17, confidence: 0.99 }, // O
        ];
        let offsets = vec![
            (0, 5),   // "email"
            (6, 18),  // "jean@test.fr"
            (19, 21), // "ok"
        ];
        let text = "email jean@test.fr ok";
        let thresholds = HashMap::new();

        let entities = extract_entities(&predictions, &offsets, text, &label_map, &thresholds, 0);

        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].text, "jean@test.fr");
        assert_eq!(entities[0].entity_type, PiiType::Email);
        assert_eq!(entities[0].start, 6);
        assert_eq!(entities[0].end, 18);
    }

    #[test]
    fn test_extract_multi_token_entity() {
        let label_map = make_label_map();
        // Text: "Je suis Jean Dupont ici"
        // Tokens: ["Je", " suis", " Jean", " Dupont", " ici"]
        let predictions = vec![
            TokenPrediction { label_id: 17, confidence: 0.99 }, // O - "Je"
            TokenPrediction { label_id: 17, confidence: 0.99 }, // O - "suis"
            TokenPrediction { label_id: 7, confidence: 0.92 },  // I-GIVENNAME - "Jean"
            TokenPrediction { label_id: 7, confidence: 0.88 },  // I-GIVENNAME - "Dupont" (same type -> merge)
            TokenPrediction { label_id: 17, confidence: 0.99 }, // O - "ici"
        ];
        let offsets = vec![
            (0, 2),   // "Je"
            (3, 7),   // "suis"
            (8, 12),  // "Jean"
            (13, 19), // "Dupont"
            (20, 23), // "ici"
        ];
        let text = "Je suis Jean Dupont ici";
        let thresholds = HashMap::new();

        let entities = extract_entities(&predictions, &offsets, text, &label_map, &thresholds, 0);

        assert_eq!(entities.len(), 1);
        // Note: "Jean" + "Dupont" merged because same consecutive type
        // Text between offsets 8..19 = "Jean Dupont" (with space in between)
        assert_eq!(entities[0].text, "Jean Dupont");
        assert_eq!(entities[0].entity_type, PiiType::GivenName);
        assert_eq!(entities[0].start, 8);
        assert_eq!(entities[0].end, 19);
        assert!((entities[0].confidence - 0.90).abs() < 0.01); // average of 0.92 and 0.88
    }

    #[test]
    fn test_extract_no_entities() {
        let label_map = make_label_map();
        let predictions = vec![
            TokenPrediction { label_id: 17, confidence: 0.99 },
            TokenPrediction { label_id: 17, confidence: 0.99 },
        ];
        let offsets = vec![(0, 5), (6, 11)];
        let text = "hello world";
        let thresholds = HashMap::new();

        let entities = extract_entities(&predictions, &offsets, text, &label_map, &thresholds, 0);
        assert!(entities.is_empty());
    }

    #[test]
    fn test_extract_below_threshold_filtered() {
        let label_map = make_label_map();
        let predictions = vec![
            TokenPrediction { label_id: 6, confidence: 0.3 }, // EMAIL but low confidence
        ];
        let offsets = vec![(0, 8)];
        let text = "test@x.y";
        let thresholds = HashMap::new(); // default threshold for Email = 0.75

        let entities = extract_entities(&predictions, &offsets, text, &label_map, &thresholds, 0);
        assert!(entities.is_empty()); // filtered because 0.3 < 0.75
    }

    #[test]
    fn test_extract_with_global_offset() {
        let label_map = make_label_map();
        let predictions = vec![
            TokenPrediction { label_id: 6, confidence: 0.95 },
        ];
        let offsets = vec![(0, 12)];
        let text = "jean@test.fr";
        let thresholds = HashMap::new();

        let entities = extract_entities(&predictions, &offsets, text, &label_map, &thresholds, 100);

        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].start, 100);
        assert_eq!(entities[0].end, 112);
    }

    #[test]
    fn test_extract_multiple_different_entities() {
        let label_map = make_label_map();
        // "Jean, email: jean@test.fr"
        let predictions = vec![
            TokenPrediction { label_id: 7, confidence: 0.90 },  // I-GIVENNAME
            TokenPrediction { label_id: 17, confidence: 0.99 }, // O
            TokenPrediction { label_id: 6, confidence: 0.95 },  // I-EMAIL
        ];
        let offsets = vec![
            (0, 4),   // "Jean"
            (4, 13),  // ", email: "
            (13, 25), // "jean@test.fr"
        ];
        let text = "Jean, email: jean@test.fr";
        let thresholds = HashMap::new();

        let entities = extract_entities(&predictions, &offsets, text, &label_map, &thresholds, 0);

        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0].text, "Jean");
        assert_eq!(entities[0].entity_type, PiiType::GivenName);
        assert_eq!(entities[1].text, "jean@test.fr");
        assert_eq!(entities[1].entity_type, PiiType::Email);
    }

    #[test]
    fn test_softmax() {
        let logits = vec![1.0, 2.0, 3.0];
        let probs = softmax(&logits);

        // Verify that sum = 1
        let sum: f32 = probs.iter().sum();
        assert!((sum - 1.0).abs() < 1e-5);

        // Verify the order
        assert!(probs[2] > probs[1]);
        assert!(probs[1] > probs[0]);
    }

    #[test]
    fn test_softmax_single_element() {
        let probs = softmax(&[5.0]);
        assert!((probs[0] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_logits_to_predictions() {
        let logits = vec![
            vec![0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 10.0, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1],
            vec![0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 10.0],
        ];
        let preds = logits_to_predictions(&logits);

        assert_eq!(preds.len(), 2);
        assert_eq!(preds[0].label_id, 6);  // I-EMAIL (index 6 a le logit max)
        assert!(preds[0].confidence > 0.9);
        assert_eq!(preds[1].label_id, 17); // O (index 17 a le logit max)
    }

    #[test]
    fn test_merge_no_overlap() {
        let seg1 = vec![PiiEntity {
            text: "Jean".to_string(),
            entity_type: PiiType::GivenName,
            start: 0, end: 4, confidence: 0.9,
        }];
        let seg2 = vec![PiiEntity {
            text: "jean@test.fr".to_string(),
            entity_type: PiiType::Email,
            start: 20, end: 32, confidence: 0.95,
        }];

        let merged = merge_segment_entities(vec![seg1, seg2]);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn test_merge_overlap_same_type_keeps_best() {
        let seg1 = vec![PiiEntity {
            text: "Jean Dupont".to_string(),
            entity_type: PiiType::GivenName,
            start: 0, end: 11, confidence: 0.85,
        }];
        let seg2 = vec![PiiEntity {
            text: "Jean Dupont".to_string(),
            entity_type: PiiType::GivenName,
            start: 0, end: 11, confidence: 0.92,
        }];

        let merged = merge_segment_entities(vec![seg1, seg2]);
        assert_eq!(merged.len(), 1);
        assert!((merged[0].confidence - 0.92).abs() < 1e-5);
    }

    #[test]
    fn test_merge_empty_segments() {
        let merged = merge_segment_entities(vec![vec![], vec![]]);
        assert!(merged.is_empty());
    }

    #[test]
    fn test_whitespace_entity_ignored() {
        let label_map = make_label_map();
        let predictions = vec![
            TokenPrediction { label_id: 7, confidence: 0.90 },
        ];
        let offsets = vec![(0, 3)];
        let text = "   "; // whitespace only
        let thresholds = HashMap::new();

        let entities = extract_entities(&predictions, &offsets, text, &label_map, &thresholds, 0);
        assert!(entities.is_empty());
    }
}
