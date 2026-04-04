#[derive(Debug, thiserror::Error)]
pub enum DetectionError {
    #[error("Erreur ONNX Runtime : {0}")]
    Onnx(String),

    #[error("Erreur tokenizer : {0}")]
    Tokenizer(String),

    #[error("Modèle introuvable : {0}")]
    ModelNotFound(String),

    #[error("Erreur de téléchargement : {0}")]
    Download(String),

    #[error("Erreur d'inférence : {0}")]
    Inference(String),
}
