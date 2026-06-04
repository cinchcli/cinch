//! `cinch auth` — authentication subcommands.
//!
//! OAuth-only flow: `auth login`, `auth status`, `auth logout`.
//! For headless / SSH environments, use `auth login --headless`.
//! Cross-device bootstrap happens via the SSH-driven `cinch device pair
//! <ssh-target>` command (separate file).

use std::time::Instant;

use crate::exit::ExitError;

mod approve;
mod login;
mod logout;
mod retry_key;
mod set_name;
mod status;

pub(crate) const CINCH_HOSTED_RELAY: &str = "https://api.cinchcli.com";

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
        /// orchestrator (e.g. `cinch device pair` over SSH) can pick it up
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
    /// Set the account-wide display name (overrides the OAuth-fetched name).
    Name {
        /// Display name — trimmed; max 64 bytes UTF-8.
        name: String,
    },
    /// (deprecated) `auth set-name` → `auth name`.
    #[command(hide = true)]
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
            let result = login::run_login(relay, force, headless, user_hint).await;
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
        Cmd::Status => status::run_status().await,
        Cmd::Logout => logout::run_logout().await,
        Cmd::Approve { user_code, relay } => approve::run_approve(&user_code, relay).await,
        Cmd::RetryKey => retry_key::run_retry_key().await,
        Cmd::Name { name } => set_name::run_set_name(&name).await,
        Cmd::SetName { name } => {
            crate::commands::deprecation_note("auth set-name", "auth name");
            set_name::run_set_name(&name).await
        }
        Cmd::Recovery(rec) => crate::commands::auth_recovery::run(rec).await,
    }
}

#[cfg(test)]
pub(crate) mod test_helpers {
    /// Serializes tests that mutate the HOME environment variable.
    /// `std::env::set_var` is process-wide; concurrent tests can stomp each
    /// other's HOME -> config-file path -> loaded credentials. Acquire this lock
    /// before set_var / load_config / save_config calls in any test that needs
    /// a clean HOME environment.
    pub static HOME_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
}
