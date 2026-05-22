use client_core::auth::load_config;
use client_core::http::RestClient;

use crate::exit::{ExitError, GENERIC_ERROR};

pub(super) async fn run_logout() -> Result<(), ExitError> {
    let cfg = load_config()
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not load config: {}", e), ""))?;

    // Best-effort relay-side revoke. Local wipe still proceeds on failure.
    if !cfg.token.is_empty() && !cfg.active_device_id.is_empty() && !cfg.relay_url.is_empty() {
        if let Ok(client) = RestClient::new(
            cfg.relay_url.clone(),
            cfg.token.clone(),
            crate::client_info::for_cli(),
        ) {
            if let Err(e) = client.revoke_device(&cfg.active_device_id).await {
                eprintln!(
                    "Warning: relay unreachable ({}) \u{2014} your device is cleared locally but still paired on the server. Revoke it from another device to fully sign out.",
                    e
                );
            }
        }
    }

    if let Some(home) = dirs::home_dir() {
        let p = home.join(".cinch").join("config.json");
        match std::fs::remove_file(&p) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(ExitError::new(
                    GENERIC_ERROR,
                    format!("Could not delete config: {}", e),
                    "",
                ));
            }
        }
    }
    eprintln!("\u{2713} Logged out. Credentials removed.");
    Ok(())
}
