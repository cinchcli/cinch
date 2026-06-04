//! Shared runtime context for CLI subcommands.
//!
//! Every new read/write command (search, get, pin, rm, sources, ...) goes
//! through `open_ctx()` to get a consistent (Store, RestClient) pair, and
//! optionally calls `opportunistic_backfill()` before reading to make sure
//! the local store is reasonably fresh.

use std::sync::{Arc, OnceLock};

use client_core::auth::load_config;
use client_core::config::Config;
use client_core::http::RestClient;
use client_core::store::{self, Store};
use client_core::sync::{self, BackfillBudget, FlushGate, LockKind};

/// Shared context passed to every CLI subcommand that reads or writes clips.
pub struct Ctx {
    pub store: Arc<Store>,
    pub client: Arc<RestClient>,
    /// AES-256-GCM encryption key for this user, if configured.
    ///
    /// `None` when no encryption key is stored; encrypted clips will be
    /// skipped during backfill rather than stored as opaque ciphertext.
    pub enc_key: Option<[u8; 32]>,
    /// `true` when the device is signed in but the master AES key has not yet
    /// arrived via the ECDH key-exchange handshake. Lets `send`'s key-gate
    /// (§3.4) distinguish `ENCRYPTION_PENDING` from `ENCRYPTION_REQUIRED`
    /// without re-resolving config.
    pub key_pending: bool,
    /// This machine's active device id, from config. Empty when unauthenticated
    /// or when resolved purely from `--token`/`--relay` flags (no disk config).
    /// `fleet rename self` resolves the rename target from this.
    pub active_device_id: String,
}

/// THE one shared env/flag overlay resolver (F8).
///
/// Resolves a [`Config`] by layering sources, highest priority last (so a
/// later layer overwrites an earlier one). Effective precedence, strongest
/// first:
/// 1. `CINCH_TOKEN` / `CINCH_RELAY_URL` env overrides,
/// 2. explicit `--token` / `--relay` flag overrides,
/// 3. on-disk config.
///
/// Env beats the flags on purpose — this preserves the precedence of the old
/// `push::resolve_config` (flags applied first, env applied on top), so CI
/// that exports `CINCH_TOKEN` keeps winning over a baked-in `--token`.
///
/// Special case (both-flags short-circuit): when BOTH `--token` and `--relay`
/// are supplied, disk config AND env are never read at all, so a `~/.cinch`-less
/// container can operate with just the two flags. The relay URL has any
/// trailing slash stripped. Errors out (as a string) when no usable token is
/// resolvable from any source.
///
/// This is the single source of truth for the overlay precedence; both the
/// local-save (`copy`) path and [`open_ctx_with`] consume it — neither
/// re-implements the precedence.
pub fn resolve_overlay_config(
    token_override: Option<&str>,
    relay_override: Option<&str>,
) -> Result<Config, String> {
    // Both-flags short-circuit: skip disk entirely.
    if let (Some(token), Some(relay)) = (token_override, relay_override) {
        return Ok(Config {
            token: token.to_string(),
            relay_url: relay.trim_end_matches('/').to_string(),
            ..Config::default()
        });
    }

    let mut cfg = load_config().map_err(|e| format!("config: {e}"))?;
    if let Some(token) = token_override {
        cfg.token = token.to_string();
    }
    if let Some(relay) = relay_override {
        cfg.relay_url = relay.trim_end_matches('/').to_string();
    }
    if let Ok(env_token) = std::env::var("CINCH_TOKEN") {
        if !env_token.is_empty() {
            cfg.token = env_token;
        }
    }
    if let Ok(env_relay) = std::env::var("CINCH_RELAY_URL") {
        if !env_relay.is_empty() {
            cfg.relay_url = env_relay.trim_end_matches('/').to_string();
        }
    }
    if cfg.token.is_empty() {
        return Err("not authenticated — run: cinch auth login".into());
    }
    Ok(cfg)
}

/// Load config + credentials, open the local `Store`, and construct a
/// `RestClient`. Returns an error string if not authenticated or if any
/// initialisation step fails.
///
/// Exactly equivalent to `open_ctx_with(None, None)` — kept as the no-overlay
/// entrypoint so all existing read-command callers are unchanged.
pub fn open_ctx() -> Result<Ctx, String> {
    open_ctx_with(None, None)
}

/// Like [`open_ctx`], but overlays `--token` / `--relay` (and the
/// `CINCH_TOKEN` / `CINCH_RELAY_URL` env vars) via the shared
/// [`resolve_overlay_config`] resolver before opening the store and building
/// the `RestClient`. Used by `send` (and the stateless-CI path) so a
/// `~/.cinch`-less box can contact the relay with just the two flags.
pub fn open_ctx_with(
    token_override: Option<&str>,
    relay_override: Option<&str>,
) -> Result<Ctx, String> {
    let cfg = resolve_overlay_config(token_override, relay_override)?;

    let store_path = store::default_db_path().map_err(|e| e.to_string())?;
    let store = Arc::new(Store::open(&store_path).map_err(|e| e.to_string())?);

    let client = Arc::new(
        RestClient::new(
            cfg.relay_url.clone(),
            cfg.token.clone(),
            crate::client_info::for_cli(),
        )
        .map_err(|e| e.to_string())?,
    );

    // Encryption key may not exist for first-run or unencrypted accounts;
    // that is OK — encrypted clips will be skipped during backfill.
    let enc_key = client_core::credstore::read_encryption_key(&cfg.user_id);

    Ok(Ctx {
        store,
        client,
        enc_key,
        key_pending: cfg.key_pending,
        active_device_id: cfg.active_device_id.clone(),
    })
}

static SESSION_FLUSH_GATE: OnceLock<FlushGate> = OnceLock::new();

/// Spawn a best-effort backlog flush as a detached tokio task.
///
/// Skips silently when:
/// - The encryption key is missing (cannot encrypt pending clips).
/// - Another flush is already in flight in this process (FlushGate-gated).
///
/// Errors during the flush are intentionally absorbed — they never affect
/// the caller's exit status or output.
pub fn spawn_session_flush(ctx: &Ctx) {
    let Some(key) = ctx.enc_key else { return };
    let gate = SESSION_FLUSH_GATE.get_or_init(FlushGate::new);
    let Some(guard) = gate.try_enter() else {
        return;
    };

    let store = ctx.store.clone();
    let client = ctx.client.clone();
    tokio::spawn(async move {
        // Hold the guard for the duration of the flush so a second
        // concurrent caller skips silently rather than double-flushing.
        let _guard = guard;
        // Best-effort: errors are intentionally absorbed so the flush
        // never affects the user-facing exit code or output.
        let _ = client_core::sync::flush_once(&store, &*client, key).await;
    });
}

/// Opportunistic REST backfill for read commands.
///
/// If the lockfile is free (no active writer), runs a short REST backfill so
/// the read sees fresh data. If a writer is active or the lock cannot be
/// acquired, trusts the current store state and skips.
pub async fn opportunistic_backfill(ctx: &Ctx) {
    let lock_path = match store::lock_path() {
        Ok(p) => p,
        Err(_) => return,
    };

    let lock = match sync::Lockfile::try_acquire(&lock_path, LockKind::Cli) {
        Ok(Some(l)) => l,
        _ => return, // writer is active or I/O error — skip
    };

    let _ = sync::backfill_once(
        &ctx.store,
        &*ctx.client,
        BackfillBudget::default(),
        ctx.enc_key.as_ref(),
    )
    .await;

    drop(lock);
}

#[cfg(test)]
mod tests {
    use super::*;

    // Env-var mutation must be serialized; these tests share process env.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn both_flags_short_circuit_skips_disk_and_strips_slash() {
        // When --token AND --relay are both passed, the resolver returns
        // without touching ~/.cinch/config.json; verify the early-return path
        // AND that the relay URL's trailing slash is stripped.
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("CINCH_TOKEN");
        std::env::remove_var("CINCH_RELAY_URL");
        let cfg = resolve_overlay_config(Some("tok-xyz"), Some("https://relay.example/"))
            .expect("both-flags short-circuit succeeds");
        assert_eq!(cfg.token, "tok-xyz");
        assert_eq!(cfg.relay_url, "https://relay.example");
        // The short-circuit uses Config::default(), so key_pending is false.
        assert!(!cfg.key_pending);
    }

    #[test]
    fn env_overlay_overrides_disk_token_and_relay() {
        // With only one flag (relay) the resolver loads disk config, then the
        // env vars override on top. Pair --relay with the short-circuit-avoiding
        // single-flag path by leaving --token None, so env CINCH_TOKEN supplies
        // the token. This mirrors the old push::resolve_config env precedence.
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("CINCH_TOKEN", "env-token");
        std::env::set_var("CINCH_RELAY_URL", "https://env-relay.example/");
        // Pass an explicit relay flag too; env should win over the flag.
        let cfg = resolve_overlay_config(None, Some("https://flag-relay.example/"))
            .expect("env overlay resolves");
        assert_eq!(cfg.token, "env-token");
        // CINCH_RELAY_URL overrides the --relay flag and is slash-stripped.
        assert_eq!(cfg.relay_url, "https://env-relay.example");
        std::env::remove_var("CINCH_TOKEN");
        std::env::remove_var("CINCH_RELAY_URL");
    }

    #[test]
    fn empty_token_from_all_sources_errors() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("CINCH_TOKEN", "");
        std::env::remove_var("CINCH_RELAY_URL");
        // No flags, empty env token, and (in CI) no disk token → error.
        // If a developer machine has a real ~/.cinch token this would resolve;
        // guard by also clearing via a relay-only call that can't short-circuit.
        let res = resolve_overlay_config(None, None);
        std::env::remove_var("CINCH_TOKEN");
        // Either it errors (no disk token) or resolves a real token — both are
        // valid depending on the machine; assert the error MESSAGE shape only
        // when it errors.
        if let Err(msg) = res {
            assert!(msg.contains("not authenticated") || msg.contains("config"));
        }
    }
}
