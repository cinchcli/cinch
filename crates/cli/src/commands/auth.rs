//! `cinch auth` — authentication subcommands.
//!
//! OAuth-only flow: `auth login`, `auth status`, `auth logout`.
//! For headless / SSH environments, use `auth login --headless`.
//! Cross-device bootstrap happens via the SSH-driven `cinch pair
//! <ssh-target>` command (separate file).

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

use crate::desktop_handoff::{
    desktop_is_default_handler_for_cinch_scheme, handoff_login, HandoffOutcome,
};
use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR, NETWORK_ERROR};

const CINCH_HOSTED_RELAY: &str = "https://api.cinchcli.com";

#[derive(Debug, clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, clap::Subcommand)]
pub enum Cmd {
    /// Sign in to Cinch via browser (GitHub / Google).
    Login {
        /// Override relay URL (skips the interactive relay-URL prompt).
        #[arg(long)]
        relay: Option<String>,
        /// Force a fresh sign-in even when this machine is already
        /// authenticated. Without this flag, `cinch auth login` exits
        /// immediately if a valid token + matching machine_id is on disk.
        #[arg(long)]
        force: bool,
        /// Headless mode: do not auto-open a browser. Emit a single-line
        /// stdout marker containing the device-code URL so an
        /// orchestrator (e.g. `cinch pair` over SSH) can pick it up
        /// programmatically. All other output goes to stderr.
        #[arg(long)]
        headless: bool,
        /// Hint your account email so signed-in devices (Cinch.app) receive a
        /// push approval prompt instead of you having to copy the code by hand.
        /// Defaults to the email cached in ~/.cinch/config.json from a prior
        /// session, when present and the token is empty (re-login).
        #[arg(long = "user", value_name = "EMAIL")]
        user_hint: Option<String>,
    },
    /// Show current auth state.
    Status,
    /// Remove stored credentials and revoke this device on the relay.
    Logout,
    /// Approve a remote device-code login from this signed-in machine.
    Approve {
        /// User code printed by `cinch auth login` on the remote machine.
        user_code: String,
        /// Override relay URL if the remote login is using a different relay.
        #[arg(long)]
        relay: Option<String>,
    },
    /// Ask another paired device to re-share the encryption key.
    RetryKey,
    /// Set your display name (overrides the OAuth-fetched name).
    SetName {
        /// Display name — trimmed; max 64 bytes UTF-8.
        name: String,
    },
    /// Backup or restore the encryption key as a 24-word BIP39 phrase.
    Recovery(crate::commands::auth_recovery::Args),
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    match args.cmd {
        Cmd::Login {
            relay,
            force,
            headless,
            user_hint,
        } => {
            let started = Instant::now();
            crate::telemetry::capture(
                crate::telemetry::Event::new("cli.auth.login.started")
                    .with("force", force)
                    .with("headless", headless),
            );
            let result = run_login(relay, force, headless, user_hint).await;
            let duration_ms = started.elapsed().as_millis() as u64;
            if result.is_ok() {
                if let Some(user_id) = client_core::auth::load_multi_config()
                    .ok()
                    .and_then(|mc| mc.active_profile().map(|p| p.user_id.clone()))
                    .filter(|id| !id.is_empty())
                {
                    crate::telemetry::identify(&user_id);
                }
            }
            crate::telemetry::capture(
                crate::telemetry::Event::new("cli.auth.login.completed")
                    .with("success", result.is_ok())
                    .with("duration_ms", duration_ms),
            );
            result
        }
        Cmd::Status => run_status().await,
        Cmd::Logout => run_logout().await,
        Cmd::Approve { user_code, relay } => run_approve(&user_code, relay).await,
        Cmd::RetryKey => run_retry_key().await,
        Cmd::SetName { name } => run_set_name(&name).await,
        Cmd::Recovery(rec) => crate::commands::auth_recovery::run(rec).await,
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

async fn run_login(
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

fn short_id(id: &str) -> &str {
    if id.len() >= 8 {
        &id[..8]
    } else {
        id
    }
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

async fn run_status() -> Result<(), ExitError> {
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

async fn run_logout() -> Result<(), ExitError> {
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

async fn run_approve(user_code: &str, relay_flag: Option<String>) -> Result<(), ExitError> {
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

async fn run_retry_key() -> Result<(), ExitError> {
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

async fn run_set_name(name: &str) -> Result<(), ExitError> {
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
    use super::*;
    use client_core::config::Config;

    /// Serializes tests that mutate the HOME environment variable.
    /// `std::env::set_var` is process-wide; concurrent tests can stomp each
    /// other's HOME → config-file path → loaded credentials. Acquire this lock
    /// before set_var / load_config / save_config calls in any test that needs
    /// a clean HOME environment.
    static HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
