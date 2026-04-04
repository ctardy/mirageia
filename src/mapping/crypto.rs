use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, AeadCore, Key, Nonce};
use zeroize::Zeroize;

use crate::mapping::error::MappingError;

/// Moteur de chiffrement AES-256-GCM pour protéger les valeurs originales en mémoire.
/// La clé est générée aléatoirement au démarrage et zéroïsée à la destruction.
pub struct CryptoEngine {
    key_bytes: [u8; 32],
}

impl Default for CryptoEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl CryptoEngine {
    /// Génère une nouvelle clé aléatoire.
    pub fn new() -> Self {
        let key = Aes256Gcm::generate_key(OsRng);
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&key);
        Self { key_bytes }
    }

    /// Chiffre une valeur. Retourne nonce (12 bytes) + ciphertext concaténés.
    pub fn encrypt(&self, plaintext: &str) -> Result<Vec<u8>, MappingError> {
        let key = Key::<Aes256Gcm>::from_slice(&self.key_bytes);
        let cipher = Aes256Gcm::new(key);
        let nonce = Aes256Gcm::generate_nonce(OsRng);

        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|e| MappingError::Encryption(e.to_string()))?;

        // nonce (12 bytes) + ciphertext
        let mut result = Vec::with_capacity(12 + ciphertext.len());
        result.extend_from_slice(&nonce);
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }

    /// Déchiffre une valeur (nonce + ciphertext concaténés).
    pub fn decrypt(&self, data: &[u8]) -> Result<String, MappingError> {
        if data.len() < 12 {
            return Err(MappingError::Decryption(
                "Données trop courtes (< 12 bytes pour le nonce)".to_string(),
            ));
        }

        let key = Key::<Aes256Gcm>::from_slice(&self.key_bytes);
        let cipher = Aes256Gcm::new(key);
        let nonce = Nonce::from_slice(&data[..12]);
        let ciphertext = &data[12..];

        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| MappingError::Decryption(e.to_string()))?;

        String::from_utf8(plaintext)
            .map_err(|e| MappingError::Decryption(format!("UTF-8 invalide : {}", e)))
    }
}

impl Drop for CryptoEngine {
    fn drop(&mut self) {
        self.key_bytes.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let engine = CryptoEngine::new();
        let plaintext = "jean.dupont@acme.fr";

        let encrypted = engine.encrypt(plaintext).unwrap();
        let decrypted = engine.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_different_nonces() {
        let engine = CryptoEngine::new();
        let plaintext = "même texte";

        let enc1 = engine.encrypt(plaintext).unwrap();
        let enc2 = engine.encrypt(plaintext).unwrap();

        // Les nonces (12 premiers bytes) doivent être différents
        assert_ne!(&enc1[..12], &enc2[..12]);
        // Les ciphertexts aussi (car nonces différents)
        assert_ne!(enc1, enc2);

        // Mais les deux déchiffrent vers le même texte
        assert_eq!(engine.decrypt(&enc1).unwrap(), plaintext);
        assert_eq!(engine.decrypt(&enc2).unwrap(), plaintext);
    }

    #[test]
    fn test_decrypt_wrong_key_fails() {
        let engine1 = CryptoEngine::new();
        let engine2 = CryptoEngine::new();

        let encrypted = engine1.encrypt("secret").unwrap();
        let result = engine2.decrypt(&encrypted);

        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_too_short() {
        let engine = CryptoEngine::new();
        let result = engine.decrypt(&[0u8; 5]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("trop courtes"));
    }

    #[test]
    fn test_encrypt_empty_string() {
        let engine = CryptoEngine::new();
        let encrypted = engine.encrypt("").unwrap();
        let decrypted = engine.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, "");
    }

    #[test]
    fn test_encrypt_unicode() {
        let engine = CryptoEngine::new();
        let plaintext = "Éric Müller — données sensibles 🔒";
        let encrypted = engine.encrypt(plaintext).unwrap();
        let decrypted = engine.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }
}
