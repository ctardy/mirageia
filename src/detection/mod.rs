pub mod error;
pub mod model;
pub mod postprocess;
pub mod regex_detector;
pub mod tokenizer;
pub mod types;

pub use types::{PiiEntity, PiiType};

#[cfg(feature = "onnx")]
use std::collections::HashMap;

use crate::detection::error::DetectionError;
#[cfg(feature = "onnx")]
use crate::detection::postprocess::{extract_entities, logits_to_predictions, merge_segment_entities};
#[cfg(feature = "onnx")]
use crate::detection::tokenizer::PiiTokenizer;
#[cfg(feature = "onnx")]
use crate::detection::types::PiiType as PType;

/// Détecteur de PII complet : tokenizer + modèle ONNX + post-traitement.
#[cfg(feature = "onnx")]
pub struct PiiDetector {
    model: model::PiiModel,
    tokenizer: PiiTokenizer,
    label_map: Vec<String>,
    thresholds: HashMap<PType, f32>,
    overlap_chars: usize,
}

#[cfg(feature = "onnx")]
impl PiiDetector {
    /// Initialise le détecteur en chargeant le modèle et le tokenizer.
    pub fn new(
        model_path: &std::path::Path,
        tokenizer_path: &std::path::Path,
        label_map: Vec<String>,
    ) -> Result<Self, DetectionError> {
        let model = model::PiiModel::load(model_path)?;
        let tokenizer = PiiTokenizer::from_file(tokenizer_path)?;

        Ok(Self {
            model,
            tokenizer,
            label_map,
            thresholds: HashMap::new(),
            overlap_chars: 200,
        })
    }

    /// Initialise depuis le répertoire de modèles par défaut (~/.mirageia/models/<model_name>/).
    pub fn from_model_name(model_name: &str) -> Result<Self, DetectionError> {
        let (model_path, tokenizer_path) = model::check_model_files(model_name)?;

        let config_path = model_path.parent().unwrap().join("config.json");
        let label_map = load_label_map(&config_path)?;

        Self::new(&model_path, &tokenizer_path, label_map)
    }

    /// Définit des seuils de confiance personnalisés par type de PII.
    pub fn with_thresholds(mut self, thresholds: HashMap<PType, f32>) -> Self {
        self.thresholds = thresholds;
        self
    }

    /// Détecte les PII dans un texte.
    pub fn detect(&self, text: &str) -> Result<Vec<PiiEntity>, DetectionError> {
        if text.trim().is_empty() {
            return Ok(vec![]);
        }

        let segments = self.tokenizer.segment_text(text, self.overlap_chars);
        let mut all_segment_entities = Vec::new();

        for segment in &segments {
            let encoded = self.tokenizer.encode(&segment.text)?;
            let logits = self.model.infer(&encoded.input_ids, &encoded.attention_mask)?;
            let predictions = logits_to_predictions(&logits);

            let entities = extract_entities(
                &predictions,
                &encoded.offsets,
                &segment.text,
                &self.label_map,
                &self.thresholds,
                segment.global_offset,
            );

            all_segment_entities.push(entities);
        }

        Ok(merge_segment_entities(all_segment_entities))
    }
}

/// Charge le label_map depuis un fichier config.json du modèle HuggingFace.
pub fn load_label_map(config_path: &std::path::Path) -> Result<Vec<String>, DetectionError> {
    let content = std::fs::read_to_string(config_path).map_err(|e| {
        DetectionError::ModelNotFound(format!("config.json introuvable : {:?} — {}", config_path, e))
    })?;

    let config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| DetectionError::Tokenizer(format!("config.json invalide : {}", e)))?;

    let id2label = config
        .get("id2label")
        .ok_or_else(|| DetectionError::Tokenizer("id2label manquant dans config.json".to_string()))?;

    let id2label_map = id2label
        .as_object()
        .ok_or_else(|| DetectionError::Tokenizer("id2label n'est pas un objet".to_string()))?;

    let max_id = id2label_map
        .keys()
        .filter_map(|k| k.parse::<usize>().ok())
        .max()
        .unwrap_or(0);

    let mut label_map = vec!["O".to_string(); max_id + 1];
    for (id_str, label_val) in id2label_map {
        if let (Ok(id), Some(label)) = (id_str.parse::<usize>(), label_val.as_str()) {
            if id <= max_id {
                label_map[id] = label.to_string();
            }
        }
    }

    Ok(label_map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_load_label_map() {
        let config = serde_json::json!({
            "id2label": {
                "0": "I-ACCOUNTNUM",
                "1": "I-EMAIL",
                "2": "I-GIVENNAME",
                "3": "O"
            }
        });

        let dir = std::env::temp_dir().join("mirageia_test_label_map");
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("config.json");
        let mut file = std::fs::File::create(&config_path).unwrap();
        file.write_all(config.to_string().as_bytes()).unwrap();

        let label_map = load_label_map(&config_path).unwrap();

        assert_eq!(label_map.len(), 4);
        assert_eq!(label_map[0], "I-ACCOUNTNUM");
        assert_eq!(label_map[1], "I-EMAIL");
        assert_eq!(label_map[2], "I-GIVENNAME");
        assert_eq!(label_map[3], "O");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_label_map_missing_file() {
        let result = load_label_map(std::path::Path::new("/nonexistent/config.json"));
        assert!(result.is_err());
    }

    #[test]
    fn test_load_label_map_invalid_json() {
        let dir = std::env::temp_dir().join("mirageia_test_invalid_json");
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("config.json");
        std::fs::write(&config_path, "not json").unwrap();

        let result = load_label_map(&config_path);
        assert!(result.is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_label_map_missing_id2label() {
        let dir = std::env::temp_dir().join("mirageia_test_no_id2label");
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("config.json");
        std::fs::write(&config_path, r#"{"model_type": "bert"}"#).unwrap();

        let result = load_label_map(&config_path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("id2label"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
