use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use sha2::{Digest, Sha256};

use crate::detection::PiiType;
use crate::mapping::crypto::CryptoEngine;
use crate::mapping::error::MappingError;

/// Entry in the mapping table.
#[derive(Debug, Clone)]
pub struct MappingEntry {
    pub id: u64,
    pub encrypted_original: Vec<u8>,
    pub pseudonym: String,
    pub pii_type: PiiType,
}

/// Encrypted bidirectional mapping table.
/// - Key `by_original_hash`: SHA-256 of the original value -> entry
/// - Key `by_pseudonym`: pseudonym -> entry
///
/// Thread-safe via RwLock.
pub struct MappingTable {
    by_original_hash: RwLock<HashMap<[u8; 32], MappingEntry>>,
    by_pseudonym: RwLock<HashMap<String, MappingEntry>>,
    crypto: CryptoEngine,
    next_id: AtomicU64,
}

impl Default for MappingTable {
    fn default() -> Self {
        Self::new()
    }
}

impl MappingTable {
    pub fn new() -> Self {
        Self {
            by_original_hash: RwLock::new(HashMap::new()),
            by_pseudonym: RwLock::new(HashMap::new()),
            crypto: CryptoEngine::new(),
            next_id: AtomicU64::new(1),
        }
    }

    /// Checks whether a pseudonym already exists for this original value.
    pub fn lookup_original(&self, original: &str) -> Option<String> {
        let hash = Self::hash_original(original);
        let map = self.by_original_hash.read().unwrap();
        map.get(&hash).map(|entry| entry.pseudonym.clone())
    }

    /// Inserts a new original -> pseudonym mapping.
    /// Returns the assigned ID.
    pub fn insert(
        &self,
        original: &str,
        pseudonym: &str,
        pii_type: PiiType,
    ) -> Result<u64, MappingError> {
        let hash = Self::hash_original(original);
        let encrypted = self.crypto.encrypt(original)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        let entry = MappingEntry {
            id,
            encrypted_original: encrypted,
            pseudonym: pseudonym.to_string(),
            pii_type,
        };

        {
            let mut by_orig = self.by_original_hash.write().unwrap();
            by_orig.insert(hash, entry.clone());
        }
        {
            let mut by_pseudo = self.by_pseudonym.write().unwrap();
            by_pseudo.insert(pseudonym.to_string(), entry);
        }

        Ok(id)
    }

    /// Looks up the original value for a given pseudonym.
    /// Decrypts the value before returning it.
    pub fn lookup_pseudonym(&self, pseudonym: &str) -> Option<String> {
        let map = self.by_pseudonym.read().unwrap();
        map.get(pseudonym).and_then(|entry| {
            self.crypto
                .decrypt(&entry.encrypted_original)
                .ok()
        })
    }

    /// Returns all known pseudonyms with their decrypted original value.
    /// Sorted by descending pseudonym length (for priority replacement).
    pub fn all_pseudonyms_sorted(&self) -> Vec<(String, String)> {
        let map = self.by_pseudonym.read().unwrap();
        let mut pairs: Vec<(String, String)> = map
            .iter()
            .filter_map(|(pseudo, entry)| {
                self.crypto
                    .decrypt(&entry.encrypted_original)
                    .ok()
                    .map(|original| (pseudo.clone(), original))
            })
            .collect();

        // Sort by descending pseudonym length
        pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        pairs
    }

    /// Returns all pseudonyms with their original value and PII type.
    /// Sorted by descending pseudonym length.
    pub fn all_entries_with_type(&self) -> Vec<(String, String, PiiType)> {
        let map = self.by_pseudonym.read().unwrap();
        let mut entries: Vec<(String, String, PiiType)> = map
            .iter()
            .filter_map(|(pseudo, entry)| {
                self.crypto
                    .decrypt(&entry.encrypted_original)
                    .ok()
                    .map(|original| (pseudo.clone(), original, entry.pii_type))
            })
            .collect();

        entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        entries
    }

    /// Number of entries in the table.
    pub fn len(&self) -> usize {
        self.by_original_hash.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// SHA-256 hash of an original value (never stored in plaintext).
    fn hash_original(original: &str) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(original.as_bytes());
        hasher.finalize().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_lookup_original() {
        let table = MappingTable::new();
        table
            .insert("jean@acme.fr", "paul@example.com", PiiType::Email)
            .unwrap();

        let pseudo = table.lookup_original("jean@acme.fr");
        assert_eq!(pseudo, Some("paul@example.com".to_string()));
    }

    #[test]
    fn test_insert_and_lookup_pseudonym() {
        let table = MappingTable::new();
        table
            .insert("jean@acme.fr", "paul@example.com", PiiType::Email)
            .unwrap();

        let original = table.lookup_pseudonym("paul@example.com");
        assert_eq!(original, Some("jean@acme.fr".to_string()));
    }

    #[test]
    fn test_lookup_missing_returns_none() {
        let table = MappingTable::new();
        assert!(table.lookup_original("inconnu").is_none());
        assert!(table.lookup_pseudonym("inconnu").is_none());
    }

    #[test]
    fn test_duplicate_original_returns_same_pseudonym() {
        let table = MappingTable::new();
        table
            .insert("jean@acme.fr", "paul@example.com", PiiType::Email)
            .unwrap();

        // Verify that lookup returns the existing pseudonym
        let pseudo = table.lookup_original("jean@acme.fr");
        assert_eq!(pseudo, Some("paul@example.com".to_string()));
    }

    #[test]
    fn test_multiple_entries() {
        let table = MappingTable::new();
        table
            .insert("Jean", "Michel", PiiType::GivenName)
            .unwrap();
        table
            .insert("Dupont", "Martin", PiiType::Surname)
            .unwrap();
        table
            .insert("192.168.1.1", "10.0.0.42", PiiType::IpAddress)
            .unwrap();

        assert_eq!(table.len(), 3);
        assert_eq!(table.lookup_original("Jean"), Some("Michel".to_string()));
        assert_eq!(
            table.lookup_pseudonym("10.0.0.42"),
            Some("192.168.1.1".to_string())
        );
    }

    #[test]
    fn test_all_pseudonyms_sorted_by_length() {
        let table = MappingTable::new();
        table.insert("a", "xx", PiiType::Unknown).unwrap();
        table.insert("b", "yyyy", PiiType::Unknown).unwrap();
        table.insert("c", "zzz", PiiType::Unknown).unwrap();

        let sorted = table.all_pseudonyms_sorted();
        assert_eq!(sorted.len(), 3);
        // Longest first
        assert_eq!(sorted[0].0, "yyyy");
        assert_eq!(sorted[1].0, "zzz");
        assert_eq!(sorted[2].0, "xx");
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let table = Arc::new(MappingTable::new());
        let mut handles = vec![];

        for i in 0..10 {
            let table = Arc::clone(&table);
            handles.push(thread::spawn(move || {
                let orig = format!("user{}@test.com", i);
                let pseudo = format!("fake{}@example.com", i);
                table.insert(&orig, &pseudo, PiiType::Email).unwrap();
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(table.len(), 10);

        // Verify that each entry can be found
        for i in 0..10 {
            let orig = format!("user{}@test.com", i);
            let pseudo = format!("fake{}@example.com", i);
            assert_eq!(table.lookup_original(&orig), Some(pseudo.clone()));
            assert_eq!(table.lookup_pseudonym(&pseudo), Some(orig));
        }
    }

    #[test]
    fn test_is_empty() {
        let table = MappingTable::new();
        assert!(table.is_empty());
        table.insert("x", "y", PiiType::Unknown).unwrap();
        assert!(!table.is_empty());
    }

    #[test]
    fn test_ids_are_unique() {
        let table = MappingTable::new();
        let id1 = table.insert("a", "x", PiiType::Unknown).unwrap();
        let id2 = table.insert("b", "y", PiiType::Unknown).unwrap();
        let id3 = table.insert("c", "z", PiiType::Unknown).unwrap();

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }
}
