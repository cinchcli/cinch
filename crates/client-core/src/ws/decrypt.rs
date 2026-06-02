//! In-place clip decryption for the WS receive path.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;

use super::DecryptOutcome;
use crate::crypto;
use crate::protocol::Clip;

/// Decrypt `clip.content` in place if `clip.encrypted` and a key is available.
/// Returns a typed outcome — never silently returns ciphertext as plaintext.
pub fn decrypt_clip_content(clip: &mut Clip, key: Option<[u8; 32]>) -> DecryptOutcome {
    if !clip.encrypted {
        return DecryptOutcome::Plaintext;
    }
    let Some(key) = key else {
        return DecryptOutcome::MissingKey;
    };
    let plaintext = match crypto::decrypt(&key, &clip.content) {
        Ok(p) => p,
        Err(e) => {
            return DecryptOutcome::TagFailed {
                error: e.to_string(),
            }
        }
    };
    let is_binary = clip
        .media_path
        .as_deref()
        .filter(|p| !p.is_empty())
        .is_some()
        || clip.content_type.starts_with("image");
    if is_binary {
        // Re-encode as base64 so the struct stays a valid String.
        clip.content = STANDARD.encode(&plaintext);
    } else {
        match String::from_utf8(plaintext) {
            Ok(s) => clip.content = s,
            Err(e) => {
                return DecryptOutcome::TagFailed {
                    error: format!("post-decrypt utf-8 invalid: {e}"),
                }
            }
        }
    }
    clip.encrypted = false;
    DecryptOutcome::Decoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decrypt_failure_does_not_silently_return_ciphertext() {
        let sender_key = [0x11u8; 32];
        let receiver_key = [0x22u8; 32];
        let blob = crypto::encrypt(&sender_key, b"hello from remote cli").unwrap();

        let mut clip = Clip {
            clip_id: "c1".into(),
            user_id: "u1".into(),
            content: blob.clone(),
            content_type: String::new(),
            encrypted: true,
            ..Default::default()
        };

        let outcome = decrypt_clip_content(&mut clip, Some(receiver_key));

        assert!(
            matches!(outcome, DecryptOutcome::TagFailed { .. }),
            "wrong-key decrypt must return TagFailed, got {:?}",
            outcome
        );
        assert!(clip.encrypted, "encrypted flag must remain true on failure");
        assert_eq!(
            clip.content, blob,
            "content must not be replaced with garbage plaintext"
        );
    }

    #[test]
    fn decrypt_missing_key_returns_missing_key_outcome() {
        let sender_key = [0x33u8; 32];
        let blob = crypto::encrypt(&sender_key, b"secret").unwrap();

        let mut clip = Clip {
            clip_id: "c2".into(),
            user_id: "u1".into(),
            content: blob.clone(),
            content_type: String::new(),
            encrypted: true,
            ..Default::default()
        };

        let outcome = decrypt_clip_content(&mut clip, None);
        assert_eq!(outcome, DecryptOutcome::MissingKey);
        assert!(
            clip.encrypted,
            "clip must remain encrypted when key is missing"
        );
        assert_eq!(
            clip.content, blob,
            "content must be untouched when key is missing"
        );
    }
}
