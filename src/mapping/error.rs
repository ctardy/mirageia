#[derive(Debug, thiserror::Error)]
pub enum MappingError {
    #[error("Erreur de chiffrement : {0}")]
    Encryption(String),

    #[error("Erreur de déchiffrement : {0}")]
    Decryption(String),

    #[error("Internal error: {0}")]
    Internal(String),
}
