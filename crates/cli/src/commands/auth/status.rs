use client_core::auth::load_config;
use client_core::http::{HttpError, RestClient};

use crate::exit::{ExitError, GENERIC_ERROR};

pub(super) async fn run_status() -> Result<(), ExitError> {
    let cfg = load_config()
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not load config: {}", e), ""))?;
    if cfg.token.is_empty() {
        eprintln!("Not authenticated");
        eprintln!("  Run: cinch auth login");
        return Ok(());
    }
    // Validate the token against the relay before reporting "Authenticated".
    let relay_verified = match RestClient::new(
        cfg.relay_url.clone(),
        cfg.token.clone(),
        crate::client_info::for_cli(),
    ) {
        Err(_) => false,
        Ok(client) => match client.list_devices().await {
            Ok(_) => true,
            Err(HttpError::Unauthorized) => {
                eprintln!("Credentials expired or revoked — relay rejected the local token.");
                eprintln!("  Run: cinch auth logout && cinch auth login");
                return Ok(());
            }
            Err(HttpError::Network(e)) => {
                eprintln!("Warning: relay unreachable ({}); showing local state.", e);
                false
            }
            Err(e) => {
                eprintln!(
                    "Warning: relay status check failed ({}); showing local state.",
                    e
                );
                false
            }
        },
    };
    if relay_verified {
        eprintln!("Authenticated");
    } else {
        eprintln!("Authenticated (unverified — relay unreachable)");
    }
    eprintln!(
        "  User:  {}",
        format_user_line(
            &cfg.display_name,
            &cfg.email,
            &cfg.identity_provider,
            &cfg.user_id
        )
    );
    if !cfg.user_id.is_empty() {
        eprintln!("  ID:    {}", cfg.user_id);
    }
    eprintln!("  Relay: {}", cfg.relay_url);
    let key_in_credstore = client_core::credstore::read_encryption_key(&cfg.user_id).is_some();
    let (line, hint) = crate::key_state::describe_key_state(&cfg, key_in_credstore);
    eprintln!("  {}", line);
    if let Some(h) = hint {
        eprintln!("         {}", h);
    }
    Ok(())
}

fn format_user_line(display_name: &str, email: &str, provider: &str, user_id: &str) -> String {
    if !display_name.is_empty() {
        if !email.is_empty() && !provider.is_empty() {
            format!("{} ({}, {})", display_name, email, provider)
        } else if !email.is_empty() {
            format!("{} ({})", display_name, email)
        } else if !provider.is_empty() {
            format!("{} ({})", display_name, provider)
        } else {
            display_name.to_string()
        }
    } else if !email.is_empty() {
        if !provider.is_empty() {
            format!("{} ({})", email, provider)
        } else {
            email.to_string()
        }
    } else if !user_id.is_empty() {
        user_id.to_string()
    } else {
        "(identity not cached — run: cinch auth login --force)".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_user_line_prefers_display_name() {
        let s = format_user_line("Alice", "alice@example.com", "github", "01HZ");
        assert_eq!(s, "Alice (alice@example.com, github)");
    }

    #[test]
    fn format_user_line_falls_back_to_email_then_user_id() {
        assert_eq!(
            format_user_line("", "alice@example.com", "github", "01HZ"),
            "alice@example.com (github)"
        );
        assert_eq!(
            format_user_line("", "alice@example.com", "", "01HZ"),
            "alice@example.com"
        );
        assert_eq!(format_user_line("", "", "", "01HZ"), "01HZ");
        assert_eq!(
            format_user_line("", "", "", ""),
            "(identity not cached — run: cinch auth login --force)"
        );
    }

    #[test]
    fn format_user_line_display_name_without_provider_or_email() {
        assert_eq!(format_user_line("Alice", "", "", "01HZ"), "Alice");
    }

    #[test]
    fn format_user_line_display_name_with_provider_only() {
        assert_eq!(
            format_user_line("Alice", "", "github", "01HZ"),
            "Alice (github)"
        );
    }
}
