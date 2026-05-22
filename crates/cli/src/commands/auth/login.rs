use std::io::IsTerminal;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use client_core::auth::load_config;
use client_core::auth_session::{install_credentials, InstallParams};
use client_core::config::default_relay_url;
use client_core::http::{HttpError, RestClient};
use client_core::machine::{hostname_or_unknown, stable_machine_id};
use sha2::{Digest, Sha256};

use super::CINCH_HOSTED_RELAY;
use crate::desktop_handoff::{
    desktop_is_default_handler_for_cinch_scheme, handoff_login, HandoffOutcome,
};
use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR, NETWORK_ERROR};

pub(super) async fn run_login(
    relay_flag: Option<String>,
    force: bool,
    headless: bool,
    user_hint_arg: Option<String>,
) -> Result<(), ExitError> {
    let mut cfg = load_config().unwrap_or_default();

    if let Some(r) = &relay_flag {
        cfg.relay_url = r.trim_end_matches('/').to_string();
    }

    // Short-circuit: this machine already has valid credentials. Skip the
    // network round-trip and let the user know — they don't need to do
    // anything (and `--force` exists if they really want a fresh sign-in).
    let machine_id = stable_machine_id();
    if !force {
        if let Some(profile) = client_core::auth::load_multi_config()
            .ok()
            .and_then(|mc| mc.active_profile().cloned())
        {
            let same_machine = !machine_id.is_empty()
                && !profile.machine_id.is_empty()
                && profile.machine_id == machine_id;
            let configured = !profile.token.is_empty()
                && !profile.user_id.is_empty()
                && !profile.device_id.is_empty();
            // Treat a missing machine_id (legacy config) as "trust the disk
            // state" — the machine_id will be backfilled on the next install.
            let trusted_disk = configured && (same_machine || profile.machine_id.is_empty());
            if trusted_disk {
                let user_short = short_id(&profile.user_id);
                eprintln!(
                    "\u{2713} Already signed in as {} on this machine.",
                    user_short
                );
                eprintln!("  Pass --force to sign in again.");
                return Ok(());
            }
        }
    }

    // Resolve the user hint before clearing the token so that
    // cfg.token.is_empty() inside resolve_user_hint reflects the true
    // on-disk state (empty = first-time or already-logged-out; non-empty =
    // --force re-login).
    let user_hint = resolve_user_hint(user_hint_arg, &cfg);

    // Force re-auth even if a token is present, matching Go's
    // `cfg.Token = ""` reset before the wizard.
    cfg.token.clear();

    let interactive = !headless && std::io::stdin().is_terminal() && relay_flag.is_none();

    let dev_default = default_relay_url();
    let relay_url = if interactive {
        prompt_relay_url(&cfg.relay_url)?
    } else if !cfg.relay_url.is_empty() && cfg.relay_url != dev_default {
        cfg.relay_url.clone()
    } else {
        CINCH_HOSTED_RELAY.to_string()
    };

    let hostname = hostname_or_unknown();

    // Hand the OAuth flow off to Cinch.app when it's installed on this device.
    // Both apps share `~/.cinch/config.json`, so adopting the desktop's
    // sign-in result means CLI + desktop end up authenticated together.
    // Skip in headless mode: the orchestrator drives the browser remotely.
    if !headless
        && !client_core::machine::in_ssh_session()
        && desktop_is_default_handler_for_cinch_scheme()
    {
        eprintln!("Signing in via Cinch.app… (Ctrl-C to cancel)");
        match handoff_login(&relay_url).await {
            HandoffOutcome::Adopted {
                user_id,
                relay_url: adopted_relay,
            } => {
                let user_short = short_id(&user_id);
                eprintln!(
                    "\u{2713} Signed in as {}. Both CLI and desktop now share this session.",
                    user_short
                );
                let _ = adopted_relay;
                return Ok(());
            }
            HandoffOutcome::TimedOut => {
                eprintln!("  Desktop did not complete sign-in — falling back to terminal flow.");
            }
        }
    }

    // Probe relay reachability before issuing a device code so URL typos
    // surface as a clean error before we send the user to a browser.
    let probe_client = RestClient::new(&relay_url, "", crate::client_info::for_cli())
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not init client: {}", e), ""))?;
    if let Err(e) = probe_client.probe_relay(&relay_url).await {
        return Err(ExitError::new(
            NETWORK_ERROR,
            format!("Cannot reach relay at {}.", relay_url),
            format!(
                "Check the URL — wrong port, http vs https, or hostname typo? ({})",
                e
            ),
        ));
    }

    let dc = probe_client
        .start_device_code(&relay_url, &hostname, &machine_id, user_hint.as_deref())
        .await
        .map_err(|e| match e {
            HttpError::Network(msg) => ExitError::new(
                NETWORK_ERROR,
                format!("Relay unreachable at {}.", relay_url),
                format!("Check your connection or try again later. ({})", msg),
            ),
            other => ExitError::from(other),
        })?;

    if headless {
        // Single-line marker on stdout for the orchestrator to parse.
        // No other stdout writes from this command in headless mode.
        println!(
            "{}",
            client_core::auth::format_device_code_marker(&dc.verification_uri, &dc.user_code,)
        );
        eprintln!("Approve from an already signed-in machine:");
        eprintln!("  cinch auth approve {}", dc.user_code);
        eprintln!("Waiting for approval...");
    } else if client_core::machine::in_ssh_session() {
        eprintln!("Waiting for approval from your other device\u{2026}");
        eprintln!();
        eprintln!("If you don't see the prompt on Cinch.app, approve manually:");
        eprintln!(
            "  \u{2022} Cinch.app \u{2192} \"Approve remote login\" \u{2192} enter code {}",
            dc.user_code
        );
        eprintln!(
            "  \u{2022} Or another signed-in terminal: cinch auth approve {}",
            dc.user_code
        );
        eprintln!();
        eprintln!("Browser URL (last resort):");
        eprintln!("  {}", dc.verification_uri);
    } else {
        eprintln!("Opening browser to sign in\u{2026} (Ctrl-C to cancel)");
        let _ = open::that(&dc.verification_uri);
    }

    let initial_interval_ms = dc.interval_ms.filter(|v| *v > 0).unwrap_or_else(|| {
        if dc.interval > 0 {
            (dc.interval as u64).saturating_mul(1000) as i64
        } else {
            1000
        }
    }) as u64;
    let expires_in = if dc.expires_in > 0 {
        dc.expires_in as u64
    } else {
        300
    };

    let poll = poll_with_spinner(
        &probe_client,
        &relay_url,
        &dc.device_code,
        initial_interval_ms,
        expires_in,
        interactive,
    )
    .await?;

    let poll_user_id = poll.user_id.as_deref().unwrap_or("");
    let poll_device_id = poll.device_id.as_deref().unwrap_or("");
    let poll_token = poll.token.as_deref().unwrap_or("");
    let poll_email = poll.email.as_deref().unwrap_or("");
    let poll_provider = poll.identity_provider.as_deref().unwrap_or("");
    let poll_display_name = poll.display_name.as_deref().unwrap_or("");

    // One atomic install: writes AES key + X25519 device key + token + config
    // with a single credential_version bump. Eliminates the lazy-generation
    // race where the desktop watcher could fire between the bump and the
    // key writes.
    let outcome = install_credentials(InstallParams {
        user_id: poll_user_id,
        device_id: poll_device_id,
        token: poll_token,
        relay_url: &relay_url,
        hostname: &hostname,
        device_private_key: None,
        email: poll_email,
        identity_provider: poll_provider,
        display_name: poll_display_name,
    })
    .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Saving credentials: {}", e), ""))?;

    if outcome.encryption_backend == "plaintext" && outcome.generated_encryption_key {
        eprintln!("  (encryption key stored in config — Keychain unavailable)");
    }

    let user_short = short_id(poll_user_id);
    if desktop_is_default_handler_for_cinch_scheme() {
        eprintln!(
            "\u{2713} Signed in as {}. Cinch.app on this machine now shares this session.",
            user_short
        );
    } else {
        eprintln!("\u{2713} Signed in as {}.", user_short);
    }

    // Register the device's X25519 public key with the relay and poll for
    // an encrypted master-key bundle. Without the public-key registration
    // the relay's ListPendingKeyExchanges sweep would never see this
    // device, so key_exchange_requested could not broadcast and another
    // device's key_exchange::Responder would have no path to encrypt the
    // master key for us. Bundle polling is best-effort: a 30s timeout
    // emits a warning + remediation and exits 0 (non-fatal).
    let authed_client = RestClient::new(
        relay_url.clone(),
        poll_token.to_string(),
        crate::client_info::for_cli(),
    )
    .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not init client: {}", e), ""))?;
    let priv_b64 = client_core::credstore::read_device_privkey(poll_user_id, poll_device_id)
        .ok_or_else(|| {
            ExitError::new(
                GENERIC_ERROR,
                "device private key missing after install".to_string(),
                "",
            )
        })?;
    let pub_b64 = client_core::crypto::pub_from_priv(&priv_b64)
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("derive pubkey: {}", e), ""))?;
    let raw_pub = URL_SAFE_NO_PAD
        .decode(&pub_b64)
        .expect("decode just-derived pubkey");
    let digest = Sha256::digest(&raw_pub);
    let fingerprint = digest[..4]
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();

    if let Err(e) = authed_client
        .register_device_public_key(&pub_b64, &fingerprint)
        .await
    {
        // Best-effort: a network blip here shouldn't kill an
        // otherwise-successful login. Surface a non-fatal warning and
        // suggest `cinch auth retry-key` once the relay is reachable —
        // that path goes through register_device_public_key implicitly
        // by virtue of the device row already carrying the priv key.
        match e {
            HttpError::Network(msg) => {
                eprintln!(
                    "\u{26A0} Could not reach the relay to register key ({}).",
                    msg
                );
            }
            other => {
                eprintln!("\u{26A0} Key registration failed: {}", other);
            }
        }
        eprintln!("  Run `cinch auth retry-key` once the relay is reachable.");
        if headless {
            println!("\u{2713} Paired");
        }
        return Ok(());
    }

    if headless {
        eprintln!("Authenticated. Waiting for another device to share encryption key (30s)...");
    } else {
        eprintln!("\u{2713} Authenticated. Waiting for another device to share encryption key...");
    }
    let key_received =
        client_core::auth::poll_key_bundle(&authed_client, &priv_b64, poll_user_id).await;
    if !key_received {
        eprintln!("\n\u{26A0} No paired device responded with the encryption key.");
        eprintln!("  Try one of:");
        eprintln!("    \u{2022} Open the cinch desktop app on another device");
        eprintln!("    \u{2022} Run `cinch pull --watch` from another paired device");
        eprintln!("    \u{2022} Run `cinch auth retry-key` once another paired device is online");
        eprintln!("  Until then, only unencrypted clipboard sharing will work.");
        // Non-fatal — exit 0.
    }

    if headless {
        // Single allowed stdout line on success — emitted only after the
        // full handshake (or the timeout warning above) so an SSH
        // orchestrator can rely on it as a completion marker.
        println!("\u{2713} Paired");
    }

    // T3: best-effort flush of any backlog clips captured pre-login.
    // Detached — never delays login completion or surfaces errors.
    if let Ok(ctx) = crate::runtime::open_ctx() {
        crate::runtime::spawn_session_flush(&ctx);
    }

    Ok(())
}

pub(super) fn short_id(id: &str) -> &str {
    if id.len() >= 8 {
        &id[..8]
    } else {
        id
    }
}

fn resolve_user_hint(arg: Option<String>, cfg: &client_core::config::Config) -> Option<String> {
    arg.filter(|s| !s.is_empty()).or_else(|| {
        if cfg.token.is_empty() && !cfg.email.is_empty() {
            Some(cfg.email.clone())
        } else {
            None
        }
    })
}

fn prompt_relay_url(current: &str) -> Result<String, ExitError> {
    let dev_default = default_relay_url();
    let default = if !current.is_empty() && current != dev_default {
        current.to_string()
    } else {
        CINCH_HOSTED_RELAY.to_string()
    };
    let url = inquire::Text::new("Relay URL")
        .with_default(&default)
        .with_validator(|input: &str| {
            if input.is_empty() {
                return Ok(inquire::validator::Validation::Invalid(
                    "relay URL cannot be empty".into(),
                ));
            }
            if !(input.starts_with("http://") || input.starts_with("https://")) {
                return Ok(inquire::validator::Validation::Invalid(
                    "URL must start with http:// or https://".into(),
                ));
            }
            Ok(inquire::validator::Validation::Valid)
        })
        .prompt()
        .map_err(|e| ExitError::new(AUTH_FAILURE, format!("Setup cancelled: {}", e), ""))?;
    Ok(url.trim_end_matches('/').to_string())
}

async fn poll_with_spinner(
    client: &RestClient,
    relay_url: &str,
    device_code: &str,
    initial_interval_ms: u64,
    expires_in: u64,
    interactive: bool,
) -> Result<client_core::rest::DeviceCodePollResponse, ExitError> {
    let spinner = if interactive {
        let pb = indicatif::ProgressBar::new_spinner();
        pb.enable_steady_tick(Duration::from_millis(120));
        pb.set_message("Waiting for sign-in...");
        Some(pb)
    } else {
        None
    };

    // Tight cadence early (1s) so the moment OAuth completes the CLI
    // catches it; back off to 3s after the first 30s to avoid hammering
    // the relay if the user takes a while in the browser.
    const FAST_POLL_BUDGET: Duration = Duration::from_secs(30);
    let initial_interval = Duration::from_millis(initial_interval_ms.max(250));
    let backoff_interval = Duration::from_secs(3);

    let started = Instant::now();
    let deadline = started + Duration::from_secs(expires_in);

    let result = loop {
        if Instant::now() > deadline {
            break Err(ExitError::new(
                AUTH_FAILURE,
                "Device code expired.",
                "Run: cinch auth login to try again.",
            ));
        }
        let interval = if started.elapsed() < FAST_POLL_BUDGET {
            initial_interval
        } else {
            backoff_interval
        };
        tokio::time::sleep(interval).await;
        let resp = match client.poll_device_code(relay_url, device_code).await {
            Ok(r) => r,
            Err(_) => continue, // transient — retry
        };
        match resp.status.as_str() {
            "complete" => break Ok(resp),
            "expired" => {
                break Err(ExitError::new(
                    AUTH_FAILURE,
                    "Device code expired.",
                    "Run: cinch auth login to try again.",
                ));
            }
            "denied" => {
                break Err(ExitError::new(
                    AUTH_FAILURE,
                    "Login request denied.",
                    "Approval was rejected from another device. Run: cinch auth login to try again.",
                ));
            }
            _ => {
                if !interactive {
                    eprint!(".");
                }
            }
        }
    };

    if let Some(pb) = spinner {
        pb.finish_and_clear();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::HOME_LOCK;
    use super::*;
    use client_core::config::Config;

    #[test]
    fn install_persists_display_name_from_poll() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().expect("tempdir");
        std::env::set_var("HOME", tmp.path());

        let outcome = install_credentials(InstallParams {
            user_id: "u1",
            device_id: "d1",
            token: "tok",
            relay_url: "https://r",
            hostname: "host",
            device_private_key: None,
            email: "alice@example.com",
            identity_provider: "github",
            display_name: "Alice Example",
        })
        .expect("install");
        assert!(!outcome.active_relay_id.is_empty());

        let cfg = load_config().expect("load");
        assert_eq!(cfg.display_name, "Alice Example");
        assert_eq!(cfg.email, "alice@example.com");
    }

    fn cfg(email: &str, token: &str) -> Config {
        Config {
            email: email.to_string(),
            token: token.to_string(),
            ..Config::default()
        }
    }

    #[test]
    fn flag_wins_over_stale_email() {
        let hint = resolve_user_hint(
            Some("flag@example.com".into()),
            &cfg("stale@example.com", ""),
        );
        assert_eq!(hint.as_deref(), Some("flag@example.com"));
    }

    #[test]
    fn stale_email_picked_when_token_empty() {
        let hint = resolve_user_hint(None, &cfg("stale@example.com", ""));
        assert_eq!(hint.as_deref(), Some("stale@example.com"));
    }

    #[test]
    fn no_hint_when_token_present() {
        let hint = resolve_user_hint(None, &cfg("stale@example.com", "live-token"));
        assert!(hint.is_none());
    }

    #[test]
    fn no_hint_when_email_blank() {
        let hint = resolve_user_hint(None, &cfg("", ""));
        assert!(hint.is_none());
    }

    #[test]
    fn flag_empty_string_treated_as_no_hint() {
        let hint = resolve_user_hint(Some("".into()), &cfg("", ""));
        assert!(hint.is_none());
    }

    #[test]
    fn flag_empty_string_falls_through_to_stale_email() {
        let hint = resolve_user_hint(Some("".into()), &cfg("stale@example.com", ""));
        assert_eq!(hint.as_deref(), Some("stale@example.com"));
    }
}
