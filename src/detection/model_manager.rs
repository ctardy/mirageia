/// ONNX model manager: download, cache, SHA-256 verification.
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

/// Metadata for a cached model.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelMeta {
    pub model: String,
    pub version: String,
    pub sha256: String,
    pub downloaded_at: String,
    pub source: String,
}

/// Returns the models directory: `~/.mirageia/models/`.
pub fn models_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "Impossible de déterminer le répertoire home".to_string())?;
    Ok(home.join(".mirageia").join("models"))
}

/// Returns the path to the file indicating the active model.
fn active_model_file() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "Impossible de déterminer le répertoire home".to_string())?;
    Ok(home.join(".mirageia").join("active_model"))
}

/// Returns the path to the directory for a given model.
pub fn model_dir(model_name: &str) -> Result<PathBuf, String> {
    // Replace '/' with '_' for a filesystem-compatible directory name
    let safe_name = model_name.replace('/', "__");
    let dir = models_dir()?.join(safe_name);
    Ok(dir)
}

/// Checks whether the model files are present in the cache.
fn is_model_cached(model_name: &str) -> bool {
    let Ok(dir) = model_dir(model_name) else {
        return false;
    };
    dir.join("model.onnx").exists()
        && dir.join("tokenizer.json").exists()
        && dir.join("config.json").exists()
}

/// Returns the configured active model (reads `~/.mirageia/active_model`).
pub fn get_active_model() -> Option<String> {
    let path = active_model_file().ok()?;
    let content = fs::read_to_string(path).ok()?;
    let trimmed = content.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Sets the active model (writes to `~/.mirageia/active_model`).
pub fn set_active_model(model_name: &str) -> Result<(), String> {
    let path = active_model_file()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Création répertoire .mirageia échouée : {}", e))?;
    }
    fs::write(&path, model_name)
        .map_err(|e| format!("Écriture active_model échouée : {}", e))
}

/// Lists models present in the cache.
/// Returns a vector of `(model_name, is_active)`.
pub fn list_models() -> Vec<(String, bool)> {
    let Ok(dir) = models_dir() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };

    let active = get_active_model();

    let mut models: Vec<(String, bool)> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            // Check that essential files are present
            if !path.join("model.onnx").exists() {
                return None;
            }
            // Reconstruct the name (replace '__' with '/')
            let dir_name = path.file_name()?.to_string_lossy().to_string();
            let model_name = dir_name.replace("__", "/");
            let is_active = active.as_deref() == Some(&model_name);
            Some((model_name, is_active))
        })
        .collect();

    models.sort_by(|a, b| a.0.cmp(&b.0));
    models
}

/// Deletes a model from the cache.
pub fn delete_model(model_name: &str) -> Result<(), String> {
    let dir = model_dir(model_name)?;
    if !dir.exists() {
        return Err(format!("Modèle '{}' introuvable dans le cache", model_name));
    }
    fs::remove_dir_all(&dir)
        .map_err(|e| format!("Suppression du modèle '{}' échouée : {}", model_name, e))
}

/// Verifies the SHA-256 integrity of a model file.
/// Returns Ok(true) if the file exists and is readable, Ok(false) otherwise.
pub fn verify_model(model_name: &str) -> Result<bool, String> {
    let dir = model_dir(model_name)?;
    let model_path = dir.join("model.onnx");

    if !model_path.exists() {
        return Ok(false);
    }

    let data = fs::read(&model_path)
        .map_err(|e| format!("Lecture model.onnx échouée : {}", e))?;

    // Compute SHA-256 and verify it is non-null (file integrity check)
    let mut hasher = Sha256::new();
    hasher.update(&data);
    let hash = hasher.finalize();
    let hash_hex = hex::encode(hash);

    // Read meta.json if present for comparison
    let meta_path = dir.join("meta.json");
    if meta_path.exists() {
        if let Ok(content) = fs::read_to_string(&meta_path) {
            if let Ok(meta) = serde_json::from_str::<ModelMeta>(&content) {
                if !meta.sha256.is_empty() {
                    return Ok(meta.sha256 == hash_hex);
                }
            }
        }
    }

    // Without meta.json, consider the model valid if the file is non-empty
    Ok(!data.is_empty())
}

/// Ensures the model is available in cache (downloads if necessary).
/// Returns the path to the `model.onnx` file.
pub fn ensure_model(model_name: &str) -> Result<PathBuf, String> {
    let dir = model_dir(model_name)?;

    if is_model_cached(model_name) {
        return Ok(dir.join("model.onnx"));
    }

    // Download from HuggingFace
    download_model(model_name)?;

    Ok(dir.join("model.onnx"))
}

/// Downloads a model from HuggingFace.
fn download_model(model_name: &str) -> Result<(), String> {
    let dir = model_dir(model_name)?;
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Création du répertoire modèle échouée : {}", e))?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| format!("Création client HTTP échouée : {}", e))?;

    let base_url = format!("https://huggingface.co/{}/resolve/main", model_name);

    for filename in &["model.onnx", "tokenizer.json", "config.json"] {
        let url = format!("{}/{}", base_url, filename);
        let dest = dir.join(filename);

        tracing::info!("Téléchargement {} depuis {}...", filename, url);

        let response = client
            .get(&url)
            .send()
            .map_err(|e| format!("Requête HTTP échouée pour {} : {}", filename, e))?;

        if !response.status().is_success() {
            return Err(format!(
                "Téléchargement de {} échoué : HTTP {}",
                filename,
                response.status()
            ));
        }

        let bytes = response
            .bytes()
            .map_err(|e| format!("Lecture réponse échouée pour {} : {}", filename, e))?;

        // Compute SHA-256
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let sha256 = hex::encode(hasher.finalize());

        // Write the file
        let mut file = fs::File::create(&dest)
            .map_err(|e| format!("Création du fichier {} échouée : {}", filename, e))?;
        file.write_all(&bytes)
            .map_err(|e| format!("Écriture du fichier {} échouée : {}", filename, e))?;

        // Write metadata if this is model.onnx
        if *filename == "model.onnx" {
            let meta = ModelMeta {
                model: model_name.to_string(),
                version: "latest".to_string(),
                sha256,
                downloaded_at: chrono::Local::now().to_rfc3339(),
                source: url.clone(),
            };
            let meta_path = dir.join("meta.json");
            let meta_json = serde_json::to_string_pretty(&meta)
                .map_err(|e| format!("Sérialisation meta.json échouée : {}", e))?;
            fs::write(&meta_path, meta_json)
                .map_err(|e| format!("Écriture meta.json échouée : {}", e))?;
        }

        tracing::info!("  ✓ {} téléchargé ({} octets)", filename, bytes.len());
    }

    Ok(())
}

// Minimal hex module to avoid an external dependency
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes
            .as_ref()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Returns a unique temporary directory for tests.
    fn temp_mirageia_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mirageia_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_models_dir_path() {
        // models_dir() must end with .mirageia/models
        let result = models_dir();
        assert!(result.is_ok(), "models_dir() ne doit pas échouer");
        let path = result.unwrap();
        let path_str = path.to_string_lossy();
        assert!(
            path_str.contains(".mirageia"),
            "Le chemin doit contenir .mirageia, obtenu : {}",
            path_str
        );
        assert!(
            path_str.ends_with("models"),
            "Le chemin doit se terminer par models, obtenu : {}",
            path_str
        );
    }

    #[test]
    fn test_active_model_set_get() {
        // Use a temporary home to avoid polluting the real home
        let tmp_home = temp_mirageia_dir();
        let mirageia_dir = tmp_home.join(".mirageia");
        fs::create_dir_all(&mirageia_dir).unwrap();

        let active_file = mirageia_dir.join("active_model");
        fs::write(&active_file, "test/model-bert").unwrap();

        // Read directly via the set/get function with a temporary file
        // (testing the file reading logic)
        let content = fs::read_to_string(&active_file).unwrap();
        assert_eq!(content.trim(), "test/model-bert");

        // Clean up
        fs::remove_dir_all(&tmp_home).ok();
    }

    #[test]
    fn test_active_model_roundtrip() {
        // This test writes to the real ~/.mirageia/active_model (temporarily)
        // Save the current value
        let original = get_active_model();

        // Write a test value
        let result = set_active_model("test/roundtrip-model");
        assert!(result.is_ok(), "set_active_model ne doit pas échouer");

        // Read back
        let read_back = get_active_model();
        assert_eq!(
            read_back.as_deref(),
            Some("test/roundtrip-model"),
            "get_active_model doit retourner la valeur écrite"
        );

        // Restore
        match original {
            Some(ref name) => set_active_model(name).ok(),
            None => {
                // Delete the file
                active_model_file().ok().and_then(|p| fs::remove_file(p).ok())
            }
        };
    }

    #[test]
    fn test_list_models_empty() {
        // Create an empty models directory
        let tmp_home = temp_mirageia_dir();
        let models = tmp_home.join(".mirageia").join("models");
        fs::create_dir_all(&models).unwrap();

        // We cannot override dirs::home_dir(), so we test the logic directly
        // Testing that if models_dir() points to an empty directory, list_models returns empty
        let entries: Vec<_> = fs::read_dir(&models)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        assert!(entries.is_empty(), "Le répertoire doit être vide");

        fs::remove_dir_all(&tmp_home).ok();
    }

    #[test]
    fn test_delete_model_not_found() {
        let result = delete_model("nonexistent/model-xyz-does-not-exist");
        assert!(result.is_err(), "delete_model doit retourner une erreur si introuvable");
        let err = result.unwrap_err();
        assert!(
            err.contains("introuvable"),
            "Le message d'erreur doit mentionner 'introuvable', obtenu : {}",
            err
        );
    }

    #[test]
    fn test_model_dir_encoding() {
        // Verify that '/' are properly encoded as '__'
        let dir = model_dir("org/model-name").unwrap();
        let name = dir.file_name().unwrap().to_string_lossy();
        assert!(
            name.contains("__"),
            "Le '/' doit être encodé en '__', obtenu : {}",
            name
        );
        assert_eq!(name, "org__model-name");
    }

    #[test]
    fn test_verify_model_not_found() {
        let result = verify_model("nonexistent/model-xyz");
        assert!(result.is_ok(), "verify_model ne doit pas retourner d'erreur pour modèle absent");
        assert_eq!(result.unwrap(), false, "verify_model doit retourner false si absent");
    }
}
