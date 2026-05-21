//! BIP39 24-word encoding of the user's AES-256 encryption key.
//!
//! The 32-byte key from `crypto::generate_aes_key()` is encoded as 24
//! English words (256 bits of entropy + 8-bit BIP39 checksum). Users
//! display this once via `cinch auth recovery show` and re-import it on
//! a new device with `cinch auth recovery restore`.
//!
//! No KDF — the AES key is already high-entropy random bytes from
//! `OsRng`. BIP39 is used purely as a human-friendly transport
//! encoding with a built-in checksum that catches typos.

use bip39::{Language, Mnemonic};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RecoveryError {
    #[error("recovery code must be 24 words, got {0}")]
    WrongWordCount(usize),
    #[error("recovery code checksum failed — check for typos")]
    BadChecksum,
    #[error("unrecognized word in recovery code: {0}")]
    UnknownWord(String),
    #[error("invalid recovery code: {0}")]
    Invalid(String),
}

/// Encode a 32-byte AES key as a 24-word BIP39 phrase (English wordlist,
/// space-separated, lowercase).
pub fn key_to_words(key: &[u8; 32]) -> String {
    Mnemonic::from_entropy_in(Language::English, key)
        .expect("32 bytes is always valid BIP39 entropy")
        .to_string()
}

/// Decode a 24-word BIP39 phrase back into the 32-byte AES key.
///
/// Accepts any whitespace separator and any case. Validates word count
/// and BIP39 checksum.
pub fn words_to_key(phrase: &str) -> Result<[u8; 32], RecoveryError> {
    let normalized = phrase
        .split_whitespace()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    let word_count = normalized.split_whitespace().count();
    if word_count != 24 {
        return Err(RecoveryError::WrongWordCount(word_count));
    }
    let mnemonic = Mnemonic::parse_in_normalized(Language::English, &normalized).map_err(|e| {
        use bip39::Error as BE;
        match e {
            BE::InvalidChecksum => RecoveryError::BadChecksum,
            BE::UnknownWord(idx) => {
                let word = normalized
                    .split_whitespace()
                    .nth(idx)
                    .unwrap_or("?")
                    .to_string();
                RecoveryError::UnknownWord(word)
            }
            other => RecoveryError::Invalid(other.to_string()),
        }
    })?;
    let (entropy_bytes, entropy_len) = mnemonic.to_entropy_array();
    if entropy_len != 32 {
        return Err(RecoveryError::Invalid(format!(
            "expected 32 bytes of entropy, got {}",
            entropy_len
        )));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&entropy_bytes[..32]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_random_key() {
        let key = crate::crypto::generate_aes_key();
        let phrase = key_to_words(&key);
        assert_eq!(phrase.split_whitespace().count(), 24);
        let decoded = words_to_key(&phrase).expect("roundtrip");
        assert_eq!(decoded, key);
    }

    #[test]
    fn all_zero_key_known_vector() {
        let key = [0u8; 32];
        let phrase = key_to_words(&key);
        // BIP39 spec: 256 bits of zero entropy → "abandon" × 23 + "art".
        assert_eq!(
            phrase,
            "abandon abandon abandon abandon abandon abandon abandon abandon \
             abandon abandon abandon abandon abandon abandon abandon abandon \
             abandon abandon abandon abandon abandon abandon abandon art"
        );
        assert_eq!(words_to_key(&phrase).unwrap(), key);
    }

    #[test]
    fn accepts_mixed_case_and_extra_whitespace() {
        let key = [0u8; 32];
        let messy = "  Abandon ABANDON abandon abandon abandon abandon abandon abandon \
                     abandon  abandon abandon abandon abandon abandon abandon abandon \
                     abandon abandon abandon abandon abandon abandon abandon ART  ";
        assert_eq!(words_to_key(messy).unwrap(), key);
    }

    #[test]
    fn rejects_wrong_word_count() {
        let err = words_to_key("abandon abandon abandon").unwrap_err();
        assert!(matches!(err, RecoveryError::WrongWordCount(3)));
    }

    #[test]
    fn rejects_bad_checksum() {
        // Swap the last word ("art") for another valid wordlist entry
        // that breaks the checksum.
        let bad = "abandon abandon abandon abandon abandon abandon abandon abandon \
                   abandon abandon abandon abandon abandon abandon abandon abandon \
                   abandon abandon abandon abandon abandon abandon abandon abandon";
        let err = words_to_key(bad).unwrap_err();
        assert!(matches!(err, RecoveryError::BadChecksum));
    }

    #[test]
    fn rejects_unknown_word() {
        let bad = "zzzzz abandon abandon abandon abandon abandon abandon abandon \
                   abandon abandon abandon abandon abandon abandon abandon abandon \
                   abandon abandon abandon abandon abandon abandon abandon art";
        let err = words_to_key(bad).unwrap_err();
        assert!(matches!(err, RecoveryError::UnknownWord(ref w) if w == "zzzzz"));
    }
}
