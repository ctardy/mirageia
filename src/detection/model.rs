use std::path::PathBuf;

use crate::detection::error::DetectionError;

/// Modèle ONNX pour la détection de PII.
#[cfg(feature = "onnx")]
pub struct PiiModel {
    session: ort::session::Session,
}

#[cfg(feature = "onnx")]
impl PiiModel {
    /// Charge un modèle ONNX depuis un fichier.
    pub fn load(model_path: &Path) -> Result<Self, DetectionError> {
        let session = ort::session::Session::builder()
            .and_then(|builder| builder.with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3))
            .and_then(|builder| builder.commit_from_file(model_path))
            .map_err(|e| DetectionError::Onnx(e.to_string()))?;

        tracing::info!("Modèle ONNX chargé depuis {:?}", model_path);

        Ok(Self { session })
    }

    /// Exécute l'inférence sur des input_ids et attention_mask.
    /// Retourne les logits bruts : pour chaque token, un vecteur de scores par label.
    pub fn infer(
        &self,
        input_ids: &[i64],
        attention_mask: &[i64],
    ) -> Result<Vec<Vec<f32>>, DetectionError> {
        let seq_len = input_ids.len();

        let input_ids_array =
            ndarray::Array2::from_shape_vec((1, seq_len), input_ids.to_vec())
                .map_err(|e| DetectionError::Inference(e.to_string()))?;

        let attention_mask_array =
            ndarray::Array2::from_shape_vec((1, seq_len), attention_mask.to_vec())
                .map_err(|e| DetectionError::Inference(e.to_string()))?;

        let outputs = self
            .session
            .run(ort::inputs![
                "input_ids" => input_ids_array,
                "attention_mask" => attention_mask_array
            ].map_err(|e| DetectionError::Inference(e.to_string()))?)
            .map_err(|e| DetectionError::Inference(e.to_string()))?;

        // La sortie est un tensor de shape [1, seq_len, num_labels]
        let output_tensor = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| DetectionError::Inference(e.to_string()))?;

        let view = output_tensor.view();
        let shape = view.shape();

        if shape.len() != 3 || shape[0] != 1 {
            return Err(DetectionError::Inference(format!(
                "Shape inattendue : {:?}, attendu [1, seq_len, num_labels]",
                shape
            )));
        }

        let num_labels = shape[2];
        let mut logits_per_token = Vec::with_capacity(seq_len);

        for token_idx in 0..shape[1] {
            let mut token_logits = Vec::with_capacity(num_labels);
            for label_idx in 0..num_labels {
                token_logits.push(view[[0, token_idx, label_idx]]);
            }
            logits_per_token.push(token_logits);
        }

        Ok(logits_per_token)
    }
}

/// Retourne le chemin du répertoire des modèles (~/.mirageia/models/).
pub fn models_dir() -> Result<PathBuf, DetectionError> {
    let home = dirs::home_dir().ok_or_else(|| {
        DetectionError::ModelNotFound("Impossible de trouver le répertoire home".to_string())
    })?;

    Ok(home.join(".mirageia").join("models"))
}

/// Vérifie que le modèle et le tokenizer existent dans le répertoire des modèles.
/// Retourne les chemins (model.onnx, tokenizer.json).
pub fn check_model_files(model_name: &str) -> Result<(PathBuf, PathBuf), DetectionError> {
    let dir = models_dir()?.join(model_name);
    let model_path = dir.join("model.onnx");
    let tokenizer_path = dir.join("tokenizer.json");

    if !model_path.exists() {
        return Err(DetectionError::ModelNotFound(format!(
            "Fichier modèle introuvable : {:?}. Placez le modèle ONNX dans ce répertoire.",
            model_path
        )));
    }

    if !tokenizer_path.exists() {
        return Err(DetectionError::ModelNotFound(format!(
            "Fichier tokenizer introuvable : {:?}. Placez tokenizer.json dans ce répertoire.",
            tokenizer_path
        )));
    }

    Ok((model_path, tokenizer_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_models_dir() {
        let dir = models_dir().unwrap();
        assert!(dir.ends_with(".mirageia/models") || dir.ends_with(".mirageia\\models"));
    }

    #[test]
    fn test_check_model_files_missing() {
        let result = check_model_files("nonexistent_model");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("introuvable"));
    }
}
