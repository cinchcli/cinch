//! `cinch auth recovery` — backup and restore the AES-256 encryption key
//! as a 24-word BIP39 phrase.
//!
//! Without a recovery code, losing the local credential store (Keychain
//! eviction, OS reinstall, hardware loss before pairing another device)
//! means every encrypted clip on the relay is unrecoverable. `show`
//! prints the words once; `restore` accepts them on a new machine;
//! `verify` confirms a backup was recorded correctly without changing
//! anything on disk.

use std::io::{IsTerminal, Write};

use client_core::auth::load_config;
use client_core::credstore::{read_encryption_key, write_encryption_key};
use client_core::recovery::{key_to_words, words_to_key, RecoveryError};

use crate::exit::{ExitError, AUTH_FAILURE, ENCRYPTION_REQUIRED, GENERIC_ERROR};

#[derive(Debug, clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, clap::Subcommand)]
pub enum Cmd {
    /// Print the encryption key as 24 BIP39 words. Record the phrase
    /// somewhere only you can reach (password manager, paper backup).
    /// Anyone with the phrase can decrypt your clipboard history.
    Show {
        /// Skip the interactive confirmation prompt. Required for piping
        /// or redirecting output.
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Restore the encryption key on this device from a 24-word phrase.
    /// Run after `cinch auth login` on a new machine. If a key is already
    /// stored, asks before overwriting.
    Restore {
        /// 24 BIP39 words separated by whitespace. Quote the whole phrase
        /// so the shell treats it as a single argument.
        phrase: String,
        /// Skip the overwrite confirmation prompt.
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Check a 24-word phrase against the currently stored key without
    /// changing anything. Useful for verifying a backup before relying
    /// on it.
    Verify { phrase: String },
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    match args.cmd {
        Cmd::Show { yes } => run_show(yes),
        Cmd::Restore { phrase, yes } => run_restore(&phrase, yes),
        Cmd::Verify { phrase } => run_verify(&phrase),
    }
}

fn require_user_id() -> Result<String, ExitError> {
    let cfg = load_config()
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not load config: {}", e), ""))?;
    if cfg.user_id.is_empty() {
        return Err(ExitError::new(
            AUTH_FAILURE,
            "Not authenticated on this machine.",
            "Run: cinch auth login",
        ));
    }
    Ok(cfg.user_id)
}

fn run_show(yes: bool) -> Result<(), ExitError> {
    let user_id = require_user_id()?;
    let key = read_encryption_key(&user_id).ok_or_else(|| {
        ExitError::new(
            ENCRYPTION_REQUIRED,
            "No encryption key stored on this device.",
            "Run: cinch auth retry-key to receive it from a paired device.",
        )
    })?;

    if !yes {
        // Refuse to leak the phrase into pipes or files without an
        // explicit opt-in — accidental `> log.txt` would otherwise
        // capture the secret. `--yes` exists for intentional
        // `cinch auth recovery show --yes > backup.txt` flows.
        if !std::io::stdout().is_terminal() {
            return Err(ExitError::new(
                GENERIC_ERROR,
                "Refusing to print the recovery code to a non-terminal without --yes.",
                "Re-run with --yes (e.g. `cinch auth recovery show --yes > backup.txt`) if that is intentional.",
            ));
        }
        let confirm = inquire::Confirm::new(
            "This prints your encryption key as 24 words. \
             Anyone who sees them can decrypt your clipboard history. Continue?",
        )
        .with_default(false)
        .prompt()
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Prompt cancelled: {}", e), ""))?;
        if !confirm {
            eprintln!("Aborted.");
            return Ok(());
        }
    }

    let phrase = key_to_words(&key);
    // Words on stdout so a user can redirect them; warnings on stderr so
    // they never land in the backup file.
    println!("{}", phrase);
    let _ = std::io::stdout().flush();
    eprintln!();
    eprintln!("Store this somewhere only you can reach (password manager, paper backup).");
    eprintln!("Without it, losing access to every paired device means losing every");
    eprintln!("encrypted clip on the relay.");

    Ok(())
}

fn run_restore(phrase: &str, yes: bool) -> Result<(), ExitError> {
    let user_id = require_user_id()?;
    let key = decode_phrase(phrase)?;

    if let Some(existing) = read_encryption_key(&user_id) {
        if existing == key {
            eprintln!("\u{2713} Recovery code matches the key already stored — nothing to do.");
            return Ok(());
        }
        if !yes {
            if !std::io::stdin().is_terminal() {
                return Err(ExitError::new(
                    GENERIC_ERROR,
                    "A different encryption key is already stored on this device.",
                    "Re-run with --yes to overwrite (clips encrypted under the old key will become unreadable).",
                ));
            }
            let confirm = inquire::Confirm::new(
                "A different encryption key is already stored on this device. \
                 Overwriting makes clips encrypted under the old key unreadable. Continue?",
            )
            .with_default(false)
            .prompt()
            .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Prompt cancelled: {}", e), ""))?;
            if !confirm {
                eprintln!("Aborted.");
                return Ok(());
            }
        }
    }

    write_encryption_key(&user_id, &key)
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not save key: {}", e), ""))?;
    eprintln!("\u{2713} Encryption key restored. cinch push/pull are ready.");
    Ok(())
}

fn run_verify(phrase: &str) -> Result<(), ExitError> {
    let user_id = require_user_id()?;
    let key = decode_phrase(phrase)?;
    let existing = read_encryption_key(&user_id).ok_or_else(|| {
        ExitError::new(
            ENCRYPTION_REQUIRED,
            "No encryption key stored on this device to verify against.",
            "Run `cinch auth retry-key`, or `cinch auth recovery restore <phrase>` to install from a backup.",
        )
    })?;
    if existing == key {
        eprintln!("\u{2713} Recovery code matches the stored encryption key.");
        Ok(())
    } else {
        Err(ExitError::new(
            GENERIC_ERROR,
            "Recovery code does not match the stored encryption key.",
            "Double-check each word against your backup.",
        ))
    }
}

fn decode_phrase(phrase: &str) -> Result<[u8; 32], ExitError> {
    words_to_key(phrase).map_err(|e| match e {
        RecoveryError::WrongWordCount(n) => ExitError::new(
            GENERIC_ERROR,
            format!("Recovery code must be 24 words, got {}.", n),
            "Pass all 24 words as a single quoted argument.",
        ),
        RecoveryError::BadChecksum => ExitError::new(
            GENERIC_ERROR,
            "Recovery code checksum failed.",
            "One or more words is likely mistyped — re-check against your backup.",
        ),
        RecoveryError::UnknownWord(w) => ExitError::new(
            GENERIC_ERROR,
            format!("Word `{}` is not in the BIP39 English wordlist.", w),
            "Check for typos; all words come from the standard BIP39 English list.",
        ),
        RecoveryError::Invalid(msg) => ExitError::new(
            GENERIC_ERROR,
            format!("Invalid recovery code: {}", msg),
            String::new(),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_phrase_wrong_word_count_maps_to_friendly_message() {
        let err = decode_phrase("abandon abandon abandon").unwrap_err();
        assert_eq!(err.code, GENERIC_ERROR);
        assert!(err.message.contains("24 words"));
        assert!(err.message.contains("3"));
    }

    #[test]
    fn decode_phrase_bad_checksum_suggests_typo() {
        // 24 valid words but a checksum that does not match the entropy.
        let bad = "abandon abandon abandon abandon abandon abandon abandon abandon \
                   abandon abandon abandon abandon abandon abandon abandon abandon \
                   abandon abandon abandon abandon abandon abandon abandon abandon";
        let err = decode_phrase(bad).unwrap_err();
        assert!(err.message.contains("checksum"));
        assert!(
            err.fix.to_lowercase().contains("typed") || err.fix.to_lowercase().contains("mistyped")
        );
    }

    #[test]
    fn decode_phrase_unknown_word_names_offender() {
        let bad = "zzzzz abandon abandon abandon abandon abandon abandon abandon \
                   abandon abandon abandon abandon abandon abandon abandon abandon \
                   abandon abandon abandon abandon abandon abandon abandon art";
        let err = decode_phrase(bad).unwrap_err();
        assert!(err.message.contains("zzzzz"));
    }

    #[test]
    fn decode_phrase_accepts_known_zero_vector() {
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon \
                      abandon abandon abandon abandon abandon abandon abandon abandon \
                      abandon abandon abandon abandon abandon abandon abandon art";
        let key = decode_phrase(phrase).unwrap();
        assert_eq!(key, [0u8; 32]);
    }
}
