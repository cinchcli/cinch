use client_core::auth::load_config;
use client_core::http::{HttpError, RestClient};

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR, NETWORK_ERROR};

pub(super) async fn run_approve(
    user_code: &str,
    relay_flag: Option<String>,
) -> Result<(), ExitError> {
    let cfg = load_config()
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not load config: {}", e), ""))?;
    if cfg.token.is_empty() {
        return Err(ExitError::new(
            AUTH_FAILURE,
            "Not authenticated on this machine.",
            "Run: cinch auth login",
        ));
    }

    let relay_url = relay_flag
        .unwrap_or_else(|| cfg.relay_url.clone())
        .trim_end_matches('/')
        .to_string();
    if relay_url.is_empty() {
        return Err(ExitError::new(
            AUTH_FAILURE,
            "No relay configured.",
            "Run: cinch auth approve <code> --relay https://api.cinchcli.com",
        ));
    }

    let client = RestClient::new(
        relay_url.clone(),
        cfg.token.clone(),
        crate::client_info::for_cli(),
    )
    .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not init client: {}", e), ""))?;
    client
        .complete_device_code(user_code.trim())
        .await
        .map_err(|e| match e {
            HttpError::Unauthorized => ExitError::new(
                AUTH_FAILURE,
                "Local credentials were rejected by the relay.",
                "Run: cinch auth login --force",
            ),
            HttpError::Network(msg) => ExitError::new(
                NETWORK_ERROR,
                format!("Cannot reach relay at {}.", relay_url),
                format!("Check your connection or relay URL. ({})", msg),
            ),
            // The relay signals plan-tier device-cap rejections by including
            // the sentinel `device_limit_exceeded` in the error message
            // (status 400 via the REST shim, 429 / CodeResourceExhausted via
            // Connect-RPC — see relay/internal/relay/store.go). Match on the
            // substring so both paths render the same humane error.
            HttpError::Relay { ref message, .. } if message.contains("device_limit_exceeded") => {
                ExitError::new(
                    AUTH_FAILURE,
                    "Your account has reached its paired-device limit.",
                    "Run `cinch device list` to see your current devices, then `cinch device revoke <id>` to free a slot. Or upgrade at https://cinchcli.com/pricing.",
                )
            }
            other => ExitError::new(
                AUTH_FAILURE,
                "Could not approve remote login.",
                format!(
                    "Code may be expired, already used, or mistyped. ({})",
                    other
                ),
            ),
        })?;

    eprintln!("\u{2713} Approved remote login.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::HOME_LOCK;
    use super::*;
    use client_core::auth_session::{install_credentials, InstallParams};

    #[tokio::test]
    async fn run_approve_renders_humane_error_on_device_limit_exceeded() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().expect("tempdir");
        std::env::set_var("HOME", tmp.path());

        let server = MockServer::start().await;
        // Mirror the relay's REST shim: HTTP 400 + a body whose `message`
        // carries the canonical `device_limit_exceeded` sentinel from
        // relay/internal/relay/store.go.
        Mock::given(method("POST"))
            .and(path("/auth/device-code/complete"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "complete_failed",
                "message": "device_limit_exceeded: user has 3/3 active devices",
                "fix": "",
            })))
            .mount(&server)
            .await;

        // Seed local credentials so run_approve can authenticate.
        install_credentials(InstallParams {
            user_id: "u1",
            device_id: "d1",
            token: "tok",
            relay_url: &server.uri(),
            hostname: "h",
            device_private_key: None,
            email: "alice@example.com",
            identity_provider: "github",
            display_name: "Alice",
        })
        .expect("install");

        let err = run_approve("ABCD-1234", Some(server.uri()))
            .await
            .expect_err("approve must fail on device_limit_exceeded");

        assert_eq!(err.code, AUTH_FAILURE);
        assert!(
            err.message.contains("paired-device limit"),
            "expected paired-device-limit message, got: {}",
            err.message
        );
        assert!(
            err.fix.contains("cinch device list") && err.fix.contains("cinch device revoke"),
            "expected recovery hint pointing at device list+revoke, got: {}",
            err.fix
        );
        assert!(
            err.fix.contains("cinchcli.com/pricing"),
            "expected pricing link in recovery hint, got: {}",
            err.fix
        );
    }
}
