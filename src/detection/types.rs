use serde::{Deserialize, Serialize};
use std::fmt;

/// Types of personally identifiable information detected by the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PiiType {
    PersonName,
    GivenName,
    Surname,
    Email,
    IpAddress,
    PhoneNumber,
    PostalAddress,
    Street,
    City,
    ZipCode,
    CreditCard,
    Iban,
    AccountNumber,
    NationalId,
    TaxNumber,
    DriverLicense,
    DateOfBirth,
    Username,
    Password,
    ApiKey,
    InternalUrl,
    ServerName,
    FilePath,
    Unknown,
}

impl fmt::Display for PiiType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PiiType::PersonName => write!(f, "PERSON_NAME"),
            PiiType::GivenName => write!(f, "GIVEN_NAME"),
            PiiType::Surname => write!(f, "SURNAME"),
            PiiType::Email => write!(f, "EMAIL"),
            PiiType::IpAddress => write!(f, "IP_ADDRESS"),
            PiiType::PhoneNumber => write!(f, "PHONE_NUMBER"),
            PiiType::PostalAddress => write!(f, "POSTAL_ADDRESS"),
            PiiType::Street => write!(f, "STREET"),
            PiiType::City => write!(f, "CITY"),
            PiiType::ZipCode => write!(f, "ZIP_CODE"),
            PiiType::CreditCard => write!(f, "CREDIT_CARD"),
            PiiType::Iban => write!(f, "IBAN"),
            PiiType::AccountNumber => write!(f, "ACCOUNT_NUMBER"),
            PiiType::NationalId => write!(f, "NATIONAL_ID"),
            PiiType::TaxNumber => write!(f, "TAX_NUMBER"),
            PiiType::DriverLicense => write!(f, "DRIVER_LICENSE"),
            PiiType::DateOfBirth => write!(f, "DATE_OF_BIRTH"),
            PiiType::Username => write!(f, "USERNAME"),
            PiiType::Password => write!(f, "PASSWORD"),
            PiiType::ApiKey => write!(f, "API_KEY"),
            PiiType::InternalUrl => write!(f, "INTERNAL_URL"),
            PiiType::ServerName => write!(f, "SERVER_NAME"),
            PiiType::FilePath => write!(f, "FILE_PATH"),
            PiiType::Unknown => write!(f, "UNKNOWN"),
        }
    }
}

/// Converts a model label (e.g., "I-EMAIL", "I-GIVENNAME") to PiiType.
pub fn label_to_pii_type(label: &str) -> Option<PiiType> {
    // Strip the BIO prefix (B-, I-) if present
    let raw = label
        .strip_prefix("B-")
        .or_else(|| label.strip_prefix("I-"))
        .unwrap_or(label);

    match raw {
        "GIVENNAME" | "FIRSTNAME" => Some(PiiType::GivenName),
        "SURNAME" | "LASTNAME" => Some(PiiType::Surname),
        "EMAIL" => Some(PiiType::Email),
        "TELEPHONENUM" | "PHONE" | "PHONE_NUMBER" => Some(PiiType::PhoneNumber),
        "CREDITCARDNUMBER" | "CREDIT_CARD" => Some(PiiType::CreditCard),
        "STREET" => Some(PiiType::Street),
        "CITY" => Some(PiiType::City),
        "ZIPCODE" | "ZIP_CODE" => Some(PiiType::ZipCode),
        "DATEOFBIRTH" | "DATE_OF_BIRTH" => Some(PiiType::DateOfBirth),
        "ACCOUNTNUM" | "ACCOUNT_NUMBER" | "IBAN" => Some(PiiType::AccountNumber),
        "SOCIALNUM" | "NATIONAL_ID" | "SSN" => Some(PiiType::NationalId),
        "TAXNUM" | "TAX_NUMBER" => Some(PiiType::TaxNumber),
        "DRIVERLICENSENUM" | "DRIVER_LICENSE" => Some(PiiType::DriverLicense),
        "IDCARDNUM" | "ID_CARD" => Some(PiiType::NationalId),
        "USERNAME" => Some(PiiType::Username),
        "PASSWORD" => Some(PiiType::Password),
        "IP_ADDRESS" | "IP" => Some(PiiType::IpAddress),
        "O" => None, // not a PII
        _ => Some(PiiType::Unknown),
    }
}

/// A PII entity detected in the text.
#[derive(Debug, Clone, PartialEq)]
pub struct PiiEntity {
    /// The detected original text.
    pub text: String,
    /// The PII type.
    pub entity_type: PiiType,
    /// Start position in the original text (byte offset).
    pub start: usize,
    /// End position in the original text (byte offset).
    pub end: usize,
    /// Confidence score (0.0 to 1.0).
    pub confidence: f32,
}

impl fmt::Display for PiiEntity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] \"{}\" ({}..{}) conf={:.2}",
            self.entity_type, self.text, self.start, self.end, self.confidence
        )
    }
}

/// Default confidence threshold for each PII type.
pub fn default_threshold(pii_type: &PiiType) -> f32 {
    match pii_type {
        // Lower thresholds for high-value types (prefer a false positive)
        PiiType::ApiKey | PiiType::Password => 0.5,
        PiiType::CreditCard | PiiType::Iban | PiiType::AccountNumber => 0.6,
        PiiType::NationalId | PiiType::TaxNumber | PiiType::DriverLicense => 0.6,
        // Standard threshold
        _ => 0.75,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_label_to_pii_type_with_prefix() {
        assert_eq!(label_to_pii_type("I-EMAIL"), Some(PiiType::Email));
        assert_eq!(label_to_pii_type("B-GIVENNAME"), Some(PiiType::GivenName));
        assert_eq!(label_to_pii_type("I-SURNAME"), Some(PiiType::Surname));
        assert_eq!(label_to_pii_type("I-TELEPHONENUM"), Some(PiiType::PhoneNumber));
        assert_eq!(label_to_pii_type("I-CREDITCARDNUMBER"), Some(PiiType::CreditCard));
    }

    #[test]
    fn test_label_to_pii_type_without_prefix() {
        assert_eq!(label_to_pii_type("EMAIL"), Some(PiiType::Email));
        assert_eq!(label_to_pii_type("STREET"), Some(PiiType::Street));
    }

    #[test]
    fn test_label_o_returns_none() {
        assert_eq!(label_to_pii_type("O"), None);
    }

    #[test]
    fn test_label_unknown() {
        assert_eq!(label_to_pii_type("SOMETHING_ELSE"), Some(PiiType::Unknown));
    }

    #[test]
    fn test_default_thresholds() {
        assert!(default_threshold(&PiiType::ApiKey) < default_threshold(&PiiType::Email));
        assert!(default_threshold(&PiiType::CreditCard) < default_threshold(&PiiType::GivenName));
        assert_eq!(default_threshold(&PiiType::Email), 0.75);
        assert_eq!(default_threshold(&PiiType::Password), 0.5);
    }

    #[test]
    fn test_pii_entity_display() {
        let entity = PiiEntity {
            text: "jean@test.fr".to_string(),
            entity_type: PiiType::Email,
            start: 10,
            end: 22,
            confidence: 0.95,
        };
        let display = format!("{}", entity);
        assert!(display.contains("EMAIL"));
        assert!(display.contains("jean@test.fr"));
        assert!(display.contains("0.95"));
    }

    #[test]
    fn test_pii_type_display() {
        assert_eq!(format!("{}", PiiType::Email), "EMAIL");
        assert_eq!(format!("{}", PiiType::GivenName), "GIVEN_NAME");
        assert_eq!(format!("{}", PiiType::CreditCard), "CREDIT_CARD");
    }

    #[test]
    fn test_label_aliases() {
        // Verify that aliases work
        assert_eq!(label_to_pii_type("I-FIRSTNAME"), Some(PiiType::GivenName));
        assert_eq!(label_to_pii_type("I-LASTNAME"), Some(PiiType::Surname));
        assert_eq!(label_to_pii_type("I-PHONE"), Some(PiiType::PhoneNumber));
        assert_eq!(label_to_pii_type("I-SSN"), Some(PiiType::NationalId));
        assert_eq!(label_to_pii_type("I-IDCARDNUM"), Some(PiiType::NationalId));
    }
}
