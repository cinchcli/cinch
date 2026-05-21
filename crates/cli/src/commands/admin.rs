//! `cinch admin` — relay administration for self-hosted operators.
//!
//! All subcommands make raw HTTP calls to `/admin/*` using the Bearer token
//! from the local config. The relay gates these endpoints with `RequireAdmin`,
//! so a 403 means the current account is not an admin.

use serde::{Deserialize, Serialize};

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR};

// ---------------------------------------------------------------------------
// Clap argument tree
// ---------------------------------------------------------------------------

#[derive(Debug, clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: AdminCmd,
}

#[derive(Debug, clap::Subcommand)]
pub enum AdminCmd {
    /// Manage invite codes (self-host).
    #[command(subcommand)]
    Invite(InviteCmd),
    /// Manage users (self-host).
    #[command(subcommand)]
    User(UserCmd),
}

#[derive(Debug, clap::Subcommand)]
pub enum InviteCmd {
    /// Create a new invite code.
    Create {
        /// Optional label for this invite (operator-side convenience only).
        #[arg(long)]
        label: Option<String>,
        /// Maximum number of times this invite can be redeemed.
        #[arg(long, default_value_t = 1)]
        uses: u32,
        /// Days until this invite expires.
        #[arg(long = "expires-days", default_value_t = 7)]
        expires_days: u32,
    },
    /// List existing invite codes (shows hashes, never plaintext codes).
    List,
    /// Revoke an invite code by its full SHA-256 hash (from `list`).
    Revoke {
        /// Full SHA-256 hex hash of the invite code.
        hash: String,
    },
}

#[derive(Debug, clap::Subcommand)]
pub enum UserCmd {
    /// List all users registered on this relay.
    List,
    /// Remove a user by ID (cannot remove your own admin account).
    Remove {
        /// User ID to remove.
        id: String,
    },
}

// ---------------------------------------------------------------------------
// Wire types (relay admin API)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct CreateInviteReq {
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_uses: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_in_days: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct CreateInviteResp {
    code: String,
    expires_at: String,
}

#[derive(Debug, Deserialize)]
struct InviteRow {
    code_hash: String,
    label: String,
    max_uses: u32,
    used_count: u32,
    // created_at is present in the relay response but not shown in the table.
    #[allow(dead_code)]
    created_at: String,
    expires_at: String,
    revoked_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListInvitesResp {
    invites: Vec<InviteRow>,
}

#[derive(Debug, Deserialize)]
struct OkResp {
    ok: bool,
}

#[derive(Debug, Deserialize)]
struct UserRow {
    id: String,
    display_name: String,
    is_admin: bool,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct ListUsersResp {
    users: Vec<UserRow>,
}

// ---------------------------------------------------------------------------
// HTTP helper
// ---------------------------------------------------------------------------

/// Returns (relay_url, bearer_token) from the local config file.
fn load_admin_creds() -> Result<(String, String), ExitError> {
    let cfg = client_core::auth::load_config().map_err(|e| {
        ExitError::new(
            AUTH_FAILURE,
            format!("Failed to load config: {e}"),
            "Run: cinch auth login",
        )
    })?;
    if cfg.token.is_empty() {
        return Err(ExitError::new(
            AUTH_FAILURE,
            "No auth token configured.",
            "Run: cinch auth login",
        ));
    }
    Ok((cfg.relay_url, cfg.token))
}

/// Build a `reqwest::Client` suitable for admin calls (TLS via rustls).
fn build_http_client() -> Result<reqwest::Client, ExitError> {
    reqwest::Client::builder()
        .use_rustls_tls()
        .build()
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("HTTP client: {e}"), ""))
}

/// Map a reqwest `StatusCode` to an `ExitError`.
async fn http_err(resp: reqwest::Response) -> ExitError {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    match status.as_u16() {
        401 | 403 => ExitError::new(
            AUTH_FAILURE,
            format!("Access denied (HTTP {status}): {body}"),
            "Your account may not have admin privileges.",
        ),
        404 => ExitError::new(
            GENERIC_ERROR,
            format!("Not found (HTTP {status}): {body}"),
            "Check the ID or hash and try again.",
        ),
        _ => ExitError::new(
            GENERIC_ERROR,
            format!("Relay error (HTTP {status}): {body}"),
            "",
        ),
    }
}

// ---------------------------------------------------------------------------
// Command dispatch
// ---------------------------------------------------------------------------

pub async fn run(args: Args) -> Result<(), ExitError> {
    match args.cmd {
        AdminCmd::Invite(cmd) => run_invite(cmd).await,
        AdminCmd::User(cmd) => run_user(cmd).await,
    }
}

// ---------------------------------------------------------------------------
// invite subcommands
// ---------------------------------------------------------------------------

async fn run_invite(cmd: InviteCmd) -> Result<(), ExitError> {
    match cmd {
        InviteCmd::Create {
            label,
            uses,
            expires_days,
        } => invite_create(label, uses, expires_days).await,
        InviteCmd::List => invite_list().await,
        InviteCmd::Revoke { hash } => invite_revoke(hash).await,
    }
}

async fn invite_create(
    label: Option<String>,
    uses: u32,
    expires_days: u32,
) -> Result<(), ExitError> {
    let (relay_url, token) = load_admin_creds()?;
    let http = build_http_client()?;

    let body = CreateInviteReq {
        label,
        max_uses: Some(uses),
        expires_in_days: Some(expires_days),
    };

    let resp = http
        .post(format!("{relay_url}/admin/invites"))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Network error: {e}"), ""))?;

    if !resp.status().is_success() {
        return Err(http_err(resp).await);
    }

    let created: CreateInviteResp = resp
        .json()
        .await
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Decode error: {e}"), ""))?;

    // Plaintext code to stdout; metadata to stderr (matches relay convention).
    println!("{}", created.code);
    eprintln!("expires: {}", created.expires_at);
    Ok(())
}

async fn invite_list() -> Result<(), ExitError> {
    let (relay_url, token) = load_admin_creds()?;
    let http = build_http_client()?;

    let resp = http
        .get(format!("{relay_url}/admin/invites"))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Network error: {e}"), ""))?;

    if !resp.status().is_success() {
        return Err(http_err(resp).await);
    }

    let list: ListInvitesResp = resp
        .json()
        .await
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Decode error: {e}"), ""))?;

    if list.invites.is_empty() {
        eprintln!("No invite codes found.");
        return Ok(());
    }

    // Compute column widths dynamically.
    const GUTTER: usize = 2;
    let hash_w = list
        .invites
        .iter()
        .map(|r| r.code_hash.len())
        .max()
        .unwrap_or(0)
        .max("HASH".len());
    let label_w = list
        .invites
        .iter()
        .map(|r| r.label.len())
        .max()
        .unwrap_or(0)
        .max("LABEL".len());
    let uses_w = list
        .invites
        .iter()
        .map(|r| format!("{}/{}", r.used_count, r.max_uses).len())
        .max()
        .unwrap_or(0)
        .max("USES".len());
    let expires_w = list
        .invites
        .iter()
        .map(|r| r.expires_at.len())
        .max()
        .unwrap_or(0)
        .max("EXPIRES".len());

    let g = " ".repeat(GUTTER);
    println!(
        "  {:<hw$}{g}{:<lw$}{g}{:<uw$}{g}{:<ew$}{g}STATUS",
        "HASH",
        "LABEL",
        "USES",
        "EXPIRES",
        hw = hash_w,
        lw = label_w,
        uw = uses_w,
        ew = expires_w,
    );

    for inv in &list.invites {
        let uses_str = format!("{}/{}", inv.used_count, inv.max_uses);
        let status = if inv.revoked_at.is_some() {
            "revoked"
        } else {
            "active"
        };
        println!(
            "  {:<hw$}{g}{:<lw$}{g}{:<uw$}{g}{:<ew$}{g}{status}",
            inv.code_hash,
            inv.label,
            uses_str,
            inv.expires_at,
            hw = hash_w,
            lw = label_w,
            uw = uses_w,
            ew = expires_w,
        );
    }

    Ok(())
}

async fn invite_revoke(hash: String) -> Result<(), ExitError> {
    let (relay_url, token) = load_admin_creds()?;
    let http = build_http_client()?;

    let resp = http
        .delete(format!("{relay_url}/admin/invites/{hash}"))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Network error: {e}"), ""))?;

    if !resp.status().is_success() {
        return Err(http_err(resp).await);
    }

    let result: OkResp = resp
        .json()
        .await
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Decode error: {e}"), ""))?;

    if result.ok {
        println!("revoked");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// user subcommands
// ---------------------------------------------------------------------------

async fn run_user(cmd: UserCmd) -> Result<(), ExitError> {
    match cmd {
        UserCmd::List => user_list().await,
        UserCmd::Remove { id } => user_remove(id).await,
    }
}

async fn user_list() -> Result<(), ExitError> {
    let (relay_url, token) = load_admin_creds()?;
    let http = build_http_client()?;

    let resp = http
        .get(format!("{relay_url}/admin/users"))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Network error: {e}"), ""))?;

    if !resp.status().is_success() {
        return Err(http_err(resp).await);
    }

    let list: ListUsersResp = resp
        .json()
        .await
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Decode error: {e}"), ""))?;

    if list.users.is_empty() {
        eprintln!("No users found.");
        return Ok(());
    }

    // Compute column widths dynamically.
    const GUTTER: usize = 2;
    let id_w = list
        .users
        .iter()
        .map(|u| u.id.len())
        .max()
        .unwrap_or(0)
        .max("ID".len());
    let name_w = list
        .users
        .iter()
        .map(|u| u.display_name.len())
        .max()
        .unwrap_or(0)
        .max("DISPLAY NAME".len());
    let admin_w = "ADMIN".len();
    let created_w = list
        .users
        .iter()
        .map(|u| u.created_at.len())
        .max()
        .unwrap_or(0)
        .max("CREATED".len());

    let g = " ".repeat(GUTTER);
    println!(
        "  {:<iw$}{g}{:<nw$}{g}{:<aw$}{g}{:<cw$}",
        "ID",
        "DISPLAY NAME",
        "ADMIN",
        "CREATED",
        iw = id_w,
        nw = name_w,
        aw = admin_w,
        cw = created_w,
    );

    for u in &list.users {
        let admin_str = if u.is_admin { "yes" } else { "no" };
        println!(
            "  {:<iw$}{g}{:<nw$}{g}{:<aw$}{g}{:<cw$}",
            u.id,
            u.display_name,
            admin_str,
            u.created_at,
            iw = id_w,
            nw = name_w,
            aw = admin_w,
            cw = created_w,
        );
    }

    Ok(())
}

async fn user_remove(id: String) -> Result<(), ExitError> {
    let (relay_url, token) = load_admin_creds()?;
    let http = build_http_client()?;

    let resp = http
        .delete(format!("{relay_url}/admin/users/{id}"))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Network error: {e}"), ""))?;

    if !resp.status().is_success() {
        return Err(http_err(resp).await);
    }

    let result: OkResp = resp
        .json()
        .await
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Decode error: {e}"), ""))?;

    if result.ok {
        println!("removed {id}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(clap::Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: AdminCmd,
    }

    #[test]
    fn test_invite_create_defaults_parse() {
        let cli =
            TestCli::try_parse_from(["test", "invite", "create"]).expect("parse invite create");
        assert!(matches!(
            cli.cmd,
            AdminCmd::Invite(InviteCmd::Create { .. })
        ));
        if let AdminCmd::Invite(InviteCmd::Create {
            label,
            uses,
            expires_days,
        }) = cli.cmd
        {
            assert!(label.is_none());
            assert_eq!(uses, 1);
            assert_eq!(expires_days, 7);
        }
    }

    #[test]
    fn test_invite_create_with_options_parses() {
        let cli = TestCli::try_parse_from([
            "test",
            "invite",
            "create",
            "--label",
            "beta",
            "--uses",
            "5",
            "--expires-days",
            "30",
        ])
        .expect("parse invite create with options");
        if let AdminCmd::Invite(InviteCmd::Create {
            label,
            uses,
            expires_days,
        }) = cli.cmd
        {
            assert_eq!(label.as_deref(), Some("beta"));
            assert_eq!(uses, 5);
            assert_eq!(expires_days, 30);
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn test_invite_list_parses() {
        let cli = TestCli::try_parse_from(["test", "invite", "list"]).expect("parse invite list");
        assert!(matches!(cli.cmd, AdminCmd::Invite(InviteCmd::List)));
    }

    #[test]
    fn test_invite_revoke_parses() {
        let cli = TestCli::try_parse_from(["test", "invite", "revoke", "deadbeef1234"])
            .expect("parse invite revoke");
        if let AdminCmd::Invite(InviteCmd::Revoke { hash }) = cli.cmd {
            assert_eq!(hash, "deadbeef1234");
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn test_invite_revoke_requires_hash() {
        let result = TestCli::try_parse_from(["test", "invite", "revoke"]);
        assert!(
            result.is_err(),
            "expected parse failure when hash is omitted"
        );
    }

    #[test]
    fn test_user_list_parses() {
        let cli = TestCli::try_parse_from(["test", "user", "list"]).expect("parse user list");
        assert!(matches!(cli.cmd, AdminCmd::User(UserCmd::List)));
    }

    #[test]
    fn test_user_remove_parses() {
        let cli = TestCli::try_parse_from(["test", "user", "remove", "01J..."])
            .expect("parse user remove");
        if let AdminCmd::User(UserCmd::Remove { id }) = cli.cmd {
            assert_eq!(id, "01J...");
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn test_user_remove_requires_id() {
        let result = TestCli::try_parse_from(["test", "user", "remove"]);
        assert!(result.is_err(), "expected parse failure when id is omitted");
    }
}
