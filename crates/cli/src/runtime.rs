//! Shared runtime context for CLI subcommands.
//!
//! Every new read/write command (search, get, pin, rm, sources, ...) goes
//! through `open_ctx()` to get a consistent (Store, RestClient) pair, and
//! optionally calls `opportunistic_backfill()` before reading to make sure
//! the local store is reasonably fresh.

use std::sync::{Arc, OnceLock};

use client_core::auth::load_config;
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
}

/// Load config + credentials, open the local `Store`, and construct a
/// `RestClient`. Returns an error string if not authenticated or if any
/// initialisation step fails.
pub fn open_ctx() -> Result<Ctx, String> {
    let cfg = load_config().map_err(|e| format!("config: {e}"))?;
    if cfg.token.is_empty() {
        return Err("not authenticated — run: cinch auth login".into());
    }

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
