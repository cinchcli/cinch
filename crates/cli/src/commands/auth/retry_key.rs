use client_core::auth::load_config;
use client_core::http::{HttpError, RestClient};

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR, NETWORK_ERROR};

pub(super) async fn run_retry_key() -> Result<(), ExitError> {
    let cfg = load_config()
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not load config: {}", e), ""))?;
    if cfg.token.is_empty() {
        return Err(ExitError::new(
            AUTH_FAILURE,
            "Not authenticated.",
            "Run: cinch auth login",
        ));
    }
    if cfg.user_id.is_empty() || cfg.active_device_id.is_empty() {
        return Err(ExitError::new(
            AUTH_FAILURE,
            "Missing user_id or device_id in config.",
            "Run: cinch auth login",
        ));
    }
    let client = RestClient::new(
        cfg.relay_url.clone(),
        cfg.token.clone(),
        crate::client_info::for_cli(),
    )
    .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not init client: {}", e), ""))?;
    client.retry_key_bundle().await.map_err(|e| match e {
        HttpError::Unauthorized => ExitError::new(
            AUTH_FAILURE,
            "Authentication failed — your device may have been revoked or your token has expired.",
            "Run: cinch auth logout && cinch auth login",
        ),
        HttpError::Relay { status: 400, .. } => ExitError::new(
            AUTH_FAILURE,
            "This device has not registered a public key yet.",
            "Run: cinch auth login",
        ),
        HttpError::Network(msg) => ExitError::new(
            NETWORK_ERROR,
            "Relay unreachable.",
            format!("Check your connection or try again later. ({})", msg),
        ),
        other => ExitError::new(GENERIC_ERROR, format!("Retry failed: {}", other), ""),
    })?;
    eprintln!("Re-broadcast key-exchange request. Waiting for another device to respond (30s)...");

    let priv_b64 = client_core::credstore::read_device_privkey(&cfg.user_id, &cfg.active_device_id)
        .ok_or_else(|| {
            ExitError::new(
                GENERIC_ERROR,
                "Device private key missing.",
                "Run: cinch auth login",
            )
        })?;

    let key_received = client_core::auth::poll_key_bundle(&client, &priv_b64, &cfg.user_id).await;
    if key_received {
        eprintln!("\u{2713} Encryption key received. You can now use cinch push/pull.");
    } else {
        eprintln!("\u{26A0} No paired device responded within 30s.");
        eprintln!("  Make sure the Cinch desktop app or `cinch pull --watch` is running on another device.");
    }

    // T3: best-effort flush — a successful retry_key may finally unblock
    // clips that were enqueued during the prior key-less window.
    if key_received {
        if let Ok(ctx) = crate::runtime::open_ctx() {
            crate::runtime::spawn_session_flush(&ctx);
        }
    }

    Ok(())
}
