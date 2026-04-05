#[cfg(feature = "onnx")]
use std::path::Path;
use std::path::PathBuf;

use crate::detection::error::DetectionError;

/// ONNX model for PII detection.
#[cfg(feature = "onnx")]
pub struct PiiModel {
    // Mutex needed because Session::run requires &mut self in ort 2.0-rc
    session: std::sync::Mutex<ort::session::Session>,
}

#[cfg(feature = "onnx")]
impl PiiModel {
    /// Loads an ONNX model from a file.
    pub fn load(model_path: &Path) -> Result<Self, DetectionError> {
        // ort 2.0-rc: Session::builder() returns Result, each builder method also returns Result
        let session = ort::session::Session::builder()
            .map_err(|e| DetectionError::Onnx(e.to_string()))?
            .commit_from_file(model_path)
            .map_err(|e| DetectionError::Onnx(e.to_string()))?;

        tracing::info!("Modèle ONNX chargé depuis {:?}", model_path);

        Ok(Self { session: std::sync::Mutex::new(session) })
    }

    /// Runs inference on input_ids and attention_mask.
    /// Returns raw logits: for each token, a vector of scores per label.
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

        // ort 2.0-rc: Tensor::from_array(ndarray::Array) creates an ort Value<Tensor<T>>
        // which implements Into<SessionInputValue> correctly
        let ids_tensor = ort::value::Tensor::<i64>::from_array(input_ids_array)
            .map_err(|e| DetectionError::Inference(e.to_string()))?;
        let mask_tensor = ort::value::Tensor::<i64>::from_array(attention_mask_array)
            .map_err(|e| DetectionError::Inference(e.to_string()))?;

        // Hold the lock for the full duration since SessionOutputs borrows from Session
        let mut session = self.session.lock().unwrap();
        let outputs = session
            .run(ort::inputs![
                "input_ids" => ids_tensor,
                "attention_mask" => mask_tensor
            ])
            .map_err(|e| DetectionError::Inference(e.to_string()))?;

        // ort 2.0-rc: try_extract_tensor returns (&Shape, &[f32]) — raw (shape, data) tuple
        let (shape, data) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| DetectionError::Inference(e.to_string()))?;

        // Shape should be [1, seq_len, num_labels]
        if shape.len() != 3 || shape[0] != 1 {
            return Err(DetectionError::Inference(format!(
                "Shape inattendue : {:?}, attendu [1, seq_len, num_labels]",
                shape
            )));
        }

        let num_tokens = shape[1] as usize;
        let num_labels = shape[2] as usize;
        let mut logits_per_token = Vec::with_capacity(num_tokens);

        for token_idx in 0..num_tokens {
            let mut token_logits = Vec::with_capacity(num_labels);
            for label_idx in 0..num_labels {
                token_logits.push(data[token_idx * num_labels + label_idx]);
            }
            logits_per_token.push(token_logits);
        }

        Ok(logits_per_token)
    }
}

/// Returns the path to the models directory (~/.mirageia/models/).
pub fn models_dir() -> Result<PathBuf, DetectionError> {
    let home = dirs::home_dir().ok_or_else(|| {
        DetectionError::ModelNotFound("Impossible de trouver le répertoire home".to_string())
    })?;

    Ok(home.join(".mirageia").join("models"))
}

/// Checks that the model and tokenizer exist in the models directory.
/// Returns the paths (model.onnx, tokenizer.json).
pub fn check_model_files(model_name: &str) -> Result<(PathBuf, PathBuf), DetectionError> {
    let safe_name = model_name.replace('/', "__");
    let dir = models_dir()?.join(safe_name);
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
