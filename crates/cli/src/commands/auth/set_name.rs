use client_core::auth::load_config;
use client_core::http::{HttpError, RestClient};

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR, NETWORK_ERROR};

pub(super) async fn run_set_name(name: &str) -> Result<(), ExitError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ExitError::new(
            GENERIC_ERROR,
            "display_name must not be empty",
            "Pass a non-empty name: cinch auth set-name \"My Name\"",
        ));
    }
    if trimmed.len() > 64 {
        return Err(ExitError::new(
            GENERIC_ERROR,
            "display_name must be 64 bytes or fewer",
            "Shorten the name.",
        ));
    }
    let cfg = load_config()
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not load config: {}", e), ""))?;
    if cfg.token.is_empty() {
        return Err(ExitError::new(
            AUTH_FAILURE,
            "Not authenticated.",
            "Run: cinch auth login",
        ));
    }
    let client = RestClient::new(
        cfg.relay_url.clone(),
        cfg.token.clone(),
        crate::client_info::for_cli(),
    )
    .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not init client: {}", e), ""))?;
    let stored = client
        .set_display_name(trimmed)
        .await
        .map_err(|e| match e {
            HttpError::Unauthorized => ExitError::new(
                AUTH_FAILURE,
                "Authentication failed.",
                "Run: cinch auth logout && cinch auth login",
            ),
            HttpError::Relay {
                status: 400,
                message,
                ..
            } => ExitError::new(GENERIC_ERROR, message, ""),
            HttpError::Network(msg) => ExitError::new(
                NETWORK_ERROR,
                "Relay unreachable.",
                format!("Check your connection or try again later. ({})", msg),
            ),
            other => ExitError::new(GENERIC_ERROR, format!("set-name failed: {}", other), ""),
        })?;

    // Save the confirmed display_name back to the local config.
    // save_config_to_disk does not persist display_name (it mirrors only
    // auth fields), so we patch the active RelayProfile directly.
    match client_core::auth::load_multi_config() {
        Ok(mut mc) => {
            if let Some(profile) = mc.active_profile_mut() {
                profile.display_name = stored.clone();
            }
            if let Err(e) = client_core::auth::save_multi_config(&mc) {
                eprintln!("Warning: relay updated but local cache write failed: {}", e);
            }
        }
        Err(e) => {
            eprintln!("Warning: relay updated but local cache write failed: {}", e);
        }
    }

    eprintln!("\u{2713} Display name updated: {}", stored);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::HOME_LOCK;
    use super::*;
    use client_core::auth_session::{install_credentials, InstallParams};

    #[tokio::test]
    async fn run_set_name_updates_local_profile_and_calls_relay() {
        use wiremock::matchers::{body_json, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let _guard = HOME_LOCK.lock().unwrap();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/display-name"))
            .and(header("authorization", "Bearer tok"))
            .and(body_json(serde_json::json!({"display_name": "Custom"})))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"ok": true, "display_name": "Custom"})),
            )
            .mount(&server)
            .await;

        let tmp = tempfile::tempdir().expect("tempdir");
        std::env::set_var("HOME", tmp.path());

        // install_credentials stores display_name (save_config_to_disk does not).
        install_credentials(InstallParams {
            user_id: "u1",
            device_id: "d1",
            token: "tok",
            relay_url: &server.uri(),
            hostname: "h",
            device_private_key: None,
            email: "alice@example.com",
            identity_provider: "github",
            display_name: "Old",
        })
        .expect("install");

        run_set_name("Custom").await.expect("set-name");

        let updated = load_config().expect("load");
        assert_eq!(updated.display_name, "Custom");
    }

    #[tokio::test]
    async fn run_set_name_rejects_blank_locally() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().expect("tempdir");
        std::env::set_var("HOME", tmp.path());
        let err = run_set_name("   ").await.expect_err("must reject");
        let msg = format!("{:?}", err);
        assert!(
            msg.contains("must not be empty") || msg.contains("empty"),
            "expected empty-rejection message, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn run_set_name_rejects_too_long_locally() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().expect("tempdir");
        std::env::set_var("HOME", tmp.path());
        let long = "a".repeat(65);
        let err = run_set_name(&long).await.expect_err("must reject");
        let msg = format!("{:?}", err);
        assert!(
            msg.contains("64") || msg.contains("too long"),
            "expected length-rejection message, got: {}",
            msg
        );
    }
}
